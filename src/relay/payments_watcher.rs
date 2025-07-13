use crate::{
    relay::{self, init_payments_received_ledger, RelayPayments},
    rpc::rpc_event_listener,
    types::{EventCallback, RpcConfig},
};
use lni::{types::Transaction, LightningNode};
use log::{info, warn, debug};

// 2. Start payment watcher
pub async fn start_payments_watcher(
    config: &RpcConfig,
    wallet: &(dyn LightningNode + Send + Sync),
) -> Result<(), Box<dyn std::error::Error>> {
    // 3. Listen for the Event PAYMENT_ID_HASH_RECEIVED
    let event = "PAYMENT_ID_HASH_RECEIVED";
    let on_event_payment_id_hash_received_callback =
        Box::new(OnTorEventPaymentIdHashReceivedCallback {});
    let result = rpc_event_listener(
        config.clone(),
        event.to_string(),
        on_event_payment_id_hash_received_callback,
        wallet,
    )
    .await;
    Ok(())
}

// 3. Listen for the Event PAYMENT_ID_HASH_RECEIVED
struct OnTorEventPaymentIdHashReceivedCallback {}
impl EventCallback for OnTorEventPaymentIdHashReceivedCallback {
    fn success(&self, response: Option<String>, wallet: &(dyn LightningNode + Send + Sync)) {
        info!("Event response: {:?}", response);
        let mut circ_id= "UNKNOWN".to_string();
        let payment_hashes = if let Some(resp) = response.as_ref() {
            // EVENT WIRE_FORMAT "650 EVENT_PAYMENT_ID_HASH_RECEIVED <CIRC_ID> <PAYHASHES>"
            if resp.starts_with("650 EVENT_PAYMENT_ID_HASH_RECEIVED ") {
                let rest = &resp["650 EVENT_PAYMENT_ID_HASH_RECEIVED ".len()..];
                let mut parts = rest.splitn(2, ' ');
                circ_id = parts.next().unwrap_or("").to_string();
                let hashes_part = parts.next().unwrap_or("").to_string();
                Some(hashes_part)
            } else {
                None
            }
        } else {
            None
        };
        info!("Circuit ID: {:?}, Payment hashes: {:?}", circ_id, payment_hashes);

        if payment_hashes.is_some() {
            // 3a. On PAYMENT_ID_HASH_RECEIVED write a row to the ledger
            // 3b. Decode the payment_hashes from the wire_format
            let relay_payments = RelayPayments::from_wire_format(&payment_hashes.clone().unwrap());

            // 3c. If you require a handshake fee check the handshake_payment_hash + handshake_preimage
            // TODO verify handshake

            // 3d. Write the payment_id_hash_round1 thru payment_id_hash_round10 to the ledger
            init_payments_received_ledger(&relay_payments, &circ_id);

            // 4. Then kick off OnInvoiceEvents (Auditor Loop)
            // TODO: naaive implementation
            for i in 0..relay_payments.payhashes.len() {
                let current_round_payment_hash = relay_payments.payhashes[i].clone();
                info!(
                    "Round {}: Payment watcher for payment hash {} for circuit {}",
                    i, current_round_payment_hash, circ_id
                );
                let params = lni::types::OnInvoiceEventParams {
                    search: Some(current_round_payment_hash),
                    polling_delay_sec: 3,
                    max_polling_sec: 60,
                    ..Default::default()
                };
                let callback = OnLnInvoiceEventCallback {};
                tokio::task::block_in_place(|| {
                    wallet.on_invoice_events(params, Box::new(callback))
                });
                // sleep for 60 seconds before next loop TODO better time management
                std::thread::sleep(std::time::Duration::from_secs(60));
            }
        }
    }
    fn failure(&self, error: Option<String>) {
        warn!("epic fail {}", error.unwrap());
    }
}

// 4. Then kick off OnLnInvoiceEvents (Auditor Loop)
// 4a. Loop: Kill circuit if payment is not received within the rounds window
struct OnLnInvoiceEventCallback {}
impl lni::types::OnInvoiceEventCallback for OnLnInvoiceEventCallback {
    fn success(&self, transaction: Option<Transaction>) {
        match transaction.clone() {
            Some(txn) => {
                info!("Successfully received payment for payment hash {} with the preimage {}. Keeping the circuit open for another 60 seconds..."
                , txn.payment_hash, txn.preimage);

                // check if the payment is received within the window in the payments leder

                info!("Settled transaction: {:?}", transaction);
            }
            None => {
                info!("No transaction found");
            }
        }
    }
    fn pending(&self, transaction: Option<Transaction>) {
        match transaction.clone() {
            Some(txn) => {
                info!(
                    "Pending payment for payment hash {} with the preimage {}",
                    txn.payment_hash, txn.preimage
                );
                info!("Pending transaction: {:?}", transaction);
            }
            None => {
                info!("No transaction found");
            }
        }
    }
    fn failure(&self, transaction: Option<Transaction>) {
        match transaction.clone() {
            Some(txn) => {
                warn!(
                    "Failed payment for payment hash {} with the preimage {}",
                    txn.payment_hash, txn.preimage
                );
                warn!("Failed transaction: {:?}", transaction);
            }
            None => {
                info!("No transaction found");
            }
        }
    }
}
