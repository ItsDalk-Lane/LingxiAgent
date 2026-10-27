//! The unified network policy of the model plane (R05 RR1 F14, T05 step 1 /
//! C12): ONE frozen policy object every outbound HTTP consumer of the model
//! plane reads — the five chat families, the operations dispatcher, the
//! egress guard's downloads and the OAuth token transport.
//!
//! The vocabulary mirrors the incumbent product's `shared/network-proxy.ts`
//! contract verbatim (the registered incumbent-parity safe harbor):
//! - `mode`: `system` (the exported HTTP(S)_PROXY/ALL_PROXY/NO_PROXY
//!   environment, snapshotted when the config loads — the incumbent's
//!   `proxyConfigFromEnvironment`; ambient env mutation mid-process is not
//!   consulted), `manual` (explicit proxy URLs + a NO_PROXY list) or
//!   `direct` (never a proxy);
//! - the NO_PROXY grammar: comma/space separated entries, `*` (everything),
//!   `*.suffix` / `.suffix` domain-suffix forms, exact hosts, optional
//!   per-entry `:port`, bracketed IPv6 hosts;
//! - FORCED local bypass: `localhost`, `::1` and `127.x.x.x` NEVER go
//!   through a proxy, whatever the lists say (the incumbent's
//!   `FORCED_LOCAL_PROXY_BYPASS` — a loopback proxy hop would move
//!   credential-bearing traffic off the machine's own stub endpoints);
//! - `trustedCaPem`: an EXPLICIT additional CA bundle (the enterprise-root
//!   case). The bundle is added ON TOP of the platform verifier's roots —
//!   the default chain/hostname/validity verification is NEVER relaxed
//!   (no `accept_invalid_certs`, no hostname bypass, no danger
//!   configuration anywhere in this module).
//!
//! Reload discipline (F02 consistency): the policy travels inside
//! [`super::config::ModelPlaneConfig`], validates at load, and a model-plane
//! reload publishes a fresh generation through [`NetworkPlane::apply`];
//! every [`NetworkClient`] rebuilds its `reqwest::Client` lazily on the next
//! request, so in-flight requests finish on the client they started with
//! and the NEXT request observes the new generation (the same
//! snapshot-then-swap shape the route table uses).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::RwLock;

use lingxi_protocol::{ErrorCode, ProtocolError};

use super::dispatch::HttpTimeouts;

/// The incumbent's default NO_PROXY list (`DEFAULT_NO_PROXY`).
pub const DEFAULT_NO_PROXY: &str = "localhost, 127.0.0.1, ::1";

/// The proxy modes (the incumbent `NETWORK_PROXY_MODES`).
pub const PROXY_MODES: [&str; 3] = ["system", "manual", "direct"];

/// Refuses with the config-error vocabulary of this module.
pub fn network_refusal(detail: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidMessage, detail, false)
}

// ── the config-side shape (serde; camelCase like every config section) ────

/// The `network.proxy` section of the model-plane config.
///
/// Validation happens in [`super::config::ModelPlaneConfig::validate`]; this
/// type only carries the DECLARED values (system mode declares no URLs —
/// the environment is read at load time on the service side).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkProxyConfig {
    /// `system` | `manual` | `direct` (the incumbent vocabulary; absent =
    /// `system`, the incumbent `DEFAULT_NETWORK_PROXY_CONFIG.mode`).
    #[serde(default = "default_proxy_mode")]
    pub mode: String,
    /// Manual mode: the proxy for plain-http targets.
    #[serde(default)]
    pub http_proxy: Option<String>,
    /// Manual mode: the proxy for https targets.
    #[serde(default)]
    pub https_proxy: Option<String>,
    /// The NO_PROXY list (comma/space separated). Default
    /// [`DEFAULT_NO_PROXY`]; loopback bypass is FORCED on top of it.
    #[serde(default)]
    pub no_proxy: Option<String>,
}

fn default_proxy_mode() -> String {
    "system".to_string()
}

impl Default for NetworkProxyConfig {
    fn default() -> Self {
        Self {
            mode: default_proxy_mode(),
            http_proxy: None,
            https_proxy: None,
            no_proxy: None,
        }
    }
}

/// The `network` section of the model-plane config (R05 RR1 F14). Absent =
/// the incumbent default (system mode, no explicit CA bundle).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkConfigSection {
    #[serde(default)]
    pub proxy: NetworkProxyConfig,
    /// An explicit additional CA bundle (PEM, possibly several
    /// certificates). Empty/absent = platform roots only. The bundle is
    /// parsed at config validation — a malformed PEM is a loud load error.
    #[serde(default)]
    pub trusted_ca_pem: Option<String>,
}

impl NetworkConfigSection {
    /// Shape validation (mode vocabulary, proxy URL forms, manual mode
    /// requiring at least one URL, PEM parseability). The incumbent's
    /// `normalizeNetworkProxyConfig(strict)` rules, verbatim in spirit:
    /// http/https/socks/socks5 schemes, a host, no path/query/fragment.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let mode = self.proxy.mode.as_str();
        if !PROXY_MODES.contains(&mode) {
            return Err(network_refusal(format!(
                "network.proxy.mode {mode:?} is not one of {PROXY_MODES:?}"
            )));
        }
        if mode != "manual" {
            return self.validate_ca();
        }
        let mut any = false;
        for (field, value) in [
            ("network.proxy.httpProxy", &self.proxy.http_proxy),
            ("network.proxy.httpsProxy", &self.proxy.https_proxy),
        ] {
            if let Some(url) = value.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
                validate_proxy_url(field, url)?;
                any = true;
            }
        }
        if !any {
            return Err(network_refusal(
                "manual network proxy requires at least one proxy URL (httpProxy or \
                 httpsProxy)"
                    .to_string(),
            ));
        }
        self.validate_ca()
    }

    fn validate_ca(&self) -> Result<(), ProtocolError> {
        if let Some(pem) = self.trusted_ca_pem.as_deref() {
            if pem.trim().is_empty() {
                return Err(network_refusal(
                    "network.trustedCaPem is present but empty; remove the key or carry a PEM \
                     bundle"
                        .to_string(),
                ));
            }
            // `from_pem_bundle` SKIPS non-PEM text — garbage yields an
            // EMPTY certificate list, not an error. An empty list is the
            // loud error here: the operator declared a CA bundle and the
            // declaration must actually carry certificates.
            let certificates =
                reqwest::Certificate::from_pem_bundle(pem.as_bytes()).map_err(|err| {
                    network_refusal(format!(
                        "network.trustedCaPem is not a parseable PEM certificate bundle: {err}"
                    ))
                })?;
            if certificates.is_empty() {
                return Err(network_refusal(
                    "network.trustedCaPem contains no PEM certificate block; a CA bundle must \
                     carry at least one CERTIFICATE"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// The incumbent `normalizeProxyUrl` rules: an absolute URL, scheme
/// http/https/socks/socks5, a host, no path (beyond `/`), no query, no
/// fragment.
fn validate_proxy_url(field: &str, url: &str) -> Result<(), ProtocolError> {
    let parsed = reqwest::Url::parse(url).map_err(|err| {
        network_refusal(format!("{field} {url:?} is not a valid proxy URL: {err}"))
    })?;
    let scheme = parsed.scheme();
    if !["http", "https", "socks", "socks5"].contains(&scheme) {
        return Err(network_refusal(format!(
            "{field} {url:?} must use http, https, socks, or socks5"
        )));
    }
    if parsed.host_str().map(str::is_empty).unwrap_or(true) {
        return Err(network_refusal(format!(
            "{field} {url:?} must include a proxy host"
        )));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(network_refusal(format!(
            "{field} {url:?} must not carry userinfo; proxy credentials are not a \
             supported configuration"
        )));
    }
    if (parsed.path() != "/" && !parsed.path().is_empty())
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(network_refusal(format!(
            "{field} {url:?} must not include a path, query, or fragment"
        )));
    }
    Ok(())
}

// ── the runtime policy ────────────────────────────────────────────────────

/// The runtime proxy policy: the DECLARED mode plus, for `system`, the
/// environment SNAPSHOT taken when the config was loaded (the incumbent
/// reads `process.env` at dispatch setup — never re-read mid-flight).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyPolicy {
    /// Never a proxy (the current pre-F14 behavior, kept as the explicit
    /// opt-out and as the isolated default of test constructors).
    Direct,
    /// The exported proxy environment, snapshotted at load.
    System { env: BTreeMap<String, String> },
    /// Explicit proxy URLs + NO_PROXY list.
    Manual {
        http: Option<String>,
        https: Option<String>,
        no_proxy: String,
    },
}

impl ProxyPolicy {
    /// Builds the runtime policy of a validated config section. `system`
    /// snapshots the CURRENT process environment (called once per
    /// config load / reload on the service side).
    pub fn from_config(config: &NetworkProxyConfig) -> Self {
        match config.mode.as_str() {
            "direct" => ProxyPolicy::Direct,
            "manual" => ProxyPolicy::Manual {
                http: normalized_declared(config.http_proxy.as_deref()),
                https: normalized_declared(config.https_proxy.as_deref()),
                no_proxy: config
                    .no_proxy
                    .clone()
                    .unwrap_or_else(|| DEFAULT_NO_PROXY.to_string()),
            },
            // The default and the explicit spelling both land here.
            _ => ProxyPolicy::System {
                env: std::env::vars().collect(),
            },
        }
    }
}

/// Trims a declared proxy URL; empty strings become `None`.
fn normalized_declared(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

/// One fully-resolved proxy set (what `effective` matching selects from).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedProxies {
    http: Option<String>,
    https: Option<String>,
    no_proxy: String,
}

impl ResolvedProxies {
    /// True when the set declares no proxy at all (direct).
    pub fn is_empty(&self) -> bool {
        self.http.is_none() && self.https.is_none()
    }
}

/// Resolves the EFFECTIVE proxy set of one policy (system snapshots derive
/// theirs from the captured environment; manual carries its own).
pub fn resolve_proxy_set(policy: &ProxyPolicy) -> ResolvedProxies {
    match policy {
        ProxyPolicy::Direct => ResolvedProxies::default(),
        ProxyPolicy::System { env } => proxies_from_environment(env),
        ProxyPolicy::Manual {
            http,
            https,
            no_proxy,
        } => ResolvedProxies {
            http: http.clone(),
            https: https.clone(),
            no_proxy: no_proxy.clone(),
        },
    }
}

/// The routing decision over one resolved set (the incumbent
/// `isNoProxyMatch` + scheme selection): forced-local bypass first, then
/// the NO_PROXY list (per-entry optional port), then scheme selection
/// (http → http||https, https → https||http). Other schemes never proxy.
/// The incumbent `envValue` precedence.
fn env_value(env: &BTreeMap<String, String>, keys: [&str; 2]) -> Option<String> {
    for key in keys {
        if let Some(value) = env.get(key).map(String::as_str).map(str::trim) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// The incumbent `proxyConfigFromEnvironment`: http from
/// HTTP_PROXY/http_proxy/ALL_PROXY/all_proxy, https from
/// HTTPS_PROXY/https_proxy/HTTP_PROXY/http_proxy/ALL_PROXY/all_proxy,
/// NO_PROXY from NO_PROXY/no_proxy. An unparseable exported URL is SKIPPED
/// (the incumbent's non-strict normalize turns it into "" — never trusted).
fn proxies_from_environment(env: &BTreeMap<String, String>) -> ResolvedProxies {
    let http = env_value(env, ["HTTP_PROXY", "http_proxy"])
        .or_else(|| env_value(env, ["ALL_PROXY", "all_proxy"]));
    let https = env_value(env, ["HTTPS_PROXY", "https_proxy"])
        .or_else(|| env_value(env, ["HTTP_PROXY", "http_proxy"]))
        .or_else(|| env_value(env, ["ALL_PROXY", "all_proxy"]));
    ResolvedProxies {
        http: http.filter(|url| reqwest::Url::parse(url).is_ok()),
        https: https.filter(|url| reqwest::Url::parse(url).is_ok()),
        no_proxy: env_value(env, ["NO_PROXY", "no_proxy"]).unwrap_or_default(),
    }
}

/// The incumbent `stripHostBrackets`.
fn strip_host_brackets(host: &str) -> String {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

/// The incumbent `isForcedLocalHost`: localhost, ::1, 127.0.0.0/8 spellings.
fn is_forced_local_host(host: &str) -> bool {
    let normalized = strip_host_brackets(host);
    if normalized == "localhost" || normalized == "::1" {
        return true;
    }
    let parts: Vec<&str> = normalized.split('.').collect();
    parts.len() == 4
        && parts[0] == "127"
        && parts[1..]
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// The incumbent `hostMatchesNoProxy` (exact, `.suffix`, `*.suffix`).
fn host_matches_no_proxy(host: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*") {
        return host.ends_with(suffix);
    }
    if let Some(suffix) = pattern.strip_prefix('.') {
        return host == suffix || host.ends_with(&format!(".{suffix}"));
    }
    host == pattern
}

/// The effective port of a parsed URL (scheme default when absent).
fn effective_url_port(url: &reqwest::Url) -> String {
    match url.port() {
        Some(port) => port.to_string(),
        None if url.scheme() == "https" => "443".to_string(),
        None => "80".to_string(),
    }
}

pub fn pick_proxy<'a>(url: &reqwest::Url, proxies: &'a ResolvedProxies) -> Option<&'a str> {
    let raw_host = url.host_str()?;
    let host = strip_host_brackets(raw_host);
    if is_forced_local_host(&host) {
        return None;
    }
    let port = effective_url_port(url);
    if no_proxy_list_matches(&host, &port, &proxies.no_proxy) {
        return None;
    }
    match url.scheme() {
        "http" => proxies.http.as_deref().or(proxies.https.as_deref()),
        "https" => proxies.https.as_deref().or(proxies.http.as_deref()),
        _ => None,
    }
}

/// Splits ONE trimmed NO_PROXY entry into its host pattern and OPTIONAL
/// per-entry port. The incumbent `splitNoProxyEntry`, ported (R05 RR1 F35):
///
/// - a bracketed IPv6 entry strips the brackets — `[2001:db8::25]` is the
///   host, `[2001:db8::25]:9443` adds a per-entry port after the closing
///   bracket (whatever follows the `]` WITHOUT a leading `:` is dropped, an
///   empty `:` tail carries no port constraint — both incumbent verbatim);
/// - a BARE multi-colon entry (a raw IPv6 literal) is the WHOLE entry as
///   the host — `2001:db8::25` never gets split into a host/port pair;
/// - every other entry keeps the per-entry `:port` semantics this module
///   already had: the tail after the LAST colon is a port only when it is
///   a non-empty digit run (a lone colon remains a host part).
///
/// A `[`-prefixed entry WITHOUT a closing bracket is malformed: it falls
/// through to the plain rules and simply never matches (the incumbent's
/// accidental `stripHostBrackets` trim of such entries is deliberately not
/// replicated — the registered contract is "bare multi-colon entries are
/// the WHOLE host", brackets included).
fn split_no_proxy_entry(entry: &str) -> (String, Option<&str>) {
    if entry.starts_with('[') {
        if let Some(end) = entry.find(']') {
            let host = entry[1..end].to_ascii_lowercase();
            let port = entry[end + 1..]
                .strip_prefix(':')
                .filter(|port| !port.is_empty());
            return (host, port);
        }
    }
    if entry.matches(':').count() > 1 {
        return (entry.to_ascii_lowercase(), None);
    }
    match entry.rsplit_once(':') {
        Some((head, tail))
            if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) =>
        {
            (head.to_string(), Some(tail))
        }
        _ => (entry.to_string(), None),
    }
}

/// True when the target's (bracket-stripped, lowercased) host and effective
/// port are matched by the NO_PROXY list. The ENTRY MATCHER itself, kept
/// separate from [`pick_proxy`] so the entry-level matching of loopback
/// literals (`[::1]:9443`) stays observable even though every real `::1`
/// URL is already direct through the FORCED local bypass (R05 RR1 F35
/// note ③: the end-to-end surface cannot distinguish the two).
fn no_proxy_list_matches(host: &str, port: &str, no_proxy: &str) -> bool {
    for entry in no_proxy.split([',', ' ']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (pattern, entry_port) = split_no_proxy_entry(entry);
        if let Some(want) = entry_port {
            if want != port {
                continue;
            }
        }
        if host_matches_no_proxy(host, &pattern) {
            return true;
        }
    }
    false
}

/// The frozen network policy of one configuration generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkPolicy {
    pub proxy: ProxyPolicy,
    pub trusted_ca_pem: Option<String>,
}

impl NetworkPolicy {
    /// The policy of a validated config section (system mode snapshots the
    /// current environment — call at load/reload time on the service side).
    pub fn from_config(config: &NetworkConfigSection) -> Self {
        Self {
            proxy: ProxyPolicy::from_config(&config.proxy),
            trusted_ca_pem: config.trusted_ca_pem.clone(),
        }
    }

    /// The proxy URL this policy routes ONE target URL through (`None` =
    /// direct). Mirrors the incumbent `resolveProxyForUrl` end to end.
    pub fn effective_proxy_for_url(&self, url: &reqwest::Url) -> Option<String> {
        let resolved = match &self.proxy {
            ProxyPolicy::Direct => return None,
            ProxyPolicy::System { env } => proxies_from_environment(env),
            ProxyPolicy::Manual {
                http,
                https,
                no_proxy,
            } => ResolvedProxies {
                http: http.clone(),
                https: https.clone(),
                no_proxy: no_proxy.clone(),
            },
        };
        pick_proxy(url, &resolved).map(str::to_string)
    }

    /// True when the policy would send this URL through a proxy.
    pub fn proxies(&self, url: &reqwest::Url) -> bool {
        self.effective_proxy_for_url(url).is_some()
    }
}

impl Default for NetworkPolicy {
    /// The incumbent default: system mode (an empty exported proxy
    /// environment behaves as direct — the incumbent's
    /// `DEFAULT_NETWORK_PROXY_CONFIG`).
    fn default() -> Self {
        Self {
            proxy: ProxyPolicy::System {
                env: BTreeMap::new(),
            },
            trusted_ca_pem: None,
        }
    }
}

// ── the reloadable plane + per-consumer client handles ────────────────────

/// The shared, reloadable source of the current policy generation. One
/// plane per service; every outbound consumer holds a
/// [`NetworkClient`] spawned from it.
pub struct NetworkPlane {
    generation: std::sync::atomic::AtomicU64,
    policy: RwLock<Arc<NetworkPolicy>>,
}

impl NetworkPlane {
    pub fn new(policy: NetworkPolicy) -> Self {
        Self {
            generation: std::sync::atomic::AtomicU64::new(1),
            policy: RwLock::new(Arc::new(policy)),
        }
    }

    /// An isolated plane pinned to DIRECT (the pre-F14 behavior). The
    /// legacy constructors of the adapters/consumers use this so existing
    /// tests keep their exact current network behavior; production wiring
    /// always passes the plane built from the loaded config.
    pub fn direct_isolated() -> Arc<Self> {
        Arc::new(Self::new(NetworkPolicy {
            proxy: ProxyPolicy::Direct,
            trusted_ca_pem: None,
        }))
    }

    /// An isolated plane with an arbitrary policy (tests).
    pub fn isolated(policy: NetworkPolicy) -> Arc<Self> {
        Arc::new(Self::new(policy))
    }

    /// The production default: system mode with the CURRENT environment
    /// snapshotted (called once at bootstrap from the loaded config).
    pub fn from_environment() -> Arc<Self> {
        Arc::new(Self::new(NetworkPolicy {
            proxy: ProxyPolicy::System {
                env: std::env::vars().collect(),
            },
            trusted_ca_pem: None,
        }))
    }

    /// The current policy (one Arc snapshot).
    pub fn policy(&self) -> Arc<NetworkPolicy> {
        self.policy
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The current generation counter.
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Publishes a new policy generation (the model-plane reload surface
    /// calls this atomically with the route-table swap). In-flight requests
    /// keep the client they started with; the next request per consumer
    /// rebuilds under the new generation.
    pub fn apply(&self, policy: NetworkPolicy) -> u64 {
        *self
            .policy
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Arc::new(policy);
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    /// Spawns one client handle bound to this plane (the consumer's timeout
    /// segments; the policy comes from the plane, per generation).
    pub fn client_handle(
        self: &Arc<Self>,
        timeouts: HttpTimeouts,
    ) -> Result<NetworkClient, ProtocolError> {
        Ok(Arc::new(NetworkClientHandle::new(
            Arc::clone(self),
            timeouts,
        )?))
    }
}

/// One consumer's lazily-rebuilt HTTP client: `(generation, client)`. The
/// client is rebuilt on first use after a policy generation change —
/// `reqwest::Client` is cheap to clone (an `Arc` handle), so every request
/// clones the current snapshot out of the lock.
pub struct NetworkClientHandle {
    plane: Arc<NetworkPlane>,
    timeouts: HttpTimeouts,
    state: RwLock<(u64, reqwest::Client)>,
}

impl NetworkClientHandle {
    fn new(plane: Arc<NetworkPlane>, timeouts: HttpTimeouts) -> Result<Self, ProtocolError> {
        let policy = plane.policy();
        let client = super::dispatch::build_client_under_policy(&timeouts, policy.as_ref())?;
        let generation = plane.generation();
        Ok(Self {
            plane,
            timeouts,
            state: RwLock::new((generation, client)),
        })
    }

    /// The client of the CURRENT policy generation (rebuilt lazily when the
    /// plane moved; a rebuild failure is loud — never a stale-policy
    /// fallback, which would silently contradict the frozen config).
    pub fn client(&self) -> Result<reqwest::Client, ProtocolError> {
        let current = self.plane.generation();
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.0 != current {
            let policy = self.plane.policy();
            state.1 = super::dispatch::build_client_under_policy(&self.timeouts, policy.as_ref())?;
            state.0 = current;
        }
        Ok(state.1.clone())
    }

    /// A ONE-OFF client of the CURRENT policy generation whose connections
    /// to `host` are PINNED to the pre-verified `addrs`
    /// (`resolve_to_addrs` — SNI/Host/certificate verification still run
    /// under the DOMAIN; only the dialed address is fixed). R05 RR1 F15:
    /// the egress guard resolves and verifies the DNS candidates itself,
    /// then dials through this so the transport cannot re-resolve the
    /// name to an address the guard never judged (a DNS rebinding between
    /// check and dial is structurally impossible). A fresh client per
    /// pinned download is the deliberate cost of the guarantee — the
    /// shared handle keeps pooling for every unpinned consumer.
    pub fn pinned_client(
        &self,
        host: &str,
        addrs: &[std::net::SocketAddr],
    ) -> Result<reqwest::Client, ProtocolError> {
        if addrs.is_empty() {
            return Err(ProtocolError::new(
                lingxi_protocol::ErrorCode::Internal,
                "a pinned client requires at least one verified address".to_string(),
                false,
            ));
        }
        let policy = self.plane.policy();
        super::dispatch::build_client_pinned_under_policy(
            &self.timeouts,
            policy.as_ref(),
            host,
            addrs,
        )
    }

    /// The plane this handle is bound to (the policy surface: e.g. the
    /// egress guard asks whether a download URL is proxied).
    pub fn plane(&self) -> &Arc<NetworkPlane> {
        &self.plane
    }

    /// The generation the cached client was built under (test
    /// observability of the lazy rebuild).
    pub fn built_generation(&self) -> u64 {
        self.state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .0
    }
}

/// The shared handle type every network consumer holds.
pub type NetworkClient = Arc<NetworkClientHandle>;

#[cfg(test)]
mod tests {
    use super::*;

    fn url(value: &str) -> reqwest::Url {
        reqwest::Url::parse(value).unwrap()
    }

    fn manual(http: Option<&str>, https: Option<&str>, no_proxy: &str) -> NetworkPolicy {
        NetworkPolicy {
            proxy: ProxyPolicy::Manual {
                http: http.map(str::to_string),
                https: https.map(str::to_string),
                no_proxy: no_proxy.to_string(),
            },
            trusted_ca_pem: None,
        }
    }

    #[test]
    fn forced_local_bypass_beats_every_list() {
        // Loopback never goes through a proxy — not when listed, not when a
        // `*` wildcard matches everything (the incumbent's forced bypass).
        let policy = manual(
            Some("http://proxy.invalid:8080"),
            Some("http://proxy.invalid:8080"),
            "",
        );
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://127.0.0.1:8080/v1")),
            None
        );
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://localhost:9/v1")),
            None
        );
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://[::1]:9/v1")),
            None
        );
        // A non-wildcard list must not bypass anything else.
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("http://api.example.invalid/v1"))
                .as_deref(),
            Some("http://proxy.invalid:8080")
        );
    }

    #[test]
    fn no_proxy_grammar_matches_the_incumbent_forms() {
        let policy = manual(
            Some("http://p.invalid:1"),
            None,
            "api.example.invalid, .corp.example, *.wild.example, exact.port.example:8443, \
             [::1]:9443, *",
        );
        // `*` bypasses everything non-local.
        assert_eq!(
            policy.effective_proxy_for_url(&url("https://anywhere.example/x")),
            None
        );
        let policy = manual(
            Some("http://p.invalid:1"),
            None,
            "api.example.invalid, .corp.example, *.wild.example, exact.port.example:8443, \
             [::1]:9443",
        );
        // Exact host.
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://api.example.invalid/x")),
            None
        );
        // Leading-dot suffix: the bare domain and any subdomain.
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://corp.example/x")),
            None
        );
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://a.b.corp.example/x")),
            None
        );
        // `*.`-prefix suffix form.
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://x.wild.example/x")),
            None
        );
        // A port-specific entry matches only that port.
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://exact.port.example:8443/x")),
            None
        );
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("http://exact.port.example:80/x"))
                .as_deref(),
            Some("http://p.invalid:1")
        );
        // An unlisted host still proxies.
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("http://other.example/x"))
                .as_deref(),
            Some("http://p.invalid:1")
        );
    }

    #[test]
    fn bracketed_ipv6_no_proxy_entries_bypass_public_literals() {
        // F35: the incumbent `splitNoProxyEntry` bracket branch. The
        // pre-fix parser split `[2001:db8::25]` at the LAST colon into a
        // host `[2001:db8` + port `25` — the entry never matched and the
        // PUBLIC IPv6 literal an operator explicitly listed stayed on the
        // proxy.
        let policy = manual(
            Some("http://p.invalid:1"),
            Some("http://p.invalid:1"),
            "[2001:db8::25]",
        );
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://[2001:db8::25]/x")),
            None
        );
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://[2001:db8::25]:80/x")),
            None
        );
        assert_eq!(
            policy.effective_proxy_for_url(&url("https://[2001:db8::25]:443/x")),
            None
        );
        // A different literal is NOT covered by the entry.
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("http://[2001:db8::26]/x"))
                .as_deref(),
            Some("http://p.invalid:1")
        );

        // The port-bearing bracket form bypasses ONLY that port.
        let policy = manual(
            Some("http://p.invalid:1"),
            Some("http://p.invalid:1"),
            "[2001:db8::25]:9443",
        );
        assert_eq!(
            policy.effective_proxy_for_url(&url("https://[2001:db8::25]:9443/x")),
            None
        );
        // Port-mismatch contrast: every other port stays on the proxy.
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("http://[2001:db8::25]:80/x"))
                .as_deref(),
            Some("http://p.invalid:1")
        );
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("https://[2001:db8::25]/x"))
                .as_deref(),
            Some("http://p.invalid:1")
        );

        // A BARE multi-colon literal (no brackets) is the WHOLE entry as
        // the host (the incumbent's colon-count rule).
        let policy = manual(Some("http://p.invalid:1"), None, "2001:db8::25");
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://[2001:db8::25]/x")),
            None
        );
    }

    #[test]
    fn loopback_bracket_entries_are_pinned_at_the_entry_level() {
        // F35 note ③: every `::1` URL is ALREADY direct through the FORCED
        // local bypass, so the end-to-end surface cannot distinguish "the
        // `[::1]:9443` entry matched" from "loopback never proxies" — the
        // ENTRY MATCHER itself is pinned here.
        assert!(no_proxy_list_matches("::1", "9443", "[::1]:9443"));
        // Port-mismatch contrasts at the entry level.
        assert!(!no_proxy_list_matches("::1", "80", "[::1]:9443"));
        assert!(!no_proxy_list_matches("::1", "443", "[::1]:9443"));
        // The bare bracket form carries no port constraint.
        assert!(no_proxy_list_matches("::1", "80", "[::1]"));
        assert!(no_proxy_list_matches("::1", "9443", "[::1]"));
        // The public literal at the entry level: any port without a port
        // suffix, only the listed port with one.
        assert!(no_proxy_list_matches(
            "2001:db8::25",
            "80",
            "[2001:db8::25]"
        ));
        assert!(!no_proxy_list_matches(
            "2001:db8::26",
            "80",
            "[2001:db8::25]"
        ));
        assert!(no_proxy_list_matches(
            "2001:db8::25",
            "9443",
            "[2001:db8::25]:9443"
        ));
        assert!(!no_proxy_list_matches(
            "2001:db8::25",
            "80",
            "[2001:db8::25]:9443"
        ));
        // The BARE multi-colon literal matches as the whole host.
        assert!(no_proxy_list_matches("2001:db8::25", "80", "2001:db8::25"));
        assert!(!no_proxy_list_matches("2001:db8::26", "80", "2001:db8::25"));
        // An unclosed bracket is malformed and never matches.
        assert!(!no_proxy_list_matches(
            "2001:db8::25",
            "80",
            "[2001:db8::25"
        ));
        // Whatever follows the closing bracket WITHOUT a leading `:` is
        // dropped (incumbent verbatim) — the entry still matches on host.
        assert!(no_proxy_list_matches("::1", "80", "[::1]junk"));
        // Whatever follows the closing bracket after an EMPTY `:` carries
        // no port constraint (the incumbent's falsy-empty-port rule).
        assert!(no_proxy_list_matches("::1", "80", "[::1]:"));
    }

    #[test]
    fn scheme_selection_prefers_the_scheme_proxy() {
        let policy = manual(Some("http://p1.invalid:1"), Some("http://p2.invalid:2"), "");
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("http://api.example.invalid/x"))
                .as_deref(),
            Some("http://p1.invalid:1")
        );
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("https://api.example.invalid/x"))
                .as_deref(),
            Some("http://p2.invalid:2")
        );
        // Only an http proxy declared: https falls back to it (incumbent).
        let policy = manual(Some("http://p1.invalid:1"), None, "");
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("https://api.example.invalid/x"))
                .as_deref(),
            Some("http://p1.invalid:1")
        );
    }

    #[test]
    fn system_mode_resolves_the_environment_snapshot() {
        let env = BTreeMap::from_iter([
            (
                "HTTPS_PROXY".to_string(),
                "http://env-proxy.invalid:3128".to_string(),
            ),
            ("NO_PROXY".to_string(), "skip.example".to_string()),
        ]);
        let policy = NetworkPolicy {
            proxy: ProxyPolicy::System { env },
            trusted_ca_pem: None,
        };
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("https://api.example.invalid/x"))
                .as_deref(),
            Some("http://env-proxy.invalid:3128")
        );
        assert_eq!(
            policy.effective_proxy_for_url(&url("https://skip.example/x")),
            None
        );
        // Lowercase spellings ride the same precedence.
        let env = BTreeMap::from_iter([(
            "https_proxy".to_string(),
            "http://lower-proxy.invalid:3128".to_string(),
        )]);
        let policy = NetworkPolicy {
            proxy: ProxyPolicy::System { env },
            trusted_ca_pem: None,
        };
        assert_eq!(
            policy
                .effective_proxy_for_url(&url("https://api.example.invalid/x"))
                .as_deref(),
            Some("http://lower-proxy.invalid:3128")
        );
        // An unparseable exported URL is skipped, never trusted.
        let env = BTreeMap::from_iter([("HTTP_PROXY".to_string(), "::::not a url".to_string())]);
        let policy = NetworkPolicy {
            proxy: ProxyPolicy::System { env },
            trusted_ca_pem: None,
        };
        assert_eq!(
            policy.effective_proxy_for_url(&url("http://api.example.invalid/x")),
            None
        );
        // An empty snapshot behaves as direct.
        let policy = NetworkPolicy::default();
        assert_eq!(
            policy.effective_proxy_for_url(&url("https://api.example.invalid/x")),
            None
        );
    }

    #[test]
    fn config_validation_is_loud_on_every_incumbent_rule() {
        // Mode vocabulary.
        let bad = NetworkConfigSection {
            proxy: NetworkProxyConfig {
                mode: "auto".into(),
                http_proxy: None,
                https_proxy: None,
                no_proxy: None,
            },
            trusted_ca_pem: None,
        };
        assert!(bad.validate().is_err());
        // Manual without any URL.
        let bad = NetworkConfigSection {
            proxy: NetworkProxyConfig {
                mode: "manual".into(),
                http_proxy: Some("  ".into()),
                https_proxy: None,
                no_proxy: None,
            },
            trusted_ca_pem: None,
        };
        assert!(bad.validate().is_err());
        // A path on the proxy URL.
        let bad = NetworkConfigSection {
            proxy: NetworkProxyConfig {
                mode: "manual".into(),
                http_proxy: Some("http://p.invalid:8080/should/not".into()),
                https_proxy: None,
                no_proxy: None,
            },
            trusted_ca_pem: None,
        };
        assert!(bad.validate().is_err());
        // An unsupported scheme.
        let bad = NetworkConfigSection {
            proxy: NetworkProxyConfig {
                mode: "manual".into(),
                http_proxy: Some("ftp://p.invalid:21".into()),
                https_proxy: None,
                no_proxy: None,
            },
            trusted_ca_pem: None,
        };
        assert!(bad.validate().is_err());
        // userinfo on the proxy URL.
        let bad = NetworkConfigSection {
            proxy: NetworkProxyConfig {
                mode: "manual".into(),
                http_proxy: Some("http://user:pass@p.invalid:8080".into()),
                https_proxy: None,
                no_proxy: None,
            },
            trusted_ca_pem: None,
        };
        assert!(bad.validate().is_err());
        // A malformed PEM bundle.
        let bad = NetworkConfigSection {
            proxy: NetworkProxyConfig {
                mode: "direct".into(),
                http_proxy: None,
                https_proxy: None,
                no_proxy: None,
            },
            trusted_ca_pem: Some("not a pem".into()),
        };
        assert!(bad.validate().is_err());
        // The happy shapes.
        let good = NetworkConfigSection {
            proxy: NetworkProxyConfig {
                mode: "manual".into(),
                http_proxy: Some("http://p.invalid:8080".into()),
                https_proxy: Some("socks5://s.invalid:1080".into()),
                no_proxy: Some("localhost, 127.0.0.1".into()),
            },
            trusted_ca_pem: None,
        };
        assert!(good.validate().is_ok());
    }

    #[test]
    fn plane_generations_rebuild_the_client_lazily() {
        let plane = NetworkPlane::direct_isolated();
        let handle = plane.client_handle(HttpTimeouts::default()).unwrap();
        let _first = handle.client().unwrap();
        // Same generation: the cached client is reused.
        assert_eq!(handle.built_generation(), plane.generation());
        // A policy publish bumps the generation; the next request observes
        // the rebuilt client.
        let before = plane.generation();
        plane.apply(NetworkPolicy {
            proxy: ProxyPolicy::Direct,
            trusted_ca_pem: None,
        });
        assert_eq!(plane.generation(), before + 1);
        assert_eq!(handle.built_generation(), before);
        let _rebuilt = handle.client().unwrap();
        assert_eq!(handle.built_generation(), plane.generation());
    }
}
