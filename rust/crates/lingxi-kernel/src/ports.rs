//! Ports: the only way the kernel talks to the outside world.
//!
//! Every dependency of the kernel on infrastructure (persistence, model
//! providers, tool execution, credentials, clock) is declared here as a
//! trait. Implementations live in `lingxi-adapters` and are injected by
//! the `lingxi-service` composition root. This is what makes the
//! dependency rule "kernel never imports adapters" enforceable rather
//! than aspirational.
//!
//! R01-T01 minimal prototype: only the port vocabulary needed to pin the
//! ownership contract is declared. Method surfaces grow in R02+.

use lingxi_protocol::{
    EventEnvelope, EventId, EventPayload, ModelCallId, NormalizedMessage, ProtocolError, RunId,
    RunStatus, Seq, SessionId, ToolCallId,
};
// ProtocolError is the error surface of the provider/tool ports below;
// StoragePort deliberately uses the richer StorageError.

use crate::RunContext;

/// Persistent record of one run's lifecycle, owned by the kernel's
/// RunSupervisor and stored through this port (implementation:
/// `lingxi-adapters` storage module; physical store: the new Rust
/// run/message database).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub status: RunStatus,
    /// Monotonic stream cursor of the last persisted key event.
    pub last_event_seq: Seq,
}

/// Explicit failure modes of the storage port. Every variant is a loud,
/// caller-visible outcome; none is ever silently degraded into success
/// (project red line: key-fact commit failures must not be reported as
/// success — acceptance R02-A07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// The bounded DB work queue is at capacity. This is backpressure:
    /// the job was NOT executed; retry after the queue drains.
    QueueFull,
    /// The DB worker is gone (closed or crashed); the job was not executed.
    QueueClosed,
    /// SQLite could not acquire the write lock within the busy timeout.
    Busy { timeout_ms: u64 },
    /// The write hit a full disk (SQLITE_FULL). The commit failed; nothing
    /// partial is durable. Retryable after space is freed.
    DiskFull { detail: String },
    /// A real filesystem IO error (permission, unwritable WAL/journal
    /// path, EIO...). The commit failed; nothing partial is durable.
    Io { detail: String },
    /// A different terminal record already exists for this run
    /// (conflicting finalize is diagnosed, never merged).
    Conflict { detail: String },
    /// The database file was written by a NEWER build; this build refuses
    /// to downgrade/replay it (no silent replay of older migrations).
    DatabaseTooNew {
        found_version: u64,
        supported_version: u64,
    },
    /// On-disk migration receipts disagree with the compiled-in migration
    /// set (tampered version table / drifted schema fingerprint).
    SchemaTampered { detail: String },
    /// The stored state violates an invariant (e.g. run row references a
    /// missing session, terminal row without its key events).
    Corrupted { detail: String },
    /// The caller asked the port to commit something that is not a legal
    /// domain fact (non-terminal "terminal", transition the state machine
    /// forbids...).
    InvalidRequest { detail: String },
    /// The durable run-id sequence space of a database is fully consumed
    /// (the stored high-water mark is `u64::MAX`). Allocation refuses
    /// LOUDLY instead of panicking, wrapping or re-issuing a number that a
    /// stored run id already owns (R02 stage-repair R5 / F02). Permanent
    /// and non-retryable for that database.
    RunIdExhausted { detail: String },
    /// Anything else; carries the SQLite error text for diagnosis.
    Internal { detail: String },
}

impl StorageError {
    /// Whether retrying the same call later can plausibly succeed.
    /// Deliberately conservative: IO errors are NOT flagged retryable
    /// (the caller must diagnose), queue-full/busy/disk-full are.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            StorageError::QueueFull | StorageError::Busy { .. } | StorageError::DiskFull { .. }
        )
    }
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::QueueFull => write!(
                f,
                "bounded DB work queue at capacity (backpressure; job not executed)"
            ),
            StorageError::QueueClosed => write!(f, "DB worker closed; job not executed"),
            StorageError::Busy { timeout_ms } => {
                write!(
                    f,
                    "sqlite write lock not acquired within busy timeout {timeout_ms}ms"
                )
            }
            StorageError::DiskFull { detail } => write!(f, "disk full: commit failed ({detail})"),
            StorageError::Io { detail } => write!(f, "storage IO failure: {detail}"),
            StorageError::Conflict { detail } => write!(f, "conflicting terminal record: {detail}"),
            StorageError::DatabaseTooNew {
                found_version,
                supported_version,
            } => write!(
                f,
                "database schema version {found_version} is newer than this build supports \
                 ({supported_version}); refusing to downgrade or replay"
            ),
            StorageError::SchemaTampered { detail } => {
                write!(f, "schema migration receipts tampered or drifted: {detail}")
            }
            StorageError::Corrupted { detail } => write!(f, "stored state corrupt: {detail}"),
            StorageError::InvalidRequest { detail } => {
                write!(f, "illegal storage request: {detail}")
            }
            StorageError::RunIdExhausted { detail } => {
                write!(f, "run id sequence exhausted: {detail}")
            }
            StorageError::Internal { detail } => write!(f, "storage internal error: {detail}"),
        }
    }
}

impl std::error::Error for StorageError {}

/// R03 RR2/F05-01: the result of the durable cross-restart request-anchor
/// lookup (the `request:` lineage rows of user runs). The logical
/// (canonical) request key can be durably bound to exactly one run, to
/// none, or — only for rows written by PRE-FIX builds whose `cause_id`
/// held the raw, un-normalized id — to SEVERAL runs; the multi-run case
/// must surface as an explicit ambiguity at the submission boundary, never
/// a silent pick of one binding (each bound run may hold confirmed or
/// unknown external effects) and never a blind fresh re-execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestBindingLookup {
    /// No run is durably bound to this logical request key — the id is
    /// fresh at the durable layer too.
    Unbound,
    /// Exactly one run is bound (the canonical anchor, or a single legacy
    /// raw-padded anchor that normalizes to this key).
    Bound { run_id: String },
    /// SEVERAL distinct runs are bound to the same logical key (the
    /// pre-fix duplicate-execution shape). Newest first (the lookup's
    /// deterministic order); the caller names them ALL.
    Ambiguous { run_ids: Vec<String> },
}

/// One key event to persist as part of a run commit. The kernel supplies
/// identity (`event_id`) and payload; the single writer assigns the
/// per-stream `seq` inside the same transaction and returns the durable
/// envelope through [`CommittedOutcome`].
#[derive(Debug, Clone, PartialEq)]
pub struct KeyEvent {
    pub event_id: EventId,
    pub payload: EventPayload,
}

/// The outcome of one run, committed by [`StoragePort::commit_run_outcome`]
/// as ONE transaction: terminal status, its key events and (when present)
/// the final normalized message become durable together or not at all.
/// Publication of the events happens strictly AFTER the commit returns
/// Ok — a failed commit must never produce a visible success or a
/// completion event (contract 02 §7/§8, acceptance R02-A07).
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    /// MUST be a terminal status (`RunStatus::is_terminal`); the
    /// implementation rejects anything else with
    /// [`StorageError::InvalidRequest`].
    pub status: RunStatus,
    #[allow(dead_code)]
    pub reason: Option<String>,
    /// Key events of the terminal transition (e.g. the final
    /// `run_state_changed`). They are persisted in the same transaction
    /// as the status, so after a crash either both exist or neither.
    pub key_events: Vec<KeyEvent>,
    /// Final normalized assistant message, if the run produced one.
    /// A run may complete WITHOUT a final message and that must never be
    /// fabricated (contract 02 §4); when present it is committed in the
    /// same transaction and published as `final_message_committed`.
    #[allow(dead_code)]
    pub final_message: Option<NormalizedMessage>,
}

impl RunOutcome {
    /// Returns Err when `status` is not terminal — the cheap domain half of
    /// the validation the storage implementation re-checks.
    pub fn validate_terminal(&self) -> Result<(), StorageError> {
        if self.status.is_terminal() {
            Ok(())
        } else {
            Err(StorageError::InvalidRequest {
                detail: format!(
                    "commit_run_outcome requires a terminal status, got {}",
                    self.status.wire_name()
                ),
            })
        }
    }
}

/// Result of a successful [`StoragePort::commit_run_outcome`].
#[derive(Debug, Clone, PartialEq)]
pub struct CommittedOutcome {
    /// `false` when an identical terminal record already existed
    /// (idempotent replay — the run had already been finalized exactly
    /// this way; events are still returned so late subscribers can catch
    /// up from the durable store).
    pub newly_committed: bool,
    /// The events exactly as committed (seq/stream assigned by the single
    /// writer, in commit order). Handed to the publisher only now that
    /// the transaction is durable.
    pub events: Vec<EventEnvelope>,
}

/// Persistence of run/session authority data: the storage port (R02-T04).
///
/// Contract (taskbook 02 §8, R02-T04):
/// - Finalize is a single idempotent transaction: terminal status + its key
///   events (+ final message) commit together, exactly once; repeated
///   identical requests are idempotent, conflicting results are diagnosed
///   with [`StorageError::Conflict`], never merged.
/// - Events are published AFTER the commit: [`CommittedOutcome::events`]
///   exists only on the success path, so a failed commit cannot produce a
///   visible success or a completion event.
/// - Implementations run all synchronous SQLite work on a bounded queue
///   owned by the adapter (backpressure is [`StorageError::QueueFull`],
///   never a silent drop), enforce a busy timeout, and surface disk-full
///   as [`StorageError::DiskFull`].
pub trait StoragePort: Send + Sync {
    /// Durable facts of one run's start (run row + first attempt row +
    /// `run_state_changed` key events). The run enters `running`.
    fn record_run_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send;

    /// Atomically commits the terminal outcome (see [`RunOutcome`]).
    fn commit_run_outcome(
        &self,
        ctx: &RunContext,
        outcome: RunOutcome,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send;

    /// Loads one run's current record (None when unknown).
    fn load_run(
        &self,
        run_id: &RunId,
    ) -> impl std::future::Future<Output = Result<Option<RunRecord>, StorageError>> + Send;

    /// Persists mid-run key events (model-call / tool-call facts) bound to
    /// `ctx`'s run and CURRENT attempt in ONE transaction (R03-T01 step 2).
    ///
    /// Contract:
    /// - The run must exist, belong to the same owner facts as `ctx`, and
    ///   still be ACTIVE — events for a terminal run are rejected loudly
    ///   AND recorded as audit-only stale facts
    ///   ([`StoragePort::record_stale_result`]); a settled run never
    ///   resurrects and its stream never grows (R03-T04 full fence).
    /// - `ctx.attempt` must be the run's CURRENT attempt (the most recently
    ///   opened one, via [`StoragePort::record_run_started`] or
    ///   [`StoragePort::record_attempt_started`]): results never attach to
    ///   an attempt that never started, and a late result carrying an older
    ///   attempt id is refused for state purposes and audited as stale —
    ///   old results never pose as the current attempt's output (the
    ///   R03-T04 attempt/generation fence).
    /// - The run's `last_event_seq` advances inside the same transaction;
    ///   events are returned for publication strictly after commit.
    fn record_run_events(
        &self,
        ctx: &RunContext,
        events: Vec<KeyEvent>,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send;

    /// Audit-only bookkeeping of one refused LATE result (the R03-T04
    /// fence). NEVER writes the run's key-event stream, its status or any
    /// message — the whole point is that a refused write still leaves a
    /// diagnosable durable trace ("拒写但留审计痕迹").
    ///
    /// Contract:
    /// - ALWAYS legal, whatever the run's state: the audited claim may name
    ///   a terminal run, a superseded attempt, an attempt that never
    ///   opened, or even a run row that does not exist — the audit row
    ///   records the CLAIM as received (it proves what arrived late, not
    ///   that the claim was ever legitimate).
    /// - Called by the run driver when it fences an in-flight result, and
    ///   by implementations of [`StoragePort::record_run_events`] when they
    ///   refuse a stale delivery (the audit write joins the refusing
    ///   transaction there).
    /// - Failures are real storage failures (loud); nothing about the
    ///   audited run changes on either path.
    fn record_stale_result(
        &self,
        ctx: &RunContext,
        refused: StaleResultFact,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;

    /// Opens a NEW attempt on an EXISTING run (R03-T01 step 2: retries
    /// increment the attempt on the same run; the run id is fixed at
    /// creation and a provider reconnect never mints a new user task).
    ///
    /// Contract:
    /// - The run must exist, match `ctx`'s owner facts, and be in a
    ///   non-terminal state (retry from `running`; `waiting_approval`
    ///   retries arrive with R04).
    /// - `ctx.attempt` must NOT already exist for the run — each retry is
    ///   a fresh attempt identity.
    /// - One transaction: new `run_attempts` row + `attempt_count`
    ///   increment. The run's STATUS does not change (still `running`).
    fn record_attempt_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send;

    /// Persists one NON-TERMINAL run phase change (R03-T03): the durable
    /// legs of `running ↔ waiting_approval` and every active state's entry
    /// into `cancelling` (the two-phase cancellation contract — a
    /// `cancelled` finalize is only legal from a durably `cancelling` run).
    ///
    /// Contract:
    /// - The run must exist and match `ctx`'s owner facts (same as every
    ///   other write of this port).
    /// - The run's durably stored status must EQUAL `from` — a caller
    ///   operating on a stale view of the run is diagnosed loudly
    ///   (Conflict), never silently absorbed.
    /// - `from` must be a legal non-terminal source of the transition and
    ///   `to` a legal NON-TERMINAL target per [`crate::RunStateMachine`];
    ///   terminal targets belong to [`StoragePort::commit_run_outcome`]
    ///   and are rejected here with [`StorageError::InvalidRequest`].
    /// - One transaction: `runs.status` update + a `run_state_changed` key
    ///   event (+ `last_event_seq` advance); events are returned for
    ///   publication strictly after the commit.
    fn record_run_state_change(
        &self,
        ctx: &RunContext,
        from: lingxi_protocol::RunStatus,
        to: lingxi_protocol::RunStatus,
        reason: Option<String>,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<CommittedOutcome, StorageError>> + Send;

    // ── R03-T05: the InvocationJournal ─────────────────────────────────────
    //
    // One journal entry per tool invocation (journal id = tool call id).
    // Write order is the contract:
    //   1. `record_invocation_intent` (prepared) and
    //      `advance_invocation` (authorized, started) are durable BEFORE
    //      the external execution is dispatched;
    //   2. `record_invocation_receipt` / `record_invocation_unknown` close
    //      the entry AFTER the external execution.
    // No method claims an atomic transaction across the external system.

    /// Durably records the invocation INTENT (phase `prepared`) before
    /// anything external is dispatched. Binding: owner facts, run /
    /// attempt / generation, target, argument digest and (when present)
    /// the idempotency key — all from `ctx` plus [`InvocationIntent`].
    ///
    /// Idempotent: re-recording the IDENTICAL intent is a replay; a
    /// conflicting intent under the same journal id is a loud
    /// [`StorageError::Conflict`].
    fn record_invocation_intent(
        &self,
        ctx: &RunContext,
        intent: InvocationIntent,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;

    /// Advances the entry to `authorized` or `started` (the run-layer
    /// authorization context decides when each is true). `started` MUST be
    /// durable before the external execution is dispatched — that write is
    /// what makes a crash between execution and receipt classifiable as
    /// UNKNOWN.
    ///
    /// The phase ladder is enforced in order (`prepared → authorized →
    /// started`); re-advancing to the CURRENT phase is an idempotent
    /// no-op; skipping or regressing is [`StorageError::InvalidRequest`].
    fn advance_invocation(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        to: InvocationPhase,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;

    /// Closes the entry with the external receipt (succeeded / failed /
    /// unknown + dedup identifier). Legal from `started` (the live path),
    /// from an intent-only phase when the call never dispatched
    /// (`dispatched: false`, e.g. a rejected approval), and from `unknown`
    /// (a VERIFIED receipt settles an unconfirmed outcome). Closing an
    /// already-closed (`succeeded`/`failed`) entry replays only when the
    /// receipt is completely identical; anything else is a loud Conflict.
    fn record_invocation_receipt(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        receipt: InvocationReceipt,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;

    /// Recovery-surface close: marks a `started` (or already-`unknown`)
    /// entry as [`InvocationPhase::Unknown`] — the durable statement that
    /// the external outcome could not be determined from local state
    /// (crash between the external execution and the receipt commit).
    ///
    /// Deliberately identity-free beyond the journal id: the entry's own
    /// bound identity (owner/run/attempt/generation) is the authority —
    /// this writes NO new identity facts, only the honest classification
    /// of the entry it names. A later verified receipt may still settle
    /// the entry.
    fn record_invocation_unknown(
        &self,
        journal_id: &ToolCallId,
        detail: String,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;

    /// Loads one run's journal entries (oldest first) — the recovery
    /// classification read.
    fn load_invocation_journal(
        &self,
        run_id: &RunId,
    ) -> impl std::future::Future<Output = Result<Vec<InvocationJournalEntry>, StorageError>> + Send;

    // ── R03-T06: run lineage (parentRunId / origin / sourceMessageId /
    //    causeId) ─────────────────────────────────────────────────────────
    //
    // The four-part identity that ties every run to what caused it. Child
    // runs (origin=subagent) fill all four anchors; user submissions carry
    // their submission anchors. Bridge/cron entries reuse THIS surface in
    // R07 — there is no second scheduler.

    /// Durably records the LINEAGE of one run. Contract:
    /// - the run must exist, belong to the same owner facts as `ctx`, and
    ///   be NON-TERMINAL (lineage is a creation fact, not a post-mortem
    ///   annotation);
    /// - one transaction;
    /// - lineage is IMMUTABLE once recorded: re-recording the IDENTICAL
    ///   lineage is an idempotent replay; a DIFFERENT lineage for the same
    ///   run is a loud [`StorageError::Conflict`] (a run's parentage is
    ///   never rewritten).
    fn record_run_lineage(
        &self,
        ctx: &RunContext,
        lineage: crate::subagent::RunLineage,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;

    /// Loads one run's recorded lineage (None when the run is unknown).
    /// An origin value this build cannot parse surfaces as
    /// [`StorageError::Corrupted`] — never a guess.
    fn load_run_lineage(
        &self,
        run_id: &RunId,
    ) -> impl std::future::Future<Output = Result<Option<crate::subagent::RunLineage>, StorageError>>
           + Send;
}

/// Read/maintenance surface over the durable key-event log (R02-T05).
///
/// The storage port ([`StoragePort`]) WRITES key events as part of run
/// transactions; this port is the READ half consumers use to rebuild a
/// consistent view: snapshots, cursor continuation and gap detection.
///
/// Contract (taskbook 02 §7, R02-T05):
/// - `seq` is assigned per stream by the single writer inside the
///   committing transaction, so within one stream the durable log is
///   strictly increasing with no holes; every event with `seq <= head`
///   is already committed, and any event committed later gets
///   `seq > head` (that is what makes snapshot+subscription joins
///   gap-free).
/// - Implementations return envelopes exactly as committed (rebuilt from
///   the durable row; a disagreement between the stored `event_type`
///   column and the payload tag is [`StorageError::Corrupted`], never a
///   guess about which one is authoritative).
/// - `purge_events_before` is the retention maintenance operation. It
///   MUST NOT be used to silently skip history: after a purge, a cursor
///   pointing before the new floor must surface as an explicit
///   expired-cursor condition on the read paths (the caller decides the
///   signal shape; the storage layer just reports the floor).
pub trait EventStorePort: Send + Sync {
    /// Highest committed `seq` of the stream (`None` when the stream has
    /// no events / is unknown — callers combine with session lookups to
    /// distinguish "stale stream" from "empty stream").
    fn stream_head(
        &self,
        stream_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<Seq>, StorageError>> + Send;

    /// Lowest retained `seq` of the stream (`None` when empty). Equals the
    /// first non-purged event; a gap between a client cursor and this
    /// floor means the events the cursor expects were truncated.
    fn stream_floor(
        &self,
        stream_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<Seq>, StorageError>> + Send;

    /// Committed events of the stream with `seq > after_seq`, ascending,
    /// at most `limit`. This is the single read used by BOTH snapshot
    /// cuts and cursor continuation (same ordering, same authority).
    fn stream_events_after(
        &self,
        stream_id: &str,
        after_seq: Seq,
        limit: u32,
    ) -> impl std::future::Future<Output = Result<Vec<EventEnvelope>, StorageError>> + Send;

    /// Retention maintenance: deletes committed events with
    /// `seq < before_seq` (exclusive) and returns how many rows were
    /// removed. See the trait docs for the no-silent-gap contract.
    fn purge_events_before(
        &self,
        stream_id: &str,
        before_seq: Seq,
    ) -> impl std::future::Future<Output = Result<u64, StorageError>> + Send;
}

/// Stable identity facts of a turn provider (R03-T01). A deterministic test
/// double reports what it simulates; a real provider (R05) reports its own
/// provider/model pair — provider+model is one identity unit, two providers
/// may share a model id without being the same model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDescriptor {
    pub provider: String,
    pub model: String,
    pub operation: String,
}

/// One provider turn, as the run driver consumes it (R03-T01 step 2/4).
///
/// Test-double boundary: a double only DECIDES what to reply; the kernel /
/// run driver owns every state decision — which turn ends the run, what the
/// outcome contract is, and the single finalize path. A double never writes
/// state and never finalizes a run.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderTurn {
    /// The provider's final assistant reply for the user. An empty-content
    /// message is NOT a final (the driver maps it to an explicit empty
    /// outcome instead of committing an empty final message).
    Final { message: NormalizedMessage },
    /// The provider requested tool calls; the RUN continues after they
    /// execute (a model call ending here does NOT end the run).
    ToolRequests { requests: Vec<ToolRequest> },
    /// Process-only content (reasoning / partial output): this model call
    /// ended, the run continues with another turn.
    Continue { process_note: String },
    /// The provider finished with zero usable content (empty reply).
    Empty { detail: String },
    /// The provider call failed. `retryable` marks transient failures where
    /// a NEW ATTEMPT on the SAME run is legitimate — a provider reconnect
    /// never mints a new user task (run id stays fixed).
    Failed {
        error: ProtocolError,
        retryable: bool,
    },
}

/// The identity triple EVERY asynchronous result must carry (R03-T04
/// fence): the writer verifies these against the CURRENT context right
/// before writing state — not only when the request was issued.
///
/// A result whose fence names an older attempt, another run or another
/// generation is a LATE result: it is refused for state purposes and only
/// its audit trace survives ([`StoragePort::record_stale_result`]). Old
/// results can never pollute the next attempt, the next run or a session
/// that has switched away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultFence {
    pub run_id: RunId,
    pub attempt: lingxi_protocol::AttemptId,
    pub generation: u64,
}

impl ResultFence {
    /// The fence of the context a call was issued under — what a healthy
    /// adapter echoes back with its result.
    pub fn of_ctx(ctx: &RunContext) -> Self {
        Self {
            run_id: ctx.run_id.clone(),
            attempt: ctx.attempt.clone(),
            generation: ctx.generation,
        }
    }

    /// The fence check the writer performs before ANY state write: the
    /// result belongs to the current run, the current attempt and the
    /// current registry generation.
    pub fn matches_ctx(&self, ctx: &RunContext) -> bool {
        self.run_id == ctx.run_id
            && self.attempt == ctx.attempt
            && self.generation == ctx.generation
    }
}

/// What `TurnProviderPort::next_turn` resolves with (R03-T04): the provider
/// turn PLUS the identity fence the driver verifies before using it. A real
/// adapter (R05) echoes the context it was called with; a deferred/raced
/// delivery carries the identity of the ORIGINAL request and is fenced.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderTurnResult {
    pub fence: ResultFence,
    pub turn: ProviderTurn,
}

impl ProviderTurnResult {
    /// A result echoing the context it was issued under (the honest default
    /// every adapter uses unless it is delivering a late/raced result).
    pub fn of_ctx(ctx: &RunContext, turn: ProviderTurn) -> Self {
        Self {
            fence: ResultFence::of_ctx(ctx),
            turn,
        }
    }
}

/// What `ToolExecutorPort::execute` resolves with (R03-T04): the structured
/// outcome PLUS the identity fence the driver verifies before persisting.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecutionResult {
    pub fence: ResultFence,
    pub outcome: ToolOutcome,
}

impl ToolExecutionResult {
    /// A result echoing the context it was issued under.
    pub fn of_ctx(ctx: &RunContext, outcome: ToolOutcome) -> Self {
        Self {
            fence: ResultFence::of_ctx(ctx),
            outcome,
        }
    }
}

/// Why one asynchronous result was fenced as stale (the R03-T04 fence
/// vocabulary — machine-diagnosable, never a silent drop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LateResultReason {
    /// The run already holds a terminal state: a settled run is never
    /// resurrected by a late result.
    RunTerminal,
    /// The result names an attempt that WAS opened but is no longer the
    /// run's current attempt (a retry or a later attempt superseded it).
    AttemptStale,
    /// The result names an attempt that never started on this run (the
    /// T01 identity floor).
    AttemptNeverOpened,
    /// The result's identity fence does not match the context the writer
    /// currently holds (another run / another generation).
    FenceMismatch,
    /// The result carried the right identity but the run's cancellation
    /// was observed before the state write ("在写状态前核对" — the write
    /// side of the biased cancellation race).
    CancelledBeforeWrite,
}

impl LateResultReason {
    /// Stable machine-readable name (audit vocabulary, evidence output).
    pub fn name(self) -> &'static str {
        match self {
            LateResultReason::RunTerminal => "run_terminal",
            LateResultReason::AttemptStale => "attempt_stale",
            LateResultReason::AttemptNeverOpened => "attempt_never_opened",
            LateResultReason::FenceMismatch => "fence_mismatch",
            LateResultReason::CancelledBeforeWrite => "cancelled_before_write",
        }
    }
}

/// The audit-only fact of one refused late result (payload of
/// [`StoragePort::record_stale_result`]). Deliberately summary-shaped: the
/// event TYPES are recorded, never full payloads — the refused content is
/// not run state and must not become unbounded bookkeeping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleResultFact {
    pub reason: LateResultReason,
    /// The event types of the refused delivery (empty when the fenced
    /// result was refused before any event was staged).
    pub refused_event_types: Vec<String>,
}

// ── R03-T05: side-effect invocation receipts (InvocationJournal) ─────────────

/// The receipt lifecycle of one tool invocation (R03-T05 怎么做 1):
/// `prepared → authorized → started → succeeded | failed | unknown`.
///
/// - `prepared` / `authorized` are intent-only phases: the external call
///   was never dispatched, so recovery classifies them as 未执行.
/// - `started` is durably recorded BEFORE the external execution is
///   dispatched; an entry left here by a crash is exactly the
///   "已执行但回执未持久化" window and recovers as UNKNOWN.
/// - `succeeded` / `failed` are closed receipts (definitive).
/// - `unknown` is the honest closure when the external outcome cannot be
///   determined — either recorded from an unobserved/fenced result or by
///   recovery classification. It is NOT final: a later VERIFIED receipt
///   may settle it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationPhase {
    Prepared,
    Authorized,
    Started,
    Succeeded,
    Failed,
    Unknown,
}

impl InvocationPhase {
    /// Stable storage/evidence vocabulary (the `invocation_journal.phase`
    /// column). Changing a value is a data migration, not a rename.
    pub fn wire_name(self) -> &'static str {
        match self {
            InvocationPhase::Prepared => "prepared",
            InvocationPhase::Authorized => "authorized",
            InvocationPhase::Started => "started",
            InvocationPhase::Succeeded => "succeeded",
            InvocationPhase::Failed => "failed",
            InvocationPhase::Unknown => "unknown",
        }
    }

    /// Parses the storage vocabulary (unknown values are corruption, never
    /// a guess).
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "prepared" => Some(InvocationPhase::Prepared),
            "authorized" => Some(InvocationPhase::Authorized),
            "started" => Some(InvocationPhase::Started),
            "succeeded" => Some(InvocationPhase::Succeeded),
            "failed" => Some(InvocationPhase::Failed),
            "unknown" => Some(InvocationPhase::Unknown),
            _ => None,
        }
    }

    /// Whether the entry holds a CLOSED receipt (`succeeded`/`failed`).
    /// `unknown` is deliberately NOT closed: a verified receipt may still
    /// settle it.
    pub fn is_closed(&self) -> bool {
        matches!(self, InvocationPhase::Succeeded | InvocationPhase::Failed)
    }
}

/// The durable intent of one tool invocation, recorded BEFORE anything
/// external is dispatched (R03-T05 阶段书怎么做 2: 副作用执行前持久化开始
/// 意图). The journal id is the tool call id the run driver minted, so the
/// receipt binds to exactly one dispatch of exactly one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationIntent {
    /// Tool call id (the journal identity; minted by the run driver).
    pub journal_id: ToolCallId,
    /// Registry target of the tool (R04 owns the real registry).
    pub target: String,
    /// Digest of the normalized arguments (the value approvals and
    /// receipts bind to).
    pub args_digest: String,
    /// Human-readable argument summary (bounded; diagnostic only).
    pub args_summary: Option<String>,
    /// The idempotency key this invocation presents to the external
    /// system, when one exists. Whether the external system HONORS the key
    /// is a per-tool capability
    /// (`crate::invocation::ToolRecoveryCapability`); the key itself is a
    /// durable fact of the receipt.
    pub idempotency_key: Option<String>,
}

/// The closed outcome half of a receipt: `unknown` is a first-class value —
/// the honest statement that the external outcome could not be determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptOutcome {
    Succeeded,
    Failed,
    Unknown,
}

impl ReceiptOutcome {
    pub fn wire_name(self) -> &'static str {
        match self {
            ReceiptOutcome::Succeeded => "succeeded",
            ReceiptOutcome::Failed => "failed",
            ReceiptOutcome::Unknown => "unknown",
        }
    }
}

/// The receipt recorded AFTER the external execution (R03-T05 怎么做 2:
/// 执行后持久化外部响应/可用去重标识). There is no cross-system atomic
/// transaction: this row records what the external system returned (or that
/// it returned nothing provable — [`ReceiptOutcome::Unknown`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationReceipt {
    pub outcome: ReceiptOutcome,
    /// Bounded diagnostic (external response summary / error code / why
    /// the outcome is unknown).
    pub detail: String,
    /// The dedup identifier the external system provided or that the
    /// invocation's idempotency key resolved to (`None` when unavailable).
    pub dedup_id: Option<String>,
    /// Whether the external execution was dispatched at all. `false` for
    /// closures that never dispatched (e.g. a rejected approval); `true`
    /// for any outcome observed after dispatch.
    pub dispatched: bool,
}

/// One durable journal entry (the read view used by recovery
/// classification). Carries everything the receipt is bound to: owner
/// facts, run/attempt/generation, target, argument digest, idempotency key
/// and the phase/receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationJournalEntry {
    pub journal_id: String,
    pub session_id: String,
    pub run_id: String,
    pub attempt: String,
    pub generation: u64,
    pub owner_kind: String,
    pub owner_subject: String,
    pub target: String,
    pub args_digest: String,
    pub args_summary: Option<String>,
    pub idempotency_key: Option<String>,
    pub phase: InvocationPhase,
    /// The closed receipt, once one exists.
    pub receipt: Option<InvocationReceipt>,
    pub prepared_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

/// One tool call a provider requested. Identity (`ToolCallId`) is minted by
/// the run driver, never by the provider — and never derived from the run
/// or attempt id.
// NOTE: not `Eq` — `ArgsDigest` follows the wire digest struct.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRequest {
    /// Registry target id (R04 owns the real registry; T01 doubles carry a
    /// stable target name).
    pub target: String,
    /// Digest of the normalized arguments (the value approvals bind to).
    pub args_digest: lingxi_protocol::ArgsDigest,
    pub args_summary: Option<String>,
    /// The structured delegation payload when (and only when) the target
    /// is a `subagent`-family tool — `subagent` (fresh dispatch),
    /// `subagent_reply` (continuation) or `subagent_close` (R03-T06).
    /// Carries WHAT the model asked for (task text, explicit access,
    /// label, agent, model, thread id) — never an authorization: the
    /// child's grant is resolved by the run layer against the parent's
    /// facts ([`crate::subagent`]), so neither this payload nor a later
    /// model/executor swap can widen permissions.
    pub delegation: Option<DelegationRequest>,
}

/// The delegation request of a `subagent`-family tool call (R03-T06).
/// The incumbent's tool parameters (`task` / `access` / `label` / `agent`
/// / `model`, plus `threadId` for reply/close), as a kernel type so the
/// driver never parses provider JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationRequest {
    /// Complete instructions and required context for the child. The
    /// child CANNOT see the parent conversation history unless it is in
    /// here (the incumbent's visibility rule). For a `subagent_close`
    /// call this carries the optional closing reason instead.
    pub task: String,
    /// Explicit access tier request (`read` / `write`); `None` = inherit
    /// the parent session's current mode (for a reply: the explicit tier,
    /// else the thread's recorded tier, else inherit — resolved by the
    /// run layer).
    pub access: Option<crate::subagent::AccessRequest>,
    /// Display-only label.
    pub label: Option<String>,
    /// Target agent id (`None` = the parent's agent).
    pub agent_id: Option<String>,
    /// Optional model override — changing it NEVER changes the grant.
    pub model: Option<String>,
    /// The thread to continue (`subagent_reply`) or close
    /// (`subagent_close`); `None` for a fresh `subagent` dispatch.
    pub thread_id: Option<String>,
}

/// Provider access for the run driver (R03-T01; the R05 handoff surface).
///
/// R03 wires deterministic doubles through the REAL run chain; R05 replaces
/// the double with real protocol adapters behind this same port. Every call
/// is pinned to its run/attempt context and its own `ModelCallId`.
///
/// The futures are boxed so the trait is object-safe: the supervisor holds
/// `Arc<dyn TurnProviderPort>` (one injection point, doubles in tests /
/// real adapters in R05).
pub trait TurnProviderPort: Send + Sync {
    /// Stable identity of what this provider simulates / serves.
    fn descriptor(&self) -> ProviderDescriptor;

    /// Produces the next model turn for the run. `input` is the user
    /// submission that started the run (the full context assembly is R06;
    /// the port stays minimal here). The result carries the identity fence
    /// of the request it answers; the driver verifies it against the
    /// CURRENT context before any state write (R03-T04).
    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        turn: u32,
        input: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>>;
}

/// Minimal tool execution port for the R03 run driver (the R04 handoff
/// surface: the unified ToolInvocationGateway replaces T01 doubles behind
/// this port shape, under the same identity, permission and cancel rules).
pub trait ToolExecutorPort: Send + Sync {
    /// Executes one authorized tool request. The outcome is the structured
    /// [`ToolOutcome`] — `Unknown` is mandatory so an externally completed
    /// side effect is never retried blindly nor reported as success. The
    /// result carries the identity fence of the request it answers; the
    /// driver verifies it against the CURRENT context before persisting
    /// (R03-T04).
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>>;
}

/// Result of one tool call. `Unknown` is mandatory: an externally
/// completed side effect with no local receipt is never silently
/// retried and never reported as success.
// NOTE: not `Eq` — it carries `ProtocolError`, whose `details` holds
// arbitrary JSON values.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolOutcome {
    Success { content_digest: String },
    Failed { error: ProtocolError },
    Cancelled,
    Unknown { reason: String },
}

/// Server-side resolution of provider credentials. Only the service
/// layer may hold credential material; the kernel sees resolved,
/// scoped handles.
pub trait CredentialPort {
    fn resolve_provider_credential(
        &self,
        ctx: &RunContext,
        provider: &str,
    ) -> Result<CredentialHandle, ProtocolError>;
}

/// Opaque, scoped credential handle. Contains no secret material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialHandle {
    pub handle_id: String,
    pub provider: String,
    pub expires_at_unix_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_protocol::{AttemptId, KnownEventPayload, RunStateChangedPayload, StreamId};

    use crate::Principal;

    fn ctx() -> RunContext {
        RunContext {
            principal: Principal::LocalUser,
            session_id: SessionId::new("s-1"),
            run_id: RunId::new("r-1"),
            attempt: AttemptId::new("a-1"),
            generation: 7,
        }
    }

    fn completed_outcome() -> RunOutcome {
        RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            key_events: vec![KeyEvent {
                event_id: EventId::new("evt-done"),
                payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                    RunStateChangedPayload {
                        from: RunStatus::Running,
                        to: RunStatus::Completed,
                        reason: None,
                    },
                )),
            }],
            final_message: None,
        }
    }

    /// Fake port proving the trait-level contract shape: events are handed
    /// back ONLY after a successful commit, and a failing commit yields an
    /// Err with no visible success.
    struct FakePort {
        fail_commits: bool,
        committed: std::sync::Mutex<Vec<RunOutcome>>,
    }

    impl StoragePort for FakePort {
        async fn record_run_started(
            &self,
            _ctx: &RunContext,
            _now_unix_ms: u64,
        ) -> Result<CommittedOutcome, StorageError> {
            Ok(CommittedOutcome {
                newly_committed: true,
                events: Vec::new(),
            })
        }

        async fn commit_run_outcome(
            &self,
            _ctx: &RunContext,
            outcome: RunOutcome,
            _now_unix_ms: u64,
        ) -> Result<CommittedOutcome, StorageError> {
            outcome.validate_terminal()?;
            if self.fail_commits {
                return Err(StorageError::Io {
                    detail: "injected commit failure".to_string(),
                });
            }
            let mut committed = self.committed.lock().expect("fake port lock");
            let identical = committed.contains(&outcome);
            committed.push(outcome);
            Ok(CommittedOutcome {
                newly_committed: !identical,
                events: vec![EventEnvelope::new(
                    EventId::new("evt-done"),
                    StreamId::new("stream-1"),
                    Seq::new(1),
                    SessionId::new("s-1"),
                    None,
                    None,
                    EventPayload::Known(KnownEventPayload::RunStateChanged(
                        RunStateChangedPayload {
                            from: RunStatus::Running,
                            to: RunStatus::Completed,
                            reason: None,
                        },
                    )),
                )],
            })
        }

        async fn load_run(&self, _run_id: &RunId) -> Result<Option<RunRecord>, StorageError> {
            Ok(None)
        }

        async fn record_run_events(
            &self,
            _ctx: &RunContext,
            events: Vec<KeyEvent>,
            _now_unix_ms: u64,
        ) -> Result<CommittedOutcome, StorageError> {
            Ok(CommittedOutcome {
                newly_committed: true,
                events: events
                    .into_iter()
                    .map(|event| {
                        EventEnvelope::new(
                            event.event_id,
                            StreamId::new("stream-1"),
                            Seq::new(1),
                            SessionId::new("s-1"),
                            None,
                            None,
                            event.payload,
                        )
                    })
                    .collect(),
            })
        }

        async fn record_attempt_started(
            &self,
            _ctx: &RunContext,
            _now_unix_ms: u64,
        ) -> Result<CommittedOutcome, StorageError> {
            Ok(CommittedOutcome {
                newly_committed: true,
                events: Vec::new(),
            })
        }

        async fn record_run_state_change(
            &self,
            _ctx: &RunContext,
            from: lingxi_protocol::RunStatus,
            to: lingxi_protocol::RunStatus,
            _reason: Option<String>,
            _now_unix_ms: u64,
        ) -> Result<CommittedOutcome, StorageError> {
            // The kernel-trait half: only the state machine's authority is
            // simulated here (the real store's transactional behavior is
            // tested against the adapter).
            crate::RunStateMachine::transition(from, to).map_err(|err| {
                StorageError::InvalidRequest {
                    detail: err.reason.to_string(),
                }
            })?;
            if to.is_terminal() {
                return Err(StorageError::InvalidRequest {
                    detail: "record_run_state_change requires a non-terminal target".to_string(),
                });
            }
            Ok(CommittedOutcome {
                newly_committed: true,
                events: Vec::new(),
            })
        }

        async fn record_stale_result(
            &self,
            _ctx: &RunContext,
            _refused: StaleResultFact,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            // Audit-only bookkeeping has no kernel rule to simulate (the
            // durable behavior is proven against the real adapter).
            Ok(())
        }

        async fn record_invocation_intent(
            &self,
            _ctx: &RunContext,
            _intent: InvocationIntent,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            // Journal durability is proven against the real adapter; the
            // kernel-trait half has no additional rule to simulate.
            Ok(())
        }

        async fn advance_invocation(
            &self,
            _ctx: &RunContext,
            _journal_id: &ToolCallId,
            to: InvocationPhase,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            if !matches!(to, InvocationPhase::Authorized | InvocationPhase::Started) {
                return Err(StorageError::InvalidRequest {
                    detail: "advance_invocation only targets authorized/started".to_string(),
                });
            }
            Ok(())
        }

        async fn record_invocation_receipt(
            &self,
            _ctx: &RunContext,
            _journal_id: &ToolCallId,
            receipt: InvocationReceipt,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            if receipt.outcome == ReceiptOutcome::Succeeded && !receipt.dispatched {
                return Err(StorageError::InvalidRequest {
                    detail: "a succeeded receipt must have dispatched the execution".to_string(),
                });
            }
            Ok(())
        }

        async fn record_invocation_unknown(
            &self,
            _journal_id: &ToolCallId,
            _detail: String,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            Ok(())
        }

        async fn load_invocation_journal(
            &self,
            _run_id: &RunId,
        ) -> Result<Vec<InvocationJournalEntry>, StorageError> {
            Ok(Vec::new())
        }

        async fn record_run_lineage(
            &self,
            _ctx: &RunContext,
            _lineage: crate::subagent::RunLineage,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            // Lineage durability is covered against the real adapter.
            Ok(())
        }

        async fn load_run_lineage(
            &self,
            _run_id: &RunId,
        ) -> Result<Option<crate::subagent::RunLineage>, StorageError> {
            Ok(None)
        }
    }

    #[test]
    fn failed_commit_yields_error_and_no_events() {
        let port = FakePort {
            fail_commits: true,
            committed: std::sync::Mutex::new(Vec::new()),
        };
        let result = futures_block_on(port.commit_run_outcome(&ctx(), completed_outcome(), 42));
        match result {
            Err(StorageError::Io { detail }) => assert!(detail.contains("injected")),
            other => panic!("expected Io error, got {other:?}"),
        }
        assert!(
            port.committed.lock().expect("fake port lock").is_empty(),
            "nothing may be durably committed on failure"
        );
    }

    #[test]
    fn successful_commit_returns_events_for_post_commit_publication() {
        let port = FakePort {
            fail_commits: false,
            committed: std::sync::Mutex::new(Vec::new()),
        };
        let ctx = ctx();
        let outcome = port.commit_run_outcome(&ctx, completed_outcome(), 42);
        let committed = futures_block_on(outcome).expect("commit succeeds");
        assert!(committed.newly_committed);
        assert_eq!(committed.events.len(), 1);
        // Idempotent replay of the identical outcome.
        let replay = futures_block_on(port.commit_run_outcome(&ctx, completed_outcome(), 43))
            .expect("replay succeeds");
        assert!(!replay.newly_committed, "identical replay is idempotent");
    }

    #[test]
    fn non_terminal_outcome_is_rejected_before_storage() {
        let bad = RunOutcome {
            status: RunStatus::Running,
            ..completed_outcome()
        };
        let err = bad.validate_terminal().unwrap_err();
        assert!(matches!(err, StorageError::InvalidRequest { .. }));
    }

    #[test]
    fn principal_storage_vocabulary_is_stable() {
        assert_eq!(Principal::LocalUser.storage_kind(), "local_user");
        assert_eq!(Principal::LocalUser.storage_subject(), "user_local");
        let device = Principal::Device {
            device_id: "dev-1".to_string(),
            user_id: "user_9".to_string(),
        };
        assert_eq!(device.storage_kind(), "device");
        assert_eq!(device.storage_subject(), "user_9");
    }

    #[test]
    fn retryable_classification_is_conservative() {
        assert!(StorageError::QueueFull.retryable());
        assert!(StorageError::Busy { timeout_ms: 5 }.retryable());
        assert!(StorageError::DiskFull {
            detail: String::new()
        }
        .retryable());
        assert!(!StorageError::Io {
            detail: String::new()
        }
        .retryable());
        assert!(!StorageError::Conflict {
            detail: String::new()
        }
        .retryable());
        assert!(!StorageError::DatabaseTooNew {
            found_version: 9,
            supported_version: 1
        }
        .retryable());
    }

    /// R03-T03: the non-terminal state-change port surface only accepts
    /// transitions the kernel state machine permits, and never a terminal
    /// target (terminals belong to `commit_run_outcome` alone).
    #[test]
    fn non_terminal_state_change_surface_is_kernel_gated() {
        let port = FakePort {
            fail_commits: false,
            committed: std::sync::Mutex::new(Vec::new()),
        };
        let ctx = ctx();
        let change = |from: RunStatus, to: RunStatus| {
            futures_block_on(port.record_run_state_change(&ctx, from, to, None, 1))
        };
        // Legal active legs: running <-> waiting_approval and both into
        // cancelling (the durable first half of two-phase cancellation).
        for (from, to) in [
            (RunStatus::Running, RunStatus::WaitingApproval),
            (RunStatus::WaitingApproval, RunStatus::Running),
            (RunStatus::Running, RunStatus::Cancelling),
            (RunStatus::WaitingApproval, RunStatus::Cancelling),
            (RunStatus::Queued, RunStatus::Cancelling),
        ] {
            assert!(
                change(from, to).is_ok(),
                "{from:?}->{to:?} must be a legal non-terminal phase change"
            );
        }
        // Terminal targets are refused on THIS surface…
        for to in [
            RunStatus::Cancelled,
            RunStatus::Completed,
            RunStatus::Failed,
        ] {
            assert!(
                change(RunStatus::Cancelling, to).is_err(),
                "terminal target {to:?} belongs to commit_run_outcome"
            );
        }
        // …and so is every illegal active leg.
        assert!(change(RunStatus::Cancelling, RunStatus::Running).is_err());
        assert!(change(RunStatus::Completed, RunStatus::Cancelling).is_err());
    }

    /// R03-T04: the identity fence every asynchronous result carries. The
    /// writer-side check (`matches_ctx`) admits only the CURRENT run,
    /// attempt and generation — an older attempt, another run or another
    /// generation is stale by construction.
    #[test]
    fn result_fence_admits_only_the_current_identity_triple() {
        let ctx = ctx(); // run r-1, attempt a-1, generation 7
        assert!(
            ResultFence::of_ctx(&ctx).matches_ctx(&ctx),
            "an echoed fence matches its own context"
        );
        // Older attempt (the retry fence): stale.
        let retry = RunContext {
            attempt: lingxi_protocol::AttemptId::new("a-0".to_string()),
            ..ctx.clone()
        };
        assert!(!ResultFence::of_ctx(&retry).matches_ctx(&ctx));
        // Newer attempt than the writer holds: also stale (the fence must
        // name the writer's CURRENT attempt, not merely any later one).
        let newer = RunContext {
            attempt: lingxi_protocol::AttemptId::new("a-2".to_string()),
            ..ctx.clone()
        };
        assert!(!ResultFence::of_ctx(&newer).matches_ctx(&ctx));
        // Another run entirely: stale.
        let other_run = RunContext {
            run_id: RunId::new("r-2".to_string()),
            ..ctx.clone()
        };
        assert!(!ResultFence::of_ctx(&other_run).matches_ctx(&ctx));
        // Another registry generation: stale (R04's tool-registry fence
        // rides the same triple).
        let other_gen = RunContext {
            generation: 8,
            ..ctx.clone()
        };
        assert!(!ResultFence::of_ctx(&other_gen).matches_ctx(&ctx));
        // The convenience carriers echo the context they wrap.
        let turn = ProviderTurnResult::of_ctx(
            &ctx,
            ProviderTurn::Continue {
                process_note: "x".to_string(),
            },
        );
        assert!(turn.fence.matches_ctx(&ctx));
        let tool = ToolExecutionResult::of_ctx(&ctx, ToolOutcome::Cancelled);
        assert!(tool.fence.matches_ctx(&ctx));
    }

    #[test]
    fn late_result_reason_vocabulary_is_stable() {
        assert_eq!(LateResultReason::RunTerminal.name(), "run_terminal");
        assert_eq!(LateResultReason::AttemptStale.name(), "attempt_stale");
        assert_eq!(
            LateResultReason::AttemptNeverOpened.name(),
            "attempt_never_opened"
        );
        assert_eq!(LateResultReason::FenceMismatch.name(), "fence_mismatch");
        assert_eq!(
            LateResultReason::CancelledBeforeWrite.name(),
            "cancelled_before_write"
        );
    }

    /// Minimal block_on for the trait-level tests (no runtime dependency in
    /// the kernel: poll the future to completion on the current thread).
    fn futures_block_on<F: std::future::Future>(fut: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        // SAFETY: the data pointer is null and every callback is a no-op;
        // these futures never suspend, so the waker is never used to wake.
        unsafe fn noop_clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        unsafe fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(fut);
        // These futures never suspend; a single poll must complete them.
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => v,
            Poll::Pending => {
                panic!("kernel test future suspended; expected immediate readiness")
            }
        }
    }
}
