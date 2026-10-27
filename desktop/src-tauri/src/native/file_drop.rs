//! R09-T03 文件拖入路径（`host:file-drop-paths`）。
//!
//! 现役 Electron：preload `getFilePath(file) => webUtils.getPathForFile(file)`
//! （`webUtils` 只在 Electron 存在）。Tauri/Wry 没有 `File.path`/`webUtils` 等价，
//! 但 `onDragDropEvent` 直接给**绝对路径**；DesktopHost-Tauri 适配层把 drop
//! 事件转成现有调用面（`MainContent.tsx`/`SkillsTab.tsx`/`markdown-cover-drop`
//! 等消费 File 对象的地方需要宿主适配，不改业务语义）。
//!
//! 本模块校验 drop 事件里的路径（绝对、无控制字符、数量/长度上限、`file://`
//! 之类的非路径形态拒绝——Tauri 给的是原生路径，出现 URI 即说明事件形态被误用）。

use super::errors::{
    deny, HostResult, E_INVALID_ARGUMENT, E_PATH_NOT_ABSOLUTE, E_WINDOW_NOT_ALLOWED,
};
use super::window_labels::{require_role, WindowRole};

pub const MAX_DROP_PATHS: usize = 256;

/// 允许接收 drop 的窗口：主窗、设置窗、quick-chat、browser-viewer（drop 到页面
/// 的输入区/技能页/对话区）。只读 viewer 不接收文件（避免把文件写回预览）。
pub fn authorize_drop_target(caller: &str) -> HostResult<WindowRole> {
    require_role(
        caller,
        &[
            WindowRole::Main,
            WindowRole::Settings,
            WindowRole::QuickChat,
            WindowRole::BrowserViewer,
        ],
    )
}

/// 归一 drop 路径列表（保序去重）。
pub fn normalize_drop_paths(raw: &[String]) -> HostResult<Vec<String>> {
    if raw.is_empty() {
        return Err(deny(E_INVALID_ARGUMENT, "drop 事件未携带路径"));
    }
    if raw.len() > MAX_DROP_PATHS {
        return Err(deny(E_INVALID_ARGUMENT, "一次 drop 路径数量超限"));
    }
    let mut paths: Vec<String> = Vec::new();
    for value in raw {
        if value.is_empty() || value.len() > 4096 {
            return Err(deny(E_INVALID_ARGUMENT, "drop 路径为空或超长"));
        }
        if value.chars().any(|c| c.is_control()) {
            return Err(deny(E_INVALID_ARGUMENT, "drop 路径含控制字符"));
        }
        // Tauri 的 drag-drop payload 是原生绝对路径；URI 形态一律拒绝。
        if value.contains("://") || value.starts_with("file:") {
            return Err(deny(
                E_INVALID_ARGUMENT,
                "drop 路径不能是 URI（宿主适配层应传原生路径）",
            ));
        }
        if !std::path::Path::new(value).is_absolute() {
            return Err(deny(E_PATH_NOT_ABSOLUTE, "drop 路径必须是绝对路径"));
        }
        if !paths.contains(value) {
            paths.push(value.clone());
        }
    }
    Ok(paths)
}

/// `getFilePath(file)` 在 Tauri 的等价物：**没有**（File 对象不暴露路径）。
/// 显式返回 null 的行为由 TS 片段登记理由，消费侧必须走 drop 适配层；
/// Rust 侧只保证事件路径形态。此函数作为契约说明与证据锚点。
pub const FILE_OBJECT_PATH_UNAVAILABLE: bool = true;

pub fn drop_caller_denied() -> super::errors::HostError {
    deny(E_WINDOW_NOT_ALLOWED, "该窗口不接收文件拖入事件")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::E_WINDOW_NOT_ALLOWED;

    #[test]
    fn drop_paths_are_native_absolute_and_deduped() {
        let paths = normalize_drop_paths(&[
            "/Users/u/Desktop/a.txt".to_string(),
            "/Users/u/Desktop/a.txt".to_string(),
            "/Users/u/Desktop/目录 文件.pdf".to_string(),
        ])
        .unwrap();
        assert_eq!(
            paths,
            vec![
                "/Users/u/Desktop/a.txt".to_string(),
                "/Users/u/Desktop/目录 文件.pdf".to_string()
            ]
        );
    }

    #[test]
    fn uri_and_relative_and_control_are_rejected() {
        for bad in [
            "file:///Users/u/a.txt",
            "https://example.com/a",
            "relative/a.txt",
            "/tmp/a\u{0}.txt",
            "",
        ] {
            assert!(
                normalize_drop_paths(&[bad.to_string()]).is_err(),
                "应拒绝 {bad:?}"
            );
        }
        assert_eq!(
            normalize_drop_paths(&[]).unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
        let too_many: Vec<String> = (0..=MAX_DROP_PATHS)
            .map(|index| format!("/tmp/f{index}"))
            .collect();
        assert_eq!(
            normalize_drop_paths(&too_many).unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
    }

    #[test]
    fn caller_policy() {
        for caller in ["main", "settings", "quick-chat", "browser-viewer"] {
            assert!(authorize_drop_target(caller).is_ok());
        }
        for caller in ["viewer-1", "skill-viewer-1", "splash", "onboarding"] {
            assert_eq!(
                authorize_drop_target(caller).unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(drop_caller_denied().code(), E_WINDOW_NOT_ALLOWED);
    }

    #[test]
    fn file_object_path_is_unavailable_contract() {
        assert!(FILE_OBJECT_PATH_UNAVAILABLE);
    }
}
