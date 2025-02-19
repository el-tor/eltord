use crate::rpc;
use crate::rpc::{ConsensusRelay, RelayTag};
use crate::types::{Relay, RpcConfig};
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
        .unwrap();
    println!("PaymentCircuitMaxFee: {}", payment_circuit_max_fee);

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
    for r in &consensus_relays {
        if r.tags.contains(&RelayTag::Guard) {
            if (filtered_relays
                .iter()
                .any(|relay| relay.fingerprint == r.fingerprint))
            {
                guard_relays.push(r);
            }
        }
        if r.tags.contains(&RelayTag::Running) {
            if (filtered_relays
                .iter()
                .any(|relay| relay.fingerprint == r.fingerprint))
            {
                middle_relays.push(r);
            }
        }
        if r.tags.contains(&RelayTag::Exit) {
            if (filtered_relays
                .iter()
                .any(|relay| relay.fingerprint == r.fingerprint))
            {
                exit_relays.push(r);
            }
        }
        println!("{:?}", r);
    }

    // Shuffle the filtered relays
    let rng = Arc::new(Mutex::new(SmallRng::from_entropy()));
    {
        let mut rng = rng.lock().unwrap();
        guard_relays.shuffle(&mut *rng);
        middle_relays.shuffle(&mut *rng);
        exit_relays.shuffle(&mut *rng);
    }

    // Pick 1 entry, 1 middle, 1 exit relay
    let mut selected_relays = Vec::new();
    let mut total_fee = 0;

    if let Some(guard) = guard_relays.iter().find(|&&r| !selected_relays.contains(r)) {
        selected_relays.push((*guard).clone());
    }
    if let Some(middle) = middle_relays
        .iter()
        .find(|&&r| !selected_relays.contains(r))
    {
        selected_relays.push((*middle).clone());
    }
    if let Some(exit) = exit_relays.iter().find(|&&r| !selected_relays.contains(r)) {
        selected_relays.push((*exit).clone());
    }

    let mut total_fee = 0;

    // TODO calculate within max fee range here

    if selected_relays.len() != 3 {
        return Err("Could not find suitable relays".into());
    }

    let matched_relays: Vec<Relay> = selected_relays
        .iter()
        .filter_map(|consensus_relay| {
            filtered_relays
                .iter()
                .find(|relay| relay.fingerprint == consensus_relay.fingerprint)
                .map(|relay| (*relay).clone())
        })
        .collect();

    Ok(matched_relays)
}

// TODO: implement more complicated relay selection algos
