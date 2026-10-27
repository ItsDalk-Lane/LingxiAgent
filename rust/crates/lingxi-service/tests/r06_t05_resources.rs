//! R06-T05 service 层生产链测试：ResourceService（res_ 信封、内容解析、
//! 票据签发/校验）与 /resources 路由面（ETag / Range / ticket 豁免）。
//!
//! 对照纪律（RC-3）：
//! - 信封：`lib/resources/resource-envelope.ts`（schemaVersion 1、res_ 前缀、
//!   `studios/{studioId}/resources/{resourceId}`、lifecycle/storage/links）。
//! - 内容解析：`core/resource-service.ts` resolveContent（:44-109）的
//!   404/400/410/409/500 错误码序列与 etag `"${mtime36}-${size36}"`。
//! - 票据：`core/resource-ticket-service.ts`（HMAC-SHA256 base64url、
//!   payload schemaVersion 1、action resources.content、TTL 5min、
//!   timingSafeEqual、key 文件 {home}/security/resource-ticket-key 0600）。
//! - 路由：`server/routes/resources.ts`（Range/416/304/Content-Disposition）
//!   与 `server/index.ts` isResourceTicketContentRequest 的 ticket 豁免。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::resources::{
    file_id_from_session_file_resource_id, resource_id_for_session_file_id, ResourceService,
    RESOURCE_TICKET_ACTION,
};
use lingxi_service::sessionfiles::SessionFileService;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServeOutcome, ServiceConfig, ServiceDeps,
    ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

// ── 纯函数与服务级（不起 HTTP） ──

#[test]
fn resource_id_roundtrip_and_stability_rules() {
    // 现役 resourceIdForSessionFileId / fileIdFromSessionFileResourceId +
    // SESSION_FILE_ID_RE（/^sf_[A-Za-z0-9][A-Za-z0-9_-]*$/）。
    assert_eq!(
        resource_id_for_session_file_id("sf_abc123"),
        Some("res_sf_abc123".to_string())
    );
    assert_eq!(resource_id_for_session_file_id("sf_"), None);
    assert_eq!(resource_id_for_session_file_id("xx_abc"), None);
    assert_eq!(resource_id_for_session_file_id("sf_has space"), None);
    assert_eq!(
        file_id_from_session_file_resource_id("res_sf_abc123"),
        Some("sf_abc123")
    );
    assert_eq!(file_id_from_session_file_resource_id("res_xx_abc"), None);
    assert_eq!(file_id_from_session_file_resource_id("other_sf_abc"), None);
}

struct ResFixture {
    home: PathBuf,
    payload_dir: PathBuf,
    service: ResourceService,
    sf: Arc<SessionFileService>,
}

impl Drop for ResFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.home);
        let _ = std::fs::remove_dir_all(&self.payload_dir);
    }
}

async fn res_fixture(tag: &str, now_ms: u64, files: &[(&str, &[u8])]) -> (ResFixture, Vec<String>) {
    let uniq = format!(
        "{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let home = std::env::temp_dir().join(format!("lingxi-r06t05-res-{uniq}"));
    let payload_dir = std::env::temp_dir().join(format!("lingxi-r06t05-res-payload-{uniq}"));
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&payload_dir);
    std::fs::create_dir_all(&home).expect("mkdir home");
    std::fs::create_dir_all(&payload_dir).expect("mkdir payload");
    let db = Arc::new(
        lingxi_adapters::storage::RunDatabase::open(
            &home.join("runs.db"),
            lingxi_adapters::storage::StoreOptions::default(),
        )
        .await
        .expect("open db"),
    );
    let sf = Arc::new(SessionFileService::new(home.clone(), db, move || now_ms));
    let mut ids = Vec::new();
    for (rel, content) in files {
        let path = payload_dir.join(rel.replace('/', "_"));
        std::fs::write(&path, content).expect("write payload");
        let view = sf
            .register_file("s-res", &path, None, None, "user_upload")
            .await
            .expect("register");
        ids.push(view.file_id);
    }
    let service = ResourceService::new(
        sf.clone(),
        "studio-test".to_string(),
        home.clone(),
        move || now_ms,
    );
    (
        ResFixture {
            home,
            payload_dir,
            service,
            sf,
        },
        ids,
    )
}

#[tokio::test]
async fn envelope_matches_incumbent_shape() {
    let (fx, ids) = res_fixture("envelope", 1_000, &[("a.txt", b"hello")]).await;
    let resource_id = resource_id_for_session_file_id(&ids[0]).unwrap();
    let envelope = fx
        .service
        .get_resource(&resource_id)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(envelope["schemaVersion"], 1);
    assert_eq!(envelope["resourceId"], resource_id);
    assert_eq!(
        envelope["name"],
        format!("studios/studio-test/resources/{resource_id}")
    );
    assert_eq!(envelope["studioId"], "studio-test");
    assert_eq!(envelope["type"], "file");
    assert_eq!(envelope["source"], "session_file");
    assert_eq!(envelope["sourceId"], ids[0]);
    assert_eq!(envelope["fileId"], ids[0]);
    assert_eq!(envelope["mime"], "text/plain");
    assert_eq!(envelope["size"], 5);
    assert_eq!(envelope["kind"], "document");
    assert_eq!(envelope["isDirectory"], false);
    assert_eq!(envelope["lifecycle"]["status"], "available");
    assert_eq!(envelope["storage"]["provider"], "session_file");
    assert_eq!(envelope["storage"]["storageKind"], "external");
    assert_eq!(envelope["storage"]["localOnly"], true);
    assert_eq!(
        envelope["links"]["self"],
        format!("/lingxi/v1/resources/{resource_id}")
    );
    assert_eq!(
        envelope["links"]["content"],
        format!("/lingxi/v1/resources/{resource_id}/content")
    );
}

#[tokio::test]
async fn resolve_content_error_sequence_matches_incumbent() {
    let (fx, ids) = res_fixture("resolve", 1_000, &[("a.bin", b"0123456789")]).await;
    // 形状非法 → 400 invalid_resource_id。
    let err = fx
        .service
        .resolve_content("not-a-resource")
        .await
        .unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (400, "invalid_resource_id")
    );
    // 未知 → 404 resource_not_found。
    let err = fx
        .service
        .resolve_content("res_sf_unknown0000000")
        .await
        .unwrap_err();
    assert_eq!((err.status, err.code.as_str()), (404, "resource_not_found"));
    // 正常 → etag 形状 "{mtime36}-{size36}"（带引号）。
    let resolved = fx
        .service
        .resolve_content(&resource_id_for_session_file_id(&ids[0]).unwrap())
        .await
        .expect("resolve");
    assert_eq!(resolved.size, 10);
    assert!(resolved.etag.starts_with('"') && resolved.etag.ends_with('"'));
    assert!(resolved.etag.contains('-'));
    // 文件删除后 → reconcile missing → 404 resource_content_missing。
    std::fs::remove_file(&resolved.file_path).expect("remove payload");
    let err = fx
        .service
        .resolve_content(&resource_id_for_session_file_id(&ids[0]).unwrap())
        .await
        .unwrap_err();
    assert_eq!(
        (err.status, err.code.as_str()),
        (404, "resource_content_missing")
    );
}

#[tokio::test]
async fn ticket_roundtrip_tamper_mismatch_and_expiry() {
    let (fx, ids) = res_fixture("ticket", 1_000_000, &[("a.txt", b"ticket")]).await;
    let resource_id = resource_id_for_session_file_id(&ids[0]).unwrap();
    let issued = fx
        .service
        .issue_ticket(&resource_id, "principal-1")
        .expect("issue");
    assert!(issued.ticket_id.starts_with("rt_"));
    assert_eq!(issued.action, RESOURCE_TICKET_ACTION);
    // roundtrip。
    let verified = fx
        .service
        .verify_ticket(&issued.ticket, &resource_id)
        .expect("verify");
    assert_eq!(verified.resource_id, resource_id);
    assert_eq!(verified.principal_id, "principal-1");
    // 篡改签名 → resource_ticket_invalid。
    let mut forged = issued.ticket.clone();
    let last = forged.len() - 1;
    let replacement = if forged.as_bytes()[last] == b'A' {
        "B"
    } else {
        "A"
    };
    forged.replace_range(last.., replacement);
    let err = fx.service.verify_ticket(&forged, &resource_id).unwrap_err();
    assert_eq!(err.code, "resource_ticket_invalid");
    // resourceId 不匹配 → invalid。
    let err = fx
        .service
        .verify_ticket(&issued.ticket, "res_sf_else0000000000")
        .unwrap_err();
    assert_eq!(err.code, "resource_ticket_invalid");
    // 形状 malformed（三段）→ invalid。
    let err = fx
        .service
        .verify_ticket(&format!("{}.extra", issued.ticket), &resource_id)
        .unwrap_err();
    assert_eq!(err.code, "resource_ticket_invalid");
    // 过期：now 推到 TTL 之后 → resource_ticket_expired。
    let expired_view = ResourceService::new(
        fx.sf.clone(),
        "studio-test".to_string(),
        fx.home.clone(),
        || 1_000_000 + 5 * 60 * 1000 + 1,
    );
    let err = expired_view
        .verify_ticket(&issued.ticket, &resource_id)
        .unwrap_err();
    assert_eq!(err.code, "resource_ticket_expired");
    // key 文件权限与位置（D4 映射：现役 {lingxiHome}/security/ → 候选人
    // key 目录直下），0600。fixture 以 home 充 key 目录。
    let key_path = fx.home.join("resource-ticket-key");
    let meta = std::fs::metadata(&key_path).expect("key file exists");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    }
}

// ── 路由级（真 TCP + 真 HTTP） ──

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

async fn start_server(tag: &str) -> TestServer {
    let home = std::env::temp_dir().join(format!(
        "lingxi-r06t05-res-http-{}-{}-{tag}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("mkdir home");
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
}

struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn into_status_body(self) -> (u16, String) {
        (
            self.status,
            String::from_utf8_lossy(&self.body).into_owned(),
        )
    }

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

/// 路由链金样：注册附件 → GET 资源信封 → POST ticket → 带 ticket 的
/// content（无 Authorization，ticket 豁免）→ Range/ETag/416/304。
#[tokio::test]
async fn resources_route_chain_ticket_range_etag() {
    let server = start_server("chain").await;
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions",
            &serde_json::json!({"sessionId": "s-res", "agentId": "agent", "title": "t"})
                .to_string(),
        )
        .await;
    assert_eq!(status, 201, "create session: {body}");
    // 附件（payload 在 home 外）。
    let outside = std::env::temp_dir().join(format!(
        "lingxi-r06t05-res-payload-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&outside).expect("mkdir payload");
    let payload = outside.join("note.txt");
    std::fs::write(&payload, b"0123456789abcdef").expect("write payload");
    let (status, body) = server
        .post(
            "/lingxi/v1/sessions/s-res/attachments/local",
            &serde_json::json!({ "path": payload.to_str().unwrap() }).to_string(),
        )
        .await;
    assert_eq!(status, 201, "attach: {body}");
    let file_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["file"]["fileId"]
        .as_str()
        .unwrap()
        .to_string();
    let resource_id = resource_id_for_session_file_id(&file_id).unwrap();

    // GET 信封。
    let (status, body) = server
        .get(&format!("/lingxi/v1/resources/{resource_id}"))
        .await;
    assert_eq!(status, 200, "envelope: {body}");
    let envelope = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(envelope["resourceId"], resource_id);

    // POST ticket。
    let (status, body) = server
        .post(&format!("/lingxi/v1/resources/{resource_id}/ticket"), "{}")
        .await;
    assert_eq!(status, 200, "ticket: {body}");
    let ticket = serde_json::from_str::<serde_json::Value>(&body).unwrap()["ticket"]
        .as_str()
        .unwrap()
        .to_string();

    // content 带 ticket + Bearer。候选人纪律差异（台账申报）：现役
    // server/index.ts:642 对 `?ticket=` 完全豁免鉴权中间件（服务 <img> 等
    // 无头客户端）；候选人中间件对一切请求鉴权（R02 已冻结），ticket 是
    // 内容级附加校验——ticket 必须仍然有效，凭证也必须存在。
    let bearer = server.bearer();
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/resources/{resource_id}/content?ticket={ticket}"),
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 200, "ticket content: {:?}", resp.status);
    assert_eq!(resp.body, b"0123456789abcdef");
    assert_eq!(resp.header("Content-Type"), Some("text/plain"));
    assert_eq!(resp.header("Accept-Ranges"), Some("bytes"));
    let etag = resp.header("ETag").expect("etag").to_string();
    assert!(etag.starts_with('"') && etag.ends_with('"'));
    let cd = resp
        .header("Content-Disposition")
        .expect("content-disposition");
    assert!(cd.starts_with("inline;"), "inline disposition: {cd}");

    // Range 206 切片（现役 parseRangeHeader 语义）。
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/resources/{resource_id}/content?ticket={ticket}"),
        &[("Authorization", &bearer), ("Range", "bytes=2-5")],
        None,
    )
    .await;
    assert_eq!(resp.status, 206);
    assert_eq!(resp.body, b"2345");
    assert_eq!(resp.header("Content-Range"), Some("bytes 2-5/16"));

    // suffix range。
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/resources/{resource_id}/content?ticket={ticket}"),
        &[("Authorization", &bearer), ("Range", "bytes=-4")],
        None,
    )
    .await;
    assert_eq!(resp.status, 206);
    assert_eq!(resp.body, b"cdef");

    // 不可满足 → 416 + Content-Range bytes */size。
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/resources/{resource_id}/content?ticket={ticket}"),
        &[("Authorization", &bearer), ("Range", "bytes=99-100")],
        None,
    )
    .await;
    assert_eq!(resp.status, 416);
    assert_eq!(resp.header("Content-Range"), Some("bytes */16"));

    // If-None-Match 命中 → 304。
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/resources/{resource_id}/content?ticket={ticket}"),
        &[("Authorization", &bearer), ("If-None-Match", &etag)],
        None,
    )
    .await;
    assert_eq!(resp.status, 304);
    assert!(resp.body.is_empty());

    // 坏 ticket（即使有 Bearer）→ 403 resource_ticket_invalid：带 ticket
    // 时票据必须有效，不能静默降级为凭证路径。
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/resources/{resource_id}/content?ticket=bad.bad"),
        &[("Authorization", &bearer)],
        None,
    )
    .await;
    assert_eq!(resp.status, 403);
    let body = String::from_utf8_lossy(&resp.body).into_owned();
    assert!(body.contains("resource_ticket_invalid"), "{body}");

    // 无 ticket 但有 Bearer（现役 createRequestContext 面）。
    let (status, _) = server
        .get(&format!("/lingxi/v1/resources/{resource_id}/content"))
        .await;
    assert_eq!(status, 200);

    // 无 ticket 且无凭证 → 401（候选人中间件鉴权底线；现役此处因 ticket
    // 豁免路径不存在 ticket 也会先被鉴权拒）。
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/resources/{resource_id}/content"),
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 401);

    // 好 ticket 但无凭证 → 401（候选人纪律：ticket 不豁免鉴权，见上注）。
    let resp = http(
        &server.addr,
        "GET",
        &format!("/lingxi/v1/resources/{resource_id}/content?ticket={ticket}"),
        &[],
        None,
    )
    .await;
    assert_eq!(resp.status, 401);

    // 未知资源 → 404。
    let (status, _) = server
        .get("/lingxi/v1/resources/res_sf_ghost000000000")
        .await;
    assert_eq!(status, 404);

    let _ = std::fs::remove_dir_all(&outside);
    server.stop_and_clean().await;
}
