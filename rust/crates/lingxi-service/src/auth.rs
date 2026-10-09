//! Authentication and authorization service (R02-T03 step 2/3/4).
//!
//! This module carries the semantics of the three incumbent credential
//! contracts into the Rust service (semantics, exercised with synthetic
//! credentials inside an isolated data root — never real user credentials
//! or the real user directory):
//!
//! 1. **Local loopback token** (incumbent `SERVER_TOKEN` /
//!    `core/server-auth.ts`): a per-start 128-bit hex secret, the
//!    highest-privilege on-machine credential, persisted owner-only (0600)
//!    next to the instance record exactly like the incumbent
//!    `server-info.json` token. It only ever authenticates a `local`
//!    connection — presenting it over a LAN connection is denied with
//!    `loopback_token_requires_local_transport`.
//! 2. **Device credentials** (incumbent `core/device-registry.ts`):
//!    `hana_dev_<base64url>` secrets stored as `secretPrefix` +
//!    salted hash + status + scopes + optional expiry, with real
//!    create/read/expiry/revocation semantics. New secrets use the incumbent
//!    scrypt parameters and base64url encoding; credentials previously
//!    issued by this Rust implementation with iterated SHA-256 remain valid.
//!    Registry field shape still uses unix-millisecond integers instead of
//!    the incumbent ISO-8601 strings.
//! 3. **Principal model** (incumbent `core/security-principal.ts`): the
//!    normalized principal vocabulary (`kind`/`credentialKind`/
//!    `connectionKind`/`trustState`/scopes) with derived principalId.
//!    Identities are created ONLY here at the trust boundary (token
//!    verification); request-supplied identity fields are never read.
//!    Web 会话 Cookie 由管理服务处理，并通过请求守卫进入同一套路由权限表。
//!
//! ## Endpoint permission table (deliverable)
//!
//! [`classify_route`] is the single authority; the table it encodes:
//!
//! | Method+Path                                | Policy              |
//! |--------------------------------------------|---------------------|
//! | GET  /lingxi/v1/health                     | public              |
//! | GET  /lingxi/v1/me                         | authenticated       |
//! | POST /lingxi/v1/ws-ticket                  | scope `chat`        |
//! | GET  /lingxi/v1/sessions                   | scope `chat`        |
//! | POST /lingxi/v1/sessions                   | scope `chat` (R06-T03 create) |
//! | GET  /lingxi/v1/sessions/{id}              | scope `chat` + owner check in handler |
//! | GET  /lingxi/v1/sessions/{id}/context-observation | scope `chat` + owner check in handler |
//! | POST /lingxi/v1/sessions/{id}/execute      | scope `chat` + owner check in handler |
//! | POST /lingxi/v1/sessions/{id}/fork         | scope `chat` + owner check (R06-T03) |
//! | POST /lingxi/v1/sessions/{id}/turns/retry  | scope `chat` + owner check (R06-T03) |
//! | POST /lingxi/v1/sessions/{id}/rewind[ /preview] | scope `chat` + owner check (R06-T03) |
//! | GET  /lingxi/v1/sessions/{id}/branch       | scope `chat` + owner check (R06-T03) |
//! | GET|POST /lingxi/v1/sessions/{id}/checkpoints[ /delete] | scope `chat` + owner check (R06-T03) |
//! | GET|PATCH /lingxi/v1/sessions/{id}/memory  | scope `chat` + owner check (R06-T03) |
//! | POST /lingxi/v1/sessions/{rename,pin,pin-order,archive,restore,cleanup} | scope `chat` (R06-T03) |
//! | GET  /lingxi/v1/sessions/archived          | scope `chat` (R06-T03) |
//! | POST /lingxi/v1/sessions/archived/delete   | scope `chat` (R06-T03) |
//! | GET  /lingxi/v1/sessions/{search,find}     | scope `chat` (R06-T03) |
//! | POST /lingxi/v1/devices/credentials        | local_only          |
//! | GET  /lingxi/v1/ws                         | scope `chat` (WS upgrade) |
//! | anything else under (or outside) /lingxi/  | local_only (fail-closed default) |
//!
//! The local owner (loopback token over a local connection) bypasses scope
//! checks, mirroring the incumbent `isLocalOwnerPrincipal` short-circuit.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::paths::{atomic_write_private, DataRootLayout};
use crate::transport::ConnectionKind;

/// Schema version of every auth registry file written by this module.
pub const AUTH_SCHEMA_VERSION: u32 = 1;
/// Owner-only loopback-token file inside the runtime dir.
pub const LOCAL_TOKEN_FILE: &str = "local-token.json";
/// Device registry files (same names as the incumbent contracts).
pub const DEVICES_FILE: &str = "devices.json";
pub const DEVICE_CREDENTIALS_FILE: &str = "device-credentials.json";
/// Secret prefixes mirror the incumbent shapes (`hana_dev_…`).
pub const DEVICE_SECRET_PREFIX: &str = "hana_dev_";
/// Fast-path prefix length used for candidate narrowing (incumbent
/// `SECRET_PREFIX_LENGTH`).
pub const SECRET_PREFIX_MATCH_LENGTH: usize = 18;
/// Iterated-SHA-256 rounds for credentials issued by earlier R02 candidates.
pub const SECRET_HASH_ITERATIONS: usize = 4096;

/// Scopes granted to the local owner principal (mirrors the incumbent
/// desktop owner profile in `shared/access-scope-profiles.ts`).
pub const LOCAL_OWNER_SCOPES: &[&str] = &[
    "chat",
    "resources.read",
    "resources.write",
    "files.read",
    "files.write",
    "settings.read",
    "settings.write",
    "providers.manage",
    "secrets.write",
    "bridge.manage",
    "studio.owner",
];

/// The synthetic user id owning the seeded R02 sessions.
pub const LOCAL_OWNER_USER_ID: &str = "user_local";

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── Principal model ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    LocalUser,
    Device,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    LoopbackToken,
    DeviceCredential,
    WebSession,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    Local,
    Lan,
    Tunnel,
    Unknown,
}

/// Normalized authenticated identity. Created exclusively by
/// [`AuthService::authenticate`] at the trust boundary; handlers receive it
/// as request extension state and MUST NOT accept identity fields from
/// request payloads (pinned by tests: forged principal headers/bodies are
/// ignored).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Principal {
    pub schema_version: u32,
    pub principal_id: String,
    pub kind: PrincipalKind,
    pub user_id: Option<String>,
    pub studio_id: Option<String>,
    pub server_node_id: Option<String>,
    pub device_id: Option<String>,
    pub credential_id: Option<String>,
    /// 仅供服务端复核 WebSession 撤销状态；不进入身份响应或持久化主体。
    #[doc(hidden)]
    #[serde(skip)]
    pub web_session_id: Option<String>,
    pub connection_kind: ConnectionKindSerde,
    pub credential_kind: CredentialKind,
    pub trust_state: TrustState,
    pub scopes: Vec<String>,
}

/// Wire shape of the connection kind on principals (camelCase-free enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKindSerde {
    Local,
    Lan,
}

impl From<ConnectionKind> for ConnectionKindSerde {
    fn from(value: ConnectionKind) -> Self {
        match value {
            ConnectionKind::Local => Self::Local,
            ConnectionKind::Lan => Self::Lan,
        }
    }
}

impl Principal {
    /// Mirrors the incumbent `isLocalOwnerPrincipal`: loopback token over a
    /// local connection — the on-machine owner, allowed on every route.
    pub fn is_local_owner(&self) -> bool {
        self.kind == PrincipalKind::LocalUser
            && self.connection_kind == ConnectionKindSerde::Local
            && self.credential_kind == CredentialKind::LoopbackToken
    }

    /// Mirrors `principalHasScope`: exact / namespace / `namespace.*`, with
    /// the local-owner bypass.
    pub fn has_scope(&self, required: &str) -> bool {
        if self.is_local_owner() {
            return true;
        }
        scope_allows(&self.scopes, required)
    }

    fn local_owner(now_ms: u64) -> Self {
        // Identity is fully derived from token verification (no clock input);
        // `now_ms` stays in the signature to mirror the device branch below.
        let _ = now_ms;
        Principal {
            schema_version: AUTH_SCHEMA_VERSION,
            principal_id: format!("principal_local_user_{LOCAL_OWNER_USER_ID}_no_studio_no_node"),
            kind: PrincipalKind::LocalUser,
            user_id: Some(LOCAL_OWNER_USER_ID.to_string()),
            studio_id: None,
            server_node_id: None,
            device_id: None,
            credential_id: None,
            web_session_id: None,
            connection_kind: ConnectionKindSerde::Local,
            credential_kind: CredentialKind::LoopbackToken,
            trust_state: TrustState::Local,
            scopes: LOCAL_OWNER_SCOPES.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Scope-set check mirroring `shared/access-scope-profiles.ts`
/// (`scopeSetAllows`): exact match, bare namespace, or `namespace.*`.
pub fn scope_allows(scopes: &[String], required: &str) -> bool {
    if required.is_empty() {
        return true;
    }
    if scopes.iter().any(|s| s == required) {
        return true;
    }
    let namespace = required.split('.').next().unwrap_or(required);
    scopes
        .iter()
        .any(|s| s == namespace || s == &format!("{namespace}.*"))
}

// ── Constant-time secret comparison ─────────────────────────────────────────

/// Constant-time equality over fixed-length SHA-256 digests of the two
/// values. Hashing first removes length as a side channel; the byte loop
/// with an accumulator has data-independent control flow and runtime for
/// equal-length inputs (the standard hash-then-compare construction; the
/// comparison itself is over public-length digests only).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn legacy_secret_matches(candidate: &str, salt: &str, expected_hash_hex: &str) -> bool {
    let actual = hash_legacy_secret(candidate, salt);
    // Compare hex digests through byte pairs to stay length-uniform.
    let a = actual.as_bytes();
    let b = expected_hash_hex.as_bytes();
    constant_time_eq(a, b)
}

fn hash_legacy_secret(secret: &str, salt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(secret.as_bytes());
    let mut digest = hasher.finalize();
    for _ in 1..SECRET_HASH_ITERATIONS {
        let mut round = Sha256::new();
        round.update(digest);
        digest = round.finalize();
    }
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn hash_device_secret(secret: &str, salt: &str) -> Result<String, AuthSetupError> {
    use base64::Engine as _;
    let params = scrypt::Params::new(14, 8, 1).map_err(|_| AuthSetupError::Crypto {
        detail: "device scrypt parameters invalid".into(),
    })?;
    let mut hash = [0u8; 32];
    scrypt::scrypt(secret.as_bytes(), salt.as_bytes(), &params, &mut hash).map_err(|_| {
        AuthSetupError::Crypto {
            detail: "device scrypt derivation failed".into(),
        }
    })?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash))
}

fn device_secret_matches(candidate: &str, salt: &str, expected_hash: &str) -> bool {
    if expected_hash.len() == 43 {
        return hash_device_secret(candidate, salt)
            .is_ok_and(|actual| constant_time_eq(actual.as_bytes(), expected_hash.as_bytes()));
    }
    if expected_hash.len() == 64 && expected_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return legacy_secret_matches(candidate, salt, expected_hash);
    }
    false
}

// ── Registry file shapes ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    pub schema_version: u32,
    pub device_id: String,
    pub user_id: String,
    pub display_name: String,
    pub device_kind: String,
    pub status: DeviceStatus,
    pub trust_state: TrustState,
    pub created_at_unix_ms: u64,
    pub last_seen_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    Active,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCredentialRecord {
    pub schema_version: u32,
    pub credential_id: String,
    pub device_id: String,
    pub secret_prefix: String,
    pub secret_hash: String,
    pub secret_salt: String,
    pub status: DeviceStatus,
    pub scopes: Vec<String>,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub last_used_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevicesRegistry {
    schema_version: u32,
    devices: Vec<DeviceRecord>,
    created_at_unix_ms: u64,
    updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsRegistry {
    schema_version: u32,
    credentials: Vec<DeviceCredentialRecord>,
    #[serde(default)]
    audit: Vec<DeviceAuditEntry>,
    #[serde(default)]
    pairing_sessions: Vec<PairingRecord>,
    created_at_unix_ms: u64,
    updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceAuditEntry {
    action: String,
    target: String,
    at_unix_ms: u64,
    #[serde(default)]
    metadata: DeviceAuditMetadata,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceAuditMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    credential_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pairing_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairingRecord {
    pairing_session_id: String,
    code_salt: String,
    code_hash: String,
    #[serde(default)]
    code_algorithm: Option<String>,
    requested_device_kind: String,
    requested_display_name: String,
    expires_at_unix_ms: u64,
    status: String,
    device_id: Option<String>,
    credential_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RegistryTransactionPhase {
    Preparing,
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryJournal {
    schema_version: u32,
    phase: RegistryTransactionPhase,
    previous_devices: DevicesRegistry,
    previous_credentials: CredentialsRegistry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalTokenFile {
    schema_version: u32,
    kind: String,
    token: String,
    instance_id: String,
    created_at_unix_ms: u64,
}

// ── Errors / denials ─────────────────────────────────────────────────────────

/// Setup failures (loud, exit 2 class).
#[derive(Debug)]
pub enum AuthSetupError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    RegistryInvalid {
        path: PathBuf,
        detail: String,
    },
    /// The platform's system secure random source failed (R9-F05). No
    /// credential is EVER issued from a weaker source: issuance refuses.
    Entropy {
        detail: String,
    },
    Crypto {
        detail: String,
    },
    InvalidInput {
        detail: String,
    },
}

impl fmt::Display for AuthSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "auth store IO failure at {}: {source}", path.display())
            }
            Self::RegistryInvalid { path, detail } => {
                write!(f, "auth registry {} is invalid: {detail}", path.display())
            }
            Self::Entropy { detail } => {
                write!(
                    f,
                    "cannot issue a security credential: {detail} (refused — no \
                     weaker fallback source is ever used)"
                )
            }
            Self::Crypto { detail } => write!(f, "cannot derive a security credential: {detail}"),
            Self::InvalidInput { detail } => write!(f, "invalid auth input: {detail}"),
        }
    }
}

impl std::error::Error for AuthSetupError {}

/// Authentication denial (wire-stable `reason` codes mirror the incumbent
/// `server-auth.ts` denial reasons).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthDenial {
    pub reason: &'static str,
    pub credential_source: Option<&'static str>,
    pub connection_kind: ConnectionKind,
}

impl AuthDenial {
    fn new(reason: &'static str, connection_kind: ConnectionKind) -> Self {
        Self {
            reason,
            credential_source: None,
            connection_kind,
        }
    }
}

/// Authorization denial produced by [`authorize`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthzDenial {
    pub status: u16,
    pub reason: &'static str,
    pub required_scope: Option<&'static str>,
}

// ── Route policy (endpoint permission table) ────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePolicy {
    Public,
    Authenticated,
    LocalOnly,
    Scope(&'static str),
}

/// Classifies a (method, path) pair against the endpoint permission table.
/// `path` must be the bare path (no query). Unknown routes fail closed to
/// `LocalOnly` — mirroring the incumbent default (`/api/*` unknown →
/// studio owner; everything else → local only; with no studio surface in
/// R02, local_only is the strictest available default).
pub fn classify_route(method: &str, path: &str) -> RoutePolicy {
    let m = method.to_ascii_uppercase();
    // 与 Axum 的实际路由保持精确匹配；不能把未注册的 `/health/` 当作公开路由。
    let p = path;

    if p == "/lingxi/v1/health" && (m == "GET" || m == "HEAD") {
        return RoutePolicy::Public;
    }
    if (m == "GET" || m == "HEAD")
        && (["/mobile", "/mobile/", "/desktop", "/desktop/"].contains(&p)
            || p.starts_with("/mobile/")
            || p.starts_with("/desktop/"))
    {
        return RoutePolicy::Public;
    }
    if (p == "/lingxi/v1/me" || p == "/lingxi/v1/server/identity") && m == "GET" {
        return RoutePolicy::Authenticated;
    }
    if p == "/lingxi/v1/session-thinking-level" && (m == "GET" || m == "POST") {
        return RoutePolicy::Scope("chat");
    }
    if (m == "POST"
        && matches!(
            p,
            "/lingxi/v1/web-auth/login" | "/lingxi/v1/web-auth/logout"
        ))
        || (m == "GET" && p == "/lingxi/v1/web-auth/session")
    {
        return RoutePolicy::Public;
    }
    if p == "/lingxi/v1/ws-ticket" {
        return if m == "POST" {
            RoutePolicy::Scope("chat")
        } else {
            RoutePolicy::LocalOnly
        };
    }
    if p == "/lingxi/v1/sessions" && (m == "GET" || m == "HEAD" || m == "POST") {
        return RoutePolicy::Scope("chat");
    }
    if let Some(rest) = p.strip_prefix("/lingxi/v1/sessions/") {
        // R06-T03 管理面静态名（先静态后参数，与 axum 静态优先同口径；
        // 误把静态名当会话 id 也只会落到同一 scope，区别只在归属校验）。
        if m == "POST"
            && matches!(
                rest,
                "rename" | "pin" | "pin-order" | "archive" | "restore" | "cleanup"
            )
        {
            return RoutePolicy::Scope("chat");
        }
        if rest == "archived/delete" && m == "POST" {
            return RoutePolicy::Scope("chat");
        }
        if !rest.is_empty() && !rest.contains('/') && (m == "GET" || m == "HEAD") {
            return RoutePolicy::Scope("chat");
        }
        if let Some(id) = rest.strip_suffix("/execute") {
            if !id.is_empty() && !id.contains('/') && m == "POST" {
                return RoutePolicy::Scope("chat");
            }
        }
        // R02-T05: event snapshot / cursor continuation page (same scope
        // as the session read it projects; ownership is re-checked per
        // request against the session the stream belongs to).
        if let Some(id) = rest.strip_suffix("/events") {
            if !id.is_empty() && !id.contains('/') && (m == "GET" || m == "HEAD") {
                return RoutePolicy::Scope("chat");
            }
        }
        // R06-T01: the context-observation read face — same scope and the
        // same per-request owner re-check as the session read it observes.
        if let Some(id) = rest.strip_suffix("/context-observation") {
            if !id.is_empty() && !id.contains('/') && (m == "GET" || m == "HEAD") {
                return RoutePolicy::Scope("chat");
            }
        }
        // R06-T03: the session tree faces — fork / retry / rewind (write,
        // sessions.write-shaped; fail-closed scope `chat` + owner re-check)
        // and the branch-history read (read, same as the session read).
        if let Some(id) = rest.strip_suffix("/fork") {
            if !id.is_empty() && !id.contains('/') && m == "POST" {
                return RoutePolicy::Scope("chat");
            }
        }
        if let Some(id) = rest.strip_suffix("/turns/retry") {
            if !id.is_empty() && !id.contains('/') && m == "POST" {
                return RoutePolicy::Scope("chat");
            }
        }
        if let Some(id) = rest.strip_suffix("/rewind/preview") {
            if !id.is_empty() && !id.contains('/') && m == "POST" {
                return RoutePolicy::Scope("chat");
            }
        }
        if let Some(id) = rest.strip_suffix("/rewind") {
            if !id.is_empty() && !id.contains('/') && m == "POST" {
                return RoutePolicy::Scope("chat");
            }
        }
        // R06-T03: 具名检查点（创建/列举/删除）与记忆开关（读/写）。
        if let Some(id) = rest.strip_suffix("/checkpoints/delete") {
            if !id.is_empty() && !id.contains('/') && m == "POST" {
                return RoutePolicy::Scope("chat");
            }
        }
        if let Some(id) = rest.strip_suffix("/checkpoints") {
            if !id.is_empty() && !id.contains('/') && (m == "GET" || m == "HEAD" || m == "POST") {
                return RoutePolicy::Scope("chat");
            }
        }
        if let Some(id) = rest.strip_suffix("/memory") {
            if !id.is_empty() && !id.contains('/') && (m == "GET" || m == "HEAD" || m == "PATCH") {
                return RoutePolicy::Scope("chat");
            }
        }
        if let Some(id) = rest.strip_suffix("/branch") {
            if !id.is_empty() && !id.contains('/') && (m == "GET" || m == "HEAD") {
                return RoutePolicy::Scope("chat");
            }
        }
        // R06-T04: 统一历史读面（分页 /history 与全量 /export）——与会话读
        // 同 scope，归属按请求重检（gate 在 handler 内先于一切实体读取）。
        if let Some(id) = rest.strip_suffix("/history") {
            if !id.is_empty() && !id.contains('/') && (m == "GET" || m == "HEAD") {
                return RoutePolicy::Scope("chat");
            }
        }
        if let Some(id) = rest.strip_suffix("/export") {
            if !id.is_empty() && !id.contains('/') && (m == "GET" || m == "HEAD") {
                return RoutePolicy::Scope("chat");
            }
        }
        // Known session subtree, wrong verb/shape: local only (fail closed).
        return RoutePolicy::LocalOnly;
    }
    if p == "/lingxi/v1/devices/credentials" {
        // Management surface: local owner only, every verb (mirrors the
        // incumbent LOCAL_ONLY /api/devices/ family).
        return RoutePolicy::LocalOnly;
    }
    if p == "/lingxi/v1/ws" && m == "GET" {
        return RoutePolicy::Scope("chat");
    }
    RoutePolicy::LocalOnly
}

/// Route-level authorization. Mirrors the incumbent `authorizeHttpRoute`
/// order: public → local owner → local_only → authenticated → scope.
pub fn authorize(
    method: &str,
    path: &str,
    principal: Option<&Principal>,
) -> Result<(), AuthzDenial> {
    let policy = classify_route(method, path);
    match policy {
        RoutePolicy::Public => Ok(()),
        RoutePolicy::LocalOnly => {
            if principal.is_some_and(Principal::is_local_owner) {
                Ok(())
            } else {
                Err(AuthzDenial {
                    status: 403,
                    reason: "local_owner_required",
                    required_scope: None,
                })
            }
        }
        RoutePolicy::Authenticated => {
            if principal.is_some() {
                Ok(())
            } else {
                Err(AuthzDenial {
                    status: 401,
                    reason: "missing_principal",
                    required_scope: None,
                })
            }
        }
        RoutePolicy::Scope(scope) => {
            let Some(principal) = principal else {
                return Err(AuthzDenial {
                    status: 401,
                    reason: "missing_principal",
                    required_scope: None,
                });
            };
            if principal.is_local_owner() || principal.has_scope(scope) {
                Ok(())
            } else {
                Err(AuthzDenial {
                    status: 403,
                    reason: "insufficient_scope",
                    required_scope: Some(scope),
                })
            }
        }
    }
}

// ── The service ──────────────────────────────────────────────────────────────

/// Shared authentication + authorization service used by BOTH the HTTP
/// middleware and the WS upgrade path (taskbook step 3: one authority, no
/// second implementation to drift).
pub struct AuthService {
    inner: std::sync::Mutex<AuthInner>,
}

#[derive(Clone)]
struct AuthInner {
    local_token: String,
    audit_home: PathBuf,
    token_file: PathBuf,
    devices_path: PathBuf,
    credentials_path: PathBuf,
    journal_path: PathBuf,
    devices: DevicesRegistry,
    credentials: CredentialsRegistry,
    base_devices: DevicesRegistry,
    base_credentials: CredentialsRegistry,
}

impl fmt::Debug for AuthService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never render the token value.
        f.write_str("AuthService(<redacted>)")
    }
}

/// A freshly issued synthetic device credential: the secret is returned
/// exactly once, like the incumbent `createDeviceCredential`.
#[derive(Debug, Clone, PartialEq)]
pub struct IssuedDeviceCredential {
    pub credential_id: String,
    pub device_id: String,
    pub secret: String,
    pub scopes: Vec<String>,
    pub expires_at_unix_ms: Option<u64>,
    pub device: DeviceRecord,
    pub credential: DeviceCredentialView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAccessSnapshot {
    pub devices: Vec<DeviceRecord>,
    pub credentials: Vec<DeviceCredentialView>,
    pub pairing_sessions: Vec<PairingView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingView {
    pub pairing_session_id: String,
    pub requested_device: PairingRequestedDevice,
    pub expires_at_unix_ms: u64,
    pub status: String,
    pub device_id: Option<String>,
    pub credential_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequestedDevice {
    pub device_kind: String,
    pub display_name: String,
}

impl From<&PairingRecord> for PairingView {
    fn from(value: &PairingRecord) -> Self {
        Self {
            pairing_session_id: value.pairing_session_id.clone(),
            requested_device: PairingRequestedDevice {
                device_kind: value.requested_device_kind.clone(),
                display_name: value.requested_display_name.clone(),
            },
            expires_at_unix_ms: value.expires_at_unix_ms,
            status: value.status.clone(),
            device_id: value.device_id.clone(),
            credential_id: value.credential_id.clone(),
        }
    }
}

pub struct CreatedPairing {
    pub pairing: PairingView,
    pub user_code: String,
}

pub struct ApprovedPairing {
    pub pairing: PairingView,
    pub issued: IssuedDeviceCredential,
}

pub enum PairingFailure {
    NotFound,
    Expired,
    AlreadyConsumed,
    InvalidCode,
    Store(AuthSetupError),
}

fn normalize_pairing_code(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|ch| ch.to_ascii_uppercase())
        .collect()
}

fn hash_pairing_code(code: &str, salt: &str) -> Result<String, AuthSetupError> {
    let params = scrypt::Params::new(14, 8, 1).map_err(|_| AuthSetupError::Crypto {
        detail: "scrypt parameters invalid".into(),
    })?;
    let mut hash = [0u8; 32];
    scrypt::scrypt(code.as_bytes(), salt.as_bytes(), &params, &mut hash).map_err(|_| {
        AuthSetupError::Crypto {
            detail: "scrypt derivation failed".into(),
        }
    })?;
    Ok(hash.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn new_pairing_code() -> Result<(String, String), AuthSetupError> {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let random = random_bytes(8)?;
    let compact: String = random
        .iter()
        .map(|byte| ALPHABET[(byte & 31) as usize] as char)
        .collect();
    let display = format!("{}-{}", &compact[..4], &compact[4..]);
    Ok((display, compact))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCredentialView {
    pub schema_version: u32,
    pub credential_id: String,
    pub device_id: String,
    pub status: DeviceStatus,
    pub scopes: Vec<String>,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub last_used_at_unix_ms: Option<u64>,
}

impl From<&DeviceCredentialRecord> for DeviceCredentialView {
    fn from(value: &DeviceCredentialRecord) -> Self {
        Self {
            schema_version: value.schema_version,
            credential_id: value.credential_id.clone(),
            device_id: value.device_id.clone(),
            status: value.status,
            scopes: value.scopes.clone(),
            created_at_unix_ms: value.created_at_unix_ms,
            expires_at_unix_ms: value.expires_at_unix_ms,
            last_used_at_unix_ms: value.last_used_at_unix_ms,
        }
    }
}

pub(crate) fn valid_device_kind(kind: &str) -> bool {
    matches!(kind, "desktop" | "mobile" | "browser" | "cli" | "unknown")
}

fn prepare_device_credential(
    user_id: &str,
    scopes: &[&str],
    expires_at_unix_ms: Option<u64>,
    device_kind: &str,
    display_name: &str,
    now: u64,
) -> Result<(IssuedDeviceCredential, DeviceCredentialRecord), AuthSetupError> {
    if !valid_device_kind(device_kind) {
        return Err(AuthSetupError::InvalidInput {
            detail: "deviceKind outside allowed values".into(),
        });
    }
    let secret = format!("{DEVICE_SECRET_PREFIX}{}", base64url_random(32)?);
    let salt = base64url_random(16)?;
    let device_id = format!("device_{}", uuidish()?);
    let credential_id = format!("cred_{}", uuidish()?);
    let record = DeviceCredentialRecord {
        schema_version: AUTH_SCHEMA_VERSION,
        credential_id: credential_id.clone(),
        device_id: device_id.clone(),
        secret_prefix: secret.chars().take(SECRET_PREFIX_MATCH_LENGTH).collect(),
        secret_hash: hash_device_secret(&secret, &salt)?,
        secret_salt: salt,
        status: DeviceStatus::Active,
        scopes: scopes.iter().map(|s| s.to_string()).collect(),
        created_at_unix_ms: now,
        expires_at_unix_ms,
        last_used_at_unix_ms: None,
    };
    let device = DeviceRecord {
        schema_version: AUTH_SCHEMA_VERSION,
        device_id: device_id.clone(),
        user_id: user_id.to_string(),
        display_name: display_name.to_string(),
        device_kind: device_kind.to_string(),
        status: DeviceStatus::Active,
        trust_state: TrustState::Lan,
        created_at_unix_ms: now,
        last_seen_at_unix_ms: None,
    };
    Ok((
        IssuedDeviceCredential {
            credential_id,
            device_id,
            secret,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            expires_at_unix_ms,
            device,
            credential: DeviceCredentialView::from(&record),
        },
        record,
    ))
}

impl AuthService {
    /// Prepares the auth stores inside the isolated data root: rotates the
    /// per-start loopback token (owner-only file, bound to this instance),
    /// ensures the device registries exist, loads them.
    pub fn bootstrap(layout: &DataRootLayout, instance_id: &str) -> Result<Self, AuthSetupError> {
        let now = now_unix_ms();
        let token_file = layout.runtime_dir.join(LOCAL_TOKEN_FILE);
        let devices_path = layout.runtime_dir.join(DEVICES_FILE);
        let credentials_path = layout.runtime_dir.join(DEVICE_CREDENTIALS_FILE);
        let journal_path = layout.runtime_dir.join("device-registry.journal.json");
        recover_registry_journal(&devices_path, &credentials_path, &journal_path)?;

        // Loopback token: fresh per start (mirrors the incumbent
        // randomBytes(16).toString("hex") SERVER_TOKEN), persisted so other
        // owner-side processes (CLI/desktop harness) can read it — 0600.
        // R9-F05: system CSPRNG only — bootstrap REFUSES to start (and no
        // token is written) when the secure source fails; the old
        // predictable xorshift fallback is gone.
        let token = hex_random(16)?;
        let record = LocalTokenFile {
            schema_version: AUTH_SCHEMA_VERSION,
            kind: "local_token".to_string(),
            token: token.clone(),
            instance_id: instance_id.to_string(),
            created_at_unix_ms: now,
        };
        write_private_json(&token_file, &record)?;

        let devices = match read_json(&devices_path)? {
            Some(value) => serde_json::from_value::<DevicesRegistry>(value).map_err(|err| {
                AuthSetupError::RegistryInvalid {
                    path: devices_path.clone(),
                    detail: err.to_string(),
                }
            })?,
            None => {
                let empty = DevicesRegistry {
                    schema_version: AUTH_SCHEMA_VERSION,
                    devices: Vec::new(),
                    created_at_unix_ms: now,
                    updated_at_unix_ms: now,
                };
                write_private_json(&devices_path, &empty)?;
                empty
            }
        };
        let credentials = match read_json(&credentials_path)? {
            Some(value) => serde_json::from_value::<CredentialsRegistry>(value).map_err(|err| {
                AuthSetupError::RegistryInvalid {
                    path: credentials_path.clone(),
                    detail: err.to_string(),
                }
            })?,
            None => {
                let empty = CredentialsRegistry {
                    schema_version: AUTH_SCHEMA_VERSION,
                    credentials: Vec::new(),
                    audit: Vec::new(),
                    pairing_sessions: Vec::new(),
                    created_at_unix_ms: now,
                    updated_at_unix_ms: now,
                };
                write_private_json(&credentials_path, &empty)?;
                empty
            }
        };
        if devices.schema_version != AUTH_SCHEMA_VERSION
            || credentials.schema_version != AUTH_SCHEMA_VERSION
        {
            return Err(AuthSetupError::RegistryInvalid {
                path: devices_path.clone(),
                detail: format!(
                    "schema version mismatch: devices={}, credentials={} (expected {AUTH_SCHEMA_VERSION})",
                    devices.schema_version, credentials.schema_version
                ),
            });
        }

        crate::security_audit::project(&layout.home, "device-registry", &credentials.audit)
            .map_err(|detail| AuthSetupError::RegistryInvalid {
                path: layout.home.join("logs/security-audit.jsonl"),
                detail,
            })?;

        Ok(Self {
            inner: std::sync::Mutex::new(AuthInner {
                local_token: token,
                audit_home: layout.home.clone(),
                token_file,
                devices_path,
                credentials_path,
                journal_path,
                base_devices: devices.clone(),
                base_credentials: credentials.clone(),
                devices,
                credentials,
            }),
        })
    }

    /// Path of the owner-only loopback-token file (for harnesses/tests).
    pub fn local_token_path(&self) -> PathBuf {
        self.lock().token_file.clone()
    }

    /// Reads the current loopback token. Production code never needs this
    /// (verification is internal); test harnesses use it to authenticate
    /// the owner shape.
    pub fn local_token(&self) -> String {
        self.lock().local_token.clone()
    }

    /// Issues a synthetic device credential (owner/management surface only
    /// — the HTTP route is local_only). Secret is returned once.
    pub fn issue_device_credential(
        &self,
        user_id: &str,
        scopes: &[&str],
        expires_at_unix_ms: Option<u64>,
    ) -> Result<IssuedDeviceCredential, AuthSetupError> {
        self.issue_device_credential_for(user_id, scopes, expires_at_unix_ms, "cli", "CLI")
    }

    pub fn issue_device_credential_for(
        &self,
        user_id: &str,
        scopes: &[&str],
        expires_at_unix_ms: Option<u64>,
        device_kind: &str,
        display_name: &str,
    ) -> Result<IssuedDeviceCredential, AuthSetupError> {
        let now = now_unix_ms();
        // 签发与配对批准共用安全随机材料和脱敏视图，随机源失败时不会落任何记录。
        let (issued, record) = prepare_device_credential(
            user_id,
            scopes,
            expires_at_unix_ms,
            device_kind,
            display_name,
            now,
        )?;

        let mut inner = self.lock();
        refresh_registries(&mut inner)?;
        let mut next = inner.clone();
        next.credentials.credentials.push(record.clone());
        next.devices.devices.push(issued.device.clone());
        next.credentials.audit.push(DeviceAuditEntry {
            action: format!("access.{device_kind}_credential.issue"),
            target: issued.device_id.clone(),
            at_unix_ms: now,
            metadata: DeviceAuditMetadata {
                credential_id: Some(issued.credential_id.clone()),
                scopes: Some(record.scopes.clone()),
                ..Default::default()
            },
        });
        next.credentials.updated_at_unix_ms = now;
        next.devices.updated_at_unix_ms = now;
        persist(&next)?;
        *inner = next;

        Ok(issued)
    }

    pub fn device_access_snapshot(&self) -> Result<DeviceAccessSnapshot, AuthSetupError> {
        let mut inner = self.lock();
        refresh_registries(&mut inner)?;
        Ok(DeviceAccessSnapshot {
            devices: inner.devices.devices.clone(),
            credentials: inner
                .credentials
                .credentials
                .iter()
                .map(DeviceCredentialView::from)
                .collect(),
            pairing_sessions: inner
                .credentials
                .pairing_sessions
                .iter()
                .map(PairingView::from)
                .collect(),
        })
    }

    pub fn create_pairing_session(
        &self,
        device_kind: &str,
        display_name: &str,
        expires_at_unix_ms: u64,
        now: u64,
    ) -> Result<CreatedPairing, AuthSetupError> {
        if !valid_device_kind(device_kind) {
            return Err(AuthSetupError::InvalidInput {
                detail: "requestedDevice.deviceKind outside allowed values".into(),
            });
        }
        let (user_code, normalized_code) = new_pairing_code()?;
        let salt = base64url_random(12)?;
        let record = PairingRecord {
            pairing_session_id: format!("pair_{}", base64url_random(16)?),
            code_hash: hash_pairing_code(&normalized_code, &salt)?,
            code_salt: salt,
            code_algorithm: Some("scrypt-sha256".into()),
            requested_device_kind: device_kind.into(),
            requested_display_name: display_name.into(),
            expires_at_unix_ms,
            status: "pending".into(),
            device_id: None,
            credential_id: None,
        };
        let mut inner = self.lock();
        refresh_registries(&mut inner)?;
        let mut next = inner.clone();
        next.credentials.pairing_sessions.push(record.clone());
        next.credentials.audit.push(DeviceAuditEntry {
            action: "devices.pairing.create".into(),
            target: record.pairing_session_id.clone(),
            at_unix_ms: now,
            metadata: DeviceAuditMetadata {
                device_kind: Some(record.requested_device_kind.clone()),
                ..Default::default()
            },
        });
        next.credentials.updated_at_unix_ms = now;
        persist(&next)?;
        *inner = next;
        Ok(CreatedPairing {
            pairing: PairingView::from(&record),
            user_code,
        })
    }

    pub fn approve_pairing_session(
        &self,
        pairing_id: &str,
        user_code: &str,
        scopes: &[&str],
        expires_at_unix_ms: Option<u64>,
        now: u64,
    ) -> Result<ApprovedPairing, PairingFailure> {
        let mut inner = self.lock();
        refresh_registries(&mut inner).map_err(PairingFailure::Store)?;
        let Some(pairing) = inner
            .credentials
            .pairing_sessions
            .iter()
            .find(|p| p.pairing_session_id == pairing_id)
        else {
            return Err(PairingFailure::NotFound);
        };
        if pairing.status != "pending" {
            return Err(PairingFailure::AlreadyConsumed);
        }
        if pairing.expires_at_unix_ms <= now {
            let mut next = inner.clone();
            let expired = next
                .credentials
                .pairing_sessions
                .iter_mut()
                .find(|p| p.pairing_session_id == pairing_id)
                .expect("配对记录在同一把锁内仍存在");
            expired.status = "expired".into();
            next.credentials.updated_at_unix_ms = now;
            persist(&next).map_err(PairingFailure::Store)?;
            *inner = next;
            return Err(PairingFailure::Expired);
        }
        let normalized_code = normalize_pairing_code(user_code);
        let valid_code = match pairing.code_algorithm.as_deref() {
            Some("scrypt-sha256") => {
                let actual = hash_pairing_code(&normalized_code, &pairing.code_salt)
                    .map_err(PairingFailure::Store)?;
                constant_time_eq(actual.as_bytes(), pairing.code_hash.as_bytes())
            }
            None => legacy_secret_matches(&normalized_code, &pairing.code_salt, &pairing.code_hash),
            _ => {
                return Err(PairingFailure::Store(AuthSetupError::Crypto {
                    detail: "unsupported pairing code algorithm".into(),
                }))
            }
        };
        if !valid_code {
            return Err(PairingFailure::InvalidCode);
        }
        let (issued, credential) = prepare_device_credential(
            LOCAL_OWNER_USER_ID,
            scopes,
            expires_at_unix_ms,
            &pairing.requested_device_kind,
            &pairing.requested_display_name,
            now,
        )
        .map_err(PairingFailure::Store)?;
        let mut next = inner.clone();
        let pairing = next
            .credentials
            .pairing_sessions
            .iter_mut()
            .find(|p| p.pairing_session_id == pairing_id)
            .expect("配对记录在同一把锁内仍存在");
        pairing.status = "approved".into();
        pairing.device_id = Some(issued.device_id.clone());
        pairing.credential_id = Some(issued.credential_id.clone());
        let pairing_view = PairingView::from(&*pairing);
        next.devices.devices.push(issued.device.clone());
        next.credentials.credentials.push(credential);
        next.credentials.audit.push(DeviceAuditEntry {
            action: "devices.pairing.approve".into(),
            target: issued.device_id.clone(),
            at_unix_ms: now,
            metadata: DeviceAuditMetadata {
                credential_id: Some(issued.credential_id.clone()),
                pairing_session_id: Some(pairing_id.into()),
                scopes: Some(issued.scopes.clone()),
                ..Default::default()
            },
        });
        next.devices.updated_at_unix_ms = now;
        next.credentials.updated_at_unix_ms = now;
        persist(&next).map_err(PairingFailure::Store)?;
        *inner = next;
        Ok(ApprovedPairing {
            pairing: pairing_view,
            issued,
        })
    }

    pub fn revoke_device(&self, device_id: &str) -> Result<Option<DeviceRecord>, AuthSetupError> {
        let now = now_unix_ms();
        let mut inner = self.lock();
        refresh_registries(&mut inner)?;
        let mut next = inner.clone();
        let Some(device) = next
            .devices
            .devices
            .iter_mut()
            .find(|d| d.device_id == device_id)
        else {
            return Ok(None);
        };
        if device.status == DeviceStatus::Active {
            device.status = DeviceStatus::Revoked;
            next.devices.updated_at_unix_ms = now;
            for credential in
                next.credentials.credentials.iter_mut().filter(|item| {
                    item.device_id == device_id && item.status == DeviceStatus::Active
                })
            {
                credential.status = DeviceStatus::Revoked;
            }
            next.credentials.audit.push(DeviceAuditEntry {
                action: "devices.revoke".into(),
                target: device_id.into(),
                at_unix_ms: now,
                metadata: DeviceAuditMetadata::default(),
            });
            next.credentials.updated_at_unix_ms = now;
            persist(&next)?;
            *inner = next;
        }
        Ok(inner
            .devices
            .devices
            .iter()
            .find(|d| d.device_id == device_id)
            .cloned())
    }

    pub fn revoke_device_credential_view(
        &self,
        credential_id: &str,
    ) -> Result<Option<DeviceCredentialView>, AuthSetupError> {
        let now = now_unix_ms();
        let mut inner = self.lock();
        refresh_registries(&mut inner)?;
        let mut next = inner.clone();
        let Some(record) = next
            .credentials
            .credentials
            .iter_mut()
            .find(|c| c.credential_id == credential_id)
        else {
            return Ok(None);
        };
        if record.status == DeviceStatus::Active {
            record.status = DeviceStatus::Revoked;
            let device_id = record.device_id.clone();
            next.credentials.updated_at_unix_ms = now;
            next.credentials.audit.push(DeviceAuditEntry {
                action: "devices.credential.revoke".into(),
                target: credential_id.into(),
                at_unix_ms: now,
                metadata: DeviceAuditMetadata {
                    device_id: Some(device_id),
                    ..Default::default()
                },
            });
            persist(&next)?;
            *inner = next;
        }
        Ok(inner
            .credentials
            .credentials
            .iter()
            .find(|c| c.credential_id == credential_id)
            .map(DeviceCredentialView::from))
    }

    /// Revokes a device credential by id (owner/management surface).
    pub fn revoke_device_credential(&self, credential_id: &str) -> Result<bool, AuthSetupError> {
        Ok(self.revoke_device_credential_view(credential_id)?.is_some())
    }

    /// Authenticates a request. Credential precedence mirrors the incumbent
    /// `parseCredential` + `authenticateRequestDetailed`:
    /// `Authorization: Bearer <token>` first; `?token=` only when
    /// `allow_query_token` AND the connection is local; then loopback token
    /// (local connections only) → device credential (expiry + revocation +
    /// connection-kind policy). Failed authentication never mutates the
    /// registries (no `lastUsedAt` writes) — pinned by the no-side-effect
    /// acceptance evidence.
    pub fn authenticate(
        &self,
        authorization: Option<&str>,
        query_token: Option<&str>,
        allow_query_token: bool,
        connection_kind: ConnectionKind,
    ) -> Result<Principal, AuthDenial> {
        let now = now_unix_ms();
        let bearer = parse_bearer(authorization);
        let (token, source) = match bearer {
            Some(token) => (token, "authorization"),
            None => {
                if allow_query_token
                    && connection_kind == ConnectionKind::Local
                    && query_token.is_some_and(|t| !t.trim().is_empty())
                {
                    (query_token.unwrap_or_default().trim().to_string(), "query")
                } else {
                    let mut denial = AuthDenial::new("missing_credential", connection_kind);
                    denial.credential_source = Some("none");
                    return Err(denial);
                }
            }
        };

        // Loopback token: hash-then-constant-time compare against the
        // per-start secret. Only ever valid on a local connection.
        let token_digest = Sha256::digest(token.as_bytes());
        let local_digest = {
            let inner = self.lock();
            Sha256::digest(inner.local_token.as_bytes())
        };
        if constant_time_eq(&token_digest, &local_digest) {
            if connection_kind != ConnectionKind::Local {
                let mut denial =
                    AuthDenial::new("loopback_token_requires_local_transport", connection_kind);
                denial.credential_source = Some(source);
                return Err(denial);
            }
            return Ok(Principal::local_owner(now));
        }

        // Device credential path.
        let principal = {
            let mut inner = self.lock();
            if refresh_registries(&mut inner).is_err() {
                tracing::error!("device registry reload failed; authentication refused");
                let mut denial = AuthDenial::new("auth_registry_unavailable", connection_kind);
                denial.credential_source = Some(source);
                return Err(denial);
            }
            let candidate_prefix_ok = |record: &DeviceCredentialRecord| {
                record.status == DeviceStatus::Active
                    && !record.secret_prefix.is_empty()
                    && token.starts_with(record.secret_prefix.as_str())
            };
            let matched = inner
                .credentials
                .credentials
                .iter()
                .filter(|record| candidate_prefix_ok(record))
                .find(|record| {
                    device_secret_matches(&token, &record.secret_salt, &record.secret_hash)
                })
                .cloned();
            let Some(record) = matched else {
                drop(inner);
                let mut denial = AuthDenial::new("invalid_credential", connection_kind);
                denial.credential_source = Some(source);
                return Err(denial);
            };
            if let Some(expires) = record.expires_at_unix_ms {
                if expires <= now {
                    drop(inner);
                    let mut denial = AuthDenial::new("invalid_credential", connection_kind);
                    denial.credential_source = Some(source);
                    return Err(denial);
                }
            }
            let device = inner
                .devices
                .devices
                .iter()
                .find(|d| d.device_id == record.device_id)
                .cloned();
            let Some(device) = device else {
                drop(inner);
                let mut denial = AuthDenial::new("invalid_credential", connection_kind);
                denial.credential_source = Some(source);
                return Err(denial);
            };
            if device.status != DeviceStatus::Active {
                drop(inner);
                let mut denial = AuthDenial::new("invalid_credential", connection_kind);
                denial.credential_source = Some(source);
                return Err(denial);
            }
            // Connection policy mirrors principalAllowsConnection: device
            // principals may use local or lan connections (the incumbent
            // tunnel→custom_remote axis is out of R02 scope and rejected).
            if device.trust_state == TrustState::Tunnel {
                drop(inner);
                let mut denial = AuthDenial::new("connection_not_allowed", connection_kind);
                denial.credential_source = Some(source);
                return Err(denial);
            }
            // Success: record lastUsedAt (the ONLY registry mutation on the
            // happy path) and persist.
            let mut next = inner.clone();
            if let Some(record_slot) = next
                .credentials
                .credentials
                .iter_mut()
                .find(|c| c.credential_id == record.credential_id)
            {
                record_slot.last_used_at_unix_ms = Some(now);
            }
            if let Some(device_slot) = next
                .devices
                .devices
                .iter_mut()
                .find(|d| d.device_id == device.device_id)
            {
                device_slot.last_seen_at_unix_ms = Some(now);
            }
            next.credentials.updated_at_unix_ms = now;
            next.devices.updated_at_unix_ms = now;
            if let Err(err) = persist(&next) {
                tracing::error!(%err, "cannot persist device credential use; authentication refused");
                let mut denial = AuthDenial::new("auth_registry_unavailable", connection_kind);
                denial.credential_source = Some(source);
                return Err(denial);
            }
            *inner = next;
            Principal {
                schema_version: AUTH_SCHEMA_VERSION,
                principal_id: format!(
                    "principal_device_{}_{}_no_node",
                    device.device_id, device.user_id
                ),
                kind: PrincipalKind::Device,
                user_id: Some(device.user_id.clone()),
                studio_id: None,
                server_node_id: None,
                device_id: Some(device.device_id.clone()),
                credential_id: Some(record.credential_id.clone()),
                web_session_id: None,
                connection_kind: connection_kind.into(),
                credential_kind: CredentialKind::DeviceCredential,
                trust_state: device.trust_state,
                scopes: record.scopes.clone(),
            }
        };
        Ok(principal)
    }

    /// 在使用 WS 票据或已建立连接时重新核对设备身份；撤销、过期及权限变化都让旧身份失效。
    pub fn validate_principal(&self, principal: &Principal) -> Result<(), AuthDenial> {
        if principal.kind != PrincipalKind::Device {
            return Ok(());
        }
        let denial = || AuthDenial::new("invalid_credential", ConnectionKind::Local);
        let mut inner = self.lock();
        if refresh_registries(&mut inner).is_err() {
            tracing::error!("device registry reload failed; WS principal refused");
            return Err(AuthDenial::new(
                "auth_registry_unavailable",
                ConnectionKind::Local,
            ));
        }
        let Some(credential_id) = principal.credential_id.as_deref() else {
            return Err(denial());
        };
        let Some(record) = inner
            .credentials
            .credentials
            .iter()
            .find(|record| record.credential_id == credential_id)
        else {
            return Err(denial());
        };
        if record.status != DeviceStatus::Active
            || record
                .expires_at_unix_ms
                .is_some_and(|expires| expires <= now_unix_ms())
            || principal.device_id.as_deref() != Some(record.device_id.as_str())
            || principal.scopes != record.scopes
        {
            return Err(denial());
        }
        let Some(device) = inner
            .devices
            .devices
            .iter()
            .find(|device| device.device_id == record.device_id)
        else {
            return Err(denial());
        };
        if device.status != DeviceStatus::Active
            || device.trust_state != principal.trust_state
            || principal.user_id.as_deref() != Some(device.user_id.as_str())
        {
            return Err(denial());
        }
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, AuthInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub fn parse_bearer(authorization: Option<&str>) -> Option<String> {
    let value = authorization?.trim();
    let (scheme, rest) = value.split_once(char::is_whitespace)?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = rest.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn persist(inner: &AuthInner) -> Result<(), AuthSetupError> {
    recover_registry_journal(
        &inner.devices_path,
        &inner.credentials_path,
        &inner.journal_path,
    )?;
    let old_devices: DevicesRegistry =
        serde_json::from_value(read_json(&inner.devices_path)?.ok_or_else(|| {
            AuthSetupError::RegistryInvalid {
                path: inner.devices_path.clone(),
                detail: "device registry missing".into(),
            }
        })?)
        .map_err(|err| AuthSetupError::RegistryInvalid {
            path: inner.devices_path.clone(),
            detail: err.to_string(),
        })?;
    let old_credentials: CredentialsRegistry =
        serde_json::from_value(read_json(&inner.credentials_path)?.ok_or_else(|| {
            AuthSetupError::RegistryInvalid {
                path: inner.credentials_path.clone(),
                detail: "credential registry missing".into(),
            }
        })?)
        .map_err(|err| AuthSetupError::RegistryInvalid {
            path: inner.credentials_path.clone(),
            detail: err.to_string(),
        })?;
    if old_devices != inner.base_devices || old_credentials != inner.base_credentials {
        return Err(AuthSetupError::RegistryInvalid {
            path: inner.journal_path.clone(),
            detail: "device registry changed concurrently; refusing stale write".into(),
        });
    }
    // 在改动注册簿前核查并补齐既有审计；日志路径失效时不得先签发或撤销。
    crate::security_audit::project(&inner.audit_home, "device-registry", &old_credentials.audit)
        .map_err(|detail| AuthSetupError::RegistryInvalid {
            path: inner.audit_home.join("logs/security-audit.jsonl"),
            detail,
        })?;
    let mut journal = RegistryJournal {
        schema_version: AUTH_SCHEMA_VERSION,
        phase: RegistryTransactionPhase::Preparing,
        previous_devices: old_devices,
        previous_credentials: old_credentials,
    };
    write_private_json(&inner.journal_path, &journal)?;
    let writes = write_private_json(&inner.devices_path, &inner.devices)
        .and_then(|()| write_private_json(&inner.credentials_path, &inner.credentials));
    if let Err(err) = writes {
        recover_registry_journal(
            &inner.devices_path,
            &inner.credentials_path,
            &inner.journal_path,
        )?;
        return Err(err);
    }
    journal.phase = RegistryTransactionPhase::Committed;
    if let Err(err) = write_private_json(&inner.journal_path, &journal) {
        recover_registry_journal(
            &inner.devices_path,
            &inner.credentials_path,
            &inner.journal_path,
        )?;
        return Err(err);
    }
    if let Err(err) = std::fs::remove_file(&inner.journal_path) {
        tracing::warn!(%err, "committed device registry journal cleanup deferred");
    }
    // 注册簿已提交后，日志投影失败不能假称整笔操作未发生；保留持久意图供重启补写。
    if let Err(err) = crate::security_audit::project(
        &inner.audit_home,
        "device-registry",
        &inner.credentials.audit,
    ) {
        tracing::error!(audit_pending = true, %err, "security audit projection pending");
    }
    Ok(())
}

/// 两份注册表的写入以 journal 为恢复边界；未提交事务启动或复读时一律回滚旧状态。
fn recover_registry_journal(
    devices: &Path,
    credentials: &Path,
    journal_path: &Path,
) -> Result<(), AuthSetupError> {
    let Some(value) = read_json(journal_path)? else {
        return Ok(());
    };
    let journal: RegistryJournal =
        serde_json::from_value(value).map_err(|err| AuthSetupError::RegistryInvalid {
            path: journal_path.to_path_buf(),
            detail: err.to_string(),
        })?;
    if journal.schema_version != AUTH_SCHEMA_VERSION {
        return Err(AuthSetupError::RegistryInvalid {
            path: journal_path.to_path_buf(),
            detail: "journal schema mismatch".into(),
        });
    }
    if matches!(journal.phase, RegistryTransactionPhase::Preparing) {
        write_private_json(devices, &journal.previous_devices)?;
        write_private_json(credentials, &journal.previous_credentials)?;
    }
    std::fs::remove_file(journal_path).map_err(|source| AuthSetupError::Io {
        path: journal_path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// 每次设备身份判断前重新读取并校验两份注册表；文件异常时不能沿用旧内存快照。
fn refresh_registries(inner: &mut AuthInner) -> Result<(), AuthSetupError> {
    recover_registry_journal(
        &inner.devices_path,
        &inner.credentials_path,
        &inner.journal_path,
    )?;
    let load = |path: &Path| -> Result<serde_json::Value, AuthSetupError> {
        read_json(path)?.ok_or_else(|| AuthSetupError::RegistryInvalid {
            path: path.to_path_buf(),
            detail: "registry missing after service startup".to_string(),
        })
    };
    let devices_path = &inner.devices_path;
    let credentials_path = &inner.credentials_path;
    let devices: DevicesRegistry = serde_json::from_value(load(devices_path)?).map_err(|err| {
        AuthSetupError::RegistryInvalid {
            path: devices_path.clone(),
            detail: err.to_string(),
        }
    })?;
    let credentials: CredentialsRegistry = serde_json::from_value(load(credentials_path)?)
        .map_err(|err| AuthSetupError::RegistryInvalid {
            path: credentials_path.clone(),
            detail: err.to_string(),
        })?;
    if devices.schema_version != AUTH_SCHEMA_VERSION
        || credentials.schema_version != AUTH_SCHEMA_VERSION
    {
        return Err(AuthSetupError::RegistryInvalid {
            path: devices_path.clone(),
            detail: format!(
                "schema version mismatch: devices={}, credentials={} (expected {AUTH_SCHEMA_VERSION})",
                devices.schema_version, credentials.schema_version
            ),
        });
    }
    inner.base_devices = devices.clone();
    inner.base_credentials = credentials.clone();
    inner.devices = devices;
    inner.credentials = credentials;
    Ok(())
}

fn read_json(path: &Path) -> Result<Option<serde_json::Value>, AuthSetupError> {
    match std::fs::read_to_string(path) {
        Ok(raw) => {
            serde_json::from_str(&raw)
                .map(Some)
                .map_err(|err| AuthSetupError::RegistryInvalid {
                    path: path.to_path_buf(),
                    detail: format!("not valid JSON: {err}"),
                })
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(AuthSetupError::Io {
            path: path.to_path_buf(),
            source: err,
        }),
    }
}

/// 临时文件创建时即只给主人读写，替换前后都不暴露凭证材料。
fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<(), AuthSetupError> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|err| AuthSetupError::RegistryInvalid {
            path: path.to_path_buf(),
            detail: format!("serialization failed: {err}"),
        })?;
    atomic_write_private(path, &bytes).map_err(|source| AuthSetupError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

// ── System secure randomness (R9-F05: no silent fallback) ───────────────────

/// Fills `n` bytes from the platform's SYSTEM secure random source
/// (`getrandom`: getrandom(2)/getentropy(2) on unix, the OS CSPRNG on
/// Windows). NEVER falls back and never degrades: any failure is
/// propagated, and every SECURITY-CREDENTIAL caller REFUSES to issue on
/// error (R9-F05: the previous implementation silently degraded non-Unix
/// builds — and any `/dev/urandom` open/read failure — to a predictable
/// pid+time xorshift sequence feeding the highest-privilege loopback
/// token, the device secret/salt and the WS tickets; that fallback is
/// gone). Non-secret correlation ids handle failures with an EXPLICIT
/// annotated degradation instead (see `inject.rs::RandomRequestIdGen` —
/// never silently).
fn random_bytes(n: usize) -> Result<Vec<u8>, AuthSetupError> {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).map_err(|source| AuthSetupError::Entropy {
        detail: format!("system secure random source failed: {source}"),
    })?;
    Ok(buf)
}

fn hex_random(n_bytes: usize) -> Result<String, AuthSetupError> {
    let bytes = random_bytes(n_bytes)?;
    let mut out = String::with_capacity(n_bytes * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    Ok(out)
}

/// Fallible system-random hex accessor (the one entropy authority,
/// R02-T07). The request-id generator is the only consumer and handles
/// the error with an EXPLICIT degraded marker — credentials never take
/// that path.
pub fn hex_random_public(n_bytes: usize) -> Result<String, AuthSetupError> {
    hex_random(n_bytes)
}

/// System-secure base64url token generator (same entropy authority);
/// fallible like every credential mint on this surface — the caller
/// refuses to issue when the OS CSPRNG fails.
pub fn random_base64url(n_bytes: usize) -> Result<String, AuthSetupError> {
    base64url_random(n_bytes)
}

fn base64url_random(n_bytes: usize) -> Result<String, AuthSetupError> {
    use base64::Engine as _;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes(n_bytes)?))
}

fn uuidish() -> Result<String, AuthSetupError> {
    hex_random(16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::prepare_layout;

    fn synthetic_home(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("lingxi-r02t03-auth-{}-{tag}", std::process::id()))
    }

    fn setup(tag: &str) -> (std::path::PathBuf, AuthService) {
        let home = synthetic_home(tag);
        let _ = std::fs::remove_dir_all(&home);
        let layout = prepare_layout(&home).unwrap();
        let svc = AuthService::bootstrap(&layout, "inst-test").unwrap();
        (home, svc)
    }

    fn teardown(home: &std::path::Path) {
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn bearer_parsing_mirrors_incumbent() {
        assert_eq!(parse_bearer(Some("Bearer abc")), Some("abc".to_string()));
        assert_eq!(parse_bearer(Some("bearer abc")), Some("abc".to_string()));
        assert_eq!(
            parse_bearer(Some("Bearer   abc  ")),
            Some("abc".to_string())
        );
        assert_eq!(parse_bearer(Some("Bearer")), None);
        assert_eq!(parse_bearer(Some("Basic xyz")), None);
        assert_eq!(parse_bearer(None), None);
    }

    #[cfg(unix)]
    #[test]
    fn failed_device_registry_write_keeps_live_memory_and_disk_unchanged() {
        let (home, svc) = setup("failed-registry-write-rollback");
        let issued = svc
            .issue_device_credential("user_local", &["chat"], None)
            .expect("initial credential");
        let now = now_unix_ms();
        let pending = svc
            .create_pairing_session("mobile", "Phone", now + 60_000, now)
            .unwrap();
        assert_eq!(pending.user_code.len(), 9);
        assert_eq!(pending.user_code.as_bytes()[4], b'-');
        let layout = prepare_layout(&home).unwrap();
        let before_devices = std::fs::read(layout.runtime_dir.join(DEVICES_FILE)).unwrap();
        let before_credentials =
            std::fs::read(layout.runtime_dir.join(DEVICE_CREDENTIALS_FILE)).unwrap();
        let before_audit = std::fs::read(home.join("logs/security-audit.jsonl")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let original_permissions = std::fs::metadata(&layout.runtime_dir)
            .unwrap()
            .permissions();
        std::fs::set_permissions(&layout.runtime_dir, std::fs::Permissions::from_mode(0o500))
            .unwrap();
        let issue_failed = svc
            .issue_device_credential("user_local", &["chat"], None)
            .is_err();
        let (issue_devices, issue_credentials) = {
            let live = svc.lock();
            (
                live.devices.devices.len(),
                live.credentials.credentials.len(),
            )
        };
        let device_revoke_failed = svc.revoke_device(&issued.device_id).is_err();
        let credential_revoke_failed = svc
            .revoke_device_credential_view(&issued.credential_id)
            .is_err();
        let pairing_approve_failed = matches!(
            svc.approve_pairing_session(
                &pending.pairing.pairing_session_id,
                &pending.user_code,
                &["chat"],
                None,
                now_unix_ms()
            ),
            Err(PairingFailure::Store(_)),
        );
        let (device_status, credential_status) = {
            let live = svc.lock();
            (
                live.devices.devices[0].status,
                live.credentials.credentials[0].status,
            )
        };
        std::fs::set_permissions(&layout.runtime_dir, original_permissions).unwrap();
        assert!(
            issue_failed
                && device_revoke_failed
                && credential_revoke_failed
                && pairing_approve_failed
        );
        assert_eq!(
            (issue_devices, issue_credentials),
            (1, 1),
            "failed issue must not remain in memory"
        );
        assert_eq!(device_status, DeviceStatus::Active);
        assert_eq!(credential_status, DeviceStatus::Active);
        assert_eq!(
            svc.device_access_snapshot().unwrap().pairing_sessions[0].status,
            "pending"
        );
        assert_eq!(
            std::fs::read(layout.runtime_dir.join(DEVICES_FILE)).unwrap(),
            before_devices
        );
        assert_eq!(
            std::fs::read(layout.runtime_dir.join(DEVICE_CREDENTIALS_FILE)).unwrap(),
            before_credentials
        );
        assert_eq!(
            std::fs::read(home.join("logs/security-audit.jsonl")).unwrap(),
            before_audit,
            "注册簿提交失败不得写成功审计事件"
        );
        let principal = svc
            .authenticate(
                Some(&format!("Bearer {}", issued.secret)),
                None,
                false,
                ConnectionKind::Local,
            )
            .expect("original credential remains valid");
        assert_eq!(
            principal.credential_id.as_deref(),
            Some(issued.credential_id.as_str())
        );
        teardown(&home);
    }

    #[test]
    fn concurrent_pairing_approval_issues_once() {
        // Node crypto.scryptSync("ABCDEFGH", "unit-test-salt", 32, {N:16384,r:8,p:1}) 的独立向量。
        assert_eq!(
            hash_pairing_code("ABCDEFGH", "unit-test-salt").unwrap(),
            "9a25122cf984d90fa97dac2666546b06451d677d6c78853065ee0e908330e9de"
        );
        use std::sync::{Arc, Barrier};
        let (home, svc) = setup("pairing-concurrent");
        let svc = Arc::new(svc);
        let now = now_unix_ms();
        let pending = svc
            .create_pairing_session("mobile", "Phone", now + 60_000, now)
            .unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let svc = svc.clone();
            let barrier = barrier.clone();
            let id = pending.pairing.pairing_session_id.clone();
            let code = normalize_pairing_code(&pending.user_code).to_ascii_lowercase();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                svc.approve_pairing_session(&id, &code, &["chat"], None, now_unix_ms())
            }));
        }
        barrier.wait();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(PairingFailure::AlreadyConsumed)))
                .count(),
            1
        );
        let snapshot = svc.device_access_snapshot().unwrap();
        assert_eq!(snapshot.devices.len(), 1);
        assert_eq!(snapshot.credentials.len(), 1);
        assert_eq!(snapshot.pairing_sessions[0].status, "approved");
        let guard = svc.lock();
        let audit = &guard.credentials.audit;
        assert_eq!(
            audit
                .iter()
                .filter(|entry| entry.action == "devices.pairing.create")
                .count(),
            1
        );
        assert_eq!(
            audit
                .iter()
                .filter(|entry| entry.action == "devices.pairing.approve")
                .count(),
            1
        );
        let created = audit
            .iter()
            .find(|entry| entry.action == "devices.pairing.create")
            .unwrap();
        assert_eq!(created.target, pending.pairing.pairing_session_id);
        assert_eq!(created.metadata.device_kind.as_deref(), Some("mobile"));
        let approved = audit
            .iter()
            .find(|entry| entry.action == "devices.pairing.approve")
            .unwrap();
        assert_eq!(approved.target, snapshot.devices[0].device_id);
        assert_eq!(
            approved.metadata.pairing_session_id.as_deref(),
            Some(pending.pairing.pairing_session_id.as_str())
        );
        assert_eq!(
            approved.metadata.credential_id.as_deref(),
            Some(snapshot.credentials[0].credential_id.as_str())
        );
        assert_eq!(
            approved.metadata.scopes.as_ref(),
            Some(&vec!["chat".to_string()])
        );
        drop(guard);
        teardown(&home);
    }

    #[test]
    fn device_secret_hash_matches_node_and_accepts_earlier_rust_hashes() {
        // Node crypto.scryptSync(secret, salt, 32).toString("base64url") 的独立向量。
        let secret = "hana_dev_unit_test_secret";
        let salt = "unit-test-salt";
        let node_hash = "RdzXdDv9cb7oKrKZWqWe4RjByXDdrhbl5Y3dYWekN7A";
        assert_eq!(hash_device_secret(secret, salt).unwrap(), node_hash);
        assert!(device_secret_matches(secret, salt, node_hash));
        assert!(!device_secret_matches("wrong", salt, node_hash));
        let earlier_hash = hash_legacy_secret(secret, salt);
        assert!(device_secret_matches(secret, salt, &earlier_hash));
        assert!(!device_secret_matches("wrong", salt, &earlier_hash));
        assert!(!device_secret_matches(secret, salt, "invalid"));
    }

    #[test]
    fn invalid_device_kind_never_issues_or_creates_pairing() {
        let (home, svc) = setup("device-kind-boundary");
        let layout = prepare_layout(&home).unwrap();
        let before_devices = std::fs::read(layout.runtime_dir.join(DEVICES_FILE)).unwrap();
        let before_credentials =
            std::fs::read(layout.runtime_dir.join(DEVICE_CREDENTIALS_FILE)).unwrap();
        let before_audit = std::fs::read(home.join("logs/security-audit.jsonl")).unwrap();
        assert!(matches!(
            svc.issue_device_credential_for(
                LOCAL_OWNER_USER_ID,
                &["chat"],
                None,
                "invalid-kind",
                "Phone",
            ),
            Err(AuthSetupError::InvalidInput { .. })
        ));
        let now = now_unix_ms();
        assert!(matches!(
            svc.create_pairing_session("invalid-kind", "Phone", now + 60_000, now),
            Err(AuthSetupError::InvalidInput { .. })
        ));
        assert_eq!(
            std::fs::read(layout.runtime_dir.join(DEVICES_FILE)).unwrap(),
            before_devices
        );
        assert_eq!(
            std::fs::read(layout.runtime_dir.join(DEVICE_CREDENTIALS_FILE)).unwrap(),
            before_credentials
        );
        assert_eq!(
            std::fs::read(home.join("logs/security-audit.jsonl")).unwrap(),
            before_audit
        );
        teardown(&home);
    }

    #[test]
    fn security_audit_log_failure_rejects_before_issuing_and_replay_is_idempotent() {
        let (home, svc) = setup("security-audit-outbox");
        let log = home.join("logs/security-audit.jsonl");
        std::fs::remove_file(&log).unwrap();
        std::fs::create_dir(&log).unwrap();
        let layout = prepare_layout(&home).unwrap();
        let before_devices = std::fs::read(layout.runtime_dir.join(DEVICES_FILE)).unwrap();
        let before_credentials =
            std::fs::read(layout.runtime_dir.join(DEVICE_CREDENTIALS_FILE)).unwrap();
        assert!(svc
            .issue_device_credential_for(LOCAL_OWNER_USER_ID, &["chat"], None, "mobile", "Phone")
            .is_err());
        assert_eq!(
            std::fs::read(layout.runtime_dir.join(DEVICES_FILE)).unwrap(),
            before_devices
        );
        assert_eq!(
            std::fs::read(layout.runtime_dir.join(DEVICE_CREDENTIALS_FILE)).unwrap(),
            before_credentials
        );
        assert!(log.is_dir(), "日志故障不能被伪装成已写入标准审计");
        std::fs::remove_dir(&log).unwrap();
        let issued = svc
            .issue_device_credential_for(LOCAL_OWNER_USER_ID, &["chat"], None, "mobile", "Phone")
            .unwrap();
        let restarted = AuthService::bootstrap(&layout, "audit-restart").unwrap();
        assert_eq!(
            restarted
                .device_access_snapshot()
                .unwrap()
                .credentials
                .len(),
            1
        );
        let first = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<_> = first.lines().collect();
        assert_eq!(lines.len(), 1);
        let event: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event["schemaVersion"], 1);
        assert!(event["eventId"]
            .as_str()
            .is_some_and(|v| v.starts_with("sec_")));
        assert!(event["timestamp"]
            .as_str()
            .is_some_and(|v| chrono::DateTime::parse_from_rfc3339(v).is_ok()));
        assert_eq!(event["action"], "access.mobile_credential.issue");
        assert_eq!(event["actor"]["credentialKind"], "loopback_token");
        assert_eq!(event["target"], issued.device_id);
        assert_eq!(event["metadata"]["credentialId"], issued.credential_id);
        assert_eq!(event["metadata"]["scopes"], serde_json::json!(["chat"]));
        assert_eq!(event["secretFields"], serde_json::json!(["secret"]));
        assert!(!first.contains(&issued.secret));
        AuthService::bootstrap(&layout, "audit-restart-again").unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            first,
            "重启不得重复投影审计事件"
        );
        teardown(&home);
    }

    #[test]
    fn expired_pairing_records_expiry_without_issuing_credential() {
        let (home, svc) = setup("pairing-expired");
        let now = now_unix_ms();
        let pending = svc
            .create_pairing_session("mobile", "Phone", now + 1, now)
            .unwrap();
        assert!(matches!(
            svc.approve_pairing_session(
                &pending.pairing.pairing_session_id,
                &pending.user_code,
                &["chat"],
                None,
                now + 1
            ),
            Err(PairingFailure::Expired),
        ));
        let snapshot = svc.device_access_snapshot().unwrap();
        assert_eq!(snapshot.pairing_sessions[0].status, "expired");
        assert!(snapshot.devices.is_empty() && snapshot.credentials.is_empty());
        teardown(&home);
    }

    #[test]
    fn revoking_device_cascades_to_its_credentials_only() {
        let (home, svc) = setup("device-cascade");
        let first = svc
            .issue_device_credential("user_local", &["chat"], None)
            .unwrap();
        let other = svc
            .issue_device_credential("user_local", &["chat"], None)
            .unwrap();
        svc.revoke_device(&first.device_id).unwrap();
        let snapshot = svc.device_access_snapshot().unwrap();
        assert_eq!(
            snapshot
                .credentials
                .iter()
                .find(|item| item.credential_id == first.credential_id)
                .unwrap()
                .status,
            DeviceStatus::Revoked
        );
        assert_eq!(
            snapshot
                .credentials
                .iter()
                .find(|item| item.credential_id == other.credential_id)
                .unwrap()
                .status,
            DeviceStatus::Active
        );
        let guard = svc.lock();
        let issued = guard
            .credentials
            .audit
            .iter()
            .find(|entry| {
                entry.target == first.device_id && entry.action.ends_with("credential.issue")
            })
            .unwrap();
        assert_eq!(
            issued.metadata.credential_id.as_deref(),
            Some(first.credential_id.as_str())
        );
        assert_eq!(
            issued.metadata.scopes.as_ref(),
            Some(&vec!["chat".to_string()])
        );
        drop(guard);
        assert!(svc
            .authenticate(
                Some(&format!("Bearer {}", other.secret)),
                None,
                false,
                ConnectionKind::Lan
            )
            .is_ok());
        svc.revoke_device_credential_view(&other.credential_id)
            .unwrap();
        let guard = svc.lock();
        let revoked = guard
            .credentials
            .audit
            .iter()
            .find(|entry| {
                entry.target == other.credential_id && entry.action == "devices.credential.revoke"
            })
            .unwrap();
        assert_eq!(
            revoked.metadata.device_id.as_deref(),
            Some(other.device_id.as_str())
        );
        drop(guard);
        assert!(svc
            .authenticate(
                Some(&format!("Bearer {}", first.secret)),
                None,
                false,
                ConnectionKind::Lan
            )
            .is_err());
        assert!(svc
            .authenticate(
                Some(&format!("Bearer {}", other.secret)),
                None,
                false,
                ConnectionKind::Lan
            )
            .is_err());
        teardown(&home);
    }

    #[test]
    fn pairing_and_issued_device_survive_restart() {
        let (home, svc) = setup("pairing-restart");
        let now = now_unix_ms();
        let pending = svc
            .create_pairing_session("mobile", "Phone", now + 60_000, now)
            .unwrap();
        let layout = prepare_layout(&home).unwrap();
        let restarted = AuthService::bootstrap(&layout, "inst-pairing-2").unwrap();
        assert_eq!(
            restarted.device_access_snapshot().unwrap().pairing_sessions[0].status,
            "pending"
        );
        let approved = restarted
            .approve_pairing_session(
                &pending.pairing.pairing_session_id,
                &pending.user_code,
                &["chat"],
                None,
                now + 1,
            )
            .ok()
            .expect("pending code remains usable after restart");
        let restarted_again = AuthService::bootstrap(&layout, "inst-pairing-3").unwrap();
        let snapshot = restarted_again.device_access_snapshot().unwrap();
        assert_eq!(snapshot.pairing_sessions[0].status, "approved");
        assert_eq!(snapshot.credentials.len(), 1);
        assert!(restarted_again
            .authenticate(
                Some(&format!("Bearer {}", approved.issued.secret)),
                None,
                false,
                ConnectionKind::Lan,
            )
            .is_ok());
        teardown(&home);
    }

    #[test]
    fn constant_time_eq_basics() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn local_token_roundtrip_and_owner_principal() {
        let (home, svc) = setup("local-token");
        let token = svc.local_token();
        assert_eq!(
            token.len(),
            32,
            "128-bit hex like the incumbent SERVER_TOKEN"
        );

        // Owner over local connection authenticates and is the local owner.
        let principal = svc
            .authenticate(
                Some(&format!("Bearer {token}")),
                None,
                false,
                ConnectionKind::Local,
            )
            .unwrap();
        assert!(principal.is_local_owner());
        assert!(principal.has_scope("chat"));

        // Wrong token is invalid_credential.
        let denial = svc
            .authenticate(Some("Bearer deadbeef"), None, false, ConnectionKind::Local)
            .unwrap_err();
        assert_eq!(denial.reason, "invalid_credential");

        // Missing credential.
        let denial = svc
            .authenticate(None, None, false, ConnectionKind::Local)
            .unwrap_err();
        assert_eq!(denial.reason, "missing_credential");

        // Loopback token over a LAN connection is refused even though the
        // token itself is valid.
        let denial = svc
            .authenticate(
                Some(&format!("Bearer {token}")),
                None,
                false,
                ConnectionKind::Lan,
            )
            .unwrap_err();
        assert_eq!(denial.reason, "loopback_token_requires_local_transport");

        // Query token: local + allowed → works; lan + allowed → missing.
        svc.authenticate(None, Some(&token), true, ConnectionKind::Local)
            .unwrap();
        let denial = svc
            .authenticate(None, Some(&token), true, ConnectionKind::Lan)
            .unwrap_err();
        assert_eq!(denial.reason, "missing_credential");
        // Query token with allow_query_token=false → missing.
        let denial = svc
            .authenticate(None, Some(&token), false, ConnectionKind::Local)
            .unwrap_err();
        assert_eq!(denial.reason, "missing_credential");

        teardown(&home);
    }

    #[test]
    fn device_credential_lifecycle_create_auth_expire_revoke() {
        let (home, svc) = setup("device-lifecycle");
        let issued = svc
            .issue_device_credential("user_remote", &["chat"], None)
            .unwrap();
        assert!(issued.secret.starts_with("hana_dev_"));

        // Authenticates as a device principal with the granted scopes.
        let principal = svc
            .authenticate(
                Some(&format!("Bearer {}", issued.secret)),
                None,
                false,
                ConnectionKind::Lan,
            )
            .unwrap();
        assert_eq!(principal.kind, PrincipalKind::Device);
        assert_eq!(principal.user_id.as_deref(), Some("user_remote"));
        assert!(principal.has_scope("chat"));
        assert!(!principal.has_scope("settings.write"));
        assert!(!principal.is_local_owner());

        // Wrong secret (right prefix family, wrong tail) is denied.
        let forged = format!(
            "{}{}",
            &issued.secret[..18],
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        );
        let denial = svc
            .authenticate(
                Some(&format!("Bearer {forged}")),
                None,
                false,
                ConnectionKind::Lan,
            )
            .unwrap_err();
        assert_eq!(denial.reason, "invalid_credential");

        // Expired credential is denied.
        let expired = svc
            .issue_device_credential("user_expired", &["chat"], Some(1))
            .unwrap();
        let denial = svc
            .authenticate(
                Some(&format!("Bearer {}", expired.secret)),
                None,
                false,
                ConnectionKind::Lan,
            )
            .unwrap_err();
        assert_eq!(denial.reason, "invalid_credential");

        // Revoked credential is denied.
        svc.revoke_device_credential(&issued.credential_id).unwrap();
        let denial = svc
            .authenticate(
                Some(&format!("Bearer {}", issued.secret)),
                None,
                false,
                ConnectionKind::Lan,
            )
            .unwrap_err();
        assert_eq!(denial.reason, "invalid_credential");

        teardown(&home);
    }

    #[test]
    fn failed_authentication_never_touches_registry_files() {
        let (home, svc) = setup("no-mutation");
        let devices =
            std::fs::read(svc.local_token_path().parent().unwrap().join(DEVICES_FILE)).unwrap();
        let creds = std::fs::read(
            svc.local_token_path()
                .parent()
                .unwrap()
                .join(DEVICE_CREDENTIALS_FILE),
        )
        .unwrap();

        // A burst of failing authentications.
        for i in 0..8 {
            let _ = svc.authenticate(
                Some(&format!("Bearer forged-{i}")),
                None,
                false,
                ConnectionKind::Local,
            );
        }
        let _ = svc.authenticate(None, None, false, ConnectionKind::Local);

        assert_eq!(
            devices,
            std::fs::read(svc.local_token_path().parent().unwrap().join(DEVICES_FILE)).unwrap(),
            "devices.json must be byte-identical after failed auths"
        );
        assert_eq!(
            creds,
            std::fs::read(
                svc.local_token_path()
                    .parent()
                    .unwrap()
                    .join(DEVICE_CREDENTIALS_FILE)
            )
            .unwrap(),
            "device-credentials.json must be byte-identical after failed auths"
        );
        teardown(&home);
    }

    #[test]
    fn token_file_is_owner_only_and_instance_bound() {
        let (home, svc) = setup("token-file");
        let path = svc.local_token_path();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "loopback token file must be 0600");
        }
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["instanceId"], "inst-test");
        assert_eq!(raw["kind"], "local_token");
        teardown(&home);
    }

    #[test]
    fn route_policy_table() {
        use RoutePolicy::*;
        assert_eq!(classify_route("GET", "/lingxi/v1/health"), Public);
        assert_eq!(classify_route("HEAD", "/lingxi/v1/health"), Public);
        assert_eq!(classify_route("GET", "/lingxi/v1/me"), Authenticated);
        assert_eq!(classify_route("POST", "/lingxi/v1/web-auth/login"), Public);
        assert_eq!(classify_route("GET", "/lingxi/v1/web-auth/session"), Public);
        assert_eq!(classify_route("POST", "/lingxi/v1/web-auth/logout"), Public);
        assert_eq!(
            classify_route("GET", "/lingxi/v1/web-auth/login"),
            LocalOnly
        );
        assert_eq!(
            classify_route("POST", "/lingxi/v1/web-auth/session"),
            LocalOnly
        );
        assert_eq!(
            classify_route("GET", "/lingxi/v1/web-auth/logout"),
            LocalOnly
        );
        assert_eq!(
            classify_route("POST", "/lingxi/v1/ws-ticket"),
            Scope("chat")
        );
        assert_eq!(classify_route("GET", "/lingxi/v1/ws-ticket"), LocalOnly);
        assert_eq!(classify_route("GET", "/lingxi/v1/sessions"), Scope("chat"));
        assert_eq!(
            classify_route("GET", "/lingxi/v1/sessions/s1"),
            Scope("chat")
        );
        assert_eq!(
            classify_route("POST", "/lingxi/v1/sessions/s1/execute"),
            Scope("chat")
        );
        // R06-T04 统一历史读面：GET/HEAD 走 chat scope，其余动词 fail-closed。
        assert_eq!(
            classify_route("GET", "/lingxi/v1/sessions/s1/history"),
            Scope("chat")
        );
        assert_eq!(
            classify_route("GET", "/lingxi/v1/sessions/s1/export"),
            Scope("chat")
        );
        assert_eq!(
            classify_route("POST", "/lingxi/v1/sessions/s1/history"),
            LocalOnly
        );
        assert_eq!(
            classify_route("GET", "/lingxi/v1/sessions/s1/history/extra"),
            LocalOnly
        );
        assert_eq!(
            classify_route("DELETE", "/lingxi/v1/sessions/s1"),
            LocalOnly
        );
        assert_eq!(
            classify_route("POST", "/lingxi/v1/sessions/s1/execute/extra"),
            LocalOnly
        );
        assert_eq!(
            classify_route("POST", "/lingxi/v1/devices/credentials"),
            LocalOnly
        );
        assert_eq!(classify_route("GET", "/lingxi/v1/ws"), Scope("chat"));
        for path in [
            "/lingxi/v1/health/",
            "/lingxi/v1/me/",
            "/lingxi/v1/sessions/",
            "/lingxi/v1/ws/",
        ] {
            assert_eq!(
                classify_route("GET", path),
                LocalOnly,
                "unregistered trailing slash must not inherit an endpoint policy"
            );
        }
        // Fail-closed defaults.
        assert_eq!(classify_route("GET", "/lingxi/v1/nope"), LocalOnly);
        assert_eq!(classify_route("GET", "/api/sessions"), LocalOnly);
        assert_eq!(classify_route("GET", "/"), LocalOnly);
    }

    #[test]
    fn authorize_order_and_denials() {
        let (home, svc) = setup("authorize");
        let owner = svc
            .authenticate(
                Some(&format!("Bearer {}", svc.local_token())),
                None,
                false,
                ConnectionKind::Local,
            )
            .unwrap();
        let device = svc
            .issue_device_credential("user_b", &["chat"], None)
            .unwrap();
        let device_p = svc
            .authenticate(
                Some(&format!("Bearer {}", device.secret)),
                None,
                false,
                ConnectionKind::Lan,
            )
            .unwrap();

        // public: no principal needed.
        authorize("GET", "/lingxi/v1/health", None).unwrap();
        // local_only: owner yes, device no, anonymous no.
        authorize("POST", "/lingxi/v1/devices/credentials", Some(&owner)).unwrap();
        assert_eq!(
            authorize("POST", "/lingxi/v1/devices/credentials", Some(&device_p))
                .unwrap_err()
                .status,
            403
        );
        assert_eq!(
            authorize("POST", "/lingxi/v1/devices/credentials", None)
                .unwrap_err()
                .status,
            403
        );
        // scope: anonymous 401, scoped device ok, unscoped device 403.
        assert_eq!(
            authorize("GET", "/lingxi/v1/sessions/s1", None)
                .unwrap_err()
                .status,
            401
        );
        authorize("GET", "/lingxi/v1/sessions/s1", Some(&device_p)).unwrap();
        let unscoped = svc
            .issue_device_credential("user_c", &["files.read"], None)
            .unwrap();
        let unscoped_p = svc
            .authenticate(
                Some(&format!("Bearer {}", unscoped.secret)),
                None,
                false,
                ConnectionKind::Lan,
            )
            .unwrap();
        let denial = authorize("GET", "/lingxi/v1/sessions/s1", Some(&unscoped_p)).unwrap_err();
        assert_eq!(denial.status, 403);
        assert_eq!(denial.reason, "insufficient_scope");
        assert_eq!(denial.required_scope, Some("chat"));
        teardown(&home);
    }

    #[test]
    fn scope_namespace_expansion() {
        assert!(scope_allows(&["chat".to_string()], "chat"));
        assert!(scope_allows(&["chat".to_string()], "chat.read"));
        assert!(scope_allows(&["chat.*".to_string()], "chat.read"));
        assert!(!scope_allows(&["chatx".to_string()], "chat"));
        assert!(scope_allows(&[], ""));
    }

    #[test]
    fn registries_reload_across_bootstrap() {
        let (home, svc) = setup("reload");
        let issued = svc
            .issue_device_credential("user_r", &["chat"], None)
            .unwrap();
        // Re-bootstrap on the same layout (registries survive; token rotates).
        let layout = prepare_layout(&home).unwrap();
        let svc2 = AuthService::bootstrap(&layout, "inst-2").unwrap();
        assert_ne!(
            svc2.local_token(),
            svc.local_token(),
            "per-start token rotation"
        );
        let p = svc2
            .authenticate(
                Some(&format!("Bearer {}", issued.secret)),
                None,
                false,
                ConnectionKind::Lan,
            )
            .unwrap();
        assert_eq!(p.user_id.as_deref(), Some("user_r"));
        teardown(&home);
    }

    #[test]
    fn live_device_revocation_is_visible_to_another_auth_instance() {
        let (home, issuer) = setup("live-cross-instance-revoke");
        let issued = issuer
            .issue_device_credential("user_r", &["chat"], None)
            .unwrap();
        let layout = prepare_layout(&home).unwrap();
        let serving = AuthService::bootstrap(&layout, "inst-serving").unwrap();
        let authorization = format!("Bearer {}", issued.secret);
        let principal = serving
            .authenticate(Some(&authorization), None, false, ConnectionKind::Lan)
            .unwrap();
        assert!(serving.validate_principal(&principal).is_ok());

        issuer
            .revoke_device_credential(&issued.credential_id)
            .unwrap();
        assert_eq!(
            serving
                .authenticate(Some(&authorization), None, false, ConnectionKind::Lan)
                .unwrap_err()
                .reason,
            "invalid_credential"
        );
        assert_eq!(
            serving.validate_principal(&principal).unwrap_err().reason,
            "invalid_credential"
        );
        teardown(&home);
    }
}
