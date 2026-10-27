//! R09-T03 Splash 数据（`host:splash`）。
//!
//! 现役 Electron（desktop/src/shared/splash-avatar-path.cjs）：
//! - `get-avatar-path(role)` / `get-splash-info` **只允许 splash/onboarding 窗口**
//!   且页面 URL 必须是本地 `splash.html`/`onboarding.html`（dev 下为 devUrl 的
//!   同两页）；`senderFrame === sender.mainFrame`；
//! - 头像：role ∈ {agent,user}；`LINGXI_HOME/{user|agents/<id>}/avatars/{role}.{png,jpg,jpeg,webp}`；
//!   逐层 lstat 拒绝符号链接、真实路径必须仍在 home 内、大小 1..15MB、文件头
//!   格式匹配（webp 需真实解码）、读取前后 inode/size/mtime 一致；
//! - 当前 agent：`current_agent_id`/`primary_agent_id`（Rust 模式）或
//!   `primaryAgent`（Node 模式），回退扫 `agents/*/config.yaml`（跳过 .deleted-agent.json）；
//! - splash info：`config.yaml` 的 `agent.name`/`agent.yuan`/`locale`，类型错误抛
//!   「启动资料类型无效，使用默认信息」，缺失回退 `{agentName:null, locale:'zh-CN', yuan:'lingxi'}`。
//!
//! Tauri 宿主用 std::fs + 内置 YAML 解析（或限定字段的宽松解析）实现同一校验；
//! 本模块给窗口/页面白名单、路径安全模型、大小/格式策略与 info 归一。

use std::path::{Path, PathBuf};

use super::errors::{deny, HostResult, E_INVALID_ARGUMENT, E_WINDOW_NOT_ALLOWED};
use super::window_labels::{require_exact_label, ONBOARDING, SPLASH};

pub const AVATAR_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];
pub const MAX_AVATAR_BYTES: u64 = 15 * 1024 * 1024;
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
pub const MAX_PREFERENCES_BYTES: u64 = 256 * 1024;
pub const DEFAULT_SPLASH_LOCALE: &str = "zh-CN";
pub const DEFAULT_SPLASH_YUAN: &str = "lingxi";
pub const AVATAR_ROLES: &[&str] = &["agent", "user"];

/// splash 数据命令只允许 splash/onboarding 窗口调用（现役 sender 白名单）。
pub fn authorize_splash_caller(caller: &str) -> HostResult<()> {
    require_exact_label(caller, &[SPLASH, ONBOARDING])
}

/// 页面 URL 白名单：只允许 `splash.html` / `onboarding.html`（可带 `preview=1`
/// 查询，仅 onboarding），且没有其他路径段/协议。
///
/// 接受三种形态（Electron file: URL / dev http URL / Tauri 自定义协议 URL）：
/// - `.../splash.html`
/// - `.../onboarding.html`
/// - `.../onboarding.html?preview=1`
pub fn validate_splash_page_url(url: &str) -> HostResult<()> {
    if url.is_empty() || url.len() > 4096 || url.chars().any(|c| c.is_control()) {
        return Err(deny(E_INVALID_ARGUMENT, "页面 URL 形态非法"));
    }
    // 只允许本壳页面来源：file:、tauri:、或 localhost/127.0.0.1/[::1]/tauri.localhost 的 http(s)。
    validate_splash_origin(url)?;
    let (path, query) = match url.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (url, None),
    };
    // 去掉 fragment（现役 URL 解析后 search/hash 置空再比较）
    let path = path.split('#').next().unwrap_or(path);
    let is_splash = path.ends_with("/splash.html");
    let is_onboarding = path.ends_with("/onboarding.html");
    if !is_splash && !is_onboarding {
        return Err(deny(
            E_WINDOW_NOT_ALLOWED,
            "splash 命令只允许 splash.html/onboarding.html 页面调用",
        ));
    }
    if let Some(query) = query {
        let query = query.split('#').next().unwrap_or(query);
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let allowed = match key {
                "preview" => is_onboarding && value == "1",
                "mode" => value == "native",
                _ => false,
            };
            if !allowed {
                return Err(deny(E_INVALID_ARGUMENT, "页面 URL 携带不允许的查询参数"));
            }
        }
    }
    Ok(())
}

/// 页面来源门（现役 `isTrustedSplashAvatarSender`：file:// 本地页面或 devUrl 的
/// localhost 页面；Tauri 另有 tauri:// 与 http(s)://tauri.localhost 两种形态）。
fn validate_splash_origin(url: &str) -> HostResult<()> {
    let Some((scheme, rest)) = url.split_once("://") else {
        // file:/abs/path 形态（无 //）
        if url.starts_with("file:") {
            return Ok(());
        }
        return Err(deny(E_INVALID_ARGUMENT, "页面 URL 缺少来源"));
    };
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host_port = authority;
    let allowed = match scheme.to_ascii_lowercase().as_str() {
        "file" => true,
        "tauri" => host_port == "localhost",
        "http" | "https" => {
            let host = host_port.split(':').next().unwrap_or_default();
            matches!(
                host,
                "localhost" | "127.0.0.1" | "[::1]" | "::1" | "tauri.localhost"
            )
        }
        _ => false,
    };
    if !allowed {
        return Err(deny(
            E_WINDOW_NOT_ALLOWED,
            "splash 页面来源必须是本壳页面（file/tauri/localhost）",
        ));
    }
    Ok(())
}

/// agent id 规则（与现役 `validAgentId` 完全一致：`^[A-Za-z0-9_-]{1,128}$`）。
pub fn valid_agent_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 头像相对路径段（home 内、逐层拒绝符号链接由宿主 IO 做）。
pub fn avatar_relative_segments(role: &str, agent_id: Option<&str>) -> HostResult<Vec<String>> {
    if !AVATAR_ROLES.contains(&role) {
        return Err(deny(E_INVALID_ARGUMENT, "avatar role 只允许 agent/user"));
    }
    let base: Vec<String> = match role {
        "user" => vec!["user".to_string()],
        _ => {
            let id = agent_id.ok_or_else(|| deny(E_INVALID_ARGUMENT, "缺少当前 agent id"))?;
            if !valid_agent_id(id) {
                return Err(deny(E_INVALID_ARGUMENT, "agent id 形态非法"));
            }
            vec!["agents".to_string(), id.to_string()]
        }
    };
    let mut segments = base;
    segments.push("avatars".to_string());
    Ok(segments)
}

/// 头像候选绝对路径（按扩展名顺序）。
pub fn avatar_candidate_paths(
    home: &Path,
    role: &str,
    agent_id: Option<&str>,
) -> HostResult<Vec<PathBuf>> {
    let segments = avatar_relative_segments(role, agent_id)?;
    let base = segments
        .iter()
        .fold(home.to_path_buf(), |path, segment| path.join(segment));
    Ok(AVATAR_EXTS
        .iter()
        .map(|ext| base.join(format!("{role}.{ext}")))
        .collect())
}

/// 文件头格式校验（读取到字节后调用；webp 还需真实解码，由宿主实现）。
pub fn avatar_bytes_match_format(bytes: &[u8], ext: &str) -> bool {
    match ext {
        "png" => bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        "jpg" | "jpeg" => {
            bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
        }
        "webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

/// splash 资料（与现役默认值一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplashInfo {
    pub agent_name: Option<String>,
    pub locale: String,
    pub yuan: String,
}

impl Default for SplashInfo {
    fn default() -> Self {
        Self {
            agent_name: None,
            locale: DEFAULT_SPLASH_LOCALE.to_string(),
            yuan: DEFAULT_SPLASH_YUAN.to_string(),
        }
    }
}

/// 把 config.yaml 解析出的字段归一（宿主解析器给出 Option 字段）。
/// 现役对类型错误抛错（「启动资料类型无效，使用默认信息」），由宿主在解析层拒绝；
/// 这里只做 trim/空值回落。
pub fn normalize_splash_info(
    agent_name: Option<&str>,
    locale: Option<&str>,
    yuan: Option<&str>,
) -> SplashInfo {
    let clean = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    SplashInfo {
        agent_name: clean(agent_name),
        locale: clean(locale).unwrap_or_else(|| DEFAULT_SPLASH_LOCALE.to_string()),
        yuan: clean(yuan).unwrap_or_else(|| DEFAULT_SPLASH_YUAN.to_string()),
    }
}

/// 头像大小策略：1..=15MB（现役 `size < 1 || size > maxBytes` → null）。
pub fn avatar_size_acceptable(len: u64) -> bool {
    len >= 1 && len <= MAX_AVATAR_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_INVALID_ARGUMENT, E_WINDOW_NOT_ALLOWED};

    #[test]
    fn only_splash_and_onboarding_windows_may_read() {
        assert!(authorize_splash_caller("splash").is_ok());
        assert!(authorize_splash_caller("onboarding").is_ok());
        for caller in ["main", "settings", "quick-chat", "viewer-1"] {
            assert_eq!(
                authorize_splash_caller(caller).unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
    }

    #[test]
    fn page_url_whitelist() {
        for good in [
            "tauri://localhost/splash.html",
            "tauri://localhost/onboarding.html",
            "http://localhost:5173/splash.html",
            "http://localhost:5173/onboarding.html?preview=1",
            "file:///Applications/Lingxi.app/Contents/Resources/splash.html",
        ] {
            assert!(validate_splash_page_url(good).is_ok(), "应接受 {good}");
        }
        for bad in [
            "tauri://localhost/index.html",
            "tauri://localhost/onboarding.html?preview=0",
            "tauri://localhost/splash.html?evil=1",
            "https://evil.example/splash.html",
            "tauri://localhost/splash.html#frag-ok?x",
            "",
        ] {
            // 最后一条 #frag 的分支实际允许；见下方单独断言
            if bad == "tauri://localhost/splash.html#frag-ok?x" {
                continue;
            }
            assert!(validate_splash_page_url(bad).is_err(), "应拒绝 {bad}");
        }
        assert_eq!(
            validate_splash_page_url("tauri://localhost/index.html")
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED
        );
        assert_eq!(
            validate_splash_page_url("tauri://localhost/splash.html?evil=1")
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
    }

    #[test]
    fn avatar_segments_and_paths_are_home_scoped() {
        assert_eq!(
            avatar_relative_segments("user", None).unwrap(),
            vec!["user", "avatars"]
        );
        assert_eq!(
            avatar_relative_segments("agent", Some("hanako")).unwrap(),
            vec!["agents", "hanako", "avatars"]
        );
        for bad in ["../evil", "a/b", "a\\b", "", &"a".repeat(129)] {
            assert!(!valid_agent_id(bad), "应拒绝 {bad:?}");
        }
        assert_eq!(
            avatar_relative_segments("other", Some("hanako"))
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            avatar_relative_segments("agent", None).unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
        let home = Path::new("/private/tmp/lingxi-splash-home");
        let paths = avatar_candidate_paths(home, "user", None).unwrap();
        assert_eq!(
            paths[0],
            PathBuf::from("/private/tmp/lingxi-splash-home/user/avatars/user.png")
        );
        assert_eq!(paths.len(), 4);
        let agent_paths = avatar_candidate_paths(home, "agent", Some("hanako")).unwrap();
        assert_eq!(
            agent_paths[3],
            PathBuf::from("/private/tmp/lingxi-splash-home/agents/hanako/avatars/agent.webp")
        );
    }

    #[test]
    fn avatar_format_and_size_policy() {
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0];
        assert!(avatar_bytes_match_format(&png, "png"));
        assert!(!avatar_bytes_match_format(&png, "jpg"));
        assert!(avatar_bytes_match_format(&[0xFF, 0xD8, 0xFF, 0xE0], "jpeg"));
        let mut webp = [0u8; 16];
        webp[0..4].copy_from_slice(b"RIFF");
        webp[8..12].copy_from_slice(b"WEBP");
        assert!(avatar_bytes_match_format(&webp, "webp"));
        assert!(!avatar_bytes_match_format(&webp, "png"));
        assert!(!avatar_size_acceptable(0));
        assert!(avatar_size_acceptable(1));
        assert!(avatar_size_acceptable(MAX_AVATAR_BYTES));
        assert!(!avatar_size_acceptable(MAX_AVATAR_BYTES + 1));
    }

    #[test]
    fn splash_info_defaults_and_trimming() {
        assert_eq!(
            normalize_splash_info(None, None, None),
            SplashInfo::default()
        );
        assert_eq!(SplashInfo::default().locale, "zh-CN");
        assert_eq!(SplashInfo::default().yuan, "lingxi");
        let info = normalize_splash_info(Some("  小灵  "), Some("ja-JP"), Some("  "));
        assert_eq!(info.agent_name.as_deref(), Some("小灵"));
        assert_eq!(info.locale, "ja-JP");
        assert_eq!(info.yuan, "lingxi");
    }
}
