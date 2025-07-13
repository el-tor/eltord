# Eltord Library Usage

Eltord can now be used both as a standalone binary and as a library in other Rust projects.

## Using as a Binary (unchanged)

```bash
# Run as relay (default)
cargo run

# Run as client
cargo run client

# Run with custom torrc file
cargo run client -f torrc.client.prod -pw password1234_

# Run relay with custom torrc file
cargo run relay -f torrc.relay.prod -pw password1234_
```

## Using as a Library

Add eltord to your `Cargo.toml`:

```toml
[dependencies]
eltor = { path = "path/to/eltord" }
# or if published to crates.io:
# eltor = "0.0.1"
```

### Basic Usage

```rust
use eltor::{init_and_run, run_with_args, parse_args, start_client, start_relay};
use eltor::types::RpcConfig;

#[tokio::main]
async fn main() {
    // Option 1: Use the full initialization (includes .env loading)
    init_and_run().await;
    
    // Option 2: Run with custom arguments
    let args = vec![
        "myapp".to_string(),
        "client".to_string(), 
        "-f".to_string(),
        "torrc.client.dev".to_string()
    ];
    run_with_args(args).await;
    
    // Option 3: Parse arguments manually and start components separately
    let args = std::env::args().collect::<Vec<String>>();
    let (mode, torrc_path, password) = parse_args(args);
    
    // Get RPC config from torrc file (you'll need to implement this part)
    // let rpc_config = get_rpc_config_from_torrc(&torrc_path, password).await.unwrap();
    
    // Start specific components
    // if mode == "client" {
    //     start_client(&rpc_config).await;
    // } else if mode == "relay" {
    //     start_relay(&rpc_config).await;
    // }
}
```

### Integration Example

```rust
use eltor::{start_client, start_relay};
use eltor::types::RpcConfig;
use eltor::rpc::get_rpc_config_from_torrc;
use log::{info, error}

#[tokio::main]
async fn main() {
    // Your application initialization
    info!("Starting my application with eltord integration...");
    
    // Configure eltord
    let torrc_path = "my_custom_torrc".to_string();
    let password = Some("my_password".to_string());
    
    if let Some(rpc_config) = get_rpc_config_from_torrc(&torrc_path, password).await {
        // Start eltord client in the background
        tokio::spawn(async move {
            start_client(&rpc_config).await;
        });
        
        // Your application logic continues here...
        info!("Eltord client started, continuing with main application...");
        
        // Keep the application running
        tokio::signal::ctrl_c().await.expect("Failed to listen for ctrl+c");
    } else {
        error!("Failed to load RPC config from torrc file");
    }
}
```

## Available Public Functions

- `init_and_run()` - Full initialization with .env support and argument parsing
- `run_with_args(args)` - Run with custom arguments  
- `parse_args(args)` - Parse command line arguments
- `start_client(rpc_config)` - Start only the client component
- `start_relay(rpc_config)` - Start only the relay component

## Public Modules

- `eltor::client` - Client functionality
- `eltor::relay` - Relay functionality  
- `eltor::rpc` - RPC client for Tor communication
- `eltor::types` - Common types and configurations
- `eltor::lightning` - Lightning network integration
- `eltor::database` - Database utilities
- `eltor::utils` - Utility functions
