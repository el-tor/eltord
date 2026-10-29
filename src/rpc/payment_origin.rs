//! Typed origin controls. Local acceptance is not relay admission or permission
//! to spend. The coordinator must durably retain operation arguments before
//! dispatch and collect every hop's authenticated commitment before funding.
use super::PaymentControl;
use std::{convert::TryInto, io};
fn bad() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid origin payment control state",
    )
}
fn number(s: &str) -> io::Result<u64> {
    let n: u64 = s.parse().map_err(|_| bad())?;
    if n.to_string() != s {
        return Err(bad());
    }
    Ok(n)
}
fn hex_id<const N: usize>(s: &str) -> io::Result<[u8; N]> {
    if s.len() != 2 * N
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(bad());
    }
    let mut out = [0; N];
    hex::decode_to_slice(s, &mut out).map_err(|_| bad())?;
    if out == [0; N] {
        return Err(bad());
    }
    Ok(out)
}
#[derive(Clone, PartialEq, Eq)]
pub struct OriginBinding {
    pub instance: [u8; 16],
    pub handle: [u8; 16],
    pub hops: u16,
    pub revision: u64,
}
impl OriginBinding {
    fn validate_hop(&self, hop: u16) -> io::Result<()> {
        if self.instance == [0; 16]
            || self.handle == [0; 16]
            || self.revision == 0
            || hop == 0
            || hop > self.hops
        {
            return Err(bad());
        }
        Ok(())
    }
    fn parse(reply: &str) -> io::Result<Self> {
        let f: Vec<_> = reply.split(' ').collect();
        if f.len() != 7 || f[..3] != ["250", "ELTOR_CIRCUIT", "1"] {
            return Err(bad());
        }
        let out = Self {
            instance: hex_id(f[3])?,
            handle: hex_id(f[4])?,
            hops: number(f[5])?.try_into().map_err(|_| bad())?,
            revision: number(f[6])?,
        };
        out.validate_hop(1)?;
        Ok(out)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialOperationKind {
    Commit,
    Start,
    Query,
}
#[derive(Clone, PartialEq, Eq)]
pub struct InitialOperation {
    pub binding: OriginBinding,
    pub hop: u16,
    pub operation: [u8; 16],
    pub nonce: [u8; 16],
    pub kind: InitialOperationKind,
    pub terms: Option<[u8; 55]>,
}
impl InitialOperation {
    /// Canonical bytes to retain durably. This never generates replacement IDs.
    pub fn command(&self) -> io::Result<String> {
        self.binding.validate_hop(self.hop)?;
        if self.operation == [0; 16] || self.nonce == [0; 16] {
            return Err(bad());
        }
        let name = match self.kind {
            InitialOperationKind::Commit => "ELTOR_INITIAL_PREPARE",
            InitialOperationKind::Start => "ELTOR_INITIAL_START",
            InitialOperationKind::Query => "ELTOR_INITIAL_REFRESH",
        };
        let mut out = format!(
            "{} 1 {} {} {} {} {}",
            name,
            hex::encode(self.binding.instance),
            hex::encode(self.binding.handle),
            self.hop,
            hex::encode(self.operation),
            hex::encode(self.nonce)
        );
        match (self.kind, self.terms) {
            (InitialOperationKind::Commit, Some(terms)) if terms[..3] == [1, 1, 10] => {
                out.push(' ');
                out.push_str(&hex::encode(terms));
            }
            (InitialOperationKind::Start | InitialOperationKind::Query, None) => (),
            _ => return Err(bad()),
        }
        Ok(out)
    }
    fn receipt(&self, reply: &str) -> io::Result<u64> {
        let f: Vec<_> = reply.split(' ').collect();
        if f.len() != 8
            || f[..3] != ["250", "ELTOR_INITIAL_QUEUED", "1"]
            || hex_id::<16>(f[3])? != self.binding.instance
            || hex_id::<16>(f[4])? != self.binding.handle
            || number(f[5])? != u64::from(self.hop)
            || hex_id::<16>(f[6])? != self.operation
        {
            return Err(bad());
        }
        let revision = number(f[7])?;
        if revision == 0 {
            return Err(bad());
        }
        Ok(revision)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialOriginState {
    Unconfirmed,
    Uncommitted,
    Committed,
    WaitPayment,
    Active,
    Grace,
    Closed,
    Expired,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialOperationPhase {
    Queued,
    Sent,
    Verified,
    Refused,
    Uncertain,
    Closed,
}
#[derive(Clone, PartialEq, Eq)]
pub struct InitialOperationStatus {
    pub operation: [u8; 16],
    pub kind: InitialOperationKind,
    pub phase: InitialOperationPhase,
    pub request_digest: [u8; 32],
    pub transmissions: u8,
}
#[derive(Clone, PartialEq, Eq)]
pub struct InitialOriginSnapshot {
    pub revision: u64,
    pub state: InitialOriginState,
    pub terms_digest: Option<[u8; 32]>,
    pub initial_allocation: Option<[u8; 32]>,
    pub relay_revision: Option<u64>,
    /// current round, elapsed upper bound, round remaining, enforcement
    /// remaining, setup remaining. None is unavailable, never an invented zero.
    pub clocks: [Option<u64>; 5],
    pub operation: Option<InitialOperationStatus>,
}
impl InitialOriginSnapshot {
    pub fn parse(
        reply: &str,
        binding: &OriginBinding,
        hop: u16,
        operation: Option<[u8; 16]>,
    ) -> io::Result<Self> {
        binding.validate_hop(hop)?;
        if reply.len() > 4096 || operation == Some([0; 16]) {
            return Err(bad());
        }
        let r: Vec<Vec<_>> = reply.split('\n').map(|l| l.split(' ').collect()).collect();
        if r.len() != 5 || r[4] != ["250", "OK"] {
            return Err(bad());
        }
        let h = &r[0];
        if h.len() != 6
            || h[..2] != ["250-ELTOR_INITIAL", "1"]
            || hex_id::<16>(h[2])? != binding.instance
            || hex_id::<16>(h[3])? != binding.handle
            || number(h[4])? != u64::from(hop)
        {
            return Err(bad());
        }
        let revision = number(h[5])?;
        if revision == 0 {
            return Err(bad());
        }
        let p = &r[1];
        if p.len() != 5 || p[0] != "250-PROFILE" {
            return Err(bad());
        }
        let state = match p[3] {
            "UNCONFIRMED" => InitialOriginState::Unconfirmed,
            "UNCOMMITTED" => InitialOriginState::Uncommitted,
            "COMMITTED" => InitialOriginState::Committed,
            "WAIT_PAYMENT" => InitialOriginState::WaitPayment,
            "ACTIVE" => InitialOriginState::Active,
            "GRACE" => InitialOriginState::Grace,
            "CLOSED" => InitialOriginState::Closed,
            "EXPIRED" => InitialOriginState::Expired,
            _ => return Err(bad()),
        };
        let terms_digest = if p[1] == "-" {
            None
        } else {
            Some(hex_id(p[1])?)
        };
        let initial_allocation = if p[2] == "-" {
            None
        } else {
            Some(hex_id(p[2])?)
        };
        let relay_revision = if p[4] == "-" {
            None
        } else {
            let n = number(p[4])?;
            if n == 0 {
                return Err(bad());
            }
            Some(n)
        };
        let c = &r[2];
        if c.len() != 6 || c[0] != "250-CLOCK" {
            return Err(bad());
        }
        let mut clocks = [None; 5];
        for (slot, value) in clocks.iter_mut().zip(&c[1..]) {
            *slot = if *value == "-" {
                None
            } else {
                Some(number(value)?)
            };
        }
        if state == InitialOriginState::Unconfirmed {
            if terms_digest.is_some()
                || initial_allocation.is_some()
                || relay_revision.is_some()
                || clocks.iter().any(Option::is_some)
            {
                return Err(bad());
            }
        } else if terms_digest.is_none() || initial_allocation.is_none() {
            return Err(bad());
        }
        if clocks[..4].iter().any(Option::is_some) && clocks[..4].iter().any(Option::is_none) {
            return Err(bad());
        }
        if matches!(
            state,
            InitialOriginState::Active | InitialOriginState::Grace
        ) && (relay_revision.is_none() || clocks[0].map_or(true, |n| n == 0))
        {
            return Err(bad());
        }
        let o = &r[3];
        if o.len() != 6 || o[0] != "250-OP" {
            return Err(bad());
        }
        let status = if let Some(expected) = operation {
            let actual = hex_id(o[1])?;
            if actual != expected {
                return Err(bad());
            }
            let kind = match o[2] {
                "COMMIT" => InitialOperationKind::Commit,
                "START" => InitialOperationKind::Start,
                "QUERY" => InitialOperationKind::Query,
                _ => return Err(bad()),
            };
            let phase = match o[3] {
                "QUEUED" => InitialOperationPhase::Queued,
                "SENT" => InitialOperationPhase::Sent,
                "VERIFIED" => InitialOperationPhase::Verified,
                "REFUSED" => InitialOperationPhase::Refused,
                "UNCERTAIN" => InitialOperationPhase::Uncertain,
                "CLOSED" => InitialOperationPhase::Closed,
                _ => return Err(bad()),
            };
            let transmissions = number(o[5])?;
            if transmissions > 4 {
                return Err(bad());
            }
            Some(InitialOperationStatus {
                operation: actual,
                kind,
                phase,
                request_digest: hex_id(o[4])?,
                transmissions: transmissions as u8,
            })
        } else {
            if o[1..] != ["-", "-", "-", "-", "-"] {
                return Err(bad());
            }
            None
        };
        Ok(Self {
            revision,
            state,
            terms_digest,
            initial_allocation,
            relay_revision,
            clocks,
            operation: status,
        })
    }
}
/// Local submission or recovered observation. Neither variant authorizes wallet
/// dispatch; the coordinator must separately establish every hop's commitment.
pub enum DurableInitialResult {
    Queued { revision: u64 },
    Recovered(InitialOriginSnapshot),
}
impl PaymentControl {
    /// Persist exact arguments and claim dispatch before writing the socket.
    /// After any prior claim (including a lost reply), only query the original
    /// operation. UNKNOWN_OPERATION remains uncertainty, never a new START.
    pub async fn durable_initial_operation(
        &mut self,
        ledger: &mut crate::payment_ledger::PaymentLedger,
        operation: &InitialOperation,
    ) -> Result<DurableInitialResult, Box<dyn std::error::Error>> {
        let saved = ledger.save_origin_operation(operation)?;
        if saved.dispatch_claimed {
            let op = saved.operation;
            let snapshot = self
                .initial_snapshot(&op.binding, op.hop, Some(op.operation))
                .await?;
            if snapshot.operation.as_ref().map(|state| state.kind) != Some(op.kind) {
                self.invalidate();
                return Err(bad().into());
            }
            if let Some(terms) = op.terms {
                use sha2::{Digest, Sha256};
                let domain = b"eltor/payment-renew/v1/terms";
                let mut hash = Sha256::new();
                hash.update((domain.len() as u16).to_be_bytes());
                hash.update(domain);
                hash.update(terms);
                let expected: [u8; 32] = hash.finalize().into();
                if snapshot.terms_digest != Some(expected) {
                    self.invalidate();
                    return Err(bad().into());
                }
            }
            return Ok(DurableInitialResult::Recovered(snapshot));
        }
        let op = ledger.claim_origin_dispatch(
            operation.binding.instance,
            operation.binding.handle,
            operation.hop,
            operation.operation,
        )?;
        let revision = self.initial_operation(&op).await?;
        Ok(DurableInitialResult::Queued { revision })
    }
    /// Bind a newly owned live circuit. Never use this to recover an old numeric
    /// circuit ID after losing its process instance/handle.
    pub async fn bind_origin(&mut self, circuit_id: u32) -> io::Result<OriginBinding> {
        if circuit_id == 0 {
            return Err(bad());
        }
        let reply = self
            .command(&format!("ELTOR_CIRCUIT_BIND 1 {}", circuit_id))
            .await?;
        OriginBinding::parse(&reply).map_err(|e| {
            self.invalidate();
            e
        })
    }
    /// Dispatch an already-durable operation once. No automatic resend on error.
    pub async fn initial_operation(&mut self, operation: &InitialOperation) -> io::Result<u64> {
        let reply = self.command(&operation.command()?).await?;
        operation.receipt(&reply).map_err(|e| {
            self.invalidate();
            e
        })
    }
    pub async fn initial_snapshot(
        &mut self,
        binding: &OriginBinding,
        hop: u16,
        operation: Option<[u8; 16]>,
    ) -> io::Result<InitialOriginSnapshot> {
        binding.validate_hop(hop)?;
        if operation == Some([0; 16]) {
            return Err(bad());
        }
        let reply = self
            .command(&format!(
                "ELTOR_INITIAL_GET 1 {} {} {} {}",
                hex::encode(binding.instance),
                hex::encode(binding.handle),
                hop,
                operation.map(hex::encode).unwrap_or_else(|| "-".into())
            ))
            .await?;
        InitialOriginSnapshot::parse(&reply, binding, hop, operation).map_err(|e| {
            self.invalidate();
            e
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> OriginBinding {
        OriginBinding {
            instance: [1; 16],
            handle: [2; 16],
            hops: 3,
            revision: 1,
        }
    }
    fn snapshot(state: &str, clocks: &str, op: &str) -> String {
        format!("250-ELTOR_INITIAL 1 {} {} 2 5\n250-PROFILE {} {} {} {}\n250-CLOCK {}\n250-OP {}\n250 OK",hex::encode([1;16]),hex::encode([2;16]),if state=="UNCONFIRMED" {"-".into()}else{hex::encode([3;32])},if state=="UNCONFIRMED" {"-".into()}else{hex::encode([4;32])},state,if state=="UNCONFIRMED" {"-"}else{"2"},clocks,op)
    }
    #[test]
    fn initial_snapshot_preserves_unavailable_and_does_not_equate_verified_with_active() {
        let b = binding();
        let wire = snapshot("UNCONFIRMED", "- - - - -", "- - - - -");
        let parsed = InitialOriginSnapshot::parse(&wire, &b, 2, None).unwrap();
        assert_eq!(parsed.clocks, [None; 5]);
        let op = format!(
            "{} START VERIFIED {} 1",
            hex::encode([5; 16]),
            hex::encode([6; 32])
        );
        let wire = snapshot("WAIT_PAYMENT", "0 0 0 0 1000", &op);
        let parsed = InitialOriginSnapshot::parse(&wire, &b, 2, Some([5; 16])).unwrap();
        assert_eq!(parsed.state, InitialOriginState::WaitPayment);
        assert_eq!(
            parsed.operation.unwrap().phase,
            InitialOperationPhase::Verified
        );
        for badwire in [
            wire.replace(" 2 5\n", " 3 5\n"),
            wire.replace(" 2 5\n", " 2 05\n"),
            wire.replace("0 0 0 0 1000", "0 - 0 0 1000"),
            wire.replace("VERIFIED", "SETTLED"),
            wire.replace(" 1\n250 OK", " 5\n250 OK"),
        ] {
            assert!(InitialOriginSnapshot::parse(&badwire, &b, 2, Some([5; 16])).is_err());
        }
        assert!(InitialOriginSnapshot::parse(&wire, &b, 2, Some([7; 16])).is_err());
        assert!(InitialOriginSnapshot::parse(
            &snapshot("ACTIVE", "- - - - 0", "- - - - -"),
            &b,
            2,
            None
        )
        .is_err());
        assert!(InitialOriginSnapshot::parse(
            &snapshot("UNCONFIRMED", "0 0 0 0 0", "- - - - -"),
            &b,
            2,
            None
        )
        .is_err());
    }
    #[test]
    fn operation_identity_is_exact_and_canonical() {
        let operation = InitialOperation {
            binding: binding(),
            hop: 2,
            operation: [5; 16],
            nonce: [6; 16],
            kind: InitialOperationKind::Start,
            terms: None,
        };
        let command = operation.command().unwrap();
        assert_eq!(
            command,
            format!(
                "ELTOR_INITIAL_START 1 {} {} 2 {} {}",
                hex::encode([1; 16]),
                hex::encode([2; 16]),
                hex::encode([5; 16]),
                hex::encode([6; 16])
            )
        );
        let receipt = format!(
            "250 ELTOR_INITIAL_QUEUED 1 {} {} 2 {} 7",
            hex::encode([1; 16]),
            hex::encode([2; 16]),
            hex::encode([5; 16])
        );
        assert_eq!(operation.receipt(&receipt).unwrap(), 7);
        assert!(operation.receipt(&receipt.replace(" 2 ", " 1 ")).is_err());
        assert!(operation.receipt(&receipt.replace(" 7", " 07")).is_err());
        for hop in [0, 4] {
            let mut bad = operation.clone();
            bad.hop = hop;
            assert!(bad.command().is_err());
        }
        let mut bad = operation.clone();
        bad.terms = Some([0; 55]);
        assert!(bad.command().is_err());
        let mut bad = operation.clone();
        bad.nonce = [0; 16];
        assert!(bad.command().is_err());
        assert!(OriginBinding::parse(&format!(
            "250 ELTOR_CIRCUIT 1 {} {} 0 1",
            hex::encode([1; 16]),
            hex::encode([2; 16])
        ))
        .is_err());
    }
    #[tokio::test]
    async fn durable_origin_recovers_lost_reply_without_second_start() {
        use crate::payment_ledger::{AccountId, PaymentLedger};
        use tokio::{
            io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
            net::TcpListener,
        };
        async fn line(s: &mut BufReader<tokio::net::TcpStream>) -> String {
            let mut value = String::new();
            tokio::time::timeout(std::time::Duration::from_secs(5), s.read_line(&mut value))
                .await
                .unwrap()
                .unwrap();
            value
        }
        for response_kind in ["START", "QUERY", "UNKNOWN"] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let config = crate::types::RpcConfig {
                addr: listener.local_addr().unwrap().to_string(),
                rpc_password: None,
                command: String::new(),
            };
            let op = InitialOperation {
                binding: binding(),
                hop: 2,
                operation: [5; 16],
                nonce: [6; 16],
                kind: InitialOperationKind::Start,
                terms: None,
            };
            let expected = op.command().unwrap() + "\r\n";
            let server = tokio::spawn(async move {
                for attempt in 0..2 {
                    let mut socket = BufReader::new(listener.accept().await.unwrap().0);
                    for expected in ["AUTHENTICATE\r\n", "SETEVENTS ELTOR_PAYMENT_STATE\r\n"] {
                        assert_eq!(line(&mut socket).await, expected);
                        socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
                    }
                    let command = line(&mut socket).await;
                    if attempt == 0 {
                        assert_eq!(command, expected);
                        // Tor could have dispatched START before this socket died.
                        // Deliberately lose the receipt, preserving that ambiguity.
                    } else {
                        assert_eq!(
                            command,
                            format!(
                                "ELTOR_INITIAL_GET 1 {} {} 2 {}\r\n",
                                hex::encode([1; 16]),
                                hex::encode([2; 16]),
                                hex::encode([5; 16])
                            )
                        );
                        if response_kind == "UNKNOWN" {
                            socket
                                .get_mut()
                                .write_all(b"552 UNKNOWN_OPERATION\r\n")
                                .await
                                .unwrap();
                            continue;
                        }
                        let reply = snapshot(
                            "WAIT_PAYMENT",
                            "0 0 60000 75000 3000",
                            &format!(
                                "{} {} VERIFIED {} 1",
                                hex::encode([5; 16]),
                                response_kind,
                                hex::encode([7; 32])
                            ),
                        )
                        .replace('\n', "\r\n")
                            + "\r\n";
                        socket.get_mut().write_all(reply.as_bytes()).await.unwrap();
                    }
                }
            });
            let path = std::env::temp_dir().join(format!(
                "eltor-origin-socket-{}.sqlite",
                rand::random::<u128>()
            ));
            let account = AccountId([9; 32]);
            let mut ledger = PaymentLedger::provision(&path, account).unwrap();
            let mut control = PaymentControl::connect(&config).await.unwrap();
            assert!(control
                .durable_initial_operation(&mut ledger, &op)
                .await
                .is_err());
            drop(control);
            drop(ledger);
            let mut ledger = PaymentLedger::reopen(&path, account).unwrap();
            let mut control = PaymentControl::connect(&config).await.unwrap();
            let result = control.durable_initial_operation(&mut ledger, &op).await;
            if response_kind == "START" {
                match result.unwrap() {
                    DurableInitialResult::Recovered(snapshot) => {
                        assert_eq!(snapshot.state, InitialOriginState::WaitPayment)
                    }
                    DurableInitialResult::Queued { .. } => panic!("must query existing operation"),
                }
            } else {
                assert!(result.is_err());
            }
            assert!(
                ledger
                    .origin_operation(op.binding.instance, op.binding.handle, op.hop, op.operation)
                    .unwrap()
                    .unwrap()
                    .dispatch_claimed
            );
            server.await.unwrap();
            drop(ledger);
            std::fs::remove_file(path).unwrap();
        }
    }
    #[tokio::test]
    async fn origin_socket_binds_queries_and_rejects_substituted_receipt() {
        use tokio::{
            io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
            net::TcpListener,
        };
        async fn read(s: &mut BufReader<tokio::net::TcpStream>) -> String {
            let mut line = String::new();
            s.read_line(&mut line).await.unwrap();
            line
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = crate::types::RpcConfig {
            addr: listener.local_addr().unwrap().to_string(),
            rpc_password: None,
            command: String::new(),
        };
        let server = tokio::spawn(async move {
            let mut socket = BufReader::new(listener.accept().await.unwrap().0);
            for expected in ["AUTHENTICATE\r\n", "SETEVENTS ELTOR_PAYMENT_STATE\r\n"] {
                assert_eq!(read(&mut socket).await, expected);
                socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
            }
            assert_eq!(read(&mut socket).await, "ELTOR_CIRCUIT_BIND 1 17\r\n");
            socket
                .get_mut()
                .write_all(
                    format!(
                        "250 ELTOR_CIRCUIT 1 {} {} 3 1\r\n",
                        hex::encode([1; 16]),
                        hex::encode([2; 16])
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            assert_eq!(
                read(&mut socket).await,
                format!(
                    "ELTOR_INITIAL_GET 1 {} {} 2 -\r\n",
                    hex::encode([1; 16]),
                    hex::encode([2; 16])
                )
            );
            socket
                .get_mut()
                .write_all(
                    (snapshot("UNCONFIRMED", "- - - - -", "- - - - -").replace('\n', "\r\n")
                        + "\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            let operation = InitialOperation {
                binding: binding(),
                hop: 2,
                operation: [5; 16],
                nonce: [6; 16],
                kind: InitialOperationKind::Query,
                terms: None,
            };
            assert_eq!(
                read(&mut socket).await,
                operation.command().unwrap() + "\r\n"
            );
            socket
                .get_mut()
                .write_all(
                    format!(
                        "250 ELTOR_INITIAL_QUEUED 1 {} {} 2 {} 2\r\n",
                        hex::encode([1; 16]),
                        hex::encode([2; 16]),
                        hex::encode([7; 16])
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let mut control = PaymentControl::connect(&config).await.unwrap();
        let binding = control.bind_origin(17).await.unwrap();
        assert_eq!(
            control
                .initial_snapshot(&binding, 2, None)
                .await
                .unwrap()
                .state,
            InitialOriginState::Unconfirmed
        );
        let operation = InitialOperation {
            binding: binding.clone(),
            hop: 2,
            operation: [5; 16],
            nonce: [6; 16],
            kind: InitialOperationKind::Query,
            terms: None,
        };
        assert!(control.initial_operation(&operation).await.is_err());
        assert!(control.initial_snapshot(&binding, 2, None).await.is_err());
        server.await.unwrap();
    }
}
