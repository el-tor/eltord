//! Origin foundation coordinator. A bounded control receipt is never a payment
//! permit. Accepted all-hop metadata, original operation identities and native
//! invoice artifacts live in the same authoritative account ledger as spending.
//! This path never invokes the legacy wallet/payment loop or provisions storage.
use crate::{
    payment_ledger::{
        outgoing::*,
        wallet::{self, EvidenceStatus},
        *,
    },
    rpc::*,
};
use lni::cln::foundation::{ClnFoundation, InvoiceExpectation, Observation};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, convert::TryFrom, error::Error, sync::Arc, time::Duration};
use tokio::time::Instant;

#[path = "foundation_plan.rs"]
mod plan;
pub use plan::{FoundationDraft, FoundationHop, FoundationLimits, FoundationPlan};
#[path = "foundation_recovery.rs"]
mod financial_recovery;
pub use financial_recovery::{reconcile_origin_financial, OriginFinancialRecoveryPage};
#[path = "foundation_renewal.rs"]
mod renewal;
const MAX_STATUS_QUERIES_PER_HOP: u64 = 256;
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn invalid() -> Box<dyn Error> {
    "Invalid or unsafe foundation origin state".into()
}
fn initial_observation(
    result: Result<DurableInitialResult>,
) -> Result<Option<DurableInitialResult>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .and_then(PaymentRejection::from_io)
                == Some(PaymentRejection::StateTransitionPending) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
fn digest(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update((domain.len() as u16).to_be_bytes());
    h.update(domain);
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}
fn validate_clock(clock: ClockObservation) -> Result<()> {
    if clock.now_ms < 0
        || clock.uncertainty_ms > 5000
        || clock
            .now_ms
            .checked_add(i64::try_from(clock.uncertainty_ms)?)
            .is_none()
    {
        return Err(invalid());
    }
    Ok(())
}
fn latest(clock: ClockObservation) -> Result<i64> {
    validate_clock(clock)?;
    clock
        .now_ms
        .checked_add(i64::try_from(clock.uncertainty_ms)?)
        .ok_or_else(invalid)
}
fn earliest(clock: ClockObservation) -> Result<i64> {
    validate_clock(clock)?;
    clock
        .now_ms
        .checked_sub(i64::try_from(clock.uncertainty_ms)?)
        .filter(|n| *n >= 0)
        .ok_or_else(invalid)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoundationProgress {
    Recovered,
    Deferred,
    AwaitingCommitment,
    AwaitingPayment {
        hop: u16,
        round: u64,
    },
    AwaitingActivation,
    AwaitingRenewal {
        period: u64,
    },
    Active {
        current_round: u64,
        renewal_required: bool,
    },
}
struct ObservedHop {
    snapshot: InitialOriginSnapshot,
    at: Instant,
}
/// Own one accepted path. Multiple paths may open the same authoritative ledger;
/// SQLite owns shared limits/dispatch claims. Every reconnect performs inventory
/// recovery and retains the exact instance/handle; it never rebinds a numeric ID.
pub struct ClientFoundation {
    ledger: PaymentLedger,
    control: PaymentControl,
    wallet: Option<Arc<ClnFoundation>>,
    plan: FoundationPlan,
    recovery: Option<PaymentRecovery>,
    next_poll: Instant,
    query_indices: Vec<u64>,
    observations: Vec<Option<ObservedHop>>,
    funded: BTreeSet<(usize, u64)>,
    last_clock: Option<(ClockObservation, Instant)>,
    start_observed: Vec<bool>,
    commit_open_ms: Option<i64>,
    pending_renewal_period: Option<u64>,
    renewal_observations: Vec<Option<RenewalOriginSnapshot>>,
}
impl ClientFoundation {
    pub fn new(
        mut ledger: PaymentLedger,
        control: PaymentControl,
        wallet: Option<Arc<ClnFoundation>>,
        plan: FoundationPlan,
    ) -> Result<Self> {
        if ledger.account_id() != plan.account
            || wallet.as_ref().map_or(false, |w| {
                AccountId(w.account().fingerprint()) != plan.account
            })
        {
            return Err(invalid());
        }
        if plan.hops.iter().any(|h| {
            FoundationPolicy::decode(&h.descriptor.terms)
                .map_or(true, |p| p.rate_msat > 0 || p.handshake_fee_msat > 0)
        }) && wallet.is_none()
        {
            return Err(
                "Positive-price foundation path requires the verified native wallet".into(),
            );
        }
        // Upfront fees were independently accepted and paid before the
        // immutable constructor. Only the authoritative bound prefunding rows
        // can satisfy that requirement; no local receipt is a fee proof.
        if plan.hops.iter().any(|h| {
            FoundationPolicy::decode(&h.descriptor.terms).map_or(true, |p| p.handshake_fee_msat > 0)
        }) {
            let scope = ledger
                .get_handshake_scope(plan.local_scope())?
                .ok_or("Missing durable handshake prefunding")?;
            if scope.account != plan.account
                || scope.instance != plan.binding.instance
                || ledger.outgoing_admission_blocked()?
            {
                return Err(invalid());
            }
            plan.configure(&mut ledger)?; // Exact native registration/caps/receiver binding.
            for (index, hop) in plan.hops.iter().enumerate() {
                if FoundationPolicy::decode(&hop.descriptor.terms)?.handshake_fee_msat == 0 {
                    continue;
                }
                let record = ledger
                    .get_handshake(plan.local_scope(), index as u16 + 1)?
                    .ok_or("Missing settled handshake prefunding")?;
                if record.closed
                    || record.fee_limit_breached
                    || !matches!(
                        record.state,
                        OutgoingState::Settled | OutgoingState::Consumed
                    )
                {
                    return Err(invalid());
                }
            }
        }
        let count = plan.hops.len();
        let mut query_indices = vec![0; count];
        for (hop, index) in query_indices.iter_mut().enumerate() {
            // A bounded supervisor retains at most 256 queries/hop. Exact prior
            // identities survive restart; no counter reset grants new slots.
            for sequence in 1..=MAX_STATUS_QUERIES_PER_HOP {
                let id = plan.identity(hop, b"eltor/origin/v1/query-operation", sequence);
                if ledger
                    .origin_operation(
                        plan.binding.instance,
                        plan.binding.handle,
                        hop as u16 + 1,
                        id,
                    )?
                    .is_some()
                {
                    *index = sequence;
                }
            }
        }
        Ok(Self {
            ledger,
            control,
            wallet,
            plan,
            recovery: None,
            next_poll: Instant::now(),
            query_indices,
            observations: (0..count).map(|_| None).collect(),
            start_observed: vec![false; count],
            commit_open_ms: None,
            pending_renewal_period: None,
            renewal_observations: (0..count).map(|_| None).collect(),
            funded: BTreeSet::new(),
            last_clock: None,
        })
    }
    pub fn binding(&self) -> &OriginBinding {
        &self.plan.binding
    }
    pub fn ledger(&self) -> &PaymentLedger {
        &self.ledger
    }
    pub async fn reconcile_financial<F>(
        &mut self,
        after: Option<Id16>,
        limit: u64,
        observe: F,
    ) -> Result<OriginFinancialRecoveryPage>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        let wallet = self.wallet.as_ref().ok_or_else(invalid)?.clone();
        reconcile_origin_financial(&mut self.ledger, &wallet, after, limit, observe).await
    }
    pub async fn reconcile_handshake_financial<F>(
        &mut self,
        after: Option<Id16>,
        limit: u64,
        observe: F,
    ) -> Result<crate::client::handshake::HandshakeFinancialRecoveryPage>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        let wallet = self.wallet.as_ref().ok_or_else(invalid)?.clone();
        crate::client::handshake::reconcile_handshake_financial(
            &mut self.ledger,
            &wallet,
            after,
            limit,
            observe,
        )
        .await
    }
    pub fn reconnect(&mut self, control: PaymentControl) {
        self.control = control;
        self.recovery = None;
        self.next_poll = Instant::now();
    }
    pub fn close(&mut self) -> Result<()> {
        self.ledger
            .close_binding(self.plan.binding.instance, self.plan.binding.handle)?;
        self.ledger.close_outgoing_path(self.plan.local_scope())?;
        self.recovery = None;
        Ok(())
    }
    fn clock<F>(&mut self, observe: &mut F) -> Result<ClockObservation>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        let now = observe()?;
        validate_clock(now)?;
        let mono = Instant::now();
        if let Some((old, at)) = self.last_clock {
            let elapsed = i64::try_from(mono.duration_since(at).as_millis())?;
            let lower = earliest(now)?
                .checked_sub(latest(old)?)
                .ok_or_else(invalid)?;
            let upper = latest(now)?
                .checked_sub(earliest(old)?)
                .ok_or_else(invalid)?;
            // Bracket the real monotonic interval; scheduler tolerance never
            // extends a stored payment deadline or an invoice validity window.
            if elapsed.saturating_add(1000) < lower || elapsed.saturating_sub(1000) > upper {
                return Err("Foundation clock discontinuity".into());
            }
        }
        self.last_clock = Some((now, mono));
        Ok(now)
    }
    async fn recover(&mut self) -> Result<()> {
        let recovery = self.control.recover().await?;
        if recovery.instance() != self.plan.binding.instance {
            self.ledger.close_instance(self.plan.binding.instance)?;
            return Err("Original foundation Tor instance is gone".into());
        }
        if !recovery
            .tracked_rows_for(PaymentRole::Origin)?
            .iter()
            .any(|r| r.handle == self.plan.binding.handle)
        {
            self.close()?;
            return Err("Original foundation circuit is gone".into());
        }
        self.plan.configure(&mut self.ledger)?;
        self.recovery = Some(recovery);
        Ok(())
    }
    /// One bounded path reconciliation. The supervisor owns sleep/reconnect and
    /// transport closure. An error stops new spending; uncertain native outcomes
    /// remain reserved and can only be looked up using their original artifact.
    pub async fn step<F>(&mut self, mut observe: F) -> Result<FoundationProgress>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        if Instant::now() < self.next_poll {
            return Ok(FoundationProgress::Deferred);
        }
        self.next_poll = Instant::now() + Duration::from_secs(1);
        self.clock(&mut observe)?;
        if self.recovery.is_none() {
            self.recover().await?;
            return Ok(FoundationProgress::Recovered);
        }
        for event in self.control.take_events()? {
            if self.recovery.as_mut().unwrap().observe(&event).is_err() {
                self.recovery = None;
                return Err("Foundation inventory resynchronization required".into());
            }
        }
        let mut started = false;
        for hop in 0..self.plan.hops.len() {
            let op = self.operation(hop, InitialOperationKind::Start, 0);
            started |= self
                .ledger
                .origin_operation(op.binding.instance, op.binding.handle, op.hop, op.operation)?
                .is_some();
        }
        if !started {
            if !self.commit_all(&mut observe).await? {
                return Ok(FoundationProgress::AwaitingCommitment);
            }
            if let Some(hop) = self.fund_round(1, &mut observe).await? {
                return Ok(FoundationProgress::AwaitingPayment { hop, round: 1 });
            }
        }
        if started {
            if let Some(hop) = self.reconcile_existing_round(1, &mut observe).await? {
                return Ok(FoundationProgress::AwaitingPayment { hop, round: 1 });
            }
        }
        if !self.ensure_start().await? {
            return Ok(FoundationProgress::AwaitingActivation);
        }
        if let Some(period) = self.pending_renewal_period.take() {
            self.reconcile_renewal(period).await?;
            return Ok(FoundationProgress::AwaitingRenewal { period });
        }
        let active = self.observe_active(&mut observe).await?;
        let Some(round) = active else {
            return Ok(FoundationProgress::AwaitingActivation);
        };
        let retained_round = self.ledger.spending_round(self.plan.local_scope())?;
        if round < retained_round || round.saturating_sub(retained_round) > 10 {
            return Err("Origin progress exceeds bounded reconciliation interval".into());
        }
        // A crash after native settlement but before observing activation loses
        // only this process's funded cache. Recheck exact retained artifacts by
        // lookup; a Settled row alone cannot attest its original funding window.
        for obligation in retained_round..=round {
            if let Some(hop) = self
                .reconcile_existing_round(obligation, &mut observe)
                .await?
            {
                return Ok(FoundationProgress::AwaitingPayment {
                    hop,
                    round: obligation,
                });
            }
        }
        self.consume_through(round)?;
        let mut ledger_round = self.ledger.spending_round(self.plan.local_scope())?;
        if round < ledger_round {
            return Err(invalid());
        }
        while ledger_round < round {
            ledger_round += 1;
            self.ledger
                .advance_spending_round(self.plan.local_scope(), ledger_round)?;
        }
        let next_period = self.next_renewal_period(round)?;
        let renewal_required = if let Some(period) = next_period {
            let required = !self
                .ledger
                .origin_period_admitted(self.plan.local_scope(), period)?;
            if required {
                self.pending_renewal_period = Some(period);
            }
            required
        } else {
            false
        };
        let next_round = round.checked_add(1).ok_or_else(invalid)?;
        if self
            .ledger
            .origin_period_admitted(self.plan.local_scope(), (next_round - 1) / 10)?
        {
            let now = self.clock(&mut observe)?;
            let window = self.window(0, round + 1, now)?;
            if earliest(now)? >= window.opens_at_ms {
                if let Some(hop) = self.fund_round(round + 1, &mut observe).await? {
                    return Ok(FoundationProgress::AwaitingPayment {
                        hop,
                        round: round + 1,
                    });
                }
            }
        }
        self.next_poll =
            Instant::now() + Duration::from_secs(if renewal_required { 1 } else { 15 });
        Ok(FoundationProgress::Active {
            current_round: round,
            renewal_required,
        })
    }
    async fn ensure_start(&mut self) -> Result<bool> {
        let mut observed = true;
        for hop in 0..self.plan.hops.len() {
            if !self.round_funded(hop, 1)? {
                return Err("START cannot precede every hop's funded round".into());
            }
            if self.start_observed[hop] {
                continue;
            }
            observed = false;
            let op = self.operation(hop, InitialOperationKind::Start, 0);
            let outcome = initial_observation(
                self.control
                    .durable_initial_operation(&mut self.ledger, &op)
                    .await,
            )?;
            let Some(outcome) = outcome else {
                self.queue_status_query(hop).await?;
                continue;
            };
            match outcome {
                DurableInitialResult::Queued { .. } => (),
                DurableInitialResult::Recovered(snapshot) => {
                    self.check_snapshot(hop, &snapshot)?;
                    match snapshot.operation.as_ref().map(|s| s.phase) {
                        Some(InitialOperationPhase::Verified) => self.start_observed[hop] = true,
                        Some(
                            InitialOperationPhase::Queued
                            | InitialOperationPhase::Sent
                            | InitialOperationPhase::Uncertain,
                        ) => (),
                        _ => return Err("Initial START refused or closed".into()),
                    }
                }
            }
        }
        // A START GET and QUERY GET may not consume the same per-hop query
        // interval. Wait for the next supervisor tick before observing again.
        Ok(observed)
    }
    async fn reconcile_existing_round<F>(
        &mut self,
        round: u64,
        observe: &mut F,
    ) -> Result<Option<u16>>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        for hop in 0..self.plan.hops.len() {
            if self.round_funded(hop, round)? {
                continue;
            }
            let operation = self.round_operation(hop, round);
            let saved = self
                .ledger
                .get_origin_invoice(operation)?
                .ok_or("Observed activation has no durable payment artifact")?;
            self.validate_intent(hop, round, &saved.intent)?;
            let record = self.ledger.get_outgoing(operation)?.ok_or_else(invalid)?;
            if record.state == OutgoingState::Prepared {
                return Err("Observed activation preceded payment dispatch".into());
            }
            let clock = self.clock(observe)?;
            let wallet = self.wallet.as_ref().ok_or_else(invalid)?.clone();
            let invoice = wallet.restore_for_lookup(
                &saved.artifact,
                saved.intent.invoice_digest,
                InvoiceExpectation {
                    amount_msat: saved.intent.amount_msat,
                    expected_payee: None,
                    expected_payment_hash: Some(saved.intent.payment_hash),
                },
            )?;
            let result =
                wallet::reconcile_cln(&mut self.ledger, &wallet, operation, &invoice, clock)
                    .await?;
            match result.evidence {
                EvidenceStatus::PaidWithinWindow if !result.record.fee_limit_breached => {
                    self.funded.insert((hop, round));
                }
                EvidenceStatus::Unresolved => return Ok(Some(hop as u16 + 1)),
                _ => return Err("Historical payment cannot fund this accepted path".into()),
            }
        }
        Ok(None)
    }
    fn operation(&self, hop: usize, kind: InitialOperationKind, sequence: u64) -> InitialOperation {
        let (op_domain, nonce_domain): (&[u8], &[u8]) = match kind {
            InitialOperationKind::Commit => (
                b"eltor/origin/v1/commit-operation",
                b"eltor/origin/v1/commit-nonce",
            ),
            InitialOperationKind::Start => (
                b"eltor/origin/v1/start-operation",
                b"eltor/origin/v1/start-nonce",
            ),
            InitialOperationKind::Query => (
                b"eltor/origin/v1/query-operation",
                b"eltor/origin/v1/query-nonce",
            ),
        };
        InitialOperation {
            binding: self.plan.binding.clone(),
            hop: hop as u16 + 1,
            operation: self.plan.identity(hop, op_domain, sequence),
            nonce: self.plan.identity(hop, nonce_domain, sequence),
            kind,
            terms: if kind == InitialOperationKind::Commit {
                Some(self.plan.hops[hop].descriptor.terms)
            } else {
                None
            },
        }
    }
    fn check_snapshot(&self, hop: usize, snapshot: &InitialOriginSnapshot) -> Result<()> {
        let reg = &self.plan.hops[hop].registration;
        if snapshot.terms_digest != Some(reg.policy_hash())
            || snapshot.initial_allocation != Some(reg.initial_allocation())
            || matches!(
                snapshot.state,
                InitialOriginState::Closed
                    | InitialOriginState::Expired
                    | InitialOriginState::Unconfirmed
            )
        {
            return Err(invalid());
        }
        if let Some(previous) = &self.observations[hop] {
            if snapshot
                .relay_revision
                .zip(previous.snapshot.relay_revision)
                .map_or(false, |(new, old)| new < old)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    fn retain_snapshot(
        &mut self,
        hop: usize,
        mut snapshot: InitialOriginSnapshot,
        request_at: Instant,
    ) -> Result<()> {
        self.check_snapshot(hop, &snapshot)?;
        let elapsed = u64::try_from(request_at.elapsed().as_millis())?.saturating_add(1);
        for index in [2, 3, 4] {
            snapshot.clocks[index] = snapshot.clocks[index].map(|ms| ms.saturating_sub(elapsed));
        }
        if let Some(previous) = &self.observations[hop] {
            if snapshot.clocks[0]
                .zip(previous.snapshot.clocks[0])
                .map_or(false, |(new, old)| new < old)
            {
                return Err(invalid());
            }
            let elapsed = u64::try_from(previous.at.elapsed().as_millis())?.saturating_add(1);
            for index in [2, 3, 4] {
                if index == 4 || snapshot.clocks[0] == previous.snapshot.clocks[0] {
                    if let Some(old) = previous.snapshot.clocks[index] {
                        snapshot.clocks[index] =
                            snapshot.clocks[index].map(|ms| ms.min(old.saturating_sub(elapsed)));
                    }
                }
            }
        }
        self.observations[hop] = Some(ObservedHop {
            snapshot,
            at: Instant::now(),
        });
        Ok(())
    }
    async fn commit_all<F>(&mut self, observe: &mut F) -> Result<bool>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        let mut all = true;
        let mut observed_latest = 0;
        for hop in 0..self.plan.hops.len() {
            let before = self.clock(observe)?;
            if latest(before)? >= self.plan.setup_deadline_ms(hop)? {
                return Err("Foundation setup expired before funding".into());
            }
            let op = self.operation(hop, InitialOperationKind::Commit, 0);
            let request_at = Instant::now();
            match self
                .control
                .durable_initial_operation(&mut self.ledger, &op)
                .await?
            {
                DurableInitialResult::Queued { .. } => all = false,
                DurableInitialResult::Recovered(snapshot) => {
                    self.check_snapshot(hop, &snapshot)?;
                    let after = self.clock(observe)?;
                    observed_latest = observed_latest.max(latest(after)?);
                    if latest(after)? >= self.plan.setup_deadline_ms(hop)?
                        || snapshot.clocks[4].unwrap_or(0) <= 1000
                    {
                        return Err("Insufficient original setup budget".into());
                    }
                    match snapshot.operation.as_ref().map(|o| o.phase) {
                        Some(InitialOperationPhase::Verified)
                            if snapshot.state == InitialOriginState::Committed =>
                        {
                            ()
                        }
                        Some(
                            InitialOperationPhase::Queued
                            | InitialOperationPhase::Sent
                            | InitialOperationPhase::Uncertain,
                        ) => all = false,
                        _ => return Err("Initial commitment refused or contradictory".into()),
                    }
                    self.retain_snapshot(hop, snapshot, request_at)?;
                }
            }
        }
        if all && self.commit_open_ms.is_none() {
            self.commit_open_ms = Some(observed_latest);
        }
        Ok(all)
    }
    fn round_operation(&self, hop: usize, round: u64) -> [u8; 16] {
        self.plan
            .identity(hop, b"eltor/origin/v1/round-payment", round)
    }
    fn round_funded(&self, hop: usize, round: u64) -> Result<bool> {
        if FoundationPolicy::decode(&self.plan.hops[hop].descriptor.terms)?.rate_msat == 0 {
            return Ok(true);
        }
        if self.funded.contains(&(hop, round)) {
            return Ok(true);
        }
        Ok(self
            .ledger
            .get_outgoing(self.round_operation(hop, round))?
            .map_or(false, |r| r.state == OutgoingState::Consumed))
    }
    fn window(&self, _hop: usize, round: u64, clock: ClockObservation) -> Result<AcceptanceWindow> {
        let mut common = AcceptanceWindow {
            opens_at_ms: 0,
            closes_at_ms: i64::MAX,
        };
        for hop in 0..self.plan.hops.len() {
            let window = self.hop_window(hop, round, clock)?;
            common.opens_at_ms = common.opens_at_ms.max(window.opens_at_ms);
            common.closes_at_ms = common.closes_at_ms.min(window.closes_at_ms);
        }
        if common.opens_at_ms > common.closes_at_ms {
            return Err("No conservative all-hop payment window".into());
        }
        Ok(common)
    }
    fn hop_window(
        &self,
        hop: usize,
        round: u64,
        _clock: ClockObservation,
    ) -> Result<AcceptanceWindow> {
        if round == 1 {
            let closes_at_ms = (0..self.plan.hops.len())
                .map(|h| self.plan.setup_deadline_ms(h))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .min()
                .ok_or_else(invalid)?;
            return Ok(AcceptanceWindow {
                opens_at_ms: self.commit_open_ms.ok_or_else(invalid)?,
                closes_at_ms,
            });
        }
        let policy = FoundationPolicy::decode(&self.plan.hops[hop].descriptor.terms)?;
        // Actual initial activation is bounded by construction and its ORIGINAL
        // setup deadline. This deliberately wide immutable T0 interval survives
        // restart without inventing precision or extending a learned deadline.
        let anchor = ActivationIntervalAnchor {
            started_at: UtcClockInterval {
                earliest_ms: earliest(self.plan.construction)?,
                latest_ms: self.plan.setup_deadline_ms(hop)?,
            },
            round_ms: 60000,
            early_ms: 60000,
            grace_ms: u64::from(policy.grace_ms),
        };
        let mut window = anchor.acceptance_window(round)?;
        window.closes_at_ms = window.closes_at_ms.min(
            earliest(self.plan.construction)?
                .checked_add(i64::try_from(policy.max_age_ms)?)
                .ok_or_else(invalid)?,
        );
        Ok(window)
    }
    async fn fund_round<F>(&mut self, round: u64, observe: &mut F) -> Result<Option<u16>>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        if round == 0
            || !self
                .ledger
                .origin_period_admitted(self.plan.local_scope(), (round - 1) / 10)?
        {
            return Err("All-hop admission required before funding this round".into());
        }
        // All-hop COMMIT precedes the first invoice fetch, reservation and pay.
        // Subsequent rounds additionally require all-hop authenticated ACTIVE.
        for hop in 0..self.plan.hops.len() {
            let state = self.observations[hop].as_ref().ok_or_else(invalid)?;
            if state
                .snapshot
                .operation
                .as_ref()
                .map_or(true, |o| o.phase != InitialOperationPhase::Verified)
                || (round == 1 && state.snapshot.state != InitialOriginState::Committed)
                || (round > 1
                    && (state.snapshot.state != InitialOriginState::Active
                        || state.snapshot.clocks[3].unwrap_or(0)
                            <= state.at.elapsed().as_millis() as u64 + 1000))
            {
                return Err(invalid());
            }
        }
        for hop in 0..self.plan.hops.len() {
            if self.round_funded(hop, round)? {
                continue;
            }
            let clock = self.clock(observe)?;
            let op = self.round_operation(hop, round);
            let saved = self.ledger.get_origin_invoice(op)?;
            let policy = FoundationPolicy::decode(&self.plan.hops[hop].descriptor.terms)?;
            // Explicit zero-priced paid capacity has no wallet dispatch. Relay
            // zero-charge authorization and authenticated ACTIVE remain required.
            if policy.rate_msat == 0 {
                continue;
            }
            let wallet = self.wallet.as_ref().ok_or_else(invalid)?.clone();
            let (intent, invoice) = if let Some(saved) = saved {
                let invoice = wallet.restore_for_lookup(
                    &saved.artifact,
                    saved.intent.invoice_digest,
                    InvoiceExpectation {
                        amount_msat: saved.intent.amount_msat,
                        expected_payee: None,
                        expected_payment_hash: Some(saved.intent.payment_hash),
                    },
                )?;
                (saved.intent, invoice)
            } else {
                let window = self.window(hop, round, clock)?;
                if earliest(clock)? < window.opens_at_ms {
                    return Ok(Some(hop as u16 + 1));
                }
                if latest(clock)?
                    .checked_add(1000)
                    .map_or(true, |n| n > window.closes_at_ms)
                {
                    return Err("Round funding deadline expired".into());
                }
                let offer = self.plan.hops[hop]
                    .descriptor
                    .bolt12_offer
                    .clone()
                    .ok_or_else(invalid)?;
                let payment_id = self.payment_id(hop, round)?;
                let invoice = wallet
                    .fetch_bolt12(
                        &offer,
                        payment_id,
                        InvoiceExpectation {
                            amount_msat: policy.rate_msat,
                            expected_payee: None,
                            expected_payment_hash: None,
                        },
                        Observation {
                            now_ms: clock.now_ms,
                            uncertainty_ms: clock.uncertainty_ms,
                        },
                    )
                    .await?;
                let intent = OutgoingIntent {
                    operation: op,
                    account: self.plan.account,
                    instance: self.plan.binding.instance,
                    handle: self.plan.binding.handle,
                    local_budget_scope: self.plan.local_scope(),
                    round,
                    kind: ChargeKind::Round,
                    payment_id: self.payment_id(hop, round)?,
                    charge_reference: self.plan.charge_reference(hop),
                    payee_reference: self.plan.payee_reference(hop),
                    accepted_terms_digest: self.plan.budget.accepted_terms_digest,
                    hop_circuit_binding: self.plan.hops[hop].registration.circuit_binding(),
                    hop_policy_hash: self.plan.hops[hop].registration.policy_hash(),
                    invoice_digest: invoice.digest(),
                    payment_hash: invoice.payment_hash(),
                    amount_msat: policy.rate_msat,
                    fee_limit_msat: self.plan.hops[hop].fee_limit_msat,
                    window,
                };
                self.ledger
                    .save_origin_invoice(&intent, invoice.invoice())?;
                (intent, invoice)
            };
            self.validate_intent(hop, round, &intent)?;
            let clock = self.clock(observe)?;
            let record = wallet::prepare_cln(&mut self.ledger, &wallet, &intent, &invoice, clock)?;
            let result = if record.state == OutgoingState::Prepared {
                wallet::dispatch_cln(&mut self.ledger, &wallet, op, &invoice, clock, 1).await?
            } else {
                wallet::reconcile_cln(&mut self.ledger, &wallet, op, &invoice, clock).await?
            };
            match result.evidence {
                EvidenceStatus::PaidWithinWindow if !result.record.fee_limit_breached => {
                    self.funded.insert((hop, round));
                }
                EvidenceStatus::Unresolved => return Ok(Some(hop as u16 + 1)),
                _ => return Err("Round payment failed or cannot fund the accepted window".into()),
            }
        }
        Ok(None)
    }
    fn validate_intent(&self, hop: usize, round: u64, intent: &OutgoingIntent) -> Result<()> {
        let reg = &self.plan.hops[hop].registration;
        if intent.operation != self.round_operation(hop, round)
            || intent.account != self.plan.account
            || intent.instance != self.plan.binding.instance
            || intent.handle != self.plan.binding.handle
            || intent.local_budget_scope != self.plan.local_scope()
            || intent.round != round
            || intent.kind != ChargeKind::Round
            || intent.payment_id != self.payment_id(hop, round)?
            || intent.charge_reference != self.plan.charge_reference(hop)
            || intent.payee_reference != self.plan.payee_reference(hop)
            || intent.accepted_terms_digest != self.plan.budget.accepted_terms_digest
            || intent.hop_circuit_binding != reg.circuit_binding()
            || intent.hop_policy_hash != reg.policy_hash()
            || intent.amount_msat
                != FoundationPolicy::decode(&self.plan.hops[hop].descriptor.terms)?.rate_msat
            || intent.fee_limit_msat != self.plan.hops[hop].fee_limit_msat
        {
            return Err(invalid());
        }
        Ok(())
    }
    async fn observe_active<F>(&mut self, observe: &mut F) -> Result<Option<u64>>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        let mut minimum = u64::MAX;
        for hop in 0..self.plan.hops.len() {
            let sequence = self.query_indices[hop];
            let mut next = sequence == 0;
            if sequence > 0 {
                let previous = self.operation(hop, InitialOperationKind::Query, sequence);
                let request_at = Instant::now();
                let outcome = initial_observation(
                    self.control
                        .durable_initial_operation(&mut self.ledger, &previous)
                        .await,
                )?;
                let Some(outcome) = outcome else {
                    self.queue_status_query(hop).await?;
                    minimum = 0;
                    continue;
                };
                match outcome {
                    DurableInitialResult::Queued { .. } => return Err(invalid()),
                    DurableInitialResult::Recovered(snapshot) => {
                        self.check_snapshot(hop, &snapshot)?;
                        if snapshot
                            .operation
                            .as_ref()
                            .map_or(false, |o| o.phase == InitialOperationPhase::Verified)
                        {
                            self.retain_snapshot(hop, snapshot, request_at)?;
                            let snapshot = &self.observations[hop]
                                .as_ref()
                                .ok_or_else(invalid)?
                                .snapshot;
                            let remaining = snapshot.clocks[2]
                                .unwrap_or(0)
                                .min(snapshot.clocks[3].unwrap_or(0));
                            if snapshot.state == InitialOriginState::Active && remaining > 1000 {
                                minimum = minimum.min(snapshot.clocks[0].unwrap_or(0));
                            } else {
                                minimum = 0;
                            }
                            next = true;
                        } else {
                            minimum = 0;
                        }
                    }
                }
            } else {
                minimum = 0;
            }
            if next {
                self.queue_status_query(hop).await?;
            }
            self.clock(observe)?;
        }
        Ok(if minimum == 0 || minimum == u64::MAX {
            None
        } else {
            Some(minimum)
        })
    }
    fn consume_through(&mut self, round: u64) -> Result<()> {
        let current = self.ledger.spending_round(self.plan.local_scope())?;
        // A client can only prepay its current or next round. Reject an
        // impossible progress jump before looping over retained obligations.
        if round < current || round.saturating_sub(current) > 10 {
            return Err("Origin progress exceeds bounded reconciliation interval".into());
        }
        // All-hop authenticated ACTIVE proves the relay accepted the original
        // fee proof. Local native settlement alone never retires this exposure.
        for (index, hop) in self.plan.hops.iter().enumerate() {
            if FoundationPolicy::decode(&hop.descriptor.terms)?.handshake_fee_msat > 0 {
                self.ledger
                    .mark_handshake_consumed(self.plan.local_scope(), index as u16 + 1)?;
            }
        }
        for consumed in current..=round {
            for hop in 0..self.plan.hops.len() {
                if FoundationPolicy::decode(&self.plan.hops[hop].descriptor.terms)?.rate_msat == 0 {
                    continue;
                }
                if !self.round_funded(hop, consumed)? {
                    return Err(
                        "Authenticated activation has no matching local funded obligation".into(),
                    );
                }
                self.ledger
                    .mark_outgoing_consumed(self.round_operation(hop, consumed))?;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "foundation_tests.rs"]
mod tests;
