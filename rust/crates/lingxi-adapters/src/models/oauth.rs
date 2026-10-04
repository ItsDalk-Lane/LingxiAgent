//! The OAuth state machines of the model plane (R05-T02 C08): the two flow
//! families the incumbent product actually uses —
//!
//! - **authorization-code + PKCE with a loopback redirect callback**
//!   (openai-codex type): `begin` mints the anti-CSRF state and the S256
//!   PKCE pair, binds a 127.0.0.1 listener and renders the authorize URL;
//!   `await_callback` accepts exactly one correctly-stated callback before
//!   the state TTL; `exchange` redeems the code with the verifier.
//! - **device-code** (xai type, a faithful port of `lib/auth/xai-oauth.ts`
//!   semantics): device authorization request, then polling with
//!   `authorization_pending` / `slow_down` (+5s per slow-down), a hard
//!   `expires_in` deadline and cancellation at every wait.
//!
//! Plus `refresh` — the refresh-token grant both families share, with the
//! incumbent's classification: `invalid_grant`/401 → re-authorization
//! (terminal), 429/5xx/timeout/network → transient, missing fields → loud
//! protocol error. A missing `expires_in` falls back to the access token's
//! JWT `exp` (the incumbent's rule); a token with neither is refused.
//!
//! Every endpoint comes from the VALIDATED provider config (no discovery
//! document is fetched at runtime); every response text that can travel
//! into an error is scrubbed of in-play material (C09). The HTTP client
//! follows NO redirect (C10): an auth endpoint that redirects is a loud
//! protocol failure, never a credential replay to another origin.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;
use sha2::Digest as _;

use super::config::OAuthFlowConfig;
use super::credentials::scrub_materials;

/// One minted OAuth token set. `expires_at_unix_ms` is the LOCAL expiry
/// ledger (the server may still kill the access token earlier — the 401
/// force-refresh path exists for exactly that, see oauth-force-refresh.ts).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_unix_ms: u64,
}

impl OAuthTokens {
    /// The material strings of this token set (for exact-match scrubbing).
    pub fn materials(&self) -> Vec<&str> {
        vec![self.access_token.as_str(), self.refresh_token.as_str()]
    }
}

/// Clock/sleep/timeout injection for the flow machines (deterministic tests
/// never sleep real seconds; production wires the system clock and
/// `tokio::time::sleep`).
pub struct FlowClock<'a> {
    pub now_unix_ms: &'a (dyn Fn() -> u64 + Send + Sync),
    pub sleep: &'a (dyn Fn(Duration) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>
             + Send
             + Sync),
    /// Per-request timeout (production default: 30s, the incumbent's
    /// XAI_OAUTH_REQUEST_TIMEOUT_MS).
    pub request_timeout: Duration,
}

/// The system-clock production clock.
pub fn system_flow_clock(request_timeout: Duration) -> FlowClock<'static> {
    FlowClock {
        now_unix_ms: &|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        },
        sleep: &|duration| Box::pin(tokio::time::sleep(duration)),
        request_timeout,
    }
}

/// Cancellation probe for the long-running flows. Production bridges the
/// run/caller cancellation tree; tests drive a `tokio::sync::Notify`.
pub trait FlowCancel: Send + Sync {
    fn is_cancelled(&self) -> bool;
    fn wait<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;
}

/// The never-cancelled probe (flows without an external cancel authority).
pub struct NeverCancel;

impl FlowCancel for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
    fn wait<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(std::future::pending())
    }
}

/// OAuth flow failures. Every text is material-scrubbed at the production
/// site; classification drives the CredentialService's error vocabulary
/// (re-authorization vs transient vs protocol violation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthError {
    /// Network/timeout/5xx — the caller's bounded retry policy applies.
    Transient { detail: String },
    /// The grant is dead (`invalid_grant`, 401 on the token endpoint,
    /// `access_denied`, ...) — re-authorization required, no auto-retry.
    ReauthorizationRequired { detail: String },
    /// The device code / state deadline elapsed.
    Expired { detail: String },
    /// The auth server's response violates the protocol shape (missing
    /// fields, untrusted URL, redirect instead of tokens, ...). Loud,
    /// never guessed around.
    Protocol { detail: String },
    /// The redirect callback carried an unknown/mismatched/expired state —
    /// zero writes, the flow fails closed.
    StateRejected { detail: String },
    /// The flow was cancelled by its caller.
    Cancelled,
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OAuthError::Transient { detail } => write!(f, "oauth transport failed: {detail}"),
            OAuthError::ReauthorizationRequired { detail } => {
                write!(
                    f,
                    "oauth grant rejected (re-authorization required): {detail}"
                )
            }
            OAuthError::Expired { detail } => write!(f, "oauth flow expired: {detail}"),
            OAuthError::Protocol { detail } => write!(f, "oauth protocol violation: {detail}"),
            OAuthError::StateRejected { detail } => {
                write!(f, "oauth callback state rejected: {detail}")
            }
            OAuthError::Cancelled => write!(f, "oauth flow cancelled"),
        }
    }
}

impl std::error::Error for OAuthError {}

/// The OAuth HTTP transport: one shared client, NO redirects (C10), every
/// request bounded by the flow clock's timeout. Proxy auto-detection is
/// DISABLED (`no_proxy`, R05-T05 §25 ruling 5): the token/refresh requests
/// this client sends carry client_secret / refresh_token material, and
/// reqwest's default `system-proxy` feature would read HTTP(S)_PROXY and the
/// OS proxy settings — an ambient proxy silently terminating
/// credential-bearing traffic is a behavior regression against the incumbent
/// (the pi-sdk OAuth flows use bare `fetch()`, which never consults ambient
/// proxies) and against the model-plane discipline in
/// [`super::dispatch::build_client_with_timeouts`].
#[derive(Clone)]
pub struct OAuthHttp {
    client: reqwest::Client,
}

impl OAuthHttp {
    pub fn new() -> Result<Self, OAuthError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|err| OAuthError::Protocol {
                detail: format!("oauth http client construction failed: {err}"),
            })?;
        Ok(Self { client })
    }

    /// POSTs one `application/x-www-form-urlencoded` body and parses the
    /// JSON payload (OAuth endpoints answer JSON even on errors). The
    /// request timeout is enforced; a redirect status is a loud protocol
    /// failure (the client follows none).
    async fn post_form(
        &self,
        url: &str,
        fields: &[(&str, &str)],
        clock: &FlowClock<'_>,
        scrub: &[&str],
    ) -> Result<(u16, TokenPayload), OAuthError> {
        let send = self.client.post(url).form(fields).send();
        let response = match tokio::time::timeout(clock.request_timeout, send).await {
            Ok(Ok(response)) => response,
            Ok(Err(err)) => {
                return Err(OAuthError::Transient {
                    detail: scrub_materials(
                        &format!("request to the oauth endpoint failed: {err}"),
                        scrub,
                    ),
                });
            }
            Err(_) => {
                return Err(OAuthError::Transient {
                    detail: format!(
                        "oauth endpoint did not answer within {}ms",
                        clock.request_timeout.as_millis()
                    ),
                });
            }
        };
        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            // A redirect from a token/authorization endpoint is never
            // followed (the client carries secrets) — loud protocol failure.
            return Err(OAuthError::Protocol {
                detail: format!(
                    "oauth endpoint answered HTTP {status} redirect; redirects are never \
                     followed with credential-bearing requests (C10)"
                ),
            });
        }
        let body = match tokio::time::timeout(clock.request_timeout, response.text()).await {
            Ok(Ok(body)) => body,
            Ok(Err(err)) => {
                return Err(OAuthError::Transient {
                    detail: scrub_materials(
                        &format!("oauth response body read failed: {err}"),
                        scrub,
                    ),
                });
            }
            Err(_) => {
                return Err(OAuthError::Transient {
                    detail: "oauth endpoint body read timed out".to_string(),
                });
            }
        };
        let payload: TokenPayload =
            serde_json::from_str(&body).map_err(|err| OAuthError::Protocol {
                detail: scrub_materials(
                    &format!("oauth endpoint returned non-JSON or an invalid payload: {err}"),
                    scrub,
                ),
            })?;
        Ok((status, payload))
    }
}

#[derive(Debug, Default, Deserialize)]
struct TokenPayload {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

/// The incumbent's positive-seconds acceptance (a JSON number or a numeric
/// string, finite, > 0, and within a sane timer range).
fn positive_seconds(value: &serde_json::Value) -> Option<u64> {
    let seconds = match value {
        serde_json::Value::Number(number) => number.as_f64()?,
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if seconds.is_finite() && seconds > 0.0 && seconds <= (i32::MAX as f64) {
        Some(seconds as u64)
    } else {
        None
    }
}

/// The JWT `exp` fallback of the incumbent (`jwtExpiryMilliseconds`): when
/// the token response omits `expires_in`, the access token's own expiry is
/// the ledger. Never fails open: an unparsable/expired claim is None.
fn jwt_expiry_unix_ms(access_token: &str) -> Option<u64> {
    let mut parts = access_token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    let exp = value.get("exp")?.as_f64()?;
    if exp.is_finite() && exp > 0.0 {
        Some((exp * 1000.0) as u64)
    } else {
        None
    }
}

/// Builds the token set of a successful token response. `previous_refresh`
/// is the incumbent's refresh rule: a refresh response WITHOUT a new
/// refresh token keeps the previous one; a LOGIN response must carry one.
fn build_tokens(
    payload: &TokenPayload,
    clock: &FlowClock<'_>,
    previous_refresh: Option<&str>,
    require_refresh: bool,
    scrub: &[&str],
) -> Result<OAuthTokens, OAuthError> {
    let access_token = match &payload.access_token {
        Some(token) if !token.is_empty() => token.clone(),
        _ => {
            return Err(OAuthError::Protocol {
                detail: "token response missing access_token".to_string(),
            });
        }
    };
    let refresh_token = match &payload.refresh_token {
        Some(token) if !token.is_empty() => token.clone(),
        _ => match previous_refresh {
            Some(previous) if !require_refresh => previous.to_string(),
            _ => {
                return Err(OAuthError::Protocol {
                    detail: if require_refresh {
                        "login token response missing refresh_token".to_string()
                    } else {
                        "refresh response has no usable refresh token".to_string()
                    },
                });
            }
        },
    };
    let now = (clock.now_unix_ms)();
    let expires_at_unix_ms = match payload.expires_in.as_ref().and_then(positive_seconds) {
        Some(seconds) => now + seconds * 1000,
        None => match jwt_expiry_unix_ms(&access_token) {
            Some(expiry) if expiry > now => expiry,
            _ => {
                return Err(OAuthError::Protocol {
                    detail: scrub_materials(
                        "token response missing a valid expiration (no expires_in, no JWT exp)",
                        scrub,
                    ),
                });
            }
        },
    };
    Ok(OAuthTokens {
        access_token,
        refresh_token,
        expires_at_unix_ms,
    })
}

/// Classifies an error token response. `error` codes come from RFC 6749 /
/// RFC 8628; the detail is scrubbed of in-play material (an auth server can
/// echo anything — C09).
fn classify_token_error(status: u16, payload: &TokenPayload, scrub: &[&str]) -> OAuthError {
    let code = payload.error.as_deref().unwrap_or("");
    let description = payload.error_description.as_deref().unwrap_or("");
    let raw = match (code.is_empty(), description.is_empty()) {
        (false, false) => format!("{code}: {description}"),
        (false, true) => code.to_string(),
        (true, false) => description.to_string(),
        (true, true) => format!("HTTP {status}"),
    };
    let detail = scrub_materials(&raw, scrub);
    match code {
        "invalid_grant"
        | "invalid_client"
        | "unauthorized_client"
        | "access_denied"
        | "authorization_denied" => OAuthError::ReauthorizationRequired { detail },
        "expired_token" => OAuthError::Expired {
            detail: format!("device code / grant expired ({detail})"),
        },
        _ => {
            if status == 401 {
                OAuthError::ReauthorizationRequired {
                    detail: format!("token endpoint answered HTTP 401 ({detail})"),
                }
            } else if status == 429 || status >= 500 {
                OAuthError::Transient {
                    detail: format!("token endpoint answered HTTP {status} ({detail})"),
                }
            } else if !code.is_empty() {
                // An unknown 4xx OAuth error: the grant is in doubt — the
                // honest classification is re-authorization, never a retry
                // loop against a terminal refusal.
                OAuthError::ReauthorizationRequired { detail }
            } else {
                OAuthError::Protocol { detail }
            }
        }
    }
}

// ── device-code flow (xai type) ─────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
struct DeviceAuthorizationPayload {
    #[serde(default)]
    device_code: Option<String>,
    #[serde(default)]
    user_code: Option<String>,
    #[serde(default)]
    verification_uri: Option<String>,
    #[serde(default)]
    verification_url: Option<String>,
    #[serde(default)]
    expires_in: Option<serde_json::Value>,
    #[serde(default)]
    interval: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

/// What the user must be shown (the driver surfaces this; the flow never
/// prints anything itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCodePrompt {
    pub user_code: String,
    pub verification_uri: String,
    pub interval: Duration,
    pub expires_in: Duration,
}

/// The verification URI rule (mirror of the incumbent's trusted-URL check,
/// adapted to config-rooted trust): absolute https — or absolute http on a
/// loopback host (the controlled-stand-in / local-server exception, the
/// same shape as the incumbent's local base-url exception). Never
/// userinfo-bearing, never relative.
fn validate_user_facing_uri(label: &str, uri: &str) -> Result<String, OAuthError> {
    let protocol_violation = |detail: &str| OAuthError::Protocol {
        detail: format!("{label} {uri:?} rejected: {detail}"),
    };
    let rest = if let Some(rest) = uri.strip_prefix("https://") {
        rest
    } else if let Some(rest) = uri.strip_prefix("http://") {
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host_only = host.split(':').next().unwrap_or_default();
        if !matches!(host_only, "127.0.0.1" | "localhost" | "[::1]") {
            return Err(protocol_violation(
                "plain http is only acceptable on a loopback host",
            ));
        }
        rest
    } else {
        return Err(protocol_violation(
            "scheme must be https (or loopback http)",
        ));
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() {
        return Err(protocol_violation("missing host"));
    }
    if host.contains('@') {
        return Err(protocol_violation("userinfo in URL"));
    }
    Ok(uri.to_string())
}

/// Runs the device-code login flow (xai type). `on_prompt` is invoked ONCE
/// with the user-facing code/URI before polling starts. Polling honors
/// `authorization_pending` (keep waiting), `slow_down` (+5s per step, the
/// incumbent's rule), the `expires_in` deadline and cancellation at every
/// wait; a denial is terminal. Errors never write anything anywhere (the
/// CredentialService persists only the success).
pub async fn run_device_code_flow(
    http: &OAuthHttp,
    config: &OAuthFlowConfig,
    clock: &FlowClock<'_>,
    cancel: &dyn FlowCancel,
    on_prompt: impl FnOnce(&DeviceCodePrompt),
) -> Result<OAuthTokens, OAuthError> {
    let device_endpoint = config
        .device_authorization_endpoint
        .as_deref()
        .ok_or_else(|| OAuthError::Protocol {
            detail: "deviceCode flow without deviceAuthorizationEndpoint (config validation \
                 hole — this must fail at load)"
                .to_string(),
        })?;
    if cancel.is_cancelled() {
        return Err(OAuthError::Cancelled);
    }
    let mut fields: Vec<(&str, &str)> = vec![("client_id", config.client_id.as_str())];
    if let Some(scopes) = &config.scopes {
        fields.push(("scope", scopes.as_str()));
    }
    let send = http.client.post(device_endpoint).form(&fields).send();
    let device = match tokio::time::timeout(clock.request_timeout, send).await {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => {
            return Err(OAuthError::Transient {
                detail: format!("device authorization request failed: {err}"),
            });
        }
        Err(_) => {
            return Err(OAuthError::Transient {
                detail: "device authorization request timed out".to_string(),
            });
        }
    };
    let status = device.status().as_u16();
    let body = match tokio::time::timeout(clock.request_timeout, device.text()).await {
        Ok(Ok(body)) => body,
        Ok(Err(err)) => {
            return Err(OAuthError::Transient {
                detail: format!("device authorization body read failed: {err}"),
            });
        }
        Err(_) => {
            return Err(OAuthError::Transient {
                detail: "device authorization body read timed out".to_string(),
            });
        }
    };
    let device: DeviceAuthorizationPayload =
        serde_json::from_str(&body).map_err(|err| OAuthError::Protocol {
            detail: format!("device authorization returned an invalid payload: {err}"),
        })?;
    if !(200..300).contains(&status) || device.error.is_some() {
        let payload = TokenPayload {
            error: device.error.clone(),
            error_description: device.error_description.clone(),
            ..TokenPayload::default()
        };
        return Err(classify_token_error(status, &payload, &[]));
    }
    let device_code = device
        .device_code
        .filter(|code| !code.is_empty())
        .ok_or_else(|| OAuthError::Protocol {
            detail: "device authorization response missing device_code".to_string(),
        })?;
    let user_code = device
        .user_code
        .filter(|code| !code.is_empty())
        .ok_or_else(|| OAuthError::Protocol {
            detail: "device authorization response missing user_code".to_string(),
        })?;
    let verification_uri = device
        .verification_uri
        .or(device.verification_url)
        .filter(|uri| !uri.is_empty())
        .ok_or_else(|| OAuthError::Protocol {
            detail: "device authorization response missing verification_uri".to_string(),
        })?;
    let verification_uri = validate_user_facing_uri("verification_uri", &verification_uri)?;
    let expires_in = device
        .expires_in
        .as_ref()
        .and_then(positive_seconds)
        .ok_or_else(|| OAuthError::Protocol {
            detail: "device authorization response missing a valid expires_in".to_string(),
        })?;
    let mut interval_seconds = match device.interval.as_ref() {
        None => 5, // the incumbent's default
        Some(value) => positive_seconds(value).ok_or_else(|| OAuthError::Protocol {
            detail: "device authorization response carries an invalid interval".to_string(),
        })?,
    };
    on_prompt(&DeviceCodePrompt {
        user_code,
        verification_uri,
        interval: Duration::from_secs(interval_seconds),
        expires_in: Duration::from_secs(expires_in),
    });
    let deadline = (clock.now_unix_ms)() + expires_in * 1000;
    loop {
        if cancel.is_cancelled() {
            return Err(OAuthError::Cancelled);
        }
        if (clock.now_unix_ms)() >= deadline {
            return Err(OAuthError::Expired {
                detail: format!("device code expired after {expires_in}s without authorization"),
            });
        }
        let sleeping = (clock.sleep)(Duration::from_secs(interval_seconds));
        tokio::select! {
            _ = sleeping => {}
            _ = cancel.wait() => return Err(OAuthError::Cancelled),
        }
        if (clock.now_unix_ms)() >= deadline {
            return Err(OAuthError::Expired {
                detail: format!("device code expired after {expires_in}s without authorization"),
            });
        }
        let (status, payload) = http
            .post_form(
                &config.token_endpoint,
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", device_code.as_str()),
                    ("client_id", config.client_id.as_str()),
                ],
                clock,
                &[device_code.as_str()],
            )
            .await?;
        if (200..300).contains(&status) && payload.error.is_none() {
            return build_tokens(&payload, clock, None, true, &[device_code.as_str()]);
        }
        match payload.error.as_deref() {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval_seconds =
                    interval_seconds
                        .checked_add(5)
                        .ok_or_else(|| OAuthError::Protocol {
                            detail: "polling interval overflowed after slow_down".to_string(),
                        })?;
                continue;
            }
            _ => {
                return Err(classify_token_error(
                    status,
                    &payload,
                    &[device_code.as_str()],
                ));
            }
        }
    }
}

// ── authorization-code + PKCE flow (openai-codex type) ─────────────────────

/// The anti-CSRF state TTL (pre-registered T02 limit): a pending
/// authorization is valid for 10 minutes, then the callback is rejected as
/// expired.
pub const OAUTH_STATE_TTL: Duration = Duration::from_secs(600);

/// One in-flight authorization-code+PKCE login. Single-use: a consumed
/// (successfully redeemed) or failed flow never accepts a second callback.
pub struct AuthorizationCodeFlow {
    state: String,
    verifier: String,
    redirect_uri: String,
    authorize_url: String,
    listener: tokio::net::TcpListener,
    expires_at_unix_ms: u64,
}

impl std::fmt::Debug for AuthorizationCodeFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthorizationCodeFlow")
            .field("redirect_uri", &self.redirect_uri)
            .field("authorize_url", &self.authorize_url)
            .field("expires_at_unix_ms", &self.expires_at_unix_ms)
            .finish_non_exhaustive()
    }
}

impl AuthorizationCodeFlow {
    /// Binds the loopback listener and mints the state + PKCE pair. The
    /// returned flow's `authorize_url` is what the user agent opens. Random
    /// material comes from the system CSPRNG — a failure is loud, never a
    /// fallback to a predictable source.
    pub fn begin(config: &OAuthFlowConfig, clock: &FlowClock<'_>) -> Result<Self, OAuthError> {
        let authorize_endpoint =
            config
                .authorize_endpoint
                .as_deref()
                .ok_or_else(|| OAuthError::Protocol {
                    detail: "authorizationCodePkce flow without authorizeEndpoint (config \
                         validation hole — this must fail at load)"
                        .to_string(),
                })?;
        let mut verifier_bytes = [0u8; 32];
        getrandom::getrandom(&mut verifier_bytes).map_err(|err| OAuthError::Protocol {
            detail: format!("system CSPRNG unavailable for the PKCE verifier: {err}"),
        })?;
        let mut state_bytes = [0u8; 16];
        getrandom::getrandom(&mut state_bytes).map_err(|err| OAuthError::Protocol {
            detail: format!("system CSPRNG unavailable for the oauth state: {err}"),
        })?;
        let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);
        let state = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(state_bytes);
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(sha2::Sha256::digest(verifier.as_bytes()));
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").map_err(|err| OAuthError::Transient {
                detail: format!("cannot bind the loopback callback listener: {err}"),
            })?;
        listener
            .set_nonblocking(true)
            .map_err(|err| OAuthError::Transient {
                detail: format!("cannot set the callback listener non-blocking: {err}"),
            })?;
        let listener =
            tokio::net::TcpListener::from_std(listener).map_err(|err| OAuthError::Transient {
                detail: format!("cannot adopt the callback listener: {err}"),
            })?;
        let port = listener
            .local_addr()
            .map_err(|err| OAuthError::Transient {
                detail: format!("callback listener has no local address: {err}"),
            })?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");
        let mut authorize_url = format!(
            "{authorize_endpoint}?response_type=code&client_id={}&redirect_uri={}&state={}\
             &code_challenge={}&code_challenge_method=S256",
            urlencode(&config.client_id),
            urlencode(&redirect_uri),
            urlencode(&state),
            urlencode(&challenge),
        );
        if let Some(scopes) = &config.scopes {
            authorize_url.push_str(&format!("&scope={}", urlencode(scopes)));
        }
        Ok(Self {
            state,
            verifier,
            redirect_uri,
            authorize_url,
            listener,
            expires_at_unix_ms: (clock.now_unix_ms)() + OAUTH_STATE_TTL.as_millis() as u64,
        })
    }

    /// The URL the user agent must open.
    pub fn authorize_url(&self) -> &str {
        &self.authorize_url
    }

    /// The loopback callback address (127.0.0.1:<port>).
    pub fn callback_addr(&self) -> std::net::SocketAddr {
        self.listener
            .local_addr()
            .expect("the callback listener is bound at begin()")
    }

    /// Waits for the ONE valid callback. A wrong/expired-state callback is
    /// answered with an HTTP error and produces NO write and NO token
    /// exchange; the flow keeps waiting for the valid one until the state
    /// TTL or cancellation. After the valid callback the listener is
    /// CLOSED (single-use) — a duplicate callback is refused by the closed
    /// port.
    pub async fn await_callback(
        self,
        clock: &FlowClock<'_>,
        cancel: &dyn FlowCancel,
    ) -> Result<CallbackGrant, OAuthError> {
        let listener = self.listener;
        let deadline = self.expires_at_unix_ms;
        loop {
            if cancel.is_cancelled() {
                return Err(OAuthError::Cancelled);
            }
            if (clock.now_unix_ms)() >= deadline {
                return Err(OAuthError::Expired {
                    detail: "authorization state expired before any valid callback arrived"
                        .to_string(),
                });
            }
            let socket = tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok((socket, _)) => socket,
                    Err(err) => {
                        return Err(OAuthError::Transient {
                            detail: format!("callback listener accept failed: {err}"),
                        });
                    }
                },
                _ = cancel.wait() => return Err(OAuthError::Cancelled),
            };
            // Parse the request WITHOUT answering yet: the response status
            // must reflect the state verdict.
            match read_callback_request_head(socket, clock).await {
                Ok(CallbackHead { socket, query }) => {
                    let state = query_param(&query, "state");
                    // Re-check the TTL AFTER the accept: a callback that
                    // sat in the backlog past the state TTL is rejected as
                    // expired, never honored.
                    if (clock.now_unix_ms)() >= deadline {
                        let _ = respond_callback(socket, "400 Bad Request", "Login state expired.")
                            .await;
                        return Err(OAuthError::Expired {
                            detail: "authorization state expired before the callback arrived"
                                .to_string(),
                        });
                    }
                    if state.as_deref() != Some(self.state.as_str()) {
                        // Wrong/foreign state: an HTTP error, NO write, NO
                        // exchange; the flow keeps waiting for the valid
                        // callback.
                        let _ =
                            respond_callback(socket, "400 Bad Request", "Login state mismatch.")
                                .await;
                        continue;
                    }
                    if let Some(error) = query_param(&query, "error") {
                        let _ = respond_callback(socket, "400 Bad Request", "Login failed.").await;
                        return Err(OAuthError::ReauthorizationRequired {
                            detail: format!(
                                "authorization endpoint returned an error callback: {}",
                                scrub_materials(
                                    &error,
                                    &[self.state.as_str(), self.verifier.as_str()],
                                )
                            ),
                        });
                    }
                    let Some(code) = query_param(&query, "code").filter(|code| !code.is_empty())
                    else {
                        let _ = respond_callback(
                            socket,
                            "400 Bad Request",
                            "Login callback malformed.",
                        )
                        .await;
                        return Err(OAuthError::Protocol {
                            detail: "valid-state callback without a code".to_string(),
                        });
                    };
                    let _ = respond_callback(
                        socket,
                        "200 OK",
                        "Lingxi login callback received. You can close this page.",
                    )
                    .await;
                    return Ok(CallbackGrant {
                        code,
                        verifier: self.verifier.clone(),
                        redirect_uri: self.redirect_uri.clone(),
                    });
                }
                Err(rejection) => {
                    // A malformed request on the callback port is not the
                    // callback; keep waiting (the port is public on the
                    // loopback while the flow is pending).
                    let _ = rejection;
                    continue;
                }
            };
        }
    }

    /// Exchanges the callback's code for tokens (PKCE verifier included).
    pub async fn exchange(
        http: &OAuthHttp,
        config: &OAuthFlowConfig,
        grant: &CallbackGrant,
        clock: &FlowClock<'_>,
    ) -> Result<OAuthTokens, OAuthError> {
        let scrub = [grant.verifier.as_str(), grant.code.as_str()];
        let (status, payload) = http
            .post_form(
                &config.token_endpoint,
                &[
                    ("grant_type", "authorization_code"),
                    ("code", grant.code.as_str()),
                    ("redirect_uri", grant.redirect_uri.as_str()),
                    ("client_id", config.client_id.as_str()),
                    ("code_verifier", grant.verifier.as_str()),
                ],
                clock,
                &scrub,
            )
            .await?;
        if (200..300).contains(&status) && payload.error.is_none() {
            return build_tokens(&payload, clock, None, true, &scrub);
        }
        Err(classify_token_error(status, &payload, &scrub))
    }
}

/// The accepted callback: the authorization code plus the secrets of the
/// pending flow (verifier/redirect) needed for the exchange.
pub struct CallbackGrant {
    code: String,
    verifier: String,
    redirect_uri: String,
}

/// Material-free by construction: the code and the verifier are secrets of
/// the in-flight flow and are never rendered.
impl std::fmt::Debug for CallbackGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackGrant")
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

struct CallbackHead {
    socket: tokio::net::TcpStream,
    query: String,
}

/// Reads ONE HTTP/1.x GET request head off the callback socket and returns
/// the query string of `/callback` (other paths get a 404 and are
/// rejected). The HTTP answer for a real callback is written by the caller
/// AFTER the state verdict, so a wrong/expired state is answered with an
/// error, never a success page. The read is bounded (16KB, one request
/// timeout) so a stalled peer cannot pin the flow.
async fn read_callback_request_head(
    mut socket: tokio::net::TcpStream,
    clock: &FlowClock<'_>,
) -> Result<CallbackHead, String> {
    use tokio::io::AsyncReadExt as _;
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 2048];
        let read = tokio::time::timeout(clock.request_timeout, socket.read(&mut chunk))
            .await
            .map_err(|_| "callback read timed out".to_string())?
            .map_err(|err| format!("callback read failed: {err}"))?;
        if read == 0 {
            return Err("callback connection closed before headers".to_string());
        }
        raw.extend_from_slice(&chunk[..read]);
        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        if raw.len() > 16 * 1024 {
            return Err("callback header block too large".to_string());
        }
    };
    let head = String::from_utf8(raw[..header_end].to_vec())
        .map_err(|_| "callback request is not utf-8".to_string())?;
    let request_line = head
        .split("\r\n")
        .next()
        .ok_or_else(|| "empty callback request".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method != "GET" {
        return Err(format!("callback method {method:?} is not GET"));
    }
    if target.split('?').next() != Some("/callback") {
        let _ = respond_callback(
            socket,
            "404 Not Found",
            "Not the Lingxi login callback path.",
        )
        .await;
        return Err("not the callback path".to_string());
    }
    let query = target
        .split_once('?')
        .map(|(_, query)| query.to_string())
        .unwrap_or_default();
    Ok(CallbackHead { socket, query })
}

/// Writes the minimal, material-free HTML answer of one callback request.
async fn respond_callback(
    mut socket: tokio::net::TcpStream,
    status: &str,
    note: &str,
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt as _;
    let body = format!("<!doctype html><title>Lingxi</title><p>{note}</p>");
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await
}

/// Minimal query-string param read (the callback surface: `code`, `state`,
/// `error`). Percent-decoding is applied; the LAST occurrence wins (a
/// duplicated key is the sender's protocol violation — never silently
/// merged).
fn query_param(query: &str, key: &str) -> Option<String> {
    let mut found = None;
    for pair in query.split('&') {
        if let Some((name, value)) = pair.split_once('=') {
            if name == key {
                found = Some(percent_decode(value));
            }
        }
    }
    found
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi * 16 + lo) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

// ── refresh (both flows share the refresh-token grant) ─────────────────────

/// The shared refresh-token grant (the incumbent `refreshToken`): posts the
/// refresh token, classifies the answer (invalid_grant/401 →
/// re-authorization; 429/5xx/timeout/network → transient), keeps the
/// previous refresh token when the response carries none. `NeverCancel` is
/// the refresh default (a bounded single request; the CredentialService's
/// coordination owns the wait-cancellation semantics).
pub async fn refresh(
    http: &OAuthHttp,
    config: &OAuthFlowConfig,
    refresh_token: &str,
    clock: &FlowClock<'_>,
) -> Result<OAuthTokens, OAuthError> {
    if refresh_token.is_empty() {
        return Err(OAuthError::Protocol {
            detail: "refresh without a refresh token".to_string(),
        });
    }
    let scrub = [refresh_token];
    let (status, payload) = http
        .post_form(
            &config.token_endpoint,
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", config.client_id.as_str()),
            ],
            clock,
            &scrub,
        )
        .await?;
    if (200..300).contains(&status) && payload.error.is_none() {
        return build_tokens(&payload, clock, Some(refresh_token), false, &scrub);
    }
    Err(classify_token_error(status, &payload, &scrub))
}

/// A `FlowCancel` over an `Arc<Notify>`+flag — the test/service bridge.
pub struct NotifyCancel {
    flag: std::sync::atomic::AtomicBool,
    notify: tokio::sync::Notify,
}

impl NotifyCancel {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            flag: std::sync::atomic::AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        })
    }
    pub fn cancel(&self) {
        self.flag.store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}

impl FlowCancel for NotifyCancel {
    fn is_cancelled(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn wait<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if self.is_cancelled() {
                return;
            }
            self.notify.notified().await;
        })
    }
}
