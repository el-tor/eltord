use eltor::{start_client, start_relay};
use eltor::rpc::get_rpc_config_from_torrc;
use std::io::{self, Write};
use log::{info, error};

/// Example showing how to integrate eltord into an existing application
/// 
/// This example demonstrates:
/// - How to configure logging to stdout to see eltord's internal logs
/// - How to start eltord components in different modes
/// - How to integrate eltord into an existing application
/// 
/// To run with different log levels:
/// - RUST_LOG=debug cargo run --example integration    # Most verbose, shows all eltord internals
/// - RUST_LOG=info cargo run --example integration     # Default, shows key operations
/// - RUST_LOG=warn cargo run --example integration     # Warnings only
/// 
/// To run in different modes:
/// - ELTORD_MODE=client cargo run --example integration
/// - ELTORD_MODE=relay cargo run --example integration
/// - ELTORD_MODE=both cargo run --example integration
/// 
/// You'll see logs like:
/// - "Starting Client Flow..." / "Starting Relay Flow..."
/// - "Connecting to Tor control port at 127.0.0.1:9051..."
/// - "Created paid Circuit with ID: 123"
/// - "BOLT12 offer found in torrc config. Running in paid mode."
/// - etc.
#[tokio::main]
async fn main() {
    // Enable logging to stdout with info level and above
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs()
        .write_style(env_logger::WriteStyle::Always)
        .init();
    
    // Also flush stdout to ensure logs are visible immediately
    io::stdout().flush().unwrap();
    log::info!("Example: Integrating eltord into an existing application");
    
    // Your application setup
    println!("Setting up my application...");
    
    // Configure eltord
    let torrc_path = "torrc.client.prod".to_string();
    let password = Some("password1234_".to_string());

    println!("Loading eltord configuration...");
    if let Some(rpc_config) = get_rpc_config_from_torrc(&torrc_path, password).await {
        println!("RPC Config loaded successfully!");
        
        // Decide which mode based on your application logic
        let mode = std::env::var("ELTORD_MODE").unwrap_or_else(|_| "client".to_string());
        
        match mode.as_str() {
            "client" => {
                println!("Starting eltord in client mode...");
                let client_handle = start_client(&rpc_config).await;
                
                // Store the handle so we can wait for it
                tokio::spawn(async move {
                    if let Err(e) = client_handle.await {
                        error!("Client task failed: {:?}", e);
                    }
                });
                
                // Periodically log to show client is active
                tokio::spawn(async {
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
                        log::info!("Client mode: still running...");
                    }
                });
            }
            "relay" => {
                println!("Starting eltord in relay mode...");
                let relay_handle = start_relay(&rpc_config).await;
                
                // Store the handle so we can wait for it
                tokio::spawn(async move {
                    if let Err(e) = relay_handle.await {
                        error!("Relay task failed: {:?}", e);
                    }
                });
            }
            "both" => {
                println!("Starting eltord in both client and relay modes...");
                let rpc_config_client = rpc_config.clone();
                let rpc_config_relay = rpc_config.clone();
                
                tokio::spawn(async move {
                    start_client(&rpc_config_client).await;
                });
                
                tokio::spawn(async move {
                    start_relay(&rpc_config_relay).await;
                });
            }
            _ => {
                println!("Unknown mode: {}. Use 'client', 'relay', or 'both'", mode);
                return;
            }
        }
        
        // Your main application logic continues here
        println!("Eltord started! Running main application...");
        
        // Simulate some application work
        // for i in 1..=2 {
        //     println!("Application working... step {}/5", i);
        //     io::stdout().flush().unwrap(); // Ensure output is visible
        //     tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        // }
        
        // println!("Application completed. Eltord continues running in background.");
        io::stdout().flush().unwrap();
        
        // In a real application, you might:
        // - Handle signals for graceful shutdown
        // - Provide API endpoints to interact with eltord
        // - Monitor eltord status
        // - etc.
        
        // For this example, keep running for a bit longer
        println!("Keeping application alive for 30 more seconds...");
        io::stdout().flush().unwrap();
        
        // Keep the application running and periodically flush to ensure logs are visible
        for i in 1..=300 {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            if i % 5 == 0 {
                //println!("Still running... {} seconds remaining", 30 - i);
                io::stdout().flush().unwrap();
            }
        }
        
    } else {
        error!("Failed to load RPC config from torrc file: {}", torrc_path);
        error!("Make sure the torrc file exists and has proper ControlPort configuration.");
    }
    
    info!("Application shutting down.");
}
