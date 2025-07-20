use eltor::{EltordProcessManager, ProcessCommand};
use log::error;
use std::time::Duration;

/// Example usage of the process manager from the eltor library
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs()
        .write_style(env_logger::WriteStyle::Always)
        .init();

    println!("=== EltorD Process Manager Example ===\n");
    
    // Create the process manager
    let (mut manager, command_sender, mut status_receiver) = EltordProcessManager::new();

    // We'll track the PID from status updates
    let current_pid = std::sync::Arc::new(std::sync::Mutex::new(None::<u32>));
    let pid_clone = current_pid.clone();

    // Start the manager in a background task
    let manager_handle = tokio::spawn(async move {
        if let Err(e) = manager.run().await {
            error!("Process manager error: {}", e);
        }
    });

    // Start a task to listen for status updates
    let status_handle = tokio::spawn(async move {
        while let Some(status) = status_receiver.recv().await {
            println!("📊 Status Update: {:?}", status);
            
            // Demonstrate PID access and update tracked PID
            if let Some(pid) = status.pid() {
                println!("   └── Process PID: {}", pid);
                *pid_clone.lock().unwrap() = Some(pid);
            } else {
                *pid_clone.lock().unwrap() = None;
            }
            if let Some(mode) = status.mode() {
                println!("   └── Running mode: {}", mode);
            }
        }
    });

    // Example: Demonstrate process management
    println!("🚀 Starting eltord in client mode...");
    command_sender.send(ProcessCommand::Start {
        mode: "client".to_string(),
        torrc_path: "torrc.client.dev".to_string(),
        password: "password1234_".to_string(),
    }).await?;

    // Wait a bit
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Check status
    println!("📋 Checking status...");
    command_sender.send(ProcessCommand::Status).await?;

    // Check tracked PID from status updates
    if let Some(pid) = *current_pid.lock().unwrap() {
        println!("🔍 Current PID from status tracking: {}", pid);
    } else {
        println!("❌ No process currently running");
    }

    // Wait a bit more
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Restart in relay mode
    println!("🔄 Restarting in relay mode...");
    command_sender.send(ProcessCommand::Restart {
        mode: "relay".to_string(),
        torrc_path: "torrc.relay.dev".to_string(),
        password: "password1234_".to_string(),
    }).await?;

    // Wait a bit
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Stop the process
    println!("🛑 Stopping process...");
    command_sender.send(ProcessCommand::Stop).await?;

    // Wait for cleanup
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Final status check
    println!("📋 Final status check...");
    command_sender.send(ProcessCommand::Status).await?;

    // Close the command channel to shut down the manager
    drop(command_sender);

    // Wait for tasks to complete
    let _ = tokio::try_join!(manager_handle, status_handle);

    println!("\n✅ Process manager example completed!");
    Ok(())
}
