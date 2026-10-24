//! R08-T04 切片 B：旧 model-observability sqlite 只读映射（v1..v7 形状）。
//! 实现自 service `observability::read_legacy_store` 原样下沉——端点改为薄壳
//! 复用同一实现，不发明第二条读链；写路径复用
//! `RunDatabase::import_legacy_model_calls`（同 call ID 幂等、冲突拒）。
//! 任何形状不符显式 Err（不静默吞行）；只读打开，源库字节永不改动。

use super::super::run_store::LegacyUsageImportRow;
use lingxi_kernel::usage::{CallOutcome, ModelCallUsageRecord};
use std::path::Path;

/// 只读打开旧库并映射成行。错误字符串与端点历史措辞逐字一致（薄壳原样
/// 包装为 400），保证行为面零漂移。
pub fn read_legacy_observability_rows(
    path: &Path,
) -> Result<Vec<LegacyUsageImportRow>, String> {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("cannot open legacy store read-only: {e}"))?;
    // 旧库 v2+ 才有 usage 投影表；v1 只有调用行。
    let has_usage: bool = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='model_call_usage'")
        .map_err(|e| format!("cannot inspect legacy schema: {e}"))?
        .exists([])
        .map_err(|e| format!("cannot inspect legacy schema: {e}"))?;
    let sql = if has_usage {
        "SELECT c.call_id, c.trace_id, c.parent_call_id, c.provider, c.model_id, c.api, \
         c.subsystem, c.call_purpose, c.session_id, c.started_at, c.ended_at, c.terminal_status, \
         u.input_total_tokens, u.output_total_tokens, u.reasoning_tokens, \
         u.cache_read_tokens, u.cache_write_tokens \
         FROM model_calls c LEFT JOIN model_call_usage u ON u.model_call_id = c.call_id \
         ORDER BY c.started_at, c.call_id"
    } else {
        "SELECT c.call_id, c.trace_id, c.parent_call_id, c.provider, c.model_id, c.api, \
         c.subsystem, c.call_purpose, c.session_id, c.started_at, c.ended_at, c.terminal_status, \
         NULL, NULL, NULL, NULL, NULL \
         FROM model_calls c ORDER BY c.started_at, c.call_id"
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("legacy schema does not match v1..v7: {e}"))?;
    let mut rows = stmt
        .query([])
        .map_err(|e| format!("legacy read failed: {e}"))?;
    let mut out = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| format!("legacy read failed: {e}"))?
    {
        let get = |i: usize| -> Option<String> { row.get::<_, Option<String>>(i).ok().flatten() };
        let call_id = get(0).ok_or_else(|| "legacy row without call_id".to_string())?;
        let started = get(9).map(|s| parse_legacy_instant(&s)).transpose()?;
        let ended = get(10).map(|s| parse_legacy_instant(&s)).transpose()?;
        let recorded = started.or(ended).unwrap_or(0);
        let outcome = match get(11).as_deref() {
            Some("ok") => CallOutcome::Succeeded,
            Some("error") => CallOutcome::Failed,
            Some("aborted") => CallOutcome::Cancelled,
            _ => CallOutcome::Unknown,
        };
        let to_u64 = |i: usize| -> Option<u64> {
            row.get::<_, Option<i64>>(i)
                .ok()
                .flatten()
                .and_then(|v| u64::try_from(v).ok())
        };
        let usage = match (to_u64(12), to_u64(13)) {
            (None, None) => None,
            (input, output) => Some(lingxi_kernel::usage::ModelCallUsage {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: to_u64(15),
                cache_write_tokens: to_u64(16),
                reasoning_tokens: to_u64(14),
                provenance: lingxi_kernel::usage::UsageProvenance::Reported,
            }),
        };
        out.push(LegacyUsageImportRow {
            record: ModelCallUsageRecord {
                session_id: get(8),
                run_id: None,
                attempt: None,
                model_call_id: call_id.clone(),
                purpose: get(7)
                    .or_else(|| get(6))
                    .unwrap_or_else(|| "unknown".to_string()),
                origin: "imported".to_string(),
                parent_run_id: None,
                cause_ref: None,
                parent_tool_call_id: None,
                provider: get(3).unwrap_or_else(|| "unknown".to_string()),
                model: get(4).unwrap_or_else(|| "unknown".to_string()),
                protocol: get(5).unwrap_or_else(|| "unknown".to_string()),
                usage,
                invalid_detail: None,
                transport_attempts: Some(1),
                outcome,
                started_at_unix_ms: started,
                settled_at_unix_ms: ended,
                emitted_tool_calls: Vec::new(),
                cost_basis: None,
            },
            recorded_at_unix_ms: recorded,
            legacy_trace_id: get(1),
            legacy_parent_call_id: get(2),
        });
    }
    Ok(out)
}

fn parse_legacy_instant(raw: &str) -> Result<u64, String> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .and_then(|t| u64::try_from(t.timestamp_millis()).ok())
        .ok_or_else(|| format!("legacy timestamp {raw:?} is not RFC3339"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t04i-obs-test-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("observability.sqlite")
    }

    fn seed_v2_store(path: &Path) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE model_calls (
               call_id TEXT PRIMARY KEY, trace_id TEXT, parent_call_id TEXT,
               provider TEXT, model_id TEXT, api TEXT, subsystem TEXT,
               call_purpose TEXT, session_id TEXT, started_at TEXT, ended_at TEXT,
               terminal_status TEXT);
             CREATE TABLE model_call_usage (
               model_call_id TEXT PRIMARY KEY, input_total_tokens INTEGER,
               output_total_tokens INTEGER, reasoning_tokens INTEGER,
               cache_read_tokens INTEGER, cache_write_tokens INTEGER);
             INSERT INTO model_calls VALUES
               ('call-a', 'trace-1', NULL, 'anthropic', 'claude-x', 'messages',
                'chat', 'session-reply', 'sess-1', '2025-01-01T00:00:00Z',
                '2025-01-01T00:00:05Z', 'ok'),
               ('call-b', 'trace-1', 'call-a', 'anthropic', 'claude-x', 'messages',
                'tool', NULL, 'sess-1', '2025-01-01T00:00:06Z',
                '2025-01-01T00:00:07Z', 'error'),
               ('call-c', NULL, NULL, 'openai', 'gpt-x', 'responses',
                'cron', 'tick', NULL, '2025-01-02T00:00:00Z', NULL, NULL);
             INSERT INTO model_call_usage VALUES
               ('call-a', 100, 50, 3, 7, 11),
               ('call-b', 20, 5, NULL, NULL, NULL);",
        )
        .unwrap();
    }

    #[test]
    fn v2_store_maps_rows_with_usage_and_traces() {
        let path = temp_path("v2");
        seed_v2_store(&path);
        let rows = read_legacy_observability_rows(&path).unwrap();
        assert_eq!(rows.len(), 3);
        let a = &rows[0];
        assert_eq!(a.record.model_call_id, "call-a");
        assert_eq!(a.legacy_trace_id.as_deref(), Some("trace-1"));
        assert_eq!(a.record.session_id.as_deref(), Some("sess-1"));
        let usage = a.record.usage.as_ref().expect("usage");
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.cache_write_tokens, Some(11));
        assert!(matches!(a.record.outcome, CallOutcome::Succeeded));
        assert!(matches!(rows[1].record.outcome, CallOutcome::Failed));
        // 无 usage 行（call-c）→ usage None，outcome Unknown。
        assert!(rows[2].record.usage.is_none());
        assert!(matches!(rows[2].record.outcome, CallOutcome::Unknown));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn v1_store_without_usage_table_still_maps() {
        let path = temp_path("v1");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE model_calls (
               call_id TEXT PRIMARY KEY, trace_id TEXT, parent_call_id TEXT,
               provider TEXT, model_id TEXT, api TEXT, subsystem TEXT,
               call_purpose TEXT, session_id TEXT, started_at TEXT, ended_at TEXT,
               terminal_status TEXT);
             INSERT INTO model_calls VALUES
               ('call-v1', 'trace-v1', NULL, 'p', 'm', 'a', 's', 'purpose', 'sess',
                '2025-03-01T00:00:00Z', '2025-03-01T00:00:01Z', 'aborted');",
        )
        .unwrap();
        drop(conn);
        let rows = read_legacy_observability_rows(&path).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].record.usage.is_none());
        assert!(matches!(rows[0].record.outcome, CallOutcome::Cancelled));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn wrong_shape_is_loud_not_silent() {
        let path = temp_path("bad");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE calls (id TEXT PRIMARY KEY, ts INTEGER);")
            .unwrap();
        drop(conn);
        let err = match read_legacy_observability_rows(&path) {
            Ok(_) => panic!("expected a loud refusal, got rows"),
            Err(err) => err,
        };
        assert!(
            err.contains("legacy schema does not match v1..v7"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn bad_timestamp_is_loud_not_silent() {
        let path = temp_path("badts");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE model_calls (
               call_id TEXT PRIMARY KEY, trace_id TEXT, parent_call_id TEXT,
               provider TEXT, model_id TEXT, api TEXT, subsystem TEXT,
               call_purpose TEXT, session_id TEXT, started_at TEXT, ended_at TEXT,
               terminal_status TEXT);
             INSERT INTO model_calls VALUES
               ('call-x', NULL, NULL, 'p', 'm', 'a', 's', 'pp', 'sess',
                'not-a-timestamp', NULL, 'ok');",
        )
        .unwrap();
        drop(conn);
        let err = match read_legacy_observability_rows(&path) {
            Ok(_) => panic!("expected a loud refusal, got rows"),
            Err(err) => err,
        };
        assert!(err.contains("is not RFC3339"), "unexpected error: {err}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
