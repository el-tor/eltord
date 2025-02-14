use crate::rpc::{get_relay_descriptors, Relay, RpcConfig};
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::error::Error;

// Simple Relay Selection Algo
// 1. Pick 3 relays, 1 entry, 1 middle, 1 exit at random
// 2. Make sure the total amount is under the PaymentCircuitMaxFee (from torrc config)
// 3. Prefer 0 handshake fee
pub async fn simple_relay_selection_algo(
    rpc_config: RpcConfig,
) -> Result<Vec<Relay>, Box<dyn Error>> {
    let relays = get_relay_descriptors(rpc_config).await.unwrap();
    Ok(relays)
    // // Assuming PaymentCircuitMaxFee is defined somewhere
    // let payment_circuit_max_fee = 1000; // Placeholder value

    // // Filter relays with 0 handshake fee
    // let mut filtered_relays: Vec<&Relay> = relays.iter().filter(|relay| relay.payment_handshake_fee == Some(0)).collect();

    // // Shuffle the filtered relays
    // let mut rng = thread_rng();
    // filtered_relays.shuffle(&mut rng);

    // // Pick 1 entry, 1 middle, 1 exit relay
    // let mut selected_relays = Vec::new();
    // let mut total_fee = 0;

    // for relay in filtered_relays {
    //     if selected_relays.len() == 3 {
    //         break;
    //     }

    //     // Check if adding this relay would exceed the max fee
    //     if total_fee + relay.interval_fee + relay.rounds_total_fee <= payment_circuit_max_fee {
    //         selected_relays.push(relay.clone());
    //         total_fee += relay.interval_fee + relay.rounds_total_fee;
    //     }
    // }

    // if selected_relays.len() != 3 {
    //     return Err("Could not find suitable relays".into());
    // }

    // Ok(selected_relays.into_iter().cloned().collect())
}

// TODO: implement more complicated relay selection algos
