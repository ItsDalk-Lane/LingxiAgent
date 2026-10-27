//! R09-T03 主题同步（`host:theme-sync`）。
//!
//! 现役 Electron（desktop/main.cjs:817-860、5938-5944）：
//! - renderer 只发「存储值」（`warm-paper`/`auto`/旧别名…），主进程按
//!   `theme-registry-data.json` 解析出具体主题与背景；
//! - `window-theme-changed` 只作用于**发送窗口**：普通窗口 setBackgroundColor，
//!   quick-chat 是透明窗特例（恒 `#00000000`，不刷背景）；
//! - 具体主题决定窗口原生 appearance（深/浅）。
//!
//! Tauri 没有 `setBackgroundColor` 的稳定等价（`Window::set_background_color`
//! 在 2.x 属 unstable 面），因此本模块输出解析结果 + 效果清单：host-core 用
//! `window.set_theme(Some(Dark/Light))` 落地原生 appearance，背景色仅在可用时
//! 应用（quick-chat 透明特例必须跳过）。主题表与 `theme-registry-data.json`
//! 逐项一致；漂移由生产者 `r09_t03_native_matrix` 的真实文件对照断言发现。

use super::errors::{deny, HostResult, E_ENUM_INVALID};
use super::window_labels::{require_role, WindowRole};

pub const THEME_STORAGE_KEY: &str = "lingxi-theme";
pub const DEFAULT_THEME: &str = "warm-paper";
pub const AUTO_LIGHT_DEFAULT: &str = "warm-paper";
pub const AUTO_DARK_DEFAULT: &str = "midnight";
pub const QUICK_CHAT_TRANSPARENT_BACKGROUND: &str = "#00000000";

/// 主题条目（id/背景色/深色标记）。与 `desktop/src/shared/theme-registry-data.json`
/// 的 themes 键和 backgroundColor 一致；`dark` 由背景亮度推导（不含手写猜测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeEntry {
    pub id: &'static str,
    pub background_hex: &'static str,
    pub dark: bool,
}

macro_rules! theme {
    ($id:expr, $bg:expr, $dark:expr) => {
        ThemeEntry {
            id: $id,
            background_hex: $bg,
            dark: $dark,
        }
    };
}

/// 11 个具体主题（顺序与 registry 一致，便于证据逐项对照）。
pub const THEMES: &[ThemeEntry] = &[
    theme!("warm-paper", "#F8F4ED", false),
    theme!("midnight", "#3B4A54", true),
    theme!("high-contrast", "#FAF8F7", false),
    theme!("grass-aroma", "#F5F8F3", false),
    theme!("contemplation", "#F3F5F7", false),
    theme!("absolutely", "#F4F3EE", false),
    theme!("delve", "#FFFFFF", false),
    theme!("deep-think", "#FCFCFD", false),
    theme!("new-warm-paper", "#F5EFE4", false),
    theme!("midnight-contrast", "#26343D", true),
    theme!("coral", "#FDF6EC", false),
];

pub const LEGACY_THEME_ALIASES: &[(&str, &str)] = &[("claude-design", "new-warm-paper")];

/// 6 位十六进制背景色 → `#RRGGBB`；非法返回 None。
pub fn normalize_hex(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let body = trimmed.strip_prefix('#')?;
    if body.len() != 6 || !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", body.to_ascii_uppercase()))
}

#[cfg(test)]
fn is_dark_hex(hex: &str) -> bool {
    let Some(normalized) = normalize_hex(hex) else {
        return false;
    };
    let parse = |offset: usize| {
        u8::from_str_radix(&normalized[offset..offset + 2], 16).expect("hex 已验证")
    };
    let (r, g, b) = (
        f64::from(parse(1)),
        f64::from(parse(3)),
        f64::from(parse(5)),
    );
    // 相对亮度（sRGB 近似）：< 0.5 视为深色主题。
    (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0 < 0.5
}

pub fn theme_by_id(id: &str) -> Option<&'static ThemeEntry> {
    THEMES.iter().find(|entry| entry.id == id)
}

/// `migrateSavedTheme` 的移植：`auto` 原样、旧别名映射、非法 → 默认主题。
pub fn migrate_saved_theme(raw: Option<&str>) -> &'static str {
    let Some(raw) = raw else {
        return DEFAULT_THEME;
    };
    if raw.is_empty() {
        return DEFAULT_THEME;
    }
    if raw == "auto" {
        return "auto";
    }
    if let Some((_, target)) = LEGACY_THEME_ALIASES.iter().find(|(from, _)| *from == raw) {
        return target;
    }
    if theme_by_id(raw).is_some() {
        return theme_by_id(raw)
            .map(|entry| entry.id)
            .unwrap_or(DEFAULT_THEME);
    }
    DEFAULT_THEME
}

/// 存储值 + 系统深色 → 具体主题 + 原生 appearance。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeResolution {
    /// 应写回 localStorage 的值（`auto` 或具体 id）。
    pub stored: &'static str,
    pub concrete: &'static ThemeEntry,
    pub native_dark: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeAppearance {
    Light,
    Dark,
}

impl ThemeResolution {
    pub fn native_appearance(&self) -> NativeAppearance {
        if self.concrete.dark {
            NativeAppearance::Dark
        } else {
            NativeAppearance::Light
        }
    }
}

pub fn resolve_saved_theme(saved: Option<&str>, system_dark: bool) -> ThemeResolution {
    let stored = migrate_saved_theme(saved);
    if stored == "auto" {
        let concrete_id = if system_dark {
            AUTO_DARK_DEFAULT
        } else {
            AUTO_LIGHT_DEFAULT
        };
        ThemeResolution {
            stored: "auto",
            concrete: theme_by_id(concrete_id).expect("auto 默认主题必须在表内"),
            native_dark: system_dark,
        }
    } else {
        let concrete = theme_by_id(stored).expect("migrate 只返回表内 id");
        ThemeResolution {
            stored: concrete.id,
            concrete,
            native_dark: concrete.dark,
        }
    }
}

/// 主题同步效果清单（host-core 在 `window_theme_changed`/`sync_window_theme` 里执行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeSyncPlan {
    pub role: WindowRole,
    pub stored: &'static str,
    pub concrete_id: &'static str,
    pub native_appearance: NativeAppearance,
    /// None = 不设置背景（quick-chat 透明特例）。
    pub background_hex: Option<String>,
    pub transparent_quick_chat: bool,
}

/// 校验调用窗口 + 主题值并产出效果。允许的调用窗口 = 所有产品窗口
/// （theme.ts 在各页面加载；viewer/skill-viewer 只读预览同样需要背景色）。
/// `Unknown`/非法语法由 `require_role` 拒绝。
pub fn plan_theme_sync(
    caller: &str,
    raw_theme: &str,
    system_dark: bool,
) -> HostResult<ThemeSyncPlan> {
    const ALLOWED: &[WindowRole] = &[
        WindowRole::Main,
        WindowRole::Settings,
        WindowRole::QuickChat,
        WindowRole::BrowserViewer,
        WindowRole::Viewer,
        WindowRole::SkillViewer,
        WindowRole::Splash,
        WindowRole::Onboarding,
    ];
    let role = require_role(caller, ALLOWED)?;
    // 空值/非法值在现役语义里不是错误（migrate 回默认）；但含控制字符等
    // 畸形输入必须拒绝，不能静默当作默认值。
    if raw_theme.len() > 64 || raw_theme.chars().any(|c| c.is_control()) {
        return Err(deny(E_ENUM_INVALID, "theme id 形态非法"));
    }
    let resolution = resolve_saved_theme(
        if raw_theme.is_empty() {
            None
        } else {
            Some(raw_theme)
        },
        system_dark,
    );
    let transparent_quick_chat = role == WindowRole::QuickChat;
    Ok(ThemeSyncPlan {
        role,
        stored: resolution.stored,
        concrete_id: resolution.concrete.id,
        native_appearance: resolution.native_appearance(),
        background_hex: if transparent_quick_chat {
            None
        } else {
            Some(resolution.concrete.background_hex.to_owned())
        },
        transparent_quick_chat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_ENUM_INVALID, E_WINDOW_NOT_ALLOWED};

    #[test]
    fn theme_table_matches_registry_shape() {
        assert_eq!(THEMES.len(), 11);
        assert_eq!(
            theme_by_id(DEFAULT_THEME).unwrap().background_hex,
            "#F8F4ED"
        );
        for entry in THEMES {
            assert_eq!(
                normalize_hex(entry.background_hex).as_deref(),
                Some(entry.background_hex.to_uppercase().as_str()),
                "{} 背景色形态",
                entry.id
            );
            assert_eq!(
                entry.dark,
                is_dark_hex(entry.background_hex),
                "{} 的 dark 标记必须与背景亮度一致",
                entry.id
            );
        }
        // 深色主题判定
        for dark in ["midnight", "midnight-contrast"] {
            assert!(theme_by_id(dark).unwrap().dark, "{dark} 必须是深色");
        }
        for light in ["warm-paper", "delve", "high-contrast", "new-warm-paper"] {
            assert!(!theme_by_id(light).unwrap().dark, "{light} 必须是浅色");
        }
    }

    #[test]
    fn saved_theme_migration_matches_incumbent() {
        assert_eq!(migrate_saved_theme(None), DEFAULT_THEME);
        assert_eq!(migrate_saved_theme(Some("")), DEFAULT_THEME);
        assert_eq!(migrate_saved_theme(Some("auto")), "auto");
        assert_eq!(migrate_saved_theme(Some("midnight")), "midnight");
        assert_eq!(migrate_saved_theme(Some("claude-design")), "new-warm-paper");
        assert_eq!(migrate_saved_theme(Some("unknown-theme")), DEFAULT_THEME);
        assert_eq!(
            migrate_saved_theme(Some("AUTO")),
            DEFAULT_THEME,
            "大小写敏感"
        );
    }

    #[test]
    fn auto_resolution_uses_system_darkness() {
        let light = resolve_saved_theme(Some("auto"), false);
        assert_eq!(light.concrete.id, AUTO_LIGHT_DEFAULT);
        assert_eq!(light.stored, "auto");
        assert!(!light.native_dark);
        let dark = resolve_saved_theme(Some("auto"), true);
        assert_eq!(dark.concrete.id, AUTO_DARK_DEFAULT);
        assert!(dark.native_dark);
        assert_eq!(dark.native_appearance(), NativeAppearance::Dark);
        // 显式浅色主题在系统深色下仍是浅色
        let explicit = resolve_saved_theme(Some("coral"), true);
        assert_eq!(explicit.concrete.id, "coral");
        assert!(!explicit.native_dark);
    }

    #[test]
    fn sync_plan_themes_caller_window_only_and_quick_chat_transparent() {
        let plan = plan_theme_sync("main", "midnight", false).unwrap();
        assert_eq!(plan.concrete_id, "midnight");
        assert_eq!(plan.native_appearance, NativeAppearance::Dark);
        assert_eq!(plan.background_hex.as_deref(), Some("#3B4A54"));
        assert!(!plan.transparent_quick_chat);

        let quick = plan_theme_sync("quick-chat", "midnight", false).unwrap();
        assert!(quick.transparent_quick_chat);
        assert_eq!(quick.background_hex, None, "quick-chat 透明窗不刷背景色");

        // 非法主题名按现役语义回落默认值（不是错误）
        let fallback = plan_theme_sync("settings", "not-a-theme", false).unwrap();
        assert_eq!(fallback.concrete_id, DEFAULT_THEME);
        // 但畸形输入必须拒绝
        assert_eq!(
            plan_theme_sync("settings", "\u{0}", false)
                .unwrap_err()
                .code(),
            E_ENUM_INVALID
        );
        assert_eq!(
            plan_theme_sync("evil-window", "midnight", false)
                .unwrap_err()
                .code(),
            E_WINDOW_NOT_ALLOWED
        );
        assert_eq!(
            plan_theme_sync("main window", "midnight", false)
                .unwrap_err()
                .code(),
            crate::native::errors::E_WINDOW_LABEL_INVALID
        );
    }

    #[test]
    fn normalize_hex_is_strict() {
        assert_eq!(normalize_hex("#f8f4ed").unwrap(), "#F8F4ED");
        assert_eq!(normalize_hex("#FFF"), None);
        assert_eq!(normalize_hex("F8F4ED"), None);
        assert_eq!(normalize_hex("#GGGGGG"), None);
        assert_eq!(
            QUICK_CHAT_TRANSPARENT_BACKGROUND, "#00000000",
            "透明背景是 8 位 RGBA，不走主题表"
        );
    }
}
