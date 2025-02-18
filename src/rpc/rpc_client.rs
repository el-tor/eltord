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

    let content = format!(
        "AUTHENTICATE \"{}\"\r\n{}\r\nQUIT\r\n",
        config.rpc_password, config.command
    );
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
