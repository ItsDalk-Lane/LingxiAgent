//! R07-T07 β：工作台 / Studio 挂载面。
//!
//! 现役平价对象：
//! - `core/mount-aware-file-service.ts`（672 行：根解析 / 列表 / 搜索 /
//!   mkdir / writeText / rename / move / movePaths / safeDelete / 上传写）；
//! - `server/routes/studio-workspaces.ts`（309 行：workspaces 列举、本机
//!   路径挂载创建、停用、挂载文件列表）；
//! - `server/routes/mobile-workbench.ts`（323 行：/api/workbench/* 与
//!   /api/mobile/workbench/* 同处理器——候选只挂 /lingxi/v1/workbench/*）；
//! - `server/http/file-content.ts`（内容服务：弱 ETag / 304 / Range /
//!   206 / 416 / 私有缓存头 / Content-Disposition）；
//! - `core/execution-lease-registry.ts` + `core/execution-lease-service.ts`
//!   （远端写租约台账：local owner → null，非属主 5min issue/consume/revoke）。
//!
//! 偏差登记（沿用台账口径）：
//! - 目录排序：现役 localeCompare("zh")；候选用码点序（CJK 相对序不同，
//!   目录优先与 ASCII 序不变）。
//! - 中间件层 scope 拒绝的审计事件（现役 route 层 recordSecurityAuditEvent
//!   denied）不随候选鉴权中间件外发；处理器层审计（创建/停用/写动作）完整。
//! - 写租约仅在非属主主体下产生台账记录（与现役 isLocalOwner → null 一致）。

use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Extension, Json, Router,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{
    agents_admin::agent_config_readback::scope_denied, resource_mounts, resourceio::OpContext,
    security_audit, Principal, ServiceState,
};

/// 现役 SEARCH_LIMIT（mount-aware-file-service.ts :6）。
const SEARCH_LIMIT: usize = 80;
/// 与 desk 路由共用的搜索上限（现役 WORKSPACE_SEARCH_LIMIT=80）。
pub(crate) const WORKSPACE_SEARCH_LIMIT: usize = SEARCH_LIMIT;
/// 现役 MAX_UPLOAD_BYTES：单文件解码后 25MB（mobile-workbench.ts :21）。
const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
/// 写租约 TTL（issueRemoteWriteLease 默认 5min）。
const WRITE_LEASE_TTL_MS: u64 = 5 * 60 * 1000;
const LEASE_FILE_RELATIVE: &str = "security/execution-leases.json";

// ── 错误面（workbenchError / workspaceError：{error, detail} + status）──

#[derive(Debug)]
pub(crate) struct WbError {
    pub(crate) status: u16,
    pub(crate) code: String,
    pub(crate) detail: String,
}

impl WbError {
    pub(crate) fn new(status: u16, code: &str, detail: impl Into<String>) -> Self {
        WbError {
            status,
            code: code.to_string(),
            detail: detail.into(),
        }
    }
    /// 现役 workbenchError：MountAwareFileError → {error: code, detail:
    /// message}；ResourceIOError 经 err.code/err.status 原样透传，detail
    /// 取 message——而现役 access-denied 的 message 即 safeMessage
    /// （errors.ts :27-35），故候选人取 safe_message 优先不扩大披露。
    fn from_io(err: &crate::resourceio::ResourceIoError) -> Self {
        WbError {
            status: err.status,
            code: err.code.clone(),
            detail: err
                .safe_message
                .clone()
                .unwrap_or_else(|| err.message.clone()),
        }
    }
    pub(crate) fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (
            status,
            Json(json!({ "error": self.code, "detail": self.detail })),
        )
            .into_response()
    }
}

impl From<crate::resourceio::ResourceIoError> for WbError {
    fn from(err: crate::resourceio::ResourceIoError) -> Self {
        WbError::from_io(&err)
    }
}

// ── 根解析（_resolveRootInternal / findLocalFsMount / publicRoot）──

#[derive(Debug, Clone)]
pub(crate) struct ResolvedRoot {
    pub(crate) id: String,
    pub(crate) mount_id: String,
    pub(crate) label: String,
    pub(crate) presentation: String,
    pub(crate) path: PathBuf,
    pub(crate) capabilities: Vec<String>,
    pub(crate) source_kind: String,
    pub(crate) provider: Option<String>,
}

impl ResolvedRoot {
    fn is_default(&self) -> bool {
        self.id == "default"
    }

    /// desk 路由的合成根（现役 fileServiceForRequest 以 dir 覆盖为 default
    /// 根构造 MountAwareFileService，desk.ts :574-587）：default 语义 +
    /// list/read/write 全能力 + local_fs。目录创建与现役
    /// _resolveRootInternal 的 mkdirSync recursive 对齐，由调用方显式做。
    pub(crate) fn default_with_path(path: PathBuf) -> Self {
        ResolvedRoot {
            id: "default".to_string(),
            mount_id: "default".to_string(),
            label: "Default".to_string(),
            presentation: "folder".to_string(),
            path,
            capabilities: vec!["list".into(), "read".into(), "write".into()],
            source_kind: "storage".to_string(),
            provider: Some("local_fs".to_string()),
        }
    }
}

fn io_capable_now(state: &ServiceState) -> u64 {
    state.clock().now_unix_ms()
}

/// 现役 _resolveRootInternal（:372-407）：default → workspace 根 + mkdir
/// 递归（无根 → 400 no_workspace）；挂载 id → 登记内 active + 本 studio +
/// storage/local_fs（缺失 → 404 unknown_root），rootLocator.path 必须绝对
/// （→ 400 invalid_mount_root），并 mkdir 递归。
fn resolve_root_internal(
    state: &ServiceState,
    studio: &str,
    root_id: &str,
) -> Result<ResolvedRoot, WbError> {
    let id = {
        let trimmed = root_id.trim();
        if trimmed.is_empty() {
            "default"
        } else {
            trimmed
        }
    };
    if id == "default" {
        let Some(path) = state.resource_io().workspace_root() else {
            return Err(WbError::new(400, "no_workspace", "no workspace"));
        };
        std::fs::create_dir_all(&path).map_err(|err| {
            WbError::new(
                400,
                "file_action_failed",
                format!("cannot create workspace root: {err}"),
            )
        })?;
        return Ok(ResolvedRoot {
            id: "default".to_string(),
            mount_id: "default".to_string(),
            label: "Default".to_string(),
            presentation: "folder".to_string(),
            path,
            capabilities: vec!["list".into(), "read".into(), "write".into()],
            source_kind: "storage".to_string(),
            provider: Some("local_fs".to_string()),
        });
    }
    // findLocalFsMount（:554-567）：登记读取失败 → None → 404 unknown_root。
    let home = state.config().data_home.clone();
    let mounts =
        resource_mounts::MountRegistry::new(&home).list_for_studio(studio, io_capable_now(state));
    let found = mounts.ok().and_then(|list| {
        list.into_iter().find(|mount| {
            mount["mountId"].as_str() == Some(id)
                && mount["status"].as_str() == Some("active")
                && mount["sourceKind"].as_str() == Some("storage")
                && mount["provider"].as_str() == Some("local_fs")
        })
    });
    let Some(mount) = found else {
        return Err(WbError::new(404, "unknown_root", "unknown root"));
    };
    let root_path = mount["rootLocator"]
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("");
    if root_path.is_empty() || !Path::new(root_path).is_absolute() {
        return Err(WbError::new(
            400,
            "invalid_mount_root",
            "invalid mount root",
        ));
    }
    let root_path = PathBuf::from(root_path);
    std::fs::create_dir_all(&root_path).map_err(|err| {
        WbError::new(
            400,
            "file_action_failed",
            format!("cannot create mount root: {err}"),
        )
    })?;
    Ok(ResolvedRoot {
        id: mount["mountId"].as_str().unwrap_or(id).to_string(),
        mount_id: mount["mountId"].as_str().unwrap_or(id).to_string(),
        label: mount["label"].as_str().unwrap_or_default().to_string(),
        presentation: mount["presentation"]
            .as_str()
            .unwrap_or("folder")
            .to_string(),
        path: root_path,
        capabilities: mount["capabilities"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        source_kind: mount["sourceKind"]
            .as_str()
            .unwrap_or("storage")
            .to_string(),
        provider: mount["provider"].as_str().map(str::to_string),
    })
}

/// 现役 publicRoot（:569-576）：剥 path；local owner + storage/local_fs →
/// 补 nativeRootPath。
fn public_root(root: &ResolvedRoot, disclose_native_root: bool) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(root.id));
    out.insert("mountId".into(), json!(root.mount_id));
    out.insert("workspaceId".into(), json!(root.id));
    out.insert("label".into(), json!(root.label));
    out.insert("presentation".into(), json!(root.presentation));
    out.insert("capabilities".into(), json!(root.capabilities));
    out.insert("sourceKind".into(), json!(root.source_kind));
    out.insert(
        "provider".into(),
        root.provider
            .clone()
            .map(Value::from)
            .unwrap_or(Value::Null),
    );
    if disclose_native_root
        && root.source_kind == "storage"
        && root.provider.as_deref() == Some("local_fs")
    {
        out.insert("nativeRootPath".into(), json!(root.path.to_string_lossy()));
    }
    Value::Object(out)
}

/// 现役 workspaceFromMount（:245-268）：作用于登记条目原值。
fn workspace_from_mount(mount: &Value, disclose_native_root: bool) -> Value {
    let native = disclose_native_root
        && mount["sourceKind"].as_str() == Some("storage")
        && mount["provider"].as_str() == Some("local_fs")
        && mount["rootLocator"]
            .get("path")
            .and_then(Value::as_str)
            .is_some_and(|p| !p.is_empty());
    let mut out = Map::new();
    let mount_id = mount["mountId"].clone();
    out.insert("workspaceId".into(), mount_id.clone());
    out.insert("mountId".into(), mount_id);
    out.insert("label".into(), mount["label"].clone());
    out.insert("sourceKind".into(), mount["sourceKind"].clone());
    out.insert(
        "provider".into(),
        mount.get("provider").cloned().unwrap_or(Value::Null),
    );
    out.insert("presentation".into(), mount["presentation"].clone());
    out.insert(
        "capabilities".into(),
        mount
            .get("capabilities")
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    out.insert("isDefault".into(), json!(false));
    if native {
        out.insert(
            "nativeRootPath".into(),
            mount["rootLocator"]["path"].clone(),
        );
    }
    if mount["sourceKind"].as_str() == Some("studio") {
        out.insert("sourceStudioId".into(), mount["sourceStudioId"].clone());
        out.insert("sourceResourceId".into(), mount["sourceResourceId"].clone());
    }
    Value::Object(out)
}

/// 现役 workspaceFromRoot（:228-243）：输入是 publicRoot 投影。
fn workspace_from_public_root(root: &Value, is_default: bool) -> Value {
    let mut out = Map::new();
    let fallback = root["mountId"].clone();
    let fallback = if fallback.is_null() {
        root["id"].clone()
    } else {
        fallback
    };
    out.insert(
        "workspaceId".into(),
        if root["workspaceId"].is_null() {
            fallback.clone()
        } else {
            root["workspaceId"].clone()
        },
    );
    out.insert("mountId".into(), fallback);
    out.insert("label".into(), root["label"].clone());
    out.insert("sourceKind".into(), root["sourceKind"].clone());
    out.insert(
        "provider".into(),
        root.get("provider").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "presentation".into(),
        if root["presentation"].is_null() {
            json!("folder")
        } else {
            root["presentation"].clone()
        },
    );
    out.insert(
        "capabilities".into(),
        root.get("capabilities")
            .cloned()
            .unwrap_or_else(|| json!([])),
    );
    out.insert("isDefault".into(), json!(is_default));
    if let Some(native) = root.get("nativeRootPath").filter(|v| v.is_string()) {
        out.insert("nativeRootPath".into(), native.clone());
    }
    Value::Object(out)
}

/// 现役 requireCapability（:578-582）。
pub(crate) fn require_capability(root: &ResolvedRoot, capability: &str) -> Result<(), WbError> {
    if !root.capabilities.iter().any(|c| c == capability) {
        return Err(WbError::new(
            403,
            "mount_capability_denied",
            "mount capability denied",
        ));
    }
    Ok(())
}

// ── 名字/路径规范（normalizePlainNameOrThrow / normalizeSubdirOrThrow /
// resolveInsideRoot / resolveFileTarget / resolveMissingPath）──

pub(crate) fn normalize_plain_name(value: Option<&Value>) -> Result<String, WbError> {
    let raw = value.and_then(Value::as_str).unwrap_or("");
    let name = raw.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(WbError::new(400, "invalid_name", "invalid name"));
    }
    Ok(name.to_string())
}

pub(crate) fn normalize_subdir(value: &str) -> Result<String, WbError> {
    let raw = value.trim_matches('/');
    if raw.is_empty() {
        return Ok(String::new());
    }
    if raw.contains('\\') || raw.split('/').any(|part| part == ".." || part == ".") {
        return Err(WbError::new(400, "invalid_subdir", "invalid_subdir"));
    }
    Ok(raw.to_string())
}

pub(crate) fn real_path(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok()
}

/// 现役 resolveMissingPath（:623-636）：缺失目标向上走查到既有前缀，
/// 中途越出 rootReal 即 None。
fn resolve_missing_path(root_real: &Path, target: &Path) -> Option<PathBuf> {
    let mut pending = Vec::new();
    let mut current = target.to_path_buf();
    loop {
        if let Some(existing) = real_path(&current) {
            let mut out = existing;
            for part in pending.iter().rev() {
                out = out.join(part);
            }
            return Some(out);
        }
        let parent = current.parent()?.to_path_buf();
        pending.push(current.file_name()?.to_string_lossy().into_owned());
        // path.relative(rootReal, current) 以 .. 开头或是绝对 → 越界。
        let parent_rel = pathdiff(&parent, root_real)?;
        if parent_rel.starts_with("..") {
            return None;
        }
        current = parent;
    }
}

/// path.relative 的最小等价（两边都须绝对；不同根（Windows 盘符）→ None）。
fn pathdiff(target: &Path, base: &Path) -> Option<PathBuf> {
    let target = target.components().collect::<Vec<_>>();
    let base = base.components().collect::<Vec<_>>();
    let mut common = 0usize;
    while common < target.len() && common < base.len() && target[common] == base[common] {
        common += 1;
    }
    if common == 0 && !base.is_empty() {
        // 现役 path.relative 对不同盘符返回绝对路径（调用方按绝对判越界）；
        // 候选人以 None 表达「不可相对」，调用方同样判越界。
        if !target.is_empty() && target[0] != base[0] {
            return None;
        }
    }
    let mut out = PathBuf::new();
    for _ in common..base.len() {
        out.push("..");
    }
    for component in &target[common..] {
        out.push(component.as_os_str());
    }
    Some(out)
}

/// 现役 resolveInsideRoot（:598-609）。
pub(crate) fn resolve_inside_root(root_path: &Path, subdir: &str) -> Option<PathBuf> {
    let root_real = real_path(root_path)?;
    let target = if subdir.is_empty() {
        root_path.to_path_buf()
    } else {
        root_path.join(subdir)
    };
    if let Some(target_real) = real_path(&target) {
        return if target_real == root_real || target_real.starts_with(root_real.join("")) {
            Some(target_real)
        } else {
            None
        };
    }
    let full = resolve_missing_path(&root_real, &target)?;
    if full == root_real || full.starts_with(root_real.join("")) {
        Some(full)
    } else {
        None
    }
}

/// 现役 resolveFileTarget（:611-621）。
pub(crate) fn resolve_file_target(root_path: &Path, dir: &Path, name: &str) -> Option<PathBuf> {
    let target = dir.join(name);
    let root_real = real_path(root_path)?;
    if let Some(resolved) = real_path(&target) {
        return if resolved == root_real || resolved.starts_with(root_real.join("")) {
            Some(resolved)
        } else {
            None
        };
    }
    let parent_real = real_path(target.parent()?)?;
    let full = parent_real.join(target.file_name()?);
    if full == root_real || full.starts_with(root_real.join("")) {
        Some(full)
    } else {
        None
    }
}

// ── ref 构造（resourceRefForRoot / resourceRefForTarget / joinResourcePath）──

pub(crate) fn join_resource_path(parts: &[&str]) -> String {
    parts
        .iter()
        .flat_map(|part| part.split('/'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn ref_for_root(root: &ResolvedRoot, subpath: &str) -> Value {
    if root.is_default() {
        let path = if subpath.is_empty() {
            root.path.clone()
        } else {
            subpath
                .split('/')
                .fold(root.path.clone(), |acc, seg| acc.join(seg))
        };
        json!({ "kind": "local-file", "path": path.to_string_lossy() })
    } else {
        json!({ "kind": "mount", "mountId": root.mount_id, "path": subpath })
    }
}

/// 现役 resourceRefForTarget（:501-514）：显式替换 \ → / 并剥首尾斜杠。
pub(crate) fn ref_for_target(root: &ResolvedRoot, subpath: &str) -> Value {
    let normalized = subpath.replace('\\', "/");
    let normalized = normalized.trim_matches('/');
    ref_for_root(root, normalized)
}

// ── 投影（listItemsForWorkbench / searchMatchesForWorkbench）──

fn iso_from_ms(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 现役 listItemsForWorkbench（:516-530）：{name,isDir,size,mtime}，
/// 目录优先 + localeCompare("zh")（候选码点序，台账登记）。
fn list_items_for_workbench(items: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = items
        .iter()
        .filter(|item| item["name"].is_string())
        .map(|item| {
            let is_dir = item["isDirectory"].as_bool().unwrap_or(false);
            json!({
                "name": item["name"],
                "isDir": is_dir,
                "size": if is_dir { Value::Null } else { item.get("size").cloned().unwrap_or(Value::Null) },
                "mtime": iso_from_ms(item["mtimeMs"].as_u64().unwrap_or(0)),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        let dir_a = a["isDir"].as_bool().unwrap_or(false);
        let dir_b = b["isDir"].as_bool().unwrap_or(false);
        if dir_a != dir_b {
            return if dir_a {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    out
}

/// 现役 searchMatchesForWorkbench（:532-544）：候选 search_names 已带
/// relativePath/parentSubdir（挂载内相对），此处投影 + 80 截断。
fn search_matches_for_workbench(matches: &[Value]) -> Vec<Value> {
    matches
        .iter()
        .filter(|m| m["name"].is_string() || m["filePath"].is_string())
        .map(|m| {
            let is_dir = m["isDirectory"].as_bool().unwrap_or(false);
            json!({
                "name": m["name"],
                "relativePath": m["relativePath"],
                "parentSubdir": m["parentSubdir"],
                "isDir": is_dir,
                "size": if is_dir { Value::Null } else { m.get("size").cloned().unwrap_or(Value::Null) },
                "mtime": iso_from_ms(m["mtimeMs"].as_u64().unwrap_or(0)),
            })
        })
        .take(SEARCH_LIMIT)
        .collect()
}

// ── 操作上下文（mutationOptions：source/reason/sessionId/sessionPath/emit）──

pub(crate) fn op_ctx(body: &Value, reason: &str) -> OpContext {
    let mut ctx =
        OpContext::local_owner(body.get("sessionId").and_then(Value::as_str).unwrap_or(""));
    ctx.reason = Some(reason.to_string());
    ctx.session_path = body
        .get("sessionPath")
        .and_then(Value::as_str)
        .map(str::to_string);
    ctx
}

pub(crate) fn op_ctx_no_emit(body: &Value, reason: &str) -> OpContext {
    let mut ctx = op_ctx(body, reason);
    ctx.emit = false;
    ctx
}

// ── 写租约台账（execution-lease-registry.ts + execution-lease-service.ts）──

struct WriteLease {
    lease_id: String,
}

/// 现役 issueRemoteWriteLease：local owner / 无 principalId → None。
fn issue_remote_write_lease(
    state: &ServiceState,
    principal: &Principal,
    studio: &str,
    mount_id: &str,
    now_ms: u64,
) -> Result<Option<WriteLease>, WbError> {
    if principal.is_local_owner() {
        return Ok(None);
    }
    if principal.principal_id.trim().is_empty() {
        return Err(WbError::new(
            403,
            "write_lease_identity_required",
            "远端写入缺少可核验的身份，未执行文件操作",
        ));
    }
    let home = state.config().data_home.clone();
    let now = iso_from_ms(now_ms);
    let expires = iso_from_ms(now_ms + WRITE_LEASE_TTL_MS);
    let lease_id = format!("lease_{}", uuid_v4());
    let (server_id, _) = state.management.identity_ids();
    let record = json!({
        "schemaVersion": 1,
        "leaseId": lease_id,
        "studioId": principal.studio_id.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| studio.to_string()),
        "targetServerNodeId": principal.server_node_id.clone().filter(|s| !s.is_empty()).unwrap_or(server_id),
        "agentId": "mobile_workbench",
        "sessionId": "mobile_workbench",
        "actorPrincipalId": principal.principal_id,
        "capabilityDecisionId": Value::Null,
        "commandClass": "write_files",
        "sandboxProfile": "workspace_write",
        "backupPolicy": "snapshot_before_write",
        "resourceIds": [mount_id],
        "mountId": if mount_id == "default" { Value::Null } else { json!(mount_id) },
        "status": "issued",
        "createdAt": now,
        "consumedAt": Value::Null,
        "revokedAt": Value::Null,
        "expiresAt": expires,
    });
    ledger_update(&home, now_ms, |registry| {
        let leases = registry["leases"]
            .as_array_mut()
            .ok_or_else(|| lease_error("写入记录格式损坏"))?;
        if leases
            .iter()
            .any(|item| item["leaseId"] == record["leaseId"])
        {
            return Err(lease_error("写入记录编号已存在"));
        }
        leases.push(record.clone());
        Ok(())
    })?;
    Ok(Some(WriteLease { lease_id }))
}

/// 文件操作完成后必须确认记录状态；失败不得冒称成功或声称已回滚文件。
fn settle_remote_write_lease(
    state: &ServiceState,
    lease: &WriteLease,
    consumed: bool,
) -> Result<(), WbError> {
    let home = state.config().data_home.clone();
    let now_ms = state.clock().now_unix_ms();
    let now = iso_from_ms(now_ms);
    let lease_id = lease.lease_id.clone();
    ledger_update(&home, now_ms, move |registry| {
        let leases = registry["leases"]
            .as_array_mut()
            .ok_or_else(|| lease_error("写入记录格式损坏"))?;
        let item = leases
            .iter_mut()
            .find(|item| item["leaseId"].as_str() == Some(lease_id.as_str()))
            .ok_or_else(|| lease_error("待确认的写入记录已丢失"))?;
        if item["status"].as_str() != Some("issued") {
            return Err(lease_error("写入记录已改变，无法确认本次结果"));
        }
        if consumed {
            item["status"] = json!("consumed");
            item["consumedAt"] = json!(now);
        } else {
            item["status"] = json!("revoked");
            item["revokedAt"] = json!(now);
        }
        Ok(())
    })
}

fn lease_error(detail: impl Into<String>) -> WbError {
    WbError::new(503, "write_lease_unavailable", detail)
}

/// execution-leases.json 保留未知字段；读、校验和原子写入作为一次不可交错的变更。
fn ledger_update(
    home: &Path,
    now_ms: u64,
    mutate: impl FnOnce(&mut Value) -> Result<(), WbError>,
) -> Result<(), WbError> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().map_err(|_| lease_error("写入记录当前不可用"))?;
    let path = home.join(LEASE_FILE_RELATIVE);
    let mut registry: Value = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| lease_error(format!("写入记录损坏，原文件已保留：{error}")))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let now = iso_from_ms(now_ms);
            json!({"schemaVersion":1,"leases":[],"createdAt":now,"updatedAt":now})
        }
        Err(error) => {
            return Err(lease_error(format!(
                "无法读取写入记录，未授权新的文件操作：{error}"
            )))
        }
    };
    if registry["schemaVersion"] != json!(1) || !registry["leases"].is_array() {
        return Err(lease_error("写入记录格式损坏，原文件已保留"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for lease in registry["leases"].as_array().expect("已核验列表") {
        let id = lease["leaseId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| lease_error("写入记录包含损坏条目，原文件已保留"))?;
        if !seen.insert(id.to_string())
            || !matches!(
                lease["status"].as_str(),
                Some("issued" | "consumed" | "revoked" | "expired")
            )
        {
            return Err(lease_error("写入记录编号重复或状态损坏，原文件已保留"));
        }
        for field in ["actorPrincipalId", "agentId", "sessionId", "expiresAt"] {
            if !lease[field]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty())
            {
                return Err(lease_error("写入记录缺少身份或有效期，原文件已保留"));
            }
        }
    }
    mutate(&mut registry)?;
    registry["updatedAt"] = json!(iso_from_ms(now_ms));
    let mut text =
        serde_json::to_string_pretty(&registry).map_err(|error| lease_error(error.to_string()))?;
    text.push('\n');
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| lease_error(format!("无法创建写入记录目录：{error}")))?;
    }
    crate::paths::atomic_write(&path, text.as_bytes())
        .map_err(|error| lease_error(format!("写入记录未保存：{error}")))
}

/// 结算出错时附上实际操作结果；客户端不能据此误认“未执行”并自动重投。
fn lease_settlement_failure(
    lease: &WriteLease,
    error: WbError,
    execution_state: &str,
    result: Value,
) -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, Json(json!({
        "ok":false,"error":"write_lease_settlement_failed",
        "detail":format!("文件操作结果如下，但写入记录待确认；请先检查结果，不要自动重试。{}",error.detail),
        "leaseId":lease.lease_id,"leaseState":"unknown","executionState":execution_state,"canRetry":false,"result":result
    }))).into_response()
}

fn uuid_v4() -> String {
    use sha2::Digest as _;
    let mut seed = Vec::new();
    seed.extend_from_slice(std::process::id().to_string().as_bytes());
    seed.extend_from_slice(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_default()
            .as_bytes(),
    );
    let mut rand = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read as _;
        let _ = f.read_exact(&mut rand);
    }
    seed.extend_from_slice(&rand);
    let digest = sha2::Sha256::digest(&seed);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

// ── 安全审计（recordSecurityAuditEvent 的处理器层部分）──

fn audit(
    state: &ServiceState,
    principal: &Principal,
    studio: &str,
    action: &str,
    result: &str,
    mount_id: Option<&str>,
    lease_id: Option<&str>,
) {
    // security_audit::project 的 target 必须是字符串（现役投影契约）；现役
    // TS 审计的 target 对象 {kind,studioId,mountId?} 信息完整保留在 metadata。
    let target = match mount_id {
        Some(mount_id) => format!("studio:{studio}:mount:{mount_id}"),
        None => format!("studio:{studio}"),
    };
    let mut metadata = json!({ "studioId": studio });
    if let Some(mount_id) = mount_id {
        metadata["mountId"] = json!(mount_id);
    }
    let mut event = json!({
        "action": action,
        "target": target,
        "result": result,
        "actor": serde_json::to_value(principal).unwrap_or(Value::Null),
        "atUnixMs": state.clock().now_unix_ms(),
        "metadata": metadata,
    });
    if let Some(lease_id) = lease_id {
        event["leaseId"] = json!(lease_id);
    }
    if let Err(err) = security_audit::project(&state.config().data_home, "workbench", &[event]) {
        tracing::warn!("workbench audit projection failed: {err}");
    }
}

// ── 服务层（MountAwareFileService 方法面）──

pub(crate) async fn list_files(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    disclose: bool,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    require_capability(root, "list")?;
    let normalized = normalize_subdir(subdir)?;
    if resolve_inside_root(&root.path, &normalized).is_none() {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    }
    let listed = state
        .resource_io()
        .list(&ref_for_root(root, &normalized), ctx)
        .await?;
    let items = listed["items"].as_array().cloned().unwrap_or_default();
    Ok(json!({
        "rootId": root.id,
        "mountId": root.mount_id,
        "mount": public_root(root, disclose),
        "subdir": normalized,
        "files": list_items_for_workbench(&items),
    }))
}

pub(crate) async fn search_files(
    state: &ServiceState,
    root: &ResolvedRoot,
    query: &str,
    disclose: bool,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    require_capability(root, "list")?;
    let q = query.trim();
    let matches = if q.is_empty() {
        Vec::new()
    } else {
        let result = state
            .resource_io()
            .search(
                &ref_for_root(root, ""),
                Some(q.to_string()),
                Some("name"),
                Some(SEARCH_LIMIT),
                ctx,
            )
            .await?;
        result["matches"].as_array().cloned().unwrap_or_default()
    };
    Ok(json!({
        "rootId": root.id,
        "mountId": root.mount_id,
        "mount": public_root(root, disclose),
        "query": q,
        "results": search_matches_for_workbench(&matches),
    }))
}

/// 现役 contentTarget（:104-114）：read 能力 + 目录/文件名解析 + 收容。
pub(crate) fn content_target(
    root: &ResolvedRoot,
    subdir: &str,
    name: Option<&Value>,
) -> Result<(PathBuf, String), WbError> {
    require_capability(root, "read")?;
    let normalized = normalize_subdir(subdir)?;
    let filename = normalize_plain_name(name)?;
    let Some(dir) = resolve_inside_root(&root.path, &normalized) else {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    };
    let Some(file_path) = resolve_file_target(&root.path, &dir, &filename) else {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    };
    Ok((file_path, filename))
}

/// 现役 _writeDir（:359-366）：write 能力 + subdir 规范 + 收容。
pub(crate) fn write_dir(root: &ResolvedRoot, subdir: &str) -> Result<(PathBuf, String), WbError> {
    require_capability(root, "write")?;
    let normalized = normalize_subdir(subdir)?;
    let Some(dir) = resolve_inside_root(&root.path, &normalized) else {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    };
    Ok((dir, normalized))
}

pub(crate) async fn files_for_directory(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    disclose: bool,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    let listed = list_files(state, root, subdir, disclose, ctx).await?;
    Ok(listed["files"].clone())
}

/// 现役 workbenchWriteResult（:410-436）。
pub(crate) fn workbench_write_result(
    root: &ResolvedRoot,
    action: &str,
    disclose: bool,
    extra: Map<String, Value>,
) -> Value {
    let mut out = Map::new();
    out.insert("ok".into(), json!(true));
    out.insert("action".into(), json!(action));
    out.insert("rootId".into(), json!(root.id));
    out.insert("mountId".into(), json!(root.mount_id));
    out.insert("mount".into(), public_root(root, disclose));
    out.extend(extra);
    Value::Object(out)
}

pub(crate) fn target_subpath(subdir: &str, name: &str) -> Result<String, WbError> {
    let normalized_dir = normalize_subdir(subdir)?;
    let plain = normalize_plain_name(Some(&Value::String(name.to_string())))?;
    Ok(join_resource_path(&[&normalized_dir, &plain]))
}

/// 现役 normalizeExpectedVersionOrNull（:584-596）：null → None；mtimeMs
/// 与 size 必须 Number() 可解析且有限，否则 400 invalid_expected_version；
/// sha256 仅字符串透传。
fn normalize_expected_version(value: Option<&Value>) -> Result<Option<Value>, WbError> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let number = |key: &str| -> Option<f64> {
        value
            .get(key)
            .and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
            })
            .filter(|n| n.is_finite())
    };
    match (number("mtimeMs"), number("size")) {
        (Some(mtime_ms), Some(size)) => {
            let mut out = json!({ "mtimeMs": mtime_ms, "size": size });
            if let Some(sha) = value.get("sha256").and_then(Value::as_str) {
                out["sha256"] = json!(sha);
            }
            Ok(Some(out))
        }
        _ => Err(WbError::new(
            400,
            "invalid_expected_version",
            "invalid expected version",
        )),
    }
}

pub(crate) async fn action_mkdir(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    body: &Value,
    disclose: bool,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    let (dir, _normalized) = write_dir(root, subdir)?;
    let name = normalize_plain_name(body.get("name"))?;
    if resolve_file_target(&root.path, &dir, &name).is_none() {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    }
    let target_ref = ref_for_target(root, &target_subpath(subdir, &name)?);
    let stat = state.resource_io().stat(&target_ref, ctx).await?;
    if stat["exists"].as_bool().unwrap_or(false) {
        return Err(WbError::new(409, "already_exists", "already exists"));
    }
    state.resource_io().mkdir(&target_ref, ctx).await?;
    let files = files_for_directory(state, root, subdir, disclose, ctx).await?;
    let mut extra = Map::new();
    extra.insert("files".into(), files);
    Ok(workbench_write_result(root, "mkdir", disclose, extra))
}

/// 现役 writeText（:143-181）：stat → mustNotExist 闸（仅 desk 侧
/// create 传 true；mobile-workbench 的 create 与 writeText 同语义不查）
/// → 已存在先存改前存档 → expectedVersion 或普通写 → 冲突是结果
/// （ok:false + conflict:true + version + files）。
pub(crate) async fn action_write_text(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    body: &Value,
    disclose: bool,
    ctx: &OpContext,
    must_not_exist: bool,
) -> Result<Value, WbError> {
    let (dir, _normalized) = write_dir(root, subdir)?;
    let name = normalize_plain_name(body.get("name"))?;
    let Some(target_path) = resolve_file_target(&root.path, &dir, &name) else {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    };
    let action_name = body.get("action").and_then(Value::as_str).unwrap_or("");
    let target_ref = ref_for_target(root, &target_subpath(subdir, &name)?);
    let stat = state.resource_io().stat(&target_ref, ctx).await?;
    // 现役（:153-155）mustNotExist → 409 target_already_exists，先于存档。
    if must_not_exist && stat["exists"].as_bool().unwrap_or(false) {
        return Err(WbError::new(
            409,
            "target_already_exists",
            "target already exists",
        ));
    }
    // 现役（:156-158）：目标已存在 → 先存改前存档（mobile-workbench-edit；
    // 存档失败 .catch(()=>null) 不阻塞主写入）。
    if stat["exists"].as_bool().unwrap_or(false) {
        let _ = crate::checkpoint_store::CheckpointStore::new(&state.config().data_home)
            .save_user_edit(&target_path, "mobile-workbench-edit", io_capable_now(state));
    }
    // 现役 String(body.content ?? "")：字符串原样，number/bool 转字符串，
    // null/undefined → ""。
    let content = match body.get("content") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(text)) => text.clone().into_bytes(),
        Some(Value::Number(number)) => number.to_string().into_bytes(),
        Some(Value::Bool(flag)) => flag.to_string().into_bytes(),
        Some(_) => b"[object Object]".to_vec(),
    };
    let expected = match normalize_expected_version(body.get("expectedVersion"))? {
        Some(expected) => expected,
        None => Value::Null,
    };
    let write_result = if expected.is_object() {
        state
            .resource_io()
            .write_expected_version(&target_ref, content, &expected, ctx)
            .await?
    } else {
        state.resource_io().write(&target_ref, content, ctx).await?
    };
    let files = files_for_directory(state, root, subdir, disclose, ctx).await?;
    if write_result.get("conflict") == Some(&Value::Bool(true)) {
        return Ok(json!({
            "ok": false,
            "action": action_name,
            "rootId": root.id,
            "mountId": root.mount_id,
            "mount": public_root(root, disclose),
            "conflict": true,
            "version": write_result.get("version").cloned().unwrap_or(Value::Null),
            "files": files,
        }));
    }
    let mut extra = Map::new();
    extra.insert(
        "version".into(),
        write_result.get("version").cloned().unwrap_or(Value::Null),
    );
    extra.insert("files".into(), files);
    Ok(workbench_write_result(root, action_name, disclose, extra))
}

pub(crate) async fn action_rename(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    body: &Value,
    disclose: bool,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    let (dir, _normalized) = write_dir(root, subdir)?;
    let old_name = normalize_plain_name(body.get("oldName"))?;
    let new_name = normalize_plain_name(body.get("newName"))?;
    if resolve_file_target(&root.path, &dir, &old_name).is_none()
        || resolve_file_target(&root.path, &dir, &new_name).is_none()
    {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    }
    state
        .resource_io()
        .rename(
            &ref_for_target(root, &target_subpath(subdir, &old_name)?),
            &ref_for_target(root, &target_subpath(subdir, &new_name)?),
            ctx,
        )
        .await?;
    let files = files_for_directory(state, root, subdir, disclose, ctx).await?;
    let mut extra = Map::new();
    extra.insert("files".into(), files);
    Ok(workbench_write_result(root, "rename", disclose, extra))
}

pub(crate) async fn action_move(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    body: &Value,
    disclose: bool,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    let (dir, _normalized) = write_dir(root, subdir)?;
    let name = normalize_plain_name(body.get("name"))?;
    let dest_subdir =
        normalize_subdir(body.get("destSubdir").and_then(Value::as_str).unwrap_or(""))?;
    let Some(dest_dir) = resolve_inside_root(&root.path, &dest_subdir) else {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    };
    let _ = dest_dir;
    if resolve_file_target(&root.path, &dir, &name).is_none() {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    }
    // 现役先 mkdir 目标目录（emit:false），再 move。
    let quiet = op_ctx_no_emit(
        body,
        ctx.reason.as_deref().unwrap_or("mobile_workbench.move"),
    );
    state
        .resource_io()
        .mkdir(&ref_for_root(root, &dest_subdir), &quiet)
        .await?;
    state
        .resource_io()
        .move_resource(
            &ref_for_target(root, &target_subpath(subdir, &name)?),
            &ref_for_target(root, &target_subpath(&dest_subdir, &name)?),
            ctx,
        )
        .await?;
    let files = files_for_directory(state, root, subdir, disclose, ctx).await?;
    let mut extra = Map::new();
    extra.insert("files".into(), files);
    Ok(workbench_write_result(root, "move", disclose, extra))
}

pub(crate) async fn action_move_paths(
    state: &ServiceState,
    root: &ResolvedRoot,
    body: &Value,
    disclose: bool,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    require_capability(root, "write")?;
    let items = body["items"].as_array().cloned().unwrap_or_default();
    let dest_subdir =
        normalize_subdir(body.get("destSubdir").and_then(Value::as_str).unwrap_or(""))?;
    let current_subdir = normalize_subdir(
        body.get("currentSubdir")
            .and_then(Value::as_str)
            .unwrap_or(""),
    )?;
    if resolve_inside_root(&root.path, &dest_subdir).is_none() {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    }
    if body.get("createDestIfMissing") == Some(&Value::Bool(false)) {
        let dest_stat = state
            .resource_io()
            .stat(&ref_for_root(root, &dest_subdir), ctx)
            .await?;
        if !dest_stat["exists"].as_bool().unwrap_or(false)
            || !dest_stat["isDirectory"].as_bool().unwrap_or(false)
        {
            return Err(WbError::new(
                400,
                "dest_not_directory",
                "destSubdir is not a directory",
            ));
        }
    } else {
        let quiet = op_ctx_no_emit(
            body,
            ctx.reason
                .as_deref()
                .unwrap_or("mobile_workbench.move_paths"),
        );
        state
            .resource_io()
            .mkdir(&ref_for_root(root, &dest_subdir), &quiet)
            .await?;
    }
    let mut touched: Vec<String> = vec![current_subdir.clone(), dest_subdir.clone()];
    let mut results: Vec<Value> = Vec::new();
    for item in &items {
        let source_subdir = match normalize_subdir(item["sourceSubdir"].as_str().unwrap_or("")) {
            Ok(value) => value,
            Err(err) => return Err(err),
        };
        let name = normalize_plain_name(item.get("name"))?;
        let Some(source_dir) = resolve_inside_root(&root.path, &source_subdir) else {
            return Err(WbError::new(400, "invalid_path", "invalid path"));
        };
        let Some(dest_dir) = resolve_inside_root(&root.path, &dest_subdir) else {
            return Err(WbError::new(400, "invalid_path", "invalid path"));
        };
        if resolve_file_target(&root.path, &source_dir, &name).is_none()
            || resolve_file_target(&root.path, &dest_dir, &name).is_none()
        {
            return Err(WbError::new(400, "invalid_path", "invalid path"));
        }
        let source_ref = ref_for_target(root, &target_subpath(&source_subdir, &name)?);
        let target_ref = ref_for_target(root, &target_subpath(&dest_subdir, &name)?);
        let source_stat = state.resource_io().stat(&source_ref, ctx).await?;
        let target_stat = state.resource_io().stat(&target_ref, ctx).await?;
        if !source_stat["exists"].as_bool().unwrap_or(false) {
            results.push(json!({ "name": name, "error": "not found" }));
            continue;
        }
        if source_stat["resourceKey"] == target_stat["resourceKey"] {
            results.push(json!({ "name": name, "ok": true, "skipped": true }));
            continue;
        }
        if target_stat["exists"].as_bool().unwrap_or(false) {
            results.push(json!({ "name": name, "error": "target already exists" }));
            continue;
        }
        let source_rel = if source_subdir.is_empty() {
            name.clone()
        } else {
            format!("{source_subdir}/{name}")
        };
        if source_stat["isDirectory"].as_bool().unwrap_or(false)
            && (dest_subdir == source_rel || dest_subdir.starts_with(&format!("{source_rel}/")))
        {
            results.push(json!({ "name": name, "error": "cannot move folder into itself" }));
            continue;
        }
        match state
            .resource_io()
            .move_resource(&source_ref, &target_ref, ctx)
            .await
        {
            Ok(_) => {
                if !touched.contains(&source_subdir) {
                    touched.push(source_subdir.clone());
                }
                if !touched.contains(&dest_subdir) {
                    touched.push(dest_subdir.clone());
                }
                results.push(json!({ "name": name, "ok": true }));
            }
            Err(err) => {
                let mapped = WbError::from_io(&err);
                results.push(json!({ "name": name, "error": mapped.code }));
            }
        }
    }
    let mut files_by_path = Map::new();
    for subdir in &touched {
        if resolve_inside_root(&root.path, subdir).is_some() {
            let files = files_for_directory(state, root, subdir, disclose, ctx).await?;
            files_by_path.insert(subdir.clone(), files);
        }
    }
    let files = if resolve_inside_root(&root.path, &current_subdir).is_some() {
        files_for_directory(state, root, &current_subdir, disclose, ctx).await?
    } else {
        json!([])
    };
    let mut extra = Map::new();
    extra.insert("results".into(), json!(results));
    extra.insert("filesByPath".into(), Value::Object(files_by_path));
    extra.insert("files".into(), files);
    Ok(workbench_write_result(root, "movePaths", disclose, extra))
}

pub(crate) async fn action_safe_delete(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    body: &Value,
    disclose: bool,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    let (dir, normalized) = write_dir(root, subdir)?;
    let name = normalize_plain_name(body.get("name"))?;
    if resolve_file_target(&root.path, &dir, &name).is_none() {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    }
    let target_ref = ref_for_target(root, &target_subpath(subdir, &name)?);
    let result = state
        .resource_io()
        .trash(
            &target_ref,
            Some("mobile-workbench"),
            Some(json!({
                "rootId": root.id,
                "mountId": root.mount_id,
                "originalName": name,
                "originalSubdir": normalized,
            })),
            ctx,
        )
        .await?;
    let files = files_for_directory(state, root, subdir, disclose, ctx).await?;
    let mut extra = Map::new();
    extra.insert(
        "trashId".into(),
        result.get("trashId").cloned().unwrap_or(Value::Null),
    );
    extra.insert("files".into(), files);
    Ok(workbench_write_result(root, "safeDelete", disclose, extra))
}

/// 现役 readText（mount-aware-file-service.ts :116-131）：stat → 缺失
/// {exists:false, content:null, version:null}；存在则 read → utf-8 有损
/// 解码 + version 取 read 结果。
pub(crate) async fn read_text(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    name: &str,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    let (_file_path, filename) =
        content_target(root, subdir, Some(&Value::String(name.to_string())))?;
    let target_ref = ref_for_target(root, &target_subpath(subdir, &filename)?);
    let stat = state.resource_io().stat(&target_ref, ctx).await?;
    if !stat["exists"].as_bool().unwrap_or(false) {
        return Ok(json!({
            "exists": false,
            "content": Value::Null,
            "version": Value::Null,
        }));
    }
    let read = state.resource_io().read(&target_ref, ctx).await?;
    Ok(json!({
        "exists": true,
        "content": String::from_utf8_lossy(&read.content),
        "version": read.meta.get("version").cloned().unwrap_or(Value::Null),
    }))
}

/// 现役 safeDeleteIfExists（mount-aware-file-service.ts :307-319）：
/// 缺失 → workbenchWriteResult(safeDelete, {trashId:null, files})；
/// 存在 → 走 safeDelete。
pub(crate) async fn safe_delete_if_exists(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    name: &str,
    disclose: bool,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    let (_file_path, filename) =
        content_target(root, subdir, Some(&Value::String(name.to_string())))?;
    let target_ref = ref_for_target(root, &target_subpath(subdir, &filename)?);
    let stat = state.resource_io().stat(&target_ref, ctx).await?;
    if !stat["exists"].as_bool().unwrap_or(false) {
        let files = files_for_directory(state, root, subdir, disclose, ctx).await?;
        let mut extra = Map::new();
        extra.insert("trashId".into(), Value::Null);
        extra.insert("files".into(), files);
        return Ok(workbench_write_result(root, "safeDelete", disclose, extra));
    }
    action_safe_delete(
        state,
        root,
        subdir,
        &json!({ "name": filename }),
        disclose,
        ctx,
    )
    .await
}

/// 现役 copyLocalPathIntoDirectory（mount-aware-file-service.ts :339-350）：
/// 源必须绝对（400 invalid_path）→ writeFileTarget 的 write 能力/名字/收
/// 容闸（basename 取文件名）→ resourceIO.copy → 返回（文件名，copy 结果）。
pub(crate) async fn copy_local_path_into_directory(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    source_path: &str,
    ctx: &OpContext,
) -> Result<(String, Value), WbError> {
    if !Path::new(source_path).is_absolute() {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    }
    let (dir, _normalized) = write_dir(root, subdir)?;
    let raw_name = Path::new(source_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let filename = normalize_plain_name(Some(&Value::String(raw_name)))?;
    if resolve_file_target(&root.path, &dir, &filename).is_none() {
        return Err(WbError::new(400, "invalid_path", "invalid path"));
    }
    let target_ref = ref_for_target(root, &target_subpath(subdir, &filename)?);
    let source_ref = json!({ "kind": "local-file", "path": source_path });
    let result = state
        .resource_io()
        .copy(&source_ref, &target_ref, ctx)
        .await?;
    Ok((filename, result))
}

// ── 路由 ──

fn scope_guard(principal: &Principal, scope: &str) -> Option<Response> {
    if scope_denied(Some(principal), scope) {
        return Some(
            (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "insufficient_scope", "scope": scope })),
            )
                .into_response(),
        );
    }
    None
}

fn mount_query_id(query: &BTreeMap<String, String>) -> String {
    query
        .get("mountId")
        .or_else(|| query.get("rootId"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

fn mount_body_id(body: &Value) -> String {
    body.get("mountId")
        .or_else(|| body.get("rootId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("default")
        .to_string()
}

async fn studio_workspaces_list(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
) -> Response {
    if let Some(response) = scope_guard(&principal, "files.read") {
        return response;
    }
    let studio = state.management.identity_ids().1;
    let disclose = principal.is_local_owner();
    let mut workspaces = Vec::new();
    // 默认工作区：no_workspace 时省略（listStudioWorkspaces :113-117）。
    match resolve_root_internal(&state, &studio, "default") {
        Ok(root) => workspaces.push(workspace_from_public_root(
            &public_root(&root, disclose),
            true,
        )),
        Err(err) if err.code == "no_workspace" => {}
        Err(err) => return err.into_response(),
    }
    let home = state.config().data_home.clone();
    match resource_mounts::MountRegistry::new(&home)
        .list_for_studio(&studio, io_capable_now(&state))
    {
        Ok(mounts) => {
            for mount in mounts {
                if mount["status"].as_str() != Some("active") {
                    continue;
                }
                workspaces.push(workspace_from_mount(&mount, disclose));
            }
        }
        Err(err) => return WbError::from_io(&err).into_response(),
    }
    Json(json!({ "studioId": studio, "workspaces": workspaces })).into_response()
}

async fn studio_workspaces_create(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(response) = scope_guard(&principal, "files.write") {
        return response;
    }
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "local_owner_required",
                "capability": "studio.workspace.create_local_path",
            })),
        )
            .into_response();
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return crate::EndpointError::from_json_rejection(&rejection).into_response()
        }
    };
    let studio = state.management.identity_ids().1;
    match create_local_path_workspace(&state, &principal, &studio, &body) {
        Ok(workspace) => Json(json!({ "ok": true, "workspace": workspace })).into_response(),
        Err(err) => err.into_response(),
    }
}

/// createLocalPathWorkspace（studio-workspaces.ts :128-172）。
fn create_local_path_workspace(
    state: &ServiceState,
    principal: &Principal,
    studio: &str,
    body: &Value,
) -> Result<Value, WbError> {
    let raw = body
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if raw.is_empty() {
        return Err(WbError::new(
            400,
            "invalid_path",
            "path must be a non-empty string",
        ));
    }
    if !Path::new(raw).is_absolute() {
        return Err(WbError::new(
            400,
            "invalid_path",
            "path must be absolute on the server",
        ));
    }
    // path.resolve：绝对路径自身即结果（现役无 .. 归一拒绝，resolve 后落盘）。
    let root_path = std::fs::canonicalize(raw).unwrap_or_else(|_| PathBuf::from(raw));
    let is_dir = std::fs::metadata(&root_path)
        .map(|meta| meta.is_dir())
        .unwrap_or(false);
    if !is_dir {
        return Err(WbError::new(
            400,
            "invalid_path",
            "path must be an existing directory",
        ));
    }
    let home = state.config().data_home.clone();
    let now_ms = io_capable_now(state);
    // win32 大小写折叠去重（isSameLocalFsRoot）；非 win32 不做。
    #[cfg(windows)]
    {
        let listed = resource_mounts::MountRegistry::new(&home)
            .list_for_studio(studio, now_ms)
            .map_err(|err| WbError::from_io(&err))?;
        let equivalent = listed.iter().find(|mount| {
            mount["status"].as_str() == Some("active")
                && mount["sourceKind"].as_str() == Some("storage")
                && mount["provider"].as_str() == Some("local_fs")
                && mount["rootLocator"]["path"]
                    .as_str()
                    .is_some_and(|p| p != root_path.to_string_lossy())
                && mount["rootLocator"]["path"]
                    .as_str()
                    .map(|p| p.to_lowercase())
                    == Some(root_path.to_string_lossy().to_lowercase())
        });
        if let Some(mount) = equivalent {
            return Ok(workspace_from_mount(mount, principal.is_local_owner()));
        }
    }
    let label = body
        .get("label")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            root_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Workspace".to_string())
        });
    let root_string = root_path.to_string_lossy().into_owned();
    let mount = json!({
        "mountId": resource_mounts::local_fs_mount_id(studio, &root_string),
        "hostStudioId": studio,
        "sourceKind": "storage",
        "provider": "local_fs",
        "rootLocator": { "path": root_string },
        "label": label,
        "presentation": "folder",
        "capabilities": ["list", "read", "write", "watch"],
        "status": "active",
    });
    let stored = resource_mounts::MountRegistry::new(&home)
        .upsert(mount, now_ms)
        .map_err(|err| WbError::from_io(&err))?;
    audit(
        state,
        principal,
        studio,
        "studio_workspace.create_local_path",
        "success",
        stored["mountId"].as_str(),
        None,
    );
    Ok(workspace_from_mount(&stored, principal.is_local_owner()))
}

async fn studio_workspaces_delete(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    AxumPath(mount_id): AxumPath<String>,
) -> Response {
    if let Some(response) = scope_guard(&principal, "files.write") {
        return response;
    }
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "local_owner_required",
                "capability": "studio.workspace.remove_local_path",
            })),
        )
            .into_response();
    }
    let studio = state.management.identity_ids().1;
    if mount_id == "default" {
        return WbError::new(
            400,
            "default_workspace",
            "default workspace cannot be removed",
        )
        .into_response();
    }
    let home = state.config().data_home.clone();
    let now_ms = io_capable_now(&state);
    let listed = match resource_mounts::MountRegistry::new(&home).list_for_studio(&studio, now_ms) {
        Ok(listed) => listed,
        Err(err) => return WbError::from_io(&err).into_response(),
    };
    let Some(active) = listed.iter().find(|mount| {
        mount["mountId"].as_str() == Some(mount_id.as_str())
            && mount["status"].as_str() == Some("active")
    }) else {
        return WbError::new(404, "workspace_not_found", "workspace mount not found")
            .into_response();
    };
    if active["sourceKind"].as_str() != Some("storage")
        || active["provider"].as_str() != Some("local_fs")
    {
        return WbError::new(
            400,
            "unsupported_workspace_mount",
            "only local_fs workspace mounts can be removed here",
        )
        .into_response();
    }
    match state.resource_io().disable_mount(
        &mount_id,
        active["rootLocator"]["path"].as_str().map(Path::new),
        || resource_mounts::MountRegistry::new(&home).disable(&mount_id, Some(&studio), now_ms),
    ) {
        Ok(stored) => {
            audit(
                &state,
                &principal,
                &studio,
                "studio_workspace.remove_local_path",
                "success",
                stored["mountId"].as_str(),
                None,
            );
            Json(json!({ "ok": true, "mountId": stored["mountId"] })).into_response()
        }
        Err(err) => WbError::from_io(&err).into_response(),
    }
}

async fn studio_workspace_files(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    AxumPath(mount_id): AxumPath<String>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    if let Some(response) = scope_guard(&principal, "files.read") {
        return response;
    }
    let studio = state.management.identity_ids().1;
    let disclose = principal.is_local_owner();
    let subdir = query.get("subdir").cloned().unwrap_or_default();
    let mount_id = if mount_id.trim().is_empty() {
        "default".to_string()
    } else {
        mount_id
    };
    let ctx = OpContext::local_owner("");
    let root = match resolve_root_internal(&state, &studio, &mount_id) {
        Ok(root) => root,
        Err(err) => return err.into_response(),
    };
    match list_files(&state, &root, &subdir, disclose, &ctx).await {
        Ok(payload) => Json(payload).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn workbench_files(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    if let Some(response) = scope_guard(&principal, "files.read") {
        return response;
    }
    let studio = state.management.identity_ids().1;
    let disclose = principal.is_local_owner();
    let mount_id = mount_query_id(&query);
    let subdir = query.get("subdir").cloned().unwrap_or_default();
    let ctx = OpContext::local_owner("");
    let root = match resolve_root_internal(&state, &studio, &mount_id) {
        Ok(root) => root,
        Err(err) => return err.into_response(),
    };
    match list_files(&state, &root, &subdir, disclose, &ctx).await {
        Ok(payload) => Json(payload).into_response(),
        Err(err) => err.into_response(),
    }
}

async fn workbench_search(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    if let Some(response) = scope_guard(&principal, "files.read") {
        return response;
    }
    let studio = state.management.identity_ids().1;
    let disclose = principal.is_local_owner();
    let mount_id = mount_query_id(&query);
    let q = query.get("q").cloned().unwrap_or_default();
    let ctx = OpContext::local_owner("");
    let root = match resolve_root_internal(&state, &studio, &mount_id) {
        Ok(root) => root,
        Err(err) => return err.into_response(),
    };
    match search_files(&state, &root, &q, disclose, &ctx).await {
        Ok(payload) => Json(payload).into_response(),
        Err(err) => err.into_response(),
    }
}

// ── 内容服务（server/http/file-content.ts serveFileContent）──

const MIME_BY_EXT: [(&str, &str); 28] = [
    (".txt", "text/plain; charset=utf-8"),
    (".md", "text/markdown; charset=utf-8"),
    (".markdown", "text/markdown; charset=utf-8"),
    (".json", "application/json; charset=utf-8"),
    (".js", "text/javascript; charset=utf-8"),
    (".mjs", "text/javascript; charset=utf-8"),
    (".cjs", "text/javascript; charset=utf-8"),
    (".ts", "text/typescript; charset=utf-8"),
    (".tsx", "text/typescript; charset=utf-8"),
    (".css", "text/css; charset=utf-8"),
    (".html", "text/html; charset=utf-8"),
    (".svg", "image/svg+xml"),
    (".png", "image/png"),
    (".jpg", "image/jpeg"),
    (".jpeg", "image/jpeg"),
    (".gif", "image/gif"),
    (".webp", "image/webp"),
    (".avif", "image/avif"),
    (".ico", "image/x-icon"),
    (".woff", "font/woff"),
    (".woff2", "font/woff2"),
    (".ttf", "font/ttf"),
    (".otf", "font/otf"),
    (".wasm", "application/wasm"),
    (".mp4", "video/mp4"),
    (".webm", "video/webm"),
    (".mov", "video/quicktime"),
    (".pdf", "application/pdf"),
];

fn guess_mime(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| format!(".{e}"));
    if let Some(ext) = ext.as_deref() {
        for (known, mime) in MIME_BY_EXT {
            if known == ext {
                return mime;
            }
        }
    }
    "application/octet-stream"
}

/// 现役 weakEtag：W/"size-floor(mtimeMs)"。
fn weak_etag(meta: &std::fs::Metadata) -> String {
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("W/\"{}-{}\"", meta.len(), mtime_ms)
}

/// 现役 parseRangeHeader（file-content.ts :82-103）。
/// 返回 Some((start, end)) / None（无 Range）/ Err(unsatisfiable)。
fn parse_range_header(value: Option<&str>, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(raw) = value else { return Ok(None) };
    let raw = raw.trim();
    let Some(body) = raw.strip_prefix("bytes=") else {
        return Err(());
    };
    let Some((left, right)) = body.split_once('-') else {
        return Err(());
    };
    if left.is_empty() && right.is_empty() {
        return Err(());
    }
    let (start, end) = if left.is_empty() {
        let suffix: u64 = right.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        (size.saturating_sub(suffix), size.saturating_sub(1))
    } else {
        let start: u64 = left.parse().map_err(|_| ())?;
        let end: u64 = if right.is_empty() {
            size.saturating_sub(1)
        } else {
            right.parse().map_err(|_| ())?
        };
        (start, end)
    };
    if size == 0 || start >= size || start > end {
        return Err(());
    }
    Ok(Some((start, end.min(size - 1))))
}

/// 现役 asciiFilenameFallback（:113-134）：非可打印 ASCII（\x20-\x7E 之外）
/// 与 `"\\\r\n;/` 及空白折叠为单 `_`，剥首尾 `_`，截 80。
fn ascii_filename_fallback(filename: &str) -> String {
    let dot = filename.rfind('.');
    let ext = dot.map(|i| &filename[i + 1..]).unwrap_or("");
    let safe_ext =
        if !ext.is_empty() && ext.len() <= 12 && ext.bytes().all(|b| b.is_ascii_alphanumeric()) {
            format!(".{ext}")
        } else {
            String::new()
        };
    let stem_source = match dot {
        Some(i) => &filename[..i],
        None => filename,
    };
    let mut stem = String::new();
    let mut last_underscore = false;
    for ch in stem_source.chars() {
        let code = ch as u32;
        let valid = (0x20..=0x7e).contains(&code)
            && !matches!(ch, '"' | '\\' | '\r' | '\n' | ';' | '/')
            && !ch.is_whitespace();
        if valid {
            stem.push(ch);
            last_underscore = false;
        } else if !last_underscore {
            stem.push('_');
            last_underscore = true;
        }
    }
    let stem = stem.trim_matches('_').chars().take(80).collect::<String>();
    let stem_ok = !stem.is_empty()
        && stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if stem_ok {
        format!("{stem}{safe_ext}")
    } else {
        format!("download{safe_ext}")
    }
}

fn content_disposition(filename: &str) -> String {
    let fallback = ascii_filename_fallback(filename);
    let encoded: String = filename
        .bytes()
        .flat_map(|b| {
            if b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
                )
            {
                vec![(b as char).to_string()]
            } else {
                vec![format!("%{b:02X}")]
            }
        })
        .collect();
    format!("inline; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

async fn workbench_content(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
    method: axum::http::Method,
) -> Response {
    if let Some(response) = scope_guard(&principal, "files.read") {
        return response;
    }
    let studio = state.management.identity_ids().1;
    let mount_id = mount_query_id(&query);
    let subdir = query.get("subdir").cloned().unwrap_or_default();
    let name = query.get("name").map(|v| Value::String(v.clone()));
    let head_only = method == axum::http::Method::HEAD;
    let root = match resolve_root_internal(&state, &studio, &mount_id) {
        Ok(root) => root,
        Err(err) => return err.into_response(),
    };
    let (file_path, filename) = match content_target(&root, &subdir, name.as_ref()) {
        Ok(target) => target,
        Err(err) => return err.into_response(),
    };
    let Ok(meta) = std::fs::metadata(&file_path) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "file_not_found" })),
        )
            .into_response();
    };
    if !meta.is_file() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "not_a_file" })),
        )
            .into_response();
    }
    let size = meta.len();
    let etag = weak_etag(&meta);
    let header_value = |value: &str| {
        HeaderValue::from_str(value).unwrap_or(HeaderValue::from_static("application/octet-stream"))
    };
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == etag)
    {
        let mut response = (StatusCode::NOT_MODIFIED, Body::empty()).into_response();
        response
            .headers_mut()
            .insert(header::ETAG, header_value(&etag));
        return response;
    }
    let range = match parse_range_header(
        headers.get(header::RANGE).and_then(|v| v.to_str().ok()),
        size,
    ) {
        Ok(range) => range,
        Err(()) => {
            let mut response = (StatusCode::RANGE_NOT_SATISFIABLE, Body::empty()).into_response();
            let headers = response.headers_mut();
            headers.insert(
                header::CONTENT_RANGE,
                header_value(&format!("bytes */{size}")),
            );
            headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
            return response;
        }
    };
    let (start, end) = range.unwrap_or((0, size.saturating_sub(1)));
    let length = if size == 0 { 0 } else { end - start + 1 };
    let status = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let body = if head_only || size == 0 {
        Body::empty()
    } else {
        // 窗口读取与媒体产物面同一形态（crate 未开 tokio fs feature）。
        let path = file_path.clone();
        match tokio::task::spawn_blocking(move || {
            use std::io::{Read as _, Seek as _, SeekFrom};
            let length = usize::try_from(length).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "window too large")
            })?;
            let mut file = std::fs::File::open(&path)?;
            file.seek(SeekFrom::Start(start))?;
            let mut buffer = vec![0u8; length];
            file.read_exact(&mut buffer)?;
            Ok::<Vec<u8>, std::io::Error>(buffer)
        })
        .await
        {
            Ok(Ok(bytes)) => Body::from(bytes),
            _ => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "file_not_found" })),
                )
                    .into_response()
            }
        }
    };
    let mut response = Response::new(body);
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, header_value(guess_mime(&filename)));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(header::CONTENT_LENGTH, header_value(&length.to_string()));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=0, must-revalidate"),
    );
    headers.insert(header::ETAG, header_value(&etag));
    headers.insert("x-lingxi-file-mtimems", header_value(&mtime_ms.to_string()));
    headers.insert("x-lingxi-file-size", header_value(&size.to_string()));
    if let Some((start, end)) = range {
        headers.insert(
            header::CONTENT_RANGE,
            header_value(&format!("bytes {start}-{end}/{size}")),
        );
    }
    headers.insert(
        header::CONTENT_DISPOSITION,
        header_value(&content_disposition(&filename)),
    );
    response
}

// ── 写动作路由（mobile-workbench.ts runWorkbenchAction + writeActionResponse）──

async fn workbench_actions(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(response) = scope_guard(&principal, "files.write") {
        return response;
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return crate::EndpointError::from_json_rejection(&rejection).into_response()
        }
    };
    let studio = state.management.identity_ids().1;
    let disclose = principal.is_local_owner();
    let mount_id = mount_body_id(&body);
    let subdir = body
        .get("subdir")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let action = body.get("action").and_then(Value::as_str).unwrap_or("");
    let audit_action = match action {
        "mkdir" => "mobile_workbench.mkdir",
        "create" | "writeText" => "mobile_workbench.write",
        "rename" => "mobile_workbench.rename",
        "move" => "mobile_workbench.move",
        "movePaths" => "mobile_workbench.move_paths",
        "safeDelete" => "mobile_workbench.safe_delete",
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "unknown_action" })),
            )
                .into_response()
        }
    };
    let lease = match issue_remote_write_lease(
        &state,
        &principal,
        &studio,
        &mount_id,
        io_capable_now(&state),
    ) {
        Ok(lease) => lease,
        Err(error) => return error.into_response(),
    };
    let ctx = op_ctx(&body, audit_action);
    let root = match resolve_root_internal(&state, &studio, &mount_id) {
        Ok(root) => root,
        Err(err) => {
            if let Some(lease) = &lease {
                if let Err(error) = settle_remote_write_lease(&state, lease, false) {
                    return lease_settlement_failure(
                        lease,
                        error,
                        "not_started",
                        json!({"error":err.code,"detail":err.detail}),
                    );
                }
            }
            return err.into_response();
        }
    };
    let result = match action {
        "mkdir" => action_mkdir(&state, &root, &subdir, &body, disclose, &ctx).await,
        "create" | "writeText" => {
            action_write_text(&state, &root, &subdir, &body, disclose, &ctx, false).await
        }
        "rename" => action_rename(&state, &root, &subdir, &body, disclose, &ctx).await,
        "move" => action_move(&state, &root, &subdir, &body, disclose, &ctx).await,
        "movePaths" => action_move_paths(&state, &root, &body, disclose, &ctx).await,
        "safeDelete" => action_safe_delete(&state, &root, &subdir, &body, disclose, &ctx).await,
        _ => unreachable!(),
    };
    match result {
        Ok(payload) => {
            if let Some(lease) = &lease {
                if let Err(error) = settle_remote_write_lease(&state, lease, true) {
                    return lease_settlement_failure(lease, error, "result_produced", payload);
                }
            }
            let ok = payload["ok"].as_bool().unwrap_or(true);
            audit(
                &state,
                &principal,
                &studio,
                audit_action,
                if ok { "success" } else { "failed" },
                None,
                lease.as_ref().map(|l| l.lease_id.as_str()),
            );
            Json(payload).into_response()
        }
        Err(err) => {
            if let Some(lease) = &lease {
                if let Err(error) = settle_remote_write_lease(&state, lease, false) {
                    return lease_settlement_failure(
                        lease,
                        error,
                        "failed_or_partial",
                        json!({"error":err.code,"detail":err.detail}),
                    );
                }
            }
            err.into_response()
        }
    }
}

/// 现役 upload（mobile-workbench.ts :119-156）：files[]（缺省按 body 单件）
/// 逐项 contentBase64 → 25MB 解码上限 → 写文件 → results[] + 目录清单。
async fn workbench_upload(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(response) = scope_guard(&principal, "files.write") {
        return response;
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return crate::EndpointError::from_json_rejection(&rejection).into_response()
        }
    };
    let studio = state.management.identity_ids().1;
    let disclose = principal.is_local_owner();
    let mount_id = mount_body_id(&body);
    let subdir = body
        .get("subdir")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let lease = match issue_remote_write_lease(
        &state,
        &principal,
        &studio,
        &mount_id,
        io_capable_now(&state),
    ) {
        Ok(lease) => lease,
        Err(error) => return error.into_response(),
    };
    let ctx = op_ctx(&body, "mobile_workbench.upload");
    let root = match resolve_root_internal(&state, &studio, &mount_id) {
        Ok(root) => root,
        Err(err) => {
            if let Some(lease) = &lease {
                if let Err(error) = settle_remote_write_lease(&state, lease, false) {
                    return lease_settlement_failure(
                        lease,
                        error,
                        "not_started",
                        json!({"error":err.code,"detail":err.detail}),
                    );
                }
            }
            return err.into_response();
        }
    };
    let outcome = upload_inner(&state, &root, &subdir, &body, disclose, &ctx).await;
    match outcome {
        Ok(payload) => {
            if let Some(lease) = &lease {
                if let Err(error) = settle_remote_write_lease(&state, lease, true) {
                    return lease_settlement_failure(lease, error, "result_produced", payload);
                }
            }
            let ok = payload["ok"].as_bool().unwrap_or(true);
            audit(
                &state,
                &principal,
                &studio,
                "mobile_workbench.upload",
                if ok { "success" } else { "failed" },
                None,
                lease.as_ref().map(|l| l.lease_id.as_str()),
            );
            Json(payload).into_response()
        }
        Err(err) => {
            if let Some(lease) = &lease {
                if let Err(error) = settle_remote_write_lease(&state, lease, false) {
                    return lease_settlement_failure(
                        lease,
                        error,
                        "failed_or_partial",
                        json!({"error":err.code,"detail":err.detail}),
                    );
                }
            }
            err.into_response()
        }
    }
}

async fn upload_inner(
    state: &ServiceState,
    root: &ResolvedRoot,
    subdir: &str,
    body: &Value,
    disclose: bool,
    ctx: &OpContext,
) -> Result<Value, WbError> {
    let files_param: Vec<Value> = match body.get("files").and_then(Value::as_array) {
        Some(list) => list.clone(),
        None => vec![body.clone()],
    };
    // writeFileTarget 的目录/能力闸先于逐项解码（现役 fileService 在循环外
    // 构造，首次写入即触发；此处显式前置，错误形态一致）。
    let (dir, _normalized) = write_dir(root, subdir)?;
    let mut results = Vec::new();
    for file in &files_param {
        let name_raw = file.get("name").and_then(Value::as_str);
        let item = async {
            let content_base64 = file
                .get("contentBase64")
                .and_then(Value::as_str)
                .unwrap_or("");
            if content_base64.is_empty() {
                return Err(WbError::new(
                    400,
                    "invalid_upload",
                    "contentBase64 required",
                ));
            }
            let bytes = decode_base64(content_base64)
                .ok_or_else(|| WbError::new(400, "invalid_upload", "contentBase64 required"))?;
            if bytes.len() > MAX_UPLOAD_BYTES {
                return Err(WbError::new(413, "file_too_large", "file too large"));
            }
            let filename = normalize_plain_name(file.get("name"))?;
            if resolve_file_target(&root.path, &dir, &filename).is_none() {
                return Err(WbError::new(400, "invalid_path", "invalid path"));
            }
            let target_ref = ref_for_target(root, &target_subpath(subdir, &filename)?);
            state
                .resource_io()
                .write(&target_ref, bytes.clone(), ctx)
                .await?;
            Ok::<(String, usize), WbError>((filename, bytes.len()))
        }
        .await;
        match item {
            Ok((filename, size)) => {
                results.push(json!({ "name": filename, "ok": true, "size": size }));
            }
            Err(err) => {
                results.push(json!({
                    "name": name_raw.map(Value::from).unwrap_or(Value::Null),
                    "ok": false,
                    "error": err.code,
                }));
            }
        }
    }
    let files = files_for_directory(state, root, subdir, disclose, ctx).await?;
    let all_ok = results
        .iter()
        .all(|item| item["ok"].as_bool().unwrap_or(false));
    Ok(json!({
        "ok": all_ok,
        "rootId": root.id,
        "mountId": root.mount_id,
        "results": results,
        "files": files,
    }))
}

/// 现役 Buffer.from(contentBase64, "base64") 的宽松解码（忽略 ASCII 空白、
/// 容忍缺省 padding）；非 base64 字符仍会产出截断字节——候选人对剥离空白
/// 后的严格/无填充两档解码取成功档，仍拒绝无法解码的输入（登记偏差：
/// 现役对垃圾输入静默产出截断内容，候选人显式失败）。
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let cleaned: String = input.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(&cleaned)
        .ok()
        .or_else(|| {
            base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(&cleaned)
                .ok()
        })
}

pub fn router() -> Router<ServiceState> {
    Router::new()
        .route(
            "/lingxi/v1/studio/workspaces",
            get(studio_workspaces_list).post(studio_workspaces_create),
        )
        .route(
            "/lingxi/v1/studio/workspaces/{mountId}",
            delete(studio_workspaces_delete),
        )
        .route(
            "/lingxi/v1/studio/workspaces/{mountId}/files",
            get(studio_workspace_files),
        )
        .route("/lingxi/v1/workbench/files", get(workbench_files))
        .route("/lingxi/v1/workbench/search", get(workbench_search))
        .route(
            "/lingxi/v1/workbench/content",
            get(workbench_content).head(workbench_content),
        )
        .route("/lingxi/v1/workbench/actions", post(workbench_actions))
        // 现役 MAX_UPLOAD_BODY_BYTES=80MiB（mobile-workbench.ts:22-27 路由级
        // bodyParser 限额）：覆盖服务全局 1MiB 默认，单文件解码后 25MiB 上限
        // 由处理器内 per-item 校验承载。
        .route(
            "/lingxi/v1/workbench/upload",
            post(workbench_upload).layer(axum::extract::DefaultBodyLimit::max(80 * 1024 * 1024)),
        )
}
