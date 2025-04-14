use lni::types::ListTransactionsParams;

use crate::types::RpcConfig;

// 1. Set Torrc Config
// 2. Handshake
// 3. Emit Event EXTENDPAIDCIRCUIT
// 4. Start Relay Payment Watcher
// 5. Init Payments Ledger
// 6. Start Lightning payment watcher
// 7. Payment Ledger Cron (Auditor Loop)
pub async fn start_relay_flow(rpc_config: &RpcConfig) {
    // TODO test gettting all payments received from lightning node
    // tokio::time::sleep(tokio::time::Duration::from_secs(6)).await;

    // let wallet = crate::lightning::load_wallet(rpc_config).await;

    // let params = ListTransactionsParams {
    //     from: 0,
    //     limit: 100,
    //     payment_hash: None,
    // };
    // match wallet.(params) {
    //     Ok(txns) => {
    //         println!("Relay recent transactions: {:?}", txns);
    //     }
    //     Err(e) => {
    //         panic!("Failed to list transactions: {:?}", e);
    //     }
    // }
}
