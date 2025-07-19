# Eltord Library Usage

Eltord can now be used both as a standalone binary and as a library in other Rust projects.

## Using as a Binary

```bash
# Run as relay (default)
cargo run

# Run as client
cargo run client

# Run with custom torrc file
cargo run client -f torrc.client.prod -pw password1234_

# Run relay with custom torrc file
cargo run relay -f torrc.relay.prod -pw password1234_

# Run both client and relay
cargo run both -f torrc.relay.prod -pw password1234_

# Run with Env var args
ARGS="eltord both -f torrc.relay.prod -pw password1234_" cargo run

```

## Using as a Library

Add eltord to your `Cargo.toml`:

```toml
[dependencies]
eltor = { path = "path/to/eltord" }
# or if published to crates.io:
# eltor = "0.0.1"
```

### Basic Lib Usage

```rust
use eltor::{init_and_run, run_with_args, parse_args, start_client, start_relay};
use eltor::types::RpcConfig;

#[tokio::main]
async fn main() {
    // Turn on logging
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs()
        .write_style(env_logger::WriteStyle::Always)
        .init();

    // Run with arguments
    let args = vec![
        "eltord".to_string(),
        "both".to_string(), // relay, client or both
        "-f".to_string(),
        "torrc.relay.prod".to_string(), // torrc_path_filename
        "-pw".to_string(),
        "password1234_".to_string(), // control port password
    ];
    run_with_args(args).await;
    
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
