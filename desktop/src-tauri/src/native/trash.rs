//! R09-T03 回收站（`host:trash`）。
//!
//! 现役 Electron（desktop/main.cjs:6261-6272 + shared/trash-item-path.cjs）：
//! `resolveTrashItemPath` 只接受字符串 + 平台绝对路径（win32 用 win32 语义，
//! 其它用 posix），`lstat` 存在后 `shell.trashItem`，失败返回 false。
//! R06 Tauri 已有 `trash` crate 实现（`trash_item` 命令）；T03 补调用窗口门与
//! 路径归一（R06 的 `main_only` 已限制主窗）。
//!
//! A06：相对路径、Windows 路径在 POSIX 主机、空串、NUL 必须拒绝。

use std::path::PathBuf;

use super::errors::{deny, HostResult, E_PATH_MISSING, E_PATH_NOT_ABSOLUTE, E_WINDOW_NOT_ALLOWED};
use super::window_labels::{require_role, WindowRole};

/// 调用窗口门：只有主窗（书桌删除动作）。
pub fn authorize_trash(caller: &str) -> HostResult<WindowRole> {
    require_role(caller, &[WindowRole::Main])
}

/// `resolveTrashItemPath` 的移植：平台原生绝对路径归一。
/// `platform` 传 `"win32"` 时按 Windows 语义；其余按 POSIX。
pub fn resolve_trash_item_path(raw: &str, platform: &str) -> Option<PathBuf> {
    if raw.is_empty() || raw.len() > 4096 {
        return None;
    }
    if raw.chars().any(|c| c.is_control()) {
        return None;
    }
    if platform == "win32" {
        // 显式按 Windows 语义判定（std::path 跟随宿主平台，mac 上会把盘符路径
        // 判成相对路径）：`C:\...` / `C:/...` / UNC `\\server\share`。
        let bytes = raw.as_bytes();
        let drive_absolute = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/');
        let unc = raw.starts_with("\\\\");
        if drive_absolute || unc {
            Some(std::path::PathBuf::from(raw))
        } else {
            None
        }
    } else if raw.starts_with('/') {
        Some(std::path::PathBuf::from(raw))
    } else {
        None
    }
}

/// 完整校验：窗口 + 路径存在性（lstat）。
pub fn plan_trash(caller: &str, raw: &str, platform: &str) -> HostResult<PathBuf> {
    authorize_trash(caller)?;
    let path = resolve_trash_item_path(raw, platform)
        .ok_or_else(|| deny(E_PATH_NOT_ABSOLUTE, "回收站路径必须是平台绝对路径"))?;
    std::fs::symlink_metadata(&path)
        .map_err(|_| deny(E_PATH_MISSING, "回收站目标不存在或不可访问"))?;
    Ok(path)
}

pub fn caller_denied() -> super::errors::HostError {
    deny(E_WINDOW_NOT_ALLOWED, "回收站动作只允许主窗口调用")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_PATH_MISSING, E_PATH_NOT_ABSOLUTE};

    #[test]
    fn path_resolution_matches_incumbent() {
        assert_eq!(
            resolve_trash_item_path("/Users/u/a.txt", "darwin"),
            Some(PathBuf::from("/Users/u/a.txt"))
        );
        assert_eq!(resolve_trash_item_path("", "darwin"), None);
        assert_eq!(resolve_trash_item_path("relative/a", "darwin"), None);
        assert_eq!(resolve_trash_item_path("/tmp/a\u{0}", "darwin"), None);
        // Windows 盘符路径在 win32 平台语义下是绝对路径
        assert_eq!(
            resolve_trash_item_path("C:\\Users\\u\\a.txt", "win32"),
            Some(PathBuf::from("C:\\Users\\u\\a.txt"))
        );
        // 同一字符串在 POSIX 语义下不是绝对路径
        assert_eq!(
            resolve_trash_item_path("C:\\Users\\u\\a.txt", "darwin"),
            None
        );
    }

    #[test]
    fn plan_trash_checks_existence_and_caller() {
        let existing = std::env::current_exe().unwrap();
        assert!(plan_trash("main", existing.to_str().unwrap(), "darwin").is_ok());
        assert_eq!(
            plan_trash("main", "/definitely/missing-r09", "darwin")
                .unwrap_err()
                .code(),
            E_PATH_MISSING
        );
        assert_eq!(
            plan_trash("main", "relative", "darwin").unwrap_err().code(),
            E_PATH_NOT_ABSOLUTE
        );
        assert_eq!(
            plan_trash("viewer-1", existing.to_str().unwrap(), "darwin")
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED
        );
        assert_eq!(caller_denied().code(), E_WINDOW_NOT_ALLOWED);
        assert_eq!(
            resolve_trash_item_path(&format!("/{}", "a".repeat(4096)), "darwin"),
            None
        );
    }
}
