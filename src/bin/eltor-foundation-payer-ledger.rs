//! Explicit payer provisioning only. No payment, Tor or fallback path.
use clap::Parser;
use std::{path::PathBuf, process::ExitCode};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: PathBuf,
}
#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    match eltor::client::provision_foundation_payer(&args.config).await {
        Ok(()) => {
            println!("Created new verified foundation payer ledger.");
            ExitCode::SUCCESS
        }
        Err(_) => {
            eprintln!("Payer provisioning refused; inspect private configuration, native capabilities and retained ledger.");
            ExitCode::FAILURE
        }
    }
}
