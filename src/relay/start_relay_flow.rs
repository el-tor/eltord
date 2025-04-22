use super::payments_watcher::start_payments_watcher;
use crate::{rpc::get_torrc_value, types::RpcConfig};

// 1. Torrc Config
// 2. Handshake Fee?
// 3. Start payment watcher
// 4. Listen for the Event PAYMENT_ID_HASH_RECEIVED
//      4a. On PAYMENT_ID_HASH_RECEIVED write a row to the ledger
//      4b. Then kick off OnInvoiceEvents (Auditor Loop)
//          - 4b1. Loop: Kill circuit if payment is not received within window
pub async fn start_relay_flow(rpc_config: &RpcConfig) {
    tokio::time::sleep(tokio::time::Duration::from_secs(6)).await;

    let rpc_config = rpc_config.clone();

    let wallet = crate::lightning::load_wallet(&rpc_config).await;

    // 1. Torrc Config 
    //    Did you (the relay) set your BOLT12 offer in the torrc?
    let conf = get_torrc_value(&rpc_config, &["PaymentBolt12Offer".to_string()]).await;
    let bolt12 = conf
        .iter()
        .find(|e| e.key == "PaymentBolt12Offer")
        .map(|entry| entry.value.clone());
    dbg!(bolt12.clone());
    if !bolt12.is_some() {
        println!("BOLT12 offer not found in torrc config. Running in free mode.");
    }

    // 2. Handshake Fee?
    // TODO

    // 3 + 4. Start the payment watcher 
    let rpc_config_clone = rpc_config.clone();
    tokio::spawn(async move {
        start_payments_watcher(&rpc_config_clone, &*wallet).await;
    });
}
