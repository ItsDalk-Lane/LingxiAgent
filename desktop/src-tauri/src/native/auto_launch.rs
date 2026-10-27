//! R09-T03 开机自启动（`host:auto-launch`）。
//!
//! 现役 Electron（desktop/login-item-settings.cjs）：
//! - 支持平台：darwin（SMAppService/login item）、win32（Run 键 + `--hana-start-at-login`）；
//!   Linux 返回 `{supported:false, openAtLogin:false, openedAtLogin:false, status:"unsupported"}`；
//! - `openedAtLogin`：win 看 argv 是否含 `--hana-start-at-login`；mac 看
//!   `wasOpenedAtLogin || wasOpenedAsHidden`；
//! - set 后重新读取真实状态回显（不允许只回显请求值）。
//!
//! Tauri：`tauri-plugin-autostart`（Rust 侧 `ManagerExt`；mac 用
//! `MacosLauncher::LaunchAgent`，正式签名形态受 BLOCKED-SIGNING-IDENTITY 限制）。
//! 本模块只做归一/计划/调用窗口门，插件调用由 host-core 按注册清单接线。

use super::errors::HostResult;
use super::window_labels::{require_role, WindowRole};

pub const START_AT_LOGIN_ARG: &str = "--hana-start-at-login";

/// 自启动状态（与现役 IPC 返回同形）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoLaunchStatus {
    pub supported: bool,
    pub open_at_login: bool,
    pub opened_at_login: bool,
    pub status: Option<String>,
    pub executable_will_launch_at_login: Option<bool>,
}

impl AutoLaunchStatus {
    pub fn unsupported() -> Self {
        Self {
            supported: false,
            open_at_login: false,
            opened_at_login: false,
            status: Some("unsupported".to_owned()),
            executable_will_launch_at_login: None,
        }
    }
}

pub fn is_supported(platform: &str) -> bool {
    matches!(platform, "darwin" | "win32")
}

/// 归一真实系统状态。
pub fn normalize_status(
    platform: &str,
    argv: &[String],
    raw_open_at_login: Option<bool>,
    raw_status: Option<&str>,
    executable_will_launch_at_login: Option<bool>,
    mac_was_opened_at_login: bool,
    mac_was_opened_as_hidden: bool,
) -> AutoLaunchStatus {
    if !is_supported(platform) {
        return AutoLaunchStatus::unsupported();
    }
    let opened_at_login = if platform == "win32" {
        argv.iter().any(|arg| arg == START_AT_LOGIN_ARG)
    } else {
        mac_was_opened_at_login || mac_was_opened_as_hidden
    };
    AutoLaunchStatus {
        supported: true,
        open_at_login: raw_open_at_login == Some(true),
        opened_at_login,
        status: raw_status.map(str::to_owned),
        executable_will_launch_at_login,
    }
}

/// set 的动作计划：enable/disable 后必须重读真实状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetAutoLaunchPlan {
    /// None = 平台不支持，不调用插件（返回 unsupported 状态）。
    pub action: Option<bool>,
    pub re_read_after: bool,
    pub creates_launch_at_login_arg: bool,
}

pub fn plan_set_enabled(platform: &str, enabled: bool) -> SetAutoLaunchPlan {
    if !is_supported(platform) {
        return SetAutoLaunchPlan {
            action: None,
            re_read_after: false,
            creates_launch_at_login_arg: false,
        };
    }
    SetAutoLaunchPlan {
        action: Some(enabled),
        re_read_after: true,
        // Windows 登录项参数由插件写入；darwin 不需要 argv 参数。
        creates_launch_at_login_arg: platform == "win32" && enabled,
    }
}

/// 读状态：设置窗与主窗（设置可能以主窗 modal 呈现）都允许。
pub fn authorize_status_read(caller: &str) -> HostResult<()> {
    require_role(caller, &[WindowRole::Main, WindowRole::Settings]).map(|_| ())
}

/// 改状态：同上；其余窗口拒绝。
pub fn authorize_set_enabled(caller: &str, enabled: bool) -> HostResult<SetAutoLaunchPlan> {
    let _ = require_role(caller, &[WindowRole::Main, WindowRole::Settings])?;
    Ok(plan_set_enabled(platform_name(), enabled))
}

/// 宿主平台字面值（与 get_platform 一致）。
pub fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else {
        "linux"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv_with(arg: &str) -> Vec<String> {
        vec![
            "/Applications/Lingxi.app/Contents/MacOS/lingxi".to_string(),
            arg.to_string(),
        ]
    }

    #[test]
    fn platform_support_matches_incumbent() {
        assert!(is_supported("darwin"));
        assert!(is_supported("win32"));
        assert!(!is_supported("linux"));
        let status = normalize_status("linux", &[], None, None, None, false, false);
        assert_eq!(status, AutoLaunchStatus::unsupported());
        assert!(!status.supported);
        assert_eq!(status.status.as_deref(), Some("unsupported"));
    }

    #[test]
    fn opened_at_login_rules_per_platform() {
        // Windows: argv 标记
        let win = normalize_status(
            "win32",
            &argv_with(START_AT_LOGIN_ARG),
            Some(true),
            None,
            Some(true),
            false,
            false,
        );
        assert!(win.opened_at_login);
        let win_not = normalize_status(
            "win32",
            &argv_with("--dev"),
            Some(true),
            None,
            None,
            false,
            false,
        );
        assert!(!win_not.opened_at_login);
        assert!(win_not.open_at_login);

        // macOS: wasOpenedAtLogin / wasOpenedAsHidden
        let mac = normalize_status("darwin", &[], Some(true), None, None, true, false);
        assert!(mac.opened_at_login);
        let mac_hidden = normalize_status("darwin", &[], Some(true), None, None, false, true);
        assert!(mac_hidden.opened_at_login);
        let mac_no = normalize_status("darwin", &[], Some(true), None, None, false, false);
        assert!(!mac_no.opened_at_login);
    }

    #[test]
    fn status_fields_are_normalized_not_echoed() {
        let status = normalize_status(
            "darwin",
            &[],
            Some(true),
            Some("enabled"),
            Some(false),
            false,
            false,
        );
        assert!(status.open_at_login);
        assert_eq!(status.status.as_deref(), Some("enabled"));
        assert_eq!(status.executable_will_launch_at_login, Some(false));
        let status = normalize_status("darwin", &[], None, None, None, false, false);
        assert!(!status.open_at_login, "缺失 openAtLogin 视为 false");
        assert_eq!(status.status, None);
        assert_eq!(status.executable_will_launch_at_login, None);
    }

    #[test]
    fn set_plan_requires_reread_and_platform_gate() {
        let enable = plan_set_enabled("darwin", true);
        assert_eq!(enable.action, Some(true));
        assert!(enable.re_read_after);
        assert!(!enable.creates_launch_at_login_arg);
        let win_enable = plan_set_enabled("win32", true);
        assert!(win_enable.creates_launch_at_login_arg);
        let disable = plan_set_enabled("win32", false);
        assert_eq!(disable.action, Some(false));
        assert!(!disable.creates_launch_at_login_arg);
        let linux = plan_set_enabled("linux", true);
        assert_eq!(linux.action, None);
        assert!(!linux.re_read_after);
    }

    #[test]
    fn caller_policy_is_main_or_settings() {
        assert!(authorize_status_read("main").is_ok());
        assert!(authorize_status_read("settings").is_ok());
        for caller in ["quick-chat", "viewer-1", "splash"] {
            assert_eq!(
                authorize_status_read(caller).unwrap_err().code(),
                crate::native::errors::E_WINDOW_NOT_ALLOWED
            );
            assert_eq!(
                authorize_set_enabled(caller, true).unwrap_err().code(),
                crate::native::errors::E_WINDOW_NOT_ALLOWED
            );
        }
        assert!(authorize_set_enabled("settings", true).is_ok());
        assert_eq!(
            platform_name(),
            if cfg!(target_os = "macos") {
                "darwin"
            } else if cfg!(target_os = "windows") {
                "win32"
            } else {
                "linux"
            }
        );
    }
}
