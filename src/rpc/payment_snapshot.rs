//! Strict decoding of the privileged pending-admission snapshot. No wallet or
//! ledger mutation occurs here; a parsed candidate is not permission to pay.
use crate::payment_ledger::{BatchRegistration, ChargeKind, Obligation};
use sha2::{Digest, Sha256};
use std::convert::TryInto;

const INVALID: &str = "Invalid pending payment admission snapshot";

#[derive(Clone, PartialEq, Eq)]
pub struct PendingAdmission {
    instance: [u8; 16],
    handle: [u8; 16],
    revision: u64,
    period: u64,
    request_digest: [u8; 32],
    remaining_ms: u64,
    terms: [u8; 55],
    binding: [u8; 32],
    initial_allocation: [u8; 32],
    allocation: [u8; 32],
    ids: [[u8; 32]; 10],
}

fn number(s: &str) -> Result<u64, &'static str> {
    let n: u64 = s.parse().map_err(|_| INVALID)?;
    if s != n.to_string() {
        return Err(INVALID);
    }
    Ok(n)
}
fn bytes<const N: usize>(s: &str) -> Result<[u8; N], &'static str> {
    if s.len() != 2 * N
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(INVALID);
    }
    let mut out = [0; N];
    hex::decode_to_slice(s, &mut out).map_err(|_| INVALID)?;
    Ok(out)
}
fn domain_hash(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update((domain.len() as u16).to_be_bytes());
    h.update(domain);
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}
fn u64_at(terms: &[u8; 55], at: usize) -> u64 {
    u64::from_be_bytes(terms[at..at + 8].try_into().expect("fixed terms offset"))
}
fn u32_at(terms: &[u8; 55], at: usize) -> u32 {
    u32::from_be_bytes(terms[at..at + 4].try_into().expect("fixed terms offset"))
}
impl PendingAdmission {
    /// Input has already passed control-channel authentication and response
    /// framing. Expected bindings originate from the retained event/query.
    pub fn parse(
        reply: &str,
        instance: [u8; 16],
        handle: [u8; 16],
        period: u64,
    ) -> Result<Self, &'static str> {
        if reply.len() > 8192 || instance == [0; 16] || handle == [0; 16] {
            return Err(INVALID);
        }
        let rows: Vec<Vec<&str>> = reply.split('\n').map(|r| r.split(' ').collect()).collect();
        if rows.len() != 17 {
            return Err(INVALID);
        }
        let a = &rows[0];
        if a.len() != 8
            || a[0] != "250-ELTOR_ADMISSION"
            || a[1] != "1"
            || bytes::<16>(a[2])? != instance
            || bytes::<16>(a[3])? != handle
            || number(a[5])? != period
        {
            return Err(INVALID);
        }
        let revision = number(a[4])?;
        let remaining_ms = number(a[7])?;
        if revision == 0 || remaining_ms == 0 {
            return Err(INVALID);
        }
        let reg = &rows[1];
        if reg.len() != 6
            || reg[0] != "250-ELTOR_REGISTRATION"
            || reg[1] != "1"
            || bytes::<16>(reg[2])? != instance
            || bytes::<16>(reg[3])? != handle
            || number(reg[4])? != revision
            || number(reg[5])? != period
        {
            return Err(INVALID);
        }
        let p = &rows[2];
        if p.len() != 5 || p[0] != "250-POLICY" {
            return Err(INVALID);
        }
        let terms = bytes::<55>(p[1])?;
        if terms[..3] != [1, 1, 10]
            || u32_at(&terms, 11) != 60000
            || u32_at(&terms, 15) != 60000
            || u32_at(&terms, 19) > 60000
            || u64_at(&terms, 31) == 0
            || u32_at(&terms, 39) < 2
            || u32_at(&terms, 51) == 0
        {
            return Err(INVALID);
        }
        let first = period
            .checked_mul(10)
            .and_then(|n| n.checked_add(1))
            .ok_or(INVALID)?;
        first.checked_add(9).ok_or(INVALID)?;
        if period >= u32_at(&terms, 39) as u64 {
            return Err(INVALID);
        }
        if period > 0 {
            let end = period
                .checked_add(1)
                .and_then(|n| n.checked_mul(600000))
                .ok_or(INVALID)?;
            if end > u64_at(&terms, 31) {
                return Err(INVALID);
            }
        }
        if number(p[4])? != first {
            return Err(INVALID);
        }
        let b = &rows[3];
        if b.len() != 2 || b[0] != "250-BINDING" {
            return Err(INVALID);
        }
        let binding = bytes::<32>(b[1])?;
        if binding == [0; 32] {
            return Err(INVALID);
        }
        let state = &rows[4];
        if state.len() != 6 || state[0] != "250-STATE" || state[1] != "ADMISSION_PENDING" {
            return Err(INVALID);
        }
        for value in &state[2..] {
            number(value)?;
        }
        let fee = &rows[5];
        if fee.len() != 5
            || fee[0] != "250-FEE"
            || number(fee[1])? != u64_at(&terms, 43)
            || fee[3] != "-"
            || fee[4] != "NONE"
        {
            return Err(INVALID);
        }
        let fee_hash = bytes::<32>(fee[2])?;
        if (u64_at(&terms, 43) == 0) != (fee_hash == [0; 32]) {
            return Err(INVALID);
        }
        let mut ids = [[0; 32]; 10];
        for i in 0..10 {
            let r = &rows[6 + i];
            if r.len() != 9
                || r[0] != "250-ROUND"
                || number(r[1])? != first + i as u64
                || number(r[3])? != u64_at(&terms, 3)
                || r[4] != "-"
                || r[5] != "-"
                || r[6] != "-"
                || r[8] != "NONE"
            {
                return Err(INVALID);
            }
            if r[7] != "-" && !(u64_at(&terms, 3) == 0 && bytes::<32>(r[7])? == [0; 32]) {
                return Err(INVALID);
            }
            ids[i] = bytes::<32>(r[2])?;
            if ids[i] == [0; 32] || ids[..i].contains(&ids[i]) {
                return Err(INVALID);
            }
        }
        if rows[16] != ["250", "OK"] {
            return Err(INVALID);
        }
        let s = Self {
            instance,
            handle,
            revision,
            period,
            request_digest: bytes(a[6])?,
            remaining_ms,
            terms,
            binding,
            initial_allocation: bytes(p[2])?,
            allocation: bytes(p[3])?,
            ids,
        };
        if s.request_digest == [0; 32] || s.initial_allocation == [0; 32] || s.allocation == [0; 32]
        {
            return Err(INVALID);
        }
        if period == 0 {
            let q = s.policy_hash();
            let flat: Vec<u8> = ids.iter().flatten().copied().collect();
            let a0 = domain_hash(b"eltor/payment-renew/v1/initial", &[&binding, &q, &flat]);
            if s.initial_allocation != a0 || s.allocation != a0 {
                return Err(INVALID);
            }
        }
        Ok(s)
    }
    pub fn policy_hash(&self) -> [u8; 32] {
        domain_hash(b"eltor/payment-renew/v1/terms", &[&self.terms])
    }
    /// Reserve only. Windows remain absent until authenticated COMMIT/T0
    /// observations are anchored; this cannot authorize a charge or activation.
    pub fn registration(&self) -> BatchRegistration {
        BatchRegistration {
            period: self.period,
            request_digest: self.request_digest,
            rounds: std::array::from_fn(|i| Obligation {
                instance: self.instance,
                handle: self.handle,
                circuit_binding: self.binding,
                round: self.period * 10 + 1 + i as u64,
                kind: ChargeKind::Round,
                payment_id: self.ids[i],
                amount_msat: u64_at(&self.terms, 3),
                expected_payment_hash: None,
                policy_hash: self.policy_hash(),
                allocation_hash: self.initial_allocation,
                window: None,
            }),
        }
    }
}

/// Read-only candidate lookup. A timeout or parse failure never permits a
/// fallback registration, payment, or renewed deadline.
pub async fn get_pending_admission(
    config: &crate::types::RpcConfig,
    instance: [u8; 16],
    handle: [u8; 16],
    period: u64,
) -> Result<PendingAdmission, Box<dyn std::error::Error>> {
    if instance == [0; 16]
        || handle == [0; 16]
        || period
            .checked_mul(10)
            .and_then(|n| n.checked_add(10))
            .is_none()
    {
        return Err(INVALID.into());
    }
    let response = super::rpc_client_limited(
        crate::types::RpcConfig {
            addr: config.addr.clone(),
            rpc_password: config.rpc_password.clone(),
            command: format!(
                "ELTOR_PAYMENT_ADMISSION_GET 1 {} {} {}",
                hex::encode(instance),
                hex::encode(handle),
                period
            ),
        },
        8192,
    )
    .await?;
    let reply = super::command_response(&response).map_err(|_| INVALID)?;
    PendingAdmission::parse(&reply, instance, handle, period).map_err(Into::into)
}

/// Persist an admission decision before notifying Tor. The caller supplies an
/// already provisioned, verified recipient-account ledger; this function never
/// creates a ledger or contacts a wallet. A failed notification leaves the
/// reservation intact for exact recovery, and never authorizes payment.
pub async fn reserve_pending_admission(
    config: &crate::types::RpcConfig,
    ledger: &mut crate::payment_ledger::PaymentLedger,
    instance: [u8; 16],
    handle: [u8; 16],
    period: u64,
    decision: [u8; 16],
) -> Result<(crate::payment_ledger::BatchReservation, u64), Box<dyn std::error::Error>> {
    if decision == [0; 16] {
        return Err("Invalid admission decision".into());
    }
    let sent = std::time::Instant::now();
    let candidate = get_pending_admission(config, instance, handle, period).await?;
    // Anchor remaining time to request dispatch, conservatively including the
    // whole round trip. A later retry never manufactures a fresh window.
    let lifetime = std::time::Duration::from_millis(candidate.remaining_ms);
    if sent.elapsed() >= lifetime {
        return Err("Admission candidate expired".into());
    }
    let reservation = ledger.reserve_batch(&candidate.registration(), decision)?;
    // A slow durable commit may cross the deadline. Preserve its decision but
    // do not attempt to admit expired work. Reconciliation retains those IDs.
    if sent.elapsed() >= lifetime {
        return Err("Admission candidate expired after reservation".into());
    }
    let receipt =
        super::send_admission_decision(config, &super::AdmissionDecision::from(&reservation))
            .await?;
    Ok((reservation, receipt))
}

/// Recover only a previously persisted decision. This sends no GET that could
/// be mistaken for a new admission window and creates no new ledger record.
/// Tor must resolve an exact retained replay before expiry checks; if the
/// original decision never arrived before expiry, it must reject this request.
pub async fn recover_admission_decision(
    config: &crate::types::RpcConfig,
    ledger: &crate::payment_ledger::PaymentLedger,
    instance: [u8; 16],
    handle: [u8; 16],
    period: u64,
) -> Result<u64, Box<dyn std::error::Error>> {
    let reservation = ledger
        .get_batch_reservation(instance, handle, period)?
        .ok_or("No durable admission decision to recover")?;
    super::send_admission_decision(config, &super::AdmissionDecision::from(&reservation)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(hop: usize) -> (String, serde_json::Value) {
        let all: serde_json::Value =
            serde_json::from_str(include_str!("../../spec/vectors/payment_renewal_v1.json"))
                .unwrap();
        let f = all["fixtures"][hop].clone();
        let a0 = f["periods"][0]["parent_digest_hex"].as_str().unwrap();
        let ids = f["initial_ids_hex"].as_str().unwrap();
        let terms = f["terms_hex"].as_str().unwrap();
        let rate = u64_at(&bytes::<55>(terms).unwrap(), 3);
        let mut rows = vec![
            format!(
                "250-ELTOR_ADMISSION 1 {} {} 1 0 {} 20000",
                hex::encode([1; 16]),
                hex::encode([2; 16]),
                hex::encode([3; 32])
            ),
            format!(
                "250-ELTOR_REGISTRATION 1 {} {} 1 0",
                hex::encode([1; 16]),
                hex::encode([2; 16])
            ),
            format!("250-POLICY {terms} {a0} {a0} 1"),
            format!("250-BINDING {}", f["C_hex"].as_str().unwrap()),
            "250-STATE ADMISSION_PENDING 0 0 20000 0".into(),
            format!("250-FEE 0 {} - NONE", hex::encode([0; 32])),
        ];
        for i in 0..10 {
            rows.push(format!(
                "250-ROUND {} {} {rate} - - - - NONE",
                i + 1,
                &ids[i * 64..(i + 1) * 64]
            ));
        }
        rows.push("250 OK".into());
        (rows.join("\n"), f)
    }
    #[test]
    fn public_vectors_resolve_to_unpayable_ledger_reservations() {
        for hop in 0..2 {
            let (wire, f) = fixture(hop);
            let parsed = PendingAdmission::parse(&wire, [1; 16], [2; 16], 0).unwrap();
            assert_eq!(
                hex::encode(parsed.policy_hash()),
                f["terms_digest_hex"].as_str().unwrap()
            );
            let batch = parsed.registration();
            assert!(batch
                .rounds
                .iter()
                .all(|r| r.window.is_none() && r.expected_payment_hash.is_none()));
            assert_eq!(batch.rounds[9].round, 10);
            assert_eq!(
                batch.rounds[0].circuit_binding,
                bytes::<32>(f["C_hex"].as_str().unwrap()).unwrap()
            );
        }
    }
    #[test]
    fn reject_incomplete_changed_or_payable_candidates() {
        let (wire, _) = fixture(0);
        let lines: Vec<_> = wire.split('\n').collect();
        for i in 0..lines.len() {
            let mut altered = lines.clone();
            altered.remove(i);
            assert!(PendingAdmission::parse(&altered.join("\n"), [1; 16], [2; 16], 0).is_err());
        }
        for (row, token, replacement) in [
            (0, 4, "01"),
            (0, 7, "0"),
            (1, 4, "2"),
            (3, 1, "00"),
            (4, 1, "ACTIVE"),
            (6, 1, "2"),
            (6, 3, "1"),
            (6, 4, "0"),
            (6, 8, "AUTHORIZED"),
            (16, 1, "OK extra"),
        ] {
            let mut altered: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            let mut fields: Vec<&str> = lines[row].split(' ').collect();
            fields[token] = replacement;
            altered[row] = fields.join(" ");
            assert!(
                PendingAdmission::parse(&altered.join("\n"), [1; 16], [2; 16], 0).is_err(),
                "row {} token {}",
                row,
                token
            );
        }
        assert!(PendingAdmission::parse(&wire, [9; 16], [2; 16], 0).is_err());
        assert!(PendingAdmission::parse(&wire, [1; 16], [2; 16], 1).is_err());
        let mut altered: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let duplicate_id = lines[6].split(' ').nth(2).unwrap();
        let mut fields: Vec<&str> = lines[7].split(' ').collect();
        fields[2] = duplicate_id;
        altered[7] = fields.join(" ");
        assert!(PendingAdmission::parse(&altered.join("\n"), [1; 16], [2; 16], 0).is_err());
    }
    #[tokio::test]
    async fn reservation_is_durable_before_notification_and_survives_lost_receipt() {
        use crate::payment_ledger::{AccountId, PaymentLedger};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        for lose_receipt in [false, true] {
            let directory = std::env::temp_dir()
                .join(format!("eltor-admission-{:032x}", rand::random::<u128>()));
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("ledger.sqlite");
            let account = AccountId([7; 32]);
            let mut ledger = PaymentLedger::provision(&path, account).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let config = crate::types::RpcConfig {
                addr: listener.local_addr().unwrap().to_string(),
                rpc_password: None,
                command: String::new(),
            };
            let server_path = path.clone();
            let (wire, _) = fixture(0);
            let server = tokio::spawn(async move {
                for step in 0..if lose_receipt { 3 } else { 2 } {
                    let (stream, _) = listener.accept().await.unwrap();
                    let (read, mut write) = stream.into_split();
                    let mut lines = BufReader::new(read).lines();
                    assert_eq!(lines.next_line().await.unwrap().unwrap(), "AUTHENTICATE");
                    let command = lines.next_line().await.unwrap().unwrap();
                    assert_eq!(lines.next_line().await.unwrap().unwrap(), "QUIT");
                    if step == 0 {
                        assert!(command.starts_with("ELTOR_PAYMENT_ADMISSION_GET 1 "));
                        write
                            .write_all(
                                format!("250 OK\r\n{}\r\n", wire.replace('\n', "\r\n")).as_bytes(),
                            )
                            .await
                            .unwrap();
                    } else {
                        assert!(command.starts_with("ELTOR_PAYMENT_ADMISSION_DECIDE 1 "));
                        // Reopen independently at the exact externally observable
                        // dispatch boundary: all IDs and decision must be durable.
                        let persisted = {
                            let check = PaymentLedger::reopen(&server_path, account).unwrap();
                            check
                                .get_batch_reservation([1; 16], [2; 16], 0)
                                .unwrap()
                                .unwrap()
                        };
                        assert_eq!(persisted.decision, [4; 16]);
                        assert_eq!(
                            command,
                            super::super::AdmissionDecision::from(&persisted)
                                .command()
                                .unwrap()
                        );
                        if !lose_receipt || step == 2 {
                            write
                                .write_all(
                                    format!(
                                        "250 OK\r\n250 ELTOR_ADMISSION_DECIDED 1 {} {} 0 {} 2\r\n",
                                        hex::encode([1; 16]),
                                        hex::encode([2; 16]),
                                        hex::encode([4; 16])
                                    )
                                    .as_bytes(),
                                )
                                .await
                                .unwrap();
                        }
                    }
                }
            });
            let result =
                reserve_pending_admission(&config, &mut ledger, [1; 16], [2; 16], 0, [4; 16]).await;
            assert_eq!(result.is_ok(), !lose_receipt);
            if lose_receipt {
                assert_eq!(
                    recover_admission_decision(&config, &ledger, [1; 16], [2; 16], 0)
                        .await
                        .unwrap(),
                    2
                );
            }
            server.await.unwrap();
            assert!(
                recover_admission_decision(&config, &ledger, [1; 16], [9; 16], 0)
                    .await
                    .is_err()
            );
            let recovered = ledger
                .get_batch_reservation([1; 16], [2; 16], 0)
                .unwrap()
                .unwrap();
            assert_eq!(recovered.decision, [4; 16]);
            let (wire, _) = fixture(0);
            let batch = PendingAdmission::parse(&wire, [1; 16], [2; 16], 0)
                .unwrap()
                .registration();
            for obligation in &batch.rounds {
                assert!(ledger.get_authorization(obligation).unwrap().is_none());
            }
            drop(ledger);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
}
