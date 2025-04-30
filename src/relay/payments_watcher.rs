use crate::{
    relay::{self, RelayPayments},
    rpc::rpc_event_listener,
    types::{EventCallback, RpcConfig},
};
use lni::{types::Transaction, LightningNode};

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
        dbg!(response.clone());

        let payment_hashes = if let Some(resp) = response.as_ref() {
            if resp.starts_with("650 EVENT_PAYMENT_ID_HASH_RECEIVED ") {
                Some(resp["650 EVENT_PAYMENT_ID_HASH_RECEIVED ".len()..].to_string())
            } else {
                None
            }
        } else {
            None
        };
        dbg!(payment_hashes.clone());

        if payment_hashes.is_some() {
            // 3a. On PAYMENT_ID_HASH_RECEIVED write a row to the ledger
            // 3b. Decode the payment_hashes via the 12 hash wire_format
            //        "handshake_payment_hash + handshake_preimage + payment_id_hash_round1 + payment_id_hash_round2 + ...payment_id_hash_round10"
            let relay_payments = RelayPayments::from_wire_format(&payment_hashes.clone().unwrap());
            // 3c. If you require a handshake fee check the handshake_payment_hash + handshake_preimage
            // 3d. Write the payment_id_hash_round1 to payment_id_hash_round10 to the ledger

            // TODO Loop relay_payments and kick off each watcher

            // 4. Then kick off OnInvoiceEvents (Auditor Loop)
            let params = lni::types::OnInvoiceEventParams {
                search: Some(relay_payments.payhashes[0].clone()),
                polling_delay_sec: 3,
                max_polling_sec: 60,
                ..Default::default()
            };
            let callback = OnLnInvoiceEventCallback {};
            tokio::task::block_in_place(|| wallet.on_invoice_events(params, Box::new(callback)));
            //.await;
        }
    }
    fn failure(&self, error: Option<String>) {
        println!("epic fail {}", error.unwrap());
    }
}

// 4. Then kick off OnLnInvoiceEvents (Auditor Loop)
// 4a. Loop: Kill circuit if payment is not received within the rounds window
struct OnLnInvoiceEventCallback {}
impl lni::types::OnInvoiceEventCallback for OnLnInvoiceEventCallback {
    fn success(&self, transaction: Option<Transaction>) {
        match transaction.clone() {
            Some(txn) => {
                println!("Successfully received payment for payment hash {} with the preimage {}. Keeping the circuit open for antoher 60 seconds"
                , txn.payment_hash, txn.preimage);

                // check if the payment is received within the window in the payments leder

                dbg!(transaction.clone());
            }
            None => {
                println!("No transaction found");
            }
        }
    }
    fn pending(&self, transaction: Option<Transaction>) {

        match transaction.clone() {
            Some(txn) => {
                println!("Pending payment for payment hash {} with the preimage {}", txn.payment_hash, txn.preimage);
                dbg!(transaction.clone());
            }
            None => {
                println!("No transaction found");
            }
        }        
    }
    fn failure(&self, transaction: Option<Transaction>) {
        match transaction.clone() {
            Some(txn) => {
                println!("Failed payment for payment hash {} with the preimage {}", txn.payment_hash, txn.preimage);
                dbg!(transaction.clone());
            }
            None => {
                println!("No transaction found");
            }
        }
    }
}
