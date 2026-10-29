//! Tor-independent, lookup-only financial recovery. Tor process/handle loss
//! closes capacity, but cannot release an uncertain native outgoing payment.
use super::*;

pub struct OriginFinancialRecoveryPage {
    pub after: Option<Id16>,
    pub examined: usize,
    pub reconciled: usize,
    pub unresolved: usize,
}
/// Reconcile at most 16 account rows from the caller's retained cursor. This can
/// run before restoring a live coordinator, even after its Tor instance closed.
/// It never fetches an invoice, claims dispatch, sends pay, or touches Tor.
pub async fn reconcile_origin_financial<F>(
    ledger: &mut PaymentLedger,
    wallet: &ClnFoundation,
    after: Option<Id16>,
    limit: u64,
    mut observe: F,
) -> Result<OriginFinancialRecoveryPage>
where
    F: FnMut() -> Result<ClockObservation>,
{
    if limit == 0 || limit > 16 || ledger.account_id() != AccountId(wallet.account().fingerprint())
    {
        return Err(invalid());
    }
    let records = ledger.unresolved_outgoing(after, limit)?;
    let mut page = OriginFinancialRecoveryPage {
        after,
        examined: records.len(),
        reconciled: 0,
        unresolved: 0,
    };
    for record in records {
        page.after = Some(record.intent.operation);
        if ledger
            .get_origin_plan(record.intent.instance, record.intent.handle)?
            .is_none()
        {
            continue;
        }
        if record.state == OutgoingState::Prepared {
            // A durable intent without a dispatch claim is not evidence that
            // pay ran. Its owner must resume under live admission or close it.
            page.unresolved += 1;
            continue;
        }
        let saved = ledger
            .get_origin_invoice(record.intent.operation)?
            .ok_or("Missing exact historical invoice artifact")?;
        if saved.intent != record.intent {
            return Err(invalid());
        }
        let invoice = wallet.restore_for_lookup(
            &saved.artifact,
            saved.intent.invoice_digest,
            InvoiceExpectation {
                amount_msat: saved.intent.amount_msat,
                expected_payee: None,
                expected_payment_hash: Some(saved.intent.payment_hash),
            },
        )?;
        let clock = observe()?;
        validate_clock(clock)?;
        let result =
            wallet::reconcile_cln(ledger, wallet, record.intent.operation, &invoice, clock).await?;
        if result.evidence == EvidenceStatus::Unresolved {
            page.unresolved += 1;
        } else {
            page.reconciled += 1;
        }
    }
    Ok(page)
}
