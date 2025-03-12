use serde::Serialize;


#[derive(Debug, Clone, Serialize)]
pub struct Relay {
    pub nickname: String,
    pub fingerprint: String,
    pub contact: Option<String>,
    pub bandwidth: Option<u32>,
    pub ip: Option<String>,
    pub port: Option<u16>,
    pub payment_bolt12_offer: Option<String>,
    pub payment_bip353: Option<String>,
    pub payment_bolt11_lnurl: Option<String>,
    pub payment_bolt11_lightning_address: Option<String>,
    pub payment_rate_msats: Option<u32>,
    pub payment_interval_seconds: Option<u32>, 
    pub payment_interval_rounds: Option<u32>,
    pub payment_handshake_fee: Option<u32>,
    pub payment_handshake_fee_payhash: Option<String>,
    pub payment_handshake_fee_preimage: Option<String>,
    pub payment_id_hashes_10: Option<Vec<String>>,
    pub relay_tag: Option<RelayTag>,
    pub hop: Option<i64>,
}


#[derive(Debug, Clone)]
pub struct RpcConfig {
    pub addr: String,
    pub rpc_password: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum RelayTag {
    Guard,
    Exit,
    Authority,
    Fast,
    HSDir,
    Running,
    Stable,
    V2Dir,
    Valid,
    Middle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsensusRelay {
    pub nickname: String,
    pub fingerprint: String,
    pub contact: Option<String>,
    pub bandwidth: Option<u32>,
    pub ip: Option<String>,
    pub port: Option<u16>,
    pub tags: Vec<RelayTag>,
    pub policy: Option<String>,
}