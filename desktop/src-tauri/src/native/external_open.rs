//! R09-T03 外链 / 打开路径 / Finder 定位（`host:external-open`）。
//!
//! 现役 Electron（desktop/main.cjs:6229-6296、3237-3246、3358-3367）：
//! - `open-external`：`new URL(url)` 后只放行 http/https，其它返回
//!   `{ok:false,error:"只支持 HTTP 或 HTTPS 浏览器地址"}`；成功 `{ok:true}`；
//! - `open-folder`：绝对路径 + `statSync().isDirectory()`；
//! - `open-file`：绝对路径 + `isFile()`；
//! - `show-in-finder`：绝对路径（不要求存在）；
//! - 页面内 `will-navigate`：http/https 外链改走系统浏览器，其它忽略。
//!
//! Tauri：`open` crate 已有 `open::that_detached`（host-core commands.rs 已在用）；
//! Finder 定位用 objc2 `NSWorkspace selectFile:inFileViewerRootedAtPath:`（零新增插件，
//! host-core 决策）。本模块给 URL/路径校验与效果分类；系统调用由 host-core 执行。

use std::path::PathBuf;

use super::argument_guard::{parse_http_url, require_existing_path_of_kind, HttpUrl};
use super::errors::{HostResult, E_URL_SCHEME_DENIED};
use super::window_labels::{require_role, WindowRole};

/// 外链打开回执（与现役 IPC 返回同形）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenExternalReceipt {
    pub ok: bool,
    pub error: Option<String>,
}

impl OpenExternalReceipt {
    pub fn success() -> Self {
        Self {
            ok: true,
            error: None,
        }
    }

    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(error.into()),
        }
    }
}

/// 外链校验：仅 http/https（A06）。
pub fn validate_open_external(url: &str) -> HostResult<HttpUrl> {
    parse_http_url(url)
}

/// 调用窗口门：主窗/设置窗/quick-chat 都可能消费外链（Markdown 链接、About 页）。
/// 只读 viewer/skill-viewer/splash/onboarding 不允许（没有外链消费者）。
pub fn authorize_open_command(caller: &str) -> HostResult<WindowRole> {
    require_role(
        caller,
        &[
            WindowRole::Main,
            WindowRole::Settings,
            WindowRole::QuickChat,
        ],
    )
}

/// 命令效果分类（host-core 映射到 `open::that_detached` / NSWorkspace）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenEffect {
    /// 系统浏览器打开 http(s) URL。
    OpenUrl(String),
    /// 系统默认程序打开普通文件。
    OpenFile(PathBuf),
    /// 文件管理器打开目录。
    OpenFolder(PathBuf),
    /// 在文件管理器中定位（Finder `open -R` 等价）。
    RevealInDir(PathBuf),
}

pub fn plan_open_external(caller: &str, url: &str) -> HostResult<OpenEffect> {
    authorize_open_command(caller)?;
    let parsed = parse_http_url(url)?;
    Ok(OpenEffect::OpenUrl(parsed.raw))
}

pub fn plan_open_file(caller: &str, path: &str) -> HostResult<OpenEffect> {
    authorize_open_command(caller)?;
    let resolved = require_existing_path_of_kind(path, "file", false)?;
    Ok(OpenEffect::OpenFile(resolved))
}

pub fn plan_open_folder(caller: &str, path: &str) -> HostResult<OpenEffect> {
    authorize_open_command(caller)?;
    let resolved = require_existing_path_of_kind(path, "folder", true)?;
    Ok(OpenEffect::OpenFolder(resolved))
}

/// `show-in-finder` 现役只要求绝对路径（可以是不存在的路径，Finder 自己处理）。
pub fn plan_reveal_in_finder(caller: &str, path: &str) -> HostResult<OpenEffect> {
    authorize_open_command(caller)?;
    let resolved = super::argument_guard::require_absolute_path(path, "path")?;
    Ok(OpenEffect::RevealInDir(resolved))
}

/// 页面导航拦截（现役 will-navigate）：只有 http/https 外链交给系统浏览器；
/// 其它协议返回 None（不导航、不打开，避免 `file://`/`javascript:` 逃逸）。
pub fn navigation_external_target(url: &str) -> Option<String> {
    match parse_http_url(url) {
        Ok(parsed) => Some(parsed.raw),
        Err(error) if error.code() == E_URL_SCHEME_DENIED => None,
        Err(_) => None,
    }
}

/// 外链失败回执的错误文案（现役中文文案保持兼容）。
pub fn external_error(url: &str) -> String {
    match validate_open_external(url) {
        Ok(_) => String::new(),
        Err(error) => error.message().to_owned(),
    }
}

/// 命令名与回执形状的稳定性（host-core 注册清单引用）。
pub const OPEN_RECEIPT_SHAPE: &[&str] = &["ok", "error"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_PATH_MISSING, E_PATH_TYPE_MISMATCH, E_WINDOW_NOT_ALLOWED};

    #[test]
    fn external_open_only_web_schemes() {
        let effect = plan_open_external("main", "https://example.com/a").unwrap();
        assert_eq!(effect, OpenEffect::OpenUrl("https://example.com/a".into()));
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
            "mailto:a@b.c",
            "chrome://settings",
        ] {
            assert_eq!(
                validate_open_external(bad).unwrap_err().code(),
                E_URL_SCHEME_DENIED,
                "应拒绝 {bad}"
            );
            assert_eq!(navigation_external_target(bad), None);
        }
        assert_eq!(
            navigation_external_target("http://example.com/x").as_deref(),
            Some("http://example.com/x")
        );
    }

    #[test]
    fn open_file_and_folder_require_correct_kind() {
        let current = std::env::current_exe().unwrap();
        let file = plan_open_file("main", current.to_str().unwrap()).unwrap();
        assert!(matches!(file, OpenEffect::OpenFile(_)));
        let temp = std::env::temp_dir();
        let folder = plan_open_folder("settings", temp.to_str().unwrap()).unwrap();
        assert!(matches!(folder, OpenEffect::OpenFolder(_)));
        // 类型错配
        assert_eq!(
            plan_open_file("main", temp.to_str().unwrap())
                .unwrap_err()
                .code(),
            E_PATH_TYPE_MISMATCH
        );
        assert_eq!(
            plan_open_folder("main", current.to_str().unwrap())
                .unwrap_err()
                .code(),
            E_PATH_TYPE_MISMATCH
        );
        assert_eq!(
            plan_open_file("main", "/definitely/missing-r09")
                .unwrap_err()
                .code(),
            E_PATH_MISSING
        );
        assert_eq!(
            plan_open_file("main", "relative").unwrap_err().code(),
            crate::native::errors::E_PATH_NOT_ABSOLUTE
        );
    }

    #[test]
    fn reveal_requires_absolute_path_only() {
        let effect = plan_reveal_in_finder("main", "/Users/u/Desktop/x.txt").unwrap();
        assert_eq!(
            effect,
            OpenEffect::RevealInDir(PathBuf::from("/Users/u/Desktop/x.txt"))
        );
        assert_eq!(
            plan_reveal_in_finder("main", "x.txt").unwrap_err().code(),
            crate::native::errors::E_PATH_NOT_ABSOLUTE
        );
    }

    #[test]
    fn caller_policy_and_receipt_shape() {
        for caller in ["main", "settings", "quick-chat"] {
            assert!(plan_open_external(caller, "https://example.com").is_ok());
        }
        for caller in ["viewer-1", "skill-viewer-1", "splash", "onboarding"] {
            assert_eq!(
                plan_open_external(caller, "https://example.com")
                    .unwrap_err()
                    .code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(OpenExternalReceipt::success().ok, true);
        let failure = OpenExternalReceipt::failure("只支持 HTTP 或 HTTPS 浏览器地址");
        assert!(!failure.ok);
        assert!(failure.error.as_deref().unwrap().contains("HTTP"));
        assert_eq!(OPEN_RECEIPT_SHAPE, &["ok", "error"]);
        assert_eq!(
            external_error("javascript:alert(1)"),
            "不允许的 URL scheme: javascript"
        );
    }
}
