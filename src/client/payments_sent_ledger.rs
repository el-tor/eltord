use crate::database;
use crate::types::Relay;

pub fn init_payments_sent_ledger(selected_relays: &Vec<Relay>, circuit_id: &String) {
    for relay in selected_relays.iter() {
        let mut i = 1;
        for payment_id_hash in relay.payment_id_hashes_10.clone().unwrap().iter() {

            let mut row = database::Payment {
                payment_id: payment_id_hash.to_string(),
                circ_id: circuit_id.to_string(),
                interval_seconds: relay.payment_interval_seconds.unwrap_or(60) as i64,
                round: i,
                relay_fingerprint: relay.fingerprint.clone(),
                updated_at: chrono::Utc::now().timestamp(),
                amount_msat: relay.payment_rate_msats.unwrap_or(0) as i64,
                handshake_fee_payhash: None,
                handshake_fee_preimage: None,
                paid: false,
                expires_at: chrono::Utc::now().timestamp() + (relay.payment_interval_seconds.unwrap_or(60) as i64  * i ), // expires now + 60 seconds for round 1, now + 120 seconds for round 2, etc
                bolt11_invoice: None, // TODO implement
                bolt12_offer: relay.payment_bolt12_offer.clone(), // TODO lookup payment preference from relay based on what capabilities your wallet has
                payment_hash: None,
                preimage: None,
                fee: None,
                has_error: false,
            };
            if i == 1 {
                row.handshake_fee_payhash = relay.payment_handshake_fee_payhash.clone();
                row.handshake_fee_preimage = relay.payment_handshake_fee_preimage.clone();
            }
            let db = database::Db::new("data/payments_sent.json".to_string()).unwrap();
            db.write_payment(row).unwrap();
            i += 1;
        }
    }
    println!("Init row in payments sent ledger for circuit: {:?}", circuit_id);
}
