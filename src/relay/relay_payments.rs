pub struct RelayPayments {
    pub handshake_payment_hash: String,
    pub handshake_preimage: String,
    pub payhashes: Vec<String>,
}
impl RelayPayments {
    // Parser for the wire_format to RelayPayments
    // Relay Payment hash wire_format is 12 (64 char) hashes concatenated together
    // "handshake_payment_hash + handshake_preimage + payment_id_hash_round1 + payment_id_hash_round2 + ...payment_id_hash_round10"
    pub fn from_wire_format(wire_format: &str) -> Self {
        let chunks: Vec<String> = wire_format
            .as_bytes()
            .chunks(64)
            .map(|chunk| String::from_utf8_lossy(chunk).to_string())
            .collect();
        let handshake_payment_hash = chunks.get(0).cloned().unwrap_or_default();
        let handshake_preimage = chunks.get(1).cloned().unwrap_or_default();
        let payhashes = if chunks.len() > 2 {
            chunks[2..].to_vec()
        } else {
            Vec::new()
        };
        RelayPayments {
            handshake_payment_hash,
            handshake_preimage,
            payhashes,
        }
    }
}
