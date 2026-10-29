use super::*;
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("eltor-outgoing-{:032x}", rand::random::<u128>()));
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
fn clock() -> ClockObservation {
    ClockObservation {
        now_ms: 150_000,
        uncertainty_ms: 0,
    }
}
fn budget() -> AccountBudget {
    AccountBudget {
        lifetime_limit_msat: 10_000,
        outstanding_limit_msat: 10_000,
        max_queued_intents: 128,
        max_wallet_rpcs: 32,
        max_unresolved_intents: 160,
    }
}
fn circuit() -> PathBudget {
    PathBudget {
        instance: [1; 16],
        handle: [2; 16],
        local_budget_scope: [3; 32],
        accepted_terms_digest: [11; 32],
        lifetime_limit_msat: 10_000,
        outstanding_limit_msat: 10_000,
        per_round_limit_msat: 5_000,
        funding: FundingSource::OwnWallet,
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
        payment_id: [40 + n; 32],
        charge_reference: [80 + n; 32],
        payee_reference: [60 + n; 32],
        accepted_terms_digest: [11; 32],
        hop_circuit_binding: [20 + n; 32],
        hop_policy_hash: [30 + n; 32],
        invoice_digest: [120 + n; 32],
        payment_hash: [100 + n; 32],
        amount_msat: 1_000,
        fee_limit_msat: 100,
        window: AcceptanceWindow {
            opens_at_ms: 100_000,
            closes_at_ms: 200_000,
        },
    }
}
fn hop_terms(n: u8, scope: Hash32) -> HopSpendingTerms {
    let i = intent(n);
    HopSpendingTerms {
        local_budget_scope: scope,
        charge_reference: i.charge_reference,
        hop_circuit_binding: i.hop_circuit_binding,
        hop_policy_hash: i.hop_policy_hash,
        payee_reference: i.payee_reference,
        handshake_amount_msat: 50,
        round_amount_msat: i.amount_msat,
        fee_limit_msat: 100,
    }
}
fn configure_hops(ledger: &mut PaymentLedger, path: &PathBudget) {
    for n in 1..=3 {
        ledger
            .configure_hop_spending_terms(&hop_terms(n, path.local_budget_scope))
            .unwrap();
    }
}
fn evidence(i: &OutgoingIntent, outcome: NativeOutcome, fee_msat: u64) -> OutgoingEvidence {
    OutgoingEvidence {
        account: i.account,
        operation: i.operation,
        payment_hash: i.payment_hash,
        invoice_digest: i.invoice_digest,
        amount_msat: i.amount_msat,
        fee_msat,
        outcome,
    }
}
fn setup() -> (Directory, PaymentLedger) {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    ledger.configure_account_budget(budget()).unwrap();
    ledger.configure_path_budget(&circuit()).unwrap();
    configure_hops(&mut ledger, &circuit());
    (dir, ledger)
}

#[test]
fn prepare_is_immutable_and_recoverable_without_dispatch_permission() {
    let (dir, mut ledger) = setup();
    let i = intent(1);
    let first = ledger.prepare_outgoing(&i, clock()).unwrap();
    assert!(first.state == OutgoingState::Prepared);
    assert_eq!(first.reserved_msat, 1100);
    assert_eq!(first.outstanding_msat, 1100);
    assert!(ledger.prepare_outgoing(&i, clock()).unwrap() == first);
    let mut changed = i.clone();
    changed.invoice_digest = [89; 32];
    assert!(matches!(
        ledger.prepare_outgoing(&changed, clock()),
        Err(LedgerError::Conflict)
    ));
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert!(ledger.get_outgoing(i.operation).unwrap().unwrap() == first);
    assert!(
        ledger
            .prepare_outgoing(
                &i,
                ClockObservation {
                    now_ms: 300_000,
                    uncertainty_ms: 0
                }
            )
            .unwrap()
            == first
    );
    let mut new = intent(2);
    new.window.closes_at_ms = 100_001;
    assert!(matches!(
        ledger.prepare_outgoing(&new, clock()),
        Err(LedgerError::OutsideWindow)
    ));
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 1100);
}

#[test]
fn unknown_outcome_retains_full_exposure_and_never_redispatches() {
    let (dir, mut ledger) = setup();
    let i = intent(1);
    ledger.prepare_outgoing(&i, clock()).unwrap();
    assert!(
        ledger
            .begin_dispatch(i.operation, clock())
            .unwrap()
            .into_intent()
            == i
    );
    ledger.mark_outgoing_unknown(i.operation).unwrap();
    ledger.mark_outgoing_unknown(i.operation).unwrap();
    assert!(ledger.begin_dispatch(i.operation, clock()).is_err());
    assert!(ledger.cancel_before_dispatch(i.operation).is_err());
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert!(matches!(
        ledger.begin_dispatch(i.operation, clock()),
        Err(LedgerError::DispatchAlreadyClaimed)
    ));
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 1100);
    for change in 0..3 {
        let mut duplicate = intent(2);
        match change {
            0 => duplicate.payment_hash = i.payment_hash,
            1 => duplicate.payment_id = i.payment_id,
            _ => duplicate.charge_reference = i.charge_reference,
        };
        assert!(matches!(
            ledger.prepare_outgoing(&duplicate, clock()),
            Err(LedgerError::Conflict)
        ));
    }
}

#[test]
fn native_settlement_preserves_lifetime_cost_and_retires_only_unused_fee_reserve() {
    let (_dir, mut ledger) = setup();
    let i = intent(1);
    ledger.prepare_outgoing(&i, clock()).unwrap();
    ledger.begin_dispatch(i.operation, clock()).unwrap();
    let e = evidence(&i, NativeOutcome::Settled, 30);
    for change in 0..4 {
        let mut wrong = e.clone();
        match change {
            0 => wrong.account = AccountId([8; 32]),
            1 => wrong.payment_hash = [89; 32],
            2 => wrong.invoice_digest = [89; 32],
            _ => wrong.amount_msat += 1,
        };
        assert!(ledger.record_outgoing_outcome(&wrong).is_err());
    }
    let settled = ledger.record_outgoing_outcome(&e).unwrap();
    assert!(settled.state == OutgoingState::Settled);
    assert_eq!(
        (
            settled.reserved_msat,
            settled.spent_msat,
            settled.outstanding_msat
        ),
        (0, 1030, 1030)
    );
    assert!(ledger.record_outgoing_outcome(&e).unwrap() == settled);
    ledger.mark_outgoing_consumed(i.operation).unwrap();
    ledger.mark_outgoing_consumed(i.operation).unwrap();
    let consumed = ledger.get_outgoing(i.operation).unwrap().unwrap();
    assert!(consumed.state == OutgoingState::Consumed);
    assert_eq!(consumed.spent_msat, 1030);
    assert_eq!(consumed.outstanding_msat, 0);
    assert!(ledger.record_outgoing_outcome(&e).unwrap() == consumed);
    assert!(matches!(
        ledger.record_outgoing_outcome(&evidence(&i, NativeOutcome::FailedFinal, 0)),
        Err(LedgerError::Conflict)
    ));
}

#[test]
fn confirmed_failure_releases_only_its_own_reservation() {
    let (_dir, mut ledger) = setup();
    let first = intent(1);
    let second = intent(2);
    ledger.prepare_outgoing(&first, clock()).unwrap();
    ledger.prepare_outgoing(&second, clock()).unwrap();
    ledger.begin_dispatch(first.operation, clock()).unwrap();
    ledger.mark_outgoing_unknown(first.operation).unwrap();
    let failed = ledger
        .record_outgoing_outcome(&evidence(&first, NativeOutcome::FailedFinal, 10))
        .unwrap();
    assert!(failed.state == OutgoingState::FailedFinal);
    assert_eq!(
        (
            failed.reserved_msat,
            failed.spent_msat,
            failed.outstanding_msat
        ),
        (0, 10, 0)
    );
    let exposure = ledger.account_exposure().unwrap();
    assert_eq!((exposure.reserved_msat, exposure.spent_msat), (1100, 10));
    let mut duplicate = intent(3);
    duplicate.payment_hash = first.payment_hash;
    assert!(ledger.prepare_outgoing(&duplicate, clock()).is_err());
    assert!(ledger.cancel_before_dispatch(second.operation).is_ok());
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 0);
}

#[test]
fn closing_instance_cancels_only_never_dispatched_intents() {
    let (dir, mut ledger) = setup();
    let first = intent(1);
    let second = intent(2);
    ledger.prepare_outgoing(&first, clock()).unwrap();
    ledger.prepare_outgoing(&second, clock()).unwrap();
    ledger.begin_dispatch(first.operation, clock()).unwrap();
    ledger.close_instance(first.instance).unwrap();
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    let first_record = ledger.get_outgoing(first.operation).unwrap().unwrap();
    assert!(first_record.state == OutgoingState::ClosedUnresolved);
    assert_eq!(first_record.reserved_msat, 1100);
    assert!(
        ledger
            .get_outgoing(second.operation)
            .unwrap()
            .unwrap()
            .state
            == OutgoingState::CancelledBeforeDispatch
    );
    assert!(ledger.begin_dispatch(first.operation, clock()).is_err());
    assert!(ledger.prepare_outgoing(&intent(3), clock()).is_err());
    let settled = ledger
        .record_outgoing_outcome(&evidence(&first, NativeOutcome::Settled, 30))
        .unwrap();
    assert!(settled.closed);
    assert_eq!(settled.spent_msat, 1030);
    assert!(matches!(
        ledger.mark_outgoing_consumed(first.operation),
        Err(LedgerError::Closed)
    ));
}

#[test]
fn concurrent_circuits_cannot_oversubscribe_wallet_budget() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    let mut b = budget();
    b.lifetime_limit_msat = 1100;
    b.outstanding_limit_msat = 1100;
    ledger.configure_account_budget(b).unwrap();
    for n in 1..=2 {
        let mut c = circuit();
        c.handle = [n; 16];
        c.local_budget_scope = [n; 32];
        ledger.configure_path_budget(&c).unwrap();
        configure_hops(&mut ledger, &c);
    }
    drop(ledger);
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (1..=2)
        .map(|n| {
            let path = dir.file();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut ledger = PaymentLedger::reopen(&path, account()).unwrap();
                let mut i = intent(n);
                i.handle = [n; 16];
                i.local_budget_scope = [n; 32];
                barrier.wait();
                ledger.prepare_outgoing(&i, clock()).is_ok()
            })
        })
        .collect();
    let mut wins = 0;
    for thread in threads {
        if thread.join().unwrap() {
            wins += 1;
        }
    }
    assert_eq!(wins, 1);
    let ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 1100);
}

#[test]
fn lifetime_round_and_current_next_caps_do_not_reset() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    ledger.configure_account_budget(budget()).unwrap();
    let mut c = circuit();
    c.lifetime_limit_msat = 2150;
    c.outstanding_limit_msat = 2150;
    c.per_round_limit_msat = 1150;
    ledger.configure_path_budget(&c).unwrap();
    configure_hops(&mut ledger, &c);
    let first = intent(1);
    ledger.prepare_outgoing(&first, clock()).unwrap();
    assert!(matches!(
        ledger.prepare_outgoing(&intent(2), clock()),
        Err(LedgerError::BudgetExceeded)
    ));
    let mut third = intent(3);
    third.round = 3;
    assert!(ledger.prepare_outgoing(&third, clock()).is_err());
    let mut second = intent(2);
    second.round = 2;
    assert!(ledger.prepare_outgoing(&second, clock()).is_err());
    ledger.begin_dispatch(first.operation, clock()).unwrap();
    ledger
        .record_outgoing_outcome(&evidence(&first, NativeOutcome::Settled, 30))
        .unwrap();
    assert!(ledger
        .advance_spending_round(c.local_budget_scope, 2)
        .is_err());
    ledger.mark_outgoing_consumed(first.operation).unwrap();
    ledger
        .advance_spending_round(c.local_budget_scope, 2)
        .unwrap();
    assert!(ledger.prepare_outgoing(&second, clock()).is_ok());
    assert_eq!(
        ledger
            .path_exposure(c.local_budget_scope)
            .unwrap()
            .spent_msat,
        1030
    );
    let mut changed = c.clone();
    changed.lifetime_limit_msat += 100;
    assert!(matches!(
        ledger.configure_path_budget(&changed),
        Err(LedgerError::Conflict)
    ));
    let mut round11 = intent(4);
    round11.round = 11;
    assert!(ledger.prepare_outgoing(&round11, clock()).is_err());
    assert!(ledger
        .advance_spending_round(c.local_budget_scope, 4)
        .is_err());
}

#[test]
fn bounded_queue_rpc_and_unresolved_work_are_distinct_from_money() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    let mut b = budget();
    b.max_queued_intents = 1;
    b.max_wallet_rpcs = 1;
    b.max_unresolved_intents = 2;
    ledger.configure_account_budget(b).unwrap();
    ledger.configure_path_budget(&circuit()).unwrap();
    configure_hops(&mut ledger, &circuit());
    let first = intent(1);
    let second = intent(2);
    ledger.prepare_outgoing(&first, clock()).unwrap();
    assert!(ledger.prepare_outgoing(&second, clock()).is_err());
    ledger.begin_dispatch(first.operation, clock()).unwrap();
    ledger.prepare_outgoing(&second, clock()).unwrap();
    assert!(ledger.begin_dispatch(second.operation, clock()).is_err());
    ledger.mark_outgoing_unknown(first.operation).unwrap();
    ledger.begin_dispatch(second.operation, clock()).unwrap();
    assert!(ledger.prepare_outgoing(&intent(3), clock()).is_err());
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 2200);
    ledger
        .record_outgoing_outcome(&evidence(&first, NativeOutcome::Settled, 0))
        .unwrap();
    assert!(ledger.prepare_outgoing(&intent(3), clock()).is_err());
    ledger.mark_outgoing_consumed(first.operation).unwrap();
    assert!(ledger.prepare_outgoing(&intent(3), clock()).is_ok());
}

#[test]
fn recipient_funding_is_explicitly_unsupported_without_a_separate_mapping() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    ledger.configure_account_budget(budget()).unwrap();
    let mut c = circuit();
    c.funding = FundingSource::RecipientSettlement;
    assert!(matches!(
        ledger.configure_path_budget(&c),
        Err(LedgerError::UnsupportedOperation)
    ));
    assert!(ledger.prepare_outgoing(&intent(1), clock()).is_err());
}

#[test]
fn native_fee_limit_violation_records_actual_cost_and_blocks_new_work() {
    let (_dir, mut ledger) = setup();
    let i = intent(1);
    ledger.prepare_outgoing(&i, clock()).unwrap();
    ledger.prepare_outgoing(&intent(2), clock()).unwrap();
    ledger.begin_dispatch(i.operation, clock()).unwrap();
    let record = ledger
        .record_outgoing_outcome(&evidence(&i, NativeOutcome::Settled, 150))
        .unwrap();
    assert!(record.fee_limit_breached);
    assert_eq!(record.spent_msat, 1150);
    assert!(ledger.outgoing_admission_blocked().unwrap());
    assert!(ledger.prepare_outgoing(&intent(3), clock()).is_err());
    assert!(ledger.begin_dispatch(intent(2).operation, clock()).is_err());
}

#[test]
fn unrepresentable_native_cost_freezes_admission_and_preserves_uncertainty() {
    let (_dir, mut ledger) = setup();
    let mut huge = intent(1);
    huge.amount_msat = u64::MAX;
    assert!(ledger.prepare_outgoing(&huge, clock()).is_err());
    let i = intent(1);
    ledger.prepare_outgoing(&i, clock()).unwrap();
    ledger.begin_dispatch(i.operation, clock()).unwrap();
    assert!(ledger
        .record_outgoing_outcome(&evidence(&i, NativeOutcome::Settled, u64::MAX))
        .is_err());
    assert!(ledger.outgoing_admission_blocked().unwrap());
    let record = ledger.get_outgoing(i.operation).unwrap().unwrap();
    assert!(record.state == OutgoingState::Unknown);
    assert_eq!(record.reserved_msat, 1100);
    assert!(ledger.prepare_outgoing(&intent(2), clock()).is_err());
}

#[test]
fn explicit_v1_upgrade_preserves_incoming_history() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    let obligation = Obligation {
        instance: [1; 16],
        handle: [2; 16],
        circuit_binding: [3; 32],
        round: 0,
        kind: ChargeKind::Handshake,
        payment_id: [200; 32],
        amount_msat: 1000,
        expected_payment_hash: Some([220; 32]),
        policy_hash: [6; 32],
        allocation_hash: [7; 32],
        window: Some(AcceptanceWindow {
            opens_at_ms: 100_000,
            closes_at_ms: 200_000,
        }),
    };
    let receipt = RecipientEvidence {
        account: account(),
        payment_hash: [220; 32],
        invoice_digest: [221; 32],
        amount_msat: 1000,
        settled_at_ms: 140_000,
        settlement_uncertainty_ms: 0,
        incoming: true,
        lightning: true,
        completed: true,
        authenticated_payment_id: None,
    };
    ledger.register_handshake(&obligation).unwrap();
    let before = ledger
        .authorize(&obligation, &receipt, [30; 16], clock())
        .unwrap();
    // Simulate the previous version's schema, with its durable account record.
    ledger.connection.execute_batch("DROP TABLE initial_clock_anchors;DROP TABLE activation_interval_anchors;DROP TABLE storage_reservations; DROP TABLE storage_policy; DROP TABLE outgoing_intents; DROP TABLE hop_spending_terms; DROP TABLE path_budgets; DROP TABLE account_budget; PRAGMA user_version=1;").unwrap();
    drop(ledger);
    assert!(PaymentLedger::reopen(&dir.file(), account()).is_err());
    assert!(PaymentLedger::upgrade_from_v1(&dir.file(), AccountId([8; 32])).is_err());
    let mut upgraded = PaymentLedger::upgrade_from_v1(&dir.file(), account()).unwrap();
    assert!(upgraded.get_authorization(&obligation).unwrap().unwrap() == before);
    assert!(
        upgraded
            .authorize(&obligation, &receipt, [30; 16], clock())
            .unwrap()
            == before
    );
    upgraded.configure_account_budget(budget()).unwrap();
    upgraded.configure_path_budget(&circuit()).unwrap();
    configure_hops(&mut upgraded, &circuit());
    upgraded.prepare_outgoing(&intent(1), clock()).unwrap();
    drop(upgraded);
    let upgraded = PaymentLedger::upgrade_from_v1(&dir.file(), account()).unwrap();
    assert_eq!(upgraded.account_exposure().unwrap().reserved_msat, 1100);
}

#[test]
fn one_circuit_closure_does_not_close_a_different_budget() {
    let (_dir, mut ledger) = setup();
    let first = intent(1);
    ledger.prepare_outgoing(&first, clock()).unwrap();
    ledger.begin_dispatch(first.operation, clock()).unwrap();
    let mut c = circuit();
    c.handle = [4; 16];
    c.local_budget_scope = [5; 32];
    ledger.configure_path_budget(&c).unwrap();
    configure_hops(&mut ledger, &c);
    ledger
        .close_outgoing_path(first.local_budget_scope)
        .unwrap();
    assert!(
        ledger.get_outgoing(first.operation).unwrap().unwrap().state
            == OutgoingState::ClosedUnresolved
    );
    let mut second = intent(2);
    second.handle = c.handle;
    second.local_budget_scope = c.local_budget_scope;
    assert!(ledger.prepare_outgoing(&second, clock()).is_ok());
}

#[test]
fn prepare_and_outcome_storage_failures_cannot_change_exposure() {
    let (_dir, mut ledger) = setup();
    let i = intent(1);
    ledger.connection.execute_batch("CREATE TRIGGER fail_prepare BEFORE INSERT ON outgoing_intents BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(ledger.prepare_outgoing(&i, clock()).is_err());
    assert_eq!(ledger.account_exposure().unwrap(), Exposure::default());
    ledger
        .connection
        .execute_batch("DROP TRIGGER fail_prepare")
        .unwrap();
    ledger.prepare_outgoing(&i, clock()).unwrap();
    ledger.begin_dispatch(i.operation, clock()).unwrap();
    ledger.connection.execute_batch("CREATE TRIGGER fail_outcome BEFORE UPDATE ON outgoing_intents BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(ledger
        .record_outgoing_outcome(&evidence(&i, NativeOutcome::Settled, 20))
        .is_err());
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 1100);
    assert_eq!(ledger.account_exposure().unwrap().spent_msat, 0);
    ledger
        .connection
        .execute_batch("DROP TRIGGER fail_outcome")
        .unwrap();
    assert!(ledger
        .record_outgoing_outcome(&evidence(&i, NativeOutcome::Settled, 20))
        .is_ok());
}

#[test]
fn dispatch_update_failure_never_returns_a_permit_or_discards_reservation() {
    let (_dir, mut ledger) = setup();
    let i = intent(1);
    ledger.prepare_outgoing(&i, clock()).unwrap();
    ledger.connection.execute_batch("CREATE TRIGGER fail_dispatch BEFORE UPDATE ON outgoing_intents BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(ledger.begin_dispatch(i.operation, clock()).is_err());
    assert!(ledger.get_outgoing(i.operation).unwrap().unwrap().state == OutgoingState::Prepared);
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 1100);
    ledger
        .connection
        .execute_batch("DROP TRIGGER fail_dispatch")
        .unwrap();
    assert!(ledger.begin_dispatch(i.operation, clock()).is_ok());
}

#[test]
fn dispatch_crash_worker() {
    let Some(path) = std::env::var_os("ELTOR_OUTGOING_CRASH_TEST_FILE") else {
        return;
    };
    let mut ledger = PaymentLedger::reopen(Path::new(&path), account()).unwrap();
    ledger.begin_dispatch(intent(1).operation, clock()).unwrap();
    std::process::exit(0);
}

#[test]
fn process_loss_after_dispatch_latch_cannot_duplicate_payment() {
    let (dir, mut ledger) = setup();
    let i = intent(1);
    ledger.prepare_outgoing(&i, clock()).unwrap();
    drop(ledger);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "payment_ledger::outgoing::tests::dispatch_crash_worker",
        ])
        .env("ELTOR_OUTGOING_CRASH_TEST_FILE", dir.file())
        .status()
        .unwrap();
    assert!(status.success());
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert!(ledger.get_outgoing(i.operation).unwrap().unwrap().state == OutgoingState::InFlight);
    assert!(ledger.begin_dispatch(i.operation, clock()).is_err());
    ledger.mark_outgoing_unknown(i.operation).unwrap();
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 1100);
}

#[test]
fn recovery_inventory_is_bounded_and_cannot_spend() {
    let (_dir, mut ledger) = setup();
    for n in 1..=3 {
        ledger.prepare_outgoing(&intent(n), clock()).unwrap();
    }
    assert!(ledger.unresolved_outgoing(None, 0).is_err());
    assert!(ledger.unresolved_outgoing(None, 129).is_err());
    let first = ledger.unresolved_outgoing(None, 2).unwrap();
    assert_eq!(first.len(), 2);
    let next = ledger
        .unresolved_outgoing(Some(first[1].intent.operation), 2)
        .unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(ledger.account_exposure().unwrap().queued_intents, 3);
    assert_eq!(ledger.account_exposure().unwrap().wallet_rpcs, 0);
}

#[test]
fn separate_accepted_hop_charges_may_pay_the_same_authority() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    ledger.configure_account_budget(budget()).unwrap();
    ledger.configure_path_budget(&circuit()).unwrap();
    let first = intent(1);
    let mut second = intent(2);
    second.payee_reference = first.payee_reference;
    ledger
        .configure_hop_spending_terms(&hop_terms(1, circuit().local_budget_scope))
        .unwrap();
    let mut shared = hop_terms(2, circuit().local_budget_scope);
    shared.payee_reference = first.payee_reference;
    ledger.configure_hop_spending_terms(&shared).unwrap();
    ledger.prepare_outgoing(&first, clock()).unwrap();
    assert!(ledger.prepare_outgoing(&second, clock()).is_ok());
    let mut repeated = intent(3);
    repeated.charge_reference = first.charge_reference;
    assert!(matches!(
        ledger.prepare_outgoing(&repeated, clock()),
        Err(LedgerError::Conflict)
    ));
}

#[test]
fn one_origin_handle_accepts_three_independent_hop_bindings_policies_and_prices() {
    let dir = Directory::new();
    let mut ledger = dir.ledger();
    let mut account_limits = budget();
    account_limits.lifetime_limit_msat = 5000;
    account_limits.outstanding_limit_msat = 5000;
    ledger.configure_account_budget(account_limits).unwrap();
    let path = circuit();
    ledger.configure_path_budget(&path).unwrap();
    for n in 1..=3 {
        let mut terms = hop_terms(n, path.local_budget_scope);
        terms.round_amount_msat = n as u64 * 700;
        ledger.configure_hop_spending_terms(&terms).unwrap();
        ledger.configure_hop_spending_terms(&terms).unwrap();
        let mut i = intent(n);
        i.amount_msat = terms.round_amount_msat;
        let saved = ledger.prepare_outgoing(&i, clock()).unwrap();
        assert!(ledger.prepare_outgoing(&i, clock()).unwrap() == saved);
        assert!(saved.intent.handle == path.handle);
        assert!(saved.intent.hop_policy_hash == terms.hop_policy_hash);
        assert!(saved.intent.hop_circuit_binding == terms.hop_circuit_binding);
    }
    assert_eq!(
        ledger
            .path_exposure(path.local_budget_scope)
            .unwrap()
            .reserved_msat,
        4500
    );
    let mut another_path = path.clone();
    another_path.local_budget_scope = [90; 32];
    another_path.handle = [91; 16];
    ledger.configure_path_budget(&another_path).unwrap();
    configure_hops(&mut ledger, &another_path);
    let mut next = intent(1);
    next.operation = [4; 16];
    next.payment_hash = [190; 32];
    next.payment_id = [191; 32];
    next.local_budget_scope = another_path.local_budget_scope;
    next.handle = another_path.handle;
    assert!(matches!(
        ledger.prepare_outgoing(&next, clock()),
        Err(LedgerError::BudgetExceeded)
    ));
}

#[test]
fn outgoing_intent_must_match_registered_hop_c_q_payee_and_price() {
    let (_dir, mut ledger) = setup();
    let i = intent(1);
    for changed_field in 0..5 {
        let mut changed = i.clone();
        match changed_field {
            0 => changed.hop_circuit_binding = [90; 32],
            1 => changed.hop_policy_hash = [90; 32],
            2 => changed.payee_reference = [90; 32],
            3 => changed.amount_msat += 1,
            _ => changed.fee_limit_msat += 1,
        };
        assert!(matches!(
            ledger.prepare_outgoing(&changed, clock()),
            Err(LedgerError::Conflict)
        ));
    }
    let mut changed_terms = hop_terms(1, circuit().local_budget_scope);
    changed_terms.hop_policy_hash = [90; 32];
    assert!(matches!(
        ledger.configure_hop_spending_terms(&changed_terms),
        Err(LedgerError::Conflict)
    ));
    let saved = ledger.prepare_outgoing(&i, clock()).unwrap();
    assert!(ledger.prepare_outgoing(&i, clock()).unwrap() == saved);
}

#[test]
fn concurrent_dispatch_claims_return_one_permit() {
    let (dir, mut ledger) = setup();
    let i = intent(1);
    ledger.prepare_outgoing(&i, clock()).unwrap();
    drop(ledger);
    let barrier = Arc::new(Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let path = dir.file();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut ledger = PaymentLedger::reopen(&path, account()).unwrap();
                barrier.wait();
                ledger.begin_dispatch(intent(1).operation, clock()).is_ok()
            })
        })
        .collect();
    let mut permits = 0;
    for thread in threads {
        if thread.join().unwrap() {
            permits += 1;
        }
    }
    assert_eq!(permits, 1);
    let ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 1100);
    assert_eq!(ledger.account_exposure().unwrap().wallet_rpcs, 1);
}
