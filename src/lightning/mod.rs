use lni::phoenixd::{PhoenixdConfig, PhoenixdNode};

pub async fn get_lightning_node_info() {
    let url = env::var("PHOENIXD_URL").unwrap();
    let password = env::var("PHOENIXD_PASSWORD").unwrap();
    let node = PhoenixdNode::new(PhoenixdConfig { url, password });
    let info = node.get_info().await.unwrap();
    println!("Node info: {:?}", info)
}
