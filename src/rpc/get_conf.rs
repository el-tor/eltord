use super::rpc_client;
use crate::types::RpcConfig;
use std::error::Error;

#[derive(Debug, Clone)]
pub struct Conf {}

pub async fn get_conf(config: &RpcConfig, setting: String) -> Result<String, Box<dyn Error>> {
    let rpc = rpc_client(RpcConfig {
        addr: config.clone().addr,
        rpc_password: config.clone().rpc_password,
        command: format!("GETCONF {}", setting).into(),
    })
    .await?;

    if rpc.starts_with("250 ") {
        let resp = rpc.trim_start_matches("250 ");
        Ok(resp.to_string())
    } else {
        Ok("".to_string())
    }
}

pub async fn get_conf_payment_circuit_max_fee(config: &RpcConfig) -> Result<u64, Box<dyn Error>> {
    let conf = get_conf(&config, "PaymentCircuitMaxFee".to_string())
        .await
        .unwrap();
    if conf.is_empty() {
        return Ok(12000);
    }
    let parts: Vec<&str> = conf.split('=').collect();
    // println!("Debug: conf = {}", conf);
    // println!("Debug: parts = {:?}", parts);
    if parts.len() == 2 {
        if let Ok(value) = parts[1].trim().parse::<u64>() {
            return Ok(value);
        }
    }
    Ok(12000)
}
