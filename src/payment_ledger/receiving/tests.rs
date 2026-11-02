use super::*;
use crate::wallet_adapters::cln::{Chain, NativeAccount};
fn account() -> NativeAccount {
    NativeAccount {
        node_id: [2; 33],
        chain: Chain::Regtest,
    }
}
struct Fixture {
    path: std::path::PathBuf,
    ledger: PaymentLedger,
    batch: BatchRegistration,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "eltor-discovery-{:032x}.sqlite",
            rand::random::<u128>()
        ));
        let mut ledger =
            PaymentLedger::provision(&path, AccountId(account().fingerprint())).unwrap();
        let batch = BatchRegistration {
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
        };
        ledger.reserve_batch(&batch, [16; 16]).unwrap();
        Self {
            path,
            ledger,
            batch,
        }
    }
    fn page(&self, index: u64) -> IncomingUpdatePage {
        IncomingUpdatePage {
            account: account(),
            from_index: index,
            next_index: index + 1,
            rows_seen: 1,
            candidates: vec![IncomingInvoiceCandidate {
                payment_id: self.batch.rounds[0].payment_id,
                payment_hash: [20; 32],
                invoice_digest: [21; 32],
                updated_index: index,
            }],
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
#[test]
fn page_and_mapping_commit_atomically_and_exact_retry_survives_independent_reopen() {
    let mut f = Fixture::new();
    let page = f.page(1);
    f.ledger.apply_native_invoice_page(&page).unwrap();
    assert!(f.ledger.native_invoice_cursor().unwrap() == 2);
    let mut reopened = PaymentLedger::reopen(&f.path, AccountId(account().fingerprint())).unwrap();
    reopened.apply_native_invoice_page(&page).unwrap();
    assert!(
        reopened
            .incoming_candidate(f.batch.rounds[0].payment_id)
            .unwrap()
            .unwrap()
            .payment_hash
            == [20; 32]
    );
    let mut changed = page;
    changed.candidates[0].invoice_digest = [22; 32];
    assert!(reopened.apply_native_invoice_page(&changed).is_err());
    assert!(reopened.native_invoice_cursor().unwrap() == 2);
}
#[test]
fn write_failure_cannot_advance_cursor_or_lose_candidate_on_restart() {
    let mut f = Fixture::new();
    f.ledger.connection.execute_batch("CREATE TRIGGER fail_cursor BEFORE UPDATE ON native_invoice_cursor BEGIN SELECT RAISE(ABORT,'fixture');END;").unwrap();
    assert!(f.ledger.apply_native_invoice_page(&f.page(1)).is_err());
    assert!(f.ledger.native_invoice_cursor().unwrap() == 1);
    assert!(f
        .ledger
        .incoming_candidate(f.batch.rounds[0].payment_id)
        .unwrap()
        .is_none());
    f.ledger
        .connection
        .execute_batch("DROP TRIGGER fail_cursor")
        .unwrap();
    f.ledger.apply_native_invoice_page(&f.page(1)).unwrap();
}
#[test]
fn changed_account_unregistered_ids_and_ambiguous_invoices_never_authorize_or_reset_cursor() {
    let mut f = Fixture::new();
    let mut page = f.page(1);
    page.account.chain = Chain::Bitcoin;
    assert!(f.ledger.apply_native_invoice_page(&page).is_err());
    page = f.page(1);
    page.candidates[0].payment_id = [99; 32];
    f.ledger.apply_native_invoice_page(&page).unwrap();
    assert!(f.ledger.incoming_candidate([99; 32]).unwrap().is_none());
    f.ledger.apply_native_invoice_page(&f.page(2)).unwrap();
    page = f.page(3);
    page.candidates[0].payment_hash = [22; 32];
    f.ledger.apply_native_invoice_page(&page).unwrap();
    assert!(matches!(
        f.ledger.incoming_candidate(f.batch.rounds[0].payment_id),
        Err(LedgerError::Conflict)
    ));
    page = f.page(4);
    page.candidates[0].payment_hash = [23; 32];
    f.ledger.apply_native_invoice_page(&page).unwrap();
    assert!(f.ledger.native_invoice_cursor().unwrap() == 5);
    assert!(matches!(f.ledger.incoming_candidate(f.batch.rounds[0].payment_id), Err(LedgerError::Conflict)));
}
#[test]
fn closure_is_handle_scoped_and_tor_restart_preserves_financial_candidates() {
    let mut f = Fixture::new();
    f.ledger.apply_native_invoice_page(&f.page(1)).unwrap();
    f.ledger.close_binding([1; 16], [2; 16]).unwrap();
    assert!(f.ledger.binding_is_closed([1; 16], [2; 16]).unwrap());
    assert!(!f.ledger.binding_is_closed([1; 16], [3; 16]).unwrap());
    let mut next = f.batch.clone();
    next.period = 1;
    for (n, r) in next.rounds.iter_mut().enumerate() {
        r.round = 11 + n as u64;
        r.payment_id = [n as u8 + 30; 32];
    }
    assert!(f.ledger.reserve_batch(&next, [22; 16]).is_err());
    f.ledger.close_other_instances([2; 16]).unwrap();
    assert!(f.ledger.binding_is_closed([1; 16], [99; 16]).unwrap());
    assert!(f
        .ledger
        .incoming_candidate(f.batch.rounds[0].payment_id)
        .unwrap()
        .is_some());
}
