//! R09-T03 全局快捷键（`host:global-shortcut`）。
//!
//! 现役 Electron（desktop/main.cjs:211-258、266-283、3037-3041、5695-5699）：
//! - 快捷键来自 `preferences.json` 的 `keybindings["quick-chat.toggle"]`（最多 3 组），
//!   无覆盖时回落 legacy `quick_chat.shortcut`，再回落默认 `Alt+Space`；
//! - `normalizeBinding` 规则：NBSP→空格、`Spacebar`→`Space`、修饰符别名归一
//!   （ctrl→Control、cmd→Command、CommandOrControl 等）、单字母大写；
//! - 注册时先注销不再需要的旧组、保留仍有效的旧组；任一组失败返回
//!   `{ok:false, shortcut: failures[0], failures, error:"shortcut is unavailable"}`；
//! - `keybindings:test-register`：已由本应用注册 → `{ok:true,self:true}`；
//!   否则注册成功后立即注销；失败 → `{ok:false,error:"shortcut is unavailable"}`；
//! - 不写 preferences（写者唯一 server，宿主只读）。
//!
//! Tauri：`tauri-plugin-global-shortcut` 仅 Rust 侧调用（前端 capability 零权限）。
//! 本模块给归一/校验/注册计划/结果形状；插件调用由 host-core 按注册清单接线。

use super::errors::{deny, HostResult, E_ENUM_INVALID, E_INVALID_ARGUMENT};

pub const MAX_BINDINGS_PER_COMMAND: usize = 3;
pub const DEFAULT_QUICK_CHAT_SHORTCUT: &str = "Alt+Space";
pub const QUICK_CHAT_TOGGLE_COMMAND: &str = "quick-chat.toggle";
pub const MAX_SHORTCUT_LEN: usize = 64;
/// Tauri/Electron accelerator 允许的修饰符集合（归一后）。
pub const MODIFIERS: &[&str] = &[
    "Shift",
    "Control",
    "Alt",
    "Meta",
    "Command",
    "CommandOrControl",
    "Super",
];

/// 与 `shared/keybindings-preferences.cjs::normalizeBinding` 同规则的归一实现。
pub fn normalize_binding(value: &str) -> String {
    let raw = value.replace('\u{00A0}', " ");
    if raw.trim().is_empty() {
        return String::new();
    }
    raw.split('+')
        .map(|part| {
            if part == "Spacebar" {
                return "Space".to_string();
            }
            if !part.is_empty() && part.trim().is_empty() {
                return "Space".to_string();
            }
            let trimmed = part.trim();
            if trimmed.is_empty() {
                return String::new();
            }
            let alias = match trimmed.to_ascii_lowercase().as_str() {
                "shift" => "Shift",
                "control" | "ctrl" => "Control",
                "alt" | "option" => "Alt",
                "meta" | "super" | "win" => "Super",
                "cmd" | "command" => "Command",
                "commandorcontrol" | "cmdorctrl" | "ctrlorcommand" => "CommandOrControl",
                _ => "",
            };
            if !alias.is_empty() {
                return alias.to_string();
            }
            if trimmed.eq_ignore_ascii_case("space") {
                return "Space".to_string();
            }
            if trimmed == "Esc" {
                return "Escape".to_string();
            }
            if trimmed.chars().count() == 1 {
                return trimmed.to_ascii_uppercase();
            }
            trimmed.to_string()
        })
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("+")
}

/// 校验归一后的 accelerator：1..=3 个修饰符 + 恰好 1 个主键，无重复修饰符。
///
/// 主键白名单覆盖现役可配置键位与 Electron accelerator 常用键；未知主键拒绝
/// （避免把 renderer 任意字符串直接送进系统注册面）。
pub fn validate_accelerator(accelerator: &str) -> HostResult<()> {
    if accelerator.is_empty() {
        return Err(deny(E_INVALID_ARGUMENT, "快捷键不能为空"));
    }
    if accelerator.len() > MAX_SHORTCUT_LEN {
        return Err(deny(E_INVALID_ARGUMENT, "快捷键超过长度上限"));
    }
    if accelerator.chars().any(|c| c.is_control()) {
        return Err(deny(E_INVALID_ARGUMENT, "快捷键含控制字符"));
    }
    let parts: Vec<&str> = accelerator.split('+').collect();
    let mut modifiers: Vec<&str> = Vec::new();
    let mut keys: Vec<&str> = Vec::new();
    for part in &parts {
        if MODIFIERS.contains(part) {
            if modifiers.contains(part) {
                return Err(deny(E_ENUM_INVALID, format!("快捷键修饰符重复: {part}")));
            }
            modifiers.push(part);
        } else {
            keys.push(part);
        }
    }
    if modifiers.len() > 3 {
        return Err(deny(E_INVALID_ARGUMENT, "快捷键修饰符最多 3 个"));
    }
    if keys.len() != 1 {
        return Err(deny(E_INVALID_ARGUMENT, "快捷键必须恰好包含一个主键"));
    }
    if !is_known_key(keys[0]) {
        return Err(deny(E_ENUM_INVALID, format!("未知快捷键主键: {}", keys[0])));
    }
    Ok(())
}

fn is_known_key(key: &str) -> bool {
    let known = [
        "Space",
        "Enter",
        "Return",
        "Tab",
        "Escape",
        "Backspace",
        "Delete",
        "Insert",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Up",
        "Down",
        "Left",
        "Right",
        "Plus",
        "Minus",
        "Comma",
        "Period",
        "Slash",
        "Backslash",
        "Semicolon",
        "Quote",
        "BracketLeft",
        "BracketRight",
        "Backquote",
    ];
    if known.contains(&key) {
        return true;
    }
    // A-Z、0-9、F1-F24
    let chars: Vec<char> = key.chars().collect();
    if chars.len() == 1 {
        if chars[0].is_ascii_uppercase() || chars[0].is_ascii_digit() {
            return true;
        }
        // 标点主键（Electron accelerator 允许 `CmdOrCtrl+,` 这类写法）；
        // 接线时映射到 platform Code（见注册清单）。
        return matches!(
            chars[0],
            ',' | '.' | '/' | '\\' | ';' | '\'' | '[' | ']' | '`' | '-' | '='
        );
    }
    if let Some(rest) = key.strip_prefix('F') {
        if let Ok(number) = rest.parse::<u8>() {
            return (1..=24).contains(&number);
        }
    }
    false
}

/// 从偏好值计算出有效快捷键列表（含回落链）。
/// `override_value` 是 `prefs.keybindings["quick-chat.toggle"]` 的原始 JSON 值
/// （host-core 用 serde_json 取值后以字符串数组传入，非字符串项被忽略）。
pub fn select_quick_chat_shortcuts(
    override_values: Option<&[String]>,
    legacy_shortcut: Option<&str>,
) -> HostResult<Vec<String>> {
    let mut selected: Vec<String> = Vec::new();
    if let Some(values) = override_values {
        for value in values {
            let normalized = normalize_binding(value);
            if normalized.is_empty() || selected.contains(&normalized) {
                continue;
            }
            selected.push(normalized);
            if selected.len() == MAX_BINDINGS_PER_COMMAND {
                break;
            }
        }
    }
    if selected.is_empty() {
        if let Some(legacy) = legacy_shortcut {
            let normalized = normalize_binding(legacy);
            if !normalized.is_empty() {
                selected.push(normalized);
            }
        }
    }
    if selected.is_empty() {
        selected.push(DEFAULT_QUICK_CHAT_SHORTCUT.to_owned());
    }
    for shortcut in &selected {
        validate_accelerator(shortcut)?;
    }
    Ok(selected)
}

/// 注册差异计划：先注销不再需要的、再注册缺失的（保留仍有效旧组，现役语义）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationPlan {
    pub unregister: Vec<String>,
    pub register: Vec<String>,
    pub unchanged: Vec<String>,
}

pub fn plan_registration(current: &[String], desired: &[String]) -> RegistrationPlan {
    let mut unregister = Vec::new();
    let mut unchanged = Vec::new();
    for shortcut in current {
        if desired.contains(shortcut) {
            unchanged.push(shortcut.clone());
        } else {
            unregister.push(shortcut.clone());
        }
    }
    let register = desired
        .iter()
        .filter(|shortcut| !current.contains(shortcut))
        .cloned()
        .collect();
    RegistrationPlan {
        unregister,
        register,
        unchanged,
    }
}

/// 注册结果回执（与现役 `{ok, shortcut, failures?, shortcuts?, error?}` 同形）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistrationResult {
    pub ok: bool,
    pub shortcut: String,
    pub shortcuts: Vec<String>,
    pub failures: Vec<String>,
    pub error: Option<String>,
}

impl RegistrationResult {
    pub fn success(shortcuts: Vec<String>) -> Self {
        Self {
            ok: true,
            shortcut: shortcuts.first().cloned().unwrap_or_default(),
            shortcuts,
            failures: Vec::new(),
            error: None,
        }
    }

    pub fn unavailable(failures: Vec<String>) -> Self {
        Self {
            ok: false,
            shortcut: failures.first().cloned().unwrap_or_default(),
            shortcuts: Vec::new(),
            failures,
            error: Some("shortcut is unavailable".to_owned()),
        }
    }
}

/// 汇总每个期望键位的注册结果（`(shortcut, registered_ok)`）。
pub fn summarize_registration(
    desired: &[String],
    results: &[(String, bool)],
) -> RegistrationResult {
    let failures: Vec<String> = results
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(shortcut, _)| shortcut.clone())
        .collect();
    if failures.is_empty() && !desired.is_empty() {
        RegistrationResult::success(desired.to_vec())
    } else {
        RegistrationResult::unavailable(failures)
    }
}

/// `keybindings:test-register` 的探测计划：已注册 → self:true；否则注册后立即注销。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRegisterPlan {
    pub shortcut: String,
    pub already_registered: bool,
    pub must_unregister_after: bool,
}

pub fn plan_test_register(raw_shortcut: &str) -> HostResult<TestRegisterPlan> {
    let normalized = normalize_binding(raw_shortcut);
    if normalized.is_empty() {
        return Err(deny(E_INVALID_ARGUMENT, "invalid shortcut"));
    }
    validate_accelerator(&normalized)?;
    Ok(TestRegisterPlan {
        shortcut: normalized,
        already_registered: false,
        must_unregister_after: true,
    })
}

/// `quick_chat_shortcut_status` 形状。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutStatus {
    pub shortcut: String,
    pub registered: bool,
    pub shortcuts: Vec<String>,
}

pub fn shortcut_status(desired: &[String], registered: &[String]) -> ShortcutStatus {
    ShortcutStatus {
        shortcut: registered
            .first()
            .or(desired.first())
            .cloned()
            .unwrap_or_default(),
        registered: registered.iter().any(|item| desired.contains(item)),
        shortcuts: if registered.is_empty() {
            desired.to_vec()
        } else {
            registered.to_vec()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_ENUM_INVALID, E_INVALID_ARGUMENT};

    #[test]
    fn normalization_matches_shared_rules() {
        assert_eq!(normalize_binding("Alt+Space"), "Alt+Space");
        assert_eq!(normalize_binding("alt+space"), "Alt+Space");
        assert_eq!(normalize_binding("ctrl+shift+m"), "Control+Shift+M");
        assert_eq!(
            normalize_binding("CommandOrControl+Alt+R"),
            "CommandOrControl+Alt+R"
        );
        assert_eq!(normalize_binding("cmdorctrl+,"), "CommandOrControl+,");
        assert_eq!(normalize_binding("Spacebar"), "Space");
        assert_eq!(normalize_binding("Esc"), "Escape");
        assert_eq!(normalize_binding("  "), "");
        assert_eq!(normalize_binding(""), "");
        // NBSP 结尾 + Space 键（现役注释里的坑）
        assert_eq!(normalize_binding("Alt+\u{00A0}"), "Alt+Space");
    }

    #[test]
    fn accelerator_validation_rejects_malformed() {
        for good in [
            "Alt+Space",
            "CommandOrControl+,",
            "Control+Shift+M",
            "Meta+F12",
            "Control+9",
            "Super+Shift+PageUp",
        ] {
            assert!(validate_accelerator(good).is_ok(), "应接受 {good}");
        }
        for bad in [
            "",
            "Alt",
            "Alt+Shift",
            "Alt+Space+Enter",
            "Alt+Alt+Space",
            "Alt+Unknown",
            "Alt+Space\u{0}",
            "Shift+Control+Alt+Meta+Space",
        ] {
            let code = validate_accelerator(bad).map(|_| ()).unwrap_err().code();
            assert!(
                code == E_INVALID_ARGUMENT || code == E_ENUM_INVALID,
                "{bad:?} 应被拒绝，实际 {code}"
            );
        }
    }

    #[test]
    fn shortcut_selection_uses_override_then_legacy_then_default() {
        let override_values = vec![
            "alt+space".to_string(),
            "Control+Shift+M".to_string(),
            "CommandOrControl+Alt+R".to_string(),
            "F13".to_string(),
            "Alt+Space".to_string(),
        ];
        let selected =
            select_quick_chat_shortcuts(Some(&override_values), Some("Alt+Shift+K")).unwrap();
        assert_eq!(selected.len(), 3, "最多 3 组");
        assert_eq!(selected[0], "Alt+Space");
        assert_eq!(selected[1], "Control+Shift+M");
        assert_eq!(selected[2], "CommandOrControl+Alt+R");

        let legacy = select_quick_chat_shortcuts(None, Some("Alt+Shift+K")).unwrap();
        assert_eq!(legacy, vec!["Alt+Shift+K"]);
        let default = select_quick_chat_shortcuts(None, None).unwrap();
        assert_eq!(default, vec![DEFAULT_QUICK_CHAT_SHORTCUT]);
        // 显式空覆盖数组 → 回落 legacy（现役 `if (list.length) return list`）
        let empty = select_quick_chat_shortcuts(Some(&[]), Some("Alt+Shift+K")).unwrap();
        assert_eq!(empty, vec!["Alt+Shift+K"]);
    }

    #[test]
    fn selection_rejects_invalid_bindings() {
        let bad = vec!["Alt+".to_string()];
        assert_eq!(
            select_quick_chat_shortcuts(Some(&bad), None)
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
    }

    #[test]
    fn registration_plan_unregisters_stale_keeps_valid() {
        let plan = plan_registration(
            &["Alt+Space".to_string(), "Control+Shift+M".to_string()],
            &["Alt+Space".to_string(), "F13".to_string()],
        );
        assert_eq!(plan.unregister, vec!["Control+Shift+M"]);
        assert_eq!(plan.register, vec!["F13"]);
        assert_eq!(plan.unchanged, vec!["Alt+Space"]);
    }

    #[test]
    fn registration_result_shapes_are_stable() {
        let ok = summarize_registration(
            &["Alt+Space".to_string()],
            &[("Alt+Space".to_string(), true)],
        );
        assert!(ok.ok);
        assert_eq!(ok.shortcut, "Alt+Space");
        assert!(ok.error.is_none());

        let failed = summarize_registration(
            &["Alt+Space".to_string(), "F13".to_string()],
            &[("Alt+Space".to_string(), true), ("F13".to_string(), false)],
        );
        assert!(!failed.ok);
        assert_eq!(failed.shortcut, "F13");
        assert_eq!(failed.failures, vec!["F13"]);
        assert_eq!(failed.error.as_deref(), Some("shortcut is unavailable"));
    }

    #[test]
    fn test_register_plan_always_cleans_up() {
        let plan = plan_test_register("alt+space").unwrap();
        assert_eq!(plan.shortcut, "Alt+Space");
        assert!(!plan.already_registered);
        assert!(plan.must_unregister_after);
        assert_eq!(
            plan_test_register("").unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            plan_test_register("Alt+").unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
    }

    #[test]
    fn status_reports_registered_only_when_desired() {
        let desired = vec!["Alt+Space".to_string()];
        let registered = vec!["Alt+Space".to_string()];
        assert_eq!(
            shortcut_status(&desired, &registered),
            ShortcutStatus {
                shortcut: "Alt+Space".to_string(),
                registered: true,
                shortcuts: registered.clone(),
            }
        );
        let none: Vec<String> = Vec::new();
        let status = shortcut_status(&desired, &none);
        assert!(!status.registered);
        assert_eq!(status.shortcuts, desired);
        assert_eq!(
            shortcut_status(&desired, &["F13".to_string()]).registered,
            false,
            "注册了其它键不代表目标键可用"
        );
    }
}
