//! Explicit v4 upgrade. Reopen never invents discovery or control history.
use super::*;
impl PaymentLedger {
    pub fn upgrade_from_v4(path: &Path, account: AccountId) -> Result<Self> {
        let mut ledger = Self::connect(path, account)?;
        let version: i64 = ledger
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 5 {
            drop(ledger);
            return Self::upgrade_from_v5(path, account);
        }
        if version == VERSION {
            drop(ledger);
            return Self::reopen(path, account);
        }
        let app: i64 = ledger
            .connection
            .pragma_query_value(None, "application_id", |r| r.get(0))?;
        if version != 4 || app != APPLICATION_ID {
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
        clock::validate_schema(&ledger.connection)?;
        ledger
            .connection
            .pragma_update(None, "foreign_keys", "OFF")?;
        let tx = ledger
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(receiving::SCHEMA)?;
        tx.execute_batch(origin::SCHEMA)?;
        tx.execute_batch(origin_artifacts::SCHEMA)?;
        outgoing_renewal::initialize(&tx)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
        drop(ledger);
        Self::reopen(path, account)
    }
}

impl PaymentLedger {
    /// Explicit v5->6 upgrade; ordinary reopen rejects older schemas.
    pub fn upgrade_from_v5(path: &Path, account: AccountId) -> Result<Self> {
        let mut ledger = Self::connect(path, account)?;
        let version: i64 = ledger
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == VERSION {
            drop(ledger);
            return Self::reopen(path, account);
        }
        let app: i64 = ledger
            .connection
            .pragma_query_value(None, "application_id", |r| r.get(0))?;
        if version != 5 || app != APPLICATION_ID {
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
        clock::validate_schema(&ledger.connection)?;
        receiving::validate_schema(&ledger.connection)?;
        origin::validate_schema(&ledger.connection)?;
        ledger
            .connection
            .pragma_update(None, "foreign_keys", "OFF")?;
        let tx = ledger
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(origin_artifacts::SCHEMA)?;
        outgoing_renewal::initialize(&tx)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
        drop(ledger);
        Self::reopen(path, account)
    }
}
