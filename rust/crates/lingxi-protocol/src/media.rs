//! 媒体作业的公开回执；不含宿主路径、凭据或供应商原始响应。
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
    Speech,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaJobPhase {
    Submitting,
    Submitted,
    Generating,
    Downloading,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaRemoteOutcome {
    Failed,
    NotYetConfirmed,
    NotSubmitted,
    Accepted,
    Completed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaRemoteCancel {
    Unsupported,
    Confirmed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaArtifact {
    pub file_id: String,
    pub filename: String,
    pub mime: String,
    #[serde(with = "crate::u64_wire_string")]
    #[schemars(with = "String")]
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaJobReceipt {
    pub task_id: String,
    pub kind: MediaKind,
    pub provider: String,
    pub status: MediaJobPhase,
    pub source_run_id: String,
    pub execution_run_id: Option<String>,
    #[serde(with = "crate::u64_wire_string")]
    #[schemars(with = "String")]
    pub revision: u64,
    pub files: Vec<MediaArtifact>,
    pub remote_outcome: MediaRemoteOutcome,
    pub remote_cancel: Option<MediaRemoteCancel>,
    pub local_delivery_stopped: bool,
    pub message: Option<String>,
    pub error_code: Option<String>,
    #[serde(with = "crate::u64_wire_string")]
    #[schemars(with = "String")]
    pub created_at_ms: u64,
    #[serde(with = "crate::u64_wire_string")]
    #[schemars(with = "String")]
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaJobListResponse {
    pub jobs: Vec<MediaJobReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaJobResponse {
    pub job: MediaJobReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaGenerationUpdate {
    pub session_id: String,
    pub media_generation: MediaJobReceipt,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaJobCancelRequest {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artifact_precision_and_closed_identity_are_preserved() {
        let artifact = MediaArtifact {
            file_id: "actual-file".into(),
            filename: "图.png".into(),
            mime: "image/png".into(),
            size_bytes: u64::MAX,
        };
        let value = serde_json::to_value(&artifact).unwrap();
        assert_eq!(value["sizeBytes"], u64::MAX.to_string());
        assert_eq!(
            serde_json::from_value::<MediaArtifact>(value.clone()).unwrap(),
            artifact
        );
        let mut forged = value.clone();
        forged["hostPath"] = serde_json::json!("/private/file");
        assert!(serde_json::from_value::<MediaArtifact>(forged).is_err());
        let mut number = value;
        number["sizeBytes"] = serde_json::json!(42);
        assert!(serde_json::from_value::<MediaArtifact>(number).is_err());
        assert!(serde_json::from_value::<MediaJobCancelRequest>(
            serde_json::json!({"owner":"forged"})
        )
        .is_err());
    }
}
