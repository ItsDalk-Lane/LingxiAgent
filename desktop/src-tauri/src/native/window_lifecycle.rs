//! R09-T03 主窗/应用生命周期（`host:main-window` + `host:app-lifecycle-quit`）。
//!
//! 现役 Electron 语义（desktop/main.cjs:3129-3297、6824-7003）：
//! - 主窗 close：非退出/更新中 → `preventDefault()` + `hide()`，同时隐藏
//!   settings / browser-viewer / quick-chat / 派生 viewer，保留 Dock 图标；
//! - `window-all-closed`：**托盘存在则常驻**，托盘缺失（创建失败/未初始化）才退出；
//! - `activate`（mac dock 点击）：无窗口且有服务端口 → 重建主窗（不 show）；
//!   否则 show 主窗；
//! - 彻底退出：先 hide 所有窗 → 关闭浏览器 tab → 只清理 owned/reused-owned
//!   服务，远端/外部 server 零接触 → 写残留诊断；
//! - before-quit 幂等：更新中（auto-updater 已完成清理）直接放行。
//!
//! 本模块只给策略/顺序，不持有进程：真正的 supervisor 清理属 R09-T02
//! （host-core 的 service/shutdown.rs），这里输出「该做什么」供其复用。

use super::errors::{deny, HostResult, E_CONFLICT, E_WINDOW_NOT_ALLOWED};
use super::window_labels::{require_role, WindowRole};

/// 主窗 close 的处置：隐藏保持 vs 真正销毁关闭。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDisposition {
    Hide,
    AllowClose,
}

/// 触发 close 时的进程级状态（与 Electron 的 isQuitting/_isUpdating/forceQuitApp 对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LifecycleFlags {
    /// 已进入退出流程（托盘退出/Cmd+Q/OS 登出）。
    pub quitting: bool,
    /// auto-updater 正在安装更新（它自己负责 server 清理）。
    pub updating: bool,
    /// 托盘不可用时 window-all-closed 的强制退出标记。
    pub force_quit: bool,
}

pub fn main_close_disposition(flags: LifecycleFlags, _tray_available: bool) -> CloseDisposition {
    if flags.quitting || flags.updating || flags.force_quit {
        CloseDisposition::AllowClose
    } else {
        // macOS 风格：关闭按钮只隐藏，Dock 保留黑点；Windows/Linux 同样
        // 由托盘常驻策略接管（现役实现三平台一致）。
        CloseDisposition::Hide
    }
}

/// 触发退出的来源（用于把 renderer 请求从「进程退出」里拆出来）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitSource {
    /// 托盘菜单退出。
    TrayQuit,
    /// 应用菜单 / Cmd+Q。
    AppMenuQuit,
    /// OS 登出/关机。
    OsLogout,
    /// 渲染层请求重启（restart_app / app:restart）。
    RendererRestart,
    /// 渲染层请求直接退出（现役没有该 IPC，必须拒绝）。
    RendererQuit,
}

/// 退出来源的调用窗口白名单。现役消费者：
/// - `restartApp`：设置窗 AccessTab（settings）、主窗 app-init（main）；
/// - 托盘退出：Rust 内部，无 renderer；
/// - renderer 直接 quit：不存在，拒绝。
pub fn validate_quit_request(source: QuitSource, caller: Option<&str>) -> HostResult<()> {
    match source {
        QuitSource::TrayQuit | QuitSource::AppMenuQuit | QuitSource::OsLogout => Ok(()),
        QuitSource::RendererRestart => {
            let caller =
                caller.ok_or_else(|| deny(E_WINDOW_NOT_ALLOWED, "重启请求缺少调用窗口身份"))?;
            require_role(caller, &[WindowRole::Main, WindowRole::Settings]).map(|_| ())
        }
        QuitSource::RendererQuit => Err(deny(
            E_WINDOW_NOT_ALLOWED,
            "renderer 不允许直接退出应用（只能经托盘/菜单/OS 登出）",
        )),
    }
}

/// 服务所有权分类（R09-T02 输出给本模块；这里不接触进程）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceOwner {
    /// 本次启动 spawn 的服务：退出时必须按 SIGTERM→KILL 梯度关闭。
    Owned,
    /// 复用到的本地服务且确认由本客户端持有（reused-owned）：退出时关闭。
    ReusedOwned,
    /// 复用到的外部/他进程服务（external reused）：零接触。
    ReusedExternal,
    /// 远端服务：零接触。
    Remote,
    NotStarted,
}

impl ServiceOwner {
    /// 是否允许终止该服务进程（A04：远端/外部复用零接触）。
    pub fn may_terminate(self) -> bool {
        matches!(self, ServiceOwner::Owned | ServiceOwner::ReusedOwned)
    }

    /// 退出后是否应保留 server-info.json（现役：未确认清理时保留供下次识别）。
    pub fn retains_server_info_on_failure(self) -> bool {
        matches!(
            self,
            ServiceOwner::ReusedOwned | ServiceOwner::ReusedExternal
        )
    }
}

/// 彻底退出时的动作清单（顺序即执行顺序；由 host-core 映射到 T02 的 shutdown）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuitPlan {
    pub hide_all_windows: bool,
    pub close_browser_tabs: bool,
    pub unregister_global_shortcuts: bool,
    pub dispose_keep_awake: bool,
    pub dispose_permission_bridge: bool,
    pub destroy_tray: bool,
    pub terminate_service: bool,
    pub write_residual_diagnostics: bool,
}

pub fn quit_plan(owner: ServiceOwner) -> QuitPlan {
    QuitPlan {
        hide_all_windows: true,
        close_browser_tabs: true,
        unregister_global_shortcuts: true,
        dispose_keep_awake: true,
        dispose_permission_bridge: true,
        destroy_tray: true,
        terminate_service: owner.may_terminate(),
        write_residual_diagnostics: true,
    }
}

/// before-quit 的幂等裁决（对应 Electron `resolveBeforeQuitServerAction` 语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeforeQuitAction {
    /// 更新中：auto-updater 已完成清理，直接放行。
    Allow,
    /// 正在异步清理：阻止本次 quit，清理完成后再次 quit。
    Wait,
    /// 首次进入：preventDefault 后开始清理。
    StartCleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BeforeQuitServerState {
    #[default]
    Idle,
    Running,
    Complete,
}

pub fn resolve_before_quit(
    state: BeforeQuitServerState,
    updating: bool,
    has_active_owned_server: bool,
) -> BeforeQuitAction {
    if updating {
        return BeforeQuitAction::Allow;
    }
    match state {
        BeforeQuitServerState::Complete => BeforeQuitAction::Allow,
        BeforeQuitServerState::Running => BeforeQuitAction::Wait,
        BeforeQuitServerState::Idle => {
            if has_active_owned_server {
                BeforeQuitAction::StartCleanup
            } else {
                BeforeQuitAction::Allow
            }
        }
    }
}

/// 主窗关闭时要一起隐藏的从属窗口（Electron: settings/browser-viewer/
/// quick-chat/viewer-*；skill-viewer 现役是 main 内 overlay，无独立窗口）。
pub fn windows_hidden_on_main_close(labels: &[String]) -> Vec<String> {
    labels
        .iter()
        .filter(|label| {
            let role = super::window_labels::role_for_label(label);
            matches!(
                role,
                WindowRole::Settings
                    | WindowRole::BrowserViewer
                    | WindowRole::QuickChat
                    | WindowRole::Viewer
            )
        })
        .cloned()
        .collect()
}

/// `window-all-closed` 的处置：托盘存在 → 常驻；否则退出（现役实现：
/// 只有托盘创建失败/未初始化时才退出，避免幽灵进程）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllClosedAction {
    StayResident,
    Quit,
}

pub fn window_all_closed_action(tray_available: bool) -> AllClosedAction {
    if tray_available {
        AllClosedAction::StayResident
    } else {
        AllClosedAction::Quit
    }
}

/// `activate`（mac Dock 点击 / 托盘双击）处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivateAction {
    /// 没有窗口但有服务端口：重建主窗（不立即 show，等 app_ready）。
    RecreateMain,
    /// 主窗存在：显示并聚焦。
    ShowMain,
    /// 服务尚未就绪：忽略（避免白屏/抢跑）。
    Noop,
}

pub fn activate_action(
    window_count: usize,
    has_main_window: bool,
    service_ready: bool,
) -> ActivateAction {
    if window_count == 0 {
        if service_ready {
            ActivateAction::RecreateMain
        } else {
            ActivateAction::Noop
        }
    } else if has_main_window {
        ActivateAction::ShowMain
    } else {
        ActivateAction::Noop
    }
}

/// 退出清理顺序（诊断/证据按此顺序断言，不依赖实现细节）。
pub const CLEANUP_ORDER: &[&str] = &[
    "hide-all-windows",
    "close-browser-tabs",
    "keep-awake-dispose",
    "permission-bridge-dispose",
    "global-shortcuts-unregister",
    "tray-destroy",
    "service-shutdown-sigterm",
    "service-shutdown-sigkill-after-grace",
    "server-info-cleanup",
    "residual-diagnostics",
];

/// 托盘/退出路径的冲突门：更新进行中不允许 renderer 重启（更新器自己重启）。
pub fn ensure_not_updating(updating: bool) -> HostResult<()> {
    if updating {
        return Err(super::errors::deny(
            E_CONFLICT,
            "更新安装进行中，禁止其他退出/重启路径重入",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_CONFLICT, E_WINDOW_NOT_ALLOWED};

    #[test]
    fn close_hides_unless_quitting_or_updating() {
        assert_eq!(
            main_close_disposition(LifecycleFlags::default(), true),
            CloseDisposition::Hide
        );
        assert_eq!(
            main_close_disposition(LifecycleFlags::default(), false),
            CloseDisposition::Hide,
            "托盘缺失时主窗 close 仍隐藏，由 window-all-closed 兜底退出"
        );
        for flags in [
            LifecycleFlags {
                quitting: true,
                ..Default::default()
            },
            LifecycleFlags {
                updating: true,
                ..Default::default()
            },
            LifecycleFlags {
                force_quit: true,
                ..Default::default()
            },
        ] {
            assert_eq!(
                main_close_disposition(flags, true),
                CloseDisposition::AllowClose
            );
        }
    }

    #[test]
    fn renderer_can_restart_only_from_main_or_settings() {
        assert!(validate_quit_request(QuitSource::RendererRestart, Some("main")).is_ok());
        assert!(validate_quit_request(QuitSource::RendererRestart, Some("settings")).is_ok());
        for caller in ["quick-chat", "viewer-1", "browser-viewer", "splash"] {
            assert_eq!(
                validate_quit_request(QuitSource::RendererRestart, Some(caller))
                    .unwrap_err()
                    .code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(
            validate_quit_request(QuitSource::RendererRestart, None)
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED
        );
        assert!(validate_quit_request(QuitSource::TrayQuit, None).is_ok());
        assert!(validate_quit_request(QuitSource::OsLogout, None).is_ok());
        // 渲染层直接退出不存在
        assert_eq!(
            validate_quit_request(QuitSource::RendererQuit, Some("main"))
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED
        );
    }

    #[test]
    fn service_ownership_decides_termination() {
        for owner in [ServiceOwner::Owned, ServiceOwner::ReusedOwned] {
            assert!(owner.may_terminate());
            assert!(quit_plan(owner).terminate_service);
        }
        for owner in [
            ServiceOwner::ReusedExternal,
            ServiceOwner::Remote,
            ServiceOwner::NotStarted,
        ] {
            assert!(!owner.may_terminate(), "远端/外部复用绝不能终止");
            assert!(!quit_plan(owner).terminate_service);
        }
        assert!(ServiceOwner::ReusedOwned.retains_server_info_on_failure());
        assert!(ServiceOwner::ReusedExternal.retains_server_info_on_failure());
        assert!(!ServiceOwner::Owned.retains_server_info_on_failure());
        let plan = quit_plan(ServiceOwner::Remote);
        assert!(plan.hide_all_windows);
        assert!(plan.unregister_global_shortcuts);
        assert!(!plan.terminate_service);
    }

    #[test]
    fn before_quit_is_idempotent_and_updating_short_circuits() {
        assert_eq!(
            resolve_before_quit(BeforeQuitServerState::Idle, true, true),
            BeforeQuitAction::Allow
        );
        assert_eq!(
            resolve_before_quit(BeforeQuitServerState::Idle, false, true),
            BeforeQuitAction::StartCleanup
        );
        assert_eq!(
            resolve_before_quit(BeforeQuitServerState::Idle, false, false),
            BeforeQuitAction::Allow
        );
        assert_eq!(
            resolve_before_quit(BeforeQuitServerState::Running, false, true),
            BeforeQuitAction::Wait
        );
        assert_eq!(
            resolve_before_quit(BeforeQuitServerState::Complete, false, true),
            BeforeQuitAction::Allow
        );
    }

    #[test]
    fn window_all_closed_respects_tray_availability() {
        assert_eq!(
            window_all_closed_action(true),
            AllClosedAction::StayResident
        );
        assert_eq!(window_all_closed_action(false), AllClosedAction::Quit);
    }

    #[test]
    fn activate_recreates_only_after_service_ready() {
        assert_eq!(
            activate_action(0, false, true),
            ActivateAction::RecreateMain
        );
        assert_eq!(activate_action(0, false, false), ActivateAction::Noop);
        assert_eq!(activate_action(2, true, true), ActivateAction::ShowMain);
        assert_eq!(activate_action(1, false, true), ActivateAction::Noop);
    }

    #[test]
    fn main_close_hides_only_secondary_windows() {
        let labels = [
            "main".to_string(),
            "settings".to_string(),
            "quick-chat".to_string(),
            "viewer-3".to_string(),
            "browser-viewer".to_string(),
            "skill-viewer-2".to_string(),
            "splash".to_string(),
            "onboarding".to_string(),
        ];
        let hidden = windows_hidden_on_main_close(&labels);
        assert_eq!(
            hidden,
            vec!["settings", "quick-chat", "viewer-3", "browser-viewer"]
        );
    }

    #[test]
    fn cleanup_order_is_stable_and_covers_service_shutdown() {
        assert_eq!(CLEANUP_ORDER[0], "hide-all-windows");
        assert!(CLEANUP_ORDER.contains(&"service-shutdown-sigterm"));
        assert!(CLEANUP_ORDER.contains(&"service-shutdown-sigkill-after-grace"));
        assert_eq!(CLEANUP_ORDER.last(), Some(&"residual-diagnostics"));
    }

    #[test]
    fn updating_blocks_restart_reentry() {
        assert!(ensure_not_updating(false).is_ok());
        assert_eq!(ensure_not_updating(true).unwrap_err().code(), E_CONFLICT);
    }
}
