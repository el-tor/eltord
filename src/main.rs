use dotenv::dotenv;
use libtor::{HiddenServiceVersion, Tor, TorAddress, TorFlag};
use rpc::get_rpc_config_from_torrc;
use std::env::{self, Args};
mod client;
mod database;
mod lightning;
mod relay;
mod rpc;
mod types;
mod utils;
use std::env::args;
use types::RpcConfig;

// Main Args:
//  Parse command-line arguments for mode ("client" or "relay") and optional -f flag
//  example usage: cargo run
//  example usage: cargo run relay
//  example usage: cargo run client
//  exanple: cargo run client -f torrc.client.dev -pw password1234_
//  example: cargo run relay -f torrc.relay.dev -pw password1234_
#[tokio::main]
async fn main() {
    dotenv().ok();
    // Check if ARGS are set in .env, and use it if present such as:
    // ARGS="eltord client -f torrc.client.dev -pw password1234_"
    // ARGS="eltord relay -f torrc.relay.dev -pw password1234_"
    let env_args = env::var("ARGS").ok();
    dbg!(env_args.clone());
    let args: Vec<String> = if let Some(env_args) = env_args {
        env_args.split_whitespace().map(|s| s.to_string()).collect()
    } else {
        std::env::args().collect()
    };
    dbg!(args.clone());
    main_run_with_args(args).await;
}

pub async fn main_run_with_args<I, S>(args: I)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    println!("PID: {}", std::process::id());
    println!("Press Enter to continue...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    
    let (mode, torrc_path, control_port_password) = parse_args(args.into_iter().map(Into::into));
    dbg!(mode.clone());
    let rpc_config = get_rpc_config_from_torrc(&torrc_path, control_port_password).await;
    dbg!(rpc_config.clone());
    if rpc_config.is_none() {
        eprintln!("Error: Could not load rpc_config from torrc file. Be sure to configure the following settings in the torrc file here '{}': ControlPort, Address, and (HashedControlPassword or CookieAuthentication) ", torrc_path);
        std::process::exit(1);
    }
    let rpc_config = rpc_config.unwrap();
    let rpc_config_relay = rpc_config.clone();

    if mode == "client" || mode.is_empty() {
        println!("Starting Client Flow...");
        tokio::spawn(async move { client::start_client_flow(&rpc_config.clone()).await });
        // backup circuit
        // tokio::spawn(async move { client::start_client_flow(&rpc_config).await });
    }

    if mode == "relay" || mode.is_empty() {
        println!("Starting Relay Flow...");
        tokio::spawn(async move { relay::start_relay_flow(&rpc_config_relay.clone()).await });
    }

    println!("Starting Tor...");
    let tor = Tor::new().flag(TorFlag::ConfigFile(torrc_path)).start();
}

fn parse_args<I>(args: I) -> (String, String, Option<String>)
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter().skip(1); // Skip program name such as eltord
    let mut mode = "relay".to_string(); // Default mode is relay
    let mut torrc_path = "torrc".to_string(); // Default torrc path is in same folder as eltord binary named torrc
    let mut control_port_password: Option<String> = None;

    // Check if first argument is "client" or "relay"
    if let Some(arg1) = args.next() {
        dbg!(arg1.clone());
        if arg1 == "client" || arg1 == "relay" {
            mode = arg1;
        } else if arg1 == "-f" {
            // Handle "cargo run -f torrc"
            if let Some(path) = args.next() {
                torrc_path = path;
            } else {
                eprintln!("Error: Missing value for -f flag");
                std::process::exit(1);
            }
        }
    }

    // Parse remaining args for -f flag
    while let Some(arg) = args.next() {
        if arg == "-f" {
            if let Some(path) = args.next() {
                torrc_path = path;
            }
        }
        if arg == "-pw" {
            if let Some(password) = args.next() {
                control_port_password = Some(password);
            }
        }
    }

    println!("Using torrc file: {} in mode {}", torrc_path, mode);
    (mode, torrc_path, control_port_password)
}
