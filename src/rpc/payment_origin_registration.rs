//! Exact local constructor metadata. A successful read is neither COMMIT nor
//! permission to pay. The coordinator must accept pinned policy/budget terms.
use super::{OriginBinding, PaymentControl};
use sha2::{Digest, Sha256};
use std::io;
fn bad() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid origin registration")
}
fn number(s: &str) -> io::Result<u64> {
    let n: u64 = s.parse().map_err(|_| bad())?;
    if n.to_string() != s {
        return Err(bad());
    }
    Ok(n)
}
fn bytes<const N: usize>(s: &str) -> io::Result<[u8; N]> {
    if s.len() != N * 2
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(bad());
    }
    let mut v = [0; N];
    hex::decode_to_slice(s, &mut v).map_err(|_| bad())?;
    Ok(v)
}
fn digest(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update((domain.len() as u16).to_be_bytes());
    h.update(domain);
    for p in parts {
        h.update(p)
    }
    h.finalize().into()
}
#[derive(Clone, PartialEq, Eq)]
pub struct OriginRegistration {
    pub binding: OriginBinding,
    pub hop: u16,
    pub revision: u64,
    terms: [u8; 55],
    circuit_binding: [u8; 32],
    policy_hash: [u8; 32],
    initial_allocation: [u8; 32],
    ids: [[u8; 32]; 10],
}
impl OriginRegistration {
    pub fn terms(&self) -> [u8; 55] {
        self.terms
    }
    pub fn circuit_binding(&self) -> [u8; 32] {
        self.circuit_binding
    }
    pub fn policy_hash(&self) -> [u8; 32] {
        self.policy_hash
    }
    pub fn initial_allocation(&self) -> [u8; 32] {
        self.initial_allocation
    }
    pub fn payment_ids(&self) -> &[[u8; 32]; 10] {
        &self.ids
    }
    pub fn parse(
        reply: &str,
        binding: &OriginBinding,
        hop: u16,
        expected_ids: &[[u8; 32]; 10],
    ) -> io::Result<Self> {
        if reply.len() > 2048
            || binding.instance == [0; 16]
            || binding.handle == [0; 16]
            || binding.revision == 0
            || hop == 0
            || hop > binding.hops
        {
            return Err(bad());
        }
        let rows: Vec<Vec<&str>> = reply.lines().map(|s| s.split(' ').collect()).collect();
        if rows.len() != 14 {
            return Err(bad());
        }
        let h = &rows[0];
        if h.len() != 6
            || h[..2] != ["250-ELTOR_INITIAL_REGISTRATION", "1"]
            || bytes::<16>(h[2])? != binding.instance
            || bytes::<16>(h[3])? != binding.handle
            || number(h[4])? != u64::from(hop)
        {
            return Err(bad());
        }
        let revision = number(h[5])?;
        if revision == 0 {
            return Err(bad());
        }
        let p = &rows[1];
        if p.len() != 4 || p[0] != "250-POLICY" {
            return Err(bad());
        }
        let terms = bytes::<55>(p[1])?;
        super::FoundationPolicy::decode(&terms)?;
        let q = bytes::<32>(p[2])?;
        let a0 = bytes::<32>(p[3])?;
        let b = &rows[2];
        if b.len() != 2 || b[0] != "250-BINDING" {
            return Err(bad());
        }
        let c = bytes::<32>(b[1])?;
        if c == [0; 32] {
            return Err(bad());
        }
        let mut ids = [[0; 32]; 10];
        for i in 0..10 {
            let r = &rows[3 + i];
            if r.len() != 3 || r[0] != "250-ROUND" || number(r[1])? != i as u64 + 1 {
                return Err(bad());
            }
            ids[i] = bytes::<32>(r[2])?;
            if ids[i] == [0; 32] || ids[..i].contains(&ids[i]) || ids[i] != expected_ids[i] {
                return Err(bad());
            }
        }
        if rows[13] != ["250", "OK"] {
            return Err(bad());
        }
        let flat: Vec<u8> = ids.iter().flatten().copied().collect();
        if q != digest(b"eltor/payment-renew/v1/terms", &[&terms])
            || a0 != digest(b"eltor/payment-renew/v1/initial", &[&c, &q, &flat])
        {
            return Err(bad());
        }
        Ok(Self {
            binding: binding.clone(),
            hop,
            revision,
            terms,
            circuit_binding: c,
            policy_hash: q,
            initial_allocation: a0,
            ids,
        })
    }
}

impl PaymentControl {
    pub async fn origin_registration(
        &mut self,
        binding: &OriginBinding,
        hop: u16,
        expected_ids: &[[u8; 32]; 10],
    ) -> io::Result<OriginRegistration> {
        if hop == 0
            || hop > binding.hops
            || binding.instance == [0; 16]
            || binding.handle == [0; 16]
            || binding.revision == 0
        {
            return Err(bad());
        }
        let reply = self
            .command(&format!(
                "ELTOR_INITIAL_REGISTRATION_GET 1 {} {} {}",
                hex::encode(binding.instance),
                hex::encode(binding.handle),
                hop
            ))
            .await?;
        OriginRegistration::parse(&reply, binding, hop, expected_ids).map_err(|error| {
            self.invalidate();
            error
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (OriginBinding, [[u8; 32]; 10], String) {
        let b = OriginBinding {
            instance: [1; 16],
            handle: [2; 16],
            hops: 3,
            revision: 1,
        };
        let ids = std::array::from_fn(|i| [i as u8 + 1; 32]);
        let mut t = [0; 55];
        t[..3].copy_from_slice(&[1, 1, 10]);
        t[11..15].copy_from_slice(&60000u32.to_be_bytes());
        t[15..19].copy_from_slice(&60000u32.to_be_bytes());
        t[19..23].copy_from_slice(&15000u32.to_be_bytes());
        t[31..39].copy_from_slice(&1800000u64.to_be_bytes());
        t[39..43].copy_from_slice(&3u32.to_be_bytes());
        t[51..55].copy_from_slice(&30000u32.to_be_bytes());
        let c = [3; 32];
        let q = digest(b"eltor/payment-renew/v1/terms", &[&t]);
        let flat: Vec<u8> = ids.iter().flatten().copied().collect();
        let a = digest(b"eltor/payment-renew/v1/initial", &[&c, &q, &flat]);
        let mut reply = format!(
            "250-ELTOR_INITIAL_REGISTRATION 1 {} {} 2 4\n250-POLICY {} {} {}\n250-BINDING {}\n",
            hex::encode(b.instance),
            hex::encode(b.handle),
            hex::encode(t),
            hex::encode(q),
            hex::encode(a),
            hex::encode(c)
        );
        for (i, id) in ids.iter().enumerate() {
            reply += &format!("250-ROUND {} {}\n", i + 1, hex::encode(id));
        }
        reply += "250 OK";
        (b, ids, reply)
    }
    #[test]
    fn constructor_registration_requires_exact_owned_ids_and_digest_binding() {
        let (binding, ids, reply) = fixture();
        let p = OriginRegistration::parse(&reply, &binding, 2, &ids).unwrap();
        assert_eq!(p.circuit_binding(), [3; 32]);
        assert!(p.payment_ids() == &ids);
        let mut wrong = ids;
        wrong[0] = [99; 32];
        assert!(OriginRegistration::parse(&reply, &binding, 2, &wrong).is_err());
        for bad in [
            reply.replace("250-ROUND 1 ", "250-ROUND 01 "),
            reply.replace("250-BINDING ", "250-OTHER "),
            reply.replace(&hex::encode([3; 32]), &hex::encode([9; 32])),
            reply.replace("250-ROUND 10 ", "250-ROUND 9 "),
            format!("{}\n250 OK", reply),
        ] {
            assert!(OriginRegistration::parse(&bad, &binding, 2, &ids).is_err());
        }
        assert!(OriginRegistration::parse(&reply, &binding, 1, &ids).is_err());
    }
    #[tokio::test]
    async fn registration_socket_reads_only_and_poisoned_metadata_cannot_be_reused() {
        use tokio::{
            io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
            net::TcpListener,
        };
        let (binding, ids, reply) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = crate::types::RpcConfig {
            addr: listener.local_addr().unwrap().to_string(),
            rpc_password: None,
            command: String::new(),
        };
        let command = format!(
            "ELTOR_INITIAL_REGISTRATION_GET 1 {} {} 2\r\n",
            hex::encode(binding.instance),
            hex::encode(binding.handle)
        );
        let server = tokio::spawn(async move {
            let mut socket = BufReader::new(listener.accept().await.unwrap().0);
            for expected in ["AUTHENTICATE\r\n", "SETEVENTS ELTOR_PAYMENT_STATE\r\n"] {
                let mut line = String::new();
                socket.read_line(&mut line).await.unwrap();
                assert_eq!(line, expected);
                socket.get_mut().write_all(b"250 OK\r\n").await.unwrap();
            }
            // Exact valid metadata is read without an activation or payment command.
            // A substituted constructor binding on the next read poisons the client.
            for response in [
                &reply,
                &reply.replace(&hex::encode([3; 32]), &hex::encode([9; 32])),
            ] {
                let mut line = String::new();
                socket.read_line(&mut line).await.unwrap();
                assert_eq!(line, command);
                socket
                    .get_mut()
                    .write_all((response.replace('\n', "\r\n") + "\r\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        let mut control = PaymentControl::connect(&config).await.unwrap();
        let record = control
            .origin_registration(&binding, 2, &ids)
            .await
            .unwrap();
        assert_eq!(record.circuit_binding(), [3; 32]);
        assert!(control
            .origin_registration(&binding, 2, &ids)
            .await
            .is_err());
        assert!(control
            .origin_registration(&binding, 2, &ids)
            .await
            .is_err());
        server.await.unwrap();
    }
}
