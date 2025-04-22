use crate::{
    rpc::rpc_event_listener,
    types::{EventCallback, RpcConfig},
};
use lni::{types::Transaction, LightningNode};


// 3. Start payment watcher
// 4. Listen for the Event PAYMENT_ID_HASH_RECEIVED
//      4a. On PAYMENT_ID_HASH_RECEIVED write a row to the ledger
//      4b. Then kick off OnInvoiceEvents (Auditor Loop)
//          - 4b1. Loop: Kill circuit if payment is not received within window

// 3. Start payment watcher
pub async fn start_payments_watcher(
    config: &RpcConfig,
    wallet: &(dyn LightningNode + Send + Sync),
) -> Result<(), Box<dyn std::error::Error>> {
    // 4. Listen for the Event PAYMENT_ID_HASH_RECEIVED
    let event = "PAYMENT_ID_HASH_RECEIVED";
    let on_event_payment_id_hash_received_callback = Box::new(OnEventPaymentIdHashReceivedCallback {});
    let result =
        rpc_event_listener(config.clone(), event.to_string(), on_event_payment_id_hash_received_callback, wallet).await;
    Ok(())
}

// 4a. On PAYMENT_ID_HASH_RECEIVED write a row to the ledger
// 4b. Then kick off OnInvoiceEvents (Auditor Loop)
struct OnEventPaymentIdHashReceivedCallback {}
impl EventCallback for OnEventPaymentIdHashReceivedCallback {
    fn success(&self, response: Option<String>, wallet: &(dyn LightningNode + Send + Sync)) {
        dbg!(response.clone());
        let payment_hash = if let Some(resp) = response.as_ref() {
            if resp.starts_with("650 EVENT_PAYMENT_ID_HASH_RECEIVED ") {
                Some(resp["650 EVENT_PAYMENT_ID_HASH_RECEIVED ".len()..].to_string())
            } else {
                None
            }
        } else {
            None
        };
        dbg!(payment_hash.clone());

        if payment_hash.is_some() {
            let params = lni::types::OnInvoiceEventParams {
                search: Some(payment_hash.unwrap()),
                polling_delay_sec: 3,
                max_polling_sec: 60,
                ..Default::default()
            };

            // 4a. On PAYMENT_ID_HASH_RECEIVED write a row to the ledger
            // TODO 

            // 4b. Then kick off OnInvoiceEvents (Auditor Loop)
            let callback = OnInvoiceEventCallback {};
            tokio::task::block_in_place(|| wallet.on_invoice_events(params, Box::new(callback)));
            //.await;
        }
    }
    fn failure(&self, error: Option<String>) {
        println!("epic fail {}", error.unwrap());
    }
}


// 4b. Then kick off OnInvoiceEvents (Auditor Loop)
//      - 4b1. Loop: Kill circuit if payment is not received within window
struct OnInvoiceEventCallback {}
impl lni::types::OnInvoiceEventCallback for OnInvoiceEventCallback {
    fn success(&self, transaction: Option<Transaction>) {
        println!("success");

        // check if the payment is received within the window in the payments leder

        dbg!(transaction.clone());
    }
    fn pending(&self, transaction: Option<Transaction>) {
        println!("pending");
        dbg!(transaction.clone());
    }
    fn failure(&self, transaction: Option<Transaction>) {
        println!("epic fail");
        dbg!(transaction.clone());
    }
}
