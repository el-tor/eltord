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
        if version == 6 {
            drop(ledger);
            return Self::upgrade_from_v6(path, account);
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
        tx.execute_batch(handshake::SCHEMA)?;
        outgoing_renewal::initialize(&tx)?;
        // New retained-history ceilings must accept every existing record
        // before schema identity changes, just as on the v6 upgrade path.
        origin_artifacts::validate_schema(&tx, account)?;
        outgoing_renewal::validate_schema(&tx)?;
        handshake::validate_schema(&tx, account)?;
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
        if version == 6 {
            drop(ledger);
            return Self::upgrade_from_v6(path, account);
        }
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
        tx.execute_batch(handshake::SCHEMA)?;
        outgoing_renewal::initialize(&tx)?;
        // Refuse incompatible retained history atomically, without pruning or
        // leaving the account marked as upgraded after a failed reopen.
        origin_artifacts::validate_schema(&tx, account)?;
        outgoing_renewal::validate_schema(&tx)?;
        handshake::validate_schema(&tx, account)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
        drop(ledger);
        Self::reopen(path, account)
    }
}

impl PaymentLedger {
    /// Explicit v6->7 addition of preconstruction accounting. Existing costs,
    /// artifacts, dispatch latches and replay records are never reset.
    pub fn upgrade_from_v6(path: &Path, account: AccountId) -> Result<Self> {
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
        if version != 6 || app != APPLICATION_ID {
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
        origin_artifacts::validate_schema(&ledger.connection, account)?;
        outgoing_renewal::validate_schema(&ledger.connection)?;
        let tx = ledger
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(handshake::SCHEMA)?;
        // Validate the new retained-history admission profile before changing
        // schema identity. A refusal rolls back the additive tables as well;
        // callers retain the complete v6 ledger and can reconcile explicitly.
        handshake::validate_schema(&tx, account)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
        drop(ledger);
        Self::reopen(path, account)
    }
}

#[cfg(test)]
mod tests {
    use super::super::outgoing::{
        AccountBudget, FundingSource, HopSpendingTerms, OutgoingIntent, PathBudget,
    };
    use super::*;

    #[test]
    fn v4_v5_v6_over_cap_history_refusal_rolls_back_schema_and_preserves_all_records() {
        for version in [4, 5, 6] {
            over_cap_history_refusal(version);
        }
    }
    fn over_cap_history_refusal(version: i64) {
        let path = std::env::temp_dir().join(format!(
            "eltor-migration-cap-{:032x}.sqlite",
            rand::random::<u128>()
        ));
        let account = AccountId([1; 32]);
        let mut ledger = PaymentLedger::provision(&path, account).unwrap();
        ledger
            .configure_account_budget(AccountBudget {
                lifetime_limit_msat: 10000,
                outstanding_limit_msat: 10000,
                max_queued_intents: 128,
                max_wallet_rpcs: 1,
                max_unresolved_intents: 128,
            })
            .unwrap();
        // An older account may have >4096 resolved cancelled attempts accumulated
        // over time while concurrent pending work remains under its caps. Build
        // valid private financial rows in one fixture transaction, no wallet.
        let tx = ledger.connection.transaction().unwrap();
        for sequence in 1u32..=4097 {
            let scope: Hash32 = Sha256::digest(sequence.to_be_bytes()).into();
            let handle: Id16 = scope[..16].try_into().unwrap();
            let budget = PathBudget {
                instance: [2; 16],
                handle,
                local_budget_scope: scope,
                accepted_terms_digest: [3; 32],
                lifetime_limit_msat: 10000,
                outstanding_limit_msat: 10000,
                per_round_limit_msat: 1000,
                funding: FundingSource::OwnWallet,
            };
            let terms = HopSpendingTerms {
                local_budget_scope: scope,
                charge_reference: [4; 32],
                hop_circuit_binding: scope,
                hop_policy_hash: [5; 32],
                payee_reference: [6; 32],
                handshake_amount_msat: 0,
                round_amount_msat: 1,
                fee_limit_msat: 0,
            };
            let intent = OutgoingIntent {
                operation: handle,
                account,
                instance: budget.instance,
                handle,
                local_budget_scope: scope,
                round: 1,
                kind: ChargeKind::Round,
                payment_id: scope,
                charge_reference: terms.charge_reference,
                payee_reference: terms.payee_reference,
                accepted_terms_digest: budget.accepted_terms_digest,
                hop_circuit_binding: terms.hop_circuit_binding,
                hop_policy_hash: terms.hop_policy_hash,
                invoice_digest: [7; 32],
                payment_hash: scope,
                amount_msat: 1,
                fee_limit_msat: 0,
                window: AcceptanceWindow {
                    opens_at_ms: 100000,
                    closes_at_ms: 200000,
                },
            };
            tx.execute("INSERT INTO path_budgets(local_budget_scope,instance,handle,payload,current_round,closed) VALUES(?1,?2,?3,?4,1,1)",params![&scope[..],&budget.instance[..],&handle[..],encode(&budget).unwrap()]).unwrap();
            tx.execute("INSERT INTO hop_spending_terms(local_budget_scope,charge_reference,hop_circuit_binding,payload) VALUES(?1,?2,?3,?4)",params![&scope[..],&terms.charge_reference[..],&terms.hop_circuit_binding[..],encode(&terms).unwrap()]).unwrap();
            tx.execute("INSERT INTO outgoing_intents(operation,instance,local_budget_scope,round,kind,payee_reference,charge_reference,payment_id,payment_hash,payload,state,closed,reserved_msat,spent_msat,outstanding_msat,fee_breached) VALUES(?1,?2,?3,1,1,?4,?5,?6,?6,?7,7,1,0,0,0,0)",params![&handle[..],&budget.instance[..],&scope[..],&terms.payee_reference[..],&terms.charge_reference[..],&scope[..],encode(&intent).unwrap()]).unwrap();
        }
        tx.execute_batch("DROP TABLE handshake_intents;DROP TABLE handshake_initial_ids;DROP TABLE handshake_hops;DROP TABLE handshake_scopes;").unwrap();
        if version < 6 {
            tx.execute_batch("DROP TABLE origin_outgoing_ids;DROP TABLE origin_allocations;DROP TABLE origin_renewal_operations;DROP TABLE origin_invoices;DROP TABLE origin_plans;DROP TABLE origin_constructions;").unwrap();
        }
        if version == 4 {
            tx.execute_batch("DROP TABLE origin_operations;DROP TABLE native_invoice_candidates;DROP TABLE native_invoice_cursor;DROP TABLE closed_bindings;").unwrap();
        }
        tx.pragma_update(None, "user_version", version).unwrap();
        tx.commit().unwrap();
        fn snapshot(c: &Connection) -> Hash32 {
            let mut digest = Sha256::new();
            let mut schema = c
                .prepare("SELECT sql FROM sqlite_master WHERE type='table' ORDER BY name")
                .unwrap();
            for row in schema.query_map([], |r| r.get::<_, String>(0)).unwrap() {
                digest.update(row.unwrap());
            }
            for table in ["path_budgets", "hop_spending_terms", "outgoing_intents"] {
                let mut query = c
                    .prepare(&format!("SELECT payload FROM {table} ORDER BY payload"))
                    .unwrap();
                for row in query.query_map([], |r| r.get::<_, Vec<u8>>(0)).unwrap() {
                    digest.update(row.unwrap());
                }
            }
            digest.finalize().into()
        }
        let original = snapshot(&ledger.connection);
        drop(ledger);
        let result = match version {
            4 => PaymentLedger::upgrade_from_v4(&path, account),
            5 => PaymentLedger::upgrade_from_v5(&path, account),
            6 => PaymentLedger::upgrade_from_v6(&path, account),
            _ => unreachable!(),
        };
        assert!(matches!(result, Err(LedgerError::InvalidLedger)));
        let c = Connection::open(&path).unwrap();
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            version
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM outgoing_intents", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            4097
        );
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name LIKE 'handshake_%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(snapshot(&c), original);
        drop(c);
        std::fs::remove_file(path).unwrap();
    }
}
