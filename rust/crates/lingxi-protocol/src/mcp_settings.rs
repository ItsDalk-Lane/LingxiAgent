//! MCP 管理请求和实际回执；身份、审批事实不接受客户端自报。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum McpSettingsAction {
    #[serde(rename = "mcp.global.enabled")]
    GlobalEnabled,
    #[serde(rename = "mcp.global.defer")]
    GlobalDefer,
    #[serde(rename = "mcp.connector.add")]
    ConnectorAdd,
    #[serde(rename = "mcp.connector.add_bulk")]
    ConnectorAddBulk,
    #[serde(rename = "mcp.connector.update")]
    ConnectorUpdate,
    #[serde(rename = "mcp.connector.remove")]
    ConnectorRemove,
    #[serde(rename = "mcp.connector.start")]
    ConnectorStart,
    #[serde(rename = "mcp.connector.stop")]
    ConnectorStop,
    #[serde(rename = "mcp.connector.refresh_tools")]
    ConnectorRefreshTools,
    #[serde(rename = "mcp.agent.connector.enable")]
    AgentConnectorEnable,
    #[serde(rename = "mcp.agent.tool.enable")]
    AgentToolEnable,
}
impl McpSettingsAction {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::GlobalEnabled => "mcp.global.enabled",
            Self::GlobalDefer => "mcp.global.defer",
            Self::ConnectorAdd => "mcp.connector.add",
            Self::ConnectorAddBulk => "mcp.connector.add_bulk",
            Self::ConnectorUpdate => "mcp.connector.update",
            Self::ConnectorRemove => "mcp.connector.remove",
            Self::ConnectorStart => "mcp.connector.start",
            Self::ConnectorStop => "mcp.connector.stop",
            Self::ConnectorRefreshTools => "mcp.connector.refresh_tools",
            Self::AgentConnectorEnable => "mcp.agent.connector.enable",
            Self::AgentToolEnable => "mcp.agent.tool.enable",
        }
    }
}

/// payload 由对应动作的管理器校验；兼容连接器保留字段，不可夹带身份或授权。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSettingsRequest {
    pub action: McpSettingsAction,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default = "empty_payload")]
    pub payload: Value,
}
fn empty_payload() -> Value {
    serde_json::json!({})
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpSettingsStatus {
    Applied,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSettingsFailure {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSettingsChange {
    pub key: String,
    pub label: String,
    pub before: String,
    pub after: String,
}

/// saved 与 runtimeApplied 分开记录；失败或取消不抹去已保存的用户决定。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSettingsUpdate {
    pub status: McpSettingsStatus,
    pub action: String,
    pub key: String,
    pub title: String,
    pub summary: String,
    pub saved: bool,
    pub runtime_applied: bool,
    pub connector_id: Option<String>,
    pub agent_id: Option<String>,
    #[serde(default)]
    pub changes: Vec<McpSettingsChange>,
    pub error: Option<McpSettingsFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSettingsResult {
    pub settings_update: McpSettingsUpdate,
}

/// 页面读取同一管理者的公开配置；回执保留各动作原有未知扩展。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSettingsState {
    pub config: Value,
    pub agent_id: Option<String>,
    pub agent_config: Value,
    pub settings_results: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpConnectorImportResult {
    pub ok: bool,
    pub id: Option<String>,
    pub error: Option<String>,
    pub saved: bool,
    pub runtime_applied: bool,
}

/// 输入全批预检后仅保存一次；逐连接启动失败不抹去已保存的配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpConnectorBulkResult {
    pub settings_update: McpSettingsUpdate,
    pub results: Vec<McpConnectorImportResult>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn management_defer_and_bulk_requests_roundtrip() {
        for (action, payload) in [
            (
                McpSettingsAction::GlobalDefer,
                serde_json::json!({"builtinDeferEnabled":true}),
            ),
            (
                McpSettingsAction::ConnectorAddBulk,
                serde_json::json!({"connectors":[{"name":"fixture","transport":"stdio","command":"owned-fixture"}]}),
            ),
        ] {
            let request = McpSettingsRequest {
                action,
                agent_id: None,
                payload,
            };
            let value = serde_json::to_value(&request).unwrap();
            assert_eq!(value["action"], action.wire_name());
            assert_eq!(
                serde_json::from_value::<McpSettingsRequest>(value).unwrap(),
                request
            );
        }
    }
    #[test]
    fn all_nine_mcp_actions_and_partial_failure_roundtrip() {
        for action in [
            McpSettingsAction::GlobalEnabled,
            McpSettingsAction::ConnectorAdd,
            McpSettingsAction::ConnectorUpdate,
            McpSettingsAction::ConnectorRemove,
            McpSettingsAction::ConnectorStart,
            McpSettingsAction::ConnectorStop,
            McpSettingsAction::ConnectorRefreshTools,
            McpSettingsAction::AgentConnectorEnable,
            McpSettingsAction::AgentToolEnable,
        ] {
            let request = McpSettingsRequest {
                action,
                agent_id: Some("b".into()),
                payload: serde_json::json!({"connectorId":"fixture","enabled":false}),
            };
            let wire = serde_json::to_value(&request).unwrap();
            assert_eq!(wire["action"], action.wire_name());
            assert_eq!(
                serde_json::from_value::<McpSettingsRequest>(wire).unwrap(),
                request
            );
        }
        let result = McpSettingsResult {
            settings_update: McpSettingsUpdate {
                status: McpSettingsStatus::Failed,
                action: "mcp.connector.start".into(),
                key: "mcp.connector.start".into(),
                title: "连接未完成".into(),
                summary: "启动意图已保存".into(),
                saved: true,
                runtime_applied: false,
                connector_id: Some("fixture".into()),
                agent_id: None,
                changes: vec![],
                error: Some(McpSettingsFailure {
                    code: "mcp_connect_failed".into(),
                    message: "连接未完成".into(),
                }),
            },
        };
        assert_eq!(
            serde_json::from_str::<McpSettingsResult>(&serde_json::to_string(&result).unwrap())
                .unwrap(),
            result
        );
        assert!(serde_json::from_value::<McpSettingsRequest>(
            serde_json::json!({"action":"mcp.connector.start","principal":{"owner":true}})
        )
        .is_err());
    }
}
