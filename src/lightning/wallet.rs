use log::info;

use lni::cln::{ClnConfig, ClnNode};
use lni::lnd::{LndConfig, LndNode};
use lni::nwc::{NwcConfig, NwcNode};
use lni::phoenixd::{PhoenixdConfig, PhoenixdNode};
use lni::strike::{StrikeConfig, StrikeNode};
use lni::{LightningNode};

use crate::rpc::get_conf;
use crate::types::RpcConfig;

pub async fn load_wallet(
    rpc_config: &RpcConfig,
) -> Result<Box<dyn LightningNode + Send + Sync>, Box<dyn std::error::Error>> {
    info!("Loading wallet...");
    let node_torrc_config = lookup_default_lightning_node_from_torrc(&rpc_config).await?;
    let lightning_node = get_lightning_node(node_torrc_config).await?;
    Ok(lightning_node)
}

pub async fn lookup_default_lightning_node_from_torrc(
    rpc_config: &RpcConfig,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    info!(
        "Looking up default lightning node from torrc with config: {:?}",
        rpc_config
    );
    let lightning_conf_str = get_conf(rpc_config, "PaymentLightningNodeConfig".to_string())
        .await
        .map_err(|e| format!("Failed to get PaymentLightningNodeConfig from torrc: {}", e))?;
    info!("Lightning config string: {}", lightning_conf_str);
    // parse the string "PaymentLightningNodeConfig type=phoenixd url=http://url.com password=pass1234 default=true"
    // TODO handle mutliple configs for PaymentLightningNodeConfig and choose default
    let node_type = get_default_value(lightning_conf_str.clone(), "type".to_string())
        .ok_or("No 'type' found in PaymentLightningNodeConfig")?;
    Ok((node_type.to_string(), lightning_conf_str))
}

pub async fn get_lightning_node(
    (node_type, lightning_conf_str): (String, String),
) -> Result<Box<dyn LightningNode + Send + Sync>, Box<dyn std::error::Error>> {
    let node_type_str = node_type.as_str();
    match node_type_str {
        "phoenixd" => {
            let url = get_default_value(lightning_conf_str.clone(), "url".to_string())
                .ok_or("url not found in torrc config")?;
            let password = get_default_value(lightning_conf_str.clone(), "password".to_string())
                .ok_or("password not found in torrc config")?;
            let config = PhoenixdConfig {
                url: url.clone(),
                password: password.clone(),
                ..Default::default()
            };
            let node: Box<dyn LightningNode + Send + Sync> = Box::new(PhoenixdNode::new(config));
            let info = node.get_info().await?;
            info!("Phoenixd Node info: {:?}", info);
            Ok(node)
        }
        "lnd" => {
            let url = get_default_value(lightning_conf_str.clone(), "url".to_string())
                .ok_or("url not found in torrc config")?;
            let macaroon = get_default_value(lightning_conf_str.clone(), "macaroon".to_string())
                .ok_or("macaroon not found in torrc config")?;
            let config = LndConfig {
                url: url.clone(),
                macaroon: macaroon.clone(),
                ..Default::default()
            };
            let node: Box<dyn LightningNode + Send + Sync> = Box::new(LndNode::new(config));
            let info = node.get_info().await?;
            info!("LND Node info: {:?}", info);
            Ok(node)
        }
        "cln" => {
            let url = get_default_value(lightning_conf_str.clone(), "url".to_string())
                .ok_or("url not found in torrc config")?;
            let rune = get_default_value(lightning_conf_str.clone(), "rune".to_string())
                .ok_or("rune not found in torrc config")?;
            let config = ClnConfig {
                url: url.clone(),
                rune: rune.clone(),
                ..Default::default()
            };
            let node: Box<dyn LightningNode + Send + Sync> = Box::new(ClnNode::new(config));
            let info = node.get_info().await?;
            info!("CLN Node info: {:?}", info);
            Ok(node)
        }
        "nwc" => {
            // PaymentLightningNodeConfig type=nwc uri=nostr+walletconnect://pubkey?relay=...&secret=... default=true
            let uri = get_default_value(lightning_conf_str.clone(), "uri".to_string())
                .ok_or("uri not found in torrc config")?;
            let config = NwcConfig {
                nwc_uri: uri.clone(),
                ..Default::default()
            };
            let node: Box<dyn LightningNode + Send + Sync> = Box::new(NwcNode::new(config));
            let info = node.get_info().await?;
            info!("NWC Node info: {:?}", info);
            Ok(node)
        }
        "strike" => {
            // PaymentLightningNodeConfig type=strike apiKey=1234abc
            let url = get_default_value(lightning_conf_str.clone(), "url".to_string())
                .unwrap_or_else(|| "https://api.strike.me/v1".to_string());
            let api_key = get_default_value(lightning_conf_str.clone(), "apiKey".to_string())
                .ok_or("apiKey not found in torrc config")?;
            let config = StrikeConfig {
                base_url: Some(url.clone()),
                api_key: api_key.clone(),
                 ..Default::default()
            };
            let node: Box<dyn LightningNode + Send + Sync> = Box::new(StrikeNode::new(config));
            let info = node.get_info().await?;
            info!("Strike Node info: {:?}", info);
            Ok(node)
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
            info!("Config parts: {:?}", parts);
            let mut val: Option<&str> = None;
            for part in parts {
                let formatted_key = format!("{}=", key);
                if part.contains(&formatted_key) {
                    // For URI values, we need to get everything after the first '='
                    // not just split on '=' and take [1]
                    if let Some(eq_idx) = part.find('=') {
                        val = Some(&part[eq_idx + 1..]);
                        break;
                    }
                }
            }
            info!("Extracted value: {:?}", val);
            return Some(val.unwrap_or_default().to_string());
        }
    }
    None
}
