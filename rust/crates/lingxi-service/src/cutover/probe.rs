//! 双锁探针（T05_DESIGN §5.2）：预检「旧实例状态」项的取证原料。
//!
//! 两条独立证据链，任一发现活写者 = fail-closed 拒绝：
//! - **旧栈（Node）**：读 `{source}/server-info.json` → token 认证 HTTP 探针
//!   （四态与 shared/server-info-probe.cjs 完全同义：alive-same-home /
//!   alive-unauthorized / not-hana / dead）。alive-same-home 与
//!   alive-unauthorized = 拒绝；not-hana 与 dead = 通过（留取证行）。
//! - **新栈（Rust）**：`{source}/lingxi-service/instance.lock` 存在 →
//!   **open 不带 create** + `File::try_lock`；拿不到 = 活写者 → 拒绝；
//!   拿到 = 立即释放。文件不存在 = 通过（尚无人持有）。
//!
//! 铁律：此模块**对源根零写零建零删**——连读探针都不允许 O_CREAT。

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const SERVER_INFO_FILE: &str = "server-info.json";
pub const PROBE_PATH: &str = "/api/server/identity";
pub const PROBE_TIMEOUT_MS: u64 = 2000;

/// 与 shared/server-info-probe.cjs 的四态一一对应（命名逐字节相同）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerInfoProbe {
    #[serde(rename = "alive-same-home")]
    AliveSameHome,
    #[serde(rename = "alive-unauthorized")]
    AliveUnauthorized,
    #[serde(rename = "not-hana")]
    NotHana,
    #[serde(rename = "dead")]
    Dead,
}

impl ServerInfoProbe {
    /// 与 isForeignServerBlocking 同口径：这两态 = 阻断。
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::AliveSameHome | Self::AliveUnauthorized)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyProbeOutcome {
    pub probe: ServerInfoProbe,
    /// 探针记录的可复述细节（端口/状态码/拒绝原因），非阻断态的取证行。
    pub detail: String,
    #[serde(rename = "serverInfoPath")]
    pub server_info_path: String,
}

#[derive(Debug)]
pub enum ProbeError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Malformed {
        path: PathBuf,
        detail: String,
    },
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeError::Io { path, source } => {
                write!(f, "probe io error at {}: {source}", path.display())
            }
            ProbeError::Malformed { path, detail } => {
                write!(f, "malformed server-info at {}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for ProbeError {}

#[derive(Deserialize)]
struct ServerInfoFile {
    port: Option<u64>,
    token: Option<String>,
}

/// 旧栈四态探针。server-info.json 不存在 = dead（无坐标可探）。
/// 文件存在但 JSON 损坏 = 响亮 Err（绝不把损坏当成 dead 放行）。
pub fn probe_legacy_server_info(source: &Path) -> Result<LegacyProbeOutcome, ProbeError> {
    let path = source.join(SERVER_INFO_FILE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LegacyProbeOutcome {
                probe: ServerInfoProbe::Dead,
                detail: "server-info.json absent; nothing to probe".into(),
                server_info_path: path.display().to_string(),
            });
        }
        Err(source) => return Err(ProbeError::Io { path, source }),
    };
    let info: ServerInfoFile = serde_json::from_str(&raw).map_err(|err| ProbeError::Malformed {
        path: path.clone(),
        detail: format!("JSON parse: {err}"),
    })?;
    let port = info.port.unwrap_or(0);
    let token = info.token.unwrap_or_default();
    if port == 0 || port > u16::MAX as u64 || token.is_empty() {
        return Ok(LegacyProbeOutcome {
            probe: ServerInfoProbe::Dead,
            detail: "server-info.json lacks port/token; no coordinates to probe".into(),
            server_info_path: path.display().to_string(),
        });
    }
    let (probe, detail) = http_identity_probe(port as u16, &token);
    Ok(LegacyProbeOutcome {
        probe,
        detail,
        server_info_path: path.display().to_string(),
    })
}

/// std-only 阻塞 HTTP/1.1 GET（与 instance.rs probe_peer 同纪律：
/// connect/read 各自限时预算，绝不无限挂起）。
fn http_identity_probe(port: u16, token: &str) -> (ServerInfoProbe, String) {
    let budget = Duration::from_millis(PROBE_TIMEOUT_MS);
    let started = Instant::now();
    let addr = format!("127.0.0.1:{port}");
    let mut stream = match TcpStream::connect_timeout(
        &match addr.parse() {
            Ok(addr) => addr,
            Err(err) => return (ServerInfoProbe::Dead, format!("addr parse: {err}")),
        },
        budget,
    ) {
        Ok(stream) => stream,
        Err(err) => return (ServerInfoProbe::Dead, format!("connect: {err}")),
    };
    let _ = stream.set_read_timeout(Some(budget));
    let _ = stream.set_write_timeout(Some(budget));
    let request = format!(
        "GET {PROBE_PATH} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    if let Err(err) = stream.write_all(request.as_bytes()) {
        return (ServerInfoProbe::Dead, format!("write: {err}"));
    }
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        if started.elapsed() > budget {
            return (ServerInfoProbe::Dead, "timeout".into());
        }
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > 256 * 1024 {
                    break;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(err) => return (ServerInfoProbe::Dead, format!("read: {err}")),
        }
    }
    classify_response(&raw)
}

fn classify_response(raw: &[u8]) -> (ServerInfoProbe, String) {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = match text.split_once("\r\n\r\n") {
        Some(pair) => pair,
        None => (text.as_ref(), ""),
    };
    let status: u64 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    match status {
        200 => match parsed
            .as_ref()
            .and_then(|b| b.get("serverId"))
            .and_then(|v| v.as_str())
        {
            Some(s) if !s.is_empty() => (
                ServerInfoProbe::AliveSameHome,
                "200 with server-identity shape".into(),
            ),
            _ => (
                ServerInfoProbe::NotHana,
                format!("200 response did not match the server-identity shape: {body:.200}"),
            ),
        },
        403 => {
            let shaped = parsed
                .as_ref()
                .map(|b| {
                    b.get("reason").and_then(|v| v.as_str()).is_some()
                        || b.get("error").and_then(|v| v.as_str()).is_some()
                })
                .unwrap_or(false);
            if shaped {
                (
                    ServerInfoProbe::AliveUnauthorized,
                    "403 auth-rejection shape".into(),
                )
            } else {
                (
                    ServerInfoProbe::NotHana,
                    format!("403 response did not match the auth-rejection shape: {body:.200}"),
                )
            }
        }
        other => (
            ServerInfoProbe::NotHana,
            format!("unexpected HTTP status {other}"),
        ),
    }
}

/// 新栈 OS 锁探针结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RustLockProbe {
    /// 锁文件不存在：无人持有过新栈实例 = 通过。
    #[serde(rename = "absent")]
    Absent,
    /// 锁可取（已立即释放）：无活写者 = 通过。
    #[serde(rename = "acquirable")]
    Acquirable,
    /// 锁被持有：活 Rust 写者 = 拒绝。
    #[serde(rename = "held")]
    Held,
}

impl RustLockProbe {
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Held)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustLockProbeOutcome {
    pub probe: RustLockProbe,
    pub detail: String,
    #[serde(rename = "lockPath")]
    pub lock_path: String,
}

/// 新栈 OS 锁非变异探针：**open 不带 create**，try_lock 后立即释放。
/// 绝不创建/截断/删除锁文件（源根零写红线）。
pub fn probe_rust_instance_lock(source: &Path) -> Result<RustLockProbeOutcome, ProbeError> {
    let path = source.join("lingxi-service").join("instance.lock");
    let outcome = |probe: RustLockProbe, detail: String| {
        Ok(RustLockProbeOutcome {
            probe,
            detail,
            lock_path: path.display().to_string(),
        })
    };
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(false)
        .truncate(false)
        .open(&path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return outcome(RustLockProbe::Absent, "instance.lock absent".into());
        }
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            // 只读回退：读模式也能 flock（unix）；windows 下不可行则响亮拒绝。
            match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(source) => return Err(ProbeError::Io { path, source }),
            }
        }
        Err(source) => return Err(ProbeError::Io { path, source }),
    };
    match file.try_lock() {
        Ok(()) => {
            drop(file); // 立即释放
            outcome(
                RustLockProbe::Acquirable,
                "lock acquired and released".into(),
            )
        }
        Err(err) => outcome(RustLockProbe::Held, format!("try_lock refused: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_root(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t05-probe-test-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn absent_server_info_is_dead_not_blocking() {
        let root = temp_root("absent");
        let out = probe_legacy_server_info(&root).unwrap();
        assert_eq!(out.probe, ServerInfoProbe::Dead);
        assert!(!out.probe.is_blocking());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn corrupt_server_info_is_loud_error() {
        let root = temp_root("corrupt");
        std::fs::write(root.join(SERVER_INFO_FILE), b"{nope").unwrap();
        let err = probe_legacy_server_info(&root).unwrap_err();
        assert!(err.to_string().contains("malformed"), "{err}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unreachable_port_is_dead_not_blocking() {
        let root = temp_root("unreach");
        // 拣一个几乎确定空闲的端口：bind 0 取得后立即释放再探测。
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        std::fs::write(
            root.join(SERVER_INFO_FILE),
            serde_json::to_string(&serde_json::json!({"port": port, "token": "t".repeat(32)}))
                .unwrap(),
        )
        .unwrap();
        let out = probe_legacy_server_info(&root).unwrap();
        assert_eq!(out.probe, ServerInfoProbe::Dead, "{}", out.detail);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn shaped_200_is_alive_same_home_and_blocking() {
        let root = temp_root("alive");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = r#"{"serverId":"srv-test-1"}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        std::fs::write(
            root.join(SERVER_INFO_FILE),
            serde_json::to_string(&serde_json::json!({"port": port, "token": "t".repeat(32)}))
                .unwrap(),
        )
        .unwrap();
        let out = probe_legacy_server_info(&root).unwrap();
        assert_eq!(out.probe, ServerInfoProbe::AliveSameHome, "{}", out.detail);
        assert!(out.probe.is_blocking());
        drop(out);
        drop(server); // detach; listener closes when thread's listener drops on process end
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn shaped_403_is_alive_unauthorized_and_blocking() {
        let root = temp_root("unauth");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let _server = std::thread::spawn(move || {
            if let Some(stream) = listener.incoming().next() {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = r#"{"error":"unauthorized","reason":"bad token"}"#;
                let resp = format!(
                    "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        std::fs::write(
            root.join(SERVER_INFO_FILE),
            serde_json::to_string(&serde_json::json!({"port": port, "token": "t".repeat(32)}))
                .unwrap(),
        )
        .unwrap();
        let out = probe_legacy_server_info(&root).unwrap();
        assert_eq!(
            out.probe,
            ServerInfoProbe::AliveUnauthorized,
            "{}",
            out.detail
        );
        assert!(out.probe.is_blocking());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rust_lock_absent_then_acquirable_then_held() {
        let root = temp_root("rustlock");
        let out = probe_rust_instance_lock(&root).unwrap();
        assert_eq!(out.probe, RustLockProbe::Absent);

        let lock_dir = root.join("lingxi-service");
        std::fs::create_dir_all(&lock_dir).unwrap();
        let lock_path = lock_dir.join("instance.lock");
        let holder = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&lock_path)
            .unwrap();
        // 未持锁时探针可取。
        let out = probe_rust_instance_lock(&root).unwrap();
        assert_eq!(out.probe, RustLockProbe::Acquirable, "{}", out.detail);
        // 同进程持锁后探针必须报 Held（flock 语义：open file description 级互斥）。
        holder.try_lock().unwrap();
        let out = probe_rust_instance_lock(&root).unwrap();
        assert_eq!(out.probe, RustLockProbe::Held, "{}", out.detail);
        assert!(out.probe.is_blocking());
        drop(holder);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn probe_never_creates_lock_file() {
        let root = temp_root("nomutate");
        probe_rust_instance_lock(&root).unwrap();
        assert!(!root.join("lingxi-service/instance.lock").exists());
        // 锁文件存在但源根其余目录树零变化已由 Absent/Acquirable 用例覆盖。
        std::fs::remove_dir_all(&root).unwrap();
    }
}
