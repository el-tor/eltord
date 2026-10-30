//! Durable one-shot positive handshake prefunding. The caller supplies exact
//! descriptor-authenticated terms before native construction. This module never
//! constructs/rebuilds a circuit, changes a path or infers a refund on closure.
use crate::payment_ledger::payer::FoundationPayer;
#[cfg(test)]
use lni::cln::foundation::ClnFoundation;
use crate::{
    payment_ledger::{
        handshake::*,
        outgoing::{NativeOutcome, OutgoingEvidence, OutgoingState},
        AcceptanceWindow, AccountId, ClockObservation, LedgerError, PaymentLedger,
    },
    rpc::FoundationPolicy,
};
use lni::cln::foundation::{
    ConstructionPaymentProof, DispatchRequest, InvoiceExpectation, Observation,
    PaymentStatus, ValidatedInvoice,
};
use std::{
    convert::{TryFrom, TryInto},
    io,
};

#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error("Native handshake invoice/account binding mismatch")]
    Binding,
    #[error("Native handshake payment remains uncertain; recover the original operation")]
    Uncertain,
    #[error("Handshake proof is outside its original accepted settlement window")]
    OutsideWindow,
    #[error("Trusted handshake clock observation unavailable")]
    Clock,
}
type Result<T> = std::result::Result<T, HandshakeError>;
fn observation(c: ClockObservation) -> Observation {
    Observation {
        now_ms: c.now_ms,
        uncertainty_ms: c.uncertainty_ms,
    }
}
fn observe(clock: &mut impl FnMut() -> io::Result<ClockObservation>) -> Result<ClockObservation> {
    clock().map_err(|_| HandshakeError::Clock)
}
fn accepted_amount(scope: &HandshakeScope, index: u16) -> Result<u64> {
    let h = scope
        .hops
        .get(usize::from(index).wrapping_sub(1))
        .ok_or(HandshakeError::Binding)?;
    let p: [u8; 55] = h
        .terms
        .as_slice()
        .try_into()
        .map_err(|_| HandshakeError::Binding)?;
    Ok(FoundationPolicy::decode(&p)
        .map_err(|_| HandshakeError::Binding)?
        .handshake_fee_msat)
}
fn check_account(
    ledger: &PaymentLedger,
    wallet: &FoundationPayer,
    scope: &HandshakeScope,
) -> Result<()> {
    if ledger.account_id() != scope.account
        || AccountId(wallet.account().fingerprint()) != scope.account
    {
        return Err(HandshakeError::Binding);
    }
    Ok(())
}
fn restore(
    wallet: &FoundationPayer,
    scope: &HandshakeScope,
    saved: &HandshakeRecord,
) -> Result<ValidatedInvoice> {
    let hop = scope
        .hops
        .get(usize::from(saved.invoice.hop).wrapping_sub(1))
        .ok_or(HandshakeError::Binding)?;
    wallet
        .restore_bolt12_for_offer_lookup(
            &saved.invoice.artifact,
            saved.invoice.invoice_digest,
            hop.offer.as_deref().ok_or(HandshakeError::Binding)?,
            hop.payment_id,
            InvoiceExpectation {
                amount_msat: accepted_amount(scope, saved.invoice.hop)?,
                expected_payee: None,
                expected_payment_hash: Some(saved.invoice.payment_hash),
            },
        )
        .map_err(|_| HandshakeError::Binding)
}
fn financial(
    scope: &HandshakeScope,
    index: u16,
    status: PaymentStatus,
) -> Result<Option<HandshakeEvidence>> {
    let h = scope
        .hops
        .get(usize::from(index).wrapping_sub(1))
        .ok_or(HandshakeError::Binding)?;
    match status {
        PaymentStatus::Settled(s) => Ok(Some(HandshakeEvidence {
            financial: OutgoingEvidence {
                account: AccountId(s.account.fingerprint()),
                operation: s.operation,
                invoice_digest: s.invoice_digest,
                payment_hash: s.payment_hash,
                amount_msat: s.amount_msat,
                fee_msat: s.fee_msat,
                outcome: NativeOutcome::Settled,
            },
            settled_earliest_ms: Some(s.settled_earliest_ms),
            settled_latest_ms: Some(s.settled_latest_ms),
        })),
        // FailedFinal needs the exact saved artifact metadata supplied below.
        PaymentStatus::FailedFinal => Ok(Some(HandshakeEvidence {
            financial: OutgoingEvidence {
                account: scope.account,
                operation: h.operation,
                invoice_digest: [0; 32],
                payment_hash: [0; 32],
                amount_msat: accepted_amount(scope, index)?,
                fee_msat: 0,
                outcome: NativeOutcome::FailedFinal,
            },
            settled_earliest_ms: None,
            settled_latest_ms: None,
        })),
        PaymentStatus::Unknown | PaymentStatus::Pending => Ok(None),
    }
}
fn save_status(
    ledger: &mut PaymentLedger,
    scope: &HandshakeScope,
    saved: &HandshakeRecord,
    status: PaymentStatus,
) -> Result<HandshakeRecord> {
    if let Some(mut e) = financial(scope, saved.invoice.hop, status)? {
        if e.financial.outcome == NativeOutcome::FailedFinal {
            e.financial.invoice_digest = saved.invoice.invoice_digest;
            e.financial.payment_hash = saved.invoice.payment_hash;
        }
        Ok(ledger.record_handshake_outcome(scope.scope, saved.invoice.hop, &e)?)
    } else {
        // Even a missing native record after a claimed dispatch keeps its full
        // reservation. A prepared-but-never-claimed record remains prepared.
        if matches!(
            saved.state,
            OutgoingState::InFlight | OutgoingState::Unknown | OutgoingState::ClosedUnresolved
        ) {
            ledger.mark_handshake_unknown(scope.scope, saved.invoice.hop)?;
        }
        Ok(ledger
            .get_handshake(scope.scope, saved.invoice.hop)?
            .ok_or(HandshakeError::Binding)?)
    }
}
/// Fetch a signed invoice from the exact pinned BOLT12 offer, persist its exact
/// artifact and fee reservation, then claim one native pay. An existing claimed
/// intent always takes lookup-only recovery, including an empty native lookup.
/// Zero fees require explicit constructor dummy material and never call a wallet.
pub async fn fund_handshake_hop(
    ledger: &mut PaymentLedger,
    wallet: &FoundationPayer,
    scope_id: [u8; 32],
    index: u16,
    observe_clock: &mut (impl FnMut() -> io::Result<ClockObservation> + Send),
) -> Result<HandshakeRecord> {
    let scope = ledger
        .get_handshake_scope(scope_id)?
        .ok_or(HandshakeError::Binding)?;
    check_account(ledger, wallet, &scope)?;
    let amount = accepted_amount(&scope, index)?;
    if amount == 0 {
        return Err(HandshakeError::Binding);
    }
    let h = scope
        .hops
        .get(usize::from(index) - 1)
        .ok_or(HandshakeError::Binding)?;
    let mut saved = ledger.get_handshake(scope_id, index)?;
    if saved.is_none() {
        let offer = h.offer.as_deref().ok_or(HandshakeError::Binding)?;
        ledger.claim_handshake_invoice_request(scope_id, index)?;
        let fetched = wallet
            .fetch_bolt12(
                offer,
                h.payment_id,
                InvoiceExpectation {
                    amount_msat: amount,
                    expected_payee: None,
                    expected_payment_hash: None,
                },
                observation(observe(observe_clock)?),
            )
            .await
            .map_err(|_| HandshakeError::Uncertain)?;
        check_account(ledger, wallet, &scope)?;
        let invoice = HandshakeInvoice {
            scope: scope_id,
            hop: index,
            invoice_digest: fetched.digest(),
            payment_hash: fetched.payment_hash(),
            artifact: fetched.invoice().to_owned(),
        };
        saved = Some(ledger.prepare_handshake_invoice(&invoice, observe(observe_clock)?)?);
    }
    let saved = saved.ok_or(HandshakeError::Binding)?;
    let invoice = restore(wallet, &scope, &saved)?;
    if saved.state != OutgoingState::Prepared {
        return recover_handshake_hop(ledger, wallet, scope_id, index, observe_clock).await;
    }
    // The externally persisted permit is consumed before the native call. Once
    // claimed, every error is lookup-only uncertainty, even before pay reaches CLN.
    let permit = ledger.begin_handshake_dispatch(scope_id, index, observe(observe_clock)?)?;
    let before = observe(observe_clock)?;
    let result = wallet
        .dispatch(
            &invoice,
            DispatchRequest {
                operation: permit.scope.hops[usize::from(index) - 1].operation,
                fee_limit_msat: h.fee_limit_msat,
                retry_for_seconds: 1,
                pay_before_ms: scope.window.closes_at_ms.min(invoice.expires_at_ms()),
            },
            observation(before),
        )
        .await;
    let current = ledger
        .get_handshake(scope_id, index)?
        .ok_or(HandshakeError::Binding)?;
    match result {
        Ok(status) => save_status(ledger, &scope, &current, status),
        Err(_) => {
            ledger.mark_handshake_unknown(scope_id, index)?;
            Err(HandshakeError::Uncertain)
        }
    }
}
/// Exact historical read only. This remains usable after closure or Tor restart;
/// it cannot acquire a dispatch permit, fetch a new invoice or change deadlines.
pub async fn recover_handshake_hop(
    ledger: &mut PaymentLedger,
    wallet: &FoundationPayer,
    scope_id: [u8; 32],
    index: u16,
    observe_clock: &mut (impl FnMut() -> io::Result<ClockObservation> + Send),
) -> Result<HandshakeRecord> {
    let scope = ledger
        .get_handshake_scope(scope_id)?
        .ok_or(HandshakeError::Binding)?;
    check_account(ledger, wallet, &scope)?;
    let saved = ledger
        .get_handshake(scope_id, index)?
        .ok_or(HandshakeError::Binding)?;
    let invoice = restore(wallet, &scope, &saved)?;
    let h = &scope.hops[usize::from(index) - 1];
    let status = wallet
        .lookup(&invoice, h.operation, observation(observe(observe_clock)?))
        .await
        .map_err(|_| HandshakeError::Uncertain)?;
    save_status(ledger, &scope, &saved, status)
}
/// Ephemeral proof for the one permitted constructor dispatch. Validate both the
/// original funding window and the recipient's normative registration window:
/// [latest possible construction - 5min, earliest construction + setup_timeout].
/// Native completed_at precision is an interval, never rounded to an exact time.
/// This does not claim the recipient has consumed the fee or admitted the path.
pub async fn handshake_construction_proof(
    ledger: &mut PaymentLedger,
    wallet: &FoundationPayer,
    scope_id: [u8; 32],
    index: u16,
    construction: ClockObservation,
    observe_clock: &mut (impl FnMut() -> io::Result<ClockObservation> + Send),
) -> Result<ConstructionPaymentProof> {
    let scope = ledger
        .get_handshake_scope(scope_id)?
        .ok_or(HandshakeError::Binding)?;
    check_account(ledger, wallet, &scope)?;
    if ledger.instance_is_closed(scope.instance)? {
        return Err(LedgerError::Closed.into());
    }
    let saved = ledger
        .get_handshake(scope_id, index)?
        .ok_or(HandshakeError::Binding)?;
    if !matches!(
        saved.state,
        OutgoingState::InFlight | OutgoingState::Unknown | OutgoingState::Settled
    ) || saved.closed
    {
        return Err(HandshakeError::Binding);
    }
    let invoice = restore(wallet, &scope, &saved)?;
    let h = &scope.hops[usize::from(index) - 1];
    let proof = wallet
        .lookup_construction_proof(&invoice, h.operation, observation(observe(observe_clock)?))
        .await
        .map_err(|_| HandshakeError::Uncertain)?
        .ok_or(HandshakeError::Uncertain)?;
    let s = proof.settlement().clone();
    let record = save_status(ledger, &scope, &saved, PaymentStatus::Settled(s.clone()))?;
    if record.fee_limit_breached {
        return Err(HandshakeError::Binding);
    }
    let uncertainty =
        i64::try_from(construction.uncertainty_ms).map_err(|_| HandshakeError::Clock)?;
    if construction.now_ms < 0 || construction.uncertainty_ms > 5000 {
        return Err(HandshakeError::Clock);
    }
    let terms: [u8; 55] = h
        .terms
        .as_slice()
        .try_into()
        .map_err(|_| HandshakeError::Binding)?;
    let p = FoundationPolicy::decode(&terms).map_err(|_| HandshakeError::Binding)?;
    let opens = construction
        .now_ms
        .checked_add(uncertainty)
        .and_then(|n| n.checked_sub(300000))
        .ok_or(HandshakeError::Clock)?
        .max(scope.window.opens_at_ms);
    let closes = construction
        .now_ms
        .checked_sub(uncertainty)
        .and_then(|n| n.checked_add(i64::from(p.setup_timeout_ms)))
        .ok_or(HandshakeError::Clock)?
        .min(scope.window.closes_at_ms);
    if s.settled_earliest_ms < opens || s.settled_latest_ms > closes || closes < opens {
        return Err(HandshakeError::OutsideWindow);
    }
    Ok(proof)
}
#[cfg(test)]
#[path = "handshake_tests.rs"]
mod tests;

pub struct HandshakeFinancialRecoveryPage {
    pub after: Option<[u8; 16]>,
    pub examined: usize,
    pub reconciled: usize,
    pub unresolved: usize,
}
/// Account-wide historical recovery, independent of a live Tor handle. Each
/// page has at most 16 exact native lookups; expiry/closure never changes the
/// original artifact, dispatch claim or accounting. Undispatched reservations
/// remain pending for their owner and never acquire a new payment permit here.
pub async fn reconcile_handshake_financial<F>(
    ledger: &mut PaymentLedger,
    wallet: &FoundationPayer,
    after: Option<[u8; 16]>,
    limit: u64,
    mut observe_clock: F,
) -> std::result::Result<HandshakeFinancialRecoveryPage, Box<dyn std::error::Error>>
where
    F: FnMut() -> std::result::Result<ClockObservation, Box<dyn std::error::Error>>,
{
    if limit == 0 || limit > 16 || ledger.account_id() != AccountId(wallet.account().fingerprint())
    {
        return Err(HandshakeError::Binding.into());
    }
    let records = ledger.unresolved_handshakes(after, limit)?;
    let mut page = HandshakeFinancialRecoveryPage {
        after,
        examined: records.len(),
        reconciled: 0,
        unresolved: 0,
    };
    for (scope, record) in records {
        let hop = scope
            .hops
            .get(usize::from(record.invoice.hop).wrapping_sub(1))
            .ok_or(HandshakeError::Binding)?;
        page.after = Some(hop.operation);
        if record.state == OutgoingState::Prepared {
            page.unresolved += 1;
            continue;
        }
        let clock = observe_clock()?;
        if clock.now_ms < 0 || clock.uncertainty_ms > 5000 {
            return Err(HandshakeError::Clock.into());
        }
        let recovered =
            recover_handshake_hop(ledger, wallet, scope.scope, record.invoice.hop, &mut || {
                Ok(clock)
            })
            .await?;
        if matches!(
            recovered.state,
            OutgoingState::Settled | OutgoingState::Consumed | OutgoingState::FailedFinal
        ) {
            page.reconciled += 1;
        } else {
            page.unresolved += 1;
        }
    }
    Ok(page)
}
