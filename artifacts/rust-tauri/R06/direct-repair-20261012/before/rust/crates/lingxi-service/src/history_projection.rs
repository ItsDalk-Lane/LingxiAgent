//! 统一历史投影 —— R06-T04 交付物②：实时/重开/重连/导出四方式共用的
//! 唯一投影函数（任务书步骤②）。
//!
//! 输入是持久事实（分支链消息行 + 相关 run 的事件行 + 本会话实存 run
//! 集合 + lineage 边），输出是统一历史项序列。纯函数：无 I/O、无时钟、
//! 无随机——四方式一致性由「同一函数 × 同一持久事实」结构性保证。
//!
//! 语义规则（与任务书/契约逐条对应）：
//! - 段聚合：assistant_segment_start/delta/end 按 segment_id 聚合；
//!   同 id 多周期 = 多个段项，按首个事件 seq 排序。
//! - 工具配对：started/completed 按稳定 toolCallId 配对；只有 started
//!   不编造结局（status=started），只有 completed 也如实产出。
//! - 终态诚实（02 §4 / 步骤⑤）：run 无 final_message_committed 且有
//!   终态 run_state_changed → RunTerminal；有 final 不另产终态项
//!   （FinalMessage 即完成展示，四方式同规则）。绝不编造 final。
//! - 锚定（每 run 的项在全分支分页遍历中恰好出现一次）：
//!   user 锚（页内 `user:{run}`）> final 锚（页内 `{run}-final`，仅当
//!   分支上不存在该 run 的 user 锚时才携带 run 组项，否则只产
//!   FinalMessage 本身）> lineage 并入最近页内锚定祖先组（按事件 seq
//!   合流）；有 fmc 但锚不在本页 → 本页不产（它在锚所在页出现）。
//! - run 关联三态（步骤④）：known 仅来自 run_id 列或本会话 id 约定
//!   且 run 实存；fork 副本（run_id 置 NULL，T03 D9）等一律 unknown，
//!   绝不按时间猜。
//! - 内容规范化（步骤①④单一入口）：final/user 的 content_json 先按
//!   Value 解析；顶层未知键 → legacy_fields 保留；整体不可解析 →
//!   LegacyRaw 原文归档；final 文本统一再过
//!   [`crate::streaming_norm::normalize_final_message`]（与实时入口同一
//!   scanner，对已规范化内容幂等：think→Reasoning 块、mood 剥离）。

use std::collections::{HashMap, HashSet};

use lingxi_adapters::storage::canonical_message_store::StoredEventRow;
use lingxi_adapters::storage::session_tree::MessageRow;
use lingxi_protocol::history::{HistoryItem, HistoryToolStatus, RunAssociation};
use lingxi_protocol::{EventPayload, KnownEventPayload, NormalizedMessage, RunStatus};

/// 投影输入（全部为持久事实的只读视图）。
pub struct ProjectionInput<'a> {
    /// 本页（或全量）分支链消息，链序旧→新。
    pub messages: &'a [MessageRow],
    /// 页 run 集合 + lineage 闭包的事件（seq 升序）。
    pub events: &'a [StoredEventRow],
    /// 本会话实存 run（id 约定命中的 run 须实存才算 known）。
    pub existing_runs: &'a HashSet<String>,
    /// (parent_run_id, child_run_id) 边（页 run 集合的向下闭包）。
    pub lineage: &'a [(String, String)],
    /// 分支上存在 `user:{run}` 消息的 run 集合（跨页锚定判定；
    /// 由 service 以主键探测计算——分页遍历不因此全扫）。
    pub user_anchors_on_branch: &'a HashSet<String>,
}

/// 统一投影入口。
pub fn project_history(input: &ProjectionInput<'_>) -> Vec<HistoryItem> {
    let runs = bucket_events(input.events);
    let anchors = resolve_anchors(input, &runs);
    let groups = build_groups(&runs, &anchors);

    let mut items = Vec::new();
    for message in input.messages {
        match classify_message(message) {
            MessageClass::ResetMarker => items.push(project_reset_marker(message)),
            MessageClass::UserAnchor(run_id) => {
                items.push(project_user_message(message, &run_id, input.existing_runs));
                if let Some(group) = groups.get(&run_id) {
                    items.extend(group.iter().cloned());
                }
            }
            MessageClass::LegacyUser => {
                items.push(project_legacy_user_message(message));
            }
            MessageClass::FinalAnchor(run_id) => {
                // run 组项只在「分支上无 user 锚」时挂到 final 锚（否则它们
                // 在 user 锚所在页已产出——每 run 恰好一次）。
                if !input.user_anchors_on_branch.contains(&run_id) {
                    if let Some(group) = groups.get(&run_id) {
                        items.extend(group.iter().cloned());
                    }
                }
                items.push(project_final_message(message, &run_id, input.existing_runs));
            }
            MessageClass::LegacyAssistant => {
                items.push(project_legacy_assistant_message(message));
            }
            MessageClass::Unknown => items.push(project_legacy_raw(
                message,
                "unrecognized entry_type or message shape",
            )),
        }
    }
    items
}

// ── 事件分桶 ──

#[derive(Debug, Default)]
struct RunBuckets {
    /// (first_seq, 段聚合) 按 first_seq 升序。
    segments: Vec<SegmentAcc>,
    tools: Vec<ToolAcc>,
    /// 终态 run_state_changed（最后一个终态）。
    terminal: Option<(RunStatus, Option<String>, u64)>,
    has_fmc: bool,
}

#[derive(Debug)]
struct SegmentAcc {
    segment_id: String,
    kind: lingxi_protocol::SegmentKind,
    phase: lingxi_protocol::AssistantPhase,
    text: String,
    first_seq: u64,
    ended: bool,
}

#[derive(Debug)]
struct ToolAcc {
    tool_call_id: String,
    target: String,
    args_digest: Option<lingxi_protocol::ContentDigest>,
    args_summary: Option<String>,
    first_seq: u64,
    result: Option<lingxi_protocol::ToolResultWire>,
}

fn bucket_events(events: &[StoredEventRow]) -> HashMap<String, RunBuckets> {
    let mut runs: HashMap<String, RunBuckets> = HashMap::new();
    // 段/工具按 (run, id) 聚合，保序用 first_seq。
    let mut segment_index: HashMap<(String, String), usize> = HashMap::new();
    let mut tool_index: HashMap<(String, String), usize> = HashMap::new();
    for event in events {
        let Some(run_id) = event.run_id.clone() else {
            continue; // 会话级事件（如 session_created）不属于任何 run 组
        };
        let buckets = runs.entry(run_id.clone()).or_default();
        let payload = match &event.payload {
            EventPayload::Known(known) => known,
            EventPayload::Unknown(_) => continue, // 未知事件原样保留在事件面，不进历史项
        };
        match payload {
            KnownEventPayload::AssistantSegmentStart(start) => {
                let key = (run_id.clone(), start.segment_id.clone());
                // 同 id 多周期：上一周期已 end → 开新段项。
                let reuse = segment_index
                    .get(&key)
                    .and_then(|&i| buckets.segments.get(i))
                    .filter(|acc| !acc.ended);
                if reuse.is_none() {
                    buckets.segments.push(SegmentAcc {
                        segment_id: start.segment_id.clone(),
                        kind: start.kind,
                        phase: start.semantic_phase,
                        text: String::new(),
                        first_seq: event.seq,
                        ended: false,
                    });
                    segment_index.insert(key, buckets.segments.len() - 1);
                }
            }
            KnownEventPayload::AssistantSegmentDelta(delta) => {
                let key = (run_id.clone(), delta.segment_id.clone());
                let idx = match segment_index.get(&key) {
                    Some(&i) => i,
                    None => {
                        // 无 start 的 delta（断流恢复面）：如实聚合，不丢。
                        buckets.segments.push(SegmentAcc {
                            segment_id: delta.segment_id.clone(),
                            kind: lingxi_protocol::SegmentKind::Text,
                            phase: delta.semantic_phase,
                            text: String::new(),
                            first_seq: event.seq,
                            ended: false,
                        });
                        segment_index.insert(key.clone(), buckets.segments.len() - 1);
                        buckets.segments.len() - 1
                    }
                };
                if let Some(acc) = buckets.segments.get_mut(idx) {
                    acc.text.push_str(&delta.delta);
                }
            }
            KnownEventPayload::AssistantSegmentEnd(end) => {
                let key = (run_id.clone(), end.segment_id.clone());
                if let Some(&i) = segment_index.get(&key) {
                    if let Some(acc) = buckets.segments.get_mut(i) {
                        acc.ended = true;
                        acc.phase = end.semantic_phase;
                    }
                }
            }
            KnownEventPayload::ToolCallStarted(started) => {
                let key = (run_id.clone(), started.tool_call.tool_call_id.to_string());
                if let std::collections::hash_map::Entry::Vacant(slot) = tool_index.entry(key) {
                    buckets.tools.push(ToolAcc {
                        tool_call_id: started.tool_call.tool_call_id.to_string(),
                        target: started.tool_call.target.clone(),
                        args_digest: Some(started.tool_call.args_digest.clone()),
                        args_summary: started.tool_call.args_summary.clone(),
                        first_seq: event.seq,
                        result: None,
                    });
                    slot.insert(buckets.tools.len() - 1);
                }
            }
            KnownEventPayload::ToolCallCompleted(completed) => {
                let key = (run_id.clone(), completed.tool_call_id.to_string());
                let idx = match tool_index.get(&key) {
                    Some(&i) => i,
                    None => {
                        // 只有 completed：如实产出（target 未知不编造）。
                        buckets.tools.push(ToolAcc {
                            tool_call_id: completed.tool_call_id.to_string(),
                            target: String::new(),
                            args_digest: None,
                            args_summary: None,
                            first_seq: event.seq,
                            result: None,
                        });
                        tool_index.insert(key.clone(), buckets.tools.len() - 1);
                        buckets.tools.len() - 1
                    }
                };
                if let Some(acc) = buckets.tools.get_mut(idx) {
                    acc.result = Some(completed.result.clone());
                }
            }
            KnownEventPayload::RunStateChanged(change) => {
                if change.to.is_terminal() {
                    buckets.terminal = Some((change.to, change.reason.clone(), event.seq));
                }
            }
            KnownEventPayload::FinalMessageCommitted(_) => {
                buckets.has_fmc = true;
            }
            _ => {}
        }
    }
    runs
}

// ── 锚定 ──

enum MessageClass {
    ResetMarker,
    UserAnchor(String),
    LegacyUser,
    FinalAnchor(String),
    LegacyAssistant,
    Unknown,
}

fn classify_message(message: &MessageRow) -> MessageClass {
    if message.entry_type == "hana-session-branch-reset" {
        return MessageClass::ResetMarker;
    }
    if message.entry_type != "message" {
        return MessageClass::Unknown;
    }
    if let Some(run_id) = message.message_id.strip_prefix("user:") {
        return MessageClass::UserAnchor(run_id.to_string());
    }
    if let Some(run_id) = message.message_id.strip_suffix("-final") {
        if !run_id.is_empty() {
            return MessageClass::FinalAnchor(run_id.to_string());
        }
    }
    match message.role.as_str() {
        "user" => MessageClass::LegacyUser,
        "assistant" => MessageClass::LegacyAssistant,
        _ => MessageClass::Unknown,
    }
}

/// 每个事件 run 的锚定解析结果。
enum Anchor {
    /// 页内 user 锚（组项挂在 UserMessage 后）。
    User,
    /// 页内 final 锚（分支无 user 锚时组项挂在 FinalMessage 前）。
    Final,
    /// 并入最近页内锚定祖先的组。
    MergeInto(String),
    /// 本页不产（fmc 存在 → 自锚在他页；或真正的孤儿——实测不可能，
    /// 出现即告警不静默）。
    Elsewhere,
}

fn resolve_anchors(
    input: &ProjectionInput<'_>,
    runs: &HashMap<String, RunBuckets>,
) -> HashMap<String, Anchor> {
    let mut anchors = HashMap::new();
    let page_user: HashSet<String> = input
        .messages
        .iter()
        .filter_map(|m| m.message_id.strip_prefix("user:").map(str::to_string))
        .collect();
    let page_final: HashSet<String> = input
        .messages
        .iter()
        .filter_map(|m| m.message_id.strip_suffix("-final").map(str::to_string))
        .filter(|id| !id.is_empty())
        .collect();
    let child_to_parent: HashMap<&str, &str> = input
        .lineage
        .iter()
        .map(|(parent, child)| (child.as_str(), parent.as_str()))
        .collect();

    for run_id in runs.keys() {
        let anchor = if page_user.contains(run_id) {
            Anchor::User
        } else if page_final.contains(run_id) {
            Anchor::Final
        } else if runs.get(run_id).is_some_and(|b| b.has_fmc) {
            // 有 final（自锚于他页）——本页不产，防止跨页重复。
            Anchor::Elsewhere
        } else {
            // 无自锚（无 final 的子代理 run）：沿 lineage 向上找最近
            // 页内锚定祖先；中间 run 若自锚他页则链条中断（不穿越）。
            let mut cursor = run_id.as_str();
            let mut resolved = Anchor::Elsewhere;
            let mut visited = HashSet::new();
            while let Some(parent) = child_to_parent.get(cursor) {
                if !visited.insert(parent) {
                    break; // 环：不出新成员即停
                }
                if page_user.contains(*parent) || page_final.contains(*parent) {
                    resolved = Anchor::MergeInto((*parent).to_string());
                    break;
                }
                if runs.get(*parent).is_some_and(|b| b.has_fmc) {
                    break; // 祖先自锚他页：本页无挂载点
                }
                cursor = parent;
            }
            resolved
        };
        anchors.insert(run_id.clone(), anchor);
    }
    anchors
}

/// run 组项（段+工具+终态，按事件 seq 合流；含 lineage 并入的后代组）。
fn build_groups(
    runs: &HashMap<String, RunBuckets>,
    anchors: &HashMap<String, Anchor>,
) -> HashMap<String, Vec<HistoryItem>> {
    // 组属主：run → 承载其组项的锚定 run。
    let mut owner_of: HashMap<&str, &str> = HashMap::new();
    for (run_id, anchor) in anchors {
        match anchor {
            Anchor::User | Anchor::Final => {
                owner_of.insert(run_id.as_str(), run_id.as_str());
            }
            Anchor::MergeInto(ancestor) => {
                owner_of.insert(run_id.as_str(), ancestor.as_str());
            }
            Anchor::Elsewhere => {}
        }
    }
    let mut groups: HashMap<String, Vec<(u64, HistoryItem)>> = HashMap::new();
    for (run_id, buckets) in runs {
        let Some(&owner) = owner_of.get(run_id.as_str()) else {
            if !matches!(anchors.get(run_id), Some(Anchor::Elsewhere)) {
                tracing::warn!(
                    run_id,
                    "history projection: run events have no anchor on this page"
                );
            }
            continue;
        };
        let group = groups.entry(owner.to_string()).or_default();
        let association = RunAssociation::known(run_id.clone());
        for segment in &buckets.segments {
            group.push((
                segment.first_seq,
                HistoryItem::AssistantSegment {
                    segment_id: segment.segment_id.clone(),
                    run: association.clone(),
                    phase: segment.phase,
                    segment_kind: segment.kind,
                    text: segment.text.clone(),
                    complete: segment.ended,
                    first_seq: segment.first_seq,
                },
            ));
        }
        for tool in &buckets.tools {
            let (status, result) = match &tool.result {
                Some(result) => (tool_status_of(result), Some(result.clone())),
                None => (HistoryToolStatus::Started, None),
            };
            group.push((
                tool.first_seq,
                HistoryItem::ToolCall {
                    tool_call_id: tool.tool_call_id.clone(),
                    run: association.clone(),
                    target: tool.target.clone(),
                    args_summary: tool.args_summary.clone(),
                    // 只有 completed 无 started：digest/目标不可得即缺省，
                    // 绝不编造参数事实。
                    args_digest: tool.args_digest.clone(),
                    status,
                    result,
                    first_seq: tool.first_seq,
                },
            ));
        }
        // 终态诚实：无 final 的 run 如实给 RunTerminal。
        if !buckets.has_fmc {
            if let Some((status, reason, seq)) = &buckets.terminal {
                group.push((
                    *seq,
                    HistoryItem::RunTerminal {
                        run_id: run_id.clone(),
                        status: *status,
                        terminal_reason: reason.clone(),
                        event_seq: *seq,
                    },
                ));
            }
        }
    }
    groups
        .into_iter()
        .map(|(owner, mut group)| {
            group.sort_by_key(|(seq, _)| *seq);
            (owner, group.into_iter().map(|(_, item)| item).collect())
        })
        .collect()
}

fn tool_status_of(result: &lingxi_protocol::ToolResultWire) -> HistoryToolStatus {
    match result.status {
        lingxi_protocol::ToolResultStatus::Success => HistoryToolStatus::Success,
        lingxi_protocol::ToolResultStatus::Failed => HistoryToolStatus::Failed,
        lingxi_protocol::ToolResultStatus::Cancelled => HistoryToolStatus::Cancelled,
        // 外部完成的副作用无本地回执：如实透传 unknown，绝不报成成功。
        lingxi_protocol::ToolResultStatus::Unknown => HistoryToolStatus::Unknown,
    }
}

// ── 消息行 → 历史项（内容规范化单一入口） ──

fn project_user_message(
    message: &MessageRow,
    run_id: &str,
    existing_runs: &HashSet<String>,
) -> HistoryItem {
    let association = if existing_runs.contains(run_id) {
        RunAssociation::known(run_id.to_string())
    } else {
        RunAssociation::unknown("run id convention resolved but no such run in this session")
    };
    match parse_message_body(&message.content_json) {
        Ok(mut map) => {
            let text = map
                .remove("text")
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            let mut legacy_fields = serde_json::Map::new();
            for (key, value) in map {
                legacy_fields.insert(key, value);
            }
            HistoryItem::UserMessage {
                message_id: message.message_id.clone(),
                seq: message.seq as u64,
                committed_at_unix_ms: message.committed_at_unix_ms,
                text,
                run: association,
                legacy_fields,
            }
        }
        Err(raw) => HistoryItem::LegacyRaw {
            message_id: message.message_id.clone(),
            seq: message.seq as u64,
            raw,
            reason: "user message body is not a JSON object".to_string(),
        },
    }
}

fn project_legacy_user_message(message: &MessageRow) -> HistoryItem {
    let association = RunAssociation::unknown("legacy user message without run linkage");
    match parse_message_body(&message.content_json) {
        Ok(mut map) => {
            let text = map
                .remove("text")
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            let mut legacy_fields = serde_json::Map::new();
            for (key, value) in map {
                legacy_fields.insert(key, value);
            }
            HistoryItem::UserMessage {
                message_id: message.message_id.clone(),
                seq: message.seq as u64,
                committed_at_unix_ms: message.committed_at_unix_ms,
                text,
                run: association,
                legacy_fields,
            }
        }
        Err(raw) => HistoryItem::LegacyRaw {
            message_id: message.message_id.clone(),
            seq: message.seq as u64,
            raw,
            reason: "legacy user message body is not a JSON object".to_string(),
        },
    }
}

fn project_final_message(
    message: &MessageRow,
    convention_run_id: &str,
    existing_runs: &HashSet<String>,
) -> HistoryItem {
    // run 关联三态：run_id 列优先（确定性事实），缺省走 id 约定 +
    // 本会话实存校验；fork 副本 run_id 置 NULL（T03 D9）→ unknown。
    let association = match &message.run_id {
        Some(column) if existing_runs.contains(column) => RunAssociation::known(column.clone()),
        Some(column) => {
            RunAssociation::unknown(format!("run_id column references unknown run {column}"))
        }
        None if existing_runs.contains(convention_run_id) => {
            RunAssociation::known(convention_run_id.to_string())
        }
        None => RunAssociation::unknown(
            "no run linkage (fork copies carry run_id NULL — T03 D9); never guessed by time",
        ),
    };
    match parse_normalized_message(&message.content_json) {
        Ok((normalized, legacy_fields)) => {
            // 与实时入口同一 scanner：think→Reasoning、mood 剥离；
            // 对已规范化内容幂等。
            let normalized = crate::streaming_norm::normalize_final_message(normalized);
            HistoryItem::FinalMessage {
                message_id: message.message_id.clone(),
                seq: message.seq as u64,
                run: association,
                model_call_id: message
                    .model_call_id
                    .clone()
                    .or(normalized.model_call_id.map(|id| id.to_string())),
                content: normalized.content,
                committed_at_unix_ms: message.committed_at_unix_ms,
                legacy_fields,
            }
        }
        Err((raw, reason)) => HistoryItem::LegacyRaw {
            message_id: message.message_id.clone(),
            seq: message.seq as u64,
            raw,
            reason,
        },
    }
}

fn project_legacy_assistant_message(message: &MessageRow) -> HistoryItem {
    match parse_normalized_message(&message.content_json) {
        Ok((normalized, legacy_fields)) => {
            let normalized = crate::streaming_norm::normalize_final_message(normalized);
            HistoryItem::FinalMessage {
                message_id: message.message_id.clone(),
                seq: message.seq as u64,
                run: RunAssociation::unknown("legacy assistant message without run linkage"),
                model_call_id: message
                    .model_call_id
                    .clone()
                    .or(normalized.model_call_id.map(|id| id.to_string())),
                content: normalized.content,
                committed_at_unix_ms: message.committed_at_unix_ms,
                legacy_fields,
            }
        }
        Err((raw, reason)) => HistoryItem::LegacyRaw {
            message_id: message.message_id.clone(),
            seq: message.seq as u64,
            raw,
            reason,
        },
    }
}

fn project_reset_marker(message: &MessageRow) -> HistoryItem {
    match parse_message_body(&message.content_json) {
        Ok(mut map) => {
            let reason = map
                .remove("reason")
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            let to = map.remove("to").and_then(|v| {
                if v.is_null() {
                    None
                } else {
                    v.as_str().map(str::to_string)
                }
            });
            let source_entry_id = map
                .remove("sourceEntryId")
                .and_then(|v| v.as_str().map(str::to_string));
            HistoryItem::ResetMarker {
                message_id: message.message_id.clone(),
                seq: message.seq as u64,
                reason,
                to,
                source_entry_id,
            }
        }
        Err(raw) => HistoryItem::LegacyRaw {
            message_id: message.message_id.clone(),
            seq: message.seq as u64,
            raw,
            reason: "reset marker body is not a JSON object".to_string(),
        },
    }
}

fn project_legacy_raw(message: &MessageRow, reason: &str) -> HistoryItem {
    let raw = serde_json::from_str(&message.content_json)
        .unwrap_or(serde_json::Value::String(message.content_json.clone()));
    HistoryItem::LegacyRaw {
        message_id: message.message_id.clone(),
        seq: message.seq as u64,
        raw,
        reason: reason.to_string(),
    }
}

/// content_json → 顶层键 map（只切分「JSON object 与否」）。
/// 已知键的提取在各 project_* 内完成，剩余键全部落入 legacy_fields
/// （步骤④：未知旧字段保留在受控扩展，不丢）。
fn parse_message_body(
    content_json: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, serde_json::Value> {
    let value: serde_json::Value = serde_json::from_str(content_json)
        .map_err(|_| serde_json::Value::String(content_json.to_string()))?;
    match value {
        serde_json::Value::Object(map) => Ok(map),
        other => Err(other),
    }
}

/// final 类消息体 → (NormalizedMessage, legacy_fields)。
/// 顶层未知键剥离进 legacy_fields 后再按严格形状解析（NormalizedMessage
/// deny_unknown_fields）；剥离后仍不合形状 → LegacyRaw。
fn parse_normalized_message(
    content_json: &str,
) -> Result<
    (
        NormalizedMessage,
        serde_json::Map<String, serde_json::Value>,
    ),
    (serde_json::Value, String),
> {
    let value: serde_json::Value = serde_json::from_str(content_json).map_err(|err| {
        (
            serde_json::Value::String(content_json.to_string()),
            format!("message body is not valid JSON: {err}"),
        )
    })?;
    let serde_json::Value::Object(mut map) = value else {
        return Err((value, "message body is not a JSON object".to_string()));
    };
    let known = ["role", "content", "modelCallId"];
    let mut legacy_fields = serde_json::Map::new();
    let extra_keys: Vec<String> = map
        .keys()
        .filter(|key| !known.contains(&key.as_str()))
        .cloned()
        .collect();
    for key in extra_keys {
        if let Some(value) = map.remove(&key) {
            legacy_fields.insert(key, value);
        }
    }
    let normalized: NormalizedMessage = serde_json::from_value(serde_json::Value::Object(map))
        .map_err(|err| {
            (
                serde_json::Value::String(content_json.to_string()),
                format!("message body is not a normalized message: {err}"),
            )
        })?;
    Ok((normalized, legacy_fields))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_protocol::history::RunAssociationState;
    use lingxi_protocol::*;

    fn msg(
        id: &str,
        parent: Option<&str>,
        run_id: Option<&str>,
        role: &str,
        content_json: &str,
        seq: i64,
    ) -> MessageRow {
        MessageRow {
            message_id: id.to_string(),
            parent_message_id: parent.map(str::to_string),
            run_id: run_id.map(str::to_string),
            role: role.to_string(),
            content_json: content_json.to_string(),
            entry_type: "message".to_string(),
            seq,
            model_call_id: None,
            committed_at_unix_ms: seq,
        }
    }

    fn event(seq: u64, run_id: &str, payload: KnownEventPayload) -> StoredEventRow {
        StoredEventRow {
            seq,
            run_id: Some(run_id.to_string()),
            event_id: format!("{run_id}-ev{seq}"),
            payload: EventPayload::Known(payload),
        }
    }

    fn segment_events(
        run: &str,
        segment: &str,
        kind: SegmentKind,
        base: u64,
        text: &str,
    ) -> Vec<StoredEventRow> {
        vec![
            event(
                base,
                run,
                KnownEventPayload::AssistantSegmentStart(AssistantSegmentStartPayload {
                    segment_id: segment.to_string(),
                    kind,
                    semantic_phase: AssistantPhase::Commentary,
                }),
            ),
            event(
                base + 1,
                run,
                KnownEventPayload::AssistantSegmentDelta(AssistantSegmentDeltaPayload {
                    segment_id: segment.to_string(),
                    delta: text.to_string(),
                    semantic_phase: AssistantPhase::Commentary,
                }),
            ),
            event(
                base + 2,
                run,
                KnownEventPayload::AssistantSegmentEnd(AssistantSegmentEndPayload {
                    segment_id: segment.to_string(),
                    semantic_phase: AssistantPhase::Commentary,
                }),
            ),
        ]
    }

    fn existing(runs: &[&str]) -> HashSet<String> {
        runs.iter().map(|r| r.to_string()).collect()
    }

    fn no_branch_user() -> HashSet<String> {
        HashSet::new()
    }

    /// 步骤④：final 内容经同一 scanner 重切——think→Reasoning、mood 剥离、
    /// 顶层未知键保留进 legacyFields。
    #[test]
    fn legacy_final_content_resplit_and_unknown_fields_kept() {
        let messages = vec![msg(
            "legacy-1",
            None,
            None,
            "assistant",
            r#"{"role":"assistant","content":[{"type":"text","text":"看<think>推理</think>答案<mood>happy</mood>"}],"moodLegacy":"happy"}"#,
            7,
        )];
        let input = ProjectionInput {
            messages: &messages,
            events: &[],
            existing_runs: &existing(&[]),
            lineage: &[],
            user_anchors_on_branch: &no_branch_user(),
        };
        let items = project_history(&input);
        assert_eq!(items.len(), 1);
        match &items[0] {
            HistoryItem::FinalMessage {
                content,
                legacy_fields,
                run,
                ..
            } => {
                assert_eq!(
                    content,
                    &vec![
                        ContentBlock::Text {
                            text: "看".to_string()
                        },
                        ContentBlock::Reasoning {
                            text: "推理".to_string()
                        },
                        ContentBlock::Text {
                            text: "答案".to_string()
                        },
                    ],
                    "think 结构化、mood 剥离、正文保持"
                );
                assert_eq!(
                    legacy_fields.get("moodLegacy"),
                    Some(&serde_json::json!("happy")),
                    "未知顶层键保留在受控 legacy 扩展"
                );
                assert_eq!(run.state, RunAssociationState::Unknown);
            }
            other => panic!("expected FinalMessage, got {other:?}"),
        }
    }

    /// 步骤④：整体不可解析的行 → LegacyRaw 原文归档。
    #[test]
    fn unparseable_row_becomes_legacy_raw() {
        let messages = vec![msg("old-1", None, None, "assistant", "not-json{{", 3)];
        let input = ProjectionInput {
            messages: &messages,
            events: &[],
            existing_runs: &existing(&[]),
            lineage: &[],
            user_anchors_on_branch: &no_branch_user(),
        };
        let items = project_history(&input);
        match &items[0] {
            HistoryItem::LegacyRaw { raw, reason, .. } => {
                assert_eq!(raw, &serde_json::json!("not-json{{"));
                assert!(reason.contains("not valid JSON"));
            }
            other => panic!("expected LegacyRaw, got {other:?}"),
        }
    }

    /// 终态诚实：有 final 的 run 不另产 RunTerminal；无 final 的 run 如实给
    /// RunTerminal（携带真实终态原因），工具卡如实配对。
    #[test]
    fn terminal_honesty_with_and_without_final() {
        let messages = vec![
            msg("user:r1", None, None, "user", r#"{"text":"q"}"#, 1),
            msg(
                "r1-final",
                Some("user:r1"),
                Some("r1"),
                "assistant",
                r#"{"role":"assistant","content":[{"type":"text","text":"a"}]}"#,
                2,
            ),
            msg(
                "user:r2",
                Some("r1-final"),
                None,
                "user",
                r#"{"text":"q2"}"#,
                3,
            ),
        ];
        let mut events = segment_events("r1", "s1", SegmentKind::Text, 10, "正文");
        events.push(event(
            20,
            "r1",
            KnownEventPayload::RunStateChanged(RunStateChangedPayload {
                from: RunStatus::Running,
                to: RunStatus::Completed,
                reason: Some("completed".to_string()),
            }),
        ));
        events.push(event(
            21,
            "r1",
            KnownEventPayload::FinalMessageCommitted(FinalMessageCommittedPayload {
                message: NormalizedMessage {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: "a".to_string(),
                    }],
                    model_call_id: None,
                },
            }),
        ));
        // r2：工具（started+completed 配对）+ 失败终态，无 final。
        events.push(event(
            30,
            "r2",
            KnownEventPayload::ToolCallStarted(ToolCallStartedPayload {
                tool_call: ToolCallDescriptor {
                    tool_call_id: ToolCallId::new("r2-tc0001"),
                    target: "read".to_string(),
                    args_digest: digest_arguments(&serde_json::json!({ "n": 1 })),
                    args_summary: None,
                },
            }),
        ));
        events.push(event(
            31,
            "r2",
            KnownEventPayload::ToolCallCompleted(ToolCallCompletedPayload {
                tool_call_id: ToolCallId::new("r2-tc0001"),
                result: ToolResultWire {
                    status: ToolResultStatus::Failed,
                    content: Vec::new(),
                    resource_refs: Vec::new(),
                    truncated: false,
                    error: None,
                },
            }),
        ));
        events.push(event(
            32,
            "r2",
            KnownEventPayload::RunStateChanged(RunStateChangedPayload {
                from: RunStatus::Running,
                to: RunStatus::Failed,
                reason: Some("provider_failed".to_string()),
            }),
        ));
        let branch_users = existing(&["r1", "r2"]); // 两个 user 锚都在分支上
        let input = ProjectionInput {
            messages: &messages,
            events: &events,
            existing_runs: &existing(&["r1", "r2"]),
            lineage: &[],
            user_anchors_on_branch: &branch_users,
        };
        let items = project_history(&input);
        let kinds: Vec<&str> = items
            .iter()
            .map(|i| match i {
                HistoryItem::UserMessage { .. } => "user",
                HistoryItem::AssistantSegment { .. } => "segment",
                HistoryItem::ToolCall { .. } => "tool",
                HistoryItem::FinalMessage { .. } => "final",
                HistoryItem::RunTerminal { .. } => "terminal",
                _ => "other",
            })
            .collect();
        // r1：user 锚挂段组；final 锚只产 FinalMessage（组项不重复、有 fmc
        // 不另产终态项）。r2：user 锚挂工具组 + 诚实终态。
        assert_eq!(
            kinds,
            vec!["user", "segment", "final", "user", "tool", "terminal"],
            "{kinds:?}"
        );
        let terminal = items
            .iter()
            .find_map(|i| match i {
                HistoryItem::RunTerminal {
                    run_id,
                    status,
                    terminal_reason,
                    ..
                } => Some((run_id.clone(), *status, terminal_reason.clone())),
                _ => None,
            })
            .expect("r2 的 RunTerminal");
        assert_eq!(terminal.0, "r2");
        assert_eq!(terminal.1, RunStatus::Failed);
        assert_eq!(terminal.2.as_deref(), Some("provider_failed"));
        let tool = items
            .iter()
            .find_map(|i| match i {
                HistoryItem::ToolCall {
                    status,
                    args_digest,
                    ..
                } => Some((*status, args_digest.is_some())),
                _ => None,
            })
            .expect("r2 的工具卡");
        assert_eq!(
            tool,
            (HistoryToolStatus::Failed, true),
            "started+completed 配对"
        );
    }

    /// 锚定：user 锚缺失（子代理 run）且无 final → lineage 并入父组。
    #[test]
    fn orphan_child_merges_into_parent_group() {
        let messages = vec![msg("user:p", None, None, "user", r#"{"text":"go"}"#, 1)];
        let mut events = segment_events("p", "ps", SegmentKind::Text, 10, "父过程");
        events.extend(segment_events("c", "cs", SegmentKind::Text, 20, "子过程"));
        events.push(event(
            30,
            "c",
            KnownEventPayload::RunStateChanged(RunStateChangedPayload {
                from: RunStatus::Running,
                to: RunStatus::Completed,
                reason: Some("completed.no_final.process_only".to_string()),
            }),
        ));
        let lineage = vec![("p".to_string(), "c".to_string())];
        let branch_users = existing(&["p"]);
        let input = ProjectionInput {
            messages: &messages,
            events: &events,
            existing_runs: &existing(&["p", "c"]),
            lineage: &lineage,
            user_anchors_on_branch: &branch_users,
        };
        let items = project_history(&input);
        let kinds: Vec<&str> = items
            .iter()
            .map(|i| match i {
                HistoryItem::UserMessage { .. } => "user",
                HistoryItem::AssistantSegment { segment_id, .. } => {
                    if segment_id == "ps" {
                        "seg-p"
                    } else {
                        "seg-c"
                    }
                }
                HistoryItem::RunTerminal { run_id, .. } => {
                    if run_id == "c" {
                        "term-c"
                    } else {
                        "term-p"
                    }
                }
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["user", "seg-p", "seg-c", "term-c"],
            "子 run 组按事件 seq 并入父组，终态如实: {kinds:?}"
        );
    }

    /// fork 副本（run_id NULL）→ run 关联 unknown（T03 D9）。
    #[test]
    fn fork_copy_run_association_is_unknown() {
        let messages = vec![msg(
            "r9-final",
            None,
            None,
            "assistant",
            r#"{"role":"assistant","content":[{"type":"text","text":"a"}]}"#,
            5,
        )];
        let input = ProjectionInput {
            messages: &messages,
            events: &[],
            existing_runs: &existing(&[]),
            lineage: &[],
            user_anchors_on_branch: &no_branch_user(),
        };
        let items = project_history(&input);
        match &items[0] {
            HistoryItem::FinalMessage { run, .. } => {
                assert_eq!(run.state, RunAssociationState::Unknown);
            }
            other => panic!("expected FinalMessage, got {other:?}"),
        }
    }
}
