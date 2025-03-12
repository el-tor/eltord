use super::rpc_client;
use std::error::Error;
use crate::types::RpcConfig;

// EXTENDPAIDCIRCUIT 0
// fingerprint_entry_guard handshake_fee_payment_hash+handshake_fee_preimage+10_payment_ids_concatinated
// fingerprint_middle_relay handshake_fee_payment_hash+handshake_fee_preimage+10_payment_ids_concatinated
// fingerprint_exit_relay handshake_fee_payment_hash+handshake_fee_preimage+10_payment_ids_concatinated
// TODO make work with N middle_relays
pub async fn extend_paid_circuit(config: &RpcConfig, command: String) -> Result<String, Box<dyn Error>> {
    let rpc = rpc_client(RpcConfig {
        addr: config.clone().addr,
        rpc_password: config.clone().rpc_password,
        command: format!("{}", command).into(),
    })
    .await;

    match rpc {
        Ok(rpc) => {
            if rpc.starts_with("250 ") {
                let resp = rpc.trim_start_matches("250 ");
                let start = resp.find("EXTENDED").map(|i| i + "EXTENDED".len()).unwrap_or(0);
                let end = resp.find("250 closing connection").unwrap_or(resp.len());
                let circuit_id = &resp[start..end].trim();
                Ok(circuit_id.to_string())
            } else {
                Ok("".to_string())
            }
        }
        Err(e) => {
            Err(e)
        }
    }
}
