//! Construction-to-coordinator orchestration. This routine performs no wallet
//! payment. Setup-fee prefunding is a separate prerequisite for nonzero fees.
use super::{
    construction::PreparedConstruction,
    foundation::{ClientFoundation, FoundationDraft, FoundationHop, FoundationLimits},
    path_terms::{acquire_path_terms, PathPriceLimits},
};
use crate::{
    payment_ledger::{ClockObservation, PaymentLedger},
    rpc::{FoundationPolicy, PaymentControl, PaymentRejection},
    types::{Relay, RpcConfig},
};
use lni::cln::foundation::ClnFoundation;
use std::{convert::TryInto, error::Error, sync::Arc, time::Duration};
use tokio::time::Instant;
type Result<T> = std::result::Result<T, Box<dyn Error>>;
/// Begin one explicitly selected path under finite, caller-approved limits.
/// An existing scope must be recovered, never passed here to regenerate IDs.
pub(crate) async fn begin_path<F>(
    config: &RpcConfig,
    mut ledger: PaymentLedger,
    mut control: PaymentControl,
    wallet: Option<Arc<ClnFoundation>>,
    mut relays: Vec<Relay>,
    scope: [u8; 32],
    limits: FoundationLimits,
    per_hop_routing_fee_msat: u64,
    mut observe: F,
) -> Result<ClientFoundation>
where
    F: FnMut() -> Result<ClockObservation>,
{
    if scope == [0; 32] || ledger.get_origin_construction(scope)?.is_some() {
        return Err("Existing or invalid construction scope requires recovery".into());
    }
    if limits.outstanding_msat > limits.lifetime_msat
        || limits.per_round_msat > limits.outstanding_msat
    {
        return Err("Invalid client spending limits".into());
    }
    let quoted = acquire_path_terms(
        config,
        &relays,
        PathPriceLimits {
            round_msat: limits.per_round_msat,
            handshake_msat: 0,
        },
    )
    .await?;
    let routing = per_hop_routing_fee_msat
        .checked_mul(relays.len() as u64)
        .ok_or("Routing cap overflow")?;
    if quoted
        .round_msat
        .checked_add(routing)
        .ok_or("Round cap overflow")?
        > limits.per_round_msat
    {
        return Err("Path exceeds accepted round cap including routing".into());
    }
    if quoted.handshake_msat != 0 {
        return Err("Nonzero handshake requires durable prefunding".into());
    }
    if quoted.round_msat > 0 && wallet.is_none() {
        return Err("Paid path requires verified outgoing wallet".into());
    }
    for descriptor in &quoted.descriptors {
        if FoundationPolicy::decode(&descriptor.terms)?.rate_msat > 0
            && descriptor.bolt12_offer.is_none()
        {
            return Err("Paid hop has no supported signed receiving offer".into());
        }
    }
    let setup_ms = quoted
        .descriptors
        .iter()
        .map(|d| FoundationPolicy::decode(&d.terms).map(|p| p.setup_timeout_ms))
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .min()
        .ok_or("Empty path")?;
    let inventory = control.inventory().await?;
    super::circuit::pregen_extend_paid_circuit_hashes(&mut relays, 10);
    let expected = quoted
        .descriptors
        .into_iter()
        .zip(&relays)
        .map(|(descriptor, relay)| -> Result<FoundationHop> {
            let ids = relay
                .payment_id_hashes_10
                .as_ref()
                .ok_or("Missing owned payment IDs")?
                .iter()
                .map(|id| -> Result<[u8; 32]> {
                    Ok(hex::decode(id)?
                        .try_into()
                        .map_err(|_| "Invalid owned payment ID")?)
                })
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .map_err(|_| "Invalid round count")?;
            Ok(FoundationHop {
                descriptor,
                payment_ids: ids,
                fee_limit_msat: per_hop_routing_fee_msat,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let context = serde_json::to_vec(
        &serde_json::json!({"limits":limits,"routing_fee_per_hop_msat":per_hop_routing_fee_msat,
        "hops":expected.iter().map(|hop|serde_json::json!({"fingerprint":hex::encode(hop.descriptor.fingerprint),"terms":hex::encode(hop.descriptor.terms),"offer":hop.descriptor.bolt12_offer})).collect::<Vec<_>>() }),
    )?;
    let construction = observe()?;
    let deadline = Instant::now() + Duration::from_millis(u64::from(setup_ms));
    let prepared =
        PreparedConstruction::new(scope, inventory.instance, &relays, &context, construction)?;
    let numeric = prepared.dispatch(&mut ledger, &mut control).await?;
    let binding = control.bind_origin(numeric).await?;
    if binding.instance != inventory.instance || usize::from(binding.hops) != expected.len() {
        return Err("Constructed path binding mismatch".into());
    }
    loop {
        if Instant::now() >= deadline {
            return Err("Construction readiness deadline expired".into());
        }
        match FoundationDraft::inspect(&mut control, binding.clone(), &expected, construction).await
        {
            Ok(draft) => {
                if Instant::now() >= deadline {
                    return Err("Construction readiness deadline expired".into());
                }
                let plan = draft.accept(&mut ledger, scope, limits)?;
                return ClientFoundation::new(ledger, control, wallet, plan);
            }
            Err(error) => {
                let pending = error
                    .downcast_ref::<std::io::Error>()
                    .and_then(PaymentRejection::from_io)
                    == Some(PaymentRejection::InitialStateUnconfirmed);
                if !pending {
                    return Err(error);
                }
            }
        }
        // The boxed diagnostic must leave scope before yielding this Send task.
        tokio::time::sleep_until((Instant::now() + Duration::from_millis(50)).min(deadline)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payment_ledger::AccountId;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
    };
    #[tokio::test]
    async fn existing_construction_scope_never_requotes_or_builds_again() {
        let directory = std::env::temp_dir().join(format!(
            "eltor-activation-recovery-{}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("ledger.sqlite");
        let account = AccountId([9; 32]);
        let mut ledger = PaymentLedger::provision(&file, account).unwrap();
        ledger
            .save_origin_construction([4; 32], [1; 16], b"original accepted construction")
            .unwrap();
        ledger.claim_origin_construction([4; 32]).unwrap();
        drop(ledger);
        let ledger = PaymentLedger::reopen(&file, account).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = RpcConfig {
            addr: listener.local_addr().unwrap().to_string(),
            rpc_password: None,
            command: String::new(),
        };
        let server = tokio::spawn(async move {
            let mut socket = BufReader::new(listener.accept().await.unwrap().0);
            for expected in ["AUTHENTICATE\r\n", "SETEVENTS ELTOR_PAYMENT_STATE\r\n"] {
                let mut line = String::new();
                socket.read_line(&mut line).await.unwrap();
                assert_eq!(line, expected);
                socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
            }
            let mut line = String::new();
            assert_eq!(socket.read_line(&mut line).await.unwrap(), 0);
        });
        let control = PaymentControl::connect(&config).await.unwrap();
        let result = begin_path(
            &config,
            ledger,
            control,
            None,
            vec![],
            [4; 32],
            FoundationLimits {
                lifetime_msat: 0,
                outstanding_msat: 0,
                per_round_msat: 0,
            },
            0,
            || panic!("must not replace original construction clock"),
        )
        .await;
        assert!(result.is_err());
        server.await.unwrap();
        let ledger = PaymentLedger::reopen(&file, account).unwrap();
        let retained = ledger.get_origin_construction([4; 32]).unwrap().unwrap();
        assert!(retained.dispatch_claimed);
        assert_eq!(retained.payload, b"original accepted construction");
        drop(ledger);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
