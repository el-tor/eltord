//! Typed origin renewal controls. Local acceptance, authenticated admission and
//! activation are distinct facts; none invokes a wallet or authorizes spending.
//! Persist and claim the exact operation before PREPARE. A lost reply is recovered
//! with GET of that identity, never a replacement nonce or automatic PREPARE.
use super::{FoundationPolicy, OriginBinding, OriginRegistration, PaymentControl};
use sha2::{Digest, Sha256};
use std::{
    convert::TryFrom,
    io,
    time::{Duration, Instant},
};

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid origin renewal state")
}
fn number(value: &str) -> io::Result<u64> {
    let n: u64 = value.parse().map_err(|_| invalid())?;
    if n.to_string() != value {
        return Err(invalid());
    }
    Ok(n)
}
fn bytes<const N: usize>(value: &str) -> io::Result<[u8; N]> {
    if value.len() != 2 * N
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let mut out = [0; N];
    hex::decode_to_slice(value, &mut out).map_err(|_| invalid())?;
    if out == [0; N] {
        return Err(invalid());
    }
    Ok(out)
}
fn validate_binding(binding: &OriginBinding, hop: u16) -> io::Result<()> {
    if binding.instance == [0; 16]
        || binding.handle == [0; 16]
        || binding.revision == 0
        || binding.hops == 0
        || binding.hops > 16
        || hop == 0
        || hop > binding.hops
    {
        return Err(invalid());
    }
    Ok(())
}
fn same_binding(a: &OriginBinding, b: &OriginBinding) -> bool {
    a.instance == b.instance && a.handle == b.handle && a.hops == b.hops
}
fn domain_hash(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update((domain.len() as u16).to_be_bytes());
    hash.update(domain);
    for part in parts {
        hash.update(part);
    }
    hash.finalize().into()
}
fn ceil_ms(duration: Duration) -> io::Result<u64> {
    u64::try_from(duration.as_millis())
        .map_err(|_| invalid())?
        .checked_add(u64::from(duration.subsec_nanos() % 1_000_000 != 0))
        .ok_or_else(invalid)
}

/// Exact durable arguments. This type never generates identifiers or nonces.
#[derive(Clone, PartialEq, Eq)]
pub struct RenewalOperation {
    pub binding: OriginBinding,
    pub hop: u16,
    pub operation: [u8; 16],
    pub period: u64,
    pub first_round: u64,
    pub nonce: [u8; 16],
    pub terms: [u8; 32],
    pub parent: [u8; 32],
    pub ids: [[u8; 32]; 10],
}
impl RenewalOperation {
    pub fn validate(&self) -> io::Result<()> {
        validate_binding(&self.binding, self.hop)?;
        let first = self
            .period
            .checked_mul(10)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(invalid)?;
        if self.period == 0
            || first != self.first_round
            || self.operation == [0; 16]
            || self.nonce == [0; 16]
            || self.terms == [0; 32]
            || self.parent == [0; 32]
            || self.period.checked_mul(600_000).is_none()
        {
            return Err(invalid());
        }
        for (i, id) in self.ids.iter().enumerate() {
            if *id == [0; 32] || self.ids[..i].contains(id) {
                return Err(invalid());
            }
        }
        Ok(())
    }
    /// Canonical bytes to journal before any socket write; no CRLF is included.
    pub fn command(&self) -> io::Result<String> {
        self.validate()?;
        Ok(format!(
            "ELTOR_RENEW_PREPARE 1 {} {} {} {} {} {} {} {} {} {}",
            hex::encode(self.binding.instance),
            hex::encode(self.binding.handle),
            self.hop,
            hex::encode(self.operation),
            self.period,
            self.first_round,
            hex::encode(self.nonce),
            hex::encode(self.terms),
            hex::encode(self.parent),
            hex::encode(self.ids.iter().flatten().copied().collect::<Vec<_>>())
        ))
    }
    /// Recompute A_next from the exact public body and retained per-hop C.
    /// The request digest R includes Tor's private MAC and cannot be recreated
    /// by the daemon; it is pinned from Tor's authenticated local observation.
    pub fn expected_allocation(&self, circuit_binding: [u8; 32]) -> io::Result<[u8; 32]> {
        self.validate()?;
        if circuit_binding == [0; 32] {
            return Err(invalid());
        }
        let mut body = [0u8; 431];
        body[0] = 1;
        body[2..10].copy_from_slice(&self.period.to_be_bytes());
        body[10..18].copy_from_slice(&self.first_round.to_be_bytes());
        body[18] = 10;
        body[19..35].copy_from_slice(&self.nonce);
        body[35..43].copy_from_slice(&(self.period * 600_000).to_be_bytes());
        body[43..47].copy_from_slice(&60_000u32.to_be_bytes());
        body[47..79].copy_from_slice(&self.terms);
        body[79..111].copy_from_slice(&self.parent);
        for (slot, id) in body[111..].chunks_exact_mut(32).zip(self.ids) {
            slot.copy_from_slice(&id);
        }
        Ok(domain_hash(
            b"eltor/payment-renew/v1/allocation",
            &[&circuit_binding, &body],
        ))
    }
    fn receipt(&self, reply: &str) -> io::Result<u64> {
        self.validate()?;
        let row: Vec<_> = reply.split(' ').collect();
        if row.len() != 8
            || row[..3] != ["250", "ELTOR_RENEW_QUEUED", "1"]
            || bytes::<16>(row[3])? != self.binding.instance
            || bytes::<16>(row[4])? != self.binding.handle
            || number(row[5])? != u64::from(self.hop)
            || bytes::<16>(row[6])? != self.operation
        {
            return Err(invalid());
        }
        let revision = number(row[7])?;
        if revision < self.binding.revision {
            return Err(invalid());
        }
        Ok(revision)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenewalOperationPhase {
    Queued,
    Sent,
    Admitted,
    Active,
    Refused,
    Uncertain,
    Closed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RenewalRelayStatus {
    Admitted = 0,
    Active = 1,
    PolicyLimit = 2,
    TooLate = 3,
    Conflict = 4,
    BadParent = 5,
    TermsMismatch = 6,
    BadSequence = 7,
    Capacity = 8,
    Closing = 9,
}
impl RenewalRelayStatus {
    fn parse(value: &str) -> io::Result<Self> {
        Ok(match number(value)? {
            0 => Self::Admitted,
            1 => Self::Active,
            2 => Self::PolicyLimit,
            3 => Self::TooLate,
            4 => Self::Conflict,
            5 => Self::BadParent,
            6 => Self::TermsMismatch,
            7 => Self::BadSequence,
            8 => Self::Capacity,
            9 => Self::Closing,
            _ => return Err(invalid()),
        })
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct RenewalOperationStatus {
    pub operation: [u8; 16],
    pub phase: RenewalOperationPhase,
    pub period: u64,
    pub request_digest: [u8; 32],
    pub allocation_digest: Option<[u8; 32]>,
    pub relay_status: Option<RenewalRelayStatus>,
    pub transmissions: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenewalSchedule {
    pub round_remaining_ms: u64,
    pub admission_remaining_ms: Option<u64>,
    pub hard_age_remaining_ms: u64,
}
impl RenewalSchedule {
    fn subtract(self, elapsed: u64) -> Self {
        Self {
            round_remaining_ms: self.round_remaining_ms.saturating_sub(elapsed),
            admission_remaining_ms: self
                .admission_remaining_ms
                .map(|n| n.saturating_sub(elapsed)),
            hard_age_remaining_ms: self.hard_age_remaining_ms.saturating_sub(elapsed),
        }
    }
}

#[derive(Clone)]
pub struct RenewalOriginSnapshot {
    binding: OriginBinding,
    hop: u16,
    revision: u64,
    terms: [u8; 32],
    active_period: u64,
    active_allocation: [u8; 32],
    current_round: u64,
    operation: Option<RenewalOperationStatus>,
    schedule: RenewalSchedule,
    observed_at: Instant,
}
impl RenewalOriginSnapshot {
    /// Structural decoding of an already-authenticated control reply. This pure
    /// decoder does not attest the source of arbitrary text. The live RPC below
    /// additionally subtracts its full measured exchange time before returning.
    pub fn parse(
        reply: &str,
        binding: &OriginBinding,
        hop: u16,
        operation: Option<[u8; 16]>,
    ) -> io::Result<Self> {
        validate_binding(binding, hop)?;
        if reply.len() > 4096
            || operation == Some([0; 16])
            || reply
                .bytes()
                .any(|b| b != b'\n' && !(32..=126).contains(&b))
        {
            return Err(invalid());
        }
        let rows: Vec<Vec<_>> = reply
            .split('\n')
            .map(|line| line.split(' ').collect())
            .collect();
        if rows.len() != 5 || rows[4] != ["250", "OK"] {
            return Err(invalid());
        }
        let h = &rows[0];
        if h.len() != 6
            || h[..2] != ["250-ELTOR_PAYMENT", "1"]
            || bytes::<16>(h[2])? != binding.instance
            || bytes::<16>(h[3])? != binding.handle
            || number(h[4])? != u64::from(hop)
        {
            return Err(invalid());
        }
        let revision = number(h[5])?;
        if revision < binding.revision {
            return Err(invalid());
        }
        let p = &rows[1];
        if p.len() != 5 || p[0] != "250-PROFILE" {
            return Err(invalid());
        }
        let terms = bytes(p[1])?;
        let active_period = number(p[2])?;
        let active_allocation = bytes(p[3])?;
        let current_round = number(p[4])?;
        if current_round == 0 || (current_round - 1) / 10 != active_period {
            return Err(invalid());
        }
        let s = &rows[2];
        if s.len() != 4 || s[0] != "250-SCHEDULE" {
            return Err(invalid());
        }
        let schedule = RenewalSchedule {
            round_remaining_ms: number(s[1])?,
            admission_remaining_ms: if s[2] == "-" {
                None
            } else {
                Some(number(s[2])?)
            },
            hard_age_remaining_ms: number(s[3])?,
        };
        if schedule.round_remaining_ms > 60_000
            || schedule
                .admission_remaining_ms
                .map_or(false, |n| n > schedule.hard_age_remaining_ms)
        {
            return Err(invalid());
        }
        let o = &rows[3];
        if o.len() != 8 || o[0] != "250-OP" {
            return Err(invalid());
        }
        let status = if let Some(expected) = operation {
            let actual = bytes(o[1])?;
            if actual != expected {
                return Err(invalid());
            }
            let phase = match o[2] {
                "QUEUED" => RenewalOperationPhase::Queued,
                "SENT" => RenewalOperationPhase::Sent,
                "ADMITTED" => RenewalOperationPhase::Admitted,
                "ACTIVE" => RenewalOperationPhase::Active,
                "REFUSED" => RenewalOperationPhase::Refused,
                "UNCERTAIN" => RenewalOperationPhase::Uncertain,
                "CLOSED" => RenewalOperationPhase::Closed,
                _ => return Err(invalid()),
            };
            let period = number(o[3])?;
            if period == 0 || period.checked_mul(600_000).is_none() {
                return Err(invalid());
            }
            let request_digest = bytes(o[4])?;
            let allocation_digest = if o[5] == "-" {
                None
            } else {
                Some(bytes(o[5])?)
            };
            let relay_status = if o[6] == "-" {
                None
            } else {
                Some(RenewalRelayStatus::parse(o[6])?)
            };
            let transmissions = u8::try_from(number(o[7])?).map_err(|_| invalid())?;
            if transmissions > 4
                || (phase == RenewalOperationPhase::Queued && transmissions != 0)
                || (matches!(
                    phase,
                    RenewalOperationPhase::Sent
                        | RenewalOperationPhase::Admitted
                        | RenewalOperationPhase::Active
                        | RenewalOperationPhase::Refused
                ) && transmissions == 0)
            {
                return Err(invalid());
            }
            match phase {
                RenewalOperationPhase::Admitted
                    if relay_status == Some(RenewalRelayStatus::Admitted)
                        && allocation_digest.is_some() =>
                {
                    ()
                }
                RenewalOperationPhase::Active
                    if matches!(
                        relay_status,
                        Some(RenewalRelayStatus::Admitted | RenewalRelayStatus::Active)
                    ) && allocation_digest.is_some() =>
                {
                    ()
                }
                RenewalOperationPhase::Refused
                    if relay_status.map_or(false, |n| n as u8 >= 2)
                        && allocation_digest.is_none() =>
                {
                    ()
                }
                RenewalOperationPhase::Queued
                | RenewalOperationPhase::Sent
                | RenewalOperationPhase::Uncertain
                    if relay_status.is_none() && allocation_digest.is_none() =>
                {
                    ()
                }
                RenewalOperationPhase::Closed
                    if allocation_digest.is_some()
                        == relay_status.map_or(false, |n| n as u8 <= 1) =>
                {
                    ()
                }
                _ => return Err(invalid()),
            }
            Some(RenewalOperationStatus {
                operation: actual,
                phase,
                period,
                request_digest,
                allocation_digest,
                relay_status,
                transmissions,
            })
        } else {
            if o[1..] != ["-", "-", "-", "-", "-", "-", "-"] {
                return Err(invalid());
            }
            None
        };
        Ok(Self {
            binding: binding.clone(),
            hop,
            revision,
            terms,
            active_period,
            active_allocation,
            current_round,
            operation: status,
            schedule,
            observed_at: Instant::now(),
        })
    }
    pub fn binding(&self) -> &OriginBinding {
        &self.binding
    }
    pub fn hop(&self) -> u16 {
        self.hop
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn terms(&self) -> [u8; 32] {
        self.terms
    }
    pub fn active_period(&self) -> u64 {
        self.active_period
    }
    pub fn active_allocation(&self) -> [u8; 32] {
        self.active_allocation
    }
    pub fn current_round(&self) -> u64 {
        self.current_round
    }
    pub fn operation(&self) -> Option<&RenewalOperationStatus> {
        self.operation.as_ref()
    }
    pub fn schedule(&self) -> RenewalSchedule {
        self.schedule
            .subtract(ceil_ms(self.observed_at.elapsed()).unwrap_or(u64::MAX))
    }
    fn admission_period(&self) -> Option<u64> {
        if self.schedule.admission_remaining_ms.is_none() {
            return None;
        }
        if let Some(op) = &self.operation {
            if op.period > self.active_period && op.phase != RenewalOperationPhase::Refused {
                return Some(op.period);
            }
        }
        self.active_period.checked_add(1)
    }
    /// Within one process, retain exact request/result identity and monotonically
    /// narrow previously learned budgets. The durable coordinator still retains
    /// its original absolute deadlines across reconnect/restart.
    pub fn constrain_with_previous(&mut self, previous: &Self) -> io::Result<()> {
        if !same_binding(&self.binding, &previous.binding)
            || self.hop != previous.hop
            || self.terms != previous.terms
            || self.revision < previous.revision
            || self.active_period < previous.active_period
            || self.current_round < previous.current_round
            || (self.active_period == previous.active_period
                && self.active_allocation != previous.active_allocation)
        {
            return Err(invalid());
        }
        match (&previous.operation, &self.operation) {
            (None, None) => (),
            (Some(a), Some(b))
                if a.operation == b.operation
                    && a.period == b.period
                    && a.request_digest == b.request_digest
                    && a.transmissions <= b.transmissions
                    && (a.allocation_digest.is_none()
                        || a.allocation_digest == b.allocation_digest) =>
            {
                if (a.phase == RenewalOperationPhase::Refused && a != b)
                    || (a.phase == RenewalOperationPhase::Active
                        && !matches!(
                            b.phase,
                            RenewalOperationPhase::Active | RenewalOperationPhase::Closed
                        ))
                    || (a.relay_status == Some(RenewalRelayStatus::Active)
                        && b.relay_status != a.relay_status)
                    || (a.relay_status == Some(RenewalRelayStatus::Admitted)
                        && !matches!(
                            b.relay_status,
                            Some(RenewalRelayStatus::Admitted | RenewalRelayStatus::Active)
                        ))
                {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
        if self.revision == previous.revision
            && (self.active_period != previous.active_period
                || self.active_allocation != previous.active_allocation
                || self.current_round != previous.current_round
                || self.operation != previous.operation)
        {
            return Err(invalid());
        }
        let mut now = self.schedule();
        let old = previous.schedule();
        now.hard_age_remaining_ms = now.hard_age_remaining_ms.min(old.hard_age_remaining_ms);
        if self.current_round == previous.current_round {
            now.round_remaining_ms = now.round_remaining_ms.min(old.round_remaining_ms);
        }
        if self.admission_period() == previous.admission_period() {
            now.admission_remaining_ms =
                match (now.admission_remaining_ms, old.admission_remaining_ms) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    _ => None,
                };
        }
        self.schedule = now;
        self.observed_at = Instant::now();
        Ok(())
    }
    /// Admission records an exact allocation; a safe payment window, every-hop
    /// commitment and wallet authority remain separate coordinator/ledger gates.
    pub fn verify_admission(
        &self,
        operation: &RenewalOperation,
        registration: &OriginRegistration,
    ) -> io::Result<VerifiedRenewalAdmission> {
        operation.validate()?;
        let status = self.operation.as_ref().ok_or_else(invalid)?;
        let policy = FoundationPolicy::decode(&registration.terms())?;
        let end = operation
            .period
            .checked_add(1)
            .and_then(|n| n.checked_mul(600_000))
            .ok_or_else(invalid)?;
        let allocation = operation.expected_allocation(registration.circuit_binding())?;
        if !same_binding(&self.binding, &operation.binding)
            || !same_binding(&self.binding, &registration.binding)
            || self.hop != operation.hop
            || registration.hop != operation.hop
            || self.terms != operation.terms
            || self.terms != registration.policy_hash()
            || operation.period >= u64::from(policy.max_periods)
            || end > policy.max_age_ms
            || status.operation != operation.operation
            || status.period != operation.period
            || status.allocation_digest != Some(allocation)
            || !matches!(
                status.phase,
                RenewalOperationPhase::Admitted | RenewalOperationPhase::Active
            )
            || !matches!(
                status.relay_status,
                Some(RenewalRelayStatus::Admitted | RenewalRelayStatus::Active)
            )
            || (operation.period == 1 && operation.parent != registration.initial_allocation())
            || operation
                .ids
                .iter()
                .any(|id| registration.payment_ids().contains(id))
        {
            return Err(invalid());
        }
        match status.phase {
            RenewalOperationPhase::Admitted
                if self.active_period.checked_add(1) == Some(operation.period)
                    && self.active_allocation == operation.parent =>
            {
                ()
            }
            RenewalOperationPhase::Active
                if self.active_period == operation.period
                    && self.active_allocation == allocation =>
            {
                ()
            }
            _ => return Err(invalid()),
        }
        let mut schedule = self.schedule();
        if schedule.hard_age_remaining_ms > policy.max_age_ms {
            return Err(invalid());
        }
        if status.phase == RenewalOperationPhase::Active {
            // The C hop snapshot now reports admission for the NEXT allocation.
            // It must not become fresh admission time for this already-active one.
            schedule.admission_remaining_ms = Some(0);
        }
        Ok(VerifiedRenewalAdmission {
            operation: operation.clone(),
            terms: registration.terms(),
            circuit_binding: registration.circuit_binding(),
            request_digest: status.request_digest,
            allocation_digest: allocation,
            phase: status.phase,
            relay_status: status.relay_status.unwrap(),
            schedule,
            observed_at: Instant::now(),
        })
    }
}

/// Only a matching admitted/active typed snapshot can construct this value.
#[derive(Clone)]
pub struct VerifiedRenewalAdmission {
    operation: RenewalOperation,
    terms: [u8; 55],
    circuit_binding: [u8; 32],
    request_digest: [u8; 32],
    allocation_digest: [u8; 32],
    phase: RenewalOperationPhase,
    relay_status: RenewalRelayStatus,
    schedule: RenewalSchedule,
    observed_at: Instant,
}
impl VerifiedRenewalAdmission {
    pub fn operation(&self) -> &RenewalOperation {
        &self.operation
    }
    pub fn binding(&self) -> &OriginBinding {
        &self.operation.binding
    }
    pub fn hop(&self) -> u16 {
        self.operation.hop
    }
    pub fn period(&self) -> u64 {
        self.operation.period
    }
    pub fn first_round(&self) -> u64 {
        self.operation.first_round
    }
    pub fn nonce(&self) -> [u8; 16] {
        self.operation.nonce
    }
    pub fn terms(&self) -> [u8; 55] {
        self.terms
    }
    pub fn policy_hash(&self) -> [u8; 32] {
        self.operation.terms
    }
    pub fn circuit_binding(&self) -> [u8; 32] {
        self.circuit_binding
    }
    pub fn parent(&self) -> [u8; 32] {
        self.operation.parent
    }
    pub fn request_digest(&self) -> [u8; 32] {
        self.request_digest
    }
    pub fn allocation_digest(&self) -> [u8; 32] {
        self.allocation_digest
    }
    pub fn payment_ids(&self) -> &[[u8; 32]; 10] {
        &self.operation.ids
    }
    pub fn phase(&self) -> RenewalOperationPhase {
        self.phase
    }
    pub fn relay_status(&self) -> RenewalRelayStatus {
        self.relay_status
    }
    pub fn schedule(&self) -> RenewalSchedule {
        self.schedule
            .subtract(ceil_ms(self.observed_at.elapsed()).unwrap_or(u64::MAX))
    }
}
impl PaymentControl {
    /// Caller must durably save/claim these exact arguments before dispatch.
    /// Never automatically resends PREPARE, generates IDs, or contacts a wallet.
    pub async fn renewal_prepare(&mut self, operation: &RenewalOperation) -> io::Result<u64> {
        let reply = self.command(&operation.command()?).await?;
        operation.receipt(&reply).map_err(|error| {
            self.invalidate();
            error
        })
    }
    /// Strictly local query; it never creates a relay request or retries one.
    pub async fn renewal_snapshot(
        &mut self,
        binding: &OriginBinding,
        hop: u16,
        operation: Option<[u8; 16]>,
    ) -> io::Result<RenewalOriginSnapshot> {
        validate_binding(binding, hop)?;
        if operation == Some([0; 16]) {
            return Err(invalid());
        }
        let sent = Instant::now();
        let reply = self
            .command(&format!(
                "ELTOR_PAYMENT_GET 1 {} {} {} {}",
                hex::encode(binding.instance),
                hex::encode(binding.handle),
                hop,
                operation.map(hex::encode).unwrap_or_else(|| "-".into())
            ))
            .await?;
        let elapsed = ceil_ms(sent.elapsed())?;
        RenewalOriginSnapshot::parse(&reply, binding, hop, operation)
            .map(|mut snapshot| {
                snapshot.schedule = snapshot.schedule.subtract(elapsed);
                snapshot
            })
            .map_err(|error| {
                self.invalidate();
                error
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        convert::TryInto,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::{TcpListener, TcpStream},
    };
    fn decode<const N: usize>(value: &str) -> [u8; N] {
        hex::decode(value).unwrap().try_into().unwrap()
    }
    fn fixture() -> (RenewalOperation, OriginRegistration, [u8; 32], [u8; 32]) {
        let json: serde_json::Value =
            serde_json::from_str(include_str!("../../spec/vectors/payment_renewal_v1.json"))
                .unwrap();
        let f = &json["fixtures"][0];
        let p = &f["periods"][0];
        let binding = OriginBinding {
            instance: [1; 16],
            handle: [2; 16],
            hops: 3,
            revision: 1,
        };
        let q = decode(f["terms_digest_hex"].as_str().unwrap());
        let parent = decode(p["parent_digest_hex"].as_str().unwrap());
        let old_ids = f["initial_ids_hex"].as_str().unwrap();
        let initial_ids: [[u8; 32]; 10] =
            std::array::from_fn(|n| decode(&old_ids[n * 64..(n + 1) * 64]));
        let mut registration = format!(
            "250-ELTOR_INITIAL_REGISTRATION 1 {} {} 2 3\n250-POLICY {} {} {}\n250-BINDING {}\n",
            hex::encode(binding.instance),
            hex::encode(binding.handle),
            f["terms_hex"].as_str().unwrap(),
            hex::encode(q),
            hex::encode(parent),
            f["C_hex"].as_str().unwrap()
        );
        for (i, id) in initial_ids.iter().enumerate() {
            registration += &format!("250-ROUND {} {}\n", i + 1, hex::encode(id));
        }
        registration += "250 OK";
        let registration =
            OriginRegistration::parse(&registration, &binding, 2, &initial_ids).unwrap();
        let ids = p["new_ids_hex"].as_str().unwrap();
        let operation = RenewalOperation {
            binding,
            hop: 2,
            operation: [7; 16],
            period: 1,
            first_round: 11,
            nonce: decode(p["renewal_nonce_hex"].as_str().unwrap()),
            terms: q,
            parent,
            ids: std::array::from_fn(|n| decode(&ids[n * 64..(n + 1) * 64])),
        };
        (
            operation,
            registration,
            decode(p["request_digest_hex"].as_str().unwrap()),
            decode(p["allocation_digest_hex"].as_str().unwrap()),
        )
    }
    fn wire(op: &RenewalOperation, request: [u8; 32], allocation: [u8; 32], phase: &str) -> String {
        let active = phase == "ACTIVE";
        let (period, current_round, profile_allocation) = if active {
            (1, 11, allocation)
        } else {
            (0, 7, op.parent)
        };
        let (digest, status, transmissions) = match phase {
            "ADMITTED" => (hex::encode(allocation), "0", 1),
            "ACTIVE" => (hex::encode(allocation), "1", 1),
            "REFUSED" => ("-".into(), "8", 1),
            "QUEUED" => ("-".into(), "-", 0),
            _ => ("-".into(), "-", 1),
        };
        format!("250-ELTOR_PAYMENT 1 {} {} 2 5\n250-PROFILE {} {} {} {}\n250-SCHEDULE 60000 180000 86000000\n250-OP {} {} 1 {} {} {} {}\n250 OK",
            hex::encode(op.binding.instance),hex::encode(op.binding.handle),hex::encode(op.terms),period,
            hex::encode(profile_allocation),current_round,hex::encode(op.operation),phase,
            hex::encode(request),digest,status,transmissions)
    }
    fn parsed(
        op: &RenewalOperation,
        request: [u8; 32],
        allocation: [u8; 32],
        phase: &str,
    ) -> RenewalOriginSnapshot {
        RenewalOriginSnapshot::parse(
            &wire(op, request, allocation, phase),
            &op.binding,
            op.hop,
            Some(op.operation),
        )
        .unwrap()
    }
    #[test]
    fn canonical_operation_and_allocation_match_public_renewal_vectors() {
        let json: serde_json::Value =
            serde_json::from_str(include_str!("../../spec/vectors/payment_renewal_v1.json"))
                .unwrap();
        let (base, _, _, _) = fixture();
        for f in json["fixtures"].as_array().unwrap() {
            for p in f["periods"].as_array().unwrap() {
                let ids = p["new_ids_hex"].as_str().unwrap();
                let op = RenewalOperation {
                    period: p["period"].as_u64().unwrap(),
                    first_round: p["first_round"].as_u64().unwrap(),
                    nonce: decode(p["renewal_nonce_hex"].as_str().unwrap()),
                    terms: decode(f["terms_digest_hex"].as_str().unwrap()),
                    parent: decode(p["parent_digest_hex"].as_str().unwrap()),
                    ids: std::array::from_fn(|n| decode(&ids[n * 64..(n + 1) * 64])),
                    ..base.clone()
                };
                assert_eq!(
                    op.expected_allocation(decode(f["C_hex"].as_str().unwrap()))
                        .unwrap(),
                    decode::<32>(p["allocation_digest_hex"].as_str().unwrap())
                );
                let command = op.command().unwrap();
                assert_eq!(command.split(' ').count(), 12);
                assert!(!command.contains('\r') && !command.contains('\n'));
                assert!(command.len() + 2 < 16384);
                assert!(command.ends_with(ids));
            }
        }
        let receipt = format!(
            "250 ELTOR_RENEW_QUEUED 1 {} {} 2 {} 7",
            hex::encode(base.binding.instance),
            hex::encode(base.binding.handle),
            hex::encode(base.operation)
        );
        assert_eq!(base.receipt(&receipt).unwrap(), 7);
        for bad in [
            receipt.replace(" 2 ", " 1 "),
            receipt.replace(" 7", " 07"),
            receipt.replace(" 7", " 0"),
            format!("{} extra", receipt),
        ] {
            assert!(base.receipt(&bad).is_err());
        }
    }
    #[test]
    fn invalid_arguments_fail_before_dispatch() {
        let (op, _, _, _) = fixture();
        let mut cases = Vec::new();
        let mut bad = op.clone();
        bad.hop = 0;
        cases.push(bad);
        let mut bad = op.clone();
        bad.binding.hops = 17;
        cases.push(bad);
        let mut bad = op.clone();
        bad.binding.instance = [0; 16];
        cases.push(bad);
        let mut bad = op.clone();
        bad.operation = [0; 16];
        cases.push(bad);
        let mut bad = op.clone();
        bad.nonce = [0; 16];
        cases.push(bad);
        let mut bad = op.clone();
        bad.period = 0;
        cases.push(bad);
        let mut bad = op.clone();
        bad.period = u64::MAX;
        cases.push(bad);
        let mut bad = op.clone();
        bad.first_round = 10;
        cases.push(bad);
        let mut bad = op.clone();
        bad.terms = [0; 32];
        cases.push(bad);
        let mut bad = op.clone();
        bad.parent = [0; 32];
        cases.push(bad);
        let mut bad = op.clone();
        bad.ids[0] = [0; 32];
        cases.push(bad);
        let mut bad = op.clone();
        bad.ids[0] = bad.ids[1];
        cases.push(bad);
        for bad in cases {
            assert!(bad.command().is_err());
        }
    }
    #[test]
    fn admission_requires_matching_args_and_authenticated_success_phase() {
        let (op, registration, r, a) = fixture();
        let snapshot = parsed(&op, r, a, "ADMITTED");
        let verified = snapshot.verify_admission(&op, &registration).unwrap();
        assert!(verified.operation() == &op);
        assert_eq!(verified.request_digest(), r);
        assert_eq!(verified.allocation_digest(), a);
        assert_eq!(verified.parent(), op.parent);
        assert_eq!(verified.payment_ids(), &op.ids);
        assert_eq!(verified.terms(), registration.terms());
        let active = parsed(&op, r, a, "ACTIVE")
            .verify_admission(&op, &registration)
            .unwrap();
        assert_eq!(active.schedule().admission_remaining_ms, Some(0));
        // ACTIVE may be learned through a matching authenticated START status
        // while the original authenticated RENEW result remains ADMITTED(0).
        let active_from_status = wire(&op, r, a, "ACTIVE").replace(" 1 1\n250 OK", " 0 1\n250 OK");
        assert!(RenewalOriginSnapshot::parse(
            &active_from_status,
            &op.binding,
            op.hop,
            Some(op.operation)
        )
        .unwrap()
        .verify_admission(&op, &registration)
        .is_ok());
        for phase in ["QUEUED", "SENT", "REFUSED", "UNCERTAIN", "CLOSED"] {
            assert!(parsed(&op, r, a, phase)
                .verify_admission(&op, &registration)
                .is_err());
        }
        for index in 0..6 {
            let mut wrong = op.clone();
            match index {
                0 => wrong.nonce[0] ^= 1,
                1 => wrong.parent[0] ^= 1,
                2 => wrong.ids.swap(0, 1),
                3 => wrong.hop = 1,
                4 => wrong.operation[0] ^= 1,
                _ => wrong.terms[0] ^= 1,
            }
            assert!(snapshot.verify_admission(&wrong, &registration).is_err());
        }
    }
    #[test]
    fn malformed_or_incoherent_snapshots_are_rejected() {
        let (op, _, r, a) = fixture();
        let good = wire(&op, r, a, "ADMITTED");
        let bad = vec![
            good.replace(" 2 5\n", " 1 5\n"),
            good.replace(" 2 5\n", " 2 05\n"),
            good.replace(" 2 5\n", " 2 0\n"),
            good.replace("ADMITTED", "VERIFIED"),
            good.replace(" 0 1\n250 OK", " 10 1\n250 OK"),
            good.replace(" 0 1\n250 OK", " 0 5\n250 OK"),
            good.replace(" 0 1\n250 OK", " - 1\n250 OK"),
            good.replace(&hex::encode(a), "-"),
            good.replace("250-SCHEDULE 60000", "250-SCHEDULE 60001"),
            good.replace("180000 86000000", "86000001 86000000"),
            good.replace(" 7\n250-SCHEDULE", " 0\n250-SCHEDULE"),
            good.replace("250-PROFILE ", "250-PROFILE  "),
            good.replace(
                &hex::encode(op.terms),
                &hex::encode(op.terms).to_uppercase(),
            ),
            good.replace("\n250 OK", "\n250-EXTRA 1\n250 OK"),
            format!("{}\n", good),
            good.replace("250 OK", "250 OK\r"),
        ];
        for value in bad {
            assert!(
                RenewalOriginSnapshot::parse(&value, &op.binding, op.hop, Some(op.operation))
                    .is_err(),
                "malformed variant unexpectedly accepted"
            );
        }
        assert!(RenewalOriginSnapshot::parse(&good, &op.binding, op.hop, Some([8; 16])).is_err());
        assert!(RenewalOriginSnapshot::parse(&good, &op.binding, op.hop, None).is_err());
        let none = good
            .lines()
            .enumerate()
            .map(|(i, line)| if i == 3 { "250-OP - - - - - - -" } else { line })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            RenewalOriginSnapshot::parse(&none, &op.binding, op.hop, None)
                .unwrap()
                .operation()
                .is_none()
        );
    }
    #[test]
    fn replayed_observations_cannot_replace_request_or_extend_deadlines() {
        let (op, _, r, a) = fixture();
        let mut old = parsed(&op, r, a, "ADMITTED");
        old.schedule = RenewalSchedule {
            round_remaining_ms: 10000,
            admission_remaining_ms: Some(10000),
            hard_age_remaining_ms: 20000,
        };
        old.observed_at = Instant::now() - Duration::from_secs(1);
        let mut fresh = parsed(&op, r, a, "ADMITTED");
        fresh.constrain_with_previous(&old).unwrap();
        let safe = fresh.schedule();
        assert!(safe.round_remaining_ms <= 9000);
        assert!(safe.admission_remaining_ms.unwrap() <= 9000);
        assert!(safe.hard_age_remaining_ms <= 19000);
        let mut conflict = parsed(&op, [77; 32], a, "ADMITTED");
        assert!(conflict.constrain_with_previous(&old).is_err());
        let mut stale = parsed(&op, r, a, "ADMITTED");
        stale.revision = 4;
        assert!(stale.constrain_with_previous(&old).is_err());
        let refused = parsed(&op, r, a, "REFUSED");
        let mut success = parsed(&op, r, a, "ADMITTED");
        success.revision = 6;
        assert!(success.constrain_with_previous(&refused).is_err());
        let mut changed = parsed(&op, r, a, "ACTIVE");
        assert!(changed.constrain_with_previous(&old).is_err()); // changed facts at same local revision
        changed.revision = 6;
        changed.constrain_with_previous(&old).unwrap();
    }
    async fn line(socket: &mut BufReader<TcpStream>) -> String {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), socket.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        line
    }
    async fn authenticate(socket: &mut BufReader<TcpStream>) {
        for expected in ["AUTHENTICATE\r\n", "SETEVENTS ELTOR_PAYMENT_STATE\r\n"] {
            assert_eq!(line(socket).await, expected);
            socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
        }
    }
    fn config(listener: &TcpListener) -> crate::types::RpcConfig {
        crate::types::RpcConfig {
            addr: listener.local_addr().unwrap().to_string(),
            rpc_password: None,
            command: String::new(),
        }
    }
    #[tokio::test]
    async fn lost_prepare_reply_recovers_only_the_exact_operation_with_get() {
        for state in ["UNKNOWN", "SENT", "ADMITTED"] {
            let (op, _, r, a) = fixture();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let config = config(&listener);
            let expected = op.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let written = calls.clone();
            let worker = tokio::spawn(async move {
                let mut first = BufReader::new(listener.accept().await.unwrap().0);
                authenticate(&mut first).await;
                assert_eq!(line(&mut first).await, expected.command().unwrap() + "\r\n");
                written.fetch_add(1, Ordering::SeqCst);
                drop(first); // native Tor may have accepted it; the receipt is lost.
                let mut second = BufReader::new(listener.accept().await.unwrap().0);
                authenticate(&mut second).await;
                assert_eq!(
                    line(&mut second).await,
                    format!(
                        "ELTOR_PAYMENT_GET 1 {} {} 2 {}\r\n",
                        hex::encode(expected.binding.instance),
                        hex::encode(expected.binding.handle),
                        hex::encode(expected.operation)
                    )
                );
                let response = if state == "UNKNOWN" {
                    "552 UNKNOWN_OPERATION".into()
                } else {
                    wire(&expected, r, a, state)
                };
                second
                    .get_mut()
                    .write_all((response.replace('\n', "\r\n") + "\r\n").as_bytes())
                    .await
                    .unwrap();
                assert_eq!(line(&mut second).await, ""); // no automatic PREPARE/START/payment follows.
            });
            let mut first = PaymentControl::connect(&config).await.unwrap();
            assert!(first.renewal_prepare(&op).await.is_err());
            drop(first);
            let mut recovered = PaymentControl::connect(&config).await.unwrap();
            let result = recovered
                .renewal_snapshot(&op.binding, op.hop, Some(op.operation))
                .await;
            if state == "UNKNOWN" {
                assert_eq!(
                    super::super::PaymentRejection::from_io(&result.err().unwrap()),
                    Some(super::super::PaymentRejection::UnknownOperation)
                );
            } else {
                assert!(result.is_ok());
            }
            drop(recovered);
            worker.await.unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }
    #[tokio::test]
    async fn malformed_receipt_poisons_connection_without_a_second_socket_write() {
        let (op, _, _, _) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = config(&listener);
        let expected = op.clone();
        let worker = tokio::spawn(async move {
            let mut socket = BufReader::new(listener.accept().await.unwrap().0);
            authenticate(&mut socket).await;
            assert_eq!(
                line(&mut socket).await,
                expected.command().unwrap() + "\r\n"
            );
            socket
                .get_mut()
                .write_all(
                    format!(
                        "250 ELTOR_RENEW_QUEUED 1 {} {} 2 {} 7\r\n",
                        hex::encode(expected.binding.instance),
                        hex::encode(expected.binding.handle),
                        hex::encode([9; 16])
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            assert_eq!(line(&mut socket).await, "");
        });
        let mut control = PaymentControl::connect(&config).await.unwrap();
        assert!(control.renewal_prepare(&op).await.is_err());
        assert!(control
            .renewal_snapshot(&op.binding, op.hop, Some(op.operation))
            .await
            .is_err());
        drop(control);
        worker.await.unwrap();
    }
    #[tokio::test]
    async fn read_only_get_preserves_capacity_errors_and_subtracts_exchange_time() {
        let (op, registration, r, a) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = config(&listener);
        let expected = op.clone();
        let worker = tokio::spawn(async move {
            let mut socket = BufReader::new(listener.accept().await.unwrap().0);
            authenticate(&mut socket).await;
            let get = format!(
                "ELTOR_PAYMENT_GET 1 {} {} 2 {}\r\n",
                hex::encode(expected.binding.instance),
                hex::encode(expected.binding.handle),
                hex::encode(expected.operation)
            );
            assert_eq!(line(&mut socket).await, get);
            socket
                .get_mut()
                .write_all(b"451 CAPACITY\r\n")
                .await
                .unwrap();
            assert_eq!(line(&mut socket).await, get);
            tokio::time::sleep(Duration::from_millis(20)).await;
            socket
                .get_mut()
                .write_all(
                    (wire(&expected, r, a, "ADMITTED")
                        .replace("60000 180000 86000000", "1000 1000 1000")
                        .replace('\n', "\r\n")
                        + "\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            assert_eq!(line(&mut socket).await, "");
        });
        let mut control = PaymentControl::connect(&config).await.unwrap();
        let rejection = control
            .renewal_snapshot(&op.binding, op.hop, Some(op.operation))
            .await
            .err()
            .unwrap();
        assert_eq!(
            super::super::PaymentRejection::from_io(&rejection),
            Some(super::super::PaymentRejection::Capacity)
        );
        let snapshot = control
            .renewal_snapshot(&op.binding, op.hop, Some(op.operation))
            .await
            .unwrap();
        assert!(snapshot.schedule().hard_age_remaining_ms <= 980);
        let verified = snapshot.verify_admission(&op, &registration).unwrap();
        assert!(verified.schedule().admission_remaining_ms.unwrap() <= 980);
        drop(control);
        worker.await.unwrap();
    }
}
