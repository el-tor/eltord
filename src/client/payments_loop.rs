use super::bandwidth_test;
use crate::database::{Db, Payment};
use crate::types::Relay;
use lni::{LightningNode, PayInvoiceResponse};
use log::{info, warn};
use std::env;

// is round expired
// if no then do bandwidth test
// if good then pay relay
// wait for next round
pub async fn start_payments_loop(
    rpc_config: &crate::types::RpcConfig,
    relays: &Vec<Relay>,
    circuit_id: &String,
    wallet: Box<dyn LightningNode + Send + Sync>,
    socks_port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Db::new("data/payments_sent.json".to_string()).unwrap();
    let mut round = 1;
    let rate_limit_delay: u64 = env::var("RATE_LIMIT_SECONDS")
        .unwrap_or("0".to_string())
        .parse()
        .unwrap();
    
    // Run initial bandwidth test before starting payment rounds
    info!("🔍 Running initial bandwidth test before payments...");
    match bandwidth_test::bandwidth_test(socks_port).await {
        Ok((latency_ms, speed_kbps)) => {
            info!("📊 Initial bandwidth test: {:.1} KB/s ({}ms for 100KB)", speed_kbps, latency_ms);
            // TODO set a configurable minimum speed threshold and abort if below
        }
        Err(e) => {
            warn!("❌ Initial bandwidth test failed: {}. Circuit may not be ready.", e);
        }
    }
    
    while round <= 10 {
        info!(
            "🥊 Round {:?} - Starting payments loop for circuit: {:?} 🥊",
            round, circuit_id
        );
        
        // Check stream capacity and warn if approaching limit
        let (total_streams, needs_more_circuits) = bandwidth_test::check_stream_capacity(rpc_config).await;
        if needs_more_circuits {
            warn!("⚠️  WARNING: {} total streams detected - approaching 256/circuit limit!", total_streams);
            warn!("🔄 Consider building additional circuits to distribute load");
            warn!("💡 TIP: Call EXTENDPAIDCIRCUIT to build more circuits");
        }
        
        // Check bandwidth before paying for this round (using real SOCKS proxy test)
        if !bandwidth_test::has_bandwidth(socks_port).await {
            warn!("❌ SOCKS bandwidth check failed before payment round {}. Stopping payments and rebuilding circuit.", round);
            return Err("Bandwidth lost before payment".into());
        }
        info!("🛜  SOCKS bandwidth check passed before payment round {} ({} total streams)", round, total_streams);
        
        for relay in relays.iter() {
            let payment_id_hash = match &relay.payment_id_hashes_10 {
                Some(hashes) => hashes[round - 1].clone(),
                None => return Err("Payment ID hashes not found".into()),
            };
            let mut payment = match db.lookup_payment_by_id(payment_id_hash) {
                Ok(payment) => payment.unwrap(),
                Err(_) => return Err("Payment for the circuit not found".into()),
            };
            // dbg!(payment.clone());
            // if zero amount, skip
            if payment.amount_msat == 0 || (payment.bolt12_offer.is_none() && payment.bolt11_invoice.is_none()) {
                info!(
                    "Payment amount is zero, skipping payment id: {:?}",
                    payment.payment_id
                );
            } else if !is_round_expired(&payment) {
                let pay_resp = pay_relay(&wallet, &payment).await;
                match pay_resp {
                    Ok(pay_resp) => {
                        payment.payment_hash = Some(pay_resp.payment_hash);
                        payment.preimage = Some(pay_resp.preimage);
                        payment.fee = Some(pay_resp.fee_msats);
                        payment.paid = true;
                        db.update_payment(payment).unwrap();
                    }
                    Err(_) => {
                        warn!("Payment failed for payment id: {:?}", payment.payment_id);
                        payment.has_error = true;
                        db.update_payment(payment).unwrap();
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(rate_limit_delay));
            } else {
                warn!("Kill circuit round is expired");
                kill_circuit();
                break;
            }
        }
        round += 1;
        
        // Wait for next round with bandwidth monitoring every 2 seconds
        if round <= 10 {
            if !wait_for_next_round_with_monitoring(rpc_config, socks_port, 45).await {
                warn!("❌ Bandwidth lost during round wait. Stopping payments and rebuilding circuit.");
                return Err("Bandwidth lost".into());
            }
        }
    }
    Ok(())
}

// check if the round is expired, allow a few seconds of padding to allow for slower lightning payments and route finding
fn is_round_expired(payment: &Payment) -> bool {
    let expiry_padding: i64 = env::var("EXPIRY_PADDING_FOR_PAYMENT_ROUND")
        .unwrap_or("15".to_string())
        .parse()
        .unwrap();
    if (payment.expires_at - chrono::Utc::now().timestamp()) < expiry_padding {
        return true;
    }
    false
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
    wallet: &Box<dyn LightningNode + Send + Sync>,
    payment: &Payment,
) -> Result<PayInvoiceResponse, Box<dyn std::error::Error>> {
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

fn kill_circuit() {}
