//! Explicit foundation controller for an already-running standalone Tor client.
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "eltor-foundation-client",
    about = "Attach a foundation client controller to existing loopback Tor"
)]
struct Args {
    #[arg(long)]
    control: SocketAddr,
    #[arg(long)]
    password_file: PathBuf,
    #[arg(long)]
    foundation_config: PathBuf,
    #[arg(long)]
    client_plan: PathBuf,
}
#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match eltor::client::run_foundation_client_controller(
        args.control,
        &args.password_file,
        &args.client_plan,
        &args.foundation_config,
    )
    .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("Foundation client controller stopped; verify explicit private files, authenticated foundation mode, original scope and retained ledger.");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standalone_client_requires_all_explicit_attachment_paths() {
        let good = [
            "eltor-foundation-client",
            "--control",
            "127.0.0.1:8060",
            "--password-file",
            "private/control-password",
            "--foundation-config",
            "private/account.json",
            "--client-plan",
            "private/client-plan.json",
        ];
        assert!(Args::try_parse_from(good).is_ok());
        assert!(Args::try_parse_from(&good[..7]).is_err());
        assert!(Args::try_parse_from(["eltor-foundation-client"]).is_err());
        let mut bad = good.to_vec();
        bad.extend(["--password", "unsupported"]);
        assert!(Args::try_parse_from(bad).is_err());
    }
}
