//! Dedicated authenticated local connection for foundation state recovery.
//! Never retries a command automatically or interprets an event as settlement.
use super::{PaymentEvent, PaymentInventory, PaymentInventoryBuilder};
use crate::types::RpcConfig;
use std::{collections::VecDeque, io, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};
const LIMIT: usize = 16384;
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or incomplete payment control exchange",
    )
}

pub struct PaymentControl {
    socket: BufReader<TcpStream>,
    events: VecDeque<PaymentEvent>,
    failed: bool,
}
impl PaymentControl {
    pub async fn connect(config: &RpcConfig) -> io::Result<Self> {
        let stream =
            tokio::time::timeout(Duration::from_secs(15), TcpStream::connect(&config.addr))
                .await
                .map_err(|_| invalid())??;
        if !stream.peer_addr()?.ip().is_loopback() {
            return Err(invalid());
        }
        let mut this = Self {
            socket: BufReader::new(stream),
            events: VecDeque::new(),
            failed: false,
        };
        let password = config.rpc_password.as_deref().unwrap_or("");
        if password.bytes().any(|b| b < 32 || b == 127) {
            return Err(invalid());
        }
        let command = if password.is_empty() {
            "AUTHENTICATE".to_owned()
        } else {
            format!(
                "AUTHENTICATE \"{}\"",
                password.replace('\\', "\\\\").replace('"', "\\\"")
            )
        };
        if this.command(&command).await? != "250 OK" {
            return Err(invalid());
        }
        if this.command("SETEVENTS ELTOR_PAYMENT_STATE").await? != "250 OK" {
            return Err(invalid());
        }
        Ok(this)
    }
    /// Cancellation/timeouts poison this connection because reply ownership is
    /// uncertain. Recover on a new connection using durable operation identities.
    pub async fn command(&mut self, command: &str) -> io::Result<String> {
        if self.failed
            || command.is_empty()
            || command.len() + 2 > LIMIT
            || command.bytes().any(|b| b < 32 || b == 127)
        {
            return Err(invalid());
        }
        self.failed = true;
        let result = tokio::time::timeout(Duration::from_secs(15), self.exchange(command))
            .await
            .map_err(|_| invalid())?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    async fn line(&mut self) -> io::Result<String> {
        let mut bytes = Vec::new();
        while bytes.len() < LIMIT {
            let byte = self.socket.read_u8().await?;
            bytes.push(byte);
            if byte == b'\n' {
                if bytes.len() < 2 || bytes[bytes.len() - 2] != b'\r' {
                    return Err(invalid());
                }
                return String::from_utf8(bytes).map_err(|_| invalid());
            }
        }
        Err(invalid())
    }
    async fn exchange(&mut self, command: &str) -> io::Result<String> {
        self.socket.get_mut().write_all(command.as_bytes()).await?;
        self.socket.get_mut().write_all(b"\r\n").await?;
        self.socket.get_mut().flush().await?;
        let mut reply = String::new();
        loop {
            let line = self.line().await?;
            if line.starts_with("650 ") {
                if self.events.len() >= 1024 {
                    return Err(invalid());
                }
                self.events
                    .push_back(PaymentEvent::parse(&line).map_err(|_| invalid())?);
                continue;
            }
            let body = line.strip_suffix("\r\n").ok_or_else(invalid)?;
            if body.len() < 4
                || !body.starts_with("250")
                || !matches!(body.as_bytes()[3], b'-' | b' ')
            {
                return Err(invalid());
            }
            if reply.len() + body.len() + 1 > LIMIT {
                return Err(invalid());
            }
            if !reply.is_empty() {
                reply.push('\n');
            }
            reply.push_str(body);
            if body.as_bytes()[3] == b' ' {
                return Ok(reply);
            }
        }
    }
    /// Subscribe first (connect does so), then fetch all pages on this exact
    /// connection. Buffered event hints must be reconciled after this snapshot.
    pub async fn inventory(&mut self) -> io::Result<PaymentInventory> {
        let mut builder = PaymentInventoryBuilder::default();
        let mut command = "ELTOR_PAYMENT_LIST 1 - -".to_string();
        let mut instance = None;
        for _ in 0..32 {
            let reply = self.command(&command).await?;
            let next = match builder.add_page(&reply) {
                Ok(next) => next,
                Err(_) => {
                    self.failed = true;
                    return Err(invalid());
                }
            };
            if let Some(cursor) = next {
                // Header already strictly validated by the inventory decoder.
                let value = reply.split(' ').nth(2).ok_or_else(invalid)?;
                let pinned = instance.get_or_insert_with(|| value.to_owned());
                command = format!("ELTOR_PAYMENT_LIST 1 {} {}", pinned, hex::encode(cursor));
            } else {
                return builder.finish().map_err(|_| invalid());
            }
        }
        self.failed = true;
        Err(invalid())
    }
    /// Build the bounded query worklist from a complete subscribed inventory.
    /// This does not authorize payment; callers must query and validate each row.
    pub async fn recover(&mut self) -> io::Result<super::PaymentRecovery> {
        let inventory = self.inventory().await?;
        let events = self.take_events()?;
        match super::PaymentRecovery::new(inventory, &events) {
            Ok(recovery) => Ok(recovery),
            Err(_) => {
                self.failed = true;
                Err(invalid())
            }
        }
    }
    /// Forward only an authorization already committed by recipient verification
    /// (or explicit zero-price accounting). A receipt is not consumption proof.
    pub async fn notify_authorization(
        &mut self,
        ledger: &mut crate::payment_ledger::PaymentLedger,
        obligation: &crate::payment_ledger::Obligation,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        use crate::payment_ledger::AuthorizationState;
        let authorization = ledger
            .get_authorization(obligation)?
            .ok_or("No durable payment authorization")?;
        if authorization.state == AuthorizationState::ClosedUnresolved {
            return Err("Payment binding is closed and unresolved".into());
        }
        let command = format!(
            "ELTOR_PAYMENT_AUTHORIZE 1 {} {} {} {} {} {} {}",
            hex::encode(obligation.instance),
            hex::encode(obligation.handle),
            hex::encode(authorization.decision),
            obligation.round,
            obligation.amount_msat,
            hex::encode(authorization.payment_hash),
            hex::encode(authorization.record_digest)
        );
        let reply = self.command(&command).await?;
        let fields: Vec<_> = reply.split(' ').collect();
        if fields.len() != 7
            || fields[..3] != ["250", "ELTOR_AUTHORIZED", "1"]
            || fields[3] != hex::encode(obligation.instance)
            || fields[4] != hex::encode(obligation.handle)
            || fields[5] != hex::encode(authorization.decision)
        {
            self.failed = true;
            return Err("Mismatched payment authorization receipt".into());
        }
        let revision: u64 = match fields[6].parse() {
            Ok(value) => value,
            Err(_) => {
                self.failed = true;
                return Err(invalid().into());
            }
        };
        if revision == 0 || revision.to_string() != fields[6] {
            self.failed = true;
            return Err("Invalid payment authorization revision".into());
        }
        Ok(revision)
    }

    /// Wait for one state hint when no command is outstanding. Idle waiting has
    /// no payment deadline semantics. If the caller cancels this future after
    /// reading begins, discard this connection and recover inventory again;
    /// partial frame bytes must never be interpreted as a fresh event.
    pub async fn next_event(&mut self) -> io::Result<PaymentEvent> {
        if self.failed {
            return Err(invalid());
        }
        if let Some(event) = self.events.pop_front() {
            return Ok(event);
        }
        self.failed = true;
        let line = self.line().await?;
        let event = PaymentEvent::parse(&line).map_err(|_| invalid())?;
        self.failed = false;
        Ok(event)
    }

    pub fn take_events(&mut self) -> io::Result<Vec<PaymentEvent>> {
        if self.failed {
            return Err(invalid());
        }
        Ok(self.events.drain(..).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{io::AsyncBufReadExt, net::TcpListener};
    async fn read(socket: &mut BufReader<TcpStream>) -> String {
        let mut s = String::new();
        socket.read_line(&mut s).await.unwrap();
        s
    }
    #[tokio::test]
    async fn payment_control_inventory_retains_interleaved_events() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let mut socket = BufReader::new(listener.accept().await.unwrap().0);
            assert_eq!(read(&mut socket).await, "AUTHENTICATE \"a\\\"b\\\\c\"\r\n");
            socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
            assert_eq!(read(&mut socket).await, "SETEVENTS ELTOR_PAYMENT_STATE\r\n");
            socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
            assert_eq!(read(&mut socket).await, "ELTOR_PAYMENT_LIST 1 - -\r\n");
            let mut reply = format!("250-ELTOR_PAYMENT_LIST 1 {:032x} 200 {:032x}\r\n", 1, 999);
            for n in 1..=128 {
                reply += &format!("250-CIRCUIT {:032x} RELAY 1\r\n", n);
            }
            reply += &format!(
                "650 ELTOR_PAYMENT_STATE 1 {:032x} 201 {:032x} RELAY 2 AUTHORIZED\r\n250 OK\r\n",
                1, 1
            );
            socket.get_mut().write_all(reply.as_bytes()).await.unwrap();
            assert_eq!(
                read(&mut socket).await,
                format!("ELTOR_PAYMENT_LIST 1 {:032x} {:032x}\r\n", 1, 999)
            );
            socket.get_mut().write_all(format!("250-ELTOR_PAYMENT_LIST 1 {:032x} 200 -\r\n250-CIRCUIT {:032x} ORIGIN 1\r\n250 OK\r\n",1,129).as_bytes()).await.unwrap();
        });
        let config = RpcConfig {
            addr,
            rpc_password: Some("a\"b\\c".into()),
            command: String::new(),
        };
        let mut client = PaymentControl::connect(&config).await.unwrap();
        let mut recovery = client.recover().await.unwrap();
        assert_eq!(recovery.instance(), 1u128.to_be_bytes());
        assert_eq!(recovery.next_query().unwrap().unwrap().revision, 2);
        let mut queried = 0;
        while let Some(row) = recovery.next_query().unwrap() {
            assert!(recovery.queried(recovery.instance(), &row).unwrap());
            queried += 1;
        }
        assert_eq!(queried, 129);
        assert!(client.take_events().unwrap().is_empty());
        server.await.unwrap();
    }
    #[tokio::test]
    async fn payment_control_bounds_lines_and_poisoned_replies() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let mut socket = BufReader::new(listener.accept().await.unwrap().0);
            for expected in ["AUTHENTICATE\r\n", "SETEVENTS ELTOR_PAYMENT_STATE\r\n"] {
                assert_eq!(read(&mut socket).await, expected);
                socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
            }
            assert_eq!(read(&mut socket).await, "ELTOR_PAYMENT_LIST 1 - -\r\n");
            socket
                .get_mut()
                .write_all(&vec![b'x'; LIMIT])
                .await
                .unwrap();
        });
        let config = RpcConfig {
            addr,
            rpc_password: None,
            command: String::new(),
        };
        let mut client = PaymentControl::connect(&config).await.unwrap();
        assert!(client.inventory().await.is_err());
        assert!(client.take_events().is_err());
        assert!(client.command("ELTOR_PAYMENT_LIST 1 - -").await.is_err());
        server.await.unwrap();
    }
    #[tokio::test]
    async fn payment_control_idle_events_and_partial_cancellation() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let mut socket = BufReader::new(listener.accept().await.unwrap().0);
            for _ in 0..2 {
                read(&mut socket).await;
                socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
            }
            socket
                .get_mut()
                .write_all(
                    format!(
                        "650 ELTOR_PAYMENT_STATE 1 {:032x} 1 {:032x} RELAY 1 REGISTERED\r\n",
                        1, 2
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket
                .get_mut()
                .write_all(b"650 ELTOR_PAYMENT_STATE 1 ")
                .await
                .unwrap();
            ready_tx.send(()).unwrap();
            let _ = finish_rx.await;
        });
        let config = RpcConfig {
            addr,
            rpc_password: None,
            command: String::new(),
        };
        let mut client = PaymentControl::connect(&config).await.unwrap();
        ready_rx.await.unwrap();
        assert_eq!(client.next_event().await.unwrap().sequence, 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), client.next_event())
                .await
                .is_err()
        );
        assert!(client.take_events().is_err());
        assert!(client.command("ELTOR_PAYMENT_LIST 1 - -").await.is_err());
        finish_tx.send(()).unwrap();
        server.await.unwrap();
    }
    #[tokio::test]
    async fn payment_control_authorization_requires_ledger_and_does_not_consume() {
        use crate::payment_ledger::*;
        let directory =
            std::env::temp_dir().join(format!("eltor-notify-{:032x}", rand::random::<u128>()));
        std::fs::create_dir(&directory).unwrap();
        let mut ledger =
            PaymentLedger::provision(&directory.join("ledger.sqlite"), AccountId([7; 32])).unwrap();
        let obligation = Obligation {
            instance: [1; 16],
            handle: [2; 16],
            circuit_binding: [3; 32],
            round: 0,
            kind: ChargeKind::Handshake,
            payment_id: [4; 32],
            amount_msat: 0,
            expected_payment_hash: None,
            policy_hash: [5; 32],
            allocation_hash: [6; 32],
            window: Some(AcceptanceWindow {
                opens_at_ms: 0,
                closes_at_ms: 2000,
            }),
        };
        ledger.register_handshake(&obligation).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = RpcConfig {
            addr: listener.local_addr().unwrap().to_string(),
            rpc_password: None,
            command: String::new(),
        };
        let server = tokio::spawn(async move {
            let mut socket = BufReader::new(listener.accept().await.unwrap().0);
            for _ in 0..2 {
                read(&mut socket).await;
                socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
            }
            let command = read(&mut socket).await;
            let fields: Vec<_> = command.trim_end().split(' ').collect();
            assert_eq!(fields.len(), 9);
            assert_eq!(fields[0], "ELTOR_PAYMENT_AUTHORIZE");
            assert_eq!(fields[5], "0");
            assert_eq!(fields[6], "0");
            assert_eq!(fields[7], hex::encode([0; 32]));
            socket
                .get_mut()
                .write_all(
                    format!(
                        "250 ELTOR_AUTHORIZED 1 {} {} {} 9\r\n",
                        fields[2], fields[3], fields[4]
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let mut client = PaymentControl::connect(&config).await.unwrap();
        assert!(client
            .notify_authorization(&mut ledger, &obligation)
            .await
            .is_err());
        ledger
            .authorize_zero(
                &obligation,
                [8; 16],
                ClockObservation {
                    now_ms: 1000,
                    uncertainty_ms: 0,
                },
            )
            .unwrap();
        assert_eq!(
            client
                .notify_authorization(&mut ledger, &obligation)
                .await
                .unwrap(),
            9
        );
        assert!(
            ledger
                .get_authorization(&obligation)
                .unwrap()
                .unwrap()
                .state
                == AuthorizationState::Authorized
        );
        server.await.unwrap();
        drop(client);
        drop(ledger);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
