//! R09-T02 启动失败的机器可读错误。
//!
//! 设计原则（AGENTS.md「禁止静默降级」）：每一次启动拒绝都带稳定 `code`、
//! 人类可读 `message` 与可对照的 `expected`/`actual`，供诊断 JSONL、失败记录
//! 与 A03 证据逐项断言。错误码不使用 `Display` 文本猜测，调用方只认 `code`。

use serde_json::{json, Value};

pub type ServiceResult<T> = Result<T, StartError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError {
    pub code: &'static str,
    pub message: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub detail: Value,
}

impl StartError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            expected: None,
            actual: None,
            detail: Value::Null,
        }
    }

    /// 期望/实际成对的拒绝（版本、摘要、身份、地址等门禁）。
    pub fn mismatch(
        code: &'static str,
        message: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            expected: Some(expected.into()),
            actual: Some(actual.into()),
            detail: Value::Null,
        }
    }

    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = detail;
        self
    }

    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    pub fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
        self
    }

    /// 失败记录/诊断事件共用的结构化形状（不含任何 token）。
    pub fn to_json(&self) -> Value {
        json!({
            "reason": self.code,
            "message": self.message,
            "expected": self.expected,
            "actual": self.actual,
            "detail": self.detail,
        })
    }
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for StartError {}

/// 启动/退出相关错误码（证据与诊断的稳定词汇）。
pub mod codes {
    pub const SERVICE_PROTOCOL_INCOMPATIBLE: &str = "RUST_SERVICE_PROTOCOL_INCOMPATIBLE";
    pub const SERVICE_DIGEST_MISMATCH: &str = "RUST_SERVICE_DIGEST_MISMATCH";
    pub const SERVICE_MANIFEST_INVALID: &str = "RUST_SERVICE_MANIFEST_INVALID";
    pub const SERVICE_BINARY_INVALID: &str = "RUST_SERVICE_BINARY_INVALID";
    pub const SERVICE_BINARY_UNAVAILABLE: &str = "RUST_SERVICE_BINARY_UNAVAILABLE";
    pub const SERVICE_SPAWN_FAILED: &str = "RUST_SERVICE_SPAWN_FAILED";
    pub const SERVICE_READY_TIMEOUT: &str = "RUST_SERVICE_READY_TIMEOUT";
    pub const SERVICE_READY_OUTPUT_EXCEEDED: &str = "RUST_SERVICE_READY_OUTPUT_EXCEEDED";
    pub const SERVICE_READY_LINE_INVALID: &str = "RUST_SERVICE_READY_LINE_INVALID";
    pub const SERVICE_EXITED_BEFORE_READY: &str = "RUST_SERVICE_EXITED_BEFORE_READY";
    pub const SERVICE_SINGLE_WRITER_BLOCKED: &str = "RUST_SERVICE_SINGLE_WRITER_BLOCKED";
    pub const SERVICE_INSTANCE_INVALID: &str = "RUST_SERVICE_INSTANCE_INVALID";
    pub const SERVICE_IDENTITY_FAILED: &str = "RUST_SERVICE_IDENTITY_FAILED";
    pub const RUNTIME_IDENTITY_MISMATCH: &str = "RUNTIME_IDENTITY_MISMATCH";
    pub const EXISTING_INSTANCE_UNVERIFIED: &str = "RUST_DESKTOP_EXISTING_INSTANCE_UNVERIFIED";
    pub const NODE_SERVER_INFO_PRESENT: &str = "RUST_DESKTOP_NODE_SERVER_INFO_PRESENT";
    pub const DATA_DOWNGRADE_REFUSED: &str = "RUST_DESKTOP_DATA_DOWNGRADE_REFUSED";
    pub const REMOTE_REQUIRES_DEV_BUILD: &str = "REMOTE_REQUIRES_DEV_BUILD";
    pub const REMOTE_ENV_INVALID: &str = "REMOTE_ENV_INVALID";
    pub const HOME_INVALID: &str = "RUST_DESKTOP_HOME_INVALID";
    pub const CRASH_RESTART_EXHAUSTED: &str = "RUST_SERVICE_CRASH_RESTART_EXHAUSTED";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatch_records_expected_and_actual() {
        let error = StartError::mismatch(
            codes::SERVICE_PROTOCOL_INCOMPATIBLE,
            "protocol gate",
            "1..=1 epoch 2",
            "1..=2 epoch 3",
        );
        let json = error.to_json();
        assert_eq!(json["reason"], codes::SERVICE_PROTOCOL_INCOMPATIBLE);
        assert_eq!(json["expected"], "1..=1 epoch 2");
        assert_eq!(json["actual"], "1..=2 epoch 3");
        assert_eq!(
            error.to_string(),
            "RUST_SERVICE_PROTOCOL_INCOMPATIBLE: protocol gate"
        );
    }
}
