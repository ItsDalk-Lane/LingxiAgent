//! R05-T07 persistence acceptance: the `model_call_usage` ledger's schema
//! version, migration and write contract (the C09 persistence leg + C10's
//! idempotency/conflict rules).
//!
//! - `migration_v4_to_v5`: an ISOLATED COPY of a version-4 database (built
//!   by executing the REAL V1–V4 migration SQL, the receipts included)
//!   upgrades in place to version 5, keeps every legacy row and gains the
//!   ledger table;
//! - `old_rows_survive_a_new_model_config`: a model-plane RELOAD (the
//!   config file rewritten with different providers, the gateway swapped)
//!   never rewrites or drops existing ledger rows — accounting is
//!   append-only and the new rows carry the new route;
//! - `identical_replay_and_conflict`: re-recording the identical row is an
//!   idempotent no-op; a DIFFERENT row under the same model call id is a
//!   loud Conflict (a call's accounting is never rewritten).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lingxi_kernel::ports::StoragePort as _;
use lingxi_kernel::usage::{ModelCallUsageRecord, ModelUsageQuery, UsageProvenance};
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};

// ── shared bits ──────────────────────────────────────────────────────────────

fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r05t07p-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn sample_record(model_call_id: &str, provider: &str) -> ModelCallUsageRecord {
    ModelCallUsageRecord {
        session_id: Some("sess_local_alpha".to_string()),
        run_id: Some("run-t07p-1".to_string()),
        attempt: Some("run-t07p-1#a1".to_string()),
        model_call_id: model_call_id.to_string(),
        purpose: "chat".to_string(),
        origin: "user".to_string(),
        parent_run_id: None,
        cause_ref: None,
        parent_tool_call_id: None,
        provider: provider.to_string(),
        model: "stub-model".to_string(),
        protocol: "openai-completions".to_string(),
        usage: Some(lingxi_kernel::usage::ModelCallUsage {
            input_tokens: Some(10),
            output_tokens: Some(4),
            cache_read_tokens: Some(2),
            cache_write_tokens: None,
            reasoning_tokens: None,
            provenance: UsageProvenance::Reported,
        }),
        invalid_detail: None,
        transport_attempts: Some(1),
        outcome: lingxi_kernel::usage::CallOutcome::Succeeded,
        started_at_unix_ms: Some(1_790_409_600_000),
        settled_at_unix_ms: Some(1_790_409_600_500),
        emitted_tool_calls: Vec::new(),
        cost_basis: None,
    }
}

// ── migration: an isolated v4 copy upgrades and keeps its rows ───────────────

#[tokio::test]
async fn migration_v4_to_v5_upgrades_an_isolated_copy_and_keeps_old_rows() {
    let dir = unique_dir("mig");
    let db_path = dir.join("runs.db");

    // Build a REAL version-4 database: the compiled-in V1–V4 SQL, the
    // schema_migrations receipts (same fingerprints the adapter writes)
    // and PRAGMA user_version — plus legacy rows the upgrade must keep.
    {
        use lingxi_adapters::storage::migrations::{fingerprint_sql, MIGRATIONS};
        use rusqlite::Connection;
        let conn = Connection::open(&db_path).expect("create v4 db");
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, \
             fingerprint TEXT NOT NULL, applied_at_unix_ms INTEGER NOT NULL, \
             applied_by TEXT NOT NULL);",
        )
        .expect("receipts table");
        let now = 1_790_409_600_000_i64;
        for migration in MIGRATIONS.iter().take_while(|m| m.version <= 4) {
            conn.execute(
                "INSERT INTO schema_migrations (version, name, fingerprint, \
                 applied_at_unix_ms, applied_by) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    migration.version as i64,
                    migration.name,
                    fingerprint_sql(migration.sql),
                    now,
                    "lingxi-test-seeder"
                ],
            )
            .expect("receipt");
            conn.execute_batch(migration.sql)
                .expect("apply v4 migration");
        }
        conn.pragma_update(None, "user_version", 4)
            .expect("user_version 4");
        // Legacy rows: a session, a run, an attempt and a key event (the
        // pre-T07 facts the upgrade must preserve untouched).
        conn.execute(
            "INSERT INTO sessions (session_id, agent_id, owner_user_id, title, \
             created_at_unix_ms) VALUES ('sess_legacy', 'lingxi', 'user_legacy', 'Legacy', 1)",
            [],
        )
        .expect("legacy session");
        conn.execute(
            "INSERT INTO runs (run_id, session_id, owner_kind, owner_subject, principal_id, \
             attempt_count, status, generation, created_at_unix_ms, updated_at_unix_ms, \
             last_event_seq) VALUES ('run_legacy', 'sess_legacy', 'local', 'owner', 'p', 1, \
             'completed', 1, 1, 1, 1)",
            [],
        )
        .expect("legacy run");
        conn.execute(
            "INSERT INTO run_attempts (run_id, attempt, generation, started_at_unix_ms) \
             VALUES ('run_legacy', 'run_legacy#a1', 1, 1)",
            [],
        )
        .expect("legacy attempt");
        conn.execute(
            "INSERT INTO key_events (event_id, stream_id, seq, session_id, run_id, attempt, \
             event_type, payload_json, committed_at_unix_ms) VALUES ('run_legacy-start', \
             'sess_legacy', 1, 'sess_legacy', 'run_legacy', 'run_legacy#a1', \
             'run_state_changed', '{}', 1)",
            [],
        )
        .expect("legacy event");
    }

    // Open with the CURRENT build: the v5 migration runs in place.
    let db = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("v4 db opens and migrates");
    {
        use rusqlite::Connection;
        let conn = Connection::open(&db_path).expect("reopen read");
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("user_version");
        assert_eq!(
            version as u64,
            lingxi_adapters::storage::migrations::supported_version()
        );
        let ledger: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = \
                 'model_call_usage'",
                [],
                |row| row.get(0),
            )
            .expect("ledger table probe");
        assert_eq!(ledger, 1, "the v5 ledger table exists after migration");
    }

    // The legacy rows are intact (old data survives the new model plane's
    // schema).
    let session = db.get_session("sess_legacy").await.expect("get session");
    assert!(session.is_some(), "the legacy session survived");
    let run = db
        .load_run(&lingxi_protocol::RunId::new("run_legacy"))
        .await;
    assert!(run.expect("load").is_some(), "the legacy run survived");

    // And the new table is live: a row records + reads back (bound to the
    // LEGACY session so the owner-scoped join resolves).
    let mut migrated_row = sample_record("run_legacy-mc0001", "legacy-provider");
    migrated_row.session_id = Some("sess_legacy".to_string());
    migrated_row.run_id = Some("run_legacy".to_string());
    migrated_row.attempt = Some("run_legacy#a1".to_string());
    db.record_model_call_usage(migrated_row, 2)
        .await
        .expect("ledger write on the migrated db");
    let rows = db
        .query_model_call_usage(ModelUsageQuery {
            owner_user_id: Some("user_legacy".to_string()),
            session_id: Some("sess_legacy".to_string()),
            run_id: None,
            ..ModelUsageQuery::default()
        })
        .await
        .expect("ledger query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider, "legacy-provider");

    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(dir);
}

/// R05 RR1 F38: the v7 migration makes `transport_attempts` NULLABLE by
/// rebuilding the ledger table. A REAL v6 database (one pre-F38 usage row
/// carrying an observed count) upgrades in place: every row keeps its
/// value verbatim, and the post-v7 schema accepts (and round-trips) the
/// NEW honest state — attempts unknown (NULL) for a call dropped before
/// settlement.
#[tokio::test]
async fn migration_v6_to_v7_makes_attempts_nullable_and_keeps_rows() {
    let dir = unique_dir("mig-v7");
    let db_path = dir.join("runs.db");
    {
        use lingxi_adapters::storage::migrations::{fingerprint_sql, MIGRATIONS};
        use rusqlite::Connection;
        let conn = Connection::open(&db_path).expect("create v6 db");
        conn.execute_batch(
            "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL, \
             fingerprint TEXT NOT NULL, applied_at_unix_ms INTEGER NOT NULL, \
             applied_by TEXT NOT NULL);",
        )
        .expect("receipts table");
        let now = 1_790_409_600_000_i64;
        for migration in MIGRATIONS.iter().take_while(|m| m.version <= 6) {
            conn.execute(
                "INSERT INTO schema_migrations (version, name, fingerprint, \
                 applied_at_unix_ms, applied_by) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    migration.version as i64,
                    migration.name,
                    fingerprint_sql(migration.sql),
                    now,
                    "lingxi-test-seeder"
                ],
            )
            .expect("receipt");
            conn.execute_batch(migration.sql)
                .expect("apply v6 migration");
        }
        conn.pragma_update(None, "user_version", 6)
            .expect("user_version 6");
        // A pre-F38 row: the count was observed (2 = a 401-refresh resend).
        conn.execute(
            "INSERT INTO model_call_usage (model_call_id, session_id, run_id, attempt, purpose, \
             origin, parent_run_id, cause_ref, provider, model, protocol, usage_state, \
             input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, \
             reasoning_tokens, missing_fields, estimate_basis, invalid_detail, \
             transport_attempts, cost_basis, recorded_at_unix_ms, outcome, \
             started_at_unix_ms, settled_at_unix_ms, parent_tool_call_id, emitted_tool_calls) \
             VALUES ('run-v6-mc0001', NULL, NULL, NULL, 'chat', 'operation', NULL, NULL, \
             'legacy-main', 'legacy-model', 'openai-completions', 'reported', 10, 4, NULL, \
             NULL, NULL, NULL, NULL, NULL, 2, NULL, 1, 'succeeded', 1, 2, NULL, NULL)",
            [],
        )
        .expect("v6 usage row");
    }

    let db = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("v6 db opens and migrates to v7");
    // The pre-F38 row survived the rebuild verbatim.
    let rows = db
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .expect("query after v7");
    assert_eq!(rows.len(), 1, "the v6 row survived the v7 rebuild");
    assert_eq!(rows[0].model_call_id, "run-v6-mc0001");
    assert_eq!(rows[0].transport_attempts, Some(2));
    assert_eq!(
        rows[0].outcome,
        lingxi_kernel::usage::CallOutcome::Succeeded
    );

    // The post-v7 schema accepts the NEW honest state: attempts unknown
    // (NULL) — the cancelled-in-flight row shape (F38), round-tripped.
    let mut unknown_attempts = sample_record("run-v7-cancelled-mc0001", "legacy-main");
    unknown_attempts.transport_attempts = None;
    unknown_attempts.outcome = lingxi_kernel::usage::CallOutcome::Cancelled;
    db.record_model_call_usage(unknown_attempts.clone(), 3)
        .await
        .expect("NULL attempts write on the migrated db");
    let reread = db
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .expect("reread");
    let cancelled = reread
        .iter()
        .find(|row| row.model_call_id == "run-v7-cancelled-mc0001")
        .expect("the cancelled row rereads");
    assert_eq!(cancelled.transport_attempts, None);
    assert_eq!(
        cancelled.outcome,
        lingxi_kernel::usage::CallOutcome::Cancelled
    );

    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(dir);
}

// ── the ledger's own write contract ──────────────────────────────────────────

#[tokio::test]
async fn identical_replay_is_idempotent_and_a_rewrite_is_a_loud_conflict() {
    let dir = unique_dir("idem");
    let db_path = dir.join("runs.db");
    let db = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("open");
    // The ledger writes rows for sessions that may not exist yet (worker
    // callbacks / operations) — no FK by design; a chat-shaped row needs
    // no run row either.
    let record = sample_record("run-x-mc0001", "main");
    db.record_model_call_usage(record.clone(), 1)
        .await
        .expect("first write");
    db.record_model_call_usage(record.clone(), 2)
        .await
        .expect("identical replay is a no-op");
    let rows = db
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .expect("query");
    assert_eq!(rows.len(), 1, "the replay added nothing");

    // A DIFFERENT fact under the same call id is refused loudly.
    let mut rewritten = record.clone();
    rewritten.usage.as_mut().unwrap().input_tokens = Some(999);
    let conflict = db
        .record_model_call_usage(rewritten, 3)
        .await
        .expect_err("a call's accounting is never rewritten");
    assert!(
        matches!(
            conflict,
            lingxi_kernel::ports::StorageError::Conflict { .. }
        ),
        "the rewrite is a Conflict: {conflict:?}"
    );

    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(dir);
}

// ── old data is never overwritten by a new model config ──────────────────────

struct Boot {
    state: ServiceState,
    home: PathBuf,
    config_path: PathBuf,
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

async fn boot_with_plane(tag: &str, plane_json: &str) -> Boot {
    let home = unique_dir(&format!("{tag}-home"));
    boot_on_home(&home, tag, plane_json).await
}

/// Boots the REAL composition root over an EXISTING home (the restart
/// path: same data root, new config file content).
async fn boot_on_home(home: &Path, tag: &str, plane_json: &str) -> Boot {
    let workspace = unique_dir(&format!("{tag}-ws"));
    let config_root = unique_dir(&format!("{tag}-cfg"));
    let config_path = config_root.join("service.json");
    let config_json = format!(
        r#"{{"home": {}, "workspace": {}, {plane_json}}}"#,
        serde_json::to_string(&home.to_string_lossy()).expect("home json"),
        serde_json::to_string(&workspace.to_string_lossy()).expect("ws json"),
    );
    std::fs::write(&config_path, &config_json).expect("write config");
    let layout = prepare_layout(home).expect("layout");
    let file = lingxi_service::config::read_service_config(&config_path).expect("service config");
    let (source, plane) =
        lingxi_service::config::resolve_model_plane(Some(&config_path), &layout.runtime_dir)
            .expect("plane resolves")
            .expect("plane present");
    let credential_service = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(
            &plane,
            &layout.runtime_dir,
            Arc::new(lingxi_service::inject::SystemClock),
        )
        .expect("credential service"),
    );
    let deps = ServiceDeps {
        model_gateway: Some(Arc::new(
            lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane),
        )),
        model_plane_source: Some(source),
        workspace_root: file.workspace,
        credential_service: Some(credential_service),
        ..ServiceDeps::default()
    };
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    Boot {
        state,
        home: home.to_path_buf(),
        config_path,
    }
}

async fn execute(state: &ServiceState, session: &str, input: &str) -> String {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_for(
            storage.as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            session,
            input,
            1_790_409_600_000,
        )
        .await
        .expect("execute accepted")
        .run_id
}

/// A minimal streaming final over the stub wire (usage included).
struct StubServer {
    addr: SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl StubServer {
    async fn start(answers: usize, input: u64, output: u64) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("addr");
        let answers = Arc::new(std::sync::atomic::AtomicUsize::new(answers));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                let answers = Arc::clone(&answers);
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt as _;
                    let _read = read_http_head(&mut socket).await;
                    if answers.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                        let _ = socket
                            .write_all(
                                b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: \
                                 application/json\r\nContent-Length: 2\r\nConnection: \
                                 close\r\n\r\n{}",
                            )
                            .await;
                        let _ = socket.shutdown().await;
                        return;
                    }
                    let mut body = String::new();
                    for frame in [
                        serde_json::json!({
                            "id": "chatcmpl-stub", "model": "stub-model",
                            "choices": [{"index": 0, "finish_reason": null,
                                "delta": {"role": "assistant", "content": "answer"}}]
                        }),
                        serde_json::json!({
                            "id": "chatcmpl-stub",
                            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
                        }),
                        serde_json::json!({
                            "id": "chatcmpl-stub", "choices": [],
                            "usage": {"prompt_tokens": input, "completion_tokens": output}
                        }),
                    ] {
                        body.push_str(&format!("data: {frame}\n\n"));
                    }
                    body.push_str("data: [DONE]\n\n");
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Self {
            addr,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.task).await;
    }
}

async fn read_http_head(socket: &mut tokio::net::TcpStream) -> String {
    use tokio::io::AsyncReadExt as _;
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), socket.read(&mut chunk))
                .await
                .expect("stub read")
                .expect("stub read");
        raw.extend_from_slice(&chunk[..read]);
        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&raw).into_owned()
}

#[tokio::test]
async fn old_ledger_rows_survive_a_model_config_reload() {
    // The OLD plane (provider `old-main`) answers one run; the ledger
    // holds its rows. (The stub carries TWO answers: the old run's and the
    // post-restart new run's.)
    let stub = StubServer::start(2, 10, 4).await;
    let plane_old = format!(
        r#""providers": {{
            "old-main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "sk-t07p-old"}}
            }}
        }},
        "models": {{"chat": {{"provider": "old-main", "model": "old-model", "capabilities": {{"tools": true}}}}}}"#,
        endpoint = serde_json::to_string(&stub.endpoint()).expect("json"),
    );
    let boot = boot_with_plane("reload-old", &plane_old).await;
    let old_run = execute(&boot.state, "sess_local_alpha", "old task").await;
    let old_rows = boot
        .state
        .storage()
        .query_model_call_usage(ModelUsageQuery {
            owner_user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
            session_id: Some("sess_local_alpha".to_string()),
            run_id: None,
            ..ModelUsageQuery::default()
        })
        .await
        .expect("old rows");
    assert_eq!(old_rows.len(), 1);
    assert_eq!(old_rows[0].provider, "old-main");

    // The NEW model config (different provider identity entirely) is
    // written to the config file and the service is RESTARTED over the
    // same home — the strongest form of "reload" (same in-memory swap is
    // covered by T01-C05; this additionally covers the durable path).
    drop(boot.state.storage().close().await);
    let plane_new = format!(
        r#""providers": {{
            "new-main": {{
                "protocol": "openai-completions",
                "endpoint": {endpoint},
                "auth": {{"kind": "apiKey", "apiKey": "sk-t07p-new"}}
            }}
        }},
        "models": {{"chat": {{"provider": "new-main", "model": "new-model", "capabilities": {{"tools": true}}}}}}"#,
        endpoint = serde_json::to_string(&stub.endpoint()).expect("json"),
    );
    let home = boot.home.clone();
    let config_path = boot.config_path.clone();
    let home_json = serde_json::to_string(&home.to_string_lossy()).expect("home json");
    let workspace_json = serde_json::to_string(&home.to_string_lossy()).expect("ws json");
    std::fs::write(
        &config_path,
        format!("{{\"home\": {home_json}, \"workspace\": {workspace_json}, {plane_new}}}"),
    )
    .expect("rewrite config");
    let boot_new = boot_on_home(&home, "reload-new", &plane_new).await;

    // The OLD rows are untouched — the new model config neither rewrites
    // nor drops accounting history.
    let rows_after = boot_new
        .state
        .storage()
        .query_model_call_usage(ModelUsageQuery {
            owner_user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
            session_id: Some("sess_local_alpha".to_string()),
            run_id: None,
            ..ModelUsageQuery::default()
        })
        .await
        .expect("rows after reload");
    assert_eq!(rows_after.len(), 1, "the old row survived the new config");
    assert_eq!(rows_after[0].provider, "old-main");
    assert_eq!(rows_after[0].run_id.as_deref(), Some(old_run.as_str()));

    // A NEW run under the new plane appends new-provider rows.
    let new_run = execute(&boot_new.state, "sess_local_alpha", "new task").await;
    let rows = boot_new
        .state
        .storage()
        .query_model_call_usage(ModelUsageQuery {
            owner_user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
            session_id: Some("sess_local_alpha".to_string()),
            run_id: None,
            ..ModelUsageQuery::default()
        })
        .await
        .expect("all rows");
    assert_eq!(rows.len(), 2, "append-only: old + new");
    let new_row = rows
        .iter()
        .find(|r| r.run_id.as_deref() == Some(new_run.as_str()))
        .expect("new row");
    assert_eq!(new_row.provider, "new-main");
    assert_eq!(new_row.model, "new-model");

    boot_new.state.storage().close().await.expect("close");
    stub.stop().await;
    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(config_path.parent().expect("config root"));
}

#[tokio::test]
async fn the_ledger_round_trips_partial_and_invalid_states() {
    let dir = unique_dir("roundtrip");
    let db_path = dir.join("runs.db");
    let db = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("open");

    // Partial: the missing half survives by NAME.
    let mut partial = sample_record("run-p-mc0001", "main");
    partial.usage = Some(lingxi_kernel::usage::ModelCallUsage {
        input_tokens: Some(7),
        output_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        reasoning_tokens: None,
        provenance: UsageProvenance::Partial {
            missing: vec!["output_tokens"],
        },
    });
    db.record_model_call_usage(partial.clone(), 1)
        .await
        .expect("partial write");

    // Invalid: no usage numbers, the violation detail survives.
    let mut invalid = sample_record("run-i-mc0001", "main");
    invalid.usage = None;
    invalid.invalid_detail = Some("usage field /usage/prompt_tokens is negative (-3)".to_string());
    db.record_model_call_usage(invalid.clone(), 1)
        .await
        .expect("invalid write");

    // Unknown: no usage at all.
    let mut unknown = sample_record("run-u-mc0001", "main");
    unknown.usage = None;
    db.record_model_call_usage(unknown.clone(), 1)
        .await
        .expect("unknown write");

    // Estimated (REVIEW-T07 F-02): a host-derived fact survives with its
    // BASIS intact — never rewritten into a provider `reported` number.
    let mut estimated = sample_record("run-e-mc0001", "main");
    estimated.usage = Some(lingxi_kernel::usage::ModelCallUsage {
        input_tokens: Some(11),
        output_tokens: Some(4),
        cache_read_tokens: None,
        cache_write_tokens: None,
        reasoning_tokens: None,
        provenance: UsageProvenance::Estimated {
            basis: "chars/4".to_string(),
        },
    });
    db.record_model_call_usage(estimated.clone(), 1)
        .await
        .expect("estimated write");

    let rows = db
        .query_model_call_usage(ModelUsageQuery::default())
        .await
        .expect("query");
    assert_eq!(rows.len(), 4);
    let mut sorted_rows = rows;
    sorted_rows.sort_by(|a, b| a.model_call_id.cmp(&b.model_call_id));
    let mut expected = vec![partial, invalid, unknown, estimated];
    expected.sort_by(|a, b| a.model_call_id.cmp(&b.model_call_id));
    assert_eq!(
        sorted_rows, expected,
        "all four states round-trip losslessly (partial / invalid / unknown / estimated)"
    );

    db.close().await.expect("close");
    let _ = std::fs::remove_dir_all(dir);
}
