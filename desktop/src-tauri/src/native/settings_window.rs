//! R09-T03 设置窗口路由（`host:settings-window`）。
//!
//! 现役 Electron（desktop/main.cjs:3299-3400、5734）：
//! - `open-settings` → `createSettingsWindow(tab)`：**主窗存在且未崩溃时** 先
//!   show/focus 主窗，再向主窗发 `open-settings-modal`（tab 缺省 "agent"）；
//! - 否则设置窗已存在则 focus + `settings-switch-tab`；再否则新建独立窗口
//!   （720x700，窗口 label 语义上是设置窗；主窗存在时还会同步主窗尺寸 90%）；
//! - `settings-switch-tab` 只发 tab 字符串。
//!
//! Tauri：主窗优先 modal 的可见行为必须保留（A13/T07 会对照 UI）。本模块给路由
//! 决策与事件载荷校验；窗口创建/焦点在 host-core。

use super::errors::{deny, HostResult, E_ENUM_INVALID, E_INVALID_ARGUMENT, E_WINDOW_NOT_ALLOWED};
use super::window_labels::{require_role, WindowRole};

pub const SETTINGS_WINDOW_WIDTH: u32 = 720;
pub const SETTINGS_WINDOW_HEIGHT: u32 = 700;
/// 主窗存在时独立设置窗尺寸取主窗 90%（现役 main.cjs 行为）。
pub const SETTINGS_MAIN_WIDTH_RATIO: f64 = 0.9;
pub const DEFAULT_TAB: &str = "agent";
pub const EVENT_OPEN_SETTINGS_MODAL: &str = "open-settings-modal";
pub const EVENT_SETTINGS_SWITCH_TAB: &str = "settings-switch-tab";

/// 设置页 tab 语法：小写字母开头，后接小写字母/数字/连字符，长度 ≤ 32。
/// 现役 `normalizeSettingsTab` 原样透传；宿主层只做语法门（不硬编码 tab 列表，
/// 避免设置页新增 tab 时宿主先拒绝），但必须拒绝空串/控制字符/超长/注入形态。
pub fn validate_tab(raw: &str) -> HostResult<&str> {
    if raw.is_empty() || raw.len() > 32 {
        return Err(deny(E_INVALID_ARGUMENT, "设置 tab 为空或超长"));
    }
    let mut chars = raw.chars();
    let first = chars.next().expect("非空已检查");
    if !first.is_ascii_lowercase() {
        return Err(deny(E_ENUM_INVALID, "设置 tab 必须以小写字母开头"));
    }
    if !raw
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(deny(E_ENUM_INVALID, "设置 tab 只允许小写字母/数字/连字符"));
    }
    Ok(raw)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsTarget {
    /// 主窗内 modal + 聚焦主窗。
    MainModal { tab: String },
    /// 独立设置窗；`create` 表示需要新建，`focus_existing` 表示已存在只需聚焦。
    SettingsWindow {
        tab: Option<String>,
        create: bool,
        focus_existing: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsRoute {
    pub target: SettingsTarget,
    pub focus_main: bool,
    pub event: &'static str,
}

/// 路由决策。`main_alive` 主窗存在；`main_crashed` 主窗 webview 崩溃（现役
/// `webContents.isCrashed()` 时也走独立窗）；`settings_alive` 独立设置窗存在。
pub fn plan_open_settings(
    caller: &str,
    raw_tab: Option<&str>,
    main_alive: bool,
    main_crashed: bool,
    settings_alive: bool,
) -> HostResult<SettingsRoute> {
    require_role(caller, &[WindowRole::Main, WindowRole::Settings])?;
    let tab = match raw_tab {
        Some(value) if !value.is_empty() => Some(validate_tab(value)?.to_owned()),
        _ => None,
    };
    Ok(route_for(tab, main_alive, main_crashed, settings_alive))
}

/// 托盘/宿主内部触发（无调用窗口，等价 Electron 托盘 open-settings 路径）。
pub fn plan_open_settings_internal(
    raw_tab: Option<&str>,
    main_alive: bool,
    main_crashed: bool,
    settings_alive: bool,
) -> HostResult<SettingsRoute> {
    let tab = match raw_tab {
        Some(value) if !value.is_empty() => Some(validate_tab(value)?.to_owned()),
        _ => None,
    };
    Ok(route_for(tab, main_alive, main_crashed, settings_alive))
}

fn route_for(
    tab: Option<String>,
    main_alive: bool,
    main_crashed: bool,
    settings_alive: bool,
) -> SettingsRoute {
    if main_alive && !main_crashed {
        return SettingsRoute {
            target: SettingsTarget::MainModal {
                tab: tab.unwrap_or_else(|| DEFAULT_TAB.to_owned()),
            },
            focus_main: true,
            event: EVENT_OPEN_SETTINGS_MODAL,
        };
    }
    if settings_alive {
        return SettingsRoute {
            target: SettingsTarget::SettingsWindow {
                tab,
                create: false,
                focus_existing: true,
            },
            focus_main: false,
            event: EVENT_SETTINGS_SWITCH_TAB,
        };
    }
    SettingsRoute {
        target: SettingsTarget::SettingsWindow {
            tab,
            create: true,
            focus_existing: false,
        },
        focus_main: false,
        event: EVENT_OPEN_SETTINGS_MODAL,
    }
}

/// 独立设置窗尺寸（主窗存活时按 90% 同步；否则固定 720x700）。
pub fn settings_window_size(main_size: Option<(u32, u32)>) -> (u32, u32) {
    match main_size {
        Some((width, height)) if width > 0 && height > 0 => (
            ((f64::from(width)) * SETTINGS_MAIN_WIDTH_RATIO).round() as u32,
            ((f64::from(height)) * SETTINGS_MAIN_WIDTH_RATIO).round() as u32,
        ),
        _ => (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT),
    }
}

/// 设置窗创建/加载的页面（与 React 产物同一 settings.html 入口）。
pub const SETTINGS_WINDOW_PAGE: &str = "settings.html";

/// 跨窗切页事件载荷：只允许 tab 字符串。
pub fn validate_switch_tab_payload(raw: &str) -> HostResult<()> {
    validate_tab(raw).map(|_| ())
}

pub fn caller_denied() -> super::errors::HostError {
    deny(
        E_WINDOW_NOT_ALLOWED,
        "打开设置只允许主窗/设置窗或宿主内部（托盘）触发",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_modal_preferred_when_main_alive() {
        let route = plan_open_settings("main", None, true, false, false).unwrap();
        assert_eq!(
            route.target,
            SettingsTarget::MainModal {
                tab: "agent".to_string()
            }
        );
        assert!(route.focus_main);
        assert_eq!(route.event, EVENT_OPEN_SETTINGS_MODAL);
        // 指定 tab 透传
        let route = plan_open_settings("main", Some("keybindings"), true, false, true).unwrap();
        assert_eq!(
            route.target,
            SettingsTarget::MainModal {
                tab: "keybindings".to_string()
            }
        );
    }

    #[test]
    fn crashed_main_falls_back_to_settings_window() {
        let route = plan_open_settings("main", Some("general"), true, true, false).unwrap();
        assert_eq!(
            route.target,
            SettingsTarget::SettingsWindow {
                tab: Some("general".to_string()),
                create: true,
                focus_existing: false
            }
        );
        // 已存在独立窗 → focus 并切 tab
        let route = plan_open_settings("main", Some("general"), true, true, true).unwrap();
        assert_eq!(route.event, EVENT_SETTINGS_SWITCH_TAB);
        match route.target {
            SettingsTarget::SettingsWindow {
                create,
                focus_existing,
                ..
            } => {
                assert!(!create);
                assert!(focus_existing);
            }
            _ => panic!("应为设置窗"),
        }
    }

    #[test]
    fn tab_validation_is_syntactic() {
        for good in ["agent", "keybindings", "general", "envdeps", "a", "tab-2"] {
            assert!(validate_tab(good).is_ok(), "应接受 {good}");
        }
        for bad in [
            "",
            "Agent",
            "tab_2",
            "tab 2",
            "../evil",
            &"a".repeat(33),
            "tab\u{0}",
        ] {
            assert!(validate_tab(bad).is_err(), "应拒绝 {bad:?}");
        }
        assert!(validate_switch_tab_payload("about").is_ok());
        assert!(validate_switch_tab_payload("bad tab").is_err());
    }

    #[test]
    fn internal_tray_route_has_no_caller() {
        let route = plan_open_settings_internal(None, true, false, false).unwrap();
        assert_eq!(
            route.target,
            SettingsTarget::MainModal {
                tab: "agent".to_string()
            }
        );
        let route = plan_open_settings_internal(None, false, false, true).unwrap();
        assert_eq!(route.event, EVENT_SETTINGS_SWITCH_TAB);
    }

    #[test]
    fn caller_policy_and_size() {
        assert!(plan_open_settings("settings", None, true, false, false).is_ok());
        for caller in ["quick-chat", "viewer-1", "browser-viewer", "splash"] {
            assert_eq!(
                plan_open_settings(caller, None, true, false, false)
                    .unwrap_err()
                    .code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(caller_denied().code(), E_WINDOW_NOT_ALLOWED);
        assert_eq!(settings_window_size(None), (720, 700));
        assert_eq!(settings_window_size(Some((960, 820))), (864, 738));
        assert_eq!(settings_window_size(Some((0, 0))), (720, 700));
        assert_eq!(SETTINGS_WINDOW_PAGE, "settings.html");
    }
}
