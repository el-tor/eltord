use crate::rpc::rpc_client;
use crate::types::RpcConfig;
use log::{info, warn};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Enables manual stream attachment mode and starts monitoring for new streams.
/// Returns a handle that continuously attaches incoming streams to circuits in round-robin fashion.
pub async fn start_stream_attachment_monitor(
    rpc_config: RpcConfig,
    primary_circuit_id: String,
    backup_circuit_id: String,
) -> Result<tokio::task::JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
    // Subscribe before disabling automatic attachment so no NEW events are lost.
    let reader = subscribe_stream_events(&rpc_config).await?;
    enable_manual_stream_attachment(&rpc_config).await?;

    // Subscribe to stream events
    let handle = tokio::spawn(async move {
        if let Err(e) =
            stream_attachment_loop(&rpc_config, &primary_circuit_id, &backup_circuit_id, reader)
                .await
        {
            warn!("Stream attachment monitor stopped: {}", e);
        }
        if let Err(e) = restore_automatic_stream_attachment(&rpc_config).await {
            warn!("Failed to restore automatic stream attachment: {}", e);
        }
    });

    Ok(handle)
}

/// Enables manual stream attachment by setting __LeaveStreamsUnattached=1
async fn enable_manual_stream_attachment(
    rpc_config: &RpcConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = RpcConfig {
        addr: rpc_config.addr.clone(),
        rpc_password: rpc_config.rpc_password.clone(),
        command: "SETCONF __LeaveStreamsUnattached=1".to_string(),
    };

    let response = match rpc_client(config).await {
        Ok(r) => r,
        Err(e) => {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("RPC call failed: {}", e),
            )))
        }
    };

    if super::command_response(&response).is_ok() {
        info!("✅ Manual stream attachment enabled");
        Ok(())
    } else {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to enable manual stream attachment: {}", response),
        )))
    }
}

/// Restore automatic attachment after a monitor exits or is explicitly stopped.
pub async fn restore_automatic_stream_attachment(
    rpc_config: &RpcConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut config = rpc_config.clone();
    config.command = "SETCONF __LeaveStreamsUnattached=0".into();
    let response = rpc_client(config).await.map_err(|e| e.to_string())?;
    super::command_response(&response).map_err(|e| e.to_string())?;
    Ok(())
}

async fn subscribe_stream_events(
    rpc_config: &RpcConfig,
) -> Result<BufReader<TcpStream>, Box<dyn std::error::Error + Send + Sync>> {
    // Connect to control port
    let mut stream = TcpStream::connect(&rpc_config.addr).await?;

    // Authenticate (always required by Tor control protocol)
    let auth_command = if let Some(password) = &rpc_config.rpc_password {
        format!("AUTHENTICATE \"{}\"\r\n", password)
    } else {
        "AUTHENTICATE\r\n".to_string()
    };

    stream.write_all(auth_command.as_bytes()).await?;

    // Create reader and check authentication response
    let mut reader = BufReader::new(stream);
    let mut auth_response = String::new();
    reader.read_line(&mut auth_response).await?;

    if !auth_response.contains("250 OK") {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("Authentication failed: {}", auth_response),
        )));
    }

    // Subscribe to STREAM events
    reader.get_mut().write_all(b"SETEVENTS STREAM\r\n").await?;
    let mut event_response = String::new();
    reader.read_line(&mut event_response).await?;

    if !event_response.contains("250 OK") {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to subscribe to STREAM events: {}", event_response),
        )));
    }

    Ok(reader)
}

/// A monitor owns one subscription and one pair of circuit IDs.
async fn stream_attachment_loop(
    rpc_config: &RpcConfig,
    primary_circuit_id: &str,
    backup_circuit_id: &str,
    mut reader: BufReader<TcpStream>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!(
        "🔄 Stream attachment monitor active - distributing streams across circuits {} and {}",
        primary_circuit_id, backup_circuit_id
    );

    // Counter for round-robin distribution
    static STREAM_COUNTER: AtomicU64 = AtomicU64::new(0);

    // Only manage client streams. Directory streams bypass controller attachment.
    let mut client_streams = HashSet::new();
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            warn!("Control connection closed");
            break;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 || fields[0] != "650" || fields[1] != "STREAM" {
            continue;
        }
        let stream_id = fields[2].to_string();
        let status = fields[3];
        if matches!(status, "CLOSED" | "FAILED") {
            client_streams.remove(&stream_id);
            continue;
        }
        if matches!(status, "NEW" | "NEWRESOLVE") {
            if fields
                .iter()
                .any(|f| matches!(*f, "PURPOSE=DIR_FETCH" | "PURPOSE=DIR_UPLOAD"))
            {
                continue;
            }
            client_streams.insert(stream_id.clone());
        } else if status != "DETACHED" || !client_streams.contains(&stream_id) {
            continue;
        }
        let count = STREAM_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Alternate between circuits
        let target_circuit = if count % 2 == 0 {
            primary_circuit_id
        } else {
            backup_circuit_id
        };

        // Attach stream to selected circuit
        if let Err(e) = attach_stream_to_circuit(rpc_config, &stream_id, target_circuit).await {
            warn!(
                "⚠️ Failed to attach stream {} to circuit {}: {}",
                stream_id, target_circuit, e
            );
            let alternate = if target_circuit == primary_circuit_id {
                backup_circuit_id
            } else {
                primary_circuit_id
            };
            // An unsuccessful ATTACH leaves the stream pending. Try the
            // other paid circuit, then let Tor retry automatically.
            if attach_stream_to_circuit(rpc_config, &stream_id, alternate)
                .await
                .is_err()
            {
                if let Err(e) = attach_stream_to_circuit(rpc_config, &stream_id, "0").await {
                    // A stream can close between its event and our RPC.
                    // A per-stream rejection must not stop all renewals.
                    warn!("Stream {} could not be attached: {}", stream_id, e);
                }
            }
        }
    }

    Ok(())
}

/// Attaches a specific stream to a specific circuit using ATTACHSTREAM
async fn attach_stream_to_circuit(
    rpc_config: &RpcConfig,
    stream_id: &str,
    circuit_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = RpcConfig {
        addr: rpc_config.addr.clone(),
        rpc_password: rpc_config.rpc_password.clone(),
        command: format!("ATTACHSTREAM {} {}", stream_id, circuit_id),
    };

    let response = match rpc_client(config).await {
        Ok(r) => r,
        Err(e) => {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("RPC call failed: {}", e),
            )))
        }
    };

    if super::command_response(&response).is_ok() {
        Ok(())
    } else {
        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("ATTACHSTREAM failed: {}", response),
        )))
    }
}
