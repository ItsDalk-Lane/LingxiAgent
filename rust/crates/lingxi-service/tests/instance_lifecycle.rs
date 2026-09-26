//! R02-T02 integration harness: the real serving loop combined with the
//! real single-writer lock and instance-record lifecycle, on a synthetic
//! isolated home under the system temp dir.
//!
//! Test layer (taskbook 01 §5): 契约/服务集成 — real axum service, real
//! loopback TCP, real OS advisory lock, real atomic record writes. The only
//! test-controlled part is the shutdown trigger (OS-signal shutdown and
//! REAL two-process rejection are exercised by the acceptance scripts
//! `scripts/rust-tauri/r02_t02_dual_instance.sh` /
//! `r02_t02_path_priority.sh`, which drive real binaries).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_service::instance::InstanceLockError;
use lingxi_service::{
    acquire, prepare_layout, run, HomeSource, InstanceGuard, InstanceRecord, ServiceConfig,
    ServiceError,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn synthetic_home(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lingxi-service-r02t02-it-{}-{tag}",
        std::process::id()
    ))
}

async fn http_get(addr: SocketAddr, path: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read");
    String::from_utf8_lossy(&raw).into_owned()
}

/// A running in-process instance mirroring the binary's startup chain:
/// lock acquired, record published at readiness, shutdown over a oneshot.
struct TestInstance {
    addr: SocketAddr,
    home: PathBuf,
    record: PathBuf,
    stale_archive: PathBuf,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<(), ServiceError>>,
}

struct Started {
    instance: TestInstance,
    guard: InstanceGuard,
    stale_taken_over: Option<InstanceRecord>,
}

async fn start_on(tag: &str, wipe: bool) -> Started {
    let home = synthetic_home(tag);
    if wipe {
        let _ = std::fs::remove_dir_all(&home);
    }
    let layout = prepare_layout(&home).expect("prepare layout");
    let (guard, stale) = acquire(&layout).expect("acquire single-writer lock");
    if wipe {
        assert!(stale.is_none(), "fresh home must have no stale record");
    }

    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static addr parses"),
        data_home: layout.home.clone(),
        home_source: HomeSource::Cli,
    };
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let record_path = layout.record_path.clone();
    let publish_guard = Arc::new(Mutex::new(guard));
    let ready_guard = Arc::clone(&publish_guard);
    let handle = tokio::spawn(async move {
        run(
            config,
            async {
                let _ = stop_rx.await;
            },
            move |addr| {
                let mut guard = ready_guard
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.publish(addr).expect("publish instance record");
                let _ = ready_tx.send(addr);
            },
        )
        .await
    });
    let addr = ready_rx.await.expect("readiness");
    // The publish closure is finished; take the guard back for the caller.
    let guard = match Arc::try_unwrap(publish_guard) {
        Ok(mutex) => mutex
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        Err(_) => unreachable!("test bug: publish closure still holds the guard"),
    };
    Started {
        instance: TestInstance {
            addr,
            home,
            record: record_path,
            stale_archive: layout.stale_archive_path.clone(),
            stop: stop_tx,
            handle,
        },
        guard,
        stale_taken_over: stale,
    }
}

impl TestInstance {
    /// Graceful stop: shutdown, join, cleanup own record, unlock.
    async fn stop_and_cleanup(self, mut guard: InstanceGuard) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("shutdown within timeout")
            .expect("join")
            .expect("clean serve result");
        guard.release().expect("cleanup own record + unlock");
        assert!(
            !self.record.exists(),
            "graceful shutdown must remove the OWN record"
        );
        let lock = self.home.join("lingxi-service").join("instance.lock");
        assert!(lock.exists(), "lock file itself is kept (never deleted)");
    }

    /// Crash simulation: stop serving and drop the lock fd WITHOUT record
    /// cleanup, exactly like SIGKILL would leave the home behind.
    async fn crash(self, _guard: InstanceGuard) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("task ends")
            .expect("join")
            .expect("serve ok until the crash point");
        // _guard dropped here: OS releases the lock; record stays.
        assert!(self.record.exists(), "crash must leave the record behind");
    }
}

#[tokio::test]
async fn locked_instance_survives_rejected_second_claim() {
    let started = start_on("dual", true).await;
    let first = started.instance;
    let guard = started.guard;

    // Sanity: record published with this instance's identity + address.
    let record: InstanceRecord = serde_json::from_str(
        &std::fs::read_to_string(&first.record).expect("record exists after readiness"),
    )
    .expect("record is valid JSON");
    assert_eq!(record.instance_id, guard.identity().instance_id);
    assert_eq!(
        record.bind_addr.as_deref(),
        Some(first.addr.to_string().as_str())
    );

    // Second claim on the SAME home must be rejected with full diagnostics.
    // acquire() may run the (blocking, std-only) diagnostic probe against
    // the live peer, so it runs on the blocking pool — never on a worker
    // thread the peer's server task might be waiting to run on.
    let layout = prepare_layout(&first.home).expect("layout of existing home");
    let err = tokio::task::spawn_blocking(move || acquire(&layout))
        .await
        .expect("join blocking acquire")
        .expect_err("second claim must be rejected");
    match &err {
        InstanceLockError::HeldByPeer(diag) => {
            let record = diag.record.as_ref().expect("peer record must be readable");
            assert_eq!(record.instance_id, guard.identity().instance_id);
            // The peer IS live and serves health: the diagnostic probe must
            // confirm it (diagnostic-only enrichment, never the authority).
            assert_eq!(diag.probe, Some(lingxi_service::PeerProbe::Live));
        }
        other => panic!("expected HeldByPeer, got {other:?}"),
    }

    // First instance keeps serving normally across the rejected attempt.
    let body = http_get(first.addr, "/lingxi/v1/health").await;
    assert!(
        body.starts_with("HTTP/1.1 200 "),
        "health after rejection: {body}"
    );
    // Data untouched: record byte-identical before/after the rejection.
    let after: InstanceRecord =
        serde_json::from_str(&std::fs::read_to_string(&first.record).expect("record still there"))
            .expect("record still valid");
    assert_eq!(
        after, record,
        "rejected second instance must not touch data"
    );

    first.stop_and_cleanup(guard).await;
    let _ = std::fs::remove_dir_all(synthetic_home("dual"));
}

#[tokio::test]
async fn crashed_owner_leaves_stale_record_and_restart_takes_over() {
    let started = start_on("crash", true).await;
    let record_before = std::fs::read_to_string(&started.instance.record).expect("record exists");
    started.instance.crash(started.guard).await;

    // Restart on the SAME home (no wipe): must take over the stale record.
    let restart = start_on("crash", false).await;
    let stale = restart
        .stale_taken_over
        .expect("restart must detect the stale record");
    let before: InstanceRecord = serde_json::from_str(&record_before).unwrap();
    assert_eq!(stale.instance_id, before.instance_id);
    // Archived verbatim.
    let archived: InstanceRecord = serde_json::from_str(
        &std::fs::read_to_string(&restart.instance.stale_archive)
            .expect("stale archive exists after takeover"),
    )
    .expect("archive is valid");
    assert_eq!(archived, before);

    // The new owner serves and its record replaced the stale one.
    let body = http_get(restart.instance.addr, "/lingxi/v1/health").await;
    assert!(body.starts_with("HTTP/1.1 200 "), "restart serves: {body}");
    let current: InstanceRecord = serde_json::from_str(
        &std::fs::read_to_string(&restart.instance.record).expect("new record"),
    )
    .unwrap();
    assert_eq!(current.instance_id, restart.guard.identity().instance_id);
    assert_ne!(current.instance_id, before.instance_id);

    restart.instance.stop_and_cleanup(restart.guard).await;
    let _ = std::fs::remove_dir_all(synthetic_home("crash"));
}

#[tokio::test]
async fn config_source_field_reaches_the_service_config() {
    // The readiness line's `source=` field is built from HomeSource; the
    // library-level resolution feeding it is asserted here.
    let cli = lingxi_service::parse_cli(vec!["--home".to_string(), "/tmp/x".to_string()]).unwrap();
    let cfg = ServiceConfig::from_sources(&cli, None, &std::env::temp_dir()).unwrap();
    assert_eq!(cfg.home_source, HomeSource::Cli);

    let cli = lingxi_service::parse_cli(Vec::<String>::new()).unwrap();
    let cfg = ServiceConfig::from_sources(&cli, Some("/tmp/y"), &std::env::temp_dir()).unwrap();
    assert_eq!(cfg.home_source, HomeSource::Env);
}
