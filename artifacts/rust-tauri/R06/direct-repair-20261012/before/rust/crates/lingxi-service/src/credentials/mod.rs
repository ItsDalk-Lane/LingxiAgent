//! The CredentialService (R05-T02): the SINGLE credential-material exit of
//! the model plane (C01). The provider is the credential owner — models and
//! purposes reference a provider, they never hold a key of their own.
//!
//! Authority split:
//! - **Static material** (`apiKey` / `authHeader`) is SEEDED from the
//!   validated provider config (the config file stays the source of truth;
//!   a reload re-seeds).
//! - **OAuth material** lives ONLY in the credential store
//!   ([`store`], `{runtime_dir}/credentials.json`); config carries the flow
//!   descriptor. Login flows mint tokens; refresh rotates them.
//!
//! Refresh coordination (C02–C07):
//! - Every provider has ONE cell: `generation + material + in-flight
//!   flight`. A 401 (or a locally expired OAuth token) joins the in-flight
//!   refresh or starts it; concurrent 401s on the same provider merge into
//!   exactly ONE refresh (C02), different providers never block each other
//!   (C03 — there is no cross-provider lock on the refresh path).
//! - A waiter that is cancelled simply drops its wait — the flight and the
//!   other waiters are unaffected (C04). The flight itself is a spawned
//!   task: even the LAST waiter leaving does not abort the transport, so a
//!   minted-but-unstored token is never lost to a cancellation race (the
//!   request timeout bounds the task; the generation fence decides whether
//!   the result may land).
//! - A revoke bumps the generation; a refresh that minted against the old
//!   generation is fenced — the late token is discarded, never written back
//!   (C05).
//! - Write-back persists FIRST (atomic, fsync'd, 0600) and flips memory
//!   second; a persist failure is reported honestly (`persisted: false`,
//!   `last_persist_failure`) — never claimed as safely saved (C06).
//! - Error classification is the incumbent's: a dead grant needs
//!   re-authorization (terminal), a missing configuration is explicit, a
//!   transport/5xx failure is transient. There is no unbounded 401 loop:
//!   the caller retries exactly once after a `Refreshed` verdict (C07).
//!
//! Credential handles (C12) are unguessable 128-bit tokens bound to
//! (provider, generation, expiry): a forged, cross-provider, expired or
//! stale-generation handle is refused loudly.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};
use std::time::Duration;

use lingxi_adapters::models::config::{AuthConfig, ModelPlaneConfig, OAuthFlowConfig};
use lingxi_adapters::models::credentials::{
    ApplicableAuth, CredentialError, ProviderCredentialPort, RefreshVerdict,
};
use lingxi_adapters::models::oauth::{self, OAuthError, OAuthHttp, OAuthTokens};
use lingxi_kernel::model_exchange::{CredentialAuthKind, ResolvedModelRoute};

use crate::inject::ServiceClock;

pub mod login;
pub mod store;
use store::CredentialStore;

/// The production OAuth request timeout (the incumbent's
/// XAI_OAUTH_REQUEST_TIMEOUT_MS — 30s). Bounds every refresh flight's
/// transport.
pub const OAUTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The credential-handle lifetime (C12): a handle is a short-lived
/// capability for surfaces that must not hold material across a long wait.
pub const CREDENTIAL_HANDLE_TTL_MS: u64 = 60_000;

/// The refresh execution seam: production drives
/// [`oauth::refresh`] over the real HTTP client; tests drive scripted
/// outcomes (and prove the single-flight merge by counting executions).
pub trait RefreshDriver: Send + Sync {
    fn refresh<'a>(
        &'a self,
        provider: &'a str,
        flow: &'a OAuthFlowConfig,
        refresh_token: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<OAuthTokens, OAuthError>> + Send + 'a>,
    >;
}

/// The production driver: real HTTP, system clock, 30s request timeout.
pub struct ProductionRefreshDriver {
    http: OAuthHttp,
    request_timeout: Duration,
}

impl ProductionRefreshDriver {
    pub fn new(request_timeout: Duration) -> Result<Self, String> {
        Ok(Self {
            http: OAuthHttp::new().map_err(|err| err.to_string())?,
            request_timeout,
        })
    }

    /// R05 RR1 F14: the production constructor — the OAuth refresh
    /// transport bound to the model plane's shared, reloadable network
    /// policy (the legacy constructor keeps the pre-F14 direct behavior).
    pub fn with_network(
        request_timeout: Duration,
        network: std::sync::Arc<lingxi_adapters::models::network::NetworkPlane>,
    ) -> Result<Self, String> {
        Ok(Self {
            http: OAuthHttp::new_with_network(network).map_err(|err| err.to_string())?,
            request_timeout,
        })
    }
}

impl RefreshDriver for ProductionRefreshDriver {
    fn refresh<'a>(
        &'a self,
        _provider: &'a str,
        flow: &'a OAuthFlowConfig,
        refresh_token: &'a str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<OAuthTokens, OAuthError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let clock = oauth::system_flow_clock(self.request_timeout);
            oauth::refresh(&self.http, flow, refresh_token, &clock).await
        })
    }
}

/// Construction failures of the production credential service — loud
/// startup errors (exit 2), never a degraded anonymous plane.
#[derive(Debug)]
pub enum CredentialServiceError {
    /// The credential store cannot be understood (malformed / version
    /// conflict / unreadable).
    Store(store::StoreLoadError),
    /// The OAuth HTTP client cannot be constructed.
    Http(String),
}

impl std::fmt::Display for CredentialServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialServiceError::Store(err) => write!(f, "{err}"),
            CredentialServiceError::Http(detail) => {
                write!(f, "oauth http client construction failed: {detail}")
            }
        }
    }
}

impl std::error::Error for CredentialServiceError {}

/// The live credential material of one provider.
enum ProviderMaterial {
    /// Explicitly keyless.
    None,
    /// Static seed from the config (apiKey / authHeader).
    Static(ApplicableAuth),
    /// OAuth: the flow descriptor plus the current token set (None = login
    /// required).
    OAuth(OAuthState),
    /// Revoked at runtime (C05) — every resolution refuses until a reload
    /// re-seeds from the config (statics) or a login re-mints (OAuth; the
    /// store row was deleted at revoke time).
    Revoked,
}

struct OAuthState {
    flow: OAuthFlowConfig,
    tokens: Option<OAuthTokens>,
}

/// One provider's coordinated credential cell.
struct ProviderCell {
    /// R05 RR1 F03: the NEVER-reused instance identity of this cell. Every
    /// `seed_cell` allocation draws a fresh number from the service-wide
    /// monotonic counter — unlike `generation` (which restarts at 1 for
    /// every new cell), `instance` can never ABA: a handle, a login
    /// transaction or a refresh flight that recorded an older instance is
    /// refused against a re-seeded/recreated provider even when the new
    /// cell's `generation` numerically matches.
    instance: u64,
    /// The auth config this cell was seeded from (reload keeps the cell —
    /// its tokens, its in-flight flight — only when this is unchanged).
    seed: AuthConfig,
    /// R05 RR1 F02: the model-plane configuration generation this cell's
    /// SEED was installed at. Cells whose seed survives a reload KEEP
    /// their epoch (the material is unchanged — still the current
    /// generation's material); a re-seeded cell carries the generation
    /// the installing reload published. `resolve` refuses any route whose
    /// `config_generation` is OLDER than this epoch — the boundary that
    /// keeps newer credential material off an older route's endpoint
    /// (within-world OAuth refreshes do NOT touch this field: they rotate
    /// material inside one configuration generation).
    seed_epoch: u64,
    /// Bumped on every install/revoke — the fence against late write-backs.
    generation: u64,
    material: ProviderMaterial,
    /// The in-flight refresh (at most one per provider — C02).
    flight: Option<RefreshFlight>,
    /// Whether the CURRENT material is durable in the store (C06 honesty).
    persisted: bool,
    last_persist_failure: Option<String>,
    last_error: Option<String>,
    last_refresh_at_unix_ms: Option<u64>,
}

/// A shared refresh flight: waiters subscribe to the verdict slot; the
/// spawned task publishes exactly once. `watch::Receiver::wait_for` is
/// cancel-safe — a dropped waiter simply leaves (C04).
#[derive(Clone)]
struct RefreshFlight {
    verdicts: tokio::sync::watch::Receiver<Option<FlightOutcome>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FlightOutcome {
    Installed {
        generation: u64,
        persisted: bool,
    },
    /// The write-back was fenced (a revoke landed, or a reload replaced the
    /// cell): the minted token was discarded, never persisted (C05).
    Discarded,
    ReauthorizationRequired {
        detail: String,
    },
    Transient {
        detail: String,
    },
}

impl FlightOutcome {
    fn into_verdict(self) -> RefreshVerdict {
        match self {
            FlightOutcome::Installed {
                generation,
                persisted,
            } => RefreshVerdict::Refreshed {
                generation,
                persisted,
            },
            // The waiter learns the honest terminal state: the credential
            // it waited on is gone — re-authorization, not a retry.
            FlightOutcome::Discarded => RefreshVerdict::Revoked,
            FlightOutcome::ReauthorizationRequired { detail } => {
                RefreshVerdict::ReauthorizationRequired { detail }
            }
            FlightOutcome::Transient { detail } => RefreshVerdict::Transient { detail },
        }
    }
}

/// The material-free status snapshot of one provider (management surface).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCredentialStatus {
    pub provider: String,
    /// `apiKey` | `authHeader` | `oauth` | `none`.
    pub kind: &'static str,
    /// `ready` | `not_logged_in` | `revoked` | `keyless`.
    pub state: String,
    pub generation: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub refresh_in_flight: bool,
    pub persisted: bool,
    pub last_persist_failure: Option<String>,
    pub last_error: Option<String>,
    pub last_refresh_at_unix_ms: Option<u64>,
    /// R05 RR1 F04 (LA-CA0BF9A7AEA9): the OAuth login verdict — `false`
    /// for non-OAuth kinds (the concept does not apply).
    pub logged_in: bool,
    /// R05 RR1 F04: the number of AVAILABLE models (the config-bound ∪
    /// custom registry dedup, served only while logged in; non-OAuth
    /// kinds report 0 — their models are static configuration, visible on
    /// the plane itself).
    pub available_models: usize,
}

/// A minted credential handle (C12). The `handle_id` is 128 bits of system
/// CSPRNG; the binding to (provider, generation, expiry) is enforced at
/// resolve time against the service's registry — a handle is data, the
/// REGISTRY is the authority, so a forged or tampered handle is refused.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialHandle {
    pub handle_id: String,
    pub provider: String,
    pub expires_at_unix_ms: u64,
}

#[derive(Clone)]
struct HandleEntry {
    /// The trusted subject the handle was minted for (F03: a handle
    /// presented by any OTHER principal is refused before material moves).
    principal: String,
    provider: String,
    /// The cell INSTANCE the handle was minted against (F03 — never
    /// reused across a provider's lifecycle).
    instance: u64,
    generation: u64,
    expires_at_unix_ms: u64,
}

struct CredentialServiceInner {
    /// Brief std lock — only map lookups/swaps, never held across an await.
    providers: StdRwLock<BTreeMap<String, Arc<tokio::sync::Mutex<ProviderCell>>>>,
    store: Option<CredentialStore>,
    refresh: Arc<dyn RefreshDriver>,
    clock: Arc<dyn ServiceClock>,
    handles: StdMutex<BTreeMap<String, HandleEntry>>,
    /// R05 RR1 F03: the service-wide monotonic cell-instance counter. A
    /// number is drawn for every `seed_cell` (construction, re-seed,
    /// remove→recreate); numbers are NEVER reused within a service
    /// lifetime, which is what makes a stale handle/flight/login
    /// transaction impossible to revive against a numerically-equal
    /// generation of a LATER cell.
    next_instance: std::sync::atomic::AtomicU64,
    /// R05 RR1 F04: the model-plane's config-bound (provider → model ids)
    /// listing, updated at construction and every reload — the
    /// OAuth-model-registry half of "available models" (the other half is
    /// the per-provider custom registry in the store).
    config_models: StdRwLock<BTreeMap<String, Vec<String>>>,
    /// R05 RR1 F04: the in-flight OAuth login transactions (one per
    /// provider max — `oauth_start` replaces any older transaction).
    logins: StdMutex<BTreeMap<String, login::PendingLogin>>,
    /// R05 RR1 F04: the login surface's `'static` flow-clock bridge parts
    /// (leaked exactly once — see `login::StaticFlowClockParts`).
    flow_clock_parts: std::sync::OnceLock<login::StaticFlowClockParts>,
}

/// The credential service. Clone-cheap (one shared inner); the production
/// implementation of [`ProviderCredentialPort`].
#[derive(Clone)]
pub struct CredentialService {
    inner: Arc<CredentialServiceInner>,
}

impl CredentialService {
    /// The general constructor (injected store/driver/clock — the test and
    /// embedding seam). Seeds every provider from the validated config;
    /// OAuth providers additionally seed their token set from the store.
    /// The seed epoch is configuration generation 1, matching the gateway
    /// a freshly built plane starts at (R05 RR1 F02's lockstep contract:
    /// every subsequent plane install reloads BOTH sides with the SAME
    /// generation number).
    pub fn new(
        config: &ModelPlaneConfig,
        store: Option<CredentialStore>,
        refresh: Arc<dyn RefreshDriver>,
        clock: Arc<dyn ServiceClock>,
    ) -> Self {
        let mut providers = BTreeMap::new();
        let mut next_instance = 1_u64;
        for (id, provider) in &config.providers {
            providers.insert(
                id.clone(),
                Arc::new(tokio::sync::Mutex::new(seed_cell(
                    id,
                    &provider.auth,
                    store.as_ref(),
                    1,
                    next_instance,
                ))),
            );
            next_instance += 1;
        }
        Self {
            inner: Arc::new(CredentialServiceInner {
                providers: StdRwLock::new(providers),
                store,
                refresh,
                clock,
                handles: StdMutex::new(BTreeMap::new()),
                next_instance: std::sync::atomic::AtomicU64::new(next_instance),
                config_models: StdRwLock::new(config_models_of(config)),
                logins: StdMutex::new(BTreeMap::new()),
                flow_clock_parts: std::sync::OnceLock::new(),
            }),
        }
    }

    /// The production assembly: the store file inside the private runtime
    /// dir (loaded loudly — a malformed/conflicting store refuses startup,
    /// never a silent empty one), the real HTTP refresh driver.
    ///
    /// R05 RR1 F03: the store is ALWAYS constructed — a service that boots
    /// on a plane without OAuth providers must still be able to seed and
    /// durably persist OAuth material when a reload hot-adds an OAuth
    /// provider (the file itself is only created on the first write, so a
    /// static-only plane gains no empty `credentials.json`).
    pub fn bootstrap(
        config: &ModelPlaneConfig,
        runtime_dir: &std::path::Path,
        clock: Arc<dyn ServiceClock>,
    ) -> Result<Self, CredentialServiceError> {
        Self::bootstrap_with_network(config, runtime_dir, clock, None)
    }

    /// R05 RR1 F14: [`Self::bootstrap`] with the model plane's shared
    /// network policy (the OAuth refresh transport routes under the SAME
    /// frozen proxy / NO_PROXY / explicit-CA policy as every other model
    /// consumer; `None` keeps the pre-F14 direct behavior).
    pub fn bootstrap_with_network(
        config: &ModelPlaneConfig,
        runtime_dir: &std::path::Path,
        clock: Arc<dyn ServiceClock>,
        network: Option<std::sync::Arc<lingxi_adapters::models::network::NetworkPlane>>,
    ) -> Result<Self, CredentialServiceError> {
        let store = Some(
            CredentialStore::load(Arc::new(store::FsStoreIo::new(runtime_dir)))
                .map_err(CredentialServiceError::Store)?,
        );
        let refresh = match network {
            Some(network) => ProductionRefreshDriver::with_network(OAUTH_REQUEST_TIMEOUT, network),
            None => ProductionRefreshDriver::new(OAUTH_REQUEST_TIMEOUT),
        }
        .map_err(CredentialServiceError::Http)?;
        Ok(Self::new(config, store, Arc::new(refresh), clock))
    }

    fn cell_for(
        &self,
        provider: &str,
    ) -> Result<Arc<tokio::sync::Mutex<ProviderCell>>, CredentialError> {
        self.inner
            .providers
            .read()
            .expect("credential providers lock")
            .get(provider)
            .cloned()
            .ok_or_else(|| CredentialError::NotConfigured {
                // An unknown provider id here is a FORGED reference (the
                // gateway only resolves configured providers) — refused
                // loudly, never an anonymous fallback (C12).
                provider: provider.to_string(),
            })
    }

    fn is_expired(&self, tokens: &OAuthTokens) -> bool {
        self.inner.clock.now_unix_ms() >= tokens.expires_at_unix_ms
    }

    /// Joins the in-flight flight of the cell or starts a new one. Called
    /// with the cell lock HELD; returns the flight to wait on.
    fn join_or_start_flight(
        &self,
        provider: &str,
        cell: &Arc<tokio::sync::Mutex<ProviderCell>>,
        guard: &mut ProviderCell,
        flow: &OAuthFlowConfig,
        refresh_token: &str,
    ) -> RefreshFlight {
        if let Some(flight) = &guard.flight {
            return flight.clone();
        }
        let (sender, receiver) = tokio::sync::watch::channel(None);
        let flight = RefreshFlight { verdicts: receiver };
        guard.flight = Some(flight.clone());
        let task = run_refresh_flight(
            Arc::clone(&self.inner),
            provider.to_string(),
            Arc::clone(cell),
            flow.clone(),
            refresh_token.to_string(),
            guard.instance,
            guard.generation,
            sender,
        );
        tokio::spawn(task);
        flight
    }

    /// Waits for one flight's verdict. Cancel-safe: a dropped wait leaves
    /// the flight and every other waiter untouched (C04).
    async fn wait_flight(&self, flight: RefreshFlight) -> RefreshVerdict {
        let mut receiver = flight.verdicts;
        let outcome = match receiver.wait_for(|outcome| outcome.is_some()).await {
            Ok(reference) => reference.clone().expect("wait_for predicate passed"),
            // The sender was dropped without a verdict — the flight task is
            // gone (a panic; the runtime never cancels it). Loud, transient
            // classification: the caller's bounded policy decides.
            Err(_) => {
                return RefreshVerdict::Transient {
                    detail: "the refresh flight ended without a verdict (task lost)".to_string(),
                };
            }
        };
        outcome.into_verdict()
    }

    async fn resolve_impl(
        &self,
        route: &ResolvedModelRoute,
    ) -> Result<ApplicableAuth, CredentialError> {
        let cell = self.cell_for(&route.provider)?;
        // REV-T02 R01 F-01: at most ONE refresh flight per resolve. A
        // token endpoint that keeps minting already-expired tokens (an
        // `expires_in` truncating to 0, a one-second grant outlived by
        // its own roundtrip, a JWT exp already past) must not spin this
        // loop into a refresh storm — the second still-expired read is
        // a loud terminal error, never another refresh.
        let mut flight_awaited = false;
        loop {
            let flight = {
                let mut guard = cell.lock().await;
                // R05 RR1 F02: the generation-consistency boundary. A
                // route resolved against an older configuration generation
                // must never receive the CURRENT generation's material —
                // that is precisely how a reload could place a NEW key on
                // an OLD endpoint. Refuse loudly (a safe, retryable
                // failure: the caller's next call re-resolves the route).
                if guard.seed_epoch > route.config_generation {
                    return Err(CredentialError::StaleRoute {
                        provider: route.provider.clone(),
                        route_generation: route.config_generation,
                        seed_epoch: guard.seed_epoch,
                    });
                }
                match &guard.material {
                    ProviderMaterial::None => return Ok(ApplicableAuth::None),
                    ProviderMaterial::Static(auth) => return Ok(auth.clone()),
                    ProviderMaterial::Revoked => {
                        return Err(CredentialError::Revoked {
                            provider: route.provider.clone(),
                        });
                    }
                    ProviderMaterial::OAuth(state) => match &state.tokens {
                        None => {
                            return Err(CredentialError::NotLoggedIn {
                                provider: route.provider.clone(),
                            });
                        }
                        Some(tokens) if !self.is_expired(tokens) => {
                            return Ok(ApplicableAuth::Bearer(tokens.access_token.clone()));
                        }
                        // Locally expired (the ledger says so — the server
                        // may still accept it, but the incumbent's rule is
                        // to refresh first): the SAME single flight as the
                        // 401 path. Clone the flight inputs FIRST so the
                        // material borrow ends before the mutable join.
                        Some(tokens) => {
                            if flight_awaited {
                                return Err(CredentialError::Transient {
                                    provider: route.provider.clone(),
                                    detail: "the token endpoint minted an already-expired \
                                             token; the refresh did not converge within this \
                                             resolve's single-flight bound (F-01)"
                                        .to_string(),
                                });
                            }
                            flight_awaited = true;
                            let flow = state.flow.clone();
                            let refresh_token = tokens.refresh_token.clone();
                            self.join_or_start_flight(
                                &route.provider,
                                &cell,
                                &mut guard,
                                &flow,
                                &refresh_token,
                            )
                        }
                    },
                }
            };
            match self.wait_flight(flight).await {
                RefreshVerdict::Refreshed { .. } => continue, // re-read the fresh material
                RefreshVerdict::Revoked => {
                    return Err(CredentialError::Revoked {
                        provider: route.provider.clone(),
                    });
                }
                RefreshVerdict::ReauthorizationRequired { detail } => {
                    return Err(CredentialError::ReauthorizationRequired {
                        provider: route.provider.clone(),
                        detail,
                    });
                }
                RefreshVerdict::Transient { detail } => {
                    return Err(CredentialError::Transient {
                        provider: route.provider.clone(),
                        detail,
                    });
                }
                RefreshVerdict::NotRefreshable => {
                    // Unreachable (a flight only starts for OAuth material);
                    // loud if the invariant ever breaks.
                    return Err(CredentialError::Transient {
                        provider: route.provider.clone(),
                        detail: "refresh flight returned NotRefreshable for OAuth material"
                            .to_string(),
                    });
                }
            }
        }
    }

    async fn report_unauthorized_impl(
        &self,
        route: &ResolvedModelRoute,
        used: &ApplicableAuth,
    ) -> Result<RefreshVerdict, CredentialError> {
        let cell = self.cell_for(&route.provider)?;
        let flight = {
            let mut guard = cell.lock().await;
            // R05 RR1 F02: a 401 arriving on a route older than the
            // current credential seed epoch belongs to a configuration
            // world that was replaced mid-call. It must not trigger (or
            // join) a refresh FOR THAT DEAD WORLD, and the bounded retry
            // that follows must not place newer material on the older
            // route — refuse here; the caller settles the safe failure.
            if guard.seed_epoch > route.config_generation {
                return Err(CredentialError::StaleRoute {
                    provider: route.provider.clone(),
                    route_generation: route.config_generation,
                    seed_epoch: guard.seed_epoch,
                });
            }
            match &guard.material {
                ProviderMaterial::Revoked => return Ok(RefreshVerdict::Revoked),
                ProviderMaterial::None => return Ok(RefreshVerdict::NotRefreshable),
                ProviderMaterial::Static(current) => {
                    if current != used {
                        // Someone already rotated the static credential
                        // (a config reload landed): the incumbent
                        // force-refresh semantics — reuse the active
                        // material, never burn anything.
                        return Ok(RefreshVerdict::Refreshed {
                            generation: guard.generation,
                            persisted: guard.persisted,
                        });
                    }
                    return Ok(RefreshVerdict::NotRefreshable);
                }
                ProviderMaterial::OAuth(state) => {
                    let Some(tokens) = &state.tokens else {
                        return Err(CredentialError::NotLoggedIn {
                            provider: route.provider.clone(),
                        });
                    };
                    let still_current = match used {
                        ApplicableAuth::Bearer(token) => token == &tokens.access_token,
                        // The material kind moved under the caller (a reload
                        // swapped the shape) — that IS a rotation.
                        _ => false,
                    };
                    if !still_current {
                        // Someone else already refreshed: reuse, never burn
                        // the fresh refresh token (the incumbent
                        // force-refresh rule).
                        return Ok(RefreshVerdict::Refreshed {
                            generation: guard.generation,
                            persisted: guard.persisted,
                        });
                    }
                    let flow = state.flow.clone();
                    let refresh_token = tokens.refresh_token.clone();
                    self.join_or_start_flight(
                        &route.provider,
                        &cell,
                        &mut guard,
                        &flow,
                        &refresh_token,
                    )
                }
            }
        };
        Ok(self.wait_flight(flight).await)
    }

    /// Revokes one provider's credential (C05): the OAuth token row is
    /// deleted from the store FIRST (a persist failure keeps the memory
    /// state and reports honestly), then the cell flips to `Revoked` with a
    /// generation bump — an in-flight refresh's write-back is fenced by the
    /// bump, its minted token discarded, never persisted.
    pub async fn revoke(&self, provider: &str) -> Result<(), CredentialError> {
        let cell = self.cell_for(provider)?;
        let mut guard = cell.lock().await;
        // R05 RR1 F04: a revocation also kills any pending login
        // transaction (a late login completing against a revoked
        // credential must never install).
        if let Some(pending) = self.take_login(provider) {
            pending.cancel.cancel();
        }
        let has_stored_tokens =
            matches!(&guard.material, ProviderMaterial::OAuth(state) if state.tokens.is_some());
        if has_stored_tokens {
            if let Some(store) = &self.inner.store {
                store.remove_tokens(provider).map_err(|detail| {
                    CredentialError::PersistenceFailed {
                        provider: provider.to_string(),
                        detail,
                    }
                })?;
            }
        }
        guard.material = ProviderMaterial::Revoked;
        guard.generation += 1;
        guard.persisted = true;
        guard.last_error = None;
        // New joins stop here; in-flight waiters settle with `Discarded`
        // (→ the Revoked verdict) when the flight's bounded task finishes.
        guard.flight = None;
        Ok(())
    }

    /// Re-seeds the credential state from a freshly validated config (the
    /// management reload surface re-reads the SAME source for the gateway
    /// and this service — one consistent plane). Cells whose seed did not
    /// change keep their material, generation, tokens and in-flight flight
    /// (and their seed epoch — the material is unchanged, so it is still
    /// the current generation's material); a changed/added provider gets a
    /// fresh cell (OAuth tokens re-seeded from the store) stamped with
    /// `config_generation` — the model-plane generation the installing
    /// reload publishes (R05 RR1 F02: the caller passes the SAME number it
    /// hands the gateway, so `resolve` can refuse routes older than the
    /// material's world); a removed provider's cell is dropped (its
    /// in-flight flight's write-back is fenced by the map-currency
    /// check).
    pub async fn reload(&self, config: &ModelPlaneConfig, config_generation: u64) {
        // Snapshot the current map (Arc clones) and RELEASE the std read
        // guard before any await — a std guard held across an await is not
        // Send, and the handler future must stay Send.
        let current: BTreeMap<String, Arc<tokio::sync::Mutex<ProviderCell>>> = self
            .inner
            .providers
            .read()
            .expect("credential providers lock")
            .iter()
            .map(|(id, cell)| (id.clone(), Arc::clone(cell)))
            .collect();
        let mut next = BTreeMap::new();
        for (id, provider) in &config.providers {
            let kept = match current.get(id) {
                Some(cell) => {
                    let guard = cell.lock().await;
                    if guard.seed == provider.auth {
                        Some(Arc::clone(cell))
                    } else {
                        None
                    }
                }
                None => None,
            };
            let cell = kept.unwrap_or_else(|| {
                // R05 RR1 F03: every fresh cell draws a NEVER-reused
                // instance number — the identity that makes a stale
                // handle/login/flight unrevivable against a recreated
                // provider, whatever its generation counter says.
                let instance = self
                    .inner
                    .next_instance
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Arc::new(tokio::sync::Mutex::new(seed_cell(
                    id,
                    &provider.auth,
                    self.inner.store.as_ref(),
                    config_generation,
                    instance,
                )))
            });
            next.insert(id.clone(), cell);
        }
        *self
            .inner
            .providers
            .write()
            .expect("credential providers lock") = next;
        // A provider that left the plane takes its pending login
        // transaction with it (the transaction's cell instance can never
        // match again — the fence does the work; this keeps the map tidy).
        let configured: Vec<String> = config.providers.keys().cloned().collect();
        self.inner
            .logins
            .lock()
            .expect("credential logins lock")
            .retain(|provider, _| configured.contains(provider));
        // R05 RR1 F04: the config-bound model listing follows the plane.
        *self
            .inner
            .config_models
            .write()
            .expect("credential config models lock") = config_models_of(config);
    }

    /// The material-free status snapshot of every provider (management
    /// surface — C06/C09: this is where `persisted: false` and
    /// `last_persist_failure` are reported honestly).
    pub async fn status(&self) -> Vec<ProviderCredentialStatus> {
        let cells: Vec<(String, Arc<tokio::sync::Mutex<ProviderCell>>)> = self
            .inner
            .providers
            .read()
            .expect("credential providers lock")
            .iter()
            .map(|(id, cell)| (id.clone(), Arc::clone(cell)))
            .collect();
        let mut out = Vec::with_capacity(cells.len());
        for (provider, cell) in cells {
            let guard = cell.lock().await;
            let (kind, state, expires_at_unix_ms) = match &guard.material {
                ProviderMaterial::None => (CredentialAuthKind::None, "keyless", None),
                ProviderMaterial::Static(auth) => (
                    match auth {
                        ApplicableAuth::Bearer(_) => CredentialAuthKind::ApiKey,
                        ApplicableAuth::Header { .. } => CredentialAuthKind::AuthHeader,
                        ApplicableAuth::None => CredentialAuthKind::None,
                    },
                    "ready",
                    None,
                ),
                ProviderMaterial::OAuth(state) => (
                    CredentialAuthKind::OAuth,
                    if state.tokens.is_some() {
                        "ready"
                    } else {
                        "not_logged_in"
                    },
                    state
                        .tokens
                        .as_ref()
                        .map(|tokens| tokens.expires_at_unix_ms),
                ),
                ProviderMaterial::Revoked => (guard.seed.kind(), "revoked", None),
            };
            // R05 RR1 F04 (LA-CA0BF9A7AEA9): loggedIn + the available
            // model count (the config-bound ∪ custom registry, only while
            // logged in).
            let (logged_in, available_models) = if kind == CredentialAuthKind::OAuth {
                let logged_in = matches!(
                    &guard.material,
                    ProviderMaterial::OAuth(state) if state.tokens.is_some()
                );
                let count = if logged_in {
                    self.available_model_ids(&provider).len()
                } else {
                    0
                };
                (logged_in, count)
            } else {
                (false, 0)
            };
            out.push(ProviderCredentialStatus {
                provider,
                kind: auth_kind_name(kind),
                state: state.to_string(),
                generation: guard.generation,
                expires_at_unix_ms,
                refresh_in_flight: guard.flight.is_some(),
                persisted: guard.persisted,
                last_persist_failure: guard.last_persist_failure.clone(),
                last_error: guard.last_error.clone(),
                last_refresh_at_unix_ms: guard.last_refresh_at_unix_ms,
                logged_in,
                available_models,
            });
        }
        out
    }

    /// R05 RR1 F04: the deduplicated (config-bound ∪ custom registry)
    /// model ids of one provider. Lock order: cell (held by the caller) →
    /// config_models/store — the same order `install_tokens`/`revoke`
    /// use, so no cycle is possible.
    fn available_model_ids(&self, provider: &str) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        let config_bound = self
            .inner
            .config_models
            .read()
            .expect("credential config models lock")
            .get(provider)
            .cloned()
            .unwrap_or_default();
        let custom = self
            .inner
            .store
            .as_ref()
            .map(|store| store.custom_models_for(provider))
            .unwrap_or_default();
        config_bound
            .into_iter()
            .chain(custom)
            .filter(|model| seen.insert(model.clone()))
            .collect()
    }

    /// Mints a credential handle for one provider (C12). An unknown
    /// provider id is refused (a forged provider reference never mints).
    /// R05 RR1 F03: the handle is bound to the MINTING PRINCIPAL and to
    /// the cell's never-reused INSTANCE identity (in addition to the
    /// generation) — a handle minted before a rotation, a revocation, or a
    /// provider remove→recreate can never resolve again, and no other
    /// subject can present it.
    pub async fn mint_handle(
        &self,
        principal: &str,
        provider: &str,
    ) -> Result<CredentialHandle, CredentialError> {
        let cell = self.cell_for(provider)?;
        let (instance, generation) = {
            let guard = cell.lock().await;
            (guard.instance, guard.generation)
        };
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes).map_err(|err| CredentialError::Transient {
            provider: provider.to_string(),
            detail: format!("system CSPRNG unavailable for a credential handle: {err}"),
        })?;
        use base64::Engine as _;
        let handle_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let expires_at_unix_ms = self.inner.clock.now_unix_ms() + CREDENTIAL_HANDLE_TTL_MS;
        {
            let mut handles = self.inner.handles.lock().expect("credential handles lock");
            let now = self.inner.clock.now_unix_ms();
            handles.retain(|_, entry| entry.expires_at_unix_ms > now);
            handles.insert(
                handle_id.clone(),
                HandleEntry {
                    principal: principal.to_string(),
                    provider: provider.to_string(),
                    instance,
                    generation,
                    expires_at_unix_ms,
                },
            );
        }
        Ok(CredentialHandle {
            handle_id,
            provider: provider.to_string(),
            expires_at_unix_ms,
        })
    }

    /// Resolves a handle to the CURRENT material. The registry is the
    /// authority: an unknown id, a provider mismatch, an expired handle, a
    /// presentation by any principal other than the minting one, or a
    /// stale instance/generation (the credential rotated, was revoked, or
    /// the provider was re-seeded/recreated since the mint) are all loud
    /// refusals (C12, R05 RR1 F03). A handle resolution NEVER triggers a
    /// refresh — it returns the current material or refuses.
    pub async fn resolve_handle(
        &self,
        principal: &str,
        handle: &CredentialHandle,
    ) -> Result<ApplicableAuth, CredentialError> {
        let entry = {
            let handles = self.inner.handles.lock().expect("credential handles lock");
            handles.get(&handle.handle_id).cloned().ok_or_else(|| {
                CredentialError::HandleRefused {
                    provider: handle.provider.clone(),
                    detail: "unknown handle id".to_string(),
                }
            })?
        };
        let refused = |detail: &str| CredentialError::HandleRefused {
            provider: handle.provider.clone(),
            detail: detail.to_string(),
        };
        if entry.provider != handle.provider {
            return Err(refused("the handle is bound to a different provider"));
        }
        if handle.expires_at_unix_ms != entry.expires_at_unix_ms {
            // The registry is the authority: a tampered expiry never
            // extends (or rewrites) a handle.
            return Err(refused("the handle's expiry does not match the registry"));
        }
        if entry.principal != principal {
            return Err(refused(
                "the handle was minted for a different principal (cross-subject presentation)",
            ));
        }
        let now = self.inner.clock.now_unix_ms();
        if now >= entry.expires_at_unix_ms {
            return Err(refused("the handle has expired"));
        }
        let cell = self.cell_for(&entry.provider)?;
        let guard = cell.lock().await;
        if guard.instance != entry.instance {
            // R05 RR1 F03: the provider's credential world was REPLACED (a
            // re-seeding reload, or a remove→recreate) — the new cell's
            // numerically-equal generation must never revive this handle.
            return Err(refused(
                "the provider's credential was replaced since the handle was minted (stale \
                 instance)",
            ));
        }
        if guard.generation != entry.generation {
            return Err(refused(
                "the credential rotated since the handle was minted (stale generation)",
            ));
        }
        match &guard.material {
            ProviderMaterial::None => Ok(ApplicableAuth::None),
            ProviderMaterial::Static(auth) => Ok(auth.clone()),
            ProviderMaterial::Revoked => Err(CredentialError::Revoked {
                provider: entry.provider.clone(),
            }),
            ProviderMaterial::OAuth(state) => match &state.tokens {
                None => Err(CredentialError::NotLoggedIn {
                    provider: entry.provider.clone(),
                }),
                Some(tokens) if self.is_expired(tokens) => Err(refused(
                    "the credential expired after the handle was minted; re-resolve through \
                     the credential port",
                )),
                Some(tokens) => Ok(ApplicableAuth::Bearer(tokens.access_token.clone())),
            },
        }
    }
}

impl ProviderCredentialPort for CredentialService {
    fn resolve<'a>(
        &'a self,
        route: &'a ResolvedModelRoute,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ApplicableAuth, CredentialError>> + Send + 'a>,
    > {
        Box::pin(async move { self.resolve_impl(route).await })
    }

    fn report_unauthorized<'a>(
        &'a self,
        route: &'a ResolvedModelRoute,
        used: &'a ApplicableAuth,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<RefreshVerdict, CredentialError>> + Send + 'a>,
    > {
        Box::pin(async move { self.report_unauthorized_impl(route, used).await })
    }
}

/// The wire name of a credential kind (status surface; the kernel enum
/// deliberately stays serialization-free).
fn auth_kind_name(kind: CredentialAuthKind) -> &'static str {
    match kind {
        CredentialAuthKind::ApiKey => "apiKey",
        CredentialAuthKind::AuthHeader => "authHeader",
        CredentialAuthKind::OAuth => "oauth",
        CredentialAuthKind::None => "none",
    }
}

/// Builds a fresh cell of one provider from its validated config seed (and
/// the store, for OAuth). `seed_epoch` is the model-plane configuration
/// generation this seed belongs to (R05 RR1 F02 — 1 at construction, the
/// installing reload's generation afterwards); `instance` is the
/// never-reused cell identity (R05 RR1 F03).
fn seed_cell(
    provider: &str,
    auth: &AuthConfig,
    store: Option<&CredentialStore>,
    seed_epoch: u64,
    instance: u64,
) -> ProviderCell {
    let material = match auth {
        AuthConfig::None => ProviderMaterial::None,
        AuthConfig::ApiKey { api_key } => {
            ProviderMaterial::Static(ApplicableAuth::Bearer(api_key.clone()))
        }
        AuthConfig::AuthHeader { header, value } => {
            ProviderMaterial::Static(ApplicableAuth::Header {
                name: header.clone(),
                value: value.clone(),
            })
        }
        AuthConfig::OAuth(flow) => ProviderMaterial::OAuth(OAuthState {
            flow: flow.clone(),
            tokens: store.and_then(|store| store.tokens_for(provider)),
        }),
    };
    ProviderCell {
        instance,
        seed: auth.clone(),
        seed_epoch,
        generation: 1,
        material,
        flight: None,
        persisted: true,
        last_persist_failure: None,
        last_error: None,
        last_refresh_at_unix_ms: None,
    }
}

/// R05 RR1 F04: the config-bound (provider → model ids) listing of a plane
/// — every operation slot's binding contributes its (provider, model) pair.
fn config_models_of(config: &ModelPlaneConfig) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut push = |binding: &Option<lingxi_adapters::models::config::RouteBinding>| {
        if let Some(binding) = binding {
            out.entry(binding.provider.clone())
                .or_default()
                .push(binding.model.clone());
        }
    };
    let models = &config.models;
    push(&models.chat);
    push(&models.title);
    push(&models.summarize);
    push(&models.memory);
    push(&models.vision);
    push(&models.approval);
    push(&models.guard);
    push(&models.embedding);
    push(&models.rerank);
    push(&models.image);
    push(&models.video);
    push(&models.speech);
    push(&models.speech_recognition);
    out
}

/// Maps a refresh-transport failure onto the flight outcome vocabulary.
/// The details arrive already scrubbed of the in-play refresh token (the
/// OAuth module scrubs at the production site — C09).
fn classify_refresh_error(error: &OAuthError) -> FlightOutcome {
    match error {
        OAuthError::Transient { detail } => FlightOutcome::Transient {
            detail: detail.clone(),
        },
        OAuthError::ReauthorizationRequired { detail } | OAuthError::Expired { detail } => {
            FlightOutcome::ReauthorizationRequired {
                detail: detail.clone(),
            }
        }
        // A protocol violation on the token endpoint is terminal for this
        // grant (never an auto-retry loop against a misbehaving server —
        // C07): the honest classification is re-authorization.
        OAuthError::Protocol { detail } | OAuthError::StateRejected { detail } => {
            FlightOutcome::ReauthorizationRequired {
                detail: format!("token endpoint protocol violation: {detail}"),
            }
        }
        OAuthError::Cancelled => FlightOutcome::Transient {
            detail: "the refresh transport was cancelled".to_string(),
        },
    }
}

/// The refresh flight body (spawned — owned by the runtime, NOT by any
/// waiter, so the last waiter's cancellation never aborts a minted-but-
/// unstored token; the driver's request timeout bounds the task).
#[allow(clippy::too_many_arguments)]
async fn run_refresh_flight(
    inner: Arc<CredentialServiceInner>,
    provider: String,
    cell: Arc<tokio::sync::Mutex<ProviderCell>>,
    flow: OAuthFlowConfig,
    refresh_token: String,
    start_instance: u64,
    start_generation: u64,
    sender: tokio::sync::watch::Sender<Option<FlightOutcome>>,
) {
    let outcome = match inner
        .refresh
        .refresh(&provider, &flow, &refresh_token)
        .await
    {
        Ok(tokens) => {
            install_tokens(
                &inner,
                &provider,
                &cell,
                tokens,
                start_instance,
                start_generation,
            )
            .await
        }
        Err(error) => {
            let outcome = classify_refresh_error(&error);
            let mut guard = cell.lock().await;
            guard.last_error = Some(match &outcome {
                FlightOutcome::ReauthorizationRequired { detail }
                | FlightOutcome::Transient { detail } => detail.clone(),
                _ => error.to_string(),
            });
            // Only clear the flight slot when it is still THIS flight's
            // cell generation (a revoke/reload may have moved on).
            if guard.generation == start_generation {
                guard.flight = None;
            }
            outcome
        }
    };
    // No receiver left is fine — the write-back already happened (or was
    // fenced); the verdict slot exists for whoever still waits.
    let _ = sender.send(Some(outcome));
}

/// Installs a minted token set: fence → persist → memory (C05/C06).
/// R05 RR1 F03: the fence is (instance, generation) — the write-back is
/// discarded unless the cell is STILL the very instance the flight (or
/// login transaction) started against, at the very generation it left.
async fn install_tokens(
    inner: &Arc<CredentialServiceInner>,
    provider: &str,
    cell: &Arc<tokio::sync::Mutex<ProviderCell>>,
    tokens: OAuthTokens,
    start_instance: u64,
    start_generation: u64,
) -> FlightOutcome {
    // Lock order is cell → store EVERYWHERE (revoke takes the same order),
    // so the fences below and the persist cannot interleave with a revoke's
    // remove+flip.
    let mut guard = cell.lock().await;
    // Fence 1 (reload), authoritative INSIDE the cell lock (REV-T02 R01
    // F-02): the cell must still be the CURRENT cell of the provider — a
    // reload that dropped/replaced it orphans this flight. Checking map
    // currency BEFORE the cell lock left a window for the reload to swap
    // the cell in between (fence 1 passed on the stale check, fence 2 then
    // ran against the orphaned cell's untouched generation); re-checking
    // here closes it. Deadlock note: nothing ever waits on a cell lock
    // while holding the providers map lock (reload snapshots and releases
    // the map before locking cells; its write section swaps the map
    // without touching cells), so this brief std read lock inside the cell
    // lock cannot cycle.
    let still_current = inner
        .providers
        .read()
        .expect("credential providers lock")
        .get(provider)
        .is_some_and(|current| Arc::ptr_eq(current, cell));
    if !still_current {
        return FlightOutcome::Discarded;
    }
    if guard.instance != start_instance {
        // Fence 1b (R05 RR1 F03): the cell the flight started against was
        // REPLACED by a later re-seed/recreate that landed on this same
        // Arc-bearing map slot — the instance identity is the tiebreaker.
        return FlightOutcome::Discarded;
    }
    if guard.generation != start_generation {
        // Fence 2 (revoke/racing install): the credential moved while the
        // transport ran — the late token is discarded, never written back.
        return FlightOutcome::Discarded;
    }
    let flow = match &guard.material {
        ProviderMaterial::OAuth(state) => state.flow.clone(),
        // R05 RR1 F04: a LOGIN may re-mint a revoked OAuth provider (the
        // C05 doc rule: "a login re-mints; the store row was deleted at
        // revoke time") — the flow comes from the seed. This arm is
        // reachable ONLY for logins that started AFTER the revoke (the
        // generation fence above discards anything older; a refresh flight
        // can never start from revoked material), so it cannot resurrect a
        // fenced credential.
        ProviderMaterial::Revoked => match &guard.seed {
            AuthConfig::OAuth(flow) => flow.clone(),
            _ => return FlightOutcome::Discarded,
        },
        // Every other material kind without a generation change is
        // impossible (each change bumps the generation) — never trust it
        // blindly.
        _ => return FlightOutcome::Discarded,
    };
    // Persist FIRST (C06): the durable record leads the memory.
    let (persisted, persist_failure) = match &inner.store {
        Some(store) => match store.put_tokens(provider, &tokens) {
            Ok(()) => (true, None),
            Err(detail) => (false, Some(detail)),
        },
        // An OAuth provider without a store is a construction bug; honest:
        // the token is usable in memory, reported as NOT durable.
        None => (
            false,
            Some("no credential store configured for an OAuth provider".to_string()),
        ),
    };
    guard.material = ProviderMaterial::OAuth(OAuthState {
        flow,
        tokens: Some(tokens),
    });
    guard.generation += 1;
    guard.persisted = persisted;
    guard.last_persist_failure = persist_failure;
    guard.last_error = None;
    guard.last_refresh_at_unix_ms = Some(inner.clock.now_unix_ms());
    guard.flight = None;
    FlightOutcome::Installed {
        generation: guard.generation,
        persisted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inject::ManualClock;

    struct ScriptedRefresh {
        calls: StdMutex<Vec<String>>,
        outcome: StdMutex<Result<OAuthTokens, OAuthError>>,
        gate: Option<Arc<tokio::sync::Notify>>,
    }

    impl ScriptedRefresh {
        fn ok(access: &str) -> Self {
            Self {
                calls: StdMutex::new(Vec::new()),
                outcome: StdMutex::new(Ok(OAuthTokens {
                    access_token: access.to_string(),
                    refresh_token: "rt-next".to_string(),
                    expires_at_unix_ms: u64::MAX,
                })),
                gate: None,
            }
        }
        fn recorded(&self) -> Vec<String> {
            self.calls.lock().expect("calls").clone()
        }
    }

    impl RefreshDriver for ScriptedRefresh {
        fn refresh<'a>(
            &'a self,
            provider: &'a str,
            _flow: &'a OAuthFlowConfig,
            refresh_token: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<OAuthTokens, OAuthError>> + Send + 'a>,
        > {
            self.calls
                .lock()
                .expect("calls")
                .push(format!("{provider}:{refresh_token}"));
            let gate = self.gate.clone();
            Box::pin(async move {
                if let Some(gate) = gate {
                    gate.notified().await;
                }
                self.outcome.lock().expect("outcome").clone()
            })
        }
    }

    fn oauth_config() -> ModelPlaneConfig {
        ModelPlaneConfig::parse_and_validate(
            r#"{
                "providers": {
                    "main": {
                        "protocol": "openai-completions",
                        "endpoint": "http://127.0.0.1:9/v1",
                        "auth": {"kind": "oauth", "flow": "deviceCode",
                            "clientId": "client-1",
                            "tokenEndpoint": "http://127.0.0.1:9/token",
                            "deviceAuthorizationEndpoint": "http://127.0.0.1:9/device"}
                    }
                },
                "models": {"chat": {"provider": "main", "model": "m-1"}}
            }"#,
        )
        .expect("valid")
    }

    fn static_config(key: &str) -> ModelPlaneConfig {
        ModelPlaneConfig::parse_and_validate(&format!(
            r#"{{
                "providers": {{
                    "main": {{
                        "protocol": "openai-completions",
                        "endpoint": "http://127.0.0.1:9/v1",
                        "auth": {{"kind": "apiKey", "apiKey": {key}}}
                    }}
                }},
                "models": {{"chat": {{"provider": "main", "model": "m-1"}}}}
            }}"#,
            key = serde_json::to_string(key).expect("json"),
        ))
        .expect("valid")
    }

    fn route_for(provider: &str) -> ResolvedModelRoute {
        ResolvedModelRoute {
            provider: provider.to_string(),
            model: "m-1".to_string(),
            operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
            protocol: lingxi_kernel::model_exchange::ProtocolFamily::OpenAiCompletions,
            endpoint: "http://127.0.0.1:9/v1".to_string(),
            credential: lingxi_kernel::model_exchange::CredentialReference {
                provider: provider.to_string(),
                auth: CredentialAuthKind::OAuth,
            },
            config_generation: 1,
            group_id: None,
        }
    }

    struct MemoryStoreIo {
        content: StdMutex<Option<String>>,
    }

    impl store::StoreIo for MemoryStoreIo {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(self.content.lock().expect("mem").clone())
        }
        fn write(&self, content: &str) -> Result<(), String> {
            *self.content.lock().expect("mem") = Some(content.to_string());
            Ok(())
        }
    }

    fn store_with_tokens(
        provider: &str,
        access: &str,
        refresh: &str,
        expires_at: u64,
    ) -> CredentialStore {
        let store = CredentialStore::load(Arc::new(MemoryStoreIo {
            content: StdMutex::new(None),
        }))
        .expect("load");
        store
            .put_tokens(
                provider,
                &OAuthTokens {
                    access_token: access.to_string(),
                    refresh_token: refresh.to_string(),
                    expires_at_unix_ms: expires_at,
                },
            )
            .expect("seed");
        store
    }

    #[tokio::test]
    async fn concurrent_401s_merge_into_one_refresh() {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let driver = Arc::new(ScriptedRefresh::ok("at-fresh"));
        let service = CredentialService::new(
            &oauth_config(),
            Some(store_with_tokens("main", "at-old", "rt-1", 1_000_000_000)),
            driver.clone(),
            clock,
        );
        let route = route_for("main");
        let used = ApplicableAuth::Bearer("at-old".to_string());
        let (a, b) = tokio::join!(
            service.report_unauthorized(&route, &used),
            service.report_unauthorized(&route, &used),
        );
        assert!(matches!(
            a,
            Ok(RefreshVerdict::Refreshed {
                persisted: true,
                ..
            })
        ));
        assert!(matches!(b, Ok(RefreshVerdict::Refreshed { .. })));
        assert_eq!(
            driver.recorded().len(),
            1,
            "one refresh for N concurrent 401s"
        );
        // The fresh token is what a later resolve returns.
        assert_eq!(
            service.resolve(&route).await.expect("resolved"),
            ApplicableAuth::Bearer("at-fresh".to_string())
        );
    }

    #[tokio::test]
    async fn a_rotated_credential_is_reused_without_a_second_refresh() {
        // The incumbent force-refresh rule: the stored token already moved
        // past the rejected one — someone else rotated; reuse, never burn
        // the fresh refresh token.
        let clock = Arc::new(ManualClock::new(1_000_000));
        let driver = Arc::new(ScriptedRefresh::ok("at-never"));
        let service = CredentialService::new(
            &oauth_config(),
            Some(store_with_tokens("main", "at-newer", "rt-1", 1_000_000_000)),
            driver.clone(),
            clock,
        );
        let route = route_for("main");
        let stale = ApplicableAuth::Bearer("at-ancient".to_string());
        let verdict = service
            .report_unauthorized(&route, &stale)
            .await
            .expect("verdict");
        assert!(matches!(verdict, RefreshVerdict::Refreshed { .. }));
        assert!(driver.recorded().is_empty(), "no refresh execution at all");
    }

    #[tokio::test]
    async fn revoke_fences_a_late_writeback() {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let gate = Arc::new(tokio::sync::Notify::new());
        let driver = Arc::new(ScriptedRefresh {
            calls: StdMutex::new(Vec::new()),
            outcome: StdMutex::new(Ok(OAuthTokens {
                access_token: "at-late".to_string(),
                refresh_token: "rt-late".to_string(),
                expires_at_unix_ms: u64::MAX,
            })),
            gate: Some(gate.clone()),
        });
        let store = store_with_tokens("main", "at-old", "rt-1", 1_000_000_000);
        let service = CredentialService::new(&oauth_config(), Some(store.clone()), driver, clock);
        let route = route_for("main");
        let used = ApplicableAuth::Bearer("at-old".to_string());
        let waiter = tokio::spawn({
            let service = service.clone();
            let route = route.clone();
            async move { service.report_unauthorized(&route, &used).await }
        });
        // Let the flight start, then revoke mid-flight.
        tokio::task::yield_now().await;
        service.revoke("main").await.expect("revoke");
        gate.notify_waiters();
        let verdict = waiter.await.expect("joined").expect("verdict");
        assert!(matches!(verdict, RefreshVerdict::Revoked));
        // The late token never landed — not in memory, not in the store.
        assert!(matches!(
            service.resolve(&route).await,
            Err(CredentialError::Revoked { .. })
        ));
        assert!(store.tokens_for("main").is_none());
    }

    #[tokio::test]
    async fn a_cancelled_waiter_does_not_disturb_the_flight() {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let gate = Arc::new(tokio::sync::Notify::new());
        let driver = Arc::new(ScriptedRefresh {
            calls: StdMutex::new(Vec::new()),
            outcome: StdMutex::new(Ok(OAuthTokens {
                access_token: "at-fresh".to_string(),
                refresh_token: "rt-next".to_string(),
                expires_at_unix_ms: u64::MAX,
            })),
            gate: Some(gate.clone()),
        });
        let service = CredentialService::new(
            &oauth_config(),
            Some(store_with_tokens("main", "at-old", "rt-1", 1_000_000_000)),
            driver.clone(),
            clock,
        );
        let route = route_for("main");
        let used = ApplicableAuth::Bearer("at-old".to_string());
        let cancelled = tokio::spawn({
            let service = service.clone();
            let route = route.clone();
            let used = used.clone();
            async move { service.report_unauthorized(&route, &used).await }
        });
        let survivor = tokio::spawn({
            let service = service.clone();
            async move { service.report_unauthorized(&route, &used).await }
        });
        tokio::task::yield_now().await;
        cancelled.abort();
        gate.notify_waiters();
        let verdict = survivor.await.expect("joined").expect("verdict");
        assert!(matches!(verdict, RefreshVerdict::Refreshed { .. }));
        assert_eq!(driver.recorded().len(), 1);
    }

    #[tokio::test]
    async fn forged_handles_and_provider_references_are_refused() {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let driver = Arc::new(ScriptedRefresh::ok("at-x"));
        let service = CredentialService::new(&static_config("sk-live"), None, driver, clock);
        // A forged provider reference never resolves, never mints.
        assert!(matches!(
            service.resolve(&route_for("ghost")).await,
            Err(CredentialError::NotConfigured { .. })
        ));
        assert!(matches!(
            service.mint_handle("principal_test", "ghost").await,
            Err(CredentialError::NotConfigured { .. })
        ));
        let handle = service
            .mint_handle("principal_test", "main")
            .await
            .expect("minted");
        assert_eq!(
            service
                .resolve_handle("principal_test", &handle)
                .await
                .expect("resolves"),
            ApplicableAuth::Bearer("sk-live".to_string())
        );
        // A forged id is refused.
        let forged = CredentialHandle {
            handle_id: "forged".to_string(),
            ..handle.clone()
        };
        assert!(matches!(
            service.resolve_handle("principal_test", &forged).await,
            Err(CredentialError::HandleRefused { .. })
        ));
        // A cross-provider presentation is refused.
        let cross = CredentialHandle {
            provider: "ghost".to_string(),
            ..handle.clone()
        };
        assert!(matches!(
            service.resolve_handle("principal_test", &cross).await,
            Err(CredentialError::HandleRefused { .. })
        ));
        // A tampered expiry is refused.
        let tampered = CredentialHandle {
            expires_at_unix_ms: u64::MAX,
            ..handle.clone()
        };
        assert!(matches!(
            service.resolve_handle("principal_test", &tampered).await,
            Err(CredentialError::HandleRefused { .. })
        ));
    }

    #[test]
    fn handle_refusal_texts_carry_no_material() {
        let err = CredentialError::HandleRefused {
            provider: "main".to_string(),
            detail: "unknown handle id".to_string(),
        };
        let text = err.to_string();
        assert!(text.contains("main"));
        assert!(!text.contains("sk-"));
    }

    /// REV-T02 R01 F-01, trigger 1: the token endpoint keeps minting tokens
    /// whose `expires_in` truncated to 0 (expires_at == now at install). The
    /// resolve must fail loudly after exactly ONE refresh, never storm.
    #[tokio::test]
    async fn resolve_refreshes_at_most_once_when_every_mint_is_already_expired() {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let driver = Arc::new(ScriptedRefresh {
            calls: StdMutex::new(Vec::new()),
            outcome: StdMutex::new(Ok(OAuthTokens {
                access_token: "at-still-expired".to_string(),
                refresh_token: "rt-next".to_string(),
                // expires_at == now: expired the moment it lands.
                expires_at_unix_ms: 1_000_000,
            })),
            gate: None,
        });
        let service = CredentialService::new(
            &oauth_config(),
            Some(store_with_tokens("main", "at-old", "rt-1", 1_000)),
            driver.clone(),
            clock,
        );
        let result = service.resolve(&route_for("main")).await;
        assert!(
            matches!(result, Err(CredentialError::Transient { .. })),
            "a non-converging refresh is a loud terminal error for this resolve: {result:?}"
        );
        assert_eq!(
            driver.recorded().len(),
            1,
            "one refresh per resolve, never a storm"
        );
    }

    /// Mints a token valid at mint time, but the roundtrip (network + persist)
    /// outlives the one-second grant: the clock advances past the expiry
    /// before the install lands.
    struct OneSecondGrantRefresh {
        calls: StdMutex<Vec<String>>,
        clock: Arc<ManualClock>,
    }

    impl RefreshDriver for OneSecondGrantRefresh {
        fn refresh<'a>(
            &'a self,
            provider: &'a str,
            _flow: &'a OAuthFlowConfig,
            refresh_token: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<OAuthTokens, OAuthError>> + Send + 'a>,
        > {
            self.calls
                .lock()
                .expect("calls")
                .push(format!("{provider}:{refresh_token}"));
            Box::pin(async move {
                let minted_at = self.clock.now_unix_ms();
                self.clock.advance(2_000);
                Ok(OAuthTokens {
                    access_token: "at-one-second".to_string(),
                    refresh_token: "rt-next".to_string(),
                    expires_at_unix_ms: minted_at + 1_000,
                })
            })
        }
    }

    /// REV-T02 R01 F-01, trigger 2: `expires_in: 1` whose own roundtrip takes
    /// longer than a second — the re-read after the refresh finds the minted
    /// token already expired. Same single-flight bound.
    #[tokio::test]
    async fn resolve_refreshes_at_most_once_when_a_one_second_grant_dies_in_transit() {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let driver = Arc::new(OneSecondGrantRefresh {
            calls: StdMutex::new(Vec::new()),
            clock: clock.clone(),
        });
        let service = CredentialService::new(
            &oauth_config(),
            Some(store_with_tokens("main", "at-old", "rt-1", 1_000)),
            driver.clone(),
            clock,
        );
        let result = service.resolve(&route_for("main")).await;
        assert!(
            matches!(result, Err(CredentialError::Transient { .. })),
            "a grant that dies in transit is a loud terminal error: {result:?}"
        );
        assert_eq!(
            driver.calls.lock().expect("calls").len(),
            1,
            "one refresh per resolve, never a storm"
        );
    }

    /// REV-T02 R01 F-02: a reload that replaces the provider's cell while the
    /// flight is in transport discards the late write-back — the store keeps
    /// the pre-reload tokens and the NEW cell is never polluted by the late
    /// mint. (Deterministic end-state pin; the closed TOCTOU window between
    /// the two locks is argued in `install_tokens`.)
    #[tokio::test]
    async fn a_reload_replacing_the_cell_discards_the_late_writeback() {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let gate = Arc::new(tokio::sync::Notify::new());
        let driver = Arc::new(ScriptedRefresh {
            calls: StdMutex::new(Vec::new()),
            outcome: StdMutex::new(Ok(OAuthTokens {
                access_token: "at-late".to_string(),
                refresh_token: "rt-late".to_string(),
                expires_at_unix_ms: u64::MAX,
            })),
            gate: Some(gate.clone()),
        });
        let store = store_with_tokens("main", "at-old", "rt-1", 1_000);
        let service =
            CredentialService::new(&oauth_config(), Some(store.clone()), driver.clone(), clock);
        let waiter = tokio::spawn({
            let service = service.clone();
            async move { service.resolve(&route_for("main")).await }
        });
        // Let the flight start and park inside the gated transport.
        tokio::task::yield_now().await;
        // A changed seed swaps in a NEW cell mid-flight (generation 2 —
        // the plane install number the paired gateway reload publishes).
        let changed = ModelPlaneConfig::parse_and_validate(
            r#"{
                "providers": {
                    "main": {
                        "protocol": "openai-completions",
                        "endpoint": "http://127.0.0.1:9/v1",
                        "auth": {"kind": "oauth", "flow": "deviceCode",
                            "clientId": "client-2",
                            "tokenEndpoint": "http://127.0.0.1:9/token",
                            "deviceAuthorizationEndpoint": "http://127.0.0.1:9/device"}
                    }
                },
                "models": {"chat": {"provider": "main", "model": "m-1"}}
            }"#,
        )
        .expect("valid");
        service.reload(&changed, 2).await;
        gate.notify_waiters();
        let result = waiter.await.expect("joined");
        assert!(
            matches!(result, Err(CredentialError::Revoked { .. })),
            "the orphaned flight's verdict surfaces as the honest terminal state: {result:?}"
        );
        assert_eq!(driver.recorded().len(), 1, "exactly one transport ran");
        // The late mint never reached the store…
        let stored = store.tokens_for("main").expect("row still present");
        assert_eq!(stored.access_token, "at-old");
        assert_eq!(stored.refresh_token, "rt-1");
        // …and never touched the NEW cell: the re-seeded cell carries no
        // refresh timestamp and no minted material beyond the store seed.
        let status = service.status().await;
        let main = status
            .iter()
            .find(|row| row.provider == "main")
            .expect("main");
        assert_eq!(main.last_refresh_at_unix_ms, None);
    }

    /// REV-T02 R01 F-04: the time-expiry leg of handle resolution (the
    /// registry is the authority; a handle past its TTL is refused).
    #[tokio::test]
    async fn an_expired_handle_is_refused() {
        let clock = Arc::new(ManualClock::new(1_000_000));
        let driver = Arc::new(ScriptedRefresh::ok("at-x"));
        let service =
            CredentialService::new(&static_config("sk-live"), None, driver, clock.clone());
        let handle = service
            .mint_handle("principal_test", "main")
            .await
            .expect("minted");
        clock.advance(CREDENTIAL_HANDLE_TTL_MS);
        assert!(matches!(
            service.resolve_handle("principal_test", &handle).await,
            Err(CredentialError::HandleRefused { ref detail, .. }) if detail.contains("expired")
        ));
    }
}
