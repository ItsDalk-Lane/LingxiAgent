//! R09-T03 托盘模型（`host:tray`）。
//!
//! 现役 Electron（desktop/main.cjs:2354-2420、6709-6710）：
//! - 菜单固定 6 项（2 个分隔符）：显示/设置/—/修复组件…/—/退出；
//! - 右键重设上下文菜单、双击显示主窗（mac 用 template image）；
//! - 图标候选：mac/linux `tray[-dev]-template.png`，win 优先 `.ico` 回退 `.png`
//!   （资源缺失即抛错，不静默用空图标）；
//! - 文案随 main i18n locale 变化，locale-changed 时重建菜单；
//! - 托盘创建失败 → `window-all-closed` 走真退出（见 window_lifecycle）。
//!
//! **R09 与现役的显式差异（Lead 2026-10-26 裁决 3）**：现役第 4 项
//! 「修复组件…」调用 Electron 主进程的 `triggerArtifactRepairFlow()`
//! （`desktop/src/shared/artifact-repair.cjs`，Node fs/签名修复 + 重启）。Tauri 宿主
//! 没有等价实现（R08 产物修复流程未迁移，属 R09-T08/后续任务），按裁决**从托盘
//! 菜单移除该不可用项**，并在 `T03_T04_PROGRESS.md`/宿主能力矩阵登记「该原能力在
//! R09 缺失」——移除是显式登记后的可见差异，不是静默隐藏，也不以 NOT_IMPLEMENTED
//! 点击项冒充已迁移。
//!
//! Tauri：`tauri::tray` + `tauri::menu`（tauri feature `tray-icon`）。本模块只给
//! 菜单模型/图标候选/事件动作映射与幂等重建语义；`TrayIconBuilder` 接线由
//! host-core 按注册清单在 setup 执行。

use super::errors::{deny, HostResult, E_ENUM_INVALID, E_NOT_IMPLEMENTED};
use super::i18n::TrayStrings;

/// 菜单/托盘 id（Tauri MenuItem id；Electron 时代无 id，需显式以支持工具断言）。
pub const TRAY_ID: &str = "lingxi-tray";
pub const ID_SHOW: &str = "tray.show";
pub const ID_SETTINGS: &str = "tray.settings";
pub const ID_QUIT: &str = "tray.quit";

/// 菜单项动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ShowPrimary,
    OpenSettings,
    Quit,
}

/// 菜单模型（分隔符无 label/action）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayMenuItem {
    Action {
        id: &'static str,
        label: String,
        action: TrayAction,
    },
    Separator,
}

/// 按当前 locale 构建 5 项菜单：show/settings/—/quit。
///
/// 与现役 Electron 的差异：第 4 项「修复组件…」在 R09 缺失（无
/// `triggerArtifactRepairFlow` 等价），按 Lead 裁决移除并登记（见模块文档）。
pub fn build_menu_model(strings: &TrayStrings) -> Vec<TrayMenuItem> {
    vec![
        TrayMenuItem::Action {
            id: ID_SHOW,
            label: strings.show.to_owned(),
            action: TrayAction::ShowPrimary,
        },
        TrayMenuItem::Action {
            id: ID_SETTINGS,
            label: strings.settings.to_owned(),
            action: TrayAction::OpenSettings,
        },
        TrayMenuItem::Separator,
        TrayMenuItem::Action {
            id: ID_QUIT,
            label: strings.quit.to_owned(),
            action: TrayAction::Quit,
        },
    ]
}

/// 菜单事件 id → 动作；未知 id 返回 None（不 panic，不误触发）。
pub fn action_for_id(id: &str) -> Option<TrayAction> {
    match id {
        ID_SHOW => Some(TrayAction::ShowPrimary),
        ID_SETTINGS => Some(TrayAction::OpenSettings),
        ID_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

/// 托盘/菜单动作执行计划（host-core 按 effects 顺序执行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEffect {
    ShowPrimaryWindow,
    OpenSettingsWindow,
    BeginQuit { hide_all_windows_first: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayActionPlan {
    pub action: TrayAction,
    pub effects: &'static [TrayEffect],
}

pub fn plan_action(action: TrayAction) -> TrayActionPlan {
    // 静态切片保证计划稳定、可断言；顺序即执行顺序。
    const SHOW: &[TrayEffect] = &[TrayEffect::ShowPrimaryWindow];
    const SETTINGS: &[TrayEffect] = &[TrayEffect::OpenSettingsWindow];
    const QUIT: &[TrayEffect] = &[TrayEffect::BeginQuit {
        hide_all_windows_first: true,
    }];
    let effects = match action {
        TrayAction::ShowPrimary => SHOW,
        TrayAction::OpenSettings => SETTINGS,
        TrayAction::Quit => QUIT,
    };
    TrayActionPlan { action, effects }
}

/// 现役「修复组件…」在 R09 缺失的登记锚点（能力矩阵/BLOCKED 文档引用）。
pub const REMOVED_REPAIR_ARTIFACTS: &str =
    "tray.repairArtifacts 在 R09 缺失：electron triggerArtifactRepairFlow 无 Tauri 等价实现，     菜单项已按 Lead 裁决移除并登记（不冒充已迁移）";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayPlatform {
    Macos,
    Windows,
    Linux,
}

impl TrayPlatform {
    pub fn from_str(raw: &str) -> HostResult<Self> {
        match raw {
            "darwin" | "macos" => Ok(Self::Macos),
            "win32" | "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            _ => Err(deny(E_ENUM_INVALID, "未知平台不能决定托盘图标")),
        }
    }
}

/// 图标候选文件名（按尝试顺序；与实际 `desktop/src/assets/` 文件一致）。
pub fn icon_candidates(platform: TrayPlatform, is_dev: bool) -> Vec<&'static str> {
    match platform {
        TrayPlatform::Windows => {
            if is_dev {
                vec!["tray-dev.ico", "tray-dev-template.png"]
            } else {
                vec!["tray.ico", "tray-template.png"]
            }
        }
        _ => {
            if is_dev {
                vec!["tray-dev-template.png"]
            } else {
                vec!["tray-template.png"]
            }
        }
    }
}

/// 是否把图标设为 macOS template image（menu bar 自动适配深浅色）。
pub fn icon_as_template(platform: TrayPlatform) -> bool {
    matches!(platform, TrayPlatform::Macos)
}

/// 托盘 tooltip（现役：dev 后缀）。
pub fn tooltip(is_dev: bool) -> &'static str {
    if is_dev {
        "LingxiAgent (dev)"
    } else {
        "LingxiAgent"
    }
}

/// 创建失败必须显式向上传播（现役 throw；不能以空图标/无托盘继续），
/// 该失败会决定 `window-all-closed` 走真退出。
pub fn creation_failure(attempted: &[String]) -> super::errors::HostError {
    deny(
        E_NOT_IMPLEMENTED,
        format!(
            "托盘图标资源不可用（已尝试: {}）；拒绝以无托盘状态静默继续",
            attempted.join(", ")
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::i18n::tray_strings;

    #[test]
    fn menu_has_four_items_without_unsupported_repair() {
        let model = build_menu_model(&tray_strings("zh"));
        assert_eq!(model.len(), 4, "show/settings/separator/quit");
        match &model[0] {
            TrayMenuItem::Action { id, label, action } => {
                assert_eq!(*id, ID_SHOW);
                assert_eq!(label, "显示 灵犀");
                assert_eq!(*action, TrayAction::ShowPrimary);
            }
            _ => panic!("第一项必须是显示动作"),
        }
        assert_eq!(model[2], TrayMenuItem::Separator);
        match &model[3] {
            TrayMenuItem::Action { id, action, .. } => {
                assert_eq!(*id, ID_QUIT);
                assert_eq!(*action, TrayAction::Quit);
            }
            _ => panic!("最后一项必须是退出"),
        }
    }

    #[test]
    fn menu_labels_follow_locale() {
        let en = build_menu_model(&tray_strings("en"));
        match &en[0] {
            TrayMenuItem::Action { label, .. } => assert_eq!(label, "Show Lingxi"),
            _ => panic!(),
        }
        let ja = build_menu_model(&tray_strings("ja"));
        match &ja[3] {
            TrayMenuItem::Action { label, .. } => assert_eq!(label, "終了"),
            _ => panic!(),
        }
    }

    #[test]
    fn menu_event_ids_map_to_actions_and_unknown_is_none() {
        assert_eq!(action_for_id(ID_SHOW), Some(TrayAction::ShowPrimary));
        assert_eq!(action_for_id(ID_SETTINGS), Some(TrayAction::OpenSettings));
        assert_eq!(
            action_for_id("tray.repairArtifacts"),
            None,
            "已移除的修复组件项不得可达"
        );
        assert_eq!(action_for_id(ID_QUIT), Some(TrayAction::Quit));
        assert_eq!(action_for_id("tray.unknown"), None);
        assert_eq!(action_for_id(""), None);
    }

    #[test]
    fn actions_plan_effects() {
        assert_eq!(
            plan_action(TrayAction::ShowPrimary).effects,
            &[TrayEffect::ShowPrimaryWindow]
        );
        assert_eq!(
            plan_action(TrayAction::OpenSettings).effects,
            &[TrayEffect::OpenSettingsWindow]
        );
        assert_eq!(
            plan_action(TrayAction::Quit).effects,
            &[TrayEffect::BeginQuit {
                hide_all_windows_first: true
            }]
        );
        assert!(REMOVED_REPAIR_ARTIFACTS.contains("R09 缺失"));
    }

    #[test]
    fn icon_candidates_follow_platform_rules() {
        assert_eq!(
            icon_candidates(TrayPlatform::Macos, false),
            vec!["tray-template.png"]
        );
        assert_eq!(
            icon_candidates(TrayPlatform::Macos, true),
            vec!["tray-dev-template.png"]
        );
        assert_eq!(
            icon_candidates(TrayPlatform::Windows, false),
            vec!["tray.ico", "tray-template.png"]
        );
        assert_eq!(
            icon_candidates(TrayPlatform::Windows, true),
            vec!["tray-dev.ico", "tray-dev-template.png"]
        );
        assert_eq!(
            icon_candidates(TrayPlatform::Linux, false),
            vec!["tray-template.png"]
        );
        assert!(icon_as_template(TrayPlatform::Macos));
        assert!(!icon_as_template(TrayPlatform::Windows));
        assert!(!icon_as_template(TrayPlatform::Linux));
        assert_eq!(tooltip(false), "LingxiAgent");
        assert_eq!(tooltip(true), "LingxiAgent (dev)");
    }

    #[test]
    fn unknown_platform_is_rejected() {
        assert_eq!(
            TrayPlatform::from_str("freebsd").unwrap_err().code(),
            E_ENUM_INVALID
        );
        assert_eq!(
            TrayPlatform::from_str("darwin").unwrap(),
            TrayPlatform::Macos
        );
        assert_eq!(
            TrayPlatform::from_str("win32").unwrap(),
            TrayPlatform::Windows
        );
        assert_eq!(
            TrayPlatform::from_str("linux").unwrap(),
            TrayPlatform::Linux
        );
    }
}
