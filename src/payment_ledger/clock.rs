//! Immutable UTC uncertainty intervals for original relay clocks. Intervals are
//! observed through authenticated local snapshots and trusted UTC/monotonic
//! brackets. A later query never replaces or narrows an already saved anchor.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UtcClockInterval {
    pub earliest_ms: i64,
    pub latest_ms: i64,
}
impl UtcClockInterval {
    fn validate(self) -> Result<()> {
        if self.earliest_ms < 0 || self.latest_ms < self.earliest_ms {
            return Err(LedgerError::InvalidInput);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialClockAnchor {
    pub constructed_at: UtcClockInterval,
    pub committed_at: UtcClockInterval,
    pub setup_timeout_ms: u64,
}
impl InitialClockAnchor {
    pub fn acceptance_window(self) -> Result<AcceptanceWindow> {
        self.constructed_at.validate()?;
        self.committed_at.validate()?;
        if self.setup_timeout_ms == 0
            || self.committed_at.latest_ms < self.constructed_at.earliest_ms
        {
            return Err(LedgerError::InvalidInput);
        }
        let closes = self
            .constructed_at
            .earliest_ms
            .checked_add(sql_number(self.setup_timeout_ms)?)
            .ok_or(LedgerError::OutsideWindow)?;
        if self.committed_at.latest_ms > closes {
            return Err(LedgerError::OutsideWindow);
        }
        Ok(AcceptanceWindow {
            opens_at_ms: self.committed_at.latest_ms,
            closes_at_ms: closes,
        })
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationIntervalAnchor {
    pub started_at: UtcClockInterval,
    pub round_ms: u64,
    pub early_ms: u64,
    pub grace_ms: u64,
}
impl ActivationIntervalAnchor {
    fn validate(self) -> Result<()> {
        self.started_at.validate()?;
        if self.round_ms != 60_000 || self.early_ms > 60_000 || self.grace_ms > 60_000 {
            return Err(LedgerError::InvalidInput);
        }
        Ok(())
    }
    pub fn acceptance_window(self, round: u64) -> Result<AcceptanceWindow> {
        self.validate()?;
        if round < 2 {
            return Err(LedgerError::InvalidInput);
        }
        let nominal = (round - 1)
            .checked_mul(self.round_ms)
            .ok_or(LedgerError::OutsideWindow)?;
        let opening = nominal.saturating_sub(self.early_ms);
        let closing = nominal
            .checked_add(self.grace_ms)
            .ok_or(LedgerError::OutsideWindow)?;
        let opens_at_ms = self
            .started_at
            .latest_ms
            .checked_add(sql_number(opening)?)
            .ok_or(LedgerError::OutsideWindow)?;
        let closes_at_ms = self
            .started_at
            .earliest_ms
            .checked_add(sql_number(closing)?)
            .ok_or(LedgerError::OutsideWindow)?;
        if opens_at_ms > closes_at_ms {
            return Err(LedgerError::OutsideWindow);
        }
        Ok(AcceptanceWindow {
            opens_at_ms,
            closes_at_ms,
        })
    }
}
impl From<ActivationAnchor> for ActivationIntervalAnchor {
    fn from(a: ActivationAnchor) -> Self {
        Self {
            started_at: UtcClockInterval {
                earliest_ms: a.started_at_ms,
                latest_ms: a.started_at_ms,
            },
            round_ms: a.round_ms,
            early_ms: a.early_ms,
            grace_ms: a.grace_ms,
        }
    }
}
impl PaymentLedger {
    /// Explicit schema3->4 migration. Legacy exact point anchors retain their
    /// exact semantics; migration never infers missing UTC evidence or times.
    pub fn upgrade_from_v3(path: &Path, account: AccountId) -> Result<Self> {
        let mut ledger = Self::connect(path, account)?;
        let version: i64 = ledger
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 4 {
            drop(ledger);
            return Self::upgrade_from_v4(path, account);
        }
        if version == VERSION {
            drop(ledger);
            return Self::reopen(path, account);
        }
        let app: i64 = ledger
            .connection
            .pragma_query_value(None, "application_id", |r| r.get(0))?;
        if version != 3 || app != APPLICATION_ID {
            return Err(LedgerError::InvalidLedger);
        }
        let stored: Vec<u8> = ledger.connection.query_row(
            "SELECT account FROM ledger_info WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        if stored != account.0 {
            return Err(LedgerError::WrongAccount);
        }
        let health: String = ledger
            .connection
            .query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        let broken: bool = ledger.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
            [],
            |r| r.get(0),
        )?;
        if health != "ok" || broken {
            return Err(LedgerError::InvalidLedger);
        }
        outgoing::validate_schema(&ledger.connection)?;
        storage::validate_schema(&ledger.connection)?;
        ledger.connection.prepare("SELECT r.id,b.decision,p.payment_id,c.payment_hash,a.record_digest,w.opens_at_ms,t.payload,x.instance FROM registrations r,batches b,payment_ids p,incoming_claims c,authorizations a,initial_windows w,activation_anchors t,closed_instances x LIMIT 0")?;
        let tx = ledger
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        tx.execute_batch(receiving::SCHEMA)?;
        tx.execute_batch(origin::SCHEMA)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
        drop(ledger);
        Self::reopen(path, account)
    }
    /// Recover only the exact payload of a durable RESERVED batch. A query
    /// cannot create IDs or reinterpret a refused batch as admitted.
    pub fn get_reserved_batch_registration(
        &self,
        instance: Id16,
        handle: Id16,
        period: u64,
    ) -> Result<Option<BatchRegistration>> {
        self.check_file_identity()?;
        let payload:Option<Vec<u8>>=self.connection.query_row("SELECT payload FROM batches WHERE instance=?1 AND handle=?2 AND period=?3 AND outcome=0",params![&instance[..],&handle[..],sql_number(period)?],|r|r.get(0)).optional()?;
        payload
            .map(|p| {
                let batch: BatchRegistration =
                    serde_json::from_slice(&p).map_err(|_| LedgerError::InvalidLedger)?;
                if batch.period != period
                    || batch
                        .rounds
                        .iter()
                        .any(|r| r.instance != instance || r.handle != handle)
                {
                    return Err(LedgerError::InvalidLedger);
                }
                for r in &batch.rounds {
                    r.validate().map_err(|_| LedgerError::InvalidLedger)?;
                }
                Ok(batch)
            })
            .transpose()
    }
    pub fn get_initial_window(
        &self,
        instance: Id16,
        handle: Id16,
    ) -> Result<Option<AcceptanceWindow>> {
        self.check_file_identity()?;
        Ok(self.connection.query_row("SELECT opens_at_ms,closes_at_ms FROM initial_windows WHERE instance=?1 AND handle=?2",params![&instance[..],&handle[..]],|r|Ok(AcceptanceWindow {opens_at_ms:r.get(0)?,closes_at_ms:r.get(1)?})).optional()?)
    }
    pub fn get_initial_clock_anchor(
        &self,
        instance: Id16,
        handle: Id16,
    ) -> Result<Option<InitialClockAnchor>> {
        self.check_file_identity()?;
        initial_anchor(&self.connection, instance, handle)
    }
    pub fn get_activation_interval_anchor(
        &self,
        instance: Id16,
        handle: Id16,
    ) -> Result<Option<ActivationIntervalAnchor>> {
        self.check_file_identity()?;
        activation_anchor(&self.connection, instance, handle)
    }
    pub fn bind_initial_clock_anchor(
        &mut self,
        instance: Id16,
        handle: Id16,
        anchor: InitialClockAnchor,
    ) -> Result<()> {
        self.check_file_identity()?;
        let window = initial_window_with_handshake(&self.connection, instance, handle, anchor)?;
        let payload = encode(&anchor)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = initial_anchor(&tx, instance, handle)? {
            return if old == anchor {
                Ok(())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM initial_windows WHERE instance=?1 AND handle=?2)",
            params![&instance[..], &handle[..]],
            |r| r.get(0),
        )?;
        if existing {
            return Err(LedgerError::Conflict);
        }
        require_initial_registration(&tx, instance, handle, false)?;
        tx.execute("INSERT INTO initial_windows(instance,handle,opens_at_ms,closes_at_ms) VALUES(?1,?2,?3,?4)",params![&instance[..],&handle[..],window.opens_at_ms,window.closes_at_ms])?;
        tx.execute(
            "INSERT INTO initial_clock_anchors(instance,handle,payload) VALUES(?1,?2,?3)",
            params![&instance[..], &handle[..], payload],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn bind_activation_interval_anchor(
        &mut self,
        instance: Id16,
        handle: Id16,
        anchor: ActivationIntervalAnchor,
    ) -> Result<()> {
        self.check_file_identity()?;
        anchor.validate()?;
        // Verify at least the next window is expressible before retaining an
        // unusably broad interval. Accepted policy values are never rounded.
        anchor.acceptance_window(2)?;
        let payload = encode(&anchor)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = activation_anchor(&tx, instance, handle)? {
            return if old == anchor {
                Ok(())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        require_initial_registration(&tx, instance, handle, true)?;
        tx.execute(
            "INSERT INTO activation_interval_anchors(instance,handle,payload) VALUES(?1,?2,?3)",
            params![&instance[..], &handle[..], payload],
        )?;
        tx.commit()?;
        Ok(())
    }
}
pub(super) fn initial_anchor(
    connection: &Connection,
    instance: Id16,
    handle: Id16,
) -> Result<Option<InitialClockAnchor>> {
    let payload: Option<Vec<u8>> = connection
        .query_row(
            "SELECT payload FROM initial_clock_anchors WHERE instance=?1 AND handle=?2",
            params![&instance[..], &handle[..]],
            |r| r.get(0),
        )
        .optional()?;
    payload
        .map(|p| {
            let a: InitialClockAnchor =
                serde_json::from_slice(&p).map_err(|_| LedgerError::InvalidLedger)?;
            a.acceptance_window()
                .map_err(|_| LedgerError::InvalidLedger)?;
            Ok(a)
        })
        .transpose()
}
pub(super) fn activation_anchor(
    connection: &Connection,
    instance: Id16,
    handle: Id16,
) -> Result<Option<ActivationIntervalAnchor>> {
    let bounded: Option<Vec<u8>> = connection
        .query_row(
            "SELECT payload FROM activation_interval_anchors WHERE instance=?1 AND handle=?2",
            params![&instance[..], &handle[..]],
            |r| r.get(0),
        )
        .optional()?;
    let point: Option<Vec<u8>> = connection
        .query_row(
            "SELECT payload FROM activation_anchors WHERE instance=?1 AND handle=?2",
            params![&instance[..], &handle[..]],
            |r| r.get(0),
        )
        .optional()?;
    match (bounded, point) {
        (Some(_), Some(_)) => Err(LedgerError::InvalidLedger),
        (Some(p), None) => {
            let a: ActivationIntervalAnchor =
                serde_json::from_slice(&p).map_err(|_| LedgerError::InvalidLedger)?;
            a.validate().map_err(|_| LedgerError::InvalidLedger)?;
            Ok(Some(a))
        }
        (None, Some(p)) => {
            let a: ActivationAnchor =
                serde_json::from_slice(&p).map_err(|_| LedgerError::InvalidLedger)?;
            a.validate().map_err(|_| LedgerError::InvalidLedger)?;
            Ok(Some(a.into()))
        }
        (None, None) => Ok(None),
    }
}
// The UTC observation remains intact. The previously accepted fee deadline is
// a separate immutable upper bound, not a reconstructed observation endpoint.
fn initial_window_with_handshake(
    connection: &Connection,
    instance: Id16,
    handle: Id16,
    anchor: InitialClockAnchor,
) -> Result<AcceptanceWindow> {
    let mut window = anchor.acceptance_window()?;
    let payloads: Vec<Vec<u8>> = connection.prepare("SELECT payload FROM registrations WHERE instance=?1 AND handle=?2 AND kind=0 AND round=0 LIMIT 2")?
        .query_map(params![&instance[..], &handle[..]], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?;
    if payloads.len() > 1 {
        return Err(LedgerError::InvalidLedger);
    }
    if let Some(payload) = payloads.first() {
        let fee: Obligation =
            serde_json::from_slice(payload).map_err(|_| LedgerError::InvalidLedger)?;
        fee.validate().map_err(|_| LedgerError::InvalidLedger)?;
        let rounds: Vec<Vec<u8>> = connection.prepare("SELECT payload FROM registrations WHERE instance=?1 AND handle=?2 AND kind=1 AND round=1 LIMIT 2")?
            .query_map(params![&instance[..], &handle[..]], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?;
        if rounds.len() != 1 {
            return Err(LedgerError::InvalidLedger);
        }
        let first: Obligation =
            serde_json::from_slice(&rounds[0]).map_err(|_| LedgerError::InvalidLedger)?;
        first.validate().map_err(|_| LedgerError::InvalidLedger)?;
        if fee.instance != instance
            || fee.handle != handle
            || fee.kind != ChargeKind::Handshake
            || fee.round != 0
            || first.instance != instance
            || first.handle != handle
            || first.kind != ChargeKind::Round
            || first.round != 1
            || fee.circuit_binding != first.circuit_binding
            || fee.policy_hash != first.policy_hash
            || fee.allocation_hash != first.allocation_hash
        {
            return Err(LedgerError::InvalidLedger);
        }
        window.closes_at_ms = window
            .closes_at_ms
            .min(fee.window.ok_or(LedgerError::InvalidLedger)?.closes_at_ms);
        if window.closes_at_ms < window.opens_at_ms {
            return Err(LedgerError::OutsideWindow);
        }
    }
    Ok(window)
}
pub(super) fn validate_schema(connection: &Connection) -> Result<()> {
    connection.prepare("SELECT i.payload,a.payload FROM initial_clock_anchors i,activation_interval_anchors a LIMIT 0")?;
    let mut stmt = connection.prepare("SELECT instance,handle FROM initial_clock_anchors")?;
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
    })? {
        let (instance, handle) = row?;
        let instance: Id16 = instance
            .try_into()
            .map_err(|_| LedgerError::InvalidLedger)?;
        let handle: Id16 = handle.try_into().map_err(|_| LedgerError::InvalidLedger)?;
        let anchor =
            initial_anchor(connection, instance, handle)?.ok_or(LedgerError::InvalidLedger)?;
        let window: AcceptanceWindow = connection.query_row(
            "SELECT opens_at_ms,closes_at_ms FROM initial_windows WHERE instance=?1 AND handle=?2",
            params![&instance[..], &handle[..]],
            |r| {
                Ok(AcceptanceWindow {
                    opens_at_ms: r.get(0)?,
                    closes_at_ms: r.get(1)?,
                })
            },
        )?;
        if window != initial_window_with_handshake(connection, instance, handle, anchor)? {
            return Err(LedgerError::InvalidLedger);
        }
    }
    let mut stmt = connection.prepare("SELECT instance,handle FROM activation_interval_anchors UNION SELECT instance,handle FROM activation_anchors")?;
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
    })? {
        let (instance, handle) = row?;
        activation_anchor(
            connection,
            instance
                .try_into()
                .map_err(|_| LedgerError::InvalidLedger)?,
            handle.try_into().map_err(|_| LedgerError::InvalidLedger)?,
        )?
        .ok_or(LedgerError::InvalidLedger)?;
    }
    Ok(())
}
pub(super) const SCHEMA:&str="
CREATE TABLE initial_clock_anchors(instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),payload BLOB NOT NULL,PRIMARY KEY(instance,handle),FOREIGN KEY(instance,handle) REFERENCES initial_windows(instance,handle));
CREATE TABLE activation_interval_anchors(instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),payload BLOB NOT NULL,PRIMARY KEY(instance,handle));
";
#[cfg(test)]
mod tests;
