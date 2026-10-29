//! Durable account accounting primitives for the paid-circuit foundation.
//!
//! This module is not connected to the legacy payment watcher or Tor admission.
//! The caller must authenticate the recipient account, immutable Tor registration,
//! native wallet evidence and monotonic/UTC clock anchor before calling it. A
//! caller-supplied account label, callback, payer note or preimage is not evidence.
//! All workers for an account must use the same authoritative database. Provision
//! is an explicit operator action for a genuinely new account, never recovery
//! from lost history. Reopen never creates or repairs missing/corrupt state.
//!
//! No preimages, invoice strings or traffic keys are stored. Identifying records
//! deliberately do not implement Debug. No retention/deletion API is provided:
//! unresolved accounting and payment-ID reservations must not be evicted.

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    convert::{TryFrom, TryInto},
    fs::OpenOptions,
    path::Path,
    time::Duration,
};
use thiserror::Error;

pub mod clock;
pub use clock::{ActivationIntervalAnchor, InitialClockAnchor, UtcClockInterval};
pub mod outgoing;
pub mod origin;
pub mod receiving;
mod migration;
pub mod storage;
pub mod wallet;

pub type Hash32 = [u8; 32];
pub type Id16 = [u8; 16];
pub(crate) const APPLICATION_ID: i64 = 0x454c5452;
pub(crate) const VERSION: i64 = 5;

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("payment ledger storage unavailable")]
    Storage(#[from] rusqlite::Error),
    #[error("payment ledger file unavailable")]
    File(#[from] std::io::Error),
    #[error("unrecognized or inconsistent payment ledger")]
    InvalidLedger,
    #[error("invalid paid obligation or evidence")]
    InvalidInput,
    #[error("immutable payment operation conflicts")]
    Conflict,
    #[error("payment obligation is not registered")]
    NotRegistered,
    #[error("payment obligation is closed")]
    Closed,
    #[error("payment account does not match authoritative ledger")]
    WrongAccount,
    #[error("recipient evidence does not match the obligation")]
    InvalidEvidence,
    #[error("settlement or authorization is outside its certain acceptance window")]
    OutsideWindow,
    #[error("financial exposure or wallet work budget exhausted")]
    BudgetExceeded,
    #[error("durable ledger storage pressure; admission or dispatch refused")]
    StoragePressure,
    #[error("wallet dispatch was already claimed; reconcile its outcome")]
    DispatchAlreadyClaimed,
    #[error("operation unsupported by the paid-circuit foundation")]
    UnsupportedOperation,
}

type Result<T> = std::result::Result<T, LedgerError>;

/// Fingerprint of an adapter-authenticated account identity and network. This
/// type does not itself authenticate identity; labels and generic NWC service
/// pubkeys are insufficient. The adapter must establish the actual account.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountId(pub Hash32);

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChargeKind {
    Handshake,
    Round,
}

impl ChargeKind {
    fn code(self) -> i64 {
        match self {
            Self::Handshake => 0,
            Self::Round => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceWindow {
    pub opens_at_ms: i64,
    pub closes_at_ms: i64,
}

/// Immutable obligation received from an authenticated Tor registration. Round
/// zero is HANDSHAKE; positive rounds are ROUND. `expected_payment_hash` binds a
/// known invoice. When absent for a positive charge, the adapter must prove the
/// exact payment-ID association in its authenticated recipient invoice record.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obligation {
    pub instance: Id16,
    pub handle: Id16,
    pub circuit_binding: Hash32,
    pub round: u64,
    pub kind: ChargeKind,
    pub payment_id: Hash32,
    pub amount_msat: u64,
    pub expected_payment_hash: Option<Hash32>,
    pub policy_hash: Hash32,
    pub allocation_hash: Hash32,
    /// Handshake's registered window. Recurring rounds use None until COMMIT
    /// (round one) or the immutable activation anchor (later rounds) exists.
    pub window: Option<AcceptanceWindow>,
}

impl Obligation {
    fn validate(&self) -> Result<()> {
        if self.instance == [0; 16]
            || self.handle == [0; 16]
            || self.circuit_binding == [0; 32]
            || self.payment_id == [0; 32]
            || (self.round == 0) != (self.kind == ChargeKind::Handshake)
            || (self.kind == ChargeKind::Handshake) != self.window.is_some()
            || self.window.map_or(false, |w| {
                w.opens_at_ms < 0 || w.closes_at_ms < w.opens_at_ms
            })
            || self.expected_payment_hash == Some([0; 32])
            || (self.amount_msat == 0 && self.expected_payment_hash.is_some())
        {
            return Err(LedgerError::InvalidInput);
        }
        sql_number(self.round)?;
        sql_number(self.amount_msat)?;
        Ok(())
    }
}

/// All ten round registrations for one immutable admission request, in order.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchRegistration {
    pub period: u64,
    pub request_digest: Hash32,
    pub rounds: [Obligation; 10],
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdmissionOutcome {
    Reserved,
    Conflict,
}

#[derive(Clone, PartialEq, Eq)]
pub struct BatchReservation {
    pub instance: Id16,
    pub handle: Id16,
    pub period: u64,
    pub request_digest: Hash32,
    pub decision: Id16,
    pub outcome: AdmissionOutcome,
}

/// A trusted adapter must obtain these fields by exact authenticated recipient
/// lookup, not from a webhook or a general search result. In particular,
/// `authenticated_payment_id` asserts a verified native invoice association;
/// this module does not make an unauthenticated payer note trustworthy.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipientEvidence {
    pub account: AccountId,
    pub payment_hash: Hash32,
    pub invoice_digest: Hash32,
    pub amount_msat: u64,
    pub settled_at_ms: i64,
    /// Bound the timestamp's precision/error in milliseconds. For a native
    /// seconds-floor timestamp, checked-multiply by 1000 and supply at least
    /// 999ms uncertainty; this symmetric bound is deliberately conservative.
    pub settlement_uncertainty_ms: u64,
    pub incoming: bool,
    pub lightning: bool,
    pub completed: bool,
    pub authenticated_payment_id: Option<Hash32>,
}

/// Trusted UTC observation bounded against the live registration's monotonic
/// deadline. After restart the caller must re-establish that anchor or refuse.
#[derive(Clone, Copy)]
pub struct ClockObservation {
    pub now_ms: i64,
    pub uncertainty_ms: u64,
}

/// Original authenticated T0 and accepted timing profile. The minute/advance/
/// grace maxima are the version-one test profile, not measured production defaults.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationAnchor {
    pub started_at_ms: i64,
    pub round_ms: u64,
    pub early_ms: u64,
    pub grace_ms: u64,
}

impl ActivationAnchor {
    fn validate(self) -> Result<()> {
        if self.started_at_ms < 0
            || self.round_ms != 60_000
            || self.early_ms > 60_000
            || self.grace_ms > 60_000
        {
            return Err(LedgerError::InvalidInput);
        }
        Ok(())
    }

    fn window(self, round: u64) -> Result<AcceptanceWindow> {
        if round < 2 {
            return Err(LedgerError::InvalidInput);
        }
        let offset = (round - 1)
            .checked_mul(self.round_ms)
            .ok_or(LedgerError::OutsideWindow)?;
        let nominal = self
            .started_at_ms
            .checked_add(sql_number(offset)?)
            .ok_or(LedgerError::OutsideWindow)?;
        Ok(AcceptanceWindow {
            opens_at_ms: nominal
                .checked_sub(sql_number(self.early_ms)?)
                .ok_or(LedgerError::OutsideWindow)?,
            closes_at_ms: nominal
                .checked_add(sql_number(self.grace_ms)?)
                .ok_or(LedgerError::OutsideWindow)?,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationState {
    Authorized,
    Consumed,
    ClosedUnresolved,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Authorization {
    pub decision: Id16,
    pub record_digest: Hash32,
    pub payment_hash: Hash32,
    pub state: AuthorizationState,
}

/// Synchronous SQLite boundary; run on a blocking worker, not the async reactor.
pub struct PaymentLedger {
    connection: Connection,
    account: AccountId,
    path: std::path::PathBuf,
    file_identity: storage::FileIdentity,
}

impl PaymentLedger {
    /// Exclusively create a new authoritative ledger. Existing files, including
    /// empty/corrupt files, are never overwritten. A failed provision leaves its
    /// file in place for inspection, requiring explicit operator recovery.
    pub fn provision(path: &Path, account: AccountId) -> Result<Self> {
        Self::provision_with_storage_budget(path, account, storage::DEFAULT_BUDGET_BYTES)
    }

    /// Exclusive creation with a smaller immutable local storage budget. The
    /// profile ceiling remains 1GiB; reopening never changes accepted limits.
    pub fn provision_with_storage_budget(
        path: &Path,
        account: AccountId,
        budget_bytes: u64,
    ) -> Result<Self> {
        storage::validate_budget(budget_bytes)?;
        if account.0 == [0; 32] {
            return Err(LedgerError::InvalidInput);
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        file.sync_all()?;
        let mut ledger = Self::connect(path, account)?;
        let tx = ledger
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        tx.execute_batch(outgoing::SCHEMA)?;
        storage::initialize(&tx, budget_bytes)?;
        tx.execute_batch(clock::SCHEMA)?;
        tx.execute_batch(receiving::SCHEMA)?;
        tx.execute_batch(origin::SCHEMA)?;
        tx.execute(
            "INSERT INTO ledger_info(singleton, account) VALUES(1, ?1)",
            params![&account.0[..]],
        )?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
        storage::validate_schema(&ledger.connection)?;
        // Persist the newly created directory entry as well as SQLite's commit.
        #[cfg(unix)]
        {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(ledger)
    }

    pub fn reopen(path: &Path, expected_account: AccountId) -> Result<Self> {
        let ledger = Self::connect(path, expected_account)?;
        let version: i64 = ledger
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        let application: i64 =
            ledger
                .connection
                .pragma_query_value(None, "application_id", |r| r.get(0))?;
        if version != VERSION || application != APPLICATION_ID {
            return Err(LedgerError::InvalidLedger);
        }
        let health: String = ledger
            .connection
            .query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if health != "ok" {
            return Err(LedgerError::InvalidLedger);
        }
        let account: Vec<u8> = ledger.connection.query_row(
            "SELECT account FROM ledger_info WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        if account != expected_account.0 {
            return Err(LedgerError::WrongAccount);
        }
        // Validate required tables without modifying an existing schema.
        ledger.connection.prepare("SELECT r.id, b.decision, p.payment_id, c.payment_hash, a.record_digest, w.opens_at_ms, t.payload, x.instance FROM registrations r, batches b, payment_ids p, incoming_claims c, authorizations a, initial_windows w, activation_anchors t, closed_instances x LIMIT 0")?;
        let broken_references: bool = ledger.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
            [],
            |r| r.get(0),
        )?;
        if broken_references {
            return Err(LedgerError::InvalidLedger);
        }
        outgoing::validate_schema(&ledger.connection)?;
        storage::validate_schema(&ledger.connection)?;
        clock::validate_schema(&ledger.connection)?;
        receiving::validate_schema(&ledger.connection)?;
        origin::validate_schema(&ledger.connection)?;
        Ok(ledger)
    }

    fn check_file_identity(&self) -> Result<()> {
        if storage::FileIdentity::read(&self.path)? != self.file_identity {
            return Err(LedgerError::InvalidLedger);
        }
        Ok(())
    }

    fn connect(path: &Path, account: AccountId) -> Result<Self> {
        let file_identity = storage::FileIdentity::read(path)?;
        let canonical_path = path.canonicalize()?;
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "fullfsync", "ON")?;
        // Rollback journals avoid long-lived WAL sidecars. SQLite creates its
        // journal with the database's restrictive permissions.
        let mode: String = connection.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        if mode.to_lowercase() != "delete" {
            return Err(LedgerError::InvalidLedger);
        }
        Ok(Self {
            connection,
            account,
            path: canonical_path,
            file_identity,
        })
    }

    /// Register a one-time handshake obligation. Recurring obligations must use
    /// reserve_batch so every ID is reserved together before Tor admission.
    pub fn register_handshake(&mut self, obligation: &Obligation) -> Result<()> {
        self.check_file_identity()?;
        obligation.validate()?;
        if obligation.kind != ChargeKind::Handshake {
            return Err(LedgerError::InvalidInput);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_registration(&tx, obligation)?;
        tx.commit()?;
        Ok(())
    }

    /// Atomically reserve all ten identifiers for this verified account. A
    /// conflict persists a stable refusal without reserving any of its IDs.
    /// Storage errors are not permission to send RESERVED (or to reset state).
    pub fn reserve_batch(
        &mut self,
        batch: &BatchRegistration,
        decision: Id16,
    ) -> Result<BatchReservation> {
        self.check_file_identity()?;
        if decision == [0; 16] || batch.request_digest == [0; 32] {
            return Err(LedgerError::InvalidInput);
        }
        let first = &batch.rounds[0];
        let base = batch
            .period
            .checked_mul(10)
            .ok_or(LedgerError::InvalidInput)?;
        for (i, round) in batch.rounds.iter().enumerate() {
            round.validate()?;
            if round.kind != ChargeKind::Round
                || round.round
                    != base
                        .checked_add(i as u64 + 1)
                        .ok_or(LedgerError::InvalidInput)?
                || round.instance != first.instance
                || round.handle != first.handle
                || round.circuit_binding != first.circuit_binding
                || round.policy_hash != first.policy_hash
                || round.allocation_hash != first.allocation_hash
                || batch.rounds[..i]
                    .iter()
                    .any(|r| r.payment_id == round.payment_id)
            {
                return Err(LedgerError::InvalidInput);
            }
        }
        let period = sql_number(batch.period)?;
        let payload = encode(batch)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(Vec<u8>, Vec<u8>, i64)> = tx.query_row(
            "SELECT payload, decision, outcome FROM batches WHERE instance=?1 AND handle=?2 AND period=?3",
            params![&first.instance[..], &first.handle[..], period],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).optional()?;
        let outcome = if let Some((old_payload, old_decision, outcome)) = existing {
            if payload != old_payload || old_decision != decision {
                return Err(LedgerError::Conflict);
            }
            outcome_from_sql(outcome)?
        } else {
            storage::check_admission(&tx)?;
            require_open_instance(&tx, first.instance)?;
            receiving::require_open_binding(&tx, first.instance, first.handle)?;
            let mut conflict = false;
            for obligation in &batch.rounds {
                let used: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM payment_ids WHERE payment_id=?1)",
                    params![&obligation.payment_id[..]],
                    |r| r.get(0),
                )?;
                let registered: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM registrations WHERE (circuit_binding=?1 AND round=?2 AND kind=1) OR (instance=?3 AND handle=?4 AND round=?2 AND kind=1))",
                    params![&obligation.circuit_binding[..], sql_number(obligation.round)?, &obligation.instance[..], &obligation.handle[..]], |r| r.get(0))?;
                conflict |= used || registered;
            }
            let outcome = if conflict {
                AdmissionOutcome::Conflict
            } else {
                AdmissionOutcome::Reserved
            };
            tx.execute("INSERT INTO batches(instance,handle,period,request_digest,decision,outcome,payload) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![&first.instance[..], &first.handle[..], period, &batch.request_digest[..], &decision[..], if conflict {1} else {0}, payload])?;
            if !conflict {
                for obligation in &batch.rounds {
                    insert_registration(&tx, obligation)?;
                    tx.execute("INSERT INTO payment_ids(payment_id,instance,handle,period) VALUES(?1,?2,?3,?4)", params![&obligation.payment_id[..], &first.instance[..], &first.handle[..], period])?;
                }
            }
            storage::check_admission(&tx)?;
            outcome
        };
        tx.commit()?;
        Ok(BatchReservation {
            instance: first.instance,
            handle: first.handle,
            period: batch.period,
            request_digest: batch.request_digest,
            decision,
            outcome,
        })
    }

    /// Read the persisted admission decision after a lost reply/restart. This
    /// never reserves IDs or invents a replacement operation identity.
    pub fn get_batch_reservation(
        &self,
        instance: Id16,
        handle: Id16,
        period: u64,
    ) -> Result<Option<BatchReservation>> {
        self.check_file_identity()?;
        let row: Option<(Vec<u8>,Vec<u8>,i64)> = self.connection.query_row("SELECT request_digest,decision,outcome FROM batches WHERE instance=?1 AND handle=?2 AND period=?3",params![&instance[..],&handle[..],sql_number(period)?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        row.map(|(request_digest, decision, outcome)| {
            Ok(BatchReservation {
                instance,
                handle,
                period,
                request_digest: request_digest
                    .try_into()
                    .map_err(|_| LedgerError::InvalidLedger)?,
                decision: decision
                    .try_into()
                    .map_err(|_| LedgerError::InvalidLedger)?,
                outcome: outcome_from_sql(outcome)?,
            })
        })
        .transpose()
    }

    /// Read financial authorization independently of live Tor state. A returned
    /// record is not proof of current activation or permission to create a path.
    pub fn get_authorization(&mut self, obligation: &Obligation) -> Result<Option<Authorization>> {
        self.check_file_identity()?;
        let tx = self.connection.transaction()?;
        let (id, _) = find_registration(&tx, obligation)?;
        let result = read_authorization(&tx, id)?.map(|(authorization, _)| authorization);
        tx.commit()?;
        Ok(result)
    }

    /// Bind round one's original COMMIT-to-setup-deadline window after Tor
    /// confirms COMMIT. Exact retry is harmless; neither query nor restart can
    /// replace or extend the stored window. This does not authorize payment.
    pub fn bind_initial_window(
        &mut self,
        instance: Id16,
        handle: Id16,
        window: AcceptanceWindow,
    ) -> Result<()> {
        self.check_file_identity()?;
        if window.opens_at_ms < 0 || window.closes_at_ms < window.opens_at_ms {
            return Err(LedgerError::InvalidInput);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(i64, i64)> = tx
            .query_row(
                "SELECT opens_at_ms,closes_at_ms FROM initial_windows WHERE instance=?1 AND handle=?2",
                params![&instance[..], &handle[..]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((opens_at_ms, closes_at_ms)) = existing {
            return if window
                == (AcceptanceWindow {
                    opens_at_ms,
                    closes_at_ms,
                }) {
                Ok(())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        require_initial_registration(&tx, instance, handle, false)?;
        tx.execute(
            "INSERT INTO initial_windows(instance,handle,opens_at_ms,closes_at_ms) VALUES(?1,?2,?3,?4)",
            params![
                &instance[..],
                &handle[..],
                window.opens_at_ms,
                window.closes_at_ms
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Bind the original authenticated first-round activation once, after its
    /// durable authorization exists. Later windows derive from this anchor;
    /// retries cannot reset T0. Caller must verify Tor's actual activation and
    /// clock anchor; writing this record is not itself proof of activation.
    pub fn bind_activation(
        &mut self,
        instance: Id16,
        handle: Id16,
        anchor: ActivationAnchor,
    ) -> Result<()> {
        self.check_file_identity()?;
        anchor.validate()?;
        let payload = encode(&anchor)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<Vec<u8>> = tx
            .query_row(
                "SELECT payload FROM activation_anchors WHERE instance=?1 AND handle=?2",
                params![&instance[..], &handle[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = existing {
            return if old == payload {
                Ok(())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        if clock::activation_anchor(&tx, instance, handle)?.is_some() {
            return Err(LedgerError::Conflict);
        }
        require_initial_registration(&tx, instance, handle, true)?;
        tx.execute(
            "INSERT INTO activation_anchors(instance,handle,payload) VALUES(?1,?2,?3)",
            params![&instance[..], &handle[..], payload],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Reserve verified funds and durably create the exact Tor authorization in
    /// one transaction. The returned decision must be persisted before any RPC;
    /// this commit does so. It does not mean Tor has activated a round.
    pub fn authorize(
        &mut self,
        obligation: &Obligation,
        evidence: &RecipientEvidence,
        decision: Id16,
        clock: ClockObservation,
    ) -> Result<Authorization> {
        self.check_file_identity()?;
        if obligation.amount_msat == 0 {
            return Err(LedgerError::InvalidInput);
        }
        if evidence.account != self.account {
            return Err(LedgerError::WrongAccount);
        }
        if evidence.payment_hash == [0; 32]
            || evidence.invoice_digest == [0; 32]
            || !evidence.incoming
            || !evidence.lightning
            || !evidence.completed
            || evidence.amount_msat != obligation.amount_msat
            || obligation.expected_payment_hash.map_or(
                evidence.authenticated_payment_id != Some(obligation.payment_id),
                |hash| hash != evidence.payment_hash,
            )
        {
            return Err(LedgerError::InvalidEvidence);
        }
        let evidence_payload = encode(evidence)?;
        self.authorize_inner(
            obligation,
            Some(evidence),
            &evidence_payload,
            decision,
            clock,
        )
    }

    /// Explicit zero-priced accounting. It never inserts a synthetic settlement
    /// or consumes the all-zero hash as if it were a node payment.
    pub fn authorize_zero(
        &mut self,
        obligation: &Obligation,
        decision: Id16,
        clock: ClockObservation,
    ) -> Result<Authorization> {
        self.check_file_identity()?;
        if obligation.amount_msat != 0 {
            return Err(LedgerError::InvalidInput);
        }
        self.authorize_inner(obligation, None, &[], decision, clock)
    }

    fn authorize_inner(
        &mut self,
        obligation: &Obligation,
        evidence: Option<&RecipientEvidence>,
        evidence_payload: &[u8],
        decision: Id16,
        clock: ClockObservation,
    ) -> Result<Authorization> {
        obligation.validate()?;
        if decision == [0; 16] {
            return Err(LedgerError::InvalidInput);
        }
        let payment_hash = evidence.map_or([0; 32], |e| e.payment_hash);
        let digest = authorization_digest(obligation, decision, payment_hash);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (id, closed) = find_registration(&tx, obligation)?;
        if let Some((authorization, old_evidence)) = read_authorization(&tx, id)? {
            if authorization.decision != decision
                || authorization.record_digest != digest
                || old_evidence != evidence_payload
            {
                return Err(LedgerError::Conflict);
            }
            tx.commit()?;
            return Ok(authorization);
        }
        if closed {
            return Err(LedgerError::Closed);
        }
        let window = effective_window(&tx, obligation)?;
        certain_time(clock.now_ms, clock.uncertainty_ms, window)?;
        if let Some(e) = evidence {
            certain_time(e.settled_at_ms, e.settlement_uncertainty_ms, window)?;
            if e.settled_at_ms
                .checked_add(sql_number(e.settlement_uncertainty_ms)?)
                .ok_or(LedgerError::OutsideWindow)?
                > clock
                    .now_ms
                    .checked_add(sql_number(clock.uncertainty_ms)?)
                    .ok_or(LedgerError::OutsideWindow)?
            {
                return Err(LedgerError::OutsideWindow);
            }
            let claimed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM incoming_claims WHERE account=?1 AND payment_hash=?2)",
                params![&self.account.0[..], &payment_hash[..]],
                |r| r.get(0),
            )?;
            if claimed {
                return Err(LedgerError::Conflict);
            }
            tx.execute("INSERT INTO incoming_claims(account,payment_hash,registration_id,amount_msat,settled_at_ms,evidence) VALUES(?1,?2,?3,?4,?5,?6)", params![&self.account.0[..], &payment_hash[..], id, sql_number(e.amount_msat)?, e.settled_at_ms, evidence_payload])?;
        }
        tx.execute("INSERT INTO authorizations(registration_id,decision,record_digest,payment_hash,state,evidence) VALUES(?1,?2,?3,?4,0,?5)", params![id, &decision[..], &digest[..], &payment_hash[..], evidence_payload])?;
        storage::release_incoming(&tx, id)?;
        tx.commit()?;
        Ok(Authorization {
            decision,
            record_digest: digest,
            payment_hash,
            state: AuthorizationState::Authorized,
        })
    }

    /// Record authenticated Tor activation confirmation. A stale decision or a
    /// closed live binding cannot consume/reassign a different obligation.
    pub fn mark_consumed(&mut self, obligation: &Obligation, decision: Id16) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (id, closed) = find_registration(&tx, obligation)?;
        let (authorization, _) = read_authorization(&tx, id)?.ok_or(LedgerError::NotRegistered)?;
        if authorization.decision != decision {
            return Err(LedgerError::Conflict);
        }
        if authorization.state == AuthorizationState::Consumed {
            return Ok(());
        }
        if closed {
            return Err(LedgerError::Closed);
        }
        tx.execute(
            "UPDATE authorizations SET state=1 WHERE registration_id=?1",
            params![id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Tor process loss closes all old bindings without erasing claims or
    /// pretending ambiguous activation is an unspent/refundable failure.
    pub fn close_instance(&mut self, instance: Id16) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT OR IGNORE INTO closed_instances(instance) VALUES(?1)",
            params![&instance[..]],
        )?;
        tx.execute("UPDATE authorizations SET state=2 WHERE state=0 AND registration_id IN (SELECT id FROM registrations WHERE instance=?1)", params![&instance[..]])?;
        tx.execute(
            "UPDATE registrations SET closed=1 WHERE instance=?1",
            params![&instance[..]],
        )?;
        outgoing::close_instance(&tx, instance)?;
        tx.commit()?;
        Ok(())
    }
}

/// Recovery contract's canonical big-endian binary record, never JSON/native structs.
pub fn authorization_digest(
    obligation: &Obligation,
    decision: Id16,
    payment_hash: Hash32,
) -> Hash32 {
    let mut hash = Sha256::new();
    hash.update([1]);
    hash.update(obligation.instance);
    hash.update(obligation.handle);
    hash.update(decision);
    hash.update(obligation.round.to_be_bytes());
    hash.update(obligation.amount_msat.to_be_bytes());
    hash.update(payment_hash);
    hash.update(obligation.policy_hash);
    hash.update(obligation.allocation_hash);
    hash.finalize().into()
}

fn sql_number(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| LedgerError::InvalidInput)
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| LedgerError::InvalidInput)
}

fn certain_time(value: i64, error: u64, window: AcceptanceWindow) -> Result<()> {
    let error = sql_number(error)?;
    if value.checked_sub(error).ok_or(LedgerError::OutsideWindow)? < window.opens_at_ms
        || value.checked_add(error).ok_or(LedgerError::OutsideWindow)? > window.closes_at_ms
    {
        return Err(LedgerError::OutsideWindow);
    }
    Ok(())
}

fn require_initial_registration(
    tx: &rusqlite::Transaction<'_>,
    instance: Id16,
    handle: Id16,
    authorized: bool,
) -> Result<()> {
    let row: Option<(i64,bool)> = tx.query_row("SELECT id,closed FROM registrations WHERE instance=?1 AND handle=?2 AND round=1 AND kind=1", params![&instance[..],&handle[..]], |r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let (id, closed) = row.ok_or(LedgerError::NotRegistered)?;
    if closed {
        return Err(LedgerError::Closed);
    }
    if authorized && read_authorization(tx, id)?.is_none() {
        return Err(LedgerError::NotRegistered);
    }
    Ok(())
}

fn effective_window(
    tx: &rusqlite::Transaction<'_>,
    obligation: &Obligation,
) -> Result<AcceptanceWindow> {
    if let Some(window) = obligation.window {
        return Ok(window);
    }
    if obligation.round == 1 {
        return tx
            .query_row(
                "SELECT opens_at_ms,closes_at_ms FROM initial_windows WHERE instance=?1 AND handle=?2",
                params![&obligation.instance[..], &obligation.handle[..]],
                |r| {
                    Ok(AcceptanceWindow {
                        opens_at_ms: r.get(0)?,
                        closes_at_ms: r.get(1)?,
                    })
                },
            )
            .optional()?
            .ok_or(LedgerError::NotRegistered);
    }
    let anchor = clock::activation_anchor(tx, obligation.instance, obligation.handle)?
        .ok_or(LedgerError::NotRegistered)?;
    anchor.acceptance_window(obligation.round)
}

fn insert_registration(tx: &rusqlite::Transaction<'_>, obligation: &Obligation) -> Result<()> {
    let payload = encode(obligation)?;
    let old: Option<(Vec<u8>, bool)> = tx.query_row("SELECT payload, closed FROM registrations WHERE circuit_binding=?1 AND round=?2 AND kind=?3", params![&obligation.circuit_binding[..], sql_number(obligation.round)?, obligation.kind.code()], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((old, _closed)) = old {
        return if old == payload {
            Ok(())
        } else {
            Err(LedgerError::Conflict)
        };
    }
    storage::check_admission(tx)?;
    require_open_instance(tx, obligation.instance)?;
    receiving::require_open_binding(tx, obligation.instance, obligation.handle)?;
    tx.execute("INSERT INTO registrations(circuit_binding,round,kind,instance,handle,payload) VALUES(?1,?2,?3,?4,?5,?6)", params![&obligation.circuit_binding[..], sql_number(obligation.round)?, obligation.kind.code(), &obligation.instance[..], &obligation.handle[..], payload])?;
    storage::acquire_incoming(tx, tx.last_insert_rowid())?;
    Ok(())
}

fn require_open_instance(tx: &rusqlite::Transaction<'_>, instance: Id16) -> Result<()> {
    let closed: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM closed_instances WHERE instance=?1)",
        params![&instance[..]],
        |r| r.get(0),
    )?;
    if closed {
        Err(LedgerError::Closed)
    } else {
        Ok(())
    }
}

fn find_registration(
    tx: &rusqlite::Transaction<'_>,
    obligation: &Obligation,
) -> Result<(i64, bool)> {
    let row: Option<(i64,Vec<u8>,bool)> = tx.query_row("SELECT id,payload,closed FROM registrations WHERE circuit_binding=?1 AND round=?2 AND kind=?3", params![&obligation.circuit_binding[..], sql_number(obligation.round)?, obligation.kind.code()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let (id, payload, closed) = row.ok_or(LedgerError::NotRegistered)?;
    if payload != encode(obligation)? {
        return Err(LedgerError::Conflict);
    }
    Ok((id, closed))
}

fn read_authorization(
    tx: &rusqlite::Transaction<'_>,
    id: i64,
) -> Result<Option<(Authorization, Vec<u8>)>> {
    let row: Option<(Vec<u8>,Vec<u8>,Vec<u8>,i64,Vec<u8>)> = tx.query_row("SELECT decision,record_digest,payment_hash,state,evidence FROM authorizations WHERE registration_id=?1", params![id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    row.map(|(decision, digest, payment_hash, state, evidence)| {
        Ok((
            Authorization {
                decision: decision
                    .try_into()
                    .map_err(|_| LedgerError::InvalidLedger)?,
                record_digest: digest.try_into().map_err(|_| LedgerError::InvalidLedger)?,
                payment_hash: payment_hash
                    .try_into()
                    .map_err(|_| LedgerError::InvalidLedger)?,
                state: match state {
                    0 => AuthorizationState::Authorized,
                    1 => AuthorizationState::Consumed,
                    2 => AuthorizationState::ClosedUnresolved,
                    _ => return Err(LedgerError::InvalidLedger),
                },
            },
            evidence,
        ))
    })
    .transpose()
}

fn outcome_from_sql(code: i64) -> Result<AdmissionOutcome> {
    match code {
        0 => Ok(AdmissionOutcome::Reserved),
        1 => Ok(AdmissionOutcome::Conflict),
        _ => Err(LedgerError::InvalidLedger),
    }
}

const SCHEMA: &str = "
CREATE TABLE ledger_info(singleton INTEGER PRIMARY KEY CHECK(singleton=1),account BLOB NOT NULL UNIQUE CHECK(length(account)=32));
CREATE TABLE registrations(id INTEGER PRIMARY KEY,circuit_binding BLOB NOT NULL CHECK(length(circuit_binding)=32),round INTEGER NOT NULL CHECK(round>=0),kind INTEGER NOT NULL CHECK(kind IN(0,1)),instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),payload BLOB NOT NULL,closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN(0,1)),UNIQUE(circuit_binding,round,kind),UNIQUE(instance,handle,round,kind));
CREATE TABLE batches(instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),period INTEGER NOT NULL CHECK(period>=0),request_digest BLOB NOT NULL CHECK(length(request_digest)=32),decision BLOB NOT NULL UNIQUE CHECK(length(decision)=16),outcome INTEGER NOT NULL CHECK(outcome IN(0,1)),payload BLOB NOT NULL,PRIMARY KEY(instance,handle,period));
CREATE TABLE payment_ids(payment_id BLOB PRIMARY KEY CHECK(length(payment_id)=32),instance BLOB NOT NULL,handle BLOB NOT NULL,period INTEGER NOT NULL,FOREIGN KEY(instance,handle,period) REFERENCES batches(instance,handle,period));
CREATE TABLE incoming_claims(account BLOB NOT NULL REFERENCES ledger_info(account),payment_hash BLOB NOT NULL CHECK(length(payment_hash)=32),registration_id INTEGER NOT NULL UNIQUE REFERENCES registrations(id),amount_msat INTEGER NOT NULL CHECK(amount_msat>0),settled_at_ms INTEGER NOT NULL,evidence BLOB NOT NULL,PRIMARY KEY(account,payment_hash));
CREATE TABLE authorizations(registration_id INTEGER PRIMARY KEY REFERENCES registrations(id),decision BLOB NOT NULL UNIQUE CHECK(length(decision)=16),record_digest BLOB NOT NULL CHECK(length(record_digest)=32),payment_hash BLOB NOT NULL CHECK(length(payment_hash)=32),state INTEGER NOT NULL CHECK(state IN(0,1,2)),evidence BLOB NOT NULL);
CREATE TABLE initial_windows(instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),opens_at_ms INTEGER NOT NULL CHECK(opens_at_ms>=0),closes_at_ms INTEGER NOT NULL CHECK(closes_at_ms>=opens_at_ms),PRIMARY KEY(instance,handle));
CREATE TABLE activation_anchors(instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),payload BLOB NOT NULL,PRIMARY KEY(instance,handle));
CREATE TABLE closed_instances(instance BLOB PRIMARY KEY CHECK(length(instance)=16));
";

#[cfg(test)]
mod tests;
