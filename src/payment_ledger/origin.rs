//! Durable local origin-control identities. Recording or dispatching a control
//! command is not evidence of relay admission, payment, or round activation.
//! A lost reply requires querying this exact operation on the same Tor instance.
use super::*;
use crate::rpc::{InitialOperation, InitialOperationKind, OriginBinding};

const LIMIT: i64 = 4096;
#[derive(Clone)]
pub struct SavedOriginOperation {
    pub operation: InitialOperation,
    pub dispatch_claimed: bool,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    instance: Id16,
    handle: Id16,
    hops: u16,
    revision: u64,
    hop: u16,
    operation: Id16,
    nonce: Id16,
    kind: u8,
    terms: Option<Vec<u8>>,
}
fn kind(value: InitialOperationKind) -> u8 {
    match value {
        InitialOperationKind::Commit => 0,
        InitialOperationKind::Start => 1,
        InitialOperationKind::Query => 2,
    }
}
impl Payload {
    fn new(op: &InitialOperation) -> Self {
        Self {
            instance: op.binding.instance,
            handle: op.binding.handle,
            hops: op.binding.hops,
            revision: op.binding.revision,
            hop: op.hop,
            operation: op.operation,
            nonce: op.nonce,
            kind: kind(op.kind),
            terms: op.terms.map(|t| t.to_vec()),
        }
    }
    fn decode(self) -> Result<InitialOperation> {
        let op = InitialOperation {
            binding: OriginBinding {
                instance: self.instance,
                handle: self.handle,
                hops: self.hops,
                revision: self.revision,
            },
            hop: self.hop,
            operation: self.operation,
            nonce: self.nonce,
            kind: match self.kind {
                0 => InitialOperationKind::Commit,
                1 => InitialOperationKind::Start,
                2 => InitialOperationKind::Query,
                _ => return Err(LedgerError::InvalidLedger),
            },
            terms: self
                .terms
                .map(|t| t.try_into().map_err(|_| LedgerError::InvalidLedger))
                .transpose()?,
        };
        op.command().map_err(|_| LedgerError::InvalidLedger)?;
        Ok(op)
    }
}
impl PaymentLedger {
    /// Save before any mutating RPC. Exact replay preserves the original record
    /// even when a newer local observation has advanced the circuit revision.
    pub fn save_origin_operation(&mut self, op: &InitialOperation) -> Result<SavedOriginOperation> {
        self.check_file_identity()?;
        let command = op.command().map_err(|_| LedgerError::InvalidInput)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(saved) = read(
            &tx,
            op.binding.instance,
            op.binding.handle,
            op.hop,
            op.operation,
        )? {
            if saved
                .operation
                .command()
                .map_err(|_| LedgerError::InvalidLedger)?
                != command
                || saved.operation.binding.hops != op.binding.hops
            {
                return Err(LedgerError::Conflict);
            }
            return Ok(saved);
        }
        require_open_instance(&tx, op.binding.instance)?;
        super::receiving::require_open_binding(&tx, op.binding.instance, op.binding.handle)?;
        storage::check_admission(&tx)?;
        origin_artifacts::control_capacity(&tx)?;
        let count: i64 =
            tx.query_row("SELECT count(*) FROM origin_operations", [], |r| r.get(0))?;
        if count >= LIMIT {
            return Err(LedgerError::BudgetExceeded);
        }
        // COMMIT and START can never be replaced by another operation identity.
        // QUERY identities are retained too; issuing a later query is a separate
        // coordinator decision after inspecting the prior authoritative result.
        let conflict:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM origin_operations WHERE instance=?1 AND handle=?2 AND hop=?3 AND (nonce=?4 OR (kind=?5 AND kind IN(0,1))))",params![&op.binding.instance[..],&op.binding.handle[..],op.hop,&op.nonce[..],kind(op.kind)],|r|r.get(0))?;
        if conflict {
            return Err(LedgerError::Conflict);
        }
        let payload =
            serde_json::to_vec(&Payload::new(op)).map_err(|_| LedgerError::InvalidInput)?;
        tx.execute("INSERT INTO origin_operations(instance,handle,hop,operation,nonce,kind,payload,command,claimed,reserve_bytes) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0,262144)",params![&op.binding.instance[..],&op.binding.handle[..],op.hop,&op.operation[..],&op.nonce[..],kind(op.kind),payload,command])?;
        storage::check_admission(&tx)?;
        tx.commit()?;
        Ok(SavedOriginOperation {
            operation: op.clone(),
            dispatch_claimed: false,
        })
    }
    pub fn origin_operation(
        &self,
        instance: Id16,
        handle: Id16,
        hop: u16,
        operation: Id16,
    ) -> Result<Option<SavedOriginOperation>> {
        self.check_file_identity()?;
        read(&self.connection, instance, handle, hop, operation)
    }
    /// The dispatch latch is durable before returning the command. Losing the
    /// command/result does not grant a second dispatch; query Tor by saved ID.
    pub fn claim_origin_dispatch(
        &mut self,
        instance: Id16,
        handle: Id16,
        hop: u16,
        operation: Id16,
    ) -> Result<InitialOperation> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_open_instance(&tx, instance)?;
        super::receiving::require_open_binding(&tx, instance, handle)?;
        let saved =
            read(&tx, instance, handle, hop, operation)?.ok_or(LedgerError::NotRegistered)?;
        if saved.dispatch_claimed {
            return Err(LedgerError::DispatchAlreadyClaimed);
        }
        storage::check_dispatch(&tx)?;
        tx.execute("UPDATE origin_operations SET claimed=1 WHERE instance=?1 AND handle=?2 AND hop=?3 AND operation=?4",params![&instance[..],&handle[..],hop,&operation[..]])?;
        tx.commit()?;
        Ok(saved.operation)
    }
}
fn read(
    c: &Connection,
    instance: Id16,
    handle: Id16,
    hop: u16,
    operation: Id16,
) -> Result<Option<SavedOriginOperation>> {
    let row:Option<(Vec<u8>,String,bool,Vec<u8>,u8)>=c.query_row("SELECT payload,command,claimed,nonce,kind FROM origin_operations WHERE instance=?1 AND handle=?2 AND hop=?3 AND operation=?4",params![&instance[..],&handle[..],hop,&operation[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    row.map(|(bytes, command, claimed, nonce, stored_kind)| {
        if bytes.len() > 2048 {
            return Err(LedgerError::InvalidLedger);
        }
        let p: Payload = serde_json::from_slice(&bytes).map_err(|_| LedgerError::InvalidLedger)?;
        let op = p.decode()?;
        if op.binding.instance != instance
            || op.binding.handle != handle
            || op.hop != hop
            || op.operation != operation
            || op.nonce[..] != nonce
            || kind(op.kind) != stored_kind
            || op.command().map_err(|_| LedgerError::InvalidLedger)? != command
        {
            return Err(LedgerError::InvalidLedger);
        }
        Ok(SavedOriginOperation {
            operation: op,
            dispatch_claimed: claimed,
        })
    })
    .transpose()
}
pub(super) fn validate_schema(c: &Connection) -> Result<()> {
    c.prepare("SELECT instance,handle,hop,operation,nonce,kind,payload,command,claimed,reserve_bytes FROM origin_operations LIMIT 0")?;
    let mut stmt = c.prepare("SELECT instance,handle,hop,operation FROM origin_operations")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, Vec<u8>>(0)?,
            r.get::<_, Vec<u8>>(1)?,
            r.get::<_, u16>(2)?,
            r.get::<_, Vec<u8>>(3)?,
        ))
    })?;
    let mut count = 0;
    for row in rows {
        let (i, h, hop, o) = row?;
        count += 1;
        if count > LIMIT {
            return Err(LedgerError::InvalidLedger);
        }
        read(
            c,
            i.try_into().map_err(|_| LedgerError::InvalidLedger)?,
            h.try_into().map_err(|_| LedgerError::InvalidLedger)?,
            hop,
            o.try_into().map_err(|_| LedgerError::InvalidLedger)?,
        )?
        .ok_or(LedgerError::InvalidLedger)?;
    }
    Ok(())
}
pub(super) const SCHEMA:&str="
CREATE TABLE origin_operations(instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),hop INTEGER NOT NULL CHECK(hop>0 AND hop<=65535),operation BLOB NOT NULL CHECK(length(operation)=16),nonce BLOB NOT NULL CHECK(length(nonce)=16),kind INTEGER NOT NULL CHECK(kind IN(0,1,2)),payload BLOB NOT NULL CHECK(length(payload)<=2048),command TEXT NOT NULL CHECK(length(command)<=1024),claimed INTEGER NOT NULL CHECK(claimed IN(0,1)),reserve_bytes INTEGER NOT NULL CHECK(reserve_bytes=262144),PRIMARY KEY(instance,handle,hop,operation),UNIQUE(instance,handle,hop,nonce));
CREATE UNIQUE INDEX origin_initial_identity ON origin_operations(instance,handle,hop,kind) WHERE kind IN(0,1);
";

#[cfg(test)]
mod tests {
    use super::*;
    fn operation() -> InitialOperation {
        InitialOperation {
            binding: OriginBinding {
                instance: [1; 16],
                handle: [2; 16],
                hops: 3,
                revision: 1,
            },
            hop: 1,
            operation: [3; 16],
            nonce: [4; 16],
            kind: InitialOperationKind::Start,
            terms: None,
        }
    }
    #[test]
    fn restart_keeps_identity_and_dispatch_latch() {
        let path =
            std::env::temp_dir().join(format!("eltor-origin-{}.sqlite", rand::random::<u128>()));
        let account = AccountId([9; 32]);
        let mut ledger = PaymentLedger::provision(&path, account).unwrap();
        let op = operation();
        ledger.save_origin_operation(&op).unwrap();
        ledger
            .claim_origin_dispatch(op.binding.instance, op.binding.handle, op.hop, op.operation)
            .unwrap();
        drop(ledger);
        let mut ledger = PaymentLedger::reopen(&path, account).unwrap();
        let mut observed = op.clone();
        observed.binding.revision = 100;
        let recovered = ledger.save_origin_operation(&observed).unwrap();
        assert!(recovered.dispatch_claimed);
        assert_eq!(recovered.operation.binding.revision, 1);
        assert!(matches!(
            ledger.claim_origin_dispatch(
                op.binding.instance,
                op.binding.handle,
                op.hop,
                op.operation
            ),
            Err(LedgerError::DispatchAlreadyClaimed)
        ));
        let mut replaced = op.clone();
        replaced.operation = [5; 16];
        replaced.nonce = [6; 16];
        assert!(matches!(
            ledger.save_origin_operation(&replaced),
            Err(LedgerError::Conflict)
        ));
        assert!(ledger.storage_status().unwrap().reserved_write_bytes >= 262144);
        drop(ledger);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn closure_blocks_unsent_operation_and_isolates_other_handle() {
        let path =
            std::env::temp_dir().join(format!("eltor-origin-{}.sqlite", rand::random::<u128>()));
        let mut ledger = PaymentLedger::provision(&path, AccountId([9; 32])).unwrap();
        let op = operation();
        ledger.save_origin_operation(&op).unwrap();
        ledger
            .close_binding(op.binding.instance, op.binding.handle)
            .unwrap();
        assert!(matches!(
            ledger.claim_origin_dispatch(
                op.binding.instance,
                op.binding.handle,
                op.hop,
                op.operation
            ),
            Err(LedgerError::Closed)
        ));
        let mut other = op.clone();
        other.binding.handle = [8; 16];
        ledger.save_origin_operation(&other).unwrap();
        ledger
            .claim_origin_dispatch(
                other.binding.instance,
                other.binding.handle,
                other.hop,
                other.operation,
            )
            .unwrap();
        drop(ledger);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn tor_restart_closes_origin_only_bindings_without_erasing_history() {
        let path =
            std::env::temp_dir().join(format!("eltor-origin-{}.sqlite", rand::random::<u128>()));
        let mut ledger = PaymentLedger::provision(&path, AccountId([9; 32])).unwrap();
        let op = operation();
        ledger.save_origin_operation(&op).unwrap();
        ledger.close_other_instances([8; 16]).unwrap();
        assert!(matches!(
            ledger.claim_origin_dispatch(
                op.binding.instance,
                op.binding.handle,
                op.hop,
                op.operation
            ),
            Err(LedgerError::Closed)
        ));
        assert!(ledger
            .origin_operation(op.binding.instance, op.binding.handle, op.hop, op.operation)
            .unwrap()
            .is_some());
        drop(ledger);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn independent_controllers_claim_once_and_corrupt_identity_fails_reopen() {
        let path =
            std::env::temp_dir().join(format!("eltor-origin-{}.sqlite", rand::random::<u128>()));
        let account = AccountId([9; 32]);
        let mut first = PaymentLedger::provision(&path, account).unwrap();
        let op = operation();
        first.save_origin_operation(&op).unwrap();
        let mut second = PaymentLedger::reopen(&path, account).unwrap();
        first
            .claim_origin_dispatch(op.binding.instance, op.binding.handle, op.hop, op.operation)
            .unwrap();
        assert!(matches!(
            second.claim_origin_dispatch(
                op.binding.instance,
                op.binding.handle,
                op.hop,
                op.operation
            ),
            Err(LedgerError::DispatchAlreadyClaimed)
        ));
        first
            .connection
            .execute(
                "UPDATE origin_operations SET nonce=?1",
                params![&[7u8; 16][..]],
            )
            .unwrap();
        drop(first);
        drop(second);
        assert!(matches!(
            PaymentLedger::reopen(&path, account),
            Err(LedgerError::InvalidLedger)
        ));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn explicit_v4_migration_preserves_account_and_requires_operator_action() {
        let path =
            std::env::temp_dir().join(format!("eltor-origin-{}.sqlite", rand::random::<u128>()));
        let account = AccountId([9; 32]);
        let ledger = PaymentLedger::provision(&path, account).unwrap();
        ledger.connection.execute_batch("DROP TABLE origin_outgoing_ids;DROP TABLE origin_allocations;DROP TABLE origin_renewal_operations;DROP TABLE origin_invoices;DROP TABLE origin_plans;DROP TABLE origin_constructions;DROP TABLE origin_operations;DROP TABLE native_invoice_candidates;DROP TABLE native_invoice_cursor;DROP TABLE closed_bindings;PRAGMA user_version=4;").unwrap();
        drop(ledger);
        assert!(matches!(
            PaymentLedger::reopen(&path, account),
            Err(LedgerError::InvalidLedger)
        ));
        assert!(matches!(
            PaymentLedger::upgrade_from_v4(&path, AccountId([8; 32])),
            Err(LedgerError::WrongAccount)
        ));
        let ledger = PaymentLedger::upgrade_from_v4(&path, account).unwrap();
        assert_eq!(ledger.native_invoice_cursor().unwrap(), 1);
        drop(ledger);
        std::fs::remove_file(path).unwrap();
    }
}
