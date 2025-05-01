use super::bandwidth_test;
use crate::database::{Db, Payment};
use crate::types::Relay;
use lni::{LightningNode, PayInvoiceResponse};
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
) -> Result<(), Box<dyn std::error::Error>> {
    let db = Db::new("payments_sent.json".to_string()).unwrap();
    let mut round = 1;
    let rate_limit_delay: u64 = env::var("RATE_LIMIT_SECONDS")
        .unwrap_or("0".to_string())
        .parse()
        .unwrap();
    while round <= 10 {
        println!(
            "Round {:?} - Starting payments loop for circuit: {:?}",
            round, circuit_id
        );
        for relay in relays.iter() {
            let payment_id_hash = match &relay.payment_id_hashes_10 {
                Some(hashes) => hashes[round - 1].clone(),
                None => return Err("Payment ID hashes not found".into()),
            };
            let mut payment = match db.lookup_payment_by_id(payment_id_hash) {
                Ok(payment) => payment.unwrap(),
                Err(_) => return Err("Payment for the circuit not found".into()),
            };
            if !is_round_expired(&payment) && bandwidth_test::is_bandwidth_good() {
                let pay_resp = tokio::task::block_in_place(|| pay_relay(&wallet, &payment));
                match pay_resp {
                    Ok(pay_resp) => {
                        payment.payment_hash = Some(pay_resp.payment_hash);
                        payment.preimage = Some(pay_resp.preimage);
                        payment.fee = Some(pay_resp.fee_msats);
                        payment.paid = true;
                        db.update_payment(payment).unwrap();
                    }
                    Err(_) => {
                        println!("Payment failed for payment id: {:?}", payment.payment_id);
                        payment.has_error = true;
                        db.update_payment(payment).unwrap();
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(rate_limit_delay));
            } else {
                println!("Kill circuit round is expired");
                kill_circuit();
                break;
            }
        }
        round += 1;
        // TODO figure out how to do for relays with different interval_seconds, hardcode 45 second intervals for now
        wait_for_next_round(45);
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

fn wait_for_next_round(interval_seconds: i64) {
    println!("Waiting for next round {}...", chrono::Utc::now());
    std::thread::sleep(std::time::Duration::from_secs(interval_seconds as u64));
}

fn pay_relay(
    wallet: &Box<dyn LightningNode + Send + Sync>,
    payment: &Payment,
) -> Result<PayInvoiceResponse, Box<dyn std::error::Error>> {
    let amount_msats = payment.amount_msat;
    println!(
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
    );
    match pay_resp {
        Ok(result) => {
            println!(
                "Payment successful for payment id {:?} with preimage {:?} and fee {:?}",
                payment.payment_id, result.preimage, result.fee_msats
            );
            Ok(result)
        }
        Err(e) => {
            println!(
                "Payment failed for payment id: {:?} with error {:?}",
                payment.payment_id, e
            );
            Err("Payment failed".into())
        }
    }

    // TODO Retry strategy
}

fn kill_circuit() {}
