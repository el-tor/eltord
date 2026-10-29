//! Foundation capability extraction from a descriptor obtained through Tor's
//! authenticated local control connection. Tor verifies the signed descriptor;
//! this parser does not turn arbitrary caller-supplied text into trusted data.
use std::io;

#[derive(Clone, PartialEq, Eq)]
pub struct FoundationDescriptor {
    pub fingerprint: [u8; 20],
    pub terms: [u8; 55],
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid foundation descriptor")
}
impl FoundationDescriptor {
    /// Require a single descriptor and the complete shared profile. The caller
    /// must compare these terms with constructor registration before COMMIT.
    pub fn parse(raw: &str, expected: [u8; 20]) -> io::Result<Self> {
        if raw.len() > 65536 || expected == [0; 20] {
            return Err(invalid());
        }
        let mut fingerprint = None;
        let mut start = None;
        let mut renew = None;
        let mut free = false;
        let mut routers = 0;
        for line in raw.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            match fields.first().copied() {
                Some("router") => {
                    routers += 1;
                    if routers > 1 {
                        return Err(invalid());
                    }
                }
                Some("fingerprint") => {
                    if fingerprint.is_some() {
                        return Err(invalid());
                    }
                    let value = fields[1..].join("");
                    let mut bytes = [0; 20];
                    hex::decode_to_slice(value, &mut bytes).map_err(|_| invalid())?;
                    fingerprint = Some(bytes);
                }
                Some("eltor-payment-start") => {
                    if start.is_some()
                        || fields.len() != 7
                        || fields[1..6] != ["1", "226", "227", "228", "229"]
                    {
                        return Err(invalid());
                    }
                    start = Some(terms(fields[6])?);
                }
                Some("eltor-payment-renew") => {
                    if renew.is_some() || fields.len() != 5 || fields[1..4] != ["1", "224", "225"] {
                        return Err(invalid());
                    }
                    renew = Some(terms(fields[4])?);
                }
                Some("eltor-free-capacity") => {
                    if free || fields != ["eltor-free-capacity", "1", "2", "128", "600000"] {
                        return Err(invalid());
                    }
                    free = true;
                }
                _ => (),
            }
        }
        if routers != 1
            || fingerprint != Some(expected)
            || !free
            || start.is_none()
            || start != renew
        {
            return Err(invalid());
        }
        Ok(Self {
            fingerprint: expected,
            terms: start.unwrap(),
        })
    }
}
/// Read one Tor-verified descriptor, never an unbounded all-relay response.
/// An authentication or framing failure returns no candidate for construction.
pub async fn foundation_descriptor(
    config: &crate::types::RpcConfig,
    fingerprint: [u8; 20],
) -> io::Result<FoundationDescriptor> {
    if fingerprint == [0; 20] {
        return Err(invalid());
    }
    let mut config = config.clone();
    // The legacy one-shot transport wraps passwords in quotes. Escape literal
    // characters and reject line controls before using that transport.
    if let Some(password) = config.rpc_password.as_mut() {
        if password.bytes().any(|b| b < 32 || b == 127) {
            return Err(invalid());
        }
        *password = password.replace('\\', "\\\\").replace('"', "\\\"");
    }
    config.command = format!("GETINFO desc/id/{}", hex::encode(fingerprint));
    let reply = super::rpc_client_limited(config, 65536)
        .await
        .map_err(|_| invalid())?;
    descriptor_reply(&reply, fingerprint)
}
fn descriptor_reply(reply: &str, fingerprint: [u8; 20]) -> io::Result<FoundationDescriptor> {
    let response = super::command_response(reply).map_err(|_| invalid())?;
    let mut lines = response.lines();
    let expected = format!("250+desc/id/{}=", hex::encode(fingerprint));
    if lines.next() != Some(expected.as_str()) {
        return Err(invalid());
    }
    let mut raw = String::new();
    let mut terminated = false;
    for line in lines.by_ref() {
        if line == "." {
            terminated = true;
            break;
        }
        // Decode the control protocol's dot-stuffed data, never interpret a
        // status-looking descriptor line as command success.
        if line.starts_with('.') && !line.starts_with("..") {
            return Err(invalid());
        }
        raw.push_str(line.strip_prefix("..").map(|_| &line[1..]).unwrap_or(line));
        raw.push('\n');
    }
    if !terminated || lines.next() != Some("250 OK") || lines.next().is_some() {
        return Err(invalid());
    }
    FoundationDescriptor::parse(&raw, fingerprint)
}

fn terms(value: &str) -> io::Result<[u8; 55]> {
    if value.len() != 110
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let mut out = [0; 55];
    hex::decode_to_slice(value, &mut out).map_err(|_| invalid())?;
    // Full semantic validation is performed by Tor and compared against the
    // constructor's validated policy; reject unsupported profile identifiers here.
    if out[..3] != [1, 1, 10] {
        return Err(invalid());
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_profile_selection_requires_complete_matching_capabilities() {
        let mut terms = [0; 55];
        terms[..3].copy_from_slice(&[1, 1, 10]);
        let terms = hex::encode(terms);
        let raw = format!("router fixture 127.0.0.1 1 0 0\nfingerprint {}\neltor-payment-start 1 226 227 228 229 {}\neltor-payment-renew 1 224 225 {}\neltor-free-capacity 1 2 128 600000\n", hex::encode([1;20]), terms, terms);
        assert!(FoundationDescriptor::parse(&raw, [1; 20]).is_ok());
        let response = format!(
            "250 OK\r\n250+desc/id/{}=\r\n{}.\r\n250 OK\r\n221 closing\r\n",
            hex::encode([1; 20]),
            raw.replace('\n', "\r\n")
        );
        assert!(descriptor_reply(&response, [1; 20]).is_ok());
        for malformed in [
            response.replacen("250 OK", "515 Authentication failed", 1),
            response.replace(".\r\n250 OK", "250 OK"),
            response.replace("250+desc/id/", "250+desc/name/"),
            response.replace(".\r\n250 OK", ".\r\n552 missing"),
        ] {
            assert!(descriptor_reply(&malformed, [1; 20]).is_err());
        }

        assert!(FoundationDescriptor::parse(&raw, [2; 20]).is_err());
        for bad in [
            raw.replace("1 224 225", "2 224 225"),
            raw.replace("1 2 128 600000", "1 2 128 600001"),
            raw.replace("eltor-payment-renew", "unknown-renew"),
            format!("{}{}", raw, raw),
            format!("{}eltor-free-capacity 1 2 128 600000\n", raw),
        ] {
            assert!(FoundationDescriptor::parse(&bad, [1; 20]).is_err());
        }
    }
}
