use super::circuit;
use super::payments_sent_ledger;
use super::select_relay_algo;
use crate::client::payments_loop;
use crate::rpc::{wait_for_tor_bootstrap, wait_for_circuit_ready};
use crate::types::RpcConfig;
use crate::{client_info, client_warn};
use std::env;

/// Starts the client flow for building and managing circuits.
///
/// This function performs the following steps:
/// 1. Wait for Tor Bootstrap
/// 2. Relay Descriptor Lookup
/// 3. Handshake Fee (currently skipped)
/// 4. Pre-generate payment ID hashes for the circuit
/// 5. Circuit build
/// 6. Initialize Payments Ledger
/// 7. Client Bandwidth Watcher and payment loops, Circuit Kill and repeat
///
/// # Arguments
///
/// * `rpc_config` - Configuration for the RPC client.
///
/// # Notes
///
/// - The function uses smart caching: tries cached Tor data first (fast path ~1 sec). 
///     Tor needs new descriptors every hour for security purposes. 
///     (TODO: optimize to save 2-3 to have background process fetch new consensus every hour)
/// - Only forces SIGNAL RELOAD if bootstrap fails (slow path ~10-30 sec)
/// - Bootstrap detection uses the Tor control protocol's `GETINFO status/bootstrap-phase` command
/// - Tor automatically refreshes consensus hourly in the background (no user impact)
/// - The number of payment rounds is determined by the `PAYMENT_INTERVAL_ROUNDS` environment variable, defaulting to 10 if not set.
/// - The function selects relays using a simple relay selection algorithm and builds a circuit with the selected relays.
/// - A backup circuit is planned but not yet implemented.
/// - Bandwidth testing and client bandwidth watcher are placeholders for future implementation.
/// - The function is designed to loop for building and managing multiple circuits, but the loop is currently commented out.
pub async fn start_client_flow(rpc_config: &RpcConfig) -> tokio::task::JoinHandle<()> {
    let rpc_config = rpc_config.clone();
    
    tokio::spawn(async move {
        loop {
            let next = client_flow_impl(&rpc_config).await;
            if next {
                client_info!("Next Circuit...");
            } else {
                // Retry after a short delay
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await; // 10 seconds
                client_info!("Retrying due to payment loop error...");
            }
        }
    })
}

async fn client_flow_impl(rpc_config: &RpcConfig) -> bool {
    // loop {
    
    // 1. Wait for Tor Bootstrap
    client_info!("Verifying Tor is ready...");
    // Check if Tor already has fresh data (uses cache)
    let bootstrap_result = wait_for_tor_bootstrap(&rpc_config, 10).await;
    if let Err(e) = bootstrap_result {
        // Bootstrap failed - Tor might have stale data or not be ready
        client_warn!("Initial bootstrap check failed: {}. Attempting SIGNAL RELOAD...", e);
        // Last resort: Force Tor to reload and fetch fresh consensus/descriptors
        let reload_config = crate::types::RpcConfig {
            addr: rpc_config.addr.clone(),
            rpc_password: rpc_config.rpc_password.clone(),
            command: "SIGNAL RELOAD".to_string(),
        };
        if let Err(reload_err) = crate::rpc::rpc_client(reload_config).await {
            client_warn!("Failed to send RELOAD signal to Tor: {}", reload_err);
        }
        // Give Tor a moment to start the reload process
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        // Final attempt: Wait for bootstrap to complete after reload
        client_info!("Waiting for Tor to bootstrap after reload...");
        if let Err(final_err) = wait_for_tor_bootstrap(&rpc_config, 30).await {
            client_warn!("Failed to bootstrap Tor after reload: {}. Retrying...", final_err);
            return false; // Retry immediately
        }
    }
    client_info!("Tor ready to build circuits.");

    let lightning_wallet = match crate::lightning::load_wallet(&rpc_config).await {
        Ok(wallet) => wallet,
        Err(e) => {
            client_warn!("Failed to load Lightning wallet: {}. Client will continue without Lightning functionality.", e);
            client_warn!("To fix this, update the PaymentLightningNodeConfig in your torrc file with valid Lightning node credentials");
            return false; // Retry immediately
        }
    };

    let payment_rounds: u16 = env::var("PAYMENT_INTERVAL_ROUNDS")
        .unwrap_or(10.to_string())
        .parse()
        .unwrap();

    // 2. Relay Descriptor Lookup
    let mut selected_relays = select_relay_algo::simple_relay_selection_algo(&rpc_config)
        .await
        .unwrap();
    client_info!(
        "Build circuit EXTENDPAIDCIRCUIT with these selected relays"
    );
    client_info!("Selected relays: {:?}", &selected_relays);

    // Handle empty selected_relays set - retry immediately
    if selected_relays.is_empty() {
        client_warn!("No relays found within fee range. Retrying immediately...");
        return false; // Retry immediately without waiting
    }

    // TODO backup circuit
    // let backup_selected_relays = simple_relay_selection_algo(&rpc_config).await.unwrap();

    // 3. Handshake Fee (simple algo is 0, so skip for now)

    // 4. Pregenerate payment id hashes for the circuit
    // TODO for bolt11 get a real payment hash from the invoice via the lightning node, like LND
    circuit::pregen_extend_paid_circuit_hashes(&mut selected_relays, payment_rounds);

    // 5. Circuit build
    // EXTENDPAIDCIRCUIT
    let circuit_id = circuit::build_circuit(&rpc_config, &selected_relays)
        .await
        .unwrap();
    client_info!("Created paid Circuit with ID: {}", circuit_id);
    
    // 5a. Wait for circuit to be BUILT before allowing SOCKS connections
    // This is critical: circuit ID is assigned immediately (LAUNCHED state),
    // but SOCKS connections will fail until the circuit reaches BUILT state.
    // Circuit building can take 2-10 seconds for a 3-hop circuit.
    client_info!("Waiting for circuit {} to be fully built...", circuit_id);
    wait_for_circuit_ready(&rpc_config, &circuit_id, 30)
        .await
        .unwrap();

    // 6. Init Payments Ledger
    payments_sent_ledger::init_payments_sent_ledger(&selected_relays, &circuit_id);

    // 7. Start Payments Loop and client bandwidth watcher, Circuit Kill. Repeat
    let socks_port = crate::rpc::get_socks_port(rpc_config).await;
    client_info!("Using SOCKS port {} for bandwidth testing", socks_port);
    client_info!("✅ Circuit is BUILT and ready for traffic!");
    client_info!("Connect your browser via socks5 on (lookup your port from the torrc file) default port {}", socks_port);
    
    let payment_loop_result = payments_loop::start_payments_loop(
        rpc_config,
        &selected_relays,
        &circuit_id,
        lightning_wallet,
        socks_port,
    )
    .await;
    
    match payment_loop_result {
        Ok(_) => {
            client_info!("Payments loop completed successfully for circuit: {}", circuit_id);
            true // Wait before next iteration on success
        }
        Err(e) => {
            client_warn!("Payments loop encountered an error for circuit {}: {}", circuit_id, e);
            false // Retry immediately on error
        }
    }

    // => => loop logic above for the desired number of circuits (Tor typically has backup circuits in case one fails)
    // Tor typically builds 3 circuits: one primary and two backups, but for our use case since it a paid circuit let just have 1 backup
    // for _ in 0..2 {
    // logic from 7.
    // }
    //}
}
