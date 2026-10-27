//! 工作台活动、摘要与循环状态的共用接口；现役数字时间戳保持兼容。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivityKind {
    Subagent,
    Workflow,
    WorkflowAgent,
    WorkflowStep,
    Heartbeat,
    Cron,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivityStatus {
    Running,
    Done,
    Failed,
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepKind {
    Parallel,
    Pipeline,
    Log,
}

/// 快照与实时活动使用同一条目。扩展字段原样保留，归属及状态必须可解析。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(transform = activity_required_nullable)]
pub struct AgentActivityEntry {
    pub id: String,
    pub kind: AgentActivityKind,
    pub status: AgentActivityStatus,
    pub session_id: Option<String>,
    pub session_path: Option<String>,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub summary: Option<String>,
    pub child_session_id: Option<String>,
    pub child_session_path: Option<String>,
    pub thread_id: Option<String>,
    pub thread_kind: Option<String>,
    pub access: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub parent_task_id: Option<String>,
    pub label: Option<String>,
    pub phase_label: Option<String>,
    pub tokens: Option<i64>,
    pub step_kind: Option<WorkflowStepKind>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentActivitySnapshot {
    pub entries: Vec<AgentActivityEntry>,
}

/// id 缺失保留现役业务错误响应；不将旧的 HTTP 200 错误改成解析错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActivitySummaryRequest {
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum ActivitySummaryResponse {
    Summary { summary: Option<String> },
    Error { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoopState {
    Running,
    Paused,
    Stopped,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(transform = loop_required_nullable)]
pub struct LoopStatusSnapshot {
    pub status: LoopState,
    pub turn_count: u32,
    pub max_turns: Option<u32>,
    pub paused_reason: Option<String>,
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_error: Option<String>,
}

// 必填且允许 null 与“非空必填”不同；不使用会去掉 null 的 required 字段属性。
fn add_required(schema: &mut schemars::Schema, fields: &[&str]) {
    let required = schema
        .ensure_object()
        .entry("required")
        .or_insert_with(|| serde_json::json!([]));
    let required = required.as_array_mut().expect("生成的必填字段列表");
    for field in fields {
        let field = Value::String((*field).to_owned());
        if !required.contains(&field) {
            required.push(field);
        }
    }
}

fn activity_required_nullable(schema: &mut schemars::Schema) {
    add_required(
        schema,
        &[
            "sessionPath",
            "agentId",
            "agentName",
            "summary",
            "childSessionPath",
            "startedAt",
            "finishedAt",
        ],
    );
}

fn loop_required_nullable(schema: &mut schemars::Schema) {
    add_required(schema, &["maxTurns", "pausedReason", "prompt"]);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_error_cannot_decode_as_an_empty_success() {
        let raw = serde_json::json!({"error":"remote result unknown"});
        let response: ActivitySummaryResponse = serde_json::from_value(raw.clone()).unwrap();
        assert!(matches!(response, ActivitySummaryResponse::Error { .. }));
        assert_eq!(serde_json::to_value(response).unwrap(), raw);
        assert!(serde_json::from_value::<ActivitySummaryResponse>(
            serde_json::json!({"summary":null,"error":"failed"})
        )
        .is_err());
        let empty: ActivitySummaryResponse =
            serde_json::from_value(serde_json::json!({"summary":null})).unwrap();
        assert!(matches!(
            empty,
            ActivitySummaryResponse::Summary { summary: None }
        ));
    }
    #[test]
    fn required_nullable_fields_keep_null_in_generated_contract() {
        for (schema, field) in [
            (schemars::schema_for!(AgentActivityEntry), "finishedAt"),
            (schemars::schema_for!(LoopStatusSnapshot), "pausedReason"),
        ] {
            let schema = schema.to_value();
            assert!(schema["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(field)));
            assert!(schema["properties"][field]["type"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("null")));
        }
    }
    #[test]
    fn activity_receipt_roundtrip_keeps_nullable_and_unknown_compatibility_fields() {
        let raw = serde_json::json!({"id":"owned", "kind":"workflow", "status":"running",
            "sessionId":"session-owned", "sessionPath":"native:session-owned",
            "finishedAt":null, "agentId":null, "reason":"receipt unknown",
            "futureCompatible":{"values":[1, false, null]}});
        let entry: AgentActivityEntry = serde_json::from_value(raw.clone()).unwrap();
        let output = serde_json::to_value(AgentActivitySnapshot {
            entries: vec![entry],
        })
        .unwrap();
        assert_eq!(output["entries"][0]["finishedAt"], Value::Null);
        assert_eq!(output["entries"][0]["agentId"], Value::Null);
        assert_eq!(output["entries"][0]["reason"], raw["reason"]);
        assert_eq!(
            output["entries"][0]["futureCompatible"],
            raw["futureCompatible"]
        );
        let mut invalid = raw;
        invalid["status"] = serde_json::json!("pretend_success");
        assert!(serde_json::from_value::<AgentActivityEntry>(invalid).is_err());
    }
}
