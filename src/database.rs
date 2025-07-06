use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("IoError: {reason}")]
    IoErr { reason: String },
    #[error("SerializationError: {reason}")]
    SerializationErr { reason: String },
    #[error("DeserializationError: {reason}")]
    DeserializationErr { reason: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Payment {
    pub payment_id: String,
    pub circ_id: String,
    pub interval_seconds: i64,
    pub round: i64,
    pub relay_fingerprint: String,
    pub updated_at: i64,
    pub amount_msat: i64,
    pub handshake_fee_payhash: Option<String>,
    pub handshake_fee_preimage: Option<String>,
    pub paid: bool,
    pub expires_at: i64,
    pub bolt11_invoice: Option<String>,
    pub bolt12_offer: Option<String>,
    pub payment_hash: Option<String>,
    pub preimage: Option<String>,
    pub fee: Option<i64>,
    pub has_error: bool,
}

#[derive(Debug, Deserialize)]
pub struct Db {
    path: String,
    #[serde(skip)]
    data: Arc<Mutex<Vec<Payment>>>,
}

impl Db {
    pub fn new(path: String) -> Result<Self, DbError> {
        let data = if let Ok(mut file) = File::open(&path) {
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .map_err(|e| DbError::IoErr {
                    reason: e.to_string(),
                })?;
            if contents.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&contents).map_err(|e| DbError::DeserializationErr {
                    reason: e.to_string(),
                })?
            }
        } else {
            Vec::new()
        };

        Ok(Self {
            path,
            data: Arc::new(Mutex::new(data)),
        })
    }

    pub fn save(&self) -> Result<(), DbError> {
        let data = self.data.lock().unwrap();
        let json = serde_json::to_string_pretty(&*data).map_err(|e| DbError::SerializationErr {
            reason: e.to_string(),
        })?;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.path)
            .map_err(|e| DbError::IoErr {
                reason: e.to_string(),
            })?;
        file.write_all(json.as_bytes())
            .map_err(|e| DbError::IoErr {
                reason: e.to_string(),
            })?;
        Ok(())
    }

    pub fn write_payment(&self, payment: Payment) -> Result<(), DbError> {
        let mut data = self.data.lock().unwrap();
        data.push(payment);
        drop(data); // Explicitly drop the lock before saving
        self.save()
    }

    // todo update row function by payment_id
    pub fn update_payment(&self, payment: Payment) -> Result<(), DbError> {
        let mut data = self.data.lock().unwrap();
        let index = data
            .iter()
            .position(|p| p.payment_id == payment.payment_id)
            .ok_or(DbError::IoErr {
                reason: "Payment not found".to_string(),
            })?;
        data[index] = payment;
        drop(data); // Explicitly drop the lock before saving
        self.save()
    }


    pub fn lookup_payment_by_id(&self, payment_id: String) -> Result<Option<Payment>, DbError> {
        let data = self.data.lock().unwrap();
        Ok(data
            .iter()
            .find(|payment| payment.payment_id == payment_id)
            .cloned())
    }

    pub fn lookup_payments(&self, circuit_id: String, round: i64) -> Result<Vec<Payment>, DbError> {
        let data = self.data.lock().unwrap();
        Ok(data
            .iter()
            .filter(|payment| payment.circ_id == circuit_id && payment.round == round)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db() {
        let payment = Payment {
            payment_id: "1".to_string(),
            circ_id: "1".to_string(),
            interval_seconds: 60,
            round: 1,
            relay_fingerprint: "1".to_string(),
            updated_at: 1,
            amount_msat: 1,
            handshake_fee_payhash: Some("1".to_string()),
            handshake_fee_preimage: Some("1".to_string()),
            paid: false.clone(),
            expires_at: 1,
            bolt11_invoice: None,
            bolt12_offer: None,
            payment_hash: None,
            preimage: None,
            fee: None,
            has_error: false,
        };
        let payment2 = Payment {
            payment_id: "2".to_string(),
            circ_id: "1".to_string(),
            interval_seconds: 60,
            round: 2,
            relay_fingerprint: "1".to_string(),
            updated_at: 1,
            amount_msat: 1,
            handshake_fee_payhash: Some("1".to_string()),
            handshake_fee_preimage: Some("1".to_string()),
            paid: false.clone(),
            expires_at: 1,
            bolt11_invoice: None,
            bolt12_offer: None,
            payment_hash: None,
            preimage: None,
            fee: None,
            has_error: false,
        };

        let db = Db::new("data/payments_sent.json".to_string()).unwrap();

        db.write_payment(payment).unwrap();
        db.write_payment(payment2).unwrap();

        let payment_lookup = db.lookup_payment_by_id("1".to_string()).unwrap();
        assert_eq!(payment_lookup.unwrap().payment_id, "1".to_string());

        let relays_to_pay = db.lookup_payments("1".to_string(), 2).unwrap();
        assert_eq!(relays_to_pay[0].payment_id, "2".to_string());
    }
}
