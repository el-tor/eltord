pub struct RelayPayments {
    pub handshake_payment_hash: String,
    pub handshake_preimage: String,
    pub payhashes: Vec<String>,
}
// write a parser for the wire_format ro RelayPayments
impl RelayPayments {
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