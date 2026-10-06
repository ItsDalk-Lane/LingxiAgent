//! R00 补充场景中的本机管理入口。所有管理写入先完整构造下一版本，再原子替换单个状态文件。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::extract::{Path as RoutePath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Extension, Json, Router};
use qrcodegen::{QrCode, QrCodeEcc};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::auth::{
    constant_time_eq, random_base64url, CredentialKind, PairingFailure, Principal, PrincipalKind,
    TrustState, LOCAL_OWNER_SCOPES, LOCAL_OWNER_USER_ID,
};
use crate::paths::{atomic_write_private, DataRootLayout};
use crate::{EndpointError, ServiceConfig, ServiceState};
use lingxi_protocol::ErrorCode;

const MANAGER_FILE: &str = "management.json";
const SCHEMA: u32 = 1;
const WEB_TTL_MS: u64 = 14 * 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    user_id: String,
    username: String,
    display_name: String,
    password_salt: Option<String>,
    password_hash: Option<String>,
    #[serde(default)]
    password_algorithm: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountView<'a> {
    user_id: &'a str,
    username: &'a str,
    display_name: &'a str,
    password_set: bool,
}

impl Account {
    fn view(&self) -> AccountView<'_> {
        AccountView {
            user_id: &self.user_id,
            username: &self.username,
            display_name: &self.display_name,
            password_set: self.password_hash.is_some(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Network {
    mode: String,
    listen_host: String,
    listen_port: u16,
    public_base_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PersistedNetwork {
    pub mode: String,
    pub listen_host: String,
    pub listen_port: u16,
}

fn validated_network(record: &ManagementRecord) -> Result<PersistedNetwork, String> {
    if record.schema_version != SCHEMA || record.account.user_id != LOCAL_OWNER_USER_ID {
        return Err("management state schema or owner mismatch".into());
    }
    match (
        &record.account.password_salt,
        &record.account.password_hash,
        &record.account.password_algorithm,
    ) {
        (None, None, None) => {}
        (Some(salt), Some(hash), None)
            if !salt.is_empty()
                && hash.len() == 64
                && hash.bytes().all(|b| b.is_ascii_hexdigit()) => {}
        (Some(salt), Some(hash), Some(algo))
            if algo == "scrypt-sha256"
                && salt.len() >= 16
                && hash.len() == 128
                && hash.bytes().all(|b| b.is_ascii_hexdigit()) => {}
        _ => return Err("management password record invalid".into()),
    }
    let host: std::net::IpAddr = record
        .network
        .listen_host
        .parse()
        .map_err(|_| "management network host invalid".to_string())?;
    match record.network.mode.as_str() {
        "loopback" if !host.is_loopback() => {
            return Err("management loopback host is not local".into())
        }
        "loopback" | "lan" => {}
        _ => return Err("management network mode invalid".into()),
    }
    if record.network.listen_port != 0 && record.network.listen_port < 1024 {
        return Err("management network port out of range".into());
    }
    if record
        .network
        .public_base_url
        .as_ref()
        .is_some_and(|url| !valid_public_base_url(url))
    {
        return Err("management public URL invalid".into());
    }
    Ok(PersistedNetwork {
        mode: record.network.mode.clone(),
        listen_host: record.network.listen_host.clone(),
        listen_port: record.network.listen_port,
    })
}

/// 启动前只读已保存设置；损坏的配置必须显错，不能悄悄退回默认监听地址。
pub(crate) fn read_persisted_network(home: &Path) -> Result<Option<PersistedNetwork>, String> {
    let path = home.join(crate::paths::RUNTIME_DIR_NAME).join(MANAGER_FILE);
    let bytes = match read_management_bytes(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("management network read failed: {err}")),
    };
    let record: ManagementRecord =
        serde_json::from_slice(&bytes).map_err(|err| format!("management state invalid: {err}"))?;
    validated_network(&record).map(Some)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebSession {
    session_id: String,
    secret_salt: String,
    secret_hash: String,
    principal: Principal,
    #[serde(default)]
    secure_required: bool,
    expires_at_unix_ms: u64,
    status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditEntry {
    action: String,
    target: String,
    at_unix_ms: u64,
    #[serde(default)]
    metadata: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    actor: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagementRecord {
    schema_version: u32,
    #[serde(default)]
    server_id: String,
    #[serde(default)]
    studio_id: String,
    account: Account,
    network: Network,
    web_sessions: Vec<WebSession>,
    default_thinking_level: String,
    session_thinking_levels: BTreeMap<String, String>,
    audit: Vec<AuditEntry>,
}

pub(crate) struct ManagementState {
    path: PathBuf,
    audit_home: PathBuf,
    inner: Mutex<ManagementRecord>,
}

impl ManagementState {
    pub(crate) fn open(layout: &DataRootLayout, config: &ServiceConfig) -> Result<Self, String> {
        let path = layout.runtime_dir.join(MANAGER_FILE);
        let mut record = match read_management_bytes(&path) {
            Ok(bytes) => serde_json::from_slice::<ManagementRecord>(&bytes)
                .map_err(|err| format!("management state invalid: {err}"))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let record = ManagementRecord {
                    schema_version: SCHEMA,
                    server_id: format!(
                        "server_{}",
                        random_base64url(16).map_err(|e| e.to_string())?
                    ),
                    studio_id: format!(
                        "studio_{}",
                        random_base64url(16).map_err(|e| e.to_string())?
                    ),
                    account: Account {
                        user_id: LOCAL_OWNER_USER_ID.into(),
                        username: "local".into(),
                        display_name: "Local User".into(),
                        password_salt: None,
                        password_hash: None,
                        password_algorithm: None,
                    },
                    network: Network {
                        mode: format!("{}", config.network_mode),
                        listen_host: config.bind_addr.ip().to_string(),
                        listen_port: config.bind_addr.port(),
                        public_base_url: None,
                    },
                    web_sessions: Vec::new(),
                    default_thinking_level: "medium".into(),
                    session_thinking_levels: BTreeMap::new(),
                    audit: Vec::new(),
                };
                validated_network(&record)?;
                write_record(&path, &record)?;
                record
            }
            Err(err) => return Err(format!("management state read failed: {err}")),
        };
        validated_network(&record)?;
        if record.server_id.is_empty() || record.studio_id.is_empty() {
            record.server_id = format!(
                "server_{}",
                random_base64url(16).map_err(|e| e.to_string())?
            );
            record.studio_id = format!(
                "studio_{}",
                random_base64url(16).map_err(|e| e.to_string())?
            );
            write_record(&path, &record)?;
        }
        crate::security_audit::project(&layout.home, "management", &record.audit)?;
        Ok(Self {
            path,
            audit_home: layout.home.clone(),
            inner: Mutex::new(record),
        })
    }

    fn read(&self) -> ManagementRecord {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn read_fresh(&self) -> Result<ManagementRecord, EndpointError> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let record = load_fresh_record(&self.path)?;
        crate::security_audit::project(&self.audit_home, "management", &record.audit)
            .map_err(manager_error)?;
        *guard = record.clone();
        Ok(record)
    }

    pub(crate) fn identity_ids(&self) -> (String, String) {
        let record = self.read();
        (record.server_id, record.studio_id)
    }

    pub(crate) fn public_base_url(&self) -> Option<String> {
        self.read().network.public_base_url
    }

    fn change<T>(
        &self,
        f: impl FnOnce(&mut ManagementRecord) -> Result<T, EndpointError>,
    ) -> Result<T, EndpointError> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *guard = load_fresh_record(&self.path)?;
        crate::security_audit::project(&self.audit_home, "management", &guard.audit)
            .map_err(manager_error)?;
        let mut next = guard.clone();
        let out = f(&mut next)?;
        write_record(&self.path, &next).map_err(manager_error)?;
        *guard = next;
        if let Err(err) =
            crate::security_audit::project(&self.audit_home, "management", &guard.audit)
        {
            tracing::error!(audit_pending = true, %err, "security audit projection pending");
        }
        Ok(out)
    }

    fn verify_password_and_upgrade(
        &self,
        username: &str,
        password: &str,
        now: u64,
        connection: crate::ConnectionKind,
    ) -> Result<Option<Account>, EndpointError> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *guard = load_fresh_record(&self.path)?;
        crate::security_audit::project(&self.audit_home, "management", &guard.audit)
            .map_err(manager_error)?;
        if ![
            guard.account.username.as_str(),
            guard.account.display_name.as_str(),
            guard.account.user_id.as_str(),
        ]
        .iter()
        .any(|value| value.eq_ignore_ascii_case(username))
        {
            return Ok(None);
        }
        if !password_matches(password, &guard.account).map_err(manager_error)? {
            return Ok(None);
        }
        if guard.account.password_algorithm.is_none() {
            let salt = random_base64url(16).map_err(auth_store_error)?;
            let hash = hash_password(password, &salt).map_err(manager_error)?;
            let mut next = guard.clone();
            next.account.password_salt = Some(salt);
            next.account.password_hash = Some(hash);
            next.account.password_algorithm = Some("scrypt-sha256".into());
            audit(
                &mut next,
                "access.account.password.migrate",
                LOCAL_OWNER_USER_ID,
                now,
            );
            if let Some(entry) = next.audit.last_mut() {
                entry.actor = Some(serde_json::json!({
                    "principalId": "principal_local_user_user_local_no_studio_no_node",
                    "kind": "local_user", "userId": LOCAL_OWNER_USER_ID,
                    "studioId": null, "serverId": null, "serverNodeId": null,
                    "deviceId": null, "credentialId": null, "agentId": null,
                    "pluginId": null, "bridgeAccountId": null,
                    "platformAccountId": null, "officialServiceKind": null,
                    "connectionKind": connection.as_str(), "credentialKind": "user_session",
                    "trustState": connection.as_str()
                }));
            }
            write_record(&self.path, &next).map_err(manager_error)?;
            *guard = next;
            if let Err(err) =
                crate::security_audit::project(&self.audit_home, "management", &guard.audit)
            {
                tracing::error!(audit_pending = true, %err, "security audit projection pending");
            }
        }
        Ok(Some(guard.account.clone()))
    }

    pub(crate) fn authenticate_cookie(
        &self,
        cookie: Option<&str>,
        now: u64,
        secure: bool,
    ) -> Result<Option<Principal>, EndpointError> {
        let Some(secret) = parse_cookie(cookie) else {
            return Ok(None);
        };
        let record = self.read_fresh()?;
        let Some(session) = record.web_sessions.iter().find(|session| {
            session.status == "active"
                && secret_matches(&secret, &session.secret_salt, &session.secret_hash)
        }) else {
            return Ok(None);
        };
        if session.secure_required && !secure {
            return Ok(None);
        }
        if session.expires_at_unix_ms <= now {
            return Ok(None);
        }
        let mut principal = session.principal.clone();
        principal.credential_kind = CredentialKind::WebSession;
        principal.web_session_id = Some(session.session_id.clone());
        Ok(Some(principal))
    }

    /// WS 票据与已建立连接必须继续受原浏览器会话约束；退出或过期后，
    /// 即使同一账号还有另一会话，也不能沿用这个连接的旧身份。
    pub(crate) fn web_session_current(
        &self,
        principal: &Principal,
        now: u64,
        secure_transport: bool,
    ) -> Result<bool, EndpointError> {
        let Some(session_id) = principal.web_session_id.as_deref() else {
            return Ok(false);
        };
        let record = load_fresh_record(&self.path)?;
        let Some(session) = record
            .web_sessions
            .iter()
            .find(|session| session.session_id == session_id)
        else {
            return Ok(false);
        };
        let mut expected = session.principal.clone();
        expected.credential_kind = CredentialKind::WebSession;
        expected.web_session_id = Some(session.session_id.clone());
        Ok(session.status == "active"
            && session.expires_at_unix_ms > now
            && (!session.secure_required || secure_transport)
            && expected == *principal)
    }

    fn revoke_cookie(
        &self,
        secret: &str,
        connection: crate::ConnectionKind,
        secure: bool,
    ) -> Result<(), EndpointError> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *guard = load_fresh_record(&self.path)?;
        crate::security_audit::project(&self.audit_home, "management", &guard.audit)
            .map_err(manager_error)?;
        let Some(index) = guard.web_sessions.iter().position(|item| {
            item.status == "active" && secret_matches(secret, &item.secret_salt, &item.secret_hash)
        }) else {
            return Ok(());
        };
        if guard.web_sessions[index].principal.connection_kind != connection.into()
            || (guard.web_sessions[index].secure_required && !secure)
        {
            return Err(EndpointError::forbidden("web_session_transport_mismatch"));
        }
        let mut next = guard.clone();
        next.web_sessions[index].status = "revoked".into();
        write_record(&self.path, &next).map_err(manager_error)?;
        *guard = next;
        Ok(())
    }
}

fn parse_cookie(header: Option<&str>) -> Option<String> {
    let mut found = None;
    for part in header?.split(';') {
        let (name, value) = part.trim().split_once('=')?;
        if name == "hana_session" {
            if found.is_some() || value.len() > 256 || !value.starts_with("hana_web_") {
                return None;
            }
            found = Some(value.to_string());
        }
    }
    found
}

fn write_record(path: &Path, record: &ManagementRecord) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(record).map_err(|e| e.to_string())?;
    atomic_write_private(path, &bytes).map_err(|e| e.to_string())?;
    Ok(())
}

fn read_management_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 16 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "management state is not a bounded regular file",
        ));
    }
    std::fs::read(path)
}

fn load_fresh_record(path: &Path) -> Result<ManagementRecord, EndpointError> {
    let bytes = read_management_bytes(path)
        .map_err(|e| manager_error(format!("management state read failed: {e}")))?;
    let record: ManagementRecord = serde_json::from_slice(&bytes)
        .map_err(|e| manager_error(format!("management state invalid: {e}")))?;
    validated_network(&record).map_err(manager_error)?;
    Ok(record)
}

fn manager_error(detail: String) -> EndpointError {
    tracing::error!(%detail, "management state operation failed");
    EndpointError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        ErrorCode::Internal,
        "management state unavailable",
    )
    .with_reason("management_store_failure")
    .with_cause("management.store_failure")
}

fn auth_store_error(err: crate::auth::AuthSetupError) -> EndpointError {
    tracing::error!(error = %err, "device registry operation failed");
    EndpointError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        ErrorCode::Internal,
        "device registry unavailable",
    )
    .with_reason("device_registry_failure")
    .with_cause("auth.device_registry_failure")
}

fn hash_secret(secret: &str, salt: &str) -> String {
    let mut hash = Sha256::digest([salt.as_bytes(), secret.as_bytes()].concat());
    for _ in 1..4096 {
        hash = Sha256::digest(hash);
    }
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn secret_matches(secret: &str, salt: &str, expected: &str) -> bool {
    constant_time_eq(hash_secret(secret, salt).as_bytes(), expected.as_bytes())
}

fn hash_password(password: &str, salt: &str) -> Result<String, String> {
    let params =
        scrypt::Params::new(14, 8, 1).map_err(|_| "scrypt parameters invalid".to_string())?;
    let mut key = [0u8; 64];
    scrypt::scrypt(password.as_bytes(), salt.as_bytes(), &params, &mut key)
        .map_err(|_| "scrypt derivation failed".to_string())?;
    Ok(key.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn password_matches(password: &str, account: &Account) -> Result<bool, String> {
    let (Some(salt), Some(expected)) = (&account.password_salt, &account.password_hash) else {
        return Ok(false);
    };
    let actual = match account.password_algorithm.as_deref() {
        Some("scrypt-sha256") => hash_password(password, salt)?,
        None => hash_secret(password, salt),
        _ => return Err("unsupported management password algorithm".into()),
    };
    Ok(constant_time_eq(actual.as_bytes(), expected.as_bytes()))
}

fn audit(next: &mut ManagementRecord, action: &str, target: &str, now: u64) {
    audit_with_metadata(next, action, target, now, serde_json::json!({}));
}

fn audit_with_metadata(
    next: &mut ManagementRecord,
    action: &str,
    target: &str,
    now: u64,
    metadata: serde_json::Value,
) {
    next.audit.push(AuditEntry {
        action: action.into(),
        target: target.into(),
        at_unix_ms: now,
        metadata,
        actor: None,
    });
}

pub(crate) fn routes() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/access/summary", get(access_summary))
        .route("/lingxi/v1/access/mobile-qr.svg", get(mobile_qr))
        .route("/lingxi/v1/access/network", put(update_network))
        .route("/lingxi/v1/access/account/profile", put(update_profile))
        .route(
            "/lingxi/v1/access/account/password",
            put(set_password).delete(clear_password),
        )
        .route(
            "/lingxi/v1/access/mobile-credentials",
            post(issue_mobile_credential),
        )
        .route(
            "/lingxi/v1/access/desktop-credentials",
            post(issue_desktop_credential),
        )
        .route("/lingxi/v1/devices", get(list_devices))
        .route("/lingxi/v1/devices/{device_id}/revoke", post(revoke_device))
        .route(
            "/lingxi/v1/devices/credentials/{credential_id}/revoke",
            post(revoke_credential),
        )
        .route("/lingxi/v1/devices/pairing-sessions", post(create_pairing))
        .route(
            "/lingxi/v1/devices/pairing-sessions/{pairing_id}/approve",
            post(approve_pairing),
        )
        .route("/lingxi/v1/web-auth/login", post(web_login))
        .route("/lingxi/v1/web-auth/session", get(web_session_status))
        .route("/lingxi/v1/web-auth/logout", post(web_logout))
        .route(
            "/lingxi/v1/session-thinking-level",
            get(read_thinking_level).post(write_thinking_level),
        )
        .route("/lingxi/v1/models/reload", post(reload_models))
        .route("/lingxi/v1/models/credentials", get(list_model_credentials))
        .route(
            "/lingxi/v1/models/credentials/{provider}/revoke",
            post(revoke_model_credential),
        )
        // R05 RR1 F04: the OAuth login surface — start/callback/poll to a
        // credential installed through the SAME authority (the six
        // R00-T02 exclusive leaves), plus logout and the OAuth model
        // registry.
        .route(
            "/lingxi/v1/models/credentials/{provider}/login",
            post(start_model_login),
        )
        .route(
            "/lingxi/v1/models/credentials/{provider}/login/callback",
            post(complete_model_login),
        )
        .route(
            "/lingxi/v1/models/credentials/{provider}/login/poll",
            post(poll_model_login),
        )
        .route(
            "/lingxi/v1/models/credentials/{provider}/logout",
            post(logout_model_credential),
        )
        .route(
            "/lingxi/v1/models/oauth/{provider}/models",
            get(list_oauth_models).post(add_oauth_model),
        )
        .route(
            "/lingxi/v1/models/oauth/{provider}/models/{model_id}",
            delete(remove_oauth_model),
        )
        .route("/lingxi/v1/server/identity", get(server_identity))
}

/// Reloads the model plane from the SAME source it was loaded from at
/// startup (R05-T01 C05): re-read + full validation + one atomic snapshot
/// swap with a bumped generation. A source that no longer carries a valid
/// plane is a loud 409 — the running snapshot is never silently cleared or
/// half-swapped, and removing the plane requires a restart (there is no
/// "unconfigure at runtime" path).
async fn reload_models(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    if principal.kind != PrincipalKind::LocalUser {
        return EndpointError::local_only().into_response();
    }
    let (source, gateway) = match (state.model_plane_source(), state.model_gateway()) {
        (Some(source), Some(gateway)) => (source.clone(), gateway.clone()),
        _ => {
            return EndpointError::not_found()
                .with_reason("model_plane_unconfigured")
                .with_cause("models.unconfigured")
                .into_response()
        }
    };
    let plane = match &source {
        crate::config::ModelPlaneSource::EmbeddedInConfigFile { path } => {
            match crate::config::read_service_config(path) {
                Ok(file) => match file.model_plane {
                    Some(plane) => plane,
                    None => {
                        return reload_invalid(
                            &source,
                            "the config file no longer carries providers/models sections \
                             (removing the model plane requires a restart)",
                        );
                    }
                },
                Err(err) => return reload_invalid(&source, &err.to_string()),
            }
        }
        crate::config::ModelPlaneSource::ModelsFile { path } => {
            let raw = match std::fs::read_to_string(path) {
                Ok(raw) => raw,
                Err(err) => {
                    return reload_invalid(
                        &source,
                        &format!("cannot read {}: {err}", path.display()),
                    );
                }
            };
            match lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&raw) {
                Ok(plane) => plane,
                Err(err) => return reload_invalid(&source, &err.to_string()),
            }
        }
    };
    // R05 RR1 F02: the credential service re-seeds from the SAME freshly
    // validated plane FIRST, stamped with the configuration generation
    // this reload is ABOUT to publish (the number `gateway.reload` below
    // installs — `upcoming_generation` is that single source). With the
    // credential re-seed leading the gateway swap there is no window in
    // which a NEW-generation route can meet OLD-generation material (the
    // gateway has not published the new routes yet), and the re-seeded
    // cells' epoch refuses to hand NEW material to OLD-generation routes
    // — the reload can never mix credential material across generations.
    // Swapping the two reloads alone would NOT close either window; the
    // generation binding is the fix.
    let next_generation = gateway.upcoming_generation();
    if let Some(credentials) = state.credential_service() {
        credentials.reload(&plane, next_generation).await;
    }
    let generation = gateway.reload(plane.clone());
    // R05 RR1 F14: the freshly validated plane's network policy publishes
    // on the SHARED policy plane atomically with the route swap — every
    // model-plane consumer's NEXT request observes the new proxy / NO_PROXY
    // / explicit-CA generation (in-flight requests keep the client they
    // started with; system mode re-snapshots the exported proxy
    // environment here, once per reload).
    if let Some(network_plane) = state.network_plane() {
        network_plane
            .apply(lingxi_adapters::models::network::NetworkPolicy::from_config(&plane.network));
    }
    // R05-T06: the operation plane's egress allowlist follows the CURRENT
    // provider endpoints (a same-origin media-product destination that a
    // reload moved is no longer same-origin — the swap is atomic with the
    // gateway's).
    if let Some(operations) = state.operations() {
        let endpoints: Vec<String> = gateway
            .provider_endpoints()
            .into_iter()
            .map(|(_, endpoint)| endpoint)
            .collect();
        if let Err(err) = operations.refresh_egress_origins(&endpoints) {
            return reload_invalid(&source, &err.to_string());
        }
    }
    let now = state.clock.now_unix_ms();
    if let Err(err) = state.management.change(|next| {
        audit_with_metadata(
            next,
            "models.reload",
            "model-plane",
            now,
            serde_json::json!({"generation": generation}),
        );
        Ok(())
    }) {
        return err.into_response();
    }
    Json(serde_json::json!({"ok": true, "generation": generation})).into_response()
}

/// The material-free credential status of every configured provider
/// (R05-T02 C06/C09): kind, state, expiry ledger, refresh-in-flight, and
/// the honest persistence flags — never any material.
async fn list_model_credentials(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    if principal.kind != PrincipalKind::LocalUser {
        return EndpointError::local_only().into_response();
    }
    let Some(credentials) = state.credential_service() else {
        return EndpointError::not_found()
            .with_reason("model_plane_unconfigured")
            .with_cause("models.unconfigured")
            .into_response();
    };
    Json(serde_json::json!({"providers": credentials.status().await})).into_response()
}

/// Revokes one provider's credential (R05-T02 C05): the stored OAuth token
/// set is deleted and the in-memory material flips to revoked with a
/// generation bump that fences any in-flight refresh's write-back. A
/// persistence failure is a loud 409 (the credential stays active — never
/// a pretend-revocation).
async fn revoke_model_credential(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    RoutePath(provider): RoutePath<String>,
) -> Response {
    if principal.kind != PrincipalKind::LocalUser {
        return EndpointError::local_only().into_response();
    }
    let Some(credentials) = state.credential_service() else {
        return EndpointError::not_found()
            .with_reason("model_plane_unconfigured")
            .with_cause("models.unconfigured")
            .into_response();
    };
    match credentials.revoke(&provider).await {
        Ok(()) => {
            let now = state.clock.now_unix_ms();
            if let Err(err) = state.management.change(|next| {
                audit_with_metadata(
                    next,
                    "models.credentials.revoke",
                    "model-plane",
                    now,
                    serde_json::json!({"provider": provider}),
                );
                Ok(())
            }) {
                return err.into_response();
            }
            Json(serde_json::json!({"ok": true, "provider": provider})).into_response()
        }
        Err(lingxi_adapters::models::credentials::CredentialError::NotConfigured { .. }) => {
            EndpointError::not_found()
                .with_reason("model_provider_unknown")
                .with_cause("models.provider_unknown")
                .into_response()
        }
        Err(err) => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Internal,
            format!("credential revocation refused: {err}"),
        )
        .with_reason("credential_revoke_failed")
        .with_cause("models.credentials_revoke_failed")
        .into_response(),
    }
}

/// R05 RR1 F04: maps a credential-surface failure onto the management
/// error vocabulary. `NotConfigured` doubles as the explicit non-OAuth /
/// invalid-input rejection (a 409 naming the reason); a persistence
/// failure is a 409 (the state changed nothing); everything else keeps
/// the classification of the login surface.
fn credential_surface_error(
    err: lingxi_adapters::models::credentials::CredentialError,
) -> Response {
    use lingxi_adapters::models::credentials::CredentialError;
    match err {
        CredentialError::NotConfigured { .. } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::InvalidMessage,
            format!("credential surface refused the request: {err}"),
        )
        .with_reason("credential_surface_refused")
        .with_cause("models.credential_surface_refused")
        .into_response(),
        // R05 RR1 F04 / RR2 F31: the login/logout/registry surfaces aimed
        // at a KNOWN non-OAuth provider keep the explicit 409 naming the
        // OAuth-only rule. The MODEL LISTING route deliberately does NOT
        // map NotOAuth through here — RR2 §四E restored that one face to
        // the original leaf boundary (LA-CFEC64F68DDE: a 404 with a
        // minimal, non-disclosing body; see `list_oauth_models`).
        CredentialError::NotOAuth { .. } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::InvalidMessage,
            format!("oauth-only surface refused: {err}"),
        )
        .with_reason("oauth_only_surface")
        .with_cause("models.oauth_only_surface")
        .into_response(),
        CredentialError::HandleRefused { .. } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::InvalidMessage,
            format!("login transaction refused: {err}"),
        )
        .with_reason("oauth_login_refused")
        .with_cause("models.oauth_login_refused")
        .into_response(),
        CredentialError::NotLoggedIn { .. } | CredentialError::ReauthorizationRequired { .. } => {
            EndpointError::new(
                StatusCode::CONFLICT,
                ErrorCode::Unauthorized,
                format!("oauth login required or failed: {err}"),
            )
            .with_reason("oauth_reauthorization_required")
            .with_cause("models.oauth_reauthorization_required")
            .into_response()
        }
        CredentialError::PersistenceFailed { .. } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Internal,
            format!("credential persistence failed: {err}"),
        )
        .with_reason("credential_persist_failed")
        .with_cause("models.credentials_persist_failed")
        .into_response(),
        CredentialError::Revoked { .. } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::Unauthorized,
            format!("credential revoked: {err}"),
        )
        .with_reason("credential_revoked")
        .with_cause("models.credentials_revoked")
        .into_response(),
        CredentialError::StaleRoute { .. } => EndpointError::new(
            StatusCode::CONFLICT,
            ErrorCode::UpstreamUnavailable,
            format!("configuration changed mid-flight: {err}"),
        )
        .with_reason("model_plane_reloaded")
        .with_cause("models.plane_reloaded")
        .into_response(),
        CredentialError::Transient { .. } => EndpointError::new(
            StatusCode::BAD_GATEWAY,
            ErrorCode::UpstreamUnavailable,
            format!("oauth transport failed: {err}"),
        )
        .with_reason("oauth_transport_failed")
        .with_cause("models.oauth_transport_failed")
        .into_response(),
    }
}

/// R05 RR1 F04 (LA-99D6C304D697, start half): starts an OAuth login for
/// one provider through the single credential authority. Local-only.
async fn start_model_login(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    RoutePath(provider): RoutePath<String>,
) -> Response {
    if principal.kind != PrincipalKind::LocalUser {
        return EndpointError::local_only().into_response();
    }
    let Some(credentials) = state.credential_service() else {
        return EndpointError::not_found()
            .with_reason("model_plane_unconfigured")
            .with_cause("models.unconfigured")
            .into_response();
    };
    let started = credentials
        .oauth_start(&principal.principal_id, &provider)
        .await;
    match started {
        Ok(start) => {
            let now = state.clock.now_unix_ms();
            if let Err(err) = state.management.change(|next| {
                audit_with_metadata(
                    next,
                    "models.credentials.login.start",
                    "model-plane",
                    now,
                    serde_json::json!({"provider": provider}),
                );
                Ok(())
            }) {
                return err.into_response();
            }
            Json(serde_json::json!({"ok": true, "provider": provider, "start": start}))
                .into_response()
        }
        Err(err) => credential_surface_error(err),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginCallbackInput {
    state: String,
    code: String,
}

/// R05 RR1 F04 (LA-99D6C304D697, 手输码 callback half): completes an
/// authorization-code login with a manually supplied (state, code).
/// One-shot: a wrong state, a replay or an expired transaction refuses
/// with ZERO writes.
async fn complete_model_login(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    RoutePath(provider): RoutePath<String>,
    input: Result<Json<LoginCallbackInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if principal.kind != PrincipalKind::LocalUser {
        return EndpointError::local_only().into_response();
    }
    let Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let Some(credentials) = state.credential_service() else {
        return EndpointError::not_found()
            .with_reason("model_plane_unconfigured")
            .with_cause("models.unconfigured")
            .into_response();
    };
    match credentials
        .oauth_complete_code(
            &principal.principal_id,
            &provider,
            &input.state,
            &input.code,
        )
        .await
    {
        Ok(outcome) => {
            let now = state.clock.now_unix_ms();
            if let Err(err) = state.management.change(|next| {
                audit_with_metadata(
                    next,
                    "models.credentials.login.callback",
                    "model-plane",
                    now,
                    serde_json::json!({"provider": provider}),
                );
                Ok(())
            }) {
                return err.into_response();
            }
            Json(serde_json::json!({"ok": true, "provider": provider, "outcome": outcome}))
                .into_response()
        }
        Err(err) => credential_surface_error(err),
    }
}

/// R05 RR1 F04 (LA-99D6C304D697, device half): drives ONE device-code
/// poll round. The client loops this endpoint until `loggedIn` or an
/// honest failure.
async fn poll_model_login(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    RoutePath(provider): RoutePath<String>,
) -> Response {
    if principal.kind != PrincipalKind::LocalUser {
        return EndpointError::local_only().into_response();
    }
    let Some(credentials) = state.credential_service() else {
        return EndpointError::not_found()
            .with_reason("model_plane_unconfigured")
            .with_cause("models.unconfigured")
            .into_response();
    };
    match credentials
        .oauth_poll_device(&principal.principal_id, &provider)
        .await
    {
        Ok(outcome) => {
            Json(serde_json::json!({"ok": true, "provider": provider, "outcome": outcome}))
                .into_response()
        }
        Err(err) => credential_surface_error(err),
    }
}

/// R05 RR1 F04 (LA-FC80B6C4FBE4): logout — deletes the credential, clears
/// the auth cache, refreshes the model list; the honest result (and the
/// refreshed listing) is the response.
async fn logout_model_credential(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    RoutePath(provider): RoutePath<String>,
) -> Response {
    if principal.kind != PrincipalKind::LocalUser {
        return EndpointError::local_only().into_response();
    }
    let Some(credentials) = state.credential_service() else {
        return EndpointError::not_found()
            .with_reason("model_plane_unconfigured")
            .with_cause("models.unconfigured")
            .into_response();
    };
    match credentials.logout(&provider).await {
        Ok(listing) => {
            let now = state.clock.now_unix_ms();
            if let Err(err) = state.management.change(|next| {
                audit_with_metadata(
                    next,
                    "models.credentials.logout",
                    "model-plane",
                    now,
                    serde_json::json!({"provider": provider}),
                );
                Ok(())
            }) {
                return err.into_response();
            }
            Json(serde_json::json!({"ok": true, "provider": provider, "models": listing.models}))
                .into_response()
        }
        Err(lingxi_adapters::models::credentials::CredentialError::NotConfigured { .. }) => {
            EndpointError::not_found()
                .with_reason("model_provider_unknown")
                .with_cause("models.provider_unknown")
                .into_response()
        }
        Err(err) => credential_surface_error(err),
    }
}

/// R05 RR1 F04 (LA-CFEC64F68DDE / LA-CA0BF9A7AEA9): lists the OAuth
/// provider's model ids. R05 RR2 F31 restores the ORIGINAL leaf boundary
/// on this one face: a KNOWN non-OAuth provider is a 404 carrying the
/// SAME minimal body as an unknown provider (no credential-kind, no
/// existence oracle, no widened disclosure — the leaf's "非 OAuth provider
/// 404；不应错误扩大写入或披露范围"). The other OAuth surfaces keep the
/// explicit 409 through `credential_surface_error`.
async fn list_oauth_models(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    RoutePath(provider): RoutePath<String>,
) -> Response {
    if principal.kind != PrincipalKind::LocalUser {
        return EndpointError::local_only().into_response();
    }
    let Some(credentials) = state.credential_service() else {
        return EndpointError::not_found()
            .with_reason("model_plane_unconfigured")
            .with_cause("models.unconfigured")
            .into_response();
    };
    match credentials.oauth_models(&provider).await {
        Ok(listing) => Json(serde_json::json!({
            "ok": true,
            "provider": listing.provider,
            "loggedIn": listing.logged_in,
            "models": listing.models,
        }))
        .into_response(),
        // R05 RR2 F31 (LA-CFEC64F68DDE): unknown providers AND known
        // non-OAuth providers share ONE minimal 404. The OAuth model
        // listing of such a provider does not exist, and the body must
        // not disclose that the provider exists or which credential
        // kind it uses (formatting `err` would leak both).
        Err(
            lingxi_adapters::models::credentials::CredentialError::NotConfigured { .. }
            | lingxi_adapters::models::credentials::CredentialError::NotOAuth { .. },
        ) => EndpointError::not_found()
            .with_reason("model_provider_unknown")
            .with_cause("models.provider_unknown")
            .into_response(),
        Err(err) => credential_surface_error(err),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthModelInput {
    model_id: String,
}

/// R05 RR1 F04 (LA-16CEB6D12A6A): adds a custom modelId to the OAuth
/// provider's registry; the response carries the refreshed listing.
async fn add_oauth_model(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    RoutePath(provider): RoutePath<String>,
    input: Result<Json<OAuthModelInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if principal.kind != PrincipalKind::LocalUser {
        return EndpointError::local_only().into_response();
    }
    let Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let Some(credentials) = state.credential_service() else {
        return EndpointError::not_found()
            .with_reason("model_plane_unconfigured")
            .with_cause("models.unconfigured")
            .into_response();
    };
    match credentials
        .oauth_add_model(&provider, &input.model_id)
        .await
    {
        Ok(listing) => {
            let now = state.clock.now_unix_ms();
            if let Err(err) = state.management.change(|next| {
                audit_with_metadata(
                    next,
                    "models.oauth.model.add",
                    "model-plane",
                    now,
                    serde_json::json!({"provider": provider, "modelId": input.model_id}),
                );
                Ok(())
            }) {
                return err.into_response();
            }
            Json(serde_json::json!({
                "ok": true,
                "provider": listing.provider,
                "loggedIn": listing.logged_in,
                "models": listing.models,
            }))
            .into_response()
        }
        Err(lingxi_adapters::models::credentials::CredentialError::NotConfigured { .. }) => {
            EndpointError::new(
                StatusCode::CONFLICT,
                ErrorCode::InvalidMessage,
                format!(
                    "provider {provider:?}: the model registry serves OAuth providers only, \
                     or the model id is invalid"
                ),
            )
            .with_reason("oauth_model_refused")
            .with_cause("models.oauth_model_refused")
            .into_response()
        }
        Err(err) => credential_surface_error(err),
    }
}

/// R05 RR1 F04 (LA-8060BE8AA02C): removes a custom modelId from the OAuth
/// provider's registry; the response carries the refreshed listing.
async fn remove_oauth_model(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    RoutePath((provider, model_id)): RoutePath<(String, String)>,
) -> Response {
    if principal.kind != PrincipalKind::LocalUser {
        return EndpointError::local_only().into_response();
    }
    let Some(credentials) = state.credential_service() else {
        return EndpointError::not_found()
            .with_reason("model_plane_unconfigured")
            .with_cause("models.unconfigured")
            .into_response();
    };
    match credentials.oauth_remove_model(&provider, &model_id).await {
        Ok(listing) => {
            let now = state.clock.now_unix_ms();
            if let Err(err) = state.management.change(|next| {
                audit_with_metadata(
                    next,
                    "models.oauth.model.remove",
                    "model-plane",
                    now,
                    serde_json::json!({"provider": provider, "modelId": model_id}),
                );
                Ok(())
            }) {
                return err.into_response();
            }
            Json(serde_json::json!({
                "ok": true,
                "provider": listing.provider,
                "loggedIn": listing.logged_in,
                "models": listing.models,
            }))
            .into_response()
        }
        Err(lingxi_adapters::models::credentials::CredentialError::NotConfigured { .. }) => {
            EndpointError::new(
                StatusCode::CONFLICT,
                ErrorCode::InvalidMessage,
                format!(
                    "provider {provider:?}: model id {model_id:?} is not in the registry \
                     (or the provider is not OAuth)"
                ),
            )
            .with_reason("oauth_model_refused")
            .with_cause("models.oauth_model_refused")
            .into_response()
        }
        Err(err) => credential_surface_error(err),
    }
}

fn reload_invalid(source: &crate::config::ModelPlaneSource, detail: &str) -> Response {
    EndpointError::new(
        StatusCode::CONFLICT,
        ErrorCode::InvalidMessage,
        format!(
            "model plane reload from {} refused: {detail}",
            source.path().display()
        ),
    )
    .with_reason("model_plane_reload_invalid")
    .with_cause("models.reload_invalid")
    .into_response()
}

async fn server_identity(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Extension(transport): Extension<crate::serve::SecureTransport>,
) -> Response {
    let record = match state.management.read_fresh() {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let mut capabilities = std::collections::BTreeSet::new();
    for scope in &principal.scopes {
        capabilities.insert(scope.as_str());
        if let Some(namespace) = scope.split('.').next() {
            capabilities.insert(namespace);
        }
    }
    Json(serde_json::json!({
        "serverId": record.server_id,
        "serverNodeId": record.server_id,
        "serverNodeKind": crate::SERVER_KIND,
        "serverNodeTransport": if transport.0 { "https" } else { "http" },
        "studioId": record.studio_id,
        "userId": principal.user_id,
        "label": "Lingxi Rust service",
        "userLabel": record.account.display_name,
        "studioLabel": "Personal Studio",
        "connectionKind": principal.connection_kind,
        "credentialKind": principal.credential_kind,
        "trustState": principal.trust_state,
        "authState": if principal.kind == PrincipalKind::Device { "paired" } else { "user" },
        "capabilities": capabilities,
        "version": crate::server_version(),
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThinkingQuery {
    session_path: Option<String>,
    pending_new_session: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThinkingInput {
    session_path: Option<String>,
    level: String,
}

fn canonical_level(level: &str) -> Option<&'static str> {
    match level.trim().to_ascii_lowercase().as_str() {
        "off" => Some("off"),
        "low" => Some("low"),
        "medium" | "auto" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        "max" | "ultracode" => Some("max"),
        _ => None,
    }
}

fn model_state_unavailable() -> EndpointError {
    EndpointError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        ErrorCode::Internal,
        "current model state unavailable",
    )
    .with_reason("model_state_unavailable")
    .with_cause("model.state_unavailable")
}

async fn check_session(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
) -> Result<(), EndpointError> {
    if session_id.is_empty()
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.len() > 128
    {
        return Err(EndpointError::invalid_message("invalid sessionPath"));
    }
    match state.sessions.get_for(principal, session_id).await {
        Ok(crate::sessions::SessionAccess::Ok(_)) => Ok(()),
        Ok(crate::sessions::SessionAccess::NotFound) => Err(EndpointError::not_found()),
        Ok(crate::sessions::SessionAccess::Forbidden) => {
            Err(EndpointError::forbidden("session_owner_mismatch"))
        }
        Err(err) => Err(EndpointError::storage(&err)),
    }
}

async fn read_thinking_level(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<ThinkingQuery>,
) -> Response {
    let session = if query.pending_new_session.as_deref() == Some("1") {
        None
    } else {
        query.session_path.as_deref().filter(|v| !v.is_empty())
    };
    if let Some(id) = session {
        if let Err(e) = check_session(&state, &principal, id).await {
            return e.into_response();
        }
    }
    model_state_unavailable().into_response()
}

async fn write_thinking_level(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    input: Result<Json<ThinkingInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let Some(level) = canonical_level(&input.level) else {
        return EndpointError::invalid_message("invalid thinking level").into_response();
    };
    let session = input.session_path.as_deref().filter(|v| !v.is_empty());
    if let Some(id) = session {
        if let Err(e) = check_session(&state, &principal, id).await {
            if e.status == StatusCode::NOT_FOUND {
                return EndpointError::new(
                    StatusCode::CONFLICT,
                    ErrorCode::Conflict,
                    "session unavailable",
                )
                .with_reason("session_unavailable")
                .into_response();
            }
            return e.into_response();
        }
    }
    let _ = level;
    model_state_unavailable().into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebLoginInput {
    credential: Option<String>,
    username: Option<String>,
    password: Option<String>,
    client_kind: Option<String>,
}

fn cookie_header(secret: &str, secure: bool, max_age: u64) -> HeaderValue {
    let mut value =
        format!("hana_session={secret}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}");
    if secure {
        value.push_str("; Secure");
    }
    HeaderValue::from_str(&value).expect("cookie only contains server-generated base64url")
}

async fn web_login(
    State(state): State<ServiceState>,
    Extension(connection): Extension<crate::ConnectionKind>,
    Extension(transport): Extension<crate::serve::SecureTransport>,
    input: Result<Json<WebLoginInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let principal = if let Some(credential) = input
        .credential
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        match state.auth.authenticate(
            Some(&format!("Bearer {credential}")),
            None,
            false,
            connection,
        ) {
            Ok(v) => v,
            Err(denial) if denial.reason == "auth_registry_unavailable" => {
                return EndpointError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::Internal,
                    "device registry unavailable",
                )
                .with_reason("device_registry_failure")
                .with_cause("auth.device_registry_failure")
                .into_response()
            }
            Err(_) => return EndpointError::forbidden("invalid_credential").into_response(),
        }
    } else {
        let (Some(username), Some(password)) =
            (input.username.as_deref(), input.password.as_deref())
        else {
            return EndpointError::invalid_message("credential or username and password required")
                .into_response();
        };
        if connection != crate::ConnectionKind::Local && !transport.0 {
            return EndpointError::invalid_message("password_login_requires_secure_context")
                .into_response();
        }
        let username = username.trim();
        let manager = state.management.clone();
        let username = username.to_string();
        let password = password.to_string();
        let now = state.clock.now_unix_ms();
        let account = match tokio::task::spawn_blocking(move || {
            manager.verify_password_and_upgrade(&username, &password, now, connection)
        })
        .await
        {
            Ok(Ok(Some(account))) => account,
            Ok(Ok(None)) => return EndpointError::forbidden("invalid_credential").into_response(),
            Ok(Err(err)) => return err.into_response(),
            Err(err) => {
                return manager_error(format!("password verification worker failed: {err}"))
                    .into_response()
            }
        };
        let scopes = match input.client_kind.as_deref() {
            Some("desktop") => [
                "chat",
                "resources.read",
                "files.read",
                "files.write",
                "studio.owner",
                "settings.read",
                "settings.write",
                "providers.manage",
                "secrets.write",
                "bridge.manage",
            ]
            .as_slice(),
            _ => ["chat", "resources.read", "files.read", "files.write"].as_slice(),
        };
        Principal {
            schema_version: crate::auth::AUTH_SCHEMA_VERSION,
            principal_id: format!("principal_web_account_{}", account.user_id),
            kind: PrincipalKind::LocalUser,
            user_id: Some(account.user_id),
            studio_id: None,
            server_node_id: None,
            device_id: None,
            credential_id: None,
            web_session_id: None,
            connection_kind: connection.into(),
            credential_kind: CredentialKind::WebSession,
            trust_state: if connection == crate::ConnectionKind::Local {
                TrustState::Local
            } else {
                TrustState::Lan
            },
            scopes: scopes.iter().map(|v| (*v).into()).collect(),
        }
    };
    let secret = match random_base64url(32) {
        Ok(v) => format!("hana_web_{v}"),
        Err(e) => return auth_store_error(e).into_response(),
    };
    let salt = match random_base64url(16) {
        Ok(v) => v,
        Err(e) => return auth_store_error(e).into_response(),
    };
    let id = match random_base64url(16) {
        Ok(v) => format!("web_{v}"),
        Err(e) => return auth_store_error(e).into_response(),
    };
    let now = state.clock.now_unix_ms();
    let expires = match now.checked_add(WEB_TTL_MS) {
        Some(v) => v,
        None => return manager_error("web session expiry overflow".into()).into_response(),
    };
    let session = WebSession {
        session_id: id,
        secret_salt: salt.clone(),
        secret_hash: hash_secret(&secret, &salt),
        principal: principal.clone(),
        secure_required: transport.0,
        expires_at_unix_ms: expires,
        status: "active".into(),
    };
    if let Err(e) = state.management.change(|next| {
        next.web_sessions.push(session);
        Ok(())
    }) {
        return e.into_response();
    }
    let mut response =
        Json(serde_json::json!({"ok": true, "expiresAtUnixMs": expires, "principal": principal}))
            .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie_header(&secret, transport.0, WEB_TTL_MS / 1000),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn web_session_status(
    State(state): State<ServiceState>,
    Extension(connection): Extension<crate::ConnectionKind>,
    Extension(transport): Extension<crate::serve::SecureTransport>,
    headers: HeaderMap,
) -> Response {
    let now = state.clock.now_unix_ms();
    let cookie = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    let principal = match state
        .management
        .authenticate_cookie(cookie, now, transport.0)
    {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let principal = principal.filter(|v| v.connection_kind == connection.into());
    let principal = match principal {
        Some(value) => match state.auth.validate_principal(&value) {
            Ok(()) => Some(value),
            Err(denial) if denial.reason == "auth_registry_unavailable" => {
                return EndpointError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::Internal,
                    "device registry unavailable",
                )
                .with_reason("device_registry_failure")
                .with_cause("auth.device_registry_failure")
                .into_response()
            }
            Err(_) => None,
        },
        None => None,
    };
    Json(serde_json::json!({"authenticated": principal.is_some(), "principal": principal}))
        .into_response()
}

async fn web_logout(
    State(state): State<ServiceState>,
    Extension(connection): Extension<crate::ConnectionKind>,
    Extension(transport): Extension<crate::serve::SecureTransport>,
    headers: HeaderMap,
) -> Response {
    let secret = parse_cookie(headers.get(header::COOKIE).and_then(|v| v.to_str().ok()));
    if let Some(secret) = secret {
        if let Err(e) = state
            .management
            .revoke_cookie(&secret, connection, transport.0)
        {
            return e.into_response();
        }
    }
    let mut response = Json(serde_json::json!({"ok": true})).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie_header("", transport.0, 0));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn network_view(
    config: &ServiceConfig,
    stored: &Network,
    port: u16,
    secure: bool,
) -> serde_json::Value {
    let scheme = if secure { "https" } else { "http" };
    let local = format!("{scheme}://127.0.0.1:{port}");
    let base = stored.public_base_url.as_deref();
    let lan_addresses = lan_ipv4_addresses();
    let candidate_lan = lan_addresses
        .first()
        .map(|ip| format!("{scheme}://{ip}:{}", stored.listen_port));
    let lan = if config.network_mode == crate::NetworkMode::Lan {
        lan_addresses
            .first()
            .map(|ip| format!("{scheme}://{ip}:{port}"))
    } else {
        None
    };
    serde_json::json!({
        "mode": stored.mode,
        "listenHost": stored.listen_host,
        "configuredPort": stored.listen_port,
        "actualPort": port,
        "runtimeMode": format!("{}", config.network_mode),
        "runtimeHost": config.bind_addr.ip().to_string(),
        "restartRequired": stored.mode != format!("{}", config.network_mode) || (stored.listen_port != 0 && stored.listen_port != port),
        "publicBaseUrl": base,
        "publicServerUrl": base.map(|b| format!("{b}/")),
        "publicMobileUrl": base.map(|b| format!("{b}/mobile/")),
        "publicDesktopUrl": base.map(|b| format!("{b}/desktop/")),
        "lanAddresses": lan_addresses,
        "localServerUrl": format!("{local}/"),
        "localMobileUrl": format!("{local}/mobile/"),
        "localDesktopUrl": format!("{local}/desktop/"),
        "lanServerUrl": lan.as_ref().map(|b| format!("{b}/")),
        "lanMobileUrl": lan.as_ref().map(|b| format!("{b}/mobile/")),
        "lanDesktopUrl": lan.as_ref().map(|b| format!("{b}/desktop/")),
        "candidateLanServerUrl": candidate_lan.as_ref().map(|b| format!("{b}/")),
        "candidateLanMobileUrl": candidate_lan.as_ref().map(|b| format!("{b}/mobile/")),
        "candidateLanDesktopUrl": candidate_lan.as_ref().map(|b| format!("{b}/desktop/"))
    })
}

fn lan_ipv4_addresses() -> Vec<String> {
    // 不依赖默认路由；同时枚举所有已启用网卡，避免离线局域网和多网卡漏报。
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    lan_ipv4_addresses_from(&interfaces)
}

fn lan_ipv4_addresses_from(interfaces: &[if_addrs::Interface]) -> Vec<String> {
    let mut addresses: Vec<String> = interfaces
        .iter()
        .filter_map(|interface| {
            if !interface.is_oper_up() || interface.is_loopback() || interface.is_link_local() {
                return None;
            }
            let std::net::IpAddr::V4(ip) = interface.ip() else {
                return None;
            };
            if ip.is_unspecified()
                || ip.is_multicast()
                || ip.octets()[0] == 0
                || ip.octets() == [255, 255, 255, 255]
            {
                return None;
            }
            Some(ip.to_string())
        })
        .collect();
    addresses.sort();
    addresses.dedup();
    addresses
}

#[derive(Deserialize)]
struct QrQuery {
    port: Option<u16>,
}

async fn mobile_qr(
    State(state): State<ServiceState>,
    Extension(transport): Extension<crate::serve::SecureTransport>,
    Query(query): Query<QrQuery>,
) -> Response {
    let record = match state.management.read_fresh() {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let network = network_view(
        &state.config,
        &record.network,
        state.actual_port(),
        transport.0,
    );
    let port = query.port.unwrap_or_else(|| state.actual_port());
    if port < 1024 {
        return EndpointError::invalid_message("port must be between 1024 and 65535")
            .into_response();
    }
    let address = network
        .get("publicMobileUrl")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            network
                .get("lanAddresses")
                .and_then(|v| v.as_array())
                .and_then(|v| v.first())
                .and_then(|v| v.as_str())
                .filter(|_| state.config.network_mode == crate::NetworkMode::Lan)
                .map(|ip| {
                    format!(
                        "{}://{ip}:{port}/mobile/",
                        if transport.0 { "https" } else { "http" }
                    )
                })
        });
    let Some(address) = address else {
        return EndpointError::invalid_message("lan_address_unavailable")
            .with_reason("lan_address_unavailable")
            .into_response();
    };
    let Ok(qr) = QrCode::encode_text(&address, QrCodeEcc::Medium) else {
        return EndpointError::invalid_message("mobile address too long for QR").into_response();
    };
    let size = qr.size();
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x, y) {
                use std::fmt::Write as _;
                let _ = write!(&mut path, "M{},{}h1v1h-1z", x + 1, y + 1);
            }
        }
    }
    let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {n} {n}\" shape-rendering=\"crispEdges\"><path fill=\"#fff\" d=\"M0,0h{n}v{n}H0z\"/><path fill=\"#000\" d=\"{path}\"/></svg>", n = size + 2);
    let mut response = (StatusCode::OK, svg).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn access_summary(
    State(state): State<ServiceState>,
    Extension(transport): Extension<crate::serve::SecureTransport>,
) -> Response {
    let record = match state.management.read_fresh() {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let devices = match state.auth.device_access_snapshot() {
        Ok(snapshot) => snapshot,
        Err(err) => return auth_store_error(err).into_response(),
    };
    Json(serde_json::json!({
        "network": network_view(&state.config, &record.network, state.actual_port(), transport.0),
        "account": record.account.view(),
        "devices": devices.devices,
        "credentials": devices.credentials,
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NetworkInput {
    mode: String,
    listen_port: Option<u16>,
    configured_port: Option<u16>,
    #[serde(default, deserialize_with = "optional_nullable_string")]
    public_base_url: Option<Option<String>>,
}

fn optional_nullable_string<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

fn valid_public_base_url(value: &str) -> bool {
    let Ok(uri) = value.parse::<axum::http::Uri>() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    value.len() <= 200
        && !value.contains(char::is_whitespace)
        && !value.contains('#')
        && matches!(uri.scheme_str(), Some("https" | "http"))
        && !authority.host().is_empty()
        && !authority.as_str().contains('@')
        && uri.path() == "/"
        && uri.query().is_none()
}

async fn update_network(
    State(state): State<ServiceState>,
    Extension(transport): Extension<crate::serve::SecureTransport>,
    input: Result<Json<NetworkInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    if input.mode != "loopback" && input.mode != "lan" {
        return EndpointError::invalid_message("mode must be loopback or lan").into_response();
    }
    let requested_port = input.listen_port.or(input.configured_port);
    if requested_port.is_some_and(|port| port < 1024) {
        return EndpointError::invalid_message("listenPort must be between 1024 and 65535")
            .into_response();
    }
    let requested_base = input.public_base_url.map(|value| {
        value
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
    });
    if requested_base
        .as_ref()
        .and_then(Option::as_ref)
        .is_some_and(|s| !valid_public_base_url(s))
    {
        return EndpointError::invalid_message("publicBaseUrl must be a valid http(s) URL")
            .into_response();
    }
    let now = state.clock.now_unix_ms();
    let next_network = match state.management.change(|next| {
        let port = requested_port.unwrap_or(next.network.listen_port);
        if port < 1024 {
            return Err(EndpointError::invalid_message(
                "listenPort must be between 1024 and 65535",
            ));
        }
        let public_base = requested_base
            .clone()
            .unwrap_or_else(|| next.network.public_base_url.clone());
        next.network.mode = input.mode.clone();
        next.network.listen_host = if input.mode == "lan" {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        }
        .into();
        next.network.listen_port = port;
        next.network.public_base_url = public_base.clone();
        let metadata =
            serde_json::json!({"mode": next.network.mode, "listenPort": next.network.listen_port});
        audit_with_metadata(
            next,
            "access.network.update",
            "server-network",
            now,
            metadata,
        );
        Ok(next.network.clone())
    }) {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    Json(serde_json::json!({"ok": true, "network": network_view(&state.config, &next_network, state.actual_port(), transport.0)})).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileInput {
    username: Option<String>,
    display_name: Option<String>,
}

async fn update_profile(
    State(state): State<ServiceState>,
    input: Result<Json<ProfileInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let username = input.username.map(|v| v.trim().to_string());
    let display_name = input.display_name.map(|v| v.trim().to_string());
    if username.as_ref().is_some_and(|v| {
        v.is_empty()
            || v.len() > 64
            || v.contains('/')
            || v.contains('\\')
            || v.chars().any(char::is_control)
    }) || display_name
        .as_ref()
        .is_some_and(|v| v.is_empty() || v.len() > 80 || v.chars().any(char::is_control))
    {
        return EndpointError::invalid_message("invalid username or displayName").into_response();
    }
    let now = state.clock.now_unix_ms();
    match state.management.change(|next| {
        if let Some(v) = username {
            next.account.username = v;
        }
        if let Some(v) = display_name {
            next.account.display_name = v;
        }
        audit(
            next,
            "access.account.profile.update",
            LOCAL_OWNER_USER_ID,
            now,
        );
        Ok(next.account.clone())
    }) {
        Ok(account) => {
            Json(serde_json::json!({"ok": true, "account": account.view()})).into_response()
        }
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize)]
struct PasswordInput {
    password: String,
}

async fn set_password(
    State(state): State<ServiceState>,
    input: Result<Json<PasswordInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    if !(8..=512).contains(&input.password.len()) {
        return EndpointError::invalid_message("password length must be 8..512").into_response();
    }
    let salt = match random_base64url(16) {
        Ok(v) => v,
        Err(e) => return auth_store_error(e).into_response(),
    };
    let salt_for_hash = salt.clone();
    let hash =
        match tokio::task::spawn_blocking(move || hash_password(&input.password, &salt_for_hash))
            .await
        {
            Ok(Ok(value)) => value,
            Ok(Err(err)) => return manager_error(err).into_response(),
            Err(err) => {
                return manager_error(format!("password hashing worker failed: {err}"))
                    .into_response()
            }
        };
    let now = state.clock.now_unix_ms();
    match state.management.change(|next| {
        next.account.password_salt = Some(salt);
        next.account.password_hash = Some(hash);
        next.account.password_algorithm = Some("scrypt-sha256".into());
        audit(
            next,
            "access.account.password.update",
            LOCAL_OWNER_USER_ID,
            now,
        );
        Ok(next.account.clone())
    }) {
        Ok(account) => {
            Json(serde_json::json!({"ok": true, "account": account.view()})).into_response()
        }
        Err(e) => e.into_response(),
    }
}

async fn clear_password(State(state): State<ServiceState>) -> Response {
    let now = state.clock.now_unix_ms();
    match state.management.change(|next| {
        next.account.password_salt = None;
        next.account.password_hash = None;
        next.account.password_algorithm = None;
        audit(
            next,
            "access.account.password.clear",
            LOCAL_OWNER_USER_ID,
            now,
        );
        Ok(next.account.clone())
    }) {
        Ok(account) => {
            Json(serde_json::json!({"ok": true, "account": account.view()})).into_response()
        }
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CredentialInput {
    display_name: Option<String>,
    scopes: Option<Vec<String>>,
    expires_at_unix_ms: Option<u64>,
}

async fn issue_mobile_credential(
    State(state): State<ServiceState>,
    Extension(transport): Extension<crate::serve::SecureTransport>,
    input: Result<Json<CredentialInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    issue_credential(state, input, "mobile", transport.0).await
}
async fn issue_desktop_credential(
    State(state): State<ServiceState>,
    Extension(transport): Extension<crate::serve::SecureTransport>,
    input: Result<Json<CredentialInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    issue_credential(state, input, "desktop", transport.0).await
}

async fn issue_credential(
    state: ServiceState,
    input: Result<Json<CredentialInput>, axum::extract::rejection::JsonRejection>,
    kind: &str,
    secure: bool,
) -> Response {
    let Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let current = match state.management.read_fresh() {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let defaults: &[&str] = if kind == "mobile" {
        &["chat", "resources.read", "files.read", "files.write"]
    } else {
        &[
            "chat",
            "resources.read",
            "files.read",
            "files.write",
            "studio.owner",
            "settings.read",
            "settings.write",
            "providers.manage",
            "secrets.write",
            "bridge.manage",
        ]
    };
    let requested = input
        .scopes
        .unwrap_or_else(|| defaults.iter().map(|s| (*s).to_string()).collect());
    if requested.is_empty() || requested.iter().any(|s| !defaults.contains(&s.as_str())) {
        return EndpointError::invalid_message("unsupported credential scope").into_response();
    }
    let scopes: Vec<String> = if kind == "desktop" {
        defaults.iter().map(|s| (*s).into()).collect()
    } else {
        defaults
            .iter()
            .filter(|&&scope| {
                scope == "resources.read" || requested.iter().any(|item| item == scope)
            })
            .map(|&scope| scope.into())
            .collect()
    };
    if input
        .expires_at_unix_ms
        .is_some_and(|time| time <= state.clock.now_unix_ms())
    {
        return EndpointError::invalid_message("credential expiry must be in the future")
            .into_response();
    }
    let name = input.display_name.unwrap_or_else(|| {
        if kind == "mobile" {
            "Mobile PWA"
        } else {
            "Desktop Frontend"
        }
        .into()
    });
    if name.trim().is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
        return EndpointError::invalid_message("invalid displayName").into_response();
    }
    let refs: Vec<&str> = scopes.iter().map(String::as_str).collect();
    let issued = match state.auth.issue_device_credential_for(
        LOCAL_OWNER_USER_ID,
        &refs,
        input.expires_at_unix_ms,
        kind,
        name.trim(),
    ) {
        Ok(v) => v,
        Err(e) => return auth_store_error(e).into_response(),
    };
    let network = network_view(&state.config, &current.network, state.actual_port(), secure);
    let suffix = if kind == "mobile" {
        "MobileUrl"
    } else {
        "DesktopUrl"
    };
    let address = ["public", "lan", "local"]
        .iter()
        .find_map(|prefix| {
            network
                .get(format!("{prefix}{suffix}"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("");
    Json(serde_json::json!({
        "ok": true, "secret": issued.secret, "accessUrl": address,
        "device": issued.device,
        "credential": issued.credential,
    }))
    .into_response()
}

async fn list_devices(State(state): State<ServiceState>) -> Response {
    let snapshot = match state.auth.device_access_snapshot() {
        Ok(v) => v,
        Err(e) => return auth_store_error(e).into_response(),
    };
    Json(serde_json::json!({"devices": snapshot.devices, "credentials": snapshot.credentials, "pairingSessions": snapshot.pairing_sessions})).into_response()
}

async fn revoke_device(
    State(state): State<ServiceState>,
    RoutePath(device_id): RoutePath<String>,
) -> Response {
    let device = match state.auth.revoke_device(&device_id) {
        Ok(Some(v)) => v,
        Ok(None) => return EndpointError::not_found().into_response(),
        Err(e) => return auth_store_error(e).into_response(),
    };
    Json(serde_json::json!({"ok": true, "device": device})).into_response()
}

async fn revoke_credential(
    State(state): State<ServiceState>,
    RoutePath(credential_id): RoutePath<String>,
) -> Response {
    let credential = match state.auth.revoke_device_credential_view(&credential_id) {
        Ok(Some(v)) => v,
        Ok(None) => return EndpointError::not_found().into_response(),
        Err(e) => return auth_store_error(e).into_response(),
    };
    Json(serde_json::json!({"ok": true, "credential": credential})).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestedDevice {
    device_kind: Option<String>,
    display_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairingInput {
    requested_device: RequestedDevice,
    ttl_ms: Option<u64>,
}

async fn create_pairing(
    State(state): State<ServiceState>,
    input: Result<Json<PairingInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let device_kind = input
        .requested_device
        .device_kind
        .unwrap_or_else(|| "unknown".into());
    if !crate::auth::valid_device_kind(&device_kind) {
        return EndpointError::invalid_message("invalid requestedDevice.deviceKind")
            .into_response();
    }
    let name = input
        .requested_device
        .display_name
        .unwrap_or_else(|| "New Device".into());
    if name.trim().is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
        return EndpointError::invalid_message("invalid requestedDevice.displayName")
            .into_response();
    }
    let ttl = input.ttl_ms.unwrap_or(300_000);
    if !(1..=600_000).contains(&ttl) {
        return EndpointError::invalid_message("ttlMs must be between 1 and 600000")
            .into_response();
    }
    let now = state.clock.now_unix_ms();
    let expires = match now.checked_add(ttl) {
        Some(v) => v,
        None => return EndpointError::invalid_message("ttlMs overflow").into_response(),
    };
    match state
        .auth
        .create_pairing_session(&device_kind, name.trim(), expires, now)
    {
        Ok(created) => Json(serde_json::json!({
            "pairingSessionId": created.pairing.pairing_session_id,
            "userCode": created.user_code,
            "expiresAtUnixMs": created.pairing.expires_at_unix_ms,
            "requestedDevice": created.pairing.requested_device,
        }))
        .into_response(),
        Err(e) => auth_store_error(e).into_response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApproveInput {
    user_code: String,
    scopes: Option<Vec<String>>,
    expires_at_unix_ms: Option<u64>,
}

async fn approve_pairing(
    State(state): State<ServiceState>,
    RoutePath(pairing_id): RoutePath<String>,
    input: Result<Json<ApproveInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let now = state.clock.now_unix_ms();
    let scopes = input.scopes.unwrap_or_else(|| {
        vec![
            "chat".into(),
            "resources.read".into(),
            "files.read".into(),
            "files.write".into(),
        ]
    });
    if scopes.is_empty()
        || scopes
            .iter()
            .any(|s| !LOCAL_OWNER_SCOPES.contains(&s.as_str()))
    {
        return EndpointError::invalid_message("invalid pairing scopes").into_response();
    }
    if input.expires_at_unix_ms.is_some_and(|v| v <= now) {
        return EndpointError::invalid_message("credential expiry must be in the future")
            .into_response();
    }
    let refs: Vec<&str> = scopes.iter().map(String::as_str).collect();
    let approved = match state.auth.approve_pairing_session(
        &pairing_id,
        input.user_code.trim(),
        &refs,
        input.expires_at_unix_ms,
        now,
    ) {
        Ok(value) => value,
        Err(PairingFailure::NotFound) => return EndpointError::not_found().into_response(),
        Err(PairingFailure::Expired) => {
            return EndpointError::invalid_message("pairing expired").into_response()
        }
        Err(PairingFailure::AlreadyConsumed) => {
            return EndpointError::invalid_message("pairing already consumed").into_response()
        }
        Err(PairingFailure::InvalidCode) => {
            return EndpointError::forbidden("invalid_pairing_code").into_response()
        }
        Err(PairingFailure::Store(err)) => return auth_store_error(err).into_response(),
    };
    Json(serde_json::json!({
        "secret": approved.issued.secret,
        "device": approved.issued.device,
        "credential": approved.issued.credential,
        "pairingSession": approved.pairing,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_base_url_is_a_single_http_origin() {
        for value in [
            "https://my-lingxi.example:18799",
            "http://192.168.4.2:18799",
        ] {
            assert!(valid_public_base_url(value), "{value}");
        }
        for value in [
            "https://my-lingxi.example:18799/mobile",
            "https://my-lingxi.example:18799?token=secret",
            "https://user@my-lingxi.example:18799",
            "https://my-lingxi.example:18799#fragment",
            "https://",
            "javascript://my-lingxi.example",
        ] {
            assert!(!valid_public_base_url(value), "{value}");
        }
    }

    #[test]
    fn lan_address_filter_covers_multiple_interfaces_without_default_route() {
        use if_addrs::{IfAddr, IfOperStatus, Ifv4Addr, Interface};
        fn interface(address: [u8; 4], status: IfOperStatus) -> Interface {
            Interface {
                name: format!("test-{}", address[3]),
                addr: IfAddr::V4(Ifv4Addr {
                    ip: std::net::Ipv4Addr::from(address),
                    netmask: std::net::Ipv4Addr::new(255, 255, 255, 0),
                    prefixlen: 24,
                    broadcast: None,
                }),
                index: None,
                oper_status: status,
                is_p2p: false,
                #[cfg(windows)]
                adapter_name: "test-adapter".into(),
            }
        }
        let interfaces = vec![
            interface([192, 168, 50, 9], IfOperStatus::Up),
            interface([10, 0, 0, 7], IfOperStatus::Up),
            interface([192, 168, 50, 9], IfOperStatus::Up),
            interface([172, 16, 0, 8], IfOperStatus::Down),
            interface([127, 0, 0, 1], IfOperStatus::Up),
            interface([169, 254, 1, 2], IfOperStatus::Up),
            interface([224, 1, 2, 3], IfOperStatus::Up),
            interface([0, 0, 0, 0], IfOperStatus::Up),
        ];
        assert_eq!(
            lan_ipv4_addresses_from(&interfaces),
            ["10.0.0.7", "192.168.50.9"]
        );
    }

    #[test]
    fn web_cookie_expires_and_requires_original_transport() {
        let root = std::env::temp_dir().join(format!(
            "lingxi-web-session-test-{}-{}",
            std::process::id(),
            random_base64url(6).unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(MANAGER_FILE);
        let secret = "hana_web_unit_test_secret";
        let salt = "unit-test-salt";
        let principal = Principal {
            schema_version: crate::auth::AUTH_SCHEMA_VERSION,
            principal_id: "principal_web_account_user_local".into(),
            kind: PrincipalKind::LocalUser,
            user_id: Some(LOCAL_OWNER_USER_ID.into()),
            studio_id: None,
            server_node_id: None,
            device_id: None,
            credential_id: None,
            web_session_id: None,
            connection_kind: crate::ConnectionKind::Lan.into(),
            credential_kind: CredentialKind::WebSession,
            trust_state: TrustState::Lan,
            scopes: vec!["chat".into()],
        };
        let state = ManagementState {
            path: path.clone(),
            audit_home: root.clone(),
            inner: Mutex::new(ManagementRecord {
                schema_version: SCHEMA,
                server_id: "server_test".into(),
                studio_id: "studio_test".into(),
                account: Account {
                    user_id: LOCAL_OWNER_USER_ID.into(),
                    username: "local".into(),
                    display_name: "Local".into(),
                    password_salt: None,
                    password_hash: None,
                    password_algorithm: None,
                },
                network: Network {
                    mode: "lan".into(),
                    listen_host: "0.0.0.0".into(),
                    listen_port: 0,
                    public_base_url: None,
                },
                web_sessions: vec![WebSession {
                    session_id: "web_test".into(),
                    secret_salt: salt.into(),
                    secret_hash: hash_secret(secret, salt),
                    principal,
                    secure_required: true,
                    expires_at_unix_ms: WEB_TTL_MS,
                    status: "active".into(),
                }],
                default_thinking_level: "medium".into(),
                session_thinking_levels: BTreeMap::new(),
                audit: Vec::new(),
            }),
        };
        write_record(&path, &state.read()).unwrap();
        let before = std::fs::read(&path).unwrap();
        let cookie = format!("hana_session={secret}");
        let live_principal = match state.authenticate_cookie(Some(&cookie), 1, true) {
            Ok(Some(principal)) => principal,
            _ => panic!("active web session required"),
        };
        assert_eq!(live_principal.web_session_id.as_deref(), Some("web_test"));
        assert!(matches!(
            state.web_session_current(&live_principal, 1, true),
            Ok(true)
        ));
        assert!(matches!(
            state.web_session_current(&live_principal, 1, false),
            Ok(false)
        ));
        assert!(matches!(
            state.web_session_current(&live_principal, WEB_TTL_MS, true),
            Ok(false)
        ));
        assert!(
            !serde_json::to_string(&live_principal)
                .unwrap()
                .contains("web_test"),
            "内部会话号不能进入公开身份响应"
        );
        assert!(matches!(
            state.authenticate_cookie(Some(&cookie), 1, false),
            Ok(None)
        ));
        assert!(matches!(
            state.authenticate_cookie(Some(&cookie), WEB_TTL_MS, true),
            Ok(None)
        ));
        assert!(matches!(
            state.authenticate_cookie(Some("hana_session=wrong"), 1, true),
            Ok(None)
        ));
        assert!(state
            .revoke_cookie(secret, crate::ConnectionKind::Local, true)
            .is_err());
        assert!(state
            .revoke_cookie(secret, crate::ConnectionKind::Lan, false)
            .is_err());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "错误连接和非 TLS 注销不得写状态文件"
        );
        let mut external: ManagementRecord = serde_json::from_slice(&before).unwrap();
        external.web_sessions[0].status = "revoked".into();
        write_record(&path, &external).unwrap();
        assert!(
            matches!(state.authenticate_cookie(Some(&cookie), 1, true), Ok(None)),
            "外部撤销须立刻生效"
        );
        assert!(matches!(
            state.web_session_current(&live_principal, 1, true),
            Ok(false)
        ));
        assert!(state
            .revoke_cookie(secret, crate::ConnectionKind::Lan, true)
            .is_ok());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            serde_json::to_vec_pretty(&external).unwrap()
        );
        std::fs::write(&path, b"{broken").unwrap();
        assert!(
            state.authenticate_cookie(Some(&cookie), 1, true).is_err(),
            "损坏的库不能沿用旧内存认证"
        );
        assert!(state.web_session_current(&live_principal, 1, true).is_err());
        assert!(state
            .change(|next| {
                next.account.username = "wrong".into();
                Ok(())
            })
            .is_err());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"{broken",
            "损坏时不能覆盖旧证据"
        );
        std::fs::write(&path, &before).unwrap();
        assert!(state
            .revoke_cookie(secret, crate::ConnectionKind::Lan, true)
            .is_ok());
        assert!(matches!(
            state.authenticate_cookie(Some(&cookie), 1, true),
            Ok(None)
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn password_scrypt_matches_node_and_upgrades_legacy_record() {
        // 预期值由 Node crypto.scryptSync(..., 64, {N:16384,r:8,p:1}) 独立生成。
        let expected = "2fd20efa1b748a1cabb72b388676c553fc091747fbe8ba50171e775d22932b828dbc475a9ecd4ea75cd877f3a50dfe064aa9ebd31419ef818f1da0e3527f6150";
        assert_eq!(
            hash_password("correct horse battery staple", "unit-test-salt").unwrap(),
            expected
        );
        let root = std::env::temp_dir().join(format!(
            "lingxi-password-migrate-test-{}-{}",
            std::process::id(),
            random_base64url(6).unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(MANAGER_FILE);
        let password = "correct horse battery staple";
        let legacy_salt = "old-salt";
        let state = ManagementState {
            path: path.clone(),
            audit_home: root.clone(),
            inner: Mutex::new(ManagementRecord {
                schema_version: SCHEMA,
                server_id: "server_test".into(),
                studio_id: "studio_test".into(),
                account: Account {
                    user_id: LOCAL_OWNER_USER_ID.into(),
                    username: "alice".into(),
                    display_name: "Alice".into(),
                    password_salt: Some(legacy_salt.into()),
                    password_hash: Some(hash_secret(password, legacy_salt)),
                    password_algorithm: None,
                },
                network: Network {
                    mode: "loopback".into(),
                    listen_host: "127.0.0.1".into(),
                    listen_port: 14500,
                    public_base_url: None,
                },
                web_sessions: Vec::new(),
                default_thinking_level: "medium".into(),
                session_thinking_levels: BTreeMap::new(),
                audit: Vec::new(),
            }),
        };
        write_record(&path, &state.read()).unwrap();
        let initial_bytes = std::fs::read(&path).unwrap();
        assert!(matches!(
            state.verify_password_and_upgrade(
                "alice",
                "wrong password",
                1,
                crate::ConnectionKind::Lan
            ),
            Ok(None)
        ));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            initial_bytes,
            "错误密码不得触发迁移写入"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::metadata(&root).unwrap().permissions();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o500)).unwrap();
            let denied =
                state.verify_password_and_upgrade("alice", password, 2, crate::ConnectionKind::Lan);
            std::fs::set_permissions(&root, permissions).unwrap();
            assert!(denied.is_err(), "迁移写盘失败不能报告登录成功");
            assert!(state.read().account.password_algorithm.is_none());
            assert_eq!(std::fs::read(&path).unwrap(), initial_bytes);
            assert!(
                std::fs::read(root.join("logs/security-audit.jsonl"))
                    .unwrap()
                    .is_empty(),
                "写盘失败不得记录成功迁移审计"
            );
        }
        assert!(matches!(
            state.verify_password_and_upgrade("alice", password, 2, crate::ConnectionKind::Lan),
            Ok(Some(_))
        ));
        let saved: ManagementRecord =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            saved.account.password_algorithm.as_deref(),
            Some("scrypt-sha256")
        );
        assert_eq!(
            saved.account.password_hash.as_deref().map(str::len),
            Some(128)
        );
        assert_eq!(
            saved
                .audit
                .iter()
                .filter(|entry| entry.action == "access.account.password.migrate")
                .count(),
            1
        );
        let log = std::fs::read_to_string(root.join("logs/security-audit.jsonl")).unwrap();
        assert_eq!(log.lines().count(), 1);
        let event: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
        assert_eq!(event["action"], "access.account.password.migrate");
        assert_eq!(event["target"], LOCAL_OWNER_USER_ID);
        assert_eq!(event["actor"]["connectionKind"], "lan");
        assert_eq!(event["actor"]["credentialKind"], "user_session");
        assert!(!log.contains(password), "安全审计不得回写明文密码");
        let saved_bytes = std::fs::read(&path).unwrap();
        assert!(matches!(
            state.verify_password_and_upgrade("alice", password, 3, crate::ConnectionKind::Lan),
            Ok(Some(_))
        ));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            saved_bytes,
            "scrypt login does not write a second migration"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("logs/security-audit.jsonl")).unwrap(),
            log
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn persisted_network_read_is_strict_and_read_only() {
        let omitted: NetworkInput = serde_json::from_str(r#"{"mode":"lan"}"#).unwrap();
        let cleared: NetworkInput =
            serde_json::from_str(r#"{"mode":"lan","publicBaseUrl":null}"#).unwrap();
        assert!(omitted.public_base_url.is_none());
        assert!(matches!(cleared.public_base_url, Some(None)));
        let home = std::env::temp_dir().join(format!(
            "lingxi-network-read-test-{}-{}",
            std::process::id(),
            random_base64url(6).unwrap()
        ));
        let runtime = home.join(crate::paths::RUNTIME_DIR_NAME);
        std::fs::create_dir_all(&runtime).unwrap();
        assert_eq!(read_persisted_network(&home).unwrap(), None);
        let path = runtime.join(MANAGER_FILE);
        let mut record = ManagementRecord {
            schema_version: SCHEMA,
            server_id: "server_test".into(),
            studio_id: "studio_test".into(),
            account: Account {
                user_id: LOCAL_OWNER_USER_ID.into(),
                username: "local".into(),
                display_name: "Local".into(),
                password_salt: None,
                password_hash: None,
                password_algorithm: None,
            },
            network: Network {
                mode: "lan".into(),
                listen_host: "0.0.0.0".into(),
                listen_port: 14500,
                public_base_url: None,
            },
            web_sessions: Vec::new(),
            default_thinking_level: "medium".into(),
            session_thinking_levels: BTreeMap::new(),
            audit: Vec::new(),
        };
        write_record(&path, &record).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            read_persisted_network(&home).unwrap(),
            Some(PersistedNetwork {
                mode: "lan".into(),
                listen_host: "0.0.0.0".into(),
                listen_port: 14500
            })
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "startup read must not rewrite settings"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let state = ManagementState {
                path: path.clone(),
                audit_home: home.clone(),
                inner: Mutex::new(record.clone()),
            };
            let permissions = std::fs::metadata(&runtime).unwrap().permissions();
            std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o500)).unwrap();
            let result = state.change(|next| {
                next.network.mode = "loopback".into();
                next.network.listen_host = "127.0.0.1".into();
                Ok(())
            });
            let live_mode = state.read().network.mode;
            std::fs::set_permissions(&runtime, permissions).unwrap();
            assert!(result.is_err(), "写盘失败必须报错");
            assert_eq!(live_mode, "lan", "失败不得修改同进程状态");
            assert_eq!(
                std::fs::read(&path).unwrap(),
                before,
                "失败不得修改磁盘状态"
            );
        }
        record.network.mode = "loopback".into();
        assert!(
            validated_network(&record).is_err(),
            "LAN host cannot masquerade as loopback"
        );
        record.network.mode = "lan".into();
        record.network.listen_port = 80;
        assert!(validated_network(&record).is_err());
        std::fs::write(&path, b"{corrupt").unwrap();
        assert!(
            read_persisted_network(&home).is_err(),
            "corrupt settings must not silently reset"
        );
        #[cfg(unix)]
        {
            std::fs::remove_file(&path).unwrap();
            let alternate = home.join("alternate.json");
            std::fs::write(&alternate, &before).unwrap();
            std::os::unix::fs::symlink(&alternate, &path).unwrap();
            assert!(
                read_persisted_network(&home).is_err(),
                "设置文件链接不得跳到其他路径"
            );
        }
        let _ = std::fs::remove_dir_all(home);
    }
}
