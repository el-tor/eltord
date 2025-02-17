use crate::rpc::{
    get_conf, get_conf_payment_circuit_max_fee, get_current_consensus, get_relay_descriptors, Relay, RelayTag, RpcConfig
};
use futures_util::TryFutureExt;
use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::error::Error;
use std::sync::{Arc, Mutex};

// Simple Relay Selection Algo
// 1. Pick 3 relays, 1 entry, 1 middle, 1 exit at random
// 2. Make sure the total amount is under the PaymentCircuitMaxFee (from torrc config)
// 3. Prefer 0 handshake fee
pub async fn simple_relay_selection_algo(
    rpc_config: RpcConfig,
) -> Result<Vec<Relay>, Box<dyn Error>> {
    let relays = get_relay_descriptors(&rpc_config).await.unwrap();
    // Ok(relays)
    // Assuming PaymentCircuitMaxFee is defined somewhere
    let payment_circuit_max_fee = get_conf_payment_circuit_max_fee(&rpc_config).await.unwrap();
    println!("PaymentCircuitMaxFee: {}", payment_circuit_max_fee);

    // Filter out relays with a handshake fee, i.e., where payment_handshake_fee is null
    let mut filtered_relays: Vec<&Relay> =  relays
        .iter()
        .filter(|relay| relay.payment_handshake_fee.is_none())
        .collect();

    // Get relays then sort by guard, middle, exit
    let consensus_relays = get_current_consensus(&rpc_config).await.unwrap();
    let mut guard_relays = Vec::new();
    let mut middle_relays = Vec::new();
    let mut exit_relays = Vec::new();
    for r in &consensus_relays {
        if r.tags.contains(&RelayTag::Guard) {
            guard_relays.push(r);
        }
        if r.tags.contains(&RelayTag::Middle) {
            middle_relays.push(r);
        }
        if r.tags.contains(&RelayTag::Exit) {
            exit_relays.push(r);
        }
        println!("{:?}", r);
    }

    // Shuffle the filtered relays
    let rng = Arc::new(Mutex::new(SmallRng::from_entropy()));
    {
        let mut rng = rng.lock().unwrap();
        filtered_relays.shuffle(&mut *rng);
    }

    // Pick 1 entry, 1 middle, 1 exit relay
    let mut selected_relays = Vec::new();
    let mut total_fee = 0;

    // 1. find an exit relay

    for relay in filtered_relays {
        if selected_relays.len() == 3 {
            break;
        }

        // TODO Check if adding this relay would exceed the max fee
        // if total_fee + relay.interval_fee + relay.rounds_total_fee <= payment_circuit_max_fee {
        //     selected_relays.push(relay.clone());
        //     total_fee += relay.interval_fee + relay.rounds_total_fee;
        // }
        selected_relays.push(relay.clone());
    }

    if selected_relays.len() != 3 {
        return Err("Could not find suitable relays".into());
    }

    Ok(selected_relays.into_iter().clone().collect())
}

// TODO: implement more complicated relay selection algos
