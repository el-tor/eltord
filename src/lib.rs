//! # Eltor - Enhanced Tor with Paid Relays
//! 
//! Eltor is a Tor network fork that adds paid relay functionality and Lightning Network integration.
//! 
//! ## Key Features
//! 
//! - **Paid Circuits**: Pay Lightning Network invoices for premium relay service
//! - **Client Mode**: Connect as a client to use paid relay circuits
//! - **Relay Mode**: Run as a paid relay to earn from providing service
//! - **Both Mode**: Run as both client and relay simultaneously
//! - **Process Management**: External process control for integration with other applications
//! 
//! ## Quick Start
//! 
//! ### Simple Usage
//! 
//! ```rust
//! use eltor::init_and_run;
//! 
//! #[tokio::main]
//! async fn main() {
//!     // Initialize logging and run with environment/CLI args
//!     init_and_run().await;
//! }
//! ```
//! 
//! ### Manual Control
//! 
//! ```rust
//! use eltor::{initialize_eltord, start_client, start_relay};
//! 
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let args = vec!["eltor", "client", "-f", "torrc.client.dev"];
//!     let (rpc_config, mode) = initialize_eltord(args.into_iter()).await?;
//!     
//!     // Start client flow
//!     let client_task = start_client(&rpc_config).await;
//!     
//!     // Wait for completion
//!     client_task.await?;
//!     Ok(())
//! }
//! ```
//! 
//! ### Process Management
//! 
//! For external applications that need to control the eltord process:
//! 
//! ```rust
//! use eltor::{EltordProcessManager, ProcessCommand};
//! use std::time::Duration;
//! 
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create process manager
//!     let (mut manager, command_sender, mut status_receiver) = EltordProcessManager::new();
//!     
//!     // Start manager in background
//!     let manager_handle = tokio::spawn(async move {
//!         manager.run().await
//!     });
//!     
//!     // Start eltord process
//!     command_sender.send(ProcessCommand::Start {
//!         mode: "client".to_string(),
//!         torrc_path: "torrc.client.dev".to_string(),
//!         password: "password123".to_string(),
//!     }).await?;
//!     
//!     // Monitor status updates
//!     tokio::spawn(async move {
//!         while let Some(status) = status_receiver.recv().await {
//!             println!("Status: {:?}", status);
//!         }
//!     });
//!     
//!     // Stop after some time
//!     tokio::time::sleep(Duration::from_secs(30)).await;
//!     command_sender.send(ProcessCommand::Stop).await?;
//!     
//!     // Clean shutdown
//!     drop(command_sender);
//!     let _ = manager_handle.await;
//!     
//!     Ok(())
//! }
//! ```
//! 
//! ## Configuration
//! 
//! Eltor uses Tor configuration files (torrc) with additional Lightning Network settings.
//! See the examples in the repository for sample configurations.

use dotenv::dotenv;
use libtor::{Tor, TorFlag};
use std::env;
use log::{info, warn, error};
use tokio::task::JoinHandle;
use std::sync::atomic::{AtomicI32, Ordering};
#[cfg(windows)]
use std::sync::atomic::AtomicU32;
#[cfg(unix)]
extern crate libc;

// Global variables to track the Tor child process for cleanup
static TOR_CHILD_PID: AtomicI32 = AtomicI32::new(0);  // Unix PID
#[cfg(windows)]
static TOR_CHILD_PROCESS_ID: AtomicU32 = AtomicU32::new(0);  // Windows Process ID

/// Clean up any spawned Tor processes
#[cfg(unix)]
fn cleanup_tor_processes() {
    let child_pid = TOR_CHILD_PID.load(Ordering::SeqCst);
    if child_pid > 0 {
        info!("Cleaning up Tor child process with PID: {}", child_pid);
        unsafe {
            // Send SIGTERM first (graceful shutdown)
            if libc::kill(child_pid, libc::SIGTERM) == 0 {
                info!("Sent SIGTERM to Tor process {}", child_pid);
                // Wait a bit for graceful shutdown
                std::thread::sleep(std::time::Duration::from_secs(2));
                
                // Check if process still exists
                if libc::kill(child_pid, 0) == 0 {
                    // Still running, force kill
                    info!("Tor process {} still running, sending SIGKILL", child_pid);
                    libc::kill(child_pid, libc::SIGKILL);
                }
            } else {
                warn!("Failed to send signal to Tor process {} (may already be dead)", child_pid);
            }
        }
        TOR_CHILD_PID.store(0, Ordering::SeqCst);
    }
}

#[cfg(not(unix))]
fn cleanup_tor_processes() {
    #[cfg(windows)]
    {
        let child_process_id = TOR_CHILD_PROCESS_ID.load(Ordering::SeqCst);
        if child_process_id > 0 {
            info!("Cleaning up Tor child process with Process ID: {}", child_process_id);
            
            // Use Windows API to terminate the process
            use std::process::Command;
            
            // Try taskkill first (graceful)
            let result = Command::new("taskkill")
                .args(&["/PID", &child_process_id.to_string(), "/T"])
                .output();
                
            match result {
                Ok(output) => {
                    if output.status.success() {
                        info!("Successfully terminated Tor process {} with taskkill", child_process_id);
                    } else {
                        warn!("taskkill failed, trying force termination...");
                        // Force kill if graceful termination failed
                        let force_result = Command::new("taskkill")
                            .args(&["/PID", &child_process_id.to_string(), "/T", "/F"])
                            .output();
                        
                        match force_result {
                            Ok(force_output) => {
                                if force_output.status.success() {
                                    info!("Force terminated Tor process {}", child_process_id);
                                } else {
                                    error!("Failed to force terminate Tor process {}", child_process_id);
                                }
                            },
                            Err(e) => {
                                error!("Error executing taskkill /F: {:?}", e);
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Error executing taskkill: {:?}", e);
                }
            }
            
            TOR_CHILD_PROCESS_ID.store(0, Ordering::SeqCst);
        }
    }
    
    #[cfg(not(any(unix, windows)))]
    {
        // Other platforms - no process control available
        warn!("Process cleanup not implemented for this platform");
    }
}

/// Setup signal handlers to cleanup processes on exit
fn setup_signal_handlers() {
    use std::sync::atomic::AtomicBool;
    
    // Global flag to ensure handler is only set once
    static HANDLER_SET: AtomicBool = AtomicBool::new(false);
    
    // Only set handler if it hasn't been set already
    if HANDLER_SET.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
        ctrlc::set_handler(move || {
            info!("Received interrupt signal, cleaning up...");
            cleanup_tor_processes();
            std::process::exit(0);
        }).expect("Error setting Ctrl-C handler");
        info!("Signal handlers set up successfully");
    }
}

/// Start Tor in a child process to isolate C library crashes
/// This protects the main application from SIGSEGV and other C-level crashes
fn start_tor_in_child_process(torrc_path: String, process_name: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    
    // Global static to prevent multiple simultaneous Tor starts across all functions
    static TOR_STARTING_GLOBAL: AtomicBool = AtomicBool::new(false);
    
    // Prevent multiple simultaneous Tor starts (mobile-safe)
    if TOR_STARTING_GLOBAL.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        info!("{} startup already in progress, waiting...", process_name);
        std::thread::sleep(std::time::Duration::from_millis(500));
        return;
    }
    
    // Use process isolation on Unix platforms, fallback to panic catching on others
    #[cfg(unix)]
    {
        // Fork a child process to isolate C library crashes (Unix/Linux/macOS only)
        unsafe {
            let pid = libc::fork();
            
            if pid == -1 {
                error!("Failed to fork child process for {}", process_name);
                TOR_STARTING_GLOBAL.store(false, Ordering::SeqCst);
                return;
            } else if pid == 0 {
                // Child process - attempt to start Tor
                // If this crashes, only the child process dies
                match Tor::new().flag(TorFlag::ConfigFile(torrc_path.clone())).start() {
                    Ok(_tor) => {
                        info!("Tor started successfully in child process ({})", process_name);
                        // Keep the child process alive to maintain Tor
                        loop {
                            std::thread::sleep(std::time::Duration::from_secs(1));
                        }
                    },
                    Err(e) => {
                        error!("Failed to start Tor in child process ({}): {:?}", process_name, e);
                        libc::exit(1);
                    }
                }
            } else {
                // Parent process - wait for child to start Tor
                info!("{} starting in child process with PID: {}", process_name, pid);
                
                // Store the child PID for cleanup
                TOR_CHILD_PID.store(pid, Ordering::SeqCst);
                
                // Wait a moment for Tor to initialize
                std::thread::sleep(std::time::Duration::from_secs(2));
                
                // Check if child process is still alive
                let mut status: libc::c_int = 0;
                let wait_result = libc::waitpid(pid, &mut status as *mut libc::c_int, libc::WNOHANG);
                
                if wait_result == 0 {
                    info!("Child {} process is running successfully", process_name);
                } else {
                    error!("Child {} process exited with status: {}", process_name, status);
                }
                
                TOR_STARTING_GLOBAL.store(false, Ordering::SeqCst);
            }
        }
    }
    
    #[cfg(windows)]
    {
        // Windows - use CreateProcess for process isolation
        use std::process::{Command, Stdio};
        use std::env;
        
        info!("Starting {} with process isolation (Windows mode)", process_name);
        
        // Get current executable path
        let current_exe = env::current_exe().unwrap_or_else(|_| "eltor.exe".into());
        
        // Start Tor in a separate process
        let mut child = Command::new(&current_exe)
            .arg("--tor-subprocess") // Special flag to indicate subprocess mode
            .arg(&torrc_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
            
        match child {
            Ok(mut process) => {
                let process_id = process.id();
                info!("{} started in child process with PID: {:?}", process_name, process_id);
                
                // Store the child process ID for cleanup
                TOR_CHILD_PROCESS_ID.store(process_id, Ordering::SeqCst);
                
                // Wait a moment for Tor to initialize
                std::thread::sleep(std::time::Duration::from_secs(2));
                
                // Check if child process is still alive
                match process.try_wait() {
                    Ok(Some(status)) => {
                        error!("Child {} process exited with status: {:?}", process_name, status);
                    },
                    Ok(None) => {
                        info!("Child {} process is running successfully", process_name);
                        // Detach the child process so it can continue running
                        std::mem::forget(process);
                    },
                    Err(e) => {
                        error!("Error checking child {} process status: {:?}", process_name, e);
                    }
                }
            },
            Err(e) => {
                error!("Failed to start child process for {}: {:?}", process_name, e);
            }
        }
        
        TOR_STARTING_GLOBAL.store(false, Ordering::SeqCst);
    }
    
    #[cfg(not(any(unix, windows)))]
    {
        // Other platforms - use panic catching instead of process isolation
        info!("Starting {} with panic protection (fallback mode)", process_name);
        
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match Tor::new().flag(TorFlag::ConfigFile(torrc_path.clone())).start() {
                Ok(_tor) => {
                    info!("Tor started successfully ({})", process_name);
                    // Keep Tor running
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                },
                Err(e) => {
                    error!("Failed to start Tor ({}): {:?}", process_name, e);
                }
            }
        }));
        
        TOR_STARTING_GLOBAL.store(false, Ordering::SeqCst);
        
        match result {
            Ok(_) => {
                info!("Tor startup completed successfully ({})", process_name);
            },
            Err(panic_info) => {
                error!("Tor startup panicked ({}): {:?}", process_name, panic_info);
                // Continue execution despite panic
            }
        }
    }
}

pub mod client;
pub mod database;
pub mod lightning;
pub mod manager;
pub mod relay;
pub mod rpc;
pub mod types;
pub mod utils;

// Re-export commonly used functions for library consumers
pub use rpc::get_rpc_config_from_torrc;
pub use types::RpcConfig;

// Re-export process manager for external applications
pub use manager::{EltordProcessManager, ProcessCommand, ProcessStatus};

// Logging macros with prefixes for easy identification
#[macro_export]
macro_rules! client_info {
    ($($arg:tt)*) => {
        log::info!("[CLIENT] {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! client_debug {
    ($($arg:tt)*) => {
        log::debug!("[CLIENT] {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! client_warn {
    ($($arg:tt)*) => {
        log::warn!("[CLIENT] {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! client_error {
    ($($arg:tt)*) => {
        log::error!("[CLIENT] {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! relay_info {
    ($($arg:tt)*) => {
        log::info!("[RELAY] {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! relay_debug {
    ($($arg:tt)*) => {
        log::debug!("[RELAY] {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! relay_warn {
    ($($arg:tt)*) => {
        log::warn!("[RELAY] {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! relay_error {
    ($($arg:tt)*) => {
        log::error!("[RELAY] {}", format!($($arg)*))
    };
}

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
        start_tor_in_child_process(torrc_path_clone, "Tor");
    });
    
    // Give Tor a moment to start up before trying to connect
    info!("Waiting for Tor to initialize...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    let mut tasks = Vec::new();

    if mode == "client" {
        info!("[CLIENT] Starting Client-only Flow...");
        let client_handle = client::start_client_flow(&rpc_config.clone()).await;
        tasks.push(client_handle);
    } else if mode == "both" {
        info!("[RELAY] Starting both Client + Relay Flows...");
        // Relay mode runs both client and relay flows
        info!("[CLIENT] Starting Client Flow (relay acts as client too)...");
        let client_handle = client::start_client_flow(&rpc_config_relay.clone()).await;
        tasks.push(client_handle);
        info!("[RELAY] Starting Relay Flow...");
        let relay_handle = relay::start_relay_flow(&rpc_config_relay.clone()).await;
        tasks.push(relay_handle);
    } else if mode == "relay" {
        // Default case - should not happen with current parsing
        info!("[DEFAULT] Starting Relay Flow...");
        let relay_handle = relay::start_relay_flow(&rpc_config_relay.clone()).await;
        tasks.push(relay_handle);
    } else {
        error!("Unknown mode: {}. Use 'client', 'relay', or 'both'", mode);
        std::process::exit(1);
    }

    // Wait for all tasks to complete (they run indefinitely)
    for task in tasks {
        if let Err(e) = task.await {
            info!("Task completed with error: {:?}", e);
        }
    }
    
    // Clean up any spawned processes before exit
    cleanup_tor_processes();
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
    dotenv().ok();
    
    // Set up signal handlers for cleanup
    setup_signal_handlers();
    
    // Check if ARGS are set in .env, and use it if present such as:
    // ARGS="eltord client -f torrc.client.dev -pw password1234_"
    // ARGS="eltord relay -f torrc.relay.dev -pw password1234_"
    // ARGS="eltord both -f torrc.relay.dev -pw password1234_"
    let env_args = env::var("ARGS").ok();
    dbg!(env_args.clone());
    info!("Environment args: {:?}", env_args);
    let args: Vec<String> = if let Some(env_args) = env_args {
        env_args.split_whitespace().map(|s| s.to_string()).collect()
    } else {
        std::env::args().collect()
    };
    dbg!(args.clone());
    info!("Parsed args: {:?}", args.clone());
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
    let mut mode = "client".to_string(); // default to client mode
    let mut torrc_path = "torrc".to_string(); // Default torrc path is in same folder as eltord binary named torrc
    let mut control_port_password: Option<String> = Some("password1234_".to_string()); // Default password for control port

    // Check if first argument is "client" or "relay" or "both"
    if let Some(arg1) = args.next() {
        info!("First argument: {:?}", arg1);
        if arg1 == "client" || arg1 == "relay" || arg1 == "both" {
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

    /// Add a client task to the task manager
    pub fn add_client_task(&mut self, task: JoinHandle<()>) {
        self.client_task = Some(task);
    }

    /// Add a relay task to the task manager
    pub fn add_relay_task(&mut self, task: JoinHandle<()>) {
        self.relay_task = Some(task);
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

    /// Abort all spawned tasks and cleanup processes
    pub fn abort_all(&self) {
        if let Some(ref client) = self.client_task {
            client.abort();
        }
        if let Some(ref relay) = self.relay_task {
            relay.abort();
        }
        // Also cleanup any Tor processes
        cleanup_tor_processes();
    }
}


/// Initialize eltord and return RPC config for manual flow management
/// This allows you to start client and relay flows independently
/// 
/// Workflow:
/// - If mode is "client": Only client flow will be available
/// - If mode is "both": Both client and relay flows will be available (relay acts as client too)
/// - If mode is "relay": Only relay flows will be available
/// 
/// # Arguments
/// 
/// * `args` - Command line arguments for configuration
/// 
/// # Returns
/// 
/// Tuple containing (RPC configuration, mode) for flow management
/// 
/// # Example
/// 
/// ```rust
/// use eltor::{initialize_eltord, start_client, start_relay};
/// 
/// #[tokio::main]
/// async fn main() {
///     let args = vec!["eltor".to_string(), "client".to_string(), "-f".to_string(), "torrc.client.dev".to_string()];
///     let (rpc_config, mode) = initialize_eltord(args).await.unwrap();
///     
///     // Always start client 
///     let client_task = start_client(&rpc_config).await;
///     
///     // Only start relay if mode is "relay"
///     if mode == "relay" {
///         let relay_task = start_relay(&rpc_config).await;
///     }
/// }
/// ```
pub async fn initialize_eltord(args: impl Iterator<Item = impl Into<String>>) -> Result<(RpcConfig, String), Box<dyn std::error::Error>> {
    let (mode, torrc_path, control_port_password) = parse_args(args.into_iter().map(Into::into));
    let rpc_config = self::get_rpc_config_from_torrc(&torrc_path, control_port_password).await;
    info!("RPC Config: {:?}", rpc_config);
    if rpc_config.is_none() {
        return Err("Could not load rpc_config from torrc file".into());
    }
    let rpc_config = rpc_config.unwrap();

    // Check if Tor is already running on this port
    let addr = rpc_config.addr.clone();
    info!("Checking if Tor is already running on {}...", addr);
    
    // Try to connect to see if Tor is already running
    if let Ok(_) = tokio::net::TcpStream::connect(&addr).await {
        info!("Tor appears to already be running on {}, skipping Tor startup", addr);
        return Ok((rpc_config, mode));
    }

    info!("Starting new Tor instance...");
    let torrc_path_clone = torrc_path.clone();
    let tor_handle = tokio::task::spawn_blocking(move || {
        start_tor_in_child_process(torrc_path_clone, "Tor initialization");
    });
    
    // Store the handle so we can manage the Tor instance lifecycle
    // For now we'll detach it, but this could be improved to allow cleanup
    let _ = tor_handle;
    
    // Give Tor a moment to start up before trying to connect
    info!("Waiting for Tor to initialize...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // Verify Tor started successfully
    if let Err(_) = tokio::net::TcpStream::connect(&addr).await {
        return Err(format!("Failed to connect to Tor on {} after startup", addr).into());
    }
    
    info!("Tor instance started successfully on {}", addr);
    Ok((rpc_config, mode))
}
