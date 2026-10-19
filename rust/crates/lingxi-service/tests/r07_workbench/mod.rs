//! R07-T07 β：工作台 / Studio 挂载面集成测试。
//!
//! 覆盖：workspaces 列举/创建/停用（含注册表落盘平价与审计事件）、
//! workbench files/search/content（含 ETag/304/Range/HEAD/内容处置头）、
//! actions 全动作（mkdir/writeText/版本冲突/rename/move/movePaths/
//! safeDelete + 改前存档）、upload（逐项结果 + 25MB 上限）、scope 门禁
//! 与非属主写租约台账。
//!
//! 夹具：真 TCP loopback + 真实 ServiceState（workspace_root 注入）；
//! 外部服务一律无（本面无外呼）。

use base64::Engine as _;
use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

mod coverage;

struct Harness {
    home: PathBuf,
    workspace: PathBuf,
    state: ServiceState,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn boot(tag: &str) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-workbench-{tag}-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(
            home.join("agents/a/config.yaml"),
            "agent:\n  name: a\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        std::fs::write(
            home.join("user/preferences.json"),
            serde_json::to_string(&json!({"current_agent_id":"a"})).unwrap(),
        )
        .unwrap();
        let workspace = home.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let home = home.canonicalize().unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
            ServiceDeps {
                workspace_root: Some(workspace.clone()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel::<()>();
        let (ready, address) = tokio::sync::oneshot::channel();
        let serving = state.clone();
        let server = tokio::spawn(async move {
            lingxi_service::run(
                serving,
                async {
                    let _ = rx.await;
                },
                |addr| {
                    let _ = ready.send(addr);
                },
                None,
            )
            .await
            .unwrap();
        });
        let base = format!("http://{}", address.await.unwrap());
        Self {
            home,
            workspace,
            state,
            base,
            stop,
            server,
        }
    }

    async fn http(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.state.auth().local_token());
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        (status, value)
    }

    async fn http_as(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        (status, value)
    }

    /// 带自定义头的请求（内容面 ETag/Range 验证用），返回状态 + 头 + 字节。
    async fn http_raw(
        &self,
        method: reqwest::Method,
        path: &str,
        headers: &[(&str, &str)],
    ) -> (u16, reqwest::header::HeaderMap, Vec<u8>) {
        let mut request = reqwest::Client::new()
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(self.state.auth().local_token());
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let heads = response.headers().clone();
        let bytes = response.bytes().await.unwrap().to_vec();
        (status, heads, bytes)
    }

    async fn shutdown(self) {
        let _ = self.stop.send(());
        let _ = self.server.await;
        // 与 r07_media_admin 同口径：先关存储工作者再清 home，避免知识库
        // worker 在 runtime 回收时被取消而产生 teardown panic 噪声。
        // workspace 在 home 之下，随 home 一并清理。
        self.state.storage().close().await.unwrap();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn b64(content: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(content)
}

fn audit_actions(home: &Path) -> Vec<String> {
    let path = home.join("logs/security-audit.jsonl");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|event| event["action"].as_str().map(str::to_string))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workspaces_list_create_disable_cycle() {
    let h = Harness::boot("ws").await;
    let studio = h.http(reqwest::Method::GET, "/lingxi/v1/me", None).await.1["studioId"]
        .as_str()
        .expect("studioId")
        .to_string();

    // ── 默认工作区列举（listStudioWorkspaces :108-126）──
    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/studio/workspaces", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["studioId"], json!(studio));
    let workspaces = body["workspaces"].as_array().unwrap();
    assert_eq!(workspaces.len(), 1, "{body}");
    let default_ws = &workspaces[0];
    assert_eq!(default_ws["isDefault"], json!(true));
    assert_eq!(default_ws["mountId"], json!("default"));
    assert_eq!(default_ws["workspaceId"], json!("default"));
    assert_eq!(default_ws["label"], json!("Default"));
    assert_eq!(default_ws["sourceKind"], json!("storage"));
    assert_eq!(default_ws["provider"], json!("local_fs"));
    assert_eq!(default_ws["presentation"], json!("folder"));
    assert_eq!(default_ws["capabilities"], json!(["list", "read", "write"]));
    // local owner → nativeRootPath 披露（现役 :239-242 + publicRoot）。
    assert_eq!(
        default_ws["nativeRootPath"].as_str().unwrap(),
        h.workspace.to_string_lossy()
    );

    // ── 创建校验（createLocalPathWorkspace :128-140）──
    for (path, message) in [
        ("", "path must be a non-empty string"),
        ("relative/dir", "path must be absolute on the server"),
    ] {
        let (status, body) = h
            .http(
                reqwest::Method::POST,
                "/lingxi/v1/studio/workspaces",
                Some(json!({ "path": path })),
            )
            .await;
        assert_eq!(status, 400, "{path}: {body}");
        assert_eq!(body["error"], json!("invalid_path"), "{path}: {body}");
        assert_eq!(body["detail"], json!(message), "{path}: {body}");
    }
    let missing = h.home.join("no-such-dir");
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/studio/workspaces",
            Some(json!({ "path": missing.to_string_lossy() })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], json!("invalid_path"));
    assert_eq!(body["detail"], json!("path must be an existing directory"));

    // ── 真实创建 + 注册表落盘平价 ──
    let extra = h.home.join("extra-root");
    std::fs::create_dir_all(&extra).unwrap();
    std::fs::write(extra.join("seed.txt"), "seed").unwrap();
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/studio/workspaces",
            Some(json!({ "path": extra.to_string_lossy(), "label": "Extra" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    let workspace = &body["workspace"];
    let mount_id = workspace["mountId"].as_str().unwrap().to_string();
    assert!(mount_id.starts_with("local_fs_"), "{workspace}");
    assert_eq!(workspace["workspaceId"], json!(mount_id));
    assert_eq!(workspace["label"], json!("Extra"));
    assert_eq!(workspace["isDefault"], json!(false));
    assert_eq!(
        workspace["capabilities"],
        json!(["list", "read", "write", "watch"])
    );
    assert_eq!(
        workspace["nativeRootPath"].as_str().unwrap(),
        extra.to_string_lossy()
    );

    let registry: Value =
        serde_json::from_str(&std::fs::read_to_string(h.home.join("studio-mounts.json")).unwrap())
            .unwrap();
    assert_eq!(registry["schemaVersion"], json!(1));
    let mounts = registry["mounts"].as_array().unwrap();
    assert_eq!(mounts.len(), 1, "{registry}");
    let entry = &mounts[0];
    assert_eq!(entry["mountId"], json!(mount_id));
    assert_eq!(entry["hostStudioId"], json!(studio));
    assert_eq!(entry["sourceKind"], json!("storage"));
    assert_eq!(entry["provider"], json!("local_fs"));
    assert_eq!(
        entry["rootLocator"]["path"].as_str().unwrap(),
        extra.to_string_lossy()
    );
    assert_eq!(entry["status"], json!("active"));
    assert_eq!(entry["grantId"], Value::Null);
    assert!(entry["createdAt"].is_string() && entry["updatedAt"].is_string());
    // 原子写带尾换行（writeJsonAtomic :136）。
    let raw = std::fs::read_to_string(h.home.join("studio-mounts.json")).unwrap();
    assert!(raw.ends_with("}\n"), "注册表 JSON 两空格缩进 + 尾换行");
    assert!(raw.contains("\n  \"mounts\""), "两空格缩进: {raw}");

    // ── 同路径 upsert：createdAt 沿用，label 更新（:159-169 + upsert）──
    let created_at = entry["createdAt"].as_str().unwrap().to_string();
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/studio/workspaces",
            Some(json!({ "path": extra.to_string_lossy(), "label": "Extra 2" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["workspace"]["mountId"], json!(mount_id));
    assert_eq!(body["workspace"]["label"], json!("Extra 2"));
    let registry: Value =
        serde_json::from_str(&std::fs::read_to_string(h.home.join("studio-mounts.json")).unwrap())
            .unwrap();
    let mounts = registry["mounts"].as_array().unwrap();
    assert_eq!(mounts.len(), 1, "同 mountId 覆盖不追加: {registry}");
    assert_eq!(mounts[0]["label"], json!("Extra 2"));
    assert_eq!(mounts[0]["createdAt"], json!(created_at), "createdAt 沿用");

    // ── 列举含挂载 + 挂载文件列表 ──
    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/studio/workspaces", None)
        .await;
    assert_eq!(status, 200, "{body}");
    let workspaces = body["workspaces"].as_array().unwrap();
    assert_eq!(workspaces.len(), 2, "{body}");
    assert!(workspaces
        .iter()
        .any(|w| w["mountId"] == json!(mount_id) && w["isDefault"] == json!(false)));
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            &format!("/lingxi/v1/studio/workspaces/{mount_id}/files"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["rootId"], json!(mount_id));
    assert_eq!(body["mountId"], json!(mount_id));
    assert_eq!(body["subdir"], json!(""));
    let files = body["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "{body}");
    assert_eq!(files[0]["name"], json!("seed.txt"));

    // ── 停用闸与停用后行为 ──
    let (status, body) = h
        .http(
            reqwest::Method::DELETE,
            "/lingxi/v1/studio/workspaces/default",
            None,
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], json!("default_workspace"));
    let (status, body) = h
        .http(
            reqwest::Method::DELETE,
            "/lingxi/v1/studio/workspaces/local_fs_missing",
            None,
        )
        .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], json!("workspace_not_found"));
    let (status, body) = h
        .http(
            reqwest::Method::DELETE,
            &format!("/lingxi/v1/studio/workspaces/{mount_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["mountId"], json!(mount_id));
    let registry: Value =
        serde_json::from_str(&std::fs::read_to_string(h.home.join("studio-mounts.json")).unwrap())
            .unwrap();
    assert_eq!(registry["mounts"][0]["status"], json!("disabled"));
    // 停用后从列举消失、文件面 404 unknown_root。
    let (_, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/studio/workspaces", None)
        .await;
    assert_eq!(body["workspaces"].as_array().unwrap().len(), 1);
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            &format!("/lingxi/v1/studio/workspaces/{mount_id}/files"),
            None,
        )
        .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], json!("unknown_root"));

    // ── 审计事件（create/remove 成功）──
    let actions = audit_actions(&h.home);
    assert!(
        actions
            .iter()
            .any(|a| a == "studio_workspace.create_local_path"),
        "create 审计: {actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|a| a == "studio_workspace.remove_local_path"),
        "remove 审计: {actions:?}"
    );

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workbench_files_search_projection_and_guards() {
    let h = Harness::boot("files").await;
    // 目录优先 + 名字序（候选人码点序，台账登记）：alpha/ Beta/ hello.txt zeta.md
    std::fs::create_dir_all(h.workspace.join("alpha")).unwrap();
    std::fs::create_dir_all(h.workspace.join("Beta")).unwrap();
    std::fs::write(h.workspace.join("alpha/a1.txt"), "a1").unwrap();
    std::fs::write(h.workspace.join("alpha/alpha-deep.txt"), "deep").unwrap();
    std::fs::write(h.workspace.join("zeta.md"), "# z\n").unwrap();
    std::fs::write(h.workspace.join("hello.txt"), "hello").unwrap();

    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/workbench/files", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["rootId"], json!("default"));
    assert_eq!(body["mountId"], json!("default"));
    assert_eq!(body["subdir"], json!(""));
    assert_eq!(
        body["mount"]["isDefault"],
        Value::Null,
        "mount 投影无 isDefault"
    );
    assert_eq!(
        body["mount"]["nativeRootPath"].as_str().unwrap(),
        h.workspace.to_string_lossy()
    );
    let files = body["files"].as_array().unwrap();
    let names: Vec<&str> = files.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["Beta", "alpha", "hello.txt", "zeta.md"],
        "目录优先: {files:?}"
    );
    for file in files {
        let is_dir = file["isDir"].as_bool().unwrap();
        if is_dir {
            assert_eq!(file["size"], Value::Null, "目录 size=null: {file}");
        } else {
            assert!(file["size"].is_number(), "文件 size 数值: {file}");
        }
        assert!(
            file["mtime"].as_str().unwrap().ends_with('Z'),
            "mtime ISO: {file}"
        );
    }

    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/files?subdir=alpha",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["subdir"], json!("alpha"));
    let names: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["a1.txt", "alpha-deep.txt"]);

    // subdir 闸：\ 与 .. 段 → 400 invalid_subdir。
    for bad in ["..", "a/../b", "a\\b", "alpha/../../etc"] {
        let (status, body) = h
            .http(
                reqwest::Method::GET,
                &format!("/lingxi/v1/workbench/files?subdir={bad}"),
                None,
            )
            .await;
        // axum 查询串原样进 handler（无 URL 解码需求——% 编码由 reqwest 原样发）。
        assert_eq!(status, 400, "{bad}: {body}");
        assert_eq!(body["error"], json!("invalid_subdir"), "{bad}: {body}");
    }
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/files?mountId=local_fs_missing",
            None,
        )
        .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], json!("unknown_root"));

    // ── 搜索（name 模式 + 空查询 + 投影）──
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/search?q=hello",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["query"], json!("hello"));
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "{body}");
    assert_eq!(results[0]["name"], json!("hello.txt"));
    assert_eq!(results[0]["relativePath"], json!("hello.txt"));
    assert_eq!(results[0]["parentSubdir"], json!(""));
    assert_eq!(results[0]["isDir"], json!(false));
    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/workbench/search?q=", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"], json!([]));
    // 大小写不敏感（search_names lowercase）。
    let (_, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/search?q=ALPHA",
            None,
        )
        .await;
    let names: Vec<&str> = body["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"alpha"), "大小写不敏感: {names:?}");
    assert!(names.contains(&"alpha-deep.txt"), "子串递归: {names:?}");

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workbench_actions_full_cycle() {
    let h = Harness::boot("actions").await;

    // ── mkdir ──
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({ "action": "mkdir", "name": "drafts" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["action"], json!("mkdir"));
    assert_eq!(body["rootId"], json!("default"));
    assert_eq!(body["mountId"], json!("default"));
    assert!(body["mount"].is_object(), "mount 投影: {body}");
    assert!(h.workspace.join("drafts").is_dir());
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({ "action": "mkdir", "name": "drafts" })),
        )
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], json!("already_exists"));
    for bad in ["..", "a/b", ""] {
        let (status, body) = h
            .http(
                reqwest::Method::POST,
                "/lingxi/v1/workbench/actions",
                Some(json!({ "action": "mkdir", "name": bad })),
            )
            .await;
        assert_eq!(status, 400, "{bad}: {body}");
        assert_eq!(body["error"], json!("invalid_name"), "{bad}: {body}");
    }

    // ── writeText：普通写 + 版本冲突 + 同版本写 ──
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "writeText", "subdir": "drafts",
                "name": "note.md", "content": "hello",
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["version"]["size"], json!(5));
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("drafts/note.md")).unwrap(),
        "hello"
    );
    // 改前存档：覆盖已存在文件 → checkpoints 目录出现 mobile-workbench-edit 记录。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "writeText", "subdir": "drafts",
                "name": "note.md", "content": "v2",
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    let checkpoints: Vec<_> = std::fs::read_dir(h.home.join("checkpoints"))
        .map(|rd| rd.filter_map(Result::ok).collect())
        .unwrap_or_default();
    assert!(!checkpoints.is_empty(), "改前存档落盘");
    let archive: Value =
        serde_json::from_str(&std::fs::read_to_string(checkpoints[0].path()).unwrap()).unwrap();
    assert_eq!(archive["tool"], json!("user-edit"));
    assert_eq!(archive["source"], json!("user-edit"));
    assert_eq!(archive["reason"], json!("mobile-workbench-edit"));
    assert_eq!(archive["content"], json!("hello"), "存档为改前内容");
    assert_eq!(archive["sessionPath"], Value::Null);

    let current_version = body["version"].clone();
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "writeText", "subdir": "drafts", "name": "note.md",
                "content": "should not land",
                "expectedVersion": { "mtimeMs": 1, "size": 1 },
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(false), "冲突是结果不是异常");
    assert_eq!(body["conflict"], json!(true));
    assert!(body["version"]["size"].is_number());
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("drafts/note.md")).unwrap(),
        "v2",
        "冲突保持外部内容"
    );
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "writeText", "subdir": "drafts", "name": "note.md",
                "content": "v3", "expectedVersion": current_version,
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true), "同版本写入: {body}");
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("drafts/note.md")).unwrap(),
        "v3"
    );
    // 畸形 expectedVersion → 400 invalid_expected_version（现役 normalize
    // 闸；不得落到 io 层被宽松接受）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "writeText", "subdir": "drafts", "name": "note.md",
                "content": "x", "expectedVersion": { "foo": 1 },
            })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], json!("invalid_expected_version"));

    // ── workbench 路由的 create 与 writeText 同语义（现役 mobile-workbench
    // :98-100 不传 mustNotExist；钉住该行为，desk 侧的 mustNotExist 是另一路由）──
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "create", "subdir": "drafts",
                "name": "note.md", "content": "overwrite via create",
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["action"], json!("create"));
    assert_eq!(
        std::fs::read_to_string(h.workspace.join("drafts/note.md")).unwrap(),
        "overwrite via create"
    );

    // ── rename / move / movePaths / safeDelete ──
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "rename", "subdir": "drafts",
                "oldName": "note.md", "newName": "renamed.md",
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert!(h.workspace.join("drafts/renamed.md").is_file());
    assert!(!h.workspace.join("drafts/note.md").exists());

    // move 到根（destSubdir ""）；再 movePaths 回 drafts。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "move", "subdir": "drafts",
                "name": "renamed.md", "destSubdir": "",
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(h.workspace.join("renamed.md").is_file());

    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "movePaths",
                "items": [{ "sourceSubdir": "", "name": "renamed.md" }],
                "destSubdir": "drafts", "currentSubdir": "",
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["action"], json!("movePaths"));
    assert_eq!(
        body["results"],
        json!([{ "name": "renamed.md", "ok": true }])
    );
    assert!(body["filesByPath"][""].is_array());
    assert!(body["filesByPath"]["drafts"].is_array());
    assert!(h.workspace.join("drafts/renamed.md").is_file());

    // movePaths 逐项失败形状：not found / target exists / 同键 skipped /
    // 目录入自身。
    std::fs::write(h.workspace.join("dup.txt"), "a").unwrap();
    std::fs::write(h.workspace.join("drafts/dup.txt"), "b").unwrap();
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "movePaths",
                "items": [
                    { "sourceSubdir": "", "name": "missing.txt" },
                    { "sourceSubdir": "", "name": "dup.txt" },
                    { "sourceSubdir": "drafts", "name": "renamed.md" },
                    { "sourceSubdir": "", "name": "drafts" },
                ],
                "destSubdir": "drafts", "currentSubdir": "",
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let results = body["results"].as_array().unwrap();
    assert_eq!(
        results[0],
        json!({ "name": "missing.txt", "error": "not found" })
    );
    assert_eq!(
        results[1],
        json!({ "name": "dup.txt", "error": "target already exists" })
    );
    // renamed.md 已在 drafts → 同键 skipped。
    assert_eq!(
        results[2],
        json!({ "name": "renamed.md", "ok": true, "skipped": true })
    );
    assert_eq!(
        results[3],
        json!({ "name": "drafts", "error": "cannot move folder into itself" })
    );

    // createDestIfMissing:false + 缺失目录 → 400 dest_not_directory。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "movePaths",
                "items": [{ "sourceSubdir": "", "name": "dup.txt" }],
                "destSubdir": "no-such-dir", "currentSubdir": "",
                "createDestIfMissing": false,
            })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], json!("dest_not_directory"));

    // safeDelete → trash 台账 + 文件消失。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({ "action": "safeDelete", "subdir": "drafts", "name": "renamed.md" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    let trash_id = body["trashId"].as_str().unwrap().to_string();
    assert!(trash_id.starts_with("trash_"), "{body}");
    assert!(!h.workspace.join("drafts/renamed.md").exists());
    let trash_meta: Value = serde_json::from_str(
        &std::fs::read_to_string(
            h.home
                .join("trash/mobile-workbench")
                .join(&trash_id)
                .join("metadata.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(trash_meta["originalName"], json!("renamed.md"));
    assert_eq!(trash_meta["originalSubdir"], json!("drafts"));
    assert_eq!(trash_meta["mountId"], json!("default"));

    // 未知动作 → 400 unknown_action。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({ "action": "purge" })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], json!("unknown_action"));

    // 写动作审计（mobile_workbench.*）。
    let actions = audit_actions(&h.home);
    for expected in [
        "mobile_workbench.mkdir",
        "mobile_workbench.write",
        "mobile_workbench.rename",
        "mobile_workbench.move",
        "mobile_workbench.move_paths",
        "mobile_workbench.safe_delete",
    ] {
        assert!(
            actions.iter().any(|a| a == expected),
            "{expected}: {actions:?}"
        );
    }

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workbench_upload_and_content() {
    let h = Harness::boot("upload").await;

    // ── upload：逐项结果 + 目录清单 ──
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/upload",
            Some(json!({
                "files": [
                    { "name": "a.bin", "contentBase64": b64(b"hello") },
                    { "name": "b.txt", "contentBase64": "" },
                    { "contentBase64": b64(b"no-name") },
                ],
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(false), "存在失败项时 ok=false");
    assert_eq!(body["rootId"], json!("default"));
    let results = body["results"].as_array().unwrap();
    assert_eq!(
        results[0],
        json!({ "name": "a.bin", "ok": true, "size": 5 })
    );
    assert_eq!(
        results[1],
        json!({ "name": "b.txt", "ok": false, "error": "invalid_upload" })
    );
    assert_eq!(
        results[2],
        json!({ "name": Value::Null, "ok": false, "error": "invalid_name" })
    );
    assert_eq!(std::fs::read(h.workspace.join("a.bin")).unwrap(), b"hello");
    assert!(body["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"] == "a.bin"));

    // 单文件超 25MB → file_too_large（413 项级错误，结果形状保持）。
    let oversized = vec![b'x'; 25 * 1024 * 1024 + 1];
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/upload",
            Some(json!({
                "files": [{ "name": "big.bin", "contentBase64": b64(&oversized) }],
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["results"][0],
        json!({ "name": "big.bin", "ok": false, "error": "file_too_large" })
    );
    assert!(!h.workspace.join("big.bin").exists());

    // ── content：GET/HEAD/ETag/304/Range/416/404/类型表 ──
    std::fs::write(h.workspace.join("note.md"), "# title\n").unwrap();
    let (status, headers, bytes) = h
        .http_raw(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/content?name=note.md",
            &[],
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(bytes, b"# title\n");
    assert_eq!(
        headers.get("content-type").unwrap().to_str().unwrap(),
        "text/markdown; charset=utf-8"
    );
    let size_str = format!("{}", b"# title\n".len());
    assert_eq!(
        headers.get("x-lingxi-file-size").unwrap().to_str().unwrap(),
        size_str
    );
    assert!(headers.get("x-lingxi-file-mtimems").is_some());
    assert_eq!(
        headers.get("cache-control").unwrap().to_str().unwrap(),
        "private, max-age=0, must-revalidate"
    );
    assert_eq!(
        headers.get("accept-ranges").unwrap().to_str().unwrap(),
        "bytes"
    );
    let disposition = headers
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        disposition.starts_with("inline; filename=\"note.md\""),
        "{disposition}"
    );
    let etag = headers.get("etag").unwrap().to_str().unwrap().to_string();
    assert!(etag.starts_with("W/\""), "{etag}");

    // If-None-Match → 304 空体 + ETag。
    let (status, headers, bytes) = h
        .http_raw(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/content?name=note.md",
            &[("if-none-match", &etag)],
        )
        .await;
    assert_eq!(status, 304);
    assert!(bytes.is_empty());
    assert_eq!(headers.get("etag").unwrap().to_str().unwrap(), etag);

    // Range 206 + Content-Range；越界 416 + bytes */size。
    let (status, headers, bytes) = h
        .http_raw(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/content?name=note.md",
            &[("range", "bytes=2-5")],
        )
        .await;
    assert_eq!(status, 206);
    assert_eq!(bytes, b"titl");
    assert_eq!(
        headers.get("content-range").unwrap().to_str().unwrap(),
        format!("bytes 2-5/{size_str}")
    );
    let (status, headers, _) = h
        .http_raw(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/content?name=note.md",
            &[("range", "bytes=99-100")],
        )
        .await;
    assert_eq!(status, 416);
    assert_eq!(
        headers.get("content-range").unwrap().to_str().unwrap(),
        format!("bytes */{size_str}")
    );

    // HEAD：状态与头同源、空体。
    let (status, headers, bytes) = h
        .http_raw(
            reqwest::Method::HEAD,
            "/lingxi/v1/workbench/content?name=note.md",
            &[],
        )
        .await;
    assert_eq!(status, 200);
    assert!(bytes.is_empty());
    assert_eq!(
        headers.get("content-length").unwrap().to_str().unwrap(),
        size_str
    );

    // 缺失 → 404 file_not_found；目录名 → 400 not_a_file；坏名 → 400。
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/content?name=missing.txt",
            None,
        )
        .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], json!("file_not_found"));
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/content?name=..",
            None,
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], json!("invalid_name"));
    std::fs::create_dir_all(h.workspace.join("dir-entry")).unwrap();
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/content?name=dir-entry",
            None,
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], json!("not_a_file"));

    // 二进制默认类型 + UTF-8 文件名处置头（filename* 百分号编码）。
    let (status, headers, _) = h
        .http_raw(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/content?name=a.bin",
            &[],
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        headers.get("content-type").unwrap().to_str().unwrap(),
        "application/octet-stream"
    );
    std::fs::write(h.workspace.join("笔记.md"), "# 笔记\n").unwrap();
    let (status, headers, _) = h
        .http_raw(
            reqwest::Method::GET,
            "/lingxi/v1/workbench/content?name=%E7%AC%94%E8%AE%B0.md",
            &[],
        )
        .await;
    assert_eq!(status, 200);
    let disposition = headers
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        disposition.contains("filename*=UTF-8''%E7%AC%94%E8%AE%B0.md"),
        "{disposition}"
    );
    assert!(
        disposition.contains("filename=\"download.md\""),
        "{disposition}"
    );

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workbench_scope_gate_and_remote_write_lease() {
    let h = Harness::boot("scope").await;

    // 无 scope 凭证：读面也拒（403 insufficient_scope）。中间件拒绝信封是
    // EndpointError 统一形状（details.reason / details.requiredScope），不是
    // 现役处理器内 {error,reason,capability} 形状——现役的 scope 检查在路由
    // 处理器里，Rust 侧由 auth 中间件先行拒绝（已登记的口径差异）。
    let denied = h
        .state
        .auth()
        .issue_device_credential("user_local", &[], None)
        .unwrap()
        .secret;
    let (status, body) = h
        .http_as(
            &denied,
            reqwest::Method::GET,
            "/lingxi/v1/workbench/files",
            None,
        )
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["details"]["reason"], json!("insufficient_scope"));
    assert_eq!(body["details"]["requiredScope"], json!("files.read"));

    // files.read 凭证：读面放行；写面 403。
    let reader = h
        .state
        .auth()
        .issue_device_credential("user_local", &["files.read"], None)
        .unwrap()
        .secret;
    let (status, _) = h
        .http_as(
            &reader,
            reqwest::Method::GET,
            "/lingxi/v1/workbench/files",
            None,
        )
        .await;
    assert_eq!(status, 200);
    let (status, body) = h
        .http_as(
            &reader,
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({ "action": "mkdir", "name": "nope" })),
        )
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["details"]["reason"], json!("insufficient_scope"));
    assert_eq!(body["details"]["requiredScope"], json!("files.write"));

    // files.write 非属主凭证：写面放行，且产生写租约台账（issued→consumed）。
    let writer_issued = h
        .state
        .auth()
        .issue_device_credential("user_local", &["files.read", "files.write"], None)
        .unwrap();
    let writer = writer_issued.secret.clone();
    let (status, body) = h
        .http_as(
            &writer,
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({ "action": "mkdir", "name": "remote-dir" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert!(h.workspace.join("remote-dir").is_dir());
    // 非属主投影不披露 nativeRootPath。
    let (status, body) = h
        .http_as(
            &writer,
            reqwest::Method::GET,
            "/lingxi/v1/workbench/files",
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["mount"].get("nativeRootPath").is_none(), "{body}");

    let ledger: Value = serde_json::from_str(
        &std::fs::read_to_string(h.home.join("security/execution-leases.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(ledger["schemaVersion"], json!(1));
    let leases = ledger["leases"].as_array().unwrap();
    assert_eq!(leases.len(), 1, "{ledger}");
    let lease = &leases[0];
    assert_eq!(lease["agentId"], json!("mobile_workbench"));
    assert_eq!(lease["sessionId"], json!("mobile_workbench"));
    assert_eq!(lease["commandClass"], json!("write_files"));
    assert_eq!(lease["sandboxProfile"], json!("workspace_write"));
    assert_eq!(lease["backupPolicy"], json!("snapshot_before_write"));
    assert_eq!(lease["resourceIds"], json!(["default"]));
    assert_eq!(lease["mountId"], Value::Null);
    assert_eq!(lease["status"], json!("consumed"), "{lease}");
    assert!(lease["consumedAt"].is_string());
    assert!(!lease["actorPrincipalId"].as_str().unwrap().is_empty());

    // 非属主创建/停用挂载 → 403 local_owner_required。
    let (status, body) = h
        .http_as(
            &writer,
            reqwest::Method::POST,
            "/lingxi/v1/studio/workspaces",
            Some(json!({ "path": h.workspace.to_string_lossy() })),
        )
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], json!("local_owner_required"));
    assert_eq!(
        body["capability"],
        json!("studio.workspace.create_local_path")
    );
    let (status, body) = h
        .http_as(
            &writer,
            reqwest::Method::DELETE,
            "/lingxi/v1/studio/workspaces/local_fs_anything",
            None,
        )
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], json!("local_owner_required"));

    // local owner 写动作不产生租约记录（isLocalOwner → null）。
    let before = std::fs::read_to_string(h.home.join("security/execution-leases.json")).unwrap();
    let (status, _) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({ "action": "mkdir", "name": "owner-dir" })),
        )
        .await;
    assert_eq!(status, 200);
    let after = std::fs::read_to_string(h.home.join("security/execution-leases.json")).unwrap();
    assert_eq!(before, after, "local owner 不写租约台账");

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn workbench_on_mount_root_end_to_end() {
    let h = Harness::boot("mount").await;
    let extra = h.home.join("mount-root");
    std::fs::create_dir_all(extra.join("sub")).unwrap();
    std::fs::write(extra.join("sub/inner.txt"), "inner").unwrap();

    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/studio/workspaces",
            Some(json!({ "path": extra.to_string_lossy(), "label": "Mount" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let mount_id = body["workspace"]["mountId"].as_str().unwrap().to_string();

    // 挂载面文件列表（mount 引用经 MountProvider 租约解析）。
    let (status, body) = h
        .http(
            reqwest::Method::GET,
            &format!("/lingxi/v1/workbench/files?mountId={mount_id}&subdir=sub"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["mountId"], json!(mount_id));
    assert_eq!(body["subdir"], json!("sub"));
    assert_eq!(body["files"][0]["name"], json!("inner.txt"));

    // 挂载面写入 + 读取回环。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "writeText", "mountId": mount_id, "subdir": "sub",
                "name": "new.txt", "content": "mounted",
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(
        std::fs::read_to_string(extra.join("sub/new.txt")).unwrap(),
        "mounted"
    );
    let (status, bytes_body) = {
        let (status, _headers, bytes) = h
            .http_raw(
                reqwest::Method::GET,
                &format!("/lingxi/v1/workbench/content?mountId={mount_id}&subdir=sub&name=new.txt"),
                &[],
            )
            .await;
        (status, bytes)
    };
    assert_eq!(status, 200);
    assert_eq!(bytes_body, b"mounted");

    // 停用挂载 → 写面 404 unknown_root（A13 联动前奏：读取面同拒）。
    let (status, _) = h
        .http(
            reqwest::Method::DELETE,
            &format!("/lingxi/v1/studio/workspaces/{mount_id}"),
            None,
        )
        .await;
    assert_eq!(status, 200);
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/workbench/actions",
            Some(json!({
                "action": "writeText", "mountId": mount_id, "subdir": "sub",
                "name": "blocked.txt", "content": "x",
            })),
        )
        .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], json!("unknown_root"));
    assert!(!extra.join("sub/blocked.txt").exists());

    h.shutdown().await;
}

/// A13：停用成功就是撤权边界，原监听和组合订阅必须在响应前释放。
#[tokio::test]
async fn mount_disable_retires_watch_and_subscription_before_returning() {
    let h = Harness::boot("a13").await;
    let extra = h.home.join("watched");
    std::fs::create_dir_all(&extra).unwrap();
    std::fs::write(extra.join("seed.txt"), "before").unwrap();
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/studio/workspaces",
            Some(json!({"path":extra})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let id = body["workspace"]["mountId"].as_str().unwrap();
    let resource = json!({"kind":"mount","mountId":id,"path":""});
    let (status, watch) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/resource-io/watch",
            Some(resource.clone()),
        )
        .await;
    assert_eq!(status, 200, "{watch}");
    let (status, subscription) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/resource-io/subscribe",
            Some(json!({"resources":[resource],"purpose":"a13"})),
        )
        .await;
    assert_eq!(status, 200, "{subscription}");
    let before = h.state.resource_io().diagnostics();
    assert_eq!(before["watches"].as_array().unwrap().len(), 1);
    assert_eq!(before["subscriptions"], 1);
    let (status, body) = h
        .http(
            reqwest::Method::DELETE,
            &format!("/lingxi/v1/studio/workspaces/{id}"),
            None,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let after = h.state.resource_io().diagnostics();
    assert_eq!(after["watches"], json!([]), "{after}");
    assert_eq!(after["subscriptions"], 0, "{after}");
    let cursor = h.state.resource_io().events_since(0)["latestSequence"]
        .as_u64()
        .unwrap_or(0);
    std::fs::write(extra.join("seed.txt"), "after withdrawal").unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    assert_eq!(
        h.state.resource_io().events_since(cursor)["events"],
        json!([])
    );
    let (status, denied) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/resource-io/watch",
            Some(json!({"kind":"mount","mountId":id,"path":""})),
        )
        .await;
    assert_ne!(status, 200, "{denied}");
    h.shutdown().await;
}

#[tokio::test]
async fn resource_watch_shutdown_releases_resources_with_live_state_reference() {
    let h = Harness::boot("watch-exit").await;
    h.state
        .resource_io()
        .watch(&json!({"kind":"local-file","path":h.workspace}))
        .await
        .unwrap();
    h.state.resource_io().shutdown();
    assert!(h
        .state
        .resource_io()
        .watch(&json!({"kind":"local-file","path":h.workspace}))
        .await
        .is_err());
    assert_eq!(h.state.resource_io().diagnostics()["watches"], json!([]));
    assert_eq!(h.state.resource_io().diagnostics()["subscriptions"], 0);
    h.shutdown().await;
}

/// A14：从旧偏好恢复后关闭实验，读取展示快照和再次启动都不能回写展示数组。
#[tokio::test]
async fn legacy_experiment_map_survives_snapshot_single_edit_and_restart() {
    let h = Harness::boot("a14").await;
    let prefs = h.home.join("user/preferences.json");
    std::fs::write(
        &prefs,
        serde_json::to_vec(&json!({
            "current_agent_id":"a", "unknown_compat":{"keep":true},
            "experiments":{"subagent.proactive_delegation":true,"custom.future":"keep",
                "memory.cache_snapshot_reflection":"on"}
        }))
        .unwrap(),
    )
    .unwrap();
    let (status, body) = h
        .http(
            reqwest::Method::PATCH,
            "/lingxi/v1/experiments/subagent.proactive_delegation",
            Some(json!({"value":false})),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let before = std::fs::read(&prefs).unwrap();
    let (status, snapshot) = h
        .http(
            reqwest::Method::GET,
            "/lingxi/v1/settings/snapshot?agentId=a",
            None,
        )
        .await;
    assert_eq!(status, 200, "{snapshot}");
    assert!(snapshot["preferences"]["experiments"].is_array());
    assert_eq!(
        std::fs::read(&prefs).unwrap(),
        before,
        "展示用数组不得写回旧字典"
    );
    let stored: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(
        stored["experiments"]["subagent.proactive_delegation"],
        false
    );
    assert_eq!(stored["experiments"]["custom.future"], "keep");
    assert_eq!(stored["unknown_compat"]["keep"], true);
    let _ = h.stop.send(());
    h.server.await.unwrap();
    h.state.storage().close().await.unwrap();
    let layout = prepare_layout(&h.home).unwrap();
    let restarted = ServiceState::bootstrap_with_deps(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: h.home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: 1000,
        },
        &layout,
        ServiceDeps {
            workspace_root: Some(h.workspace.clone()),
            ..ServiceDeps::default()
        },
    )
    .await
    .unwrap();
    let response = lingxi_service::agents_admin::agent_config_readback::experiments_list_route(
        axum::extract::State(restarted.clone()),
    )
    .await;
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let resolved: Value = serde_json::from_slice(&bytes).unwrap();
    let experiments = resolved["experiments"].as_array().unwrap();
    assert_eq!(
        experiments
            .iter()
            .find(|e| e["id"] == "subagent.proactive_delegation")
            .unwrap()["value"],
        false
    );
    assert_eq!(std::fs::read(&prefs).unwrap(), before);
    restarted.storage().close().await.unwrap();
    std::fs::remove_dir_all(&h.home).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_writes_refuse_corrupt_or_unreadable_lease_registry_without_file_changes() {
    for kind in ["corrupt", "directory", "bad-shape"] {
        let h = Harness::boot(&format!("lease-fault-{kind}")).await;
        let writer = h
            .state
            .auth()
            .issue_device_credential("user_local", &["files.read", "files.write"], None)
            .unwrap()
            .secret;
        let ledger = h.home.join("security/execution-leases.json");
        std::fs::create_dir_all(ledger.parent().unwrap()).unwrap();
        if kind == "directory" {
            std::fs::create_dir(&ledger).unwrap();
            std::fs::write(ledger.join("preserved"), "original").unwrap();
        } else {
            std::fs::write(
                &ledger,
                if kind == "corrupt" {
                    "{broken"
                } else {
                    "{\"schemaVersion\":1,\"leases\":{}}"
                },
            )
            .unwrap();
        }
        std::fs::write(h.workspace.join("sentinel.txt"), "original").unwrap();
        for (path, body) in [
            (
                "/lingxi/v1/workbench/actions",
                json!({"action":"mkdir","name":"must-not-exist"}),
            ),
            (
                "/lingxi/v1/workbench/actions",
                json!({"action":"writeText","name":"sentinel.txt","content":"changed"}),
            ),
            (
                "/lingxi/v1/workbench/upload",
                json!({"name":"sentinel.txt","contentBase64":b64(b"uploaded")}),
            ),
        ] {
            let (status, result) = h
                .http_as(&writer, reqwest::Method::POST, path, Some(body))
                .await;
            assert_eq!(status, 503, "{kind}: {result}");
            assert_eq!(result["error"], "write_lease_unavailable", "{result}");
            assert!(!h.workspace.join("must-not-exist").exists());
            assert_eq!(
                std::fs::read_to_string(h.workspace.join("sentinel.txt")).unwrap(),
                "original"
            );
        }
        if kind == "directory" {
            assert_eq!(
                std::fs::read_to_string(ledger.join("preserved")).unwrap(),
                "original"
            );
        } else {
            assert_eq!(
                std::fs::read_to_string(&ledger).unwrap(),
                if kind == "corrupt" {
                    "{broken"
                } else {
                    "{\"schemaVersion\":1,\"leases\":{}}"
                }
            );
        }
        h.shutdown().await;
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_writes_refuse_unwritable_lease_registry_before_mutation() {
    use std::os::unix::fs::PermissionsExt;
    let h = Harness::boot("lease-parent-readonly").await;
    let writer = h
        .state
        .auth()
        .issue_device_credential("user_local", &["files.read", "files.write"], None)
        .unwrap()
        .secret;
    let security = h.home.join("security");
    std::fs::create_dir_all(&security).unwrap();
    let ledger = security.join("execution-leases.json");
    let raw = json!({"schemaVersion":1,"leases":[],"createdAt":"2026-10-18T00:00:00Z","updatedAt":"2026-10-18T00:00:00Z"}).to_string();
    std::fs::write(&ledger, &raw).unwrap();
    std::fs::write(h.workspace.join("sentinel.txt"), "original").unwrap();
    std::fs::set_permissions(&security, std::fs::Permissions::from_mode(0o500)).unwrap();
    for (path, body) in [
        (
            "/lingxi/v1/workbench/actions",
            json!({"action":"writeText","name":"sentinel.txt","content":"changed"}),
        ),
        (
            "/lingxi/v1/workbench/upload",
            json!({"name":"new.txt","contentBase64":b64(b"uploaded")}),
        ),
    ] {
        let (status, result) = h
            .http_as(&writer, reqwest::Method::POST, path, Some(body))
            .await;
        assert_eq!(status, 503, "{result}");
        assert_eq!(result["error"], "write_lease_unavailable");
        assert_eq!(
            std::fs::read_to_string(h.workspace.join("sentinel.txt")).unwrap(),
            "original"
        );
        assert!(!h.workspace.join("new.txt").exists());
        assert_eq!(std::fs::read_to_string(&ledger).unwrap(), raw);
    }
    std::fs::set_permissions(&security, std::fs::Permissions::from_mode(0o700)).unwrap();
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lease_settlement_failure_reports_landed_action_and_upload_without_fake_success() {
    for upload in [false, true] {
        let h = Harness::boot(if upload {
            "lease-upload-settle"
        } else {
            "lease-action-settle"
        })
        .await;
        let writer = h
            .state
            .auth()
            .issue_device_credential("user_local", &["files.read", "files.write"], None)
            .unwrap()
            .secret;
        let ledger = h.home.join("security/execution-leases.json");
        let target = h.workspace.join("landed.txt");
        h.state
            .resource_io()
            .file_safety()
            .set_test_hook(Some(std::sync::Arc::new({
                let ledger = ledger.clone();
                let target = target.clone();
                move |path, step| {
                    if path == target && step == "after_replace" {
                        std::fs::write(&ledger, "{settlement-corrupt")
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(())
                }
            })));
        let (path, body) = if upload {
            (
                "/lingxi/v1/workbench/upload",
                json!({"name":"landed.txt","contentBase64":b64(b"actual result")}),
            )
        } else {
            (
                "/lingxi/v1/workbench/actions",
                json!({"action":"writeText","name":"landed.txt","content":"actual result"}),
            )
        };
        let (status, result) = h
            .http_as(&writer, reqwest::Method::POST, path, Some(body))
            .await;
        assert_eq!(status, 503, "{result}");
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"], "write_lease_settlement_failed");
        assert_eq!(result["executionState"], "result_produced");
        assert_eq!(result["leaseState"], "unknown");
        assert_eq!(result["canRetry"], false);
        assert_eq!(result["result"]["ok"], true, "{result}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "actual result");
        assert_eq!(
            std::fs::read_to_string(&ledger).unwrap(),
            "{settlement-corrupt"
        );
        h.state.resource_io().file_safety().set_test_hook(None);
        h.shutdown().await;
    }
}
