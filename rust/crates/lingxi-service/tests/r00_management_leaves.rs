//! R00 管理叶的真实 Rust 服务证据：合成数据根、真实 TCP、前后持久化对照。
//! 这里只登记实际覆盖的案例；未覆盖的原断言仍由阶段门禁保持 BLOCKED。

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lingxi_service::{
    prepare_layout, run, CliOptions, HomeSource, ManualClock, NetworkMode, ServeOutcome,
    ServiceClock, ServiceConfig, ServiceDeps, ServiceError, ServiceState,
};
use qrcodegen::{QrCode, QrCodeEcc};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

struct TestServer {
    addr: SocketAddr,
    home: PathBuf,
    runtime: PathBuf,
    token: String,
    clock: std::sync::Arc<ManualClock>,
    clock_epoch: u64,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<ServeOutcome, ServiceError>>,
}

struct HttpResponse {
    status: u16,
    headers: String,
    body: String,
}

impl HttpResponse {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("response must be JSON")
    }
}

async fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> HttpResponse {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect to real service");
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
    stream
        .write_all(head.as_bytes())
        .await
        .expect("write request");
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).await.expect("write body");
    }
    let mut raw = Vec::new();
    loop {
        let mut chunk = [0_u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => break,
            Ok(size) => raw.extend_from_slice(&chunk[..size]),
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                break
            }
            Err(err) => panic!("read response: {err}"),
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let (headers, body) = text
        .split_once("\r\n\r\n")
        .expect("HTTP header/body separator");
    let status = headers
        .split_whitespace()
        .nth(1)
        .expect("HTTP status")
        .parse()
        .expect("numeric HTTP status");
    HttpResponse {
        status,
        headers: headers.into(),
        body: body.into(),
    }
}

async fn start_server() -> TestServer {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let home = std::env::temp_dir().join(format!(
        "lingxi-r02-r00-management-{}-{unique}",
        std::process::id()
    ));
    assert!(!home.exists(), "test home must be fresh");
    let config = ServiceConfig {
        bind_addr: "127.0.0.1:0".parse().expect("static address"),
        data_home: home.clone(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    };
    let clock_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64;
    let clock = std::sync::Arc::new(ManualClock::new(clock_epoch));
    start_server_with_config(config, clock).await
}

async fn start_server_with_config(
    config: ServiceConfig,
    clock: std::sync::Arc<ManualClock>,
) -> TestServer {
    let home = config.data_home.clone();
    let layout = prepare_layout(&home).expect("prepare synthetic home");
    let runtime = layout.runtime_dir.clone();
    let clock_epoch = clock.now_unix_ms();
    let state = ServiceState::bootstrap_with_deps(
        config,
        &layout,
        ServiceDeps {
            clock: clock.clone(),
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("bootstrap");
    let token = state.auth().local_token();
    let (stop, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            state,
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
    let addr = match ready_rx.await {
        Ok(addr) => addr,
        Err(err) => panic!(
            "real service READY failed: {err}; run result: {:?}",
            handle.await
        ),
    };
    TestServer {
        addr,
        home,
        runtime,
        token,
        clock,
        clock_epoch,
        stop,
        handle,
    }
}

impl TestServer {
    fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn owner(&self, method: &str, path: &str, body: Option<&str>) -> HttpResponse {
        let bearer = self.bearer();
        request(self.addr, method, path, &[("Authorization", &bearer)], body).await
    }

    fn manager_bytes(&self) -> Vec<u8> {
        std::fs::read(self.runtime.join("management.json")).expect("management state file")
    }

    fn manager_json(&self) -> Value {
        serde_json::from_slice(&self.manager_bytes()).expect("management state JSON")
    }

    fn registry_bytes(&self) -> (Vec<u8>, Vec<u8>) {
        let devices = std::fs::read(self.runtime.join(lingxi_service::auth::DEVICES_FILE))
            .expect("device registry");
        let credentials = std::fs::read(
            self.runtime
                .join(lingxi_service::auth::DEVICE_CREDENTIALS_FILE),
        )
        .expect("credential registry");
        (devices, credentials)
    }

    async fn stop(self) -> PathBuf {
        self.stop.send(()).expect("signal service stop");
        tokio::time::timeout(Duration::from_secs(10), self.handle)
            .await
            .expect("bounded shutdown")
            .expect("service task")
            .expect("clean service shutdown");
        self.home
    }
}

fn record(cases: &mut Vec<Value>, name: &str, observed: Value) {
    cases.push(json!({"case": name, "expect": 1, "actual": 1, "ok": true, "observed": observed}));
    // 中途断言失败时也保留截至失败前已经完成的真实案例；生产者仍按缺项判 FAIL。
    write_cases(cases);
}

fn assert_me_projection(me: &Value, kind: &str, credential_kind: &str, user_id: &str) {
    assert_eq!(me["kind"], kind);
    assert_eq!(me["credentialKind"], credential_kind);
    assert_eq!(me["userId"], user_id);
    assert_eq!(me["serverNodeKind"], "lingxi-service");
    assert_eq!(me["version"], me["serverVersion"]);
    assert!(me["serverVersion"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(me["serverId"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(me["studioId"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    for field in [
        "principalId",
        "kind",
        "userId",
        "studioId",
        "serverNodeId",
        "credentialKind",
        "scopes",
    ] {
        assert_eq!(
            me[field], me["principal"][field],
            "{field} must match the authenticated principal"
        );
    }
    assert_eq!(me["serverId"], me["serverNodeId"]);
    let scopes = me["scopes"].as_array().expect("principal scopes array");
    let expected: std::collections::BTreeSet<&str> = scopes
        .iter()
        .map(|value| value.as_str().expect("scope string"))
        .flat_map(|scope| [scope, scope.split('.').next().expect("namespace")])
        .collect();
    let actual: std::collections::BTreeSet<&str> = me["capabilities"]
        .as_array()
        .expect("capabilities array")
        .iter()
        .map(|value| value.as_str().expect("capability string"))
        .collect();
    assert_eq!(
        actual, expected,
        "capabilities must follow only this principal's scopes"
    );
    assert!(me.get("secret").is_none() && me.get("token").is_none());
}

fn audit_count(manager: &Value) -> usize {
    manager["audit"].as_array().expect("audit list").len()
}

fn block_runtime_writes(runtime: &Path) -> std::fs::Permissions {
    let prior = std::fs::metadata(runtime)
        .expect("synthetic runtime metadata")
        .permissions();
    let mut blocked = prior.clone();
    blocked.set_readonly(true);
    std::fs::set_permissions(runtime, blocked).expect("block synthetic runtime writes");
    prior
}

fn restore_runtime_writes(runtime: &Path, prior: std::fs::Permissions) {
    std::fs::set_permissions(runtime, prior).expect("restore synthetic runtime permissions");
}

fn assert_svg_encodes_address(svg: &str, address: &str) {
    let qr = QrCode::encode_text(address, QrCodeEcc::Medium).expect("expected QR content encodes");
    let actual_path = svg
        .split("<path fill=\"#000\" d=\"")
        .nth(1)
        .and_then(|tail| tail.split_once('"'))
        .map(|(path, _)| path)
        .expect("black module path");
    let mut actual = HashSet::new();
    for segment in actual_path.split('M').filter(|segment| !segment.is_empty()) {
        let coords = segment
            .strip_suffix("h1v1h-1z")
            .expect("unit-size black module");
        let (x, y) = coords.split_once(',').expect("QR module coordinates");
        actual.insert((
            x.parse::<i32>().expect("x") - 1,
            y.parse::<i32>().expect("y") - 1,
        ));
    }
    let mut expected = HashSet::new();
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            if qr.get_module(x, y) {
                expected.insert((x, y));
            }
        }
    }
    assert_eq!(
        actual, expected,
        "SVG QR modules must encode the actual mobile address"
    );
}

fn set_cookie(response: &HttpResponse) -> String {
    response
        .headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("set-cookie")
                .then(|| value.trim().split(';').next().unwrap_or("").to_string())
        })
        .filter(|value| value.starts_with("hana_session="))
        .expect("session cookie")
}

fn write_cases(cases: &[Value]) {
    let Ok(path) = std::env::var("R02_MANAGEMENT_CASES_PATH") else {
        return;
    };
    let path = Path::new(&path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("evidence dir");
    }
    let bytes = serde_json::to_vec_pretty(
        &json!({"schema": "lingxi.leaf-case-results.v1", "cases": cases}),
    )
    .expect("serialize management cases");
    std::fs::write(path, [bytes, b"\n".to_vec()].concat()).expect("write management evidence");
}

fn write_standard_audit_evidence(server: &TestServer, log: &[u8], secrets: &[&str]) {
    let Ok(case_path) = std::env::var("R02_MANAGEMENT_CASES_PATH") else {
        return;
    };
    let output = Path::new(&case_path)
        .parent()
        .expect("management evidence directory");
    std::fs::create_dir_all(output).expect("create management evidence directory");
    let device_registry: Value = serde_json::from_slice(&server.registry_bytes().1)
        .expect("credential registry for audit evidence");
    let source = json!({
        "managementIntents": server.manager_json()["audit"],
        "deviceIntents": device_registry["audit"],
    });
    let source_bytes =
        serde_json::to_vec_pretty(&source).expect("serialize committed audit intents");
    for secret in secrets {
        assert!(
            !source_bytes
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()),
            "committed audit intents must not contain plaintext secrets"
        );
    }
    std::fs::write(output.join("security-audit-sources.json"), source_bytes)
        .expect("write audit source evidence");
    std::fs::write(output.join("security-audit.jsonl"), log)
        .expect("write actual standard security audit evidence");
}

fn assert_standard_audit_matches_sources(server: &TestServer, secrets: &[&str]) -> Vec<u8> {
    let path = server.home.join("logs/security-audit.jsonl");
    let bytes = std::fs::read(&path).expect("standard security audit JSONL");
    let registry: Value =
        serde_json::from_slice(&server.registry_bytes().1).expect("credential registry JSON");
    let mut expected: Vec<Value> = server.manager_json()["audit"]
        .as_array()
        .expect("management audit intents")
        .to_vec();
    expected.extend(
        registry["audit"]
            .as_array()
            .expect("device audit intents")
            .iter()
            .cloned(),
    );
    let events: Vec<Value> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("one valid audit JSON object per line"))
        .collect();
    assert_eq!(
        events.len(),
        expected.len(),
        "no missing or duplicate audit projection"
    );
    let mut event_ids = HashSet::new();
    for event in events {
        assert_eq!(event["schemaVersion"], 1);
        assert_eq!(event["result"], "success");
        assert_eq!(event["actor"]["kind"], "local_user");
        assert_eq!(event["actor"]["userId"], "user_local");
        assert_eq!(event["actor"]["connectionKind"], "local");
        assert_eq!(event["actor"]["credentialKind"], "loopback_token");
        assert_eq!(event["actor"]["trustState"], "local");
        let id = event["eventId"].as_str().expect("stable audit event id");
        assert!(id.starts_with("sec_") && event_ids.insert(id.to_owned()));
        let event_time = chrono::DateTime::parse_from_rfc3339(
            event["timestamp"].as_str().expect("audit ISO time"),
        )
        .expect("valid audit timestamp")
        .timestamp_millis();
        let matching = expected
            .iter()
            .position(|source| {
                source["action"] == event["action"]
                    && source["target"] == event["target"]
                    && source["atUnixMs"].as_i64() == Some(event_time)
                    && source
                        .get("metadata")
                        .filter(|value| value.is_object())
                        .cloned()
                        .unwrap_or_else(|| json!({}))
                        == event["metadata"]
            })
            .expect("each standard event matches exactly one committed source intent");
        expected.remove(matching);
    }
    assert!(expected.is_empty(), "all committed intents projected");
    for secret in secrets {
        assert!(
            !bytes
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()),
            "one-time or password secret must not enter standard audit"
        );
    }
    bytes
}

#[tokio::test]
async fn r00_management_positive_and_negative_branches_on_real_service() {
    let server = start_server().await;
    let mut cases = Vec::new();

    let before_me = (server.manager_bytes(), server.registry_bytes());
    let denied_me = request(server.addr, "GET", "/lingxi/v1/me", &[], None).await;
    assert_eq!(denied_me.status, 401);
    assert_eq!((server.manager_bytes(), server.registry_bytes()), before_me);
    record(
        &mut cases,
        "management-me-denied-no-state-change",
        json!({"status": 401, "stateUnchanged": true}),
    );
    let owner_me = server.owner("GET", "/lingxi/v1/me", None).await;
    assert_eq!(owner_me.status, 200);
    let owner_me = owner_me.json();
    assert_me_projection(&owner_me, "local_user", "loopback_token", "user_local");
    record(
        &mut cases,
        "management-me-owner-version-identity-capabilities",
        json!({"status": 200, "kind": "local_user", "projectionMatchesScopes": true}),
    );
    let forged_me = request(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[
            ("Authorization", &server.bearer()),
            ("X-Lingxi-Principal", "principal_forged"),
        ],
        None,
    )
    .await;
    assert_eq!(forged_me.status, 200);
    assert_eq!(forged_me.json(), owner_me);
    record(
        &mut cases,
        "management-me-forged-header-ignored",
        json!({"status": 200, "forgedPrincipalEchoed": false}),
    );

    let summary = server.owner("GET", "/lingxi/v1/access/summary", None).await;
    assert_eq!(summary.status, 200);
    let initial = summary.json();
    assert_eq!(
        initial["account"]["userId"],
        lingxi_service::auth::LOCAL_OWNER_USER_ID
    );
    assert_eq!(initial["devices"], json!([]));
    assert_eq!(initial["credentials"], json!([]));
    record(
        &mut cases,
        "management-summary-initial-real-state",
        json!({"status": 200, "emptyDevices": true}),
    );
    let before = server.manager_bytes();
    let before_registries = server.registry_bytes();
    let stranger = request(server.addr, "GET", "/lingxi/v1/access/summary", &[], None).await;
    assert_ne!(stranger.status, 200);
    assert_eq!(server.manager_bytes(), before);
    assert_eq!(server.registry_bytes(), before_registries);
    record(
        &mut cases,
        "management-summary-unauthorized-no-state-change",
        json!({"status": stranger.status, "stateUnchanged": true}),
    );
    let before_manager = server.manager_bytes();
    let before_registries = server.registry_bytes();
    let device_file = server.runtime.join(lingxi_service::auth::DEVICES_FILE);
    let held_device_file = server.runtime.join("devices.json.r02-test-held");
    std::fs::rename(&device_file, &held_device_file)
        .expect("temporarily withhold only synthetic registry");
    let unavailable_summary = server.owner("GET", "/lingxi/v1/access/summary", None).await;
    let unavailable_list = server.owner("GET", "/lingxi/v1/devices", None).await;
    std::fs::rename(&held_device_file, &device_file).expect("restore synthetic registry");
    assert_eq!(unavailable_summary.status, 500);
    assert_eq!(unavailable_list.status, 500);
    assert!(!unavailable_summary.body.contains("\"devices\":[]"));
    assert!(!unavailable_list.body.contains("\"devices\":[]"));
    assert_eq!(server.manager_bytes(), before_manager);
    assert_eq!(server.registry_bytes(), before_registries);
    record(
        &mut cases,
        "management-summary-registry-failure-not-empty",
        json!({"status": 500, "falseEmpty": false, "stateUnchanged": true}),
    );
    record(
        &mut cases,
        "management-device-list-registry-failure-not-empty",
        json!({"status": 500, "falseEmpty": false, "stateUnchanged": true}),
    );

    let before = server.manager_json();
    let profile = server
        .owner(
            "PUT",
            "/lingxi/v1/access/account/profile",
            Some(r#"{"username":"r02-owner","displayName":"R02 Owner"}"#),
        )
        .await;
    assert_eq!(profile.status, 200);
    assert_eq!(profile.json()["account"]["username"], "r02-owner");
    assert_eq!(profile.json()["account"]["displayName"], "R02 Owner");
    let after = server.manager_json();
    assert_eq!(after["account"]["username"], "r02-owner");
    assert_eq!(after["account"]["displayName"], "R02 Owner");
    assert_eq!(audit_count(&after), audit_count(&before) + 1);
    let profile_audit = after["audit"]
        .as_array()
        .expect("profile audit")
        .last()
        .expect("last profile audit");
    assert_eq!(profile_audit["action"], "access.account.profile.update");
    assert_eq!(
        profile_audit["target"],
        lingxi_service::auth::LOCAL_OWNER_USER_ID
    );
    record(
        &mut cases,
        "management-profile-success-account-audit",
        json!({"status": 200, "accountAndAuditPersisted": true}),
    );
    let before = server.manager_bytes();
    let bad = server
        .owner(
            "PUT",
            "/lingxi/v1/access/account/profile",
            Some(r#"{"username":"bad/name"}"#),
        )
        .await;
    assert_eq!(bad.status, 400);
    let unauthorized = request(
        server.addr,
        "PUT",
        "/lingxi/v1/access/account/profile",
        &[],
        Some(r#"{"username":"intruder"}"#),
    )
    .await;
    assert_ne!(unauthorized.status, 200);
    assert_eq!(server.manager_bytes(), before);
    record(
        &mut cases,
        "management-profile-invalid-and-unauthorized-unchanged",
        json!({"invalid": bad.status, "unauthorized": unauthorized.status, "stateUnchanged": true}),
    );

    let before = server.manager_json();
    let network = server.owner("PUT", "/lingxi/v1/access/network", Some(r#"{"mode":"lan","listenPort":14500,"publicBaseUrl":"https://r02.example.invalid"}"#)).await;
    assert_eq!(network.status, 200);
    let body = network.json();
    assert_eq!(body["network"]["mode"], "lan");
    assert_eq!(body["network"]["restartRequired"], true);
    let after = server.manager_json();
    assert_eq!(after["network"]["mode"], "lan");
    assert_eq!(after["network"]["listenPort"], 14500);
    assert_eq!(
        after["network"]["publicBaseUrl"],
        "https://r02.example.invalid"
    );
    assert_eq!(audit_count(&after), audit_count(&before) + 1);
    let network_audit = after["audit"]
        .as_array()
        .expect("network audit")
        .last()
        .expect("last network audit");
    assert_eq!(network_audit["action"], "access.network.update");
    assert_eq!(network_audit["target"], "server-network");
    record(
        &mut cases,
        "management-network-success-summary-audit",
        json!({"status": 200, "restartRequired": true, "persisted": true}),
    );
    let before = server.manager_bytes();
    let bad = server
        .owner(
            "PUT",
            "/lingxi/v1/access/network",
            Some(r#"{"mode":"lan","listenPort":1}"#),
        )
        .await;
    assert_eq!(bad.status, 400);
    let unauthorized = request(
        server.addr,
        "PUT",
        "/lingxi/v1/access/network",
        &[],
        Some(r#"{"mode":"loopback","listenPort":14500}"#),
    )
    .await;
    assert_ne!(unauthorized.status, 200);
    assert_eq!(server.manager_bytes(), before);
    record(
        &mut cases,
        "management-network-invalid-and-unauthorized-unchanged",
        json!({"invalid": bad.status, "unauthorized": unauthorized.status, "stateUnchanged": true}),
    );

    let qr = server
        .owner("GET", "/lingxi/v1/access/mobile-qr.svg?port=14500", None)
        .await;
    assert_eq!(qr.status, 200);
    assert!(qr.headers.to_ascii_lowercase().contains("image/svg+xml"));
    assert!(qr.body.contains("<svg") && qr.body.contains("<path"));
    assert_svg_encodes_address(&qr.body, "https://r02.example.invalid/mobile/");
    record(
        &mut cases,
        "management-mobile-qr-svg-present",
        json!({"status": 200, "svg": true, "modulesEncodeExactMobileUrl": true}),
    );
    let qr_before = (server.manager_bytes(), server.registry_bytes());
    let qr_denied = request(
        server.addr,
        "GET",
        "/lingxi/v1/access/mobile-qr.svg?port=14500",
        &[],
        None,
    )
    .await;
    assert_ne!(qr_denied.status, 200);
    assert!(!qr_denied.body.contains("<svg"));
    assert_eq!((server.manager_bytes(), server.registry_bytes()), qr_before);
    record(
        &mut cases,
        "management-mobile-qr-unauthorized-no-svg",
        json!({"status": qr_denied.status, "svgDisclosed": false}),
    );
    let cleared_public = server
        .owner(
            "PUT",
            "/lingxi/v1/access/network",
            Some(r#"{"mode":"loopback","listenPort":14500,"publicBaseUrl":null}"#),
        )
        .await;
    assert_eq!(cleared_public.status, 200);
    let unavailable = server
        .owner("GET", "/lingxi/v1/access/mobile-qr.svg?port=14500", None)
        .await;
    assert_eq!(unavailable.status, 400);
    assert!(unavailable.body.contains("lan_address_unavailable"));
    record(
        &mut cases,
        "management-mobile-qr-lan-unavailable-refused",
        json!({"status": 400, "reason": "lan_address_unavailable"}),
    );
    let restored_public = server.owner("PUT", "/lingxi/v1/access/network", Some(r#"{"mode":"lan","listenPort":14500,"publicBaseUrl":"https://r02.example.invalid"}"#)).await;
    assert_eq!(restored_public.status, 200);

    let before_audit = audit_count(&server.manager_json());
    let password = server
        .owner(
            "PUT",
            "/lingxi/v1/access/account/password",
            Some(r#"{"password":"r02-test-password"}"#),
        )
        .await;
    assert_eq!(password.status, 200);
    assert_eq!(password.json()["account"]["passwordSet"], true);
    assert_eq!(
        server.manager_json()["account"]["passwordHash"]
            .as_str()
            .map(str::is_empty),
        Some(false)
    );
    assert_eq!(audit_count(&server.manager_json()), before_audit + 1);
    let password_set_audit = server.manager_json()["audit"]
        .as_array()
        .expect("password set audit")
        .last()
        .expect("last password set audit")
        .clone();
    assert_eq!(
        password_set_audit["action"],
        "access.account.password.update"
    );
    assert_eq!(
        password_set_audit["target"],
        lingxi_service::auth::LOCAL_OWNER_USER_ID
    );
    assert!(!server
        .manager_bytes()
        .windows("r02-test-password".len())
        .any(|bytes| bytes == b"r02-test-password"));
    record(
        &mut cases,
        "management-password-set-account-audit",
        json!({"status": 200, "passwordHashPersisted": true}),
    );
    let before = server.manager_bytes();
    let bad = server
        .owner(
            "PUT",
            "/lingxi/v1/access/account/password",
            Some(r#"{"password":"short"}"#),
        )
        .await;
    assert_eq!(bad.status, 400);
    let unauthorized_set = request(
        server.addr,
        "PUT",
        "/lingxi/v1/access/account/password",
        &[],
        Some(r#"{"password":"intruder-password"}"#),
    )
    .await;
    assert_ne!(unauthorized_set.status, 200);
    assert_eq!(server.manager_bytes(), before);
    record(
        &mut cases,
        "management-password-invalid-keeps-prior",
        json!({"invalid": 400, "unauthorized": unauthorized_set.status, "stateUnchanged": true}),
    );
    let before_fault = server.manager_bytes();
    let held_runtime = server.runtime.with_extension("r02-held-for-write-fault");
    assert!(!held_runtime.exists());
    std::fs::rename(&server.runtime, &held_runtime).expect("withhold only synthetic runtime");
    let failed_profile = server
        .owner(
            "PUT",
            "/lingxi/v1/access/account/profile",
            Some(r#"{"username":"r02-should-not-save"}"#),
        )
        .await;
    let failed_network = server
        .owner(
            "PUT",
            "/lingxi/v1/access/network",
            Some(r#"{"mode":"loopback","listenPort":14600}"#),
        )
        .await;
    let failed_set = server
        .owner(
            "PUT",
            "/lingxi/v1/access/account/password",
            Some(r#"{"password":"r02-should-not-save-password"}"#),
        )
        .await;
    let failed_clear = server
        .owner("DELETE", "/lingxi/v1/access/account/password", None)
        .await;
    std::fs::rename(&held_runtime, &server.runtime).expect("restore synthetic runtime");
    for response in [&failed_profile, &failed_network, &failed_set, &failed_clear] {
        assert_eq!(response.status, 500);
        assert!(response.body.contains("management_store_failure"));
    }
    assert_eq!(server.manager_bytes(), before_fault);
    let after_fault = server.owner("GET", "/lingxi/v1/access/summary", None).await;
    assert_eq!(after_fault.status, 200);
    assert_eq!(after_fault.json()["account"]["username"], "r02-owner");
    assert_eq!(after_fault.json()["account"]["passwordSet"], true);
    assert_eq!(after_fault.json()["network"]["configuredPort"], 14500);
    record(
        &mut cases,
        "management-profile-store-failure-unchanged",
        json!({"status": failed_profile.status, "diskAndLiveStateUnchanged": true}),
    );
    record(
        &mut cases,
        "management-network-store-failure-unchanged",
        json!({"status": failed_network.status, "diskAndLiveStateUnchanged": true}),
    );
    record(
        &mut cases,
        "management-password-set-store-failure-unchanged",
        json!({"status": failed_set.status, "diskAndLiveStateUnchanged": true}),
    );
    record(
        &mut cases,
        "management-password-clear-store-failure-unchanged",
        json!({"status": failed_clear.status, "diskAndLiveStateUnchanged": true}),
    );
    let before_audit = audit_count(&server.manager_json());
    let cleared = server
        .owner("DELETE", "/lingxi/v1/access/account/password", None)
        .await;
    assert_eq!(cleared.status, 200);
    assert_eq!(cleared.json()["account"]["passwordSet"], false);
    assert!(server.manager_json()["account"]["passwordHash"].is_null());
    assert_eq!(audit_count(&server.manager_json()), before_audit + 1);
    let password_clear_audit = server.manager_json()["audit"]
        .as_array()
        .expect("password clear audit")
        .last()
        .expect("last password clear audit")
        .clone();
    assert_eq!(
        password_clear_audit["action"],
        "access.account.password.clear"
    );
    assert_eq!(
        password_clear_audit["target"],
        lingxi_service::auth::LOCAL_OWNER_USER_ID
    );
    record(
        &mut cases,
        "management-password-clear-account-audit",
        json!({"status": 200, "passwordRemoved": true}),
    );
    let before = server.manager_bytes();
    let unauthorized = request(
        server.addr,
        "DELETE",
        "/lingxi/v1/access/account/password",
        &[],
        None,
    )
    .await;
    assert_ne!(unauthorized.status, 200);
    assert_eq!(server.manager_bytes(), before);
    record(
        &mut cases,
        "management-password-clear-unauthorized-keeps-prior",
        json!({"status": unauthorized.status, "stateUnchanged": true}),
    );

    let before_issue_fault = server.registry_bytes();
    let prior_permissions = block_runtime_writes(&server.runtime);
    let failed_mobile = server
        .owner(
            "POST",
            "/lingxi/v1/access/mobile-credentials",
            Some(r#"{"displayName":"Should Not Exist"}"#),
        )
        .await;
    let failed_desktop = server
        .owner(
            "POST",
            "/lingxi/v1/access/desktop-credentials",
            Some(r#"{"displayName":"Should Not Exist"}"#),
        )
        .await;
    restore_runtime_writes(&server.runtime, prior_permissions);
    assert_eq!(failed_mobile.status, 500);
    assert_eq!(failed_desktop.status, 500);
    assert!(failed_mobile.body.contains("device_registry_failure"));
    assert!(failed_desktop.body.contains("device_registry_failure"));
    assert!(!failed_mobile.body.contains("\"secret\""));
    assert!(!failed_desktop.body.contains("\"secret\""));
    assert_eq!(server.registry_bytes(), before_issue_fault);
    record(
        &mut cases,
        "management-mobile-credential-store-failure-no-secret",
        json!({"status": failed_mobile.status, "registriesUnchanged": true, "secretAbsent": true}),
    );
    record(
        &mut cases,
        "management-desktop-credential-store-failure-no-secret",
        json!({"status": failed_desktop.status, "registriesUnchanged": true, "secretAbsent": true}),
    );

    let before = server.registry_bytes();
    let mobile = server
        .owner(
            "POST",
            "/lingxi/v1/access/mobile-credentials",
            Some(r#"{"displayName":"R02 Mobile"}"#),
        )
        .await;
    assert_eq!(mobile.status, 200, "mobile credential must issue");
    let mobile = mobile.json();
    let mobile_secret = mobile["secret"].as_str().expect("mobile one-time secret");
    let mobile_device = mobile["device"]["deviceId"]
        .as_str()
        .expect("mobile device id")
        .to_string();
    assert!(mobile_secret.len() >= 32);
    assert_eq!(mobile["accessUrl"], "https://r02.example.invalid/mobile/");
    let after = server.registry_bytes();
    assert_ne!(after, before);
    assert!(!after
        .0
        .windows(mobile_secret.len())
        .any(|window| window == mobile_secret.as_bytes()));
    assert!(!after
        .1
        .windows(mobile_secret.len())
        .any(|window| window == mobile_secret.as_bytes()));
    let registry: Value = serde_json::from_slice(&after.1).expect("credential registry JSON");
    assert_eq!(
        registry["credentials"]
            .as_array()
            .expect("credentials")
            .len(),
        1
    );
    assert_eq!(
        registry["audit"]
            .as_array()
            .expect("credential audit")
            .len(),
        1
    );
    let mobile_audit = registry["audit"]
        .as_array()
        .expect("mobile audit")
        .last()
        .expect("last mobile audit");
    assert_eq!(mobile_audit["action"], "access.mobile_credential.issue");
    assert_eq!(mobile_audit["target"], mobile_device);
    assert_eq!(
        mobile_audit["metadata"]["credentialId"],
        mobile["credential"]["credentialId"]
    );
    assert_eq!(
        mobile_audit["metadata"]["scopes"],
        mobile["credential"]["scopes"]
    );
    record(
        &mut cases,
        "management-mobile-credential-secret-address-audit",
        json!({"status": 200, "secretOneTime": true, "accessUrl": "https://r02.example.invalid/mobile/", "persistedWithoutPlainSecret": true}),
    );

    let device_me = request(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {mobile_secret}"))],
        None,
    )
    .await;
    assert_eq!(device_me.status, 200);
    assert_me_projection(
        &device_me.json(),
        "device",
        "device_credential",
        "user_local",
    );
    assert_ne!(device_me.json()["principalId"], owner_me["principalId"]);
    record(
        &mut cases,
        "management-me-device-version-identity-capabilities",
        json!({"status": 200, "kind": "device", "projectionMatchesScopes": true}),
    );

    let before = server.registry_bytes();
    let desktop = server
        .owner(
            "POST",
            "/lingxi/v1/access/desktop-credentials",
            Some(r#"{"displayName":"R02 Desktop"}"#),
        )
        .await;
    assert_eq!(desktop.status, 200, "desktop credential must issue");
    let desktop = desktop.json();
    let desktop_secret = desktop["secret"].as_str().expect("desktop one-time secret");
    let desktop_credential = desktop["credential"]["credentialId"]
        .as_str()
        .expect("desktop credential id")
        .to_string();
    assert!(desktop_secret.len() >= 32);
    assert_eq!(desktop["accessUrl"], "https://r02.example.invalid/desktop/");
    let after = server.registry_bytes();
    assert_ne!(after, before);
    assert!(!after
        .0
        .windows(desktop_secret.len())
        .any(|window| window == desktop_secret.as_bytes()));
    assert!(!after
        .1
        .windows(desktop_secret.len())
        .any(|window| window == desktop_secret.as_bytes()));
    let registry: Value = serde_json::from_slice(&after.1).expect("credential registry JSON");
    assert_eq!(
        registry["credentials"]
            .as_array()
            .expect("credentials")
            .len(),
        2
    );
    assert_eq!(
        registry["audit"]
            .as_array()
            .expect("credential audit")
            .len(),
        2
    );
    let desktop_audit = registry["audit"]
        .as_array()
        .expect("desktop audit")
        .last()
        .expect("last desktop audit");
    assert_eq!(desktop_audit["action"], "access.desktop_credential.issue");
    assert_eq!(desktop_audit["target"], desktop["device"]["deviceId"]);
    assert_eq!(
        desktop_audit["metadata"]["credentialId"],
        desktop["credential"]["credentialId"]
    );
    assert_eq!(
        desktop_audit["metadata"]["scopes"],
        desktop["credential"]["scopes"]
    );
    record(
        &mut cases,
        "management-desktop-credential-secret-address-audit",
        json!({"status": 200, "secretOneTime": true, "accessUrl": "https://r02.example.invalid/desktop/", "persistedWithoutPlainSecret": true}),
    );

    let before = server.registry_bytes();
    let invalid = server
        .owner(
            "POST",
            "/lingxi/v1/access/mobile-credentials",
            Some(r#"{"scopes":["admin"]}"#),
        )
        .await;
    assert_eq!(invalid.status, 400);
    let unauthorized_mobile = request(
        server.addr,
        "POST",
        "/lingxi/v1/access/mobile-credentials",
        &[],
        Some(r#"{}"#),
    )
    .await;
    assert_ne!(unauthorized_mobile.status, 200);
    assert_eq!(server.registry_bytes(), before);
    record(
        &mut cases,
        "management-mobile-credential-invalid-unauthorized-unchanged",
        json!({"invalid": invalid.status, "unauthorized": unauthorized_mobile.status, "registriesUnchanged": true}),
    );
    let before = server.registry_bytes();
    let invalid = server
        .owner(
            "POST",
            "/lingxi/v1/access/desktop-credentials",
            Some(r#"{"scopes":["admin"]}"#),
        )
        .await;
    assert_eq!(invalid.status, 400);
    let unauthorized_desktop = request(
        server.addr,
        "POST",
        "/lingxi/v1/access/desktop-credentials",
        &[],
        Some(r#"{}"#),
    )
    .await;
    assert_ne!(unauthorized_desktop.status, 200);
    assert_eq!(server.registry_bytes(), before);
    record(
        &mut cases,
        "management-desktop-credential-invalid-unauthorized-unchanged",
        json!({"invalid": invalid.status, "unauthorized": unauthorized_desktop.status, "registriesUnchanged": true}),
    );

    let populated_summary = server.owner("GET", "/lingxi/v1/access/summary", None).await;
    assert_eq!(populated_summary.status, 200);
    let populated = populated_summary.json();
    assert_eq!(populated["account"]["username"], "r02-owner");
    assert_eq!(populated["network"]["mode"], "lan");
    assert_eq!(
        populated["devices"]
            .as_array()
            .expect("summary devices")
            .len(),
        2
    );
    assert_eq!(
        populated["credentials"]
            .as_array()
            .expect("summary credentials")
            .len(),
        2
    );
    record(
        &mut cases,
        "management-summary-populated-matches-stores",
        json!({"status": 200, "accountNetworkDevicesCredentialsMatch": true}),
    );

    let listed = server.owner("GET", "/lingxi/v1/devices", None).await;
    assert_eq!(listed.status, 200);
    let listed_body = listed.json();
    assert_eq!(listed_body["devices"].as_array().expect("devices").len(), 2);
    assert_eq!(
        listed_body["credentials"]
            .as_array()
            .expect("credentials")
            .len(),
        2
    );
    assert_eq!(listed_body["pairingSessions"], json!([]));
    assert!(!listed.body.contains(mobile_secret) && !listed.body.contains(desktop_secret));
    record(
        &mut cases,
        "management-device-list-redacts-secrets",
        json!({"status": 200, "devices": 2, "credentials": 2, "secretsAbsent": true}),
    );
    let unauthorized = request(server.addr, "GET", "/lingxi/v1/devices", &[], None).await;
    assert_ne!(unauthorized.status, 200);
    record(
        &mut cases,
        "management-device-list-unauthorized-refused",
        json!({"status": unauthorized.status}),
    );

    let before_revoke_fault = server.registry_bytes();
    let prior_permissions = block_runtime_writes(&server.runtime);
    let failed_revoke = server
        .owner(
            "POST",
            &format!("/lingxi/v1/devices/credentials/{desktop_credential}/revoke"),
            None,
        )
        .await;
    restore_runtime_writes(&server.runtime, prior_permissions);
    assert_eq!(failed_revoke.status, 500);
    assert!(failed_revoke.body.contains("device_registry_failure"));
    assert_eq!(server.registry_bytes(), before_revoke_fault);
    record(
        &mut cases,
        "management-credential-revoke-store-failure-target-still-active",
        json!({"status": failed_revoke.status, "registriesUnchanged": true}),
    );

    let before = server.registry_bytes();
    let revoked = server
        .owner(
            "POST",
            &format!("/lingxi/v1/devices/credentials/{desktop_credential}/revoke"),
            None,
        )
        .await;
    assert_eq!(revoked.status, 200);
    assert_eq!(
        revoked.json()["credential"]["credentialId"],
        desktop_credential
    );
    assert_eq!(revoked.json()["credential"]["status"], "revoked");
    let desktop_auth = request(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {desktop_secret}"))],
        None,
    )
    .await;
    assert_ne!(desktop_auth.status, 200);
    let mobile_auth = request(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {mobile_secret}"))],
        None,
    )
    .await;
    assert_eq!(mobile_auth.status, 200);
    assert_ne!(server.registry_bytes(), before);
    let before_audit: Value = serde_json::from_slice(&before.1).expect("before revoke audit");
    let after_audit: Value =
        serde_json::from_slice(&server.registry_bytes().1).expect("after revoke audit");
    assert_eq!(
        after_audit["audit"].as_array().expect("audit").len(),
        before_audit["audit"].as_array().expect("audit").len() + 1
    );
    assert_eq!(
        after_audit["audit"]
            .as_array()
            .expect("audit")
            .last()
            .expect("last audit")["target"],
        desktop_credential
    );
    assert_eq!(
        after_audit["audit"]
            .as_array()
            .expect("audit")
            .last()
            .expect("last audit")["action"],
        "devices.credential.revoke"
    );
    assert_eq!(
        after_audit["audit"]
            .as_array()
            .expect("credential revoke audit")
            .last()
            .expect("last credential revoke audit")["metadata"]["deviceId"],
        desktop["device"]["deviceId"]
    );
    record(
        &mut cases,
        "management-credential-revoke-target-only",
        json!({"status": 200, "revokedCredentialRejected": true, "otherCredentialAccepted": true, "exactAudit": true}),
    );
    let before = server.registry_bytes();
    let missing = server
        .owner(
            "POST",
            "/lingxi/v1/devices/credentials/missing/revoke",
            None,
        )
        .await;
    assert_eq!(missing.status, 404);
    let unauthorized = request(
        server.addr,
        "POST",
        &format!("/lingxi/v1/devices/credentials/{desktop_credential}/revoke"),
        &[],
        None,
    )
    .await;
    assert_ne!(unauthorized.status, 200);
    assert_eq!(server.registry_bytes(), before);
    record(
        &mut cases,
        "management-credential-revoke-missing-no-side-effect",
        json!({"missing": 404, "unauthorized": unauthorized.status, "registriesUnchanged": true}),
    );

    let other = server
        .owner(
            "POST",
            "/lingxi/v1/access/desktop-credentials",
            Some(r#"{"displayName":"Other Desktop"}"#),
        )
        .await;
    assert_eq!(other.status, 200);
    let other = other.json();
    let other_secret = other["secret"].as_str().expect("other credential secret");
    let before_revoke_fault = server.registry_bytes();
    let prior_permissions = block_runtime_writes(&server.runtime);
    let failed_revoke = server
        .owner(
            "POST",
            &format!("/lingxi/v1/devices/{mobile_device}/revoke"),
            None,
        )
        .await;
    restore_runtime_writes(&server.runtime, prior_permissions);
    assert_eq!(failed_revoke.status, 500);
    assert!(failed_revoke.body.contains("device_registry_failure"));
    assert_eq!(server.registry_bytes(), before_revoke_fault);
    record(
        &mut cases,
        "management-device-revoke-store-failure-target-still-active",
        json!({"status": failed_revoke.status, "registriesUnchanged": true}),
    );

    let before = server.registry_bytes();
    let revoked = server
        .owner(
            "POST",
            &format!("/lingxi/v1/devices/{mobile_device}/revoke"),
            None,
        )
        .await;
    assert_eq!(revoked.status, 200);
    assert_eq!(revoked.json()["device"]["deviceId"], mobile_device);
    assert_eq!(revoked.json()["device"]["status"], "revoked");
    let mobile_auth = request(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {mobile_secret}"))],
        None,
    )
    .await;
    assert_ne!(mobile_auth.status, 200);
    let other_auth = request(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {other_secret}"))],
        None,
    )
    .await;
    assert_eq!(other_auth.status, 200);
    assert_ne!(server.registry_bytes(), before);
    let before_audit: Value =
        serde_json::from_slice(&before.1).expect("before device revoke audit");
    let after_audit: Value =
        serde_json::from_slice(&server.registry_bytes().1).expect("after device revoke audit");
    assert!(after_audit["credentials"]
        .as_array()
        .expect("device credential list")
        .iter()
        .filter(|credential| credential["deviceId"] == mobile_device)
        .all(|credential| credential["status"] == "revoked"));
    assert_eq!(
        after_audit["audit"].as_array().expect("audit").len(),
        before_audit["audit"].as_array().expect("audit").len() + 1
    );
    assert_eq!(
        after_audit["audit"]
            .as_array()
            .expect("audit")
            .last()
            .expect("last audit")["target"],
        mobile_device
    );
    assert_eq!(
        after_audit["audit"]
            .as_array()
            .expect("audit")
            .last()
            .expect("last audit")["action"],
        "devices.revoke"
    );
    record(
        &mut cases,
        "management-device-revoke-target-auth-rejected",
        json!({"status": 200, "revokedDeviceCredentialRejected": true, "otherDeviceUnaffected": true, "exactAudit": true}),
    );
    let before = server.registry_bytes();
    let missing = server
        .owner("POST", "/lingxi/v1/devices/missing/revoke", None)
        .await;
    assert_eq!(missing.status, 404);
    let unauthorized = request(
        server.addr,
        "POST",
        &format!("/lingxi/v1/devices/{mobile_device}/revoke"),
        &[],
        None,
    )
    .await;
    assert_ne!(unauthorized.status, 200);
    assert_eq!(server.registry_bytes(), before);
    record(
        &mut cases,
        "management-device-revoke-missing-no-side-effect",
        json!({"missing": 404, "unauthorized": unauthorized.status, "registriesUnchanged": true}),
    );

    let before = server.registry_bytes();
    let invalid = server
        .owner(
            "POST",
            "/lingxi/v1/devices/pairing-sessions",
            Some(r#"{"requestedDevice":{"deviceKind":"invalid-kind"}}"#),
        )
        .await;
    assert_eq!(invalid.status, 400);
    let unauthorized = request(
        server.addr,
        "POST",
        "/lingxi/v1/devices/pairing-sessions",
        &[],
        Some(r#"{"requestedDevice":{"deviceKind":"mobile"}}"#),
    )
    .await;
    assert_ne!(unauthorized.status, 200);
    assert_eq!(server.registry_bytes(), before);
    record(
        &mut cases,
        "management-pairing-create-invalid-unauthorized-unchanged",
        json!({"invalid": invalid.status, "unauthorized": unauthorized.status, "registriesUnchanged": true}),
    );

    let before_pairing_fault = server.registry_bytes();
    let prior_permissions = block_runtime_writes(&server.runtime);
    let failed_create = server
        .owner(
            "POST",
            "/lingxi/v1/devices/pairing-sessions",
            Some(r#"{"requestedDevice":{"deviceKind":"mobile"}}"#),
        )
        .await;
    restore_runtime_writes(&server.runtime, prior_permissions);
    assert_eq!(failed_create.status, 500);
    assert!(failed_create.body.contains("device_registry_failure"));
    assert!(!failed_create.body.contains("userCode"));
    assert_eq!(server.registry_bytes(), before_pairing_fault);
    record(
        &mut cases,
        "management-pairing-create-store-failure-no-code",
        json!({"status": failed_create.status, "registriesUnchanged": true, "codeAbsent": true}),
    );

    let before_pairing_registry: Value = serde_json::from_slice(&server.registry_bytes().1)
        .expect("credential registry before pairing create");
    let created = server.owner("POST", "/lingxi/v1/devices/pairing-sessions", Some(r#"{"requestedDevice":{"deviceKind":"mobile","displayName":"Paired Mobile"},"ttlMs":300000}"#)).await;
    assert_eq!(created.status, 200);
    let created = created.json();
    let pairing_id = created["pairingSessionId"]
        .as_str()
        .expect("pairing id")
        .to_string();
    let user_code = created["userCode"]
        .as_str()
        .expect("one-time pairing code")
        .to_string();
    let expires = created["expiresAtUnixMs"].as_u64().expect("pairing expiry");
    assert_eq!(expires, server.clock_epoch + 300_000);
    let registry: Value =
        serde_json::from_slice(&server.registry_bytes().1).expect("credential registry JSON");
    assert!(registry["pairingSessions"]
        .as_array()
        .expect("pairing sessions")
        .iter()
        .any(|entry| entry["pairingSessionId"] == pairing_id && entry["status"] == "pending"));
    let pairing_list = server.owner("GET", "/lingxi/v1/devices", None).await;
    assert_eq!(pairing_list.status, 200);
    assert!(pairing_list.json()["pairingSessions"]
        .as_array()
        .expect("listed pairing sessions")
        .iter()
        .any(|entry| entry["pairingSessionId"] == pairing_id && entry["status"] == "pending"));
    assert!(!pairing_list.body.contains(&user_code));
    record(
        &mut cases,
        "management-device-list-includes-pending-pairing-redacted",
        json!({"status": 200, "pendingPairingVisible": true, "userCodeAbsent": true}),
    );
    assert_eq!(
        registry["audit"].as_array().expect("pairing audit").len(),
        before_pairing_registry["audit"]
            .as_array()
            .expect("prior pairing audit")
            .len()
            + 1
    );
    let create_audit = registry["audit"]
        .as_array()
        .expect("pairing audit")
        .last()
        .expect("pairing create audit");
    assert_eq!(create_audit["action"], "devices.pairing.create");
    assert_eq!(create_audit["target"], pairing_id);
    assert_eq!(create_audit["metadata"]["deviceKind"], "mobile");
    assert!(!server
        .registry_bytes()
        .1
        .windows(user_code.len())
        .any(|window| window == user_code.as_bytes()));
    record(
        &mut cases,
        "management-pairing-create-code-expiry-audit",
        json!({"status": 200, "expiryMs": expires, "pendingPersisted": true, "oneTimeCodeNotStored": true, "exactAudit": true}),
    );

    let approve_path = format!("/lingxi/v1/devices/pairing-sessions/{pairing_id}/approve");
    let before = server.registry_bytes();
    let wrong = server
        .owner("POST", &approve_path, Some(r#"{"userCode":"wrong"}"#))
        .await;
    assert_eq!(wrong.status, 403);
    let invalid = server
        .owner(
            "POST",
            &approve_path,
            Some(&json!({"userCode": user_code, "scopes": ["admin"]}).to_string()),
        )
        .await;
    assert_eq!(invalid.status, 400);
    let unauthorized = request(
        server.addr,
        "POST",
        &approve_path,
        &[],
        Some(&json!({"userCode": user_code}).to_string()),
    )
    .await;
    assert_ne!(unauthorized.status, 200);
    assert_eq!(server.registry_bytes(), before);
    record(
        &mut cases,
        "management-pairing-approve-wrong-code-scope-auth-unchanged",
        json!({"wrongCode": wrong.status, "invalidScope": invalid.status, "unauthorized": unauthorized.status, "registriesUnchanged": true}),
    );

    let before_approve_fault = server.registry_bytes();
    let prior_permissions = block_runtime_writes(&server.runtime);
    let failed_approve = server
        .owner(
            "POST",
            &approve_path,
            Some(&json!({"userCode": user_code}).to_string()),
        )
        .await;
    restore_runtime_writes(&server.runtime, prior_permissions);
    assert_eq!(failed_approve.status, 500);
    assert!(failed_approve.body.contains("device_registry_failure"));
    assert!(!failed_approve.body.contains("\"secret\""));
    assert_eq!(server.registry_bytes(), before_approve_fault);
    record(
        &mut cases,
        "management-pairing-approve-store-failure-still-pending",
        json!({"status": failed_approve.status, "registriesUnchanged": true, "secretAbsent": true}),
    );

    let before = server.registry_bytes();
    let approved = server
        .owner(
            "POST",
            &approve_path,
            Some(&json!({"userCode": user_code, "scopes": ["chat", "resources.read"]}).to_string()),
        )
        .await;
    assert_eq!(approved.status, 200);
    let approved_body = approved.json();
    assert_eq!(approved_body["pairingSession"]["status"], "approved");
    assert_eq!(
        approved_body["credential"]["scopes"],
        json!(["chat", "resources.read"])
    );
    let pairing_secret = approved_body["secret"]
        .as_str()
        .expect("pairing one-time secret");
    let after = server.registry_bytes();
    assert_ne!(after, before);
    let before_approve: Value = serde_json::from_slice(&before.1).expect("before approve registry");
    let after_approve: Value = serde_json::from_slice(&after.1).expect("after approve registry");
    assert_eq!(
        after_approve["credentials"]
            .as_array()
            .expect("approved credentials")
            .len(),
        before_approve["credentials"]
            .as_array()
            .expect("prior credentials")
            .len()
            + 1
    );
    assert_eq!(
        after_approve["audit"]
            .as_array()
            .expect("approved audit")
            .len(),
        before_approve["audit"]
            .as_array()
            .expect("prior audit")
            .len()
            + 1
    );
    let approve_audit = after_approve["audit"]
        .as_array()
        .expect("approved audit")
        .last()
        .expect("approve audit");
    assert_eq!(approve_audit["action"], "devices.pairing.approve");
    assert_eq!(approve_audit["target"], approved_body["device"]["deviceId"]);
    assert_eq!(
        approve_audit["metadata"]["credentialId"],
        approved_body["credential"]["credentialId"]
    );
    assert_eq!(approve_audit["metadata"]["pairingSessionId"], pairing_id);
    assert_eq!(
        approve_audit["metadata"]["scopes"],
        approved_body["credential"]["scopes"]
    );
    assert!(!after
        .1
        .windows(pairing_secret.len())
        .any(|window| window == pairing_secret.as_bytes()));
    let pairing_auth = request(
        server.addr,
        "GET",
        "/lingxi/v1/me",
        &[("Authorization", &format!("Bearer {pairing_secret}"))],
        None,
    )
    .await;
    assert_eq!(pairing_auth.status, 200);
    record(
        &mut cases,
        "management-pairing-approve-once-scope-secret-audit",
        json!({"status": 200, "approvedPersisted": true, "scopeExact": true, "secretNotStored": true, "credentialAuthenticates": true, "exactAudit": true}),
    );
    let before = server.registry_bytes();
    let replay = server
        .owner(
            "POST",
            &approve_path,
            Some(&json!({"userCode": user_code}).to_string()),
        )
        .await;
    assert_ne!(replay.status, 200);
    assert_eq!(server.registry_bytes(), before);
    record(
        &mut cases,
        "management-pairing-replay-does-not-sign",
        json!({"status": replay.status, "registriesUnchanged": true}),
    );

    let concurrent = server
        .owner(
            "POST",
            "/lingxi/v1/devices/pairing-sessions",
            Some(r#"{"requestedDevice":{"deviceKind":"desktop","displayName":"Concurrent Pair"}}"#),
        )
        .await;
    assert_eq!(concurrent.status, 200);
    let concurrent = concurrent.json();
    let concurrent_path = format!(
        "/lingxi/v1/devices/pairing-sessions/{}/approve",
        concurrent["pairingSessionId"]
            .as_str()
            .expect("concurrent id")
    );
    let concurrent_body = json!({"userCode": concurrent["userCode"]}).to_string();
    let before = server.registry_bytes();
    let (first, second) = tokio::join!(
        server.owner("POST", &concurrent_path, Some(&concurrent_body)),
        server.owner("POST", &concurrent_path, Some(&concurrent_body)),
    );
    assert_eq!(
        [first.status, second.status]
            .iter()
            .filter(|status| **status == 200)
            .count(),
        1
    );
    let before_registry: Value =
        serde_json::from_slice(&before.1).expect("before credential registry");
    let after_registry: Value =
        serde_json::from_slice(&server.registry_bytes().1).expect("after credential registry");
    assert_eq!(
        after_registry["credentials"]
            .as_array()
            .expect("after credentials")
            .len(),
        before_registry["credentials"]
            .as_array()
            .expect("before credentials")
            .len()
            + 1
    );
    assert_eq!(
        after_registry["audit"]
            .as_array()
            .expect("concurrent audit")
            .len(),
        before_registry["audit"]
            .as_array()
            .expect("prior concurrent audit")
            .len()
            + 1
    );
    assert_eq!(
        after_registry["audit"]
            .as_array()
            .expect("concurrent audit")
            .last()
            .expect("concurrent approve audit")["action"],
        "devices.pairing.approve"
    );
    let concurrent_success = if first.status == 200 { &first } else { &second };
    assert_eq!(
        after_registry["audit"]
            .as_array()
            .expect("concurrent audit")
            .last()
            .expect("concurrent approve audit")["target"],
        concurrent_success.json()["device"]["deviceId"]
    );
    record(
        &mut cases,
        "management-pairing-concurrent-only-one-credential",
        json!({"statuses": [first.status, second.status], "newCredentials": 1, "newApproveAudit": 1}),
    );

    let set = server
        .owner(
            "PUT",
            "/lingxi/v1/access/account/password",
            Some(r#"{"password":"r02-test-password"}"#),
        )
        .await;
    assert_eq!(set.status, 200);
    let before_sessions = server.manager_json()["webSessions"]
        .as_array()
        .expect("web sessions")
        .len();
    let login = request(
        server.addr,
        "POST",
        "/lingxi/v1/web-auth/login",
        &[],
        Some(r#"{"username":"r02-owner","password":"r02-test-password","clientKind":"desktop"}"#),
    )
    .await;
    assert_eq!(login.status, 200);
    assert!(login.headers.contains("HttpOnly"));
    assert!(login.headers.contains("Max-Age=1209600"));
    let cookie = set_cookie(&login);
    let cookie_secret = cookie.strip_prefix("hana_session=").expect("cookie name");
    let desktop_login = login.json();
    let login_expires = desktop_login["expiresAtUnixMs"]
        .as_u64()
        .expect("web expiry");
    assert_eq!(login_expires, server.clock_epoch + 1_209_600_000);
    assert!(desktop_login["principal"]["scopes"]
        .as_array()
        .expect("desktop scopes")
        .iter()
        .any(|value| value == "settings.write"));
    assert!(!login.body.contains(cookie_secret));
    let manager = server.manager_json();
    assert_eq!(
        manager["webSessions"]
            .as_array()
            .expect("web sessions")
            .len(),
        before_sessions + 1
    );
    assert_eq!(
        manager["webSessions"]
            .as_array()
            .expect("web sessions")
            .last()
            .expect("created web session")["expiresAtUnixMs"],
        login_expires
    );
    assert!(!server
        .manager_bytes()
        .windows(cookie_secret.len())
        .any(|window| window == cookie_secret.as_bytes()));
    record(
        &mut cases,
        "management-web-login-password-cookie-14day-desktop-scope",
        json!({"status": 200, "httpOnly": true, "maxAgeSeconds": 1_209_600, "expiresAtUnixMs": login_expires, "storedWithoutPlainCookie": true, "desktopWriteScope": true}),
    );

    let before = server.manager_bytes();
    let priority = request(server.addr, "POST", "/lingxi/v1/web-auth/login", &[], Some(r#"{"credential":"invalid-test-credential","username":"r02-owner","password":"r02-test-password"}"#)).await;
    assert_ne!(priority.status, 200);
    assert_eq!(server.manager_bytes(), before);
    record(
        &mut cases,
        "management-web-login-credential-priority-denies-invalid-token",
        json!({"status": priority.status, "sessionStoreUnchanged": true}),
    );

    let before_session_reads = server.manager_bytes();
    let valid = request(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(valid.status, 200);
    assert_eq!(valid.json()["authenticated"], true);
    let valid_principal = valid.json()["principal"].clone();
    assert_eq!(
        valid_principal["userId"],
        lingxi_service::auth::LOCAL_OWNER_USER_ID
    );
    assert_eq!(valid_principal["credentialKind"], "web_session");
    assert_eq!(valid_principal["connectionKind"], "local");
    assert!(valid_principal["scopes"]
        .as_array()
        .expect("desktop web scopes")
        .iter()
        .any(|scope| scope == "settings.write"));
    assert!(!valid.body.contains(cookie_secret));
    assert_eq!(server.manager_bytes(), before_session_reads);
    record(
        &mut cases,
        "management-web-session-valid-sanitized",
        json!({"status": 200, "authenticated": true, "cookieSecretAbsent": true, "connectionKind": "local", "desktopWriteScope": true, "storeUnchanged": true}),
    );
    let invalid = request(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", "hana_session=invalid")],
        None,
    )
    .await;
    assert_eq!(invalid.status, 200);
    assert_eq!(invalid.json()["authenticated"], false);
    assert!(invalid.json()["principal"].is_null());
    assert_eq!(server.manager_bytes(), before_session_reads);
    record(
        &mut cases,
        "management-web-session-invalid-false",
        json!({"status": 200, "authenticated": false, "principal": null, "storeUnchanged": true}),
    );

    let mobile_login = request(
        server.addr,
        "POST",
        "/lingxi/v1/web-auth/login",
        &[],
        Some(r#"{"username":"r02-owner","password":"r02-test-password","clientKind":"mobile"}"#),
    )
    .await;
    assert_eq!(mobile_login.status, 200);
    let mobile_cookie = set_cookie(&mobile_login);
    assert!(!mobile_login.json()["principal"]["scopes"]
        .as_array()
        .expect("mobile scopes")
        .iter()
        .any(|value| value == "settings.write"));
    record(
        &mut cases,
        "management-web-login-mobile-scope-restricted",
        json!({"status": 200, "noSettingsWrite": true}),
    );
    let before_mobile_session = server.manager_bytes();
    let mobile_session = request(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &mobile_cookie)],
        None,
    )
    .await;
    assert_eq!(mobile_session.status, 200);
    let mobile_session_json = mobile_session.json();
    assert_eq!(mobile_session_json["authenticated"], true);
    let mobile_principal = &mobile_session_json["principal"];
    assert_eq!(mobile_principal["credentialKind"], "web_session");
    assert_eq!(mobile_principal["connectionKind"], "local");
    assert!(!mobile_principal["scopes"]
        .as_array()
        .expect("mobile web scopes")
        .iter()
        .any(|scope| scope == "settings.write"));
    let mobile_secret = mobile_cookie.strip_prefix("hana_session=").unwrap();
    assert!(!mobile_session.body.contains(mobile_secret));
    assert_eq!(server.manager_bytes(), before_mobile_session);
    record(
        &mut cases,
        "management-web-session-mobile-scope-no-write",
        json!({"status": 200, "connectionKind": "local", "credentialKind": "web_session", "settingsWrite": false, "cookieSecretAbsent": true, "storeUnchanged": true}),
    );

    let before_web_fault = server.manager_bytes();
    let held_runtime = server.runtime.with_extension("r02-held-for-web-fault");
    assert!(!held_runtime.exists());
    std::fs::rename(&server.runtime, &held_runtime)
        .expect("withhold synthetic runtime for web writes");
    let failed_login = request(
        server.addr,
        "POST",
        "/lingxi/v1/web-auth/login",
        &[],
        Some(r#"{"username":"r02-owner","password":"r02-test-password","clientKind":"desktop"}"#),
    )
    .await;
    let failed_logout = request(
        server.addr,
        "POST",
        "/lingxi/v1/web-auth/logout",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    std::fs::rename(&held_runtime, &server.runtime)
        .expect("restore synthetic runtime after web writes");
    assert_eq!(failed_login.status, 500);
    assert_eq!(failed_logout.status, 500);
    assert!(failed_login.body.contains("management_store_failure"));
    assert!(failed_logout.body.contains("management_store_failure"));
    assert!(!failed_login
        .headers
        .to_ascii_lowercase()
        .contains("set-cookie"));
    assert_eq!(server.manager_bytes(), before_web_fault);
    let after_failed_logout = request(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(after_failed_logout.json()["authenticated"], true);
    record(
        &mut cases,
        "management-web-login-store-failure-no-session",
        json!({"status": failed_login.status, "cookieIssued": false, "storeUnchanged": true}),
    );
    record(
        &mut cases,
        "management-web-logout-store-failure-session-alive",
        json!({"status": failed_logout.status, "sessionStillValid": true, "storeUnchanged": true}),
    );

    let logout = request(
        server.addr,
        "POST",
        "/lingxi/v1/web-auth/logout",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(logout.status, 200);
    assert!(logout.headers.contains("Max-Age=0"));
    let after = request(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(after.json()["authenticated"], false);
    let other = request(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &mobile_cookie)],
        None,
    )
    .await;
    assert_eq!(other.json()["authenticated"], true);
    record(
        &mut cases,
        "management-web-logout-only-current-cookie",
        json!({"status": 200, "clearedCookie": true, "currentRevoked": true, "otherSessionAlive": true}),
    );

    let before_invalid_logout = server.manager_bytes();
    let invalid_logout = request(
        server.addr,
        "POST",
        "/lingxi/v1/web-auth/logout",
        &[("Cookie", "hana_session=invalid")],
        None,
    )
    .await;
    assert_eq!(invalid_logout.status, 200);
    assert!(invalid_logout.headers.contains("Max-Age=0"));
    assert_eq!(server.manager_bytes(), before_invalid_logout);
    let still_valid = request(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &mobile_cookie)],
        None,
    )
    .await;
    assert_eq!(still_valid.json()["authenticated"], true);
    record(
        &mut cases,
        "management-web-logout-invalid-cookie-no-other-revoke",
        json!({"status": 200, "otherSessionAlive": true, "storeUnchanged": true}),
    );

    server.clock.advance(1_209_600_001);
    let expired = request(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &mobile_cookie)],
        None,
    )
    .await;
    assert_eq!(expired.status, 200);
    assert_eq!(expired.json()["authenticated"], false);
    assert!(expired.json()["principal"].is_null());
    let after_expired_session = server.manager_bytes();
    let expired_again = request(
        server.addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &mobile_cookie)],
        None,
    )
    .await;
    assert_eq!(expired_again.json()["authenticated"], false);
    assert_eq!(server.manager_bytes(), after_expired_session);
    record(
        &mut cases,
        "management-web-session-expired-false",
        json!({"status": 200, "authenticated": false, "principal": null, "advancedClockMs": 1_209_600_001_u64, "storeUnchangedAfterRead": true}),
    );

    let short_pairing = server
        .owner(
            "POST",
            "/lingxi/v1/devices/pairing-sessions",
            Some(r#"{"requestedDevice":{"deviceKind":"mobile"},"ttlMs":1}"#),
        )
        .await;
    assert_eq!(short_pairing.status, 200);
    let short_pairing = short_pairing.json();
    let short_id = short_pairing["pairingSessionId"]
        .as_str()
        .expect("short pairing id");
    let short_code = short_pairing["userCode"]
        .as_str()
        .expect("short pairing code");
    let before_expired: Value =
        serde_json::from_slice(&server.registry_bytes().1).expect("before expiry registry");
    server.clock.advance(2);
    let expired_approve = server
        .owner(
            "POST",
            &format!("/lingxi/v1/devices/pairing-sessions/{short_id}/approve"),
            Some(&json!({"userCode": short_code}).to_string()),
        )
        .await;
    assert_eq!(expired_approve.status, 400);
    assert!(expired_approve.body.contains("pairing expired"));
    let after_expired: Value =
        serde_json::from_slice(&server.registry_bytes().1).expect("after expiry registry");
    assert_eq!(after_expired["credentials"], before_expired["credentials"]);
    assert!(after_expired["pairingSessions"]
        .as_array()
        .expect("pairings after expiry")
        .iter()
        .any(|pairing| pairing["pairingSessionId"] == short_id && pairing["status"] == "expired"));
    record(
        &mut cases,
        "management-pairing-expired-code-does-not-sign",
        json!({"status": 400, "credentialStoreUnchanged": true, "pairingMarkedExpired": true}),
    );

    // 标准审计日志路径失效时，管理和设备写入都不得先提交再报错。
    let audit_path = server.home.join("logs/security-audit.jsonl");
    let held_audit_path = server.home.join("logs/security-audit-held.jsonl");
    let audit_before_fault = std::fs::read(&audit_path).expect("audit before path fault");
    let management_before_fault = server.manager_bytes();
    let registry_before_fault = server.registry_bytes();
    std::fs::rename(&audit_path, &held_audit_path).expect("hold audit log");
    std::fs::create_dir(&audit_path).expect("block audit log path with directory");
    let profile_audit_failure = server
        .owner(
            "PUT",
            "/lingxi/v1/access/account/profile",
            Some(r#"{"displayName":"must-not-commit-without-audit"}"#),
        )
        .await;
    let device_audit_failure = server
        .owner(
            "POST",
            "/lingxi/v1/devices/pairing-sessions",
            Some(r#"{"requestedDevice":{"deviceKind":"mobile"},"ttlMs":60000}"#),
        )
        .await;
    assert_eq!(profile_audit_failure.status, 500);
    assert_eq!(device_audit_failure.status, 500);
    assert!(profile_audit_failure
        .body
        .contains("management_store_failure"));
    assert!(device_audit_failure
        .body
        .contains("device_registry_failure"));
    assert!(!profile_audit_failure
        .body
        .contains("must-not-commit-without-audit"));
    assert!(!device_audit_failure.body.contains("userCode"));
    assert_eq!(server.manager_bytes(), management_before_fault);
    assert_eq!(server.registry_bytes(), registry_before_fault);
    std::fs::remove_dir(&audit_path).expect("remove blocked audit path");
    std::fs::rename(&held_audit_path, &audit_path).expect("restore audit log");
    assert_eq!(std::fs::read(&audit_path).unwrap(), audit_before_fault);
    let recovered_summary = server.owner("GET", "/lingxi/v1/access/summary", None).await;
    assert_eq!(recovered_summary.status, 200);
    assert_eq!(std::fs::read(&audit_path).unwrap(), audit_before_fault);
    record(
        &mut cases,
        "management-standard-audit-path-failure-no-write-recovery",
        json!({"managementStatus": 500, "deviceStatus": 500, "managementUnchanged": true,
            "registryUnchanged": true, "auditUnchanged": true, "recoveredStatus": 200,
            "noSecretOnFailure": true}),
    );

    let audit = assert_standard_audit_matches_sources(
        &server,
        &[
            mobile_secret,
            desktop_secret,
            other_secret,
            "r02-test-password",
        ],
    );
    assert!(
        audit.len() > 100,
        "standard audit must hold real committed events"
    );
    record(
        &mut cases,
        "management-standard-audit-all-actions-identities-no-secrets",
        json!({"sourceIntentCount": server.manager_json()["audit"].as_array().unwrap().len()
            + serde_json::from_slice::<Value>(&server.registry_bytes().1).unwrap()["audit"].as_array().unwrap().len(),
            "projectionExact": true, "eventIdsUnique": true, "secretsAbsent": true}),
    );

    // 保存新的本机监听地址，再真正停服、重新读取配置并绑定该端口。
    let temporary_listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("choose free synthetic restart port");
    let saved_port = temporary_listener.local_addr().expect("port").port();
    drop(temporary_listener);
    let saved_network = server
        .owner(
            "PUT",
            "/lingxi/v1/access/network",
            Some(
                &json!({"mode":"loopback","listenPort":saved_port,"publicBaseUrl":null})
                    .to_string(),
            ),
        )
        .await;
    assert_eq!(saved_network.status, 200);
    let audit_after_save = assert_standard_audit_matches_sources(
        &server,
        &[
            mobile_secret,
            desktop_secret,
            other_secret,
            "r02-test-password",
        ],
    );
    write_standard_audit_evidence(
        &server,
        &audit_after_save,
        &[
            mobile_secret,
            desktop_secret,
            other_secret,
            "r02-test-password",
        ],
    );
    let home = server.home.clone();
    let runtime = server.runtime.clone();
    let clock = server.clock.clone();
    let clock_epoch = server.clock_epoch;
    assert_eq!(server.stop().await, home);
    let stored = ServiceConfig::from_sources(
        &CliOptions {
            home: Some(home.clone()),
            ..CliOptions::default()
        },
        None,
        &std::env::temp_dir(),
    )
    .expect("read persisted network on restart");
    assert_eq!(stored.network_mode, NetworkMode::Loopback);
    assert_eq!(stored.bind_addr.port(), saved_port);
    assert!(stored.bind_addr.ip().is_loopback());
    let restarted = start_server_with_config(stored, clock.clone()).await;
    assert_eq!(restarted.addr.port(), saved_port);
    let restarted_summary = restarted
        .owner("GET", "/lingxi/v1/access/summary", None)
        .await;
    assert_eq!(restarted_summary.status, 200);
    assert_eq!(
        restarted_summary.json()["network"]["configuredPort"],
        saved_port
    );
    assert_eq!(
        std::fs::read(home.join("logs/security-audit.jsonl")).unwrap(),
        audit_after_save,
        "restart must not duplicate standard audit events"
    );
    record(
        &mut cases,
        "management-network-saved-restart-real-bind-no-audit-duplicate",
        json!({"savedPort": saved_port, "actualPort": restarted.addr.port(), "summaryPort": saved_port, "auditUnchanged": true}),
    );
    assert_eq!(restarted.stop().await, home);

    // 预占原端口，确保显式 CLI bind 真正覆盖旧设置，而不是偶然复用同一地址。
    let occupied_saved_port = std::net::TcpListener::bind(("127.0.0.1", saved_port))
        .expect("occupy saved port in synthetic test");
    let override_config = ServiceConfig::from_sources(
        &CliOptions {
            home: Some(home.clone()),
            bind: Some("127.0.0.1:0".into()),
            network_mode: Some("loopback".into()),
            ..CliOptions::default()
        },
        None,
        &std::env::temp_dir(),
    )
    .expect("explicit CLI bind overrides saved port");
    assert_eq!(override_config.bind_addr.port(), 0);
    let overridden = start_server_with_config(override_config, clock).await;
    assert_ne!(overridden.addr.port(), saved_port);
    assert_eq!(
        overridden
            .owner("GET", "/lingxi/v1/access/summary", None)
            .await
            .status,
        200
    );
    record(
        &mut cases,
        "management-network-cli-override-real-bind",
        json!({"savedPortOccupied": true, "actualPort": overridden.addr.port(), "savedPort": saved_port}),
    );
    assert_eq!(overridden.stop().await, home);
    drop(occupied_saved_port);

    // 损坏的旧设置必须拒绝启动，不能回退到默认目录或默认监听。
    let management_path = runtime.join("management.json");
    let valid_management = std::fs::read(&management_path).expect("saved management file");
    std::fs::write(&management_path, b"{").expect("inject corrupt synthetic settings");
    let corrupt_error = ServiceConfig::from_sources(
        &CliOptions {
            home: Some(home.clone()),
            ..CliOptions::default()
        },
        None,
        &std::env::temp_dir(),
    )
    .expect_err("corrupt saved settings must not silently reset");
    assert!(corrupt_error
        .to_string()
        .contains("saved network configuration invalid"));
    assert_eq!(std::fs::read(&management_path).unwrap(), b"{");
    std::fs::write(&management_path, valid_management).expect("restore synthetic settings");
    record(
        &mut cases,
        "management-network-corrupt-saved-settings-refused",
        json!({"explicitError": corrupt_error.to_string(), "storedBytesUnchanged": true}),
    );

    // 用真实网卡地址接入，而不是把 loopback 请求误当成手机浏览器验收。
    let lan_ip = if_addrs::get_if_addrs()
        .expect("enumerate real LAN interfaces")
        .into_iter()
        .filter(|interface| {
            interface.is_oper_up() && !interface.is_link_local() && !interface.is_loopback()
        })
        .map(|interface| interface.ip())
        .find(|ip| matches!(ip, std::net::IpAddr::V4(_)))
        .expect("an active non-loopback IPv4 interface is required for LAN browser acceptance");
    let lan = start_server_with_config(
        ServiceConfig {
            bind_addr: "0.0.0.0:0".parse().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Lan,
            shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
        },
        std::sync::Arc::new(ManualClock::new(clock_epoch)),
    )
    .await;
    let local_addr = SocketAddr::new("127.0.0.1".parse().unwrap(), lan.addr.port());
    let lan_addr = SocketAddr::new(lan_ip, lan.addr.port());
    let origin = format!("http://{lan_addr}");
    let bearer = lan.bearer();
    let issued = request(
        local_addr,
        "POST",
        "/lingxi/v1/access/mobile-credentials",
        &[("Authorization", &bearer)],
        Some(r#"{"displayName":"LAN browser fixture"}"#),
    )
    .await;
    assert_eq!(issued.status, 200);
    let credential = issued.json()["secret"].as_str().unwrap().to_string();
    let login_body = json!({"credential": credential, "clientKind": "mobile"}).to_string();
    let before_foreign = lan.manager_bytes();
    let foreign = request(
        lan_addr,
        "POST",
        "/lingxi/v1/web-auth/login",
        &[("Origin", "http://evil.example")],
        Some(&login_body),
    )
    .await;
    assert_eq!(foreign.status, 403);
    assert!(!foreign
        .headers
        .to_ascii_lowercase()
        .contains("access-control-allow-origin"));
    assert_eq!(lan.manager_bytes(), before_foreign);
    let wrong_scheme = request(
        lan_addr,
        "POST",
        "/lingxi/v1/web-auth/login",
        &[("Origin", &format!("https://{lan_addr}"))],
        Some(&login_body),
    )
    .await;
    assert_eq!(wrong_scheme.status, 403);
    let preflight = request(
        lan_addr,
        "OPTIONS",
        "/lingxi/v1/web-auth/login",
        &[
            ("Origin", &origin),
            ("Access-Control-Request-Method", "POST"),
        ],
        None,
    )
    .await;
    assert_eq!(preflight.status, 204);
    assert!(preflight
        .headers
        .to_ascii_lowercase()
        .contains(&format!("access-control-allow-origin: {origin}")));
    let lan_login = request(
        lan_addr,
        "POST",
        "/lingxi/v1/web-auth/login",
        &[("Origin", &origin)],
        Some(&login_body),
    )
    .await;
    assert_eq!(lan_login.status, 200);
    assert!(lan_login
        .headers
        .to_ascii_lowercase()
        .contains(&format!("access-control-allow-origin: {origin}")));
    let lan_cookie = set_cookie(&lan_login);
    assert!(lan_login.headers.contains("HttpOnly"));
    assert!(lan_login.headers.contains("Max-Age=1209600"));
    assert_eq!(
        lan_login.json()["expiresAtUnixMs"].as_u64(),
        Some(clock_epoch + 1_209_600_000)
    );
    assert!(!lan_login.json()["principal"]["scopes"]
        .as_array()
        .expect("token login scopes")
        .iter()
        .any(|scope| scope == "settings.write"));
    record(
        &mut cases,
        "management-web-login-token-positive-14day-mobile-scope",
        json!({"status": 200, "httpOnly": true, "maxAgeSeconds": 1_209_600,
            "expiresAtUnixMs": clock_epoch + 1_209_600_000,
            "mobileSettingsWrite": false, "credentialTransport": "token"}),
    );
    let before_lan_session = lan.manager_bytes();
    let lan_session = request(
        lan_addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Origin", &origin), ("Cookie", &lan_cookie)],
        None,
    )
    .await;
    assert_eq!(lan_session.status, 200);
    assert_eq!(lan_session.json()["authenticated"], true);
    let lan_session_json = lan_session.json();
    let lan_principal = &lan_session_json["principal"];
    assert_eq!(lan_principal["connectionKind"], "lan");
    assert_eq!(lan_principal["credentialKind"], "web_session");
    assert!(!lan_principal["scopes"]
        .as_array()
        .expect("LAN mobile scopes")
        .iter()
        .any(|scope| scope == "settings.write"));
    assert!(!lan_session
        .body
        .contains(lan_cookie.strip_prefix("hana_session=").unwrap()));
    assert_eq!(lan.manager_bytes(), before_lan_session);
    let before_mismatched_connection = lan.manager_bytes();
    let wrong_connection = request(
        local_addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Cookie", &lan_cookie)],
        None,
    )
    .await;
    assert_eq!(wrong_connection.status, 200);
    assert_eq!(wrong_connection.json()["authenticated"], false);
    assert!(wrong_connection.json()["principal"].is_null());
    assert_eq!(lan.manager_bytes(), before_mismatched_connection);
    record(
        &mut cases,
        "management-web-session-connection-mismatch-false",
        json!({"lanConnectionKind": "lan", "loopbackAuthenticated": false, "principal": null, "mobileSettingsWrite": false, "cookieSecretAbsent": true, "storeUnchanged": true}),
    );
    let lan_logout = request(
        lan_addr,
        "POST",
        "/lingxi/v1/web-auth/logout",
        &[("Origin", &origin), ("Cookie", &lan_cookie)],
        None,
    )
    .await;
    assert_eq!(lan_logout.status, 200);
    let revoked_lan_session = request(
        lan_addr,
        "GET",
        "/lingxi/v1/web-auth/session",
        &[("Origin", &origin), ("Cookie", &lan_cookie)],
        None,
    )
    .await;
    assert_eq!(revoked_lan_session.json()["authenticated"], false);
    record(
        &mut cases,
        "management-lan-browser-origin-login-session-logout",
        json!({"origin": origin, "preflightStatus": 204, "loginStatus": 200,
            "sessionAuthenticated": true, "logoutStatus": 200,
            "revokedSessionAuthenticated": false, "foreignStatus": 403,
            "wrongSchemeStatus": 403, "foreignStateUnchanged": true}),
    );
    assert_eq!(lan.stop().await, home);
    write_cases(&cases);
    std::fs::remove_dir_all(home).expect("remove only this synthetic management home");
}
