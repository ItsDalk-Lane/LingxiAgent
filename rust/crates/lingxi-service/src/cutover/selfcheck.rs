//! R08-T05 Phase D —— 切割自检（self-check）协调器驱动。
//!
//! 设计锚：`docs/rust-tauri/R08/T05_DESIGN.md` §9.1/§9.2。
//!
//! 自检进程 = `lingxi-service --home <target> --self-check`：完整启动链
//! （layout → epoch 门 → 存储打开 → 单写者实例锁）照常，但后台写者全部
//! 结构性禁启，业务 HTTP 面结构性关闭（仅 health + 只读 probe，其余一律
//! 响亮 503）。本模块是协调器侧驱动：拉起进程 → 解析 `SELF_CHECK_LISTEN`
//! 行 → 读本地令牌 → 探计数并与采用回执期望值逐项对拍 → 抽样断言业务面
//! 响亮关闭 → SIGTERM → 等退出 → 锁释放探针（复用
//! [`super::probe::probe_rust_instance_lock`] 非变异缝）。
//!
//! 纪律：
//! - 期望计数为零 ≠ 通过；期望值由导入回执给出，逐字段对拍，差异响亮列出。
//! - 任何超时/畸形行/非预期状态码都是响亮错误，绝不静默降级。
//! - 进程拉起类 e2e 属 Phase E（epoch 常数切换后）；本层逻辑全部可单测。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 就绪行前缀（替换正常路径的 `LINGXI_SERVICE_READY`，一行 k=v 契约）。
pub const SELF_CHECK_LISTEN_PREFIX: &str = "SELF_CHECK_LISTEN";
/// 只读探针路径。
pub const PROBE_PATH: &str = "/lingxi/v1/self-check/probe";
/// 健康检查路径（与正常面一致的 Public 端点）。
pub const HEALTH_PATH: &str = "/lingxi/v1/health";
/// 本地令牌文件相对 home 的路径（auth.rs LOCAL_TOKEN_FILE @ runtime_dir）。
pub const LOCAL_TOKEN_RELPATH: &str = "lingxi-service/local-token.json";

/// 解析出的监听行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfCheckListen {
    /// `addr=127.0.0.1:PORT`。
    pub addr: String,
    /// `home=<abs path>`（信息字段，不做裁决）。
    pub home: Option<String>,
    /// `source=<home source>`（信息字段，不做裁决）。
    pub source: Option<String>,
}

/// 采用回执给出的期望计数（§9.1 三项）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfCheckExpectations {
    pub sessions: u64,
    #[serde(rename = "modelCallUsage")]
    pub model_call_usage: u64,
    #[serde(rename = "cronJobs")]
    pub cron_jobs: u64,
}

/// probe 实测计数 + schema 版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelfCheckCounts {
    pub sessions: u64,
    pub model_call_usage: u64,
    pub cron_jobs: u64,
    pub user_version: u64,
}

/// 单字段计数差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountMismatch {
    pub field: &'static str,
    pub expected: u64,
    pub actual: u64,
}

/// 逐字段对拍；空 Vec = 全等。差异响亮列出，绝不四舍五入。
pub fn compare_counts(
    expected: &SelfCheckExpectations,
    actual: &SelfCheckCounts,
) -> Vec<CountMismatch> {
    let mut mismatches = Vec::new();
    let fields: [(&'static str, u64, u64); 3] = [
        ("sessions", expected.sessions, actual.sessions),
        (
            "modelCallUsage",
            expected.model_call_usage,
            actual.model_call_usage,
        ),
        ("cronJobs", expected.cron_jobs, actual.cron_jobs),
    ];
    for (field, expected, actual) in fields {
        if expected != actual {
            mismatches.push(CountMismatch {
                field,
                expected,
                actual,
            });
        }
    }
    mismatches
}

/// 驱动错误：全部响亮，带取证细节。
#[derive(Debug)]
pub enum SelfCheckError {
    /// 命中前缀但行形畸形（缺 addr= / addr 不可解析）。
    MalformedListenLine { line: String, detail: String },
    /// 拉起失败。
    Spawn {
        binary: PathBuf,
        source: std::io::Error,
    },
    /// 等就绪行超时或进程先死。
    ListenLineMissing {
        budget: Duration,
        stdout_tail: Vec<String>,
        child_exit: Option<String>,
    },
    /// 令牌文件不可读/畸形。
    TokenUnreadable { path: PathBuf, detail: String },
    /// HTTP 层错误。
    Http { op: &'static str, detail: String },
    /// 非预期状态码（probe / health / 关闭断言）。
    UnexpectedStatus {
        op: &'static str,
        status: u16,
        body: String,
    },
    /// probe 响应体畸形。
    MalformedBody { detail: String },
    /// 计数对拍失败。
    CountMismatch { mismatches: Vec<CountMismatch> },
    /// 业务面漏出：被抽样的业务路由没有响亮 503。
    SurfaceLeak {
        path: String,
        status: u16,
        body: String,
    },
    /// SIGTERM 后未在预算内退出。
    ShutdownTimeout { budget: Duration },
    /// 退出码非 0。
    UnexpectedExit { status: String },
    /// 退出后实例锁仍被持有。
    LockStillHeld { detail: String },
}

impl std::fmt::Display for SelfCheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedListenLine { line, detail } => {
                write!(f, "malformed SELF_CHECK_LISTEN line ({detail}): {line}")
            }
            Self::Spawn { binary, source } => {
                write!(f, "cannot spawn {}: {source}", binary.display())
            }
            Self::ListenLineMissing {
                budget,
                stdout_tail,
                child_exit,
            } => write!(
                f,
                "SELF_CHECK_LISTEN line not observed within {budget:?} \
                 (child_exit={child_exit:?}, stdout_tail={stdout_tail:?})"
            ),
            Self::TokenUnreadable { path, detail } => {
                write!(f, "local token unreadable at {}: {detail}", path.display())
            }
            Self::Http { op, detail } => write!(f, "self-check {op} http error: {detail}"),
            Self::UnexpectedStatus { op, status, body } => {
                write!(f, "self-check {op} unexpected status {status}: {body}")
            }
            Self::MalformedBody { detail } => {
                write!(f, "self-check probe body malformed: {detail}")
            }
            Self::CountMismatch { mismatches } => {
                let parts: Vec<String> = mismatches
                    .iter()
                    .map(|m| format!("{} expected={} actual={}", m.field, m.expected, m.actual))
                    .collect();
                write!(f, "self-check counts diverge: {}", parts.join("; "))
            }
            Self::SurfaceLeak { path, status, body } => write!(
                f,
                "self-check surface leak at {path}: status {status} (expected 503 \
                 self_check_surface_closed): {body}"
            ),
            Self::ShutdownTimeout { budget } => {
                write!(
                    f,
                    "self-check child did not exit within {budget:?} after SIGTERM"
                )
            }
            Self::UnexpectedExit { status } => {
                write!(f, "self-check child exited with {status} (expected 0)")
            }
            Self::LockStillHeld { detail } => {
                write!(
                    f,
                    "instance lock still held after self-check exit: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for SelfCheckError {}

/// 解析一行 stdout：非监听行 → Ok(None)；命中前缀但畸形 → Err（响亮）；
/// 合法行 → Ok(Some)。
pub fn parse_self_check_listen_line(line: &str) -> Result<Option<SelfCheckListen>, SelfCheckError> {
    let rest = match line.strip_prefix(SELF_CHECK_LISTEN_PREFIX) {
        Some(rest) => rest.trim_start(),
        None => return Ok(None),
    };
    if rest.is_empty() {
        return Err(SelfCheckError::MalformedListenLine {
            line: line.to_string(),
            detail: "prefix present but no k=v payload".into(),
        });
    }
    let mut addr: Option<String> = None;
    let mut home: Option<String> = None;
    let mut source: Option<String> = None;
    for token in rest.split_whitespace() {
        match token.split_once('=') {
            Some(("addr", value)) => addr = Some(value.to_string()),
            Some(("home", value)) => home = Some(value.to_string()),
            Some(("source", value)) => source = Some(value.to_string()),
            _ => {
                return Err(SelfCheckError::MalformedListenLine {
                    line: line.to_string(),
                    detail: format!("unrecognized token: {token}"),
                })
            }
        }
    }
    let addr = addr.ok_or_else(|| SelfCheckError::MalformedListenLine {
        line: line.to_string(),
        detail: "missing addr=".into(),
    })?;
    // addr 必须可解析为 socket 地址（host:port）——畸形值响亮拒绝，
    // 不把不可拨号的地址带进探针。
    addr.parse::<std::net::SocketAddr>()
        .map_err(|err| SelfCheckError::MalformedListenLine {
            line: line.to_string(),
            detail: format!("addr= not a socket address: {addr}: {err}"),
        })?;
    Ok(Some(SelfCheckListen { addr, home, source }))
}

/// 从 home 读取本地回环令牌（0600 owner-only 文件，camelCase JSON）。
pub fn read_local_token(home: &Path) -> Result<String, SelfCheckError> {
    let path = home.join(LOCAL_TOKEN_RELPATH);
    let raw = std::fs::read(&path).map_err(|err| SelfCheckError::TokenUnreadable {
        path: path.clone(),
        detail: err.to_string(),
    })?;
    let value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|err| SelfCheckError::TokenUnreadable {
            path: path.clone(),
            detail: format!("not JSON: {err}"),
        })?;
    value["token"]
        .as_str()
        .map(str::to_string)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| SelfCheckError::TokenUnreadable {
            path,
            detail: "missing/empty \"token\" field".into(),
        })
}

/// probe 响应体的最小契约（serde 严格取数；缺字段 = MalformedBody）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProbeBody {
    counts: ProbeCounts,
    user_version: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProbeCounts {
    sessions: serde_json::Value,
    model_call_usage: serde_json::Value,
    cron_jobs: serde_json::Value,
}

fn strict_u64(value: &serde_json::Value, field: &'static str) -> Result<u64, SelfCheckError> {
    value.as_u64().ok_or_else(|| SelfCheckError::MalformedBody {
        detail: format!("field {field} missing or not an unsigned integer: {value}"),
    })
}

/// 只读计数探针：GET probe，Bearer 令牌，要求 200 + 严格契约体。
pub async fn probe_counts(
    client: &reqwest::Client,
    addr: &str,
    token: &str,
) -> Result<SelfCheckCounts, SelfCheckError> {
    let url = format!("http://{addr}{PROBE_PATH}");
    let response = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|err| SelfCheckError::Http {
            op: "probe",
            detail: err.to_string(),
        })?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(|err| SelfCheckError::Http {
        op: "probe",
        detail: format!("body read: {err}"),
    })?;
    if status != 200 {
        return Err(SelfCheckError::UnexpectedStatus {
            op: "probe",
            status,
            body,
        });
    }
    let parsed: ProbeBody =
        serde_json::from_str(&body).map_err(|err| SelfCheckError::MalformedBody {
            detail: format!("{err}: {body}"),
        })?;
    Ok(SelfCheckCounts {
        sessions: strict_u64(&parsed.counts.sessions, "sessions")?,
        model_call_usage: strict_u64(&parsed.counts.model_call_usage, "modelCallUsage")?,
        cron_jobs: strict_u64(&parsed.counts.cron_jobs, "cronJobs")?,
        user_version: strict_u64(&parsed.user_version, "userVersion")?,
    })
}

/// 健康端点必须是 200（Public，无令牌也通）。
pub async fn probe_health(client: &reqwest::Client, addr: &str) -> Result<(), SelfCheckError> {
    let url = format!("http://{addr}{HEALTH_PATH}");
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|err| SelfCheckError::Http {
            op: "health",
            detail: err.to_string(),
        })?;
    let status = response.status().as_u16();
    if status != 200 {
        let body = response.text().await.unwrap_or_default();
        return Err(SelfCheckError::UnexpectedStatus {
            op: "health",
            status,
            body,
        });
    }
    Ok(())
}

/// 抽样断言业务面响亮关闭：带令牌打一个读路由 + 一个写路由，两个都
/// 必须 503 且 error=self_check_surface_closed；任何其他结果（404 吞、
/// 2xx 漏）都是 SurfaceLeak。
pub async fn assert_surface_closed(
    client: &reqwest::Client,
    addr: &str,
    token: &str,
) -> Result<(), SelfCheckError> {
    for (method, path) in [("GET", "/lingxi/v1/cron"), ("POST", "/lingxi/v1/sessions")] {
        let url = format!("http://{addr}{path}");
        let request = match method {
            "GET" => client.get(&url),
            _ => client.post(&url),
        }
        .bearer_auth(token);
        let response = request.send().await.map_err(|err| SelfCheckError::Http {
            op: "surface-closed",
            detail: err.to_string(),
        })?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|err| SelfCheckError::Http {
            op: "surface-closed",
            detail: format!("body read: {err}"),
        })?;
        let closed = status == 503
            && serde_json::from_str::<serde_json::Value>(&body)
                .map(|value| value["error"].as_str() == Some("self_check_surface_closed"))
                .unwrap_or(false);
        if !closed {
            return Err(SelfCheckError::SurfaceLeak {
                path: path.to_string(),
                status,
                body,
            });
        }
    }
    Ok(())
}

/// 完整自检驱动的通过报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfCheckReport {
    pub addr: String,
    pub counts: SelfCheckCounts,
    pub surface_closed_paths: Vec<String>,
    pub lock_released: bool,
}

/// 拉起 → 探 → 对拍 → 关闭断言 → SIGTERM → 等退出 → 锁释放探针。
///
/// 任一步失败都尽力 SIGTERM 子进程再返回响亮错误（不留孤儿自检进程
/// 持锁占着目标根）。进程 e2e 属 Phase E；本函数逻辑路径全部经
/// tokio 可测缝注入。
pub async fn run_self_check(
    binary: &Path,
    home: &Path,
    expectations: &SelfCheckExpectations,
    listen_budget: Duration,
    shutdown_budget: Duration,
) -> Result<SelfCheckReport, SelfCheckError> {
    use tokio::io::AsyncBufReadExt as _;

    let mut command = tokio::process::Command::new(binary);
    command
        .arg("--home")
        .arg(home)
        .arg("--self-check")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // 防御：绝不继承调用方 stdin。
        .stdin(std::process::Stdio::null());
    let mut child = command.spawn().map_err(|source| SelfCheckError::Spawn {
        binary: binary.to_path_buf(),
        source,
    })?;

    // 失败路径统一收尾：SIGTERM → 等一小段 → 仍未死则 kill。
    async fn terminate(child: &mut tokio::process::Child) {
        send_sigterm(child);
        let grace = Duration::from_secs(2);
        if tokio::time::timeout(grace, child.wait()).await.is_err() {
            let _ = child.kill().await;
        }
    }

    let stdout = child
        .stdout
        .take()
        .expect("piped stdout is present before spawn returns Ok");
    let mut lines = tokio::io::BufReader::new(stdout).lines();
    let mut tail: Vec<String> = Vec::new();
    let listen = loop {
        let next = tokio::time::timeout(listen_budget, lines.next_line()).await;
        match next {
            Ok(Ok(Some(line))) => {
                match parse_self_check_listen_line(&line) {
                    Ok(Some(listen)) => break listen,
                    Ok(None) => {
                        // 保留尾部上下文用于响亮报错（有界）。
                        if tail.len() < 32 {
                            tail.push(line);
                        }
                    }
                    Err(err) => {
                        terminate(&mut child).await;
                        return Err(err);
                    }
                }
            }
            Ok(Ok(None)) => {
                let exit = child
                    .try_wait()
                    .ok()
                    .flatten()
                    .map(|status| status.to_string());
                return Err(SelfCheckError::ListenLineMissing {
                    budget: listen_budget,
                    stdout_tail: tail,
                    child_exit: exit,
                });
            }
            Ok(Err(err)) => {
                terminate(&mut child).await;
                return Err(SelfCheckError::Http {
                    op: "stdout-read",
                    detail: err.to_string(),
                });
            }
            Err(_) => {
                terminate(&mut child).await;
                let exit = child
                    .try_wait()
                    .ok()
                    .flatten()
                    .map(|status| status.to_string());
                return Err(SelfCheckError::ListenLineMissing {
                    budget: listen_budget,
                    stdout_tail: tail,
                    child_exit: exit,
                });
            }
        }
    };

    let result = drive_probes(&listen, home, expectations).await;
    if let Err(err) = result {
        terminate(&mut child).await;
        return Err(err);
    }
    let (counts, surface_closed_paths) = result.expect("checked above");

    // 正常通道：SIGTERM → 预算内等退出 → 退出码 0。
    send_sigterm(&mut child);
    let status = match tokio::time::timeout(shutdown_budget, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => {
            terminate(&mut child).await;
            return Err(SelfCheckError::Http {
                op: "wait-exit",
                detail: err.to_string(),
            });
        }
        Err(_) => {
            terminate(&mut child).await;
            return Err(SelfCheckError::ShutdownTimeout {
                budget: shutdown_budget,
            });
        }
    };
    if !status.success() {
        return Err(SelfCheckError::UnexpectedExit {
            status: status.to_string(),
        });
    }

    // 锁释放探针（非变异缝：open 不 create，try_lock 立即释放）。
    let lock = super::probe::probe_rust_instance_lock(home).map_err(|err| {
        SelfCheckError::LockStillHeld {
            detail: format!("lock probe io failure: {err}"),
        }
    })?;
    if lock.probe.is_blocking() {
        return Err(SelfCheckError::LockStillHeld {
            detail: lock.detail,
        });
    }

    Ok(SelfCheckReport {
        addr: listen.addr,
        counts,
        surface_closed_paths,
        lock_released: true,
    })
}

/// 探针段：token → health → probe 计数 → 对拍 → 关闭断言。
async fn drive_probes(
    listen: &SelfCheckListen,
    home: &Path,
    expectations: &SelfCheckExpectations,
) -> Result<(SelfCheckCounts, Vec<String>), SelfCheckError> {
    let token = read_local_token(home)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|err| SelfCheckError::Http {
            op: "client-build",
            detail: err.to_string(),
        })?;
    probe_health(&client, &listen.addr).await?;
    let counts = probe_counts(&client, &listen.addr, &token).await?;
    let mismatches = compare_counts(expectations, &counts);
    if !mismatches.is_empty() {
        return Err(SelfCheckError::CountMismatch { mismatches });
    }
    assert_surface_closed(&client, &listen.addr, &token).await?;
    Ok((
        counts,
        vec![
            "/lingxi/v1/cron".to_string(),
            "/lingxi/v1/sessions".to_string(),
        ],
    ))
}

#[cfg(unix)]
fn send_sigterm(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        // libc 依赖已随 statfs（P2 裁决）引入；SIGTERM 走正常关停通道。
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    }
}

#[cfg(not(unix))]
fn send_sigterm(child: &mut tokio::process::Child) {
    // 无 SIGTERM 语义的平台退化为 kill（响亮，不静默）。
    let _ = child.start_kill();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expectations() -> SelfCheckExpectations {
        SelfCheckExpectations {
            sessions: 3,
            model_call_usage: 7,
            cron_jobs: 2,
        }
    }

    #[test]
    fn parse_listen_line_happy_path() {
        let listen = parse_self_check_listen_line(
            "SELF_CHECK_LISTEN addr=127.0.0.1:54321 home=/tmp/x source=flag",
        )
        .expect("parses")
        .expect("is a listen line");
        assert_eq!(listen.addr, "127.0.0.1:54321");
        assert_eq!(listen.home.as_deref(), Some("/tmp/x"));
        assert_eq!(listen.source.as_deref(), Some("flag"));
    }

    #[test]
    fn parse_listen_line_ignores_other_lines() {
        assert!(parse_self_check_listen_line(
            "LINGXI_SERVICE_READY addr=127.0.0.1:1 home=/h source=env"
        )
        .expect("ok")
        .is_none());
        assert!(parse_self_check_listen_line("").expect("ok").is_none());
        assert!(parse_self_check_listen_line("info: boot")
            .expect("ok")
            .is_none());
    }

    #[test]
    fn parse_listen_line_rejects_malformed_loudly() {
        // 前缀在但缺 addr → 响亮拒绝。
        let err = parse_self_check_listen_line("SELF_CHECK_LISTEN home=/tmp/x").unwrap_err();
        assert!(matches!(err, SelfCheckError::MalformedListenLine { .. }));
        // addr 不可解析 → 响亮拒绝。
        let err = parse_self_check_listen_line("SELF_CHECK_LISTEN addr=not-an-addr").unwrap_err();
        assert!(matches!(err, SelfCheckError::MalformedListenLine { .. }));
        // 裸前缀 → 响亮拒绝。
        let err = parse_self_check_listen_line("SELF_CHECK_LISTEN").unwrap_err();
        assert!(matches!(err, SelfCheckError::MalformedListenLine { .. }));
        // 未知 token → 响亮拒绝（不静默吞）。
        let err =
            parse_self_check_listen_line("SELF_CHECK_LISTEN addr=127.0.0.1:9 bogus").unwrap_err();
        assert!(matches!(err, SelfCheckError::MalformedListenLine { .. }));
    }

    #[test]
    fn compare_counts_exact_and_diff_per_field() {
        let expected = expectations();
        let exact = SelfCheckCounts {
            sessions: 3,
            model_call_usage: 7,
            cron_jobs: 2,
            user_version: 28,
        };
        assert!(compare_counts(&expected, &exact).is_empty());
        let diverged = SelfCheckCounts {
            sessions: 3,
            model_call_usage: 6,
            cron_jobs: 0,
            user_version: 28,
        };
        let mismatches = compare_counts(&expected, &diverged);
        assert_eq!(
            mismatches,
            vec![
                CountMismatch {
                    field: "modelCallUsage",
                    expected: 7,
                    actual: 6
                },
                CountMismatch {
                    field: "cronJobs",
                    expected: 2,
                    actual: 0
                },
            ]
        );
        let message = SelfCheckError::CountMismatch {
            mismatches: mismatches.clone(),
        }
        .to_string();
        assert!(message.contains("modelCallUsage expected=7 actual=6"));
        assert!(message.contains("cronJobs expected=2 actual=0"));
    }

    #[test]
    fn read_local_token_roundtrip_and_loud_failures() {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-t05-selfcheck-token-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let runtime = dir.join("lingxi-service");
        std::fs::create_dir_all(&runtime).expect("runtime dir");
        // 缺文件 → 响亮。
        let err = read_local_token(&dir).unwrap_err();
        assert!(matches!(err, SelfCheckError::TokenUnreadable { .. }));
        // 畸形 JSON → 响亮。
        let token_path = runtime.join("local-token.json");
        std::fs::write(&token_path, b"not json").expect("write");
        let err = read_local_token(&dir).unwrap_err();
        assert!(matches!(err, SelfCheckError::TokenUnreadable { .. }));
        // 缺 token 字段 → 响亮。
        std::fs::write(&token_path, br#"{"schemaVersion":1}"#).expect("write");
        let err = read_local_token(&dir).unwrap_err();
        assert!(matches!(err, SelfCheckError::TokenUnreadable { .. }));
        // 合法 → 取得令牌。
        std::fs::write(
            &token_path,
            br#"{"schemaVersion":1,"kind":"local_token","token":"abc123","instanceId":"i","createdAtUnixMs":0}"#,
        )
        .expect("write");
        assert_eq!(read_local_token(&dir).expect("token"), "abc123");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn strict_u64_rejects_non_integer() {
        assert_eq!(
            strict_u64(&serde_json::json!(28), "userVersion").expect("ok"),
            28
        );
        assert!(matches!(
            strict_u64(&serde_json::json!("28"), "userVersion"),
            Err(SelfCheckError::MalformedBody { .. })
        ));
        assert!(matches!(
            strict_u64(&serde_json::json!(-1), "userVersion"),
            Err(SelfCheckError::MalformedBody { .. })
        ));
    }

    #[test]
    fn expectations_deserialize_camel_case() {
        let parsed: SelfCheckExpectations =
            serde_json::from_str(r#"{"sessions":1,"modelCallUsage":2,"cronJobs":3}"#)
                .expect("camelCase contract");
        assert_eq!(parsed.model_call_usage, 2);
        assert_eq!(parsed.cron_jobs, 3);
    }
}
