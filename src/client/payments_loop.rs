use super::bandwidth_test;
use crate::database::{Db, Payment};
use crate::types::Relay;
use lni::{LightningNode, PayInvoiceResponse};
use log::{error, info, warn};
use std::env;

/// Runs payment loops on two circuits in round-robin fashion.
/// Alternates between primary and backup circuits for each payment round.
/// This provides load balancing and redundancy.
pub async fn start_payments_loop_round_robin(
    rpc_config: &crate::types::RpcConfig,
    primary_relays: &Vec<Relay>,
    primary_circuit_id: &String,
    backup_relays: &Vec<Relay>,
    backup_circuit_id: &String,
    wallet: std::sync::Arc<Box<dyn LightningNode + Send + Sync>>,
    socks_port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let db = load_or_create_db()?;
    let rate_limit_delay = get_rate_limit_delay();
    let max_rounds = 10;
    
    info!("🔄 Starting round-robin payment loop with {} rounds", max_rounds);
    info!("   Primary circuit: {}", primary_circuit_id);
    info!("   Backup circuit: {}", backup_circuit_id);
    
    let mut first_bandwidth_check = true; // Track if this is the first successful bandwidth check
    let mut stream_monitor_started = false; // Track if we've started the stream attachment monitor
    
    for round in 1..=max_rounds {
        // Determine which circuit to use for this round (alternate between them)
        let (current_relays, current_circuit_id, circuit_name) = if round % 2 == 1 {
            (primary_relays, primary_circuit_id, "PRIMARY")
        } else {
            (backup_relays, backup_circuit_id, "BACKUP")
        };
        
        info!(
            "🥊 Round {}/{} - Using {} circuit {} 🥊",
            round, max_rounds, circuit_name, current_circuit_id
        );
        
        // Check stream capacity and warn if approaching limit
        check_and_warn_stream_capacity(rpc_config).await;
        
        // Check bandwidth before paying for this round
        if !bandwidth_test::has_bandwidth(socks_port).await {
            warn!("❌ SOCKS bandwidth check failed before payment round {} on {} circuit.", round, circuit_name);
            warn!("🔄 FAILOVER: Switching to {} circuit for this round", if circuit_name == "PRIMARY" { "BACKUP" } else { "PRIMARY" });
            
            // Switch to the other circuit for this round
            let (failover_relays, failover_name) = if circuit_name == "PRIMARY" {
                (backup_relays, "BACKUP")
            } else {
                (primary_relays, "PRIMARY")
            };
            
            // Try the failover circuit
            if !bandwidth_test::has_bandwidth(socks_port).await {
                warn!("❌ FAILOVER FAILED: {} circuit also has no bandwidth. Both circuits down.", failover_name);
                return Err("Both circuits have lost bandwidth".into());
            }
            
            info!("✅ FAILOVER SUCCESS: {} circuit has bandwidth, continuing with it", failover_name);
            
            // Start stream monitor on first bandwidth check during failover path too
            if first_bandwidth_check {
                info!("🔄 Bootstrapping 100%");
                first_bandwidth_check = false;
                
                if !stream_monitor_started {
                    info!("🌊 Starting stream attachment monitor for round-robin stream distribution...");
                    match crate::rpc::start_stream_attachment_monitor(
                        rpc_config.clone(),
                        primary_circuit_id.clone(),
                        backup_circuit_id.clone(),
                    )
                    .await
                    {
                        Ok(_handle) => {
                            info!("✅ Stream attachment monitor started - streams will be distributed 50/50 across both circuits");
                            stream_monitor_started = true;
                        }
                        Err(e) => {
                            warn!("⚠️  Failed to start stream attachment monitor: {}", e);
                            warn!("⚠️  Falling back to Tor's automatic stream assignment");
                        }
                    }
                }
            }
            
            // Process payments on failover circuit for this round
            process_payments_for_relays(
                &db,
                failover_relays,
                round,
                &**wallet,
                rate_limit_delay,
                failover_name,
            ).await?;
            
            // Wait for next round with monitoring
            if round < max_rounds {
                if !wait_for_next_round_with_monitoring(rpc_config, socks_port, 45).await {
                    warn!("❌ Bandwidth lost during round wait after failover.");
                    return Err("Bandwidth lost during round wait after failover".into());
                }
            }
            continue;
        }
        
        let (total_streams, _) = bandwidth_test::check_stream_capacity(rpc_config).await;
        
        // Log "Bootstrapping 100%" on first successful bandwidth check (means SOCKS is fully ready)
        if first_bandwidth_check {
            info!("🔄 Bootstrapping 100%");
            first_bandwidth_check = false;
            
            // NOW it's safe to start the stream attachment monitor
            // This ensures Tor has working circuits BEFORE we set __LeaveStreamsUnattached=1
            if !stream_monitor_started {
                info!("🌊 Starting stream attachment monitor for round-robin stream distribution...");
                match crate::rpc::start_stream_attachment_monitor(
                    rpc_config.clone(),
                    primary_circuit_id.clone(),
                    backup_circuit_id.clone(),
                )
                .await
                {
                    Ok(_handle) => {
                        info!("✅ Stream attachment monitor started - streams will be distributed 50/50 across both circuits");
                        stream_monitor_started = true;
                    }
                    Err(e) => {
                        warn!("⚠️  Failed to start stream attachment monitor: {}", e);
                        warn!("⚠️  Falling back to Tor's automatic stream assignment");
                    }
                }
            }
        }
        
        info!("🛜  SOCKS bandwidth check passed before payment round {} on {} circuit ({} total streams)", round, circuit_name, total_streams);
        
        // Process payments for all relays in current circuit
        process_payments_for_relays(
            &db,
            current_relays,
            round,
            &**wallet,
            rate_limit_delay,
            circuit_name,
        ).await?;
        
        // Wait for next round with bandwidth monitoring
        if round < max_rounds {
            if !wait_for_next_round_with_monitoring(rpc_config, socks_port, 45).await {
                warn!("❌ Bandwidth lost during round wait.");
                return Err("Bandwidth lost during round wait".into());
            }
        }
    }
    
    info!("✅ Round-robin payment loops completed successfully for both circuits!");
    Ok(())
}

pub async fn start_payments_loop(
    rpc_config: &crate::types::RpcConfig,
    relays: &Vec<Relay>,
    circuit_id: &String,
    wallet: std::sync::Arc<Box<dyn LightningNode + Send + Sync>>,
    socks_port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let db = load_or_create_db()?;
    let rate_limit_delay = get_rate_limit_delay();
    let max_rounds = 10;
    
    let mut first_bandwidth_check = true; // Track if this is the first successful bandwidth check
    
    for round in 1..=max_rounds {
        info!(
            "🥊 Round {:?} - Starting payments loop for circuit: {:?} 🥊",
            round, circuit_id
        );
        
        // Check stream capacity and warn if approaching limit
        check_and_warn_stream_capacity(rpc_config).await;
        
        // Check bandwidth before paying for this round (using real SOCKS proxy test)
        if !bandwidth_test::has_bandwidth(socks_port).await {
            warn!("❌ SOCKS bandwidth check failed before payment round {}. Stopping payments and rebuilding circuit.", round);
            return Err("Bandwidth lost before payment".into());
        }
        
        let (total_streams, _) = bandwidth_test::check_stream_capacity(rpc_config).await;
        
        // Log "Bootstrapping 100%" on first successful bandwidth check (means SOCKS is fully ready)
        if first_bandwidth_check {
            info!("🔄 Bootstrapping 100%");
            first_bandwidth_check = false;
        }
        
        info!("🛜  SOCKS bandwidth check passed before payment round {} ({} total streams)", round, total_streams);
        
        // Process payments for all relays
        process_payments_for_relays(
            &db,
            relays,
            round,
            &**wallet,
            rate_limit_delay,
            "SINGLE",
        ).await?;
        
        // Wait for next round with bandwidth monitoring
        if round < max_rounds {
            if !wait_for_next_round_with_monitoring(rpc_config, socks_port, 45).await {
                warn!("❌ Bandwidth lost during round wait. Stopping payments and rebuilding circuit.");
                return Err("Bandwidth lost".into());
            }
        }
    }
    
    Ok(())
}

/// Load the payments database or create a fresh one if corrupted
fn load_or_create_db() -> Result<Db, Box<dyn std::error::Error + Send + Sync>> {
    match Db::new("data/payments_sent.json".to_string()) {
        Ok(db) => Ok(db),
        Err(e) => {
            error!("Failed to load payments database: {}. Creating backup and starting fresh...", e);
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
        .unwrap_or("0".to_string())
        .parse()
        .unwrap()
}

/// Check stream capacity and warn if approaching limit
async fn check_and_warn_stream_capacity(rpc_config: &crate::types::RpcConfig) {
    let (total_streams, needs_more_circuits) = bandwidth_test::check_stream_capacity(rpc_config).await;
    if needs_more_circuits {
        warn!("⚠️  WARNING: {} total streams detected - approaching 256/circuit limit!", total_streams);
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
            Some(hashes) => hashes[round - 1].clone(),
            None => return Err("Payment ID hashes not found".into()),
        };
        
        let mut payment = match db.lookup_payment_by_id(payment_id_hash) {
            Ok(Some(payment)) => payment,
            Ok(None) => return Err("Payment not found in database".into()),
            Err(_) => return Err("Payment for the circuit not found".into()),
        };
        
        // Skip if zero amount or no invoice
        if payment.amount_msat == 0 || (payment.bolt12_offer.is_none() && payment.bolt11_invoice.is_none()) {
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
                warn!("Payment failed for payment id: {:?} on {} circuit", payment.payment_id, circuit_name);
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
    (payment.expires_at - chrono::Utc::now().timestamp()) < expiry_padding
}

/// Waits for the next payment round while monitoring bandwidth every 2 seconds.
/// Uses heartbeat checks (every 2s) and full bandwidth tests (every 60s) via SOCKS proxy.
/// Returns true if bandwidth remains good throughout the wait.
/// Returns false if bandwidth is lost, signaling to stop payments and rebuild circuit.
async fn wait_for_next_round_with_monitoring(
    rpc_config: &crate::types::RpcConfig,
    socks_port: u16,
    interval_seconds: i64,
) -> bool {
    info!("Waiting for next round with SOCKS bandwidth monitoring ({}s interval)...", interval_seconds);
    
    let heartbeat_interval = 2; // Heartbeat check every 2 seconds
    let bandwidth_test_interval = 45; // Full bandwidth test every 45 seconds (matches wait interval)
    let log_interval = 10; // Log stats every 10 seconds
    let iterations = interval_seconds / heartbeat_interval;
    
    let mut last_bandwidth_test = -45i64; // Initialize to -45 so first test runs immediately
    
    for i in 0..iterations {
        tokio::time::sleep(tokio::time::Duration::from_secs(heartbeat_interval as u64)).await;
        
        let elapsed = (i + 1) * heartbeat_interval;
        
        // Check stream capacity (via RPC)
        let (total_streams, needs_more) = bandwidth_test::check_stream_capacity(rpc_config).await;
        
        // Heartbeat check every 2 seconds (lightweight SOCKS test)
        if !bandwidth_test::has_bandwidth(socks_port).await {
            warn!(
                "[T+{:02}s] ❌ HEARTBEAT FAILED | 🌊 Total streams: {}",
                elapsed, total_streams
            );
            warn!(
                "SOCKS heartbeat check failed at iteration {}/{} during round wait", 
                i + 1, 
                iterations
            );
            return false;
        }
        
        // Full bandwidth test every 45 seconds (throughput measurement)
        if elapsed - last_bandwidth_test >= bandwidth_test_interval {
            match bandwidth_test::bandwidth_test(socks_port).await {
                Ok((latency_ms, speed_kbps)) => {
                    info!(
                        "[T+{:02}s] 📊 BANDWIDTH TEST | Latency: {}ms | Speed: {:.1} KB/s | Streams: {}",
                        elapsed, latency_ms, speed_kbps, total_streams
                    );
                }
                Err(e) => {
                    warn!(
                        "[T+{:02}s] ❌ BANDWIDTH TEST FAILED | Error: {} | Streams: {}",
                        elapsed, e, total_streams
                    );
                    return false;
                }
            }
            last_bandwidth_test = elapsed;
        }
        
        // Log every 10 seconds
        if elapsed % log_interval == 0 {
            info!(
                "[T+{:02}s] ✅ HEARTBEAT OK | 🌊 Streams: {}{}",
                elapsed, total_streams,
                if needs_more { " ⚠️ APPROACHING LIMIT!" } else { "" }
            );
        }
    }
    
    // Handle remaining time if interval_seconds is not evenly divisible by heartbeat_interval
    let remaining = interval_seconds % heartbeat_interval;
    if remaining > 0 {
        tokio::time::sleep(tokio::time::Duration::from_secs(remaining as u64)).await;
        
        // Final heartbeat check
        if !bandwidth_test::has_bandwidth(socks_port).await {
            warn!("[T+{:02}s] ❌ FAILED | SOCKS heartbeat check failed during final check", interval_seconds);
            return false;
        }
    }
    
    info!("✅ Round wait completed with good SOCKS bandwidth");
    true
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

    let pay_resp = wallet.pay_offer(
        payment.bolt12_offer.clone().unwrap(),
        amount_msats,
        Some(payment.payment_id.clone()),
    ).await;
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
