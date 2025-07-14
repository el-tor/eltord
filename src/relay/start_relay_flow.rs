use super::payments_watcher::start_payments_watcher;
use crate::{rpc::get_torrc_value, types::RpcConfig};
use log::{debug, error, info, warn};

// 1. Torrc Config
// 2. Start payment watcher
// 3. Listen for the Tor Event PAYMENT_ID_HASH_RECEIVED
//    - 3a. On PAYMENT_ID_HASH_RECEIVED write a row to the ledger
//    - 3b. Decode the payment_hashes via the 12 hash wire_format 
//           "handshake_payment_hash + handshake_preimage + payment_id_hash_round1 + payment_id_hash_round2 + ...payment_id_hash_round10"
//    - 3c. If you require a handshake fee check the handshake_payment_hash + handshake_preimage
//    - 3d. Write the payment_id_hash_round1 to payment_id_hash_round10 to the ledger
// 4. Then kick off OnLnInvoiceEvents (Auditor Loop)
//    - 4a. Loop: Kill circuit if payment is not received within window
pub async fn start_relay_flow(rpc_config: &RpcConfig) -> tokio::task::JoinHandle<()> {
    let rpc_config = rpc_config.clone();
    
    tokio::spawn(async move {
        relay_flow_impl(&rpc_config).await;
    })
}

async fn relay_flow_impl(rpc_config: &RpcConfig) {
    tokio::time::sleep(tokio::time::Duration::from_secs(6)).await;

    let wallet = match crate::lightning::load_wallet(&rpc_config).await {
        Ok(wallet) => wallet,
        Err(e) => {
            warn!("Failed to load Lightning wallet: {}. Relay will continue without Lightning functionality.", e);
            warn!("To fix this, update the PaymentLightningNodeConfig in your torrc file with valid Lightning node credentials");
            return;
        }
    };

    // 1. Torrc Config 
    //    Did you (the relay) set your BOLT12 offer in the torrc?
    let conf = get_torrc_value(&rpc_config, &["PaymentBolt12Offer".to_string()]).await;
    let bolt12 = conf
        .iter()
        .find(|e| e.key == "PaymentBolt12Offer")
        .map(|entry| entry.value.clone());
    info!("BOLT12 offer from torrc: {:?}", bolt12);
    if !bolt12.is_some() {
        info!("BOLT12 offer not found in torrc config. Running in free mode.");
    } else {
        info!("BOLT12 offer found in torrc config. Running in paid mode.");
    }

    // 2 - 4. Start the payment watcher 
    info!("Starting payment watcher...");
    let rpc_config_clone = rpc_config.clone();
    let payment_watcher_handle = tokio::spawn(async move {
        if let Err(e) = start_payments_watcher(&rpc_config_clone, &*wallet).await {
            error!("Payment watcher failed: {:?}", e);
        }
    });

    // Wait for the payment watcher to complete (it runs indefinitely)
    if let Err(e) = payment_watcher_handle.await {
        error!("Payment watcher task panicked: {:?}", e);
    }
}
