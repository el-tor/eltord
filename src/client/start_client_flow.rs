
use crate::rpc::{self, RpcConfig};
use crate::client::simple_relay_selection_algo;

// 1. Relay Descriptor Lookup
// 2. Handshake Fee
// 3. Circuit build
// 4. Test Bandwidth
// 5. Init Payments Ledger
// 6. Client Bandwidth Watcher
// 7. Circuit Kill. Repeat
pub async fn start_client_flow(rpc_config: RpcConfig) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        

        let selected_relays = simple_relay_selection_algo(rpc_config.clone()).await.unwrap();
        println!("{:?}", selected_relays);
    }
}
