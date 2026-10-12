//! R06-T05 Round 5 生产链测试：fs 直读（叶 #23-24）。
//!
//! 对照纪律（RC-3，逐条亲读 `server/routes/fs.ts`）：
//! - resolveAllowedPath（:25-54）：词法 resolve → 首个命中的根定
//!   生死；lstat 命中 symlink 一律拒绝；realpath 必须在 realRoot 内；
//!   ENOENT 时父目录 realpath 在根内则放行词法路径（404 语义保留）。
//! - GET /fs/read（:89-99）：missing path 400 / path not allowed 403 /
//!   safeReadFile 失败 404 "file not found"；成功 c.text（utf-8 有损，
//!   shared/safe-fs.ts :9 Node utf-8 解码即替换字符）。
//! - GET /fs/read-base64（:102-115）：同闸；读出任何错误 404；成功
//!   base64 文本。
//! - 设计登记（01_design :87/:145）：候选人加 20MB 上限（413
//!   "file too large"——与现役 docx/xlsx 面的上限词汇一致）；授权根
//!   = data_home + workspace（现役 = lingxiHome + 全体 agent desk，
//!   :77-86）；LocalOnly。docx-html/xlsx-html 依赖 mammoth/ExcelJS，
//!   不在叶 #23-24 范围（叶图 deferred）。D12 延展：query 未知/重复
//!   键 → 400。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── 纯函数：resolve_allowed_path 对照表（fs.ts :25-54） ──

#[tokio::test]
async fn resolve_allowed_path_mirror_table() {
    let uniq = format!(
        "{}-{}-resolve",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root_a = std::env::temp_dir()
        .join(format!("lingxi-r06t05-fs-a-{uniq}"))
        .canonicalize()
        .unwrap_or_else(|_| {
            let p = std::env::temp_dir().join(format!("lingxi-r06t05-fs-a-{uniq}"));
            std::fs::create_dir_all(&p).expect("mkdir a");
            p.canonicalize().expect("canonical a")
        });
    let root_b = std::env::temp_dir().join(format!("lingxi-r06t05-fs-b-{uniq}"));
    std::fs::create_dir_all(&root_b).expect("mkdir b");
    let root_b = root_b.canonicalize().expect("canonical b");
    let roots = vec![root_a.clone(), root_b.clone()];

    // 根内现有文件 → realpath 放行。
    let inside = root_a.join("a.txt");
    std::fs::write(&inside, b"a").expect("write");
    let resolved = lingxi_service::fsread::resolve_allowed_path(&inside.to_string_lossy(), &roots);
    assert_eq!(resolved.as_deref(), Some(inside.as_path()));

    // 根外 → None。
    let outside = std::env::temp_dir().join(format!("lingxi-r06t05-fs-out-{uniq}.txt"));
    std::fs::write(&outside, b"o").expect("write outside");
    assert!(
        lingxi_service::fsread::resolve_allowed_path(&outside.to_string_lossy(), &roots).is_none(),
        "outside refused"
    );

    // `..` 越界（词法 resolve 后仍在根外）→ None。
    let escaped = format!("{}/../outside.txt", root_a.to_string_lossy());
    assert!(lingxi_service::fsread::resolve_allowed_path(&escaped, &roots).is_none());

    // 不存在但父目录在根内 → 词法路径放行（现役 :43-49 的 404 语义）。
    let missing = root_a.join("sub/../not-there.txt");
    let resolved = lingxi_service::fsread::resolve_allowed_path(&missing.to_string_lossy(), &roots);
    assert_eq!(
        resolved.as_deref(),
        Some(root_a.join("not-there.txt").as_path()),
        "lexical fallthrough keeps 404 semantics"
    );

    // symlink 一律拒绝（:37-38）——即使指向根内。
    let link = root_a.join("link.txt");
    std::os::unix::fs::symlink(&inside, &link).expect("symlink");
    assert!(
        lingxi_service::fsread::resolve_allowed_path(&link.to_string_lossy(), &roots).is_none(),
        "symlink refused"
    );

    // 根本身（目录）词法在根内：realpath == realRoot → 放行（读目录后续
    // 404——目录读不出内容）。
    let resolved = lingxi_service::fsread::resolve_allowed_path(&root_a.to_string_lossy(), &roots);
    assert_eq!(resolved.as_deref(), Some(root_a.as_path()));

    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
    let _ = std::fs::remove_file(&outside);
}

// ── 路由级（真 TCP + 真 HTTP） ──

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    workspace: PathBuf,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn start_server(tag: &str) -> TestServer {
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06t05-fs-http-{uniq}"));
    let workspace = std::env::temp_dir().join(format!("lingxi-r06t05-fs-http-ws-{uniq}"));
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
        workspace_root: Some(workspace.clone()),
        ..ServiceDeps::default()
    };
    let state = ServiceState::bootstrap_with_deps(config, &layout, deps)
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
        workspace,
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
        let _ = std::fs::remove_dir_all(&self.workspace);
    }
}

struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
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

/// 两叶面全链 + 错误词汇（fs.ts :89-115）。
#[tokio::test]
async fn fs_read_routes_faces_and_error_vocabulary() {
    let server = start_server("routes").await;
    let bearer = server.bearer();

    // LocalOnly：无 token → 401。
    let resp = http(&server.addr, "GET", "/lingxi/v1/fs/read?path=/x", &[], None).await;
    assert_eq!(resp.status, 401, "read no token");
    let resp = http(
        &server.addr,
        "GET",
        "/lingxi/v1/fs/read-base64?path=/x",
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 401, "read-base64 no token");

    // missing path → 400 "missing path"（:91/:104）。
    let resp = http(
        &server.addr,
        "GET",
        "/lingxi/v1/fs/read",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 400);
    assert!(String::from_utf8_lossy(&resp.body).contains("missing path"));

    // D12 延展：未知/重复 query 键 → 400。
    let resp = http(
        &server.addr,
        "GET",
        "/lingxi/v1/fs/read?path=/x&bogus=1",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 400, "unknown query key");
    let resp = http(
        &server.addr,
        "GET",
        "/lingxi/v1/fs/read?path=/x&path=/y",
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 400, "duplicate query key");

    // 根外 → 403 "path not allowed"（:93-95）。
    let outside = std::env::temp_dir().join("lingxi-r06t05-fs-outside-route.txt");
    std::fs::write(&outside, b"out").expect("outside");
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/fs/read?path={}", outside.display()),
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 403);
    assert!(String::from_utf8_lossy(&resp.body).contains("path not allowed"));
    let _ = std::fs::remove_file(&outside);

    // workspace 内文本（含非 ASCII + 非法 utf-8 字节的有损替换）。
    let doc = server.workspace.join("直读 笔记.txt");
    let mut raw = "直读内容\n".as_bytes().to_vec();
    raw.extend_from_slice(&[0xff, 0xfe]); // 非法 utf-8 → 替换字符
    std::fs::write(&doc, &raw).expect("doc");
    let path_q = query_encode(&doc.to_string_lossy());
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/fs/read?path={path_q}"),
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 200, "read ok");
    let content_type = resp
        .header("content-type")
        .expect("content-type")
        .to_string();
    let text = String::from_utf8(resp.body).expect("utf-8 body");
    assert!(text.starts_with("直读内容\n"), "body: {text:?}");
    assert!(text.contains('\u{FFFD}'), "lossy replacement: {text:?}");
    assert!(
        content_type.starts_with("text/plain"),
        "c.text → text/plain"
    );

    // data_home 内同样可读（授权根含 lingxiHome，:77-86 候选映射）。
    let home_doc = server.home.join("home-note.txt");
    std::fs::write(&home_doc, b"home").expect("home doc");
    let resp = http(
        &server.addr,
        "GET",
        &format!(
            "/lingxi/v1/fs/read?path={}",
            query_encode(&home_doc.to_string_lossy())
        ),
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 200, "data_home readable");
    assert_eq!(resp.body, b"home");

    // read-base64：原样字节 → base64（:110-111）。金样经 `base64` 独立核算。
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/fs/read-base64?path={path_q}"),
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 200, "read-base64 ok");
    assert_eq!(
        String::from_utf8(resp.body).expect("b64 text"),
        "55u06K+75YaF5a65Cv/+"
    );

    // 不存在（父目录在根内）→ 404 "file not found"（:96-97）。
    let missing = server.workspace.join("不存在.txt");
    let resp = http(
        &server.addr,
        "GET",
        &format!(
            "/lingxi/v1/fs/read?path={}",
            query_encode(&missing.to_string_lossy())
        ),
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 404);
    assert!(String::from_utf8_lossy(&resp.body).contains("file not found"));

    // 目录 → 读失败 → 404（readFileSync EISDIR → safeReadFile fallback null）。
    let resp = http(
        &server.addr,
        "GET",
        &format!(
            "/lingxi/v1/fs/read?path={}",
            query_encode(&server.workspace.to_string_lossy())
        ),
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 404, "dir reads as not found");

    // 20MB 上限（设计登记 :87）→ 413 "file too large"。稀疏文件。
    let huge = server.workspace.join("huge.bin");
    let f = std::fs::File::create(&huge).expect("create");
    f.set_len(lingxi_service::fsread::MAX_FS_READ_BYTES + 1)
        .expect("set_len");
    drop(f);
    let resp = http(
        &server.addr,
        "GET",
        &format!(
            "/lingxi/v1/fs/read?path={}",
            query_encode(&huge.to_string_lossy())
        ),
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 413);
    assert!(String::from_utf8_lossy(&resp.body).contains("file too large"));

    // symlink 在根内 → 403（resolveAllowedPath :37-38）。
    let link = server.workspace.join("link-note.txt");
    std::os::unix::fs::symlink(&doc, &link).expect("symlink");
    let resp = http(
        &server.addr,
        "GET",
        &format!(
            "/lingxi/v1/fs/read?path={}",
            query_encode(&link.to_string_lossy())
        ),
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 403, "symlink refused");

    server.stop_and_clean().await;
}

/// query 值的百分号编码（路径含空格/非 ASCII）：保留 RFC 3986
/// unreserved 集，其余逐字节 %XX 大写（避免给测试面引额外依赖边）。
fn query_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
