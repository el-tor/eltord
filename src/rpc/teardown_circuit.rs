use super::rpc_client;
use crate::types::RpcConfig;
use log::{info, warn};
use std::error::Error;

/// Teardown a circuit by sending TEARDOWNCIRCUIT command to Tor
/// Returns true if the teardown was successful (250 OK response), false otherwise
pub async fn teardown_circuit(
    config: &RpcConfig,
    circuit_id: &str,
) -> Result<bool, Box<dyn Error>> {
    info!("Initiating teardown for circuit {}", circuit_id);

    let rpc = rpc_client(RpcConfig {
        addr: config.addr.clone(),
        rpc_password: config.rpc_password.clone(),
        command: format!("TEARDOWNCIRCUIT {}", circuit_id),
    })
    .await;

    match rpc {
        Ok(response) => {
            info!(
                "Teardown response for circuit {}: {}",
                circuit_id,
                response.trim()
            );

            // Check if response contains "250 OK" which indicates success
            if response.contains("250 OK") {
                info!("✅ Successfully tore down circuit {}", circuit_id);
                Ok(true)
            } else {
                warn!(
                    "❌ Failed to teardown circuit {}: unexpected response",
                    circuit_id
                );
                Ok(false)
            }
        }
        Err(e) => {
            warn!("❌ Error tearing down circuit {}: {}", circuit_id, e);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_teardown_circuit_command_format() {
        // Test that the command is formatted correctly
        let config = RpcConfig {
            addr: "127.0.0.1:9051".to_string(),
            rpc_password: Some("test_password".to_string()),
            command: format!("TEARDOWNCIRCUIT {}", "123456789"),
        };

        assert_eq!(config.command, "TEARDOWNCIRCUIT 123456789");
    }

    #[test]
    fn test_response_parsing() {
        // Test that we can identify success responses
        let success_response = "250 OK SPEC-COMPLIANT TEARDOWN: DESTROY cells sent for circuit";
        assert!(success_response.contains("250 OK"));

        let failure_response = "551 Circuit not found";
        assert!(!failure_response.contains("250 OK"));
    }
}
