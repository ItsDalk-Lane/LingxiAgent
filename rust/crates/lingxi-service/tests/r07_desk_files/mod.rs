//! R07-T07 β3：desk 工作台文件面集成测试。
//!
//! 覆盖：desk/path（默认根、dir 覆盖、批准门禁含父目录回退怪癖、
//! home_folder/cwd_history 根、五语言文案）、desk/files（列表投影、
//! 子目录守卫、"invalid path" 符号链接逃逸）、desk/search-files
//! （query/limit 语义）、desk/jian（读写删循环 + 非字符串 content）、
//! desk/files 七动作（upload 的 local-owner 闸/敏感路径/目录与符号链
//! 接复制、create 的 mustNotExist、mkdir/rename/move/movePaths/remove
//! 与全部守卫形态）、scope 门禁（中间件信封）。
//!
//! 夹具：真 TCP loopback + 真实 ServiceState；desk 根 = 注入的
//! workspace_root（现役默认 = OH-WorkSpace 默认工作区）；无外部服务。

use lingxi_service::{
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

struct Harness {
    home: PathBuf,
    workspace: Option<PathBuf>,
    state: ServiceState,
    base: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn boot(tag: &str) -> Self {
        Self::boot_with_workspace(tag, true).await
    }

    async fn boot_without_workspace(tag: &str) -> Self {
        Self::boot_with_workspace(tag, false).await
    }

    async fn boot_with_workspace(tag: &str, with_workspace: bool) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-desk-{tag}-{}",
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
        let workspace = with_workspace.then(|| {
            let workspace = home.join("workspace");
            std::fs::create_dir_all(&workspace).unwrap();
            workspace
        });
        let home = home.canonicalize().unwrap();
        let workspace = workspace.map(|w| w.canonicalize().unwrap());
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
                workspace_root: workspace.clone(),
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

    fn workspace(&self) -> &Path {
        self.workspace.as_ref().expect("workspace")
    }

    fn write_agent_config(&self, yaml: &str) {
        std::fs::write(self.home.join("agents/a/config.yaml"), yaml).unwrap();
    }

    fn write_preferences(&self, prefs: Value) {
        std::fs::write(
            self.home.join("user/preferences.json"),
            serde_json::to_string(&prefs).unwrap(),
        )
        .unwrap();
    }

    async fn http(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        self.http_as(&self.state.auth().local_token(), method, path, body)
            .await
    }

    async fn http_q(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
    ) -> (u16, Value) {
        let mut url = format!("{}{path}", self.base);
        if !query.is_empty() {
            let encoded: Vec<String> = query
                .iter()
                .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
                .collect();
            url = format!("{url}?{}", encoded.join("&"));
        }
        let response = reqwest::Client::new()
            .request(method, url)
            .bearer_auth(self.state.auth().local_token())
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
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
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn shutdown(self) {
        let _ = self.stop.send(());
        let _ = self.server.await;
        self.state.storage().close().await.unwrap();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// 家外目录（门禁负例与 upload 源）。调用方负责清理。
fn outside_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r07-desk-outside-{tag}-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// application/x-www-form-urlencoded 的最小编码（查询值含 / 与中文）。
fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_default_override_and_approval() {
    let h = Harness::boot("path").await;
    let workspace = h.workspace().to_path_buf();

    // 默认：无 home_folder → 服务 workspace_root（现役 homeCwd 兜底）。
    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/desk/path", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["path"], json!(workspace.to_string_lossy()));

    // dir 覆盖获批（workspace 本身是批准根）。
    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/path",
            &[("dir", &workspace.to_string_lossy())],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["path"], json!(workspace.to_string_lossy()));

    // 家外目录：未获批 → 200 守卫字符串（默认 zh 文案）。
    let outside = outside_dir("path");
    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/path",
            &[("dir", &outside.to_string_lossy())],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["error"], json!("目录不在允许范围内"));

    // 父目录回退怪癖（_realPathForWorkspaceCheck）：workspace 下不存在的
    // 子目录按父目录解析获批，并被 mkdir。
    let missing_child = workspace.join("later-created");
    assert!(!missing_child.exists());
    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/path",
            &[("dir", &missing_child.to_string_lossy())],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["path"], json!(missing_child.to_string_lossy()));
    assert!(missing_child.is_dir());

    // home_folder 覆盖：写进助手配置后默认 desk 目录跟随。
    let home_folder = outside_dir("home-folder");
    h.write_agent_config(&format!(
        "agent:\n  name: a\nmemory:\n  enabled: false\ndesk:\n  home_folder: \"{}\"\n",
        home_folder.display()
    ));
    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/desk/path", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["path"], json!(home_folder.to_string_lossy()));

    // cwd_history 目录也获批（覆盖装回 workspace，获批靠 history）。
    // 现役 cwd_history 在 config.yaml 顶层（config.ts:233 写顶层）。
    let history_dir = outside_dir("history");
    h.write_agent_config(&format!(
        "agent:\n  name: a\nmemory:\n  enabled: false\ndesk:\n  home_folder: \"{}\"\ncwd_history:\n  - \"{}\"\n",
        home_folder.display(),
        history_dir.display()
    ));
    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/path",
            &[("dir", &history_dir.to_string_lossy())],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["path"], json!(history_dir.to_string_lossy()));

    // home_folder 缺失（目录不存在）→ 回落默认工作区，配置不被改写
    // （登记偏差：现役惰性清洗由候选人 GC 承担）。
    let gone = outside_dir("gone");
    std::fs::remove_dir_all(&gone).unwrap();
    h.write_agent_config(&format!(
        "agent:\n  name: a\nmemory:\n  enabled: false\ndesk:\n  home_folder: \"{}\"\n",
        gone.display()
    ));
    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/desk/path", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["path"], json!(workspace.to_string_lossy()));
    let config = std::fs::read_to_string(h.home.join("agents/a/config.yaml")).unwrap();
    assert!(config.contains("home_folder"), "{config}");

    for dir in [outside, home_folder, history_dir] {
        let _ = std::fs::remove_dir_all(dir);
    }
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn files_listing_and_subdir_guards() {
    let h = Harness::boot("files").await;
    let workspace = h.workspace().to_path_buf();
    std::fs::write(workspace.join("b-file.txt"), "b").unwrap();
    std::fs::create_dir_all(workspace.join("a-dir")).unwrap();
    std::fs::write(workspace.join("a-dir/inner.md"), "inner").unwrap();

    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/desk/files", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["subdir"], json!(""));
    assert_eq!(body["basePath"], json!(workspace.to_string_lossy()));
    let files = body["files"].as_array().unwrap();
    // 目录优先 + 码点序（登记的排序口径）。
    assert_eq!(files.len(), 2, "{body}");
    assert_eq!(files[0]["name"], json!("a-dir"));
    assert_eq!(files[0]["isDir"], json!(true));
    assert_eq!(files[0]["size"], Value::Null);
    assert_eq!(files[1]["name"], json!("b-file.txt"));
    assert_eq!(files[1]["isDir"], json!(false));
    assert_eq!(files[1]["size"], json!(1));
    assert!(files[0]["mtime"].is_string());

    // 子目录浏览：subdir 原文回显。
    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/files",
            &[("subdir", "a-dir")],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["subdir"], json!("a-dir"));
    assert_eq!(body["files"][0]["name"], json!("inner.md"));

    // substring 守卫："a..b" 也拒（现役 includes("..") 语义）。
    for bad in ["..", "a..b", "a\\b"] {
        let (status, body) = h
            .http_q(
                reqwest::Method::GET,
                "/lingxi/v1/desk/files",
                &[("subdir", bad)],
            )
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["error"], json!("invalid subdir"), "{bad}");
    }

    // 符号链接逃逸 → invalid path（realpath 后在基目录之外）。
    let outside = outside_dir("escape");
    let link = workspace.join("linkout");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/files",
            &[("subdir", "linkout")],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["error"], json!("invalid path"));

    // dir 覆盖：workspace 内的子目录（批准根之内）直接作为基目录。
    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/files",
            &[("dir", &workspace.join("a-dir").to_string_lossy())],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["basePath"],
        json!(workspace.join("a-dir").to_string_lossy())
    );
    assert_eq!(body["files"][0]["name"], json!("inner.md"));

    let _ = std::fs::remove_dir_all(outside);
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn search_files_flow() {
    let h = Harness::boot("search").await;
    let workspace = h.workspace().to_path_buf();
    std::fs::create_dir_all(workspace.join("alpha")).unwrap();
    std::fs::write(workspace.join("alpha/alpha-note.txt"), "x").unwrap();
    std::fs::write(workspace.join("alpha/beta.txt"), "x").unwrap();
    std::fs::write(workspace.join("alpha.md"), "x").unwrap();

    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/search-files",
            &[("q", "alpha")],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["basePath"], json!(workspace.to_string_lossy()));
    assert_eq!(body["query"], json!("alpha"));
    let results = body["results"].as_array().unwrap();
    let names: Vec<&str> = results.iter().filter_map(|r| r["name"].as_str()).collect();
    assert!(names.contains(&"alpha"), "{names:?}");
    assert!(names.contains(&"alpha-note.txt"), "{names:?}");
    assert!(names.contains(&"alpha.md"), "{names:?}");
    assert!(!names.contains(&"beta.txt"), "{names:?}");
    assert!(results[0]["relativePath"].is_string(), "{results:?}");

    // limit 语义：1 → 截断 1；非数字/0/负数 → 默认 80（parseInt 口径）。
    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/search-files",
            &[("q", "alpha"), ("limit", "1")],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"].as_array().unwrap().len(), 1);
    for raw in ["abc", "0", "-3"] {
        let (status, body) = h
            .http_q(
                reqwest::Method::GET,
                "/lingxi/v1/desk/search-files",
                &[("q", "alpha"), ("limit", raw)],
            )
            .await;
        assert_eq!(status, 200, "{body}");
        assert!(
            body["results"].as_array().unwrap().len() >= 3,
            "{raw}: {body}"
        );
    }
    // parseInt 前缀语义："2xyz" → 2。
    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/search-files",
            &[("q", "alpha"), ("limit", "2xyz")],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"].as_array().unwrap().len(), 2, "{body}");

    // 空 query → 空结果（现役 searchFiles 的 q 为空不查）。
    let (status, body) = h
        .http_q(reqwest::Method::GET, "/lingxi/v1/desk/search-files", &[])
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"], json!([]));
    assert_eq!(body["query"], json!(""));

    // 默认上限必须由超过上限的真实匹配证明，不能只检查参数默认值。
    for index in 0..101 {
        std::fs::write(
            workspace.join(format!("bulk-match-{index:03}.txt")),
            "entry",
        )
        .unwrap();
    }
    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/search-files",
            &[("q", "bulk-match-")],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"].as_array().unwrap().len(), 80, "{body}");

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jian_cycle_and_locale() {
    let h = Harness::boot("jian").await;
    let workspace = h.workspace().to_path_buf();

    // 缺失 → content: null。
    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/desk/jian", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["content"], Value::Null);

    // 写入 → 读回。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/jian",
            Some(json!({ "content": "今日要点：交付 β3" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["content"], json!("今日要点：交付 β3"));
    assert_eq!(
        std::fs::read_to_string(workspace.join("jian.md")).unwrap(),
        "今日要点：交付 β3"
    );
    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/desk/jian", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["content"], json!("今日要点：交付 β3"));

    // 空白内容 → 删除（safeDeleteIfExists）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/jian",
            Some(json!({ "content": "   " })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["content"], Value::Null);
    assert!(!workspace.join("jian.md").exists());

    // 已缺失再删 → ok（trashId:null 路径）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/jian",
            Some(json!({ "content": Value::Null })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));

    // 非字符串 content → 400（现役 TypeError 口径，无 code 字段）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/jian",
            Some(json!({ "content": 42 })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], json!("content.trim is not a function"));
    assert!(body.get("code").is_none(), "{body}");

    // 子目录守卫。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/jian",
            Some(json!({ "subdir": "a..b", "content": "x" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["error"], json!("invalid subdir"));

    // dirNotAllowed 文案随 locale：en / zh-TW / zh。
    let outside = outside_dir("jian-locale");
    for (locale, expected) in [
        ("en", "Directory not in allowed scope"),
        ("zh-TW", "目錄不在允許範圍內"),
        ("zh", "目录不在允许范围内"),
    ] {
        h.write_preferences(json!({"current_agent_id": "a", "locale": locale}));
        let (status, body) = h
            .http(
                reqwest::Method::POST,
                "/lingxi/v1/desk/jian",
                Some(json!({ "dir": outside.to_string_lossy(), "content": "x" })),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["error"], json!(expected), "{locale}");
    }
    h.write_preferences(json!({"current_agent_id": "a"}));

    let _ = std::fs::remove_dir_all(outside);
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_gate_and_flow() {
    let h = Harness::boot("upload").await;
    let workspace = h.workspace().to_path_buf();

    // 源 staging：文件 + 目录（含嵌套文件与符号链接）。
    let staging = outside_dir("upload-src");
    std::fs::write(staging.join("note.txt"), "note-body").unwrap();
    std::fs::create_dir_all(staging.join("bundle/nested")).unwrap();
    std::fs::write(staging.join("bundle/nested/deep.txt"), "deep").unwrap();
    std::os::unix::fs::symlink(staging.join("note.txt"), staging.join("bundle/link-note")).unwrap();

    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({
                "action": "upload",
                "paths": [
                    staging.join("note.txt").to_string_lossy(),
                    staging.join("bundle").to_string_lossy(),
                ],
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 2, "{body}");
    assert_eq!(results[0]["name"], json!("note.txt"));
    assert_eq!(results[1]["name"], json!("bundle"));
    assert_eq!(
        std::fs::read_to_string(workspace.join("note.txt")).unwrap(),
        "note-body"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("bundle/nested/deep.txt")).unwrap(),
        "deep"
    );
    // 符号链接重建为符号链接（cpSync dereference:false 口径）。
    let link_meta = std::fs::symlink_metadata(workspace.join("bundle/link-note")).unwrap();
    assert!(link_meta.file_type().is_symlink());
    // files 投影随行。
    let names: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["name"].as_str())
        .collect();
    assert!(
        names.contains(&"note.txt") && names.contains(&"bundle"),
        "{names:?}"
    );

    // 逐项失败：相对路径 / 缺失 / 敏感路径（lingxi home 之下）。
    std::fs::write(h.home.join("secret.txt"), "s").unwrap();
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({
                "action": "upload",
                "paths": [
                    "relative/path.txt",
                    staging.join("missing.txt").to_string_lossy(),
                    h.home.join("secret.txt").to_string_lossy(),
                ],
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    let results = body["results"].as_array().unwrap();
    assert_eq!(results[0]["error"], json!("invalid path"));
    assert_eq!(results[1]["error"], json!("invalid path"));
    assert_eq!(results[2]["error"], json!("sensitive path blocked"));

    // paths 缺失 → deskRouteError 400。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "upload", "paths": [] })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body["error"],
        json!({ "code": "workspace_file_validation_failed", "message": "paths required" })
    );

    // 远端（非 local owner）凭证 → 403 守卫字符串。
    let remote = h
        .state
        .auth()
        .issue_device_credential("user_remote", &["files.read", "files.write"], None)
        .unwrap()
        .secret;
    let (status, body) = h
        .http_as(
            &remote,
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({
                "action": "upload",
                "paths": [staging.join("note.txt").to_string_lossy()],
            })),
        )
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(
        body["error"],
        json!("upload by absolute path requires local owner")
    );

    let _ = std::fs::remove_dir_all(staging);
    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_and_actions_flow() {
    let h = Harness::boot("actions").await;
    let workspace = h.workspace().to_path_buf();

    // create：mustNotExist 语义。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "create", "name": "new.txt", "content": "hello" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert!(body["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"] == "new.txt"));
    assert_eq!(
        std::fs::read_to_string(workspace.join("new.txt")).unwrap(),
        "hello"
    );
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "create", "name": "new.txt", "content": "again" })),
        )
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], json!("target already exists"));
    assert_eq!(body["code"], json!("target_already_exists"));
    // 校验守卫形态：缺 name/content → deskRouteError；非平凡名 → 200。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "create", "name": "x.txt" })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body["error"],
        json!({ "code": "workspace_file_validation_failed", "message": "name and content required" })
    );
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "create", "name": "a/b", "content": "x" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["error"], json!("invalid name"));

    // mkdir + 409。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "mkdir", "name": "docs" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(workspace.join("docs").is_dir());
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "mkdir", "name": "docs" })),
        )
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"], json!("already exists"));
    assert_eq!(body["code"], json!("already_exists"));

    // rename。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "rename", "oldName": "new.txt", "newName": "renamed.txt" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(workspace.join("renamed.txt").is_file());
    assert!(!workspace.join("new.txt").exists());

    // move：names + destFolder（dest 必须已存在且是目录）。
    std::fs::write(workspace.join("m1.txt"), "1").unwrap();
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "move", "names": ["m1.txt"], "destFolder": "docs" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"][0]["ok"], json!(true));
    assert!(workspace.join("docs/m1.txt").is_file());
    // 同批成功和失败互不冒充：目标重名项的源文件与既有目标都保留。
    std::fs::write(workspace.join("mixed-ok.txt"), "move me").unwrap();
    std::fs::write(workspace.join("mixed-blocked.txt"), "keep source").unwrap();
    std::fs::write(workspace.join("docs/mixed-blocked.txt"), "keep target").unwrap();
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({
                "action":"move", "names":["mixed-ok.txt","mixed-blocked.txt"],
                "destFolder":"docs"
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"][0], json!({"name":"mixed-ok.txt","ok":true}));
    assert_eq!(body["results"][1]["name"], "mixed-blocked.txt");
    assert!(body["results"][1]["error"].is_string(), "{body}");
    assert!(!workspace.join("mixed-ok.txt").exists());
    assert_eq!(
        std::fs::read_to_string(workspace.join("docs/mixed-ok.txt")).unwrap(),
        "move me"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("mixed-blocked.txt")).unwrap(),
        "keep source"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("docs/mixed-blocked.txt")).unwrap(),
        "keep target"
    );
    // 自移入与缺失 dest 的守卫（200 字符串）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "move", "names": ["docs"], "destFolder": "docs" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["error"], json!("cannot move folder into itself"));
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "move", "names": ["renamed.txt"], "destFolder": "missing" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["error"], json!("destFolder is not a directory"));

    // movePaths：逐项 + filesByPath；dest 缺失守卫。
    std::fs::write(workspace.join("p1.txt"), "p1").unwrap();
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({
                "action": "movePaths",
                "items": [{ "sourceSubdir": "", "name": "p1.txt" }],
                "destSubdir": "docs",
                "currentSubdir": "",
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["results"][0]["ok"], json!(true));
    assert!(body["filesByPath"]["docs"].is_array(), "{body}");
    assert!(workspace.join("docs/p1.txt").is_file());
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({
                "action": "movePaths",
                "items": [{ "sourceSubdir": "", "name": "renamed.txt" }],
                "destSubdir": "not-there",
            })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["error"], json!("destSubdir is not a directory"));

    // remove：trashId + files；缺失 → 404 not found 映射。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "remove", "name": "renamed.txt" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert!(body["trashId"].is_string(), "{body}");
    assert!(!workspace.join("renamed.txt").exists());
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "remove", "name": "renamed.txt" })),
        )
        .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], json!("not found"));
    assert_eq!(body["code"], json!("resource_not_found"));

    // 未知动作：deskRouteError 400；action 缺席 → "unknown action: undefined"。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "frobnicate" })),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body["error"],
        json!({ "code": "unknown_workspace_file_action", "message": "unknown action: frobnicate" })
    );
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body["error"],
        json!({ "code": "unknown_workspace_file_action", "message": "unknown action: undefined" })
    );

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_workspace_fallbacks() {
    let h = Harness::boot_without_workspace("nows").await;

    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/desk/path", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["path"], Value::Null);

    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/desk/files", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["files"], json!([]));
    assert_eq!(body["subdir"], json!(""));
    assert_eq!(body["basePath"], Value::Null);

    let (status, body) = h
        .http_q(
            reqwest::Method::GET,
            "/lingxi/v1/desk/search-files",
            &[("q", "x")],
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["results"], json!([]));
    assert_eq!(body["basePath"], Value::Null);
    assert_eq!(body["query"], json!("x"));

    let (status, body) = h
        .http(reqwest::Method::GET, "/lingxi/v1/desk/jian", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["content"], Value::Null);

    // POST 面：noWorkspace 守卫字符串（zh 默认）。
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/jian",
            Some(json!({ "content": "x" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["error"], json!("未设置工作台"));
    let (status, body) = h
        .http(
            reqwest::Method::POST,
            "/lingxi/v1/desk/files",
            Some(json!({ "action": "mkdir", "name": "x" })),
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["error"], json!("未设置工作台"));

    h.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scope_gates() {
    let h = Harness::boot("scope").await;

    // 无凭证 → 401。
    let response = reqwest::Client::new()
        .get(format!("{}/lingxi/v1/desk/files", h.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 401);

    // 无 scope 凭证：读面 403（中间件统一信封，与 workbench 面同口径）。
    let denied = h
        .state
        .auth()
        .issue_device_credential("user_local", &[], None)
        .unwrap()
        .secret;
    let (status, body) = h
        .http_as(&denied, reqwest::Method::GET, "/lingxi/v1/desk/files", None)
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["details"]["reason"], json!("insufficient_scope"));
    assert_eq!(body["details"]["requiredScope"], json!("files.read"));

    // files.read：GET 放行，POST（files.write）403。
    let reader = h
        .state
        .auth()
        .issue_device_credential("user_local", &["files.read"], None)
        .unwrap()
        .secret;
    let (status, _) = h
        .http_as(&reader, reqwest::Method::GET, "/lingxi/v1/desk/path", None)
        .await;
    assert_eq!(status, 200);
    let (status, body) = h
        .http_as(
            &reader,
            reqwest::Method::POST,
            "/lingxi/v1/desk/jian",
            Some(json!({ "content": "x" })),
        )
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["details"]["requiredScope"], json!("files.write"));

    h.shutdown().await;
}
