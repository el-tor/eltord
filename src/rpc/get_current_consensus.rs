use super::{rpc_client, RpcConfig, microdesc_to_fingerprint};
use std::error::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum RelayTag {
    Guard,
    Middle,
    Exit,
    Authority,
    Fast,
    HSDir,
    Running,
    Stable,
    V2Dir,
    Valid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsensusRelay {
    pub nickname: String,
    pub fingerprint: String,
    pub contact: Option<String>,
    pub bandwidth: Option<u32>,
    pub ip: Option<String>,
    pub port: Option<u16>,
    pub tags: Vec<RelayTag>,
    pub policy: Option<String>,
}

pub async fn get_current_consensus(
    config: &RpcConfig,
) -> Result<Vec<ConsensusRelay>, Box<dyn Error>> {
    let rpc = rpc_client(RpcConfig {
        addr: config.clone().addr,
        rpc_password: config.clone().rpc_password,
        command: "GETINFO ns/all".into(),
    })
    .await?;

    let mut relays = Vec::new();
    let mut current_relay: Option<ConsensusRelay> = None;

    for line in rpc.lines() {
        if line.starts_with("r ") {
            // Store the previous relay if it exists
            if let Some(relay) = current_relay.take() {
                relays.push(relay);
            }

            // Parse 'r' line: r <nickname> <fingerprint> <digest> <publication time> <ip> <orport> <dirport>
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 9 {
                let fp = parts[2].to_string();
                let fp: &str = fp.as_str();
                // TODO this might be slow if it has to parse thousands of descriptors. Maybe in the future just compute after
                // the 3 relay are selected in the simple_relay_selection_algo
                let fingerprint = microdesc_to_fingerprint(fp).unwrap();
                current_relay = Some(ConsensusRelay {
                    nickname: parts[1].to_string(),
                    fingerprint,
                    contact: None,
                    bandwidth: None,
                    ip: Some(parts[5].to_string()),
                    port: parts[6].parse().ok(),
                    tags: Vec::new(),
                    policy: None,
                });
            }
        } else if line.starts_with("s ") {
            if let Some(relay) = &mut current_relay {
                relay.tags = line[2..]
                    .split_whitespace()
                    .filter_map(|tag| match tag {
                        "Guard" => Some(RelayTag::Guard),
                        "Middle" => Some(RelayTag::Middle),
                        "Exit" => Some(RelayTag::Exit),
                        "Authority" => Some(RelayTag::Authority),
                        "Fast" => Some(RelayTag::Fast),
                        "HSDir" => Some(RelayTag::HSDir),
                        "Running" => Some(RelayTag::Running),
                        "Stable" => Some(RelayTag::Stable),
                        "V2Dir" => Some(RelayTag::V2Dir),
                        "Valid" => Some(RelayTag::Valid),
                        _ => None,
                    })
                    .collect();
            }
        } else if line.starts_with("w Bandwidth=") {
            if let Some(relay) = &mut current_relay {
                if let Ok(bw) = line[11..].parse::<u32>() {
                    relay.bandwidth = Some(bw);
                }
            }
        } else if line.starts_with("p ") {
            if let Some(relay) = &mut current_relay {
                relay.policy = Some(line[2..].to_string());
            }
        }
    }

    // Store the last relay (if any)
    if let Some(relay) = current_relay {
        relays.push(relay);
    }

    Ok(relays)
}

//// Sample Consensus Document

// 250+ns/all=
// r test004r MJyJq8PncKpIN+vpLoZmrnEAZDE n3kz1aHz554Qt4LfC0Bh21xKv+M 2038-01-01 00:00:00 127.0.0.14 5059 0
// s Exit Fast Guard HSDir Running Stable V2Dir Valid
// w Bandwidth=27
// p accept 1-65535
// r test001a RGKaO53hhKag26Cg3lSRbSQzmys GocGIqbue40or3ZkYx11383Ku+k 2038-01-01 00:00:00 127.0.0.11 5056 7056
// s Authority Exit Fast Guard HSDir Running Stable V2Dir Valid
// w Bandwidth=29
// p reject 1-65535
// r test007m RkL4prCV71lrP3RoiJUylBz38SE mJdgojD5kF9zOzrXJn7tz/aYBfQ 2038-01-01 00:00:00 127.0.0.17 5062 0
// s Exit Fast Guard HSDir Running Stable V2Dir Valid
// w Bandwidth=28
// p accept 1-65535
// r test006m RwhwSGoa1MrPAoH/jPyzHSS7EXA 3wM+lPFsqDeTnzAeqCm+1cJ4ysw 2038-01-01 00:00:00 127.0.0.16 5061 0
// s Exit Fast Guard HSDir Running Stable V2Dir Valid
// w Bandwidth=17
// p accept 1-65535
// r test002a UqT+qd9hzrpYyL9fH2Uacy7+qxQ n2gEZ2E+x/H5tA+gTeq1uq7S0kY 2038-01-01 00:00:00 127.0.0.12 5057 7057
// s Authority Exit Fast Guard HSDir Running Stable V2Dir Valid
// w Bandwidth=42
// p reject 1-65535
// r test003a ZTI7Ag8MrNEDBs3XbHvR+Ux5jo4 MVy4Eji3K1V61meE29n2E1HXM8w 2038-01-01 00:00:00 127.0.0.13 5058 7058
// s Authority Exit Fast Guard HSDir Running Stable V2Dir Valid
// w Bandwidth=53
// p reject 1-65535
// r test008m ltyfn6sTYUr21EUbh765VGqeuKM IZe0RVOJ9Xy4+IIp5UO+Bwv2XCQ 2038-01-01 00:00:00 127.0.0.18 5063 0
// s Exit Fast Guard HSDir Running Stable V2Dir Valid
// w Bandwidth=18
// p accept 1-65535
// r test000a p2SdibSO7l+xxFg8c/jaaIihnBI BLmZXd+iJ8yKcyyFBOy7KiFYYuI 2038-01-01 00:00:00 127.0.0.10 5055 7055
// s Authority Exit Fast Guard HSDir Running Stable V2Dir Valid
// w Bandwidth=32
// p reject 1-65535
// .
// 250 OK
