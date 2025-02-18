
use crate::rpc::{self, RpcConfig};
use crate::client::{build_circuit, simple_relay_selection_algo};

// 1. Relay Descriptor Lookup
// 2. Handshake Fee
// 3. Circuit build
// 4. Test Bandwidth
// 5. Init Payments Ledger
// 6. Client Bandwidth Watcher
// 7. Circuit Kill. Repeat
pub async fn start_client_flow(rpc_config: RpcConfig) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(6)).await;
        
        // 1. Relay Descriptor Lookup
        let selected_relays = simple_relay_selection_algo(&rpc_config).await.unwrap();
        println!("Build circuit EXTENDPAIDCIRCUIT with these selected relays {:?}", selected_relays);

        // 2. Handshake Fee (simple algo is 0, so skip for now)

        // 3. Circuit build
        // EXTENDPAIDCIRCUIT
        let circuit_id = build_circuit(&rpc_config, selected_relays).await.unwrap();
        println!("Created paid Circuit with ID: {}", circuit_id);


        // 4. Test Bandwidth

        // 5. Init Payments Ledger
        // Make sure to pay each relay in the circuit out of band using lightning

        // 6. Client Bandwidth Watcher and payment loops

        // 7. Circuit Kill. Repeat

        // => => loop this for the desired number of circuits (Tor typically has backup circuits in case one fails)
        // Tor typically builds 3 circuits: one primary and two backups, but for our use case since it a paid circuit let just have 1 backup
        // for _ in 0..2 {
            // Implement the logic for building and managing circuits here
        // }
    }
}
