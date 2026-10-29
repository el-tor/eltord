//! Reconcile authenticated Tor consumption with already-durable recipient proofs.
//! This bridge never creates an authorization or infers refunds from missing state.
use super::{RegisteredCharge, RelayAuthorization, RelayRegistration};
use crate::payment_ledger::{
    AuthorizationState, BatchRegistration, ChargeKind, Obligation, PaymentLedger,
};

pub fn reconcile_relay_consumption(
    ledger: &mut PaymentLedger,
    registration: &RelayRegistration,
    expected: &BatchRegistration,
    handshake: &Obligation,
) -> Result<usize, Box<dyn std::error::Error>> {
    registration.validate_for(expected)?;
    if ledger
        .get_reserved_batch_registration(
            registration.instance,
            registration.handle,
            registration.period,
        )?
        .as_ref()
        != Some(expected)
    {
        return Err("Consumption snapshot has no matching durable batch".into());
    }
    let first = &expected.rounds[0];
    if registration.instance != first.instance
        || registration.handle != first.handle
        || registration.period != expected.period
        || handshake.instance != first.instance
        || handshake.handle != first.handle
        || handshake.circuit_binding != first.circuit_binding
        || handshake.allocation_hash != first.allocation_hash
        || handshake.policy_hash != registration.policy_hash()
        || handshake.round != 0
        || handshake.kind != ChargeKind::Handshake
    {
        return Err("Mismatched consumption snapshot binding".into());
    }
    // Preflight every reported authorization before mutating any consumption.
    // If a later record is contradictory, an earlier one is not partially applied.
    let mut consumed = Vec::with_capacity(11);
    check(ledger, handshake, &registration.fee, &mut consumed)?;
    for (wire, obligation) in registration.rounds.iter().zip(&expected.rounds) {
        if obligation.instance != registration.instance
            || obligation.handle != registration.handle
            || obligation.circuit_binding != first.circuit_binding
            || obligation.policy_hash != registration.policy_hash()
            || obligation.kind != ChargeKind::Round
            || obligation.round != wire.round
            || obligation.payment_id != wire.payment_id
            || (wire.charge.authorization == RelayAuthorization::Consumed
                && wire.round > registration.current_round)
        {
            return Err("Mismatched round consumption binding".into());
        }
        check(ledger, obligation, &wire.charge, &mut consumed)?;
    }
    // Each update is durable and idempotent. A storage failure partway through
    // leaves confirmed earlier records intact; replay finishes only the rest.
    let count = consumed.len();
    for (obligation, decision) in consumed {
        ledger.mark_consumed(obligation, decision)?;
    }
    Ok(count)
}
fn check<'a>(
    ledger: &mut PaymentLedger,
    obligation: &'a Obligation,
    charge: &RegisteredCharge,
    consumed: &mut Vec<(&'a Obligation, [u8; 16])>,
) -> Result<(), Box<dyn std::error::Error>> {
    if obligation.amount_msat != charge.amount_msat {
        return Err("Mismatched consumption amount".into());
    }
    if charge.authorization == RelayAuthorization::None {
        return Ok(());
    }
    let saved = ledger
        .get_authorization(obligation)?
        .ok_or("Tor authorization has no durable recipient proof")?;
    if charge.decision != Some(saved.decision)
        || charge.payment_hash != Some(saved.payment_hash)
        || saved.state == AuthorizationState::ClosedUnresolved
    {
        return Err("Tor authorization differs from durable recipient proof".into());
    }
    if charge.authorization == RelayAuthorization::Consumed
        && saved.state != AuthorizationState::Consumed
    {
        consumed.push((obligation, saved.decision));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payment_ledger::{AcceptanceWindow, AccountId, ClockObservation, RecipientEvidence};
    use crate::rpc::payment_registration::tests::fixture;
    struct File(std::path::PathBuf);
    impl Drop for File {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn setup() -> (File, PaymentLedger, BatchRegistration, Obligation) {
        let (_, batch) = fixture(true);
        let path = File(std::env::temp_dir().join(format!(
            "eltor-consumption-{:032x}.sqlite",
            rand::random::<u128>()
        )));
        let mut ledger = PaymentLedger::provision(&path.0, AccountId([9; 32])).unwrap();
        ledger.reserve_batch(&batch, [8; 16]).unwrap();
        let window = AcceptanceWindow {
            opens_at_ms: 100_000,
            closes_at_ms: 200_000,
        };
        ledger
            .bind_initial_window([1; 16], [2; 16], window)
            .unwrap();
        let mut fee = batch.rounds[0].clone();
        fee.kind = ChargeKind::Handshake;
        fee.round = 0;
        fee.payment_id = [90; 32];
        fee.amount_msat = 0;
        fee.window = Some(window);
        ledger.register_handshake(&fee).unwrap();
        let clock = ClockObservation {
            now_ms: 150_000,
            uncertainty_ms: 0,
        };
        ledger.authorize_zero(&fee, [30; 16], clock).unwrap();
        ledger
            .authorize(
                &batch.rounds[0],
                &RecipientEvidence {
                    account: AccountId([9; 32]),
                    payment_hash: [40; 32],
                    invoice_digest: [41; 32],
                    amount_msat: batch.rounds[0].amount_msat,
                    settled_at_ms: 140_000,
                    settlement_uncertainty_ms: 0,
                    incoming: true,
                    lightning: true,
                    completed: true,
                    authenticated_payment_id: Some(batch.rounds[0].payment_id),
                },
                [31; 16],
                clock,
            )
            .unwrap();
        (path, ledger, batch, fee)
    }
    #[test]
    fn consumption_requires_exact_saved_proofs_and_recovers_idempotently() {
        let (path, mut ledger, batch, fee) = setup();
        let (wire, _) = fixture(true);
        let good = RelayRegistration::parse(&wire, &batch).unwrap();
        let mut bad = good.clone();
        bad.rounds[0].charge.payment_hash = Some([42; 32]);
        assert!(reconcile_relay_consumption(&mut ledger, &bad, &batch, &fee).is_err());
        assert!(
            ledger.get_authorization(&fee).unwrap().unwrap().state
                == AuthorizationState::Authorized
        );
        assert!(
            ledger
                .get_authorization(&batch.rounds[0])
                .unwrap()
                .unwrap()
                .state
                == AuthorizationState::Authorized
        );
        assert_eq!(
            reconcile_relay_consumption(&mut ledger, &good, &batch, &fee).unwrap(),
            2
        );
        drop(ledger);
        let mut ledger = PaymentLedger::reopen(&path.0, AccountId([9; 32])).unwrap();
        assert_eq!(
            reconcile_relay_consumption(&mut ledger, &good, &batch, &fee).unwrap(),
            0
        );
        assert!(
            ledger
                .get_authorization(&batch.rounds[0])
                .unwrap()
                .unwrap()
                .state
                == AuthorizationState::Consumed
        );
    }
    #[test]
    fn authorized_only_and_closed_bindings_never_become_consumed() {
        let (_path, mut ledger, batch, fee) = setup();
        let (wire, _) = fixture(false);
        let mut pending = RelayRegistration::parse(&wire, &batch).unwrap();
        pending.fee.authorization = RelayAuthorization::Authorized;
        pending.fee.decision = Some([30; 16]);
        pending.rounds[0].charge.authorization = RelayAuthorization::Authorized;
        pending.rounds[0].charge.payment_hash = Some([40; 32]);
        pending.rounds[0].charge.decision = Some([31; 16]);
        assert_eq!(
            reconcile_relay_consumption(&mut ledger, &pending, &batch, &fee).unwrap(),
            0
        );
        assert!(
            ledger.get_authorization(&fee).unwrap().unwrap().state
                == AuthorizationState::Authorized
        );
        ledger.close_instance([1; 16]).unwrap();
        let (wire, _) = fixture(true);
        let active = RelayRegistration::parse(&wire, &batch).unwrap();
        assert!(reconcile_relay_consumption(&mut ledger, &active, &batch, &fee).is_err());
        assert!(
            ledger.get_authorization(&fee).unwrap().unwrap().state
                == AuthorizationState::ClosedUnresolved
        );
    }
}
