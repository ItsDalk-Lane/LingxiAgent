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
// ProtocolError is still the error surface of the model/tool/credential
// ports below; StoragePort deliberately uses the richer StorageError.

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
            StorageError::Internal { detail } => write!(f, "storage internal error: {detail}"),
        }
    }
}

impl std::error::Error for StorageError {}

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

/// Model provider access. Every request is pinned to
/// principal/run/attempt/modelCall/purpose/provider/model/operation/
/// budget/deadline by the kernel; credentials are resolved server-side
/// through [`CredentialPort`], never by callers and never by workers.
pub trait ModelPort {
    fn complete(
        &self,
        ctx: &RunContext,
        call: ModelCallId,
        request_digest: &str,
    ) -> Result<String, ProtocolError>;
}

/// Execution of one prepared, authorized tool invocation.
///
/// The kernel produces a `PreparedInvocation` internally; it is not
/// forgeable from model output. Cross-worker transfer uses short-lived
/// authorization tickets verified by the host — a JSON field saying
/// `"approved": true` is never an authorization.
pub trait ToolPort {
    fn execute_prepared(
        &self,
        ctx: &RunContext,
        call: ToolCallId,
        prepared_digest: &str,
    ) -> Result<ToolOutcome, ProtocolError>;
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
