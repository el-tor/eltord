use super::rpc_client;
use crate::types::{Relay, RpcConfig};
use std::error::Error;

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

    // TODO: fix crash if relay has not descriptors
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
                    payment_interval_seconds: None,
                    payment_interval_rounds: None,
                    payment_handshake_fee: None,
                    payment_id_hashes_10: None,
                    payment_handshake_fee_payhash: None,
                    payment_handshake_fee_preimage: None,
                    relay_tag: None,
                    hop: None,
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
                    relay.payment_interval_seconds = Some(rate);
                }
            }
        } else if line.starts_with("PaymentInvervalRounds ") {
            // TODO Not being used, need to think more about this, hardcode to 10 now so we can pass in 10 payment id hashed during circuit build
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
