use super::{rpc_client, RpcConfig};
use std::error::Error;

#[derive(Debug, Clone)]
pub struct Relay {
    pub nickname: String,
    pub fingerprint: String,
    pub contact: Option<String>,
    pub bandwidth: Option<u32>,
    pub ip: Option<String>,
    pub port: Option<u16>,
    pub payment_bolt12_offer: Option<String>,
    pub payment_bip353: Option<String>,
    pub payment_bolt11_lnurl: Option<String>,
    pub payment_bolt11_lightning_address: Option<String>,
    pub payment_rate_msats: Option<u32>,
    pub payment_interval: Option<u32>,
    pub payment_interval_rounds: Option<u32>,
    pub payment_handshake_fee: Option<u32>,
}

pub async fn get_relay_descriptors(config: &RpcConfig) -> Result<Vec<Relay>, Box<dyn Error>> {
    let rpc = rpc_client(RpcConfig {
        addr: config.clone().addr,
        rpc_password: config.clone().rpc_password,
        command: "GETINFO desc/all-recent".into(),
    })
    .await
    .unwrap();

    let mut relays = Vec::new();
    let mut current_relay: Option<Relay> = None;

    for line in rpc.lines() {
        if line.starts_with("router ") {
            // Store the previous relay if it exists
            if let Some(relay) = current_relay.take() {
                relays.push(relay);
            }

            // Parse 'router' line: router <nickname> <address> <orport> <socksport> <dirport>
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Ok(port) = parts[3].parse::<u16>() {
                current_relay = Some(Relay {
                    nickname: parts[1].to_string(),
                    fingerprint: String::new(),
                    contact: None,
                    bandwidth: None,
                    payment_bolt12_offer: None,
                    ip: Some(parts[2].to_string()),
                    port: Some(port),
                    payment_bip353: None,
                    payment_bolt11_lnurl: None,
                    payment_bolt11_lightning_address: None,
                    payment_rate_msats: None,
                    payment_interval: None,
                    payment_interval_rounds: None,
                    payment_handshake_fee: None,
                });
            }
        } else if line.starts_with("fingerprint ") {
            if let Some(relay) = &mut current_relay {
                relay.fingerprint = line["fingerprint ".len()..].to_string().replace(" ", "");
            }
        } else if line.starts_with("contact ") {
            if let Some(relay) = &mut current_relay {
                relay.contact = Some(line["contact ".len()..].to_string());
            }
        } else if line.starts_with("bandwidth ") {
            if let Some(relay) = &mut current_relay {
                let parts: Vec<&str> = line["bandwidth ".len()..].split_whitespace().collect();
                if let Ok(bw) = parts.get(2).unwrap_or(&"0").parse::<u32>() {
                    relay.bandwidth = Some(bw);
                }
            }
        } else if line.starts_with("PaymentBolt12Offer ") {
            if let Some(relay) = &mut current_relay {
                relay.payment_bolt12_offer = Some(line["PaymentBolt12Offer ".len()..].to_string());
            }
        } else if line.starts_with("PaymentBip353 ") {
            if let Some(relay) = &mut current_relay {
                relay.payment_bip353 = Some(line["PaymentBip353 ".len()..].to_string());
            }
        } else if line.starts_with("PaymentBolt11Lnurl ") {
            if let Some(relay) = &mut current_relay {
                relay.payment_bolt11_lnurl = Some(line["PaymentBolt11Lnurl ".len()..].to_string());
            }
        } else if line.starts_with("PaymentBolt11LightningAddress ") {
            if let Some(relay) = &mut current_relay {
                relay.payment_bolt11_lightning_address =
                    Some(line["PaymentBolt11LightningAddress ".len()..].to_string());
            }
        } else if line.starts_with("PaymentRateMsats ") {
            if let Some(relay) = &mut current_relay {
                if let Ok(rate) = line["PaymentRateMsats ".len()..].parse::<u32>() {
                    relay.payment_rate_msats = Some(rate);
                }
            }
        } else if line.starts_with("PaymentInterval ") {
            if let Some(relay) = &mut current_relay {
                if let Ok(rate) = line["PaymentInterval ".len()..].parse::<u32>() {
                    relay.payment_interval = Some(rate);
                }
            }
        } else if line.starts_with("PaymentInvervalRounds ") {
            if let Some(relay) = &mut current_relay {
                if let Ok(rate) = line["PaymentInvervalRounds ".len()..].parse::<u32>() {
                    relay.payment_interval_rounds = Some(rate);
                }
            }
        } else if line.starts_with("PaymentHandshakeFee ") {
            if let Some(relay) = &mut current_relay {
                if let Ok(rate) = line["PaymentHandshakeFee ".len()..].parse::<u32>() {
                    relay.payment_handshake_fee = Some(rate);
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
