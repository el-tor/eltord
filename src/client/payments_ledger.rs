use crate::{database, rpc::Relay};

pub fn init_payments_ledger(selected_relays: &Vec<Relay>, circuit_id: String) {
    for relay in selected_relays.iter() {
        let mut i = 1;
        for payment_id_hash in relay.payment_id_hashes_10.clone().unwrap().iter() {

            let mut row = database::Payment {
                payment_id: payment_id_hash.to_string(),
                circ_id: circuit_id.to_string(),
                round: i,
                relay_fingerprint: relay.fingerprint.clone(),
                updated_at: chrono::Utc::now().timestamp(),
                amount_msat: relay.payment_rate_msats.unwrap_or(0) as i64,
                handshake_fee_payhash: None,
                handshake_fee_preimage: None,
                paid: false,
                expires_at: chrono::Utc::now().timestamp() + relay.payment_interval.unwrap_or(0) as i64,
            };
            if i == 1 {
                row.handshake_fee_payhash = relay.payment_handshake_fee_payhash.clone();
                row.handshake_fee_preimage = relay.payment_handshake_fee_preimage.clone();
            }
            let db = database::Db::new("payments.json".to_string()).unwrap();
            db.write_payment(row).unwrap();
            i += 1;
        }
    }
    println!("Init row in payments ledger for circuit: {:?}", circuit_id);
}
