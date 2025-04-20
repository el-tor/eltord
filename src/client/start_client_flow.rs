use super::circuit;
use super::payments_ledger;
use super::select_relay_algo;
use crate::client::payments_loop;
use crate::types::RpcConfig;
use std::env;

/// Starts the client flow for building and managing circuits.
///
/// This function performs the following steps:
/// 1. Relay Descriptor Lookup
/// 2. Handshake Fee (currently skipped)
/// 3. Pre-generate payment ID hashes for the circuit
/// 4. Circuit build
/// 5. Test Bandwidth (currently not implemented)
/// 6. Initialize Payments Ledger
/// 7. Client Bandwidth Watcher and payment loops, Circuit Kill and repeat
///
/// # Arguments
///
/// * `rpc_config` - Configuration for the RPC client.
///
/// # Notes
///
/// - The function currently sleeps for 6 seconds before starting the flow.
/// - The number of payment rounds is determined by the `PAYMENT_INTERVAL_ROUNDS` environment variable, defaulting to 10 if not set.
/// - The function selects relays using a simple relay selection algorithm and builds a circuit with the selected relays.
/// - A backup circuit is planned but not yet implemented.
/// - Bandwidth testing and client bandwidth watcher are placeholders for future implementation.
/// - The function is designed to loop for building and managing multiple circuits, but the loop is currently commented out.
pub async fn start_client_flow(rpc_config: &RpcConfig) {
    // loop {
    tokio::time::sleep(tokio::time::Duration::from_secs(6)).await;

    let lightning_wallet = crate::lightning::load_wallet(&rpc_config).await;

    let payment_rounds: u16 = env::var("PAYMENT_INTERVAL_ROUNDS")
        .unwrap_or(10.to_string())
        .parse()
        .unwrap();

    // 1. Relay Descriptor Lookup
    let mut selected_relays = select_relay_algo::simple_relay_selection_algo(&rpc_config)
        .await
        .unwrap();
    println!(
        "Build circuit EXTENDPAIDCIRCUIT with these selected relays",
    );
    dbg!(&selected_relays);
    // TODO backup circuit
    // let backup_selected_relays = simple_relay_selection_algo(&rpc_config).await.unwrap();

    // 2. Handshake Fee (simple algo is 0, so skip for now)

    // 3. Pregenerate payment id hashes for the circuit
    // TODO for bolt11 get a real payment hash from the invoice via the lightning node, like LND
    circuit::pregen_extend_paid_circuit_hashes(&mut selected_relays, payment_rounds);

    // 4. Circuit build
    // EXTENDPAIDCIRCUIT
    let circuit_id = circuit::build_circuit(&rpc_config, &selected_relays)
        .await
        .unwrap();
    println!("Created paid Circuit with ID: {}", circuit_id);
    println!("Connect your browser via sock5 on: {}", 18057); // TODO remove hardcodded socks5 port

    // 5. Test Bandwidth
    // TODO: Implement bandwidth test

    // 6. Init Payments Ledger
    payments_ledger::init_payments_ledger(&selected_relays, &circuit_id);

    // 7. Start Payments Loop and client bandwidth watcher, Circuit Kill. Repeat
    let payment_loop_result = payments_loop::start_payments_loop(
        rpc_config,
        &selected_relays,
        &circuit_id,
        lightning_wallet,
    )
    .await;

    // => => loop logic above for the desired number of circuits (Tor typically has backup circuits in case one fails)
    // Tor typically builds 3 circuits: one primary and two backups, but for our use case since it a paid circuit let just have 1 backup
    // for _ in 0..2 {
    // logic from 7.
    // }
    //}
}
