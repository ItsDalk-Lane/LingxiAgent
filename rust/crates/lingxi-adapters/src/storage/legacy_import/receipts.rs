//! R08-T04: 收据三件套（migration-receipt.json / issues.jsonl / SUMMARY.md）。
//! 全部从 V27 簿记表取数重写（幂等：同 run 重跑覆盖同路径，不追加重复行）。

use super::{EngineOutcome, ImportError, TARGET_DB_RELPATH};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn write_file(path: &Path, content: &str) -> Result<(), ImportError> {
    std::fs::write(path, content).map_err(|e| ImportError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

/// 从簿记表读取并落盘收据三件套，返回引擎摘要。
pub(crate) fn write_receipts(
    conn: &Connection,
    receipt_dir: &Path,
    run_id: &str,
    source_root_sha256: &str,
    source_unchanged: bool,
    resumed: bool,
    resumed_from_run_id: Option<&str>,
    now_unix_ms: i64,
) -> Result<EngineOutcome, ImportError> {
    let mut by_strategy: BTreeMap<String, [i64; 5]> = BTreeMap::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT strategy, state, COUNT(*) FROM import_batches
                 WHERE run_id = ?1 GROUP BY strategy, state",
            )
            .map_err(|e| ImportError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![run_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|e| ImportError::Db(e.to_string()))?;
        for row in rows {
            let (strategy, state, count) = row.map_err(|e| ImportError::Db(e.to_string()))?;
            let slot = by_strategy.entry(strategy).or_insert([0; 5]);
            match state.as_str() {
                "planned" => slot[0] += count,
                "in_progress" => slot[1] += count,
                "done" => slot[2] += count,
                "failed" => slot[3] += count,
                _ => slot[4] += count, // skipped 等
            }
        }
    }

    let batches_total: i64 = by_strategy.values().map(|s| s.iter().sum::<i64>()).sum();
    let batches_done: i64 = by_strategy.values().map(|s| s[2]).sum();
    let batches_pending_convert: i64 = by_strategy
        .get("convert")
        .map(|s| s[0] + s[1])
        .unwrap_or(0);

    let mut issues: Vec<serde_json::Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT seq, batch_key, severity, kind, ref_id, message FROM import_issues
                 WHERE run_id = ?1 ORDER BY seq",
            )
            .map_err(|e| ImportError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![run_id], |row| {
                Ok(serde_json::json!({
                    "seq": row.get::<_, i64>(0)?,
                    "batchKey": row.get::<_, String>(1)?,
                    "severity": row.get::<_, String>(2)?,
                    "kind": row.get::<_, String>(3)?,
                    "refId": row.get::<_, Option<String>>(4)?,
                    "message": row.get::<_, String>(5)?,
                }))
            })
            .map_err(|e| ImportError::Db(e.to_string()))?;
        for row in rows {
            issues.push(row.map_err(|e| ImportError::Db(e.to_string()))?);
        }
    }

    let batches_failed: i64 = by_strategy.values().map(|s| s[3]).sum();
    let batches_skipped: i64 = by_strategy.values().map(|s| s[4]).sum();
    let status: &'static str = if batches_pending_convert > 0 {
        "partial"
    } else if batches_failed > 0 {
        "failed"
    } else {
        "complete"
    };

    // B3 切片 B/C：扩展收据（V28 import_run_extensions）。空表 → 收据无
    // extensions 键（B2 形状逐字节不变）；非空按 ext_key 排序原样带出。
    let mut extensions: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT ext_key, payload_json FROM import_run_extensions
                 WHERE run_id = ?1 ORDER BY ext_key",
            )
            .map_err(|e| ImportError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![run_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| ImportError::Db(e.to_string()))?;
        for row in rows {
            let (key, payload) = row.map_err(|e| ImportError::Db(e.to_string()))?;
            let value: serde_json::Value = serde_json::from_str(&payload)
                .map_err(|e| ImportError::Db(format!("extension {key} payload unreadable: {e}")))?;
            extensions.insert(key, value);
        }
    }

    let strategy_json: serde_json::Map<String, serde_json::Value> = by_strategy
        .iter()
        .map(|(strategy, counts)| {
            (
                strategy.clone(),
                serde_json::json!({
                    "planned": counts[0],
                    "inProgress": counts[1],
                    "done": counts[2],
                    "failed": counts[3],
                    "skipped": counts[4],
                }),
            )
        })
        .collect();

    let mut receipt = serde_json::json!({
        "schema": "lingxi.legacy-import-receipt/v1",
        "runId": run_id,
        "resumed": resumed,
        "generatedAtUnixMs": now_unix_ms,
        "sourceRootSha256": source_root_sha256,
        "sourceUnchanged": source_unchanged,
        "target": {
            "db": TARGET_DB_RELPATH,
            "note": "target root mirrors the new home layout; T05 adoption consumes this receipt"
        },
        "batches": {
            "total": batches_total,
            "done": batches_done,
            "pendingConvert": batches_pending_convert,
            "failed": batches_failed,
            "skipped": batches_skipped,
            "byStrategy": serde_json::Value::Object(strategy_json),
        },
        "issues": { "count": issues.len(), "file": "issues.jsonl" },
        "status": status,
        "phase": "R08-T04 (LegacyImporter: sessionfiles/trace/cron/runtime-state/plugins/epoch/knowledge; T05 adoption consumes this receipt)"
    });
    if let Some(from) = resumed_from_run_id {
        // A08 续跑链：仅续跑时写（首轮收据形状不变）。
        receipt["resumedFromRunId"] = serde_json::json!(from);
    }
    if !extensions.is_empty() {
        receipt["extensions"] = serde_json::json!(extensions);
    }

    let receipt_path = receipt_dir.join("migration-receipt.json");
    write_file(
        &receipt_path,
        &serde_json::to_string_pretty(&receipt).unwrap_or_default(),
    )?;

    let issues_text: String = issues
        .iter()
        .map(|issue| serde_json::to_string(issue).unwrap_or_default() + "\n")
        .collect();
    write_file(&receipt_dir.join("issues.jsonl"), &issues_text)?;

    let mut summary = String::new();
    summary.push_str(&format!("# Legacy import {run_id}\n\n"));
    summary.push_str(&format!("- status: **{status}** (resumed={resumed})\n"));
    if let Some(from) = resumed_from_run_id {
        summary.push_str(&format!("- resumedFromRunId: `{from}`\n"));
    }
    summary.push_str(&format!("- sourceRootSha256: `{source_root_sha256}`\n"));
    summary.push_str(&format!("- sourceUnchanged: {source_unchanged}\n"));
    summary.push_str(&format!(
        "- batches: total={batches_total} done={batches_done} pendingConvert={batches_pending_convert} failed={batches_failed} skipped={batches_skipped}\n"
    ));
    for (strategy, counts) in &by_strategy {
        summary.push_str(&format!(
            "  - {strategy}: planned={} inProgress={} done={} failedOrSkipped={}\n",
            counts[0], counts[1], counts[2], counts[3]
        ));
    }
    summary.push_str(&format!("- issues: {} (see issues.jsonl)\n", issues.len()));
    if !extensions.is_empty() {
        summary.push_str(&format!(
            "- extensions: {}\n",
            extensions.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    summary.push_str(
        "\nB3：convert 批次含 sessionfiles 目录级实转；legacyTrace/legacyCron/运行态登记/知识库取证/旧插件识别见 extensions（若有）。\n",
    );
    write_file(&receipt_dir.join("SUMMARY.md"), &summary)?;

    Ok(EngineOutcome {
        run_id: run_id.to_string(),
        resumed,
        source_root_sha256: source_root_sha256.to_string(),
        source_unchanged,
        batches_total: batches_total as usize,
        batches_done: batches_done as usize,
        batches_pending_convert: batches_pending_convert as usize,
        issues: issues.len(),
        status,
        interrupted: false,
        receipt_path: PathBuf::from(receipt_dir.join("migration-receipt.json")),
    })
}
