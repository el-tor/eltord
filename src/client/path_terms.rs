//! Read-only pre-construction price checks. Passing these checks is not relay
//! admission, invoice verification, or permission to dispatch a payment.
use crate::{
    rpc::{foundation_descriptor, FoundationDescriptor, FoundationPolicy},
    types::{Relay, RpcConfig},
};
use std::{collections::HashSet, io};
#[derive(Clone, Copy)]
pub(crate) struct PathPriceLimits {
    pub round_msat: u64,
    pub handshake_msat: u64,
}
pub(crate) struct AcceptedPathTerms {
    pub descriptors: Vec<FoundationDescriptor>,
    pub round_msat: u64,
    pub handshake_msat: u64,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Foundation path exceeds accepted terms",
    )
}
impl AcceptedPathTerms {
    fn validate(
        descriptors: Vec<FoundationDescriptor>,
        limits: PathPriceLimits,
    ) -> io::Result<Self> {
        if descriptors.is_empty() || descriptors.len() > 16 {
            return Err(invalid());
        }
        let mut seen = HashSet::new();
        let mut round_msat = 0u64;
        let mut handshake_msat = 0u64;
        for descriptor in &descriptors {
            if descriptor.fingerprint == [0; 20] || !seen.insert(descriptor.fingerprint) {
                return Err(invalid());
            }
            let policy = FoundationPolicy::decode(&descriptor.terms)?;
            round_msat = round_msat
                .checked_add(policy.rate_msat)
                .ok_or_else(invalid)?;
            handshake_msat = handshake_msat
                .checked_add(policy.handshake_fee_msat)
                .ok_or_else(invalid)?;
            if round_msat > limits.round_msat || handshake_msat > limits.handshake_msat {
                return Err(invalid());
            }
        }
        Ok(Self {
            descriptors,
            round_msat,
            handshake_msat,
        })
    }
}
/// Preserve the caller's selected path and order (including its guard/isolation
/// policy). A failed query never silently replaces a hop or increases a budget.
pub(crate) async fn acquire_path_terms(
    config: &RpcConfig,
    relays: &[Relay],
    limits: PathPriceLimits,
) -> io::Result<AcceptedPathTerms> {
    if relays.is_empty() || relays.len() > 16 {
        return Err(invalid());
    }
    let mut fingerprints = Vec::with_capacity(relays.len());
    for relay in relays {
        let mut fingerprint = [0; 20];
        hex::decode_to_slice(&relay.fingerprint, &mut fingerprint).map_err(|_| invalid())?;
        if fingerprint == [0; 20] || fingerprints.contains(&fingerprint) {
            return Err(invalid());
        }
        fingerprints.push(fingerprint);
    }
    let mut descriptors = Vec::with_capacity(relays.len());
    for fingerprint in fingerprints {
        descriptors.push(foundation_descriptor(config, fingerprint).await?);
    }
    // Routing fees and lifetime/outstanding exposure are separately accepted and
    // enforced by the durable outgoing ledger before dispatch, not inferred here.
    AcceptedPathTerms::validate(descriptors, limits)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor(id: u8, rate: u64, handshake: u64) -> FoundationDescriptor {
        let mut terms = [0; 55];
        terms[..3].copy_from_slice(&[1, 1, 10]);
        terms[3..11].copy_from_slice(&rate.to_be_bytes());
        terms[11..15].copy_from_slice(&60000u32.to_be_bytes());
        terms[15..19].copy_from_slice(&60000u32.to_be_bytes());
        terms[31..39].copy_from_slice(&1800000u64.to_be_bytes());
        terms[39..43].copy_from_slice(&3u32.to_be_bytes());
        terms[43..51].copy_from_slice(&handshake.to_be_bytes());
        terms[51..55].copy_from_slice(&30000u32.to_be_bytes());
        FoundationDescriptor {
            fingerprint: [id; 20],
            terms,
            bolt12_offer: None,
        }
    }
    #[test]
    fn aggregate_terms_reject_overflow_and_budget_changes_without_truncation() {
        let limits = PathPriceLimits {
            round_msat: 6_000_000_000,
            handshake_msat: 10,
        };
        let path = vec![
            descriptor(1, 3_000_000_000, 4),
            descriptor(2, 3_000_000_000, 6),
        ];
        let accepted = AcceptedPathTerms::validate(path.clone(), limits).unwrap();
        assert_eq!(accepted.round_msat, 6_000_000_000);
        assert_eq!(accepted.handshake_msat, 10);
        assert_eq!(accepted.descriptors[1].fingerprint, [2; 20]);
        assert!(AcceptedPathTerms::validate(
            path.clone(),
            PathPriceLimits {
                round_msat: limits.round_msat - 1,
                ..limits
            }
        )
        .is_err());
        assert!(AcceptedPathTerms::validate(
            path,
            PathPriceLimits {
                handshake_msat: 9,
                ..limits
            }
        )
        .is_err());
        let unlimited = PathPriceLimits {
            round_msat: u64::MAX,
            handshake_msat: u64::MAX,
        };
        assert!(AcceptedPathTerms::validate(
            vec![descriptor(1, u64::MAX, 0), descriptor(2, 1, 0)],
            unlimited
        )
        .is_err());
        assert!(AcceptedPathTerms::validate(
            vec![descriptor(1, 0, u64::MAX), descriptor(2, 0, 1)],
            unlimited
        )
        .is_err());
        assert!(AcceptedPathTerms::validate(
            vec![descriptor(1, 0, 0), descriptor(1, 0, 0)],
            unlimited
        )
        .is_err());
        assert!(AcceptedPathTerms::validate(vec![], unlimited).is_err());
        assert!(AcceptedPathTerms::validate(
            vec![descriptor(1, 0, 0)],
            PathPriceLimits {
                round_msat: 0,
                handshake_msat: 0
            }
        )
        .is_ok());
    }
}
