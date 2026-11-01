//! Explicit payer config; no inference, provisioning fallback or wallet switch.
use crate::{
    payment_ledger::{payer::FoundationPayer, AccountId, PaymentLedger},
    relay::FoundationConfig,
};
use lni::{
    cln::foundation::{Chain, InvoiceVerifier, NativeAccount},
    phoenixd::{foundation::PhoenixdFoundation, PhoenixdConfig},
};
use serde::Deserialize;
use std::convert::TryInto;
use std::{
    error::Error,
    io::{self, Read},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid private foundation payer configuration",
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    wallet: String,
    ledger_path: PathBuf,
    node_id: String,
    chain: String,
    phoenixd_url: String,
    phoenixd_password_file: PathBuf,
    clock_uncertainty_ms: u64,
}
pub(crate) enum PayerConfig {
    Cln(FoundationConfig),
    Phoenixd {
        ledger: PathBuf,
        account: NativeAccount,
        wallet: PhoenixdConfig,
        uncertainty: u64,
    },
}
fn read_private(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > max as u64 {
        return Err(invalid());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if meta.permissions().mode() & 0o077 != 0 || meta.uid() != unsafe { libc::geteuid() } {
            return Err(invalid());
        }
    }
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(invalid());
    }
    Ok(bytes)
}
fn resolve(base: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}
impl PayerConfig {
    pub(crate) fn load(path: &Path) -> io::Result<Self> {
        let bytes = read_private(path, 8192)?;
        let base = path.parent().unwrap_or(Path::new("."));
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if value.get("wallet").is_none() {
            return Ok(Self::Cln(FoundationConfig::parse(&bytes, base)?));
        }
        let v: FileConfig = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if v.wallet != "phoenixd"
            || v.clock_uncertainty_ms > 5000
            || v.node_id.len() != 66
            || !v
                .node_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || v.ledger_path.as_os_str().is_empty()
            || v.phoenixd_password_file.as_os_str().is_empty()
            || v.phoenixd_url.len() > 2048
        {
            return Err(invalid());
        }
        let node_id = hex::decode(v.node_id)
            .map_err(|_| invalid())?
            .try_into()
            .map_err(|_| invalid())?;
        let chain = match v.chain.as_str() {
            "bitcoin" => Chain::Bitcoin,
            "testnet" => Chain::Testnet,
            "regtest" => Chain::Regtest,
            "signet" => Chain::Signet,
            _ => return Err(invalid()),
        };
        let account = NativeAccount { node_id, chain };
        InvoiceVerifier::new(account).map_err(|_| invalid())?;
        let bytes = read_private(&resolve(base, v.phoenixd_password_file), 4096)?;
        let password = String::from_utf8(bytes)
            .map_err(|_| invalid())?
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        if password.is_empty() || !password.bytes().all(|b| (32..=126).contains(&b)) {
            return Err(invalid());
        }
        Ok(Self::Phoenixd {
            ledger: resolve(base, v.ledger_path),
            account,
            uncertainty: v.clock_uncertainty_ms,
            wallet: PhoenixdConfig {
                url: v.phoenixd_url,
                password,
                ..Default::default()
            },
        })
    }
    fn ledger_path(&self) -> &Path {
        match self {
            Self::Cln(v) => v.ledger_path(),
            Self::Phoenixd { ledger, .. } => ledger,
        }
    }
    fn account(&self) -> NativeAccount {
        match self {
            Self::Cln(v) => v.account(),
            Self::Phoenixd { account, .. } => *account,
        }
    }
    async fn connect(&self) -> Result<FoundationPayer> {
        match self {
            Self::Cln(v) => Ok(v.connect_outgoing_native().await?.into()),
            Self::Phoenixd {
                wallet, account, ..
            } => Ok(PhoenixdFoundation::connect(wallet.clone(), *account)
                .await?
                .into()),
        }
    }
    pub(crate) async fn open(self) -> Result<(PaymentLedger, FoundationPayer, u64)> {
        // Storage must already exist and match before wallet network access.
        let ledger =
            PaymentLedger::reopen(self.ledger_path(), AccountId(self.account().fingerprint()))?;
        let uncertainty = match &self {
            Self::Cln(v) => v.clock_uncertainty_ms,
            Self::Phoenixd { uncertainty, .. } => *uncertainty,
        };
        let payer = self.connect().await?;
        Ok((ledger, payer, uncertainty))
    }
}
/// Explicit account-ceiling extension after read-only native capability checks.
/// Never dispatches a payment, clears a reservation, or alters an accepted path.
pub async fn extend_foundation_payer_budget(
    path: &Path,
    expected: u64,
    new_limit: u64,
) -> Result<()> {
    let config = PayerConfig::load(path)?;
    let (mut ledger, _wallet, _) = config.open().await?;
    ledger.extend_account_lifetime_limit(expected, new_limit)?;
    Ok(())
}

/// Explicit concurrent-exposure extension within the existing total ceiling.
/// Preserves historical costs, uncertain reservations and accepted path limits.
pub async fn extend_foundation_payer_outstanding(
    path: &Path,
    expected: u64,
    new_limit: u64,
) -> Result<()> {
    let config = PayerConfig::load(path)?;
    let (mut ledger, _wallet, _) = config.open().await?;
    ledger.extend_account_outstanding_limit(expected, new_limit)?;
    Ok(())
}

/// Explicit exclusive creation after read-only native account/capability checks.
/// Never called by runtime startup and never replaces or migrates existing state.
pub async fn provision_foundation_payer(path: &Path) -> Result<()> {
    let config = PayerConfig::load(path)?;
    match std::fs::symlink_metadata(config.ledger_path()) {
        Ok(_) => {
            return Err(
                io::Error::new(io::ErrorKind::AlreadyExists, "Payer ledger already exists").into(),
            )
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let payer = config.connect().await?;
    PaymentLedger::provision(
        config.ledger_path(),
        AccountId(payer.account().fingerprint()),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Files(PathBuf);
    impl Files {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "eltor-payer-fixture-{}",
                hex::encode(rand::random::<[u8; 16]>())
            ));
            std::fs::create_dir(&p).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::write(p.join("password"), b"public-fixture-password\n").unwrap();
            std::fs::set_permissions(p.join("password"), std::fs::Permissions::from_mode(0o600))
                .unwrap();
            Self(p)
        }
        fn value(&self) -> serde_json::Value {
            serde_json::json!({"wallet":"phoenixd","ledger_path":"payer.sqlite","node_id":"0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798","chain":"regtest","phoenixd_url":"http://127.0.0.1:1/","phoenixd_password_file":"password","clock_uncertainty_ms":1000})
        }
        fn save(&self, v: &serde_json::Value) -> PathBuf {
            let p = self.0.join("account.json");
            std::fs::write(&p, serde_json::to_vec(v).unwrap()).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
            p
        }
    }
    impl Drop for Files {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn explicit_selection_and_private_credentials_never_guess_or_fall_back() {
        let f = Files::new();
        let v = f.value();
        let cfg = PayerConfig::load(&f.save(&v)).unwrap();
        assert_eq!(cfg.ledger_path(), f.0.join("payer.sqlite"));
        assert!(matches!(cfg, PayerConfig::Phoenixd { .. }));
        for (key, value) in [
            ("wallet", serde_json::json!("unknown")),
            ("clock_uncertainty_ms", serde_json::json!(5001)),
            ("extra", serde_json::json!(true)),
        ] {
            let mut bad = v.clone();
            bad[key] = value;
            assert!(PayerConfig::load(&f.save(&bad)).is_err());
        }
        let mut missing = v.clone();
        missing.as_object_mut().unwrap().remove("wallet");
        assert!(PayerConfig::load(&f.save(&missing)).is_err());
        std::fs::set_permissions(f.0.join("password"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(PayerConfig::load(&f.save(&v)).is_err());
        std::fs::remove_file(f.0.join("password")).unwrap();
        std::os::unix::fs::symlink("account.json", f.0.join("password")).unwrap();
        assert!(PayerConfig::load(&f.save(&v)).is_err());
    }
    #[tokio::test]
    async fn missing_ledger_refuses_startup_before_wallet_and_never_provisions() {
        let f = Files::new();
        let cfg = PayerConfig::load(&f.save(&f.value())).unwrap();
        assert!(cfg.open().await.is_err());
        assert!(!f.0.join("payer.sqlite").exists());
    }
}
