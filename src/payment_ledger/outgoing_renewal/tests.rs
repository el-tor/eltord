use super::*;
use crate::payment_ledger::outgoing::*;
use crate::rpc::RenewalOriginSnapshot;
use std::path::PathBuf;
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "eltor-outgoing-renewal-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("account.sqlite")
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
fn scope() -> Hash32 {
    [4; 32]
}
fn charge(hop: u8) -> Hash32 {
    [80 + hop; 32]
}
fn binding() -> OriginBinding {
    OriginBinding {
        instance: [2; 16],
        handle: [3; 16],
        hops: 3,
        revision: 1,
    }
}
fn registration(hop: u8) -> OriginRegistration {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../spec/vectors/payment_renewal_v1.json"
    ))
    .unwrap();
    let mut policy: [u8; 55] = hex::decode(value["fixtures"][0]["terms_hex"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    policy[3..11].copy_from_slice(&(u64::from(hop) * 1000).to_be_bytes());
    let c = [70 + hop; 32];
    let q = hash(b"eltor/payment-renew/v1/terms", &[&policy]);
    let ids: [[u8; 32]; 10] = std::array::from_fn(|n| [20 + hop * 10 + n as u8; 32]);
    let flat: Vec<u8> = ids.iter().flatten().copied().collect();
    let a0 = hash(b"eltor/payment-renew/v1/initial", &[&c, &q, &flat]);
    let b = binding();
    let mut lines = vec![
        format!(
            "250-ELTOR_INITIAL_REGISTRATION 1 {} {} {hop} 1",
            hex::encode(b.instance),
            hex::encode(b.handle)
        ),
        format!(
            "250-POLICY {} {} {}",
            hex::encode(policy),
            hex::encode(q),
            hex::encode(a0)
        ),
        format!("250-BINDING {}", hex::encode(c)),
    ];
    for (n, id) in ids.iter().enumerate() {
        lines.push(format!("250-ROUND {} {}", n + 1, hex::encode(id)));
    }
    lines.push("250 OK".into());
    OriginRegistration::parse(&lines.join("\n"), &b, u16::from(hop), &ids).unwrap()
}
fn operation(hop: u8) -> RenewalOperation {
    let r = registration(hop);
    RenewalOperation {
        binding: binding(),
        hop: u16::from(hop),
        operation: [hop + 1; 16],
        period: 1,
        first_round: 11,
        nonce: [hop + 10; 16],
        terms: r.policy_hash(),
        parent: r.initial_allocation(),
        ids: std::array::from_fn(|n| [120 + hop * 10 + n as u8; 32]),
    }
}
fn proof(op: &RenewalOperation) -> VerifiedRenewalAdmission {
    let r = registration(op.hop as u8);
    let a = op.expected_allocation(r.circuit_binding()).unwrap();
    let wire=format!("250-ELTOR_PAYMENT 1 {} {} {} 2\n250-PROFILE {} 0 {} 10\n250-SCHEDULE 30000 60000 1200000\n250-OP {} ADMITTED 1 {} {} 0 1\n250 OK",hex::encode(op.binding.instance),hex::encode(op.binding.handle),op.hop,hex::encode(op.terms),hex::encode(r.initial_allocation()),hex::encode(op.operation),hex::encode([170+op.hop as u8;32]),hex::encode(a));
    RenewalOriginSnapshot::parse(&wire, &op.binding, op.hop, Some(op.operation))
        .unwrap()
        .verify_admission(op, &r)
        .unwrap()
}
fn setup() -> (Directory, PaymentLedger) {
    let dir = Directory::new();
    let mut ledger = PaymentLedger::provision(&dir.file(), account()).unwrap();
    ledger
        .save_origin_construction(scope(), [2; 16], b"exact three-hop constructor")
        .unwrap();
    ledger.claim_origin_construction(scope()).unwrap();
    ledger
        .save_origin_plan(
            [2; 16],
            [3; 16],
            scope(),
            b"accepted three independent hop prices",
        )
        .unwrap();
    ledger
        .configure_account_budget(AccountBudget {
            lifetime_limit_msat: 1_000_000,
            outstanding_limit_msat: 100_000,
            max_queued_intents: 128,
            max_wallet_rpcs: 32,
            max_unresolved_intents: 160,
        })
        .unwrap();
    ledger
        .configure_path_budget(&PathBudget {
            instance: [2; 16],
            handle: [3; 16],
            local_budget_scope: scope(),
            accepted_terms_digest: [8; 32],
            lifetime_limit_msat: 100_000,
            outstanding_limit_msat: 100_000,
            per_round_limit_msat: 10_000,
            funding: FundingSource::OwnWallet,
        })
        .unwrap();
    for hop in 1..=3 {
        let r = registration(hop);
        ledger
            .configure_hop_spending_terms(&HopSpendingTerms {
                local_budget_scope: scope(),
                charge_reference: charge(hop),
                hop_circuit_binding: r.circuit_binding(),
                hop_policy_hash: r.policy_hash(),
                payee_reference: [90 + hop; 32],
                handshake_amount_msat: 0,
                round_amount_msat: u64::from(hop) * 1000,
                fee_limit_msat: 10,
            })
            .unwrap();
    }
    for hop in 1..=3 {
        ledger
            .register_origin_initial_allocation(scope(), charge(hop), &registration(hop))
            .unwrap();
    }
    (dir, ledger)
}
fn admitted(ledger: &mut PaymentLedger, hop: u8) {
    let op = operation(hop);
    ledger.save_origin_renewal_operation(&op).unwrap();
    ledger
        .claim_origin_renewal_dispatch(op.binding.instance, op.binding.handle, op.hop, op.operation)
        .unwrap();
    ledger
        .admit_origin_renewal(scope(), charge(hop), &proof(&op))
        .unwrap();
}
fn intent(hop: u8, round: u64) -> (OutgoingIntent, String) {
    let r = registration(hop);
    let artifact = format!("lnbc1synthetic-hop-{hop}-round-{round}");
    let id = if round <= 10 {
        r.payment_ids()[(round - 1) as usize]
    } else {
        operation(hop).ids[((round - 1) % 10) as usize]
    };
    (
        OutgoingIntent {
            operation: [200 + hop; 16],
            account: account(),
            instance: [2; 16],
            handle: [3; 16],
            local_budget_scope: scope(),
            round,
            kind: ChargeKind::Round,
            payment_id: id,
            charge_reference: charge(hop),
            payee_reference: [90 + hop; 32],
            accepted_terms_digest: [8; 32],
            hop_circuit_binding: r.circuit_binding(),
            hop_policy_hash: r.policy_hash(),
            invoice_digest: Sha256::digest(artifact.as_bytes()).into(),
            payment_hash: [180 + hop; 32],
            amount_msat: u64::from(hop) * 1000,
            fee_limit_msat: 10,
            window: AcceptanceWindow {
                opens_at_ms: 100_000,
                closes_at_ms: 200_000,
            },
        },
        artifact,
    )
}
fn clock() -> ClockObservation {
    ClockObservation {
        now_ms: 150_000,
        uncertainty_ms: 0,
    }
}
#[test]
fn all_three_distinct_hops_must_be_admitted_before_round_eleven_and_share_original_caps() {
    let (dir, mut ledger) = setup();
    assert!(ledger.origin_period_admitted(scope(), 0).unwrap());
    for round in 2..=10 {
        ledger.advance_spending_round(scope(), round).unwrap();
    }
    assert!(!ledger.origin_period_admitted(scope(), 1).unwrap());
    let (i, artifact) = intent(1, 11);
    ledger.save_origin_invoice(&i, &artifact).unwrap();
    assert!(matches!(
        ledger.prepare_outgoing(&i, clock()),
        Err(LedgerError::NotRegistered)
    ));
    admitted(&mut ledger, 1);
    admitted(&mut ledger, 2);
    assert!(!ledger.origin_period_admitted(scope(), 1).unwrap());
    assert!(ledger.advance_spending_round(scope(), 11).is_err());
    admitted(&mut ledger, 3);
    assert!(ledger.origin_period_admitted(scope(), 1).unwrap());
    let saved = ledger
        .get_origin_allocation(scope(), charge(1), 1)
        .unwrap()
        .unwrap();
    assert!(
        saved.payment_ids == operation(1).ids
            && saved.allocation_digest == proof(&operation(1)).allocation_digest()
    );
    ledger.advance_spending_round(scope(), 11).unwrap();
    for hop in 1..=3 {
        let (i, artifact) = intent(hop, 11);
        ledger.save_origin_invoice(&i, &artifact).unwrap();
        let saved = ledger.prepare_outgoing(&i, clock()).unwrap();
        assert_eq!(saved.reserved_msat, u64::from(hop) * 1000 + 10);
    }
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 6030);
    assert!(ledger.advance_spending_round(scope(), 12).is_err());
    drop(ledger);
    let ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert_eq!(ledger.spending_round(scope()).unwrap(), 11);
    assert!(ledger.origin_period_admitted(scope(), 1).unwrap());
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 6030);
}
#[test]
fn queued_or_unclaimed_operation_is_not_admission_and_retries_never_dispatch_twice() {
    let (dir, mut first) = setup();
    let op = operation(1);
    first.save_origin_renewal_operation(&op).unwrap();
    assert!(first
        .admit_origin_renewal(scope(), charge(1), &proof(&op))
        .is_err());
    assert!(!first.origin_period_admitted(scope(), 1).unwrap());
    let mut second = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    first
        .claim_origin_renewal_dispatch(op.binding.instance, op.binding.handle, op.hop, op.operation)
        .unwrap();
    assert!(matches!(
        second.claim_origin_renewal_dispatch(
            op.binding.instance,
            op.binding.handle,
            op.hop,
            op.operation
        ),
        Err(LedgerError::DispatchAlreadyClaimed)
    ));
    second
        .admit_origin_renewal(scope(), charge(1), &proof(&op))
        .unwrap();
    second
        .admit_origin_renewal(scope(), charge(1), &proof(&op))
        .unwrap();
    let mut changed = op.clone();
    changed.operation = [99; 16];
    assert!(matches!(
        second.save_origin_renewal_operation(&changed),
        Err(LedgerError::Conflict)
    ));
    changed = op.clone();
    changed.parent = [99; 32];
    assert!(matches!(
        second.save_origin_renewal_operation(&changed),
        Err(LedgerError::Conflict)
    ));
}
#[test]
fn wrong_ids_policy_payee_price_and_reused_account_ids_cannot_be_funded() {
    let (_dir, mut ledger) = setup();
    for hop in 1..=3 {
        admitted(&mut ledger, hop);
    }
    for round in 2..=10 {
        ledger.advance_spending_round(scope(), round).unwrap();
    }
    for field in 0..5 {
        let (mut i, artifact) = intent(1, 11);
        i.operation = [210 + field; 16];
        match field {
            0 => i.payment_id = [99; 32],
            1 => i.hop_policy_hash = [99; 32],
            2 => i.hop_circuit_binding = [99; 32],
            3 => i.payee_reference = [99; 32],
            _ => i.amount_msat += 1,
        };
        ledger.save_origin_invoice(&i, &artifact).unwrap();
        assert!(ledger.prepare_outgoing(&i, clock()).is_err());
    }
    let (_dir, mut ledger) = setup();
    admitted(&mut ledger, 1);
    let mut other = operation(2);
    other.ids = operation(1).ids;
    ledger.save_origin_renewal_operation(&other).unwrap();
    ledger
        .claim_origin_renewal_dispatch(
            other.binding.instance,
            other.binding.handle,
            other.hop,
            other.operation,
        )
        .unwrap();
    assert!(matches!(
        ledger.admit_origin_renewal(scope(), charge(2), &proof(&other)),
        Err(LedgerError::Conflict)
    ));
    let count: i64 = ledger
        .connection
        .query_row(
            "SELECT count(*) FROM origin_outgoing_ids WHERE scope=?1 AND charge=?2 AND period=1",
            params![&scope()[..], &charge(2)[..]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}
#[test]
fn managed_dispatch_requires_saved_artifact_and_native_hop_registration_cannot_expand() {
    let (_dir, mut ledger) = setup();
    let (i, artifact) = intent(1, 1);
    assert!(matches!(
        ledger.prepare_outgoing(&i, clock()),
        Err(LedgerError::NotRegistered)
    ));
    ledger.save_origin_invoice(&i, &artifact).unwrap();
    ledger.prepare_outgoing(&i, clock()).unwrap();
    let extra = HopSpendingTerms {
        local_budget_scope: scope(),
        charge_reference: [99; 32],
        hop_circuit_binding: [98; 32],
        hop_policy_hash: [97; 32],
        payee_reference: [96; 32],
        handshake_amount_msat: 0,
        round_amount_msat: 1,
        fee_limit_msat: 0,
    };
    assert!(matches!(
        ledger.configure_hop_spending_terms(&extra),
        Err(LedgerError::Conflict)
    ));
    ledger
        .connection
        .execute(
            "DELETE FROM origin_invoices WHERE operation=?1",
            params![&i.operation[..]],
        )
        .unwrap();
    assert!(ledger.begin_dispatch(i.operation, clock()).is_err());
}
#[test]
fn v5_constraint_upgrade_retains_unknown_cost_and_rejects_corrupted_allocation_maps() {
    let (dir, mut ledger) = setup();
    let (i, artifact) = intent(1, 1);
    ledger.save_origin_invoice(&i, &artifact).unwrap();
    ledger.prepare_outgoing(&i, clock()).unwrap();
    ledger.begin_dispatch(i.operation, clock()).unwrap();
    ledger.mark_outgoing_unknown(i.operation).unwrap();
    // v5 never had managed origin plans. Keep its financial rows exactly and
    // remove only v6 journals in this faithful synthetic old-schema fixture.
    ledger.connection.execute_batch("DROP TABLE origin_outgoing_ids;DROP TABLE origin_allocations;DROP TABLE origin_renewal_operations;DROP TABLE origin_invoices;DROP TABLE origin_plans;DROP TABLE origin_constructions;PRAGMA user_version=5;").unwrap();
    drop(ledger);
    let ledger = PaymentLedger::upgrade_from_v5(&dir.file(), account()).unwrap();
    let recovered = ledger.get_outgoing(i.operation).unwrap().unwrap();
    assert!(recovered.state == OutgoingState::Unknown && recovered.reserved_msat == 1010);
    assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 1010);
    drop(ledger);
    let (dir, mut ledger) = setup();
    ledger
        .connection
        .execute(
            "UPDATE origin_outgoing_ids SET round=99 WHERE payment_id=?1",
            params![&registration(1).payment_ids()[0][..]],
        )
        .unwrap();
    drop(ledger);
    assert!(matches!(
        PaymentLedger::reopen(&dir.file(), account()),
        Err(LedgerError::InvalidLedger)
    ));
}
