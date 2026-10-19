//! 办公与封面入口只携带资源描述；调用者身份由服务认证。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeautifyWorkbenchTarget {
    pub kind: String,
    pub mount_id: Option<String>,
    pub root_id: Option<String>,
    #[serde(default)]
    pub subdir: String,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeautifyUploadedImage {
    #[serde(default, alias = "name")]
    pub filename: Option<String>,
    pub content_base64: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeautifyApplyRequest {
    pub file_path: Option<String>,
    #[serde(alias = "remoteContentRef")]
    pub target: Option<BeautifyWorkbenchTarget>,
    #[serde(alias = "generatedFilePath")]
    pub image_file_path: Option<String>,
    pub image: Option<BeautifyUploadedImage>,
    pub request_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeautifyPresetRequest {
    pub file_path: Option<String>,
    #[serde(alias = "remoteContentRef")]
    pub target: Option<BeautifyWorkbenchTarget>,
    pub preset_id: String,
    pub request_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeautifyGenerateRequest {
    pub file_path: Option<String>,
    #[serde(alias = "remoteContentRef")]
    pub target: Option<BeautifyWorkbenchTarget>,
    pub executor_agent_id: Option<String>,
    pub agent_id: Option<String>,
    pub theme_tone: Option<String>,
    pub user_guidance: Option<String>,
    pub request_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatible_fields_are_preserved_but_identity_cannot_be_supplied() {
        let value = serde_json::json!({"remoteContentRef":{"kind":"mobile-workbench","rootId":"mount","subdir":"notes","name":"paper.md"},"image":{"name":"cover.png","contentBase64":"YWJj"},"requestId":"request-stable"});
        let request: BeautifyApplyRequest = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            request.image.unwrap().filename.as_deref(),
            Some("cover.png")
        );
        assert_eq!(request.target.unwrap().root_id.as_deref(), Some("mount"));
        let mut forged = value;
        forged["principal"] = serde_json::json!("local-owner");
        assert!(serde_json::from_value::<BeautifyApplyRequest>(forged).is_err());
    }
}
