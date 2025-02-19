
use rand::Rng;
use sha2::{Digest, Sha256};
use hex;
use std::fmt::Write;
use base64;
use base64::decode;

// Generate random payment hash and preimage
pub fn get_random_payhash_and_preimage() -> (String, String) {
    let mut rng = rand::thread_rng();
    let preimage: [u8; 32] = rng.gen();
    let mut hasher = Sha256::new();
    hasher.update(&preimage);
    let payment_hash = hasher.finalize();
    (hex::encode(payment_hash), hex::encode(preimage))
}

pub fn microdesc_to_fingerprint(base64_id: &str) -> Option<String> {
    // Decode the Base64-encoded identity
    let bytes = decode(base64_id).ok()?;
    // Convert raw bytes to an uppercase hex string without spacing
    let mut hex_fingerprint = String::new();
    for byte in bytes.iter() {
        write!(&mut hex_fingerprint, "{:02X}", byte).ok()?;
    }
    println!("Tor Hex FP {}", hex_fingerprint);
    Some(hex_fingerprint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_microdesc_to_fingerprint() {
        // from /tor/status-vote/current/consensus 
        // r test004r MJyJq8PncKpIN+vpLoZmrnEAZDE UFllhJfsX6SMQoUQu02abxBAiig 2025-02-16 22:25:12 127.0.0.14 5059 0
        let base64_id = "MJyJq8PncKpIN+vpLoZmrnEAZDE";
        // to /tor/server/all
        // fingerprint 309C89ABC3E770AA4837EBE92E8666AE71006431
        let expected_fingerprint = "309C89ABC3E770AA4837EBE92E8666AE71006431";
        let result = microdesc_to_fingerprint(base64_id).unwrap();
        assert_eq!(result, expected_fingerprint);
    }
}
