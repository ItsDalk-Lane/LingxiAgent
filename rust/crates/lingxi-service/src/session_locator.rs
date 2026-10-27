//! 会话路径仅用于定位，持久身份始终来自实际数据库会话行。
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::StorageError;

pub fn native(session_id: &str) -> String {
    format!("native:{session_id}")
}

/// 同时提供 id/path 时必须一致；旧 JSONL 仅认可可信目录、真实头和数据库登记。
pub async fn resolve(
    storage: &RunDatabase,
    session_id: Option<&str>,
    session_path: Option<&str>,
) -> Result<String, StorageError> {
    let invalid = || StorageError::InvalidRequest {
        detail: "session locator does not resolve to one session".into(),
    };
    let id = session_id.map(str::trim).filter(|value| !value.is_empty());
    let from_path = match session_path.filter(|value| !value.trim().is_empty()) {
        Some(value) if value.starts_with("native:") => Some(
            value
                .strip_prefix("native:")
                .filter(|id| !id.is_empty())
                .ok_or_else(invalid)?
                .to_string(),
        ),
        Some(value) => Some(
            lingxi_adapters::storage::legacy_session_files::session_id_for_path(
                storage,
                std::path::Path::new(value),
            )
            .await?,
        ),
        None => None,
    };
    if id
        .zip(from_path.as_deref())
        .is_some_and(|(id, path_id)| id != path_id)
    {
        return Err(invalid());
    }
    let resolved = id.or(from_path.as_deref()).ok_or_else(invalid)?;
    if storage.get_session(resolved).await?.is_none() {
        return Err(invalid());
    }
    Ok(resolved.to_string())
}
