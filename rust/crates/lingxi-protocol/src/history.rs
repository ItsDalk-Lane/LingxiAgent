//! R06-T04：统一消息历史投影的线型（CanonicalMessageStore 的读出产物）。
//!
//! 与既有 `wire::HistoryEntry` 的分工：`HistoryEntry` 是 R02 **事件日志**
//! 分页条目（/events 面，seq/eventId/eventType/summary）；本模块是
//! **消息历史**投影（/history、/export 面）——把 messages + key_events
//! 两类持久事实投影为「用户消息 / 助手段 / 工具卡 / 最终回复 / 终态 /
//! 重置标记 / 原始档案」的统一项序列。实时、重开、重连、导出四方式共用
//! 同一投影函数与同一 vocabulary（任务书 R06-T04 步骤②）。
//!
//! 线型纪律：
//! - seq 一律 u64 十进制字符串（§3 精度规则，同 [`crate::Seq`]）。
//! - 游标是不透明 base64url 令牌（canonical JSON → base64url NO_PAD，
//!   镜像 `service::events::SubscribeCursor` 机制）；服务端严格解码 +
//!   存在性校验，任何偏差响亮 400，绝不静默当作首页。
//! - run 关联三态诚实：只有确定性来源（run_id 列 / 本会话 id 约定且
//!   run 实存）给 `known`；fork 副本等无法确定的一律 `unknown`，绝不
//!   按时间猜测（任务书步骤④）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::wire::{AssistantPhase, ContentBlock, ContentDigest, SegmentKind, ToolResultWire};
use crate::{u64_wire_string, Cursor, RunStatus};

/// 投影/导出/schema 版本锚点。
pub const CANONICAL_HISTORY_SCHEMA: &str = "lingxi.canonical-history.v1";

/// 一条历史项的 run 关联（步骤④：无法确定即 unknown）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunAssociation {
    pub state: RunAssociationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// unknown 时的原因（如 fork 副本 run_id 置 NULL）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunAssociationState {
    Known,
    Unknown,
}

impl RunAssociation {
    pub fn known(run_id: impl Into<String>) -> Self {
        Self {
            state: RunAssociationState::Known,
            run_id: Some(run_id.into()),
            reason: None,
        }
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Self {
            state: RunAssociationState::Unknown,
            run_id: None,
            reason: Some(reason.into()),
        }
    }
}

/// 工具卡状态：`started`（只有开始事件，绝不编造结局）或真实结果状态；
/// `unknown` 镜像 ToolResultStatus::Unknown（外部完成的副作用无本地回执，
/// 绝不报成成功）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HistoryToolStatus {
    Started,
    Success,
    Failed,
    Cancelled,
    Unknown,
}

/// 统一历史项。`kind` 为判别字段（snake_case）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HistoryItem {
    /// 用户消息（回合输入锚点）。
    UserMessage {
        message_id: String,
        #[serde(with = "u64_wire_string")]
        #[schemars(with = "String")]
        seq: u64,
        committed_at_unix_ms: i64,
        text: String,
        run: RunAssociation,
        /// 步骤④：旧记录顶层未知键原样保留（受控 legacy 扩展）。
        #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
        legacy_fields: serde_json::Map<String, serde_json::Value>,
    },
    /// 助手段（正文/推理；来自 assistant_segment_* 事件聚合）。
    AssistantSegment {
        segment_id: String,
        run: RunAssociation,
        phase: AssistantPhase,
        segment_kind: SegmentKind,
        text: String,
        /// start..end 周期完整闭合。
        complete: bool,
        #[serde(with = "u64_wire_string")]
        #[schemars(with = "String")]
        first_seq: u64,
    },
    /// 工具卡（started/completed 事件配对；稳定 toolCallId 全程不变）。
    ToolCall {
        tool_call_id: String,
        run: RunAssociation,
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args_summary: Option<String>,
        /// 参数摘要来自 started 事件；只有 completed 时缺省（不编造参数事实）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args_digest: Option<ContentDigest>,
        status: HistoryToolStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<ToolResultWire>,
        #[serde(with = "u64_wire_string")]
        #[schemars(with = "String")]
        first_seq: u64,
    },
    /// 最终回复（messages 表 `{run_id}-final` 行；content 已过同一
    /// scanner：think→Reasoning 块、mood 剥离）。
    FinalMessage {
        message_id: String,
        #[serde(with = "u64_wire_string")]
        #[schemars(with = "String")]
        seq: u64,
        run: RunAssociation,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_call_id: Option<String>,
        content: Vec<ContentBlock>,
        committed_at_unix_ms: i64,
        #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
        legacy_fields: serde_json::Map<String, serde_json::Value>,
    },
    /// 无 final 的 run 的诚实终态（02 §4：绝不编造 final）。
    RunTerminal {
        run_id: String,
        status: RunStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal_reason: Option<String>,
        #[serde(with = "u64_wire_string")]
        #[schemars(with = "String")]
        event_seq: u64,
    },
    /// 分支重置标记（rewind/retry；T03 形状原样呈现）。
    ResetMarker {
        message_id: String,
        #[serde(with = "u64_wire_string")]
        #[schemars(with = "String")]
        seq: u64,
        reason: String,
        /// 重置目标消息（根重置为 None）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_entry_id: Option<String>,
    },
    /// 不可规范化的旧行原文档案（步骤④：保留，不丢不改）。
    LegacyRaw {
        message_id: String,
        #[serde(with = "u64_wire_string")]
        #[schemars(with = "String")]
        seq: u64,
        raw: serde_json::Value,
        reason: String,
    },
}

/// 分页游标（不透明）：`{"m": message_id, "q": seq}` canonical JSON →
/// base64url NO_PAD。严格解码：任何偏差都是 malformed。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryCursor {
    pub message_id: String,
    pub seq: u64,
}

impl HistoryCursor {
    pub fn encode(&self) -> Cursor {
        let body = serde_json::json!({
            "m": self.message_id,
            "q": self.seq,
        });
        use base64::Engine as _;
        let raw = crate::canon::canonical_string(&body);
        Cursor::new(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes()))
    }

    /// 严格解码（镜像 SubscribeCursor::decode 的失败即 malformed 纪律）。
    pub fn decode(cursor: &Cursor) -> Result<Self, String> {
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(cursor.as_str().as_bytes())
            .map_err(|_| "cursor is not valid base64url".to_string())?;
        let text = String::from_utf8(raw).map_err(|_| "cursor bytes are not UTF-8".to_string())?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|err| format!("cursor body is not JSON: {err}"))?;
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Body {
            m: String,
            q: u64,
        }
        let body: Body = serde_json::from_value(value)
            .map_err(|err| format!("cursor body shape mismatch: {err}"))?;
        Ok(Self {
            message_id: body.m,
            seq: body.q,
        })
    }
}

/// 分页元信息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryPageMeta {
    pub limit: u32,
    pub has_more: bool,
    /// 下一页的 before 游标（has_more=false 时缺省）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_before: Option<Cursor>,
}

/// 一页统一历史投影（GET /history 的响应体）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryPage {
    pub schema_version: String,
    pub session_id: String,
    /// 分支头 revision（ETag 材料；u64 十进制字符串）。
    #[serde(with = "u64_wire_string")]
    #[schemars(with = "String")]
    pub head_revision: u64,
    pub items: Vec<HistoryItem>,
    pub page: HistoryPageMeta,
}

/// 全量导出（GET /export 的响应体；items 与分页遍历全集逐项一致）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryExport {
    pub schema_version: String,
    pub session_id: String,
    #[serde(with = "u64_wire_string")]
    #[schemars(with = "String")]
    pub head_revision: u64,
    pub items: Vec<HistoryItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_roundtrip_and_strict_decode() {
        let cursor = HistoryCursor {
            message_id: "m42".to_string(),
            seq: 42,
        };
        let encoded = cursor.encode();
        let decoded = HistoryCursor::decode(&encoded).expect("roundtrip");
        assert_eq!(decoded, cursor);

        // 畸形输入逐一响亮失败。
        for bad in ["!!!", "AAAA", "", "eyJtIjoibTQyIn0"] {
            assert!(
                HistoryCursor::decode(&Cursor::new(bad)).is_err(),
                "malformed cursor {bad:?} must fail loudly"
            );
        }
    }

    #[test]
    fn seq_fields_are_decimal_strings_on_the_wire() {
        let item = HistoryItem::RunTerminal {
            run_id: "r".to_string(),
            status: RunStatus::Completed,
            terminal_reason: None,
            event_seq: 9_007_199_254_740_993, // 2^53+1：超出 JS 安全整数
        };
        let value = serde_json::to_value(&item).expect("serialize");
        assert_eq!(value["eventSeq"], "9007199254740993");
        assert_eq!(value["kind"], "run_terminal");
        assert_eq!(value["status"], "completed");
    }
}
