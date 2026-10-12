//! R06-T05 Round 5 生产链测试：文件历史（叶 #11 + 叶面断言 #19-21）。
//!
//! 对照纪律（RC-3，逐条亲读）：
//! - `lib/file-history/history-store.ts`：recordSnapshot 合并窗
//!   （:84-116：同 sha → unchanged；<60s 且双侧非 restore → UPDATE
//!   merged；否则 INSERT）、listFiles ORDER BY lastCapturedAt DESC
//!   （:144-151）、listVersions ORDER BY capturedAt DESC,id DESC
//!   （:153-160）、getSnapshotContent 不存在抛
//!   "file-history snapshot N not found"（:162-169）。
//! - `lib/file-history/file-history-service.ts`：workspaceHashForRoot =
//!   sha256(resolve 后斜杠归一)[..16]（:19-22）；_capture 的准入闸
//!   （:227-241：isFile、MAX_SNAPSHOT_BYTES 5MB、策略函数）。
//! - `lib/file-history/text-file-policy.ts`：扩展名/文件名/噪音目录
//!   三张表逐行落（:4-62）。
//! - `server/routes/file-history.ts`：agentId 必填 400（:23/:74）、
//!   workspace not tracked 404（:26/:77）、invalid relPath/id/
//!   snapshotId 400（:45/:57/:79）、snapshot 404（:66/:91）、
//!   restore = ResourceIO 写回 + captureNow "restore"（:86-89）。
//! - 设计登记（07_diff_ledger）：候选人单工作区单库（v9 表，无
//!   gzip/op_context/deleted_at）；D8 接线 = ResourceIO 写路径在
//!   emit 后同步捕获（origin 默认 "event"，restore 路由传 "restore"）；
//!   D12 延展：query 未知键/重复键 → 400。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::filehistory::{
    is_ignored_rel_path, is_tracked_file, workspace_hash_for_root, FileHistoryService,
    RecordOutcome, MAX_SNAPSHOT_BYTES, MERGE_WINDOW_MS,
};
use lingxi_service::resourceaccess::ResourceAccess;
use lingxi_service::resourceio::{OpContext, ResourceIoService};
use lingxi_service::resources::ResourceService;
use lingxi_service::sessionfiles::SessionFileService;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── 服务级夹具（手动时钟，不起 HTTP） ──

struct FhFixture {
    home: PathBuf,
    workspace: PathBuf,
    now: Arc<std::sync::Mutex<u64>>,
    fh: Arc<FileHistoryService>,
    db: Arc<lingxi_adapters::storage::RunDatabase>,
}

impl Drop for FhFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
        let _ = std::fs::remove_dir_all(&self.workspace);
    }
}

async fn fh_fixture(tag: &str, now_ms: u64) -> FhFixture {
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06t05-fh-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06t05-fh-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    let home = home.canonicalize().expect("canonical home");
    let workspace = workspace.canonicalize().expect("canonical workspace");
    let db = Arc::new(
        lingxi_adapters::storage::RunDatabase::open(
            &home.join("runs.db"),
            lingxi_adapters::storage::StoreOptions::default(),
        )
        .await
        .expect("open db"),
    );
    let clock = {
        let now = Arc::new(std::sync::Mutex::new(now_ms));
        let now2 = now.clone();
        let tick = move || *now2.lock().expect("clock");
        (now, tick)
    };
    let fh = Arc::new(FileHistoryService::new(
        db.clone(),
        Some(workspace.clone()),
        clock.1.clone(),
    ));
    FhFixture {
        home,
        workspace,
        now: clock.0,
        fh,
        db,
    }
}

// ── 纯函数金样 ──

#[test]
fn policy_golden_table_mirrors_text_file_policy() {
    // 尺寸上限与合并窗常数（text-file-policy.ts :4；history-store.ts :42）。
    assert_eq!(MAX_SNAPSHOT_BYTES, 5 * 1024 * 1024);
    assert_eq!(MERGE_WINDOW_MS, 60_000);

    // isTrackedFile（text-file-policy.ts :55-62）。
    for yes in [
        "src/main.rs",
        "a/b/notes.md",
        "style.css",
        "x.ts",
        "README",
        "Makefile",
        "Dockerfile",
        ".gitignore", // basename 命中 KNOWN_TEXT_FILENAMES（extOf idx=0 → null）
        ".env",
        "config.yaml",
        "script.SH", // extOf 小写化（:40）
    ] {
        assert!(is_tracked_file(yes), "tracked: {yes}");
    }
    for no in [
        "Cargo.lock",        // CHURN_FILENAMES（:24）
        "package-lock.json", // CHURN_FILENAMES 优先于扩展名白名单
        "yarn.lock",
        "pnpm-lock.yaml",
        "server.log", // CHURN_EXTENSIONS（:25）
        "x.lock",
        "a.tmp",
        "b.swp",
        "image.png",
        "archive.tar.gz",
        "foo.", // extOf idx==len-1 → null
        "LICENSE.txt.bak",
    ] {
        assert!(!is_tracked_file(no), "untracked: {no}");
    }

    // isIgnoredRelPath（:44-52）：只查目录段（末段是文件名，不查）。
    for yes in [
        "node_modules/dep/index.ts",
        "a/node_modules/b.ts",
        ".git/config",
        "x/.hidden/y.ts",
        "target/debug/build.rs",
        "dist/bundle.js",
        "__pycache__/m.py",
    ] {
        assert!(is_ignored_rel_path(yes), "ignored: {yes}");
    }
    for no in [
        "src/main.rs",
        ".hidden", // 单段路径无目录段可判（:46 i < len-1）
        "node_modules",
        "src/.env", // 末段不查
    ] {
        assert!(!is_ignored_rel_path(no), "not ignored: {no}");
    }
}

#[test]
fn workspace_hash_golden() {
    // workspaceHashForRoot（file-history-service.ts :19-22）：
    // sha256(斜杠归一路径) 十六进制前 16 字符。金样经 shasum -a 256 独立核算。
    assert_eq!(
        workspace_hash_for_root(std::path::Path::new("/a/b")),
        "662b7b62a798bb2d"
    );
    assert_eq!(
        workspace_hash_for_root(std::path::Path::new("/tmp/ws")),
        "b011ea26cc731e1a"
    );
}

// ── 合并窗 / unchanged / restore 永不合并（history-store.ts :84-116） ──

#[tokio::test]
async fn record_snapshot_merge_window_and_restore_semantics() {
    let fx = fh_fixture("merge", 1_000_000).await;
    let t0 = 1_000_000u64;

    // 首录 → inserted。
    let r1 = fx
        .fh
        .record_snapshot("a.txt", b"v1", "event")
        .await
        .expect("record v1");
    let id1 = match r1 {
        RecordOutcome::Inserted(id) => id,
        other => panic!("v1 inserted, got {other:?}"),
    };

    // 同内容 → unchanged，id 不变（:91-94）。
    *fx.now.lock().expect("clock") = t0 + 10;
    let r = fx
        .fh
        .record_snapshot("a.txt", b"v1", "event")
        .await
        .expect("record same");
    assert_eq!(r, RecordOutcome::Unchanged(id1));

    // 窗口内不同内容 → merged，行 id 不变、内容/时间更新（:103-108）。
    *fx.now.lock().expect("clock") = t0 + 1_000;
    let r = fx
        .fh
        .record_snapshot("a.txt", b"v2", "event")
        .await
        .expect("record v2");
    assert_eq!(r, RecordOutcome::Merged(id1));
    let snap = fx
        .fh
        .get_snapshot_content(id1)
        .await
        .expect("snapshot row")
        .expect("present");
    assert_eq!(snap.content, b"v2");
    assert_eq!(snap.captured_at_ms, t0 + 1_000);

    // 出窗（距最新 69s > 60s）→ inserted 新行。
    *fx.now.lock().expect("clock") = t0 + 70_000;
    let r = fx
        .fh
        .record_snapshot("a.txt", b"v3", "event")
        .await
        .expect("record v3");
    let id2 = match r {
        RecordOutcome::Inserted(id) => id,
        other => panic!("v3 inserted, got {other:?}"),
    };
    assert_ne!(id1, id2);

    // restore 永不合入别人（origin == "restore" 直接 INSERT，:100-101）。
    *fx.now.lock().expect("clock") = t0 + 71_000;
    let r = fx
        .fh
        .record_snapshot("a.txt", b"v4", "restore")
        .await
        .expect("record v4");
    assert!(matches!(r, RecordOutcome::Inserted(_)));

    // 别人也永不合入 restore（latest.origin == "restore" → 不 withinWindow）。
    *fx.now.lock().expect("clock") = t0 + 72_000;
    let r = fx
        .fh
        .record_snapshot("a.txt", b"v5", "event")
        .await
        .expect("record v5");
    assert!(matches!(r, RecordOutcome::Inserted(_)));

    // 版本列表：captured_at DESC, id DESC（listVersions :158）。
    let versions = fx.fh.list_versions("a.txt").await.expect("versions");
    assert_eq!(versions.len(), 4);
    assert!(
        versions
            .windows(2)
            .all(|w| (w[0].captured_at_ms, w[0].id) >= (w[1].captured_at_ms, w[1].id)),
        "desc order: {versions:?}"
    );
    assert_eq!(versions[0].origin, "event");
    assert_eq!(versions[1].origin, "restore");

    // listFiles：lastCapturedAt DESC（:149）。b 的第二录须出窗
    //（距 b1 68s ≥ 60s）才增生行；窗内会 merged 进同一行。
    fx.fh
        .record_snapshot("b.txt", b"b1", "event")
        .await
        .expect("record b");
    *fx.now.lock().expect("clock") = t0 + 140_000;
    fx.fh
        .record_snapshot("b.txt", b"b2-later", "event")
        .await
        .expect("record b2");
    let files = fx.fh.list_files().await.expect("files");
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].rel_path, "b.txt", "lastCapturedAt DESC");
    assert_eq!(files[0].version_count, 2);
    assert_eq!(files[1].rel_path, "a.txt");
    assert_eq!(files[1].version_count, 4);
}

// ── 捕获准入闸（_capture :227-241 + _locate :197-213） ──

#[tokio::test]
async fn capture_from_disk_policy_gates() {
    let fx = fh_fixture("capture", 5_000).await;
    let ws = &fx.workspace;
    std::fs::create_dir_all(ws.join("src")).expect("mkdir src");
    std::fs::create_dir_all(ws.join("node_modules")).expect("mkdir nm");
    std::fs::write(ws.join("src/ok.rs"), b"fn main() {}").expect("ok.rs");
    std::fs::write(ws.join("notes.log"), b"noise").expect("log");
    std::fs::write(ws.join("node_modules/dep.ts"), b"x").expect("dep");
    std::fs::write(ws.join("binary.bin"), b"\x00\x01").expect("bin");
    let big = std::fs::File::create(ws.join("big.txt")).expect("big");
    big.set_len(MAX_SNAPSHOT_BYTES + 1).expect("set_len");
    drop(big);

    // 受跟踪文本 → 捕获。
    assert!(
        fx.fh
            .capture_from_disk(&ws.join("src/ok.rs"), "event")
            .await,
        "tracked captured"
    );
    let versions = fx.fh.list_versions("src/ok.rs").await.expect("versions");
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].origin, "event");
    assert_eq!(versions[0].size_bytes, 12);

    // churn 扩展名 / 忽略目录 / 非文本扩展名 / 超尺寸 → 静默不捕获。
    for (abs, why) in [
        (ws.join("notes.log"), "churn ext"),
        (ws.join("node_modules/dep.ts"), "ignored dir"),
        (ws.join("binary.bin"), "untracked ext"),
        (ws.join("big.txt"), "oversized"),
        (ws.join("missing.rs"), "not a file"),
        (fx.home.join("outside.rs"), "outside workspace"),
    ] {
        assert!(
            !fx.fh.capture_from_disk(&abs, "event").await,
            "refused: {why}"
        );
    }
    // 目录不是文件。
    assert!(!fx.fh.capture_from_disk(&ws.join("src"), "event").await);
    // 相对路径不入闸。
    assert!(
        !fx.fh
            .capture_from_disk(std::path::Path::new("src/ok.rs"), "event")
            .await
    );

    // 未跟踪工作区的服务：全部拒绝。
    let bare = FileHistoryService::new(fx.db.clone(), None, || 5_000);
    assert!(!bare.has_workspace());
    assert!(!bare.capture_from_disk(&ws.join("src/ok.rs"), "event").await);
    let fx2 = fh_fixture("capture-b", 6_000).await;
    assert!(fx2.fh.has_workspace());
    fx2.home.metadata().expect("fx2 alive");
    drop(fx2);
}

// ── D8：ResourceIO 写路径捕获（origin event / restore，emit 门控） ──

#[tokio::test]
async fn resourceio_write_captures_history_with_origin() {
    let fx = fh_fixture("io-capture", 50_000).await;
    let sf = Arc::new(SessionFileService::new(fx.home.clone(), fx.db.clone(), {
        let now = fx.now.clone();
        move || *now.lock().expect("clock")
    }));
    let resources = Arc::new(ResourceService::new(
        sf.clone(),
        "studio-test".to_string(),
        fx.home.clone(),
        {
            let now = fx.now.clone();
            move || *now.lock().expect("clock")
        },
    ));
    let io = ResourceIoService::new(
        sf,
        resources,
        fx.home.clone(),
        Some((
            Arc::new(ResourceAccess::new(std::slice::from_ref(&fx.workspace)).expect("access")),
            fx.workspace.clone(),
        )),
        40,
        Arc::new({
            let now = fx.now.clone();
            move || *now.lock().expect("clock")
        }),
    );
    io.set_file_history(fx.fh.clone());
    let target = fx.workspace.join("src/app.ts");
    let target_ref = serde_json::json!({"kind": "local-file", "path": target.to_str().unwrap()});

    // 默认 ctx（emit=true，无 capture_origin）→ origin "event"。
    io.write(
        &target_ref,
        b"first".to_vec(),
        &OpContext::local_owner("s-1"),
    )
    .await
    .expect("write 1");
    let versions = fx.fh.list_versions("src/app.ts").await.expect("versions");
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].origin, "event");

    // 窗口内再写 → merged（仍一行）。
    io.write(
        &target_ref,
        b"second".to_vec(),
        &OpContext::local_owner("s-1"),
    )
    .await
    .expect("write 2");
    let versions = fx.fh.list_versions("src/app.ts").await.expect("versions");
    assert_eq!(versions.len(), 1, "merged within window");

    // capture_origin = "restore" → 新行 origin "restore"（永不合并）。
    let mut ctx = OpContext::local_owner("s-1");
    ctx.capture_origin = Some("restore".to_string());
    io.write(&target_ref, b"third".to_vec(), &ctx)
        .await
        .expect("write restore");
    let versions = fx.fh.list_versions("src/app.ts").await.expect("versions");
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].origin, "restore");

    // emit=false → 不捕获（现役 emit:false 无 resource.changed 事件）。
    let mut silent = OpContext::local_owner("s-1");
    silent.emit = false;
    io.write(&target_ref, b"fourth".to_vec(), &silent)
        .await
        .expect("write silent");
    let versions = fx.fh.list_versions("src/app.ts").await.expect("versions");
    assert_eq!(versions.len(), 2, "emit=false skips capture");

    // 未接线的 io（无 set_file_history）→ 写入成功但不捕获。
    let fx2 = fh_fixture("io-bare", 60_000).await;
    let sf2 = Arc::new(SessionFileService::new(
        fx2.home.clone(),
        fx2.db.clone(),
        || 60_000,
    ));
    let resources2 = Arc::new(ResourceService::new(
        sf2.clone(),
        "studio-test".to_string(),
        fx2.home.clone(),
        || 60_000,
    ));
    let io2 = ResourceIoService::new(
        sf2,
        resources2,
        fx2.home.clone(),
        Some((
            Arc::new(ResourceAccess::new(std::slice::from_ref(&fx2.workspace)).expect("access")),
            fx2.workspace.clone(),
        )),
        40,
        Arc::new(|| 60_000),
    );
    let t2 = fx2.workspace.join("nohist.md");
    let t2_ref = serde_json::json!({"kind": "local-file", "path": t2.to_str().unwrap()});
    io2.write(&t2_ref, b"x".to_vec(), &OpContext::local_owner("s-1"))
        .await
        .expect("write bare");
    assert!(
        fx2.fh
            .list_versions("nohist.md")
            .await
            .expect("versions")
            .is_empty(),
        "unwired io captures nothing"
    );
}

// ── 路由级（真 TCP + 真 HTTP） ──

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    workspace: Option<PathBuf>,
    token: String,
    storage: Arc<lingxi_adapters::storage::RunDatabase>,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn start_server(tag: &str, with_workspace: bool) -> TestServer {
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06t05-fh-http-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06t05-fh-http-ws-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&workspace).expect("mkdir workspace");
    let home = home.canonicalize().expect("canonical home");
    let workspace = workspace.canonicalize().expect("canonical workspace");
    let layout = prepare_layout(&home).expect("prepare layout");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static loopback addr"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let deps = ServiceDeps {
        workspace_root: with_workspace.then_some(workspace.clone()),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
        .await
        .expect("bootstrap");
    let token = state.auth().local_token();
    let storage = state.storage().clone();
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
        workspace: with_workspace.then_some(workspace),
        token,
        storage,
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
        if let Some(ws) = &self.workspace {
            let _ = std::fs::remove_dir_all(ws);
        }
    }

    async fn post(&self, path: &str, body: &serde_json::Value) -> (u16, String) {
        http(
            &self.addr,
            "POST",
            path,
            &[("Authorization", &self.bearer())],
            Some(&body.to_string()),
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
}

struct HttpResponse {
    status: u16,
    #[allow(dead_code)] // 统一 HTTP 夹具形状；本套件只断言状态与体。
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    #[allow(dead_code)]
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn into_status_body(self) -> (u16, String) {
        (
            self.status,
            String::from_utf8_lossy(&self.body).into_owned(),
        )
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
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| (p, p + 4));
    let (head_bytes, body_bytes) = match split {
        Some((h, b)) => (&raw[..h], &raw[b..]),
        None => (&raw[..], &[][..]),
    };
    let head = String::from_utf8_lossy(head_bytes).into_owned();
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
        body: body_bytes.to_vec(),
    }
}

/// 四叶面全链 + 错误词汇（file-history.ts 逐面）。
#[tokio::test]
async fn file_history_routes_full_chain_and_error_vocabulary() {
    let server = start_server("routes", true).await;
    let ws = server.workspace.clone().expect("workspace");
    let ws_hash = {
        // 候选人 hash 与 bootstrap 布线一致（canonical + 斜杠归一）。
        lingxi_service::filehistory::workspace_hash_for_root(&ws)
    };

    // ── 鉴权与 agentId 必填（叶 #19-21；LocalOnly 中间件） ──
    for path in [
        "/lingxi/v1/file-history/files",
        "/lingxi/v1/file-history/versions?relPath=a.txt",
        "/lingxi/v1/file-history/snapshot?id=1",
    ] {
        let resp = http(&server.addr, "GET", path, &[], None).await;
        assert_eq!(resp.status, 401, "no token refuses: {path}");
    }
    let resp = http(
        &server.addr,
        "POST",
        "/lingxi/v1/file-history/restore",
        &[],
        Some("{}"),
    )
    .await;
    assert_eq!(resp.status, 401, "restore no token refuses");

    // 无 agentId → 400 {error:"agentId required"}（:23）。
    let (status, body) = server.get("/lingxi/v1/file-history/files").await;
    assert_eq!(status, 400, "files: {body}");
    assert_eq!(json_err(&body), "agentId required");
    let (status, body) = server
        .get("/lingxi/v1/file-history/versions?relPath=a.txt")
        .await;
    assert_eq!(status, 400);
    assert_eq!(json_err(&body), "agentId required");
    let (status, body) = server.get("/lingxi/v1/file-history/snapshot?id=1").await;
    assert_eq!(status, 400);
    assert_eq!(json_err(&body), "agentId required");
    let (status, body) = server
        .post(
            "/lingxi/v1/file-history/restore",
            &serde_json::json!({"snapshotId": 1}),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(json_err(&body), "agentId required");

    // D12 延展：未知/重复 query 键 → 400。
    let (status, _body) = server
        .get("/lingxi/v1/file-history/files?agentId=a&bogus=1")
        .await;
    assert_eq!(status, 400, "unknown query key");
    let (status, _body) = server
        .get("/lingxi/v1/file-history/files?agentId=a&agentId=b")
        .await;
    assert_eq!(status, 400, "duplicate query key");

    // 空库：已跟踪工作区 → 200 空表。
    let (status, body) = server
        .get("/lingxi/v1/file-history/files?agentId=agent-x")
        .await;
    assert_eq!(status, 200, "files: {body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["files"],
        serde_json::json!([])
    );

    // ── 备料：旧版本直接落库（backdate 120s，避开合并窗），新版本走
    // ResourceIO 写路由（D8 捕获 origin "event"） ──
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64;
    let old_id = lingxi_adapters::storage::session_files::insert_file_snapshot(
        &server.storage,
        &lingxi_adapters::storage::session_files::FileSnapshotInsert {
            workspace_hash: &ws_hash,
            rel_path: "doc.txt",
            captured_at_ms: now_ms - 120_000,
            origin: "event",
            content: b"original".to_vec(),
        },
    )
    .await
    .expect("seed old snapshot");

    let doc = ws.join("doc.txt");
    let doc_ref = serde_json::json!({"kind": "local-file", "path": doc.to_str().unwrap()});
    let (status, body) = server
        .post(
            "/lingxi/v1/resource-io/write",
            &serde_json::json!({"resource": doc_ref, "content": "current"}),
        )
        .await;
    assert_eq!(status, 200, "write route: {body}");

    // ── files 面：形状 + lastCapturedAt DESC（:144-151） ──
    let (status, body) = server
        .get("/lingxi/v1/file-history/files?agentId=agent-x")
        .await;
    assert_eq!(status, 200);
    let files = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let files = files["files"].as_array().expect("files array");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["relPath"], "doc.txt");
    assert_eq!(files[0]["deletedAt"], serde_json::Value::Null);
    assert_eq!(files[0]["snapshotCount"], 2);
    assert!(files[0]["lastCapturedAt"].as_u64().expect("ms") >= now_ms - 1_000);

    // ── versions 面：形状 + 倒序（:153-160） ──
    let (status, body) = server
        .get("/lingxi/v1/file-history/versions?agentId=agent-x&relPath=doc.txt")
        .await;
    assert_eq!(status, 200, "versions: {body}");
    let versions = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let versions = versions["versions"].as_array().expect("versions array");
    assert_eq!(versions.len(), 2);
    assert!(
        versions[0]["capturedAt"].as_u64().unwrap() > versions[1]["capturedAt"].as_u64().unwrap()
    );
    assert_eq!(versions[0]["origin"], "event");
    assert_eq!(versions[0]["opContext"], serde_json::Value::Null);
    assert_eq!(versions[0]["rawSize"], 7);
    assert_eq!(versions[1]["id"], old_id);
    assert_eq!(versions[1]["rawSize"], 8);

    // invalid relPath → 400（:45）；缺 relPath 同样 400。
    for q in [
        "relPath=../x",
        "relPath=a\\\\b",
        "relPath=a//b",
        "relPath=",
        "",
    ] {
        let (status, body) = server
            .get(&format!("/lingxi/v1/file-history/versions?agentId=a&{q}"))
            .await;
        assert_eq!(status, 400, "relPath {q:?}: {body}");
        assert_eq!(json_err(&body), "invalid relPath");
    }

    // ── snapshot 面 ──
    let (status, body) = server
        .get(&format!(
            "/lingxi/v1/file-history/snapshot?agentId=a&id={old_id}"
        ))
        .await;
    assert_eq!(status, 200, "snapshot: {body}");
    let snap = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(snap["relPath"], "doc.txt");
    assert_eq!(snap["origin"], "event");
    assert_eq!(snap["content"], "original");
    assert!(snap["capturedAt"].is_u64());

    // invalid id → 400（:57）；不存在 → 404 "file-history snapshot N not found"。
    for q in ["id=0", "id=-3", "id=abc", ""] {
        let (status, body) = server
            .get(&format!("/lingxi/v1/file-history/snapshot?agentId=a&{q}"))
            .await;
        assert_eq!(status, 400, "id {q:?}: {body}");
        assert_eq!(json_err(&body), "invalid id");
    }
    let (status, body) = server
        .get("/lingxi/v1/file-history/snapshot?agentId=a&id=999999")
        .await;
    assert_eq!(status, 404);
    assert_eq!(json_err(&body), "file-history snapshot 999999 not found");

    // ── restore 面：ResourceIO 写回 + captureNow "restore"（:86-89） ──
    let (status, body) = server
        .post(
            "/lingxi/v1/file-history/restore",
            &serde_json::json!({"agentId": "a", "snapshotId": old_id}),
        )
        .await;
    assert_eq!(status, 200, "restore: {body}");
    let restored = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(restored["ok"], true);
    assert_eq!(restored["relPath"], "doc.txt");
    assert_eq!(std::fs::read(&doc).expect("disk"), b"original");

    // restore 落一行 origin="restore"（不合并）。
    let (_status, body) = server
        .get("/lingxi/v1/file-history/versions?agentId=a&relPath=doc.txt")
        .await;
    let versions = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    let versions = versions["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(versions[0]["origin"], "restore");

    // 再 restore 同一快照：内容未变 → unchanged，不增生行（:91-94）。
    let (status, _body) = server
        .post(
            "/lingxi/v1/file-history/restore",
            &serde_json::json!({"agentId": "a", "snapshotId": old_id}),
        )
        .await;
    assert_eq!(status, 200);
    let (_status, body) = server
        .get("/lingxi/v1/file-history/versions?agentId=a&relPath=doc.txt")
        .await;
    let versions = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(versions["versions"].as_array().unwrap().len(), 3);

    // invalid snapshotId → 400；不存在 → 404（:79/:91）。
    let (status, body) = server
        .post(
            "/lingxi/v1/file-history/restore",
            &serde_json::json!({"agentId": "a", "snapshotId": "nope"}),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(json_err(&body), "invalid snapshotId");
    let (status, body) = server
        .post(
            "/lingxi/v1/file-history/restore",
            &serde_json::json!({"agentId": "a", "snapshotId": 424242}),
        )
        .await;
    assert_eq!(status, 404);
    assert_eq!(json_err(&body), "file-history snapshot 424242 not found");

    // JSON 语法坏 → 400（from_json_rejection 既有面）。
    let resp = http(
        &server.addr,
        "POST",
        "/lingxi/v1/file-history/restore",
        &[("Authorization", &server.bearer())],
        Some("{bad json"),
    )
    .await;
    assert_eq!(resp.status, 400);

    server.stop_and_clean().await;
}

/// 未配置工作区 → 404 "workspace not tracked"（:26）。
#[tokio::test]
async fn file_history_routes_workspace_not_tracked() {
    let server = start_server("untracked", false).await;
    let (status, body) = server.get("/lingxi/v1/file-history/files?agentId=a").await;
    assert_eq!(status, 404);
    assert_eq!(json_err(&body), "workspace not tracked");
    let (status, body) = server
        .post(
            "/lingxi/v1/file-history/restore",
            &serde_json::json!({"agentId": "a", "snapshotId": 1}),
        )
        .await;
    assert_eq!(status, 404);
    assert_eq!(json_err(&body), "workspace not tracked");
    server.stop_and_clean().await;
}

fn json_err(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .unwrap_or_else(|err| panic!("json body {body:?}: {err}"))["error"]
        .as_str()
        .expect("error field")
        .to_string()
}
