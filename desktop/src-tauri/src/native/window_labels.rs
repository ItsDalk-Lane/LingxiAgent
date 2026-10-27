//! R09-T03 窗口 label 白名单与调用方身份门（A06 / T06 四集合的运行时面）。
//!
//! Tauri 每个窗口的 label 是 capability 分组的唯一键；自定义命令必须校验
//! 「哪个窗口在调用」，不能只信 renderer 自报。Electron 时代靠
//! `BrowserWindow.fromWebContents(event.sender)` 隐式取得调用窗口；Tauri 的
//! `WebviewWindow` 注入参数提供等价身份，本模块把身份判定收敛成纯逻辑，
//! 供 host-core 的 IPC 边界与 evidence 生产者共享。
//!
//! 语法边界（来自 Tauri window label 约束 + 现役窗口命名）：
//! - 固定 label：main / settings / quick-chat / browser-viewer / splash / onboarding；
//! - 动态 label：viewer-<id>、skill-viewer-<id>，后缀只允许 `[a-z0-9_-]`（1..=32）；
//! - 控制字符、空格、`/`、`:`、`*`、超长一律拒绝（杜绝 glob/路径注入）。
//!
//! 本文件不依赖 tauri：host-core 在命令边界把 `window.label()` 传进来即可。

use super::errors::{deny, HostResult, E_WINDOW_LABEL_INVALID, E_WINDOW_NOT_ALLOWED};

pub const MAIN: &str = "main";
pub const SETTINGS: &str = "settings";
pub const QUICK_CHAT: &str = "quick-chat";
pub const BROWSER_VIEWER: &str = "browser-viewer";
pub const SPLASH: &str = "splash";
pub const ONBOARDING: &str = "onboarding";
pub const VIEWER_PREFIX: &str = "viewer-";
pub const SKILL_VIEWER_PREFIX: &str = "skill-viewer-";

pub const MAX_LABEL_LEN: usize = 64;
const MAX_DYNAMIC_SUFFIX_LEN: usize = 32;

/// 宿主管理的窗口角色（与 capabilities/*.json 的 windows glob 一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowRole {
    Main,
    Settings,
    QuickChat,
    BrowserViewer,
    Viewer,
    SkillViewer,
    Splash,
    Onboarding,
    /// 语法合法但不在白名单（例如未来的新窗口未登记 capability）。
    Unknown,
}

impl WindowRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Settings => "settings",
            Self::QuickChat => "quick-chat",
            Self::BrowserViewer => "browser-viewer",
            Self::Viewer => "viewer",
            Self::SkillViewer => "skill-viewer",
            Self::Splash => "splash",
            Self::Onboarding => "onboarding",
            Self::Unknown => "unknown",
        }
    }

    /// 该角色窗口的 capability 文件（T06 四集合比对用）。
    pub fn capability_group(self) -> Option<&'static str> {
        match self {
            Self::Main => Some("main"),
            Self::Settings => Some("settings"),
            Self::QuickChat => Some("quick-chat"),
            Self::BrowserViewer => Some("browser-viewer"),
            Self::Viewer => Some("viewer"),
            Self::SkillViewer => Some("skill-viewer"),
            Self::Splash => Some("splash"),
            Self::Onboarding => Some("onboarding"),
            Self::Unknown => None,
        }
    }
}

fn valid_dynamic_suffix(suffix: &str) -> bool {
    !suffix.is_empty()
        && suffix.len() <= MAX_DYNAMIC_SUFFIX_LEN
        && suffix
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// label → 角色。纯字符串判定，不做大小写折叠（label 区分大小写）。
pub fn role_for_label(label: &str) -> WindowRole {
    match label {
        MAIN => WindowRole::Main,
        SETTINGS => WindowRole::Settings,
        QUICK_CHAT => WindowRole::QuickChat,
        BROWSER_VIEWER => WindowRole::BrowserViewer,
        SPLASH => WindowRole::Splash,
        ONBOARDING => WindowRole::Onboarding,
        _ => {
            if let Some(suffix) = label.strip_prefix(SKILL_VIEWER_PREFIX) {
                if valid_dynamic_suffix(suffix) {
                    return WindowRole::SkillViewer;
                }
            }
            if let Some(suffix) = label.strip_prefix(VIEWER_PREFIX) {
                if valid_dynamic_suffix(suffix) {
                    return WindowRole::Viewer;
                }
            }
            WindowRole::Unknown
        }
    }
}

/// label 语法检查（不要求在白名单内，供「创建新窗口」与「未知窗口」诊断用）。
pub fn validate_label_syntax(label: &str) -> HostResult<()> {
    if label.is_empty() {
        return Err(deny(E_WINDOW_LABEL_INVALID, "窗口 label 不能为空"));
    }
    if label.len() > MAX_LABEL_LEN {
        return Err(deny(
            E_WINDOW_LABEL_INVALID,
            format!("窗口 label 超过 {MAX_LABEL_LEN} 字符"),
        ));
    }
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_');
    if !label.chars().all(allowed) {
        return Err(deny(
            E_WINDOW_LABEL_INVALID,
            "窗口 label 只允许 ASCII 字母/数字/-/_",
        ));
    }
    Ok(())
}

/// 调用方身份门：语法非法 → `WINDOW_LABEL_INVALID`；词法合法但角色不在
/// `allowed` → `WINDOW_NOT_ALLOWED`（两者都是稳定 code，IPC 负向证据直接断言）。
pub fn require_role(caller: &str, allowed: &[WindowRole]) -> HostResult<WindowRole> {
    validate_label_syntax(caller)?;
    let role = role_for_label(caller);
    if role == WindowRole::Unknown {
        return Err(deny(
            E_WINDOW_NOT_ALLOWED,
            format!("未知窗口 label 不允许调用宿主能力: {caller}"),
        ));
    }
    if !allowed.contains(&role) {
        return Err(deny(
            E_WINDOW_NOT_ALLOWED,
            format!("窗口 {}（{}）不允许调用该能力", caller, role.as_str()),
        ));
    }
    Ok(role)
}

/// 精确 label 白名单（splash/onboarding 这类固定窗口用）。
pub fn require_exact_label(caller: &str, allowed: &[&str]) -> HostResult<()> {
    validate_label_syntax(caller)?;
    if allowed.iter().any(|label| *label == caller) {
        return Ok(());
    }
    Err(deny(
        E_WINDOW_NOT_ALLOWED,
        format!("该能力只允许 {allowed:?} 调用，收到 {caller}"),
    ))
}

/// 生成受控动态 label：`viewer-42` / `skill-viewer-7`（只用数字，杜绝注入）。
pub fn generated_label(prefix: &str, id: u64) -> String {
    format!("{prefix}{id}")
}

/// 该角色是否属于「主窗口的从属窗口」（主窗关闭时一起隐藏，见 window_lifecycle）。
pub fn is_secondary(role: WindowRole) -> bool {
    matches!(
        role,
        WindowRole::Settings
            | WindowRole::QuickChat
            | WindowRole::BrowserViewer
            | WindowRole::Viewer
            | WindowRole::SkillViewer
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_WINDOW_LABEL_INVALID, E_WINDOW_NOT_ALLOWED};

    #[test]
    fn fixed_labels_map_to_roles() {
        assert_eq!(role_for_label("main"), WindowRole::Main);
        assert_eq!(role_for_label("settings"), WindowRole::Settings);
        assert_eq!(role_for_label("quick-chat"), WindowRole::QuickChat);
        assert_eq!(role_for_label("browser-viewer"), WindowRole::BrowserViewer);
        assert_eq!(role_for_label("splash"), WindowRole::Splash);
        assert_eq!(role_for_label("onboarding"), WindowRole::Onboarding);
        assert_eq!(role_for_label("Main"), WindowRole::Unknown, "不折叠大小写");
        assert_eq!(role_for_label(""), WindowRole::Unknown);
    }

    #[test]
    fn dynamic_labels_are_strict() {
        assert_eq!(role_for_label("viewer-42"), WindowRole::Viewer);
        assert_eq!(role_for_label("viewer-main"), WindowRole::Viewer);
        assert_eq!(role_for_label("skill-viewer-7"), WindowRole::SkillViewer);
        // 以下都不能被当成合法动态窗口：
        assert_eq!(role_for_label("viewer-"), WindowRole::Unknown);
        assert_eq!(role_for_label("viewer-*"), WindowRole::Unknown, "glob 注入");
        assert_eq!(role_for_label("viewer-A"), WindowRole::Unknown, "大写后缀");
        assert_eq!(
            role_for_label("viewer-a/b"),
            WindowRole::Unknown,
            "路径注入"
        );
        assert_eq!(
            role_for_label(&format!("viewer-{}", "a".repeat(33))),
            WindowRole::Unknown,
            "超长后缀"
        );
        // `skill-viewer-` 不能被 `viewer-` 前缀抢先匹配
        assert_eq!(role_for_label("skill-viewer-x1"), WindowRole::SkillViewer);
    }

    #[test]
    fn syntax_rejects_injection_shapes() {
        for bad in [
            "",
            "main window",
            "viewer-1\n",
            "viewer/1",
            "viewer:1",
            "运 行",
            "*",
        ] {
            assert_eq!(
                validate_label_syntax(bad).unwrap_err().code(),
                E_WINDOW_LABEL_INVALID,
                "应拒绝 {bad:?}"
            );
        }
        assert!(validate_label_syntax(&"a".repeat(MAX_LABEL_LEN)).is_ok());
        assert_eq!(
            validate_label_syntax(&"a".repeat(MAX_LABEL_LEN + 1))
                .unwrap_err()
                .code(),
            E_WINDOW_LABEL_INVALID
        );
    }

    #[test]
    fn require_role_enforces_caller_identity() {
        // 主窗命令不能被设置窗/未知窗口调用
        assert_eq!(
            require_role("main", &[WindowRole::Main]).unwrap(),
            WindowRole::Main
        );
        for caller in ["settings", "quick-chat", "viewer-1", "splash"] {
            assert_eq!(
                require_role(caller, &[WindowRole::Main])
                    .unwrap_err()
                    .code(),
                E_WINDOW_NOT_ALLOWED,
                "{caller} 不应通过 main-only 门"
            );
        }
        assert_eq!(
            require_role("evil window", &[WindowRole::Main])
                .unwrap_err()
                .code(),
            E_WINDOW_LABEL_INVALID
        );
        // 语法合法但未登记角色 → 身份门拒绝（不是语法错误）
        assert_eq!(
            require_role("future-window", &[WindowRole::Main])
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED,
            "字符合法但角色未知 → WINDOW_NOT_ALLOWED"
        );
        // 多角色白名单
        assert!(require_role("settings", &[WindowRole::Main, WindowRole::Settings]).is_ok());
    }

    #[test]
    fn require_exact_label_for_splash_data() {
        assert!(require_exact_label("splash", &[SPLASH, ONBOARDING]).is_ok());
        assert!(require_exact_label("onboarding", &[SPLASH, ONBOARDING]).is_ok());
        assert_eq!(
            require_exact_label("main", &[SPLASH, ONBOARDING])
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED
        );
        assert_eq!(
            require_exact_label("viewer-1", &[SPLASH, ONBOARDING])
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED
        );
    }

    #[test]
    fn generated_labels_round_trip() {
        let label = generated_label(VIEWER_PREFIX, 42);
        assert_eq!(label, "viewer-42");
        assert_eq!(role_for_label(&label), WindowRole::Viewer);
        let skill = generated_label(SKILL_VIEWER_PREFIX, 7);
        assert_eq!(role_for_label(&skill), WindowRole::SkillViewer);
    }

    #[test]
    fn capability_groups_are_one_to_one() {
        let roles = [
            WindowRole::Main,
            WindowRole::Settings,
            WindowRole::QuickChat,
            WindowRole::BrowserViewer,
            WindowRole::Viewer,
            WindowRole::SkillViewer,
            WindowRole::Splash,
            WindowRole::Onboarding,
        ];
        let mut groups: Vec<&str> = Vec::new();
        for role in roles {
            groups.push(role.capability_group().expect("每个角色都有 capability 组"));
            assert!(
                is_secondary(role)
                    || matches!(
                        role,
                        WindowRole::Main | WindowRole::Splash | WindowRole::Onboarding
                    )
            );
        }
        groups.sort_unstable();
        groups.dedup();
        assert_eq!(groups.len(), 8, "capability 组必须一一对应");
        assert!(WindowRole::Unknown.capability_group().is_none());
    }
}
