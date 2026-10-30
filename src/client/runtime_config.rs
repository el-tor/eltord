//! Explicit private client plan: stable local ownership and finite spending caps.
//! Restart reads the same scope; it never invents a replacement paid attempt.
use super::foundation::FoundationLimits;
use serde::Deserialize;
use std::{
    collections::HashSet,
    io::{self, Read},
    path::Path,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilePlan {
    scope: String,
    fingerprints: Vec<String>,
    lifetime_msat: u64,
    outstanding_msat: u64,
    per_round_msat: u64,
    routing_fee_per_hop_msat: u64,
    #[serde(default)]
    handshake_msat: u64,
    #[serde(default)]
    handshake_funding_timeout_ms: Option<u64>,
}
pub(crate) struct ClientPlanConfig {
    pub scope: [u8; 32],
    pub fingerprints: Vec<[u8; 20]>,
    pub limits: FoundationLimits,
    pub routing_fee_per_hop_msat: u64,
    pub handshake_msat: u64,
    pub handshake_funding_timeout_ms: Option<u64>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid private foundation client plan",
    )
}
fn decode<const N: usize>(s: &str) -> io::Result<[u8; N]> {
    if s.len() != N * 2
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let mut value = [0; N];
    hex::decode_to_slice(s, &mut value).map_err(|_| invalid())?;
    if value == [0; N] {
        return Err(invalid());
    }
    Ok(value)
}
impl ClientPlanConfig {
    /// This single-path runtime uses the same financial ceiling account-wide.
    /// The ledger refuses any change to a previously accepted account budget.
    pub(crate) fn account_budget(&self) -> crate::payment_ledger::outgoing::AccountBudget {
        crate::payment_ledger::outgoing::AccountBudget {
            lifetime_limit_msat: self.limits.lifetime_msat,
            outstanding_limit_msat: self.limits.outstanding_msat,
            max_queued_intents: 128,
            max_wallet_rpcs: 1,
            max_unresolved_intents: 128,
        }
    }
    pub(crate) fn load(path: &Path) -> io::Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(path)?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.len() > 8192 {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(invalid());
            }
        }
        let mut bytes = Vec::new();
        file.take(8193).read_to_end(&mut bytes)?;
        Self::parse(&bytes)
    }
    fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > 8192 {
            return Err(invalid());
        }
        let value: FilePlan = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        if value.fingerprints.is_empty()
            || value.fingerprints.len() > 16
            || value.per_round_msat > value.outstanding_msat
            || value.outstanding_msat > value.lifetime_msat
            || value.handshake_msat > value.outstanding_msat
            || (value.handshake_msat > 0 && value.handshake_funding_timeout_ms.is_none())
            || value
                .handshake_funding_timeout_ms
                .map_or(false, |ms| !(1..=300_000).contains(&ms))
        {
            return Err(invalid());
        }
        let scope = decode(&value.scope)?;
        let fingerprints = value
            .fingerprints
            .iter()
            .map(|v| decode::<20>(v))
            .collect::<io::Result<Vec<_>>>()?;
        let unique: HashSet<_> = fingerprints.iter().collect();
        if unique.len() != fingerprints.len() {
            return Err(invalid());
        }
        // The number of paid hops is known only after authenticated quoting.
        // Zero-priced hops dispatch no payment and incur no routing fee. The
        // exact positive-hop sum is checked before constructing the circuit.
        if value.routing_fee_per_hop_msat > value.per_round_msat {
            return Err(invalid());
        }
        Ok(Self {
            scope,
            fingerprints,
            limits: FoundationLimits {
                lifetime_msat: value.lifetime_msat,
                outstanding_msat: value.outstanding_msat,
                per_round_msat: value.per_round_msat,
            },
            routing_fee_per_hop_msat: value.routing_fee_per_hop_msat,
            handshake_msat: value.handshake_msat,
            handshake_funding_timeout_ms: value.handshake_funding_timeout_ms,
        })
    }
    /// Old signed prices remain pinned by the saved plan. Only user-controlled
    /// path and spending terms are compared here; recovery never requotes.
    pub(crate) fn matches_saved_context(&self, bytes: &[u8]) -> io::Result<bool> {
        if bytes.len() > 16384 {
            return Err(invalid());
        }
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        let limits = serde_json::to_value(self.limits).map_err(|_| invalid())?;
        let hops = value["hops"].as_array().ok_or_else(invalid)?;
        Ok(
            value["handshake_msat"].as_u64().unwrap_or(0) == self.handshake_msat
                && value["handshake_funding_timeout_ms"].as_u64()
                    == self.handshake_funding_timeout_ms
                && value["limits"] == limits
                && value["routing_fee_per_hop_msat"].as_u64()
                    == Some(self.routing_fee_per_hop_msat)
                && hops.len() == self.fingerprints.len()
                && hops
                    .iter()
                    .zip(&self.fingerprints)
                    .all(|(hop, fp)| hop["fingerprint"].as_str() == Some(hex::encode(fp).as_str())),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> serde_json::Value {
        serde_json::json!({"scope":hex::encode([1;32]),"fingerprints":[hex::encode([2;20])],"lifetime_msat":100,"outstanding_msat":20,"per_round_msat":10,"routing_fee_per_hop_msat":1})
    }
    #[cfg(unix)]
    #[test]
    fn private_plan_refuses_symlinks_public_permissions_and_oversize() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("eltor-plan-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("plan.json");
        std::fs::write(&path, serde_json::to_vec(&input()).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(ClientPlanConfig::load(&path).is_ok());
        let link = dir.join("link.json");
        symlink(&path, &link).unwrap();
        assert!(ClientPlanConfig::load(&link).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(ClientPlanConfig::load(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&path, vec![b' '; 8193]).unwrap();
        assert!(ClientPlanConfig::load(&path).is_err());
        assert!(ClientPlanConfig::load(&dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn handshake_spending_requires_explicit_cap_and_finite_window() {
        let original = input();
        let plan = ClientPlanConfig::parse(&serde_json::to_vec(&original).unwrap()).unwrap();
        assert_eq!(plan.handshake_msat, 0);
        assert_eq!(plan.handshake_funding_timeout_ms, None);
        let mut paid = original;
        paid["handshake_msat"] = serde_json::json!(10);
        assert!(ClientPlanConfig::parse(&serde_json::to_vec(&paid).unwrap()).is_err());
        for ms in [0, 300_001, u64::MAX] {
            paid["handshake_funding_timeout_ms"] = serde_json::json!(ms);
            assert!(ClientPlanConfig::parse(&serde_json::to_vec(&paid).unwrap()).is_err());
        }
        paid["handshake_funding_timeout_ms"] = serde_json::json!(60_000);
        let plan = ClientPlanConfig::parse(&serde_json::to_vec(&paid).unwrap()).unwrap();
        let saved = serde_json::json!({"limits":plan.limits,"routing_fee_per_hop_msat":1,"hops":[{"fingerprint":hex::encode([2;20])}]});
        assert!(!plan
            .matches_saved_context(&serde_json::to_vec(&saved).unwrap())
            .unwrap());
        paid["handshake_msat"] = serde_json::json!(21);
        assert!(ClientPlanConfig::parse(&serde_json::to_vec(&paid).unwrap()).is_err());
    }
    #[test]
    fn runtime_account_limits_cannot_increase_after_restart() {
        use crate::payment_ledger::{AccountId, PaymentLedger};
        let directory =
            std::env::temp_dir().join(format!("eltor-plan-budget-{}", rand::random::<u128>()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("ledger.sqlite");
        let plan = ClientPlanConfig::parse(&serde_json::to_vec(&input()).unwrap()).unwrap();
        let account = AccountId([11; 32]);
        let mut ledger = PaymentLedger::provision(&path, account).unwrap();
        ledger
            .configure_account_budget(plan.account_budget())
            .unwrap();
        drop(ledger);
        let mut ledger = PaymentLedger::reopen(&path, account).unwrap();
        let mut changed = input();
        changed["lifetime_msat"] = serde_json::json!(101);
        let changed = ClientPlanConfig::parse(&serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(ledger
            .configure_account_budget(changed.account_budget())
            .is_err());
        ledger
            .configure_account_budget(plan.account_budget())
            .unwrap();
        drop(ledger);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn explicit_finite_plan_and_exact_recovery_terms() {
        let plan = ClientPlanConfig::parse(&serde_json::to_vec(&input()).unwrap()).unwrap();
        assert_eq!(plan.scope, [1; 32]);
        let mut saved = serde_json::json!({"limits":plan.limits,"routing_fee_per_hop_msat":1,"hops":[{"fingerprint":hex::encode([2;20]),"terms":"retained signed price"}]});
        assert!(plan
            .matches_saved_context(&serde_json::to_vec(&saved).unwrap())
            .unwrap());
        saved["limits"]["lifetime_msat"] = serde_json::json!(101);
        assert!(!plan
            .matches_saved_context(&serde_json::to_vec(&saved).unwrap())
            .unwrap());
        for (key, value) in [
            ("scope", serde_json::json!(hex::encode([0; 32]))),
            (
                "fingerprints",
                serde_json::json!([hex::encode([2; 20]), hex::encode([2; 20])]),
            ),
            ("per_round_msat", serde_json::json!(21)),
            ("routing_fee_per_hop_msat", serde_json::json!(u64::MAX)),
            ("extra", serde_json::json!(true)),
        ] {
            let mut bad = input();
            bad[key] = value;
            assert!(ClientPlanConfig::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
        }
    }
}
