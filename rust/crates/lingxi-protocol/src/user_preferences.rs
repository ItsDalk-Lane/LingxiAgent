//! 用户界面偏好与全局改前存档的只读元数据合同；存档正文不进入返回类型。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeybindingsPreferences {
    pub keybindings: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeybindingsUpdate {
    pub ok: bool,
    pub keybindings: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SidebarProjectViewPreferences {
    pub collapsed_project_ids: Vec<String>,
    pub collapsed_folder_ids: Vec<String>,
    pub show_all_project_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SidebarSessionListRowMode {
    #[default]
    TwoLine,
    SingleLine,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SidebarSessionListPreferences {
    pub row_mode: SidebarSessionListRowMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SidebarUiPreferences {
    pub project_view: SidebarProjectViewPreferences,
    pub session_list: SidebarSessionListPreferences,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SidebarUiResponse {
    pub sidebar_ui: SidebarUiPreferences,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SidebarUiUpdate {
    pub ok: bool,
    pub sidebar_ui: SidebarUiPreferences,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointArchiveMetadata {
    pub id: String,
    pub ts: u64,
    pub tool: String,
    pub source: String,
    pub reason: String,
    pub path: String,
    pub size: u64,
    pub session_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckpointArchiveList {
    pub checkpoints: Vec<CheckpointArchiveMetadata>,
}
