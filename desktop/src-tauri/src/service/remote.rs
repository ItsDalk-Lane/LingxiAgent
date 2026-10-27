//! R09-T02 远端模式（C21 / A04）。
//!
//! 环境契约与 Electron `desktop/src/shared/rust-local-service.cjs:319-375` 同形，
//! 便于 R08-T03 夹具平移：
//! - `LINGXI_DESKTOP_RUST_REMOTE_URL`（http(s)、显式端口、禁 userinfo/path/query/hash）；
//! - `LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE`（绝对路径、owner-only、`{"token":"<32 hex>"}`）；
//! - 非 loopback 的明文 http 需要 `LINGXI_DESKTOP_RUST_REMOTE_ALLOW_CLEARTEXT=1`；
//! - 必须同时声明 `LINGXI_DESKTOP_SERVER_RUNTIME=rust`（矛盾即拒绝）。
//!
//! 远端仅 debug/dev 构建允许（release 拒绝 `REMOTE_REQUIRES_DEV_BUILD`），且退出
//! 路径结构性零接触（不持有 Child 句柄，见 supervisor）。

use std::net::IpAddr;
use std::path::Path;

use super::env::EnvSnapshot;
use super::error::{codes, ServiceResult, StartError};
use super::instance;

pub const REMOTE_URL_ENV: &str = "LINGXI_DESKTOP_RUST_REMOTE_URL";
pub const REMOTE_TOKEN_ENV: &str = "LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE";
pub const REMOTE_CLEARTEXT_ENV: &str = "LINGXI_DESKTOP_RUST_REMOTE_ALLOW_CLEARTEXT";
pub const RUNTIME_ENV: &str = "LINGXI_DESKTOP_SERVER_RUNTIME";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTarget {
    pub host: String,
    pub port: u16,
    pub transport: String,
    pub base_url: String,
    pub token: String,
}

fn env_error(message: impl Into<String>) -> StartError {
    StartError::new(codes::REMOTE_ENV_INVALID, message)
}

fn is_loopback_host(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    host.parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

fn is_hex_32(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// 远端请求是否存在（只看 URL 变量是否非空）。
pub fn remote_present(env: &EnvSnapshot) -> bool {
    env.get_non_empty(REMOTE_URL_ENV).is_some()
}

/// 解析并验证远端契约；不存在远端配置时返回 `Ok(None)`。
pub fn remote_from_env(env: &EnvSnapshot) -> ServiceResult<Option<RemoteTarget>> {
    let Some(raw_url) = env.get_non_empty(REMOTE_URL_ENV) else {
        return Ok(None);
    };
    let runtime = env.get_non_empty(RUNTIME_ENV).unwrap_or("node");
    if runtime != "rust" {
        return Err(env_error(
            "LINGXI_DESKTOP_RUST_REMOTE_URL requires LINGXI_DESKTOP_SERVER_RUNTIME=rust",
        )
        .with_detail(serde_json::json!({"runtime": runtime})));
    }
    if cfg!(windows) {
        return Err(env_error(
            "Rust remote desktop connection is not supported on Windows in preview",
        ));
    }
    let token_path = env
        .get_non_empty(REMOTE_TOKEN_ENV)
        .ok_or_else(|| env_error("Rust remote requires LINGXI_DESKTOP_RUST_REMOTE_TOKEN_FILE"))?;
    if !Path::new(token_path).is_absolute() {
        return Err(env_error("Rust remote token file must be an absolute path")
            .with_detail(serde_json::json!({"tokenFile": token_path})));
    }
    let target = parse_remote_url(raw_url, env.get(REMOTE_CLEARTEXT_ENV) == Some("1"))?;
    // URL 形状先于令牌文件读取校验：坏 URL 不触发任何文件访问。
    let credential =
        instance::read_private_json(Path::new(token_path), true, None).map_err(|error| {
            env_error(format!(
                "Rust remote token file is missing or not owner-only: {}",
                error.message
            ))
            .with_detail(serde_json::json!({"tokenFile": token_path}))
        })?;
    let token = credential
        .get("token")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            env_error(
                "Rust remote token file must contain a {\"token\":\"<32 lowercase hex>\"} record",
            )
            .with_detail(serde_json::json!({"tokenFile": token_path}))
        })?;
    if !is_hex_32(&token) {
        return Err(
            env_error("Rust remote token file token is not 32 lowercase hex characters")
                .with_detail(serde_json::json!({"tokenFile": token_path})),
        );
    }
    Ok(Some(RemoteTarget { token, ..target }))
}

fn parse_remote_url(raw_url: &str, allow_cleartext: bool) -> ServiceResult<RemoteTarget> {
    let url = reqwest::Url::parse(raw_url)
        .map_err(|_| env_error("invalid LINGXI_DESKTOP_RUST_REMOTE_URL: not an absolute URL"))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(env_error(
            "invalid LINGXI_DESKTOP_RUST_REMOTE_URL: only http(s) is allowed",
        ));
    }
    // 显式端口：默认端口会被 URL 归一化为 None，直接拒绝。
    let Some(port) = url.port() else {
        return Err(env_error(
            "invalid LINGXI_DESKTOP_RUST_REMOTE_URL: an explicit port is required",
        ));
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        return Err(env_error(
            "invalid LINGXI_DESKTOP_RUST_REMOTE_URL: host/port shape is not allowed",
        )
        .with_detail(serde_json::json!({
            "userinfo": !url.username().is_empty() || url.password().is_some(),
            "path": url.path(),
            "query": url.query(),
            "fragment": url.fragment(),
        })));
    }
    let host = url.host_str().unwrap_or_default().to_string();
    let transport = if scheme == "https" { "https" } else { "http" };
    if transport == "http" && !is_loopback_host(&host) && !allow_cleartext {
        return Err(env_error(
            "Rust remote over cleartext http requires LINGXI_DESKTOP_RUST_REMOTE_ALLOW_CLEARTEXT=1 (drill only)",
        )
        .with_detail(serde_json::json!({"host": host})));
    }
    Ok(RemoteTarget {
        host,
        port,
        transport: transport.to_string(),
        base_url: url.origin().ascii_serialization(),
        token: String::new(),
    })
}

/// release（非 debug）构建拒绝远端；debug/dev 预览允许（Q-08 裁决）。
pub fn enforce_dev_only(debug_build: bool) -> ServiceResult<()> {
    if debug_build {
        Ok(())
    } else {
        Err(StartError::new(
            codes::REMOTE_REQUIRES_DEV_BUILD,
            "Rust remote desktop connection is a dev preview; release builds refuse it",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r09-remote-{name}-{}-{}",
            std::process::id(),
            super::super::diagnostics::now_unix_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env_with(url: &str, token_file: &str) -> EnvSnapshot {
        EnvSnapshot::from_pairs([
            (REMOTE_URL_ENV, url),
            (REMOTE_TOKEN_ENV, token_file),
            (RUNTIME_ENV, "rust"),
        ])
    }

    fn write_token(dir: &Path, value: &str) -> String {
        let path = dir.join("token.json");
        fs::write(&path, format!("{{\"token\":\"{value}\"}}")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn valid_loopback_remote_is_accepted() {
        let dir = temp_dir("ok");
        let token_file = write_token(&dir, "0123456789abcdef0123456789abcdef");
        let env = env_with("http://127.0.0.1:43119", &token_file);
        let target = remote_from_env(&env).unwrap().unwrap();
        assert_eq!(target.port, 43119);
        assert_eq!(target.base_url, "http://127.0.0.1:43119");
        assert_eq!(target.transport, "http");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_runtime_flag_is_rejected() {
        let dir = temp_dir("runtime");
        let token_file = write_token(&dir, "0123456789abcdef0123456789abcdef");
        let env = EnvSnapshot::from_pairs([
            (REMOTE_URL_ENV, "http://127.0.0.1:43119"),
            (REMOTE_TOKEN_ENV, token_file.as_str()),
        ]);
        let error = remote_from_env(&env).unwrap_err();
        assert_eq!(error.code, codes::REMOTE_ENV_INVALID);
        assert!(error.message.contains("LINGXI_DESKTOP_SERVER_RUNTIME=rust"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn url_shape_violations_are_rejected() {
        let dir = temp_dir("shape");
        let token_file = write_token(&dir, "0123456789abcdef0123456789abcdef");
        for url in [
            "http://127.0.0.1:43119/path",
            "http://127.0.0.1:43119?query=1",
            "http://user:pass@127.0.0.1:43119",
            "ftp://127.0.0.1:43119",
            "http://127.0.0.1",
        ] {
            let env = env_with(url, &token_file);
            let error = remote_from_env(&env).unwrap_err();
            assert_eq!(error.code, codes::REMOTE_ENV_INVALID, "url {url}");
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn cleartext_gate_applies_to_non_loopback_hosts() {
        let dir = temp_dir("cleartext");
        let token_file = write_token(&dir, "0123456789abcdef0123456789abcdef");
        let env = env_with("http://192.168.1.9:43119", &token_file);
        let error = remote_from_env(&env).unwrap_err();
        assert!(error.message.contains("ALLOW_CLEARTEXT"));
        let mut pairs = vec![
            (REMOTE_URL_ENV, "http://192.168.1.9:43119"),
            (REMOTE_TOKEN_ENV, token_file.as_str()),
            (RUNTIME_ENV, "rust"),
        ];
        pairs.push((REMOTE_CLEARTEXT_ENV, "1"));
        let env = EnvSnapshot::from_pairs(pairs);
        let target = remote_from_env(&env).unwrap().unwrap();
        assert_eq!(target.host, "192.168.1.9");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn token_file_must_be_absolute_and_private() {
        let dir = temp_dir("token");
        let token_file = write_token(&dir, "0123456789abcdef0123456789abcdef");
        let mut env = env_with("http://127.0.0.1:43119", "relative/token.json");
        let error = remote_from_env(&env).unwrap_err();
        assert!(error.message.contains("absolute"));
        env = env_with("http://127.0.0.1:43119", &token_file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&token_file, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let error = remote_from_env(&env).unwrap_err();
        assert!(error.message.contains("owner-only"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_token_is_rejected() {
        let dir = temp_dir("badtoken");
        let token_file = write_token(&dir, "not-hex");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&token_file, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let env = env_with("http://127.0.0.1:43119", &token_file);
        let error = remote_from_env(&env).unwrap_err();
        assert!(error.message.contains("32 lowercase hex"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn release_builds_refuse_remote() {
        assert!(enforce_dev_only(true).is_ok());
        let error = enforce_dev_only(false).unwrap_err();
        assert_eq!(error.code, codes::REMOTE_REQUIRES_DEV_BUILD);
    }
}
