//! Durable, account-local candidate discovery and relay binding recovery.
//! Candidate hashes are lookup hints, never settlement/authorization evidence.
use super::*;
use lni::cln::foundation::{
    IncomingInvoiceCandidate, IncomingUpdatePage, INCOMING_UPDATE_PAGE_LIMIT,
};

#[derive(Clone, PartialEq, Eq)]
pub struct StoredIncomingCandidate {
    pub payment_hash: Hash32,
    pub invoice_digest: Hash32,
}
impl PaymentLedger {
    pub fn account_id(&self) -> AccountId {
        self.account
    }
    pub fn native_invoice_cursor(&self) -> Result<u64> {
        self.check_file_identity()?;
        let (provider, next): (String, i64) = self.connection.query_row(
            "SELECT provider,next_updated_index FROM native_invoice_cursor WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if provider != "cln_updated_v1" || next < 1 {
            return Err(LedgerError::InvalidLedger);
        }
        Ok(next as u64)
    }
    /// Commit the bounded native page and all relevant associations together.
    /// No restart can advance a cursor past a mapping that was not committed.
    pub fn apply_native_invoice_page(&mut self, page: &IncomingUpdatePage) -> Result<()> {
        self.check_file_identity()?;
        if AccountId(page.account.fingerprint()) != self.account {
            return Err(LedgerError::WrongAccount);
        }
        if page.from_index == 0
            || page.next_index < page.from_index
            || page.rows_seen > INCOMING_UPDATE_PAGE_LIMIT
            || page.candidates.len() > page.rows_seen
            || (page.rows_seen == 0
                && (page.next_index != page.from_index || !page.candidates.is_empty()))
            || (page.rows_seen > 0
                && (page.next_index == page.from_index
                    || page.next_index - page.from_index < page.rows_seen as u64))
        {
            return Err(LedgerError::InvalidEvidence);
        }
        let from = sql_number(page.from_index)?;
        let next = sql_number(page.next_index)?;
        let mut prior = None;
        for candidate in &page.candidates {
            if candidate.payment_id == [0; 32]
                || candidate.payment_hash == [0; 32]
                || candidate.invoice_digest == [0; 32]
                || candidate.updated_index < page.from_index
                || candidate.updated_index >= page.next_index
                || prior.map_or(false, |n| candidate.updated_index <= n)
            {
                return Err(LedgerError::InvalidEvidence);
            }
            prior = Some(candidate.updated_index);
        }
        let digest = page_digest(page);
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (old,old_from,old_digest):(i64,Option<i64>,Option<Vec<u8>>)=tx.query_row("SELECT next_updated_index,last_from_index,last_page_digest FROM native_invoice_cursor WHERE singleton=1 AND provider='cln_updated_v1'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if old == next && old_from == Some(from) && old_digest.as_deref() == Some(&digest[..]) {
            return Ok(());
        }
        if old != from {
            return Err(LedgerError::Conflict);
        }
        for candidate in &page.candidates {
            let registered: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM payment_ids WHERE payment_id=?1)",
                params![&candidate.payment_id[..]],
                |r| r.get(0),
            )?;
            if !registered {
                continue;
            }
            store_candidate(&tx, candidate)?;
        }
        tx.execute("UPDATE native_invoice_cursor SET next_updated_index=?1,last_from_index=?2,last_page_digest=?3 WHERE singleton=1",params![next,from,&digest[..]])?;
        tx.commit()?;
        Ok(())
    }
    /// Multiple paid native invoices with one ID require explicit reconciliation.
    /// Never arbitrarily choose a winner or infer a refund from this ambiguity.
    pub fn incoming_candidate(
        &self,
        payment_id: Hash32,
    ) -> Result<Option<StoredIncomingCandidate>> {
        self.check_file_identity()?;
        let rows:Vec<(Vec<u8>,Vec<u8>)>=self.connection.prepare("SELECT payment_hash,invoice_digest FROM native_invoice_candidates WHERE payment_id=?1 LIMIT 3")?
            .query_map(params![&payment_id[..]],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<std::result::Result<_,_>>()?;
        if rows.len() > 1 {
            return Err(LedgerError::Conflict);
        }
        rows.into_iter()
            .next()
            .map(|(hash, digest)| {
                Ok(StoredIncomingCandidate {
                    payment_hash: hash.try_into().map_err(|_| LedgerError::InvalidLedger)?,
                    invoice_digest: digest.try_into().map_err(|_| LedgerError::InvalidLedger)?,
                })
            })
            .transpose()
    }
    pub fn get_handshake_obligation(
        &self,
        instance: Id16,
        handle: Id16,
    ) -> Result<Option<Obligation>> {
        self.check_file_identity()?;
        let payloads:Vec<Vec<u8>>=self.connection.prepare("SELECT payload FROM registrations WHERE instance=?1 AND handle=?2 AND round=0 AND kind=0 LIMIT 2")?.query_map(params![&instance[..],&handle[..]],|r|r.get(0))?.collect::<std::result::Result<_,_>>()?;
        if payloads.len() > 1 {
            return Err(LedgerError::InvalidLedger);
        }
        payloads
            .into_iter()
            .next()
            .map(|p| {
                let o: Obligation =
                    serde_json::from_slice(&p).map_err(|_| LedgerError::InvalidLedger)?;
                o.validate().map_err(|_| LedgerError::InvalidLedger)?;
                if o.instance != instance
                    || o.handle != handle
                    || o.round != 0
                    || o.kind != ChargeKind::Handshake
                {
                    return Err(LedgerError::InvalidLedger);
                }
                Ok(o)
            })
            .transpose()
    }
    pub fn latest_admission_period(&self, instance: Id16, handle: Id16) -> Result<Option<u64>> {
        self.check_file_identity()?;
        let n: Option<i64> = self.connection.query_row(
            "SELECT max(period) FROM batches WHERE instance=?1 AND handle=?2",
            params![&instance[..], &handle[..]],
            |r| r.get(0),
        )?;
        n.map(|n| u64::try_from(n).map_err(|_| LedgerError::InvalidLedger))
            .transpose()
    }
    pub fn binding_is_closed(&self, instance: Id16, handle: Id16) -> Result<bool> {
        self.check_file_identity()?;
        Ok(self.connection.query_row("SELECT EXISTS(SELECT 1 FROM closed_bindings WHERE instance=?1 AND handle=?2) OR EXISTS(SELECT 1 FROM closed_instances WHERE instance=?1)",params![&instance[..],&handle[..]],|r|r.get(0))?)
    }
    /// Authoritative UNKNOWN_CIRCUIT closes only that local binding; funds stay
    /// consumed/unknown and cannot be reassigned by a later event or query.
    pub fn close_binding(&mut self, instance: Id16, handle: Id16) -> Result<()> {
        self.check_file_identity()?;
        if instance == [0; 16] || handle == [0; 16] {
            return Err(LedgerError::InvalidInput);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT OR IGNORE INTO closed_bindings(instance,handle) VALUES(?1,?2)",
            params![&instance[..], &handle[..]],
        )?;
        tx.execute("UPDATE authorizations SET state=2 WHERE state=0 AND registration_id IN(SELECT id FROM registrations WHERE instance=?1 AND handle=?2)",params![&instance[..],&handle[..]])?;
        tx.execute(
            "UPDATE registrations SET closed=1 WHERE instance=?1 AND handle=?2",
            params![&instance[..], &handle[..]],
        )?;
        tx.execute("UPDATE outgoing_intents SET closed=1,state=CASE WHEN state=0 THEN 7 WHEN state IN(1,2) THEN 6 ELSE state END,reserved_msat=CASE WHEN state=0 THEN 0 ELSE reserved_msat END,outstanding_msat=CASE WHEN state=0 THEN 0 ELSE outstanding_msat END WHERE local_budget_scope IN(SELECT local_budget_scope FROM path_budgets WHERE instance=?1 AND handle=?2)",params![&instance[..],&handle[..]])?;
        tx.execute(
            "UPDATE path_budgets SET closed=1 WHERE instance=?1 AND handle=?2",
            params![&instance[..], &handle[..]],
        )?;
        storage::release_cancelled(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub fn close_other_instances(&mut self, current: Id16) -> Result<()> {
        self.check_file_identity()?;
        if current == [0; 16] {
            return Err(LedgerError::InvalidInput);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT OR IGNORE INTO closed_instances(instance) SELECT instance FROM registrations WHERE instance!=?1 UNION SELECT instance FROM batches WHERE instance!=?1 UNION SELECT instance FROM path_budgets WHERE instance!=?1 UNION SELECT instance FROM origin_operations WHERE instance!=?1 UNION SELECT instance FROM origin_constructions WHERE instance!=?1",params![&current[..]])?;
        tx.execute("UPDATE authorizations SET state=2 WHERE state=0 AND registration_id IN(SELECT id FROM registrations WHERE instance!=?1)",params![&current[..]])?;
        tx.execute(
            "UPDATE registrations SET closed=1 WHERE instance!=?1",
            params![&current[..]],
        )?;
        tx.execute("UPDATE outgoing_intents SET closed=1,state=CASE WHEN state=0 THEN 7 WHEN state IN(1,2) THEN 6 ELSE state END,reserved_msat=CASE WHEN state=0 THEN 0 ELSE reserved_msat END,outstanding_msat=CASE WHEN state=0 THEN 0 ELSE outstanding_msat END WHERE instance!=?1",params![&current[..]])?;
        tx.execute(
            "UPDATE path_budgets SET closed=1 WHERE instance!=?1",
            params![&current[..]],
        )?;
        storage::release_cancelled(&tx)?;
        tx.commit()?;
        Ok(())
    }
}
fn store_candidate(
    tx: &rusqlite::Transaction<'_>,
    candidate: &IncomingInvoiceCandidate,
) -> Result<()> {
    let old:Option<Vec<u8>>=tx.query_row("SELECT invoice_digest FROM native_invoice_candidates WHERE payment_id=?1 AND payment_hash=?2",params![&candidate.payment_id[..],&candidate.payment_hash[..]],|r|r.get(0)).optional()?;
    if let Some(old) = old {
        return if old == candidate.invoice_digest {
            Ok(())
        } else {
            Err(LedgerError::Conflict)
        };
    }
    let count: i64 = tx.query_row(
        "SELECT count(*) FROM native_invoice_candidates WHERE payment_id=?1",
        params![&candidate.payment_id[..]],
        |r| r.get(0),
    )?;
    if count >= 2 {
        return Err(LedgerError::BudgetExceeded);
    }
    tx.execute("INSERT INTO native_invoice_candidates(payment_id,payment_hash,invoice_digest,updated_index) VALUES(?1,?2,?3,?4)",params![&candidate.payment_id[..],&candidate.payment_hash[..],&candidate.invoice_digest[..],sql_number(candidate.updated_index)?])?;
    Ok(())
}
fn page_digest(page: &IncomingUpdatePage) -> Hash32 {
    let mut hash = Sha256::new();
    hash.update(b"eltor/cln-incoming-page/v1\0");
    hash.update(page.account.fingerprint());
    hash.update(page.from_index.to_be_bytes());
    hash.update(page.next_index.to_be_bytes());
    hash.update((page.rows_seen as u64).to_be_bytes());
    hash.update((page.candidates.len() as u64).to_be_bytes());
    for c in &page.candidates {
        hash.update(c.payment_id);
        hash.update(c.payment_hash);
        hash.update(c.invoice_digest);
        hash.update(c.updated_index.to_be_bytes());
    }
    hash.finalize().into()
}
pub(super) fn require_open_binding(
    connection: &Connection,
    instance: Id16,
    handle: Id16,
) -> Result<()> {
    let closed: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM closed_bindings WHERE instance=?1 AND handle=?2)",
        params![&instance[..], &handle[..]],
        |r| r.get(0),
    )?;
    if closed {
        return Err(LedgerError::Closed);
    }
    Ok(())
}
pub(super) fn validate_schema(connection: &Connection) -> Result<()> {
    connection.prepare("SELECT c.next_updated_index,c.last_page_digest,i.payment_hash,b.handle FROM native_invoice_cursor c,native_invoice_candidates i,closed_bindings b LIMIT 0")?;
    let valid:bool=connection.query_row("SELECT count(*)=1 FROM native_invoice_cursor WHERE singleton=1 AND provider='cln_updated_v1' AND next_updated_index>=1",[],|r|r.get(0))?;
    if !valid {
        return Err(LedgerError::InvalidLedger);
    }
    let invalid:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM native_invoice_candidates GROUP BY payment_id HAVING count(*)>2) OR EXISTS(SELECT 1 FROM native_invoice_cursor WHERE next_updated_index<1 OR (last_from_index IS NULL)!=(last_page_digest IS NULL) OR (last_from_index IS NOT NULL AND (last_from_index<1 OR last_from_index>next_updated_index OR length(last_page_digest)!=32)))",[],|r|r.get(0))?;
    if invalid {
        return Err(LedgerError::InvalidLedger);
    }
    Ok(())
}
pub(super) const SCHEMA:&str="
CREATE TABLE closed_bindings(instance BLOB NOT NULL CHECK(length(instance)=16),handle BLOB NOT NULL CHECK(length(handle)=16),PRIMARY KEY(instance,handle));
CREATE TABLE native_invoice_cursor(singleton INTEGER PRIMARY KEY CHECK(singleton=1),provider TEXT NOT NULL CHECK(provider='cln_updated_v1'),next_updated_index INTEGER NOT NULL CHECK(next_updated_index>=1),last_from_index INTEGER CHECK(last_from_index>=1),last_page_digest BLOB CHECK(length(last_page_digest)=32),CHECK((last_from_index IS NULL)=(last_page_digest IS NULL)));
INSERT INTO native_invoice_cursor(singleton,provider,next_updated_index) VALUES(1,'cln_updated_v1',1);
CREATE TABLE native_invoice_candidates(payment_id BLOB NOT NULL REFERENCES payment_ids(payment_id) CHECK(length(payment_id)=32),payment_hash BLOB NOT NULL UNIQUE CHECK(length(payment_hash)=32),invoice_digest BLOB NOT NULL CHECK(length(invoice_digest)=32),updated_index INTEGER NOT NULL CHECK(updated_index>=1),PRIMARY KEY(payment_id,payment_hash));
";
#[cfg(test)]
mod tests;
