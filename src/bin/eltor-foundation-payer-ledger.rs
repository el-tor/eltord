//! Explicit payer provisioning and operator-approved ceiling extension. No payments.
use clap::Parser;
use std::{path::PathBuf, process::ExitCode};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: PathBuf,
    #[arg(long, requires = "lifetime_limit_msat")]
    expected_lifetime_msat: Option<u64>,
    #[arg(long, requires = "expected_lifetime_msat")]
    lifetime_limit_msat: Option<u64>,
    #[arg(long, requires = "outstanding_limit_msat", conflicts_with = "expected_lifetime_msat")]
    expected_outstanding_msat: Option<u64>,
    #[arg(long, requires = "expected_outstanding_msat")]
    outstanding_limit_msat: Option<u64>,
}
#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let extending = args.expected_lifetime_msat.zip(args.lifetime_limit_msat);
    let outstanding = args.expected_outstanding_msat.zip(args.outstanding_limit_msat);
    let result = match extending {
        Some((expected, limit)) => {
            eltor::client::extend_foundation_payer_budget(&args.config, expected, limit).await
        }
        None => match outstanding {
            Some((expected, limit)) => {
                eltor::client::extend_foundation_payer_outstanding(&args.config, expected, limit).await
            }
            None => eltor::client::provision_foundation_payer(&args.config).await,
        },
    };
    match result {
        Ok(()) => {
            println!(
                "{}",
                if extending.is_some() {
                    "Extended verified payer lifetime ceiling; all history and reservations retained."
                } else if outstanding.is_some() {
                    "Extended verified payer concurrent ceiling; total ceiling and all history retained."
                } else {
                    "Created new verified foundation payer ledger."
                }
            );
            ExitCode::SUCCESS
        }
        Err(_) => {
            eprintln!("Payer operation refused; inspect private configuration, native capabilities and retained ledger.");
            ExitCode::FAILURE
        }
    }
}
