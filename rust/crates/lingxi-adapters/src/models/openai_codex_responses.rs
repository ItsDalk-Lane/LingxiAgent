//! The `openai-codex-responses` protocol adapter (R05-T03): the ChatGPT
//! Codex Responses wire family — the responses vocabulary over the codex
//! transport (the incumbent `core/llm-client.ts` codex branch is the
//! reviewed baseline, replicated 1:1 where the contract is the same).
//!
//! Wire facts (registered in docs/rust-tauri/R05/PROTOCOL_WIRE_MATRIX.json):
//! - URL: the incumbent `resolveCodexResponsesUrl` rule — trailing slashes
//!   stripped; a base already ending `/codex/responses` is used as-is; a
//!   base ending `/codex` gains `/responses`; anything else gains
//!   `/codex/responses`. (The incumbent's default base is
//!   `https://chatgpt.com/backend-api`; the Rust plane config carries the
//!   endpoint EXPLICITLY — a missing endpoint is a config error, never a
//!   defaulted host.)
//! - Headers: `OpenAI-Beta: responses=experimental`, `originator: pi`,
//!   `chatgpt-account-id` extracted from the OAuth access token's JWT
//!   (`https://api.openai.com/auth`.`chatgpt_account_id` claim, base64url
//!   payload), `Authorization: Bearer <token>`. A missing/unparseable
//!   account id is a loud non-retryable auth failure — never a fabricated
//!   account id, never a request without it.
//! - Body: `{model, store:false, stream:true, instructions, input}` — the
//!   family is ALWAYS streamed; `instructions` is the host system prompt
//!   or, when none was resolved, the incumbent's documented default
//!   [`DEFAULT_CODEX_INSTRUCTIONS`] (the adapter-injected instruction is
//!   pinned verbatim).
//! - The token-family fields the incumbent strips
//!   (`max_output_tokens`/`max_completion_tokens`/`max_tokens`/
//!   `maxOutputTokens`/`temperature`) never enter THIS renderer — the body
//!   is built natively, so there is nothing to strip (the incumbent strips
//!   because it reuses a shared request builder; documented in the
//!   matrix).
//! - Response: SSE frames; the terminal `response.completed` /
//!   `response.failed` / `response.incomplete` frame carries the
//!   authoritative aggregate (usage included) and is parsed by the SHARED
//!   responses parser ([`super::openai_responses`]). `[DONE]` terminates.
//! - Opaque state (reasoning items incl. `encrypted_content`) round-trips
//!   tagged to THIS family; an `openai-responses` opaque is never echoed
//!   here (C10).

use base64::Engine as _;
use lingxi_kernel::model_exchange::{ModelTurnInput, ResolvedModelRoute, ToolDeclarationSnapshot};
use lingxi_kernel::ports::{ProviderTurn, ProviderTurnResult};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::RunContext;
use lingxi_protocol::{ErrorCode, ModelCallId, ProtocolError};

use super::credentials::ApplicableAuth;
use super::dispatch;
use super::openai_responses::{
    parse_responses_response_as, parse_responses_stream_as, render_input_items,
    ResponsesStreamAccumulator,
};
use super::ParsedChat;

/// The `provider` tag of this family's opaque blocks.
pub const FAMILY: &str = "openai-codex-responses";

/// The incumbent's adapter-injected instruction when no system prompt was
/// resolved (`DEFAULT_CODEX_UTILITY_INSTRUCTIONS`, verbatim).
pub const DEFAULT_CODEX_INSTRUCTIONS: &str =
    "You are Hana's utility model.\nFollow the user request exactly and return only the requested content.";

/// The JWT claim path carrying the ChatGPT account id.
const CODEX_ACCOUNT_CLAIM_PATH: &str = "https://api.openai.com/auth";

/// The incumbent `resolveCodexResponsesUrl` rule.
pub fn resolve_codex_responses_url(endpoint: &str) -> String {
    let raw = endpoint.trim_end_matches('/');
    if raw.ends_with("/codex/responses") {
        return raw.to_string();
    }
    if let Some(stripped) = raw.strip_suffix("/codex") {
        return format!("{stripped}/codex/responses");
    }
    format!("{raw}/codex/responses")
}

/// Extracts the ChatGPT account id from an OAuth access token's JWT
/// payload (the incumbent `extractAccountIdFromToken`): the
/// `chatgpt_account_id` claim under `https://api.openai.com/auth`,
/// base64url-decoded. `None` when the token is not a JWT carrying the
/// claim — the caller fails loudly, never fabricates.
pub fn extract_account_id_from_token(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    let account = value
        .get(CODEX_ACCOUNT_CLAIM_PATH)?
        .get("chatgpt_account_id")?
        .as_str()?;
    if account.is_empty() {
        None
    } else {
        Some(account.to_string())
    }
}

/// The real HTTP adapter for the openai-codex-responses family. Stateless
/// per call; no redirects (C10); always streamed.
pub struct OpenAiCodexResponsesAdapter {
    client: reqwest::Client,
    schema_budget: SchemaBudget,
    timeouts: dispatch::HttpTimeouts,
}

impl OpenAiCodexResponsesAdapter {
    pub fn new(schema_budget: SchemaBudget) -> Result<Self, ProtocolError> {
        Self::new_with_timeouts(schema_budget, dispatch::HttpTimeouts::default())
    }

    /// Explicit timeout segments (R05-T05; tests inject shorter windows).
    pub fn new_with_timeouts(
        schema_budget: SchemaBudget,
        timeouts: dispatch::HttpTimeouts,
    ) -> Result<Self, ProtocolError> {
        Ok(Self {
            client: dispatch::build_client_with_timeouts(&timeouts)?,
            schema_budget,
            timeouts,
        })
    }

    /// Executes ONE codex turn (always `stream: true`; R05-T04: decoded
    /// INCREMENTALLY — delta events emit live to `deltas`, the terminal
    /// event's aggregate is validated by the shared responses parser). An
    /// `Err` from the sink means the driver is gone: the read stops and the
    /// call winds down as cancelled.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_chat<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        route: &'a ResolvedModelRoute,
        auth: &'a ApplicableAuth,
        compat: Option<&'a super::compat::CompatCall>,
        deltas: &'a dyn lingxi_kernel::ports::TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(self.execute_chat_stream(ctx, call, input, route, auth, compat, deltas))
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_chat_stream(
        &self,
        ctx: &RunContext,
        call: &ModelCallId,
        input: &ModelTurnInput,
        route: &ResolvedModelRoute,
        auth: &ApplicableAuth,
        compat: Option<&super::compat::CompatCall>,
        deltas: &dyn lingxi_kernel::ports::TurnDeltaSink,
    ) -> ProviderTurnResult {
        let served_by = || lingxi_kernel::ports::ProviderDescriptor {
            provider: route.provider.clone(),
            model: route.model.clone(),
            operation: route.operation.describe(),
        };
        let fail = |error: ProtocolError, retryable: bool| {
            ProviderTurnResult::of_ctx(ctx, ProviderTurn::Failed { error, retryable })
                .with_served_by(served_by())
        };
        // The account id is a hard requirement of this family (the
        // incumbent refuses the same way): only an OAuth bearer JWT carries
        // it. Anything else is a loud non-retryable auth failure — never a
        // fabricated id, never a request without one.
        let ApplicableAuth::Bearer(token) = auth else {
            return fail(
                ProtocolError::new(
                    ErrorCode::Unauthorized,
                    "openai-codex-responses requires an OAuth bearer credential (the codex \
                     account id rides its JWT); this route's credential shape cannot serve it"
                        .to_string(),
                    false,
                ),
                false,
            );
        };
        let Some(account_id) = extract_account_id_from_token(token) else {
            return fail(
                ProtocolError::new(
                    ErrorCode::Unauthorized,
                    "Codex OAuth account id is required for openai-codex-responses (the \
                     access token's JWT carries no chatgpt_account_id claim)"
                        .to_string(),
                    false,
                ),
                false,
            );
        };
        let body = match render_codex_request(input, route) {
            Ok(body) => body,
            Err(error) => return fail(error, false),
        };
        // R05-T05: the ported provider-compat layer patches the rendered
        // envelope before dispatch (None pins the byte-exact golden wire).
        let body = match super::compat::apply_for_call(body, route, compat) {
            Ok(body) => body,
            Err(error) => return fail(error, false),
        };
        let url = resolve_codex_responses_url(&route.endpoint);
        let request = self
            .client
            .post(&url)
            .bearer_auth(token)
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "pi")
            .header("chatgpt-account-id", account_id)
            .json(&body);
        let response = match dispatch::send_with_timeouts(
            request,
            &self.timeouts,
            input.deadline_unix_ms,
            auth,
        )
        .await
        {
            Ok(response) => response,
            Err((error, retryable)) => return fail(error, retryable),
        };
        let accumulator = ResponsesStreamAccumulator::new(FAMILY);
        let mut drive_handler = dispatch::FamilyStreamDrive {
            accumulator,
            sink: deltas,
        };
        let drive = dispatch::drive_sse_stream_within_budget(
            response,
            &mut drive_handler,
            input.deadline_unix_ms,
        )
        .await;
        if let Err((error, retryable)) = drive {
            return fail(error, retryable);
        }
        match drive_handler
            .accumulator
            .finish(call, &input.tools, &self.schema_budget)
        {
            Ok(parsed) => ProviderTurnResult {
                fence: lingxi_kernel::ports::ResultFence::of_ctx(ctx),
                usage: parsed.usage(),
                turn: parsed.turn,
                usage_report: parsed.usage_report,
                served_protocol: Some(
                    lingxi_kernel::model_exchange::ProtocolFamily::OpenAiCodexResponses
                        .config_name()
                        .to_string(),
                ),
                transport_attempts: 1,
                served_by: Some(served_by()),
            },
            Err(error) => {
                let retryable = error.retryable;
                fail(error, retryable)
            }
        }
    }
}

/// Builds the codex request body (pure): the shared responses `input`
/// rendering plus the family's fixed `store:false / stream:true` and the
/// instructions rule.
pub fn render_codex_request(
    input: &ModelTurnInput,
    route: &ResolvedModelRoute,
) -> Result<serde_json::Value, ProtocolError> {
    let items = render_input_items(input, FAMILY)?;
    let tools: Vec<serde_json::Value> = input
        .tools
        .declarations
        .iter()
        .map(|declaration| {
            serde_json::json!({
                "type": "function",
                "name": declaration.wire_name,
                "description": declaration.description,
                "parameters": declaration.input_schema.schema,
            })
        })
        .collect();
    let instructions = match &input.system_prompt {
        Some(system) if !system.trim().is_empty() => system.clone(),
        _ => DEFAULT_CODEX_INSTRUCTIONS.to_string(),
    };
    let mut body = serde_json::json!({
        "model": route.model,
        "store": false,
        "stream": true,
        "instructions": instructions,
        "input": items,
    });
    // R05-T06: the host-decided output cap (the incumbent `callText`
    // `max_output_tokens`); absent when undecided.
    if let Some(cap) = input.max_output_tokens {
        body["max_output_tokens"] = serde_json::Value::from(cap);
    }
    if !tools.is_empty() {
        body["tools"] = serde_json::Value::Array(tools);
    }
    Ok(body)
}

/// The codex buffered-parse entry (golden tests; production always takes
/// the stream path).
pub fn parse_codex_response(
    call: &ModelCallId,
    snapshot: &ToolDeclarationSnapshot,
    body: &serde_json::Value,
    schema_budget: &SchemaBudget,
) -> Result<ParsedChat, ProtocolError> {
    parse_responses_response_as(call, snapshot, body, schema_budget, FAMILY)
}

/// The codex stream-parse entry: the shared responses stream aggregation
/// with THIS family's opaque tag (the exact path `execute_chat` takes after
/// the SSE read).
pub fn parse_codex_stream(
    call: &ModelCallId,
    snapshot: &ToolDeclarationSnapshot,
    events: &[super::streaming::SseEvent],
    schema_budget: &SchemaBudget,
) -> Result<ParsedChat, ProtocolError> {
    parse_responses_stream_as(call, snapshot, events, schema_budget, FAMILY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_kernel::model_exchange::{
        CredentialAuthKind, CredentialReference, ModelOperation, ProtocolFamily,
        ToolDeclarationSnapshot,
    };

    fn route() -> ResolvedModelRoute {
        ResolvedModelRoute {
            provider: "openai-codex-oauth".to_string(),
            model: "gpt-codex-test".to_string(),
            operation: ModelOperation::Chat,
            protocol: ProtocolFamily::OpenAiCodexResponses,
            endpoint: "https://chatgpt.example.test/backend-api".to_string(),
            credential: CredentialReference {
                provider: "openai-codex-oauth".to_string(),
                auth: CredentialAuthKind::OAuth,
            },
            config_generation: 1,
            group_id: None,
        }
    }

    #[test]
    fn the_incumbent_url_rule_is_verbatim() {
        assert_eq!(
            resolve_codex_responses_url("https://chatgpt.com/backend-api/"),
            "https://chatgpt.com/backend-api/codex/responses"
        );
        assert_eq!(
            resolve_codex_responses_url("https://proxy.test/codex"),
            "https://proxy.test/codex/responses"
        );
        assert_eq!(
            resolve_codex_responses_url("https://proxy.test/codex/responses"),
            "https://proxy.test/codex/responses"
        );
    }

    #[test]
    fn account_id_extracts_from_the_jwt_claim_or_fails_honestly() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({
                "https://api.openai.com/auth": {"chatgpt_account_id": "acct-123"}
            }))
            .expect("json"),
        );
        let token = format!("header.{payload}.signature");
        assert_eq!(
            extract_account_id_from_token(&token).as_deref(),
            Some("acct-123")
        );
        // Not a JWT / no claim / empty claim → None (the caller refuses).
        assert_eq!(extract_account_id_from_token("not-a-jwt"), None);
        let bare = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&serde_json::json!({"sub": "x"})).expect("json"));
        assert_eq!(extract_account_id_from_token(&format!("h.{bare}.s")), None);
    }

    #[test]
    fn codex_body_pins_store_stream_and_instructions() {
        let input = ModelTurnInput::first_turn("hello", ToolDeclarationSnapshot::empty());
        let body = render_codex_request(&input, &route()).expect("renders");
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["instructions"], DEFAULT_CODEX_INSTRUCTIONS);
        assert!(body.get("max_output_tokens").is_none());
        assert!(body.get("temperature").is_none());
        assert_eq!(body["input"][0]["role"], "user");

        let mut hosted = ModelTurnInput::first_turn("hello", ToolDeclarationSnapshot::empty());
        hosted.system_prompt = Some("host persona".to_string());
        let body = render_codex_request(&hosted, &route()).expect("renders");
        assert_eq!(body["instructions"], "host persona");
    }
}
