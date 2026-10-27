//! R08-T01 环境依赖探测面（现役 server/routes/env-deps.ts +
//! lib/env-deps/detect.ts + lib/env-deps/registry.ts 的端口）。
//!
//! GET  /lingxi/v1/system/env-deps          读缓存报告（?refresh=1 强制重探）
//! POST /lingxi/v1/system/env-deps/refresh  强制重探并返回新报告
//!
//! 语义逐项对齐现役：
//! - 登记表 13 项逐字移植（registry.ts :36-198）；python3 候选命令按平台
//!   win32 → ["python","python3"]，其余 → ["python3","python"]。
//! - 探测：execFile(bin, versionArgs, 3s 超时) → spawn + try_wait 轮询 + kill
//!   本地实现（std::process::Command 无内建超时，仓内无可复用 helper）。
//!   note 口径经现役实证：spawn 失败（ENOENT/EACCES）与非零退出 → 内部
//!   not_found；超时 → probe_timeout；spawn_failed 现役仅参数类型同步 throw
//!   可达（静态注册表不可达）。note 在现役 dep 对象构造时被丢弃
//!   （detect.ts :207/:216/:219 均不携带），候选同样不出现在 JSON 中。
//! - 版本提取：首行 `(\d+\.\d+(?:\.\d+)?(?:[-\w.]*)?)`，无匹配回落首行
//!   trim 前 60 字符（手写等价扫描，不引 regex）。
//! - managed 类：先查托管目录（一层子目录浅扫、X_OK），命中则探测结果仅取
//!   version、ok 被忽略、恒 installed + managed:true（detect.ts :203-208
//!   原样）；未命中回落 PATH 首个成功候选，managed:false；全失败 missing
//!   （无 managed 键）。
//! - 项目信号：深度 ≤2、visited 封顶 2000（每个条目含被跳过的先自增再判）、
//!   跳过 node_modules/.git/点开头（.github 除外，文件与目录同口径）、
//!   精确文件名或 *.ext 后缀；symlink 既不是文件也不是目录（Dirent 同口径）。
//! - 缓存：进程内单例 Mutex，key = roots.join("|")，TTL 60s；force 只旁路
//!   读取，命中写回无条件发生（detect.ts :182/:234）；时间戳用真实系统时钟
//!   （现役 Date.now 同口径；注入的 ManualClock 在测试中恒定，不能用于
//!   checkedAt）。
//! - 并发：现役 Promise.all 并发探测 → 候选顺序探测（lead 批准的登记决策；
//!   仅延迟差异，13×≤3s 有界，且避免测试机 PATH 探测风暴）。整块在
//!   spawn_blocking 中执行，不占用 async worker。
//! - 工作区根：?agentId= 指定 → 该助手 explicit + home（复用
//!   desk_files::selected_agent_desk_roots，与现役 resolveWorkspaceRoots
//!   的 getExplicitHomeCwd + getHomeCwd 同形）；缺省取 agent_order 排序后
//!   第一个存活助手（engine.listAgents 排序口径）；任何一步失败 → 空根
//!   （现役 try/catch 降级为无项目感知，路由从不因此 5xx）。
//! - 整体失败才 500 {"error":"env_deps_detection_failed","message":...}：
//!   候选实现里探测全程收敛（panic 经 spawn_blocking JoinError 落入该形状）。
//!
//! 登记的显式偏差（lead 裁定）：
//! 1. 托管目录根：现役 detect.ts :100 零参 resolveLingxiHome() 恒为
//!    `~/.lingxi`（os.homedir，忽略 LINGXI_HOME 配置，与 server/index.ts
//!    :252 的进程 home 可能分叉）。候选用服务实际 data_home
//!    （{data_home}/runtime/pi-sdk/bin）——与服务真实运行拓扑一致且保住
//!    测试隔离；默认部署下两者重合。
//! 2. findBundledBin（lib/bundled-bins.ts）依赖 Electron 打包 resources
//!    路径，独立 Rust 服务内恒为 None（拓扑裁定，非静默降级）；只保留
//!    托管目录 + PATH 两级。
//! 3. 现役 missing 分支的惰性配置清洗副作用由 GC 路径承担
//!    （desk_files.rs :151-155 既有登记），本读路径不改写配置。

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::ServiceState;

// ── 常量（detect.ts :24-28）─────────────────────────────────────────────

const PROBE_TIMEOUT: Duration = Duration::from_millis(3000);
const SCAN_MAX_DEPTH: u32 = 2;
const SCAN_MAX_ENTRIES: u32 = 2000;
const CACHE_TTL_MS: u64 = 60_000;

// ── 登记表（registry.ts :36-198 逐字移植）───────────────────────────────

struct InstallHint {
    darwin: Option<&'static str>,
    win32: Option<&'static str>,
    linux: Option<&'static str>,
    other: Option<&'static str>,
}

struct EnvDepEntry {
    id: &'static str,
    label: &'static str,
    kind: &'static str,
    binaries: &'static [&'static str],
    version_args: &'static [&'static str],
    managed_bin_name: Option<&'static str>,
    required_by: &'static [&'static str],
    project_signals: &'static [&'static str],
    install_hint: Option<InstallHint>,
}

const fn hint(
    darwin: Option<&'static str>,
    win32: Option<&'static str>,
    linux: Option<&'static str>,
    other: Option<&'static str>,
) -> Option<InstallHint> {
    Some(InstallHint {
        darwin,
        win32,
        linux,
        other,
    })
}

const VERSION: &[&str] = &["--version"];

static REGISTRY: &[EnvDepEntry] = &[
    EnvDepEntry {
        id: "git",
        label: "Git",
        kind: "binary",
        binaries: &["git"],
        version_args: VERSION,
        managed_bin_name: None,
        required_by: &["workspace_snapshots", "file_history"],
        project_signals: &[],
        install_hint: hint(
            Some("xcode-select --install"),
            Some("winget install Git.Git"),
            Some("sudo apt install git"),
            None,
        ),
    },
    EnvDepEntry {
        id: "python3",
        label: "Python 3",
        kind: "runtime",
        // registry.ts :54 —— win32 → ["python","python3"]，其余反转。
        binaries: if cfg!(windows) {
            &["python", "python3"]
        } else {
            &["python3", "python"]
        },
        version_args: VERSION,
        managed_bin_name: None,
        required_by: &["run_code"],
        project_signals: &["*.py", "requirements.txt", "pyproject.toml", "Pipfile"],
        install_hint: hint(
            Some("brew install python@3"),
            Some("winget install Python.Python.3.12"),
            Some("sudo apt install python3"),
            None,
        ),
    },
    EnvDepEntry {
        id: "node",
        label: "Node.js",
        kind: "runtime",
        binaries: &["node"],
        version_args: VERSION,
        managed_bin_name: None,
        required_by: &["run_code"],
        project_signals: &["package.json"],
        install_hint: hint(
            Some("brew install node"),
            Some("winget install OpenJS.NodeJS.LTS"),
            Some("sudo apt install nodejs"),
            None,
        ),
    },
    EnvDepEntry {
        id: "ripgrep",
        label: "ripgrep",
        kind: "managed",
        binaries: &["rg"],
        version_args: VERSION,
        managed_bin_name: Some("rg"),
        required_by: &["grep_tool"],
        project_signals: &[],
        install_hint: None,
    },
    EnvDepEntry {
        id: "fd",
        label: "fd",
        kind: "managed",
        binaries: &["fd"],
        version_args: VERSION,
        managed_bin_name: Some("fd"),
        required_by: &["find_tool"],
        project_signals: &[],
        install_hint: None,
    },
    EnvDepEntry {
        id: "ast_grep",
        label: "ast-grep",
        kind: "managed",
        binaries: &["sg", "ast-grep"],
        version_args: VERSION,
        managed_bin_name: Some("ast-grep"),
        required_by: &["ast_tools"],
        project_signals: &[],
        install_hint: hint(
            Some("brew install ast-grep"),
            Some("winget install ast-grep.ast-grep"),
            Some("cargo install ast-grep"),
            None,
        ),
    },
    EnvDepEntry {
        id: "semgrep",
        label: "Semgrep",
        kind: "binary",
        binaries: &["semgrep"],
        version_args: VERSION,
        managed_bin_name: None,
        required_by: &["security_scan"],
        project_signals: &[],
        install_hint: hint(
            Some("brew install semgrep"),
            Some("pip install semgrep"),
            Some("pip install semgrep"),
            None,
        ),
    },
    EnvDepEntry {
        id: "gitleaks",
        label: "Gitleaks",
        kind: "binary",
        binaries: &["gitleaks"],
        version_args: &["version"],
        managed_bin_name: None,
        required_by: &["security_scan"],
        project_signals: &[],
        install_hint: hint(
            Some("brew install gitleaks"),
            Some("winget install gitleaks"),
            Some("sudo apt install gitleaks"),
            None,
        ),
    },
    EnvDepEntry {
        id: "typescript",
        label: "TypeScript (tsc)",
        kind: "binary",
        binaries: &["tsc"],
        version_args: VERSION,
        managed_bin_name: None,
        required_by: &["lsp"],
        project_signals: &["tsconfig.json", "*.ts", "*.tsx"],
        install_hint: hint(None, None, None, Some("npm install -g typescript")),
    },
    EnvDepEntry {
        id: "pyright",
        label: "Pyright",
        kind: "binary",
        binaries: &["pyright"],
        version_args: VERSION,
        managed_bin_name: None,
        required_by: &["lsp"],
        project_signals: &["*.py", "pyproject.toml"],
        install_hint: hint(None, None, None, Some("npm install -g pyright")),
    },
    EnvDepEntry {
        id: "gopls",
        label: "gopls",
        kind: "binary",
        binaries: &["gopls"],
        version_args: &["version"],
        managed_bin_name: None,
        required_by: &["lsp"],
        project_signals: &["go.mod"],
        install_hint: hint(
            None,
            None,
            None,
            Some("go install golang.org/x/tools/gopls@latest"),
        ),
    },
    EnvDepEntry {
        id: "rust_analyzer",
        label: "rust-analyzer",
        kind: "binary",
        binaries: &["rust-analyzer"],
        version_args: VERSION,
        managed_bin_name: None,
        required_by: &["lsp"],
        project_signals: &["Cargo.toml"],
        install_hint: hint(None, None, None, Some("rustup component add rust-analyzer")),
    },
    EnvDepEntry {
        id: "clangd",
        label: "clangd",
        kind: "binary",
        binaries: &["clangd"],
        version_args: VERSION,
        managed_bin_name: None,
        required_by: &["lsp"],
        project_signals: &["*.c", "*.cpp", "*.cc", "compile_commands.json"],
        install_hint: hint(
            Some("brew install llvm"),
            Some("winget install LLVM.LLVM"),
            Some("sudo apt install clangd"),
            None,
        ),
    },
];

// ── 探测（detect.ts :31-95）─────────────────────────────────────────────

struct ProbeOut {
    ok: bool,
    version: Option<String>,
}

/// extractVersion：首行 semver 形正则，无匹配回落 trim 前 60 字符。
/// 手写等价于 /(\d+\.\d+(?:\.\d+)?(?:[-\w.]*)?)/（含逐位重试语义）。
fn extract_version(output: &str) -> Option<String> {
    let first_line = output.split('\n').next().unwrap_or("");
    let first_line = first_line.strip_suffix('\r').unwrap_or(first_line);
    let bytes = first_line.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i].is_ascii_digit() {
            let start = i;
            let mut j = i;
            while j < len && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j + 1 < len && bytes[j] == b'.' && bytes[j + 1].is_ascii_digit() {
                j += 1;
                while j < len && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j + 1 < len && bytes[j] == b'.' && bytes[j + 1].is_ascii_digit() {
                    j += 1;
                    while j < len && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                }
                // (?:[-\w.]*)? —— \w = [A-Za-z0-9_]
                while j < len
                    && (bytes[j] == b'-'
                        || bytes[j] == b'.'
                        || bytes[j] == b'_'
                        || bytes[j].is_ascii_alphanumeric())
                {
                    j += 1;
                }
                return Some(first_line[start..j].to_string());
            }
            i = start + 1;
        } else {
            i += 1;
        }
    }
    let trimmed = first_line.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(60).collect())
    }
}

/// 单次执行 + 3s 超时上限的内部原语：spawn 失败/等待错误/超时 → None；
/// 否则返回 (退出是否成功, stdout, stderr)。stdout/stderr 由读线程排空，
/// 避免管道写满挂住子进程。
fn run_capture(program: &OsStr, args: &[&str]) -> Option<(bool, Vec<u8>, Vec<u8>)> {
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take().expect("stdout 管道已取走");
    let mut stderr = child.stderr.take().expect("stderr 管道已取走");
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    status.map(|status| (status.success(), out, err))
}

/// defaultProbe：spawn 失败/非零退出/等待错误 → not ok（现役实证同收敛为
/// not_found）；超时 → 杀死后 not ok（现役 probe_timeout）。
fn probe(program: &OsStr, args: &[&str]) -> ProbeOut {
    let Some((success, out, err)) = run_capture(program, args) else {
        return ProbeOut {
            ok: false,
            version: None,
        };
    };
    if !success {
        return ProbeOut {
            ok: false,
            version: None,
        };
    }
    let mut combined = String::from_utf8_lossy(&out).into_owned();
    combined.push('\n');
    combined.push_str(&String::from_utf8_lossy(&err));
    ProbeOut {
        ok: true,
        version: extract_version(&combined),
    }
}

/// probeBinPath：`which bin`（win32 `where`），stdout 首行 trim，任何失败
/// （含非零退出/超时）→ None（detect.ts :61-66 `err → undefined`）。
fn probe_bin_path(bin: &str) -> Option<String> {
    let tool = if cfg!(windows) { "where" } else { "which" };
    let (success, out, _) = run_capture(OsStr::new(tool), &[bin])?;
    if !success {
        return None;
    }
    let text = String::from_utf8_lossy(&out);
    let first = text
        .split('\n')
        .next()
        .unwrap_or("")
        .trim_end_matches('\r')
        .trim();
    if first.is_empty() {
        None
    } else {
        Some(first.to_string())
    }
}

// ── 托管目录（detect.ts :96-126）────────────────────────────────────────

/// X_OK 同形：unix 任一执行位；其他平台以存在性近似（fs.accessSync X_OK
/// 在 win32 上本就退化为存在性检查）。
fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = &meta;
        true
    }
}

/// 托管二进制查找：平铺候选 → 一层子目录浅扫（目录项序按 readdir 原序，
/// 与现役一致不做排序）。
fn managed_bin_path(data_home: &Path, bin_name: &str) -> Option<PathBuf> {
    let dir = data_home.join("runtime").join("pi-sdk").join("bin");
    let candidates: [String; 2] = if cfg!(windows) {
        [format!("{bin_name}.exe"), bin_name.to_string()]
    } else {
        [bin_name.to_string(), format!("{bin_name}.exe")]
    };
    for name in &candidates {
        let path = dir.join(name);
        if is_executable(&path) {
            return Some(path);
        }
    }
    let entries = std::fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        for name in &candidates {
            let path = entry.path().join(name);
            if is_executable(&path) {
                return Some(path);
            }
        }
    }
    None
}

// ── 项目信号扫描（detect.ts :128-155）───────────────────────────────────

fn signal_matches(file_name: &str, signal: &str) -> bool {
    if let Some(ext) = signal.strip_prefix('*') {
        return file_name.ends_with(ext);
    }
    file_name == signal
}

fn walk_signals(dir: &Path, depth: u32, signals: &[&str], visited: &mut u32) -> bool {
    if depth > SCAN_MAX_DEPTH || *visited >= SCAN_MAX_ENTRIES {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries {
        // 现役 `visited++ >= CAP`：含被跳过条目先自增再判。
        let current = *visited;
        *visited += 1;
        if current >= SCAN_MAX_ENTRIES {
            return false;
        }
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "node_modules" || name == ".git" || (name.starts_with('.') && name != ".github")
        {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // Dirent 同口径：symlink 既不是 isFile 也不是 isDirectory → 跳过。
        if file_type.is_file() && signals.iter().any(|s| signal_matches(&name, s)) {
            return true;
        }
        if file_type.is_dir() && walk_signals(&entry.path(), depth + 1, signals, visited) {
            return true;
        }
    }
    false
}

fn scan_project_signals(roots: &[PathBuf], signals: &[&str]) -> bool {
    if signals.is_empty() {
        return false;
    }
    let mut visited = 0u32;
    roots
        .iter()
        .any(|root| walk_signals(root, 0, signals, &mut visited))
}

// ── 安装指引（detect.ts :157-161）───────────────────────────────────────

fn pick_install_hint(entry: &EnvDepEntry) -> Option<&'static str> {
    let hint = entry.install_hint.as_ref()?;
    // process.platform → darwin/win32/linux；其余平台键不存在 → 落 other。
    let platform = if cfg!(target_os = "macos") {
        hint.darwin
    } else if cfg!(windows) {
        hint.win32
    } else if cfg!(target_os = "linux") {
        hint.linux
    } else {
        None
    };
    platform.or(hint.other)
}

// ── 缓存（detect.ts :163, :182-184, :234）───────────────────────────────

struct CacheSlot {
    key: String,
    at_ms: u64,
    report: Value,
}

static CACHE: Mutex<Option<CacheSlot>> = Mutex::new(None);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn cache_lock() -> std::sync::MutexGuard<'static, Option<CacheSlot>> {
    CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ── 报告构建（detect.ts :179-243）───────────────────────────────────────

fn probe_entry(entry: &EnvDepEntry, data_home: &Path, roots: &[PathBuf]) -> Value {
    let needed_by_project = scan_project_signals(roots, entry.project_signals);
    let mut dep = serde_json::Map::new();
    dep.insert("id".into(), json!(entry.id));
    dep.insert("label".into(), json!(entry.label));
    dep.insert("kind".into(), json!(entry.kind));
    dep.insert("requiredBy".into(), json!(entry.required_by));
    dep.insert("neededByProject".into(), json!(needed_by_project));
    if let Some(hint) = pick_install_hint(entry) {
        dep.insert("installHint".into(), json!(hint));
    }

    // managed 类：内置（Electron 拓扑，候选恒 None）→ 托管目录；命中即
    // installed + managed:true，探测 ok 被忽略、version 可为空。
    if entry.kind == "managed" {
        if let Some(bin_name) = entry.managed_bin_name {
            if let Some(managed_path) = managed_bin_path(data_home, bin_name) {
                let found = probe(managed_path.as_os_str(), entry.version_args);
                dep.insert("status".into(), json!("installed"));
                dep.insert("managed".into(), json!(true));
                dep.insert("path".into(), json!(managed_path.to_string_lossy()));
                if let Some(version) = found.version {
                    dep.insert("version".into(), json!(version));
                }
                return Value::Object(dep);
            }
        }
    }

    // PATH 探测：候选命令按顺序，第一个成功即收。
    for bin in entry.binaries {
        let found = probe(OsStr::new(bin), entry.version_args);
        if found.ok {
            dep.insert("status".into(), json!("installed"));
            dep.insert("managed".into(), json!(false));
            if let Some(version) = found.version {
                dep.insert("version".into(), json!(version));
            }
            if let Some(path) = probe_bin_path(bin) {
                dep.insert("path".into(), json!(path));
            }
            return Value::Object(dep);
        }
    }

    dep.insert("status".into(), json!("missing"));
    Value::Object(dep)
}

fn detect_report(data_home: &Path, roots: &[PathBuf], force: bool) -> Value {
    let cache_key = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("|");
    if !force {
        let guard = cache_lock();
        if let Some(slot) = guard.as_ref() {
            if slot.key == cache_key && now_ms().saturating_sub(slot.at_ms) < CACHE_TTL_MS {
                return slot.report.clone();
            }
        }
    }
    let started = Instant::now();
    let deps: Vec<Value> = REGISTRY
        .iter()
        .map(|entry| probe_entry(entry, data_home, roots))
        .collect();
    let installed = deps
        .iter()
        .filter(|dep| dep["status"] == "installed")
        .count();
    let missing: Vec<&Value> = deps
        .iter()
        .filter(|dep| dep["status"] != "installed")
        .collect();
    let project_missing: Vec<Value> = missing
        .iter()
        .filter(|dep| dep["neededByProject"] == true)
        .filter_map(|dep| dep["id"].as_str().map(|id| json!(id)))
        .collect();
    let report = json!({
        "checkedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "durationMs": started.elapsed().as_millis() as u64,
        "deps": deps,
        "summary": {
            "total": REGISTRY.len(),
            "installed": installed,
            "missing": missing.len(),
            "projectMissing": project_missing,
        },
    });
    let mut guard = cache_lock();
    *guard = Some(CacheSlot {
        key: cache_key,
        at_ms: now_ms(),
        report: report.clone(),
    });
    report
}

// ── 工作区根（routes/env-deps.ts :14-26）────────────────────────────────

fn resolve_workspace_roots(state: &ServiceState, agent_id: Option<&str>) -> Vec<PathBuf> {
    // 现役 try/catch 全包：任何一步失败 → 空根（无项目感知），路由不 5xx。
    let id: Option<String> = match agent_id {
        Some(id) => Some(id.to_string()),
        None => state
            .memory()
            .ordered_live_agents()
            .ok()
            .and_then(|agents| agents.into_iter().next())
            .map(|(id, _)| id),
    };
    let Some(id) = id else {
        return Vec::new();
    };
    // 现役 Set 去重保序（explicit 在前、home 在后）。
    let mut roots: Vec<PathBuf> = Vec::new();
    for root in crate::desk_files::selected_agent_desk_roots(state, &id) {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

// ── 查询参数（Hono c.req.query 同口径：首值优先、宽松解码）───────────────

fn form_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| -> Option<u8> {
                    match b {
                        b'0'..=b'9' => Some(b - b'0'),
                        b'a'..=b'f' => Some(b - b'a' + 10),
                        b'A'..=b'F' => Some(b - b'A' + 10),
                        _ => None,
                    }
                };
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi << 4) | lo);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_param(raw_query: Option<&str>, key: &str) -> Option<String> {
    for pair in raw_query?.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if form_decode(name) == key {
            return Some(form_decode(value));
        }
    }
    None
}

// ── 路由 ────────────────────────────────────────────────────────────────

async fn respond(state: ServiceState, raw_query: Option<String>, force: bool) -> Response {
    // 现役 `c.req.query("agentId") || null`：空串视同未传。
    let agent_id = query_param(raw_query.as_deref(), "agentId").filter(|id| !id.is_empty());
    let outcome = tokio::task::spawn_blocking(move || {
        let roots = resolve_workspace_roots(&state, agent_id.as_deref());
        let data_home = state.config().data_home.clone();
        detect_report(&data_home, &roots, force)
    })
    .await;
    match outcome {
        Ok(report) => Json(report).into_response(),
        Err(join) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "env_deps_detection_failed",
                "message": join.to_string(),
            })),
        )
            .into_response(),
    }
}

pub async fn env_deps_get_route(
    State(state): State<ServiceState>,
    RawQuery(raw): RawQuery,
) -> Response {
    let force = query_param(raw.as_deref(), "refresh").as_deref() == Some("1");
    respond(state, raw, force).await
}

pub async fn env_deps_refresh_route(
    State(state): State<ServiceState>,
    RawQuery(raw): RawQuery,
) -> Response {
    respond(state, raw, true).await
}
