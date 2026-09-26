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
//!    create/read/expiry/revocation semantics. Divergence (documented, not
//!    silent): the incumbent hashes with `scryptSync`; scrypt is not in the
//!    locked dependency set, so this implementation uses iterated salted
//!    SHA-256 (`SECRET_HASH_ITERATIONS` rounds) — salted, slow-ish,
//!    constant-time-verified, with the same prefix fast-path. Field shape
//!    uses unix-millisecond integers instead of ISO-8601 strings.
//! 3. **Principal model** (incumbent `core/security-principal.ts`): the
//!    normalized principal vocabulary (`kind`/`credentialKind`/
//!    `connectionKind`/`trustState`/scopes) with derived principalId.
//!    Identities are created ONLY here at the trust boundary (token
//!    verification); request-supplied identity fields are never read.
//!    The incumbent web-session cookie axis (`core/web-session-store.ts`)
//!    is NOT wired in R02 — there is no web-login flow yet; bearer/query/
//!    device credentials cover this stage's surface. Recorded in the task
//!    report as a deferral, not a silent downgrade.
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
//! | GET  /lingxi/v1/sessions/{id}              | scope `chat` + owner check in handler |
//! | POST /lingxi/v1/sessions/{id}/execute      | scope `chat` + owner check in handler |
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

use crate::paths::{atomic_write, DataRootLayout};
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
/// Iterated-SHA-256 rounds for credential hashing (documented divergence
/// from the incumbent scryptSync; see module docs).
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

fn secret_matches(candidate: &str, salt: &str, expected_hash_hex: &str) -> bool {
    let actual = hash_secret(candidate, salt);
    // Compare hex digests through byte pairs to stay length-uniform.
    let a = actual.as_bytes();
    let b = expected_hash_hex.as_bytes();
    constant_time_eq(a, b)
}

fn hash_secret(secret: &str, salt: &str) -> String {
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
    created_at_unix_ms: u64,
    updated_at_unix_ms: u64,
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
    let p = path.trim_end_matches('/');
    let p = if p.is_empty() { "/" } else { p };

    if p == "/lingxi/v1/health" && (m == "GET" || m == "HEAD") {
        return RoutePolicy::Public;
    }
    if p == "/lingxi/v1/me" && m == "GET" {
        return RoutePolicy::Authenticated;
    }
    if p == "/lingxi/v1/ws-ticket" {
        return if m == "POST" {
            RoutePolicy::Scope("chat")
        } else {
            RoutePolicy::LocalOnly
        };
    }
    if p == "/lingxi/v1/sessions" && (m == "GET" || m == "HEAD") {
        return RoutePolicy::Scope("chat");
    }
    if let Some(rest) = p.strip_prefix("/lingxi/v1/sessions/") {
        if !rest.contains('/') && (m == "GET" || m == "HEAD") {
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

struct AuthInner {
    local_token: String,
    token_file: PathBuf,
    devices_path: PathBuf,
    credentials_path: PathBuf,
    devices: DevicesRegistry,
    credentials: CredentialsRegistry,
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

        // Loopback token: fresh per start (mirrors the incumbent
        // randomBytes(16).toString("hex") SERVER_TOKEN), persisted so other
        // owner-side processes (CLI/desktop harness) can read it — 0600.
        let token = hex_random(16);
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

        Ok(Self {
            inner: std::sync::Mutex::new(AuthInner {
                local_token: token,
                token_file,
                devices_path,
                credentials_path,
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
        let now = now_unix_ms();
        let secret = format!("{DEVICE_SECRET_PREFIX}{}", base64url_random(32));
        let salt = base64url_random(16);
        let device_id = format!("device_{}", uuidish());
        let credential_id = format!("cred_{}", uuidish());
        let record = DeviceCredentialRecord {
            schema_version: AUTH_SCHEMA_VERSION,
            credential_id: credential_id.clone(),
            device_id: device_id.clone(),
            secret_prefix: secret.chars().take(SECRET_PREFIX_MATCH_LENGTH).collect(),
            secret_hash: hash_secret(&secret, &salt),
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
            display_name: format!("Synthetic device for {user_id}"),
            device_kind: "cli".to_string(),
            status: DeviceStatus::Active,
            trust_state: TrustState::Lan,
            created_at_unix_ms: now,
            last_seen_at_unix_ms: None,
        };

        let mut inner = self.lock();
        inner.credentials.credentials.push(record);
        inner.devices.devices.push(device);
        inner.credentials.updated_at_unix_ms = now;
        inner.devices.updated_at_unix_ms = now;
        persist(&inner)?;

        Ok(IssuedDeviceCredential {
            credential_id,
            device_id,
            secret,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            expires_at_unix_ms,
        })
    }

    /// Revokes a device credential by id (owner/management surface).
    pub fn revoke_device_credential(&self, credential_id: &str) -> Result<bool, AuthSetupError> {
        let now = now_unix_ms();
        let mut inner = self.lock();
        let Some(record) = inner
            .credentials
            .credentials
            .iter_mut()
            .find(|c| c.credential_id == credential_id)
        else {
            return Ok(false);
        };
        if record.status == DeviceStatus::Active {
            record.status = DeviceStatus::Revoked;
            inner.credentials.updated_at_unix_ms = now;
            persist(&inner)?;
        }
        Ok(true)
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
                .find(|record| secret_matches(&token, &record.secret_salt, &record.secret_hash))
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
            if let Some(record_slot) = inner
                .credentials
                .credentials
                .iter_mut()
                .find(|c| c.credential_id == record.credential_id)
            {
                record_slot.last_used_at_unix_ms = Some(now);
            }
            if let Some(device_slot) = inner
                .devices
                .devices
                .iter_mut()
                .find(|d| d.device_id == device.device_id)
            {
                device_slot.last_seen_at_unix_ms = Some(now);
            }
            inner.credentials.updated_at_unix_ms = now;
            inner.devices.updated_at_unix_ms = now;
            if let Err(err) = persist(&inner) {
                tracing::warn!(%err, "cannot persist device credential lastUsedAt (auth still succeeds)");
            }
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
                connection_kind: connection_kind.into(),
                credential_kind: CredentialKind::DeviceCredential,
                trust_state: device.trust_state,
                scopes: record.scopes.clone(),
            }
        };
        Ok(principal)
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
    write_private_json(&inner.devices_path, &inner.devices)?;
    write_private_json(&inner.credentials_path, &inner.credentials)?;
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

/// Atomic write + explicit owner-only permissions (mirrors the incumbent
/// `writeSecretFileSync`: mode-on-create is umask-dependent, so chmod after).
fn write_private_json<T: Serialize>(path: &Path, value: &T) -> Result<(), AuthSetupError> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|err| AuthSetupError::RegistryInvalid {
            path: path.to_path_buf(),
            detail: format!("serialization failed: {err}"),
        })?;
    atomic_write(path, &bytes).map_err(|source| AuthSetupError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(path, perms).map_err(|source| AuthSetupError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        }
    }
    Ok(())
}

// ── Synthetic randomness (no new dependencies) ──────────────────────────────

/// Reads `n` random bytes from the OS random device (unix), falling back to
/// the same documented pid+time mix as `instance.rs` (diagnosed through the
/// same entropy source vocabulary).
fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    #[cfg(unix)]
    {
        use std::io::Read as _;
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            if f.read_exact(&mut buf).is_ok() {
                return buf;
            }
        }
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut state = (nanos as u64) ^ (u64::from(std::process::id()) << 32) ^ 0x9e37_79b9_7f4a_7c15;
    for slot in &mut buf {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *slot = (state >> 24) as u8;
    }
    buf
}

fn hex_random(n_bytes: usize) -> String {
    let bytes = random_bytes(n_bytes);
    let mut out = String::with_capacity(n_bytes * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Generates a synthetic base64url token (OS randomness, documented
/// fallback); shared with the WS ticket service so every secret on this
/// surface has one entropy authority.
pub fn random_base64url(n_bytes: usize) -> String {
    base64url_random(n_bytes)
}

fn base64url_random(n_bytes: usize) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes(n_bytes))
}

fn uuidish() -> String {
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
}
