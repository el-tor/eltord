//! Bounded private origin recovery artifacts. Constructor dispatch is latched
//! before the wire; an ambiguous result cannot yield another construction. Raw
//! outgoing invoice artifacts are retained only for exact lookup after restart.
use super::outgoing::OutgoingIntent;
use super::*;
const LIMIT: i64 = 4096;
const MAX_PLAN: usize = 65536;
const RESERVE: i64 = 262144;
#[derive(Clone)]
pub struct SavedOriginConstruction {
    pub instance: Id16,
    pub payload: Vec<u8>,
    pub dispatch_claimed: bool,
}
#[derive(Clone)]
pub struct SavedOriginPlan {
    pub local_budget_scope: Hash32,
    pub payload: Vec<u8>,
}
#[derive(Clone)]
pub struct SavedOriginInvoice {
    pub intent: OutgoingIntent,
    pub artifact: String,
}
impl PaymentLedger {
    pub fn save_origin_construction(
        &mut self,
        scope: Hash32,
        instance: Id16,
        payload: &[u8],
    ) -> Result<SavedOriginConstruction> {
        self.check_file_identity()?;
        if scope == [0; 32] || instance == [0; 16] || payload.is_empty() || payload.len() > MAX_PLAN
        {
            return Err(LedgerError::InvalidInput);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = construction(&tx, scope)? {
            return if old.instance == instance && old.payload == payload {
                Ok(old)
            } else {
                Err(LedgerError::Conflict)
            };
        }
        require_open_instance(&tx, instance)?;
        storage::check_admission(&tx)?;
        control_capacity(&tx)?;
        tx.execute("INSERT INTO origin_constructions(scope,instance,payload,digest,claimed,reserve_bytes) VALUES(?1,?2,?3,?4,0,?5)",params![&scope[..],&instance[..],payload,&digest(payload)[..],RESERVE])?;
        storage::check_admission(&tx)?;
        tx.commit()?;
        self.get_origin_construction(scope)?
            .ok_or(LedgerError::InvalidLedger)
    }
    pub fn get_origin_construction(
        &self,
        scope: Hash32,
    ) -> Result<Option<SavedOriginConstruction>> {
        self.check_file_identity()?;
        construction(&self.connection, scope)
    }
    pub fn claim_origin_construction(&mut self, scope: Hash32) -> Result<SavedOriginConstruction> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut saved = construction(&tx, scope)?.ok_or(LedgerError::NotRegistered)?;
        require_open_instance(&tx, saved.instance)?;
        if saved.dispatch_claimed {
            return Err(LedgerError::DispatchAlreadyClaimed);
        }
        storage::check_admission(&tx)?;
        tx.execute(
            "UPDATE origin_constructions SET claimed=1 WHERE scope=?1 AND claimed=0",
            params![&scope[..]],
        )?;
        tx.commit()?;
        saved.dispatch_claimed = true;
        Ok(saved)
    }
    pub fn save_origin_plan(
        &mut self,
        instance: Id16,
        handle: Id16,
        scope: Hash32,
        payload: &[u8],
    ) -> Result<SavedOriginPlan> {
        self.check_file_identity()?;
        if instance == [0; 16]
            || handle == [0; 16]
            || scope == [0; 32]
            || payload.is_empty()
            || payload.len() > MAX_PLAN
        {
            return Err(LedgerError::InvalidInput);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = plan(&tx, instance, handle)? {
            return if old.local_budget_scope == scope && old.payload == payload {
                Ok(old)
            } else {
                Err(LedgerError::Conflict)
            };
        }
        require_open_instance(&tx, instance)?;
        receiving::require_open_binding(&tx, instance, handle)?;
        let staged = construction(&tx, scope)?.ok_or(LedgerError::NotRegistered)?;
        if staged.instance != instance || !staged.dispatch_claimed {
            return Err(LedgerError::Conflict);
        }
        capacity(&tx, "origin_plans")?;
        storage::check_admission(&tx)?;
        let already: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM origin_plans WHERE scope=?1)",
            params![&scope[..]],
            |r| r.get(0),
        )?;
        if already {
            return Err(LedgerError::Conflict);
        }
        tx.execute(
            "INSERT INTO origin_plans(instance,handle,scope,payload,digest) VALUES(?1,?2,?3,?4,?5)",
            params![
                &instance[..],
                &handle[..],
                &scope[..],
                payload,
                &digest(payload)[..]
            ],
        )?;
        storage::check_admission(&tx)?;
        tx.commit()?;
        Ok(SavedOriginPlan {
            local_budget_scope: scope,
            payload: payload.to_vec(),
        })
    }
    pub fn get_origin_plan(&self, instance: Id16, handle: Id16) -> Result<Option<SavedOriginPlan>> {
        self.check_file_identity()?;
        plan(&self.connection, instance, handle)
    }
    /// Exact unique scope lookup for restart; no numeric Tor ID is inferred.
    pub fn get_origin_plan_by_scope(
        &self,
        scope: Hash32,
    ) -> Result<Option<(Id16, Id16, SavedOriginPlan)>> {
        self.check_file_identity()?;
        let row: Option<(Vec<u8>, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT instance,handle FROM origin_plans WHERE scope=?1",
                params![&scope[..]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(instance, handle)| {
            let instance: Id16 = instance
                .try_into()
                .map_err(|_| LedgerError::InvalidLedger)?;
            let handle: Id16 = handle.try_into().map_err(|_| LedgerError::InvalidLedger)?;
            let saved =
                plan(&self.connection, instance, handle)?.ok_or(LedgerError::InvalidLedger)?;
            if saved.local_budget_scope != scope {
                return Err(LedgerError::InvalidLedger);
            }
            Ok((instance, handle, saved))
        })
        .transpose()
    }
    pub fn save_origin_invoice(
        &mut self,
        intent: &OutgoingIntent,
        artifact: &str,
    ) -> Result<SavedOriginInvoice> {
        self.check_file_identity()?;
        outgoing::validate_intent(intent, self.account)?;
        if !valid_invoice(artifact) || digest(artifact.as_bytes()) != intent.invoice_digest {
            return Err(LedgerError::InvalidInput);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = invoice(&tx, intent.operation, self.account)? {
            return if old.intent == *intent && old.artifact == artifact {
                Ok(old)
            } else {
                Err(LedgerError::Conflict)
            };
        }
        require_open_instance(&tx, intent.instance)?;
        receiving::require_open_binding(&tx, intent.instance, intent.handle)?;
        let plan = plan(&tx, intent.instance, intent.handle)?.ok_or(LedgerError::NotRegistered)?;
        if plan.local_budget_scope != intent.local_budget_scope {
            return Err(LedgerError::Conflict);
        }
        capacity(&tx, "origin_invoices")?;
        storage::check_admission(&tx)?;
        tx.execute("INSERT INTO origin_invoices(operation,instance,handle,intent,artifact,digest,reserve_bytes) VALUES(?1,?2,?3,?4,?5,?6,16384)",params![&intent.operation[..],&intent.instance[..],&intent.handle[..],encode(intent)?,artifact,&intent.invoice_digest[..]])?;
        storage::check_admission(&tx)?;
        tx.commit()?;
        Ok(SavedOriginInvoice {
            intent: intent.clone(),
            artifact: artifact.to_owned(),
        })
    }
    pub fn get_origin_invoice(&self, operation: Id16) -> Result<Option<SavedOriginInvoice>> {
        self.check_file_identity()?;
        invoice(&self.connection, operation, self.account)
    }
}
fn digest(payload: &[u8]) -> Hash32 {
    Sha256::digest(payload).into()
}
fn valid_invoice(artifact: &str) -> bool {
    !artifact.is_empty()
        && artifact.len() <= 2048
        && artifact.bytes().all(|b| (33..=126).contains(&b))
        && (artifact.starts_with("lni1")
            || artifact.starts_with("lnbc")
            || artifact.starts_with("lntb")
            || artifact.starts_with("lntbs"))
}
pub(super) fn control_capacity(connection: &Connection) -> Result<()> {
    let count:i64=connection.query_row("SELECT (SELECT count(*) FROM origin_constructions)+(SELECT count(*) FROM origin_operations)+(SELECT count(*) FROM origin_renewal_operations)",[],|r|r.get(0))?;
    if count >= 4096 {
        Err(LedgerError::BudgetExceeded)
    } else {
        Ok(())
    }
}
fn capacity(connection: &Connection, table: &str) -> Result<()> {
    // table names are internal literals, never caller data.
    let count: i64 =
        connection.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
    if count >= LIMIT {
        Err(LedgerError::BudgetExceeded)
    } else {
        Ok(())
    }
}
fn construction(connection: &Connection, scope: Hash32) -> Result<Option<SavedOriginConstruction>> {
    let row:Option<(Vec<u8>,Vec<u8>,Vec<u8>,i64,i64)>=connection.query_row("SELECT instance,payload,digest,claimed,reserve_bytes FROM origin_constructions WHERE scope=?1",params![&scope[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    row.map(|(instance, payload, saved, claimed, reserve)| {
        let instance: Id16 = instance
            .try_into()
            .map_err(|_| LedgerError::InvalidLedger)?;
        if scope == [0; 32]
            || instance == [0; 16]
            || payload.is_empty()
            || payload.len() > MAX_PLAN
            || saved != digest(&payload)
            || ![0, 1].contains(&claimed)
            || reserve != RESERVE
        {
            return Err(LedgerError::InvalidLedger);
        }
        Ok(SavedOriginConstruction {
            instance,
            payload,
            dispatch_claimed: claimed == 1,
        })
    })
    .transpose()
}
fn plan(connection: &Connection, instance: Id16, handle: Id16) -> Result<Option<SavedOriginPlan>> {
    let row: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = connection
        .query_row(
            "SELECT scope,payload,digest FROM origin_plans WHERE instance=?1 AND handle=?2",
            params![&instance[..], &handle[..]],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    row.map(|(scope, payload, saved)| {
        let scope: Hash32 = scope.try_into().map_err(|_| LedgerError::InvalidLedger)?;
        let stage = construction(connection, scope)?.ok_or(LedgerError::InvalidLedger)?;
        if instance == [0; 16]
            || handle == [0; 16]
            || stage.instance != instance
            || !stage.dispatch_claimed
            || payload.is_empty()
            || payload.len() > MAX_PLAN
            || saved != digest(&payload)
        {
            return Err(LedgerError::InvalidLedger);
        }
        Ok(SavedOriginPlan {
            local_budget_scope: scope,
            payload,
        })
    })
    .transpose()
}
fn invoice(
    connection: &Connection,
    operation: Id16,
    account: AccountId,
) -> Result<Option<SavedOriginInvoice>> {
    let row:Option<(Vec<u8>,Vec<u8>,Vec<u8>,String,Vec<u8>,i64)>=connection.query_row("SELECT instance,handle,intent,artifact,digest,reserve_bytes FROM origin_invoices WHERE operation=?1",params![&operation[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    row.map(|(instance, handle, payload, artifact, saved, reserve)| {
        let intent: OutgoingIntent =
            serde_json::from_slice(&payload).map_err(|_| LedgerError::InvalidLedger)?;
        outgoing::validate_intent(&intent, account).map_err(|_| LedgerError::InvalidLedger)?;
        let plan =
            plan(connection, intent.instance, intent.handle)?.ok_or(LedgerError::InvalidLedger)?;
        if intent.operation != operation
            || intent.instance != instance.as_slice()
            || intent.handle != handle.as_slice()
            || plan.local_budget_scope != intent.local_budget_scope
            || !valid_invoice(&artifact)
            || digest(artifact.as_bytes()) != intent.invoice_digest
            || saved != intent.invoice_digest
            || reserve != 16384
        {
            return Err(LedgerError::InvalidLedger);
        }
        Ok(SavedOriginInvoice { intent, artifact })
    })
    .transpose()
}
pub(super) fn require_managed_invoice(
    connection: &Connection,
    intent: &OutgoingIntent,
    account: AccountId,
) -> Result<()> {
    let managed: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM origin_plans WHERE scope=?1)",
        params![&intent.local_budget_scope[..]],
        |r| r.get(0),
    )?;
    if managed {
        let saved =
            invoice(connection, intent.operation, account)?.ok_or(LedgerError::NotRegistered)?;
        if saved.intent != *intent {
            return Err(LedgerError::Conflict);
        }
    }
    Ok(())
}
pub(super) fn validate_schema(connection: &Connection, account: AccountId) -> Result<()> {
    let control_count:i64=connection.query_row("SELECT (SELECT count(*) FROM origin_constructions)+(SELECT count(*) FROM origin_operations)+(SELECT count(*) FROM origin_renewal_operations)",[],|r|r.get(0))?;
    if control_count > 4096 {
        return Err(LedgerError::InvalidLedger);
    }
    connection.prepare("SELECT c.scope,p.scope,i.operation FROM origin_constructions c,origin_plans p,origin_invoices i LIMIT 0")?;
    for table in ["origin_constructions", "origin_plans", "origin_invoices"] {
        let count: i64 =
            connection.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        if count > LIMIT {
            return Err(LedgerError::InvalidLedger);
        }
    }
    let mut stmt = connection.prepare("SELECT scope FROM origin_constructions")?;
    for row in stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))? {
        construction(
            connection,
            row?.try_into().map_err(|_| LedgerError::InvalidLedger)?,
        )?
        .ok_or(LedgerError::InvalidLedger)?;
    }
    let mut stmt = connection.prepare("SELECT instance,handle FROM origin_plans")?;
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
    })? {
        let (i, h) = row?;
        plan(
            connection,
            i.try_into().map_err(|_| LedgerError::InvalidLedger)?,
            h.try_into().map_err(|_| LedgerError::InvalidLedger)?,
        )?
        .ok_or(LedgerError::InvalidLedger)?;
    }
    let mut stmt = connection.prepare("SELECT operation FROM origin_invoices")?;
    for row in stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))? {
        invoice(
            connection,
            row?.try_into().map_err(|_| LedgerError::InvalidLedger)?,
            account,
        )?
        .ok_or(LedgerError::InvalidLedger)?;
    }
    let mut stmt=connection.prepare("SELECT o.operation FROM outgoing_intents o JOIN origin_plans p ON p.scope=o.local_budget_scope")?;
    for row in stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))? {
        let operation: Id16 = row?.try_into().map_err(|_| LedgerError::InvalidLedger)?;
        let saved = invoice(connection, operation, account)?.ok_or(LedgerError::InvalidLedger)?;
        let existing: Vec<u8> = connection.query_row(
            "SELECT payload FROM outgoing_intents WHERE operation=?1",
            params![&operation[..]],
            |r| r.get(0),
        )?;
        if serde_json::from_slice::<OutgoingIntent>(&existing)
            .map_err(|_| LedgerError::InvalidLedger)?
            != saved.intent
        {
            return Err(LedgerError::InvalidLedger);
        }
    }
    Ok(())
}
pub(super) const SCHEMA:&str="
CREATE TABLE origin_constructions(scope BLOB PRIMARY KEY CHECK(length(scope)=32),instance BLOB NOT NULL CHECK(length(instance)=16),payload BLOB NOT NULL CHECK(length(payload)>0 AND length(payload)<=65536),digest BLOB NOT NULL CHECK(length(digest)=32),claimed INTEGER NOT NULL CHECK(claimed IN(0,1)),reserve_bytes INTEGER NOT NULL CHECK(reserve_bytes=262144));
CREATE TABLE origin_plans(instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),scope BLOB NOT NULL UNIQUE REFERENCES origin_constructions(scope),payload BLOB NOT NULL CHECK(length(payload)>0 AND length(payload)<=65536),digest BLOB NOT NULL CHECK(length(digest)=32),PRIMARY KEY(instance,handle));
CREATE TABLE origin_invoices(operation BLOB PRIMARY KEY CHECK(length(operation)=16),instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),intent BLOB NOT NULL CHECK(length(intent)<=8192),artifact TEXT NOT NULL CHECK(length(artifact)>0 AND length(artifact)<=2048),digest BLOB NOT NULL CHECK(length(digest)=32),reserve_bytes INTEGER NOT NULL CHECK(reserve_bytes=16384),FOREIGN KEY(instance,handle) REFERENCES origin_plans(instance,handle));
";
#[cfg(test)]
mod tests;
