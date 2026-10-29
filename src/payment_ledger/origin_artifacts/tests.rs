use super::*;
use std::path::PathBuf;
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "eltor-origin-artifacts-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("account.sqlite")
    }
    fn ledger(&self) -> PaymentLedger {
        PaymentLedger::provision(&self.file(), account()).unwrap()
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
fn intent() -> OutgoingIntent {
    OutgoingIntent {
        operation: [1; 16],
        account: account(),
        instance: [2; 16],
        handle: [3; 16],
        local_budget_scope: [4; 32],
        round: 1,
        kind: ChargeKind::Round,
        payment_id: [5; 32],
        charge_reference: [6; 32],
        payee_reference: [7; 32],
        accepted_terms_digest: [8; 32],
        hop_circuit_binding: [9; 32],
        hop_policy_hash: [10; 32],
        invoice_digest: digest(b"lnbc1synthetic-bound-artifact"),
        payment_hash: [12; 32],
        amount_msat: 1000,
        fee_limit_msat: 10,
        window: AcceptanceWindow {
            opens_at_ms: 100_000,
            closes_at_ms: 200_000,
        },
    }
}
fn accepted(ledger: &mut PaymentLedger) {
    ledger
        .save_origin_construction([4; 32], [2; 16], b"canonical constructor arguments")
        .unwrap();
    ledger.claim_origin_construction([4; 32]).unwrap();
    ledger
        .save_origin_plan([2; 16], [3; 16], [4; 32], b"immutable accepted path")
        .unwrap();
}
#[test]
fn constructor_dispatch_is_durable_single_use_and_cannot_adopt_a_restarted_instance() {
    let dir = Directory::new();
    let mut first = dir.ledger();
    first
        .save_origin_construction([4; 32], [2; 16], b"canonical constructor arguments")
        .unwrap();
    let mut second = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert!(
        first
            .claim_origin_construction([4; 32])
            .unwrap()
            .dispatch_claimed
    );
    assert!(matches!(
        second.claim_origin_construction([4; 32]),
        Err(LedgerError::DispatchAlreadyClaimed)
    ));
    assert!(second
        .save_origin_plan([8; 16], [3; 16], [4; 32], b"immutable accepted path")
        .is_err());
    second
        .save_origin_plan([2; 16], [3; 16], [4; 32], b"immutable accepted path")
        .unwrap();
    assert!(second
        .save_origin_plan([2; 16], [7; 16], [4; 32], b"immutable accepted path")
        .is_err());
    assert!(second
        .save_origin_construction([4; 32], [2; 16], b"changed path")
        .is_err());
    second.close_other_instances([8; 16]).unwrap();
    drop(first);
    drop(second);
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    let saved = ledger.get_origin_construction([4; 32]).unwrap().unwrap();
    assert!(saved.instance == [2; 16] && saved.dispatch_claimed);
    assert!(
        ledger
            .get_origin_plan([2; 16], [3; 16])
            .unwrap()
            .unwrap()
            .payload
            == b"immutable accepted path"
    );
    assert!(matches!(
        ledger.claim_origin_construction([4; 32]),
        Err(LedgerError::Closed)
    ));
    assert!(ledger
        .save_origin_construction([4; 32], [2; 16], b"canonical constructor arguments")
        .is_ok());
}
#[test]
fn saved_invoice_recovers_exact_original_intent_and_window_before_prepare_even_after_close() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    accepted(&mut ledger);
    let i = intent();
    ledger
        .save_origin_invoice(&i, "lnbc1synthetic-bound-artifact")
        .unwrap();
    assert!(ledger.get_outgoing(i.operation).unwrap().is_none());
    let reserved = ledger.storage_status().unwrap().reserved_write_bytes;
    ledger
        .save_origin_invoice(&i, "lnbc1synthetic-bound-artifact")
        .unwrap();
    assert_eq!(
        ledger.storage_status().unwrap().reserved_write_bytes,
        reserved
    );
    let mut changed = i.clone();
    changed.window.closes_at_ms += 1;
    assert!(matches!(
        ledger.save_origin_invoice(&changed, "lnbc1synthetic-bound-artifact"),
        Err(LedgerError::Conflict)
    ));
    ledger.close_binding(i.instance, i.handle).unwrap();
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    let (instance, handle, plan) = ledger
        .get_origin_plan_by_scope(i.local_budget_scope)
        .unwrap()
        .unwrap();
    assert!(
        instance == i.instance && handle == i.handle && plan.payload == b"immutable accepted path"
    );
    assert!(ledger.get_origin_plan_by_scope([99; 32]).unwrap().is_none());
    let saved = ledger.get_origin_invoice(i.operation).unwrap().unwrap();
    assert!(saved.intent == i && saved.artifact == "lnbc1synthetic-bound-artifact");
    assert!(ledger.save_origin_invoice(&i, &saved.artifact).is_ok());
    changed.operation = [18; 16];
    assert!(matches!(
        ledger.save_origin_invoice(&changed, &saved.artifact),
        Err(LedgerError::Closed)
    ));
}
#[test]
fn artifact_digest_account_bounds_and_corruption_fail_closed() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    accepted(&mut ledger);
    let mut i = intent();
    assert!(ledger.save_origin_invoice(&i, "lnbc1different").is_err());
    i.account = AccountId([8; 32]);
    assert!(ledger
        .save_origin_invoice(&i, "lnbc1synthetic-bound-artifact")
        .is_err());
    i = intent();
    assert!(ledger
        .save_origin_construction([8; 32], [2; 16], &vec![1; 65537])
        .is_err());
    assert!(ledger
        .save_origin_plan([2; 16], [8; 16], [4; 32], &[])
        .is_err());
    ledger
        .save_origin_invoice(&i, "lnbc1synthetic-bound-artifact")
        .unwrap();
    ledger
        .connection
        .execute("UPDATE origin_invoices SET artifact='lnbc1changed'", [])
        .unwrap();
    assert!(ledger.get_origin_invoice(i.operation).is_err());
    drop(ledger);
    assert!(matches!(
        PaymentLedger::reopen(&dir.file(), account()),
        Err(LedgerError::InvalidLedger)
    ));
}
#[test]
fn physical_pressure_preserves_existing_stage_and_never_yields_replacement_dispatch() {
    let dir = Directory::new();
    let mut ledger =
        PaymentLedger::provision_with_storage_budget(&dir.file(), account(), 4 * 1024 * 1024)
            .unwrap();
    ledger
        .save_origin_construction([1; 32], [2; 16], b"first immutable constructor")
        .unwrap();
    ledger.claim_origin_construction([1; 32]).unwrap();
    let mut refused = false;
    for n in 2u8..=32 {
        match ledger.save_origin_construction([n; 32], [2; 16], &vec![n; 65536]) {
            Ok(_) => (),
            Err(LedgerError::StoragePressure) => {
                refused = true;
                break;
            }
            Err(error) => panic!("unexpected storage error: {}", error),
        }
    }
    assert!(refused);
    assert!(
        ledger
            .get_origin_construction([1; 32])
            .unwrap()
            .unwrap()
            .dispatch_claimed
    );
    assert!(matches!(
        ledger.claim_origin_construction([1; 32]),
        Err(LedgerError::DispatchAlreadyClaimed)
    ));
}
#[test]
fn explicit_v5_upgrade_preserves_prior_finance_and_initializes_empty_artifact_journal() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    ledger.close_instance([8; 16]).unwrap();
    drop(ledger);
    let connection = Connection::open(dir.file()).unwrap();
    connection.execute_batch("DROP TABLE origin_outgoing_ids;DROP TABLE origin_allocations;DROP TABLE origin_renewal_operations;DROP TABLE origin_invoices;DROP TABLE origin_plans;DROP TABLE origin_constructions;PRAGMA user_version=5;").unwrap();
    drop(connection);
    assert!(PaymentLedger::reopen(&dir.file(), account()).is_err());
    assert!(matches!(
        PaymentLedger::upgrade_from_v5(&dir.file(), AccountId([8; 32])),
        Err(LedgerError::WrongAccount)
    ));
    let ledger = PaymentLedger::upgrade_from_v5(&dir.file(), account()).unwrap();
    assert!(ledger.binding_is_closed([8; 16], [2; 16]).unwrap());
    assert!(ledger.get_origin_construction([4; 32]).unwrap().is_none());
}
