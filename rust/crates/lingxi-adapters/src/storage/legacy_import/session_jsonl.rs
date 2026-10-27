//! 旧会话 JSONL 逐行解析器（R08-T04 B2 切片 A）。
//!
//! 契约（T04_DESIGN ⑥）：
//! - 容忍截断尾行：末行不是合法 JSON 且不带换行结束 → `corrupt_tail`，
//!   完好前缀照常导入，余量字节由调用方进 quarantine，绝不整会话丢弃。
//! - 中间行非法 JSON → `corrupt_lines`（逐行记录，继续解析后续行）。
//! - 未知 entryType 不在这里裁决：解析器只保原始行字节 + 解析出的公共
//!   字段，未知块归档/标记由 convert_sessions 决定。
//! - 已知条目的未知字段零裁剪：`raw` 持有整行 JSON Value 透传。

use serde_json::Value;

/// 一条解析出的 JSONL 条目。
#[derive(Debug, Clone)]
pub struct ParsedEntry {
    /// 1-based 行号（含 corrupt 行在内按物理行计）。
    pub line_no: u64,
    /// 整行 JSON（未知字段透传的载体）。
    pub raw: Value,
    /// entryType 原值（缺省视为 "message" 类用户/助手条目由调用方按 role 判）。
    pub entry_type: Option<String>,
    /// 条目 id（旧 JSONL 的 `id` 字段）。
    pub id: Option<String>,
    /// 条目声明的 runId（runBoundary / 消息归属）。
    pub run_id: Option<String>,
    /// 条目时间戳（unix ms，`ts` 字段）。
    pub ts: Option<i64>,
    /// role 字段（user/assistant/system/…）。
    pub role: Option<String>,
}

/// 截断尾行证据。
#[derive(Debug, Clone)]
pub struct CorruptTail {
    pub line_no: u64,
    pub bytes: Vec<u8>,
}

/// 中间非法行证据。
#[derive(Debug, Clone)]
pub struct CorruptLine {
    pub line_no: u64,
    pub bytes: Vec<u8>,
}

/// 一个会话 JSONL 的解析结果。
#[derive(Debug, Default)]
pub struct ParsedSession {
    pub entries: Vec<ParsedEntry>,
    pub corrupt_lines: Vec<CorruptLine>,
    pub corrupt_tail: Option<CorruptTail>,
    /// 物理总行数（含空行与损坏行）。
    pub total_lines: u64,
}

fn parse_line(line: &str, line_no: u64) -> Option<ParsedEntry> {
    let raw: Value = serde_json::from_str(line).ok()?;
    if !raw.is_object() {
        return None;
    }
    let entry_type = raw
        .get("entryType")
        .and_then(Value::as_str)
        .map(str::to_string);
    let id = raw.get("id").and_then(Value::as_str).map(str::to_string);
    let run_id = raw
        .get("runId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let ts = raw.get("ts").and_then(Value::as_i64);
    let role = raw.get("role").and_then(Value::as_str).map(str::to_string);
    // T07 F-T07-1 修复（加性）：真实旧栈会话 JSONL 是 Pi SDK 格式
    // （lib/session-jsonl.ts 生产读取面 + tests/history-pagination-run-continuity
    // .test.ts L87-122 先例）——首行 {type:"session",…}，条目
    // {type:"message",id,parentId,timestamp,message:{role,content}}。
    // 仅当 entryType 缺省时按 type 识别（夹具 entryType 行为零变化；
    // t07-registry-amend 双跑对拍兜底）：
    //   type:"session" 头行 ≡ sessionMeta 控制条目（转换器按控制类跳过）；
    //   type:"message" + message.role∈{user,assistant,system} → 消息类
    //     （role 取嵌套值，走「None ⇒ role 判消息」既有分支）；
    //   type:"message" + message.role=="toolResult" → 非消息已知类
    //     （与夹具 toolCall/toolResult 同路径：原值透传 + role='legacy'）；
    //   其余 Pi type（compaction/model_change 等）不在此裁决 → 维持 unknown 块。
    // 时间：Pi 用 RFC3339 `timestamp` 字符串，归一为 unix ms（ts 优先级不变）。
    let pi_type = raw.get("type").and_then(Value::as_str);
    let (entry_type, role, ts) = match (entry_type.is_none(), pi_type) {
        (true, Some("session")) => (Some("sessionMeta".to_string()), role, ts),
        (true, Some("message")) => {
            let message_role = raw
                .get("message")
                .and_then(|m| m.get("role"))
                .and_then(Value::as_str);
            let iso_ts = raw
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.timestamp_millis());
            match message_role {
                Some("toolResult") => (Some("toolResult".to_string()), role, ts.or(iso_ts)),
                Some(r @ ("user" | "assistant" | "system")) => {
                    (entry_type, Some(r.to_string()), ts.or(iso_ts))
                }
                _ => (entry_type, role, ts),
            }
        }
        _ => (entry_type, role, ts),
    };
    Some(ParsedEntry {
        line_no,
        raw,
        entry_type,
        id,
        run_id,
        ts,
        role,
    })
}

/// 解析整个 JSONL 字节流。
///
/// 行切分按 `\n`；`\r\n` 归一。空行跳过（不计损坏）。末行无 `\n` 结尾且
/// 非合法 JSON → corrupt_tail；中间非法行 → corrupt_lines。
pub fn parse_session_jsonl(bytes: &[u8]) -> ParsedSession {
    let mut out = ParsedSession::default();
    let text = String::from_utf8_lossy(bytes);
    let ends_with_newline = bytes.last() == Some(&b'\n');
    let mut lines: Vec<&str> = text.split('\n').collect();
    // split('\n') 对结尾 \n 产生尾部空串，去掉。
    if ends_with_newline {
        lines.pop();
    }
    let last_idx = lines.len().saturating_sub(1);
    for (idx, raw_line) in lines.iter().enumerate() {
        let line_no = (idx + 1) as u64;
        out.total_lines = line_no;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(line, line_no) {
            Some(entry) => out.entries.push(entry),
            None => {
                let is_tail = idx == last_idx && !ends_with_newline;
                let evidence = CorruptLine {
                    line_no,
                    bytes: line.as_bytes().to_vec(),
                };
                if is_tail {
                    out.corrupt_tail = Some(CorruptTail {
                        line_no: evidence.line_no,
                        bytes: evidence.bytes,
                    });
                } else {
                    out.corrupt_lines.push(evidence);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_file_parses_all_entries() {
        let data = b"{\"entryType\":\"user\",\"id\":\"m1\",\"content\":\"hi\"}\n{\"entryType\":\"assistant\",\"id\":\"m2\",\"content\":\"yo\"}\n";
        let parsed = parse_session_jsonl(data);
        assert_eq!(parsed.entries.len(), 2);
        assert!(parsed.corrupt_tail.is_none());
        assert!(parsed.corrupt_lines.is_empty());
        assert_eq!(parsed.entries[0].entry_type.as_deref(), Some("user"));
    }

    #[test]
    fn truncated_tail_is_quarantined_not_dropped() {
        let mut data = b"{\"entryType\":\"user\",\"id\":\"m1\"}\n".to_vec();
        data.extend_from_slice(b"{\"entryType\":\"assistant\",\"content\":\"cut of");
        let parsed = parse_session_jsonl(&data);
        assert_eq!(parsed.entries.len(), 1);
        let tail = parsed.corrupt_tail.expect("tail");
        assert_eq!(tail.line_no, 2);
        assert!(tail.bytes.starts_with(b"{\"entryType\""));
    }

    #[test]
    fn mid_file_corrupt_line_recorded_and_parsing_continues() {
        let data = b"{\"entryType\":\"user\",\"id\":\"m1\"}\nnot-json\n{\"entryType\":\"assistant\",\"id\":\"m2\"}\n";
        let parsed = parse_session_jsonl(data);
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.corrupt_lines.len(), 1);
        assert_eq!(parsed.corrupt_lines[0].line_no, 2);
        assert!(parsed.corrupt_tail.is_none());
    }

    #[test]
    fn unknown_fields_pass_through_in_raw() {
        let data = b"{\"entryType\":\"assistant\",\"id\":\"m1\",\"customVendorField\":{\"x\":1}}\n";
        let parsed = parse_session_jsonl(data);
        assert_eq!(
            parsed.entries[0].raw["customVendorField"]["x"],
            serde_json::json!(1)
        );
    }

    #[test]
    fn crlf_and_blank_lines_tolerated() {
        let data = b"{\"entryType\":\"user\",\"id\":\"m1\"}\r\n\r\n{\"entryType\":\"assistant\",\"id\":\"m2\"}\r\n";
        let parsed = parse_session_jsonl(data);
        assert_eq!(parsed.entries.len(), 2);
        assert!(parsed.corrupt_lines.is_empty());
    }

    /// T07 F-T07-1：真实旧栈 Pi SDK 格式识别（头行 ≡ sessionMeta；message.role
    /// 嵌套；ISO timestamp → unix ms；toolResult → 非消息已知类）。
    #[test]
    fn pi_sdk_real_format_entries_recognized() {
        let data = concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"7e5f\",\"cwd\":\"/tmp\",\"timestamp\":\"2026-10-01T09:00:00Z\"}\n",
            "{\"type\":\"message\",\"id\":\"u1\",\"parentId\":null,\"timestamp\":\"2026-10-01T10:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
            "{\"type\":\"message\",\"id\":\"a1\",\"parentId\":\"u1\",\"timestamp\":\"2026-10-01T10:00:01Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"yo\"}]}}\n",
            "{\"type\":\"message\",\"id\":\"r1\",\"parentId\":\"a1\",\"timestamp\":\"2026-10-01T10:00:02Z\",\"message\":{\"role\":\"toolResult\",\"toolCallId\":\"tu-1\",\"content\":\"out\"}}\n",
        );
        let parsed = parse_session_jsonl(data.as_bytes());
        assert_eq!(parsed.entries.len(), 4);
        let header = &parsed.entries[0];
        assert_eq!(header.entry_type.as_deref(), Some("sessionMeta"));
        let user = &parsed.entries[1];
        assert_eq!(user.entry_type, None);
        assert_eq!(user.role.as_deref(), Some("user"));
        assert_eq!(user.id.as_deref(), Some("u1"));
        assert_eq!(user.ts, Some(1_790_848_800_000));
        let assistant = &parsed.entries[2];
        assert_eq!(assistant.role.as_deref(), Some("assistant"));
        let tool_result = &parsed.entries[3];
        assert_eq!(tool_result.entry_type.as_deref(), Some("toolResult"));
        // 夹具 entryType 行为零变化：显式 entryType 优先，不触发 Pi 分支。
        let fixture = parse_session_jsonl(b"{\"entryType\":\"user\",\"id\":\"m1\",\"content\":\"x\"}\n");
        assert_eq!(fixture.entries[0].entry_type.as_deref(), Some("user"));
    }
}
