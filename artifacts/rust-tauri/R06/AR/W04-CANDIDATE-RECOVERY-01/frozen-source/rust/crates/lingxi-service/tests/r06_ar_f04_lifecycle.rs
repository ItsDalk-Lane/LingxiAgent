//! R06-AR-F04 正式入口回归：真实HTTP、数据库、文件和物理线程。
//! 只有外部供应商响应与时钟可控；引用、分支、清理、登记均由实际服务执行。
//! 继承原本地主人全见合同，设备主体按原认证授权；不以全局旧别名放大会话范围。

use lingxi_service::ServiceClock as _;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnProviderPort,
};
use lingxi_protocol::{ContentBlock, ErrorCode, ModelCallId, NormalizedMessage, ProtocolError};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

async fn archive_and_delete(server: &TestServer, session: &str) {
    let body = serde_json::json!({"sessionId":session}).to_string();
    assert_eq!(
        server.post("/lingxi/v1/sessions/archive", &body).await.0,
        200
    );
    assert_eq!(
        server
            .post("/lingxi/v1/sessions/archived/delete", &body)
            .await
            .0,
        200
    );
}

/// 外部文件即使恰在缓存目录也不授予删除权；另一会话真实引用保护原物到最后移除。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f04_existing_paths_preserve_authority_and_other_session_payload() {
    let server = start_server(
        "path-ownership",
        ScriptedProvider::new(vec![ScriptStep::Final("bound".into())]),
    )
    .await;
    for sid in ["path-owner", "path-ref", "path-external", "normal-owned"] {
        create_session(&server, sid, sid).await;
    }
    let normal = upload(&server, "normal-owned", "normal.png").await;
    let normal_path = PathBuf::from(normal["realPath"].as_str().unwrap());
    archive_and_delete(&server, "normal-owned").await;
    assert!(!normal_path.exists(), "真正无引用的托管副本必须实际删除");
    let owned = upload(&server, "path-owner", "owned.png").await;
    let path = PathBuf::from(owned["realPath"].as_str().unwrap());
    let original = std::fs::read(&path).unwrap();
    let external = path.parent().unwrap().join("existing-user-authority.txt");
    std::fs::write(&external, b"EXTERNAL AUTHORITY").unwrap();
    let authority = server
        .state
        .session_files()
        .register_file("path-external", &external, None, None, "tool")
        .await
        .unwrap();
    assert_eq!(authority.storage_kind, "external");
    archive_and_delete(&server, "path-external").await;
    assert_eq!(std::fs::read(&external).unwrap(), b"EXTERNAL AUTHORITY");
    let reference = server
        .state
        .session_files()
        .register_file("path-ref", &path, None, None, "tool")
        .await
        .unwrap();
    assert_eq!(reference.storage_kind, "external");
    server
        .execute(
            "path-ref",
            &format!("[SessionFile] {{\"fileId\":\"{}\"}}", reference.file_id),
        )
        .await;
    let refs = server
        .state
        .session_files()
        .references_for_session("path-ref")
        .await
        .unwrap();
    assert!(refs.iter().any(|r| r.file_id == reference.file_id));
    archive_and_delete(&server, "path-owner").await;
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let request = serde_json::json!({"resource":{"kind":"session-file","fileId":reference.file_id,"sessionId":"path-ref"},"sessionId":"path-ref","encoding":"base64"}).to_string();
    let (status, body) = server.post("/lingxi/v1/resource-io/read", &request).await;
    assert_eq!(status, 200, "已提交引用必须继续实际可读: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["content"],
        "iVBORw0KGgo="
    );
    archive_and_delete(&server, "path-ref").await;
    assert!(!path.exists(), "最后真实引用移除后回收托管原物");
    assert_eq!(std::fs::read(&external).unwrap(), b"EXTERNAL AUTHORITY");
    println!("F04 OWNERSHIP genuine upload removed; existing cache-root authority preserved; actual cross-session committed ref read200/8B after owner delete; last ref removed then actual owned byte collected");
    server.stop_and_clean().await;
}

/// 真实冷清扫后重传相同字节恢复原身份；中断恢复不得破坏旧登记或留下新孤立副本。
#[tokio::test]
async fn f04_expired_owned_blob_recovers_identity_atomically() {
    let clock = Arc::new(lingxi_service::ManualClock::new(1_000_000));
    let server = start_server_at(
        synthetic_home("blob-recovery"),
        ScriptedProvider::new(vec![]),
        clock.clone(),
    )
    .await;
    create_session(&server, "blob-recovery", "blob-recovery").await;
    let original = upload(&server, "blob-recovery", "original.png").await;
    let original_path = PathBuf::from(original["realPath"].as_str().unwrap());
    let bytes = std::fs::read(&original_path).unwrap();
    clock.advance(lingxi_service::sessionfiles::SESSION_FILE_CACHE_INACTIVE_TTL_MS + 1);
    let cleaned = server
        .state
        .session_files()
        .cleanup_cold_sessions()
        .await
        .unwrap();
    assert_eq!(cleaned.deleted, 1);
    assert!(!original_path.exists());
    let view = server
        .state
        .session_files()
        .get_file("blob-recovery", original["fileId"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(view.status, "expired");
    let db = rusqlite::Connection::open(server.home.join("lingxi-service/data/runs.db")).unwrap();
    db.execute_batch("CREATE TRIGGER f04_recovery_interrupt BEFORE UPDATE ON session_files WHEN NEW.status='available' BEGIN SELECT RAISE(ABORT,'synthetic owned recovery interrupt'); END;").unwrap();
    let request =
        serde_json::json!({"mime":"image/png","name":"repeat.png","dataBase64":"iVBORw0KGgo="})
            .to_string();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/blob-recovery/attachments/blob",
            &request,
        )
        .await;
    assert_eq!(status, 500, "中断必须准确拒绝: {body}");
    assert_eq!(
        std::fs::read_dir(original_path.parent().unwrap())
            .unwrap()
            .count(),
        0
    );
    let old_status: String = db
        .query_row(
            "SELECT status FROM session_files WHERE file_id=?1",
            [original["fileId"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old_status, "expired");
    db.execute_batch("DROP TRIGGER f04_recovery_interrupt")
        .unwrap();
    let recovered = upload(&server, "blob-recovery", "repeat.png").await;
    assert_eq!(recovered["fileId"], original["fileId"]);
    assert_eq!(recovered["status"], "available");
    assert_eq!(
        std::fs::read(recovered["realPath"].as_str().unwrap()).unwrap(),
        bytes
    );
    let repeated = upload(&server, "blob-recovery", "again.png").await;
    assert_eq!(repeated["realPath"], recovered["realPath"]);
    assert_eq!(
        std::fs::read_dir(original_path.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM session_files WHERE owner_session_id='blob-recovery'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    println!("F04 RECOVERY actual cleanup1/expired; injected UPDATE failure500 compensated new byte and retained old identity; recovery201/sameID/available8B; repeated201/same actual path/exactly one row and payload");
    drop(db);
    server.stop_and_clean().await;
}

/// 可信旧登记的 missing 与 expired 不同：原托管文件确实恢复到原名时可正常复活。
#[tokio::test]
async fn f04_trusted_missing_owned_name_restores_without_new_ownership() {
    let server = start_server("trusted-missing", ScriptedProvider::new(vec![])).await;
    create_session(&server, "trusted-missing", "trusted-missing").await;
    let original = upload(&server, "trusted-missing", "trusted.png").await;
    let id = original["fileId"].as_str().unwrap();
    let path = PathBuf::from(original["realPath"].as_str().unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let row = lingxi_adapters::storage::session_files::get_session_file(server.state.storage(), id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.storage_kind,
        lingxi_adapters::storage::session_files::StorageKind::ManagedCache
    );
    let sidecar = server.home.join("trusted-original.files.json");
    let saved = serde_json::json!({"version":1,"files":{id:{"fileId":id,"realPath":path,"filePath":path,"sourceKey":row.source_key,"storageKind":"managed_cache","filename":row.filename,"mime":row.mime,"size":row.size_bytes,"origin":"user_upload","createdAt":row.registered_at_ms,"status":"available"}},"refs":[]});
    std::fs::write(&sidecar, saved.to_string()).unwrap();
    let moved = server.home.join("retained-original-owned-byte.png");
    for restore in ["blob", "same-path"] {
        assert!(server
            .state
            .session_files()
            .references_for_session("trusted-missing")
            .await
            .unwrap()
            .is_empty());
        std::fs::rename(&path, &moved).unwrap();
        lingxi_adapters::storage::session_files::delete_session_file(server.state.storage(), id)
            .await
            .unwrap();
        let (status, body) = server
            .post(
                "/lingxi/v1/sessions/trusted-missing/files/import-legacy",
                &serde_json::json!({"sidecarPath":sidecar}).to_string(),
            )
            .await;
        assert_eq!(status, 200, "导入本程序真实上传的原登记: {body}");
        assert_eq!(
            server
                .state
                .session_files()
                .get_file("trusted-missing", id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "missing"
        );
        std::fs::rename(&moved, &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let restored = if restore == "blob" {
            let uploaded = upload(&server, "trusted-missing", "trusted.png").await;
            assert_eq!(uploaded["fileId"], id);
            assert_eq!(uploaded["realPath"], path.to_string_lossy().as_ref());
            server
                .state
                .session_files()
                .get_file("trusted-missing", id)
                .await
                .unwrap()
                .unwrap()
        } else {
            server
                .state
                .session_files()
                .register_file("trusted-missing", &path, None, None, "tool")
                .await
                .unwrap()
        };
        assert_eq!(restored.file_id, id);
        assert_eq!(restored.storage_kind, "managed_cache");
        assert_eq!(restored.status, "available");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        println!("F04 TRUSTED MISSING {restore}: own actual upload8B provenance -> retained same byte moved -> original sidecar import missing -> original byte/name restored -> originalID available8B, no extra file or ownership granted");
    }
    server.stop_and_clean().await;
}

// ── 确定性 provider 替身（只产出外部响应，不写任何状态） ──

enum ScriptStep {
    /// 立即返回 Final。
    Final(String),
}

struct ScriptedProvider {
    script: std::sync::Mutex<VecDeque<ScriptStep>>,
    inputs: std::sync::Mutex<Vec<String>>,
}

impl ScriptedProvider {
    fn new(script: Vec<ScriptStep>) -> Arc<Self> {
        Arc::new(Self {
            script: std::sync::Mutex::new(script.into_iter().collect()),
            inputs: std::sync::Mutex::new(Vec::new()),
        })
    }
}

impl TurnProviderPort for ScriptedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.provider".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ModelCallId,
        _input: &'a ModelTurnInput,
        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.inputs.lock().unwrap().push(format!("{_input:?}"));
        let step = self.script.lock().unwrap().pop_front();
        let ctx_at_issue = ctx.clone();
        let call_id = call.to_string();
        Box::pin(async move {
            match step {
                Some(ScriptStep::Final(text)) => ProviderTurnResult::of_ctx(
                    &ctx_at_issue,
                    ProviderTurn::Final {
                        message: NormalizedMessage {
                            role: "assistant".to_string(),
                            content: vec![ContentBlock::Text { text }],
                            model_call_id: Some(ModelCallId::new(call_id)),
                        },
                    },
                ),
                None => ProviderTurnResult::of_ctx(
                    &ctx_at_issue,
                    ProviderTurn::Failed {
                        error: ProtocolError::new(
                            ErrorCode::UpstreamUnavailable,
                            "script exhausted",
                            false,
                        ),
                        retryable: false,
                    },
                ),
            }
        })
    }
}

// ── 实际服务夹具 ──

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    token: String,
    state: ServiceState,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t03-svc-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

async fn start_server(tag: &str, provider: Arc<ScriptedProvider>) -> TestServer {
    start_server_at(
        synthetic_home(tag),
        provider,
        Arc::new(lingxi_service::SystemClock),
    )
    .await
}
async fn start_server_at(
    home: PathBuf,
    provider: Arc<ScriptedProvider>,
    clock: Arc<dyn lingxi_service::ServiceClock>,
) -> TestServer {
    start_server_at_scoped(home, provider, clock, None).await
}

async fn start_server_at_scoped(
    home: PathBuf,
    provider: Arc<ScriptedProvider>,
    clock: Arc<dyn lingxi_service::ServiceClock>,
    workspace_root: Option<PathBuf>,
) -> TestServer {
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(
        config,
        &layout,
        ServiceDeps {
            turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
            clock,
            workspace_root,
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("bootstrap");
    let token = state.auth().local_token();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let serve_state = state.clone();
    let handle = tokio::spawn(async move {
        run(
            serve_state,
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
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!("readiness: {err}; service result: {:?}", handle.await),
    };
    println!(
        "READY addr={addr} actual_home={} source=cli os_home={} effective_lingxi_home={}",
        home.display(),
        std::env::var("HOME").unwrap(),
        std::env::var("LINGXI_HOME").unwrap()
    );
    TestServer {
        addr,
        home,
        token,
        state,
        stop: stop_tx,
        handle,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f04_local_read_cancelled_future_requires_physical_lease() {
    let clock = Arc::new(lingxi_service::ManualClock::new(1000));
    let home = synthetic_home("local-physical-read");
    let server = start_server_at_scoped(
        home.clone(),
        ScriptedProvider::new(vec![]),
        clock.clone(),
        Some(home),
    )
    .await;
    create_session(&server, "reader", "reader").await;
    let file = upload(&server, "reader", "reader.png").await;
    let path = PathBuf::from(file["realPath"].as_str().unwrap());
    let expected = std::fs::read(&path).unwrap();
    let io = server.state.resource_io().clone();
    let ctx = lingxi_service::resourceio::OpContext::local_owner("reader");
    let reference = serde_json::json!({"kind":"local-file","path":path});
    assert_eq!(io.read(&reference, &ctx).await.unwrap().content, expected);
    let request =
        serde_json::json!({"resource":reference,"sessionId":"reader","encoding":"base64"})
            .to_string();
    let (status, body) = server.post("/lingxi/v1/resource-io/read", &request).await;
    assert_eq!(status, 200, "正式LocalOnly入口: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["content"],
        "iVBORw0KGgo="
    );
    let device = server.mint_device_token("user_local").await;
    for method in ["read", "stat", "list", "search", "materialize"] {
        let (status, body) = http(
            &server.addr,
            "POST",
            &format!("/lingxi/v1/resource-io/{method}"),
            &[("Authorization", &format!("Bearer {device}"))],
            Some(&request),
        )
        .await;
        assert_eq!(status, 403, "原LocalOnly合同拒设备主体 {method}: {body}");
    }
    assert_eq!(io.stat(&reference, &ctx).await.unwrap()["exists"], true);
    assert_eq!(
        io.materialize(&reference, &ctx).await.unwrap()["filePath"],
        path.to_string_lossy().as_ref()
    );
    let directory = serde_json::json!({"kind":"local-file","path":path.parent().unwrap()});
    io.list(&directory, &ctx).await.unwrap();
    io.search(
        &directory,
        Some("reader".into()),
        Some("name"),
        Some(80),
        &ctx,
    )
    .await
    .unwrap();
    let before = server.state.storage().query_one_text("SELECT json_object('messages',(SELECT COUNT(*) FROM messages),'refs',(SELECT COUNT(*) FROM session_file_refs),'events',(SELECT COUNT(*) FROM key_events),'history',(SELECT COUNT(*) FROM file_history_snapshots))", vec![]).await.unwrap();
    let denied = std::env::temp_dir().join("f04-outside-workspace-sentinel.txt");
    std::fs::write(&denied, b"unchanged").unwrap();
    let outside = serde_json::json!({"kind":"local-file","path":denied});
    assert_eq!(io.read(&outside, &ctx).await.unwrap_err().status, 403);
    assert_eq!(io.stat(&outside, &ctx).await.unwrap_err().status, 403);
    assert_eq!(
        io.materialize(&outside, &ctx).await.unwrap_err().status,
        403
    );
    let outside_dir = serde_json::json!({"kind":"local-file","path":denied.parent().unwrap()});
    assert_eq!(io.list(&outside_dir, &ctx).await.unwrap_err().status, 403);
    assert_eq!(
        io.search(
            &outside_dir,
            Some("sentinel".into()),
            Some("name"),
            Some(80),
            &ctx
        )
        .await
        .unwrap_err()
        .status,
        403
    );
    assert_eq!(std::fs::read(&denied).unwrap(), b"unchanged");
    let after = server.state.storage().query_one_text("SELECT json_object('messages',(SELECT COUNT(*) FROM messages),'refs',(SELECT COUNT(*) FROM session_file_refs),'events',(SELECT COUNT(*) FROM key_events),'history',(SELECT COUNT(*) FROM file_history_snapshots))", vec![]).await.unwrap();
    assert_eq!(before, after, "正常读族和权限拒绝必须零写副作用");
    println!("LOCAL_READ authorized read8B/stat/list/search/materialize; denied outside roots; actual SQL before={before:?} after={after:?}");
    clock.advance(lingxi_service::sessionfiles::SESSION_FILE_CACHE_INACTIVE_TTL_MS + 1);
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered = std::sync::Mutex::new(Some(entered_tx));
    let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let gate = release.clone();
    server
        .state
        .session_files()
        .set_payload_read_observer(Arc::new(move || {
            let _ = entered.lock().unwrap().take().unwrap().send(());
            let (lock, cv) = &*gate;
            let (_held, timeout) = cv
                .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(5), |v| !*v)
                .unwrap();
            assert!(!timeout.timed_out(), "物理读取线程屏障必须释放");
            println!("LOCAL_READ actual physical barrier released");
        }));
    let reader = tokio::spawn(async move { io.read(&reference, &ctx).await });
    entered_rx.await.unwrap();
    reader.abort();
    assert!(reader.await.unwrap_err().is_cancelled());
    let files = server.state.session_files().clone();
    let cleanup = tokio::spawn(async move { files.cleanup_cold_sessions().await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !cleanup.is_finished(),
        "Future取消后物理线程仍阻塞，清理不得越过正式租约"
    );
    assert_eq!(std::fs::read(&path).unwrap(), expected);
    {
        let (lock, cv) = &*release;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
    assert_eq!(cleanup.await.unwrap().unwrap().deleted, 1);
    assert!(!path.exists());
    println!(
        "LOCAL_READ canceled future retained8B until physical exit; then real cleanup deleted=1"
    );
    server.stop_and_clean().await;
}

impl TestServer {
    fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn stop_and_clean(self) {
        self.stop.send(()).expect("server still listening");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("server shuts down within timeout")
            .expect("server task join")
            .expect("clean serve result");
        println!("F04 RETAINED synthetic_home={}", self.home.display());
    }

    async fn post(&self, path: &str, body: &str) -> (u16, String) {
        http(
            &self.addr,
            "POST",
            path,
            &[("Authorization", &self.bearer())],
            Some(body),
        )
        .await
    }

    async fn get(&self, path: &str) -> (u16, String) {
        http(
            &self.addr,
            "GET",
            path,
            &[("Authorization", &self.bearer())],
            None,
        )
        .await
    }

    /// 执行一轮并等待 run 终态（scripted Final 即时完成）。
    async fn execute(&self, session_id: &str, input: &str) -> String {
        let (status, body) = self
            .post(
                &format!("/lingxi/v1/sessions/{session_id}/execute"),
                &serde_json::json!({ "input": input }).to_string(),
            )
            .await;
        assert_eq!(status, 200, "execute {session_id}: {body}");
        let run_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["runId"]
            .as_str()
            .unwrap()
            .to_string();
        // 等终态（最多 5s）。
        for _ in 0..100 {
            let (status, reason) = run_row(self.state.storage(), &run_id).await;
            if status == "completed" || status == "failed" {
                assert_eq!(
                    status, "completed",
                    "scripted Final must complete (reason: {reason:?})"
                );
                return run_id;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("run {run_id} did not finalize within 5s");
    }

    /// 当前分支的消息 id 链（GET branch）。
    async fn branch_ids(&self, session_id: &str) -> Vec<String> {
        let (status, body) = self
            .get(&format!("/lingxi/v1/sessions/{session_id}/branch"))
            .await;
        assert_eq!(status, 200, "branch {session_id}: {body}");
        let parsed = serde_json::from_str::<serde_json::Value>(&body).unwrap();
        parsed["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["messageId"].as_str().unwrap().to_string())
            .collect()
    }

    /// 设备主体令牌（跨主体 403 测试）。
    async fn mint_device_token(&self, user_id: &str) -> String {
        let (status, body) = self
            .post(
                "/lingxi/v1/devices/credentials",
                &serde_json::json!({ "userId": user_id, "scopes": ["chat"] }).to_string(),
            )
            .await;
        assert_eq!(status, 201, "mint device credential: {body}");
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["secret"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

#[tokio::test]
async fn f04_attempts_and_recovery_keep_original_history_anchor() {
    use lingxi_adapters::storage::session_tree::{append_message, AppendMessage};
    use lingxi_kernel::ports::StoragePort as _;
    let home = synthetic_home("attempt-anchor");
    let provider = ScriptedProvider::new(vec![]);
    let clock = Arc::new(lingxi_service::ManualClock::new(1000));
    let server = start_server_at(home.clone(), provider.clone(), clock.clone()).await;
    create_session(&server, "attempt-anchor", "attempt-anchor").await;
    append_message(
        server.state.storage(),
        AppendMessage {
            session_id: "attempt-anchor",
            message_id: "original",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: r#"{"text":"real pre-run branch"}"#,
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: 1000,
        },
    )
    .await
    .unwrap();
    let ctx = |run_id: &str, attempt| {
        let run = lingxi_protocol::RunId::new(run_id.to_string());
        lingxi_kernel::RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("attempt-anchor"),
            attempt: lingxi_kernel::attempt_id(&run, attempt),
            run_id: run,
            generation: 1,
        }
    };
    let known = "r-f04-known-anchor";
    server
        .state
        .storage()
        .record_run_started(&ctx(known, 1), 1000)
        .await
        .unwrap();
    let anchor_sql = "SELECT json_group_array(json_object('run',run_id,'anchor',anchor_message_id)) FROM run_history_anchors ORDER BY run_id";
    let first = server
        .state
        .storage()
        .query_one_text(anchor_sql, vec![])
        .await
        .unwrap();
    assert!(first.as_deref().unwrap().contains("original"));
    append_message(
        server.state.storage(),
        AppendMessage {
            session_id: "attempt-anchor",
            message_id: "moved",
            parent_message_id: Some("original"),
            run_id: None,
            role: "user",
            content_json: r#"{"text":"real moved branch"}"#,
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: 1001,
        },
    )
    .await
    .unwrap();
    server
        .state
        .storage()
        .record_attempt_started(&ctx(known, 2), 1002)
        .await
        .unwrap();
    assert_eq!(
        server
            .state
            .storage()
            .query_one_text(anchor_sql, vec![])
            .await
            .unwrap(),
        first,
        "第二次尝试不得重建或移动锚"
    );
    assert!(
        !server
            .state
            .storage()
            .record_run_started(&ctx(known, 1), 1003)
            .await
            .unwrap()
            .newly_committed,
        "原合同允许同身份重放，且不能重建归属"
    );
    create_session(&server, "foreign-anchor", "foreign-anchor").await;
    let mut illegal = ctx(known, 1);
    illegal.session_id = lingxi_protocol::SessionId::new("foreign-anchor");
    assert!(
        server
            .state
            .storage()
            .record_run_started(&illegal, 1003)
            .await
            .is_err(),
        "跨会话非法复用Run仍必须报错"
    );
    let unknown = "r-f04-legacy-unproven";
    server
        .state
        .storage()
        .record_run_started(&ctx(unknown, 1), 1004)
        .await
        .unwrap();
    // 精确模拟已存在但没有可证明历史锚的旧运行；不根据当前分支回填。
    let legacy_fixture =
        rusqlite::Connection::open(home.join("lingxi-service/data/runs.db")).unwrap();
    assert_eq!(
        legacy_fixture
            .execute("DELETE FROM run_history_anchors WHERE run_id=?1", [unknown])
            .unwrap(),
        1
    );
    drop(legacy_fixture);
    server
        .state
        .storage()
        .record_attempt_started(&ctx(unknown, 2), 1005)
        .await
        .unwrap();
    assert_eq!(
        server
            .state
            .storage()
            .query_one_text(anchor_sql, vec![])
            .await
            .unwrap(),
        first,
        "未知旧Run的重试不得铸造归属"
    );
    assert_eq!(
        server
            .state
            .storage()
            .query_one_text("SELECT COUNT(*) FROM run_attempts", vec![])
            .await
            .unwrap()
            .as_deref(),
        Some("4")
    );
    println!("ATTEMPT2 actual anchors={first:?}; known attempt2 succeeded, run reuse refused, legacy-unproven attempt2 did not backfill");
    server.stop.send(()).unwrap();
    server.handle.await.unwrap().unwrap();
    drop(server.state);
    for restart in 0..2 {
        let restarted = start_server_at(home.clone(), provider.clone(), clock.clone()).await;
        let actual = restarted
            .state
            .storage()
            .query_one_text(anchor_sql, vec![])
            .await
            .unwrap();
        assert_eq!(actual, first, "恢复和重复启停只保留首个真实锚");
        assert_eq!(
            run_row(restarted.state.storage(), known).await.0,
            "interrupted_needs_attention"
        );
        assert_eq!(
            run_row(restarted.state.storage(), unknown).await.0,
            "interrupted_needs_attention"
        );
        let (status, body) = restarted
            .get("/lingxi/v1/sessions/attempt-anchor/history?all=1")
            .await;
        assert_eq!(status, 200);
        let history: serde_json::Value = serde_json::from_str(&body).unwrap();
        let items = history["items"].as_array().unwrap();
        assert_eq!(
            items
                .iter()
                .filter(|item| item["kind"] == "run_terminal" && item["runId"] == known)
                .count(),
            1,
            "有真实锚的恢复中断必须准确展示一次"
        );
        assert!(
            !items.iter().any(|item| item["runId"] == unknown),
            "未知旧运行不猜测当前分支归属"
        );
        assert!(
            provider.inputs.lock().unwrap().is_empty(),
            "恢复审计不发给模型"
        );
        println!("ATTEMPT2 restart={restart} original-anchor={actual:?} actual-model-inputs=0");
        restarted.stop_and_clean().await;
    }
}

async fn run_row(
    storage: &Arc<lingxi_adapters::storage::RunDatabase>,
    run_id: &str,
) -> (String, Option<String>) {
    let status = storage
        .query_one_text(
            "SELECT status FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query status")
        .expect("run row");
    let reason = storage
        .query_one_text(
            "SELECT terminal_reason FROM runs WHERE run_id = ?1",
            vec![run_id.to_string()],
        )
        .await
        .expect("query reason");
    (status, reason)
}

/// 一次实际TCP连接发送HTTP请求，认证头不进入公开日志。
async fn http(
    addr: &SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String) {
    http_written(addr, method, path, headers, body, None).await
}
async fn http_written(
    addr: &SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
    written: Option<tokio::sync::oneshot::Sender<()>>,
) -> (u16, String) {
    let (status, body, _) = http_full(addr, method, path, headers, body, written).await;
    (status, body)
}
async fn http_full(
    addr: &SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
    written: Option<tokio::sync::oneshot::Sender<()>>,
) -> (u16, String, Option<String>) {
    let request_input = body.unwrap_or("").to_string();
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.expect("write head");
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).await.expect("write body");
    }
    if let Some(written) = written {
        let _ = written.send(());
    }
    let mut raw = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(err)
                if err.kind() == std::io::ErrorKind::ConnectionReset
                    || err.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                break;
            }
            Err(err) => panic!("read response: {err}"),
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("malformed response: {text:?}"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no status in {head:?}"));
    // 合成设备凭据仍仅留在独占测试目录，公共请求记录不输出认证秘密。
    let logged_body = if path.contains("device") {
        "[LOCAL_ONLY authentication response]"
    } else {
        body
    };
    println!(
        "F04_HTTP {}",
        serde_json::json!({"method":method,"path":path,"input":request_input,"status":status,"body":logged_body})
    );
    let etag = head.lines().find_map(|line| {
        line.split_once(':').and_then(|(key, value)| {
            key.eq_ignore_ascii_case("etag")
                .then(|| value.trim().to_owned())
        })
    });
    (status, body.to_string(), etag)
}

/// 创建顶层会话（管理面创建路由）。
async fn create_session(server: &TestServer, session_id: &str, title: &str) {
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": session_id,
                "agentId": "agent",
                "title": title,
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create session {session_id}: {body}");
}

#[tokio::test]
async fn f04_c01_formal_fork_retains_attachment_in_siblings() {
    let server = start_server(
        "f04-c01",
        ScriptedProvider::new(vec![ScriptStep::Final("ok".into())]),
    )
    .await;
    create_session(&server, "parent", "source").await;
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/parent/attachments/blob",
            &serde_json::json!({"name":"a.png","mime":"image/png","dataBase64":"iVBORw0KGgo="})
                .to_string(),
        )
        .await;
    assert_eq!(status, 201, "upload: {body}");
    let file: serde_json::Value =
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["file"].clone();
    let id = file["fileId"].as_str().unwrap();
    let bytes = std::fs::read(file["realPath"].as_str().unwrap()).unwrap();
    let (status, body) = server
        .get(&format!("/lingxi/v1/sessions/parent/files/{id}"))
        .await;
    assert_eq!(status, 200, "normal positive: {body}");
    println!(
        "POSITIVE uploaded parent attachment readable with correct bytes {}",
        bytes.len()
    );
    let run = server
        .execute("parent", &format!("[SessionFile] {{\"fileId\":\"{id}\"}}"))
        .await;
    for child in ["child-a", "child-b"] {
        let (status, body) = server.post("/lingxi/v1/sessions/parent/fork", &serde_json::json!({"newSessionId":child,"boundaryMessageId":format!("{run}-final")}).to_string()).await;
        assert_eq!(status, 200, "fork: {body}");
        let (status, body) = server
            .get(&format!("/lingxi/v1/sessions/{child}/files/{id}"))
            .await;
        assert_eq!(
            status, 200,
            "F04_C01 formal fork must inherit old sf identity: {body}"
        );
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            std::fs::read(value["file"]["realPath"].as_str().unwrap()).unwrap(),
            bytes
        );
    }
    let source_ids = server.branch_ids("parent").await;
    assert_eq!(server.branch_ids("child-a").await, source_ids);
    let (status,body)=server.post("/lingxi/v1/sessions/child-a/fork",&serde_json::json!({"newSessionId":"grandchild","boundaryMessageId":format!("{run}-final")}).to_string()).await;
    assert_eq!(status, 200, "第二次正式fork: {body}");
    assert_eq!(
        server
            .get(&format!("/lingxi/v1/sessions/grandchild/files/{id}"))
            .await
            .0,
        200
    );
    create_session(&server, "unrelated", "unrelated").await;
    assert_eq!(
        server
            .get(&format!("/lingxi/v1/sessions/unrelated/files/{id}"))
            .await
            .0,
        404,
        "伪造另一会话不能读附件"
    );
    let other = server.mint_device_token("user_other").await;
    let (status, _) = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/sessions/child-b/files/{id}"),
        &[("Authorization", &format!("Bearer {other}"))],
        None,
    )
    .await;
    assert_eq!(status, 403, "另一主体被正式权限闸拒绝");
    assert_eq!(
        server
            .post("/lingxi/v1/sessions/archive", r#"{"sessionId":"child-b"}"#)
            .await
            .0,
        200
    );
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/archived/delete",
            r#"{"sessionId":"child-b"}"#,
        )
        .await;
    assert_eq!(status, 200, "删除一个兄弟分支: {body}");
    for sid in ["parent", "child-a", "grandchild"] {
        let (status, body) = server
            .get(&format!("/lingxi/v1/sessions/{sid}/files/{id}"))
            .await;
        assert_eq!(status, 200, "删除兄弟后仍可读: {body}");
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            std::fs::read(value["file"]["realPath"].as_str().unwrap()).unwrap(),
            bytes
        );
    }
    println!("C01 formal parent+siblings+second fork bytes preserved; deleted sibling; session forgery404 subject403");
    server.stop_and_clean().await;
}

async fn upload(server: &TestServer, sid: &str, name: &str) -> serde_json::Value {
    let (status, body) = server
        .post(
            &format!("/lingxi/v1/sessions/{sid}/attachments/blob"),
            &serde_json::json!({"mime":"image/png","name":name,"dataBase64":"iVBORw0KGgo="})
                .to_string(),
        )
        .await;
    assert_eq!(status, 201, "upload: {body}");
    serde_json::from_str::<serde_json::Value>(&body).unwrap()["file"].clone()
}
async fn activity(server: &TestServer, sid: &str) -> u64 {
    server
        .state
        .storage()
        .query_one_text(
            "SELECT CAST(last_activity_unix_ms AS TEXT) FROM sessions WHERE session_id=?1",
            vec![sid.into()],
        )
        .await
        .unwrap()
        .unwrap()
        .parse()
        .unwrap()
}

#[tokio::test]
async fn f04_c02_real_activity_cleanup_and_clock_rollback() {
    let clock = Arc::new(lingxi_service::ManualClock::new(1_000_000));
    let server = start_server_at(
        synthetic_home("f04-c02"),
        ScriptedProvider::new(vec![ScriptStep::Final("delivered".into())]),
        clock.clone(),
    )
    .await;
    create_session(&server, "cache", "cold").await;
    let keep = upload(&server, "cache", "keep.png").await;
    let run = server
        .execute(
            "cache",
            &format!("[SessionFile] {{\"fileId\":{}}}", keep["fileId"]),
        )
        .await;
    let _same = upload(&server, "cache", "temp.png").await;
    // 内容寻址去重会复用同字节；因此临时缓存使用不同真实字节。
    let (status,body)=server.post("/lingxi/v1/sessions/cache/attachments/blob",&serde_json::json!({"mime":"image/png","name":"different.png","dataBase64":"iVBORw0KGgoBAg=="}).to_string()).await;
    assert_eq!(status, 201, "different upload: {body}");
    let temporary = serde_json::from_str::<serde_json::Value>(&body).unwrap()["file"].clone();
    assert_ne!(temporary["fileId"], keep["fileId"]);
    assert_eq!(activity(&server, "cache").await, 1_000_000);
    clock.advance(lingxi_service::sessionfiles::SESSION_FILE_CACHE_INACTIVE_TTL_MS + 1);
    let cleanup = server
        .state
        .session_files()
        .cleanup_cold_sessions()
        .await
        .unwrap();
    assert_eq!(cleanup.deleted, 1);
    assert_eq!(cleanup.expired, vec![temporary["fileId"].as_str().unwrap()]);
    assert!(!PathBuf::from(temporary["realPath"].as_str().unwrap()).exists());
    assert!(PathBuf::from(keep["realPath"].as_str().unwrap()).exists());
    let refs = server
        .state
        .session_files()
        .references_for_session("cache")
        .await
        .unwrap();
    assert!(refs
        .iter()
        .any(|r| r.message_id.as_deref() == Some(format!("user:{run}").as_str())));
    create_session(&server, "warm", "warm").await;
    let future = clock.now_unix_ms();
    let warm = upload(&server, "warm", "warm.png").await;
    clock.set(500);
    let _ = upload(&server, "warm", "retry.png").await;
    assert_eq!(
        activity(&server, "warm").await,
        future,
        "clock rollback must not regress real activity"
    );
    let cleanup = server
        .state
        .session_files()
        .cleanup_cold_sessions()
        .await
        .unwrap();
    assert_eq!(cleanup.deleted, 0);
    assert!(PathBuf::from(warm["realPath"].as_str().unwrap()).exists());
    println!("C02 real cache deletion=1; protected refs survive; real activity rollback monotonic {future}");
    server.stop_and_clean().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f04_c03_reference_commit_cleanup_window_is_atomic() {
    let clock = Arc::new(lingxi_service::ManualClock::new(1000));
    let server = Arc::new(
        start_server_at(
            synthetic_home("f04-c03"),
            ScriptedProvider::new(vec![ScriptStep::Final("unused".into())]),
            clock.clone(),
        )
        .await,
    );
    create_session(&server, "racing", "racing").await;
    let file = upload(&server, "racing", "a.png").await;
    create_session(&server, "committed", "committed").await;
    let committed = upload(&server, "committed", "protected.png").await;
    let protected_id = committed["fileId"].as_str().unwrap();
    let protected_run = server
        .execute(
            "committed",
            &format!("[SessionFile] {{\"fileId\":\"{protected_id}\"}}"),
        )
        .await;
    let protected_bytes = std::fs::read(committed["realPath"].as_str().unwrap()).unwrap();
    for child in ["protected-a", "protected-b"] {
        let (status,body)=server.post("/lingxi/v1/sessions/committed/fork",&serde_json::json!({"newSessionId":child,"boundaryMessageId":format!("{protected_run}-final")}).to_string()).await;
        assert_eq!(status, 200, "受保护分支: {body}");
    }
    assert_eq!(
        server
            .post(
                "/lingxi/v1/sessions/archive",
                r#"{"sessionId":"protected-a"}"#
            )
            .await
            .0,
        200
    );
    clock.advance(lingxi_service::sessionfiles::SESSION_FILE_CACHE_INACTIVE_TTL_MS + 1);
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered = std::sync::Mutex::new(Some(entered_tx));
    let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let release2 = release.clone();
    server
        .state
        .session_files()
        .set_cleanup_observer(Arc::new(move |_| {
            if let Some(tx) = entered.lock().unwrap().take() {
                let _ = tx.send(());
                let (lock, cv) = &*release2;
                let done = lock.lock().unwrap();
                let (_guard, timeout) = cv
                    .wait_timeout_while(done, Duration::from_secs(5), |done| !*done)
                    .unwrap();
                assert!(!timeout.timed_out(), "barrier must be released");
            }
        }));
    let files = server.state.session_files().clone();
    let cleanup = tokio::spawn(async move { files.cleanup_cold_sessions().await });
    entered_rx.await.unwrap();
    let peer = server.clone();
    let id = file["fileId"].as_str().unwrap().to_string();
    let (submitted_tx, submitted_rx) = tokio::sync::oneshot::channel();
    let submit = tokio::spawn(async move {
        let payload = serde_json::json!({"input":format!("[SessionFile] {{\"fileId\":\"{id}\"}}")})
            .to_string();
        http_written(
            &peer.addr,
            "POST",
            "/lingxi/v1/sessions/racing/execute",
            &[("Authorization", &format!("Bearer {}", peer.token))],
            Some(&payload),
            Some(submitted_tx),
        )
        .await
    });
    submitted_rx.await.unwrap();
    let peer = server.clone();
    let (delete_tx, delete_rx) = tokio::sync::oneshot::channel();
    let deleting = tokio::spawn(async move {
        http_written(
            &peer.addr,
            "POST",
            "/lingxi/v1/sessions/archived/delete",
            &[("Authorization", &format!("Bearer {}", peer.token))],
            Some(r#"{"sessionId":"protected-a"}"#),
            Some(delete_tx),
        )
        .await
    });
    delete_rx.await.unwrap();
    let peer = server.clone();
    let boundary = format!("{protected_run}-final");
    let (fork_tx, fork_rx) = tokio::sync::oneshot::channel();
    let forking = tokio::spawn(async move {
        http_written(
            &peer.addr,
            "POST",
            "/lingxi/v1/sessions/committed/fork",
            &[("Authorization", &format!("Bearer {}", peer.token))],
            Some(
                &serde_json::json!({"newSessionId":"protected-c","boundaryMessageId":boundary})
                    .to_string(),
            ),
            Some(fork_tx),
        )
        .await
    });
    fork_rx.await.unwrap();
    // 三个正式请求已经把完整输入写入真实TCP连接，清理仍停在检查之后。
    assert!(!submit.is_finished() && !deleting.is_finished() && !forking.is_finished());
    println!("C03 BARRIER after-check entered; actual execute/delete/fork TCP bodies written; queue still held; release next");
    {
        let (lock, cv) = &*release;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
    let outcome = cleanup.await.unwrap().unwrap();
    assert_eq!(outcome.deleted, 1);
    let (status, body) = submit.await.unwrap();
    assert_eq!(
        status, 500,
        "先完成清理后，正式提交必须明确拒绝不存在的附件: {body}"
    );
    assert!(body.contains("session_file_reference_missing"));
    assert_eq!(
        server.branch_ids("racing").await.len(),
        0,
        "引用失败不得留下消息"
    );
    assert!(server
        .state
        .session_files()
        .references_for_session("racing")
        .await
        .unwrap()
        .is_empty());
    let (status, body) = deleting.await.unwrap();
    assert_eq!(status, 200, "并发分支删除: {body}");
    let (status, body) = forking.await.unwrap();
    assert_eq!(status, 200, "并发新分支引用: {body}");
    for sid in ["committed", "protected-b", "protected-c"] {
        let (status, body) = server
            .get(&format!("/lingxi/v1/sessions/{sid}/files/{protected_id}"))
            .await;
        assert_eq!(status, 200, "竞态后合法引用仍可读: {body}");
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            std::fs::read(value["file"]["realPath"].as_str().unwrap()).unwrap(),
            protected_bytes
        );
    }
    let db = server.state.storage();
    for table in ["messages", "session_file_refs"] {
        let count = db
            .query_one_text(
                &format!("SELECT CAST(COUNT(*) AS TEXT) FROM {table} WHERE session_id=?1"),
                vec!["racing".into()],
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(count, "0", "失败引用的{table}必须零提交");
        println!("C03 SQL racing {table} count={count}");
    }
    let failed = db
        .query_one_text(
            "SELECT status || ':' || COALESCE(terminal_reason,'') FROM runs WHERE session_id=?1",
            vec!["racing".into()],
        )
        .await
        .unwrap()
        .unwrap();
    assert!(
        failed.starts_with("failed:"),
        "已承诺任务必须真实失败终态，不留伪成功或悬空运行: {failed}"
    );
    assert_eq!(
        failed, "failed:failed.storage_error",
        "沿原收尾终态词汇保存真实失败"
    );
    println!("C03 SQL racing retained honest failed run={failed}");
    let model_calls = db.query_one_text(
        "SELECT CAST(COUNT(*) AS TEXT) FROM key_events WHERE session_id=?1 AND event_type='model_call_started'",
        vec!["racing".into()],
    ).await.unwrap().unwrap();
    assert_eq!(model_calls, "0", "本地存储拒绝不能伪造供应商调用");
    let (status, body) = server.get("/lingxi/v1/sessions/racing/history?all=1").await;
    assert_eq!(status, 200);
    let history: serde_json::Value = serde_json::from_str(&body).unwrap();
    let terminals: Vec<_> = history["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "run_terminal")
        .collect();
    assert_eq!(terminals.len(), 1, "原统一历史只能显示一次真实终态");
    assert_eq!(terminals[0]["status"], "failed");
    assert_eq!(terminals[0]["terminalReason"], "failed.storage_error");
    assert!(!history["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "final_message"));

    assert_eq!(
        std::fs::read(committed["realPath"].as_str().unwrap()).unwrap(),
        protected_bytes,
        "引用先成功提交的载荷经过整个窗口仍保留"
    );
    assert_eq!(run_row(db, &protected_run).await.0, "completed");
    assert!(!server
        .state
        .session_files()
        .references_for_session("committed")
        .await
        .unwrap()
        .is_empty());
    println!("C03 protected committed ref/run/bytes survive; racing submit refused through existing non-retryable-storage500 boundary");
    println!("C03 deterministic after-check/before-delete queue window: stale reference rejected, no successful dangling ref, deletion happened");
    Arc::try_unwrap(server).ok().unwrap().stop_and_clean().await;
}

#[tokio::test]
async fn f04_c04_legacy_import_interrupt_missing_and_restart() {
    let home = synthetic_home("f04-c04");
    let clock = Arc::new(lingxi_service::ManualClock::new(1000));
    let server = start_server_at(
        home.clone(),
        ScriptedProvider::new(vec![ScriptStep::Final("new".into())]),
        clock.clone(),
    )
    .await;
    create_session(&server, "legacy", "legacy").await;
    let input = synthetic_home("legacy-source");
    std::fs::create_dir_all(&input).unwrap();
    let payload = input.join("old.txt");
    std::fs::write(&payload, b"original-old-bytes").unwrap();
    let sidecar = input.join("old.files.json");
    let raw = serde_json::json!({"version":1,"files":{"sf_0011223344556677":{"fileId":"sf_0011223344556677","realPath":payload,"filePath":payload,"status":"available","storageKind":"external","legacyFileIds":["sf_1111222233334444"]},"sf_8899aabbccddeeff":{"fileId":"sf_8899aabbccddeeff","realPath":input.join("missing.txt"),"status":"available"}},"refs":[{"fileId":"sf_0011223344556677"},{"fileId":"sf_8899aabbccddeeff"}]});
    std::fs::write(&sidecar, raw.to_string()).unwrap();
    let body = serde_json::json!({"sidecarPath":sidecar}).to_string();
    let (status, reply) = server
        .post("/lingxi/v1/sessions/legacy/files/import-legacy", &body)
        .await;
    assert_eq!(status, 200, "import: {reply}");
    let (status, reply) = server
        .get("/lingxi/v1/sessions/legacy/files/sf_1111222233334444")
        .await;
    assert_eq!(status, 200, "old alias: {reply}");
    assert_eq!(std::fs::read(&payload).unwrap(), b"original-old-bytes");
    let (status, reply) = server
        .get("/lingxi/v1/sessions/legacy/files/sf_8899aabbccddeeff")
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&reply).unwrap()["file"]["status"],
        "missing"
    );
    let dbpath = home.join("lingxi-service/data/runs.db");
    let db = rusqlite::Connection::open(&dbpath).unwrap();
    db.execute_batch("CREATE TRIGGER f04_interrupt BEFORE INSERT ON session_files WHEN NEW.origin='user_upload' BEGIN SELECT RAISE(ABORT,'synthetic register interrupt'); END;").unwrap();
    let (status, _) = server
        .post(
            "/lingxi/v1/sessions/legacy/attachments/blob",
            &serde_json::json!({"mime":"image/png","dataBase64":"iVBORw0KGgo="}).to_string(),
        )
        .await;
    assert_ne!(status, 201, "registry interruption cannot report delivery");
    assert_eq!(
        std::fs::read_dir(lingxi_service::sessionfiles::session_files_cache_dir(
            &home,
            Some("legacy"),
            None
        ))
        .unwrap()
        .count(),
        0,
        "failed blob registration must compensate owned bytes"
    );
    db.execute_batch("DROP TRIGGER f04_interrupt").unwrap();
    drop(db);
    let new = upload(&server, "legacy", "new.png").await;
    server
        .execute(
            "legacy",
            &format!("[SessionFile] {{\"fileId\":{}}}", new["fileId"]),
        )
        .await;
    let refs_before = server
        .state
        .session_files()
        .references_for_session("legacy")
        .await
        .unwrap()
        .len();
    server.stop.send(()).unwrap();
    server.handle.await.unwrap().unwrap();
    drop(server.state);
    let restarted =
        start_server_at(home.clone(), ScriptedProvider::new(vec![]), clock.clone()).await;
    for _ in 0..2 {
        let (status, reply) = restarted
            .post("/lingxi/v1/sessions/legacy/files/import-legacy", &body)
            .await;
        assert_eq!(status, 200, "repeat recovery: {reply}");
    }
    let refs_after = restarted
        .state
        .session_files()
        .references_for_session("legacy")
        .await
        .unwrap();
    assert_eq!(refs_after.len(), refs_before, "recovery must be idempotent");
    let list = restarted
        .state
        .session_files()
        .list_files("legacy")
        .await
        .unwrap();
    assert_eq!(list.len(), 3);
    assert!(list
        .iter()
        .any(|f| f.file_id == new["fileId"].as_str().unwrap()));
    assert!(PathBuf::from(new["realPath"].as_str().unwrap()).exists());
    println!("C04 imported old identity, missing marked honestly; DB interruption compensated bytes; restart repeated imports preserve exactly three identities and references");
    restarted.stop_and_clean().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f04_cancelled_read_retains_physical_lease_until_exit() {
    let clock = Arc::new(lingxi_service::ManualClock::new(1000));
    let server = start_server_at(
        synthetic_home("physical-read"),
        ScriptedProvider::new(vec![]),
        clock.clone(),
    )
    .await;
    create_session(&server, "reader", "reader").await;
    let file = upload(&server, "reader", "reader.png").await;
    let path = PathBuf::from(file["realPath"].as_str().unwrap());
    let expected = std::fs::read(&path).unwrap();
    clock.advance(lingxi_service::sessionfiles::SESSION_FILE_CACHE_INACTIVE_TTL_MS + 1);
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered = std::sync::Mutex::new(Some(entered_tx));
    let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let gate = release.clone();
    server
        .state
        .session_files()
        .set_payload_read_observer(Arc::new(move || {
            let _ = entered.lock().unwrap().take().unwrap().send(());
            let (lock, cv) = &*gate;
            let (_held, timeout) = cv
                .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(5), |v| !*v)
                .unwrap();
            assert!(!timeout.timed_out(), "物理读取线程屏障必须释放");
            println!(
                "PHYSICAL_READ barrier released; physical thread continues with retained lease"
            );
        }));
    let io = server.state.resource_io().clone();
    let id = file["fileId"].as_str().unwrap().to_string();
    let reader = tokio::spawn(async move {
        io.read(
            &serde_json::json!({"kind":"session-file","fileId":id,"sessionId":"reader"}),
            &lingxi_service::resourceio::OpContext::local_owner("reader"),
        )
        .await
    });
    entered_rx.await.unwrap();
    reader.abort();
    assert!(reader.await.unwrap_err().is_cancelled());
    let files = server.state.session_files().clone();
    let cleanup = tokio::spawn(async move { files.cleanup_cold_sessions().await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !cleanup.is_finished(),
        "Future取消后物理线程仍阻塞，清理不得越过正式租约"
    );
    assert_eq!(std::fs::read(&path).unwrap(), expected);
    {
        let (lock, cv) = &*release;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
    assert_eq!(cleanup.await.unwrap().unwrap().deleted, 1);
    assert!(!path.exists());
    println!("PHYSICAL_READ canceled future, payload retained through physical exit, then real cleanup deleted=1");
    server.stop_and_clean().await;
}

#[tokio::test]
async fn f04_legacy_plain_text_is_preserved_but_broken_file_objects_roll_back() {
    use lingxi_adapters::storage::session_tree::{append_message, AppendMessage};
    let server = start_server(
        "legacy-text",
        ScriptedProvider::new(vec![ScriptStep::Final("unused".into())]),
    )
    .await;
    create_session(&server, "legacy-text", "legacy-text").await;
    let content = r#"{"text":"needle in content"#;
    append_message(
        server.state.storage(),
        AppendMessage {
            session_id: "legacy-text",
            message_id: "plain",
            parent_message_id: None,
            run_id: None,
            role: "user",
            content_json: content,
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: 1000,
        },
    )
    .await
    .unwrap();
    let error = append_message(
        server.state.storage(),
        AppendMessage {
            session_id: "legacy-text",
            message_id: "broken",
            parent_message_id: Some("plain"),
            run_id: None,
            role: "user",
            content_json: r#"{"type":"session_file","fileId":"sf_bad"#,
            entry_type: "message",
            model_call_id: None,
            now_unix_ms: 1001,
        },
    )
    .await
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("malformed_session_file_reference"));
    let stored = server
        .state
        .storage()
        .query_one_text(
            "SELECT content_json FROM messages WHERE session_id=?1",
            vec!["legacy-text".into()],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored, content, "原纯文字按字节保留");
    assert_eq!(server.branch_ids("legacy-text").await, vec!["plain"]);
    assert!(server
        .state
        .session_files()
        .references_for_session("legacy-text")
        .await
        .unwrap()
        .is_empty());
    server.stop_and_clean().await;
}

/// 真实无消息失败的分支锚、条件读取和分页；原历史断言不变。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f04_failed_unmessaged_run_has_exact_branch_anchor_and_pagination() {
    let provider = ScriptedProvider::new(
        (0..8)
            .map(|i| ScriptStep::Final(format!("normal {i}")))
            .collect(),
    );
    let server = start_server("history-audits", provider.clone()).await;
    create_session(&server, "audit", "audit").await;
    create_session(&server, "empty", "empty").await;
    let bearer = server.bearer();
    let path = "/lingxi/v1/sessions/audit/history";
    let (status, body, etag) = http_full(
        &server.addr,
        "GET",
        path,
        &[("Authorization", &bearer)],
        None,
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["items"],
        serde_json::json!([])
    );
    let empty_etag = etag.unwrap();
    async fn fail(server: &TestServer) -> String {
        let (status, body) = server
            .post(
                "/lingxi/v1/sessions/audit/execute",
                r#"{"input":"[SessionFile] {\"fileId\":\"sf_0000000000000000\"}"}"#,
            )
            .await;
        assert_eq!(status, 500, "真实存储拒绝: {body}");
        server.state.storage().query_one_text("SELECT run_id FROM runs WHERE session_id='audit' AND status='failed' ORDER BY last_event_seq DESC LIMIT 1",vec![]).await.unwrap().unwrap()
    }
    let root_fail = fail(&server).await;
    assert!(provider.inputs.lock().unwrap().is_empty(), "拒绝未调用模型");
    let (status, body, etag) = http_full(
        &server.addr,
        "GET",
        path,
        &[("Authorization", &bearer), ("If-None-Match", &empty_etag)],
        None,
        None,
    )
    .await;
    assert_eq!(status, 200, "无消息终态必须令旧空历史ETag失效");
    let first_etag = etag.unwrap();
    assert_ne!(first_etag, empty_etag);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["items"].as_array().unwrap().len(), 1);
    assert_eq!(value["items"][0]["runId"], root_fail);
    assert_eq!(
        http_full(
            &server.addr,
            "GET",
            path,
            &[("Authorization", &bearer), ("If-None-Match", &first_etag)],
            None,
            None
        )
        .await
        .0,
        304
    );
    let q1 = server.execute("audit", "normal one").await;
    let anchored_fail = fail(&server).await;
    let q2 = server.execute("audit", "normal two").await;
    let q3 = server.execute("audit", "normal three").await;
    let removed_fail = fail(&server).await;
    let (status, body) = server.get("/lingxi/v1/sessions/audit/export").await;
    assert_eq!(status, 200);
    let export: serde_json::Value = serde_json::from_str(&body).unwrap();
    let mut before = None;
    let mut assembled = Vec::new();
    let mut pages = 0;
    loop {
        let page_path = match before {
            Some(ref cursor) => format!("{path}?limit=1&before={cursor}"),
            None => format!("{path}?limit=1"),
        };
        let (status, body) = server.get(&page_path).await;
        assert_eq!(status, 200);
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        let mut page = value["items"].as_array().unwrap().clone();
        page.append(&mut assembled);
        assembled = page;
        pages += 1;
        if value["page"]["hasMore"] == false {
            break;
        }
        before = Some(value["page"]["nextBefore"].as_str().unwrap().to_string());
        assert!(pages < 20);
    }
    assert_eq!(
        serde_json::Value::Array(assembled.clone()),
        export["items"],
        "逐页前插和全量导出严格一致"
    );
    assert!(pages >= 6);
    for id in [&root_fail, &anchored_fail, &removed_fail] {
        assert_eq!(
            assembled
                .iter()
                .filter(|item| item["kind"] == "run_terminal" && item["runId"] == *id)
                .count(),
            1,
            "每个失败只展示一次"
        );
    }
    let model_inputs = provider.inputs.lock().unwrap().clone();
    assert_eq!(model_inputs.len(), 3);
    for input in &model_inputs {
        assert!(!input.contains("run_terminal"));
        assert!(!input.contains("failed.storage_error"));
        for id in [&root_fail, &anchored_fail, &removed_fail] {
            assert!(!input.contains(id));
        }
    }
    println!("F04 AUDIT actual model inputs={model_inputs:?}; no failure audit inserted into model messages");
    let (status,body)=server.post("/lingxi/v1/sessions/audit/fork",&serde_json::json!({"newSessionId":"audit-fork","boundaryMessageId":format!("{q3}-final")}).to_string()).await;
    assert_eq!(status, 200, "{body}");
    let (_, body) = server
        .get("/lingxi/v1/sessions/audit-fork/history?all=1")
        .await;
    let fork: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(!fork["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["kind"] == "run_terminal"));
    for (name, target) in [
        ("keep", format!("{q2}-final")),
        ("root", format!("{q1}-final")),
    ] {
        let (status, body) = server
            .post(
                "/lingxi/v1/sessions/audit/checkpoints",
                &serde_json::json!({"name":name,"targetMessageId":target,"filePaths":[]})
                    .to_string(),
            )
            .await;
        assert_eq!(status, 200, "{body}");
    }
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/audit/rewind",
            r#"{"checkpoint":"keep","restoreFiles":false}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (_, body) = server.get("/lingxi/v1/sessions/audit/history?all=1").await;
    let rewound: serde_json::Value = serde_json::from_str(&body).unwrap();
    let items = rewound["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["runId"] == root_fail));
    assert!(items.iter().any(|i| i["runId"] == anchored_fail));
    assert!(
        !items.iter().any(|i| i["runId"] == removed_fail),
        "已离开分支链的真实锚不得串回"
    );
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/audit/rewind",
            r#"{"checkpoint":"root","restoreFiles":false}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let (_, body) = server.get("/lingxi/v1/sessions/audit/history?all=1").await;
    let reset: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        !reset["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "run_terminal"),
        "根重置不得回穿旧空根审计"
    );
    let reset_fail = fail(&server).await;
    let (_, body) = server.get("/lingxi/v1/sessions/audit/history?all=1").await;
    let after: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        after["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["kind"] == "run_terminal")
            .count(),
        1
    );
    assert!(
        after["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["runId"] == reset_fail),
        "根重置后的真实标记锚仍可展示"
    );
    let (_, body) = server.get("/lingxi/v1/sessions/empty/history?all=1").await;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["items"],
        serde_json::json!([])
    );
    let device = server.mint_device_token("foreign-user").await;
    assert_eq!(
        http(
            &server.addr,
            "GET",
            path,
            &[("Authorization", &format!("Bearer {device}"))],
            None
        )
        .await
        .0,
        403
    );
    println!("F04 AUDIT real SQL anchors={:?}",server.state.storage().query_one_text("SELECT json_group_array(json_object('run',run_id,'session',session_id,'anchor',anchor_message_id)) FROM run_history_anchors",vec![]).await.unwrap());
    server.stop_and_clean().await;
}

/// 删除一个正式会话必须回收其独有托管字节，而兄弟副本仍可读。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f04_permanent_delete_removes_owned_cache_and_retains_other_branch() {
    let server = start_server(
        "delete-cache",
        ScriptedProvider::new(vec![ScriptStep::Final("normal".into())]),
    )
    .await;
    create_session(&server, "delete-owner", "delete-owner").await;
    let file = upload(&server, "delete-owner", "only.png").await;
    let path = PathBuf::from(file["realPath"].as_str().unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let run = server
        .execute(
            "delete-owner",
            &format!(
                "[SessionFile] {{\"fileId\":\"{}\"}}",
                file["fileId"].as_str().unwrap()
            ),
        )
        .await;
    let (status,body)=server.post("/lingxi/v1/sessions/delete-owner/fork",&serde_json::json!({"newSessionId":"surviving","boundaryMessageId":format!("{run}-final")}).to_string()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        server
            .post(
                "/lingxi/v1/sessions/archive",
                r#"{"sessionId":"delete-owner","childMode":"detach_children"}"#
            )
            .await
            .0,
        200
    );
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/archived/delete",
            r#"{"sessionId":"delete-owner"}"#,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    println!(
        "F04 DELETE actual own bytes exists={} after formal removal path={}",
        path.exists(),
        path.display()
    );
    assert!(!path.exists(), "删除正式行后不得永久遗留独有托管载荷");
    let (status, body) = server
        .get(&format!(
            "/lingxi/v1/sessions/surviving/files/{}",
            file["fileId"].as_str().unwrap()
        ))
        .await;
    assert_eq!(status, 200, "{body}");
    let retained: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        std::fs::read(retained["file"]["realPath"].as_str().unwrap()).unwrap(),
        bytes
    );
    server.stop_and_clean().await;
}

/// 真正删除字节被拒绝时保留可恢复身份，重启从该事实继续；权威文件不追认为缓存。
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f04_delete_physical_failure_preserves_identity_until_restart_cleanup() {
    use std::os::unix::fs::PermissionsExt;
    let provider = ScriptedProvider::new(vec![]);
    let server = start_server("delete-failure", provider.clone()).await;
    create_session(&server, "delete-failure", "delete-failure").await;
    let file = upload(&server, "delete-failure", "remain.png").await;
    let path = PathBuf::from(file["realPath"].as_str().unwrap());
    let bytes = std::fs::read(&path).unwrap();
    let parent = path.parent().unwrap();
    let mode = std::fs::metadata(parent).unwrap().permissions().mode();
    let authoritative = server.home.join("authority.txt");
    std::fs::write(&authoritative, "AUTHORITATIVE-SENTINEL").unwrap();
    server
        .state
        .session_files()
        .register_file("delete-failure", &authoritative, None, None, "tool")
        .await
        .unwrap();
    assert_eq!(
        server
            .post(
                "/lingxi/v1/sessions/archive",
                r#"{"sessionId":"delete-failure"}"#
            )
            .await
            .0,
        200
    );
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/archived/delete",
            r#"{"sessionId":"delete-failure"}"#,
        )
        .await;
    // 在任何断言前还原自己夹具的权限，原始失败窗口与身份仍留待检查。
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(mode)).unwrap();
    assert_eq!(
        status, 500,
        "物理失败必须响亮且说明已删除会话、清扫仍待恢复: {body}"
    );
    assert!(body.contains("cleanup pending"));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        std::fs::read_to_string(&authoritative).unwrap(),
        "AUTHORITATIVE-SENTINEL"
    );
    let count=server.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM session_files WHERE file_id=?1 AND storage_kind='managed_cache' AND status='available'",vec![file["fileId"].as_str().unwrap().into()]).await.unwrap().unwrap();
    assert_eq!(count, "1", "物理失败仍持久保留完整托管身份");
    assert_eq!(
        server
            .state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM sessions WHERE session_id='delete-failure'",
                vec![]
            )
            .await
            .unwrap()
            .unwrap(),
        "0"
    );
    println!("F04 DELETE failure: actual500, DB session gone, owned byte+identity retained, authoritative untouched");
    let home = server.home.clone();
    server.stop_and_clean().await;
    let restarted = start_server_at(home, provider, Arc::new(lingxi_service::SystemClock)).await;
    restarted
        .state
        .session_files()
        .cleanup_cold_sessions()
        .await
        .unwrap();
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(&authoritative).unwrap(),
        "AUTHORITATIVE-SENTINEL"
    );
    assert_eq!(
        restarted
            .state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM session_files WHERE file_id=?1",
                vec![file["fileId"].as_str().unwrap().into()]
            )
            .await
            .unwrap()
            .unwrap(),
        "0"
    );
    println!(
        "F04 DELETE restart: actual orphan byte removed then identity cleared, authority preserved"
    );
    restarted.stop_and_clean().await;
}

/// 正式删除在物理读线程取消后仍等待该线程退出，不抽走已授权的字节。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f04_permanent_delete_waits_cancelled_physical_reader() {
    let server = start_server("delete-reader", ScriptedProvider::new(vec![])).await;
    create_session(&server, "delete-reader", "delete-reader").await;
    let file = upload(&server, "delete-reader", "reader.png").await;
    let path = PathBuf::from(file["realPath"].as_str().unwrap());
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        server
            .post(
                "/lingxi/v1/sessions/archive",
                r#"{"sessionId":"delete-reader"}"#
            )
            .await
            .0,
        200
    );
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered = std::sync::Mutex::new(Some(entered_tx));
    let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let physical_gate = release.clone();
    server.state.session_files().set_payload_read_observer(Arc::new(move || {
        let _=entered.lock().unwrap().take().unwrap().send(());
        let (lock,cv)=&*physical_gate;
        let (_held,timeout)=cv.wait_timeout_while(lock.lock().unwrap(),Duration::from_secs(5),|v|!*v).unwrap();
        assert!(!timeout.timed_out());println!("F04 DELETE physical reader released, retained lease still held until actual thread return");
    }));
    let io = server.state.resource_io().clone();
    let id = file["fileId"].as_str().unwrap().to_owned();
    let reader = tokio::spawn(async move {
        io.read(
            &serde_json::json!({"kind":"session-file","fileId":id,"sessionId":"delete-reader"}),
            &lingxi_service::resourceio::OpContext::local_owner("delete-reader"),
        )
        .await
    });
    entered_rx.await.unwrap();
    reader.abort();
    assert!(reader.await.unwrap_err().is_cancelled());
    let addr = server.addr;
    let bearer = server.bearer();
    let deletion = tokio::spawn(async move {
        http(
            &addr,
            "POST",
            "/lingxi/v1/sessions/archived/delete",
            &[("Authorization", &bearer)],
            Some(r#"{"sessionId":"delete-reader"}"#),
        )
        .await
    });
    let mut committed = false;
    for _ in 0..100 {
        if server
            .state
            .storage()
            .query_one_text(
                "SELECT CAST(COUNT(*) AS TEXT) FROM sessions WHERE session_id='delete-reader'",
                vec![],
            )
            .await
            .unwrap()
            .unwrap()
            == "0"
        {
            committed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(committed, "真实会话删除已提交，不能靠未执行请求证明租约");
    assert!(!deletion.is_finished());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(server.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM session_files WHERE owner_session_id='delete-reader'",vec![]).await.unwrap().unwrap(),"1");
    {
        let (lock, cv) = &*release;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
    let (status, body) = deletion.await.unwrap();
    assert_eq!(status, 200, "{body}");
    assert!(!path.exists());
    assert_eq!(server.state.storage().query_one_text("SELECT CAST(COUNT(*) AS TEXT) FROM session_files WHERE owner_session_id='delete-reader'",vec![]).await.unwrap().unwrap(),"0");
    println!("F04 DELETE actual DB commit before cleanup; canceled read physical lease prevented deletion until thread exit; byte and identity then removed");
    server.stop_and_clean().await;
}
