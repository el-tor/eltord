use dotenv::dotenv;
use env_logger;
use libtor::{Tor, TorFlag};
use std::env;
use log::{info, debug, warn, error};
use tokio::task::JoinHandle;

pub mod client;
pub mod database;
pub mod lightning;
pub mod relay;
pub mod rpc;
pub mod types;
pub mod utils;

use types::RpcConfig;

// Re-export commonly used functions for library consumers
pub use rpc::get_rpc_config_from_torrc;

/// Main entry point for running eltord with provided arguments
/// 
/// # Arguments
/// 
/// * `args` - Iterator of arguments (typically from command line)
/// 
/// # Example
/// 
/// ```rust
/// use eltor::run_with_args;
/// 
/// #[tokio::main]
/// async fn main() {
///     let args = vec!["eltor".to_string(), "client".to_string(), "-f".to_string(), "torrc.client.dev".to_string()];
///     run_with_args(args).await;
/// }
/// ```
pub async fn run_with_args<I, S>(args: I)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    info!("PID: {}", std::process::id());
    //println!("Press Enter to continue...");
    //let mut input = String::new();
    // std::io::stdin().read_line(&mut input).unwrap();

    let (mode, torrc_path, control_port_password) = parse_args(args.into_iter().map(Into::into));
    info!("Mode: {:?}", mode);
    let rpc_config = self::get_rpc_config_from_torrc(&torrc_path, control_port_password).await;
    info!("RPC Config: {:?}", rpc_config);
    if rpc_config.is_none() {
        error!("Error: Could not load rpc_config from torrc file. Be sure to configure the following settings in the torrc file here '{}': ControlPort, Address, and (HashedControlPassword or CookieAuthentication) ", torrc_path);
        std::process::exit(1);
    }
    let rpc_config = rpc_config.unwrap();
    // let rpc_config_2 = rpc_config.clone();
    let rpc_config_relay = rpc_config.clone();

    info!("Starting Tor...");
    let torrc_path_clone = torrc_path.clone();
    let _tor = tokio::task::spawn_blocking(move || {
        Tor::new().flag(TorFlag::ConfigFile(torrc_path_clone)).start()
    });
    
    // Give Tor a moment to start up before trying to connect
    info!("Waiting for Tor to initialize...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    let mut tasks = Vec::new();

    if mode == "client" || mode.is_empty() {
        info!("Starting Client Flow...");
        let client_handle = client::start_client_flow(&rpc_config.clone()).await;
        tasks.push(client_handle);
        // backup circuit
        // tokio::spawn(async move { client::start_client_flow(&rpc_config_2).await });
    }

    if mode == "relay" || mode.is_empty() {
        info!("Starting Relay Flow...");
        let relay_handle = relay::start_relay_flow(&rpc_config_relay.clone()).await;
        tasks.push(relay_handle);
    }

    // Wait for all tasks to complete (they run indefinitely)
    for task in tasks {
        if let Err(e) = task.await {
            info!("Task completed with error: {:?}", e);
        }
    }
}

/// Initialize eltord with environment variables and arguments
/// 
/// This function handles:
/// - Initializing the logger for binary execution
/// - Loading environment variables from .env file
/// - Parsing ARGS environment variable if set
/// - Falling back to command line arguments
/// 
/// # Example
/// 
/// ```rust
/// use eltor::init_and_run;
/// 
/// #[tokio::main]
/// async fn main() {
///     init_and_run().await;
/// }
/// ```
pub async fn init_and_run() {
    // Initialize logger for binary execution (library users handle their own logging)
    // Enable logging to stdout with info level and above
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs()
        .write_style(env_logger::WriteStyle::Always)
        .init();
    
    dotenv().ok();
    // Check if ARGS are set in .env, and use it if present such as:
    // ARGS="eltord client -f torrc.client.dev -pw password1234_"
    // ARGS="eltord relay -f torrc.relay.dev -pw password1234_"
    let env_args = env::var("ARGS").ok();
    info!("Environment args: {:?}", env_args);
    let args: Vec<String> = if let Some(env_args) = env_args {
        env_args.split_whitespace().map(|s| s.to_string()).collect()
    } else {
        std::env::args().collect()
    };
    info!("Parsed args: {:?}", args);
    run_with_args(args).await;
}

/// Parse command line arguments
/// 
/// # Arguments
/// 
/// * `args` - Iterator of string arguments
/// 
/// # Returns
/// 
/// A tuple containing (mode, torrc_path, control_port_password)
/// 
/// # Examples
/// 
/// ```rust
/// use eltor::parse_args;
/// 
/// let args = vec!["eltor".to_string(), "client".to_string(), "-f".to_string(), "torrc.client.dev".to_string()];
/// let (mode, torrc_path, password) = parse_args(args);
/// assert_eq!(mode, "client");
/// assert_eq!(torrc_path, "torrc.client.dev");
/// ```
pub fn parse_args<I>(args: I) -> (String, String, Option<String>)
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter().skip(1); // Skip program name such as eltord
    let mut mode = "relay".to_string(); // Default mode is relay
    let mut torrc_path = "torrc".to_string(); // Default torrc path is in same folder as eltord binary named torrc
    let mut control_port_password: Option<String> = None;

    // Check if first argument is "client" or "relay"
    if let Some(arg1) = args.next() {
        info!("First argument: {:?}", arg1);
        if arg1 == "client" || arg1 == "relay" {
            mode = arg1;
        } else if arg1 == "-f" {
            // Handle "cargo run -f torrc"
            if let Some(path) = args.next() {
                torrc_path = path;
            } else {
                error!("Error: Missing value for -f flag");
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

    info!("Using torrc file: {} in mode {}", torrc_path, mode);
    (mode, torrc_path, control_port_password)
}

/// Start the client flow with the given RPC configuration
/// 
/// # Arguments
/// 
/// * `rpc_config` - RPC configuration for connecting to Tor
/// 
/// # Example
/// 
/// ```rust
/// use eltor::{start_client, types::RpcConfig};
/// 
/// #[tokio::main]
/// async fn main() {
///     let rpc_config = RpcConfig {
///         // ... configure your RPC settings
///         ..Default::default()
///     };
///     start_client(&rpc_config).await;
/// }
/// ```
pub async fn start_client(rpc_config: &RpcConfig) -> tokio::task::JoinHandle<()> {
    client::start_client_flow(rpc_config).await
}

/// Start the relay flow with the given RPC configuration
/// 
/// # Arguments
/// 
/// * `rpc_config` - RPC configuration for connecting to Tor
/// 
/// # Example
/// 
/// ```rust
/// use eltor::{start_relay, types::RpcConfig};
/// 
/// #[tokio::main]
/// async fn main() {
///     let rpc_config = RpcConfig {
///         // ... configure your RPC settings
///         ..Default::default()
///     };
///     start_relay(&rpc_config).await;
/// }
/// ```
pub async fn start_relay(rpc_config: &RpcConfig) -> tokio::task::JoinHandle<()> {
    relay::start_relay_flow(rpc_config).await
}

/// Task management for spawned eltord flows
pub struct EltordTasks {
    pub client_task: Option<JoinHandle<()>>,
    pub relay_task: Option<JoinHandle<()>>,
}

impl EltordTasks {
    pub fn new() -> Self {
        Self {
            client_task: None,
            relay_task: None,
        }
    }

    /// Wait for all spawned tasks to complete
    pub async fn join_all(self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(client) = self.client_task {
            if let Err(e) = client.await {
                warn!("Client task failed: {:?}", e);
            }
        }
        if let Some(relay) = self.relay_task {
            if let Err(e) = relay.await {
                warn!("Relay task failed: {:?}", e);
            }
        }
        Ok(())
    }

    /// Abort all spawned tasks
    pub fn abort_all(&self) {
        if let Some(ref client) = self.client_task {
            client.abort();
        }
        if let Some(ref relay) = self.relay_task {
            relay.abort();
        }
    }
}

/// Start eltord flows and return task handles for proper management
/// This allows library consumers to control task lifecycle and ensure logs aren't lost
pub async fn start_with_task_management(args: impl Iterator<Item = impl Into<String>>) -> Result<EltordTasks, Box<dyn std::error::Error>> {
    let (mode, torrc_path, control_port_password) = parse_args(args.into_iter().map(Into::into));
    info!("Mode: {:?}", mode);
    let rpc_config = self::get_rpc_config_from_torrc(&torrc_path, control_port_password).await;
    info!("RPC Config: {:?}", rpc_config);
    if rpc_config.is_none() {
        return Err("Could not load rpc_config from torrc file".into());
    }
    let rpc_config = rpc_config.unwrap();

    info!("Starting Tor...");
    let torrc_path_clone = torrc_path.clone();
    let _tor = tokio::task::spawn_blocking(move || {
        Tor::new().flag(TorFlag::ConfigFile(torrc_path_clone)).start()
    });
    
    // Give Tor a moment to start up before trying to connect
    info!("Waiting for Tor to initialize...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    let mut tasks = EltordTasks::new();

    if mode == "client" || mode.is_empty() {
        info!("Starting Client Flow...");
        let rpc_config_client = rpc_config.clone();
        tasks.client_task = Some(client::start_client_flow(&rpc_config_client).await);
    }

    if mode == "relay" || mode.is_empty() {
        info!("Starting Relay Flow...");
        let rpc_config_relay = rpc_config.clone();
        tasks.relay_task = Some(relay::start_relay_flow(&rpc_config_relay).await);
    }

    Ok(tasks)
}
