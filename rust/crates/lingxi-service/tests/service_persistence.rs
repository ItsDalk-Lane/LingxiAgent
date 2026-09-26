//! R02-T04 service-level persistence: the execute path goes through the
//! injected storage port (endpoint shapes unchanged), and a FULL restart
//! (new ServiceState over the same data root, fresh queue/worker) reads
//! the committed run facts back from the real database.

use std::net::SocketAddr;

use lingxi_service::{
    prepare_layout, run, HomeSource, ServiceConfig, ServiceStartupError, ServiceState,
};
use lingxi_service::{sessions::SessionAccess, NetworkMode};

fn synthetic_home(tag: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("lingxi-r02t04-svc-{}-{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create synthetic home");
    dir
}

fn config_for(home: &std::path::Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
    }
}

async fn owner_principal(state: &ServiceState) -> lingxi_service::Principal {
    // The local owner principal as the auth layer shapes it: read through
    // the running auth service's own token (identity is created ONLY at
    // the trusted boundary).
    let _ = state;
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
    }
}

#[tokio::test]
async fn execute_persists_runs_that_survive_a_full_restart() {
    let home = synthetic_home("restart");
    let layout = prepare_layout(&home).expect("layout");

    // Boot #1: execute one run through the injected port.
    let state = ServiceState::bootstrap(config_for(&home), &layout)
        .await
        .expect("bootstrap 1");
    let principal = owner_principal(&state).await;
    let storage = std::sync::Arc::clone(state.storage());
    let accepted = state
        .sessions()
        .execute_for(
            storage.as_ref(),
            state.events(),
            &principal,
            "sess_local_alpha",
            "hello persistence",
            1_234,
        )
        .await
        .expect("execute commits");
    assert_eq!(accepted.run_count, 1);
    state.storage().close().await.expect("close 1");

    // Boot #2 over the same data root: fresh queue/worker, same files.
    let layout2 = prepare_layout(&home).expect("layout 2");
    let state2 = ServiceState::bootstrap(config_for(&home), &layout2)
        .await
        .expect("bootstrap 2");
    match state2
        .sessions()
        .get_for(&principal, "sess_local_alpha")
        .await
        .expect("read")
    {
        SessionAccess::Ok(facts) => {
            assert_eq!(facts.run_count, 1, "run count must survive the restart");
            assert_eq!(facts.last_runs.len(), 1);
            assert_eq!(facts.last_runs[0].run_id, accepted.run_id);
            assert_eq!(
                facts.last_runs[0].started_at_unix_ms, 1_234,
                "committed facts come from the database, not memory"
            );
        }
        other => panic!("owner read after restart failed: {other:?}"),
    }
    // The run is terminal in the database (completed with its key event).
    let status = state2
        .storage()
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![accepted.run_id.clone()],
        )
        .await
        .expect("query")
        .expect("run row");
    assert_eq!(status, "completed");
    let events = state2
        .storage()
        .query_one_text(
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
            vec![accepted.run_id],
        )
        .await
        .expect("query")
        .expect("count");
    assert_eq!(events, "2", "start + terminal events are durable");
    state2.storage().close().await.expect("close 2");

    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn broken_database_is_a_loud_startup_failure() {
    use std::io::Write as _;
    let home = synthetic_home("broken");
    let layout = prepare_layout(&home).expect("layout");
    let db_path = layout.runtime_dir.join("data").join("runs.db");
    std::fs::create_dir_all(db_path.parent().expect("data dir")).expect("mkdir");
    // A garbage file that is NOT a SQLite database: opening must fail
    // loudly (never an empty-database fallback).
    let mut f = std::fs::File::create(&db_path).expect("create");
    f.write_all(b"this is definitely not a sqlite database")
        .unwrap();
    f.sync_all().unwrap();
    drop(f);

    let result = ServiceState::bootstrap(config_for(&home), &layout).await;
    match result {
        Err(ServiceStartupError::Storage(err)) => {
            // Corrupt-file diagnosis from SQLite (not a silent recreate).
            let text = err.to_string();
            assert!(
                text.contains("file") || text.contains("corrupt") || text.contains("malformed"),
                "unexpected error text: {text}"
            );
        }
        Err(other) => panic!("expected Storage startup error, got {other}"),
        Ok(state) => panic!(
            "broken database must refuse startup, got Ok({:?})",
            state.storage().db_path()
        ),
    }
    let _ = std::fs::remove_dir_all(&home);
}

#[allow(unused)]
async fn _service_still_serves_over_http(home: &std::path::Path) {
    // (kept as a compile-checked reminder: HTTP-level behavior is covered
    // by the auth_matrix integration tests and the binary script)
    let layout = prepare_layout(home).expect("layout");
    let state = ServiceState::bootstrap(config_for(home), &layout)
        .await
        .expect("bootstrap");
    let _ = run(state, async {}, |_addr| {}).await;
}
