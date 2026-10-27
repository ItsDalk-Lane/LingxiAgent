//! R09-T03 窗口控制命令的调用方门（`host:window-controls`）。
//!
//! 现役 Electron（desktop/main.cjs:6605-6618）：`window-minimize/maximize/close/
//! is-maximized` 经 `BrowserWindow.fromWebContents(event.sender)` 只作用于**调用
//! 者自己的窗口**；渲染层不能指定 window id。Tauri 若直接暴露 `core:window`
//! 全家桶会把「操作任意窗口」变成前端能力（T06 收窄目标），因此 T03 用自定义
//! 命令 + label 门替代：命令接收注入的 `WebviewWindow`，不接收窗口 id 参数。
//!
//! 本模块只做门与动作语义，命令实现（minimize/is_maximized 等）在 host-core
//! 的 commands.rs（已注册同名命令）中调用这里的纯函数。

use super::errors::{deny, HostResult, E_WINDOW_NOT_ALLOWED};
use super::window_labels::{require_role, WindowRole};

/// 四个窗口控制动作（一对一映射现役 IPC）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowControlAction {
    Minimize,
    /// 现役语义：已最大化 → 还原，否则最大化（toggle）。
    MaximizeToggle,
    Close,
    QueryMaximized,
}

/// 窗口控制允许的调用窗口：主窗 + 设置窗（Windows/Linux 自绘标题栏）。
/// viewer/skill-viewer/browser-viewer/quick-chat/splash/onboarding 一律拒绝。
pub fn authorize_window_control(
    caller: &str,
    action: WindowControlAction,
) -> HostResult<WindowRole> {
    require_role(caller, &[WindowRole::Main, WindowRole::Settings]).map(|role| {
        // `close` 语义按窗口角色分流：主窗 close 必须走「隐藏到托盘」策略，
        // 不能由这里直接 destroy（否则会把常驻进程错杀）。返回角色让调用方
        // 决定调用 `window.close()` 还是 `window.hide()`。
        let _ = action;
        role
    })
}

/// close 动作的路由：主窗 → 交给宿主的 close 策略（隐藏），其它 → 真关闭。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseRoute {
    /// 主窗：触发 CloseRequested 策略（默认 hide）。
    LifecyclePolicy,
    /// 设置窗等：直接关闭窗口。
    DirectClose,
}

pub fn close_route(role: WindowRole) -> HostResult<CloseRoute> {
    match role {
        WindowRole::Main => Ok(CloseRoute::LifecyclePolicy),
        WindowRole::Settings => Ok(CloseRoute::DirectClose),
        _ => Err(deny(E_WINDOW_NOT_ALLOWED, "该窗口不允许触发窗口关闭动作")),
    }
}

/// 最大化 toggle 的目标动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaximizeOutcome {
    Maximize,
    Unmaximize,
}

pub fn toggle_outcome(is_maximized: bool) -> MaximizeOutcome {
    if is_maximized {
        MaximizeOutcome::Unmaximize
    } else {
        MaximizeOutcome::Maximize
    }
}

/// 最大化状态变化广播的事件名（现役 `window-maximized` / `window-unmaximized`）。
pub fn maximized_event_name(maximized: bool) -> &'static str {
    if maximized {
        "window-maximized"
    } else {
        "window-unmaximized"
    }
}

/// 命令参数面：渲染层不允许传窗口 id（任何非空 windowId 都是越权尝试）。
pub fn reject_window_id_argument(window_id: Option<&str>) -> HostResult<()> {
    match window_id {
        None => Ok(()),
        Some(_) => Err(deny(
            E_WINDOW_NOT_ALLOWED,
            "窗口控制命令不接受 renderer 指定窗口 id（只作用于调用者窗口）",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::E_WINDOW_NOT_ALLOWED;

    #[test]
    fn only_main_and_settings_may_control_windows() {
        for caller in ["main", "settings"] {
            for action in [
                WindowControlAction::Minimize,
                WindowControlAction::MaximizeToggle,
                WindowControlAction::Close,
                WindowControlAction::QueryMaximized,
            ] {
                assert!(
                    authorize_window_control(caller, action).is_ok(),
                    "{caller} 应允许 {action:?}"
                );
            }
        }
        for caller in [
            "quick-chat",
            "viewer-1",
            "browser-viewer",
            "skill-viewer-2",
            "splash",
            "onboarding",
        ] {
            assert_eq!(
                authorize_window_control(caller, WindowControlAction::Minimize)
                    .unwrap_err()
                    .code(),
                E_WINDOW_NOT_ALLOWED,
                "{caller} 不应获得窗口控制"
            );
        }
        assert_eq!(
            authorize_window_control("viewer-*", WindowControlAction::Close)
                .unwrap_err()
                .code(),
            crate::native::errors::E_WINDOW_LABEL_INVALID
        );
    }

    #[test]
    fn main_close_goes_through_lifecycle_policy() {
        assert_eq!(
            close_route(WindowRole::Main).unwrap(),
            CloseRoute::LifecyclePolicy
        );
        assert_eq!(
            close_route(WindowRole::Settings).unwrap(),
            CloseRoute::DirectClose
        );
        for role in [
            WindowRole::QuickChat,
            WindowRole::Viewer,
            WindowRole::BrowserViewer,
            WindowRole::SkillViewer,
            WindowRole::Splash,
            WindowRole::Onboarding,
            WindowRole::Unknown,
        ] {
            assert_eq!(
                close_route(role).unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED,
                "{role:?} 不允许直接关闭"
            );
        }
    }

    #[test]
    fn toggle_and_event_names_match_incumbent() {
        assert_eq!(toggle_outcome(false), MaximizeOutcome::Maximize);
        assert_eq!(toggle_outcome(true), MaximizeOutcome::Unmaximize);
        assert_eq!(maximized_event_name(true), "window-maximized");
        assert_eq!(maximized_event_name(false), "window-unmaximized");
    }

    #[test]
    fn renderer_supplied_window_id_is_rejected() {
        assert!(reject_window_id_argument(None).is_ok());
        assert_eq!(
            reject_window_id_argument(Some("main")).unwrap_err().code(),
            E_WINDOW_NOT_ALLOWED
        );
        assert_eq!(
            reject_window_id_argument(Some("viewer-1"))
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED
        );
    }
}
