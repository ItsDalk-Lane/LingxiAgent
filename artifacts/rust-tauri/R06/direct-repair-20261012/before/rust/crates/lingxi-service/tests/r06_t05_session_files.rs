//! R06-T05 service 层生产链测试：会话文件注册、身份公式、旧 sidecar 导入、
//! fork、引用检查与缓存清理。
//!
//! 验收覆盖：
//! - R06-A09：旧附件（现役 sidecar 里的 sf_ 身份 + 原文件）导入后经全部
//!   客户端读路径可读；身份↔文件对应正确、无断链；越权主体被拒。
//! - R06-A10：附件已交付并被历史引用后，缓存清理保证权威文件可用、
//!   临时副本按策略回收。
//!
//! 对照纪律（RC-3）：
//! - sf_ 身份公式金样：由现役 `lib/session-files/session-file-registry.ts`
//!   （buildSessionFileId :867-874 / sessionFileOwnerKey :876-888 /
//!   buildSessionFileSourceKey :24-33 / sessionFilesCacheDir :17-22）在
//!   node 运行时实算产出，逐字节嵌入本文件。
//! - 安全闸：`shared/file-import-security.ts`（inspectLocalImportPath）
//!   与 `shared/path-security.ts`（SENSITIVE_DIRS + lingxiHome 封锁）。
//! - 清理语义：cleanupColdSessionFiles（session-file-registry.ts:517-571），
//!   冷度来源映射为 sessions.last_activity_unix_ms（差异台账 D3）。
//! - 上传白名单/限量：`shared/{image,audio,video}-mime.ts` 与
//!   `server/routes/upload.ts`（sanitizeBlobName/uniqueUploadName）。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::sessionfiles::{
    build_session_file_id, build_session_file_source_key, session_file_owner_key,
    session_files_cache_dir, InspectError, SessionFileService, SESSION_FILE_CACHE_INACTIVE_TTL_MS,
};
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── 身份公式金样（node 实算，见 01_design 与 07_diff_ledger D2） ──

#[test]
fn sf_identity_formulas_match_incumbent_golden() {
    // ownerKey：sessionId 优先（trim 后非空），否则 path:。
    assert_eq!(
        session_file_owner_key(Some("session-alpha"), None),
        "id:session-alpha"
    );
    assert_eq!(
        session_file_owner_key(Some("  session-alpha  "), None),
        "id:session-alpha"
    );
    assert_eq!(
        session_file_owner_key(None, Some("/tmp/sessions/s-beta.jsonl")),
        "path:/tmp/sessions/s-beta.jsonl"
    );
    assert_eq!(
        session_file_owner_key(Some("   "), Some("/tmp/sessions/s-beta.jsonl")),
        "path:/tmp/sessions/s-beta.jsonl"
    );

    // sourceKey：ns 清洗 + sha256(JSON.stringify(parts.map(String)))。
    let source_key = build_session_file_source_key(
        "upload:path:v1",
        &["/tmp/real/a.txt", "file", "42", "1700000000000"],
    );
    assert_eq!(
        source_key,
        "upload:path:v1:c7ad4cc7af5b3e96fc961c632bb8fab25300e070672b17f2aaf8c724edad3c4d"
    );
    // ns 非法字符逐字符替换为 _（含 CJK），并截断到 80。
    let cleaned = build_session_file_source_key("up load/路径!@#", &["x"]);
    assert_eq!(
        cleaned,
        "up_load______:cd65ea2c2ad99e94a85b1b6df72efef9cb2ed0ae933a60c32ce16317f7d7d6aa"
    );

    // file id：sf_ + sha256(JSON.stringify([ownerKey, sourceKey || identityKey]))[..16]。
    assert_eq!(
        build_session_file_id("id:session-alpha", Some(&source_key), "/tmp/real/a.txt"),
        "sf_138f5e7f5bf163d2"
    );
    assert_eq!(
        build_session_file_id("path:/tmp/sessions/s-beta.jsonl", None, "/tmp/real/b.txt"),
        "sf_f096490da3199f23"
    );

    // 缓存目录：{home}/session-files/{sha256(ownerKey)[..24]}。
    let dir = session_files_cache_dir(Path::new("/home"), Some("session-alpha"), None);
    assert_eq!(
        dir,
        PathBuf::from("/home/session-files/9f59565e2640fb6fdab340ae")
    );
}

// ── service 级测试夹具（真 SQLite + 真临时文件，不起 HTTP） ──

struct ServiceFixture {
    home: PathBuf,
    service: SessionFileService,
    /// 随夹具持有库句柄（service 自持 Arc；此字段钉住归属，备断言
    /// 直查）。当前套件未直查 → allow(dead_code)。
    #[allow(dead_code)]
    db: Arc<lingxi_adapters::storage::RunDatabase>,
}

fn fresh_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r06t05-sf-{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir home");
    dir
}

async fn fixture(tag: &str, now_ms: u64) -> ServiceFixture {
    let home = fresh_home(tag);
    let db_path = home.join("runs.db");
    let db = lingxi_adapters::storage::RunDatabase::open(
        &db_path,
        lingxi_adapters::storage::StoreOptions::default(),
    )
    .await
    .expect("open db");
    let db = Arc::new(db);
    let service = SessionFileService::new(home.clone(), db.clone(), move || now_ms);
    ServiceFixture { home, service, db }
}

impl ServiceFixture {
    fn write_file(&self, rel: &str, content: &[u8]) -> PathBuf {
        let path = self.home.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir parent");
        std::fs::write(&path, content).expect("write file");
        path
    }
}

impl Drop for ServiceFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

#[tokio::test]
async fn register_file_dedups_by_source_key_and_real_path() {
    let fx = fixture("dedup", 1_000).await;
    let real = fx.write_file("real/a.txt", b"hello");
    let first = fx
        .service
        .register_file(
            "s1",
            &real,
            Some("upload:path:v1:k"),
            Some("lbl"),
            "user_upload",
        )
        .await
        .expect("first register");
    assert!(first.file_id.starts_with("sf_"));
    assert_eq!(first.mime, "text/plain");
    assert_eq!(first.size_bytes, 5);
    // 现役 registerFile（registry :74-165）：同 owner 同 sourceKey 命中既有行。
    let again = fx
        .service
        .register_file("s1", &real, Some("upload:path:v1:k"), None, "user_upload")
        .await
        .expect("dedup register");
    assert_eq!(again.file_id, first.file_id);
    // 同 owner 同 realPath（无 sourceKey）也命中既有行。
    let by_path = fx
        .service
        .register_file("s1", &real, None, None, "user_upload")
        .await
        .expect("dedup by path");
    assert_eq!(by_path.file_id, first.file_id);
}

#[tokio::test]
async fn inspect_local_import_enforces_incumbent_security_gate() {
    let fx = fixture("gate", 1_000).await;
    let real = fx.write_file("real/a.txt", b"x");
    // 相对路径 → PATH_INVALID。
    let err = fx
        .service
        .inspect_local_import("relative/a.txt")
        .unwrap_err();
    assert_eq!(err, InspectError::PathInvalid);
    // 不存在 → NOT_FOUND。
    let missing = fx.home.join("nope.txt");
    let err = fx
        .service
        .inspect_local_import(missing.to_str().unwrap())
        .unwrap_err();
    assert_eq!(err, InspectError::NotFound);
    // 符号链接 → SYMLINK。
    let link = fx.home.join("link.txt");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let err = fx
        .service
        .inspect_local_import(link.to_str().unwrap())
        .unwrap_err();
    assert_eq!(err, InspectError::Symlink);
    // lingxiHome 内部路径 → PATH_BLOCKED（现役 isSensitivePath 封锁 lingxiHome）。
    let inside = fx.write_file("inner/secret.txt", b"s");
    let err = fx
        .service
        .inspect_local_import_with_home(inside.to_str().unwrap(), Some(&fx.home))
        .unwrap_err();
    assert_eq!(err, InspectError::PathBlocked);
    // 正常文件 → ok。
    let inspected = fx
        .service
        .inspect_local_import(real.to_str().unwrap())
        .expect("inspect ok");
    assert_eq!(inspected.kind, "file");
}

#[tokio::test]
async fn a09_import_legacy_sidecar_preserves_ids_and_readability() {
    let fx = fixture("a09", 1_000).await;
    // 现役 sidecar version 1：{version, files:{id:row}, refs:[...]}。
    let payload = fx.write_file(
        "legacy/pic.png",
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    );
    let sidecar = fx.home.join("legacy-session.jsonl.files.json");
    std::fs::write(
        &sidecar,
        serde_json::json!({
            "version": 1,
            "files": {
                "sf_legacylegacy01": {
                    "fileId": "sf_legacylegacy01",
                    "filePath": "/shown/pic.png",
                    "realPath": payload.to_str().unwrap(),
                    "storageKind": "external",
                    "status": "available",
                    "label": "pic",
                    "filename": "pic.png",
                    "mime": "image/png",
                    "size": 8,
                    "mtimeMs": 1700000000000u64,
                    "isDirectory": false,
                    "fileKind": "image",
                    "origin": "user_upload",
                    "registeredAt": 1000,
                    "updatedAt": 1000,
                    "legacyFileIds": ["sf_evenolder00000"],
                    "legacyFilePaths": ["/old/pic.png"]
                }
            },
            "refs": [
                {"fileId": "sf_legacylegacy01", "messageId": "m-old", "kind": "message_marker", "createdAt": 900}
            ]
        })
        .to_string(),
    )
    .expect("write sidecar");
    let report = fx
        .service
        .import_legacy_sidecar("s-new", &sidecar)
        .await
        .expect("import");
    assert_eq!(report.imported, 1);
    assert_eq!(report.skipped, 0);
    // 身份保持：旧 sf_ id 原样可读（A09 身份↔文件对应）。
    let row = fx
        .service
        .get_file("s-new", "sf_legacylegacy01")
        .await
        .expect("get")
        .expect("present");
    assert_eq!(row.mime, "image/png");
    assert_eq!(row.real_path, payload.to_str().unwrap());
    // 旧 legacyFileIds 也登记为 alias，可读。
    let via_alias = fx
        .service
        .get_file("s-new", "sf_evenolder00000")
        .await
        .expect("alias get")
        .expect("present");
    assert_eq!(via_alias.file_id, "sf_legacylegacy01");
    // 断链检测：realPath 真实存在 → 无 broken；删掉文件后完整性报告必须报断链。
    assert!(fx
        .service
        .integrity_report("s-new")
        .await
        .expect("report")
        .broken
        .is_empty());
    std::fs::remove_file(&payload).expect("remove payload");
    let broken = fx
        .service
        .integrity_report("s-new")
        .await
        .expect("report")
        .broken;
    assert_eq!(broken, vec!["sf_legacylegacy01".to_string()]);
}

#[tokio::test]
async fn fork_session_files_rewrites_ids_and_keeps_legacy_aliases() {
    let fx = fixture("fork", 1_000).await;
    let real = fx.write_file("real/doc.md", b"# hi");
    let source = fx
        .service
        .register_file(
            "s-src",
            &real,
            Some("upload:path:v1:doc"),
            None,
            "user_upload",
        )
        .await
        .expect("register");
    // 现役 forkSessionFiles（registry :255-360）只复制「保留引用可达」的文件；
    // retained 身份集显式传入（来源是会话 fork 点之前的历史条目）。
    let forked = fx
        .service
        .fork_session_files("s-src", "s-dst", std::slice::from_ref(&source.file_id))
        .await
        .expect("fork");
    assert_eq!(forked.len(), 1);
    let new_id = &forked[0];
    assert_ne!(new_id, &source.file_id);
    assert!(new_id.starts_with("sf_"));
    // retained 之外的文件不可达 → 不复制。
    let other = fx
        .service
        .register_file(
            "s-src",
            &fx.write_file("real/other.md", b"x"),
            None,
            None,
            "user_upload",
        )
        .await
        .expect("register other");
    let forked2 = fx
        .service
        .fork_session_files("s-src", "s-dst2", std::slice::from_ref(&source.file_id))
        .await
        .expect("fork2");
    assert_eq!(
        forked2.len(),
        1,
        "unreachable file must not fork: {other:?}"
    );
    // 旧 id 在新会话经 alias 仍可解析到 fork 行（现役 legacyFileIds 累积语义）。
    let resolved = fx
        .service
        .get_file("s-dst", &source.file_id)
        .await
        .expect("alias resolve")
        .expect("present");
    assert_eq!(&resolved.file_id, new_id);
    assert!(resolved.legacy_file_ids.contains(&source.file_id));
}

#[tokio::test]
async fn a10_cleanup_keeps_delivered_external_and_cleans_cold_managed() {
    let now = 100 * 60 * 60 * 1000u64; // 100h 起点
    let fx = fixture("a10", now).await;
    // managed：blob 上传落在会话缓存目录。
    let managed = fx
        .service
        .register_blob(
            "s-cold",
            Some("voice.wav"),
            "audio/wav",
            b"RIFFdata",
            "voice_input",
        )
        .await
        .expect("register blob");
    assert_eq!(managed.storage_kind, "managed_cache");
    assert!(Path::new(&managed.real_path).exists());
    // external：本地路径注册，不复制。
    let real = fx.write_file("real/keep.txt", b"keep");
    let external = fx
        .service
        .register_file("s-cold", &real, None, None, "user_upload")
        .await
        .expect("register external");
    assert_eq!(external.storage_kind, "external");
    // 把会话标记为冷（last_activity 超过 72h TTL），再扫。
    fx.service
        .mark_session_activity("s-cold", now - SESSION_FILE_CACHE_INACTIVE_TTL_MS - 1)
        .await
        .expect("mark activity");
    let outcome = fx.service.cleanup_cold_sessions().await.expect("cleanup");
    assert_eq!(outcome.expired, vec![managed.file_id.clone()]);
    // 冷会话的 managed 缓存已回收：行转 expired，目录删除。
    let after = fx
        .service
        .get_file("s-cold", &managed.file_id)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(after.status, "expired");
    assert!(!Path::new(&managed.real_path).exists());
    // external 权威文件绝不被清理（A10 红线）。
    let ext_after = fx
        .service
        .get_file("s-cold", &external.file_id)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(ext_after.status, "available");
    assert!(Path::new(&real).exists());
}

#[tokio::test]
async fn cleanup_skips_cold_managed_file_still_referenced_by_live_session() {
    let now = 100 * 60 * 60 * 1000u64;
    let fx = fixture("a10-ref", now).await;
    // fork 共享同一 payload 的场景：s-live 的 ref 指向 s-cold 的 managed 文件。
    let managed = fx
        .service
        .register_blob(
            "s-cold",
            None,
            "image/png",
            &[0x89, 0x50, 0x4E, 0x47],
            "user_upload",
        )
        .await
        .expect("register blob");
    fx.service
        .insert_reference("s-live", &managed.file_id, Some("m-live"), "message_marker")
        .await
        .expect("insert ref");
    fx.service
        .mark_session_activity("s-cold", now - SESSION_FILE_CACHE_INACTIVE_TTL_MS - 1)
        .await
        .expect("mark cold");
    fx.service
        .mark_session_activity("s-live", now)
        .await
        .expect("mark live");
    let outcome = fx.service.cleanup_cold_sessions().await.expect("cleanup");
    // 任务书第 4 条：被存活会话引用的交付物不得被闲置清理删除。
    assert!(
        outcome.expired.is_empty(),
        "referenced file must survive: {outcome:?}"
    );
    assert!(Path::new(&managed.real_path).exists());
    let row = fx
        .service
        .get_file("s-cold", &managed.file_id)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(row.status, "available");
}

#[tokio::test]
async fn cleanup_never_touches_warm_sessions() {
    let now = 100 * 60 * 60 * 1000u64;
    let fx = fixture("a10-warm", now).await;
    let managed = fx
        .service
        .register_blob(
            "s-warm",
            None,
            "image/png",
            &[0x89, 0x50, 0x4E, 0x47],
            "user_upload",
        )
        .await
        .expect("register blob");
    fx.service
        .mark_session_activity("s-warm", now)
        .await
        .expect("mark warm");
    let outcome = fx.service.cleanup_cold_sessions().await.expect("cleanup");
    assert!(outcome.expired.is_empty());
    assert!(Path::new(&managed.real_path).exists());
}

#[tokio::test]
async fn bootstrap_cleanup_reaps_cold_managed_payloads_once_at_startup() {
    // D14：现役 server/index.ts:563-570 启动清扫一次的候选人接线。
    // 第一轮（模拟上一进程）：在 bootstrap 将打开的同一个库文件里造
    // 冷会话 + managed 载荷，关连接；第二轮 bootstrap 应回收载荷。
    let home = fresh_home("startup-cleanup");
    let layout = prepare_layout(&home).expect("prepare layout");
    let data_dir = layout.runtime_dir.join("data");
    std::fs::create_dir_all(&data_dir).expect("mkdir data");
    let db_path = data_dir.join(lingxi_adapters::storage::RUNS_DB_FILE_NAME);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let (payload_path, file_id);
    {
        let db = Arc::new(
            lingxi_adapters::storage::RunDatabase::open(
                &db_path,
                lingxi_adapters::storage::StoreOptions::default(),
            )
            .await
            .expect("open db"),
        );
        let service = SessionFileService::new(home.clone(), db.clone(), move || now);
        let blob = service
            .register_blob(
                "s-cold",
                None,
                "image/png",
                &[0x89, 0x50, 0x4E, 0x47],
                "user_upload",
            )
            .await
            .expect("register blob");
        assert_eq!(blob.storage_kind, "managed_cache");
        payload_path = blob.real_path.clone();
        file_id = blob.file_id.clone();
        assert!(Path::new(&payload_path).exists());
        // 冷化（超过 72h TTL）。
        lingxi_adapters::storage::session_files::upsert_session_activity(
            &db,
            "s-cold",
            now - SESSION_FILE_CACHE_INACTIVE_TTL_MS - 60_000,
        )
        .await
        .expect("cold mark");
        db.close().await.expect("close first-process db");
    }
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, ServiceDeps::default())
        .await
        .expect("bootstrap");
    // 清扫在 spawn 里：轮询载荷消失（上限 5s）。
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while Path::new(&payload_path).exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "startup cleanup did not reap the cold managed payload"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // 行状态 expired（经同一 state 的注册表全局读）。
    let view = state
        .session_files()
        .get_file_global(&file_id)
        .await
        .expect("lookup")
        .expect("row retained");
    assert_eq!(view.status, "expired");
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn reference_checker_collects_marker_identities_and_reports_broken() {
    let fx = fixture("refs", 1_000).await;
    let real = fx.write_file("real/pic.png", &[0x89, 0x50, 0x4E, 0x47]);
    let registered = fx
        .service
        .register_file("s1", &real, None, None, "user_upload")
        .await
        .expect("register");
    // 现役标记格式：SESSION_FILE_MARKER_RE / ATTACHED_MEDIA_MARKER_RE / 类型化对象
    // （collectSessionFileReferenceIdentities，registry :997-1021）。纯文本只认
    // 两种带字面量前缀的标记；类型化对象以结构化值出现时才收集。
    let messages = vec![
        serde_json::Value::String(format!(
            "看看这个 [SessionFile] {{\"fileId\":\"{}\"}} 还有 [attached_image: /shown/other.png]",
            registered.file_id
        )),
        serde_json::json!({"type": "session_file", "fileId": "sf_ghostghostghos"}),
    ];
    let found = fx
        .service
        .collect_reference_identities("s1", &messages)
        .await
        .expect("collect");
    assert!(found.identities.contains(&registered.file_id));
    assert!(found.identities.contains(&"sf_ghostghostghos".to_string()));
    assert!(found.identities.contains(&"/shown/other.png".to_string()));
    // 未注册身份 → broken 报告必须点名，不得静默吞掉。
    assert_eq!(found.broken.len(), 2);
    assert!(found.broken.contains(&"sf_ghostghostghos".to_string()));
    assert!(found.broken.contains(&"/shown/other.png".to_string()));
    // 已注册的引用落成 refs 行（供 A10 交叉会话保护消费）。
    let refs = fx.service.references_for_session("s1").await.expect("refs");
    assert!(refs.iter().any(|r| r.file_id == registered.file_id));
}

// ── 路由级测试（真 TCP + 真 HTTP + 真 SQLite，与 r06_t04 同构） ──

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn start_server(tag: &str) -> TestServer {
    let home = fresh_home(tag);
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, ServiceDeps::default())
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
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
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!("readiness: {err}; service result: {:?}", handle.await),
    };
    TestServer {
        addr,
        home,
        token,
        stop: stop_tx,
        handle,
    }
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
        let _ = std::fs::remove_dir_all(&self.home);
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
        .into_status_body()
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
        .into_status_body()
    }

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

struct HttpResponse {
    status: u16,
    #[allow(dead_code)] // 统一 HTTP 夹具形状；本套件只断言状态与体。
    headers: Vec<(String, String)>,
    body: String,
}

impl HttpResponse {
    fn into_status_body(self) -> (u16, String) {
        (self.status, self.body)
    }
}

async fn http(
    addr: &SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> HttpResponse {
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
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let headers = head
        .lines()
        .skip(1)
        .filter_map(|line| {
            line.split_once(':')
                .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        })
        .collect();
    HttpResponse {
        status,
        headers,
        body: body.to_string(),
    }
}

async fn create_session(server: &TestServer, session_id: &str) {
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({
                "sessionId": session_id,
                "agentId": "agent",
                "title": "t",
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create session {session_id}: {body}");
}

#[tokio::test]
async fn attachments_routes_register_list_and_enforce_ownership() {
    let server = start_server("routes").await;
    create_session(&server, "s-http").await;
    // 本地路径附件：安全闸通过 → 201 + sf_ 身份。payload 必须在
    // lingxiHome（server.home）之外——安全闸封锁 home 内部路径。
    let outside = fresh_home("routes-payload");
    let payload = outside.join("upload.txt");
    std::fs::write(&payload, b"route-body").expect("write payload");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s-http/attachments/local",
            &serde_json::json!({ "path": payload.to_str().unwrap() }).to_string(),
        )
        .await;
    assert_eq!(status, 201, "attach local: {body}");
    let file_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(file_id.starts_with("sf_"));
    // 列表读路径（A09 读路径之一）。
    let (status, body) = server.get("/lingxi/v1/sessions/s-http/files").await;
    assert_eq!(status, 200, "list files: {body}");
    let files = serde_json::from_str::<serde_json::Value>(&body).unwrap()["files"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["fileId"], file_id);
    // 单件读路径（alias 感知）。
    let (status, body) = server
        .get(&format!("/lingxi/v1/sessions/s-http/files/{file_id}"))
        .await;
    assert_eq!(status, 200, "get file: {body}");
    // 越权：别的用户的设备主体 → 403（现役 can_access 语义）。
    let intruder = server.mint_device_token("user_intruder").await;
    let (status, _) = http(
        &server.addr,
        "GET",
        "/lingxi/v1/sessions/s-http/files",
        &[("Authorization", &format!("Bearer {intruder}"))],
        None,
    )
    .await
    .into_status_body();
    assert_eq!(status, 403, "cross-principal must be refused");
    let _ = std::fs::remove_dir_all(&outside);
    server.stop_and_clean().await;
}

#[tokio::test]
async fn attachments_local_route_maps_security_gate_errors() {
    let server = start_server("gate-http").await;
    create_session(&server, "s-gate").await;
    // 相对路径 → 400 PATH_INVALID。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s-gate/attachments/local",
            &serde_json::json!({ "path": "relative.txt" }).to_string(),
        )
        .await;
    assert_eq!(status, 400, "relative path: {body}");
    assert!(body.contains("PATH_INVALID"));
    // 不存在 → 404。
    let (status, _) = server
        .post(
            "/lingxi/v1/sessions/s-gate/attachments/local",
            &serde_json::json!({ "path": "/definitely/not/here.bin" }).to_string(),
        )
        .await;
    assert_eq!(status, 404);
    // lingxiHome 内部 → 403 PATH_BLOCKED（server.home 即 lingxiHome）。
    let inside = server.home.join("secret.txt");
    std::fs::write(&inside, b"s").expect("write inside");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s-gate/attachments/local",
            &serde_json::json!({ "path": inside.to_str().unwrap() }).to_string(),
        )
        .await;
    assert_eq!(status, 403, "blocked path: {body}");
    assert!(body.contains("PATH_BLOCKED"));
    server.stop_and_clean().await;
}

#[tokio::test]
async fn attachments_blob_route_enforces_whitelist_limits_and_registers_managed() {
    let server = start_server("blob-http").await;
    create_session(&server, "s-blob").await;
    // 白名单外 mime → 415。
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s-blob/attachments/blob",
            &serde_json::json!({
                "name": "evil.exe",
                "mime": "application/x-msdownload",
                "dataBase64": "TVo="
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 415, "mime whitelist: {body}");
    // 合法图片 → 201 + managed_cache。
    let png_b64 = base64_encode(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s-blob/attachments/blob",
            &serde_json::json!({
                "name": "../pasted.png",
                "mime": "image/png",
                "dataBase64": png_b64
            })
            .to_string(),
        )
        .await;
    assert_eq!(status, 201, "blob attach: {body}");
    let parsed = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let file = &parsed["file"];
    assert_eq!(file["storageKind"], "managed_cache");
    // 文件名经 sanitizeBlobName 清洗（../ 剥除）。
    let filename = file["filename"].as_str().unwrap();
    assert!(
        !filename.contains("..") && !filename.contains('/'),
        "sanitized: {filename}"
    );
    server.stop_and_clean().await;
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
