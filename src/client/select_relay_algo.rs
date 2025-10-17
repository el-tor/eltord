use crate::rpc;
use crate::types::{ConsensusRelay, RelayTag};
use crate::types::{Relay, RpcConfig};
use log::{debug, info, warn};
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
    
    let payment_circuit_max_fee = rpc::get_conf_payment_circuit_max_fee(&rpc_config)
        .await
        .unwrap_or(11000);
    info!("PaymentCircuitMaxFee: {}", payment_circuit_max_fee);

    // Filter out relays with a handshake fee
    // TODO implement handshake fee budget
    let filtered_relays: Vec<&Relay> = relays
        .iter()
        .filter(|relay| relay.payment_handshake_fee.is_none())
        .collect();

    // Get consensus relays
    let consensus_relays = rpc::get_current_consensus(&rpc_config).await.unwrap();
    let consensus_relays: Vec<ConsensusRelay> = consensus_relays
        .into_iter()
        .filter(|r| r.tags.contains(&RelayTag::Running))
        .collect();
    
    // Get preferred entry and exit nodes from torrc
    let preferred_entry_relays = rpc::get_conf_entry_nodes(&rpc_config).await;
    let preferred_exit_relays = rpc::get_conf_exit_nodes(&rpc_config).await;
    
    // Categorize relays by role
    let (guard_relays, middle_relays, exit_relays) = categorize_relays(
        &consensus_relays,
        &filtered_relays,
        preferred_entry_relays.as_ref(),
        preferred_exit_relays.as_ref(),
    );

    info!("Available relays - Guards: {}, Middle: {}, Exit: {}", 
          guard_relays.len(), middle_relays.len(), exit_relays.len());

    if guard_relays.is_empty() {
        warn!("No guard relays available! Check your EntryNodes configuration or relay availability.");
        return Ok(Vec::new());
    }
    if exit_relays.is_empty() {
        warn!("No exit relays available! Check your ExitNodes configuration or relay availability.");
        return Ok(Vec::new());
    }
    if middle_relays.is_empty() {
        warn!("No middle relays available!");
        return Ok(Vec::new());
    }

    // Try to find a circuit within fee limits
    select_circuit_within_fee_limit(
        payment_circuit_max_fee as u32,
        guard_relays,
        middle_relays,
        exit_relays,
        &filtered_relays,
        &consensus_relays,
        preferred_entry_relays.as_ref(),
        preferred_exit_relays.as_ref(),
    )
}

/// Categorizes consensus relays into guard, middle, and exit pools
/// Returns (guards, middles, exits) as vectors of ConsensusRelay references
/// Strategy: Build pools of ALL available relays by role, preferences will be applied later
fn categorize_relays<'a>(
    consensus_relays: &'a [ConsensusRelay],
    filtered_relays: &[&Relay],
    _preferred_entry_relays: Option<&crate::rpc::TorrcEntry>,
    _preferred_exit_relays: Option<&crate::rpc::TorrcEntry>,
) -> (Vec<&'a ConsensusRelay>, Vec<&'a ConsensusRelay>, Vec<&'a ConsensusRelay>) {
    let mut guard_relays = Vec::new();
    let mut middle_relays = Vec::new();
    let mut exit_relays = Vec::new();

    for relay in consensus_relays {
        // Check if relay is in our filtered list (no handshake fee)
        let is_available = filtered_relays
            .iter()
            .any(|r| r.fingerprint == relay.fingerprint);
        
        if !is_available {
            continue;
        }

        // Categorize all available relays by their capabilities
        if relay.tags.contains(&RelayTag::Guard) {
            guard_relays.push(relay);
        }
        
        if relay.tags.contains(&RelayTag::Running) {
            middle_relays.push(relay);
        }
        
        if relay.tags.contains(&RelayTag::Exit) {
            exit_relays.push(relay);
        }
    }

    (guard_relays, middle_relays, exit_relays)
}

/// Attempts to select a circuit within the fee limit
/// Strategy: First select random circuit, then apply EntryNodes/ExitNodes preferences
fn select_circuit_within_fee_limit(
    max_fee: u32,
    mut guard_relays: Vec<&ConsensusRelay>,
    mut middle_relays: Vec<&ConsensusRelay>,
    mut exit_relays: Vec<&ConsensusRelay>,
    filtered_relays: &[&Relay],
    consensus_relays: &[ConsensusRelay],
    preferred_entry_relays: Option<&crate::rpc::TorrcEntry>,
    preferred_exit_relays: Option<&crate::rpc::TorrcEntry>,
) -> Result<Vec<Relay>, Box<dyn Error>> {
    const MAX_RETRIES: u32 = 10;
    let rng = Arc::new(Mutex::new(SmallRng::from_entropy()));

    for attempt in 1..=MAX_RETRIES {
        debug!("Relay selection attempt {}/{}", attempt, MAX_RETRIES);

        // Shuffle for randomness
        {
            let mut rng = rng.lock().unwrap();
            guard_relays.shuffle(&mut *rng);
            middle_relays.shuffle(&mut *rng);
            exit_relays.shuffle(&mut *rng);
        }

        // Try to pick one of each type
        let selected_consensus = match select_three_relays(
            &guard_relays,
            &middle_relays,
            &exit_relays,
        ) {
            Some(relays) => relays,
            None => {
                debug!("Could not find 3 suitable relays on attempt {}", attempt);
                continue;
            }
        };

        // Match consensus relays to full relay descriptors
        let mut matched_relays: Vec<Relay> = selected_consensus
            .iter()
            .filter_map(|consensus_relay| {
                filtered_relays
                    .iter()
                    .find(|relay| relay.fingerprint == consensus_relay.fingerprint)
                    .map(|relay| (*relay).clone())
            })
            .collect();

        if matched_relays.len() != 3 {
            debug!("Could not match all 3 relays to descriptors on attempt {}", attempt);
            continue;
        }

        // Apply EntryNodes preference: replace guard (first hop) if configured
        if let Some(preferred_entry) = preferred_entry_relays {
            let preferred_fingerprint = &preferred_entry.value;
            if let Some(preferred_relay) = filtered_relays
                .iter()
                .find(|r| &r.fingerprint == preferred_fingerprint)
            {
                info!("Replacing guard with preferred EntryNode: {}", preferred_relay.nickname);
                matched_relays[0] = (*preferred_relay).clone();
            } else {
                warn!("Configured EntryNode {} not found in available relays, using random guard", preferred_fingerprint);
            }
        }

        // Apply ExitNodes preference: replace exit (third hop) if configured
        if let Some(preferred_exit) = preferred_exit_relays {
            let preferred_fingerprint = &preferred_exit.value;
            if let Some(preferred_relay) = filtered_relays
                .iter()
                .find(|r| &r.fingerprint == preferred_fingerprint)
            {
                info!("Replacing exit with preferred ExitNode: {}", preferred_relay.nickname);
                matched_relays[2] = (*preferred_relay).clone();
            } else {
                warn!("Configured ExitNode {} not found in available relays, using random exit", preferred_fingerprint);
            }
        }

        // Check fee limit (after applying preferences)
        if !is_circuit_under_max_fee(max_fee, &matched_relays) {
            debug!("Circuit exceeds maximum fee on attempt {}, retrying...", attempt);
            continue;
        }

        // Success! Tag and number the hops
        tag_circuit_relays(&mut matched_relays);

        info!(
            "✅ Successfully found circuit within fee limit on attempt {}/{}",
            attempt, MAX_RETRIES
        );
        info!("   Guard: {}", matched_relays[0].nickname);
        info!("   Middle: {}", matched_relays[1].nickname);
        info!("   Exit: {}", matched_relays[2].nickname);
        
        return Ok(matched_relays);
    }

    // All attempts failed
    warn!(
        "Failed to find a circuit within maximum fee of {} msats after {} attempts",
        max_fee, MAX_RETRIES
    );
    Ok(Vec::new())
}

/// Selects one guard, one middle, and one exit relay (ensuring no duplicates)
fn select_three_relays<'a>(
    guard_relays: &[&'a ConsensusRelay],
    middle_relays: &[&'a ConsensusRelay],
    exit_relays: &[&'a ConsensusRelay],
) -> Option<Vec<ConsensusRelay>> {
    let mut selected = Vec::new();

    // Pick guard
    let guard = guard_relays.iter().find(|&&r| !selected.contains(r))?;
    selected.push((*guard).clone());

    // Pick middle (must be different from guard)
    let middle = middle_relays
        .iter()
        .find(|&&r| !selected.contains(r))?;
    selected.push((*middle).clone());

    // Pick exit (must be different from guard and middle)
    let exit = exit_relays
        .iter()
        .find(|&&r| !selected.contains(r))?;
    selected.push((*exit).clone());

    Some(selected)
}

/// Tags relays with their role and hop number
fn tag_circuit_relays(relays: &mut [Relay]) {
    for (i, relay) in relays.iter_mut().enumerate() {
        let hop = (i + 1) as i64;
        relay.relay_tag = Some(match hop {
            1 => RelayTag::Guard,
            2 => RelayTag::Middle,
            3 => RelayTag::Exit,
            _ => unreachable!(),
        });
        relay.hop = Some(hop);
    }
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
