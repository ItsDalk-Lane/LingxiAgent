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
use lingxi_kernel::ports::{RequestBindingLookup, StorageError, StoragePort};
use lingxi_kernel::Principal as KernelPrincipal;
use lingxi_protocol::RunStatus;

use crate::auth::{Principal, PrincipalKind, LOCAL_OWNER_USER_ID};
use crate::cancel::CancelPhase;
use crate::events::EventService;
use crate::runs::DriveAuthorization;
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
    /// R03 repair G02/F03 — the run's driver had already IRREVOCABLY
    /// claimed its terminal settlement when this request arrived (the
    /// frozen linearization point of the cancel-vs-terminal race): the
    /// single finalize transaction for the run's completed/failed
    /// terminal is in flight. Nothing was cancelled and nothing stopped
    /// on this request — reporting `Accepted` would promise a stop that
    /// will not happen. The durable terminal (query the run row) is the
    /// answer; once it is durable a later request reads
    /// [`CancelRunOutcome::AlreadyTerminal`].
    TooLate { run_id: String, detail: String },
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
    /// R03 repair G04/F05 (C01/C03): the explicit requestId is currently
    /// RESERVED by a submission that is still between its admission and
    /// the durable run-start verdict — nothing replayable exists yet.
    /// An explicit RETRYABLE refusal: never a half-committed fake result,
    /// never a run id without a durable row.
    AdmissionInFlight {
        request_id: String,
    },
    /// R03 repair G04/F05 (C05): this process's dedup registry has NO
    /// binding for the id, but the DURABLE store shows a run already
    /// bound to the same (owner, session, requestId) anchor by an
    /// EARLIER process life (the per-run lineage `cause_id`). The safe
    /// cross-restart contract is an explicit refusal naming that run:
    /// query its durable outcome, or resubmit under a NEW id — the
    /// service never silently re-executes a task whose earlier life may
    /// have confirmed or unknown external effects (and never claims
    /// exactly-once across arbitrary external systems).
    RequestIdBoundToEarlierRun {
        request_id: String,
        run_id: String,
    },
    /// R03 RR2/F05-01: the CANONICAL (logical) requestId resolves to MORE
    /// THAN ONE durably bound run. This is the frozen pre-fix shape: the
    /// baseline built the lineage anchor from the RAW id, so distinct raw
    /// spellings of one logical key (" req-42 " and "req-42") could each
    /// bind their own run across restarts. The honest contract is an
    /// explicit ambiguity naming EVERY bound run — the client verifies
    /// those runs' durable outcomes or resubmits under a NEW id; the
    /// service never silently picks one binding (each bound run may hold
    /// confirmed or unknown external effects) and never re-executes the
    /// task as if it were fresh. `run_ids` is newest-first.
    RequestIdBoundAmbiguous {
        request_id: String,
        run_ids: Vec<String>,
    },
    /// R03-A14: the service's submission intake is CLOSED — the shutdown
    /// already began. The submission was refused at the admission chain
    /// BEFORE any side effect (no run id allocated, nothing written); the
    /// client may retry against the NEXT process instance.
    ShuttingDown,
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
    /// R03 repair G05/F06: the submission input exceeds the OFFICIAL
    /// input budget ([`crate::limits::MAX_SUBMISSION_INPUT_BYTES`] — the
    /// same number the transport layer enforces as the 1 MiB body/frame
    /// limit). Refused LOUDLY at admission BEFORE any side effect: no
    /// run id is allocated, nothing is written, nothing is dispatched,
    /// and no id→run binding is left behind. Within the budget the input
    /// flows to the execution surface BYTE-HONEST and complete — input
    /// is never silently truncated, and the refusal names both byte
    /// counts so the client can split or shrink the request knowingly.
    InputTooLarge {
        bytes: usize,
        limit_bytes: usize,
    },
    /// R03-T06: the background-drive registry is at its hard cap (service
    /// protection; nothing was written, nothing was executed).
    BackgroundRegistryFull {
        cap: usize,
    },
    /// The storage port refused or failed: no visible success was produced
    /// (R02-A07). Carries the domain storage error for the endpoint's
    /// error surface.
    Storage(StorageError),
}

/// The shared admission outcome of the two submission surfaces
/// (R03-T06): a freshly admitted run (holding its session lease and —
/// for explicit-id submissions — the G04/F05 owner-side binding verdict
/// handle), or an idempotent REPLAY of an earlier DURABLY ADMITTED
/// acceptance.
enum AdmissionOutcome {
    Admitted {
        run_id: String,
        lease: crate::session_supervisor::SessionLease,
        agent_id: String,
        /// R03 repair G04/F05: the two-phase id→run binding handle. The
        /// submission path holds it through the durable run-start verdict;
        /// `None` on the plain (no-id) path.
        binding: Option<crate::dedup::AdmissionBinding>,
    },
    Replayed {
        accepted: ExecuteAccepted,
    },
}

/// R03 RR2/F05-01 — the trusted admission boundary's SINGLE canonical
/// request-id fact. Called exactly once at the head of BOTH submission
/// surfaces (foreground and background), BEFORE any session read, busy
/// gate, run-id allocation, dedup reservation, durable write or dispatch;
/// every consumer below — the in-memory dedup key, the cross-restart
/// durable lookup, the [`DriveAuthorization`] lineage anchor and every
/// error echo — uses the returned canonical id, so no layer can re-derive
/// a different identity from the raw bytes. The RAW id (when it differs)
/// is kept ONLY as an audit log field; it never becomes an identity
/// source again. The canonicalization rule itself is the single shared
/// [`lingxi_kernel::subagent::canonical_request_id`] (Rust `str::trim`,
/// the full Unicode whitespace set); the acceptance POLICY (non-empty,
/// length bound) is [`crate::dedup::validate_request_id`], which applies
/// that rule and refuses illegal ids LOUDLY before any side effect.
fn canonicalize_submission_request_id(
    raw: Option<&str>,
) -> Result<Option<String>, SessionExecuteError> {
    match raw {
        None => Ok(None),
        Some(raw) => {
            let canonical = crate::dedup::validate_request_id(raw)
                .map_err(|detail| SessionExecuteError::InvalidRequestId { detail })?;
            if canonical != raw {
                tracing::info!(
                    raw_request_id = raw,
                    canonical_request_id = %canonical,
                    "requestId canonicalized at the admission boundary — the raw form is an \
                     audit-only field; every identity below uses the canonical fact"
                );
            }
            Ok(Some(canonical))
        }
    }
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
    /// R03 RR2/F05-01: the durable cross-restart request-anchor lookup
    /// (the G04/F05 C05 contract with the legacy compatible read). Answers
    /// how the (owner kind, owner subject, session, CANONICAL request id)
    /// namespace's `request:` lineage anchors relate to existing run rows:
    /// unbound, bound to exactly one run, or bound to SEVERAL (the frozen
    /// pre-fix shape — raw-padded anchors of one logical key). The
    /// submission surface uses it to REFUSE a post-restart same-key retry
    /// explicitly (naming the run(s)) instead of silently re-executing a
    /// task whose earlier life may have confirmed or unknown external
    /// effects. `canonical_request_id` must be the CANONICAL form (the
    /// single normalization rule lives at the submission boundary).
    fn find_request_binding(
        &self,
        session_id: &str,
        owner_kind: &str,
        owner_subject: &str,
        canonical_request_id: &str,
    ) -> impl Future<Output = Result<RequestBindingLookup, StorageError>> + Send;
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
    fn find_request_binding_erased<'a>(
        &'a self,
        session_id: &'a str,
        owner_kind: &'a str,
        owner_subject: &'a str,
        canonical_request_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<RequestBindingLookup, StorageError>> + Send + 'a>>;
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
    fn find_request_binding_erased<'a>(
        &'a self,
        session_id: &'a str,
        owner_kind: &'a str,
        owner_subject: &'a str,
        canonical_request_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<RequestBindingLookup, StorageError>> + Send + 'a>> {
        Box::pin(SessionBackend::find_request_binding(
            self,
            session_id,
            owner_kind,
            owner_subject,
            canonical_request_id,
        ))
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
    fn find_request_binding(
        &self,
        session_id: &str,
        owner_kind: &str,
        owner_subject: &str,
        canonical_request_id: &str,
    ) -> impl Future<Output = Result<RequestBindingLookup, StorageError>> + Send {
        RunDatabase::find_request_binding(
            self,
            session_id,
            owner_kind,
            owner_subject,
            canonical_request_id,
        )
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
    /// R03 repair G04/F05: shared as `Arc` because the owner-side
    /// [`crate::dedup::AdmissionBinding`] verdict handles (including the
    /// one moved into a detached background drive) must reach it after
    /// the submission call itself has returned.
    dedup: std::sync::Arc<crate::dedup::SubmissionDedup>,
    /// R03-T07/A14: the submission intake gate. Defaults to an ALWAYS-OPEN
    /// gate (tests construct the store directly); the composition root
    /// injects the process's real gate via
    /// [`SessionStore::with_concurrency_and_intake`] so closing it at
    /// signal time refuses every subsequent admission on BOTH submission
    /// surfaces.
    intake: std::sync::Arc<crate::shutdown::SubmissionIntake>,
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
        Self::with_concurrency_and_intake(
            backend,
            limits,
            std::sync::Arc::new(crate::shutdown::SubmissionIntake::new()),
        )
    }

    /// The composition-root construction (R03-T07): the concurrency policy
    /// PLUS the process's submission intake gate — the shutdown future
    /// closes it at signal time and the shared admission chain below
    /// refuses every later submission with
    /// [`SessionExecuteError::ShuttingDown`] (A14: no submission is
    /// absorbed once the exit has begun).
    pub fn with_concurrency_and_intake(
        backend: impl SessionBackend + 'static,
        limits: SessionConcurrencyLimits,
        intake: std::sync::Arc<crate::shutdown::SubmissionIntake>,
    ) -> Self {
        Self {
            backend: Box::new(backend),
            gate: SessionSupervisor::new(limits),
            dedup: std::sync::Arc::new(crate::dedup::SubmissionDedup::default()),
            intake,
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
    ///   performs; R03 repair G04/F05: only a DURABLY ADMITTED binding
    ///   replays — the run row exists by construction);
    /// - same id + changed digest → [`SessionExecuteError::DuplicateRequestConflict`]:
    ///   the recorded execution is NOT reused for the new content and NO
    ///   new execution starts;
    /// - fresh id → the admission (busy gate + run-id allocation + the
    ///   id→run reservation) is serialized per key, so a concurrent duplicate
    ///   cannot slip a second admission in between; while the first
    ///   submission is between its reservation and the durable run start,
    ///   duplicates get the explicit retryable
    ///   [`SessionExecuteError::AdmissionInFlight`] (never a half-committed
    ///   fake result).
    ///
    /// R03 repair G04/F05 compensation: a drive that fails BEFORE its
    /// durable start is verified against the store and — when the run row
    /// provably never existed — the reservation is safely retracted (the
    /// retry re-admits fresh); an un-verifiable outcome keeps the binding
    /// for lazy resolution. After the durable start NOTHING retracts the
    /// binding (a lost response replays the real run).
    ///
    /// The idempotency registry is process-memory and bounded; the
    /// CROSS-RESTART same-key contract is the durable lineage anchor (see
    /// [`SessionExecuteError::RequestIdBoundToEarlierRun`]).
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
        // R03 RR2/F05-01 — the SINGLE canonicalization boundary of the
        // foreground submission surface. Everything below (the dedup key,
        // the cross-restart anchor lookup, the DriveAuthorization lineage
        // and the error echoes) consumes this ONE canonical fact; the raw
        // id stays an audit-only log field (see
        // `canonicalize_submission_request_id`).
        let canonical_request_id = canonicalize_submission_request_id(submission.request_id)?;
        let submission = ExecuteSubmission {
            input: submission.input,
            request_id: canonical_request_id.as_deref(),
        };
        let (run_id, lease, agent_id, binding) = match self
            .admit_submission(port, principal, session_id, &submission, now_ms)
            .await?
        {
            AdmissionOutcome::Replayed { accepted } => return Ok(accepted),
            AdmissionOutcome::Admitted {
                run_id,
                lease,
                agent_id,
                binding,
            } => (run_id, lease, agent_id, binding),
        };

        // R03 repair G05/F06: the EXECUTION PAYLOAD is the FULL, byte-
        // honest input — no projection, no truncation. The admission
        // chain above has already verified the official input budget
        // (and the transport layers bound the same input tighter still),
        // so everything below this point is legal content whose TAIL is
        // as much a part of the task as its head. Bounded summaries are
        // a LOG-ONLY concern (see the tracing calls: counts, never
        // content).
        //
        // Drive the full lifecycle (start → turns → single finalize); the
        // run drains the session's steering channel before each provider
        // turn. `lease` frees the session on EVERY exit path below.
        let finish = match supervisor
            .drive_run(
                port,
                events,
                &kernel_principal_of(principal),
                session_id,
                &agent_id,
                &run_id,
                submission.input,
                1,
                now_ms,
                Some(lease.steering_inbox()),
                None,
                DriveAuthorization::user_submission(submission.request_id),
                binding.as_ref(),
                session_id,
            )
            .await
        {
            Ok(finish) => finish,
            Err(err) => {
                // R03 repair G04/F05 (C02): the drive failed — the run id
                // was NOT durably admitted (nothing was dispatched, no
                // external action can have happened before the durable
                // start). Settle the reservation against the store: a
                // verified-absent row safely retracts it; a present row
                // (the start committed after all) promotes it; an
                // unreadable store keeps it (unverified — never deleted).
                if let Some(binding) = binding.as_ref() {
                    if !binding.is_committed() {
                        match port
                            .load_run(&lingxi_protocol::RunId::new(run_id.clone()))
                            .await
                        {
                            Ok(Some(_)) => binding.commit_durable(),
                            Ok(None) => binding.release_not_started(),
                            Err(_) => binding.mark_unverified(),
                        }
                    }
                }
                return Err(err.into());
            }
        };
        // Defensive no-op by construction (the drive committed the binding
        // at its durable start); keeps the promise even if a future
        // refactor moves the start write.
        if let Some(binding) = binding.as_ref() {
            binding.commit_durable();
        }

        tracing::info!(
            run_id = %run_id,
            session_id = session_id,
            outcome = %finish.terminal_reason(),
            // Log-only summaries are COUNTS of the full input — the
            // content itself is task data, not log data (G05/F06).
            input_chars = submission.input.chars().count(),
            input_bytes = submission.input.len(),
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

    /// The BACKGROUND submission surface (R03-T06 deliverable 后台提交接
    /// 口): the SAME admission chain (ownership → busy gate → requestId
    /// dedup), but the drive is spawned as a DETACHED supervised task of
    /// the same run supervisor — the run's lifetime is DECOUPLED from the
    /// caller's connection. A client disconnect (or a closed desktop
    /// window — the desktop is just another client) cannot cancel the
    /// run; reconnecting with the same requestId replays idempotently
    /// (nothing re-executed) and the run stays queryable through the
    /// session/events surfaces. The service-exit difference is bounded by
    /// the registry's exit hook (drain + honest unconfirmed report).
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_background_for(
        &self,
        storage: &std::sync::Arc<lingxi_adapters::storage::RunDatabase>,
        events: &std::sync::Arc<EventService>,
        supervisor: &std::sync::Arc<crate::runs::RunSupervisor>,
        background: &std::sync::Arc<crate::background::BackgroundDriveRegistry>,
        principal: &Principal,
        session_id: &str,
        submission: &ExecuteSubmission<'_>,
        now_ms: u64,
    ) -> Result<ExecuteAccepted, SessionExecuteError> {
        // R03 RR2/F05-01 — the background surface canonicalizes through the
        // SAME single boundary as the foreground one: the dedup key, the
        // cross-restart anchor lookup and the DriveAuthorization lineage
        // of the detached drive all consume this ONE canonical fact.
        let canonical_request_id = canonicalize_submission_request_id(submission.request_id)?;
        let submission = ExecuteSubmission {
            input: submission.input,
            request_id: canonical_request_id.as_deref(),
        };
        let (run_id, lease, _agent_id, binding) = match self
            .admit_submission(storage.as_ref(), principal, session_id, &submission, now_ms)
            .await?
        {
            AdmissionOutcome::Replayed { accepted } => return Ok(accepted),
            AdmissionOutcome::Admitted {
                run_id,
                lease,
                agent_id,
                binding,
            } => (run_id, lease, agent_id, binding),
        };
        // R03 repair G05/F06: the BACKGROUND entry executes the same
        // FULL, byte-honest input as the foreground — the admission
        // chain has already verified the official input budget, so
        // nothing here may shorten the task content.
        // The session lease moves INTO the detached drive: the session
        // stays honestly busy until the background run settles (every
        // exit path of the drive). R03 repair G04/F05: the id→run binding
        // verdict handle moves with it — the DETACHED task owns the
        // reservation until its own durable start (commit) or its honest
        // failure compensation; a REFUSED dispatch (registry / supervisor
        // capacity) retracts the reservation here because nothing was
        // spawned and nothing was written (a loud refusal, never a fake
        // acceptance).
        crate::background::spawn_background_drive(
            supervisor,
            storage,
            events,
            background,
            kernel_principal_of(principal),
            session_id.to_string(),
            _agent_id,
            run_id.clone(),
            submission.input.to_string(),
            DriveAuthorization::user_submission(submission.request_id),
            binding,
            now_ms,
            lease,
        )
        .map_err(|rejected| SessionExecuteError::BackgroundRegistryFull { cap: rejected.cap })?;

        tracing::info!(
            run_id = %run_id,
            session_id = session_id,
            "background submission accepted: the drive is decoupled from this caller's \
             connection (disconnect ≠ cancel)"
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

    /// The shared admission chain of the foreground and background
    /// submission surfaces: session lookup + ownership, the busy gate,
    /// run-id allocation and (for explicit-id submissions) the T04
    /// idempotency decision — the admission of a fresh run happens under
    /// the dedup key lock, so a concurrent duplicate can never slip a
    /// second admission in between.
    ///
    /// R03 repair G04/F05: the id→run binding is now a TWO-PHASE ledger
    /// (reserved → durably admitted, with an unverified backstop — see
    /// [`crate::dedup`]). A REPLAY is only returned for a DURABLY ADMITTED
    /// binding (the run row exists by construction); an in-flight
    /// reservation answers the explicit retryable
    /// [`SessionExecuteError::AdmissionInFlight`]; an UNVERIFIED binding
    /// (a previous owner that exited without a verdict) is resolved
    /// against the durable store through `port` right here — promoted
    /// when the row exists, safely retracted (and the admission re-run)
    /// when it provably never did, and the storage error surfaced when it
    /// cannot be read (the binding is kept).
    ///
    /// R03-A14: the shutdown intake gate refuses FRESH admissions inside
    /// the admission closure — an idempotent REPLAY of an already-admitted
    /// submission (same explicit requestId + content) is still answered
    /// with the original acceptance: it is a query about an EXISTING task,
    /// not a new submission, and the exiting process still owes the client
    /// that answer.
    async fn admit_submission<P: StoragePort>(
        &self,
        port: &P,
        principal: &Principal,
        session_id: &str,
        submission: &ExecuteSubmission<'_>,
        now_ms: u64,
    ) -> Result<AdmissionOutcome, SessionExecuteError> {
        // R03 repair G05/F06 — the OFFICIAL input budget, enforced at the
        // very head of the admission chain (BOTH submission surfaces),
        // before the session read, the busy gate, the run-id allocation,
        // the dedup reservation or ANY durable side effect. Over the
        // budget the submission is refused loudly; within it the input
        // is executed complete and byte-honest (see the drive calls
        // below — the full `submission.input`, never a projection). The
        // budget equals the transport body/frame limit, so a legal
        // HTTP/WS request can never trip this leg; it exists so that
        // over-budget input is REFUSED instead of silently truncated.
        let input_bytes = submission.input.len();
        if input_bytes > crate::limits::MAX_SUBMISSION_INPUT_BYTES {
            tracing::warn!(
                session_id = session_id,
                input_bytes = input_bytes,
                limit_bytes = crate::limits::MAX_SUBMISSION_INPUT_BYTES,
                "submission refused: input exceeds the official budget — resubmit within \
                 the budget (input is never silently truncated)"
            );
            return Err(SessionExecuteError::InputTooLarge {
                bytes: input_bytes,
                limit_bytes: crate::limits::MAX_SUBMISSION_INPUT_BYTES,
            });
        }
        let row = match self.backend.get_session_erased(session_id).await {
            Ok(Some(row)) => row,
            Ok(None) => return Err(SessionExecuteError::NotFound),
            Err(err) => return Err(SessionExecuteError::Storage(err)),
        };
        if !Self::can_access(principal, &row.owner_user_id) {
            return Err(SessionExecuteError::Forbidden);
        }
        let agent_id = row.agent_id;

        // R03-T04 admission. The synchronous admission closure acquires the
        // session's single owner slot (R03-T02 busy gate) and allocates the
        // run id; for explicit-id submissions the closure runs UNDER the
        // dedup registry's key lock, so a concurrent same-id duplicate can
        // neither slip a second admission in between nor observe a
        // half-admitted binding — it replays, conflicts or gets the
        // in-flight refusal once this admission settles. A failed admission
        // records NOTHING (no sticky id); a successful one leaves a PENDING
        // reservation whose owner-side verdict handle returns to the caller.
        let admission = || -> Result<(String, _), SessionExecuteError> {
            // R03-A14: the exit has already begun — refuse the FRESH
            // admission BEFORE the busy gate, the run-id allocation or any
            // write. A submission racing the signal either wins this check
            // (admitted; its run is an EXISTING task the exit then settles
            // by policy) or loses it (refused here); it can never be
            // absorbed into a process that is already draining. Idempotent
            // replays of earlier acceptances never reach this closure.
            if self.intake.is_closed() {
                tracing::info!(
                    session_id = session_id,
                    "submission refused: the service is shutting down (intake closed)"
                );
                return Err(SessionExecuteError::ShuttingDown);
            }
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

        let Some(raw) = submission.request_id else {
            let (run_id, lease) = admission()?;
            return Ok(AdmissionOutcome::Admitted {
                run_id,
                lease,
                agent_id,
                binding: None,
            });
        };
        let request_id = crate::dedup::validate_request_id(raw)
            .map_err(|detail| SessionExecuteError::InvalidRequestId { detail })?;
        let kernel_principal = kernel_principal_of(principal);
        let key = crate::dedup::DedupKey {
            owner_kind: kernel_principal.storage_kind().to_string(),
            owner_subject: kernel_principal.storage_subject(),
            session_id: session_id.to_string(),
            request_id: request_id.clone(),
        };
        let digest = crate::dedup::normalized_request_digest_hex(submission.input);

        // R03 repair G04/F05 (C05), extended by RR2/F05-01 — the
        // cross-restart same-key contract over the CANONICAL logical key.
        // This process's registry has no binding for the key, but the
        // DURABLE lineage anchors may: an earlier process life admitted a
        // run under the same (owner, session, logical requestId) — the
        // lookup is legacy-compatible (pre-fix rows whose anchor held the
        // RAW id still normalize to this key) and ambiguity-honest:
        //   * ONE bound run → refuse EXPLICITLY naming that run;
        //   * SEVERAL bound runs (the pre-fix duplicate shape) → refuse as
        //     an explicit AMBIGUITY naming every run — never a silent pick
        //     of one binding, never a blind fresh re-execution;
        //   * none → the id is fresh at the durable layer too.
        // (Re-check the in-memory registry after the read so a
        // concurrently admitted SAME-PROCESS binding still wins with its
        // replay.)
        if self.dedup.lookup(&key).is_none() {
            let earlier = self
                .backend
                .find_request_binding_erased(
                    session_id,
                    kernel_principal.storage_kind(),
                    &kernel_principal.storage_subject(),
                    &request_id,
                )
                .await
                .map_err(SessionExecuteError::Storage)?;
            if self.dedup.lookup(&key).is_none() {
                match earlier {
                    RequestBindingLookup::Bound {
                        run_id: earlier_run,
                    } => {
                        tracing::warn!(
                            session_id = session_id,
                            request_id = %request_id,
                            earlier_run_id = %earlier_run,
                            "requestId is durably bound to a run from an earlier process life — \
                             refusing explicitly instead of silently re-executing (query that \
                             run or resubmit under a new id)"
                        );
                        return Err(SessionExecuteError::RequestIdBoundToEarlierRun {
                            request_id,
                            run_id: earlier_run,
                        });
                    }
                    RequestBindingLookup::Ambiguous { run_ids } => {
                        tracing::warn!(
                            session_id = session_id,
                            request_id = %request_id,
                            bound_run_ids = ?run_ids,
                            "requestId (canonical logical key) is durably bound to SEVERAL runs \
                             (pre-fix raw-anchor rows) — refusing as an explicit ambiguity; \
                             verify the named runs or resubmit under a new id"
                        );
                        return Err(SessionExecuteError::RequestIdBoundAmbiguous {
                            request_id,
                            run_ids,
                        });
                    }
                    RequestBindingLookup::Unbound => {}
                }
            }
        }

        // The admission rounds: an UNVERIFIED binding whose row provably
        // never existed is retracted and the admission re-runs (bounded —
        // concurrent resolutions make the next round terminal).
        for round in 0..4 {
            match self.dedup.admit(key.clone(), digest.clone(), admission) {
                Err(full) => {
                    return Err(SessionExecuteError::IdempotencyRegistryFull { cap: full.cap })
                }
                Ok(Err(rejection)) => return Err(rejection),
                Ok(Ok(crate::dedup::DedupDecision::Replay { run_id })) => {
                    tracing::info!(
                        run_id = %run_id,
                        session_id = session_id,
                        "idempotent submission replayed: same explicit requestId with the \
                         same normalized content — the durably admitted acceptance is \
                         returned, nothing is re-executed"
                    );
                    let run_count = self
                        .backend
                        .count_runs_erased(session_id)
                        .await
                        .map_err(SessionExecuteError::Storage)?;
                    return Ok(AdmissionOutcome::Replayed {
                        accepted: ExecuteAccepted {
                            run_id,
                            run_count,
                            replayed: true,
                        },
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
                Ok(Ok(crate::dedup::DedupDecision::InFlight { request_id })) => {
                    tracing::info!(
                        session_id = session_id,
                        request_id = %request_id,
                        "duplicate requestId while the first submission is still between its \
                         reservation and the durable run start — explicit retryable refusal \
                         (no half-committed result)"
                    );
                    return Err(SessionExecuteError::AdmissionInFlight { request_id });
                }
                Ok(Ok(crate::dedup::DedupDecision::Unverified {
                    request_id: _,
                    run_id,
                    same_digest,
                    recorded_digest,
                })) => {
                    // The previous owner of this id exited WITHOUT a
                    // durable-start verdict. Resolve against the store
                    // NOW: the read is the linearization point of the
                    // outcome (run rows are never deleted).
                    match port
                        .load_run(&lingxi_protocol::RunId::new(run_id.clone()))
                        .await
                    {
                        Ok(Some(_)) => {
                            self.dedup.resolve_unverified_present(&key, &run_id);
                            if same_digest {
                                tracing::info!(
                                    run_id = %run_id,
                                    session_id = session_id,
                                    "unverified admission resolved: the durable run row \
                                     exists — the binding is promoted and replays honestly"
                                );
                                let run_count = self
                                    .backend
                                    .count_runs_erased(session_id)
                                    .await
                                    .map_err(SessionExecuteError::Storage)?;
                                return Ok(AdmissionOutcome::Replayed {
                                    accepted: ExecuteAccepted {
                                        run_id,
                                        run_count,
                                        replayed: true,
                                    },
                                });
                            }
                            tracing::warn!(
                                session_id = session_id,
                                run_id = %run_id,
                                "unverified admission resolved to an EXISTING run; changed \
                                 content under the same id conflicts"
                            );
                            return Err(SessionExecuteError::DuplicateRequestConflict {
                                request_id,
                                recorded_digest,
                                submitted_digest: digest,
                            });
                        }
                        Ok(None) => {
                            // Proven: nothing was ever durably admitted
                            // under this id — no run row, hence no
                            // external action (every external action of a
                            // drive happens after its durable start).
                            // Retract and re-admit.
                            let retracted = self.dedup.resolve_unverified_absent(&key, &run_id);
                            tracing::info!(
                                session_id = session_id,
                                request_id = %request_id,
                                stale_run_id = %run_id,
                                retracted,
                                round,
                                "unverified admission resolved: the run row provably never \
                                 existed — the reservation is retracted and the id is fresh \
                                 again"
                            );
                            continue;
                        }
                        Err(err) => {
                            // The outcome stays UNKNOWN: the binding is
                            // kept (never deleted on an unverifiable
                            // error) and the storage failure surfaces.
                            tracing::warn!(
                                session_id = session_id,
                                request_id = %request_id,
                                run_id = %run_id,
                                error = ?err,
                                "unverified admission could not be resolved against the \
                                 store — binding KEPT, storage error surfaced"
                            );
                            return Err(SessionExecuteError::Storage(err));
                        }
                    }
                }
                Ok(Ok(crate::dedup::DedupDecision::Fresh {
                    run_id,
                    admitted,
                    binding,
                })) => {
                    return Ok(AdmissionOutcome::Admitted {
                        run_id,
                        lease: admitted,
                        agent_id,
                        binding: Some(binding),
                    });
                }
            }
        }
        // Bounded churn (concurrent same-key resolutions): an honest
        // retryable refusal — never a fabricated admission.
        tracing::warn!(
            session_id = session_id,
            request_id = %request_id,
            "admission resolution churn — refusing retryably"
        );
        Err(SessionExecuteError::AdmissionInFlight { request_id })
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
            crate::cancel::FireOutcome::TooLate => Ok(CancelRunOutcome::TooLate {
                run_id: run_id.to_string(),
                detail: "the run's terminal settlement was already irrevocably claimed \
                         (the single finalize transaction is in flight or just committed); \
                         nothing was cancelled and nothing stopped on this request — query \
                         the run row for the durable terminal"
                    .to_string(),
            }),
            crate::cancel::FireOutcome::NotLive => {
                // R03 repair G02/F03: the driver may have settled and
                // deregistered between the row load above and the fire —
                // reload before reporting the dangling state so a run
                // that just completed is answered AlreadyTerminal, never
                // misreported as an active-but-driverless row.
                match port
                    .load_run(&lingxi_protocol::RunId::new(run_id.to_string()))
                    .await
                {
                    Ok(Some(fresh)) if fresh.status.is_terminal() => {
                        Ok(CancelRunOutcome::AlreadyTerminal {
                            run_id: run_id.to_string(),
                            status: fresh.status,
                        })
                    }
                    Ok(Some(fresh)) => Ok(CancelRunOutcome::DanglingActive {
                        run_id: run_id.to_string(),
                        status: fresh.status,
                        detail: "durable run row is active but no live driver exists in this \
                                 process (restart or abandoned drive); recovery classification \
                                 belongs to the R03-T07 startup scan"
                            .to_string(),
                    }),
                    Ok(None) => Err(SessionExecuteError::NotFound),
                    Err(err) => Err(SessionExecuteError::Storage(err)),
                }
            }
        }
    }

    /// Sets the session's current permission mode (R03-T06): the parent
    /// fact the subagent attenuation inherits from (the incumbent's
    /// per-session `operate/ask/read_only` mode). Ownership-checked like
    /// every other session mutation; persisting the mode as durable
    /// session state is R06 — this sets the run-layer registry.
    pub async fn set_permission_mode_for(
        &self,
        principal: &Principal,
        session_id: &str,
        mode: lingxi_kernel::subagent::SessionPermissionMode,
    ) -> Result<(), SessionExecuteError> {
        let row = match self.backend.get_session_erased(session_id).await {
            Ok(Some(row)) => row,
            Ok(None) => return Err(SessionExecuteError::NotFound),
            Err(err) => return Err(SessionExecuteError::Storage(err)),
        };
        if !Self::can_access(principal, &row.owner_user_id) {
            return Err(SessionExecuteError::Forbidden);
        }
        self.gate
            .set_permission_mode(session_id, mode)
            .map_err(|_| SessionExecuteError::SessionRegistryFull)
    }

    /// Reads the lineage of one run under the SAME ownership rule as every
    /// other session surface (R03-T06: the parent-child relationship is
    /// queryable — including after refusals and crashes).
    pub async fn run_lineage_for<P: StoragePort>(
        &self,
        port: &P,
        principal: &Principal,
        run_id: &str,
    ) -> Result<Option<lingxi_kernel::subagent::RunLineage>, SessionExecuteError> {
        let record = match port
            .load_run(&lingxi_protocol::RunId::new(run_id.to_string()))
            .await
        {
            Ok(Some(record)) => record,
            Ok(None) => return Ok(None),
            Err(err) => return Err(SessionExecuteError::Storage(err)),
        };
        let session_id = record.session_id.to_string();
        let row = match self.backend.get_session_erased(&session_id).await {
            Ok(Some(row)) => row,
            Ok(None) => return Err(SessionExecuteError::NotFound),
            Err(err) => return Err(SessionExecuteError::Storage(err)),
        };
        if !Self::can_access(principal, &row.owner_user_id) {
            return Err(SessionExecuteError::Forbidden);
        }
        port.load_run_lineage(&lingxi_protocol::RunId::new(run_id.to_string()))
            .await
            .map_err(SessionExecuteError::Storage)
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
    use lingxi_protocol::{RunId, SessionId, ToolCallId};
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
        async fn find_request_binding(
            &self,
            _session_id: &str,
            _owner_kind: &str,
            _owner_subject: &str,
            _canonical_request_id: &str,
        ) -> Result<RequestBindingLookup, StorageError> {
            // The unit-test backend keeps no durable lineage ledger — the
            // G04/F05 cross-restart contract is covered by the integration
            // suite against the REAL store (run_lineage.cause_id). No
            // earlier-life anchor is ever reported here, so the in-process
            // admission lifecycle is what these unit tests exercise.
            Ok(RequestBindingLookup::Unbound)
        }
    }

    /// Fake port recording committed outcomes; can inject commit failure
    /// (`fail_outcome`) and, for the G04/F05 compensation paths, a
    /// run-start failure (`fail_start`). Shares the run list with the
    /// backend so counts stay coherent; `load_run` reflects the recorded
    /// run list so the post-failure verification reads a truthful store.
    struct FakePort {
        fail_outcome: bool,
        fail_start: bool,
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
            if self.fail_start {
                return Err(StorageError::Io {
                    detail: "injected run-start failure (unit)".to_string(),
                });
            }
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
            run_id: &RunId,
        ) -> Result<Option<lingxi_kernel::ports::RunRecord>, StorageError> {
            // Truthful for the compensation paths: a run exists exactly
            // when its start was recorded through this port.
            let recorded = self
                .runs
                .lock()
                .expect("runs lock")
                .iter()
                .any(|(_, run)| run == run_id.as_str());
            if recorded {
                Ok(Some(lingxi_kernel::ports::RunRecord {
                    run_id: run_id.clone(),
                    session_id: SessionId::new("sess_local_alpha".to_string()),
                    status: lingxi_protocol::RunStatus::Running,
                    last_event_seq: lingxi_protocol::Seq::new(0),
                }))
            } else {
                Ok(None)
            }
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

        async fn record_run_lineage(
            &self,
            _ctx: &RunContext,
            _lineage: lingxi_kernel::subagent::RunLineage,
            _now_unix_ms: u64,
        ) -> Result<(), StorageError> {
            // Lineage durability is covered against the real adapter.
            Ok(())
        }

        async fn load_run_lineage(
            &self,
            _run_id: &RunId,
        ) -> Result<Option<lingxi_kernel::subagent::RunLineage>, StorageError> {
            Ok(None)
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
            fail_start: false,
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
            fail_start: false,
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
            fail_start: false,
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
            fail_start: false,
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
            fail_start: false,
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

    // ── R03 repair G04/F05: the admission lifecycle through the REAL
    //    store-less unit harness (ownership + gate + dedup real; the port
    //    is the shared test fake whose start write is failable). ────────

    /// C02 (unit leg): a run-start failure under an explicit requestId
    /// retracts the reservation (verified absent against the port) — the
    /// SAME id re-admits fresh and only ONE run ever exists; no ghost
    /// replay can survive the failure.
    #[tokio::test]
    async fn start_failure_under_request_id_re_admits_the_same_key_fresh() {
        let (store, runs) = store_with_runs();
        let port = FakePort {
            fail_outcome: false,
            fail_start: true,
            runs: std::sync::Arc::clone(&runs),
            outcomes: StdMutex::new(Vec::new()),
        };
        let (events, dir) = event_service_for_test().await;
        let owner = owner_principal();
        let submission = ExecuteSubmission {
            input: "hello",
            request_id: Some("unit-c02"),
        };

        // The start transaction fails: an explicit error, nothing durable.
        match store
            .execute_submission_for(
                &port,
                &events,
                &supervisor(),
                &owner,
                "sess_local_alpha",
                &submission,
                1234,
            )
            .await
        {
            Err(SessionExecuteError::Storage(StorageError::Io { detail })) => {
                assert!(detail.contains("run-start"))
            }
            other => panic!("start failure must surface, got {other:?}"),
        }
        assert_eq!(store.run_count("sess_local_alpha").await.unwrap(), 0);

        // Storage recovered (a healthy port over the SAME store): the
        // SAME id re-admits fresh — exactly one valid execution, and the
        // post-settle retry replays that real run.
        let healed = FakePort {
            fail_outcome: false,
            fail_start: false,
            runs,
            outcomes: StdMutex::new(Vec::new()),
        };
        let accepted = store
            .execute_submission_for(
                &healed,
                &events,
                &supervisor(),
                &owner,
                "sess_local_alpha",
                &submission,
                1235,
            )
            .await
            .expect("the same id re-admits after the failure is compensated");
        assert!(!accepted.replayed, "the retry is a REAL fresh start");
        assert_eq!(store.run_count("sess_local_alpha").await.unwrap(), 1);
        let replay = store
            .execute_submission_for(
                &healed,
                &events,
                &supervisor(),
                &owner,
                "sess_local_alpha",
                &submission,
                1236,
            )
            .await
            .expect("post-settle retry replays");
        assert!(replay.replayed);
        assert_eq!(replay.run_id, accepted.run_id);
        assert_eq!(store.run_count("sess_local_alpha").await.unwrap(), 1);
        drop(events);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// C03 (unit leg): a same-key duplicate that arrives while the first
    /// submission is parked between reservation and durable start gets
    /// the explicit retryable in-flight refusal — never a half-committed
    /// fake replay of a run that does not exist.
    #[tokio::test(flavor = "current_thread")]
    async fn same_key_duplicate_in_the_start_window_is_in_flight_not_a_ghost() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let (store, runs) = store_with_runs();
        let parked = std::sync::Arc::new(AtomicBool::new(false));
        let release = std::sync::Arc::new(AtomicBool::new(false));
        // A port whose start write parks until released: the deterministic
        // reservation→durable-start window.
        struct ParkingPort {
            inner: FakePort,
            parked: std::sync::Arc<AtomicBool>,
            release: std::sync::Arc<AtomicBool>,
        }
        impl StoragePort for ParkingPort {
            async fn record_run_started(
                &self,
                ctx: &RunContext,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                self.parked.store(true, Ordering::SeqCst);
                while !self.release.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
                StoragePort::record_run_started(&self.inner, ctx, now_unix_ms).await
            }
            async fn commit_run_outcome(
                &self,
                ctx: &RunContext,
                outcome: lingxi_kernel::ports::RunOutcome,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                StoragePort::commit_run_outcome(&self.inner, ctx, outcome, now_unix_ms).await
            }
            async fn load_run(
                &self,
                run_id: &RunId,
            ) -> Result<Option<lingxi_kernel::ports::RunRecord>, StorageError> {
                StoragePort::load_run(&self.inner, run_id).await
            }
            async fn record_run_events(
                &self,
                ctx: &RunContext,
                events: Vec<KeyEvent>,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                StoragePort::record_run_events(&self.inner, ctx, events, now_unix_ms).await
            }
            async fn record_stale_result(
                &self,
                ctx: &RunContext,
                refused: lingxi_kernel::ports::StaleResultFact,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                StoragePort::record_stale_result(&self.inner, ctx, refused, now_unix_ms).await
            }
            async fn record_attempt_started(
                &self,
                ctx: &RunContext,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                StoragePort::record_attempt_started(&self.inner, ctx, now_unix_ms).await
            }
            async fn record_run_state_change(
                &self,
                ctx: &RunContext,
                from: lingxi_protocol::RunStatus,
                to: lingxi_protocol::RunStatus,
                reason: Option<String>,
                now_unix_ms: u64,
            ) -> Result<CommittedOutcome, StorageError> {
                StoragePort::record_run_state_change(
                    &self.inner,
                    ctx,
                    from,
                    to,
                    reason,
                    now_unix_ms,
                )
                .await
            }
            async fn record_invocation_intent(
                &self,
                ctx: &RunContext,
                intent: lingxi_kernel::ports::InvocationIntent,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                StoragePort::record_invocation_intent(&self.inner, ctx, intent, now_unix_ms).await
            }
            async fn advance_invocation(
                &self,
                ctx: &RunContext,
                journal_id: &ToolCallId,
                to: lingxi_kernel::ports::InvocationPhase,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                StoragePort::advance_invocation(&self.inner, ctx, journal_id, to, now_unix_ms).await
            }
            async fn record_invocation_receipt(
                &self,
                ctx: &RunContext,
                journal_id: &ToolCallId,
                receipt: lingxi_kernel::ports::InvocationReceipt,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                StoragePort::record_invocation_receipt(
                    &self.inner,
                    ctx,
                    journal_id,
                    receipt,
                    now_unix_ms,
                )
                .await
            }
            async fn record_invocation_unknown(
                &self,
                journal_id: &ToolCallId,
                detail: String,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                StoragePort::record_invocation_unknown(&self.inner, journal_id, detail, now_unix_ms)
                    .await
            }
            async fn load_invocation_journal(
                &self,
                run_id: &RunId,
            ) -> Result<Vec<lingxi_kernel::ports::InvocationJournalEntry>, StorageError>
            {
                StoragePort::load_invocation_journal(&self.inner, run_id).await
            }
            async fn record_run_lineage(
                &self,
                ctx: &RunContext,
                lineage: lingxi_kernel::subagent::RunLineage,
                now_unix_ms: u64,
            ) -> Result<(), StorageError> {
                StoragePort::record_run_lineage(&self.inner, ctx, lineage, now_unix_ms).await
            }
            async fn load_run_lineage(
                &self,
                run_id: &RunId,
            ) -> Result<Option<lingxi_kernel::subagent::RunLineage>, StorageError> {
                StoragePort::load_run_lineage(&self.inner, run_id).await
            }
        }
        let port = std::sync::Arc::new(ParkingPort {
            inner: FakePort {
                fail_outcome: false,
                fail_start: false,
                runs,
                outcomes: StdMutex::new(Vec::new()),
            },
            parked: std::sync::Arc::clone(&parked),
            release: std::sync::Arc::clone(&release),
        });
        let (events, dir) = event_service_for_test().await;
        let owner = owner_principal();
        let submission = ExecuteSubmission {
            input: "hello",
            request_id: Some("unit-c03"),
        };

        // The first submission parks between reservation and durable start.
        let store_a = std::sync::Arc::new(store);
        let events_a = std::sync::Arc::new(events);
        let first = tokio::spawn({
            let store = std::sync::Arc::clone(&store_a);
            let events = std::sync::Arc::clone(&events_a);
            let port = std::sync::Arc::clone(&port);
            let owner = owner.clone();
            async move {
                store
                    .execute_submission_for(
                        port.as_ref(),
                        events.as_ref(),
                        &supervisor(),
                        &owner,
                        "sess_local_alpha",
                        &submission,
                        1234,
                    )
                    .await
            }
        });
        while !parked.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }

        // The concurrent same-key duplicate: explicit in-flight refusal.
        match store_a
            .execute_submission_for(
                port.as_ref(),
                events_a.as_ref(),
                &supervisor(),
                &owner,
                "sess_local_alpha",
                &submission,
                1234,
            )
            .await
        {
            Err(SessionExecuteError::AdmissionInFlight { request_id }) => {
                assert_eq!(request_id, "unit-c03");
            }
            other => panic!("in-window duplicate must be in-flight, got {other:?}"),
        }
        // Changed content under the same key still conflicts.
        let changed = ExecuteSubmission {
            input: "changed",
            request_id: Some("unit-c03"),
        };
        match store_a
            .execute_submission_for(
                port.as_ref(),
                events_a.as_ref(),
                &supervisor(),
                &owner,
                "sess_local_alpha",
                &changed,
                1234,
            )
            .await
        {
            Err(SessionExecuteError::DuplicateRequestConflict { .. }) => {}
            other => panic!("changed content must conflict, got {other:?}"),
        }

        // Release: the first submission commits and settles; the retry
        // then replays the REAL run.
        release.store(true, Ordering::SeqCst);
        let accepted = first.await.expect("task").expect("first completes");
        assert!(!accepted.replayed);
        let replay = store_a
            .execute_submission_for(
                port.as_ref(),
                events_a.as_ref(),
                &supervisor(),
                &owner,
                "sess_local_alpha",
                &submission,
                1237,
            )
            .await
            .expect("post-settle retry replays");
        assert!(replay.replayed);
        assert_eq!(replay.run_id, accepted.run_id);
        assert_eq!(store_a.run_count("sess_local_alpha").await.unwrap(), 1);
        drop(events_a);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[allow(unused)]
    fn _key_event_shape_compiles(_: KeyEvent) {}
}
