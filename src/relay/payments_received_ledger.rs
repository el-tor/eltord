use crate::types::Relay;
use crate::{database, relay};
use log::{error, info, warn};

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
            bolt11_invoice: None,                               // TODO implement
            bolt12_offer: Some("MY_BOLT_12_OFFER".to_string()), // TODO lookup
            payment_hash: None,
            preimage: None,
            fee: None,
            has_error: false,
        };

        // Create data folder if it doesn't exist
        // TODO read from config file
        if let Err(e) = std::fs::create_dir_all("data") {
            error!("Failed to create data directory: {}", e);
            continue;
        }
        // Create payments_received.json file if it doesn't exist
        let payments_received_path = "data/payments_received.json";
        if !std::path::Path::new(payments_received_path).exists() {
            if let Err(e) = std::fs::File::create(payments_received_path) {
                error!("Failed to create payments_received.json: {}", e);
                continue;
            }
        }

        let db = match database::Db::new(payments_received_path.to_string()) {
            Ok(db) => db,
            Err(e) => {
                error!("Failed to load payments_received ledger: {}. Creating backup and starting fresh...", e);
                // Backup the corrupted file
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let backup_path = format!("data/payments_received.json.backup_{}", timestamp);
                if let Err(backup_err) = std::fs::copy(payments_received_path, &backup_path) {
                    warn!("Could not create backup: {}", backup_err);
                } else {
                    info!("Corrupted database backed up to: {}", backup_path);
                }
                // Start with empty database
                if let Err(write_err) = std::fs::write(payments_received_path, "[]") {
                    error!("Failed to reset database file: {}", write_err);
                    continue;
                }
                match database::Db::new(payments_received_path.to_string()) {
                    Ok(db) => db,
                    Err(e2) => {
                        error!("Failed to create fresh database: {}", e2);
                        continue;
                    }
                }
            }
        };
        if let Err(e) = db.write_payment(row) {
            error!("Failed to write payment to database: {}", e);
        }
        i += 1;
    }

    info!(
        "Init row in payments received ledger for circuit: {:?}",
        circuit_id
    );
}
