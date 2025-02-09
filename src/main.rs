use dotenv::dotenv;
use libtor::{HiddenServiceVersion, Tor, TorAddress, TorFlag};
use lni::phoenixd::{PhoenixdConfig, PhoenixdNode};
use std::error::Error;
use std::{env, fmt::format};
mod database;
pub use database::{Db, DbError, Payment};
mod rpc;
use rpc::{get_relay_descriptors, rpc_client, RpcConfig, Relay};

#[tokio::main]
async fn main() {
    dotenv().ok();

    println!("Generating hashed control password...");
    let password = env::var("CONTROL_PASSWORD").unwrap_or("password1234_".into());
    let hashed_password = "16:281EC5644A4F548A60D50A0DD4DF835FFD50EDED062FD270D7269943DA";
    let control_port: u16 = env::var("CONTROL_PORT")
        .unwrap_or("9051".into())
        .parse()
        .unwrap();

    println!("Starting Tor...");
    tokio::spawn(async move {
        let tor = Tor::new()
            .flag(TorFlag::DataDirectory("/tmp/tor-rust".into()))
            .flag(TorFlag::SocksPort(19050))
            .flag(TorFlag::ControlPort(control_port.into()))
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
    });


    // get_lightning_node_info().await;

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    loop {
        let rpc_config = RpcConfig {
            addr: format!("127.0.0.1:{}", &control_port.clone()),
            rpc_password: password.clone(),
            command: "GETINFO ns/all".into(),
        };
    
        let relays = get_relay_descriptors(rpc_config).await;
        match relays {
            Ok(relays) => {
                for relay in relays {
                    println!("{:?}", relay);
                }
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(100)).await;
    }
}

async fn get_lightning_node_info() {
    let url = env::var("PHOENIXD_URL").unwrap();
    let password = env::var("PHOENIXD_PASSWORD").unwrap();
    let node = PhoenixdNode::new(PhoenixdConfig { url, password });
    let info = node.get_info().await.unwrap();
    println!("Node info: {:?}", info)
}
