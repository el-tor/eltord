use eltor::{run_with_args, parse_args};

/// Example showing how to use eltord as a library with custom arguments
#[tokio::main]
async fn main() {
    println!("Example: Using eltord as a library with custom arguments");
    
    // Example 1: Run with predefined arguments
    println!("\n--- Running client mode with custom torrc ---");
    let client_args = vec![
        "myapp".to_string(),
        "client".to_string(),
        "-f".to_string(),
        "torrc.client.dev".to_string(),
        "-pw".to_string(),
        "password123".to_string(),
    ];
    
    // This would start the client - commented out for demo
    // run_with_args(client_args).await;
    
    // Example 2: Parse arguments manually
    println!("--- Parsing arguments manually ---");
    let test_args = vec![
        "myapp".to_string(),
        "relay".to_string(),
        "-f".to_string(),
        "torrc.relay.prod".to_string(),
    ];
    
    let (mode, torrc_path, password) = parse_args(test_args);
    println!("Parsed - Mode: {}, Torrc: {}, Password: {:?}", mode, torrc_path, password);
    
    // Example 3: Use command line arguments
    println!("\n--- Using actual command line arguments ---");
    let args: Vec<String> = std::env::args().collect();
    println!("Command line args: {:?}", args);
    
    if args.len() > 1 {
        println!("Running with command line arguments...");
        run_with_args(args).await;
    } else {
        println!("No command line arguments provided. Use: cargo run --example custom_usage client -f torrc.client.dev");
    }
}
