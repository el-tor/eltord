use std::fmt::Write;
use base64;
use base64::decode;

pub fn microdesc_to_fingerprint(base64_id: &str) -> Option<String> {
    // Decode the Base64-encoded identity
    let bytes = decode(base64_id).ok()?;
    // Convert raw bytes to an uppercase hex string with spacing
    let mut hex_fingerprint = String::new();
    for (i, byte) in bytes.iter().enumerate() {
        if i > 0 && i % 2 == 0 {
            hex_fingerprint.push(' ');
        }
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
        // fingerprint 309C 89AB C3E7 70AA 4837 EBE9 2E86 66AE 7100 6431
        let expected_fingerprint = "309C 89AB C3E7 70AA 4837 EBE9 2E86 66AE 7100 6431";
        let result = microdesc_to_fingerprint(base64_id).unwrap();
        assert_eq!(result, expected_fingerprint);
    }
}
