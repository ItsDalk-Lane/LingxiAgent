//! R09-T03 应用信息 / 平台字面值（`host:app-info-platform`）。
//!
//! 现役 Electron：`get-platform` 返回 `process.platform`（`darwin`/`win32`/`linux`），
//! `get-app-version` 返回**壳版本**（`app.getVersion()`），不是内容版本。
//! 渲染层 `platform.js` 在 React 之前用 `getPlatform` 设 `data-platform`。
//!
//! Tauri 注意：`tauri::platform()` 返回 `"macOS"` 风格命名，**不能**直接透传给
//! 渲染层（会破坏 data-platform/CSS 与 WindowControls 判定），必须映射回 Node
//! 字面值。本模块是唯一映射点。

/// Node `process.platform` 兼容字面值。
pub fn platform_literal() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        // 未支持平台不猜成 linux：渲染层会按 unknown 处理，证据显式失败。
        "unknown"
    }
}

/// 把任意平台命名（Tauri `"macOS"`/`"MacOS"`/Node 字面值/大小写变体）映射到
/// Node 字面值；未知返回 None（不静默回落）。
pub fn platform_literal_from(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "darwin" | "macos" | "mac" | "macosx" => Some("darwin"),
        "win32" | "windows" | "win" => Some("win32"),
        "linux" => Some("linux"),
        _ => None,
    }
}

/// 壳版本语义说明（文档断言锚点）：`get_app_version` 只返回壳版本；
/// 内容版本唯一出口是 `train_update_status.currentVersion`。
pub const SHELL_VERSION_COMMAND: &str = "get_app_version";
pub const CONTENT_VERSION_OWNER: &str = "train_update_status.currentVersion";

/// 版本字符串基本校验（非空、≤64、无控制字符）——命令返回前自检，避免把
/// 畸形构建版本写进渲染层。
pub fn validate_version(value: &str) -> Option<&str> {
    if value.is_empty() || value.len() > 64 || value.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(value)
}

/// `data-platform` 引导属性值（与 platform.js 一致；未知平台返回 "web" 语义的
/// 保守值 "unknown"，由证据断言）。
pub fn data_platform_attribute() -> &'static str {
    platform_literal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_literal_matches_node_process_platform() {
        let expected = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "windows") {
            "win32"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else {
            "unknown"
        };
        assert_eq!(platform_literal(), expected);
        assert_eq!(data_platform_attribute(), expected);
    }

    #[test]
    fn tauri_naming_is_mapped_not_passed_through() {
        assert_eq!(platform_literal_from("macOS"), Some("darwin"));
        assert_eq!(platform_literal_from("MacOS"), Some("darwin"));
        assert_eq!(platform_literal_from("darwin"), Some("darwin"));
        assert_eq!(platform_literal_from("windows"), Some("win32"));
        assert_eq!(platform_literal_from("win32"), Some("win32"));
        assert_eq!(platform_literal_from("linux"), Some("linux"));
        assert_eq!(platform_literal_from("freebsd"), None);
        assert_eq!(platform_literal_from(""), None);
        // 关键：Tauri 的 "macOS" 绝不能原样成为 data-platform
        assert_ne!(platform_literal_from("macOS"), Some("macOS"));
    }

    #[test]
    fn version_semantics_are_separate() {
        assert_eq!(SHELL_VERSION_COMMAND, "get_app_version");
        assert_eq!(CONTENT_VERSION_OWNER, "train_update_status.currentVersion");
        assert_eq!(validate_version("1.2.3"), Some("1.2.3"));
        assert_eq!(validate_version(""), None);
        assert_eq!(validate_version("1.2.3\n"), None);
        assert_eq!(validate_version(&"1".repeat(65)), None);
    }
}
