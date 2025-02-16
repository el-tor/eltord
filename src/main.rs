use dotenv::dotenv;
use libtor::{HiddenServiceVersion, Tor, TorAddress, TorFlag};
use rpc::RpcConfig;
use std::env;
mod database;
pub use database::{Db, DbError, Payment};
mod client;
mod rpc;
use client::start_client_flow;

#[tokio::main]
async fn main() {
    dotenv().ok();

    println!("Generating hashed control password...");
    let password = env::var("CONTROL_PASSWORD").unwrap_or("password1234_".into());
    let hashed_password = "16:281EC5644A4F548A60D50A0DD4DF835FFD50EDED062FD270D7269943DA";
    let control_port: u16 = env::var("CONTROL_PORT")
        .unwrap_or("7781".into())
        .parse()
        .unwrap();

    let rpc_config = RpcConfig {
        addr: format!("127.0.0.1:{}", control_port),
        rpc_password: password.clone(),
        command: "".into(),
    };
    tokio::spawn(async move {
        start_client_flow(rpc_config).await
    });

    println!("Starting Tor...");
    let tor = Tor::new()
        .flag(TorFlag::DataDirectory("./tmp/tor-rust".into()))
        .flag(TorFlag::SocksPort(18057))
        .flag(TorFlag::ControlPort(control_port.into()))
        .flag(TorFlag::HashedControlPassword(
            hashed_password.trim().into(),
        ))
        .flag(TorFlag::HiddenServiceDir("./tmp/tor-rust/hs-dir".into()))
        .flag(TorFlag::HiddenServiceVersion(HiddenServiceVersion::V3))
        .flag(TorFlag::HiddenServicePort(
            TorAddress::Port(8000),
            None.into(),
        ))
        .flag(TorFlag::ConfigFile("torrc".into()))
        .start();
}
