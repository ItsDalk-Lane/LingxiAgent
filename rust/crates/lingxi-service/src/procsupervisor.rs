//! The real process supervisor of the native command/PTY tools (R04-T05).
//!
//! Owns every child process the Rust tool surface spawns: one-shot piped
//! commands and PTY-backed persistent terminals. The design rules come
//! from the stage book (R04-T05 怎么做 3/4 and the master prompt §4.4):
//!
//! # Admission before dispatch (R04-RR1-F01)
//! A live-registry slot is reserved ATOMICALLY (one lock-held
//! check-and-increment — not a lock-free pre-check, which would be a
//! TOCTOU) BEFORE any OS resource is created or any process dispatched.
//! A full registry therefore refuses with ZERO dispatch. From reservation
//! to reaper takeover the slot is owned by exactly one [`LiveSlot`]
//! guard: every failure path (spawn error, group-verification failure,
//! PTY initialization failure, future drop before registration) drops
//! the guard, which returns the slot exactly once; a successful spawn
//! COMMITS the guard into the registry, transferring the release duty to
//! the record's settle path (itself exactly-once). A step that can only
//! fail AFTER the dispatch (process-group verification) keeps the
//! dispatched fact in its error and performs a bounded kill + reap of
//! the child we own — never a silent "nothing happened".
//!
//! # Ownership identity, never a bare PID
//! Every spawn is registered under a CSPRNG handle (`proc:<32 hex>`).
//! Termination accepts ONLY a registered handle whose record is still
//! non-terminal — while a record is live the child is either running or
//! an unreaped zombie, so its PID cannot have been recycled; after the
//! reaper observes the exit the record goes terminal and `terminate`
//! refuses instead of firing `killpg` at what might now be someone
//! else's process group. The pgid used by `killpg` is VERIFIED right
//! after spawn (`getpgid(child) == child` — every spawn `setsid`s into
//! its own group), so a group we kill is provably a group we created.
//! Nothing outside this supervisor's spawn path can be adopted, so a
//! leftover foreign process can never be mistaken for a managed one.
//!
//! # The bounded cleanup responsibility chain
//! `terminate(handle, reason)` runs: mark `Terminating` → `killpg(pgid,
//! SIGKILL)` → bounded wait for the exit observation (`cleanup_timeout`)
//! → reaper-side reclamation (bounded stdio grace, pipe/master closure,
//! spill-file closure) → terminal record + audit receipt. If the bound
//! expires the record honestly says `CleanupTimedOut` (the kill was
//! sent; the exit was not observed) — never a fabricated success.
//!
//! # Future-drop does not shed ownership
//! The calling future (the gateway executor inside the run driver) may
//! be dropped at any await point by the run's cancellation tree. A Drop
//! guard in the tool layer initiates [`Self::terminate_detached`]:
//! `killpg` fires synchronously and the BOUNDED TAIL (wait + reclaim +
//! receipt) runs on a supervisor-owned task, so the cleanup
//! responsibility survives the dropped future. This is NOT tokio's
//! `kill_on_drop` (which only signals the direct child and only when
//! the `Child` itself drops): `kill_on_drop` stays disabled here and is
//! never the cleanup guarantee.
//!
//! # Lifetime registration (persistent terminals)
//! A PTY terminal is registered as `PersistentTerminal` with its owner
//! (principal + session + creating run). Its lifetime deliberately
//! SPANS tool calls: a run being cancelled does not kill it (the tool
//! call that started it already returned "running"); it dies on the
//! explicit close path or on supervisor shutdown (`shutdown_all` — the
//! frozen application-exit policy: terminate EVERYTHING managed, each
//! bounded). Leftover foreign processes can never be mistaken for
//! persistent terminals because adoption does not exist.
//!
//! # Platform scope (honest)
//! The POSIX process-group mechanism (`setsid`/`killpg`/`getpgid`) is
//! implemented and machine-verified on macOS. Non-Unix builds fail
//! CLOSED with `SpawnFailure::UnsupportedPlatform` — no silent degrade
//! and no invented Windows Job Object behavior (the Windows form stays
//! the stage's registered platform deferral).
//!
//! # Output bounds (both families)
//! One-shot collectors and PTY transcripts are bounded in memory
//! (rolling window / ring) with full-output SPILL to a supervisor-owned
//! directory, itself capped (`spill_cap_bytes`) — an output storm fills
//! the spill to its cap and is reported as capped, never unbounded.
//! UTF-8 chunk boundaries are handled by joining bytes before decoding
//! and holding back an incomplete trailing sequence until more bytes
//! arrive (a multibyte character split across two PTY reads is
//! delivered whole, never mangled).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{self, Write as _};
use std::os::fd::{AsRawFd as _, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_protocol::ToolCallId;

use crate::inject::ServiceClock;

/// Hard cap of concurrently LIVE (non-terminal) managed processes. A full
/// registry refuses new spawns loudly — never an unbounded backlog.
pub const DEFAULT_LIVE_PROCESS_CAP: usize = 64;
/// Bounded ring of settled records kept for receipts/diagnostics.
pub const DEFAULT_SETTLED_RING_CAP: usize = 1024;
/// Bounded wait for the exit observation after `killpg`.
pub const DEFAULT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);
/// Bounded stdio grace after the direct child exits (grandchildren may
/// still hold the pipe write ends — the incumbent's `exitStdioGraceMs`).
pub const DEFAULT_STDIO_GRACE: Duration = Duration::from_millis(250);
/// Rolling in-memory window of one-shot output (the incumbent's
/// `MAX_ROLLING_BYTES` = 2 × the 50 KiB result budget).
pub const DEFAULT_OUTPUT_WINDOW_BYTES: usize = 100 * 1024;
/// PTY transcript ring bound.
pub const DEFAULT_TRANSCRIPT_RING_BYTES: usize = 256 * 1024;
/// Full-output spill cap (a storm stops at the cap and is reported).
pub const DEFAULT_SPILL_CAP_BYTES: u64 = 64 * 1024 * 1024;
/// Per-record audit bound.
const AUDIT_CAP: usize = 64;

// ── identity & facts ────────────────────────────────────────────────────────

/// Opaque process handle (CSPRNG; `proc:` + 32 hex chars). Model-visible in
/// tool results, but authorization is by the OWNER record, never by the
/// string alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessHandleId(Arc<str>);

impl ProcessHandleId {
    fn mint() -> Result<Self, getrandom::Error> {
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes)?;
        let mut hex = String::with_capacity(2 + bytes.len() * 2);
        hex.push_str("proc:");
        for b in bytes {
            hex.push_str(&format!("{b:02x}"));
        }
        Ok(Self(Arc::from(hex.as_str())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProcessHandleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl ProcessHandleId {
    /// Parses a model-supplied process id string. Only the exact minted
    /// format (`proc:` + 32 lowercase hex) parses; the REGISTRY lookup
    /// remains the authority — this is a format gate so a forged string
    /// never reaches the record map.
    pub fn parse(value: &str) -> Option<Self> {
        let rest = value.strip_prefix("proc:")?;
        if rest.len() != 32
            || !rest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        Some(Self(Arc::from(value)))
    }
}

/// The owner facts a process is bound to at spawn (from the trusted run
/// context — a model payload can never populate these).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOwner {
    pub principal_kind: String,
    pub principal_subject: String,
    pub session_id: String,
    pub run_id: String,
    /// The tool call that started this process (audit anchor).
    pub tool_call_id: ToolCallId,
}

/// The registered lifetime of one managed process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessKind {
    /// A one-shot piped command — lifetime bound to the tool call; the
    /// caller-future drop guard terminates it.
    OneShot,
    /// A PTY-backed persistent terminal — lifetime spans tool calls
    /// until the explicit close or supervisor shutdown.
    PersistentTerminal,
}

impl ProcessKind {
    pub fn wire_name(self) -> &'static str {
        match self {
            ProcessKind::OneShot => "one_shot",
            ProcessKind::PersistentTerminal => "persistent_terminal",
        }
    }
}

/// Why a termination was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationReason {
    /// The calling future was dropped (run cancelled / CALL-level abort).
    CallerDropped,
    /// The one-shot timeout watchdog fired.
    Timeout,
    /// The explicit close path (terminal closed by its owner).
    Close,
    /// Supervisor shutdown (the frozen application-exit policy).
    Shutdown,
}

impl TerminationReason {
    pub fn wire_name(self) -> &'static str {
        match self {
            TerminationReason::CallerDropped => "caller_dropped",
            TerminationReason::Timeout => "timeout",
            TerminationReason::Close => "close",
            TerminationReason::Shutdown => "shutdown",
        }
    }
}

/// How the direct child ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitFact {
    Code(i32),
    Signal(i32),
}

impl ExitFact {
    /// Shell-convention numeric status (`128 + signal` for signals) — the
    /// value surfaced in `ToolRunStatus::Exited`.
    pub fn status_code(self) -> i64 {
        match self {
            ExitFact::Code(code) => code as i64,
            ExitFact::Signal(sig) => 128 + sig as i64,
        }
    }

    pub fn describe(self) -> String {
        match self {
            ExitFact::Code(code) => format!("exit code {code}"),
            ExitFact::Signal(sig) => format!("signal {sig}"),
        }
    }
}

/// The phase of one managed process (broadcast to waiters via watch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordPhase {
    Running,
    Terminating {
        reason: TerminationReason,
    },
    /// Natural exit observed.
    Exited {
        fact: ExitFact,
    },
    /// Termination completed: kill sent, exit observed, resources reclaimed.
    Terminated {
        reason: TerminationReason,
        fact: ExitFact,
    },
    /// Kill sent but the exit was NOT observed within the cleanup bound.
    CleanupTimedOut {
        reason: TerminationReason,
    },
}

impl RecordPhase {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            RecordPhase::Exited { .. }
                | RecordPhase::Terminated { .. }
                | RecordPhase::CleanupTimedOut { .. }
        )
    }
}

/// Why a spawn was refused.
#[derive(Debug)]
pub enum SpawnFailure {
    /// The command line was structurally unusable.
    Invalid(String),
    /// `spawn(2)` failed (missing binary, bad cwd, resource limits).
    Io {
        message: String,
        cwd_hint: Option<String>,
    },
    /// The live-process registry is full.
    RegistryFull,
    /// Process-group ownership could not be established/verified.
    GroupOwnership(String),
    /// CSPRNG failure — no predictable handle is ever minted.
    Entropy,
    /// Non-Unix build: no invented platform mechanism.
    UnsupportedPlatform,
}

impl std::fmt::Display for SpawnFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpawnFailure::Invalid(detail) => write!(f, "invalid spawn request: {detail}"),
            SpawnFailure::Io { message, cwd_hint } => match cwd_hint {
                Some(hint) => write!(f, "{message}. {hint}"),
                None => write!(f, "{message}"),
            },
            SpawnFailure::RegistryFull => write!(
                f,
                "the managed live-process registry is full; refusing the spawn instead of \
                 running unmanaged"
            ),
            SpawnFailure::GroupOwnership(detail) => {
                write!(
                    f,
                    "process-group ownership could not be established: {detail}"
                )
            }
            SpawnFailure::Entropy => write!(
                f,
                "the system CSPRNG refused to mint a process handle; the spawn is refused"
            ),
            SpawnFailure::UnsupportedPlatform => write!(
                f,
                "the native process supervisor only implements the POSIX process-group \
                 mechanism on Unix builds; this platform is unsupported and the spawn is \
                 refused rather than run unmanaged"
            ),
        }
    }
}

/// Verification fault injection (R04-RR1-F01-C03): forces the NEXT spawn
/// to fail at a controlled boundary of the reserve → dispatch → register
/// responsibility chain, so tests can prove the compensation (slot
/// returned exactly once, dispatched fact recorded, created resources
/// reclaimed) without waiting for a rare organic failure of the same
/// branch. The hook only chooses WHEN the branch fires — the rollback it
/// exercises is the one a real failure takes. It is NEVER reachable from
/// model or tool input: only process-local Rust code holding the
/// supervisor can arm it, and no product path does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpawnFaultPoint {
    /// No fault armed (the production default).
    #[default]
    None,
    /// The next PTY spawn fails at `open_pty_pair` — the pre-dispatch
    /// initialization boundary (zero OS dispatch; the slot must return
    /// and no pty fd may leak).
    PtyPairOpen,
    /// The next spawn (either kind) fails the post-dispatch
    /// process-group verification — the dispatched-then-failed boundary
    /// (bounded kill + reap of the really-dispatched child, slot
    /// return, dispatched fact recorded in the error).
    GroupVerify,
}

/// Appends the honest dispatched-fact to a post-dispatch rollback error:
/// the process DID start and was SIGKILLed and reaped by the spawn
/// rollback (R04-RR1-F01 repair requirement 3 — never pretend a
/// dispatched process never existed).
fn note_dispatched_rollback(failure: &mut SpawnFailure, pid: i32) {
    match failure {
        SpawnFailure::GroupOwnership(detail) => {
            *detail = format!(
                "{detail}; the already-dispatched child (pid {pid}) was SIGKILLed and \
                 reaped by the spawn rollback — the dispatch DID happen"
            );
        }
        other => {
            // No other post-dispatch failure exists today; keep the arm
            // total so a future variant cannot silently lose the fact.
            *other = SpawnFailure::Invalid(format!(
                "{other}; the already-dispatched child (pid {pid}) was SIGKILLed and \
                 reaped by the spawn rollback — the dispatch DID happen"
            ));
        }
    }
}

/// Knobs of the supervisor (all bounded).
#[derive(Debug, Clone)]
pub struct SupervisorLimits {
    pub live_cap: usize,
    pub settled_ring_cap: usize,
    pub cleanup_timeout: Duration,
    pub stdio_grace: Duration,
    pub output_window_bytes: usize,
    pub transcript_ring_bytes: usize,
    pub spill_cap_bytes: u64,
    /// Supervisor-owned directory for full-output spill references.
    pub spill_dir: PathBuf,
}

impl SupervisorLimits {
    pub fn with_spill_dir(spill_dir: PathBuf) -> Self {
        Self {
            live_cap: DEFAULT_LIVE_PROCESS_CAP,
            settled_ring_cap: DEFAULT_SETTLED_RING_CAP,
            cleanup_timeout: DEFAULT_CLEANUP_TIMEOUT,
            stdio_grace: DEFAULT_STDIO_GRACE,
            output_window_bytes: DEFAULT_OUTPUT_WINDOW_BYTES,
            transcript_ring_bytes: DEFAULT_TRANSCRIPT_RING_BYTES,
            spill_cap_bytes: DEFAULT_SPILL_CAP_BYTES,
            spill_dir,
        }
    }
}

/// One spawn request.
pub struct SpawnSpec {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub owner: ProcessOwner,
    pub kind: ProcessKind,
    pub cols: u16,
    pub rows: u16,
}

impl SpawnSpec {
    pub fn validate(&self) -> Result<(), SpawnFailure> {
        if self.argv.is_empty() || self.argv.iter().any(|a| a.is_empty()) {
            return Err(SpawnFailure::Invalid(
                "argv must be a non-empty program with non-empty arguments".to_string(),
            ));
        }
        if !(1..=1024).contains(&self.cols) || !(1..=1024).contains(&self.rows) {
            return Err(SpawnFailure::Invalid(
                "pty dimensions must be within 1..=1024".to_string(),
            ));
        }
        Ok(())
    }
}

/// The spawn result handed back to the tool layer.
#[derive(Debug, Clone)]
pub struct SpawnedProcess {
    pub id: ProcessHandleId,
    pub pid: i32,
    pub pgid: i32,
}

/// The receipt of one termination attempt — the accurate statement of what
/// actually happened (the "收据准确" evidence of R04-A09).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminationOutcome {
    /// The handle is unknown (never minted, or already evicted).
    UnknownProcess,
    /// The process was already terminal — nothing was killed.
    AlreadyTerminal(RecordPhase),
    /// Kill sent, exit observed within the bound, resources reclaimed.
    Terminated {
        fact: ExitFact,
        reclaimed: bool,
        drained_stdio: bool,
    },
    /// Kill sent; the exit was not observed within the cleanup bound.
    CleanupTimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminationReceipt {
    pub id: ProcessHandleId,
    pub pid: i32,
    pub pgid: i32,
    pub kind: ProcessKind,
    pub reason: TerminationReason,
    pub outcome: TerminationOutcome,
}

/// Snapshot of one record (audit-facing view).
#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub id: ProcessHandleId,
    pub pid: i32,
    pub pgid: i32,
    pub kind: ProcessKind,
    pub owner: ProcessOwner,
    pub argv: Vec<String>,
    pub phase: RecordPhase,
    pub spawned_at_unix_ms: u64,
    pub audit: Vec<AuditEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub at_unix_ms: u64,
    pub kind: &'static str,
    pub detail: String,
}

// ── bounded output plumbing ────────────────────────────────────────────────

/// Spill reference surfaced in results (the 落盘引用).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpillInfo {
    pub path: PathBuf,
    pub bytes_written: u64,
    pub capped: bool,
}

/// Full-output spill writer (bounded; supervisor-owned directory).
struct SpillWriter {
    file: Option<std::fs::File>,
    path: PathBuf,
    written: u64,
    capped: bool,
    cap: u64,
}

impl SpillWriter {
    fn open(dir: &Path, cap: u64, seed: [u8; 8], prefix: &str) -> Option<Self> {
        let hex: String = seed.iter().map(|b| format!("{b:02x}")).collect();
        let path = dir.join(format!("{prefix}-{hex}.log"));
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .ok()?;
        Some(Self {
            file: Some(file),
            path,
            written: 0,
            capped: false,
            cap,
        })
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        if self.capped {
            return;
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let room = self.cap.saturating_sub(self.written);
        if room == 0 {
            self.capped = true;
            let _ = file.sync_all();
            self.file = None;
            return;
        }
        let take = bytes.len().min(room as usize);
        match file.write_all(&bytes[..take]) {
            Ok(()) => {
                self.written += take as u64;
                if (take as u64) == room {
                    self.capped = true;
                    let _ = file.sync_all();
                    self.file = None;
                }
            }
            Err(_) => {
                // Spill failure degrades to the bounded window only — the
                // process keeps running; the result reports no live spill
                // writer (recorded as capped, never retried silently).
                self.capped = true;
                self.file = None;
            }
        }
    }

    fn close(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = file.sync_all();
        }
    }

    fn info(&self) -> Option<SpillInfo> {
        // A spill that never wrote a byte is not a reference worth surfacing.
        Some(SpillInfo {
            path: self.path.clone(),
            bytes_written: self.written,
            capped: self.capped,
        })
    }
}

/// Which stream a chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// The bounded one-shot output collector (the incumbent
/// `CommandOutputCollector`): rolling window + counters + bounded spill.
struct CollectorCore {
    window: VecDeque<u8>,
    window_cap: usize,
    total_bytes: u64,
    stdout_bytes: u64,
    stderr_bytes: u64,
    spill: Option<SpillWriter>,
}

impl CollectorCore {
    fn append(&mut self, bytes: &[u8], stream: OutputStream) {
        let n = bytes.len() as u64;
        self.total_bytes += n;
        match stream {
            OutputStream::Stdout => self.stdout_bytes += n,
            OutputStream::Stderr => self.stderr_bytes += n,
        }
        if let Some(spill) = self.spill.as_mut() {
            spill.write_bytes(bytes);
        }
        for b in bytes {
            self.window.push_back(*b);
        }
        while self.window.len() > self.window_cap {
            self.window.pop_front();
        }
    }

    fn close_spill(&mut self) {
        if let Some(spill) = self.spill.as_mut() {
            spill.close();
        }
    }
}

/// The immutable result-facing view of a one-shot collector.
#[derive(Debug, Clone)]
pub struct OutputSnapshot {
    pub window: Vec<u8>,
    pub total_bytes: u64,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub spill: Option<SpillInfo>,
}

/// The delivered view of a PTY read.
#[derive(Debug, Clone)]
pub struct TranscriptDelivery {
    pub text: String,
    pub last_seq: u64,
    pub truncated: bool,
    pub dropped_undelivered_bytes: u64,
    pub total_bytes: u64,
    pub spill: Option<SpillInfo>,
}

/// The bounded PTY transcript: chunk ring + seq cursor + spill, with
/// UTF-8-boundary-safe delivery. A trailing INCOMPLETE sequence stays in
/// its chunk (the cursor stops before it): the next delivery re-joins
/// that chunk whole, so split multibyte characters are delivered intact
/// and bytes are never duplicated or dropped mid-character.
struct TranscriptCore {
    chunks: VecDeque<(u64, Vec<u8>)>,
    ring_bytes: usize,
    ring_cap: usize,
    /// Seq of the next chunk (starts at 1; cursor 0 = nothing delivered).
    next_seq: u64,
    /// Delivered up to (inclusive) this seq.
    cursor_seq: u64,
    dropped_undelivered_bytes: u64,
    total_bytes: u64,
    spill: Option<SpillWriter>,
}

impl TranscriptCore {
    fn append(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.total_bytes += bytes.len() as u64;
        if let Some(spill) = self.spill.as_mut() {
            spill.write_bytes(bytes);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.ring_bytes += bytes.len();
        self.chunks.push_back((seq, bytes.to_vec()));
        while self.ring_bytes > self.ring_cap {
            // Evict the oldest chunk. If it is UNDELIVERED data the loss is
            // recorded honestly (dropped_undelivered_bytes).
            let Some((seq, evicted)) = self.chunks.pop_front() else {
                break;
            };
            self.ring_bytes -= evicted.len();
            if seq > self.cursor_seq {
                self.dropped_undelivered_bytes += evicted.len() as u64;
            }
        }
    }

    /// Delivers everything after the cursor, advancing the cursor only
    /// across chunks whose bytes end on a COMPLETE UTF-8 boundary. With
    /// `force` (the terminal is terminal — no more bytes can arrive) the
    /// remainder is lossy-delivered instead of held back.
    fn deliver_since_cursor(&mut self, force: bool) -> TranscriptDelivery {
        let undelivered: Vec<(u64, usize)> = self
            .chunks
            .iter()
            .filter(|(seq, _)| *seq > self.cursor_seq)
            .map(|(seq, bytes)| (*seq, bytes.len()))
            .collect();
        let mut joined: Vec<u8> = Vec::new();
        for (seq, _) in &undelivered {
            if let Some((_, bytes)) = self.chunks.iter().find(|(s, _)| s == seq) {
                joined.extend_from_slice(bytes);
            }
        }
        let dropped = self.dropped_undelivered_bytes;
        let total = self.total_bytes;
        let spill = self.spill.as_ref().and_then(|s| s.info());
        if joined.is_empty() {
            return TranscriptDelivery {
                text: String::new(),
                last_seq: self.cursor_seq,
                truncated: dropped > 0,
                dropped_undelivered_bytes: dropped,
                total_bytes: total,
                spill,
            };
        }
        let (text, consumed) = if force {
            (String::from_utf8_lossy(&joined).into_owned(), joined.len())
        } else {
            decode_prefix_utf8(&joined)
        };
        // Advance the cursor across every chunk whose bytes are fully
        // consumed (byte-offset accounting — a partially consumed chunk
        // stays wholly undelivered and is re-joined next time).
        let mut offset = 0usize;
        let mut new_cursor = self.cursor_seq;
        for (seq, len) in &undelivered {
            if offset + len <= consumed {
                offset += len;
                new_cursor = *seq;
            } else {
                break;
            }
        }
        self.cursor_seq = new_cursor;
        TranscriptDelivery {
            text,
            last_seq: self.cursor_seq,
            truncated: dropped > 0 || consumed < joined.len(),
            dropped_undelivered_bytes: dropped + (joined.len() - consumed) as u64,
            total_bytes: total,
            spill,
        }
    }

    fn close_spill(&mut self) {
        if let Some(spill) = self.spill.as_mut() {
            spill.close();
        }
    }
}

/// Decodes `bytes` returning the text and how many bytes were consumed. A
/// trailing INCOMPLETE UTF-8 sequence is not consumed (held back by the
/// caller); interior invalid bytes become replacement characters and are
/// consumed (never held, never dropped silently).
fn decode_prefix_utf8(bytes: &[u8]) -> (String, usize) {
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), bytes.len()),
        Err(err) => {
            let valid = err.valid_up_to();
            if err.error_len().is_none() {
                // Trailing incomplete sequence: deliver the valid prefix.
                let head = String::from_utf8_lossy(&bytes[..valid]).into_owned();
                (head, valid)
            } else {
                // Interior invalid bytes: lossy-decode everything.
                (String::from_utf8_lossy(bytes).into_owned(), bytes.len())
            }
        }
    }
}

// ── the record ──────────────────────────────────────────────────────────────

struct RecordInner {
    phase: RecordPhase,
    audit: Vec<AuditEvent>,
    settled: bool,
    /// One-shot collector (shared with the pump tasks).
    collector: Option<Arc<Mutex<CollectorCore>>>,
    /// PTY transcript.
    transcript: Option<Arc<Mutex<TranscriptCore>>>,
    /// Whether both stdio pumps observed EOF before the grace expired.
    drained_stdio: bool,
    /// Whether the reaper finished reclaiming (pipes/master/spill).
    reclaimed: bool,
    pty_master: Option<Arc<PtyMasterHandle>>,
}

struct ProcessRecord {
    id: ProcessHandleId,
    owner: ProcessOwner,
    kind: ProcessKind,
    argv: Vec<String>,
    pid: i32,
    pgid: i32,
    spawned_at_unix_ms: u64,
    inner: Mutex<RecordInner>,
    phase_tx: tokio::sync::watch::Sender<RecordPhase>,
}

fn lock_or_poison<T>(lock: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl ProcessRecord {
    fn snapshot(&self) -> ProcessSnapshot {
        let inner = lock_or_poison(&self.inner);
        ProcessSnapshot {
            id: self.id.clone(),
            pid: self.pid,
            pgid: self.pgid,
            kind: self.kind,
            owner: self.owner.clone(),
            argv: self.argv.clone(),
            phase: inner.phase.clone(),
            spawned_at_unix_ms: self.spawned_at_unix_ms,
            audit: inner.audit.clone(),
        }
    }

    fn audit(&self, at_unix_ms: u64, kind: &'static str, detail: String) {
        let mut inner = lock_or_poison(&self.inner);
        if inner.audit.len() >= AUDIT_CAP {
            inner.audit.remove(0);
        }
        inner.audit.push(AuditEvent {
            at_unix_ms,
            kind,
            detail,
        });
    }
}

// ── the supervisor ─────────────────────────────────────────────────────────

#[derive(Default)]
struct SupervisorState {
    records: HashMap<ProcessHandleId, Arc<ProcessRecord>>,
    settled: VecDeque<ProcessHandleId>,
    live_count: usize,
}

/// The real process supervisor (see the module docs). Interior-mutable,
/// `Send + Sync`, owned by the composition root that registers the
/// native process tools.
pub struct ProcessSupervisor {
    state: Mutex<SupervisorState>,
    clock: Arc<dyn ServiceClock>,
    limits: SupervisorLimits,
    /// One-shot verification fault (see [`SpawnFaultPoint`]); consumed by
    /// the next spawn. Production code never arms it.
    fault: Mutex<SpawnFaultPoint>,
}

/// A live-registry slot reserved ATOMICALLY before any OS dispatch
/// (R04-RR1-F01: the old order dispatched the process first and checked
/// the cap after, so a full registry leaked a running, unowned child).
///
/// The slot is accounted in `live_count` from reservation until the
/// record settles. Exactly-once release: an ARMED guard returns the slot
/// on drop (every failure path of the spawn — IO, group verification,
/// PTY initialization, a panic mid-spawn), while [`LiveSlot::commit`]
/// disarms the guard as it installs the record — the destructor of a
/// committed guard releases nothing, and the record's `settle` path
/// (itself exactly-once via the `settled` flag) owns the release from
/// there. A slot can therefore never be returned twice.
struct LiveSlot {
    supervisor: Arc<ProcessSupervisor>,
    armed: bool,
}

impl LiveSlot {
    /// Installs a fully-initialized record and DISARMS the guard: from
    /// here the record's settle path owns the release duty (the reaper
    /// always settles, so a committed slot never leaks).
    fn commit(mut self, record: Arc<ProcessRecord>) {
        let mut state = lock_or_poison(&self.supervisor.state);
        state.records.insert(record.id.clone(), record);
        self.armed = false;
    }
}

impl Drop for LiveSlot {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut state = lock_or_poison(&self.supervisor.state);
        state.live_count = state.live_count.saturating_sub(1);
    }
}

impl ProcessSupervisor {
    /// Builds the supervisor with a supervisor-owned spill directory
    /// (created; a failure to create it is loud — spill references are a
    /// result feature, not a silent no-op).
    pub fn new(clock: Arc<dyn ServiceClock>, limits: SupervisorLimits) -> io::Result<Self> {
        std::fs::create_dir_all(&limits.spill_dir).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!(
                    "process supervisor spill directory {} cannot be created: {err}",
                    limits.spill_dir.display()
                ),
            )
        })?;
        Ok(Self {
            state: Mutex::new(SupervisorState::default()),
            clock,
            limits,
            fault: Mutex::new(SpawnFaultPoint::None),
        })
    }

    /// Arms a one-shot verification fault (see [`SpawnFaultPoint`]).
    /// Verification-only API: no product or model-reachable path calls it.
    pub fn arm_spawn_fault_for_verification(&self, point: SpawnFaultPoint) {
        *lock_or_poison(&self.fault) = point;
    }

    fn take_spawn_fault(&self) -> SpawnFaultPoint {
        std::mem::replace(&mut *lock_or_poison(&self.fault), SpawnFaultPoint::None)
    }

    fn now_ms(&self) -> u64 {
        self.clock.now_unix_ms()
    }

    fn mint_handle(&self) -> Result<ProcessHandleId, SpawnFailure> {
        ProcessHandleId::mint().map_err(|_| SpawnFailure::Entropy)
    }

    fn spill_seed(&self) -> [u8; 8] {
        let mut seed = [0u8; 8];
        let _ = getrandom::getrandom(&mut seed);
        seed
    }

    /// Atomically reserves one live-registry slot BEFORE any OS dispatch.
    /// The cap check and the increment are one critical section — two
    /// concurrent spawns can never both take the last slot (a lock-free
    /// pre-check outside the lock would be a TOCTOU, not admission
    /// control). A reserved slot counts as live immediately: it is
    /// unavailable to other spawns for the whole reserve → dispatch →
    /// register window.
    fn reserve_live_slot(self: &Arc<Self>) -> Result<LiveSlot, SpawnFailure> {
        let mut state = lock_or_poison(&self.state);
        if state.live_count >= self.limits.live_cap {
            return Err(SpawnFailure::RegistryFull);
        }
        state.live_count += 1;
        Ok(LiveSlot {
            supervisor: Arc::clone(self),
            armed: true,
        })
    }

    /// Exactly-once settle: moves the record into the bounded settled ring
    /// and decrements the live count on the first call only.
    fn settle(self: &Arc<Self>, id: &ProcessHandleId) {
        let mut state = lock_or_poison(&self.state);
        let first = match state.records.get(id) {
            Some(record) => {
                let mut inner = lock_or_poison(&record.inner);
                if inner.settled {
                    false
                } else {
                    inner.settled = true;
                    true
                }
            }
            None => false,
        };
        if !first {
            return;
        }
        state.live_count = state.live_count.saturating_sub(1);
        state.settled.push_back(id.clone());
        while state.settled.len() > self.limits.settled_ring_cap {
            if let Some(evict) = state.settled.pop_front() {
                state.records.remove(&evict);
            }
        }
    }

    fn any_record(&self, id: &ProcessHandleId) -> Option<Arc<ProcessRecord>> {
        lock_or_poison(&self.state).records.get(id).cloned()
    }

    // ── spawn (POSIX process-group ownership) ──────────────────────────────

    /// Spawns one managed process (one-shot piped or PTY terminal).
    #[cfg(unix)]
    pub async fn spawn(self: &Arc<Self>, spec: SpawnSpec) -> Result<SpawnedProcess, SpawnFailure> {
        spec.validate()?;
        let id = self.mint_handle()?;
        let now = self.now_ms();
        match spec.kind {
            ProcessKind::OneShot => self.clone().spawn_oneshot(spec, id, now).await,
            ProcessKind::PersistentTerminal => self.clone().spawn_pty(spec, id, now).await,
        }
    }

    #[cfg(not(unix))]
    pub async fn spawn(self: &Arc<Self>, _spec: SpawnSpec) -> Result<SpawnedProcess, SpawnFailure> {
        Err(SpawnFailure::UnsupportedPlatform)
    }

    #[cfg(unix)]
    async fn spawn_oneshot(
        self: Arc<Self>,
        spec: SpawnSpec,
        id: ProcessHandleId,
        now: u64,
    ) -> Result<SpawnedProcess, SpawnFailure> {
        // R04-RR1-F01: the capacity slot is reserved BEFORE any OS
        // dispatch — a full registry refuses with zero processes created.
        // Every failure below drops the LiveSlot guard, which returns the
        // slot exactly once.
        let slot = self.reserve_live_slot()?;
        let fault = self.take_spawn_fault();
        let mut command = tokio::process::Command::new(&spec.argv[0]);
        command
            .args(&spec.argv[1..])
            .current_dir(&spec.cwd)
            .env_clear()
            .envs(&spec.env)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(false);
        // Every spawn becomes its own session/process group: the group we
        // later killpg is provably the group we created.
        // SAFETY: setsid in pre_exec is async-signal-safe.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let cwd_missing = !spec.cwd.exists();
        let mut child = command.spawn().map_err(|err| SpawnFailure::Io {
            message: format!("spawning {:?} failed: {err}", spec.argv[0]),
            cwd_hint: cwd_missing.then(|| {
                format!(
                    "Likely cause: working directory does not exist: {}. The executable \
                     path may be fine.",
                    spec.cwd.display()
                )
            }),
        })?;
        let pid = child.id().expect("tokio child id") as i32;
        if let Err(mut failure) = self.verify_group(pid, &spec, fault) {
            // A post-dispatch failure: the child REALLY started, so the
            // rollback is a bounded kill + reap of the child we own and
            // the error keeps the dispatched fact — never a silent
            // "nothing happened", and never a bare kill_on_drop handoff.
            // SAFETY: kill our own just-spawned child.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            let _ = child.wait().await;
            note_dispatched_rollback(&mut failure, pid);
            return Err(failure); // the LiveSlot drops: slot returned once
        }
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let spill = SpillWriter::open(
            &self.limits.spill_dir,
            self.limits.spill_cap_bytes,
            self.spill_seed(),
            "lingxi-exec-output",
        );
        let collector = Arc::new(Mutex::new(CollectorCore {
            window: VecDeque::new(),
            window_cap: self.limits.output_window_bytes,
            total_bytes: 0,
            stdout_bytes: 0,
            stderr_bytes: 0,
            spill,
        }));
        let record = Arc::new(ProcessRecord {
            id: id.clone(),
            owner: spec.owner.clone(),
            kind: ProcessKind::OneShot,
            argv: spec.argv.clone(),
            pid,
            pgid: pid,
            spawned_at_unix_ms: now,
            inner: Mutex::new(RecordInner {
                phase: RecordPhase::Running,
                audit: vec![AuditEvent {
                    at_unix_ms: now,
                    kind: "spawned",
                    detail: format!(
                        "one-shot pid={pid} pgid={pid} argv={:?} cwd={}",
                        spec.argv,
                        spec.cwd.display()
                    ),
                }],
                settled: false,
                collector: Some(Arc::clone(&collector)),
                transcript: None,
                drained_stdio: false,
                reclaimed: false,
                pty_master: None,
            }),
            phase_tx: tokio::sync::watch::Sender::new(RecordPhase::Running),
        });
        // Commit: the record is fully initialized and the reaper is about
        // to take over — the slot's release duty transfers to the settle
        // path. From registration on, no spawn-side failure exists.
        slot.commit(Arc::clone(&record));
        // The reaper owns the wait + the bounded stdio grace + reclaim +
        // terminal recording + settle. It is a supervisor-owned task: its
        // life is independent of any caller future.
        let grace = self.limits.stdio_grace;
        let supervisor = Arc::clone(&self);
        let reaper_record = Arc::clone(&record);
        tokio::spawn(async move {
            let out_pump = stdout.map(|stream| {
                let collector = Arc::clone(&collector);
                tokio::spawn(pump_stream(stream, collector, OutputStream::Stdout))
            });
            let err_pump = stderr.map(|stream| {
                let collector = Arc::clone(&collector);
                tokio::spawn(pump_stream(stream, collector, OutputStream::Stderr))
            });
            let status = child.wait().await;
            // Bounded stdio grace: grandchildren may still hold the write
            // ends. After the grace, leftover pump tasks are aborted —
            // dropping our read ends closes the pipes (reclaim).
            let mut drained = true;
            for pump in [out_pump, err_pump].into_iter().flatten() {
                if tokio::time::timeout(grace, pump).await.is_err() {
                    drained = false;
                }
            }
            let fact = match status {
                Ok(status) => exit_fact_of(&status),
                Err(_) => ExitFact::Code(-1),
            };
            let leftover_group = group_has_members(reaper_record.pgid);
            finalize_exit(&reaper_record, fact, drained, leftover_group);
            supervisor.settle(&reaper_record.id);
        });
        Ok(SpawnedProcess { id, pid, pgid: pid })
    }

    #[cfg(unix)]
    fn verify_group(
        &self,
        pid: i32,
        spec: &SpawnSpec,
        fault: SpawnFaultPoint,
    ) -> Result<(), SpawnFailure> {
        // SAFETY: getpgid is a plain syscall.
        let pgid = unsafe { libc::getpgid(pid) };
        if pgid == pid && fault == SpawnFaultPoint::GroupVerify {
            // Verification-only fault: the REAL check passed, but the
            // armed hook forces this branch so the dispatched-then-failed
            // rollback (kill + reap + slot return + dispatched fact) is
            // observable without waiting for a silent setsid failure.
            return Err(SpawnFailure::GroupOwnership(format!(
                "verification fault point armed: forcing the post-dispatch \
                 group-verification failure (getpgid({pid}) actually returned {pgid} — the \
                 group was fine; the fault exercises the rollback)"
            )));
        }
        if pgid != pid {
            return Err(SpawnFailure::GroupOwnership(format!(
                "getpgid({pid}) returned {pgid}, expected the child to lead its own group \
                 (setsid failed silently?); argv={:?}",
                spec.argv
            )));
        }
        Ok(())
    }

    #[cfg(unix)]
    async fn spawn_pty(
        self: Arc<Self>,
        spec: SpawnSpec,
        id: ProcessHandleId,
        now: u64,
    ) -> Result<SpawnedProcess, SpawnFailure> {
        // R04-RR1-F01: the capacity slot is reserved BEFORE the pty pair
        // is opened and before any OS dispatch — a full registry refuses
        // with zero resources created. Every failure below drops the
        // LiveSlot guard (exactly-once release) and every fd this path
        // opened is closed by its owner (OwnedFd drop / explicit drop).
        let slot = self.reserve_live_slot()?;
        let fault = self.take_spawn_fault();
        if fault == SpawnFaultPoint::PtyPairOpen {
            // Verification-only fault: exercises the pre-dispatch
            // initialization failure (no pty was opened; zero dispatch).
            return Err(SpawnFailure::Io {
                message: "verification fault point armed: forcing the pty-pair open \
                           failure (no pty was opened; zero dispatch)"
                    .to_string(),
                cwd_hint: None,
            });
        }
        let (master_fd, slave_fd) = open_pty_pair().map_err(|err| SpawnFailure::Io {
            message: format!("opening a pty pair failed: {err}"),
            cwd_hint: None,
        })?;
        let slave_raw = slave_fd.as_raw_fd();
        let mut command = tokio::process::Command::new(&spec.argv[0]);
        command
            .args(&spec.argv[1..])
            .current_dir(&spec.cwd)
            .env_clear()
            .envs(&spec.env)
            .stdin(dup_cloexec(slave_raw).map_err(spawn_err)?)
            .stdout(dup_cloexec(slave_raw).map_err(spawn_err)?)
            .stderr(dup_cloexec(slave_raw).map_err(spawn_err)?)
            .kill_on_drop(false);
        // setsid + TIOCSCTTY: the child becomes a session leader with the
        // pty slave as its controlling terminal (node-pty's wiring — this
        // is what makes \x03 INTR and SIGWINCH reach the foreground group).
        // SAFETY: both calls are async-signal-safe.
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(
                    slave_raw,
                    libc::TIOCSCTTY as libc::c_ulong,
                    0 as libc::c_int,
                ) == -1
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let cwd_missing = !spec.cwd.exists();
        let mut child = command.spawn().map_err(|err| SpawnFailure::Io {
            message: format!("spawning {:?} failed: {err}", spec.argv[0]),
            cwd_hint: cwd_missing.then(|| {
                format!(
                    "Likely cause: working directory does not exist: {}. The executable \
                     path may be fine.",
                    spec.cwd.display()
                )
            }),
        })?;
        // The parent's copies of the slave close now (the child holds its
        // dup2'd copies; keeping ours would wedge the pty open forever).
        drop(slave_fd);
        let pid = child.id().expect("tokio child id") as i32;
        if let Err(mut failure) = self.verify_group(pid, &spec, fault) {
            // Post-dispatch failure: kill AND reap the child we really
            // dispatched, close the master we really opened (explicit —
            // never rely on "closing the pty happens to signal the
            // child" as the cleanup), keep the dispatched fact, return
            // the slot exactly once.
            // SAFETY: kill our own just-spawned child.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            let _ = child.wait().await;
            drop(master_fd); // close our own pty master fd
            note_dispatched_rollback(&mut failure, pid);
            return Err(failure); // the LiveSlot drops: slot returned once
        }
        let master = Arc::new(PtyMasterHandle {
            fd: Mutex::new(Some(master_fd)),
        });
        set_pty_size(master.raw_fd(), spec.cols, spec.rows);
        let spill = SpillWriter::open(
            &self.limits.spill_dir,
            self.limits.spill_cap_bytes,
            self.spill_seed(),
            "lingxi-pty-transcript",
        );
        let transcript = Arc::new(Mutex::new(TranscriptCore {
            chunks: VecDeque::new(),
            ring_bytes: 0,
            ring_cap: self.limits.transcript_ring_bytes,
            next_seq: 1,
            cursor_seq: 0,
            dropped_undelivered_bytes: 0,
            total_bytes: 0,
            spill,
        }));
        let record = Arc::new(ProcessRecord {
            id: id.clone(),
            owner: spec.owner.clone(),
            kind: ProcessKind::PersistentTerminal,
            argv: spec.argv.clone(),
            pid,
            pgid: pid,
            spawned_at_unix_ms: now,
            inner: Mutex::new(RecordInner {
                phase: RecordPhase::Running,
                audit: vec![AuditEvent {
                    at_unix_ms: now,
                    kind: "spawned",
                    detail: format!(
                        "pty terminal pid={pid} pgid={pid} argv={:?} cwd={} cols={} rows={}",
                        spec.argv,
                        spec.cwd.display(),
                        spec.cols,
                        spec.rows
                    ),
                }],
                settled: false,
                collector: None,
                transcript: Some(Arc::clone(&transcript)),
                drained_stdio: false,
                reclaimed: false,
                pty_master: Some(Arc::clone(&master)),
            }),
            phase_tx: tokio::sync::watch::Sender::new(RecordPhase::Running),
        });
        // Commit: the record is fully initialized; the reader and the
        // reaper are about to take over. The slot's release duty
        // transfers to the settle path.
        slot.commit(Arc::clone(&record));
        // The master reader appends pty output into the transcript until
        // the master closes (child exit → EIO/EOF, or our reclaim).
        {
            let reader_master = Arc::clone(&master);
            let reader_transcript = Arc::clone(&transcript);
            tokio::spawn(async move {
                pty_read_loop(reader_master, reader_transcript).await;
            });
        }
        // The pty reaper: wait + terminal recording + settle.
        {
            let supervisor = Arc::clone(&self);
            let reaper_record = Arc::clone(&record);
            tokio::spawn(async move {
                let status = child.wait().await;
                let fact = match status {
                    Ok(status) => exit_fact_of(&status),
                    Err(_) => ExitFact::Code(-1),
                };
                let leftover_group = group_has_members(reaper_record.pgid);
                finalize_exit(&reaper_record, fact, true, leftover_group);
                supervisor.settle(&reaper_record.id);
            });
        }
        Ok(SpawnedProcess { id, pid, pgid: pid })
    }

    // ── queries ─────────────────────────────────────────────────────────────

    /// Snapshot of one record (live or recently settled).
    pub fn record(&self, id: &ProcessHandleId) -> Option<ProcessSnapshot> {
        self.any_record(id).map(|record| record.snapshot())
    }

    /// The handle ids of every non-terminal record.
    pub fn live_handles(&self) -> Vec<ProcessHandleId> {
        let state = lock_or_poison(&self.state);
        state
            .records
            .values()
            .filter(|record| {
                let inner = lock_or_poison(&record.inner);
                !inner.phase.is_terminal()
            })
            .map(|record| record.id.clone())
            .collect()
    }

    /// The one-shot output snapshot (window + counters + spill info).
    pub fn output_snapshot(&self, id: &ProcessHandleId) -> Option<OutputSnapshot> {
        let record = self.any_record(id)?;
        let inner = lock_or_poison(&record.inner);
        let collector = inner.collector.as_ref()?;
        let core = lock_or_poison(collector);
        Some(OutputSnapshot {
            window: core.window.iter().copied().collect(),
            total_bytes: core.total_bytes,
            stdout_bytes: core.stdout_bytes,
            stderr_bytes: core.stderr_bytes,
            spill: core.spill.as_ref().map(|spill| SpillInfo {
                path: spill.path.clone(),
                bytes_written: spill.written,
                capped: spill.capped,
            }),
        })
    }

    /// Waits (unbounded — the tool-layer timeout watchdog bounds it) for a
    /// terminal phase.
    pub async fn wait_terminal(&self, id: &ProcessHandleId) -> Option<RecordPhase> {
        let record = self.any_record(id)?;
        let mut rx = record.phase_tx.subscribe();
        loop {
            let phase = rx.borrow().clone();
            if phase.is_terminal() {
                return Some(phase);
            }
            if rx.changed().await.is_err() {
                return None;
            }
        }
    }

    // ── PTY surface ─────────────────────────────────────────────────────────

    /// Writes bytes to a persistent terminal's master.
    #[cfg(unix)]
    pub async fn pty_write(&self, id: &ProcessHandleId, bytes: &[u8]) -> Result<(), String> {
        let record = self
            .any_record(id)
            .ok_or_else(|| "unknown process".to_string())?;
        let master = {
            let inner = lock_or_poison(&record.inner);
            if !matches!(record.kind, ProcessKind::PersistentTerminal) {
                return Err("process is not an interactive terminal".to_string());
            }
            if !matches!(inner.phase, RecordPhase::Running) {
                return Err("terminal is not running".to_string());
            }
            inner
                .pty_master
                .clone()
                .ok_or_else(|| "no pty master".to_string())?
        };
        pty_write_all(master, bytes)
            .await
            .map_err(|err| format!("pty write failed: {err}"))
    }

    /// Resizes a persistent terminal (TIOCSWINSZ on the master).
    #[cfg(unix)]
    pub fn pty_resize(&self, id: &ProcessHandleId, cols: u16, rows: u16) -> Result<(), String> {
        let record = self
            .any_record(id)
            .ok_or_else(|| "unknown process".to_string())?;
        let inner = lock_or_poison(&record.inner);
        if !matches!(record.kind, ProcessKind::PersistentTerminal) {
            return Err("process is not an interactive terminal".to_string());
        }
        if !matches!(inner.phase, RecordPhase::Running) {
            return Err("terminal is not running".to_string());
        }
        let master = inner
            .pty_master
            .clone()
            .ok_or_else(|| "no pty master".to_string())?;
        drop(inner);
        set_pty_size(master.raw_fd(), cols, rows);
        record.audit(self.now_ms(), "resized", format!("cols={cols} rows={rows}"));
        Ok(())
    }

    /// Delivers transcript output accumulated since the last delivery.
    /// Once the record is TERMINAL the delivery is forced (lossy on any
    /// dangling partial sequence — no bytes can arrive anymore, so the
    /// honest end state is a replacement character, not silence).
    pub async fn pty_deliver(&self, id: &ProcessHandleId) -> Option<TranscriptDelivery> {
        let record = self.any_record(id)?;
        let transcript = {
            let inner = lock_or_poison(&record.inner);
            inner.transcript.clone()?
        };
        let force = {
            let inner = lock_or_poison(&record.inner);
            inner.phase.is_terminal()
        };
        let mut core = lock_or_poison(&transcript);
        Some(core.deliver_since_cursor(force))
    }

    /// The terminal's phase snapshot (for write_stdin status reporting).
    pub fn phase_of(&self, id: &ProcessHandleId) -> Option<RecordPhase> {
        self.any_record(id)
            .map(|record| lock_or_poison(&record.inner).phase.clone())
    }

    // ── the bounded cleanup chain ───────────────────────────────────────────

    /// Terminates one managed process: mark → `killpg(SIGKILL)` → bounded
    /// wait for the exit observation → reaper reclaims (bounded stdio
    /// grace, pipe/master closure, spill closure) → receipt.
    pub async fn terminate(
        self: &Arc<Self>,
        id: &ProcessHandleId,
        reason: TerminationReason,
    ) -> TerminationReceipt {
        let Some(record) = self.any_record(id) else {
            return TerminationReceipt {
                id: id.clone(),
                pid: 0,
                pgid: 0,
                kind: ProcessKind::OneShot,
                reason,
                outcome: TerminationOutcome::UnknownProcess,
            };
        };
        let started = mark_terminating(&record, reason, self.now_ms());
        if !started {
            let phase = lock_or_poison(&record.inner).phase.clone();
            return TerminationReceipt {
                id: record.id.clone(),
                pid: record.pid,
                pgid: record.pgid,
                kind: record.kind,
                reason,
                outcome: TerminationOutcome::AlreadyTerminal(phase),
            };
        }
        let outcome = self.await_termination(&record, reason).await;
        TerminationReceipt {
            id: record.id.clone(),
            pid: record.pid,
            pgid: record.pgid,
            kind: record.kind,
            reason,
            outcome,
        }
    }

    /// Drop-guard entry: `killpg` fires synchronously; the bounded tail
    /// runs on a supervisor-owned task so it survives the dropped future.
    pub fn terminate_detached(self: &Arc<Self>, id: &ProcessHandleId, reason: TerminationReason) {
        let Some(record) = self.any_record(id) else {
            return;
        };
        if !mark_terminating(&record, reason, self.now_ms()) {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let supervisor = Arc::clone(self);
                let record = Arc::clone(&record);
                handle.spawn(async move {
                    supervisor.await_termination(&record, reason).await;
                });
            }
            Err(_) => {
                // The kill WAS sent synchronously; the reaper still records
                // the exit and settles. Only this tail's timeout
                // bookkeeping is unavailable — recorded, never swallowed.
                record.audit(
                    self.now_ms(),
                    "detached_tail_no_runtime",
                    "killpg sent from a drop guard outside a runtime; the reaper still \
                     records the exit"
                        .to_string(),
                );
            }
        }
    }

    async fn await_termination(
        self: &Arc<Self>,
        record: &Arc<ProcessRecord>,
        reason: TerminationReason,
    ) -> TerminationOutcome {
        let deadline = tokio::time::Instant::now() + self.limits.cleanup_timeout;
        let mut rx = record.phase_tx.subscribe();
        let outcome = loop {
            let phase = rx.borrow().clone();
            if phase.is_terminal() {
                break Some(phase);
            }
            let changed = tokio::time::timeout_at(deadline, rx.changed()).await;
            match changed {
                Ok(Ok(())) => continue,
                Ok(Err(_)) | Err(_) => break None,
            }
        };
        let now = self.now_ms();
        match outcome {
            Some(phase) => {
                let (fact, reclaimed, drained) = terminal_facts(record);
                record.audit(
                    now,
                    "termination_settled",
                    format!(
                        "reason={} phase={:?} reclaimed={reclaimed} drained_stdio={drained}",
                        reason.wire_name(),
                        phase
                    ),
                );
                self.settle(&record.id);
                TerminationOutcome::Terminated {
                    fact,
                    reclaimed,
                    drained_stdio: drained,
                }
            }
            None => {
                // Bounded cleanup expired: state it honestly.
                {
                    let mut inner = lock_or_poison(&record.inner);
                    if !inner.phase.is_terminal() {
                        inner.phase = RecordPhase::CleanupTimedOut { reason };
                        let _ = record.phase_tx.send(inner.phase.clone());
                    }
                }
                record.audit(
                    now,
                    "cleanup_timed_out",
                    format!(
                        "killpg was sent but the exit was not observed within {:?}",
                        self.limits.cleanup_timeout
                    ),
                );
                // Reclaim what we can from this side (master/spill handles).
                reclaim_handles(record);
                TerminationOutcome::CleanupTimedOut
            }
        }
    }

    /// The frozen application-exit policy: terminate EVERYTHING still
    /// managed (one-shots and persistent terminals alike), each bounded.
    pub async fn shutdown_all(self: &Arc<Self>) -> Vec<TerminationReceipt> {
        let handles = self.live_handles();
        let mut receipts = Vec::with_capacity(handles.len());
        for id in handles {
            receipts.push(self.terminate(&id, TerminationReason::Shutdown).await);
        }
        receipts
    }
}

impl Drop for ProcessSupervisor {
    fn drop(&mut self) {
        // Best-effort synchronous kill of anything still live (the async
        // chain cannot run in Drop): the honest bounded chain is
        // `shutdown_all`, which composition roots must call before drop.
        let state = lock_or_poison(&self.state);
        for record in state.records.values() {
            let inner = lock_or_poison(&record.inner);
            if !inner.phase.is_terminal() {
                #[cfg(unix)]
                // SAFETY: killpg on a group this supervisor created and
                // whose record is still non-terminal (pid not reusable).
                unsafe {
                    libc::killpg(record.pgid, libc::SIGKILL);
                }
            }
        }
        // The spill directory is supervisor-owned (unique name, never the
        // shared /tmp itself): removing it never touches foreign files.
        let _ = std::fs::remove_dir_all(&self.limits.spill_dir);
    }
}

// ── phase helpers ───────────────────────────────────────────────────────────

/// Marks a record Terminating and fires `killpg` under the lock (the mark
/// and the kill are one atomic step — no window where two terminators
/// disagree). Returns whether a termination is in progress or was started
/// by this call; `false` means the record was already terminal.
fn mark_terminating(record: &Arc<ProcessRecord>, reason: TerminationReason, now: u64) -> bool {
    let mut inner = lock_or_poison(&record.inner);
    match inner.phase.clone() {
        RecordPhase::Running => {
            inner.phase = RecordPhase::Terminating { reason };
            let _ = record.phase_tx.send(inner.phase.clone());
            if inner.audit.len() >= AUDIT_CAP {
                inner.audit.remove(0);
            }
            inner.audit.push(AuditEvent {
                at_unix_ms: now,
                kind: "killpg_sent",
                detail: format!(
                    "killpg({}) SIGKILL reason={}",
                    record.pgid,
                    reason.wire_name()
                ),
            });
            #[cfg(unix)]
            // SAFETY: pgid was verified at spawn (child leads its own
            // group) and the record is non-terminal — the child (or its
            // unreaped zombie) still owns that pid, so the group cannot be
            // a recycled foreign one.
            unsafe {
                libc::killpg(record.pgid, libc::SIGKILL);
            }
            #[cfg(not(unix))]
            let _ = record.pgid;
            true
        }
        RecordPhase::Terminating { .. } => true,
        RecordPhase::Exited { .. }
        | RecordPhase::Terminated { .. }
        | RecordPhase::CleanupTimedOut { .. } => false,
    }
}

/// Records a terminal phase from the reaper (natural exit or post-kill),
/// performs the reclamation and broadcasts.
fn finalize_exit(
    record: &Arc<ProcessRecord>,
    fact: ExitFact,
    drained_stdio: bool,
    leftover_group: bool,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    {
        let mut inner = lock_or_poison(&record.inner);
        let reason = match inner.phase {
            RecordPhase::Terminating { reason } => Some(reason),
            _ => None,
        };
        inner.drained_stdio = drained_stdio;
        inner.phase = match reason {
            Some(reason) => RecordPhase::Terminated { reason, fact },
            None => RecordPhase::Exited { fact },
        };
        if inner.audit.len() >= AUDIT_CAP {
            inner.audit.remove(0);
        }
        inner.audit.push(AuditEvent {
            at_unix_ms: now,
            kind: "exit_observed",
            detail: format!(
                "{} drained_stdio={drained_stdio} leftover_group_after_exit={leftover_group}",
                fact.describe()
            ),
        });
        // Reclaim: close the pty master (one-shots have no master; their
        // pumps were joined/aborted by the reaper after the grace).
        if let Some(master) = inner.pty_master.take() {
            master.close();
        }
        if let Some(transcript) = inner.transcript.as_ref() {
            lock_or_poison(transcript).close_spill();
        }
        if let Some(collector) = inner.collector.as_ref() {
            lock_or_poison(collector).close_spill();
        }
        inner.reclaimed = true;
        let _ = record.phase_tx.send(inner.phase.clone());
    }
}

fn reclaim_handles(record: &Arc<ProcessRecord>) {
    let mut inner = lock_or_poison(&record.inner);
    if let Some(master) = inner.pty_master.take() {
        master.close();
    }
    if let Some(transcript) = inner.transcript.as_ref() {
        lock_or_poison(transcript).close_spill();
    }
    if let Some(collector) = inner.collector.as_ref() {
        lock_or_poison(collector).close_spill();
    }
}

fn terminal_facts(record: &Arc<ProcessRecord>) -> (ExitFact, bool, bool) {
    let inner = lock_or_poison(&record.inner);
    let fact = match inner.phase.clone() {
        RecordPhase::Exited { fact } | RecordPhase::Terminated { fact, .. } => fact,
        RecordPhase::CleanupTimedOut { .. } => ExitFact::Signal(9),
        _ => ExitFact::Code(-1),
    };
    (fact, inner.reclaimed, inner.drained_stdio)
}

#[cfg(unix)]
fn exit_fact_of(status: &std::process::ExitStatus) -> ExitFact {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        ExitFact::Code(code)
    } else if let Some(sig) = status.signal() {
        ExitFact::Signal(sig)
    } else {
        ExitFact::Code(-1)
    }
}

/// Probes whether the process group still has members (`kill(pgid, 0)`).
/// Used only for AUDIT (the honest note that a normally-exited command may
/// leave backgrounded grandchildren in its group — the incumbent's
/// documented behavior); never as a kill decision.
#[cfg(unix)]
fn group_has_members(pgid: i32) -> bool {
    // SAFETY: signal-0 probe on a group; no effect.
    unsafe { libc::killpg(pgid, 0) == 0 }
}

#[cfg(not(unix))]
fn group_has_members(_pgid: i32) -> bool {
    false
}

/// Pumps one piped stream into the collector until EOF.
#[cfg(unix)]
async fn pump_stream<R>(mut stream: R, collector: Arc<Mutex<CollectorCore>>, which: OutputStream)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                lock_or_poison(&collector).append(&buf[..n], which);
            }
            Err(_) => break,
        }
    }
}

// ── PTY plumbing (libc; no new dependencies) ───────────────────────────────

/// An open pty master (interior-mutable fd so close/reclaim can race the
/// reader task safely).
pub struct PtyMasterHandle {
    fd: Mutex<Option<OwnedFd>>,
}

impl PtyMasterHandle {
    pub fn raw_fd(&self) -> RawFd {
        lock_or_poison(&self.fd)
            .as_ref()
            .map(|fd| fd.as_raw_fd())
            .unwrap_or(-1)
    }

    pub fn is_open(&self) -> bool {
        lock_or_poison(&self.fd).is_some()
    }

    fn close(&self) {
        *lock_or_poison(&self.fd) = None;
    }

    fn try_clone_fd(&self) -> Option<OwnedFd> {
        lock_or_poison(&self.fd)
            .as_ref()
            .and_then(|fd| fd.try_clone().ok())
    }
}

fn spawn_err(err: io::Error) -> SpawnFailure {
    SpawnFailure::Io {
        message: format!("preparing pty stdio failed: {err}"),
        cwd_hint: None,
    }
}

/// Opens a pty pair (master + slave, both `O_CLOEXEC`).
#[cfg(unix)]
fn open_pty_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    use std::os::fd::FromRawFd;
    /// `ptsname(3)` writes into a static buffer (non-reentrant): all
    /// callers serialize through this lock (the portable-pty approach).
    static PTSNAME_LOCK: Mutex<()> = Mutex::new(());
    // SAFETY: plain libc pty allocation calls.
    unsafe {
        let master_fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        if master_fd == -1 {
            return Err(io::Error::last_os_error());
        }
        if libc::grantpt(master_fd) == -1 {
            let err = io::Error::last_os_error();
            libc::close(master_fd);
            return Err(err);
        }
        if libc::unlockpt(master_fd) == -1 {
            let err = io::Error::last_os_error();
            libc::close(master_fd);
            return Err(err);
        }
        // The master MUST be nonblocking: tokio's AsyncFd readiness loop
        // requires it (a blocking master would wedge a worker thread in
        // read(2) — the portable-pty rule).
        let flags = libc::fcntl(master_fd, libc::F_GETFL);
        if flags == -1 || libc::fcntl(master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            let err = io::Error::last_os_error();
            libc::close(master_fd);
            return Err(err);
        }
        let path = {
            let _guard = lock_or_poison(&PTSNAME_LOCK);
            let ptr = libc::ptsname(master_fd);
            if ptr.is_null() {
                let err = io::Error::last_os_error();
                libc::close(master_fd);
                return Err(err);
            }
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().to_string()
        };
        let slave_fd = libc::open(
            std::ffi::CString::new(path.as_bytes())?.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        );
        if slave_fd == -1 {
            let err = io::Error::last_os_error();
            libc::close(master_fd);
            return Err(err);
        }
        Ok((
            OwnedFd::from_raw_fd(master_fd),
            OwnedFd::from_raw_fd(slave_fd),
        ))
    }
}

/// Dups `fd` with `F_DUPFD_CLOEXEC` as a stdio slot (so the extra copy
/// does not leak past exec).
#[cfg(unix)]
fn dup_cloexec(fd: RawFd) -> io::Result<std::process::Stdio> {
    use std::os::fd::FromRawFd;
    // SAFETY: fcntl F_DUPFD_CLOEXEC on a live fd.
    let dup = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if dup == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the dup is a fresh owned fd.
    Ok(std::process::Stdio::from(unsafe {
        OwnedFd::from_raw_fd(dup)
    }))
}

#[cfg(unix)]
fn set_pty_size(fd: RawFd, cols: u16, rows: u16) {
    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }
    let ws = Winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if fd >= 0 {
        // SAFETY: TIOCSWINSZ with a winsize struct.
        unsafe {
            libc::ioctl(fd, libc::TIOCSWINSZ, &ws as *const Winsize);
        }
    }
}

#[cfg(unix)]
async fn pty_read_loop(master: Arc<PtyMasterHandle>, transcript: Arc<Mutex<TranscriptCore>>) {
    let Some(fd) = master.try_clone_fd() else {
        return;
    };
    let async_fd = match tokio::io::unix::AsyncFd::new(fd) {
        Ok(async_fd) => async_fd,
        Err(_) => return,
    };
    let mut buf = [0u8; 8192];
    loop {
        let mut guard = match async_fd.readable().await {
            Ok(guard) => guard,
            Err(_) => return,
        };
        match guard.try_io(|io| raw_read(io.get_ref().as_raw_fd(), &mut buf)) {
            Ok(Ok(0)) => return,
            Ok(Ok(n)) => {
                lock_or_poison(&transcript).append(&buf[..n]);
            }
            Ok(Err(err)) => {
                // macOS reports EIO once the slave side is fully closed.
                if err.kind() == io::ErrorKind::WouldBlock {
                    continue;
                }
                return;
            }
            Err(_would_block) => continue,
        }
    }
}

/// Raw `read(2)` on a nonblocking pty master fd (AsyncFd provides the
/// readiness; the syscall itself is ours).
#[cfg(unix)]
fn raw_read(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    // SAFETY: read into a live buffer of the given length.
    let n = unsafe {
        libc::read(
            fd,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as libc::size_t,
        )
    };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

/// Raw `write(2)` on a nonblocking pty master fd.
#[cfg(unix)]
fn raw_write(fd: RawFd, buf: &[u8]) -> io::Result<usize> {
    // SAFETY: write from a live buffer of the given length.
    let n = unsafe {
        libc::write(
            fd,
            buf.as_ptr() as *const libc::c_void,
            buf.len() as libc::size_t,
        )
    };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

#[cfg(unix)]
async fn pty_write_all(master: Arc<PtyMasterHandle>, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let Some(fd) = master.try_clone_fd() else {
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "pty master closed",
        ));
    };
    let async_fd = tokio::io::unix::AsyncFd::new(fd)?;
    let mut written = 0usize;
    while written < bytes.len() {
        let mut guard = async_fd.writable().await?;
        match guard.try_io(|io| raw_write(io.get_ref().as_raw_fd(), &bytes[written..])) {
            Ok(Ok(n)) => written += n,
            Ok(Err(err)) if err.kind() == io::ErrorKind::WouldBlock => continue,
            Ok(Err(err)) => return Err(err),
            Err(_would_block) => continue,
        }
    }
    Ok(())
}

// ── unit tests (deterministic, no processes) ───────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_handles_parse_and_forged_ones_do_not() {
        let minted = ProcessHandleId::mint().expect("CSPRNG mints");
        assert!(minted.as_str().starts_with("proc:"));
        assert_eq!(minted.as_str().len(), 5 + 32);
        let parsed = ProcessHandleId::parse(minted.as_str()).expect("minted parses");
        assert_eq!(parsed, minted);
        // Forged forms never parse.
        assert!(ProcessHandleId::parse("proc:deadbeef").is_none());
        assert!(ProcessHandleId::parse("garbage").is_none());
        assert!(ProcessHandleId::parse("xproc:0000").is_none());
        // Wrong length.
        assert!(ProcessHandleId::parse(&format!("proc:{}", "a".repeat(31))).is_none());
        assert!(ProcessHandleId::parse(&format!("proc:{}", "a".repeat(33))).is_none());
        // Non-lowercase hex is not the minted form.
        assert!(
            ProcessHandleId::parse(&format!("proc:{}", "A".repeat(32))).is_none(),
            "uppercase hex never parses"
        );
    }

    fn transcript(ring_cap: usize) -> TranscriptCore {
        TranscriptCore {
            chunks: VecDeque::new(),
            ring_bytes: 0,
            ring_cap,
            next_seq: 1,
            cursor_seq: 0,
            dropped_undelivered_bytes: 0,
            total_bytes: 0,
            spill: None,
        }
    }

    #[test]
    fn transcript_delivers_split_multibyte_characters_intact() {
        // "日" = E6 97 A5, split across two appends.
        let mut core = transcript(1024);
        core.append(b"a");
        core.append(&[0xE6, 0x97]);
        let first = core.deliver_since_cursor(false);
        assert_eq!(first.text, "a");
        assert!(
            first.truncated,
            "an incomplete trailing sequence is honestly flagged"
        );
        core.append(&[0xA5]); // completes 日
        let second = core.deliver_since_cursor(false);
        assert_eq!(second.text, "日", "no duplication, no mangling");
        let third = core.deliver_since_cursor(false);
        assert_eq!(third.text, "", "cursor advanced past the completed char");
    }

    #[test]
    fn transcript_force_delivery_flushes_a_dangling_partial() {
        let mut core = transcript(1024);
        core.append(b"ok");
        core.append(&[0xE6, 0x97]); // never completed
        let delivered = core.deliver_since_cursor(true);
        assert_eq!(delivered.text, "ok\u{FFFD}", "forced flush is lossy-honest");
        assert!(!delivered.truncated);
    }

    #[test]
    fn transcript_ring_drop_of_undelivered_is_counted_honestly() {
        let mut core = transcript(8);
        core.append(b"0123456789ABCDEF"); // 16 bytes > 8 cap
        let delivered = core.deliver_since_cursor(false);
        assert!(delivered.truncated);
        assert!(
            delivered.dropped_undelivered_bytes >= 8,
            "dropped undelivered bytes counted: {:?}",
            delivered.dropped_undelivered_bytes
        );
        assert_eq!(delivered.total_bytes, 16);
    }

    #[test]
    fn decode_prefix_utf8_boundary_variants() {
        // Fully valid.
        assert_eq!(decode_prefix_utf8(b"abc"), ("abc".to_string(), 3));
        // Trailing incomplete: valid prefix consumed, tail held.
        let (text, consumed) = decode_prefix_utf8(b"ab\xE6\x97");
        assert_eq!(text, "ab");
        assert_eq!(consumed, 2);
        // Interior invalid byte: lossy whole-slice, fully consumed.
        let (text, consumed) = decode_prefix_utf8(b"ab\xFFcd");
        assert!(text.contains('\u{FFFD}'));
        assert_eq!(consumed, 5);
    }

    #[test]
    fn exit_fact_status_codes_follow_the_shell_convention() {
        assert_eq!(ExitFact::Code(7).status_code(), 7);
        assert_eq!(ExitFact::Signal(9).status_code(), 137);
        assert_eq!(ExitFact::Signal(15).status_code(), 143);
    }

    // ── R04-RR1-F01: live-slot admission accounting ──────────────────────

    fn test_supervisor(live_cap: usize) -> Arc<ProcessSupervisor> {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-procsup-unit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut limits = SupervisorLimits::with_spill_dir(dir);
        limits.live_cap = live_cap;
        Arc::new(
            ProcessSupervisor::new(Arc::new(crate::inject::SystemClock), limits)
                .expect("test supervisor"),
        )
    }

    fn dummy_record(id: ProcessHandleId) -> Arc<ProcessRecord> {
        Arc::new(ProcessRecord {
            id,
            owner: ProcessOwner {
                principal_kind: "test".to_string(),
                principal_subject: "test".to_string(),
                session_id: "sess".to_string(),
                run_id: "run".to_string(),
                tool_call_id: ToolCallId::new("call".to_string()),
            },
            kind: ProcessKind::OneShot,
            argv: vec!["unit".to_string()],
            pid: 1234,
            pgid: 1234,
            spawned_at_unix_ms: 0,
            inner: Mutex::new(RecordInner {
                phase: RecordPhase::Running,
                audit: Vec::new(),
                settled: false,
                collector: None,
                transcript: None,
                drained_stdio: false,
                reclaimed: false,
                pty_master: None,
            }),
            phase_tx: tokio::sync::watch::Sender::new(RecordPhase::Running),
        })
    }

    fn live_count_of(supervisor: &ProcessSupervisor) -> usize {
        lock_or_poison(&supervisor.state).live_count
    }

    #[test]
    fn live_slot_reservation_enforces_the_cap_atomically() {
        let supervisor = test_supervisor(1);
        // The first reservation takes the only slot; the second is a
        // capacity refusal (zero dispatch follows from the ordering).
        let slot = supervisor.reserve_live_slot().expect("first reserves");
        assert_eq!(live_count_of(&supervisor), 1);
        assert!(
            matches!(
                supervisor.reserve_live_slot(),
                Err(SpawnFailure::RegistryFull)
            ),
            "the second reservation must be a capacity refusal"
        );
        assert_eq!(live_count_of(&supervisor), 1, "refusal consumes nothing");
        drop(slot);
        assert_eq!(live_count_of(&supervisor), 0, "drop returns the slot once");
        // The returned slot is immediately reusable (no leak).
        let _again = supervisor.reserve_live_slot().expect("slot reusable");
        assert_eq!(live_count_of(&supervisor), 1);
    }

    #[test]
    fn live_slot_commit_transfers_release_to_settle_exactly_once() {
        let supervisor = test_supervisor(1);
        let id = ProcessHandleId::mint().expect("mint");
        let record = dummy_record(id.clone());
        let slot = supervisor.reserve_live_slot().expect("reserves");
        assert_eq!(live_count_of(&supervisor), 1);
        // Commit consumes the guard: a later "drop" of the spawn path
        // cannot release the committed slot.
        slot.commit(Arc::clone(&record));
        assert_eq!(live_count_of(&supervisor), 1, "commit keeps the slot");
        assert!(
            matches!(
                supervisor.reserve_live_slot(),
                Err(SpawnFailure::RegistryFull)
            ),
            "the committed slot is live"
        );
        // Settle releases exactly once: repeated (late/repeated) settles
        // never decrement again.
        supervisor.settle(&id);
        assert_eq!(live_count_of(&supervisor), 0, "settle returns the slot");
        supervisor.settle(&id);
        supervisor.settle(&id);
        assert_eq!(
            live_count_of(&supervisor),
            0,
            "repeated settles never over-release"
        );
        // A late settle for a record that no longer exists is a no-op —
        // it must not mint phantom capacity (no negative undercount).
        let ghost = ProcessHandleId::mint().expect("mint");
        supervisor.settle(&ghost);
        assert_eq!(live_count_of(&supervisor), 0);
        let _ = supervisor.reserve_live_slot().expect("capacity is exact");
    }

    #[test]
    fn live_slot_drop_after_panic_style_abandon_still_releases() {
        let supervisor = test_supervisor(2);
        // Simulate a spawn path abandoned between reserve and commit (the
        // guard's Drop is the compensation — including unwind paths).
        {
            let _slot = supervisor.reserve_live_slot().expect("reserves");
            assert_eq!(live_count_of(&supervisor), 1);
        }
        assert_eq!(live_count_of(&supervisor), 0);
        // Rotation: many reserve/release rounds never drift the count.
        for _ in 0..8 {
            let slot = supervisor.reserve_live_slot().expect("reserves");
            assert_eq!(live_count_of(&supervisor), 1);
            drop(slot);
            assert_eq!(live_count_of(&supervisor), 0);
        }
    }
}
