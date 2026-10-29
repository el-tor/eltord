//! Explicit operator maintenance, separate from runtime startup. Native calls
//! verify account and receiving capabilities only; this module has no pay path.
use super::foundation_startup::FoundationConfig;
use crate::payment_ledger::{AccountId, PaymentLedger, APPLICATION_ID, VERSION};
use rusqlite::{Connection, OpenFlags};
use std::{io, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerOperation {
    Provision,
    Migrate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerOperationReport {
    pub operation: LedgerOperation,
    /// None for exclusive new creation. A current schema is an explicit no-op
    /// migration, still requiring native receiving capability verification.
    pub previous_schema: Option<i64>,
    pub current_schema: i64,
}
/// Diagnostics contain no config paths, credentials, account identifiers, native
/// response bodies or payment associations. Inspect original files locally.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OperatorError {
    #[error("Invalid or inaccessible private foundation configuration")]
    Configuration,
    #[error("Ledger target already exists; provisioning never replaces existing state")]
    TargetExists,
    #[error("Existing ledger target is missing; migration never creates replacement state")]
    TargetMissing,
    #[error("Ledger target is invalid, corrupt, or uses unsupported storage settings")]
    InvalidLedger,
    #[error("Existing ledger belongs to a different configured receiving account")]
    WrongAccount,
    #[error("Existing ledger schema is not supported by this build")]
    UnsupportedSchema,
    #[error("Pinned native receiving account or read-only capabilities could not be verified")]
    NativeVerification,
    #[error("Ledger operation could not complete; retained state requires explicit inspection")]
    Storage,
}
type Result<T> = std::result::Result<T, OperatorError>;

/// All filesystem/native mutations require this explicit operator entrypoint.
/// Provision checks absence, verifies the pinned CLN recipient, then uses the
/// ledger's atomic create_new. Migrate verifies retained state read-only before
/// native verification and transactional upgrades; it never resets history.
pub async fn operate_foundation_ledger(
    operation: LedgerOperation,
    config_path: &Path,
) -> Result<LedgerOperationReport> {
    let config = FoundationConfig::load(config_path).map_err(|_| OperatorError::Configuration)?;
    let account = AccountId(config.account().fingerprint());
    let previous = match operation {
        LedgerOperation::Provision => {
            require_absent(config.ledger_path())?;
            None
        }
        LedgerOperation::Migrate => Some(inspect_existing(config.ledger_path(), account)?),
    };
    config
        .verify_recipient()
        .await
        .map_err(|_| OperatorError::NativeVerification)?;
    match operation {
        LedgerOperation::Provision => {
            // create_new remains authoritative if another process created the
            // path while native verification was in progress.
            PaymentLedger::provision(config.ledger_path(), account).map_err(
                |error| match error {
                    crate::payment_ledger::LedgerError::File(error)
                        if error.kind() == io::ErrorKind::AlreadyExists =>
                    {
                        OperatorError::TargetExists
                    }
                    _ => OperatorError::Storage,
                },
            )?;
        }
        LedgerOperation::Migrate => {
            // This existing explicit dispatcher covers v1..current and fully
            // validates schema/account/health again before transactional writes.
            PaymentLedger::upgrade_from_v1(config.ledger_path(), account)
                .map_err(|_| OperatorError::Storage)?;
        }
    }
    Ok(LedgerOperationReport {
        operation,
        previous_schema: previous,
        current_schema: VERSION,
    })
}
fn require_absent(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(OperatorError::TargetExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(OperatorError::Storage),
    }
}
fn inspect_existing(path: &Path, account: AccountId) -> Result<i64> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            OperatorError::TargetMissing
        } else {
            OperatorError::InvalidLedger
        }
    })?;
    if !metadata.is_file() {
        return Err(OperatorError::InvalidLedger);
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|_| OperatorError::InvalidLedger)?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| OperatorError::InvalidLedger)?;
    let app: i64 = connection
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .map_err(|_| OperatorError::InvalidLedger)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| OperatorError::InvalidLedger)?;
    if app != APPLICATION_ID {
        return Err(OperatorError::InvalidLedger);
    }
    if !(1..=VERSION).contains(&version) {
        return Err(OperatorError::UnsupportedSchema);
    }
    let stored: Vec<u8> = connection
        .query_row(
            "SELECT account FROM ledger_info WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .map_err(|_| OperatorError::InvalidLedger)?;
    if stored != account.0 {
        return Err(OperatorError::WrongAccount);
    }
    let check: String = connection
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|_| OperatorError::InvalidLedger)?;
    let broken: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
            [],
            |r| r.get(0),
        )
        .map_err(|_| OperatorError::InvalidLedger)?;
    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .map_err(|_| OperatorError::InvalidLedger)?;
    if check != "ok" || broken || journal.to_lowercase() != "delete" {
        return Err(OperatorError::InvalidLedger);
    }
    Ok(version)
}
#[cfg(test)]
mod tests;
