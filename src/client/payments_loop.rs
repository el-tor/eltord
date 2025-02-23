use super::bandwidth_test;
use crate::database::{Db, Payment};
use crate::lightning;
use lni::phoenixd::PhoenixdNode;
use lni::PayInvoiceResponse;
use std::env;

// is round expired
// if no then do bandwidth test
// if good then pay relay
// wait for next round
pub async fn start_payments_loop(circuit_id: &String) -> Result<(), Box<dyn std::error::Error>> {
    let db = Db::new("payments.json".to_string()).unwrap();
    let wallet = lightning::load_wallet().await;
    let mut round = 1;
    println!("Starting payments loop for circuit: {:?}", circuit_id);
    while round <= 10 {
        let payments = match db.lookup_payments(circuit_id.to_string(), round) {
            Ok(payments) => payments,
            Err(_) => return Err("Payment for the circuit not found".into()),
        };
        for payment in payments.iter() {
            if (!is_round_expired((&payment)) && bandwidth_test::is_bandwidth_good()) {
                pay_relay(&wallet, &payment).await;
                // TODO write fee and preimage to ledger and handle errors/retry
            } else {
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
    print!("Waiting for next round {}...", chrono::Utc::now());
    std::thread::sleep(std::time::Duration::from_secs(interval_seconds as u64));
}

async fn pay_relay(
    wallet: &PhoenixdNode,
    payment: &Payment,
) -> Result<PayInvoiceResponse, Box<dyn std::error::Error>> {

    let amount_msats = payment.amount_msat;
    println!(
        "Paying {} sats relay: {:?}",
        amount_msats / 1000,
        Some(payment.bolt12_offer.clone())
    );
    // TODO Retry strategy
    let pay_resp = wallet
        .pay_offer(
            payment.bolt12_offer.clone().unwrap(),
            amount_msats,
            Some(payment.payment_id.clone()),
        )
        .await;
    match pay_resp {
        Ok(result) => {
            println!("Payment successful");
            Ok(result)
        }
        Err(e) => {
            println!("Payment failed: {:?}", e);
            Err("Payment failed".into())
        }
    }
}

fn kill_circuit() {}
