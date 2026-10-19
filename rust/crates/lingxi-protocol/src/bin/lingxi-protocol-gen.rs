//! lingxi-protocol-gen — generates every derived protocol artifact from the
//! authoritative serde types in this crate:
//!
//!   contracts/generated/jsonschema/<Type>.schema.json   JSON Schema 2020-12
//!   contracts/generated/ts/lingxi-protocol.ts           TypeScript bindings
//!   contracts/generated/golden/<name>.json              cross-language golden
//!   contracts/generated/golden/index.json               golden metadata
//!   contracts/generated/MANIFEST.json                   sha256 manifest
//!
//! Generated output must never be hand-edited. `--check` regenerates
//! everything in memory and fails (exit 1, diff summary on stderr) if the
//! on-disk tree differs — that is the "regenerate → no diff" CI gate.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lingxi_protocol::canon::{canonical_bytes, sha256_hex, CANONICALIZATION_ID};
use lingxi_protocol::handshake::{accept_hello, negotiate_protocol};
use lingxi_protocol::*;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{json, Map, Value};

// ── Schema registry ────────────────────────────────────────────────────────

fn root_schema<T: JsonSchema>() -> Value {
    let settings = schemars::generate::SchemaSettings::draft2020_12();
    let generator = settings.into_generator();
    generator.into_root_schema_for::<T>().to_value()
}

/// Every protocol type exported as a standalone JSON Schema document, in
/// deterministic registration order.
fn schema_registry() -> Vec<(&'static str, Value)> {
    vec![
        (
            "KeybindingsPreferences",
            root_schema::<KeybindingsPreferences>(),
        ),
        ("KeybindingsUpdate", root_schema::<KeybindingsUpdate>()),
        (
            "SidebarProjectViewPreferences",
            root_schema::<SidebarProjectViewPreferences>(),
        ),
        (
            "SidebarSessionListRowMode",
            root_schema::<SidebarSessionListRowMode>(),
        ),
        (
            "SidebarSessionListPreferences",
            root_schema::<SidebarSessionListPreferences>(),
        ),
        (
            "SidebarUiPreferences",
            root_schema::<SidebarUiPreferences>(),
        ),
        ("SidebarUiResponse", root_schema::<SidebarUiResponse>()),
        ("SidebarUiUpdate", root_schema::<SidebarUiUpdate>()),
        (
            "CheckpointArchiveMetadata",
            root_schema::<CheckpointArchiveMetadata>(),
        ),
        (
            "CheckpointArchiveList",
            root_schema::<CheckpointArchiveList>(),
        ),
        (
            "McpSessionPermissionRequest",
            root_schema::<McpSessionPermissionRequest>(),
        ),
        (
            "McpSessionPermissionResponse",
            root_schema::<McpSessionPermissionResponse>(),
        ),
        ("McpAppsQuery", root_schema::<McpAppsQuery>()),
        ("McpResourceQuery", root_schema::<McpResourceQuery>()),
        ("McpAppLaunchRequest", root_schema::<McpAppLaunchRequest>()),
        ("McpAppDescriptor", root_schema::<McpAppDescriptor>()),
        (
            "McpOAuthStartResponse",
            root_schema::<McpOAuthStartResponse>(),
        ),
        (
            "McpOAuthPollResponse",
            root_schema::<McpOAuthPollResponse>(),
        ),
        (
            "McpOAuthCancelResponse",
            root_schema::<McpOAuthCancelResponse>(),
        ),
        ("McpAppsResponse", root_schema::<McpAppsResponse>()),
        (
            "McpAppLaunchResponse",
            root_schema::<McpAppLaunchResponse>(),
        ),
        (
            "McpOAuthLogoutResponse",
            root_schema::<McpOAuthLogoutResponse>(),
        ),
        ("McpSettingsAction", root_schema::<McpSettingsAction>()),
        ("McpSettingsRequest", root_schema::<McpSettingsRequest>()),
        ("McpSettingsStatus", root_schema::<McpSettingsStatus>()),
        ("McpSettingsFailure", root_schema::<McpSettingsFailure>()),
        ("McpSettingsChange", root_schema::<McpSettingsChange>()),
        ("McpSettingsUpdate", root_schema::<McpSettingsUpdate>()),
        ("McpSettingsResult", root_schema::<McpSettingsResult>()),
        ("McpSettingsState", root_schema::<McpSettingsState>()),
        (
            "McpConnectorImportResult",
            root_schema::<McpConnectorImportResult>(),
        ),
        (
            "McpConnectorBulkResult",
            root_schema::<McpConnectorBulkResult>(),
        ),
        ("InteractiveCardPut", root_schema::<InteractiveCardPut>()),
        (
            "InteractiveCardPutResponse",
            root_schema::<InteractiveCardPutResponse>(),
        ),
        (
            "SettingsArchiveFormat",
            root_schema::<SettingsArchiveFormat>(),
        ),
        ("SettingsArchive", root_schema::<SettingsArchive>()),
        (
            "ModelConfigurationApplyReport",
            root_schema::<ModelConfigurationApplyReport>(),
        ),
        ("AgentActivityKind", root_schema::<AgentActivityKind>()),
        ("AgentActivityStatus", root_schema::<AgentActivityStatus>()),
        ("WorkflowStepKind", root_schema::<WorkflowStepKind>()),
        ("AgentActivityEntry", root_schema::<AgentActivityEntry>()),
        (
            "AgentActivitySnapshot",
            root_schema::<AgentActivitySnapshot>(),
        ),
        (
            "ActivitySummaryRequest",
            root_schema::<ActivitySummaryRequest>(),
        ),
        (
            "ActivitySummaryResponse",
            root_schema::<ActivitySummaryResponse>(),
        ),
        ("LoopState", root_schema::<LoopState>()),
        ("LoopStatusSnapshot", root_schema::<LoopStatusSnapshot>()),
        (
            "AutomationNotificationStatus",
            root_schema::<AutomationNotificationStatus>(),
        ),
        (
            "AutomationNotificationReceipt",
            root_schema::<AutomationNotificationReceipt>(),
        ),
        (
            "AutomationNotificationClaim",
            root_schema::<AutomationNotificationClaim>(),
        ),
        (
            "AutomationDisplayReceipt",
            root_schema::<AutomationDisplayReceipt>(),
        ),
        (
            "AutomationNotificationCommand",
            root_schema::<AutomationNotificationCommand>(),
        ),
        (
            "BeautifyWorkbenchTarget",
            root_schema::<BeautifyWorkbenchTarget>(),
        ),
        (
            "BeautifyUploadedImage",
            root_schema::<BeautifyUploadedImage>(),
        ),
        (
            "BeautifyApplyRequest",
            root_schema::<BeautifyApplyRequest>(),
        ),
        (
            "BeautifyPresetRequest",
            root_schema::<BeautifyPresetRequest>(),
        ),
        (
            "BeautifyGenerateRequest",
            root_schema::<BeautifyGenerateRequest>(),
        ),
        ("MediaKind", root_schema::<MediaKind>()),
        ("MediaJobPhase", root_schema::<MediaJobPhase>()),
        ("MediaRemoteOutcome", root_schema::<MediaRemoteOutcome>()),
        ("MediaRemoteCancel", root_schema::<MediaRemoteCancel>()),
        ("MediaArtifact", root_schema::<MediaArtifact>()),
        ("MediaJobReceipt", root_schema::<MediaJobReceipt>()),
        (
            "MediaJobListResponse",
            root_schema::<MediaJobListResponse>(),
        ),
        ("MediaJobResponse", root_schema::<MediaJobResponse>()),
        (
            "MediaGenerationUpdate",
            root_schema::<MediaGenerationUpdate>(),
        ),
        (
            "MediaJobCancelRequest",
            root_schema::<MediaJobCancelRequest>(),
        ),
        ("BrowserAction", root_schema::<BrowserAction>()),
        ("BrowserCommand", root_schema::<BrowserCommand>()),
        ("BrowserSettings", root_schema::<BrowserSettings>()),
        ("BrowserOpenBehavior", root_schema::<BrowserOpenBehavior>()),
        ("BrowserTab", root_schema::<BrowserTab>()),
        ("BrowserWorkspace", root_schema::<BrowserWorkspace>()),
        ("BrowserReply", root_schema::<BrowserReply>()),
        ("BridgeRole", root_schema::<BridgeRole>()),
        (
            "BridgeConversationKind",
            root_schema::<BridgeConversationKind>(),
        ),
        (
            "BridgeAttachmentKind",
            root_schema::<BridgeAttachmentKind>(),
        ),
        ("BridgeAttachment", root_schema::<BridgeAttachment>()),
        ("BridgeReplyContext", root_schema::<BridgeReplyContext>()),
        ("BridgeTargetType", root_schema::<BridgeTargetType>()),
        ("BridgeReceiptStatus", root_schema::<BridgeReceiptStatus>()),
        (
            "BridgeConnectionStatus",
            root_schema::<BridgeConnectionStatus>(),
        ),
        ("BridgeWorkerCommand", root_schema::<BridgeWorkerCommand>()),
        ("BridgeWorkerEvent", root_schema::<BridgeWorkerEvent>()),
        ("BridgeTakeoverScope", root_schema::<BridgeTakeoverScope>()),
        (
            "BridgeDiagnosticCommand",
            root_schema::<BridgeDiagnosticCommand>(),
        ),
        ("BridgeQrcodeStatus", root_schema::<BridgeQrcodeStatus>()),
        (
            "BridgeDiagnosticInfo",
            root_schema::<BridgeDiagnosticInfo>(),
        ),
        (
            "BridgeDiagnosticResult",
            root_schema::<BridgeDiagnosticResult>(),
        ),
        (
            "BridgeEffectOperation",
            root_schema::<BridgeEffectOperation>(),
        ),
        ("BridgeEffectStatus", root_schema::<BridgeEffectStatus>()),
        (
            "BridgeWaitingReceiptMode",
            root_schema::<BridgeWaitingReceiptMode>(),
        ),
        ("BridgeStreamMode", root_schema::<BridgeStreamMode>()),
        (
            "BridgeWaitingReceiptCapability",
            root_schema::<BridgeWaitingReceiptCapability>(),
        ),
        (
            "BridgeStreamCapability",
            root_schema::<BridgeStreamCapability>(),
        ),
        (
            "BridgeEffectCapabilities",
            root_schema::<BridgeEffectCapabilities>(),
        ),
        ("BridgeEffectRequest", root_schema::<BridgeEffectRequest>()),
        ("BridgeEffectReceipt", root_schema::<BridgeEffectReceipt>()),
        ("SessionId", root_schema::<SessionId>()),
        ("RunId", root_schema::<RunId>()),
        ("AttemptId", root_schema::<AttemptId>()),
        ("ModelCallId", root_schema::<ModelCallId>()),
        ("ToolCallId", root_schema::<ToolCallId>()),
        ("ResourceId", root_schema::<ResourceId>()),
        ("StreamId", root_schema::<StreamId>()),
        ("EventId", root_schema::<EventId>()),
        ("Seq", root_schema::<Seq>()),
        ("RunStatus", root_schema::<RunStatus>()),
        ("ErrorCode", root_schema::<ErrorCode>()),
        ("ProtocolError", root_schema::<ProtocolError>()),
        ("ContractVersions", root_schema::<ContractVersions>()),
        ("ClientHello", root_schema::<ClientHello>()),
        ("ServerHello", root_schema::<ServerHello>()),
        ("Cursor", root_schema::<Cursor>()),
        ("PageRequest", root_schema::<PageRequest>()),
        ("HistoryEntry", root_schema::<HistoryEntry>()),
        ("HistoryPage", root_schema::<Page<HistoryEntry>>()),
        ("ContentDigest", root_schema::<ContentDigest>()),
        ("ResourceKind", root_schema::<ResourceKind>()),
        ("ResourceRef", root_schema::<ResourceRef>()),
        ("AssistantPhase", root_schema::<AssistantPhase>()),
        ("SegmentKind", root_schema::<SegmentKind>()),
        ("ContentBlock", root_schema::<ContentBlock>()),
        ("NormalizedMessage", root_schema::<NormalizedMessage>()),
        ("PrincipalKind", root_schema::<PrincipalKind>()),
        ("PrincipalRef", root_schema::<PrincipalRef>()),
        ("Budget", root_schema::<Budget>()),
        ("ModelRequest", root_schema::<ModelRequest>()),
        ("UsageRecord", root_schema::<UsageRecord>()),
        ("ToolCallDescriptor", root_schema::<ToolCallDescriptor>()),
        ("ToolResultStatus", root_schema::<ToolResultStatus>()),
        ("ToolResultWire", root_schema::<ToolResultWire>()),
        ("ApprovalRequest", root_schema::<ApprovalRequest>()),
        ("ApprovalDecision", root_schema::<ApprovalDecision>()),
        ("KnownEventPayload", root_schema::<KnownEventPayload>()),
        ("EventPayload", root_schema::<EventPayload>()),
        ("EventEnvelope", root_schema::<EventEnvelope>()),
        ("ToolSchemaDocument", root_schema::<ToolSchemaDocument>()),
        // R07-T09：聊天客户端只读上下文（CLI/mobile 共享；无供应商地址/凭据）。
        ("ClientContextQuery", root_schema::<ClientContextQuery>()),
        (
            "ClientModelSelection",
            root_schema::<ClientModelSelection>(),
        ),
        ("ClientModelState", root_schema::<ClientModelState>()),
        ("ClientModelSummary", root_schema::<ClientModelSummary>()),
        ("ClientModelContext", root_schema::<ClientModelContext>()),
        ("ClientAgentSummary", root_schema::<ClientAgentSummary>()),
        (
            "ClientAvatarAvailability",
            root_schema::<ClientAvatarAvailability>(),
        ),
        (
            "MobileBootstrapResponse",
            root_schema::<MobileBootstrapResponse>(),
        ),
    ]
}

// ── TypeScript emitter ─────────────────────────────────────────────────────

fn sanitize_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

struct TsEmitter {
    /// Global $defs pool: sanitized name -> schema. Collisions must be
    /// identical (same Rust type reached from multiple roots).
    defs: BTreeMap<String, Value>,
    def_order: Vec<String>,
}

impl TsEmitter {
    fn new() -> Self {
        Self {
            defs: BTreeMap::new(),
            def_order: Vec::new(),
        }
    }

    fn absorb_root(&mut self, root: &Value) {
        if let Some(defs) = root.get("$defs").and_then(|d| d.as_object()) {
            for (name, schema) in defs {
                let clean = sanitize_name(name);
                match self.defs.get(&clean) {
                    Some(existing) => assert_eq!(
                        existing, schema,
                        "conflicting $defs for {clean}: the same name maps to two schemas"
                    ),
                    None => {
                        self.defs.insert(clean.clone(), schema.clone());
                        self.def_order.push(clean);
                    }
                }
            }
        }
    }

    fn ts_for(&self, schema: &Value) -> String {
        if schema.as_bool() == Some(true) || schema.as_object().is_some_and(|o| o.is_empty()) {
            return "unknown".to_string();
        }
        if let Some(r) = schema.get("$ref").and_then(|r| r.as_str()) {
            let name = r.rsplit('/').next().unwrap_or(r);
            return sanitize_name(name);
        }
        if let Some(c) = schema.get("const") {
            return literal_ts(c);
        }
        if let Some(e) = schema.get("enum").and_then(|e| e.as_array()) {
            return e.iter().map(literal_ts).collect::<Vec<_>>().join(" | ");
        }
        for (key, joiner) in [("anyOf", " | "), ("oneOf", " | ")] {
            if let Some(variants) = schema.get(key).and_then(|v| v.as_array()) {
                let mut parts: Vec<String> = variants.iter().map(|v| self.ts_for(v)).collect();
                parts.sort();
                parts.dedup();
                if parts.len() == 1 {
                    return parts.pop().unwrap();
                }
                return parts
                    .iter()
                    .map(|p| {
                        if p.starts_with('{') || p.contains(" & ") {
                            format!("({p})")
                        } else {
                            p.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(joiner);
            }
        }
        if let Some(parts) = schema.get("allOf").and_then(|v| v.as_array()) {
            return parts
                .iter()
                .map(|v| self.ts_for(v))
                .collect::<Vec<_>>()
                .join(" & ");
        }
        match schema.get("type").and_then(|t| t.as_str()) {
            Some("string") => "string".to_string(),
            Some("integer") | Some("number") => "number".to_string(),
            Some("boolean") => "boolean".to_string(),
            Some("null") => "null".to_string(),
            Some("array") => {
                let item = schema
                    .get("items")
                    .map(|i| self.ts_for(i))
                    .unwrap_or_else(|| "unknown".to_string());
                if item.contains(" | ") || item.contains(" & ") {
                    format!("({item})[]")
                } else {
                    format!("{item}[]")
                }
            }
            Some("object") => self.object_ts(schema),
            _ => {
                // type may be an array of types (e.g. ["string","null"])
                if let Some(types) = schema.get("type").and_then(|t| t.as_array()) {
                    return types
                        .iter()
                        .map(|t| match t.as_str() {
                            Some("null") => "null".to_string(),
                            Some("string") => "string".to_string(),
                            Some("integer") | Some("number") => "number".to_string(),
                            Some("boolean") => "boolean".to_string(),
                            Some("array") => "unknown[]".to_string(),
                            Some("object") => "Record<string, unknown>".to_string(),
                            _ => "unknown".to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                }
                "unknown".to_string()
            }
        }
    }

    fn object_ts(&self, schema: &Value) -> String {
        let props = schema.get("properties").and_then(|p| p.as_object());
        let required: Vec<&str> = schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let additional = schema.get("additionalProperties");
        if props.is_none() {
            return match additional {
                Some(Value::Bool(true)) | None => "Record<string, unknown>".to_string(),
                Some(s) if s.is_object() => format!("Record<string, {}>", self.ts_for(s)),
                _ => "Record<string, never>".to_string(),
            };
        }
        let props = props.unwrap();
        let mut fields = String::new();
        for (name, prop_schema) in props {
            let optional = if required.contains(&name.as_str()) {
                ""
            } else {
                "?"
            };
            let description = prop_schema.get("description").and_then(|d| d.as_str());
            if let Some(d) = description {
                fields.push_str(&format!("  /** {} */\n", d.replace("*/", "* /")));
            }
            fields.push_str(&format!(
                "  {}{optional}: {};\n",
                json_prop_name(name),
                self.ts_for(prop_schema)
            ));
        }
        match additional {
            Some(Value::Bool(true)) => {
                fields.push_str("  [key: string]: unknown;\n");
            }
            Some(s) if s.is_object() && s.as_bool() != Some(false) => {
                fields.push_str(&format!("  [key: string]: {};\n", self.ts_for(s)));
            }
            _ => {}
        }
        format!("{{\n{fields}}}")
    }

    fn render(&self, registry: &[(&str, Value)]) -> String {
        let mut out = String::new();
        out.push_str(
            "/**\n \
             * GENERATED FILE — DO NOT EDIT.\n \
             * Authority: rust/crates/lingxi-protocol (serde types).\n \
             * Regenerate: cargo run -p lingxi-protocol --bin lingxi-protocol-gen\n \
             * Verify:     cargo run -p lingxi-protocol --bin lingxi-protocol-gen -- --check\n \
             */\n\n",
        );
        for name in &self.def_order {
            let schema = &self.defs[name];
            let description = schema.get("description").and_then(|d| d.as_str());
            if let Some(d) = description {
                out.push_str(&format!("/** {} */\n", d.replace("*/", "* /")));
            }
            out.push_str(&format!(
                "export type {name} = {};\n\n",
                self.ts_for(schema)
            ));
        }
        for (type_name, root) in registry {
            if self.defs.contains_key(&sanitize_name(type_name)) {
                continue; // already emitted as a def
            }
            let mut shallow = root.clone();
            if let Some(obj) = shallow.as_object_mut() {
                obj.remove("$defs");
            }
            out.push_str(&format!(
                "export type {} = {};\n\n",
                sanitize_name(type_name),
                self.ts_for(&shallow)
            ));
        }
        out
    }
}

fn literal_ts(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

fn json_prop_name(name: &str) -> String {
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
        && !name.is_empty()
    {
        name.to_string()
    } else {
        format!("\"{name}\"")
    }
}

// ── Golden fixtures ────────────────────────────────────────────────────────

struct Golden {
    file: &'static str,
    type_name: &'static str,
    bytes: Vec<u8>,
    checks: Vec<Value>,
}

const LONG_TOOL_CALL_ID: &str =
    "toolcall-0123456789abcdef-0123456789abcdef-0123456789abcdef-0123456789abcdef-\
     0123456789abcdef-0123456789abcdef-0123456789abcdef-0123456789abcdef-ZH-工具调用";

fn golden_samples() -> Vec<Golden> {
    let mut out: Vec<Golden> = Vec::new();
    push(
        &mut out,
        "user-preferences-KeybindingsPreferences.json",
        "KeybindingsPreferences",
        &serde_json::from_value::<KeybindingsPreferences>(
            json!({"keybindings":{"app.new-session":["Alt+N"]}}),
        )
        .expect("内置协议样本必须有效"),
        vec![],
    );
    push(
        &mut out,
        "user-preferences-KeybindingsUpdate.json",
        "KeybindingsUpdate",
        &serde_json::from_value::<KeybindingsUpdate>(json!({"ok":true,"keybindings":{}}))
            .expect("内置协议样本必须有效"),
        vec![],
    );
    push(
        &mut out,
        "user-preferences-SidebarProjectViewPreferences.json",
        "SidebarProjectViewPreferences",
        &serde_json::from_value::<SidebarProjectViewPreferences>(
            json!({"collapsedProjectIds":[],"collapsedFolderIds":[],"showAllProjectIds":[]}),
        )
        .expect("内置协议样本必须有效"),
        vec![],
    );
    push(
        &mut out,
        "user-preferences-SidebarSessionListRowMode.json",
        "SidebarSessionListRowMode",
        &serde_json::from_value::<SidebarSessionListRowMode>(json!("single-line"))
            .expect("内置协议样本必须有效"),
        vec![],
    );
    push(
        &mut out,
        "user-preferences-SidebarSessionListPreferences.json",
        "SidebarSessionListPreferences",
        &serde_json::from_value::<SidebarSessionListPreferences>(json!({"rowMode":"single-line"}))
            .expect("内置协议样本必须有效"),
        vec![],
    );
    push(&mut out, "user-preferences-SidebarUiPreferences.json", "SidebarUiPreferences", &serde_json::from_value::<SidebarUiPreferences>(json!({"projectView":{"collapsedProjectIds":[],"collapsedFolderIds":[],"showAllProjectIds":[]},"sessionList":{"rowMode":"two-line"}})).expect("内置协议样本必须有效"), vec![]);
    push(&mut out, "user-preferences-SidebarUiResponse.json", "SidebarUiResponse", &serde_json::from_value::<SidebarUiResponse>(json!({"sidebarUi":{"projectView":{"collapsedProjectIds":[],"collapsedFolderIds":[],"showAllProjectIds":[]},"sessionList":{"rowMode":"two-line"}}})).expect("内置协议样本必须有效"), vec![]);
    push(&mut out, "user-preferences-SidebarUiUpdate.json", "SidebarUiUpdate", &serde_json::from_value::<SidebarUiUpdate>(json!({"ok":true,"sidebarUi":{"projectView":{"collapsedProjectIds":[],"collapsedFolderIds":[],"showAllProjectIds":[]},"sessionList":{"rowMode":"two-line"}}})).expect("内置协议样本必须有效"), vec![]);
    push(&mut out, "user-preferences-CheckpointArchiveMetadata.json", "CheckpointArchiveMetadata", &serde_json::from_value::<CheckpointArchiveMetadata>(json!({"id":"fixture-archive","ts":1000,"tool":"user-edit","source":"user-edit","reason":"edit-start","path":"/owned/fixture.txt","size":3,"sessionPath":null})).expect("内置协议样本必须有效"), vec![]);
    push(
        &mut out,
        "user-preferences-CheckpointArchiveList.json",
        "CheckpointArchiveList",
        &serde_json::from_value::<CheckpointArchiveList>(json!({"checkpoints":[]}))
            .expect("内置协议样本必须有效"),
        vec![],
    );

    push(
        &mut out,
        "mcp-session-permission-request.json",
        "McpSessionPermissionRequest",
        &McpSessionPermissionRequest {
            session_id: "session-a".into(),
            target_id: "tool:mcp:fixture:echo".into(),
            capability: "mcp.echo.invoke".into(),
        },
        vec![],
    );
    push(
        &mut out,
        "mcp-session-permission-response.json",
        "McpSessionPermissionResponse",
        &McpSessionPermissionResponse {
            ok: true,
            session_id: "session-a".into(),
            target_id: "tool:mcp:fixture:echo".into(),
            capability: "mcp.echo.invoke".into(),
            persisted: false,
        },
        vec![],
    );

    push(
        &mut out,
        "mcp-apps-McpAppsQuery.json",
        "McpAppsQuery",
        &serde_json::from_value::<McpAppsQuery>(json!({"agentId":null,"connectorId":null}))
            .expect("内置协议样本必须有效"),
        vec![],
    );
    push(
        &mut out,
        "mcp-apps-McpResourceQuery.json",
        "McpResourceQuery",
        &serde_json::from_value::<McpResourceQuery>(json!({"uri":"ui://fixture","agentId":null}))
            .expect("内置协议样本必须有效"),
        vec![],
    );
    push(
        &mut out,
        "mcp-apps-McpAppLaunchRequest.json",
        "McpAppLaunchRequest",
        &serde_json::from_value::<McpAppLaunchRequest>(
            json!({"launchInput":{"query":"fixture"},"input":null,"agentId":null}),
        )
        .expect("内置协议样本必须有效"),
        vec![],
    );
    push(&mut out, "mcp-apps-McpAppDescriptor.json", "McpAppDescriptor", &serde_json::from_value::<McpAppDescriptor>(json!({"connectorId":"fixture","serverId":"fixture","toolName":"view","title":"View","description":"fixture","resourceUri":"ui://fixture","visibility":["app"],"inputSchema":{"type":"object"}})).expect("内置协议样本必须有效"), vec![]);
    push(&mut out, "mcp-apps-McpOAuthStartResponse.json", "McpOAuthStartResponse", &serde_json::from_value::<McpOAuthStartResponse>(json!({"sessionId":"opaque-fixture-state","url":"http://127.0.0.1/authorize?state=opaque-fixture-state"})).expect("内置协议样本必须有效"), vec![]);
    push(
        &mut out,
        "mcp-apps-McpOAuthPollResponse.json",
        "McpOAuthPollResponse",
        &serde_json::from_value::<McpOAuthPollResponse>(json!({"status":"pending"}))
            .expect("内置协议样本必须有效"),
        vec![],
    );
    push(
        &mut out,
        "mcp-apps-McpOAuthCancelResponse.json",
        "McpOAuthCancelResponse",
        &serde_json::from_value::<McpOAuthCancelResponse>(json!({"cancelled":1}))
            .expect("内置协议样本必须有效"),
        vec![],
    );
    push(&mut out, "mcp-apps-McpAppsResponse.json", "McpAppsResponse", &serde_json::from_value::<McpAppsResponse>(json!({"apps":[{"connectorId":"fixture","serverId":"fixture","toolName":"view","title":"View","description":"fixture","resourceUri":"ui://fixture","visibility":["app"],"inputSchema":{"type":"object"}}],"state":{"config":{"enabled":false,"connectors":[]},"agentId":null,"agentConfig":{"connectors":{}},"settingsResults":[]}})).expect("内置协议样本必须有效"), vec![]);
    push(&mut out, "mcp-apps-McpAppLaunchResponse.json", "McpAppLaunchResponse", &serde_json::from_value::<McpAppLaunchResponse>(json!({"connectorId":"fixture","serverId":"fixture","toolName":"view","title":"View","description":"fixture","resourceUri":"ui://fixture","visibility":["app"],"type":"mcp_app","launchInput":{},"binding":{"kind":"mcp-app"},"app":{"connectorId":"fixture","serverId":"fixture","toolName":"view","title":"View","description":"fixture","resourceUri":"ui://fixture","visibility":["app"],"inputSchema":{"type":"object"}}})).expect("内置协议样本必须有效"), vec![]);
    push(&mut out, "mcp-apps-McpOAuthLogoutResponse.json", "McpOAuthLogoutResponse", &serde_json::from_value::<McpOAuthLogoutResponse>(json!({"connector":{"id":"fixture","authStatus":"needs_auth"},"state":{"config":{"enabled":false,"connectors":[]},"agentId":null,"agentConfig":{"connectors":{}},"settingsResults":[]}})).expect("内置协议样本必须有效"), vec![]);

    push(
        &mut out,
        "mcp-settings-action.json",
        "McpSettingsAction",
        &McpSettingsAction::ConnectorStop,
        vec![],
    );
    push(
        &mut out,
        "mcp-settings-status.json",
        "McpSettingsStatus",
        &McpSettingsStatus::Cancelled,
        vec![],
    );
    push(
        &mut out,
        "mcp-settings-failure.json",
        "McpSettingsFailure",
        &McpSettingsFailure {
            code: "mcp_connect_failed".into(),
            message: "连接未完成".into(),
        },
        vec![],
    );
    push(
        &mut out,
        "mcp-settings-change.json",
        "McpSettingsChange",
        &McpSettingsChange {
            key: "mcp.connector.stop".into(),
            label: "fixture".into(),
            before: "运行中".into(),
            after: "已停用，重启后仍停用".into(),
        },
        vec![],
    );
    push(
        &mut out,
        "mcp-settings-update.json",
        "McpSettingsUpdate",
        &McpSettingsUpdate {
            status: McpSettingsStatus::Applied,
            action: "mcp.connector.stop".into(),
            key: "mcp.connector.stop".into(),
            title: "连接器已停用".into(),
            summary: "fixture 已停止，重启应用后仍保持停用。".into(),
            saved: true,
            runtime_applied: true,
            connector_id: Some("fixture".into()),
            agent_id: Some("b".into()),
            changes: vec![McpSettingsChange {
                key: "mcp.connector.stop".into(),
                label: "fixture".into(),
                before: "运行中".into(),
                after: "已停用，重启后仍停用".into(),
            }],
            error: None,
        },
        vec![],
    );
    push(
        &mut out,
        "mcp-settings-request.json",
        "McpSettingsRequest",
        &McpSettingsRequest {
            action: McpSettingsAction::AgentToolEnable,
            agent_id: Some("b".into()),
            payload: json!({"connectorId":"fixture","toolName":"echo","enabled":false}),
        },
        vec![],
    );
    push(
        &mut out,
        "mcp-settings-partial-failure.json",
        "McpSettingsResult",
        &McpSettingsResult {
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
        },
        vec![],
    );
    push(
        &mut out,
        "mcp-defer-request.json",
        "McpSettingsRequest",
        &McpSettingsRequest {
            action: McpSettingsAction::GlobalDefer,
            agent_id: None,
            payload: json!({"builtinDeferEnabled":true}),
        },
        vec![],
    );
    push(
        &mut out,
        "mcp-bulk-request.json",
        "McpSettingsRequest",
        &McpSettingsRequest {
            action: McpSettingsAction::ConnectorAddBulk,
            agent_id: None,
            payload: json!({"connectors":[{"name":"fixture","transport":"stdio","command":"owned-fixture"}]}),
        },
        vec![],
    );
    push(
        &mut out,
        "mcp-settings-state.json",
        "McpSettingsState",
        &McpSettingsState {
            config: json!({"enabled":false,"connectors":[],"builtinDeferEnabled":true}),
            agent_id: Some("a".into()),
            agent_config: json!({"connectors":{}}),
            settings_results: vec![],
        },
        vec![],
    );
    push(
        &mut out,
        "mcp-import-row.json",
        "McpConnectorImportResult",
        &McpConnectorImportResult {
            ok: false,
            id: Some("fixture".into()),
            error: Some("启动未完成".into()),
            saved: true,
            runtime_applied: false,
        },
        vec![],
    );
    push(
        &mut out,
        "mcp-import-result.json",
        "McpConnectorBulkResult",
        &McpConnectorBulkResult {
            settings_update: McpSettingsUpdate {
                status: McpSettingsStatus::Failed,
                action: "mcp.connector.add_bulk".into(),
                key: "mcp.connector.add_bulk".into(),
                title: "连接器已保存，部分启动未完成".into(),
                summary: "1项已保存；启动失败不回滚配置".into(),
                saved: true,
                runtime_applied: false,
                connector_id: None,
                agent_id: None,
                changes: vec![],
                error: None,
            },
            results: vec![McpConnectorImportResult {
                ok: false,
                id: Some("fixture".into()),
                error: Some("启动未完成".into()),
                saved: true,
                runtime_applied: false,
            }],
        },
        vec![],
    );
    push(
        &mut out,
        "interactive-card-put.json",
        "InteractiveCardPut",
        &InteractiveCardPut {
            code: "<p>你好</p>".into(),
            title: Some("卡片".into()),
            vars_css: String::new(),
        },
        vec![],
    );
    push(
        &mut out,
        "interactive-card-put-response.json",
        "InteractiveCardPutResponse",
        &InteractiveCardPutResponse {
            ok: true,
            frame_url: format!("/lingxi/v1/cards/example/frame/{}", "a".repeat(64)),
        },
        vec![],
    );
    push(
        &mut out,
        "settings-archive-format.json",
        "SettingsArchiveFormat",
        &SettingsArchiveFormat::LingxiSettings,
        vec![],
    );
    push(
        &mut out,
        "settings-archive.json",
        "SettingsArchive",
        &SettingsArchive {
            format: SettingsArchiveFormat::LingxiSettings,
            version: 1,
            config: json!({"locale":"zh-CN"}),
            model_preferences: json!({"models":{"title":null}}),
            experiments: BTreeMap::new(),
        },
        vec![],
    );
    push(
        &mut out,
        "model-configuration-apply-report.json",
        "ModelConfigurationApplyReport",
        &ModelConfigurationApplyReport {
            saved: true,
            applied: true,
            generation: 9_007_199_254_740_993,
            activated: true,
            cleared_bindings: vec!["knowledge".into()],
            audit_recorded: true,
            projection_confirmed: true,
        },
        vec![],
    );
    let activity: AgentActivityEntry = serde_json::from_value(json!({
        "id":"workflow-owned", "kind":"workflow", "status":"running",
        "sessionId":"session-owned", "sessionPath":"native:session-owned",
        "agentId":null, "agentName":null, "summary":"真实后台任务",
        "childSessionPath":null, "startedAt":1_700_000_000_000_i64, "finishedAt":null,
        "resumeSafe":false, "futureCompatible":{"kept":true}
    }))
    .expect("固定活动样本");
    push(
        &mut out,
        "agent-activity-snapshot.json",
        "AgentActivitySnapshot",
        &AgentActivitySnapshot {
            entries: vec![activity],
        },
        vec![],
    );
    push(
        &mut out,
        "loop-status-unconfirmed.json",
        "LoopStatusSnapshot",
        &LoopStatusSnapshot {
            status: LoopState::Running,
            turn_count: 2,
            max_turns: Some(20),
            paused_reason: None,
            prompt: Some("核对文件".into()),
            runtime_error: Some("保存失败，自动推进已停止，状态尚未确认".into()),
        },
        vec![],
    );
    push(
        &mut out,
        "activity-summary-request.json",
        "ActivitySummaryRequest",
        &ActivitySummaryRequest {
            id: Some("activity-owned".into()),
        },
        vec![],
    );
    push(
        &mut out,
        "activity-summary-null.json",
        "ActivitySummaryResponse",
        &ActivitySummaryResponse::Summary { summary: None },
        vec![],
    );
    push(
        &mut out,
        "activity-summary-error.json",
        "ActivitySummaryResponse",
        &ActivitySummaryResponse::Error {
            error: "remote result unknown".into(),
        },
        vec![],
    );
    let target = BeautifyWorkbenchTarget {
        kind: "workbench-file".into(),
        mount_id: Some("mount-owned".into()),
        root_id: None,
        subdir: "notes".into(),
        name: "文档.md".into(),
    };
    push(
        &mut out,
        "beautify-workbench-target.json",
        "BeautifyWorkbenchTarget",
        &target,
        vec![],
    );
    let uploaded = BeautifyUploadedImage {
        filename: Some("cover.png".into()),
        content_base64: "YWJj".into(),
    };
    push(
        &mut out,
        "beautify-uploaded-image.json",
        "BeautifyUploadedImage",
        &uploaded,
        vec![],
    );
    push(
        &mut out,
        "beautify-apply-request.json",
        "BeautifyApplyRequest",
        &BeautifyApplyRequest {
            file_path: None,
            target: Some(target.clone()),
            image_file_path: None,
            image: Some(uploaded),
            request_id: Some("request-stable".into()),
        },
        vec![],
    );
    push(
        &mut out,
        "beautify-preset-request.json",
        "BeautifyPresetRequest",
        &BeautifyPresetRequest {
            file_path: None,
            target: Some(target.clone()),
            preset_id: "landscape".into(),
            request_id: Some("request-stable".into()),
        },
        vec![],
    );
    push(
        &mut out,
        "beautify-generate-request.json",
        "BeautifyGenerateRequest",
        &BeautifyGenerateRequest {
            file_path: None,
            target: Some(target),
            executor_agent_id: Some("agent-selected".into()),
            agent_id: None,
            theme_tone: Some("light".into()),
            user_guidance: Some("自然风景".into()),
            request_id: Some("request-stable".into()),
        },
        vec![],
    );

    let media_job = MediaJobReceipt {
        task_id: "media-stable-operation".into(),
        kind: MediaKind::Image,
        provider: "configured-provider".into(),
        status: MediaJobPhase::Completed,
        source_run_id: "run-source".into(),
        execution_run_id: Some("run-media".into()),
        revision: 9_007_199_254_740_993,
        files: vec![MediaArtifact {
            file_id: "file-actual".into(),
            filename: "图片.png".into(),
            mime: "image/png".into(),
            size_bytes: 9_007_199_254_740_993,
        }],
        remote_outcome: MediaRemoteOutcome::Completed,
        remote_cancel: None,
        local_delivery_stopped: false,
        message: Some("已生成并交付实际文件".into()),
        error_code: None,
        created_at_ms: 1_700_000_000_000,
        updated_at_ms: 1_700_000_001_000,
    };
    for (file, phase) in [
        ("media-submitting.json", MediaJobPhase::Submitting),
        ("media-submitted.json", MediaJobPhase::Submitted),
        ("media-generating.json", MediaJobPhase::Generating),
        ("media-downloading.json", MediaJobPhase::Downloading),
        ("media-completed.json", MediaJobPhase::Completed),
        ("media-failed.json", MediaJobPhase::Failed),
        ("media-cancelled.json", MediaJobPhase::Cancelled),
        ("media-unknown.json", MediaJobPhase::Unknown),
    ] {
        let mut job = media_job.clone();
        job.status = phase;
        if phase != MediaJobPhase::Completed {
            job.files.clear();
        }
        if phase == MediaJobPhase::Failed {
            job.remote_outcome = MediaRemoteOutcome::Failed;
        }
        if phase == MediaJobPhase::Cancelled {
            job.local_delivery_stopped = true;
            job.remote_cancel = Some(MediaRemoteCancel::Unsupported);
            job.message = Some("已停止本地交付；远端可能仍在运行".into());
        }
        push(&mut out, file, "MediaJobReceipt", &job, vec![]);
    }
    push(
        &mut out,
        "media-artifact.json",
        "MediaArtifact",
        &media_job.files[0],
        vec![],
    );
    push(
        &mut out,
        "media-job-list.json",
        "MediaJobListResponse",
        &MediaJobListResponse {
            jobs: vec![media_job.clone()],
        },
        vec![],
    );
    push(
        &mut out,
        "media-job-response.json",
        "MediaJobResponse",
        &MediaJobResponse {
            job: media_job.clone(),
        },
        vec![],
    );
    push(
        &mut out,
        "media-generation-update.json",
        "MediaGenerationUpdate",
        &MediaGenerationUpdate {
            session_id: "session-owned".into(),
            media_generation: media_job,
        },
        vec![],
    );
    push(
        &mut out,
        "media-cancel-request.json",
        "MediaJobCancelRequest",
        &MediaJobCancelRequest {},
        vec![],
    );

    push(
        &mut out,
        "browser-action.json",
        "BrowserAction",
        &BrowserAction::Navigate,
        vec![],
    );
    push(
        &mut out,
        "browser-settings.json",
        "BrowserSettings",
        &BrowserSettings::default(),
        vec![],
    );
    push(
        &mut out,
        "browser-open-behavior.json",
        "BrowserOpenBehavior",
        &BrowserOpenBehavior::Smart,
        vec![],
    );
    let tab = BrowserTab {
        tab_id: "controlled-tab-1".into(),
        title: "本地样本页面".into(),
        url: Some("http://127.0.0.1/fixture".into()),
        can_go_back: false,
        can_go_forward: false,
        created_at: "2026-10-16T00:00:00Z".into(),
        updated_at: "2026-10-16T00:00:01Z".into(),
    };
    push(&mut out, "browser-tab.json", "BrowserTab", &tab, vec![]);
    let workspace = BrowserWorkspace {
        session_id: "sample-session".into(),
        session_path: "native:sample-session".into(),
        execution_node: "actual-service-node".into(),
        running: true,
        resumable: true,
        unavailable_reason: None,
        url: tab.url.clone(),
        title: Some(tab.title.clone()),
        active_tab_id: Some(tab.tab_id.clone()),
        tabs: vec![tab],
        suspended: false,
        user_takeover: false,
        revoked: false,
        visible_capable: true,
    };
    push(
        &mut out,
        "browser-workspace.json",
        "BrowserWorkspace",
        &workspace,
        vec![],
    );
    let mut browser_command = BrowserCommand::new(BrowserAction::Navigate);
    browser_command.url = Some("http://127.0.0.1/fixture".into());
    push(
        &mut out,
        "browser-command.json",
        "BrowserCommand",
        &browser_command,
        vec![],
    );
    push(
        &mut out,
        "browser-reply.json",
        "BrowserReply",
        &BrowserReply {
            state: workspace,
            files: vec![],
            downloads: vec![],
            result: json!({"url":"http://127.0.0.1/fixture","title":"本地样本页面"}),
        },
        vec![],
    );

    fn push<T: Serialize>(
        out: &mut Vec<Golden>,
        file: &'static str,
        type_name: &'static str,
        value: &T,
        checks: Vec<Value>,
    ) {
        out.push(Golden {
            file,
            type_name,
            bytes: canonical_bytes(value),
            checks,
        });
    }

    // 1. handshake: client hello
    let hello = ClientHello {
        protocol: "lingxi.wire".into(),
        client_kind: "desktop".into(),
        client_version: "0.1.0-proto".into(),
        protocol_min: 1,
        protocol_max: 1,
        caps: vec!["events.v1".into()],
    };
    push(&mut out, "client-hello.json", "ClientHello", &hello, vec![]);

    // 2. handshake: server hello
    let server_hello = accept_hello(
        &hello,
        1,
        "lingxi-proto-server",
        "0.0.0-r01t02",
        1,
        &["events.v1"],
    );
    push(
        &mut out,
        "server-hello.json",
        "ServerHello",
        &server_hello,
        vec![],
    );

    // 3. handshake failure: version incompatible (client 2..=2)
    let mut too_new = hello.clone();
    too_new.protocol_min = 2;
    too_new.protocol_max = 2;
    let incompatible = negotiate_protocol(&too_new).unwrap_err();
    push(
        &mut out,
        "error-version-incompatible.json",
        "ProtocolError",
        &incompatible,
        vec![],
    );

    // 4. event: assistant segment delta — Chinese text, seq beyond 2^53-1
    let beyond = Seq::new((1u64 << 53) + 11); // 9007199254741003
    let env_delta = EventEnvelope::new(
        EventId::new("evt-0001-中文事件"),
        StreamId::new("stream-session-1"),
        beyond,
        SessionId::new("sess-灵犀-0001"),
        Some(RunId::new("run-0001")),
        Some(AttemptId::new("attempt-1")),
        EventPayload::Known(KnownEventPayload::AssistantSegmentDelta(
            AssistantSegmentDeltaPayload {
                segment_id: "seg-0001".into(),
                delta: "你好，这是灵犀的中文流式增量。".into(),
                semantic_phase: AssistantPhase::FinalAnswer,
            },
        )),
    );
    push(
        &mut out,
        "event-assistant-segment-delta.json",
        "EventEnvelope",
        &env_delta,
        vec![json!({
            "kind": "big_seq_string",
            "field": "seq",
            "note": "seq exceeds 2^53-1; TS must keep it a string/BigInt, never a Number",
        })],
    );

    // 5. event: run terminal failed with error (错误样本)
    let env_failed = EventEnvelope::new(
        EventId::new("evt-0002"),
        StreamId::new("stream-session-1"),
        Seq::new(9007),
        SessionId::new("sess-灵犀-0001"),
        Some(RunId::new("run-0001")),
        Some(AttemptId::new("attempt-1")),
        EventPayload::Known(KnownEventPayload::RunStateChanged(RunStateChangedPayload {
            from: RunStatus::Running,
            to: RunStatus::Failed,
            reason: Some("模型调用失败：上游超时（示例错误）".into()),
        })),
    );
    push(
        &mut out,
        "event-run-state-failed.json",
        "EventEnvelope",
        &env_failed,
        vec![],
    );

    // 6a. event: tool call started — carries the normalized-argument digest
    // (参数摘要) the approval boundary binds to; the TS side recomputes
    // sha256(canonical(args)) from the recorded `args` and asserts equality
    // with the digest embedded in this golden.
    let args = json!({"command": "ls -la", "path": "/tmp/灵犀测试", "timeoutMs": 30000});
    let args_digest = digest_arguments(&args);
    let env_tool_started = EventEnvelope::new(
        EventId::new("evt-0002a"),
        StreamId::new("stream-session-1"),
        Seq::new(9009),
        SessionId::new("sess-灵犀-0001"),
        Some(RunId::new("run-0001")),
        Some(AttemptId::new("attempt-1")),
        EventPayload::Known(KnownEventPayload::ToolCallStarted(ToolCallStartedPayload {
            tool_call: ToolCallDescriptor {
                tool_call_id: ToolCallId::new(LONG_TOOL_CALL_ID),
                target: "exec_command".into(),
                args_digest: args_digest.clone(),
                args_summary: Some("exec_command: ls -la /tmp/灵犀测试".into()),
            },
        })),
    );
    push(
        &mut out,
        "event-tool-call-started.json",
        "EventEnvelope",
        &env_tool_started,
        vec![json!({
            "kind": "args_digest",
            "args": args,
            "digestField": ["payload", "toolCall", "argsDigest", "hex"],
            "note": "TS recomputes sha256(canonical(args)) and asserts equality with the digest embedded at digestField",
        })],
    );

    // 6b. event: tool call completed with attachment + opaque block (附件样本)
    let attachment = ResourceRef {
        resource_id: ResourceId::new("res-attachment-报告-0001"),
        kind: ResourceKind::Attachment,
        display_name: Some("季度报告（第三季度）.pdf".into()),
        uri: Some("lingxi-resource://res-attachment-报告-0001/content".into()),
        digest: Some(ContentDigest {
            algorithm: "sha256".into(),
            canonicalization: CANONICALIZATION_ID.into(),
            hex: sha256_hex("fake-pdf-bytes-for-golden".as_bytes()),
        }),
        // > 2^53-1 to prove the decimal-string rule on u64 fields
        size_bytes: Some((1u64 << 53) + 99),
    };
    let tool_result = ToolResultWire {
        details: None,
        status: ToolResultStatus::Success,
        content: vec![
            ContentBlock::Text {
                text: "已生成附件并保存。".into(),
            },
            ContentBlock::ResourceRef {
                resource: attachment.clone(),
            },
            ContentBlock::Opaque {
                provider: "anthropic".into(),
                data: json!({"signature": "EqQBCkgIAR...REDACTED-示例签名", "thinking": "加密的思考块占位"}),
            },
        ],
        resource_refs: vec![attachment],
        truncated: false,
        error: None,
    };
    let env_tool = EventEnvelope::new(
        EventId::new("evt-0003"),
        StreamId::new("stream-session-1"),
        Seq::new(9010),
        SessionId::new("sess-灵犀-0001"),
        Some(RunId::new("run-0001")),
        Some(AttemptId::new("attempt-1")),
        EventPayload::Known(KnownEventPayload::ToolCallCompleted(
            ToolCallCompletedPayload {
                tool_call_id: ToolCallId::new(LONG_TOOL_CALL_ID),
                result: tool_result,
            },
        )),
    );
    push(
        &mut out,
        "event-tool-call-completed.json",
        "EventEnvelope",
        &env_tool,
        vec![],
    );

    // 7. event: unknown future event type preserved verbatim
    let unknown_raw: Map<String, Value> = serde_json::from_value(json!({
        "type": "future_recall_started",
        "permissionHint": "memory.write",
        "payload": {"zh": "未来事件，必须原样保留", "nested": [1, 2, 3]},
    }))
    .unwrap();
    let env_unknown = EventEnvelope::new(
        EventId::new("evt-0004"),
        StreamId::new("stream-session-1"),
        Seq::new(9011),
        SessionId::new("sess-灵犀-0001"),
        None,
        None,
        EventPayload::Unknown(UnknownEventPayload {
            event_type: "future_recall_started".into(),
            raw: unknown_raw,
        }),
    );
    push(
        &mut out,
        "event-unknown-future.json",
        "EventEnvelope",
        &env_unknown,
        vec![json!({
            "kind": "unknown_event_preserved",
            "note": "payload.type is not in the v1 vocabulary; TS must surface it as unknown and keep every field",
        })],
    );

    // 8. history page: pagination cursor + nulls + big seq
    let page: Page<HistoryEntry> = Page {
        items: vec![
            HistoryEntry {
                seq: Seq::new(9001),
                event_id: EventId::new("evt-0001-中文事件"),
                event_type: "assistant_segment_delta".into(),
                summary: Some("中文摘要：最终回答增量".into()),
            },
            HistoryEntry {
                seq: Seq::new(9002),
                event_id: EventId::new("evt-0002"),
                event_type: "run_state_changed".into(),
                summary: None,
            },
        ],
        next_cursor: Some(Cursor::new("cur-opaque-0000000000000000-不透明的游标")),
        snapshot_seq: Seq::new((1u64 << 53) + 11),
    };
    push(&mut out, "history-page.json", "HistoryPage", &page, vec![]);

    // 9. model request: full identity pin, u64 budget strings (空值 + 长 ID)
    let model_request = ModelRequest {
        principal: PrincipalRef {
            kind: PrincipalKind::LocalUser,
            principal_id: "local-user:owner-0001".into(),
        },
        run_id: RunId::new("run-0001"),
        attempt: AttemptId::new("attempt-1"),
        model_call_id: ModelCallId::new(
            "modelcall-超长标识符-0123456789abcdef-0123456789abcdef-0123456789abcdef-中文尾巴",
        ),
        purpose: "chat-turn".into(),
        provider: "anthropic".into(),
        model: "claude-example-1".into(),
        operation: "chat".into(),
        budget: Budget {
            max_tokens: Some(8192),
            max_cost_micros: Some((1u64 << 53) + 12345),
            deadline_unix_ms: None,
        },
        config_generation: 7,
    };
    push(
        &mut out,
        "model-request.json",
        "ModelRequest",
        &model_request,
        vec![],
    );

    // 10. approval request: binds target/principal/run/args-digest/resources/generation/expiry
    let approval = ApprovalRequest {
        approval_id: "appr-0001".into(),
        target: "exec_command".into(),
        principal: PrincipalRef {
            kind: PrincipalKind::LocalUser,
            principal_id: "local-user:owner-0001".into(),
        },
        run_id: RunId::new("run-0001"),
        attempt: AttemptId::new("attempt-1"),
        args_digest: digest_arguments(
            &json!({"command": "rm -rf /tmp/示例", "reason": "清理临时目录"}),
        ),
        resources: vec![],
        generation: 7,
        expires_at_unix_ms: 1_800_000_000_000,
        remaining_uses: Some(1),
    };
    push(
        &mut out,
        "approval-request.json",
        "ApprovalRequest",
        &approval,
        vec![],
    );

    // 11. third-party tool schema preserved verbatim at the boundary
    let tool_schema = ToolSchemaDocument {
        dialect: "json-schema/draft-07".into(),
        schema: json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "title": "第三方工具参数（原样保留，含非标准扩展）",
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "搜索关键词"},
                "limit": {"type": "integer", "default": 10},
            },
            "required": ["query"],
            "x-vendor-extension": {"honorific": "供應商自訂欄位", "tags": ["甲", "乙"]},
        }),
    };
    push(
        &mut out,
        "tool-schema-document.json",
        "ToolSchemaDocument",
        &tool_schema,
        vec![json!({
            "kind": "schema_verbatim",
            "note": "schema must round-trip byte-identically, x-vendor-extension included",
        })],
    );

    // 后台通知逐状态保存；领取和真实显示回执各自拥有独立的跨语言样本。
    let display = AutomationDisplayReceipt {
        delivery_id: "delivery-automation-1".into(),
        channel: "desktop".into(),
        displayed: true,
    };
    let mut started_receipt = None;
    for (file, status) in [
        (
            "automation-notification-queued.json",
            AutomationNotificationStatus::Queued,
        ),
        (
            "automation-notification-started.json",
            AutomationNotificationStatus::Started,
        ),
        (
            "automation-notification-confirmed.json",
            AutomationNotificationStatus::Confirmed,
        ),
        (
            "automation-notification-unknown.json",
            AutomationNotificationStatus::Unknown,
        ),
        (
            "automation-notification-failed.json",
            AutomationNotificationStatus::Failed,
        ),
    ] {
        let receipt = AutomationNotificationReceipt {
            delivery_id:"delivery-automation-1".into(),trigger_id:Some("trigger-automation-1".into()),run_id:Some("run-automation-1".into()),
            channel:"desktop".into(),recipient:"user_local".into(),
            payload_json:json!({"title":"合成提醒","body":"任务完成","sessionId":"activity-1","runId":"run-automation-1"}).to_string(),
            status,platform_receipt:if status==AutomationNotificationStatus::Confirmed {Some(serde_json::to_value(&display).unwrap())}else{None},
            error:match status {AutomationNotificationStatus::Unknown=>Some("显示结果未确认，禁止重发".into()),AutomationNotificationStatus::Failed=>Some("桌面系统未确认显示".into()),_=>None},
            created_at:1_800_000_000_000,updated_at:1_800_000_000_001,
        };
        if status == AutomationNotificationStatus::Started {
            started_receipt = Some(receipt.clone());
        }
        push(
            &mut out,
            file,
            "AutomationNotificationReceipt",
            &receipt,
            vec![],
        );
    }
    push(
        &mut out,
        "bridge-connection-status.json",
        "BridgeConnectionStatus",
        &BridgeConnectionStatus::Disconnected,
        vec![],
    );
    for (file, status) in [
        (
            "bridge-worker-connecting.json",
            BridgeConnectionStatus::Connecting,
        ),
        (
            "bridge-worker-connected.json",
            BridgeConnectionStatus::Connected,
        ),
        (
            "bridge-worker-disconnected.json",
            BridgeConnectionStatus::Disconnected,
        ),
    ] {
        push(
            &mut out,
            file,
            "BridgeWorkerEvent",
            &BridgeWorkerEvent::ConnectionStatus {
                connector_id: "connector-1".into(),
                generation: 7,
                status,
                error: (status == BridgeConnectionStatus::Disconnected)
                    .then(|| "platform_connection_unavailable".into()),
            },
            vec![],
        );
    }
    push(
        &mut out,
        "automation-notification-status.json",
        "AutomationNotificationStatus",
        &AutomationNotificationStatus::Started,
        vec![],
    );
    push(
        &mut out,
        "automation-notification-claim.json",
        "AutomationNotificationClaim",
        &AutomationNotificationClaim {
            reserved: true,
            receipt: started_receipt.unwrap(),
        },
        vec![],
    );
    push(
        &mut out,
        "automation-display-receipt.json",
        "AutomationDisplayReceipt",
        &display,
        vec![],
    );
    for (file, command) in [
        (
            "automation-command-claim.json",
            AutomationNotificationCommand::Claim {
                delivery_id: display.delivery_id.clone(),
            },
        ),
        (
            "automation-command-ack.json",
            AutomationNotificationCommand::Ack {
                delivery_id: display.delivery_id.clone(),
                platform_receipt: display.clone(),
            },
        ),
        (
            "automation-command-failed.json",
            AutomationNotificationCommand::Failed {
                delivery_id: display.delivery_id.clone(),
                error: "桌面系统未确认显示".into(),
            },
        ),
        (
            "automation-command-unknown.json",
            AutomationNotificationCommand::Unknown {
                delivery_id: display.delivery_id.clone(),
                error: "展示结果未确认，禁止重发".into(),
            },
        ),
    ] {
        command.validate().expect("生成的通知命令必须满足回执约束");
        push(
            &mut out,
            file,
            "AutomationNotificationCommand",
            &command,
            vec![],
        );
    }

    // 五个平台的回发上下文、附件事实和三种真实回执都经过同一闭合类型。
    let context: BridgeReplyContext = serde_json::from_value(json!({"messageId":"platform-msg","messageThreadId":42,"contextToken":"微信上下文","targetChatId":"actual-user","isGroup":false,"targetType":"user","guildId":"guild","channelId":"channel","rootId":"root","parentId":"parent","text":"原消息","senderId":"sender"})).unwrap();
    let attachment = BridgeAttachment {
        attachment_id: "bytes-1".into(),
        kind: BridgeAttachmentKind::File,
        platform_ref: Some("platform-file".into()),
        filename: Some("资料.txt".into()),
        mime: Some("text/plain".into()),
        size_bytes: Some(12),
    };
    push(
        &mut out,
        "bridge-role.json",
        "BridgeRole",
        &BridgeRole::Owner,
        vec![],
    );
    push(
        &mut out,
        "bridge-conversation.json",
        "BridgeConversationKind",
        &BridgeConversationKind::Dm,
        vec![],
    );
    push(
        &mut out,
        "bridge-attachment-kind.json",
        "BridgeAttachmentKind",
        &BridgeAttachmentKind::File,
        vec![],
    );
    push(
        &mut out,
        "bridge-target-type.json",
        "BridgeTargetType",
        &BridgeTargetType::User,
        vec![],
    );
    push(
        &mut out,
        "bridge-attachment.json",
        "BridgeAttachment",
        &attachment,
        vec![],
    );
    push(
        &mut out,
        "bridge-context.json",
        "BridgeReplyContext",
        &context,
        vec![],
    );
    push(
        &mut out,
        "bridge-command-send.json",
        "BridgeWorkerCommand",
        &BridgeWorkerCommand::Send {
            connector_id: "connector-1".into(),
            generation: 7,
            dispatch_seq: Some(1),
            delivery_id: "delivery-1".into(),
            conversation_id: "dm-1".into(),
            text: "真实回复".into(),
            reply_context: Some(context.clone()),
            attachments: vec!["bytes-1".into()],
        },
        vec![],
    );
    push(
        &mut out,
        "bridge-message.json",
        "BridgeWorkerEvent",
        &BridgeWorkerEvent::Message {
            connector_id: "connector-1".into(),
            generation: 7,
            message_id: "platform-msg".into(),
            conversation_id: "dm-1".into(),
            conversation_kind: BridgeConversationKind::Dm,
            sender_id: "sender".into(),
            display_name: Some("平台公开昵称".into()),
            avatar_url: Some("https://example.invalid/public-avatar.png".into()),
            text: "请读取附件".into(),
            attachments: vec![attachment],
            reply_context: Some(Box::new(context)),
        },
        vec![],
    );
    for (file, status) in [
        (
            "bridge-receipt-confirmed.json",
            BridgeReceiptStatus::Confirmed,
        ),
        ("bridge-receipt-failed.json", BridgeReceiptStatus::Failed),
        ("bridge-receipt-unknown.json", BridgeReceiptStatus::Unknown),
    ] {
        push(
            &mut out,
            file,
            "BridgeWorkerEvent",
            &BridgeWorkerEvent::Receipt {
                connector_id: "connector-1".into(),
                generation: 7,
                delivery_id: "delivery-1".into(),
                status,
                platform_message_ids: if status == BridgeReceiptStatus::Confirmed {
                    vec!["actual-platform-id".into()]
                } else {
                    vec![]
                },
                error: if status == BridgeReceiptStatus::Confirmed {
                    None
                } else {
                    Some("未取得真实确认".into())
                },
            },
            vec![],
        );
    }
    push(
        &mut out,
        "bridge-receipt-status.json",
        "BridgeReceiptStatus",
        &BridgeReceiptStatus::Unknown,
        vec![],
    );

    let takeover = BridgeTakeoverScope {
        actor_subject: bridge_subject("telegram", "connector-1", "sender").unwrap(),
        session_id: "desktop-selected-1".into(),
    };
    push(
        &mut out,
        "bridge-takeover-scope.json",
        "BridgeTakeoverScope",
        &takeover,
        vec![],
    );
    push(
        &mut out,
        "bridge-diagnostic-command.json",
        "BridgeDiagnosticCommand",
        &BridgeDiagnosticCommand::Verify {
            platform: "telegram".into(),
            config: json!({"botToken":"synthetic-only"}),
        },
        vec![],
    );
    push(
        &mut out,
        "bridge-qrcode-status.json",
        "BridgeQrcodeStatus",
        &BridgeQrcodeStatus::Waiting,
        vec![],
    );
    let info = BridgeDiagnosticInfo {
        username: Some("synthetic_bot".into()),
        name: Some("测试机器人".into()),
        msg: None,
        credential_ok: Some(true),
        auth_mode: Some("token".into()),
    };
    push(
        &mut out,
        "bridge-diagnostic-info.json",
        "BridgeDiagnosticInfo",
        &info,
        vec![],
    );
    push(
        &mut out,
        "bridge-diagnostic-result.json",
        "BridgeDiagnosticResult",
        &BridgeDiagnosticResult::DiagnosticResult {
            ok: true,
            info: Some(info),
            qrcode_url: None,
            qrcode_id: None,
            status: None,
            bot_token: None,
            bot_id: None,
            user_id: None,
            base_url: None,
            error: None,
        },
        vec![],
    );

    let receipt_cap = BridgeWaitingReceiptCapability {
        mode: BridgeWaitingReceiptMode::NativeTyping,
        scopes: vec![BridgeConversationKind::Dm, BridgeConversationKind::Group],
        refresh_interval_ms: 4000,
        cancellable: false,
    };
    let stream_cap = BridgeStreamCapability {
        mode: BridgeStreamMode::RichDraft,
        scopes: vec![BridgeConversationKind::Dm],
        min_interval_ms: 500,
        max_chars: 32768,
        rich: true,
        receipt_folded: false,
    };
    let capabilities = BridgeEffectCapabilities {
        receipt: receipt_cap.clone(),
        streams: vec![stream_cap.clone()],
    };
    let effect_context: BridgeReplyContext =
        serde_json::from_value(json!({"isGroup":false,"messageId":"actual-inbound"})).unwrap();
    let effect = BridgeEffectRequest {
        effect_id: "effect-1".into(),
        stream_id: "stream-1".into(),
        inbox_id: "actual-inbox".into(),
        run_id: "actual-run".into(),
        sequence: 1,
        operation: BridgeEffectOperation::StreamUpdate,
        delivery_id: None,
        conversation_id: "actual-dm".into(),
        reply_context: effect_context,
        text: "未经最终确认的预览".into(),
        rich: true,
    };
    push(
        &mut out,
        "bridge-effect-operation.json",
        "BridgeEffectOperation",
        &BridgeEffectOperation::StreamUpdate,
        vec![],
    );
    push(
        &mut out,
        "bridge-effect-status.json",
        "BridgeEffectStatus",
        &BridgeEffectStatus::Unknown,
        vec![],
    );
    push(
        &mut out,
        "bridge-waiting-receipt-mode.json",
        "BridgeWaitingReceiptMode",
        &BridgeWaitingReceiptMode::NativeTyping,
        vec![],
    );
    push(
        &mut out,
        "bridge-stream-mode.json",
        "BridgeStreamMode",
        &BridgeStreamMode::RichDraft,
        vec![],
    );
    push(
        &mut out,
        "bridge-waiting-receipt-capability.json",
        "BridgeWaitingReceiptCapability",
        &receipt_cap,
        vec![],
    );
    push(
        &mut out,
        "bridge-stream-capability.json",
        "BridgeStreamCapability",
        &stream_cap,
        vec![],
    );
    push(
        &mut out,
        "bridge-effect-capabilities.json",
        "BridgeEffectCapabilities",
        &capabilities,
        vec![],
    );
    push(
        &mut out,
        "bridge-worker-ready-capabilities.json",
        "BridgeWorkerEvent",
        &BridgeWorkerEvent::Ready {
            connector_id: "connector-1".into(),
            generation: 7,
            capabilities: Some(capabilities),
        },
        vec![],
    );
    push(
        &mut out,
        "bridge-effect-request.json",
        "BridgeEffectRequest",
        &effect,
        vec![],
    );
    push(
        &mut out,
        "bridge-worker-effect.json",
        "BridgeWorkerCommand",
        &BridgeWorkerCommand::Effect {
            connector_id: "connector-1".into(),
            generation: 7,
            dispatch_seq: Some(2),
            effect,
        },
        vec![],
    );
    for (file, status, ack, ids, error) in [
        (
            "bridge-effect-confirmed.json",
            BridgeEffectStatus::Confirmed,
            true,
            vec!["actual-native-message".into()],
            None,
        ),
        (
            "bridge-effect-unknown.json",
            BridgeEffectStatus::Unknown,
            false,
            vec![],
            Some("native_outcome_unconfirmed".into()),
        ),
        (
            "bridge-effect-skipped.json",
            BridgeEffectStatus::Skipped,
            false,
            vec![],
            Some("native_cleanup_not_supported".into()),
        ),
    ] {
        let receipt = BridgeEffectReceipt {
            effect_id: "effect-final".into(),
            stream_id: "stream-1".into(),
            sequence: 3,
            operation: BridgeEffectOperation::StreamFinish,
            delivery_id: Some("real-final-outbox".into()),
            status,
            platform_ack: ack,
            platform_message_ids: ids,
            error,
        };
        receipt.validate().unwrap();
        push(&mut out, file, "BridgeEffectReceipt", &receipt, vec![]);
        if status == BridgeEffectStatus::Confirmed {
            push(
                &mut out,
                "bridge-worker-effect-receipt.json",
                "BridgeWorkerEvent",
                &BridgeWorkerEvent::EffectReceipt {
                    connector_id: "connector-1".into(),
                    generation: 7,
                    receipt,
                },
                vec![],
            );
        }
    }

    // R07-T09：聊天客户端只读上下文样本。代次必须是十进制字符串；
    // nullable defaultThinkingLevel 保持 null，不得省略或改写为 "auto"。
    let client_model = ClientModelSummary {
        id: "claude-example-1".into(),
        name: "示例对话模型".into(),
        provider: "anthropic".into(),
        is_current: true,
        reasoning: true,
        xhigh: true,
        thinking_levels: vec!["off".into(), "medium".into(), "high".into()],
        default_thinking_level: None,
        audio: false,
        audio_transport: None,
        audio_transport_supported: false,
        input: vec!["text".into(), "image".into()],
    };
    push(
        &mut out,
        "client-context-query.json",
        "ClientContextQuery",
        &serde_json::from_value::<ClientContextQuery>(
            json!({"agentId":"agent-owned","sessionId":null}),
        )
        .expect("内置协议样本必须有效"),
        vec![],
    );
    push(
        &mut out,
        "client-model-selection.json",
        "ClientModelSelection",
        &ClientModelSelection {
            id: client_model.id.clone(),
            provider: client_model.provider.clone(),
        },
        vec![],
    );
    push(
        &mut out,
        "client-model-state.json",
        "ClientModelState",
        &ClientModelState::Ready,
        vec![],
    );
    push(
        &mut out,
        "client-model-summary.json",
        "ClientModelSummary",
        &client_model,
        vec![json!({
            "kind": "nullable_default_thinking_level",
            "field": "defaultThinkingLevel",
            "note": "null must round-trip as null; consumers normalize it, never fabricate a level",
        })],
    );
    push(
        &mut out,
        "client-model-context.json",
        "ClientModelContext",
        &ClientModelContext {
            models: vec![client_model.clone()],
            active_model: Some(ClientModelSelection {
                id: client_model.id.clone(),
                provider: client_model.provider.clone(),
            }),
            model_state: ClientModelState::Ready,
            // 超过 2^53-1 的真实代次：TS 不得用 Number 承载。
            model_generation: "9007199254740993".into(),
            thinking_level: "medium".into(),
        },
        vec![json!({
            "kind": "decimal_string_generation",
            "field": "modelGeneration",
            "note": "generation exceeds 2^53-1; TS must keep it a string, never a Number",
        })],
    );
    push(
        &mut out,
        "client-agent-summary.json",
        "ClientAgentSummary",
        &ClientAgentSummary {
            id: "agent-owned".into(),
            name: "灵犀助手".into(),
            yuan: "lingxi".into(),
            is_primary: true,
            is_current: true,
            has_avatar: true,
            home_folder: None,
            effective_home_folder: None,
            memory_master_enabled: true,
        },
        vec![],
    );
    push(
        &mut out,
        "client-avatar-availability.json",
        "ClientAvatarAvailability",
        &ClientAvatarAvailability {
            agent: true,
            user: false,
        },
        vec![],
    );
    push(
        &mut out,
        "mobile-bootstrap-response.json",
        "MobileBootstrapResponse",
        &MobileBootstrapResponse {
            locale: "zh-CN".into(),
            agent_name: "灵犀助手".into(),
            user_name: "本机用户".into(),
            current_agent_id: Some("agent-owned".into()),
            agent_yuan: "lingxi".into(),
            home_folder: None,
            cwd_history: vec![],
            memory_master_enabled: true,
            memory_enabled: true,
            thinking_level: "medium".into(),
            permission_mode: "ask".into(),
            appearance: json!({"theme":"dark"}),
            editor: json!({"fontSize":14}),
            chat: json!({"layout":"bubbles"}),
            avatars: ClientAvatarAvailability {
                agent: true,
                user: false,
            },
            agents: vec![ClientAgentSummary {
                id: "agent-owned".into(),
                name: "灵犀助手".into(),
                yuan: "lingxi".into(),
                is_primary: true,
                is_current: true,
                has_avatar: true,
                home_folder: None,
                effective_home_folder: None,
                memory_master_enabled: true,
            }],
            models: vec![client_model.clone()],
            active_model: Some(ClientModelSelection {
                id: client_model.id.clone(),
                provider: client_model.provider.clone(),
            }),
            current_model: Some(ClientModelSelection {
                id: client_model.id.clone(),
                provider: client_model.provider.clone(),
            }),
            model_state: ClientModelState::Ready,
            model_generation: "9007199254740993".into(),
        },
        vec![],
    );

    out
}

// ── Output assembly ────────────────────────────────────────────────────────

fn pretty(value: &Value) -> Vec<u8> {
    let mut s = serde_json::to_string_pretty(value).expect("schema serialization");
    s.push('\n');
    s.into_bytes()
}

/// Builds the full generated tree in memory: relpath -> bytes.
fn build_tree() -> BTreeMap<String, Vec<u8>> {
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    let registry = schema_registry();
    let mut type_names = Vec::new();
    let mut emitter = TsEmitter::new();
    for (name, schema) in &registry {
        type_names.push((*name).to_string());
        emitter.absorb_root(schema);
        files.insert(format!("jsonschema/{name}.schema.json"), pretty(schema));
    }
    files.insert(
        "jsonschema/index.json".to_string(),
        pretty(&json!({
            "dialect": "https://json-schema.org/draft/2020-12/schema",
            "authority": "rust/crates/lingxi-protocol (serde types)",
            "types": type_names,
        })),
    );

    files.insert(
        "ts/lingxi-protocol.ts".to_string(),
        emitter.render(&registry).into_bytes(),
    );

    let samples = golden_samples();
    let mut index_entries = Vec::new();
    for sample in &samples {
        files.insert(format!("golden/{}", sample.file), sample.bytes.clone());
        index_entries.push(json!({
            "file": sample.file,
            "type": sample.type_name,
            "sha256": sha256_hex(&sample.bytes),
            "checks": sample.checks,
        }));
    }
    files.insert(
        "golden/index.json".to_string(),
        pretty(&json!({
            "authority": "rust/crates/lingxi-protocol (golden_samples in lingxi-protocol-gen)",
            "canonicalization": CANONICALIZATION_ID,
            "samples": index_entries,
        })),
    );

    // MANIFEST over every file but itself.
    let manifest: Map<String, Value> = files
        .iter()
        .map(|(path, bytes)| (path.clone(), sha256_hex(bytes).into()))
        .collect();
    files.insert(
        "MANIFEST.json".to_string(),
        pretty(&json!({
            "generatedBy": "lingxi-protocol-gen (rust/crates/lingxi-protocol)",
            "sha256": Value::Object(manifest),
        })),
    );

    files
}

fn write_tree(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> std::io::Result<()> {
    for (rel, bytes) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
    }
    Ok(())
}

fn check_tree(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    let mut problems = Vec::new();
    // stale files on disk that regeneration no longer produces
    for sub in ["jsonschema", "ts", "golden"] {
        let dir = root.join(sub);
        if dir.is_dir() {
            for entry in std::fs::read_dir(&dir).expect("read generated dir") {
                let entry = entry.expect("dir entry");
                let name = entry.file_name().to_string_lossy().into_owned();
                let rel = format!("{sub}/{name}");
                if !files.contains_key(&rel) {
                    problems.push(format!("stale file on disk: {rel}"));
                }
            }
        }
    }
    for (rel, bytes) in files {
        let path = root.join(rel);
        match std::fs::read(&path) {
            Ok(disk) if disk == *bytes => {}
            Ok(disk) => problems.push(format!(
                "drift: {rel} (disk {} bytes sha256={}, regenerated {} bytes sha256={})",
                disk.len(),
                sha256_hex(&disk),
                bytes.len(),
                sha256_hex(bytes)
            )),
            Err(_) => problems.push(format!("missing: {rel}")),
        }
    }
    problems
}

// R06-T06: force-links the default test guard into this bin's TEST HARNESS
// build only (cfg(test)); normal bin and release builds never see it.
#[cfg(test)]
#[allow(unused_imports)]
use lingxi_test_guard as _;

fn main() -> ExitCode {
    // RR-T08-F1 hardening: bind the repo root from the runtime working
    // directory and REFUSE to run against a checkout other than the one
    // this binary was compiled in (a shared/leaked CARGO_TARGET_DIR can
    // otherwise execute a foreign checkout's binary and validate that
    // checkout's tree — observed wrong-tree green, 2026-09-25).
    let repo_root = match lingxi_protocol::devgate::bound_repo_root() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    let args: Vec<String> = std::env::args().collect();
    let check = args.iter().any(|a| a == "--check");
    let out = args
        .windows(2)
        .find(|w| w[0] == "--out")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| repo_root.join("contracts/generated"));

    let files = build_tree();
    if check {
        let problems = check_tree(&out, &files);
        if problems.is_empty() {
            println!(
                "OK: {} generated files match regeneration (no diff) under {}",
                files.len(),
                out.display()
            );
            ExitCode::SUCCESS
        } else {
            eprintln!("GENERATED DRIFT DETECTED ({} problems):", problems.len());
            for p in &problems {
                eprintln!("  - {p}");
            }
            eprintln!("regenerate with: cargo run -p lingxi-protocol --bin lingxi-protocol-gen");
            ExitCode::FAILURE
        }
    } else {
        match write_tree(&out, &files) {
            Ok(()) => {
                println!(
                    "wrote {} generated files under {}",
                    files.len(),
                    out.display()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("failed to write generated tree: {e}");
                ExitCode::FAILURE
            }
        }
    }
}
