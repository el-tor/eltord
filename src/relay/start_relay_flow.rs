// 1. Set Torrc Config
// 2. Handshake
// 3. Emit Event EXTENDPAIDCIRCUIT
// 4. Start Relay Payment Watcher
// 5. Init Payments Ledger
// 6. Start Lightning payment watcher
// 7. Payment Ledger Cron (Auditor Loop)

use crate::rpc::RpcConfig;
use crate::relay::simple_relay_selection_algo;

pub async fn start_relay_flow(control_port: u16, password: String) {
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
