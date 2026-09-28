//! WebSocket surface (R02-T03): one-shot upgrade tickets, a real RFC 6455
//! upgrade handshake, and the post-upgrade `lingxi.wire` handshake plus a
//! representative authorized request — all sharing the SAME transport
//! guard/authentication/authorization chain as HTTP.
//!
//! ## Why a hand-rolled upgrade instead of axum's `ws` feature
//!
//! axum 0.8's `ws` feature would add `tokio-tungstenite`/`tungstenite` to
//! `rust/Cargo.lock` — new third-party crates that are not locked anywhere
//! in the workspace. The task rule is "no new dependencies beyond already
//! locked versions", so this module implements the upgrade with the parts
//! that ARE locked: `hyper::upgrade::OnUpgrade` (hyper 1.11.1 is axum's own
//! HTTP implementation) for taking over the connection, plus `sha1` +
//! `base64` for the `Sec-WebSocket-Accept` computation — the exact roles
//! they already play in `lingxi-protocol`'s handshake prototype. The frame
//! codec below is the RFC 6455 subset (text/close/ping/pong, masked client
//! frames, unmasked server frames) with hard size bounds, mirroring the
//! audited prototype implementation in
//! `rust/crates/lingxi-protocol/src/bin/lingxi-proto-server.rs`.
//!
//! ## Ticket lifecycle (mirrors the incumbent `core/ws-auth-ticket.ts`)
//!
//! `POST /lingxi/v1/ws-ticket` (scope `chat`) issues a `hana_ws_…` ticket
//! bound to (principal, connection kind, path) with a short TTL. The ticket
//! is consumed at the upgrade in a single-use `remove` — replay of an
//! already-used ticket, an expired ticket, a ticket presented on a
//! different path or a different connection kind are all rejected with
//! `invalid_ws_ticket` before the 101 is sent.
//!
//! ## Post-upgrade protocol
//!
//! 1. First text frame MUST be a `lingxi.wire` `ClientHello`; the frozen
//!    `negotiate_protocol` decides. Success ⇒ `ServerHello` frame; failure
//!    ⇒ `ProtocolError` frame + close 4409 (the R01-T02 prototype codes).
//! 2. Subsequent text frames are JSON requests `{"type": ...}`:
//!    `session_read` runs the same session-ownership rule as the HTTP read
//!    endpoint (unauthorized ⇒ `ProtocolError` frame + close 4401/4403);
//!    anything else is `invalid_message`.
//! 3. Ping frames are answered with Pong; Close is echoed.

use std::collections::VecDeque;
use std::sync::Mutex;

use base64::Engine as _;
use hyper::upgrade::OnUpgrade;
use serde::{Deserialize, Serialize};
use sha1::{Digest as _, Sha1};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

use crate::auth::AuthDenial;
use crate::transport::ConnectionKind;

/// WS ticket secret prefix (mirrors the incumbent `hana_ws_` shape).
pub const WS_TICKET_PREFIX: &str = "hana_ws_";
/// Default ticket TTL (30 s, incumbent default).
pub const DEFAULT_WS_TICKET_TTL_MS: u64 = 30_000;
/// Default ticket-table bound (incumbent default 512).
pub const DEFAULT_WS_MAX_TICKETS: usize = 512;

/// RFC 6455 GUID for the accept-key computation.
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Computes the `Sec-WebSocket-Accept` value for a client key.
pub fn websocket_accept(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

/// Single-use WS upgrade tickets.
pub struct WsTicketService {
    ttl_ms: u64,
    max_tickets: usize,
    inner: Mutex<TicketBook>,
}

impl std::fmt::Debug for WsTicketService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render live ticket material.
        f.debug_struct("WsTicketService")
            .field("ttl_ms", &self.ttl_ms)
            .field("max_tickets", &self.max_tickets)
            .finish_non_exhaustive()
    }
}

struct TicketBook {
    // Insertion order preserved for overflow pruning (mirrors JS Map).
    order: VecDeque<String>,
    records: std::collections::HashMap<String, TicketRecord>,
}

struct TicketRecord {
    principal: crate::auth::Principal,
    connection_kind: ConnectionKind,
    secure_transport: bool,
    path: String,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedTicket {
    pub ticket: String,
    pub expires_at_unix_ms: u64,
}

impl WsTicketService {
    pub fn new(ttl_ms: u64, max_tickets: usize) -> Self {
        Self {
            ttl_ms: ttl_ms.max(1),
            max_tickets: max_tickets.max(1),
            inner: Mutex::new(TicketBook {
                order: VecDeque::new(),
                records: std::collections::HashMap::new(),
            }),
        }
    }

    /// Issues a ticket bound to (principal, connection kind, path).
    /// R9-F05: the ticket material comes from the system secure random
    /// source ONLY — when it fails, NO ticket is issued and the error is
    /// propagated (the caller answers 5xx; the old predictable xorshift
    /// fallback is gone).
    pub fn issue(
        &self,
        principal: crate::auth::Principal,
        connection_kind: ConnectionKind,
        secure_transport: bool,
        path: &str,
        now_ms: u64,
    ) -> Result<IssuedTicket, crate::auth::AuthSetupError> {
        let ticket = format!("{WS_TICKET_PREFIX}{}", crate::auth::random_base64url(32)?);
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Prune expired + enforce the bound BEFORE inserting (mirrors the
        // incumbent prune order so issuing never grows the table).
        inner
            .records
            .retain(|_, record| record.expires_at_unix_ms > now_ms);
        let live: std::collections::HashSet<String> = inner.records.keys().cloned().collect();
        inner.order.retain(|t| live.contains(t));
        while inner.order.len() + 1 > self.max_tickets {
            if let Some(oldest) = inner.order.pop_front() {
                inner.records.remove(&oldest);
            } else {
                break;
            }
        }
        let expires_at = now_ms.saturating_add(self.ttl_ms);
        inner.order.push_back(ticket.clone());
        inner.records.insert(
            ticket.clone(),
            TicketRecord {
                principal,
                connection_kind,
                secure_transport,
                path: path.to_string(),
                expires_at_unix_ms: expires_at,
            },
        );
        Ok(IssuedTicket {
            ticket,
            expires_at_unix_ms: expires_at,
        })
    }

    /// Consumes a ticket (single use — removed whether or not it validates).
    /// Returns the bound principal when the ticket exists, is unexpired,
    /// matches the path and the connection kind.
    pub fn consume(
        &self,
        ticket: &str,
        connection_kind: ConnectionKind,
        secure_transport: bool,
        path: &str,
        now_ms: u64,
    ) -> Option<crate::auth::Principal> {
        if ticket.trim().is_empty() {
            return None;
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let record = inner.records.remove(ticket)?;
        let live: std::collections::HashSet<String> = inner.records.keys().cloned().collect();
        inner.order.retain(|t| live.contains(t));
        if record.expires_at_unix_ms <= now_ms {
            return None;
        }
        if record.path != path {
            return None;
        }
        if record.connection_kind != connection_kind {
            return None;
        }
        if record.secure_transport != secure_transport {
            return None;
        }
        Some(record.principal)
    }
}

// ── Upgrade extractor ───────────────────────────────────────────────────────

/// Extractor pulling hyper's upgrade handle for a valid RFC 6455 upgrade
/// request. Rejections mirror the handshake requirements (GET, Connection:
/// upgrade, Upgrade: websocket, Sec-WebSocket-Version: 13, a client key).
pub struct WsUpgrade {
    pub sec_websocket_key: String,
    pub on_upgrade: OnUpgrade,
}

/// Rejection of the upgrade request shape (plain status + reason; the
/// middleware auth chain has usually already answered before this runs).
#[derive(Debug)]
pub struct WsUpgradeRejection {
    pub status: u16,
    pub reason: &'static str,
}

impl axum::response::IntoResponse for WsUpgradeRejection {
    fn into_response(self) -> axum::response::Response {
        (
            axum::http::StatusCode::from_u16(self.status)
                .unwrap_or(axum::http::StatusCode::BAD_REQUEST),
            axum::Json(serde_json::json!({
                "code": "invalid_message",
                "message": self.reason,
                "retryable": false,
                "details": { "reason": self.reason }
            })),
        )
            .into_response()
    }
}

impl<S> axum::extract::FromRequestParts<S> for WsUpgrade
where
    S: Send + Sync,
{
    type Rejection = WsUpgradeRejection;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        use axum::http::header;

        let header_eq = |name: header::HeaderName, expected: &str| -> bool {
            parts
                .headers
                .get(&name)
                .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(expected.as_bytes()))
        };
        let header_has_token = |name: header::HeaderName, expected: &str| -> bool {
            parts.headers.get_all(name).iter().any(|value| {
                value.to_str().is_ok_and(|text| {
                    text.split(',')
                        .any(|token| token.trim().eq_ignore_ascii_case(expected))
                })
            })
        };

        if parts.method != axum::http::Method::GET {
            return Err(WsUpgradeRejection {
                status: 405,
                reason: "ws upgrade requires GET",
            });
        }
        if !header_has_token(header::CONNECTION, "upgrade") {
            return Err(WsUpgradeRejection {
                status: 400,
                reason: "missing Connection: upgrade",
            });
        }
        if !header_eq(header::UPGRADE, "websocket") {
            return Err(WsUpgradeRejection {
                status: 400,
                reason: "missing Upgrade: websocket",
            });
        }
        if !header_eq(header::SEC_WEBSOCKET_VERSION, "13") {
            return Err(WsUpgradeRejection {
                status: 400,
                reason: "unsupported Sec-WebSocket-Version",
            });
        }
        let mut keys = parts.headers.get_all(header::SEC_WEBSOCKET_KEY).iter();
        let key = keys.next().and_then(|value| value.to_str().ok());
        let sec_websocket_key = key.map(str::trim).unwrap_or_default();
        let key_is_valid = keys.next().is_none()
            && base64::engine::general_purpose::STANDARD
                .decode(sec_websocket_key)
                .is_ok_and(|bytes| bytes.len() == 16);
        if !key_is_valid {
            return Err(WsUpgradeRejection {
                status: 400,
                reason: "missing or invalid Sec-WebSocket-Key",
            });
        }
        let on_upgrade = parts
            .extensions
            .remove::<OnUpgrade>()
            .ok_or(WsUpgradeRejection {
                status: 400,
                reason: "connection not upgradable",
            })?;
        Ok(Self {
            sec_websocket_key: sec_websocket_key.to_string(),
            on_upgrade,
        })
    }
}

/// Builds the 101 Switching Protocols response for a validated client key.
pub fn switching_protocols_response(key: &str) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    axum::response::Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(header::UPGRADE, "websocket")
        .header(header::CONNECTION, "Upgrade")
        .header(header::SEC_WEBSOCKET_ACCEPT, websocket_accept(key))
        .body(axum::body::Body::empty())
        .unwrap_or_else(|err| {
            // Building this static response cannot fail; if it ever does,
            // fail loudly instead of serving a malformed handshake.
            panic!("cannot build 101 Switching Protocols response: {err}")
        })
}

// ── Frame codec (RFC 6455 subset, bounded) ─────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum WsFrame {
    Text(Vec<u8>),
    Close(u16, String),
    Ping(Vec<u8>),
    Pong,
}

/// 单一读任务保留帧的中间读取状态；主循环选择事件或定时检查时只会
/// 取消队列接收，不会取消已读取一半的帧。队列容量 1，不能无限预读。
pub(crate) struct ClientFrameReader {
    frames: tokio::sync::mpsc::Receiver<std::io::Result<Option<WsFrame>>>,
    tasks: tokio::task::JoinSet<()>,
}

impl ClientFrameReader {
    pub(crate) fn new<IO>(mut reader: IO) -> Self
    where
        IO: AsyncRead + Unpin + Send + 'static,
    {
        let (sender, frames) = tokio::sync::mpsc::channel(1);
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            loop {
                let frame = read_client_ws_frame(&mut reader).await;
                let terminal = !matches!(frame, Ok(Some(_)));
                if sender.send(frame).await.is_err() || terminal {
                    break;
                }
            }
        });
        Self { frames, tasks }
    }

    pub(crate) async fn recv(&mut self) -> std::io::Result<Option<WsFrame>> {
        self.frames
            .recv()
            .await
            .unwrap_or_else(|| Err(std::io::Error::other("ws frame reader stopped")))
    }

    pub(crate) async fn shutdown(&mut self) {
        self.tasks.shutdown().await;
    }
}

/// 读取服务端帧；服务端允许未掩码。首字节前结束返回 `None`，超限帧报协议错误。
pub async fn read_ws_frame<IO>(io: &mut IO) -> std::io::Result<Option<WsFrame>>
where
    IO: AsyncRead + Unpin,
{
    read_frame(io, false).await
}

/// 读取已建立连接的客户端帧；RFC 6455 §5.1 要求所有客户端帧（含 ping/close）都掩码。
pub async fn read_client_ws_frame<IO>(io: &mut IO) -> std::io::Result<Option<WsFrame>>
where
    IO: AsyncRead + Unpin,
{
    read_frame(io, true).await
}

async fn read_frame<IO>(io: &mut IO, require_mask: bool) -> std::io::Result<Option<WsFrame>>
where
    IO: AsyncRead + Unpin,
{
    let mut header = [0u8; 2];
    match io.read_exact(&mut header[..1]).await {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    }
    io.read_exact(&mut header[1..]).await.map_err(|err| {
        if err.kind() == std::io::ErrorKind::UnexpectedEof {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "incomplete frame header")
        } else {
            err
        }
    })?;
    if header[0] & 0x70 != 0 || header[0] & 0x80 == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsupported fragmented or RSV frame",
        ));
    }
    let opcode = header[0] & 0x0f;
    let masked = header[1] & 0x80 != 0;
    if require_mask && !masked {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unmasked client frame",
        ));
    }
    let mut len = u64::from(header[1] & 0x7f);
    if len == 126 {
        let mut ext = [0u8; 2];
        io.read_exact(&mut ext).await?;
        len = u64::from(u16::from_be_bytes(ext));
    } else if len == 127 {
        let mut ext = [0u8; 8];
        io.read_exact(&mut ext).await?;
        len = u64::from_be_bytes(ext);
    }
    if len > crate::limits::WS_FRAME_LIMIT_BYTES as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    if matches!(opcode, 0x8..=0xA) && len > 125 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "control frame too large",
        ));
    }
    let mask = if masked {
        let mut m = [0u8; 4];
        io.read_exact(&mut m).await?;
        Some(m)
    } else {
        None
    };
    let mut payload = vec![0u8; len as usize];
    io.read_exact(&mut payload).await?;
    if let Some(m) = mask {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= m[i % 4];
        }
    }
    Ok(Some(match opcode {
        0x1 => WsFrame::Text(payload),
        0x8 => {
            if payload.len() == 1 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid close frame",
                ));
            }
            let code = if payload.len() >= 2 {
                u16::from_be_bytes([payload[0], payload[1]])
            } else {
                1005
            };
            let reason =
                String::from_utf8(payload[2.min(payload.len())..].to_vec()).map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid close reason")
                })?;
            WsFrame::Close(code, reason)
        }
        0x9 => WsFrame::Ping(payload),
        0xA => WsFrame::Pong,
        other => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported opcode {other:#x}"),
            ))
        }
    }))
}

/// Writes one server frame (never masked, per RFC 6455 server rules).
pub async fn write_ws_frame<IO>(io: &mut IO, opcode: u8, payload: &[u8]) -> std::io::Result<()>
where
    IO: AsyncWrite + Unpin,
{
    let mut out = vec![0x80 | opcode];
    if payload.len() < 126 {
        out.push(payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        out.push(126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    io.write_all(&out).await?;
    io.flush().await
}

pub async fn write_ws_text<IO>(io: &mut IO, payload: &[u8]) -> std::io::Result<()>
where
    IO: AsyncWrite + Unpin,
{
    write_ws_frame(io, 0x1, payload).await
}

pub async fn write_ws_close<IO>(io: &mut IO, code: u16, reason: &str) -> std::io::Result<()>
where
    IO: AsyncWrite + Unpin,
{
    let mut payload = code.to_be_bytes().to_vec();
    payload.extend_from_slice(reason.as_bytes());
    write_ws_frame(io, 0x8, &payload).await
}

// ── WS request vocabulary (post-handshake) ─────────────────────────────────

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", deny_unknown_fields)]
pub enum WsClientRequest {
    SessionRead {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    /// R02-T05 event subscription: `streamId` + optional server-issued
    /// `cursor` (resume). Answers with a `subscribed`/`snapshot_required`
    /// CONTROL frame (see `events.rs`), then business-event frames —
    /// which are bare canonical `EventEnvelope` objects (PROTOCOL_SPEC
    /// §5), never mixed into the control class.
    SubscribeEvents {
        #[serde(rename = "streamId")]
        stream_id: String,
        #[serde(default)]
        cursor: Option<lingxi_protocol::Cursor>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum WsServerMessage {
    SessionReadResult { session_id: String, run_count: u64 },
}

/// WS close codes used by this surface (the R01-T02 prototype already
/// established 4409 = version_incompatible; authorization failures get the
/// 4401/4403/4404 family; final mapping across all surfaces is tracked as
/// RR-T02-F5 for R08).
pub const WS_CLOSE_INVALID_MESSAGE: u16 = 4409;
pub const WS_CLOSE_UNAUTHORIZED: u16 = 4401;
pub const WS_CLOSE_FORBIDDEN: u16 = 4403;
pub const WS_CLOSE_NOT_FOUND: u16 = 4404;
/// RFC 6455 1013 "Try Again Later" — the close code for an explicit
/// resource-cap rejection (R02-T07: subscriber/connection registry full;
/// the client may retry after backing off).
pub const WS_CLOSE_TRY_AGAIN_LATER: u16 = 1013;

/// Authentication shape used at the upgrade boundary: bearer/query token or
/// a one-shot ws ticket. Defined here so both the HTTP middleware and the
/// WS handler share one enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsCredential {
    Bearer(String),
    QueryToken(String),
    Ticket(String),
}

/// Extracts the WS credential from a request's headers + query string.
/// Mirrors the incumbent `/ws` chain: `Authorization` bearer, `?token=`
/// (local connections only — enforced by the caller passing the connection
/// kind), `?wsTicket=` one-shot.
pub fn ws_credential_from(authorization: Option<&str>, query: &str) -> Option<WsCredential> {
    if let Some(bearer) = crate::auth::parse_bearer(authorization) {
        return Some(WsCredential::Bearer(bearer));
    }
    for (key, value) in parse_query_pairs(query) {
        match key.as_str() {
            "token" => return Some(WsCredential::QueryToken(value)),
            "wsTicket" => return Some(WsCredential::Ticket(value)),
            _ => {}
        }
    }
    None
}

/// Minimal `application/x-www-form-urlencoded` pair parser for the two
/// query keys we read (percent-decoding only the %XX forms).
pub fn parse_query_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

fn percent_decode(input: &str) -> String {
    fn hex_value(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // Decode on raw bytes instead of slicing &str: '%' followed by
            // multi-byte UTF-8 must never be sliced mid-char (that panics).
            b'%' => match (bytes.get(i + 1), bytes.get(i + 2)) {
                (Some(&hi), Some(&lo)) => match (hex_value(hi), hex_value(lo)) {
                    (Some(hi), Some(lo)) => {
                        out.push(hi << 4 | lo);
                        i += 3;
                    }
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                },
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Denial of a WS upgrade (status + machine reason), parallel to the HTTP
/// middleware denial body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsUpgradeDenial {
    pub status: u16,
    pub reason: &'static str,
}

impl From<AuthDenial> for WsUpgradeDenial {
    fn from(denial: AuthDenial) -> Self {
        Self {
            status: 401,
            reason: denial.reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(user: &str) -> crate::auth::Principal {
        use crate::auth::{ConnectionKindSerde, CredentialKind, PrincipalKind, TrustState};
        crate::auth::Principal {
            schema_version: 1,
            principal_id: format!("principal_test_{user}"),
            kind: PrincipalKind::Device,
            user_id: Some(user.to_string()),
            studio_id: None,
            server_node_id: None,
            device_id: Some("device_t".to_string()),
            credential_id: Some("cred_t".to_string()),
            web_session_id: None,
            connection_kind: ConnectionKindSerde::Lan,
            credential_kind: CredentialKind::DeviceCredential,
            trust_state: TrustState::Lan,
            scopes: vec!["chat".to_string()],
        }
    }

    #[test]
    fn websocket_accept_known_vector() {
        // RFC 6455 §1.3 example vector.
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn ticket_lifecycle_single_use_expiry_and_binding() {
        let svc = WsTicketService::new(1000, 8);
        let p = principal("user_a");
        // R9-F05: issue() is system-CSPRNG-only and fallible; this machine
        // has a working source, so issuance succeeds.
        let issued = svc
            .issue(p.clone(), ConnectionKind::Local, false, "/lingxi/v1/ws", 0)
            .expect("system CSPRNG available");
        assert!(issued.ticket.starts_with("hana_ws_"));

        // Consume with the right binding → principal.
        let got = svc
            .consume(
                &issued.ticket,
                ConnectionKind::Local,
                false,
                "/lingxi/v1/ws",
                500,
            )
            .unwrap();
        assert_eq!(got, p);

        // Replay: already consumed.
        assert!(svc
            .consume(
                &issued.ticket,
                ConnectionKind::Local,
                false,
                "/lingxi/v1/ws",
                600
            )
            .is_none());

        // Expired ticket.
        let exp = svc
            .issue(p.clone(), ConnectionKind::Local, false, "/lingxi/v1/ws", 0)
            .expect("system CSPRNG available");
        assert!(svc
            .consume(
                &exp.ticket,
                ConnectionKind::Local,
                false,
                "/lingxi/v1/ws",
                2000
            )
            .is_none());

        // Wrong connection kind / wrong path.
        let bound = svc
            .issue(p.clone(), ConnectionKind::Local, false, "/lingxi/v1/ws", 0)
            .expect("system CSPRNG available");
        assert!(svc
            .consume(
                &bound.ticket,
                ConnectionKind::Lan,
                false,
                "/lingxi/v1/ws",
                100
            )
            .is_none());
        let bound2 = svc
            .issue(p, ConnectionKind::Local, false, "/lingxi/v1/ws", 0)
            .expect("system CSPRNG available");
        assert!(svc
            .consume(&bound2.ticket, ConnectionKind::Local, false, "/other", 100)
            .is_none());

        // Empty ticket string.
        assert!(svc
            .consume("", ConnectionKind::Local, false, "/lingxi/v1/ws", 100)
            .is_none());

        // 同一连接类型但安全传输属性不同也不能复用票据。
        let secure = svc
            .issue(
                principal("user_b"),
                ConnectionKind::Lan,
                true,
                "/lingxi/v1/ws",
                0,
            )
            .unwrap();
        assert!(svc
            .consume(
                &secure.ticket,
                ConnectionKind::Lan,
                false,
                "/lingxi/v1/ws",
                100
            )
            .is_none());
    }

    #[test]
    fn ticket_table_is_bounded_and_prunes_expired() {
        let svc = WsTicketService::new(1000, 4);
        let p = principal("user_a");
        for i in 0..10 {
            svc.issue(p.clone(), ConnectionKind::Local, false, "/lingxi/v1/ws", i)
                .expect("system CSPRNG available");
        }
        let inner = svc.inner.lock().unwrap();
        assert!(
            inner.records.len() <= 4,
            "table must stay bounded, got {}",
            inner.records.len()
        );
        assert!(inner.order.len() <= 4);
    }

    #[tokio::test]
    async fn frame_codec_roundtrip_through_memory() {
        // Use a duplex pipe to prove write→read roundtrip of the codec.
        let (mut client, mut server) = tokio::io::duplex(4096);
        let payload = b"{\"type\":\"session_read\"}";
        let handle = tokio::spawn(async move {
            write_ws_text(&mut client, payload).await.unwrap();
        });
        let frame = read_ws_frame(&mut server).await.unwrap();
        handle.await.unwrap();
        assert_eq!(frame, Some(WsFrame::Text(payload.to_vec())));
    }

    #[tokio::test]
    async fn client_frames_require_mask_for_text_and_control_opcodes() {
        for opcode in [0x1u8, 0x8, 0x9, 0xA] {
            let mut unmasked = std::io::Cursor::new(vec![0x80 | opcode, 0]);
            let err = read_client_ws_frame(&mut unmasked).await.unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
            assert_eq!(err.to_string(), "unmasked client frame");
        }

        let payload = b"hello";
        let mask = [0x11, 0x22, 0x33, 0x44];
        let mut masked = vec![0x81, 0x80 | payload.len() as u8];
        masked.extend_from_slice(&mask);
        masked.extend(
            payload
                .iter()
                .enumerate()
                .map(|(i, byte)| byte ^ mask[i % 4]),
        );
        let mut cursor = std::io::Cursor::new(masked);
        assert_eq!(
            read_client_ws_frame(&mut cursor).await.unwrap(),
            Some(WsFrame::Text(payload.to_vec()))
        );
    }

    #[tokio::test]
    async fn malformed_client_frames_fail_closed() {
        for frame in [
            vec![0x81],                      // 半截帧头不是正常断开
            vec![0x01, 0x80],                // 此子集不支持分片
            vec![0xC1, 0x80],                // 未协商的 RSV 位
            vec![0x88, 0x81, 0, 0, 0, 0, 0], // close 长度 1 非法
        ] {
            let mut cursor = std::io::Cursor::new(frame);
            assert_eq!(
                read_client_ws_frame(&mut cursor).await.unwrap_err().kind(),
                std::io::ErrorKind::InvalidData,
            );
        }
    }

    #[tokio::test]
    async fn client_reader_keeps_partial_frame_across_other_select_branches() {
        let (mut client, server) = tokio::io::duplex(128);
        let (reader, _writer) = tokio::io::split(server);
        let mut framed = ClientFrameReader::new(reader);
        let mask = [1_u8, 2, 3, 4];
        client.write_all(&[0x81, 0x80 | 5, mask[0]]).await.unwrap();
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {},
            frame = framed.recv() => panic!("partial frame was delivered: {frame:?}"),
        }
        let payload = b"hello";
        let mut remainder = mask[1..].to_vec();
        remainder.extend(
            payload
                .iter()
                .enumerate()
                .map(|(i, byte)| byte ^ mask[i % 4]),
        );
        client.write_all(&remainder).await.unwrap();
        assert_eq!(
            framed.recv().await.unwrap(),
            Some(WsFrame::Text(payload.to_vec()))
        );
        framed.shutdown().await;
    }

    #[test]
    fn upgrade_rejection_preserves_its_status() {
        use axum::response::IntoResponse as _;
        let response = WsUpgradeRejection {
            status: 400,
            reason: "missing Upgrade: websocket",
        }
        .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected_not_buffered() {
        // Hand-roll a frame header declaring a giant length; the codec must
        // refuse without allocating.
        let mut huge = vec![0x81, 0xFF];
        huge.extend_from_slice(&u64::MAX.to_be_bytes());
        let mut cursor = std::io::Cursor::new(huge);
        let result = read_ws_frame(&mut cursor).await;
        assert!(result.is_err(), "giant frame must be a protocol error");
    }

    #[test]
    fn ws_request_vocabulary_parses_and_rejects_unknown() {
        let req: WsClientRequest =
            serde_json::from_str(r#"{"type":"session_read","sessionId":"s1"}"#).unwrap();
        assert_eq!(
            req,
            WsClientRequest::SessionRead {
                session_id: "s1".to_string()
            }
        );
        assert!(serde_json::from_str::<WsClientRequest>(r#"{"type":"exec"}"#).is_err());
        assert!(serde_json::from_str::<WsClientRequest>(
            r#"{"type":"session_read","sessionId":"s","principalId":"x"}"#
        )
        .is_err());
    }

    #[test]
    fn query_pair_parsing() {
        let pairs = parse_query_pairs("wsTicket=abc%2Fdef&x=1&empty=");
        assert_eq!(
            pairs,
            vec![
                ("wsTicket".to_string(), "abc/def".to_string()),
                ("x".to_string(), "1".to_string()),
                ("empty".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn query_pair_parsing_unicode_and_bad_escapes_never_panic() {
        // R2-F02 same-root regression: '%' adjacent to multi-byte UTF-8
        // used to be sliced mid-char and panic. Bad escapes pass through
        // verbatim; valid ones still decode.
        let pairs = parse_query_pairs("wsTicket=%zz&x=%e2%80%94&raw=%—&half=%2");
        assert_eq!(pairs[0], ("wsTicket".to_string(), "%zz".to_string()));
        assert_eq!(pairs[1], ("x".to_string(), "—".to_string()));
        assert_eq!(pairs[2], ("raw".to_string(), "%—".to_string()));
        assert_eq!(pairs[3], ("half".to_string(), "%2".to_string()));
        // A bare non-ASCII query never panics and round-trips.
        let pairs = parse_query_pairs("note=中文—é🙂");
        assert_eq!(pairs, vec![("note".to_string(), "中文—é🙂".to_string())]);
    }

    #[test]
    fn ws_credential_extraction_order() {
        assert_eq!(
            ws_credential_from(Some("Bearer tok1"), ""),
            Some(WsCredential::Bearer("tok1".to_string()))
        );
        assert_eq!(
            ws_credential_from(None, "token=tok2"),
            Some(WsCredential::QueryToken("tok2".to_string()))
        );
        assert_eq!(
            ws_credential_from(None, "wsTicket=tok3"),
            Some(WsCredential::Ticket("tok3".to_string()))
        );
        assert_eq!(ws_credential_from(None, ""), None);
        assert_eq!(ws_credential_from(Some("Basic x"), ""), None);
    }
}
