//! Socket-level coordinator renewal tests use public synthetic regtest invoices.
use super::*;
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
#[derive(Default)]
struct RenewalTor {
    base: Arc<TorState>,
    round: AtomicUsize,
    renewals: Mutex<BTreeMap<(usize, String), RenewalOperation>>,
    withhold: AtomicBool,
    refuse: AtomicBool,
    lose: AtomicBool,
    pending: AtomicBool,
    capacity: AtomicBool,
    revision: AtomicUsize,
    admission_expired: AtomicBool,
}
fn decode<const N: usize>(text: &str) -> [u8; N] {
    hex::decode(text).unwrap().try_into().unwrap()
}
fn renewal_wire(state: &RenewalTor, hop: usize, op: &RenewalOperation) -> String {
    let round = state.round.load(Ordering::SeqCst).max(1) as u64;
    let admitted = !(hop == 1 && state.withhold.load(Ordering::SeqCst));
    let refused = hop == 1 && state.refuse.load(Ordering::SeqCst);
    let active = round >= op.first_round;
    let a = op.expected_allocation([hop as u8 + 3; 32]).unwrap();
    let (phase, status, digest) = if refused {
        ("REFUSED", "8", "-".into())
    } else if !admitted {
        ("SENT", "-", "-".into())
    } else if active {
        ("ACTIVE", "1", hex::encode(a))
    } else {
        ("ADMITTED", "0", hex::encode(a))
    };
    format!("250-ELTOR_PAYMENT 1 {} {} {} {}\n250-PROFILE {} {} {} {}\n250-SCHEDULE 59000 {} 900000\n250-OP {} {} {} {} {} {} 1\n250 OK",
        hex::encode(op.binding.instance),hex::encode(op.binding.handle),hop+1,100+state.revision.fetch_add(1,Ordering::SeqCst),
        hex::encode(op.terms),(round-1)/10,hex::encode(if active{a}else{op.parent}),round,
        if state.admission_expired.load(Ordering::SeqCst){0}else{180000},hex::encode(op.operation),phase,op.period,hex::encode([80+hop as u8;32]),digest,status)
}
async fn renewal_tor(
    req: Vec<FoundationHop>,
    state: Arc<RenewalTor>,
) -> (crate::types::RpcConfig, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = crate::types::RpcConfig {
        addr: listener.local_addr().unwrap().to_string(),
        rpc_password: None,
        command: String::new(),
    };
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let state = state.clone();
            let req = req.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(&mut socket);
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap() == 0 {
                        return;
                    }
                    let command = line.trim_end_matches("\r\n").to_string();
                    state.base.commands.lock().unwrap().push(command.clone());
                    let f: Vec<_> = command.split(' ').collect();
                    let reply = match f[0] {
                        "AUTHENTICATE" | "SETEVENTS" => "250 OK".into(),
                        "ELTOR_PAYMENT_LIST" => format!(
                            "250-ELTOR_PAYMENT_LIST 1 {} 1 -\n250-CIRCUIT {} ORIGIN 1\n250 OK",
                            hex::encode([1; 16]),
                            hex::encode([2; 16])
                        ),
                        "ELTOR_INITIAL_REGISTRATION_GET" => {
                            let hop = f[4].parse::<usize>().unwrap() - 1;
                            registration(hop, &req[hop])
                        }
                        "ELTOR_INITIAL_PREPARE"
                        | "ELTOR_INITIAL_START"
                        | "ELTOR_INITIAL_REFRESH" => {
                            let hop = f[4].parse::<usize>().unwrap() - 1;
                            let kind = match f[0] {
                                "ELTOR_INITIAL_PREPARE" => "COMMIT",
                                "ELTOR_INITIAL_START" => "START",
                                _ => "QUERY",
                            };
                            state
                                .base
                                .ops
                                .lock()
                                .unwrap()
                                .insert((hop, f[5].into()), kind.into());
                            format!(
                                "250 ELTOR_INITIAL_QUEUED 1 {} {} {} {} 1",
                                f[2], f[3], f[4], f[5]
                            )
                        }
                        "ELTOR_INITIAL_GET" => {
                            let hop = f[4].parse::<usize>().unwrap() - 1;
                            snapshot(&state.base, hop, f[5], &req[hop]).replace(
                                "1 1000 59000 74000 0",
                                &format!(
                                    "{} 1000 59000 74000 0",
                                    state.round.load(Ordering::SeqCst).max(1)
                                ),
                            )
                        }
                        "ELTOR_RENEW_PREPARE" => {
                            let hop = f[4].parse::<usize>().unwrap() - 1;
                            let op = RenewalOperation {
                                binding: binding(),
                                hop: hop as u16 + 1,
                                operation: decode(f[5]),
                                period: f[6].parse().unwrap(),
                                first_round: f[7].parse().unwrap(),
                                nonce: decode(f[8]),
                                terms: decode(f[9]),
                                parent: decode(f[10]),
                                ids: std::array::from_fn(|n| decode(&f[11][n * 64..(n + 1) * 64])),
                            };
                            state
                                .renewals
                                .lock()
                                .unwrap()
                                .insert((hop, f[5].into()), op);
                            if state.lose.swap(false, Ordering::SeqCst) {
                                return;
                            }
                            format!(
                                "250 ELTOR_RENEW_QUEUED 1 {} {} {} {} 1",
                                f[2], f[3], f[4], f[5]
                            )
                        }
                        "ELTOR_PAYMENT_GET" => {
                            if state.capacity.swap(false, Ordering::SeqCst) {
                                "451 CAPACITY".into()
                            } else if state.pending.swap(false, Ordering::SeqCst) {
                                "551 STATE_TRANSITION_PENDING".into()
                            } else {
                                let hop = f[4].parse::<usize>().unwrap() - 1;
                                let op = state
                                    .renewals
                                    .lock()
                                    .unwrap()
                                    .get(&(hop, f[5].into()))
                                    .cloned()
                                    .unwrap();
                                renewal_wire(&state, hop, &op)
                            }
                        }
                        _ => panic!("unexpected renewal control command"),
                    };
                    reader
                        .get_mut()
                        .write_all((reply.replace('\n', "\r\n") + "\r\n").as_bytes())
                        .await
                        .unwrap();
                }
            });
        }
    });
    (config, task)
}
async fn accepted_seeded(
    d: &Directory,
    config: &crate::types::RpcConfig,
    req: &[FoundationHop],
) -> (PaymentLedger, PaymentControl, FoundationPlan) {
    let mut l = ledger(d);
    let mut c = PaymentControl::connect(config).await.unwrap();
    let draft = FoundationDraft::inspect(
        &mut c,
        binding(),
        req,
        ClockObservation {
            now_ms: NOW - 1000,
            uncertainty_ms: 1,
        },
    )
    .await
    .unwrap();
    let plan = draft
        .accept_seeded(
            &mut l,
            [8; 32],
            FoundationLimits {
                lifetime_msat: 100000,
                outstanding_msat: 10000,
                per_round_msat: 10000,
            },
            [6; 32],
        )
        .unwrap();
    (l, c, plan)
}
async fn renewal_tick(r: &mut ClientFoundation, now: i64) -> Result<FoundationProgress> {
    // Advance the supplied UTC sample in this socket fixture without real
    // minute-long sleeps. Production clock discontinuity checks remain active.
    r.last_clock = None;
    r.next_poll = Instant::now();
    r.step(|| {
        Ok(ClockObservation {
            now_ms: now,
            uncertainty_ms: 0,
        })
    })
    .await
}
async fn ready_zero(
    d: &Directory,
    state: Arc<RenewalTor>,
) -> (
    ClientFoundation,
    crate::types::RpcConfig,
    tokio::task::JoinHandle<()>,
) {
    let req = requirements(false);
    let (config, server) = renewal_tor(req.clone(), state.clone()).await;
    let (l, c, p) = accepted_seeded(d, &config, &req).await;
    let mut r = ClientFoundation::new(l, c, None, p).unwrap();
    for _ in 0..7 {
        renewal_tick(&mut r, NOW).await.unwrap();
    }
    state.base.active.store(true, Ordering::SeqCst);
    state.round.store(7, Ordering::SeqCst);
    (r, config, server)
}
#[tokio::test]
async fn renewal_get_capacity_defers_same_connection_without_repreparing_or_funding() {
    let d = Directory::new();
    let state = Arc::new(RenewalTor::default());
    state.round.store(7, Ordering::SeqCst);
    let req = requirements(false);
    let (config, server) = renewal_tor(req.clone(), state.clone()).await;
    let (l, c, p) = accepted_seeded(&d, &config, &req).await;
    let mut r = ClientFoundation::new(l, c, None, p).unwrap();
    r.last_clock = Some((
        ClockObservation {
            now_ms: NOW,
            uncertainty_ms: 0,
        },
        Instant::now(),
    ));
    assert!(!r.reconcile_renewal(1).await.unwrap());
    let first = r.renewal_operation(0, 1).unwrap();
    state.capacity.store(true, Ordering::SeqCst);
    assert!(!r.reconcile_renewal(1).await.unwrap());
    assert!(!r.ledger.origin_period_admitted([8; 32], 1).unwrap());
    assert!(r.payment_id(0, 11).is_err());
    assert!(r.reconcile_renewal(1).await.unwrap());
    let commands = state.base.commands.lock().unwrap();
    assert_eq!(
        commands.iter().filter(|c| c.starts_with("AUTHENTICATE")).count(),
        1
    );
    assert_eq!(
        commands.iter().filter(|c| c.starts_with("ELTOR_RENEW_PREPARE")).count(),
        2
    );
    let original_get = format!(
        "ELTOR_PAYMENT_GET 1 {} {} 1 {}",
        hex::encode(first.binding.instance),
        hex::encode(first.binding.handle),
        hex::encode(first.operation),
    );
    assert_eq!(commands.iter().filter(|c| **c == original_get).count(), 2);
    assert_eq!(r.ledger.account_exposure().unwrap().spent_msat, 0);
    server.abort();
}
#[tokio::test]
async fn partial_renewal_and_lost_receipt_restore_only_exact_operations_before_round_eleven() {
    let d = Directory::new();
    let state = Arc::new(RenewalTor::default());
    state.withhold.store(true, Ordering::SeqCst);
    let (mut r, config, server) = ready_zero(&d, state.clone()).await;
    assert!(matches!(
        renewal_tick(&mut r, NOW + 390000).await.unwrap(),
        FoundationProgress::Active {
            current_round: 7,
            renewal_required: true
        }
    ));
    state.lose.store(true, Ordering::SeqCst);
    assert!(renewal_tick(&mut r, NOW + 390000).await.is_err());
    let first = r.renewal_operation(0, 1).unwrap();
    assert!(
        r.ledger
            .get_origin_renewal_operation([1; 16], [2; 16], 1, first.operation)
            .unwrap()
            .unwrap()
            .dispatch_claimed
    );
    drop(r);
    let mut l = PaymentLedger::reopen(&d.file(), AccountId(account().fingerprint())).unwrap();
    let p = FoundationPlan::restore(&mut l, [1; 16], [2; 16]).unwrap();
    let c = PaymentControl::connect(&config).await.unwrap();
    let mut r = ClientFoundation::new(l, c, None, p).unwrap();
    for _ in 0..7 {
        renewal_tick(&mut r, NOW + 390000).await.unwrap();
    }
    assert!(r
        .ledger
        .get_origin_allocation([8; 32], r.plan.charge_reference(0), 1)
        .unwrap()
        .is_some());
    assert!(!r.ledger.origin_period_admitted([8; 32], 1).unwrap());
    assert!(r.payment_id(0, 11).is_err());
    state.pending.store(true, Ordering::SeqCst); // Explicit QUERY recovers status coherence.
    for _ in 0..3 {
        renewal_tick(&mut r, NOW + 390000).await.unwrap();
    }
    state.withhold.store(false, Ordering::SeqCst);
    for _ in 0..5 {
        renewal_tick(&mut r, NOW + 390000).await.unwrap();
    }
    assert!(r.ledger.origin_period_admitted([8; 32], 1).unwrap());
    assert_eq!(r.payment_id(0, 11).unwrap(), first.ids[0]);
    let a = r
        .ledger
        .get_origin_allocation([8; 32], r.plan.charge_reference(0), 1)
        .unwrap()
        .unwrap();
    assert_eq!(a.parent, r.plan.hops[0].registration.initial_allocation());
    assert_eq!(
        a.allocation_digest,
        first.expected_allocation(a.circuit_binding).unwrap()
    );
    state.round.store(11, Ordering::SeqCst);
    assert!(matches!(
        renewal_tick(&mut r, NOW + 630000).await.unwrap(),
        FoundationProgress::Active {
            current_round: 11,
            ..
        }
    ));
    let commands = state.base.commands.lock().unwrap();
    let prepares: Vec<_> = commands
        .iter()
        .filter(|c| c.starts_with("ELTOR_RENEW_PREPARE"))
        .collect();
    assert_eq!(prepares.len(), 2); // One exact write per hop despite lost receipt/restart.
    assert_eq!(
        prepares
            .iter()
            .filter(|c| c.split(' ').nth(5) == Some(hex::encode(first.operation).as_str()))
            .count(),
        1
    );
    server.abort();
}
#[tokio::test]
async fn refused_renewal_cannot_manufacture_a_later_allocation_or_round_id() {
    let d = Directory::new();
    let state = Arc::new(RenewalTor::default());
    state.refuse.store(true, Ordering::SeqCst);
    let (mut r, _, server) = ready_zero(&d, state.clone()).await;
    let mut failed = false;
    for _ in 0..5 {
        if renewal_tick(&mut r, NOW + 390000).await.is_err() {
            failed = true;
            break;
        }
    }
    assert!(failed);
    assert!(!r.ledger.origin_period_admitted([8; 32], 1).unwrap());
    assert!(r.payment_id(0, 11).is_err());
    assert!(r
        .ledger
        .get_origin_allocation([8; 32], r.plan.charge_reference(1), 1)
        .unwrap()
        .is_none());
    server.abort();
}
#[derive(Default)]
struct RoundNative {
    rows: Mutex<BTreeMap<String, (u64, String)>>,
    fetches: Mutex<Vec<u64>>,
    pays: AtomicUsize,
    completed_sec: std::sync::atomic::AtomicU64,
}
fn invoice_for(round: u64) -> String {
    if round == 1 {
        return INVOICE.trim().into();
    }
    let rows: serde_json::Value =
        serde_json::from_str(include_str!("foundation/fixtures/renewal-invoices.json")).unwrap();
    rows.as_array()
        .unwrap()
        .iter()
        .find(|row| row["round"] == round)
        .expect("only11 fixture invoices")["invoice"]
        .as_str()
        .unwrap()
        .into()
}
fn round_note(round: u64) -> [u8; 32] {
    if round == 1 {
        [7; 32]
    } else if round <= 10 {
        [round as u8 + 7; 32]
    } else {
        digest(
            b"eltor/origin/v1/renewal-payment-id",
            &[
                &[6; 32],
                &1u16.to_be_bytes(),
                &1u64.to_be_bytes(),
                &0u16.to_be_bytes(),
            ],
        )
    }
}
fn round_preimage(round: u64) -> [u8; 32] {
    if round == 1 {
        [42; 32]
    } else {
        [42 + round as u8; 32]
    }
}
async fn round_wallet(
    d: &Directory,
    tor: Arc<RenewalTor>,
    state: Arc<RoundNative>,
) -> (Arc<FoundationPayer>, tokio::task::JoinHandle<()>) {
    use serde_json::json;
    use tokio::io::AsyncReadExt;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = lni::cln::ClnConfig {
        url: format!("http://{}/", listener.local_addr().unwrap()),
        rune: "synthetic".into(),
        ..Default::default()
    };
    let path = d.file();
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let state = state.clone();
            let tor = tor.clone();
            let path = path.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let header_end;
                loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                    if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = i + 4;
                        break;
                    }
                }
                let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse().unwrap())
                    })
                    .unwrap();
                while bytes.len() < header_end + length {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                }
                let args: serde_json::Value =
                    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                let method = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split(' ')
                    .nth(1)
                    .unwrap()
                    .trim_start_matches("/v1/");
                let reply = match method {
                    "getinfo" => {
                        json!({"id":hex::encode(account().node_id),"network":"regtest","version":"v25.09"})
                    }
                    "help" => {
                        json!({"help":[{"command":match args["command"].as_str().unwrap(){"pay"=>"pay bolt11 [label] [maxfee]","listpays"=>"listpays [payment_hash]","fetchinvoice"=>"fetchinvoice offer [amount_msat] [payer_note]",_=>panic!("unexpected help")}}]})
                    }
                    "fetchinvoice" => {
                        assert_eq!(*tor.base.committed.lock().unwrap(), [true, true]);
                        let round = (1..=11)
                            .find(|r| args["payer_note"] == hex::encode(round_note(*r)))
                            .unwrap();
                        if round == 11 {
                            let ledger =
                                PaymentLedger::reopen(&path, AccountId(account().fingerprint()))
                                    .unwrap();
                            assert!(ledger.origin_period_admitted([8; 32], 1).unwrap());
                        }
                        assert_eq!(args["offer"], OFFER.trim());
                        state.fetches.lock().unwrap().push(round);
                        json!({"invoice":invoice_for(round)})
                    }
                    "pay" => {
                        let round = (1..=11)
                            .find(|r| args["bolt11"] == invoice_for(*r))
                            .unwrap();
                        let label = args["label"].as_str().unwrap().to_string();
                        let ledger =
                            PaymentLedger::reopen(&path, AccountId(account().fingerprint()))
                                .unwrap();
                        let op = decode(&label);
                        assert!(
                            ledger.get_outgoing(op).unwrap().unwrap().state
                                == OutgoingState::InFlight
                        );
                        assert!(
                            ledger.get_origin_invoice(op).unwrap().unwrap().artifact
                                == invoice_for(round)
                        );
                        assert!(state
                            .rows
                            .lock()
                            .unwrap()
                            .insert(label, (round, invoice_for(round)))
                            .is_none());
                        state.pays.fetch_add(1, Ordering::SeqCst);
                        json!({"status":"complete"})
                    }
                    "listpays" => {
                        let rows=state.rows.lock().unwrap().iter().filter(|(_, (round,_))|args["payment_hash"]==hex::encode(Sha256::digest(round_preimage(*round)))).map(|(label,(round,invoice))|json!({"payment_hash":hex::encode(Sha256::digest(round_preimage(*round))),"label":label,"bolt12":invoice,"status":"complete","created_at":1_700_000_000u64,"completed_at":state.completed_sec.load(Ordering::SeqCst),"amount_msat":2000,"amount_sent_msat":2050,"preimage":hex::encode(round_preimage(*round))})).collect::<Vec<_>>();
                        json!({"pays":rows})
                    }
                    _ => panic!("unexpected native operation"),
                };
                let body = serde_json::to_vec(&reply).unwrap();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            });
        }
    });
    (
        Arc::new(FoundationPayer::from(ClnFoundation::connect(config, account()).await.unwrap())),
        task,
    )
}
#[tokio::test]
async fn positive_round_eleven_uses_new_admitted_ids_after_every_hop_and_preserves_original_prices()
{
    let d = Directory::new();
    let req = requirements(true);
    let state = Arc::new(RenewalTor::default());
    state.withhold.store(true, Ordering::SeqCst);
    let (config, server) = renewal_tor(req.clone(), state.clone()).await;
    let (l, c, p) = accepted_seeded(&d, &config, &req).await;
    let native = Arc::new(RoundNative::default());
    native
        .completed_sec
        .store((NOW / 1000) as u64, Ordering::SeqCst);
    let (wallet, work) = round_wallet(&d, state.clone(), native.clone()).await;
    let mut r = ClientFoundation::new(l, c, Some(wallet.clone()), p).unwrap();
    for _ in 0..7 {
        renewal_tick(&mut r, NOW).await.unwrap();
    }
    state.base.active.store(true, Ordering::SeqCst);
    state.round.store(1, Ordering::SeqCst);
    for round in 1..=9 {
        let now = NOW + (round - 1) * 60000 + 30000;
        state.round.store(round as usize, Ordering::SeqCst);
        native
            .completed_sec
            .store((now / 1000) as u64, Ordering::SeqCst);
        for _ in 0..7 {
            renewal_tick(&mut r, now).await.unwrap();
        }
        if round >= 7 {
            assert!(!r.ledger.origin_period_admitted([8; 32], 1).unwrap());
            assert!(!native.fetches.lock().unwrap().contains(&11));
        }
    }
    // Round ten was funded during round nine, yet round eleven cannot even
    // fetch until both authenticated admissions and the round-ten window.
    assert_eq!(native.pays.load(Ordering::SeqCst), 10);
    state.withhold.store(false, Ordering::SeqCst);
    for _ in 0..7 {
        renewal_tick(&mut r, NOW + 510000).await.unwrap();
    }
    assert!(r.ledger.origin_period_admitted([8; 32], 1).unwrap());
    assert_eq!(native.pays.load(Ordering::SeqCst), 10);
    assert!(!native.fetches.lock().unwrap().contains(&11));
    state.round.store(10, Ordering::SeqCst);
    native
        .completed_sec
        .store(((NOW + 570000) / 1000) as u64, Ordering::SeqCst);
    for _ in 0..3 {
        renewal_tick(&mut r, NOW + 570000).await.unwrap();
    }
    assert_eq!(native.pays.load(Ordering::SeqCst), 11);
    let next = r
        .ledger
        .get_outgoing(r.round_operation(0, 11))
        .unwrap()
        .unwrap();
    assert_eq!(next.intent.payment_id, round_note(11));
    assert_eq!(next.intent.amount_msat, 2000);
    assert_eq!(next.intent.fee_limit_msat, 100);
    assert_eq!(
        next.intent.hop_policy_hash,
        r.plan.hops[0].registration.policy_hash()
    );
    assert_eq!(
        next.intent.hop_circuit_binding,
        r.plan.hops[0].registration.circuit_binding()
    );
    assert!(next.state == OutgoingState::Settled);
    // Reopen after pay but before observing native activation. Restore original
    // artifacts and lookup settlement; never dispatch/fetch the eleventh twice.
    drop(r);
    let mut l = PaymentLedger::reopen(&d.file(), AccountId(account().fingerprint())).unwrap();
    let p = FoundationPlan::restore(&mut l, [1; 16], [2; 16]).unwrap();
    let c = PaymentControl::connect(&config).await.unwrap();
    let mut r = ClientFoundation::new(l, c, Some(wallet), p).unwrap();
    state.round.store(11, Ordering::SeqCst);
    for _ in 0..2 {
        renewal_tick(&mut r, NOW + 600000).await.unwrap();
    }
    assert!(matches!(
        renewal_tick(&mut r, NOW + 600000).await.unwrap(),
        FoundationProgress::Active {
            current_round: 11,
            ..
        }
    ));
    assert!(
        r.ledger
            .get_outgoing(r.round_operation(0, 11))
            .unwrap()
            .unwrap()
            .state
            == OutgoingState::Consumed
    );
    assert_eq!(r.ledger.path_exposure([8; 32]).unwrap().spent_msat, 22550);
    assert_eq!(native.fetches.lock().unwrap().len(), 11);
    assert_eq!(
        state
            .base
            .commands
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("ELTOR_RENEW_PREPARE"))
            .count(),
        2
    );
    server.abort();
    work.abort();
}

#[tokio::test]
async fn earlier_admission_survives_restart_after_cutoff_but_new_late_prepare_is_rejected() {
    let d = Directory::new();
    let state = Arc::new(RenewalTor::default());
    let (mut r, config, server) = ready_zero(&d, state.clone()).await;
    renewal_tick(&mut r, NOW + 390000).await.unwrap();
    renewal_tick(&mut r, NOW + 390000).await.unwrap(); // Both exact PREPARE writes, replies not yet queried.
    assert_eq!(state.renewals.lock().unwrap().len(), 2);
    assert!(!r.ledger.origin_period_admitted([8; 32], 1).unwrap());
    drop(r);
    state.admission_expired.store(true, Ordering::SeqCst);
    let mut l = PaymentLedger::reopen(&d.file(), AccountId(account().fingerprint())).unwrap();
    let p = FoundationPlan::restore(&mut l, [1; 16], [2; 16]).unwrap();
    let c = PaymentControl::connect(&config).await.unwrap();
    let mut r = ClientFoundation::new(l, c, None, p).unwrap();
    for _ in 0..5 {
        renewal_tick(&mut r, NOW + 550000).await.unwrap();
    }
    assert!(r.ledger.origin_period_admitted([8; 32], 1).unwrap());
    assert_eq!(
        state
            .base
            .commands
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("ELTOR_RENEW_PREPARE"))
            .count(),
        2
    );
    server.abort();
    let d = Directory::new();
    let state = Arc::new(RenewalTor::default());
    let (mut r, _, server) = ready_zero(&d, state.clone()).await;
    renewal_tick(&mut r, NOW + 550000).await.unwrap();
    assert!(renewal_tick(&mut r, NOW + 550000).await.is_err());
    assert!(state.renewals.lock().unwrap().is_empty());
    server.abort();
}
#[tokio::test]
async fn initial_observation_subtracts_full_exchange_and_cannot_refill_same_round_deadlines() {
    let d = Directory::new();
    let req = requirements(false);
    let state = Arc::new(TorState::default());
    state.instance.store(1, Ordering::SeqCst);
    let (config, server) = tor_server(req.clone(), state.clone()).await;
    let (l, c, p) = accepted_seeded(&d, &config, &req).await;
    let mut r = ClientFoundation::new(l, c, None, p).unwrap();
    state.active.store(true, Ordering::SeqCst);
    state
        .ops
        .lock()
        .unwrap()
        .insert((0, hex::encode([5; 16])), "QUERY".into());
    let wire = snapshot(&state, 0, &hex::encode([5; 16]), &req[0]);
    let observation = InitialOriginSnapshot::parse(&wire, &binding(), 1, Some([5; 16])).unwrap();
    r.retain_snapshot(
        0,
        observation.clone(),
        Instant::now() - Duration::from_millis(100),
    )
    .unwrap();
    let first = r.observations[0].as_ref().unwrap().snapshot.clocks;
    assert!(first[2].unwrap() <= 58900);
    assert!(first[3].unwrap() <= 73900);
    r.retain_snapshot(0, observation.clone(), Instant::now())
        .unwrap();
    let next = r.observations[0].as_ref().unwrap().snapshot.clocks;
    assert!(next[2].unwrap() < first[2].unwrap());
    assert!(next[3].unwrap() < first[3].unwrap());
    let mut stale = observation;
    stale.clocks[0] = Some(0);
    assert!(r.retain_snapshot(0, stale, Instant::now()).is_err());
    server.abort();
}
