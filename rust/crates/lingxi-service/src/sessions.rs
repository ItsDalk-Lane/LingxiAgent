//! Representative business surface: the session/read+execute endpoints
//! backed by the REAL run/message database (R02-T04 replaces the R02-T03
//! in-memory minimal face; endpoint shapes — ids, ownership, run records —
//! are the part future tasks keep).
//!
//! Design boundary (taskbook): every call still runs the real transport
//! guard → authentication → route authorization → per-resource ownership
//! chain; the write path now persists through the kernel's [`StoragePort`]
//! (endpoint → kernel port → adapters SQLite implementation → bounded
//! single-writer queue): a successful execute commits a run row + attempt
//! row + `run_state_changed` key events durably, and a FAILED commit
//! surfaces an explicit error with no visible success (R02-A07).
//!
//! Every session is owned by a user id; the local owner (loopback token)
//! sees everything, a device principal only its own user's sessions — the
//! cross-principal boundary that acceptance R02-A05 exercises with a
//! foreign sessionId.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use lingxi_adapters::storage::{RunDatabase, RunSummaryRow, SessionRow};
use lingxi_kernel::ports::{StorageError, StoragePort};
use lingxi_kernel::Principal as KernelPrincipal;
use lingxi_protocol::RunStatus;

use crate::auth::{Principal, PrincipalKind, LOCAL_OWNER_USER_ID};
use crate::cancel::CancelPhase;
use crate::events::EventService;
use crate::session_supervisor::{SessionConcurrencyLimits, SessionSupervisor, SteerOutcome};

/// Summary shape returned by the read endpoint (runs carry only committed
/// facts; no echo of failed attempts).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session_id: String,
    pub agent_id: String,
    pub owner_user_id: String,
    pub title: String,
    pub run_count: u64,
    pub last_runs: Vec<RunSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub run_id: String,
    pub principal_id: String,
    pub started_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteAccepted {
    pub run_id: String,
    pub run_count: u64,
    /// R03-T04/A08: `true` when this response is an idempotent REPLAY of an
    /// earlier accepted submission with the same explicit requestId and the
    /// same normalized content — nothing was re-executed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replayed: bool,
}

/// Outcome of an authorized cancellation request against one run
/// (R03-T03). Every leg is explicit — the frozen incumbent
/// `abortSession` shape (pre-prompt abort / streaming force-release /
/// no-op `false`) maps onto: live run → [`CancelRunOutcome::Accepted`],
/// settled run → [`CancelRunOutcome::AlreadyTerminal`], active-but-
/// driverless row → [`CancelRunOutcome::DanglingActive`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelRunOutcome {
    /// The cancellation fired on the live run's tree (first request
    /// wins; the durable `cancelling` leg, the bounded child cleanup and
    /// the single `cancelled` finalize are driven by the run's driver).
    Accepted { run_id: String, phase: CancelPhase },
    /// The run already holds a terminal state: nothing was cancelled
    /// (mirrors the incumbent's no-op `false` — diagnosable, not busy).
    AlreadyTerminal { run_id: String, status: RunStatus },
    /// The durable row is ACTIVE but NO live driver exists in this
    /// process (e.g. the process restarted, or the driving future
    /// disappeared before finalize). The explainable state is reported
    /// honestly; classifying/recovering such rows is R03-T07's startup
    /// scan — nothing here fabricates a terminal.
    DanglingActive {
        run_id: String,
        status: RunStatus,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecuteRequest {
    /// Free-form input; bounded by the body limit. Any identity-ish fields
    /// are structurally rejected here (deny_unknown_fields) — principal
    /// comes from the auth chain, never from the payload.
    pub input: String,
    /// OPTIONAL explicit idempotency key (R03-T04/A08). When present, the
    /// submission is deduplicated against the recorded normalized-content
    /// digest: same id + same content → idempotent replay of the original
    /// acceptance; same id + changed content → an explicit conflict (the
    /// old execution is not reused, a new one does not start). The key is
    /// bound to the CALLING principal and THIS session — never a global
    /// namespace. Absent (the default) keeps the plain submission path.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// One execute submission as the service surface consumes it (R03-T04):
/// the input plus the OPTIONAL explicit idempotency key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecuteSubmission<'a> {
    pub input: &'a str,
    pub request_id: Option<&'a str>,
}

impl<'a> ExecuteSubmission<'a> {
    /// The plain (non-idempotent) submission shape — the exact pre-T04
    /// behavior.
    pub fn plain(input: &'a str) -> Self {
        Self {
            input,
            request_id: None,
        }
    }
}

/// Outcome of a store lookup.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionAccess {
    Ok(SessionFacts),
    NotFound,
    Forbidden,
}

/// The committed facts of one session (projection of the run database).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionFacts {
    pub session_id: String,
    pub agent_id: String,
    pub owner_user_id: String,
    pub title: String,
    pub run_count: u64,
    pub last_runs: Vec<RunSummary>,
}

/// Error of the execute mutation.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionExecuteError {
    NotFound,
    Forbidden,
    /// The session already owns a running task: the FROZEN incumbent
    /// `session_busy` gate (R03-T02). The submission was NOT accepted and
    /// wrote NOTHING — retry later or steer the running turn instead.
    Busy,
    /// The tracked-session registry is at its hard cap (service
    /// protection; nothing was written).
    SessionRegistryFull,
    /// A steering submission overflowed the bounded steering inbox
    /// (nothing was accepted).
    SteeringInboxFull,
    /// R03-A08: this explicit requestId was already accepted with DIFFERENT
    /// normalized content. The recorded execution is NOT reused for the new
    /// content and NO new execution starts — an explicit conflict the
    /// client must resolve (new id, or the original content).
    DuplicateRequestConflict {
        request_id: String,
        recorded_digest: String,
        submitted_digest: String,
    },
    /// The idempotency-key registry is at its hard cap (service protection;
    /// nothing was written, nothing was executed).
    IdempotencyRegistryFull {
        cap: usize,
    },
    /// The explicit requestId failed validation (empty / over-long). The
    /// submission was refused before any side effect.
    InvalidRequestId {
        detail: String,
    },
    /// The storage port refused or failed: no visible success was produced
    /// (R02-A07). Carries the domain storage error for the endpoint's
    /// error surface.
    Storage(StorageError),
}

/// Storage reads the session surface needs. Implemented by
/// [`RunDatabase`] (real SQLite store) and by the in-memory fake in tests.
pub trait SessionBackend: Send + Sync {
    fn get_session(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<Option<SessionRow>, StorageError>> + Send;
    fn list_sessions(&self) -> impl Future<Output = Result<Vec<SessionRow>, StorageError>> + Send;
    fn count_runs(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<u64, StorageError>> + Send;
    fn total_runs(&self) -> impl Future<Output = Result<u64, StorageError>> + Send;
    fn recent_runs(
        &self,
        session_id: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<RunSummaryRow>, StorageError>> + Send;
    /// Allocates the run id for a NEW execute (R02 stage-repair R1 / F01).
    /// This is the ONLY legitimate source of run ids on this surface: an
    /// atomic, durably-seeded per-database counter — never a `total_runs()
    /// + 1` read, which races under concurrency and let distinct
    /// submissions collapse into one run. Two calls never return the same
    /// id, including at an identical `now_ms` and across restarts.
    /// R5-F02: exhaustion surfaces as an explicit
    /// [`StorageError::RunIdExhausted`] (never a panic/wrap/re-issue).
    fn allocate_run_id(&self, now_ms: u64) -> Result<String, StorageError>;
}

/// Object-safe erasure of [`SessionBackend`] (RPITIT traits are not
/// dyn-compatible; the blanket impl forwards).
pub trait SessionBackendErased: Send + Sync {
    fn get_session_erased<'a>(
        &'a self,
        session_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<SessionRow>, StorageError>> + Send + 'a>>;
    fn list_sessions_erased(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SessionRow>, StorageError>> + Send + '_>>;
    fn count_runs_erased<'a>(
        &'a self,
        session_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<u64, StorageError>> + Send + 'a>>;
    fn total_runs_erased(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<u64, StorageError>> + Send + '_>>;
    fn recent_runs_erased<'a>(
        &'a self,
        session_id: &'a str,
        limit: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<RunSummaryRow>, StorageError>> + Send + 'a>>;
    fn allocate_run_id_erased(&self, now_ms: u64) -> Result<String, StorageError>;
}

impl<T: SessionBackend> SessionBackendErased for T {
    fn get_session_erased<'a>(
        &'a self,
        session_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<SessionRow>, StorageError>> + Send + 'a>> {
        Box::pin(SessionBackend::get_session(self, session_id))
    }
    fn list_sessions_erased(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SessionRow>, StorageError>> + Send + '_>> {
        Box::pin(SessionBackend::list_sessions(self))
    }
    fn count_runs_erased<'a>(
        &'a self,
        session_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<u64, StorageError>> + Send + 'a>> {
        Box::pin(SessionBackend::count_runs(self, session_id))
    }
    fn total_runs_erased(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<u64, StorageError>> + Send + '_>> {
        Box::pin(SessionBackend::total_runs(self))
    }
    fn recent_runs_erased<'a>(
        &'a self,
        session_id: &'a str,
        limit: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<RunSummaryRow>, StorageError>> + Send + 'a>> {
        Box::pin(SessionBackend::recent_runs(self, session_id, limit))
    }
    fn allocate_run_id_erased(&self, now_ms: u64) -> Result<String, StorageError> {
        SessionBackend::allocate_run_id(self, now_ms)
    }
}

impl SessionBackend for RunDatabase {
    fn get_session(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<Option<SessionRow>, StorageError>> + Send {
        RunDatabase::get_session(self, session_id)
    }
    fn list_sessions(&self) -> impl Future<Output = Result<Vec<SessionRow>, StorageError>> + Send {
        RunDatabase::list_sessions(self)
    }
    fn count_runs(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<u64, StorageError>> + Send {
        RunDatabase::count_runs(self, session_id)
    }
    fn total_runs(&self) -> impl Future<Output = Result<u64, StorageError>> + Send {
        RunDatabase::total_runs(self)
    }
    fn recent_runs(
        &self,
        session_id: &str,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<RunSummaryRow>, StorageError>> + Send {
        RunDatabase::recent_runs(self, session_id, limit)
    }
    fn allocate_run_id(&self, now_ms: u64) -> Result<String, StorageError> {
        RunDatabase::allocate_run_id(self, now_ms)
    }
}

pub struct SessionStore {
    backend: Box<dyn SessionBackendErased>,
    /// R03-T02: the session serialization owner — one explicit owner per
    /// session; busy sessions reject further normal submissions (frozen
    /// incumbent `session_busy` gate) and accept steering instead.
    gate: std::sync::Arc<SessionSupervisor>,
    /// R03-T04: the submission-surface requestId dedup (bounded,
    /// principal+session-scoped). Only submissions carrying an explicit id
    /// touch it — the plain path is byte-identical to the pre-T04 flow.
    dedup: crate::dedup::SubmissionDedup,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionStore(<storage-backed>)")
    }
}

impl SessionStore {
    /// Wraps a real storage backend with the production-default session
    /// concurrency policy.
    pub fn new(backend: impl SessionBackend + 'static) -> Self {
        Self::with_concurrency(backend, SessionConcurrencyLimits::default())
    }

    /// Full injection: the session concurrency policy is explicit (the
    /// composition root passes the `ServiceDeps` value; tests inject small
    /// caps through the same path — the gate semantics are identical).
    pub fn with_concurrency(
        backend: impl SessionBackend + 'static,
        limits: SessionConcurrencyLimits,
    ) -> Self {
        Self {
            backend: Box::new(backend),
            gate: SessionSupervisor::new(limits),
            dedup: crate::dedup::SubmissionDedup::default(),
        }
    }

    /// The session serialization owner (observability; also lets callers
    /// submit steering for a running turn).
    pub fn session_supervisor(&self) -> &std::sync::Arc<SessionSupervisor> {
        &self.gate
    }

    /// The synthetic sessions seeded into a fresh database (owned by the
    /// local owner user; identical ids to the R02-T03 in-memory seed so
    /// endpoint consumers see no shape change).
    pub fn seed_rows(now_ms: u64) -> Vec<SessionRow> {
        vec![
            SessionRow {
                session_id: "sess_local_alpha".to_string(),
                agent_id: "lingxi".to_string(),
                owner_user_id: LOCAL_OWNER_USER_ID.to_string(),
                title: "Synthetic session alpha".to_string(),
                created_at_unix_ms: now_ms as i64,
            },
            SessionRow {
                session_id: "sess_local_beta".to_string(),
                agent_id: "lingxi".to_string(),
                owner_user_id: LOCAL_OWNER_USER_ID.to_string(),
                title: "Synthetic session beta".to_string(),
                created_at_unix_ms: now_ms as i64,
            },
        ]
    }

    /// Ownership rule: the local owner sees everything; any other
    /// principal only sessions owned by its own user id.
    pub fn can_access(principal: &Principal, owner_user_id: &str) -> bool {
        if principal.is_local_owner() {
            return true;
        }
        principal.user_id.as_deref() == Some(owner_user_id)
    }

    /// Lists the sessions visible to `principal` (identity projection of
    /// the run database; read-only).
    pub async fn list_for(&self, principal: &Principal) -> Result<Vec<SessionView>, StorageError> {
        let rows = self.backend.list_sessions_erased().await?;
        let mut views = Vec::with_capacity(rows.len());
        for row in rows {
            if !Self::can_access(principal, &row.owner_user_id) {
                continue;
            }
            views.push(self.facts_of(row).await?.into());
        }
        Ok(views)
    }

    /// Reads one session under the ownership rule.
    pub async fn get_for(
        &self,
        principal: &Principal,
        session_id: &str,
    ) -> Result<SessionAccess, StorageError> {
        match self.backend.get_session_erased(session_id).await? {
            None => Ok(SessionAccess::NotFound),
            Some(row) if Self::can_access(principal, &row.owner_user_id) => {
                Ok(SessionAccess::Ok(self.facts_of(row).await?))
            }
            Some(_) => Ok(SessionAccess::Forbidden),
        }
    }

    /// Executes: drives ONE run of the session through the real run
    /// lifecycle (R03-T01) — durable start (queued→running) → model/tool
    /// turns under the [`RunSupervisor`] → exactly one finalize through the
    /// storage port's single settlement transaction — and publishes each
    /// commit's events strictly AFTER the commit returned Ok (R02-T05
    /// wiring of the T04 authority chain). Storage failures surface as
    /// [`SessionExecuteError::Storage`] — no visible success (R02-A07).
    ///
    /// R03-T02 session serialization: the submission first reserves the
    /// session's ONE owner slot ([`SessionSupervisor`]). A busy session
    /// rejects the submission with [`SessionExecuteError::Busy`] — the
    /// frozen incumbent `session_busy` gate — BEFORE any durable side
    /// effect (no run id is allocated, nothing is written). The lease is
    /// RAII: normal settle, error, timeout or cancellation of the drive
    /// all free the session for the next submission.
    // Port/events are passed explicitly (R02 style: the session surface
    // does not own them); adding the supervisor keeps the same shape.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_for<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        supervisor: &crate::runs::RunSupervisor,
        principal: &Principal,
        session_id: &str,
        input: &str,
        now_ms: u64,
    ) -> Result<ExecuteAccepted, SessionExecuteError> {
        self.execute_submission_for(
            port,
            events,
            supervisor,
            principal,
            session_id,
            &ExecuteSubmission::plain(input),
            now_ms,
        )
        .await
    }

    /// The execute surface with the OPTIONAL explicit requestId (R03-T04 /
    /// A08). Submissions WITHOUT an id take the exact pre-T04 path;
    /// submissions WITH an id are deduplicated against the recorded
    /// normalized-content digest, scoped to the CALLING principal and THIS
    /// session:
    /// - same id + same digest → idempotent REPLAY of the original
    ///   acceptance (the original run id is returned, `replayed: true`,
    ///   nothing re-executed — legal even while the original run is still
    ///   driving, which is precisely the lost-response retry a client
    ///   performs);
    /// - same id + changed digest → [`SessionExecuteError::DuplicateRequestConflict`]:
    ///   the recorded execution is NOT reused for the new content and NO
    ///   new execution starts;
    /// - fresh id → the admission (busy gate + run-id allocation + the
    ///   id→run binding) is serialized per key, so a concurrent duplicate
    ///   cannot slip a second admission in between.
    ///
    /// The idempotency registry is process-memory and bounded (restart
    /// semantics belong to R03-T07; a post-restart retry is a fully
    /// validated fresh submission).
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_submission_for<P: StoragePort>(
        &self,
        port: &P,
        events: &EventService,
        supervisor: &crate::runs::RunSupervisor,
        principal: &Principal,
        session_id: &str,
        submission: &ExecuteSubmission<'_>,
        now_ms: u64,
    ) -> Result<ExecuteAccepted, SessionExecuteError> {
        let row = match self.backend.get_session_erased(session_id).await {
            Ok(Some(row)) => row,
            Ok(None) => return Err(SessionExecuteError::NotFound),
            Err(err) => return Err(SessionExecuteError::Storage(err)),
        };
        if !Self::can_access(principal, &row.owner_user_id) {
            return Err(SessionExecuteError::Forbidden);
        }

        // R03-T04 admission. The synchronous admission closure acquires the
        // session's single owner slot (R03-T02 busy gate) and allocates the
        // run id; for explicit-id submissions the closure runs UNDER the
        // dedup registry's key lock, so a concurrent same-id duplicate can
        // neither slip a second admission in between nor observe a
        // half-admitted binding — it replays or conflicts once this
        // admission settles. A failed admission records NOTHING (no sticky
        // id); a successful one permanently binds the id to the run.
        let admission = || -> Result<(String, _), SessionExecuteError> {
            let lease = match self.gate.try_begin_run(session_id) {
                Ok(lease) => lease,
                Err(crate::session_supervisor::BusyGateError::Busy) => {
                    tracing::info!(
                        session_id = session_id,
                        "normal submission to a busy session rejected (session_busy; retryable)"
                    );
                    return Err(SessionExecuteError::Busy);
                }
                Err(crate::session_supervisor::BusyGateError::RegistryFull) => {
                    return Err(SessionExecuteError::SessionRegistryFull);
                }
            };
            // R02 stage-repair R1 / F01 + R03-T01 step 2: the run id comes
            // from the backend's atomic allocator and is FIXED at task
            // creation; retries open new attempts on this same run, and
            // provider reconnects never mint a second user task.
            let run_id = self
                .backend
                .allocate_run_id_erased(now_ms)
                .map_err(SessionExecuteError::Storage)?;
            Ok((run_id, lease))
        };

        let (run_id, lease) = match submission.request_id {
            None => admission()?,
            Some(raw) => {
                let request_id = crate::dedup::validate_request_id(raw)
                    .map_err(|detail| SessionExecuteError::InvalidRequestId { detail })?;
                let kernel_principal = kernel_principal_of(principal);
                let key = crate::dedup::DedupKey {
                    owner_kind: kernel_principal.storage_kind().to_string(),
                    owner_subject: kernel_principal.storage_subject(),
                    session_id: session_id.to_string(),
                    request_id,
                };
                let digest = crate::dedup::normalized_request_digest_hex(submission.input);
                match self.dedup.admit(key, digest, admission) {
                    Err(full) => {
                        return Err(SessionExecuteError::IdempotencyRegistryFull { cap: full.cap })
                    }
                    Ok(Err(rejection)) => return Err(rejection),
                    Ok(Ok(crate::dedup::DedupDecision::Replay { run_id })) => {
                        tracing::info!(
                            run_id = %run_id,
                            session_id = session_id,
                            "idempotent submission replayed: same explicit requestId with the \
                             same normalized content — the original acceptance is returned, \
                             nothing is re-executed"
                        );
                        let run_count = self
                            .backend
                            .count_runs_erased(session_id)
                            .await
                            .map_err(SessionExecuteError::Storage)?;
                        return Ok(ExecuteAccepted {
                            run_id,
                            run_count,
                            replayed: true,
                        });
                    }
                    Ok(Ok(crate::dedup::DedupDecision::Conflict {
                        request_id,
                        recorded_digest,
                        submitted_digest,
                    })) => {
                        tracing::warn!(
                            session_id = session_id,
                            request_id = %request_id,
                            "duplicate requestId with CHANGED content refused: the recorded \
                             execution is not reused and no new execution starts (conflict)"
                        );
                        return Err(SessionExecuteError::DuplicateRequestConflict {
                            request_id,
                            recorded_digest,
                            submitted_digest,
                        });
                    }
                    Ok(Ok(crate::dedup::DedupDecision::Fresh { run_id, admitted })) => {
                        (run_id, admitted)
                    }
                }
            }
        };

        // Bound what we record (defense in depth; the body limit already
        // bounds the request).
        let recorded_input: String = submission.input.chars().take(2000).collect();

        // Drive the full lifecycle (start → turns → single finalize); the
        // run drains the session's steering channel before each provider
        // turn. `lease` frees the session on EVERY exit path below.
        let finish = supervisor
            .drive_run(
                port,
                events,
                &kernel_principal_of(principal),
                session_id,
                &row.agent_id,
                &run_id,
                &recorded_input,
                1,
                now_ms,
                Some(lease.steering_inbox()),
            )
            .await
            .map_err(SessionExecuteError::from)?;

        tracing::info!(
            run_id = %run_id,
            session_id = session_id,
            outcome = %finish.terminal_reason(),
            input_chars = recorded_input.chars().count(),
            "run settled through the single finalize path"
        );

        let run_count = self
            .backend
            .count_runs_erased(session_id)
            .await
            .map_err(SessionExecuteError::Storage)?;
        Ok(ExecuteAccepted {
            run_id,
            run_count,
            replayed: false,
        })
    }

    /// Submits a STEERING / follow-up input for the session's RUNNING turn
    /// (R03-T02 step 2: normal submissions and steering stay distinct).
    ///
    /// Frozen incumbent semantics (`session-coordinator.steerSession` /
    /// chat steer route): steering NEVER interrupts the running loop — the
    /// text reaches the run's NEXT model call through the session's
    /// steering channel; when the session is idle the outcome is
    /// [`SteerOutcome::Miss`] and the caller falls back to a normal
    /// submission ("steer missed, falling back to prompt"). The durable
    /// user-message projection of a steered commit (runSplit) is R06.
    pub async fn steer_for(
        &self,
        principal: &Principal,
        session_id: &str,
        text: &str,
    ) -> Result<SteerOutcome, SessionExecuteError> {
        let row = match self.backend.get_session_erased(session_id).await {
            Ok(Some(row)) => row,
            Ok(None) => return Err(SessionExecuteError::NotFound),
            Err(err) => return Err(SessionExecuteError::Storage(err)),
        };
        if !Self::can_access(principal, &row.owner_user_id) {
            return Err(SessionExecuteError::Forbidden);
        }
        match self.gate.steering_submit(session_id, text) {
            Ok(outcome) => Ok(outcome),
            Err(_) => Err(SessionExecuteError::SteeringInboxFull),
        }
    }

    /// Requests the cancellation of ONE run (R03-T03): resolves the run
    /// through the durable store, checks the SAME session-ownership rule
    /// as execute, then fires the run supervisor's cancellation tree.
    ///
    /// Four-phase honesty: `Accepted` returns the phase SNAPSHOT at the
    /// request moment (`requested`); the durable `cancelling` leg, the
    /// bounded child cleanup and the single `cancelled` finalize are
    /// performed by the run's own driver — query
    /// [`crate::runs::RunSupervisor::cancel_phase`] for the live phase
    /// or the run row for the durable terminal. An already-terminal run
    /// is a diagnosable no-op; an active row without a live driver in
    /// this process is reported as the explainable dangling state
    /// (recovery classification = R03-T07).
    pub async fn cancel_run_for<P: StoragePort>(
        &self,
        port: &P,
        supervisor: &crate::runs::RunSupervisor,
        principal: &Principal,
        run_id: &str,
    ) -> Result<CancelRunOutcome, SessionExecuteError> {
        let record = match port
            .load_run(&lingxi_protocol::RunId::new(run_id.to_string()))
            .await
        {
            Ok(Some(record)) => record,
            Ok(None) => return Err(SessionExecuteError::NotFound),
            Err(err) => return Err(SessionExecuteError::Storage(err)),
        };
        // Ownership: the same rule as every other session surface — the
        // run's session decides who may cancel it.
        let session_id = record.session_id.to_string();
        let row = match self.backend.get_session_erased(&session_id).await {
            Ok(Some(row)) => row,
            Ok(None) => return Err(SessionExecuteError::NotFound),
            Err(err) => return Err(SessionExecuteError::Storage(err)),
        };
        if !Self::can_access(principal, &row.owner_user_id) {
            return Err(SessionExecuteError::Forbidden);
        }
        if record.status.is_terminal() {
            return Ok(CancelRunOutcome::AlreadyTerminal {
                run_id: run_id.to_string(),
                status: record.status,
            });
        }
        match supervisor.cancel_run(run_id, "user") {
            crate::cancel::FireOutcome::Fired | crate::cancel::FireOutcome::AlreadyCancelling => {
                Ok(CancelRunOutcome::Accepted {
                    run_id: run_id.to_string(),
                    phase: supervisor
                        .cancel_phase(run_id)
                        .unwrap_or(CancelPhase::Active),
                })
            }
            crate::cancel::FireOutcome::NotLive => Ok(CancelRunOutcome::DanglingActive {
                run_id: run_id.to_string(),
                status: record.status,
                detail: "durable run row is active but no live driver exists in this \
                             process (restart or abandoned drive); recovery classification \
                             belongs to the R03-T07 startup scan"
                    .to_string(),
            }),
        }
    }

    /// Test/evidence helper: observable run count for a session.
    pub async fn run_count(&self, session_id: &str) -> Result<u64, StorageError> {
        self.backend.count_runs_erased(session_id).await
    }

    async fn facts_of(&self, row: SessionRow) -> Result<SessionFacts, StorageError> {
        let run_count = self.backend.count_runs_erased(&row.session_id).await?;
        let recent = self.backend.recent_runs_erased(&row.session_id, 5).await?;
        Ok(SessionFacts {
            session_id: row.session_id,
            agent_id: row.agent_id,
            owner_user_id: row.owner_user_id,
            title: row.title,
            run_count,
            last_runs: recent
                .into_iter()
                .map(|r| RunSummary {
                    run_id: r.run_id,
                    principal_id: r.principal_id,
                    started_at_unix_ms: r.started_at_unix_ms as u64,
                })
                .collect(),
        })
    }
}

impl From<SessionFacts> for SessionView {
    fn from(facts: SessionFacts) -> Self {
        Self {
            session_id: facts.session_id,
            agent_id: facts.agent_id,
            owner_user_id: facts.owner_user_id,
            title: facts.title,
            run_count: facts.run_count,
            last_runs: facts.last_runs,
        }
    }
}

fn kernel_principal_of(principal: &Principal) -> KernelPrincipal {
    match principal.kind {
        PrincipalKind::LocalUser => KernelPrincipal::LocalUser,
        PrincipalKind::Device => KernelPrincipal::Device {
            device_id: principal
                .device_id
                .clone()
                .unwrap_or_else(|| "unknown-device".to_string()),
            user_id: principal
                .user_id
                .clone()
                .unwrap_or_else(|| "unknown-user".to_string()),
        },
        // Authenticated-but-unclassified principals never reach the write
        // path with a fabricated identity; they execute as a narrowed
        // automation surface (never as the local owner).
        PrincipalKind::Unknown => KernelPrincipal::Automation {
            surface: "unclassified".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{ConnectionKindSerde, CredentialKind, PrincipalKind as Pk, TrustState};
    use crate::events::EventLimits;
    use crate::runs::RunSupervisor;
    use lingxi_kernel::ports::{CommittedOutcome, KeyEvent};
    use lingxi_kernel::RunContext;
    use lingxi_protocol::{RunId, ToolCallId};
    use std::sync::Mutex as StdMutex;

    /// R03-T01: the no-provider supervisor is the production default wiring
    /// until R05 registers real providers; these ownership-logic unit tests
    /// drive the REAL lifecycle through it.
    fn supervisor() -> RunSupervisor {
        RunSupervisor::without_provider()
    }

    /// Real event service over a real (temp) run database: the publication
    /// path in these unit tests runs the SAME code as production, never a
    /// mocked hub (only the session backend stays in-memory because these
    /// tests exercise ownership logic, not persistence — persistence is
    /// covered by service_persistence.rs / event_subscription.rs).
    async fn event_service_for_test() -> (EventService, std::path::PathBuf) {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r02t05-sessunit-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db = std::sync::Arc::new(
            lingxi_adapters::storage::RunDatabase::open(
                &dir.join("runs.db"),
                lingxi_adapters::storage::StoreOptions::default(),
            )
            .await
            .expect("open event read db"),
        );
        let sessions = std::sync::Arc::new(SessionStore::new((*db).clone()));
        let service = EventService::new(db, sessions, EventLimits::default()).expect("limits");
        (service, dir)
    }

    fn owner_principal() -> Principal {
        Principal {
            schema_version: 1,
            principal_id: "principal_local".to_string(),
            kind: Pk::LocalUser,
            user_id: Some(LOCAL_OWNER_USER_ID.to_string()),
            studio_id: None,
            server_node_id: None,
            device_id: None,
            credential_id: None,
            web_session_id: None,
            connection_kind: ConnectionKindSerde::Local,
            credential_kind: CredentialKind::LoopbackToken,
            trust_state: TrustState::Local,
            scopes: vec!["chat".to_string()],
        }
    }

    fn device_principal(user: &str) -> Principal {
        Principal {
            schema_version: 1,
            principal_id: format!("principal_device_{user}"),
            kind: Pk::Device,
            user_id: Some(user.to_string()),
            studio_id: None,
            server_node_id: None,
            device_id: Some("device_x".to_string()),
            credential_id: Some("cred_x".to_string()),
            web_session_id: None,
            connection_kind: ConnectionKindSerde::Lan,
            credential_kind: CredentialKind::DeviceCredential,
            trust_state: TrustState::Lan,
            scopes: vec!["chat".to_string()],
        }
    }

    type SharedRuns = std::sync::Arc<StdMutex<Vec<(String, String)>>>;

    /// In-memory backend for the ownership-logic unit tests.
    struct MemoryBackend {
        sessions: Vec<SessionRow>,
        runs: SharedRuns, // (session_id, run_id)
        /// Atomic id allocator mirroring the real backend's contract (F01):
        /// unique per call, seeded from the current run count.
        next_run_seq: std::sync::atomic::AtomicU64,
    }

    impl SessionBackend for MemoryBackend {
        async fn get_session(&self, session_id: &str) -> Result<Option<SessionRow>, StorageError> {
            Ok(self
                .sessions
                .iter()
                .find(|s| s.session_id == session_id)
                .cloned())
        }
        async fn list_sessions(&self) -> Result<Vec<SessionRow>, StorageError> {
            Ok(self.sessions.clone())
        }
        async fn count_runs(&self, session_id: &str) -> Result<u64, StorageError> {
            Ok(self
                .runs
                .lock()
                .expect("runs lock")
                .iter()
                .filter(|(sid, _)| sid == session_id)
                .count() as u64)
        }
        async fn total_runs(&self) -> Result<u64, StorageError> {
            Ok(self.runs.lock().expect("runs lock").len() as u64)
        }
        async fn recent_runs(
            &self,
            session_id: &str,
            limit: u32,
        ) -> Result<Vec<RunSummaryRow>, StorageError> {
            Ok(self
                .runs
                .lock()
                .expect("runs lock")
                .iter()
                .filter(|(sid, _)| sid == session_id)
                .rev()
                .take(limit as usize)
                .map(|(_, run)| RunSummaryRow {
                    run_id: run.clone(),
                    principal_id: "principal_local".to_string(),
                    started_at_unix_ms: 0,
                    status: "completed".to_string(),
                })
                .collect())
        }
        fn allocate_run_id(&self, now_ms: u64) -> Result<String, StorageError> {
            let seq = self
                .next_run_seq
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            Ok(format!("run_{now_ms:016x}_{seq:06x}"))
        }
    }

    /// Fake port recording committed outcomes; can inject commit failure.
    /// Shares the run list with the backend so counts stay coherent.
    struct FakePort {
        fail_outcome: bool,
        runs: SharedRuns,
        outcomes: StdMutex<Vec<String>>,
    }

    impl StoragePort for FakePort {
        async fn record_run_started(
            &self,
            ctx: &RunContext,
            _now_unix_ms: u64,
        ) -> Result<CommittedOutcome, StorageError> {
            // R03-T02 fixture: one await point inside the drive, modeling
            // the real storage round trip the RunDatabase always has.
            // Without it the fake is fully synchronous and the session's
            // busy window would be zero-width — the serialization gate
            // could never be observed by a concurrent submission. (The
            // real-backend concurrency matrix lives in
            // tests/execute_concurrency.rs.)
            tokio::task::yield_now().await;
            self.runs
                .lock()
                .expect("runs lock")
                .push((ctx.session_id.to_string(), ctx.run_id.to_string()));
            Ok(CommittedOutcome {
                newly_committed: true,
                events: Vec::new(),
            })
        }
        async fn commit_run_outcome(
            &self,
            ctx: &RunContext,
            _outcome: lingxi_kernel::ports::RunOutcome,
            _now_unix_ms: u64,
        ) -> Result<CommittedOutcome, StorageError> {
            if self.fail_outcome {
                return Err(StorageError::Io {
                    detail: "injected commit failure".to_string(),
                });
            }
            self.outcomes
                .lock()
                .expect("outcomes lock")
                .push(ctx.run_id.to_string());
            Ok(CommittedOutcome {
                newly_committed: true,
                events: Vec::new(),
            })
        }
        async fn load_run(
            &self,
            _run_id: &RunId,
        ) -> Result<Option<lingxi_kernel::ports::RunRecord>, StorageError> {
            Ok(None)
        }
        async fn record_run_events(
            &self,
            _ctx: &RunContext,
            _events: Vec<KeyEvent>,
            _now_unix_ms: u64,
        ) -> Result<CommittedOutcome, StorageError> {
            Ok(CommittedOutcome {
                newly_committed: true,
                events: Vec::new(),
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
            // Kernel-gated shape only (the transactional behavior of the
            // real store is covered against the adapter).
            lingxi_kernel::RunStateMachine::transition(from, to).map_err(|err| {
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
            _refused: lingxi_kernel::ports::StaleResultFact,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            // Audit-only (durable behavior is covered against the adapter).
            Ok(())
        }

        async fn record_invocation_intent(
            &self,
            _ctx: &RunContext,
            _intent: lingxi_kernel::ports::InvocationIntent,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            // Journal durability is covered against the real adapter.
            Ok(())
        }

        async fn advance_invocation(
            &self,
            _ctx: &RunContext,
            _journal_id: &ToolCallId,
            _to: lingxi_kernel::ports::InvocationPhase,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            Ok(())
        }

        async fn record_invocation_receipt(
            &self,
            _ctx: &RunContext,
            _journal_id: &ToolCallId,
            _receipt: lingxi_kernel::ports::InvocationReceipt,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
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
        ) -> Result<Vec<lingxi_kernel::ports::InvocationJournalEntry>, StorageError> {
            Ok(Vec::new())
        }
    }

    fn store_with_runs() -> (SessionStore, SharedRuns) {
        let runs: SharedRuns = std::sync::Arc::new(StdMutex::new(Vec::new()));
        (
            SessionStore::new(MemoryBackend {
                sessions: SessionStore::seed_rows(1000),
                runs: std::sync::Arc::clone(&runs),
                next_run_seq: std::sync::atomic::AtomicU64::new(0),
            }),
            runs,
        )
    }

    fn store() -> SessionStore {
        store_with_runs().0
    }

    #[tokio::test]
    async fn owner_sees_everything_and_execute_records_runs() {
        let (store, runs) = store_with_runs();
        let port = FakePort {
            fail_outcome: false,
            runs,
            outcomes: StdMutex::new(Vec::new()),
        };
        let (events, dir) = event_service_for_test().await;
        let owner = owner_principal();
        assert_eq!(store.list_for(&owner).await.unwrap().len(), 2);

        let accepted = store
            .execute_for(
                &port,
                &events,
                &supervisor(),
                &owner,
                "sess_local_alpha",
                "hello",
                1234,
            )
            .await
            .unwrap();
        assert_eq!(accepted.run_count, 1);
        assert_eq!(store.run_count("sess_local_alpha").await.unwrap(), 1);
        assert_eq!(
            store.run_count("sess_local_beta").await.unwrap(),
            0,
            "other session untouched"
        );

        match store.get_for(&owner, "sess_local_alpha").await.unwrap() {
            SessionAccess::Ok(_) => {}
            other => panic!("owner read must succeed, got {other:?}"),
        }
        drop(events);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cross_principal_access_is_forbidden_not_found() {
        let (store, runs) = store_with_runs();
        let port = FakePort {
            fail_outcome: false,
            runs,
            outcomes: StdMutex::new(Vec::new()),
        };
        let (events, dir) = event_service_for_test().await;
        let foreign = device_principal("user_remote_b");
        assert!(
            store.list_for(&foreign).await.unwrap().is_empty(),
            "no sessions owned by user_remote_b"
        );
        match store.get_for(&foreign, "sess_local_alpha").await.unwrap() {
            SessionAccess::Forbidden => {}
            other => panic!("cross-principal read must be Forbidden, got {other:?}"),
        }
        match store
            .execute_for(
                &port,
                &events,
                &supervisor(),
                &foreign,
                "sess_local_alpha",
                "inject",
                1500,
            )
            .await
        {
            Err(SessionExecuteError::Forbidden) => {}
            other => panic!("cross-principal execute must be Forbidden, got {other:?}"),
        }
        assert_eq!(
            store.run_count("sess_local_alpha").await.unwrap(),
            0,
            "denied execute must leave zero side effects"
        );
        match store.get_for(&foreign, "sess_missing").await.unwrap() {
            SessionAccess::NotFound => {}
            other => panic!("unknown session must be NotFound, got {other:?}"),
        }
        drop(events);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn same_user_device_principal_can_access() {
        let store = store();
        let same_user = device_principal(LOCAL_OWNER_USER_ID);
        match store.get_for(&same_user, "sess_local_beta").await.unwrap() {
            SessionAccess::Ok(_) => {}
            other => panic!("same-user device read must succeed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn failed_commit_is_a_storage_error_not_a_success() {
        let (store, runs) = store_with_runs();
        let port = FakePort {
            fail_outcome: true,
            runs,
            outcomes: StdMutex::new(Vec::new()),
        };
        let (events, dir) = event_service_for_test().await;
        let owner = owner_principal();
        match store
            .execute_for(
                &port,
                &events,
                &supervisor(),
                &owner,
                "sess_local_alpha",
                "hello",
                1234,
            )
            .await
        {
            Err(SessionExecuteError::Storage(StorageError::Io { detail })) => {
                assert!(detail.contains("injected"))
            }
            other => panic!("expected Storage error, got {other:?}"),
        }
        assert!(
            port.outcomes.lock().unwrap().is_empty(),
            "no outcome may be committed on failure"
        );
        drop(events);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn concurrent_executes_serialize_one_accepted_and_no_run_id_collapse() {
        // R02 stage-repair R1 / F01 regression + R03-T02 frozen semantics.
        // Pre-R03-T02, 64 distinct-input executes on ONE session at the
        // same millisecond had to all be accepted with distinct run ids
        // (the F01 fix). R03-T02 adopts the incumbent Node gate: a busy
        // session REJECTS further normal submissions (`session_busy`,
        // retryable) — so exactly ONE of the 64 wins the session and the
        // other 63 are rejected with zero durable side effects. The F01
        // public invariant itself (every ACCEPTED execute mints its own,
        // never-collapsing run id — including rapid sequential submits at
        // one fixed millisecond) is asserted right after.
        let (store, runs) = store_with_runs();
        let port = std::sync::Arc::new(FakePort {
            fail_outcome: false,
            runs,
            outcomes: StdMutex::new(Vec::new()),
        });
        let (events, dir) = event_service_for_test().await;
        let store = std::sync::Arc::new(store);
        let events = std::sync::Arc::new(events);
        let owner = owner_principal();
        let mut tasks = Vec::new();
        for n in 0..64 {
            let store = std::sync::Arc::clone(&store);
            let port = std::sync::Arc::clone(&port);
            let events = std::sync::Arc::clone(&events);
            let owner = owner.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .execute_for(
                        port.as_ref(),
                        events.as_ref(),
                        &supervisor(),
                        &owner,
                        "sess_local_alpha",
                        &format!("distinct-input-{n}"),
                        4242,
                    )
                    .await
            }));
        }
        let mut accepted_ids = Vec::new();
        let mut busy_rejections = 0;
        for t in tasks {
            match t.await.unwrap() {
                Ok(accepted) => accepted_ids.push(accepted.run_id),
                Err(SessionExecuteError::Busy) => busy_rejections += 1,
                other => panic!("expected accept or Busy, got {other:?}"),
            }
        }
        assert_eq!(
            accepted_ids.len(),
            1,
            "exactly one submission owns the session"
        );
        assert_eq!(
            busy_rejections, 63,
            "the rest are the frozen session_busy gate"
        );
        assert_eq!(store.run_count("sess_local_alpha").await.unwrap(), 1);

        // F01 protection preserved: rapid sequential submits at the SAME
        // millisecond still mint distinct, never-collapsing run ids.
        let mut sequential_ids = Vec::new();
        for n in 0..8 {
            let accepted = store
                .execute_for(
                    port.as_ref(),
                    events.as_ref(),
                    &supervisor(),
                    &owner,
                    "sess_local_alpha",
                    &format!("sequential-{n}"),
                    4242,
                )
                .await
                .expect("idle session accepts");
            sequential_ids.push(accepted.run_id);
        }
        let all: std::collections::HashSet<_> =
            accepted_ids.iter().chain(sequential_ids.iter()).collect();
        assert_eq!(
            all.len(),
            9,
            "every accepted execute is its own run: {all:?}"
        );
        assert_eq!(store.run_count("sess_local_alpha").await.unwrap(), 9);
        drop(events);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn busy_rejection_writes_nothing_and_steering_is_distinct() {
        // R03-T02 step 2: normal submission vs steering stay distinct on
        // the session surface. A busy session rejects a NORMAL submission
        // (zero side effects) but ACCEPTS steering for the running turn;
        // an idle session reports a steering MISS (the frozen fallback).
        let (store, runs) = store_with_runs();
        let port = FakePort {
            fail_outcome: false,
            runs,
            outcomes: StdMutex::new(Vec::new()),
        };
        let (events, dir) = event_service_for_test().await;
        let owner = owner_principal();

        // Hold the session busy through the supervisor directly (the same
        // reservation execute_for makes).
        let lease = store
            .session_supervisor()
            .try_begin_run("sess_local_alpha")
            .expect("reserve the session");
        match store
            .execute_for(
                &port,
                &events,
                &supervisor(),
                &owner,
                "sess_local_alpha",
                "second",
                1,
            )
            .await
        {
            Err(SessionExecuteError::Busy) => {}
            other => panic!("busy session must reject a normal submission, got {other:?}"),
        }
        assert!(
            port.outcomes.lock().unwrap().is_empty(),
            "the rejected submission committed nothing"
        );
        // Steering is NOT a new submission: accepted for the running turn.
        assert_eq!(
            store
                .steer_for(&owner, "sess_local_alpha", "focus on the file")
                .await
                .unwrap(),
            SteerOutcome::Accepted
        );
        // Cross-principal steering stays Forbidden (zero side effects).
        let foreign = device_principal("user_remote_b");
        assert_eq!(
            store
                .steer_for(&foreign, "sess_local_alpha", "inject")
                .await,
            Err(SessionExecuteError::Forbidden)
        );
        drop(lease);
        // Idle session: steering MISSES (the caller falls back to a normal
        // submission — frozen incumbent behavior).
        assert_eq!(
            store
                .steer_for(&owner, "sess_local_alpha", "after the run")
                .await
                .unwrap(),
            SteerOutcome::Miss
        );
        // And an idle session accepts a normal submission again.
        assert!(store
            .execute_for(
                &port,
                &events,
                &supervisor(),
                &owner,
                "sess_local_alpha",
                "next",
                2
            )
            .await
            .is_ok());
        drop(events);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn execute_request_rejects_identity_shaped_payload_fields() {
        // deny_unknown_fields: smuggling a principalId into the payload is a
        // hard parse error, not a silently ignored field.
        let raw = r#"{"input":"x","principalId":"forged"}"#;
        let err = serde_json::from_str::<ExecuteRequest>(raw).unwrap_err();
        assert!(err.to_string().contains("unknown field"), "err: {err}");
    }

    #[test]
    fn seed_rows_cover_both_synthetic_sessions_for_the_local_owner() {
        let rows = SessionStore::seed_rows(7);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.owner_user_id == LOCAL_OWNER_USER_ID));
        assert!(rows.iter().any(|r| r.session_id == "sess_local_alpha"));
        assert!(rows.iter().any(|r| r.session_id == "sess_local_beta"));
    }

    #[allow(unused)]
    fn _key_event_shape_compiles(_: KeyEvent) {}
}
