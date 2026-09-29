//! R02 stage-repair R1 / F01 regression (stage-level review finding):
//! concurrent executes with DISTINCT inputs at the SAME millisecond must
//! each commit their own run — never collapse into one shared run id with
//! multiple success receipts. Covered on both levels that collapsed
//! pre-fix: the in-process service composition (fixed clock) and the real
//! HTTP transport (real loopback TCP, real router, real SQLite).
//!
//! Also covered: allocator reseed across a restart (same home, same fixed
//! millisecond — ids must still be unique), and the durable fact counts
//! (one run row + two key events per accepted execute).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::inject::{ManualClock, SequentialRequestIdGen};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState, StoreOptions, SubscribeOutcome,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

const FIXED_NOW_MS: u64 = 1_790_409_600_000;

fn synthetic_home(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lingxi-r02repair-f01-{tag}-{}", std::process::id()))
}

fn cleanup(path: &std::path::Path) {
    if path.exists() {
        let _ = std::fs::remove_dir_all(path);
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

fn test_config(home: PathBuf) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home,
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

/// 64 concurrent distinct-input executes through the in-process composition
/// return 64 distinct, newly committed runs.
async fn execute_64_concurrent(state: &ServiceState, tag: &str) -> Vec<String> {
    let owner = owner_principal();
    let mut tasks = Vec::new();
    for n in 0..64u32 {
        let state = state.clone();
        let owner = owner.clone();
        let tag = tag.to_string();
        tasks.push(tokio::spawn(async move {
            state
                .sessions()
                .execute_for(
                    state.storage().as_ref(),
                    state.events(),
                    state.runs(),
                    &owner,
                    "sess_local_alpha",
                    &format!("{tag}-distinct-input-{n}"),
                    FIXED_NOW_MS,
                )
                .await
                .expect("execute accepted")
                .run_id
        }));
    }
    let mut ids = Vec::new();
    for t in tasks {
        ids.push(t.await.unwrap());
    }
    ids
}

#[tokio::test]
async fn in_process_concurrent_executes_mint_distinct_runs_and_reseed_on_restart() {
    let home = synthetic_home("inproc");
    cleanup(&home);
    let layout = prepare_layout(&home).expect("prepare layout");
    let state = ServiceState::bootstrap_with_deps(
        test_config(home.clone()),
        &layout,
        ServiceDeps::default(),
    )
    .await
    .expect("bootstrap");

    let ids_first = execute_64_concurrent(&state, "first").await;
    let unique: std::collections::HashSet<_> = ids_first.iter().collect();
    assert_eq!(
        unique.len(),
        64,
        "64 distinct submissions => 64 distinct run ids, got {} ids: {:?}",
        unique.len(),
        ids_first
    );
    assert_eq!(
        state
            .sessions()
            .run_count("sess_local_alpha")
            .await
            .unwrap(),
        64,
        "every accepted execute commits its own durable run"
    );
    let (runs, key_events, _) = state.storage().run_fact_summary().await.unwrap();
    assert_eq!(runs, 64, "one run row per accepted execute");
    assert_eq!(
        key_events, 128,
        "two key events per run (start + terminal outcome)"
    );

    // Restart against the SAME home: the allocator reseeds from durable
    // state, so another 64 executes at the SAME fixed millisecond still
    // mint ids never used before.
    state.storage().close().await.expect("close storage");
    drop(state);
    let layout = prepare_layout(&home).expect("prepare layout");
    let state = ServiceState::bootstrap_with_deps(
        test_config(home.clone()),
        &layout,
        ServiceDeps::default(),
    )
    .await
    .expect("bootstrap after restart");

    let ids_second = execute_64_concurrent(&state, "second").await;
    let unique_second: std::collections::HashSet<_> = ids_second.iter().collect();
    assert_eq!(unique_second.len(), 64, "post-restart ids are distinct");
    let first_set: std::collections::HashSet<_> = ids_first.iter().collect();
    let overlap: Vec<_> = unique_second.intersection(&first_set).collect();
    assert!(
        overlap.is_empty(),
        "no run id may repeat across a restart at the same millisecond: {overlap:?}"
    );
    assert_eq!(
        state
            .sessions()
            .run_count("sess_local_alpha")
            .await
            .unwrap(),
        128
    );
    state.storage().close().await.expect("close storage");
    cleanup(&home);
}

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

impl TestServer {
    async fn stop_and_assert_clean(self) {
        self.stop.send(()).expect("server task still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
    }

    fn local_token(&self) -> String {
        let raw =
            std::fs::read_to_string(self.home.join("lingxi-service").join("local-token.json"))
                .expect("local token file exists");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("token json");
        value["token"].as_str().expect("token string").to_string()
    }
}

async fn http_post_execute(addr: SocketAddr, token: &str, input: &str) -> (u16, String) {
    let body = serde_json::json!({ "input": input }).to_string();
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let request = format!(
        "POST /lingxi/v1/sessions/sess_local_alpha/execute HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Connection: close\r\n\
         Authorization: Bearer {token}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read response");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no status in head: {head}"));
    (status, body.to_string())
}

#[tokio::test]
async fn http_concurrent_executes_at_one_fixed_millisecond_mint_distinct_runs() {
    // The F01 kill condition through the REAL transport: the injected clock
    // never advances, so every request shares now_ms — pre-fix the racy
    // count read collapsed these into a handful of shared run ids.
    let home = synthetic_home("http");
    cleanup(&home);
    let clock = Arc::new(ManualClock::new(FIXED_NOW_MS));
    let deps = ServiceDeps {
        clock: clock.clone(),
        request_ids: Arc::new(SequentialRequestIdGen::new()),
        ..ServiceDeps::default()
    };
    let config = test_config(home.clone());
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let layout = prepare_layout(&config.data_home).expect("prepare layout");
    let state = ServiceState::bootstrap_with_deps(config.clone(), &layout, deps)
        .await
        .expect("bootstrap with deps");
    let test_view = state.clone();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None, // no drain budget: the test drives the stop signal itself
        )
        .await
    });
    let addr = ready_rx.await.expect("service reports readiness");
    let server = TestServer {
        addr,
        home: home.clone(),
        stop: stop_tx,
        handle,
    };
    let token = server.local_token();

    let mut tasks = Vec::new();
    for n in 0..64u32 {
        let token = token.clone();
        let addr = server.addr;
        tasks.push(tokio::spawn(async move {
            http_post_execute(addr, &token, &format!("http-distinct-input-{n}")).await
        }));
    }
    let mut ids = std::collections::HashSet::new();
    for t in tasks {
        let (status, body) = t.await.unwrap();
        assert_eq!(status, 200, "execute accepted: {body}");
        let json: serde_json::Value =
            serde_json::from_str(&body).unwrap_or_else(|_| panic!("body not json: {body:?}"));
        ids.insert(json["runId"].as_str().expect("runId").to_string());
    }
    assert_eq!(
        ids.len(),
        64,
        "64 concurrent HTTP executes => 64 distinct run ids, got {}",
        ids.len()
    );
    assert_eq!(
        test_view
            .sessions()
            .run_count("sess_local_alpha")
            .await
            .unwrap(),
        64
    );
    let (runs, key_events, _) = test_view.storage().run_fact_summary().await.unwrap();
    assert_eq!((runs, key_events), (64, 128));

    server.stop_and_assert_clean().await;
    cleanup(&home);
}

#[tokio::test]
async fn http_running_storage_fault_has_no_success_event_or_restart_run() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!(
        "lingxi-r02-a07-live-fault-{}-{unique}",
        std::process::id()
    ));
    assert!(!home.exists());
    let config = test_config(home.clone());
    let layout = prepare_layout(&home).expect("fresh synthetic home");
    let state = ServiceState::bootstrap_with_deps(
        config.clone(),
        &layout,
        ServiceDeps {
            store_options: StoreOptions {
                busy_timeout_ms: 0,
                ..StoreOptions::default()
            },
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("service bootstraps before the injected runtime fault");
    let test_view = state.clone();
    let (cut, subscription) = match state
        .events()
        .subscribe(&owner_principal(), "sess_local_alpha", None)
        .await
        .expect("subscribe before storage fault")
    {
        SubscribeOutcome::Started { cut, subscription } => (cut, subscription),
        SubscribeOutcome::RequiresSnapshot(_) => panic!("fresh stream needs no snapshot"),
    };
    assert!(cut.events.is_empty());
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let server = TestServer {
        addr: ready_rx.await.expect("service ready before fault"),
        home: home.clone(),
        stop: stop_tx,
        handle,
    };
    let db_path = test_view.storage().db_path().to_path_buf();
    let external = rusqlite::Connection::open(&db_path).expect("external SQLite fault handle");
    external
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold the database write lock while service is running");
    let (status, body) =
        http_post_execute(server.addr, &server.local_token(), "must-not-succeed").await;
    assert_eq!(
        status, 503,
        "locked live database must return backpressure: {body}"
    );
    assert!(
        body.contains("db_busy"),
        "machine reason must explain runtime storage failure: {body}"
    );
    assert!(
        subscription.mailbox().try_recv().is_none(),
        "failed commit must publish no live success event"
    );
    external
        .execute_batch("ROLLBACK")
        .expect("release injected write lock");
    let (running_runs, running_events, _) = test_view.storage().run_fact_summary().await.unwrap();
    assert_eq!((running_runs, running_events), (0, 0));
    assert!(subscription.mailbox().try_recv().is_none());

    // 第二种故障发生在开始事件已提交之后、终态同事务写入时。
    external
        .execute_batch(
            "CREATE TRIGGER r02_fail_terminal BEFORE UPDATE OF status ON runs \
             WHEN NEW.status = 'completed' BEGIN SELECT RAISE(ABORT, 'injected terminal write failure'); END;",
        )
        .expect("inject a durable terminal-write failure into the live database");
    let (terminal_status, terminal_body) = http_post_execute(
        server.addr,
        &server.local_token(),
        "terminal-must-not-succeed",
    )
    .await;
    assert_eq!(
        terminal_status, 500,
        "terminal write failure must not return success: {terminal_body}"
    );
    assert!(
        terminal_body.contains("db_failure"),
        "machine reason must identify storage failure: {terminal_body}"
    );
    assert!(
        matches!(
            subscription.mailbox().try_recv(),
            Some(lingxi_service::SubscriptionFrame::Event(_))
        ),
        "the committed start may be published"
    );
    assert!(
        subscription.mailbox().try_recv().is_none(),
        "rolled-back terminal event must not be published"
    );
    external
        .execute_batch("DROP TRIGGER r02_fail_terminal")
        .expect("remove only the synthetic trigger");
    let (partial_runs, partial_events, _) = test_view.storage().run_fact_summary().await.unwrap();
    assert_eq!((partial_runs, partial_events), (1, 1));
    let (run_id, run_status): (String, String) = external
        .query_row("SELECT run_id, status FROM runs", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(
        run_status, "running",
        "a failed terminal transaction cannot become completed"
    );
    let persisted_start: String = external
        .query_row(
            "SELECT payload_json FROM key_events WHERE run_id = ?1",
            [&run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(persisted_start.contains("running") && !persisted_start.contains("completed"));
    drop(external);
    drop(subscription);
    server.stop_and_assert_clean().await;

    let disk = rusqlite::Connection::open(&db_path).expect("read actual database after stop");
    let disk_runs: i64 = disk
        .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0))
        .unwrap();
    let disk_events: i64 = disk
        .query_row("SELECT COUNT(*) FROM key_events", [], |row| row.get(0))
        .unwrap();
    let disk_status: String = disk
        .query_row(
            "SELECT status FROM runs WHERE run_id = ?1",
            [&run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!((disk_runs, disk_events), (1, 1));
    assert_eq!(disk_status, "running");
    drop(disk);
    drop(test_view);
    let restarted_layout = prepare_layout(&home).unwrap();
    let restarted =
        ServiceState::bootstrap_with_deps(config, &restarted_layout, ServiceDeps::default())
            .await
            .expect("restart reads the same database after runtime fault");
    let (restart_runs, restart_events, _) = restarted.storage().run_fact_summary().await.unwrap();
    assert_eq!((restart_runs, restart_events), (1, 1));
    let restarted_run = restarted
        .sessions()
        .get_for(&owner_principal(), "sess_local_alpha")
        .await
        .unwrap();
    let lingxi_service::SessionAccess::Ok(facts) = restarted_run else {
        panic!("owner session missing after restart")
    };
    assert_eq!(facts.last_runs.len(), 1);
    assert_eq!(facts.last_runs[0].run_id, run_id);
    println!("R02_A07_LIVE_FAULT busyHttp={status} busyReason=db_busy busyLiveEvents=0 busyRuns={running_runs} busyKeyEvents={running_events} terminalHttp={terminal_status} terminalReason=db_failure terminalLiveEvents=1 terminalRunId={run_id} terminalStoredStatus={run_status} diskRuns={disk_runs} diskKeyEvents={disk_events} diskStatus={disk_status} restartRuns={restart_runs} restartKeyEvents={restart_events}");
    if let Ok(evidence) = std::env::var("R02_A07_LIVE_FAULT_EVIDENCE") {
        let path = PathBuf::from(evidence);
        std::fs::create_dir_all(path.parent().expect("evidence file has a parent")).unwrap();
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema": "lingxi.r02-a07-live-fault.v1",
                "busy": {"http": status, "reason": "db_busy", "liveEvents": 0,
                    "runs": running_runs, "keyEvents": running_events},
                "terminal": {"http": terminal_status, "reason": "db_failure",
                    "liveEvents": 1, "runId": run_id, "storedStatus": run_status},
                "disk": {"runs": disk_runs, "keyEvents": disk_events, "status": disk_status},
                "restart": {"runs": restart_runs, "keyEvents": restart_events,
                    "runId": facts.last_runs[0].run_id},
            }))
            .unwrap(),
        )
        .unwrap();
    }
    restarted.storage().close().await.unwrap();
    drop(restarted);
    std::fs::remove_dir_all(&home).expect("remove only the synthetic home created by this test");
}
