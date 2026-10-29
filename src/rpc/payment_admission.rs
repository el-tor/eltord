//! Typed local admission decisions. A successful receipt confirms reservation
//! admission only; it is never evidence of settlement or round activation.
use crate::types::RpcConfig;
use std::error::Error;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AdmissionResult {
    Reserved,
    Conflict,
    Capacity,
}

impl AdmissionResult {
    fn wire(self) -> &'static str {
        match self {
            Self::Reserved => "RESERVED",
            Self::Conflict => "CONFLICT",
            Self::Capacity => "CAPACITY",
        }
    }
}

/// Immutable local bindings; deliberately no Debug implementation for logs.
#[derive(Clone, PartialEq, Eq)]
pub struct AdmissionDecision {
    pub instance: [u8; 16],
    pub handle: [u8; 16],
    pub period: u64,
    pub request_digest: [u8; 32],
    pub decision: [u8; 16],
    pub result: AdmissionResult,
}

impl AdmissionDecision {
    pub fn command(&self) -> Result<String, Box<dyn Error>> {
        if self.decision == [0; 16] {
            return Err("Admission decision must be nonzero".into());
        }
        // Every period must have ten representable one-based rounds.
        self.period
            .checked_mul(10)
            .and_then(|n| n.checked_add(10))
            .ok_or("Admission period overflows round range")?;
        Ok(format!(
            "ELTOR_PAYMENT_ADMISSION_DECIDE 1 {} {} {} {} {} {}",
            hex::encode(self.instance),
            hex::encode(self.handle),
            self.period,
            hex::encode(self.request_digest),
            hex::encode(self.decision),
            self.result.wire()
        ))
    }

    /// Parse an already authentication-checked command response. Require the
    /// exact binding and canonical encoding, not a generic `250 OK` substring.
    pub fn receipt_revision(&self, reply: &str) -> Result<u64, Box<dyn Error>> {
        let fields: Vec<_> = reply.split(' ').collect();
        if fields.len() != 8
            || fields[0] != "250"
            || fields[1] != "ELTOR_ADMISSION_DECIDED"
            || fields[2] != "1"
            || fields[3] != hex::encode(self.instance)
            || fields[4] != hex::encode(self.handle)
            || fields[5] != self.period.to_string()
            || fields[6] != hex::encode(self.decision)
        {
            return Err("Mismatched admission receipt".into());
        }
        let revision = fields[7]
            .parse::<u64>()
            .map_err(|_| "Invalid admission revision")?;
        if revision == 0 || fields[7] != revision.to_string() {
            return Err("Noncanonical admission revision".into());
        }
        Ok(revision)
    }
}

/// Caller must persist this exact decision before dispatch. Transport failure
/// is an unknown outcome: query/retry this decision, never create another one.
pub async fn send_admission_decision(
    config: &RpcConfig,
    decision: &AdmissionDecision,
) -> Result<u64, Box<dyn Error>> {
    let response = super::rpc_client_limited(
        RpcConfig {
            addr: config.addr.clone(),
            rpc_password: config.rpc_password.clone(),
            command: decision.command()?,
        },
        2048,
    )
    .await?;
    let reply =
        super::command_response(&response).map_err(|_| "Admission command was not confirmed")?;
    decision.receipt_revision(&reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision() -> AdmissionDecision {
        AdmissionDecision {
            instance: [1; 16],
            handle: [2; 16],
            period: 3,
            request_digest: [4; 32],
            decision: [5; 16],
            result: AdmissionResult::Reserved,
        }
    }

    fn receipt(d: &AdmissionDecision, rev: &str) -> String {
        format!(
            "250 ELTOR_ADMISSION_DECIDED 1 {} {} {} {} {}",
            hex::encode(d.instance),
            hex::encode(d.handle),
            d.period,
            hex::encode(d.decision),
            rev
        )
    }

    #[test]
    fn receipt_requires_exact_binding_and_canonical_revision() {
        let d = decision();
        assert_eq!(d.receipt_revision(&receipt(&d, "7")).unwrap(), 7);
        let mut other = d.clone();
        other.handle = [8; 16];
        assert!(d.receipt_revision(&receipt(&other, "7")).is_err());
        other = d.clone();
        other.instance = [8; 16];
        assert!(d.receipt_revision(&receipt(&other, "7")).is_err());
        other = d.clone();
        other.decision = [8; 16];
        assert!(d.receipt_revision(&receipt(&other, "7")).is_err());
        other = d.clone();
        other.period += 1;
        assert!(d.receipt_revision(&receipt(&other, "7")).is_err());
        for rev in ["0", "07", "+7", "-7", "18446744073709551616", "7\n250 OK"] {
            assert!(d.receipt_revision(&receipt(&d, rev)).is_err());
        }
        assert!(d.receipt_revision("250 OK").is_err());
    }

    #[test]
    fn command_rejects_unusable_decisions_and_round_overflow() {
        let mut d = decision();
        assert!(d.command().unwrap().ends_with(" RESERVED"));
        d.decision = [0; 16];
        assert!(d.command().is_err());
        d.decision = [5; 16];
        d.period = u64::MAX / 10;
        assert!(d.command().is_err());
    }

    #[tokio::test]
    async fn oversized_control_reply_is_rejected_without_a_newline() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            let _ = stream.write_all(&vec![b'x'; 4096]).await;
        });
        let config = RpcConfig {
            addr: address.to_string(),
            rpc_password: None,
            command: String::new(),
        };
        assert!(send_admission_decision(&config, &decision()).await.is_err());
        server.await.unwrap();
    }
}
