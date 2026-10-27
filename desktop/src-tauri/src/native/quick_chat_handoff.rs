//! R09-T03 Quick Chat → 主窗会话交接（`host:quick-chat-session-handoff`）。
//!
//! 现役 Electron（desktop/quick-chat-session-handoff.cjs 83 行）协议：
//! 1. 只有 quick-chat 窗口（webContents 主帧）可信；主窗必须存活；
//! 2. 目标校验：`sessionPath` 非空 ≤8192，`ownerKey` 必须是 64 位小写十六进制，
//!    `sessionId` 可空但非空时必须非空白；`native:<id>` 前缀必须与 sessionId 一致；
//! 3. 新请求顶掉 pending（旧请求回 `QUICK_CHAT_SUPERSEDED`），15s 超时；
//! 4. 主窗 ack：requestId 必须匹配且发送者可信；ok=false → 失败码原样回；
//!    ok=true 时 sessionPath/ownerKey/sessionId 必须与请求一致（防串会话）；
//! 5. 收到合法 ack 后：恢复/显示/聚焦主窗，轮询等待焦点 ≤2s，确认主窗已聚焦
//!    才发出「已切到主窗」结果并收起 quick-chat；任一步失败保留 quick-chat 并回
//!    `QUICK_CHAT_FOCUS_FAILED`。
//!
//! 本模块把状态机与结果码做成纯逻辑（不接触窗口），glue 负责事件/焦点轮询。

use super::errors::{deny, HostResult, E_INVALID_ARGUMENT};

pub const HANDOFF_TIMEOUT_MS: u64 = 15_000;
pub const FOCUS_TIMEOUT_MS: u64 = 2_000;
pub const MAX_SESSION_PATH_LEN: usize = 8192;
pub const MAX_SESSION_ID_LEN: usize = 512;
pub const EVENT_OPEN_SESSION: &str = "quick-chat-open-session";

/// 现役失败码（renderer 直接消费，禁止改名）。
pub const CODE_SENDER_REJECTED: &str = "QUICK_CHAT_SENDER_REJECTED";
pub const CODE_MAIN_UNAVAILABLE: &str = "QUICK_CHAT_MAIN_UNAVAILABLE";
pub const CODE_INVALID_TARGET: &str = "QUICK_CHAT_INVALID_TARGET";
pub const CODE_SUPERSEDED: &str = "QUICK_CHAT_SUPERSEDED";
pub const CODE_OPEN_TIMEOUT: &str = "QUICK_CHAT_OPEN_TIMEOUT";
pub const CODE_MAIN_DESTROYED: &str = "QUICK_CHAT_MAIN_DESTROYED";
pub const CODE_MAIN_NAVIGATED: &str = "QUICK_CHAT_MAIN_NAVIGATED";
pub const CODE_SOURCE_DESTROYED: &str = "QUICK_CHAT_SOURCE_DESTROYED";
pub const CODE_DELIVERY_FAILED: &str = "QUICK_CHAT_DELIVERY_FAILED";
pub const CODE_SWITCH_FAILED: &str = "QUICK_CHAT_SWITCH_FAILED";
pub const CODE_TARGET_MISMATCH: &str = "QUICK_CHAT_TARGET_MISMATCH";
pub const CODE_FOCUS_FAILED: &str = "QUICK_CHAT_FOCUS_FAILED";
pub const CODE_CLOSED: &str = "QUICK_CHAT_CLOSED";

/// 渲染层提交的目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffTarget {
    pub session_path: String,
    pub session_id: Option<String>,
    pub owner_key: String,
}

/// 校验后的交接请求（requestId/expiresAt 由宿主生成）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffRequest {
    pub request_id: String,
    pub session_path: String,
    pub session_id: Option<String>,
    pub owner_key: String,
    pub expires_at_ms: u64,
}

/// 失败结果（与现役 `result(false, ...)` 同形）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffFailure {
    pub code: &'static str,
    pub message: &'static str,
}

impl HandoffFailure {
    pub fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

pub fn valid_owner_key(value: &str) -> bool {
    value.len() == 64
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// 目标校验（现役逐条规则的纯逻辑版本）。
pub fn validate_target(target: &HandoffTarget) -> Result<(), HandoffFailure> {
    if target.session_path.trim().is_empty()
        || target.session_path.len() > MAX_SESSION_PATH_LEN
        || target.session_path.chars().any(|c| c.is_control())
    {
        return Err(HandoffFailure::new(
            CODE_INVALID_TARGET,
            "目标会话资料不完整",
        ));
    }
    if !valid_owner_key(&target.owner_key) {
        return Err(HandoffFailure::new(
            CODE_INVALID_TARGET,
            "目标会话资料不完整",
        ));
    }
    if let Some(session_id) = &target.session_id {
        if session_id.trim().is_empty() || session_id.len() > MAX_SESSION_ID_LEN {
            return Err(HandoffFailure::new(
                CODE_INVALID_TARGET,
                "目标会话资料不完整",
            ));
        }
    }
    if let Some(rest) = target.session_path.strip_prefix("native:") {
        let session_id = target
            .session_id
            .as_deref()
            .filter(|value| !value.is_empty());
        if session_id.is_none() || session_id != Some(rest) {
            return Err(HandoffFailure::new(
                CODE_INVALID_TARGET,
                "目标会话标识不一致",
            ));
        }
    }
    Ok(())
}

/// 发起交接（现役 `open()` 的同步部分）：返回请求或失败。
/// 调用方负责：pending 内旧请求先以 SUPERSEDED 失败、随后等待 ack/超时。
pub fn plan_open(
    sender_trusted: bool,
    main_alive: bool,
    target: &HandoffTarget,
    request_id: String,
    now_ms: u64,
) -> Result<HandoffRequest, HandoffFailure> {
    if !sender_trusted {
        return Err(HandoffFailure::new(
            CODE_SENDER_REJECTED,
            "快捷聊天窗口已失效",
        ));
    }
    if !main_alive {
        return Err(HandoffFailure::new(
            CODE_MAIN_UNAVAILABLE,
            "主窗口已关闭，请重新打开后再试",
        ));
    }
    validate_target(target)?;
    Ok(HandoffRequest {
        request_id,
        session_path: target.session_path.trim().to_string(),
        session_id: target.session_id.clone().filter(|value| !value.is_empty()),
        owner_key: target.owner_key.clone(),
        expires_at_ms: now_ms.saturating_add(HANDOFF_TIMEOUT_MS),
    })
}

/// 主窗 ack 的渲染层回包。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffAck {
    pub request_id: Option<String>,
    pub ok: bool,
    pub session_path: Option<String>,
    pub session_id: Option<String>,
    pub owner_key: Option<String>,
    pub code: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AckDecision {
    /// ack 不匹配/不可信：忽略（不改状态）。
    Ignored,
    /// 主窗报告失败：settle 失败结果。
    Rejected(HandoffFailure),
    /// 校验通过：显示主窗 → 等焦点 → 收起 quick-chat → settle 成功。
    Complete {
        session_id: String,
        focus_timeout_ms: u64,
    },
}

/// 处理 ack（现役 `acknowledge()` 的纯逻辑部分）。
pub fn decide_ack(
    request: &HandoffRequest,
    ack_sender_trusted: bool,
    main_alive: bool,
    ack: &HandoffAck,
) -> AckDecision {
    if !ack_sender_trusted || !main_alive {
        return AckDecision::Ignored;
    }
    if ack.request_id.as_deref() != Some(request.request_id.as_str()) {
        return AckDecision::Ignored;
    }
    if !ack.ok {
        let code = ack
            .code
            .clone()
            .unwrap_or_else(|| CODE_SWITCH_FAILED.to_string());
        let message = ack
            .message
            .clone()
            .unwrap_or_else(|| "目标会话未能打开".to_string());
        return AckDecision::Rejected(HandoffFailure::new(
            dynamically_safe_code(&code),
            dynamically_safe_message(&message),
        ));
    }
    let session_id = ack.session_id.clone().unwrap_or_default();
    if ack.session_path.as_deref() != Some(request.session_path.as_str())
        || ack.owner_key.as_deref() != Some(request.owner_key.as_str())
        || session_id.is_empty()
        || request
            .session_id
            .as_deref()
            .is_some_and(|expected| expected != session_id)
    {
        return AckDecision::Rejected(HandoffFailure::new(
            CODE_TARGET_MISMATCH,
            "主窗口返回的会话或来源不一致",
        ));
    }
    AckDecision::Complete {
        session_id,
        focus_timeout_ms: FOCUS_TIMEOUT_MS,
    }
}

/// ack 可能携带渲染层自报 code/message：只接受稳定大写码 + 有限长度文本，
/// 其它回落固定失败码（避免把任意长文本/控制字符写进日志或结果）。
fn dynamically_safe_code(code: &str) -> &'static str {
    match code {
        CODE_SENDER_REJECTED
        | CODE_MAIN_UNAVAILABLE
        | CODE_INVALID_TARGET
        | CODE_SUPERSEDED
        | CODE_OPEN_TIMEOUT
        | CODE_MAIN_DESTROYED
        | CODE_MAIN_NAVIGATED
        | CODE_SOURCE_DESTROYED
        | CODE_DELIVERY_FAILED
        | CODE_SWITCH_FAILED
        | CODE_TARGET_MISMATCH
        | CODE_FOCUS_FAILED
        | CODE_CLOSED => match code {
            CODE_SENDER_REJECTED => CODE_SENDER_REJECTED,
            CODE_MAIN_UNAVAILABLE => CODE_MAIN_UNAVAILABLE,
            CODE_INVALID_TARGET => CODE_INVALID_TARGET,
            CODE_SUPERSEDED => CODE_SUPERSEDED,
            CODE_OPEN_TIMEOUT => CODE_OPEN_TIMEOUT,
            CODE_MAIN_DESTROYED => CODE_MAIN_DESTROYED,
            CODE_MAIN_NAVIGATED => CODE_MAIN_NAVIGATED,
            CODE_SOURCE_DESTROYED => CODE_SOURCE_DESTROYED,
            CODE_DELIVERY_FAILED => CODE_DELIVERY_FAILED,
            CODE_TARGET_MISMATCH => CODE_TARGET_MISMATCH,
            CODE_FOCUS_FAILED => CODE_FOCUS_FAILED,
            CODE_CLOSED => CODE_CLOSED,
            _ => CODE_SWITCH_FAILED,
        },
        _ => CODE_SWITCH_FAILED,
    }
}

fn dynamically_safe_message(_message: &str) -> &'static str {
    "目标会话未能打开"
}

/// 焦点确认结果：只有主窗真正 is_focused 才能收起 quick-chat。
pub fn finish_focus(focus_confirmed: bool, quick_chat_alive: bool) -> Result<(), HandoffFailure> {
    if !focus_confirmed {
        return Err(HandoffFailure::new(
            CODE_FOCUS_FAILED,
            "主窗口无法获得焦点，快捷聊天窗口已保留",
        ));
    }
    if !quick_chat_alive {
        return Err(HandoffFailure::new(
            CODE_SOURCE_DESTROYED,
            "窗口状态已改变，未完成会话交接",
        ));
    }
    Ok(())
}

/// 请求参数中不允许出现控制字符（补齐 validate_target 的输入门）。
pub fn reject_control_characters(value: &str) -> HostResult<()> {
    if value.chars().any(|c| c.is_control()) {
        return Err(deny(E_INVALID_ARGUMENT, "交接参数含控制字符"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::E_INVALID_ARGUMENT;

    fn target() -> HandoffTarget {
        HandoffTarget {
            session_path: "native:abc123".to_string(),
            session_id: Some("abc123".to_string()),
            owner_key: "a".repeat(64),
        }
    }

    fn request() -> HandoffRequest {
        plan_open(true, true, &target(), "req-1".to_string(), 1_000).unwrap()
    }

    #[test]
    fn open_requires_trusted_quick_chat_and_alive_main() {
        let failure = plan_open(false, true, &target(), "r".into(), 0).unwrap_err();
        assert_eq!(failure.code, CODE_SENDER_REJECTED);
        let failure = plan_open(true, false, &target(), "r".into(), 0).unwrap_err();
        assert_eq!(failure.code, CODE_MAIN_UNAVAILABLE);
        let request = plan_open(true, true, &target(), "r".into(), 1_000).unwrap();
        assert_eq!(request.expires_at_ms, 1_000 + HANDOFF_TIMEOUT_MS);
        assert_eq!(request.session_path, "native:abc123");
    }

    #[test]
    fn target_validation_matches_incumbent_rules() {
        assert!(validate_target(&target()).is_ok());
        // native: 前缀必须与 sessionId 一致
        for bad in [
            HandoffTarget {
                session_path: "native:abc".to_string(),
                session_id: Some("other".to_string()),
                owner_key: "a".repeat(64),
            },
            HandoffTarget {
                session_path: "native:abc".to_string(),
                session_id: None,
                owner_key: "a".repeat(64),
            },
            HandoffTarget {
                session_path: "".to_string(),
                session_id: Some("x".to_string()),
                owner_key: "a".repeat(64),
            },
            HandoffTarget {
                session_path: "native:abc".to_string(),
                session_id: Some("abc".to_string()),
                owner_key: "A".repeat(64),
            },
            HandoffTarget {
                session_path: "native:abc".to_string(),
                session_id: Some("abc".to_string()),
                owner_key: "a".repeat(63),
            },
            HandoffTarget {
                session_path: "x".repeat(MAX_SESSION_PATH_LEN + 1),
                session_id: None,
                owner_key: "a".repeat(64),
            },
        ] {
            assert_eq!(
                validate_target(&bad).unwrap_err().code,
                CODE_INVALID_TARGET,
                "应拒绝 {bad:?}"
            );
        }
        // 非 native 会话允许任意 path + 可选 sessionId（现役规则）
        assert!(validate_target(&HandoffTarget {
            session_path: "remote:session-1".to_string(),
            session_id: None,
            owner_key: "a".repeat(64),
        })
        .is_ok());
    }

    #[test]
    fn ack_must_match_request_and_session() {
        let request = request();
        // 正常完成
        let ack = HandoffAck {
            request_id: Some("req-1".to_string()),
            ok: true,
            session_path: Some("native:abc123".to_string()),
            session_id: Some("abc123".to_string()),
            owner_key: Some("a".repeat(64)),
            code: None,
            message: None,
        };
        assert_eq!(
            decide_ack(&request, true, true, &ack),
            AckDecision::Complete {
                session_id: "abc123".to_string(),
                focus_timeout_ms: 2_000
            }
        );
        // 不可信发送者/主窗死亡/requestId 不符 → 忽略
        assert_eq!(
            decide_ack(&request, false, true, &ack),
            AckDecision::Ignored
        );
        assert_eq!(
            decide_ack(&request, true, false, &ack),
            AckDecision::Ignored
        );
        let mut wrong_id = ack.clone();
        wrong_id.request_id = Some("other".into());
        assert_eq!(
            decide_ack(&request, true, true, &wrong_id),
            AckDecision::Ignored
        );
        // 目标不一致
        let mut mismatch = ack.clone();
        mismatch.session_path = Some("native:other".into());
        assert_eq!(
            decide_ack(&request, true, true, &mismatch),
            AckDecision::Rejected(HandoffFailure::new(
                CODE_TARGET_MISMATCH,
                "主窗口返回的会话或来源不一致"
            ))
        );
        let mut owner_mismatch = ack.clone();
        owner_mismatch.owner_key = Some("b".repeat(64));
        assert!(matches!(
            decide_ack(&request, true, true, &owner_mismatch),
            AckDecision::Rejected(failure) if failure.code == CODE_TARGET_MISMATCH
        ));
    }

    #[test]
    fn ack_failure_codes_are_whitelisted() {
        let request = request();
        let failure = HandoffAck {
            request_id: Some("req-1".to_string()),
            ok: false,
            session_path: None,
            session_id: None,
            owner_key: None,
            code: Some("QUICK_CHAT_SWITCH_FAILED".to_string()),
            message: Some("boom".to_string()),
        };
        match decide_ack(&request, true, true, &failure) {
            AckDecision::Rejected(failure) => {
                assert_eq!(failure.code, CODE_SWITCH_FAILED);
                // 渲染层自报文本不进入结果（防日志注入）
                assert_eq!(failure.message, "目标会话未能打开");
            }
            other => panic!("应为失败: {other:?}"),
        }
        let injected = HandoffAck {
            request_id: Some("req-1".to_string()),
            ok: false,
            code: Some("EVIL_CODE\nwith newline".to_string()),
            ..failure
        };
        match decide_ack(&request, true, true, &injected) {
            AckDecision::Rejected(failure) => assert_eq!(failure.code, CODE_SWITCH_FAILED),
            other => panic!("应为失败: {other:?}"),
        }
    }

    #[test]
    fn focus_must_be_confirmed_before_hiding() {
        assert!(finish_focus(true, true).is_ok());
        assert_eq!(
            finish_focus(false, true).unwrap_err().code,
            CODE_FOCUS_FAILED
        );
        assert_eq!(
            finish_focus(true, false).unwrap_err().code,
            CODE_SOURCE_DESTROYED
        );
        assert_eq!(
            reject_control_characters("bad\u{0}").unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(FOCUS_TIMEOUT_MS, 2_000);
        assert_eq!(HANDOFF_TIMEOUT_MS, 15_000);
        assert_eq!(EVENT_OPEN_SESSION, "quick-chat-open-session");
    }
}
