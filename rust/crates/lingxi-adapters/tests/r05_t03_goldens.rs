//! R05-T03 golden suite: the offline protocol substitutes for the five
//! chat wire families (C01-C12 evidence at the adapter boundary).
//!
//! Every golden under `tests/golden/<family>/{forward,tool_roundtrip,error}.json`
//! is a self-contained contract pin:
//! - `forward` / `tool_roundtrip` cases build the [`ModelTurnInput`] from
//!   the golden's `input_spec`, render the provider request with the
//!   family's PURE renderer and require semantic equality with
//!   `expected_request` (a hand-authored wire document — never a snapshot
//!   of the implementation's own output), then parse the golden's
//!   `response_wire` (buffered JSON, or an SSE frame list under
//!   `wire_mode: "sse"`) and require the projected turn + usage to equal
//!   `expected_turn`.
//! - `error` cases dispatch the REAL adapter over loopback HTTP against a
//!   raw-TCP stub (the far end of the wire, never an in-process double) and
//!   pin the failure classification (C07), the material scrubbing (C09) and
//!   the transport facts (path + auth header mapping per family).
//!
//! The goldens encode the protocol-correct wire shape (the incumbent
//! `core/llm-client.ts` baselines plus each family's published request
//! vocabulary); a change that alters any rendered byte or parsed projection
//! breaks this suite loudly — that is the point.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lingxi_adapters::models::{
    anthropic_messages,
    credentials::ApplicableAuth,
    google_generative_ai, openai_codex_responses, openai_completions, openai_responses,
    streaming::{NullDeltaSink, SseDecoder, SseEvent},
    ParsedChat,
};
use lingxi_kernel::model_exchange::{
    CredentialAuthKind, CredentialReference, ExchangeItem, ModelOperation, ModelTurnInput,
    ProtocolFamily, RequestedToolCall, ResolvedModelRoute, ToolDeclaration,
    ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{ProviderTurn, ToolOutcome, ToolRequest, ToolRunStatus, ToolSuccess};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{
    AttemptId, ContentBlock, ErrorCode, ModelCallId, ProtocolError, ResourceId, ResourceKind,
    ResourceRef, RunId, SessionId, ToolCallId, ToolSchemaDocument,
};
use serde::Deserialize;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── the golden document shape ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Golden {
    family: String,
    case: String,
    route: RouteSpec,
    input: InputSpec,
    /// Pinned outbound body (forward / tool_roundtrip; error cases compare
    /// the stub-recorded body against a fresh render instead).
    #[serde(default)]
    expected_request: Option<serde_json::Value>,
    /// "buffered" (default) or "sse".
    #[serde(default)]
    wire_mode: Option<String>,
    #[serde(default)]
    response_wire: Option<ResponseWire>,
    #[serde(default)]
    expected_turn: Option<serde_json::Value>,
    /// Error cases: the credential the adapter dispatches with.
    #[serde(default)]
    auth: Option<AuthSpec>,
    #[serde(default)]
    http_error: Option<HttpErrorSpec>,
    #[serde(default)]
    expected_transport: Option<TransportSpec>,
    #[serde(default)]
    expected_failure: Option<FailureSpec>,
}

#[derive(Debug, Deserialize)]
struct RouteSpec {
    provider: String,
    model: String,
}

#[derive(Debug, Deserialize)]
struct InputSpec {
    submission: String,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    tools: Vec<ToolSpec>,
    #[serde(default)]
    prior: Vec<PriorItem>,
}

#[derive(Debug, Deserialize)]
struct ToolSpec {
    target: String,
    wire_name: String,
    description: String,
    schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
enum PriorItem {
    #[serde(rename = "assistant")]
    Assistant(AssistantSpec),
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultSpec),
}

#[derive(Debug, Deserialize)]
struct AssistantSpec {
    call: String,
    #[serde(default)]
    content: Vec<serde_json::Value>,
    #[serde(default)]
    tool_calls: Vec<AssistantCallSpec>,
}

#[derive(Debug, Deserialize)]
struct AssistantCallSpec {
    seq: u32,
    provider_call_id: String,
    target: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ToolResultSpec {
    seq: u32,
    provider_call_id: String,
    outcome: OutcomeSpec,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OutcomeSpec {
    SuccessText {
        text: String,
    },
    SuccessFull {
        text: String,
        #[serde(default)]
        resource_refs: Vec<ResourceRefSpec>,
        #[serde(default)]
        truncated: bool,
        #[serde(default)]
        status: Option<StatusSpec>,
    },
    Failed {
        code: String,
        message: String,
    },
    Cancelled,
    Unknown {
        reason: String,
    },
}

#[derive(Debug, Deserialize)]
struct ResourceRefSpec {
    id: String,
    kind: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Debug, Deserialize)]
enum StatusSpec {
    #[serde(rename = "exited")]
    Exited(i64),
    #[serde(rename = "running")]
    Running(String),
    #[serde(rename = "stop_unconfirmed")]
    StopUnconfirmed { handle: String, detail: String },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ResponseWire {
    Sse { sse_frames: Vec<String> },
    Buffered(serde_json::Value),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AuthSpec {
    Bearer { token: String },
    BearerJwt { chatgpt_account_id: String },
    Header { name: String, value: String },
    None,
}

#[derive(Debug, Deserialize)]
struct HttpErrorSpec {
    status: u16,
    body: String,
}

#[derive(Debug, Deserialize)]
struct TransportSpec {
    path: String,
    #[serde(default)]
    headers: Vec<(String, String)>,
    /// Headers that must NOT appear on the wire (the auth-mapping honesty
    /// half: anthropic never sends `authorization`, google neither, ...).
    #[serde(default)]
    absent_headers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FailureSpec {
    code: String,
    retryable: bool,
    message: String,
}

// ── golden discovery ─────────────────────────────────────────────────────────

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn load_goldens() -> Vec<(PathBuf, Golden)> {
    let mut out = Vec::new();
    let root = golden_dir();
    let mut families: Vec<_> = std::fs::read_dir(&root)
        .expect("golden dir exists")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.is_dir())
        .collect();
    families.sort();
    for family_dir in families {
        let mut files: Vec<_> = std::fs::read_dir(&family_dir)
            .expect("family dir readable")
            .map(|entry| entry.expect("dir entry").path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
            .collect();
        files.sort();
        for file in files {
            let text = std::fs::read_to_string(&file).expect("golden readable");
            let golden: Golden = serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("{}: {err}", file.display()));
            assert_eq!(
                golden.family,
                family_dir
                    .file_name()
                    .expect("family name")
                    .to_str()
                    .expect("utf8"),
                "{}: family field must equal the directory name",
                file.display()
            );
            assert_eq!(
                golden.case,
                file.file_stem().expect("stem").to_str().expect("utf8"),
                "{}: case field must equal the file stem",
                file.display()
            );
            out.push((file, golden));
        }
    }
    out
}

#[test]
fn the_golden_set_covers_five_families_times_three_cases() {
    let goldens = load_goldens();
    assert_eq!(goldens.len(), 15, "5 families x 3 cases");
    for family in [
        "openai-completions",
        "anthropic-messages",
        "google-generative-ai",
        "openai-responses",
        "openai-codex-responses",
    ] {
        for case in ["forward", "tool_roundtrip", "error"] {
            assert!(
                goldens
                    .iter()
                    .any(|(_, golden)| golden.family == family && golden.case == case),
                "missing golden {family}/{case}.json"
            );
        }
    }
}

// ── input_spec → ModelTurnInput ─────────────────────────────────────────────

fn content_block_of(value: &serde_json::Value) -> ContentBlock {
    if let Some(text) = value.get("text").and_then(|t| t.as_str()) {
        return ContentBlock::Text {
            text: text.to_string(),
        };
    }
    if let Some(text) = value.get("reasoning").and_then(|t| t.as_str()) {
        return ContentBlock::Reasoning {
            text: text.to_string(),
        };
    }
    if let Some(opaque) = value.get("opaque") {
        return ContentBlock::Opaque {
            provider: opaque
                .get("provider")
                .and_then(|p| p.as_str())
                .expect("opaque.provider")
                .to_string(),
            data: opaque.get("data").expect("opaque.data").clone(),
        };
    }
    panic!("unknown content block in golden: {value}");
}

fn error_code_of(wire_name: &str) -> ErrorCode {
    match wire_name {
        "invalid_message" => ErrorCode::InvalidMessage,
        "version_incompatible" => ErrorCode::VersionIncompatible,
        "unauthorized" => ErrorCode::Unauthorized,
        "forbidden" => ErrorCode::Forbidden,
        "not_found" => ErrorCode::NotFound,
        "conflict" => ErrorCode::Conflict,
        "cancelled" => ErrorCode::Cancelled,
        "budget_exceeded" => ErrorCode::BudgetExceeded,
        "upstream_unavailable" => ErrorCode::UpstreamUnavailable,
        "cursor_expired" => ErrorCode::CursorExpired,
        "unknown_schema_dialect" => ErrorCode::UnknownSchemaDialect,
        "internal" => ErrorCode::Internal,
        other => panic!("unknown error code in golden: {other}"),
    }
}

fn build_outcome(spec: &OutcomeSpec) -> ToolOutcome {
    match spec {
        OutcomeSpec::SuccessText { text } => ToolOutcome::success_text(text.clone()),
        OutcomeSpec::SuccessFull {
            text,
            resource_refs,
            truncated,
            status,
        } => {
            let mut success =
                ToolSuccess::from_content(vec![ContentBlock::Text { text: text.clone() }]);
            success.resource_refs = resource_refs
                .iter()
                .map(|reference| ResourceRef {
                    resource_id: ResourceId::new(reference.id.clone()),
                    kind: match reference.kind.as_str() {
                        "session_file" => ResourceKind::SessionFile,
                        "attachment" => ResourceKind::Attachment,
                        "artifact" => ResourceKind::Artifact,
                        "export" => ResourceKind::Export,
                        other => panic!("unknown resource kind in golden: {other}"),
                    },
                    display_name: reference.name.clone(),
                    uri: reference.uri.clone(),
                    digest: None,
                    size_bytes: None,
                })
                .collect();
            success.truncated = *truncated;
            success.status = status.as_ref().map(|status| {
                Box::new(match status {
                    StatusSpec::Exited(code) => ToolRunStatus::Exited { code: *code },
                    StatusSpec::Running(handle) => ToolRunStatus::Running {
                        handle: handle.clone(),
                    },
                    StatusSpec::StopUnconfirmed { handle, detail } => {
                        ToolRunStatus::StopUnconfirmed {
                            handle: handle.clone(),
                            detail: detail.clone(),
                        }
                    }
                })
            });
            ToolOutcome::Success { result: success }
        }
        OutcomeSpec::Failed { code, message } => ToolOutcome::Failed {
            error: ProtocolError::new(error_code_of(code), message.clone(), false),
        },
        OutcomeSpec::Cancelled => ToolOutcome::Cancelled,
        OutcomeSpec::Unknown { reason } => ToolOutcome::Unknown {
            reason: reason.clone(),
        },
    }
}

fn host_call_id(seq: u32) -> ToolCallId {
    ToolCallId::new(format!("golden-tc{seq:04}"))
}

fn build_input(spec: &InputSpec) -> ModelTurnInput {
    let budget = SchemaBudget::default();
    let tools = ToolDeclarationSnapshot {
        catalog_generation: 7,
        declarations: spec
            .tools
            .iter()
            .map(|tool| ToolDeclaration {
                target: tool.target.clone(),
                wire_name: tool.wire_name.clone(),
                description: tool.description.clone(),
                input_schema: ToolSchemaDocument {
                    dialect: "json-schema/2020-12".to_string(),
                    schema: tool.schema.clone(),
                },
            })
            .collect(),
    };
    let mut input = ModelTurnInput {
        submission: spec.submission.clone(),
        system_prompt: spec.system_prompt.clone(),
        turn: 1,
        prior: Vec::new(),
        tools,
        deadline_unix_ms: None,
        images: Vec::new(),
        max_output_tokens: None,
    };
    for item in &spec.prior {
        match item {
            PriorItem::Assistant(assistant) => {
                let mut tool_calls = Vec::new();
                for call in &assistant.tool_calls {
                    let request = ToolRequest::from_effective_arguments(
                        call.target.clone(),
                        call.arguments.clone(),
                        &budget,
                    )
                    .expect("golden arguments are effective")
                    .with_provider_call_id(call.provider_call_id.clone());
                    tool_calls.push(RequestedToolCall {
                        tool_call_id: host_call_id(call.seq),
                        provider_call_id: request.provider_call_id.clone(),
                        target: request.target.clone(),
                        arguments: request.arguments.clone(),
                        args_digest: request.args_digest.clone(),
                        args_summary: None,
                    });
                }
                input.prior.push(ExchangeItem::AssistantTurn {
                    call: ModelCallId::new(assistant.call.clone()),
                    content: assistant.content.iter().map(content_block_of).collect(),
                    tool_calls,
                });
            }
            PriorItem::ToolResult(result) => {
                input.prior.push(ExchangeItem::ToolResult {
                    tool_call_id: host_call_id(result.seq),
                    provider_call_id: Some(result.provider_call_id.clone()),
                    outcome: build_outcome(&result.outcome),
                });
            }
        }
    }
    input
}

fn build_route(golden: &Golden, endpoint: &str) -> ResolvedModelRoute {
    ResolvedModelRoute {
        provider: golden.route.provider.clone(),
        model: golden.route.model.clone(),
        operation: ModelOperation::Chat,
        protocol: ProtocolFamily::parse(&golden.family).expect("family parses"),
        endpoint: endpoint.to_string(),
        credential: CredentialReference {
            provider: golden.route.provider.clone(),
            auth: match &golden.auth {
                Some(AuthSpec::BearerJwt { .. }) => CredentialAuthKind::OAuth,
                Some(AuthSpec::Header { .. }) => CredentialAuthKind::AuthHeader,
                Some(AuthSpec::None) => CredentialAuthKind::None,
                _ => CredentialAuthKind::ApiKey,
            },
        },
        config_generation: 1,
        group_id: None,
    }
}

// ── family dispatch (render + parse) ─────────────────────────────────────────

fn render_for(
    family: &str,
    input: &ModelTurnInput,
    route: &ResolvedModelRoute,
    sse: bool,
) -> serde_json::Value {
    match family {
        "openai-completions" => {
            openai_completions::render_chat_request(input, route, sse).expect("render")
        }
        "anthropic-messages" => anthropic_messages::render_messages_request(
            input,
            route,
            anthropic_messages::DEFAULT_MAX_OUTPUT_TOKENS,
            sse,
        )
        .expect("render"),
        "google-generative-ai" => {
            google_generative_ai::render_generate_request(input, route).expect("render")
        }
        "openai-responses" => {
            openai_responses::render_responses_request(input, route, sse).expect("render")
        }
        "openai-codex-responses" => {
            openai_codex_responses::render_codex_request(input, route).expect("render")
        }
        other => panic!("unknown family {other}"),
    }
}

fn decode_sse(frames: &[String]) -> Vec<SseEvent> {
    let body = frames
        .iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect::<String>();
    let mut decoder = SseDecoder::new();
    let mut events = decoder.feed(body.as_bytes()).expect("sse feed");
    let finish = decoder.finish().expect("sse finish");
    assert!(
        !finish.discarded_partial,
        "golden SSE frames must end on a frame boundary"
    );
    events.extend(finish.events);
    events
}

fn parse_for(family: &str, wire: &ResponseWire, snapshot: &ToolDeclarationSnapshot) -> ParsedChat {
    let call = ModelCallId::new("golden-mc0001");
    let budget = SchemaBudget::default();
    match (family, wire) {
        ("openai-completions", ResponseWire::Buffered(body)) => {
            openai_completions::parse_chat_response(&call, snapshot, body, &budget)
        }
        ("openai-completions", ResponseWire::Sse { sse_frames }) => {
            openai_completions::parse_chat_stream(&call, snapshot, &decode_sse(sse_frames), &budget)
        }
        ("anthropic-messages", ResponseWire::Buffered(body)) => {
            anthropic_messages::parse_messages_response(&call, snapshot, body, &budget)
        }
        ("google-generative-ai", ResponseWire::Buffered(body)) => {
            google_generative_ai::parse_generate_response(&call, snapshot, body, &budget)
        }
        ("openai-responses", ResponseWire::Buffered(body)) => {
            openai_responses::parse_responses_response(&call, snapshot, body, &budget)
        }
        ("openai-responses", ResponseWire::Sse { sse_frames }) => {
            openai_responses::parse_responses_stream(
                &call,
                snapshot,
                &decode_sse(sse_frames),
                &budget,
            )
        }
        ("openai-codex-responses", ResponseWire::Buffered(body)) => {
            openai_codex_responses::parse_codex_response(&call, snapshot, body, &budget)
        }
        ("openai-codex-responses", ResponseWire::Sse { sse_frames }) => {
            openai_codex_responses::parse_codex_stream(
                &call,
                snapshot,
                &decode_sse(sse_frames),
                &budget,
            )
        }
        (family, _) => panic!("family {family} has no parser for this wire mode"),
    }
    .expect("golden response parses")
}

// ── the turn projection (the golden-facing shape of a ParsedChat) ───────────

fn project_content(content: &[ContentBlock]) -> serde_json::Value {
    serde_json::Value::Array(
        content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => serde_json::json!({"text": text}),
                ContentBlock::Reasoning { text } => serde_json::json!({"reasoning": text}),
                ContentBlock::Opaque { provider, data } => {
                    serde_json::json!({"opaque": {"provider": provider, "data": data}})
                }
                ContentBlock::ResourceRef { resource } => {
                    serde_json::json!({"resource": serde_json::to_value(resource).expect("resource json")})
                }
            })
            .collect(),
    )
}

fn project_parsed(parsed: &ParsedChat) -> serde_json::Value {
    let usage = match &parsed.usage() {
        Some(usage) => serde_json::json!({
            "input_tokens": usage.input_tokens,
            "output_tokens": usage.output_tokens,
        }),
        None => serde_json::Value::Null,
    };
    let mut projection = match &parsed.turn {
        ProviderTurn::Final { message } => serde_json::json!({
            "kind": "final",
            "content": project_content(&message.content),
        }),
        ProviderTurn::ToolRequests { requests, content } => serde_json::json!({
            "kind": "tool_requests",
            "content": project_content(content),
            "calls": requests
                .iter()
                .map(|request| serde_json::json!({
                    "target": request.target,
                    "provider_call_id": request.provider_call_id,
                    "arguments": request.arguments.as_value(),
                }))
                .collect::<Vec<_>>(),
        }),
        ProviderTurn::Empty { detail } => serde_json::json!({
            "kind": "empty",
            "detail": detail,
        }),
        ProviderTurn::Continue { process_note } => serde_json::json!({
            "kind": "continue",
            "detail": process_note,
        }),
        ProviderTurn::Failed { error, retryable } => serde_json::json!({
            "kind": "failed",
            "code": error.code.wire_name(),
            "retryable": retryable,
            "message": error.message,
        }),
    };
    projection["usage"] = usage;
    projection
}

// ── the forward / tool_roundtrip runner ─────────────────────────────────────

#[test]
fn forward_and_roundtrip_goldens_render_and_parse_exactly() {
    for (file, golden) in load_goldens() {
        let Some(expected_request) = &golden.expected_request else {
            continue; // error cases run in the async runner below
        };
        let context = || format!("{}", file.display());
        let input = build_input(&golden.input);
        let route = build_route(&golden, "https://golden-endpoint.invalid");
        let sse = golden.wire_mode.as_deref() == Some("sse");
        let rendered = render_for(&golden.family, &input, &route, sse);
        assert_eq!(
            &rendered,
            expected_request,
            "{}: rendered request must equal the golden",
            context()
        );
        let wire = golden
            .response_wire
            .as_ref()
            .unwrap_or_else(|| panic!("{}: response_wire required", context()));
        let parsed = parse_for(&golden.family, wire, &input.tools);
        let projected = project_parsed(&parsed);
        let expected_turn = golden
            .expected_turn
            .as_ref()
            .unwrap_or_else(|| panic!("{}: expected_turn required", context()));
        assert_eq!(
            &projected,
            expected_turn,
            "{}: parsed turn projection must equal the golden",
            context()
        );
    }
}

// ── the error runner (real loopback HTTP, raw-TCP stub) ─────────────────────

struct RecordedRequest {
    method: String,
    path: String,
    /// Header names lowercased (the wire casing is a transport detail).
    headers: Vec<(String, String)>,
    body: serde_json::Value,
}

struct ErrorStub {
    endpoint: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl ErrorStub {
    async fn start(status: u16, body: String) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let task_requests = Arc::clone(&requests);
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                let recorded = read_request(&mut socket).await;
                task_requests.lock().expect("requests").push(recorded);
                let reason = match status {
                    400 => "Bad Request",
                    401 => "Unauthorized",
                    403 => "Forbidden",
                    404 => "Not Found",
                    408 => "Request Timeout",
                    422 => "Unprocessable Entity",
                    429 => "Too Many Requests",
                    500 => "Internal Server Error",
                    502 => "Bad Gateway",
                    503 => "Service Unavailable",
                    other => panic!("stub: unscripted status {other}"),
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        Self {
            endpoint: format!("http://{addr}"),
            requests,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn recorded(&self) -> Vec<RecordedRequest> {
        self.requests
            .lock()
            .expect("requests")
            .drain(..)
            .map(|r| RecordedRequest {
                method: r.method,
                path: r.path,
                headers: r.headers,
                body: r.body,
            })
            .collect()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), &mut self.task).await;
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> RecordedRequest {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), socket.read(&mut chunk))
                .await
                .expect("stub read stalled")
                .expect("stub read");
        if read == 0 {
            panic!("stub: connection closed before headers completed");
        }
        raw.extend_from_slice(&chunk[..read]);
        if let Some(pos) = find_subslice(&raw, b"\r\n\r\n") {
            break pos;
        }
        assert!(raw.len() < 256 * 1024, "stub: header block too large");
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("stub: utf8 headers");
    let mut lines = head.split("\r\n");
    let request_line = lines.next().expect("request line");
    let mut words = request_line.split_whitespace();
    let method = words.next().expect("method").to_string();
    let path = words.next().expect("path").to_string();
    let mut headers = Vec::new();
    let mut content_length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = Some(value.parse::<usize>().expect("numeric content-length"));
            }
            headers.push((name, value));
        }
    }
    let content_length = content_length.expect("json posts carry a content-length");
    let body_start = header_end + 4;
    while raw.len() - body_start < content_length {
        let mut chunk = [0_u8; 4096];
        let read =
            tokio::time::timeout(std::time::Duration::from_secs(10), socket.read(&mut chunk))
                .await
                .expect("stub body read stalled")
                .expect("stub body read");
        if read == 0 {
            panic!("stub: connection closed mid-body");
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    let body: serde_json::Value =
        serde_json::from_slice(&raw[body_start..body_start + content_length])
            .expect("stub: request body must be JSON");
    RecordedRequest {
        method,
        path,
        headers,
        body,
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Builds the golden JWT for codex cases: a three-segment token whose
/// payload carries the account claim (the segments are test material —
/// never a real credential).
fn golden_jwt(account_id: &str) -> String {
    use base64::Engine as _;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&serde_json::json!({
            "https://api.openai.com/auth": {"chatgpt_account_id": account_id}
        }))
        .expect("json"),
    );
    format!("goldenheader.{payload}.goldensig")
}

fn resolve_auth(spec: &AuthSpec) -> (ApplicableAuth, Option<String>) {
    match spec {
        AuthSpec::Bearer { token } => (ApplicableAuth::Bearer(token.clone()), None),
        AuthSpec::BearerJwt { chatgpt_account_id } => {
            let jwt = golden_jwt(chatgpt_account_id);
            (ApplicableAuth::Bearer(jwt.clone()), Some(jwt))
        }
        AuthSpec::Header { name, value } => (
            ApplicableAuth::Header {
                name: name.clone(),
                value: value.clone(),
            },
            None,
        ),
        AuthSpec::None => (ApplicableAuth::None, None),
    }
}

fn golden_ctx() -> RunContext {
    RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sess-golden"),
        run_id: RunId::new("run-golden"),
        attempt: AttemptId::new("run-golden-attempt-1"),
        generation: 1,
    }
}

/// The endpoint shape each family's config would carry against the stub
/// (mirrors the production URL rules: openai families keep their `/v1`
/// base, codex keeps its `/backend-api` base, anthropic/google take the
/// bare origin and join their own paths).
fn stub_endpoint_for(family: &str, base: &str) -> String {
    match family {
        "openai-completions" | "openai-responses" => format!("{base}/v1"),
        "openai-codex-responses" => format!("{base}/backend-api"),
        _ => base.to_string(),
    }
}

async fn execute_error_case(
    golden: &Golden,
    input: &ModelTurnInput,
    route: &ResolvedModelRoute,
    auth: &ApplicableAuth,
) -> lingxi_kernel::ports::ProviderTurnResult {
    let ctx = golden_ctx();
    let call = ModelCallId::new("golden-mc0001");
    let budget = SchemaBudget::default();
    // R05-T05: the golden harness pins the PRE-compat wire byte-exactly (the
    // fictional golden route providers would otherwise trip real compat
    // matchers — e.g. `claude-` ids pull the anthropic patch). The compat
    // layer has its own golden suite (r05_t05_compat.rs).
    let no_compat = None;
    match golden.family.as_str() {
        "openai-completions" => {
            openai_completions::OpenAiCompletionsAdapter::new(budget)
                .expect("adapter")
                .execute_chat(&ctx, &call, input, route, auth, no_compat, &NullDeltaSink)
                .await
        }
        "anthropic-messages" => {
            anthropic_messages::AnthropicMessagesAdapter::new(
                budget,
                anthropic_messages::DEFAULT_MAX_OUTPUT_TOKENS,
            )
            .expect("adapter")
            .execute_chat(&ctx, &call, input, route, auth, no_compat, &NullDeltaSink)
            .await
        }
        "google-generative-ai" => {
            google_generative_ai::GoogleGenerativeAiAdapter::new(budget)
                .expect("adapter")
                .execute_chat(&ctx, &call, input, route, auth, &NullDeltaSink)
                .await
        }
        "openai-responses" => {
            openai_responses::OpenAiResponsesAdapter::new(budget)
                .expect("adapter")
                .execute_chat(&ctx, &call, input, route, auth, no_compat, &NullDeltaSink)
                .await
        }
        "openai-codex-responses" => {
            openai_codex_responses::OpenAiCodexResponsesAdapter::new(budget)
                .expect("adapter")
                .execute_chat(&ctx, &call, input, route, auth, no_compat, &NullDeltaSink)
                .await
        }
        other => panic!("unknown family {other}"),
    }
}

#[tokio::test]
async fn error_goldens_classify_and_transport_exactly() {
    for (file, golden) in load_goldens() {
        let (Some(http_error), Some(expected_failure), Some(expected_transport)) = (
            &golden.http_error,
            &golden.expected_failure,
            &golden.expected_transport,
        ) else {
            continue;
        };
        let context = || format!("{}", file.display());
        let auth_spec = golden
            .auth
            .as_ref()
            .unwrap_or_else(|| panic!("{}: auth required", context()));
        let (auth, jwt) = resolve_auth(auth_spec);
        let resolve = |text: &str| -> String {
            match &jwt {
                Some(jwt) => text.replace("${jwt}", jwt),
                None => text.to_string(),
            }
        };
        let stub = ErrorStub::start(http_error.status, resolve(&http_error.body)).await;
        let input = build_input(&golden.input);
        let route = build_route(&golden, &stub_endpoint_for(&golden.family, &stub.endpoint));
        let result = execute_error_case(&golden, &input, &route, &auth).await;
        match &result.turn {
            ProviderTurn::Failed { error, retryable } => {
                assert_eq!(
                    error.code.wire_name(),
                    expected_failure.code,
                    "{}: failure code",
                    context()
                );
                assert_eq!(
                    *retryable,
                    expected_failure.retryable,
                    "{}: retryable",
                    context()
                );
                assert_eq!(
                    error.message,
                    resolve(&expected_failure.message),
                    "{}: failure message (material scrubbed, C09)",
                    context()
                );
            }
            other => panic!("{}: expected a Failed turn, got {other:?}", context()),
        }
        let recorded = stub.recorded();
        stub.stop().await;
        assert_eq!(recorded.len(), 1, "{}: exactly one request", context());
        let request = &recorded[0];
        assert_eq!(request.method, "POST", "{}: method", context());
        assert_eq!(
            request.path,
            expected_transport.path,
            "{}: request path",
            context()
        );
        for (name, value) in &expected_transport.headers {
            let found = request
                .headers
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone());
            assert_eq!(
                found.as_deref(),
                Some(resolve(value).as_str()),
                "{}: header {name}",
                context()
            );
        }
        for name in &expected_transport.absent_headers {
            assert!(
                !request.headers.iter().any(|(n, _)| n == name),
                "{}: header {name} must NOT be sent",
                context()
            );
        }
        // The body on the wire is exactly the family renderer's output (the
        // execute path renders once and sends verbatim). R05-T04 (D3): the
        // production execute path ALWAYS renders the streaming shape — the
        // golden's own wire_mode describes its RESPONSE wire, not the
        // request.
        let expected_body = render_for(&golden.family, &input, &route, true);
        assert_eq!(
            request.body,
            expected_body,
            "{}: the dispatched body is the rendered request",
            context()
        );
    }
}
