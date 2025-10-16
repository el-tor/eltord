use crate::types::RpcConfig;
use log::{debug, info};
use std::error::Error;
use tokio::time::{sleep, Duration};

/// Waits for Tor to complete bootstrapping by polling the `status/bootstrap-phase` control command.
///
/// This function continuously polls the Tor control port using `GETINFO status/bootstrap-phase`
/// until the bootstrap process reaches 95% or higher (circuit_create stage) AND relay descriptors are available.
///
/// # Arguments
///
/// * `rpc_config` - Configuration for the RPC client (contains control port address and password)
/// * `timeout_secs` - Maximum time to wait for bootstrap completion (in seconds)
///
/// # Returns
///
/// * `Ok(())` - Bootstrap completed successfully (PROGRESS>=95) and descriptors available
/// * `Err(Box<dyn Error + Send + Sync>)` - Timeout reached or connection error
///
/// # Example Response Format
///
/// The Tor control protocol returns bootstrap status in this format:
/// ```text
/// 250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=95 TAG=circuit_create SUMMARY="Establishing a Tor circuit"
/// ```
///
/// # Note
///
/// Bootstrap PROGRESS=95 (circuit_create) means Tor has loaded enough directory info for circuits.
/// We accept 95% or higher instead of waiting for 100% because:
/// - At 95%, Tor has already downloaded relay descriptors
/// - Waiting for 100% can cause unnecessary delays (100% = first circuit built)
/// - We verify descriptors are available before proceeding anyway
///
/// # References
///
/// - Tor Control Spec: https://spec.torproject.org/control-spec/commands.html#getinfo
/// - Bootstrap phases: https://spec.torproject.org/control-spec/server-status.html#bootstrap-phase
pub async fn wait_for_tor_bootstrap(
    rpc_config: &RpcConfig,
    timeout_secs: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!("Waiting for Tor bootstrap (timeout: {}s)...", timeout_secs);
    
    let start_time = std::time::Instant::now();
    let timeout_duration = Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_millis(500); // Poll every 500ms
    
    let mut bootstrap_complete = false;
    
    loop {
        // Check if timeout has been reached
        if start_time.elapsed() > timeout_duration {
            return Err(format!(
                "Tor bootstrap timeout after {} seconds",
                timeout_secs
            )
            .into());
        }
        
        // Query bootstrap status using GETINFO status/bootstrap-phase
        let bootstrap_config = RpcConfig {
            addr: rpc_config.addr.clone(),
            rpc_password: rpc_config.rpc_password.clone(),
            command: "GETINFO status/bootstrap-phase".to_string(),
        };
        
        let response_result = crate::rpc::rpc_client(bootstrap_config)
            .await
            .map_err(|e| e.to_string());
        
        match response_result {
            Ok(response) => {
                debug!("Bootstrap response: {}", response.trim());
                
                // Parse the response to extract PROGRESS value
                // Expected format: "250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=100 TAG=done SUMMARY=\"Done\""
                if let Some(progress) = extract_bootstrap_progress(&response) {
                    if progress < 100 {
                        info!("Tor bootstrap progress: {}%", progress);
                    } else if !bootstrap_complete {
                        info!("Tor bootstrap progress: 100%");
                        bootstrap_complete = true;
                    }
                    
                    // At 95% (circuit_create), Tor has loaded enough directory info to build circuits.
                    // We verify descriptors are available and that at least one general-purpose circuit exists.
                    // This ensures SOCKS is ready: bootstrap ≥95% + descriptors + working circuit = SOCKS ready
                    // Reference: https://spec.torproject.org/socks-extensions.html (optimistic data section)
                    if progress >= 95 {
                        match verify_descriptors_available(rpc_config).await {
                            Ok(true) => {
                                // Descriptors available, now check if there's a usable circuit
                                match verify_circuit_available(rpc_config).await {
                                    Ok(true) => {
                                        info!("Tor ready: {}% bootstrap + descriptors + working circuit!", progress);
                                        return Ok(());
                                    }
                                    Ok(false) => {
                                        debug!("Bootstrap at {}% with descriptors but no working circuit yet, waiting...", progress);
                                    }
                                    Err(e) => {
                                        debug!("Error checking circuits: {}", e);
                                    }
                                }
                            }
                            Ok(false) => {
                                debug!("Bootstrap at {}% but descriptors not yet available, waiting...", progress);
                            }
                            Err(e) => {
                                debug!("Error checking descriptors: {}", e);
                                // Continue polling
                            }
                        }
                    }
                }
            }
            Err(e) => {
                debug!("Error querying bootstrap status: {}", e);
                // Continue polling even on error - Tor might still be starting up
            }
        }
        
        // Wait before next poll
        sleep(poll_interval).await;
    }
}

/// Verifies that relay descriptors are actually available via `GETINFO desc/all-recent`.
///
/// # Arguments
///
/// * `rpc_config` - Configuration for the RPC client
///
/// # Returns
///
/// * `Ok(true)` - Descriptors are available (response contains "router" entries)
/// * `Ok(false)` - No descriptors available yet
/// * `Err` - Connection or RPC error
///
/// # Note
///
/// This prevents the "All routers are down or won't exit" error that occurs when
/// bootstrap reaches 100% but relay descriptors haven't been downloaded yet.
async fn verify_descriptors_available(rpc_config: &RpcConfig) -> Result<bool, Box<dyn Error + Send + Sync>> {
    let desc_config = RpcConfig {
        addr: rpc_config.addr.clone(),
        rpc_password: rpc_config.rpc_password.clone(),
        command: "GETINFO desc/all-recent".to_string(),
    };
    
    let response_result = crate::rpc::rpc_client(desc_config)
        .await
        .map_err(|e| e.to_string());
    
    match response_result {
        Ok(response) => {
            // Check if response contains any "router" entries
            let descriptor_count = response.lines().filter(|line| line.starts_with("router ")).count();
            
            if descriptor_count > 0 {
                info!("Found {} relay descriptors available", descriptor_count);
                Ok(true)
            } else {
                debug!("No relay descriptors available yet");
                Ok(false)
            }
        }
        Err(e) => {
            debug!("Error checking descriptors: {}", e);
            Ok(false) // Treat errors as "not ready yet"
        }
    }
}

/// Verifies that at least one general-purpose circuit is BUILT and ready for SOCKS traffic.
///
/// # Arguments
///
/// * `rpc_config` - Configuration for the RPC client
///
/// # Returns
///
/// * `Ok(true)` - At least one BUILT circuit exists for general use
/// * `Ok(false)` - No working circuits available yet
/// * `Err` - Connection or RPC error
///
/// # Note
///
/// This is the final check for SOCKS readiness. The SOCKS proxy listener opens early
/// during bootstrap, but connections will fail until there's a working circuit.
/// Circuit states: LAUNCHED → BUILDING → EXTENDED → BUILT
async fn verify_circuit_available(rpc_config: &RpcConfig) -> Result<bool, Box<dyn Error + Send + Sync>> {
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
            // Look for any circuit in BUILT state with PURPOSE=GENERAL
            // Format: "123 BUILT $FP1~relay1,$FP2~relay2,$FP3~relay3 PURPOSE=GENERAL"
            for line in response.lines() {
                if line.contains(" BUILT ") && line.contains("PURPOSE=GENERAL") {
                    debug!("Found working general-purpose circuit: {}", line);
                    return Ok(true);
                }
            }
            debug!("No BUILT general-purpose circuits found yet");
            Ok(false)
        }
        Err(e) => {
            debug!("Error checking circuits: {}", e);
            Ok(false) // Treat errors as "not ready yet"
        }
    }
}

/// Extracts the PROGRESS value from a Tor bootstrap-phase response.
///
/// # Arguments
///
/// * `response` - The raw response from `GETINFO status/bootstrap-phase`
///
/// # Returns
///
/// * `Some(progress)` - The progress percentage (0-100)
/// * `None` - If parsing fails or PROGRESS field is not found
///
/// # Example
///
/// ```text
/// Input: "250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=85 TAG=loading_descriptors SUMMARY=\"Loading relay descriptors\""
/// Output: Some(85)
/// ```
fn extract_bootstrap_progress(response: &str) -> Option<u32> {
    // Look for "PROGRESS=" in the response
    for line in response.lines() {
        if line.contains("PROGRESS=") {
            // Find the PROGRESS= field and extract the number
            if let Some(progress_start) = line.find("PROGRESS=") {
                let progress_str = &line[progress_start + 9..]; // Skip "PROGRESS="
                
                // Extract digits until we hit a space or end of string
                let progress_digits: String = progress_str
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                
                if let Ok(progress) = progress_digits.parse::<u32>() {
                    return Some(progress);
                }
            }
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_bootstrap_progress_complete() {
        let response = r#"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=100 TAG=done SUMMARY="Done"
250 OK
"#;
        assert_eq!(extract_bootstrap_progress(response), Some(100));
    }

    #[test]
    fn test_extract_bootstrap_progress_partial() {
        let response = r#"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=85 TAG=loading_descriptors SUMMARY="Loading relay descriptors"
250 OK
"#;
        assert_eq!(extract_bootstrap_progress(response), Some(85));
    }

    #[test]
    fn test_extract_bootstrap_progress_early() {
        let response = r#"250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=5 TAG=conn SUMMARY="Connecting to a relay"
250 OK
"#;
        assert_eq!(extract_bootstrap_progress(response), Some(5));
    }

    #[test]
    fn test_extract_bootstrap_progress_invalid() {
        let response = "250 OK\n";
        assert_eq!(extract_bootstrap_progress(response), None);
    }
}
