use crate::rpc;
use crate::types::{ConsensusRelay, RelayTag};
use crate::types::{Relay, RpcConfig};
use log::{debug, info};
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::error::Error;
use std::sync::{Arc, Mutex};

// Simple Relay Selection Algo
// 1. Pick 3 relays, 1 entry, 1 middle, 1 exit at random
// 2. Make sure the total amount is under the PaymentCircuitMaxFee (from torrc config)
// 3. Prefer 0 handshake fee
// TODO optimize this algo as more relays are added (not currently optimized)
pub async fn simple_relay_selection_algo(
    rpc_config: &RpcConfig,
) -> Result<Vec<Relay>, Box<dyn Error>> {
    let relays = rpc::get_relay_descriptors(&rpc_config).await.unwrap();
    // Ok(relays)
    // Assuming PaymentCircuitMaxFee is defined somewhere
    let payment_circuit_max_fee = rpc::get_conf_payment_circuit_max_fee(&rpc_config)
        .await
        .unwrap_or(11000);
    info!("PaymentCircuitMaxFee: {}", payment_circuit_max_fee);

    // Filter out relays with a handshake fee, i.e., where payment_handshake_fee is null
    let filtered_relays: Vec<&Relay> = relays
        .iter()
        .filter(|relay| relay.payment_handshake_fee.is_none())
        .collect();

    // Get relays then sort by guard, middle, exit
    let consensus_relays = rpc::get_current_consensus(&rpc_config).await.unwrap();
    let consensus_relays: Vec<ConsensusRelay> = consensus_relays
        .into_iter()
        .filter(|r| r.tags.contains(&RelayTag::Running))
        .collect();
    let mut guard_relays = Vec::new();
    let mut middle_relays = Vec::new();
    let mut exit_relays = Vec::new();
    
    // Get preferred entry and exit nodes from torrc
    let preferred_entry_relays = rpc::get_conf_entry_nodes(&rpc_config).await;
    let preferred_exit_relays = rpc::get_conf_exit_nodes(&rpc_config).await;
    
    // TODO preferred_exit_fingerprints might contain a relay name, need to handle if its a nickname and then lookup the fingerprint
    for r in &consensus_relays {
        let preferred_exit_fingerprint = &Some(preferred_exit_relays.clone().unwrap().value);

        if r.tags.contains(&RelayTag::Guard) {
            // If EntryNodes is configured, only add guards that match
            if let Some(ref entry_config) = preferred_entry_relays {
                if entry_config.value.as_str() == r.fingerprint
                    && filtered_relays
                        .iter()
                        .filter(|relay| {
                            preferred_exit_fingerprint.as_ref() != Some(&relay.fingerprint)
                        })
                        .any(|relay| relay.fingerprint == r.fingerprint)
                {
                    guard_relays.push(r);
                }
            } else {
                // No EntryNodes configured, use all guard relays
                if filtered_relays
                    .iter()
                    .filter(|relay| {
                        preferred_exit_fingerprint.as_ref() != Some(&relay.fingerprint)
                    })
                    .any(|relay| relay.fingerprint == r.fingerprint)
                {
                    guard_relays.push(r);
                }
            }
        }
        if r.tags.contains(&RelayTag::Running) {
            if filtered_relays
                .iter()
                .filter(|relay| preferred_exit_fingerprint.as_ref() != Some(&relay.fingerprint))
                .any(|relay| relay.fingerprint == r.fingerprint)
            {
                middle_relays.push(r);
            }
        }
        if r.tags.contains(&RelayTag::Exit) {
            if preferred_exit_fingerprint.is_some()
                && !preferred_exit_fingerprint.as_ref().unwrap().is_empty()
            {
                // TODO: if value of ExitNodes is {us},{de} etc.. then find an exit in that country
                // TODO: also check if StrictNodes is set in torrc
                // TODO: if value is nickname then look fingerprint from nickname
                if preferred_exit_fingerprint.as_ref().unwrap().as_str() == &r.fingerprint {
                    exit_relays.push(r);
                }
            } else {
                if filtered_relays
                    .iter()
                    .filter(|relay| preferred_exit_fingerprint.as_ref() != Some(&relay.fingerprint))
                    .any(|relay| relay.fingerprint == r.fingerprint)
                {
                    exit_relays.push(r);
                }
            }
        }
        info!("{:?}", r);
    }

    // Retry up to 10 times to find a circuit within max fee range
    const MAX_RETRIES: u32 = 10;
    let rng = Arc::new(Mutex::new(SmallRng::from_entropy()));

    for attempt in 1..=MAX_RETRIES {
        debug!("Relay selection attempt {}/{}", attempt, MAX_RETRIES);

        // Shuffle the filtered relays for this attempt
        {
            let mut rng = rng.lock().unwrap();
            guard_relays.shuffle(&mut *rng);
            middle_relays.shuffle(&mut *rng);
            exit_relays.shuffle(&mut *rng);
        }

        // Pick 1 entry, 1 middle, 1 exit relay
        let mut selected_relays = Vec::new();

        // Entry
        if let Some(guard) = guard_relays.iter().find(|&&r| !selected_relays.contains(r)) {
            selected_relays.push((*guard).clone());
        }
        // Middle
        if let Some(middle) = middle_relays
            .iter()
            .find(|&&r| !selected_relays.contains(r))
        {
            selected_relays.push((*middle).clone());
        }
        // Exit
        if let Some(exit) = exit_relays.iter().find(|&&r| !selected_relays.contains(r)) {
            selected_relays.push((*exit).clone());
        }

        if selected_relays.len() != 3 {
            debug!("Could not find 3 suitable relays on attempt {}", attempt);
            continue;
        }

        let mut matched_relays: Vec<Relay> = selected_relays
            .iter()
            .filter_map(|consensus_relay| {
                filtered_relays
                    .iter()
                    .find(|relay| relay.fingerprint == consensus_relay.fingerprint)
                    .map(|relay| (*relay).clone())
            })
            .collect();

        // Check if the circuit is under the maximum fee for 10 rounds
        if !is_circuit_under_max_fee(payment_circuit_max_fee as u32, &matched_relays) {
            debug!(
                "Circuit exceeds maximum fee on attempt {}, retrying...",
                attempt
            );
            continue;
        }

        // Success! Add tags and hop numbers
        let mut i = 1;
        for relay in matched_relays.iter_mut() {
            relay.relay_tag = Some(match i {
                1 => RelayTag::Guard,
                2 => RelayTag::Middle,
                3 => RelayTag::Exit,
                _ => unreachable!(),
            });
            relay.hop = Some(i);
            i += 1;
        }

        info!(
            "Successfully found circuit within fee limit on attempt {}/{}",
            attempt, MAX_RETRIES
        );
        return Ok(matched_relays);
    }

    // If we get here, all attempts failed
    // Return empty vector instead of error to keep daemon running
    info!(
        "Warning: Failed to find a circuit within maximum fee of {} msats after {} attempts. Returning empty circuit.", 
        payment_circuit_max_fee, MAX_RETRIES
    );
    Ok(Vec::new())
}

/// Checks if 10 rounds of payments for the selected relays do not exceed the max_fee
///
/// # Arguments
/// * `max_fee` - Maximum fee allowed for the circuit in millisatoshis
/// * `selected_relays` - Vector of relays in the circuit
///
/// # Returns
/// * `true` if the total cost for 10 rounds is under or equal to max_fee
/// * `false` if the total cost exceeds max_fee
fn is_circuit_under_max_fee(max_fee: u32, selected_relays: &[Relay]) -> bool {
    let rounds = 10;
    let mut total_cost = 0u32;

    for relay in selected_relays {
        // Get the payment rate per round for this relay
        let payment_rate = relay.payment_rate_msats.unwrap_or(0);

        // Add the cost for 10 rounds of this relay
        total_cost = total_cost.saturating_add(payment_rate.saturating_mul(rounds));

        // Early exit if we've already exceeded the max fee
        if total_cost >= max_fee {
            debug!(
                "Circuit exceeds max fee: {} msats > {} msats (relay: {})",
                total_cost, max_fee, relay.nickname
            );
            return false;
        }
    }

    debug!(
        "Circuit total cost for {} rounds: {} msats (max: {} msats)",
        rounds, total_cost, max_fee
    );

    total_cost <= max_fee
}

// TODO: implement more complicated relay selection algos
