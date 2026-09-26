//! Transport-boundary policy: network mode, connection-kind inference, and
//! the Origin/Host guards that keep a malicious web page from riding a
//! loopback listener (R02-T03 step 1/3, acceptance R02-A06).
//!
//! ## Network mode
//!
//! `Loopback` is the default and the only mode a plain start can end up in;
//! `Lan` requires the explicit `--network-mode lan` flag. In loopback mode
//! the bind address must itself be loopback (a non-loopback `--bind` with
//! the default mode is a loud configuration error, never a silent LAN
//! exposure).
//!
//! ## Connection kind (mirrors the incumbent `server/http/transport-context.ts`)
//!
//! The Host header decides what the *client thinks* it is talking to; the
//! remote address decides where the connection *actually* came from. Both
//! must agree with the configured network mode before any authentication
//! runs:
//!
//! - loopback mode: Host must be a loopback host (DNS-rebinding guard —
//!   a rebinding attack arrives with a foreign Host), remote (when known)
//!   must be loopback ⇒ [`ConnectionKind::Local`];
//! - lan mode: loopback Host + loopback remote still classifies as `Local`
//!   (the loopback token keeps working for on-machine callers); anything
//!   else is `Lan`.
//!
//! A request whose kind cannot be inferred is rejected with
//! `invalid_transport` *before* authentication — the same fail-closed order
//! the incumbent Node middleware uses.
//!
//! ## Origin policy (documented and tested, see R02-A06)
//!
//! Browsers always attach an `Origin` header to cross-origin HTTP requests
//! and to **every** WebSocket handshake; a local CLI/curl does not. The
//! policy therefore is:
//!
//! - `Origin` **absent** ⇒ allowed. The caller is a non-browser client
//!   (CLI/curl/desktop native). A web page cannot strip its own `Origin`,
//!   so this branch is unreachable for browser-driven traffic. This is the
//!   documented CLI shape and it is covered by the acceptance matrix.
//! - `Origin` present ⇒ it must be in the allowlist (mirrors the incumbent
//!   `server/http/cors-policy.ts`): `http(s)://localhost[:port]`,
//!   `http(s)://127.0.0.1[:port]`, `http(s)://[::1][:port]` (loopback
//!   additions beyond the incumbent regex are documented), the Electron
//!   file:// forms (`file://`, `file:///`) and the sandboxed-origin string
//!   `null` (browsers emit `null` for `file://` pages; an http(s) page can
//!   never produce it). Everything else — `http://evil.example`, `null`
//!   forgeries over plain HTTP from remote pages, exotic schemes — is
//!   rejected with `bad_origin` before authentication.

use std::net::SocketAddr;

/// Configured network exposure of the listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    /// Default: loopback bind + loopback-only transport semantics.
    Loopback,
    /// Explicit opt-in (`--network-mode lan`): non-loopback binds allowed;
    /// loopback tokens stop working for non-local connections.
    Lan,
}

impl NetworkMode {
    /// Parses the strict CLI vocabulary (`loopback` / `lan`). Anything else
    /// is `None` so the CLI layer can fail loudly with the offending value.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "loopback" => Some(Self::Loopback),
            "lan" => Some(Self::Lan),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loopback => "loopback",
            Self::Lan => "lan",
        }
    }
}

impl std::fmt::Display for NetworkMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Transport classification of one incoming connection (R02 surface: local
/// and lan; the incumbent `custom_remote`/`relay`/`cloud` kinds stay out of
/// scope until a stage actually serves them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionKind {
    Local,
    Lan,
}
impl ConnectionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Lan => "lan",
        }
    }
}

/// Why a connection could not be classified (wire-stable reason codes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportRejection {
    /// Host header is not a loopback host while the server is in loopback
    /// mode — the DNS-rebinding signature.
    LoopbackHostMismatch,
    /// Remote address is not loopback while the server is in loopback mode.
    LoopbackRemoteMismatch,
    InvalidNetworkMode,
}

impl TransportRejection {
    pub fn reason_code(self) -> &'static str {
        match self {
            Self::LoopbackHostMismatch => "loopback_host_mismatch",
            Self::LoopbackRemoteMismatch => "loopback_remote_mismatch",
            Self::InvalidNetworkMode => "invalid_network_mode",
        }
    }
}

/// Normalizes a Host header to a bare host: lowercases, strips the port
/// (once, IPv6-safe) and keeps bracketed IPv6 forms intact. Mirrors the
/// incumbent `normalizeHostHeader`.
fn normalize_host_header(host_header: &str) -> String {
    let raw = host_header.trim().to_ascii_lowercase();
    if raw.is_empty() {
        return raw;
    }
    if raw.starts_with('[') {
        if let Some(end) = raw.find(']') {
            return raw[..=end].to_string();
        }
    }
    let colon = raw.rfind(':');
    if let Some(colon) = colon {
        if raw.find(':') == Some(colon) {
            return raw[..colon].to_string();
        }
    }
    raw
}

/// Is this Host header value a loopback host? (`localhost`, `127.0.0.1`
/// style v4 loopback, `::1` / `[::1]`).
pub fn host_is_loopback(host_header: &str) -> bool {
    let host = normalize_host_header(host_header);
    match host.as_str() {
        "" => false,
        "localhost" | "::1" | "[::1]" => true,
        other => other.strip_prefix("127.").is_some_and(|rest| {
            // Strict dotted-quad: exactly three more dot-separated
            // decimal octets, each within u8 range (stricter than the
            // incumbent's `\d{1,3}` regex — malformed Hosts like
            // 127.0.0.256 fail closed here).
            let mut parts = rest.split('.');
            let mut count = 0usize;
            let mut all_ok = true;
            for part in parts.by_ref() {
                count += 1;
                if count > 3
                    || part.is_empty()
                    || part.len() > 3
                    || !part.bytes().all(|b| b.is_ascii_digit())
                    || part.parse::<u8>().is_err()
                {
                    all_ok = false;
                }
            }
            all_ok && count == 3
        }),
    }
}

/// Infers the connection kind. Mirrors the incumbent
/// `inferHttpConnectionKind`: Host decides what the client believes it is
/// talking to, remote address where the bytes actually came from; loopback
/// mode demands both be loopback.
pub fn infer_connection_kind(
    host_header: &str,
    remote: Option<SocketAddr>,
    mode: NetworkMode,
) -> Result<ConnectionKind, TransportRejection> {
    let host_is_loopback = host_is_loopback(host_header);
    let remote_is_loopback = remote.map(|addr| addr.ip().is_loopback());

    match mode {
        NetworkMode::Loopback => {
            if !host_is_loopback {
                return Err(TransportRejection::LoopbackHostMismatch);
            }
            if remote_is_loopback == Some(false) {
                return Err(TransportRejection::LoopbackRemoteMismatch);
            }
            Ok(ConnectionKind::Local)
        }
        NetworkMode::Lan => {
            if host_is_loopback && remote_is_loopback == Some(true) {
                return Ok(ConnectionKind::Local);
            }
            Ok(ConnectionKind::Lan)
        }
    }
}

/// Verdict of the Origin policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginVerdict {
    /// No `Origin` header at all — the documented non-browser (CLI/curl)
    /// shape; allowed.
    Absent,
    /// `Origin` present and matched the allowlist.
    Allowed,
    /// `Origin` present but foreign; must be rejected before auth.
    Forbidden,
}

/// Applies the documented Origin allowlist (see module docs). `null` is the
/// browser-emitted sandboxed/file origin string, not a missing header.
pub fn check_origin(origin_header: Option<&str>) -> OriginVerdict {
    let Some(origin) = origin_header else {
        return OriginVerdict::Absent;
    };
    let value = origin.trim();
    if value.is_empty() {
        // An empty Origin header is not the absent-Header shape; treat as
        // foreign (fail closed).
        return OriginVerdict::Forbidden;
    }
    if value == "null" || value == "file://" || value == "file:///" {
        return OriginVerdict::Allowed;
    }
    for scheme in ["http://", "https://"] {
        let Some(rest) = value.strip_prefix(scheme) else {
            continue;
        };
        let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = normalize_host_header(host_port);
        return match host.as_str() {
            "localhost" | "127.0.0.1" | "[::1]" | "::1" => OriginVerdict::Allowed,
            _ => OriginVerdict::Forbidden,
        };
    }
    OriginVerdict::Forbidden
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> SocketAddr {
        "127.0.0.5:1234".parse().unwrap()
    }
    fn remote() -> SocketAddr {
        "192.168.1.9:4444".parse().unwrap()
    }

    #[test]
    fn network_mode_parses_strictly() {
        assert_eq!(NetworkMode::parse("loopback"), Some(NetworkMode::Loopback));
        assert_eq!(NetworkMode::parse("lan"), Some(NetworkMode::Lan));
        assert_eq!(
            NetworkMode::parse("LAN"),
            None,
            "strict vocabulary, no case games"
        );
        assert_eq!(NetworkMode::parse("internet"), None);
        assert_eq!(NetworkMode::parse(""), None);
    }

    #[test]
    fn host_loopback_detection_covers_v4_v6_and_ports() {
        for host in [
            "localhost",
            "LOCALHOST:8080",
            "127.0.0.1",
            "127.0.0.1:62442",
            "127.1.2.3",
            "::1",
            "[::1]:9000",
            "[::1]",
        ] {
            assert!(host_is_loopback(host), "{host} must be loopback");
        }
        for host in [
            "evil.example",
            "evil.example:8080",
            "sub.localhost",
            "127.0.0.1.evil.example",
            "0.0.0.0",
            "192.168.1.4",
            "localhost.evil.example",
            "127.0.0",
            "127.0.0.256",
            "",
        ] {
            assert!(!host_is_loopback(host), "{host} must NOT be loopback");
        }
    }

    #[test]
    fn loopback_mode_requires_loopback_host_and_remote() {
        assert_eq!(
            infer_connection_kind("127.0.0.1:9000", Some(local()), NetworkMode::Loopback),
            Ok(ConnectionKind::Local)
        );
        // Unknown remote (e.g. unix socket shape) still classifies by Host.
        assert_eq!(
            infer_connection_kind("localhost", None, NetworkMode::Loopback),
            Ok(ConnectionKind::Local)
        );
        // DNS rebinding: foreign Host on loopback listener.
        assert_eq!(
            infer_connection_kind("evil.example", Some(local()), NetworkMode::Loopback),
            Err(TransportRejection::LoopbackHostMismatch)
        );
        // Remote bytes not from loopback while the server is loopback-only.
        assert_eq!(
            infer_connection_kind("localhost", Some(remote()), NetworkMode::Loopback),
            Err(TransportRejection::LoopbackRemoteMismatch)
        );
    }

    #[test]
    fn lan_mode_still_recognizes_local_connections() {
        assert_eq!(
            infer_connection_kind("127.0.0.1:9", Some(local()), NetworkMode::Lan),
            Ok(ConnectionKind::Local)
        );
        assert_eq!(
            infer_connection_kind("my-nas.local", Some(remote()), NetworkMode::Lan),
            Ok(ConnectionKind::Lan)
        );
        // Loopback host with a non-loopback remote is NOT local (spoofed Host).
        assert_eq!(
            infer_connection_kind("localhost", Some(remote()), NetworkMode::Lan),
            Ok(ConnectionKind::Lan)
        );
    }

    #[test]
    fn origin_policy_allowlist() {
        assert_eq!(check_origin(None), OriginVerdict::Absent);
        assert_eq!(
            check_origin(Some("http://localhost:5173")),
            OriginVerdict::Allowed
        );
        assert_eq!(
            check_origin(Some("http://127.0.0.1:62442")),
            OriginVerdict::Allowed
        );
        assert_eq!(
            check_origin(Some("https://localhost")),
            OriginVerdict::Allowed
        );
        assert_eq!(
            check_origin(Some("http://[::1]:8080")),
            OriginVerdict::Allowed
        );
        assert_eq!(check_origin(Some("null")), OriginVerdict::Allowed);
        assert_eq!(check_origin(Some("file://")), OriginVerdict::Allowed);
        assert_eq!(check_origin(Some("file:///")), OriginVerdict::Allowed);

        assert_eq!(
            check_origin(Some("http://evil.example")),
            OriginVerdict::Forbidden
        );
        assert_eq!(
            check_origin(Some("https://evil.example:443/path")),
            OriginVerdict::Forbidden
        );
        // Subdomain of localhost and rebinding-style hosts are NOT allowed.
        assert_eq!(
            check_origin(Some("http://sub.localhost:1")),
            OriginVerdict::Forbidden
        );
        assert_eq!(
            check_origin(Some("http://127.0.0.1.evil.example")),
            OriginVerdict::Forbidden
        );
        // Exotic schemes and empty values fail closed.
        assert_eq!(
            check_origin(Some("ws://localhost")),
            OriginVerdict::Forbidden
        );
        assert_eq!(check_origin(Some("")), OriginVerdict::Forbidden);
        assert_eq!(
            check_origin(Some("chrome-extension://abc")),
            OriginVerdict::Forbidden
        );
    }
}
