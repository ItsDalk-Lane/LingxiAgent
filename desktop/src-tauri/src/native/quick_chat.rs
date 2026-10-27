//! R09-T03 Quick Chat 小窗（`host:quick-chat-window`）。
//!
//! 现役 Electron（desktop/main.cjs:506-515、2800-3030）：
//! - 窗口 480x142(compact)/520(chat)，min 360x118，frameless/置顶/透明/不显示
//!   在任务栏(mac 除外)/acceptFirstMouse；
//! - `quick-chat-resize` 接受 `'compact' | 'chat' | {mode, height}`；
//!   compact 固定不可 resize，chat 可 resize 且「只增不缩」（尊重用户手动拉大）；
//! - show：setAlwaysOnTop(floating)、mac visibleOnAllWorkspaces + app focus steal、
//!   show+focus、向页面发 `quick-chat-shown`；
//! - hide：保存窗口状态 → mac 焦点保护（主窗 setFocusable(false) 300ms）→ hide；
//! - 状态落 `LINGXI_HOME/user/quick-chat-window-state.json`（保存 x/y/width/height +
//!   chat 模式的 chatWidth/chatHeight；compact 保存时若旧状态像 chat 尺寸也保留）。
//!
//! Tauri 透明窗在 Linux 常失败（OQ-08）→ 平台阻塞登记，不用空实现冒充。

use std::path::PathBuf;

use super::errors::{
    deny, HostResult, E_INVALID_ARGUMENT, E_PLATFORM_UNSUPPORTED, E_WINDOW_NOT_ALLOWED,
};
use super::window_labels::{require_role, WindowRole};
use super::window_state::{sanitize_window_state, PersistedWindowState, WindowStateOptions};

pub const QUICK_CHAT_WIDTH: u32 = 480;
pub const QUICK_CHAT_COMPACT_HEIGHT: u32 = 142;
pub const QUICK_CHAT_CHAT_HEIGHT: u32 = 520;
pub const QUICK_CHAT_MIN_WIDTH: u32 = 360;
pub const QUICK_CHAT_MIN_HEIGHT: u32 = 118;
pub const QUICK_CHAT_STATE_RELATIVE_PATH: &str = "user/quick-chat-window-state.json";
pub const QUICK_CHAT_SAVE_DEBOUNCE_MS: u64 = 300;
pub const QUICK_CHAT_SCREEN_MARGIN: f64 = 24.0;
pub const EVENT_QUICK_CHAT_SHOWN: &str = "quick-chat-shown";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickChatMode {
    Compact,
    Chat,
}

/// `normalizeQuickChatResizeRequest` 的移植：对象取 mode/height，字符串取 mode。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuickChatResizeRequest {
    pub mode: QuickChatMode,
    pub height: Option<f64>,
}

pub fn normalize_resize_request(
    raw_mode: Option<&str>,
    raw_height: Option<f64>,
) -> QuickChatResizeRequest {
    QuickChatResizeRequest {
        mode: if raw_mode == Some("chat") {
            QuickChatMode::Chat
        } else {
            QuickChatMode::Compact
        },
        height: raw_height.filter(|value| value.is_finite()),
    }
}

/// `quickChatHeightForMode` 的移植。
pub fn height_for_mode(
    mode: QuickChatMode,
    requested_height: Option<f64>,
    work_area_height: f64,
) -> u32 {
    let base = match mode {
        QuickChatMode::Chat => QUICK_CHAT_CHAT_HEIGHT,
        QuickChatMode::Compact => QUICK_CHAT_COMPACT_HEIGHT,
    };
    let height = match requested_height {
        Some(value) if value.is_finite() => (value.round() as f64).max(f64::from(base)),
        _ => f64::from(base),
    };
    let area_height = if work_area_height.is_finite() && work_area_height > 0.0 {
        work_area_height
    } else {
        height
    };
    let max_height = f64::from(QUICK_CHAT_MIN_HEIGHT).max(area_height - QUICK_CHAT_SCREEN_MARGIN);
    (height.min(max_height).max(f64::from(QUICK_CHAT_MIN_HEIGHT))) as u32
}

/// 持久化的 quick-chat 状态（字段与现役 JSON 一致）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct QuickChatPersistedState {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub chat_width: Option<f64>,
    pub chat_height: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickChatBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

fn default_state(
    mode: QuickChatMode,
    requested_height: Option<f64>,
    cursor_area: Option<(f64, f64, f64, f64)>,
) -> QuickChatBounds {
    let (area_x, area_y, area_width, area_height) = cursor_area.unwrap_or((
        0.0,
        0.0,
        f64::from(QUICK_CHAT_WIDTH),
        f64::from(QUICK_CHAT_COMPACT_HEIGHT),
    ));
    let height = height_for_mode(mode, requested_height, area_height);
    QuickChatBounds {
        width: QUICK_CHAT_WIDTH,
        height,
        x: (area_x + (area_width - f64::from(QUICK_CHAT_WIDTH)) / 2.0).round() as i32,
        y: (area_y + (area_height - f64::from(height)) / 3.0).round() as i32,
    }
}

/// `resolveQuickChatWindowBounds` 的移植：按模式宽度 + 高度 + 可见性 sanitize。
/// `areas` 是显示器工作区（第一个视为主屏）；`cursor_area` 是鼠标所在屏工作区。
pub fn resolve_bounds(
    mode: QuickChatMode,
    state: Option<QuickChatPersistedState>,
    requested_height: Option<f64>,
    areas: &[super::window_state::WorkArea],
    cursor_area: Option<(f64, f64, f64, f64)>,
) -> QuickChatBounds {
    let Some(state) = state else {
        return default_state(mode, requested_height, cursor_area);
    };
    let persisted = PersistedWindowState {
        x: state.x,
        y: state.y,
        width: Some(f64::from(
            state
                .chat_width
                .unwrap_or(state.width.unwrap_or(f64::from(QUICK_CHAT_WIDTH))),
        )),
        height: state.height,
        is_maximized: None,
    };
    let chat_width = state
        .chat_width
        .or(state.width)
        .unwrap_or(f64::from(QUICK_CHAT_WIDTH));
    // 工作区高度取首个显示器（现役 screen.getDisplayNearestPoint(...).workArea.height）；
    // 缺失时用 compact 高度兜底，避免 0 高度把 maxHeight 压到 MIN。
    let area_height = areas
        .first()
        .map(|area| area.height)
        .or(cursor_area.map(|area| area.3))
        .unwrap_or(f64::from(QUICK_CHAT_COMPACT_HEIGHT));
    let requested_mode_height = height_for_mode(mode, requested_height, area_height);
    let width = match mode {
        QuickChatMode::Chat => (chat_width.round() as u32).max(QUICK_CHAT_MIN_WIDTH),
        QuickChatMode::Compact => QUICK_CHAT_WIDTH,
    };
    let height = match mode {
        QuickChatMode::Chat => height_for_mode(
            mode,
            Some(f64::from(
                requested_mode_height.max((state.chat_height.unwrap_or(0.0)).round() as u32),
            )),
            area_height,
        ),
        QuickChatMode::Compact => requested_mode_height,
    };
    let options = WindowStateOptions::quick_chat(
        f64::from(width),
        f64::from(height),
        f64::from(QUICK_CHAT_MIN_HEIGHT.min(height)),
    );
    let sanitized = sanitize_window_state(
        Some(PersistedWindowState {
            width: Some(f64::from(width)),
            height: Some(f64::from(height)),
            ..persisted
        }),
        areas,
        &options,
    );
    match sanitized {
        Some(bounds) => QuickChatBounds {
            x: bounds.x,
            y: bounds.y,
            width: if mode == QuickChatMode::Chat {
                bounds.width.max(QUICK_CHAT_MIN_WIDTH)
            } else {
                QUICK_CHAT_WIDTH
            },
            height: bounds.height,
        },
        None => default_state(mode, requested_height, cursor_area),
    }
}

/// 保存合并（现役 saveQuickChatWindowState 的语义）。
pub fn merge_saved_state(
    previous: Option<QuickChatPersistedState>,
    bounds: QuickChatBounds,
    mode: QuickChatMode,
) -> QuickChatPersistedState {
    let previous = previous.unwrap_or_default();
    let mut next = previous;
    next.x = Some(f64::from(bounds.x));
    next.y = Some(f64::from(bounds.y));
    next.width = Some(f64::from(bounds.width));
    next.height = Some(f64::from(bounds.height));
    let previous_looks_like_chat = previous
        .width
        .is_some_and(|width| width > f64::from(QUICK_CHAT_WIDTH))
        || previous
            .height
            .is_some_and(|height| height > f64::from(QUICK_CHAT_COMPACT_HEIGHT));
    if mode == QuickChatMode::Chat {
        next.chat_width = Some(f64::from(bounds.width));
        next.chat_height = Some(f64::from(bounds.height));
    } else if next.chat_width.is_none() && previous_looks_like_chat {
        next.chat_width = previous.width;
        next.chat_height = previous.height;
    }
    next
}

/// show 效果（host-core 映射到 Tauri window API）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowPlan {
    pub always_on_top: bool,
    pub visible_on_all_workspaces: bool,
    pub focus_steal: bool,
    pub restore_if_minimized: bool,
    pub show: bool,
    pub focus: bool,
    pub emit_shown: bool,
}

pub fn show_plan(platform: &str) -> ShowPlan {
    let macos = platform == "darwin";
    ShowPlan {
        always_on_top: true,
        visible_on_all_workspaces: macos,
        focus_steal: macos,
        restore_if_minimized: true,
        show: true,
        focus: true,
        emit_shown: true,
    }
}

/// hide 效果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidePlan {
    pub save_state: bool,
    pub suspend_main_focus_macos: bool,
    pub hide: bool,
}

pub fn hide_plan(platform: &str) -> HidePlan {
    HidePlan {
        save_state: true,
        suspend_main_focus_macos: platform == "darwin",
        hide: true,
    }
}

/// chat 模式的「只增不缩」规则：已在 chat 模式时，新 bounds 不小于当前。
pub fn apply_chat_grow_only(
    previous_mode: QuickChatMode,
    current: QuickChatBounds,
    mut desired: QuickChatBounds,
) -> QuickChatBounds {
    if previous_mode == QuickChatMode::Chat {
        desired.height = desired.height.max(current.height);
        desired.width = desired.width.max(current.width);
    }
    desired
}

pub fn authorize_show(caller: &str) -> HostResult<()> {
    require_role(caller, &[WindowRole::Main, WindowRole::QuickChat]).map(|_| ())
}

pub fn authorize_hide(caller: &str) -> HostResult<()> {
    require_role(caller, &[WindowRole::Main, WindowRole::QuickChat]).map(|_| ())
}

pub fn authorize_resize(caller: &str) -> HostResult<()> {
    require_role(caller, &[WindowRole::QuickChat]).map(|_| ())
}

pub fn quick_chat_state_path(lingxi_home: &str) -> HostResult<PathBuf> {
    if !std::path::Path::new(lingxi_home).is_absolute() {
        return Err(deny(
            super::errors::E_PATH_NOT_ABSOLUTE,
            "LINGXI_HOME 必须是绝对路径",
        ));
    }
    Ok(PathBuf::from(lingxi_home).join(QUICK_CHAT_STATE_RELATIVE_PATH))
}

/// Linux 透明窗未验证：创建时显式阻塞（OQ-08），不静默降级为不透明窗。
pub fn transparent_window_supported(platform: &str) -> HostResult<()> {
    if platform == "linux" {
        return Err(deny(
            E_PLATFORM_UNSUPPORTED,
            "Linux 下 Tauri 透明置顶窗未验证（OQ-08/BLOCKED_PLATFORM）；不得以不透明窗冒充",
        ));
    }
    Ok(())
}

pub fn resize_payload_error() -> super::errors::HostError {
    deny(E_INVALID_ARGUMENT, "quick_chat_resize 参数形态非法")
}

pub fn caller_denied(action: &'static str) -> super::errors::HostError {
    deny(E_WINDOW_NOT_ALLOWED, format!("{action} 不允许该窗口调用"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::window_state::WorkArea;

    #[test]
    fn mode_and_height_normalization() {
        assert_eq!(
            normalize_resize_request(Some("chat"), Some(600.4)).mode,
            QuickChatMode::Chat
        );
        assert_eq!(
            normalize_resize_request(None, None).mode,
            QuickChatMode::Compact
        );
        assert_eq!(
            normalize_resize_request(Some("fullscreen"), None).mode,
            QuickChatMode::Compact,
            "未知 mode 回落 compact（现役语义）"
        );
        assert_eq!(
            height_for_mode(QuickChatMode::Compact, None, 1080.0),
            QUICK_CHAT_COMPACT_HEIGHT
        );
        assert_eq!(
            height_for_mode(QuickChatMode::Chat, None, 1080.0),
            QUICK_CHAT_CHAT_HEIGHT
        );
        // 只增不缩：请求低于 base → base
        assert_eq!(
            height_for_mode(QuickChatMode::Compact, Some(100.0), 1080.0),
            QUICK_CHAT_COMPACT_HEIGHT
        );
        // 不超过工作区 - 24
        assert_eq!(
            height_for_mode(QuickChatMode::Chat, Some(2000.0), 900.0),
            876
        );
        // 工作区不足时保底 MIN_HEIGHT
        assert_eq!(
            height_for_mode(QuickChatMode::Chat, None, 100.0),
            QUICK_CHAT_MIN_HEIGHT
        );
    }

    #[test]
    fn default_bounds_center_on_cursor_display() {
        let bounds = default_state(
            QuickChatMode::Compact,
            None,
            Some((0.0, 0.0, 1920.0, 1080.0)),
        );
        assert_eq!(bounds.width, 480);
        assert_eq!(bounds.height, 142);
        assert_eq!(bounds.x, 720, "水平居中");
        assert!(bounds.y > 0);
    }

    #[test]
    fn resolve_bounds_restores_and_sanitizes() {
        let areas = [WorkArea::new(0.0, 0.0, 1920.0, 1080.0)];
        let state = QuickChatPersistedState {
            x: Some(200.0),
            y: Some(300.0),
            width: Some(480.0),
            height: Some(142.0),
            ..Default::default()
        };
        let bounds = resolve_bounds(QuickChatMode::Compact, Some(state), None, &areas, None);
        assert_eq!((bounds.x, bounds.y), (200, 300));
        assert_eq!((bounds.width, bounds.height), (480, 142));

        // 离屏 → 居中
        let offscreen = QuickChatPersistedState {
            x: Some(99999.0),
            y: Some(99999.0),
            ..state
        };
        let bounds = resolve_bounds(QuickChatMode::Compact, Some(offscreen), None, &areas, None);
        assert_eq!(bounds.x, 720);

        // chat 模式恢复 chatWidth/chatHeight
        let chat_state = QuickChatPersistedState {
            x: Some(100.0),
            y: Some(100.0),
            width: Some(480.0),
            height: Some(142.0),
            chat_width: Some(800.0),
            chat_height: Some(600.0),
        };
        let bounds = resolve_bounds(QuickChatMode::Chat, Some(chat_state), None, &areas, None);
        assert_eq!((bounds.width, bounds.height), (800, 600));
        // compact 永远回到固定宽度
        let bounds = resolve_bounds(QuickChatMode::Compact, Some(chat_state), None, &areas, None);
        assert_eq!(bounds.width, QUICK_CHAT_WIDTH);
    }

    #[test]
    fn chat_mode_is_grow_only() {
        let current = QuickChatBounds {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        let desired = QuickChatBounds {
            x: 0,
            y: 0,
            width: 700,
            height: 500,
        };
        let grown = apply_chat_grow_only(QuickChatMode::Chat, current, desired);
        assert_eq!((grown.width, grown.height), (800, 600));
        let allowed = QuickChatBounds {
            width: 900,
            height: 700,
            ..desired
        };
        let grown = apply_chat_grow_only(QuickChatMode::Chat, current, allowed);
        assert_eq!((grown.width, grown.height), (900, 700));
        // 从 compact 进入 chat 不受只增不缩限制
        let from_compact = apply_chat_grow_only(QuickChatMode::Compact, current, desired);
        assert_eq!((from_compact.width, from_compact.height), (700, 500));
    }

    #[test]
    fn state_saving_merges_chat_dimensions() {
        let previous = QuickChatPersistedState {
            width: Some(800.0),
            height: Some(600.0),
            ..Default::default()
        };
        let bounds = QuickChatBounds {
            x: 10,
            y: 20,
            width: 480,
            height: 142,
        };
        let saved = merge_saved_state(Some(previous), bounds, QuickChatMode::Compact);
        assert_eq!((saved.x, saved.y), (Some(10.0), Some(20.0)));
        // compact 保存时旧 chat 尺寸保留
        assert_eq!(saved.chat_width, Some(800.0));
        assert_eq!(saved.chat_height, Some(600.0));

        let bounds = QuickChatBounds {
            x: 10,
            y: 20,
            width: 900,
            height: 700,
        };
        let saved = merge_saved_state(None, bounds, QuickChatMode::Chat);
        assert_eq!(saved.chat_width, Some(900.0));
        assert_eq!(saved.chat_height, Some(700.0));
    }

    #[test]
    fn show_hide_plans_follow_platform() {
        let mac = show_plan("darwin");
        assert!(mac.always_on_top && mac.visible_on_all_workspaces && mac.focus_steal);
        assert!(mac.emit_shown);
        let linux = show_plan("linux");
        assert!(linux.always_on_top);
        assert!(!linux.visible_on_all_workspaces);
        assert!(!linux.focus_steal);
        assert!(hide_plan("darwin").suspend_main_focus_macos);
        assert!(!hide_plan("win32").suspend_main_focus_macos);
    }

    #[test]
    fn caller_policies_and_platform_gate() {
        assert!(authorize_show("main").is_ok());
        assert!(authorize_show("quick-chat").is_ok());
        assert_eq!(
            authorize_show("settings").unwrap_err().code(),
            E_WINDOW_NOT_ALLOWED
        );
        assert!(authorize_resize("quick-chat").is_ok());
        assert_eq!(
            authorize_resize("main").unwrap_err().code(),
            E_WINDOW_NOT_ALLOWED
        );
        assert!(transparent_window_supported("darwin").is_ok());
        assert!(transparent_window_supported("win32").is_ok());
        assert_eq!(
            transparent_window_supported("linux").unwrap_err().code(),
            E_PLATFORM_UNSUPPORTED
        );
        assert_eq!(
            quick_chat_state_path("relative").unwrap_err().code(),
            crate::native::errors::E_PATH_NOT_ABSOLUTE
        );
        assert!(quick_chat_state_path("/private/tmp/lingxi-r09")
            .unwrap()
            .ends_with("user/quick-chat-window-state.json"));
        assert_eq!(QUICK_CHAT_SCREEN_MARGIN, 24.0);
    }
}
