use std::error::Error;

use super::{rpc_client, RpcConfig};

#[derive(Debug)]
pub struct Relay {
    nickname: String,
    fingerprint: String,
    contact: Option<String>,
    bandwidth: Option<u32>,
}

pub async fn get_relay_descriptors(config: RpcConfig) -> Result<Vec<Relay>, Box<dyn Error>> {
    let rpc = rpc_client(RpcConfig {
        addr: config.addr,
        rpc_password: config.rpc_password,
        command: "GETINFO ns/all".into(),
    })
    .await
    .unwrap();
    let response = rpc.as_str();

    let mut relays = Vec::new();
    let mut current_relay: Option<Relay> = None;

    for line in response.lines() {
        if line.starts_with("r ") {
            // Store the previous relay if it exists
            if let Some(relay) = current_relay.take() {
                relays.push(relay);
            }

            // Parse 'r' line: r <nickname> <fingerprint> ...
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                current_relay = Some(Relay {
                    nickname: parts[1].to_string(),
                    fingerprint: parts[2].to_string(),
                    contact: None,
                    bandwidth: None,
                });
            }
        } else if line.starts_with("contact ") {
            if let Some(relay) = &mut current_relay {
                relay.contact = Some(line["contact ".len()..].to_string());
            }
        } else if line.starts_with("w Bandwidth=") {
            if let Some(relay) = &mut current_relay {
                if let Ok(bw) = line["w Bandwidth=".len()..].parse::<u32>() {
                    relay.bandwidth = Some(bw);
                }
            }
        }
    }

    // Store the last relay (if any)
    if let Some(relay) = current_relay {
        relays.push(relay);
    }

    Ok(relays)
}
