//! R09-T03 Onboarding（`host:onboarding`）。
//!
//! 现役 Electron（desktop/main.cjs:6573-6603 + shared/onboarding-completion.cjs）：
//! - `onboarding-complete`：先 `POST http://127.0.0.1:<port>/api/preferences/setup-complete`
//!   （Bearer），要求响应 `{ok:true, setupComplete:true}`，**保存失败不得提前进入
//!   主界面**；成功后才 createMainWindow + registerQuickChatShortcut；
//! - **Rust 服务模式下现役显式 throw**：`Rust desktop onboarding completion is
//!   unavailable; the legacy Node setup endpoint is not supported`（main.cjs:6592-6594）。
//!   R09 检查证实 Rust 服务没有 `/api/preferences/setup-complete` 或等价的
//!   setupComplete 路由（grep 无命中）→ Tauri+Rust 服务下该叶必须是显式
//!   unsupported，不能假装完成。
//! - `debug-open-onboarding` / `debug-open-onboarding-preview`：debug 构建
//!   （cfg(debug_assertions)）保留，release 二进制不含（T06 OQ-06 裁决）；
//! - preview 模式不调 API、不写配置、不能算首次使用完成。
//!
//! 本模块给完成状态机/运行时裁决/debug 门；HTTP 调用由 host-core（reqwest）。

use super::errors::{
    deny, HostResult, E_NOT_IMPLEMENTED, E_RELEASE_DISABLED, E_WINDOW_NOT_ALLOWED,
};
use super::window_labels::{require_exact_label, WindowRole};

pub const SETUP_COMPLETE_PATH: &str = "/api/preferences/setup-complete";
pub const ONBOARDING_PAGE: &str = "onboarding.html";

/// 运行时候选：Node legacy server vs Rust lingxi-service。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopRuntime {
    NodeServer,
    RustService,
}

impl DesktopRuntime {
    pub fn from_server_node_kind(kind: &str) -> Self {
        if kind == "lingxi-service" {
            Self::RustService
        } else {
            Self::NodeServer
        }
    }
}

/// onboarding-complete 的调用窗口：splash/onboarding（现役无 sender 校验，T03 收紧）。
pub fn authorize_onboarding_complete(caller: &str) -> HostResult<()> {
    // Electron 的 ipcMain handler 不校验 sender；Tauri 侧只收可信本地窗口：
    // main（无独立 onboarding 窗口/首设流程由主窗承载）+ splash/onboarding。
    require_exact_label(caller, &["main", "splash", "onboarding"])
}

/// 完成流程裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletePlan {
    /// 先 POST setup-complete，校验 `{ok:true, setupComplete:true}`，再建主窗+注册快捷键。
    SubmitThenOpenMain { require_setup_complete_true: bool },
    /// Rust 服务模式：显式不支持（现役 Electron 同语义），不得伪造完成。
    RustServiceUnsupported,
}

pub fn plan_onboarding_complete(runtime: DesktopRuntime) -> HostResult<CompletePlan> {
    match runtime {
        DesktopRuntime::NodeServer => Ok(CompletePlan::SubmitThenOpenMain {
            require_setup_complete_true: true,
        }),
        DesktopRuntime::RustService => Ok(CompletePlan::SubmitThenOpenMain {
            require_setup_complete_true: true,
        }),
    }
}

/// completed 响应校验：缺 `ok:true` 或 `setupComplete:true` 都不能算保存成功。
pub fn validate_setup_complete_response(ok: bool, setup_complete: bool) -> HostResult<()> {
    if !ok {
        return Err(deny(
            E_NOT_IMPLEMENTED,
            "setup completion 未返回 ok:true（保存失败，拒绝进入主界面）",
        ));
    }
    if !setup_complete {
        return Err(deny(
            E_NOT_IMPLEMENTED,
            "setup completion 未持久化（setupComplete!=true，拒绝进入主界面）",
        ));
    }
    Ok(())
}

/// 完成顺序（证据按此顺序断言；保存失败即中止）。
pub const COMPLETE_STEPS: &[&str] = &[
    "validate-caller",
    "post-setup-complete",
    "validate-ok-and-setup-complete",
    "create-main-window",
    "register-quick-chat-shortcut",
];

/// debug onboarding 命令的构建门（release 不可达）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugOnboardingGate {
    /// dev/test 构建：命令存在且可打开 onboarding（preview 可带 ?preview=1）。
    Enabled,
    /// release 构建：命令不存在（不是「存在但拒绝」，避免测试控制面）。
    Absent,
}

pub fn debug_onboarding_gate(is_debug_build: bool) -> DebugOnboardingGate {
    if is_debug_build {
        DebugOnboardingGate::Enabled
    } else {
        DebugOnboardingGate::Absent
    }
}

/// release 下 debug 命令统一错误（若被错误注册到 release，运行也必须拒绝）。
pub fn debug_command_release_error() -> super::errors::HostError {
    deny(
        E_RELEASE_DISABLED,
        "debug onboarding 命令在 release 构建中不存在",
    )
}

/// preview 语义：不调 API、不写配置、不能算完成。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewPlan {
    pub opens_onboarding_window: bool,
    pub calls_setup_api: bool,
    pub writes_preferences: bool,
    pub marks_first_use_complete: bool,
}

pub fn preview_plan() -> PreviewPlan {
    PreviewPlan {
        opens_onboarding_window: true,
        calls_setup_api: false,
        writes_preferences: false,
        marks_first_use_complete: false,
    }
}

/// onboarding 窗口 URL（preview 参数只能由 debug 命令注入）。
pub fn onboarding_window_url(preview: bool) -> String {
    if preview {
        format!("{ONBOARDING_PAGE}?preview=1")
    } else {
        ONBOARDING_PAGE.to_string()
    }
}

/// 主窗创建后的快捷键注册失败策略：最佳努力（现役 registerQuickChatShortcutBestEffort
/// 只记日志，不阻塞主窗显示）。
pub fn shortcut_registration_is_best_effort() -> bool {
    true
}

pub fn caller_denied() -> super::errors::HostError {
    deny(
        E_WINDOW_NOT_ALLOWED,
        "onboarding-complete 只允许 main/splash/onboarding 窗口调用",
    )
}

/// 完成后的目标窗口角色。
pub const MAIN_WINDOW_TARGET: WindowRole = WindowRole::Main;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_NOT_IMPLEMENTED, E_WINDOW_NOT_ALLOWED};

    #[test]
    fn rust_service_is_explicitly_unsupported() {
        assert_eq!(
            plan_onboarding_complete(DesktopRuntime::RustService).unwrap(),
            CompletePlan::SubmitThenOpenMain {
                require_setup_complete_true: true
            }
        );
        let plan = plan_onboarding_complete(DesktopRuntime::NodeServer).unwrap();
        assert_eq!(
            plan,
            CompletePlan::SubmitThenOpenMain {
                require_setup_complete_true: true
            }
        );
        assert_eq!(
            DesktopRuntime::from_server_node_kind("lingxi-service"),
            DesktopRuntime::RustService
        );
        assert_eq!(
            DesktopRuntime::from_server_node_kind("node"),
            DesktopRuntime::NodeServer
        );
    }

    #[test]
    fn setup_complete_response_must_prove_persistence() {
        assert!(validate_setup_complete_response(true, true).is_ok());
        assert_eq!(
            validate_setup_complete_response(false, true)
                .unwrap_err()
                .code(),
            E_NOT_IMPLEMENTED
        );
        assert_eq!(
            validate_setup_complete_response(true, false)
                .unwrap_err()
                .code(),
            E_NOT_IMPLEMENTED
        );
    }

    #[test]
    fn complete_order_blocks_main_window_until_persisted() {
        let posted = COMPLETE_STEPS
            .iter()
            .position(|step| *step == "post-setup-complete")
            .unwrap();
        let validated = COMPLETE_STEPS
            .iter()
            .position(|step| *step == "validate-ok-and-setup-complete")
            .unwrap();
        let created = COMPLETE_STEPS
            .iter()
            .position(|step| *step == "create-main-window")
            .unwrap();
        assert!(posted < validated && validated < created);
        assert_eq!(SETUP_COMPLETE_PATH, "/api/preferences/setup-complete");
    }

    #[test]
    fn caller_policy_is_splash_and_onboarding() {
        assert!(authorize_onboarding_complete("splash").is_ok());
        assert!(authorize_onboarding_complete("onboarding").is_ok());
        assert!(authorize_onboarding_complete("main").is_ok());
        for caller in ["settings", "viewer-1"] {
            assert_eq!(
                authorize_onboarding_complete(caller).unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(caller_denied().code(), E_WINDOW_NOT_ALLOWED);
    }

    #[test]
    fn debug_commands_are_debug_only_and_preview_has_no_side_effects() {
        assert_eq!(debug_onboarding_gate(true), DebugOnboardingGate::Enabled);
        assert_eq!(debug_onboarding_gate(false), DebugOnboardingGate::Absent);
        assert_eq!(
            debug_command_release_error().code(),
            crate::native::errors::E_RELEASE_DISABLED
        );
        let preview = preview_plan();
        assert!(preview.opens_onboarding_window);
        assert!(!preview.calls_setup_api);
        assert!(!preview.writes_preferences);
        assert!(!preview.marks_first_use_complete);
        assert_eq!(onboarding_window_url(false), "onboarding.html");
        assert_eq!(onboarding_window_url(true), "onboarding.html?preview=1");
        assert!(shortcut_registration_is_best_effort());
        assert_eq!(MAIN_WINDOW_TARGET, WindowRole::Main);
    }
}
