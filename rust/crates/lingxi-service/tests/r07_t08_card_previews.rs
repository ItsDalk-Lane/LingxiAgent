//! 角色卡别名与预览素材的真实 HTTP 回归；不替换服务、文件或权限判断。
//! 原 TS 计划没有时间 TTL：失效指计划文件已不存在，不伪造“等时过期”。

use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use std::{io::Read as _, path::PathBuf, time::Duration};

struct Harness {
    home: PathBuf,
    state: ServiceState,
    base: String,
    avatar: Vec<u8>,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn boot() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-card-previews-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        std::fs::create_dir_all(home.join("agents/a/avatars")).unwrap();
        std::fs::create_dir_all(home.join("workspace")).unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            "{\"current_agent_id\":\"a\"}",
        )
        .unwrap();
        std::fs::write(
            home.join("agents/a/config.yaml"),
            "agent:\n  name: 素材助手\n  yuan: lingxi\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        std::fs::write(home.join("agents/a/identity.md"), "角色卡真实身份").unwrap();
        std::fs::write(home.join("agents/a/AGENTS.md"), "角色卡真实行为").unwrap();
        let avatar = std::fs::read(repo.join("desktop/src/assets/Lingxi.png")).unwrap();
        std::fs::write(home.join("agents/a/avatars/agent.png"), &avatar).unwrap();
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 3000,
            },
            &layout,
            ServiceDeps {
                workspace_root: Some(home.join("workspace")),
                product_material_dir: Some(repo.join("lib")),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let (ready, address) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let server = tokio::spawn(async move {
            lingxi_service::run(
                serving,
                async {
                    let _ = stopped.await;
                },
                |addr| {
                    ready.send(addr).unwrap();
                },
                None,
            )
            .await
            .unwrap();
        });
        let base = format!("http://{}", address.await.unwrap());
        eprintln!("T08角色卡预览现场：{}", home.display());
        Self {
            home,
            state,
            base,
            avatar,
            stop,
            server,
        }
    }

    async fn raw_as(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        token: Option<&str>,
    ) -> reqwest::Response {
        let mut request = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .request(method, format!("{}{path}", self.base));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        request.send().await.unwrap()
    }

    async fn raw(&self, method: Method, path: &str, body: Option<Value>) -> reqwest::Response {
        self.raw_as(method, path, body, Some(&self.state.auth().local_token()))
            .await
    }

    async fn json(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let response = self.raw(method, path, body).await;
        let status = response.status();
        let bytes = response.bytes().await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn export(&self) -> PathBuf {
        let (status, value) = self
            .json(
                Method::POST,
                "/lingxi/v1/character-cards/export",
                Some(json!({"agentId":"a","exportMemory":false})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let file = PathBuf::from(value["filePath"].as_str().unwrap());
        assert!(file.starts_with(self.home.join("workspace")));
        file
    }

    async fn close(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), self.server)
            .await
            .unwrap()
            .unwrap();
        self.state.storage().close().await.unwrap();
        std::fs::remove_dir_all(self.home).unwrap();
    }
}

#[tokio::test]
async fn export_plan_alias_matches_preview_without_package_write_then_real_export_and_asset() {
    let h = Harness::boot().await;
    let original = std::fs::read(h.home.join("agents/a/config.yaml")).unwrap();
    let workspace_entries = || {
        std::fs::read_dir(h.home.join("workspace"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let workspace_before = workspace_entries();
    let (status, preview) = h
        .json(
            Method::POST,
            "/lingxi/v1/character-cards/export/preview",
            Some(json!({"agentId":"a"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let (status, plan) = h
        .json(
            Method::POST,
            "/lingxi/v1/character-cards/export/plan",
            Some(json!({"agentId":"a"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "原别名必须实际可用：{plan}");
    assert_eq!(plan, preview);
    assert_eq!(plan["plan"]["assets"]["avatar"], true);
    assert_eq!(
        workspace_entries(),
        workspace_before,
        "两个预览都不得写导出包"
    );
    assert_eq!(
        std::fs::read(h.home.join("agents/a/config.yaml")).unwrap(),
        original
    );
    let (status, invalid) = h
        .json(
            Method::POST,
            "/lingxi/v1/character-cards/export/plan",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(invalid["error"].as_str().unwrap().contains("agentId"));
    assert_eq!(
        h.raw_as(
            Method::POST,
            "/lingxi/v1/character-cards/export/plan",
            Some(json!({"agentId":"a"})),
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let device = h
        .state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["settings.read", "settings.write"],
            None,
        )
        .unwrap();
    assert_eq!(
        h.raw_as(
            Method::POST,
            "/lingxi/v1/character-cards/export/plan",
            Some(json!({"agentId":"a"})),
            Some(&device.secret)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    let response = h
        .raw(
            Method::GET,
            "/lingxi/v1/character-cards/export/a/assets/avatar",
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.bytes().await.unwrap().as_ref(),
        h.avatar.as_slice()
    );
    let response = h
        .raw(
            Method::GET,
            "/lingxi/v1/character-cards/export/a/assets/unknown",
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let file = h.export().await;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file).unwrap()).unwrap();
    let mut descriptor = String::new();
    zip.by_name("card.json")
        .unwrap()
        .read_to_string(&mut descriptor)
        .unwrap();
    let descriptor: Value = serde_json::from_str(&descriptor).unwrap();
    assert_eq!(descriptor["prompts"]["identity"], "角色卡真实身份");
    assert!(descriptor.get("memory").is_none());
    let mut bytes = Vec::new();
    zip.by_name(descriptor["assets"]["avatar"].as_str().unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, h.avatar);
    drop(zip);
    h.close().await;
}

#[tokio::test]
async fn plan_assets_read_real_bytes_and_refuse_missing_invalid_and_escape_without_mutation() {
    let h = Harness::boot().await;
    let file = h.export().await;
    let (status, planned) = h
        .json(
            Method::POST,
            "/lingxi/v1/character-cards/plan",
            Some(json!({"path":file})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{planned}");
    let token = planned["plan"]["token"].as_str().unwrap();
    let path = format!("/lingxi/v1/character-cards/plans/{token}/assets/avatar");
    let response = h.raw(Method::GET, &path, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["cache-control"], "private, max-age=3600");
    assert_eq!(
        response.bytes().await.unwrap().as_ref(),
        h.avatar.as_slice()
    );
    assert_eq!(
        h.raw_as(Method::GET, &path, None, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let device = h
        .state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["settings.read"],
            None,
        )
        .unwrap();
    assert_eq!(
        h.raw_as(Method::GET, &path, None, Some(&device.secret))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.raw(
            Method::GET,
            &format!("/lingxi/v1/character-cards/plans/{token}/assets/unknown"),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    for other in ["invalid", "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"] {
        let (status, error) = h
            .json(
                Method::GET,
                &format!("/lingxi/v1/character-cards/plans/{other}/assets/avatar"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(error["error"].is_string());
    }
    let stage = h.home.join(".ephemeral/character-card-imports").join(token);
    let plan_file = stage.join("plan.json");
    let original = std::fs::read(&plan_file).unwrap();
    let mut plan: Value = serde_json::from_slice(&original).unwrap();
    let outside = stage.join("outside.png");
    let mut outside_bytes = h.avatar.clone();
    outside_bytes.extend_from_slice(b"owned-outside-marker");
    std::fs::write(&outside, &outside_bytes).unwrap();
    for escaped in [
        "../outside.png".to_owned(),
        outside.to_string_lossy().into_owned(),
    ] {
        plan["assets"]["avatar"] = json!(escaped);
        std::fs::write(&plan_file, serde_json::to_vec(&plan).unwrap()).unwrap();
        let (status, error) = h.json(Method::GET, &path, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(error["error"].is_string());
        assert!(!error.to_string().contains("owned-outside-marker"));
        assert_eq!(std::fs::read(&outside).unwrap(), outside_bytes);
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, stage.join("package/linked.png")).unwrap();
        plan["assets"]["avatar"] = json!("linked.png");
        std::fs::write(&plan_file, serde_json::to_vec(&plan).unwrap()).unwrap();
        assert_eq!(
            h.raw(Method::GET, &path, None).await.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(std::fs::read(&outside).unwrap(), outside_bytes);
    }
    std::fs::write(&plan_file, &original).unwrap();
    assert_eq!(
        h.raw(Method::GET, &path, None).await.status(),
        StatusCode::OK
    );
    // 原TS没有时间TTL。这里真实移除自己的计划文件，验证失效计划不再能读素材或创建助手。
    std::fs::remove_file(&plan_file).unwrap();
    let (status, error) = h.json(Method::GET, &path, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].is_string());
    let (status, error) = h
        .json(
            Method::POST,
            "/lingxi/v1/character-cards/import",
            Some(json!({"token":token})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].is_string());
    assert_eq!(std::fs::read_dir(h.home.join("agents")).unwrap().count(), 1);
    assert_eq!(std::fs::read(&outside).unwrap(), outside_bytes);
    h.close().await;
}

#[tokio::test]
async fn oversized_theme_variables_clear_by_utf8_bytes_while_valid_card_still_registers() {
    let h = Harness::boot().await;
    let path = "/lingxi/v1/cards/vars_size";
    let prefix = "--at-limit:";
    let limit_css = format!("{prefix}{};", "x".repeat(16 * 1024 - prefix.len() - 1));
    assert_eq!(limit_css.len(), 16 * 1024);
    let (status, registered) = h
        .json(
            Method::PUT,
            path,
            Some(json!({"code":"<main>恰好上限正文</main>","varsCss":limit_css})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{registered}");
    let html = h.raw(Method::GET, path, None).await.text().await.unwrap();
    assert!(html.contains(&limit_css));
    // 字符数没超，但UTF-8字节已超：必须沿原注册接口清空变量，不能拒绝合法正文。
    let over = format!("--over-marker:{};", "界".repeat(16 * 1024 / 3 + 1));
    assert!(over.chars().count() < 16 * 1024 && over.len() > 16 * 1024);
    let code = "<main>超额变量不阻止新正文</main>";
    let (status, registered) = h
        .json(Method::PUT, path, Some(json!({"code":code,"varsCss":over})))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "超额变量应清空后成功登记：{registered}"
    );
    assert_eq!(registered["ok"], true);
    let html = h.raw(Method::GET, path, None).await.text().await.unwrap();
    assert!(html.contains(code));
    assert!(!html.contains("--over-marker") && !html.contains(prefix));
    let frame = registered["frameUrl"].as_str().unwrap();
    let html = h
        .raw_as(Method::GET, frame, None, None)
        .await
        .text()
        .await
        .unwrap();
    assert!(html.contains(code) && !html.contains("--over-marker"));
    let (status, _) = h
        .json(
            Method::PUT,
            path,
            Some(json!({"code":"x".repeat(512 * 1024 + 1),"varsCss":over})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "正文独立512KiB上限保持不变"
    );
    assert!(h
        .raw(Method::GET, path, None)
        .await
        .text()
        .await
        .unwrap()
        .contains(code));
    h.close().await;
}
