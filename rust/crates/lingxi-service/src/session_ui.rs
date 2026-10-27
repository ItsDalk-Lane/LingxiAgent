//! 会话页面独有资料；模型与思考档位由聊天配置的唯一存储负责。
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::StorageError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionMetadata {
    pub session_id: String,
    pub cwd: Option<String>,
    #[serde(default)]
    pub workspace_folders: Vec<String>,
    pub project_id: Option<String>,
}
fn io(error: impl std::fmt::Display) -> StorageError {
    StorageError::Io {
        detail: format!("session workspace metadata: {error}"),
    }
}
fn path(storage: &RunDatabase, session: &str) -> Result<PathBuf, StorageError> {
    Ok(storage
        .db_path()
        .parent()
        .ok_or_else(|| io("missing database parent"))?
        .join("session-ui")
        .join(format!(
            "{}.json",
            lingxi_kernel::session_tree::sha256_hex(session.as_bytes())
        )))
}
pub(crate) fn read(
    storage: &RunDatabase,
    session: &str,
) -> Result<Option<SessionMetadata>, StorageError> {
    let path = path(storage, session)?;
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(error)),
        Ok(_) => {}
    }
    let snapshot = crate::file_safety::Snapshot::read(&path).map_err(io)?;
    let value: SessionMetadata = serde_json::from_slice(&snapshot.bytes).map_err(io)?;
    if value.session_id != session {
        return Err(io("stored session identity mismatch"));
    }
    if value
        .cwd
        .as_ref()
        .is_some_and(|cwd| !std::path::Path::new(cwd).is_absolute())
        || value
            .workspace_folders
            .iter()
            .any(|path| !std::path::Path::new(path).is_absolute())
    {
        return Err(io("stored workspace is not an absolute path"));
    }
    Ok(Some(value))
}
pub(crate) fn workspace(
    storage: &RunDatabase,
    session: &str,
) -> Result<Option<String>, StorageError> {
    Ok(read(storage, session)?.and_then(|value| value.cwd))
}
pub(crate) fn save(storage: &RunDatabase, value: &SessionMetadata) -> Result<(), StorageError> {
    if value.session_id.trim().is_empty() {
        return Err(io("session identity is required"));
    }
    for raw in value.cwd.iter().chain(value.workspace_folders.iter()) {
        let path = std::path::Path::new(raw);
        if !path.is_absolute() || !std::fs::metadata(path).map_err(io)?.is_dir() {
            return Err(io("workspace must be an existing absolute directory"));
        }
    }
    let text = serde_json::to_string_pretty(value).map_err(io)?;
    lingxi_adapters::storage::agent_memory::atomic_write(&path(storage, &value.session_id)?, &text)
        .map_err(io)
}

pub(crate) fn remove_uncreated(storage: &RunDatabase, session: &str) -> Result<(), StorageError> {
    match std::fs::remove_file(path(storage, session)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io(error)),
    }
}
