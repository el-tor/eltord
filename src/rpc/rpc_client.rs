use crate::types::{EventCallback, RpcConfig};
use lni::LightningNode;
use std::error::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

// TOR RPC Commands
// https://spec.torproject.org/control-spec/commands.html?highlight=Setevent#extended_events

// Implementing the Send trait for RpcConfig
unsafe impl Send for RpcConfig {}

// Returns an RPC client response
pub async fn rpc_client(config: RpcConfig) -> Result<String, Box<dyn Error>> {
    println!("Connecting to Tor control port...");
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

pub async fn rpc_event_listener(
    config: RpcConfig,
    event: String,
    event_callback: Box<dyn EventCallback + Send + Sync>,
    wallet: &(dyn LightningNode + Send + Sync),
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to Tor control port...");
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
        println!("Tor event: {}", line.trim_end());
        event_callback.success(Some(line.clone().trim_end().to_string()), wallet);
    }

    Ok(())
}
