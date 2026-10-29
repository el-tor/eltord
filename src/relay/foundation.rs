//! Relay-side foundation orchestration. Events are query hints; only exact
//! native recipient evidence and durable authorization can reach Tor. This
//! coordinator has no outgoing-payment interface and never provisions storage.
use crate::{payment_ledger::*, rpc::*};
use lni::cln::foundation::{ClnFoundation, IncomingStatus, Observation};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    error::Error,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MAX_BINDINGS: usize = 128;
const MAX_RECOVERY_HISTORY: usize = 4096;
const ACTIONABLE_BURST: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayStep {
    Recovered,
    Idle,
    Deferred,
    Reconciled {
        notifications: usize,
        consumed: usize,
    },
}
/// The supervisor supplies already-opened durable storage, an exact verified
/// account, and a connected authenticated controller. Errors never enable a
/// legacy fallback. Reconnect replaces only the ephemeral control connection.
pub struct RelayFoundation {
    ledger: PaymentLedger,
    wallet: ClnFoundation,
    control: PaymentControl,
    recovery: Option<PaymentRecovery>,
    live: BTreeMap<[u8; 16], PaymentInventoryRow>,
    next: usize,
    actionable: VecDeque<[u8; 16]>,
    financial: BTreeSet<[u8; 16]>,
    probed: BTreeMap<[u8; 16], u64>,
    closed: BTreeSet<[u8; 16]>,
    actionable_streak: u8,
    last_inventory: Instant,
    instance: Option<[u8; 16]>,
}
impl RelayFoundation {
    pub fn new(
        ledger: PaymentLedger,
        wallet: ClnFoundation,
        control: PaymentControl,
    ) -> Result<Self> {
        if ledger.account_id() != AccountId(wallet.account().fingerprint()) {
            return Err("Foundation recipient account mismatch".into());
        }
        Ok(Self {
            ledger,
            wallet,
            control,
            recovery: None,
            live: BTreeMap::new(),
            next: 0,
            actionable: VecDeque::new(),
            financial: BTreeSet::new(),
            probed: BTreeMap::new(),
            closed: BTreeSet::new(),
            actionable_streak: 0,
            last_inventory: Instant::now(),
            instance: None,
        })
    }
    pub fn reconnect(&mut self, control: PaymentControl) {
        self.control = control;
        self.recovery = None;
    }
    /// One bounded page and one live binding per call. The supervisor chooses
    /// a bounded polling cadence; no cancelled event-read future is reused.
    pub async fn step<F>(&mut self, mut observe: F) -> Result<RelayStep>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        let result = self.step_inner(&mut observe).await;
        if result.is_err() {
            self.recovery = None;
        }
        result
    }
    fn binding_closed(&mut self, instance: [u8; 16], handle: [u8; 16]) -> Result<bool> {
        if self.closed.contains(&handle) {
            return Ok(true);
        }
        if !self.ledger.binding_is_closed(instance, handle)? {
            return Ok(false);
        }
        // Only a bounded lookup cache; eviction never removes the durable
        // tombstone or permits a handle to reopen without checking that ledger.
        if self.closed.len() >= MAX_RECOVERY_HISTORY {
            self.closed.clear();
        }
        self.closed.insert(handle);
        Ok(true)
    }
    fn has_financial_work(&self, instance: [u8; 16], handle: [u8; 16]) -> Result<bool> {
        Ok(self
            .ledger
            .latest_admission_period(instance, handle)?
            .is_some()
            || self
                .ledger
                .get_handshake_obligation(instance, handle)?
                .is_some())
    }
    fn forget_work(&mut self, handle: [u8; 16]) {
        self.live.remove(&handle);
        self.financial.remove(&handle);
        self.probed.remove(&handle);
        self.actionable.retain(|h| *h != handle);
    }
    fn schedule_actionable(&mut self, handle: [u8; 16]) -> Result<()> {
        if self.live.contains_key(&handle) && !self.actionable.contains(&handle) {
            if self.actionable.len() >= MAX_BINDINGS {
                return Err("Foundation relay work capacity exceeded".into());
            }
            self.actionable.push_back(handle);
        }
        Ok(())
    }
    fn schedule_hints(&mut self, events: &[PaymentEvent]) -> Result<()> {
        for event in events {
            if event.role == PaymentRole::Relay
                && event.kind != PaymentEventKind::Registered
                && self.live.get(&event.handle).is_some_and(|row| {
                    row.revision == event.revision
                        && self.probed.get(&event.handle).copied().unwrap_or(0) < event.revision
                })
            {
                // Only the current, not-yet-probed revision can wake ordinary
                // work. Buffered hints matching the fresh inventory can guide
                // its first query, but stale/duplicate hints cannot reawaken a
                // dormant row. Financial followups have their own bounded queue.
                // Hints determine query order only. No reservation, clock,
                // authorization or consumption follows without the typed query.
                self.schedule_actionable(event.handle)?;
            }
        }
        Ok(())
    }
    async fn synchronize(&mut self) -> Result<()> {
        let inventory = self.control.inventory().await?;
        let events = self.control.take_events()?;
        let recovery = PaymentRecovery::new(inventory, &events)?;
        let instance = recovery.instance();
        if self.instance != Some(instance) {
            self.ledger.close_other_instances(instance)?;
            self.live.clear();
            self.financial.clear();
            self.probed.clear();
            self.closed.clear();
            self.actionable.clear();
            self.actionable_streak = 0;
            self.next = 0;
        }
        let mut live = BTreeMap::new();
        for row in recovery.tracked_rows_for(PaymentRole::Relay)? {
            // Recovery can retain 4096 historical hints. The 128 work ceiling
            // counts only bindings not already durably closed.
            if !self.binding_closed(instance, row.handle)? {
                live.insert(row.handle, row);
            }
        }
        // An absent inventory row never releases accounting. Retain only
        // previously known durable financial work for an authoritative terminal
        // query; ordinary absent candidates do not accumulate across recovery.
        for handle in &self.financial {
            if let Some(row) = self.live.get(handle) {
                if !self.ledger.binding_is_closed(instance, *handle)? {
                    live.entry(*handle).or_insert_with(|| row.clone());
                }
            }
        }
        if live.len() > MAX_BINDINGS {
            return Err("Foundation relay work capacity exceeded".into());
        }
        self.live = live;
        let live = &self.live;
        self.financial.retain(|h| live.contains_key(h));
        self.probed.retain(|h, _| live.contains_key(h));
        self.actionable.retain(|h| live.contains_key(h));
        for handle in self.live.keys().copied().collect::<Vec<_>>() {
            if self.has_financial_work(instance, handle)? {
                self.financial.insert(handle);
                self.schedule_actionable(handle)?;
            }
        }
        self.instance = Some(instance);
        self.recovery = Some(recovery);
        self.last_inventory = Instant::now();
        self.schedule_hints(&events)?;
        Ok(())
    }
    fn drain_hints(&mut self) -> Result<()> {
        let events = self.control.take_events()?;
        let recovery = self
            .recovery
            .as_mut()
            .ok_or("Foundation requires inventory recovery")?;
        for event in &events {
            recovery.observe(event)?;
        }
        let instance = recovery.instance();
        let rows = recovery.tracked_rows_for(PaymentRole::Relay)?;
        for row in rows {
            if self.binding_closed(instance, row.handle)? {
                self.forget_work(row.handle);
                continue;
            }
            self.live.insert(row.handle, row);
        }
        if self.live.len() > MAX_BINDINGS {
            return Err("Foundation relay work capacity exceeded".into());
        }
        self.schedule_hints(&events)?;
        Ok(())
    }
    fn next_work(&mut self) -> Option<PaymentInventoryRow> {
        let ordinary: Vec<_> = self
            .live
            .values()
            .filter(|row| {
                !self.financial.contains(&row.handle)
                    && self.probed.get(&row.handle).copied().unwrap_or(0) < row.revision
            })
            .cloned()
            .collect();
        if !self.actionable.is_empty()
            && (self.actionable_streak < ACTIONABLE_BURST || ordinary.is_empty())
        {
            self.actionable_streak = self.actionable_streak.saturating_add(1);
            let handle = self.actionable.pop_front().expect("nonempty bounded queue");
            return self.live.get(&handle).cloned();
        }
        if !ordinary.is_empty() {
            let row = ordinary[self.next % ordinary.len()].clone();
            self.next = self.next.wrapping_add(1);
            self.actionable_streak = 0;
            return Some(row);
        }
        None
    }
    async fn step_inner<F>(&mut self, observe: &mut F) -> Result<RelayStep>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        if self.recovery.is_none() {
            self.synchronize().await?;
            return Ok(RelayStep::Recovered);
        }
        self.drain_hints()?;
        let Some(row) = self.next_work() else {
            // take_events only drains already parsed bytes. A real bounded
            // inventory read is needed to ingest hints while all candidates are
            // dormant; never spin on the in-memory event queue indefinitely.
            if self.last_inventory.elapsed() >= Duration::from_secs(1) {
                self.synchronize().await?;
                return Ok(RelayStep::Idle);
            }
            return Ok(RelayStep::Deferred);
        };
        let instance = self
            .instance
            .ok_or("Foundation requires inventory recovery")?;
        if self.binding_closed(instance, row.handle)? {
            self.forget_work(row.handle);
            return Ok(RelayStep::Deferred);
        }
        let result = self.reconcile_binding(instance, &row, observe).await;
        if self.binding_closed(instance, row.handle)? {
            self.forget_work(row.handle);
        } else if self.has_financial_work(instance, row.handle)? {
            self.financial.insert(row.handle);
            self.schedule_actionable(row.handle)?;
        } else if matches!(
            result.as_ref().err().and_then(rejection),
            Some(
                PaymentRejection::InitialStateUnconfirmed
                    | PaymentRejection::UnknownPeriod
                    | PaymentRejection::DeadlineExpired
            )
        ) {
            // Authoritative absence of pending work for this revision, not
            // closure/payment. A new lifecycle revision wakes it immediately.
            self.probed.insert(row.handle, row.revision);
        } else if result.is_err() {
            self.schedule_actionable(row.handle)?;
        }
        match result {
            Err(error) if rejection(&error) == Some(PaymentRejection::UnknownCircuit) => {
                self.ledger.close_binding(instance, row.handle)?;
                self.forget_work(row.handle);
                self.recovery = None;
                Ok(RelayStep::Deferred)
            }
            Err(error)
                if matches!(
                    rejection(&error),
                    Some(
                        PaymentRejection::StateTransitionPending
                            | PaymentRejection::InitialStateUnconfirmed
                            | PaymentRejection::UnknownPeriod
                            | PaymentRejection::DeadlineExpired
                            | PaymentRejection::Capacity
                    )
                ) =>
            {
                Ok(RelayStep::Deferred)
            }
            other => other,
        }
    }
    async fn reconcile_binding<F>(
        &mut self,
        instance: [u8; 16],
        row: &PaymentInventoryRow,
        observe: &mut F,
    ) -> Result<RelayStep>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        let mut period = self
            .ledger
            .latest_admission_period(instance, row.handle)?
            .unwrap_or(0);
        if self
            .ledger
            .get_batch_reservation(instance, row.handle, period)?
            .is_none()
        {
            self.control
                .admit_pending(
                    &mut self.ledger,
                    instance,
                    row.handle,
                    period,
                    &mut *observe,
                )
                .await?;
        }
        if period == 0
            && self
                .ledger
                .get_batch_reservation(instance, row.handle, period)?
                .is_some_and(|r| r.outcome == AdmissionOutcome::Conflict)
        {
            // Recover the exact persisted refusal. The canonical decision
            // receipt confirms this initial candidate was refused, not paid.
            self.control
                .admit_pending(
                    &mut self.ledger,
                    instance,
                    row.handle,
                    period,
                    &mut *observe,
                )
                .await?;
            self.ledger.close_binding(instance, row.handle)?;
            self.live.remove(&row.handle);
            return Ok(RelayStep::Deferred);
        }
        let mut expected = self
            .ledger
            .get_reserved_batch_registration(instance, row.handle, period)?;
        // A refused next-period admission never replaces its still-live prior
        // financial allocation. At most current and admitted-next are queried.
        if expected.is_none() && period > 0 {
            period -= 1;
            expected = self
                .ledger
                .get_reserved_batch_registration(instance, row.handle, period)?;
        }
        let expected = expected.ok_or("Foundation has no durable admitted allocation")?;
        // Erase the boxed diagnostic before a second await: the existing RPC
        // error type is intentionally not Send, while this runtime task is.
        let first_observed = {
            let result = self
                .control
                .relay_registration_observed(&expected, &mut *observe)
                .await;
            match result {
                Ok(observed) => Some(observed),
                Err(error) => {
                    if rejection(&error) != Some(PaymentRejection::InitialStateUnconfirmed) {
                        return Err(error);
                    }
                    None
                }
            }
        };
        let mut observed = if let Some(observed) = first_observed {
            observed
        } else {
            self.control
                .admit_pending(
                    &mut self.ledger,
                    instance,
                    row.handle,
                    period,
                    &mut *observe,
                )
                .await?;
            self.control
                .relay_registration_observed(&expected, &mut *observe)
                .await?
        };
        let active = observed.registration.current_round.saturating_sub(1) / 10;
        if active > period || period > active + 1 {
            return Err("Foundation registration periods exceed current/next bound".into());
        }
        let mut next_snapshot = None;
        let current_expected = if active != period {
            let current = self
                .ledger
                .get_reserved_batch_registration(instance, row.handle, active)?
                .ok_or("Missing durable current allocation")?;
            let current_observed = self
                .control
                .relay_registration_observed(&current, &mut *observe)
                .await?;
            if current_observed.registration.current_round != observed.registration.current_round {
                return Ok(RelayStep::Deferred);
            }
            next_snapshot = Some(observed.registration);
            observed = current_observed;
            current
        } else {
            expected.clone()
        };
        // A hint observed while reading the snapshot can invalidate continuity
        // or announce a newer revision. Resynchronize/requery before claims or
        // authorization notifications, not after acting on the stale snapshot.
        self.drain_hints()?;
        if self
            .live
            .get(&row.handle)
            .is_some_and(|r| r.revision > observed.registration.revision)
        {
            return Ok(RelayStep::Deferred);
        }
        let handshake = self
            .ledger
            .get_handshake_obligation(instance, row.handle)?
            .ok_or("Missing original handshake obligation")?;
        let snapshot = &observed.registration;
        validate_relay_consumption(&mut self.ledger, snapshot, &current_expected, &handshake)?;
        if let Some(next) = next_snapshot.as_ref() {
            validate_relay_consumption(&mut self.ledger, next, &expected, &handshake)?;
        }
        let consumed =
            reconcile_relay_consumption(&mut self.ledger, snapshot, &current_expected, &handshake)?;
        if matches!(
            snapshot.phase,
            RelayPaymentPhase::Closed | RelayPaymentPhase::Expired
        ) {
            self.ledger.close_binding(instance, row.handle)?;
            self.live.remove(&row.handle);
            return Ok(RelayStep::Reconciled {
                notifications: 0,
                consumed,
            });
        }
        observed.bind_saved_clocks(&mut self.ledger)?;
        // IDs were durable before COMMIT. UPDATED-index discovery never skips
        // associations whose payment arrived after registration. The cursor and
        // every relevant signed association commit in the same transaction.
        let page = self
            .wallet
            .incoming_updates(self.ledger.native_invoice_cursor()?)
            .await?;
        self.ledger.apply_native_invoice_page(&page)?;
        let mut obligations = Vec::with_capacity(3);
        if self
            .ledger
            .get_authorization(&handshake)?
            .map_or(true, |a| a.state != AuthorizationState::Consumed)
        {
            obligations.push(handshake);
        }
        let current = snapshot.current_round.max(1);
        for round in [current, current.checked_add(1).ok_or("Round overflow")?] {
            if snapshot.current_round == 0 && round > 1 {
                continue;
            }
            let obligation = current_expected
                .rounds
                .iter()
                .chain(expected.rounds.iter())
                .find(|o| o.round == round);
            if let Some(obligation) = obligation {
                obligations.push(obligation.clone());
            }
        }
        let mut notifications = 0;
        for obligation in obligations {
            self.drain_hints()?;
            if self.authorize_recipient(&obligation, observe).await? {
                self.control
                    .notify_authorization(&mut self.ledger, &obligation)
                    .await?;
                notifications += 1;
            }
        }
        // Renewals are probed only at the single next period. No candidate or
        // missing period can create a replacement ID, fee, or payment window.
        if snapshot.current_round > 0 && active == period {
            let next = period.checked_add(1).ok_or("Period overflow")?;
            if self
                .ledger
                .get_batch_reservation(instance, row.handle, next)?
                .is_none()
            {
                match self
                    .control
                    .admit_pending(&mut self.ledger, instance, row.handle, next, &mut *observe)
                    .await
                {
                    Ok(_) => (),
                    Err(error)
                        if matches!(
                            rejection(&error),
                            Some(
                                PaymentRejection::UnknownPeriod
                                    | PaymentRejection::StateTransitionPending
                                    | PaymentRejection::DeadlineExpired
                                    | PaymentRejection::Capacity
                            )
                        ) =>
                    {
                        ()
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        self.drain_hints()?;
        let mut queried = row.clone();
        queried.revision = snapshot.revision;
        if self
            .recovery
            .as_ref()
            .ok_or("Foundation requires resync")?
            .tracked_rows_for(PaymentRole::Relay)?
            .iter()
            .any(|r| r.handle == row.handle)
        {
            self.recovery
                .as_mut()
                .expect("recovery exists")
                .queried(instance, &queried)?;
        }
        Ok(RelayStep::Reconciled {
            notifications,
            consumed,
        })
    }
    async fn authorize_recipient<F>(
        &mut self,
        obligation: &Obligation,
        observe: &mut F,
    ) -> Result<bool>
    where
        F: FnMut() -> Result<ClockObservation>,
    {
        if let Some(saved) = self.ledger.get_authorization(obligation)? {
            return Ok(saved.state == AuthorizationState::Authorized);
        }
        let decision = loop {
            let id = rand::random();
            if id != [0; 16] {
                break id;
            }
        };
        if obligation.amount_msat == 0 {
            return match self.ledger.authorize_zero(obligation, decision, observe()?) {
                Ok(_) => Ok(true),
                Err(LedgerError::OutsideWindow | LedgerError::NotRegistered) => Ok(false),
                Err(error) => Err(error.into()),
            };
        }
        let candidate = if let Some(hash) = obligation.expected_payment_hash {
            Some((hash, None))
        } else {
            self.ledger
                .incoming_candidate(obligation.payment_id)?
                .map(|c| (c.payment_hash, Some(c.invoice_digest)))
        };
        let Some((hash, digest)) = candidate else {
            return Ok(false);
        };
        let before = observe()?;
        let status = self
            .wallet
            .lookup_incoming(
                hash,
                Observation {
                    now_ms: before.now_ms,
                    uncertainty_ms: before.uncertainty_ms,
                },
            )
            .await?;
        let IncomingStatus::Settled(proof) = status else {
            return Ok(false);
        };
        if proof.account != self.wallet.account()
            || digest.is_some_and(|d| d != proof.invoice_digest)
        {
            return Err("Recipient discovery differs from exact native invoice proof".into());
        }
        let evidence =
            crate::payment_ledger::wallet::recipient_evidence(self.ledger.account_id(), &proof)?;
        // A slow wallet lookup cannot borrow its pre-request UTC observation to
        // authorize a charge after the immutable financial deadline.
        match self
            .ledger
            .authorize(obligation, &evidence, decision, observe()?)
        {
            Ok(_) => Ok(true),
            Err(LedgerError::OutsideWindow) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
}
fn rejection(error: &Box<dyn Error>) -> Option<PaymentRejection> {
    error
        .downcast_ref::<std::io::Error>()
        .and_then(PaymentRejection::from_io)
}
#[cfg(test)]
mod tests;
