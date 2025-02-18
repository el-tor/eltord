use crate::rpc::{extend_paid_circuit, Relay, RpcConfig};
use rand::Rng;
use sha2::{Digest, Sha256};
use hex;

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
    relays: Vec<Relay>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut extend_paid_circuit_rows = Vec::new();

    
    for relay in relays {
        let (handshake_payhash, handshake_preimage) = get_random_payhash_and_preimage();
        print!("Handshake Payment Hash: {}\n", handshake_payhash);
        print!("Handshake Payment Preimage: {}\n", handshake_preimage);
        let mut payment_hashes = Vec::new();
        let mut payment_ids_concatinated_10 = String::new();
        for _ in 0..10 {
            let (payhash, preimage) = get_random_payhash_and_preimage();
            payment_hashes.push(payhash.clone());
            payment_ids_concatinated_10.push_str(&payhash);
        }

        let row = ExtendPaidCircuitRow {
            relay_fingerprint: relay.fingerprint.clone(),
            handshake_fee_payment_hash: handshake_payhash,
            handshake_fee_preimage: handshake_preimage,
            payment_ids_concatinated_10: payment_ids_concatinated_10,
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
    print!("EXTENDPAIDCIRCUIT Command: {}", command);
    let circuit_id = extend_paid_circuit(&rpc_config, command).await.unwrap();
    Ok(circuit_id)
}

// Generate random payment hash and preimage
fn get_random_payhash_and_preimage() -> (String, String) {
    let mut rng = rand::thread_rng();
    let preimage: [u8; 32] = rng.gen();
    let mut hasher = Sha256::new();
    hasher.update(&preimage);
    let payment_hash = hasher.finalize();
    (hex::encode(payment_hash), hex::encode(preimage))
}