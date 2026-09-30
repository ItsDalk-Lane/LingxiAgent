//! REVIEWER-REPAIR-R03-G04-R1 independent probe (ran ONLY inside the
//! disposable /tmp git worktree; preserved here as review evidence).
//!
//! Orchestration target: the executor-disclosed gap — a start-write
//! failure INSIDE the dispatched background task (no port seam: the drive
//! uses Arc<RunDatabase> directly). This probe injects it through the
//! REAL library with NO test port: an external rusqlite connection (held
//! open by a dedicated thread) takes the database write lock (WAL), so
//! the detached task's `record_run_started` hits SQLITE_BUSY after the
//! worker busy_timeout (~5s) and fails BEFORE any durable fact.
//!
//! Results (2026-09-30, ~/.cargo/bin/cargo 1.98.1, --locked):
//! - candidate (198e0da1e + 7 modified files): PASSED in 5.43s — the
//!   background task's failure compensation retracts the reservation
//!   (load_run -> None through the real store), the same-key retry
//!   re-admits FRESH (new run id, not replayed), exactly ONE real
//!   execution, post-settle retry replays the real run.
//!   (logs/probe-bg-internal-start-failure-candidate.log, exit 0)
//! - pre-fix (198e0da1e sources restored): FAILED at
//!   `assert!(!retry.replayed, "the retry must be a fresh admission")`
//!   in 5.38s — the retry returned a GHOST replay of the failed run
//!   (replayed=true, no durable row): the exact disclosed defect.
//!   (logs/probe-bg-internal-start-failure-prefix.log, exit 101)

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::ports::{
    CommittedOutcome, InvocationIntent, InvocationPhase, InvocationReceipt, KeyEvent,
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, RunOutcome, StaleResultFact,
    StorageError, StoragePort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError, RunId, ToolCallId,
};

use lingxi_service::{
    prepare_layout, ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig, ServiceDeps,
    ServiceState,
};

const NOW_MS: u64 = 1_790_409_600_000;

struct ScriptedProvider {
    scripts: std::sync::Mutex<HashMap<String, VecDeque<ProviderTurn>>>,
}

impl ScriptedProvider {
    fn new(scripts: Vec<(&'static str, Vec<ProviderTurn>)>) -> Arc<Self> {
        Arc::new(Self {
            scripts: std::sync::Mutex::new(
                scripts
                    .into_iter()
                    .map(|(m, s)| (m.to_string(), s.into_iter().collect()))
                    .collect(),
            ),
        })
    }
}

impl lingxi_kernel::ports::TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.g04probe".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        _turn: u32,
        input: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let marker = input.split(':').next().unwrap_or(input).trim().to_string();
        let ctx_at_issue = ctx.clone();
        let turn = self
            .scripts
            .lock()
            .unwrap()
            .get_mut(&marker)
            .and_then(|q| q.pop_front())
            .unwrap_or_else(|| ProviderTurn::Failed {
                error: ProtocolError::new(ErrorCode::UpstreamUnavailable, "exhausted", false),
                retryable: false,
            });
        Box::pin(async move { ProviderTurnResult::of_ctx(&ctx_at_issue, turn) })
    }
}

fn final_turn() -> ProviderTurn {
    ProviderTurn::Final {
        message: NormalizedMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::Text {
                text: "probe done".to_string(),
            }],
            model_call_id: None,
        },
    }
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
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

async fn submit_bg(
    state: &ServiceState,
    session: &'static str,
    input: &str,
    request_id: &str,
) -> Result<lingxi_service::ExecuteAccepted, lingxi_service::SessionExecuteError> {
    let submission = ExecuteSubmission {
        input,
        request_id: Some(request_id),
    };
    let storage = Arc::clone(state.storage());
    let events = Arc::clone(state.events());
    let runs = Arc::clone(state.runs());
    let background = Arc::clone(state.background());
    state
        .sessions()
        .execute_background_for(
            &storage,
            &events,
            &runs,
            &background,
            &owner_principal(),
            session,
            &submission,
            NOW_MS,
        )
        .await
}

async fn query_text(state: &ServiceState, sql: &str, arg: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("query")
}

async fn session_runs(state: &ServiceState, session: &str) -> usize {
    query_text(
        state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        session,
    )
    .await
    .and_then(|v| v.parse().ok())
    .unwrap_or(0)
}

#[tokio::test]
async fn g04r1_probe_background_internal_start_failure_is_compensated() {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-g04r1-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("addr"),
        data_home: dir.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let layout = prepare_layout(&dir).expect("layout");
    let provider = ScriptedProvider::new(vec![("PROBE", vec![final_turn(), final_turn()])]);
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");

    // The external write lock on the REAL database (WAL): held OPEN by a
    // dedicated thread (the connection must SURVIVE across awaits, or the
    // transaction rolls back and the lock releases).
    let db_path: PathBuf = state.storage().db_path().to_path_buf();
    let (locked_tx, locked_rx) = std::sync::mpsc::channel::<()>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let lock_thread = std::thread::spawn(move || {
        let conn = rusqlite::Connection::open(&db_path).expect("external open");
        conn.busy_timeout(std::time::Duration::from_millis(2_000))
            .expect("external busy timeout");
        conn.execute_batch("BEGIN EXCLUSIVE;").expect("exclusive");
        locked_tx.send(()).expect("lock signal");
        // Hold the write lock until the test says to let go.
        let _ = release_rx.recv();
        let _ = conn.execute_batch("COMMIT;");
    });
    locked_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the external exclusive lock landed");

    let first = submit_bg(&state, "sess_local_alpha", "PROBE: x", "probe-k")
        .await
        .expect("dispatch accepted (the failure happens inside the task)");

    // Wait for the drive to fail and its compensation to run (the busy
    // timeout on the worker is 5s).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !state.background().live_ids().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the blocked background drive never settled"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    // The run row must NOT exist (the start write never committed).
    let status = query_text(
        &state,
        "SELECT status FROM runs WHERE run_id = ?1",
        &first.run_id,
    )
    .await;
    assert!(
        status.is_none(),
        "the start write was blocked; no durable row may exist (got {status:?})"
    );

    // Release the external write lock.
    drop(release_tx);
    lock_thread.join().expect("lock thread exits");

    // Same-key retry: the reservation must have been compensated away —
    // a FRESH real admission, never a ghost replay.
    let retry = submit_bg(&state, "sess_local_alpha", "PROBE: x", "probe-k")
        .await
        .expect("retry after the compensated internal start failure");
    assert!(!retry.replayed, "the retry must be a fresh admission");
    assert_ne!(retry.run_id, first.run_id);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while query_text(&state, "SELECT status FROM runs WHERE run_id = ?1", &retry.run_id).await
        .as_deref()
        != Some("completed")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the retried background run never settled"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        session_runs(&state, "sess_local_alpha").await,
        1,
        "exactly ONE real execution"
    );

    // Post-settle same-key retry replays the real run.
    let replay = submit_bg(&state, "sess_local_alpha", "PROBE: x", "probe-k")
        .await
        .expect("post-settle retry replays");
    assert!(replay.replayed);
    assert_eq!(replay.run_id, retry.run_id);
    assert_eq!(session_runs(&state, "sess_local_alpha").await, 1);

    state.storage().close().await.expect("close");
    let _ = std::fs::remove_dir_all(&dir);
}
