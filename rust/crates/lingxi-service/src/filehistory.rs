//! R06-T05 Round 5：工作区文件历史（叶 #11 + 叶面断言 #19-21）。
//!
//! 对照现役（RC-3，逐项锚点进 07_diff_ledger）：
//! - `lib/file-history/history-store.ts`：recordSnapshot 合并窗
//!   （:84-116——同 sha → unchanged；窗内（<60s、非负、双侧非
//!   restore）→ UPDATE merged；否则 INSERT）、listFiles
//!   （:144-151，lastCapturedAt DESC）、listVersions（:153-160，
//!   capturedAt DESC,id DESC）、getSnapshotContent 缺失抛
//!   "file-history snapshot N not found"（:167）。
//! - `lib/file-history/file-history-service.ts`：workspaceHashForRoot
//!   （:19-22，sha256(斜杠归一路径)[..16]）、_capture 准入闸
//!   （:227-241，isFile + 尺寸 + 策略）、_locate 的最深根匹配与
//!   ".." 拒绝（:197-213）。
//! - `lib/file-history/text-file-policy.ts`：扩展名/文件名/churn/
//!   噪音目录四表逐行落（:4-62）。
//! - `server/routes/file-history.ts`：四叶面错误词汇
//!   （agentId required 400 / workspace not tracked 404 /
//!   invalid relPath|id|snapshotId 400 / snapshot 404 /
//!   corrupt snapshot path 500），restore = ResourceIO 写回 +
//!   restore-origin 再捕获（:86-89）。
//!
//! 设计登记（台账）：
//! - D8：候选人单工作区单库（v9 `file_history_snapshots` 表在
//!   runs.db，现役是每工作区一个 history.sqlite 双表 + gzip +
//!   op_context + files.deleted_at——差异逐列登记）；无 watcher/
//!   sweep/删除与重命名历史标记（退役到后续阶段）。
//! - ResourceIO 接线：写路径 emit 后同步捕获（现役是 150ms 防抖
//!   异步捕获；候选人不防抖，净落库状态一致），origin 默认
//!   "event"，restore 路由经 OpContext.capture_origin 传
//!   "restore"（现役 restore 后 ResourceIO 写触发的事件捕获与
//!   captureNow("restore") 竞态合并，净效果 = 一行 restore）。
//! - D12 延展：query 未知键/重复键 → 400。
//! - 单工作区：agentId 只做存在性校验（:23 的必填语义），不映射
//!   多工作区（现役按 agentId 解析 desk；候选实例只有一个根）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use sha2::Digest as _;

use lingxi_adapters::storage::session_files::{
    self, FileHistoryFileEntry, FileSnapshotInsert, FileSnapshotMeta,
};
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::StorageError;

use crate::resourceio::OpContext;
use crate::ServiceState;

// ── 常数（text-file-policy.ts :4；history-store.ts :42；file-history-service.ts :11-17） ──

pub const MAX_SNAPSHOT_BYTES: u64 = 5 * 1024 * 1024;
pub const MERGE_WINDOW_MS: u64 = 60_000;
pub const MAX_AGE_MS: u64 = 30 * 24 * 3600 * 1000;
pub const MAX_TOTAL_BYTES: u64 = 500 * 1024 * 1024;

const TEXT_EXTENSIONS: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "mdx",
    "rst",
    "tex",
    "json",
    "jsonc",
    "json5",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "properties",
    "xml",
    "html",
    "htm",
    "xhtml",
    "css",
    "scss",
    "less",
    "svg",
    "js",
    "jsx",
    "ts",
    "tsx",
    "mjs",
    "cjs",
    "mts",
    "cts",
    "vue",
    "svelte",
    "astro",
    "py",
    "rb",
    "go",
    "rs",
    "java",
    "kt",
    "kts",
    "swift",
    "scala",
    "clj",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hpp",
    "hh",
    "cs",
    "php",
    "lua",
    "pl",
    "r",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "bat",
    "cmd",
    "sql",
    "graphql",
    "gql",
    "proto",
    "csv",
    "tsv",
    "env",
];

const KNOWN_TEXT_FILENAMES: &[&str] = &[
    ".gitignore",
    ".gitattributes",
    ".editorconfig",
    ".env",
    ".npmrc",
    ".nvmrc",
    "Makefile",
    "Dockerfile",
    "LICENSE",
    "README",
    "CHANGELOG",
];

const CHURN_FILENAMES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
];
const CHURN_EXTENSIONS: &[&str] = &["log", "lock", "tmp", "temp", "swp"];

const IGNORED_DIR_NAMES: &[&str] = &[
    "node_modules",
    "dist",
    "build",
    "out",
    "coverage",
    "target",
    "__pycache__",
    "venv",
    ".venv",
];

// ── 纯函数（text-file-policy.ts / file-history-service.ts / file-history.ts） ──

fn basename_of(rel_path: &str) -> &str {
    match rel_path.rfind('/') {
        Some(idx) => &rel_path[idx + 1..],
        None => rel_path,
    }
}

/// extOf（:37-41）：末点必须在非首非末位置，否则 None。
fn ext_of(name: &str) -> Option<String> {
    let idx = name.rfind('.')?;
    if idx == 0 || idx == name.len() - 1 {
        return None;
    }
    Some(name[idx + 1..].to_lowercase())
}

/// isIgnoredRelPath（:44-52）：任一**目录段**（不含末段文件名）命中
/// 忽略表或以 "." 开头即整棵排除。
pub fn is_ignored_rel_path(rel_path: &str) -> bool {
    let segments: Vec<&str> = rel_path.split('/').collect();
    for seg in &segments[..segments.len().saturating_sub(1)] {
        if seg.is_empty() {
            continue;
        }
        if IGNORED_DIR_NAMES.contains(seg) || seg.starts_with('.') {
            return true;
        }
    }
    false
}

/// isTrackedFile（:55-62）：churn 黑名单优先于扩展名白名单。
pub fn is_tracked_file(rel_path: &str) -> bool {
    let name = basename_of(rel_path);
    if CHURN_FILENAMES.contains(&name) {
        return false;
    }
    let ext = ext_of(name);
    if let Some(ext) = &ext {
        if CHURN_EXTENSIONS.contains(&ext.as_str()) {
            return false;
        }
    }
    if KNOWN_TEXT_FILENAMES.contains(&name) {
        return true;
    }
    ext.is_some_and(|ext| TEXT_EXTENSIONS.contains(&ext.as_str()))
}

/// workspaceHashForRoot（file-history-service.ts :19-22）：
/// sha256(斜杠归一化路径) 十六进制前 16 字符。入参须已是绝对路径
/// （现役 path.resolve；候选调用方传 canonical 根）。
pub fn workspace_hash_for_root(root: &Path) -> String {
    let normalized = root.to_string_lossy().replace('\\', "/");
    let digest = sha2::Sha256::digest(normalized.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// safeRelPath（file-history.ts :10-16）：非空、非绝对、无反斜杠、
/// 无空/`.`/`..` 段。
pub fn safe_rel_path(value: &str) -> Option<&str> {
    if value.is_empty() || Path::new(value).is_absolute() || value.contains('\\') {
        return None;
    }
    if value
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return None;
    }
    Some(value)
}

// ── 服务 ──

/// recordSnapshot 的三种结局（history-store.ts :32-35）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordOutcome {
    Inserted(i64),
    Merged(i64),
    Unchanged(i64),
}

/// getSnapshotContent 的返回形状（:162-169）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInfo {
    pub rel_path: String,
    pub captured_at_ms: u64,
    pub origin: String,
    pub content: Vec<u8>,
}

pub struct FileHistoryService {
    db: Arc<RunDatabase>,
    workspace: Option<PathBuf>,
    workspace_hash: Option<String>,
    now: Box<dyn Fn() -> u64 + Send + Sync>,
    merge_window_ms: u64,
}

impl FileHistoryService {
    /// workspace 为 None 时实例存在但未跟踪任何根（hasWorkspace 恒假，
    /// 路由面走 404 "workspace not tracked"）。传入的根做 canonical 归一
    /// （失败则按未跟踪处理——与现役 statSync 失败跳过 :90-91 同语义）。
    pub fn new(
        db: Arc<RunDatabase>,
        workspace_root: Option<PathBuf>,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        Self::with_merge_window(db, workspace_root, MERGE_WINDOW_MS, now)
    }

    pub fn with_merge_window(
        db: Arc<RunDatabase>,
        workspace_root: Option<PathBuf>,
        merge_window_ms: u64,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        let workspace = workspace_root.and_then(|root| std::fs::canonicalize(root).ok());
        let workspace_hash = workspace.as_ref().map(|root| workspace_hash_for_root(root));
        FileHistoryService {
            db,
            workspace,
            workspace_hash,
            now: Box::new(now),
            merge_window_ms,
        }
    }

    pub fn has_workspace(&self) -> bool {
        self.workspace_hash.is_some()
    }

    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }

    fn require_hash(&self) -> Result<&str, StorageError> {
        self.workspace_hash
            .as_deref()
            .ok_or_else(|| StorageError::InvalidRequest {
                detail: "file-history: workspace not tracked".to_string(),
            })
    }

    /// recordSnapshot（history-store.ts :84-116）。
    pub async fn record_snapshot(
        &self,
        rel_path: &str,
        content: &[u8],
        origin: &str,
    ) -> Result<RecordOutcome, StorageError> {
        let captured_at = (self.now)();
        self.record_snapshot_at(rel_path, content, origin, captured_at)
            .await
    }

    pub async fn record_snapshot_at(
        &self,
        rel_path: &str,
        content: &[u8],
        origin: &str,
        captured_at_ms: u64,
    ) -> Result<RecordOutcome, StorageError> {
        let ws = self.require_hash()?.to_string();
        let digest = sha2::Sha256::digest(content);
        let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        let latest = session_files::latest_file_snapshot_meta(&self.db, &ws, rel_path).await?;
        if let Some(latest) = &latest {
            // 同 sha → unchanged（:91-94）。
            if latest.sha256 == hash {
                return Ok(RecordOutcome::Unchanged(latest.id));
            }
            // withinWindow（:97-101）：窗内、非负、双侧非 restore。
            let delta = captured_at_ms as i128 - latest.captured_at_ms as i128;
            let within_window = delta >= 0
                && (delta as u64) < self.merge_window_ms
                && latest.origin != "restore"
                && origin != "restore";
            if within_window {
                session_files::update_file_snapshot(
                    &self.db,
                    &ws,
                    latest.id,
                    captured_at_ms,
                    origin,
                    content,
                )
                .await?;
                return Ok(RecordOutcome::Merged(latest.id));
            }
        }
        let id = session_files::insert_file_snapshot(
            &self.db,
            &FileSnapshotInsert {
                workspace_hash: &ws,
                rel_path,
                captured_at_ms,
                origin,
                content: content.to_vec(),
            },
        )
        .await?;
        Ok(RecordOutcome::Inserted(id))
    }

    /// _capture（file-history-service.ts :227-241）+ _locate（:197-213）
    /// 的单根映射：绝对路径必须落在工作区内，过忽略表/跟踪表/尺寸闸。
    /// 错误一律吞（tracing::warn 留痕），返回是否捕获。
    pub async fn capture_from_disk(&self, abs_path: &Path, origin: &str) -> bool {
        let outcome = self.capture_from_disk_inner(abs_path, origin).await;
        if let Err(err) = &outcome {
            tracing::warn!(
                "file-history capture error for {}: {err}",
                abs_path.display()
            );
        }
        matches!(outcome, Ok(true))
    }

    async fn capture_from_disk_inner(&self, abs_path: &Path, origin: &str) -> Result<bool, String> {
        let Some(root) = self.workspace.as_deref() else {
            return Ok(false);
        };
        if !abs_path.is_absolute() {
            return Ok(false);
        }
        // _locate :200-210：解析后必须严格在工作区根内。
        let resolved = match std::fs::canonicalize(abs_path) {
            Ok(resolved) => resolved,
            Err(_) => return Ok(false),
        };
        let rel = match resolved.strip_prefix(root) {
            Ok(rel) if !rel.as_os_str().is_empty() => rel,
            _ => return Ok(false),
        };
        let rel_slash = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if is_ignored_rel_path(&rel_slash) || !is_tracked_file(&rel_slash) {
            return Ok(false);
        }
        let meta = std::fs::metadata(&resolved).map_err(|err| err.to_string())?;
        if !meta.is_file() {
            return Ok(false);
        }
        if meta.len() > MAX_SNAPSHOT_BYTES {
            tracing::warn!(
                "file-history skip oversized file: {rel_slash} ({} bytes)",
                meta.len()
            );
            return Ok(false);
        }
        let content = std::fs::read(&resolved).map_err(|err| err.to_string())?;
        self.record_snapshot(&rel_slash, &content, origin)
            .await
            .map_err(|err| err.to_string())?;
        Ok(true)
    }

    pub async fn list_files(&self) -> Result<Vec<FileHistoryFileEntry>, StorageError> {
        let ws = self.require_hash()?.to_string();
        session_files::list_file_history_files(&self.db, &ws).await
    }

    pub async fn list_versions(
        &self,
        rel_path: &str,
    ) -> Result<Vec<FileSnapshotMeta>, StorageError> {
        let ws = self.require_hash()?.to_string();
        session_files::list_file_history_versions(&self.db, &ws, rel_path).await
    }

    pub async fn get_snapshot_content(
        &self,
        id: i64,
    ) -> Result<Option<SnapshotInfo>, StorageError> {
        let ws = self.require_hash()?.to_string();
        Ok(session_files::get_file_snapshot(&self.db, &ws, id)
            .await?
            .map(|row| SnapshotInfo {
                rel_path: row.rel_path,
                captured_at_ms: row.captured_at_ms,
                origin: row.origin,
                content: row.content,
            }))
    }

    /// 保留策略（history-store.ts enforceRetention :176-186 的单库映射）。
    pub async fn enforce_retention(&self) -> Result<u64, StorageError> {
        let ws = self.require_hash()?.to_string();
        session_files::prune_file_history(&self.db, &ws, MAX_AGE_MS, MAX_TOTAL_BYTES, (self.now)())
            .await
    }
}

// ── 路由（server/routes/file-history.ts 四叶面） ──

fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error": message.into()}))).into_response()
}

/// D12 延展：query 只认声明过的键，未知/重复 → 400。
/// Err 直接携带 axum Response 供 handler 原样 return；拒绝路径非热
/// 路径，不做 Box 瘦身（lib.rs gate_session_tree_write 同款纪律）。
#[allow(clippy::result_large_err)]
fn parse_strict_query(
    query: Option<&str>,
    allowed: &[&str],
) -> Result<Vec<(String, String)>, Response> {
    let pairs = crate::ws::parse_query_pairs(query.unwrap_or(""));
    let mut seen: Vec<&str> = Vec::new();
    for (key, _) in &pairs {
        if !allowed.contains(&key.as_str()) {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                format!("unknown file-history query parameter: {key}"),
            ));
        }
        if seen.contains(&key.as_str()) {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                format!("duplicate file-history query parameter: {key}"),
            ));
        }
        seen.push(key.as_str());
    }
    Ok(pairs)
}

fn query_value<'p>(pairs: &'p [(String, String)], key: &str) -> Option<&'p str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// withWorkspace（file-history.ts :21-28）的单工作区映射：agentId 必填 +
/// 已跟踪工作区（未配置工作区的实例一律 404）。
#[allow(clippy::result_large_err)] // 同 parse_strict_query 的响应直携纪律。
fn require_workspace(state: &ServiceState, agent_id: Option<&str>) -> Result<(), Response> {
    let agent_id = agent_id.unwrap_or("");
    if agent_id.is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "agentId required"));
    }
    if !state.file_history().has_workspace() {
        return Err(json_error(StatusCode::NOT_FOUND, "workspace not tracked"));
    }
    Ok(())
}

/// GET /lingxi/v1/file-history/files（file-history.ts :30-38）。
pub async fn list_files_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let pairs = match parse_strict_query(query.as_deref(), &["agentId"]) {
        Ok(pairs) => pairs,
        Err(resp) => return resp,
    };
    if let Err(resp) = require_workspace(&state, query_value(&pairs, "agentId")) {
        return resp;
    }
    match state.file_history().list_files().await {
        Ok(files) => {
            let files: Vec<Value> = files
                .into_iter()
                .map(|entry| {
                    json!({
                        "relPath": entry.rel_path,
                        // 候选人无 files.deleted_at 列（D8 台账）：恒 null。
                        "deletedAt": Value::Null,
                        "lastCapturedAt": entry.last_captured_at_ms,
                        "snapshotCount": entry.version_count,
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({ "files": files }))).into_response()
        }
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

/// GET /lingxi/v1/file-history/versions（file-history.ts :40-50）。
pub async fn list_versions_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let pairs = match parse_strict_query(query.as_deref(), &["agentId", "relPath"]) {
        Ok(pairs) => pairs,
        Err(resp) => return resp,
    };
    if let Err(resp) = require_workspace(&state, query_value(&pairs, "agentId")) {
        return resp;
    }
    let Some(rel_path) = query_value(&pairs, "relPath").and_then(safe_rel_path) else {
        return json_error(StatusCode::BAD_REQUEST, "invalid relPath");
    };
    match state.file_history().list_versions(rel_path).await {
        Ok(versions) => {
            let versions: Vec<Value> = versions
                .into_iter()
                .map(|meta| {
                    json!({
                        "id": meta.id,
                        "capturedAt": meta.captured_at_ms,
                        "origin": meta.origin,
                        // 候选人无 op_context 列（D8 台账）：恒 null。
                        "opContext": Value::Null,
                        "rawSize": meta.size_bytes,
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({ "versions": versions }))).into_response()
        }
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

/// snapshot id 解析（:56-57 / :78-79）：正整数，否则 400。
fn parse_snapshot_id(raw: Option<&str>) -> Option<i64> {
    let raw = raw?;
    let id: i64 = raw.parse().ok()?;
    if id <= 0 {
        return None;
    }
    Some(id)
}

#[allow(clippy::result_large_err)] // 同 parse_strict_query 的响应直携纪律。
async fn snapshot_or_404(state: &ServiceState, id: i64) -> Result<SnapshotInfo, Response> {
    match state.file_history().get_snapshot_content(id).await {
        Ok(Some(snapshot)) => Ok(snapshot),
        Ok(None) => Err(json_error(
            StatusCode::NOT_FOUND,
            format!("file-history snapshot {id} not found"),
        )),
        Err(err) => Err(json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            err.to_string(),
        )),
    }
}

/// GET /lingxi/v1/file-history/snapshot（file-history.ts :52-68）。
pub async fn snapshot_route(
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    let pairs = match parse_strict_query(query.as_deref(), &["agentId", "id"]) {
        Ok(pairs) => pairs,
        Err(resp) => return resp,
    };
    if let Err(resp) = require_workspace(&state, query_value(&pairs, "agentId")) {
        return resp;
    }
    let Some(id) = parse_snapshot_id(query_value(&pairs, "id")) else {
        return json_error(StatusCode::BAD_REQUEST, "invalid id");
    };
    match snapshot_or_404(&state, id).await {
        Ok(snapshot) => (
            StatusCode::OK,
            Json(json!({
                "relPath": snapshot.rel_path,
                "capturedAt": snapshot.captured_at_ms,
                "origin": snapshot.origin,
                // 现役 Buffer.toString("utf-8")：有损替换（:63）。
                "content": String::from_utf8_lossy(&snapshot.content),
            })),
        )
            .into_response(),
        Err(resp) => resp,
    }
}

/// POST /lingxi/v1/file-history/restore（file-history.ts :70-93）：
/// 读快照 → safeRelPath 复核（腐坏 500）→ ResourceIO 写回（写路径
/// 经 OpContext.capture_origin 落一行 "restore" 历史）。
pub async fn restore_route(
    State(state): State<ServiceState>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return crate::EndpointError::from_json_rejection(&rejection).into_response()
        }
    };
    let agent_id = body.get("agentId").and_then(Value::as_str).unwrap_or("");
    if let Err(resp) = require_workspace(&state, Some(agent_id)) {
        return resp;
    }
    let snapshot_id = body
        .get("snapshotId")
        .and_then(Value::as_i64)
        .filter(|id| *id > 0);
    let Some(snapshot_id) = snapshot_id else {
        return json_error(StatusCode::BAD_REQUEST, "invalid snapshotId");
    };
    let snapshot = match snapshot_or_404(&state, snapshot_id).await {
        Ok(snapshot) => snapshot,
        Err(resp) => return resp,
    };
    let Some(rel_path) = safe_rel_path(&snapshot.rel_path) else {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "corrupt snapshot path");
    };
    let Some(root) = state.file_history().workspace_root() else {
        return json_error(StatusCode::NOT_FOUND, "workspace not tracked");
    };
    let abs_path = rel_path
        .split('/')
        .fold(root.to_path_buf(), |acc, seg| acc.join(seg));
    let mut ctx = OpContext::local_owner("");
    ctx.capture_origin = Some("restore".to_string());
    let reference = json!({"kind": "local-file", "path": abs_path.to_string_lossy()});
    match state
        .resource_io()
        .write(&reference, snapshot.content, &ctx)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "relPath": rel_path })),
        )
            .into_response(),
        Err(err) => json_error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}
