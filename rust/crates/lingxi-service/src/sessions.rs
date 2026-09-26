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
use lingxi_kernel::{Principal as KernelPrincipal, RunContext};
use lingxi_protocol::{AttemptId, RunId};
use lingxi_protocol::{
    EventPayload, KnownEventPayload, RunStateChangedPayload, RunStatus, SessionId as WireSessionId,
};

use crate::auth::{Principal, PrincipalKind, LOCAL_OWNER_USER_ID};

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
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecuteRequest {
    /// Free-form input; bounded by the body limit. Any identity-ish fields
    /// are structurally rejected here (deny_unknown_fields) — principal
    /// comes from the auth chain, never from the payload.
    pub input: String,
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
}

pub struct SessionStore {
    backend: Box<dyn SessionBackendErased>,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionStore(<storage-backed>)")
    }
}

impl SessionStore {
    /// Wraps a real storage backend.
    pub fn new(backend: impl SessionBackend + 'static) -> Self {
        Self {
            backend: Box::new(backend),
        }
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

    /// Executes: persists a run (start + terminal outcome with its key
    /// events, each as one storage-port transaction) bound to the
    /// authenticated principal. Storage failures surface as
    /// [`SessionExecuteError::Storage`] — no visible success (R02-A07).
    ///
    /// The R02 representative run completes WITHOUT a final assistant
    /// message on purpose: completion state and delivery quality are
    /// separate facts (contract 02 §4) and a final message must never be
    /// fabricated; real model turns arrive with R05.
    pub async fn execute_for<P: StoragePort>(
        &self,
        port: &P,
        principal: &Principal,
        session_id: &str,
        input: &str,
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

        // Bound what we record (defense in depth; the body limit already
        // bounds the request).
        let recorded_input: String = input.chars().take(2000).collect();
        let run_seq = self
            .backend
            .total_runs_erased()
            .await
            .map_err(SessionExecuteError::Storage)?
            + 1;
        let run_id = format!("run_{:016x}_{:06x}", now_ms, run_seq);

        let ctx = RunContext {
            principal: kernel_principal_of(principal),
            session_id: WireSessionId::new(session_id.to_string()),
            run_id: RunId::new(run_id.clone()),
            attempt: AttemptId::new(format!("{run_id}#a1")),
            generation: 1,
        };

        // 1) Durable run start (one transaction: run row + attempt row +
        //    run_state_changed queued→running key event).
        port.record_run_started(&ctx, now_ms)
            .await
            .map_err(SessionExecuteError::Storage)?;

        // 2) Terminal outcome in one transaction with its key event.
        //    Failed -> no success response, no completion event visible.
        let outcome = lingxi_kernel::ports::RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            key_events: vec![lingxi_kernel::ports::KeyEvent {
                event_id: lingxi_protocol::EventId::new(format!("{run_id}-done")),
                payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                    RunStateChangedPayload {
                        from: RunStatus::Running,
                        to: RunStatus::Completed,
                        reason: None,
                    },
                )),
            }],
            final_message: None,
        };
        let committed = port
            .commit_run_outcome(&ctx, outcome, now_ms)
            .await
            .map_err(SessionExecuteError::Storage)?;
        // Publication happens HERE, strictly after the commit returned Ok:
        // until R02-T05 wires the event streams, publication is the HTTP
        // response plus this audit log line.
        tracing::info!(
            run_id = %run_id,
            session_id = %session_id,
            events = %committed
                .events
                .iter()
                .map(|e| e.event_id.to_string())
                .collect::<Vec<_>>()
                .join(","),
            input_chars = recorded_input.chars().count(),
            "run committed and published (post-commit)"
        );

        let run_count = self
            .backend
            .count_runs_erased(session_id)
            .await
            .map_err(SessionExecuteError::Storage)?;
        Ok(ExecuteAccepted { run_id, run_count })
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
    use lingxi_kernel::ports::{CommittedOutcome, KeyEvent};
    use std::sync::Mutex as StdMutex;

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
    }

    fn store_with_runs() -> (SessionStore, SharedRuns) {
        let runs: SharedRuns = std::sync::Arc::new(StdMutex::new(Vec::new()));
        (
            SessionStore::new(MemoryBackend {
                sessions: SessionStore::seed_rows(1000),
                runs: std::sync::Arc::clone(&runs),
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
        let owner = owner_principal();
        assert_eq!(store.list_for(&owner).await.unwrap().len(), 2);

        let accepted = store
            .execute_for(&port, &owner, "sess_local_alpha", "hello", 1234)
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
    }

    #[tokio::test]
    async fn cross_principal_access_is_forbidden_not_found() {
        let (store, runs) = store_with_runs();
        let port = FakePort {
            fail_outcome: false,
            runs,
            outcomes: StdMutex::new(Vec::new()),
        };
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
            .execute_for(&port, &foreign, "sess_local_alpha", "inject", 1500)
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
        let owner = owner_principal();
        match store
            .execute_for(&port, &owner, "sess_local_alpha", "hello", 1234)
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
