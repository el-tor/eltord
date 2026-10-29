use super::bandwidth_test;
use crate::database::{Db, Payment};
use crate::types::Relay;
use lni::{LightningNode, PayInvoiceResponse};
use log::{error, info, warn};
use std::env;

/// Both paid circuits have independent payment ledgers. Each must receive every
/// round; alternating circuit AND round skipped half of each relay's invoices.
pub async fn start_payments_loop_round_robin(
    rpc_config: &crate::types::RpcConfig,
    primary_relays: &Vec<Relay>,
    primary_circuit_id: &String,
    backup_relays: &Vec<Relay>,
    backup_circuit_id: &String,
    wallet: std::sync::Arc<Box<dyn LightningNode + Send + Sync>>,
    socks_port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_circuit_rounds(
        rpc_config,
        &[
            (primary_relays, primary_circuit_id),
            (backup_relays, backup_circuit_id),
        ],
        wallet,
        socks_port,
    )
    .await
}

pub async fn start_payments_loop(
    rpc_config: &crate::types::RpcConfig,
    relays: &Vec<Relay>,
    circuit_id: &String,
    wallet: std::sync::Arc<Box<dyn LightningNode + Send + Sync>>,
    socks_port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_circuit_rounds(rpc_config, &[(relays, circuit_id)], wallet, socks_port).await
}

async fn run_circuit_rounds(
    rpc_config: &crate::types::RpcConfig,
    circuits: &[(&Vec<Relay>, &String)],
    wallet: std::sync::Arc<Box<dyn LightningNode + Send + Sync>>,
    socks_port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let db = load_or_create_db()?;
    let rounds = circuits
        .iter()
        .flat_map(|(relays, _)| relays.iter())
        .map(|r| r.payment_id_hashes_10.as_ref().map_or(0, Vec::len))
        .min()
        .unwrap_or(0);
    if rounds == 0 {
        return Err("No payment rounds available".into());
    }
    // Start one monitor and keep its handle for the entire circuit generation.
    let mut monitor = crate::rpc::start_stream_attachment_monitor(
        rpc_config.clone(),
        circuits[0].1.clone(),
        circuits.last().unwrap().1.clone(),
    )
    .await?;
    let started = tokio::time::Instant::now();
    let period = circuits
        .iter()
        .flat_map(|(relays, _)| relays.iter())
        .map(|r| r.payment_interval_seconds.unwrap_or(60))
        .min()
        .unwrap_or(60)
        .min(60)
        .saturating_sub(15)
        .max(1);
    let rounds_future = async {
        for round in 1..=rounds {
            check_and_warn_stream_capacity(rpc_config).await;
            let mut live = 0;
            let response = crate::rpc::rpc_client(crate::types::RpcConfig {
                command: "GETINFO circuit-status".into(),
                ..rpc_config.clone()
            })
            .await
            .map_err(|e| e.to_string())?;
            let status = crate::rpc::command_response(&response).map_err(|e| e.to_string())?;
            for (relays, id) in circuits {
                if !status
                    .lines()
                    .any(|line| line.starts_with(&format!("{} BUILT ", id)))
                {
                    warn!(
                        "Circuit {} is no longer BUILT; using the surviving circuit",
                        id
                    );
                    continue;
                }
                live += 1;
                process_payments_for_relays(
                    &db,
                    relays,
                    round,
                    &**wallet,
                    get_rate_limit_delay(),
                    id,
                )
                .await?;
            }
            if live == 0 {
                return Err("All paid circuits are unavailable".into());
            }
            if round == 1 {
                info!("🔄 Bootstrapping 100%");
            }
            if round < rounds {
                // RPC, wallet calls and health checks count toward the interval.
                let due = started + std::time::Duration::from_secs(period as u64 * round as u64);
                let remaining = due.saturating_duration_since(tokio::time::Instant::now());
                if !wait_for_next_round_with_monitoring(
                    rpc_config,
                    socks_port,
                    remaining.as_secs() as i64,
                )
                .await
                {
                    return Err("SOCKS transport unavailable".into());
                }
            }
        }
        Ok(())
    };
    let result = tokio::select! {
        result = rounds_future => result,
        _ = &mut monitor => Err("Stream attachment monitor stopped".into()),
    };
    // Dropping JoinHandle detaches a task. Explicitly stop the old generation
    // before the next one changes circuit IDs or manual-attachment state.
    if !monitor.is_finished() {
        monitor.abort();
        let _ = monitor.await;
    }
    if let Err(e) = crate::rpc::restore_automatic_stream_attachment(rpc_config).await {
        warn!("Could not restore automatic attachment: {}", e);
    }
    result
}

/// Load the payments database or create a fresh one if corrupted
fn load_or_create_db() -> Result<Db, Box<dyn std::error::Error + Send + Sync>> {
    match Db::new("data/payments_sent.json".to_string()) {
        Ok(db) => Ok(db),
        Err(e) => {
            error!(
                "Failed to load payments database: {}. Creating backup and starting fresh...",
                e
            );
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let backup_path = format!("data/payments_sent.json.backup_{}", timestamp);
            if let Err(backup_err) = std::fs::copy("data/payments_sent.json", &backup_path) {
                warn!("Could not create backup: {}", backup_err);
            } else {
                info!("Corrupted database backed up to: {}", backup_path);
            }
            std::fs::write("data/payments_sent.json", "[]")?;
            Ok(Db::new("data/payments_sent.json".to_string())?)
        }
    }
}

/// Get the rate limit delay from environment variable
fn get_rate_limit_delay() -> u64 {
    env::var("RATE_LIMIT_SECONDS")
        .unwrap_or("1".to_string())
        .parse()
        .unwrap()
}

/// Check stream capacity and warn if approaching limit
async fn check_and_warn_stream_capacity(rpc_config: &crate::types::RpcConfig) {
    let (total_streams, needs_more_circuits) =
        bandwidth_test::check_stream_capacity(rpc_config).await;
    if needs_more_circuits {
        warn!(
            "⚠️  WARNING: {} total streams detected - approaching 256/circuit limit!",
            total_streams
        );
        warn!("🔄 Consider building additional circuits to distribute load");
        warn!("💡 TIP: Call EXTENDPAIDCIRCUIT to build more circuits");
    }
}

/// Process payments for a set of relays in a given round
async fn process_payments_for_relays(
    db: &Db,
    relays: &Vec<Relay>,
    round: usize,
    wallet: &(dyn LightningNode + Send + Sync),
    rate_limit_delay: u64,
    circuit_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for relay in relays.iter() {
        let payment_id_hash = match &relay.payment_id_hashes_10 {
            Some(hashes) => hashes
                .get(round - 1)
                .ok_or("Payment round exceeds available hashes")?
                .clone(),
            None => return Err("Payment ID hashes not found".into()),
        };

        let mut payment = match db.lookup_payment_by_id(payment_id_hash) {
            Ok(Some(payment)) => payment,
            Ok(None) => return Err("Payment not found in database".into()),
            Err(_) => return Err("Payment for the circuit not found".into()),
        };

        // Skip if zero amount or no invoice
        if payment.amount_msat == 0
            || (payment.bolt12_offer.is_none() && payment.bolt11_invoice.is_none())
        {
            info!(
                "Payment amount is zero, skipping payment id: {:?}",
                payment.payment_id
            );
            continue;
        }

        // Check if round is expired
        if is_round_expired(&payment) {
            warn!("Round expired for {} circuit", circuit_name);
            return Err(format!("Round expired on {} circuit", circuit_name).into());
        }

        // Attempt payment
        match pay_relay(wallet, &payment).await {
            Ok(pay_resp) => {
                payment.payment_hash = Some(pay_resp.payment_hash);
                payment.preimage = Some(pay_resp.preimage);
                payment.fee = Some(pay_resp.fee_msats);
                payment.paid = true;
                db.update_payment(payment)?;
            }
            Err(_) => {
                warn!(
                    "Payment failed for payment id: {:?} on {} circuit",
                    payment.payment_id, circuit_name
                );
                payment.has_error = true;
                db.update_payment(payment)?;
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(rate_limit_delay)).await;
    }

    Ok(())
}

// check if the round is expired, allow a few seconds of padding to allow for slower lightning payments and route finding
fn is_round_expired(payment: &Payment) -> bool {
    let expiry_padding: i64 = env::var("EXPIRY_PADDING_FOR_PAYMENT_ROUND")
        .unwrap_or("15".to_string())
        .parse()
        .unwrap();
    chrono::Utc::now().timestamp() > payment.expires_at.saturating_add(expiry_padding)
}

/// Waits for the next payment round while monitoring bandwidth every 2 seconds.
/// Uses small heartbeat checks via SOCKS, bounded by the next payment deadline.
/// Returns true if bandwidth remains good throughout the wait.
/// Returns false if bandwidth is lost, signaling to stop payments and rebuild circuit.
async fn wait_for_next_round_with_monitoring(
    rpc_config: &crate::types::RpcConfig,
    socks_port: u16,
    interval_seconds: i64,
) -> bool {
    let deadline = tokio::time::Instant::now()
        + std::time::Duration::from_secs(interval_seconds.max(0) as u64);
    let mut failures = 0;
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return true,
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
        }
        let healthy = tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return true,
            healthy = bandwidth_test::has_bandwidth(socks_port) => healthy,
        };
        if healthy {
            failures = 0;
        } else {
            failures += 1;
        }
        if failures >= 2 {
            return false;
        }
        // Large throughput probes are diagnostic tools, not part of the
        // payment deadline: a 45-second download used to delay relay renewals.
        let _ = rpc_config;
    }
}

async fn pay_relay(
    wallet: &(dyn LightningNode + Send + Sync),
    payment: &Payment,
) -> Result<PayInvoiceResponse, Box<dyn std::error::Error + Send + Sync>> {
    let amount_msats = payment.amount_msat;
    info!(
        "Paying {} sats relay: {:?} with payment id: {:?}",
        amount_msats / 1000,
        Some(
            payment
                .bolt12_offer
                .clone()
                .map(|offer| offer.chars().take(10).collect::<String>())
        ),
        payment.payment_id
    );

    let pay_resp = wallet
        .pay_offer(
            payment.bolt12_offer.clone().unwrap(),
            amount_msats,
            Some(payment.payment_id.clone()),
        )
        .await;
    match pay_resp {
        Ok(result) => {
            info!(
                "Payment successful for payment id {:?} with preimage {:?} and fee {:?}",
                payment.payment_id, result.preimage, result.fee_msats
            );
            Ok(result)
        }
        Err(e) => {
            warn!(
                "Payment failed for payment id: {:?} with error {:?}",
                payment.payment_id, e
            );
            Err("Payment failed".into())
        }
    }

    // TODO Retry strategy
}

#[cfg(test)]
mod stability_tests {
    use super::*;
    use lni::*;
    use std::sync::{Arc, Mutex};
    struct Wallet(Arc<Mutex<Vec<String>>>);
    #[async_trait::async_trait]
    impl LightningNode for Wallet {
        async fn get_permissions(&self) -> Result<Permissions, ApiError> {
            unimplemented!()
        }
        async fn get_info(&self) -> Result<NodeInfo, ApiError> {
            unimplemented!()
        }
        async fn create_invoice(&self, _: CreateInvoiceParams) -> Result<Transaction, ApiError> {
            unimplemented!()
        }
        async fn pay_invoice(&self, _: PayInvoiceParams) -> Result<PayInvoiceResponse, ApiError> {
            unimplemented!()
        }
        async fn create_offer(&self, _: CreateOfferParams) -> Result<Offer, ApiError> {
            unimplemented!()
        }
        async fn get_offer(&self, _: Option<String>) -> Result<Offer, ApiError> {
            unimplemented!()
        }
        async fn list_offers(&self, _: Option<String>) -> Result<Vec<Offer>, ApiError> {
            unimplemented!()
        }
        async fn lookup_invoice(&self, _: LookupInvoiceParams) -> Result<Transaction, ApiError> {
            unimplemented!()
        }
        async fn list_transactions(
            &self,
            _: ListTransactionsParams,
        ) -> Result<Vec<Transaction>, ApiError> {
            unimplemented!()
        }
        async fn decode(&self, _: String) -> Result<String, ApiError> {
            unimplemented!()
        }
        async fn decode_offer(&self, _: String) -> Result<String, ApiError> {
            unimplemented!()
        }
        async fn on_invoice_events(
            &self,
            _: OnInvoiceEventParams,
            _: Arc<dyn OnInvoiceEventCallback>,
        ) {
            unimplemented!()
        }
        async fn pay_offer(
            &self,
            _: String,
            _: i64,
            note: Option<String>,
        ) -> Result<PayInvoiceResponse, ApiError> {
            self.0.lock().unwrap().push(note.unwrap());
            Ok(PayInvoiceResponse {
                payment_hash: "mock".into(),
                preimage: "mock".into(),
                fee_msats: 0,
            })
        }
    }

    fn relay(label: &str) -> Relay {
        Relay {
            nickname: label.into(),
            fingerprint: label.into(),
            contact: None,
            bandwidth: None,
            ip: None,
            port: None,
            payment_bolt12_offer: Some("mock-offer".into()),
            payment_bip353: None,
            payment_bolt11_lnurl: None,
            payment_bolt11_lightning_address: None,
            payment_rate_msats: Some(1000),
            payment_interval_seconds: Some(16),
            payment_interval_rounds: Some(2),
            payment_handshake_fee: None,
            payment_handshake_fee_payhash: None,
            payment_handshake_fee_preimage: None,
            payment_id_hashes_10: Some(vec![format!("{}1", label), format!("{}2", label)]),
            relay_tag: None,
            hop: None,
        }
    }

    #[tokio::test]
    async fn both_circuits_receive_every_round_and_expiry_allows_grace() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stream);
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                            break;
                        }
                        let response = if line.starts_with("GETINFO circuit-status") {
                            "250+circuit-status=\r\n41 BUILT test\r\n42 BUILT test\r\n.\r\n250 OK\r\n"
                        } else if line.starts_with("QUIT") {
                            "250 closing connection\r\n"
                        } else {
                            "250 OK\r\n"
                        };
                        if reader
                            .get_mut()
                            .write_all(response.as_bytes())
                            .await
                            .is_err()
                        {
                            break;
                        }
                        if line.starts_with("QUIT") {
                            break;
                        }
                    }
                });
            }
        });
        let previous = std::env::current_dir().unwrap();
        let dir = std::env::temp_dir().join(format!("eltor-round-test-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let primary = vec![relay("A")];
        let backup = vec![relay("B")];
        super::super::payments_sent_ledger::init_payments_sent_ledger(&primary, &"41".into());
        super::super::payments_sent_ledger::init_payments_sent_ledger(&backup, &"42".into());
        let mut payment = crate::database::Db::new("data/payments_sent.json".into())
            .unwrap()
            .lookup_payment_by_id("A1".into())
            .unwrap()
            .unwrap();
        payment.expires_at = chrono::Utc::now().timestamp() + 10;
        assert!(!is_round_expired(&payment));
        payment.expires_at = chrono::Utc::now().timestamp() - 10;
        assert!(!is_round_expired(&payment));
        payment.expires_at = chrono::Utc::now().timestamp() - 30;
        assert!(is_round_expired(&payment));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let result = start_payments_loop_round_robin(
            &crate::types::RpcConfig {
                addr,
                rpc_password: Some("test".into()),
                command: String::new(),
            },
            &primary,
            &"41".into(),
            &backup,
            &"42".into(),
            Arc::new(Box::new(Wallet(calls.clone()))),
            0,
        )
        .await;
        std::env::set_current_dir(previous).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        server.abort();
        result.unwrap();
        assert_eq!(*calls.lock().unwrap(), vec!["A1", "B1", "A2", "B2"]);
    }
}
