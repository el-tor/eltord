use eltor::init_and_run;
use std::env;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs()
        .write_style(env_logger::WriteStyle::Always)
        .init();
    
    // Check for Windows subprocess mode
    let args: Vec<String> = env::args().collect();
    if args.len() >= 3 && args[1] == "--tor-subprocess" {
        // This is a subprocess for Windows process isolation
        let torrc_path = &args[2];
        run_tor_subprocess(torrc_path.clone()).await;
    } else {
        // Normal execution
        init_and_run().await;
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
