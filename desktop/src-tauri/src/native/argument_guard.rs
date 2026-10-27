//! R09-T03 IPC 参数护栏（A06）——所有自定义命令共享的类型/scope 校验原语。
//!
//! 设计约束（任务书 §4-3 / A06）：
//! - 畸形路径/URL/命令样式参数**按类型/scope 拒绝**，绝不转成任意 shell；
//! - 校验必须是纯逻辑、可单测、可在未注册宿主时独立运行（本文件不依赖 tauri）；
//! - 证据按稳定 `code` 断言（见 `errors.rs`），不靠错误文案。
//!
//! 注意：路径里出现 `$()`/`;`/`|` 等字符**不**等于注入——宿主从不经 shell 执行
//! 路径，正当文件名可以含这些字符。A06 真正要证明的是：相对路径、NUL/控制字符、
//! 超长参数、错误 scheme、窗口 id 注入、枚举外取值全部被拒绝；执行面只接受
//! 「绝对路径 + 期望的文件类型」，其余交系统 API（which 本身不接受 argv 拼接）。

use std::path::{Path, PathBuf};

use super::errors::{
    deny, HostResult, E_ENUM_INVALID, E_INVALID_ARGUMENT, E_NUMBER_INVALID, E_PATH_NOT_ABSOLUTE,
    E_URL_INVALID, E_URL_SCHEME_DENIED,
};

pub const MAX_PATH_LEN: usize = 4096;
pub const MAX_URL_LEN: usize = 8192;
pub const MAX_TEXT_ARG_LEN: usize = 64 * 1024;
pub const MAX_NOTIFICATION_TITLE_LEN: usize = 256;
pub const MAX_NOTIFICATION_BODY_LEN: usize = 4096;

fn has_control_or_nul(value: &str) -> bool {
    value.chars().any(|c| c.is_control())
}

/// 非空、无控制字符、长度受控的字符串参数（id/名称/枚举原文等）。
pub fn require_text<'a>(
    value: &'a str,
    field: &'static str,
    max_len: usize,
) -> HostResult<&'a str> {
    if value.is_empty() {
        return Err(deny(E_INVALID_ARGUMENT, format!("{field} 不能为空")));
    }
    if value.len() > max_len {
        return Err(deny(
            E_INVALID_ARGUMENT,
            format!("{field} 超过长度上限 {max_len}"),
        ));
    }
    if has_control_or_nul(value) {
        return Err(deny(E_INVALID_ARGUMENT, format!("{field} 含非法控制字符")));
    }
    Ok(value)
}

/// 允许空串的文本参数（通知 body 可为空），仍拒绝控制字符与超长。
pub fn require_optional_text<'a>(
    value: &'a str,
    field: &'static str,
    max_len: usize,
) -> HostResult<&'a str> {
    if value.len() > max_len {
        return Err(deny(
            E_INVALID_ARGUMENT,
            format!("{field} 超过长度上限 {max_len}"),
        ));
    }
    if has_control_or_nul(value) {
        return Err(deny(E_INVALID_ARGUMENT, format!("{field} 含非法控制字符")));
    }
    Ok(value)
}

/// 绝对路径校验（平台原生语义）：空串/相对路径/NUL/超长 → 拒绝。
///
/// 不做 `canonicalize`（那要求路径已存在，且会把不存在的合法目标路径拒绝掉）；
/// 存在性/类型由各命令在真正动作前用 `std::fs` 检查。
pub fn require_absolute_path(raw: &str, field: &'static str) -> HostResult<PathBuf> {
    if raw.is_empty() {
        return Err(deny(E_PATH_NOT_ABSOLUTE, format!("{field} 不能为空")));
    }
    if raw.len() > MAX_PATH_LEN {
        return Err(deny(
            E_PATH_NOT_ABSOLUTE,
            format!("{field} 超过路径长度上限 {MAX_PATH_LEN}"),
        ));
    }
    if has_control_or_nul(raw) {
        return Err(deny(E_PATH_NOT_ABSOLUTE, format!("{field} 含非法控制字符")));
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(deny(E_PATH_NOT_ABSOLUTE, format!("{field} 必须是绝对路径")));
    }
    Ok(path.to_path_buf())
}

/// 已存在路径的类型检查（文件/目录），不是期望类型即拒绝。
pub fn require_existing_path_of_kind(
    raw: &str,
    field: &'static str,
    expect_dir: bool,
) -> HostResult<PathBuf> {
    let path = require_absolute_path(raw, field)?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| {
        deny(
            super::errors::E_PATH_MISSING,
            format!("{field} 不存在或不可访问"),
        )
    })?;
    if metadata.file_type().is_symlink() {
        // 现役 open-folder/open-file 走 statSync（跟随符号链接）；但宿主写/删/读
        // 的安全边界一律拒绝符号链接（R06 files.rs 同规则）。此处只用于「打开」
        // 类动作，跟随链接是现役语义，故仅按目标类型判断。
        let followed = std::fs::metadata(&path).map_err(|_| {
            deny(
                super::errors::E_PATH_MISSING,
                format!("{field} 链接目标不可访问"),
            )
        })?;
        return ensure_kind(&path, &followed, field, expect_dir);
    }
    ensure_kind(&path, &metadata, field, expect_dir)
}

fn ensure_kind(
    path: &Path,
    metadata: &std::fs::Metadata,
    field: &'static str,
    expect_dir: bool,
) -> HostResult<PathBuf> {
    let matches = if expect_dir {
        metadata.is_dir()
    } else {
        metadata.is_file()
    };
    if !matches {
        return Err(deny(
            super::errors::E_PATH_TYPE_MISMATCH,
            format!(
                "{field} 必须是{}",
                if expect_dir { "目录" } else { "普通文件" }
            ),
        ));
    }
    Ok(path.to_path_buf())
}

/// http/https URL 的纯解析结果（不引入 url crate：命令层统一只用 scheme+host 判定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpUrl {
    pub scheme: &'static str,
    pub host: String,
    pub port: Option<u16>,
    pub raw: String,
}

/// 只接受 `http://` / `https://` 且 authority 非空的绝对 URL。
///
/// 显式拒绝（A06）：
/// - `javascript:`/`file:`/`data:`/`vbscript:` 等其它 scheme（`URL_SCHEME_DENIED`）；
/// - 无 `://`、authority 为空、含空白/控制字符、反斜杠（避免 Win 路径歧义）；
/// - 超长（> 8192）。
/// 现役 Electron `open-external` 只做 `new URL()` + scheme 判定；这里收紧到
/// 不允许空白/反斜杠（两者都不可能是合法可点击外链，且能绕过朴素前缀判断）。
pub fn parse_http_url(raw: &str) -> HostResult<HttpUrl> {
    if raw.is_empty() {
        return Err(deny(E_URL_INVALID, "URL 不能为空"));
    }
    if raw.len() > MAX_URL_LEN {
        return Err(deny(E_URL_INVALID, "URL 超过长度上限"));
    }
    if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(deny(E_URL_INVALID, "URL 含空白或控制字符"));
    }
    if raw.contains('\\') {
        return Err(deny(E_URL_INVALID, "URL 不允许反斜杠"));
    }
    let scheme_end = raw.find("://");
    let Some(scheme_end) = scheme_end else {
        // 无 authority 的形态（如 `javascript:alert(1)`、`mailto:x`）一律按
        // scheme 拒绝，不进入「解析失败」兜底。
        let scheme = raw.split(':').next().unwrap_or_default();
        if !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            return Err(deny(
                E_URL_SCHEME_DENIED,
                format!("不允许的 URL scheme: {scheme}"),
            ));
        }
        return Err(deny(E_URL_INVALID, "URL 缺少 scheme 分隔符"));
    };
    let scheme = &raw[..scheme_end];
    let scheme_lower = scheme.to_ascii_lowercase();
    let scheme_static: &'static str = match scheme_lower.as_str() {
        "http" => "http",
        "https" => "https",
        _ => {
            return Err(deny(
                E_URL_SCHEME_DENIED,
                format!("只允许 http/https，收到 {scheme}"),
            ))
        }
    };
    let authority_and_rest = &raw[scheme_end + 3..];
    let authority_end = authority_and_rest
        .find(['/', '?', '#'])
        .unwrap_or(authority_and_rest.len());
    let authority = &authority_and_rest[..authority_end];
    if authority.is_empty() {
        return Err(deny(E_URL_INVALID, "URL 缺少主机名"));
    }
    // 去除 userinfo（user:pass@host）只用于取 host；userinfo 本身允许（现役
    // Electron 允许带凭据的 http(s) 链接），但 host 必须非空。
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    if host_port.is_empty() {
        return Err(deny(E_URL_INVALID, "URL 缺少主机名"));
    }
    let (host, port) = split_host_port(host_port)?;
    if host.is_empty() {
        return Err(deny(E_URL_INVALID, "URL 缺少主机名"));
    }
    Ok(HttpUrl {
        scheme: scheme_static,
        host,
        port,
        raw: raw.to_owned(),
    })
}

fn split_host_port(host_port: &str) -> HostResult<(String, Option<u16>)> {
    // IPv6 literal: [::1]:8080
    if let Some(rest) = host_port.strip_prefix('[') {
        let Some((host, tail)) = rest.split_once(']') else {
            return Err(deny(E_URL_INVALID, "IPv6 主机缺少右括号"));
        };
        if host.is_empty() {
            return Err(deny(E_URL_INVALID, "URL 缺少主机名"));
        }
        let port = parse_port(tail)?;
        return Ok((host.to_owned(), port));
    }
    match host_port.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => Ok((host.to_owned(), parse_port(port)?)),
        _ => Ok((host_port.to_owned(), None)),
    }
}

fn parse_port(raw: &str) -> HostResult<Option<u16>> {
    let raw = raw.strip_prefix(':').unwrap_or(raw);
    if raw.is_empty() {
        return Err(deny(E_URL_INVALID, "URL 端口为空"));
    }
    let port: u16 = raw
        .parse()
        .map_err(|_| deny(E_URL_INVALID, "URL 端口非法"))?;
    if port == 0 {
        return Err(deny(E_URL_INVALID, "URL 端口不能为 0"));
    }
    Ok(Some(port))
}

/// 枚举白名单：返回白名单中的强类型值，未知取值拒绝。
pub fn require_enum<T: Copy>(
    raw: &str,
    field: &'static str,
    allowed: &[(&str, T)],
) -> HostResult<T> {
    allowed
        .iter()
        .find(|(name, _)| *name == raw)
        .map(|(_, value)| *value)
        .ok_or_else(|| {
            let names = allowed
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join("/");
            deny(
                E_ENUM_INVALID,
                format!("{field} 只允许 {names}，收到 {raw}"),
            )
        })
}

/// u32 范围校验（尺寸/数量/索引），拒绝负数与 NaN（f64 入参）。
pub fn require_u32_bounded(raw: f64, field: &'static str, min: u32, max: u32) -> HostResult<u32> {
    if !raw.is_finite() || raw.fract() != 0.0 || raw < 0.0 {
        return Err(deny(E_NUMBER_INVALID, format!("{field} 必须是整数")));
    }
    let value = raw as u32;
    if value < min || value > max {
        return Err(deny(
            E_NUMBER_INVALID,
            format!("{field} 必须在 [{min}, {max}] 内"),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{
        E_ENUM_INVALID, E_INVALID_ARGUMENT, E_NUMBER_INVALID, E_PATH_MISSING, E_PATH_TYPE_MISMATCH,
        E_URL_INVALID, E_URL_SCHEME_DENIED,
    };

    #[test]
    fn text_rejects_empty_control_and_overlong() {
        assert_eq!(
            require_text("", "title", 16).unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            require_text("bad\u{0}name", "title", 16)
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            require_text("new\nline", "title", 16).unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            require_text("0123456789abcdefg", "title", 16)
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            require_text("0123456789abcdef", "title", 16).unwrap(),
            "0123456789abcdef"
        );
        assert_eq!(require_text("正常名称", "title", 16).unwrap(), "正常名称");
        // 通知 body 允许空串
        assert_eq!(require_optional_text("", "body", 4096).unwrap(), "");
    }

    #[test]
    fn absolute_path_guard_accepts_posix_and_rejects_relative() {
        assert!(require_absolute_path("/tmp/x.txt", "path").is_ok());
        // 路径里允许 shell 元字符：宿主从不经 shell 执行，文件名本身合法。
        assert!(require_absolute_path("/tmp/$(rm -rf /).txt", "path").is_ok());
        assert!(require_absolute_path("/tmp/名 字.txt", "path").is_ok());
        for bad in ["", "relative/path", "./relative", "../up", "~/.lingxi"] {
            assert_eq!(
                require_absolute_path(bad, "path").unwrap_err().code(),
                E_PATH_NOT_ABSOLUTE,
                "应拒绝 {bad:?}"
            );
        }
        assert_eq!(
            require_absolute_path("/tmp/a\u{0}b", "path")
                .unwrap_err()
                .code(),
            E_PATH_NOT_ABSOLUTE
        );
        let too_long = format!("/{}", "a".repeat(MAX_PATH_LEN));
        assert_eq!(
            require_absolute_path(&too_long, "path").unwrap_err().code(),
            E_PATH_NOT_ABSOLUTE
        );
    }

    #[test]
    fn existing_path_kind_guard() {
        let temp = std::env::temp_dir();
        let file = std::env::current_exe().expect("current exe");
        assert!(require_existing_path_of_kind(file.to_str().unwrap(), "file", false).is_ok());
        assert_eq!(
            require_existing_path_of_kind(temp.to_str().unwrap(), "file", false)
                .unwrap_err()
                .code(),
            E_PATH_TYPE_MISMATCH
        );
        assert_eq!(
            require_existing_path_of_kind("/definitely/not/here-r09", "file", false)
                .unwrap_err()
                .code(),
            E_PATH_MISSING
        );
    }

    #[test]
    fn http_url_accepts_only_web_schemes() {
        assert_eq!(
            parse_http_url("https://example.com/a?b=1").unwrap().host,
            "example.com"
        );
        assert_eq!(
            parse_http_url("http://127.0.0.1:8080/x").unwrap().port,
            Some(8080)
        );
        assert_eq!(
            parse_http_url("HTTP://Example.COM/").unwrap().scheme,
            "http"
        );
        assert_eq!(
            parse_http_url("http://user:pass@example.com/")
                .unwrap()
                .host,
            "example.com"
        );
        assert_eq!(parse_http_url("http://[::1]:9000/").unwrap().host, "::1");
    }

    #[test]
    fn http_url_denies_dangerous_and_malformed() {
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,<script>",
            "vbscript:msgbox(1)",
            "tauri://localhost/",
        ] {
            assert_eq!(
                parse_http_url(bad).unwrap_err().code(),
                E_URL_SCHEME_DENIED,
                "应拒绝 scheme: {bad}"
            );
        }
        for bad in [
            "",
            "example.com/no-scheme",
            "http://",
            "http://:8080/",
            "https://exa mple.com/",
            "https://example.com\\@evil",
            "http://host:99999/",
            "http://host:0/",
        ] {
            let code = parse_http_url(bad).unwrap_err().code();
            assert!(
                code == E_URL_INVALID || code == E_URL_SCHEME_DENIED,
                "{bad:?} 必须以 URL_INVALID/URL_SCHEME_DENIED 拒绝，实际 {code}"
            );
        }
        let too_long = format!("https://example.com/{}", "a".repeat(MAX_URL_LEN));
        assert_eq!(parse_http_url(&too_long).unwrap_err().code(), E_URL_INVALID);
    }

    #[test]
    fn enum_whitelist() {
        #[derive(Clone, Copy, PartialEq, Debug)]
        enum Mode {
            Compact,
            Chat,
        }
        let allowed = [("compact", Mode::Compact), ("chat", Mode::Chat)];
        assert_eq!(require_enum("chat", "mode", &allowed).unwrap(), Mode::Chat);
        assert_eq!(
            require_enum("fullscreen", "mode", &allowed)
                .unwrap_err()
                .code(),
            E_ENUM_INVALID
        );
        assert_eq!(
            require_enum("", "mode", &allowed).unwrap_err().code(),
            E_ENUM_INVALID
        );
    }

    #[test]
    fn number_bounds() {
        assert_eq!(require_u32_bounded(3.0, "height", 1, 10).unwrap(), 3);
        for bad in [-1.0, 0.0, 11.0, 1.5, f64::NAN, f64::INFINITY] {
            assert_eq!(
                require_u32_bounded(bad, "height", 1, 10)
                    .unwrap_err()
                    .code(),
                E_NUMBER_INVALID,
                "应拒绝 {bad}"
            );
        }
    }
}
