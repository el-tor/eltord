//! Local control events are hints to query state, never payment evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaymentRole {
    Origin,
    Relay,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaymentEventKind {
    AdmissionPending,
    Registered,
    Committed,
    StartRequested,
    Authorized,
    RoundActive,
    RenewAdmitted,
    RenewRefused,
    Grace,
    Expired,
    Closed,
}
#[derive(Clone, PartialEq, Eq)]
pub struct PaymentEvent {
    pub instance: [u8; 16],
    pub sequence: u64,
    pub handle: [u8; 16],
    pub role: PaymentRole,
    pub revision: u64,
    pub kind: PaymentEventKind,
}
impl PaymentEvent {
    /// Parse one complete CRLF-terminated event from an authenticated control
    /// connection. The caller must bound its receive buffer before this parser.
    pub fn parse(line: &str) -> Result<Self, &'static str> {
        const BAD: &str = "Invalid payment state event";
        if line.len() > 256 {
            return Err(BAD);
        }
        let body = line.strip_suffix("\r\n").ok_or(BAD)?;
        let fields: Vec<_> = body.split(' ').collect();
        if fields.len() != 9 || fields[..3] != ["650", "ELTOR_PAYMENT_STATE", "1"] {
            return Err(BAD);
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
        fn count(s: &str) -> Result<u64, &'static str> {
            let n: u64 = s.parse().map_err(|_| BAD)?;
            if n == 0 || n.to_string() != s {
                return Err(BAD);
            }
            Ok(n)
        }
        Ok(Self {
            instance: id(fields[3])?,
            sequence: count(fields[4])?,
            handle: id(fields[5])?,
            role: match fields[6] {
                "ORIGIN" => PaymentRole::Origin,
                "RELAY" => PaymentRole::Relay,
                _ => return Err(BAD),
            },
            revision: count(fields[7])?,
            kind: match fields[8] {
                "ADMISSION_PENDING" => PaymentEventKind::AdmissionPending,
                "REGISTERED" => PaymentEventKind::Registered,
                "COMMITTED" => PaymentEventKind::Committed,
                "START_REQUESTED" => PaymentEventKind::StartRequested,
                "AUTHORIZED" => PaymentEventKind::Authorized,
                "ROUND_ACTIVE" => PaymentEventKind::RoundActive,
                "RENEW_ADMITTED" => PaymentEventKind::RenewAdmitted,
                "RENEW_REFUSED" => PaymentEventKind::RenewRefused,
                "GRACE" => PaymentEventKind::Grace,
                "EXPIRED" => PaymentEventKind::Expired,
                "CLOSED" => PaymentEventKind::Closed,
                _ => return Err(BAD),
            },
        })
    }
}

/// Result of observing a hint. Even Query requires a fresh authoritative
/// snapshot before any financial action; it is never an authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaymentEventAction {
    Ignore,
    Query,
    Resynchronize,
}

/// Constant-space stream continuity guard. Install a cursor only after a full
/// inventory snapshot has been validated while buffering subscribed events.
/// A gap poisons the cursor until a fresh inventory is installed: later events
/// cannot silently repair a missing transition.
#[derive(Default)]
pub struct PaymentEventCursor {
    snapshot: Option<([u8; 16], u64)>,
}
impl PaymentEventCursor {
    pub fn from_inventory(instance: [u8; 16], sequence: u64) -> Result<Self, &'static str> {
        if instance == [0; 16] || sequence == u64::MAX {
            return Err("Invalid payment inventory cursor");
        }
        Ok(Self {
            snapshot: Some((instance, sequence)),
        })
    }
    /// Also call on parse errors, bounded-buffer overflow, expired inventory,
    /// or control disconnect. This never changes a ledger/payment outcome.
    pub fn invalidate(&mut self) {
        self.snapshot = None;
    }
    pub fn observe(&mut self, event: &PaymentEvent) -> PaymentEventAction {
        let Some((instance, previous)) = self.snapshot else {
            return PaymentEventAction::Resynchronize;
        };
        if event.instance != instance {
            self.invalidate();
            return PaymentEventAction::Resynchronize;
        }
        if event.sequence <= previous {
            return PaymentEventAction::Ignore;
        }
        if previous.checked_add(1) != Some(event.sequence) || event.sequence == u64::MAX {
            self.invalidate();
            return PaymentEventAction::Resynchronize;
        }
        self.snapshot = Some((instance, event.sequence));
        PaymentEventAction::Query
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const EVENT: &str = "650 ELTOR_PAYMENT_STATE 1 11111111111111111111111111111111 42 22222222222222222222222222222222 RELAY 7 ADMISSION_PENDING\r\n";
    #[test]
    fn payment_event_strict_frame() {
        let event = PaymentEvent::parse(EVENT).unwrap();
        assert_eq!(event.sequence, 42);
        assert_eq!(event.revision, 7);
        assert_eq!(event.role, PaymentRole::Relay);
        assert_eq!(event.kind, PaymentEventKind::AdmissionPending);
        for bad in [
            EVENT.replace(" 42 ", " 042 "),
            EVENT.replace(" 42 ", " 0 "),
            EVENT.replace(" 42 ", " 18446744073709551616 "),
            EVENT.replace(" 7 ", " 0 "),
            EVENT.replace(" RELAY ", " UNKNOWN "),
            EVENT.replace("ADMISSION_PENDING", "PAID"),
            EVENT.replace("650 ", "650  "),
            EVENT.replace("\r\n", "\n"),
            EVENT.replace(
                "11111111111111111111111111111111",
                "00000000000000000000000000000000",
            ),
            EVENT.replace(
                "22222222222222222222222222222222",
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            ),
            format!("{}{}", EVENT, EVENT),
        ] {
            assert!(PaymentEvent::parse(&bad).is_err());
        }
    }
    #[test]
    fn payment_event_continuity_requires_inventory() {
        let mut event = PaymentEvent::parse(EVENT).unwrap();
        let mut missing = PaymentEventCursor::default();
        assert_eq!(missing.observe(&event), PaymentEventAction::Resynchronize);
        let mut cursor = PaymentEventCursor::from_inventory(event.instance, 41).unwrap();
        assert_eq!(cursor.observe(&event), PaymentEventAction::Query);
        assert_eq!(cursor.observe(&event), PaymentEventAction::Ignore);
        event.sequence = 40;
        assert_eq!(cursor.observe(&event), PaymentEventAction::Ignore);
        event.sequence = 44;
        assert_eq!(cursor.observe(&event), PaymentEventAction::Resynchronize);
        event.sequence = 43;
        assert_eq!(cursor.observe(&event), PaymentEventAction::Resynchronize);
        cursor = PaymentEventCursor::from_inventory(event.instance, 42).unwrap();
        assert_eq!(cursor.observe(&event), PaymentEventAction::Query);
        event.instance = [3; 16];
        assert_eq!(cursor.observe(&event), PaymentEventAction::Resynchronize);
        cursor = PaymentEventCursor::from_inventory(event.instance, 43).unwrap();
        cursor.invalidate();
        assert_eq!(cursor.observe(&event), PaymentEventAction::Resynchronize);
        cursor = PaymentEventCursor::from_inventory(event.instance, u64::MAX - 1).unwrap();
        event.sequence = u64::MAX;
        assert_eq!(cursor.observe(&event), PaymentEventAction::Resynchronize);
        assert!(PaymentEventCursor::from_inventory(event.instance, u64::MAX).is_err());
        assert!(PaymentEventCursor::from_inventory([0; 16], 0).is_err());
        assert!(PaymentEventCursor::from_inventory(event.instance, 0).is_ok());
    }
}
