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
            let response = super::command_response(&rpc)?;
            let circuit_id = response.strip_prefix("250 EXTENDED ")
                .ok_or("Tor did not return an EXTENDED circuit ID")?.trim();
            let id: u32 = circuit_id.parse()?;
            if id == 0 { return Err("Tor returned circuit ID zero".into()); }
            Ok(id.to_string())
        }
        Err(e) => {
            Err(e)
        }
    }
}
