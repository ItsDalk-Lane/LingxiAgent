//! R09-T03 宿主侧 i18n（`host:i18n-main`）。
//!
//! 现役 Electron（desktop/main.cjs:730-800、5980-5995）：主进程从
//! `LINGXI_HOME/user/preferences.json` 读 `locale`，按固定规则归一（zh-Hant→
//! zh-TW、zh*→zh、ja*→ja、ko*→ko、其余 en），加载 `src/locales/<key>.json` 的
//! `main` 段，`tray.show/settings/repairArtifacts/quit` 与
//! `notification.ready` 用该文案；`locale-changed` 事件到达时重置缓存并重建
//! 托盘菜单。
//!
//! Tauri：Rust 侧内置同样的 5 语言托盘/通知文案（不读 renderer localStorage），
//! 语言变化由 settings_changed("locale-changed") 触发本模块重建菜单模型。

use super::errors::{HostResult, E_INVALID_ARGUMENT};

pub const SUPPORTED_LOCALES: &[&str] = &["zh", "zh-TW", "en", "ja", "ko"];
pub const FALLBACK_LOCALE: &str = "en";

/// 主进程 i18n 只消费的键（托盘 + 首启通知）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayStrings {
    pub show: &'static str,
    pub settings: &'static str,
    pub repair_artifacts: &'static str,
    pub quit: &'static str,
    pub notification_ready: &'static str,
}

/// 与 `desktop/src/locales/*.json` 的 `main.tray.*` / `main.notification.ready`
/// 逐字一致（漂移由 r09_t03_native_matrix 对照真实文件断言）。
pub fn tray_strings(locale: &str) -> TrayStrings {
    match normalize_locale(locale) {
        "zh" => TrayStrings {
            show: "显示 灵犀",
            settings: "设置",
            repair_artifacts: "修复组件…",
            quit: "退出",
            notification_ready: "通知已就绪",
        },
        "zh-TW" => TrayStrings {
            show: "顯示 靈犀",
            settings: "設定",
            repair_artifacts: "修復元件…",
            quit: "結束",
            notification_ready: "通知已就緒",
        },
        "ja" => TrayStrings {
            show: "Lingxi を表示",
            settings: "設定",
            repair_artifacts: "コンポーネントを修復…",
            quit: "終了",
            notification_ready: "通知が有効になりました",
        },
        "ko" => TrayStrings {
            show: "Lingxi 표시",
            settings: "설정",
            repair_artifacts: "구성 요소 복구…",
            quit: "종료",
            notification_ready: "알림이 준비되었어요",
        },
        _ => TrayStrings {
            show: "Show Lingxi",
            settings: "Settings",
            repair_artifacts: "Repair Components…",
            quit: "Quit",
            notification_ready: "Notifications enabled",
        },
    }
}

/// `_resolveLocaleKey` 的移植：不支持/缺失 → zh（主进程默认中文），
/// 但未知非中文语言 → en。
pub fn normalize_locale(raw: &str) -> &'static str {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "zh";
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower == "zh-tw" || lower.starts_with("zh-hant") {
        return "zh-TW";
    }
    if lower.starts_with("zh") {
        return "zh";
    }
    if lower.starts_with("ja") {
        return "ja";
    }
    if lower.starts_with("ko") {
        return "ko";
    }
    if lower.starts_with("en") {
        return "en";
    }
    // 未知语言：主进程现役按 en 兜底文案（locale 文件也只有 5 种）。
    FALLBACK_LOCALE
}

/// 从 preferences 的原始 locale 值（可能缺失/非字符串）解析最终语言键。
pub fn resolve_locale_preference(raw: Option<&str>) -> &'static str {
    match raw {
        None => "zh",
        Some(value) => normalize_locale(value),
    }
}

/// settings_changed("locale-changed") 的宿主侧效果：是否需要重建托盘菜单。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocaleChangePlan {
    pub rebuild_tray_menu: bool,
    pub new_locale: &'static str,
}

pub fn plan_locale_change(raw_locale: Option<&str>) -> HostResult<LocaleChangePlan> {
    if let Some(value) = raw_locale {
        if value.len() > 64 || value.chars().any(|c| c.is_control()) {
            return Err(super::errors::deny(E_INVALID_ARGUMENT, "locale 形态非法"));
        }
    }
    let new_locale = resolve_locale_preference(raw_locale);
    Ok(LocaleChangePlan {
        rebuild_tray_menu: true,
        new_locale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::E_INVALID_ARGUMENT;

    #[test]
    fn locale_resolution_matches_incumbent() {
        assert_eq!(normalize_locale(""), "zh");
        assert_eq!(normalize_locale("zh"), "zh");
        assert_eq!(normalize_locale("zh-CN"), "zh");
        assert_eq!(normalize_locale("zh-Hans"), "zh");
        assert_eq!(normalize_locale("zh-TW"), "zh-TW");
        assert_eq!(normalize_locale("zh-Hant"), "zh-TW");
        assert_eq!(normalize_locale("ja-JP"), "ja");
        assert_eq!(normalize_locale("ko-KR"), "ko");
        assert_eq!(normalize_locale("en-US"), "en");
        assert_eq!(normalize_locale("fr-FR"), "en", "未知语言 → en 兜底");
        assert_eq!(normalize_locale(" XX "), "en");
        assert_eq!(resolve_locale_preference(None), "zh");
        assert_eq!(resolve_locale_preference(Some("ja")), "ja");
    }

    #[test]
    fn tray_strings_cover_five_locales_verbatim() {
        let zh = tray_strings("zh-CN");
        assert_eq!(zh.show, "显示 灵犀");
        assert_eq!(zh.settings, "设置");
        assert_eq!(zh.repair_artifacts, "修复组件…");
        assert_eq!(zh.quit, "退出");
        assert_eq!(zh.notification_ready, "通知已就绪");

        assert_eq!(tray_strings("zh-TW").quit, "結束");
        assert_eq!(tray_strings("ja").show, "Lingxi を表示");
        assert_eq!(tray_strings("ko").settings, "설정");
        assert_eq!(tray_strings("en").repair_artifacts, "Repair Components…");
        // 5 种文案互不相同（show 字段）
        let mut shows: Vec<&str> = SUPPORTED_LOCALES
            .iter()
            .map(|locale| tray_strings(locale).show)
            .collect();
        shows.sort_unstable();
        shows.dedup();
        assert_eq!(shows.len(), 5);
    }

    #[test]
    fn locale_change_rebuilds_tray_menu() {
        let plan = plan_locale_change(Some("ja-JP")).unwrap();
        assert!(plan.rebuild_tray_menu);
        assert_eq!(plan.new_locale, "ja");
        assert_eq!(plan_locale_change(None).unwrap().new_locale, "zh");
        assert_eq!(
            plan_locale_change(Some("bad\u{0}locale"))
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
    }
}
