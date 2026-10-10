//! R06-T05 会话文件服务：sf_ 身份注册、旧 sidecar 迁移适配、fork 复制、
//! 引用检查与冷缓存清理（交付物② SessionFile 迁移适配 + ③ 文件引用检查器
//! 的注册侧）。
//!
//! 对照现役（RC-3，逐项锚点进 07_diff_ledger）：
//! - 身份公式：`lib/session-files/session-file-registry.ts`
//!   buildSessionFileId :867-874 / sessionFileOwnerKey :876-888 /
//!   buildSessionFileSourceKey :24-33 / sessionFilesCacheDir :17-22。
//! - 注册去重与 materialization 保留：registerFile :74-165。
//! - fork：forkSessionFiles :255-360（仅「保留引用可达」的文件被复制，
//!   legacyFileIds/legacyFilePaths 累积，managed payload 复制到目标缓存）。
//! - 清理：cleanupColdSessionFiles :517-571（冷度来源映射差异 D3：
//!   jsonl mtime → sessions.last_activity_unix_ms；任务书第 4 条新增
//!   跨会话引用保护——被存活会话引用的交付物不得回收）。
//! - 安全闸：`shared/file-import-security.ts` inspectLocalImportPath +
//!   `shared/path-security.ts` isSensitivePath（SENSITIVE_DIRS +
//!   lingxiHome 封锁）。
//! - 标记收集：collectSessionFileReferenceIdentities :997-1021 +
//!   SESSION_FILE_MARKER_RE / ATTACHED_MEDIA_MARKER_RE :944-945。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use lingxi_adapters::storage::session_files as store;
use lingxi_adapters::storage::session_files::{
    FileKind, RefKind, SessionFileOrigin, SessionFileRow, SessionFileStatus, StorageKind,
};
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::StorageError;

use crate::filemeta;

/// 现役 SESSION_FILE_CACHE_INACTIVE_TTL_MS（registry :9）。
pub const SESSION_FILE_CACHE_INACTIVE_TTL_MS: u64 = 72 * 60 * 60 * 1000;

/// 现役 readSample 的采样上限（registry :856-866，8KB）。
const MIME_SAMPLE_BYTES: usize = 8192;

/// 现役 SENSITIVE_DIRS（path-security.ts:14），相对 $HOME。
const SENSITIVE_DIRS: &[&str] = &[".ssh", ".gnupg", ".aws", ".config/gcloud", ".kube"];

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    out
}

// ── 身份公式（registry :17-33, :867-888；金样在 tests/r06_t05_session_files.rs） ──

/// 现役 sessionFileOwnerKey：trim 后非空的 sessionId 优先（`id:` 前缀），
/// 否则 sessionPath（`path:` 前缀）。
pub fn session_file_owner_key(session_id: Option<&str>, session_path: Option<&str>) -> String {
    if let Some(id) = session_id {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return format!("id:{trimmed}");
        }
    }
    if let Some(path) = session_path {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return format!("path:{trimmed}");
        }
    }
    // 现役对全空输入落成 `path:undefined` 一类字符串属于调用方缺陷；
    // 候选响亮拒绝由调用方签名（Option 双空在类型外）与调试断言兜底。
    "path:".to_string()
}

/// 现役 buildSessionFileSourceKey：ns 逐字符清洗（非法字符 → `_`，截断
/// 80，空 → "source"）+ `sha256hex(JSON.stringify(parts.map(String)))`。
pub fn build_session_file_source_key(namespace: &str, parts: &[&str]) -> String {
    let cleaned: String = namespace
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let ns = if cleaned.is_empty() {
        "source"
    } else {
        &cleaned
    };
    // JSON.stringify(parts.map(String))：字符串数组的 JSON 序列化——逐项
    // 走 serde_json 的字符串转义，与 JS JSON.stringify 对 ASCII/控制字符
    // 的转义规则一致（金样锁定）。
    let values: Vec<serde_json::Value> = parts
        .iter()
        .map(|p| serde_json::Value::String((*p).to_string()))
        .collect();
    let json = serde_json::Value::Array(values).to_string();
    format!("{ns}:{}", sha256_hex(json.as_bytes()))
}

/// 现役 buildSessionFileId：`sf_` + sha256hex(JSON.stringify([ownerKey,
/// sourceKey || identityKey]))[..16]。
pub fn build_session_file_id(
    owner_key: &str,
    source_key: Option<&str>,
    identity_key: &str,
) -> String {
    let second = source_key.unwrap_or(identity_key);
    let json = serde_json::Value::Array(vec![
        serde_json::Value::String(owner_key.to_string()),
        serde_json::Value::String(second.to_string()),
    ])
    .to_string();
    let digest = sha256_hex(json.as_bytes());
    format!("sf_{}", &digest[..16])
}

/// 现役 sessionFilesCacheDir：`{home}/session-files/{sha256(ownerKey)[..24]}`。
pub fn session_files_cache_dir(
    home: &Path,
    session_id: Option<&str>,
    session_path: Option<&str>,
) -> PathBuf {
    let owner_key = session_file_owner_key(session_id, session_path);
    let hash = sha256_hex(owner_key.as_bytes());
    home.join("session-files").join(&hash[..24])
}

/// 现役 canonicalFilesystemPathSync + filesystemIdentityKeySync
/// （link-aware-fs.ts:5-22）：realpath 优先，失败回退绝对路径；
/// Windows 追加小写归一。
pub fn canonical_identity_key(path: &Path) -> String {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let canonical = std::fs::canonicalize(&resolved).unwrap_or(resolved);
    let text = canonical.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        text.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        text
    }
}

// ── 本地导入安全闸（file-import-security.ts + path-security.ts） ──

/// 现役 LocalImportPathError 的五值错误码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectError {
    PathInvalid,
    NotFound,
    Symlink,
    PathBlocked,
    TypeUnsupported,
}

impl InspectError {
    pub fn code(self) -> &'static str {
        match self {
            InspectError::PathInvalid => "PATH_INVALID",
            InspectError::NotFound => "NOT_FOUND",
            InspectError::Symlink => "SYMLINK",
            InspectError::PathBlocked => "PATH_BLOCKED",
            InspectError::TypeUnsupported => "TYPE_UNSUPPORTED",
        }
    }
}

impl std::fmt::Display for InspectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for InspectError {}

#[derive(Debug)]
pub struct InspectedPath {
    pub real_path: PathBuf,
    pub kind: &'static str,
    pub size_bytes: u64,
    pub mtime_ms: u64,
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn real_path_or_none(value: &Path) -> Option<PathBuf> {
    let resolved = if value.is_absolute() {
        value.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(value)
    };
    std::fs::canonicalize(resolved).ok()
}

/// 现役 isSensitivePath：相对/不可解析路径按敏感处理；$HOME 下的
/// SENSITIVE_DIRS 与 lingxiHome 一律封锁。
fn is_sensitive_path(resolved: &Path, lingxi_home: Option<&Path>) -> bool {
    let Some(home) = home_dir() else {
        // 无法确定 $HOME 时按敏感处理（现役 realPath 失败 → true 的同构
        // 保守臂）。
        return true;
    };
    for dir in SENSITIVE_DIRS {
        let sensitive = home.join(dir);
        if resolved == sensitive || resolved.starts_with(&sensitive) {
            return true;
        }
    }
    if let Some(lingxi_home) = lingxi_home {
        if let Some(real_home) = real_path_or_none(lingxi_home) {
            if resolved == real_home || resolved.starts_with(&real_home) {
                return true;
            }
        }
    }
    false
}

/// 现役 inspectLocalImportPath（file-import-security.ts:33-74）：
/// 绝对路径校验 → lstat → symlink 拒绝 → realpath → 敏感封锁 →
/// 类型校验（目录仅在 allow_directories 时放行）。
pub fn inspect_local_import_path(
    file_path: &str,
    lingxi_home: Option<&Path>,
    allow_directories: bool,
) -> Result<InspectedPath, InspectError> {
    let input = Path::new(file_path);
    if !input.is_absolute() {
        return Err(InspectError::PathInvalid);
    }
    let input_meta = std::fs::symlink_metadata(input).map_err(|_| InspectError::NotFound)?;
    if input_meta.file_type().is_symlink() {
        return Err(InspectError::Symlink);
    }
    let resolved = std::fs::canonicalize(input).map_err(|_| InspectError::NotFound)?;
    if is_sensitive_path(&resolved, lingxi_home) {
        return Err(InspectError::PathBlocked);
    }
    let meta = std::fs::symlink_metadata(&resolved).map_err(|_| InspectError::NotFound)?;
    if meta.file_type().is_symlink() {
        return Err(InspectError::Symlink);
    }
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if meta.is_file() {
        return Ok(InspectedPath {
            real_path: resolved,
            kind: "file",
            size_bytes: meta.len(),
            mtime_ms,
        });
    }
    if allow_directories && meta.is_dir() {
        return Ok(InspectedPath {
            real_path: resolved,
            kind: "directory",
            size_bytes: 0,
            mtime_ms,
        });
    }
    Err(InspectError::TypeUnsupported)
}

// ── 服务错误 ──

#[derive(Debug)]
pub enum SessionFileError {
    Storage(StorageError),
    Inspect(InspectError),
    Io(std::io::Error),
    /// 现役白名单之外的 blob mime（route 映射 415）。
    UnsupportedMedia(String),
    /// base64 解码失败（route 映射 400）。
    InvalidBase64,
    /// sidecar version 不受支持（现役 loader 只认 version 1）。
    UnsupportedSidecarVersion(i64),
    /// sidecar JSON 损坏。
    BadSidecar(String),
    /// managed cache 目标越出 session-files 根（现役
    /// _assertManagedCacheTarget :693-700 的响亮拒绝）。
    ManagedCacheEscape(String),
    /// fork 身份冲突（现役 forked session file id collision）。
    ForkCollision(String),
}

impl std::fmt::Display for SessionFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionFileError::Storage(err) => write!(f, "storage: {err}"),
            SessionFileError::Inspect(err) => write!(f, "inspect: {err}"),
            SessionFileError::Io(err) => write!(f, "io: {err}"),
            SessionFileError::UnsupportedMedia(mime) => write!(f, "unsupported media: {mime}"),
            SessionFileError::InvalidBase64 => f.write_str("invalid base64 payload"),
            SessionFileError::UnsupportedSidecarVersion(v) => {
                write!(f, "unsupported sidecar version: {v}")
            }
            SessionFileError::BadSidecar(detail) => write!(f, "bad sidecar: {detail}"),
            SessionFileError::ManagedCacheEscape(path) => {
                write!(
                    f,
                    "managed cache file is outside session-files root: {path}"
                )
            }
            SessionFileError::ForkCollision(id) => {
                write!(f, "forked session file id collision: {id}")
            }
        }
    }
}

impl std::error::Error for SessionFileError {}

impl From<StorageError> for SessionFileError {
    fn from(err: StorageError) -> Self {
        SessionFileError::Storage(err)
    }
}

impl From<std::io::Error> for SessionFileError {
    fn from(err: std::io::Error) -> Self {
        SessionFileError::Io(err)
    }
}

// ── 对外视图（serializeSessionFile 的候选子集，camelCase wire） ──

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFileView {
    pub file_id: String,
    pub session_id: String,
    pub file_path: String,
    pub real_path: String,
    pub storage_kind: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub filename: String,
    pub mime: String,
    pub size_bytes: u64,
    pub mtime_ms: u64,
    pub is_directory: bool,
    pub kind: String,
    pub origin: String,
    pub created_at_ms: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub legacy_file_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub legacy_file_paths: Vec<String>,
}

impl SessionFileView {
    fn from_row(row: &SessionFileRow) -> Self {
        SessionFileView {
            file_id: row.file_id.clone(),
            session_id: row.owner_session_id.clone(),
            file_path: row.file_path.clone(),
            real_path: row.real_path.clone(),
            storage_kind: row.storage_kind.wire().to_string(),
            status: row.status.wire().to_string(),
            label: row.label.clone(),
            filename: row.filename.clone(),
            mime: row.mime.clone(),
            size_bytes: row.size_bytes,
            mtime_ms: row.mtime_ms,
            is_directory: row.is_directory,
            kind: row.file_kind.wire().to_string(),
            origin: row.origin.wire().to_string(),
            created_at_ms: row.registered_at_ms,
            legacy_file_ids: row.legacy_file_ids.clone(),
            legacy_file_paths: row.legacy_file_paths.clone(),
        }
    }
}

// ── 报告类型 ──

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOutcome {
    pub expired: Vec<String>,
    pub deleted: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceScan {
    pub identities: Vec<String>,
    pub broken: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    pub checked: usize,
    pub broken: Vec<String>,
}

// ── 服务 ──

pub struct SessionFileService {
    home: PathBuf,
    db: Arc<RunDatabase>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl SessionFileService {
    pub fn new(
        home: PathBuf,
        db: Arc<RunDatabase>,
        now: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Self {
        SessionFileService {
            home,
            db,
            now: Arc::new(now),
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    fn now_ms(&self) -> u64 {
        (self.now)()
    }

    /// route 层安全闸：lingxiHome 默认取服务 home（现役
    /// inspectLocalImportPath({lingxiHome: engine.lingxiHome})）。
    pub fn inspect_local_import(&self, file_path: &str) -> Result<InspectedPath, InspectError> {
        inspect_local_import_path(file_path, None, true)
    }

    pub fn inspect_local_import_with_home(
        &self,
        file_path: &str,
        lingxi_home: Option<&Path>,
    ) -> Result<InspectedPath, InspectError> {
        inspect_local_import_path(file_path, lingxi_home, true)
    }

    fn managed_cache_root(&self) -> PathBuf {
        // 与现役 isInsideRoot(identityKey(file), identityKey(root)) 同口径：
        // 根侧也过 canonical（macOS /tmp → /private/tmp 这类 symlink 宿主
        // 必须对齐，否则 managed 判定整体失效）。home 必定存在（布局在
        // bootstrap 已 prepare），session-files 子目录可能尚未创建——
        // 对 home canonical 后拼接，而不是 canonicalize 不存在的子目录。
        let home = std::fs::canonicalize(&self.home).unwrap_or_else(|_| self.home.clone());
        home.join("session-files")
    }

    /// 现役 registerFile（:74-165）：sourceKey 优先去重，realPath 兜底；
    /// 命中且 materialization 仍在时保留既有 filePath/realPath；新行
    /// 8KB 采样探测 mime；storageKind 按「realPath 是否落在托管缓存根
    /// 内」判定（现役由调用方显式传 managed_cache——upload 路由的 blob
    /// 副本恰好都写进该根，位置判定与现役效果一致；差异台账申报）。
    pub async fn register_file(
        &self,
        session_id: &str,
        real_path: &Path,
        source_key: Option<&str>,
        label: Option<&str>,
        origin: &str,
    ) -> Result<SessionFileView, SessionFileError> {
        let now = self.now_ms();
        let canonical = std::fs::canonicalize(real_path).map_err(|_| {
            SessionFileError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {}", real_path.display()),
            ))
        })?;
        let identity = canonical_identity_key(&canonical);
        let owner_key = session_file_owner_key(Some(session_id), None);

        let existing = match source_key {
            Some(sk) => {
                match store::get_session_file_by_source_key(&self.db, session_id, sk).await? {
                    Some(row) => Some(row),
                    None => {
                        store::get_session_file_by_real_path(
                            &self.db,
                            session_id,
                            &canonical.to_string_lossy(),
                        )
                        .await?
                    }
                }
            }
            None => {
                store::get_session_file_by_real_path(
                    &self.db,
                    session_id,
                    &canonical.to_string_lossy(),
                )
                .await?
            }
        };

        if let Some(existing) = existing {
            // 现役命中语义：保留既有 id；materialization 仍在且 sourceKey
            // 相同而 realPath 不同 → 保留既有路径；status 复活为 available。
            let keep = source_key.is_some()
                && existing.source_key.as_deref() == source_key
                && existing.real_path != canonical.to_string_lossy()
                && Path::new(&existing.real_path).exists();
            if !keep && existing.real_path != canonical.to_string_lossy() {
                store::update_session_file_paths(
                    &self.db,
                    &existing.file_id,
                    &real_path.to_string_lossy(),
                    &canonical.to_string_lossy(),
                    now,
                )
                .await?;
            }
            if existing.status != SessionFileStatus::Available || label.is_some() {
                store::update_session_file_status(
                    &self.db,
                    &existing.file_id,
                    SessionFileStatus::Available,
                    None,
                    now,
                )
                .await?;
            }
            let fresh = store::get_session_file(&self.db, &existing.file_id)
                .await?
                .ok_or_else(|| {
                    SessionFileError::Storage(StorageError::InvalidRequest {
                        detail: format!("session file vanished mid-register: {}", existing.file_id),
                    })
                })?;
            return Ok(SessionFileView::from_row(&fresh));
        }

        let meta = std::fs::metadata(&canonical)?;
        let is_directory = meta.is_dir();
        let filename = canonical
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = filemeta::ext_of_name(&filename);
        let sample = if meta.is_file() {
            read_sample(&canonical)?
        } else {
            Vec::new()
        };
        let mime = if is_directory {
            "inode/directory".to_string()
        } else {
            filemeta::detect_mime(&sample, "application/octet-stream", &filename)
        };
        let kind = filemeta::infer_file_kind(&mime, &ext, is_directory);
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let file_id = build_session_file_id(&owner_key, source_key, &identity);
        let storage_kind = if canonical.starts_with(self.managed_cache_root()) {
            StorageKind::ManagedCache
        } else {
            StorageKind::External
        };
        let row = SessionFileRow {
            file_id: file_id.clone(),
            owner_session_id: session_id.to_string(),
            owner_key,
            source_key: source_key.map(str::to_string),
            identity_key: identity,
            file_path: real_path.to_string_lossy().into_owned(),
            real_path: canonical.to_string_lossy().into_owned(),
            storage_kind,
            status: SessionFileStatus::Available,
            label: Some(label.unwrap_or(&filename).to_string()),
            filename,
            mime,
            size_bytes: if is_directory { 0 } else { meta.len() },
            mtime_ms,
            is_directory,
            file_kind: parse_kind(kind),
            origin: parse_origin(origin),
            registered_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: None,
            legacy_file_ids: Vec::new(),
            legacy_file_paths: Vec::new(),
        };
        store::insert_session_file(&self.db, &row).await?;
        Ok(SessionFileView::from_row(&row))
    }

    /// 现役 blob 上传（upload.ts sourceKeyForUploadBlob :242-250 +
    /// sanitizeBlobName/uniqueUploadName）：白名单外 mime 响亮拒绝；
    /// 内容寻址 sourceKey 去重；副本写进会话托管缓存后走 register_file。
    pub async fn register_blob(
        &self,
        session_id: &str,
        name: Option<&str>,
        mime: &str,
        bytes: &[u8],
        origin: &str,
    ) -> Result<SessionFileView, SessionFileError> {
        if !filemeta::is_allowed_upload_blob_mime(mime) {
            return Err(SessionFileError::UnsupportedMedia(mime.to_string()));
        }
        // 现役 presentation：voice-input 否则 attachment（normalizePresentation
        // + originForPresentation，upload.ts:206-220 的反向）。
        let presentation = if origin == "voice_input" {
            "voice-input"
        } else {
            "attachment"
        };
        let source_key = build_session_file_source_key(
            "upload:blob-content:v1",
            &[presentation, mime, &sha256_hex(bytes)],
        );
        // 现役 existingSessionFileForSourceKey：命中且 target 仍在 → 直接复用。
        if let Some(existing) =
            store::get_session_file_by_source_key(&self.db, session_id, &source_key).await?
        {
            if existing.status != SessionFileStatus::Expired
                && Path::new(&existing.real_path).exists()
            {
                return Ok(SessionFileView::from_row(&existing));
            }
        }
        let sanitized = filemeta::sanitize_blob_name(name, mime);
        let ext = filemeta::ext_of_name(&sanitized);
        let (base, ext_with_dot) = if ext.is_empty() {
            (sanitized.as_str(), String::new())
        } else {
            (
                &sanitized[..sanitized.len() - ext.len() - 1],
                format!(".{ext}"),
            )
        };
        let random = random_hex4()?;
        let unique = filemeta::unique_upload_name(base, &ext_with_dot, self.now_ms(), &random);
        let cache_dir = session_files_cache_dir(&self.home, Some(session_id), None);
        std::fs::create_dir_all(&cache_dir)?;
        let dest = cache_dir.join(unique);
        std::fs::write(&dest, bytes)?;
        self.register_file(session_id, &dest, Some(&source_key), None, origin)
            .await
    }

    /// alias 感知读取（A09 读路径）：sf_ id 直接命中且属本会话 → 行；
    /// 直接命中但属别的会话（fork 后源行仍在）→ 继续 alias 探测，让旧
    /// id 在新会话解析到 fork 行；否则 alias 解析到 canonical 再读。
    pub async fn get_file(
        &self,
        session_id: &str,
        id_or_alias: &str,
    ) -> Result<Option<SessionFileView>, SessionFileError> {
        if let Some(row) = store::get_session_file(&self.db, id_or_alias).await? {
            if row.owner_session_id == session_id {
                return Ok(Some(SessionFileView::from_row(&row)));
            }
        }
        if let Some(target) = store::resolve_alias(&self.db, id_or_alias).await? {
            if let Some(row) = store::get_session_file(&self.db, &target).await? {
                if row.owner_session_id == session_id {
                    return Ok(Some(SessionFileView::from_row(&row)));
                }
            }
        }
        // 跨会话的 id 猜测不泄露存在性（与现役 can_access 同 stance）。
        Ok(None)
    }

    /// 全局按 fileId 解析（R06-T05 资源面）：资源不属于任何单个会话——
    /// 现役 ResourceService._findSessionFileByResourceId
    /// （resource-service.ts :111-126 + :171-217）先让「仍持有该 id 的
    /// 会话」命中，alias 目标兜底；候选人注册表主键即 fileId，直查即
    /// owner 命中，alias 解析到 canonical 行兜底。
    pub async fn get_file_global(
        &self,
        file_id: &str,
    ) -> Result<Option<SessionFileView>, SessionFileError> {
        if let Some(row) = store::get_session_file(&self.db, file_id).await? {
            return Ok(Some(SessionFileView::from_row(&row)));
        }
        if let Some(target) = store::resolve_alias(&self.db, file_id).await? {
            if let Some(row) = store::get_session_file(&self.db, &target).await? {
                return Ok(Some(SessionFileView::from_row(&row)));
            }
        }
        Ok(None)
    }

    pub async fn list_files(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionFileView>, SessionFileError> {
        let rows = store::list_session_files_for_owner(&self.db, session_id).await?;
        Ok(rows.iter().map(SessionFileView::from_row).collect())
    }

    /// 交付物②的迁移适配：读现役 version-1 sidecar，逐行搬进 v9 表，
    /// sf_ id 逐字节保留；legacyFileIds 同步登记别名（A09 无断链）。
    pub async fn import_legacy_sidecar(
        &self,
        session_id: &str,
        sidecar_path: &Path,
    ) -> Result<ImportReport, SessionFileError> {
        let raw = std::fs::read_to_string(sidecar_path)?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|err| SessionFileError::BadSidecar(err.to_string()))?;
        let version = parsed
            .get("version")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(-1);
        if version != 1 {
            return Err(SessionFileError::UnsupportedSidecarVersion(version));
        }
        let now = self.now_ms();
        let owner_key = session_file_owner_key(Some(session_id), None);
        let files = parsed
            .get("files")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| SessionFileError::BadSidecar("missing files map".to_string()))?;
        let mut imported = 0usize;
        let mut skipped = 0usize;
        // 确定性顺序：按 key 排序（JSON 对象序不可靠）。
        let mut keys: Vec<&String> = files.keys().collect();
        keys.sort();
        for key in keys {
            let value = &files[key];
            let Some(row) = self.map_legacy_row(session_id, &owner_key, value, now) else {
                skipped += 1;
                continue;
            };
            if store::get_session_file(&self.db, &row.file_id)
                .await?
                .is_some()
            {
                skipped += 1;
                continue;
            }
            let legacy_ids = row.legacy_file_ids.clone();
            store::insert_session_file(&self.db, &row).await?;
            for legacy in legacy_ids {
                // 别名冲突不致命：现役语义是「旧 id 仍可解析」，后到的
                // 同别名词条跳过（保留先登记者的指向）。
                if store::resolve_alias(&self.db, &legacy).await?.is_none() {
                    store::insert_alias(&self.db, &legacy, &row.file_id, now).await?;
                }
            }
            imported += 1;
        }
        // 现役 sidecar refs（注册操作日志）→ 候选 import 引用行。
        if let Some(refs) = parsed.get("refs").and_then(serde_json::Value::as_array) {
            for refr in refs {
                let Some(file_id) = refr.get("fileId").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                if store::get_session_file(&self.db, file_id).await?.is_none() {
                    continue;
                }
                let created = refr
                    .get("createdAt")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(now);
                store::insert_ref(
                    &self.db,
                    &store::SessionFileRefRow {
                        file_id: file_id.to_string(),
                        session_id: session_id.to_string(),
                        message_id: None,
                        ref_kind: RefKind::Import,
                        created_at_ms: created,
                    },
                )
                .await?;
            }
        }
        Ok(ImportReport { imported, skipped })
    }

    fn map_legacy_row(
        &self,
        session_id: &str,
        owner_key: &str,
        value: &serde_json::Value,
        now: u64,
    ) -> Option<SessionFileRow> {
        let file_id = value.get("fileId").and_then(serde_json::Value::as_str)?;
        let real_path = value.get("realPath").and_then(serde_json::Value::as_str)?;
        if file_id.is_empty() || real_path.is_empty() {
            return None;
        }
        let file_path = value
            .get("filePath")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(real_path);
        let filename = value
            .get("filename")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                Path::new(file_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        let is_directory = value
            .get("isDirectory")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let mime = value
            .get("mime")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("application/octet-stream");
        let ext = filemeta::ext_of_name(&filename);
        let kind = value
            .get("fileKind")
            .and_then(serde_json::Value::as_str)
            .map(parse_kind)
            .unwrap_or_else(|| parse_kind(filemeta::infer_file_kind(mime, &ext, is_directory)));
        let origin = value
            .get("origin")
            .and_then(serde_json::Value::as_str)
            .map(parse_origin)
            .unwrap_or(SessionFileOrigin::Unknown);
        let storage_kind = match value.get("storageKind").and_then(serde_json::Value::as_str) {
            Some("managed_cache") => StorageKind::ManagedCache,
            _ => StorageKind::External,
        };
        let status = match value.get("status").and_then(serde_json::Value::as_str) {
            Some("expired") => SessionFileStatus::Expired,
            Some("missing") => SessionFileStatus::Missing,
            _ => SessionFileStatus::Available,
        };
        let legacy_file_ids = string_list(value.get("legacyFileIds"));
        let legacy_file_paths = string_list(value.get("legacyFilePaths"));
        let registered = value
            .get("createdAt")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(now);
        let updated = value
            .get("updatedAt")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(registered);
        Some(SessionFileRow {
            file_id: file_id.to_string(),
            owner_session_id: session_id.to_string(),
            owner_key: owner_key.to_string(),
            source_key: value
                .get("sourceKey")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            identity_key: canonical_identity_key(Path::new(real_path)),
            file_path: file_path.to_string(),
            real_path: real_path.to_string(),
            storage_kind,
            status,
            label: value
                .get("label")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            filename,
            mime: mime.to_string(),
            size_bytes: value
                .get("size")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            mtime_ms: value
                .get("mtimeMs")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            is_directory,
            file_kind: kind,
            origin,
            registered_at_ms: registered,
            updated_at_ms: updated,
            expires_at_ms: value.get("missingAt").and_then(serde_json::Value::as_u64),
            legacy_file_ids,
            legacy_file_paths,
        })
    }

    /// 现役 forkSessionFiles（:255-360）：仅「保留引用可达」的文件被复制；
    /// managed payload 复制进目标会话缓存（uniqueManagedForkName 碰撞
    /// 追加 `-{n}-`）；新 id 由目标 ownerKey 派生；旧 id 与旧路径累积进
    /// legacy 列表并登记别名。
    pub async fn fork_session_files(
        &self,
        source_session_id: &str,
        target_session_id: &str,
        retained_identities: &[String],
    ) -> Result<Vec<String>, SessionFileError> {
        let retained: HashSet<&str> = retained_identities.iter().map(String::as_str).collect();
        let source_files = store::list_session_files_for_owner(&self.db, source_session_id).await?;
        let now = self.now_ms();
        let target_owner_key = session_file_owner_key(Some(target_session_id), None);
        let target_cache_dir = session_files_cache_dir(&self.home, Some(target_session_id), None);
        let mut used_names: HashSet<String> = HashSet::new();
        let mut new_ids = Vec::new();
        for source in &source_files {
            let mut identities: Vec<&str> = vec![
                source.file_id.as_str(),
                source.file_path.as_str(),
                source.real_path.as_str(),
            ];
            identities.extend(source.legacy_file_ids.iter().map(String::as_str));
            identities.extend(source.legacy_file_paths.iter().map(String::as_str));
            if !identities.iter().any(|id| retained.contains(id)) {
                continue;
            }
            let (child_file_path, child_real_path) =
                if source.storage_kind == StorageKind::ManagedCache {
                    let managed_name = unique_managed_fork_name(source, &mut used_names);
                    let dest = target_cache_dir.join(&managed_name);
                    std::fs::create_dir_all(&target_cache_dir)?;
                    if source.status == SessionFileStatus::Available
                        && Path::new(&source.real_path).exists()
                    {
                        copy_payload(Path::new(&source.real_path), &dest, source.is_directory)?;
                    }
                    (
                        dest.to_string_lossy().into_owned(),
                        dest.to_string_lossy().into_owned(),
                    )
                } else {
                    (source.file_path.clone(), source.real_path.clone())
                };
            let identity = canonical_identity_key(Path::new(&child_real_path));
            let child_id =
                build_session_file_id(&target_owner_key, source.source_key.as_deref(), &identity);
            if store::get_session_file(&self.db, &child_id)
                .await?
                .is_some()
            {
                return Err(SessionFileError::ForkCollision(child_id));
            }
            let mut legacy_ids = source.legacy_file_ids.clone();
            if !legacy_ids.contains(&source.file_id) {
                legacy_ids.push(source.file_id.clone());
            }
            legacy_ids.retain(|id| id != &child_id);
            let mut legacy_paths = source.legacy_file_paths.clone();
            for p in [&source.file_path, &source.real_path] {
                if !legacy_paths.contains(p) {
                    legacy_paths.push(p.clone());
                }
            }
            legacy_paths.retain(|p| p != &child_file_path && p != &child_real_path);
            let row = SessionFileRow {
                file_id: child_id.clone(),
                owner_session_id: target_session_id.to_string(),
                owner_key: target_owner_key.clone(),
                source_key: source.source_key.clone(),
                identity_key: identity,
                file_path: child_file_path,
                real_path: child_real_path,
                storage_kind: source.storage_kind,
                status: source.status,
                label: source.label.clone(),
                filename: source.filename.clone(),
                mime: source.mime.clone(),
                size_bytes: source.size_bytes,
                mtime_ms: source.mtime_ms,
                is_directory: source.is_directory,
                file_kind: source.file_kind,
                origin: source.origin,
                registered_at_ms: now,
                updated_at_ms: now,
                expires_at_ms: source.expires_at_ms,
                legacy_file_ids: legacy_ids.clone(),
                legacy_file_paths: legacy_paths,
            };
            store::insert_session_file(&self.db, &row).await?;
            // 旧 id 全链登记别名（A09：fork 前的历史引用无断链）。
            for legacy in &legacy_ids {
                if store::resolve_alias(&self.db, legacy).await?.is_none() {
                    store::insert_alias(&self.db, legacy, &child_id, now).await?;
                }
            }
            store::insert_ref(
                &self.db,
                &store::SessionFileRefRow {
                    file_id: child_id.clone(),
                    session_id: target_session_id.to_string(),
                    message_id: None,
                    ref_kind: RefKind::Import,
                    created_at_ms: now,
                },
            )
            .await?;
            new_ids.push(child_id);
        }
        Ok(new_ids)
    }

    /// 冷缓存清理（cleanupColdSessionFiles :517-571 的全量扫描形态 +
    /// 任务书第 4 条跨会话引用保护）：冷会话集合 → 逐文件（仅
    /// managed_cache 且非 expired）→ 被任何「非冷」会话引用的跳过 →
    /// managed-cache 根守卫 → 递归删除 → 行转 expired（expires_at_ms
    /// 承载现役 missingAt）。
    pub async fn cleanup_cold_sessions(&self) -> Result<CleanupOutcome, SessionFileError> {
        let now = self.now_ms();
        let cutoff = now.saturating_sub(SESSION_FILE_CACHE_INACTIVE_TTL_MS);
        let cold = store::list_cold_session_ids(&self.db, cutoff).await?;
        let cold_set: HashSet<&str> = cold.iter().map(String::as_str).collect();
        let root = self.managed_cache_root();
        let mut expired = Vec::new();
        let mut deleted = 0usize;
        for session_id in &cold {
            let files = store::list_session_files_for_owner(&self.db, session_id).await?;
            for file in &files {
                if file.storage_kind != StorageKind::ManagedCache
                    || file.status == SessionFileStatus::Expired
                {
                    continue;
                }
                // 任务书第 4 条：被存活（非冷）会话引用的交付物不得回收。
                let refs = store::list_refs_for_file(&self.db, &file.file_id).await?;
                if refs
                    .iter()
                    .any(|r| !cold_set.contains(r.session_id.as_str()))
                {
                    continue;
                }
                let target = if file.real_path.is_empty() {
                    file.file_path.clone()
                } else {
                    file.real_path.clone()
                };
                // 现役 _assertManagedCacheTarget（:693-700）：目标必须落在
                // session-files 根内（组件级包含，与 isInsideRoot 同口径），
                // 否则响亮拒绝整个清理（不静默跳过）。
                let canonical_target = canonical_identity_key(Path::new(&target));
                let canonical_root = canonical_identity_key(&root);
                if !Path::new(&canonical_target).starts_with(Path::new(&canonical_root)) {
                    return Err(SessionFileError::ManagedCacheEscape(target));
                }
                let target_path = Path::new(&target);
                if target_path.exists() {
                    if target_path.is_dir() {
                        std::fs::remove_dir_all(target_path)?;
                    } else {
                        std::fs::remove_file(target_path)?;
                    }
                    deleted += 1;
                }
                store::update_session_file_status(
                    &self.db,
                    &file.file_id,
                    SessionFileStatus::Expired,
                    Some(now),
                    now,
                )
                .await?;
                expired.push(file.file_id.clone());
            }
        }
        Ok(CleanupOutcome { expired, deleted })
    }

    /// 冷度写入（差异台账 D3 配套）：会话最近一次活动时刻。
    pub async fn mark_session_activity(
        &self,
        session_id: &str,
        last_activity_unix_ms: u64,
    ) -> Result<(), SessionFileError> {
        store::upsert_session_activity(&self.db, session_id, last_activity_unix_ms).await?;
        Ok(())
    }

    pub async fn insert_reference(
        &self,
        session_id: &str,
        file_id: &str,
        message_id: Option<&str>,
        kind: &str,
    ) -> Result<(), SessionFileError> {
        let ref_kind = match kind {
            "message_marker" => RefKind::MessageMarker,
            "media_marker" => RefKind::MediaMarker,
            "typed_object" => RefKind::TypedObject,
            "stage" => RefKind::Stage,
            "upload" => RefKind::Upload,
            "import" => RefKind::Import,
            "restore" => RefKind::Restore,
            other => {
                return Err(SessionFileError::Storage(StorageError::InvalidRequest {
                    detail: format!("unknown ref kind {other:?}"),
                }))
            }
        };
        store::insert_ref(
            &self.db,
            &store::SessionFileRefRow {
                file_id: file_id.to_string(),
                session_id: session_id.to_string(),
                message_id: message_id.map(str::to_string),
                ref_kind,
                created_at_ms: self.now_ms(),
            },
        )
        .await?;
        Ok(())
    }

    pub async fn references_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<store::SessionFileRefRow>, SessionFileError> {
        Ok(store::list_refs_for_session(&self.db, session_id).await?)
    }

    /// 交付物③的注册侧：collectSessionFileReferenceIdentities
    /// （:997-1021）——结构化值递归 + 文本标记扫描；解析不到注册行的
    /// 身份进 broken（点名，不静默）。解析成功的身份落成引用行
    /// （message_marker），供 A10 跨会话保护消费。
    pub async fn collect_reference_identities(
        &self,
        session_id: &str,
        messages: &[serde_json::Value],
    ) -> Result<ReferenceScan, SessionFileError> {
        let mut ordered: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for message in messages {
            collect_identities(message, &mut ordered, &mut seen);
        }
        let known = store::list_session_files_for_owner(&self.db, session_id).await?;
        let mut broken = Vec::new();
        let existing_refs = store::list_refs_for_session(&self.db, session_id).await?;
        let referenced: HashSet<&str> = existing_refs.iter().map(|r| r.file_id.as_str()).collect();
        for identity in &ordered {
            match self.resolve_identity(session_id, identity, &known).await? {
                Some(file_id) => {
                    if !referenced.contains(file_id.as_str()) {
                        store::insert_ref(
                            &self.db,
                            &store::SessionFileRefRow {
                                file_id: file_id.clone(),
                                session_id: session_id.to_string(),
                                message_id: None,
                                ref_kind: RefKind::MessageMarker,
                                created_at_ms: self.now_ms(),
                            },
                        )
                        .await?;
                    }
                }
                None => broken.push(identity.clone()),
            }
        }
        Ok(ReferenceScan {
            identities: ordered,
            broken,
        })
    }

    async fn resolve_identity(
        &self,
        session_id: &str,
        identity: &str,
        known: &[SessionFileRow],
    ) -> Result<Option<String>, SessionFileError> {
        if let Some(rest) = identity.strip_prefix("sf_") {
            let _ = rest;
            if let Some(row) = store::get_session_file(&self.db, identity).await? {
                if row.owner_session_id == session_id {
                    return Ok(Some(row.file_id));
                }
            }
            if let Some(target) = store::resolve_alias(&self.db, identity).await? {
                if let Some(row) = store::get_session_file(&self.db, &target).await? {
                    if row.owner_session_id == session_id {
                        return Ok(Some(row.file_id));
                    }
                }
            }
            return Ok(None);
        }
        // 路径身份：file_path / real_path / legacy 路径匹配（现役
        // sessionFileIsReachable 的身份全集语义）。
        for row in known {
            if row.file_path == identity
                || row.real_path == identity
                || row.legacy_file_paths.iter().any(|p| p == identity)
            {
                return Ok(Some(row.file_id.clone()));
            }
        }
        Ok(None)
    }

    /// 完整性报告：逐文件核实 realPath 仍在盘上；缺失者点名（A09
    /// 「无断链」的探测面）。
    pub async fn integrity_report(
        &self,
        session_id: &str,
    ) -> Result<IntegrityReport, SessionFileError> {
        let rows = store::list_session_files_for_owner(&self.db, session_id).await?;
        let mut broken = Vec::new();
        for row in &rows {
            if row.status == SessionFileStatus::Expired {
                continue;
            }
            if !Path::new(&row.real_path).exists() {
                broken.push(row.file_id.clone());
            }
        }
        Ok(IntegrityReport {
            checked: rows.len(),
            broken,
        })
    }
}

// ── 模块内纯函数 ──

fn read_sample(path: &Path) -> Result<Vec<u8>, std::io::Error> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; MIME_SAMPLE_BYTES];
    let n = file.read(&mut buf)?;
    buf.truncate(n);
    Ok(buf)
}

fn parse_kind(wire: &str) -> FileKind {
    match wire {
        "directory" => FileKind::Directory,
        "image" => FileKind::Image,
        "video" => FileKind::Video,
        "audio" => FileKind::Audio,
        "document" => FileKind::Document,
        _ => FileKind::Unknown,
    }
}

fn parse_origin(wire: &str) -> SessionFileOrigin {
    match wire {
        "stage_files" => SessionFileOrigin::StageFiles,
        "user_upload" => SessionFileOrigin::UserUpload,
        "user_attachment" => SessionFileOrigin::UserAttachment,
        "bridge_inbound" => SessionFileOrigin::BridgeInbound,
        "agent_write" => SessionFileOrigin::AgentWrite,
        "agent_artifact" => SessionFileOrigin::AgentArtifact,
        "plugin_output" => SessionFileOrigin::PluginOutput,
        "install_skill_output" => SessionFileOrigin::InstallSkillOutput,
        "agent_edit" => SessionFileOrigin::AgentEdit,
        "browser_screenshot" => SessionFileOrigin::BrowserScreenshot,
        "skill_install_source" => SessionFileOrigin::SkillInstallSource,
        "plugin_install_source" => SessionFileOrigin::PluginInstallSource,
        "bridge_manual_send" => SessionFileOrigin::BridgeManualSend,
        "voice_input" => SessionFileOrigin::VoiceInput,
        "session_fork" => SessionFileOrigin::SessionFork,
        _ => SessionFileOrigin::Unknown,
    }
}

fn string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 现役 uniqueManagedForkName（registry :1049-1063）：`{sourceId}-{basename}`，
/// 碰撞时 `{sourceId}-{n}-{basename}`（n 从 2 起）。
fn unique_managed_fork_name(source: &SessionFileRow, used: &mut HashSet<String>) -> String {
    let source_id: String = source
        .file_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let source_id = if source_id.is_empty() {
        "session-file"
    } else {
        &source_id
    };
    let basename = Path::new(&source.file_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "file".to_string());
    let basename: String = basename
        .chars()
        .rev()
        .take(160)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let initial = format!("{source_id}-{basename}");
    let mut candidate = initial.clone();
    let mut suffix = 2u32;
    while used.contains(&candidate) {
        candidate = format!("{source_id}-{suffix}-{basename}");
        suffix += 1;
    }
    used.insert(candidate.clone());
    candidate
}

fn copy_payload(source: &Path, dest: &Path, is_directory: bool) -> Result<(), std::io::Error> {
    if is_directory {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let child_dest = dest.join(entry.file_name());
            let child_type = entry.file_type()?;
            if child_type.is_dir() {
                copy_payload(&entry.path(), &child_dest, true)?;
            } else if child_type.is_file() {
                std::fs::copy(entry.path(), &child_dest)?;
            }
            // symlink 等其它类型不复制（安全闸同 stance：不跟随）。
        }
        Ok(())
    } else {
        std::fs::copy(source, dest).map(|_| ())
    }
}

fn random_hex4() -> Result<String, SessionFileError> {
    let mut buf = [0u8; 4];
    getrandom::getrandom(&mut buf).map_err(|err| {
        SessionFileError::Io(std::io::Error::other(format!(
            "system secure random source failed: {err}"
        )))
    })?;
    let mut out = String::with_capacity(8);
    for b in buf {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
    }
    Ok(out)
}

// ── 标记与结构化值收集（registry :944-1021） ──

fn add_identity(ordered: &mut Vec<String>, seen: &mut HashSet<String>, value: &str) {
    let trimmed = value.trim();
    if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
        ordered.push(trimmed.to_string());
    }
}

fn collect_identities(
    value: &serde_json::Value,
    ordered: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    match value {
        serde_json::Value::String(text) => collect_from_text(text, ordered, seen),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_identities(item, ordered, seen);
            }
        }
        serde_json::Value::Object(map) => {
            collect_from_object(map, ordered, seen);
            for item in map.values() {
                collect_identities(item, ordered, seen);
            }
        }
        _ => {}
    }
}

fn collect_from_object(
    map: &serde_json::Map<String, serde_json::Value>,
    ordered: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    let get_str = |key: &str| map.get(key).and_then(serde_json::Value::as_str);
    let explicit_type = matches!(get_str("type"), Some("session_file") | Some("session-file"))
        || matches!(get_str("kind"), Some("session_file") | Some("session-file"));
    let file_id = get_str("fileId").map(str::trim).filter(|s| !s.is_empty());
    if let Some(file_id) = file_id {
        add_identity(ordered, seen, file_id);
    }
    if explicit_type && file_id.is_none() {
        if let Some(id) = get_str("id") {
            add_identity(ordered, seen, id);
        }
    }
    let has_path = get_str("filePath").is_some() || get_str("realPath").is_some();
    if file_id.is_some() || explicit_type || has_path {
        if let Some(p) = get_str("filePath") {
            add_identity(ordered, seen, p);
        }
        if let Some(p) = get_str("realPath") {
            add_identity(ordered, seen, p);
        }
        if file_id.is_some() || explicit_type {
            if let Some(p) = get_str("path") {
                add_identity(ordered, seen, p);
            }
        }
    }
}

/// 文本标记扫描（SESSION_FILE_MARKER_RE / ATTACHED_MEDIA_MARKER_RE 的
/// 手写等价，无 regex 依赖）：
/// - `[SessionFile]\s+(\{[^\r\n]*\})`：`{` 到本行最后一个 `}`，JSON 解析
///   失败按现役 catch 静默跳过（可见文本不是授权引用）。
/// - `\[attached_(?:image|video|audio):\s*([^\]]+)\]`：捕获到 `]` 前。
fn collect_from_text(text: &str, ordered: &mut Vec<String>, seen: &mut HashSet<String>) {
    let has_session_file_marker = text.contains("[SessionFile]");
    if has_session_file_marker {
        let mut rest = text;
        while let Some(pos) = rest.find("[SessionFile]") {
            rest = &rest[pos + "[SessionFile]".len()..];
            let trimmed_start = rest.len() - rest.trim_start().len();
            rest = &rest[trimmed_start..];
            if !rest.starts_with('{') {
                continue;
            }
            // \{[^\r\n]*\} 贪婪：本行内最后一个 }。
            let line_end = rest.find(['\r', '\n']).unwrap_or(rest.len());
            let line = &rest[..line_end];
            if let Some(close) = line.rfind('}') {
                let candidate = &line[..=close];
                if let Ok(serde_json::Value::Object(map)) =
                    serde_json::from_str::<serde_json::Value>(candidate)
                {
                    collect_from_object(&map, ordered, seen);
                }
                rest = &rest[close + 1..];
            } else {
                break;
            }
        }
    }
    // 现役：hasSessionFileMarker || hasAttachedMediaMarker 时扫媒体标记
    // （含 [SessionFile] 的文本也会扫 attached_）。
    if has_session_file_marker || text.contains("[attached_") {
        let mut rest = text;
        while let Some(pos) = rest.find("[attached_") {
            rest = &rest[pos + "[attached_".len()..];
            let kind_ok = rest.starts_with("image:")
                || rest.starts_with("video:")
                || rest.starts_with("audio:");
            if !kind_ok {
                continue;
            }
            let Some(colon) = rest.find(':') else {
                break;
            };
            rest = &rest[colon + 1..];
            let skipped = rest.len() - rest.trim_start().len();
            rest = &rest[skipped..];
            match rest.find(']') {
                Some(end) => {
                    let captured = rest[..end].to_string();
                    add_identity(ordered, seen, &captured);
                    rest = &rest[end + 1..];
                }
                None => break,
            }
        }
    }
}

// ── 路由 handler（history.rs 模式：归属闸先于一切实体读取） ──

use axum::extract::{Path as AxumPath, State};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::{EndpointError, Principal, ServiceState};

async fn gate_session(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
) -> Option<Response> {
    match state.sessions().get_for(principal, session_id).await {
        Ok(crate::sessions::SessionAccess::Ok(_)) => None,
        Ok(crate::sessions::SessionAccess::NotFound) => {
            Some(EndpointError::not_found().into_response())
        }
        Ok(crate::sessions::SessionAccess::Forbidden) => {
            Some(EndpointError::forbidden("cross_principal_access").into_response())
        }
        Err(err) => Some(EndpointError::storage(&err).into_response()),
    }
}

fn service_error_response(err: SessionFileError) -> Response {
    match err {
        SessionFileError::Storage(err) => EndpointError::storage(&err).into_response(),
        SessionFileError::Inspect(inspect) => match inspect {
            InspectError::PathInvalid => {
                EndpointError::invalid_message("PATH_INVALID: path must be absolute")
                    .into_response()
            }
            InspectError::NotFound => EndpointError::not_found().into_response(),
            InspectError::Symlink => {
                EndpointError::invalid_message("SYMLINK: symlink not allowed").into_response()
            }
            InspectError::PathBlocked => EndpointError::forbidden("PATH_BLOCKED").into_response(),
            InspectError::TypeUnsupported => EndpointError::invalid_message(
                "TYPE_UNSUPPORTED: regular file or directory required",
            )
            .into_response(),
        },
        SessionFileError::Io(err) if err.kind() == std::io::ErrorKind::NotFound => {
            EndpointError::not_found().into_response()
        }
        SessionFileError::Io(err) => EndpointError::storage(&StorageError::InvalidRequest {
            detail: format!("io: {err}"),
        })
        .into_response(),
        SessionFileError::UnsupportedMedia(mime) => {
            EndpointError::unsupported_media_type(format!("unsupported media type: {mime}"))
                .into_response()
        }
        SessionFileError::InvalidBase64 => {
            EndpointError::invalid_message("dataBase64 is not valid base64").into_response()
        }
        SessionFileError::UnsupportedSidecarVersion(version) => {
            EndpointError::invalid_message(format!("unsupported sidecar version: {version}"))
                .into_response()
        }
        SessionFileError::BadSidecar(detail) => {
            EndpointError::invalid_message(format!("bad sidecar: {detail}")).into_response()
        }
        SessionFileError::ManagedCacheEscape(path) => {
            EndpointError::storage(&StorageError::InvalidRequest {
                detail: format!("managed cache file is outside session-files root: {path}"),
            })
            .into_response()
        }
        SessionFileError::ForkCollision(id) => {
            EndpointError::storage(&StorageError::InvalidRequest {
                detail: format!("forked session file id collision: {id}"),
            })
            .into_response()
        }
    }
}

/// `GET /lingxi/v1/sessions/{session_id}/files` — 会话文件列表（A09 读
/// 路径之一；按注册序）。
pub async fn list_session_files_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(session_id): AxumPath<String>,
) -> Response {
    if let Some(denial) = gate_session(&state, &principal, &session_id).await {
        return denial;
    }
    match state.session_files().list_files(&session_id).await {
        Ok(files) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({ "files": files })),
        )
            .into_response(),
        Err(err) => service_error_response(err),
    }
}

/// `GET /lingxi/v1/sessions/{session_id}/files/{file_id}` — 单件读取
/// （alias 感知：fork/导入前的旧 sf_ id 仍可解析，A09 无断链）。
pub async fn get_session_file_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath((session_id, file_id)): AxumPath<(String, String)>,
) -> Response {
    if let Some(denial) = gate_session(&state, &principal, &session_id).await {
        return denial;
    }
    match state.session_files().get_file(&session_id, &file_id).await {
        Ok(Some(file)) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({ "file": file })),
        )
            .into_response(),
        Ok(None) => EndpointError::not_found().into_response(),
        Err(err) => service_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportLegacyBody {
    pub sidecar_path: String,
}

/// `POST /lingxi/v1/sessions/{session_id}/files/import-legacy` — 交付物②
/// 的迁移适配入口：读现役 version-1 sidecar，sf_ 身份逐字节保留。
pub async fn import_legacy_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(session_id): AxumPath<String>,
    body: Result<Json<ImportLegacyBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(denial) = gate_session(&state, &principal, &session_id).await {
        return denial;
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    let sidecar = PathBuf::from(&body.sidecar_path);
    if !sidecar.is_absolute() {
        return EndpointError::invalid_message("sidecarPath must be absolute").into_response();
    }
    match state
        .session_files()
        .import_legacy_sidecar(&session_id, &sidecar)
        .await
    {
        Ok(report) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "imported": report.imported,
                "skipped": report.skipped,
            })),
        )
            .into_response(),
        Err(err) => service_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachLocalBody {
    pub path: String,
    pub label: Option<String>,
    pub origin: Option<String>,
}

/// `POST /lingxi/v1/sessions/{session_id}/attachments/local` — 本地路径
/// 附件：安全闸（inspectLocalImportPath）→ sourceKeyForUploadPath 去重 →
/// 注册。external 引用，不复制字节。
pub async fn attach_local_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(session_id): AxumPath<String>,
    body: Result<Json<AttachLocalBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(denial) = gate_session(&state, &principal, &session_id).await {
        return denial;
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    let home = state.session_files().home().to_path_buf();
    let inspected = match state
        .session_files()
        .inspect_local_import_with_home(&body.path, Some(&home))
    {
        Ok(inspected) => inspected,
        Err(err) => return service_error_response(SessionFileError::Inspect(err)),
    };
    // 现役 sourceKeyForUploadPath（upload.ts:226-233）。
    let source_key = build_session_file_source_key(
        "upload:path:v1",
        &[
            &inspected.real_path.to_string_lossy(),
            inspected.kind,
            &inspected.size_bytes.to_string(),
            &inspected.mtime_ms.to_string(),
        ],
    );
    match state
        .session_files()
        .register_file(
            &session_id,
            &inspected.real_path,
            Some(&source_key),
            body.label.as_deref(),
            body.origin.as_deref().unwrap_or("user_upload"),
        )
        .await
    {
        Ok(file) => (
            axum::http::StatusCode::CREATED,
            Json(serde_json::json!({ "file": file })),
        )
            .into_response(),
        Err(err) => service_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachBlobBody {
    pub name: Option<String>,
    pub mime: String,
    pub data_base64: String,
    pub origin: Option<String>,
}

/// `POST /lingxi/v1/sessions/{session_id}/attachments/blob` — 内存附件：
/// mime 白名单（415）→ base64 长度上限（413）→ 解码（400）→ 托管缓存
/// 副本 + 内容寻址注册。
pub async fn attach_blob_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(session_id): AxumPath<String>,
    body: Result<Json<AttachBlobBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Some(denial) = gate_session(&state, &principal, &session_id).await {
        return denial;
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    if !filemeta::is_allowed_upload_blob_mime(&body.mime) {
        return service_error_response(SessionFileError::UnsupportedMedia(body.mime.clone()));
    }
    // 现役 isUploadBlobBase64WithinLimit：按 mime 族限量，超限 413。
    if !filemeta::upload_blob_base64_within_limit(&body.data_base64, &body.mime) {
        return EndpointError::payload_too_large(format!(
            "dataBase64 exceeds {} chars for {}",
            filemeta::upload_blob_max_base64_chars(&body.mime),
            body.mime
        ))
        .into_response();
    }
    use base64::Engine as _;
    let bytes = match base64::engine::general_purpose::STANDARD.decode(&body.data_base64) {
        Ok(bytes) => bytes,
        Err(_) => return service_error_response(SessionFileError::InvalidBase64),
    };
    // 现役 upload.ts:470-480：空 blob 与「video mime 但字节不兼容」都是
    // 响亮拒绝（内容伪装防护）。
    if bytes.is_empty() {
        return EndpointError::invalid_message("empty blob").into_response();
    }
    if filemeta::is_allowed_chat_video_mime(&body.mime)
        && !filemeta::is_chat_video_bytes_compatible(&bytes, &body.mime)
    {
        return EndpointError::invalid_message("video content does not match mimeType")
            .into_response();
    }
    let origin = body.origin.as_deref().unwrap_or("user_upload");
    // 现役：voice-input presentation 要求 audio mime（upload.ts:467-470）。
    if origin == "voice_input" && !filemeta::is_allowed_upload_audio_mime(&body.mime) {
        return EndpointError::invalid_message("voice-input requires audio mimeType")
            .into_response();
    }
    match state
        .session_files()
        .register_blob(
            &session_id,
            body.name.as_deref(),
            &body.mime,
            &bytes,
            origin,
        )
        .await
    {
        Ok(file) => (
            axum::http::StatusCode::CREATED,
            Json(serde_json::json!({ "file": file })),
        )
            .into_response(),
        Err(err) => service_error_response(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_markers_match_incumbent_regex_semantics() {
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        collect_from_text(
            "看 [SessionFile] {\"fileId\":\"sf_aaaabbbbccccdddd\"} 和 [attached_video: /v/mv.mp4]",
            &mut ordered,
            &mut seen,
        );
        assert_eq!(
            ordered,
            vec!["sf_aaaabbbbccccdddd".to_string(), "/v/mv.mp4".to_string()]
        );
        // 损坏 JSON 静默跳过（现役 catch 臂）。
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        collect_from_text("[SessionFile] {not-json}", &mut ordered, &mut seen);
        assert!(ordered.is_empty());
        // 非媒体种类的 attached_ 不识别。
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        collect_from_text("[attached_doc: /d.txt]", &mut ordered, &mut seen);
        assert!(ordered.is_empty());
    }

    #[test]
    fn typed_object_collection_matches_incumbent() {
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        collect_identities(
            &serde_json::json!({
                "type": "session_file",
                "fileId": "sf_1111222233334444",
                "filePath": "/shown/a.png",
                "realPath": "/real/a.png",
                "nested": {"id": "ignored-without-type"}
            }),
            &mut ordered,
            &mut seen,
        );
        assert!(ordered.contains(&"sf_1111222233334444".to_string()));
        assert!(ordered.contains(&"/shown/a.png".to_string()));
        assert!(ordered.contains(&"/real/a.png".to_string()));
        assert!(!ordered.contains(&"ignored-without-type".to_string()));
    }

    #[test]
    fn fork_name_collision_suffixes() {
        let mut used = HashSet::new();
        let row = SessionFileRow {
            file_id: "sf_src0000000000000".to_string(),
            owner_session_id: "s".to_string(),
            owner_key: "id:s".to_string(),
            source_key: None,
            identity_key: "/i".to_string(),
            file_path: "/cache/photo.png".to_string(),
            real_path: "/cache/photo.png".to_string(),
            storage_kind: StorageKind::ManagedCache,
            status: SessionFileStatus::Available,
            label: None,
            filename: "photo.png".to_string(),
            mime: "image/png".to_string(),
            size_bytes: 1,
            mtime_ms: 1,
            is_directory: false,
            file_kind: FileKind::Image,
            origin: SessionFileOrigin::UserUpload,
            registered_at_ms: 1,
            updated_at_ms: 1,
            expires_at_ms: None,
            legacy_file_ids: vec![],
            legacy_file_paths: vec![],
        };
        let first = unique_managed_fork_name(&row, &mut used);
        assert_eq!(first, "sf_src0000000000000-photo.png");
        let second = unique_managed_fork_name(&row, &mut used);
        assert_eq!(second, "sf_src0000000000000-2-photo.png");
    }
}
