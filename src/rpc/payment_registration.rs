//! Privileged relay registration snapshots and conservative clock observations.
//! Parsing never creates a reservation, changes a deadline, or proves settlement.
use crate::payment_ledger::{
    AcceptanceWindow, ActivationIntervalAnchor, BatchRegistration, ClockObservation,
    InitialClockAnchor, PaymentLedger, UtcClockInterval,
};
use sha2::{Digest, Sha256};
use std::{
    convert::{TryFrom, TryInto},
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum RegistrationError {
    #[error("invalid relay payment registration snapshot")]
    Invalid,
    #[error("relay registration differs from durable accepted terms")]
    Binding,
    #[error("invalid or unbounded registration clock observation")]
    Clock,
    #[error("registration has no activation clock")]
    NotActivated,
    #[error("legacy initial clock has no immutable uncertainty provenance")]
    LegacyClock,
}
type Result<T> = std::result::Result<T, RegistrationError>;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RelayPaymentPhase {
    Committed,
    WaitPayment,
    Active,
    Grace,
    Closed,
    Expired,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RelayAuthorization {
    None,
    Authorized,
    Consumed,
}
#[derive(Clone, PartialEq, Eq)]
pub struct RegisteredCharge {
    pub amount_msat: u64,
    pub payment_hash: Option<[u8; 32]>,
    pub decision: Option<[u8; 16]>,
    pub authorization: RelayAuthorization,
}
#[derive(Clone, PartialEq, Eq)]
pub struct RegisteredRound {
    pub round: u64,
    pub payment_id: [u8; 32],
    pub opens_in_ms: Option<u64>,
    pub closes_in_ms: Option<u64>,
    pub charge: RegisteredCharge,
}
/// No Debug: per-hop identifiers, payment associations and accepted terms must
/// not enter routine logs. All immutable fields are checked against the caller's
/// already-durable reservation, including C, Q, A0 and all ten ordered IDs.
#[derive(Clone, PartialEq, Eq)]
pub struct RelayRegistration {
    pub instance: [u8; 16],
    pub handle: [u8; 16],
    pub revision: u64,
    pub period: u64,
    pub phase: RelayPaymentPhase,
    pub current_round: u64,
    pub elapsed_ms: u64,
    pub setup_remaining_ms: u64,
    pub enforcement_remaining_ms: u64,
    pub constructed_elapsed_ms: u64,
    pub commit_elapsed_ms: u64,
    pub fee: RegisteredCharge,
    pub rounds: [RegisteredRound; 10],
    terms: [u8; 55],
    initial_allocation: [u8; 32],
    allocation: [u8; 32],
    binding: [u8; 32],
}
fn num(s: &str) -> Result<u64> {
    let n: u64 = s.parse().map_err(|_| RegistrationError::Invalid)?;
    if n.to_string() != s {
        return Err(RegistrationError::Invalid);
    }
    Ok(n)
}
fn bytes<const N: usize>(s: &str) -> Result<[u8; N]> {
    if s.len() != N * 2
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(RegistrationError::Invalid);
    }
    let mut result = [0; N];
    hex::decode_to_slice(s, &mut result).map_err(|_| RegistrationError::Invalid)?;
    Ok(result)
}
fn u32_at(t: &[u8; 55], n: usize) -> u64 {
    u32::from_be_bytes(t[n..n + 4].try_into().expect("fixed policy offset")) as u64
}
fn u64_at(t: &[u8; 55], n: usize) -> u64 {
    u64::from_be_bytes(t[n..n + 8].try_into().expect("fixed policy offset"))
}
fn hash(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update((domain.len() as u16).to_be_bytes());
    h.update(domain);
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}
fn charge(
    amount: u64,
    hash_text: &str,
    decision: &str,
    auth: &str,
    fee: bool,
) -> Result<RegisteredCharge> {
    let authorization = match auth {
        "NONE" => RelayAuthorization::None,
        "AUTHORIZED" => RelayAuthorization::Authorized,
        "CONSUMED" => RelayAuthorization::Consumed,
        _ => return Err(RegistrationError::Invalid),
    };
    let payment_hash = if hash_text == "-" {
        None
    } else {
        Some(bytes(hash_text)?)
    };
    let decision = if decision == "-" {
        None
    } else {
        Some(bytes(decision)?)
    };
    if authorization == RelayAuthorization::None {
        if decision.is_some() || (!fee && payment_hash.is_some()) {
            return Err(RegistrationError::Invalid);
        }
    } else if decision.map_or(true, |d| d == [0; 16])
        || payment_hash.map_or(true, |h| (amount == 0) != (h == [0; 32]))
    {
        return Err(RegistrationError::Invalid);
    }
    if fee && payment_hash.map_or(true, |h| (amount == 0) != (h == [0; 32])) {
        return Err(RegistrationError::Invalid);
    }
    Ok(RegisteredCharge {
        amount_msat: amount,
        payment_hash,
        decision,
        authorization,
    })
}
impl RelayRegistration {
    pub fn parse(reply: &str, expected: &BatchRegistration) -> Result<Self> {
        if reply.len() > 8192
            || expected.rounds[0].instance == [0; 16]
            || expected.rounds[0].handle == [0; 16]
        {
            return Err(RegistrationError::Invalid);
        }
        let lines: Vec<Vec<&str>> = reply
            .split('\n')
            .map(|line| line.split(' ').collect())
            .collect();
        if lines.len() != 17 || lines[16] != ["250", "OK"] {
            return Err(RegistrationError::Invalid);
        }
        let r = &lines[0];
        if r.len() != 6 || r[..2] != ["250-ELTOR_REGISTRATION", "1"] {
            return Err(RegistrationError::Invalid);
        }
        let instance = bytes(r[2])?;
        let handle = bytes(r[3])?;
        let revision = num(r[4])?;
        let period = num(r[5])?;
        if revision == 0
            || period != expected.period
            || instance != expected.rounds[0].instance
            || handle != expected.rounds[0].handle
        {
            return Err(RegistrationError::Binding);
        }
        let p = &lines[1];
        if p.len() != 5 || p[0] != "250-POLICY" {
            return Err(RegistrationError::Invalid);
        }
        let terms = bytes::<55>(p[1])?;
        let initial_allocation = bytes(p[2])?;
        let allocation = bytes(p[3])?;
        let first = period
            .checked_mul(10)
            .and_then(|n| n.checked_add(1))
            .ok_or(RegistrationError::Invalid)?;
        first.checked_add(9).ok_or(RegistrationError::Invalid)?;
        if terms[..3] != [1, 1, 10]
            || u32_at(&terms, 11) != 60_000
            || u32_at(&terms, 15) != 60_000
            || u32_at(&terms, 19) > 60_000
            || u64_at(&terms, 31) == 0
            || u32_at(&terms, 39) < 2
            || period >= u32_at(&terms, 39)
            || u32_at(&terms, 51) == 0
            || num(p[4])? != first
            || initial_allocation == [0; 32]
            || allocation == [0; 32]
        {
            return Err(RegistrationError::Invalid);
        }
        if period > 0
            && period
                .checked_add(1)
                .and_then(|n| n.checked_mul(600_000))
                .map_or(true, |end| end > u64_at(&terms, 31))
        {
            return Err(RegistrationError::Invalid);
        }
        let b = &lines[2];
        if b.len() != 2 || b[0] != "250-BINDING" {
            return Err(RegistrationError::Invalid);
        }
        let binding = bytes(b[1])?;
        if binding == [0; 32] {
            return Err(RegistrationError::Invalid);
        }
        let s = &lines[3];
        if s.len() != 6 || s[0] != "250-STATE" {
            return Err(RegistrationError::Invalid);
        }
        let phase = match s[1] {
            "COMMITTED" => RelayPaymentPhase::Committed,
            "WAIT_PAYMENT" => RelayPaymentPhase::WaitPayment,
            "ACTIVE" => RelayPaymentPhase::Active,
            "GRACE" => RelayPaymentPhase::Grace,
            "CLOSED" => RelayPaymentPhase::Closed,
            "EXPIRED" => RelayPaymentPhase::Expired,
            _ => return Err(RegistrationError::Invalid),
        };
        let current_round = num(s[2])?;
        let elapsed_ms = num(s[3])?;
        let setup_remaining_ms = num(s[4])?;
        let enforcement_remaining_ms = num(s[5])?;
        let c = &lines[4];
        if c.len() != 3 || c[0] != "250-CLOCK" {
            return Err(RegistrationError::Invalid);
        }
        let constructed_elapsed_ms = num(c[1])?;
        let commit_elapsed_ms = num(c[2])?;
        if commit_elapsed_ms > constructed_elapsed_ms {
            return Err(RegistrationError::Invalid);
        }
        let f = &lines[5];
        if f.len() != 5 || f[0] != "250-FEE" || num(f[1])? != u64_at(&terms, 43) {
            return Err(RegistrationError::Invalid);
        }
        let fee = charge(num(f[1])?, f[2], f[3], f[4], true)?;
        let mut rounds = Vec::new();
        for n in 0..10 {
            let row = &lines[6 + n];
            if row.len() != 9
                || row[0] != "250-ROUND"
                || num(row[1])? != first + n as u64
                || num(row[3])? != u64_at(&terms, 3)
            {
                return Err(RegistrationError::Invalid);
            }
            let payment_id = bytes(row[2])?;
            if payment_id == [0; 32]
                || rounds
                    .iter()
                    .any(|r: &RegisteredRound| r.payment_id == payment_id)
            {
                return Err(RegistrationError::Invalid);
            }
            let (opens, closes) = match (row[4], row[5]) {
                ("-", "-") => (None, None),
                ("-", _) | (_, "-") => return Err(RegistrationError::Invalid),
                (a, b) => (Some(num(a)?), Some(num(b)?)),
            };
            if opens.zip(closes).map_or(false, |(o, c)| o > c) {
                return Err(RegistrationError::Invalid);
            }
            rounds.push(RegisteredRound {
                round: first + n as u64,
                payment_id,
                opens_in_ms: opens,
                closes_in_ms: closes,
                charge: charge(num(row[3])?, row[7], row[6], row[8], false)?,
            });
        }
        let result = Self {
            instance,
            handle,
            revision,
            period,
            phase,
            current_round,
            elapsed_ms,
            setup_remaining_ms,
            enforcement_remaining_ms,
            constructed_elapsed_ms,
            commit_elapsed_ms,
            fee,
            rounds: rounds.try_into().map_err(|_| RegistrationError::Invalid)?,
            terms,
            initial_allocation,
            allocation,
            binding,
        };
        result.validate_expected(expected)?;
        result.validate_state()?;
        Ok(result)
    }
    pub(crate) fn validate_for(&self, expected: &BatchRegistration) -> Result<()> {
        if self.revision == 0
            || self.period != expected.period
            || self.instance != expected.rounds[0].instance
            || self.handle != expected.rounds[0].handle
        {
            return Err(RegistrationError::Binding);
        }
        if self.fee.amount_msat != u64_at(&self.terms, 43) {
            return Err(RegistrationError::Invalid);
        }
        for (record, fee) in
            std::iter::once((&self.fee, true)).chain(self.rounds.iter().map(|r| (&r.charge, false)))
        {
            let hash = record.payment_hash.map(hex::encode);
            let decision = record.decision.map(hex::encode);
            let authorization = match record.authorization {
                RelayAuthorization::None => "NONE",
                RelayAuthorization::Authorized => "AUTHORIZED",
                RelayAuthorization::Consumed => "CONSUMED",
            };
            charge(
                record.amount_msat,
                hash.as_deref().unwrap_or("-"),
                decision.as_deref().unwrap_or("-"),
                authorization,
                fee,
            )?;
        }
        self.validate_expected(expected)?;
        self.validate_state()
    }
    fn validate_expected(&self, expected: &BatchRegistration) -> Result<()> {
        let q = self.policy_hash();
        for (wire, saved) in self.rounds.iter().zip(&expected.rounds) {
            if saved.instance != self.instance
                || saved.handle != self.handle
                || saved.circuit_binding != self.binding
                || saved.policy_hash != q
                || saved.allocation_hash != self.initial_allocation
                || saved.round != wire.round
                || saved.payment_id != wire.payment_id
                || saved.amount_msat != wire.charge.amount_msat
                || saved.kind != crate::payment_ledger::ChargeKind::Round
                || saved.window.is_some()
            {
                return Err(RegistrationError::Binding);
            }
        }
        if self.period == 0 {
            let flat: Vec<u8> = self.rounds.iter().flat_map(|r| r.payment_id).collect();
            let expected = hash(
                b"eltor/payment-renew/v1/initial",
                &[&self.binding, &q, &flat],
            );
            if self.initial_allocation != expected || self.allocation != expected {
                return Err(RegistrationError::Binding);
            }
        }
        Ok(())
    }
    fn validate_state(&self) -> Result<()> {
        let timeout = u32_at(&self.terms, 51);
        let grace = u32_at(&self.terms, 19);
        if matches!(
            self.phase,
            RelayPaymentPhase::Closed | RelayPaymentPhase::Expired
        ) && (self.setup_remaining_ms != 0 || self.enforcement_remaining_ms != 0)
        {
            return Err(RegistrationError::Invalid);
        }
        if self.current_round == 0 {
            if self.elapsed_ms != 0
                || self.enforcement_remaining_ms != 0
                || self.period != 0
                || matches!(
                    self.phase,
                    RelayPaymentPhase::Active | RelayPaymentPhase::Grace
                )
            {
                return Err(RegistrationError::Invalid);
            }
            let remaining = timeout.saturating_sub(self.constructed_elapsed_ms);
            if matches!(
                self.phase,
                RelayPaymentPhase::Committed | RelayPaymentPhase::WaitPayment
            ) && (remaining == 0 || self.setup_remaining_ms != remaining)
            {
                return Err(RegistrationError::Invalid);
            }
        } else {
            if self.setup_remaining_ms != 0
                || self.elapsed_ms > self.commit_elapsed_ms
                || matches!(
                    self.phase,
                    RelayPaymentPhase::Committed | RelayPaymentPhase::WaitPayment
                )
            {
                return Err(RegistrationError::Invalid);
            }
            if matches!(
                self.phase,
                RelayPaymentPhase::Active | RelayPaymentPhase::Grace
            ) {
                if self.current_round != self.elapsed_ms / 60_000 + 1
                    || self.elapsed_ms >= u64_at(&self.terms, 31)
                {
                    return Err(RegistrationError::Invalid);
                }
                let nominal = (self.current_round - 1)
                    .checked_mul(60_000)
                    .ok_or(RegistrationError::Invalid)?;
                let end = if self.phase == RelayPaymentPhase::Grace {
                    nominal.checked_add(grace)
                } else {
                    nominal
                        .checked_add(60_000)
                        .and_then(|n| n.checked_add(grace))
                }
                .ok_or(RegistrationError::Invalid)?
                .min(u64_at(&self.terms, 31));
                if self.enforcement_remaining_ms != end.saturating_sub(self.elapsed_ms)
                    || self.enforcement_remaining_ms == 0
                {
                    return Err(RegistrationError::Invalid);
                }
            }
        }
        if self.constructed_elapsed_ms - self.commit_elapsed_ms >= timeout {
            return Err(RegistrationError::Invalid);
        }
        let mut decisions = Vec::new();
        let mut hashes = Vec::new();
        for charge in std::iter::once(&self.fee).chain(self.rounds.iter().map(|r| &r.charge)) {
            if let Some(d) = charge.decision {
                if decisions.contains(&d) {
                    return Err(RegistrationError::Invalid);
                }
                decisions.push(d);
            }
            if let Some(h) = charge.payment_hash {
                if h != [0; 32] {
                    if hashes.contains(&h) {
                        return Err(RegistrationError::Invalid);
                    }
                    hashes.push(h);
                }
            }
            if self.current_round == 0 && charge.authorization == RelayAuthorization::Consumed {
                return Err(RegistrationError::Invalid);
            }
        }
        if matches!(
            self.phase,
            RelayPaymentPhase::Active | RelayPaymentPhase::Grace
        ) && self.fee.authorization != RelayAuthorization::Consumed
        {
            return Err(RegistrationError::Invalid);
        }
        for round in &self.rounds {
            let expected = if round.round == 1 {
                Some((
                    0,
                    u32_at(&self.terms, 51).saturating_sub(self.constructed_elapsed_ms),
                ))
            } else if self.current_round > 0 {
                let nominal = (round.round - 1)
                    .checked_mul(60_000)
                    .ok_or(RegistrationError::Invalid)?;
                Some((
                    nominal
                        .saturating_sub(60_000)
                        .saturating_sub(self.elapsed_ms),
                    nominal
                        .checked_add(grace)
                        .ok_or(RegistrationError::Invalid)?
                        .saturating_sub(self.elapsed_ms),
                ))
            } else {
                None
            };
            if round.opens_in_ms.zip(round.closes_in_ms) != expected {
                return Err(RegistrationError::Invalid);
            }
            if round.charge.authorization == RelayAuthorization::Consumed
                && round.round > self.current_round
            {
                return Err(RegistrationError::Invalid);
            }
            if self.current_round == round.round
                && matches!(
                    self.phase,
                    RelayPaymentPhase::Active | RelayPaymentPhase::Grace
                )
            {
                let needed = if self.phase == RelayPaymentPhase::Active {
                    RelayAuthorization::Consumed
                } else {
                    RelayAuthorization::None
                };
                if round.charge.authorization != needed {
                    return Err(RegistrationError::Invalid);
                }
            }
        }
        Ok(())
    }
    pub fn policy_hash(&self) -> [u8; 32] {
        hash(b"eltor/payment-renew/v1/terms", &[&self.terms])
    }
    pub fn setup_timeout_ms(&self) -> u64 {
        u32_at(&self.terms, 51)
    }
    pub fn grace_ms(&self) -> u64 {
        u32_at(&self.terms, 19)
    }
    pub fn lead_ms(&self) -> u64 {
        u32_at(&self.terms, 15)
    }
}

/// Independent trusted UTC observations at request dispatch and receipt, with
/// measured uncertainty (including timestamp precision), bracket one C sample.
/// Full monotonic round trip is retained; no half-RTT assumption is made.
#[derive(Clone, Copy)]
pub struct RegistrationClockSample {
    pub sent: ClockObservation,
    pub received: ClockObservation,
    pub round_trip: Duration,
}
pub type UtcInterval = UtcClockInterval;
impl RegistrationClockSample {
    fn sample_interval(&self) -> Result<UtcInterval> {
        if self.sent.now_ms < 0
            || self.received.now_ms < 0
            || self.sent.uncertainty_ms > 5000
            || self.received.uncertainty_ms > 5000
        {
            return Err(RegistrationError::Clock);
        }
        let a = i64::try_from(self.sent.uncertainty_ms).map_err(|_| RegistrationError::Clock)?;
        let b =
            i64::try_from(self.received.uncertainty_ms).map_err(|_| RegistrationError::Clock)?;
        let lo = self
            .sent
            .now_ms
            .checked_sub(a)
            .ok_or(RegistrationError::Clock)?;
        let hi = self
            .received
            .now_ms
            .checked_add(b)
            .ok_or(RegistrationError::Clock)?;
        let floor =
            i64::try_from(self.round_trip.as_millis()).map_err(|_| RegistrationError::Clock)?;
        let ceil = floor
            .checked_add(i64::from(self.round_trip.subsec_nanos() % 1_000_000 != 0))
            .ok_or(RegistrationError::Clock)?;
        let delta = self
            .received
            .now_ms
            .checked_sub(self.sent.now_ms)
            .ok_or(RegistrationError::Clock)?;
        if lo < 0
            || hi < lo
            || delta.checked_add(a + b).map_or(true, |n| n < 0)
            || delta.checked_sub(a + b).map_or(true, |n| n > ceil)
        {
            return Err(RegistrationError::Clock);
        }
        Ok(UtcInterval {
            earliest_ms: lo,
            latest_ms: hi,
        })
    }
    pub(crate) fn age(&self, age: u64) -> Result<UtcInterval> {
        let sample = self.sample_interval()?;
        let age = i64::try_from(age).map_err(|_| RegistrationError::Clock)?;
        let earliest_ms = sample
            .earliest_ms
            .checked_sub(age)
            .ok_or(RegistrationError::Clock)?;
        let latest_ms = sample
            .latest_ms
            .checked_sub(age)
            .ok_or(RegistrationError::Clock)?;
        if earliest_ms < 0 {
            return Err(RegistrationError::Clock);
        }
        Ok(UtcInterval {
            earliest_ms,
            latest_ms,
        })
    }
    pub fn construction(&self, snapshot: &RelayRegistration) -> Result<UtcInterval> {
        self.age(snapshot.constructed_elapsed_ms)
    }
    pub fn commitment(&self, snapshot: &RelayRegistration) -> Result<UtcInterval> {
        self.age(snapshot.commit_elapsed_ms)
    }
    /// At exact T0 elapsed_ms is zero but current_round=1 proves activation.
    pub fn activation(&self, snapshot: &RelayRegistration) -> Result<UtcInterval> {
        if snapshot.current_round == 0 {
            return Err(RegistrationError::NotActivated);
        }
        self.age(snapshot.elapsed_ms)
    }
    pub fn initial_anchor(&self, snapshot: &RelayRegistration) -> Result<InitialClockAnchor> {
        Ok(InitialClockAnchor {
            constructed_at: self.construction(snapshot)?,
            committed_at: self.commitment(snapshot)?,
            setup_timeout_ms: snapshot.setup_timeout_ms(),
        })
    }
    pub fn activation_anchor(
        &self,
        snapshot: &RelayRegistration,
    ) -> Result<ActivationIntervalAnchor> {
        Ok(ActivationIntervalAnchor {
            started_at: self.activation(snapshot)?,
            round_ms: 60_000,
            early_ms: snapshot.lead_ms(),
            grace_ms: snapshot.grace_ms(),
        })
    }
    pub fn initial_window(&self, snapshot: &RelayRegistration) -> Result<AcceptanceWindow> {
        let committed = self.commitment(snapshot)?;
        let constructed = self.construction(snapshot)?;
        let closes = constructed
            .earliest_ms
            .checked_add(
                i64::try_from(snapshot.setup_timeout_ms()).map_err(|_| RegistrationError::Clock)?,
            )
            .ok_or(RegistrationError::Clock)?;
        if committed.latest_ms > closes {
            return Err(RegistrationError::Clock);
        }
        Ok(AcceptanceWindow {
            opens_at_ms: committed.latest_ms,
            closes_at_ms: closes,
        })
    }
}
pub struct ObservedRelayRegistration {
    pub registration: RelayRegistration,
    pub clock: RegistrationClockSample,
}
pub struct RegistrationClockBinding {
    pub initial_window: AcceptanceWindow,
    pub activation: Option<ActivationIntervalAnchor>,
}
fn overlaps(a: UtcClockInterval, b: UtcClockInterval) -> bool {
    a.earliest_ms <= b.latest_ms && b.earliest_ms <= a.latest_ms
}
impl ObservedRelayRegistration {
    /// Bind a missing original anchor once. Later observations only check
    /// consistency and retain the original interval; they cannot narrow it,
    /// replace it, or turn a query into another start/setup event.
    pub fn bind_saved_clocks(
        &self,
        ledger: &mut PaymentLedger,
    ) -> std::result::Result<RegistrationClockBinding, Box<dyn std::error::Error>> {
        let s = &self.registration;
        let saved = ledger
            .get_reserved_batch_registration(s.instance, s.handle, s.period)?
            .ok_or(RegistrationError::Binding)?;
        s.validate_for(&saved)?;
        let initial = self.clock.initial_anchor(s)?;
        if let Some(old) = ledger.get_initial_clock_anchor(s.instance, s.handle)? {
            if old.setup_timeout_ms != initial.setup_timeout_ms
                || !overlaps(old.constructed_at, initial.constructed_at)
                || !overlaps(old.committed_at, initial.committed_at)
            {
                return Err(RegistrationError::Clock.into());
            }
        } else if ledger.get_initial_window(s.instance, s.handle)?.is_some() {
            // Migration preserves old point-only financial records, but cannot
            // invent missing UTC/RTT provenance or approve a wider live window.
            return Err(RegistrationError::LegacyClock.into());
        } else {
            ledger.bind_initial_clock_anchor(s.instance, s.handle, initial)?;
        }
        let initial_window = ledger
            .get_initial_window(s.instance, s.handle)?
            .ok_or(RegistrationError::Clock)?;
        let old = ledger.get_activation_interval_anchor(s.instance, s.handle)?;
        let activation = if s.current_round == 0 {
            if old.is_some() {
                return Err(RegistrationError::Clock.into());
            }
            None
        } else {
            let observed = self.clock.activation_anchor(s)?;
            if let Some(old) = old {
                if old.round_ms != observed.round_ms
                    || old.early_ms != observed.early_ms
                    || old.grace_ms != observed.grace_ms
                    || !overlaps(old.started_at, observed.started_at)
                {
                    return Err(RegistrationError::Clock.into());
                }
                Some(old)
            } else {
                ledger.bind_activation_interval_anchor(s.instance, s.handle, observed)?;
                Some(observed)
            }
        };
        Ok(RegistrationClockBinding {
            initial_window,
            activation,
        })
    }
}

impl super::PaymentControl {
    pub async fn relay_registration(
        &mut self,
        expected: &BatchRegistration,
    ) -> std::result::Result<RelayRegistration, Box<dyn std::error::Error>> {
        let first = &expected.rounds[0];
        if first.instance == [0; 16] || first.handle == [0; 16] {
            return Err(RegistrationError::Invalid.into());
        }
        let reply = self
            .command(&format!(
                "ELTOR_PAYMENT_REGISTRATION_GET 1 {} {} {}",
                hex::encode(first.instance),
                hex::encode(first.handle),
                expected.period
            ))
            .await?;
        match RelayRegistration::parse(&reply, expected) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                self.invalidate();
                Err(error.into())
            }
        }
    }
    /// Capture a trusted UTC bracket around the full paced control exchange.
    /// The provider must include read precision and clock accuracy uncertainty.
    pub async fn relay_registration_observed<F>(
        &mut self,
        expected: &BatchRegistration,
        mut observe: F,
    ) -> std::result::Result<ObservedRelayRegistration, Box<dyn std::error::Error>>
    where
        F: FnMut() -> std::result::Result<ClockObservation, Box<dyn std::error::Error>>,
    {
        let began = std::time::Instant::now();
        let sent = observe()?;
        let registration = self.relay_registration(expected).await?;
        let received = observe()?;
        let round_trip = began.elapsed();
        let clock = RegistrationClockSample {
            sent,
            received,
            round_trip,
        };
        if let Err(error) = clock.sample_interval() {
            self.invalidate();
            return Err(error.into());
        }
        Ok(ObservedRelayRegistration {
            registration,
            clock,
        })
    }
}
#[cfg(test)]
pub(crate) mod tests;
