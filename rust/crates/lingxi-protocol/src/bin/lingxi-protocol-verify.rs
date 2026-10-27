//! lingxi-protocol-verify — the Rust leg of the cross-language round-trip
//! gate (R01-A03).
//!
//!   1. Reads every golden file (Rust-encoded canonical bytes).
//!   2. Deserializes it into the authoritative Rust type named in
//!      `golden/index.json`, re-serializes canonically, and asserts
//!      byte-equality (Rust encode → Rust read closure).
//!   3. Reads the TypeScript consumer's re-encoded copy from `--ts DIR` and
//!      asserts byte-equality with the golden (Rust encode → TS decode →
//!      TS encode → Rust read closure).
//!
//! Exit 0 only if every sample passes both closures.

use std::path::Path;
use std::process::ExitCode;

use lingxi_protocol::canon::{canonical_bytes, sha256_hex};
use lingxi_protocol::devgate::bound_repo_root;
use lingxi_protocol::*;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Deserialize `bytes` into the authoritative type `type_name`, re-encode
/// canonically, and return the re-encoded bytes.
fn reencode_typed(type_name: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    fn go<T: DeserializeOwned + serde::Serialize>(bytes: &[u8]) -> Result<Vec<u8>, String> {
        let value: T = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        Ok(canonical_bytes(&value))
    }
    match type_name {
        "AgentActivitySnapshot" => go::<AgentActivitySnapshot>(bytes),
        "LoopStatusSnapshot" => go::<LoopStatusSnapshot>(bytes),
        "ActivitySummaryRequest" => go::<ActivitySummaryRequest>(bytes),
        "ActivitySummaryResponse" => go::<ActivitySummaryResponse>(bytes),
        "BeautifyWorkbenchTarget" => go::<BeautifyWorkbenchTarget>(bytes),
        "BeautifyUploadedImage" => go::<BeautifyUploadedImage>(bytes),
        "BeautifyApplyRequest" => go::<BeautifyApplyRequest>(bytes),
        "BeautifyPresetRequest" => go::<BeautifyPresetRequest>(bytes),
        "BeautifyGenerateRequest" => go::<BeautifyGenerateRequest>(bytes),
        "MediaKind" => go::<MediaKind>(bytes),
        "MediaJobPhase" => go::<MediaJobPhase>(bytes),
        "MediaRemoteOutcome" => go::<MediaRemoteOutcome>(bytes),
        "MediaRemoteCancel" => go::<MediaRemoteCancel>(bytes),
        "MediaArtifact" => go::<MediaArtifact>(bytes),
        "MediaJobReceipt" => go::<MediaJobReceipt>(bytes),
        "MediaJobListResponse" => go::<MediaJobListResponse>(bytes),
        "MediaJobResponse" => go::<MediaJobResponse>(bytes),
        "MediaGenerationUpdate" => go::<MediaGenerationUpdate>(bytes),
        "MediaJobCancelRequest" => go::<MediaJobCancelRequest>(bytes),
        "InteractiveCardPut" => go::<InteractiveCardPut>(bytes),
        "InteractiveCardPutResponse" => go::<InteractiveCardPutResponse>(bytes),
        "SettingsArchiveFormat" => go::<SettingsArchiveFormat>(bytes),
        "SettingsArchive" => go::<SettingsArchive>(bytes),
        "ModelConfigurationApplyReport" => go::<ModelConfigurationApplyReport>(bytes),
        "KeybindingsPreferences" => go::<KeybindingsPreferences>(bytes),
        "KeybindingsUpdate" => go::<KeybindingsUpdate>(bytes),
        "SidebarProjectViewPreferences" => go::<SidebarProjectViewPreferences>(bytes),
        "SidebarSessionListRowMode" => go::<SidebarSessionListRowMode>(bytes),
        "SidebarSessionListPreferences" => go::<SidebarSessionListPreferences>(bytes),
        "SidebarUiPreferences" => go::<SidebarUiPreferences>(bytes),
        "SidebarUiResponse" => go::<SidebarUiResponse>(bytes),
        "SidebarUiUpdate" => go::<SidebarUiUpdate>(bytes),
        "CheckpointArchiveMetadata" => go::<CheckpointArchiveMetadata>(bytes),
        "CheckpointArchiveList" => go::<CheckpointArchiveList>(bytes),
        "McpSessionPermissionRequest" => go::<McpSessionPermissionRequest>(bytes),
        "McpSessionPermissionResponse" => go::<McpSessionPermissionResponse>(bytes),
        "McpAppsQuery" => go::<McpAppsQuery>(bytes),
        "McpResourceQuery" => go::<McpResourceQuery>(bytes),
        "McpAppLaunchRequest" => go::<McpAppLaunchRequest>(bytes),
        "McpAppDescriptor" => go::<McpAppDescriptor>(bytes),
        "McpOAuthStartResponse" => go::<McpOAuthStartResponse>(bytes),
        "McpOAuthPollResponse" => go::<McpOAuthPollResponse>(bytes),
        "McpOAuthCancelResponse" => go::<McpOAuthCancelResponse>(bytes),
        "McpAppsResponse" => go::<McpAppsResponse>(bytes),
        "McpAppLaunchResponse" => go::<McpAppLaunchResponse>(bytes),
        "McpOAuthLogoutResponse" => go::<McpOAuthLogoutResponse>(bytes),
        "McpSettingsAction" => go::<McpSettingsAction>(bytes),
        "McpSettingsRequest" => go::<McpSettingsRequest>(bytes),
        "McpSettingsStatus" => go::<McpSettingsStatus>(bytes),
        "McpSettingsFailure" => go::<McpSettingsFailure>(bytes),
        "McpSettingsChange" => go::<McpSettingsChange>(bytes),
        "McpSettingsUpdate" => go::<McpSettingsUpdate>(bytes),
        "McpSettingsResult" => go::<McpSettingsResult>(bytes),
        "McpSettingsState" => go::<McpSettingsState>(bytes),
        "McpConnectorImportResult" => go::<McpConnectorImportResult>(bytes),
        "McpConnectorBulkResult" => go::<McpConnectorBulkResult>(bytes),
        "BrowserAction" => go::<BrowserAction>(bytes),
        "BrowserCommand" => {
            let command: BrowserCommand =
                serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            command.validate().map_err(String::from)?;
            go::<BrowserCommand>(bytes)
        }
        "BrowserSettings" => go::<BrowserSettings>(bytes),
        "BrowserOpenBehavior" => go::<BrowserOpenBehavior>(bytes),
        "BrowserTab" => go::<BrowserTab>(bytes),
        "BrowserWorkspace" => go::<BrowserWorkspace>(bytes),
        "BrowserReply" => go::<BrowserReply>(bytes),
        "AutomationNotificationStatus" => go::<AutomationNotificationStatus>(bytes),
        "AutomationNotificationReceipt" => go::<AutomationNotificationReceipt>(bytes),
        "AutomationNotificationClaim" => go::<AutomationNotificationClaim>(bytes),
        "AutomationDisplayReceipt" => go::<AutomationDisplayReceipt>(bytes),
        "AutomationNotificationCommand" => {
            let command: AutomationNotificationCommand =
                serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
            command.validate().map_err(str::to_owned)?;
            Ok(canonical_bytes(&command))
        }
        "BridgeRole" => go::<BridgeRole>(bytes),
        "BridgeConversationKind" => go::<BridgeConversationKind>(bytes),
        "BridgeAttachmentKind" => go::<BridgeAttachmentKind>(bytes),
        "BridgeAttachment" => go::<BridgeAttachment>(bytes),
        "BridgeReplyContext" => go::<BridgeReplyContext>(bytes),
        "BridgeTargetType" => go::<BridgeTargetType>(bytes),
        "BridgeReceiptStatus" => go::<BridgeReceiptStatus>(bytes),
        "BridgeConnectionStatus" => go::<BridgeConnectionStatus>(bytes),
        "BridgeWorkerCommand" => {
            let command: BridgeWorkerCommand =
                serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            if let BridgeWorkerCommand::Effect { effect, .. } = &command {
                effect.validate().map_err(str::to_owned)?;
            }
            Ok(canonical_bytes(&command))
        }
        "BridgeWorkerEvent" => {
            let event: BridgeWorkerEvent =
                serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            if let BridgeWorkerEvent::EffectReceipt { receipt, .. } = &event {
                receipt.validate().map_err(str::to_owned)?;
            }
            Ok(canonical_bytes(&event))
        }
        "BridgeTakeoverScope" => go::<BridgeTakeoverScope>(bytes),
        "BridgeDiagnosticCommand" => go::<BridgeDiagnosticCommand>(bytes),
        "BridgeQrcodeStatus" => go::<BridgeQrcodeStatus>(bytes),
        "BridgeDiagnosticInfo" => go::<BridgeDiagnosticInfo>(bytes),
        "BridgeDiagnosticResult" => go::<BridgeDiagnosticResult>(bytes),
        "BridgeEffectOperation" => go::<BridgeEffectOperation>(bytes),
        "BridgeEffectStatus" => go::<BridgeEffectStatus>(bytes),
        "BridgeWaitingReceiptMode" => go::<BridgeWaitingReceiptMode>(bytes),
        "BridgeStreamMode" => go::<BridgeStreamMode>(bytes),
        "BridgeWaitingReceiptCapability" => go::<BridgeWaitingReceiptCapability>(bytes),
        "BridgeStreamCapability" => go::<BridgeStreamCapability>(bytes),
        "BridgeEffectCapabilities" => go::<BridgeEffectCapabilities>(bytes),
        "BridgeEffectRequest" => {
            let effect: BridgeEffectRequest =
                serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            effect.validate().map_err(str::to_owned)?;
            Ok(canonical_bytes(&effect))
        }
        "BridgeEffectReceipt" => {
            let receipt: BridgeEffectReceipt =
                serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            receipt.validate().map_err(str::to_owned)?;
            Ok(canonical_bytes(&receipt))
        }
        "ClientHello" => go::<ClientHello>(bytes),
        "ServerHello" => go::<ServerHello>(bytes),
        "ProtocolError" => go::<ProtocolError>(bytes),
        "ContractVersions" => go::<ContractVersions>(bytes),
        // R07-T09：聊天客户端只读上下文。
        "ClientContextQuery" => go::<ClientContextQuery>(bytes),
        "ClientModelSelection" => go::<ClientModelSelection>(bytes),
        "ClientModelState" => go::<ClientModelState>(bytes),
        "ClientModelSummary" => go::<ClientModelSummary>(bytes),
        "ClientModelContext" => go::<ClientModelContext>(bytes),
        "ClientAgentSummary" => go::<ClientAgentSummary>(bytes),
        "ClientAvatarAvailability" => go::<ClientAvatarAvailability>(bytes),
        "MobileBootstrapResponse" => go::<MobileBootstrapResponse>(bytes),
        "EventEnvelope" => go::<EventEnvelope>(bytes),
        "HistoryPage" => go::<Page<HistoryEntry>>(bytes),
        "ModelRequest" => go::<ModelRequest>(bytes),
        "ApprovalRequest" => go::<ApprovalRequest>(bytes),
        "ApprovalDecision" => go::<ApprovalDecision>(bytes),
        "ToolSchemaDocument" => go::<ToolSchemaDocument>(bytes),
        other => Err(format!("no authoritative type registered for {other:?}")),
    }
}

// R06-T06: force-links the default test guard into this bin's TEST HARNESS
// build only (cfg(test)); normal bin and release builds never see it.
#[cfg(test)]
#[allow(unused_imports)]
use lingxi_test_guard as _;

fn main() -> ExitCode {
    // RR-T08-F1 hardening (same rule as lingxi-protocol-gen): bind the
    // repo root from the runtime working directory, never from a
    // compile-time path that a reused target dir could point at another
    // checkout with.
    let bound_root = match bound_repo_root() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    let args: Vec<String> = std::env::args().collect();
    let golden_dir = args
        .windows(2)
        .find(|w| w[0] == "--golden")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| {
            bound_root
                .join("contracts/generated/golden")
                .to_string_lossy()
                .into_owned()
        });
    let ts_dir = args
        .windows(2)
        .find(|w| w[0] == "--ts")
        .map(|w| w[1].clone());

    let index_path = Path::new(&golden_dir).join("index.json");
    let index: Value =
        serde_json::from_slice(&std::fs::read(&index_path).expect("read golden/index.json"))
            .expect("parse golden/index.json");

    let mut failures: Vec<String> = Vec::new();
    let mut count = 0usize;

    for sample in index["samples"].as_array().expect("samples array") {
        count += 1;
        let file = sample["file"].as_str().unwrap();
        let type_name = sample["type"].as_str().unwrap();
        let expected_sha = sample["sha256"].as_str().unwrap();
        let golden_path = Path::new(&golden_dir).join(file);
        let golden = std::fs::read(&golden_path).expect("read golden file");

        if sha256_hex(&golden) != expected_sha {
            failures.push(format!("{file}: sha256 mismatch vs golden/index.json"));
        }

        match reencode_typed(type_name, &golden) {
            Ok(re) if re == golden => {}
            Ok(re) => failures.push(format!(
                "{file}: Rust decode→encode drift ({} vs {} bytes)",
                re.len(),
                golden.len()
            )),
            Err(e) => failures.push(format!("{file}: Rust decode failed: {e}")),
        }

        if let Some(dir) = &ts_dir {
            let ts_path = Path::new(dir).join(file);
            match std::fs::read(&ts_path) {
                Ok(ts_bytes) if ts_bytes == golden => {}
                Ok(ts_bytes) => {
                    // Diagnose: does the TS output at least parse as the same
                    // typed value (semantic equality) even if bytes differ?
                    let note = match reencode_typed(type_name, &ts_bytes) {
                        Ok(re) if re == golden => {
                            " (semantically equal after Rust re-canonicalization; \
                             TS canonicalization differs)"
                        }
                        _ => " (NOT semantically equal)",
                    };
                    failures.push(format!(
                        "{file}: TS re-encoded bytes differ from golden{note}"
                    ));
                }
                Err(e) => failures.push(format!("{file}: TS re-encoded copy unreadable: {e}")),
            }
        }

        // Sample-specific checks recorded in golden/index.json
        for check in sample["checks"]
            .as_array()
            .map(|v| v.as_slice())
            .unwrap_or(&[])
        {
            if check["kind"] == "big_seq_string" {
                let v: Value = serde_json::from_slice(&golden).unwrap();
                let seq = v["seq"].as_str().expect("seq must be a string");
                let parsed: u64 = seq.parse().expect("seq must be a u64 decimal string");
                if parsed < (1u64 << 53) {
                    failures.push(format!(
                        "{file}: big_seq_string sample does not exceed 2^53-1"
                    ));
                }
            }
            // R07-T09：代次十进制字符串与可空默认档位约束同样必须在 Rust
            // 侧实际成立，不能只在 TS 侧校验而在此静默跳过。
            if check["kind"] == "decimal_string_generation" {
                let v: Value = serde_json::from_slice(&golden).unwrap();
                let field = check["field"].as_str().expect("check field");
                let value = v
                    .get(field)
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("{file}: {field} must be a string"));
                let parsed: u64 = value
                    .parse()
                    .unwrap_or_else(|_| panic!("{file}: {field} must be a u64 decimal string"));
                if parsed < (1u64 << 53) {
                    failures.push(format!(
                        "{file}: decimal_string_generation sample does not exceed 2^53-1"
                    ));
                }
            }
            if check["kind"] == "nullable_default_thinking_level" {
                let v: Value = serde_json::from_slice(&golden).unwrap();
                let field = check["field"].as_str().expect("check field");
                match v.get(field) {
                    Some(Value::Null) => {}
                    Some(_) => {
                        failures.push(format!("{file}: {field} must be null in this sample"))
                    }
                    None => failures.push(format!("{file}: {field} was dropped instead of null")),
                }
            }
        }
    }

    if failures.is_empty() {
        println!(
            "OK: {count} golden samples verified (Rust decode→encode byte-stable{})",
            if ts_dir.is_some() {
                "; TS re-encoded copies byte-identical"
            } else {
                ""
            }
        );
        ExitCode::SUCCESS
    } else {
        eprintln!("ROUND-TRIP FAILURES ({}):", failures.len());
        for f in &failures {
            eprintln!("  - {f}");
        }
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod automation_tests {
    use super::*;

    #[test]
    fn notification_commands_have_a_typed_round_trip_and_refuse_false_receipts() {
        for value in [
            serde_json::json!({"action":"claim","deliveryId":"d1"}),
            serde_json::json!({"action":"ack","deliveryId":"d1","platformReceipt":{"deliveryId":"d1","channel":"desktop","displayed":true}}),
            serde_json::json!({"action":"failed","deliveryId":"d1","error":"未显示"}),
            serde_json::json!({"action":"unknown","deliveryId":"d1","error":"展示结果未知"}),
        ] {
            let bytes = canonical_bytes(&value);
            assert_eq!(
                reencode_typed("AutomationNotificationCommand", &bytes).unwrap(),
                bytes
            );
        }
        for value in [
            serde_json::json!({"action":"claim","deliveryId":"d1","displayed":true}),
            serde_json::json!({"action":"ack","deliveryId":"d1","platformReceipt":{"deliveryId":"other","channel":"desktop","displayed":true}}),
            serde_json::json!({"action":"ack","deliveryId":"d1","platformReceipt":{"deliveryId":"d1","channel":"desktop","displayed":false}}),
        ] {
            assert!(
                reencode_typed("AutomationNotificationCommand", &canonical_bytes(&value)).is_err()
            );
        }
    }
}
