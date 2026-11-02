//! Bounded, immutable inventory recovery. Partial pages never produce a usable
//! inventory, and an inventory is not proof of wallet settlement.
use super::{PaymentEventCursor, PaymentRole};
const BAD: &str = "Invalid payment inventory";

#[derive(Clone, PartialEq, Eq)]
pub struct PaymentInventoryRow {
    pub handle: [u8; 16],
    pub role: PaymentRole,
    pub revision: u64,
}
pub struct PaymentInventory {
    pub instance: [u8; 16],
    pub sequence: u64,
    pub rows: Vec<PaymentInventoryRow>,
}
impl PaymentInventory {
    pub fn event_cursor(&self) -> Result<PaymentEventCursor, &'static str> {
        PaymentEventCursor::from_inventory(self.instance, self.sequence)
    }
}
#[derive(Default)]
pub struct PaymentInventoryBuilder {
    identity: Option<([u8; 16], u64)>,
    rows: Vec<PaymentInventoryRow>,
    cursors: Vec<[u8; 16]>,
    complete: bool,
    failed: bool,
}
fn id(s: &str) -> Result<[u8; 16], &'static str> {
    if s.len() != 32
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(BAD);
    }
    let mut out = [0; 16];
    hex::decode_to_slice(s, &mut out).map_err(|_| BAD)?;
    if out == [0; 16] {
        return Err(BAD);
    }
    Ok(out)
}
fn number(s: &str) -> Result<u64, &'static str> {
    let n: u64 = s.parse().map_err(|_| BAD)?;
    if n.to_string() != s {
        return Err(BAD);
    }
    Ok(n)
}
impl PaymentInventoryBuilder {
    /// Input uses command_response's normalized LF framing, without trailing LF.
    /// Call only for the response to the requested cursor. Network failures or
    /// cursor expiration require discarding this builder and starting again.
    pub fn add_page(&mut self, reply: &str) -> Result<Option<[u8; 16]>, &'static str> {
        if self.failed || self.complete {
            return Err(BAD);
        }
        let result = self.add_page_inner(reply);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn add_page_inner(&mut self, reply: &str) -> Result<Option<[u8; 16]>, &'static str> {
        if reply.len() > 16384 {
            return Err(BAD);
        }
        let lines: Vec<_> = reply.split('\n').collect();
        if lines.len() < 2 || lines.len() > 130 || lines.last() != Some(&"250 OK") {
            return Err(BAD);
        }
        let header: Vec<_> = lines[0].split(' ').collect();
        if header.len() != 5 || header[..2] != ["250-ELTOR_PAYMENT_LIST", "1"] {
            return Err(BAD);
        }
        let identity = (id(header[2])?, number(header[3])?);
        if identity.1 == u64::MAX || self.identity.is_some_and(|old| old != identity) {
            return Err(BAD);
        }
        let next = if header[4] == "-" {
            None
        } else {
            Some(id(header[4])?)
        };
        if next.is_some_and(|cursor| self.cursors.contains(&cursor)) {
            return Err(BAD);
        }
        let count = lines.len() - 2;
        if self.rows.len() + count > 4096 || (next.is_some() && count != 128) {
            return Err(BAD);
        }
        let mut page = Vec::with_capacity(count);
        let mut previous = self.rows.last().map(|row| row.handle);
        for line in &lines[1..lines.len() - 1] {
            let fields: Vec<_> = line.split(' ').collect();
            if fields.len() != 4 || fields[0] != "250-CIRCUIT" {
                return Err(BAD);
            }
            let handle = id(fields[1])?;
            if previous.is_some_and(|old| old >= handle) {
                return Err(BAD);
            }
            let role = match fields[2] {
                "ORIGIN" => PaymentRole::Origin,
                "RELAY" => PaymentRole::Relay,
                _ => return Err(BAD),
            };
            let revision = number(fields[3])?;
            if revision == 0 {
                return Err(BAD);
            }
            page.push(PaymentInventoryRow {
                handle,
                role,
                revision,
            });
            previous = Some(handle);
        }
        if next.is_some() && self.rows.len() + count >= 4096 {
            return Err(BAD);
        }
        self.identity = Some(identity);
        self.rows.extend(page);
        if let Some(cursor) = next {
            self.cursors.push(cursor);
        } else {
            self.complete = true;
        }
        Ok(next)
    }
    pub fn finish(self) -> Result<PaymentInventory, &'static str> {
        if self.failed || !self.complete {
            return Err(BAD);
        }
        let (instance, sequence) = self.identity.ok_or(BAD)?;
        Ok(PaymentInventory {
            instance,
            sequence,
            rows: self.rows,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn page(start: u128, count: usize, next: &str) -> String {
        let mut out = format!("250-ELTOR_PAYMENT_LIST 1 {:032x} 500 {}\n", 1, next);
        for n in start..start + count as u128 {
            out += &format!("250-CIRCUIT {:032x} RELAY 1\n", n);
        }
        out + "250 OK"
    }
    #[test]
    fn inventory_requires_complete_consistent_pages() {
        let mut b = PaymentInventoryBuilder::default();
        let cursor = format!("{:032x}", 999);
        assert_eq!(
            b.add_page(&page(1, 128, &cursor)).unwrap(),
            Some(id(&cursor).unwrap())
        );
        assert_eq!(b.add_page(&page(129, 1, "-")).unwrap(), None);
        let inventory = b.finish().unwrap();
        assert_eq!(inventory.rows.len(), 129);
        assert!(inventory.event_cursor().is_ok());
        let mut b = PaymentInventoryBuilder::default();
        b.add_page(&page(1, 128, &cursor)).unwrap();
        assert!(b.finish().is_err());
        for second in [
            page(128, 1, "-"),
            page(129, 1, "-").replace(" 500 ", " 501 "),
            page(129, 128, &cursor),
        ] {
            let mut b = PaymentInventoryBuilder::default();
            b.add_page(&page(1, 128, &cursor)).unwrap();
            assert!(b.add_page(&second).is_err());
            assert!(b.finish().is_err());
        }
    }
    #[test]
    fn inventory_rejects_partial_and_noncanonical_rows() {
        for reply in [
            page(1, 1, "-").replace(" RELAY 1", " RELAY 0"),
            page(1, 1, "-").replace(" RELAY 1", " RELAY 01"),
            page(1, 1, "-").replace("250 OK", "250 OK\n"),
            page(1, 1, &format!("{:032x}", 999)),
            page(1, 129, "-"),
            page(1, 1, "-").replace("RELAY", "UNKNOWN"),
        ] {
            let mut b = PaymentInventoryBuilder::default();
            assert!(b.add_page(&reply).is_err());
            assert!(b.finish().is_err());
        }
        let mut b = PaymentInventoryBuilder::default();
        b.add_page(&page(1, 0, "-")).unwrap();
        assert_eq!(b.finish().unwrap().rows.len(), 0);
    }
    #[test]
    fn inventory_enforces_total_capacity_and_cursor_progress() {
        let mut b = PaymentInventoryBuilder::default();
        for index in 0..32 {
            let next = if index == 31 {
                "-".to_string()
            } else {
                format!("{:032x}", 10000 + index)
            };
            b.add_page(&page(1 + index * 128, 128, &next)).unwrap();
        }
        assert_eq!(b.finish().unwrap().rows.len(), 4096);
        let mut b = PaymentInventoryBuilder::default();
        for index in 0..31 {
            b.add_page(&page(
                1 + index * 128,
                128,
                &format!("{:032x}", 10000 + index),
            ))
            .unwrap();
        }
        assert!(b
            .add_page(&page(3969, 128, &format!("{:032x}", 10031)))
            .is_err());
        assert!(b.finish().is_err());
    }
}
