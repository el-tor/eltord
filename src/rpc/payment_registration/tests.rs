use super::*;
use crate::payment_ledger::{ChargeKind, Obligation};
pub(crate) fn fixture(active: bool) -> (String, BatchRegistration) {
    let json: serde_json::Value = serde_json::from_str(include_str!(
        "../../../spec/vectors/payment_renewal_v1.json"
    ))
    .unwrap();
    let f = &json["fixtures"][0];
    let terms = bytes::<55>(f["terms_hex"].as_str().unwrap()).unwrap();
    let binding = bytes::<32>(f["C_hex"].as_str().unwrap()).unwrap();
    let allocation = bytes::<32>(f["periods"][0]["parent_digest_hex"].as_str().unwrap()).unwrap();
    let ids = f["initial_ids_hex"].as_str().unwrap();
    let batch = BatchRegistration {
        period: 0,
        request_digest: [3; 32],
        rounds: std::array::from_fn(|n| Obligation {
            instance: [1; 16],
            handle: [2; 16],
            circuit_binding: binding,
            round: n as u64 + 1,
            kind: ChargeKind::Round,
            payment_id: bytes(&ids[n * 64..(n + 1) * 64]).unwrap(),
            amount_msat: u64_at(&terms, 3),
            expected_payment_hash: None,
            policy_hash: hash(b"eltor/payment-renew/v1/terms", &[&terms]),
            allocation_hash: allocation,
            window: None,
        }),
    };
    let timeout = u32_at(&terms, 51);
    let grace = u32_at(&terms, 19);
    let rate = u64_at(&terms, 3);
    let mut lines = vec![
        format!(
            "250-ELTOR_REGISTRATION 1 {} {} 3 0",
            hex::encode([1; 16]),
            hex::encode([2; 16])
        ),
        format!(
            "250-POLICY {} {} {} 1",
            hex::encode(terms),
            hex::encode(allocation),
            hex::encode(allocation)
        ),
        format!("250-BINDING {}", hex::encode(binding)),
        if active {
            format!("250-STATE ACTIVE 1 0 0 {}", 60_000 + grace)
        } else {
            format!("250-STATE COMMITTED 0 0 {} 0", timeout - 20_000)
        },
        if active {
            "250-CLOCK 30000 15000".into()
        } else {
            "250-CLOCK 20000 5000".into()
        },
        if active {
            format!(
                "250-FEE 0 {} {} CONSUMED",
                hex::encode([0; 32]),
                hex::encode([30; 16])
            )
        } else {
            format!("250-FEE 0 {} - NONE", hex::encode([0; 32]))
        },
    ];
    for n in 0..10 {
        let window = if n == 0 {
            format!("0 {}", timeout - if active { 30_000 } else { 20_000 })
        } else if active {
            format!(
                "{} {}",
                (n as u64 * 60_000).saturating_sub(60_000),
                n as u64 * 60_000 + grace
            )
        } else {
            "- -".into()
        };
        let auth = if active && n == 0 {
            format!(
                "{} {} CONSUMED",
                hex::encode([31; 16]),
                hex::encode([40; 32])
            )
        } else {
            "- - NONE".into()
        };
        lines.push(format!(
            "250-ROUND {} {} {rate} {window} {auth}",
            n + 1,
            &ids[n * 64..(n + 1) * 64]
        ));
    }
    lines.push("250 OK".into());
    (lines.join("\n"), batch)
}
fn sample() -> RegistrationClockSample {
    RegistrationClockSample {
        sent: ClockObservation {
            now_ms: 1_000_000,
            uncertainty_ms: 2,
        },
        received: ClockObservation {
            now_ms: 1_000_010,
            uncertainty_ms: 3,
        },
        round_trip: Duration::from_millis(10),
    }
}
#[test]
fn exact_committed_and_t0_zero_elapsed_snapshots_preserve_immutable_terms() {
    for active in [false, true] {
        let (wire, batch) = fixture(active);
        let parsed = RelayRegistration::parse(&wire, &batch).unwrap();
        assert!(parsed.revision == 3 && parsed.binding == batch.rounds[0].circuit_binding);
        assert!(
            parsed.rounds[9].round == 10 && parsed.policy_hash() == batch.rounds[0].policy_hash
        );
        if active {
            let t0 = sample().activation(&parsed).unwrap();
            assert!(t0.earliest_ms == 999_998 && t0.latest_ms == 1_000_013);
        } else {
            assert!(matches!(
                sample().activation(&parsed),
                Err(RegistrationError::NotActivated)
            ));
        }
    }
}
#[test]
fn clocks_recover_original_commit_and_construction_instead_of_using_clamped_zero() {
    let (wire, batch) = fixture(false);
    let parsed = RelayRegistration::parse(&wire, &batch).unwrap();
    let commit = sample().commitment(&parsed).unwrap();
    assert!(commit.earliest_ms == 994_998 && commit.latest_ms == 995_013);
    let initial = sample().initial_window(&parsed).unwrap();
    assert!(initial.opens_at_ms == 995_013);
    assert!(initial.closes_at_ms == 979_998 + parsed.setup_timeout_ms() as i64);
    assert!(parsed.rounds[0].opens_in_ms == Some(0));
}
#[test]
fn missing_duplicate_noncanonical_and_changed_immutable_fields_are_rejected() {
    let (wire, batch) = fixture(false);
    let rows: Vec<_> = wire.lines().collect();
    for n in 0..rows.len() {
        let mut bad = rows.clone();
        bad.remove(n);
        assert!(RelayRegistration::parse(&bad.join("\n"), &batch).is_err());
    }
    for (row, token, text) in [
        (0, 4, "03"),
        (1, 4, "2"),
        (2, 1, "00"),
        (3, 1, "ADMISSION_PENDING"),
        (4, 2, "-"),
        (4, 2, "20001"),
        (5, 3, "00000000000000000000000000000000"),
        (6, 4, "1"),
        (7, 4, "0"),
        (6, 8, "AUTHORIZED"),
    ] {
        let mut lines: Vec<String> = rows.iter().map(|s| s.to_string()).collect();
        let mut fields: Vec<_> = rows[row].split(' ').collect();
        fields[token] = text;
        lines[row] = fields.join(" ");
        assert!(RelayRegistration::parse(&lines.join("\n"), &batch).is_err());
    }
    for n in 0..5 {
        let mut changed = batch.clone();
        match n {
            0 => changed.rounds[0].circuit_binding = [9; 32],
            1 => changed.rounds[3].payment_id = [9; 32],
            2 => changed.rounds[5].amount_msat += 1,
            3 => changed.rounds[0].policy_hash = [9; 32],
            _ => changed.rounds[4].allocation_hash = [9; 32],
        };
        assert!(RelayRegistration::parse(&wire, &changed).is_err());
    }
    assert!(RelayRegistration::parse(&(wire.clone() + "\n"), &batch).is_err());
    assert!(
        RelayRegistration::parse(&wire.replace(" 3 0\n", " 18446744073709551616 0\n"), &batch)
            .is_err()
    );
}
#[test]
fn paid_status_requires_consistent_authorizations_hashes_and_round_arithmetic() {
    let (wire, batch) = fixture(true);
    let rows: Vec<_> = wire.lines().collect();
    for (row, token, text) in [
        (3, 2, "2"),
        (3, 3, "60000"),
        (3, 4, "1"),
        (3, 5, "1"),
        (5, 4, "AUTHORIZED"),
        (6, 8, "AUTHORIZED"),
        (
            6,
            7,
            "0000000000000000000000000000000000000000000000000000000000000000",
        ),
        (7, 8, "CONSUMED"),
    ] {
        let mut lines: Vec<String> = rows.iter().map(|s| s.to_string()).collect();
        let mut fields: Vec<_> = rows[row].split(' ').collect();
        fields[token] = text;
        lines[row] = fields.join(" ");
        assert!(RelayRegistration::parse(&lines.join("\n"), &batch).is_err());
    }
}
#[test]
fn rollback_unbounded_uncertainty_and_impossible_transport_clocks_fail_closed() {
    let (wire, batch) = fixture(true);
    let parsed = RelayRegistration::parse(&wire, &batch).unwrap();
    let mut clock = sample();
    clock.received.now_ms = 999_990;
    assert!(clock.activation(&parsed).is_err());
    let mut clock = sample();
    clock.round_trip = Duration::from_nanos(1);
    assert!(clock.activation(&parsed).is_err());
    let mut clock = sample();
    clock.sent.uncertainty_ms = 5001;
    assert!(clock.activation(&parsed).is_err());
    let mut clock = sample();
    clock.sent.now_ms = 1;
    clock.received.now_ms = 11;
    assert!(clock.commitment(&parsed).is_err());
}

#[test]
fn observed_binding_retains_original_intervals_through_narrower_retries_and_restart() {
    use crate::payment_ledger::{AccountId, PaymentLedger, RecipientEvidence};
    let directory = std::env::temp_dir().join(format!(
        "eltor-observed-clock-{:032x}",
        rand::random::<u128>()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("account.sqlite");
    let mut ledger = PaymentLedger::provision(&path, AccountId([9; 32])).unwrap();
    let (wire, batch) = fixture(false);
    ledger.reserve_batch(&batch, [15; 16]).unwrap();
    let observed = ObservedRelayRegistration {
        registration: RelayRegistration::parse(&wire, &batch).unwrap(),
        clock: sample(),
    };
    let saved = observed.bind_saved_clocks(&mut ledger).unwrap();
    assert!(saved.activation.is_none());
    let evidence = RecipientEvidence {
        account: AccountId([9; 32]),
        payment_hash: [40; 32],
        invoice_digest: [41; 32],
        amount_msat: batch.rounds[0].amount_msat,
        settled_at_ms: 999_000,
        settlement_uncertainty_ms: 0,
        incoming: true,
        lightning: true,
        completed: true,
        authenticated_payment_id: Some(batch.rounds[0].payment_id),
    };
    ledger
        .authorize(
            &batch.rounds[0],
            &evidence,
            [31; 16],
            ClockObservation {
                now_ms: 1_000_020,
                uncertainty_ms: 0,
            },
        )
        .unwrap();
    let (wire, _) = fixture(true);
    let mut active = ObservedRelayRegistration {
        registration: RelayRegistration::parse(&wire, &batch).unwrap(),
        clock: sample(),
    };
    active.clock.sent.now_ms += 10_000;
    active.clock.received.now_ms += 10_000;
    let original = active
        .bind_saved_clocks(&mut ledger)
        .unwrap()
        .activation
        .unwrap();
    assert!(
        original.started_at.earliest_ms == 1_009_998 && original.started_at.latest_ms == 1_010_013
    );
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&path, AccountId([9; 32])).unwrap();
    active.clock.sent.now_ms += 5_000;
    active.clock.received.now_ms += 5_000;
    active.clock.sent.uncertainty_ms = 1;
    active.clock.received.uncertainty_ms = 1;
    active.registration.constructed_elapsed_ms += 5_000;
    active.registration.commit_elapsed_ms += 5_000;
    active.registration.elapsed_ms += 5_000;
    active.registration.enforcement_remaining_ms -= 5_000;
    for r in &mut active.registration.rounds {
        r.opens_in_ms = r.opens_in_ms.map(|n| n.saturating_sub(5_000));
        r.closes_in_ms = r.closes_in_ms.map(|n| n.saturating_sub(5_000));
    }
    active.registration.validate_state().unwrap();
    let recovered = active.bind_saved_clocks(&mut ledger).unwrap();
    assert!(
        recovered.activation == Some(original) && recovered.initial_window == saved.initial_window
    );
    active.clock.sent.now_ms += 100_000;
    active.clock.received.now_ms += 100_000;
    assert!(active.bind_saved_clocks(&mut ledger).is_err());
    assert!(
        ledger
            .get_activation_interval_anchor([1; 16], [2; 16])
            .unwrap()
            == Some(original)
    );
    drop(ledger);
    std::fs::remove_dir_all(directory).unwrap();
}

async fn control_fixture(
    replies: Vec<String>,
) -> (crate::rpc::PaymentControl, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let worker = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        for (n, reply) in vec!["250 OK".to_string(), "250 OK".to_string()]
            .into_iter()
            .chain(replies)
            .enumerate()
        {
            let mut command = String::new();
            stream.read_line(&mut command).await.unwrap();
            if n == 0 {
                assert!(command == "AUTHENTICATE\r\n");
            } else if n == 1 {
                assert!(command == "SETEVENTS ELTOR_PAYMENT_STATE\r\n");
            } else {
                assert!(command.starts_with("ELTOR_PAYMENT_REGISTRATION_GET 1 "));
            }
            stream
                .get_mut()
                .write_all(format!("{}\r\n", reply.replace('\n', "\r\n")).as_bytes())
                .await
                .unwrap();
        }
    });
    let config = crate::types::RpcConfig {
        addr: address.to_string(),
        rpc_password: None,
        command: String::new(),
    };
    (
        crate::rpc::PaymentControl::connect(&config).await.unwrap(),
        worker,
    )
}
fn read_clock() -> std::result::Result<ClockObservation, Box<dyn std::error::Error>> {
    Ok(ClockObservation {
        now_ms: 1_000_000,
        uncertainty_ms: 5000,
    })
}
#[tokio::test]
async fn observed_query_brackets_full_paced_exchange_and_malformed_reply_invalidates_connection() {
    let (wire, batch) = fixture(false);
    let malformed = wire.replace("250-BINDING ", "250-UNKNOWN ");
    let (mut control, worker) = control_fixture(vec![wire, malformed]).await;
    let observed = control
        .relay_registration_observed(&batch, read_clock)
        .await
        .unwrap();
    assert!(observed.registration.revision == 3 && observed.clock.round_trip > Duration::ZERO);
    assert!(control
        .relay_registration_observed(&batch, read_clock)
        .await
        .is_err());
    assert!(control.relay_registration(&batch).await.is_err());
    worker.await.unwrap();
}
#[tokio::test]
async fn canonical_transition_pending_is_retryable_without_new_clock_or_registration() {
    let (wire, batch) = fixture(false);
    let (mut control, worker) =
        control_fixture(vec!["551 STATE_TRANSITION_PENDING".into(), wire]).await;
    let error = control
        .relay_registration_observed(&batch, read_clock)
        .await
        .err()
        .unwrap();
    let io = error.downcast_ref::<std::io::Error>().unwrap();
    assert!(super::super::PaymentRejection::from_io(io).is_some());
    assert!(control
        .relay_registration_observed(&batch, read_clock)
        .await
        .is_ok());
    worker.await.unwrap();
}

#[test]
fn admitted_next_period_preserves_a0_financial_binding_and_distinct_allocation() {
    let (wire, mut batch) = fixture(true);
    let json: serde_json::Value = serde_json::from_str(include_str!(
        "../../../spec/vectors/payment_renewal_v1.json"
    ))
    .unwrap();
    let f = &json["fixtures"][0];
    let p = &f["periods"][0];
    assert!(p["period"].as_u64() == Some(1));
    let ids = p["new_ids_hex"].as_str().unwrap();
    let allocation = p["allocation_digest_hex"].as_str().unwrap();
    batch.period = 1;
    batch.request_digest = bytes(p["request_digest_hex"].as_str().unwrap()).unwrap();
    for (n, r) in batch.rounds.iter_mut().enumerate() {
        r.round = 11 + n as u64;
        r.payment_id = bytes(&ids[n * 64..(n + 1) * 64]).unwrap();
    }
    let mut lines: Vec<String> = wire.lines().map(str::to_owned).collect();
    let mut header: Vec<_> = lines[0].split(' ').map(str::to_owned).collect();
    header[5] = "1".into();
    lines[0] = header.join(" ");
    let mut policy: Vec<_> = lines[1].split(' ').map(str::to_owned).collect();
    policy[3] = allocation.into();
    policy[4] = "11".into();
    lines[1] = policy.join(" ");
    let grace = u32_at(&bytes(f["terms_hex"].as_str().unwrap()).unwrap(), 19);
    lines[3] = format!("250-STATE ACTIVE 7 360000 0 {}", 60_000 + grace);
    lines[4] = "250-CLOCK 390000 375000".into();
    for n in 0..10 {
        let round = 11 + n as u64;
        let nominal = (round - 1) * 60_000;
        lines[6 + n] = format!(
            "250-ROUND {round} {} {} {} {} - - NONE",
            &ids[n * 64..(n + 1) * 64],
            batch.rounds[n].amount_msat,
            nominal - 60_000 - 360_000,
            nominal + grace - 360_000
        );
    }
    let parsed = RelayRegistration::parse(&lines.join("\n"), &batch).unwrap();
    assert!(parsed.period == 1 && parsed.rounds[0].round == 11);
    assert!(
        parsed.initial_allocation == batch.rounds[0].allocation_hash
            && parsed.allocation != parsed.initial_allocation
    );
    let mut incorrect = batch;
    for r in &mut incorrect.rounds {
        r.allocation_hash = parsed.allocation;
    }
    assert!(parsed.validate_for(&incorrect).is_err());
}

#[test]
fn runtime_binding_refuses_legacy_point_only_window_without_inventing_provenance() {
    use crate::payment_ledger::{AccountId, PaymentLedger};
    let directory = std::env::temp_dir().join(format!(
        "eltor-legacy-clock-{:032x}",
        rand::random::<u128>()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("account.sqlite");
    let mut ledger = PaymentLedger::provision(&path, AccountId([9; 32])).unwrap();
    let (wire, batch) = fixture(false);
    ledger.reserve_batch(&batch, [15; 16]).unwrap();
    let prior = AcceptanceWindow {
        opens_at_ms: 1,
        closes_at_ms: 2_000_000,
    };
    ledger.bind_initial_window([1; 16], [2; 16], prior).unwrap();
    let observed = ObservedRelayRegistration {
        registration: RelayRegistration::parse(&wire, &batch).unwrap(),
        clock: sample(),
    };
    assert!(observed.bind_saved_clocks(&mut ledger).is_err());
    assert!(ledger.get_initial_window([1; 16], [2; 16]).unwrap() == Some(prior));
    assert!(ledger
        .get_initial_clock_anchor([1; 16], [2; 16])
        .unwrap()
        .is_none());
    drop(ledger);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn callback_time_in_outer_monotonic_bracket_does_not_require_equal_utc_sample_delta() {
    let mut clock = sample();
    clock.round_trip = Duration::from_secs(1);
    let bounded = clock.sample_interval().unwrap();
    assert!(bounded.earliest_ms == 999_998 && bounded.latest_ms == 1_000_013);
    clock.round_trip = Duration::from_micros(500);
    assert!(clock.sample_interval().is_err());
}
