//! One accepted construction: pin descriptors/IDs, durably prefund required
//! handshake fees, then inject transient proofs into one sanitized constructor.
//! Existing claimed construction never causes a replacement path or payment.
use crate::payment_ledger::payer::FoundationPayer;
use super::{
    construction::PreparedConstruction,
    foundation::{ClientFoundation, FoundationDraft, FoundationHop, FoundationLimits},
    handshake::{fund_handshake_hop, handshake_construction_proof},
    path_terms::{acquire_path_terms, AcceptedPathTerms, PathPriceLimits},
};
use crate::{
    payment_ledger::{
        handshake::{HandshakeHop, HandshakeScope},
        outgoing::OutgoingState,
        AcceptanceWindow, ClockObservation, PaymentLedger,
    },
    rpc::{
        FoundationDescriptor, FoundationPolicy, OriginBinding, PaymentControl, PaymentRejection,
    },
    types::{Relay, RpcConfig},
};

use rand::RngCore;
use std::{
    convert::{TryFrom, TryInto},
    error::Error,
    io,
    sync::Arc,
    time::Duration,
};
use tokio::time::Instant;
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MAX_BIND_OBSERVATIONS: usize = 128;
const BIND_OBSERVATION_INTERVAL: Duration = Duration::from_millis(250);

/// The constructor receipt precedes the first cold OR connection. Only that
/// still-owned live connection may wait for its payment-control registration;
/// this is never numeric-ID recovery after a restart or a lost receipt.
async fn await_constructed_binding(
    control: &mut PaymentControl,
    numeric: u32,
    instance: [u8; 16],
    hops: usize,
    deadline: Instant,
) -> io::Result<OriginBinding> {
    for attempt in 0..MAX_BIND_OBSERVATIONS {
        if Instant::now() >= deadline {
            break;
        }
        let reply = tokio::time::timeout_at(deadline, control.bind_origin(numeric))
            .await
            .map_err(|_| {
                log::warn!("Foundation construction BIND timed out at original setup deadline; receipt observed, accepted plan absent, durable claim retained");
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Original construction deadline expired",
                )
            })?;
        match reply {
            Ok(binding) => {
                if Instant::now() >= deadline {
                    break;
                }
                if binding.instance != instance || usize::from(binding.hops) != hops {
                    log::warn!("Foundation construction BIND validation failed; receipt observed, accepted plan absent, durable claim retained");
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Constructed path binding mismatch",
                    ));
                }
                log::info!("Foundation construction BIND ready; original receipt verified, accepted plan pending");
                return Ok(binding);
            }
            Err(error)
                if PaymentRejection::from_io(&error) == Some(PaymentRejection::UnknownCircuit) =>
            {
                if attempt == 0 {
                    log::info!("Foundation construction BIND pending: UnknownCircuit; receipt observed, original setup deadline and durable claim retained");
                }
                if attempt + 1 == MAX_BIND_OBSERVATIONS {
                    break;
                }
            }
            Err(error) => {
                if let Some(rejection) = PaymentRejection::from_io(&error) {
                    log::warn!("Foundation construction BIND stopped: {:?}; receipt observed, accepted plan absent, durable claim retained", rejection);
                } else {
                    log::warn!("Foundation construction BIND stopped: I/O {:?}; receipt observed, accepted plan absent, durable claim retained", error.kind());
                }
                return Err(error);
            }
        }
        tokio::time::sleep_until((Instant::now() + BIND_OBSERVATION_INTERVAL).min(deadline)).await;
    }
    log::warn!("Foundation construction BIND original deadline or observation cap reached; receipt observed, accepted plan absent, durable claim retained");
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "Original construction binding unavailable",
    ))
}
/// Begin one explicitly selected path under finite, caller-approved limits.
/// An existing scope must be recovered, never passed here to regenerate IDs.
pub(crate) async fn begin_path<F>(
    config: &RpcConfig,
    mut ledger: PaymentLedger,
    mut control: PaymentControl,
    wallet: Option<Arc<FoundationPayer>>,
    mut relays: Vec<Relay>,
    scope: [u8; 32],
    limits: FoundationLimits,
    per_hop_routing_fee_msat: u64,
    handshake_cap_msat: u64,
    handshake_funding_timeout_ms: Option<u64>,
    mut observe: F,
) -> Result<ClientFoundation>
where
    F: FnMut() -> Result<ClockObservation> + Send,
{
    if scope == [0; 32] || ledger.get_origin_construction(scope)?.is_some() {
        return Err("Existing or invalid construction scope requires recovery".into());
    }
    if limits.outstanding_msat > limits.lifetime_msat
        || limits.per_round_msat > limits.outstanding_msat
    {
        return Err("Invalid client spending limits".into());
    }
    let inventory = control.inventory().await?;
    let saved_funding = ledger.get_handshake_scope(scope)?;
    let quoted = if let Some(saved) = &saved_funding {
        if saved.instance != inventory.instance
            || saved.account != ledger.account_id()
            || saved.lifetime_limit_msat != limits.lifetime_msat
            || saved.outstanding_limit_msat != limits.outstanding_msat
            || saved.per_round_limit_msat != limits.per_round_msat
            || saved.hops.len() != relays.len()
            || Some((saved.window.closes_at_ms - saved.window.opens_at_ms) as u64)
                != handshake_funding_timeout_ms
        {
            return Err("Saved prefunding differs from original accepted path".into());
        }
        let mut round_msat = 0u64;
        let mut handshake_msat = 0u64;
        let mut descriptors = Vec::new();
        for (saved, relay) in saved.hops.iter().zip(&relays) {
            if hex::encode(saved.fingerprint) != relay.fingerprint.to_ascii_lowercase()
                || saved.fee_limit_msat != per_hop_routing_fee_msat
            {
                return Err("Saved prefunding path differs".into());
            }
            let terms: [u8; 55] = saved
                .terms
                .as_slice()
                .try_into()
                .map_err(|_| "Invalid saved prefunding policy")?;
            let p = FoundationPolicy::decode(&terms)?;
            round_msat = round_msat
                .checked_add(p.rate_msat)
                .ok_or("Round cap overflow")?;
            handshake_msat = handshake_msat
                .checked_add(p.handshake_fee_msat)
                .ok_or("Setup cap overflow")?;
            descriptors.push(FoundationDescriptor {
                fingerprint: saved.fingerprint,
                terms,
                bolt12_offer: saved.offer.clone(),
            });
        }
        if handshake_msat > handshake_cap_msat {
            return Err("Saved setup fees exceed explicit acceptance".into());
        }
        AcceptedPathTerms {
            descriptors,
            round_msat,
            handshake_msat,
        }
    } else {
        acquire_path_terms(
            config,
            &relays,
            PathPriceLimits {
                round_msat: limits.per_round_msat,
                handshake_msat: handshake_cap_msat,
            },
        )
        .await?
    };
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
    if (quoted.round_msat > 0 || quoted.handshake_msat > 0) && wallet.is_none() {
        return Err("Paid path requires verified outgoing wallet".into());
    }
    for descriptor in &quoted.descriptors {
        let policy = FoundationPolicy::decode(&descriptor.terms)?;
        if (policy.rate_msat > 0 || policy.handshake_fee_msat > 0)
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
    super::circuit::pregen_extend_paid_circuit_hashes(&mut relays, 10);
    // A saved scope is authoritative before the first constructor claim too:
    // restore exact descriptors, note IDs and ten IDs, never requote/regenerate.
    if let Some(saved) = &saved_funding {
        for (relay, hop) in relays.iter_mut().zip(&saved.hops) {
            relay.payment_id_hashes_10 =
                Some(hop.initial_payment_ids.iter().map(hex::encode).collect());
        }
    }

    let expected = quoted
        .descriptors
        .iter()
        .cloned()
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
    let funding = if quoted.handshake_msat > 0 {
        let timeout = handshake_funding_timeout_ms
            .filter(|ms| (1..=300000).contains(ms))
            .ok_or("Positive setup fees require an explicit finite funding timeout")?;
        let fee_count = quoted
            .descriptors
            .iter()
            .try_fold(0u64, |n, d| -> Result<u64> {
                Ok(n + u64::from(FoundationPolicy::decode(&d.terms)?.handshake_fee_msat > 0))
            })?;
        let setup_max = quoted
            .handshake_msat
            .checked_add(
                per_hop_routing_fee_msat
                    .checked_mul(fee_count)
                    .ok_or("Setup routing cap overflow")?,
            )
            .ok_or("Setup cap overflow")?;
        let initial_max = setup_max
            .checked_add(quoted.round_msat)
            .and_then(|n| n.checked_add(routing))
            .ok_or("Initial funding cap overflow")?;
        if initial_max > limits.lifetime_msat || initial_max > limits.outstanding_msat {
            return Err("Setup plus first-round exposure exceeds accepted limits".into());
        }
        let saved = if let Some(saved) = saved_funding {
            saved
        } else {
            let clock = observe()?;
            if clock.now_ms < 0 || clock.uncertainty_ms > 5000 {
                return Err("Invalid trusted funding clock".into());
            }
            let opens = clock
                .now_ms
                .checked_sub(i64::try_from(clock.uncertainty_ms)?)
                .filter(|n| *n >= 0)
                .ok_or("Funding clock overflow")?;
            let closes = opens
                .checked_add(i64::try_from(timeout)?)
                .ok_or("Funding deadline overflow")?;
            let mut hops = Vec::new();
            for h in &expected {
                let mut operation = [0; 16];
                let mut payment_id = [0; 32];
                rand::rngs::OsRng.fill_bytes(&mut operation);
                rand::rngs::OsRng.fill_bytes(&mut payment_id);
                hops.push(HandshakeHop {
                    fingerprint: h.descriptor.fingerprint,
                    terms: h.descriptor.terms.to_vec(),
                    offer: h.descriptor.bolt12_offer.clone(),
                    operation,
                    payment_id,
                    initial_payment_ids: h.payment_ids,
                    fee_limit_msat: h.fee_limit_msat,
                });
            }
            let saved = HandshakeScope {
                scope,
                instance: inventory.instance,
                account: ledger.account_id(),
                lifetime_limit_msat: limits.lifetime_msat,
                outstanding_limit_msat: limits.outstanding_msat,
                per_round_limit_msat: limits.per_round_msat,
                window: AcceptanceWindow {
                    opens_at_ms: opens,
                    closes_at_ms: closes,
                },
                hops,
            };
            ledger.save_handshake_scope(&saved)?;
            saved
        };
        let wallet = wallet.as_ref().ok_or("Missing native setup wallet")?;
        for (index, h) in expected.iter().enumerate() {
            if FoundationPolicy::decode(&h.descriptor.terms)?.handshake_fee_msat == 0 {
                continue;
            }
            let record =
                fund_handshake_hop(&mut ledger, wallet, scope, index as u16 + 1, &mut || {
                    observe().map_err(|_| {
                        io::Error::new(io::ErrorKind::Other, "Trusted clock unavailable")
                    })
                })
                .await?;
            if record.state != OutgoingState::Settled || record.closed || record.fee_limit_breached
            {
                return Err("Setup funding remains uncertain or unusable; recover exact original operations".into());
            }
        }
        Some(saved)
    } else {
        None
    };
    // Recover only ephemeral proof from the exact settled native operation. No
    // proof enters saved context, financial records, status replies or logs.
    let mut proofs = Vec::new();
    if let Some(funding) = &funding {
        let wallet = wallet.as_ref().ok_or("Missing native setup wallet")?;
        for (index, h) in expected.iter().enumerate() {
            if FoundationPolicy::decode(&h.descriptor.terms)?.handshake_fee_msat == 0 {
                continue;
            }
            let now = observe()?;
            let proof = handshake_construction_proof(
                &mut ledger,
                wallet,
                scope,
                index as u16 + 1,
                now,
                &mut || {
                    observe().map_err(|_| {
                        io::Error::new(io::ErrorKind::Other, "Trusted clock unavailable")
                    })
                },
            )
            .await?;
            if proof.settlement().amount_msat
                != FoundationPolicy::decode(&h.descriptor.terms)?.handshake_fee_msat
            {
                return Err("Setup proof amount mismatch".into());
            }
            proofs.push((index, proof));
        }
        if funding.instance != control.inventory().await?.instance {
            return Err("Tor instance changed after prefunding".into());
        }
    }
    let context = serde_json::to_vec(
        &serde_json::json!({"limits":limits,"routing_fee_per_hop_msat":per_hop_routing_fee_msat,"handshake_msat":handshake_cap_msat,"handshake_funding_timeout_ms":handshake_funding_timeout_ms,
        "hops":expected.iter().map(|hop|serde_json::json!({"fingerprint":hex::encode(hop.descriptor.fingerprint),"terms":hex::encode(hop.descriptor.terms),"offer":hop.descriptor.bolt12_offer})).collect::<Vec<_>>() }),
    )?;
    let construction = observe()?;
    if construction.now_ms < 0 || construction.uncertainty_ms > 5000 {
        return Err("Invalid trusted construction clock".into());
    }
    let uncertainty = i64::try_from(construction.uncertainty_ms)?;
    for (index, proof) in &proofs {
        let paid = proof.settlement();
        let policy = FoundationPolicy::decode(&expected[*index].descriptor.terms)?;
        let opens = construction
            .now_ms
            .checked_add(uncertainty)
            .and_then(|n| n.checked_sub(300000))
            .ok_or("Construction clock overflow")?;
        let closes = construction
            .now_ms
            .checked_sub(uncertainty)
            .and_then(|n| n.checked_add(i64::from(policy.setup_timeout_ms)))
            .ok_or("Construction clock overflow")?;
        if paid.settled_earliest_ms < opens || paid.settled_latest_ms > closes {
            return Err("Setup proof became too old or uncertain before construction".into());
        }
        relays[*index].payment_handshake_fee_payhash = Some(hex::encode(paid.payment_hash));
        relays[*index].payment_handshake_fee_preimage = Some(hex::encode(proof.preimage()));
    }

    let deadline = Instant::now() + Duration::from_millis(u64::from(setup_ms));
    let prepared =
        PreparedConstruction::new(scope, inventory.instance, &relays, &context, construction)?;
    let numeric = prepared.dispatch(&mut ledger, &mut control).await?;
    log::info!(
        "Foundation constructor receipt observed; durable claim retained, accepted plan pending"
    );
    let binding = await_constructed_binding(
        &mut control,
        numeric,
        inventory.instance,
        expected.len(),
        deadline,
    )
    .await?;
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
        // Re-inspection begins at hop one and therefore shares its one-GET/s
        // token. Respect that floor while the remaining cold hops complete.
        tokio::time::sleep_until((Instant::now() + Duration::from_secs(1)).min(deadline)).await;
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
    async fn cold_constructor_binding_waits_on_same_receipt_and_stops_on_terminal_rejection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = RpcConfig {
            addr: listener.local_addr().unwrap().to_string(),
            rpc_password: None,
            command: String::new(),
        };
        let server = tokio::spawn(async move {
            for terminal in [false, true] {
                let mut socket = BufReader::new(listener.accept().await.unwrap().0);
                for expected in ["AUTHENTICATE\r\n", "SETEVENTS ELTOR_PAYMENT_STATE\r\n"] {
                    let mut line = String::new();
                    socket.read_line(&mut line).await.unwrap();
                    assert_eq!(line, expected);
                    socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
                }
                for attempt in 0..if terminal { 1 } else { 3 } {
                    let mut line = String::new();
                    socket.read_line(&mut line).await.unwrap();
                    assert_eq!(line, "ELTOR_CIRCUIT_BIND 1 17\r\n");
                    let reply = if terminal {
                        "551 DEADLINE_EXPIRED\r\n".into()
                    } else if attempt < 2 {
                        "552 UNKNOWN_CIRCUIT\r\n".into()
                    } else {
                        format!(
                            "250 ELTOR_CIRCUIT 1 {} {} 3 1\r\n",
                            hex::encode([1; 16]),
                            hex::encode([2; 16])
                        )
                    };
                    socket.get_mut().write_all(reply.as_bytes()).await.unwrap();
                }
                let mut line = String::new();
                assert_eq!(socket.read_line(&mut line).await.unwrap(), 0);
            }
        });
        let mut control = PaymentControl::connect(&config).await.unwrap();
        let at = Instant::now();
        let binding =
            await_constructed_binding(&mut control, 17, [1; 16], 3, at + Duration::from_secs(2))
                .await
                .unwrap();
        assert!(at.elapsed() >= BIND_OBSERVATION_INTERVAL * 2);
        assert_eq!(binding.instance, [1; 16]);
        assert_eq!(binding.handle, [2; 16]);
        // An expired original budget cannot grant another observation.
        assert_eq!(
            await_constructed_binding(&mut control, 17, [1; 16], 3, Instant::now())
                .await
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::TimedOut
        );
        drop(control);
        let mut control = PaymentControl::connect(&config).await.unwrap();
        let error = await_constructed_binding(
            &mut control,
            17,
            [1; 16],
            3,
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(
            PaymentRejection::from_io(&error),
            Some(PaymentRejection::DeadlineExpired)
        );
        drop(control);
        server.await.unwrap();
    }
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
            0,
            None,
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
