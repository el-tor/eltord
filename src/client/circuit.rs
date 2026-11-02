use crate::rpc;
use crate::types::{Relay, RpcConfig};
use crate::utils::get_random_payhash_and_preimage;
use log::info;

struct ExtendPaidCircuitRow {
    relay_fingerprint: String,
    handshake_fee_payment_hash: String,
    handshake_fee_preimage: String,
    payment_ids_concatinated_10: String,
}

// 0. loop each relay and check if handshake fee is required, is so then pay the handshake fee and record the payment hash and preimage
// TODO skip for now since we are using a simple algo that does not require a handshake fee
// 1. generate a dummy payment hash and preimage for the handshake fee to pad the data for privacy
// 2. generate N (10 default) payment ids hashes one for each round in the interval. These will be passed to the relay to verify the payment on their lightning node
//  if bolt12 is being used the payment id is passed in the bolt12 offer as a payer note
//  if bolt11 is being used then the payment id can be the pregenerated payment hash of a bolt11 invoice (make sure expiration of the invoice is bigger than the interval time)
// 3. EXTENDPAIDCIRCUIT with the relays fingerprint and payment id hashes
// 4. return the circuit id so the client can watch it.
pub async fn build_circuit(
    rpc_config: &RpcConfig,
    relays: &Vec<Relay>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let mut extend_paid_circuit_rows = Vec::new();

    for relay in relays.iter() {
        let row = ExtendPaidCircuitRow {
            relay_fingerprint: relay.fingerprint.clone(),
            handshake_fee_payment_hash: relay
                .payment_handshake_fee_payhash
                .clone()
                .unwrap_or_default(),
            handshake_fee_preimage: relay
                .payment_handshake_fee_preimage
                .clone()
                .unwrap_or_default(),
            payment_ids_concatinated_10: relay
                .payment_id_hashes_10
                .clone()
                .unwrap_or_default()
                .join(""),
        };
        extend_paid_circuit_rows.push(row);
    }

    let mut command = String::from("+EXTENDPAIDCIRCUIT 0\n");
    for row in extend_paid_circuit_rows {
        command.push_str(&format!(
            "{} {}{}{}\n",
            row.relay_fingerprint,
            row.handshake_fee_payment_hash,
            row.handshake_fee_preimage,
            row.payment_ids_concatinated_10
        ));
    }
    command.push_str(".");
    info!("Requesting paid circuit construction");
    let circuit_id = rpc::extend_paid_circuit(&rpc_config, command)
        .await
        .map_err(|e| e.to_string())?;
    let mut public_relays = serde_json::to_value(relays)?;
    redact_payment_material(&mut public_relays);
    let event_data = serde_json::json!({
        "event": "CIRCUIT_BUILT",
        "circuit_id": circuit_id,
        "relays": public_relays
    });
    info!("EVENT:{}:ENDEVENT", event_data.to_string());
    Ok(circuit_id)
}

/// Encode the foundation constructor without allowing framing injection or
/// silently truncating a period. The caller must durably retain this exact path
/// and its payment material before submission; this function never pays.
/// Handshake proof verification against the native wallet remains a separate
/// requirement for a nonzero fee.
pub(crate) fn foundation_constructor(relays: &[Relay]) -> Result<String, std::io::Error> {
    use sha2::{Digest, Sha256};
    use std::collections::HashSet;
    fn invalid() -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid foundation constructor",
        )
    }
    fn decode(s: &str, size: usize) -> Result<Vec<u8>, std::io::Error> {
        if s.len() != size * 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        hex::decode(s).map_err(|_| invalid())
    }
    if relays.is_empty() || relays.len() > 16 {
        return Err(invalid());
    }
    let mut fingerprints = HashSet::new();
    let mut ids = HashSet::new();
    let mut command = String::from("+EXTENDPAIDCIRCUIT 0\r\n");
    for relay in relays {
        let fingerprint = decode(&relay.fingerprint, 20)?;
        if fingerprint.iter().all(|b| *b == 0) || !fingerprints.insert(fingerprint.clone()) {
            return Err(invalid());
        }
        let hash = decode(
            relay
                .payment_handshake_fee_payhash
                .as_deref()
                .ok_or_else(invalid)?,
            32,
        )?;
        let preimage = decode(
            relay
                .payment_handshake_fee_preimage
                .as_deref()
                .ok_or_else(invalid)?,
            32,
        )?;
        if Sha256::digest(&preimage).as_slice() != hash.as_slice() {
            return Err(invalid());
        }
        let rounds = relay.payment_id_hashes_10.as_ref().ok_or_else(invalid)?;
        if rounds.len() != 10 {
            return Err(invalid());
        }
        command.push_str(&hex::encode(fingerprint));
        command.push(' ');
        command.push_str(&hex::encode(hash));
        command.push_str(&hex::encode(preimage));
        for id in rounds {
            let id = decode(id, 32)?;
            // Even across hops, shared IDs would introduce unwanted linkability.
            if id.iter().all(|b| *b == 0) || !ids.insert(id.clone()) {
                return Err(invalid());
            }
            command.push_str(&hex::encode(id));
        }
        command.push_str("\r\n");
    }
    command.push_str(".\r\n");
    if command.len() > 16384 {
        return Err(invalid());
    }
    Ok(command)
}

pub fn kill_circuit() {
    // TODO
}

pub fn pregen_extend_paid_circuit_hashes(
    selected_relays: &mut Vec<Relay>,
    payment_rounds: u16,
) -> &Vec<Relay> {
    for relay in selected_relays.iter_mut() {
        // Generate payhash and preimage for handshake fee
        let (handshake_payhash, handshake_preimage) = get_random_payhash_and_preimage();
        relay.payment_handshake_fee_payhash = Some(handshake_payhash);
        relay.payment_handshake_fee_preimage = Some(handshake_preimage);

        // Generate 10 payment id hashes for each round of payment in the circuit lifetime
        let mut payment_id_hashes_10 = Vec::new();
        for _ in 0..payment_rounds {
            payment_id_hashes_10.push(get_random_payhash_and_preimage().0);
        }
        relay.payment_id_hashes_10 = Some(payment_id_hashes_10);
    }
    selected_relays
}

// UI path metadata must not export credentials or cross-round payment IDs to
// the persistent log stream. Keep the original in-memory relay records intact.
fn redact_payment_material(relays: &mut serde_json::Value) {
    if let Some(rows) = relays.as_array_mut() {
        for row in rows {
            if let Some(object) = row.as_object_mut() {
                for key in [
                    "payment_handshake_fee_payhash",
                    "payment_handshake_fee_preimage",
                    "payment_id_hashes_10",
                ] {
                    object.remove(key);
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    fn foundation_relay(n: u8) -> crate::types::Relay {
        use sha2::{Digest, Sha256};
        crate::types::Relay {
            nickname: String::new(),
            fingerprint: hex::encode([n; 20]),
            contact: None,
            bandwidth: None,
            ip: None,
            port: None,
            payment_bolt12_offer: None,
            payment_bip353: None,
            payment_bolt11_lnurl: None,
            payment_bolt11_lightning_address: None,
            payment_rate_msats: Some(0),
            payment_interval_seconds: Some(60),
            payment_interval_rounds: Some(10),
            payment_handshake_fee: Some(0),
            payment_handshake_fee_payhash: Some(hex::encode(Sha256::digest([n; 32]))),
            payment_handshake_fee_preimage: Some(hex::encode([n; 32])),
            payment_id_hashes_10: Some(
                (1..=10)
                    .map(|i| {
                        let mut id = [n; 32];
                        id[31] = i;
                        hex::encode(id)
                    })
                    .collect(),
            ),
            relay_tag: None,
            hop: None,
        }
    }
    #[test]
    fn foundation_constructor_bounds_framing_and_independent_hop_ids() {
        let mut relays: Vec<_> = (1..=16).map(foundation_relay).collect();
        let command = super::foundation_constructor(&relays).unwrap();
        assert!(command.len() <= 16384);
        let rows: Vec<_> = command.lines().collect();
        assert_eq!(rows.len(), 18);
        assert_eq!(rows[0], "+EXTENDPAIDCIRCUIT 0");
        assert_eq!(rows[17], ".");
        for row in &rows[1..17] {
            assert_eq!(row.len(), 40 + 1 + 768);
        }
        relays.push(foundation_relay(17));
        assert!(super::foundation_constructor(&relays).is_err());
        assert!(super::foundation_constructor(&[]).is_err());
        let base = vec![foundation_relay(1), foundation_relay(2)];
        let mut bad = base.clone();
        bad[1].payment_id_hashes_10 = bad[0].payment_id_hashes_10.clone();
        assert!(super::foundation_constructor(&bad).is_err());
        let mut bad = base.clone();
        bad[0].payment_id_hashes_10.as_mut().unwrap().pop();
        assert!(super::foundation_constructor(&bad).is_err());
        let mut bad = base.clone();
        bad[0].fingerprint.push_str("\r\n.");
        assert!(super::foundation_constructor(&bad).is_err());
        let mut bad = base.clone();
        bad[0].payment_handshake_fee_preimage = Some(hex::encode([99; 32]));
        assert!(super::foundation_constructor(&bad).is_err());
        let mut bad = base;
        bad[1].fingerprint = bad[0].fingerprint.clone();
        assert!(super::foundation_constructor(&bad).is_err());
    }
    #[tokio::test]
    async fn lost_constructor_reply_cannot_rebuild_after_ledger_restart() {
        use crate::{
            client::construction::PreparedConstruction,
            payment_ledger::{AccountId, ClockObservation, PaymentLedger},
            rpc::PaymentControl,
            types::RpcConfig,
        };
        use tokio::{
            io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
            net::TcpListener,
        };
        let directory = std::env::temp_dir().join(format!(
            "eltor-constructor-recovery-{}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("ledger.sqlite");
        let account = AccountId([9; 32]);
        let mut ledger = PaymentLedger::provision(&path, account).unwrap();
        let mut relay = foundation_relay(1);
        use sha2::{Digest, Sha256};
        relay.payment_handshake_fee_preimage = Some(hex::encode([77; 32]));
        relay.payment_handshake_fee_payhash = Some(hex::encode(Sha256::digest([77; 32])));
        let prepared = PreparedConstruction::new(
            [4; 32],
            [1; 16],
            &[relay.clone()],
            b"accepted fixture terms",
            ClockObservation {
                now_ms: 100000,
                uncertainty_ms: 0,
            },
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = RpcConfig {
            addr: listener.local_addr().unwrap().to_string(),
            rpc_password: None,
            command: String::new(),
        };
        let server = tokio::spawn(async move {
            for attempt in 0..2 {
                let mut socket = BufReader::new(listener.accept().await.unwrap().0);
                for expected in ["AUTHENTICATE\r\n", "SETEVENTS ELTOR_PAYMENT_STATE\r\n"] {
                    let mut line = String::new();
                    socket.read_line(&mut line).await.unwrap();
                    assert_eq!(line, expected);
                    socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
                }
                if attempt == 1 {
                    let mut line = String::new();
                    assert_eq!(socket.read_line(&mut line).await.unwrap(), 0);
                    continue;
                }
                let mut line = String::new();
                socket.read_line(&mut line).await.unwrap();
                assert_eq!(line, "ELTOR_PAYMENT_LIST 1 - -\r\n");
                socket
                    .get_mut()
                    .write_all(
                        format!(
                            "250-ELTOR_PAYMENT_LIST 1 {} 0 -\r\n250 OK\r\n",
                            hex::encode([1; 16])
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                let mut line = String::new();
                socket.read_line(&mut line).await.unwrap();
                assert_eq!(line, "+EXTENDPAIDCIRCUIT 0\r\n");
                loop {
                    let mut line = String::new();
                    socket.read_line(&mut line).await.unwrap();
                    if line == ".\r\n" {
                        break;
                    }
                }
                // Tor received the whole constructor, but its reply is lost.
            }
        });
        let mut control = PaymentControl::connect(&config).await.unwrap();
        assert!(prepared.dispatch(&mut ledger, &mut control).await.is_err());
        drop(control);
        drop(ledger);
        let mut ledger = PaymentLedger::reopen(&path, account).unwrap();
        let mut prepared = PreparedConstruction::restore(&ledger, [4; 32])
            .unwrap()
            .unwrap();
        assert!(
            ledger
                .get_origin_construction([4; 32])
                .unwrap()
                .unwrap()
                .dispatch_claimed
        );
        let saved = ledger.get_origin_construction([4; 32]).unwrap().unwrap();
        let persisted = String::from_utf8(saved.payload).unwrap();
        assert!(!persisted.contains(&hex::encode([77; 32])));
        let record: serde_json::Value = serde_json::from_str(&persisted).unwrap();
        assert_eq!(record["version"], 2);
        assert!(record.get("command").is_none());
        let mut altered = relay.clone();
        altered.payment_id_hashes_10.as_mut().unwrap()[0] = hex::encode([99; 32]);
        assert!(prepared.restore_proofs(&[altered]).is_err());
        prepared.restore_proofs(&[relay]).unwrap();
        // Even exact recovered proofs cannot clear the original dispatch claim.
        let mut control = PaymentControl::connect(&config).await.unwrap();
        assert!(prepared.dispatch(&mut ledger, &mut control).await.is_err());
        drop(control);
        server.await.unwrap();
        drop(ledger);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn circuit_ui_metadata_excludes_payment_material() {
        let mut value = serde_json::json!([{"fingerprint":"public relay", "payment_rate_msats":1000,
            "payment_handshake_fee_payhash":"private hash", "payment_handshake_fee_preimage":"private preimage", "payment_id_hashes_10":["private id"]}]);
        super::redact_payment_material(&mut value);
        assert_eq!(
            value,
            serde_json::json!([{"fingerprint":"public relay", "payment_rate_msats":1000}])
        );
    }
}
