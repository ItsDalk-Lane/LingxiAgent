//! MCP 应用目录、资源请求和连接器独立登录的公开合同；不包含凭据。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpAppsQuery {
    pub agent_id: Option<String>,
    pub connector_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpResourceQuery {
    pub uri: String,
    pub agent_id: Option<String>,
}
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpAppLaunchRequest {
    pub launch_input: Option<Value>,
    pub input: Option<Value>,
    pub agent_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpAppDescriptor {
    pub connector_id: String,
    pub server_id: String,
    pub tool_name: String,
    pub title: String,
    pub description: String,
    pub resource_uri: String,
    pub visibility: Vec<String>,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub output_schema: Option<Value>,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none", default)]
    pub meta: Option<Value>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpAppsResponse {
    pub apps: Vec<McpAppDescriptor>,
    pub state: crate::McpSettingsState,
}
/// launch 只返回现役描述与输入绑定，不表示已执行工具。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpAppLaunchResponse {
    #[serde(rename = "type")]
    pub kind: String,
    pub connector_id: String,
    pub server_id: String,
    pub tool_name: String,
    pub title: String,
    pub description: String,
    pub resource_uri: String,
    pub visibility: Vec<String>,
    pub launch_input: Value,
    pub binding: Value,
    pub app: McpAppDescriptor,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpOAuthStartResponse {
    pub session_id: String,
    pub url: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpOAuthPollResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub credential_persistence: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpOAuthCancelResponse {
    pub cancelled: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpOAuthLogoutResponse {
    pub connector: Value,
    pub state: crate::McpSettingsState,
}
