//! R03-T04 integration: submission-surface requestId dedup through the
//! REAL service composition (R03-A08 相同 requestId 不同内容拒绝).
//!
//! The dedup key is bound to the CALLING principal and THE session, and
//! the recorded value is the NORMALIZED-content digest of the accepted
//! submission:
//! - same id + changed content → an explicit CONFLICT: the recorded
//!   execution is NOT reused, NO new execution starts;
//! - same id + same content → an idempotent REPLAY of the original
//!   acceptance (the original run id, `replayed: true`, nothing
//!   re-executed);
//! - different sessions / different principals sharing the same id string
//!   never cross-use each other's submissions (no global namespace);
//! - absent id → the exact pre-T04 submission path.
//!
//! Test tier: contract/service-integration against the real composition
//! root (real storage, real session gate, real run supervisor). The
//! provider is the production default (none) so runs settle immediately
//! with the explicit `completed.no_final.no_provider_configured` outcome —
//! run COUNTS are the reuse/no-new-execution witnesses.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use lingxi_service::{
    ExecuteSubmission, HomeSource, NetworkMode, Principal, PrincipalKind, ServiceConfig,
    ServiceDeps, ServiceState, SessionExecuteError,
};

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03t04-dedup-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn config_for(home: &std::path::Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

fn owner_principal() -> Principal {
    Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
    }
}

/// A same-user DEVICE principal: a DIFFERENT trusted principal kind that
/// still owns the seeded sessions (the namespace-separation witness).
fn device_principal_of_local_user() -> Principal {
    Principal {
        schema_version: 1,
        principal_id: "principal_device_local".to_string(),
        kind: PrincipalKind::Device,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: Some("device_a".to_string()),
        credential_id: None,
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Lan,
        credential_kind: lingxi_service::CredentialKind::DeviceCredential,
        trust_state: lingxi_service::TrustState::Lan,
        scopes: vec!["chat".to_string()],
    }
}

async fn boot(tag: &str) -> (ServiceState, PathBuf) {
    let home = synthetic_home(tag);
    let layout = lingxi_service::prepare_layout(&home).expect("layout");
    let state =
        ServiceState::bootstrap_with_deps(config_for(&home), &layout, ServiceDeps::default())
            .await
            .expect("bootstrap");
    (state, home)
}

async fn teardown(state: &ServiceState, home: &std::path::Path) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

async fn submit(
    state: &ServiceState,
    principal: &Principal,
    session: &'static str,
    input: &str,
    request_id: Option<&str>,
) -> Result<lingxi_service::ExecuteAccepted, SessionExecuteError> {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_submission_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            principal,
            session,
            &ExecuteSubmission { input, request_id },
            1_790_409_600_000,
        )
        .await
}

async fn runs_of_session(state: &ServiceState, session: &str) -> i64 {
    state
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
            vec![session.to_string()],
        )
        .await
        .expect("query")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Test evidence sink (the established R02/R03 env-var pattern).
fn write_evidence(key: &str, value: serde_json::Value) {
    if let Ok(path) = std::env::var("R03_T04_EVIDENCE") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut doc = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(map) = doc.as_object_mut() {
            map.insert(key.to_string(), value);
        }
        let _ = std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap());
    }
}

/// R03-A08 (acceptance literal): the first submission has been accepted;
/// resubmitting the SAME requestId with CHANGED content must return a
/// conflict, reuse no old execution and start no new one.
#[tokio::test(flavor = "current_thread")]
async fn r03_a08_same_request_id_different_content_is_rejected_as_conflict() {
    let (state, home) = boot("a08-conflict").await;
    let owner = owner_principal();

    // First submission ACCEPTED (and settled: no provider is configured).
    let first = submit(
        &state,
        &owner,
        "sess_local_alpha",
        "original accepted content",
        Some("client-req-0001"),
    )
    .await
    .expect("first submission accepted");
    assert!(!first.replayed);
    let runs_after_first = runs_of_session(&state, "sess_local_alpha").await;
    assert_eq!(runs_after_first, 1);

    // SAME id, CHANGED content → explicit conflict.
    match submit(
        &state,
        &owner,
        "sess_local_alpha",
        "changed content under the same id",
        Some("client-req-0001"),
    )
    .await
    {
        Err(SessionExecuteError::DuplicateRequestConflict {
            request_id,
            recorded_digest,
            submitted_digest,
        }) => {
            assert_eq!(request_id, "client-req-0001");
            assert_ne!(recorded_digest, submitted_digest);
        }
        other => panic!("same id different content must conflict, got {other:?}"),
    }

    // 不复用旧执行、不新增执行: exactly the ONE run of the first
    // submission exists — no second execution was started, and the
    // conflict did not silently fall back to a fresh run.
    assert_eq!(
        runs_of_session(&state, "sess_local_alpha").await,
        runs_after_first,
        "the conflicting resubmission must not create a run"
    );

    write_evidence(
        "r03_a08_conflict",
        serde_json::json!({
            "request_id": "client-req-0001",
            "first_run": first.run_id,
            "runs_after_first": runs_after_first,
            "runs_after_conflict": runs_of_session(&state, "sess_local_alpha").await,
        }),
    );
    println!(
        "R03_A08_TRACE conflict: id=client-req-0001 runs={} (unchanged)",
        runs_after_first
    );
    teardown(&state, &home).await;
}

/// The idempotent half: the same id with the SAME (normalized) content
/// replays the original acceptance — the SAME run id, no second
/// execution. A retry of a lost response is exactly this shape.
#[tokio::test(flavor = "current_thread")]
async fn same_request_id_same_content_replays_without_new_execution() {
    let (state, home) = boot("a08-replay").await;
    let owner = owner_principal();

    let first = submit(
        &state,
        &owner,
        "sess_local_alpha",
        "line one\nline two",
        Some("req-r1"),
    )
    .await
    .expect("accepted");
    // Replay with the SAME content typed with CRLF endings: the same
    // normalized submission (trailing newlines are CONTENT and are not
    // folded — only line-ending style is).
    let replay = submit(
        &state,
        &owner,
        "sess_local_alpha",
        "line one\r\nline two",
        Some("req-r1"),
    )
    .await
    .expect("idempotent replay");
    assert_eq!(replay.run_id, first.run_id, "the original run is returned");
    assert!(replay.replayed, "the response is marked as a replay");
    assert_eq!(runs_of_session(&state, "sess_local_alpha").await, 1);

    // The conflict path is content-bound, not id-bound: after the replay,
    // changed content under the same id still conflicts.
    assert!(matches!(
        submit(
            &state,
            &owner,
            "sess_local_alpha",
            "different",
            Some("req-r1")
        )
        .await,
        Err(SessionExecuteError::DuplicateRequestConflict { .. })
    ));
    assert_eq!(runs_of_session(&state, "sess_local_alpha").await, 1);

    write_evidence(
        "r03_a08_replay",
        serde_json::json!({
            "request_id": "req-r1",
            "first_run": first.run_id,
            "replayed_run": replay.run_id,
            "same_run": first.run_id == replay.run_id,
            "runs_total": 1,
        }),
    );
    println!("R03_A08_TRACE replay: same run id returned, runs_total=1");
    teardown(&state, &home).await;
}

/// The key is bound to the trusted principal AND the session: the same id
/// string in another session (or from another principal kind) is its own
/// submission — a global requestId namespace must never let different
/// subjects cross-use each other's ids.
#[tokio::test(flavor = "current_thread")]
async fn request_id_namespace_is_scoped_to_principal_and_session() {
    let (state, home) = boot("a08-scope").await;
    let owner = owner_principal();
    let device = device_principal_of_local_user();

    // Same principal, DIFFERENT session, same id + different content: its
    // own namespace — accepted, not a conflict.
    let alpha = submit(
        &state,
        &owner,
        "sess_local_alpha",
        "alpha content",
        Some("shared-id"),
    )
    .await
    .expect("alpha accepts");
    let beta = submit(
        &state,
        &owner,
        "sess_local_beta",
        "beta content",
        Some("shared-id"),
    )
    .await
    .expect("beta accepts: the id namespace is per-session");
    assert_ne!(alpha.run_id, beta.run_id);
    assert_eq!(runs_of_session(&state, "sess_local_alpha").await, 1);
    assert_eq!(runs_of_session(&state, "sess_local_beta").await, 1);

    // A DIFFERENT principal kind on the SAME session with the SAME id:
    // its own namespace — accepted and independent.
    let device_run = submit(
        &state,
        &device,
        "sess_local_alpha",
        "device principal content",
        Some("shared-id"),
    )
    .await
    .expect("device principal accepts: the id namespace is per-principal");
    assert_ne!(device_run.run_id, alpha.run_id);
    assert_eq!(runs_of_session(&state, "sess_local_alpha").await, 2);

    // Cross-principal replay attempts do NOT see each other's bindings:
    // the owner replaying 'shared-id' still replays ITS alpha run.
    let owner_replay = submit(
        &state,
        &owner,
        "sess_local_alpha",
        "alpha content",
        Some("shared-id"),
    )
    .await
    .expect("owner replay");
    assert_eq!(owner_replay.run_id, alpha.run_id);
    assert!(owner_replay.replayed);
    assert_eq!(runs_of_session(&state, "sess_local_alpha").await, 2);

    write_evidence(
        "r03_a08_namespace",
        serde_json::json!({
            "alpha_run": alpha.run_id,
            "beta_run": beta.run_id,
            "device_run": device_run.run_id,
            "owner_replay_run": owner_replay.run_id,
            "alpha_runs_total": 2,
        }),
    );
    println!("R03_A08_TRACE namespace: per-principal+session scoping held");
    teardown(&state, &home).await;
}

/// Invalid ids are refused loudly before any side effect; submissions
/// WITHOUT an id keep the exact pre-T04 behavior (no dedup at all).
#[tokio::test(flavor = "current_thread")]
async fn invalid_ids_are_refused_and_plain_submissions_are_undeduplicated() {
    let (state, home) = boot("a08-invalid").await;
    let owner = owner_principal();

    for bad in ["   ", &"x".repeat(200)] {
        match submit(&state, &owner, "sess_local_alpha", "x", Some(bad)).await {
            Err(SessionExecuteError::InvalidRequestId { detail }) => {
                assert!(!detail.is_empty());
            }
            other => panic!("invalid id must be refused, got {other:?}"),
        }
    }
    assert_eq!(
        runs_of_session(&state, "sess_local_alpha").await,
        0,
        "refused submissions write nothing"
    );

    // No id → no dedup: identical inputs are distinct submissions.
    for _ in 0..3 {
        submit(&state, &owner, "sess_local_alpha", "identical", None)
            .await
            .expect("plain submissions keep working");
    }
    assert_eq!(
        runs_of_session(&state, "sess_local_alpha").await,
        3,
        "without an explicit id there is nothing to deduplicate"
    );

    write_evidence(
        "r03_a08_invalid_and_plain",
        serde_json::json!({
            "invalid_refused_runs": 0,
            "plain_runs": 3,
        }),
    );
    println!("R03_A08_TRACE invalid/plain: invalid refused loudly; plain path unchanged");
    teardown(&state, &home).await;
}
