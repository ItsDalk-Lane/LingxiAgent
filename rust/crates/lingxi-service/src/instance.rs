//! Instance identity, single-writer lock and instance-record lifecycle
//! (R02-T02 steps 3–4).
//!
//! ## Lock mechanism (ADR-free local decision, recorded in the task report)
//!
//! The single-writer lock is an OS advisory lock on
//! `{home}/lingxi-service/instance.lock`, taken with `File::try_lock`
//! (flock(2) on unix, LockFileEx on Windows — both in the standard library
//! since Rust 1.89; the workspace pins 1.98.1). Chosen over alternatives
//! because:
//! - **zero new dependencies** (the task guidance prefers std-only locks);
//! - the lock is owned by the *open file description*, so it disappears
//!   exactly when the owning process dies — SIGKILL included. Stale-lock
//!   files are structurally impossible, unlike O_EXCL lock files.
//!
//! ## Stale-record detection (never "does this PID exist")
//!
//! The lock is the ONLY liveness authority. The instance record
//! (`instance.json`) is diagnostics + identity, never a liveness oracle:
//! - lock acquirable + record present => the record is STALE (its owner
//!   died without cleanup); the new instance archives it to
//!   `instance.stale.json`, logs the takeover, and becomes the owner;
//! - lock NOT acquirable => a live peer holds the home: reject with a
//!   full diagnostic. The recorded PID is *reported* for humans but is
//!   never consulted for the decision — PID reuse can neither cause a
//!   false takeover nor a false rejection.
//!
//! This mirrors the incumbent Node gate's philosophy (`server/index.ts`
//! same-home mutex: "不信任裸 PID，因为 PID 会被系统复用") with a
//! stronger primitive: the Node gate probes the recorded server over
//! token-authenticated HTTP to distinguish dead from live; here the kernel
//! itself answers liveness through the advisory lock, and an HTTP health
//! probe is kept only as a *diagnostic* enrichment of the rejection
//! message (authoritative for nothing).
//!
//! ## Version handshake
//!
//! The record snapshots the single version authorities from
//! lingxi-protocol (server kind/version, wire protocol range, data epoch).
//! On takeover of a stale record the handshake fields are compared and a
//! mismatch is logged (e.g. a newer server taking over an older owner's
//! home) — informational, never a blocker: the lock already decided
//! ownership, and ADR-004's epoch gate owns data-version policy, not this
//! module.

use std::fmt;
use std::fs::File;
use std::io::{self, Read as _};
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::paths::{atomic_write, DataRootLayout};

/// Record schema version (bump on incompatible record changes).
pub const INSTANCE_RECORD_SCHEMA_VERSION: u32 = 1;

/// Per-start identity of a service instance. `instance_id` identifies this
/// particular claim of the home; `start_nonce` is a second independent
/// random value so that (instance_id, start_nonce) pairs can never collide
/// across starts even under a degraded entropy fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceIdentity {
    pub instance_id: String,
    pub start_nonce: String,
    pub pid: u32,
    pub started_at_unix_ms: u64,
}

impl InstanceIdentity {
    /// Generates a fresh identity. Entropy: 16 bytes (`instance_id`) + 8
    /// bytes (`start_nonce`) read from the OS random device on unix; if that
    /// read fails, a documented fallback mixes pid + nanosecond time (still
    /// unique per start in practice, weaker against adversarial
    /// prediction — logged by the caller via [`entropy_source`]).
    pub fn generate() -> Self {
        let instance_id = hex_string(16);
        let start_nonce = hex_string(8);
        InstanceIdentity {
            instance_id,
            start_nonce,
            pid: std::process::id(),
            started_at_unix_ms: unix_ms(),
        }
    }
}

/// Which entropy source the last [`InstanceIdentity::generate`] used
/// (diagnostic for the safe log).
pub fn entropy_source() -> &'static str {
    if OS_RANDOM_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed) {
        "os-random-device"
    } else {
        "pid-time-fallback"
    }
}

static OS_RANDOM_AVAILABLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

fn hex_string(n_bytes: usize) -> String {
    let mut bytes = vec![0u8; n_bytes];
    if !fill_random(&mut bytes) {
        OS_RANDOM_AVAILABLE.store(false, std::sync::atomic::Ordering::Relaxed);
        // Fallback: deterministic-but-unique mix. Distinct calls differ in
        // the nanosecond timestamp; the caller logs the degraded source.
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut state =
            (nanos as u64) ^ (u64::from(std::process::id()) << 32) ^ 0x9e37_79b9_7f4a_7c15;
        for slot in &mut bytes {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *slot = (state >> 24) as u8;
        }
    }
    let mut out = String::with_capacity(n_bytes * 2);
    {
        use std::fmt::Write as _;
        for b in bytes {
            let _ = write!(out, "{b:02x}");
        }
    }
    out
}

#[cfg(unix)]
fn fill_random(buf: &mut [u8]) -> bool {
    match File::open("/dev/urandom") {
        Ok(mut f) => f.read_exact(buf).is_ok(),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn fill_random(_buf: &mut [u8]) -> bool {
    // No OS random device path on non-unix; the fallback mix is used and
    // reported through `entropy_source` (platform not verified here).
    false
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// On-disk instance record (atomically written, machine-readable
/// diagnostics). Field names are camelCase like every JSON surface here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRecord {
    pub schema_version: u32,
    pub server_kind: String,
    pub server_version: String,
    pub wire_protocol_min: u32,
    pub wire_protocol_max: u32,
    pub data_epoch: u32,
    pub instance_id: String,
    pub start_nonce: String,
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_addr: Option<String>,
    pub started_at_unix_ms: u64,
    pub home_path: String,
}

impl InstanceRecord {
    /// Builds the record for THIS instance at publish time (after the
    /// listener is bound, so the address is known).
    pub fn for_identity(identity: &InstanceIdentity, bind_addr: SocketAddr, home: &Path) -> Self {
        InstanceRecord {
            schema_version: INSTANCE_RECORD_SCHEMA_VERSION,
            server_kind: crate::SERVER_KIND.to_string(),
            server_version: crate::server_version().to_string(),
            wire_protocol_min: lingxi_protocol::handshake::WIRE_PROTOCOL_MIN_SUPPORTED,
            wire_protocol_max: lingxi_protocol::handshake::WIRE_PROTOCOL_MAX_SUPPORTED,
            data_epoch: lingxi_protocol::ContractVersions::R00_BASELINE.data_epoch,
            instance_id: identity.instance_id.clone(),
            start_nonce: identity.start_nonce.clone(),
            pid: identity.pid,
            bind_addr: Some(bind_addr.to_string()),
            started_at_unix_ms: identity.started_at_unix_ms,
            home_path: home.display().to_string(),
        }
    }

    /// Version-handshake comparison against the current authorities:
    /// returns a human summary of every mismatch (empty = handshake agrees).
    pub fn handshake_mismatches(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.wire_protocol_min != lingxi_protocol::handshake::WIRE_PROTOCOL_MIN_SUPPORTED
            || self.wire_protocol_max != lingxi_protocol::handshake::WIRE_PROTOCOL_MAX_SUPPORTED
        {
            out.push(format!(
                "wire protocol {}..={} vs current {}..={}",
                self.wire_protocol_min,
                self.wire_protocol_max,
                lingxi_protocol::handshake::WIRE_PROTOCOL_MIN_SUPPORTED,
                lingxi_protocol::handshake::WIRE_PROTOCOL_MAX_SUPPORTED
            ));
        }
        if self.data_epoch != lingxi_protocol::ContractVersions::R00_BASELINE.data_epoch {
            out.push(format!(
                "data epoch {} vs current {} (epoch policy is owned by the ADR-004 gate)",
                self.data_epoch,
                lingxi_protocol::ContractVersions::R00_BASELINE.data_epoch
            ));
        }
        if self.server_version != crate::server_version() {
            out.push(format!(
                "server version {} vs current {}",
                self.server_version,
                crate::server_version()
            ));
        }
        out
    }
}

/// Reads a record if present and well-formed. A *malformed* record is not
/// an error at acquisition time: the lock still decides ownership, so a
/// corrupted record is surfaced as `Malformed` for diagnostics and the
/// takeover proceeds (never a silent swallow, never a blocker).
pub fn read_record(path: &Path) -> Option<std::io::Result<InstanceRecord>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
        Err(err) => return Some(Err(err)),
    };
    Some(serde_json::from_str(&raw).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("instance record is not valid JSON: {err}"),
        )
    }))
}

/// Outcome of a diagnostic liveness probe against the recorded peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerProbe {
    /// Peer answered /lingxi/v1/health with a lingxi-service payload.
    Live,
    /// Recorded peer could not be probed (no address recorded, connection
    /// refused/timeout, or non-lingxi response). Diagnostic only — the
    /// lock remains the authority that triggered rejection.
    Unreachable(String),
}

/// Minimal blocking HTTP/1.1 GET of the peer health endpoint (deliberately
/// std-only; 500ms connect + 500ms read budget). Never authoritative.
pub fn probe_peer(addr: &str) -> PeerProbe {
    let Ok(addr) = addr.parse::<SocketAddr>() else {
        return PeerProbe::Unreachable(format!(
            "recorded address {addr:?} is not a socket address"
        ));
    };
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(500))
    else {
        return PeerProbe::Unreachable(format!("connect to {addr} failed or timed out"));
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    use std::io::Write as _;
    let request =
        format!("GET /lingxi/v1/health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    if let Err(err) = stream.write_all(request.as_bytes()) {
        return PeerProbe::Unreachable(format!("probe write failed: {err}"));
    }
    let mut response = Vec::new();
    let mut buf = [0u8; 4096];
    // Read until EOF or budget exhaustion; a keep-alive peer that is slow
    // to close must not turn a perfectly good answer into "unreachable".
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(err) => {
                return PeerProbe::Unreachable(format!("probe read failed: {err}"));
            }
        }
        if response.len() > 64 * 1024 {
            return PeerProbe::Unreachable("probe response exceeds 64KiB budget".to_string());
        }
    }
    let text = String::from_utf8_lossy(&response);
    if !text.starts_with("HTTP/1.1 200") {
        return PeerProbe::Unreachable(format!(
            "peer answered with non-200 status: {}",
            text.split_once("\r\n")
                .map(|(head, _)| head)
                .unwrap_or(&text)
        ));
    }
    if text.contains(crate::SERVER_KIND) {
        PeerProbe::Live
    } else {
        PeerProbe::Unreachable("peer is not a lingxi-service".to_string())
    }
}

/// Full diagnostics of a live peer holding the single-writer lock
/// (record if readable, probe outcome). Boxed inside
/// [`InstanceLockError::HeldByPeer`] to keep the error small.
#[derive(Debug)]
pub struct HeldByPeerDiagnostic {
    pub home: std::path::PathBuf,
    pub record: Option<InstanceRecord>,
    pub record_error: Option<String>,
    pub probe: Option<PeerProbe>,
}

/// Errors of single-writer acquisition.
#[derive(Debug)]
pub enum InstanceLockError {
    /// The lock file could not be opened (permission/IO problems).
    Io {
        path: std::path::PathBuf,
        source: io::Error,
    },
    /// A live peer holds the single-writer lock for this home. Carries the
    /// full diagnostic (record if readable, probe outcome).
    HeldByPeer(Box<HeldByPeerDiagnostic>),
}

impl fmt::Display for InstanceLockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstanceLockError::Io { path, source } => {
                write!(f, "cannot open lock file {}: {source}", path.display())
            }
            InstanceLockError::HeldByPeer(diag) => {
                let HeldByPeerDiagnostic {
                    home,
                    record,
                    record_error,
                    probe,
                } = diag.as_ref();
                write!(
                    f,
                    "another lingxi-service instance holds the single-writer lock for data root {}",
                    home.display()
                )?;
                if let Some(record) = record {
                    write!(
                        f,
                        "; recorded peer pid={} instanceId={} startNonce={} startedAtUnixMs={}",
                        record.pid,
                        record.instance_id,
                        record.start_nonce,
                        record.started_at_unix_ms
                    )?;
                    if let Some(addr) = &record.bind_addr {
                        write!(f, " addr={addr}")?;
                    }
                } else if let Some(err) = record_error {
                    write!(f, "; instance record unreadable: {err}")?;
                } else {
                    write!(f, "; no instance record (peer may still be starting up)")?;
                }
                if let Some(probe) = probe {
                    match probe {
                        PeerProbe::Live => write!(f, "; health probe: peer is LIVE")?,
                        PeerProbe::Unreachable(reason) => {
                            write!(f, "; health probe unreachable ({reason}); the OS file lock is the authority, not PID checks")?
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for InstanceLockError {}

/// Machine-readable stderr marker emitted by the binary when a second
/// instance is rejected (mirrors the Node gate's LINGXI_* marker style).
pub const SINGLE_WRITER_BLOCKED_MARKER: &str = "LINGXI_SERVICE_SINGLE_WRITER_BLOCKED";
/// Machine-readable stderr marker emitted when a stale record is taken over.
pub const STALE_RECORD_MARKER: &str = "LINGXI_SERVICE_STALE_RECORD_TAKEN_OVER";

/// An acquired single-writer lock plus this instance's identity. Dropping
/// the guard releases the OS lock (process exit included); explicit
/// [`InstanceGuard::release`] additionally removes the owned record.
#[derive(Debug)]
pub struct InstanceGuard {
    lock_file: File,
    layout: DataRootLayout,
    identity: InstanceIdentity,
    published: bool,
}

/// Attempts to acquire the single-writer lock for `layout.home`.
///
/// On success, returns the guard plus the stale record that was found and
/// archived (if any) so the caller can log/handshake-compare it.
pub fn acquire(
    layout: &DataRootLayout,
) -> Result<(InstanceGuard, Option<InstanceRecord>), InstanceLockError> {
    let lock_file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&layout.lock_path)
        .map_err(|source| InstanceLockError::Io {
            path: layout.lock_path.clone(),
            source,
        })?;
    if let Err(err) = lock_file.try_lock() {
        tracing::debug!(lock_error = %err, "try_lock refused: a live peer holds the lock");
        // A live peer holds the lock. Build the full diagnostic; the lock
        // is the authority, the record and probe only explain WHO.
        let (record, record_error) = match read_record(&layout.record_path) {
            Some(Ok(record)) => (Some(record), None),
            Some(Err(err)) => (None, Some(err.to_string())),
            None => (None, None),
        };
        let probe = record
            .as_ref()
            .and_then(|r| r.bind_addr.as_deref())
            .map(probe_peer);
        return Err(InstanceLockError::HeldByPeer(Box::new(
            HeldByPeerDiagnostic {
                home: layout.home.clone(),
                record,
                record_error,
                probe,
            },
        )));
    }

    // Lock acquired: any record left on disk is by construction stale
    // (its owner no longer holds the lock). Archive it, never delete it.
    let mut stale = None;
    match read_record(&layout.record_path) {
        Some(Ok(record)) => {
            if let Err(err) =
                atomic_write(&layout.stale_archive_path, record_json(&record).as_bytes())
            {
                tracing::warn!(
                    stale_archive = %layout.stale_archive_path.display(),
                    %err,
                    "cannot archive stale instance record (continuing; the record will be atomically replaced)"
                );
            }
            stale = Some(record);
        }
        Some(Err(err)) => {
            tracing::warn!(
                %err,
                "unreadable leftover instance record found while taking over the lock; it will be replaced"
            );
        }
        None => {}
    }

    Ok((
        InstanceGuard {
            lock_file,
            layout: layout.clone(),
            identity: InstanceIdentity::generate(),
            published: false,
        },
        stale,
    ))
}

fn record_json(record: &InstanceRecord) -> String {
    serde_json::to_string_pretty(record).unwrap_or_else(|_| {
        // Serialization of this plain struct cannot fail; if it ever does,
        // fall back to a minimal valid JSON rather than crashing startup.
        format!(
            "{{\"instanceId\":\"{}\",\"unserializable\":true}}",
            record.instance_id
        )
    })
}

impl InstanceGuard {
    pub fn identity(&self) -> &InstanceIdentity {
        &self.identity
    }

    pub fn layout(&self) -> &DataRootLayout {
        &self.layout
    }

    /// Atomically publishes the instance record (called once the listener
    /// is bound and the concrete address is known).
    pub fn publish(&mut self, bind_addr: SocketAddr) -> io::Result<()> {
        let record = InstanceRecord::for_identity(&self.identity, bind_addr, &self.layout.home);
        atomic_write(&self.layout.record_path, record_json(&record).as_bytes())?;
        self.published = true;
        Ok(())
    }

    /// Releases the claim: removes the instance record **only if it is
    /// still our own** (re-read + identity match — a foreign record is
    /// never deleted by us), then explicitly unlocks the OS lock (the
    /// file drop would also release it; being explicit keeps the shutdown
    /// path observable in logs and errors).
    pub fn release(&mut self) -> io::Result<()> {
        self.cleanup_own_record()?;
        self.lock_file.unlock()
    }

    /// Shutdown cleanup only: removes the record iff it carries our
    /// instance_id. A record belonging to anyone else (should be
    /// impossible while we hold the lock, but a defensive check is cheap)
    /// is left in place and reported.
    pub fn cleanup_own_record(&mut self) -> io::Result<()> {
        if !self.published {
            return Ok(());
        }
        match read_record(&self.layout.record_path) {
            Some(Ok(record)) => {
                if record.instance_id == self.identity.instance_id {
                    std::fs::remove_file(&self.layout.record_path)
                } else {
                    tracing::warn!(
                        foreign_instance = %record.instance_id,
                        "instance record on shutdown is not ours; leaving it in place"
                    );
                    Ok(())
                }
            }
            Some(Err(err)) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("cannot verify record ownership for cleanup: {err}"),
            )),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::prepare_layout;

    fn synthetic(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("lingxi-r02t02-inst-{}-{tag}", std::process::id()))
    }

    #[test]
    fn identity_is_unique_and_well_formed() {
        let a = InstanceIdentity::generate();
        let b = InstanceIdentity::generate();
        assert_ne!(a.instance_id, b.instance_id);
        assert_ne!(a.start_nonce, b.start_nonce);
        assert_eq!(a.instance_id.len(), 32);
        assert_eq!(a.start_nonce.len(), 16);
        assert!(a.instance_id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn second_acquire_in_process_is_rejected_with_diagnostics() {
        let home = synthetic("lock-hold");
        let _ = std::fs::remove_dir_all(&home);
        let layout = prepare_layout(&home).unwrap();
        let (mut guard, stale) = acquire(&layout).unwrap();
        assert!(stale.is_none());

        let err = acquire(&layout).unwrap_err();
        match &err {
            InstanceLockError::HeldByPeer(diag) => {
                // The guard has not published yet -> "no instance record".
                assert!(diag.record.is_none());
            }
            other => panic!("expected HeldByPeer, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("single-writer lock"), "message: {msg}");
        assert!(msg.contains("no instance record"), "message: {msg}");

        // Publish, then a second acquire must carry the full diagnostics.
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        guard.publish(addr).unwrap();
        let err = acquire(&layout).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("single-writer lock"), "message: {msg}");
        assert!(
            msg.contains(&format!("pid={}", guard.identity().pid)),
            "message: {msg}"
        );
        assert!(
            msg.contains(&format!("instanceId={}", guard.identity().instance_id)),
            "message: {msg}"
        );
        assert!(msg.contains("addr=127.0.0.1:1"), "message: {msg}");
        // Probe against a port nothing serves: diagnostic-only unreachable.
        assert!(msg.contains("health probe unreachable"), "message: {msg}");

        // Releasing the guard must free the home for the next instance.
        guard.release().unwrap();
        assert!(!layout.record_path.exists(), "own record must be removed");
        let (_guard2, stale2) = acquire(&layout).unwrap();
        assert!(stale2.is_none());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn stale_record_is_archived_and_taken_over_regardless_of_pid() {
        let home = synthetic("stale-takeover");
        let _ = std::fs::remove_dir_all(&home);
        let layout = prepare_layout(&home).unwrap();

        // Simulate a crashed owner: record on disk (claiming a LIVE,
        // reused-unrelated pid — our own test-runner pid qualifies), but
        // NO lock held (the owner is gone). PID existence must not matter.
        let fake = InstanceRecord {
            schema_version: INSTANCE_RECORD_SCHEMA_VERSION,
            server_kind: crate::SERVER_KIND.to_string(),
            server_version: "0.0.0".to_string(),
            wire_protocol_min: 1,
            wire_protocol_max: 1,
            data_epoch: 1,
            instance_id: "deadbeefdeadbeefdeadbeefdeadbeef".to_string(),
            start_nonce: "cafebabecafebabe".to_string(),
            pid: std::process::id(), // exists right now, unrelated process
            bind_addr: Some("127.0.0.1:1".parse::<SocketAddr>().unwrap().to_string()),
            started_at_unix_ms: 1,
            home_path: layout.home.display().to_string(),
        };
        std::fs::write(
            &layout.record_path,
            serde_json::to_string_pretty(&fake).unwrap(),
        )
        .unwrap();

        let (mut guard, stale) = acquire(&layout).unwrap();
        let stale = stale.expect("stale record must be detected");
        assert_eq!(stale.instance_id, fake.instance_id);
        // Archived verbatim.
        let archived: InstanceRecord =
            serde_json::from_str(&std::fs::read_to_string(&layout.stale_archive_path).unwrap())
                .unwrap();
        assert_eq!(archived, fake);
        // No handshake mismatch at identical versions.
        assert!(stale.handshake_mismatches().is_empty());

        // New identity owns the record now.
        let addr: SocketAddr = "127.0.0.1:2".parse().unwrap();
        guard.publish(addr).unwrap();
        let current: InstanceRecord =
            serde_json::from_str(&std::fs::read_to_string(&layout.record_path).unwrap()).unwrap();
        assert_eq!(current.instance_id, guard.identity().instance_id);
        assert_ne!(current.instance_id, fake.instance_id);
        guard.release().unwrap();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn stale_record_with_nonexistent_pid_is_still_taken_over() {
        // The mirror case: a dead pid in the record AND no lock. Takeover
        // must happen through the lock, not through pid liveness.
        let home = synthetic("stale-deadpid");
        let _ = std::fs::remove_dir_all(&home);
        let layout = prepare_layout(&home).unwrap();
        let fake = InstanceRecord {
            schema_version: INSTANCE_RECORD_SCHEMA_VERSION,
            server_kind: crate::SERVER_KIND.to_string(),
            server_version: "0.0.0".to_string(),
            wire_protocol_min: 1,
            wire_protocol_max: 1,
            data_epoch: 1,
            instance_id: "aabb".repeat(8),
            start_nonce: "0011".repeat(4),
            pid: u32::MAX, // effectively never a real pid
            bind_addr: None,
            started_at_unix_ms: 0,
            home_path: layout.home.display().to_string(),
        };
        std::fs::write(
            &layout.record_path,
            serde_json::to_string_pretty(&fake).unwrap(),
        )
        .unwrap();
        let (_guard, stale) = acquire(&layout).unwrap();
        assert_eq!(stale.expect("takeover").pid, u32::MAX);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn held_lock_with_live_foreign_pid_still_rejects() {
        // Adversarial: record claims OUR pid (alive!) and the lock IS held
        // by another open file description. Decision must be rejection —
        // pid existence never overrides the lock.
        let home = synthetic("lock-foreignpid");
        let _ = std::fs::remove_dir_all(&home);
        let layout = prepare_layout(&home).unwrap();
        let (mut guard, _) = acquire(&layout).unwrap();
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        guard.publish(addr).unwrap();

        // Overwrite the record to lie about the pid (simulating pid reuse
        // into an unrelated live process — our own).
        let mut lying: InstanceRecord =
            serde_json::from_str(&std::fs::read_to_string(&layout.record_path).unwrap()).unwrap();
        lying.pid = std::process::id();
        std::fs::write(
            &layout.record_path,
            serde_json::to_string_pretty(&lying).unwrap(),
        )
        .unwrap();

        let err = acquire(&layout).unwrap_err();
        assert!(
            matches!(err, InstanceLockError::HeldByPeer(_)),
            "lock authority must reject regardless of recorded pid: {err}"
        );
        guard.release().unwrap();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn release_never_deletes_a_foreign_record() {
        let home = synthetic("cleanup-foreign");
        let _ = std::fs::remove_dir_all(&home);
        let layout = prepare_layout(&home).unwrap();
        let (mut guard, _) = acquire(&layout).unwrap();
        let addr: SocketAddr = "127.0.0.1:3".parse().unwrap();
        guard.publish(addr).unwrap();

        // Sabotage: replace the record with a foreign one (as if a next
        // owner raced in; impossible while we hold the lock, but cleanup
        // must be defensive).
        let foreign = InstanceRecord {
            schema_version: INSTANCE_RECORD_SCHEMA_VERSION,
            server_kind: crate::SERVER_KIND.to_string(),
            server_version: "0.0.0".to_string(),
            wire_protocol_min: 1,
            wire_protocol_max: 1,
            data_epoch: 1,
            instance_id: "ff".repeat(16),
            start_nonce: "ee".repeat(8),
            pid: 1,
            bind_addr: None,
            started_at_unix_ms: 1,
            home_path: layout.home.display().to_string(),
        };
        std::fs::write(
            &layout.record_path,
            serde_json::to_string_pretty(&foreign).unwrap(),
        )
        .unwrap();
        guard.release().unwrap();
        assert!(
            layout.record_path.exists(),
            "foreign record must NOT be deleted by our shutdown"
        );
        let left: InstanceRecord =
            serde_json::from_str(&std::fs::read_to_string(&layout.record_path).unwrap()).unwrap();
        assert_eq!(left.instance_id, foreign.instance_id);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn record_handshake_mismatch_is_reported() {
        let mut record = InstanceRecord::for_identity(
            &InstanceIdentity::generate(),
            "127.0.0.1:9".parse().unwrap(),
            Path::new("/tmp/x"),
        );
        assert!(record.handshake_mismatches().is_empty());
        record.data_epoch = 2;
        record.server_version = "9.9.9".to_string();
        let mismatches = record.handshake_mismatches();
        assert_eq!(mismatches.len(), 2, "mismatches: {mismatches:?}");
        assert!(
            mismatches.iter().any(|m| m.contains("data epoch 2")),
            "mismatches: {mismatches:?}"
        );
    }

    #[test]
    fn record_json_round_trips_camel_case() {
        let record = InstanceRecord::for_identity(
            &InstanceIdentity::generate(),
            "127.0.0.1:9".parse().unwrap(),
            Path::new("/tmp/x"),
        );
        let json = serde_json::to_value(&record).unwrap();
        assert!(json.get("schemaVersion").is_some());
        assert!(json.get("instanceId").is_some());
        assert!(json.get("startNonce").is_some());
        assert!(json.get("bindAddr").is_some());
        assert!(json.get("startedAtUnixMs").is_some());
        assert!(json.get("wireProtocolMin").is_some());
        let back: InstanceRecord = serde_json::from_value(json).unwrap();
        assert_eq!(back, record);
    }

    #[test]
    fn unreadable_leftover_record_is_replaced_not_fatal() {
        let home = synthetic("leftover-garbage");
        let _ = std::fs::remove_dir_all(&home);
        let layout = prepare_layout(&home).unwrap();
        std::fs::write(&layout.record_path, b"{ not json").unwrap();
        let (mut guard, stale) = acquire(&layout).unwrap();
        assert!(stale.is_none(), "garbage record is not parsed as stale");
        let addr: SocketAddr = "127.0.0.1:4".parse().unwrap();
        guard.publish(addr).unwrap();
        let current: InstanceRecord =
            serde_json::from_str(&std::fs::read_to_string(&layout.record_path).unwrap()).unwrap();
        assert_eq!(current.instance_id, guard.identity().instance_id);
        guard.release().unwrap();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn probe_peer_reports_unreachable_for_dead_port() {
        match probe_peer("127.0.0.1:1") {
            PeerProbe::Unreachable(_) => {}
            PeerProbe::Live => panic!("nothing serves port 1; probe must not report live"),
        }
        match probe_peer("not-an-addr") {
            PeerProbe::Unreachable(_) => {}
            PeerProbe::Live => panic!("invalid address must be unreachable"),
        }
    }
}
