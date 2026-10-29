use super::*;
use lni::{
    cln::foundation::{Chain, NativeAccount},
    cln::ClnConfig,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    convert::TryInto,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc, Mutex,
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

// Synthetic fixed-key invoice/preimage for regtest only. Generated from public
// payment_renewal_v1 IDs; never usable against a real wallet or network.
const INVOICE: &str = include_str!("fixtures/received-bolt12.txt");
const NOW: i64 = 1_700_000_101_000;
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
        uncertainty_ms: 1,
    })
}
fn hash() -> [u8; 32] {
    Sha256::digest([42; 32]).into()
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "eltor-relay-foundation-{:032x}",
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
fn vector() -> Value {
    serde_json::from_str::<Value>(include_str!(
        "../../../spec/vectors/payment_renewal_v1.json"
    ))
    .unwrap()["fixtures"][0]
        .clone()
}
fn payment_id() -> String {
    vector()["initial_ids_hex"].as_str().unwrap()[..64].to_owned()
}
fn pending(instance: u8) -> String {
    let f = vector();
    let a = f["periods"][0]["parent_digest_hex"].as_str().unwrap();
    let ids = f["initial_ids_hex"].as_str().unwrap();
    let mut rows = vec![
        format!(
            "250-ELTOR_ADMISSION 1 {} {} 1 0 {} 19000",
            hex::encode([instance; 16]),
            hex::encode([2; 16]),
            hex::encode([3; 32])
        ),
        format!(
            "250-ELTOR_REGISTRATION 1 {} {} 1 0",
            hex::encode([instance; 16]),
            hex::encode([2; 16])
        ),
        format!("250-POLICY {} {a} {a} 1", f["terms_hex"].as_str().unwrap()),
        format!("250-BINDING {}", f["C_hex"].as_str().unwrap()),
        "250-STATE ADMISSION_PENDING 0 0 19000 0".into(),
        "250-CLOCK 11000 -".into(),
        format!("250-FEE 0 {} - NONE", hex::encode([0; 32])),
    ];
    for n in 0..10 {
        rows.push(format!(
            "250-ROUND {} {} 1000 - - - - NONE",
            n + 1,
            &ids[n * 64..(n + 1) * 64]
        ));
    }
    rows.push("250 OK".into());
    rows.join("\n")
}
fn batch(instance: u8) -> BatchRegistration {
    PendingAdmission::parse(&pending(instance), [instance; 16], [2; 16], 0)
        .unwrap()
        .registration()
}
#[derive(Default)]
struct ControlState {
    instance: AtomicU8,
    active: AtomicBool,
    lose_round_receipt: AtomicBool,
    gap: AtomicBool,
    unknown: AtomicBool,
    admitted: AtomicBool,
    renewal_pending: AtomicBool,
    renewal_admitted: AtomicBool,
    unproven_next_authorization: AtomicBool,
    auth: Mutex<BTreeMap<u64, (String, String)>>,
    commands: Mutex<Vec<String>>,
    inventory_count: AtomicU64,
    many_candidates: AtomicBool,
    hints_on_idle: AtomicBool,
    empty_inventory: AtomicBool,
    registration_capacity_once: AtomicBool,
    read_times: Mutex<Vec<(String, Instant)>>,
}
fn registration(state: &ControlState) -> String {
    let instance = state.instance.load(Ordering::SeqCst);
    let mut rows: Vec<_> = pending(instance)
        .split('\n')
        .skip(1)
        .map(str::to_owned)
        .collect();
    let active = state.active.load(Ordering::SeqCst);
    let auth = state.auth.lock().unwrap();
    rows[0] = format!(
        "250-ELTOR_REGISTRATION 1 {} {} 4 0",
        hex::encode([instance; 16]),
        hex::encode([2; 16])
    );
    rows[3] = if active {
        "250-STATE ACTIVE 1 0 0 75000".into()
    } else {
        "250-STATE COMMITTED 0 0 19000 0".into()
    };
    rows[4] = "250-CLOCK 11000 6000".into();
    if let Some((decision, hash)) = auth.get(&0) {
        rows[5] = format!(
            "250-FEE 0 {hash} {decision} {}",
            if active { "CONSUMED" } else { "AUTHORIZED" }
        );
    }
    for n in 0..10 {
        let id = &vector()["initial_ids_hex"].as_str().unwrap().to_owned()[n * 64..(n + 1) * 64];
        let window = if n == 0 {
            "0 19000".to_owned()
        } else if active {
            format!(
                "{} {}",
                (n as u64 * 60000).saturating_sub(60000),
                n as u64 * 60000 + 15000
            )
        } else {
            "- -".into()
        };
        let charge = if let Some((decision, hash)) = auth.get(&(n as u64 + 1)) {
            format!(
                "{decision} {hash} {}",
                if active && n == 0 {
                    "CONSUMED"
                } else {
                    "AUTHORIZED"
                }
            )
        } else {
            "- - NONE".into()
        };
        rows[6 + n] = format!("250-ROUND {} {id} 1000 {window} {charge}", n + 1);
    }
    rows.join("\n")
}
fn renewal_pending(state: &ControlState) -> String {
    let instance = state.instance.load(Ordering::SeqCst);
    let f = vector();
    let renewal = &f["periods"][0];
    let mut rows: Vec<String> = pending(instance).split('\n').map(str::to_owned).collect();
    rows[0] = format!(
        "250-ELTOR_ADMISSION 1 {} {} 4 1 {} 19000",
        hex::encode([instance; 16]),
        hex::encode([2; 16]),
        renewal["request_digest_hex"].as_str().unwrap()
    );
    rows[1] = format!(
        "250-ELTOR_REGISTRATION 1 {} {} 4 1",
        hex::encode([instance; 16]),
        hex::encode([2; 16])
    );
    rows[2] = format!(
        "250-POLICY {} {} {} 11",
        f["terms_hex"].as_str().unwrap(),
        renewal["parent_digest_hex"].as_str().unwrap(),
        renewal["allocation_digest_hex"].as_str().unwrap()
    );
    rows[4] = "250-STATE ADMISSION_PENDING 1 0 0 75000".into();
    rows[5] = "250-CLOCK 11000 6000".into();
    if let Some((decision, hash)) = state.auth.lock().unwrap().get(&0) {
        rows[6] = format!("250-FEE 0 {hash} {decision} CONSUMED");
    }
    let ids = renewal["new_ids_hex"].as_str().unwrap();
    for n in 0..10 {
        rows[7 + n] = format!(
            "250-ROUND {} {} 1000 - - - - NONE",
            11 + n,
            &ids[n * 64..(n + 1) * 64]
        );
    }
    rows.join("\n")
}
fn renewal_registration(state: &ControlState) -> String {
    let mut rows: Vec<String> = renewal_pending(state)
        .split('\n')
        .skip(1)
        .map(str::to_owned)
        .collect();
    rows[3] = "250-STATE ACTIVE 1 0 0 75000".into();
    let f = vector();
    let ids = f["periods"][0]["new_ids_hex"].as_str().unwrap();
    for n in 0..10 {
        let charge = if n == 0 && state.unproven_next_authorization.load(Ordering::SeqCst) {
            format!(
                "{} {} AUTHORIZED",
                hex::encode([99; 16]),
                hex::encode([98; 32])
            )
        } else {
            "- - NONE".into()
        };
        rows[6 + n] = format!(
            "250-ROUND {} {} 1000 {} {} {charge}",
            11 + n,
            &ids[n * 64..(n + 1) * 64],
            (10 + n) * 60000 - 60000,
            (10 + n) * 60000 + 15000
        );
    }
    rows.join("\n")
}
async fn controller(
    path: PathBuf,
    state: Arc<ControlState>,
) -> (crate::types::RpcConfig, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = crate::types::RpcConfig {
        addr: listener.local_addr().unwrap().to_string(),
        rpc_password: None,
        command: String::new(),
    };
    let worker = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let state = state.clone();
            let path = path.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                while let Some(command) = lines.next_line().await.unwrap() {
                    let fields: Vec<_> = command.split(' ').collect();
                    state.commands.lock().unwrap().push(command.clone());
                    if matches!(
                        fields[0],
                        "ELTOR_PAYMENT_ADMISSION_GET" | "ELTOR_PAYMENT_REGISTRATION_GET"
                    ) {
                        state
                            .read_times
                            .lock()
                            .unwrap()
                            .push((fields[3].into(), Instant::now()));
                    }
                    let instance = state.instance.load(Ordering::SeqCst);
                    let i = hex::encode([instance; 16]);
                    let h = hex::encode([2; 16]);
                    let reply = match fields[0] {
                        "AUTHENTICATE" | "SETEVENTS" => "250 OK".into(),
                        "ELTOR_PAYMENT_LIST" => {
                            state.inventory_count.fetch_add(1, Ordering::SeqCst);
                            if state.many_candidates.load(Ordering::SeqCst) {
                                let sequence = if state.hints_on_idle.load(Ordering::SeqCst) {
                                    10
                                } else {
                                    12
                                };
                                let mut rows =
                                    vec![format!("250-ELTOR_PAYMENT_LIST 1 {i} {sequence} -")];
                                if !state.empty_inventory.load(Ordering::SeqCst) {
                                    rows.push(format!("250-CIRCUIT {h} RELAY 1"));
                                    for value in (3..53).chain(100..110) {
                                        rows.push(format!(
                                            "250-CIRCUIT {} RELAY 1",
                                            hex::encode([value; 16])
                                        ));
                                    }
                                }
                                rows.push("250 OK".into());
                                rows.join("\n")
                            } else {
                                format!("250-ELTOR_PAYMENT_LIST 1 {i} 10 -\n250-CIRCUIT {h} RELAY 1\n250 OK")
                            }
                        }
                        "ELTOR_PAYMENT_ADMISSION_GET" => {
                            if fields[3] != h {
                                if state.hints_on_idle.swap(false, Ordering::SeqCst) {
                                    format!("650 ELTOR_PAYMENT_STATE 1 {i} 11 {h} RELAY 2 ADMISSION_PENDING\n650 ELTOR_PAYMENT_STATE 1 {i} 12 {h} RELAY 3 START_REQUESTED\n551 INITIAL_STATE_UNCONFIRMED")
                                } else {
                                    "551 INITIAL_STATE_UNCONFIRMED".into()
                                }
                            } else if fields[4] == "0" && !state.admitted.load(Ordering::SeqCst) {
                                pending(instance)
                            } else if fields[4] == "1"
                                && state.renewal_pending.load(Ordering::SeqCst)
                            {
                                renewal_pending(&state)
                            } else {
                                "552 UNKNOWN_PERIOD".into()
                            }
                        }
                        "ELTOR_PAYMENT_ADMISSION_DECIDE" => {
                            let ledger =
                                PaymentLedger::reopen(&path, AccountId(account().fingerprint()))
                                    .unwrap();
                            assert!(ledger
                                .get_handshake_obligation([instance; 16], [2; 16])
                                .unwrap()
                                .is_some());
                            let reservation = ledger
                                .get_batch_reservation(
                                    [instance; 16],
                                    [2; 16],
                                    fields[4].parse().unwrap(),
                                )
                                .unwrap()
                                .unwrap();
                            assert_eq!(
                                command,
                                AdmissionDecision::from(&reservation).command().unwrap()
                            );
                            if fields[4] == "0" {
                                state.admitted.store(true, Ordering::SeqCst);
                            } else {
                                state.renewal_admitted.store(true, Ordering::SeqCst);
                            }
                            format!(
                                "250 ELTOR_ADMISSION_DECIDED 1 {i} {h} {} {} 2",
                                fields[4], fields[6]
                            )
                        }
                        "ELTOR_PAYMENT_REGISTRATION_GET" => {
                            if state
                                .registration_capacity_once
                                .swap(false, Ordering::SeqCst)
                            {
                                "451 CAPACITY".into()
                            } else if state.unknown.load(Ordering::SeqCst) {
                                "552 UNKNOWN_CIRCUIT".into()
                            } else if fields[4] == "1" {
                                assert!(state.renewal_admitted.load(Ordering::SeqCst));
                                renewal_registration(&state)
                            } else if state.gap.swap(false, Ordering::SeqCst) {
                                format!(
                                    "650 ELTOR_PAYMENT_STATE 1 {i} 12 {h} RELAY 4 AUTHORIZED\n{}",
                                    registration(&state)
                                )
                            } else {
                                registration(&state)
                            }
                        }
                        "ELTOR_PAYMENT_AUTHORIZE" => {
                            let round: u64 = fields[5].parse().unwrap();
                            let mut ledger =
                                PaymentLedger::reopen(&path, AccountId(account().fingerprint()))
                                    .unwrap();
                            let obligation = if round == 0 {
                                ledger
                                    .get_handshake_obligation([instance; 16], [2; 16])
                                    .unwrap()
                                    .unwrap()
                            } else {
                                batch(instance).rounds[(round - 1) as usize].clone()
                            };
                            let authorization =
                                ledger.get_authorization(&obligation).unwrap().unwrap();
                            assert_eq!(fields[4], hex::encode(authorization.decision));
                            assert_eq!(fields[7], hex::encode(authorization.payment_hash));
                            assert_eq!(fields[8], hex::encode(authorization.record_digest));
                            state
                                .auth
                                .lock()
                                .unwrap()
                                .insert(round, (fields[4].into(), fields[7].into()));
                            if round == 1 && state.lose_round_receipt.swap(false, Ordering::SeqCst)
                            {
                                return;
                            }
                            format!("250 ELTOR_AUTHORIZED 1 {i} {h} {} 4", fields[4])
                        }
                        _ => panic!("unexpected controller command type"),
                    };
                    if write
                        .write_all(format!("{}\r\n", reply.replace('\n', "\r\n")).as_bytes())
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            });
        }
    });
    (cfg, worker)
}
#[derive(Default)]
struct NativeState {
    amount_delta: AtomicU64,
    paid_at: AtomicU64,
    advance_clock_on_lookup: AtomicBool,
    clock_now: AtomicU64,
    wrong_account: AtomicBool,
    lookups: AtomicU64,
    pages: AtomicU64,
    calls: Mutex<Vec<String>>,
}
async fn native(state: Arc<NativeState>) -> (ClnConfig, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let worker = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let state = state.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let header;
                loop {
                    let mut buf = [0; 1024];
                    let n = stream.read(&mut buf).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(p) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        header = p + 4;
                        break;
                    }
                    assert!(bytes.len() < 131072);
                }
                let headers = String::from_utf8(bytes[..header].to_vec()).unwrap();
                let path = headers.split_whitespace().nth(1).unwrap().to_owned();
                let len: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < header + len {
                    let mut buf = [0; 1024];
                    let n = stream.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                let body: Value = serde_json::from_slice(&bytes[header..header + len]).unwrap();
                state.calls.lock().unwrap().push(path.clone());
                let mut row = json!({"bolt12":INVOICE.trim(),"payment_hash":hex::encode(hash()),"status":"paid","amount_msat":1000,"amount_received_msat":1000+state.amount_delta.load(Ordering::SeqCst),"paid_at":if state.paid_at.load(Ordering::SeqCst)==0 {1_700_000_100u64}else{state.paid_at.load(Ordering::SeqCst)},"payment_preimage":hex::encode([42;32]),"invreq_payer_note":payment_id(),"created_index":1,"updated_index":3});
                let reply = match path.as_str() {
                    "/v1/getinfo" => {
                        json!({"id":if state.wrong_account.load(Ordering::SeqCst){"0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".into()}else{hex::encode(account().node_id)},"network":"regtest"})
                    }
                    "/v1/help" => {
                        json!({"help":[{"command":"listinvoices [payment_hash] [index] [start] [limit]"}]})
                    }
                    "/v1/listinvoices" => {
                        if body.get("index").is_some() {
                            state.pages.fetch_add(1, Ordering::SeqCst);
                            assert_eq!(body["index"], "updated");
                            assert_eq!(body["limit"], 16);
                            if body["start"] == 1 {
                                json!({"invoices":[row]})
                            } else {
                                assert_eq!(body["start"], 4);
                                json!({"invoices":[]})
                            }
                        } else {
                            state.lookups.fetch_add(1, Ordering::SeqCst);
                            if state.advance_clock_on_lookup.load(Ordering::SeqCst) {
                                state
                                    .clock_now
                                    .store((NOW + 30_000) as u64, Ordering::SeqCst);
                            }
                            assert_eq!(body, json!({"payment_hash":hex::encode(hash())}));
                            row.as_object_mut().unwrap().remove("created_index");
                            row.as_object_mut().unwrap().remove("updated_index");
                            json!({"invoices":[row]})
                        }
                    }
                    _ => panic!("relay attempted a native non-recipient method"),
                };
                let text = reply.to_string();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{text}",text.len()).as_bytes()).await.unwrap();
            });
        }
    });
    (
        ClnConfig {
            url,
            rune: "fixture-only-rune".into(),
            http_timeout: Some(2),
            ..Default::default()
        },
        worker,
    )
}
async fn setup(
    dir: &Directory,
    state: Arc<ControlState>,
    native_state: Arc<NativeState>,
) -> (
    RelayFoundation,
    crate::types::RpcConfig,
    ClnConfig,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
) {
    state.instance.store(1, Ordering::SeqCst);
    let ledger = PaymentLedger::provision(&dir.file(), AccountId(account().fingerprint())).unwrap();
    let (config, controller) = controller(dir.file(), state).await;
    let (native_config, native) = native(native_state).await;
    let wallet = ClnFoundation::connect_receiving(native_config.clone(), account())
        .await
        .unwrap();
    let control = PaymentControl::connect(&config).await.unwrap();
    (
        RelayFoundation::new(ledger, wallet, control).unwrap(),
        config,
        native_config,
        controller,
        native,
    )
}
#[tokio::test]
async fn actionable_work_preempts_idle_candidates_without_resurrecting_closed_history() {
    let dir = Directory::new();
    let state = Arc::new(ControlState::default());
    state.many_candidates.store(true, Ordering::SeqCst);
    state.hints_on_idle.store(true, Ordering::SeqCst);
    state
        .registration_capacity_once
        .store(true, Ordering::SeqCst);
    let n = Arc::new(NativeState::default());
    let (mut coordinator, _, _, controller, native) = setup(&dir, state.clone(), n.clone()).await;
    for value in 100..110 {
        coordinator
            .ledger
            .close_binding([1; 16], [value; 16])
            .unwrap();
    }
    assert_eq!(coordinator.step(clock).await.unwrap(), RelayStep::Recovered);
    assert_eq!(coordinator.live.len(), 51);
    // Start behind the first ordinary candidate: its real socket read receives
    // pending admission/START hints for the financially relevant handle.
    coordinator.next = 1;
    assert_eq!(coordinator.step(clock).await.unwrap(), RelayStep::Deferred);
    assert_eq!(coordinator.probed.get(&[3; 16]), Some(&1));
    assert_eq!(coordinator.step(clock).await.unwrap(), RelayStep::Deferred);
    assert!(state.admitted.load(Ordering::SeqCst));
    assert!(coordinator.financial.contains(&[2; 16]));
    assert_eq!(
        coordinator.step(clock).await.unwrap(),
        RelayStep::Reconciled {
            notifications: 2,
            consumed: 0,
        }
    );
    // The legitimate shared per-hop floor is enforced across admission and
    // registration reads, including the CAPACITY retry, without scanning50rows.
    let read_times = state.read_times.lock().unwrap();
    let reads: Vec<_> = read_times
        .iter()
        .filter(|(h, _)| *h == hex::encode([2; 16]))
        .collect();
    assert_eq!(reads.len(), 3);
    for pair in reads.windows(2) {
        assert!(pair[1].1.duration_since(pair[0].1) >= Duration::from_millis(1100));
    }
    assert_eq!(read_times.len(), 4);
    drop(read_times);
    assert_eq!(n.lookups.load(Ordering::SeqCst), 1);
    assert!(coordinator.live.keys().all(|h| h[0] < 100));
    // A stale same-revision hint cannot reawaken a dormant ordinary candidate.
    let old = PaymentEvent {
        instance: [1; 16],
        sequence: 10,
        handle: [3; 16],
        role: PaymentRole::Relay,
        revision: 1,
        kind: PaymentEventKind::StartRequested,
    };
    coordinator.schedule_hints(&[old]).unwrap();
    assert!(!coordinator.actionable.contains(&[3; 16]));
    // Bounded urgent bursts still admit an ordinary background probe.
    coordinator.actionable_streak = 0;
    for _ in 0..ACTIONABLE_BURST {
        assert_eq!(coordinator.next_work().unwrap().handle, [2; 16]);
        coordinator.schedule_actionable([2; 16]).unwrap();
    }
    assert_ne!(coordinator.next_work().unwrap().handle, [2; 16]);
    // A fresh same-instance inventory replaces ordinary work; absence is not a
    // financial close. The exact admitted binding remains for a terminal query.
    state.empty_inventory.store(true, Ordering::SeqCst);
    coordinator.synchronize().await.unwrap();
    assert_eq!(coordinator.live.len(), 1);
    assert!(coordinator.live.contains_key(&[2; 16]));
    assert!(!coordinator
        .ledger
        .binding_is_closed([1; 16], [2; 16])
        .unwrap());
    coordinator.drain_hints().unwrap();
    assert_eq!(coordinator.live.len(), 1);
    // Dormant work still performs an actual inventory read to receive events.
    coordinator.live.clear();
    coordinator.financial.clear();
    coordinator.actionable.clear();
    coordinator.last_inventory = Instant::now() - Duration::from_secs(2);
    let inventories = state.inventory_count.load(Ordering::SeqCst);
    assert_eq!(coordinator.step(clock).await.unwrap(), RelayStep::Idle);
    assert!(state.inventory_count.load(Ordering::SeqCst) > inventories);
    controller.abort();
    native.abort();
}

#[tokio::test]
async fn recipient_discovery_authorization_and_exact_consumption_run_over_local_sockets() {
    let dir = Directory::new();
    let state = Arc::new(ControlState::default());
    let n = Arc::new(NativeState::default());
    let (mut coordinator, _, _, controller, native) = setup(&dir, state.clone(), n.clone()).await;
    assert_eq!(coordinator.step(clock).await.unwrap(), RelayStep::Recovered);
    assert_eq!(
        coordinator.step(clock).await.unwrap(),
        RelayStep::Reconciled {
            notifications: 2,
            consumed: 0
        }
    );
    let first = batch(1).rounds[0].clone();
    assert!(
        coordinator
            .ledger
            .get_authorization(&first)
            .unwrap()
            .unwrap()
            .state
            == AuthorizationState::Authorized
    );
    assert_eq!(n.lookups.load(Ordering::SeqCst), 1);
    state.active.store(true, Ordering::SeqCst);
    assert_eq!(
        coordinator.step(clock).await.unwrap(),
        RelayStep::Reconciled {
            notifications: 0,
            consumed: 2
        }
    );
    assert!(
        coordinator
            .ledger
            .get_authorization(&first)
            .unwrap()
            .unwrap()
            .state
            == AuthorizationState::Consumed
    );
    assert_eq!(coordinator.ledger.native_invoice_cursor().unwrap(), 4);
    assert_eq!(n.lookups.load(Ordering::SeqCst), 1);
    assert!(n
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|c| matches!(c.as_str(), "/v1/getinfo" | "/v1/help" | "/v1/listinvoices")));
    controller.abort();
    native.abort();
}

#[tokio::test]
async fn lost_authorization_receipt_reopens_exact_ledger_without_another_claim_or_scan_reset() {
    let dir = Directory::new();
    let state = Arc::new(ControlState::default());
    let n = Arc::new(NativeState::default());
    state.lose_round_receipt.store(true, Ordering::SeqCst);
    let (mut coordinator, cfg, native_cfg, controller, native) =
        setup(&dir, state.clone(), n.clone()).await;
    coordinator.step(clock).await.unwrap();
    assert!(coordinator.step(clock).await.is_err());
    let first = batch(1).rounds[0].clone();
    let saved = coordinator
        .ledger
        .get_authorization(&first)
        .unwrap()
        .unwrap();
    assert!(saved.state == AuthorizationState::Authorized);
    assert_eq!(coordinator.ledger.native_invoice_cursor().unwrap(), 4);
    drop(coordinator);
    state.active.store(true, Ordering::SeqCst);
    let ledger = PaymentLedger::reopen(&dir.file(), AccountId(account().fingerprint())).unwrap();
    let wallet = ClnFoundation::connect_receiving(native_cfg, account())
        .await
        .unwrap();
    let control = PaymentControl::connect(&cfg).await.unwrap();
    let mut recovered = RelayFoundation::new(ledger, wallet, control).unwrap();
    recovered.step(clock).await.unwrap();
    let result = recovered.step(clock).await.unwrap();
    assert_eq!(
        result,
        RelayStep::Reconciled {
            notifications: 0,
            consumed: 2
        }
    );
    let after = recovered.ledger.get_authorization(&first).unwrap().unwrap();
    assert!(
        after.decision == saved.decision
            && after.payment_hash == saved.payment_hash
            && after.state == AuthorizationState::Consumed
    );
    assert_eq!(n.lookups.load(Ordering::SeqCst), 1);
    assert_eq!(
        state
            .commands
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("ELTOR_PAYMENT_ADMISSION_GET") && c.ends_with(" 0"))
            .count(),
        1
    );
    assert_eq!(
        state
            .commands
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with("ELTOR_PAYMENT_AUTHORIZE"))
            .count(),
        2
    );
    controller.abort();
    native.abort();
}

#[tokio::test]
async fn control_event_gap_requires_inventory_before_any_native_claim_or_notification() {
    let dir = Directory::new();
    let state = Arc::new(ControlState::default());
    let n = Arc::new(NativeState::default());
    let (mut coordinator, _, _, controller, native) = setup(&dir, state.clone(), n.clone()).await;
    coordinator.step(clock).await.unwrap();
    state.gap.store(true, Ordering::SeqCst);
    assert!(coordinator.step(clock).await.is_err());
    assert!(state.auth.lock().unwrap().is_empty());
    assert_eq!(n.pages.load(Ordering::SeqCst), 0);
    assert!(coordinator.recovery.is_none());
    assert_eq!(coordinator.step(clock).await.unwrap(), RelayStep::Recovered);
    assert_eq!(state.inventory_count.load(Ordering::SeqCst), 2);
    assert_eq!(
        coordinator.step(clock).await.unwrap(),
        RelayStep::Reconciled {
            notifications: 2,
            consumed: 0
        }
    );
    controller.abort();
    native.abort();
}

#[tokio::test]
async fn wrong_received_msat_account_and_original_settlement_window_never_authorize_round() {
    for fault in 0..4 {
        let dir = Directory::new();
        let state = Arc::new(ControlState::default());
        let n = Arc::new(NativeState::default());
        let (mut coordinator, _, _, controller, native) =
            setup(&dir, state.clone(), n.clone()).await;
        coordinator.step(clock).await.unwrap();
        match fault {
            0 => n.amount_delta.store(1, Ordering::SeqCst),
            1 => n.wrong_account.store(true, Ordering::SeqCst),
            2 => n.paid_at.store(1_700_000_094, Ordering::SeqCst),
            _ => {
                n.advance_clock_on_lookup.store(true, Ordering::SeqCst);
                n.clock_now.store(NOW as u64, Ordering::SeqCst);
            }
        }
        let outcome = coordinator
            .step(|| {
                let now = n.clock_now.load(Ordering::SeqCst);
                Ok(ClockObservation {
                    now_ms: if now == 0 { NOW } else { now as i64 },
                    uncertainty_ms: 1,
                })
            })
            .await;
        if fault < 2 {
            assert!(outcome.is_err());
        } else {
            assert_eq!(
                outcome.unwrap(),
                RelayStep::Reconciled {
                    notifications: 1,
                    consumed: 0
                }
            );
        }
        assert!(coordinator
            .ledger
            .get_authorization(&batch(1).rounds[0])
            .unwrap()
            .is_none());
        assert!(!state.auth.lock().unwrap().contains_key(&1));
        controller.abort();
        native.abort();
    }
}

#[tokio::test]
async fn unknown_handle_and_new_tor_instance_close_live_bindings_without_erasing_financial_history()
{
    let dir = Directory::new();
    let state = Arc::new(ControlState::default());
    let n = Arc::new(NativeState::default());
    let (mut coordinator, cfg, _, controller, native) = setup(&dir, state.clone(), n.clone()).await;
    coordinator.step(clock).await.unwrap();
    coordinator.step(clock).await.unwrap();
    let first = batch(1).rounds[0].clone();
    let saved = coordinator
        .ledger
        .get_authorization(&first)
        .unwrap()
        .unwrap();
    state.unknown.store(true, Ordering::SeqCst);
    assert_eq!(coordinator.step(clock).await.unwrap(), RelayStep::Deferred);
    assert!(coordinator
        .ledger
        .binding_is_closed([1; 16], [2; 16])
        .unwrap());
    let closed = coordinator
        .ledger
        .get_authorization(&first)
        .unwrap()
        .unwrap();
    assert!(
        closed.decision == saved.decision && closed.state == AuthorizationState::ClosedUnresolved
    );
    assert!(coordinator
        .ledger
        .incoming_candidate(first.payment_id)
        .unwrap()
        .is_some());
    state.instance.store(3, Ordering::SeqCst);
    state.unknown.store(false, Ordering::SeqCst);
    coordinator.reconnect(PaymentControl::connect(&cfg).await.unwrap());
    assert_eq!(coordinator.step(clock).await.unwrap(), RelayStep::Recovered);
    assert!(coordinator
        .ledger
        .binding_is_closed([1; 16], [2; 16])
        .unwrap());
    assert_eq!(coordinator.ledger.native_invoice_cursor().unwrap(), 4);
    assert!(
        coordinator
            .ledger
            .get_authorization(&first)
            .unwrap()
            .unwrap()
            .state
            == AuthorizationState::ClosedUnresolved
    );
    controller.abort();
    native.abort();
}

#[tokio::test]
async fn current_and_admitted_next_snapshots_preflight_together_before_consumption() {
    for invalid_next in [false, true] {
        let dir = Directory::new();
        let state = Arc::new(ControlState::default());
        let n = Arc::new(NativeState::default());
        let (mut coordinator, _, _, controller, native) =
            setup(&dir, state.clone(), n.clone()).await;
        coordinator.step(clock).await.unwrap();
        coordinator.step(clock).await.unwrap();
        let first = batch(1).rounds[0].clone();
        let original_fee = coordinator
            .ledger
            .get_handshake_obligation([1; 16], [2; 16])
            .unwrap()
            .unwrap();
        state.active.store(true, Ordering::SeqCst);
        state.renewal_pending.store(true, Ordering::SeqCst);
        // Exercise the same durable admission API over the persistent socket,
        // before current consumption has been reconciled. No synthetic proof.
        coordinator
            .control
            .admit_pending(&mut coordinator.ledger, [1; 16], [2; 16], 1, clock)
            .await
            .unwrap();
        assert!(
            coordinator
                .ledger
                .get_handshake_obligation([1; 16], [2; 16])
                .unwrap()
                == Some(original_fee.clone())
        );
        let next = coordinator
            .ledger
            .get_reserved_batch_registration([1; 16], [2; 16], 1)
            .unwrap()
            .unwrap();
        assert_eq!(next.rounds[0].round, 11);
        assert_eq!(next.rounds[0].allocation_hash, first.allocation_hash);
        state
            .unproven_next_authorization
            .store(invalid_next, Ordering::SeqCst);
        let result = coordinator.step(clock).await;
        if invalid_next {
            assert!(result.is_err());
            assert!(
                coordinator
                    .ledger
                    .get_authorization(&first)
                    .unwrap()
                    .unwrap()
                    .state
                    == AuthorizationState::Authorized
            );
            assert!(
                coordinator
                    .ledger
                    .get_authorization(&original_fee)
                    .unwrap()
                    .unwrap()
                    .state
                    == AuthorizationState::Authorized
            );
            assert_eq!(n.pages.load(Ordering::SeqCst), 1);
        } else {
            assert_eq!(
                result.unwrap(),
                RelayStep::Reconciled {
                    notifications: 0,
                    consumed: 2
                }
            );
            assert!(
                coordinator
                    .ledger
                    .get_authorization(&first)
                    .unwrap()
                    .unwrap()
                    .state
                    == AuthorizationState::Consumed
            );
        }
        assert_eq!(n.lookups.load(Ordering::SeqCst), 1);
        let commands = state.commands.lock().unwrap();
        assert!(commands
            .iter()
            .any(|c| c.starts_with("ELTOR_PAYMENT_REGISTRATION_GET") && c.ends_with(" 1")));
        assert!(commands
            .iter()
            .any(|c| c.starts_with("ELTOR_PAYMENT_REGISTRATION_GET") && c.ends_with(" 0")));
        controller.abort();
        native.abort();
    }
}

#[tokio::test]
async fn durable_initial_identifier_conflict_refuses_and_closes_only_its_binding() {
    let dir = Directory::new();
    let state = Arc::new(ControlState::default());
    let n = Arc::new(NativeState::default());
    let (mut coordinator, _, _, controller, native) = setup(&dir, state.clone(), n.clone()).await;
    let mut other = batch(1);
    for r in &mut other.rounds {
        r.instance = [1; 16];
        r.handle = [8; 16];
        r.circuit_binding = [9; 32];
    }
    coordinator.ledger.reserve_batch(&other, [10; 16]).unwrap();
    coordinator.step(clock).await.unwrap();
    assert_eq!(coordinator.step(clock).await.unwrap(), RelayStep::Deferred);
    let refusal = coordinator
        .ledger
        .get_batch_reservation([1; 16], [2; 16], 0)
        .unwrap()
        .unwrap();
    assert!(refusal.outcome == AdmissionOutcome::Conflict);
    assert!(coordinator
        .ledger
        .binding_is_closed([1; 16], [2; 16])
        .unwrap());
    assert!(!coordinator
        .ledger
        .binding_is_closed([1; 16], [8; 16])
        .unwrap());
    assert!(
        coordinator
            .ledger
            .get_reserved_batch_registration([1; 16], [8; 16], 0)
            .unwrap()
            == Some(other)
    );
    assert!(state.auth.lock().unwrap().is_empty());
    assert_eq!(n.pages.load(Ordering::SeqCst), 0);
    controller.abort();
    native.abort();
}
