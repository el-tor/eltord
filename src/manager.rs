use log::{info, warn, error};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, RwLock};
use tokio::time::sleep;

/// Process management commands for external control
#[derive(Debug, Clone)]
pub enum ProcessCommand {
    Start { mode: String, torrc_path: String, password: String },
    Stop,
    Restart { mode: String, torrc_path: String, password: String },
    Status,
}

/// Process status information
#[derive(Debug, Clone)]
pub enum ProcessStatus {
    Stopped,
    Starting,
    Running { pid: u32, mode: String },
    Stopping,
    Error { message: String },
}

/// External process manager for eltord
/// 
/// This allows an external application to control the eltord process
/// through commands and get status updates.
/// 
/// # Example
/// 
/// ```rust
/// use eltor::manager::{EltordProcessManager, ProcessCommand, ProcessStatus};
/// use std::time::Duration;
/// 
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Create the process manager
///     let (mut manager, command_sender, mut status_receiver) = EltordProcessManager::new();
/// 
///     // Start the manager in a background task
///     let manager_handle = tokio::spawn(async move {
///         manager.run().await
///     });
/// 
///     // Start eltord in client mode
///     command_sender.send(ProcessCommand::Start {
///         mode: "client".to_string(),
///         torrc_path: "torrc.client.dev".to_string(),
///         password: "password1234_".to_string(),
///     }).await?;
/// 
///     // Listen for status updates
///     tokio::spawn(async move {
///         while let Some(status) = status_receiver.recv().await {
///             println!("Status: {:?}", status);
///         }
///     });
/// 
///     // Wait a bit then stop
///     tokio::time::sleep(Duration::from_secs(10)).await;
///     command_sender.send(ProcessCommand::Stop).await?;
/// 
///     // Clean shutdown
///     drop(command_sender);
///     let _ = manager_handle.await;
/// 
///     Ok(())
/// }
/// ```
pub struct EltordProcessManager {
    process: Arc<RwLock<Option<Child>>>,
    status: Arc<RwLock<ProcessStatus>>,
    is_running: Arc<AtomicBool>,
    command_rx: Arc<RwLock<Option<mpsc::Receiver<ProcessCommand>>>>,
    status_tx: Option<mpsc::Sender<ProcessStatus>>,
}

impl EltordProcessManager {
    /// Create a new process manager
    /// 
    /// Returns (manager, command_sender, status_receiver)
    /// - Send ProcessCommand through command_sender to control the process
    /// - Receive ProcessStatus updates through status_receiver
    pub fn new() -> (Self, mpsc::Sender<ProcessCommand>, mpsc::Receiver<ProcessStatus>) {
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (status_tx, status_rx) = mpsc::channel(32);

        let manager = Self {
            process: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ProcessStatus::Stopped)),
            is_running: Arc::new(AtomicBool::new(false)),
            command_rx: Arc::new(RwLock::new(Some(cmd_rx))),
            status_tx: Some(status_tx),
        };

        (manager, cmd_tx, status_rx)
    }

    /// Start the process manager main loop
    /// This should be run in a tokio task
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Starting EltordProcessManager");
        
        // Take ownership of the command receiver
        let mut command_rx = {
            let mut rx_guard = self.command_rx.write().await;
            rx_guard.take().ok_or("Command receiver already taken")?
        };

        let status_tx = self.status_tx.clone();

        loop {
            tokio::select! {
                // Handle commands
                cmd = command_rx.recv() => {
                    match cmd {
                        Some(ProcessCommand::Start { mode, torrc_path, password }) => {
                            info!("Received start command: mode={}, torrc={}", mode, torrc_path);
                            self.start_process(mode, torrc_path, password).await;
                        }
                        Some(ProcessCommand::Stop) => {
                            info!("Received stop command");
                            self.stop_process().await;
                        }
                        Some(ProcessCommand::Restart { mode, torrc_path, password }) => {
                            info!("Received restart command");
                            self.stop_process().await;
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            self.start_process(mode, torrc_path, password).await;
                        }
                        Some(ProcessCommand::Status) => {
                            let status = self.status.read().await.clone();
                            if let Some(ref tx) = status_tx {
                                let _ = tx.send(status).await;
                            }
                        }
                        None => {
                            info!("Command channel closed, shutting down manager");
                            break;
                        }
                    }
                }

                // Monitor running process
                _ = self.monitor_process(), if self.is_running.load(Ordering::Relaxed) => {}
            }
        }

        // Clean shutdown
        self.stop_process().await;
        Ok(())
    }

    /// Start the eltord process with given configuration
    async fn start_process(&self, mode: String, torrc_path: String, password: String) {
        if self.is_running.load(Ordering::Relaxed) {
            warn!("Process is already running, stop it first");
            return;
        }

        self.set_status(ProcessStatus::Starting).await;

        // Build the command to run the eltor binary
        let mut cmd = Command::new("cargo");
        cmd.args(&["run", "--", &mode, "-f", &torrc_path, "-pw", &password])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id().unwrap_or(0);
                info!("Started eltord process with PID: {}", pid);
                
                *self.process.write().await = Some(child);
                self.is_running.store(true, Ordering::Relaxed);
                self.set_status(ProcessStatus::Running { pid, mode }).await;

                // Start monitoring the process output
                self.start_output_monitoring().await;
            }
            Err(e) => {
                error!("Failed to start eltord process: {}", e);
                self.set_status(ProcessStatus::Error { 
                    message: format!("Failed to start process: {}", e) 
                }).await;
            }
        }
    }

    /// Stop the running eltord process
    async fn stop_process(&self) {
        if !self.is_running.load(Ordering::Relaxed) {
            return;
        }

        self.set_status(ProcessStatus::Stopping).await;
        self.is_running.store(false, Ordering::Relaxed);

        let mut process_guard = self.process.write().await;
        if let Some(mut child) = process_guard.take() {
            info!("Stopping eltord process...");

            // Try graceful shutdown first
            let _ = child.start_kill();
            
            // Wait up to 10 seconds for graceful shutdown
            match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
                Ok(Ok(status)) => {
                    info!("Process stopped gracefully with status: {}", status);
                }
                Ok(Err(e)) => {
                    error!("Error waiting for process: {}", e);
                }
                Err(_) => {
                    warn!("Process didn't stop gracefully, force killing...");
                    let _ = child.kill().await;
                }
            }
        }

        self.set_status(ProcessStatus::Stopped).await;
    }

    /// Monitor the running process for crashes
    async fn monitor_process(&self) {
        if !self.is_running.load(Ordering::Relaxed) {
            return;
        }

        let mut process_guard = self.process.write().await;
        if let Some(child) = process_guard.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process has exited
                    warn!("Eltord process exited with status: {}", status);
                    self.is_running.store(false, Ordering::Relaxed);
                    
                    if status.success() {
                        self.set_status(ProcessStatus::Stopped).await;
                    } else {
                        self.set_status(ProcessStatus::Error { 
                            message: format!("Process exited with status: {}", status) 
                        }).await;
                    }
                    *process_guard = None;
                }
                Ok(None) => {
                    // Process is still running
                }
                Err(e) => {
                    error!("Error checking process status: {}", e);
                    self.set_status(ProcessStatus::Error { 
                        message: format!("Error monitoring process: {}", e) 
                    }).await;
                }
            }
        }
        
        // Sleep briefly to avoid busy waiting
        sleep(Duration::from_secs(1)).await;
    }

    /// Start monitoring process output in background tasks
    async fn start_output_monitoring(&self) {
        let mut process_guard = self.process.write().await;
        if let Some(child) = process_guard.as_mut() {
            // Take ownership of stdout and stderr
            if let Some(stdout) = child.stdout.take() {
                tokio::spawn(async move {
                    let stdout_reader = BufReader::new(stdout);
                    let mut lines = stdout_reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        info!("[ELTORD-STDOUT] {}", line);
                    }
                });
            }

            if let Some(stderr) = child.stderr.take() {
                tokio::spawn(async move {
                    let stderr_reader = BufReader::new(stderr);
                    let mut lines = stderr_reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        warn!("[ELTORD-STDERR] {}", line);
                    }
                });
            }
        }
    }

    /// Update the process status and notify listeners
    async fn set_status(&self, status: ProcessStatus) {
        *self.status.write().await = status.clone();
        if let Some(ref tx) = self.status_tx {
            let _ = tx.send(status).await;
        }
    }

    /// Get current process status
    pub async fn get_status(&self) -> ProcessStatus {
        self.status.read().await.clone()
    }

    /// Check if process is currently running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::Relaxed)
    }
}
