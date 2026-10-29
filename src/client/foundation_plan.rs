//! Immutable origin-path acceptance. Signed descriptor metadata is a quote;
//! accepting this object is an explicit local monetary decision, never a wallet
//! action. The complete accepted bytes are retained in the authoritative ledger.
use super::*;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;

pub const MAX_HOPS: usize = 16;
pub const MAX_PLAN_BYTES: usize = 65536;

#[derive(Clone)]
pub struct FoundationHop {
    pub descriptor: FoundationDescriptor,
    pub payment_ids: [[u8; 32]; 10],
    pub fee_limit_msat: u64,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoundationLimits {
    pub lifetime_msat: u64,
    pub outstanding_msat: u64,
    pub per_round_msat: u64,
}
/// Read-only staged observations. Its fields are private so a local receipt,
/// substituted price, or unverified registration cannot manufacture acceptance.
pub struct FoundationDraft {
    pub(super) binding: OriginBinding,
    pub(super) hops: Vec<AcceptedHop>,
    pub(super) construction: ClockObservation,
}
#[derive(Clone)]
pub(super) struct AcceptedHop {
    pub descriptor: FoundationDescriptor,
    pub registration: OriginRegistration,
    pub fee_limit_msat: u64,
}
pub struct FoundationPlan {
    pub(super) binding: OriginBinding,
    pub(super) hops: Vec<AcceptedHop>,
    pub(super) construction: ClockObservation,
    pub(super) budget: PathBudget,
    pub(super) seed: [u8; 32],
    pub(super) account: AccountId,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedHop {
    fingerprint: String,
    terms: String,
    offer: Option<String>,
    circuit_binding: String,
    policy_hash: String,
    allocation: String,
    ids: Vec<String>,
    fee_limit_msat: u64,
    revision: u64,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedClock {
    now_ms: i64,
    uncertainty_ms: u64,
}
impl From<ClockObservation> for SavedClock {
    fn from(c: ClockObservation) -> Self {
        Self {
            now_ms: c.now_ms,
            uncertainty_ms: c.uncertainty_ms,
        }
    }
}
impl From<SavedClock> for ClockObservation {
    fn from(c: SavedClock) -> Self {
        Self {
            now_ms: c.now_ms,
            uncertainty_ms: c.uncertainty_ms,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedPlan {
    version: u8,
    account: AccountId,
    instance: String,
    handle: String,
    revision: u64,
    construction: SavedClock,
    budget: PathBudget,
    seed: String,
    hops: Vec<SavedHop>,
}
fn decode<const N: usize>(s: &str) -> Result<[u8; N]> {
    if s.len() != N * 2
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let mut out = [0; N];
    hex::decode_to_slice(s, &mut out).map_err(|_| invalid())?;
    Ok(out)
}
impl FoundationDraft {
    /// The constructor owner supplies its before-construction UTC sample. A
    /// later BIND receipt must never become a replacement setup-clock origin.
    pub async fn inspect(
        control: &mut PaymentControl,
        binding: OriginBinding,
        expected: &[FoundationHop],
        construction: ClockObservation,
    ) -> Result<Self> {
        validate_clock(construction)?;
        if expected.is_empty()
            || expected.len() > MAX_HOPS
            || binding.hops as usize != expected.len()
        {
            return Err(invalid());
        }
        let mut hops = Vec::with_capacity(expected.len());
        let mut ids = std::collections::BTreeSet::new();
        let mut fingerprints = std::collections::BTreeSet::new();
        for (index, expected) in expected.iter().enumerate() {
            if !fingerprints.insert(expected.descriptor.fingerprint) {
                return Err(invalid());
            }
            let policy = FoundationPolicy::decode(&expected.descriptor.terms)?;
            if policy.rate_msat > 0 && expected.descriptor.bolt12_offer.is_none() {
                return Err(
                    "Positive-price foundation hop has no supported pinned receiver".into(),
                );
            }
            if expected
                .descriptor
                .bolt12_offer
                .as_ref()
                .map_or(false, |offer| offer.is_empty() || offer.len() > 2048)
            {
                return Err(invalid());
            }
            for id in expected.payment_ids {
                if id == [0; 32] || !ids.insert(id) {
                    return Err(invalid());
                }
            }
            let registration = control
                .origin_registration(&binding, index as u16 + 1, &expected.payment_ids)
                .await?;
            if registration.terms() != expected.descriptor.terms {
                return Err(invalid());
            }
            hops.push(AcceptedHop {
                descriptor: expected.descriptor.clone(),
                registration,
                fee_limit_msat: expected.fee_limit_msat,
            });
        }
        Ok(Self {
            binding,
            hops,
            construction,
        })
    }
    pub fn total_round_msat(&self) -> Result<u64> {
        self.hops.iter().try_fold(0u64, |sum, hop| {
            sum.checked_add(FoundationPolicy::decode(&hop.descriptor.terms)?.rate_msat)
                .ok_or_else(invalid)
        })
    }
    /// The caller has explicitly accepted these finite limits. No larger cap,
    /// new account, missing database or replacement plan is inferred here.
    pub fn accept(
        self,
        ledger: &mut PaymentLedger,
        scope: [u8; 32],
        limits: FoundationLimits,
    ) -> Result<FoundationPlan> {
        if scope == [0; 32]
            || limits.outstanding_msat > limits.lifetime_msat
            || limits.per_round_msat > limits.lifetime_msat
        {
            return Err(invalid());
        }
        if ledger
            .get_origin_plan(self.binding.instance, self.binding.handle)?
            .is_some()
        {
            return Err("Existing accepted foundation path must be restored".into());
        }
        let required = self.hops.iter().try_fold(0u64, |sum, hop| {
            sum.checked_add(FoundationPolicy::decode(&hop.descriptor.terms)?.rate_msat)
                .and_then(|n| n.checked_add(hop.fee_limit_msat))
                .ok_or_else(invalid)
        })?;
        if required > limits.per_round_msat || required > limits.outstanding_msat {
            return Err(invalid());
        }
        let mut seed = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);
        let mut plan = FoundationPlan {
            account: ledger.account_id(),
            binding: self.binding,
            hops: self.hops,
            construction: self.construction,
            seed,
            budget: PathBudget {
                instance: [0; 16],
                handle: [0; 16],
                local_budget_scope: scope,
                accepted_terms_digest: [0; 32],
                lifetime_limit_msat: limits.lifetime_msat,
                outstanding_limit_msat: limits.outstanding_msat,
                per_round_limit_msat: limits.per_round_msat,
                funding: FundingSource::OwnWallet,
            },
        };
        plan.budget.instance = plan.binding.instance;
        plan.budget.handle = plan.binding.handle;
        plan.budget.accepted_terms_digest = plan.acceptance_digest()?;
        let payload = plan.encode()?;
        ledger.save_origin_plan(plan.binding.instance, plan.binding.handle, scope, &payload)?;
        plan.configure(ledger)?;
        Ok(plan)
    }
}
impl FoundationPlan {
    pub fn binding(&self) -> &OriginBinding {
        &self.binding
    }
    pub fn local_scope(&self) -> [u8; 32] {
        self.budget.local_budget_scope
    }
    pub fn budget(&self) -> &PathBudget {
        &self.budget
    }
    pub fn setup_deadline_ms(&self, hop: usize) -> Result<i64> {
        let policy =
            FoundationPolicy::decode(&self.hops.get(hop).ok_or_else(invalid)?.descriptor.terms)?;
        self.construction
            .now_ms
            .checked_sub(i64::try_from(self.construction.uncertainty_ms)?)
            .and_then(|n| n.checked_add(i64::from(policy.setup_timeout_ms)))
            .ok_or_else(invalid)
    }
    pub(super) fn charge_reference(&self, hop: usize) -> [u8; 32] {
        let reg = &self.hops[hop].registration;
        digest(
            b"eltor/origin/v1/hop-charge",
            &[
                &reg.circuit_binding(),
                &reg.policy_hash(),
                &reg.initial_allocation(),
            ],
        )
    }
    pub(super) fn payee_reference(&self, hop: usize) -> [u8; 32] {
        let d = &self.hops[hop].descriptor;
        digest(
            b"eltor/origin/v1/pinned-receiver",
            &[
                &d.fingerprint,
                d.bolt12_offer.as_deref().unwrap_or("").as_bytes(),
            ],
        )
    }
    /// Fresh secret seed is persisted before dispatch. Domain separation gives
    /// independent stable identities for each hop/kind/sequence across recovery.
    pub(super) fn identity(&self, hop: usize, domain: &[u8], sequence: u64) -> [u8; 16] {
        digest(
            domain,
            &[
                &self.seed,
                &(hop as u16 + 1).to_be_bytes(),
                &sequence.to_be_bytes(),
            ],
        )[..16]
            .try_into()
            .unwrap()
    }
    fn saved(&self) -> SavedPlan {
        SavedPlan {
            version: 1,
            account: self.account,
            instance: hex::encode(self.binding.instance),
            handle: hex::encode(self.binding.handle),
            revision: self.binding.revision,
            construction: self.construction.into(),
            budget: self.budget.clone(),
            seed: hex::encode(self.seed),
            hops: self
                .hops
                .iter()
                .map(|h| SavedHop {
                    fingerprint: hex::encode(h.descriptor.fingerprint),
                    terms: hex::encode(h.descriptor.terms),
                    offer: h.descriptor.bolt12_offer.clone(),
                    circuit_binding: hex::encode(h.registration.circuit_binding()),
                    policy_hash: hex::encode(h.registration.policy_hash()),
                    allocation: hex::encode(h.registration.initial_allocation()),
                    ids: h
                        .registration
                        .payment_ids()
                        .iter()
                        .map(hex::encode)
                        .collect(),
                    fee_limit_msat: h.fee_limit_msat,
                    revision: h.registration.revision,
                })
                .collect(),
        }
    }
    fn acceptance_digest(&self) -> Result<[u8; 32]> {
        let mut saved = self.saved();
        saved.budget.accepted_terms_digest = [0; 32];
        // Operational randomness does not change the accepted monetary terms.
        saved.seed.clear();
        Ok(digest(
            b"eltor/origin/v1/accepted-path",
            &[&serde_json::to_vec(&saved)?],
        ))
    }
    pub(super) fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(&self.saved())?;
        if bytes.len() > MAX_PLAN_BYTES {
            return Err(invalid());
        }
        Ok(bytes)
    }
    pub fn restore(
        ledger: &mut PaymentLedger,
        instance: [u8; 16],
        handle: [u8; 16],
    ) -> Result<Self> {
        let record = ledger
            .get_origin_plan(instance, handle)?
            .ok_or("No durable accepted foundation path")?;
        if record.payload.len() > MAX_PLAN_BYTES {
            return Err(invalid());
        }
        let saved: SavedPlan = serde_json::from_slice(&record.payload)?;
        validate_clock(saved.construction.into())?;
        if saved.version != 1
            || saved.account != ledger.account_id()
            || saved.hops.is_empty()
            || saved.hops.len() > MAX_HOPS
            || decode::<16>(&saved.instance)? != instance
            || decode::<16>(&saved.handle)? != handle
            || saved.revision == 0
            || record.local_budget_scope != saved.budget.local_budget_scope
            || saved.budget.instance != instance
            || saved.budget.handle != handle
        {
            return Err(invalid());
        }
        let binding = OriginBinding {
            instance,
            handle,
            hops: saved.hops.len() as u16,
            revision: saved.revision,
        };
        let mut hops = Vec::with_capacity(saved.hops.len());
        let mut all_ids = std::collections::BTreeSet::new();
        let mut fingerprints = std::collections::BTreeSet::new();
        for (index, h) in saved.hops.iter().enumerate() {
            let terms = decode::<55>(&h.terms)?;
            FoundationPolicy::decode(&terms)?;
            let fingerprint = decode::<20>(&h.fingerprint)?;
            if !fingerprints.insert(fingerprint)
                || h.ids.len() != 10
                || h.offer
                    .as_ref()
                    .map_or(false, |s| s.is_empty() || s.len() > 2048)
            {
                return Err(invalid());
            }
            let mut ids = [[0; 32]; 10];
            for (to, from) in ids.iter_mut().zip(&h.ids) {
                *to = decode(from)?;
                if !all_ids.insert(*to) {
                    return Err(invalid());
                }
            }
            let mut wire = format!("250-ELTOR_INITIAL_REGISTRATION 1 {} {} {} {}\n250-POLICY {} {} {}\n250-BINDING {}\n", saved.instance, saved.handle, index + 1, h.revision, h.terms, h.policy_hash, h.allocation, h.circuit_binding);
            for (round, id) in h.ids.iter().enumerate() {
                wire += &format!("250-ROUND {} {}\n", round + 1, id);
            }
            wire += "250 OK";
            let registration = OriginRegistration::parse(&wire, &binding, index as u16 + 1, &ids)?;
            hops.push(AcceptedHop {
                descriptor: FoundationDescriptor {
                    fingerprint,
                    terms,
                    bolt12_offer: h.offer.clone(),
                },
                registration,
                fee_limit_msat: h.fee_limit_msat,
            });
        }
        let plan = Self {
            binding,
            hops,
            construction: saved.construction.into(),
            budget: saved.budget,
            seed: decode(&saved.seed)?,
            account: saved.account,
        };
        if plan.seed == [0; 32] || plan.budget.accepted_terms_digest != plan.acceptance_digest()? {
            return Err(invalid());
        }
        plan.configure(ledger)?;
        Ok(plan)
    }
    pub(super) fn configure(&self, ledger: &mut PaymentLedger) -> Result<()> {
        if ledger.account_id() != self.account
            || ledger.binding_is_closed(self.binding.instance, self.binding.handle)?
        {
            return Err(invalid());
        }
        ledger.configure_path_budget(&self.budget)?;
        for (index, hop) in self.hops.iter().enumerate() {
            let policy = FoundationPolicy::decode(&hop.descriptor.terms)?;
            ledger.configure_hop_spending_terms(&HopSpendingTerms {
                local_budget_scope: self.local_scope(),
                charge_reference: self.charge_reference(index),
                hop_circuit_binding: hop.registration.circuit_binding(),
                hop_policy_hash: hop.registration.policy_hash(),
                payee_reference: self.payee_reference(index),
                handshake_amount_msat: policy.handshake_fee_msat,
                round_amount_msat: policy.rate_msat,
                fee_limit_msat: hop.fee_limit_msat,
            })?;
        }
        for (index, hop) in self.hops.iter().enumerate() {
            ledger.register_origin_initial_allocation(
                self.local_scope(),
                self.charge_reference(index),
                &hop.registration,
            )?;
        }
        Ok(())
    }
}
