//! R09-T03 拖出文件（`host:drag-out`）。
//!
//! 现役 Electron（desktop/main.cjs:6238-6255）：`start-drag` 接受单个字符串或
//! 字符串数组；用 `app.getFileIcon(paths[0])` 取图标，失败用 1x1 透明 PNG 兜底
//! （macOS 要求 icon 非空）；单文件 `startDrag({file})`，多文件
//! `startDrag({files})`。
//!
//! Tauri：`Window::start_dragging`（多文件/图标平台语义需实测；macOS 需非空
//! 图标）。本模块校验 renderer 传入路径（绝对/数量/控制字符），并输出拖动形状；
//! 图标生成由 host-core 用打包的 fallback PNG 或 file-icon 等价实现。

use super::errors::{
    deny, HostResult, E_INVALID_ARGUMENT, E_PATH_NOT_ABSOLUTE, E_WINDOW_NOT_ALLOWED,
};
use super::window_labels::{require_role, WindowRole};

pub const MAX_DRAG_PATHS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DragShape {
    Single(String),
    Multiple(Vec<String>),
}

/// 拖动请求：绝对路径列表（保序去重）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragPayload {
    pub paths: Vec<String>,
    pub shape: DragShape,
}

/// 调用窗口门：现役消费者全部在主窗（书桌树/聊天消息附件）；只读窗口拒绝。
pub fn authorize_start_drag(caller: &str) -> HostResult<WindowRole> {
    require_role(caller, &[WindowRole::Main])
}

/// 校验并归一拖动路径。renderer 传单个字符串时由 host-core 包成单元素数组。
pub fn normalize_drag_paths(paths: &[String]) -> HostResult<DragPayload> {
    if paths.is_empty() {
        return Err(deny(E_INVALID_ARGUMENT, "拖动至少需要一个路径"));
    }
    if paths.len() > MAX_DRAG_PATHS {
        return Err(deny(E_INVALID_ARGUMENT, "一次拖动路径数量超限"));
    }
    let mut normalized: Vec<String> = Vec::new();
    for raw in paths {
        if raw.is_empty() {
            return Err(deny(E_INVALID_ARGUMENT, "拖动路径不能为空"));
        }
        if raw.len() > 4096 {
            return Err(deny(E_INVALID_ARGUMENT, "拖动路径超长"));
        }
        if raw.chars().any(|c| c.is_control()) {
            return Err(deny(E_INVALID_ARGUMENT, "拖动路径含控制字符"));
        }
        if !std::path::Path::new(raw).is_absolute() {
            return Err(deny(
                E_PATH_NOT_ABSOLUTE,
                "拖动路径必须是绝对路径（renderer 不能拖出任意相对路径）",
            ));
        }
        if !normalized.contains(raw) {
            normalized.push(raw.clone());
        }
    }
    let shape = if normalized.len() == 1 {
        DragShape::Single(normalized[0].clone())
    } else {
        DragShape::Multiple(normalized.clone())
    };
    Ok(DragPayload {
        paths: normalized,
        shape,
    })
}

/// 完整校验入口：窗口 + 路径。
pub fn plan_start_drag(caller: &str, paths: &[String]) -> HostResult<DragPayload> {
    authorize_start_drag(caller)?;
    normalize_drag_paths(paths)
}

/// 现役 fallback（macOS icon 非空要求）：调用方需嵌入该 1x1 透明 PNG。
/// 这里只给不变量说明，不存字节（资源由 host-core 打包加载）。
pub const FALLBACK_ICON_REQUIRED: bool = true;

pub fn caller_denied() -> super::errors::HostError {
    deny(E_WINDOW_NOT_ALLOWED, "拖出文件只允许主窗口调用")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_and_multiple_shapes() {
        let single = normalize_drag_paths(&["/tmp/a.txt".to_string()]).unwrap();
        assert_eq!(single.shape, DragShape::Single("/tmp/a.txt".to_string()));
        let multi =
            normalize_drag_paths(&["/tmp/a.txt".to_string(), "/tmp/b.txt".to_string()]).unwrap();
        assert_eq!(
            multi.shape,
            DragShape::Multiple(vec!["/tmp/a.txt".to_string(), "/tmp/b.txt".to_string()])
        );
        // 去重后只剩一个 → 单文件形状
        let dedupe =
            normalize_drag_paths(&["/tmp/a.txt".to_string(), "/tmp/a.txt".to_string()]).unwrap();
        assert_eq!(dedupe.paths, vec!["/tmp/a.txt"]);
        assert_eq!(dedupe.shape, DragShape::Single("/tmp/a.txt".to_string()));
    }

    #[test]
    fn invalid_paths_are_rejected() {
        for bad in [
            vec![],
            vec!["".to_string()],
            vec!["relative/a.txt".to_string()],
            vec!["/tmp/a\u{0}.txt".to_string()],
            vec![format!("/{}", "a".repeat(4096))],
        ] {
            assert!(normalize_drag_paths(&bad).is_err(), "应拒绝 {bad:?}");
        }
        let too_many: Vec<String> = (0..=MAX_DRAG_PATHS)
            .map(|index| format!("/tmp/f{index}"))
            .collect();
        assert_eq!(
            normalize_drag_paths(&too_many).unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
    }

    #[test]
    fn caller_policy_is_main_only() {
        assert!(plan_start_drag("main", &["/tmp/a".to_string()]).is_ok());
        for caller in ["settings", "quick-chat", "viewer-1", "browser-viewer"] {
            assert_eq!(
                plan_start_drag(caller, &["/tmp/a".to_string()])
                    .unwrap_err()
                    .code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(caller_denied().code(), E_WINDOW_NOT_ALLOWED);
    }

    #[test]
    fn fallback_icon_policy_is_documented_as_required() {
        assert!(FALLBACK_ICON_REQUIRED);
    }
}
