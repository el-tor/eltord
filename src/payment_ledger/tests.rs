use super::*;
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};

struct TestDirectory(PathBuf);
impl TestDirectory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("eltor-ledger-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("account.sqlite")
    }
    fn provision(&self) -> PaymentLedger {
        PaymentLedger::provision(&self.file(), account()).unwrap()
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn account() -> AccountId {
    AccountId([9; 32])
}
fn obligation() -> Obligation {
    Obligation {
        instance: [1; 16],
        handle: [2; 16],
        circuit_binding: [3; 32],
        round: 0,
        kind: ChargeKind::Handshake,
        payment_id: [4; 32],
        amount_msat: 1000,
        expected_payment_hash: Some([5; 32]),
        policy_hash: [6; 32],
        allocation_hash: [7; 32],
        window: Some(AcceptanceWindow {
            opens_at_ms: 100000,
            closes_at_ms: 200000,
        }),
    }
}
fn evidence() -> RecipientEvidence {
    RecipientEvidence {
        account: account(),
        payment_hash: [5; 32],
        invoice_digest: [8; 32],
        amount_msat: 1000,
        settled_at_ms: 150000,
        settlement_uncertainty_ms: 0,
        incoming: true,
        lightning: true,
        completed: true,
        authenticated_payment_id: None,
    }
}
fn clock() -> ClockObservation {
    ClockObservation {
        now_ms: 160000,
        uncertainty_ms: 0,
    }
}
fn batch() -> BatchRegistration {
    BatchRegistration {
        period: 0,
        request_digest: [10; 32],
        rounds: std::array::from_fn(|i| {
            let mut o = obligation();
            o.round = i as u64 + 1;
            o.kind = ChargeKind::Round;
            o.payment_id = [20 + i as u8; 32];
            o.expected_payment_hash = None;
            o.window = None;
            o
        }),
    }
}
fn count(ledger: &PaymentLedger, table: &str) -> i64 {
    ledger
        .connection
        .query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn provision_and_reopen_fail_closed() {
    let directory = TestDirectory::new();
    assert!(PaymentLedger::reopen(&directory.file(), account()).is_err());
    assert!(!directory.file().exists());
    let ledger = directory.provision();
    assert!(PaymentLedger::provision(&directory.file(), account()).is_err());
    drop(ledger);
    assert!(PaymentLedger::reopen(&directory.file(), AccountId([8; 32])).is_err());
    assert!(PaymentLedger::reopen(&directory.file(), account()).is_ok());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(directory.file())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    std::fs::write(directory.file(), b"corrupt ledger history").unwrap();
    assert!(PaymentLedger::reopen(&directory.file(), account()).is_err());
    assert!(PaymentLedger::provision(&directory.file(), account()).is_err());
    assert_eq!(
        std::fs::read(directory.file()).unwrap(),
        b"corrupt ledger history"
    );
    std::fs::write(directory.file(), b"").unwrap();
    assert!(PaymentLedger::reopen(&directory.file(), account()).is_err());
    assert_eq!(std::fs::metadata(directory.file()).unwrap().len(), 0);
}

#[test]
fn batch_reserves_every_id_atomically_and_refusal_is_durable() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let batch = batch();
    assert!(ledger.reserve_batch(&batch, [11; 16]).unwrap().outcome == AdmissionOutcome::Reserved);
    assert_eq!(count(&ledger, "payment_ids"), 10);
    assert!(ledger.reserve_batch(&batch, [11; 16]).unwrap().outcome == AdmissionOutcome::Reserved);
    assert_eq!(count(&ledger, "registrations"), 10);
    let mut second = batch.clone();
    for (i, o) in second.rounds.iter_mut().enumerate() {
        o.handle = [12; 16];
        o.circuit_binding = [13; 32];
        o.payment_id = [40 + i as u8; 32];
    }
    // The last ID conflicts; the first nine must remain unreserved.
    second.rounds[9].payment_id = batch.rounds[9].payment_id;
    assert!(ledger.reserve_batch(&second, [14; 16]).unwrap().outcome == AdmissionOutcome::Conflict);
    assert_eq!(count(&ledger, "payment_ids"), 10);
    assert_eq!(count(&ledger, "registrations"), 10);
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&directory.file(), account()).unwrap();
    assert!(ledger.reserve_batch(&second, [14; 16]).unwrap().outcome == AdmissionOutcome::Conflict);
    assert!(matches!(
        ledger.reserve_batch(&second, [15; 16]),
        Err(LedgerError::Conflict)
    ));
    second.request_digest = [16; 32];
    assert!(matches!(
        ledger.reserve_batch(&second, [14; 16]),
        Err(LedgerError::Conflict)
    ));
}

#[test]
fn batch_rejects_changed_order_duplicate_ids_rounds_and_overflow() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let mut b = batch();
    b.rounds[9].payment_id = b.rounds[0].payment_id;
    assert!(matches!(
        ledger.reserve_batch(&b, [11; 16]),
        Err(LedgerError::InvalidInput)
    ));
    b = batch();
    b.rounds[1].round = 1;
    assert!(ledger.reserve_batch(&b, [11; 16]).is_err());
    b = batch();
    b.period = u64::MAX;
    assert!(ledger.reserve_batch(&b, [11; 16]).is_err());
    b = batch();
    ledger.reserve_batch(&b, [11; 16]).unwrap();
    b.rounds.swap(0, 1);
    assert!(ledger.reserve_batch(&b, [11; 16]).is_err());
    b = batch();
    b.rounds[0].amount_msat += 1;
    assert!(matches!(
        ledger.reserve_batch(&b, [11; 16]),
        Err(LedgerError::Conflict)
    ));
}

#[test]
fn failed_batch_transaction_leaves_no_partial_admission() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    ledger.connection.execute_batch("CREATE TRIGGER fail_batch BEFORE INSERT ON payment_ids WHEN (SELECT COUNT(*) FROM payment_ids)=5 BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    let b = batch();
    assert!(ledger.reserve_batch(&b, [11; 16]).is_err());
    for table in ["batches", "registrations", "payment_ids"] {
        assert_eq!(count(&ledger, table), 0);
    }
    ledger
        .connection
        .execute_batch("DROP TRIGGER fail_batch")
        .unwrap();
    assert!(ledger.reserve_batch(&b, [11; 16]).unwrap().outcome == AdmissionOutcome::Reserved);
}

#[test]
fn competing_batches_cannot_reserve_one_identifier_twice() {
    let directory = TestDirectory::new();
    drop(directory.provision());
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|i| {
            let path = directory.file();
            let barrier = barrier.clone();
            let mut b = batch();
            for (n, o) in b.rounds.iter_mut().enumerate() {
                o.handle = [40 + i; 16];
                o.circuit_binding = [50 + i; 32];
                o.payment_id = [70 + i * 10 + n as u8; 32];
            }
            b.rounds[9].payment_id = [90; 32];
            std::thread::spawn(move || {
                let mut ledger = PaymentLedger::reopen(&path, account()).unwrap();
                barrier.wait();
                ledger.reserve_batch(&b, [60 + i; 16]).unwrap().outcome
            })
        })
        .collect();
    let mut reserved = 0;
    let mut conflict = 0;
    for thread in threads {
        match thread.join().unwrap() {
            AdmissionOutcome::Reserved => reserved += 1,
            AdmissionOutcome::Conflict => conflict += 1,
        }
    }
    assert_eq!((reserved, conflict), (1, 1));
    let ledger = PaymentLedger::reopen(&directory.file(), account()).unwrap();
    assert_eq!(count(&ledger, "payment_ids"), 10);
    assert_eq!(count(&ledger, "registrations"), 10);
    assert_eq!(count(&ledger, "batches"), 2);
}

#[test]
fn exact_settlement_retry_survives_reopen_and_cannot_rebind() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let o = obligation();
    let e = evidence();
    ledger.register_handshake(&o).unwrap();
    ledger.register_handshake(&o).unwrap();
    let initial = ledger.authorize(&o, &e, [11; 16], clock()).unwrap();
    assert_eq!(count(&ledger, "incoming_claims"), 1);
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&directory.file(), account()).unwrap();
    // An exact retry retrieves its durable decision even after the window.
    let replay = ledger
        .authorize(
            &o,
            &e,
            [11; 16],
            ClockObservation {
                now_ms: 300000,
                uncertainty_ms: 0,
            },
        )
        .unwrap();
    assert!(initial == replay);
    assert!(matches!(
        ledger.authorize(&o, &e, [12; 16], clock()),
        Err(LedgerError::Conflict)
    ));
    let mut changed = o.clone();
    changed.amount_msat += 1;
    assert!(matches!(
        ledger.register_handshake(&changed),
        Err(LedgerError::Conflict)
    ));
    let mut other = o.clone();
    other.handle = [13; 16];
    other.circuit_binding = [14; 32];
    ledger.register_handshake(&other).unwrap();
    assert!(matches!(
        ledger.authorize(&other, &e, [15; 16], clock()),
        Err(LedgerError::Conflict)
    ));
    assert_eq!(count(&ledger, "authorizations"), 1);
}

#[test]
fn recipient_evidence_must_match_account_amount_hash_and_native_settlement() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let o = obligation();
    ledger.register_handshake(&o).unwrap();
    for change in 0..8 {
        let mut e = evidence();
        match change {
            0 => e.account = AccountId([8; 32]),
            1 => e.payment_hash = [6; 32],
            2 => e.amount_msat += 1,
            3 => e.incoming = false,
            4 => e.lightning = false,
            5 => e.completed = false,
            6 => e.invoice_digest = [0; 32],
            _ => e.settled_at_ms = 0,
        }
        assert!(ledger.authorize(&o, &e, [11; 16], clock()).is_err());
    }
    assert_eq!(count(&ledger, "incoming_claims"), 0);
    assert_eq!(count(&ledger, "authorizations"), 0);
}

#[test]
fn exact_payment_id_mapping_is_required_when_invoice_hash_is_not_registered() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let batch = batch();
    ledger.reserve_batch(&batch, [11; 16]).unwrap();
    ledger
        .bind_initial_window(
            batch.rounds[0].instance,
            batch.rounds[0].handle,
            AcceptanceWindow {
                opens_at_ms: 100000,
                closes_at_ms: 200000,
            },
        )
        .unwrap();
    let o = &batch.rounds[0];
    let mut e = evidence();
    assert!(matches!(
        ledger.authorize(o, &e, [12; 16], clock()),
        Err(LedgerError::InvalidEvidence)
    ));
    e.authenticated_payment_id = Some([90; 32]);
    assert!(ledger.authorize(o, &e, [12; 16], clock()).is_err());
    e.authenticated_payment_id = Some(o.payment_id);
    assert!(ledger.authorize(o, &e, [12; 16], clock()).is_ok());
}

#[test]
fn settlement_and_clock_uncertainty_cannot_widen_windows() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let o = obligation();
    ledger.register_handshake(&o).unwrap();
    for timestamp in [99000, 201000] {
        let mut e = evidence();
        e.settled_at_ms = timestamp;
        assert!(matches!(
            ledger.authorize(&o, &e, [11; 16], clock()),
            Err(LedgerError::OutsideWindow)
        ));
    }
    let mut e = evidence();
    e.settled_at_ms = 100000;
    e.settlement_uncertainty_ms = 1;
    assert!(matches!(
        ledger.authorize(&o, &e, [11; 16], clock()),
        Err(LedgerError::OutsideWindow)
    ));
    assert!(ledger
        .authorize(
            &o,
            &evidence(),
            [11; 16],
            ClockObservation {
                now_ms: 200000,
                uncertainty_ms: 1
            }
        )
        .is_err());
    e = evidence();
    e.settled_at_ms = 170000;
    assert!(ledger.authorize(&o, &e, [11; 16], clock()).is_err());
    e = evidence();
    e.settled_at_ms = 100000;
    assert!(ledger.authorize(&o, &e, [11; 16], clock()).is_ok());
}

#[test]
fn zero_charge_has_durable_decision_without_fabricated_settlement() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    for i in 1..=2 {
        let mut o = obligation();
        o.amount_msat = 0;
        o.expected_payment_hash = None;
        o.handle = [i; 16];
        o.circuit_binding = [i; 32];
        ledger.register_handshake(&o).unwrap();
        assert!(ledger.authorize(&o, &evidence(), [i; 16], clock()).is_err());
        let auth = ledger.authorize_zero(&o, [i; 16], clock()).unwrap();
        assert!(auth.payment_hash == [0; 32]);
        assert!(ledger.authorize_zero(&o, [i; 16], clock()).unwrap() == auth);
    }
    assert_eq!(count(&ledger, "incoming_claims"), 0);
    assert_eq!(count(&ledger, "authorizations"), 2);
}

#[test]
fn authorization_insert_failure_rolls_back_incoming_claim() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let o = obligation();
    ledger.register_handshake(&o).unwrap();
    ledger.connection.execute_batch("CREATE TRIGGER fail_authorization BEFORE INSERT ON authorizations BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert!(ledger
        .authorize(&o, &evidence(), [11; 16], clock())
        .is_err());
    assert_eq!(count(&ledger, "incoming_claims"), 0);
    assert_eq!(count(&ledger, "authorizations"), 0);
    ledger
        .connection
        .execute_batch("DROP TRIGGER fail_authorization")
        .unwrap();
    assert!(ledger.authorize(&o, &evidence(), [11; 16], clock()).is_ok());
}

#[test]
fn independent_connections_cannot_claim_one_payment_twice() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let mut registrations = Vec::new();
    for i in 1..=8 {
        let mut o = obligation();
        o.handle = [i; 16];
        o.circuit_binding = [i; 32];
        ledger.register_handshake(&o).unwrap();
        registrations.push(o);
    }
    drop(ledger);
    let barrier = Arc::new(Barrier::new(registrations.len()));
    let threads: Vec<_> = registrations
        .into_iter()
        .enumerate()
        .map(|(i, o)| {
            let path = directory.file();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut ledger = PaymentLedger::reopen(&path, account()).unwrap();
                barrier.wait();
                ledger
                    .authorize(&o, &evidence(), [i as u8 + 20; 16], clock())
                    .is_ok()
            })
        })
        .collect();
    let wins = threads
        .into_iter()
        .filter_map(|t| if t.join().unwrap() { Some(()) } else { None })
        .count();
    assert_eq!(wins, 1);
    let ledger = PaymentLedger::reopen(&directory.file(), account()).unwrap();
    assert_eq!(count(&ledger, "incoming_claims"), 1);
    assert_eq!(count(&ledger, "authorizations"), 1);
}

#[test]
fn instance_closure_preserves_claim_and_unknown_activation() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let o = obligation();
    ledger.register_handshake(&o).unwrap();
    ledger
        .authorize(&o, &evidence(), [11; 16], clock())
        .unwrap();
    ledger.close_instance(o.instance).unwrap();
    let auth = ledger
        .authorize(&o, &evidence(), [11; 16], clock())
        .unwrap();
    assert!(auth.state == AuthorizationState::ClosedUnresolved);
    assert!(matches!(
        ledger.mark_consumed(&o, [11; 16]),
        Err(LedgerError::Closed)
    ));
    assert_eq!(count(&ledger, "incoming_claims"), 1);
    let mut fresh = o.clone();
    fresh.instance = [12; 16];
    fresh.handle = [13; 16];
    fresh.circuit_binding = [14; 32];
    ledger.register_handshake(&fresh).unwrap();
    assert!(matches!(
        ledger.authorize(&fresh, &evidence(), [15; 16], clock()),
        Err(LedgerError::Conflict)
    ));
}

#[test]
fn authenticated_consumption_is_idempotent_and_never_reassigns_claim() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let o = obligation();
    ledger.register_handshake(&o).unwrap();
    ledger
        .authorize(&o, &evidence(), [11; 16], clock())
        .unwrap();
    assert!(matches!(
        ledger.mark_consumed(&o, [12; 16]),
        Err(LedgerError::Conflict)
    ));
    ledger.mark_consumed(&o, [11; 16]).unwrap();
    ledger.mark_consumed(&o, [11; 16]).unwrap();
    ledger.close_instance(o.instance).unwrap();
    assert!(
        ledger
            .authorize(&o, &evidence(), [11; 16], clock())
            .unwrap()
            .state
            == AuthorizationState::Consumed
    );
}

#[test]
fn preactivation_reservation_does_not_invent_or_reset_round_windows() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let batch = batch();
    ledger.reserve_batch(&batch, [11; 16]).unwrap();
    let first = &batch.rounds[0];
    let mut e = evidence();
    e.authenticated_payment_id = Some(first.payment_id);
    assert!(matches!(
        ledger.authorize(first, &e, [12; 16], clock()),
        Err(LedgerError::NotRegistered)
    ));
    let initial = AcceptanceWindow {
        opens_at_ms: 100000,
        closes_at_ms: 200000,
    };
    ledger
        .bind_initial_window(first.instance, first.handle, initial)
        .unwrap();
    ledger
        .bind_initial_window(first.instance, first.handle, initial)
        .unwrap();
    assert!(matches!(
        ledger.bind_initial_window(
            first.instance,
            first.handle,
            AcceptanceWindow {
                opens_at_ms: 100000,
                closes_at_ms: 201000
            }
        ),
        Err(LedgerError::Conflict)
    ));
    let anchor = ActivationAnchor {
        started_at_ms: 160000,
        round_ms: 60_000,
        early_ms: 60_000,
        grace_ms: 15_000,
    };
    assert!(matches!(
        ledger.bind_activation(first.instance, first.handle, anchor),
        Err(LedgerError::NotRegistered)
    ));
    ledger.authorize(first, &e, [12; 16], clock()).unwrap();
    let second = &batch.rounds[1];
    e.payment_hash = [55; 32];
    e.authenticated_payment_id = Some(second.payment_id);
    e.settled_at_ms = 165000;
    let later = ClockObservation {
        now_ms: 170000,
        uncertainty_ms: 0,
    };
    assert!(matches!(
        ledger.authorize(second, &e, [13; 16], later),
        Err(LedgerError::NotRegistered)
    ));
    ledger
        .bind_activation(first.instance, first.handle, anchor)
        .unwrap();
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&directory.file(), account()).unwrap();
    ledger
        .bind_activation(first.instance, first.handle, anchor)
        .unwrap();
    let mut changed = anchor;
    changed.started_at_ms += 1;
    assert!(matches!(
        ledger.bind_activation(first.instance, first.handle, changed),
        Err(LedgerError::Conflict)
    ));
    e.settled_at_ms = 236000;
    assert!(matches!(
        ledger.authorize(
            second,
            &e,
            [13; 16],
            ClockObservation {
                now_ms: 236000,
                uncertainty_ms: 0
            }
        ),
        Err(LedgerError::OutsideWindow)
    ));
    e.settled_at_ms = 165000;
    assert!(ledger.authorize(second, &e, [13; 16], later).is_ok());
    // Round eleven still derives from the original T0, not its admission.
    let window = anchor.window(11).unwrap();
    assert_eq!(window.opens_at_ms, 700000);
    assert_eq!(window.closes_at_ms, 775000);
}

#[test]
fn activation_record_cannot_bind_a_closed_instance() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let batch = batch();
    ledger.reserve_batch(&batch, [11; 16]).unwrap();
    let first = &batch.rounds[0];
    ledger.close_instance(first.instance).unwrap();
    let mut new_handle = obligation();
    new_handle.handle = [91; 16];
    new_handle.circuit_binding = [92; 32];
    assert!(matches!(
        ledger.register_handshake(&new_handle),
        Err(LedgerError::Closed)
    ));
    assert!(matches!(
        ledger.bind_initial_window(
            first.instance,
            first.handle,
            AcceptanceWindow {
                opens_at_ms: 100000,
                closes_at_ms: 200000
            }
        ),
        Err(LedgerError::Closed)
    ));
    assert!(matches!(
        ledger.bind_activation(
            first.instance,
            first.handle,
            ActivationAnchor {
                started_at_ms: 160000,
                round_ms: 60_000,
                early_ms: 60_000,
                grace_ms: 15_000
            }
        ),
        Err(LedgerError::Closed)
    ));
}

#[test]
fn recovery_reads_cannot_create_reservations_or_authorizations() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let o = obligation();
    assert!(ledger
        .get_batch_reservation(o.instance, o.handle, 0)
        .unwrap()
        .is_none());
    assert!(matches!(
        ledger.get_authorization(&o),
        Err(LedgerError::NotRegistered)
    ));
    assert_eq!(count(&ledger, "payment_ids"), 0);
    assert_eq!(count(&ledger, "authorizations"), 0);
    let b = batch();
    let saved = ledger.reserve_batch(&b, [11; 16]).unwrap();
    assert!(
        ledger
            .get_batch_reservation(saved.instance, saved.handle, saved.period)
            .unwrap()
            .unwrap()
            == saved
    );
    ledger.register_handshake(&o).unwrap();
    assert!(ledger.get_authorization(&o).unwrap().is_none());
    let saved = ledger
        .authorize(&o, &evidence(), [12; 16], clock())
        .unwrap();
    assert!(ledger.get_authorization(&o).unwrap().unwrap() == saved);
    assert_eq!(count(&ledger, "authorizations"), 1);
}

#[test]
fn numeric_overflow_is_rejected_without_truncation() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let mut o = obligation();
    o.amount_msat = u64::MAX;
    assert!(matches!(
        ledger.register_handshake(&o),
        Err(LedgerError::InvalidInput)
    ));
    o = obligation();
    o.round = u64::MAX;
    o.kind = ChargeKind::Round;
    o.window = None;
    assert!(matches!(o.validate(), Err(LedgerError::InvalidInput)));
    let anchor = ActivationAnchor {
        started_at_ms: i64::MAX - 1,
        round_ms: 60_000,
        early_ms: 60_000,
        grace_ms: 15_000,
    };
    assert!(anchor.window(2).is_err());
    assert!(certain_time(
        160000,
        u64::MAX,
        AcceptanceWindow {
            opens_at_ms: 100000,
            closes_at_ms: 200000
        }
    )
    .is_err());
    assert_eq!(count(&ledger, "registrations"), 0);
}

#[test]
fn authorization_digest_has_independent_binary_known_answer() {
    // Independently computed from the 161-byte concatenation specified in
    // recovery.md, using Python hashlib and explicit big-endian integers.
    assert_eq!(
        hex::encode(authorization_digest(&obligation(), [11; 16], [5; 32])),
        "1fac1d42ac976d7c48b578676c415f7c2ae2b14912ea437e022aeebf9b406ef1"
    );
}

#[test]
fn crash_before_commit_worker() {
    let Some(path) = std::env::var_os("ELTOR_LEDGER_CRASH_TEST_FILE") else {
        return;
    };
    let ledger = PaymentLedger::reopen(Path::new(&path), account()).unwrap();
    ledger
        .connection
        .execute_batch("PRAGMA cache_size=1; BEGIN IMMEDIATE;")
        .unwrap();
    ledger.connection.execute("INSERT INTO incoming_claims(account,payment_hash,registration_id,amount_msat,settled_at_ms,evidence) VALUES(?1,?2,1,1000,150000,x'5b5d')",params![&account().0[..],&[5u8;32][..]]).unwrap();
    // Skip Connection/Transaction destructors to model process loss while
    // the durable transaction is incomplete, not graceful Rust rollback.
    std::process::exit(0);
}

#[test]
fn process_loss_before_commit_leaves_no_partial_claim() {
    let directory = TestDirectory::new();
    let mut ledger = directory.provision();
    let o = obligation();
    ledger.register_handshake(&o).unwrap();
    drop(ledger);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "payment_ledger::tests::crash_before_commit_worker",
        ])
        .env("ELTOR_LEDGER_CRASH_TEST_FILE", directory.file())
        .status()
        .unwrap();
    assert!(status.success());
    let mut ledger = PaymentLedger::reopen(&directory.file(), account()).unwrap();
    assert_eq!(count(&ledger, "incoming_claims"), 0);
    assert_eq!(count(&ledger, "authorizations"), 0);
    assert!(ledger.authorize(&o, &evidence(), [11; 16], clock()).is_ok());
}
