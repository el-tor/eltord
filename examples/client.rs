use eltor::{run_with_args};

#[tokio::main]
async fn main() {
    println!("Example: Using eltord as a client");

    // Enable logging to stdout with info level and above
    // 1. Setup global logger configuration
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs()
        .write_style(env_logger::WriteStyle::Always)
        .init();

    // Set args for client, like where to find the torrc file
    println!("\n--- Running client mode with custom torrc ---");
    let client_args = vec![
        "eltord".to_string(),
        "client".to_string(),
        "-f".to_string(),
        "torrc.client.prod".to_string(),
        "--pw".to_string(),
        "password1234_".to_string(),
    ];

    // Start eltord as client
    run_with_args(client_args).await;
}
