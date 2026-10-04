//! The gateway-backed turn provider (R05-T01/T02): the production
//! [`TurnProviderPort`]. Every `next_turn` resolves the chat route through
//! the gateway AT SEND TIME (a config reload takes effect on the next
//! call, C05) and dispatches through the protocol adapter of the route's
//! family. Resolution failures are loud [`ProviderTurn::Failed`]s — the
//! run settles `failed.provider_error`, never a silent fallback.
//!
//! Credential flow (R05-T02): material comes ONLY from the injected
//! [`ProviderCredentialPort`] (C01 — the gateway holds no material exit). A
//! 401 answer triggers ONE coordinated refresh report and, on a `Refreshed`
//! verdict, exactly ONE retry with the fresh material — there is no
//! unbounded 401 loop anywhere (C07). A 403 never triggers a refresh.

use std::sync::Arc;

use lingxi_kernel::model_exchange::{
    ModelGatewayPort, ModelOperation, ModelRouteRequest, ModelTurnInput,
};
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ErrorCode, ModelCallId, ProtocolError};

use super::anthropic_messages::AnthropicMessagesAdapter;
use super::credentials::{CredentialError, ProviderCredentialPort, RefreshVerdict};
use super::gateway::ConfigModelGateway;
use super::google_generative_ai::GoogleGenerativeAiAdapter;
use super::openai_codex_responses::OpenAiCodexResponsesAdapter;
use super::openai_completions::OpenAiCompletionsAdapter;
use super::openai_responses::OpenAiResponsesAdapter;

/// Maps a gateway resolution failure onto the wire error vocabulary (the
/// message carries the full diagnostic; the code classifies it).
fn gateway_error_code(error: &lingxi_kernel::model_exchange::ModelGatewayError) -> ErrorCode {
    use lingxi_kernel::model_exchange::ModelGatewayError;
    match error {
        ModelGatewayError::RouteNotConfigured { .. }
        | ModelGatewayError::UnknownProvider { .. } => ErrorCode::NotFound,
        ModelGatewayError::ModelPinRequiresProvider { .. }
        | ModelGatewayError::CapabilityUnsupported { .. }
        | ModelGatewayError::OperationUnsupportedByProvider { .. } => ErrorCode::InvalidMessage,
        ModelGatewayError::MissingCredential { .. } => ErrorCode::Unauthorized,
        ModelGatewayError::ProtocolNotImplemented { .. } => ErrorCode::Internal,
    }
}

/// Maps a credential failure onto the wire error vocabulary. `retryable`
/// honors the classification: a dead grant or a revocation is terminal, a
/// transient auth-endpoint failure is retryable (bounded by the run's own
/// attempt policy). A caller's own cancellation drops the resolve future —
/// it never surfaces as a credential error value.
fn credential_failure(error: &CredentialError) -> (ErrorCode, bool) {
    match error {
        CredentialError::NotConfigured { .. }
        | CredentialError::NotLoggedIn { .. }
        | CredentialError::Revoked { .. }
        | CredentialError::ReauthorizationRequired { .. } => (ErrorCode::Unauthorized, false),
        CredentialError::Transient { .. } => (ErrorCode::UpstreamUnavailable, true),
        CredentialError::PersistenceFailed { .. } => (ErrorCode::Internal, true),
        CredentialError::HandleRefused { .. } => (ErrorCode::Internal, false),
    }
}

/// The production provider: gateway + credential service + one adapter per
/// chat protocol family (R05-T03). Dispatch is by the route's resolved
/// family — never a silent re-route onto another family's wire shape.
pub struct GatewayedProvider {
    gateway: Arc<ConfigModelGateway>,
    credentials: Arc<dyn ProviderCredentialPort>,
    completions: OpenAiCompletionsAdapter,
    anthropic: AnthropicMessagesAdapter,
    google: GoogleGenerativeAiAdapter,
    responses: OpenAiResponsesAdapter,
    codex: OpenAiCodexResponsesAdapter,
}

impl GatewayedProvider {
    pub fn new(
        gateway: Arc<ConfigModelGateway>,
        credentials: Arc<dyn ProviderCredentialPort>,
        schema_budget: lingxi_kernel::toolcatalog::SchemaBudget,
    ) -> Result<Self, ProtocolError> {
        Ok(Self {
            gateway,
            credentials,
            completions: OpenAiCompletionsAdapter::new(schema_budget)?,
            // The anthropic family's REQUIRED max_tokens pins the documented
            // constant (the per-call budget is a registered contract gap).
            anthropic: AnthropicMessagesAdapter::new(
                schema_budget,
                super::anthropic_messages::DEFAULT_MAX_OUTPUT_TOKENS,
            )?,
            google: GoogleGenerativeAiAdapter::new(schema_budget)?,
            responses: OpenAiResponsesAdapter::new(schema_budget)?,
            codex: OpenAiCodexResponsesAdapter::new(schema_budget)?,
        })
    }

    /// Dispatches ONE turn through the adapter of the route's family. A
    /// family the gateway never resolves for chat (media/speech) reaching
    /// here is a loud internal failure, never a guessed re-route. R05-T04:
    /// every chat family streams — `deltas` receives the live increments.
    /// (Boxed-future shape: the trait's `Send + 'a` bound needs the
    /// explicit lifetime pinning an inherent `async fn` cannot express.)
    #[allow(clippy::too_many_arguments)]
    fn execute_route<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        route: &'a lingxi_kernel::model_exchange::ResolvedModelRoute,
        auth: &'a super::credentials::ApplicableAuth,
        compat: Option<&'a super::compat::CompatCall>,
        deltas: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(async move {
            use lingxi_kernel::model_exchange::ProtocolFamily;
            match route.protocol {
                ProtocolFamily::OpenAiCompletions => {
                    self.completions
                        .execute_chat(ctx, call, input, route, auth, compat, deltas)
                        .await
                }
                ProtocolFamily::AnthropicMessages => {
                    self.anthropic
                        .execute_chat(ctx, call, input, route, auth, compat, deltas)
                        .await
                }
                ProtocolFamily::GoogleGenerativeAi => {
                    // The google family takes NO compat patch (R05-T05
                    // registered deviation: every ported patch targets the
                    // openai/anthropic envelopes; no google route carries a
                    // compat declaration the port could honor).
                    self.google
                        .execute_chat(ctx, call, input, route, auth, deltas)
                        .await
                }
                ProtocolFamily::OpenAiResponses => {
                    self.responses
                        .execute_chat(ctx, call, input, route, auth, compat, deltas)
                        .await
                }
                ProtocolFamily::OpenAiCodexResponses => {
                    self.codex
                        .execute_chat(ctx, call, input, route, auth, compat, deltas)
                        .await
                }
                other => ProviderTurnResult::of_ctx(
                    ctx,
                    ProviderTurn::Failed {
                        error: ProtocolError::new(
                            ErrorCode::Internal,
                            format!(
                                "protocol family {} has no adapter in this build; the gateway \
                                 should have refused before dispatch (never silently re-routed)",
                                other.config_name()
                            ),
                            false,
                        ),
                        retryable: false,
                    },
                ),
            }
        })
    }

    fn failed(&self, ctx: &RunContext, error: &CredentialError) -> ProviderTurnResult {
        let (code, retryable) = credential_failure(error);
        ProviderTurnResult::of_ctx(
            ctx,
            ProviderTurn::Failed {
                error: ProtocolError::new(code, error.to_string(), retryable),
                retryable,
            },
        )
    }

    /// R05-T06 (§29.8): the operation-aware turn entry. The run driver's
    /// `next_turn` is `Chat` through here; auxiliary slots (and any other
    /// turn-shaped operation) resolve their OWN route binding through the
    /// same gateway/credential/refresh discipline — never a share of the
    /// chat route (C07), never a silent re-route (the servable matrix is
    /// enforced at resolution). The compat layer reads the operation's own
    /// option set (auxiliary = the incumbent `callText` Utility mode).
    pub fn next_turn_for_operation<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        operation: ModelOperation,
        input: &'a ModelTurnInput,
        deltas: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        Box::pin(async move {
            let route = match self
                .gateway
                .resolve_route(&ModelRouteRequest::for_operation(operation))
            {
                Ok(route) => route,
                Err(error) => {
                    let code = gateway_error_code(&error);
                    return ProviderTurnResult::of_ctx(
                        ctx,
                        ProviderTurn::Failed {
                            error: ProtocolError::new(code, error.to_string(), false),
                            // A configuration gap is not transient: retrying
                            // the same config fails the same way.
                            retryable: false,
                        },
                    );
                }
            };
            let auth = match self.credentials.resolve(&route).await {
                Ok(auth) => auth,
                Err(error) => return self.failed(ctx, &error),
            };
            // R05-T05: the compat call context of this dispatch — the route
            // binding's declared hints (config snapshot) + the operation's
            // option set. A pinned pair no configured binding names carries
            // no hints (derivation only).
            let compat = super::compat::CompatCall {
                hints: self.gateway.compat_hints(&route.provider, &route.model),
                options: super::compat::CompatOptions::for_operation(&operation),
            };
            let result = self
                .execute_route(ctx, call, input, &route, &auth, Some(&compat), deltas)
                .await;
            // The single bounded refresh-and-retry (C02/C07): ONLY a 401
            // (Unauthorized) on a credential-bearing route reports back to
            // the credential service; a `Refreshed` verdict earns exactly
            // one retry with the fresh material. A second 401 settles as the
            // failure it is — there is no refresh loop.
            let is_unauthorized = matches!(
                &result.turn,
                ProviderTurn::Failed { error, .. } if error.code == ErrorCode::Unauthorized
            );
            if !is_unauthorized || matches!(auth, super::credentials::ApplicableAuth::None) {
                return result;
            }
            match self.credentials.report_unauthorized(&route, &auth).await {
                Ok(RefreshVerdict::Refreshed { .. }) => {
                    let fresh = match self.credentials.resolve(&route).await {
                        Ok(fresh) => fresh,
                        Err(error) => return self.failed(ctx, &error),
                    };
                    // The retry re-streams through the SAME sink — safe:
                    // a 401 is classified from the STATUS before any body
                    // byte streams, so the first attempt emitted no deltas.
                    //
                    // R05-T07 (C03): the retry is a SECOND PHYSICAL
                    // provider request under the same logical call — the
                    // result is marked `transport_attempts: 2` so the
                    // usage ledger never merges two possibly-billable
                    // sends into one invisible request.
                    let mut retried = self
                        .execute_route(ctx, call, input, &route, &fresh, Some(&compat), deltas)
                        .await;
                    retried.transport_attempts = 2;
                    retried
                }
                Ok(RefreshVerdict::NotRefreshable) => result,
                Ok(RefreshVerdict::Revoked) => self.failed(
                    ctx,
                    &CredentialError::Revoked {
                        provider: route.provider.clone(),
                    },
                ),
                Ok(RefreshVerdict::ReauthorizationRequired { detail }) => self.failed(
                    ctx,
                    &CredentialError::ReauthorizationRequired {
                        provider: route.provider.clone(),
                        detail,
                    },
                ),
                Ok(RefreshVerdict::Transient { detail }) => self.failed(
                    ctx,
                    &CredentialError::Transient {
                        provider: route.provider.clone(),
                        detail,
                    },
                ),
                Err(error) => self.failed(ctx, &error),
            }
        })
    }
}

impl TurnProviderPort for GatewayedProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        // The identity of the CURRENT chat route when one resolves; the
        // explicit unconfigured marker otherwise (the persisted call facts
        // then honestly name that no provider/model served the call — never
        // a fabricated identity). The per-turn persisted identity is the
        // result's `served_by`; this descriptor is the fallback/diagnostic.
        match self
            .gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
        {
            Ok(route) => ProviderDescriptor {
                provider: route.provider,
                model: route.model,
                operation: "chat".to_string(),
            },
            Err(_) => ProviderDescriptor {
                provider: "<unconfigured>".to_string(),
                model: "<unconfigured>".to_string(),
                operation: "chat".to_string(),
            },
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        deltas: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        // The chat operation IS the run driver's turn (R05-T06: one
        // implementation serves chat and the auxiliary slots; the operation
        // selects the route binding and the compat option set, nothing
        // else).
        self.next_turn_for_operation(ctx, call, ModelOperation::Chat, input, deltas)
    }
}
