//! Shared signed policy decoding, byte-for-byte with Tor payment_profile.c.
use std::{convert::TryInto, io};
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FoundationPolicy {
    pub rate_msat: u64,
    pub grace_ms: u32,
    pub paid_quota_bytes: u64,
    pub max_age_ms: u64,
    pub max_periods: u32,
    pub handshake_fee_msat: u64,
    pub setup_timeout_ms: u32,
}
impl FoundationPolicy {
    pub fn decode(terms: &[u8; 55]) -> io::Result<Self> {
        let u32_at = |at| u32::from_be_bytes(terms[at..at + 4].try_into().unwrap());
        let u64_at = |at| u64::from_be_bytes(terms[at..at + 8].try_into().unwrap());
        let out = Self {
            rate_msat: u64_at(3),
            grace_ms: u32_at(19),
            paid_quota_bytes: u64_at(23),
            max_age_ms: u64_at(31),
            max_periods: u32_at(39),
            handshake_fee_msat: u64_at(43),
            setup_timeout_ms: u32_at(51),
        };
        if terms[..3] != [1, 1, 10]
            || u32_at(11) != 60000
            || u32_at(15) != 60000
            || out.grace_ms > 60000
            || out.max_age_ms == 0
            || out.max_periods < 2
            || out.setup_timeout_ms == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid foundation policy",
            ));
        }
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_matches_shared_vector_and_rejects_invalid_schedule() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../spec/vectors/payment_renewal_v1.json"))
                .unwrap();
        for f in fixture["fixtures"].as_array().unwrap() {
            let terms: [u8; 55] = hex::decode(f["terms_hex"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap();
            let p = FoundationPolicy::decode(&terms).unwrap();
            assert_eq!(p.rate_msat, 1000 * f["hop"].as_u64().unwrap());
            assert_eq!(p.grace_ms, 15000);
            assert_eq!(p.max_age_ms, 86400000);
            assert_eq!(p.max_periods, 144);
            assert_eq!(p.handshake_fee_msat, 0);
            assert_eq!(p.setup_timeout_ms, 30000);
            for (at, len) in [(11, 4), (15, 4), (31, 8), (39, 4), (51, 4)] {
                let mut bad = terms;
                bad[at..at + len].fill(0);
                assert!(FoundationPolicy::decode(&bad).is_err());
            }
            let mut bad = terms;
            bad[19..23].copy_from_slice(&60001u32.to_be_bytes());
            assert!(FoundationPolicy::decode(&bad).is_err());
        }
    }
}
