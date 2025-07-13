use eltor::{init_and_run};

/// Example showing how to use eltord as a library with full initialization
#[tokio::main]
async fn main() {
    println!("Example: Using eltord as a library with full initialization");
    
    // This will:
    // 1. Load .env file if present
    // 2. Check for ARGS environment variable
    // 3. Parse command line arguments
    // 4. Start the appropriate mode (client/relay)
    // 5. Start Tor with the specified config
    init_and_run().await;
}
