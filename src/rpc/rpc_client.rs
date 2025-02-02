use std::error::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

// TOR RPC Commands
// https://spec.torproject.org/control-spec/commands.html?highlight=Setevent#extended_events

pub struct RpcConfig {
    pub addr: String,
    pub rpc_password: String,
    pub command: String,
}

// TODO turn into impl and struct called RpcClient
// returns an rpc client
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

    println!("Sending {} command...", config.command);
    writer.write_all(format!("{}\r\n", config.command).as_bytes()).await?;
    writer.flush().await?;

    // Read the response
    println!("Reading response...");
    response.clear();
    reader.read_line(&mut response).await?;
    if response.starts_with("250") {
        let mut resp = String::new();
        while reader.read_line(&mut resp).await? > 0 {
            if resp.ends_with(".\r\n") {
                break;
            }
        }
        return Ok(resp);
    }

    println!("Failed to get response.");
    Err("Failed to get response".into())
}
