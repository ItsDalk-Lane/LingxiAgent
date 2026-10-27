//! 设置文件交换与互动卡片的共用边界。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractiveCardPut {
    pub code: String,
    pub title: Option<String>,
    #[serde(default)]
    pub vars_css: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InteractiveCardPutResponse {
    pub ok: bool,
    pub frame_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SettingsArchiveFormat {
    #[serde(rename = "lingxi-settings")]
    LingxiSettings,
}

/// 内部设置沿现役字段校验；根结构关闭，禁止夹带其他导入动作。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsArchive {
    pub format: SettingsArchiveFormat,
    pub version: u32,
    pub config: Value,
    pub model_preferences: Value,
    pub experiments: BTreeMap<String, Value>,
}

/// 同一配置来源保存与运行时发布的实际回执，失败时也保留已完成的事实。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelConfigurationApplyReport {
    pub saved: bool,
    pub applied: bool,
    #[serde(with = "crate::u64_wire_string")]
    #[schemars(with = "String")]
    pub generation: u64,
    pub activated: bool,
    pub cleared_bindings: Vec<String>,
    pub audit_recorded: bool,
    pub projection_confirmed: bool,
}
