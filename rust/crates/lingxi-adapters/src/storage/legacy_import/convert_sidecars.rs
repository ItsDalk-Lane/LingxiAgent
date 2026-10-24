//! R08-T04 B2 切片 A（续）：session-sidecars（meta/titles 权威标题/归档）与
//! session-manifest-sqlite（只读校验）转换器。
//!
//! 纪律：
//! - sidecar 永不反写源文件；不认识的顶层键收进 detail_json `unhandledKeys`，不静默吞。
//! - 版本非 1 → error issue `unsupported-sidecar-version` + 批次 failed。
//! - 指向不存在会话的条目 → warning `sidecar-orphan`，其余条目照常应用。
//! - manifest.db 只读打开校验：manifest 有而目标无 → warning `manifest-mismatch`；
//!   目标多出 manifest 的会话只计入 detail（导入面本就允许新增）。
//! - -wal/-shm 批次不独立处理：随主库只读打开隐式生效 → 批次 skipped + 说明（响亮的）。

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

use super::ImportError;

use super::{insert_issue, now_unix_ms};

#[derive(Debug, Default)]
pub struct SidecarStats {
    /// 处理的会话条目数（rows_done 基准）。
    pub entries: u64,
    pub titles_applied: u64,
    pub archived_applied: u64,
    pub orphans: u64,
    pub unhandled_keys: Vec<String>,
    /// true = 不导入语义（如 bridge-sessions/vision-notes 仅登记）。
    pub register_only: bool,
}

fn apply_title(
    conn: &Connection,
    run_id: &str,
    batch_key: &str,
    session_id: &str,
    title: &str,
    stats: &mut SidecarStats,
    seq: &mut i64,
) -> Result<(), ImportError> {
    let changed = conn
        .execute(
            "UPDATE sessions SET title = ?2 WHERE session_id = ?1",
            rusqlite::params![session_id, title],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    if changed == 0 {
        *seq += 1;
        insert_issue(
            conn,
            run_id,
            *seq,
            batch_key,
            "warning",
            "sidecar-orphan",
            Some(session_id),
            &format!(
                "sidecar references session '{session_id}' that has no imported row; \
                 title '{title}' not applied"
            ),
        )?;
        stats.orphans += 1;
    } else {
        stats.titles_applied += 1;
    }
    Ok(())
}

fn apply_archived(
    conn: &Connection,
    session_id: &str,
    stats: &mut SidecarStats,
) -> Result<(), ImportError> {
    let changed = conn
        .execute(
            "UPDATE sessions SET lifecycle = 'archived',
                archived_at_unix_ms = COALESCE(archived_at_unix_ms, ?2)
             WHERE session_id = ?1",
            rusqlite::params![session_id, now_unix_ms()],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;
    if changed > 0 {
        stats.archived_applied += 1;
    }
    Ok(())
}

/// 解析 meta 风格 `{version, sessions:{id:{title?,archived?}}}`。
/// 返回 false = 形状不认识（调用方登记 unmapped-shape）。
fn apply_meta_shape(
    conn: &Connection,
    run_id: &str,
    batch_key: &str,
    raw: &Value,
    stats: &mut SidecarStats,
    seq: &mut i64,
) -> Result<bool, ImportError> {
    let version = raw.get("version").and_then(Value::as_i64).unwrap_or(1);
    if version != 1 {
        *seq += 1;
        insert_issue(
            conn,
            run_id,
            *seq,
            batch_key,
            "error",
            "unsupported-sidecar-version",
            None,
            &format!("sidecar version {version} unsupported; only v1 is imported"),
        )?;
        return Err(ImportError::Refused {
            reason: format!("unsupported sidecar version {version} in {batch_key}"),
        });
    }
    let Some(sessions) = raw.get("sessions").and_then(Value::as_object) else {
        return Ok(false);
    };
    for (session_id, meta) in sessions {
        stats.entries += 1;
        if let Some(title) = meta.get("title").and_then(Value::as_str) {
            apply_title(conn, run_id, batch_key, session_id, title, stats, seq)?;
        }
        if meta.get("archived").and_then(Value::as_bool) == Some(true) {
            apply_archived(conn, session_id, stats)?;
        }
    }
    for key in raw
        .as_object()
        .map(|o| o.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        if key != "version" && key != "sessions" {
            stats.unhandled_keys.push(key);
        }
    }
    Ok(true)
}

fn apply_titles_shape(
    conn: &Connection,
    run_id: &str,
    batch_key: &str,
    raw: &Value,
    stats: &mut SidecarStats,
    seq: &mut i64,
) -> Result<bool, ImportError> {
    let version = raw.get("version").and_then(Value::as_i64).unwrap_or(1);
    if version != 1 {
        *seq += 1;
        insert_issue(
            conn,
            run_id,
            *seq,
            batch_key,
            "error",
            "unsupported-sidecar-version",
            None,
            &format!("sidecar version {version} unsupported; only v1 is imported"),
        )?;
        return Err(ImportError::Refused {
            reason: format!("unsupported sidecar version {version} in {batch_key}"),
        });
    }
    let Some(titles) = raw.get("titles").and_then(Value::as_object) else {
        return Ok(false);
    };
    for (session_id, title) in titles {
        stats.entries += 1;
        if let Some(title) = title.as_str() {
            apply_title(conn, run_id, batch_key, session_id, title, stats, seq)?;
        }
    }
    for key in raw
        .as_object()
        .map(|o| o.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        if key != "version" && key != "titles" {
            stats.unhandled_keys.push(key);
        }
    }
    Ok(true)
}

/// session-sidecars 批次：按文件名分派 meta/titles/其它。
pub fn convert_sidecar_file(
    conn: &Connection,
    run_id: &str,
    batch_key: &str,
    source_root: &Path,
    relpath: &str,
    seq: &mut i64,
) -> Result<SidecarStats, ImportError> {
    let mut stats = SidecarStats::default();
    let path = source_root.join(relpath);
    let bytes = std::fs::read(&path).map_err(|e| ImportError::Io {
        path: path.clone(),
        message: e.to_string(),
    })?;
    let raw: Value = serde_json::from_slice(&bytes).map_err(|e| ImportError::Refused {
        reason: format!("sidecar {relpath} is not valid JSON: {e}"),
    })?;
    let file_name = Path::new(relpath)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let handled = match file_name.as_str() {
        "session-meta.json" => apply_meta_shape(conn, run_id, batch_key, &raw, &mut stats, seq)?,
        "session-titles.json" => {
            apply_titles_shape(conn, run_id, batch_key, &raw, &mut stats, seq)?
        }
        _ => {
            // bridge-sessions.json / session-vision-notes.json / 子代理 session-meta.json：
            // 形状未核定——尝试 meta 风格；不成立则登记顶层键 + unmapped-shape 警告。
            let meta_like = apply_meta_shape(conn, run_id, batch_key, &raw, &mut stats, seq)?;
            if !meta_like {
                stats.register_only = true;
                stats.unhandled_keys = raw
                    .as_object()
                    .map(|o| o.keys().cloned().collect::<Vec<String>>())
                    .unwrap_or_default();
                *seq += 1;
                insert_issue(
                    conn,
                    run_id,
                    *seq,
                    batch_key,
                    "warning",
                    "unmapped-shape",
                    Some(relpath),
                    &format!(
                        "sidecar '{file_name}' shape not mapped in this slice; registered \
                         without semantic import (top-level keys recorded in batch detail)"
                    ),
                )?;
            }
            meta_like
        }
    };
    if !handled && file_name == "session-meta.json" || !handled && file_name == "session-titles.json"
    {
        return Err(ImportError::Refused {
            reason: format!("sidecar {relpath} lacks expected v1 shape"),
        });
    }
    Ok(stats)
}

#[derive(Debug, Default)]
pub struct ManifestStats {
    pub manifest_rows: u64,
    pub matched: u64,
    pub mismatched: u64,
    pub extra_in_target: u64,
}

/// session-manifest-sqlite 批次：只读校验主库；-wal/-shm 返回 None（skipped）。
pub fn validate_manifest_file(
    conn: &Connection,
    run_id: &str,
    batch_key: &str,
    source_root: &Path,
    relpath: &str,
    seq: &mut i64,
) -> Result<Option<ManifestStats>, ImportError> {
    if relpath.ends_with("-wal") || relpath.ends_with("-shm") {
        *seq += 1;
        insert_issue(
            conn,
            run_id,
            *seq,
            batch_key,
            "info",
            "special-file-skipped",
            Some(relpath),
            "sqlite wal/shm companion validated implicitly via the read-only open of \
             session-manifest.db; no independent import action",
        )?;
        return Ok(None);
    }
    let path = source_root.join(relpath);
    let manifest = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| ImportError::Refused {
            reason: format!("cannot open legacy manifest read-only: {e}"),
        })?;
    let rows: BTreeMap<String, (Option<String>, Option<i64>)> = {
        let mut stmt = manifest
            .prepare("SELECT session_id, agent_id, updated_at FROM sessions")
            .map_err(|e| ImportError::Refused {
                reason: format!("legacy manifest lacks sessions table: {e}"),
            })?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (row.get::<_, Option<String>>(1)?, row.get::<_, Option<i64>>(2)?),
                ))
            })
            .map_err(|e: rusqlite::Error| ImportError::Db(e.to_string()))?;
        rows.collect::<Result<BTreeMap<String, (Option<String>, Option<i64>)>, _>>()
            .map_err(|e: rusqlite::Error| ImportError::Db(e.to_string()))?
    };
    let mut stats = ManifestStats {
        manifest_rows: rows.len() as u64,
        ..ManifestStats::default()
    };
    for (session_id, (agent_id, _updated_at)) in &rows {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE session_id = ?1",
                rusqlite::params![session_id],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .map_err(|e| ImportError::Db(e.to_string()))?;
        if exists {
            stats.matched += 1;
        } else {
            stats.mismatched += 1;
            *seq += 1;
            insert_issue(
                conn,
                run_id,
                *seq,
                batch_key,
                "warning",
                "manifest-mismatch",
                Some(session_id),
                &format!(
                    "manifest lists session '{session_id}' (agent {agent_id:?}) but no \
                     imported row exists; transcript may be missing from source"
                ),
            )?;
        }
    }
    let target_total: i64 = conn
        .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
        .map_err(|e| ImportError::Db(e.to_string()))?;
    stats.extra_in_target = (target_total as u64).saturating_sub(stats.matched);
    Ok(Some(stats))
}
