## To Run examples

```bash
# Run custom usage example with client mode
cargo run --package eltor --example client -- client -f torrc.client.prod --pw password1234_

# Run process manager example
cargo run --package eltor --example manager
```

## Manager Example

The `manager.rs` example provides external process control for eltord. It demonstrates:

- Starting eltord process with different configurations (client/relay/both modes)
- Monitoring process status and output
- Gracefully stopping processes
- Handling process crashes and restarts
- External control through command channels

### Key Features

- **Process Control**: Start, stop, restart eltord processes
- **Status Monitoring**: Real-time status updates and process health checks
- **Output Capture**: Capture and log stdout/stderr from the eltord process
- **Graceful Shutdown**: Proper cleanup when terminating processes
- **External API**: Channel-based communication for integration with other applications

### Usage in Your Application

```rust
use eltor::{EltordProcessManager, ProcessCommand, ProcessStatus};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create the process manager
    let (mut manager, command_sender, mut status_receiver) = EltordProcessManager::new();

    // Start the manager in a background task
    let manager_handle = tokio::spawn(async move {
        manager.run().await
    });

    // Start eltord in client mode
    command_sender.send(ProcessCommand::Start {
        mode: "client".to_string(),
        torrc_path: "torrc.client.dev".to_string(),
        password: "password1234_".to_string(),
    }).await?;

    // Listen for status updates
    tokio::spawn(async move {
        while let Some(status) = status_receiver.recv().await {
            println!("Status: {:?}", status);
        }
    });

    // Wait a bit then stop
    tokio::time::sleep(Duration::from_secs(10)).await;
    command_sender.send(ProcessCommand::Stop).await?;

    // Clean shutdown
    drop(command_sender);
    let _ = manager_handle.await;

    Ok(())
}
```
