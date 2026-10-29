//! Durable ten-round renewal admission. A PREPARE receipt cannot admit an
//! allocation, and an admitted allocation cannot bypass all-hop payment gates.
use super::*;

impl ClientFoundation {
    pub(super) fn next_renewal_period(&self, round: u64) -> Result<Option<u64>> {
        if round == 0 || (round - 1) % 10 + 1 < 7 {
            return Ok(None);
        }
        let period = (round - 1) / 10 + 1;
        for hop in &self.plan.hops {
            let policy = FoundationPolicy::decode(&hop.descriptor.terms)?;
            if period >= u64::from(policy.max_periods)
                || period
                    .checked_add(1)
                    .and_then(|p| p.checked_mul(600_000))
                    .map_or(true, |end| end > policy.max_age_ms)
            {
                return Ok(None);
            }
        }
        Ok(Some(period))
    }
    pub(super) fn renewal_operation(&self, hop: usize, period: u64) -> Result<RenewalOperation> {
        if period == 0 {
            return Err(invalid());
        }
        let prior = self
            .ledger
            .get_origin_allocation(
                self.plan.local_scope(),
                self.plan.charge_reference(hop),
                period - 1,
            )?
            .ok_or("Missing retained renewal parent")?;
        let registration = &self.plan.hops[hop].registration;
        if prior.circuit_binding != registration.circuit_binding()
            || prior.policy_hash != registration.policy_hash()
            || prior.initial_allocation != registration.initial_allocation()
        {
            return Err(invalid());
        }
        let ids = std::array::from_fn(|slot| {
            digest(
                b"eltor/origin/v1/renewal-payment-id",
                &[
                    &self.plan.seed,
                    &(hop as u16 + 1).to_be_bytes(),
                    &period.to_be_bytes(),
                    &(slot as u16).to_be_bytes(),
                ],
            )
        });
        let op = RenewalOperation {
            binding: self.plan.binding.clone(),
            hop: hop as u16 + 1,
            operation: self
                .plan
                .identity(hop, b"eltor/origin/v1/renewal-operation", period),
            nonce: self
                .plan
                .identity(hop, b"eltor/origin/v1/renewal-nonce", period),
            period,
            first_round: period
                .checked_mul(10)
                .and_then(|r| r.checked_add(1))
                .ok_or_else(invalid)?,
            terms: registration.policy_hash(),
            parent: prior.allocation_digest,
            ids,
        };
        op.validate()?;
        Ok(op)
    }
    fn require_renewal_dispatch_window(&self, period: u64) -> Result<()> {
        let (clock, at) = self.last_clock.ok_or_else(invalid)?;
        let now = latest(clock)?
            .checked_add(i64::try_from(at.elapsed().as_millis())?)
            .ok_or_else(invalid)?;
        let cutoff = earliest(self.plan.construction)?
            .checked_add(i64::try_from(
                period.checked_mul(600_000).ok_or_else(invalid)?,
            )?)
            .and_then(|n| n.checked_sub(60_000))
            .ok_or_else(invalid)?;
        // Reserve PaymentControl's complete 15-second exchange deadline. The
        // native relay independently enforces its actual original T0 cutoff.
        if now.checked_add(15_000).map_or(true, |n| n >= cutoff) {
            return Err("Original renewal dispatch deadline expired".into());
        }
        Ok(())
    }
    /// Called on a separate tick from INITIAL_GET so a native per-hop query
    /// token is never consumed twice in one reconciliation. Exact journaled
    /// claims survive a dropped socket; a claimed PREPARE is only queried.
    pub(super) async fn reconcile_renewal(&mut self, period: u64) -> Result<bool> {
        for hop in 0..self.plan.hops.len() {
            if self
                .ledger
                .get_origin_allocation(
                    self.plan.local_scope(),
                    self.plan.charge_reference(hop),
                    period,
                )?
                .is_some()
            {
                continue;
            }
            let op = self.renewal_operation(hop, period)?;
            let saved = match self.ledger.get_origin_renewal_operation(
                op.binding.instance,
                op.binding.handle,
                op.hop,
                op.operation,
            )? {
                Some(saved) => saved,
                None => {
                    self.require_renewal_dispatch_window(period)?;
                    self.ledger.save_origin_renewal_operation(&op)?
                }
            };
            if saved.operation != op {
                return Err(invalid());
            }
            if !saved.dispatch_claimed {
                self.require_renewal_dispatch_window(period)?;
                let claimed = self.ledger.claim_origin_renewal_dispatch(
                    op.binding.instance,
                    op.binding.handle,
                    op.hop,
                    op.operation,
                )?;
                if claimed.operation != op {
                    return Err(invalid());
                }
                self.control.renewal_prepare(&op).await?;
                continue;
            }
            let mut snapshot = match self
                .control
                .renewal_snapshot(&op.binding, op.hop, Some(op.operation))
                .await
            {
                Ok(snapshot) => snapshot,
                Err(error)
                    if PaymentRejection::from_io(&error)
                        == Some(PaymentRejection::StateTransitionPending) =>
                {
                    // GET remains read-only. Only a fresh separately persisted
                    // explicit QUERY asks Tor to obtain coherent status clocks.
                    self.queue_status_query(hop).await?;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if let Some(previous) = &self.renewal_observations[hop] {
                if previous.operation().map(|s| s.operation) == Some(op.operation) {
                    snapshot.constrain_with_previous(previous)?;
                }
            }
            let status = snapshot.operation().ok_or_else(invalid)?;
            match status.phase {
                RenewalOperationPhase::Admitted | RenewalOperationPhase::Active => {
                    let proof =
                        snapshot.verify_admission(&op, &self.plan.hops[hop].registration)?;
                    let budget = proof.schedule();
                    // A successful immutable admission can be recovered after
                    // its admission cutoff; its success does not create a new
                    // deadline or permit a new PREPARE/payment outside windows.
                    if budget.hard_age_remaining_ms <= 1000 {
                        return Err("Renewal admission deadline expired".into());
                    }
                    self.ledger.admit_origin_renewal(
                        self.plan.local_scope(),
                        self.plan.charge_reference(hop),
                        &proof,
                    )?;
                }
                RenewalOperationPhase::Queued
                | RenewalOperationPhase::Sent
                | RenewalOperationPhase::Uncertain => (),
                RenewalOperationPhase::Refused | RenewalOperationPhase::Closed => {
                    return Err(
                        "Renewal refused or closed; no later-period funding permitted".into(),
                    )
                }
            }
            self.renewal_observations[hop] = Some(snapshot);
        }
        Ok(self
            .ledger
            .origin_period_admitted(self.plan.local_scope(), period)?)
    }
    pub(super) fn payment_id(&self, hop: usize, round: u64) -> Result<[u8; 32]> {
        if round == 0 {
            return Err(invalid());
        }
        let period = (round - 1) / 10;
        if !self
            .ledger
            .origin_period_admitted(self.plan.local_scope(), period)?
        {
            return Err("Every hop must admit this exact allocation before funding".into());
        }
        let allocation = self
            .ledger
            .get_origin_allocation(
                self.plan.local_scope(),
                self.plan.charge_reference(hop),
                period,
            )?
            .ok_or_else(invalid)?;
        let reg = &self.plan.hops[hop].registration;
        if allocation.circuit_binding != reg.circuit_binding()
            || allocation.policy_hash != reg.policy_hash()
            || allocation.initial_allocation != reg.initial_allocation()
        {
            return Err(invalid());
        }
        Ok(allocation.payment_ids[((round - 1) % 10) as usize])
    }
    pub(super) async fn queue_status_query(&mut self, hop: usize) -> Result<()> {
        let next = self.query_indices[hop].checked_add(1).ok_or_else(invalid)?;
        if next > MAX_STATUS_QUERIES_PER_HOP {
            return Err("Bounded origin observation budget exhausted".into());
        }
        self.query_indices[hop] = next;
        let op = self.operation(hop, InitialOperationKind::Query, next);
        match self
            .control
            .durable_initial_operation(&mut self.ledger, &op)
            .await?
        {
            DurableInitialResult::Queued { .. } => Ok(()),
            DurableInitialResult::Recovered(_) => Err(invalid()),
        }
    }
}
