use eltor::init_and_run;
use eltor::logging::setup_logging;
use clap::Parser;
use dotenv::dotenv;
use std::env;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Mode: client, relay, or both
    #[arg(value_name = "MODE")]
    mode: Option<String>,
    
    /// Torrc configuration file path
    #[arg(short = 'f', long = "config")]
    config: Option<String>,
    
    /// Control port password
    #[arg(short = 'p', long = "pw")]
    password: Option<String>,
    
    /// Optional log file path for output (non-blocking file logging)
    #[arg(short = 'l', long = "log-file")]
    log_file: Option<String>,
    
    /// Keep logs on exit (don't clear log file on startup or delete on exit)
    #[arg(short = 'k', long = "keep-logs")]
    keep_logs: bool,
    
    /// Internal flag for Windows Tor subprocess
    #[arg(long = "tor-subprocess", hide = true)]
    tor_subprocess: bool,
    
    /// Torrc path for subprocess mode
    #[arg(hide = true)]
    torrc_path: Option<String>,
}

#[tokio::main]
async fn main() {
    // Load .env file first
    dotenv().ok();
    
    // Check if ARGS env variable is set (takes precedence over CLI args)
    let args = if let Ok(env_args) = env::var("ARGS") {
        // Parse ARGS environment variable
        let args_vec: Vec<String> = env_args.split_whitespace().map(|s| s.to_string()).collect();
        Args::parse_from(args_vec)
    } else {
        // Parse regular command line arguments
        Args::parse()
    };
    
    // Setup logging with optional file output
    let log_file_path = args.log_file.clone();
    setup_logging(log_file_path.clone(), args.keep_logs);
    
    // Check for Windows subprocess mode
    if args.tor_subprocess {
        if let Some(torrc_path) = args.torrc_path {
            run_tor_subprocess(torrc_path).await;
        } else {
            eprintln!("Error: --tor-subprocess requires torrc path argument");
            std::process::exit(1);
        }
    } else {
        // Build arguments in the format expected by lib.rs and set as ARGS env var
        let mut lib_args = vec!["eltord".to_string()];
        
        if let Some(mode) = args.mode {
            lib_args.push(mode);
        }
        
        if let Some(config) = args.config {
            lib_args.push("-f".to_string());
            lib_args.push(config);
        }
        
        if let Some(password) = args.password {
            lib_args.push("--pw".to_string());
            lib_args.push(password);
        }
        
        // Set the ARGS env var so lib.rs can use it
        env::set_var("ARGS", lib_args.join(" "));
        
        // Normal execution
        init_and_run().await;
        
        // Clean up log file on exit unless -k flag was used
        if !args.keep_logs {
            if let Some(path) = log_file_path {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

#[cfg(windows)]
async fn run_tor_subprocess(torrc_path: String) {
    use libtor::{Tor, TorFlag};
    use log::{info, error};
    
    info!("Starting Tor subprocess with torrc: {}", torrc_path);
    
    // Start Tor in subprocess (Windows process isolation mode)
    match Tor::new().flag(TorFlag::ConfigFile(torrc_path.clone())).start() {
        Ok(_tor) => {
            info!("Tor started successfully in subprocess");
            // Keep the subprocess alive to maintain Tor
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        },
        Err(e) => {
            error!("Failed to start Tor in subprocess: {:?}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
async fn run_tor_subprocess(_torrc_path: String) {
    // This should never be called on non-Windows platforms
    eprintln!("Error: --tor-subprocess flag is only supported on Windows");
    std::process::exit(1);
}
