//! 当前会话内的 MCP 能力授权；不写入连接器或持久化配置。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSessionPermissionRequest {
    pub session_id: String,
    pub target_id: String,
    pub capability: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSessionPermissionResponse {
    pub ok: bool,
    pub session_id: String,
    pub target_id: String,
    pub capability: String,
    pub persisted: bool,
}
