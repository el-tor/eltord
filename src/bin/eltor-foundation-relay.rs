//! Attach a foundation controller to existing local Tor; never start Tor here.
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "eltor-foundation-relay",
    about = "Attach a foundation relay controller to an existing loopback Tor process"
)]
struct Args {
    /// Numeric loopback control endpoint, including port.
    #[arg(long)]
    control: SocketAddr,
    /// Private regular file containing the existing Tor control password.
    #[arg(long)]
    password_file: PathBuf,
    /// Explicit private account configuration; its ledger must already exist.
    #[arg(long)]
    foundation_config: PathBuf,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match eltor::relay::run_foundation_relay_controller(
        args.control,
        &args.password_file,
        &args.foundation_config,
    )
    .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("Foundation relay controller stopped; verify the loopback endpoint, private authentication/configuration, foundation mode and existing ledger.");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_requires_explicit_files_and_never_accepts_a_password_argument() {
        let good = [
            "eltor-foundation-relay",
            "--control",
            "127.0.0.1:8061",
            "--password-file",
            "private/control-password",
            "--foundation-config",
            "private/account.json",
        ];
        assert!(Args::try_parse_from(good).is_ok());
        assert!(Args::try_parse_from(["eltor-foundation-relay"]).is_err());
        let mut bad = good.to_vec();
        bad.extend(["--password", "not-an-accepted-option"]);
        assert!(Args::try_parse_from(bad).is_err());
        let mut bad = good.to_vec();
        bad[2] = "localhost:8061";
        assert!(Args::try_parse_from(bad).is_err());
    }
}
