use super::*;
struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let p =
            std::env::temp_dir().join(format!("eltor-handshake-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self) -> std::path::PathBuf {
        self.0.join("ledger.sqlite")
    }
    fn ledger(&self) -> PaymentLedger {
        let mut l = PaymentLedger::provision(&self.file(), account()).unwrap();
        l.configure_account_budget(AccountBudget {
            lifetime_limit_msat: 10000,
            outstanding_limit_msat: 10000,
            max_queued_intents: 16,
            max_wallet_rpcs: 4,
            max_unresolved_intents: 32,
        })
        .unwrap();
        l
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn account() -> AccountId {
    AccountId([1; 32])
}
fn clock() -> ClockObservation {
    ClockObservation {
        now_ms: 150000,
        uncertainty_ms: 0,
    }
}
fn scope() -> HandshakeScope {
    let mut terms = vec![0; 55];
    terms[..3].copy_from_slice(&[1, 1, 10]);
    terms[3..11].copy_from_slice(&1000u64.to_be_bytes());
    terms[11..15].copy_from_slice(&60000u32.to_be_bytes());
    terms[15..19].copy_from_slice(&60000u32.to_be_bytes());
    terms[31..39].copy_from_slice(&1800000u64.to_be_bytes());
    terms[39..43].copy_from_slice(&3u32.to_be_bytes());
    terms[43..51].copy_from_slice(&2000u64.to_be_bytes());
    terms[51..55].copy_from_slice(&30000u32.to_be_bytes());
    HandshakeScope {
        scope: [2; 32],
        instance: [3; 16],
        account: account(),
        lifetime_limit_msat: 10000,
        outstanding_limit_msat: 10000,
        per_round_limit_msat: 5000,
        window: AcceptanceWindow {
            opens_at_ms: 100000,
            closes_at_ms: 200000,
        },
        hops: vec![HandshakeHop {
            fingerprint: [4; 20],
            terms,
            offer: Some("lno1fixture".into()),
            operation: [5; 16],
            payment_id: [6; 32],
            initial_payment_ids: std::array::from_fn(|i| [20 + i as u8; 32]),
            fee_limit_msat: 100,
        }],
    }
}
fn invoice() -> HandshakeInvoice {
    let artifact = "lni1fixture".to_owned();
    HandshakeInvoice {
        scope: scope().scope,
        hop: 1,
        invoice_digest: hash(artifact.as_bytes()),
        payment_hash: [7; 32],
        artifact,
    }
}
fn evidence(outcome: NativeOutcome) -> HandshakeEvidence {
    HandshakeEvidence {
        financial: OutgoingEvidence {
            account: account(),
            operation: [5; 16],
            invoice_digest: invoice().invoice_digest,
            payment_hash: [7; 32],
            amount_msat: 2000,
            fee_msat: if outcome == NativeOutcome::Settled {
                50
            } else {
                0
            },
            outcome,
        },
        settled_earliest_ms: if outcome == NativeOutcome::Settled {
            Some(150000)
        } else {
            None
        },
        settled_latest_ms: if outcome == NativeOutcome::Settled {
            Some(150999)
        } else {
            None
        },
    }
}
fn registration() -> OriginRegistration {
    let s = scope();
    let ids = std::array::from_fn(|i| [20 + i as u8; 32]);
    let binding = crate::rpc::OriginBinding {
        instance: s.instance,
        handle: [8; 16],
        hops: 1,
        revision: 1,
    };
    let c = [9; 32];
    fn digest(d: &[u8], parts: &[&[u8]]) -> Hash32 {
        let mut h = Sha256::new();
        h.update((d.len() as u16).to_be_bytes());
        h.update(d);
        for p in parts {
            h.update(p);
        }
        h.finalize().into()
    }
    let q = digest(b"eltor/payment-renew/v1/terms", &[&s.hops[0].terms]);
    let all: Vec<u8> = ids.iter().flatten().copied().collect();
    let a = digest(b"eltor/payment-renew/v1/initial", &[&c, &q, &all]);
    let mut wire = format!(
        "250-ELTOR_INITIAL_REGISTRATION 1 {} {} 1 1\n250-POLICY {} {} {}\n250-BINDING {}\n",
        hex::encode(binding.instance),
        hex::encode(binding.handle),
        hex::encode(&s.hops[0].terms),
        hex::encode(q),
        hex::encode(a),
        hex::encode(c)
    );
    for (i, id) in ids.iter().enumerate() {
        wire.push_str(&format!("250-ROUND {} {}\n", i + 1, hex::encode(id)));
    }
    wire.push_str("250 OK");
    OriginRegistration::parse(&wire, &binding, 1, &ids).unwrap()
}
fn path() -> PathBudget {
    let s = scope();
    PathBudget {
        instance: s.instance,
        handle: [8; 16],
        local_budget_scope: s.scope,
        accepted_terms_digest: [10; 32],
        lifetime_limit_msat: s.lifetime_limit_msat,
        outstanding_limit_msat: s.outstanding_limit_msat,
        per_round_limit_msat: s.per_round_limit_msat,
        funding: FundingSource::OwnWallet,
    }
}
fn hops() -> Vec<HandshakePathHop> {
    vec![HandshakePathHop {
        registration: registration(),
        charge_reference: [11; 32],
        payee_reference: payee_reference(&scope().hops[0]),
        fee_limit_msat: 100,
    }]
}
#[test]
fn uncertainty_is_retained_across_restart_and_instance_closure_without_retry() {
    let d = Directory::new();
    let mut l = d.ledger();
    l.save_handshake_scope(&scope()).unwrap();
    l.save_handshake_scope(&scope()).unwrap();
    l.prepare_handshake_invoice(&invoice(), clock()).unwrap();
    assert_eq!(l.account_exposure().unwrap().reserved_msat, 2100);
    l.begin_handshake_dispatch(scope().scope, 1, clock())
        .unwrap();
    drop(l);
    let mut l = PaymentLedger::reopen(&d.file(), account()).unwrap();
    assert!(matches!(
        l.begin_handshake_dispatch(scope().scope, 1, clock()),
        Err(LedgerError::DispatchAlreadyClaimed)
    ));
    l.mark_handshake_unknown(scope().scope, 1).unwrap();
    l.close_other_instances([12; 16]).unwrap();
    assert!(l.instance_is_closed(scope().instance).unwrap());
    assert!(
        l.get_handshake(scope().scope, 1).unwrap().unwrap().state
            == OutgoingState::ClosedUnresolved
    );
    assert_eq!(l.account_exposure().unwrap().reserved_msat, 2100);
    let r = l
        .record_handshake_outcome(scope().scope, 1, &evidence(NativeOutcome::Settled))
        .unwrap();
    assert!(r.closed);
    assert_eq!(l.account_exposure().unwrap().spent_msat, 2050);
    assert_eq!(l.account_exposure().unwrap().outstanding_msat, 2050);
    drop(l);
    PaymentLedger::reopen(&d.file(), account()).unwrap();
}
#[test]
fn handoff_preserves_caps_and_cost_and_rejects_changed_hops_or_second_fee() {
    let d = Directory::new();
    let mut l = d.ledger();
    let s = scope();
    l.save_handshake_scope(&s).unwrap();
    l.prepare_handshake_invoice(&invoice(), clock()).unwrap();
    l.begin_handshake_dispatch(s.scope, 1, clock()).unwrap();
    l.record_handshake_outcome(s.scope, 1, &evidence(NativeOutcome::Settled))
        .unwrap();
    assert!(l.configure_path_budget(&path()).is_err());
    l.save_origin_construction(s.scope, s.instance, b"sanitized constructor")
        .unwrap();
    l.claim_origin_construction(s.scope).unwrap();
    l.save_origin_plan(s.instance, path().handle, s.scope, b"accepted plan")
        .unwrap();
    let mut wrong = path();
    wrong.lifetime_limit_msat += 1;
    assert!(l.bind_handshake_path(&wrong, &hops()).is_err());
    let mut wronghop = hops();
    wronghop[0].payee_reference = [55; 32];
    assert!(l.bind_handshake_path(&path(), &wronghop).is_err());
    l.bind_handshake_path(&path(), &hops()).unwrap();
    l.bind_handshake_path(&path(), &hops()).unwrap();
    l.configure_path_budget(&path()).unwrap();
    assert_eq!(l.path_exposure(s.scope).unwrap().spent_msat, 2050);
    assert_eq!(l.account_exposure().unwrap().spent_msat, 2050);
    l.mark_handshake_consumed(s.scope, 1).unwrap();
    assert_eq!(l.account_exposure().unwrap().outstanding_msat, 0);
    assert_eq!(l.account_exposure().unwrap().spent_msat, 2050);
    assert!(matches!(
        l.begin_handshake_dispatch(s.scope, 1, clock()),
        Err(LedgerError::DispatchAlreadyClaimed)
    ));
    drop(l);
    PaymentLedger::reopen(&d.file(), account()).unwrap();
}
#[test]
fn failed_final_fees_and_fee_breach_never_disappear_or_reissue() {
    let d = Directory::new();
    let mut l = d.ledger();
    let s = scope();
    l.save_handshake_scope(&s).unwrap();
    l.prepare_handshake_invoice(&invoice(), clock()).unwrap();
    l.begin_handshake_dispatch(s.scope, 1, clock()).unwrap();
    let mut e = evidence(NativeOutcome::FailedFinal);
    e.financial.fee_msat = 150;
    l.record_handshake_outcome(s.scope, 1, &e).unwrap();
    assert!(l.outgoing_admission_blocked().unwrap());
    assert_eq!(l.account_exposure().unwrap().spent_msat, 150);
    assert_eq!(l.account_exposure().unwrap().reserved_msat, 0);
    assert!(l.begin_handshake_dispatch(s.scope, 1, clock()).is_err());
    drop(l);
    PaymentLedger::reopen(&d.file(), account()).unwrap();
}
#[test]
fn migration_v6_is_explicit_preserves_unknown_and_rejects_corrupt_artifact() {
    let d = Directory::new();
    let mut l = d.ledger();
    let s = scope();
    l.save_origin_construction(s.scope, s.instance, b"one construction")
        .unwrap();
    l.claim_origin_construction(s.scope).unwrap();
    let round = priced_round(&mut l, 2000);
    l.prepare_outgoing(&round, clock()).unwrap();
    l.begin_dispatch(round.operation, clock()).unwrap();
    l.mark_outgoing_unknown(round.operation).unwrap();
    l.connection.execute_batch("DROP TABLE handshake_intents;DROP TABLE handshake_initial_ids;DROP TABLE handshake_hops;DROP TABLE handshake_scopes;PRAGMA user_version=6;").unwrap();
    drop(l);
    assert!(matches!(
        PaymentLedger::reopen(&d.file(), account()),
        Err(LedgerError::InvalidLedger)
    ));
    let mut l = PaymentLedger::upgrade_from_v1(&d.file(), account()).unwrap();
    assert!(
        l.get_origin_construction(s.scope)
            .unwrap()
            .unwrap()
            .dispatch_claimed
    );
    let old = l.get_outgoing(round.operation).unwrap().unwrap();
    assert!(old.state == OutgoingState::Unknown);
    assert_eq!(old.reserved_msat, 2100);
    l.save_handshake_scope(&s).unwrap();
    l.prepare_handshake_invoice(&invoice(), clock()).unwrap();
    l.connection
        .execute(
            "UPDATE handshake_intents SET payload=?1",
            params![b"{}".as_slice()],
        )
        .unwrap();
    drop(l);
    assert!(PaymentLedger::reopen(&d.file(), account()).is_err());
}
#[test]
fn duplicate_identity_and_shared_account_exposure_are_atomic() {
    let d = Directory::new();
    let mut l = d.ledger();
    let s = scope();
    l.save_handshake_scope(&s).unwrap();
    l.prepare_handshake_invoice(&invoice(), clock()).unwrap();
    let mut duplicate = s.clone();
    duplicate.scope = [13; 32];
    assert!(l.save_handshake_scope(&duplicate).is_err());
    let mut distinct = s.clone();
    distinct.scope = [14; 32];
    distinct.hops[0].operation = [15; 16];
    distinct.hops[0].payment_id = [16; 32];
    distinct.hops[0].terms[43..51].copy_from_slice(&9000u64.to_be_bytes());
    assert!(matches!(
        l.save_handshake_scope(&distinct),
        Err(LedgerError::BudgetExceeded)
    ));
    assert_eq!(l.unresolved_handshakes(None, 16).unwrap().len(), 1);
    assert_eq!(l.account_exposure().unwrap().reserved_msat, 2100);
}

fn priced_round(l: &mut PaymentLedger, amount: u64) -> OutgoingIntent {
    let b = PathBudget {
        instance: [3; 16],
        handle: [90; 16],
        local_budget_scope: [91; 32],
        accepted_terms_digest: [92; 32],
        lifetime_limit_msat: 10000,
        outstanding_limit_msat: 10000,
        per_round_limit_msat: 10000,
        funding: FundingSource::OwnWallet,
    };
    l.configure_path_budget(&b).unwrap();
    l.configure_hop_spending_terms(&HopSpendingTerms {
        local_budget_scope: b.local_budget_scope,
        charge_reference: [93; 32],
        hop_circuit_binding: [94; 32],
        hop_policy_hash: [95; 32],
        payee_reference: [96; 32],
        handshake_amount_msat: 0,
        round_amount_msat: amount,
        fee_limit_msat: 100,
    })
    .unwrap();
    OutgoingIntent {
        operation: [97; 16],
        account: account(),
        instance: b.instance,
        handle: b.handle,
        local_budget_scope: b.local_budget_scope,
        round: 1,
        kind: ChargeKind::Round,
        payment_id: [98; 32],
        charge_reference: [93; 32],
        payee_reference: [96; 32],
        accepted_terms_digest: b.accepted_terms_digest,
        hop_circuit_binding: [94; 32],
        hop_policy_hash: [95; 32],
        invoice_digest: [99; 32],
        payment_hash: [100; 32],
        amount_msat: amount,
        fee_limit_msat: 100,
        window: scope().window,
    }
}
#[test]
fn setup_and_rounds_share_account_caps_and_concurrent_fee_dispatch_has_one_winner() {
    let d = Directory::new();
    let mut l = d.ledger();
    l.save_handshake_scope(&scope()).unwrap();
    l.prepare_handshake_invoice(&invoice(), clock()).unwrap();
    let round = priced_round(&mut l, 9000);
    assert!(matches!(
        l.prepare_outgoing(&round, clock()),
        Err(LedgerError::BudgetExceeded)
    ));
    drop(l);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let p = d.file();
        let gate = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let mut l = PaymentLedger::reopen(&p, account()).unwrap();
            gate.wait();
            l.begin_handshake_dispatch(scope().scope, 1, clock())
                .is_ok()
        }));
    }
    assert_eq!(
        workers
            .into_iter()
            .filter_map(|w| w.join().ok())
            .filter(|won| *won)
            .count(),
        1
    );
    let l = PaymentLedger::reopen(&d.file(), account()).unwrap();
    assert_eq!(l.account_exposure().unwrap().reserved_msat, 2100);
    assert!(l.get_handshake(scope().scope, 1).unwrap().unwrap().state == OutgoingState::InFlight);
}
