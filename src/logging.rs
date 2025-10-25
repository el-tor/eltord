use std::fs::OpenOptions;
use std::sync::{Arc, Mutex};

/// Setup logging with optional file output.
/// Uses non-blocking writes to avoid impacting performance.
/// 
/// # Arguments
/// 
/// * `log_file` - Optional path to log file
/// * `keep_logs` - If false, clears the log file on startup
pub fn setup_logging(log_file: Option<String>, keep_logs: bool) {
    let file_writer = log_file.map(|path| {
        // Create parent directory if needed
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        
        // Clear log file on startup if keep_logs is false
        if !keep_logs {
            let _ = std::fs::write(&path, "");
        }
        
        // Open file in append mode
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect(&format!("Failed to open log file: {}", path));
        
        Arc::new(Mutex::new(file))
    });
    
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Info)
        .format(move |buf, record| {
            use std::io::Write as _;
            
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let log_line = format!("[{} {}] {}\n", timestamp, record.level(), record.args());
            
            // Write to stdout (console)
            write!(buf, "{}", log_line)?;
            
            // Non-blocking write to file if configured
            if let Some(ref file) = file_writer {
                // Use try_lock to avoid blocking - if lock is busy, skip this write
                if let Ok(mut f) = file.try_lock() {
                    let _ = f.write_all(log_line.as_bytes());
                    let _ = f.flush(); // Ensure logs are written immediately
                }
            }
            
            Ok(())
        })
        .write_style(env_logger::WriteStyle::Always)
        .init();
}
