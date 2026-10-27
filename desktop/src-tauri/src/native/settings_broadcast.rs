//! R09-T03 跨窗口设置广播（`host:settings-broadcast`，高风险项）。
//!
//! 现役 Electron（desktop/main.cjs:5948-6010）：renderer 调
//! `settingsChanged(type, data)` → 主进程转发给**除发送者以外**的主窗/设置窗/
//! browser-viewer，并执行真实副作用：
//! - `theme-changed`：解析具体主题并更新 browser-viewer 主题；
//! - `network-proxy-changed`：应用系统代理；
//! - `keep-awake-changed`：keep-awake manager 同步；
//! - `quick-chat-shortcut-changed`：重新注册全局快捷键；
//! - `locale-changed`：重置 i18n 并重建托盘菜单。
//!
//! 关键红线：**只转发不执行 side effect = 功能表面通过但系统状态不生效**
//! （A05 失败）。因此 Tauri 命令必须在 Rust 侧执行本模块输出的 side effects，
//! 再把事件 fanout 给其它窗口；事件名/载荷形状做白名单校验（A06）。
//!
//! 本模块用 `SettingsEventInput`（host-core 从 serde_json::Value 抽出的字段）
//! 保持纯逻辑、可单测；未知事件一律拒绝。

use super::errors::{deny, HostResult, E_ENUM_INVALID};
use super::window_labels::{role_for_label, WindowRole};

/// 事件白名单（从现役消费者逐一提取；新增事件必须同时更新这里与生产者断言）。
pub const EVENT_TYPES: &[&str] = &[
    "theme-changed",
    "keybindings-changed",
    "network-proxy-changed",
    "keep-awake-changed",
    "quick-chat-shortcut-changed",
    "quick-chat-preferences-changed",
    "locale-changed",
    "experiment-changed",
    "bridge-status-changed",
    "models-changed",
    "skills-changed",
    "editor-typography-changed",
    "chat-layout-changed",
    "hardware-acceleration-changed",
    "sidebar-ui-changed",
    "font-changed",
    "paper-texture-changed",
    "leaves-overlay-changed",
];

/// Rust 侧必须执行的副作用（不执行 = 假通过）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideEffect {
    /// 更新 browser-viewer 主题缓存（主题名已校验）。
    UpdateBrowserViewerTheme { theme: String },
    /// 应用系统代理（payload 名为 network_proxy 的对象；具体应用由 host-core）。
    ApplyNetworkProxy,
    /// keep-awake manager 同步布尔值。
    ApplyKeepAwake { enabled: bool },
    /// 重新注册 quick-chat 全局快捷键。
    ReregisterQuickChatShortcut,
    /// 重置 i18n 缓存并重建托盘菜单。
    RebuildTrayForLocale { locale: String },
}

/// host-core 从 serde_json::Value 抽出的字段集合（缺失为 None）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsEventInput<'a> {
    pub event: &'a str,
    /// theme-changed 的 theme（也是 locale 的 locale 字段来源之一）。
    pub theme: Option<&'a str>,
    /// keep-awake-changed 的 keep_awake。
    pub keep_awake: Option<bool>,
    /// locale-changed 的 locale。
    pub locale: Option<&'a str>,
    /// data 是否为对象（models/skills/experiment/bridge 等的基本形状门）。
    pub payload_is_object: bool,
    /// experiment-changed 的 id。
    pub experiment_id: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BroadcastPlan {
    pub event: &'static str,
    pub side_effects: Vec<SideEffect>,
    /// fanout 目标角色（发送者自身由 host-core 排除）。
    pub fanout_roles: Vec<WindowRole>,
}

fn event_static(raw: &str) -> HostResult<&'static str> {
    EVENT_TYPES
        .iter()
        .find(|event| **event == raw)
        .copied()
        .ok_or_else(|| {
            deny(
                E_ENUM_INVALID,
                format!("settings-changed 事件不在白名单: {raw}"),
            )
        })
}

fn require_object(input: &SettingsEventInput) -> HostResult<()> {
    if !input.payload_is_object {
        return Err(deny(E_INVALID_PAYLOAD, "该设置事件的 data 必须是对象"));
    }
    Ok(())
}

use super::errors::E_INVALID_ARGUMENT as E_INVALID_PAYLOAD;

/// 校验事件 + 产出副作用/fanout 计划。
pub fn plan_broadcast(input: &SettingsEventInput) -> HostResult<BroadcastPlan> {
    let event = event_static(input.event)?;
    let mut side_effects = Vec::new();
    let mut fanout_roles = vec![WindowRole::Main, WindowRole::Settings];
    match event {
        "theme-changed" => {
            let theme = input
                .theme
                .ok_or_else(|| deny(E_INVALID_PAYLOAD, "theme-changed 缺少 theme"))?;
            if theme.len() > 64 || theme.chars().any(|c| c.is_control()) {
                return Err(deny(E_INVALID_PAYLOAD, "theme 形态非法"));
            }
            side_effects.push(SideEffect::UpdateBrowserViewerTheme {
                theme: theme.to_owned(),
            });
            fanout_roles.push(WindowRole::BrowserViewer);
        }
        "network-proxy-changed" => {
            require_object(input)?;
            side_effects.push(SideEffect::ApplyNetworkProxy);
        }
        "keep-awake-changed" => {
            let enabled = input.keep_awake.ok_or_else(|| {
                deny(
                    E_INVALID_PAYLOAD,
                    "keep-awake-changed 缺少 keep_awake 布尔值",
                )
            })?;
            side_effects.push(SideEffect::ApplyKeepAwake { enabled });
        }
        "quick-chat-shortcut-changed" => {
            side_effects.push(SideEffect::ReregisterQuickChatShortcut);
        }
        "locale-changed" => {
            let locale = input
                .locale
                .or(input.theme)
                .ok_or_else(|| deny(E_INVALID_PAYLOAD, "locale-changed 缺少 locale"))?;
            if locale.len() > 64 || locale.chars().any(|c| c.is_control()) {
                return Err(deny(E_INVALID_PAYLOAD, "locale 形态非法"));
            }
            side_effects.push(SideEffect::RebuildTrayForLocale {
                locale: locale.to_owned(),
            });
        }
        "experiment-changed" => {
            require_object(input)?;
            let id = input
                .experiment_id
                .ok_or_else(|| deny(E_INVALID_PAYLOAD, "experiment-changed 缺少 id"))?;
            if id.is_empty() || id.len() > 128 || id.chars().any(|c| c.is_control()) {
                return Err(deny(E_INVALID_PAYLOAD, "experiment id 形态非法"));
            }
        }
        "keybindings-changed" => {
            require_object(input)?;
        }
        "bridge-status-changed" | "models-changed" | "skills-changed" => {
            require_object(input)?;
        }
        "editor-typography-changed" | "chat-layout-changed" | "sidebar-ui-changed" => {
            require_object(input)?;
        }
        "hardware-acceleration-changed"
        | "font-changed"
        | "paper-texture-changed"
        | "leaves-overlay-changed"
        | "quick-chat-preferences-changed" => {
            require_object(input)?;
        }
        other => {
            return Err(deny(E_ENUM_INVALID, format!("未接线事件: {other}")));
        }
    }
    Ok(BroadcastPlan {
        event,
        side_effects,
        fanout_roles,
    })
}

/// 发送者窗口门：现役所有窗口都能发（preload 统一暴露）；T03 收紧到主窗/设置窗/
/// browser-viewer（真正有设置写入口的窗口），只读窗口不能伪造设置广播。
pub fn authorize_sender(caller: &str) -> HostResult<WindowRole> {
    super::window_labels::validate_label_syntax(caller)?;
    let role = role_for_label(caller);
    match role {
        WindowRole::Main | WindowRole::Settings | WindowRole::BrowserViewer => Ok(role),
        _ => Err(deny(
            super::errors::E_WINDOW_NOT_ALLOWED,
            "该窗口不允许发送设置变更广播",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_ENUM_INVALID, E_INVALID_ARGUMENT, E_WINDOW_NOT_ALLOWED};

    fn input(event: &str) -> SettingsEventInput<'_> {
        SettingsEventInput {
            event,
            payload_is_object: true,
            ..Default::default()
        }
    }

    #[test]
    fn every_whitelisted_event_plans() {
        for event in EVENT_TYPES {
            let mut value = input(event);
            value.theme = Some("midnight");
            value.locale = Some("ja");
            value.keep_awake = Some(true);
            value.experiment_id = Some("rust-core");
            let plan = plan_broadcast(&value)
                .unwrap_or_else(|error| panic!("白名单事件 {event} 应可规划，实际 {error}"));
            assert!(plan.fanout_roles.contains(&WindowRole::Main));
            assert!(plan.fanout_roles.contains(&WindowRole::Settings));
        }
    }

    #[test]
    fn unknown_events_are_rejected() {
        assert_eq!(
            plan_broadcast(&input("evil-event")).unwrap_err().code(),
            E_ENUM_INVALID
        );
        assert_eq!(
            plan_broadcast(&input("")).unwrap_err().code(),
            E_ENUM_INVALID
        );
    }

    #[test]
    fn required_payload_fields_are_enforced() {
        assert_eq!(
            plan_broadcast(&input("theme-changed")).unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            plan_broadcast(&input("keep-awake-changed"))
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            plan_broadcast(&input("experiment-changed"))
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
        let mut non_object = input("models-changed");
        non_object.payload_is_object = false;
        assert_eq!(
            plan_broadcast(&non_object).unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
    }

    #[test]
    fn side_effects_are_not_optional() {
        let mut theme = input("theme-changed");
        theme.theme = Some("midnight");
        let plan = plan_broadcast(&theme).unwrap();
        assert_eq!(
            plan.side_effects,
            vec![SideEffect::UpdateBrowserViewerTheme {
                theme: "midnight".to_string()
            }]
        );
        assert!(plan.fanout_roles.contains(&WindowRole::BrowserViewer));

        let mut keep_awake = input("keep-awake-changed");
        keep_awake.keep_awake = Some(true);
        assert_eq!(
            plan_broadcast(&keep_awake).unwrap().side_effects,
            vec![SideEffect::ApplyKeepAwake { enabled: true }]
        );

        let mut locale = input("locale-changed");
        locale.locale = Some("ko");
        assert_eq!(
            plan_broadcast(&locale).unwrap().side_effects,
            vec![SideEffect::RebuildTrayForLocale {
                locale: "ko".to_string()
            }]
        );

        assert_eq!(
            plan_broadcast(&input("network-proxy-changed"))
                .unwrap()
                .side_effects,
            vec![SideEffect::ApplyNetworkProxy]
        );
        assert_eq!(
            plan_broadcast(&input("quick-chat-shortcut-changed"))
                .unwrap()
                .side_effects,
            vec![SideEffect::ReregisterQuickChatShortcut]
        );
    }

    #[test]
    fn sender_policy() {
        for caller in ["main", "settings", "browser-viewer"] {
            assert!(authorize_sender(caller).is_ok());
        }
        for caller in ["quick-chat", "viewer-1", "skill-viewer-1", "splash"] {
            assert_eq!(
                authorize_sender(caller).unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(
            authorize_sender("evil *").unwrap_err().code(),
            crate::native::errors::E_WINDOW_LABEL_INVALID
        );
    }
}
