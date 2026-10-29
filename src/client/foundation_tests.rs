use super::*;
use lni::cln::{
    foundation::{Chain, NativeAccount},
    ClnConfig,
};
use serde_json::{json, Value};
use std::convert::TryInto;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
const NOW: i64 = 1_700_000_100_000;
const OFFER: &str = include_str!("foundation/fixtures/round-one-offer.txt");
const INVOICE: &str = include_str!("foundation/fixtures/round-one-invoice.txt");
fn account() -> NativeAccount {
    NativeAccount {
        node_id: hex::decode("031b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f")
            .unwrap()
            .try_into()
            .unwrap(),
        chain: Chain::Regtest,
    }
}
fn clock() -> Result<ClockObservation> {
    Ok(ClockObservation {
        now_ms: NOW,
        uncertainty_ms: 0,
    })
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "eltor-origin-coordinator-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
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
fn binding() -> OriginBinding {
    OriginBinding {
        instance: [1; 16],
        handle: [2; 16],
        hops: 2,
        revision: 1,
    }
}
fn terms(price: u64) -> [u8; 55] {
    let mut t = [0; 55];
    t[..3].copy_from_slice(&[1, 1, 10]);
    t[3..11].copy_from_slice(&price.to_be_bytes());
    t[11..15].copy_from_slice(&60000u32.to_be_bytes());
    t[15..19].copy_from_slice(&60000u32.to_be_bytes());
    t[19..23].copy_from_slice(&15000u32.to_be_bytes());
    t[31..39].copy_from_slice(&1800000u64.to_be_bytes());
    t[39..43].copy_from_slice(&3u32.to_be_bytes());
    t[51..55].copy_from_slice(&30000u32.to_be_bytes());
    t
}
fn requirements(positive: bool) -> Vec<FoundationHop> {
    (0..2)
        .map(|hop| FoundationHop {
            descriptor: FoundationDescriptor {
                fingerprint: [hop as u8 + 1; 20],
                terms: terms(if positive && hop == 0 { 2000 } else { 0 }),
                bolt12_offer: if positive && hop == 0 {
                    Some(OFFER.trim().into())
                } else {
                    None
                },
            },
            payment_ids: std::array::from_fn(|i| {
                if hop == 0 && i == 0 {
                    [7; 32]
                } else {
                    [hop as u8 * 20 + i as u8 + 8; 32]
                }
            }),
            fee_limit_msat: if positive && hop == 0 { 100 } else { 0 },
        })
        .collect()
}
fn registration(hop: usize, r: &FoundationHop) -> String {
    let b = binding();
    let c = [hop as u8 + 3; 32];
    let q = digest(b"eltor/payment-renew/v1/terms", &[&r.descriptor.terms]);
    let ids: Vec<u8> = r.payment_ids.iter().flatten().copied().collect();
    let a = digest(b"eltor/payment-renew/v1/initial", &[&c, &q, &ids]);
    let mut s = format!(
        "250-ELTOR_INITIAL_REGISTRATION 1 {} {} {} 1\n250-POLICY {} {} {}\n250-BINDING {}\n",
        hex::encode(b.instance),
        hex::encode(b.handle),
        hop + 1,
        hex::encode(r.descriptor.terms),
        hex::encode(q),
        hex::encode(a),
        hex::encode(c)
    );
    for (i, id) in r.payment_ids.iter().enumerate() {
        s += &format!("250-ROUND {} {}\n", i + 1, hex::encode(id));
    }
    s + "250 OK"
}
#[derive(Default)]
struct TorState {
    commands: Mutex<Vec<String>>,
    ops: Mutex<BTreeMap<(usize, String), String>>,
    committed: Mutex<[bool; 2]>,
    withhold_second: AtomicBool,
    active: AtomicBool,
    lose_start: AtomicBool,
    instance: AtomicUsize,
}
fn snapshot(state: &TorState, hop: usize, op: &str, req: &FoundationHop) -> String {
    let b = binding();
    let q = digest(b"eltor/payment-renew/v1/terms", &[&req.descriptor.terms]);
    let ids: Vec<u8> = req.payment_ids.iter().flatten().copied().collect();
    let a = digest(
        b"eltor/payment-renew/v1/initial",
        &[&[hop as u8 + 3; 32], &q, &ids],
    );
    let kind = state
        .ops
        .lock()
        .unwrap()
        .get(&(hop, op.into()))
        .cloned()
        .unwrap();
    let verified = !(kind == "COMMIT" && hop == 1 && state.withhold_second.load(Ordering::SeqCst));
    if kind == "COMMIT" && verified {
        state.committed.lock().unwrap()[hop] = true;
    }
    let active = state.active.load(Ordering::SeqCst);
    let profile = if active {
        "ACTIVE"
    } else if kind == "START" || kind == "QUERY" {
        "WAIT_PAYMENT"
    } else {
        "COMMITTED"
    };
    format!("250-ELTOR_INITIAL 1 {} {} {} 2\n250-PROFILE {} {} {} 2\n250-CLOCK {}\n250-OP {} {} {} {} 1\n250 OK",hex::encode(b.instance),hex::encode(b.handle),hop+1,hex::encode(q),hex::encode(a),profile,if active{"1 1000 59000 74000 0"}else{"0 0 0 0 25000"},op,kind,if verified{"VERIFIED"}else{"SENT"},hex::encode([9;32]))
}
async fn tor_server(
    req: Vec<FoundationHop>,
    state: Arc<TorState>,
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
            let req = req.clone();
            let state = state.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(&mut socket);
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap() == 0 {
                        return;
                    }
                    let command = line.trim_end_matches("\r\n").to_owned();
                    state.commands.lock().unwrap().push(command.clone());
                    let f: Vec<_> = command.split(' ').collect();
                    let reply = match f[0] {
                        "AUTHENTICATE" | "SETEVENTS" => "250 OK".into(),
                        "ELTOR_PAYMENT_LIST" => format!(
                            "250-ELTOR_PAYMENT_LIST 1 {} 1 -\n250-CIRCUIT {} ORIGIN 1\n250 OK",
                            hex::encode([state.instance.load(Ordering::SeqCst) as u8; 16]),
                            hex::encode([2; 16])
                        ),
                        "ELTOR_INITIAL_REGISTRATION_GET" => {
                            let hop: usize = f[4].parse::<usize>().unwrap() - 1;
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
                                .ops
                                .lock()
                                .unwrap()
                                .insert((hop, f[5].into()), kind.into());
                            if kind == "START" && state.lose_start.swap(false, Ordering::SeqCst) {
                                return;
                            }
                            format!(
                                "250 ELTOR_INITIAL_QUEUED 1 {} {} {} {} 1",
                                f[2], f[3], f[4], f[5]
                            )
                        }
                        "ELTOR_INITIAL_GET" => {
                            let hop = f[4].parse::<usize>().unwrap() - 1;
                            snapshot(&state, hop, f[5], &req[hop])
                        }
                        _ => panic!("unexpected controlcommand"),
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
fn ledger(d: &Directory) -> PaymentLedger {
    let mut l = PaymentLedger::provision(&d.file(), AccountId(account().fingerprint())).unwrap();
    l.configure_account_budget(AccountBudget {
        lifetime_limit_msat: 100000,
        outstanding_limit_msat: 100000,
        max_queued_intents: 128,
        max_wallet_rpcs: 32,
        max_unresolved_intents: 128,
    })
    .unwrap();
    l.save_origin_construction([8; 32], [1; 16], b"exact privately accepted constructor")
        .unwrap();
    l.claim_origin_construction([8; 32]).unwrap();
    l
}
async fn accepted(
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
    let p = draft
        .accept(
            &mut l,
            [8; 32],
            FoundationLimits {
                lifetime_msat: 100000,
                outstanding_msat: 10000,
                per_round_msat: 10000,
            },
        )
        .unwrap();
    (l, c, p)
}
async fn tick(r: &mut ClientFoundation) -> Result<FoundationProgress> {
    r.next_poll = Instant::now();
    r.step(clock).await
}
#[tokio::test]
async fn zero_price_still_requires_every_commit_and_authenticated_active() {
    let d = Directory::new();
    let req = requirements(false);
    let state = Arc::new(TorState::default());
    state.instance.store(1, Ordering::SeqCst);
    state.withhold_second.store(true, Ordering::SeqCst);
    let (config, server) = tor_server(req.clone(), state.clone()).await;
    let (l, c, p) = accepted(&d, &config, &req).await;
    let mut runner = ClientFoundation::new(l, c, None, p).unwrap();
    assert_eq!(
        tick(&mut runner).await.unwrap(),
        FoundationProgress::Recovered
    );
    assert_eq!(
        tick(&mut runner).await.unwrap(),
        FoundationProgress::AwaitingCommitment
    );
    assert_eq!(
        tick(&mut runner).await.unwrap(),
        FoundationProgress::AwaitingCommitment
    );
    assert!(!state
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.starts_with("ELTOR_INITIAL_START")));
    state.withhold_second.store(false, Ordering::SeqCst);
    for _ in 0..4 {
        let _ = tick(&mut runner).await.unwrap();
    }
    assert!(state
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.starts_with("ELTOR_INITIAL_START")));
    assert!(runner.ledger.account_exposure().unwrap().reserved_msat == 0);
    state.active.store(true, Ordering::SeqCst);
    for _ in 0..3 {
        let progress = tick(&mut runner).await.unwrap();
        if matches!(progress, FoundationProgress::Active { .. }) {
            server.abort();
            return;
        }
    }
    panic!("all-hopzeroactivationnotobserved")
}
#[tokio::test]
async fn accepted_plan_reopens_exactly_and_changed_policy_never_commits() {
    let d = Directory::new();
    let req = requirements(false);
    let state = Arc::new(TorState::default());
    state.instance.store(1, Ordering::SeqCst);
    let (config, server) = tor_server(req.clone(), state.clone()).await;
    let (l, c, p) = accepted(&d, &config, &req).await;
    let bytes = p.encode().unwrap();
    drop((l, c, p));
    let mut l = PaymentLedger::reopen(&d.file(), AccountId(account().fingerprint())).unwrap();
    let p = FoundationPlan::restore(&mut l, [1; 16], [2; 16]).unwrap();
    assert_eq!(bytes, p.encode().unwrap());
    let mut wrong = req.clone();
    wrong[1].descriptor.terms = terms(9000);
    let mut c = PaymentControl::connect(&config).await.unwrap();
    assert!(
        FoundationDraft::inspect(&mut c, binding(), &wrong, clock().unwrap())
            .await
            .is_err()
    );
    assert!(!state
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.starts_with("ELTOR_INITIAL_PREPARE")));
    server.abort();
}

#[derive(Default)]
struct NativeState {
    fetches: AtomicUsize,
    pays: AtomicUsize,
    lookups: AtomicUsize,
    settled: AtomicBool,
    lose_pay: AtomicBool,
    label: Mutex<Option<String>>,
}
async fn wallet_server(
    directory: &Directory,
    tor: Arc<TorState>,
    state: Arc<NativeState>,
) -> (Arc<ClnFoundation>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = ClnConfig {
        url: format!("http://{}/", listener.local_addr().unwrap()),
        rune: "synthetic-fixture-only".into(),
        ..Default::default()
    };
    let path = directory.file();
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let tor = tor.clone();
            let state = state.clone();
            let path = path.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0; 1024];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    assert!(bytes.len() < 16384);
                    if let Some(at) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
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
                let args: Value =
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
                        json!({"help":[{"command": match args["command"].as_str().unwrap() {"pay"=>"pay bolt11 [label] [maxfee]","listpays"=>"listpays [payment_hash]","fetchinvoice"=>"fetchinvoice offer [amount_msat] [payer_note]",_=>panic!("unexpectednativehelp")} }]})
                    }
                    "fetchinvoice" => {
                        assert!(*tor.committed.lock().unwrap() == [true, true]);
                        assert!(args["offer"] == OFFER.trim());
                        assert!(args["payer_note"] == hex::encode([7; 32]));
                        assert_eq!(state.fetches.fetch_add(1, Ordering::SeqCst), 0);
                        json!({"invoice":INVOICE.trim()})
                    }
                    "pay" => {
                        assert!(*tor.committed.lock().unwrap() == [true, true]);
                        let operation: [u8; 16] = hex::decode(args["label"].as_str().unwrap())
                            .unwrap()
                            .try_into()
                            .unwrap();
                        // A separate connection observes the real durable claim
                        // and artifact before the native socket receives pay.
                        let ledger =
                            PaymentLedger::reopen(&path, AccountId(account().fingerprint()))
                                .unwrap();
                        assert!(
                            ledger.get_outgoing(operation).unwrap().unwrap().state
                                == OutgoingState::InFlight
                        );
                        assert!(
                            ledger
                                .get_origin_invoice(operation)
                                .unwrap()
                                .unwrap()
                                .artifact
                                == INVOICE.trim()
                        );
                        assert_eq!(state.pays.fetch_add(1, Ordering::SeqCst), 0);
                        *state.label.lock().unwrap() = Some(args["label"].as_str().unwrap().into());
                        if state.lose_pay.swap(false, Ordering::SeqCst) {
                            return;
                        }
                        json!({"status":"complete"})
                    }
                    "listpays" => {
                        state.lookups.fetch_add(1, Ordering::SeqCst);
                        if state.pays.load(Ordering::SeqCst) == 0
                            || !state.settled.load(Ordering::SeqCst)
                        {
                            json!({"pays":[]})
                        } else {
                            json!({"pays":[{"payment_hash":hex::encode(Sha256::digest([42;32])),"label":state.label.lock().unwrap().clone().unwrap(),"bolt12":INVOICE.trim(),"status":"complete","created_at":1_700_000_000u64,"completed_at":1_700_000_101u64,"amount_msat":2000,"amount_sent_msat":2050,"preimage":hex::encode([42;32])}]})
                        }
                    }
                    _ => panic!("unexpectednativeoperation"),
                };
                let body = serde_json::to_vec(&reply).unwrap();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            });
        }
    });
    let wallet = Arc::new(ClnFoundation::connect(config, account()).await.unwrap());
    (wallet, task)
}

async fn tick_later(r: &mut ClientFoundation) -> Result<FoundationProgress> {
    r.next_poll = Instant::now();
    r.step(|| {
        Ok(ClockObservation {
            now_ms: NOW + 1000,
            uncertainty_ms: 0,
        })
    })
    .await
}

#[tokio::test]
async fn positive_price_uses_durable_native_pay_once_and_recovers_lost_pay_and_start() {
    let d = Directory::new();
    let req = requirements(true);
    let tor = Arc::new(TorState::default());
    tor.instance.store(1, Ordering::SeqCst);
    tor.withhold_second.store(true, Ordering::SeqCst);
    let (config, server) = tor_server(req.clone(), tor.clone()).await;
    let (ledger, control, plan) = accepted(&d, &config, &req).await;
    let native = Arc::new(NativeState::default());
    native.lose_pay.store(true, Ordering::SeqCst);
    let (wallet, wallet_task) = wallet_server(&d, tor.clone(), native.clone()).await;
    let mut runner = ClientFoundation::new(ledger, control, Some(wallet.clone()), plan).unwrap();
    for _ in 0..3 {
        tick(&mut runner).await.unwrap();
    }
    assert_eq!(native.fetches.load(Ordering::SeqCst), 0);
    assert_eq!(native.pays.load(Ordering::SeqCst), 0);
    tor.withhold_second.store(false, Ordering::SeqCst);
    let status = tick(&mut runner).await.unwrap();
    assert!(matches!(
        status,
        FoundationProgress::AwaitingPayment { hop: 1, round: 1 }
    ));
    let operation = runner.round_operation(0, 1);
    assert!(
        runner
            .ledger
            .get_outgoing(operation)
            .unwrap()
            .unwrap()
            .state
            == OutgoingState::Unknown
    );
    assert!(!tor
        .commands
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.starts_with("ELTOR_INITIAL_START")));
    drop(runner);
    // The original accepted plan and invoice are restored; missing native
    // results cause lookup only, never another fetch, invoice or pay operation.
    let mut ledger = PaymentLedger::reopen(&d.file(), AccountId(account().fingerprint())).unwrap();
    let plan = FoundationPlan::restore(&mut ledger, [1; 16], [2; 16]).unwrap();
    let control = PaymentControl::connect(&config).await.unwrap();
    let mut runner = ClientFoundation::new(ledger, control, Some(wallet.clone()), plan).unwrap();
    tick(&mut runner).await.unwrap();
    assert!(matches!(
        tick(&mut runner).await.unwrap(),
        FoundationProgress::AwaitingPayment { .. }
    ));
    native.settled.store(true, Ordering::SeqCst);
    tor.lose_start.store(true, Ordering::SeqCst);
    assert!(tick_later(&mut runner).await.is_err()); // Lost first START receipt.
    assert_eq!(native.fetches.load(Ordering::SeqCst), 1);
    assert_eq!(native.pays.load(Ordering::SeqCst), 1);
    drop(runner);
    let mut ledger = PaymentLedger::reopen(&d.file(), AccountId(account().fingerprint())).unwrap();
    let plan = FoundationPlan::restore(&mut ledger, [1; 16], [2; 16]).unwrap();
    let control = PaymentControl::connect(&config).await.unwrap();
    let mut runner = ClientFoundation::new(ledger, control, Some(wallet), plan).unwrap();
    for _ in 0..5 {
        tick_later(&mut runner).await.unwrap();
    }
    let starts = tor
        .commands
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.starts_with("ELTOR_INITIAL_START"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 2); // One immutable START for each independent hop.
    tor.active.store(true, Ordering::SeqCst);
    let mut active = false;
    for _ in 0..3 {
        active |= matches!(
            tick_later(&mut runner).await.unwrap(),
            FoundationProgress::Active {
                current_round: 1,
                ..
            }
        );
    }
    assert!(active);
    assert_eq!(native.fetches.load(Ordering::SeqCst), 1);
    assert_eq!(native.pays.load(Ordering::SeqCst), 1);
    let record = runner.ledger.get_outgoing(operation).unwrap().unwrap();
    assert!(record.state == OutgoingState::Consumed);
    assert_eq!(record.spent_msat, 2050);
    assert_eq!(record.outstanding_msat, 0);
    server.abort();
    wallet_task.abort();
}

#[tokio::test]
async fn closed_tor_instance_keeps_native_payment_recoverable_without_any_dispatch() {
    let d = Directory::new();
    let req = requirements(true);
    let tor = Arc::new(TorState::default());
    tor.instance.store(1, Ordering::SeqCst);
    let (config, server) = tor_server(req.clone(), tor.clone()).await;
    let (ledger, control, plan) = accepted(&d, &config, &req).await;
    let native = Arc::new(NativeState::default());
    native.lose_pay.store(true, Ordering::SeqCst);
    let (wallet, wallet_task) = wallet_server(&d, tor.clone(), native.clone()).await;
    let mut runner = ClientFoundation::new(ledger, control, Some(wallet.clone()), plan).unwrap();
    for _ in 0..3 {
        tick(&mut runner).await.unwrap();
    }
    let operation = runner.round_operation(0, 1);
    assert!(
        runner
            .ledger
            .get_outgoing(operation)
            .unwrap()
            .unwrap()
            .state
            == OutgoingState::Unknown
    );
    drop(runner);
    let control_count = tor.commands.lock().unwrap().len();
    let mut ledger = PaymentLedger::reopen(&d.file(), AccountId(account().fingerprint())).unwrap();
    ledger.close_instance([1; 16]).unwrap();
    let page = reconcile_origin_financial(&mut ledger, &wallet, None, 16, clock)
        .await
        .unwrap();
    assert_eq!((page.examined, page.reconciled, page.unresolved), (1, 0, 1));
    assert_eq!(page.after, Some(operation));
    assert!(
        ledger
            .get_outgoing(operation)
            .unwrap()
            .unwrap()
            .outstanding_msat
            > 0
    );
    native.settled.store(true, Ordering::SeqCst);
    let page = reconcile_origin_financial(&mut ledger, &wallet, None, 16, || {
        Ok(ClockObservation {
            now_ms: NOW + 1000,
            uncertainty_ms: 0,
        })
    })
    .await
    .unwrap();
    assert_eq!((page.examined, page.reconciled, page.unresolved), (1, 1, 0));
    let record = ledger.get_outgoing(operation).unwrap().unwrap();
    assert!(record.state == OutgoingState::Settled);
    // Native settlement fixes actual cost but does not invent capacity
    // consumption after circuit loss; the prepaid obligation remains visible.
    assert_eq!((record.spent_msat, record.outstanding_msat), (2050, 2050));
    assert_eq!(native.fetches.load(Ordering::SeqCst), 1);
    assert_eq!(native.pays.load(Ordering::SeqCst), 1);
    assert_eq!(tor.commands.lock().unwrap().len(), control_count);
    assert!(
        reconcile_origin_financial(&mut ledger, &wallet, None, 17, clock)
            .await
            .is_err()
    );
    assert!(
        reconcile_origin_financial(&mut ledger, &wallet, None, 0, clock)
            .await
            .is_err()
    );
    server.abort();
    wallet_task.abort();
}

#[path = "foundation_renewal_tests.rs"]
mod renewal;
