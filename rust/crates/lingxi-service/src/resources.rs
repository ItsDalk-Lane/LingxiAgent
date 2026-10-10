//! R06-T05 资源面：`res_` 信封、内容解析、资源票据签发/校验与
//! `/lingxi/v1/resources/*` 路由（交付物① ResourceService）。
//!
//! 对照现役（RC-3，逐项锚点进 07_diff_ledger）：
//! - 信封与 id 规则：`lib/resources/resource-envelope.ts`
//!   （schemaVersion 1、`res_` 前缀、`SESSION_FILE_ID_RE`、
//!   `studios/{studioId}/resources/{resourceId}`、lifecycle/storage/links）。
//! - 内容解析：`core/resource-service.ts` resolveContent :44-109
//!   （400/404/410/409/500 错误链、reconcile presentation-only
//!   :128-169、etag `"${mtimeMs36}-${size36}"` :107）。
//! - 票据：`core/resource-ticket-service.ts`（HMAC-SHA256 base64url、
//!   payload schemaVersion 1、action `resources.content`、TTL clamp
//!   [1, 5min]、timingSafeEqual、key 0600 原子写；路径按 D4 从现役
//!   `{lingxiHome}/security/` 映射到候选人私有运行目录）。
//! - 路由面：`server/routes/resources.ts`（If-None-Match→304、
//!   parseRangeHeader :152-173、416 `bytes */size`、Content-Disposition
//!   `inline; filename="ascii"; filename*=UTF-8''pct`）。
//!
//! 候选人纪律差异（台账 D 项）：现役 `server/index.ts:642` 对
//! `?ticket=` 的 content 请求完全豁免鉴权中间件（服务 <img> 等无头
//! 客户端）；候选人中间件对一切请求鉴权（R02 冻结），ticket 降级为
//! 内容级**附加**校验——带 ticket 时必须有效，凭证也必须存在。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use base64::Engine as _;
use hmac::{Hmac, KeyInit as _, Mac as _};
use sha2::Sha256;

use crate::filemeta;
use crate::sessionfiles::{SessionFileService, SessionFileView};

// ── id 规则（resource-envelope.ts :1-16） ──

const SESSION_FILE_RESOURCE_PREFIX: &str = "res_";

/// 现役 SESSION_FILE_ID_RE（`/^sf_[A-Za-z0-9][A-Za-z0-9_-]*$/`）。
fn is_stable_session_file_id(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("sf_") else {
        return false;
    };
    let mut bytes = rest.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// 现役 resourceIdForSessionFileId：不稳定 id（空尾、含空白等）返回 None。
pub fn resource_id_for_session_file_id(file_id: &str) -> Option<String> {
    is_stable_session_file_id(file_id).then(|| format!("{SESSION_FILE_RESOURCE_PREFIX}{file_id}"))
}

/// 现役 fileIdFromSessionFileResourceId。
pub fn file_id_from_session_file_resource_id(resource_id: &str) -> Option<&str> {
    let file_id = resource_id.strip_prefix(SESSION_FILE_RESOURCE_PREFIX)?;
    is_stable_session_file_id(file_id).then_some(file_id)
}

// ── 错误（core/resource-service.ts ResourceError :8-18 +
// core/resource-ticket-service.ts ResourceTicketError :11-21） ──

#[derive(Debug)]
pub struct ResourceError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl ResourceError {
    fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        ResourceError {
            status,
            code: code.to_string(),
            message: message.into(),
        }
    }

    fn ticket_invalid(message: &str) -> Self {
        Self::new(403, "resource_ticket_invalid", message)
    }

    fn internal(detail: impl std::fmt::Display) -> Self {
        Self::new(
            500,
            "resource_error",
            format!("resource service failure: {detail}"),
        )
    }
}

impl std::fmt::Display for ResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for ResourceError {}

// ── 解析结果与票据类型 ──

/// 现役 resolveContent 返回形状（resource-service.ts :99-108）。
#[derive(Debug)]
pub struct ResolvedContent {
    pub resource_id: String,
    pub file_path: PathBuf,
    pub mime: String,
    pub size: u64,
    pub filename: String,
    pub mtime_ms: u64,
    pub etag: String,
}

/// 现役 issueResourceTicket 的对外字段（resource-ticket-service.ts :38-54）。
#[derive(Debug)]
pub struct IssuedTicket {
    pub ticket: String,
    pub ticket_id: String,
    pub resource_id: String,
    pub action: String,
    pub expires_at: String,
}

/// 现役 verifyResourceTicket 的对外字段（:96-105）。
#[derive(Debug)]
pub struct VerifiedTicket {
    pub ticket_id: String,
    pub resource_id: String,
    pub principal_id: String,
    pub expires_at: String,
}

// ── 服务 ──

pub const RESOURCE_TICKET_ACTION: &str = "resources.content";
pub const DEFAULT_RESOURCE_TICKET_TTL_MS: u64 = 5 * 60 * 1000;
const RESOURCE_TICKET_KEY_FILE: &str = "resource-ticket-key";

pub struct ResourceService {
    session_files: Arc<SessionFileService>,
    studio_id: String,
    /// 票据 key 目录（D4：现役 `{lingxiHome}/security/` → 候选人私有
    /// 运行目录 `{runtime_dir}/`，0600 纪律不变）。
    key_dir: PathBuf,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl ResourceService {
    pub fn new(
        session_files: Arc<SessionFileService>,
        studio_id: String,
        key_dir: PathBuf,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        ResourceService {
            session_files,
            studio_id,
            key_dir,
            now: Arc::new(now),
        }
    }

    fn now_ms(&self) -> u64 {
        (self.now)()
    }

    /// 现役 getResource（resource-service.ts :38-42）：id 形状非法抛
    /// 400；未命中返回 None（路由 404）。
    pub async fn get_resource(
        &self,
        resource_id: &str,
    ) -> Result<Option<serde_json::Value>, ResourceError> {
        let view = self.find_reconciled(resource_id).await?;
        Ok(match view {
            Some(view) => envelope_for(&view, &self.studio_id),
            None => None,
        })
    }

    /// 现役 resolveContent（:44-109）错误链逐条对应。
    pub async fn resolve_content(
        &self,
        resource_id: &str,
    ) -> Result<ResolvedContent, ResourceError> {
        let Some(view) = self.find_reconciled(resource_id).await? else {
            return Err(ResourceError::new(
                404,
                "resource_not_found",
                "resource not found",
            ));
        };
        // 形状非法的 id 在 find_reconciled 已抛 400；envelope 必然可构
        // （file_id 已过稳定校验）。
        if view.status == "expired" {
            return Err(ResourceError::new(
                410,
                "resource_expired",
                "resource expired",
            ));
        }
        if view.is_directory {
            return Err(ResourceError::new(
                409,
                "resource_is_directory",
                "resource content is not available for directories",
            ));
        }
        let source = if !view.real_path.is_empty() {
            view.real_path.clone()
        } else {
            view.file_path.clone()
        };
        if source.is_empty() || !Path::new(&source).is_absolute() {
            return Err(ResourceError::new(
                500,
                "invalid_resource_content_path",
                "resource content path is invalid",
            ));
        }
        let real = std::fs::canonicalize(&source).map_err(|_| {
            ResourceError::new(404, "resource_content_missing", "resource content missing")
        })?;
        let meta = std::fs::metadata(&real).map_err(|_| {
            ResourceError::new(404, "resource_content_missing", "resource content missing")
        })?;
        if !meta.is_file() {
            return Err(ResourceError::new(
                409,
                "resource_not_file",
                "resource content is not a regular file",
            ));
        }
        let size = meta.len();
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(view.mtime_ms);
        // 现役 :107 `"{mtimeMs.toString(36)}-{size.toString(36)}"`。
        let etag = format!(
            "\"{}-{}\"",
            filemeta::to_base36(mtime_ms),
            filemeta::to_base36(size)
        );
        let filename = if view.filename.is_empty() {
            real.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "download".to_string())
        } else {
            view.filename.clone()
        };
        let mime = if view.mime.is_empty() {
            "application/octet-stream".to_string()
        } else {
            view.mime.clone()
        };
        Ok(ResolvedContent {
            resource_id: resource_id.to_string(),
            file_path: real,
            mime,
            size,
            filename,
            mtime_ms,
            etag,
        })
    }

    /// 现役 _findSessionFileByResourceId（:111-126）+ reconcile
    /// （:128-169，presentation-only：不持久化 missing/mtime）。
    async fn find_reconciled(
        &self,
        resource_id: &str,
    ) -> Result<Option<SessionFileView>, ResourceError> {
        let Some(file_id) = file_id_from_session_file_resource_id(resource_id) else {
            return Err(ResourceError::new(
                400,
                "invalid_resource_id",
                "invalid resource id",
            ));
        };
        // owner 命中优先，alias 目标兜底（现役 :176-177 注释纪律）。
        let view = self
            .session_files
            .get_file_global(file_id)
            .await
            .map_err(ResourceError::internal)?;
        Ok(view.map(|view| self.reconcile(view)))
    }

    /// 现役 _reconcileFileAvailability（:128-169）：expired 不动；源不可
    /// stat → status "missing"；可 stat → 以盘上事实刷新
    /// realPath/status/mtime/size/isDirectory。候选人差异：missingAt 不
    /// 落盘也不入信封（presentation-only，台账申报）。
    fn reconcile(&self, view: SessionFileView) -> SessionFileView {
        if view.status == "expired" {
            return view;
        }
        let source = if !view.real_path.is_empty() {
            view.real_path.clone()
        } else {
            view.file_path.clone()
        };
        if source.is_empty() || !Path::new(&source).is_absolute() {
            return view;
        }
        let now = self.now_ms();
        let _ = now; // missingAt 不持久化（见上注）
        let Ok(real) = std::fs::canonicalize(&source) else {
            let mut out = view;
            out.status = "missing".to_string();
            return out;
        };
        let Ok(meta) = std::fs::metadata(&real) else {
            let mut out = view;
            out.status = "missing".to_string();
            return out;
        };
        let is_directory = meta.is_dir();
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(view.mtime_ms);
        let mut out = view;
        out.real_path = real.to_string_lossy().into_owned();
        out.status = "available".to_string();
        out.mtime_ms = mtime_ms;
        out.size_bytes = if is_directory { 0 } else { meta.len() };
        out.is_directory = is_directory;
        out
    }

    // ── 票据（resource-ticket-service.ts） ──

    /// 现役 issueResourceTicket（:23-54）。TTL clamp [1, 5min]；payload
    /// 字段集与现役一致；key 文件首次使用时生成（0600 原子写）。
    pub fn issue_ticket(
        &self,
        resource_id: &str,
        principal_id: &str,
    ) -> Result<IssuedTicket, ResourceError> {
        self.issue_ticket_with_ttl(resource_id, principal_id, DEFAULT_RESOURCE_TICKET_TTL_MS)
    }

    fn issue_ticket_with_ttl(
        &self,
        resource_id: &str,
        principal_id: &str,
        ttl_ms: u64,
    ) -> Result<IssuedTicket, ResourceError> {
        if resource_id.trim().is_empty() || principal_id.trim().is_empty() {
            return Err(ResourceError::new(
                500,
                "resource_error",
                "resourceId and principalId are required",
            ));
        }
        let issued_ms = self.now_ms();
        let safe_ttl = ttl_ms.clamp(1, DEFAULT_RESOURCE_TICKET_TTL_MS);
        let ticket_id = format!("rt_{}", uuid_v4()?);
        let issued_at = iso_millis(issued_ms)?;
        let expires_at = iso_millis(issued_ms + safe_ttl)?;
        let payload = serde_json::json!({
            "schemaVersion": 1,
            "ticketId": ticket_id,
            "resourceId": resource_id,
            "studioId": self.studio_id,
            "action": RESOURCE_TICKET_ACTION,
            "principalId": principal_id,
            "issuedAt": issued_at,
            "expiresAt": expires_at,
        });
        let body = b64url_encode(payload.to_string().as_bytes());
        let key = read_or_create_ticket_key(&self.key_dir)?;
        let signature = sign_body(&key, &body);
        Ok(IssuedTicket {
            ticket: format!("{body}.{signature}"),
            ticket_id,
            resource_id: resource_id.to_string(),
            action: RESOURCE_TICKET_ACTION.to_string(),
            expires_at,
        })
    }

    /// 现役 verifyResourceTicket（:56-106）：形状 → 签名（常量时间）→
    /// payload → schemaVersion/action → resourceId → 过期。
    pub fn verify_ticket(
        &self,
        ticket: &str,
        resource_id: &str,
    ) -> Result<VerifiedTicket, ResourceError> {
        if ticket.trim().is_empty() {
            return Err(ResourceError::ticket_invalid("resource ticket required"));
        }
        let segments: Vec<&str> = ticket.split('.').collect();
        if segments.len() != 2 || segments[0].is_empty() || segments[1].is_empty() {
            return Err(ResourceError::ticket_invalid("resource ticket malformed"));
        }
        let (body, signature) = (segments[0], segments[1]);
        let key = read_or_create_ticket_key(&self.key_dir)?;
        let expected = sign_body(&key, body);
        if !timing_safe_eq(signature, &expected) {
            return Err(ResourceError::ticket_invalid(
                "resource ticket signature invalid",
            ));
        }
        let decoded = b64url_decode(body)
            .map_err(|_| ResourceError::ticket_invalid("resource ticket payload invalid"))?;
        let payload: serde_json::Value = serde_json::from_slice(&decoded)
            .map_err(|_| ResourceError::ticket_invalid("resource ticket payload invalid"))?;
        if payload.get("schemaVersion") != Some(&serde_json::json!(1))
            || payload.get("action").and_then(|v| v.as_str()) != Some(RESOURCE_TICKET_ACTION)
        {
            return Err(ResourceError::ticket_invalid(
                "resource ticket action invalid",
            ));
        }
        if payload.get("resourceId").and_then(|v| v.as_str()) != Some(resource_id) {
            return Err(ResourceError::ticket_invalid(
                "resource ticket resource mismatch",
            ));
        }
        let Some(expires_ms) = payload
            .get("expiresAt")
            .and_then(|v| v.as_str())
            .and_then(parse_iso_millis)
        else {
            return Err(ResourceError::ticket_invalid(
                "resource ticket timestamp invalid",
            ));
        };
        if expires_ms <= self.now_ms() as i64 {
            return Err(ResourceError::new(
                403,
                "resource_ticket_expired",
                "resource ticket expired",
            ));
        }
        Ok(VerifiedTicket {
            ticket_id: payload
                .get("ticketId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            resource_id: resource_id.to_string(),
            principal_id: payload
                .get("principalId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            expires_at: payload
                .get("expiresAt")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        })
    }
}

// ── 信封（resource-envelope.ts :18-69） ──

fn envelope_for(view: &SessionFileView, studio_id: &str) -> Option<serde_json::Value> {
    if studio_id.trim().is_empty() {
        return None;
    }
    let resource_id = resource_id_for_session_file_id(&view.file_id)?;
    let display_name = view.label.as_deref().unwrap_or(&view.filename);
    let mut links = serde_json::json!({
        "self": format!("/lingxi/v1/resources/{resource_id}"),
    });
    if view.status == "available" && !view.is_directory {
        links["content"] = serde_json::json!(format!("/lingxi/v1/resources/{resource_id}/content"));
    }
    Some(serde_json::json!({
        "schemaVersion": 1,
        "resourceId": resource_id,
        "name": format!("studios/{studio_id}/resources/{resource_id}"),
        "studioId": studio_id,
        "type": "file",
        "source": "session_file",
        "sourceId": view.file_id,
        "fileId": view.file_id,
        "displayName": display_name,
        "filename": view.filename,
        "ext": filemeta::ext_of_name(&view.filename),
        "mime": view.mime,
        "size": if view.is_directory { serde_json::Value::Null } else { serde_json::json!(view.size_bytes) },
        "kind": view.kind,
        "isDirectory": view.is_directory,
        "origin": view.origin,
        "createdAt": view.created_at_ms,
        "mtimeMs": view.mtime_ms,
        "lifecycle": {
            "status": view.status,
            "missingAt": serde_json::Value::Null,
        },
        "storage": {
            "provider": "session_file",
            "storageKind": view.storage_kind,
            "localOnly": true,
        },
        "links": links,
    }))
}

// ── 票据内部 ──

/// 现役 resourceTicketKeyPath（:108-111）→ D4 映射：现役
/// `{lingxiHome}/security/resource-ticket-key`（core/security-dir.ts）
/// 挪进候选人私有运行目录 `{runtime_dir}/resource-ticket-key`（0600
/// 纪律与原子写不变；该 key 无外部消费者，路径是纯内部选择）。
fn ticket_key_path(key_dir: &Path) -> PathBuf {
    key_dir.join(RESOURCE_TICKET_KEY_FILE)
}

/// 现役 readOrCreateTicketKey（:120-132）：读出非空即用；否则 32 字节
/// 随机 → base64url，0600 原子写（末尾换行，读出时 trim）。
fn read_or_create_ticket_key(key_dir: &Path) -> Result<String, ResourceError> {
    let path = ticket_key_path(key_dir);
    match std::fs::read_to_string(&path) {
        Ok(existing) => {
            let trimmed = existing.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(ResourceError::internal(err)),
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ResourceError::internal)?;
    }
    let mut raw = [0u8; 32];
    getrandom::getrandom(&mut raw).map_err(ResourceError::internal)?;
    let key = b64url_encode(&raw);
    crate::paths::atomic_write_private(&path, format!("{key}\n").as_bytes())
        .map_err(ResourceError::internal)?;
    Ok(key)
}

/// 现役 signBody（:113-118）：HMAC-SHA256（key 为文件内 base64url 字符串
/// 的 UTF-8 字节，与 crypto.createHmac(sha256, keyString) 同口径），
/// 摘要 base64url。
fn sign_body(key: &str, body: &str) -> String {
    // hmac 0.13：构造器在 digest::KeyInit 上（Mac trait 只管 update/finalize）。
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(body.as_bytes());
    b64url_encode(&mac.finalize().into_bytes())
}

/// 现役 timingSafeEqual（:142-146）：长度先行，逐字节累积。
fn timing_safe_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn b64url_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn b64url_decode(value: &str) -> Result<Vec<u8>, base64::DecodeError> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(value)
}

/// RFC 4122 v4 UUID（手写，避免 uuid feature 进锁）。
fn uuid_v4() -> Result<String, ResourceError> {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).map_err(ResourceError::internal)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
        b[14], b[15]
    ))
}

/// `new Date(ms).toISOString()` 形状：恒三位毫秒 + Z。
fn iso_millis(ms: u64) -> Result<String, ResourceError> {
    let ms_i64 = i64::try_from(ms).map_err(|_| ResourceError::internal("timestamp overflow"))?;
    let dt = chrono::DateTime::from_timestamp_millis(ms_i64)
        .ok_or_else(|| ResourceError::internal("timestamp out of range"))?;
    Ok(dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn parse_iso_millis(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

// ── 路由 handler ──

use axum::extract::{Path as AxumPath, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::{EndpointError, Principal, ServiceState};

fn resource_error_response(err: ResourceError) -> Response {
    let status = axum::http::StatusCode::from_u16(err.status)
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    EndpointError::resource_failure(status, &err.code, err.message).into_response()
}

/// `GET /lingxi/v1/resources/{resource_id}` — 信封读取（现役
/// resources.ts :15-24；未命中 404）。
pub async fn get_resource_route(
    axum::Extension(_principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(resource_id): AxumPath<String>,
) -> Response {
    match state.resources().get_resource(&resource_id).await {
        Ok(Some(envelope)) => (axum::http::StatusCode::OK, Json(envelope)).into_response(),
        Ok(None) => EndpointError::not_found().into_response(),
        Err(err) => resource_error_response(err),
    }
}

/// `POST /lingxi/v1/resources/{resource_id}/ticket` — 先 resolveContent
/// （不存在/过期在此被拒），再签发（现役 :26-48）。
pub async fn issue_resource_ticket_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(resource_id): AxumPath<String>,
) -> Response {
    if let Err(err) = state.resources().resolve_content(&resource_id).await {
        return resource_error_response(err);
    }
    match state
        .resources()
        .issue_ticket(&resource_id, &principal.principal_id)
    {
        Ok(issued) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "ticket": issued.ticket,
                "ticketId": issued.ticket_id,
                "resourceId": resource_id,
                "expiresAt": issued.expires_at,
                "contentUrl": format!(
                    "/lingxi/v1/resources/{}/content?ticket={}",
                    encode_uri_component(&resource_id),
                    encode_uri_component(&issued.ticket)
                ),
            })),
        )
            .into_response(),
        Err(err) => resource_error_response(err),
    }
}

/// `GET|HEAD /lingxi/v1/resources/{resource_id}/content` — 现役
/// serveResourceContent（:103-141）：ticket 有则先验（候选人纪律：
/// 鉴权中间件已过，ticket 是内容级附加校验）；If-None-Match→304；
/// Range→206/416；HEAD 由 axum `get` 路由自动应答（headers 一致、
/// body 由 hyper 剥除，与现役 headOnly 空体同口径）。
pub async fn resource_content_route(
    axum::Extension(_principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(resource_id): AxumPath<String>,
    headers: axum::http::HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    if let Some(ticket) = query.get("ticket") {
        if let Err(err) = state.resources().verify_ticket(ticket, &resource_id) {
            return resource_error_response(err);
        }
    }
    let content = match state.resources().resolve_content(&resource_id).await {
        Ok(content) => content,
        Err(err) => return resource_error_response(err),
    };

    if let Some(inm) = headers
        .get(axum::http::header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        if inm == content.etag {
            return (
                axum::http::StatusCode::NOT_MODIFIED,
                [(axum::http::header::ETAG, content.etag.clone())],
                axum::body::Body::empty(),
            )
                .into_response();
        }
    }

    let range = parse_range_header(
        headers
            .get(axum::http::header::RANGE)
            .and_then(|v| v.to_str().ok()),
        content.size,
    );
    if range == RangeHeader::Unsatisfiable {
        return (
            axum::http::StatusCode::RANGE_NOT_SATISFIABLE,
            [
                (
                    axum::http::header::CONTENT_RANGE,
                    format!("bytes */{}", content.size),
                ),
                (axum::http::header::ACCEPT_RANGES, "bytes".to_string()),
            ],
            axum::body::Body::empty(),
        )
            .into_response();
    }

    let (start, end) = match range {
        RangeHeader::Satisfied(start, end) => (start, end),
        // 现役 :123 —— size==0 时 end 落到 -1，length 0。
        RangeHeader::Absent => (0, content.size.saturating_sub(1)),
        RangeHeader::Unsatisfiable => unreachable!("handled above"),
    };
    let length = if content.size == 0 {
        0
    } else {
        end - start + 1
    };
    let status = if matches!(range, RangeHeader::Satisfied(..)) {
        axum::http::StatusCode::PARTIAL_CONTENT
    } else {
        axum::http::StatusCode::OK
    };

    // T05 声明：内容全量内存读（台账 perf note：现役 createReadStream
    // 流式，流式化留待后续任务；语义/头/状态码保真）。tokio 未启用 fs
    // feature（不动 feature 面），阻塞读放进 blocking 池。
    let read_path = content.file_path.clone();
    let bytes = match tokio::task::spawn_blocking(move || std::fs::read(read_path)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) => {
            return resource_error_response(ResourceError::new(
                404,
                "resource_content_missing",
                format!("resource content missing: {err}"),
            ))
        }
        Err(err) => {
            return resource_error_response(ResourceError::internal(err));
        }
    };
    let body = if content.size == 0 {
        Vec::new()
    } else {
        let start = start.min(bytes.len() as u64) as usize;
        let end = (end as usize).min(bytes.len().saturating_sub(1));
        if start > end {
            Vec::new()
        } else {
            bytes[start..=end].to_vec()
        }
    };

    let mut builder = Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, content.mime.clone())
        .header(axum::http::header::ACCEPT_RANGES, "bytes")
        .header(axum::http::header::CONTENT_LENGTH, length.to_string())
        .header(
            axum::http::header::CACHE_CONTROL,
            "private, max-age=0, must-revalidate",
        )
        .header(axum::http::header::ETAG, content.etag.clone())
        .header(
            axum::http::header::CONTENT_DISPOSITION,
            content_disposition(&content.filename),
        );
    if let RangeHeader::Satisfied(start, end) = range {
        builder = builder.header(
            axum::http::header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{}", content.size),
        );
    }
    builder
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| EndpointError::not_found().into_response())
}

// ── Range / Content-Disposition（resources.ts :152-196） ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeHeader {
    Absent,
    Unsatisfiable,
    Satisfied(u64, u64),
}

/// 现役 parseRangeHeader（resources.ts :152-173）：
/// `/^bytes=(\d*)-(\d*)$/`；双空 / 非安全整数 / size<=0 / start>=size /
/// start>end → 不可满足；suffix 语义 `start=max(size-n,0) end=size-1`；
/// 满足时 end 收拢到 `min(end, size-1)`。
fn parse_range_header(value: Option<&str>, size: u64) -> RangeHeader {
    let Some(value) = value else {
        return RangeHeader::Absent;
    };
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return RangeHeader::Unsatisfiable;
    };
    let Some((start_s, end_s)) = spec.split_once('-') else {
        return RangeHeader::Unsatisfiable;
    };
    if end_s.contains('-')
        || !start_s.bytes().all(|b| b.is_ascii_digit())
        || !end_s.bytes().all(|b| b.is_ascii_digit())
    {
        return RangeHeader::Unsatisfiable;
    }
    if start_s.is_empty() && end_s.is_empty() {
        return RangeHeader::Unsatisfiable;
    }
    let (start, end) = if start_s.is_empty() {
        let Ok(suffix) = end_s.parse::<u64>() else {
            return RangeHeader::Unsatisfiable;
        };
        if suffix == 0 {
            return RangeHeader::Unsatisfiable;
        }
        (size.saturating_sub(suffix), size.saturating_sub(1))
    } else {
        let Ok(start) = start_s.parse::<u64>() else {
            return RangeHeader::Unsatisfiable;
        };
        let end = if end_s.is_empty() {
            size.saturating_sub(1)
        } else {
            match end_s.parse::<u64>() {
                Ok(end) => end,
                Err(_) => return RangeHeader::Unsatisfiable,
            }
        };
        (start, end)
    };
    if size == 0 || start >= size || start > end {
        return RangeHeader::Unsatisfiable;
    }
    RangeHeader::Satisfied(start, end.min(size - 1))
}

/// 现役 contentDisposition（:176-178）。
fn content_disposition(filename: &str) -> String {
    format!(
        "inline; filename=\"{}\"; filename*=UTF-8''{}",
        ascii_filename_fallback(filename),
        encode_uri_component(filename)
    )
}

/// 现役 asciiFilenameFallback（:180-196）：扩展名白名单
/// `[A-Za-z0-9]{1,12}`；stem 非可打印 ASCII / `"\\\r\n;/` / 空白 → `_`，
/// 连 `_` 收拢、首尾 `_` 剥离、截 80；stem 不合 `[A-Za-z0-9._-]+` →
/// `download`。
fn ascii_filename_fallback(filename: &str) -> String {
    let dot = filename.rfind('.');
    let ext = dot.map(|i| &filename[i + 1..]).unwrap_or("");
    let safe_ext =
        if !ext.is_empty() && ext.len() <= 12 && ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
            format!(".{ext}")
        } else {
            String::new()
        };
    let stem_src = dot.map(|i| &filename[..i]).unwrap_or(filename);
    let mut replaced = String::new();
    for ch in stem_src.chars() {
        let code = ch as u32;
        if !(0x20..=0x7e).contains(&code)
            || matches!(ch, '"' | '\\' | '\r' | '\n' | ';' | '/')
            || ch.is_whitespace()
        {
            replaced.push('_');
        } else {
            replaced.push(ch);
        }
    }
    let mut collapsed = String::new();
    let mut prev_underscore = false;
    for ch in replaced.chars() {
        if ch == '_' {
            if prev_underscore {
                continue;
            }
            prev_underscore = true;
        } else {
            prev_underscore = false;
        }
        collapsed.push(ch);
    }
    let trimmed = collapsed.trim_matches('_');
    let stem: String = trimmed.chars().take(80).collect();
    if !stem.is_empty()
        && stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
    {
        format!("{stem}{safe_ext}")
    } else {
        format!("download{safe_ext}")
    }
}

/// `encodeURIComponent` 语义：unreserved（`A-Za-z0-9-_.!~*'()`）原样，
/// 其余按 UTF-8 字节 `%XX` 大写。
fn encode_uri_component(value: &str) -> String {
    let mut out = String::new();
    for b in value.as_bytes() {
        match *b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(*b as char),
            _ => {
                use std::fmt::Write as _;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_header_mirrors_incumbent_table() {
        // 现役 parseRangeHeader 判定表逐行对应（size=16）。
        assert_eq!(parse_range_header(None, 16), RangeHeader::Absent);
        assert_eq!(
            parse_range_header(Some("bytes=2-5"), 16),
            RangeHeader::Satisfied(2, 5)
        );
        assert_eq!(
            parse_range_header(Some("bytes=-4"), 16),
            RangeHeader::Satisfied(12, 15)
        );
        assert_eq!(
            parse_range_header(Some("bytes=5-"), 16),
            RangeHeader::Satisfied(5, 15)
        );
        // end 收拢到 size-1。
        assert_eq!(
            parse_range_header(Some("bytes=4-99"), 16),
            RangeHeader::Satisfied(4, 15)
        );
        // 不可满足族。
        assert_eq!(
            parse_range_header(Some("bytes=99-100"), 16),
            RangeHeader::Unsatisfiable
        );
        assert_eq!(
            parse_range_header(Some("bytes="), 16),
            RangeHeader::Unsatisfiable
        );
        assert_eq!(
            parse_range_header(Some("bytes=-0"), 16),
            RangeHeader::Unsatisfiable
        );
        assert_eq!(
            parse_range_header(Some("bytes=0-1"), 0),
            RangeHeader::Unsatisfiable
        );
        assert_eq!(
            parse_range_header(Some("items=0-1"), 16),
            RangeHeader::Unsatisfiable
        );
        assert_eq!(
            parse_range_header(Some("bytes=5-4"), 16),
            RangeHeader::Unsatisfiable
        );
        assert_eq!(
            parse_range_header(Some("bytes=1-2-3"), 16),
            RangeHeader::Unsatisfiable
        );
        assert_eq!(
            parse_range_header(Some("bytes=a-b"), 16),
            RangeHeader::Unsatisfiable
        );
        // suffix 超过 size → 全文件。
        assert_eq!(
            parse_range_header(Some("bytes=-999"), 16),
            RangeHeader::Satisfied(0, 15)
        );
    }

    #[test]
    fn content_disposition_ascii_and_unicode() {
        assert_eq!(
            content_disposition("note.txt"),
            "inline; filename=\"note.txt\"; filename*=UTF-8''note.txt"
        );
        // 非 ASCII 名：fallback 剥成 download.txt，原样进 filename*。
        assert_eq!(
            content_disposition("报告.txt"),
            "inline; filename=\"download.txt\"; filename*=UTF-8''%E6%8A%A5%E5%91%8A.txt"
        );
        // 无扩展名 + 非法字符。
        assert_eq!(
            content_disposition("a/b\\c"),
            "inline; filename=\"a_b_c\"; filename*=UTF-8''a%2Fb%5Cc"
        );
    }

    #[test]
    fn resource_id_predicates_match_incumbent() {
        assert!(is_stable_session_file_id("sf_abc-DEF_123"));
        assert!(!is_stable_session_file_id("sf_"));
        assert!(!is_stable_session_file_id("sf_-lead"));
        assert!(!is_stable_session_file_id("rs_abc"));
    }
}
