//! R09-T03 主窗重载 / 应用重启（`host:reload-restart`）。
//!
//! 现役 Electron：
//! - `reload-main-window`（main.cjs:6531）：只 reload 主窗；
//! - `app:restart` / `restartApp()`（main.cjs:3055）：置 `isExitingServer`/
//!   `isQuitting` 后 `app.relaunch()+app.quit()`，走与托盘退出相同的清理路径，
//!   不把 owned server 晾成孤儿；更新安装中（`_isUpdating`）由 updater 自己负责。
//!
//! Tauri 等价：`WebviewWindow::reload()` 只作用于调用窗口（需主窗门）；
//! 重启由 Rust 内部调度（复用 T02 退出清理 + `AppHandle::restart()`），
//! **不接受 renderer 传参**（argv/env 都不能由渲染层注入）。

use super::errors::{deny, HostResult, E_CONFLICT, E_WINDOW_NOT_ALLOWED};
use super::window_labels::{require_role, WindowRole};

/// reload 只允许主窗（现役 handler 取 mainWindow 而非 sender 窗口）。
pub fn authorize_reload(caller: &str) -> HostResult<()> {
    require_role(caller, &[WindowRole::Main]).map(|_| ())
}

/// 重启请求的来源窗口：主窗 app-init / 设置窗 AccessTab（现役消费者）。
pub fn authorize_restart(caller: &str) -> HostResult<RestartPlan> {
    let role = require_role(caller, &[WindowRole::Main, WindowRole::Settings])?;
    Ok(RestartPlan {
        caller_role: role,
        // 重启不携带任何 renderer 参数：argv/env 由宿主进程自身决定。
        accepts_arguments: false,
        reuse_quit_cleanup: true,
        reentry_guard: true,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartPlan {
    pub caller_role: WindowRole,
    pub accepts_arguments: bool,
    pub reuse_quit_cleanup: bool,
    pub reentry_guard: bool,
}

/// 进程级重启/退出互斥门（镜像现役 isQuitting/_isUpdating）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RestartGuard {
    pub quitting: bool,
    pub updating: bool,
    pub restart_in_flight: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    /// 允许进入「标记退出 → 清理 owned server → relaunch」路径。
    Proceed,
}

pub fn begin_restart(guard: RestartGuard) -> HostResult<RestartDecision> {
    if guard.updating {
        return Err(deny(
            E_CONFLICT,
            "更新安装进行中：由 updater 负责重启，拒绝 renderer 重入",
        ));
    }
    if guard.quitting || guard.restart_in_flight {
        return Err(deny(E_CONFLICT, "退出/重启已在进行中"));
    }
    Ok(RestartDecision::Proceed)
}

/// 拒绝 renderer 注入重启参数（`--args`、env 覆盖、可执行路径）。
pub fn reject_restart_arguments(
    argv_override: Option<&str>,
    env_override: Option<&str>,
) -> HostResult<()> {
    if argv_override.is_some() {
        return Err(deny(
            E_WINDOW_NOT_ALLOWED,
            "重启命令不接受 renderer 提供 argv",
        ));
    }
    if env_override.is_some() {
        return Err(deny(
            E_WINDOW_NOT_ALLOWED,
            "重启命令不接受 renderer 提供环境变量",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_is_main_only() {
        assert!(authorize_reload("main").is_ok());
        for caller in ["settings", "quick-chat", "viewer-1", "splash"] {
            assert_eq!(
                authorize_reload(caller).unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED,
                "{caller} 不允许 reload 主窗"
            );
        }
        assert_eq!(
            authorize_reload("main window").unwrap_err().code(),
            crate::native::errors::E_WINDOW_LABEL_INVALID
        );
    }

    #[test]
    fn restart_allowed_from_main_and_settings_only() {
        for caller in ["main", "settings"] {
            let plan = authorize_restart(caller).unwrap();
            assert!(!plan.accepts_arguments);
            assert!(plan.reuse_quit_cleanup);
            assert!(plan.reentry_guard);
        }
        for caller in ["quick-chat", "viewer-1", "browser-viewer", "onboarding"] {
            assert_eq!(
                authorize_restart(caller).unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
    }

    #[test]
    fn restart_guard_blocks_reentry_and_updates() {
        assert_eq!(
            begin_restart(RestartGuard::default()).unwrap(),
            RestartDecision::Proceed
        );
        for guard in [
            RestartGuard {
                quitting: true,
                ..Default::default()
            },
            RestartGuard {
                updating: true,
                ..Default::default()
            },
            RestartGuard {
                restart_in_flight: true,
                ..Default::default()
            },
        ] {
            assert_eq!(
                begin_restart(guard).unwrap_err().code(),
                E_CONFLICT,
                "{guard:?} 必须拒绝重入"
            );
        }
    }

    #[test]
    fn restart_arguments_are_rejected() {
        assert!(reject_restart_arguments(None, None).is_ok());
        assert_eq!(
            reject_restart_arguments(Some("--dev"), None)
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED
        );
        assert_eq!(
            reject_restart_arguments(None, Some("LINGXI_HOME=/tmp/x"))
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED
        );
    }
}
