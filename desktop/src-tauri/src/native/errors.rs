//! R09-T03/T04 宿主能力的稳定错误码。
//!
//! 红线「禁止静默降级」：宿主能力受限/参数非法一律返回带稳定机器码的
//! `HostError`（渲染层与 IPC 负向证据都按 code 断言），不返回 `None` 让调用
//! 方猜测，也不把畸形参数转成系统动作。渲染侧 `HostCapabilityError` 使用
//! 同一约定：`code` 是设备/权限/参数边界的唯一判定面。

use std::fmt;

/// 宿主拒绝的稳定错误：`code` 用于机器断言，`message` 用于用户可读提示。
///
/// 注意：`message` 可能被写入诊断日志，因此不得包含 token、原始文件内容或
/// 渲染层传入的任意长文本（调用方负责只放稳定描述）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    code: &'static str,
    message: String,
}

impl HostError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HostError {}

pub type HostResult<T> = Result<T, HostError>;

/// 便捷构造：`deny("code", "message")`。
pub fn deny(code: &'static str, message: impl Into<String>) -> HostError {
    HostError::new(code, message)
}

// ── 稳定 code 常量（跨 Rust/TS/生产者/证据共享；不要改名，只追加）──

/// 窗口中不存在的 label、或该能力不允许该窗口调用。
pub const E_WINDOW_NOT_ALLOWED: &str = "WINDOW_NOT_ALLOWED";
/// 窗口 label 语法非法（控制字符/超长/非白名单形状）。
pub const E_WINDOW_LABEL_INVALID: &str = "WINDOW_LABEL_INVALID";
/// 字符串参数为空或超长或含控制字符。
pub const E_INVALID_ARGUMENT: &str = "INVALID_ARGUMENT";
/// 要求绝对路径但收到相对路径/空串/NUL。
pub const E_PATH_NOT_ABSOLUTE: &str = "PATH_NOT_ABSOLUTE";
/// 路径必须是普通文件或目录但类型不符。
pub const E_PATH_TYPE_MISMATCH: &str = "PATH_TYPE_MISMATCH";
/// 路径不存在（命令层在真实文件系统检查后返回）。
pub const E_PATH_MISSING: &str = "PATH_MISSING";
/// URL 不是 http/https 或结构非法。
pub const E_URL_INVALID: &str = "URL_INVALID";
/// URL scheme 被拒绝（javascript/file/data/…）。
pub const E_URL_SCHEME_DENIED: &str = "URL_SCHEME_DENIED";
/// 枚举/白名单外取值。
pub const E_ENUM_INVALID: &str = "ENUM_INVALID";
/// 数值参数越界或非整数。
pub const E_NUMBER_INVALID: &str = "NUMBER_INVALID";
/// 该能力在当前平台没有等价实现（显式 unsupported，不是静默 no-op）。
pub const E_PLATFORM_UNSUPPORTED: &str = "PLATFORM_UNSUPPORTED";
/// 需要宿主注册/系统实现但当前构建未启用（本轮 BLOCKED 的机器面）。
pub const E_NOT_IMPLEMENTED: &str = "NOT_IMPLEMENTED";
/// 远端会话/未获本机明确授权的调用方读取本机设备。
pub const E_REMOTE_DEVICE_DENIED: &str = "REMOTE_DEVICE_DENIED";
/// 权限被拒绝/受限/未决定（附权限名）。
pub const E_PERMISSION_DENIED: &str = "PERMISSION_DENIED";
/// 权限请求状态机不允许当前操作（重复请求、录制中撤销等）。
pub const E_PERMISSION_STATE: &str = "PERMISSION_STATE";
/// 令牌/句柄过期或不属于调用窗口。
pub const E_TOKEN_INVALID: &str = "TOKEN_INVALID";
/// 请求发生冲突（快捷键占用、版本 CAS 冲突等）。
pub const E_CONFLICT: &str = "CONFLICT";
/// 能力在 release 构建中被剔除（debug 命令）。
pub const E_RELEASE_DISABLED: &str = "RELEASE_DISABLED";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_carries_stable_code_and_display() {
        let error = deny(E_URL_SCHEME_DENIED, "只支持 HTTP/HTTPS");
        assert_eq!(error.code(), E_URL_SCHEME_DENIED);
        assert_eq!(error.message(), "只支持 HTTP/HTTPS");
        assert_eq!(format!("{error}"), "URL_SCHEME_DENIED: 只支持 HTTP/HTTPS");
    }

    #[test]
    fn errors_compare_by_code_and_message() {
        assert_eq!(
            deny(E_CONFLICT, "occupied"),
            HostError::new(E_CONFLICT, "occupied")
        );
        assert_ne!(deny(E_CONFLICT, "occupied"), deny(E_CONFLICT, "other"));
    }

    #[test]
    fn codes_are_distinct_and_uppercase() {
        let codes = [
            E_WINDOW_NOT_ALLOWED,
            E_WINDOW_LABEL_INVALID,
            E_INVALID_ARGUMENT,
            E_PATH_NOT_ABSOLUTE,
            E_PATH_TYPE_MISMATCH,
            E_PATH_MISSING,
            E_URL_INVALID,
            E_URL_SCHEME_DENIED,
            E_ENUM_INVALID,
            E_NUMBER_INVALID,
            E_PLATFORM_UNSUPPORTED,
            E_NOT_IMPLEMENTED,
            E_REMOTE_DEVICE_DENIED,
            E_PERMISSION_DENIED,
            E_PERMISSION_STATE,
            E_TOKEN_INVALID,
            E_CONFLICT,
            E_RELEASE_DISABLED,
        ];
        for (index, code) in codes.iter().enumerate() {
            assert!(
                code.chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
                "code 必须是大写机器码: {code}"
            );
            for other in codes.iter().skip(index + 1) {
                assert_ne!(code, other, "code 重复");
            }
        }
    }
}
