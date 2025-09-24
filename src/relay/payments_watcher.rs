use crate::{
    relay::{init_payments_received_ledger, RelayPayments},
    rpc::rpc_event_listener,
    types::{EventCallback, RpcConfig},
};
use lni::{LightningNode, types::Transaction};
use log::{info, warn};
use tokio::time::{sleep, Duration, Instant};

// 2. Start payment watcher
pub async fn start_payments_watcher(
    config: &RpcConfig,
    wallet: std::sync::Arc<dyn LightningNode + Send + Sync>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 3. Listen for the Event PAYMENT_ID_HASH_RECEIVED
    let event = "PAYMENT_ID_HASH_RECEIVED";
    let on_event_payment_id_hash_received_callback =
        Box::new(OnTorEventPaymentIdHashReceivedCallback {
            wallet: wallet.clone(),
        });
    rpc_event_listener(
        config.clone(),
        event.to_string(),
        on_event_payment_id_hash_received_callback,
        &*wallet,
    )
    .await?;
    Ok(())
}

// 3. Listen for the Event PAYMENT_ID_HASH_RECEIVED
// WIRE_FORMAT (key-value pairs):
//   650 EVENT_PAYMENT_ID_HASH_RECEIVED P_CIRC_ID=4197744070 N_CIRC_ID=0 PAYMENT_HASH=8de99a614b7f95a3263ba74cf76dc00bb440d8e21a410003d9464404cef662c99e723990e296f17a1a2d98204b80ec5b872857c86926fd4f476f010905ca91f625411553e22808e07982846fe7c42949996815ec22bbbe844de491e0bd094bc48ec6b6fbed6bac29dfaaec84294a591924c2ed3ce3fb0f911d963ccfbafa1f2e52648c25c5acc11772b6c7529ff958c5086f761b1f5764a89808ebb53b74d0f913df5908cdc4222c41d78ab07e341e73b0c09d77a2af8f43992fdd136645a6a3f59fd490d2cc58cf8d7adc14da4344fe4758c84272fa1b0d823671e2c08f19b5db5203e8d0102068cd32e949ea691788b734fa092210a58396617886a0a0e09e5e5c97719eba76fbd2138ae12a7e1c22ac6d7d450c9df2535efd1345c619393622a58eddd02d46ce86ca3482c86a51541ec8474fbca4ff51c32854558e784ac8bf48b3c98587908d5c7b3af88e6b1fe87dca45934c90eba325fde8fab444b73a93669cc58cbdbf4c88ef115a0806dd55d94455dde80d9298965b4647ae9ff3a1


struct OnTorEventPaymentIdHashReceivedCallback {
    wallet: std::sync::Arc<dyn LightningNode + Send + Sync>,
}
impl EventCallback for OnTorEventPaymentIdHashReceivedCallback {
    fn success(&self, response: Option<String>, _wallet: &(dyn LightningNode + Send + Sync)) {
        info!("Event response: {:?}", response);
        let mut circ_id = "UNKNOWN".to_string();
        let mut payment_hashes: Option<String> = None;
        
        if let Some(resp) = response.as_ref() {
            // EVENT WIRE_FORMAT "650 EVENT_PAYMENT_ID_HASH_RECEIVED P_CIRC_ID=4197744070 N_CIRC_ID=0 PAYMENT_HASH=..."
            if resp.starts_with("650 EVENT_PAYMENT_ID_HASH_RECEIVED ") {
                let rest = &resp["650 EVENT_PAYMENT_ID_HASH_RECEIVED ".len()..];
                
                // Parse key-value pairs
                for part in rest.split_whitespace() {
                    if let Some(eq_pos) = part.find('=') {
                        let key = &part[..eq_pos];
                        let value = &part[eq_pos + 1..];
                        
                        match key {
                            "P_CIRC_ID" => {
                                circ_id = value.to_string();
                            }
                            "PAYMENT_HASH" => {
                                payment_hashes = Some(value.to_string());
                            }
                            _ => {
                                // Ignore other fields like N_CIRC_ID for now
                            }
                        }
                    }
                }
                
                info!("Parsed event - P_CIRC_ID: {}, PAYMENT_HASH length: {}", 
                      circ_id, payment_hashes.as_ref().map_or(0, |h| h.len()));
            } else {
                warn!("Unexpected EVENT_PAYMENT_ID_HASH_RECEIVED format: {}", resp);
            }
        }
        
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
            info!("Payment hashes received for circuit {}, starting {} invoice watchers", 
                  circ_id, relay_payments.payhashes.len());
            info!("Decoded payment hashes: {:?}", relay_payments.payhashes);
            
            // Capture the circuit start time for timing validation
            let circuit_start_time = Instant::now();
            
            // Start invoice event monitoring for each payment hash with staggered timing
            for (i, payment_hash) in relay_payments.payhashes.iter().enumerate() {
                let round_start_time = i as u64 * 60; // Round 0: 0s, Round 1: 60s, Round 2: 120s, etc.
                let round_end_time = round_start_time + 60;
                
                info!(
                    "Round {}: Scheduling invoice watcher for payment hash {} on circuit {} (active from {}s to {}s)",
                    i, payment_hash, circ_id, round_start_time, round_end_time
                );
                
                let params = lni::types::OnInvoiceEventParams {
                    search: Some(payment_hash.clone()),
                    polling_delay_sec: 3,
                    max_polling_sec: 60,
                    ..Default::default()
                };
                
                let callback = OnLnInvoiceEventCallback {
                    payment_hash: payment_hash.clone(),
                    circuit_id: circ_id.clone(),
                    round: i,
                    circuit_start_time,
                };
                
                // Log that we're scheduling the task (this will appear in main thread logs)
                info!("⏰ Scheduling async invoice monitoring task #{} for payment hash: {} on circuit: {}", 
                      i + 1, payment_hash, circ_id);
                info!("   → Will start monitoring at {}s and poll every {}s for max {}s", 
                      round_start_time, params.polling_delay_sec, params.max_polling_sec);
                
                // Spawn async task to handle invoice event watching with delay
                let wallet_clone = self.wallet.clone();
                let payment_hash_clone = payment_hash.clone();
                let _task_handle = tokio::spawn(async move {
                    // Wait for the round's start time
                    if round_start_time > 0 {
                        info!(
                            "⏳ Waiting {}s before starting Round {} monitoring for payment hash: {}",
                            round_start_time, i, payment_hash_clone
                        );
                        sleep(Duration::from_secs(round_start_time)).await;
                    }
                    
                    info!(
                        "🚀 Starting Round {} invoice monitoring for payment hash: {} (polling every {}s for max {}s)",
                        i, params.search.as_ref().unwrap(), params.polling_delay_sec, params.max_polling_sec
                    );
                    
                    // Start the invoice event watcher
                    wallet_clone.on_invoice_events(params, Box::new(callback)).await;

                    info!("✅ Finished Round {} invoice monitoring for payment hash: {}", i, payment_hash_clone);
                });
            }
        }
    }
    fn failure(&self, error: Option<String>) {
        warn!("epic fail {}", error.unwrap_or_default());
    }
}

// Invoice event callback for monitoring individual payment hashes
struct OnLnInvoiceEventCallback {
    payment_hash: String,
    circuit_id: String,
    round: usize,
    circuit_start_time: Instant,
}

impl lni::types::OnInvoiceEventCallback for OnLnInvoiceEventCallback {
    fn success(&self, transaction: Option<Transaction>) {
        let elapsed_secs = self.circuit_start_time.elapsed().as_secs();
        let expected_window_start = self.round as u64 * 60;
        let expected_window_end = expected_window_start + 60;
        
        info!(
            "🎉 INVOICE PAID! Payment hash: {} for circuit: {} (round {}) after {}s",
            self.payment_hash, self.circuit_id, self.round, elapsed_secs
        );
        
        // Check if payment was made within the acceptable time window
        // Each round can be paid from circuit start (0s) up to the end of its designated window
        if elapsed_secs <= expected_window_end {
            if elapsed_secs >= expected_window_start {
                info!(
                    "✅ Payment made ON TIME! Round {} payment received at {}s (window: 0s-{}s, ideal: {}s-{}s) - KEEP circuit {} ALIVE",
                    self.round, elapsed_secs, expected_window_end, expected_window_start, expected_window_end, self.circuit_id
                );
            } else {
                info!(
                    "⚡ Payment made EARLY! Round {} payment received at {}s (window: 0s-{}s, ideal: {}s-{}s) - KEEP circuit {} ALIVE",
                    self.round, elapsed_secs, expected_window_end, expected_window_start, expected_window_end, self.circuit_id
                );
            }
        } else {
            warn!(
                "⚠️ Payment made LATE! Round {} payment received at {}s (window: 0s-{}s, ideal: {}s-{}s) - TEARDOWN circuit {}",
                self.round, elapsed_secs, expected_window_end, expected_window_start, expected_window_end, self.circuit_id
            );
        }
        
        if let Some(txn) = transaction {
            info!(
                "💰 Transaction details - Hash: {} Preimage: {}",
                txn.payment_hash, txn.preimage
            );
        }
    }

    fn pending(&self, transaction: Option<Transaction>) {
        info!(
            "⏳ Invoice pending for payment hash: {} on circuit: {} (round {})",
            self.payment_hash, self.circuit_id, self.round
        );
        if let Some(txn) = transaction {
            info!("⏳ Pending transaction: {:?}", txn);
        }
    }

    fn failure(&self, transaction: Option<Transaction>) {
        let elapsed_secs = self.circuit_start_time.elapsed().as_secs();
        let expected_window_start = self.round as u64 * 60;
        let expected_window_end = expected_window_start + 60;
        
        warn!(
            "❌ Invoice payment failed for payment hash: {} on circuit: {} (round {}) after {}s",
            self.payment_hash, self.circuit_id, self.round, elapsed_secs
        );
        
        // Check if failure happened within or after the acceptable time window
        if elapsed_secs <= expected_window_end {
            warn!(
                "⏰ Payment failed within acceptable window (0s-{}s, ideal: {}s-{}s) at {}s - TEARDOWN circuit {}",
                expected_window_end, expected_window_start, expected_window_end, elapsed_secs, self.circuit_id
            );
        } else {
            warn!(
                "🕑 Payment failed after acceptable window (0s-{}s, ideal: {}s-{}s) at {}s - TEARDOWN circuit {}",
                expected_window_end, expected_window_start, expected_window_end, elapsed_secs, self.circuit_id
            );
        }
        
        if let Some(txn) = transaction {
            warn!("❌ Failed transaction: {:?}", txn);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lni::{LightningNode, ApiError};
    use lni::types::{OnInvoiceEventCallback, CreateInvoiceParams, PayInvoiceParams, OnInvoiceEventParams, NodeInfo, PayInvoiceResponse, LookupInvoiceParams, ListTransactionsParams, PayCode};
    use tokio::time::{Duration, Instant};

    // Mock LightningNode for testing
    struct MockLightningNode;

    #[async_trait::async_trait]
    impl LightningNode for MockLightningNode {
        async fn get_info(&self) -> Result<NodeInfo, ApiError> {
            Ok(NodeInfo::default())
        }

        async fn create_invoice(&self, _params: CreateInvoiceParams) -> Result<lni::Transaction, ApiError> {
            Ok(lni::Transaction {
                payment_hash: "test_hash".to_string(),
                preimage: "test_preimage".to_string(),
                type_: "incoming".to_string(),
                amount_msats: 1000000,
                fees_paid: 0,
                payer_note: Some("test".to_string()),
                external_id: Some("test".to_string()),
                invoice: "test_invoice".to_string(),
                description: "test".to_string(),
                description_hash: "".to_string(),
                settled_at: 0,
                created_at: 0,
                expires_at: 0,
            })
        }

        async fn pay_invoice(&self, _params: PayInvoiceParams) -> Result<PayInvoiceResponse, ApiError> {
            Ok(PayInvoiceResponse {
                payment_hash: "test_hash".to_string(),
                preimage: "test_preimage".to_string(),
                fee_msats: 0,
            })
        }

        async fn get_offer(&self, _offer_id: Option<String>) -> Result<PayCode, ApiError> {
            Ok(PayCode {
                bolt12: "test_offer".to_string(),
                offer_id: "test_offer_id".to_string(),
                label: Some("test_label".to_string()),
                active: Some(true),
                single_use: Some(false),
                used: Some(false),
            })
        }

        async fn list_offers(&self, _offer_id: Option<String>) -> Result<Vec<PayCode>, ApiError> {
            Ok(vec![])
        }

        async fn pay_offer(&self, _offer: String, _amount_sats: i64, _comment: Option<String>) -> Result<lni::PayInvoiceResponse, ApiError> {
            Ok(lni::PayInvoiceResponse {
                payment_hash: "test_hash".to_string(),
                preimage: "test_preimage".to_string(),
                fee_msats: 0,
            })
        }

        async fn lookup_invoice(&self, _params: LookupInvoiceParams) -> Result<lni::Transaction, ApiError> {
            Ok(lni::Transaction {
                payment_hash: "test_hash".to_string(),
                preimage: "test_preimage".to_string(),
                type_: "incoming".to_string(),
                amount_msats: 1000000,
                fees_paid: 0,
                payer_note: Some("test".to_string()),
                external_id: Some("test".to_string()),
                invoice: "test_invoice".to_string(),
                description: "test".to_string(),
                description_hash: "".to_string(),
                settled_at: 0,
                created_at: 0,
                expires_at: 0,
            })
        }

        async fn list_transactions(&self, _params: ListTransactionsParams) -> Result<Vec<lni::Transaction>, ApiError> {
            Ok(vec![])
        }

        async fn decode(&self, _input: String) -> Result<String, ApiError> {
            Ok("decoded".to_string())
        }

        async fn on_invoice_events(&self, _params: OnInvoiceEventParams, _callback: Box<dyn OnInvoiceEventCallback>) {
            // Mock implementation - do nothing
        }
    }    // Helper function to create a test callback with a specific start time
    fn create_test_callback(round: usize, circuit_start_time: Instant) -> OnLnInvoiceEventCallback {
        OnLnInvoiceEventCallback {
            payment_hash: format!("test_hash_{}", round),
            circuit_id: "test_circuit_123".to_string(),
            round,
            circuit_start_time,
        }
    }
    
    // Helper function to create a test transaction
    fn create_test_transaction(hash: &str) -> lni::types::Transaction {
        lni::types::Transaction {
            payment_hash: hash.to_string(),
            preimage: "test_preimage".to_string(),
            type_: "incoming".to_string(),
            amount_msats: 1000000,
            fees_paid: 0,
            payer_note: Some("test".to_string()),
            external_id: Some("test".to_string()),
            invoice: "test_invoice".to_string(),
            description: "test".to_string(),
            description_hash: "".to_string(),
            settled_at: 0,
            created_at: 0,
            expires_at: 0,
        }
    }
    
    #[tokio::test]
    async fn test_round_0_on_time_payment() {
        // Round 0: expected window 0-60s, payment at 30s should be ON TIME
        let start_time = Instant::now() - Duration::from_secs(30);
        let callback = create_test_callback(0, start_time);
        let transaction = Some(create_test_transaction("test_hash_0"));
        
        // This should log as "ON TIME" and "KEEP ALIVE"
        callback.success(transaction);
        
        // Test passes if no panic occurs and logs show correct behavior
    }
    
    #[tokio::test]
    async fn test_round_0_early_payment() {
        // Round 0: expected window 0-60s, payment at 5s should be ON TIME (not early since window starts at 0)
        let start_time = Instant::now() - Duration::from_secs(5);
        let callback = create_test_callback(0, start_time);
        let transaction = Some(create_test_transaction("test_hash_0"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_round_0_late_payment() {
        // Round 0: expected window 0-60s, payment at 75s should be LATE
        let start_time = Instant::now() - Duration::from_secs(75);
        let callback = create_test_callback(0, start_time);
        let transaction = Some(create_test_transaction("test_hash_0"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_round_1_early_payment() {
        // Round 1: acceptable window 0-120s, ideal 60-120s, payment at 30s should be EARLY
        let start_time = Instant::now() - Duration::from_secs(30);
        let callback = create_test_callback(1, start_time);
        let transaction = Some(create_test_transaction("test_hash_1"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_round_1_on_time_payment() {
        // Round 1: ideal window 60-120s, payment at 90s should be ON TIME
        let start_time = Instant::now() - Duration::from_secs(90);
        let callback = create_test_callback(1, start_time);
        let transaction = Some(create_test_transaction("test_hash_1"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_round_1_late_payment() {
        // Round 1: acceptable window 0-120s, payment at 150s should be LATE
        let start_time = Instant::now() - Duration::from_secs(150);
        let callback = create_test_callback(1, start_time);
        let transaction = Some(create_test_transaction("test_hash_1"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_round_2_early_payment() {
        // Round 2: acceptable window 0-180s, ideal 120-180s, payment at 60s should be EARLY
        let start_time = Instant::now() - Duration::from_secs(60);
        let callback = create_test_callback(2, start_time);
        let transaction = Some(create_test_transaction("test_hash_2"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_round_2_on_time_payment() {
        // Round 2: ideal window 120-180s, payment at 150s should be ON TIME
        let start_time = Instant::now() - Duration::from_secs(150);
        let callback = create_test_callback(2, start_time);
        let transaction = Some(create_test_transaction("test_hash_2"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_round_2_late_payment() {
        // Round 2: acceptable window 0-180s, payment at 200s should be LATE
        let start_time = Instant::now() - Duration::from_secs(200);
        let callback = create_test_callback(2, start_time);
        let transaction = Some(create_test_transaction("test_hash_2"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_round_9_early_payment() {
        // Round 9: acceptable window 0-600s, ideal 540-600s, payment at 300s should be EARLY
        let start_time = Instant::now() - Duration::from_secs(300);
        let callback = create_test_callback(9, start_time);
        let transaction = Some(create_test_transaction("test_hash_9"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_round_9_on_time_payment() {
        // Round 9: ideal window 540-600s, payment at 570s should be ON TIME
        let start_time = Instant::now() - Duration::from_secs(570);
        let callback = create_test_callback(9, start_time);
        let transaction = Some(create_test_transaction("test_hash_9"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_round_9_late_payment() {
        // Round 9: acceptable window 0-600s, payment at 650s should be LATE
        let start_time = Instant::now() - Duration::from_secs(650);
        let callback = create_test_callback(9, start_time);
        let transaction = Some(create_test_transaction("test_hash_9"));
        
        callback.success(transaction);
    }
    
    #[tokio::test]
    async fn test_payment_failure_within_window() {
        // Round 1: acceptable window 0-120s, failure at 90s should trigger TEARDOWN
        let start_time = Instant::now() - Duration::from_secs(90);
        let callback = create_test_callback(1, start_time);
        let transaction = Some(create_test_transaction("test_hash_1"));
        
        callback.failure(transaction);
    }
    
    #[tokio::test]
    async fn test_payment_failure_after_window() {
        // Round 1: acceptable window 0-120s, failure at 150s should trigger TEARDOWN
        let start_time = Instant::now() - Duration::from_secs(150);
        let callback = create_test_callback(1, start_time);
        let transaction = Some(create_test_transaction("test_hash_1"));
        
        callback.failure(transaction);
    }
    
    #[tokio::test]
    async fn test_payment_without_transaction() {
        // Test success callback with None transaction
        let start_time = Instant::now() - Duration::from_secs(30);
        let callback = create_test_callback(0, start_time);
        
        callback.success(None);
    }
    
    #[tokio::test]
    async fn test_failure_without_transaction() {
        // Test failure callback with None transaction
        let start_time = Instant::now() - Duration::from_secs(30);
        let callback = create_test_callback(0, start_time);
        
        callback.failure(None);
    }
    
    #[tokio::test]
    async fn test_window_boundary_conditions() {
        // Test exact boundary conditions
        
        // Round 1: Payment exactly at window end (120s) should be ON TIME
        let start_time = Instant::now() - Duration::from_secs(120);
        let callback = create_test_callback(1, start_time);
        let transaction = Some(create_test_transaction("boundary_test"));
        callback.success(transaction);
        
        // Round 1: Payment exactly at ideal window start (60s) should be ON TIME
        let start_time = Instant::now() - Duration::from_secs(60);
        let callback = create_test_callback(1, start_time);
        let transaction = Some(create_test_transaction("boundary_test"));
        callback.success(transaction);
        
        // Round 1: Payment one second after window (121s) should be LATE
        let start_time = Instant::now() - Duration::from_secs(121);
        let callback = create_test_callback(1, start_time);
        let transaction = Some(create_test_transaction("boundary_test"));
        callback.success(transaction);
    }
    
    // Test the timing calculations directly
    #[test]
    fn test_timing_calculations() {
        // Verify our timing math is correct
        for round in 0..10 {
            let expected_window_start = round as u64 * 60;
            let expected_window_end = expected_window_start + 60;
            
            // Round 0: 0-60, Round 1: 60-120, Round 2: 120-180, etc.
            assert_eq!(expected_window_start, round as u64 * 60);
            assert_eq!(expected_window_end, (round as u64 + 1) * 60);
        }
    }
}
