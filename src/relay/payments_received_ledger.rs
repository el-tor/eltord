use crate::types::Relay;
use crate::{database, relay};

use super::{relay_payments, RelayPayments};

pub fn init_payments_received_ledger(relay_payments: &RelayPayments, circuit_id: &String) {
    let mut i = 1;
    for payment_id_hash in relay_payments.payhashes.clone().iter() {
        let mut row = database::Payment {
            payment_id: payment_id_hash.to_string(),
            circ_id: circuit_id.to_string(),
            interval_seconds: 60, //relay.payment_interval_seconds.unwrap_or(60) as i64,
            round: i,
            relay_fingerprint: "ME".to_string(), //relay_payments.fingerprint.clone(),
            updated_at: chrono::Utc::now().timestamp(),
            amount_msat: 0, //relay.payment_rate_msats.unwrap_or(0) as i64,
            handshake_fee_payhash: Some(relay_payments.handshake_payment_hash.clone()),
            handshake_fee_preimage: Some(relay_payments.handshake_preimage.clone()),
            paid: false,
            expires_at: chrono::Utc::now().timestamp() + 60,
            bolt11_invoice: None,                         // TODO implement
            bolt12_offer: Some("MY_BOLT_12_OFFER".to_string()), // TODO lookup
            payment_hash: None,
            preimage: None,
            fee: None,
            has_error: false,
        };

        let db = database::Db::new("data/payments_received.json".to_string()).unwrap();
        db.write_payment(row).unwrap();
        i += 1;
    }

    println!(
        "Init row in payments received ledger for circuit: {:?}",
        circuit_id
    );
}
