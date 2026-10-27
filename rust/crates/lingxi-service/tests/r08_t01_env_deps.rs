//! R08-T01：现役 GET /api/system/env-deps 与 POST /api/system/env-deps/refresh
//! 忠实移植验收——真实服务、真实凭证、真实进程探测（受管目录桩）、真实系统
//! 时钟下的缓存生命周期。全程无 mock / 无 skip。
//!
//! 单测试函数设计：进程内缓存是全局 Mutex<单槽>，多个 #[tokio::test] 并行
//! 会互相驱逐缓存键导致 checkedAt 断言抖动；相位全部串在一个函数内执行。
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 现役注册表 13 项的既定顺序（server/lib/env-deps/registry.ts）。
const REGISTRY_IDS: [&str; 13] = [
    "git",
    "python3",
    "node",
    "ripgrep",
    "fd",
    "ast_grep",
    "semgrep",
    "gitleaks",
    "typescript",
    "pyright",
    "gopls",
    "rust_analyzer",
    "clangd",
];

struct Server {
    home: PathBuf,
    state: ServiceState,
    addr: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}

impl Server {
    async fn boot(workspace_root: Option<PathBuf>) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-env-deps-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let home = home.canonicalize().unwrap();
        std::fs::create_dir_all(home.join("user")).unwrap();
        let layout = prepare_layout(&home).unwrap();
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 5000,
            },
            &layout,
            ServiceDeps {
                workspace_root,
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let (ready, addr) = tokio::sync::oneshot::channel();
        let running = state.clone();
        let task = tokio::spawn(async move {
            run(
                running,
                async {
                    let _ = rx.await;
                },
                |a| {
                    let _ = ready.send(a);
                },
                None,
            )
            .await
        });
        Self {
            home,
            state,
            addr: addr.await.unwrap().to_string(),
            token,
            stop,
            task,
        }
    }

    /// token=None 时完全不带 Authorization 头（401 门用例）。
    async fn raw(&self, method: &str, path: &str, token: Option<&str>) -> (u16, Value) {
        let mut stream = tokio::net::TcpStream::connect(&self.addr).await.unwrap();
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let wire = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(wire.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        // 探测为顺序执行且单项上限 3s；留足极端慢机的余量但仍真实限时。
        tokio::time::timeout(Duration::from_secs(60), stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let end = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
        let status = String::from_utf8_lossy(&bytes[..end])
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        (status, serde_json::from_slice(&bytes[end + 4..]).unwrap())
    }

    async fn close(self) {
        self.stop.send(()).unwrap();
        let outcome = self.task.await.unwrap().unwrap();
        assert!(!outcome.drain_timed_out, "服务退出未完成真实连接排空");
    }
}

fn dep<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["deps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == id)
        .unwrap_or_else(|| panic!("报告缺依赖项 {id}: {report}"))
}

/// 相位 5（仅 unix）：受管目录 `{data_home}/runtime/pi-sdk/bin` 桩探测。
/// - rg 平铺桩：输出 "ripgrep 14.1.1 (rev …)" → installed + managed:true +
///   version "14.1.1"（extractVersion 首行首个 X.Y[.Z]）。
/// - fd 一层子目录桩且退出码非零：现役受管命中**忽略 probe ok**（detect.ts
///   :141-145）→ 仍 installed + managed:true 且无 version 键。
/// 非 unix 平台无 X_OK 语义与 shebang 执行保证，由 cfg 空实现显式不承担该相位。
#[cfg(unix)]
async fn phase_managed_dir(s: &Server) {
    use std::os::unix::fs::PermissionsExt;
    let bin = s.home.join("runtime/pi-sdk/bin");
    std::fs::create_dir_all(bin.join("10.2.0")).unwrap();
    let rg = bin.join("rg");
    std::fs::write(&rg, "#!/bin/sh\necho 'ripgrep 14.1.1 (rev 9cc0ad12fc)'\n").unwrap();
    std::fs::set_permissions(&rg, std::fs::Permissions::from_mode(0o755)).unwrap();
    let fd = bin.join("10.2.0/fd");
    std::fs::write(&fd, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&fd, std::fs::Permissions::from_mode(0o755)).unwrap();

    let (status, report) = s
        .raw(
            "GET",
            "/lingxi/v1/system/env-deps?agentId=ghost&refresh=1",
            Some(&s.token),
        )
        .await;
    assert_eq!(status, 200, "{report}");
    let rg_dep = dep(&report, "ripgrep");
    assert_eq!(rg_dep["status"], "installed", "{rg_dep}");
    assert_eq!(rg_dep["managed"], true, "{rg_dep}");
    assert_eq!(rg_dep["version"], "14.1.1", "{rg_dep}");
    assert_eq!(
        rg_dep["path"].as_str().unwrap(),
        rg.to_string_lossy().as_ref(),
        "受管路径应为平铺桩绝对路径"
    );
    let fd_dep = dep(&report, "fd");
    assert_eq!(fd_dep["status"], "installed", "{fd_dep}");
    assert_eq!(fd_dep["managed"], true, "{fd_dep}");
    assert!(
        fd_dep.get("version").is_none(),
        "非零退出不应带版本键（ok 被忽略）：{fd_dep}"
    );
    assert!(
        fd_dep["path"].as_str().unwrap().ends_with("10.2.0/fd"),
        "一层子目录桩应被扫到：{fd_dep}"
    );
}

#[cfg(not(unix))]
async fn phase_managed_dir(_s: &Server) {}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn env_deps_auth_contract_signals_managed_and_cache_lifecycle() {
    // ---- 相位 0：造工作区信号夹具 ----------------------------------------
    // go.mod（gopls 命中）；.github/workflows/x.py（python3 命中——现役对
    // .github 的跳过例外）；node_modules/pkg/package.json 与 .git/package.json
    // （node 信号只出现在必被跳过的目录里 → 必须为 false）。
    let suffix = lingxi_adapters::storage::agent_memory::random_token_suffix();
    let ws = std::env::temp_dir().join(format!("lingxi-env-deps-ws-{suffix}"));
    std::fs::create_dir_all(ws.join("node_modules/pkg")).unwrap();
    std::fs::create_dir_all(ws.join(".github/workflows")).unwrap();
    std::fs::create_dir_all(ws.join(".git")).unwrap();
    std::fs::write(ws.join("go.mod"), "module example.com/envdeps\n").unwrap();
    std::fs::write(ws.join(".github/workflows/x.py"), "print(1)\n").unwrap();
    std::fs::write(ws.join("node_modules/pkg/package.json"), "{}\n").unwrap();
    std::fs::write(ws.join(".git/package.json"), "{}\n").unwrap();
    let ws = ws.canonicalize().unwrap();

    let s = Server::boot(Some(ws.clone())).await;

    // ---- 相位 1：授权门（LocalOnly：仅本机 token 可达） -------------------
    assert_eq!(
        s.raw("GET", "/lingxi/v1/system/env-deps", None).await.0,
        401
    );
    assert_eq!(
        s.raw("POST", "/lingxi/v1/system/env-deps/refresh", None)
            .await
            .0,
        401
    );
    let scoped = s
        .state
        .auth()
        .issue_device_credential(
            lingxi_service::LOCAL_OWNER_USER_ID,
            &["settings.read"],
            None,
        )
        .unwrap();
    assert_eq!(
        s.raw("GET", "/lingxi/v1/system/env-deps", Some(&scoped.secret))
            .await
            .0,
        403
    );
    assert_eq!(
        s.raw(
            "POST",
            "/lingxi/v1/system/env-deps/refresh",
            Some(&scoped.secret)
        )
        .await
        .0,
        403
    );

    // ---- 相位 2：报告契约 + 零助手（无 agentId → 无根 → 项目感知关闭）-----
    let (status, report) = s
        .raw("GET", "/lingxi/v1/system/env-deps", Some(&s.token))
        .await;
    assert_eq!(status, 200, "{report}");
    assert!(
        report["checkedAt"].as_str().unwrap().len() >= 20,
        "{report}"
    );
    assert!(report["durationMs"].as_u64().is_some(), "{report}");
    let deps = report["deps"].as_array().unwrap();
    assert_eq!(deps.len(), 13, "{report}");
    let got_ids: Vec<&str> = deps.iter().map(|d| d["id"].as_str().unwrap()).collect();
    assert_eq!(got_ids, REGISTRY_IDS, "注册表顺序须逐项一致");
    assert_eq!(report["summary"]["total"], 13, "{report}");
    let installed = report["summary"]["installed"].as_u64().unwrap();
    let missing = report["summary"]["missing"].as_u64().unwrap();
    assert_eq!(installed + missing, 13, "{report}");
    assert_eq!(
        report["summary"]["projectMissing"],
        serde_json::json!([]),
        "无根 → 无项目缺失：{report}"
    );
    for d in deps {
        let st = d["status"].as_str().unwrap();
        assert!(st == "installed" || st == "missing", "非法状态：{d}");
        assert_eq!(d["neededByProject"], false, "无根时项目感知必须关闭：{d}");
        if st == "missing" {
            assert!(d.get("version").is_none(), "missing 不得带 version：{d}");
            assert!(d.get("path").is_none(), "missing 不得带 path：{d}");
            assert!(d.get("managed").is_none(), "missing 不得带 managed：{d}");
        } else {
            assert!(
                d["managed"].is_boolean(),
                "installed 必须带布尔 managed：{d}"
            );
        }
    }

    // ---- 相位 3：未知 agentId → 200 不报错 ------------------------------
    // 现役 getHomeCwd(未知 id) 回落默认工作区（候选 = 服务 workspace_root），
    // 因此根 = [ws]，信号应生效。
    let (status, ghost) = s
        .raw(
            "GET",
            "/lingxi/v1/system/env-deps?agentId=ghost",
            Some(&s.token),
        )
        .await;
    assert_eq!(status, 200, "{ghost}");
    assert_eq!(
        dep(&ghost, "gopls")["neededByProject"],
        true,
        "go.mod 应命中"
    );
    assert_eq!(
        dep(&ghost, "python3")["neededByProject"],
        true,
        ".github 是跳过规则的现役例外"
    );
    assert_eq!(
        dep(&ghost, "node")["neededByProject"],
        false,
        "node_modules/.git 内的 package.json 必须被跳过"
    );
    assert_eq!(dep(&ghost, "rust_analyzer")["neededByProject"], false);
    assert_eq!(dep(&ghost, "typescript")["neededByProject"], false);

    // ---- 相位 4：默认第一助手根解析 --------------------------------------
    // ws2 只含 Cargo.toml；助手 alpha 的 desk.home_folder 指向 ws2。无 agentId
    // 时现役取 listAgents[0]（候选 = ordered_live_agents 首个）。
    let ws2 = std::env::temp_dir().join(format!("lingxi-env-deps-ws2-{suffix}"));
    std::fs::create_dir_all(&ws2).unwrap();
    std::fs::write(ws2.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let ws2 = ws2.canonicalize().unwrap();
    let agent_dir = s.home.join("agents/alpha");
    std::fs::create_dir_all(&agent_dir).unwrap();
    std::fs::write(
        agent_dir.join("config.yaml"),
        format!(
            "agent:\n  name: Alpha\ndesk:\n  home_folder: {}\n",
            ws2.display()
        ),
    )
    .unwrap();
    let (status, defaulted) = s
        .raw("GET", "/lingxi/v1/system/env-deps", Some(&s.token))
        .await;
    assert_eq!(status, 200, "{defaulted}");
    assert_eq!(
        dep(&defaulted, "rust_analyzer")["neededByProject"],
        true,
        "默认第一助手的 Cargo.toml 根应命中"
    );
    assert_eq!(
        dep(&defaulted, "gopls")["neededByProject"],
        false,
        "根已切到 ws2（无 go.mod），证明默认助手解析真实生效"
    );
    // 显式 agentId=alpha 与默认解析同根 → 缓存命中，checkedAt 相同。
    let (_, explicit) = s
        .raw(
            "GET",
            "/lingxi/v1/system/env-deps?agentId=alpha",
            Some(&s.token),
        )
        .await;
    assert_eq!(
        explicit["checkedAt"], defaulted["checkedAt"],
        "同根应命中同一缓存槽"
    );

    // ---- 相位 5：受管目录探测（unix；其他平台 cfg 空相位） ----------------
    phase_managed_dir(&s).await;

    // ---- 相位 6：缓存生命周期（真实系统时钟，毫秒分辨率） -----------------
    // 锚点：refresh=1 强制重算并写缓存（现役 force 也写槽，detect.ts :215-217）。
    let (status, anchor) = s
        .raw(
            "GET",
            "/lingxi/v1/system/env-deps?agentId=ghost&refresh=1",
            Some(&s.token),
        )
        .await;
    assert_eq!(status, 200, "{anchor}");
    let (_, hit) = s
        .raw(
            "GET",
            "/lingxi/v1/system/env-deps?agentId=ghost",
            Some(&s.token),
        )
        .await;
    assert_eq!(hit["checkedAt"], anchor["checkedAt"], "同根同键应命中缓存");
    let (_, hit2) = s
        .raw(
            "GET",
            "/lingxi/v1/system/env-deps?agentId=ghost",
            Some(&s.token),
        )
        .await;
    assert_eq!(hit2["checkedAt"], anchor["checkedAt"], "重复 GET 仍命中");
    // refresh=1 → 新 checkedAt，并覆写缓存槽。
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (_, refreshed) = s
        .raw(
            "GET",
            "/lingxi/v1/system/env-deps?agentId=ghost&refresh=1",
            Some(&s.token),
        )
        .await;
    assert_ne!(
        refreshed["checkedAt"], anchor["checkedAt"],
        "refresh=1 必须真实重算（checkedAt 变化）"
    );
    let (_, hit3) = s
        .raw(
            "GET",
            "/lingxi/v1/system/env-deps?agentId=ghost",
            Some(&s.token),
        )
        .await;
    assert_eq!(
        hit3["checkedAt"], refreshed["checkedAt"],
        "refresh=1 之后 GET 应命中新槽（force 也写缓存）"
    );
    // POST /refresh 恒强制刷新 → 新 checkedAt，同样写缓存。
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (status, posted) = s
        .raw(
            "POST",
            "/lingxi/v1/system/env-deps/refresh?agentId=ghost",
            Some(&s.token),
        )
        .await;
    assert_eq!(status, 200, "{posted}");
    assert_ne!(
        posted["checkedAt"], refreshed["checkedAt"],
        "POST /refresh 必须真实重算"
    );
    let (_, hit4) = s
        .raw(
            "GET",
            "/lingxi/v1/system/env-deps?agentId=ghost",
            Some(&s.token),
        )
        .await;
    assert_eq!(
        hit4["checkedAt"], posted["checkedAt"],
        "POST /refresh 之后 GET 应命中新槽"
    );

    s.close().await;
}
