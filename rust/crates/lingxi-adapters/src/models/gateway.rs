//! The config-backed model gateway (R05-T01): the production
//! [`ModelGatewayPort`]. ONE place decides which provider/model pair serves
//! an operation; every refusal is loud (no fallback route, no silent model
//! substitution).
//!
//! Reload semantics (C05): the gateway holds an atomic snapshot; a reload
//! validates the new config completely and swaps the snapshot, bumping the
//! generation. An in-flight call keeps the snapshot it resolved against —
//! it never observes a half-swapped config.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use lingxi_kernel::model_exchange::{
    CredentialAuthKind, CredentialReference, ModelGatewayError, ModelGatewayPort,
    ModelRouteRequest, ProtocolFamily, ResolvedModelRoute,
};

use super::config::{AuthConfig, ModelPlaneConfig};

/// The pre-parsed provider entry of one snapshot (protocol already in the
/// contract vocabulary; the endpoint already validated).
#[derive(Debug, Clone)]
struct ProviderEntry {
    protocol: ProtocolFamily,
    endpoint: String,
    auth: AuthConfig,
}

/// One immutable configuration snapshot.
#[derive(Debug)]
struct GatewaySnapshot {
    generation: u64,
    providers: BTreeMap<String, ProviderEntry>,
    models: super::config::ModelsSection,
}

impl GatewaySnapshot {
    fn from_config(generation: u64, config: ModelPlaneConfig) -> Self {
        let providers = config
            .providers
            .iter()
            .map(|(id, provider)| {
                (
                    id.clone(),
                    ProviderEntry {
                        // Validated at load; parse cannot fail here.
                        protocol: ProtocolFamily::parse(&provider.protocol)
                            .expect("validated at load"),
                        endpoint: provider.endpoint.clone(),
                        auth: provider.auth.clone(),
                    },
                )
            })
            .collect();
        Self {
            generation,
            providers,
            models: config.models,
        }
    }
}

/// The production model gateway. Cloneable handle: every clone shares the
/// same atomic snapshot cell (the service hands clones to the provider
/// adapter and the management reload surface).
#[derive(Debug, Clone)]
pub struct ConfigModelGateway {
    current: Arc<RwLock<Arc<GatewaySnapshot>>>,
}

impl ConfigModelGateway {
    /// Builds the gateway from a validated config (generation 1). A config
    /// with zero providers is legal (every route resolves to a loud
    /// `RouteNotConfigured`/`UnknownProvider` — the explicit unconfigured
    /// state, C03).
    pub fn from_validated(config: ModelPlaneConfig) -> Self {
        Self {
            current: Arc::new(RwLock::new(Arc::new(GatewaySnapshot::from_config(
                1, config,
            )))),
        }
    }

    /// Atomically swaps in a new validated config and returns the new
    /// generation (C05). Validation happened in
    /// [`ModelPlaneConfig::validate`] before this call — a reload never
    /// applies a partial or invalid config.
    pub fn reload(&self, config: ModelPlaneConfig) -> u64 {
        let mut current = self.current.write().expect("gateway snapshot lock");
        let generation = current.generation + 1;
        *current = Arc::new(GatewaySnapshot::from_config(generation, config));
        generation
    }

    /// The provider entries of the current snapshot (diagnostics/management
    /// surfaces; auth materials are NOT exposed through it).
    ///
    /// C01 note (R05-T02): this gateway deliberately has NO credential-
    /// material accessor — material exits the plane only through
    /// [`super::credentials::ProviderCredentialPort`].
    pub fn provider_ids(&self) -> Vec<String> {
        self.current
            .read()
            .expect("gateway snapshot lock")
            .providers
            .keys()
            .cloned()
            .collect()
    }

    /// The (provider id, endpoint) pairs of the CURRENT snapshot. An
    /// endpoint is configuration, not credential material (it is already
    /// visible wherever the config file is); the R05-T06 operation plane
    /// seeds its egress allowlist from it — URL media products may only
    /// leave through a same-origin destination or public https.
    pub fn provider_endpoints(&self) -> Vec<(String, String)> {
        self.current
            .read()
            .expect("gateway snapshot lock")
            .providers
            .iter()
            .map(|(id, entry)| (id.clone(), entry.endpoint.clone()))
            .collect()
    }

    /// The declared compat hints of the binding serving (provider, model)
    /// — the config-snapshot half of the TS model object's compat fields
    /// (R05-T05). Scans the configured routes (chat, the six auxiliary
    /// slots, the six T06 operation bindings); an explicit pair pin onto a
    /// model NO configured binding names carries no declared hints →
    /// `None` (the compat layer then derives by provider/endpoint/api
    /// only).
    pub fn compat_hints(
        &self,
        provider: &str,
        model: &str,
    ) -> Option<super::config::RouteCompatHints> {
        let snapshot = self.current.read().expect("gateway snapshot lock");
        let models = &snapshot.models;
        let found = [
            models.chat.as_ref(),
            models.title.as_ref(),
            models.summarize.as_ref(),
            models.memory.as_ref(),
            models.vision.as_ref(),
            models.approval.as_ref(),
            models.guard.as_ref(),
            models.embedding.as_ref(),
            models.rerank.as_ref(),
            models.image.as_ref(),
            models.video.as_ref(),
            models.speech.as_ref(),
            models.speech_recognition.as_ref(),
        ]
        .into_iter()
        .flatten()
        .find(|binding| binding.provider == provider && binding.model == model)
        .and_then(|binding| binding.compat.clone());
        found
    }
}

impl ModelGatewayPort for ConfigModelGateway {
    fn resolve_route(
        &self,
        request: &ModelRouteRequest,
    ) -> Result<ResolvedModelRoute, ModelGatewayError> {
        let snapshot = self.current.read().expect("gateway snapshot lock");
        // Every operation resolves its OWN binding (chat, the six
        // auxiliary slots, and the six T06 operation bindings share one
        // independent-resolution surface); an operation without a binding
        // is the explicit unconfigured state — never a silent fallback
        // onto the chat route (C06/C07).
        let configured: Option<&super::config::RouteBinding> =
            snapshot.models.binding_for_operation(request.operation);
        // The pin pair rule (C04): provider+model are ONE identity unit.
        let binding = match (&request.provider, &request.model) {
            (Some(provider), Some(model)) => {
                if !snapshot.providers.contains_key(provider) {
                    return Err(ModelGatewayError::UnknownProvider {
                        provider: provider.clone(),
                    });
                }
                super::config::RouteBinding {
                    provider: provider.clone(),
                    model: model.clone(),
                    // An explicit pair pin onto an unconfigured model carries
                    // no declared hints (derivation only — see compat_hints)
                    // and no binding-scoped group id.
                    compat: None,
                    group_id: None,
                }
            }
            (Some(provider), None) => match configured {
                Some(binding) if &binding.provider == provider => binding.clone(),
                _ => {
                    return Err(ModelGatewayError::RouteNotConfigured {
                        operation: format!(
                            "{} (pinned provider {provider:?} holds no configured route for it)",
                            request.operation.describe()
                        ),
                    });
                }
            },
            (None, Some(model)) => {
                return Err(ModelGatewayError::ModelPinRequiresProvider {
                    model: model.clone(),
                });
            }
            (None, None) => match configured {
                Some(binding) => binding.clone(),
                None => {
                    return Err(ModelGatewayError::RouteNotConfigured {
                        operation: request.operation.describe(),
                    });
                }
            },
        };
        let Some(entry) = snapshot.providers.get(&binding.provider) else {
            return Err(ModelGatewayError::UnknownProvider {
                provider: binding.provider.clone(),
            });
        };
        // The family↔operation servable matrix (a contract fact): the
        // resolved provider's family must serve the requested operation
        // class. Load-time validation already refuses such a binding; a
        // pinned pair (provider+model) bypasses the binding table, so the
        // matrix is re-enforced HERE at resolution — never a silent
        // re-route onto another family's wire shape.
        if !entry.protocol.serves(request.operation.operation_class()) {
            return Err(ModelGatewayError::OperationUnsupportedByProvider {
                operation: request.operation.describe(),
                provider: binding.provider.clone(),
                protocol: entry.protocol.config_name(),
            });
        }
        let auth_kind = match &entry.auth {
            AuthConfig::ApiKey { api_key } => {
                if api_key.is_empty() {
                    return Err(ModelGatewayError::MissingCredential {
                        provider: binding.provider.clone(),
                    });
                }
                CredentialAuthKind::ApiKey
            }
            AuthConfig::AuthHeader { value, .. } => {
                if value.is_empty() {
                    return Err(ModelGatewayError::MissingCredential {
                        provider: binding.provider.clone(),
                    });
                }
                CredentialAuthKind::AuthHeader
            }
            // OAuth material never lives in the config; the route names the
            // kind and the credential service owns the token set.
            AuthConfig::OAuth(_) => CredentialAuthKind::OAuth,
            AuthConfig::None => CredentialAuthKind::None,
        };
        Ok(ResolvedModelRoute {
            provider: binding.provider.clone(),
            model: binding.model,
            operation: request.operation,
            protocol: entry.protocol,
            endpoint: entry.endpoint.clone(),
            credential: CredentialReference {
                // The reference's provider id is the RESOLVED route's
                // provider — the identity the material belongs to.
                provider: binding.provider,
                auth: auth_kind,
            },
            config_generation: snapshot.generation,
            group_id: binding.group_id,
        })
    }

    fn config_generation(&self) -> u64 {
        self.current
            .read()
            .expect("gateway snapshot lock")
            .generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::config::ModelPlaneConfig;
    use lingxi_kernel::model_exchange::ModelOperation;

    fn config_json(chat_model: &str) -> String {
        format!(
            r#"{{
                "providers": {{
                    "main": {{
                        "protocol": "openai-completions",
                        "endpoint": "https://api.example.test/v1",
                        "auth": {{"kind": "apiKey", "apiKey": "sk-test"}}
                    }},
                    "local": {{
                        "protocol": "openai-completions",
                        "endpoint": "http://127.0.0.1:11434/v1",
                        "auth": {{"kind": "none"}}
                    }}
                }},
                "models": {{
                    "chat": {{"provider": "main", "model": "{chat_model}"}},
                    "title": {{"provider": "local", "model": "qwen-test"}}
                }}
            }}"#
        )
    }

    fn gateway() -> ConfigModelGateway {
        ConfigModelGateway::from_validated(
            ModelPlaneConfig::parse_and_validate(&config_json("gpt-test")).expect("valid"),
        )
    }

    #[test]
    fn chat_and_auxiliary_routes_resolve_independently() {
        let gateway = gateway();
        let chat = gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .expect("chat resolves");
        assert_eq!(chat.provider, "main");
        assert_eq!(chat.model, "gpt-test");
        assert_eq!(chat.protocol, ProtocolFamily::OpenAiCompletions);
        assert_eq!(chat.credential.auth, CredentialAuthKind::ApiKey);
        assert_eq!(chat.config_generation, 1);
        let title = gateway
            .resolve_route(&ModelRouteRequest::for_operation(
                ModelOperation::Auxiliary(lingxi_kernel::model_exchange::AuxiliarySlot::Title),
            ))
            .expect("title resolves");
        assert_eq!(title.provider, "local");
        assert_eq!(title.credential.auth, CredentialAuthKind::None);
        // An unconfigured slot is a loud state, never the chat route's share.
        let guard = gateway.resolve_route(&ModelRouteRequest::for_operation(
            ModelOperation::Auxiliary(lingxi_kernel::model_exchange::AuxiliarySlot::Guard),
        ));
        assert!(matches!(
            guard,
            Err(ModelGatewayError::RouteNotConfigured { .. })
        ));
    }

    #[test]
    fn unconfigured_operations_are_loud_route_gaps() {
        let gateway = gateway();
        // R05-T06: every operation class resolves through its own binding;
        // with no binding configured the answer is the explicit
        // RouteNotConfigured state (never the chat route's share).
        for operation in [
            ModelOperation::Embedding,
            ModelOperation::Rerank,
            ModelOperation::SpeechRecognition,
            ModelOperation::MediaGeneration {
                kind: lingxi_kernel::model_exchange::MediaGenerationKind::Image,
            },
        ] {
            let err = gateway
                .resolve_route(&ModelRouteRequest::for_operation(operation))
                .unwrap_err();
            assert!(
                matches!(err, ModelGatewayError::RouteNotConfigured { .. }),
                "{err:?}"
            );
        }
    }

    #[test]
    fn the_pin_pair_rule_never_guesses_a_provider_from_a_model() {
        let gateway = gateway();
        let err = gateway
            .resolve_route(&ModelRouteRequest {
                operation: ModelOperation::Chat,
                provider: None,
                model: Some("gpt-test".to_string()),
            })
            .unwrap_err();
        assert!(matches!(
            err,
            ModelGatewayError::ModelPinRequiresProvider { .. }
        ));
        // A full pair pin resolves the pair explicitly — even onto a model
        // id the configured route does not name (the pin IS the identity).
        let pinned = gateway
            .resolve_route(&ModelRouteRequest {
                operation: ModelOperation::Chat,
                provider: Some("main".to_string()),
                model: Some("other-model".to_string()),
            })
            .expect("explicit pair resolves");
        assert_eq!(pinned.model, "other-model");
        // Provider-only pin must equal the configured route's provider.
        let err = gateway
            .resolve_route(&ModelRouteRequest {
                operation: ModelOperation::Chat,
                provider: Some("local".to_string()),
                model: None,
            })
            .unwrap_err();
        assert!(matches!(err, ModelGatewayError::RouteNotConfigured { .. }));
        let ok = gateway
            .resolve_route(&ModelRouteRequest {
                operation: ModelOperation::Chat,
                provider: Some("main".to_string()),
                model: None,
            })
            .expect("provider pin matching the configured route");
        assert_eq!(ok.model, "gpt-test");
    }

    #[test]
    fn a_family_outside_the_operations_matrix_is_loud() {
        // The family↔operation servable matrix (T06): a chat route onto a
        // speech-recognition family refuses at resolution (load-time
        // validation refuses the binding too — this pins the PIN path,
        // which bypasses the binding table).
        let json = config_json("gpt-test").replace("openai-completions", "volcengine-bigasr");
        let gateway = ConfigModelGateway::from_validated(
            ModelPlaneConfig::parse_and_validate(&json).expect("valid"),
        );
        let err = gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .unwrap_err();
        assert!(
            matches!(
                err,
                ModelGatewayError::OperationUnsupportedByProvider { .. }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn the_five_chat_families_resolve_for_dispatch() {
        for family in [
            "openai-completions",
            "anthropic-messages",
            "google-generative-ai",
            "openai-responses",
            "openai-codex-responses",
        ] {
            let json = config_json("gpt-test").replace("openai-completions", family);
            let gateway = ConfigModelGateway::from_validated(
                ModelPlaneConfig::parse_and_validate(&json).expect("valid"),
            );
            let route = gateway
                .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
                .unwrap_or_else(|err| panic!("{family} must resolve for dispatch: {err}"));
            assert_eq!(route.protocol.config_name(), family);
        }
    }

    #[test]
    fn reload_swaps_atomically_and_bumps_the_generation() {
        let gateway = gateway();
        assert_eq!(gateway.config_generation(), 1);
        let generation = gateway.reload(
            ModelPlaneConfig::parse_and_validate(&config_json("gpt-test-v2")).expect("valid"),
        );
        assert_eq!(generation, 2);
        assert_eq!(gateway.config_generation(), 2);
        let route = gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .expect("resolves");
        assert_eq!(route.model, "gpt-test-v2");
        assert_eq!(route.config_generation, 2);
    }
}
