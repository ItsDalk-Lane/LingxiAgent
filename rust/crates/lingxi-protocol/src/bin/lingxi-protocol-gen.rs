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
        if schema.as_bool() == Some(true) || schema.as_object().map_or(false, |o| o.is_empty()) {
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
            out.push_str(&format!("export type {name} = {};\n\n", self.ts_for(schema)));
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
        && !name.chars().next().map_or(false, |c| c.is_ascii_digit())
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
    let server_hello = accept_hello(&hello, 1, "lingxi-proto-server", "0.0.0-r01t02", 1, &["events.v1"]);
    push(&mut out, "server-hello.json", "ServerHello", &server_hello, vec![]);

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
    push(&mut out, "event-run-state-failed.json", "EventEnvelope", &env_failed, vec![]);

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
    push(&mut out, "model-request.json", "ModelRequest", &model_request, vec![]);

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
        args_digest: digest_arguments(&json!({"command": "rm -rf /tmp/示例", "reason": "清理临时目录"})),
        resources: vec![],
        generation: 7,
        expires_at_unix_ms: 1_800_000_000_000,
        remaining_uses: Some(1),
    };
    push(&mut out, "approval-request.json", "ApprovalRequest", &approval, vec![]);

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

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("crate is at rust/crates/lingxi-protocol")
        .to_path_buf()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let check = args.iter().any(|a| a == "--check");
    let out = args
        .windows(2)
        .find(|w| w[0] == "--out")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| repo_root().join("contracts/generated"));

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
