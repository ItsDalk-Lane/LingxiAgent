//! The OAuth login surface of the credential authority (R05 RR1 F04): the
//! six R00-T02 exclusive leaves — start/callback/device-poll to credential
//! installation, per-provider loggedIn + model counts, the OAuth model
//! listing, the custom modelId registry, and logout — served by the SAME
//! `CredentialService` that owns refresh/revoke (one credential authority,
//! no second store, no parallel install path).
//!
//! Boundary discipline:
//! - Every login result lands through the FENCED install path
//!   ([`super::install_tokens`]): the transaction records the cell's
//!   (instance, generation) at start; a late login that returns after a
//!   revoke, a replacing reload or a provider recreation is DISCARDED,
//!   never installed (the C05/F03 fences).
//! - A login transaction is ONE-SHOT per provider: the state is CONSUMED
//!   by the FIRST completing attempt — the browser listener leg and the
//!   manual-code leg both consume through the same state-matched
//!   atomic take ([`Self::consume_pkce_login_matching`], F30) — so a
//!   replayed callback finds nothing left (C08 — wrong state, expired
//!   state, duplicate callback and a foreign principal all refuse with
//!   zero writes).
//! - Nothing a caller supplies carries material OUT: the start guidance is
//!   the authorize URL / device prompt; outcomes carry status flags only.
//! - Failures are honest: a failed or fenced login never reports
//!   `loggedIn`.

use std::sync::Arc;
use std::time::Duration;

use lingxi_adapters::models::config::{AuthConfig, OAuthFlowConfig, OAuthFlowKind};
use lingxi_adapters::models::credentials::CredentialError;
use lingxi_adapters::models::oauth::{self, DeviceCodeGrant, OAuthError, OAuthHttp};

use super::{CredentialService, ProviderMaterial};

/// The login-start guidance of one provider (what the user must see — the
/// authorize URL for the PKCE flow, the code+URI for the device flow).
/// Carries no secrets.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(
    rename_all = "camelCase",
    tag = "flow",
    rename_all_fields = "camelCase"
)]
pub enum LoginStart {
    AuthorizationCodePkce {
        authorize_url: String,
        callback_addr: std::net::SocketAddr,
        expires_at_unix_ms: u64,
    },
    DeviceCode {
        user_code: String,
        verification_uri: String,
        interval_ms: u64,
        expires_at_unix_ms: u64,
    },
}

/// The state of one device-code login after a poll round.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(
    rename_all = "camelCase",
    tag = "status",
    rename_all_fields = "camelCase"
)]
pub enum DevicePollStatus {
    /// `authorization_pending` / `slow_down` — poll again after
    /// `interval_ms`.
    Pending { slow_down: bool, interval_ms: u64 },
    /// The grant completed and the token set was installed (the C06
    /// `persisted` honesty flag travels along).
    LoggedIn { persisted: bool },
}

/// The listing of one OAuth provider's models (the CFEC64F68DDE leaf):
/// the deduplicated union of the config-bound route models and the custom
/// registry — served ONLY while logged in (a logged-out provider has zero
/// AVAILABLE models, whatever its registry says).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthModelListing {
    pub provider: String,
    pub logged_in: bool,
    pub models: Vec<String>,
}

/// One in-flight login transaction (one per provider max — a new start
/// cancels and replaces the previous one). `kind` carries the flow
/// secrets that only a completing attempt can consume.
pub(crate) struct PendingLogin {
    pub(crate) started_by: String,
    pub(crate) cell_instance: u64,
    pub(crate) cell_generation: u64,
    pub(crate) expires_at_unix_ms: u64,
    pub(crate) kind: PendingLoginKind,
    pub(crate) cancel: Arc<oauth::NotifyCancel>,
}

pub(crate) enum PendingLoginKind {
    AuthorizationCodePkce {
        state: String,
        verifier: String,
        redirect_uri: String,
    },
    DeviceCode {
        device_code: String,
        interval_ms: u64,
    },
}

/// The `'static` flow-clock bridge parts, leaked EXACTLY ONCE per
/// credential service (the `FlowClock` type is invariant over its
/// lifetime parameter, so a `'static` pair is the sanctioned bridge — the
/// same pattern the oauth flow fixtures use). Two small allocations per
/// SERVICE instance, never per call.
pub(crate) struct StaticFlowClockParts {
    pub(crate) now: &'static (dyn Fn() -> u64 + Send + Sync),
    pub(crate) sleep: &'static (dyn Fn(Duration) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
                  + Send
                  + Sync),
}

impl CredentialService {
    /// The login surface's flow clock (deterministic `now` through the
    /// injected service clock; production sleep).
    pub(crate) fn login_flow_clock(&self) -> oauth::FlowClock<'static> {
        let parts = self.inner.flow_clock_parts.get_or_init(|| {
            let clock = Arc::clone(&self.inner.clock);
            let now: &'static (dyn Fn() -> u64 + Send + Sync) =
                Box::leak(Box::new(move || clock.now_unix_ms()));
            let sleep: &'static (dyn Fn(
                Duration,
            )
                -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
                          + Send
                          + Sync) = Box::leak(Box::new(|duration| {
                Box::pin(tokio::time::sleep(duration))
                    as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
            }));
            StaticFlowClockParts { now, sleep }
        });
        oauth::FlowClock {
            now_unix_ms: parts.now,
            sleep: parts.sleep,
            request_timeout: super::OAUTH_REQUEST_TIMEOUT,
        }
    }
}

fn login_http() -> Result<OAuthHttp, CredentialError> {
    OAuthHttp::new().map_err(|error| CredentialError::Transient {
        provider: "<login-surface>".to_string(),
        detail: error.to_string(),
    })
}

impl CredentialService {
    /// R05 RR1 F04 (LA-99D6C304D697): starts a login for an OAuth provider
    /// through the service's single credential authority.
    /// AuthorizationCodePkce binds the loopback listener (a spawned task
    /// redeems the browser callback through the same completing path);
    /// DeviceCode requests the device authorization and returns the
    /// prompt. A previously pending transaction for the provider is
    /// CANCELLED and replaced (its state can never complete afterwards).
    pub async fn oauth_start(
        &self,
        principal: &str,
        provider: &str,
    ) -> Result<LoginStart, CredentialError> {
        let cell = self.cell_for(provider)?;
        let (seed, instance, generation) = {
            let guard = cell.lock().await;
            (guard.seed.clone(), guard.instance, guard.generation)
        };
        let AuthConfig::OAuth(flow) = &seed else {
            return Err(not_oauth(provider, &seed));
        };
        let cancel = oauth::NotifyCancel::new();
        // Replacing an older transaction cancels its listener first.
        if let Some(previous) = self.take_login(provider) {
            previous.cancel.cancel();
        }
        let now = self.inner.clock.now_unix_ms();
        let http = login_http()?;
        let start = match flow.flow {
            OAuthFlowKind::AuthorizationCodePkce => {
                let flow_clock = self.login_flow_clock();
                let flow = oauth::AuthorizationCodeFlow::begin(flow, &flow_clock)
                    .map_err(|error| login_error(provider, &error))?;
                let authorize_url = flow.authorize_url().to_string();
                let callback_addr = flow.callback_addr();
                let expires_at = now + oauth::OAUTH_STATE_TTL.as_millis() as u64;
                let (state, verifier, redirect_uri) = flow.pending_transaction_parts();
                self.inner
                    .logins
                    .lock()
                    .expect("credential logins lock")
                    .insert(
                        provider.to_string(),
                        PendingLogin {
                            started_by: principal.to_string(),
                            cell_instance: instance,
                            cell_generation: generation,
                            expires_at_unix_ms: expires_at,
                            kind: PendingLoginKind::AuthorizationCodePkce {
                                state: state.clone(),
                                verifier,
                                redirect_uri,
                            },
                            cancel: Arc::clone(&cancel),
                        },
                    );
                // The browser leg: a spawned task waits for the ONE valid
                // loopback callback and redeems it through the same
                // completing path the manual-code surface uses — INCLUDING
                // the one-shot consumption (F30): the task consumes THIS
                // flow's transaction (state-matched, so a completion of an
                // older flow never eats the transaction a newer start
                // installed) BEFORE the exchange, exactly like the
                // manual-code leg; after that, any replay — manual or
                // browser — finds nothing left. The task ends at the state
                // TTL, on cancellation, or after the one grant; a
                // wrong-state browser callback answers 400 and keeps
                // waiting (the flow machine's own rule, C08).
                let service = self.clone();
                let provider_owned = provider.to_string();
                let principal_owned = principal.to_string();
                let task_cancel = Arc::clone(&cancel);
                tokio::spawn(async move {
                    let flow_clock = service.login_flow_clock();
                    if let Ok(grant) = flow.await_callback(&flow_clock, task_cancel.as_ref()).await
                    {
                        if service
                            .consume_pkce_login_matching(&provider_owned, &principal_owned, &state)
                            .is_ok()
                        {
                            let _ = service
                                .complete_with_grant(&principal_owned, &provider_owned, grant)
                                .await;
                        }
                    }
                });
                LoginStart::AuthorizationCodePkce {
                    authorize_url,
                    callback_addr,
                    expires_at_unix_ms: expires_at,
                }
            }
            OAuthFlowKind::DeviceCode => {
                let flow_clock = self.login_flow_clock();
                let (grant, prompt) = oauth::request_device_authorization(&http, flow, &flow_clock)
                    .await
                    .map_err(|error| login_error(provider, &error))?;
                let expires_at = now + prompt.expires_in.as_millis() as u64;
                let interval_ms = prompt.interval.as_millis() as u64;
                self.inner
                    .logins
                    .lock()
                    .expect("credential logins lock")
                    .insert(
                        provider.to_string(),
                        PendingLogin {
                            started_by: principal.to_string(),
                            cell_instance: instance,
                            cell_generation: generation,
                            expires_at_unix_ms: expires_at,
                            kind: PendingLoginKind::DeviceCode {
                                device_code: grant.device_code().to_string(),
                                interval_ms,
                            },
                            cancel,
                        },
                    );
                LoginStart::DeviceCode {
                    user_code: prompt.user_code,
                    verification_uri: prompt.verification_uri,
                    interval_ms,
                    expires_at_unix_ms: expires_at,
                }
            }
        };
        Ok(start)
    }

    /// R05 RR1 F04: completes an authorization-code login with a MANUALLY
    /// supplied (state, code) pair — the 手输码 callback leg. Validation
    /// and consumption happen atomically under ONE lock hold
    /// ([`Self::consume_pkce_login_matching`]): a wrong state, a wrong flow
    /// kind, a foreign principal or an expired state refuse with ZERO
    /// writes AND leave the pending login intact (a caller who cannot
    /// present the state must not be able to kill the login session
    /// either); only a state-matching completion CONSUMES the one-shot
    /// transaction, and a replay after that finds nothing left.
    pub async fn oauth_complete_code(
        &self,
        principal: &str,
        provider: &str,
        state: &str,
        code: &str,
    ) -> Result<DevicePollStatus, CredentialError> {
        let pending = self.consume_pkce_login_matching(provider, principal, state)?;
        let PendingLoginKind::AuthorizationCodePkce {
            verifier,
            redirect_uri,
            ..
        } = pending.kind
        else {
            // Unreachable in practice: the kind was validated under the
            // same lock that removed the entry. Refuse honestly anyway.
            return Err(CredentialError::HandleRefused {
                provider: provider.to_string(),
                detail: "the pending login is a device-code transaction, not a callback flow"
                    .to_string(),
            });
        };
        let grant = oauth::CallbackGrant::new(code.to_string(), verifier, redirect_uri);
        self.complete_with_grant(principal, provider, grant).await
    }

    /// R05 RR1 F04: drives ONE device-code poll round. `Pending` keeps the
    /// transaction (with slow_down interval growth); `LoggedIn` consumes it
    /// through the fenced install; a terminal OAuth failure consumes it and
    /// reports the classification honestly (never `loggedIn`).
    pub async fn oauth_poll_device(
        &self,
        principal: &str,
        provider: &str,
    ) -> Result<DevicePollStatus, CredentialError> {
        let flow = {
            let cell = self.cell_for(provider)?;
            let guard = cell.lock().await;
            let AuthConfig::OAuth(flow) = &guard.seed else {
                return Err(not_oauth(provider, &guard.seed));
            };
            flow.clone()
        };
        let (started_by, cell_instance, cell_generation, expires_at, device_code, interval_ms) = {
            let logins = self.inner.logins.lock().expect("credential logins lock");
            let pending = logins
                .get(provider)
                .ok_or_else(|| CredentialError::HandleRefused {
                    provider: provider.to_string(),
                    detail: "no pending device-code login for this provider".to_string(),
                })?;
            let PendingLoginKind::DeviceCode {
                device_code,
                interval_ms,
            } = &pending.kind
            else {
                return Err(CredentialError::HandleRefused {
                    provider: provider.to_string(),
                    detail: "the pending login is not a device-code transaction".to_string(),
                });
            };
            (
                pending.started_by.clone(),
                pending.cell_instance,
                pending.cell_generation,
                pending.expires_at_unix_ms,
                device_code.clone(),
                *interval_ms,
            )
        };
        if started_by != principal {
            return Err(CredentialError::HandleRefused {
                provider: provider.to_string(),
                detail: "the login was started by a different principal".to_string(),
            });
        }
        if self.inner.clock.now_unix_ms() >= expires_at {
            self.take_login(provider);
            return Err(CredentialError::ReauthorizationRequired {
                provider: provider.to_string(),
                detail: "the device code expired without authorization".to_string(),
            });
        }
        let http = login_http()?;
        let flow_clock = self.login_flow_clock();
        let grant = DeviceCodeGrant::from_device_code(device_code);
        let outcome =
            oauth::poll_device_token(&http, &flow, &flow_clock, &grant, interval_ms / 1_000)
                .await
                .map_err(|error| {
                    // A TERMINAL verdict consumes the transaction (the login is
                    // over, honestly failed); a transient one keeps it retryable.
                    if !matches!(error, OAuthError::Transient { .. } | OAuthError::Cancelled) {
                        self.take_login(provider);
                    }
                    login_error(provider, &error)
                })?;
        match outcome {
            oauth::DevicePollOutcome::AuthorizationPending => Ok(DevicePollStatus::Pending {
                slow_down: false,
                interval_ms,
            }),
            oauth::DevicePollOutcome::SlowDown { interval } => {
                let grown = interval.as_millis() as u64;
                if let Some(entry) = self
                    .inner
                    .logins
                    .lock()
                    .expect("credential logins lock")
                    .get_mut(provider)
                {
                    if let PendingLoginKind::DeviceCode { interval_ms, .. } = &mut entry.kind {
                        *interval_ms = grown;
                    }
                }
                Ok(DevicePollStatus::Pending {
                    slow_down: true,
                    interval_ms: grown,
                })
            }
            oauth::DevicePollOutcome::Done(tokens) => {
                // Consume the transaction, then install through the fence.
                self.take_login(provider);
                let cell = self.cell_for(provider)?;
                let outcome = super::install_tokens(
                    &self.inner,
                    provider,
                    &cell,
                    tokens,
                    cell_instance,
                    cell_generation,
                )
                .await;
                match outcome {
                    super::FlightOutcome::Installed { persisted, .. } => {
                        Ok(DevicePollStatus::LoggedIn { persisted })
                    }
                    // The fence refused the install (a revoke/reload won
                    // the race): an honest refusal, never a fake login.
                    _ => Err(CredentialError::Revoked {
                        provider: provider.to_string(),
                    }),
                }
            }
        }
    }

    /// R05 RR1 F04 (LA-FC80B6C4FBE4): logout — the OAuth token row is
    /// deleted from the store, the auth cache (in-memory tokens, the
    /// in-flight refresh, any pending login) is cleared with a generation
    /// bump that fences late write-backs, and the refreshed model list is
    /// returned (a logged-out provider serves zero available models).
    /// Honest: a persist failure is reported, never a pretend-logout.
    pub async fn logout(&self, provider: &str) -> Result<OAuthModelListing, CredentialError> {
        let cell = self.cell_for(provider)?;
        let mut guard = cell.lock().await;
        let AuthConfig::OAuth(flow) = &guard.seed else {
            return Err(not_oauth(provider, &guard.seed));
        };
        let flow = flow.clone();
        // Cancel + drop any pending login transaction first.
        if let Some(pending) = self.take_login(provider) {
            pending.cancel.cancel();
        }
        // Persist-first: the store row's token leg goes BEFORE memory.
        if let Some(store) = &self.inner.store {
            store
                .remove_tokens(provider)
                .map_err(|detail| CredentialError::PersistenceFailed {
                    provider: provider.to_string(),
                    detail,
                })?;
        }
        guard.material = ProviderMaterial::OAuth(super::OAuthState { flow, tokens: None });
        // The bump fences in-flight refresh write-backs and outstanding
        // handles; new resolutions see the logged-out state.
        guard.generation += 1;
        guard.persisted = true;
        guard.last_error = None;
        guard.flight = None;
        drop(guard);
        self.oauth_models(provider).await
    }

    /// R05 RR1 F04 (LA-CFEC64F68DDE / LA-CA0BF9A7AEA9): the OAuth model
    /// listing of one provider. Non-OAuth providers are explicitly
    /// rejected. Material-free by construction.
    pub async fn oauth_models(&self, provider: &str) -> Result<OAuthModelListing, CredentialError> {
        let cell = self.cell_for(provider)?;
        let logged_in = {
            let guard = cell.lock().await;
            match &guard.seed {
                AuthConfig::OAuth(_) => matches!(
                    &guard.material,
                    ProviderMaterial::OAuth(state) if state.tokens.is_some()
                ),
                seed => return Err(not_oauth(provider, seed)),
            }
        };
        let models = if logged_in {
            self.available_model_ids(provider)
        } else {
            Vec::new()
        };
        Ok(OAuthModelListing {
            provider: provider.to_string(),
            logged_in,
            models,
        })
    }

    /// R05 RR1 F04 (LA-16CEB6D12A6A): adds a model id to the provider's
    /// custom registry (OAuth providers only) and returns the refreshed
    /// listing. Invalid or duplicate ids change nothing and report loudly.
    pub async fn oauth_add_model(
        &self,
        provider: &str,
        model_id: &str,
    ) -> Result<OAuthModelListing, CredentialError> {
        self.assert_oauth_provider(provider).await?;
        let trimmed = model_id.trim();
        if trimmed.is_empty()
            || trimmed.len() > 200
            || trimmed.chars().any(char::is_control)
            || trimmed.chars().any(char::is_whitespace)
        {
            return Err(invalid_model_id(provider, model_id));
        }
        let store = self.oauth_store(provider)?;
        store
            .add_custom_model(provider, trimmed)
            .map_err(|detail| CredentialError::PersistenceFailed {
                provider: provider.to_string(),
                detail,
            })?;
        self.oauth_models(provider).await
    }

    /// R05 RR1 F04 (LA-8060BE8AA02C): removes a model id from the custom
    /// registry and returns the refreshed listing. An absent id reports
    /// loudly and changes nothing.
    pub async fn oauth_remove_model(
        &self,
        provider: &str,
        model_id: &str,
    ) -> Result<OAuthModelListing, CredentialError> {
        self.assert_oauth_provider(provider).await?;
        let store = self.oauth_store(provider)?;
        store
            .remove_custom_model(provider, model_id)
            .map_err(|detail| CredentialError::PersistenceFailed {
                provider: provider.to_string(),
                detail,
            })?;
        self.oauth_models(provider).await
    }

    /// Removes the provider's pending login transaction — the ONE-SHOT
    /// consumption point for the paths that own the transaction outright
    /// (replacement by a new start, revoke/logout cancellation, device-flow
    /// terminal states).
    pub(crate) fn take_login(&self, provider: &str) -> Option<PendingLogin> {
        self.inner
            .logins
            .lock()
            .expect("credential logins lock")
            .remove(provider)
    }

    /// R05 RR1 F30: the ONE-SHOT consumption for a COMPLETING attempt —
    /// validation and removal happen under a SINGLE lock hold, so only a
    /// completion that belongs to THIS flow (PKCE kind, same principal,
    /// SAME state, unexpired) can ever consume the transaction. Every
    /// mismatch refuses with ZERO writes and leaves the transaction in
    /// place; in particular a completion carrying the state of an OLDER
    /// flow never eats the transaction a newer start installed (the states
    /// are independent CSPRNG draws, so a stale state can only mismatch),
    /// and a replay after a first completion finds nothing left.
    pub(crate) fn consume_pkce_login_matching(
        &self,
        provider: &str,
        principal: &str,
        state: &str,
    ) -> Result<PendingLogin, CredentialError> {
        let refuse = |detail: &str| CredentialError::HandleRefused {
            provider: provider.to_string(),
            detail: detail.to_string(),
        };
        let mut logins = self.inner.logins.lock().expect("credential logins lock");
        let Some(pending) = logins.get(provider) else {
            return Err(refuse(
                "no pending login for this provider (start one first)",
            ));
        };
        let PendingLoginKind::AuthorizationCodePkce {
            state: expected, ..
        } = &pending.kind
        else {
            return Err(refuse(
                "the pending login is a device-code transaction, not a callback flow",
            ));
        };
        if pending.started_by != principal {
            return Err(refuse(
                "the login was started by a different principal (cross-subject completion)",
            ));
        }
        if state != expected {
            return Err(refuse(
                "callback state mismatch (zero writes; the pending login stays alive)",
            ));
        }
        if self.inner.clock.now_unix_ms() >= pending.expires_at_unix_ms {
            return Err(refuse("the login state expired before the callback"));
        }
        // State-matched: consume (remove) while STILL holding the lock — a
        // concurrent twin of this completion, or a replacing start, loses
        // the race here and cannot be fed the removed verifier.
        Ok(logins
            .remove(provider)
            .expect("the pending login was validated under this lock"))
    }

    /// The completing path both login surfaces share: validate the flow,
    /// exchange the grant, install through the fenced
    /// [`super::install_tokens`]. The transaction was ALREADY consumed by
    /// the caller — BOTH callers (the manual-code surface and the browser
    /// listener task) take the state-matched one-shot consumption
    /// ([`Self::consume_pkce_login_matching`], F30) BEFORE calling this —
    /// and the install's own (instance, generation) fence is the second,
    /// authoritative line of defense.
    pub(crate) async fn complete_with_grant(
        &self,
        principal: &str,
        provider: &str,
        grant: oauth::CallbackGrant,
    ) -> Result<DevicePollStatus, CredentialError> {
        let cell = self.cell_for(provider)?;
        let (flow, instance, generation) = {
            let guard = cell.lock().await;
            let AuthConfig::OAuth(flow) = &guard.seed else {
                return Err(not_oauth(provider, &guard.seed));
            };
            (flow.clone(), guard.instance, guard.generation)
        };
        let _ = principal;
        let http = login_http()?;
        let flow_clock = self.login_flow_clock();
        let tokens = oauth::AuthorizationCodeFlow::exchange(&http, &flow, &grant, &flow_clock)
            .await
            .map_err(|error| login_error(provider, &error))?;
        let outcome =
            super::install_tokens(&self.inner, provider, &cell, tokens, instance, generation).await;
        match outcome {
            super::FlightOutcome::Installed { persisted, .. } => {
                Ok(DevicePollStatus::LoggedIn { persisted })
            }
            // A fence refused the install (a revoke/reload won the race):
            // an honest refusal, never a fake login.
            _ => Err(CredentialError::Revoked {
                provider: provider.to_string(),
            }),
        }
    }

    async fn assert_oauth_provider(&self, provider: &str) -> Result<(), CredentialError> {
        let cell = self.cell_for(provider)?;
        let guard = cell.lock().await;
        match &guard.seed {
            AuthConfig::OAuth(_) => Ok(()),
            seed => Err(not_oauth(provider, seed)),
        }
    }

    fn oauth_store(
        &self,
        provider: &str,
    ) -> Result<&super::store::CredentialStore, CredentialError> {
        self.inner
            .store
            .as_ref()
            .ok_or_else(|| CredentialError::PersistenceFailed {
                provider: provider.to_string(),
                detail: "no credential store configured for the custom model registry".to_string(),
            })
    }
}

fn invalid_model_id(provider: &str, model_id: &str) -> CredentialError {
    CredentialError::NotConfigured {
        provider: format!(
            "{provider}: invalid custom model id {model_id:?} (empty, whitespace, control \
             characters or longer than 200 chars are refused)"
        ),
    }
}

fn not_oauth(provider: &str, seed: &AuthConfig) -> CredentialError {
    let kind = match seed {
        AuthConfig::ApiKey { .. } => "apiKey",
        AuthConfig::AuthHeader { .. } => "authHeader",
        AuthConfig::OAuth(_) => "oauth",
        AuthConfig::None => "none",
    };
    CredentialError::NotOAuth {
        provider: provider.to_string(),
        kind: kind.to_string(),
    }
}

fn login_error(provider: &str, error: &OAuthError) -> CredentialError {
    match error {
        OAuthError::Transient { detail } => CredentialError::Transient {
            provider: provider.to_string(),
            detail: detail.clone(),
        },
        OAuthError::ReauthorizationRequired { detail }
        | OAuthError::Expired { detail }
        | OAuthError::Protocol { detail }
        | OAuthError::StateRejected { detail } => CredentialError::ReauthorizationRequired {
            provider: provider.to_string(),
            detail: detail.clone(),
        },
        OAuthError::Cancelled => CredentialError::Transient {
            provider: provider.to_string(),
            detail: "the login flow was cancelled".to_string(),
        },
    }
}

// The flow config type is re-exported for the module's readers.
#[allow(dead_code)]
fn _flow_type_marker(_flow: &OAuthFlowConfig) {}
