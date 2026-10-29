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
    auth: Mutex<BTreeMap<u64, (String, String)>>,
    commands: Mutex<Vec<String>>,
    inventory_count: AtomicU64,
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
                    let instance = state.instance.load(Ordering::SeqCst);
                    let i = hex::encode([instance; 16]);
                    let h = hex::encode([2; 16]);
                    let reply = match fields[0] {
                        "AUTHENTICATE" | "SETEVENTS" => "250 OK".into(),
                        "ELTOR_PAYMENT_LIST" => {
                            state.inventory_count.fetch_add(1, Ordering::SeqCst);
                            format!("250-ELTOR_PAYMENT_LIST 1 {i} 10 -\n250-CIRCUIT {h} RELAY 1\n250 OK")
                        }
                        "ELTOR_PAYMENT_ADMISSION_GET" => {
                            if fields[4] == "0" && !state.admitted.load(Ordering::SeqCst) {
                                pending(instance)
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
                                .get_batch_reservation([instance; 16], [2; 16], 0)
                                .unwrap()
                                .unwrap();
                            assert_eq!(
                                command,
                                AdmissionDecision::from(&reservation).command().unwrap()
                            );
                            state.admitted.store(true, Ordering::SeqCst);
                            format!("250 ELTOR_ADMISSION_DECIDED 1 {i} {h} 0 {} 2", fields[6])
                        }
                        "ELTOR_PAYMENT_REGISTRATION_GET" => {
                            if state.unknown.load(Ordering::SeqCst) {
                                "552 UNKNOWN_CIRCUIT".into()
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
