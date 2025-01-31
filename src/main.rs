use dotenv::dotenv;
use libtor::{HiddenServiceVersion, Tor, TorAddress, TorFlag};
use lni::phoenixd::{PhoenixdConfig, PhoenixdNode};
use std::env;
use std::error::Error;

async fn get_lightning_node_info() {
   
    let url = env::var("PHOENIXD_URL").unwrap();
    let password = env::var("PHOENIXD_PASSWORD").unwrap();
    let node = PhoenixdNode::new(PhoenixdConfig { url, password });
    let info = node.get_info().await.unwrap();
    println!("Node info: {:?}", info)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();
    get_lightning_node_info().await;

    println!("Generating hashed control password...");

    let hashed_password = "16:281EC5644A4F548A60D50A0DD4DF835FFD50EDED062FD270D7269943DA";

    println!("Starting Tor...");
    let tor = Tor::new()
        .flag(TorFlag::DataDirectory("/tmp/tor-rust".into()))
        .flag(TorFlag::SocksPort(19050))
        .flag(TorFlag::ControlPort(9051))
        .flag(TorFlag::HashedControlPassword(
            hashed_password.trim().into(),
        ))
        .flag(TorFlag::HiddenServiceDir("/tmp/tor-rust/hs-dir".into()))
        .flag(TorFlag::HiddenServiceVersion(HiddenServiceVersion::V3))
        .flag(TorFlag::HiddenServicePort(
            TorAddress::Port(8000),
            None.into(),
        ))
        .start();

    match tor {
        Ok(_) => println!("Tor started successfully."),
        Err(e) => {
            println!("Failed to start Tor: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}
