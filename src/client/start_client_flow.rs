
use crate::rpc::RpcConfig;
use crate::client::simple_relay_selection_algo;

// 1. Relay Descriptor Lookup
// 2. Handshake Fee
// 3. Circuit build
// 4. Test Bandwidth
// 5. Init Payments Ledger
// 6. Client Bandwidth Watcher
// 7. Circuit Kill. Repeat
pub async fn start_client_flow(control_port: u16, password: String) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        let rpc_config = RpcConfig {
            addr: format!("127.0.0.1:{}", control_port),
            rpc_password: password.clone(),
            command: "".into(),
        };

        let selected_relays = simple_relay_selection_algo(rpc_config).await.unwrap();
        println!("{:?}", selected_relays);
    }
}
