use std::error::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

// TOR RPC Commands
// https://spec.torproject.org/control-spec/commands.html?highlight=Setevent#extended_events

#[derive(Debug, Clone)]
pub struct RpcConfig {
    pub addr: String,
    pub rpc_password: String,
    pub command: String,
}

// Implementing the Send trait for RpcConfig
unsafe impl Send for RpcConfig {}

// Returns an RPC client response
pub async fn rpc_client(config: RpcConfig) -> Result<String, Box<dyn Error>> {
    println!("Connecting to Tor control port...");
    let stream = TcpStream::connect(config.addr).await?;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    // Authenticate with the control port using the hardcoded password
    println!("Authenticating...");
    writer
        .write_all(format!("AUTHENTICATE \"{}\"\r\n", config.rpc_password).as_bytes())
        .await?;
    writer.flush().await?;

    let mut response = String::new();
    reader.read_line(&mut response).await?;
    if !response.starts_with("250") {
        return Err("Authentication failed".into());
    }

    // Send the command
    println!("Sending {} command...", config.command);
    writer
        .write_all(format!("{}\r\n", config.command).as_bytes())
        .await?;
    writer.flush().await?;

    // Read response line by line
    let mut response = String::new();
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // EOF
        }
        response.push_str(&line);

        // Check for "250 OK" which indicates end of response
        if line.trim().starts_with("250 ") {
            break;
        }
    }

    Ok(response)
}
