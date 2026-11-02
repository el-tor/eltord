//! Typed origin allocation admission. A queued RPC or caller boolean cannot
//! authorize a new period; all hops retain their independent C/Q/IDs and prices.
use super::outgoing::{HopSpendingTerms, OutgoingIntent};
use super::*;
use crate::rpc::{OriginBinding, OriginRegistration, RenewalOperation, VerifiedRenewalAdmission};

#[derive(Clone)]
pub struct SavedOriginRenewalOperation {
    pub operation: RenewalOperation,
    pub dispatch_claimed: bool,
}
#[derive(Clone)]
pub struct SavedOriginAllocation {
    pub period: u64,
    pub first_round: u64,
    pub circuit_binding: Hash32,
    pub policy_hash: Hash32,
    pub initial_allocation: Hash32,
    pub parent: Hash32,
    pub request_digest: Hash32,
    pub allocation_digest: Hash32,
    pub payment_ids: [[u8; 32]; 10],
    pub operation: Option<Id16>,
    pub nonce: Option<Id16>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct OperationPayload {
    instance: Id16,
    handle: Id16,
    hops: u16,
    revision: u64,
    hop: u16,
    operation: Id16,
    period: u64,
    first_round: u64,
    nonce: Id16,
    terms: Hash32,
    parent: Hash32,
    ids: [[u8; 32]; 10],
}
impl OperationPayload {
    fn new(op: &RenewalOperation) -> Self {
        Self {
            instance: op.binding.instance,
            handle: op.binding.handle,
            hops: op.binding.hops,
            revision: op.binding.revision,
            hop: op.hop,
            operation: op.operation,
            period: op.period,
            first_round: op.first_round,
            nonce: op.nonce,
            terms: op.terms,
            parent: op.parent,
            ids: op.ids,
        }
    }
    fn decode(self) -> Result<RenewalOperation> {
        let op = RenewalOperation {
            binding: OriginBinding {
                instance: self.instance,
                handle: self.handle,
                hops: self.hops,
                revision: self.revision,
            },
            hop: self.hop,
            operation: self.operation,
            period: self.period,
            first_round: self.first_round,
            nonce: self.nonce,
            terms: self.terms,
            parent: self.parent,
            ids: self.ids,
        };
        op.validate().map_err(|_| LedgerError::InvalidLedger)?;
        Ok(op)
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Allocation {
    scope: Hash32,
    charge: Hash32,
    instance: Id16,
    handle: Id16,
    hop: u16,
    hops: u16,
    period: u64,
    c: Hash32,
    q: Hash32,
    policy: Vec<u8>,
    a0: Hash32,
    parent: Hash32,
    request: Hash32,
    allocation: Hash32,
    operation: Option<Id16>,
    nonce: Option<Id16>,
    ids: [[u8; 32]; 10],
}
impl PaymentLedger {
    pub fn register_origin_initial_allocation(
        &mut self,
        scope: Hash32,
        charge: Hash32,
        registration: &OriginRegistration,
    ) -> Result<()> {
        self.check_file_identity()?;
        let a = Allocation {
            scope,
            charge,
            instance: registration.binding.instance,
            handle: registration.binding.handle,
            hop: registration.hop,
            hops: registration.binding.hops,
            period: 0,
            c: registration.circuit_binding(),
            q: registration.policy_hash(),
            policy: registration.terms().to_vec(),
            a0: registration.initial_allocation(),
            parent: [0; 32],
            request: [0; 32],
            allocation: registration.initial_allocation(),
            operation: None,
            nonce: None,
            ids: *registration.payment_ids(),
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_allocation(&tx, &a)?;
        tx.commit()?;
        Ok(())
    }
    pub fn save_origin_renewal_operation(
        &mut self,
        op: &RenewalOperation,
    ) -> Result<SavedOriginRenewalOperation> {
        self.check_file_identity()?;
        op.validate().map_err(|_| LedgerError::InvalidInput)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = renewal_operation(
            &tx,
            op.binding.instance,
            op.binding.handle,
            op.hop,
            op.operation,
        )? {
            return if old.operation == *op {
                Ok(old)
            } else {
                Err(LedgerError::Conflict)
            };
        }
        require_open_instance(&tx, op.binding.instance)?;
        receiving::require_open_binding(&tx, op.binding.instance, op.binding.handle)?;
        let plan = plan_for_binding(&tx, op.binding.instance, op.binding.handle)?;
        let prior = allocation_for_hop(&tx, plan, op.hop, op.period - 1)?
            .ok_or(LedgerError::NotRegistered)?;
        if prior.q != op.terms || prior.allocation != op.parent || prior.hops != op.binding.hops {
            return Err(LedgerError::Conflict);
        }
        validate_period(&prior, op.period)?;
        let current = current_round(&tx, plan)?;
        if op.period != (current - 1) / 10 + 1 {
            return Err(LedgerError::InvalidInput);
        }
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM origin_renewal_operations WHERE (scope=?1 AND hop=?2 AND period=?3) OR (instance=?4 AND handle=?5 AND nonce=?6))",params![&plan[..],i64::from(op.hop),sql_number(op.period)?,&op.binding.instance[..],&op.binding.handle[..],&op.nonce[..]],|r|r.get(0))?;
        if duplicate {
            return Err(LedgerError::Conflict);
        }
        origin_artifacts::control_capacity(&tx)?;
        storage::check_admission(&tx)?;
        let command = op.command().map_err(|_| LedgerError::InvalidInput)?;
        tx.execute("INSERT INTO origin_renewal_operations(instance,handle,hop,operation,scope,period,nonce,payload,command,claimed,reserve_bytes) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,262144)",params![&op.binding.instance[..],&op.binding.handle[..],i64::from(op.hop),&op.operation[..],&plan[..],sql_number(op.period)?,&op.nonce[..],encode(&OperationPayload::new(op))?,command])?;
        storage::check_admission(&tx)?;
        tx.commit()?;
        Ok(SavedOriginRenewalOperation {
            operation: op.clone(),
            dispatch_claimed: false,
        })
    }
    pub fn get_origin_renewal_operation(
        &self,
        instance: Id16,
        handle: Id16,
        hop: u16,
        operation: Id16,
    ) -> Result<Option<SavedOriginRenewalOperation>> {
        self.check_file_identity()?;
        renewal_operation(&self.connection, instance, handle, hop, operation)
    }
    pub fn claim_origin_renewal_dispatch(
        &mut self,
        instance: Id16,
        handle: Id16,
        hop: u16,
        operation: Id16,
    ) -> Result<SavedOriginRenewalOperation> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut saved = renewal_operation(&tx, instance, handle, hop, operation)?
            .ok_or(LedgerError::NotRegistered)?;
        require_open_instance(&tx, instance)?;
        receiving::require_open_binding(&tx, instance, handle)?;
        if saved.dispatch_claimed {
            return Err(LedgerError::DispatchAlreadyClaimed);
        }
        storage::check_dispatch(&tx)?;
        tx.execute("UPDATE origin_renewal_operations SET claimed=1 WHERE instance=?1 AND handle=?2 AND hop=?3 AND operation=?4 AND claimed=0",params![&instance[..],&handle[..],i64::from(hop),&operation[..]])?;
        tx.commit()?;
        saved.dispatch_claimed = true;
        Ok(saved)
    }
    pub fn admit_origin_renewal(
        &mut self,
        scope: Hash32,
        charge: Hash32,
        proof: &VerifiedRenewalAdmission,
    ) -> Result<()> {
        self.check_file_identity()?;
        let first = read_allocation(&self.connection, scope, charge, 0)?
            .ok_or(LedgerError::NotRegistered)?;
        let a = Allocation {
            scope,
            charge,
            instance: proof.binding().instance,
            handle: proof.binding().handle,
            hop: proof.hop(),
            hops: proof.binding().hops,
            period: proof.period(),
            c: proof.circuit_binding(),
            q: proof.policy_hash(),
            policy: proof.terms().to_vec(),
            a0: first.a0,
            parent: proof.parent(),
            request: proof.request_digest(),
            allocation: proof.allocation_digest(),
            operation: Some(proof.operation().operation),
            nonce: Some(proof.nonce()),
            ids: *proof.payment_ids(),
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = read_allocation(&tx, scope, charge, a.period)? {
            return if old == a {
                Ok(())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        if proof.schedule().hard_age_remaining_ms == 0 {
            return Err(LedgerError::OutsideWindow);
        }
        let saved = renewal_operation(
            &tx,
            a.instance,
            a.handle,
            a.hop,
            proof.operation().operation,
        )?
        .ok_or(LedgerError::NotRegistered)?;
        if !saved.dispatch_claimed || saved.operation != *proof.operation() {
            return Err(LedgerError::Conflict);
        }
        insert_allocation(&tx, &a)?;
        tx.commit()?;
        Ok(())
    }
    /// Exact retained parent/allocation lookup, independent of live Tor state.
    pub fn get_origin_allocation(
        &self,
        scope: Hash32,
        charge: Hash32,
        period: u64,
    ) -> Result<Option<SavedOriginAllocation>> {
        self.check_file_identity()?;
        read_allocation(&self.connection, scope, charge, period)?
            .map(|a| {
                Ok(SavedOriginAllocation {
                    period: a.period,
                    first_round: a
                        .period
                        .checked_mul(10)
                        .and_then(|n| n.checked_add(1))
                        .ok_or(LedgerError::InvalidLedger)?,
                    circuit_binding: a.c,
                    policy_hash: a.q,
                    initial_allocation: a.a0,
                    parent: a.parent,
                    request_digest: a.request,
                    allocation_digest: a.allocation,
                    payment_ids: a.ids,
                    operation: a.operation,
                    nonce: a.nonce,
                })
            })
            .transpose()
    }
    pub fn origin_period_admitted(&self, scope: Hash32, period: u64) -> Result<bool> {
        self.check_file_identity()?;
        all_hops_admitted(&self.connection, scope, period)
    }
}
fn hash(domain: &[u8], parts: &[&[u8]]) -> Hash32 {
    let mut h = Sha256::new();
    h.update((domain.len() as u16).to_be_bytes());
    h.update(domain);
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}
fn raw_u64(policy: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(policy[at..at + 8].try_into().expect("checked policy"))
}
fn raw_u32(policy: &[u8], at: usize) -> u64 {
    u32::from_be_bytes(policy[at..at + 4].try_into().expect("checked policy")) as u64
}
fn validate(a: &Allocation) -> Result<()> {
    if a.scope == [0; 32]
        || a.charge == [0; 32]
        || a.instance == [0; 16]
        || a.handle == [0; 16]
        || a.c == [0; 32]
        || a.q == [0; 32]
        || a.a0 == [0; 32]
        || a.allocation == [0; 32]
        || a.hop == 0
        || a.hop > a.hops
        || a.policy.len() != 55
    {
        return Err(LedgerError::InvalidInput);
    }
    if a.policy[..3] != [1, 1, 10]
        || raw_u32(&a.policy, 11) != 60000
        || raw_u32(&a.policy, 15) != 60000
        || raw_u32(&a.policy, 19) > 60000
        || raw_u64(&a.policy, 31) == 0
        || raw_u32(&a.policy, 39) < 2
        || raw_u32(&a.policy, 51) == 0
        || hash(b"eltor/payment-renew/v1/terms", &[&a.policy]) != a.q
    {
        return Err(LedgerError::InvalidInput);
    }
    sql_number(raw_u64(&a.policy, 31))?;
    sql_number(a.period)?;
    for (n, id) in a.ids.iter().enumerate() {
        if *id == [0; 32] || a.ids[..n].contains(id) {
            return Err(LedgerError::InvalidInput);
        }
    }
    validate_period(a, a.period)?;
    if a.period == 0 {
        let ids: Vec<u8> = a.ids.iter().flatten().copied().collect();
        if a.parent != [0; 32]
            || a.request != [0; 32]
            || a.operation.is_some()
            || a.nonce.is_some()
            || a.allocation != a.a0
            || a.a0 != hash(b"eltor/payment-renew/v1/initial", &[&a.c, &a.q, &ids])
        {
            return Err(LedgerError::InvalidInput);
        }
    } else if a.parent == [0; 32]
        || a.request == [0; 32]
        || a.operation.map_or(true, |x| x == [0; 16])
        || a.nonce.map_or(true, |x| x == [0; 16])
    {
        return Err(LedgerError::InvalidInput);
    }
    Ok(())
}
fn validate_period(a: &Allocation, period: u64) -> Result<()> {
    if period >= raw_u32(&a.policy, 39)
        || (period > 0
            && period
                .checked_add(1)
                .and_then(|p| p.checked_mul(600000))
                .map_or(true, |end| end > raw_u64(&a.policy, 31)))
    {
        return Err(LedgerError::InvalidInput);
    }
    Ok(())
}
fn plan_for_binding(connection: &Connection, instance: Id16, handle: Id16) -> Result<Hash32> {
    let plan: Vec<u8> = connection
        .query_row(
            "SELECT scope FROM origin_plans WHERE instance=?1 AND handle=?2",
            params![&instance[..], &handle[..]],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(LedgerError::NotRegistered)?;
    plan.try_into().map_err(|_| LedgerError::InvalidLedger)
}
fn current_round(connection: &Connection, scope: Hash32) -> Result<u64> {
    let n: i64 = connection
        .query_row(
            "SELECT current_round FROM path_budgets WHERE local_budget_scope=?1",
            params![&scope[..]],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(LedgerError::NotRegistered)?;
    if n < 1 {
        return Err(LedgerError::InvalidLedger);
    }
    u64::try_from(n).map_err(|_| LedgerError::InvalidLedger)
}
fn check_terms(connection: &Connection, a: &Allocation) -> Result<()> {
    if plan_for_binding(connection, a.instance, a.handle)? != a.scope {
        return Err(LedgerError::Conflict);
    }
    let payload:Vec<u8>=connection.query_row("SELECT payload FROM hop_spending_terms WHERE local_budget_scope=?1 AND charge_reference=?2",params![&a.scope[..],&a.charge[..]],|r|r.get(0)).optional()?.ok_or(LedgerError::NotRegistered)?;
    let terms: HopSpendingTerms =
        serde_json::from_slice(&payload).map_err(|_| LedgerError::InvalidLedger)?;
    if terms.local_budget_scope != a.scope
        || terms.charge_reference != a.charge
        || terms.hop_circuit_binding != a.c
        || terms.hop_policy_hash != a.q
        || terms.round_amount_msat != raw_u64(&a.policy, 3)
        || terms.handshake_amount_msat != raw_u64(&a.policy, 43)
    {
        return Err(LedgerError::Conflict);
    }
    Ok(())
}
fn insert_allocation(tx: &rusqlite::Transaction<'_>, a: &Allocation) -> Result<()> {
    validate(a)?;
    if let Some(old) = read_allocation(tx, a.scope, a.charge, a.period)? {
        return if old == *a {
            Ok(())
        } else {
            Err(LedgerError::Conflict)
        };
    }
    if allocation_for_hop(tx, a.scope, a.hop, a.period)?.is_some() {
        return Err(LedgerError::Conflict);
    }
    require_open_instance(tx, a.instance)?;
    receiving::require_open_binding(tx, a.instance, a.handle)?;
    check_terms(tx, a)?;
    let current = current_round(tx, a.scope)?;
    if a.period > (current - 1) / 10 + 1 {
        return Err(LedgerError::InvalidInput);
    }
    if a.period > 0 {
        let prior = read_allocation(tx, a.scope, a.charge, a.period - 1)?
            .ok_or(LedgerError::NotRegistered)?;
        if a.parent != prior.allocation
            || a.c != prior.c
            || a.q != prior.q
            || a.policy != prior.policy
            || a.a0 != prior.a0
            || a.hop != prior.hop
            || a.hops != prior.hops
        {
            return Err(LedgerError::Conflict);
        }
    }
    let count: i64 = tx.query_row("SELECT count(*) FROM origin_allocations", [], |r| r.get(0))?;
    if count >= 4096 {
        return Err(LedgerError::BudgetExceeded);
    }
    storage::check_admission(tx)?;
    for id in a.ids {
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM origin_outgoing_ids WHERE payment_id=?1) OR EXISTS(SELECT 1 FROM outgoing_intents WHERE payment_id=?1) OR EXISTS(SELECT 1 FROM payment_ids WHERE payment_id=?1)",params![&id[..]],|r|r.get(0))?;
        if duplicate {
            return Err(LedgerError::Conflict);
        }
    }
    let payload = encode(a)?;
    let digest: Hash32 = Sha256::digest(&payload).into();
    tx.execute("INSERT INTO origin_allocations(scope,charge,hop,period,payload,digest,reserve_bytes) VALUES(?1,?2,?3,?4,?5,?6,65536)",params![&a.scope[..],&a.charge[..],i64::from(a.hop),sql_number(a.period)?,payload,&digest[..]])?;
    for (n, id) in a.ids.iter().enumerate() {
        let round = a
            .period
            .checked_mul(10)
            .and_then(|r| r.checked_add(n as u64 + 1))
            .ok_or(LedgerError::InvalidInput)?;
        tx.execute("INSERT INTO origin_outgoing_ids(payment_id,scope,charge,period,round) VALUES(?1,?2,?3,?4,?5)",params![&id[..],&a.scope[..],&a.charge[..],sql_number(a.period)?,sql_number(round)?])?;
    }
    storage::check_admission(tx)?;
    Ok(())
}
fn decode(payload: &[u8], digest: &[u8]) -> Result<Allocation> {
    if <[u8; 32]>::from(Sha256::digest(payload)) != digest {
        return Err(LedgerError::InvalidLedger);
    }
    let a: Allocation = serde_json::from_slice(payload).map_err(|_| LedgerError::InvalidLedger)?;
    validate(&a).map_err(|_| LedgerError::InvalidLedger)?;
    Ok(a)
}
fn read_allocation(
    connection: &Connection,
    scope: Hash32,
    charge: Hash32,
    period: u64,
) -> Result<Option<Allocation>> {
    let row:Option<(i64,Vec<u8>,Vec<u8>,i64)>=connection.query_row("SELECT hop,payload,digest,reserve_bytes FROM origin_allocations WHERE scope=?1 AND charge=?2 AND period=?3",params![&scope[..],&charge[..],sql_number(period)?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    row.map(|(hop,payload,digest,reserve)|{
        let a=decode(&payload,&digest)?;
        if a.scope!=scope || a.charge!=charge || a.period!=period || i64::from(a.hop)!=hop || reserve!=65536 {return Err(LedgerError::InvalidLedger);}check_terms(connection,&a).map_err(|_|LedgerError::InvalidLedger)?;
        if period>0 {
            let (payload,digest):(Vec<u8>,Vec<u8>)=connection.query_row("SELECT payload,digest FROM origin_allocations WHERE scope=?1 AND charge=?2 AND period=?3",params![&scope[..],&charge[..],sql_number(period-1)?],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let prior=decode(&payload,&digest)?;
            let saved=renewal_operation(connection,a.instance,a.handle,a.hop,a.operation.ok_or(LedgerError::InvalidLedger)?)?.ok_or(LedgerError::InvalidLedger)?;
            if prior.scope!=scope || prior.charge!=charge || prior.period!=period-1 || a.parent!=prior.allocation || a.c!=prior.c || a.q!=prior.q || a.policy!=prior.policy || a.a0!=prior.a0 || a.hop!=prior.hop || a.hops!=prior.hops || !saved.dispatch_claimed || saved.operation.ids!=a.ids || saved.operation.terms!=a.q || saved.operation.parent!=a.parent || saved.operation.nonce!=a.nonce.ok_or(LedgerError::InvalidLedger)? || saved.operation.expected_allocation(a.c).map_err(|_|LedgerError::InvalidLedger)?!=a.allocation {return Err(LedgerError::InvalidLedger);}
        }
        for (n,id) in a.ids.iter().enumerate() {
            let correct:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM origin_outgoing_ids WHERE payment_id=?1 AND scope=?2 AND charge=?3 AND period=?4 AND round=?5)",params![&id[..],&a.scope[..],&a.charge[..],sql_number(a.period)?,sql_number(a.period*10+n as u64+1)?],|r|r.get(0))?;
            if !correct {return Err(LedgerError::InvalidLedger);}
        }
        Ok(a)
    }).transpose()
}
fn allocation_for_hop(
    connection: &Connection,
    scope: Hash32,
    hop: u16,
    period: u64,
) -> Result<Option<Allocation>> {
    let charge: Option<Vec<u8>> = connection
        .query_row(
            "SELECT charge FROM origin_allocations WHERE scope=?1 AND hop=?2 AND period=?3",
            params![&scope[..], i64::from(hop), sql_number(period)?],
            |r| r.get(0),
        )
        .optional()?;
    charge
        .map(|c| {
            read_allocation(
                connection,
                scope,
                c.try_into().map_err(|_| LedgerError::InvalidLedger)?,
                period,
            )?
            .ok_or(LedgerError::InvalidLedger)
        })
        .transpose()
}
fn renewal_operation(
    connection: &Connection,
    instance: Id16,
    handle: Id16,
    hop: u16,
    operation: Id16,
) -> Result<Option<SavedOriginRenewalOperation>> {
    let row:Option<(Vec<u8>,i64,Vec<u8>,Vec<u8>,String,i64,i64)>=connection.query_row("SELECT scope,period,nonce,payload,command,claimed,reserve_bytes FROM origin_renewal_operations WHERE instance=?1 AND handle=?2 AND hop=?3 AND operation=?4",params![&instance[..],&handle[..],i64::from(hop),&operation[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
    row.map(
        |(scope, period, nonce, payload, command, claimed, reserve)| {
            let saved: OperationPayload =
                serde_json::from_slice(&payload).map_err(|_| LedgerError::InvalidLedger)?;
            let op = saved.decode()?;
            if op.binding.instance != instance
                || op.binding.handle != handle
                || op.hop != hop
                || op.operation != operation
                || op.period != u64::try_from(period).map_err(|_| LedgerError::InvalidLedger)?
                || op.nonce.as_slice() != nonce
                || scope != plan_for_binding(connection, instance, handle)?
                || ![0, 1].contains(&claimed)
                || reserve != 262144
                || command != op.command().map_err(|_| LedgerError::InvalidLedger)?
            {
                return Err(LedgerError::InvalidLedger);
            }
            Ok(SavedOriginRenewalOperation {
                operation: op,
                dispatch_claimed: claimed == 1,
            })
        },
    )
    .transpose()
}
fn all_hops_admitted(connection: &Connection, scope: Hash32, period: u64) -> Result<bool> {
    let mut stmt = connection.prepare(
        "SELECT charge FROM origin_allocations WHERE scope=?1 AND period=?2 ORDER BY hop",
    )?;
    let charges: Vec<Vec<u8>> = stmt
        .query_map(params![&scope[..], sql_number(period)?], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    if charges.is_empty() {
        return Ok(false);
    }
    let mut expected = None;
    for charge in &charges {
        let a = read_allocation(
            connection,
            scope,
            charge
                .clone()
                .try_into()
                .map_err(|_| LedgerError::InvalidLedger)?,
            period,
        )?
        .ok_or(LedgerError::InvalidLedger)?;
        if expected.is_some_and(|h| h != a.hops) {
            return Err(LedgerError::InvalidLedger);
        }
        expected = Some(a.hops);
    }
    let hops = expected.ok_or(LedgerError::InvalidLedger)?;
    let configured: i64 = connection.query_row(
        "SELECT count(*) FROM hop_spending_terms WHERE local_budget_scope=?1",
        params![&scope[..]],
        |r| r.get(0),
    )?;
    Ok(charges.len() == usize::from(hops) && configured == i64::from(hops))
}
pub(super) fn require_new_hop_slot(connection: &Connection, scope: Hash32) -> Result<()> {
    let row: Option<(Vec<u8>, Vec<u8>)> = connection
        .query_row(
            "SELECT payload,digest FROM origin_allocations WHERE scope=?1 AND period=0 LIMIT 1",
            params![&scope[..]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((payload, digest)) = row {
        let a = decode(&payload, &digest)?;
        let configured: i64 = connection.query_row(
            "SELECT count(*) FROM hop_spending_terms WHERE local_budget_scope=?1",
            params![&scope[..]],
            |r| r.get(0),
        )?;
        if configured >= i64::from(a.hops) {
            return Err(LedgerError::Conflict);
        }
    }
    Ok(())
}
pub(super) fn check_intent(
    connection: &Connection,
    intent: &OutgoingIntent,
    current: u64,
) -> Result<()> {
    if (intent.round == 0 && current != 1)
        || (intent.round > 0
            && (intent.round < current || intent.round > current.saturating_add(1)))
    {
        return Err(LedgerError::InvalidInput);
    }
    let managed: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM origin_plans WHERE scope=?1)",
        params![&intent.local_budget_scope[..]],
        |r| r.get(0),
    )?;
    if !managed {
        return if intent.round <= 10 {
            Ok(())
        } else {
            Err(LedgerError::NotRegistered)
        };
    }
    let period = intent.round.saturating_sub(1) / 10;
    if !all_hops_admitted(connection, intent.local_budget_scope, period)? {
        return Err(LedgerError::NotRegistered);
    }
    let a = read_allocation(
        connection,
        intent.local_budget_scope,
        intent.charge_reference,
        period,
    )?
    .ok_or(LedgerError::NotRegistered)?;
    if a.instance != intent.instance
        || a.handle != intent.handle
        || a.c != intent.hop_circuit_binding
        || a.q != intent.hop_policy_hash
        || (intent.round > 0 && a.ids[((intent.round - 1) % 10) as usize] != intent.payment_id)
    {
        return Err(LedgerError::Conflict);
    }
    Ok(())
}
pub(super) fn check_advance(connection: &Connection, scope: Hash32, round: u64) -> Result<()> {
    if round > 10 && !all_hops_admitted(connection, scope, (round - 1) / 10)? {
        return Err(LedgerError::NotRegistered);
    }
    Ok(())
}
pub(super) fn validate_schema(connection: &Connection) -> Result<()> {
    connection.prepare("SELECT a.digest,i.payment_id,r.command FROM origin_allocations a,origin_outgoing_ids i,origin_renewal_operations r LIMIT 0")?;
    let count: i64 =
        connection.query_row("SELECT count(*) FROM origin_allocations", [], |r| r.get(0))?;
    if count > 4096 {
        return Err(LedgerError::InvalidLedger);
    }
    let ids: i64 =
        connection.query_row("SELECT count(*) FROM origin_outgoing_ids", [], |r| r.get(0))?;
    if ids != count * 10 {
        return Err(LedgerError::InvalidLedger);
    }
    let mut stmt = connection.prepare("SELECT scope,charge,period FROM origin_allocations")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, Vec<u8>>(0)?,
            r.get::<_, Vec<u8>>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })? {
        let (scope, charge, period) = row?;
        let a = read_allocation(
            connection,
            scope.try_into().map_err(|_| LedgerError::InvalidLedger)?,
            charge.try_into().map_err(|_| LedgerError::InvalidLedger)?,
            u64::try_from(period).map_err(|_| LedgerError::InvalidLedger)?,
        )?
        .ok_or(LedgerError::InvalidLedger)?;
        for (n, id) in a.ids.iter().enumerate() {
            let correct:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM origin_outgoing_ids WHERE payment_id=?1 AND scope=?2 AND charge=?3 AND period=?4 AND round=?5)",params![&id[..],&a.scope[..],&a.charge[..],sql_number(a.period)?,sql_number(a.period*10+n as u64+1)?],|r|r.get(0))?;
            if !correct {
                return Err(LedgerError::InvalidLedger);
            }
        }
    }
    let mut stmt = connection
        .prepare("SELECT instance,handle,hop,operation FROM origin_renewal_operations")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, Vec<u8>>(0)?,
            r.get::<_, Vec<u8>>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Vec<u8>>(3)?,
        ))
    })? {
        let (i, h, hop, op) = row?;
        renewal_operation(
            connection,
            i.try_into().map_err(|_| LedgerError::InvalidLedger)?,
            h.try_into().map_err(|_| LedgerError::InvalidLedger)?,
            u16::try_from(hop).map_err(|_| LedgerError::InvalidLedger)?,
            op.try_into().map_err(|_| LedgerError::InvalidLedger)?,
        )?
        .ok_or(LedgerError::InvalidLedger)?;
    }
    let mut stmt = connection.prepare(
        "SELECT local_budget_scope,current_round FROM path_budgets WHERE current_round>10",
    )?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?)))? {
        let (scope, round) = row?;
        check_advance(
            connection,
            scope.try_into().map_err(|_| LedgerError::InvalidLedger)?,
            u64::try_from(round).map_err(|_| LedgerError::InvalidLedger)?,
        )
        .map_err(|_| LedgerError::InvalidLedger)?;
    }
    let mut stmt=connection.prepare("SELECT o.payload FROM outgoing_intents o JOIN origin_plans p ON p.scope=o.local_budget_scope")?;
    for row in stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))? {
        let i: OutgoingIntent =
            serde_json::from_slice(&row?).map_err(|_| LedgerError::InvalidLedger)?;
        check_intent(connection, &i, i.round.max(1)).map_err(|_| LedgerError::InvalidLedger)?;
    }
    Ok(())
}
/// Rebuild the legacy ten-round CHECK without touching any referencing rows.
/// Explicit migrations disable FK actions on this connection before BEGIN and
/// validate all references again on reopen; every change rolls back together.
pub(super) fn initialize(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute_batch("CREATE TABLE path_budgets_v6(local_budget_scope BLOB PRIMARY KEY CHECK(length(local_budget_scope)=32),instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),payload BLOB NOT NULL,current_round INTEGER NOT NULL CHECK(current_round>=1),closed INTEGER NOT NULL CHECK(closed IN(0,1)),UNIQUE(instance,handle));INSERT INTO path_budgets_v6 SELECT * FROM path_budgets;DROP TABLE path_budgets;ALTER TABLE path_budgets_v6 RENAME TO path_budgets;")?;
    tx.execute_batch(SCHEMA)?;
    Ok(())
}
pub(super) const SCHEMA:&str="
CREATE TABLE origin_renewal_operations(instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),hop INTEGER NOT NULL CHECK(hop>0 AND hop<=65535),operation BLOB NOT NULL CHECK(length(operation)=16),scope BLOB NOT NULL REFERENCES origin_constructions(scope),period INTEGER NOT NULL CHECK(period>0),nonce BLOB NOT NULL CHECK(length(nonce)=16),payload BLOB NOT NULL CHECK(length(payload)<=8192),command TEXT NOT NULL CHECK(length(command)<=2048),claimed INTEGER NOT NULL CHECK(claimed IN(0,1)),reserve_bytes INTEGER NOT NULL CHECK(reserve_bytes=262144),PRIMARY KEY(instance,handle,hop,operation),UNIQUE(scope,hop,period),UNIQUE(instance,handle,nonce));
CREATE TABLE origin_allocations(scope BLOB NOT NULL REFERENCES path_budgets(local_budget_scope),charge BLOB NOT NULL,hop INTEGER NOT NULL CHECK(hop>0 AND hop<=65535),period INTEGER NOT NULL CHECK(period>=0),payload BLOB NOT NULL CHECK(length(payload)<=8192),digest BLOB NOT NULL CHECK(length(digest)=32),reserve_bytes INTEGER NOT NULL CHECK(reserve_bytes=65536),PRIMARY KEY(scope,charge,period),UNIQUE(scope,hop,period),FOREIGN KEY(scope,charge) REFERENCES hop_spending_terms(local_budget_scope,charge_reference));
CREATE TABLE origin_outgoing_ids(payment_id BLOB PRIMARY KEY CHECK(length(payment_id)=32),scope BLOB NOT NULL,charge BLOB NOT NULL,period INTEGER NOT NULL,round INTEGER NOT NULL CHECK(round>0),UNIQUE(scope,charge,round),FOREIGN KEY(scope,charge,period) REFERENCES origin_allocations(scope,charge,period));
";
#[cfg(test)]
mod tests;
