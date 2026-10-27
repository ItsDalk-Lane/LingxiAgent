//! 聊天客户端只读上下文：不包含供应商地址、凭据或自由模型元数据。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientContextQuery {
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientModelSelection {
    pub id: String,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClientModelState {
    Ready,
    Unconfigured,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientModelSummary {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub is_current: bool,
    pub reasoning: bool,
    pub xhigh: bool,
    pub thinking_levels: Vec<String>,
    pub default_thinking_level: Option<String>,
    pub audio: bool,
    pub audio_transport: Option<String>,
    pub audio_transport_supported: bool,
    pub input: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientModelContext {
    pub models: Vec<ClientModelSummary>,
    pub active_model: Option<ClientModelSelection>,
    pub model_state: ClientModelState,
    /// 代次以十进制字符串传输，不能因客户端数值精度丢失归属。
    pub model_generation: String,
    pub thinking_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientAgentSummary {
    pub id: String,
    pub name: String,
    pub yuan: String,
    pub is_primary: bool,
    pub is_current: bool,
    pub has_avatar: bool,
    pub home_folder: Option<String>,
    pub effective_home_folder: Option<String>,
    pub memory_master_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientAvatarAvailability {
    pub agent: bool,
    pub user: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MobileBootstrapResponse {
    pub locale: String,
    pub agent_name: String,
    pub user_name: String,
    pub current_agent_id: Option<String>,
    pub agent_yuan: String,
    pub home_folder: Option<String>,
    pub cwd_history: Vec<String>,
    pub memory_master_enabled: bool,
    pub memory_enabled: bool,
    pub thinking_level: String,
    pub permission_mode: String,
    /// 这里只能放入已归一化的已知外观、排版和聊天布局字段。
    pub appearance: Value,
    pub editor: Value,
    pub chat: Value,
    pub avatars: ClientAvatarAvailability,
    pub agents: Vec<ClientAgentSummary>,
    pub models: Vec<ClientModelSummary>,
    pub active_model: Option<ClientModelSelection>,
    pub current_model: Option<ClientModelSelection>,
    pub model_state: ClientModelState,
    pub model_generation: String,
}
