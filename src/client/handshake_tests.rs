use super::*;
use lni::cln::{
    foundation::{Chain, NativeAccount},
    ClnConfig,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
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
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p =
            std::env::temp_dir().join(format!("eltor-prefunding-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self) -> PathBuf {
        self.0.join("account.sqlite")
    }
    fn ledger(&self) -> PaymentLedger {
        let mut l =
            PaymentLedger::provision(&self.file(), AccountId(account().fingerprint())).unwrap();
        l.configure_account_budget(crate::payment_ledger::outgoing::AccountBudget {
            lifetime_limit_msat: 10000,
            outstanding_limit_msat: 10000,
            max_queued_intents: 16,
            max_wallet_rpcs: 4,
            max_unresolved_intents: 32,
        })
        .unwrap();
        l.save_handshake_scope(&scope()).unwrap();
        l
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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
        account: AccountId(account().fingerprint()),
        lifetime_limit_msat: 10000,
        outstanding_limit_msat: 10000,
        per_round_limit_msat: 5000,
        window: AcceptanceWindow {
            opens_at_ms: NOW - 10000,
            closes_at_ms: NOW + 10000,
        },
        hops: vec![HandshakeHop {
            fingerprint: [4; 20],
            terms,
            offer: Some(OFFER.trim().into()),
            operation: [5; 16],
            payment_id: [7; 32],
            initial_payment_ids: std::array::from_fn(|i| [20 + i as u8; 32]),
            fee_limit_msat: 100,
        }],
    }
}
fn clock() -> io::Result<ClockObservation> {
    Ok(ClockObservation {
        now_ms: NOW + 2000,
        uncertainty_ms: 0,
    })
}
#[derive(Default)]
struct State {
    pays: AtomicUsize,
    fetches: AtomicUsize,
    lookups: AtomicUsize,
    lose_pay: AtomicBool,
    lose_fetch: AtomicBool,
    unknown: AtomicBool,
    wrong_label: AtomicBool,
    wrong_account: AtomicBool,
    fee_breach: AtomicBool,
}
async fn wallet(
    directory: &Directory,
    state: Arc<State>,
) -> (ClnFoundation, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = ClnConfig {
        url: format!("http://{}/", listener.local_addr().unwrap()),
        rune: "local-fixture-only".into(),
        http_timeout: Some(2),
        ..Default::default()
    };
    let path = directory.file();
    let worker = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let state = state.clone();
            let path = path.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut b = [0; 1024];
                    let n = socket.read(&mut b).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&b[..n]);
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
                            .map(|n| n.trim().parse().unwrap())
                    })
                    .unwrap();
                while bytes.len() < header_end + length {
                    let mut b = [0; 1024];
                    let n = socket.read(&mut b).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&b[..n]);
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
                        json!({"id":hex::encode(if state.wrong_account.load(Ordering::SeqCst){[2;33]}else{account().node_id}),"network":"regtest","version":"v25.09"})
                    }
                    "help" => {
                        json!({"help":[{"command":match args["command"].as_str().unwrap(){"pay"=>"pay bolt11 [label] [maxfee]","listpays"=>"listpays [payment_hash]","fetchinvoice"=>"fetchinvoice offer [amount_msat] [payer_note]",_=>panic!("unexpected help")}}]})
                    }
                    "fetchinvoice" => {
                        assert_eq!(args["offer"], OFFER.trim());
                        assert_eq!(args["payer_note"], hex::encode([7; 32]));
                        assert_eq!(args["amount_msat"], "2000msat");
                        assert_eq!(state.fetches.fetch_add(1, Ordering::SeqCst), 0);
                        if state.lose_fetch.load(Ordering::SeqCst) {
                            return;
                        }
                        json!({"invoice":INVOICE.trim()})
                    }
                    "pay" => {
                        let ledger =
                            PaymentLedger::reopen(&path, AccountId(account().fingerprint()))
                                .unwrap();
                        let saved = ledger.get_handshake([2; 32], 1).unwrap().unwrap();
                        assert!(saved.state == OutgoingState::InFlight);
                        assert_eq!(saved.invoice.artifact, INVOICE.trim());
                        assert_eq!(ledger.account_exposure().unwrap().reserved_msat, 2100);
                        assert_eq!(args["maxfee"], "100msat");
                        assert_eq!(args["label"], hex::encode([5; 16]));
                        assert_eq!(args["bolt11"], INVOICE.trim());
                        assert_eq!(state.pays.fetch_add(1, Ordering::SeqCst), 0);
                        if state.lose_pay.load(Ordering::SeqCst) {
                            return;
                        }
                        json!({"status":"complete"})
                    }
                    "listpays" => {
                        state.lookups.fetch_add(1, Ordering::SeqCst);
                        if state.pays.load(Ordering::SeqCst) == 0
                            || state.unknown.load(Ordering::SeqCst)
                        {
                            json!({"pays":[]})
                        } else {
                            json!({"pays":[{"payment_hash":hex::encode(Sha256::digest([42;32])),"label":hex::encode(if state.wrong_label.load(Ordering::SeqCst){[6;16]}else{[5;16]}),"bolt12":INVOICE.trim(),"status":"complete","created_at":1_700_000_000u64,"completed_at":1_700_000_100u64,"amount_msat":2000,"amount_sent_msat":if state.fee_breach.load(Ordering::SeqCst){2200}else{2050},"preimage":hex::encode([42;32])}]})
                        }
                    }
                    _ => panic!("unexpected native method"),
                };
                let body = serde_json::to_vec(&reply).unwrap();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",body.len()).as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            });
        }
    });
    let wallet = ClnFoundation::connect(config, account()).await.unwrap();
    (wallet, worker)
}
#[tokio::test]
async fn lost_pay_restarts_with_exact_lookup_only_then_yields_ephemeral_bound_proof() {
    let d = Directory::new();
    let mut l = d.ledger();
    let state = Arc::new(State::default());
    state.lose_pay.store(true, Ordering::SeqCst);
    let (w, server) = wallet(&d, state.clone()).await;
    assert!(matches!(
        fund_handshake_hop(&mut l, &w, [2; 32], 1, &mut clock).await,
        Err(HandshakeError::Uncertain)
    ));
    drop(l);
    let mut l = PaymentLedger::reopen(&d.file(), AccountId(account().fingerprint())).unwrap();
    state.unknown.store(true, Ordering::SeqCst);
    let unresolved = fund_handshake_hop(&mut l, &w, [2; 32], 1, &mut clock)
        .await
        .unwrap();
    assert!(unresolved.state == OutgoingState::Unknown);
    assert_eq!(unresolved.reserved_msat, 2100);
    state.unknown.store(false, Ordering::SeqCst);
    state.wrong_label.store(true, Ordering::SeqCst);
    assert!(
        handshake_construction_proof(&mut l, &w, [2; 32], 1, clock().unwrap(), &mut clock)
            .await
            .is_err()
    );
    state.wrong_label.store(false, Ordering::SeqCst);
    let page = reconcile_handshake_financial(&mut l, &w, None, 1, || Ok(clock()?))
        .await
        .unwrap();
    assert_eq!(page.examined, 1);
    assert_eq!(page.reconciled, 1);
    assert_eq!(page.unresolved, 0);
    assert_eq!(page.after, Some([5; 16]));
    let settled = l.get_handshake([2; 32], 1).unwrap().unwrap();
    assert_eq!(settled.spent_msat, 2050);
    let proof = handshake_construction_proof(&mut l, &w, [2; 32], 1, clock().unwrap(), &mut clock)
        .await
        .unwrap();
    assert_eq!(proof.preimage(), &[42; 32]);
    drop(proof);
    let late = ClockObservation {
        now_ms: NOW + 300001,
        uncertainty_ms: 0,
    };
    assert!(matches!(
        handshake_construction_proof(&mut l, &w, [2; 32], 1, late, &mut clock).await,
        Err(HandshakeError::OutsideWindow)
    ));
    assert_eq!(state.fetches.load(Ordering::SeqCst), 1);
    assert_eq!(state.pays.load(Ordering::SeqCst), 1);
    // No preimage is retained in financial state or construction staging.
    assert!(!serde_json::to_string(&settled.evidence)
        .unwrap()
        .contains(&hex::encode([42; 32])));
    server.abort();
}
#[tokio::test]
async fn lost_invoice_reply_is_bounded_and_never_refetched_or_paid_after_restart() {
    let d = Directory::new();
    let mut l = d.ledger();
    let state = Arc::new(State::default());
    state.lose_fetch.store(true, Ordering::SeqCst);
    let (w, server) = wallet(&d, state.clone()).await;
    assert!(fund_handshake_hop(&mut l, &w, [2; 32], 1, &mut clock)
        .await
        .is_err());
    drop(l);
    let mut l = PaymentLedger::reopen(&d.file(), AccountId(account().fingerprint())).unwrap();
    assert!(fund_handshake_hop(&mut l, &w, [2; 32], 1, &mut clock)
        .await
        .is_err());
    assert_eq!(state.fetches.load(Ordering::SeqCst), 1);
    assert_eq!(state.pays.load(Ordering::SeqCst), 0);
    assert_eq!(l.account_exposure().unwrap().reserved_msat, 0);
    server.abort();
}
#[tokio::test]
async fn native_account_change_and_actual_fee_breach_never_supply_constructor_proof() {
    let d = Directory::new();
    let mut l = d.ledger();
    let state = Arc::new(State::default());
    let (w, server) = wallet(&d, state.clone()).await;
    state.wrong_account.store(true, Ordering::SeqCst);
    assert!(fund_handshake_hop(&mut l, &w, [2; 32], 1, &mut clock)
        .await
        .is_err());
    assert_eq!(state.pays.load(Ordering::SeqCst), 0);
    server.abort();
    let d = Directory::new();
    let mut l = d.ledger();
    let state = Arc::new(State::default());
    state.fee_breach.store(true, Ordering::SeqCst);
    let (w, server) = wallet(&d, state.clone()).await;
    let r = fund_handshake_hop(&mut l, &w, [2; 32], 1, &mut clock)
        .await
        .unwrap();
    assert_eq!(r.spent_msat, 2200);
    assert!(l.outgoing_admission_blocked().unwrap());
    assert!(
        handshake_construction_proof(&mut l, &w, [2; 32], 1, clock().unwrap(), &mut clock)
            .await
            .is_err()
    );
    server.abort();
}
