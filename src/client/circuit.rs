use crate::rpc;
use crate::types::{Relay, RpcConfig};
use crate::utils::get_random_payhash_and_preimage;
use log::{debug, info};

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
    info!("EXTENDPAIDCIRCUIT Command: {}", command);
    let circuit_id = rpc::extend_paid_circuit(&rpc_config, command)
        .await
        .unwrap();
    let event_data = serde_json::json!({
        "event": "CIRCUIT_BUILT",
        "circuit_id": circuit_id,
        "relays": relays
    });
    info!("EVENT:{}:ENDEVENT", event_data.to_string());
    Ok(circuit_id)
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
        info!("Handshake Payment Hash: {}\n", handshake_payhash);
        info!("Handshake Payment Preimage: {}\n", handshake_preimage);
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
