use crate::database;
use crate::types::Relay;
use log::{error, info, warn};

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
                expires_at: chrono::Utc::now().timestamp()
                    + (relay.payment_interval_seconds.unwrap_or(60) as i64 * i), // expires now + 60 seconds for round 1, now + 120 seconds for round 2, etc
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
            // Create data folder if it doesn't exist
            // TODO read from config file
            if let Err(e) = std::fs::create_dir_all("data") {
                error!("Failed to create data directory: {}", e);
                continue;
            }
            // Create payments_sent.json file if it doesn't exist
            let payments_sent_path = "data/payments_sent.json";
            if !std::path::Path::new(payments_sent_path).exists() {
                if let Err(e) = std::fs::File::create(payments_sent_path) {
                    error!("Failed to create payments_sent.json: {}", e);
                    continue;
                }
            }
            let db = match database::Db::new(payments_sent_path.to_string()) {
                Ok(db) => db,
                Err(e) => {
                    error!("Failed to load payments_sent ledger: {}. Creating backup and starting fresh...", e);
                    // Backup the corrupted file
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    let backup_path = format!("data/payments_sent.json.backup_{}", timestamp);
                    if let Err(backup_err) = std::fs::copy(payments_sent_path, &backup_path) {
                        warn!("Could not create backup: {}", backup_err);
                    } else {
                        info!("Corrupted database backed up to: {}", backup_path);
                    }
                    // Start with empty database
                    if let Err(write_err) = std::fs::write(payments_sent_path, "[]") {
                        error!("Failed to reset database file: {}", write_err);
                        continue;
                    }
                    match database::Db::new(payments_sent_path.to_string()) {
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
    }
    info!(
        "Init row in payments sent ledger for circuit: {:?}",
        circuit_id
    );
}
