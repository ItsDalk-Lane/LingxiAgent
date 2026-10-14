//! 原R06资源义务的专属真实行为补证；不代替其他原断言。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Fixture {
    root: PathBuf,
    workspace: PathBuf,
    state: ServiceState,
    addr: SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}

impl Fixture {
    async fn start(tag: &str) -> Self {
        Self::start_with_workspace(tag, true).await
    }

    async fn start_with_workspace(tag: &str, with_workspace: bool) -> Self {
        Self::start_with_request_limit(tag, with_workspace, ServiceDeps::default().rate_max).await
    }

    async fn start_with_request_limit(tag: &str, with_workspace: bool, rate_max: u32) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-original-resource-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let home = root.join("home");
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home,
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
            },
            &layout,
            ServiceDeps {
                workspace_root: with_workspace.then_some(workspace.clone()),
                rate_max,
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let (stop, stop_rx) = tokio::sync::oneshot::channel();
        let (ready, ready_rx) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let task = tokio::spawn(async move {
            run(
                serving,
                async {
                    let _ = stop_rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
        });
        let addr = ready_rx.await.unwrap();
        let fx = Self {
            root,
            workspace,
            state,
            addr,
            stop,
            task,
        };
        let response = fx
            .post(
                "/lingxi/v1/sessions",
                json!({"sessionId":"s-proof","agentId":"agent","title":"proof",
            "authorizedFolders":[fx.workspace]}),
            )
            .await;
        assert_eq!(response.0, 201);
        fx
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        token: Option<String>,
    ) -> (u16, Vec<u8>, String) {
        let body = body.map(|value| value.to_string()).unwrap_or_default();
        let authorization = token
            .map(|value| format!("Authorization: Bearer {value}\r\n"))
            .unwrap_or_default();
        let wire = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\n{authorization}Connection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            self.addr,
            body.len()
        );
        let mut stream = tokio::net::TcpStream::connect(self.addr).await.unwrap();
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut raw))
            .await
            .unwrap()
            .unwrap();
        let boundary = raw
            .windows(4)
            .position(|value| value == b"\r\n\r\n")
            .unwrap();
        let headers = String::from_utf8(raw[..boundary].to_vec()).unwrap();
        let status = headers
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        (status, raw[boundary + 4..].to_vec(), headers)
    }

    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let (status, bytes, _) = self
            .request(
                "POST",
                path,
                Some(body),
                Some(self.state.auth().local_token()),
            )
            .await;
        let value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&bytes)}));
        (status, value)
    }

    async fn finish(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        std::fs::remove_dir_all(self.root).unwrap();
    }
}

fn io_body(operation: &str, reference: &Value, destination: &Value) -> Value {
    match operation {
        "rename" | "move" => json!({"from":reference,"to":destination}),
        "write" => json!({"resource":reference,"content":"refused replacement"}),
        "write-expected-version" => {
            json!({"resource":reference,"content":"refused replacement","expectedVersion":{"mtimeMs":1,"size":1}})
        }
        "search" => json!({"resource":reference,"query":"proof"}),
        _ => json!({"resource":reference}),
    }
}

async fn stage_registration_counts(fx: &Fixture) -> [String; 3] {
    let mut result = Vec::new();
    for sql in [
        "SELECT COUNT(*) FROM sessions",
        "SELECT COUNT(*) FROM session_files",
        "SELECT COUNT(*) FROM session_file_refs",
    ] {
        result.push(
            fx.state
                .storage()
                .query_one_text(sql, Vec::new())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    result.try_into().unwrap()
}

#[tokio::test]
async fn resourceio_operation_specific_provider_refusals_preserve_files_and_registrations() {
    let fx = Fixture::start("providers").await;
    let file = fx.workspace.join("proof.txt");
    std::fs::write(&file, b"proof bytes").unwrap();
    let attached = fx
        .post(
            "/lingxi/v1/sessions/s-proof/attachments/local",
            json!({"path":file}),
        )
        .await;
    assert_eq!(attached.0, 201, "{:?}", attached.1);
    let sf = json!({"kind":"session-file","fileId":attached.1["file"]["fileId"]});
    let local = json!({"kind":"local-file","path":file});
    let destination = json!({"kind":"local-file","path":fx.workspace.join("destination.txt")});
    let operations = [
        "stat",
        "read",
        "write",
        "write-expected-version",
        "list",
        "search",
        "rename",
        "move",
        "trash",
        "watch",
        "subscribe",
    ];
    for operation in operations {
        let invalid = fx
            .post(&format!("/lingxi/v1/resource-io/{operation}"), json!({}))
            .await;
        assert_eq!(invalid.0, 400, "{operation}缺少必需输入: {invalid:?}");
        for reference in [
            json!({"kind":"mount","mountId":"m-proof","path":"proof.txt"}),
            json!({"kind":"url","url":"https://example.invalid/proof"}),
        ] {
            let result = fx
                .post(
                    &format!("/lingxi/v1/resource-io/{operation}"),
                    io_body(operation, &reference, &destination),
                )
                .await;
            assert!([400, 501].contains(&result.0), "{operation}: {result:?}");
            let text = result.1.to_string();
            assert!(
                text.contains("unsupported_provider")
                    || text.contains("cross_provider_move_unsupported"),
                "{operation}: {text}"
            );
        }
        if !["stat", "read"].contains(&operation) {
            let result = fx
                .post(
                    &format!("/lingxi/v1/resource-io/{operation}"),
                    io_body(operation, &sf, &sf),
                )
                .await;
            assert_eq!(result.0, 403, "{operation}: {result:?}");
            assert!(
                result.1.to_string().contains("capability_denied"),
                "{operation}: {result:?}"
            );
        }
        let denied = fx
            .request(
                "POST",
                &format!("/lingxi/v1/resource-io/{operation}"),
                Some(io_body(operation, &local, &destination)),
                None,
            )
            .await;
        assert_eq!(denied.0, 401, "{operation}: {:?}", denied.1);
        assert_eq!(
            std::fs::read(&file).unwrap(),
            b"proof bytes",
            "{operation}失败不得改写原文件"
        );
        assert!(!fx.workspace.join("destination.txt").exists());
        assert_eq!(
            fx.state
                .session_files()
                .list_files("s-proof")
                .await
                .unwrap()
                .len(),
            1
        );
    }
    for (method, path) in [
        ("GET", "/lingxi/v1/resource-io/events?since=0"),
        ("GET", "/lingxi/v1/resource-io/watch-diagnostics"),
        ("DELETE", "/lingxi/v1/resource-io/watch/missing"),
        ("DELETE", "/lingxi/v1/resource-io/subscriptions/missing"),
    ] {
        assert_eq!(fx.request(method, path, None, None).await.0, 401, "{path}");
    }
    assert_eq!(fx.state.resource_io().diagnostics()["subscriptions"], 0);
    assert_eq!(
        fx.state.resource_io().diagnostics()["watches"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"proof bytes");
    fx.finish().await;
}

#[tokio::test]
async fn original_resource_watch_handle_lifecycle_and_cursor_reads_use_real_provider_registrations()
{
    let fx = Fixture::start("watch-handle-original").await;
    let file = fx.workspace.join("watch-source.txt");
    std::fs::write(&file, b"initial source").unwrap();
    let metadata = std::fs::metadata(&file).unwrap().modified().unwrap();
    let local = json!({"kind":"local-file","path":file});
    let (status, attached) = fx
        .post(
            "/lingxi/v1/sessions/s-proof/attachments/local",
            json!({"path":file}),
        )
        .await;
    assert_eq!(status, 201, "{attached}");
    let registered = stage_registration_counts(&fx).await;
    let local_token = fx.state.auth().local_token();
    let read_json = |bytes: &[u8]| serde_json::from_slice::<Value>(bytes).unwrap();
    let (status, subscription) = fx
        .post(
            "/lingxi/v1/resource-io/subscribe",
            json!({"resources":[local],"purpose":"original-lifecycle"}),
        )
        .await;
    assert_eq!(status, 200, "{subscription}");
    let subscription_id = subscription["subscriptionId"].as_str().unwrap();
    let (status, watch) = fx
        .post("/lingxi/v1/resource-io/watch", json!({"resource":local}))
        .await;
    assert_eq!(status, 200, "{watch}");
    let watch_id = watch["watchId"].as_str().unwrap();
    let (status, bytes, _) = fx
        .request(
            "GET",
            "/lingxi/v1/resource-io/watch-diagnostics",
            None,
            Some(local_token.clone()),
        )
        .await;
    assert_eq!(status, 200);
    let diagnostics = read_json(&bytes);
    assert_eq!(diagnostics["ok"], true);
    assert_eq!(diagnostics["diagnostics"]["subscriptions"], 1);
    assert_eq!(
        diagnostics["diagnostics"]["watches"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(diagnostics["diagnostics"]["watches"][0]["refCount"], 2);
    assert!(diagnostics["diagnostics"]["watches"][0]["resourceKey"]
        .as_str()
        .unwrap()
        .starts_with("local_fs:"));
    let watch_path = format!("/lingxi/v1/resource-io/watch/{watch_id}");
    let subscription_path = format!("/lingxi/v1/resource-io/subscriptions/{subscription_id}");
    for path in [&watch_path, &subscription_path] {
        let (status, _, _) = fx.request("DELETE", path, None, None).await;
        assert_eq!(status, 401);
        assert_eq!(
            fx.state.resource_io().diagnostics()["watches"][0]["refCount"],
            2
        );
    }
    for (path, released, count) in [
        (watch_path.as_str(), true, 1),
        (watch_path.as_str(), false, 1),
        (subscription_path.as_str(), true, 0),
        (subscription_path.as_str(), false, 0),
    ] {
        let (status, bytes, _) = fx
            .request("DELETE", path, None, Some(local_token.clone()))
            .await;
        assert_eq!(status, 200);
        assert_eq!(read_json(&bytes), json!({"ok":true,"released":released}));
        assert_eq!(
            fx.state.resource_io().diagnostics()["watches"]
                .as_array()
                .unwrap()
                .len(),
            count
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"initial source");
        assert_eq!(
            std::fs::metadata(&file).unwrap().modified().unwrap(),
            metadata
        );
        assert_eq!(stage_registration_counts(&fx).await, registered);
    }
    let (status, bytes, _) = fx
        .request(
            "GET",
            "/lingxi/v1/resource-io/events?since=0",
            None,
            Some(local_token.clone()),
        )
        .await;
    assert_eq!(status, 200);
    let events_before = read_json(&bytes);
    for reference in [
        json!({"kind":"mount","mountId":"unsupported-original-mount","path":"watch-source.txt"}),
        json!({"kind":"url","url":"https://example.invalid/watch-source.txt"}),
        json!({"kind":"session-file","fileId":attached["file"]["fileId"]}),
    ] {
        let expected = if reference["kind"] == "session-file" {
            403
        } else {
            400
        };
        for operation in ["watch", "subscribe"] {
            let body = if operation == "watch" {
                json!({"resource":reference})
            } else {
                json!({"resources":[local,reference]})
            };
            let (status, error) = fx
                .post(&format!("/lingxi/v1/resource-io/{operation}"), body)
                .await;
            assert_eq!(status, expected, "{operation}: {error}");
            assert_ne!(error["ok"], true);
            assert!(
                error.to_string().contains(if expected == 403 {
                    "capability_denied"
                } else {
                    "unsupported_provider"
                }),
                "{error}"
            );
            let (status, bytes, _) = fx
                .request(
                    "GET",
                    "/lingxi/v1/resource-io/watch-diagnostics",
                    None,
                    Some(local_token.clone()),
                )
                .await;
            assert_eq!(status, 200);
            let diagnostics = read_json(&bytes);
            assert_eq!(diagnostics["diagnostics"]["subscriptions"], 0);
            assert!(diagnostics["diagnostics"]["watches"]
                .as_array()
                .unwrap()
                .is_empty());
            let (status, bytes, _) = fx
                .request(
                    "GET",
                    "/lingxi/v1/resource-io/events?since=0",
                    None,
                    Some(local_token.clone()),
                )
                .await;
            assert_eq!(status, 200);
            assert_eq!(read_json(&bytes), events_before);
            assert_eq!(std::fs::read(&file).unwrap(), b"initial source");
            assert_eq!(stage_registration_counts(&fx).await, registered);
        }
    }
    let missing = json!({"kind":"local-file","path":fx.workspace.join("missing-watch")});
    let (status, error) = fx
        .post(
            "/lingxi/v1/resource-io/subscribe",
            json!({"resources":[local,missing]}),
        )
        .await;
    assert_eq!(status, 400, "{error}");
    assert!(fx.state.resource_io().diagnostics()["watches"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(fx.state.resource_io().diagnostics()["subscriptions"], 0);
    let unauthorized = json!({"kind":"local-file","path":fx.root.join("outside-workspace")});
    std::fs::write(fx.root.join("outside-workspace"), b"outside sentinel").unwrap();
    let (status, error) = fx
        .post(
            "/lingxi/v1/resource-io/watch",
            json!({"resource":unauthorized}),
        )
        .await;
    assert_eq!(status, 403, "{error}");
    assert_eq!(
        std::fs::read(fx.root.join("outside-workspace")).unwrap(),
        b"outside sentinel"
    );
    fx.finish().await;
}

#[tokio::test]
async fn original_resource_events_since_and_stale_resync_follow_real_writes_without_read_side_effects(
) {
    let fx = Fixture::start_with_request_limit("events-retention-original", true, 2048).await;
    let file = fx.workspace.join("event-source.txt");
    let local = json!({"kind":"local-file","path":file});
    let token = fx.state.auth().local_token();
    let read_json = |bytes: &[u8]| serde_json::from_slice::<Value>(bytes).unwrap();
    let (status, result) = fx
        .post(
            "/lingxi/v1/resource-io/write",
            json!({"resource":local,"content":"first","sessionId":"s-proof"}),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    let (status, bytes, _) = fx
        .request(
            "GET",
            "/lingxi/v1/resource-io/events?since=0",
            None,
            Some(token.clone()),
        )
        .await;
    assert_eq!(status, 200);
    let first = read_json(&bytes);
    assert_eq!(first["stale"], false);
    let cursor = first["latestSequence"].as_u64().unwrap();
    assert!(cursor > 0);
    assert!(first["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["type"] == "resource.changed"
            && event["resourceKey"]
                .as_str()
                .unwrap()
                .starts_with("local_fs:")));
    for size in 1..=1005 {
        let (status, result) = fx
            .post(
                "/lingxi/v1/resource-io/write",
                json!({"resource":local,"content":"x".repeat(size),"sessionId":"s-proof"}),
            )
            .await;
        assert_eq!(status, 200, "write {size}: {result}");
    }
    let bytes_before = std::fs::read(&file).unwrap();
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    let counts = stage_registration_counts(&fx).await;
    let events_before = fx.state.resource_io().events_since(0);
    let (status, bytes, _) = fx
        .request(
            "GET",
            &format!("/lingxi/v1/resource-io/events?since={cursor}"),
            None,
            Some(token.clone()),
        )
        .await;
    assert_eq!(status, 200);
    let stale = read_json(&bytes);
    assert_eq!(stale["stale"], true);
    assert_eq!(stale["resync"], "resource-stat-required");
    assert!(stale["events"].as_array().unwrap().is_empty());
    let latest = stale["latestSequence"].as_u64().unwrap();
    assert!(latest >= cursor + 1005);
    let (status, bytes, _) = fx
        .request(
            "GET",
            &format!("/lingxi/v1/resource-io/events?since={}", latest - 3),
            None,
            Some(token.clone()),
        )
        .await;
    assert_eq!(status, 200);
    let recent = read_json(&bytes);
    assert_eq!(recent["stale"], false);
    assert_eq!(recent["latestSequence"], latest);
    assert_eq!(recent["events"].as_array().unwrap().len(), 3);
    for (offset, event) in recent["events"].as_array().unwrap().iter().enumerate() {
        assert_eq!(event["sequence"], latest - 2 + offset as u64);
        assert_eq!(event["type"], "resource.changed");
    }
    for path in [
        "/lingxi/v1/resource-io/events?since=abc",
        "/lingxi/v1/resource-io/events?since=-1",
        "/lingxi/v1/resource-io/events?since=0&since=1",
        "/lingxi/v1/resource-io/events?resource=unsupported",
    ] {
        assert_eq!(
            fx.request("GET", path, None, Some(token.clone())).await.0,
            400,
            "{path}"
        );
    }
    assert_eq!(
        fx.request("GET", "/lingxi/v1/resource-io/events?since=0", None, None)
            .await
            .0,
        401
    );
    assert_eq!(
        fx.request(
            "GET",
            "/lingxi/v1/resource-io/watch-diagnostics",
            None,
            None
        )
        .await
        .0,
        401
    );
    let (status, bytes, _) = fx
        .request(
            "GET",
            "/lingxi/v1/resource-io/watch-diagnostics",
            None,
            Some(token),
        )
        .await;
    assert_eq!(status, 200);
    let diagnostics = read_json(&bytes);
    assert_eq!(diagnostics["diagnostics"]["subscriptions"], 0);
    assert!(diagnostics["diagnostics"]["watches"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(std::fs::read(&file).unwrap(), bytes_before);
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(stage_registration_counts(&fx).await, counts);
    assert_eq!(fx.state.resource_io().events_since(0), events_before);
    fx.finish().await;
}

#[tokio::test]
async fn attachment_blob_voice_reuse_and_directory_reference_preserve_actual_payloads() {
    use base64::Engine;
    let fx = Fixture::start("attachments").await;
    let png = b"\x89PNG\r\n\x1a\n";
    let body = json!({"name":"proof.png","mime":"image/png","dataBase64":base64::engine::general_purpose::STANDARD.encode(png)});
    let media = fx
        .post("/lingxi/v1/sessions/s-proof/attachments/blob", body.clone())
        .await;
    assert_eq!(media.0, 201);
    assert_eq!(
        std::fs::read(media.1["file"]["realPath"].as_str().unwrap()).unwrap(),
        png
    );
    let reused = fx
        .post("/lingxi/v1/sessions/s-proof/attachments/blob", body)
        .await;
    assert_eq!(reused.0, 201);
    assert_eq!(reused.1["file"]["fileId"], media.1["file"]["fileId"]);
    assert_eq!(reused.1["file"]["realPath"], media.1["file"]["realPath"]);
    let voice_bytes = b"RIFF0000WAVEproof";
    let voice = fx.post("/lingxi/v1/sessions/s-proof/attachments/blob", json!({"name":"voice.wav","mime":"audio/wav",
        "origin":"voice_input","dataBase64":base64::engine::general_purpose::STANDARD.encode(voice_bytes)})).await;
    assert_eq!(voice.0, 201);
    assert_eq!(voice.1["file"]["origin"], "voice_input");
    assert_eq!(voice.1["file"]["kind"], "audio");
    assert_eq!(
        std::fs::read(voice.1["file"]["realPath"].as_str().unwrap()).unwrap(),
        voice_bytes
    );
    let before = fx
        .state
        .session_files()
        .list_files("s-proof")
        .await
        .unwrap()
        .len();
    for invalid in [
        json!({"mime":"image/png","origin":"voice_input","dataBase64":"iVBORw0KGgo="}),
        json!({"mime":"audio/wav","dataBase64":"!!!"}),
    ] {
        let denied = fx
            .post("/lingxi/v1/sessions/s-proof/attachments/blob", invalid)
            .await;
        assert_eq!(denied.0, 400);
        assert_eq!(
            fx.state
                .session_files()
                .list_files("s-proof")
                .await
                .unwrap()
                .len(),
            before
        );
    }
    let directory = fx.workspace.join("original-directory");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("retained.txt"), b"original").unwrap();
    let attached = fx
        .post(
            "/lingxi/v1/sessions/s-proof/attachments/local",
            json!({"path":directory}),
        )
        .await;
    assert_eq!(attached.0, 201);
    assert_eq!(attached.1["file"]["isDirectory"], true);
    assert_eq!(attached.1["file"]["storageKind"], "external");
    assert_eq!(
        attached.1["file"]["realPath"],
        directory.to_string_lossy().as_ref()
    );
    let reused = fx
        .post(
            "/lingxi/v1/sessions/s-proof/attachments/local",
            json!({"path":directory}),
        )
        .await;
    assert_eq!(reused.0, 201);
    assert_eq!(reused.1["file"]["fileId"], attached.1["file"]["fileId"]);
    assert_eq!(
        std::fs::read(directory.join("retained.txt")).unwrap(),
        b"original"
    );
    let failed = fx
        .post(
            "/lingxi/v1/sessions/missing/attachments/blob",
            json!({"mime":"image/png","dataBase64":"iVBORw0KGgo="}),
        )
        .await;
    assert_eq!(failed.0, 404);
    assert_eq!(
        fx.state
            .session_files()
            .list_files("s-proof")
            .await
            .unwrap()
            .len(),
        before + 1
    );
    fx.finish().await;
}

#[tokio::test]
async fn resource_metadata_ticket_and_content_head_are_owner_bound() {
    let fx = Fixture::start("resource-owner").await;
    let payload = fx.workspace.join("content.txt");
    std::fs::write(&payload, b"owner content").unwrap();
    let attached = fx
        .post(
            "/lingxi/v1/sessions/s-proof/attachments/local",
            json!({"path":payload}),
        )
        .await;
    assert_eq!(attached.0, 201);
    let id = lingxi_service::resources::resource_id_for_session_file_id(
        attached.1["file"]["fileId"].as_str().unwrap(),
    )
    .unwrap();
    let token = fx.state.auth().local_token();
    let metadata = fx
        .request(
            "GET",
            &format!("/lingxi/v1/resources/{id}"),
            None,
            Some(token.clone()),
        )
        .await;
    assert_eq!(metadata.0, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&metadata.1).unwrap()["resourceId"],
        id
    );
    let ticket = fx
        .post(&format!("/lingxi/v1/resources/{id}/ticket"), json!({}))
        .await;
    assert_eq!(ticket.0, 200);
    assert!(ticket.1["ticket"].as_str().is_some());
    let path = format!(
        "/lingxi/v1/resources/{id}/content?ticket={}",
        ticket.1["ticket"].as_str().unwrap()
    );
    let get = fx.request("GET", &path, None, Some(token.clone())).await;
    assert_eq!(get.0, 200);
    assert_eq!(get.1, b"owner content");
    let head = fx.request("HEAD", &path, None, Some(token)).await;
    assert_eq!(head.0, 200);
    assert!(head.1.is_empty());
    assert!(head.2.to_lowercase().contains("content-length: 13"));
    let minted = fx
        .post(
            "/lingxi/v1/devices/credentials",
            json!({"userId":"user_intruder","scopes":["chat"]}),
        )
        .await;
    assert_eq!(minted.0, 201);
    let intruder = minted.1["secret"].as_str().unwrap().to_string();
    for (method, path, body) in [
        ("GET", format!("/lingxi/v1/resources/{id}"), None),
        (
            "POST",
            format!("/lingxi/v1/resources/{id}/ticket"),
            Some(json!({})),
        ),
        ("GET", format!("/lingxi/v1/resources/{id}/content"), None),
        ("HEAD", format!("/lingxi/v1/resources/{id}/content"), None),
    ] {
        let denied = fx
            .request(method, &path, body, Some(intruder.clone()))
            .await;
        assert_eq!(denied.0, 403, "{method} {path}: {:?}", denied.1);
        assert!(!String::from_utf8_lossy(&denied.1).contains("owner content"));
    }
    assert_eq!(std::fs::read(&payload).unwrap(), b"owner content");
    fx.finish().await;
}

#[tokio::test]
async fn stage_files_gateway_only_delivers_real_authorized_session_files() {
    use lingxi_kernel::ports::{ToolOutcome, ToolRequest};
    use lingxi_kernel::subagent::SessionPermissionMode;
    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetRef};
    use lingxi_protocol::{AttemptId, ContentBlock, RunId, SessionId, ToolCallId};
    use lingxi_service::toolgateway::{
        CallerSurface, InvocationPermissionContext, ToolInvocationGateway,
    };
    let fx = Fixture::start("stage").await;
    let registry = Arc::new(ToolRegistry::new());
    let clock = Arc::new(lingxi_service::inject::SystemClock);
    let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
        clock.clone(),
    ));
    let budget = SchemaBudget::default();
    let gateway = ToolInvocationGateway::new(
        registry.clone(),
        approvals,
        clock,
        budget,
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    );
    lingxi_service::stagefiles::register_stage_files(
        &registry,
        &gateway,
        fx.state.session_files().clone(),
        Arc::new(
            lingxi_service::resourceaccess::ResourceAccess::new(std::slice::from_ref(
                &fx.workspace,
            ))
            .unwrap(),
        ),
        fx.workspace.clone(),
        &budget,
    );
    let target = registry
        .resolve(&ToolTargetRef::ByName {
            name: "stage_files".into(),
        })
        .unwrap();
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("s-proof"),
        run_id: RunId::new("stage-proof"),
        attempt: AttemptId::new("stage-proof#a1"),
        generation: 1,
    };
    let file = fx.workspace.join("delivered.txt");
    std::fs::write(&file, b"actual delivered bytes").unwrap();
    let request = ToolRequest::from_effective_arguments(
        target.as_str(),
        json!({"filepaths":[file]}),
        &budget,
    )
    .unwrap();
    let call = ToolCallId::new("stage-good");
    let prepared = gateway
        .prepare_from_request(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate,
            },
            &call,
            &request,
        )
        .unwrap();
    let result = gateway
        .execute_prepared(&ctx, &call, &prepared.handle)
        .await
        .unwrap();
    let ToolOutcome::Success { result } = result.outcome else {
        panic!("应真实交付成功")
    };
    let ContentBlock::Text { text } = &result.content[0] else {
        panic!("应包含文件登记标识")
    };
    let files = fx
        .state
        .session_files()
        .list_files("s-proof")
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    assert!(text.contains(&files[0].file_id));
    assert_eq!(
        std::fs::read(&files[0].real_path).unwrap(),
        b"actual delivered bytes"
    );
    // 同一交付结果的展示字段和消费者链都须来自真实登记文件。
    let delivered: Value = serde_json::from_str(text.strip_prefix("[SessionFile] ").unwrap())
        .expect("真实交付文本包含结构化文件展示信息");
    assert_eq!(delivered["fileId"], files[0].file_id);
    assert_eq!(delivered["filename"], "delivered.txt");
    assert_eq!(delivered["filename"], files[0].filename);
    let resource_id =
        lingxi_service::resources::resource_id_for_session_file_id(&files[0].file_id).unwrap();
    let token = fx.state.auth().local_token();
    let metadata = fx
        .request(
            "GET",
            &format!("/lingxi/v1/resources/{resource_id}"),
            None,
            Some(token.clone()),
        )
        .await;
    assert_eq!(metadata.0, 200);
    let metadata: Value = serde_json::from_slice(&metadata.1).unwrap();
    assert_eq!(metadata["resourceId"], resource_id);
    assert_eq!(metadata["fileId"], delivered["fileId"]);
    assert_eq!(metadata["filename"], delivered["filename"]);
    assert_eq!(metadata["displayName"], "delivered.txt");
    assert_eq!(metadata["size"], b"actual delivered bytes".len() as u64);
    let ticket = fx
        .post(
            &format!("/lingxi/v1/resources/{resource_id}/ticket"),
            json!({}),
        )
        .await;
    assert_eq!(ticket.0, 200);
    let content_path = format!(
        "{}?ticket={}",
        metadata["links"]["content"].as_str().unwrap(),
        ticket.1["ticket"].as_str().unwrap()
    );
    let downloaded = fx
        .request("GET", &content_path, None, Some(token.clone()))
        .await;
    assert_eq!(downloaded.0, 200);
    assert_eq!(downloaded.1, b"actual delivered bytes");
    assert!(downloaded.2.to_lowercase().contains("delivered.txt"));
    let head = fx.request("HEAD", &content_path, None, Some(token)).await;
    assert_eq!(head.0, 200);
    assert!(head.1.is_empty());
    let minted = fx
        .post(
            "/lingxi/v1/devices/credentials",
            json!({"userId":"stage_intruder","scopes":["chat"]}),
        )
        .await;
    assert_eq!(minted.0, 201);
    let intruder = minted.1["secret"].as_str().unwrap().to_string();
    for (method, path, body) in [
        ("GET", format!("/lingxi/v1/resources/{resource_id}"), None),
        (
            "POST",
            format!("/lingxi/v1/resources/{resource_id}/ticket"),
            Some(json!({})),
        ),
        ("GET", content_path.clone(), None),
        ("HEAD", content_path.clone(), None),
    ] {
        let refused = fx
            .request(method, &path, body, Some(intruder.clone()))
            .await;
        assert_eq!(refused.0, 403, "{method} {path}: {:?}", refused.1);
        assert!(!String::from_utf8_lossy(&refused.1).contains("actual delivered bytes"));
    }
    let anonymous = fx.request("GET", &content_path, None, None).await;
    assert_eq!(anonymous.0, 401);
    assert!(!String::from_utf8_lossy(&anonymous.1).contains("actual delivered bytes"));
    assert_eq!(std::fs::read(&file).unwrap(), b"actual delivered bytes");
    assert_eq!(
        fx.state
            .session_files()
            .list_files("s-proof")
            .await
            .unwrap()
            .len(),
        1
    );
    for (index, args) in [
        json!({"filepaths":[fx.workspace.join("missing.txt")]}),
        json!({"filepaths":["relative.txt"]}),
        json!({"fileIds":["sf_missing"]}),
    ]
    .into_iter()
    .enumerate()
    {
        let request =
            ToolRequest::from_effective_arguments(target.as_str(), args, &budget).unwrap();
        let call = ToolCallId::new(format!("stage-bad-{index}"));
        if let Ok(prepared) = gateway.prepare_from_request(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate,
            },
            &call,
            &request,
        ) {
            let result = gateway
                .execute_prepared(&ctx, &call, &prepared.handle)
                .await
                .unwrap();
            assert!(matches!(result.outcome, ToolOutcome::Failed { .. }));
        }
        assert_eq!(
            fx.state
                .session_files()
                .list_files("s-proof")
                .await
                .unwrap()
                .len(),
            1
        );
    }
    // 正常公开入口缺少会话时必须在启动模型/工具前拒绝，不能产生交付登记。
    let before_missing = stage_registration_counts(&fx).await;
    let absent = fx
        .post(
            "/lingxi/v1/sessions/http-absent-session/execute",
            json!({"input":"stage_files should never start without a session"}),
        )
        .await;
    assert_eq!(absent.0, 404, "{absent:?}");
    assert_eq!(absent.1["code"], "not_found");
    assert_eq!(stage_registration_counts(&fx).await, before_missing);

    // 实际SQLite拒绝登记：来源文件有效且已授权，不能用路径错误替代登记失败。
    let registration_file = fx.workspace.join("unregistrable.txt");
    std::fs::write(&registration_file, b"bytes whose registration must fail").unwrap();
    let registration_file = registration_file.canonicalize().unwrap();
    let db = rusqlite::Connection::open(fx.root.join("home/lingxi-service/data/runs.db")).unwrap();
    db.execute_batch(&format!(
        "CREATE TRIGGER r06_stage_registration_refused BEFORE INSERT ON session_files \
         WHEN NEW.owner_session_id='s-proof' AND NEW.real_path='{}' \
         BEGIN SELECT RAISE(ABORT, 'original_stage_registration_refused'); END;",
        registration_file.to_string_lossy().replace('\'', "''")
    ))
    .unwrap();
    let before_registration = stage_registration_counts(&fx).await;
    let request = ToolRequest::from_effective_arguments(
        target.as_str(),
        json!({"filepaths":[registration_file]}),
        &budget,
    )
    .unwrap();
    let call = ToolCallId::new("stage-registration-refused");
    let prepared = gateway
        .prepare_from_request(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate,
            },
            &call,
            &request,
        )
        .unwrap();
    let registration_result = gateway
        .execute_prepared(&ctx, &call, &prepared.handle)
        .await
        .unwrap();
    db.execute_batch("DROP TRIGGER r06_stage_registration_refused;")
        .unwrap();
    drop(db);
    match registration_result.outcome {
        ToolOutcome::Failed { error } => assert!(
            error
                .message
                .contains("original_stage_registration_refused"),
            "{error:?}"
        ),
        outcome => panic!("真实登记失败不得宣称已交付：{outcome:?}"),
    }
    assert_eq!(stage_registration_counts(&fx).await, before_registration);
    assert_eq!(
        std::fs::read(&registration_file).unwrap(),
        b"bytes whose registration must fail"
    );

    // 缺失、空白或不属于当前主体的身份必须失败，且不能产生会话、文件或引用。
    for (label, principal, session_id) in [
        ("missing", ctx.principal.clone(), "missing-session"),
        ("empty", ctx.principal.clone(), ""),
        ("whitespace", ctx.principal.clone(), "   "),
        (
            "foreign",
            lingxi_kernel::Principal::Device {
                device_id: "stage-foreign-device".into(),
                user_id: "stage_intruder".into(),
            },
            "s-proof",
        ),
    ] {
        let mut invalid_context = ctx.clone();
        invalid_context.principal = principal;
        invalid_context.session_id = SessionId::new(session_id);
        let request = ToolRequest::from_effective_arguments(
            target.as_str(),
            json!({"filepaths":[file]}),
            &budget,
        )
        .unwrap();
        let call = ToolCallId::new(format!("stage-invalid-session-{label}"));
        let before = stage_registration_counts(&fx).await;
        let outcome = match gateway.prepare_from_request(
            &invalid_context,
            CallerSurface::UserRun,
            "agent",
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate,
            },
            &call,
            &request,
        ) {
            Ok(prepared) => {
                gateway
                    .execute_prepared(&invalid_context, &call, &prepared.handle)
                    .await
                    .unwrap()
                    .outcome
            }
            Err(refusal) => ToolOutcome::Failed {
                error: refusal.to_tool_error(),
            },
        };
        let ToolOutcome::Failed { error } = outcome else {
            panic!("{label}会话身份不得成功登记或宣称已交付：{outcome:?}")
        };
        assert!(!error.message.is_empty());
        assert_eq!(stage_registration_counts(&fx).await, before, "{label}");
        assert_eq!(std::fs::read(&file).unwrap(), b"actual delivered bytes");
    }
    // 已交付身份的载荷失效后必须拒绝，而不是继续给出成功交付标识。
    std::fs::remove_file(&file).unwrap();
    let request = ToolRequest::from_effective_arguments(
        target.as_str(),
        json!({"fileIds":[files[0].file_id]}),
        &budget,
    )
    .unwrap();
    let call = ToolCallId::new("stage-expired-payload");
    let prepared = gateway
        .prepare_from_request(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate,
            },
            &call,
            &request,
        )
        .unwrap();
    let result = gateway
        .execute_prepared(&ctx, &call, &prepared.handle)
        .await
        .unwrap();
    assert!(matches!(result.outcome, ToolOutcome::Failed { .. }));
    fx.finish().await;
}

#[tokio::test]
async fn local_resource_rename_move_trash_and_conflicts_match_real_bytes() {
    let fx = Fixture::start("mutations").await;
    let source = fx.workspace.join("source.txt");
    let renamed = fx.workspace.join("renamed.txt");
    let moved = fx.workspace.join("moved.txt");
    let occupied = fx.workspace.join("occupied.txt");
    std::fs::write(&source, b"original mutation bytes").unwrap();
    std::fs::write(&occupied, b"existing target bytes").unwrap();
    for (operation, from, to) in [("rename", &source, &renamed), ("move", &renamed, &moved)] {
        let result = fx
            .post(
                &format!("/lingxi/v1/resource-io/{operation}"),
                json!({
                    "from":{"kind":"local-file","path":from}, "to":{"kind":"local-file","path":to}
                }),
            )
            .await;
        assert_eq!(result.0, 200, "{operation}: {result:?}");
        assert!(!from.exists());
        assert_eq!(std::fs::read(to).unwrap(), b"original mutation bytes");
        let conflict = fx.post(&format!("/lingxi/v1/resource-io/{operation}"), json!({
            "from":{"kind":"local-file","path":to}, "to":{"kind":"local-file","path":occupied}
        })).await;
        assert_eq!(conflict.0, 409, "{operation}: {conflict:?}");
        assert_eq!(std::fs::read(to).unwrap(), b"original mutation bytes");
        assert_eq!(std::fs::read(&occupied).unwrap(), b"existing target bytes");
    }
    let trash = fx
        .post(
            "/lingxi/v1/resource-io/trash",
            json!({"resource":{"kind":"local-file","path":moved}}),
        )
        .await;
    assert_eq!(trash.0, 200, "{trash:?}");
    assert!(!moved.exists());
    let retained = fx
        .root
        .join("home/trash/resource-io")
        .join(trash.1["trashId"].as_str().unwrap())
        .join("payload");
    assert_eq!(std::fs::read(retained).unwrap(), b"original mutation bytes");
    let missing = fx
        .post(
            "/lingxi/v1/resource-io/trash",
            json!({"resource":{"kind":"local-file","path":moved}}),
        )
        .await;
    assert_eq!(missing.0, 404);
    assert_eq!(std::fs::read(&occupied).unwrap(), b"existing target bytes");
    fx.finish().await;
}

#[tokio::test]
async fn resourceio_write_conflict_refuses_external_change_at_commit_boundary() {
    let fx = Fixture::start("write-conflict").await;
    for operation in ["write", "write-expected-version"] {
        let file = fx.workspace.join(format!("contended-{operation}.txt"));
        std::fs::write(&file, b"original").unwrap();
        let reference = json!({"kind":"local-file","path":file});
        let stat = fx
            .post("/lingxi/v1/resource-io/stat", json!({"resource":reference}))
            .await;
        assert_eq!(stat.0, 200, "{stat:?}");
        let events_before = fx.state.resource_io().events_since(0);
        fx.state
            .resource_io()
            .file_safety()
            .set_test_hook(Some(Arc::new(move |path, step| {
                if step == "before_replace" {
                    std::fs::write(path, b"external user change")
                        .map_err(|error| error.to_string())?;
                }
                Ok(())
            })));
        let response = fx
            .post(
                &format!("/lingxi/v1/resource-io/{operation}"),
                json!({"resource":reference,"content":"tool replacement",
                    "expectedVersion":stat.1["version"]}),
            )
            .await;
        assert_eq!(response.0, 409, "{operation}: {response:?}");
        if operation == "write" {
            assert_eq!(response.1["code"], "conflict");
            assert!(response
                .1
                .to_string()
                .contains("file_changed_or_not_authorized"));
        } else {
            assert_eq!(response.1["ok"], false);
            assert_eq!(response.1["conflict"], true);
        }
        assert_eq!(std::fs::read(&file).unwrap(), b"external user change");
        assert_eq!(fx.state.resource_io().events_since(0), events_before);
        fx.state.resource_io().file_safety().set_test_hook(None);
    }
    fx.finish().await;
}

#[tokio::test]
async fn registered_read_reports_missing_and_decode_failure_without_foreign_content() {
    use lingxi_kernel::ports::{ToolOutcome, ToolRequest};
    use lingxi_kernel::subagent::SessionPermissionMode;
    use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry};
    use lingxi_protocol::{AttemptId, ContentBlock, ErrorCode, RunId, SessionId, ToolCallId};
    use lingxi_service::toolgateway::{
        CallerSurface, InvocationPermissionContext, ToolInvocationGateway,
    };

    let fx = Fixture::start("read-errors").await;
    let budget = SchemaBudget::default();
    let registry = Arc::new(ToolRegistry::new());
    let clock = Arc::new(lingxi_service::inject::SystemClock);
    let approvals = Arc::new(lingxi_service::approval_service::ApprovalService::new(
        clock.clone(),
    ));
    let gateway = ToolInvocationGateway::new(
        registry.clone(),
        approvals,
        clock.clone(),
        budget,
        lingxi_service::toolgateway::DEFAULT_PREPARED_TTL_MS,
        lingxi_service::toolgateway::DEFAULT_LIVE_PREPARED_CAP,
    );
    let core = lingxi_service::filetools::register_core_file_tools(
        &registry,
        &gateway,
        Arc::new(
            lingxi_service::resourceaccess::ResourceAccess::new(std::slice::from_ref(
                &fx.workspace,
            ))
            .unwrap(),
        ),
        fx.workspace.clone(),
        clock,
        Arc::new(lingxi_service::filetools::NoopChangeLog),
        &budget,
    );
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("s-proof"),
        run_id: RunId::new("read-errors-proof"),
        attempt: AttemptId::new("read-errors-proof#a1"),
        generation: 1,
    };
    let private = fx.root.join("private.txt");
    std::fs::write(&private, b"PRIVATE-SENTINEL-NOT-DISCLOSED").unwrap();
    let undecodable = fx.workspace.join("undecodable.txt");
    let undecodable_bytes = b"\xff\xfeRAW-CONTENT-NOT-TEXT";
    std::fs::write(&undecodable, undecodable_bytes).unwrap();

    for (index, path, expected) in [
        (
            0,
            fx.workspace.join("missing.txt"),
            Some(ErrorCode::NotFound),
        ),
        (1, fx.workspace.clone(), Some(ErrorCode::InvalidMessage)),
        (2, undecodable.clone(), None),
        (3, private.clone(), Some(ErrorCode::Forbidden)),
    ] {
        let request = ToolRequest::from_effective_arguments(
            core.read_target.as_str(),
            json!({"path":path}),
            &budget,
        )
        .unwrap();
        let call = ToolCallId::new(format!("read-errors-{index}"));
        let prepared = gateway.prepare_from_request(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            InvocationPermissionContext::UserSession {
                mode: SessionPermissionMode::Operate,
            },
            &call,
            &request,
        );
        if index == 3 {
            let refusal = prepared.expect_err("越权在真实授权闸拒绝");
            assert!(!format!("{refusal:?}").contains("PRIVATE-SENTINEL-NOT-DISCLOSED"));
            continue;
        }
        let prepared = prepared.unwrap();
        let result = gateway
            .execute_prepared(&ctx, &call, &prepared.handle)
            .await
            .unwrap();
        if let Some(expected) = expected {
            let ToolOutcome::Failed { error } = result.outcome else {
                panic!("不存在或目录输入必须明确失败")
            };
            assert_eq!(error.code, expected);
            assert!(
                error
                    .message
                    .contains(if index == 0 { "ENOENT" } else { "directory" }),
                "{}",
                error.message
            );
            assert!(!error.message.contains("PRIVATE-SENTINEL-NOT-DISCLOSED"));
        } else {
            let ToolOutcome::Success { result } = result.outcome else {
                panic!("不能解码的文件只能返回明确元数据说明")
            };
            let text = result
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains("Text decoding failed"));
            assert!(text.contains("binary or non-UTF-8"));
            assert!(text.contains("sha256"));
            assert!(!text.contains("RAW-CONTENT-NOT-TEXT"));
            assert!(!text.contains("PRIVATE-SENTINEL-NOT-DISCLOSED"));
            assert!(result.resource_refs.is_empty());
        }
    }
    assert_eq!(
        std::fs::read(&private).unwrap(),
        b"PRIVATE-SENTINEL-NOT-DISCLOSED"
    );
    assert_eq!(std::fs::read(&undecodable).unwrap(), undecodable_bytes);
    assert!(!fx.workspace.join("missing.txt").exists());
    assert!(fx
        .state
        .session_files()
        .list_files("s-proof")
        .await
        .unwrap()
        .is_empty());
    fx.finish().await;
}

#[tokio::test]
async fn file_history_failures_and_conflict_preserve_user_bytes_and_snapshot_rows() {
    use lingxi_service::filehistory::RecordOutcome;

    let fx = Fixture::start("history-failures").await;
    let file = fx.workspace.join("restore.txt");
    let private = fx.root.join("private.txt");
    std::fs::write(&file, b"current user bytes").unwrap();
    std::fs::write(&private, b"private bytes stay outside workspace").unwrap();
    let old = fx
        .state
        .file_history()
        .record_snapshot("restore.txt", b"historic bytes", "restore")
        .await
        .unwrap();
    let RecordOutcome::Inserted(old_id) = old else {
        panic!("新库必须实际创建旧快照")
    };
    let corrupt = fx
        .state
        .file_history()
        .record_snapshot("../private.txt", b"forbidden snapshot bytes", "restore")
        .await
        .unwrap();
    let RecordOutcome::Inserted(corrupt_id) = corrupt else {
        panic!("腐坏路径负控必须实际持久保存")
    };
    let history = fx.state.file_history();
    let files_before = history.list_files().await.unwrap();
    let versions_before = history.list_versions("restore.txt").await.unwrap();
    let old_before = history.get_snapshot_content(old_id).await.unwrap();
    let corrupt_before = history.get_snapshot_content(corrupt_id).await.unwrap();
    let events_before = fx.state.resource_io().events_since(0);

    for (body, expected_status, expected_error) in [
        (json!({"snapshotId":old_id}), 400, "agentId required"),
        (
            json!({"agentId":"agent","snapshotId":0}),
            400,
            "invalid snapshotId",
        ),
        (
            json!({"agentId":"agent","snapshotId":999999}),
            404,
            "not found",
        ),
        (
            json!({"agentId":"agent","snapshotId":corrupt_id}),
            500,
            "corrupt snapshot path",
        ),
    ] {
        let result = fx.post("/lingxi/v1/file-history/restore", body).await;
        assert_eq!(result.0, expected_status, "{result:?}");
        assert!(result.1["error"].as_str().unwrap().contains(expected_error));
        assert_eq!(std::fs::read(&file).unwrap(), b"current user bytes");
        assert_eq!(
            std::fs::read(&private).unwrap(),
            b"private bytes stay outside workspace"
        );
        assert_eq!(history.list_files().await.unwrap(), files_before);
        assert_eq!(
            history.list_versions("restore.txt").await.unwrap(),
            versions_before
        );
        assert_eq!(
            history.get_snapshot_content(old_id).await.unwrap(),
            old_before
        );
        assert_eq!(
            history.get_snapshot_content(corrupt_id).await.unwrap(),
            corrupt_before
        );
        assert_eq!(fx.state.resource_io().events_since(0), events_before);
    }
    let unauthorized = fx
        .request(
            "POST",
            "/lingxi/v1/file-history/restore",
            Some(json!({"agentId":"agent","snapshotId":old_id})),
            None,
        )
        .await;
    assert_eq!(unauthorized.0, 401);
    let absolute = fx
        .request(
            "GET",
            "/lingxi/v1/file-history/versions?agentId=agent&relPath=/absolute.txt",
            None,
            Some(fx.state.auth().local_token()),
        )
        .await;
    assert_eq!(absolute.0, 400);
    assert!(String::from_utf8_lossy(&absolute.1).contains("invalid relPath"));
    assert_eq!(std::fs::read(&file).unwrap(), b"current user bytes");
    assert_eq!(history.list_files().await.unwrap(), files_before);

    // 真实写入作业在临时文件阶段失败，快照和目标字节均不能冒充成功改变。
    fx.state
        .resource_io()
        .file_safety()
        .set_test_hook(Some(Arc::new(|_, step| {
            if step == "temp_write" {
                return Err("proof storage write failure".to_string());
            }
            Ok(())
        })));
    let failed = fx
        .post(
            "/lingxi/v1/file-history/restore",
            json!({"agentId":"agent","snapshotId":old_id}),
        )
        .await;
    assert_eq!(failed.0, 500, "{failed:?}");
    assert!(failed.1["error"]
        .as_str()
        .unwrap()
        .contains("proof storage write failure"));
    assert_eq!(std::fs::read(&file).unwrap(), b"current user bytes");
    assert_eq!(history.list_files().await.unwrap(), files_before);
    assert_eq!(
        history.list_versions("restore.txt").await.unwrap(),
        versions_before
    );
    assert_eq!(
        history.get_snapshot_content(old_id).await.unwrap(),
        old_before
    );
    assert_eq!(fx.state.resource_io().events_since(0), events_before);

    // 正确版本通过读取后，用户在真实提交边界改动目标，恢复不能覆盖后改。
    fx.state
        .resource_io()
        .file_safety()
        .set_test_hook(Some(Arc::new(|path, step| {
            if step == "before_replace" {
                std::fs::write(path, b"actual concurrent user edit")
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        })));
    let conflict = fx
        .post(
            "/lingxi/v1/file-history/restore",
            json!({"agentId":"agent","snapshotId":old_id}),
        )
        .await;
    assert_eq!(conflict.0, 500, "{conflict:?}");
    assert!(conflict.1["error"]
        .as_str()
        .unwrap()
        .contains("file_changed_or_not_authorized"));
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"actual concurrent user edit"
    );
    assert_eq!(
        std::fs::read(&private).unwrap(),
        b"private bytes stay outside workspace"
    );
    assert_eq!(history.list_files().await.unwrap(), files_before);
    assert_eq!(
        history.list_versions("restore.txt").await.unwrap(),
        versions_before
    );
    assert_eq!(
        history.get_snapshot_content(old_id).await.unwrap(),
        old_before
    );
    assert_eq!(
        history.get_snapshot_content(corrupt_id).await.unwrap(),
        corrupt_before
    );
    assert_eq!(fx.state.resource_io().events_since(0), events_before);
    fx.state.resource_io().file_safety().set_test_hook(None);

    let db = rusqlite::Connection::open(fx.root.join("home/lingxi-service/data/runs.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER r06_restore_snapshot_refused BEFORE INSERT ON file_history_snapshots \
         BEGIN SELECT RAISE(ABORT, 'proof restore snapshot storage failure'); END;",
    )
    .unwrap();
    let capture_failed = fx
        .post(
            "/lingxi/v1/file-history/restore",
            json!({"agentId":"agent","snapshotId":old_id}),
        )
        .await;
    assert_eq!(capture_failed.0, 500, "{capture_failed:?}");
    assert!(capture_failed.1["error"]
        .as_str()
        .unwrap()
        .contains("proof restore snapshot storage failure"));
    assert_eq!(capture_failed.1["fileWritten"], true);
    assert_eq!(capture_failed.1["snapshotRecorded"], false);
    assert_eq!(capture_failed.1["relPath"], "restore.txt");
    assert_eq!(std::fs::read(&file).unwrap(), b"historic bytes");
    assert_eq!(
        history.list_versions("restore.txt").await.unwrap(),
        versions_before
    );
    assert_eq!(
        history.get_snapshot_content(old_id).await.unwrap(),
        old_before
    );
    assert_eq!(
        std::fs::read(&private).unwrap(),
        b"private bytes stay outside workspace"
    );
    assert_ne!(fx.state.resource_io().events_since(0), events_before);
    db.execute_batch("DROP TRIGGER r06_restore_snapshot_refused;")
        .unwrap();
    drop(db);

    // 移除失败条件后同一真实恢复入口成功，并且只新增一条恢复快照。
    let restored = fx
        .post(
            "/lingxi/v1/file-history/restore",
            json!({"agentId":"agent","snapshotId":old_id}),
        )
        .await;
    assert_eq!(restored.0, 200, "{restored:?}");
    assert_eq!(restored.1["ok"], true);
    assert_eq!(restored.1["relPath"], "restore.txt");
    assert_eq!(std::fs::read(&file).unwrap(), b"historic bytes");
    let versions = history.list_versions("restore.txt").await.unwrap();
    assert_eq!(versions.len(), versions_before.len() + 1);
    assert_eq!(versions[0].origin, "restore");
    let snapshot = history
        .get_snapshot_content(versions[0].id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.content, b"historic bytes");
    let repeated = fx
        .post(
            "/lingxi/v1/file-history/restore",
            json!({"agentId":"agent","snapshotId":old_id}),
        )
        .await;
    assert_eq!(repeated.0, 200, "{repeated:?}");
    let repeated_versions = history.list_versions("restore.txt").await.unwrap();
    assert_eq!(repeated_versions.len(), versions.len() + 1);
    assert_eq!(repeated_versions[0].origin, "restore");
    assert_eq!(
        history
            .get_snapshot_content(repeated_versions[0].id)
            .await
            .unwrap()
            .unwrap()
            .content,
        b"historic bytes"
    );
    assert_eq!(
        history.get_snapshot_content(old_id).await.unwrap(),
        old_before
    );
    assert_eq!(
        std::fs::read(&private).unwrap(),
        b"private bytes stay outside workspace"
    );
    assert!(fx
        .state
        .session_files()
        .list_files("s-proof")
        .await
        .unwrap()
        .is_empty());
    fx.finish().await;
}

#[tokio::test]
async fn subscribe_without_local_provider_refuses_on_its_real_entry_without_residue() {
    // 复用现有无工作区配置：真实组合根的local_fs provider不注册。
    let fx = Fixture::start_with_workspace("subscribe-provider-absent", false).await;
    let watched = fx.workspace.join("available-on-disk.txt");
    std::fs::write(&watched, b"existing watched bytes").unwrap();
    let events_before = fx.state.resource_io().events_since(0);
    assert_eq!(fx.state.resource_io().diagnostics()["subscriptions"], 0);
    assert!(fx.state.resource_io().diagnostics()["watches"]
        .as_array()
        .unwrap()
        .is_empty());
    let response = fx
        .post(
            "/lingxi/v1/resource-io/subscribe",
            json!({"resources":[{"kind":"local-file","path":watched}],"purpose":"proof"}),
        )
        .await;
    assert_eq!(response.0, 501, "{response:?}");
    assert!(response.1.to_string().contains("provider_not_available"));
    assert_ne!(response.1["ok"], true);
    assert!(response.1.get("subscriptionId").is_none());
    let diagnostics = fx.state.resource_io().diagnostics();
    assert_eq!(diagnostics["subscriptions"], 0);
    assert!(diagnostics["watches"].as_array().unwrap().is_empty());
    assert_eq!(fx.state.resource_io().events_since(0), events_before);
    assert_eq!(std::fs::read(&watched).unwrap(), b"existing watched bytes");
    assert!(fx
        .state
        .session_files()
        .list_files("s-proof")
        .await
        .unwrap()
        .is_empty());
    fx.finish().await;
}
