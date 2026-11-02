use super::*;
use crate::payment_ledger::{
    AcceptanceWindow, AuthorizationState, BatchRegistration, ChargeKind, ClockObservation,
    Obligation, RecipientEvidence,
};
use crate::wallet_adapters::cln::{Chain, NativeAccount};
use serde_json::{json, Value};
use std::{
    convert::TryInto,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
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
fn simulate_v4(path: &Path) {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch("DROP TABLE origin_outgoing_ids;DROP TABLE origin_allocations;DROP TABLE origin_renewal_operations;DROP TABLE origin_invoices;DROP TABLE origin_plans;DROP TABLE origin_constructions;DROP TABLE origin_operations;DROP TABLE native_invoice_candidates;DROP TABLE native_invoice_cursor;DROP TABLE closed_bindings;PRAGMA user_version=4;").unwrap();
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "eltor-ledger-operator-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn ledger(&self) -> PathBuf {
        self.0.join("account.sqlite")
    }
    fn config(&self, url: &str) -> PathBuf {
        let path = self.0.join("operator.json");
        let value = json!({"ledger_path":"account.sqlite","node_id":hex::encode(native_account().node_id),"chain":"regtest","cln_url":url,"cln_rune":"fixture-only-read-rune","clock_uncertainty_ms":1});
        std::fs::write(&path, value.to_string()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[derive(Clone, Copy)]
enum NativeReply {
    Valid,
    WrongAccount,
    MissingCapability,
    ReadDenied,
}
async fn native(reply: NativeReply) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    native_with_target(reply, None).await
}
async fn native_with_target(
    reply: NativeReply,
    target: Option<PathBuf>,
) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let worker = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let calls = calls.clone();
            let target = target.clone();
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
                    if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        header = at + 4;
                        break;
                    }
                    assert!(bytes.len() < 16384);
                }
                let headers = String::from_utf8(bytes[..header].to_vec()).unwrap();
                let path = headers.split_whitespace().nth(1).unwrap();
                let len: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap();
                while bytes.len() < header + len {
                    let mut buf = [0; 1024];
                    let n = stream.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                }
                let body: Value = serde_json::from_slice(&bytes[header..header + len]).unwrap();
                calls.fetch_add(1, Ordering::SeqCst);
                let result = match path {
                    "/v1/getinfo" => {
                        json!({"id":if matches!(reply,NativeReply::WrongAccount){"031b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f".to_owned()}else{hex::encode(native_account().node_id)},"network":"regtest"})
                    }
                    "/v1/help" => {
                        if let Some(target) = target {
                            std::fs::write(target, b"concurrent fixture target").unwrap();
                        }
                        assert_eq!(body, json!({"command":"listinvoices"}));
                        json!({"help":[{"command":if matches!(reply,NativeReply::MissingCapability){"listinvoices [payment_hash]"}else{"listinvoices [payment_hash] [index] [start] [limit]"}}]})
                    }
                    "/v1/listinvoices" => {
                        assert_eq!(body, json!({"index":"updated","start":1,"limit":16}));
                        if matches!(reply, NativeReply::ReadDenied) {
                            json!({"error":"fixture read permission denied"})
                        } else {
                            json!({"invoices":[]})
                        }
                    }
                    _ => panic!("operator attempted native data access or spending method"),
                };
                let text = result.to_string();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{text}",text.len()).as_bytes()).await.unwrap();
            });
        }
    });
    (url, count, worker)
}
fn batch() -> BatchRegistration {
    BatchRegistration {
        period: 0,
        request_digest: [3; 32],
        rounds: std::array::from_fn(|n| Obligation {
            instance: [1; 16],
            handle: [2; 16],
            circuit_binding: [3; 32],
            round: n as u64 + 1,
            kind: ChargeKind::Round,
            payment_id: [n as u8 + 4; 32],
            amount_msat: 1000,
            expected_payment_hash: None,
            policy_hash: [14; 32],
            allocation_hash: [15; 32],
            window: None,
        }),
    }
}
#[tokio::test]
async fn exclusive_provision_verifies_receiving_identity_without_pay_permissions() {
    let dir = Directory::new();
    let (url, calls, worker) = native(NativeReply::Valid).await;
    let path = dir.config(&url);
    let report = operate_foundation_ledger(LedgerOperation::Provision, &path)
        .await
        .unwrap();
    assert_eq!(
        report,
        LedgerOperationReport {
            operation: LedgerOperation::Provision,
            previous_schema: None,
            current_schema: VERSION
        }
    );
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    let ledger = PaymentLedger::reopen(&dir.ledger(), account()).unwrap();
    assert_eq!(ledger.native_invoice_cursor().unwrap(), 1);
    drop(ledger);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.ledger())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let before = std::fs::read(dir.ledger()).unwrap();
    assert_eq!(
        operate_foundation_ledger(LedgerOperation::Provision, &path)
            .await
            .unwrap_err(),
        OperatorError::TargetExists
    );
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    assert_eq!(std::fs::read(dir.ledger()).unwrap(), before);
    worker.abort();
}
#[tokio::test]
async fn wrong_native_account_or_missing_read_capability_never_creates_or_migrates() {
    for reply in [
        NativeReply::WrongAccount,
        NativeReply::MissingCapability,
        NativeReply::ReadDenied,
    ] {
        for operation in [LedgerOperation::Provision, LedgerOperation::Migrate] {
            let dir = Directory::new();
            let (url, calls, worker) = native(reply).await;
            let path = dir.config(&url);
            let before = if operation == LedgerOperation::Migrate {
                drop(PaymentLedger::provision(&dir.ledger(), account()).unwrap());
                simulate_v4(&dir.ledger());
                Some(std::fs::read(dir.ledger()).unwrap())
            } else {
                None
            };
            assert_eq!(
                operate_foundation_ledger(operation, &path)
                    .await
                    .unwrap_err(),
                OperatorError::NativeVerification
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                if matches!(reply, NativeReply::WrongAccount) {
                    1
                } else if matches!(reply, NativeReply::MissingCapability) {
                    2
                } else {
                    5
                }
            );
            if let Some(before) = before {
                assert_eq!(std::fs::read(dir.ledger()).unwrap(), before);
            } else {
                assert!(!dir.ledger().exists());
            }
            worker.abort();
        }
    }
}
#[tokio::test]
async fn invalid_existing_targets_fail_before_any_native_request_or_replacement() {
    let (url, calls, worker) = native(NativeReply::Valid).await;
    for fault in 0..6 {
        let dir = Directory::new();
        let path = dir.config(&url);
        match fault {
            0 => (),
            1 => std::fs::write(dir.ledger(), b"invalid fixture database").unwrap(),
            2 => {
                drop(PaymentLedger::provision(&dir.ledger(), AccountId([88; 32])).unwrap());
            }
            3 => {
                drop(PaymentLedger::provision(&dir.ledger(), account()).unwrap());
                Connection::open(dir.ledger())
                    .unwrap()
                    .pragma_update(None, "user_version", VERSION + 1)
                    .unwrap();
            }
            4 => std::fs::create_dir(dir.ledger()).unwrap(),
            _ => {
                std::fs::write(dir.ledger(), []).unwrap();
            }
        }
        let expected = match fault {
            0 => OperatorError::TargetMissing,
            2 => OperatorError::WrongAccount,
            3 => OperatorError::UnsupportedSchema,
            _ => OperatorError::InvalidLedger,
        };
        let before = std::fs::read(dir.ledger()).ok();
        assert_eq!(
            operate_foundation_ledger(LedgerOperation::Migrate, &path)
                .await
                .unwrap_err(),
            expected
        );
        if let Some(before) = before {
            assert_eq!(std::fs::read(dir.ledger()).unwrap(), before);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        if fault == 0 {
            assert!(!dir.ledger().exists());
        }
    }
    worker.abort();
}
#[tokio::test]
async fn explicit_v4_migration_preserves_claims_authorizations_and_tombstones() {
    let dir = Directory::new();
    let (url, calls, worker) = native(NativeReply::Valid).await;
    let path = dir.config(&url);
    let mut ledger = PaymentLedger::provision(&dir.ledger(), account()).unwrap();
    let b = batch();
    ledger.reserve_batch(&b, [16; 16]).unwrap();
    ledger
        .bind_initial_window(
            [1; 16],
            [2; 16],
            AcceptanceWindow {
                opens_at_ms: 100_000,
                closes_at_ms: 200_000,
            },
        )
        .unwrap();
    let proof = RecipientEvidence {
        account: account(),
        payment_hash: [20; 32],
        invoice_digest: [21; 32],
        amount_msat: 1000,
        settled_at_ms: 150_000,
        settlement_uncertainty_ms: 0,
        incoming: true,
        lightning: true,
        completed: true,
        authenticated_payment_id: Some(b.rounds[0].payment_id),
    };
    let saved = ledger
        .authorize(
            &b.rounds[0],
            &proof,
            [17; 16],
            ClockObservation {
                now_ms: 150_000,
                uncertainty_ms: 0,
            },
        )
        .unwrap();
    ledger.mark_consumed(&b.rounds[0], [17; 16]).unwrap();
    ledger.close_instance([1; 16]).unwrap();
    drop(ledger);
    let connection = Connection::open(dir.ledger()).unwrap();
    connection.execute_batch("DROP TABLE origin_outgoing_ids;DROP TABLE origin_allocations;DROP TABLE origin_renewal_operations;DROP TABLE origin_invoices;DROP TABLE origin_plans;DROP TABLE origin_constructions;DROP TABLE origin_operations;DROP TABLE native_invoice_candidates;DROP TABLE native_invoice_cursor;DROP TABLE closed_bindings;PRAGMA user_version=4;").unwrap();
    drop(connection);
    assert!(PaymentLedger::reopen(&dir.ledger(), account()).is_err());
    let report = operate_foundation_ledger(LedgerOperation::Migrate, &path)
        .await
        .unwrap();
    assert_eq!(report.previous_schema, Some(4));
    assert_eq!(report.current_schema, VERSION);
    let mut ledger = PaymentLedger::reopen(&dir.ledger(), account()).unwrap();
    let recovered = ledger.get_authorization(&b.rounds[0]).unwrap().unwrap();
    assert!(
        recovered.decision == saved.decision
            && recovered.payment_hash == saved.payment_hash
            && recovered.state == AuthorizationState::Consumed
    );
    assert!(
        ledger
            .get_reserved_batch_registration([1; 16], [2; 16], 0)
            .unwrap()
            == Some(b)
    );
    assert!(ledger.binding_is_closed([1; 16], [2; 16]).unwrap());
    assert_eq!(ledger.native_invoice_cursor().unwrap(), 1);
    ledger
        .apply_native_invoice_page(&crate::wallet_adapters::cln::IncomingUpdatePage {
            account: native_account(),
            from_index: 1,
            next_index: 8,
            rows_seen: 1,
            candidates: vec![],
        })
        .unwrap();
    drop(ledger);
    let connection = Connection::open(dir.ledger()).unwrap();
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM incoming_claims WHERE payment_hash=?1",
            [&[20u8; 32][..]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    drop(connection);
    let unchanged = std::fs::read(dir.ledger()).unwrap();
    let report = operate_foundation_ledger(LedgerOperation::Migrate, &path)
        .await
        .unwrap();
    assert_eq!(report.previous_schema, Some(VERSION));
    assert_eq!(std::fs::read(dir.ledger()).unwrap(), unchanged);
    assert_eq!(
        PaymentLedger::reopen(&dir.ledger(), account())
            .unwrap()
            .native_invoice_cursor()
            .unwrap(),
        8
    );
    assert_eq!(calls.load(Ordering::SeqCst), 12);
    worker.abort();
}
#[cfg(unix)]
#[tokio::test]
async fn symlink_target_and_public_config_are_rejected_without_native_access() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = Directory::new();
    let (url, calls, worker) = native(NativeReply::Valid).await;
    let path = dir.config(&url);
    let original = dir.0.join("retained.sqlite");
    drop(PaymentLedger::provision(&original, account()).unwrap());
    symlink(&original, dir.ledger()).unwrap();
    assert_eq!(
        operate_foundation_ledger(LedgerOperation::Provision, &path)
            .await
            .unwrap_err(),
        OperatorError::TargetExists
    );
    assert_eq!(
        operate_foundation_ledger(LedgerOperation::Migrate, &path)
            .await
            .unwrap_err(),
        OperatorError::InvalidLedger
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        operate_foundation_ledger(LedgerOperation::Migrate, &path)
            .await
            .unwrap_err(),
        OperatorError::Configuration
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    worker.abort();
}

#[tokio::test]
async fn target_created_during_native_verification_is_never_overwritten() {
    let dir = Directory::new();
    let (url, calls, worker) = native_with_target(NativeReply::Valid, Some(dir.ledger())).await;
    let path = dir.config(&url);
    assert_eq!(
        operate_foundation_ledger(LedgerOperation::Provision, &path)
            .await
            .unwrap_err(),
        OperatorError::TargetExists
    );
    assert_eq!(
        std::fs::read(dir.ledger()).unwrap(),
        b"concurrent fixture target"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    worker.abort();
}
