//! 31CDA283A11A：实际HTTP、SQLite、JSONL和sidecar，失败与重启必须保全文件。
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
struct Harness {
    home: PathBuf,
    state: ServiceState,
    addr: SocketAddr,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}
#[derive(Clone)]
struct Client {
    addr: SocketAddr,
    token: String,
}
impl Client {
    async fn request(&self, method: &str, path: &str, body: Value) -> (u16, Value) {
        let text = body.to_string();
        let mut socket = TcpStream::connect(self.addr).await.unwrap();
        let wire=format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",self.token,text.len());
        socket.write_all(wire.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        socket.read_to_end(&mut bytes).await.unwrap();
        let reply = String::from_utf8(bytes).unwrap();
        let Some((header, body)) = reply.split_once("\r\n\r\n") else {
            return (0, json!({"connectionClosed":true}));
        };
        let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        let value = serde_json::from_str(body).unwrap_or_else(|_| json!({"raw":body}));
        (status, value)
    }
}
fn unique(tag: &str) -> PathBuf {
    PathBuf::from(std::env::var("LINGXI_HOME").expect("合成HOME")).join(format!(
        "restore-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
async fn boot(home: PathBuf) -> Harness {
    let layout = prepare_layout(&home).unwrap();
    let home = home.canonicalize().unwrap();
    let state = ServiceState::bootstrap_with_deps(
        ServiceConfig {
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: 5000,
        },
        &layout,
        ServiceDeps::default(),
    )
    .await
    .unwrap();
    let token = state.auth().local_token().to_string();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let served = state.clone();
    let task = tokio::spawn(async move {
        lingxi_service::run(
            served,
            async move {
                let _ = rx.await;
            },
            move |addr| {
                let _ = ready_tx.send(addr);
            },
            None,
        )
        .await
    });
    let addr = ready_rx.await.unwrap();
    Harness {
        home,
        state,
        addr,
        token,
        stop: tx,
        task,
    }
}
impl Harness {
    fn client(&self) -> Client {
        Client {
            addr: self.addr,
            token: self.token.clone(),
        }
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.client().request("POST", path, body).await
    }
    async fn archived(&self, id: &str, agent: &str) -> PathBuf {
        let (code, result) = self
            .post(
                "/lingxi/v1/sessions",
                json!({"sessionId":id,"agentId":agent,"title":"归档恢复"}),
            )
            .await;
        assert_eq!(code, 201, "{result}");
        let (code, result) = self
            .post("/lingxi/v1/sessions/archive", json!({"sessionId":id}))
            .await;
        assert_eq!(code, 200, "{result}");
        let dir = self
            .home
            .join("agents")
            .join(agent)
            .join("sessions/archived");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("2026-10-14_{id}.jsonl"));
        std::fs::write(&file, jsonl(id)).unwrap();
        std::fs::write(sidecar(&file), sidecar_bytes(id, &file)).unwrap();
        file
    }
    async fn life(&self, id: &str) -> Option<String> {
        self.state
            .storage()
            .query_one_text(
                "SELECT lifecycle FROM sessions WHERE session_id=?1",
                vec![id.into()],
            )
            .await
            .unwrap()
    }
    async fn pending(&self, id: &str) -> String {
        self.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM session_operations WHERE session_id=?1 AND state='prepared' AND file_effects=1",vec![id.into()]).await.unwrap().unwrap()
    }
    async fn stop(self) -> PathBuf {
        let home = self.home.clone();
        let _ = self.stop.send(());
        assert!(self.task.await.unwrap().is_ok());
        home
    }
}
fn jsonl(id: &str) -> String {
    format!(
        "{}\n{}\n",
        json!({"type":"session","id":id,"version":3,"timestamp":"2026-10-14T00:00:00Z"}),
        json!({"type":"message","id":"legacy-msg","message":{"role":"user","content":[{"type":"text","text":"旧会话须完整保留"}]}})
    )
}
fn sidecar(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.files.json", path.display()))
}
fn active(path: &Path) -> PathBuf {
    path.parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(path.file_name().unwrap())
}
fn sidecar_bytes(id: &str, path: &Path) -> String {
    json!({"version":1,"sessionId":id,"sessionPath":path,"files":{},"refs":[],"unknown":"保留未知字段"}).to_string()
}
fn assert_archived_intact(src: &Path, before: &str, side: &str) {
    assert_eq!(std::fs::read_to_string(src).unwrap(), before);
    assert_eq!(std::fs::read_to_string(sidecar(src)).unwrap(), side);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn physical_restore_sidecar_then_restart_archive_delete_and_db_only_remain_real() {
    let h = boot(unique("happy")).await;
    let src = h.archived("happy", "lingxi").await;
    let dest = active(&src);
    let bytes = std::fs::read(&src).unwrap();
    let attachments = std::fs::read(sidecar(&src)).unwrap();
    let (code, result) = h
        .post("/lingxi/v1/sessions/restore", json!({"sessionPath":src}))
        .await;
    assert_eq!(code, 200, "{result}");
    assert_eq!(result["restoredPath"], dest.to_string_lossy().as_ref());
    assert_eq!(result["sessionId"], "happy");
    assert!(!src.exists() && !sidecar(&src).exists());
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert_eq!(std::fs::read(sidecar(&dest)).unwrap(), attachments);
    assert_eq!(h.life("happy").await.as_deref(), Some("active"));
    assert_eq!(h.pending("happy").await, "0");
    let home = h.stop().await;
    let h = boot(home).await;
    let (code, result) = h
        .post("/lingxi/v1/sessions/archive", json!({"sessionId":"happy"}))
        .await;
    assert_eq!(code, 200, "{result}");
    assert!(src.exists() && sidecar(&src).exists());
    assert!(!dest.exists() && !sidecar(&dest).exists());
    let missing_backup = src.with_extension("source-saved");
    std::fs::rename(&src, &missing_backup).unwrap();
    assert_eq!(
        h.post("/lingxi/v1/sessions/restore", json!({"sessionId":"happy"}))
            .await
            .0,
        404,
        "可信locator有缺源时不能变成DB-only成功"
    );
    std::fs::rename(missing_backup, &src).unwrap();
    let (code, result) = h
        .post(
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"happy","sessionPath":src}),
        )
        .await;
    assert_eq!(code, 200, "{result}");
    assert_eq!(result["restoredPath"], dest.to_string_lossy().as_ref());
    let (code, result) = h
        .post("/lingxi/v1/sessions/archive", json!({"sessionId":"happy"}))
        .await;
    assert_eq!(code, 200, "{result}");
    let (code, result) = h
        .post(
            "/lingxi/v1/sessions/archived/delete",
            json!({"sessionId":"happy"}),
        )
        .await;
    assert_eq!(code, 200, "{result}");
    assert!(h.life("happy").await.is_none());
    assert!(!src.exists() && !sidecar(&src).exists());
    assert!(!dest.exists() && !sidecar(&dest).exists());
    let (code, result) = h
        .post(
            "/lingxi/v1/sessions",
            json!({"sessionId":"db-only","agentId":"plain","title":"纯数据库"}),
        )
        .await;
    assert_eq!(code, 201, "{result}");
    assert_eq!(
        h.post(
            "/lingxi/v1/sessions/archive",
            json!({"sessionId":"db-only"})
        )
        .await
        .0,
        200
    );
    let (code, result) = h
        .post(
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"db-only"}),
        )
        .await;
    assert_eq!(code, 200, "{result}");
    assert!(result.get("restoredPath").is_none());
    assert!(!h.home.join("agents/plain/sessions").exists());
    h.stop().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn header_only_old_bug_is_repaired_but_message_and_standalone_sidecar_conflicts_preserve() {
    let h = boot(unique("conflicts")).await;
    for kind in [
        "messages",
        "sidecar",
        "header",
        "orphan-header",
        "foreign-header",
    ] {
        let src = h.archived(kind, "lingxi").await;
        let dest = active(&src);
        let source = std::fs::read_to_string(&src).unwrap();
        let attached = std::fs::read_to_string(sidecar(&src)).unwrap();
        if kind == "messages" {
            std::fs::write(&dest, jsonl(kind)).unwrap();
        } else if kind == "sidecar" {
            std::fs::write(sidecar(&dest), "existing-sidecar").unwrap();
        } else {
            std::fs::write(&dest, format!("{}\n", json!({"type":"session","id":match kind {"orphan-header"=>"old-bug-orphan","foreign-header"=>"messages",_=>kind}}))).unwrap();
            std::fs::write(sidecar(&dest), "old-header-bug-sidecar").unwrap();
        }
        let (code, result) = h
            .post("/lingxi/v1/sessions/restore", json!({"sessionId":kind}))
            .await;
        if kind == "header" || kind == "orphan-header" {
            assert_eq!(code, 200, "{result}");
            assert_eq!(std::fs::read_to_string(&dest).unwrap(), source);
            assert_eq!(std::fs::read_to_string(sidecar(&dest)).unwrap(), attached);
            assert!(!src.exists());
        } else {
            assert_eq!(code, 409, "{result}");
            assert_archived_intact(&src, &source, &attached);
            assert_eq!(h.life(kind).await.as_deref(), Some("archived"));
        }
    }
    h.stop().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn owner_path_agent_header_duplicate_missing_nonregular_and_link_guards_are_real() {
    let h = boot(unique("guards")).await;
    let src = h.archived("guards", "lingxi").await;
    let source = std::fs::read_to_string(&src).unwrap();
    let attached = std::fs::read_to_string(sidecar(&src)).unwrap();
    let (code, credential) = h
        .post(
            "/lingxi/v1/devices/credentials",
            json!({"userId":"other-owner","scopes":["chat"]}),
        )
        .await;
    assert_eq!(code, 201, "{credential}");
    let remote = Client {
        addr: h.addr,
        token: credential["secret"].as_str().unwrap().into(),
    };
    assert_eq!(
        remote
            .request(
                "POST",
                "/lingxi/v1/sessions/restore",
                json!({"sessionPath":src})
            )
            .await
            .0,
        403
    );
    for body in [
        json!({"sessionPath":h.home.join("outside.jsonl")}),
        json!({"sessionPath":h.home.join("agents/lingxi/sessions/archived/../fake.jsonl")}),
        json!({"sessionId":"wrong","sessionPath":src}),
        json!({"sessionId":"missing"}),
    ] {
        assert_ne!(h.post("/lingxi/v1/sessions/restore", body).await.0, 200);
    }
    let another = h.home.join("agents/another/sessions/archived");
    std::fs::create_dir_all(&another).unwrap();
    let fake = another.join("fake.jsonl");
    std::fs::write(&fake, jsonl("guards")).unwrap();
    assert_eq!(
        h.post("/lingxi/v1/sessions/restore", json!({"sessionPath":fake}))
            .await
            .0,
        403
    );
    let duplicate = src.parent().unwrap().join("duplicate.jsonl");
    std::fs::write(&duplicate, jsonl("guards")).unwrap();
    assert_eq!(
        h.post("/lingxi/v1/sessions/restore", json!({"sessionId":"guards"}))
            .await
            .0,
        409
    );
    std::fs::remove_file(duplicate).unwrap();
    #[cfg(unix)]
    {
        let link = src.with_file_name("link.jsonl");
        std::os::unix::fs::symlink(&src, &link).unwrap();
        assert_eq!(
            h.post("/lingxi/v1/sessions/restore", json!({"sessionPath":link}))
                .await
                .0,
            403
        );
        std::fs::remove_file(link).unwrap();
        let dest = active(&src);
        std::os::unix::fs::symlink(h.home.join("external-sentinel"), &dest).unwrap();
        assert_ne!(
            h.post("/lingxi/v1/sessions/restore", json!({"sessionId":"guards"}))
                .await
                .0,
            200
        );
        std::fs::remove_file(dest).unwrap();
    }
    let saved = src.with_extension("regular-source-saved");
    std::fs::rename(&src, &saved).unwrap();
    std::fs::create_dir(&src).unwrap();
    assert_eq!(
        h.post("/lingxi/v1/sessions/restore", json!({"sessionId":"guards"}))
            .await
            .0,
        409,
        "非regular来源拒绝"
    );
    std::fs::remove_dir(&src).unwrap();
    std::fs::rename(saved, &src).unwrap();
    #[cfg(unix)]
    {
        let archived = src.parent().unwrap();
        let saved = archived.with_file_name("archived-source-saved");
        std::fs::rename(archived, &saved).unwrap();
        std::os::unix::fs::symlink(&saved, archived).unwrap();
        assert_eq!(
            h.post("/lingxi/v1/sessions/restore", json!({"sessionId":"guards"}))
                .await
                .0,
            409,
            "目录祖先链接拒绝"
        );
        std::fs::remove_file(archived).unwrap();
        std::fs::rename(saved, archived).unwrap();
    }
    assert_archived_intact(&src, &source, &attached);
    assert_eq!(h.life("guards").await.as_deref(), Some("archived"));
    h.stop().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_sqlite_write_failure_after_prepared_plan_preserves_files_and_lifecycle() {
    let h = boot(unique("db-fault")).await;
    let src = h.archived("db-fault", "lingxi").await;
    let source = std::fs::read_to_string(&src).unwrap();
    let attached = std::fs::read_to_string(sidecar(&src)).unwrap();
    let conn = rusqlite::Connection::open(h.state.storage().db_path()).unwrap();
    conn.execute_batch("CREATE TRIGGER restore_refuse BEFORE UPDATE OF lifecycle ON sessions WHEN NEW.session_id='db-fault' AND NEW.lifecycle='active' BEGIN SELECT RAISE(ABORT,'ACTUAL_DB_WRITE_FAILURE'); END;").unwrap();
    let (code, result) = h
        .post(
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"db-fault"}),
        )
        .await;
    assert_eq!(code, 500, "{result}");
    assert_archived_intact(&src, &source, &attached);
    assert!(!active(&src).exists());
    assert_eq!(h.life("db-fault").await.as_deref(), Some("archived"));
    assert_eq!(h.pending("db-fault").await, "0");
    conn.execute_batch("DROP TRIGGER restore_refuse").unwrap();
    assert_eq!(
        h.post(
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"db-fault"})
        )
        .await
        .0,
        200
    );
    h.stop().await;
}

// 必须观察prepared已持久化后再取得外部写锁；探针超时证明实际最终写者等待。
async fn while_final_write_waits(kind: &str) {
    let h = boot(unique(kind)).await;
    let mut caught = false;
    for index in 0..12 {
        let case_id = format!("{kind}-{index}");
        let src = h.archived(&case_id, "lingxi").await;
        let dest = active(&src);
        let old = std::fs::read_to_string(&src).unwrap();
        let attached = std::fs::read_to_string(sidecar(&src)).unwrap();
        let id = case_id.clone();
        let db = h.state.storage().db_path().to_path_buf();
        let watcher = tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(db).unwrap();
            for _ in 0..100000 {
                let count:i64=conn.query_row("SELECT COUNT(*) FROM session_operations WHERE session_id=?1 AND operation_kind='legacy_session_file_transition' AND state='prepared'",[&id],|r|r.get(0)).unwrap();
                if count > 0 {
                    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
                    let phase: String = conn
                        .query_row(
                            "SELECT lifecycle FROM sessions WHERE session_id=?1",
                            [&id],
                            |r| r.get(0),
                        )
                        .unwrap();
                    return (conn, phase);
                }
                std::thread::yield_now();
            }
            panic!("未观察到真实prepared移动计划")
        });
        let client = h.client();
        let id = case_id.clone();
        let pending = tokio::spawn(async move {
            client
                .request(
                    "POST",
                    "/lingxi/v1/sessions/restore",
                    json!({"sessionId":id}),
                )
                .await
        });
        let (writer, phase) = watcher.await.unwrap();
        if phase != "archived" {
            writer.execute_batch("ROLLBACK").unwrap();
            drop(writer);
            assert_eq!(pending.await.unwrap().0, 200);
            continue;
        }
        assert!(
            src.exists() && sidecar(&src).exists() && !dest.exists(),
            "必须在真实两文件移动前取得写锁"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            h.state
                .storage()
                .query_one_text("SELECT 'actual-writer-waits'", vec![])
        )
        .await
        .is_err());
        match kind {
            "late-sidecar" => {
                std::fs::create_dir(sidecar(&dest)).unwrap();
                std::fs::write(sidecar(&dest).join("sentinel"), "外部目标不能被覆盖").unwrap();
            }
            "late-io" => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(
                        dest.parent().unwrap(),
                        std::fs::Permissions::from_mode(0o500),
                    )
                    .unwrap();
                }
            }
            "late-source" => {
                std::fs::write(&src, old.replace("旧会话须完整保留", "用户在等待期间修改")).unwrap()
            }
            "late-owner" => {
                writer
                    .execute(
                        "UPDATE sessions SET owner_user_id='changed-owner' WHERE session_id=?1",
                        [&case_id],
                    )
                    .unwrap();
            }
            _ => panic!("unknown probe"),
        }
        writer.execute_batch("COMMIT").unwrap();
        drop(writer);
        let (code, result) = pending.await.unwrap();
        eprintln!("ARCHIVE_REAL_FINAL_WRITE_WAIT kind={kind} status={code} result={result}");
        assert_ne!(code, 200, "变化后不能成功: {result}");
        if kind == "late-source" {
            assert!(std::fs::read_to_string(&src)
                .unwrap()
                .contains("用户在等待期间修改"));
        } else {
            assert_eq!(std::fs::read_to_string(&src).unwrap(), old);
        }
        assert_eq!(std::fs::read_to_string(sidecar(&src)).unwrap(), attached);
        assert!(!dest.exists());
        assert_eq!(h.life(&case_id).await.as_deref(), Some("archived"));
        if kind == "late-sidecar" {
            assert_eq!(
                std::fs::read_to_string(sidecar(&dest).join("sentinel")).unwrap(),
                "外部目标不能被覆盖"
            );
        }
        assert_eq!(
            h.pending(&case_id).await,
            "0",
            "本次未移动或已明确回滚时不能留下执行阻断"
        );
        caught = true;
        break;
    }
    assert!(caught, "未捕获prepared后/实际最终提交前窗口，不能计PASS");
    h.stop().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_second_file_rename_failure_rolls_back_jsonl_and_preserves_external_target() {
    while_final_write_waits("late-sidecar").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_source_change_during_final_sqlite_wait_is_refused_and_preserved() {
    while_final_write_waits("late-source").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_owner_change_during_final_sqlite_wait_cannot_authorize_stale_owner() {
    while_final_write_waits("late-owner").await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_permission_io_failure_during_final_write_rolls_back_everything() {
    while_final_write_waits("late-io").await;
}

#[cfg(unix)]
struct Binary {
    child: tokio::process::Child,
    client: Client,
}
#[cfg(unix)]
impl Binary {
    async fn start(home: &Path) -> Self {
        use tokio::io::{AsyncBufReadExt, BufReader};
        std::fs::create_dir_all(home).unwrap();
        let stderr = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(home.with_extension("binary-stderr.log"))
            .unwrap();
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_lingxi-service"))
            .args(["--home", home.to_str().unwrap(), "--bind", "127.0.0.1:0"])
            .env(
                "LINGXI_PRODUCT_DIR",
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../lib"),
            )
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(stderr)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let addr = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let mut line = String::new();
                assert!(
                    stdout.read_line(&mut line).await.unwrap() > 0,
                    "实际二进制启动失败，见binary-stderr.log"
                );
                if line.starts_with("LINGXI_SERVICE_READY ") {
                    break line
                        .split_whitespace()
                        .find_map(|s| s.strip_prefix("addr="))
                        .unwrap()
                        .parse::<SocketAddr>()
                        .unwrap();
                }
            }
        })
        .await
        .unwrap();
        tokio::spawn(async move {
            let mut sink = Vec::new();
            let _ = stdout.read_to_end(&mut sink).await;
        });
        let token: Value = serde_json::from_slice(
            &std::fs::read(home.join("lingxi-service/local-token.json")).unwrap(),
        )
        .unwrap();
        Self {
            child,
            client: Client {
                addr,
                token: token["token"].as_str().unwrap().into(),
            },
        }
    }
    async fn stop(mut self) {
        self.child.kill().await.unwrap();
        self.child.wait().await.unwrap();
    }
}
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_binary_kill_after_jsonl_move_before_sidecar_or_db_commit_recovers_on_restart() {
    let home = unique("actual-crash");
    let mut binary = Binary::start(&home).await;
    let home = home.canonicalize().unwrap();
    let (code, result) = binary
        .client
        .request(
            "POST",
            "/lingxi/v1/sessions",
            json!({"sessionId":"actual-crash","agentId":"lingxi","title":"实际中断"}),
        )
        .await;
    assert_eq!(code, 201, "{result}");
    assert_eq!(
        binary
            .client
            .request(
                "POST",
                "/lingxi/v1/sessions/archive",
                json!({"sessionId":"actual-crash"})
            )
            .await
            .0,
        200
    );
    let dir = home.join("agents/lingxi/sessions/archived");
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("actual-crash.jsonl");
    let dest = active(&src);
    std::fs::write(&src, jsonl("actual-crash")).unwrap();
    let mut attachments: Value =
        serde_json::from_str(&sidecar_bytes("actual-crash", &src)).unwrap();
    attachments["unknownPadding"] = Value::String("x".repeat(8 * 1024 * 1024));
    let attached = attachments.to_string();
    std::fs::write(sidecar(&src), &attached).unwrap();
    let pid = binary.child.id().unwrap();
    let watch_source = src.clone();
    let watch_dest = dest.clone();
    let db = home.join("lingxi-service/data/runs.db");
    let watcher = tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open(db).unwrap();
        for _ in 0..100000 {
            if watch_dest.exists()
                && !watch_source.exists()
                && sidecar(&watch_source).exists()
                && !sidecar(&watch_dest).exists()
            {
                let phase: String = conn
                    .query_row(
                        "SELECT lifecycle FROM sessions WHERE session_id='actual-crash'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                let prepared:i64=conn.query_row("SELECT COUNT(*) FROM session_operations WHERE session_id='actual-crash' AND operation_kind='legacy_session_file_transition' AND state='prepared'",[],|r|r.get(0)).unwrap();
                // SAFETY: 仅终止本场景启动的合成服务子进程，PID仍由父Child持有。
                assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGKILL) }, 0);
                return (phase, prepared);
            }
            std::thread::sleep(Duration::from_micros(20));
        }
        panic!("没有捕获真实两文件之间窗口，不能把模拟崩溃计为PASS")
    });
    let client = binary.client.clone();
    let pending = tokio::spawn(async move {
        client
            .request(
                "POST",
                "/lingxi/v1/sessions/restore",
                json!({"sessionId":"actual-crash"}),
            )
            .await
    });
    let (phase, prepared) = watcher.await.unwrap();
    assert_eq!(phase, "archived");
    assert_eq!(prepared, 1);
    let status = binary.child.wait().await.unwrap();
    assert!(!status.success());
    assert_eq!(pending.await.unwrap().0, 0, "实际中断的HTTP不能返回成功");
    assert!(dest.exists() && !src.exists());
    assert!(sidecar(&src).exists() && !sidecar(&dest).exists());
    eprintln!("ARCHIVE_ACTUAL_CRASH phase={phase} prepared={prepared} jsonlMoved=true sidecarMoved=false committed=false");
    let restarted = Binary::start(&home).await;
    assert!(!dest.exists() && !sidecar(&dest).exists());
    assert_eq!(
        std::fs::read_to_string(&src).unwrap(),
        jsonl("actual-crash")
    );
    assert_eq!(std::fs::read_to_string(sidecar(&src)).unwrap(), attached);
    let conn = rusqlite::Connection::open(home.join("lingxi-service/data/runs.db")).unwrap();
    let pending:i64=conn.query_row("SELECT COUNT(*) FROM session_operations WHERE session_id='actual-crash' AND state='prepared'",[],|r|r.get(0)).unwrap();
    assert_eq!(pending, 0);
    assert_eq!(
        restarted
            .client
            .request(
                "POST",
                "/lingxi/v1/sessions/restore",
                json!({"sessionId":"actual-crash"})
            )
            .await
            .0,
        200
    );
    assert_eq!(std::fs::read_to_string(sidecar(&dest)).unwrap(), attached);
    restarted.stop().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_db_locator_write_failure_after_both_renames_rolls_back_files_and_db() {
    let h = boot(unique("late-db-fault")).await;
    let src = h.archived("late-db-fault", "lingxi").await;
    let old = std::fs::read_to_string(&src).unwrap();
    let attached = std::fs::read_to_string(sidecar(&src)).unwrap();
    let conn = rusqlite::Connection::open(h.state.storage().db_path()).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_locator_commit BEFORE INSERT ON session_operations WHEN NEW.operation_kind='legacy_session_locator' BEGIN SELECT RAISE(ABORT,'ACTUAL_AFTER_BOTH_FILES_DB_FAILURE'); END;").unwrap();
    let (code, result) = h
        .post(
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"late-db-fault"}),
        )
        .await;
    assert_eq!(code, 500, "{result}");
    assert!(result
        .to_string()
        .contains("ACTUAL_AFTER_BOTH_FILES_DB_FAILURE"));
    assert_archived_intact(&src, &old, &attached);
    assert!(!active(&src).exists() && !sidecar(&active(&src)).exists());
    assert_eq!(h.life("late-db-fault").await.as_deref(), Some("archived"));
    assert_eq!(h.pending("late-db-fault").await, "0");
    conn.execute_batch("DROP TRIGGER fail_locator_commit")
        .unwrap();
    assert_eq!(
        h.post(
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"late-db-fault"})
        )
        .await
        .0,
        200
    );
    h.stop().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn device_owner_changed_after_auth_gate_before_prepared_plan_cannot_restore() {
    let h = boot(unique("before-prepare")).await;
    let src = h.archived("before-prepare", "lingxi").await;
    let old = std::fs::read_to_string(&src).unwrap();
    let attached = std::fs::read_to_string(sidecar(&src)).unwrap();
    let (code, credential) = h
        .post(
            "/lingxi/v1/devices/credentials",
            json!({"userId":lingxi_service::LOCAL_OWNER_USER_ID,"scopes":["chat"]}),
        )
        .await;
    assert_eq!(code, 201, "{credential}");
    let client = Client {
        addr: h.addr,
        token: credential["secret"].as_str().unwrap().into(),
    };
    let (read_code, read_result) = client
        .request("GET", "/lingxi/v1/sessions/before-prepare", json!({}))
        .await;
    assert_eq!(read_code, 200, "设备本来必须有真实访问权: {read_result}");
    let writer = rusqlite::Connection::open(h.state.storage().db_path()).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let pending = tokio::spawn(async move {
        client
            .request(
                "POST",
                "/lingxi/v1/sessions/restore",
                json!({"sessionId":"before-prepare"}),
            )
            .await
    });
    let mut waiting = false;
    for _ in 0..100 {
        if tokio::time::timeout(
            Duration::from_millis(20),
            h.state
                .storage()
                .query_one_text("SELECT 'waiting-before-prepare'", vec![]),
        )
        .await
        .is_err()
        {
            waiting = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        waiting,
        "必须观察设备认证结束后的真实prepared写者等待，不能以延迟猜测达点"
    );
    let count:i64=writer.query_row("SELECT COUNT(*) FROM session_operations WHERE session_id='before-prepare' AND operation_kind='legacy_session_file_transition'",[],|r|r.get(0)).unwrap();
    assert_eq!(count, 0, "这是prepared之前的真实归属窗口");
    writer.execute_batch("UPDATE sessions SET owner_user_id='new-owner' WHERE session_id='before-prepare'; COMMIT;").unwrap();
    drop(writer);
    let (code, result) = pending.await.unwrap();
    assert_eq!(code, 409, "旧设备主体不能沿旧归属保存: {result}");
    assert!(result
        .to_string()
        .contains("authorized restore owner/agent changed before preparation"));
    assert_archived_intact(&src, &old, &attached);
    assert!(!active(&src).exists());
    assert_eq!(h.life("before-prepare").await.as_deref(), Some("archived"));
    assert_eq!(h.pending("before-prepare").await, "0");
    h.stop().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn busy_and_two_actual_concurrent_restores_have_one_physical_commit() {
    let h = boot(unique("concurrency")).await;
    let src = h.archived("concurrency", "lingxi").await;
    let old = std::fs::read_to_string(&src).unwrap();
    let attached = std::fs::read_to_string(sidecar(&src)).unwrap();
    let permit = h
        .state
        .storage()
        .session_admission()
        .try_acquire(&["concurrency"], usize::MAX)
        .unwrap();
    let (code, result) = h
        .post(
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"concurrency"}),
        )
        .await;
    assert_eq!(code, 409, "{result}");
    assert!(result.to_string().contains("session_busy"));
    assert_archived_intact(&src, &old, &attached);
    assert_eq!(h.pending("concurrency").await, "0");
    drop(permit);
    let a = h.client();
    let b = h.client();
    let (one, two) = tokio::join!(
        a.request(
            "POST",
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"concurrency"})
        ),
        b.request(
            "POST",
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"concurrency"})
        )
    );
    let mut statuses = [one.0, two.0];
    statuses.sort();
    assert_eq!(statuses, [200, 409], "真实并发不能双成功: {one:?} {two:?}");
    let success = if one.0 == 200 { one.1 } else { two.1 };
    assert_eq!(
        success["restoredPath"],
        active(&src).to_string_lossy().as_ref()
    );
    assert_eq!(std::fs::read_to_string(active(&src)).unwrap(), old);
    assert_eq!(
        std::fs::read_to_string(sidecar(&active(&src))).unwrap(),
        attached
    );
    assert!(!src.exists() && !sidecar(&src).exists());
    assert_eq!(h.life("concurrency").await.as_deref(), Some("active"));
    let count=h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM session_operations WHERE session_id='concurrency' AND operation_kind='legacy_session_file_transition'",vec![]).await.unwrap().unwrap();
    assert_eq!(count, "1");
    assert_eq!(h.pending("concurrency").await, "0");
    h.stop().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_periodic_cleanup_deletes_legacy_pair_and_does_not_leave_suspended_files() {
    let h = boot(unique("cleanup-physical")).await;
    let src = h.archived("cleanup-physical", "lingxi").await;
    tokio::time::sleep(Duration::from_millis(10)).await;
    let (code, result) = h
        .post("/lingxi/v1/sessions/cleanup", json!({"maxAgeDays":0}))
        .await;
    assert_eq!(code, 200, "{result}");
    assert!(result["deleted"].as_u64().unwrap() >= 1);
    assert!(h.life("cleanup-physical").await.is_none());
    assert!(!src.exists() && !sidecar(&src).exists());
    assert!(!active(&src).exists() && !sidecar(&active(&src)).exists());
    assert_eq!(h.pending("cleanup-physical").await, "0");
    h.stop().await;
}
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_post_commit_cleanup_io_failure_reports_committed_state_and_restart_finishes() {
    use std::os::unix::fs::PermissionsExt;
    let h = boot(unique("cleanup-error")).await;
    let src = h.archived("cleanup-error", "lingxi").await;
    let dest = active(&src);
    let original = std::fs::read_to_string(&src).unwrap();
    let attached = std::fs::read_to_string(sidecar(&src)).unwrap();
    std::fs::write(
        &dest,
        format!("{}\n", json!({"type":"session","id":"cleanup-error"})),
    )
    .unwrap();
    std::fs::write(sidecar(&dest), "x".repeat(16 * 1024 * 1024)).unwrap();
    let db = h.state.storage().db_path().to_path_buf();
    let directory = dest.parent().unwrap().to_path_buf();
    let watcher = tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open(db).unwrap();
        for _ in 0..100000 {
            let text:Option<String>=conn.query_row("SELECT snapshot_json FROM session_operations WHERE session_id='cleanup-error' AND operation_kind='legacy_session_file_transition' AND state='prepared'",[],|r|r.get(0)).optional().unwrap();
            if let Some(text) = text {
                let plan: Value = serde_json::from_str(&text).unwrap();
                let header = PathBuf::from(plan["cleanup"][0][0].as_str().unwrap());
                let side = PathBuf::from(plan["cleanup"][1][0].as_str().unwrap());
                let phase: String = conn
                    .query_row(
                        "SELECT lifecycle FROM sessions WHERE session_id='cleanup-error'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                if phase == "active" && !header.exists() && side.exists() {
                    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500))
                        .unwrap();
                    return side;
                }
            }
            std::thread::sleep(Duration::from_micros(20));
        }
        panic!("未观察到真正已提交且已开始物理清扫的窗口，不能声称覆盖")
    });
    let client = h.client();
    let pending = tokio::spawn(async move {
        client
            .request(
                "POST",
                "/lingxi/v1/sessions/restore",
                json!({"sessionId":"cleanup-error"}),
            )
            .await
    });
    let remaining = watcher.await.unwrap();
    let (code, result) = pending.await.unwrap();
    std::fs::set_permissions(
        dest.parent().unwrap(),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    assert_eq!(code, 500, "真实清扫权限失败必须明确报错: {result}");
    assert!(result
        .to_string()
        .contains("legacy lifecycle commit succeeded"));
    assert!(result.to_string().contains("Permission denied"));
    assert_eq!(h.life("cleanup-error").await.as_deref(), Some("active"));
    assert!(!src.exists() && !sidecar(&src).exists());
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
    assert_eq!(std::fs::read_to_string(sidecar(&dest)).unwrap(), attached);
    assert!(remaining.exists());
    assert_eq!(h.pending("cleanup-error").await, "1");
    let home = h.stop().await;
    let h = boot(home).await;
    assert!(!remaining.exists());
    assert_eq!(h.pending("cleanup-error").await, "0");
    assert_eq!(h.life("cleanup-error").await.as_deref(), Some("active"));
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), original);
    h.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_different_sessions_concurrently_commit_their_own_pairs_and_journals() {
    let h = boot(unique("different-concurrent-success")).await;
    let mut originals = Vec::new();
    for id in ["different-a", "different-b"] {
        let src = h.archived(id, "lingxi").await;
        let mut body: Value = serde_json::from_str(&sidecar_bytes(id, &src)).unwrap();
        body["concurrentPadding"] = json!("x".repeat(1024 * 1024));
        let side = body.to_string();
        std::fs::write(sidecar(&src), &side).unwrap();
        originals.push((id, src, side));
    }
    let a = h.client();
    let b = h.client();
    let (ra, rb) = tokio::join!(
        a.request(
            "POST",
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"different-a"})
        ),
        b.request(
            "POST",
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"different-b"})
        )
    );
    assert_eq!(ra.0, 200, "{ra:?}");
    assert_eq!(rb.0, 200, "{rb:?}");
    for (id, src, attached) in originals {
        assert!(!src.exists() && !sidecar(&src).exists());
        assert_eq!(std::fs::read_to_string(active(&src)).unwrap(), jsonl(id));
        assert_eq!(
            std::fs::read_to_string(sidecar(&active(&src))).unwrap(),
            attached
        );
        assert_eq!(h.life(id).await.as_deref(), Some("active"));
        assert_eq!(h.pending(id).await, "0");
        let receipts = h.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM session_operations WHERE session_id=?1 AND operation_kind='legacy_session_file_transition' AND state='completed'", vec![id.into()]).await.unwrap();
        assert_eq!(receipts.as_deref(), Some("1"));
    }
    h.stop().await;
}

// 沿用独立反例的真实并发/文件观察/SIGKILL/重启链；观察窗口同时接受正常prepared，以便核验修后保护。
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_concurrent_actual_sigkill_retains_prepared_and_recovers() {
    let home = unique("independent-concurrent-crash");
    let mut binary = Binary::start(&home).await;
    let home = home.canonicalize().unwrap();
    let dir = home.join("agents/lingxi/sessions/archived");
    let mut files = Vec::new();
    for id in ["independent-a", "independent-b"] {
        let (code, value) = binary
            .client
            .request(
                "POST",
                "/lingxi/v1/sessions",
                json!({"sessionId":id,"agentId":"lingxi","title":id}),
            )
            .await;
        assert_eq!(code, 201, "{value}");
        assert_eq!(
            binary
                .client
                .request(
                    "POST",
                    "/lingxi/v1/sessions/archive",
                    json!({"sessionId":id})
                )
                .await
                .0,
            200
        );
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join(format!("{id}.jsonl"));
        std::fs::write(&src, jsonl(id)).unwrap();
        let mut body: Value = serde_json::from_str(&sidecar_bytes(id, &src)).unwrap();
        body["independentPadding"] = json!("x".repeat(16 * 1024 * 1024));
        std::fs::write(sidecar(&src), body.to_string()).unwrap();
        files.push((id.to_string(), src));
    }
    let pid = binary.child.id().unwrap();
    let db = home.join("lingxi-service/data/runs.db");
    let watch = files.clone();
    let watcher = tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open(db).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            for (id, src) in &watch {
                let state:Option<(String,String,String)>=conn.query_row("SELECT o.state,COALESCE(o.receipt_json,''),s.lifecycle FROM session_operations o JOIN sessions s ON s.session_id=o.session_id WHERE o.session_id=?1 AND o.operation_kind='legacy_session_file_transition' ORDER BY o.created_at_unix_ms DESC LIMIT 1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().unwrap();
                if let Some((state, receipt, phase)) = state {
                    let total:i64=conn.query_row("SELECT COUNT(*) FROM session_operations WHERE session_id IN ('independent-a','independent-b') AND operation_kind='legacy_session_file_transition'",[],|r|r.get(0)).unwrap();
                    if phase == "archived" && total == 2 {
                        let moved = active(src).exists()
                            && !src.exists()
                            && sidecar(src).exists()
                            && !sidecar(&active(src)).exists();
                        if moved {
                            // 仅终止本实验实际启动的合成服务，捕获同一真实双文件中断并核验prepared保护。
                            assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGKILL) }, 0);
                            return Some((id.clone(), src.clone(), state, receipt, phase));
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_micros(20));
        }
        None
    });
    let ca = binary.client.clone();
    let cb = binary.client.clone();
    let a = tokio::spawn(async move {
        ca.request(
            "POST",
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"independent-a"}),
        )
        .await
    });
    let b = tokio::spawn(async move {
        cb.request(
            "POST",
            "/lingxi/v1/sessions/restore",
            json!({"sessionId":"independent-b"}),
        )
        .await
    });
    let observed = watcher.await.unwrap();
    if let Some((id, src, state, receipt, phase)) = observed {
        binary.child.wait().await.unwrap();
        let ra = a.await.unwrap();
        let rb = b.await.unwrap();
        eprintln!("INDEPENDENT_CONCURRENT_JOURNAL id={id} state={state} phase={phase} receipt={receipt} responses={ra:?}/{rb:?}");
        assert!(active(&src).exists() && !src.exists() && sidecar(&src).exists());
        let restarted = Binary::start(&home).await;
        let restored_back = src.exists() && !active(&src).exists();
        eprintln!("INDEPENDENT_RESTART_BACK={restored_back} source_jsonl={} active_jsonl={} source_sidecar={} active_sidecar={}",src.exists(),active(&src).exists(),sidecar(&src).exists(),sidecar(&active(&src)).exists());
        let conn = rusqlite::Connection::open(home.join("lingxi-service/data/runs.db")).unwrap();
        for (other_id, other_src) in &files {
            assert!(other_src.exists() && !active(other_src).exists());
            assert_eq!(std::fs::read_to_string(other_src).unwrap(), jsonl(other_id));
            let mut expected: Value =
                serde_json::from_str(&sidecar_bytes(other_id, other_src)).unwrap();
            expected["independentPadding"] = json!("x".repeat(16 * 1024 * 1024));
            assert_eq!(
                std::fs::read_to_string(sidecar(other_src)).unwrap(),
                expected.to_string()
            );
            assert!(!sidecar(&active(other_src)).exists());
            let pending:i64=conn.query_row("SELECT COUNT(*) FROM session_operations WHERE session_id=?1 AND state='prepared'",[other_id],|r|r.get(0)).unwrap();
            assert_eq!(pending, 0);
        }
        restarted.stop().await;
        assert!(
            restored_back,
            "另一个会话错误完成仍在执行的prepared日志，真实重启没有退回未提交JSONL"
        );
        assert_eq!(
            state, "prepared",
            "真实移动未提交期间，本日志必须保持prepared"
        );
        assert!(receipt.is_empty(), "活跃日志不得提前产生completed收据");
        assert_eq!(std::fs::read_to_string(&src).unwrap(), jsonl(&id));
        assert!(sidecar(&src).exists() && !sidecar(&active(&src)).exists());
    } else {
        assert_eq!(a.await.unwrap().0, 200);
        assert_eq!(b.await.unwrap().0, 200);
        binary.stop().await;
        panic!("没有捕获跨会话真实双文件中断窗口，不能以未观察到反例签PASS");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_final_writer_refuses_completed_bad_digest_and_replaced_snapshot_journals() {
    for kind in ["journal-completed", "journal-digest", "journal-snapshot"] {
        let h = boot(unique(kind)).await;
        let mut caught = false;
        for index in 0..12 {
            let id = format!("{kind}-{index}");
            let src = h.archived(&id, "lingxi").await;
            let original = std::fs::read_to_string(&src).unwrap();
            let attached = std::fs::read_to_string(sidecar(&src)).unwrap();
            let db = h.state.storage().db_path().to_path_buf();
            let watched = id.clone();
            let watcher = tokio::task::spawn_blocking(move || {
                let conn = rusqlite::Connection::open(db).unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(15);
                while std::time::Instant::now() < deadline {
                    let count:i64=conn.query_row("SELECT COUNT(*) FROM session_operations WHERE session_id=?1 AND operation_kind='legacy_session_file_transition' AND state='prepared'",[&watched],|r|r.get(0)).unwrap();
                    if count > 0 {
                        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
                        let phase: String = conn
                            .query_row(
                                "SELECT lifecycle FROM sessions WHERE session_id=?1",
                                [&watched],
                                |r| r.get(0),
                            )
                            .unwrap();
                        return (conn, phase);
                    }
                    std::thread::yield_now();
                }
                panic!("未观察到真实日志，不能制造假prepared测试")
            });
            let client = h.client();
            let requested = id.clone();
            let pending = tokio::spawn(async move {
                client
                    .request(
                        "POST",
                        "/lingxi/v1/sessions/restore",
                        json!({"sessionId":requested}),
                    )
                    .await
            });
            let (writer, phase) = watcher.await.unwrap();
            if phase != "archived" {
                writer.execute_batch("ROLLBACK").unwrap();
                assert_eq!(pending.await.unwrap().0, 200);
                continue;
            }
            assert!(src.exists() && sidecar(&src).exists() && !active(&src).exists());
            assert!(tokio::time::timeout(
                Duration::from_millis(200),
                h.state
                    .storage()
                    .query_one_text("SELECT 'actual-final-writer-waits'", vec![])
            )
            .await
            .is_err());
            let (operation,text,digest):(String,String,String)=writer.query_row("SELECT operation_id,snapshot_json,input_digest FROM session_operations WHERE session_id=?1 AND operation_kind='legacy_session_file_transition' AND state='prepared'",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
            match kind {
                "journal-completed" => {
                    writer
                        .execute(
                            "UPDATE session_operations SET state='completed' WHERE operation_id=?1",
                            [&operation],
                        )
                        .unwrap();
                }
                "journal-digest" => {
                    writer.execute("UPDATE session_operations SET input_digest='bad-external-digest' WHERE operation_id=?1",[&operation]).unwrap();
                }
                "journal-snapshot" => {
                    let mut replacement: Value = serde_json::from_str(&text).unwrap();
                    replacement["created_at_ms"] = json!(1);
                    let replaced = replacement.to_string();
                    let replaced_digest = lingxi_protocol::canon::sha256_hex(replaced.as_bytes());
                    writer.execute("UPDATE session_operations SET snapshot_json=?2,input_digest=?3 WHERE operation_id=?1",rusqlite::params![operation,replaced,replaced_digest]).unwrap();
                }
                _ => unreachable!(),
            }
            writer.execute_batch("COMMIT").unwrap();
            let (status, response) = pending.await.unwrap();
            eprintln!("ARCHIVE_FINAL_JOURNAL kind={kind} status={status} response={response}");
            assert_eq!(
                status, 500,
                "日志已经失效，不能继续触碰文件或报告成功: {response}"
            );
            assert!(response
                .to_string()
                .contains("prepared journal missing/changed"));
            assert_archived_intact(&src, &original, &attached);
            assert!(!active(&src).exists() && !sidecar(&active(&src)).exists());
            assert_eq!(h.life(&id).await.as_deref(), Some("archived"));
            // 外部改写的记录不应被失败收尾抹成已处理；仅测试修复自己注入的变更后再重启。
            assert_eq!(
                h.pending(&id).await,
                if kind == "journal-completed" {
                    "0"
                } else {
                    "1"
                }
            );
            writer.execute("UPDATE session_operations SET state='prepared',snapshot_json=?2,input_digest=?3 WHERE operation_id=?1",rusqlite::params![operation,text,digest]).unwrap();
            let home = h.stop().await;
            let restarted = boot(home).await;
            assert_eq!(restarted.pending(&id).await, "0");
            assert_archived_intact(&src, &original, &attached);
            assert_eq!(
                restarted
                    .post("/lingxi/v1/sessions/restore", json!({"sessionId":id}))
                    .await
                    .0,
                200
            );
            restarted.stop().await;
            caught = true;
            break;
        }
        assert!(caught, "没有捕获真实最终事务等待窗口，不能计PASS");
    }
}
