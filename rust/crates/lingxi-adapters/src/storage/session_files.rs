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
//! 注册去重由唯一索引兜底（同 owner 同 source_key 响亮冲突）。
//! 服务层给定冷度和托管根；涉及真实字节的提交在同一队列、事务和物理租约内
//! 保护有效引用，并保留删除失败时的持久清扫身份。

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
/// R08-T04 追加 `broken`（A07 legacy 附件坏因：payload 缺失/截断仍登记行、
/// 响亮报告，绝不以 available 冒充可读）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFileStatus {
    Available,
    Missing,
    Expired,
    Broken,
}

impl SessionFileStatus {
    pub fn wire(self) -> &'static str {
        match self {
            SessionFileStatus::Available => "available",
            SessionFileStatus::Missing => "missing",
            SessionFileStatus::Expired => "expired",
            SessionFileStatus::Broken => "broken",
        }
    }

    fn parse(raw: &str) -> Result<Self, StorageError> {
        match raw {
            "available" => Ok(SessionFileStatus::Available),
            "missing" => Ok(SessionFileStatus::Missing),
            "expired" => Ok(SessionFileStatus::Expired),
            "broken" => Ok(SessionFileStatus::Broken),
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
    FileCopy,
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
            SessionFileOrigin::FileCopy => "file_copy",
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
            "file_copy" => Ok(SessionFileOrigin::FileCopy),
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
        SessionFileOrigin::FileCopy
        | SessionFileOrigin::AgentWrite
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
        .submit(move |conn| insert_file_on_connection(conn, &row))
        .await
}

pub async fn update_session_file_metadata(
    db: &RunDatabase,
    row: &SessionFileRow,
) -> Result<(), StorageError> {
    let row = row.clone();
    db.queue().submit(move |conn| {
        let changed = conn.execute(
            "UPDATE session_files SET mime=?1,size_bytes=?2,mtime_ms=?3,is_directory=?4,file_kind=?5,updated_at_ms=?6 WHERE file_id=?7 AND owner_session_id=?8",
            rusqlite::params![
                row.mime, row.size_bytes as i64, row.mtime_ms as i64,
                if row.is_directory { 1i64 } else { 0i64 }, row.file_kind.wire(),
                row.updated_at_ms as i64, row.file_id, row.owner_session_id,
            ],
        ).map_err(map_err)?;
        if changed != 1 {
            return Err(StorageError::InvalidRequest {
                detail: "session file vanished while refreshing metadata".into(),
            });
        }
        Ok(())
    }).await
}

pub(crate) fn insert_file_on_connection(
    conn: &rusqlite::Connection,
    row: &SessionFileRow,
) -> Result<(), StorageError> {
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
            let target:Option<(String,String,String,String)>=conn.query_row("SELECT f.real_path,f.status,s.owner_user_id,s.agent_id FROM session_files f JOIN sessions s ON s.session_id=f.owner_session_id WHERE f.file_id=?1",[&refr.file_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(map_err)?;
            let subject:Option<(String,String)>=conn.query_row("SELECT owner_user_id,agent_id FROM sessions WHERE session_id=?1",[&refr.session_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(map_err)?;
            if let Some((path,status,owner,agent))=target {
                if status!="available"||!std::path::Path::new(&path).exists(){return Err(invalid("reference payload missing".into()));}
                if let Some((subject,subject_agent))=subject {if owner!=subject||agent!=subject_agent{return Err(invalid("cross_subject_file_reference".into()));}}
            }
            conn.execute(
                "INSERT INTO session_file_refs (file_id, session_id, message_id, ref_kind, created_at_ms) \
                 SELECT ?1, ?2, ?3, ?4, ?5 WHERE NOT EXISTS \
                 (SELECT 1 FROM session_file_refs WHERE file_id=?1 AND session_id=?2 AND message_id IS ?3 AND ref_kind=?4)",
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

/// 整批交付引用同事务提交，正式租约由执行器移入队列，直到物理提交退出。
pub async fn insert_stage_refs(
    db: &RunDatabase,
    session_id: &str,
    file_ids: Vec<String>,
    message_id: &str,
    now: u64,
    lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> Result<(), StorageError> {
    let session_id = session_id.to_string();
    let message_id = message_id.to_string();
    db.queue().submit(move |conn| {
        let _lease = lease;
        let tx = conn.unchecked_transaction().map_err(map_err)?;
        for file_id in file_ids {
            let file: Option<(String, String, String)> = tx.query_row(
                "SELECT owner_session_id,real_path,status FROM session_files WHERE file_id=?1",
                [&file_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).optional().map_err(map_err)?;
            let Some((owner, path, status)) = file else {
                return Err(invalid(format!("session_file_reference_missing:{file_id}")));
            };
            if owner != session_id {
                return Err(invalid(format!("session_file_reference_not_owned:{file_id}")));
            }
            if status != "available" || !std::path::Path::new(&path).exists() {
                return Err(invalid(format!("session_file_reference_missing:{file_id}")));
            }
            tx.execute(
                "INSERT INTO session_file_refs(file_id,session_id,message_id,ref_kind,created_at_ms) SELECT ?1,?2,?3,'stage',?4 WHERE NOT EXISTS (SELECT 1 FROM session_file_refs WHERE file_id=?1 AND session_id=?2 AND message_id=?3 AND ref_kind='stage')",
                rusqlite::params![file_id,session_id,message_id,now as i64],
            ).map_err(map_err)?;
        }
        touch_on_connection(&tx, &session_id, now)?;
        tx.commit().map_err(map_err)
    }).await
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

fn add_identity(
    ordered: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    value: &str,
) {
    let trimmed = value.trim();
    if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
        ordered.push(trimmed.to_string());
    }
}

/// 完整原生挂载引用有独立授权生命周期；这里只分类身份，不授予读取权限。
/// 裸路径、错配路径和显式会话附件仍交给原有归属校验。
fn is_native_mount_path_object(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    fn descriptor(map: &serde_json::Map<String, serde_json::Value>) -> Option<&str> {
        let get = |key: &str| map.get(key).and_then(serde_json::Value::as_str);
        if get("kind") != Some("mount") || get("provider") != Some("mount") {
            return None;
        }
        let id = get("mountId")?;
        let path = get("path")?;
        let file = get("filePath")?;
        if id.trim().is_empty()
            || file.contains('\0')
            || path.contains('\0')
            || path.contains('\\')
            || (!path.is_empty()
                && path
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == ".."))
            || !std::path::Path::new(file).is_absolute()
        {
            return None;
        }
        Some(file)
    }
    if map.contains_key("realPath") {
        return false;
    }
    if descriptor(map).is_some() {
        return true;
    }
    let Some(resource) = map.get("resource").and_then(serde_json::Value::as_object) else {
        return false;
    };
    let Some(path) = descriptor(resource) else {
        return false;
    };
    map.get("filePath").and_then(serde_json::Value::as_str) == Some(path)
}

fn collect_identities(
    value: &serde_json::Value,
    ordered: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    native_resources: bool,
) {
    let native_resources = native_resources
        && value
            .get("target")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|target| {
                matches!(
                    target,
                    "tool:first-party:file" | "tool:first-party:materialize"
                )
            });
    match value {
        serde_json::Value::String(text) => collect_from_text(text, ordered, seen),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_identities(item, ordered, seen, native_resources);
            }
        }
        serde_json::Value::Object(map) => {
            collect_from_object(map, ordered, seen, native_resources);
            for item in map.values() {
                collect_identities(item, ordered, seen, native_resources);
            }
        }
        _ => {}
    }
}

fn collect_from_object(
    map: &serde_json::Map<String, serde_json::Value>,
    ordered: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    native_resources: bool,
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
    if file_id.is_some()
        || explicit_type
        || (has_path && !(native_resources && is_native_mount_path_object(map)))
    {
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
fn collect_from_text(
    text: &str,
    ordered: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
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
                    collect_from_object(&map, ordered, seen, false);
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

/// 活动只前进；时钟回退不会把近期会话变冷。
pub(crate) fn touch_on_connection(
    conn: &rusqlite::Connection,
    session_id: &str,
    now: u64,
) -> Result<(), StorageError> {
    conn.execute("UPDATE sessions SET last_activity_unix_ms=MAX(COALESCE(last_activity_unix_ms,0),?2) WHERE session_id=?1", rusqlite::params![session_id,now as i64]).map_err(map_err)?;
    Ok(())
}

pub(crate) fn files_on_connection(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<SessionFileRow>, StorageError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {FILE_COLS} FROM session_files WHERE owner_session_id=?1"
        ))
        .map_err(map_err)?;
    let rows = stmt
        .query_map([session_id], read_raw_file_row)
        .map_err(map_err)?;
    rows.map(|row| decode_raw_file_row(row.map_err(map_err)?))
        .collect()
}

pub fn reference_identities(value: &serde_json::Value) -> Vec<String> {
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_identities(value, &mut ids, &mut seen, false);
    ids
}

/// 模型参数只表示意图，不是附件。原始档案不改；引用索引和分叉复用这一裁剪。
pub(crate) fn model_history_reference_value(value: &serde_json::Value) -> serde_json::Value {
    let mut references = value.clone();
    if let Some(items) = references
        .pointer_mut("/payload/items")
        .and_then(serde_json::Value::as_array_mut)
    {
        for item in items {
            if item["kind"] != "assistant" {
                continue;
            }
            if let Some(object) = item.as_object_mut() {
                object.remove("calls");
            }
        }
    }
    references
}

fn native_reference_identities(value: &serde_json::Value) -> Vec<String> {
    let mut ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_identities(value, &mut ids, &mut seen, true);
    ids
}

/// 消息和引用同事务登记。只解析当前会话的身份，不将另一会话的旧别名提升为授权。
pub(crate) fn sync_message_refs(
    conn: &rusqlite::Connection,
    session_id: &str,
    message_id: &str,
    content: &str,
    now: u64,
) -> Result<(), StorageError> {
    // 只有真实已存模型档案、且来源Run仍属于同一用户，才能保存原生Mount来源。
    // 普通正文/附件不能靠一个resource字段绕过原有会话文件归属检查。
    let archive=conn.query_row("SELECT EXISTS(SELECT 1 FROM messages m JOIN sessions s ON s.session_id=m.session_id JOIN runs r ON r.run_id=json_extract(CASE WHEN json_valid(m.content_json) THEN m.content_json ELSE '{}' END,'$.sourceRun') WHERE m.session_id=?1 AND m.message_id=?2 AND m.role='model_history' AND m.entry_type='lingxi-model-exchange-v1' AND json_extract(CASE WHEN json_valid(m.content_json) THEN m.content_json ELSE '{}' END,'$.schema')='lingxi-model-exchange-v1' AND json_extract(CASE WHEN json_valid(m.content_json) THEN m.content_json ELSE '{}' END,'$.owner')=s.owner_user_id AND r.owner_subject=s.owner_user_id AND r.session_id=json_extract(CASE WHEN json_valid(m.content_json) THEN m.content_json ELSE '{}' END,'$.sourceSession'))",rusqlite::params![session_id,message_id],|row|row.get::<_,bool>(0)).map_err(map_err)?;
    if archive {
        let value: serde_json::Value = serde_json::from_str(content)
            .map_err(|error| invalid(format!("model history reference JSON: {error}")))?;
        let native = value
            .pointer("/payload/projection/target")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|target| {
                matches!(
                    target,
                    "tool:first-party:file" | "tool:first-party:materialize"
                )
            });
        let references = model_history_reference_value(&value).to_string();
        return sync_refs_inner(conn, session_id, message_id, &references, now, native, true);
    }
    // 正式最终答复也可能回显未知编号；只有实际Run的固定终稿行享有此分类。
    let model_reply=conn.query_row("SELECT EXISTS(SELECT 1 FROM messages m JOIN sessions s ON s.session_id=m.session_id JOIN runs r ON r.run_id=m.run_id WHERE m.session_id=?1 AND m.message_id=?2 AND m.message_id=r.run_id||'-final' AND m.role='assistant' AND m.entry_type='message' AND r.owner_subject=s.owner_user_id)",rusqlite::params![session_id,message_id],|row|row.get::<_,bool>(0)).map_err(map_err)?;
    sync_refs_inner(
        conn,
        session_id,
        message_id,
        content,
        now,
        false,
        model_reply,
    )
}

/// 分叉副本不持有来源Run外键；分类必须读取实际来源行，不能靠正文中的role自报。
pub(crate) fn sync_forked_message_refs(
    conn: &rusqlite::Connection,
    source_session: &str,
    target_session: &str,
    message_id: &str,
    content: &str,
    now: u64,
) -> Result<(), StorageError> {
    let facts=conn.query_row("SELECT m.role,m.entry_type FROM messages m JOIN sessions s ON s.session_id=m.session_id JOIN sessions t ON t.session_id=?2 WHERE m.session_id=?1 AND m.message_id=?3 AND s.owner_user_id=t.owner_user_id",rusqlite::params![source_session,target_session,message_id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?))).optional().map_err(map_err)?
        .ok_or_else(|| invalid("fork_reference_source_not_owned".into()))?;
    let archive = facts.0 == "model_history" && facts.1 == "lingxi-model-exchange-v1";
    let reply = facts.0 == "assistant" && facts.1 == "message";
    if archive {
        let value: serde_json::Value = serde_json::from_str(content)
            .map_err(|error| invalid(format!("fork history reference JSON: {error}")))?;
        let native = value
            .pointer("/payload/projection/target")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|target| {
                matches!(
                    target,
                    "tool:first-party:file" | "tool:first-party:materialize"
                )
            });
        return sync_refs_inner(
            conn,
            target_session,
            message_id,
            &model_history_reference_value(&value).to_string(),
            now,
            native,
            true,
        );
    }
    sync_refs_inner(conn, target_session, message_id, content, now, false, reply)
}

/// 仅由真实ToolCallCompleted提交入口调用；不是正文可选标记。
pub(crate) fn sync_native_tool_result_refs(
    conn: &rusqlite::Connection,
    session_id: &str,
    message_id: &str,
    content: &str,
    now: u64,
    run_id: Option<&str>,
    tool_call_id: &str,
) -> Result<(), StorageError> {
    let native=conn.query_row("SELECT EXISTS(SELECT 1 FROM key_events WHERE session_id=?1 AND run_id=?2 AND event_type='tool_call_started' AND json_extract(payload_json,'$.toolCall.toolCallId')=?3 AND json_extract(payload_json,'$.toolCall.target') IN ('tool:first-party:file','tool:first-party:materialize'))",rusqlite::params![session_id,run_id,tool_call_id],|row|row.get::<_,bool>(0)).map_err(map_err)?;
    sync_refs_inner(conn, session_id, message_id, content, now, native, true)
}
fn sync_refs_inner(
    conn: &rusqlite::Connection,
    session_id: &str,
    message_id: &str,
    content: &str,
    now: u64,
    native: bool,
    model_candidates: bool,
) -> Result<(), StorageError> {
    let value: serde_json::Value = match serde_json::from_str(content) {
        Ok(value) => value,
        Err(error) => {
            // 原历史允许原样文字；只有明确宣称结构化附件的损坏输入才拒绝。
            let structured = content.trim_start().starts_with(['{', '[']);
            let declares_file = [
                "\"fileId\"",
                "\"filePath\"",
                "\"realPath\"",
                "\"session_file\"",
                "\"session-file\"",
            ]
            .iter()
            .any(|key| content.contains(key));
            if structured && declares_file {
                return Err(invalid(format!(
                    "malformed_session_file_reference: {error}"
                )));
            }
            serde_json::Value::String(content.to_string())
        }
    };
    let ids = if native {
        native_reference_identities(&value)
    } else {
        reference_identities(&value)
    };
    if ids.is_empty() {
        return touch_on_connection(conn, session_id, now);
    }
    let files = files_on_connection(conn, session_id)?;
    for identity in ids {
        let file = files.iter().find(|r| {
            r.file_id == identity
                || r.legacy_file_ids.contains(&identity)
                || r.file_path == identity
                || r.real_path == identity
                || r.legacy_file_paths.contains(&identity)
        });
        let Some(file) = file else {
            // 模型文字和工具回显不是附件授权；未归属的候选只留在原始正文，不登记引用。
            if model_candidates {
                continue;
            }
            return Err(invalid(format!(
                "session_file_reference_not_owned:{identity}"
            )));
        };
        if file.status != SessionFileStatus::Available
            || !std::path::Path::new(&file.real_path).exists()
        {
            if model_candidates {
                continue;
            }
            return Err(invalid(format!(
                "session_file_reference_missing:{}",
                file.file_id
            )));
        }
        conn.execute("INSERT INTO session_file_refs (file_id,session_id,message_id,ref_kind,created_at_ms) SELECT ?1,?2,?3,'message_marker',?4 WHERE NOT EXISTS (SELECT 1 FROM session_file_refs WHERE file_id=?1 AND session_id=?2 AND message_id=?3)",rusqlite::params![file.file_id,session_id,message_id,now as i64]).map_err(map_err)?;
    }
    touch_on_connection(conn, session_id, now)
}
/// 文件副本与分支消息属于同一提交。失败只补偿本次新建的副本，不触碰源文件。
#[derive(Default)]
pub(crate) struct ForkCopies {
    paths: Vec<std::path::PathBuf>,
    committed: bool,
}
impl ForkCopies {
    pub(crate) fn commit(&mut self) {
        self.committed = true;
    }
}
impl Drop for ForkCopies {
    fn drop(&mut self) {
        if !self.committed {
            for path in &self.paths {
                let result = if path.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                };
                if let Err(err) = result {
                    if err.kind() != std::io::ErrorKind::NotFound {
                        tracing::error!("fork 文件补偿失败: {err}");
                    }
                }
            }
        }
    }
}

pub(crate) fn fork_files_on_connection(
    conn: &rusqlite::Connection,
    source: &str,
    target: &str,
    retained: &[String],
    home: &std::path::Path,
    now: u64,
    copies: &mut ForkCopies,
) -> Result<(), StorageError> {
    let files = files_on_connection(conn, source)?;
    let owner = format!("id:{target}");
    let hash = sha256_hex(owner.as_bytes());
    let dest_dir = home.join("session-files").join(&hash[..24]);
    let mut used = std::collections::HashSet::new();
    for mut file in files {
        let reachable = retained.iter().any(|id| {
            file.file_id == *id
                || file.legacy_file_ids.contains(id)
                || file.file_path == *id
                || file.real_path == *id
                || file.legacy_file_paths.contains(id)
        });
        if !reachable {
            continue;
        }
        let old_id = file.file_id.clone();
        let old_path = file.file_path.clone();
        let old_real = file.real_path.clone();
        if file.status != SessionFileStatus::Available || !std::path::Path::new(&old_real).exists()
        {
            return Err(invalid(format!("fork_attachment_missing:{old_id}")));
        }
        if file.storage_kind == StorageKind::ManagedCache {
            let name = std::path::Path::new(&file.filename)
                .file_name()
                .ok_or_else(|| invalid("fork filename invalid".into()))?
                .to_string_lossy()
                .to_string();
            let mut n = 0;
            let mut dest = dest_dir.join(format!("{old_id}-{name}"));
            while dest.exists() || !used.insert(dest.clone()) {
                n += 1;
                dest = dest_dir.join(format!("{old_id}-{n}-{name}"));
            }
            std::fs::create_dir_all(&dest_dir).map_err(|e| invalid(e.to_string()))?;
            copies.paths.push(dest.clone());
            copy_fork_payload(std::path::Path::new(&old_real), &dest)?;
            file.file_path = dest.to_string_lossy().into();
            file.real_path = file.file_path.clone();
        }
        let canonical =
            std::fs::canonicalize(&file.real_path).map_err(|e| invalid(e.to_string()))?;
        file.identity_key = canonical.to_string_lossy().into();
        #[cfg(windows)]
        {
            file.identity_key = file.identity_key.to_lowercase();
        }
        let identity = serde_json::json!([
            owner,
            file.source_key.as_deref().unwrap_or(&file.identity_key)
        ])
        .to_string();
        file.file_id = format!("sf_{}", &sha256_hex(identity.as_bytes())[..16]);
        file.owner_key = owner.clone();
        file.owner_session_id = target.into();
        if !file.legacy_file_ids.contains(&old_id) {
            file.legacy_file_ids.push(old_id);
        }
        for old in [old_path, old_real] {
            if !file.legacy_file_paths.contains(&old) {
                file.legacy_file_paths.push(old);
            }
        }
        file.registered_at_ms = now;
        file.updated_at_ms = now;
        insert_file_on_connection(conn, &file)?;
    }
    touch_on_connection(conn, target, now)
}

fn copy_fork_payload(
    source: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), StorageError> {
    let meta = std::fs::symlink_metadata(source).map_err(|e| invalid(e.to_string()))?;
    if meta.file_type().is_symlink() {
        return Err(invalid("fork symlink payload refused".into()));
    }
    if meta.is_dir() {
        std::fs::create_dir(target).map_err(|e| invalid(e.to_string()))?;
        for entry in std::fs::read_dir(source).map_err(|e| invalid(e.to_string()))? {
            let entry = entry.map_err(|e| invalid(e.to_string()))?;
            copy_fork_payload(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else if meta.is_file() {
        let mut out = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(target)
            .map_err(|e| invalid(e.to_string()))?;
        let mut input = std::fs::File::open(source).map_err(|e| invalid(e.to_string()))?;
        std::io::copy(&mut input, &mut out).map_err(|e| invalid(e.to_string()))?;
        out.sync_all().map_err(|e| invalid(e.to_string()))?;
    } else {
        return Err(invalid("fork payload is not a file or directory".into()));
    }
    Ok(())
}

pub type MaintenanceObserver = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

/// 跨会话保护实际被删除的名字及已交付目录后代；符号链接由真实规范路径归一。
/// 独立硬链接仍有自己的可读名字，删除无引用的托管名字不会抽走它的字节。
fn referenced_payload_paths(
    conn: &rusqlite::Connection,
) -> Result<Vec<(std::path::PathBuf, bool)>, StorageError> {
    let mut stmt = conn.prepare("SELECT real_path,file_path FROM session_files f WHERE EXISTS (SELECT 1 FROM session_file_refs r WHERE r.file_id=f.file_id) OR EXISTS (SELECT 1 FROM runs t WHERE t.session_id=f.owner_session_id AND t.status IN ('running','queued','waiting_approval','cancelling'))").map_err(map_err)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(map_err)?;
    let mut paths = Vec::new();
    for row in rows {
        let (real, stored) = row.map_err(map_err)?;
        let path = std::path::PathBuf::from(if real.is_empty() { stored } else { real });
        let canonical = match std::fs::canonicalize(&path) {
            Ok(path) => path,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => path,
            Err(err) => {
                return Err(invalid(format!(
                    "referenced payload inspection failed: {err}"
                )))
            }
        };
        let is_directory = match std::fs::metadata(&canonical) {
            Ok(metadata) => metadata.is_dir(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
            Err(err) => {
                return Err(invalid(format!(
                    "referenced payload type inspection failed: {err}"
                )))
            }
        };
        paths.push((canonical, is_directory));
    }
    Ok(paths)
}

fn deletion_would_remove_reference(
    candidate: &std::path::Path,
    referenced: &[(std::path::PathBuf, bool)],
) -> bool {
    #[cfg(windows)]
    let normalized_candidate = std::path::PathBuf::from(candidate.to_string_lossy().to_lowercase());
    #[cfg(windows)]
    let candidate = normalized_candidate.as_path();
    referenced.iter().any(|(path, is_directory)| {
        #[cfg(windows)]
        let normalized_path = std::path::PathBuf::from(path.to_string_lossy().to_lowercase());
        #[cfg(windows)]
        let path = &normalized_path;
        // 回收目录不得抽走被引子项；已交付真实目录也保护其实际托管子项。
        path.starts_with(candidate) || (*is_directory && candidate.starts_with(path))
    })
}

/// 在单写队列内从引用检查一直执行到实际删除；中途取消调用者不会拆开窗口。
pub async fn cleanup_cold_payloads(
    db: &RunDatabase,
    root: std::path::PathBuf,
    cutoff: u64,
    now: u64,
    observer: Option<MaintenanceObserver>,
) -> Result<(Vec<String>, usize), StorageError> {
    let lease = db.file_payload_leases().write_owned().await;
    let timeout = db.options().busy_timeout_ms;
    db.queue().submit(move|conn|{
        let _lease=lease;
        super::run_store::with_write_txn(conn,timeout,|conn|{
        let mut stmt=conn.prepare(&format!("SELECT {FILE_COLS} FROM session_files WHERE storage_kind='managed_cache' AND status!='expired' AND (NOT EXISTS (SELECT 1 FROM sessions s WHERE s.session_id=session_files.owner_session_id) OR owner_session_id IN (SELECT session_id FROM sessions WHERE last_activity_unix_ms IS NOT NULL AND last_activity_unix_ms<?1 AND lifecycle!='deleted'))")).map_err(map_err)?;
        let rows=stmt.query_map([cutoff as i64],read_raw_file_row).map_err(map_err)?;
        let files:Vec<SessionFileRow>=rows.map(|r|decode_raw_file_row(r.map_err(map_err)?)).collect::<Result<_,_>>()?;
        drop(stmt);
        let referenced = referenced_payload_paths(conn)?;
        let root=std::fs::canonicalize(&root).unwrap_or(root);
        let mut expired=Vec::new();let mut deleted=0;
        for file in files {
            // 冷会话的历史引用也有效。只有真正无引用的可重建缓存才回收。
            let refs:i64=conn.query_row("SELECT COUNT(*) FROM session_file_refs WHERE file_id=?1",[&file.file_id],|r|r.get(0)).map_err(map_err)?;
            let active:i64=conn.query_row("SELECT COUNT(*) FROM runs WHERE session_id=?1 AND status IN ('running','queued','waiting_approval','cancelling')",[&file.owner_session_id],|r|r.get(0)).map_err(map_err)?;
            if refs>0||active>0{continue;}
            if let Some(observer)=&observer { observer(&file.file_id); }
            let path=std::path::PathBuf::from(if file.real_path.is_empty(){&file.file_path}else{&file.real_path});
            let canonical=std::fs::canonicalize(&path).unwrap_or_else(|_|path.clone());
            if !canonical.starts_with(&root)||canonical==root{return Err(invalid("managed cache target outside root".into()));}
            if deletion_would_remove_reference(&canonical,&referenced){continue;}
            // 父目录的符号链接不能将删除带出所有权根。
            let parent=path.parent().ok_or_else(||invalid("cache target has no parent".into()))?;
            let parent=std::fs::canonicalize(parent).map_err(|e|invalid(e.to_string()))?;
            if !parent.starts_with(&root){return Err(invalid("managed cache parent outside root".into()));}
            match std::fs::symlink_metadata(&path) {
                Ok(meta)=>{
                    if meta.file_type().is_symlink(){return Err(invalid("managed cache symlink refused".into()));}
                    let result=if meta.is_dir(){std::fs::remove_dir_all(&path)}else{std::fs::remove_file(&path)};
                    result.map_err(|e|invalid(format!("cache removal failed: {e}")))?;deleted+=1;
                }
                Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},
                Err(e)=>return Err(invalid(e.to_string())),
            }
            conn.execute("UPDATE session_files SET status='expired',expires_at_ms=?2,updated_at_ms=?2 WHERE file_id=?1",rusqlite::params![file.file_id,now as i64]).map_err(map_err)?;
            // 会话已删的托管身份只在字节删除和登记成功后移除，失败时保留可恢复凭据。
            let orphan: bool = conn.query_row("SELECT NOT EXISTS (SELECT 1 FROM sessions WHERE session_id=?1)",[&file.owner_session_id],|r|r.get(0)).map_err(map_err)?;
            if orphan {
                conn.execute("DELETE FROM session_file_aliases WHERE canonical_file_id=?1",[&file.file_id]).map_err(map_err)?;
                conn.execute("DELETE FROM session_files WHERE file_id=?1",[&file.file_id]).map_err(map_err)?;
            }
            expired.push(file.file_id);
        }
        Ok((expired,deleted))
        })
    }).await
}

/// 运行期的真实活动单调前进；历史导入的原 upsert 接口保留显式历史时刻。
pub async fn record_current_activity(
    db: &RunDatabase,
    session_id: &str,
    now: u64,
) -> Result<(), StorageError> {
    let sid = session_id.to_string();
    db.queue().submit(move|conn|{
        conn.execute("INSERT INTO sessions (session_id,agent_id,owner_user_id,title,created_at_unix_ms,last_activity_unix_ms) VALUES (?1,'agent','user_local','',?2,?2) ON CONFLICT(session_id) DO UPDATE SET last_activity_unix_ms=MAX(COALESCE(sessions.last_activity_unix_ms,0),?2)",rusqlite::params![sid,now as i64]).map_err(map_err)?;Ok(())
    }).await
}

/// 兼容调用也通过同一个事务复制；租约随实际队列任务到退出。
pub async fn fork_file_subset(
    db: &RunDatabase,
    source: String,
    target: String,
    retained: Vec<String>,
    home: std::path::PathBuf,
    now: u64,
) -> Result<Vec<String>, StorageError> {
    let lease = db.file_payload_leases().read_owned().await;
    let timeout = db.options().busy_timeout_ms;
    db.queue().submit(move|conn|{
        let _lease=lease;
        let mut copies=ForkCopies::default();
        let ids=super::run_store::with_write_txn(conn,timeout,|conn|{
            fork_files_on_connection(conn,&source,&target,&retained,&home,now,&mut copies)?;
            let files=files_on_connection(conn,&target)?;
            for file in &files {
                conn.execute("INSERT INTO session_file_refs (file_id,session_id,ref_kind,created_at_ms) SELECT ?1,?2,'import',?3 WHERE NOT EXISTS (SELECT 1 FROM session_file_refs WHERE file_id=?1 AND session_id=?2 AND message_id IS NULL AND ref_kind='import')",rusqlite::params![file.file_id,target,now as i64]).map_err(map_err)?;
            }
            Ok(files.into_iter().map(|f|f.file_id).collect())
        })?;
        copies.commit();Ok(ids)
    }).await
}

/// 托管字节和登记在实际写队列同一闭包内完成，调用者取消不拆开二者。
pub async fn register_owned_blob(
    db: &RunDatabase,
    row: SessionFileRow,
    bytes: Vec<u8>,
    lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> Result<SessionFileRow, StorageError> {
    register_owned_blob_inner(db, row, bytes, lease, false, None).await
}

/// Bridge 不使用导入场景的会话补行；写入字节前在同一事务确认真实活动会话仍在。
pub async fn register_owned_bridge_blob(
    db: &RunDatabase,
    row: SessionFileRow,
    bytes: Vec<u8>,
    lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> Result<SessionFileRow, StorageError> {
    register_owned_blob_inner(db, row, bytes, lease, true, None).await
}

/// 媒体字节、会话文件和可见交付摘要在同一真实事务提交。
pub async fn register_owned_media_blob(
    db: &RunDatabase,
    row: SessionFileRow,
    bytes: Vec<u8>,
    lease: tokio::sync::OwnedRwLockReadGuard<()>,
    guard: super::media::MediaPublicationGuard,
) -> Result<SessionFileRow, StorageError> {
    register_owned_blob_inner(db, row, bytes, lease, true, Some(guard)).await
}

async fn register_owned_blob_inner(
    db: &RunDatabase,
    row: SessionFileRow,
    bytes: Vec<u8>,
    lease: tokio::sync::OwnedRwLockReadGuard<()>,
    require_active_session: bool,
    media: Option<super::media::MediaPublicationGuard>,
) -> Result<SessionFileRow, StorageError> {
    let timeout = db.options().busy_timeout_ms;
    db.queue()
        .submit(move |conn| {
            let _lease = lease;
            let mut copies = ForkCopies::default();
            let stored = super::run_store::with_write_txn(conn, timeout, |conn| {
                if let Some(guard)=media.as_ref(){super::media::verify_publication(conn,guard)?;}
                if require_active_session {
                    let active: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id=?1 AND lifecycle='active')",[&row.owner_session_id],|row|row.get(0)).map_err(map_err)?;
                    if !active {
                        return Err(invalid("bridge_blob_session_missing_or_inactive".into()));
                    }
                }
                let existing = files_on_connection(conn, &row.owner_session_id)?
                    .into_iter()
                    .find(|f| f.source_key == row.source_key);
                if let Some(file) = &existing {
                    if std::path::Path::new(&file.real_path).exists() {
                        if file.status == SessionFileStatus::Missing
                            && file.storage_kind == StorageKind::ManagedCache
                        {
                            // 已有可信托管名字的合法恢复不扩大所有权，先亲验请求字节一致。
                            use std::io::Read as _;
                            let input = std::fs::File::open(&file.real_path).map_err(|e| invalid(e.to_string()))?;
                            let mut restored = Vec::new();
                            input.take((bytes.len() as u64).saturating_add(1)).read_to_end(&mut restored).map_err(|e| invalid(e.to_string()))?;
                            if restored == bytes {
                                conn.execute("UPDATE session_files SET status='available',expires_at_ms=NULL,updated_at_ms=?2 WHERE file_id=?1", rusqlite::params![file.file_id,row.updated_at_ms as i64]).map_err(map_err)?;
                                touch_on_connection(conn, &row.owner_session_id, row.updated_at_ms)?;
                                let mut file = file.clone();
                                file.status = SessionFileStatus::Available;
                                file.expires_at_ms = None;
                                file.updated_at_ms = row.updated_at_ms;
                                if let Some(guard)=media.as_ref(){super::media::commit_publication(conn,guard,&file)?;}
                                return Ok(file);
                            }
                        }
                        if file.status != SessionFileStatus::Available {
                            return Err(invalid("unavailable_blob_payload_still_exists".into()));
                        }
                        touch_on_connection(conn, &row.owner_session_id, row.updated_at_ms)?;
                        if let Some(guard)=media.as_ref(){super::media::commit_publication(conn,guard,file)?;}
                        return Ok(file.clone());
                    }
                }
                let mut row = row;
                if let Some(file) = &existing {
                    // 真正重新创建副本才可恢复过期身份；旧引用和别名继续指向同一个 id。
                    row.file_id = file.file_id.clone();
                    row.registered_at_ms = file.registered_at_ms;
                    row.legacy_file_ids = file.legacy_file_ids.clone();
                    row.legacy_file_paths = file.legacy_file_paths.clone();
                    for path in [&file.file_path, &file.real_path] {
                        if !row.legacy_file_paths.contains(path) {
                            row.legacy_file_paths.push(path.clone());
                        }
                    }
                }
                let path = std::path::PathBuf::from(&row.real_path);
                let parent = path.parent().ok_or_else(|| invalid("blob parent missing".into()))?;
                std::fs::create_dir_all(parent).map_err(|e| invalid(e.to_string()))?;
                let mut file = std::fs::OpenOptions::new().create_new(true).write(true).open(&path).map_err(|e| invalid(e.to_string()))?;
                copies.paths.push(path);
                std::io::Write::write_all(&mut file, &bytes).map_err(|e| invalid(e.to_string()))?;
                file.sync_all().map_err(|e| invalid(e.to_string()))?;
                if existing.is_some() {
                    let changed = conn.execute("UPDATE session_files SET identity_key=?2,file_path=?3,real_path=?4,storage_kind='managed_cache',status='available',label=?5,filename=?6,mime=?7,size_bytes=?8,mtime_ms=?9,is_directory=0,file_kind=?10,origin=?11,updated_at_ms=?12,expires_at_ms=NULL,legacy_file_ids_json=?13,legacy_file_paths_json=?14 WHERE file_id=?1 AND owner_session_id=?15 AND source_key=?16",rusqlite::params![row.file_id,row.identity_key,row.file_path,row.real_path,row.label,row.filename,row.mime,row.size_bytes as i64,row.mtime_ms as i64,row.file_kind.wire(),row.origin.wire(),row.updated_at_ms as i64,serde_json::to_string(&row.legacy_file_ids).map_err(|e|invalid(e.to_string()))?,serde_json::to_string(&row.legacy_file_paths).map_err(|e|invalid(e.to_string()))?,row.owner_session_id,row.source_key]).map_err(map_err)?;
                    if changed != 1 {
                        return Err(invalid("owned_blob_recovery_identity_changed".into()));
                    }
                } else {
                    insert_file_on_connection(conn, &row)?;
                }
                touch_on_connection(conn, &row.owner_session_id, row.updated_at_ms)?;
                if let Some(guard)=media.as_ref(){super::media::commit_publication(conn,guard,&row)?;}
                Ok(row)
            })?;
            copies.commit();
            Ok(stored)
        })
        .await
}
