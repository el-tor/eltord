//! A failed mode lookup must not route a foundation circuit through legacy
//! wallet logic. Configuration is intent, not permission to bypass Tor's mode.
use crate::{rpc::PaymentControl, types::RpcConfig};
use std::io;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientPaymentMode {
    Legacy,
    Foundation,
}
pub(crate) async fn payment_mode(
    config: &RpcConfig,
    foundation_configured: bool,
) -> io::Result<ClientPaymentMode> {
    let mut control = PaymentControl::connect(config).await?;
    match control.command("GETCONF ElTorFoundation").await?.as_str() {
        "250 ElTorFoundation=0" if !foundation_configured => Ok(ClientPaymentMode::Legacy),
        "250 ElTorFoundation=1" => Ok(ClientPaymentMode::Foundation),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Client payment profile mismatch",
        )),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
    };
    #[tokio::test]
    async fn only_explicit_disabled_mode_can_enter_legacy_wallet_flow() {
        for (reply, configured, expected) in [
            (
                "250 ElTorFoundation=0",
                false,
                Some(ClientPaymentMode::Legacy),
            ),
            ("250 ElTorFoundation=0", true, None),
            (
                "250 ElTorFoundation=1",
                false,
                Some(ClientPaymentMode::Foundation),
            ),
            (
                "250 ElTorFoundation=1",
                true,
                Some(ClientPaymentMode::Foundation),
            ),
            ("250 ElTorFoundation=2", false, None),
            ("552 UNKNOWN_OPTION", false, None),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let config = RpcConfig {
                addr: listener.local_addr().unwrap().to_string(),
                rpc_password: None,
                command: String::new(),
            };
            let server = tokio::spawn(async move {
                let mut stream = BufReader::new(listener.accept().await.unwrap().0);
                for command in [
                    "AUTHENTICATE\r\n",
                    "SETEVENTS ELTOR_PAYMENT_STATE\r\n",
                    "GETCONF ElTorFoundation\r\n",
                ] {
                    let mut line = String::new();
                    stream.read_line(&mut line).await.unwrap();
                    assert_eq!(line, command);
                    let response = if command.starts_with("GETCONF") {
                        reply
                    } else {
                        "250 OK"
                    };
                    stream
                        .get_mut()
                        .write_all(format!("{}\r\n", response).as_bytes())
                        .await
                        .unwrap();
                }
                // Mode selection must never send an activation or wallet command.
                let mut tail = String::new();
                assert_eq!(stream.read_line(&mut tail).await.unwrap(), 0);
            });
            assert_eq!(payment_mode(&config, configured).await.ok(), expected);
            server.await.unwrap();
        }
    }
}
