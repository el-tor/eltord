//! One-shot construction owned by the durable origin journal. A claimed request
//! with no receipt stays uncertain; it is never replaced by another build here.
use crate::{
    payment_ledger::{ClockObservation, PaymentLedger},
    rpc::PaymentControl,
    types::Relay,
};
use std::io;
pub(crate) struct PreparedConstruction {
    scope: [u8; 32],
    instance: [u8; 16],
    command: String,
    payload: Vec<u8>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or uncertain origin construction",
    )
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
            || clock.uncertainty_ms > 5000
        {
            return Err(invalid());
        }
        let command = super::circuit::foundation_constructor(relays)?;
        let payload = serde_json::to_vec(&serde_json::json!({
            "version":1,"instance":hex::encode(instance),"command":command,
            "accepted_context":hex::encode(accepted_context),"construction_clock":{"now_ms":clock.now_ms,"uncertainty_ms":clock.uncertainty_ms},
        }))
        .map_err(|_| invalid())?;
        if payload.len() > 65536 {
            return Err(invalid());
        }
        Ok(Self {
            scope,
            instance,
            command,
            payload,
        })
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
        if control.inventory().await?.instance != self.instance {
            return Err(invalid());
        }
        ledger
            .claim_origin_construction(self.scope)
            .map_err(|_| invalid())?;
        // Same TCP connection pins the process through dispatch. Any error after
        // this point leaves the durable claim set, including malformed receipts.
        control.construct_origin(&self.command).await
    }
}
