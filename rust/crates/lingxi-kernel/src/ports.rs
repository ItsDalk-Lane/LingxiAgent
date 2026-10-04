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
    ContentBlock, EventEnvelope, EventId, EventPayload, ModelCallId, NormalizedMessage,
    ProtocolError, RunId, RunStatus, Seq, SessionId, ToolCallId, UsageRecord,
};
// ProtocolError is the error surface of the provider/tool ports below;
// StoragePort deliberately uses the richer StorageError.

use crate::model_exchange::ModelTurnInput;
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

    // ── R05-T07: the usage ledger ───────────────────────────────────────────
    //
    // One row per MODEL CALL (host-minted call identity): identity,
    // route, purpose, causal parentage and the normalized usage fact.
    // Written BEFORE the call's completion event is published; a failure
    // here is a real storage failure (never a silent accounting loss and
    // never a published success — T07-C10).

    /// Durably records ONE model call's usage/trace fact. Always legal
    /// (a usage row is an accounting fact, not a run-state transition —
    /// auxiliary/worker/operation calls have no live run row of their
    /// own). Idempotent: re-recording the IDENTICAL row (same
    /// `model_call_id`, same content) is a replay; a DIFFERENT row under
    /// the same `model_call_id` is a loud [`StorageError::Conflict`] (a
    /// call's accounting is never rewritten).
    fn record_model_call_usage(
        &self,
        record: crate::usage::ModelCallUsageRecord,
        now_unix_ms: u64,
    ) -> impl std::future::Future<Output = Result<(), StorageError>> + Send;

    /// Queries the usage ledger. Rows come back oldest-first. The
    /// `owner_user_id` scope of [`crate::usage::ModelUsageQuery`] is
    /// applied against the session owner (authorization isolation is part
    /// of the read — T07-C09); a scope naming nothing returns an empty
    /// vec, never another principal's rows.
    fn query_model_call_usage(
        &self,
        query: crate::usage::ModelUsageQuery,
    ) -> impl std::future::Future<
        Output = Result<Vec<crate::usage::ModelCallUsageRecord>, StorageError>,
    > + Send;
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
    /// execute (a model call ending here does NOT end the run). `content`
    /// carries the turn's own content blocks (text / reasoning /
    /// provider-opaque) — a mixed response (content AND tool calls, the
    /// real shape of OpenAI/Anthropic turns) keeps every block for the
    /// exchange history instead of dropping the content half.
    ToolRequests {
        requests: Vec<ToolRequest>,
        content: Vec<ContentBlock>,
    },
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
///
/// R05-T01: `usage` carries the provider-reported token usage of the call
/// when the protocol reports one (the OpenAI `usage` object). `None` means
/// the provider did not report usage — the driver persists `None`, never a
/// fabricated estimate.
///
/// R05-T07: `usage_report` carries the RICHER usage fact (component tokens,
/// provenance, invalid-vs-unknown) the usage ledger persists; `usage`
/// remains the frozen wire projection. `served_protocol` names the protocol
/// family that actually served the call (the ledger's `protocol` column —
/// the descriptor deliberately stays the wire-visible triple).
/// `transport_attempts` counts the PHYSICAL provider requests the logical
/// call sent (>= 1; a 401-refresh resend is a second billable request —
/// T07-C03).
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderTurnResult {
    pub fence: ResultFence,
    pub turn: ProviderTurn,
    pub usage: Option<UsageRecord>,
    /// The richer usage fact of the usage ledger (R05-T07). Defaults to
    /// `Unknown`; the driver synthesizes a plain `Reported` fact from
    /// `usage` when only the legacy projection is set (a double that
    /// reported usage IS a reported usage).
    pub usage_report: crate::usage::ReportedUsage,
    /// The protocol family (config vocabulary) that served the call.
    pub served_protocol: Option<String>,
    /// Physical provider requests sent for this logical call (>= 1).
    pub transport_attempts: u32,
    /// The identity of the route that ACTUALLY served this turn, when the
    /// adapter reports it (R05-T01: the real gateway-backed adapter fills it
    /// from the resolved route, so a mid-run config reload never mislabels
    /// the persisted `model_call_*` facts — the driver persists THIS, not a
    /// pre-call guess; doubles leave it `None` and the driver falls back to
    /// the port's static descriptor, which for a deterministic double is
    /// exact).
    pub served_by: Option<ProviderDescriptor>,
}

impl ProviderTurnResult {
    /// A result echoing the context it was issued under (the honest default
    /// every adapter uses unless it is delivering a late/raced result).
    /// Usage defaults to `None` (not reported), `served_by` to `None`
    /// (the caller falls back to the port descriptor).
    pub fn of_ctx(ctx: &RunContext, turn: ProviderTurn) -> Self {
        Self {
            fence: ResultFence::of_ctx(ctx),
            turn,
            usage: None,
            usage_report: crate::usage::ReportedUsage::Unknown,
            served_protocol: None,
            transport_attempts: 1,
            served_by: None,
        }
    }

    /// The same echo with the provider-reported usage attached.
    pub fn of_ctx_with_usage(ctx: &RunContext, turn: ProviderTurn, usage: UsageRecord) -> Self {
        Self {
            fence: ResultFence::of_ctx(ctx),
            turn,
            usage_report: crate::usage::ReportedUsage::Known(usage.clone().into()),
            usage: Some(usage),
            served_protocol: None,
            transport_attempts: 1,
            served_by: None,
        }
    }

    /// Attaches the serving route's identity (see the field docs).
    pub fn with_served_by(mut self, descriptor: ProviderDescriptor) -> Self {
        self.served_by = Some(descriptor);
        self
    }

    /// Attaches the richer usage fact + the wire projection together (the
    /// only honest way to set them: the projection is DERIVED from the
    /// fact, never independently asserted).
    pub fn with_usage_report(mut self, report: crate::usage::ReportedUsage) -> Self {
        self.usage = match &report {
            crate::usage::ReportedUsage::Known(usage) => usage.wire_record(),
            _ => None,
        };
        self.usage_report = report;
        self
    }

    /// Marks a second physical provider request (the single 401-refresh
    /// resend) — T07-C03: the possibly-billable request stays countable.
    pub fn with_transport_attempts(mut self, attempts: u32) -> Self {
        self.transport_attempts = attempts.max(1);
        self
    }

    /// Names the protocol family that served the call.
    pub fn with_served_protocol(mut self, protocol: impl Into<String>) -> Self {
        self.served_protocol = Some(protocol.into());
        self
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
///
/// R04-T01: the request carries the COMPLETE effective arguments
/// ([`crate::toolcatalog::EffectiveArguments`]) — the R03 digest-only shape
/// was a test boundary, not the contract executors consume. `args_digest`
/// is DERIVED from `arguments` by the trusted boundary that built the
/// request (the R04 gateway / a provider adapter), and the run driver
/// re-verifies the pair on every call
/// ([`ToolRequest::digest_matches_arguments`]): a self-filled digest for
/// OTHER arguments is a loud protocol violation with zero dispatch.
// NOTE: not `Eq` — `ArgsDigest` follows the wire digest struct.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRequest {
    /// Registry target id (R04's [`crate::toolcatalog::ToolRegistry`]
    /// owns the real target identities; R03 doubles carry a stable target
    /// name).
    pub target: String,
    /// The complete, immutable, validated + normalized argument object
    /// the executor consumes (R04-T01).
    pub arguments: crate::toolcatalog::EffectiveArguments,
    /// Digest of `arguments` (the value approvals bind to), computed by
    /// the trusted boundary that produced the request.
    pub args_digest: lingxi_protocol::ArgsDigest,
    /// Bounded diagnostic summary. UNTRUSTED when it arrives with provider
    /// output: the trusted boundary's own summaries are shape-only
    /// ([`crate::toolcatalog::summarize_arguments`]).
    pub args_summary: Option<String>,
    /// The provider's own correlation id for this call (R05-T01; e.g. an
    /// OpenAI `tool_calls[].id`). Protocol correlation data ONLY: it never
    /// binds journal/receipt/state writes (the driver-minted
    /// [`ToolCallId`] is the only host identity); the exchange history
    /// carries it so the adapter can emit the protocol-required pairing on
    /// the result message.
    pub provider_call_id: Option<String>,
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

impl ToolRequest {
    /// Builds a request from ALREADY-EFFECTIVE arguments, deriving the
    /// digest from them (the trusted-boundary path: provider doubles in
    /// tests, the R04 gateway and R05 adapters in production). `None`
    /// when the value is not a JSON object or violates the canonical
    /// invariants — a loud refusal, never a guessed default.
    pub fn from_effective_arguments(
        target: impl Into<String>,
        arguments: serde_json::Value,
        budget: &crate::toolcatalog::SchemaBudget,
    ) -> Option<Self> {
        let arguments =
            crate::toolcatalog::EffectiveArguments::from_value(arguments, budget).ok()?;
        let args_digest = arguments.digest();
        Some(Self {
            target: target.into(),
            arguments,
            args_digest,
            args_summary: None,
            provider_call_id: None,
            delegation: None,
        })
    }

    /// Attaches a diagnostic summary (untrusted at the model boundary;
    /// shape-only when produced by the catalog boundary).
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.args_summary = Some(summary.into());
        self
    }

    /// Attaches the provider's own correlation id (R05-T01; protocol
    /// correlation data only — never a host identity).
    pub fn with_provider_call_id(mut self, provider_call_id: impl Into<String>) -> Self {
        self.provider_call_id = Some(provider_call_id.into());
        self
    }

    /// Attaches the subagent-family delegation payload (R03-T06).
    pub fn with_delegation(mut self, delegation: DelegationRequest) -> Self {
        self.delegation = Some(delegation);
        self
    }

    /// Whether the stored digest is exactly the digest of the stored
    /// effective arguments. The run driver checks this before journaling
    /// anything: a mismatch is a forged/adulterated request.
    pub fn digest_matches_arguments(&self) -> bool {
        self.arguments.digest_matches(&self.args_digest)
    }
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

/// One in-flight increment of a model turn (R05-T04). The provider emits
/// these through [`TurnDeltaSink`] AS THE NETWORK STREAM DELIVERS them —
/// never buffered to the end and re-sliced (the fake-streaming shape the
/// stage explicitly forbids). Deltas are PROGRESS facts only: they carry
/// no tool arguments, no signatures and no usage (tools dispatch solely
/// from the terminal turn's complete batch — R05-T04 batch admission —
/// and usage rides the final [`ProviderTurnResult`]).
///
/// The driver's event bridge normalizes this stream into the wire
/// vocabulary (`model_call_delta` / `assistant_segment_*`); the terminal
/// [`ProviderTurn`] stays the authority for history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelTurnDelta {
    /// A visible-body text increment of the current text block. In-band
    /// reserved tags (`<think>`, `<mood>`, …) are NOT the adapter's
    /// concern — the host-side normalization layer structures them; the
    /// adapter passes the text through verbatim.
    Text(String),
    /// A reasoning/thinking increment of the current PROVIDER-NATIVE
    /// reasoning block (anthropic `thinking_delta`, openai
    /// `reasoning_content`, google `thought` parts, responses reasoning
    /// summary deltas).
    Reasoning(String),
}

/// The sink's receiver is gone (the run settled or was cancelled while
/// the provider was mid-stream). The provider must STOP reading the
/// network stream and wind the call down — it never buffers the remainder
/// "for later" and never blocks on a dead driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnDeltaSinkClosed;

impl std::fmt::Display for TurnDeltaSinkClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the turn delta sink is closed (the run driver is gone); the provider must stop \
             reading the stream"
        )
    }
}

impl std::error::Error for TurnDeltaSinkClosed {}

/// The live-delta half of the provider contract (R05-T04): the run
/// driver's end of one in-flight model turn. Implementations are
/// bounded-channel senders — `emit` applies backpressure (the provider
/// awaits capacity), so a slow driver slows the network read instead of
/// growing memory without bound.
///
/// Object safety: `emit` returns a boxed future so `&dyn TurnDeltaSink`
/// can cross the `Arc<dyn TurnProviderPort>` boundary.
pub trait TurnDeltaSink: Send + Sync {
    /// Emits one delta. `Err(TurnDeltaSinkClosed)` once the driver is
    /// gone — a terminal condition for the stream, never retried.
    fn emit<'a>(
        &'a self,
        delta: ModelTurnDelta,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TurnDeltaSinkClosed>> + Send + 'a>,
    >;
}

/// Provider access for the run driver (R03-T01; evolved to the typed
/// per-turn exchange at R05-T01; gained the live-delta sink at R05-T04).
///
/// R03/R04 wired deterministic doubles through the REAL run chain with a
/// bare `&str` input; R05-T01 replaces that parameter list with the typed
/// [`ModelTurnInput`] (submission + complete prior exchange + the send-time
/// tool declaration snapshot + deadline facts) — there is ONE signature and
/// real protocol adapters sit behind this same port. Every call is pinned
/// to its run/attempt context and its own `ModelCallId`.
///
/// The futures are boxed so the trait is object-safe: the supervisor holds
/// `Arc<dyn TurnProviderPort>` (one injection point, doubles in tests /
/// real adapters in R05).
pub trait TurnProviderPort: Send + Sync {
    /// Stable identity of what this provider simulates / serves.
    fn descriptor(&self) -> ProviderDescriptor;

    /// Produces the next model turn for the run. `input` is the typed
    /// per-turn exchange ([`ModelTurnInput`]: the run's submission with
    /// drained steering, the 1-based turn index, the complete prior
    /// exchange of this attempt, and the tool declaration snapshot taken
    /// from the live registry at send time). The result carries the
    /// identity fence of the request it answers; the driver verifies it
    /// against the CURRENT context before any state write (R03-T04).
    ///
    /// `deltas` is the live-progress half (R05-T04): a streaming provider
    /// emits [`ModelTurnDelta`]s through it AS THE WIRE DELIVERS them. A
    /// provider whose protocol is not streamed simply never emits — that
    /// is an explicit protocol fact, not a fallback (the terminal turn
    /// stays complete and authoritative either way). An `Err(
    /// TurnDeltaSinkClosed)` from `emit` means the driver is gone: stop
    /// reading, wind down, let the fence classify the late result.
    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        deltas: &'a dyn TurnDeltaSink,
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
///
/// R04-T01: `Success` carries the CONSUMABLE structured result
/// ([`ToolSuccess`]) — actual content blocks, resource references,
/// truncation and the process run/exit status. The R03 shape (a bare
/// content digest) was a test boundary; the digest survives as the
/// integrity/audit value (the journal receipt's dedup id), while the run
/// driver, the steering payloads and R05 adapters read the real content.
// NOTE: not `Eq` — it carries `ProtocolError`, whose `details` holds
// arbitrary JSON values.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolOutcome {
    Success { result: ToolSuccess },
    Failed { error: ProtocolError },
    Cancelled,
    Unknown { reason: String },
}

/// The structured success payload of one tool call (R04-T01). Everything
/// here is consumable data; `content_digest` is derived by this boundary
/// over the canonical content and never replaces it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSuccess {
    /// Actual content blocks of the result (text, resource refs, provider
    /// opaque payloads).
    pub content: Vec<lingxi_protocol::ContentBlock>,
    /// Real resources the call produced/touched (their existence and
    /// authorization belong to the resource layer; a model claim never
    /// mints one).
    pub resource_refs: Vec<lingxi_protocol::ResourceRef>,
    /// Whether the content was truncated at a boundary. A truncated
    /// result MUST set this — silent truncation is forbidden.
    pub truncated: bool,
    /// Process-family execution status (exit code / still-running /
    /// unconfirmed-stop handle). `None` for non-process tools.
    /// "Started/running" is a status here, never a disguised success
    /// claim (T05 owns the full semantics). Boxed: cold metadata whose
    /// inline growth would bloat every `Result<_, ToolOutcome>` helper
    /// past the large-Err bound.
    pub status: Option<Box<ToolRunStatus>>,
    /// SHA-256 of the canonical content blocks — the audit/dedup value.
    pub content_digest: String,
}

/// Run/exit status of a process-family tool call (R04-T01; the full
/// PTY/process lifecycle lands with T05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolRunStatus {
    /// The process exited with this code. ONLY from a REAL observation
    /// (a resolved `wait()`/trusted system receipt): an observed
    /// SIGKILL is the legitimate `code: 137`; a stop that was merely
    /// REQUESTED never claims this variant (R04-RR1-F03).
    Exited { code: i64 },
    /// The process is still running under this opaque handle (its
    /// lifetime and cancellation belong to the process supervisor).
    Running { handle: String },
    /// A stop was requested (watchdog timeout / close / caller drop)
    /// but the exit was NOT observed as a trusted receipt within the
    /// bound — or the wait itself failed. The process's final state is
    /// unconfirmed: this is neither "exited", "killed" nor a confirmed
    /// failure, and the caller must not assume the absence of further
    /// side effects. The referenced handle stays queryable; a late REAL
    /// observation lands on the process record — never silently
    /// upgraded here (R04-RR1-F03).
    StopUnconfirmed { handle: String, detail: String },
}

impl ToolSuccess {
    /// Builds a success payload from content blocks, deriving the digest
    /// over their canonical form.
    pub fn from_content(content: Vec<lingxi_protocol::ContentBlock>) -> Self {
        let value = serde_json::to_value(&content).expect("content blocks serialize to JSON");
        let canonical = lingxi_protocol::canon::canonical_json_bytes(&value);
        Self {
            content,
            resource_refs: Vec::new(),
            truncated: false,
            status: None,
            content_digest: lingxi_protocol::canon::sha256_hex(&canonical),
        }
    }

    /// The minimal text-content success (the common shape).
    pub fn text(text: impl Into<String>) -> Self {
        Self::from_content(vec![lingxi_protocol::ContentBlock::Text {
            text: text.into(),
        }])
    }
}

impl ToolOutcome {
    /// A successful outcome carrying one text content block whose digest
    /// is derived from the content itself.
    pub fn success_text(text: impl Into<String>) -> Self {
        ToolOutcome::Success {
            result: ToolSuccess::text(text),
        }
    }
}

/// HISTORICAL (R00 era) — zero implementations and zero call sites in the
/// workspace (REV-T02 R01, grep-verified). The LIVE credential path is
/// `ProviderCredentialPort` in `lingxi-adapters` (`models::credentials`),
/// implemented by the service-side `CredentialService`; credential material
/// never enters the kernel — only `model_exchange::CredentialReference`
/// (identity + kind) does. Retained pending the R05-T08 keep-or-remove
/// ruling; do not build new code on this trait.
///
/// Original contract: server-side resolution of provider credentials; only
/// the service layer may hold credential material, the kernel sees
/// resolved, scoped handles.
#[deprecated(
    note = "R00-era remnant with no implementors; the live path is lingxi-adapters' \
            ProviderCredentialPort — removal is an R05-T08 ruling"
)]
// The signature references the historical `CredentialHandle` below, which
// stays non-deprecated so this definition body itself warns on nothing;
// the trait's own `deprecated` mark is the fence for any new use.
#[allow(deprecated)]
pub trait CredentialPort {
    fn resolve_provider_credential(
        &self,
        ctx: &RunContext,
        provider: &str,
    ) -> Result<CredentialHandle, ProtocolError>;
}

/// HISTORICAL companion of the deprecated [`CredentialPort`] above (R00 era,
/// zero use sites beyond that trait's own signature — kept warning-free by
/// `allow(deprecated)` there). The live handle type is `CredentialHandle` in
/// `lingxi-service` (`credentials`), backed by the service registry.
/// Opaque, scoped credential handle; contains no secret material.
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

        async fn record_model_call_usage(
            &self,
            _record: crate::usage::ModelCallUsageRecord,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            Ok(())
        }

        async fn query_model_call_usage(
            &self,
            _query: crate::usage::ModelUsageQuery,
        ) -> Result<Vec<crate::usage::ModelCallUsageRecord>, StorageError> {
            Ok(Vec::new())
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
