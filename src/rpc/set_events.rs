use super::rpc_client;
use super::rpc_event_listener;
use crate::types::RpcConfig;
use std::error::Error;

pub async fn event_payment_received(config: &RpcConfig) -> Result<Option<String>, Box<dyn Error>> {
    let event = "PAYMENT_ID_HASH_RECEIVED"; // "CIRC NOTICE";
    let rpc = rpc_event_listener(config.clone(), event.to_string()).await?;
    // TODO how to handle 650 payment received event?
    // dbg!(rpc.clone());
    // if rpc.starts_with("650 ") {
    //     let payment_id_hash = rpc.trim_start_matches("650 EVENT_PAYMENT_ID_HASH_RECEIVED ");
    //     Ok(Some(payment_id_hash.to_string()))
    // } else {
    //     Ok(None)
    // }
    Ok(None)
}
