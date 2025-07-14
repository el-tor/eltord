// Simple integration example that demonstrates eltord library usage
// without requiring a Lightning node connection
//
// Usage:
// cargo run --example simple_integration

use eltor::get_rpc_config_from_torrc;
use log::info;
use std::io::{self, Write};
use tokio::time::Duration;

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
    info!("Simple Integration Example: Testing eltord logging without Lightning node");
    
    // Your application setup
    println!("Setting up my application...");
    
    // Configure eltord
    let torrc_path = "torrc".to_string();
    let password = None; // Use cookie authentication or set a password
    
    println!("Loading eltord configuration...");
    if let Some(rpc_config) = get_rpc_config_from_torrc(&torrc_path, password).await {
        println!("RPC Config loaded successfully!");
        
        // Test basic RPC connection (this should work even without Lightning)
        info!("Testing Tor RPC connection...");
        
        // Just test the config parsing without starting full client/relay
        println!("RPC Config: {:?}", rpc_config);
        
        // Run for a few seconds to see if we get any eltord logs
        for i in 1..=5 {
            info!("Integration heartbeat {}/5", i);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        
        println!("Simple integration test completed!");
    } else {
        println!("Failed to load RPC config from torrc file: {}", torrc_path);
        println!("Make sure the torrc file exists and has proper ControlPort configuration");
    }
}
