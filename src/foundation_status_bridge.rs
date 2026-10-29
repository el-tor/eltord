//! Private parent-owned sidecar observations. This transports no authority and
//! never queries Tor, a wallet or financial storage. Unsupported clocks fail shut.
use crate::FoundationClientStatus;
use serde::{Deserialize, Serialize};
use std::{
    convert::TryFrom,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
};
use tokio::time::Instant;

pub const STATUS_DIRECTORY_ENV: &str = "ELTOR_PRIVATE_STATUS_DIRECTORY";
pub const STATUS_GENERATION_ENV: &str = "ELTOR_PRIVATE_STATUS_GENERATION";
const LIMIT: usize = 512;
const RECORD: &str = "observation.json";
const NEXT: &str = "next";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    pid: u32,
    generation: String,
    phase: FoundationClientStatus,
    expires_mono_ms: Option<u64>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid private status observation",
    )
}
fn generation_valid(generation: &str) -> bool {
    generation.len() == 32
        && generation
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[cfg(unix)]
fn private(metadata: &fs::Metadata, directory: bool) -> io::Result<()> {
    private_for_uid(metadata, directory, unsafe { libc::geteuid() })
}
#[cfg(unix)]
fn private_for_uid(metadata: &fs::Metadata, directory: bool, expected_uid: u32) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if metadata.uid() != expected_uid
        || metadata.permissions().mode() & 0o777 != if directory { 0o700 } else { 0o600 }
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file() || metadata.nlink() != 1
        }
    {
        return Err(invalid());
    }
    Ok(())
}
#[cfg(not(unix))]
fn private(_: &fs::Metadata, _: bool) -> io::Result<()> {
    Err(invalid())
}
#[cfg(target_os = "linux")]
fn monotonic_ms() -> io::Result<u64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0
        || time.tv_sec < 0
        || !(0..1_000_000_000).contains(&time.tv_nsec)
    {
        return Err(invalid());
    }
    (time.tv_sec as u64)
        .checked_mul(1000)
        .and_then(|n| n.checked_add(time.tv_nsec as u64 / 1_000_000))
        .ok_or_else(invalid)
}
#[cfg(not(target_os = "linux"))]
fn monotonic_ms() -> io::Result<u64> {
    Err(invalid())
}
fn checked_directory(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    private(&metadata, true)?;
    Ok(metadata)
}
#[cfg(unix)]
fn identity(metadata: &fs::Metadata) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Ok((metadata.dev(), metadata.ino()))
}
#[cfg(not(unix))]
fn identity(_: &fs::Metadata) -> io::Result<(u64, u64)> {
    Err(invalid())
}
fn read_record(path: &Path) -> io::Result<Record> {
    checked_directory(path)?;
    read_private_record(&path.join(RECORD))
}
fn read_private_record(path: &Path) -> io::Result<Record> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    private(&metadata, false)?;
    if metadata.len() > LIMIT as u64 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err(invalid());
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

/// One bounded artifact per owned launch. Keep this object private in the parent
/// alongside its actual Child handle; never reconstruct it from a PID file.
pub struct FoundationStatusBridge {
    directory: PathBuf,
    generation: String,
    directory_identity: (u64, u64),
}
impl FoundationStatusBridge {
    pub fn create(base: &Path) -> io::Result<Self> {
        monotonic_ms()?;
        let generation = hex::encode(rand::random::<[u8; 16]>());
        let directory = fs::canonicalize(base)?.join(format!(".foundation-status-{generation}"));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
        let directory_identity = identity(&checked_directory(&directory)?)?;
        Ok(Self {
            directory,
            generation,
            directory_identity,
        })
    }
    pub fn configure_child(&self, command: &mut Command) {
        command
            .env(STATUS_DIRECTORY_ENV, &self.directory)
            .env(STATUS_GENERATION_ENV, &self.generation);
    }
    /// The parent must first establish that its exact owned Child is still
    /// running. Every failed/unknown observation is recovering, never ACTIVE.
    pub fn read(&self, expected_pid: u32) -> FoundationClientStatus {
        self.read_checked(expected_pid)
            .unwrap_or(FoundationClientStatus::Recovering)
    }
    fn read_checked(&self, expected_pid: u32) -> io::Result<FoundationClientStatus> {
        if identity(&checked_directory(&self.directory)?)? != self.directory_identity {
            return Err(invalid());
        }
        let record = read_record(&self.directory)?;
        if record.version != 1
            || expected_pid == 0
            || record.pid != expected_pid
            || record.generation != self.generation
            || !generation_valid(&record.generation)
            || record.phase == FoundationClientStatus::NotConfigured
        {
            return Err(invalid());
        }
        let now = monotonic_ms()?;
        match (record.phase, record.expires_mono_ms) {
            (FoundationClientStatus::LastVerifiedActive, Some(expires))
                if now < expires && expires.saturating_sub(now) <= 60_000 =>
            {
                Ok(record.phase)
            }
            (FoundationClientStatus::LastVerifiedActive, _) | (_, Some(_)) => Err(invalid()),
            (_, None) => Ok(record.phase),
        }
    }
}
impl Drop for FoundationStatusBridge {
    fn drop(&mut self) {
        // Never recurse or remove an unknown directory tree. Errors merely
        // leave this one private artifact for explicit local cleanup.
        if checked_directory(&self.directory)
            .and_then(|m| identity(&m))
            .ok()
            != Some(self.directory_identity)
        {
            return;
        }
        for name in [RECORD, NEXT] {
            let path = self.directory.join(name);
            let owned = (|| -> io::Result<bool> {
                let record = read_private_record(&path)?;
                Ok(record.generation == self.generation)
            })()
            .unwrap_or(false);
            if owned {
                let _ = fs::remove_file(path);
            }
        }
        let _ = fs::remove_dir(&self.directory);
    }
}
fn write_record(directory: &Path, record: &Record) -> io::Result<()> {
    checked_directory(directory)?;
    let bytes = serde_json::to_vec(record).map_err(|_| invalid())?;
    if bytes.len() > LIMIT {
        return Err(invalid());
    }
    let next = directory.join(NEXT);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(&next)?;
    let temporary_identity = identity(&file.metadata()?)?;
    let result = (|| {
        file.write_all(&bytes)?;
        fs::rename(&next, directory.join(RECORD))
    })();
    if result.is_err() {
        if fs::symlink_metadata(&next).and_then(|m| identity(&m)).ok() == Some(temporary_identity) {
            let _ = fs::remove_file(next);
        }
    }
    result
}
fn active_expiry(expires: Instant) -> io::Result<u64> {
    // OS time is sampled first; the result cannot extend the original expiry.
    let before = monotonic_ms()?;
    let now = Instant::now();
    before
        .checked_add(
            u64::try_from(expires.saturating_duration_since(now).as_millis())
                .map_err(|_| invalid())?,
        )
        .ok_or_else(invalid)
}
pub(crate) fn publish(phase: FoundationClientStatus, expires: Option<Instant>) {
    // Best effort only: a missing/private/full/invalid artifact must never stop
    // or change the financial controller. The parent will show recovering.
    let _ = (|| -> io::Result<()> {
        let directory = std::env::var_os(STATUS_DIRECTORY_ENV).ok_or_else(invalid)?;
        let generation = std::env::var(STATUS_GENERATION_ENV).map_err(|_| invalid())?;
        if !generation_valid(&generation) {
            return Err(invalid());
        }
        // Sample OS time before Tokio time. Flooring the remaining duration
        // gives a conservative absolute deadline, even on cached publication.
        let expires_mono_ms = if phase == FoundationClientStatus::LastVerifiedActive {
            Some(active_expiry(expires.ok_or_else(invalid)?)?)
        } else {
            None
        };
        let directory = Path::new(&directory);
        let result = write_record(
            directory,
            &Record {
                version: 1,
                pid: std::process::id(),
                generation,
                phase,
                expires_mono_ms,
            },
        );
        result
    })();
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn bridge_rejects_wrong_owner_stale_generation_and_expired_original_lease() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir();
        let bridge = FoundationStatusBridge::create(&base).unwrap();
        assert!(
            active_expiry(Instant::now() - std::time::Duration::from_millis(1)).unwrap()
                <= monotonic_ms().unwrap()
        );
        let mut record = Record {
            version: 1,
            pid: 12345,
            generation: bridge.generation.clone(),
            phase: FoundationClientStatus::LastVerifiedActive,
            expires_mono_ms: Some(monotonic_ms().unwrap() + 1000),
        };
        write_record(&bridge.directory, &record).unwrap();
        assert_eq!(
            bridge.read(12345),
            FoundationClientStatus::LastVerifiedActive
        );
        assert_eq!(bridge.read(12346), FoundationClientStatus::Recovering);
        assert!(private_for_uid(
            &fs::metadata(bridge.directory.join(RECORD)).unwrap(),
            false,
            unsafe { libc::geteuid() }.wrapping_add(1)
        )
        .is_err());
        record.generation = "0".repeat(32);
        write_record(&bridge.directory, &record).unwrap();
        assert_eq!(bridge.read(12345), FoundationClientStatus::Recovering);
        fs::write(bridge.directory.join(NEXT), b"preexisting fixture").unwrap();
        assert!(write_record(&bridge.directory, &record).is_err());
        assert_eq!(
            fs::read(bridge.directory.join(NEXT)).unwrap(),
            b"preexisting fixture"
        );
        fs::remove_file(bridge.directory.join(NEXT)).unwrap();
        record.generation = bridge.generation.clone();
        record.expires_mono_ms = Some(monotonic_ms().unwrap().saturating_sub(1));
        write_record(&bridge.directory, &record).unwrap();
        assert_eq!(bridge.read(12345), FoundationClientStatus::Recovering);
        record.expires_mono_ms = Some(monotonic_ms().unwrap() + 1000);
        write_record(&bridge.directory, &record).unwrap();
        fs::set_permissions(
            bridge.directory.join(RECORD),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert_eq!(bridge.read(12345), FoundationClientStatus::Recovering);
        fs::set_permissions(
            bridge.directory.join(RECORD),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        fs::remove_file(bridge.directory.join(RECORD)).unwrap();
        std::os::unix::fs::symlink("/dev/null", bridge.directory.join(RECORD)).unwrap();
        assert_eq!(bridge.read(12345), FoundationClientStatus::Recovering);
        fs::remove_file(bridge.directory.join(RECORD)).unwrap();
    }
}
