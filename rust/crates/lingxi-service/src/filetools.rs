//! Native read / write / edit file executors (R04-T04).
//!
//! These are REAL filesystem executors behind the T02 gateway — the
//! incumbent semantics (`@earendil-works/pi-coding-agent` read/write/edit
//! + the Lingxi sandbox guards) ported to Rust:
//!
//! # read (`read.js` + `lib/sandbox/read-enhanced.ts` + freshness wrapper)
//! - `path` (relative to the tool cwd or absolute), 1-indexed `offset`,
//!   `limit`; an offset past EOF is the incumbent's explicit error.
//! - Result truncation: 2000 lines / 50 KiB (whichever hits first), never
//!   a partial line, with the incumbent's continuation notices
//!   (`[Showing lines X-Y of Z. Use offset=N to continue.]`,
//!   `[N more lines in file. …]`) and the `truncated` flag set on the
//!   structured result.
//! - Encoding: a UTF-8 BOM is stripped. Content that is not valid UTF-8
//!   is NOT returned as mojibake: the result reports the file as
//!   binary/non-UTF-8 with its real size and digest (the incumbent's
//!   GBK transcoding and xlsx/docx office decoding are worker-surface
//!   capabilities — registered as NOT migrated here, never silently
//!   degraded).
//! - Duplicate-read stub: an ADJACENT identical read (same path/offset/
//!   limit, within 30s, stat unchanged) returns the incumbent's
//!   `[Duplicate read: …]` stub (`lib/sandbox/file-freshness.ts`).
//! - A successful read registers the file's fingerprint (mtime+size) for
//!   the staleness conflict below.
//!
//! # write (`write.js` + mutation-freshness guard)
//! - Creates parent directories, writes UTF-8, `Successfully wrote to
//!   {path}`.
//! - Conflict semantics (never a default silent overwrite of a file the
//!   caller has read at a different version): if THIS session read the
//!   file and its fingerprint has changed since (user/other process),
//!   the call fails `FILE_STALE_SINCE_READ` with zero side effects —
//!   exactly `wrapMutationToolWithFreshness`.
//!
//! # edit (`edit.js` + `edit-diff.js`)
//! - `edits[] { oldText, newText }` matched against the ORIGINAL content
//!   (not incrementally); BOM stripped for matching and restored; CRLF
//!   detected, normalized for matching, restored on write.
//! - Match failures keep the incumbent vocabulary: not-found / `Found N
//!   occurrences` / empty oldText / overlap / no-change — each refuses
//!   with the file untouched.
//! - Exact match first, then the fuzzy layer (trailing-whitespace strip
//!   plus smart quotes/dashes/Unicode spaces folded to ASCII) with the
//!   unchanged-lines-preserving overlay. The incumbent's NFKC fold is
//!   NOT ported (no normalization crate in the locked tree) — a
//!   registered gap: NFKC-only differences fall back to the exact-match
//!   error instead of fuzzy-matching.
//!
//! # Atomicity, concurrency and TOCTOU
//! - All mutations serialize on a per-CANONICAL-path lock (the incumbent
//!   `withFileMutationQueue` keyed by realpath).
//! - Every mutation writes a SAME-DIRECTORY temp file and swaps it in
//!   with one atomic rename; the temp file is removed on every failure
//!   path — success, conflict and failure all leave zero temp residue.
//! - TOCTOU is constrained by PROVABLE mechanisms, not by a single
//!   canonicalize: on Unix the executor opens the parent directory chain
//!   component-by-component with `O_NOFOLLOW` from `/` and opens the
//!   final component `openat(dirfd, …, O_NOFOLLOW)` — a symlink swapped
//!   in after authorization is refused (ELOOP → re-resolve the REAL
//!   target, RE-AUTHORIZE it, retry; bounded churn loop) instead of
//!   silently followed. Before the rename the path is re-opened through
//!   the same dirfd and the (device, inode, mtime, size) must still be
//!   the identity that was read — an external replacement or same-inode
//!   modification in the window is a conflict, never a lost update.
//! - Windows currently uses the std-filesystem fallback (no `O_NOFOLLOW`
//!   walk); junction/reparse targets are detected and refused on the
//!   mutation path. Registered Windows form — real-machine verification
//!   is the stage's Windows deferral.
//!
//! # Failure honesty
//! A write that fails (disk full, permission, interrupted before the
//! rename) leaves the ORIGINAL file in place and records NO success
//! product: the tool outcome is `Failed`, no `ResourceRef` is minted, and
//! the change log receives nothing. Partial side effects (a temp file
//! created then the rename refused) are cleaned up and reported.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::ports::{
    ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess,
};
use lingxi_kernel::toolcatalog::{
    EffectiveArguments, SchemaBudget, ToolManifest, ToolRegistry, ToolTargetId,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ContentDigest, ErrorCode, ProtocolError, ResourceId, ResourceKind, ResourceRef,
    ToolCallId, ToolSchemaDocument,
};
use sha2::{Digest as _, Sha256};
use tracing::warn;

use crate::inject::ServiceClock;
use crate::resourceaccess::{ResourceAccess, ResourceOp, ResourceScope};
use crate::toolgateway::{ResourceExtractionInput, ResourceExtractor, ToolInvocationGateway};

/// Incumbent `DEFAULT_MAX_LINES` (read truncation).
pub const READ_MAX_LINES: usize = 2000;
/// Incumbent `DEFAULT_MAX_BYTES` (read truncation, 50 KiB).
pub const READ_MAX_BYTES: usize = 50 * 1024;
/// Incumbent duplicate-read window (`DUP_READ_WINDOW_MS`).
pub const DUPLICATE_READ_WINDOW_MS: u64 = 30_000;
/// Bound of the freshness registry (`MAX_SESSIONS`).
const FRESHNESS_MAX_SESSIONS: usize = 64;
/// Bound of the freshness registry (`MAX_FILES_PER_SESSION`).
const FRESHNESS_MAX_FILES_PER_SESSION: usize = 256;
/// Cap of the before-version content digest capture for `write` (the
/// after-digest is always computed from the written bytes). Beyond this
/// the record's before-digest is `None` — recorded explicitly, never a
/// silent guess.
const WRITE_BEFORE_DIGEST_CAP: u64 = 8 * 1024 * 1024;
/// Bounded mutation-lock registry (per canonical path).
const MUTATION_LOCK_CAP: usize = 1024;
/// Max symlink-churn re-resolutions per operation.
const MAX_SYMLINK_CHURN: usize = 8;
/// Temp-file prefix (same-directory atomic replace).
const TEMP_PREFIX: &str = ".lingxi-write-";

// ── error vocabulary (incumbent texts; stable codes embedded) ───────────────

/// The incumbent staleness errorCode (`details.errorCode` in
/// `wrapMutationToolWithFreshness`), embedded in the tool error message.
pub const FILE_STALE_SINCE_READ: &str = "FILE_STALE_SINCE_READ";
/// A file was replaced/modified underneath the mutation between the read
/// and the atomic swap (TOCTOU guard) — same conflict class, distinct
/// code so tests can tell them apart.
pub const FILE_CHANGED_DURING_MUTATION: &str = "FILE_CHANGED_DURING_MUTATION";
/// An authorized path kept resolving through moving symlinks.
pub const RESOURCE_SYMLINK_CHURN: &str = "RESOURCE_SYMLINK_CHURN";

fn failed(code: ErrorCode, message: String) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(code, message, false),
    }
}

/// A successful text outcome with the derived audit digest.
fn text_success(text: String, truncated: bool, resource_refs: Vec<ResourceRef>) -> ToolOutcome {
    let content = vec![ContentBlock::Text { text }];
    let value = serde_json::to_value(&content).expect("content blocks serialize");
    let canonical = lingxi_protocol::canon::canonical_json_bytes(&value);
    ToolOutcome::Success {
        result: ToolSuccess {
            content_digest: hex(&Sha256::digest(&canonical)),
            content,
            resource_refs,
            truncated,
            status: None,
        },
    }
}

// ── freshness registry (`lib/sandbox/file-freshness.ts`) ────────────────────

/// (mtime, size) fingerprint of one file — the incumbent's
/// `FileFingerprint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileFingerprint {
    pub modified: std::time::SystemTime,
    pub size: u64,
}

impl FileFingerprint {
    pub fn of_metadata(metadata: &std::fs::Metadata) -> Option<Self> {
        if !metadata.is_file() {
            return None;
        }
        Some(Self {
            modified: metadata.modified().ok()?,
            size: metadata.len(),
        })
    }

    pub fn stat(path: &Path) -> Option<Self> {
        Self::of_metadata(&std::fs::metadata(path).ok()?)
    }

    /// Which field changed relative to `other` ("size" wins, like the
    /// incumbent `checkFreshBeforeMutation`).
    pub fn changed_by(&self, other: &FileFingerprint) -> Option<&'static str> {
        if self.size != other.size {
            Some("size")
        } else if self.modified != other.modified {
            Some("mtime")
        } else {
            None
        }
    }
}

struct SessionFreshness {
    files: HashMap<PathBuf, FileFingerprint>,
    last_read: Option<(String, FileFingerprint, u64 /*at_unix_ms*/)>,
}

#[derive(Default)]
struct FreshnessState {
    sessions: HashMap<String, SessionFreshness>,
    order: Vec<String>,
}

/// Session-scoped freshness registry: read fingerprints in, staleness
/// conflicts out, adjacent duplicate reads stubbed (all incumbent
/// semantics, bounded memory).
pub struct FreshnessRegistry {
    state: std::sync::Mutex<FreshnessState>,
    clock: Arc<dyn ServiceClock>,
    duplicate_window_ms: u64,
}

impl FreshnessRegistry {
    pub fn new(clock: Arc<dyn ServiceClock>) -> Self {
        Self::with_window(clock, DUPLICATE_READ_WINDOW_MS)
    }

    pub fn with_window(clock: Arc<dyn ServiceClock>, duplicate_window_ms: u64) -> Self {
        Self {
            state: std::sync::Mutex::new(FreshnessState::default()),
            clock,
            duplicate_window_ms,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FreshnessState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn session_of<'a>(state: &'a mut FreshnessState, session: &str) -> &'a mut SessionFreshness {
        if !state.sessions.contains_key(session) {
            state.order.push(session.to_string());
            while state.order.len() > FRESHNESS_MAX_SESSIONS {
                let evict = state.order.remove(0);
                state.sessions.remove(&evict);
            }
            state.sessions.insert(
                session.to_string(),
                SessionFreshness {
                    files: HashMap::new(),
                    last_read: None,
                },
            );
        }
        state.sessions.get_mut(session).expect("just inserted")
    }

    fn bound_files(record: &mut SessionFreshness) {
        while record.files.len() > FRESHNESS_MAX_FILES_PER_SESSION {
            let evict = record.files.keys().next().cloned();
            match evict {
                Some(key) => {
                    record.files.remove(&key);
                }
                None => break,
            }
        }
    }

    /// A successful read registered its fingerprint and read identity.
    pub fn observe_read(
        &self,
        session: &str,
        canonical: &Path,
        fingerprint: FileFingerprint,
        read_key: String,
    ) {
        let now = self.clock.now_unix_ms();
        let mut state = self.lock();
        let record = Self::session_of(&mut state, session);
        record.files.insert(canonical.to_path_buf(), fingerprint);
        record.last_read = Some((read_key, fingerprint, now));
        Self::bound_files(record);
    }

    /// Adjacent identical read (same identity key, inside the window,
    /// fingerprint unchanged) → the age in seconds (>= 1), else `None`.
    pub fn check_duplicate(&self, session: &str, canonical: &Path, read_key: &str) -> Option<u64> {
        let state = self.lock();
        let record = state.sessions.get(session)?;
        let (key, fingerprint, at) = record.last_read.as_ref()?;
        if key != read_key {
            return None;
        }
        let now = self.clock.now_unix_ms();
        let age_ms = now.checked_sub(*at)?;
        if age_ms > self.duplicate_window_ms {
            return None;
        }
        let current = FileFingerprint::stat(canonical)?;
        if current.changed_by(fingerprint).is_some() {
            return None;
        }
        Some((age_ms / 1000).max(1))
    }

    /// The pre-mutation staleness check: the session registered a
    /// fingerprint and the CURRENT one differs → Some(what changed).
    /// No registration / unstatable → `None` (the incumbent passes).
    pub fn check_stale(&self, session: &str, canonical: &Path) -> Option<&'static str> {
        let state = self.lock();
        let observed = state.sessions.get(session)?.files.get(canonical).copied()?;
        let current = FileFingerprint::stat(canonical)?;
        current.changed_by(&observed)
    }

    /// A successful mutation writes the new fingerprint back so
    /// consecutive edits don't flag their own output as external.
    pub fn observe_mutation(&self, session: &str, canonical: &Path, fingerprint: FileFingerprint) {
        let mut state = self.lock();
        let record = Self::session_of(&mut state, session);
        record.files.insert(canonical.to_path_buf(), fingerprint);
        Self::bound_files(record);
    }

    /// Drops one session's registry (session teardown).
    pub fn forget_session(&self, session: &str) {
        let mut state = self.lock();
        state.sessions.remove(session);
        state.order.retain(|s| s != session);
    }
}

impl std::fmt::Debug for FileTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileTools")
            .field("cwd", &self.cwd.display().to_string())
            .finish_non_exhaustive()
    }
}

// ── the modification-record interface (checkpoint/rewind hand-off) ──────────

/// One file version snapshot (checkpoint/rewind input; the full product
/// integration is R06/R07 — this is the explicit recording interface the
/// task freezes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileVersion {
    pub size_bytes: u64,
    /// SHA-256 of the raw bytes; `None` only for the before-version of a
    /// very large overwrite (capture cap) — recorded as absent, never
    /// guessed.
    pub sha256_hex: Option<String>,
    pub mtime_unix_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeOperation {
    Created,
    Modified,
}

impl FileChangeOperation {
    pub fn wire_name(self) -> &'static str {
        match self {
            FileChangeOperation::Created => "created",
            FileChangeOperation::Modified => "modified",
        }
    }
}

/// The explicit file-modification record (what checkpoint/rewind and the
/// file-change journal consume later).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileModificationRecord {
    /// Canonical real path.
    pub path: PathBuf,
    pub operation: FileChangeOperation,
    pub before: Option<FileVersion>,
    pub after: FileVersion,
    pub at_unix_ms: u64,
}

/// Receives one record per SUCCESSFUL mutation (failed/interrupted
/// mutations produce nothing — a write that did not land is not a
/// product).
pub trait FileChangeLog: Send + Sync {
    fn record(&self, record: FileModificationRecord);
}

/// The default no-op log (no product integration before R06).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopChangeLog;

impl FileChangeLog for NoopChangeLog {
    fn record(&self, _record: FileModificationRecord) {}
}

// ── mutation serialization (per canonical path) ─────────────────────────────

#[derive(Default)]
struct MutationLocks {
    locks: HashMap<PathBuf, Arc<std::sync::Mutex<()>>>,
}

impl MutationLocks {
    fn lock_of(&mut self, canonical: &Path) -> Arc<std::sync::Mutex<()>> {
        if let Some(existing) = self.locks.get(canonical) {
            return Arc::clone(existing);
        }
        let fresh = Arc::new(std::sync::Mutex::new(()));
        self.locks
            .insert(canonical.to_path_buf(), Arc::clone(&fresh));
        // Bounded: when over cap, drop entries nobody holds anymore.
        if self.locks.len() > MUTATION_LOCK_CAP {
            self.locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        }
        fresh
    }
}

// ── filesystem IO discipline (unix dirfd walk; windows std fallback) ───────

#[cfg(unix)]
mod fsio {
    use std::ffi::OsStr;
    use std::io;
    use std::os::unix::io::{FromRawFd, RawFd};
    use std::path::{Component, Path};

    use super::TEMP_PREFIX;

    /// An owned parent-directory handle. All opens of the final target
    /// are `openat`-relative to THIS handle, so a path component swapped
    /// after authorization cannot redirect the operation.
    pub struct DirFd {
        fd: RawFd,
    }

    impl Drop for DirFd {
        fn drop(&mut self) {
            // # Safety: the fd is owned, opened once and never duplicated.
            unsafe { libc::close(self.fd) };
        }
    }

    impl DirFd {
        /// Opens the directory chain of a CANONICAL path component by
        /// component from `/` with `O_NOFOLLOW`: canonical paths carry no
        /// symlinks, so an `ELOOP` here is a post-authorization swap.
        /// `NotFound` may mean legitimately-missing parents (write path
        /// creates them); the caller decides.
        pub fn open(path: &Path) -> io::Result<DirFd> {
            let mut fd = io_error_fd(unsafe {
                libc::open(
                    c"/".as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
                )
            })?;
            for component in path.components() {
                match component {
                    Component::RootDir | Component::Prefix(_) => {}
                    Component::Normal(name) => {
                        let cname = os_to_c(name);
                        let next = io_error_fd(unsafe {
                            libc::openat(
                                fd,
                                cname.as_ptr().cast(),
                                libc::O_RDONLY
                                    | libc::O_DIRECTORY
                                    | libc::O_NOFOLLOW
                                    | libc::O_CLOEXEC,
                            )
                        })?;
                        let _ = unsafe { libc::close(fd) };
                        fd = next;
                    }
                    Component::CurDir | Component::ParentDir => {
                        return Err(io::Error::other(
                            "canonical paths never carry `.`/`..` components",
                        ));
                    }
                }
            }
            Ok(DirFd { fd })
        }

        /// Opens the FINAL component read-only without following a
        /// symlink: `Some(Ok(file))` when it exists, `None` on ENOENT,
        /// `Some(Err(ELOOP/ENOTDIR))` when it is (or resolves through) a
        /// link.
        pub fn open_file_nofollow(&self, name: &OsStr) -> Option<io::Result<std::fs::File>> {
            let cname = os_to_c(name);
            let fd = io_error_fd(unsafe {
                libc::openat(
                    self.fd,
                    cname.as_ptr().cast(),
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            });
            match fd {
                Ok(fd) => Some(Ok(unsafe { std::fs::File::from_raw_fd(fd) })),
                Err(err)
                    if err.raw_os_error() == Some(libc::ELOOP)
                        || err.raw_os_error() == Some(libc::ENOTDIR) =>
                {
                    Some(Err(err))
                }
                Err(err) if err.raw_os_error() == Some(libc::ENOENT) => None,
                Err(err) => Some(Err(err)),
            }
        }

        /// Creates a NEW exclusive temp file in this directory.
        pub fn create_temp(
            &self,
            random_hex: &str,
        ) -> io::Result<(std::fs::File, std::ffi::OsString)> {
            let mut attempt = 0u32;
            loop {
                let name = std::ffi::OsString::from(format!("{TEMP_PREFIX}{random_hex}-{attempt}"));
                let cname = os_to_c(&name);
                let fd = io_error_fd(unsafe {
                    libc::openat(
                        self.fd,
                        cname.as_ptr().cast(),
                        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC,
                        0o644,
                    )
                });
                match fd {
                    Ok(fd) => {
                        return Ok((unsafe { std::fs::File::from_raw_fd(fd) }, name));
                    }
                    Err(err) if err.raw_os_error() == Some(libc::EEXIST) => {
                        attempt += 1;
                        if attempt > 64 {
                            return Err(io::Error::other(
                                "temp-file name collisions exhausted in the target directory",
                            ));
                        }
                    }
                    Err(err) => return Err(err),
                }
            }
        }

        /// Atomically renames `from` → `to` WITHIN this directory.
        pub fn rename_within(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            let cfrom = os_to_c(from);
            let cto = os_to_c(to);
            let rc = unsafe {
                libc::renameat(self.fd, cfrom.as_ptr().cast(), self.fd, cto.as_ptr().cast())
            };
            if rc != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Removes a file in this directory (temp cleanup).
        pub fn unlink(&self, name: &OsStr) {
            let cname = os_to_c(name);
            // # Safety: the fd is owned and valid for the lifetime of self.
            unsafe { libc::unlinkat(self.fd, cname.as_ptr().cast(), 0) };
        }

        /// Applies the original file's permission bits to the temp file
        /// so an atomic replace does not silently chmod the target.
        pub fn apply_mode(file: &std::fs::File, mode: u32) {
            use std::os::unix::io::AsRawFd;
            // # Safety: the fd is borrowed for the call only.
            unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) };
        }

        /// Best-effort durability of a completed rename.
        pub fn sync(&self) {
            // # Safety: the fd is owned and valid.
            unsafe { libc::fsync(self.fd) };
        }
    }

    /// The identity of one open file — (device, inode, mtime, size,
    /// mode) from the HANDLE's own metadata (never re-stat by path).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FileIdentity {
        pub dev: u64,
        pub ino: u64,
        pub modified: std::time::SystemTime,
        pub size: u64,
        pub mode: u32,
    }

    impl FileIdentity {
        pub fn of(file: &std::fs::File) -> io::Result<Self> {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata()?;
            Ok(Self {
                dev: metadata.dev(),
                ino: metadata.ino(),
                modified: metadata.modified()?,
                size: metadata.len(),
                mode: metadata.mode(),
            })
        }

        pub fn fingerprint(&self) -> super::FileFingerprint {
            super::FileFingerprint {
                modified: self.modified,
                size: self.size,
            }
        }
    }

    /// Whether the error is the "symlink where a real file was
    /// authorized" signal (ELOOP, or ENOTDIR: the leaf resolves through
    /// a file — both mean the path no longer is what was authorized).
    pub fn is_symlink_swap(err: &io::Error) -> bool {
        err.raw_os_error() == Some(libc::ELOOP) || err.raw_os_error() == Some(libc::ENOTDIR)
    }

    fn io_error_fd(fd: RawFd) -> io::Result<RawFd> {
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(fd)
        }
    }

    fn os_to_c(name: &OsStr) -> Vec<u8> {
        use std::os::unix::ffi::OsStrExt;
        let mut bytes = name.as_bytes().to_vec();
        bytes.push(0);
        bytes
    }
}

/// The std-filesystem fallback (Windows): the same open/temp/rename
/// sequence without the `O_NOFOLLOW` walk. A reparse-point final
/// component is detected and refused (junction honesty); the open-time
/// symlink-swap race is NOT closed on Windows — registered form,
/// real-machine verification deferred to the stage's Windows leg.
#[cfg(windows)]
mod fsio {
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::path::Path;

    use super::TEMP_PREFIX;

    /// Message marker shared by every reparse refusal in this module
    /// (detected by [`is_symlink_swap`]).
    const SYMLINK_SWAP_MARK: &str = "SYMLINK-OR-REPARSE-WHERE-A-REAL-FILE-WAS-AUTHORIZED";

    pub struct DirFd {
        dir: std::path::PathBuf,
    }

    impl DirFd {
        pub fn open(path: &Path) -> io::Result<DirFd> {
            // The parent chain of a canonical path: verified via
            // canonicalize on open (junctions resolve there).
            std::fs::canonicalize(path)?;
            Ok(DirFd {
                dir: path.to_path_buf(),
            })
        }

        pub fn open_file_nofollow(&self, name: &OsStr) -> Option<io::Result<std::fs::File>> {
            let target = self.dir.join(name);
            match std::fs::symlink_metadata(&target) {
                Ok(meta) => {
                    if meta.file_type().is_symlink()
                        || crate::resourceaccess::is_reparse_point(&target).unwrap_or(false)
                    {
                        return Some(Err(io::Error::other(format!(
                            "{SYMLINK_SWAP_MARK}: {target:?}"
                        ))));
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
                Err(err) => return Some(Err(err)),
            }
            match std::fs::File::open(&target) {
                Ok(file) => Some(Ok(file)),
                Err(err) if err.kind() == io::ErrorKind::NotFound => None,
                Err(err) => Some(Err(err)),
            }
        }

        pub fn create_temp(&self, random_hex: &str) -> io::Result<(std::fs::File, OsString)> {
            let mut attempt = 0u32;
            loop {
                let name = OsString::from(format!("{TEMP_PREFIX}{random_hex}-{attempt}"));
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(self.dir.join(&name))
                {
                    Ok(file) => return Ok((file, name)),
                    Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                        attempt += 1;
                        if attempt > 64 {
                            return Err(io::Error::other(
                                "temp-file name collisions exhausted in the target directory",
                            ));
                        }
                    }
                    Err(err) => return Err(err),
                }
            }
        }

        pub fn rename_within(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            std::fs::rename(self.dir.join(from), self.dir.join(to))
        }

        pub fn unlink(&self, name: &OsStr) {
            let _ = std::fs::remove_file(self.dir.join(name));
        }

        pub fn apply_mode(_file: &std::fs::File, _mode: u32) {
            // Windows permission bits are ACL-based; the std fallback
            // keeps the temp file's default ACL. Registered form.
        }

        pub fn sync(&self) {
            // Same-directory renames are journaled by the filesystem
            // driver; std has no directory fsync handle.
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FileIdentity {
        pub modified: std::time::SystemTime,
        pub size: u64,
    }

    impl FileIdentity {
        pub fn of(file: &std::fs::File) -> io::Result<Self> {
            let metadata = file.metadata()?;
            Ok(Self {
                modified: metadata.modified()?,
                size: metadata.len(),
            })
        }

        pub fn fingerprint(&self) -> super::FileFingerprint {
            super::FileFingerprint {
                modified: self.modified,
                size: self.size,
            }
        }
    }

    pub fn is_symlink_swap(err: &io::Error) -> bool {
        err.to_string().contains(SYMLINK_SWAP_MARK)
    }
}

#[cfg(unix)]
use fsio::{DirFd, FileIdentity};
#[cfg(unix)]
fn is_symlink_swap(err: &std::io::Error) -> bool {
    fsio::is_symlink_swap(err)
}

#[cfg(windows)]
use fsio::{DirFd, FileIdentity};
#[cfg(windows)]
fn is_symlink_swap(err: &std::io::Error) -> bool {
    fsio::is_symlink_swap(err)
}

// ── edit matching (port of `edit-diff.js`) ──────────────────────────────────

fn normalize_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn detect_line_ending(content: &str) -> &'static str {
    let crlf = content.find("\r\n");
    let lf = content.find('\n');
    match (crlf, lf) {
        (Some(c), Some(l)) if c < l => "\r\n",
        _ => "\n",
    }
}

fn restore_line_endings(text: &str, ending: &str) -> String {
    if ending == "\r\n" {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
    }
}

/// The incumbent's confusable folding minus NFKC (registered gap): per
/// line trailing-whitespace strip + smart quotes/dashes/Unicode spaces.
fn normalize_for_fuzzy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (line_no, line) in text.split('\n').enumerate() {
        if line_no > 0 {
            out.push('\n');
        }
        for ch in line.trim_end().chars() {
            match ch {
                '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => out.push('\''),
                '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => out.push('"'),
                '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
                | '\u{2212}' => out.push('-'),
                '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => {
                    out.push(' ');
                }
                other => out.push(other),
            }
        }
    }
    out
}

struct FuzzyFind {
    found: bool,
    index: usize,
    match_len: usize,
}

fn fuzzy_find(content: &str, old_text: &str) -> FuzzyFind {
    if let Some(index) = content.find(old_text) {
        return FuzzyFind {
            found: true,
            index,
            match_len: old_text.len(),
        };
    }
    let fuzzy_content = normalize_for_fuzzy(content);
    let fuzzy_old = normalize_for_fuzzy(old_text);
    match fuzzy_content.find(&fuzzy_old) {
        Some(index) => FuzzyFind {
            found: true,
            index,
            match_len: fuzzy_old.len(),
        },
        None => FuzzyFind {
            found: false,
            index: 0,
            match_len: 0,
        },
    }
}

fn count_occurrences(content: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    content.match_indices(needle).count()
}

#[derive(Clone)]
struct MatchedEdit {
    edit_index: usize,
    match_index: usize,
    match_length: usize,
    new_text: String,
}

/// The edit-application errors, carrying the incumbent's exact texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditMatchError {
    EmptyOldText { index: usize },
    NotFound { index: usize },
    Duplicate { index: usize, occurrences: usize },
    Overlap { first: usize, second: usize },
    NoChange,
}

impl EditMatchError {
    /// The incumbent message for the error (single-edit and
    /// multi-edit phrasings both preserved).
    pub fn message(&self, path: &str, total_edits: usize) -> String {
        match self {
            EditMatchError::EmptyOldText { index } => {
                if total_edits == 1 {
                    format!("oldText must not be empty in {path}.")
                } else {
                    format!("edits[{index}].oldText must not be empty in {path}.")
                }
            }
            EditMatchError::NotFound { index } => {
                if total_edits == 1 {
                    format!(
                        "Could not find the exact text in {path}. The old text must match exactly \
                         including all whitespace and newlines."
                    )
                } else {
                    format!(
                        "Could not find edits[{index}] in {path}. The oldText must match exactly \
                         including all whitespace and newlines."
                    )
                }
            }
            EditMatchError::Duplicate { index, occurrences } => {
                if total_edits == 1 {
                    format!(
                        "Found {occurrences} occurrences of the text in {path}. The text must be \
                         unique. Please provide more context to make it unique."
                    )
                } else {
                    format!(
                        "Found {occurrences} occurrences of edits[{index}] in {path}. Each \
                         oldText must be unique. Please provide more context to make it unique."
                    )
                }
            }
            EditMatchError::Overlap { first, second } => format!(
                "edits[{first}] and edits[{second}] overlap in {path}. Merge them into one edit \
                 or target disjoint regions."
            ),
            EditMatchError::NoChange => {
                if total_edits == 1 {
                    format!(
                        "No changes made to {path}. The replacement produced identical content. \
                         This might indicate an issue with special characters or the text not \
                         existing as expected."
                    )
                } else {
                    format!(
                        "No changes made to {path}. The replacements produced identical content."
                    )
                }
            }
        }
    }
}

#[derive(Clone)]
struct EditPair {
    old_text: String,
    new_text: String,
}

/// Applies all edits against the ORIGINAL (LF-normalized) content — the
/// port of `applyEditsToNormalizedContent` (exact match first; the fuzzy
/// layer runs in normalized space and overlays line-level changes back
/// onto the original bytes).
fn apply_edits(normalized_content: &str, edits: &[EditPair]) -> Result<String, EditMatchError> {
    let normalized_edits: Vec<EditPair> = edits
        .iter()
        .map(|edit| EditPair {
            old_text: normalize_to_lf(&edit.old_text),
            new_text: normalize_to_lf(&edit.new_text),
        })
        .collect();
    for (index, edit) in normalized_edits.iter().enumerate() {
        if edit.old_text.is_empty() {
            return Err(EditMatchError::EmptyOldText { index });
        }
    }
    let used_fuzzy = normalized_edits
        .iter()
        .any(|edit| !normalized_content.contains(&edit.old_text));
    let replacement_base = if used_fuzzy {
        normalize_for_fuzzy(normalized_content)
    } else {
        normalized_content.to_string()
    };
    let mut matched: Vec<MatchedEdit> = Vec::with_capacity(normalized_edits.len());
    for (index, edit) in normalized_edits.iter().enumerate() {
        let find = fuzzy_find(&replacement_base, &edit.old_text);
        if !find.found {
            return Err(EditMatchError::NotFound { index });
        }
        let occurrences = count_occurrences(&replacement_base, &edit.old_text);
        if occurrences > 1 {
            return Err(EditMatchError::Duplicate { index, occurrences });
        }
        matched.push(MatchedEdit {
            edit_index: index,
            match_index: find.index,
            match_length: find.match_len,
            new_text: edit.new_text.clone(),
        });
    }
    matched.sort_by_key(|edit| edit.match_index);
    for pair in matched.windows(2) {
        let previous = &pair[0];
        let current = &pair[1];
        if previous.match_index + previous.match_length > current.match_index {
            return Err(EditMatchError::Overlap {
                first: previous.edit_index,
                second: current.edit_index,
            });
        }
    }
    let new_content = if used_fuzzy {
        apply_replacements_preserving_unchanged_lines(
            normalized_content,
            &replacement_base,
            &matched,
        )?
    } else {
        apply_replacements(&replacement_base, &matched)
    };
    if normalized_content == new_content {
        return Err(EditMatchError::NoChange);
    }
    Ok(new_content)
}

fn apply_replacements(content: &str, replacements: &[MatchedEdit]) -> String {
    let mut result = content.to_string();
    for replacement in replacements.iter().rev() {
        let start = replacement.match_index;
        let end = start + replacement.match_length;
        result.replace_range(start..end, &replacement.new_text);
    }
    result
}

/// Splits like the incumbent's `/[^\n]*\n|[^\n]+/g`.
fn split_lines_with_endings(content: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = content.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        let mut end = start;
        while end < bytes.len() && bytes[end] != b'\n' {
            end += 1;
        }
        if end < bytes.len() {
            end += 1; // include the newline
        }
        lines.push(&content[start..end]);
        start = end;
    }
    lines
}

fn line_spans(content: &str) -> Vec<(usize, usize)> {
    let mut offset = 0;
    split_lines_with_endings(content)
        .into_iter()
        .map(|line| {
            let span = (offset, offset + line.len());
            offset = span.1;
            span
        })
        .collect()
}

fn replacement_line_range(
    lines: &[(usize, usize)],
    start_offset: usize,
    end_offset: usize,
) -> Option<(usize, usize)> {
    let mut start_line = None;
    for (index, (start, end)) in lines.iter().enumerate() {
        if start_offset >= *start && start_offset < *end {
            start_line = Some(index);
            break;
        }
    }
    let start_line = start_line?;
    let mut end_line = start_line;
    while end_line < lines.len() && lines[end_line].1 < end_offset {
        end_line += 1;
    }
    if end_line >= lines.len() {
        return None;
    }
    Some((start_line, end_line + 1))
}

fn apply_replacements_preserving_unchanged_lines(
    original_content: &str,
    base_content: &str,
    replacements: &[MatchedEdit],
) -> Result<String, EditMatchError> {
    let original_lines = split_lines_with_endings(original_content);
    let base_lines = line_spans(base_content);
    if original_lines.len() != base_lines.len() {
        // Unreachable through the trimmed-space fuzzy fold (line counts
        // never change); treated as a no-change refusal, never a guess.
        return Err(EditMatchError::NoChange);
    }
    struct Group {
        start_line: usize,
        end_line: usize,
        replacements: Vec<MatchedEdit>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for replacement in replacements {
        let range = replacement_line_range(
            &base_lines,
            replacement.match_index,
            replacement.match_index + replacement.match_length,
        )
        .ok_or(EditMatchError::NoChange)?;
        if let Some(current) = groups.last_mut() {
            if range.0 < current.end_line {
                current.end_line = current.end_line.max(range.1);
                current.replacements.push(replacement.clone());
                continue;
            }
        }
        groups.push(Group {
            start_line: range.0,
            end_line: range.1,
            replacements: vec![replacement.clone()],
        });
    }
    let mut original_index = 0;
    let mut result = String::new();
    for group in &groups {
        result.push_str(&original_lines[original_index..group.start_line].concat());
        let group_start = base_lines[group.start_line].0;
        let group_end = base_lines[group.end_line - 1].1;
        let slice = &base_content[group_start..group_end];
        let shifted: Vec<MatchedEdit> = group
            .replacements
            .iter()
            .map(|replacement| MatchedEdit {
                edit_index: replacement.edit_index,
                match_index: replacement.match_index - group_start,
                match_length: replacement.match_length,
                new_text: replacement.new_text.clone(),
            })
            .collect();
        result.push_str(&apply_replacements(slice, &shifted));
        original_index = group.end_line;
    }
    result.push_str(&original_lines[original_index..].concat());
    Ok(result)
}

// ── read truncation (port of `truncate.js` truncateHead) ────────────────────

struct HeadTruncation {
    content: String,
    truncated: bool,
    truncated_by: Option<&'static str>,
    output_lines: usize,
    first_line_exceeds: bool,
}

fn split_lines_for_counting(content: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = content.split('\n').collect();
    if content.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn truncate_head(content: &str, max_lines: usize, max_bytes: usize) -> HeadTruncation {
    let total_bytes = content.len();
    let lines = split_lines_for_counting(content);
    let total_lines = lines.len();
    if total_lines <= max_lines && total_bytes <= max_bytes {
        return HeadTruncation {
            content: content.to_string(),
            truncated: false,
            truncated_by: None,
            output_lines: total_lines,
            first_line_exceeds: false,
        };
    }
    let first_line_bytes = lines.first().map(|l| l.len()).unwrap_or(0);
    if first_line_bytes > max_bytes {
        return HeadTruncation {
            content: String::new(),
            truncated: true,
            truncated_by: Some("bytes"),
            output_lines: 0,
            first_line_exceeds: true,
        };
    }
    let mut output: Vec<&str> = Vec::with_capacity(max_lines.min(total_lines));
    let mut output_bytes = 0usize;
    let mut truncated_by = "lines";
    for (index, line) in lines.iter().enumerate() {
        if index >= max_lines {
            break;
        }
        let line_bytes = line.len() + usize::from(index > 0);
        if output_bytes + line_bytes > max_bytes {
            truncated_by = "bytes";
            break;
        }
        output.push(line);
        output_bytes += line_bytes;
    }
    if output.len() >= max_lines && output_bytes <= max_bytes {
        truncated_by = "lines";
    }
    let joined = output.join("\n");
    HeadTruncation {
        content: joined,
        truncated: true,
        truncated_by: Some(truncated_by),
        output_lines: output.len(),
        first_line_exceeds: false,
    }
}

// ── the executors ───────────────────────────────────────────────────────────

/// Which native file tool one executor serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileToolKind {
    Read,
    Write,
    Edit,
}

/// Test-only seam (the R04 scope-matrix `test_hooks` allowance): invoked
/// after the temp file is written, immediately BEFORE the pre-rename
/// identity re-verification. NEVER set by production constructors — it
/// lets tests deterministically exercise the TOCTOU window (swap the
/// target file mid-mutation) without mocking any production logic.
pub type MutationTestHook = Arc<dyn Fn(&Path) + Send + Sync>;

/// The shared state of the native file tools.
pub struct FileTools {
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    freshness: FreshnessRegistry,
    changes: Arc<dyn FileChangeLog>,
    clock: Arc<dyn ServiceClock>,
    mutations: std::sync::Mutex<MutationLocks>,
    mutation_hook: std::sync::Mutex<Option<MutationTestHook>>,
}

/// The directory handle + final component of one authorized target.
struct OpenTarget {
    dir: DirFd,
    name: std::ffi::OsString,
}

/// RAII cleanup of the temp file: whatever path the mutation takes
/// (early return, conflict, error, or a PANIC mid-mutation), the temp
/// file is unlinked through the same directory handle; it is disarmed
/// only after the successful atomic rename consumed it.
struct TempGuard<'a> {
    dir: &'a DirFd,
    name: std::ffi::OsString,
    armed: bool,
}

impl TempGuard<'_> {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.dir.unlink(&self.name);
        }
    }
}

/// What `open_file_nofollow` found at the final component.
enum FinalOpen {
    Exists(std::fs::File),
    Missing,
    Swap,
    Error(std::io::Error),
}

fn open_final(target: &OpenTarget) -> FinalOpen {
    match target.dir.open_file_nofollow(&target.name) {
        Some(Ok(file)) => FinalOpen::Exists(file),
        None => FinalOpen::Missing,
        Some(Err(err)) if is_symlink_swap(&err) => FinalOpen::Swap,
        Some(Err(err)) => FinalOpen::Error(err),
    }
}

/// The authorize → dir-chain → final-open walk with the bounded symlink
/// churn loop: any symlink appearing after authorization re-resolves the
/// REAL target and RE-AUTHORIZES it. `create_parents` (write path)
/// creates authorized-but-missing parent directories first.
enum OpenOutcome {
    Ready {
        scope: ResourceScope,
        target: OpenTarget,
        file: Option<std::fs::File>,
    },
    Refused(ToolOutcome),
}

/// Extracts the string `path` argument.
fn arg_path(args: &serde_json::Value) -> Result<String, ToolOutcome> {
    args.get("path")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            failed(
                ErrorCode::InvalidMessage,
                "file tools require a string `path` argument".to_string(),
            )
        })
}

/// The stale-conflict outcome (zero side effects, user content kept).
fn stale_conflict() -> ToolOutcome {
    failed(
        ErrorCode::Conflict,
        format!(
            "{FILE_STALE_SINCE_READ}: File has been modified since it was last read (changed by \
             the user or another process). Re-read the file, then retry this change."
        ),
    )
}

impl FileTools {
    pub fn new(
        access: Arc<ResourceAccess>,
        cwd: PathBuf,
        clock: Arc<dyn ServiceClock>,
        changes: Arc<dyn FileChangeLog>,
    ) -> Self {
        Self {
            access,
            cwd,
            freshness: FreshnessRegistry::new(Arc::clone(&clock)),
            changes,
            clock,
            mutations: std::sync::Mutex::new(MutationLocks::default()),
            mutation_hook: std::sync::Mutex::new(None),
        }
    }

    /// Installs the test-only mutation seam (see [`MutationTestHook`]).
    /// Production code never calls this.
    #[doc(hidden)]
    pub fn set_mutation_test_hook(&self, hook: Option<MutationTestHook>) {
        *self
            .mutation_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = hook;
    }

    /// The resource-scope derivation the gateway binds at prepare and
    /// re-derives before dispatch (`资源规范化` in the invocation chain).
    pub fn resource_extractor(
        access: Arc<ResourceAccess>,
        cwd: PathBuf,
        op: ResourceOp,
    ) -> ResourceExtractor {
        Arc::new(
            move |input: &ResourceExtractionInput, args: &EffectiveArguments| {
                let Some(path) = args.as_value().get("path").and_then(|v| v.as_str()) else {
                    return Err("file tools require a string `path` argument".to_string());
                };
                let scope = access
                    .authorize(
                        input.principal_kind,
                        &input.principal_subject,
                        &input.session_id,
                        Path::new(path),
                        &cwd,
                        op,
                    )
                    .map_err(|refusal| format!("{refusal}"))?;
                Ok(vec![scope])
            },
        )
    }

    fn authorize(
        &self,
        ctx: &RunContext,
        raw_path: &str,
        op: ResourceOp,
    ) -> Result<ResourceScope, ToolOutcome> {
        self.access
            .authorize(
                ctx.principal.storage_kind(),
                &ctx.principal.storage_subject(),
                &ctx.session_id.to_string(),
                Path::new(raw_path),
                &self.cwd,
                op,
            )
            .map_err(|refusal| failed(ErrorCode::Forbidden, format!("{refusal}")))
    }

    fn open_with_churn(
        &self,
        ctx: &RunContext,
        raw_path: &str,
        op: ResourceOp,
        create_parents: bool,
    ) -> OpenOutcome {
        let mut churn = 0usize;
        loop {
            let scope = match self.authorize(ctx, raw_path, op) {
                Ok(scope) => scope,
                Err(outcome) => return OpenOutcome::Refused(outcome),
            };
            let parent = scope.path.parent().unwrap_or(Path::new("/")).to_path_buf();
            let dir = match DirFd::open(&parent) {
                Ok(dir) => dir,
                Err(err) if is_symlink_swap(&err) => {
                    churn += 1;
                    if churn > MAX_SYMLINK_CHURN {
                        return OpenOutcome::Refused(churn_refusal(raw_path, churn));
                    }
                    warn!(
                        path = raw_path,
                        attempt = churn,
                        "authorized path component swapped to a symlink after authorization; \
                         re-resolving and re-authorizing the real target"
                    );
                    continue;
                }
                Err(err) if create_parents && err.kind() == std::io::ErrorKind::NotFound => {
                    if let Err(create_err) = std::fs::create_dir_all(&parent) {
                        return OpenOutcome::Refused(failed(
                            ErrorCode::UpstreamUnavailable,
                            format!(
                                "Could not create parent directories of {raw_path}. Error code: \
                                 {}.",
                                create_err.kind()
                            ),
                        ));
                    }
                    continue;
                }
                Err(err) => {
                    return OpenOutcome::Refused(failed(
                        ErrorCode::UpstreamUnavailable,
                        format!(
                            "Could not open the authorized target of {raw_path}. Error code: {}.",
                            err.kind()
                        ),
                    ));
                }
            };
            let name = match scope.path.file_name() {
                Some(name) => name.to_os_string(),
                None => {
                    return OpenOutcome::Refused(failed(
                        ErrorCode::UpstreamUnavailable,
                        format!(
                            "authorized target {} has no file name",
                            scope.path.display()
                        ),
                    ));
                }
            };
            let target = OpenTarget { dir, name };
            match open_final(&target) {
                FinalOpen::Exists(file) => {
                    return OpenOutcome::Ready {
                        scope,
                        target,
                        file: Some(file),
                    };
                }
                FinalOpen::Missing => {
                    return OpenOutcome::Ready {
                        scope,
                        target,
                        file: None,
                    };
                }
                FinalOpen::Swap => {
                    churn += 1;
                    if churn > MAX_SYMLINK_CHURN {
                        return OpenOutcome::Refused(churn_refusal(raw_path, churn));
                    }
                    warn!(
                        path = raw_path,
                        attempt = churn,
                        "final component became a symlink after authorization; re-resolving and \
                         re-authorizing the real target"
                    );
                }
                FinalOpen::Error(err) => {
                    return OpenOutcome::Refused(failed(
                        ErrorCode::UpstreamUnavailable,
                        format!("Could not open {raw_path}. Error code: {}.", err.kind()),
                    ));
                }
            }
        }
    }

    fn mutation_lock(&self, canonical: &Path) -> Arc<std::sync::Mutex<()>> {
        self.mutations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .lock_of(canonical)
    }

    // ── read ────────────────────────────────────────────────────────────────

    fn run_read(&self, ctx: &RunContext, args: &serde_json::Value) -> ToolOutcome {
        let raw_path = match arg_path(args) {
            Ok(path) => path,
            Err(outcome) => return outcome,
        };
        let offset = args.get("offset").and_then(|v| v.as_i64());
        let limit = args.get("limit").and_then(|v| v.as_i64());
        if let Some(offset) = offset {
            if offset < 1 {
                return failed(
                    ErrorCode::InvalidMessage,
                    format!("offset must be a 1-indexed line number >= 1 (got {offset})"),
                );
            }
        }
        if let Some(limit) = limit {
            if limit < 1 {
                return failed(
                    ErrorCode::InvalidMessage,
                    format!("limit must be >= 1 (got {limit})"),
                );
            }
        }
        // Adjacent duplicate read (incumbent `checkDuplicateRead`): same
        // identity, inside the window, fingerprint unchanged.
        let read_key = format!(
            "{{\"path\":{raw_path:?},\"offset\":{},\"limit\":{}}}",
            offset
                .map(|o| o.to_string())
                .unwrap_or_else(|| "null".to_string()),
            limit
                .map(|l| l.to_string())
                .unwrap_or_else(|| "null".to_string()),
        );
        let (scope, target, file) =
            match self.open_with_churn(ctx, &raw_path, ResourceOp::Read, false) {
                OpenOutcome::Ready {
                    scope,
                    target,
                    file,
                } => (scope, target, file),
                OpenOutcome::Refused(outcome) => return outcome,
            };
        let session = ctx.session_id.to_string();
        if let Some(age_seconds) = self
            .freshness
            .check_duplicate(&session, &scope.path, &read_key)
        {
            return text_success(
                format!(
                    "[Duplicate read: {raw_path} is unchanged since your identical read \
                     {age_seconds}s ago. The earlier tool result is still current — refer to it \
                     instead of re-reading. Read with a different offset or limit to force fresh \
                     content.]"
                ),
                false,
                Vec::new(),
            );
        }
        let Some(mut file) = file else {
            return failed(
                ErrorCode::NotFound,
                format!("Could not read file: {raw_path}. Error code: ENOENT."),
            );
        };
        let identity = match FileIdentity::of(&file) {
            Ok(identity) => identity,
            Err(err) => {
                return failed(
                    ErrorCode::UpstreamUnavailable,
                    format!(
                        "Could not read file: {raw_path}. Error code: {}.",
                        err.kind()
                    ),
                );
            }
        };
        if file.metadata().map(|m| m.is_dir()).unwrap_or(false) {
            return failed(
                ErrorCode::InvalidMessage,
                format!("Could not read file: {raw_path}. It is a directory."),
            );
        }
        let mut bytes = Vec::new();
        if let Err(err) = file.read_to_end(&mut bytes) {
            return failed(
                ErrorCode::UpstreamUnavailable,
                format!(
                    "Could not read file: {raw_path}. Error code: {}.",
                    err.kind()
                ),
            );
        }
        drop(target);
        let fingerprint = identity.fingerprint();
        let outcome = match decode_text(&bytes) {
            TextDecode::Text(text) => render_text_read(&text, offset, limit),
            TextDecode::Binary => {
                // Honest binary/non-UTF-8 result: no mojibake, no silent
                // GBK guess. The incumbent's GBK/office decoding is a
                // worker-surface capability (registered not migrated).
                text_success(
                    format!(
                        "[Read {raw_path}: {} bytes of binary or non-UTF-8 content (sha256 {}). \
                         Text decoding failed; GBK/office decoding and image attachments are \
                         not migrated on the Rust file tools yet — only this metadata is \
                         returned.]",
                        bytes.len(),
                        hex(&Sha256::digest(&bytes)),
                    ),
                    false,
                    Vec::new(),
                )
            }
        };
        if matches!(outcome, ToolOutcome::Success { .. }) {
            self.freshness
                .observe_read(&session, &scope.path, fingerprint, read_key);
        }
        outcome
    }

    // ── write ───────────────────────────────────────────────────────────────

    fn run_write(&self, ctx: &RunContext, args: &serde_json::Value) -> ToolOutcome {
        let raw_path = match arg_path(args) {
            Ok(path) => path,
            Err(outcome) => return outcome,
        };
        let Some(content) = args.get("content").and_then(|v| v.as_str()) else {
            return failed(
                ErrorCode::InvalidMessage,
                "write requires a string `content` argument".to_string(),
            );
        };
        let (scope, target, file) =
            match self.open_with_churn(ctx, &raw_path, ResourceOp::Write, true) {
                OpenOutcome::Ready {
                    scope,
                    target,
                    file,
                } => (scope, target, file),
                OpenOutcome::Refused(outcome) => return outcome,
            };
        let session = ctx.session_id.to_string();
        let mutation_lock = self.mutation_lock(&scope.path);
        let _guard = mutation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Staleness conflict (never a silent overwrite of a read version).
        if self.freshness.check_stale(&session, &scope.path).is_some() {
            return stale_conflict();
        }
        let mut before: Option<(FileIdentity, Option<String>)> = None;
        if let Some(mut file) = file {
            let identity = match FileIdentity::of(&file) {
                Ok(identity) => identity,
                Err(err) => {
                    return failed(
                        ErrorCode::UpstreamUnavailable,
                        format!(
                            "Could not write file: {raw_path}. Error code: {}.",
                            err.kind()
                        ),
                    );
                }
            };
            let digest = if identity.size <= WRITE_BEFORE_DIGEST_CAP {
                let mut bytes = Vec::new();
                if let Err(err) = file.read_to_end(&mut bytes) {
                    return failed(
                        ErrorCode::UpstreamUnavailable,
                        format!(
                            "Could not write file: {raw_path}. Error code: {}.",
                            err.kind()
                        ),
                    );
                }
                Some(hex(&Sha256::digest(&bytes)))
            } else {
                None
            };
            before = Some((identity, digest));
        }
        self.finish_mutation(
            ctx,
            &raw_path,
            &scope,
            &target,
            before.is_some(),
            before,
            content.as_bytes(),
            format!("Successfully wrote to {raw_path}"),
        )
    }

    // ── edit ────────────────────────────────────────────────────────────────

    fn run_edit(&self, ctx: &RunContext, args: &serde_json::Value) -> ToolOutcome {
        let raw_path = match arg_path(args) {
            Ok(path) => path,
            Err(outcome) => return outcome,
        };
        let Some(edits_raw) = args.get("edits").and_then(|v| v.as_array()) else {
            return failed(
                ErrorCode::InvalidMessage,
                "edit requires an `edits` array argument".to_string(),
            );
        };
        let mut edits: Vec<EditPair> = Vec::with_capacity(edits_raw.len());
        for entry in edits_raw {
            let (Some(old_text), Some(new_text)) = (
                entry.get("oldText").and_then(|v| v.as_str()),
                entry.get("newText").and_then(|v| v.as_str()),
            ) else {
                return failed(
                    ErrorCode::InvalidMessage,
                    "each edits[] entry needs string `oldText` and `newText`".to_string(),
                );
            };
            edits.push(EditPair {
                old_text: old_text.to_string(),
                new_text: new_text.to_string(),
            });
        }
        if edits.is_empty() {
            return failed(
                ErrorCode::InvalidMessage,
                "Edit tool input is invalid. edits must contain at least one replacement."
                    .to_string(),
            );
        }
        let (scope, target, file) =
            match self.open_with_churn(ctx, &raw_path, ResourceOp::Write, false) {
                OpenOutcome::Ready {
                    scope,
                    target,
                    file,
                } => (scope, target, file),
                OpenOutcome::Refused(outcome) => return outcome,
            };
        let session = ctx.session_id.to_string();
        let mutation_lock = self.mutation_lock(&scope.path);
        let _guard = mutation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.freshness.check_stale(&session, &scope.path).is_some() {
            return stale_conflict();
        }
        let Some(mut file) = file else {
            return failed(
                ErrorCode::NotFound,
                format!("Could not edit file: {raw_path}. Error code: ENOENT."),
            );
        };
        let identity = match FileIdentity::of(&file) {
            Ok(identity) => identity,
            Err(err) => {
                return failed(
                    ErrorCode::UpstreamUnavailable,
                    format!(
                        "Could not edit file: {raw_path}. Error code: {}.",
                        err.kind()
                    ),
                );
            }
        };
        let mut raw_bytes = Vec::new();
        if let Err(err) = file.read_to_end(&mut raw_bytes) {
            return failed(
                ErrorCode::UpstreamUnavailable,
                format!(
                    "Could not edit file: {raw_path}. Error code: {}.",
                    err.kind()
                ),
            );
        }
        let before_digest = hex(&Sha256::digest(&raw_bytes));
        // BOM strip for matching (restored on write); CRLF detected,
        // normalized for matching, restored on write.
        let (bom, body) = split_bom(&raw_bytes);
        let raw_content = match std::str::from_utf8(body) {
            Ok(text) => text,
            Err(_) => {
                return failed(
                    ErrorCode::InvalidMessage,
                    format!(
                        "Could not edit file: {raw_path}. The file is not valid UTF-8 text; \
                         binary files cannot be edited through the text edit tool."
                    ),
                );
            }
        };
        let original_ending = detect_line_ending(raw_content);
        let normalized_content = normalize_to_lf(raw_content);
        let new_content = match apply_edits(&normalized_content, &edits) {
            Ok(new_content) => new_content,
            Err(err) => {
                return failed(
                    ErrorCode::InvalidMessage,
                    err.message(&raw_path, edits.len()),
                );
            }
        };
        let final_text = restore_line_endings(&new_content, original_ending);
        let mut final_bytes = Vec::with_capacity(bom.len() + final_text.len());
        final_bytes.extend_from_slice(bom);
        final_bytes.extend_from_slice(final_text.as_bytes());
        self.finish_mutation(
            ctx,
            &raw_path,
            &scope,
            &target,
            true,
            Some((identity, Some(before_digest))),
            &final_bytes,
            format!(
                "Successfully replaced {} block(s) in {raw_path}.",
                edits.len()
            ),
        )
    }

    // ── the shared atomic swap ──────────────────────────────────────────────

    /// Writes `new_bytes` through a same-directory temp file and ONE
    /// atomic rename. When the target existed, the path is re-opened
    /// through the same dirfd right before the rename and must still be
    /// the identity that was read — an external replacement or
    /// modification in the window is a conflict, never a lost update.
    /// The temp file is removed on EVERY failure path.
    #[allow(clippy::too_many_arguments)]
    fn finish_mutation(
        &self,
        ctx: &RunContext,
        raw_path: &str,
        scope: &ResourceScope,
        target: &OpenTarget,
        existed: bool,
        before: Option<(FileIdentity, Option<String>)>,
        new_bytes: &[u8],
        success_text: String,
    ) -> ToolOutcome {
        let session = ctx.session_id.to_string();
        let random_hex = crate::auth::hex_random_public(8).unwrap_or_else(|_| {
            // Entropy failure must not become a GUESSABLE temp name: fall
            // back to a per-process monotonic counter (still unique
            // within the process; collisions resolved by O_EXCL retry).
            use std::sync::atomic::{AtomicU64, Ordering};
            static FALLBACK: AtomicU64 = AtomicU64::new(0);
            format!("fallback{:016x}", FALLBACK.fetch_add(1, Ordering::Relaxed))
        });
        let (mut temp_file, temp_name) = match target.dir.create_temp(&random_hex) {
            Ok(created) => created,
            Err(err) => {
                return failed(
                    ErrorCode::UpstreamUnavailable,
                    format!(
                        "Could not write the temporary file next to {raw_path}. Error code: {} \
                         (the original file is untouched).",
                        err.kind()
                    ),
                );
            }
        };
        let mut temp_guard = TempGuard {
            dir: &target.dir,
            name: temp_name.clone(),
            armed: true,
        };
        let mut write_error: Option<ToolOutcome> = None;
        if let Err(err) = temp_file.write_all(new_bytes) {
            write_error = Some(failed(
                ErrorCode::UpstreamUnavailable,
                format!(
                    "Could not write file: {raw_path}. Error code: {} (the original file is \
                     untouched).",
                    err.kind()
                ),
            ));
        }
        if write_error.is_none() {
            if let Err(err) = temp_file.sync_all() {
                write_error = Some(failed(
                    ErrorCode::UpstreamUnavailable,
                    format!(
                        "Could not flush file: {raw_path}. Error code: {} (the original file is \
                         untouched).",
                        err.kind()
                    ),
                ));
            }
        }
        // Preserve the original permission bits across the replace.
        if write_error.is_none() {
            if let Some((before_identity, _)) = &before {
                DirFd::apply_mode(&temp_file, before_identity.mode);
            }
        }
        drop(temp_file);
        if let Some(outcome) = write_error {
            return outcome; // the guard unlinks the temp file
        }
        // Test-only seam: fires inside the mutation window (after the
        // temp write, before the identity re-verification).
        if let Some(hook) = self
            .mutation_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            hook(&scope.path);
        }
        // Pre-rename re-verification through the SAME dirfd: the path
        // must still be the identity that was read.
        let mut existed_now = existed;
        if let Some(bound_identity) = before.as_ref() {
            match open_final(target) {
                FinalOpen::Exists(current) => match FileIdentity::of(&current) {
                    Ok(current_identity) => {
                        if identity_matches(&current_identity, bound_identity) {
                            existed_now = true;
                        } else {
                            return failed(
                                ErrorCode::Conflict,
                                format!(
                                    "{FILE_CHANGED_DURING_MUTATION}: {raw_path} was modified or \
                                     replaced by another writer between the read and the write; \
                                     the current content is kept and nothing was overwritten. \
                                     Re-read the file and retry."
                                ),
                            );
                        }
                    }
                    Err(err) => {
                        return failed(
                            ErrorCode::UpstreamUnavailable,
                            format!(
                                "Could not verify {raw_path} before replacing it. Error code: \
                                 {} (the original file is untouched).",
                                err.kind()
                            ),
                        );
                    }
                },
                FinalOpen::Swap => {
                    return failed(
                        ErrorCode::Conflict,
                        format!(
                            "{FILE_CHANGED_DURING_MUTATION}: {raw_path} became a symlink while \
                             the write was in flight; refusing to replace through it (the link \
                             target is untouched)."
                        ),
                    );
                }
                FinalOpen::Error(err) => {
                    return failed(
                        ErrorCode::UpstreamUnavailable,
                        format!(
                            "Could not verify {raw_path} before replacing it. Error code: {} \
                             (the original file is untouched).",
                            err.kind()
                        ),
                    );
                }
                FinalOpen::Missing => {
                    // Deleted in the window: creating it back is a normal
                    // outcome (the incumbent's last-writer-wins write).
                    existed_now = false;
                }
            }
        }
        if let Err(err) = target.dir.rename_within(&temp_name, &target.name) {
            return failed(
                ErrorCode::UpstreamUnavailable,
                format!(
                    "Could not replace file: {raw_path}. Error code: {} (the original file is \
                     untouched).",
                    err.kind()
                ),
            );
        }
        // The rename consumed the temp name — disarm the cleanup guard so
        // its Drop can never unlink a REUSED temp name.
        temp_guard.disarm();
        drop(temp_guard);
        target.dir.sync();
        // The after-version comes from the path we just replaced — no
        // success product is minted without verified existence.
        let Some(after) = FileFingerprint::stat(&scope.path) else {
            return failed(
                ErrorCode::Internal,
                format!(
                    "Wrote {raw_path} but could not stat the result afterwards; treat this as \
                     an UNKNOWN outcome and re-check the file (no success product is minted)"
                ),
            );
        };
        let after_version = FileVersion {
            size_bytes: after.size,
            sha256_hex: Some(hex(&Sha256::digest(new_bytes))),
            mtime_unix_ms: system_time_ms(after.modified),
        };
        self.changes.record(FileModificationRecord {
            path: scope.path.clone(),
            operation: if existed_now {
                FileChangeOperation::Modified
            } else {
                FileChangeOperation::Created
            },
            before: before.map(|(identity, digest)| FileVersion {
                size_bytes: identity.size,
                sha256_hex: digest,
                mtime_unix_ms: system_time_ms(identity.modified),
            }),
            after: after_version.clone(),
            at_unix_ms: self.clock.now_unix_ms(),
        });
        self.freshness
            .observe_mutation(&session, &scope.path, after);
        let resource_ref = ResourceRef {
            resource_id: ResourceId::new(format!("file:{}", scope.path.display())),
            kind: ResourceKind::Artifact,
            display_name: scope
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned()),
            uri: Some(format!("file://{}", scope.path.display())),
            digest: after_version.sha256_hex.map(|hex| ContentDigest {
                algorithm: "sha256".to_string(),
                canonicalization: "raw-bytes".to_string(),
                hex,
            }),
            size_bytes: Some(after_version.size_bytes),
        };
        text_success(success_text, false, vec![resource_ref])
    }
}

/// Identity comparison across the platform shapes (unix compares
/// device+inode+mtime+size; windows compares mtime+size — the weaker
/// registered form).
#[cfg(unix)]
fn identity_matches(current: &FileIdentity, before: &(FileIdentity, Option<String>)) -> bool {
    let (before_identity, _) = before;
    current.dev == before_identity.dev
        && current.ino == before_identity.ino
        && current.modified == before_identity.modified
        && current.size == before_identity.size
}

#[cfg(windows)]
fn identity_matches(current: &FileIdentity, before: &(FileIdentity, Option<String>)) -> bool {
    let (before_identity, _) = before;
    current.modified == before_identity.modified && current.size == before_identity.size
}

fn churn_refusal(raw_path: &str, churn: usize) -> ToolOutcome {
    failed(
        ErrorCode::Forbidden,
        format!(
            "{RESOURCE_SYMLINK_CHURN}: the path {raw_path:?} kept resolving through moving \
             symlinks; refusing after {churn} re-resolutions (zero side effects)"
        ),
    )
}

/// Renders the text-read result (pagination + truncation + continuation
/// notices — the incumbent `read.js` text path).
fn render_text_read(text: &str, offset: Option<i64>, limit: Option<i64>) -> ToolOutcome {
    let all_lines: Vec<&str> = text.split('\n').collect();
    let total_file_lines = all_lines.len();
    let start_line = offset.map(|o| (o.max(1) as usize) - 1).unwrap_or(0);
    let start_line_display = start_line + 1;
    if start_line >= all_lines.len() {
        return failed(
            ErrorCode::InvalidMessage,
            format!(
                "Offset {} is beyond end of file ({total_file_lines} lines total)",
                offset.unwrap_or(1)
            ),
        );
    }
    let selected: String;
    let mut user_limited_lines: Option<usize> = None;
    if let Some(limit) = limit {
        let limit = (limit.max(1) as usize).min(all_lines.len() - start_line);
        selected = all_lines[start_line..start_line + limit].join("\n");
        user_limited_lines = Some(limit);
    } else {
        selected = all_lines[start_line..].join("\n");
    }
    let truncation = truncate_head(&selected, READ_MAX_LINES, READ_MAX_BYTES);
    let output_text = if truncation.first_line_exceeds {
        let first_line_size = format_size(all_lines[start_line].len());
        format!(
            "[Line {start_line_display} is {first_line_size}, exceeds {} limit. This single line \
             cannot be served through read; use an offset past it (byte-oriented reads arrive \
             with the process tools).]",
            format_size(READ_MAX_BYTES)
        )
    } else if truncation.truncated {
        let end_line_display = start_line_display + truncation.output_lines - 1;
        let next_offset = end_line_display + 1;
        let suffix = if truncation.truncated_by == Some("lines") {
            format!(
                "\n\n[Showing lines {start_line_display}-{end_line_display} of \
                 {total_file_lines}. Use offset={next_offset} to continue.]"
            )
        } else {
            format!(
                "\n\n[Showing lines {start_line_display}-{end_line_display} of \
                 {total_file_lines} ({} limit). Use offset={next_offset} to continue.]",
                format_size(READ_MAX_BYTES)
            )
        };
        format!("{}{suffix}", truncation.content)
    } else if let Some(user_limit) = user_limited_lines {
        if start_line + user_limit < all_lines.len() {
            let remaining = all_lines.len() - (start_line + user_limit);
            let next_offset = start_line + user_limit + 1;
            format!(
                "{}\n\n[{remaining} more lines in file. Use offset={next_offset} to continue.]",
                truncation.content
            )
        } else {
            truncation.content
        }
    } else {
        truncation.content
    };
    text_success(output_text, truncation.truncated, Vec::new())
}

fn system_time_ms(time: std::time::SystemTime) -> u64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

enum TextDecode {
    Text(String),
    Binary,
}

/// UTF-8 with BOM strip (the incumbent `decodeBuffer`'s valid-UTF-8
/// path); anything else is binary (the GBK/office legs are registered
/// as not migrated).
fn decode_text(bytes: &[u8]) -> TextDecode {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return match std::str::from_utf8(&bytes[3..]) {
            Ok(text) => TextDecode::Text(text.to_string()),
            Err(_) => TextDecode::Binary,
        };
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => TextDecode::Text(text.to_string()),
        Err(_) => TextDecode::Binary,
    }
}

/// Splits a UTF-8 BOM off the raw bytes (`splitBom`).
fn split_bom(bytes: &[u8]) -> (&[u8], &[u8]) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (&bytes[..3], &bytes[3..])
    } else {
        (&[], bytes)
    }
}

// ── registration (the real resident core file tools) ────────────────────────

/// The registered identities of the native file tools.
#[derive(Debug, Clone)]
pub struct CoreFileTools {
    pub read_target: ToolTargetId,
    pub write_target: ToolTargetId,
    pub edit_target: ToolTargetId,
    pub tools: Arc<FileTools>,
}

fn file_manifest(
    local_name: &str,
    kind: lingxi_kernel::toolcatalog::PermissionKind,
    schema: serde_json::Value,
) -> ToolManifest {
    use lingxi_kernel::invocation::ToolRecoveryCapability;
    use lingxi_kernel::toolcatalog::{DeclaredPermission, PermissionContract, ToolOrigin};
    ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: local_name.to_string(),
        display_name: local_name.to_string(),
        aliases: Vec::new(),
        version: "1.0.0".to_string(),
        description: format!(
            "Native Rust {local_name} tool (R04-T04): the incumbent pi-coding-agent {local_name} \
             semantics behind the unified gateway"
        ),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema,
        },
        output_schema: None,
        permission: PermissionContract {
            kind,
            capability_base: format!("{local_name}.files"),
        },
        availability: lingxi_kernel::toolcatalog::Availability::Available,
        timeout_ms: Some(60_000),
        max_concurrency: None,
        declared_permission: match kind {
            lingxi_kernel::toolcatalog::PermissionKind::Read => DeclaredPermission::ReadOnly,
            _ => DeclaredPermission::Execute,
        },
        recovery: ToolRecoveryCapability::CONSERVATIVE,
    }
}

/// Registers the three REAL file tools on the registry and binds their
/// executors (with resource-scope derivation) on the gateway. This is
/// the composition entry the Rust stack calls when the native file
/// tools are enabled — the production default bootstrap does NOT call
/// it yet (no production default changes in R04).
pub fn register_core_file_tools(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    clock: Arc<dyn ServiceClock>,
    changes: Arc<dyn FileChangeLog>,
    budget: &SchemaBudget,
) -> CoreFileTools {
    let tools = Arc::new(FileTools::new(
        Arc::clone(&access),
        cwd.clone(),
        clock,
        changes,
    ));
    let read_manifest = file_manifest(
        "read",
        lingxi_kernel::toolcatalog::PermissionKind::Read,
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1},
            },
            "required": ["path"],
            "additionalProperties": false,
        }),
    );
    let write_manifest = file_manifest(
        "write",
        lingxi_kernel::toolcatalog::PermissionKind::Execute,
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "content": {"type": "string"},
            },
            "required": ["path", "content"],
            "additionalProperties": false,
        }),
    );
    let edit_manifest = file_manifest(
        "edit",
        lingxi_kernel::toolcatalog::PermissionKind::Execute,
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "oldText": {"type": "string"},
                            "newText": {"type": "string"},
                        },
                        "required": ["oldText", "newText"],
                        "additionalProperties": false,
                    },
                },
            },
            "required": ["path", "edits"],
            "additionalProperties": false,
        }),
    );
    let read_target = registry
        .register(read_manifest, budget)
        .expect("the resident read tool registers")
        .target_id;
    let write_target = registry
        .register(write_manifest, budget)
        .expect("the resident write tool registers")
        .target_id;
    let edit_target = registry
        .register(edit_manifest, budget)
        .expect("the resident edit tool registers")
        .target_id;
    gateway.bind_executor_with_resources(
        read_target.clone(),
        Arc::new(CoreFileExecutor::new(
            Arc::clone(&tools),
            FileToolKind::Read,
        )),
        Some(FileTools::resource_extractor(
            Arc::clone(&access),
            cwd.clone(),
            ResourceOp::Read,
        )),
        "R04-T04 native read executor (real filesystem)",
    );
    gateway.bind_executor_with_resources(
        write_target.clone(),
        Arc::new(CoreFileExecutor::new(
            Arc::clone(&tools),
            FileToolKind::Write,
        )),
        Some(FileTools::resource_extractor(
            Arc::clone(&access),
            cwd.clone(),
            ResourceOp::Write,
        )),
        "R04-T04 native write executor (atomic temp+rename, freshness conflicts)",
    );
    gateway.bind_executor_with_resources(
        edit_target.clone(),
        Arc::new(CoreFileExecutor::new(
            Arc::clone(&tools),
            FileToolKind::Edit,
        )),
        Some(FileTools::resource_extractor(
            Arc::clone(&access),
            cwd,
            ResourceOp::Write,
        )),
        "R04-T04 native edit executor (exact/fuzzy match, CRLF/BOM preserved)",
    );
    CoreFileTools {
        read_target,
        write_target,
        edit_target,
        tools,
    }
}

/// One executor bound to one target (read / write / edit). Dispatches
/// through the T02 gateway ONLY — the executor is never callable from
/// business code directly (boundary check B1).
pub struct CoreFileExecutor {
    tools: Arc<FileTools>,
    kind: FileToolKind,
}

impl CoreFileExecutor {
    pub fn new(tools: Arc<FileTools>, kind: FileToolKind) -> Self {
        Self { tools, kind }
    }
}

impl ToolExecutorPort for CoreFileExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        // Blocking filesystem work runs on the blocking pool; everything
        // the closure needs is cloned out of the borrowed inputs. If the
        // surrounding task is dropped at the await point, an in-flight
        // mutation completes detached — the journal's
        // `started`-without-receipt window classifies recovery as
        // UNKNOWN (the R03 contract), never as success.
        let tools = Arc::clone(&self.tools);
        let kind = self.kind;
        let ctx = ctx.clone();
        let args = request.arguments.as_value().clone();
        Box::pin(async move {
            let work_ctx = ctx.clone();
            let outcome = tokio::task::spawn_blocking(move || match kind {
                FileToolKind::Read => tools.run_read(&work_ctx, &args),
                FileToolKind::Write => tools.run_write(&work_ctx, &args),
                FileToolKind::Edit => tools.run_edit(&work_ctx, &args),
            })
            .await
            .unwrap_or_else(|join_err| {
                failed(
                    ErrorCode::Internal,
                    format!("file executor worker panicked: {join_err}"),
                )
            });
            ToolExecutionResult::of_ctx(&ctx, outcome)
        })
    }
}
