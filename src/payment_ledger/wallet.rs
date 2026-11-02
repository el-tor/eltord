//! Native CLN-to-ledger bridge. The controller supplies authenticated accepted
//! terms and a trusted clock/deadline anchor. This module does not infer quote,
//! Tor handle, policy, allocation, or offer ownership from a Lightning invoice.
//! A durable dispatch claim precedes every native dispatch. Recovery is lookup
//! only, including missing native records and transport failures.
use super::{outgoing::*, *};
use super::payer::PayerDriver;
use async_trait::async_trait;
use crate::wallet_adapters::cln::{
    AdapterError, ClnFoundation, DispatchRequest, IncomingSettlement, IncomingStatus, Observation,
    PaymentStatus, ValidatedInvoice,
};

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error(transparent)]
    Ledger(#[from] LedgerError),
    #[error("native account or immutable invoice binding mismatch")]
    BindingMismatch,
    #[error("native receiving evidence unavailable")]
    EvidenceUnavailable,
    #[error("native receiving lookup failed")]
    ReceivingLookup,
}
type Result<T> = std::result::Result<T, BridgeError>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EvidenceStatus {
    /// Financial settlement and its full native precision interval fit the
    /// immutable accepted payment window. Live Tor authorization is separate.
    PaidWithinWindow,
    /// Cost is recorded, but this payment cannot authorize the accepted round.
    PaidOutsideWindow,
    FailedFinal,
    /// Full financial reservation remains. The controller may only reconcile.
    Unresolved,
}
pub struct BridgeResult {
    pub record: OutgoingRecord,
    pub evidence: EvidenceStatus,
}

/// Reserve principal and maximum fee only after immutable native invoice and
/// account checks. The accepted issuer/quote/round binding is caller-owned.
pub fn prepare_cln<W: PayerDriver + ?Sized>(
    ledger: &mut PaymentLedger,
    wallet: &W,
    intent: &OutgoingIntent,
    invoice: &ValidatedInvoice,
    clock: ClockObservation,
) -> Result<OutgoingRecord> {
    let wallet = NativeWallet {
        wallet,
        retry_for_seconds: 1,
    };
    check_binding(ledger, &wallet, intent, invoice)?;
    Ok(ledger.prepare_outgoing(intent, clock)?)
}

pub async fn dispatch_cln<W: PayerDriver + ?Sized>(
    ledger: &mut PaymentLedger,
    wallet: &W,
    operation: Id16,
    invoice: &ValidatedInvoice,
    clock: ClockObservation,
    retry_for_seconds: u16,
) -> Result<BridgeResult> {
    dispatch(
        ledger,
        &NativeWallet {
            wallet,
            retry_for_seconds,
        },
        operation,
        invoice,
        clock,
    )
    .await
}

/// Restore the exact historical invoice using ClnFoundation::restore_for_lookup
/// and the durable intent digest/hash/amount when the original artifact expired.
/// This path never invokes native pay, even if the native lookup finds nothing.
pub async fn reconcile_cln<W: PayerDriver + ?Sized>(
    ledger: &mut PaymentLedger,
    wallet: &W,
    operation: Id16,
    invoice: &ValidatedInvoice,
    clock: ClockObservation,
) -> Result<BridgeResult> {
    reconcile(
        ledger,
        &NativeWallet {
            wallet,
            retry_for_seconds: 0,
        },
        operation,
        invoice,
        clock,
    )
    .await
}

/// Obtain exact account-owned Lightning evidence and atomically authorize it.
/// Zero-price obligations must use the ledger's explicit authorize_zero path.
pub async fn authorize_cln(
    ledger: &mut PaymentLedger,
    wallet: &ClnFoundation,
    obligation: &Obligation,
    payment_hash: Hash32,
    decision: Id16,
    clock: ClockObservation,
) -> Result<Authorization> {
    if obligation.amount_msat == 0 {
        return Err(LedgerError::InvalidInput.into());
    }
    if ledger.account != AccountId(wallet.account().fingerprint()) {
        return Err(BridgeError::BindingMismatch);
    }
    let proof = match wallet
        .lookup_incoming(payment_hash, observation(clock))
        .await
        .map_err(|_| BridgeError::ReceivingLookup)?
    {
        IncomingStatus::Settled(proof) => proof,
        _ => return Err(BridgeError::EvidenceUnavailable),
    };
    let evidence = recipient_evidence(ledger.account, &proof)?;
    Ok(ledger.authorize(obligation, &evidence, decision, clock)?)
}

pub(crate) fn recipient_evidence(account: AccountId, proof: &IncomingSettlement) -> Result<RecipientEvidence> {
    if account != AccountId(proof.account.fingerprint()) || proof.settled_earliest_ms < 0 {
        return Err(BridgeError::BindingMismatch);
    }
    let uncertainty = proof
        .settled_latest_ms
        .checked_sub(proof.settled_earliest_ms)
        .and_then(|n| u64::try_from(n).ok())
        .ok_or(BridgeError::BindingMismatch)?;
    Ok(RecipientEvidence {
        account,
        payment_hash: proof.payment_hash,
        invoice_digest: proof.invoice_digest,
        amount_msat: proof.amount_received_msat,
        settled_at_ms: proof.settled_earliest_ms,
        settlement_uncertainty_ms: uncertainty,
        incoming: true,
        lightning: true,
        completed: true,
        authenticated_payment_id: proof.payment_id,
    })
}

#[async_trait]
trait Wallet: Sync {
    type Invoice: Sync;
    fn account(&self) -> AccountId;
    fn matches(&self, intent: &OutgoingIntent, invoice: &Self::Invoice) -> bool;
    fn validate_dispatch(
        &self,
        intent: &OutgoingIntent,
        invoice: &Self::Invoice,
        clock: ClockObservation,
    ) -> bool;
    async fn pay(
        &self,
        intent: &OutgoingIntent,
        invoice: &Self::Invoice,
        clock: ClockObservation,
    ) -> std::result::Result<PaymentStatus, AdapterError>;
    async fn lookup(
        &self,
        intent: &OutgoingIntent,
        invoice: &Self::Invoice,
        clock: ClockObservation,
    ) -> std::result::Result<PaymentStatus, AdapterError>;
}
struct NativeWallet<'a, W: PayerDriver + ?Sized> {
    wallet: &'a W,
    retry_for_seconds: u16,
}
#[async_trait]
impl<W: PayerDriver + ?Sized> Wallet for NativeWallet<'_, W> {
    type Invoice = ValidatedInvoice;
    fn account(&self) -> AccountId {
        AccountId(self.wallet.account().fingerprint())
    }
    fn matches(&self, intent: &OutgoingIntent, invoice: &ValidatedInvoice) -> bool {
        invoice.account() == self.wallet.account()
            && invoice.digest() == intent.invoice_digest
            && invoice.payment_hash() == intent.payment_hash
            && invoice.amount_msat() == intent.amount_msat
    }
    fn validate_dispatch(
        &self,
        intent: &OutgoingIntent,
        invoice: &ValidatedInvoice,
        clock: ClockObservation,
    ) -> bool {
        let latest = i64::try_from(clock.uncertainty_ms)
            .ok()
            .and_then(|e| clock.now_ms.checked_add(e));
        let deadline = intent.window.closes_at_ms.min(invoice.expires_at_ms());
        self.retry_for_seconds > 0
            && latest.map_or(false, |now| {
                now >= 0 && i64::from(self.retry_for_seconds) * 1000 <= deadline.saturating_sub(now)
            })
    }
    async fn pay(
        &self,
        intent: &OutgoingIntent,
        invoice: &ValidatedInvoice,
        clock: ClockObservation,
    ) -> std::result::Result<PaymentStatus, AdapterError> {
        self.wallet
            .dispatch(
                invoice,
                DispatchRequest {
                    operation: intent.operation,
                    fee_limit_msat: intent.fee_limit_msat,
                    retry_for_seconds: self.retry_for_seconds,
                    pay_before_ms: intent.window.closes_at_ms.min(invoice.expires_at_ms()),
                },
                observation(clock),
            )
            .await
    }
    async fn lookup(
        &self,
        intent: &OutgoingIntent,
        invoice: &ValidatedInvoice,
        clock: ClockObservation,
    ) -> std::result::Result<PaymentStatus, AdapterError> {
        self.wallet
            .lookup(invoice, intent.operation, observation(clock))
            .await
    }
}
fn observation(clock: ClockObservation) -> Observation {
    Observation {
        now_ms: clock.now_ms,
        uncertainty_ms: clock.uncertainty_ms,
    }
}
fn check_binding<W: Wallet>(
    ledger: &PaymentLedger,
    wallet: &W,
    intent: &OutgoingIntent,
    invoice: &W::Invoice,
) -> Result<()> {
    if ledger.account != wallet.account()
        || intent.account != wallet.account()
        || !wallet.matches(intent, invoice)
    {
        return Err(BridgeError::BindingMismatch);
    }
    Ok(())
}
async fn dispatch<W: Wallet>(
    ledger: &mut PaymentLedger,
    wallet: &W,
    operation: Id16,
    invoice: &W::Invoice,
    clock: ClockObservation,
) -> Result<BridgeResult> {
    let before = ledger
        .get_outgoing(operation)?
        .ok_or(LedgerError::NotRegistered)?;
    check_binding(ledger, wallet, &before.intent, invoice)?;
    if !wallet.validate_dispatch(&before.intent, invoice, clock) {
        return Err(BridgeError::BindingMismatch);
    }
    let intent = ledger.begin_dispatch(operation, clock)?.into_intent();
    // Do not insert a wallet call, yield, or fallible transformation before the
    // durable claim above. Every later error preserves financial uncertainty.
    let status = wallet.pay(&intent, invoice, clock).await;
    apply(ledger, &intent, status)
}
async fn reconcile<W: Wallet>(
    ledger: &mut PaymentLedger,
    wallet: &W,
    operation: Id16,
    invoice: &W::Invoice,
    clock: ClockObservation,
) -> Result<BridgeResult> {
    let before = ledger
        .get_outgoing(operation)?
        .ok_or(LedgerError::NotRegistered)?;
    check_binding(ledger, wallet, &before.intent, invoice)?;
    if matches!(
        before.state,
        OutgoingState::Prepared | OutgoingState::CancelledBeforeDispatch
    ) {
        return Err(LedgerError::Conflict.into());
    }
    let status = wallet.lookup(&before.intent, invoice, clock).await;
    apply(ledger, &before.intent, status)
}
fn apply(
    ledger: &mut PaymentLedger,
    intent: &OutgoingIntent,
    status: std::result::Result<PaymentStatus, AdapterError>,
) -> Result<BridgeResult> {
    let (evidence, native) = match status {
        Ok(PaymentStatus::Settled(proof)) => {
            if AccountId(proof.account.fingerprint()) != ledger.account
                || proof.operation != intent.operation
                || proof.invoice_digest != intent.invoice_digest
                || proof.payment_hash != intent.payment_hash
                || proof.amount_msat != intent.amount_msat
                || proof.settled_earliest_ms < 0
                || proof.settled_latest_ms < proof.settled_earliest_ms
            {
                return unresolved(ledger, intent.operation);
            }
            let within = proof.settled_earliest_ms >= intent.window.opens_at_ms
                && proof.settled_latest_ms <= intent.window.closes_at_ms;
            (
                if within {
                    EvidenceStatus::PaidWithinWindow
                } else {
                    EvidenceStatus::PaidOutsideWindow
                },
                OutgoingEvidence {
                    account: ledger.account,
                    operation: intent.operation,
                    payment_hash: intent.payment_hash,
                    invoice_digest: intent.invoice_digest,
                    amount_msat: intent.amount_msat,
                    fee_msat: proof.fee_msat,
                    outcome: NativeOutcome::Settled,
                },
            )
        }
        Ok(PaymentStatus::FailedFinal) => (
            EvidenceStatus::FailedFinal,
            OutgoingEvidence {
                account: ledger.account,
                operation: intent.operation,
                payment_hash: intent.payment_hash,
                invoice_digest: intent.invoice_digest,
                amount_msat: intent.amount_msat,
                fee_msat: 0,
                outcome: NativeOutcome::FailedFinal,
            },
        ),
        _ => return unresolved(ledger, intent.operation),
    };
    Ok(BridgeResult {
        record: ledger.record_outgoing_outcome(&native)?,
        evidence,
    })
}
fn unresolved(ledger: &mut PaymentLedger, operation: Id16) -> Result<BridgeResult> {
    let before = ledger
        .get_outgoing(operation)?
        .ok_or(LedgerError::NotRegistered)?;
    if matches!(
        before.state,
        OutgoingState::InFlight | OutgoingState::Unknown | OutgoingState::ClosedUnresolved
    ) {
        ledger.mark_outgoing_unknown(operation)?;
    }
    Ok(BridgeResult {
        record: ledger
            .get_outgoing(operation)?
            .ok_or(LedgerError::NotRegistered)?,
        evidence: EvidenceStatus::Unresolved,
    })
}

#[cfg(test)]
mod tests;
