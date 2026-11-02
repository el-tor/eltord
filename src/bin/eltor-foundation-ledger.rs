//! Explicit ledger maintenance. Runtime daemon startup never invokes this CLI.
use clap::{Parser, Subcommand};
use eltor::relay::{operate_foundation_ledger, LedgerOperation};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "eltor-foundation-ledger",
    about = "Explicit local foundation ledger creation and migration"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Create a new authoritative ledger after verifying its pinned recipient.
    Provision {
        #[arg(long)]
        config: PathBuf,
    },
    /// Upgrade retained history after verifying its original pinned recipient.
    Migrate {
        #[arg(long)]
        config: PathBuf,
    },
}
#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let (operation, config) = match args.command {
        Command::Provision { config } => (LedgerOperation::Provision, config),
        Command::Migrate { config } => (LedgerOperation::Migrate, config),
    };
    match operate_foundation_ledger(operation, &config).await {
        Ok(report) => {
            match operation {
                LedgerOperation::Provision => println!(
                    "Created new foundation ledger (schema {}).",
                    report.current_schema
                ),
                LedgerOperation::Migrate => println!(
                    "Verified foundation ledger migration (schema {} -> {}).",
                    report.previous_schema.unwrap_or(report.current_schema),
                    report.current_schema
                ),
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}.", error);
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_require_explicit_action_and_config_without_reset_flags() {
        for action in ["provision", "migrate"] {
            assert!(Args::try_parse_from([
                "eltor-foundation-ledger",
                action,
                "--config",
                "operator.json"
            ])
            .is_ok());
        }
        for args in [
            vec!["eltor-foundation-ledger"],
            vec!["eltor-foundation-ledger", "provision"],
            vec![
                "eltor-foundation-ledger",
                "provision",
                "--config",
                "operator.json",
                "--force",
            ],
            vec![
                "eltor-foundation-ledger",
                "reset",
                "--config",
                "operator.json",
            ],
        ] {
            assert!(Args::try_parse_from(args).is_err());
        }
    }
}
