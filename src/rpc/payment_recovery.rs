//! Bounded query work after subscribed inventory recovery. This contains no
//! financial state: neither a hint nor an absent handle authorizes spending.
use super::{
    PaymentEvent, PaymentEventAction, PaymentEventCursor, PaymentInventory, PaymentInventoryRow,
    PaymentRole,
};
use std::collections::BTreeMap;

const BAD: &str = "Payment recovery requires a new inventory";
struct Entry {
    role: PaymentRole,
    revision: u64,
    queried: bool,
}
pub struct PaymentRecovery {
    instance: [u8; 16],
    cursor: PaymentEventCursor,
    entries: BTreeMap<[u8; 16], Entry>,
    valid: bool,
}
impl PaymentRecovery {
    /// Reconcile buffered hints only after the complete immutable inventory.
    /// Every listed handle still requires an authoritative typed state query.
    pub fn new(inventory: PaymentInventory, events: &[PaymentEvent]) -> Result<Self, &'static str> {
        let cursor = inventory.event_cursor()?;
        let mut this = Self {
            instance: inventory.instance,
            cursor,
            entries: BTreeMap::new(),
            valid: true,
        };
        if inventory.rows.len() > 4096 {
            return Err(BAD);
        }
        for row in inventory.rows {
            if row.handle == [0; 16]
                || row.revision == 0
                || this
                    .entries
                    .insert(
                        row.handle,
                        Entry {
                            role: row.role,
                            revision: row.revision,
                            queried: false,
                        },
                    )
                    .is_some()
            {
                return Err(BAD);
            }
        }
        for event in events {
            this.observe(event)?;
        }
        Ok(this)
    }
    pub fn instance(&self) -> [u8; 16] {
        self.instance
    }
    pub fn invalidate(&mut self) {
        self.valid = false;
        self.cursor.invalidate();
    }
    pub fn observe(&mut self, event: &PaymentEvent) -> Result<(), &'static str> {
        if !self.valid {
            return Err(BAD);
        }
        match self.cursor.observe(event) {
            PaymentEventAction::Ignore => return Ok(()),
            PaymentEventAction::Resynchronize => {
                self.invalidate();
                return Err(BAD);
            }
            PaymentEventAction::Query => (),
        }
        if event.handle == [0; 16] || event.revision == 0 {
            self.invalidate();
            return Err(BAD);
        }
        if let Some(entry) = self.entries.get_mut(&event.handle) {
            if entry.role != event.role {
                self.invalidate();
                return Err(BAD);
            }
            if event.revision > entry.revision {
                entry.revision = event.revision;
                entry.queried = false;
            }
        } else {
            // Includes CLOSED: a closure hint is not authoritative reconciliation.
            if self.entries.len() >= 4096 {
                self.invalidate();
                return Err(BAD);
            }
            self.entries.insert(
                event.handle,
                Entry {
                    role: event.role,
                    revision: event.revision,
                    queried: false,
                },
            );
        }
        Ok(())
    }
    pub fn next_query(&self) -> Result<Option<PaymentInventoryRow>, &'static str> {
        if !self.valid {
            return Err(BAD);
        }
        Ok(self
            .entries
            .iter()
            .find(|(_, entry)| !entry.queried)
            .map(|(handle, entry)| PaymentInventoryRow {
                handle: *handle,
                role: entry.role,
                revision: entry.revision,
            }))
    }
    /// Role filtering does not mark a row queried; each selected entry still
    /// requires its role-specific authoritative query before reconciliation.
    pub fn next_query_for(
        &self,
        role: PaymentRole,
    ) -> Result<Option<PaymentInventoryRow>, &'static str> {
        if !self.valid {
            return Err(BAD);
        }
        Ok(self
            .entries
            .iter()
            .find(|(_, e)| e.role == role && !e.queried)
            .map(|(handle, e)| PaymentInventoryRow {
                handle: *handle,
                role: e.role,
                revision: e.revision,
            }))
    }
    pub fn tracked_rows_for(
        &self,
        role: PaymentRole,
    ) -> Result<Vec<PaymentInventoryRow>, &'static str> {
        if !self.valid {
            return Err(BAD);
        }
        Ok(self
            .entries
            .iter()
            .filter(|(_, e)| e.role == role)
            .map(|(handle, e)| PaymentInventoryRow {
                handle: *handle,
                role: e.role,
                revision: e.revision,
            })
            .collect())
    }
    /// Call only after validating a full typed query response on this connection.
    /// Drain buffered hints first: a newer hint keeps an older response pending.
    /// A successful return means query bookkeeping only, never paid activation.
    pub fn queried(
        &mut self,
        instance: [u8; 16],
        row: &PaymentInventoryRow,
    ) -> Result<bool, &'static str> {
        if !self.valid || instance != self.instance {
            self.invalidate();
            return Err(BAD);
        }
        let Some(entry) = self.entries.get_mut(&row.handle) else {
            self.invalidate();
            return Err(BAD);
        };
        if entry.role != row.role || row.revision == 0 {
            self.invalidate();
            return Err(BAD);
        }
        if row.revision < entry.revision {
            return Ok(false);
        }
        entry.revision = row.revision;
        entry.queried = true;
        Ok(true)
    }
    /// UNKNOWN_CIRCUIT requires financial reconciliation by the caller. This
    /// helper deliberately cannot turn it into a successful state observation.
    pub fn missing(&mut self, _handle: [u8; 16]) {
        self.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::PaymentEventKind;
    fn row(revision: u64) -> PaymentInventoryRow {
        PaymentInventoryRow {
            handle: [2; 16],
            role: PaymentRole::Relay,
            revision,
        }
    }
    fn inventory() -> PaymentInventory {
        PaymentInventory {
            instance: [1; 16],
            sequence: 10,
            rows: vec![row(2)],
        }
    }
    fn event(sequence: u64, revision: u64) -> PaymentEvent {
        PaymentEvent {
            instance: [1; 16],
            sequence,
            handle: [2; 16],
            role: PaymentRole::Relay,
            revision,
            kind: PaymentEventKind::RoundActive,
        }
    }
    #[test]
    fn buffered_events_and_query_races_do_not_authorize_or_skip_state() {
        let mut r = PaymentRecovery::new(inventory(), &[event(10, 1), event(11, 3)]).unwrap();
        assert_eq!(r.next_query().unwrap().unwrap().revision, 3);
        assert!(!r.queried([1; 16], &row(2)).unwrap());
        assert!(r.queried([1; 16], &row(3)).unwrap());
        assert!(r.next_query().unwrap().is_none());
        r.observe(&event(12, 2)).unwrap();
        assert!(r.next_query().unwrap().is_none());
        r.observe(&event(13, 4)).unwrap();
        assert_eq!(r.next_query().unwrap().unwrap().revision, 4);
        assert!(!r.queried([1; 16], &row(3)).unwrap());
    }
    #[test]
    fn gaps_role_changes_and_missing_state_require_full_recovery() {
        let mut r = PaymentRecovery::new(inventory(), &[]).unwrap();
        assert!(r.observe(&event(12, 3)).is_err());
        assert!(r.observe(&event(11, 3)).is_err());
        assert!(r.next_query().is_err());
        let mut r = PaymentRecovery::new(inventory(), &[]).unwrap();
        let mut e = event(11, 3);
        e.role = PaymentRole::Origin;
        assert!(r.observe(&e).is_err());
        let mut r = PaymentRecovery::new(inventory(), &[]).unwrap();
        r.missing([2; 16]);
        assert!(r.queried([1; 16], &row(3)).is_err());
        let mut r = PaymentRecovery::new(inventory(), &[]).unwrap();
        assert!(r.queried([3; 16], &row(3)).is_err());
    }
    #[test]
    fn role_selectors_do_not_skip_another_role_or_mark_queries_complete() {
        let mut i = inventory();
        i.rows.push(PaymentInventoryRow {
            handle: [1; 16],
            role: PaymentRole::Origin,
            revision: 1,
        });
        let mut r = PaymentRecovery::new(i, &[]).unwrap();
        assert!(r.next_query().unwrap().unwrap().role == PaymentRole::Origin);
        assert!(
            r.next_query_for(PaymentRole::Relay)
                .unwrap()
                .unwrap()
                .handle
                == [2; 16]
        );
        assert_eq!(r.tracked_rows_for(PaymentRole::Relay).unwrap().len(), 1);
        assert!(r.next_query_for(PaymentRole::Relay).unwrap().is_some());
        r.invalidate();
        assert!(r.tracked_rows_for(PaymentRole::Relay).is_err());
    }
    #[test]
    fn churn_cannot_grow_recovery_without_bound() {
        let mut i = inventory();
        i.rows = (1u128..=4096)
            .map(|n| PaymentInventoryRow {
                handle: n.to_be_bytes(),
                role: PaymentRole::Relay,
                revision: 1,
            })
            .collect();
        let mut r = PaymentRecovery::new(i, &[]).unwrap();
        let mut e = event(11, 1);
        e.handle = 4097u128.to_be_bytes();
        e.kind = PaymentEventKind::Closed;
        assert!(r.observe(&e).is_err());
        assert!(r.next_query().is_err());
    }
}
