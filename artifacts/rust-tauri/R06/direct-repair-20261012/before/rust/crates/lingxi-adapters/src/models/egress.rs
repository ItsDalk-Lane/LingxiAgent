//! The media-egress guard (R05-T06 §29.11, C11B): the URL-fetch discipline
//! for media PRODUCTS and pre-fetch inputs. The dialects surface provider
//! URLs ([`super::operations::MediaProductRef::Url`], a gemini reference
//! image, a dashscope audio address); fetching them is the SERVICE layer's
//! job THROUGH THIS GUARD — the adapters never fetch unguarded, and the
//! guard NEVER carries provider credentials.
//!
//! Policy (pre-registered, §29.11):
//! - scheme is http or https;
//! - the authority carries NO userinfo (`user:pass@host` refuses);
//! - the host is classified by the SAME WHATWG parser the HTTP client
//!   dials with (the `url` crate behind `reqwest`), so EVERY spelling
//!   that normalizes to a literal IP — dotted decimal, hex/octal/
//!   short-form/integer/leading-zero/trailing-dot IPv4, percent-encoded
//!   hosts, IPv4-mapped and IPv4-compatible IPv6 — is judged as that IP;
//!   a literal IP in the private / loopback / link-local / unspecified
//!   ranges refuses — the single exception is a URL whose ORIGIN equals
//!   a configured provider endpoint origin (the loopback stub and
//!   self-hosted same-origin asset case; fix-r1 closed the review F-01
//!   gap where non-decimal spellings rode the hostname branch);
//! - an https PUBLIC hostname passes ONLY after its REAL DNS RESOLUTION is
//!   verified: every candidate address is judged by the guarded-range
//!   table (loopback / private / link-local / ULA / mapped spellings), a
//!   hostname with ANY guarded candidate refuses, and the download DIALS
//!   THROUGH the verified address set (`resolve_to_addrs` — the transport
//!   cannot re-resolve the name to an address the guard never judged; R05
//!   RR1 F15 closed the check-then-dial gap the audit's CN-F06 counter
//!   example proved: `https://localhost:<port>/` reached an unauthorized
//!   loopback listener). When the frozen network policy routes the URL
//!   through a PROXY, the proxy is the resolution authority — the local
//!   candidates are still verified as defense in depth, but the dial is
//!   the proxy's; plain http is same-origin-only (any host);
//! - redirects are never followed (the shared no-redirect client);
//! - the download carries NO credential material of any provider;
//! - the body is read under [`EGRESS_DOWNLOAD_MAX_BYTES`] DURING the read
//!   (a loud refusal, never a truncation);
//! - the response Content-Type must be an `image/`, `video/`, or `audio/`
//!   type — anything else refuses (an HTML error page is never
//!   materialized as a product).

use std::time::Duration;

use lingxi_protocol::{ErrorCode, ProtocolError};

use super::dispatch::{self, HttpTimeouts};

/// The egress download byte cap (§29.11: 64 MiB, enforced mid-read).
pub const EGRESS_DOWNLOAD_MAX_BYTES: usize = 64 * 1024 * 1024;

fn refuse(detail: String) -> ProtocolError {
    ProtocolError::new(ErrorCode::Forbidden, detail, false)
}

/// The origin allowlist of a provider-endpoint set (invalid endpoints
/// refuse loudly — they were validated at config load, so a failure here
/// is a wiring bug).
fn parse_origins(endpoints: &[String]) -> Result<Vec<String>, ProtocolError> {
    let mut origins = Vec::new();
    for endpoint in endpoints {
        let parts = parse_url(endpoint).map_err(|_| {
            refuse(format!(
                "provider endpoint {endpoint:?} is not a usable http(s) URL for the egress \
                 allowlist"
            ))
        })?;
        origins.push(parts.origin);
    }
    Ok(origins)
}

/// The parse of one candidate URL's security-relevant parts.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UrlParts {
    scheme: String,
    /// The origin with default ports elided (`scheme://host[:port]`) —
    /// the same shape both sides of the allowlist produce.
    origin: String,
    /// The WHATWG-CANONICAL host serialization (`url`/`reqwest`): an
    /// IPv4 host renders dotted-decimal, an IPv6 host renders colon-hex
    /// WITHOUT brackets (and therefore contains `':'`), a domain renders
    /// ASCII/punycode with neither. This is exactly the host string the
    /// HTTP client will dial, so the guard's literal-IP classification
    /// cannot diverge from the dialer's by spelling (fix-r1 / F-01).
    host: String,
}

fn parse_url(url: &str) -> Result<UrlParts, ProtocolError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(refuse("egress URL is empty".to_string()));
    }
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return Err(refuse(format!(
            "egress URL {url:?} carries no scheme; only http(s) is fetchable"
        )));
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(refuse(format!(
            "egress URL scheme {scheme:?} is refused; only http(s) is fetchable"
        )));
    }
    // The WHATWG parser TOLERATES redundant leading slashes
    // (`https:///a.png` skips them and dials host `a.png`); the guard
    // stays stricter than the dialer here — an empty raw authority was
    // never a usable product/endpoint URL, so it refuses loudly.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(refuse(format!("egress URL {url:?} carries no host")));
    }
    // The authority is classified by the SAME WHATWG parser the HTTP
    // client dials with (`reqwest` resolves hosts through this parser),
    // so hex/octal/short-form/integer/leading-zero/trailing-dot IPv4
    // spellings, percent-encoded hosts, malformed ports, unterminated
    // IPv6 brackets, and zone ids are all seen exactly as the client
    // sees them — refused here when the client would refuse, and
    // normalized to the literal IP it would dial when it would not
    // (review F-01: the guard must never classify a guarded address's
    // spelling as a public hostname).
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| {
        refuse(format!(
            "egress URL {url:?} is not a usable http(s) URL under the WHATWG \
             parser the HTTP client dials with; refusing"
        ))
    })?;
    if scheme != parsed.scheme() {
        return Err(refuse(format!(
            "egress URL {url:?} scheme disagrees with its WHATWG parse; refusing"
        )));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(refuse(format!(
            "egress URL {url:?} carries userinfo in its authority; refusing"
        )));
    }
    // `host_str` serializes IPv6 WITH brackets (mapped addresses as hex
    // groups, e.g. `[::ffff:7f00:1]`); strip them so the host field is
    // the bare literal the IPv6 parser takes.
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| refuse(format!("egress URL {url:?} carries no host")))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    // Default ports elide: `Url::port` is None exactly when the explicit
    // port equals the scheme default, so `:443` https and bare https are
    // one origin on BOTH sides of the allowlist comparison.
    let origin = match parsed.port() {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    };
    Ok(UrlParts {
        scheme,
        origin,
        host,
    })
}

/// Parses a dotted IPv4 literal (four decimal octets, each 0-255; leading
/// zeros refuse). Host classification is upstream: `parse_url` feeds this
/// only the WHATWG-canonical serializations, so the stricter-than-spec
/// stance here is defense in depth over an already-normalized host, never
/// the sole gatekeeper of "is this host a literal IP".
fn parse_ipv4(host: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut out = [0u8; 4];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if part.len() > 1 && part.starts_with('0') {
            return None;
        }
        out[index] = part.parse::<u16>().ok()?.try_into().ok()?;
    }
    Some(out)
}

/// Parses an IPv6 literal (the bracket-stripped `[…]` form) into 16 bytes.
/// Covers the forms a URL host can carry; an unparseable host that LOOKS
/// like an IPv6 literal (contains a colon) is the CALLER's refusal.
fn parse_ipv6(host: &str) -> Option<[u8; 16]> {
    let mut left = [0u16; 8];
    let mut right = [0u16; 8];
    let (head, tail) = match host.split_once("::") {
        Some((head, tail)) => (head, Some(tail)),
        None => (host, None),
    };
    let groups = |slice: &str, out: &mut [u16; 8]| -> Option<usize> {
        if slice.is_empty() {
            return Some(0);
        }
        let parts: Vec<&str> = slice.split(':').collect();
        if parts.len() > 8 {
            return None;
        }
        // An embedded IPv4 tail ("::ffff:1.2.3.4") occupies two groups.
        if let Some(last) = parts.last() {
            if last.contains('.') {
                if parts.len() > 7 {
                    return None;
                }
                let octets = parse_ipv4(last)?;
                for (index, part) in parts[..parts.len() - 1].iter().enumerate() {
                    out[index] = u16::from_str_radix(part, 16).ok()?;
                }
                let base = parts.len() - 1;
                out[base] = u16::from_be_bytes([octets[0], octets[1]]);
                out[base + 1] = u16::from_be_bytes([octets[2], octets[3]]);
                return Some(parts.len() + 1);
            }
        }
        for (index, part) in parts.iter().enumerate() {
            if part.is_empty() || part.len() > 4 {
                return None;
            }
            out[index] = u16::from_str_radix(part, 16).ok()?;
        }
        Some(parts.len())
    };
    let head_groups = groups(head, &mut left)?;
    match tail {
        Some(tail) => {
            let tail_groups = groups(tail, &mut right)?;
            if head_groups + tail_groups > 7 {
                return None;
            }
            let mut bytes = [0u8; 16];
            for (index, group) in left.iter().take(head_groups).enumerate() {
                bytes[index * 2..index * 2 + 2].copy_from_slice(&group.to_be_bytes());
            }
            for (index, group) in right.iter().take(tail_groups).enumerate() {
                let at = 14 - (tail_groups - 1 - index) * 2;
                bytes[at..at + 2].copy_from_slice(&group.to_be_bytes());
            }
            Some(bytes)
        }
        None => {
            if head_groups != 8 {
                return None;
            }
            let mut bytes = [0u8; 16];
            for (index, group) in left.iter().enumerate() {
                bytes[index * 2..index * 2 + 2].copy_from_slice(&group.to_be_bytes());
            }
            Some(bytes)
        }
    }
}

/// True when the literal IP falls in a range that is never a legitimate
/// public egress destination (private / loopback / link-local /
/// unspecified; the RFC 6598 shared range rides "private" — registered).
/// The 16-byte arm additionally FOLDS: an IPv4-mapped `::ffff:0:0/96`
/// literal is judged as its embedded v4, and the deprecated
/// IPv4-compatible `::/96` refuses outright (fix-r1 / F-01 — no spelling
/// of a guarded address may pass).
fn ip_is_guarded(octets: &[u8]) -> bool {
    match octets.len() {
        4 => {
            let [a, b, _, _] = [octets[0], octets[1], octets[2], octets[3]];
            a == 0 // unspecified 0.0.0.0/8
                || a == 10 // private 10/8
                || a == 127 // loopback 127/8
                || (a == 100 && (b & 0xc0) == 64) // RFC 6598 shared 100.64/10
                || (a == 169 && b == 254) // link-local 169.254/16
                || (a == 172 && (b & 0xf0) == 16) // private 172.16/12
                || (a == 192 && b == 168) // private 192.168/16
                || (a == 198 && (b & 0xfe) == 18) // benchmark 198.18/15
        }
        16 => {
            // An IPv4-MAPPED literal (::ffff:0:0/96 — e.g. `[::ffff:127.0.0.1]`)
            // folds to its embedded v4 policy: a dual-stack socket dials the
            // mapped v4 address, so the spelling may not dodge the v4 ranges
            // (fix-r1 / F-01).
            if octets[..10].iter().all(|byte| *byte == 0)
                && octets[10] == 0xff
                && octets[11] == 0xff
            {
                return ip_is_guarded(&octets[12..]);
            }
            // The deprecated IPv4-COMPATIBLE ::/96 (which also subsumes `::`
            // and `::1`) embeds a v4 spelling; it is never a legitimate
            // public egress destination, so it refuses outright.
            if octets[..12].iter().all(|byte| *byte == 0) {
                return true;
            }
            let first = octets[0];
            (first & 0xfe) == 0xfc // ULA fc00::/7 (private)
                || (first == 0xfe && (octets[1] & 0xc0) == 0x80) // link-local fe80::/10
                || first == 0xff // multicast (never an egress destination)
        }
        _ => false,
    }
}

/// The egress guard's hostname resolver (R05 RR1 F15): `(host, port) →
/// candidate socket addresses`. The DEFAULT is the operating system's
/// resolver (`tokio::net::lookup_host` — the same getaddrinfo path the
/// transport would use); tests inject a controlled resolver to pin exact
/// candidate sets (mixed public/private records, re-resolution, failures)
/// without touching real DNS. The resolver is an IO boundary ONLY — the
/// GUARDED-RANGE JUDGMENT of the returned candidates is always this
/// module's own code.
pub type EgressResolver = std::sync::Arc<
    dyn Fn(
            &str,
            u16,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<std::net::SocketAddr>, String>> + Send>,
        > + Send
        + Sync,
>;

/// The default OS resolver (getaddrinfo via tokio).
fn system_resolver() -> EgressResolver {
    std::sync::Arc::new(
        |host: &str,
         port: u16|
         -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<std::net::SocketAddr>, String>> + Send>,
        > {
            let host = host.to_string();
            Box::pin(async move {
                let candidates = tokio::net::lookup_host((host.as_str(), port))
                    .await
                    .map_err(|err| format!("system resolver refused {host:?}: {err}"))?;
                Ok(candidates.collect())
            })
        },
    )
}

/// True when the candidate is judged by the socket address's IP (v4 or v6)
/// through the guarded-range table.
fn socket_addr_is_guarded(addr: &std::net::SocketAddr) -> bool {
    match addr {
        std::net::SocketAddr::V4(v4) => ip_is_guarded(&v4.ip().octets()),
        std::net::SocketAddr::V6(v6) => {
            let octets = v6.ip().octets();
            // A v4-mapped candidate folds to its embedded v4 policy (the
            // dual-stack dial would reach the v4 address).
            ip_is_guarded(&octets)
        }
    }
}

/// The egress guard: one configured provider-endpoint origin allowlist, a
/// policy-bound client handle and the resolver.
pub struct EgressGuard {
    allowed_origins: std::sync::RwLock<Vec<String>>,
    client: dispatch::NetworkClient,
    timeouts: HttpTimeouts,
    resolver: EgressResolver,
}

impl EgressGuard {
    /// Builds the guard from the configured provider endpoint URLs. Origins
    /// are compared as `scheme://host[:port]` with default ports elided.
    /// The legacy constructor keeps the pre-F14 direct policy and the
    /// system resolver; production wiring uses
    /// [`Self::from_provider_endpoints_and_network`].
    pub fn from_provider_endpoints(endpoints: &[String]) -> Result<Self, ProtocolError> {
        Self::from_provider_endpoints_and_network(
            endpoints,
            super::network::NetworkPlane::direct_isolated(),
        )
    }

    /// R05 RR1 F14: the production constructor — the download client is
    /// BOUND to the model plane's shared, reloadable network policy.
    pub fn from_provider_endpoints_and_network(
        endpoints: &[String],
        network: std::sync::Arc<super::network::NetworkPlane>,
    ) -> Result<Self, ProtocolError> {
        Ok(Self {
            allowed_origins: std::sync::RwLock::new(parse_origins(endpoints)?),
            client: network.client_handle(HttpTimeouts::default())?,
            timeouts: HttpTimeouts::default(),
            resolver: system_resolver(),
        })
    }

    /// Overrides the resolver (a TEST seam for controlled candidate sets —
    /// see [`EgressResolver`]; the guarded-range judgment stays here).
    pub fn with_resolver(mut self, resolver: EgressResolver) -> Self {
        self.resolver = resolver;
        self
    }

    /// Atomically swaps the allowlist (the model-plane reload surface calls
    /// this after a config swap so same-origin egress follows the CURRENT
    /// endpoints; an in-flight download keeps the origins it started with).
    pub fn set_provider_endpoints(&self, endpoints: &[String]) -> Result<(), ProtocolError> {
        let origins = parse_origins(endpoints)?;
        *self.allowed_origins.write().expect("egress origins lock") = origins;
        Ok(())
    }

    /// Policy check of ONE URL (no fetch, no DNS). Public for the service
    /// layer's pre-flight and for tests. R05 RR1 F15: this remains the
    /// SYNCHRONOUS pass (scheme/userinfo/literal-IP/origin rules); the
    /// HOSTNAME resolution judgment lives in [`Self::resolve_candidates`]
    /// and runs inside [`Self::download`] (and the service pre-flight via
    /// [`Self::check_url_resolved`]).
    pub fn check_url(&self, url: &str) -> Result<(), ProtocolError> {
        self.check_parts(&parse_url(url)?).map(|_| ())
    }

    /// The shared policy core: parses once, answers the parsed parts AND
    /// whether the ORIGIN EXCEPTION matched (the configured-endpoint
    /// origin is the explicit local-model exception — R05 RR1 F15 keeps it
    /// minimal and origin-exact).
    fn check_parts(&self, parts: &UrlParts) -> Result<bool, ProtocolError> {
        let allowed = self.allowed_origins.read().expect("egress origins lock");
        let same_origin = allowed.contains(&parts.origin);
        // Literal-IP hosts in guarded ranges refuse unless the origin is a
        // configured provider endpoint (the loopback-stub / self-hosted
        // exception). `parts.host` is the WHATWG-canonical serialization,
        // so an IPv4 host of ANY spelling arrives here dotted-decimal and
        // an IPv6 host arrives colon-hex (unbracketed, contains ':').
        if let Some(octets) = parse_ipv4(&parts.host) {
            if ip_is_guarded(&octets) && !same_origin {
                return Err(refuse(
                    "egress URL targets a guarded literal IP; refusing (the only exception \
                     is a configured provider endpoint origin)"
                        .to_string(),
                ));
            }
        } else if parts.host.contains(':') {
            match parse_ipv6(&parts.host) {
                Some(octets) => {
                    if ip_is_guarded(&octets) && !same_origin {
                        return Err(refuse(
                            "egress URL targets a guarded literal IPv6; refusing (the only \
                             exception is a configured provider endpoint origin)"
                                .to_string(),
                        ));
                    }
                }
                None => {
                    return Err(refuse(
                        "egress URL carries an unparseable IPv6 literal; refusing".to_string(),
                    ));
                }
            }
        }
        // Plain http is same-origin-only (any host); https hostnames pass
        // the SYNC layer and are judged on their RESOLVED candidates in
        // the async layer (`resolve_candidates`).
        if parts.scheme == "http" && !same_origin {
            return Err(refuse(
                "egress URL is plain http outside the configured provider endpoint origins; \
                 refusing"
                    .to_string(),
            ));
        }
        Ok(same_origin)
    }

    /// True when the URL's host is a HOSTNAME (not an IP literal) — the
    /// resolution-judgment candidates exist only for these.
    fn host_is_name(parts: &UrlParts) -> bool {
        parse_ipv4(&parts.host).is_none() && !parts.host.contains(':')
    }

    /// R05 RR1 F15 — resolves the URL's hostname and judges EVERY candidate
    /// through the guarded-range table:
    /// - a resolution failure or an EMPTY candidate set refuses loudly
    ///   (an unresolvable product URL is never dialed "to see");
    /// - ANY guarded candidate (loopback / private / link-local / ULA /
    ///   mapped / multicast — including a MIXED public+private record set)
    ///   refuses: the transport could dial that candidate;
    /// - an all-public candidate set is returned VERIFIED — the caller
    ///   dials THROUGH it (pinned), so a DNS change between check and
    ///   dial cannot reach an address this function never judged.
    ///
    /// Literal-IP URLs (judged synchronously) and same-origin exceptions
    /// (the configured endpoint origin itself) return an EMPTY set = no
    /// resolution judgment applies.
    async fn resolve_candidates(
        &self,
        parts: &UrlParts,
        same_origin: bool,
    ) -> Result<Vec<std::net::SocketAddr>, ProtocolError> {
        if same_origin || !Self::host_is_name(parts) || parts.scheme != "https" {
            return Ok(Vec::new());
        }
        let port = parts
            .origin
            .rsplit(':')
            .next()
            .and_then(|tail| tail.parse::<u16>().ok())
            .unwrap_or(443);
        let candidates = (self.resolver)(&parts.host, port).await.map_err(|detail| {
            refuse(format!(
                "egress URL host {:?} could not be resolved ({}); an unresolvable \
                     destination is never dialed",
                parts.host, detail
            ))
        })?;
        if candidates.is_empty() {
            return Err(refuse(format!(
                "egress URL host {:?} resolved to ZERO addresses; refusing",
                parts.host
            )));
        }
        for candidate in &candidates {
            if socket_addr_is_guarded(candidate) {
                return Err(refuse(format!(
                    "egress URL host {:?} resolves to a guarded address ({candidate}); a \
                     hostname whose record set contains ANY loopback/private/link-local \
                     candidate is never dialed (the only exception is a configured \
                     provider endpoint origin)",
                    parts.host
                )));
            }
        }
        Ok(candidates)
    }

    /// The full async policy judgment of one URL (no fetch): the sync rules
    /// PLUS the resolution judgment of [`Self::resolve_candidates`]. The
    /// service layer's pre-flight calls this; `download` calls it as its
    /// first stage and dials through the verified candidates.
    pub async fn check_url_resolved(&self, url: &str) -> Result<(), ProtocolError> {
        let parts = parse_url(url)?;
        let same_origin = self.check_parts(&parts)?;
        self.resolve_candidates(&parts, same_origin).await?;
        Ok(())
    }

    /// Fetches one media URL through the policy: no credentials, no
    /// redirects, the Content-Type MUST be image/, video/, or audio/, the
    /// body is read under [`EGRESS_DOWNLOAD_MAX_BYTES`] mid-read.
    /// R05 RR1 F15: an https HOSTNAME is dialed THROUGH its verified
    /// candidate set (a per-download pinned client, `resolve_to_addrs` —
    /// SNI/Host/certificate verification still run under the domain); when
    /// the frozen network policy routes this URL through a proxy, the
    /// proxy is the resolution authority (the verified-candidate judgment
    /// still ran as defense in depth) and the dial is the ordinary
    /// policy-bound client. Returns the bytes with the response
    /// Content-Type (mime hint resolution is the caller's).
    pub async fn download(
        &self,
        url: &str,
        deadline_unix_ms: Option<u64>,
    ) -> Result<(Vec<u8>, String), (ProtocolError, bool)> {
        let parts = parse_url(url).map_err(|error| (error, false))?;
        let same_origin = self.check_parts(&parts).map_err(|error| (error, false))?;
        let verified = self
            .resolve_candidates(&parts, same_origin)
            .await
            .map_err(|error| (error, false))?;
        // Which client dials: pinned (direct + hostname + verified
        // candidates) or the ordinary policy client (proxy / literal IP /
        // same-origin exception).
        let target = reqwest::Url::parse(url).map_err(|err| {
            (
                refuse(format!("egress URL {url:?} is not parseable: {err}")),
                false,
            )
        })?;
        let proxied = self.client.plane().policy().proxies(&target);
        let request_client = if !verified.is_empty() && !proxied {
            self.client
                .pinned_client(&parts.host, &verified)
                .map_err(|error| (error, false))?
        } else {
            self.client.client().map_err(|error| (error, false))?
        };
        let request = request_client.get(url);
        let response =
            dispatch::send_head_with_timeouts(request, &self.timeouts, deadline_unix_ms).await?;
        let status = response.status();
        let headers = response.headers().clone();
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !status.is_success() {
            // The body excerpt of a plain-media GET carries no credential
            // material (this request never sent any) — classification
            // reuses the shared table with an empty material set.
            let body_text = String::from_utf8_lossy(
                &read_bounded(response, 64 * 1024, deadline_unix_ms).await?,
            )
            .into_owned();
            let no_auth = super::credentials::ApplicableAuth::None;
            return Err(dispatch::classify_error_status(
                status,
                &body_text,
                &no_auth,
                dispatch::parse_retry_after(&headers),
            ));
        }
        if !(content_type.starts_with("image/")
            || content_type.starts_with("video/")
            || content_type.starts_with("audio/"))
        {
            return Err((
                refuse(format!(
                    "eggress download of {url:?} answered Content-Type {content_type:?}; only \
                     image/, video/, or audio/ products are fetchable"
                )),
                false,
            ));
        }
        let bytes = read_bounded(response, EGRESS_DOWNLOAD_MAX_BYTES, deadline_unix_ms).await?;
        Ok((bytes, content_type))
    }
}

/// The bounded mid-read of a download body (the §29.11 cap discipline —
/// the same shape as the operations crate's `read_body_bounded`, kept
/// local so the guard owns its own limit).
async fn read_bounded(
    mut response: reqwest::Response,
    cap: usize,
    deadline_unix_ms: Option<u64>,
) -> Result<Vec<u8>, (ProtocolError, bool)> {
    let mut out = Vec::new();
    loop {
        let next = response.chunk();
        let chunk = match dispatch::remaining_budget_ms(deadline_unix_ms) {
            Some(remaining) => {
                match tokio::time::timeout(Duration::from_millis(remaining), next).await {
                    Ok(result) => result,
                    Err(_) => {
                        return Err((
                            dispatch::budget_exceeded_error(
                                "egress download outlived its deadline mid-body".to_string(),
                            ),
                            false,
                        ));
                    }
                }
            }
            None => next.await,
        };
        match chunk {
            Ok(Some(bytes)) => {
                if out.len() + bytes.len() > cap {
                    return Err((
                        ProtocolError::new(
                            ErrorCode::InvalidMessage,
                            format!(
                                "egress download body exceeded the {cap} byte cap mid-read; \
                                 refused, never truncated"
                            ),
                            false,
                        ),
                        false,
                    ));
                }
                out.extend_from_slice(&bytes);
            }
            Ok(None) => return Ok(out),
            Err(err) => {
                return Err((
                    ProtocolError::new(
                        ErrorCode::UpstreamUnavailable,
                        format!("egress download read failed mid-body: {err}"),
                        false,
                    ),
                    false,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard(endpoints: &[&str]) -> EgressGuard {
        let owned: Vec<String> = endpoints.iter().map(|s| s.to_string()).collect();
        EgressGuard::from_provider_endpoints(&owned).expect("guard builds")
    }

    #[test]
    fn guarded_ipv4_literals_refuse_outside_the_endpoint_exception() {
        let guard = guard(&["https://api.example.test/v1"]);
        for url in [
            "http://127.0.0.1:8080/asset.png",
            "http://10.1.2.3/asset.png",
            "http://192.168.3.5/asset.png",
            "http://172.16.9.9/asset.png",
            "http://169.254.169.254/latest/meta-data",
            "http://0.0.0.0/asset.png",
            "http://100.64.0.1/asset.png",
            "http://[::1]/asset.png",
            "http://[fe80::1]/asset.png",
            "http://[fc00::5]/asset.png",
            "http://[::]/asset.png",
        ] {
            let err = guard.check_url(url).expect_err(url);
            assert!(err.message.contains("guarded"), "{url}: {err}",);
        }
    }

    #[test]
    fn the_endpoint_origin_exception_lets_the_loopback_stub_through() {
        let guard = guard(&["http://127.0.0.1:18080/v1"]);
        guard
            .check_url("http://127.0.0.1:18080/files/product.wav")
            .expect("same-origin loopback asset passes");
        // A DIFFERENT loopback port is still refused.
        let err = guard
            .check_url("http://127.0.0.1:9999/files/product.wav")
            .expect_err("different port");
        assert!(err.message.contains("guarded"));
    }

    #[test]
    fn scheme_userinfo_and_host_shapes_refuse() {
        let guard = guard(&["https://api.example.test/v1"]);
        for url in [
            "ftp://example.test/a.png",
            "file:///etc/passwd",
            "https://user:pass@example.test/a.png",
            "http://user@example.test/a.png",
            "https:///a.png",
            "http://[::bad/a.png",
        ] {
            assert!(guard.check_url(url).is_err(), "{url} must refuse");
        }
    }

    #[test]
    fn https_public_passes_and_plain_http_is_same_origin_only() {
        let g = guard(&["https://api.example.test/v1"]);
        g.check_url("https://cdn.provider.example/audio.wav")
            .expect("https public hostname passes");
        // A public literal IP over https is a legitimate destination.
        g.check_url("https://1.1.1.1/asset.png")
            .expect("public literal IP over https passes");
        let err = g
            .check_url("http://cdn.provider.example/audio.wav")
            .expect_err("plain http outside origins");
        assert!(err.message.contains("plain http"));
        // Same-origin plain http (the configured endpoint itself) passes.
        let g = guard(&["http://internal.example.test:8080/api"]);
        g.check_url("http://internal.example.test:8080/files/p.png")
            .expect("same-origin http passes");
    }

    #[test]
    fn ipv4_parsing_is_strict_and_default_ports_elide() {
        assert_eq!(parse_ipv4("127.0.0.1"), Some([127, 0, 0, 1]));
        assert_eq!(parse_ipv4("010.0.0.1"), None, "leading zeros refuse");
        assert_eq!(parse_ipv4("1.2.3"), None);
        assert_eq!(parse_ipv4("1.2.3.4.5"), None);
        assert_eq!(parse_ipv4("256.1.1.1"), None);
        let g = guard(&["https://api.example.test/v1"]);
        g.check_url("https://example.test:443/a.png").expect("443");
        let err = g
            .check_url("http://127.0.0.1/a.png")
            .expect_err("80 vs none");
        assert!(err.message.contains("guarded"));
        // origin_of semantics: :443 on https elides, so the allowlist match
        // treats them as one origin.
        let g = guard(&["https://api.example.test:443/v1"]);
        g.check_url("https://example.test/x.png").expect("elided");
    }

    // fix-r1 / review F-01: EVERY spelling that the WHATWG parser (the one
    // reqwest dials with) normalizes to a guarded address must refuse here —
    // the probe battery the reviewer ran (13 BYPASS rows) plus the
    // percent-encoded and v4-compatible classes, as regressions.
    #[test]
    fn non_decimal_ipv4_spellings_and_mapped_ipv6_refuse_like_their_normalized_form() {
        let guard = guard(&["https://api.example.test/v1"]);
        for url in [
            // hex / octal / short-form / single-integer / leading-zero /
            // trailing-dot IPv4 spellings — all normalize to guarded v4.
            "https://0x7f.0.0.1/asset.png",
            "https://0x7f000001/asset.png",
            "https://2130706433/asset.png",
            "https://0177.0.0.1/asset.png",
            "https://0177.0.0.1:18080/asset.png",
            "https://127.1/asset.png",
            "https://127.000.000.001/asset.png",
            "https://127.0.0.1./asset.png",
            "https://3232235885/asset.png", // = 192.168.1.109
            // percent-encoded host digits normalize to 127.0.0.1
            "https://%31%32%37.0.0.1/asset.png",
            // IPv4-mapped IPv6 folds to the embedded (guarded) v4 policy
            "https://[::ffff:127.0.0.1]/asset.png",
            "https://[::ffff:169.254.169.254]/latest/meta-data",
            "https://[::ffff:10.1.2.3]/asset.png",
            "https://[::ffff:192.168.3.5]/asset.png",
            "https://[::ffff:0.0.0.0]/asset.png",
            // the deprecated IPv4-compatible ::/96 is never public egress
            "https://[::7f00:1]/asset.png",
        ] {
            let err = guard.check_url(url).expect_err(url);
            assert!(err.message.contains("guarded"), "{url}: {err}",);
        }
        // Zone ids refuse at the parse level (the WHATWG parser the client
        // dials with rejects them too — the guard never sees a host).
        assert!(guard
            .check_url("https://[fe80::1%25eth0]/asset.png")
            .is_err());
        // The fold is policy-exact, not a blanket refusal: spellings that
        // normalize to a PUBLIC v4 still pass over https.
        guard
            .check_url("https://[::ffff:8.8.8.8]/asset.png")
            .expect("a mapped PUBLIC v4 passes over https");
        guard
            .check_url("https://0x8.0x8.0x8.0x8/asset.png") // = 8.8.8.8 in hex
            .expect("a hex spelling of a PUBLIC v4 passes over https");
    }

    #[test]
    fn the_same_origin_exception_matches_normalized_origins_across_spellings() {
        // The exception is on the NORMALIZED origin: non-decimal spellings
        // of the loopback stub host still match the endpoint origin.
        let g = guard(&["http://127.0.0.1:18080/v1"]);
        for url in [
            "http://0x7f.0.0.1:18080/files/product.wav",
            "http://2130706433:18080/files/product.wav",
            "http://127.1:18080/files/product.wav",
        ] {
            g.check_url(url).expect("same normalized origin passes");
        }
        // A guarded spelling OUTSIDE the configured origin still refuses.
        let err = g
            .check_url("http://0177.0.0.1:9999/files/product.wav")
            .expect_err("different port");
        assert!(err.message.contains("guarded"));
        // Default-port elision keeps `:443` https and bare https one origin.
        let g = guard(&["https://api.example.test:443/v1"]);
        g.check_url("https://api.example.test/x.png")
            .expect("elided");
    }
}
