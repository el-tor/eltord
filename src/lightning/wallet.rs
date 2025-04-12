use std::env;

use lni::phoenixd::{PhoenixdConfig, PhoenixdNode};

pub async fn load_wallet() -> PhoenixdNode {
    println!("Loading wallet...");
    get_lightning_node_info().await
}

pub fn lookup_default_lightning_node_from_torrc(){
    // let torrc = std::fs::read_to_string("/etc/tor/torrc").unwrap();
    // let mut lines = torrc.lines();
    // let mut lightning_node = None;
    // for line in lines {
    //     if line.contains("HiddenServicePort 9735") {
    //         let parts: Vec<&str> = line.split_whitespace().collect();
    //         lightning_node = Some(parts[2].to_string());
    //         break;
    //     }
    // }
    // lightning_node
}

pub async fn get_lightning_node_info() -> PhoenixdNode {
    // TODO: read from torrc file
    let url = env::var("PHOENIXD_URL").unwrap();
    let password = env::var("PHOENIXD_PASSWORD").unwrap();
    let config = PhoenixdConfig {
        url: url.clone(),
        password: password.clone(),
        ..Default::default()
    };
    let node = PhoenixdNode::new(config);
    let info = node.get_info().unwrap();
    println!("Node info: {:?}", info);
    node
}
