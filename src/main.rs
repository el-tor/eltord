use dotenv::dotenv;
use libtor::{HiddenServiceVersion, Tor, TorAddress, TorFlag};
use std::env;
mod client;
mod database;
mod lightning;
mod relay;
mod rpc;
mod types;
mod utils;
use std::env::args;
use types::RpcConfig;

#[tokio::main]
async fn main() {
    dotenv().ok();

    // Parse command-line arguments for the optional -f flag
    let mut args = args().skip(1); // Skip the first argument (program name)
    let mut torrc_path = "torrc".to_string(); // Default value
                                              // read from env?
    if let Ok(torrc_env) = env::var("TORRC_DEV") {
        torrc_path = torrc_env;
    }
    // read from args?
    while let Some(arg) = args.next() {
        if arg == "-f" {
            if let Some(path) = args.next() {
                torrc_path = path;
            } else {
                eprintln!("Error: Missing value for -f flag");
                std::process::exit(1);
            }
        }
    }

    println!("Using torrc file: {}", torrc_path);

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
    // let rpc_config2 = rpc_config.clone();
    tokio::spawn(async move { client::start_client_flow(&rpc_config.clone()).await });
    // backup circuit
    // tokio::spawn(async move { client::start_client_flow(&rpc_config2).await });

    //tokio::spawn(async move { relay::start_relay_flow(&rpc_config2.clone()).await });

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
            TorAddress::Port(4747),
            None.into(),
        ))
        .flag(TorFlag::ConfigFile(torrc_path))
        .start();
}
