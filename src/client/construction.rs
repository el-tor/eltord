//! One-shot construction owned by the durable origin journal. A claimed request
//! with no receipt stays uncertain; it is never replaced by another build here.
use crate::{
    payment_ledger::{ClockObservation, PaymentLedger},
    rpc::PaymentControl,
    types::Relay,
};
use sha2::{Digest, Sha256};
use std::io;
pub(crate) struct PreparedConstruction {
    scope: [u8; 32],
    instance: [u8; 16],
    command: Option<String>,
    command_digest: String,
    template: String,
    payload: Vec<u8>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or uncertain origin construction",
    )
}
// The preimage occupies bytes 32..64 of each hop's binary constructor material.
// Keep hashes and round IDs for exact recovery, but never persist this secret.
fn sanitized(command: &str) -> io::Result<String> {
    let mut lines = command.split("\r\n");
    if lines.next() != Some("+EXTENDPAIDCIRCUIT 0") {
        return Err(invalid());
    }
    let mut result = String::from("+EXTENDPAIDCIRCUIT 0\r\n");
    let mut hops = 0;
    for line in lines.by_ref() {
        if line == "." {
            if hops == 0 || lines.next() != Some("") || lines.next().is_some() {
                return Err(invalid());
            }
            result.push_str(".\r\n");
            return Ok(result);
        }
        if line.len() != 809
            || line.as_bytes()[40] != b' '
            || !line
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 40 || b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        hops += 1;
        if hops > 16 {
            return Err(invalid());
        }
        result.push_str(&line[..105]);
        result.push_str(&"0".repeat(64));
        result.push_str(&line[169..]);
        result.push_str("\r\n");
    }
    Err(invalid())
}
impl PreparedConstruction {
    pub(crate) fn new(
        scope: [u8; 32],
        instance: [u8; 16],
        relays: &[Relay],
        accepted_context: &[u8],
        clock: ClockObservation,
    ) -> io::Result<Self> {
        if scope == [0; 32]
            || instance == [0; 16]
            || accepted_context.is_empty()
            || accepted_context.len() > 16384
            || clock.now_ms < 0
            || clock
                .now_ms
                .checked_add(clock.uncertainty_ms as i64)
                .is_none()
            || clock.uncertainty_ms > 5000
        {
            return Err(invalid());
        }
        let command = super::circuit::foundation_constructor(relays)?;
        let template = sanitized(&command)?;
        let command_digest = hex::encode(Sha256::digest(command.as_bytes()));
        let payload = serde_json::to_vec(&serde_json::json!({
            "version":2,"instance":hex::encode(instance),"command_digest":command_digest,"template":template,
            "accepted_context":hex::encode(accepted_context),"construction_clock":{"now_ms":clock.now_ms,"uncertainty_ms":clock.uncertainty_ms},
        }))
        .map_err(|_| invalid())?;
        if payload.len() > 65536 {
            return Err(invalid());
        }
        Ok(Self {
            scope,
            instance,
            command: Some(command),
            command_digest,
            template,
            payload,
        })
    }
    /// Recover the exact original context and clock instead of constructing new
    /// arguments from current settings. This never clears a dispatch claim.
    pub(crate) fn restore(ledger: &PaymentLedger, scope: [u8; 32]) -> io::Result<Option<Self>> {
        let Some(saved) = ledger
            .get_origin_construction(scope)
            .map_err(|_| invalid())?
        else {
            return Ok(None);
        };
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Clock {
            now_ms: i64,
            uncertainty_ms: u64,
        }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            version: u8,
            instance: String,
            #[serde(default)]
            command: Option<String>,
            #[serde(default)]
            command_digest: Option<String>,
            #[serde(default)]
            template: Option<String>,
            accepted_context: String,
            construction_clock: Clock,
        }
        let envelope: Envelope = serde_json::from_slice(&saved.payload).map_err(|_| invalid())?;
        let clock = &envelope.construction_clock;
        let context = hex::decode(&envelope.accepted_context).map_err(|_| invalid())?;
        if !matches!(envelope.version, 1 | 2)
            || envelope.instance != hex::encode(saved.instance)
            || context.is_empty()
            || context.len() > 16384
            || hex::encode(&context) != envelope.accepted_context
            || clock.now_ms < 0
            || clock.uncertainty_ms > 5000
            || clock
                .now_ms
                .checked_add(clock.uncertainty_ms as i64)
                .is_none()
        {
            return Err(invalid());
        }
        let (command_digest, template) = match (
            envelope.version,
            envelope.command,
            envelope.command_digest,
            envelope.template,
        ) {
            // Compatibility only for previously retained zero-fee fixture records.
            (1, Some(command), None, None) => (
                hex::encode(Sha256::digest(command.as_bytes())),
                sanitized(&command)?,
            ),
            (2, None, Some(digest), Some(template))
                if digest.len() == 64
                    && digest
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && sanitized(&template)? == template =>
            {
                (digest, template)
            }
            _ => return Err(invalid()),
        };
        Ok(Some(Self {
            scope,
            instance: saved.instance,
            command: None,
            command_digest,
            template,
            payload: saved.payload,
        }))
    }
    /// Supply proof material obtained by lookup only. A changed path, proof or
    /// round ID cannot replace the exact constructor committed before dispatch.
    pub(crate) fn restore_proofs(&mut self, relays: &[Relay]) -> io::Result<()> {
        let command = super::circuit::foundation_constructor(relays)?;
        if hex::encode(Sha256::digest(command.as_bytes())) != self.command_digest
            || sanitized(&command)? != self.template
        {
            return Err(invalid());
        }
        self.command = Some(command);
        Ok(())
    }
    pub(crate) fn accepted_context(&self) -> io::Result<Vec<u8>> {
        let envelope: serde_json::Value =
            serde_json::from_slice(&self.payload).map_err(|_| invalid())?;
        hex::decode(envelope["accepted_context"].as_str().ok_or_else(invalid)?)
            .map_err(|_| invalid())
    }
    pub(crate) async fn dispatch(
        &self,
        ledger: &mut PaymentLedger,
        control: &mut PaymentControl,
    ) -> io::Result<u32> {
        // Exact saved bytes are authoritative. A restart may inspect this record
        // but cannot acquire another dispatch right for its scope.
        let saved = ledger
            .save_origin_construction(self.scope, self.instance, &self.payload)
            .map_err(|_| invalid())?;
        if saved.dispatch_claimed {
            return Err(invalid());
        }
        let command = self.command.as_deref().ok_or_else(invalid)?;
        if control.inventory().await?.instance != self.instance {
            return Err(invalid());
        }
        ledger
            .claim_origin_construction(self.scope)
            .map_err(|_| invalid())?;
        // Same TCP connection pins the process through dispatch. Any error after
        // this point leaves the durable claim set, including malformed receipts.
        control.construct_origin(command).await
    }
}
