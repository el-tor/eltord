use super::*;
use lni::cln::foundation::{Chain, NativeAccount, NativeSettlement};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("eltor-wallet-{:032x}", rand::random::<u128>()));
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
fn native_account() -> NativeAccount {
    NativeAccount {
        node_id: hex::decode("0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap()
            .try_into()
            .unwrap(),
        chain: Chain::Regtest,
    }
}
fn account() -> AccountId {
    AccountId(native_account().fingerprint())
}
fn clock() -> ClockObservation {
    ClockObservation {
        now_ms: 150_000,
        uncertainty_ms: 0,
    }
}
fn intent() -> OutgoingIntent {
    OutgoingIntent {
        operation: [1; 16],
        account: account(),
        instance: [2; 16],
        handle: [3; 16],
        local_budget_scope: [4; 32],
        round: 1,
        kind: ChargeKind::Round,
        payment_id: [5; 32],
        charge_reference: [6; 32],
        payee_reference: [7; 32],
        accepted_terms_digest: [8; 32],
        hop_circuit_binding: [9; 32],
        hop_policy_hash: [10; 32],
        invoice_digest: [11; 32],
        payment_hash: [12; 32],
        amount_msat: 2_000,
        fee_limit_msat: 100,
        window: AcceptanceWindow {
            opens_at_ms: 100_000,
            closes_at_ms: 200_000,
        },
    }
}
fn ledger(dir: &Directory) -> PaymentLedger {
    ledger_for(dir, &intent(), clock())
}
fn ledger_for(dir: &Directory, i: &OutgoingIntent, at: ClockObservation) -> PaymentLedger {
    let mut ledger = PaymentLedger::provision(&dir.file(), account()).unwrap();
    ledger
        .configure_account_budget(AccountBudget {
            lifetime_limit_msat: 10_000,
            outstanding_limit_msat: 10_000,
            max_queued_intents: 128,
            max_wallet_rpcs: 32,
            max_unresolved_intents: 100,
        })
        .unwrap();
    ledger
        .configure_path_budget(&PathBudget {
            instance: i.instance,
            handle: i.handle,
            local_budget_scope: i.local_budget_scope,
            accepted_terms_digest: i.accepted_terms_digest,
            lifetime_limit_msat: 10_000,
            outstanding_limit_msat: 10_000,
            per_round_limit_msat: 10_000,
            funding: FundingSource::OwnWallet,
        })
        .unwrap();
    ledger
        .configure_hop_spending_terms(&HopSpendingTerms {
            local_budget_scope: i.local_budget_scope,
            charge_reference: i.charge_reference,
            hop_circuit_binding: i.hop_circuit_binding,
            hop_policy_hash: i.hop_policy_hash,
            payee_reference: i.payee_reference,
            handshake_amount_msat: 50,
            round_amount_msat: i.amount_msat,
            fee_limit_msat: i.fee_limit_msat,
        })
        .unwrap();
    ledger.prepare_outgoing(i, at).unwrap();
    ledger
}
fn settled() -> PaymentStatus {
    let i = intent();
    PaymentStatus::Settled(NativeSettlement {
        account: native_account(),
        operation: i.operation,
        invoice_digest: i.invoice_digest,
        payment_hash: i.payment_hash,
        amount_msat: i.amount_msat,
        fee_msat: 50,
        settled_earliest_ms: 150_000,
        settled_latest_ms: 150_999,
    })
}
struct FixtureWallet {
    path: PathBuf,
    pays: AtomicUsize,
    lookups: AtomicUsize,
    status: Mutex<VecDeque<Option<PaymentStatus>>>,
    account: AccountId,
    matches: bool,
    can_dispatch: bool,
}
impl FixtureWallet {
    fn new(dir: &Directory, status: Vec<Option<PaymentStatus>>) -> Self {
        Self {
            path: dir.file(),
            pays: AtomicUsize::new(0),
            lookups: AtomicUsize::new(0),
            status: Mutex::new(status.into()),
            account: account(),
            matches: true,
            can_dispatch: true,
        }
    }
    fn next(&self) -> std::result::Result<PaymentStatus, AdapterError> {
        self.status
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected wallet call")
            .ok_or(AdapterError::Uncertain)
    }
}
#[async_trait]
impl Wallet for FixtureWallet {
    type Invoice = ();
    fn account(&self) -> AccountId {
        self.account
    }
    fn matches(&self, _: &OutgoingIntent, _: &()) -> bool {
        self.matches
    }
    fn validate_dispatch(&self, _: &OutgoingIntent, _: &(), _: ClockObservation) -> bool {
        self.can_dispatch
    }
    async fn pay(
        &self,
        i: &OutgoingIntent,
        _: &(),
        _: ClockObservation,
    ) -> std::result::Result<PaymentStatus, AdapterError> {
        // A separate connection must see the durable claim before native pay.
        let reopened = PaymentLedger::reopen(&self.path, account()).unwrap();
        let record = reopened.get_outgoing(i.operation).unwrap().unwrap();
        assert!(record.state == OutgoingState::InFlight);
        assert!(record.reserved_msat == 2_100);
        self.pays.fetch_add(1, Ordering::SeqCst);
        self.next()
    }
    async fn lookup(
        &self,
        _: &OutgoingIntent,
        _: &(),
        _: ClockObservation,
    ) -> std::result::Result<PaymentStatus, AdapterError> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        self.next()
    }
}

#[tokio::test]
async fn durable_claim_precedes_native_pay_and_native_msat_cost_is_recorded() {
    let dir = Directory::new();
    let mut ledger = ledger(&dir);
    let wallet = FixtureWallet::new(&dir, vec![Some(settled())]);
    let result = dispatch(&mut ledger, &wallet, intent().operation, &(), clock())
        .await
        .unwrap();
    assert!(result.evidence == EvidenceStatus::PaidWithinWindow);
    assert!(result.record.state == OutgoingState::Settled);
    assert!(result.record.spent_msat == 2_050 && result.record.reserved_msat == 0);
    assert!(wallet.pays.load(Ordering::SeqCst) == 1);
    assert!(
        dispatch(&mut ledger, &wallet, intent().operation, &(), clock())
            .await
            .is_err()
    );
    assert!(wallet.pays.load(Ordering::SeqCst) == 1);
}

#[tokio::test]
async fn transport_loss_reopen_and_missing_lookup_never_redispatch_or_release_reserve() {
    let dir = Directory::new();
    let mut ledger = ledger(&dir);
    let wallet = FixtureWallet::new(
        &dir,
        vec![None, Some(PaymentStatus::Unknown), Some(settled())],
    );
    let result = dispatch(&mut ledger, &wallet, intent().operation, &(), clock())
        .await
        .unwrap();
    assert!(result.evidence == EvidenceStatus::Unresolved && result.record.reserved_msat == 2_100);
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert!(
        dispatch(&mut ledger, &wallet, intent().operation, &(), clock())
            .await
            .is_err()
    );
    let result = reconcile(&mut ledger, &wallet, intent().operation, &(), clock())
        .await
        .unwrap();
    assert!(result.evidence == EvidenceStatus::Unresolved && result.record.reserved_msat == 2_100);
    let result = reconcile(&mut ledger, &wallet, intent().operation, &(), clock())
        .await
        .unwrap();
    assert!(
        result.evidence == EvidenceStatus::PaidWithinWindow && result.record.spent_msat == 2_050
    );
    assert!(wallet.pays.load(Ordering::SeqCst) == 1 && wallet.lookups.load(Ordering::SeqCst) == 2);
}

#[tokio::test]
async fn failed_durable_claim_never_calls_wallet_and_persist_failure_requires_lookup() {
    let dir = Directory::new();
    let mut ledger = ledger(&dir);
    let wallet = FixtureWallet::new(&dir, vec![Some(settled()), Some(settled())]);
    ledger.connection.execute_batch("CREATE TRIGGER refuse_dispatch BEFORE UPDATE ON outgoing_intents WHEN NEW.state=1 BEGIN SELECT RAISE(ABORT,'fixture');END;").unwrap();
    assert!(
        dispatch(&mut ledger, &wallet, intent().operation, &(), clock())
            .await
            .is_err()
    );
    assert!(wallet.pays.load(Ordering::SeqCst) == 0);
    ledger.connection.execute_batch("DROP TRIGGER refuse_dispatch;CREATE TRIGGER refuse_settlement BEFORE UPDATE ON outgoing_intents WHEN NEW.state=3 BEGIN SELECT RAISE(ABORT,'fixture');END;").unwrap();
    assert!(
        dispatch(&mut ledger, &wallet, intent().operation, &(), clock())
            .await
            .is_err()
    );
    assert!(
        ledger
            .get_outgoing(intent().operation)
            .unwrap()
            .unwrap()
            .state
            == OutgoingState::InFlight
    );
    ledger
        .connection
        .execute_batch("DROP TRIGGER refuse_settlement;")
        .unwrap();
    let result = reconcile(&mut ledger, &wallet, intent().operation, &(), clock())
        .await
        .unwrap();
    assert!(result.record.spent_msat == 2_050);
    assert!(wallet.pays.load(Ordering::SeqCst) == 1 && wallet.lookups.load(Ordering::SeqCst) == 1);
}

#[tokio::test]
async fn wrong_account_invoice_or_dispatch_window_fails_before_claim() {
    for change in 0..3 {
        let dir = Directory::new();
        let mut ledger = ledger(&dir);
        let mut wallet = FixtureWallet::new(&dir, vec![]);
        match change {
            0 => wallet.account = AccountId([9; 32]),
            1 => wallet.matches = false,
            _ => wallet.can_dispatch = false,
        }
        assert!(
            dispatch(&mut ledger, &wallet, intent().operation, &(), clock())
                .await
                .is_err()
        );
        assert!(wallet.pays.load(Ordering::SeqCst) == 0);
        assert!(
            ledger
                .get_outgoing(intent().operation)
                .unwrap()
                .unwrap()
                .state
                == OutgoingState::Prepared
        );
    }
}

#[tokio::test]
async fn late_native_payment_records_cost_but_cannot_authorize_and_fee_breach_freezes() {
    let dir = Directory::new();
    let mut ledger = ledger(&dir);
    let mut proof = match settled() {
        PaymentStatus::Settled(p) => p,
        _ => unreachable!(),
    };
    proof.settled_earliest_ms = 199_500;
    proof.settled_latest_ms = 200_499;
    proof.fee_msat = 101;
    let wallet = FixtureWallet::new(&dir, vec![Some(PaymentStatus::Settled(proof))]);
    let result = dispatch(&mut ledger, &wallet, intent().operation, &(), clock())
        .await
        .unwrap();
    assert!(
        result.evidence == EvidenceStatus::PaidOutsideWindow && result.record.spent_msat == 2_101
    );
    assert!(result.record.fee_limit_breached && ledger.outgoing_admission_blocked().unwrap());
}

#[tokio::test]
async fn inconsistent_native_bindings_never_release_financial_exposure() {
    for change in 0..5 {
        let dir = Directory::new();
        let mut ledger = ledger(&dir);
        let mut proof = match settled() {
            PaymentStatus::Settled(p) => p,
            _ => unreachable!(),
        };
        match change {
            0 => proof.account.chain = Chain::Bitcoin,
            1 => proof.operation = [2; 16],
            2 => proof.amount_msat = 2,
            3 => proof.invoice_digest = [3; 32],
            _ => proof.settled_latest_ms = 0,
        }
        let wallet = FixtureWallet::new(&dir, vec![Some(PaymentStatus::Settled(proof))]);
        let result = dispatch(&mut ledger, &wallet, intent().operation, &(), clock())
            .await
            .unwrap();
        assert!(
            result.evidence == EvidenceStatus::Unresolved && result.record.reserved_msat == 2_100
        );
    }
}

#[tokio::test]
async fn closed_unresolved_only_reconciles_and_final_native_failure_releases_its_reserve() {
    let dir = Directory::new();
    let mut ledger = ledger(&dir);
    let wallet = FixtureWallet::new(
        &dir,
        vec![
            Some(PaymentStatus::Pending),
            Some(PaymentStatus::FailedFinal),
        ],
    );
    dispatch(&mut ledger, &wallet, intent().operation, &(), clock())
        .await
        .unwrap();
    ledger.close_instance(intent().instance).unwrap();
    let result = reconcile(&mut ledger, &wallet, intent().operation, &(), clock())
        .await
        .unwrap();
    assert!(result.evidence == EvidenceStatus::FailedFinal && result.record.closed);
    assert!(result.record.reserved_msat == 0 && result.record.spent_msat == 0);
    assert!(wallet.pays.load(Ordering::SeqCst) == 1);
}

#[test]
fn recipient_mapping_preserves_account_actual_msat_id_and_conservative_precision() {
    let proof = IncomingSettlement {
        account: native_account(),
        invoice_digest: [1; 32],
        payment_hash: [2; 32],
        amount_received_msat: 2_001,
        payment_id: Some([3; 32]),
        settled_earliest_ms: 150_000,
        settled_latest_ms: 150_999,
    };
    let evidence = recipient_evidence(account(), &proof).unwrap();
    assert!(evidence.amount_msat == 2_001 && evidence.authenticated_payment_id == Some([3; 32]));
    assert!(evidence.settled_at_ms == 150_000 && evidence.settlement_uncertainty_ms == 999);
    assert!(evidence.incoming && evidence.lightning && evidence.completed);
    assert!(recipient_evidence(AccountId([9; 32]), &proof).is_err());
    let mut invalid = proof;
    invalid.settled_latest_ms = 149_999;
    assert!(recipient_evidence(account(), &invalid).is_err());
}

// Generated with fixed public test keys and preimage; no wallet funds or secrets.
const NATIVE_INVOICE: &str = "lnbcrt20n1pj48ugqdq4d3hkxctvypnxj7r5w4ex2pp5238x9nhgqvmsncufuke8256r6rg04rzg2qs4e7mrx9chaqx3463ssp5qvpsxqcrqvpsxqcrqvpsxqcrqvpsxqcrqvpsxqcrqvpsxqcrqvps9qrsgqxqrrsscqpjqxcdh8wn6pkfyp098awygpuelrdrc5czep07sq5jnfrwqqv2fk4h6ws36exaunxy9xszjksd4sgsm8xtjlkxdpe89gnvwpdpmxp9kucpwlgm5z";

#[tokio::test]
async fn real_native_adapter_and_ledger_recover_dropped_pay_after_reopen_without_redispatch() {
    use lni::cln::{foundation::InvoiceExpectation, ClnConfig};
    use serde_json::{json, Value};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = Directory::new();
    let mut i = intent();
    i.payment_hash = Sha256::digest([42; 32]).into();
    i.invoice_digest = Sha256::digest(NATIVE_INVOICE.as_bytes()).into();
    let now = ClockObservation {
        now_ms: 1_700_000_100_000,
        uncertainty_ms: 0,
    };
    i.window = AcceptanceWindow {
        opens_at_ms: now.now_ms - 60_000,
        closes_at_ms: now.now_ms + 60_000,
    };
    let mut ledger = ledger_for(&dir, &i, now);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let path = dir.file();
    let operation = i.operation;
    let hash = i.payment_hash;
    let worker = tokio::spawn(async move {
        let info = json!({"id":hex::encode(native_account().node_id),"network":"regtest"});
        let completed = json!({"pays":[{"payment_hash":hex::encode(hash),"label":hex::encode(operation),
            "bolt11":NATIVE_INVOICE,"status":"complete","created_at":1_700_000_000u64,
            "completed_at":1_700_000_100u64,"amount_msat":2000,"amount_sent_msat":2050,
            "preimage":hex::encode([42;32])}]});
        let steps = [
            ("getinfo", Some(info.clone())),
            (
                "help",
                Some(json!({"help":[{"command":"pay bolt11 [label] [maxfee]"}]})),
            ),
            (
                "help",
                Some(json!({"help":[{"command":"listpays [payment_hash]"}]})),
            ),
            (
                "help",
                Some(json!({"help":[{"command":"fetchinvoice offer [amount_msat] [payer_note]"}]})),
            ),
            ("getinfo", Some(info.clone())),
            ("listpays", Some(json!({"pays":[]}))),
            ("pay", None),
            ("getinfo", Some(info)),
            ("listpays", Some(completed)),
        ];
        for (method, reply) in steps {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let end;
            loop {
                let mut next = [0; 1024];
                let count = stream.read(&mut next).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&next[..count]);
                assert!(bytes.len() < 8192);
                if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    end = at + 4;
                    break;
                }
            }
            let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
            assert!(header.starts_with(&format!("POST /v1/{method} HTTP/1.1\r\n")));
            let length = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while bytes.len() - end < length {
                let mut next = [0; 1024];
                let count = stream.read(&mut next).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&next[..count]);
            }
            let args: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
            if method == "pay" {
                let observed = PaymentLedger::reopen(&path, account())
                    .unwrap()
                    .get_outgoing(operation)
                    .unwrap()
                    .unwrap();
                assert!(
                    observed.state == OutgoingState::InFlight && observed.reserved_msat == 2100
                );
                assert!(
                    args == json!({"bolt11":NATIVE_INVOICE,"maxfee":"100msat","label":hex::encode(operation),"retry_for":1})
                );
            }
            if method == "listpays" {
                assert!(args == json!({"payment_hash":hex::encode(hash)}));
            }
            if let Some(reply) = reply {
                let text = reply.to_string();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",text.len(),text).as_bytes()).await.unwrap();
            }
        }
    });
    let wallet = ClnFoundation::connect(
        ClnConfig {
            url: format!("http://{address}"),
            rune: "fixture-rune".into(),
            http_timeout: Some(3),
            ..Default::default()
        },
        native_account(),
    )
    .await
    .unwrap();
    let invoice = wallet
        .restore_for_lookup(
            NATIVE_INVOICE,
            i.invoice_digest,
            InvoiceExpectation {
                amount_msat: i.amount_msat,
                expected_payment_hash: Some(i.payment_hash),
                expected_payee: None,
            },
        )
        .unwrap();
    prepare_cln(&mut ledger, &wallet, &i, &invoice, now).unwrap();
    let result = dispatch_cln(&mut ledger, &wallet, i.operation, &invoice, now, 1)
        .await
        .unwrap();
    assert!(result.evidence == EvidenceStatus::Unresolved && result.record.reserved_msat == 2100);
    drop(ledger);
    let mut ledger = PaymentLedger::reopen(&dir.file(), account()).unwrap();
    assert!(
        dispatch_cln(&mut ledger, &wallet, i.operation, &invoice, now, 1)
            .await
            .is_err()
    );
    let result = reconcile_cln(&mut ledger, &wallet, i.operation, &invoice, now)
        .await
        .unwrap();
    assert!(
        result.evidence == EvidenceStatus::PaidWithinWindow && result.record.spent_msat == 2050
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap();
}

#[test]
fn recipient_bridge_cannot_clip_native_amount_or_precision_to_authorize() {
    for case in 0..3 {
        let dir = Directory::new();
        let mut ledger = PaymentLedger::provision(&dir.file(), account()).unwrap();
        let obligation = Obligation {
            instance: [1; 16],
            handle: [2; 16],
            circuit_binding: [3; 32],
            round: 0,
            kind: ChargeKind::Handshake,
            payment_id: [4; 32],
            amount_msat: 2000,
            expected_payment_hash: Some([5; 32]),
            policy_hash: [6; 32],
            allocation_hash: [7; 32],
            window: Some(AcceptanceWindow {
                opens_at_ms: if case == 2 { 150_000 } else { 149_000 },
                closes_at_ms: 151_000,
            }),
        };
        ledger.register_handshake(&obligation).unwrap();
        let proof = IncomingSettlement {
            account: native_account(),
            invoice_digest: [8; 32],
            payment_hash: [5; 32],
            amount_received_msat: if case == 0 { 2001 } else { 2000 },
            payment_id: None,
            settled_earliest_ms: 150_000,
            settled_latest_ms: 150_999,
        };
        let evidence = recipient_evidence(account(), &proof).unwrap();
        let result = ledger.authorize(
            &obligation,
            &evidence,
            [9; 16],
            ClockObservation {
                now_ms: 150_999,
                uncertainty_ms: 0,
            },
        );
        match case {
            0 => assert!(matches!(result, Err(LedgerError::InvalidEvidence))),
            1 => assert!(result.is_ok()),
            _ => assert!(matches!(result, Err(LedgerError::OutsideWindow))),
        }
    }
}

#[tokio::test]
async fn phoenixd_socket_loss_retains_durable_claim_and_reopen_only_recovers() {
    use lni::{phoenixd::{foundation::PhoenixdFoundation,PhoenixdConfig},cln::foundation::{InvoiceExpectation,InvoiceVerifier}};
    use sha2::{Digest,Sha256};
    use tokio::io::{AsyncReadExt,AsyncWriteExt};
    let dir=Directory::new();
    let artifact=include_str!("../../client/foundation/fixtures/round-one-invoice.txt").trim().to_owned();
    let offer=include_str!("../../client/foundation/fixtures/round-one-offer.txt").trim();
    let at=ClockObservation{now_ms:1_700_000_100_000,uncertainty_ms:0};
    let mut i=intent();i.payment_id=[7;32];i.invoice_digest=Sha256::digest(artifact.as_bytes()).into();
    i.payment_hash=Sha256::digest([42u8;32]).into();i.fee_limit_msat=1000;
    i.window=AcceptanceWindow{opens_at_ms:at.now_ms-1000,closes_at_ms:at.now_ms+60000};
    let mut ledger=ledger_for(&dir,&i,at);
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let addr=listener.local_addr().unwrap();
    let pays=std::sync::Arc::new(AtomicUsize::new(0));let observed=pays.clone();let path=dir.file();
    let op=hex::encode(i.operation);let operation=format!("{}-{}-{}-{}-{}",&op[..8],&op[8..12],&op[12..16],&op[16..20],&op[20..]);
    let invoice=artifact.clone();let opid=i.operation;
    let server=tokio::spawn(async move {
        loop {
            let (mut socket,_)=listener.accept().await.unwrap();let mut bytes=Vec::new();
            let end=loop {let mut part=[0u8;2048];let n=socket.read(&mut part).await.unwrap();if n==0{return;}bytes.extend_from_slice(&part[..n]);assert!(bytes.len()<16384);if let Some(n)=bytes.windows(4).position(|b|b==b"\r\n\r\n"){break n+4;}};
            let headers=std::str::from_utf8(&bytes[..end]).unwrap();let uri=headers.lines().next().unwrap().split_whitespace().nth(1).unwrap().to_owned();
            let len=headers.lines().find_map(|s|s.to_ascii_lowercase().strip_prefix("content-length:").map(|v|v.trim().parse::<usize>().unwrap())).unwrap_or(0);
            while bytes.len()<end+len {let mut part=[0u8;2048];let n=socket.read(&mut part).await.unwrap();assert!(n>0);bytes.extend_from_slice(&part[..n]);}
            let reply=match uri.as_str() {
                "/getinfo"=>Some(serde_json::json!({"nodeId":hex::encode(native_account().node_id),"chain":"regtest"})),
                "/eltor/capabilities"=>Some(serde_json::json!({"version":1,"fetchBeforePay":1,"boundedFeeMsat":1})),
                "/eltor/payinvoice"=>{
                    let text=std::str::from_utf8(&bytes[end..end+len]).unwrap();
                    assert!(!text.contains('%') && !text.contains('+')); // These fixture fields are URL-safe ASCII.
                    let form:std::collections::BTreeMap<String,String>=text.split('&').map(|p|{let(k,v)=p.split_once('=').unwrap();(k.to_owned(),v.to_owned())}).collect();
                    assert_eq!(form["operation"],operation);assert_eq!(form["invoice"],invoice);assert_eq!(form["maxFeeMsat"],"1000");
                    let stored=PaymentLedger::reopen(&path,account()).unwrap().get_outgoing(opid).unwrap().unwrap();
                    assert!(stored.state==OutgoingState::InFlight);assert_eq!(stored.reserved_msat,3000);
                    assert_eq!(observed.fetch_add(1,Ordering::SeqCst),0);
                    continue; // Simulate loss after native dispatch; never resend.
                }
                v if v==format!("/eltor/outgoing/{operation}")=>{
                    if observed.load(Ordering::SeqCst)==0 {None} else {Some(serde_json::json!({"operation":operation,"invoice":invoice,"paymentHash":hex::encode(Sha256::digest([42u8;32])),"state":"paid","amountMsat":2000,"feeMsat":1000,"createdAt":at.now_ms-10,"completedAt":at.now_ms-1,"preimage":hex::encode([42u8;32])}))}
                }
                _=>panic!("unexpected synthetic native method"),
            };
            let (status,body)=match reply {Some(v)=>("200 OK",serde_json::to_vec(&v).unwrap()),None=>("204 No Content",vec![])};
            socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.unwrap();socket.write_all(&body).await.unwrap();
        }
    });
    let wallet=PhoenixdFoundation::connect(PhoenixdConfig{url:format!("http://{addr}/"),password:"public-fixture-only".into(),..Default::default()},native_account()).await.unwrap();
    let checked=InvoiceVerifier::new(native_account()).unwrap().restore_bolt12_for_offer_lookup(&artifact,i.invoice_digest,offer,i.payment_id,InvoiceExpectation{amount_msat:i.amount_msat,expected_payee:None,expected_payment_hash:Some(i.payment_hash)}).unwrap();
    let first=dispatch_cln(&mut ledger,&wallet,i.operation,&checked,at,1).await.unwrap();
    assert!(first.record.state==OutgoingState::Unknown);assert_eq!(first.record.reserved_msat,3000);drop(ledger);
    let mut reopened=PaymentLedger::reopen(&dir.file(),account()).unwrap();
    let result=reconcile_cln(&mut reopened,&wallet,i.operation,&checked,at).await.unwrap();
    assert!(result.evidence==EvidenceStatus::PaidWithinWindow);assert_eq!(result.record.spent_msat,3000);
    assert_eq!(result.record.reserved_msat,0);assert_eq!(pays.load(Ordering::SeqCst),1);
    assert!(dispatch_cln(&mut reopened,&wallet,i.operation,&checked,at,1).await.is_err());
    assert_eq!(pays.load(Ordering::SeqCst),1);server.abort();
}
