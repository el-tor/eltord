//! Bounded local ledger storage. The test profile charges database allocation,
//! worst-case rollback-journal headroom and durable future-write reservations.
//! Reservations are conservative accounting, not a guarantee against another
//! process exhausting the shared filesystem. Any actual write failure remains
//! fail-closed. No pressure handling deletes financial history or replay state.
use super::*;

pub const DEFAULT_BUDGET_BYTES: u64 = 1_073_741_824;
pub const MIN_BUDGET_BYTES: u64 = 4 * 1024 * 1024;
/// Conservative per-pending-obligation allowance, including index/log growth.
/// Must be measured against deployment workloads before production claims.
pub const OBLIGATION_WRITE_RESERVE_BYTES: u64 = 256 * 1024;
const FILESYSTEM_MARGIN_BYTES: u64 = 1024 * 1024;

#[derive(PartialEq, Eq)]
pub(super) struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}
impl FileIdentity {
    pub(super) fn read(path: &Path) -> Result<Self> {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(LedgerError::InvalidLedger);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StorageStatus {
    pub budget_bytes: u64,
    pub database_bytes: u64,
    pub journal_bytes: u64,
    pub rollback_headroom_bytes: u64,
    pub reserved_write_bytes: u64,
    pub charged_bytes: u64,
    pub filesystem_available_bytes: u64,
    pub admissions_open: bool,
    pub dispatch_open: bool,
}
impl PaymentLedger {
    pub fn storage_status(&self) -> Result<StorageStatus> {
        self.check_file_identity()?;
        measure(&self.connection)
    }

    /// Explicit schema2->3 migration. Ordinary reopen never creates policy or
    /// reservations. Existing unresolved rows acquire headroom without deleting
    /// any evidence; pressure may immediately block new admission/dispatch.
    pub fn upgrade_from_v2(path: &Path, expected_account: AccountId) -> Result<Self> {
        let mut ledger = Self::connect(path, expected_account)?;
        let version: i64 = ledger
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 4 {
            drop(ledger);
            return Self::upgrade_from_v4(path, expected_account);
        }
        if version == 5 {
            drop(ledger);
            return Self::upgrade_from_v5(path, expected_account);
        }
        if version == 6 {
            drop(ledger);
            return Self::upgrade_from_v6(path, expected_account);
        }
        if version == VERSION {
            drop(ledger);
            return Self::reopen(path, expected_account);
        }
        if version == 3 {
            drop(ledger);
            return Self::upgrade_from_v3(path, expected_account);
        }
        let application: i64 =
            ledger
                .connection
                .pragma_query_value(None, "application_id", |r| r.get(0))?;
        if version != 2 || application != APPLICATION_ID {
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
        ledger.connection.prepare("SELECT r.id,b.decision,p.payment_id,c.payment_hash,a.record_digest,w.opens_at_ms,t.payload,x.instance FROM registrations r,batches b,payment_ids p,incoming_claims c,authorizations a,initial_windows w,activation_anchors t,closed_instances x LIMIT 0")?;
        ledger
            .connection
            .pragma_update(None, "foreign_keys", "OFF")?;
        let tx = ledger
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        initialize(&tx, DEFAULT_BUDGET_BYTES)?;
        reserve_legacy(&tx)?;
        tx.execute_batch(clock::SCHEMA)?;
        tx.execute_batch(receiving::SCHEMA)?;
        tx.execute_batch(origin::SCHEMA)?;
        tx.execute_batch(origin_artifacts::SCHEMA)?;
        tx.execute_batch(handshake::SCHEMA)?;
        outgoing_renewal::initialize(&tx)?;
        origin_artifacts::validate_schema(&tx, expected_account)?;
        outgoing_renewal::validate_schema(&tx)?;
        handshake::validate_schema(&tx, expected_account)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
        drop(ledger);
        Self::reopen(path, expected_account)
    }
}

pub(super) fn validate_budget(bytes: u64) -> Result<()> {
    if bytes < MIN_BUDGET_BYTES || bytes > DEFAULT_BUDGET_BYTES {
        return Err(LedgerError::InvalidInput);
    }
    Ok(())
}
pub(super) fn initialize(tx: &rusqlite::Transaction<'_>, budget: u64) -> Result<()> {
    validate_budget(budget)?;
    tx.execute_batch(SCHEMA)?;
    tx.execute(
        "INSERT INTO storage_policy(singleton,budget_bytes) VALUES(1,?1)",
        params![sql_number(budget)?],
    )?;
    Ok(())
}
pub(super) fn validate_schema(connection: &Connection) -> Result<()> {
    connection.prepare("SELECT p.budget_bytes,r.kind,r.reference,r.bytes FROM storage_policy p,storage_reservations r LIMIT 0")?;
    let cap = budget(connection)?;
    validate_budget(cap).map_err(|_| LedgerError::InvalidLedger)?;
    // The page quota limits database growth and charges estimated rollback
    // amplification against the budget. This estimate still needs measurement. Existing oversized ledgers stay readable/reconcilable;
    // lowering max_page_count cannot truncate or erase their existing pages.
    let page: u64 = connection.pragma_query_value(None, "page_size", |r| r.get(0))?;
    let quota = cap.saturating_sub(64 * 1024)
        / (page
            .checked_mul(2)
            .and_then(|n| n.checked_add(8))
            .ok_or(LedgerError::InvalidLedger)?);
    connection.pragma_update(None, "max_page_count", quota.max(1))?;
    let mut stmt=connection.prepare("SELECT r.id FROM registrations r LEFT JOIN authorizations a ON r.id=a.registration_id WHERE a.registration_id IS NULL")?;
    for id in stmt.query_map([], |r| r.get::<_, i64>(0))? {
        let id = id?;
        let present:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM storage_reservations WHERE kind=0 AND reference=?1 AND bytes=?2)",params![&id.to_be_bytes()[..],sql_number(OBLIGATION_WRITE_RESERVE_BYTES)?],|r|r.get(0))?;
        if !present {
            return Err(LedgerError::InvalidLedger);
        }
    }
    let broken:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM outgoing_intents o LEFT JOIN storage_reservations r ON r.kind=1 AND r.reference=o.operation WHERE (o.state IN(0,1,2,6) AND (r.reference IS NULL OR r.bytes!=262144)) OR (o.state NOT IN(0,1,2,6) AND r.reference IS NOT NULL))",[],|r|r.get(0))?;
    if broken {
        return Err(LedgerError::InvalidLedger);
    }
    let mut stmt = connection.prepare("SELECT kind,reference,bytes FROM storage_reservations")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Vec<u8>>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        let (kind, key, bytes) = row?;
        let present:bool=match kind {
            0=>{let id=i64::from_be_bytes(key.try_into().map_err(|_|LedgerError::InvalidLedger)?);
                connection.query_row("SELECT EXISTS(SELECT 1 FROM registrations r LEFT JOIN authorizations a ON r.id=a.registration_id WHERE r.id=?1 AND a.registration_id IS NULL)",params![id],|r|r.get(0))?},
            1=>connection.query_row("SELECT EXISTS(SELECT 1 FROM outgoing_intents WHERE operation=?1 AND state IN(0,1,2,6))",params![key],|r|r.get(0))?,
            _=>false,
        };
        if !present || bytes != OBLIGATION_WRITE_RESERVE_BYTES as i64 {
            return Err(LedgerError::InvalidLedger);
        }
    }
    Ok(())
}
fn budget(connection: &Connection) -> Result<u64> {
    let n: i64 = connection.query_row(
        "SELECT budget_bytes FROM storage_policy WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    u64::try_from(n).map_err(|_| LedgerError::InvalidLedger)
}
fn file_bytes(path: &Path) -> Result<u64> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(LedgerError::InvalidLedger);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(metadata.len().max(
            metadata
                .blocks()
                .checked_mul(512)
                .ok_or(LedgerError::InvalidLedger)?,
        ))
    }
    #[cfg(not(unix))]
    {
        Ok(metadata.len())
    }
}
#[cfg(unix)]
fn available_bytes(path: &Path) -> Result<u64> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| LedgerError::InvalidLedger)?;
    let mut info = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), info.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let info = unsafe { info.assume_init() };
    if info.f_flag & libc::ST_RDONLY as libc::c_ulong != 0 {
        return Err(LedgerError::StoragePressure);
    }
    (info.f_bavail as u64)
        .checked_mul(info.f_frsize as u64)
        .ok_or(LedgerError::InvalidLedger)
}
#[cfg(not(unix))]
fn available_bytes(_path: &Path) -> Result<u64> {
    Err(LedgerError::UnsupportedOperation)
}
fn measure(connection: &Connection) -> Result<StorageStatus> {
    let budget = budget(connection)?;
    let path = Path::new(
        connection
            .path()
            .filter(|p| !p.is_empty())
            .ok_or(LedgerError::InvalidLedger)?,
    );
    if !path.is_file() {
        return Err(LedgerError::InvalidLedger);
    }
    let pages: u64 = connection.pragma_query_value(None, "page_count", |r| r.get(0))?;
    let page: u64 = connection.pragma_query_value(None, "page_size", |r| r.get(0))?;
    let database =
        file_bytes(path)?.max(pages.checked_mul(page).ok_or(LedgerError::InvalidLedger)?);
    let mut journal = 0u64;
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        journal = journal
            .checked_add(file_bytes(Path::new(&sidecar))?)
            .ok_or(LedgerError::InvalidLedger)?;
    }
    let rollback = database
        .checked_add(pages.checked_mul(8).ok_or(LedgerError::InvalidLedger)?)
        .and_then(|n| n.checked_add(64 * 1024))
        .ok_or(LedgerError::InvalidLedger)?;
    let reserved: i64 = connection.query_row(
        "SELECT coalesce(sum(bytes),0) FROM storage_reservations",
        [],
        |r| r.get(0),
    )?;
    let origin_reserved: i64 = connection.query_row(
        "SELECT coalesce(sum(reserve_bytes),0) FROM origin_operations",
        [],
        |r| r.get(0),
    )?;
    let artifacts_reserved:i64=connection.query_row("SELECT coalesce((SELECT sum(reserve_bytes) FROM origin_constructions),0)+coalesce((SELECT sum(reserve_bytes) FROM origin_invoices),0)+coalesce((SELECT sum(reserve_bytes) FROM origin_renewal_operations),0)+coalesce((SELECT sum(reserve_bytes) FROM origin_allocations),0)+coalesce((SELECT sum(reserve_bytes) FROM handshake_scopes),0)+coalesce((SELECT sum(reserve_bytes) FROM handshake_intents),0)",[],|r|r.get(0))?;
    let reserved = u64::try_from(reserved)
        .map_err(|_| LedgerError::InvalidLedger)?
        .checked_add(u64::try_from(origin_reserved).map_err(|_| LedgerError::InvalidLedger)?)
        .and_then(|n| n.checked_add(u64::try_from(artifacts_reserved).ok()?))
        .ok_or(LedgerError::InvalidLedger)?;
    let charged = database
        .checked_add(journal.max(rollback))
        .and_then(|n| n.checked_add(reserved))
        .ok_or(LedgerError::InvalidLedger)?;
    let available = available_bytes(path)?;
    let needed = rollback
        .saturating_sub(journal)
        .checked_add(reserved)
        .and_then(|n| n.checked_add(FILESYSTEM_MARGIN_BYTES))
        .ok_or(LedgerError::InvalidLedger)?;
    Ok(StorageStatus {
        budget_bytes: budget,
        database_bytes: database,
        journal_bytes: journal,
        rollback_headroom_bytes: rollback,
        reserved_write_bytes: reserved,
        charged_bytes: charged,
        filesystem_available_bytes: available,
        admissions_open: charged < budget * 80 / 100 && available >= needed,
        dispatch_open: charged < budget * 95 / 100 && available >= needed,
    })
}
pub(super) fn check_admission(connection: &Connection) -> Result<()> {
    if !measure(connection)?.admissions_open {
        return Err(LedgerError::StoragePressure);
    }
    Ok(())
}
pub(super) fn check_dispatch(connection: &Connection) -> Result<()> {
    if !measure(connection)?.dispatch_open {
        return Err(LedgerError::StoragePressure);
    }
    Ok(())
}
fn acquire(tx: &rusqlite::Transaction<'_>, kind: i64, reference: &[u8]) -> Result<()> {
    if kind == 0 {
        check_admission(tx)?;
    } else {
        check_dispatch(tx)?;
    }
    tx.execute(
        "INSERT INTO storage_reservations(kind,reference,bytes) VALUES(?1,?2,?3)",
        params![kind, reference, sql_number(OBLIGATION_WRITE_RESERVE_BYTES)?],
    )?;
    if kind == 0 {
        check_admission(tx)
    } else {
        check_dispatch(tx)
    }
}
pub(super) fn acquire_incoming(tx: &rusqlite::Transaction<'_>, registration: i64) -> Result<()> {
    acquire(tx, 0, &registration.to_be_bytes())
}
pub(super) fn acquire_outgoing(tx: &rusqlite::Transaction<'_>, operation: Id16) -> Result<()> {
    acquire(tx, 1, &operation)
}
pub(super) fn release_incoming(tx: &rusqlite::Transaction<'_>, registration: i64) -> Result<()> {
    tx.execute(
        "DELETE FROM storage_reservations WHERE kind=0 AND reference=?1",
        params![&registration.to_be_bytes()[..]],
    )?;
    Ok(())
}
pub(super) fn release_outgoing(tx: &rusqlite::Transaction<'_>, operation: Id16) -> Result<()> {
    tx.execute(
        "DELETE FROM storage_reservations WHERE kind=1 AND reference=?1",
        params![&operation[..]],
    )?;
    Ok(())
}
pub(super) fn release_cancelled(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute("DELETE FROM storage_reservations WHERE kind=1 AND reference IN (SELECT operation FROM outgoing_intents WHERE state=7)",[])?;
    Ok(())
}
pub(super) fn reserve_legacy(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let ids:Vec<i64>=tx.prepare("SELECT r.id FROM registrations r LEFT JOIN authorizations a ON r.id=a.registration_id WHERE a.registration_id IS NULL")?
        .query_map([],|r|r.get(0))?.collect::<std::result::Result<_,_>>()?;
    for id in ids {
        tx.execute(
            "INSERT INTO storage_reservations(kind,reference,bytes) VALUES(0,?1,?2)",
            params![
                &id.to_be_bytes()[..],
                sql_number(OBLIGATION_WRITE_RESERVE_BYTES)?
            ],
        )?;
    }
    tx.execute("INSERT INTO storage_reservations(kind,reference,bytes) SELECT 1,operation,?1 FROM outgoing_intents WHERE state IN(0,1,2,6)",params![sql_number(OBLIGATION_WRITE_RESERVE_BYTES)?])?;
    Ok(())
}
const SCHEMA:&str="
CREATE TABLE storage_policy(singleton INTEGER PRIMARY KEY CHECK(singleton=1),budget_bytes INTEGER NOT NULL CHECK(budget_bytes>=4194304 AND budget_bytes<=1073741824));
CREATE TABLE storage_reservations(kind INTEGER NOT NULL CHECK(kind IN(0,1)),reference BLOB NOT NULL CHECK((kind=0 AND length(reference)=8) OR (kind=1 AND length(reference)=16)),bytes INTEGER NOT NULL CHECK(bytes=262144),PRIMARY KEY(kind,reference));
";

#[cfg(test)]
mod tests;
