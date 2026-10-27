//! R09-T03 系统文件对话框（`host:file-dialogs`）。
//!
//! 现役 Electron（desktop/main.cjs:6011-6073）：
//! - `select-folder`：picker；取消 → null；
//! - `select-files`：`multiple` 语义；取消 → []；调用窗口是 sender；
//! - `select-skill`：只接受技能目录或 `.zip`/`.skill` 文件（现役：目录 + 文件混合
//!   选择后再判定）；取消 → null；
//! - 所有对话框以调用窗口为 parent（模态归属）。
//!
//! Tauri：`tauri-plugin-dialog` 的 callback API（R01 spike 实测：**禁用 blocking
//! 变体**——外部取消后不兑现；自动化取消用 Cmd+period）。本模块给参数/结果
//! 归一与调用窗口门；插件回调由 host-core 接线。

use super::errors::{
    deny, HostResult, E_INVALID_ARGUMENT, E_PATH_NOT_ABSOLUTE, E_WINDOW_NOT_ALLOWED,
};
use super::window_labels::{require_role, WindowRole};

/// 技能选择接受的扩展名（小写，含点）。
pub const SKILL_FILE_EXTENSIONS: &[&str] = &[".zip", ".skill"];
pub const MAX_SELECTED_PATHS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Folder,
    Files { multiple: bool },
    Skill,
}

/// 调用窗口门：主窗（输入区附件/知识库）与设置窗（技能页）都允许；只读窗口拒绝。
pub fn authorize_dialog(caller: &str, kind: DialogKind) -> HostResult<WindowRole> {
    require_role(caller, &[WindowRole::Main, WindowRole::Settings]).map(|role| {
        let _ = kind;
        role
    })
}

/// 对话框选项（host-core 映射到 plugin dialog builder）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogOptions {
    pub multiple: bool,
    /// 只允许目录。
    pub directory_only: bool,
    /// 允许的文件扩展名（空 = 不限制；仅 Skill 使用）。
    pub extensions: &'static [&'static str],
}

pub fn dialog_options(kind: DialogKind) -> DialogOptions {
    match kind {
        DialogKind::Folder => DialogOptions {
            multiple: false,
            directory_only: true,
            extensions: &[],
        },
        DialogKind::Files { multiple } => DialogOptions {
            multiple,
            directory_only: false,
            extensions: &[],
        },
        DialogKind::Skill => DialogOptions {
            multiple: false,
            directory_only: false,
            extensions: SKILL_FILE_EXTENSIONS,
        },
    }
}

/// 取消回调的统一结果形状（与现役 IPC 返回一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogResult {
    /// 目录选择取消（现有调用面以 null 表示）。
    FolderCancelled,
    Files(Vec<String>),
    /// 技能选择取消（现有调用面以 null 表示）。
    SkillCancelled,
}

pub fn cancelled_result(kind: DialogKind) -> DialogResult {
    match kind {
        DialogKind::Folder => DialogResult::FolderCancelled,
        DialogKind::Files { .. } => DialogResult::Files(Vec::new()),
        DialogKind::Skill => DialogResult::SkillCancelled,
    }
}

/// 结果路径归一：只允许绝对路径、去重、保序、数量受限；相对/控制字符一律拒绝
/// （对话框不该返回这些，出现即视为边界被破坏，不能静默接受）。
pub fn normalize_dialog_paths(paths: &[String]) -> HostResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for raw in paths {
        if raw.is_empty() || raw.len() > 4096 {
            return Err(deny(E_INVALID_ARGUMENT, "对话框返回了空/超长路径"));
        }
        if raw.chars().any(|c| c.is_control()) {
            return Err(deny(E_INVALID_ARGUMENT, "对话框返回路径含控制字符"));
        }
        if !std::path::Path::new(raw).is_absolute() {
            return Err(deny(
                E_PATH_NOT_ABSOLUTE,
                "对话框返回了相对路径（系统对话框不应如此）",
            ));
        }
        if !out.contains(raw) {
            out.push(raw.clone());
        }
        if out.len() > MAX_SELECTED_PATHS {
            return Err(deny(E_INVALID_ARGUMENT, "一次选择的路径数量超限"));
        }
    }
    Ok(out)
}

/// 技能选择结果：目录或指定扩展名文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSelection {
    pub path: String,
    pub is_directory: bool,
}

pub fn skill_extension_allowed(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    SKILL_FILE_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

/// 校验技能选择：空/相对拒绝；非目录时必须是指定扩展名。
/// `is_directory` 由宿主 `std::fs` 判定后传入（本函数不做 IO）。
pub fn normalize_skill_selection(path: &str, is_directory: bool) -> HostResult<SkillSelection> {
    if path.is_empty() || path.len() > 4096 || path.chars().any(|c| c.is_control()) {
        return Err(deny(E_INVALID_ARGUMENT, "技能路径非法"));
    }
    if !std::path::Path::new(path).is_absolute() {
        return Err(deny(E_PATH_NOT_ABSOLUTE, "技能路径必须是绝对路径"));
    }
    if !is_directory && !skill_extension_allowed(path) {
        return Err(deny(
            E_INVALID_ARGUMENT,
            "技能选择只接受目录或 .zip/.skill 文件",
        ));
    }
    Ok(SkillSelection {
        path: path.to_owned(),
        is_directory,
    })
}

/// `select_files` 的 multiple 参数归一：缺省 true（现役 `multiple.unwrap_or(true)`）。
pub fn normalize_multiple_flag(raw: Option<bool>) -> bool {
    raw.unwrap_or(true)
}

/// 非主窗/设置窗调用时的稳定拒绝。
pub fn dialog_caller_denied() -> super::errors::HostError {
    deny(E_WINDOW_NOT_ALLOWED, "文件对话框只允许主窗口或设置窗口调用")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_policy() {
        for caller in ["main", "settings"] {
            for kind in [
                DialogKind::Folder,
                DialogKind::Files { multiple: true },
                DialogKind::Skill,
            ] {
                assert!(authorize_dialog(caller, kind).is_ok());
            }
        }
        for caller in ["quick-chat", "viewer-1", "browser-viewer", "skill-viewer-1"] {
            assert_eq!(
                authorize_dialog(caller, DialogKind::Folder)
                    .unwrap_err()
                    .code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(dialog_caller_denied().code(), E_WINDOW_NOT_ALLOWED);
    }

    #[test]
    fn dialog_options_are_explicit() {
        let folder = dialog_options(DialogKind::Folder);
        assert!(folder.directory_only);
        assert!(!folder.multiple);
        let files = dialog_options(DialogKind::Files { multiple: false });
        assert!(!files.directory_only);
        assert!(!files.multiple);
        let skill = dialog_options(DialogKind::Skill);
        assert_eq!(skill.extensions, SKILL_FILE_EXTENSIONS);
        assert!(!skill.multiple);
        // multiple 缺省 true
        assert!(normalize_multiple_flag(None));
        assert!(!normalize_multiple_flag(Some(false)));
    }

    #[test]
    fn cancel_results_match_incumbent_shapes() {
        assert_eq!(
            cancelled_result(DialogKind::Folder),
            DialogResult::FolderCancelled
        );
        assert_eq!(
            cancelled_result(DialogKind::Files { multiple: true }),
            DialogResult::Files(Vec::new())
        );
        assert_eq!(
            cancelled_result(DialogKind::Skill),
            DialogResult::SkillCancelled
        );
    }

    #[test]
    fn paths_are_absolute_deduped_bounded() {
        let paths = normalize_dialog_paths(&[
            "/tmp/a.txt".to_string(),
            "/tmp/a.txt".to_string(),
            "/tmp/b.txt".to_string(),
        ])
        .unwrap();
        assert_eq!(paths, vec!["/tmp/a.txt", "/tmp/b.txt"]);
        for bad in ["", "relative/a.txt", "/tmp/a\u{0}.txt"] {
            assert!(
                normalize_dialog_paths(&[bad.to_string()]).is_err(),
                "应拒绝 {bad:?}"
            );
        }
        let many: Vec<String> = (0..=MAX_SELECTED_PATHS)
            .map(|index| format!("/tmp/f{index}"))
            .collect();
        assert_eq!(
            normalize_dialog_paths(&many).unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
    }

    #[test]
    fn skill_selection_rules() {
        assert_eq!(
            normalize_skill_selection("/Users/u/skills/demo", true).unwrap(),
            SkillSelection {
                path: "/Users/u/skills/demo".to_string(),
                is_directory: true
            }
        );
        assert!(normalize_skill_selection("/tmp/demo.zip", false).is_ok());
        assert!(normalize_skill_selection("/tmp/demo.SKILL", false).is_ok());
        for bad in ["/tmp/demo.txt", "/tmp/demo"] {
            assert_eq!(
                normalize_skill_selection(bad, false).unwrap_err().code(),
                E_INVALID_ARGUMENT,
                "应拒绝非技能文件 {bad}"
            );
        }
        assert_eq!(
            normalize_skill_selection("relative.zip", false)
                .unwrap_err()
                .code(),
            E_PATH_NOT_ABSOLUTE
        );
        assert!(skill_extension_allowed("/tmp/a.zip"));
        assert!(!skill_extension_allowed("/tmp/a.zip.sig"));
    }
}
