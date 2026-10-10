//! SessionFileStore — R06-T05：会话文件（sf_ 身份）、旧 id 别名、显式引用
//! 与各工作区文件历史快照的 SQLite 存取（v9 表）。
//!
//! 对照现役（RC-3）：
//! - 行字段集 = `lib/session-files/session-file-registry.ts` sidecar
//!   （`{sessionPath}.files.json`，version 1）中 files map value 的字段集，
//!   加上 fork/导入产生的 legacyFileIds/legacyFilePaths（差异台账 D1：
//!   sidecar JSON → DB 表；sf_ id 逐字节保留）。
//! - `session_file_aliases` 是现役 `legacyFileIds` 数组的结构化：fork/导入
//!   前的旧 sf_ id 仍可解析到现行行（R06-A09 无断链）。
//! - `file_history_snapshots` 对应 `lib/file-history/history-store.ts`：
//!   workspace_hash 分桶 + rel_path 版本序列；仅文本策略文件（service 层
//!   执行策略，本层如实存取）。
//!
//! 本模块只做存取与约束，不做策略：注册去重由唯一索引兜底
//! （同 owner 同 source_key 响亮冲突），冷度/保留策略在 service 层。

use lingxi_kernel::ports::StorageError;
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

use super::migrations;
use super::run_store::RunDatabase;

fn map_err(err: rusqlite::Error) -> StorageError {
    migrations::map_rusqlite(err)
}

fn invalid(detail: String) -> StorageError {
    StorageError::InvalidRequest { detail }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

// ── 行类型与枚举（wire 字符串镜像现役） ──

/// 现役 storageKind：`external`（用户原文件引用，永不删除）/
/// `managed_cache`（托管副本，冷会话可回收）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    External,
    ManagedCache,
}

impl StorageKind {
    pub fn wire(self) -> &'static str {
        match self {
            StorageKind::External => "external",
            StorageKind::ManagedCache => "managed_cache",
        }
    }

    fn parse(raw: &str) -> Result<Self, StorageError> {
        match raw {
            "external" => Ok(StorageKind::External),
            "managed_cache" => Ok(StorageKind::ManagedCache),
            other => Err(invalid(format!(
                "unknown session_files.storage_kind {other:?}"
            ))),
        }
    }
}

/// 现役 status：`available` / `missing` / `expired`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFileStatus {
    Available,
    Missing,
    Expired,
}

impl SessionFileStatus {
    pub fn wire(self) -> &'static str {
        match self {
            SessionFileStatus::Available => "available",
            SessionFileStatus::Missing => "missing",
            SessionFileStatus::Expired => "expired",
        }
    }

    fn parse(raw: &str) -> Result<Self, StorageError> {
        match raw {
            "available" => Ok(SessionFileStatus::Available),
            "missing" => Ok(SessionFileStatus::Missing),
            "expired" => Ok(SessionFileStatus::Expired),
            other => Err(invalid(format!("unknown session_files.status {other:?}"))),
        }
    }
}

/// 现役 inferFileKind：directory/image/video/audio/document/unknown。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Directory,
    Image,
    Video,
    Audio,
    Document,
    Unknown,
}

impl FileKind {
    pub fn wire(self) -> &'static str {
        match self {
            FileKind::Directory => "directory",
            FileKind::Image => "image",
            FileKind::Video => "video",
            FileKind::Audio => "audio",
            FileKind::Document => "document",
            FileKind::Unknown => "unknown",
        }
    }

    fn parse(raw: &str) -> Result<Self, StorageError> {
        match raw {
            "directory" => Ok(FileKind::Directory),
            "image" => Ok(FileKind::Image),
            "video" => Ok(FileKind::Video),
            "audio" => Ok(FileKind::Audio),
            "document" => Ok(FileKind::Document),
            "unknown" => Ok(FileKind::Unknown),
            other => Err(invalid(format!(
                "unknown session_files.file_kind {other:?}"
            ))),
        }
    }
}

/// 现役 origin 开放字符串集（session-file-registry.ts inferOperation 的
/// switch 输入域 + forkSessionFileRefs 的 "session_fork" + 默认
/// "unknown"）。注意本列存的是**现役 origin 原值**（user_upload 等），
/// 不是 inferOperation 的派生 operation——派生映射在 service 层做
/// （RC-3：sidecar 的 origin 字段逐字节保留，A09 导入不丢信息）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFileOrigin {
    StageFiles,
    UserUpload,
    UserAttachment,
    BridgeInbound,
    AgentWrite,
    AgentArtifact,
    PluginOutput,
    InstallSkillOutput,
    AgentEdit,
    BrowserScreenshot,
    SkillInstallSource,
    PluginInstallSource,
    BridgeManualSend,
    VoiceInput,
    SessionFork,
    Unknown,
}

impl SessionFileOrigin {
    pub fn wire(self) -> &'static str {
        match self {
            SessionFileOrigin::StageFiles => "stage_files",
            SessionFileOrigin::UserUpload => "user_upload",
            SessionFileOrigin::UserAttachment => "user_attachment",
            SessionFileOrigin::BridgeInbound => "bridge_inbound",
            SessionFileOrigin::AgentWrite => "agent_write",
            SessionFileOrigin::AgentArtifact => "agent_artifact",
            SessionFileOrigin::PluginOutput => "plugin_output",
            SessionFileOrigin::InstallSkillOutput => "install_skill_output",
            SessionFileOrigin::AgentEdit => "agent_edit",
            SessionFileOrigin::BrowserScreenshot => "browser_screenshot",
            SessionFileOrigin::SkillInstallSource => "skill_install_source",
            SessionFileOrigin::PluginInstallSource => "plugin_install_source",
            SessionFileOrigin::BridgeManualSend => "bridge_manual_send",
            SessionFileOrigin::VoiceInput => "voice_input",
            SessionFileOrigin::SessionFork => "session_fork",
            SessionFileOrigin::Unknown => "unknown",
        }
    }

    fn parse(raw: &str) -> Result<Self, StorageError> {
        match raw {
            "stage_files" => Ok(SessionFileOrigin::StageFiles),
            "user_upload" => Ok(SessionFileOrigin::UserUpload),
            "user_attachment" => Ok(SessionFileOrigin::UserAttachment),
            "bridge_inbound" => Ok(SessionFileOrigin::BridgeInbound),
            "agent_write" => Ok(SessionFileOrigin::AgentWrite),
            "agent_artifact" => Ok(SessionFileOrigin::AgentArtifact),
            "plugin_output" => Ok(SessionFileOrigin::PluginOutput),
            "install_skill_output" => Ok(SessionFileOrigin::InstallSkillOutput),
            "agent_edit" => Ok(SessionFileOrigin::AgentEdit),
            "browser_screenshot" => Ok(SessionFileOrigin::BrowserScreenshot),
            "skill_install_source" => Ok(SessionFileOrigin::SkillInstallSource),
            "plugin_install_source" => Ok(SessionFileOrigin::PluginInstallSource),
            "bridge_manual_send" => Ok(SessionFileOrigin::BridgeManualSend),
            "voice_input" => Ok(SessionFileOrigin::VoiceInput),
            "session_fork" => Ok(SessionFileOrigin::SessionFork),
            "unknown" => Ok(SessionFileOrigin::Unknown),
            other => Err(invalid(format!("unknown session_files.origin {other:?}"))),
        }
    }
}

/// 现役 inferOperation（registry :1076-1104）：origin → operation 派生。
pub fn infer_operation(origin: SessionFileOrigin) -> &'static str {
    match origin {
        SessionFileOrigin::StageFiles => "staged",
        SessionFileOrigin::UserUpload => "uploaded",
        SessionFileOrigin::UserAttachment | SessionFileOrigin::BridgeInbound => "attached",
        SessionFileOrigin::AgentWrite
        | SessionFileOrigin::AgentArtifact
        | SessionFileOrigin::PluginOutput
        | SessionFileOrigin::InstallSkillOutput => "created",
        SessionFileOrigin::AgentEdit => "modified",
        SessionFileOrigin::BrowserScreenshot => "captured",
        SessionFileOrigin::SkillInstallSource | SessionFileOrigin::PluginInstallSource => {
            "referenced"
        }
        SessionFileOrigin::BridgeManualSend => "sent",
        SessionFileOrigin::VoiceInput => "recorded",
        SessionFileOrigin::SessionFork | SessionFileOrigin::Unknown => "registered",
    }
}

/// 引用来源类别：消息 [SessionFile] 标记 / attached_* 媒体标记 / typed
/// 对象 / stage 交付 / 上传登记 / 导入 sidecar refs / 恢复快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    MessageMarker,
    MediaMarker,
    TypedObject,
    Stage,
    Upload,
    Import,
    Restore,
}

impl RefKind {
    pub fn wire(self) -> &'static str {
        match self {
            RefKind::MessageMarker => "message_marker",
            RefKind::MediaMarker => "media_marker",
            RefKind::TypedObject => "typed_object",
            RefKind::Stage => "stage",
            RefKind::Upload => "upload",
            RefKind::Import => "import",
            RefKind::Restore => "restore",
        }
    }

    fn parse(raw: &str) -> Result<Self, StorageError> {
        match raw {
            "message_marker" => Ok(RefKind::MessageMarker),
            "media_marker" => Ok(RefKind::MediaMarker),
            "typed_object" => Ok(RefKind::TypedObject),
            "stage" => Ok(RefKind::Stage),
            "upload" => Ok(RefKind::Upload),
            "import" => Ok(RefKind::Import),
            "restore" => Ok(RefKind::Restore),
            other => Err(invalid(format!(
                "unknown session_file_refs.ref_kind {other:?}"
            ))),
        }
    }
}

/// session_files 一行（字段 = 现役 sidecar files map value 全集 +
/// legacyFileIds/legacyFilePaths）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFileRow {
    pub file_id: String,
    pub owner_session_id: String,
    pub owner_key: String,
    pub source_key: Option<String>,
    pub identity_key: String,
    pub file_path: String,
    pub real_path: String,
    pub storage_kind: StorageKind,
    pub status: SessionFileStatus,
    pub label: Option<String>,
    pub filename: String,
    pub mime: String,
    pub size_bytes: u64,
    pub mtime_ms: u64,
    pub is_directory: bool,
    pub file_kind: FileKind,
    pub origin: SessionFileOrigin,
    pub registered_at_ms: u64,
    pub updated_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub legacy_file_ids: Vec<String>,
    pub legacy_file_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFileRefRow {
    pub file_id: String,
    pub session_id: String,
    pub message_id: Option<String>,
    pub ref_kind: RefKind,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct FileSnapshotInsert<'a> {
    pub workspace_hash: &'a str,
    pub rel_path: &'a str,
    pub captured_at_ms: u64,
    pub origin: &'a str,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshotRow {
    pub id: i64,
    pub workspace_hash: String,
    pub rel_path: String,
    pub captured_at_ms: u64,
    pub origin: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshotMeta {
    pub id: i64,
    pub rel_path: String,
    pub captured_at_ms: u64,
    pub origin: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHistoryFileEntry {
    pub rel_path: String,
    pub version_count: u64,
    pub last_captured_at_ms: u64,
}

// ── 行解码 ──

fn parse_json_string_array(raw: &str) -> Result<Vec<String>, StorageError> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|err| StorageError::InvalidRequest {
            detail: format!("session_files legacy json column is not valid json: {err}"),
        })?;
    let items = value
        .as_array()
        .ok_or_else(|| StorageError::InvalidRequest {
            detail: "session_files legacy json column is not an array".to_string(),
        })?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(
            item.as_str()
                .ok_or_else(|| StorageError::InvalidRequest {
                    detail: "session_files legacy json array holds a non-string".to_string(),
                })?
                .to_string(),
        );
    }
    Ok(out)
}

const FILE_COLS: &str = "file_id, owner_session_id, owner_key, source_key, identity_key, \
     file_path, real_path, storage_kind, status, label, filename, mime, size_bytes, \
     mtime_ms, is_directory, file_kind, origin, registered_at_ms, updated_at_ms, \
     expires_at_ms, legacy_file_ids_json, legacy_file_paths_json";

/// 查询行原始形态：枚举/JSON 列仍是 TEXT（query_map/query_row 闭包只能
/// 返回 rusqlite 错误，语义解析在闭包外完成——非法值响亮报 StorageError）。
struct RawFileRow {
    file_id: String,
    owner_session_id: String,
    owner_key: String,
    source_key: Option<String>,
    identity_key: String,
    file_path: String,
    real_path: String,
    storage_kind: String,
    status: String,
    label: Option<String>,
    filename: String,
    mime: String,
    size_bytes: i64,
    mtime_ms: i64,
    is_directory: i64,
    file_kind: String,
    origin: String,
    registered_at_ms: i64,
    updated_at_ms: i64,
    expires_at_ms: Option<i64>,
    legacy_file_ids_json: String,
    legacy_file_paths_json: String,
}

fn read_raw_file_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawFileRow> {
    Ok(RawFileRow {
        file_id: row.get(0)?,
        owner_session_id: row.get(1)?,
        owner_key: row.get(2)?,
        source_key: row.get(3)?,
        identity_key: row.get(4)?,
        file_path: row.get(5)?,
        real_path: row.get(6)?,
        storage_kind: row.get(7)?,
        status: row.get(8)?,
        label: row.get(9)?,
        filename: row.get(10)?,
        mime: row.get(11)?,
        size_bytes: row.get(12)?,
        mtime_ms: row.get(13)?,
        is_directory: row.get(14)?,
        file_kind: row.get(15)?,
        origin: row.get(16)?,
        registered_at_ms: row.get(17)?,
        updated_at_ms: row.get(18)?,
        expires_at_ms: row.get(19)?,
        legacy_file_ids_json: row.get(20)?,
        legacy_file_paths_json: row.get(21)?,
    })
}

fn decode_raw_file_row(raw: RawFileRow) -> Result<SessionFileRow, StorageError> {
    Ok(SessionFileRow {
        file_id: raw.file_id,
        owner_session_id: raw.owner_session_id,
        owner_key: raw.owner_key,
        source_key: raw.source_key,
        identity_key: raw.identity_key,
        file_path: raw.file_path,
        real_path: raw.real_path,
        storage_kind: StorageKind::parse(&raw.storage_kind)?,
        status: SessionFileStatus::parse(&raw.status)?,
        label: raw.label,
        filename: raw.filename,
        mime: raw.mime,
        size_bytes: raw.size_bytes as u64,
        mtime_ms: raw.mtime_ms as u64,
        is_directory: raw.is_directory != 0,
        file_kind: FileKind::parse(&raw.file_kind)?,
        origin: SessionFileOrigin::parse(&raw.origin)?,
        registered_at_ms: raw.registered_at_ms as u64,
        updated_at_ms: raw.updated_at_ms as u64,
        expires_at_ms: raw.expires_at_ms.map(|v| v as u64),
        legacy_file_ids: parse_json_string_array(&raw.legacy_file_ids_json)?,
        legacy_file_paths: parse_json_string_array(&raw.legacy_file_paths_json)?,
    })
}

struct RawRefRow {
    file_id: String,
    session_id: String,
    message_id: Option<String>,
    ref_kind: String,
    created_at_ms: i64,
}

fn read_raw_ref_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRefRow> {
    Ok(RawRefRow {
        file_id: row.get(0)?,
        session_id: row.get(1)?,
        message_id: row.get(2)?,
        ref_kind: row.get(3)?,
        created_at_ms: row.get(4)?,
    })
}

fn decode_raw_ref_row(raw: RawRefRow) -> Result<SessionFileRefRow, StorageError> {
    Ok(SessionFileRefRow {
        file_id: raw.file_id,
        session_id: raw.session_id,
        message_id: raw.message_id,
        ref_kind: RefKind::parse(&raw.ref_kind)?,
        created_at_ms: raw.created_at_ms as u64,
    })
}

const REF_COLS: &str = "file_id, session_id, message_id, ref_kind, created_at_ms";

// ── session_files ──

pub async fn insert_session_file(
    db: &RunDatabase,
    row: &SessionFileRow,
) -> Result<(), StorageError> {
    let row = row.clone();
    db.queue()
        .submit(move |conn| {
            conn.execute(
                &format!(
                    "INSERT INTO session_files ({FILE_COLS}) VALUES \
                     (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)"
                ),
                rusqlite::params![
                    row.file_id,
                    row.owner_session_id,
                    row.owner_key,
                    row.source_key,
                    row.identity_key,
                    row.file_path,
                    row.real_path,
                    row.storage_kind.wire(),
                    row.status.wire(),
                    row.label,
                    row.filename,
                    row.mime,
                    row.size_bytes as i64,
                    row.mtime_ms as i64,
                    if row.is_directory { 1i64 } else { 0i64 },
                    row.file_kind.wire(),
                    row.origin.wire(),
                    row.registered_at_ms as i64,
                    row.updated_at_ms as i64,
                    row.expires_at_ms.map(|v| v as i64),
                    serde_json::to_string(&row.legacy_file_ids).unwrap_or_else(|_| "[]".into()),
                    serde_json::to_string(&row.legacy_file_paths).unwrap_or_else(|_| "[]".into()),
                ],
            )
            .map_err(map_err)?;
            Ok(())
        })
        .await
}

pub async fn get_session_file(
    db: &RunDatabase,
    file_id: &str,
) -> Result<Option<SessionFileRow>, StorageError> {
    let file_id = file_id.to_string();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                &format!("SELECT {FILE_COLS} FROM session_files WHERE file_id=?1"),
                rusqlite::params![file_id],
                read_raw_file_row,
            )
            .optional()
            .map_err(map_err)?
            .map(decode_raw_file_row)
            .transpose()
        })
        .await
}

/// 现役 getBySourceKey：同 owner 下按 source_key 复用既有登记。
pub async fn get_session_file_by_source_key(
    db: &RunDatabase,
    owner_session_id: &str,
    source_key: &str,
) -> Result<Option<SessionFileRow>, StorageError> {
    let owner = owner_session_id.to_string();
    let key = source_key.to_string();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                &format!(
                    "SELECT {FILE_COLS} FROM session_files \
                     WHERE owner_session_id=?1 AND source_key=?2"
                ),
                rusqlite::params![owner, key],
                read_raw_file_row,
            )
            .optional()
            .map_err(map_err)?
            .map(decode_raw_file_row)
            .transpose()
        })
        .await
}

/// 现役 getByFilePath：同 owner 下按 real_path 找既有登记。
pub async fn get_session_file_by_real_path(
    db: &RunDatabase,
    owner_session_id: &str,
    real_path: &str,
) -> Result<Option<SessionFileRow>, StorageError> {
    let owner = owner_session_id.to_string();
    let path = real_path.to_string();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                &format!(
                    "SELECT {FILE_COLS} FROM session_files \
                     WHERE owner_session_id=?1 AND real_path=?2"
                ),
                rusqlite::params![owner, path],
                read_raw_file_row,
            )
            .optional()
            .map_err(map_err)?
            .map(decode_raw_file_row)
            .transpose()
        })
        .await
}

/// 现役 listReachable 的存储侧：owner 的全部登记，注册序（file_id 决胜）。
pub async fn list_session_files_for_owner(
    db: &RunDatabase,
    owner_session_id: &str,
) -> Result<Vec<SessionFileRow>, StorageError> {
    let owner = owner_session_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {FILE_COLS} FROM session_files \
                     WHERE owner_session_id=?1 ORDER BY registered_at_ms, file_id"
                ))
                .map_err(map_err)?;
            let rows = stmt
                .query_map(rusqlite::params![owner], read_raw_file_row)
                .map_err(map_err)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(decode_raw_file_row(row.map_err(map_err)?)?);
            }
            Ok(out)
        })
        .await
}

/// 多个 owner 的全部行（冷会话清理枚举）。
pub async fn list_session_files_for_owners(
    db: &RunDatabase,
    owner_session_ids: &[String],
) -> Result<Vec<SessionFileRow>, StorageError> {
    let owners = owner_session_ids.to_vec();
    db.queue()
        .submit(move |conn| {
            let mut out = Vec::new();
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {FILE_COLS} FROM session_files WHERE owner_session_id=?1"
                ))
                .map_err(map_err)?;
            for owner in &owners {
                let rows = stmt
                    .query_map(rusqlite::params![owner], read_raw_file_row)
                    .map_err(map_err)?;
                for row in rows {
                    out.push(decode_raw_file_row(row.map_err(map_err)?)?);
                }
            }
            Ok(out)
        })
        .await
}

pub async fn update_session_file_status(
    db: &RunDatabase,
    file_id: &str,
    status: SessionFileStatus,
    expires_at_ms: Option<u64>,
    updated_at_ms: u64,
) -> Result<(), StorageError> {
    let file_id = file_id.to_string();
    db.queue()
        .submit(move |conn| {
            conn.execute(
                "UPDATE session_files SET status=?2, expires_at_ms=?3, updated_at_ms=?4 \
                 WHERE file_id=?1",
                rusqlite::params![
                    file_id,
                    status.wire(),
                    expires_at_ms.map(|v| v as i64),
                    updated_at_ms as i64
                ],
            )
            .map_err(map_err)?;
            Ok(())
        })
        .await
}

/// 现役 _reconcileFileAvailability 的落库：realpath/可用性校正。
pub async fn update_session_file_paths(
    db: &RunDatabase,
    file_id: &str,
    file_path: &str,
    real_path: &str,
    updated_at_ms: u64,
) -> Result<(), StorageError> {
    let file_id = file_id.to_string();
    let file_path = file_path.to_string();
    let real_path = real_path.to_string();
    db.queue()
        .submit(move |conn| {
            conn.execute(
                "UPDATE session_files SET file_path=?2, real_path=?3, updated_at_ms=?4 \
                 WHERE file_id=?1",
                rusqlite::params![file_id, file_path, real_path, updated_at_ms as i64],
            )
            .map_err(map_err)?;
            Ok(())
        })
        .await
}

/// 会话永久删除联动（D13）：行删除；别名一并清除；refs 由调用方按
/// session 另行删除。用户的 external 文件本体不由本函数触碰。
pub async fn delete_session_file(db: &RunDatabase, file_id: &str) -> Result<(), StorageError> {
    let file_id = file_id.to_string();
    db.queue()
        .submit(move |conn| {
            conn.execute(
                "DELETE FROM session_file_aliases WHERE canonical_file_id=?1",
                rusqlite::params![file_id],
            )
            .map_err(map_err)?;
            conn.execute(
                "DELETE FROM session_files WHERE file_id=?1",
                rusqlite::params![file_id],
            )
            .map_err(map_err)?;
            Ok(())
        })
        .await
}

// ── aliases ──

pub async fn insert_alias(
    db: &RunDatabase,
    alias_file_id: &str,
    canonical_file_id: &str,
    created_at_ms: u64,
) -> Result<(), StorageError> {
    let alias = alias_file_id.to_string();
    let canonical = canonical_file_id.to_string();
    db.queue()
        .submit(move |conn| {
            conn.execute(
                "INSERT INTO session_file_aliases (alias_file_id, canonical_file_id, created_at_ms) \
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![alias, canonical, created_at_ms as i64],
            )
            .map_err(map_err)?;
            Ok(())
        })
        .await
}

pub async fn resolve_alias(
    db: &RunDatabase,
    alias_file_id: &str,
) -> Result<Option<String>, StorageError> {
    let alias = alias_file_id.to_string();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                "SELECT canonical_file_id FROM session_file_aliases WHERE alias_file_id=?1",
                rusqlite::params![alias],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_err)
        })
        .await
}

/// fork 时的别名批量登记 + 现行行 legacy 列表更新由 service 组合；本函数
/// 列出某 canonical 行的全部旧 id（fork 旧主对照）。
pub async fn list_aliases_for_canonical(
    db: &RunDatabase,
    canonical_file_id: &str,
) -> Result<Vec<String>, StorageError> {
    let canonical = canonical_file_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT alias_file_id FROM session_file_aliases \
                     WHERE canonical_file_id=?1 ORDER BY created_at_ms, alias_file_id",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map(rusqlite::params![canonical], |row| row.get(0))
                .map_err(map_err)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

// ── refs ──

pub async fn insert_ref(db: &RunDatabase, refr: &SessionFileRefRow) -> Result<(), StorageError> {
    let refr = refr.clone();
    db.queue()
        .submit(move |conn| {
            conn.execute(
                "INSERT INTO session_file_refs (file_id, session_id, message_id, ref_kind, created_at_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    refr.file_id,
                    refr.session_id,
                    refr.message_id,
                    refr.ref_kind.wire(),
                    refr.created_at_ms as i64
                ],
            )
            .map_err(map_err)?;
            Ok(())
        })
        .await
}

pub async fn list_refs_for_session(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Vec<SessionFileRefRow>, StorageError> {
    let session_id = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {REF_COLS} FROM session_file_refs WHERE session_id=?1 ORDER BY id"
                ))
                .map_err(map_err)?;
            let rows = stmt
                .query_map(rusqlite::params![session_id], read_raw_ref_row)
                .map_err(map_err)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(decode_raw_ref_row(row.map_err(map_err)?)?);
            }
            Ok(out)
        })
        .await
}

pub async fn list_refs_for_file(
    db: &RunDatabase,
    file_id: &str,
) -> Result<Vec<SessionFileRefRow>, StorageError> {
    let file_id = file_id.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {REF_COLS} FROM session_file_refs WHERE file_id=?1 ORDER BY id"
                ))
                .map_err(map_err)?;
            let rows = stmt
                .query_map(rusqlite::params![file_id], read_raw_ref_row)
                .map_err(map_err)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(decode_raw_ref_row(row.map_err(map_err)?)?);
            }
            Ok(out)
        })
        .await
}

pub async fn delete_refs_for_session(
    db: &RunDatabase,
    session_id: &str,
) -> Result<(), StorageError> {
    let session_id = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            conn.execute(
                "DELETE FROM session_file_refs WHERE session_id=?1",
                rusqlite::params![session_id],
            )
            .map_err(map_err)?;
            Ok(())
        })
        .await
}

// ── 冷会话枚举（清理用；现役对照：cleanupColdSessions 扫 agentsDir 的
// jsonl mtime —— 候选映射为 sessions.last_activity_unix_ms，台账 D3） ──

/// 列出最后活动早于 cutoff 且未删除的会话 id（含 archived：现役按
/// jsonl mtime 判定，归档时 mtime 被置为归档瞬间，同样会冷）。
pub async fn list_cold_session_ids(
    db: &RunDatabase,
    cutoff_unix_ms: u64,
) -> Result<Vec<String>, StorageError> {
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT session_id FROM sessions \
                     WHERE last_activity_unix_ms IS NOT NULL \
                       AND last_activity_unix_ms < ?1 \
                       AND lifecycle != 'deleted' \
                     ORDER BY session_id",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map(rusqlite::params![cutoff_unix_ms as i64], |row| row.get(0))
                .map_err(map_err)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

// ── file_history_snapshots ──

/// 冷度写入（差异台账 D3 的配套）：现役 cleanupColdSessionFiles 以会话
/// jsonl 的 mtime 判冷；候选以 sessions.last_activity_unix_ms 承载同一
/// 语义。会话行已存在时只更新该列；不存在（导入/测试场景）时插入最小
/// 合法行，其余列用与 SessionRow 约定一致的占位值，绝不伪造业务事实。
pub async fn upsert_session_activity(
    db: &RunDatabase,
    session_id: &str,
    last_activity_unix_ms: u64,
) -> Result<(), StorageError> {
    let session_id = session_id.to_string();
    db.queue()
        .submit(move |conn| {
            conn.execute(
                "INSERT INTO sessions (session_id, agent_id, owner_user_id, title, \
                                        created_at_unix_ms, last_activity_unix_ms) \
                 VALUES (?1, 'agent', 'user_local', '', ?2, ?2) \
                 ON CONFLICT(session_id) DO UPDATE SET last_activity_unix_ms=?2",
                rusqlite::params![session_id, last_activity_unix_ms as i64],
            )
            .map_err(map_err)?;
            Ok(())
        })
        .await
}

// ── file_history_snapshots ──

pub async fn insert_file_snapshot(
    db: &RunDatabase,
    input: &FileSnapshotInsert<'_>,
) -> Result<i64, StorageError> {
    let workspace_hash = input.workspace_hash.to_string();
    let rel_path = input.rel_path.to_string();
    let captured_at_ms = input.captured_at_ms;
    let origin = input.origin.to_string();
    let content = input.content.clone();
    db.queue()
        .submit(move |conn| {
            let sha = sha256_hex(&content);
            conn.execute(
                "INSERT INTO file_history_snapshots \
                 (workspace_hash, rel_path, captured_at_ms, origin, size_bytes, sha256, content) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    workspace_hash,
                    rel_path,
                    captured_at_ms as i64,
                    origin,
                    content.len() as i64,
                    sha,
                    content
                ],
            )
            .map_err(map_err)?;
            Ok(conn.last_insert_rowid())
        })
        .await
}

pub async fn list_file_history_files(
    db: &RunDatabase,
    workspace_hash: &str,
) -> Result<Vec<FileHistoryFileEntry>, StorageError> {
    let ws = workspace_hash.to_string();
    db.queue()
        .submit(move |conn| {
            // 现役 listFiles 排序：lastCapturedAt DESC（history-store.ts :149）。
            let mut stmt = conn
                .prepare(
                    "SELECT rel_path, COUNT(*), MAX(captured_at_ms) \
                     FROM file_history_snapshots WHERE workspace_hash=?1 \
                     GROUP BY rel_path ORDER BY MAX(captured_at_ms) DESC, rel_path",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map(rusqlite::params![ws], |row| {
                    Ok(FileHistoryFileEntry {
                        rel_path: row.get(0)?,
                        version_count: row.get::<_, i64>(1)? as u64,
                        last_captured_at_ms: row.get::<_, i64>(2)? as u64,
                    })
                })
                .map_err(map_err)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

/// 现役 listVersions：captured_at DESC, id DESC（history-store.ts :158）。
pub async fn list_file_history_versions(
    db: &RunDatabase,
    workspace_hash: &str,
    rel_path: &str,
) -> Result<Vec<FileSnapshotMeta>, StorageError> {
    let ws = workspace_hash.to_string();
    let rel = rel_path.to_string();
    db.queue()
        .submit(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, rel_path, captured_at_ms, origin, size_bytes, sha256 \
                     FROM file_history_snapshots \
                     WHERE workspace_hash=?1 AND rel_path=?2 \
                     ORDER BY captured_at_ms DESC, id DESC",
                )
                .map_err(map_err)?;
            let rows = stmt
                .query_map(rusqlite::params![ws, rel], |row| {
                    Ok(FileSnapshotMeta {
                        id: row.get(0)?,
                        rel_path: row.get(1)?,
                        captured_at_ms: row.get::<_, i64>(2)? as u64,
                        origin: row.get(3)?,
                        size_bytes: row.get::<_, i64>(4)? as u64,
                        sha256: row.get(5)?,
                    })
                })
                .map_err(map_err)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(map_err)?);
            }
            Ok(out)
        })
        .await
}

pub async fn get_file_snapshot(
    db: &RunDatabase,
    workspace_hash: &str,
    id: i64,
) -> Result<Option<FileSnapshotRow>, StorageError> {
    let ws = workspace_hash.to_string();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                "SELECT id, workspace_hash, rel_path, captured_at_ms, origin, size_bytes, sha256, content \
                 FROM file_history_snapshots WHERE workspace_hash=?1 AND id=?2",
                rusqlite::params![ws, id],
                |row| {
                    Ok(FileSnapshotRow {
                        id: row.get(0)?,
                        workspace_hash: row.get(1)?,
                        rel_path: row.get(2)?,
                        captured_at_ms: row.get::<_, i64>(3)? as u64,
                        origin: row.get(4)?,
                        size_bytes: row.get::<_, i64>(5)? as u64,
                        sha256: row.get(6)?,
                        content: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(map_err)
        })
        .await
}

/// 现役 recordSnapshot 的 latest 探测（history-store.ts :87-89）：
/// 同 (workspace, rel_path) 的最新一行（captured_at DESC, id DESC）。
pub async fn latest_file_snapshot_meta(
    db: &RunDatabase,
    workspace_hash: &str,
    rel_path: &str,
) -> Result<Option<FileSnapshotMeta>, StorageError> {
    let ws = workspace_hash.to_string();
    let rel = rel_path.to_string();
    db.queue()
        .submit(move |conn| {
            conn.query_row(
                "SELECT id, rel_path, captured_at_ms, origin, size_bytes, sha256 \
                 FROM file_history_snapshots \
                 WHERE workspace_hash=?1 AND rel_path=?2 \
                 ORDER BY captured_at_ms DESC, id DESC LIMIT 1",
                rusqlite::params![ws, rel],
                |row| {
                    Ok(FileSnapshotMeta {
                        id: row.get(0)?,
                        rel_path: row.get(1)?,
                        captured_at_ms: row.get::<_, i64>(2)? as u64,
                        origin: row.get(3)?,
                        size_bytes: row.get::<_, i64>(4)? as u64,
                        sha256: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(map_err)
        })
        .await
}

/// 现役合并臂的 UPDATE（history-store.ts :104-106）：行 id 不变，内容、
/// 尺寸、sha、捕获时间与 origin 全部刷成新值（候选人无 gzip/op_context
/// 两列，台账登记）。
pub async fn update_file_snapshot(
    db: &RunDatabase,
    workspace_hash: &str,
    id: i64,
    captured_at_ms: u64,
    origin: &str,
    content: &[u8],
) -> Result<(), StorageError> {
    let ws = workspace_hash.to_string();
    let origin = origin.to_string();
    let content = content.to_vec();
    db.queue()
        .submit(move |conn| {
            let sha = sha256_hex(&content);
            conn.execute(
                "UPDATE file_history_snapshots \
                 SET captured_at_ms=?3, origin=?4, size_bytes=?5, sha256=?6, content=?7 \
                 WHERE id=?1 AND workspace_hash=?2",
                rusqlite::params![
                    id,
                    ws,
                    captured_at_ms as i64,
                    origin,
                    content.len() as i64,
                    sha,
                    content
                ],
            )
            .map_err(map_err)?;
            Ok(())
        })
        .await
}

/// 保留策略（现役 history-store 的 maxAgeMs/maxTotalBytes，对照
/// FILE_HISTORY_DEFAULTS）：先删超龄，再按总字节从旧到新删。
/// 返回删除行数。
pub async fn prune_file_history(
    db: &RunDatabase,
    workspace_hash: &str,
    max_age_ms: u64,
    max_total_bytes: u64,
    now_ms: u64,
) -> Result<u64, StorageError> {
    let ws = workspace_hash.to_string();
    db.queue()
        .submit(move |conn| {
            let cutoff = now_ms.saturating_sub(max_age_ms) as i64;
            let aged = conn
                .execute(
                    "DELETE FROM file_history_snapshots \
                     WHERE workspace_hash=?1 AND captured_at_ms < ?2",
                    rusqlite::params![ws, cutoff],
                )
                .map_err(map_err)?;
            let mut pruned = aged as u64;
            loop {
                let total: i64 = conn
                    .query_row(
                        "SELECT COALESCE(SUM(size_bytes), 0) FROM file_history_snapshots \
                         WHERE workspace_hash=?1",
                        rusqlite::params![ws],
                        |row| row.get(0),
                    )
                    .map_err(map_err)?;
                if total as u64 <= max_total_bytes {
                    break;
                }
                // 最旧一行（守护：没有行可删时绝不死循环）。
                let oldest: Option<i64> = conn
                    .query_row(
                        "SELECT id FROM file_history_snapshots \
                         WHERE workspace_hash=?1 ORDER BY id ASC LIMIT 1",
                        rusqlite::params![ws],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(map_err)?;
                let Some(oldest_id) = oldest else { break };
                conn.execute(
                    "DELETE FROM file_history_snapshots WHERE id=?1",
                    rusqlite::params![oldest_id],
                )
                .map_err(map_err)?;
                pruned += 1;
            }
            Ok(pruned)
        })
        .await
}
