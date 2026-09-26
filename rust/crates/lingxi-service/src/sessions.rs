//! Representative business surface for R02-T03: a minimal in-memory session
//! store backing one "read" endpoint (`GET /lingxi/v1/sessions/{id}`) and
//! one "execute" endpoint (`POST /lingxi/v1/sessions/{id}/execute`).
//!
//! Design boundary (taskbook): these endpoints are NOT shells — every call
//! runs the real transport guard → authentication → route authorization →
//! per-resource ownership chain, and a successful execute really mutates
//! observable server state (a run record bound to the authenticated
//! principal), which is exactly what the R02-A05 "no side effect on denial"
//! evidence snapshots around. The storage/event internals are deliberately
//! minimal and will be replaced by the R02-T04 (SQLite storage port) and
//! R02-T05 (event streams) implementations; the endpoint shapes (ids,
//! ownership, run records) are the part future tasks keep.
//!
//! Every session is owned by a user id; the local owner (loopback token)
//! sees everything, a device principal only its own user's sessions —
//! the cross-principal boundary that acceptance R02-A05 exercises with a
//! foreign sessionId.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::auth::{Principal, PrincipalKind, LOCAL_OWNER_USER_ID};

/// A recorded execution (the minimal run fact; R02-T03 records it, R02-T04
/// persists it, R02-T05 streams it).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub run_id: String,
    pub principal_id: String,
    pub credential_kind: String,
    pub input: String,
    pub started_at_unix_ms: u64,
}

/// A session (minimal shape: identity, owner, title, runs).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub session_id: String,
    pub agent_id: String,
    pub owner_user_id: String,
    pub title: String,
    pub created_at_unix_ms: u64,
    pub runs: Vec<RunRecord>,
}

/// Summary shape returned by the read endpoint (no input echo of failed
/// attempts; runs carry only committed facts).
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
    Ok(SessionRecord),
    NotFound,
    Forbidden,
}

/// Error of the execute mutation (keeps the Ok variant free of payloads).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionExecuteError {
    NotFound,
    Forbidden,
}

impl From<SessionAccess> for SessionExecuteError {
    fn from(value: SessionAccess) -> Self {
        match value {
            SessionAccess::Ok(_) => unreachable!("Ok is not an execute error"),
            SessionAccess::NotFound => Self::NotFound,
            SessionAccess::Forbidden => Self::Forbidden,
        }
    }
}

pub struct SessionStore {
    inner: Mutex<SessionStoreInner>,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionStore(<state>)")
    }
}

struct SessionStoreInner {
    sessions: Vec<SessionRecord>,
    run_seq: u64,
}

impl SessionStore {
    /// Seeds the synthetic R02 sessions (owned by the local owner user).
    pub fn seeded(now_ms: u64) -> Self {
        let sessions = vec![
            SessionRecord {
                session_id: "sess_local_alpha".to_string(),
                agent_id: "lingxi".to_string(),
                owner_user_id: LOCAL_OWNER_USER_ID.to_string(),
                title: "Synthetic session alpha".to_string(),
                created_at_unix_ms: now_ms,
                runs: Vec::new(),
            },
            SessionRecord {
                session_id: "sess_local_beta".to_string(),
                agent_id: "lingxi".to_string(),
                owner_user_id: LOCAL_OWNER_USER_ID.to_string(),
                title: "Synthetic session beta".to_string(),
                created_at_unix_ms: now_ms,
                runs: Vec::new(),
            },
        ];
        Self {
            inner: Mutex::new(SessionStoreInner {
                sessions,
                run_seq: 0,
            }),
        }
    }

    /// Ownership rule: the local owner sees everything; any other
    /// principal only sessions owned by its own user id.
    pub fn can_access(principal: &Principal, session: &SessionRecord) -> bool {
        if principal.is_local_owner() {
            return true;
        }
        principal.user_id.as_deref() == Some(session.owner_user_id.as_str())
    }

    /// Lists the sessions visible to `principal` (identity projection of
    /// the store; read-only).
    pub fn list_for(&self, principal: &Principal) -> Vec<SessionView> {
        let inner = self.lock();
        inner
            .sessions
            .iter()
            .filter(|s| Self::can_access(principal, s))
            .map(session_view)
            .collect()
    }

    /// Reads one session under the ownership rule.
    pub fn get_for(&self, principal: &Principal, session_id: &str) -> SessionAccess {
        let inner = self.lock();
        match inner.sessions.iter().find(|s| s.session_id == session_id) {
            None => SessionAccess::NotFound,
            Some(session) if Self::can_access(principal, session) => {
                SessionAccess::Ok(session.clone())
            }
            Some(_) => SessionAccess::Forbidden,
        }
    }

    /// Executes: appends a run record bound to the authenticated principal.
    /// This is the write side effect that R02-A05 proves does NOT happen on
    /// denied requests.
    pub fn execute_for(
        &self,
        principal: &Principal,
        session_id: &str,
        input: &str,
        now_ms: u64,
    ) -> Result<ExecuteAccepted, SessionExecuteError> {
        // Bound what we record (defense in depth; the body limit already
        // bounds the request).
        let recorded_input: String = input.chars().take(2000).collect();
        let mut inner = self.lock();
        let Some(index) = inner
            .sessions
            .iter()
            .position(|s| s.session_id == session_id)
        else {
            return Err(SessionExecuteError::NotFound);
        };
        if !Self::can_access(principal, &inner.sessions[index]) {
            return Err(SessionExecuteError::Forbidden);
        }
        inner.run_seq += 1;
        let run_id = format!("run_{:016x}_{}", now_ms, inner.run_seq);
        let record = RunRecord {
            run_id: run_id.clone(),
            principal_id: principal.principal_id.clone(),
            credential_kind: credential_kind_name(principal),
            input: recorded_input,
            started_at_unix_ms: now_ms,
        };
        let session = &mut inner.sessions[index];
        session.runs.push(record);
        Ok(ExecuteAccepted {
            run_id,
            run_count: session.runs.len() as u64,
        })
    }

    /// Test/evidence helper: observable run count for a session.
    pub fn run_count(&self, session_id: &str) -> Option<u64> {
        let inner = self.lock();
        inner
            .sessions
            .iter()
            .find(|s| s.session_id == session_id)
            .map(|s| s.runs.len() as u64)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SessionStoreInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn session_view(session: &SessionRecord) -> SessionView {
    SessionView {
        session_id: session.session_id.clone(),
        agent_id: session.agent_id.clone(),
        owner_user_id: session.owner_user_id.clone(),
        title: session.title.clone(),
        run_count: session.runs.len() as u64,
        last_runs: session
            .runs
            .iter()
            .rev()
            .take(5)
            .map(|r| RunSummary {
                run_id: r.run_id.clone(),
                principal_id: r.principal_id.clone(),
                started_at_unix_ms: r.started_at_unix_ms,
            })
            .collect(),
    }
}

fn credential_kind_name(principal: &Principal) -> String {
    match principal.kind {
        PrincipalKind::LocalUser => "loopback_token".to_string(),
        PrincipalKind::Device => "device_credential".to_string(),
        PrincipalKind::Unknown => "none".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{ConnectionKindSerde, CredentialKind, PrincipalKind as Pk, TrustState};

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

    #[test]
    fn owner_sees_everything_and_execute_records_runs() {
        let store = SessionStore::seeded(1000);
        let owner = owner_principal();
        assert_eq!(store.list_for(&owner).len(), 2);

        let accepted = store
            .execute_for(&owner, "sess_local_alpha", "hello", 1234)
            .unwrap();
        assert_eq!(accepted.run_count, 1);
        assert_eq!(store.run_count("sess_local_alpha"), Some(1));
        assert_eq!(
            store.run_count("sess_local_beta"),
            Some(0),
            "other session untouched"
        );

        match store.get_for(&owner, "sess_local_alpha") {
            SessionAccess::Ok(_) => {}
            other => panic!("owner read must succeed, got {other:?}"),
        }
    }

    #[test]
    fn cross_principal_access_is_forbidden_not_found() {
        let store = SessionStore::seeded(1000);
        let foreign = device_principal("user_remote_b");
        assert!(
            store.list_for(&foreign).is_empty(),
            "no sessions owned by user_remote_b"
        );
        match store.get_for(&foreign, "sess_local_alpha") {
            SessionAccess::Forbidden => {}
            other => panic!("cross-principal read must be Forbidden, got {other:?}"),
        }
        match store.execute_for(&foreign, "sess_local_alpha", "inject", 1500) {
            Err(SessionExecuteError::Forbidden) => {}
            other => panic!("cross-principal execute must be Forbidden, got {other:?}"),
        }
        assert_eq!(
            store.run_count("sess_local_alpha"),
            Some(0),
            "denied execute must leave zero side effects"
        );
        match store.get_for(&foreign, "sess_missing") {
            SessionAccess::NotFound => {}
            other => panic!("unknown session must be NotFound, got {other:?}"),
        }
    }

    #[test]
    fn same_user_device_principal_can_access() {
        let store = SessionStore::seeded(1000);
        let same_user = device_principal(LOCAL_OWNER_USER_ID);
        match store.get_for(&same_user, "sess_local_beta") {
            SessionAccess::Ok(_) => {}
            other => panic!("same-user device read must succeed, got {other:?}"),
        }
    }

    #[test]
    fn execute_request_rejects_identity_shaped_payload_fields() {
        // deny_unknown_fields: smuggling a principalId into the payload is a
        // hard parse error, not a silently ignored field.
        let raw = r#"{"input":"x","principalId":"forged"}"#;
        let err = serde_json::from_str::<ExecuteRequest>(raw).unwrap_err();
        assert!(err.to_string().contains("unknown field"), "err: {err}");
    }
}
