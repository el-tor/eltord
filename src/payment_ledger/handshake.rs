//! One-shot preconstruction fees. No Tor handle/C exists yet; the immutable
//! local scope is genuine accounting state, never a fabricated wire binding.
//! Principal+maximum fees share account/path exposure with later round intents.
//! Signed invoice artifacts are retained for exact lookup; preimages never are.
use super::{outgoing::*, *};
use crate::rpc::{FoundationPolicy, OriginRegistration};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandshakeHop {
    pub fingerprint: [u8; 20],
    pub terms: Vec<u8>,
    pub offer: Option<String>,
    pub operation: Id16,
    pub payment_id: Hash32,
    pub initial_payment_ids: [[u8; 32]; 10],
    pub fee_limit_msat: u64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandshakeScope {
    pub scope: Hash32,
    pub instance: Id16,
    pub account: AccountId,
    pub lifetime_limit_msat: u64,
    pub outstanding_limit_msat: u64,
    pub per_round_limit_msat: u64,
    /// Explicit finite accepted funding interval, independent of the relay's
    /// setup clock which starts only at construction. Never renewed on retry.
    pub window: AcceptanceWindow,
    pub hops: Vec<HandshakeHop>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandshakeInvoice {
    pub scope: Hash32,
    pub hop: u16,
    pub invoice_digest: Hash32,
    pub payment_hash: Hash32,
    pub artifact: String,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandshakeEvidence {
    pub financial: OutgoingEvidence,
    pub settled_earliest_ms: Option<i64>,
    pub settled_latest_ms: Option<i64>,
}
#[derive(Clone, PartialEq, Eq)]
pub struct HandshakeRecord {
    pub invoice: HandshakeInvoice,
    pub state: OutgoingState,
    pub closed: bool,
    pub reserved_msat: u64,
    pub spent_msat: u64,
    pub outstanding_msat: u64,
    pub fee_limit_breached: bool,
    pub evidence: Option<HandshakeEvidence>,
}
/// Deliberately not Clone. Only a committed latch yields this native-pay permit.
pub struct HandshakeDispatchPermit {
    pub scope: HandshakeScope,
    pub invoice: HandshakeInvoice,
}
pub struct HandshakePathHop {
    pub registration: OriginRegistration,
    pub charge_reference: Hash32,
    pub payee_reference: Hash32,
    pub fee_limit_msat: u64,
}
impl PaymentLedger {
    pub fn save_handshake_scope(&mut self, scope: &HandshakeScope) -> Result<()> {
        self.check_file_identity()?;
        validate_scope(scope, self.account)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = read_scope(&tx, scope.scope, self.account)? {
            return if old == *scope {
                Ok(())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        require_open_instance(&tx, scope.instance)?;
        let (budget, blocked) = outgoing::account_budget(&tx)?;
        if blocked {
            return Err(LedgerError::BudgetExceeded);
        }
        let maximum = scope.hops.iter().try_fold(0u64, |n, h| {
            let policy = policy(h)?;
            n.checked_add(policy.handshake_fee_msat)
                .and_then(|n| {
                    n.checked_add(if policy.handshake_fee_msat > 0 {
                        h.fee_limit_msat
                    } else {
                        0
                    })
                })
                .ok_or(LedgerError::InvalidInput)
        })?;
        // Admission verifies that the full known setup can fit. Individual
        // invoice writes atomically reserve it against concurrent wallet work.
        outgoing::enforce_exposure(
            outgoing::exposure(&tx, None, None)?,
            maximum,
            budget.lifetime_limit_msat,
            budget.outstanding_limit_msat,
        )?;
        if maximum > scope.lifetime_limit_msat || maximum > scope.outstanding_limit_msat {
            return Err(LedgerError::BudgetExceeded);
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM path_budgets WHERE local_budget_scope=?1)",
            params![&scope.scope[..]],
            |r| r.get(0),
        )?;
        if exists {
            return Err(LedgerError::Conflict);
        }
        origin_artifacts::control_capacity(&tx)?;
        storage::check_admission(&tx)?;
        let payload = encode(scope)?;
        tx.execute("INSERT INTO handshake_scopes(scope,instance,payload,digest,closed,reserve_bytes) VALUES(?1,?2,?3,?4,0,262144)",params![&scope.scope[..],&scope.instance[..],&payload,&hash(&payload)[..]])?;
        for (index, hop) in scope.hops.iter().enumerate() {
            let duplicate: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM outgoing_intents WHERE operation=?1 OR payment_id=?2)",
                params![&hop.operation[..], &hop.payment_id[..]],
                |r| r.get(0),
            )?;
            if duplicate {
                return Err(LedgerError::Conflict);
            }
            tx.execute(
                "INSERT INTO handshake_hops(scope,hop,operation,payment_id) VALUES(?1,?2,?3,?4)",
                params![
                    &scope.scope[..],
                    index as i64 + 1,
                    &hop.operation[..],
                    &hop.payment_id[..]
                ],
            )?;
        }
        for (index, hop) in scope.hops.iter().enumerate() {
            for (round, id) in hop.initial_payment_ids.iter().enumerate() {
                let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM outgoing_intents WHERE payment_id=?1) OR EXISTS(SELECT 1 FROM payment_ids WHERE payment_id=?1)",params![&id[..]],|r|r.get(0))?;
                if duplicate {
                    return Err(LedgerError::Conflict);
                }
                tx.execute("INSERT INTO handshake_initial_ids(scope,hop,round,payment_id) VALUES(?1,?2,?3,?4)",params![&scope.scope[..],index as i64+1,round as i64+1,&id[..]])?;
            }
        }
        storage::check_admission(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub fn instance_is_closed(&self, instance: Id16) -> Result<bool> {
        self.check_file_identity()?;
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM closed_instances WHERE instance=?1)",
            params![&instance[..]],
            |r| r.get(0),
        )?)
    }
    pub fn get_handshake_scope(&self, scope: Hash32) -> Result<Option<HandshakeScope>> {
        self.check_file_identity()?;
        read_scope(&self.connection, scope, self.account)
    }
    /// Fetch-only native invoice request is latched once too. A lost reply must
    /// not mint an unlimited stream of invoices with the same payer note.
    pub fn claim_handshake_invoice_request(&mut self, scope: Hash32, index: u16) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let saved = read_scope(&tx, scope, self.account)?.ok_or(LedgerError::NotRegistered)?;
        if policy(hop(&saved, index)?)?.handshake_fee_msat == 0 {
            return Err(LedgerError::InvalidInput);
        }
        require_live_scope(&tx, &saved)?;
        let claimed: bool = tx.query_row(
            "SELECT fetch_claimed FROM handshake_hops WHERE scope=?1 AND hop=?2",
            params![&scope[..], i64::from(index)],
            |r| r.get(0),
        )?;
        if claimed {
            return Err(LedgerError::DispatchAlreadyClaimed);
        }
        storage::check_dispatch(&tx)?;
        tx.execute(
            "UPDATE handshake_hops SET fetch_claimed=1 WHERE scope=?1 AND hop=?2",
            params![&scope[..], i64::from(index)],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Artifact and maximum financial reservation commit together before pay.
    pub fn prepare_handshake_invoice(
        &mut self,
        invoice: &HandshakeInvoice,
        clock: ClockObservation,
    ) -> Result<HandshakeRecord> {
        self.check_file_identity()?;
        validate_invoice(invoice)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = read_record(&tx, invoice.scope, invoice.hop)? {
            return if old.invoice == *invoice {
                Ok(old)
            } else {
                Err(LedgerError::Conflict)
            };
        }
        let scope =
            read_scope(&tx, invoice.scope, self.account)?.ok_or(LedgerError::NotRegistered)?;
        let hop = hop(&scope, invoice.hop)?;
        let amount = policy(hop)?.handshake_fee_msat;
        if amount == 0 {
            return Err(LedgerError::InvalidInput);
        }
        require_live_scope(&tx, &scope)?;
        certain_time(clock.now_ms, clock.uncertainty_ms, scope.window)?;
        let (account, blocked) = outgoing::account_budget(&tx)?;
        if blocked {
            return Err(LedgerError::BudgetExceeded);
        }
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM outgoing_intents WHERE payment_hash=?1 OR operation=?2 OR payment_id=?3) OR EXISTS(SELECT 1 FROM incoming_claims WHERE payment_hash=?1)",params![&invoice.payment_hash[..],&hop.operation[..],&hop.payment_id[..]],|r|r.get(0))?;
        if duplicate {
            return Err(LedgerError::Conflict);
        }
        let maximum = amount
            .checked_add(hop.fee_limit_msat)
            .ok_or(LedgerError::InvalidInput)?;
        let total = outgoing::exposure(&tx, None, None)?;
        outgoing::enforce_exposure(
            total,
            maximum,
            account.lifetime_limit_msat,
            account.outstanding_limit_msat,
        )?;
        outgoing::enforce_exposure(
            outgoing::exposure(&tx, Some(scope.scope), None)?,
            maximum,
            scope.lifetime_limit_msat,
            scope.outstanding_limit_msat,
        )?;
        if total.queued_intents >= account.max_queued_intents
            || total.unresolved_intents >= account.max_unresolved_intents
        {
            return Err(LedgerError::BudgetExceeded);
        }
        let count:i64=tx.query_row("SELECT (SELECT count(*) FROM handshake_intents)+(SELECT count(*) FROM outgoing_intents)",[],|r|r.get(0))?;
        if count >= 4096 {
            return Err(LedgerError::BudgetExceeded);
        }
        storage::check_dispatch(&tx)?;
        tx.execute("INSERT INTO handshake_intents(scope,hop,operation,payment_hash,payload,state,closed,reserved_msat,spent_msat,outstanding_msat,fee_breached,reserve_bytes) VALUES(?1,?2,?3,?4,?5,0,0,?6,0,?6,0,262144)",params![&scope.scope[..],i64::from(invoice.hop),&hop.operation[..],&invoice.payment_hash[..],encode(invoice)?,sql_number(maximum)?])?;
        storage::check_dispatch(&tx)?;
        let record =
            read_record(&tx, scope.scope, invoice.hop)?.ok_or(LedgerError::InvalidLedger)?;
        tx.commit()?;
        Ok(record)
    }
    pub fn get_handshake(&self, scope: Hash32, hop: u16) -> Result<Option<HandshakeRecord>> {
        self.check_file_identity()?;
        read_record(&self.connection, scope, hop)
    }
    pub fn begin_handshake_dispatch(
        &mut self,
        scope: Hash32,
        index: u16,
        clock: ClockObservation,
    ) -> Result<HandshakeDispatchPermit> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let saved = read_scope(&tx, scope, self.account)?.ok_or(LedgerError::NotRegistered)?;
        let record = read_record(&tx, scope, index)?.ok_or(LedgerError::NotRegistered)?;
        if record.state != OutgoingState::Prepared {
            return Err(LedgerError::DispatchAlreadyClaimed);
        }
        require_live_scope(&tx, &saved)?;
        certain_time(clock.now_ms, clock.uncertainty_ms, saved.window)?;
        let (budget, blocked) = outgoing::account_budget(&tx)?;
        if blocked || outgoing::exposure(&tx, None, None)?.wallet_rpcs >= budget.max_wallet_rpcs {
            return Err(LedgerError::BudgetExceeded);
        }
        storage::check_dispatch(&tx)?;
        tx.execute(
            "UPDATE handshake_intents SET state=1 WHERE scope=?1 AND hop=?2",
            params![&scope[..], i64::from(index)],
        )?;
        tx.commit()?;
        Ok(HandshakeDispatchPermit {
            scope: saved,
            invoice: record.invoice,
        })
    }
    pub fn mark_handshake_unknown(&mut self, scope: Hash32, index: u16) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old = read_record(&tx, scope, index)?.ok_or(LedgerError::NotRegistered)?;
        if !matches!(
            old.state,
            OutgoingState::InFlight | OutgoingState::Unknown | OutgoingState::ClosedUnresolved
        ) {
            return Err(LedgerError::Conflict);
        }
        tx.execute("UPDATE handshake_intents SET state=CASE WHEN closed=1 THEN 6 ELSE 2 END WHERE scope=?1 AND hop=?2",params![&scope[..],i64::from(index)])?;
        tx.commit()?;
        Ok(())
    }
    /// Financial accounting retains actual native cost even if its time/fee is
    /// unusable for construction. Construction proof eligibility is separate.
    pub fn record_handshake_outcome(
        &mut self,
        scope: Hash32,
        index: u16,
        evidence: &HandshakeEvidence,
    ) -> Result<HandshakeRecord> {
        self.check_file_identity()?;
        if evidence.financial.account != self.account {
            return Err(LedgerError::WrongAccount);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let saved = read_scope(&tx, scope, self.account)?.ok_or(LedgerError::NotRegistered)?;
        let hop = hop(&saved, index)?;
        let old = read_record(&tx, scope, index)?.ok_or(LedgerError::NotRegistered)?;
        let e = &evidence.financial;
        if e.operation != hop.operation
            || e.invoice_digest != old.invoice.invoice_digest
            || e.payment_hash != old.invoice.payment_hash
            || e.amount_msat != policy(hop)?.handshake_fee_msat
        {
            return Err(LedgerError::InvalidEvidence);
        }
        match (
            e.outcome,
            evidence.settled_earliest_ms,
            evidence.settled_latest_ms,
        ) {
            (NativeOutcome::Settled, Some(a), Some(b)) if a >= 0 && b >= a => {}
            (NativeOutcome::FailedFinal, None, None) => {}
            _ => return Err(LedgerError::InvalidEvidence),
        }
        if let Some(ref prior) = old.evidence {
            return if prior == evidence {
                Ok(old)
            } else {
                Err(LedgerError::Conflict)
            };
        }
        if !matches!(
            old.state,
            OutgoingState::InFlight | OutgoingState::Unknown | OutgoingState::ClosedUnresolved
        ) {
            return Err(LedgerError::Conflict);
        }
        let principal = if e.outcome == NativeOutcome::Settled {
            e.amount_msat
        } else {
            0
        };
        let actual = principal
            .checked_add(e.fee_msat)
            .filter(|n| *n <= i64::MAX as u64);
        let Some(actual) = actual else {
            tx.execute("UPDATE account_budget SET blocked=1 WHERE singleton=1", [])?;
            tx.execute(
                "UPDATE handshake_intents SET exception_evidence=?3 WHERE scope=?1 AND hop=?2",
                params![&scope[..], i64::from(index), encode(evidence)?],
            )?;
            tx.commit()?;
            return Err(LedgerError::InvalidEvidence);
        };
        let breach = e.fee_msat > hop.fee_limit_msat;
        if breach {
            tx.execute("UPDATE account_budget SET blocked=1 WHERE singleton=1", [])?;
        }
        let settled = e.outcome == NativeOutcome::Settled;
        tx.execute("UPDATE handshake_intents SET state=?3,reserved_msat=0,spent_msat=?4,outstanding_msat=?5,fee_breached=?6,outcome=?7,reserve_bytes=0 WHERE scope=?1 AND hop=?2",params![&scope[..],i64::from(index),if settled {3}else{5},sql_number(actual)?,if settled {sql_number(actual)?}else{0},breach,encode(evidence)?])?;
        let record = read_record(&tx, scope, index)?.ok_or(LedgerError::InvalidLedger)?;
        tx.commit()?;
        Ok(record)
    }
    /// All-hop handoff after one claimed construction, preserving the original
    /// scope/caps and native per-hop policies/receivers. No financial rows move,
    /// so handoff never drops or double-counts setup cost.
    pub fn bind_handshake_path(
        &mut self,
        budget: &PathBudget,
        hops: &[HandshakePathHop],
    ) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let scope = read_scope(&tx, budget.local_budget_scope, self.account)?
            .ok_or(LedgerError::NotRegistered)?;
        match_budget(&scope, budget)?;
        if hops.len() != scope.hops.len() {
            return Err(LedgerError::Conflict);
        }
        let mut binding = Vec::new();
        for (i, (expected, actual)) in scope.hops.iter().zip(hops).enumerate() {
            let reg = &actual.registration;
            if reg.binding.instance != scope.instance
                || reg.binding.handle != budget.handle
                || usize::from(reg.binding.hops) != hops.len()
                || usize::from(reg.hop) != i + 1
                || *reg.payment_ids() != expected.initial_payment_ids
                || reg.terms().as_slice() != expected.terms
                || actual.fee_limit_msat != expected.fee_limit_msat
                || actual.payee_reference != payee_reference(expected)
                || actual.charge_reference == [0; 32]
            {
                return Err(LedgerError::Conflict);
            }
            if policy(expected)?.handshake_fee_msat > 0 {
                let record = read_record(&tx, scope.scope, (i + 1) as u16)?
                    .ok_or(LedgerError::NotRegistered)?;
                if !matches!(
                    record.state,
                    OutgoingState::Settled | OutgoingState::Consumed
                ) || record.fee_limit_breached
                {
                    return Err(LedgerError::InvalidEvidence);
                }
            }
            binding.extend_from_slice(&reg.circuit_binding());
            binding.extend_from_slice(&reg.policy_hash());
            binding.extend_from_slice(&actual.charge_reference);
            binding.extend_from_slice(&actual.payee_reference);
            let terms = HopSpendingTerms {
                local_budget_scope: scope.scope,
                charge_reference: actual.charge_reference,
                hop_circuit_binding: reg.circuit_binding(),
                hop_policy_hash: reg.policy_hash(),
                payee_reference: actual.payee_reference,
                handshake_amount_msat: policy(expected)?.handshake_fee_msat,
                round_amount_msat: policy(expected)?.rate_msat,
                fee_limit_msat: actual.fee_limit_msat,
            };
            let prior: Option<Vec<u8>> = tx.query_row(
                "SELECT bound_terms FROM handshake_hops WHERE scope=?1 AND hop=?2",
                params![&scope.scope[..], i as i64 + 1],
                |r| r.get(0),
            )?;
            let payload = encode(&terms)?;
            if prior.as_ref().map_or(false, |old| *old != payload) {
                return Err(LedgerError::Conflict);
            }
            tx.execute(
                "UPDATE handshake_hops SET bound_terms=?3 WHERE scope=?1 AND hop=?2",
                params![&scope.scope[..], i as i64 + 1, payload],
            )?;
        }
        let binding_digest = hash(&binding);
        let prior:Option<(Vec<u8>,Vec<u8>,Vec<u8>)>=tx.query_row("SELECT handle,path_digest,binding_digest FROM handshake_scopes WHERE scope=?1 AND handle IS NOT NULL",params![&scope.scope[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((handle, path, digest)) = prior {
            return if handle == budget.handle
                && path == budget.accepted_terms_digest
                && digest == binding_digest
            {
                Ok(())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        require_open_instance(&tx, scope.instance)?;
        receiving::require_open_binding(&tx, scope.instance, budget.handle)?;
        let staged=tx.query_row("SELECT EXISTS(SELECT 1 FROM origin_constructions c JOIN origin_plans p ON p.scope=c.scope WHERE c.scope=?1 AND c.instance=?2 AND c.claimed=1 AND p.handle=?3)",params![&scope.scope[..],&scope.instance[..],&budget.handle[..]],|r|r.get::<_,bool>(0))?;
        if !staged {
            return Err(LedgerError::NotRegistered);
        }
        tx.execute(
            "UPDATE handshake_scopes SET handle=?2,path_digest=?3,binding_digest=?4 WHERE scope=?1",
            params![
                &scope.scope[..],
                &budget.handle[..],
                &budget.accepted_terms_digest[..],
                &binding_digest[..]
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    /// Caller must establish authenticated native consumption, as for recurring
    /// outgoing usage. Closure alone is never consumption or a refund.
    pub fn mark_handshake_consumed(&mut self, scope: Hash32, index: u16) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old = read_record(&tx, scope, index)?.ok_or(LedgerError::NotRegistered)?;
        if old.state == OutgoingState::Consumed {
            return Ok(());
        }
        if old.state != OutgoingState::Settled || old.closed {
            return Err(LedgerError::Conflict);
        }
        let bound: bool = tx.query_row(
            "SELECT handle IS NOT NULL FROM handshake_scopes WHERE scope=?1",
            params![&scope[..]],
            |r| r.get(0),
        )?;
        if !bound {
            return Err(LedgerError::NotRegistered);
        }
        tx.execute(
            "UPDATE handshake_intents SET state=4,outstanding_msat=0 WHERE scope=?1 AND hop=?2",
            params![&scope[..], i64::from(index)],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn close_handshake_scope(&mut self, scope: Hash32) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if read_scope(&tx, scope, self.account)?.is_none() {
            return Err(LedgerError::NotRegistered);
        }
        close(&tx, "scope=?1", &scope[..])?;
        tx.commit()?;
        Ok(())
    }
    pub fn unresolved_handshakes(
        &self,
        after: Option<Id16>,
        limit: u64,
    ) -> Result<Vec<(HandshakeScope, HandshakeRecord)>> {
        self.check_file_identity()?;
        if limit == 0 || limit > 128 {
            return Err(LedgerError::InvalidInput);
        }
        let mut statement=self.connection.prepare("SELECT scope,hop FROM handshake_intents WHERE state IN(0,1,2,3,6) AND (?1 IS NULL OR operation>?1) ORDER BY operation LIMIT ?2")?;
        let keys = statement
            .query_map(
                params![after.as_ref().map(|id| &id[..]), sql_number(limit)?],
                |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, u16>(1)?)),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        keys.into_iter()
            .map(|(scope, index)| {
                let scope: Hash32 = scope.try_into().map_err(|_| LedgerError::InvalidLedger)?;
                Ok((
                    read_scope(&self.connection, scope, self.account)?
                        .ok_or(LedgerError::InvalidLedger)?,
                    read_record(&self.connection, scope, index)?
                        .ok_or(LedgerError::InvalidLedger)?,
                ))
            })
            .collect()
    }
}
fn hash(bytes: &[u8]) -> Hash32 {
    Sha256::digest(bytes).into()
}
fn hop(scope: &HandshakeScope, index: u16) -> Result<&HandshakeHop> {
    scope
        .hops
        .get(usize::from(index).wrapping_sub(1))
        .ok_or(LedgerError::InvalidInput)
}
fn policy(hop: &HandshakeHop) -> Result<FoundationPolicy> {
    let bytes: [u8; 55] = hop
        .terms
        .as_slice()
        .try_into()
        .map_err(|_| LedgerError::InvalidInput)?;
    FoundationPolicy::decode(&bytes).map_err(|_| LedgerError::InvalidInput)
}
/// Same pinned receiving-authority commitment used by the origin plan.
pub fn payee_reference(hop: &HandshakeHop) -> Hash32 {
    let domain = b"eltor/origin/v1/pinned-receiver";
    let mut h = Sha256::new();
    h.update((domain.len() as u16).to_be_bytes());
    h.update(domain);
    h.update(hop.fingerprint);
    h.update(hop.offer.as_deref().unwrap_or("").as_bytes());
    h.finalize().into()
}
fn validate_scope(scope: &HandshakeScope, account: AccountId) -> Result<()> {
    if scope.account != account {
        return Err(LedgerError::WrongAccount);
    }
    if scope.scope == [0; 32]
        || scope.instance == [0; 16]
        || scope.hops.is_empty()
        || scope.hops.len() > 16
        || scope.window.opens_at_ms < 0
        || scope.window.closes_at_ms <= scope.window.opens_at_ms
        || scope.window.closes_at_ms - scope.window.opens_at_ms > 300000
        || scope.outstanding_limit_msat > scope.lifetime_limit_msat
        || scope.per_round_limit_msat > scope.lifetime_limit_msat
    {
        return Err(LedgerError::InvalidInput);
    }
    sql_number(scope.lifetime_limit_msat)?;
    sql_number(scope.outstanding_limit_msat)?;
    sql_number(scope.per_round_limit_msat)?;
    let mut fingerprints = std::collections::HashSet::new();
    let mut operations = std::collections::HashSet::new();
    let mut ids = std::collections::HashSet::new();
    for h in &scope.hops {
        if h.fingerprint == [0; 20]
            || h.operation == [0; 16]
            || h.payment_id == [0; 32]
            || !fingerprints.insert(h.fingerprint)
            || !operations.insert(h.operation)
            || !ids.insert(h.payment_id)
        {
            return Err(LedgerError::InvalidInput);
        }
        for id in &h.initial_payment_ids {
            if *id == [0; 32] || !ids.insert(*id) {
                return Err(LedgerError::InvalidInput);
            }
        }
        let p = policy(h)?;
        sql_number(
            p.handshake_fee_msat
                .checked_add(h.fee_limit_msat)
                .ok_or(LedgerError::InvalidInput)?,
        )?;
        if let Some(offer) = &h.offer {
            if !offer.starts_with("lno1")
                || offer.len() > 2048
                || !offer
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            {
                return Err(LedgerError::InvalidInput);
            }
        }
        if p.handshake_fee_msat > 0 && h.offer.is_none() {
            return Err(LedgerError::UnsupportedOperation);
        }
    }
    Ok(())
}
fn validate_invoice(invoice: &HandshakeInvoice) -> Result<()> {
    if invoice.scope == [0; 32]
        || invoice.hop == 0
        || invoice.invoice_digest == [0; 32]
        || invoice.payment_hash == [0; 32]
        || invoice.artifact.len() > 2048
        || !invoice.artifact.starts_with("lni1")
        || !invoice
            .artifact
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        || hash(invoice.artifact.as_bytes()) != invoice.invoice_digest
    {
        return Err(LedgerError::InvalidInput);
    }
    Ok(())
}
fn read_scope(c: &Connection, scope: Hash32, account: AccountId) -> Result<Option<HandshakeScope>> {
    let row: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = c
        .query_row(
            "SELECT instance,payload,digest FROM handshake_scopes WHERE scope=?1",
            params![&scope[..]],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    row.map(|(instance, payload, digest)| {
        let saved: HandshakeScope =
            serde_json::from_slice(&payload).map_err(|_| LedgerError::InvalidLedger)?;
        validate_scope(&saved, account).map_err(|_| LedgerError::InvalidLedger)?;
        if saved.scope != scope
            || saved.instance.as_slice() != instance
            || hash(&payload).as_slice() != digest
        {
            return Err(LedgerError::InvalidLedger);
        }
        Ok(saved)
    })
    .transpose()
}
fn read_record(c: &Connection, scope: Hash32, index: u16) -> Result<Option<HandshakeRecord>> {
    let row:Option<(Vec<u8>,i64,bool,i64,i64,i64,bool,Option<Vec<u8>>)>=c.query_row("SELECT payload,state,closed,reserved_msat,spent_msat,outstanding_msat,fee_breached,outcome FROM handshake_intents WHERE scope=?1 AND hop=?2",params![&scope[..],i64::from(index)],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional()?;
    row.map(
        |(payload, state, closed, reserved, spent, outstanding, breach, evidence)| {
            let invoice: HandshakeInvoice =
                serde_json::from_slice(&payload).map_err(|_| LedgerError::InvalidLedger)?;
            validate_invoice(&invoice).map_err(|_| LedgerError::InvalidLedger)?;
            if invoice.scope != scope || invoice.hop != index {
                return Err(LedgerError::InvalidLedger);
            }
            Ok(HandshakeRecord {
                invoice,
                state: match state {
                    0 => OutgoingState::Prepared,
                    1 => OutgoingState::InFlight,
                    2 => OutgoingState::Unknown,
                    3 => OutgoingState::Settled,
                    4 => OutgoingState::Consumed,
                    5 => OutgoingState::FailedFinal,
                    6 => OutgoingState::ClosedUnresolved,
                    7 => OutgoingState::CancelledBeforeDispatch,
                    _ => return Err(LedgerError::InvalidLedger),
                },
                closed,
                reserved_msat: u64::try_from(reserved).map_err(|_| LedgerError::InvalidLedger)?,
                spent_msat: u64::try_from(spent).map_err(|_| LedgerError::InvalidLedger)?,
                outstanding_msat: u64::try_from(outstanding)
                    .map_err(|_| LedgerError::InvalidLedger)?,
                fee_limit_breached: breach,
                evidence: evidence
                    .map(|v| serde_json::from_slice(&v).map_err(|_| LedgerError::InvalidLedger))
                    .transpose()?,
            })
        },
    )
    .transpose()
}
fn require_live_scope(c: &Connection, scope: &HandshakeScope) -> Result<()> {
    require_open_instance(c, scope.instance)?;
    let (closed, bound): (bool, bool) = c.query_row(
        "SELECT closed,handle IS NOT NULL FROM handshake_scopes WHERE scope=?1",
        params![&scope.scope[..]],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if closed || bound {
        return Err(LedgerError::Closed);
    }
    Ok(())
}
fn match_budget(scope: &HandshakeScope, b: &PathBudget) -> Result<()> {
    if b.instance != scope.instance
        || b.local_budget_scope != scope.scope
        || b.handle == [0; 16]
        || b.accepted_terms_digest == [0; 32]
        || b.lifetime_limit_msat != scope.lifetime_limit_msat
        || b.outstanding_limit_msat != scope.outstanding_limit_msat
        || b.per_round_limit_msat != scope.per_round_limit_msat
        || b.funding != FundingSource::OwnWallet
    {
        return Err(LedgerError::Conflict);
    }
    Ok(())
}
pub(super) fn require_path(c: &Connection, b: &PathBudget, account: AccountId) -> Result<()> {
    if let Some(scope) = read_scope(c, b.local_budget_scope, account)? {
        match_budget(&scope, b)?;
        let row: (bool, Option<Vec<u8>>, Option<Vec<u8>>) = c.query_row(
            "SELECT closed,handle,path_digest FROM handshake_scopes WHERE scope=?1",
            params![&scope.scope[..]],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if row.0 {
            return Err(LedgerError::Closed);
        }
        if row.1.as_deref() != Some(&b.handle[..])
            || row.2.as_deref() != Some(&b.accepted_terms_digest[..])
        {
            return Err(LedgerError::Conflict);
        }
    }
    Ok(())
}

pub(super) fn require_hop_terms(c: &Connection, terms: &HopSpendingTerms) -> Result<()> {
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM handshake_scopes WHERE scope=?1)",
        params![&terms.local_budget_scope[..]],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let mut query = c.prepare("SELECT bound_terms FROM handshake_hops WHERE scope=?1")?;
    let rows = query
        .query_map(params![&terms.local_budget_scope[..]], |r| {
            r.get::<_, Option<Vec<u8>>>(0)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let expected = encode(terms)?;
    if !rows
        .iter()
        .any(|r| r.as_deref() == Some(expected.as_slice()))
    {
        return Err(LedgerError::Conflict);
    }
    Ok(())
}
pub(super) fn forbid_duplicate(c: &Connection, intent: &OutgoingIntent) -> Result<()> {
    let duplicate:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM handshake_hops WHERE operation=?1 OR payment_id=?2) OR EXISTS(SELECT 1 FROM handshake_intents WHERE payment_hash=?3) OR EXISTS(SELECT 1 FROM handshake_scopes WHERE scope=?4 AND ?5=0)",params![&intent.operation[..],&intent.payment_id[..],&intent.payment_hash[..],&intent.local_budget_scope[..],intent.kind.code()],|r|r.get(0))?;
    if duplicate {
        return Err(LedgerError::Conflict);
    }
    Ok(())
}
pub(super) fn close(tx: &rusqlite::Transaction<'_>, predicate: &str, key: &[u8]) -> Result<()> {
    tx.execute(&format!("UPDATE handshake_intents SET closed=1,state=CASE WHEN state=0 THEN 7 WHEN state IN(1,2) THEN 6 ELSE state END,reserved_msat=CASE WHEN state=0 THEN 0 ELSE reserved_msat END,outstanding_msat=CASE WHEN state=0 THEN 0 ELSE outstanding_msat END,reserve_bytes=CASE WHEN state=0 THEN 0 ELSE reserve_bytes END WHERE scope IN(SELECT scope FROM handshake_scopes WHERE {predicate})"),params![key])?;
    tx.execute(
        &format!("UPDATE handshake_scopes SET closed=1 WHERE {predicate}"),
        params![key],
    )?;
    Ok(())
}
pub(super) fn validate_schema(c: &Connection, account: AccountId) -> Result<()> {
    c.prepare("SELECT s.scope,s.handle,s.path_digest,s.binding_digest,h.operation,h.fetch_claimed,h.bound_terms,i.outcome,i.exception_evidence FROM handshake_scopes s,handshake_hops h,handshake_intents i LIMIT 0")?;
    let mut rows =
        c.prepare("SELECT scope,instance,handle,path_digest,binding_digest FROM handshake_scopes")?;
    let saved = rows
        .query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Option<Vec<u8>>>(2)?,
                r.get::<_, Option<Vec<u8>>>(3)?,
                r.get::<_, Option<Vec<u8>>>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if saved.len() > 4096 {
        return Err(LedgerError::InvalidLedger);
    }
    for (key, instance, handle, path, binding) in saved {
        let key: Hash32 = key.try_into().map_err(|_| LedgerError::InvalidLedger)?;
        let scope = read_scope(c, key, account)?.ok_or(LedgerError::InvalidLedger)?;
        if instance != scope.instance
            || handle.is_some() != path.is_some()
            || handle.is_some() != binding.is_some()
        {
            return Err(LedgerError::InvalidLedger);
        }
        let count: i64 = c.query_row(
            "SELECT count(*) FROM handshake_hops WHERE scope=?1",
            params![&key[..]],
            |r| r.get(0),
        )?;
        if count != scope.hops.len() as i64 {
            return Err(LedgerError::InvalidLedger);
        }
        let id_count: i64 = c.query_row(
            "SELECT count(*) FROM handshake_initial_ids WHERE scope=?1",
            params![&key[..]],
            |r| r.get(0),
        )?;
        if id_count != scope.hops.len() as i64 * 10 {
            return Err(LedgerError::InvalidLedger);
        }
        let mut mapped_binding = Vec::new();
        for (i, h) in scope.hops.iter().enumerate() {
            for (round, id) in h.initial_payment_ids.iter().enumerate() {
                let saved:Vec<u8>=c.query_row("SELECT payment_id FROM handshake_initial_ids WHERE scope=?1 AND hop=?2 AND round=?3",params![&key[..],i as i64+1,round as i64+1],|r|r.get(0))?;
                if saved.as_slice() != id {
                    return Err(LedgerError::InvalidLedger);
                }
            }
            let bound: Option<Vec<u8>> = c.query_row(
                "SELECT bound_terms FROM handshake_hops WHERE scope=?1 AND hop=?2",
                params![&key[..], i as i64 + 1],
                |r| r.get(0),
            )?;
            if bound.is_some() != handle.is_some() {
                return Err(LedgerError::InvalidLedger);
            }
            if let Some(payload) = bound {
                let t: HopSpendingTerms =
                    serde_json::from_slice(&payload).map_err(|_| LedgerError::InvalidLedger)?;
                if t.local_budget_scope != scope.scope
                    || t.charge_reference == [0; 32]
                    || t.hop_circuit_binding == [0; 32]
                    || t.hop_policy_hash == [0; 32]
                    || t.payee_reference != payee_reference(h)
                    || t.fee_limit_msat != h.fee_limit_msat
                    || t.round_amount_msat != policy(h)?.rate_msat
                    || t.handshake_amount_msat != policy(h)?.handshake_fee_msat
                {
                    return Err(LedgerError::InvalidLedger);
                }
                mapped_binding.extend_from_slice(&t.hop_circuit_binding);
                mapped_binding.extend_from_slice(&t.hop_policy_hash);
                mapped_binding.extend_from_slice(&t.charge_reference);
                mapped_binding.extend_from_slice(&t.payee_reference);
                let configured:Option<Vec<u8>>=c.query_row("SELECT payload FROM hop_spending_terms WHERE local_budget_scope=?1 AND charge_reference=?2",params![&key[..],&t.charge_reference[..]],|r|r.get(0)).optional()?;
                if configured.as_ref().map_or(false, |old| *old != payload) {
                    return Err(LedgerError::InvalidLedger);
                }
            }
            let mapped: (Vec<u8>, Vec<u8>) = c.query_row(
                "SELECT operation,payment_id FROM handshake_hops WHERE scope=?1 AND hop=?2",
                params![&key[..], i as i64 + 1],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if mapped.0 != h.operation || mapped.1 != h.payment_id {
                return Err(LedgerError::InvalidLedger);
            }
            if let Some(record) = read_record(c, key, (i + 1) as u16)? {
                validate_finance(h, &record, account)?;
                let identity:(Vec<u8>,Vec<u8>)=c.query_row("SELECT operation,payment_hash FROM handshake_intents WHERE scope=?1 AND hop=?2",params![&key[..],i as i64+1],|r|Ok((r.get(0)?,r.get(1)?)))?;
                if identity.0 != h.operation || identity.1 != record.invoice.payment_hash {
                    return Err(LedgerError::InvalidLedger);
                }
            }
        }
        if let Some(binding) = binding {
            if binding != hash(&mapped_binding) {
                return Err(LedgerError::InvalidLedger);
            }
        }
    }
    let count: i64 = c.query_row(
        "SELECT (SELECT count(*) FROM handshake_intents)+(SELECT count(*) FROM outgoing_intents)",
        [],
        |r| r.get(0),
    )?;
    if count > 4096 {
        return Err(LedgerError::InvalidLedger);
    }
    Ok(())
}
fn validate_finance(hop: &HandshakeHop, r: &HandshakeRecord, account: AccountId) -> Result<()> {
    let amount = policy(hop)?.handshake_fee_msat;
    let maximum = amount
        .checked_add(hop.fee_limit_msat)
        .ok_or(LedgerError::InvalidLedger)?;
    if amount == 0 {
        return Err(LedgerError::InvalidLedger);
    }
    let expected = match r.state {
        OutgoingState::Prepared
        | OutgoingState::InFlight
        | OutgoingState::Unknown
        | OutgoingState::ClosedUnresolved => {
            if r.evidence.is_some() {
                return Err(LedgerError::InvalidLedger);
            }
            (maximum, 0, maximum)
        }
        OutgoingState::CancelledBeforeDispatch => {
            if r.evidence.is_some() {
                return Err(LedgerError::InvalidLedger);
            }
            (0, 0, 0)
        }
        OutgoingState::Settled | OutgoingState::Consumed | OutgoingState::FailedFinal => {
            let e = r.evidence.as_ref().ok_or(LedgerError::InvalidLedger)?;
            let f = &e.financial;
            if f.account != account
                || f.operation != hop.operation
                || f.payment_hash != r.invoice.payment_hash
                || f.invoice_digest != r.invoice.invoice_digest
                || f.amount_msat != amount
                || r.fee_limit_breached != (f.fee_msat > hop.fee_limit_msat)
            {
                return Err(LedgerError::InvalidLedger);
            }
            let settled = matches!(r.state, OutgoingState::Settled | OutgoingState::Consumed);
            if settled != (f.outcome == NativeOutcome::Settled)
                || (settled
                    && (e.settled_earliest_ms.map_or(true, |a| a < 0)
                        || e.settled_latest_ms < e.settled_earliest_ms))
                || (!settled && (e.settled_earliest_ms.is_some() || e.settled_latest_ms.is_some()))
            {
                return Err(LedgerError::InvalidLedger);
            }
            let cost = (if settled { amount } else { 0 })
                .checked_add(f.fee_msat)
                .ok_or(LedgerError::InvalidLedger)?;
            (
                0,
                cost,
                if r.state == OutgoingState::Settled {
                    cost
                } else {
                    0
                },
            )
        }
    };
    if (r.reserved_msat, r.spent_msat, r.outstanding_msat) != expected {
        return Err(LedgerError::InvalidLedger);
    }
    Ok(())
}
pub(super) const SCHEMA:&str="
CREATE TABLE IF NOT EXISTS handshake_scopes(scope BLOB PRIMARY KEY CHECK(length(scope)=32),instance BLOB NOT NULL CHECK(length(instance)=16),payload BLOB NOT NULL,digest BLOB NOT NULL CHECK(length(digest)=32),closed INTEGER NOT NULL CHECK(closed IN(0,1)),handle BLOB CHECK(handle IS NULL OR length(handle)=16),path_digest BLOB CHECK(path_digest IS NULL OR length(path_digest)=32),binding_digest BLOB CHECK(binding_digest IS NULL OR length(binding_digest)=32),reserve_bytes INTEGER NOT NULL CHECK(reserve_bytes>=0));
CREATE TABLE IF NOT EXISTS handshake_hops(scope BLOB NOT NULL REFERENCES handshake_scopes(scope),hop INTEGER NOT NULL CHECK(hop BETWEEN 1 AND 16),operation BLOB NOT NULL UNIQUE CHECK(length(operation)=16),payment_id BLOB NOT NULL UNIQUE CHECK(length(payment_id)=32),bound_terms BLOB,fetch_claimed INTEGER NOT NULL DEFAULT 0 CHECK(fetch_claimed IN(0,1)),PRIMARY KEY(scope,hop));
CREATE TABLE IF NOT EXISTS handshake_initial_ids(scope BLOB NOT NULL,hop INTEGER NOT NULL,round INTEGER NOT NULL CHECK(round BETWEEN 1 AND 10),payment_id BLOB NOT NULL UNIQUE CHECK(length(payment_id)=32),PRIMARY KEY(scope,hop,round),FOREIGN KEY(scope,hop) REFERENCES handshake_hops(scope,hop));
CREATE TABLE IF NOT EXISTS handshake_intents(scope BLOB NOT NULL,hop INTEGER NOT NULL,operation BLOB NOT NULL UNIQUE CHECK(length(operation)=16),payment_hash BLOB NOT NULL UNIQUE CHECK(length(payment_hash)=32),payload BLOB NOT NULL,state INTEGER NOT NULL CHECK(state BETWEEN 0 AND 7),closed INTEGER NOT NULL CHECK(closed IN(0,1)),reserved_msat INTEGER NOT NULL CHECK(reserved_msat>=0),spent_msat INTEGER NOT NULL CHECK(spent_msat>=0),outstanding_msat INTEGER NOT NULL CHECK(outstanding_msat>=0),fee_breached INTEGER NOT NULL CHECK(fee_breached IN(0,1)),outcome BLOB,exception_evidence BLOB,reserve_bytes INTEGER NOT NULL CHECK(reserve_bytes>=0),PRIMARY KEY(scope,hop),FOREIGN KEY(scope,hop) REFERENCES handshake_hops(scope,hop));
";
#[cfg(test)]
#[path = "handshake_tests.rs"]
mod tests;
