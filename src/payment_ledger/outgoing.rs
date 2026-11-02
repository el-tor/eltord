//! Outgoing intent and exposure accounting. These primitives do not contact a
//! wallet or authorize a real purchase. Operators must supply explicit accepted
//! budgets, authenticate invoice/payee/quote bindings, and use an adapter with
//! native fee limits and exact outgoing operation/hash lookup before integration.
//! A dispatch permit is issued once; losing it is uncertainty, not permission to
//! issue another payment. A zero-priced obligation uses the receiving ledger's
//! explicit zero-charge path rather than a wallet payment.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountBudget {
    pub lifetime_limit_msat: u64,
    pub outstanding_limit_msat: u64,
    pub max_queued_intents: u64,
    pub max_wallet_rpcs: u64,
    pub max_unresolved_intents: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FundingSource {
    OwnWallet,
    RecipientSettlement,
}

/// Immutable accepted limits. Changing limits or resetting a circuit's lifetime
/// history requires a future explicit operator/reconciliation workflow; this API
/// deliberately permits only exact replays. Managed origin plans can fund later
/// periods only after typed all-hop admission; legacy financial scopes remain
/// bounded to rounds one through ten.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathBudget {
    pub instance: Id16,
    pub handle: Id16,
    /// Local accounting scope only; never transmitted or substituted for hop C.
    pub local_budget_scope: Hash32,
    pub accepted_terms_digest: Hash32,
    pub lifetime_limit_msat: u64,
    pub outstanding_limit_msat: u64,
    pub per_round_limit_msat: u64,
    pub funding: FundingSource,
}

/// Immutable accepted terms for one independent hop. C and Q retain their wire
/// meanings; neither is a shared path ID or an aggregate price commitment.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HopSpendingTerms {
    pub local_budget_scope: Hash32,
    pub charge_reference: Hash32,
    pub hop_circuit_binding: Hash32,
    pub hop_policy_hash: Hash32,
    pub payee_reference: Hash32,
    pub handshake_amount_msat: u64,
    pub round_amount_msat: u64,
    pub fee_limit_msat: u64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingIntent {
    pub operation: Id16,
    pub account: AccountId,
    pub instance: Id16,
    pub handle: Id16,
    pub local_budget_scope: Hash32,
    pub round: u64,
    pub kind: ChargeKind,
    pub payment_id: Hash32,
    /// Immutable accepted hop/charge reference. Distinct hops may legitimately
    /// pay the same receiving authority; a fresh invoice is not a fresh charge.
    pub charge_reference: Hash32,
    /// Authenticated canonical receiving-authority reference, not a caller label.
    pub payee_reference: Hash32,
    pub accepted_terms_digest: Hash32,
    pub hop_circuit_binding: Hash32,
    pub hop_policy_hash: Hash32,
    pub invoice_digest: Hash32,
    pub payment_hash: Hash32,
    pub amount_msat: u64,
    pub fee_limit_msat: u64,
    pub window: AcceptanceWindow,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OutgoingState {
    Prepared,
    InFlight,
    Unknown,
    Settled,
    Consumed,
    FailedFinal,
    ClosedUnresolved,
    CancelledBeforeDispatch,
}

#[derive(Clone, PartialEq, Eq)]
pub struct OutgoingRecord {
    pub intent: OutgoingIntent,
    pub state: OutgoingState,
    pub closed: bool,
    pub reserved_msat: u64,
    pub spent_msat: u64,
    pub outstanding_msat: u64,
    /// A native fee-limit violation is accounted at actual cost and freezes new
    /// work for the account. It must never be hidden by clipping recorded fees.
    pub fee_limit_breached: bool,
}

/// No Clone implementation: callers must consume one durable dispatch claim in
/// their wallet adapter. An existing/recovered intent never yields another one.
pub struct DispatchPermit {
    intent: OutgoingIntent,
}
impl DispatchPermit {
    pub fn into_intent(self) -> OutgoingIntent {
        self.intent
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeOutcome {
    Settled,
    FailedFinal,
}

/// Trusted adapter evidence from exact native outgoing lookup. A timeout,
/// exception, missing lookup result or cancellation is never FailedFinal.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingEvidence {
    pub account: AccountId,
    pub operation: Id16,
    pub payment_hash: Hash32,
    pub invoice_digest: Hash32,
    pub amount_msat: u64,
    pub fee_msat: u64,
    pub outcome: NativeOutcome,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Exposure {
    pub reserved_msat: u64,
    pub spent_msat: u64,
    pub outstanding_msat: u64,
    pub queued_intents: u64,
    pub wallet_rpcs: u64,
    pub unresolved_intents: u64,
}

impl PaymentLedger {
    /// Explicit transactional v1->current upgrade; ordinary reopen never changes a
    /// schema. Receiving claims/authorizations remain intact. No lost-history
    /// reset or inferred wallet outcomes are part of migration.
    pub fn upgrade_from_v1(path: &Path, expected_account: AccountId) -> Result<Self> {
        let mut ledger = Self::connect(path, expected_account)?;
        let version: i64 = ledger
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version == 4 {
            drop(ledger);
            return Self::upgrade_from_v4(path, expected_account);
        }
        if version == 5 {
            drop(ledger);
            return Self::upgrade_from_v5(path, expected_account);
        }
        if version == 6 {
            drop(ledger);
            return Self::upgrade_from_v6(path, expected_account);
        }
        if version == VERSION {
            drop(ledger);
            return Self::reopen(path, expected_account);
        }
        if version == 3 {
            drop(ledger);
            return Self::upgrade_from_v3(path, expected_account);
        }
        if version == 2 {
            drop(ledger);
            return Self::upgrade_from_v2(path, expected_account);
        }
        let app: i64 = ledger
            .connection
            .pragma_query_value(None, "application_id", |r| r.get(0))?;
        if version != 1 || app != APPLICATION_ID {
            return Err(LedgerError::InvalidLedger);
        }
        let account: Vec<u8> = ledger.connection.query_row(
            "SELECT account FROM ledger_info WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        if account != expected_account.0 {
            return Err(LedgerError::WrongAccount);
        }
        let check: String = ledger
            .connection
            .query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(LedgerError::InvalidLedger);
        }
        let broken: bool = ledger.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
            [],
            |r| r.get(0),
        )?;
        if broken {
            return Err(LedgerError::InvalidLedger);
        }
        ledger.connection.prepare("SELECT r.id,b.decision,p.payment_id,c.payment_hash,a.record_digest,w.opens_at_ms,t.payload,x.instance FROM registrations r,batches b,payment_ids p,incoming_claims c,authorizations a,initial_windows w,activation_anchors t,closed_instances x LIMIT 0")?;
        ledger
            .connection
            .pragma_update(None, "foreign_keys", "OFF")?;
        let tx = ledger
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        storage::initialize(&tx, storage::DEFAULT_BUDGET_BYTES)?;
        storage::reserve_legacy(&tx)?;
        tx.execute_batch(clock::SCHEMA)?;
        tx.execute_batch(receiving::SCHEMA)?;
        tx.execute_batch(origin::SCHEMA)?;
        tx.execute_batch(origin_artifacts::SCHEMA)?;
        tx.execute_batch(handshake::SCHEMA)?;
        outgoing_renewal::initialize(&tx)?;
        origin_artifacts::validate_schema(&tx, expected_account)?;
        outgoing_renewal::validate_schema(&tx)?;
        handshake::validate_schema(&tx, expected_account)?;
        tx.pragma_update(None, "user_version", VERSION)?;
        tx.commit()?;
        drop(ledger);
        Self::reopen(path, expected_account)
    }

    pub fn configure_account_budget(&mut self, budget: AccountBudget) -> Result<()> {
        self.check_file_identity()?;
        validate_limits(budget.lifetime_limit_msat, budget.outstanding_limit_msat)?;
        if budget.max_queued_intents == 0
            || budget.max_queued_intents > 128
            || budget.max_wallet_rpcs == 0
            || budget.max_wallet_rpcs > 32
            || budget.max_unresolved_intents == 0
        {
            return Err(LedgerError::InvalidInput);
        }
        sql_number(budget.max_unresolved_intents)?;
        let payload = encode(&budget)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Option<Vec<u8>> = tx
            .query_row(
                "SELECT payload FROM account_budget WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = old {
            return if old == payload {
                Ok(())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        tx.execute(
            "INSERT INTO account_budget(singleton,payload,blocked) VALUES(1,?1,0)",
            params![payload],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Explicit operator action only. Runtime configuration remains immutable.
    /// Raises only the account lifetime ceiling, retaining spent/reserved funds,
    /// all accepted path budgets and every other account constraint.
    pub fn extend_account_lifetime_limit(&mut self, expected: u64, new_limit: u64) -> Result<()> {
        self.check_file_identity()?;
        if expected == 0 || new_limit <= expected {
            return Err(LedgerError::InvalidInput);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (mut old, blocked) = account_budget(&tx)?;
        if blocked {
            return Err(LedgerError::BudgetExceeded);
        }
        if old.lifetime_limit_msat == new_limit {
            return Ok(()); // Exact retry; no further increase.
        }
        if old.lifetime_limit_msat != expected {
            return Err(LedgerError::Conflict);
        }
        validate_limits(new_limit, old.outstanding_limit_msat)?;
        old.lifetime_limit_msat = new_limit;
        tx.execute(
            "UPDATE account_budget SET payload=?1 WHERE singleton=1 AND blocked=0",
            params![encode(&old)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Explicit operator action only. Runtime configuration remains immutable.
    /// Raises only the account concurrent-exposure ceiling, retaining spent/reserved funds,
    /// all accepted path budgets and every other account constraint.
    pub fn extend_account_outstanding_limit(&mut self, expected: u64, new_limit: u64) -> Result<()> {
        self.check_file_identity()?;
        if expected == 0 || new_limit <= expected {
            return Err(LedgerError::InvalidInput);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (mut old, blocked) = account_budget(&tx)?;
        if blocked {
            return Err(LedgerError::BudgetExceeded);
        }
        if old.outstanding_limit_msat == new_limit {
            return Ok(()); // Exact retry; no further increase.
        }
        if old.outstanding_limit_msat != expected {
            return Err(LedgerError::Conflict);
        }
        validate_limits(old.lifetime_limit_msat, new_limit)?;
        old.outstanding_limit_msat = new_limit;
        tx.execute(
            "UPDATE account_budget SET payload=?1 WHERE singleton=1 AND blocked=0",
            params![encode(&old)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn configure_path_budget(&mut self, budget: &PathBudget) -> Result<()> {
        self.check_file_identity()?;
        if budget.funding != FundingSource::OwnWallet {
            return Err(LedgerError::UnsupportedOperation);
        }
        validate_limits(budget.lifetime_limit_msat, budget.outstanding_limit_msat)?;
        sql_number(budget.per_round_limit_msat)?;
        if budget.instance == [0; 16]
            || budget.handle == [0; 16]
            || budget.local_budget_scope == [0; 32]
            || budget.accepted_terms_digest == [0; 32]
            || budget.per_round_limit_msat > budget.lifetime_limit_msat
        {
            return Err(LedgerError::InvalidInput);
        }
        let payload = encode(budget)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Option<Vec<u8>> = tx
            .query_row(
                "SELECT payload FROM path_budgets WHERE local_budget_scope=?1",
                params![&budget.local_budget_scope[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = old {
            return if old == payload {
                Ok(())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        handshake::require_path(&tx, budget, self.account)?;
        require_open_instance(&tx, budget.instance)?;
        account_budget(&tx)?;
        storage::check_admission(&tx)?;
        tx.execute("INSERT INTO path_budgets(local_budget_scope,instance,handle,payload,current_round,closed) VALUES(?1,?2,?3,?4,1,0)",params![&budget.local_budget_scope[..],&budget.instance[..],&budget.handle[..],payload])?;
        storage::check_admission(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Register independently authenticated hop terms before any wallet intent.
    /// Retries recover the exact terms; changing C/Q/payee/prices is a conflict.
    pub fn configure_hop_spending_terms(&mut self, terms: &HopSpendingTerms) -> Result<()> {
        self.check_file_identity()?;
        if [
            terms.local_budget_scope,
            terms.charge_reference,
            terms.hop_circuit_binding,
            terms.hop_policy_hash,
            terms.payee_reference,
        ]
        .contains(&[0; 32])
        {
            return Err(LedgerError::InvalidInput);
        }
        sql_number(
            terms
                .handshake_amount_msat
                .checked_add(terms.fee_limit_msat)
                .ok_or(LedgerError::InvalidInput)?,
        )?;
        sql_number(
            terms
                .round_amount_msat
                .checked_add(terms.fee_limit_msat)
                .ok_or(LedgerError::InvalidInput)?,
        )?;
        let payload = encode(terms)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old:Option<Vec<u8>>=tx.query_row("SELECT payload FROM hop_spending_terms WHERE local_budget_scope=?1 AND charge_reference=?2",params![&terms.local_budget_scope[..],&terms.charge_reference[..]],|r|r.get(0)).optional()?;
        if let Some(old) = old {
            return if old == payload {
                Ok(())
            } else {
                Err(LedgerError::Conflict)
            };
        }
        handshake::require_hop_terms(&tx, terms)?;
        outgoing_renewal::require_new_hop_slot(&tx, terms.local_budget_scope)?;
        let (_, _, closed) = path_budget(&tx, terms.local_budget_scope)?;
        if closed {
            return Err(LedgerError::Closed);
        }
        storage::check_admission(&tx)?;
        tx.execute("INSERT INTO hop_spending_terms(local_budget_scope,charge_reference,hop_circuit_binding,payload) VALUES(?1,?2,?3,?4)",params![&terms.local_budget_scope[..],&terms.charge_reference[..],&terms.hop_circuit_binding[..],payload])?;
        storage::check_admission(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Persist immutable invoice/quote/limits and reserve principal+maximum fees.
    /// The returned record is not a dispatch permit. Exact replays recover the
    /// original state even after expiry; changed identities/terms conflict.
    pub fn prepare_outgoing(
        &mut self,
        intent: &OutgoingIntent,
        clock: ClockObservation,
    ) -> Result<OutgoingRecord> {
        self.check_file_identity()?;
        validate_intent(intent, self.account)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(old) = read_outgoing(&tx, intent.operation)? {
            return if old.intent == *intent {
                Ok(old)
            } else {
                Err(LedgerError::Conflict)
            };
        }
        storage::check_dispatch(&tx)?;
        certain_time(clock.now_ms, clock.uncertainty_ms, intent.window)?;
        require_open_instance(&tx, intent.instance)?;
        let (account, blocked) = account_budget(&tx)?;
        let (circuit, current, closed) = path_budget(&tx, intent.local_budget_scope)?;
        if closed {
            return Err(LedgerError::Closed);
        }
        if blocked {
            return Err(LedgerError::BudgetExceeded);
        }
        handshake::forbid_duplicate(&tx, intent)?;
        match_path(intent, &circuit)?;
        match_hop_terms(&tx, intent)?;
        origin_artifacts::require_managed_invoice(&tx, intent, self.account)?;
        outgoing_renewal::check_intent(&tx, intent, current)?;
        let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM outgoing_intents WHERE payment_hash=?1 OR payment_id=?2 OR (local_budget_scope=?3 AND round=?4 AND kind=?5 AND charge_reference=?6))",params![&intent.payment_hash[..],&intent.payment_id[..],&intent.local_budget_scope[..],sql_number(intent.round)?,intent.kind.code(),&intent.charge_reference[..]],|r|r.get(0))?;
        if duplicate {
            return Err(LedgerError::Conflict);
        }
        let maximum = intent
            .amount_msat
            .checked_add(intent.fee_limit_msat)
            .ok_or(LedgerError::InvalidInput)?;
        let total = exposure(&tx, None, None)?;
        let per_circuit = exposure(&tx, Some(intent.local_budget_scope), None)?;
        let per_round = exposure(&tx, Some(intent.local_budget_scope), Some(intent.round))?;
        enforce_exposure(
            total,
            maximum,
            account.lifetime_limit_msat,
            account.outstanding_limit_msat,
        )?;
        enforce_exposure(
            per_circuit,
            maximum,
            circuit.lifetime_limit_msat,
            circuit.outstanding_limit_msat,
        )?;
        if total.queued_intents >= account.max_queued_intents
            || total.unresolved_intents >= account.max_unresolved_intents
            || committed(per_round)?
                .checked_add(maximum)
                .ok_or(LedgerError::BudgetExceeded)?
                > circuit.per_round_limit_msat
        {
            return Err(LedgerError::BudgetExceeded);
        }
        storage::acquire_outgoing(&tx, intent.operation)?;
        tx.execute("INSERT INTO outgoing_intents(operation,instance,local_budget_scope,round,kind,payee_reference,charge_reference,payment_id,payment_hash,payload,state,closed,reserved_msat,spent_msat,outstanding_msat,fee_breached) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0,0,?11,0,?11,0)",params![&intent.operation[..],&intent.instance[..],&intent.local_budget_scope[..],sql_number(intent.round)?,intent.kind.code(),&intent.payee_reference[..],&intent.charge_reference[..],&intent.payment_id[..],&intent.payment_hash[..],encode(intent)?,sql_number(maximum)?])?;
        let record = read_outgoing(&tx, intent.operation)?.ok_or(LedgerError::InvalidLedger)?;
        storage::check_dispatch(&tx)?;
        tx.commit()?;
        Ok(record)
    }

    /// Claim dispatch exactly once, before any adapter call. A crash after this
    /// commit requires native lookup; another call cannot yield another permit.
    pub fn begin_dispatch(
        &mut self,
        operation: Id16,
        clock: ClockObservation,
    ) -> Result<DispatchPermit> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = read_outgoing(&tx, operation)?.ok_or(LedgerError::NotRegistered)?;
        if record.state != OutgoingState::Prepared {
            return Err(LedgerError::DispatchAlreadyClaimed);
        }
        if record.closed {
            return Err(LedgerError::Closed);
        }
        let (account, blocked) = account_budget(&tx)?;
        let (_, current, closed) = path_budget(&tx, record.intent.local_budget_scope)?;
        if closed {
            return Err(LedgerError::Closed);
        }
        if blocked || exposure(&tx, None, None)?.wallet_rpcs >= account.max_wallet_rpcs {
            return Err(LedgerError::BudgetExceeded);
        }
        storage::check_dispatch(&tx)?;
        origin_artifacts::require_managed_invoice(&tx, &record.intent, self.account)?;
        outgoing_renewal::check_intent(&tx, &record.intent, current)?;
        certain_time(clock.now_ms, clock.uncertainty_ms, record.intent.window)?;
        tx.execute(
            "UPDATE outgoing_intents SET state=1 WHERE operation=?1",
            params![&operation[..]],
        )?;
        tx.commit()?;
        Ok(DispatchPermit {
            intent: record.intent,
        })
    }

    pub fn mark_outgoing_unknown(&mut self, operation: Id16) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = read_outgoing(&tx, operation)?.ok_or(LedgerError::NotRegistered)?;
        match record.state {
            OutgoingState::InFlight => {
                tx.execute(
                    "UPDATE outgoing_intents SET state=2 WHERE operation=?1",
                    params![&operation[..]],
                )?;
            }
            OutgoingState::Unknown | OutgoingState::ClosedUnresolved => {}
            _ => return Err(LedgerError::Conflict),
        }
        tx.commit()?;
        Ok(())
    }

    /// Native terminal evidence alone releases uncertainty. Actual fees are
    /// recorded without clipping, including violations that freeze new spend.
    pub fn record_outgoing_outcome(
        &mut self,
        evidence: &OutgoingEvidence,
    ) -> Result<OutgoingRecord> {
        self.check_file_identity()?;
        if evidence.account != self.account {
            return Err(LedgerError::WrongAccount);
        }
        let payload = encode(evidence)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = read_outgoing(&tx, evidence.operation)?.ok_or(LedgerError::NotRegistered)?;
        let intent = &record.intent;
        if evidence.payment_hash != intent.payment_hash
            || evidence.invoice_digest != intent.invoice_digest
            || evidence.amount_msat != intent.amount_msat
        {
            return Err(LedgerError::InvalidEvidence);
        }
        let old: Option<Vec<u8>> = tx.query_row(
            "SELECT outcome FROM outgoing_intents WHERE operation=?1",
            params![&evidence.operation[..]],
            |r| r.get(0),
        )?;
        if let Some(old) = old {
            return if old == payload {
                Ok(record)
            } else {
                Err(LedgerError::Conflict)
            };
        }
        if !matches!(
            record.state,
            OutgoingState::InFlight | OutgoingState::Unknown | OutgoingState::ClosedUnresolved
        ) {
            return Err(LedgerError::Conflict);
        }
        let (state, principal) = match evidence.outcome {
            NativeOutcome::Settled => (3, intent.amount_msat),
            NativeOutcome::FailedFinal => (5, 0),
        };
        let spent = principal
            .checked_add(evidence.fee_msat)
            .and_then(|amount| i64::try_from(amount).ok());
        let Some(spent) = spent else {
            // Preserve the full native evidence, freeze admission, and retain
            // uncertainty when a native amount exceeds this schema's range.
            tx.execute("UPDATE account_budget SET blocked=1 WHERE singleton=1", [])?;
            tx.execute("UPDATE outgoing_intents SET state=CASE WHEN closed=1 THEN 6 ELSE 2 END,exception_evidence=?2 WHERE operation=?1",params![&evidence.operation[..],payload])?;
            tx.commit()?;
            return Err(LedgerError::InvalidEvidence);
        };
        let spent = u64::try_from(spent).map_err(|_| LedgerError::InvalidLedger)?;
        let breach = evidence.fee_msat > intent.fee_limit_msat;
        if breach {
            tx.execute("UPDATE account_budget SET blocked=1 WHERE singleton=1", [])?;
        }
        tx.execute("UPDATE outgoing_intents SET state=?2,reserved_msat=0,spent_msat=?3,outstanding_msat=?4,fee_breached=?5,outcome=?6 WHERE operation=?1",params![&evidence.operation[..],state,sql_number(spent)?,if state==3 {sql_number(spent)?}else{0},breach,payload])?;
        storage::release_outgoing(&tx, evidence.operation)?;
        let result = read_outgoing(&tx, evidence.operation)?.ok_or(LedgerError::InvalidLedger)?;
        tx.commit()?;
        Ok(result)
    }

    /// Cancel only an intent whose durable dispatch latch was never claimed.
    pub fn cancel_before_dispatch(&mut self, operation: Id16) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = read_outgoing(&tx, operation)?.ok_or(LedgerError::NotRegistered)?;
        if record.state == OutgoingState::CancelledBeforeDispatch {
            return Ok(());
        }
        if record.state != OutgoingState::Prepared {
            return Err(LedgerError::Conflict);
        }
        tx.execute("UPDATE outgoing_intents SET state=7,reserved_msat=0,outstanding_msat=0 WHERE operation=?1",params![&operation[..]])?;
        storage::release_outgoing(&tx, operation)?;
        tx.commit()?;
        Ok(())
    }

    /// Authenticated usage confirmation retires prepaid exposure, while actual
    /// spend continues counting toward lifetime caps. Closure cannot prove use.
    pub fn mark_outgoing_consumed(&mut self, operation: Id16) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let record = read_outgoing(&tx, operation)?.ok_or(LedgerError::NotRegistered)?;
        if record.state == OutgoingState::Consumed {
            return Ok(());
        }
        if record.closed {
            return Err(LedgerError::Closed);
        }
        if record.state != OutgoingState::Settled {
            return Err(LedgerError::Conflict);
        }
        tx.execute(
            "UPDATE outgoing_intents SET state=4,outstanding_msat=0 WHERE operation=?1",
            params![&operation[..]],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Caller supplies authenticated live-round progress. Old unresolved or
    /// prepaid obligations must be accounted as used before advancing; a new
    /// round cannot hide their exposure or purchase an unadmitted eleventh round.
    pub fn advance_spending_round(&mut self, binding: Hash32, round: u64) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (_, current, closed) = path_budget(&tx, binding)?;
        if round == current {
            return Ok(());
        }
        if closed {
            return Err(LedgerError::Closed);
        }
        if round != current + 1 {
            return Err(LedgerError::InvalidInput);
        }
        outgoing_renewal::check_advance(&tx, binding, round)?;
        let old:i64=tx.query_row("SELECT COALESCE(SUM(outstanding_msat),0) FROM outgoing_intents WHERE local_budget_scope=?1 AND round>0 AND round<?2",params![&binding[..],sql_number(round)?],|r|r.get(0))?;
        if old != 0 {
            return Err(LedgerError::BudgetExceeded);
        }
        tx.execute(
            "UPDATE path_budgets SET current_round=?2 WHERE local_budget_scope=?1",
            params![&binding[..], sql_number(round)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_outgoing(&self, operation: Id16) -> Result<Option<OutgoingRecord>> {
        self.check_file_identity()?;
        read_outgoing(&self.connection, operation)
    }

    /// Read retained progress without mutating or guessing forward on restart.
    pub fn spending_round(&self, scope: Hash32) -> Result<u64> {
        self.check_file_identity()?;
        Ok(path_budget(&self.connection, scope)?.1)
    }
    /// Close financial admission for one failed live circuit. This does not send
    /// a Tor command and cannot turn an uncertain dispatched payment into failure.
    pub fn close_outgoing_path(&mut self, binding: Hash32) -> Result<()> {
        self.check_file_identity()?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        path_budget(&tx, binding)?;
        tx.execute("UPDATE outgoing_intents SET closed=1,state=CASE WHEN state=0 THEN 7 WHEN state IN(1,2) THEN 6 ELSE state END,reserved_msat=CASE WHEN state=0 THEN 0 ELSE reserved_msat END,outstanding_msat=CASE WHEN state=0 THEN 0 ELSE outstanding_msat END WHERE local_budget_scope=?1",params![&binding[..]])?;
        tx.execute(
            "UPDATE path_budgets SET closed=1 WHERE local_budget_scope=?1",
            params![&binding[..]],
        )?;
        handshake::close(&tx, "scope=?1", &binding[..])?;
        storage::release_cancelled(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub fn account_exposure(&self) -> Result<Exposure> {
        self.check_file_identity()?;
        exposure(&self.connection, None, None)
    }
    pub fn path_exposure(&self, binding: Hash32) -> Result<Exposure> {
        self.check_file_identity()?;
        exposure(&self.connection, Some(binding), None)
    }
    pub fn outgoing_admission_blocked(&self) -> Result<bool> {
        self.check_file_identity()?;
        Ok(account_budget(&self.connection)?.1)
    }

    /// Bounded recovery hints, ordered by operation ID. Each operation must be
    /// queried again before action; pagination is not an immutable snapshot.
    pub fn unresolved_outgoing(
        &self,
        after: Option<Id16>,
        limit: u64,
    ) -> Result<Vec<OutgoingRecord>> {
        self.check_file_identity()?;
        if limit == 0 || limit > 128 {
            return Err(LedgerError::InvalidInput);
        }
        let mut statement=self.connection.prepare("SELECT operation FROM outgoing_intents WHERE state IN(0,1,2,3,6) AND (?1 IS NULL OR operation>?1) ORDER BY operation LIMIT ?2")?;
        let ids = statement
            .query_map(
                params![after.as_ref().map(|id| &id[..]), sql_number(limit)?],
                |r| r.get::<_, Vec<u8>>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| {
                read_outgoing(
                    &self.connection,
                    id.try_into().map_err(|_| LedgerError::InvalidLedger)?,
                )?
                .ok_or(LedgerError::InvalidLedger)
            })
            .collect()
    }
}

fn validate_limits(lifetime: u64, outstanding: u64) -> Result<()> {
    sql_number(lifetime)?;
    sql_number(outstanding)?;
    if outstanding > lifetime {
        return Err(LedgerError::InvalidInput);
    }
    Ok(())
}

pub(super) fn validate_intent(intent: &OutgoingIntent, account: AccountId) -> Result<()> {
    if intent.account != account {
        return Err(LedgerError::WrongAccount);
    }
    let fields = [
        intent.local_budget_scope,
        intent.payment_id,
        intent.charge_reference,
        intent.payee_reference,
        intent.accepted_terms_digest,
        intent.hop_circuit_binding,
        intent.hop_policy_hash,
        intent.invoice_digest,
        intent.payment_hash,
    ];
    if intent.operation == [0; 16]
        || intent.instance == [0; 16]
        || intent.handle == [0; 16]
        || fields.contains(&[0; 32])
        || intent.amount_msat == 0
        || (intent.round == 0) != (intent.kind == ChargeKind::Handshake)
        || intent.window.opens_at_ms < 0
        || intent.window.closes_at_ms < intent.window.opens_at_ms
    {
        return Err(LedgerError::InvalidInput);
    }
    sql_number(intent.round)?;
    sql_number(
        intent
            .amount_msat
            .checked_add(intent.fee_limit_msat)
            .ok_or(LedgerError::InvalidInput)?,
    )?;
    Ok(())
}

pub(super) fn account_budget(connection: &Connection) -> Result<(AccountBudget, bool)> {
    let row: Option<(Vec<u8>, bool)> = connection
        .query_row(
            "SELECT payload,blocked FROM account_budget WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (payload, blocked) = row.ok_or(LedgerError::NotRegistered)?;
    Ok((
        serde_json::from_slice(&payload).map_err(|_| LedgerError::InvalidLedger)?,
        blocked,
    ))
}

pub(super) fn path_budget(
    connection: &Connection,
    binding: Hash32,
) -> Result<(PathBudget, u64, bool)> {
    let row: Option<(Vec<u8>, i64, bool)> = connection
        .query_row(
            "SELECT payload,current_round,closed FROM path_budgets WHERE local_budget_scope=?1",
            params![&binding[..]],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (payload, current, closed) = row.ok_or(LedgerError::NotRegistered)?;
    Ok((
        serde_json::from_slice(&payload).map_err(|_| LedgerError::InvalidLedger)?,
        u64::try_from(current).map_err(|_| LedgerError::InvalidLedger)?,
        closed,
    ))
}

fn match_path(intent: &OutgoingIntent, circuit: &PathBudget) -> Result<()> {
    if intent.instance != circuit.instance
        || intent.handle != circuit.handle
        || intent.accepted_terms_digest != circuit.accepted_terms_digest
    {
        Err(LedgerError::Conflict)
    } else {
        Ok(())
    }
}

fn match_hop_terms(connection: &Connection, intent: &OutgoingIntent) -> Result<()> {
    let payload:Vec<u8>=connection.query_row("SELECT payload FROM hop_spending_terms WHERE local_budget_scope=?1 AND charge_reference=?2",params![&intent.local_budget_scope[..],&intent.charge_reference[..]],|r|r.get(0)).optional()?.ok_or(LedgerError::NotRegistered)?;
    let terms: HopSpendingTerms =
        serde_json::from_slice(&payload).map_err(|_| LedgerError::InvalidLedger)?;
    let expected = if intent.kind == ChargeKind::Handshake {
        terms.handshake_amount_msat
    } else {
        terms.round_amount_msat
    };
    if intent.hop_circuit_binding != terms.hop_circuit_binding
        || intent.hop_policy_hash != terms.hop_policy_hash
        || intent.payee_reference != terms.payee_reference
        || intent.amount_msat != expected
        || intent.fee_limit_msat > terms.fee_limit_msat
    {
        return Err(LedgerError::Conflict);
    }
    Ok(())
}

fn committed(exposure: Exposure) -> Result<u64> {
    exposure
        .reserved_msat
        .checked_add(exposure.spent_msat)
        .ok_or(LedgerError::BudgetExceeded)
}
pub(super) fn enforce_exposure(
    exposure: Exposure,
    maximum: u64,
    lifetime: u64,
    outstanding: u64,
) -> Result<()> {
    if committed(exposure)?
        .checked_add(maximum)
        .ok_or(LedgerError::BudgetExceeded)?
        > lifetime
        || exposure
            .outstanding_msat
            .checked_add(maximum)
            .ok_or(LedgerError::BudgetExceeded)?
            > outstanding
    {
        Err(LedgerError::BudgetExceeded)
    } else {
        Ok(())
    }
}

pub(super) fn exposure(
    connection: &Connection,
    binding: Option<Hash32>,
    round: Option<u64>,
) -> Result<Exposure> {
    let round = round.map(sql_number).transpose()?;
    let row:(i64,i64,i64,i64,i64,i64)=connection.query_row("SELECT COALESCE(SUM(reserved_msat),0),COALESCE(SUM(spent_msat),0),COALESCE(SUM(outstanding_msat),0),COALESCE(SUM(state=0),0),COALESCE(SUM(state=1),0),COALESCE(SUM(state IN(0,1,2,3,6)),0) FROM (SELECT local_budget_scope,round,state,reserved_msat,spent_msat,outstanding_msat FROM outgoing_intents UNION ALL SELECT scope AS local_budget_scope,0 AS round,state,reserved_msat,spent_msat,outstanding_msat FROM handshake_intents) WHERE (?1 IS NULL OR local_budget_scope=?1) AND (?2 IS NULL OR round=?2)",params![binding.as_ref().map(|b|&b[..]),round],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
    let convert = |value| u64::try_from(value).map_err(|_| LedgerError::InvalidLedger);
    Ok(Exposure {
        reserved_msat: convert(row.0)?,
        spent_msat: convert(row.1)?,
        outstanding_msat: convert(row.2)?,
        queued_intents: convert(row.3)?,
        wallet_rpcs: convert(row.4)?,
        unresolved_intents: convert(row.5)?,
    })
}

fn read_outgoing(connection: &Connection, operation: Id16) -> Result<Option<OutgoingRecord>> {
    let row:Option<(Vec<u8>,i64,bool,i64,i64,i64,bool)>=connection.query_row("SELECT payload,state,closed,reserved_msat,spent_msat,outstanding_msat,fee_breached FROM outgoing_intents WHERE operation=?1",params![&operation[..]],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional()?;
    row.map(
        |(payload, state, closed, reserved, spent, outstanding, breach)| {
            Ok(OutgoingRecord {
                intent: serde_json::from_slice(&payload).map_err(|_| LedgerError::InvalidLedger)?,
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
            })
        },
    )
    .transpose()
}

pub(super) fn close_instance(tx: &rusqlite::Transaction<'_>, instance: Id16) -> Result<()> {
    tx.execute("UPDATE outgoing_intents SET closed=1,state=CASE WHEN state=0 THEN 7 WHEN state IN(1,2) THEN 6 ELSE state END,reserved_msat=CASE WHEN state=0 THEN 0 ELSE reserved_msat END,outstanding_msat=CASE WHEN state=0 THEN 0 ELSE outstanding_msat END WHERE instance=?1",params![&instance[..]])?;
    tx.execute(
        "UPDATE path_budgets SET closed=1 WHERE instance=?1",
        params![&instance[..]],
    )?;
    handshake::close(tx, "instance=?1", &instance[..])?;
    storage::release_cancelled(tx)?;
    Ok(())
}

pub(super) fn validate_schema(connection: &Connection) -> Result<()> {
    connection.prepare("SELECT a.payload,c.current_round,o.outcome,h.payload FROM account_budget a,path_budgets c,outgoing_intents o,hop_spending_terms h LIMIT 0")?;
    Ok(())
}

pub(super) const SCHEMA:&str="
CREATE TABLE account_budget(singleton INTEGER PRIMARY KEY CHECK(singleton=1),payload BLOB NOT NULL,blocked INTEGER NOT NULL CHECK(blocked IN(0,1)));
CREATE TABLE path_budgets(local_budget_scope BLOB PRIMARY KEY CHECK(length(local_budget_scope)=32),instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),payload BLOB NOT NULL,current_round INTEGER NOT NULL CHECK(current_round>=1),closed INTEGER NOT NULL CHECK(closed IN(0,1)),UNIQUE(instance,handle));
CREATE TABLE hop_spending_terms(local_budget_scope BLOB NOT NULL REFERENCES path_budgets(local_budget_scope),charge_reference BLOB NOT NULL CHECK(length(charge_reference)=32),hop_circuit_binding BLOB NOT NULL CHECK(length(hop_circuit_binding)=32),payload BLOB NOT NULL,PRIMARY KEY(local_budget_scope,charge_reference),UNIQUE(local_budget_scope,hop_circuit_binding));
CREATE TABLE outgoing_intents(operation BLOB PRIMARY KEY CHECK(length(operation)=16),instance BLOB NOT NULL CHECK(length(instance)=16),local_budget_scope BLOB NOT NULL REFERENCES path_budgets(local_budget_scope),round INTEGER NOT NULL CHECK(round>=0),kind INTEGER NOT NULL CHECK(kind IN(0,1)),payee_reference BLOB NOT NULL CHECK(length(payee_reference)=32),charge_reference BLOB NOT NULL CHECK(length(charge_reference)=32),payment_id BLOB NOT NULL UNIQUE CHECK(length(payment_id)=32),payment_hash BLOB NOT NULL UNIQUE CHECK(length(payment_hash)=32),payload BLOB NOT NULL,state INTEGER NOT NULL CHECK(state BETWEEN 0 AND 7),closed INTEGER NOT NULL CHECK(closed IN(0,1)),reserved_msat INTEGER NOT NULL CHECK(reserved_msat>=0),spent_msat INTEGER NOT NULL CHECK(spent_msat>=0),outstanding_msat INTEGER NOT NULL CHECK(outstanding_msat>=0),fee_breached INTEGER NOT NULL CHECK(fee_breached IN(0,1)),outcome BLOB,exception_evidence BLOB,UNIQUE(local_budget_scope,round,kind,charge_reference));
";

#[cfg(test)]
#[path = "outgoing_tests.rs"]
mod tests;
