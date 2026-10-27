//! R09-T03 主窗 ready 时序（`host:app-ready`）。
//!
//! 现役 Electron（desktop/main.cjs:6620-6653 + createMainWindow:3181-3197）：
//! - `app-ready` 只接受主窗调用（现役缺失显式 sender 校验，T03 补上）；
//! - 幂等：重复调用再次 show，但只记录一次 `alreadyReady`；
//! - `startHiddenAtLogin`（登录自启）时不 show、不探测通知权限；
//! - mac 首启通知权限 `not-determined` 时发一条静默通知触发系统授权；
//! - 200ms 后关闭 splash/onboarding，让主窗先稳定显示；
//! - 30s 内没收到 app-ready → 强制 show 主窗（防白屏），写诊断事件。
//!
//! 本模块输出「该做什么」的效果清单；真正的 window.show()/close() 由
//! host-core 的 commands.rs / host.rs 执行。

use super::errors::HostResult;
use super::window_labels::{require_role, WindowRole};

/// 现役 30s 初始化兜底。
pub const APP_READY_TIMEOUT_MS: u64 = 30_000;
/// splash/onboarding 延迟关闭（现役 setTimeout 200ms）。
pub const SPLASH_CLOSE_DELAY_MS: u64 = 200;

/// 收到 app-ready 时要执行的效果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppReadyEffects {
    pub already_ready: bool,
    pub show_main_window: bool,
    pub probe_macos_notification_permission: bool,
    pub close_splash_after_ms: u64,
    pub close_onboarding_after_ms: u64,
    pub record_diagnostics: bool,
}

/// 校验调用方并计算效果。`start_hidden_at_login` 来自宿主启动参数
/// （登录项启动时后台准备，不抢前台）。
pub fn on_app_ready(
    caller: &str,
    already_ready: bool,
    start_hidden_at_login: bool,
) -> HostResult<AppReadyEffects> {
    require_role(caller, &[WindowRole::Main])?;
    Ok(AppReadyEffects {
        already_ready,
        show_main_window: !start_hidden_at_login,
        // 现役：仅 darwin + 未 hidden-at-login 时探测通知权限。
        probe_macos_notification_permission: cfg!(target_os = "macos") && !start_hidden_at_login,
        // 现役是「就绪后延迟关闭」；即使已 ready 也沿用同一延迟语义。
        close_splash_after_ms: SPLASH_CLOSE_DELAY_MS,
        close_onboarding_after_ms: SPLASH_CLOSE_DELAY_MS,
        record_diagnostics: true,
    })
}

/// 启动兜底超时的效果：hidden-at-login 时不强显（现役 `if (_startHiddenAtLogin) return;`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitTimeoutEffects {
    pub force_show: bool,
    pub record_diagnostics: bool,
}

pub fn init_timeout_effects(start_hidden_at_login: bool) -> InitTimeoutEffects {
    InitTimeoutEffects {
        force_show: !start_hidden_at_login,
        record_diagnostics: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::E_WINDOW_NOT_ALLOWED;

    #[test]
    fn only_main_window_may_signal_ready() {
        assert!(on_app_ready("main", false, false).is_ok());
        for caller in [
            "settings",
            "quick-chat",
            "viewer-1",
            "browser-viewer",
            "splash",
            "onboarding",
        ] {
            assert_eq!(
                on_app_ready(caller, false, false).unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED,
                "{caller} 不能冒充主窗 ready"
            );
        }
        assert_eq!(
            on_app_ready("main()", false, false).unwrap_err().code(),
            crate::native::errors::E_WINDOW_LABEL_INVALID
        );
    }

    #[test]
    fn ready_shows_main_and_schedules_splash_close() {
        let effects = on_app_ready("main", false, false).unwrap();
        assert!(effects.show_main_window);
        assert!(!effects.already_ready);
        assert_eq!(effects.close_splash_after_ms, 200);
        assert_eq!(effects.close_onboarding_after_ms, 200);
        assert!(effects.record_diagnostics);
        assert_eq!(
            effects.probe_macos_notification_permission,
            cfg!(target_os = "macos"),
            "只有 mac 首启探测通知权限"
        );
    }

    #[test]
    fn ready_is_idempotent_and_hidden_at_login_stays_hidden() {
        let effects = on_app_ready("main", true, false).unwrap();
        assert!(effects.already_ready);
        assert!(effects.show_main_window);

        let hidden = on_app_ready("main", false, true).unwrap();
        assert!(!hidden.show_main_window, "登录自启不抢前台");
        assert!(!hidden.probe_macos_notification_permission);
    }

    #[test]
    fn init_timeout_forces_show_unless_hidden_at_login() {
        let normal = init_timeout_effects(false);
        assert!(normal.force_show);
        assert!(normal.record_diagnostics);
        let hidden = init_timeout_effects(true);
        assert!(!hidden.force_show);
        assert_eq!(APP_READY_TIMEOUT_MS, 30_000);
    }
}
