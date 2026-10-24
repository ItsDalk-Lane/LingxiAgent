//! B3 切片 C：8 个运行态 Convert 类 = 逐字节归档 + 键登记。
//! 每个批次：源文件字节原样复制到 target/legacy-import/runtime-state-archive/
//! （绝不覆盖、幂等重跑），解析后按类提取键写入 V28
//! legacy_runtime_state_registry，未知字段随完整解析 JSON 原样保留在
//! detail_json；解析失败不丢字节——归档仍生效、登记退化为整文件键 +
//! warning 级 issue。已登记条目只声明「已登记、消费端待对应任务」，不发明
//! 消费语义。

use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::{ImportError, ImportArgs, insert_issue, now_unix_ms};

/// 切片 C 覆盖的 8 个运行态类（与 classes.rs 的 Convert 注册一一对应）。
pub(crate) const RUNTIME_STATE_CLASSES: &[&str] = &[
    "conversation-map-layout",
    "deferred-result-state",
    "execution-leases",
    "loop-state",
    "plugin-task-registry",
    "subagent-state",
    "terminal-session-state",
    "workflow-state",
];

pub(crate) const ARCHIVE_DIR: &str = "legacy-import/runtime-state-archive";

/// 从解析后的 JSON 提取登记键；提不到 → 空（调用方回退为整文件键）。
fn extract_keys(store_class: &str, relpath: &str, parsed: &Value) -> Vec<String> {
    let ids = |arr: Option<&Vec<Value>>| -> Vec<String> {
        arr.map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
    };
    match store_class {
        "conversation-map-layout" => parsed
            .get("layout")
            .and_then(Value::as_object)
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default(),
        "deferred-result-state" => ids(parsed.get("tasks").and_then(Value::as_array)),
        "execution-leases" => ids(parsed.get("leases").and_then(Value::as_array)),
        "plugin-task-registry" => ids(parsed.get("tasks").and_then(Value::as_array)),
        "subagent-state" => {
            let mut keys = ids(parsed.get("runs").and_then(Value::as_array));
            keys.extend(ids(parsed.get("threads").and_then(Value::as_array)));
            keys
        }
        "terminal-session-state" => Path::new(relpath)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| vec![s.to_string()])
            .unwrap_or_default(),
        "workflow-state" => {
            let mut keys = ids(parsed.get("active").and_then(Value::as_array));
            keys.extend(ids(parsed.get("runs").and_then(Value::as_array)));
            keys
        }
        // loop-state 及其他：无内层键，整文件登记。
        _ => Vec::new(),
    }
}

/// 解析源字节：.jsonl 逐行（行号入 detail），.json 整体。失败返回 Err（含原因）。
fn parse_source(relpath: &str, bytes: &[u8]) -> Result<Value, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|e| format!("source is not UTF-8: {e}"))?;
    if relpath.ends_with(".jsonl") {
        let mut lines = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line)
                .map_err(|e| format!("line {} is not JSON: {e}", index + 1))?;
            lines.push(value);
        }
        Ok(Value::Array(lines))
    } else {
        serde_json::from_str(text).map_err(|e| format!("source is not JSON: {e}"))
    }
}

/// 逐字节归档（已存在且字节全等 → 幂等跳过；存在但不同 → 响亮失败）。
fn archive_bytes(source: &Path, dest: &Path) -> Result<bool, ImportError> {
    let bytes = std::fs::read(source).map_err(|e| ImportError::Io {
        path: source.to_path_buf(),
        message: e.to_string(),
    })?;
    if dest.exists() {
        let existing = std::fs::read(dest)
            .map_err(|e| ImportError::Db(format!("read archive {}: {e}", dest.display())))?;
        if existing == bytes {
            return Ok(false); // 幂等重跑：不覆盖。
        }
        return Err(ImportError::Db(format!(
            "archive target {} already exists with different bytes; refusing to overwrite",
            dest.display()
        )));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ImportError::Db(format!("mkdir {}: {e}", parent.display())))?;
    }
    std::fs::write(dest, &bytes)
        .map_err(|e| ImportError::Db(format!("write archive {}: {e}", dest.display())))?;
    Ok(true)
}

/// 执行切片 C：返回 (登记条目总数, 每类明细)。无可处理批次时返回 None。
pub(crate) fn convert_runtime_state(
    conn: &Connection,
    run_id: &str,
    args: &ImportArgs,
    seq: &mut i64,
) -> Result<Option<(u64, Vec<Value>)>, ImportError> {
    let class_list = RUNTIME_STATE_CLASSES
        .iter()
        .map(|c| format!("'{c}'"))
        .collect::<Vec<_>>()
        .join(",");
    let pending: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT batch_key, source_relpath, store_class FROM import_batches
                 WHERE run_id = ?1 AND strategy = 'convert'
                   AND store_class IN ({class_list})
                   AND state IN ('planned','in_progress') ORDER BY batch_key"
            ))
            .map_err(|e| ImportError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![run_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| ImportError::Db(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| ImportError::Db(e.to_string()))?
    };
    if pending.is_empty() {
        return Ok(None);
    }
    let mut total_entries = 0u64;
    let mut per_class: Vec<Value> = Vec::new();
    for (batch_key, relpath, store_class) in pending {
        conn.execute(
            "UPDATE import_batches SET state = 'in_progress' WHERE run_id = ?1 AND batch_key = ?2",
            rusqlite::params![run_id, batch_key],
        )
        .map_err(|e| ImportError::Db(e.to_string()))?;

        let result: Result<(Vec<String>, Value, String), ImportError> = (|| {
            let source_path = args.source.join(&relpath);
            if !source_path.exists() {
                return Err(ImportError::Refused {
                    reason: format!("source vanished between runs: {relpath}"),
                });
            }
            let bytes = std::fs::read(&source_path).map_err(|e| ImportError::Io {
                path: source_path.clone(),
                message: e.to_string(),
            })?;
            let sha256 = super::manifest::sha256_hex(&bytes);
            let archived_relpath = format!("{ARCHIVE_DIR}/{relpath}");
            archive_bytes(&source_path, &args.target.join(&archived_relpath))?;
            let (keys, detail) = match parse_source(&relpath, &bytes) {
                Ok(parsed) => {
                    let mut keys = extract_keys(&store_class, &relpath, &parsed);
                    // jsonl 场景：逐行 id 补充（workflow/terminal journal）。
                    if relpath.ends_with(".jsonl") {
                        if let Value::Array(lines) = &parsed {
                            for line in lines {
                                if let Some(id) = line.get("id").and_then(Value::as_str) {
                                    if !keys.iter().any(|k| k == id) {
                                        keys.push(id.to_string());
                                    }
                                }
                            }
                        }
                    }
                    // 未知字段原样保留：完整解析结果进 detail_json。
                    (keys, parsed)
                }
                Err(reason) => {
                    *seq += 1;
                    insert_issue(
                        conn, run_id, *seq, &batch_key, "warning", "unmapped-shape",
                        Some(&relpath),
                        &format!("runtime-state source unparsable ({reason}); raw bytes archived"),
                    )?;
                    let text = String::from_utf8_lossy(&bytes).to_string();
                    (Vec::new(), Value::String(text))
                }
            };
            let keys = if keys.is_empty() { vec![relpath.clone()] } else { keys };
            Ok((keys, detail, sha256))
        })();

        match result {
            Ok((keys, detail, sha256)) => {
                let registered_at = now_unix_ms();
                for key in &keys {
                    conn.execute(
                        "INSERT INTO legacy_runtime_state_registry
                         (run_id, store_class, entry_key, source_relpath, source_sha256,
                          archived_relpath, detail_json, registered_at_unix_ms)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                         ON CONFLICT(run_id, store_class, entry_key) DO UPDATE SET
                          source_sha256 = excluded.source_sha256,
                          archived_relpath = excluded.archived_relpath,
                          detail_json = excluded.detail_json,
                          registered_at_unix_ms = excluded.registered_at_unix_ms",
                        rusqlite::params![
                            run_id, store_class, key, relpath, sha256,
                            format!("{ARCHIVE_DIR}/{relpath}"),
                            detail.to_string(), registered_at
                        ],
                    )
                    .map_err(|e| ImportError::Db(e.to_string()))?;
                }
                total_entries += keys.len() as u64;
                conn.execute(
                    "UPDATE import_batches SET state = 'done', rows_planned = ?3, rows_done = ?3,
                        detail_json = ?4 WHERE run_id = ?1 AND batch_key = ?2",
                    rusqlite::params![
                        run_id, batch_key, keys.len() as i64,
                        serde_json::json!({
                            "archivedTo": format!("{ARCHIVE_DIR}/{relpath}"),
                            "sha256": sha256,
                            "keys": keys,
                        }).to_string()
                    ],
                )
                .map_err(|e| ImportError::Db(e.to_string()))?;
                per_class.push(serde_json::json!({
                    "class": store_class,
                    "sourceRelpath": relpath,
                    "entries": keys.len(),
                    "status": "archived+registered",
                }));
            }
            Err(err) => {
                let message = err.to_string();
                *seq += 1;
                insert_issue(
                    conn, run_id, *seq, &batch_key, "error", "batch-error",
                    Some(&relpath), &message,
                )?;
                conn.execute(
                    "UPDATE import_batches SET state = 'failed' WHERE run_id = ?1 AND batch_key = ?2",
                    rusqlite::params![run_id, batch_key],
                )
                .map_err(|e| ImportError::Db(e.to_string()))?;
                per_class.push(serde_json::json!({
                    "class": store_class,
                    "sourceRelpath": relpath,
                    "status": "failed",
                    "error": message,
                }));
            }
        }
    }
    Ok(Some((total_entries, per_class)))
}

/// 阶段出口明细里的归档根（供收据/断言引用）。
#[allow(dead_code)]
pub(crate) fn archive_path(target: &Path, relpath: &str) -> PathBuf {
    target.join(ARCHIVE_DIR).join(relpath)
}
