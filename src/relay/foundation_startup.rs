//! Explicit local foundation configuration. Never infer a new account or create
//! a replacement ledger when state is missing. Configuration contains credentials
//! and intentionally has no Debug implementation.
use crate::payment_ledger::{AccountId, ClockObservation, PaymentLedger};
use lni::cln::{
    foundation::{Chain, ClnFoundation, NativeAccount},
    ClnConfig,
};
use serde::Deserialize;
use std::{
    convert::TryInto,
    io::{self, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
const MAX_CONFIG: usize = 8192;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    ledger_path: PathBuf,
    node_id: String,
    chain: String,
    cln_url: String,
    cln_rune: String,
    clock_uncertainty_ms: u64,
}
pub(crate) struct FoundationConfig {
    ledger_path: PathBuf,
    account: NativeAccount,
    wallet: ClnConfig,
    pub clock_uncertainty_ms: u64,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid foundation configuration",
    )
}
impl FoundationConfig {
    pub fn load(path: &Path) -> io::Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_CONFIG as u64 {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(invalid());
            }
        }
        let mut bytes = Vec::new();
        file.take((MAX_CONFIG + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_CONFIG {
            return Err(invalid());
        }
        Self::parse(&bytes, path.parent().unwrap_or(Path::new(".")))
    }
    fn parse(bytes: &[u8], base: &Path) -> io::Result<Self> {
        let value: FileConfig = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        if value.ledger_path.as_os_str().is_empty()
            || value.node_id.len() != 66
            || value.clock_uncertainty_ms > 5000
            || value.cln_url.is_empty()
            || value.cln_url.len() > 2048
            || value.cln_rune.is_empty()
            || value.cln_rune.len() > 4096
            || !value
                .node_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        let node_id = hex::decode(value.node_id)
            .map_err(|_| invalid())?
            .try_into()
            .map_err(|_| invalid())?;
        let chain = match value.chain.as_str() {
            "bitcoin" => Chain::Bitcoin,
            "testnet" => Chain::Testnet,
            "regtest" => Chain::Regtest,
            "signet" => Chain::Signet,
            _ => return Err(invalid()),
        };
        let ledger_path = if value.ledger_path.is_absolute() {
            value.ledger_path
        } else {
            base.join(value.ledger_path)
        };
        Ok(Self {
            ledger_path,
            account: NativeAccount { node_id, chain },
            wallet: ClnConfig {
                url: value.cln_url,
                rune: value.cln_rune,
                ..Default::default()
            },
            clock_uncertainty_ms: value.clock_uncertainty_ms,
        })
    }
    // Operator-only access preserves a single bounded private config parser.
    // Runtime startup still calls open(), which never creates or migrates.
    pub(super) fn ledger_path(&self) -> &Path {
        &self.ledger_path
    }
    pub(super) fn account(&self) -> NativeAccount {
        self.account
    }
    pub(super) async fn verify_recipient(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wallet = ClnFoundation::connect_receiving(self.wallet.clone(), self.account).await?;
        // Help advertises API support; a bounded read also checks this rune's
        // actual receiving permission. This probe never advances a ledger cursor.
        wallet.incoming_updates(1).await?;
        Ok(())
    }
    /// Opening precedes wallet access. A missing ledger never causes provisioning
    /// and credentials never appear in returned diagnostics from this module.
    pub async fn open(
        self,
    ) -> Result<(PaymentLedger, ClnFoundation, u64), Box<dyn std::error::Error>> {
        let ledger =
            PaymentLedger::reopen(&self.ledger_path, AccountId(self.account.fingerprint()))?;
        let wallet = ClnFoundation::connect_receiving(self.wallet, self.account).await?;
        Ok((ledger, wallet, self.clock_uncertainty_ms))
    }
}
/// The operator supplies a conservative UTC error bound. Observation brackets
/// additionally reject rollback/discontinuity against monotonic elapsed time.
pub(crate) fn observe_clock(
    uncertainty_ms: u64,
) -> Result<ClockObservation, Box<dyn std::error::Error>> {
    if uncertainty_ms > 5000 {
        return Err(invalid().into());
    }
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()
        .map_err(|_| invalid())?;
    Ok(ClockObservation {
        now_ms,
        uncertainty_ms,
    })
}
/// Returns false only after authenticated confirmation that foundation mode is
/// disabled. A failed/unknown query or incomplete setup never enables legacy
/// payment handling on a potentially foundation-enabled relay.
pub(crate) async fn run_if_enabled(rpc: &crate::types::RpcConfig) -> bool {
    match run(rpc).await {
        Ok(enabled) => enabled,
        Err(_) => {
            log::error!("Foundation relay startup stopped; verify mode, existing ledger, wallet identity and private configuration");
            true
        }
    }
}
async fn run(rpc: &crate::types::RpcConfig) -> Result<bool, Box<dyn std::error::Error>> {
    run_with_path(
        rpc,
        std::env::var_os("ELTOR_FOUNDATION_CONFIG").map(PathBuf::from),
    )
    .await
}
async fn run_with_path(
    rpc: &crate::types::RpcConfig,
    configured: Option<PathBuf>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut control = crate::rpc::PaymentControl::connect(rpc).await?;
    let mode = control.command("GETCONF ElTorFoundation").await?;
    match mode.as_str() {
        "250 ElTorFoundation=0" if configured.is_none() => return Ok(false),
        "250 ElTorFoundation=1" => (),
        _ => return Err(invalid().into()),
    }
    let path = configured.ok_or_else(invalid)?;
    let config = FoundationConfig::load(Path::new(&path))?;
    let (ledger, wallet, uncertainty) = config.open().await?;
    let mut coordinator = super::RelayFoundation::new(ledger, wallet, control)?;
    let mut backoff = 1u64;
    loop {
        // Drop the non-Send diagnostic before sleeping/reconnecting. Never log
        // provider response bodies, account identifiers, paths or credentials.
        let failed = coordinator
            .step(|| observe_clock(uncertainty))
            .await
            .is_err();
        if failed {
            if backoff == 1 {
                log::warn!(
                    "Foundation relay requires authoritative state recovery; no legacy fallback"
                );
            }
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(30);
                if let Ok(mut control) = crate::rpc::PaymentControl::connect(rpc).await {
                    if control
                        .command("GETCONF ElTorFoundation")
                        .await
                        .ok()
                        .as_deref()
                        == Some("250 ElTorFoundation=1")
                    {
                        coordinator.reconnect(control);
                        break;
                    }
                }
            }
        } else {
            backoff = 1;
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> serde_json::Value {
        serde_json::json!({"ledger_path":"account.sqlite","node_id":"0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798","chain":"regtest","cln_url":"http://127.0.0.1:1","cln_rune":"fixture","clock_uncertainty_ms":1000})
    }
    #[tokio::test]
    async fn authenticated_mode_requires_explicit_config_and_never_guesses_disabled() {
        use tokio::{
            io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
            net::TcpListener,
        };
        for mode in ["0", "1", "2"] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let rpc = crate::types::RpcConfig {
                addr: listener.local_addr().unwrap().to_string(),
                rpc_password: None,
                command: String::new(),
            };
            let server = tokio::spawn(async move {
                let mut socket = BufReader::new(listener.accept().await.unwrap().0);
                for expected in [
                    "AUTHENTICATE\r\n",
                    "SETEVENTS ELTOR_PAYMENT_STATE\r\n",
                    "GETCONF ElTorFoundation\r\n",
                ] {
                    let mut line = String::new();
                    socket.read_line(&mut line).await.unwrap();
                    assert_eq!(line, expected);
                    let reply = if expected.starts_with("GETCONF") {
                        format!("250 ElTorFoundation={}\r\n", mode)
                    } else {
                        "250 OK\r\n".into()
                    };
                    socket.get_mut().write_all(reply.as_bytes()).await.unwrap();
                }
            });
            let result = run_with_path(&rpc, None).await;
            if mode == "0" {
                assert!(!result.unwrap())
            } else {
                assert!(result.is_err())
            }
            server.await.unwrap();
        }
    }
    #[test]
    fn config_pins_account_and_rejects_unknown_or_unsafe_clock_fields() {
        let base = Path::new("runtime");
        let value = input();
        let cfg = FoundationConfig::parse(&serde_json::to_vec(&value).unwrap(), base).unwrap();
        assert_eq!(cfg.ledger_path, base.join("account.sqlite"));
        assert!(cfg.account.chain == Chain::Regtest);
        for (key, value) in [
            ("clock_uncertainty_ms", serde_json::json!(5001)),
            ("chain", serde_json::json!("unknown")),
            ("extra", serde_json::json!(true)),
        ] {
            let mut bad = input();
            bad[key] = value;
            assert!(FoundationConfig::parse(&serde_json::to_vec(&bad).unwrap(), base).is_err());
        }
        let mut missing = input();
        missing
            .as_object_mut()
            .unwrap()
            .remove("clock_uncertainty_ms");
        assert!(FoundationConfig::parse(&serde_json::to_vec(&missing).unwrap(), base).is_err());
    }
    #[tokio::test]
    async fn missing_ledger_never_provisions_or_contacts_wallet() {
        let base = std::env::temp_dir().join(format!("eltor-config-{}", rand::random::<u128>()));
        std::fs::create_dir(&base).unwrap();
        let cfg = FoundationConfig::parse(&serde_json::to_vec(&input()).unwrap(), &base).unwrap();
        assert!(cfg.open().await.is_err());
        assert!(!base.join("account.sqlite").exists());
        std::fs::remove_dir(base).unwrap();
    }
}
