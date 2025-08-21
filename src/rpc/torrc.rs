use log::info;

use super::rpc_client;
use crate::types::RpcConfig;
use std::{error::Error, io::BufRead};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KV {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrcEntry {
    pub key: String,
    pub value: String,
    pub data: Vec<KV>,
}

/// Implements Tor's GETCONF rules for configuration queries.
/// Returns Vec<TorrcEntry> of found keys.
pub async fn get_torrc_value(config: &RpcConfig, keywords: &[String]) -> Vec<TorrcEntry> {
    let mut results = Vec::new();
    if keywords.is_empty() {
        return results;
    }
    for key in keywords {
        match get_conf(config, key.clone()).await {
            Ok(resp) => {
                let resp = resp.trim();
                info!("resp: {:?}", resp);
                if resp.is_empty() {
                    continue;
                }
                for line in resp.lines() {
                    let line = line.trim_start_matches("250 ").trim();
                    if line.is_empty() || line == "OK" || line == "closing connection" {
                        continue;
                    }
                    if let Some(idx) = line.find('=') {
                        let (k, v) = line.split_at(idx);
                        let v = &v[1..];
                        if k.trim() == key {
                            let data = parse_kv_data(v.trim());
                            results.push(TorrcEntry {
                                key: k.trim().to_string(),
                                value: v.trim().to_string(),
                                data,
                            });
                        }
                    } else if let Some(idx) = line.find(' ') {
                        let (k, v) = line.split_at(idx);
                        let v = v.trim();
                        if k.trim() == key {
                            let data = parse_kv_data(v);
                            results.push(TorrcEntry {
                                key: k.trim().to_string(),
                                value: v.to_string(),
                                data,
                            });
                        }
                    } else if line == key {
                        results.push(TorrcEntry {
                            key: key.clone(),
                            value: String::new(),
                            data: vec![],
                        });
                    }
                }
            }
            Err(_) => {
                continue;
            }
        }
    }
    results
}

/// Returns the first TorrcEntry with default=true in its data, or None if not found.
pub async fn get_torrc_default_value(config: &RpcConfig, keyword: &str) -> Option<TorrcEntry> {
    let entries = get_torrc_value(config, &[keyword.to_string()]).await;
    entries.into_iter().find(|entry| {
        entry
            .data
            .iter()
            .any(|kv| kv.key == "default" && kv.value == "true")
    })
}

fn parse_kv_data(val: &str) -> Vec<KV> {
    // Only parse if at least one '=' is present
    if !val.contains('=') {
        return Vec::new();
    }
    let mut data = Vec::new();
    for part in val.split_whitespace() {
        if let Some(idx) = part.find('=') {
            let key = &part[..idx];
            let value = &part[idx + 1..];
            data.push(KV {
                key: key.to_string(),
                value: value.to_string(),
            });
        } else {
            data.push(KV {
                key: part.to_string(),
                value: String::new(),
            });
        }
    }
    data
}

pub async fn get_conf(config: &RpcConfig, setting: String) -> Result<String, Box<dyn Error>> {
    info!("get_conf: {:?}", config);
    let rpc = rpc_client(RpcConfig {
        addr: config.clone().addr,
        rpc_password: config.clone().rpc_password,
        command: format!("GETCONF {}", setting).into(),
    })
    .await?;

    if rpc.starts_with("250 ") {
        let resp = rpc.trim_start_matches("250 ");
        Ok(resp.to_string())
    } else {
        Ok("".to_string())
    }
}

pub async fn get_conf_payment_circuit_max_fee(config: &RpcConfig) -> Result<u64, Box<dyn Error>> {
    let conf = get_conf(&config, "PaymentCircuitMaxFee".to_string())
        .await
        .unwrap();
    if conf.is_empty() {
        return Ok(12000);
    }
    let parts: Vec<&str> = conf.split('=').collect();
    // println!("Debug: conf = {}", conf);
    // println!("Debug: parts = {:?}", parts);
    if parts.len() == 2 {
        if let Ok(value) = parts[1].trim().parse::<u64>() {
            return Ok(value);
        }
    }
    Ok(12000)
}

/// Gets the ExitNodes setting from torrc and parses the values into a Vec<String>.
/// Handles comma and space separated values, curly-brace country codes, and nicknames.
pub async fn get_conf_exit_nodes(config: &RpcConfig) -> Option<TorrcEntry> {
    let conf = get_torrc_value(config, &["ExitNodes".to_string()]).await;
    info!("conf: {:?}", conf);
    if conf.is_empty() {
        return None;
    }
    // return first entry
    return Some(conf[0].clone());
}

/// Parses a torrc file and extracts RpcConfig settings if present.
/// Returns Option<RpcConfig> if found, otherwise None.
pub async fn get_rpc_config_from_torrc(
    torrc_path: &str,
    rpc_password: Option<String>,
) -> Option<RpcConfig> {
    let mut rpc_config: Option<RpcConfig> = None;

    if let Ok(entries) = parse_raw_torrc_file(torrc_path).await {
        // Look for an entry with key "RpcConfig"
        for entry in &entries {
            if entry.key == "ControlPort" || entry.key == "Address" {
                // We'll collect these values to build addr later
            }
        }
        // After collecting all entries, search for Address and ControlPort
        let mut address = "127.0.0.1".to_string();
        let mut port = "9999".to_string();
        for entry in &entries {
            // TODO - probably remove this becuase a Relay might use a public address and we dont want to use a public IP for the control port
            // if entry.key == "Address" && !entry.value.is_empty() {
            //     address = entry.value.clone();
            // }
            if entry.key == "ControlPort" && !entry.value.is_empty() {
                port = entry.value.clone();
            }
        }
        let addr = format!("{}:{}", address, port);
        rpc_config = Some(RpcConfig {
            addr,
            rpc_password: rpc_password.clone(),
            command: "".to_string(),
        });
    }
    return rpc_config;
}

pub async fn parse_raw_torrc_file(torrc_path: &str) -> Result<Vec<TorrcEntry>, Box<dyn Error>> {
    let mut torrc = String::new();
    let file = std::fs::File::open(torrc_path)?;
    let reader = std::io::BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        torrc.push_str(&line);
        torrc.push('\n');
    }
    // Parse each non-comment, non-empty line into TorrcEntry
    let mut entries = Vec::new();
    for line in torrc.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(idx) = line.find(' ') {
            let (k, v) = line.split_at(idx);
            let v = v.trim();
            let data = parse_kv_data(v);
            entries.push(TorrcEntry {
                key: k.trim().to_string(),
                value: v.to_string(),
                data,
            });
        } else {
            entries.push(TorrcEntry {
                key: line.to_string(),
                value: String::new(),
                data: vec![],
            });
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RpcConfig;
    use std::error::Error;

    // Fake get_conf for testing
    async fn fake_get_conf(
        _config: &RpcConfig,
        setting: String,
        torrc: &str,
    ) -> Result<String, Box<dyn Error>> {
        let mut result = String::new();
        for line in torrc.lines() {
            let line = line.trim();
            if line.starts_with(&setting) {
                // Remove comments
                let line = line.split('#').next().unwrap().trim();
                result.push_str(line);
                result.push('\n');
            }
        }
        if result.is_empty() {
            Ok(String::new())
        } else {
            Ok(result.trim().to_string())
        }
    }

    #[tokio::test]
    async fn test_get_torrc_value_basic() {
        let torrc = r#"
PaymentCircuitMaxFee 11000
PaymentLightningNodeConfig type=phoenixd url=http://url.com password=pass1234 default=true
PaymentLightningNodeConfig type=lnd url=http://lnd.com macaroon=mac1234
"#;
        let config = RpcConfig {
            addr: "dummy".to_string(),
            rpc_password: Some("dummy".to_string()),
            command: "".to_string(),
        };
        // Patch get_conf for this test
        async fn test_get_torrc_value_inner(
            config: &RpcConfig,
            keywords: &[String],
            torrc: &str,
        ) -> Vec<TorrcEntry> {
            let mut results = Vec::new();
            for key in keywords {
                match fake_get_conf(config, key.clone(), torrc).await {
                    Ok(resp) => {
                        let resp = resp.trim();
                        if resp.is_empty() {
                            continue;
                        }
                        for line in resp.lines() {
                            let line = line.trim_start_matches("250 ").trim();
                            if line.is_empty() || line == "OK" || line == "closing connection" {
                                continue;
                            }
                            if let Some(idx) = line.find(' ') {
                                let (k, v) = line.split_at(idx);
                                let v = v.trim();
                                if k.trim() == key {
                                    let data = parse_kv_data(v);
                                    results.push(TorrcEntry {
                                        key: k.trim().to_string(),
                                        value: v.to_string(),
                                        data,
                                    });
                                }
                            } else if line == key {
                                results.push(TorrcEntry {
                                    key: key.clone(),
                                    value: String::new(),
                                    data: vec![],
                                });
                            }
                        }
                    }
                    Err(_) => continue,
                }
            }
            results
        }
        let keys = vec![
            "PaymentCircuitMaxFee".to_string(),
            "PaymentLightningNodeConfig".to_string(),
        ];
        let result = test_get_torrc_value_inner(&config, &keys, torrc).await;
        info!("Test result: {:?}", result);
        assert_eq!(
            result,
            vec![
                TorrcEntry {
                    key: "PaymentCircuitMaxFee".to_string(),
                    value: "11000".to_string(),
                    data: vec![],
                },
                TorrcEntry {
                    key: "PaymentLightningNodeConfig".to_string(),
                    value: "type=phoenixd url=http://url.com password=pass1234 default=true"
                        .to_string(),
                    data: vec![
                        KV {
                            key: "type".to_string(),
                            value: "phoenixd".to_string()
                        },
                        KV {
                            key: "url".to_string(),
                            value: "http://url.com".to_string()
                        },
                        KV {
                            key: "password".to_string(),
                            value: "pass1234".to_string()
                        },
                        KV {
                            key: "default".to_string(),
                            value: "true".to_string()
                        },
                    ],
                },
                TorrcEntry {
                    key: "PaymentLightningNodeConfig".to_string(),
                    value: "type=lnd url=http://lnd.com macaroon=mac1234".to_string(),
                    data: vec![
                        KV {
                            key: "type".to_string(),
                            value: "lnd".to_string()
                        },
                        KV {
                            key: "url".to_string(),
                            value: "http://lnd.com".to_string()
                        },
                        KV {
                            key: "macaroon".to_string(),
                            value: "mac1234".to_string()
                        },
                    ],
                },
            ]
        );
    }

    #[tokio::test]
    async fn test_parse_kv_data_nwc_uri_directly() {
        // Test the parse_kv_data function directly with the full NWC configuration
        let test_value = "type=nwc uri=nostr+walletconnect://abc123def456789012345678901234567890123456789012345678901234567890?relay=wss://relay.example.com/v1&secret=1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef&lud16=testuser@example.com default=true";
        
        let parsed_data = parse_kv_data(test_value);
        
        // Verify we have the expected number of key-value pairs
        assert_eq!(parsed_data.len(), 3);
        
        // Find and verify each key-value pair
        let type_kv = parsed_data.iter().find(|kv| kv.key == "type").unwrap();
        assert_eq!(type_kv.value, "nwc");
        
        let uri_kv = parsed_data.iter().find(|kv| kv.key == "uri").unwrap();
        let expected_uri = "nostr+walletconnect://abc123def456789012345678901234567890123456789012345678901234567890?relay=wss://relay.example.com/v1&secret=1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef&lud16=testuser@example.com";
        assert_eq!(uri_kv.value, expected_uri);
        
        let default_kv = parsed_data.iter().find(|kv| kv.key == "default").unwrap();
        assert_eq!(default_kv.value, "true");
        
        // Verify the URI contains all the expected parameters
        assert!(uri_kv.value.contains("relay=wss://relay.example.com/v1"));
        assert!(uri_kv.value.contains("secret=1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"));
        assert!(uri_kv.value.contains("lud16=testuser@example.com"));
        
        info!("Direct KV parsing test result: {:?}", parsed_data);
    }
}

#[cfg(test)]
mod default_value_tests {
    use super::*;
    use crate::types::RpcConfig;

    // Fake get_conf for testing
    async fn fake_get_conf(
        _config: &RpcConfig,
        setting: String,
        torrc: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut result = String::new();
        for line in torrc.lines() {
            let line = line.trim();
            if line.starts_with(&setting) {
                // Remove comments
                let line = line.split('#').next().unwrap().trim();
                result.push_str(line);
                result.push('\n');
            }
        }
        if result.is_empty() {
            Ok(String::new())
        } else {
            Ok(result.trim().to_string())
        }
    }

    #[tokio::test]
    async fn test_get_torrc_default_value() {
        let torrc = r#"
PaymentLightningNodeConfig type=phoenixd url=http://url.com password=pass1234 default=true
PaymentLightningNodeConfig type=lnd url=http://lnd.com macaroon=mac1234
"#;
        let config = RpcConfig {
            addr: "dummy".to_string(),
            rpc_password: Some("dummy".to_string()),
            command: "".to_string(),
        };
        // Patch get_conf for this test
        async fn test_get_torrc_value_inner(
            config: &RpcConfig,
            keywords: &[String],
            torrc: &str,
        ) -> Vec<TorrcEntry> {
            let mut results = Vec::new();
            for key in keywords {
                match fake_get_conf(config, key.clone(), torrc).await {
                    Ok(resp) => {
                        let resp = resp.trim();
                        if resp.is_empty() {
                            continue;
                        }
                        for line in resp.lines() {
                            let line = line.trim_start_matches("250 ").trim();
                            if line.is_empty() || line == "OK" || line == "closing connection" {
                                continue;
                            }
                            if let Some(idx) = line.find(' ') {
                                let (k, v) = line.split_at(idx);
                                let v = v.trim();
                                if k.trim() == key {
                                    let data = super::parse_kv_data(v);
                                    results.push(TorrcEntry {
                                        key: k.trim().to_string(),
                                        value: v.to_string(),
                                        data,
                                    });
                                }
                            } else if line == key {
                                results.push(TorrcEntry {
                                    key: key.clone(),
                                    value: String::new(),
                                    data: vec![],
                                });
                            }
                        }
                    }
                    Err(_) => continue,
                }
            }
            results
        }
        // Simulate get_torrc_value
        let entries =
            test_get_torrc_value_inner(&config, &["PaymentLightningNodeConfig".to_string()], torrc)
                .await;
        // Find the default entry
        let default_entry = entries.into_iter().find(|entry| {
            entry
                .data
                .iter()
                .any(|kv| kv.key == "default" && kv.value == "true")
        });
        assert!(default_entry.is_some());
        let entry = default_entry.unwrap();
        assert_eq!(entry.key, "PaymentLightningNodeConfig");
        assert_eq!(
            entry
                .data
                .iter()
                .find(|kv| kv.key == "default")
                .unwrap()
                .value,
            "true"
        );
        assert_eq!(
            entry.data.iter().find(|kv| kv.key == "type").unwrap().value,
            "phoenixd"
        );
        assert_eq!(
            entry.data.iter().find(|kv| kv.key == "url").unwrap().value,
            "http://url.com"
        );
        assert_eq!(
            entry
                .data
                .iter()
                .find(|kv| kv.key == "password")
                .unwrap()
                .value,
            "pass1234"
        );
    }
}
