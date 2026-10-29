use super::super::outgoing::*;
use super::*;
use std::path::PathBuf;
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("eltor-storage-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self) -> PathBuf {
        self.0.join("account.sqlite")
    }
    fn ledger(&self) -> PaymentLedger {
        PaymentLedger::provision_with_storage_budget(&self.file(), account(), 8 * 1024 * 1024)
            .unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn account() -> AccountId {
    AccountId([9; 32])
}
fn clock() -> ClockObservation {
    ClockObservation {
        now_ms: 150_000,
        uncertainty_ms: 0,
    }
}
fn obligation(n: u8) -> Obligation {
    Obligation {
        instance: [1; 16],
        handle: [n; 16],
        circuit_binding: [n; 32],
        round: 0,
        kind: ChargeKind::Handshake,
        payment_id: [n + 10; 32],
        amount_msat: 1000,
        expected_payment_hash: Some([n + 20; 32]),
        policy_hash: [4; 32],
        allocation_hash: [5; 32],
        window: Some(AcceptanceWindow {
            opens_at_ms: 100_000,
            closes_at_ms: 200_000,
        }),
    }
}
fn receipt(o: &Obligation) -> RecipientEvidence {
    RecipientEvidence {
        account: account(),
        payment_hash: o.expected_payment_hash.unwrap(),
        invoice_digest: [8; 32],
        amount_msat: o.amount_msat,
        settled_at_ms: 140_000,
        settlement_uncertainty_ms: 0,
        incoming: true,
        lightning: true,
        completed: true,
        authenticated_payment_id: None,
    }
}
fn intent(n: u8) -> OutgoingIntent {
    OutgoingIntent {
        operation: [n; 16],
        account: account(),
        instance: [1; 16],
        handle: [2; 16],
        local_budget_scope: [3; 32],
        round: 1,
        kind: ChargeKind::Round,
        payment_id: [n + 30; 32],
        charge_reference: [n + 40; 32],
        payee_reference: [n + 50; 32],
        accepted_terms_digest: [4; 32],
        hop_circuit_binding: [n + 60; 32],
        hop_policy_hash: [n + 70; 32],
        invoice_digest: [n + 80; 32],
        payment_hash: [n + 90; 32],
        amount_msat: 1000,
        fee_limit_msat: 100,
        window: AcceptanceWindow {
            opens_at_ms: 100_000,
            closes_at_ms: 200_000,
        },
    }
}
fn setup(ledger: &mut PaymentLedger) {
    ledger
        .configure_account_budget(AccountBudget {
            lifetime_limit_msat: 100_000,
            outstanding_limit_msat: 100_000,
            max_queued_intents: 128,
            max_wallet_rpcs: 32,
            max_unresolved_intents: 100,
        })
        .unwrap();
    ledger
        .configure_path_budget(&PathBudget {
            instance: [1; 16],
            handle: [2; 16],
            local_budget_scope: [3; 32],
            accepted_terms_digest: [4; 32],
            lifetime_limit_msat: 100_000,
            outstanding_limit_msat: 100_000,
            per_round_limit_msat: 100_000,
            funding: FundingSource::OwnWallet,
        })
        .unwrap();
    for n in 1..4 {
        let i = intent(n);
        ledger
            .configure_hop_spending_terms(&HopSpendingTerms {
                local_budget_scope: i.local_budget_scope,
                charge_reference: i.charge_reference,
                hop_circuit_binding: i.hop_circuit_binding,
                hop_policy_hash: i.hop_policy_hash,
                payee_reference: i.payee_reference,
                handshake_amount_msat: 50,
                round_amount_msat: i.amount_msat,
                fee_limit_msat: 100,
            })
            .unwrap();
    }
}
fn fill_to(ledger: &mut PaymentLedger, percent: u64) {
    ledger
        .connection
        .execute_batch("CREATE TABLE IF NOT EXISTS fixture_ballast(value BLOB);")
        .unwrap();
    while ledger.storage_status().unwrap().charged_bytes < 8 * 1024 * 1024 * percent / 100 {
        ledger
            .connection
            .execute(
                "INSERT INTO fixture_ballast(value) VALUES(zeroblob(8192))",
                [],
            )
            .unwrap();
    }
}
fn outcome(i: &OutgoingIntent) -> OutgoingEvidence {
    OutgoingEvidence {
        account: account(),
        operation: i.operation,
        payment_hash: i.payment_hash,
        invoice_digest: i.invoice_digest,
        amount_msat: i.amount_msat,
        fee_msat: 20,
        outcome: NativeOutcome::Settled,
    }
}

#[test]
fn immutable_physical_budget_and_pending_write_reservations_survive_reopen_and_retries() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    setup(&mut ledger);
    let before = ledger.storage_status().unwrap();
    assert!(before.budget_bytes == 8 * 1024 * 1024 && before.database_bytes > 0);
    let o = obligation(1);
    ledger.register_handshake(&o).unwrap();
    ledger.prepare_outgoing(&intent(1), clock()).unwrap();
    let status = ledger.storage_status().unwrap();
    assert!(status.reserved_write_bytes == 2 * OBLIGATION_WRITE_RESERVE_BYTES);
    assert!(
        status.rollback_headroom_bytes >= status.database_bytes
            && status.charged_bytes > status.database_bytes + status.reserved_write_bytes
    );
    ledger.register_handshake(&o).unwrap();
    ledger.prepare_outgoing(&intent(1), clock()).unwrap();
    assert!(ledger.storage_status().unwrap().reserved_write_bytes == status.reserved_write_bytes);
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert!(ledger.storage_status().unwrap().budget_bytes == status.budget_bytes);
    ledger
        .authorize(&o, &receipt(&o), [6; 16], clock())
        .unwrap();
    assert!(
        ledger.storage_status().unwrap().reserved_write_bytes == OBLIGATION_WRITE_RESERVE_BYTES
    );
    ledger.cancel_before_dispatch(intent(1).operation).unwrap();
    assert!(ledger.storage_status().unwrap().reserved_write_bytes == 0);
    assert!(ledger.get_authorization(&o).unwrap().is_some());
    assert!(ledger.get_outgoing(intent(1).operation).unwrap().is_some());
}

#[test]
fn pressure_stops_admissions_at_eighty_but_existing_financial_work_at_ninety_five() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    setup(&mut ledger);
    let o = obligation(1);
    ledger.register_handshake(&o).unwrap();
    ledger.prepare_outgoing(&intent(1), clock()).unwrap();
    fill_to(&mut ledger, 82);
    let pressure = ledger.storage_status().unwrap();
    assert!(!pressure.admissions_open && pressure.dispatch_open);
    assert!(matches!(
        ledger.register_handshake(&obligation(2)),
        Err(LedgerError::StoragePressure)
    ));
    // Exact retries recover durable decisions without treating them as new work.
    assert!(ledger.register_handshake(&o).is_ok());
    ledger.begin_dispatch(intent(1).operation, clock()).unwrap();
    ledger.prepare_outgoing(&intent(2), clock()).unwrap();
    fill_to(&mut ledger, 96);
    assert!(!ledger.storage_status().unwrap().dispatch_open);
    assert!(matches!(
        ledger.begin_dispatch(intent(2).operation, clock()),
        Err(LedgerError::StoragePressure)
    ));
    assert!(
        ledger
            .get_outgoing(intent(2).operation)
            .unwrap()
            .unwrap()
            .state
            == OutgoingState::Prepared
    );
    // Existing promised reconciliation still commits and preserves replay rows.
    ledger
        .authorize(&o, &receipt(&o), [6; 16], clock())
        .unwrap();
    ledger
        .record_outgoing_outcome(&outcome(&intent(1)))
        .unwrap();
    assert!(
        ledger
            .get_outgoing(intent(1).operation)
            .unwrap()
            .unwrap()
            .spent_msat
            == 1020
    );
    let claims: i64 = ledger
        .connection
        .query_row("SELECT count(*) FROM incoming_claims", [], |r| r.get(0))
        .unwrap();
    assert!(claims == 1);
}

#[test]
fn failed_batch_headroom_reservation_is_atomic_and_never_reserves_partial_ids() {
    let dir = Directory::new();
    let mut ledger =
        PaymentLedger::provision_with_storage_budget(&dir.file(), account(), MIN_BUDGET_BYTES)
            .unwrap();
    ledger.connection.execute_batch("CREATE TABLE fixture_ballast(value BLOB);INSERT INTO fixture_ballast VALUES(zeroblob(524288));").unwrap();
    let batch = BatchRegistration {
        period: 0,
        request_digest: [20; 32],
        rounds: std::array::from_fn(|n| {
            let mut o = obligation(1);
            o.kind = ChargeKind::Round;
            o.round = n as u64 + 1;
            o.payment_id = [n as u8 + 1; 32];
            o.window = None;
            o
        }),
    };
    assert!(matches!(
        ledger.reserve_batch(&batch, [8; 16]),
        Err(LedgerError::StoragePressure)
    ));
    for table in [
        "batches",
        "registrations",
        "payment_ids",
        "storage_reservations",
    ] {
        let count: i64 = ledger
            .connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert!(count == 0);
    }
}

#[test]
fn unknown_closure_keeps_headroom_and_history_and_terminal_resolution_releases_only_reserve() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    setup(&mut ledger);
    ledger.prepare_outgoing(&intent(1), clock()).unwrap();
    ledger.prepare_outgoing(&intent(2), clock()).unwrap();
    ledger.begin_dispatch(intent(1).operation, clock()).unwrap();
    ledger.mark_outgoing_unknown(intent(1).operation).unwrap();
    ledger.close_instance(intent(1).instance).unwrap();
    assert!(
        ledger.storage_status().unwrap().reserved_write_bytes == OBLIGATION_WRITE_RESERVE_BYTES
    );
    assert!(
        ledger
            .get_outgoing(intent(1).operation)
            .unwrap()
            .unwrap()
            .state
            == OutgoingState::ClosedUnresolved
    );
    ledger
        .record_outgoing_outcome(&outcome(&intent(1)))
        .unwrap();
    assert!(ledger.storage_status().unwrap().reserved_write_bytes == 0);
    let rows: i64 = ledger
        .connection
        .query_row("SELECT count(*) FROM outgoing_intents", [], |r| r.get(0))
        .unwrap();
    assert!(rows == 2);
}

#[test]
fn explicit_v2_upgrade_preserves_unresolved_reservations_and_authorization_history() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    setup(&mut ledger);
    let pending = obligation(1);
    ledger.register_handshake(&pending).unwrap();
    let paid = obligation(2);
    ledger.register_handshake(&paid).unwrap();
    ledger
        .authorize(&paid, &receipt(&paid), [6; 16], clock())
        .unwrap();
    ledger.prepare_outgoing(&intent(1), clock()).unwrap();
    ledger.begin_dispatch(intent(1).operation, clock()).unwrap();
    ledger
        .connection
        .execute_batch(
            "DROP TABLE initial_clock_anchors;DROP TABLE activation_interval_anchors;DROP TABLE storage_reservations;DROP TABLE storage_policy;PRAGMA user_version=2;",
        )
        .unwrap();
    drop(ledger);
    assert!(PaymentLedger::reopen(&dir.file(), account()).is_err());
    assert!(PaymentLedger::upgrade_from_v2(&dir.file(), AccountId([8; 32])).is_err());
    let mut ledger = PaymentLedger::upgrade_from_v2(&dir.file(), account()).unwrap();
    assert!(
        ledger.storage_status().unwrap().reserved_write_bytes == 2 * OBLIGATION_WRITE_RESERVE_BYTES
    );
    assert!(ledger.storage_status().unwrap().budget_bytes == DEFAULT_BUDGET_BYTES);
    assert!(ledger.get_authorization(&paid).unwrap().is_some());
    assert!(
        ledger
            .get_outgoing(intent(1).operation)
            .unwrap()
            .unwrap()
            .state
            == OutgoingState::InFlight
    );
    assert!(ledger.begin_dispatch(intent(1).operation, clock()).is_err());
    drop(ledger);
    assert!(PaymentLedger::upgrade_from_v2(&dir.file(), account()).is_ok());
}

#[test]
fn missing_reservation_corruption_and_invalid_policy_never_reopen_as_empty_history() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    let o = obligation(1);
    ledger.register_handshake(&o).unwrap();
    ledger
        .connection
        .execute("DELETE FROM storage_reservations", [])
        .unwrap();
    drop(ledger);
    assert!(PaymentLedger::reopen(&dir.file(), account()).is_err());
    let dir = Directory::new();
    assert!(PaymentLedger::provision_with_storage_budget(
        &dir.file(),
        account(),
        DEFAULT_BUDGET_BYTES + 1
    )
    .is_err());
    assert!(!dir.file().exists());
}

#[cfg(unix)]
#[test]
fn unexpected_sidecar_symlink_fails_closed_and_page_quota_preserves_history_on_full() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    let o = obligation(1);
    ledger.register_handshake(&o).unwrap();
    let target = dir.0.join("fixture-sidecar");
    std::fs::write(&target, []).unwrap();
    let mut journal = dir.file().into_os_string();
    journal.push("-journal");
    std::os::unix::fs::symlink(&target, &journal).unwrap();
    assert!(ledger.storage_status().is_err());
    assert!(ledger.register_handshake(&obligation(2)).is_err());
    std::fs::remove_file(&journal).unwrap();
    ledger
        .connection
        .execute_batch("CREATE TABLE fixture_ballast(value BLOB)")
        .unwrap();
    assert!(ledger
        .connection
        .execute("INSERT INTO fixture_ballast VALUES(zeroblob(8388608))", [])
        .is_err());
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert!(ledger.get_authorization(&o).unwrap().is_none());
    assert!(ledger.register_handshake(&o).is_ok());
    assert!(
        ledger.storage_status().unwrap().reserved_write_bytes == OBLIGATION_WRITE_RESERVE_BYTES
    );
}

#[cfg(unix)]
#[test]
fn deleted_or_replaced_authoritative_file_blocks_live_ledger_before_new_spend() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    setup(&mut ledger);
    ledger.prepare_outgoing(&intent(1), clock()).unwrap();
    std::fs::rename(dir.file(), dir.0.join("retained-authority.sqlite")).unwrap();
    std::fs::write(dir.file(), []).unwrap();
    assert!(ledger.storage_status().is_err());
    assert!(ledger.begin_dispatch(intent(1).operation, clock()).is_err());
    assert!(ledger.register_handshake(&obligation(1)).is_err());
    assert!(std::fs::metadata(dir.file()).unwrap().len() == 0);
    drop(ledger);
    assert!(PaymentLedger::reopen(&dir.file(), account()).is_err());
    assert!(PaymentLedger::provision(&dir.file(), account()).is_err());
}
