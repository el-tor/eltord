use std::{env, string};

use lni::cln::{ClnConfig, ClnNode};
use lni::lnd::{LndConfig, LndNode};
use lni::phoenixd::{PhoenixdConfig, PhoenixdNode};
use lni::LightningNode;

use crate::rpc::get_conf;
use crate::types::RpcConfig;

pub async fn load_wallet(rpc_config: &RpcConfig) -> Box<dyn LightningNode + Send + Sync> {
    println!("Loading wallet...");
    let node_torrc_config = lookup_default_lightning_node_from_torrc(&rpc_config).await;
    let lightning_node = tokio::task::block_in_place(|| get_lightning_node(node_torrc_config)); // TODO research more into tokio block in place
    lightning_node
}

pub async fn lookup_default_lightning_node_from_torrc(rpc_config: &RpcConfig) -> (String, String) {
    let lightning_conf_str = get_conf(rpc_config, "PaymentLightningNodeConfig".to_string())
        .await
        .unwrap();
    // parse the string "PaymentLightningNodeConfig type=phoenixd url=http://url.com password=pass1234 default=true"
    // TODO handle mutliple configs for PaymentLightningNodeConfig and choose default
    let node_type = get_default_value(lightning_conf_str.clone(), "type".to_string());
    (node_type.unwrap().to_string(), lightning_conf_str)
}

pub fn get_lightning_node(
    (node_type, lightning_conf_str): (String, String),
) -> Box<dyn LightningNode + Send + Sync> {
    let node_type_str = node_type.as_str();
    match node_type_str {
        "phoenixd" => {
            let url = get_default_value(lightning_conf_str.clone(), "url".to_string())
                .expect("url not found in torrc config");
            let password = get_default_value(lightning_conf_str.clone(), "password".to_string())
                .expect("password not found in torrc config");
            let config = PhoenixdConfig {
                url: url.clone(),
                password: password.clone(),
                ..Default::default()
            };
            let u = url.clone().as_str();
            let node: Box<dyn LightningNode + Send + Sync> = Box::new(PhoenixdNode::new(config));
            let info = node.get_info().unwrap();
            println!("Phoenixd Node info: {:?}", info);
            node
        }
        "lnd" => {
            let url = get_default_value(lightning_conf_str.clone(), "url".to_string())
                .expect("url not found in torrc config");
            let macaroon = get_default_value(lightning_conf_str.clone(), "macaroon".to_string())
                .expect("macaroon not found in torrc config");
            let config = LndConfig {
                url: url.clone(),
                macaroon: macaroon.clone(),
                ..Default::default()
            };
            let node: Box<dyn LightningNode + Send + Sync> = Box::new(LndNode::new(config));
            let info = node.get_info().unwrap();
            println!("LND Node info: {:?}", info);
            node
        }
        "cln" => {
            let url = get_default_value(lightning_conf_str.clone(), "url".to_string())
                .expect("url not found in torrc config");
            let rune = get_default_value(lightning_conf_str.clone(), "rune".to_string())
                .expect("rune not found in torrc config");
            let config = ClnConfig {
                url: url.clone(),
                rune: rune.clone(),
                ..Default::default()
            };
            let node: Box<dyn LightningNode + Send + Sync> = Box::new(ClnNode::new(config));
            let info = node.get_info().unwrap();
            println!("CLN Node info: {:?}", info);
            node
        }
        _ => panic!("Unsupported node type: {}", node_type),
    }
}

fn get_default_value(lightning_conf_str: String, key: String) -> Option<String> {
    let config_array = lightning_conf_str.split("\r\n").collect::<Vec<&str>>();

    for config in config_array {
        if config.contains("default=true") {
            let binding =
                config.replace(&"PaymentLightningNodeConfig=".to_string(), &"".to_string());
            let parts: Vec<&str> = binding.split_whitespace().collect();
            dbg!(&parts);
            let mut val: Option<&str> = None;
            for part in parts {
                let formatted_key = format!("{}=", key);
                if part.contains(&formatted_key) {
                    val = Some(part.split("=").collect::<Vec<&str>>()[1]);
                    break;
                }
            }
            dbg!(&val);
            return Some(val.unwrap_or_default().to_string());
        }
    }
    None
}
