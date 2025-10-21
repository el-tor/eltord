use crate::types::RpcConfig;
use log::{debug, info};
use std::error::Error;
use tokio::time::{sleep, Duration};

/// Waits for a Tor circuit to be fully built and ready for use.
///
/// This function polls the circuit status using `GETINFO circuit-status` until
/// the specified circuit reaches BUILT state. This is critical because:
/// - Circuit ID is assigned immediately (LAUNCHED state)
/// - But SOCKS connections fail until circuit is BUILT
/// - Can take 2-10 seconds for 3-hop circuit to fully build
///
/// # Arguments
///
/// * `rpc_config` - Configuration for the RPC client (contains control port address and password)
/// * `circuit_id` - The circuit ID to wait for (e.g., "123")
/// * `timeout_secs` - Maximum time to wait for circuit to build (in seconds)
///
/// # Returns
///
/// * `Ok(())` - Circuit is BUILT and ready for SOCKS connections
/// * `Err(Box<dyn Error + Send + Sync>)` - Timeout, circuit failed, or connection error
///
/// # Circuit States
///
/// Tor circuits progress through these states:
/// - **LAUNCHED**: Circuit ID assigned, starting to build
/// - **BUILDING**: Extending through hops (relay 1, 2, 3...)
/// - **EXTENDED**: All hops extended successfully
/// - **BUILT**: Circuit fully constructed and ready for traffic ✅
/// - **FAILED**: Circuit build failed (relay unreachable, etc.)
/// - **CLOSED**: Circuit was closed
///
/// # Example Response Format
///
/// ```text
/// 250-circuit-status=
/// 123 BUILT $FP1~relay1,$FP2~relay2,$FP3~relay3 PURPOSE=GENERAL
/// 124 BUILDING $FP1~relay1 PURPOSE=GENERAL
/// .
/// 250 OK
/// ```
///
/// # References
///
/// - Tor Control Spec: https://spec.torproject.org/control-spec/commands.html#getinfo
/// - Circuit Status: https://spec.torproject.org/control-spec/server-status.html#circuit-status
pub async fn wait_for_circuit_ready(
    rpc_config: &RpcConfig,
    circuit_id: &str,
    timeout_secs: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!("Waiting for circuit {} to be BUILT (timeout: {}s)...", circuit_id, timeout_secs);
    
    let start_time = std::time::Instant::now();
    let timeout_duration = Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_millis(200); // Poll every 200ms for responsive detection
    
    loop {
        // Check if timeout has been reached
        if start_time.elapsed() > timeout_duration {
            return Err(format!(
                "Circuit {} build timeout after {} seconds",
                circuit_id, timeout_secs
            )
            .into());
        }
        
        // Query circuit status using GETINFO circuit-status
        let circuit_config = RpcConfig {
            addr: rpc_config.addr.clone(),
            rpc_password: rpc_config.rpc_password.clone(),
            command: "GETINFO circuit-status".to_string(),
        };
        
        let response_result = crate::rpc::rpc_client(circuit_config)
            .await
            .map_err(|e| e.to_string());
        
        match response_result {
            Ok(response) => {
                debug!("Circuit status response: {}", response.trim());
                
                // Parse the response to find our circuit
                if let Some(state) = extract_circuit_state(&response, circuit_id) {
                    match state.as_str() {
                        "BUILT" => {
                            info!("Circuit {} is BUILT and ready for traffic!", circuit_id);
                            return Ok(());
                        }
                        "FAILED" | "CLOSED" => {
                            return Err(format!(
                                "Circuit {} entered {} state (build failed)",
                                circuit_id, state
                            )
                            .into());
                        }
                        state => {
                            debug!("Circuit {} state: {}", circuit_id, state);
                            // Continue polling for LAUNCHED, BUILDING, EXTENDED
                        }
                    }
                } else {
                    debug!("Circuit {} not found in status (may not be launched yet)", circuit_id);
                }
            }
            Err(e) => {
                debug!("Error querying circuit status: {}", e);
                // Continue polling even on error
            }
        }
        
        // Wait before next poll
        sleep(poll_interval).await;
    }
}

/// Extracts the state of a specific circuit from the circuit-status response.
///
/// # Arguments
///
/// * `response` - The raw response from `GETINFO circuit-status`
/// * `circuit_id` - The circuit ID to search for
///
/// # Returns
///
/// * `Some(state)` - The state of the circuit (BUILT, BUILDING, etc.)
/// * `None` - Circuit not found in response
///
/// # Example
///
/// ```text
/// Input:
///   response = "250-circuit-status=\n123 BUILT $FP1~relay1,$FP2~relay2\n.\n250 OK"
///   circuit_id = "123"
/// Output: Some("BUILT")
/// ```
fn extract_circuit_state(response: &str, circuit_id: &str) -> Option<String> {
    for line in response.lines() {
        // Look for lines starting with the circuit ID
        // Format: "123 BUILT $FP1~relay1,$FP2~relay2 PURPOSE=GENERAL"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[0] == circuit_id {
            // parts[0] = circuit_id, parts[1] = state
            return Some(parts[1].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_circuit_state_built() {
        let response = r#"250-circuit-status=
123 BUILT $FP1~relay1,$FP2~relay2,$FP3~relay3 PURPOSE=GENERAL
124 BUILDING $FP1~relay1 PURPOSE=GENERAL
.
250 OK
"#;
        assert_eq!(extract_circuit_state(response, "123"), Some("BUILT".to_string()));
    }

    #[test]
    fn test_extract_circuit_state_building() {
        let response = r#"250-circuit-status=
123 BUILDING $FP1~relay1 PURPOSE=GENERAL
.
250 OK
"#;
        assert_eq!(extract_circuit_state(response, "123"), Some("BUILDING".to_string()));
    }

    #[test]
    fn test_extract_circuit_state_not_found() {
        let response = r#"250-circuit-status=
124 BUILT $FP1~relay1,$FP2~relay2 PURPOSE=GENERAL
.
250 OK
"#;
        assert_eq!(extract_circuit_state(response, "123"), None);
    }

    #[test]
    fn test_extract_circuit_state_failed() {
        let response = r#"250-circuit-status=
123 FAILED
.
250 OK
"#;
        assert_eq!(extract_circuit_state(response, "123"), Some("FAILED".to_string()));
    }
}
