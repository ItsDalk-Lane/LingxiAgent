//! R09-T04 页面截图渲染（`host:screenshot-render`，OQ-04）。
//!
//! 现役 Electron（desktop/main.cjs:4893-4960、5377、6357-6400）：
//! - 6 个主题（solarized-light/dark、solarized-light-desktop/dark-desktop、
//!   sakura-light/sakura-light-desktop）→ 宽度 460/880 + 主题背景色；
//! - 隐藏 offscreen BrowserWindow（deviceScaleFactor 2）渲染 HTML → capturePage，
//!   长文分片（每片 ≤4000px，按主屏工作区高度-96 收敛）后纵向拼接；
//! - `preview` 模式返回 base64 **不写盘**；保存模式写到
//!   `saveDir || ~/Desktop` 下 `screenshots/<locale>/hanako-YYYYMMDD-HHMMSS[-NN-of-MM].png`；
//! - 未知主题/捕获失败返回 `{success:false,error}`（不抛）。
//!
//! Tauri/Wry 没有官方稳定的跨平台页面捕获 API（OQ-04）：本模块先落**可验证的
//! 策略层**（主题白名单/保存目录 scope/分段索引/文件名/结果形状），捕获执行
//! 显式登记为未实现（BLOCKED，不允许假绿）；真实实现需 wry 平台钩子或
//! 服务端 HTML→PNG 渲染，属后续 spike/裁决。

use std::path::{Path, PathBuf};

use super::errors::{
    deny, HostResult, E_INVALID_ARGUMENT, E_NOT_IMPLEMENTED, E_PATH_NOT_ABSOLUTE,
    E_URL_SCHEME_DENIED,
};
use super::window_labels::{require_role, WindowRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenshotTheme {
    pub id: &'static str,
    pub width: u32,
    pub background_hex: &'static str,
}

/// 6 主题（与现役 `SCREENSHOT_THEMES` 逐项一致）。
pub const SCREENSHOT_THEMES: &[ScreenshotTheme] = &[
    ScreenshotTheme {
        id: "solarized-light",
        width: 460,
        background_hex: "#F8F5ED",
    },
    ScreenshotTheme {
        id: "solarized-dark",
        width: 460,
        background_hex: "#002b36",
    },
    ScreenshotTheme {
        id: "solarized-light-desktop",
        width: 880,
        background_hex: "#F8F5ED",
    },
    ScreenshotTheme {
        id: "solarized-dark-desktop",
        width: 880,
        background_hex: "#002b36",
    },
    ScreenshotTheme {
        id: "sakura-light",
        width: 460,
        background_hex: "#8ABDCE",
    },
    ScreenshotTheme {
        id: "sakura-light-desktop",
        width: 880,
        background_hex: "#8ABDCE",
    },
];

pub const CAPTURE_SCALE: u32 = 2;
pub const MAX_SEGMENT_HEIGHT: u32 = 4000;
pub const SEGMENT_SCREEN_MARGIN: u32 = 96;
pub const DEFAULT_SAVE_SUBDIR: &str = "screenshots";
pub const FILENAME_PREFIX: &str = "hanako-";

pub fn theme_by_id(id: &str) -> Option<&'static ScreenshotTheme> {
    SCREENSHOT_THEMES.iter().find(|theme| theme.id == id)
}

/// 主题未知 → 现役 `{success:false,error:"Unknown theme: X"}`。
pub fn validate_theme(raw: &str) -> HostResult<&'static ScreenshotTheme> {
    theme_by_id(raw).ok_or_else(|| deny(E_INVALID_ARGUMENT, format!("Unknown theme: {raw}")))
}

/// 单个分段高度上限（现役 resolveScreenshotMaxSegmentHeight：主屏工作区高-96，
/// 再与 4000 取小；工作区缺失用 4000）。
pub fn max_segment_height(work_area_height: Option<u32>) -> u32 {
    let Some(height) = work_area_height.filter(|height| *height > 0) else {
        return MAX_SEGMENT_HEIGHT;
    };
    let stable = height.saturating_sub(SEGMENT_SCREEN_MARGIN);
    let capped = if stable > 0 { stable } else { height };
    capped.clamp(1, MAX_SEGMENT_HEIGHT)
}

/// 分段索引校验：无分段时返回 None；有分段时 total>1 且 1<=index<=total（≤999）。
pub fn segment_suffix(
    segment_total: Option<u32>,
    segment_index: Option<u32>,
) -> HostResult<Option<String>> {
    match (segment_total, segment_index) {
        (None, None) => Ok(None),
        (Some(total), Some(index)) => {
            if total <= 1 {
                return Ok(None);
            }
            if total > 999 {
                return Err(deny(E_INVALID_ARGUMENT, "截图分段总数超限"));
            }
            if index == 0 || index > total {
                return Err(deny(E_INVALID_ARGUMENT, "截图分段索引越界"));
            }
            Ok(Some(format!("-{index:02}-of-{total:02}")))
        }
        _ => Err(deny(
            E_INVALID_ARGUMENT,
            "segmentTotal/segmentIndex 必须成对出现",
        )),
    }
}

/// 保存目录：`base/screenshots/<locale>`；base 默认 `~/Desktop`。
/// 只允许绝对路径且位于 home 或工作区根之内（D8/T06 scope：不能写任意目录）。
pub fn resolve_save_dir(
    raw_base: Option<&str>,
    home: &str,
    workspace_root: Option<&str>,
    locale: &str,
) -> HostResult<PathBuf> {
    if !Path::new(home).is_absolute() {
        return Err(deny(E_PATH_NOT_ABSOLUTE, "home 必须是绝对路径"));
    }
    let base = match raw_base {
        Some(value) if !value.is_empty() => {
            if !Path::new(value).is_absolute() {
                return Err(deny(E_PATH_NOT_ABSOLUTE, "截图保存目录必须是绝对路径"));
            }
            PathBuf::from(value)
        }
        _ => Path::new(home).join("Desktop"),
    };
    let resolved = base.join(DEFAULT_SAVE_SUBDIR).join(sanitize_locale(locale));
    let canonical_base = normalize_lexically(&base);
    let ancestors = [
        Path::new(home),
        workspace_root.map(Path::new).unwrap_or(Path::new(home)),
    ];
    let inside = ancestors
        .iter()
        .any(|root| canonical_base.starts_with(normalize_lexically(root)));
    if !inside {
        // base 本身可以是 home/workspace 的子路径；反之（写 home 之外）拒绝。
        return Err(deny(
            E_URL_SCHEME_DENIED,
            "截图保存目录必须位于本机 home 或工作区根之内",
        ));
    }
    Ok(resolved)
}

fn sanitize_locale(locale: &str) -> &str {
    // 目录名只允许 [a-z0-9-]，否则回退 zh（避免路径穿越/非法字符）。
    let valid = !locale.is_empty()
        && locale.len() <= 16
        && locale
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        locale
    } else {
        "zh"
    }
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// 截图文件名（现役 `hanako-YYYYMMDD-HHMMSS[-NN-of-MM].png`）。
pub fn screenshot_file_name(timestamp: &str, suffix: Option<&str>) -> HostResult<String> {
    if timestamp.len() != 15
        || !timestamp.chars().all(|c| c.is_ascii_digit() || c == '-')
        || timestamp.as_bytes().get(8) != Some(&b'-')
    {
        return Err(deny(E_INVALID_ARGUMENT, "截图时间戳必须是 YYYYMMDD-HHMMSS"));
    }
    Ok(format!(
        "{FILENAME_PREFIX}{timestamp}{}.png",
        suffix.unwrap_or("")
    ))
}

/// 渲染结果形状（与现役 handler 返回一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenshotResult {
    Preview { base64: String },
    Saved { file_path: String, dir: String },
    Failure { error: String },
}

/// 预览/保存模式判定 + 保存路径拼装（捕获字节由执行层提供）。
pub struct SavePlan {
    pub dir: PathBuf,
    pub file_path: PathBuf,
}

pub fn plan_save(
    theme: &ScreenshotTheme,
    segment_suffix: Option<&str>,
    timestamp: &str,
    save_dir: &Path,
) -> HostResult<SavePlan> {
    let _ = theme;
    let file_name = screenshot_file_name(timestamp, segment_suffix)?;
    Ok(SavePlan {
        dir: save_dir.to_path_buf(),
        file_path: save_dir.join(file_name),
    })
}

/// 捕获能力（OQ-04）：Tauri/Wry 无稳定跨平台页面捕获 API → 显式 BLOCKED。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSupport {
    Unsupported(&'static str),
}

pub fn capture_support() -> CaptureSupport {
    CaptureSupport::Unsupported(
        "Tauri/Wry 无官方稳定跨平台页面捕获 API（OQ-04）；需 wry 平台钩子或服务端 HTML→PNG 渲染（T04 spike 未完成）",
    )
}

pub fn capture_unavailable_error() -> super::errors::HostError {
    deny(
        E_NOT_IMPLEMENTED,
        "截图捕获在 Tauri 宿主尚未实现（OQ-04）：主题/保存目录策略已实现，捕获执行 BLOCKED",
    )
}

/// 命令调用窗口门：主窗（截图来自聊天/任务卡片）。
pub fn authorize_screenshot_caller(caller: &str) -> HostResult<WindowRole> {
    require_role(caller, &[WindowRole::Main])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_INVALID_ARGUMENT, E_NOT_IMPLEMENTED, E_WINDOW_NOT_ALLOWED};

    #[test]
    fn theme_table_matches_incumbent_six_themes() {
        assert_eq!(SCREENSHOT_THEMES.len(), 6);
        let light = validate_theme("solarized-light").unwrap();
        assert_eq!(light.width, 460);
        assert_eq!(light.background_hex, "#F8F5ED");
        let desktop = validate_theme("solarized-dark-desktop").unwrap();
        assert_eq!(desktop.width, 880);
        assert_eq!(desktop.background_hex, "#002b36");
        assert_eq!(
            validate_theme("sakura-light").unwrap().background_hex,
            "#8ABDCE"
        );
        assert_eq!(
            validate_theme("unknown-theme").unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(validate_theme("").unwrap_err().message(), "Unknown theme: ");
    }

    #[test]
    fn segment_height_and_index_validation() {
        assert_eq!(max_segment_height(None), 4000);
        assert_eq!(max_segment_height(Some(1080)), 984);
        assert_eq!(max_segment_height(Some(9000)), 4000);
        assert_eq!(max_segment_height(Some(0)), 4000);
        assert_eq!(max_segment_height(Some(96)), 96);
        assert_eq!(segment_suffix(None, None).unwrap(), None);
        assert_eq!(segment_suffix(Some(1), Some(1)).unwrap(), None);
        assert_eq!(
            segment_suffix(Some(3), Some(2)).unwrap().as_deref(),
            Some("-02-of-03")
        );
        for bad in [
            Some((Some(3), Some(0))),
            Some((Some(3), Some(4))),
            Some((Some(3), None)),
            Some((None, Some(1))),
            Some((Some(1000), Some(1))),
        ] {
            let (total, index) = bad.unwrap();
            assert!(
                segment_suffix(total, index).is_err(),
                "应拒绝 {total:?}/{index:?}"
            );
        }
    }

    #[test]
    fn save_dir_is_scoped_to_home_or_workspace() {
        let home = "/Users/u";
        let dir = resolve_save_dir(None, home, None, "zh").unwrap();
        assert_eq!(dir, PathBuf::from("/Users/u/Desktop/screenshots/zh"));
        let custom = resolve_save_dir(
            Some("/Users/u/project"),
            home,
            Some("/Users/u/project"),
            "en-US",
        )
        .unwrap();
        assert_eq!(
            custom,
            PathBuf::from("/Users/u/project/screenshots/zh"),
            "locale 目录名只允许 [a-z0-9-]，en-US 回落 zh"
        );
        // 非法 locale 回退 zh
        let fallback = resolve_save_dir(None, home, None, "../evil").unwrap();
        assert!(fallback.ends_with("screenshots/zh"));
        // 相对 base 拒绝
        assert_eq!(
            resolve_save_dir(Some("relative/dir"), home, None, "zh")
                .unwrap_err()
                .code(),
            E_PATH_NOT_ABSOLUTE
        );
        // home 之外拒绝
        assert_eq!(
            resolve_save_dir(Some("/etc"), home, None, "zh")
                .unwrap_err()
                .code(),
            E_URL_SCHEME_DENIED
        );
    }

    #[test]
    fn file_names_and_result_shapes() {
        assert_eq!(
            screenshot_file_name("20261026-214530", None).unwrap(),
            "hanako-20261026-214530.png"
        );
        assert_eq!(
            screenshot_file_name("20261026-214530", Some("-02-of-03")).unwrap(),
            "hanako-20261026-214530-02-of-03.png"
        );
        assert_eq!(
            screenshot_file_name("2026-10-26", None).unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
        let theme = validate_theme("sakura-light").unwrap();
        let plan = plan_save(
            theme,
            None,
            "20261026-214530",
            Path::new("/Users/u/Desktop/screenshots/zh"),
        )
        .unwrap();
        assert_eq!(
            plan.file_path,
            PathBuf::from("/Users/u/Desktop/screenshots/zh/hanako-20261026-214530.png")
        );
        // 结果形状：preview 不带路径；保存带 filePath/dir；失败以 error 表达
        assert_eq!(
            ScreenshotResult::Preview {
                base64: "AAA".to_string()
            }
            .clone(),
            ScreenshotResult::Preview {
                base64: "AAA".to_string()
            }
        );
        let failure = ScreenshotResult::Failure {
            error: "x".to_string(),
        };
        match failure {
            ScreenshotResult::Failure { error } => assert_eq!(error, "x"),
            _ => panic!(),
        }
    }

    #[test]
    fn capture_is_explicitly_blocked_and_caller_gated() {
        match capture_support() {
            CaptureSupport::Unsupported(reason) => {
                assert!(reason.contains("OQ-04"));
                assert!(reason.contains("wry"));
            }
        }
        assert_eq!(capture_unavailable_error().code(), E_NOT_IMPLEMENTED);
        assert!(authorize_screenshot_caller("main").is_ok());
        for caller in ["settings", "quick-chat", "viewer-1"] {
            assert_eq!(
                authorize_screenshot_caller(caller).unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(CAPTURE_SCALE, 2);
    }
}
