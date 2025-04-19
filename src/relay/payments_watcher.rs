use crate::{rpc::event_payment_received, types::RpcConfig};

pub async fn start_payments_watcher(config: &RpcConfig) {
    event_payment_received(config).await;

}