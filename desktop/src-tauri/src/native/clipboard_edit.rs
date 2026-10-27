//! R09-T03 剪贴板编辑命令（`host:clipboard-edit-command`）。
//!
//! 现役 Electron（desktop/main.cjs:5656-5664）：
//! `run-edit-command` 只接受 `cut/copy/paste/selectAll` 四个枚举值，未知值
//! `throw new Error("Unknown edit command")`；随后调用
//! `event.sender[command]()`（即 focused webContents 的内建编辑动作）。
//! R06 的 Tauri 实现用 mac `objc2 firstResponder tryToPerform`，非 mac 面需
//! 等价适配；T03 只在命令层补「白名单 + 调用窗口门」，不改变编辑语义。
//!
//! A06 重点：renderer 不能把任意命令样式字符串（`delete`、`undo`、
//! `pasteAndMatchStyle`、`toggleDevTools`…）带进宿主动作面。

use super::errors::{deny, HostResult, E_INVALID_ARGUMENT, E_WINDOW_NOT_ALLOWED};
use super::window_labels::{require_role, WindowRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCommand {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl EditCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cut => "cut",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::SelectAll => "selectAll",
        }
    }

    /// macOS firstResponder selector（R06 实现使用）。
    pub fn macos_selector(self) -> &'static str {
        match self {
            Self::Cut => "cut:",
            Self::Copy => "copy:",
            Self::Paste => "paste:",
            Self::SelectAll => "selectAll:",
        }
    }
}

/// 精确白名单解析；未知命令返回 `INVALID_ARGUMENT`（不 return false 静默）。
pub fn parse_edit_command(raw: &str) -> HostResult<EditCommand> {
    match raw {
        "cut" => Ok(EditCommand::Cut),
        "copy" => Ok(EditCommand::Copy),
        "paste" => Ok(EditCommand::Paste),
        "selectAll" => Ok(EditCommand::SelectAll),
        _ => Err(deny(
            E_INVALID_ARGUMENT,
            "Unknown edit command：只允许 cut/copy/paste/selectAll",
        )),
    }
}

/// 调用窗口门：主窗/设置窗输入区（现役消费在 InputContextMenu，设置窗也有输入框）。
pub fn authorize_edit_command(caller: &str) -> HostResult<WindowRole> {
    require_role(caller, &[WindowRole::Main, WindowRole::Settings])
}

/// 完整校验入口：窗口 + 命令。
pub fn plan_edit_command(caller: &str, raw: &str) -> HostResult<EditCommand> {
    authorize_edit_command(caller)?;
    parse_edit_command(raw)
}

/// 非白名单调用窗口的稳定错误。
pub fn caller_denied() -> super::errors::HostError {
    deny(
        E_WINDOW_NOT_ALLOWED,
        "编辑命令只允许主窗口或设置窗口的输入区域调用",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_set_is_exactly_four() {
        assert_eq!(parse_edit_command("cut").unwrap(), EditCommand::Cut);
        assert_eq!(parse_edit_command("copy").unwrap(), EditCommand::Copy);
        assert_eq!(parse_edit_command("paste").unwrap(), EditCommand::Paste);
        assert_eq!(
            parse_edit_command("selectAll").unwrap(),
            EditCommand::SelectAll
        );
        for bad in [
            "",
            "Cut",
            "selectall",
            "delete",
            "undo",
            "redo",
            "pasteAndMatchStyle",
            "toggleDevTools",
            "openDevTools",
            "javascript:alert(1)",
            "cut; rm -rf /",
        ] {
            assert_eq!(
                parse_edit_command(bad).unwrap_err().code(),
                E_INVALID_ARGUMENT,
                "应拒绝 {bad:?}"
            );
        }
    }

    #[test]
    fn selectors_match_r06_mapping() {
        assert_eq!(EditCommand::Cut.macos_selector(), "cut:");
        assert_eq!(EditCommand::Copy.macos_selector(), "copy:");
        assert_eq!(EditCommand::Paste.macos_selector(), "paste:");
        assert_eq!(EditCommand::SelectAll.macos_selector(), "selectAll:");
        assert_eq!(EditCommand::SelectAll.as_str(), "selectAll");
    }

    #[test]
    fn caller_policy() {
        for caller in ["main", "settings"] {
            assert!(plan_edit_command(caller, "copy").is_ok());
        }
        for caller in ["quick-chat", "viewer-1", "browser-viewer"] {
            assert_eq!(
                plan_edit_command(caller, "copy").unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(caller_denied().code(), E_WINDOW_NOT_ALLOWED);
        assert_eq!(
            plan_edit_command("main", "../evil").unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
    }
}
