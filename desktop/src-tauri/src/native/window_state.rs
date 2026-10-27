//! R09-T03 窗口状态持久化（`host:window-state`，OQ-13 壳自态）。
//!
//! 现役实现：`LINGXI_HOME/user/window-state.json`（main.cjs:2712/2769/2779 +
//! desktop/src/shared/window-state.cjs），保存 `{x,y,width,height,isMaximized}`
//! （最大化时取 `getNormalBounds()`），500ms 防抖 + 串行写链，读取后按当前
//! 显示器工作区 sanitize（最小可见交集 80x60，越界/不可见则居中）。
//!
//! 不用 tauri-plugin-window-state：其默认路径会改变壳自态存储位置
//! （R00 STORES adjacent_compatible），插件也拿不到 `LINGXI_HOME` 语义。
//! 本模块移植 sanitize 纯函数 + 路径/防抖常量，供 host-core 在 setup/窗口事件
//! 里调用；文件 IO 由 host-core 做（标准库，无 tauri 依赖，本模块也可只读）。

use super::errors::{deny, HostResult, E_PATH_NOT_ABSOLUTE};
use std::path::PathBuf;

/// 现役路径相对 LINGXI_HOME（R00 STORES adjacent_compatible）。
pub const WINDOW_STATE_RELATIVE_PATH: &str = "user/window-state.json";
pub const WINDOW_STATE_SAVE_DEBOUNCE_MS: u64 = 500;

/// 显示器工作区（Electron `display.workArea`）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkArea {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl WorkArea {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn normalize(self) -> Option<Self> {
        let finite_origin = self.x.is_finite() && self.y.is_finite();
        if !finite_origin || !(self.width.is_finite() && self.width > 0.0) {
            return None;
        }
        if !(self.height.is_finite() && self.height > 0.0) {
            return None;
        }
        Some(Self {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        })
    }
}

/// 磁盘上的原始状态（字段都可能缺失/类型错误；由 host-core 反序列化时容错）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PersistedWindowState {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub is_maximized: Option<bool>,
}

/// sanitize 参数（现役 main 采用 960x820 / min 420x500；quick-chat 另有预设）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowStateOptions {
    pub default_width: f64,
    pub default_height: f64,
    pub min_width: f64,
    pub min_height: f64,
    pub min_visible_width: f64,
    pub min_visible_height: f64,
}

impl Default for WindowStateOptions {
    fn default() -> Self {
        Self::main()
    }
}

impl WindowStateOptions {
    pub fn main() -> Self {
        Self {
            default_width: 960.0,
            default_height: 820.0,
            min_width: 420.0,
            min_height: 500.0,
            min_visible_width: 80.0,
            min_visible_height: 60.0,
        }
    }

    /// quick-chat 现役：宽固定 360、compact/chat 高度不同，最小可见 96x72。
    pub fn quick_chat(default_width: f64, default_height: f64, min_height: f64) -> Self {
        Self {
            default_width,
            default_height,
            min_width: default_width,
            min_height,
            min_visible_width: 96.0,
            min_visible_height: 72.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesiredWindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_maximized: bool,
}

fn bounded_size(value: Option<f64>, default: f64, min: f64, max: f64) -> f64 {
    let normalized = match value {
        Some(number) if number.is_finite() && number > 0.0 => number.round(),
        _ => default,
    };
    // JS clamp(value, min(minValue,maxValue), maxValue)
    let lower = min.min(max);
    normalized.clamp(lower, max)
}

fn intersection_visible(
    rect: (f64, f64, f64, f64),
    area: WorkArea,
    min_visible_width: f64,
    min_visible_height: f64,
) -> bool {
    let left = rect.0.max(area.x);
    let top = rect.1.max(area.y);
    let right = (rect.0 + rect.2).min(area.x + area.width);
    let bottom = (rect.1 + rect.3).min(area.y + area.height);
    let width = (right - left).max(0.0);
    let height = (bottom - top).max(0.0);
    width >= min_visible_width && height >= min_visible_height
}

fn center_in_area(width: f64, height: f64, area: WorkArea) -> (f64, f64) {
    (
        (area.x + (area.width - width) / 2.0).round(),
        (area.y + (area.height - height) / 2.0).round(),
    )
}

fn virtual_area(areas: &[WorkArea]) -> Option<WorkArea> {
    if areas.is_empty() {
        return None;
    }
    let left = areas
        .iter()
        .map(|area| area.x)
        .fold(f64::INFINITY, f64::min);
    let top = areas
        .iter()
        .map(|area| area.y)
        .fold(f64::INFINITY, f64::min);
    let right = areas
        .iter()
        .map(|area| area.x + area.width)
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = areas
        .iter()
        .map(|area| area.y + area.height)
        .fold(f64::NEG_INFINITY, f64::max);
    Some(WorkArea::new(left, top, right - left, bottom - top))
}

/// 移植 `desktop/src/shared/window-state.cjs::sanitizeWindowState`。
///
/// - 无效显示器条目被忽略；完全没有显示器时用主窗默认尺寸作为工作区；
/// - 尺寸先按虚拟桌面边界收敛（最小 420x500，最大不超虚拟桌面）；
/// - 有合法坐标且与任一工作区有 >=80x60 可见交集 → 保留位置；
/// - 否则按主显示器居中，并重新按主显示器收敛尺寸；
/// - `isMaximized` 仅 `Some(true)` 为真（缺省/畸形 → false）。
pub fn sanitize_window_state(
    state: Option<PersistedWindowState>,
    displays: &[WorkArea],
    options: &WindowStateOptions,
) -> Option<DesiredWindowState> {
    let state = state?;
    let areas: Vec<WorkArea> = displays
        .iter()
        .filter_map(|area| area.normalize())
        .collect();
    let primary = areas
        .first()
        .copied()
        .unwrap_or_else(|| WorkArea::new(0.0, 0.0, options.default_width, options.default_height));
    let visible_areas: Vec<WorkArea> = if areas.is_empty() {
        vec![primary]
    } else {
        areas.clone()
    };
    let virtual_area = virtual_area(&visible_areas).unwrap_or(primary);
    let saved_width = bounded_size(
        state.width,
        options.default_width,
        options.min_width,
        virtual_area.width,
    );
    let saved_height = bounded_size(
        state.height,
        options.default_height,
        options.min_height,
        virtual_area.height,
    );
    let is_maximized = state.is_maximized == Some(true);

    if let (Some(raw_x), Some(raw_y)) = (state.x, state.y) {
        if raw_x.is_finite() && raw_y.is_finite() {
            let candidate = (raw_x.round(), raw_y.round(), saved_width, saved_height);
            let visible = visible_areas.iter().any(|area| {
                intersection_visible(
                    candidate,
                    *area,
                    options.min_visible_width,
                    options.min_visible_height,
                )
            });
            if visible {
                return Some(DesiredWindowState {
                    x: candidate.0 as i32,
                    y: candidate.1 as i32,
                    width: saved_width as u32,
                    height: saved_height as u32,
                    is_maximized,
                });
            }
        }
    }

    let recentered_width = bounded_size(
        state.width,
        options.default_width,
        options.min_width,
        primary.width,
    );
    let recentered_height = bounded_size(
        state.height,
        options.default_height,
        options.min_height,
        primary.height,
    );
    let (x, y) = center_in_area(recentered_width, recentered_height, primary);
    Some(DesiredWindowState {
        x: x as i32,
        y: y as i32,
        width: recentered_width as u32,
        height: recentered_height as u32,
        is_maximized,
    })
}

/// 保存到磁盘的形态：最大化时用 normal bounds（现役 `getNormalBounds()`）。
pub fn state_to_persist(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    is_maximized: bool,
) -> PersistedWindowState {
    PersistedWindowState {
        x: Some(x.round()),
        y: Some(y.round()),
        width: Some(width.round()),
        height: Some(height.round()),
        is_maximized: Some(is_maximized),
    }
}

/// 壳自态文件绝对路径（home 必须由宿主解析且为绝对路径）。
pub fn persistence_path(lingxi_home: &str) -> HostResult<PathBuf> {
    if !std::path::Path::new(lingxi_home).is_absolute() {
        return Err(deny(E_PATH_NOT_ABSOLUTE, "LINGXI_HOME 必须是绝对路径"));
    }
    Ok(PathBuf::from(lingxi_home).join(WINDOW_STATE_RELATIVE_PATH))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn displays() -> Vec<WorkArea> {
        vec![
            WorkArea::new(0.0, 0.0, 1920.0, 1080.0),
            WorkArea::new(1920.0, 0.0, 2560.0, 1440.0),
        ]
    }

    #[test]
    fn missing_or_null_state_yields_none() {
        assert_eq!(
            sanitize_window_state(None, &displays(), &WindowStateOptions::main()),
            None
        );
    }

    #[test]
    fn defaults_and_clamps_sizes() {
        let opts = WindowStateOptions::main();
        // 无尺寸 → 默认 960x820，无坐标 → 主屏居中
        let state = PersistedWindowState::default();
        let desired = sanitize_window_state(Some(state), &displays(), &opts).unwrap();
        assert_eq!((desired.width, desired.height), (960, 820));
        assert_eq!((desired.x, desired.y), (480, 130));
        assert!(!desired.is_maximized);

        // 小于最小尺寸 → 抬到 min
        let tiny = PersistedWindowState {
            width: Some(100.0),
            height: Some(200.0),
            ..Default::default()
        };
        let desired = sanitize_window_state(Some(tiny), &displays(), &opts).unwrap();
        assert_eq!((desired.width, desired.height), (420, 500));

        // 超过虚拟桌面宽度 → 收敛到虚拟桌面
        let huge = PersistedWindowState {
            x: Some(0.0),
            y: Some(0.0),
            width: Some(99999.0),
            height: Some(99999.0),
            is_maximized: Some(false),
        };
        let desired = sanitize_window_state(Some(huge), &displays(), &opts).unwrap();
        assert_eq!((desired.width, desired.height), (4480, 1440));
    }

    #[test]
    fn visible_position_is_preserved_across_displays() {
        let opts = WindowStateOptions::main();
        let state = PersistedWindowState {
            x: Some(-200.0),
            y: Some(100.0),
            width: Some(960.0),
            height: Some(820.0),
            is_maximized: Some(true),
        };
        // x=-200: 与主屏交集 (0..760)=760 >=80，y=100..920 >=60
        let desired = sanitize_window_state(Some(state), &displays(), &opts).unwrap();
        assert_eq!((desired.x, desired.y), (-200, 100));
        assert!(desired.is_maximized, "最大化标记必须保留");

        // 第二显示器上的位置必须保留
        let second = PersistedWindowState {
            x: Some(2000.0),
            y: Some(120.0),
            width: Some(800.0),
            height: Some(600.0),
            ..Default::default()
        };
        let desired = sanitize_window_state(Some(second), &displays(), &opts).unwrap();
        assert_eq!((desired.x, desired.y), (2000, 120));
    }

    #[test]
    fn offscreen_position_is_recentered_on_primary() {
        let opts = WindowStateOptions::main();
        let offscreen = PersistedWindowState {
            x: Some(99999.0),
            y: Some(99999.0),
            width: Some(960.0),
            height: Some(820.0),
            ..Default::default()
        };
        let desired = sanitize_window_state(Some(offscreen), &displays(), &opts).unwrap();
        assert_eq!((desired.x, desired.y), (480, 130));
        // 半可见（只露 40px 宽 < 80）也必须居中：单显示器场景
        let single = vec![displays()[0]];
        let barely = PersistedWindowState {
            x: Some(1880.0),
            y: Some(0.0),
            width: Some(960.0),
            height: Some(820.0),
            ..Default::default()
        };
        let desired = sanitize_window_state(Some(barely), &single, &opts).unwrap();
        assert_eq!((desired.x, desired.y), (480, 130));
    }

    #[test]
    fn invalid_display_entries_are_ignored_and_empty_display_falls_back() {
        let opts = WindowStateOptions::main();
        let bad = [
            WorkArea::new(f64::NAN, 0.0, 100.0, 100.0),
            WorkArea::new(0.0, 0.0, 0.0, 100.0),
            WorkArea::new(0.0, 0.0, 100.0, -5.0),
        ];
        // 只剩无效显示器 → 用默认 960x820 工作区；坐标 (0,0) 在其中完全可见，
        // 与现役 JS 行为一致：保留 (0,0) 而不是居中。
        let state = PersistedWindowState {
            x: Some(0.0),
            y: Some(0.0),
            ..Default::default()
        };
        let desired = sanitize_window_state(Some(state), &bad, &opts).unwrap();
        assert_eq!((desired.x, desired.y), (0, 0));
        assert_eq!(
            sanitize_window_state(Some(PersistedWindowState::default()), &[], &opts)
                .unwrap()
                .width,
            960
        );
    }

    #[test]
    fn persistence_path_is_home_adjacent() {
        let path = persistence_path("/private/tmp/lingxi-r09-home").unwrap();
        assert_eq!(
            path,
            PathBuf::from("/private/tmp/lingxi-r09-home/user/window-state.json")
        );
        assert_eq!(
            persistence_path("relative/home").unwrap_err().code(),
            E_PATH_NOT_ABSOLUTE
        );
    }

    #[test]
    fn persisted_form_round_trips_through_sanitize() {
        let opts = WindowStateOptions::main();
        let persisted = state_to_persist(100.5, 200.4, 800.6, 600.2, false);
        assert_eq!(persisted.x, Some(101.0));
        assert_eq!(persisted.y, Some(200.0));
        let desired = sanitize_window_state(Some(persisted), &displays(), &opts).unwrap();
        assert_eq!((desired.x, desired.y), (101, 200));
        assert_eq!((desired.width, desired.height), (801, 600));
        // 现役 quick-chat 预设
        let quick = WindowStateOptions::quick_chat(360.0, 420.0, 120.0);
        assert_eq!(quick.min_width, 360.0);
        assert_eq!(quick.min_visible_width, 96.0);
    }
}
