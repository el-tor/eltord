use crate::types::{EventCallback, RpcConfig};
use lni::LightningNode;
use log::{debug, error, info, warn};
use std::error::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

// TOR RPC Commands
// https://spec.torproject.org/control-spec/commands.html?highlight=Setevent#extended_events

// Implementing the Send trait for RpcConfig
unsafe impl Send for RpcConfig {}

// Returns an RPC client response
pub async fn rpc_client(config: RpcConfig) -> Result<String, Box<dyn Error>> {
    tokio::time::timeout(std::time::Duration::from_secs(15), rpc_client_inner(config))
        .await
        .map_err(|_| -> Box<dyn Error> { "Tor control request timed out".into() })?
}

async fn rpc_client_inner(config: RpcConfig) -> Result<String, Box<dyn Error>> {
    // info!("Connecting to Tor control port at {}...", config.addr);
    let stream = TcpStream::connect(config.addr).await?;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    let pw = config.rpc_password.clone().filter(|p| !p.is_empty());

    let content = if pw.is_some() {
        format!(
            "AUTHENTICATE \"{}\"\r\n{}\r\nQUIT\r\n",
            pw.unwrap(),
            config.command
        )
    } else {
        format!("AUTHENTICATE\r\n{}\r\nQUIT\r\n", config.command)
    };
    writer.write_all(content.as_bytes()).await?;
    writer.flush().await?;

    // Read until EOF
    let mut response = String::new();
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break;
        }
        response.push_str(&line);
    }

    Ok(response)
}

/// Return only the command's response, after validating authentication and the
/// final command status. AUTHENTICATE's 250 OK is not command success.
pub fn command_response(response: &str) -> Result<String, Box<dyn Error>> {
    let mut lines = response.lines();
    if lines.next() != Some("250 OK") {
        return Err("Tor control authentication failed".into());
    }
    let mut command_lines = Vec::new();
    let mut complete = false;
    let mut data_block = false;
    for line in lines {
        if complete {
            break;
        }
        if data_block {
            command_lines.push(line);
            if line == "." {
                data_block = false;
            }
            continue;
        }
        if line.starts_with("650") {
            continue;
        }
        if line.len() >= 4 && line.as_bytes()[..3].iter().all(u8::is_ascii_digit) {
            if !line.starts_with("250") {
                return Err(format!("Tor control command failed: {}", line).into());
            }
            data_block = line.as_bytes()[3] == b'+';
            complete = line.as_bytes()[3] == b' ';
        }
        command_lines.push(line);
    }
    if !complete {
        return Err("Incomplete Tor control command response".into());
    }
    Ok(command_lines.join("\n"))
}

pub async fn rpc_event_listener(
    config: RpcConfig,
    event: String,
    event_callback: Box<dyn EventCallback + Send + Sync>,
    wallet: &(dyn LightningNode + Send + Sync),
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Connecting to Tor control port for event listening at {}...",
        config.addr
    );
    let stream = TcpStream::connect(config.addr.clone()).await?;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    let pw = config.rpc_password.clone().filter(|p| !p.is_empty());

    // Authenticate and subscribe to events (e.g., CIRC, NOTICE, etc.)
    let content = if pw.is_some() {
        format!(
            "AUTHENTICATE \"{}\"\r\nSETEVENTS {}\r\n",
            pw.unwrap(),
            event
        )
    } else {
        format!("AUTHENTICATE\r\nSETEVENTS {}\r\n", event)
    };
    writer.write_all(content.as_bytes()).await?;
    writer.flush().await?;

    // Continuously read and print events
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // Connection closed
        }
        info!("Tor event: {}", line.trim_end());
        event_callback.success(Some(line.clone().trim_end().to_string()), wallet);
    }

    Ok(())
}
