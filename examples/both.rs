use eltor::{run_with_args};

#[tokio::main]
async fn main() {
    println!("Example: Using eltord as both client and relay");
    // 1. Setup global logger configuration
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_secs()
        .write_style(env_logger::WriteStyle::Always)
        .init();

    // Set args for relay, like where to find the torrc file
    println!("\n--- Running both Client+Relay flow ---");
    let both_args = vec![
        "eltord".to_string(),
        "both".to_string(),
        "-f".to_string(),
        "torrc.relay.prod".to_string(),
        "-pw".to_string(),
        "password1234_".to_string(),
    ];

    // Start eltord as both client and relay
    run_with_args(both_args).await;
}
