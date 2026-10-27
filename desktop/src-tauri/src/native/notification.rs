//! R09-T03 系统通知（`host:notification`）。
//!
//! 现役 Electron（desktop/main.cjs `show-notification` + shared/desktop-notification-*.cjs）：
//! - 只接受主窗调用（T03 补显式 sender 门）；
//! - `desktopFocusPolicy === "when_unfocused"` 且当前有聚焦窗口时抑制，回执
//!   `{shown:false, reason:"hana_focused"}`；
//! - 系统不支持 → `{shown:false, reason:"unsupported"}`；
//! - 真实展示回执：show/failed/close/10s 超时 → `{shown:true}` /
//!   `{shown:false, reason:"failed"|"unconfirmed"}`（提交请求不等于送达）；
//! - agentId 仅用于解析 `LINGXI_HOME/agents/<id>/avatars/agent.{png,jpg,jpeg,webp}`
//!   作为 icon；不合法/缺失一律无 icon（禁止拿当前焦点 agent 兜底）；
//! - 点击通知 → 恢复（最小化还原）+ show + focus 主窗。
//!
//! Tauri：`tauri-plugin-notification`（仅 Rust 侧）能展示/probe 权限，但**没有
//! 通用桌面 click 回调**（OQ-03）。本模块把「点击聚焦」显式建模为
//! `ClickFocusSupport::Unsupported`（附原因），不假装已实现；平台补丁/降级
//! 留待 A05 证据与后续任务裁决。

use std::path::PathBuf;

use super::errors::{
    deny, HostResult, E_INVALID_ARGUMENT, E_PLATFORM_UNSUPPORTED, E_WINDOW_NOT_ALLOWED,
};
use super::window_labels::{require_role, WindowRole};

pub const DEFAULT_TITLE: &str = "Lingxi";
pub const MAX_TITLE_LEN: usize = 256;
pub const MAX_BODY_LEN: usize = 4096;
pub const SHOW_RECEIPT_TIMEOUT_MS: u64 = 10_000;
pub const AVATAR_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPolicy {
    Always,
    WhenUnfocused,
}

/// `normalizeDesktopNotificationOptions` 的移植：只有字符串 "when_unfocused"
/// 生效，其余（数组/对象/未知值）回落 "always"。
pub fn normalize_focus_policy(raw: Option<&str>) -> FocusPolicy {
    match raw {
        Some("when_unfocused") => FocusPolicy::WhenUnfocused,
        _ => FocusPolicy::Always,
    }
}

/// agentId 合法性（与 `resolveAgentAvatarPath` 同规则）：非空、无路径分隔符、
/// 无 `..`、长度 ≤ 128。不合法不是错误 → 返回 None（无 icon），与现役一致。
pub fn valid_agent_id(raw: &str) -> Option<&str> {
    if raw.is_empty() || raw.len() > 128 {
        return None;
    }
    if raw.contains("..") || raw.contains('/') || raw.contains('\\') {
        return None;
    }
    if raw.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(raw)
}

/// 头像候选路径（按扩展名顺序；是否存在由调用方 `std::fs` 检查）。
/// `home` 必须是宿主解析的绝对 LINGXI_HOME。
pub fn avatar_candidate_paths(home: &str, agent_id: &str) -> HostResult<Vec<PathBuf>> {
    let Some(agent_id) = valid_agent_id(agent_id) else {
        return Ok(Vec::new());
    };
    let base = PathBuf::from(home)
        .join("agents")
        .join(agent_id)
        .join("avatars");
    Ok(AVATAR_EXTS
        .iter()
        .map(|ext| base.join(format!("agent.{ext}")))
        .collect())
}

/// 校验后的通知请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRequest {
    pub title: String,
    pub body: String,
    pub agent_id: Option<String>,
    pub focus_policy: FocusPolicy,
    /// body 被显式截断（不是静默丢弃；回执/诊断带上该标记）。
    pub body_truncated: bool,
}

/// 校验通知参数（主窗 only）。title 空 → 默认 "Lingxi"；body 空 → ""。
pub fn validate_notification_request(
    caller: &str,
    title: &str,
    body: &str,
    agent_id: Option<&str>,
    raw_focus_policy: Option<&str>,
) -> HostResult<NotificationRequest> {
    require_role(caller, &[WindowRole::Main])?;
    if title.len() > MAX_TITLE_LEN * 4 || body.len() > MAX_BODY_LEN * 4 {
        return Err(deny(
            E_INVALID_ARGUMENT,
            "通知 title/body 超过硬上限（疑似注入或误用）",
        ));
    }
    if title.chars().any(|c| c.is_control()) || body.chars().any(|c| c.is_control()) {
        return Err(deny(E_INVALID_ARGUMENT, "通知 title/body 含控制字符"));
    }
    let title = if title.is_empty() {
        DEFAULT_TITLE.to_string()
    } else if title.chars().count() > MAX_TITLE_LEN {
        // 显式截断（系统 toast 标题长度有限），通过 body_truncated 汇报。
        title.chars().take(MAX_TITLE_LEN).collect()
    } else {
        title.to_string()
    };
    let body_truncated = body.chars().count() > MAX_BODY_LEN;
    let body = if body_truncated {
        body.chars().take(MAX_BODY_LEN).collect()
    } else {
        body.to_string()
    };
    Ok(NotificationRequest {
        title,
        body,
        agent_id: agent_id.and_then(valid_agent_id).map(str::to_owned),
        focus_policy: normalize_focus_policy(raw_focus_policy),
        body_truncated,
    })
}

/// 是否抑制展示（Only when_unfocused + 当前有存活聚焦窗口）。
pub fn should_suppress(policy: FocusPolicy, focused_window_alive: bool) -> bool {
    policy == FocusPolicy::WhenUnfocused && focused_window_alive
}

/// 送达回执（现役 `showNotificationWithReceipt` 的状态集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptReason {
    /// 因主窗聚焦被抑制。
    HanaFocused,
    /// OS/插件不支持通知。
    Unsupported,
    /// 系统报告 failed。
    Failed,
    /// 超时/close 先到：未确认展示，不能算送达。
    Unconfirmed,
}

impl ReceiptReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HanaFocused => "hana_focused",
            Self::Unsupported => "unsupported",
            Self::Failed => "failed",
            Self::Unconfirmed => "unconfirmed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationReceipt {
    pub shown: bool,
    pub reason: Option<ReceiptReason>,
}

pub fn suppressed_receipt() -> NotificationReceipt {
    NotificationReceipt {
        shown: false,
        reason: Some(ReceiptReason::HanaFocused),
    }
}

pub fn unsupported_receipt() -> NotificationReceipt {
    NotificationReceipt {
        shown: false,
        reason: Some(ReceiptReason::Unsupported),
    }
}

pub fn shown_receipt() -> NotificationReceipt {
    NotificationReceipt {
        shown: true,
        reason: None,
    }
}

pub fn failed_receipt(reason: ReceiptReason) -> NotificationReceipt {
    debug_assert!(!reason_is_shown(reason));
    NotificationReceipt {
        shown: false,
        reason: Some(reason),
    }
}

fn reason_is_shown(reason: ReceiptReason) -> bool {
    matches!(reason, ReceiptReason::HanaFocused)
}

/// 通知权限状态（用于首启 mac 探测；插件 `permission_state` 的归一）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationPermission {
    NotDetermined,
    Granted,
    Denied,
    Unsupported,
}

/// 点击通知后的聚焦计划。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickFocusPlan {
    pub restore_if_minimized: bool,
    pub show: bool,
    pub focus: bool,
    pub target: WindowRole,
}

pub fn click_focus_plan() -> ClickFocusPlan {
    ClickFocusPlan {
        restore_if_minimized: true,
        show: true,
        focus: true,
        target: WindowRole::Main,
    }
}

/// 点击回调支持度（OQ-03）。
///
/// - macOS：宿主已用 `UNUserNotificationCenter` delegate 实现点击聚焦
///   （`native/notification_center_macos.rs`，objc2-user-notifications 0.3.2），
///   但**真实点击后的聚焦**仍需注册后人工/GUI 证据，故状态为「已实现待验证」；
/// - 其它平台：插件无通用桌面 click 回调（notify-rust 的阻塞式
///   `wait_for_response` 不能嵌入 Tauri 事件循环），显式 Unsupported。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickFocusSupport {
    /// 实现已落地，等待注册后真实点击验证（不得据此声称 A05 通过）。
    ImplementedPendingVerification(&'static str),
    Unsupported(&'static str),
}

pub fn click_focus_support_for(platform: &str) -> ClickFocusSupport {
    if platform == "darwin" {
        return ClickFocusSupport::ImplementedPendingVerification(
            "macOS UNUserNotificationCenter delegate 已实现点击聚焦（native/notification_center_macos.rs）；待注册后真实点击证据",
        );
    }
    ClickFocusSupport::Unsupported(
        "tauri-plugin-notification 桌面端无通用 click 回调（OQ-03）；系统层面点击激活 app 的行为不作为宿主已实现聚焦的证据",
    )
}

/// 插件缺失/平台不支持时的明确错误。
pub fn notification_unsupported_error() -> super::errors::HostError {
    deny(
        E_PLATFORM_UNSUPPORTED,
        "当前平台/构建不支持系统通知（显式 unsupported，不是静默 no-op）",
    )
}

/// 非主窗调用通知显示命令的拒绝（预留给 host-core 的错误面）。
pub fn notification_caller_denied() -> super::errors::HostError {
    deny(E_WINDOW_NOT_ALLOWED, "show_notification 只允许主窗口调用")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_policy_normalization() {
        assert_eq!(normalize_focus_policy(None), FocusPolicy::Always);
        assert_eq!(
            normalize_focus_policy(Some("when_unfocused")),
            FocusPolicy::WhenUnfocused
        );
        assert_eq!(
            normalize_focus_policy(Some("WHEN_UNFOCUSED")),
            FocusPolicy::Always,
            "大小写敏感（现役严格等值）"
        );
        assert_eq!(normalize_focus_policy(Some("always")), FocusPolicy::Always);
        // 抑制只在 when_unfocused + 有聚焦窗口
        assert!(!should_suppress(FocusPolicy::Always, true));
        assert!(should_suppress(FocusPolicy::WhenUnfocused, true));
        assert!(!should_suppress(FocusPolicy::WhenUnfocused, false));
    }

    #[test]
    fn request_validation_defaults_and_limits() {
        let request = validate_notification_request("main", "", "", None, None).unwrap();
        assert_eq!(request.title, "Lingxi");
        assert_eq!(request.body, "");
        assert!(!request.body_truncated);

        let long_title = "标".repeat(MAX_TITLE_LEN + 5);
        let request =
            validate_notification_request("main", &long_title, "body", None, None).unwrap();
        assert_eq!(request.title.chars().count(), MAX_TITLE_LEN);
        let long_body = "x".repeat(MAX_BODY_LEN + 5);
        let request = validate_notification_request("main", "t", &long_body, None, None).unwrap();
        assert!(request.body_truncated);
        assert_eq!(request.body.chars().count(), MAX_BODY_LEN);
        // 硬上限之外直接拒绝
        assert_eq!(
            validate_notification_request(
                "main",
                "t",
                &"x".repeat(MAX_BODY_LEN * 4 + 1),
                None,
                None
            )
            .unwrap_err()
            .code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            validate_notification_request("main", "bad\u{0}", "", None, None)
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
    }

    #[test]
    fn only_main_window_may_show_notifications() {
        assert!(validate_notification_request("main", "t", "b", None, None).is_ok());
        for caller in ["settings", "quick-chat", "viewer-1", "browser-viewer"] {
            assert_eq!(
                validate_notification_request(caller, "t", "b", None, None)
                    .unwrap_err()
                    .code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(notification_caller_denied().code(), E_WINDOW_NOT_ALLOWED);
    }

    #[test]
    fn agent_id_and_avatar_paths_are_sanitized() {
        assert_eq!(valid_agent_id("hanako"), Some("hanako"));
        for bad in ["", "../evil", "a/b", "a\\b", "name\u{0}", &"a".repeat(129)] {
            assert_eq!(valid_agent_id(bad), None, "应拒绝 agentId {bad:?}");
        }
        let request =
            validate_notification_request("main", "t", "b", Some("../evil"), None).unwrap();
        assert_eq!(request.agent_id, None, "非法 agentId 不使用 icon");
        let paths = avatar_candidate_paths("/private/tmp/lingxi-home", "hanako").unwrap();
        assert_eq!(paths.len(), 4);
        assert_eq!(
            paths[0],
            PathBuf::from("/private/tmp/lingxi-home/agents/hanako/avatars/agent.png")
        );
        assert!(
            avatar_candidate_paths("/private/tmp/lingxi-home", "../evil")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn receipts_distinguish_shown_from_unconfirmed() {
        assert_eq!(
            shown_receipt(),
            NotificationReceipt {
                shown: true,
                reason: None
            }
        );
        assert_eq!(
            suppressed_receipt().reason,
            Some(ReceiptReason::HanaFocused)
        );
        assert_eq!(
            unsupported_receipt().reason,
            Some(ReceiptReason::Unsupported)
        );
        // 提交/超时/失败都不是送达
        for reason in [ReceiptReason::Failed, ReceiptReason::Unconfirmed] {
            let receipt = failed_receipt(reason);
            assert!(!receipt.shown);
            assert_eq!(receipt.reason, Some(reason));
        }
        assert_eq!(SHOW_RECEIPT_TIMEOUT_MS, 10_000);
        assert_eq!(ReceiptReason::HanaFocused.as_str(), "hana_focused");
        assert_eq!(ReceiptReason::Unconfirmed.as_str(), "unconfirmed");
    }

    #[test]
    fn click_focus_is_explicitly_unsupported() {
        let plan = click_focus_plan();
        assert!(plan.restore_if_minimized && plan.show && plan.focus);
        assert_eq!(plan.target, WindowRole::Main);
        match click_focus_support_for("darwin") {
            ClickFocusSupport::ImplementedPendingVerification(reason) => {
                assert!(reason.contains("待注册后真实点击证据"));
            }
            other => panic!("mac 应为已实现待验证: {other:?}"),
        }
        for platform in ["win32", "linux"] {
            match click_focus_support_for(platform) {
                ClickFocusSupport::Unsupported(reason) => {
                    assert!(reason.contains("click"), "原因必须说明缺 click 回调");
                }
                other => panic!("{platform} 应为 Unsupported: {other:?}"),
            }
        }
        assert_eq!(
            notification_unsupported_error().code(),
            E_PLATFORM_UNSUPPORTED
        );
    }
}
