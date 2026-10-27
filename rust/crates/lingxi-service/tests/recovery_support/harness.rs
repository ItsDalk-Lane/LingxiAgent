//! Shared harness of the R03-T07 permanent tests (startup scan, exit race,
//! crash points). Mirrors the T05/T06 fixtures: REAL composition root
//! (`ServiceState::bootstrap_with_deps`), REAL `RunDatabase`, no sleeps —
//! the durable crash shapes are committed through the SAME StoragePort
//! write sequence the run driver produces, then the "previous process" is
//! dropped (or kill -9'd, in the crash-point suite) with the run still
//! active.

#![allow(dead_code)]

use std::sync::Arc;

use lingxi_kernel::invocation::ToolRecoveryCapability;
use lingxi_kernel::ports::{InvocationIntent, InvocationPhase, StoragePort};
use lingxi_kernel::Principal as KernelPrincipal;
use lingxi_protocol::{AttemptId, RunId, SessionId, ToolCallId};
use lingxi_service::invocations::RecoveryCapabilitySource;
use lingxi_service::{ServiceConfig, ServiceDeps, ServiceState, LOCAL_OWNER_USER_ID};

pub fn synthetic_home(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03t07-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

pub fn config_for(home: &std::path::Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0"
            .parse::<std::net::SocketAddr>()
            .expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: lingxi_service::HomeSource::Cli,
        network_mode: lingxi_service::NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

pub fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(LOCAL_OWNER_USER_ID.to_string()),
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

/// The kernel write context of one run (the shape every port write
/// validates against).
pub fn run_context(run_id: &str) -> lingxi_kernel::RunContext {
    lingxi_kernel::RunContext {
        principal: KernelPrincipal::LocalUser,
        session_id: SessionId::new("sess_local_alpha".to_string()),
        run_id: RunId::new(run_id.to_string()),
        attempt: AttemptId::new(format!("{run_id}#a1")),
        generation: 1,
    }
}

/// The durable crash-window journal shape: intent → authorized → started,
/// NO receipt (exactly the write order the run driver produces around the
/// external execution; T05's live-chain tests prove that order).
pub async fn journal_started(
    state: &ServiceState,
    ctx: &lingxi_kernel::RunContext,
    target: &str,
    idempotency_key: Option<String>,
) {
    journal_at(
        state,
        ctx,
        target,
        idempotency_key,
        InvocationPhase::Started,
    )
    .await;
}

/// A journal entry left at an arbitrary pre-receipt phase.
pub async fn journal_at(
    state: &ServiceState,
    ctx: &lingxi_kernel::RunContext,
    target: &str,
    idempotency_key: Option<String>,
    phase: InvocationPhase,
) {
    let journal_id = ToolCallId::new(format!("{}-tc0001", ctx.run_id.as_str()));
    state
        .storage()
        .record_invocation_intent(
            ctx,
            InvocationIntent {
                journal_id: journal_id.clone(),
                target: target.to_string(),
                args_digest: "ab".to_string(),
                args_summary: Some(format!("{target} fixed payload")),
                idempotency_key,
            },
            1_000,
        )
        .await
        .expect("journal intent");
    if phase != InvocationPhase::Prepared {
        state
            .storage()
            .advance_invocation(ctx, &journal_id, InvocationPhase::Authorized, 1_001)
            .await
            .expect("authorized");
    }
    if phase == InvocationPhase::Started {
        state
            .storage()
            .advance_invocation(ctx, &journal_id, InvocationPhase::Started, 1_002)
            .await
            .expect("started");
    }
}

/// Which durable journal shape the "previous process" leaves behind.
pub struct CrashShape {
    pub target: String,
    pub idempotency_key: Option<String>,
    pub phase: InvocationPhase,
}

/// Boots a "previous process" over `home` and leaves ONE running run with
/// the given journal shape behind (the run stays active — the process is
/// then closed/dropped/killed by the caller).
pub async fn crash_shape(home: &std::path::Path, shape: CrashShape) -> (ServiceState, String) {
    let layout = lingxi_service::prepare_layout(home).expect("layout");
    let state = ServiceState::bootstrap(config_for(home), &layout)
        .await
        .expect("bootstrap previous process");
    let ctx = run_context("run_x");
    state
        .storage()
        .record_run_started(&ctx, 1_000)
        .await
        .expect("run started");
    journal_at(
        &state,
        &ctx,
        &shape.target,
        shape.idempotency_key,
        shape.phase,
    )
    .await;
    (state, "run_x".to_string())
}

/// A capability source that VERIFIES one target's recovery capability
/// (R04's registry replaces the resolution; the default stays
/// conservative). `read_only` tools re-execute safely;
/// `ledger.double`-style targets honor their idempotency key.
pub fn capability_source_for(target: &str) -> Arc<dyn RecoveryCapabilitySource> {
    struct Verified(String);
    impl RecoveryCapabilitySource for Verified {
        fn capability_of(&self, target: &str) -> ToolRecoveryCapability {
            let mut cap = ToolRecoveryCapability::CONSERVATIVE;
            if target == self.0 {
                if self.0 == "grep" {
                    cap.read_only = true;
                } else {
                    cap.honors_idempotency_key = true;
                }
            }
            cap
        }
    }
    Arc::new(Verified(target.to_string()))
}

/// Boots the restarted process with an explicit recovery capability source.
pub async fn boot_with_caps(
    home: &std::path::Path,
    caps: Arc<dyn RecoveryCapabilitySource>,
) -> ServiceState {
    let layout = lingxi_service::prepare_layout(home).expect("restart layout");
    ServiceState::bootstrap_with_deps(
        config_for(home),
        &layout,
        ServiceDeps {
            recovery_capabilities: caps,
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("bootstrap restart")
}

/// Bounded wait (1ms poll; the T02–T06 deterministic-wait precedent).
pub async fn wait_until(max_ms: u64, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(max_ms);
    while !cond() {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    true
}

/// Writes one machine-readable evidence key into the R03-T07 evidence JSON
/// when `R03_T07_EVIDENCE` points at a file (same convention as T05's
/// `R03_T05_EVIDENCE`).
pub fn write_evidence(key: &str, value: serde_json::Value) {
    if let Ok(path) = std::env::var("R03_T07_EVIDENCE") {
        let path = std::path::PathBuf::from(path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut root: serde_json::Map<String, serde_json::Value> = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        root.insert(key.to_string(), value);
        if let Ok(bytes) = serde_json::to_vec_pretty(&root) {
            let _ = std::fs::write(&path, bytes);
        }
    }
}
