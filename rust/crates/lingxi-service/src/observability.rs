//! R07-T10 观测查询、筛选、详情、导出与旧库导入。
//!
//! 数据来源只有 R05 权威用量账本（model_call_usage）+ V24 旧 trace 映射
//! 表——不另建会猜测归属的聚合日志。响应形状复刻旧 Node 观测契约
//! （shared/model-observability-api-contract.ts）的字段名，账本没有的
//! 维度（payload/blob/provenance 图）诚实填空值，不编造。
//!
//! 鉴权：整个面是 LocalOnly（对齐旧面 STUDIO_OWNER/LOCAL_ONLY 分类）。

use std::path::Path;

use axum::body::Body;
use axum::extract::{Path as RoutePath, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use base64::Engine as _;
use lingxi_adapters::storage::run_store::{
    LegacyUsageImportRow, UsageGroupBy, UsagePage,
};
use lingxi_kernel::usage::{CallOutcome, ModelCallUsageRecord, UsageFilter};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::{EndpointError, ServiceState};

/// 明细页硬上限（与旧面 limit 上限一致）。
const PAGE_MAX_LIMIT: u32 = 200;
const PAGE_DEFAULT_LIMIT: u32 = 50;
/// 导出行数上限（旧面默认 50k / 硬顶 100k）。
const EXPORT_DEFAULT_MAX: u32 = 50_000;
const EXPORT_HARD_MAX: u32 = 100_000;
const EXPORT_SCHEMA_VERSION: u32 = 1;

pub(crate) fn routes() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/observability/health", get(observability_health))
        .route("/lingxi/v1/observability/query/calls", post(query_calls))
        .route("/lingxi/v1/observability/query/aggregate", post(query_aggregate))
        .route("/lingxi/v1/observability/query/traces", post(query_traces))
        .route("/lingxi/v1/observability/calls/{call_id}", get(call_detail))
        .route("/lingxi/v1/observability/traces/{trace_id}", get(trace_detail))
        .route("/lingxi/v1/observability/export", post(export_calls))
        .route("/lingxi/v1/observability/import", post(import_legacy))
}

// ---------------------------------------------------------------------------
// filter DTO（单值或数组皆收，归一成多值；未知字段显式 400 由 deny_unknown_fields 承担）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<String> {
        match self {
            OneOrMany::One(v) => vec![v],
            OneOrMany::Many(v) => v,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObservabilityFilterInput {
    /// ISO-8601 或 unix ms（含）。
    #[serde(default)]
    since: Option<serde_json::Value>,
    /// ISO-8601 或 unix ms（不含）。
    #[serde(default)]
    until: Option<serde_json::Value>,
    #[serde(default)]
    provider: Option<OneOrMany>,
    #[serde(default)]
    model: Option<OneOrMany>,
    /// 旧面 modelId 别名。
    #[serde(default)]
    model_id: Option<OneOrMany>,
    /// 旧面 category/subsystem 与 callPurpose 都投影到账本 purpose。
    #[serde(default)]
    category: Option<OneOrMany>,
    #[serde(default)]
    subsystem: Option<OneOrMany>,
    #[serde(default)]
    call_purpose: Option<OneOrMany>,
    #[serde(default)]
    purpose: Option<OneOrMany>,
    #[serde(default)]
    session_id: Option<OneOrMany>,
    #[serde(default)]
    run_id: Option<OneOrMany>,
    #[serde(default)]
    origin: Option<OneOrMany>,
    /// ok/error/aborted/incomplete（旧词汇，映射到账本 outcome）。
    #[serde(default)]
    terminal_status: Option<OneOrMany>,
    #[serde(default)]
    outcome: Option<OneOrMany>,
    /// native:<sessionId> 形态（聊天区旧调用点用 sessionPath）。
    #[serde(default)]
    session_path: Option<OneOrMany>,
}

fn bad_query(field: &'static str, detail: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        axum::Json(serde_json::json!({
            "error": "invalid_query",
            "field": field,
            "message": detail.into(),
        })),
    )
        .into_response()
}

/// ISO-8601（含 Z 或偏移）或 unix ms 数字 → unix ms。
fn parse_instant(value: &serde_json::Value, field: &'static str) -> Result<u64, Response> {
    match value {
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| bad_query(field, "expected unix milliseconds")),
        serde_json::Value::String(s) => {
            if let Ok(n) = s.parse::<u64>() {
                return Ok(n);
            }
            let parsed = chrono::DateTime::parse_from_rfc3339(s)
                .map_err(|_| bad_query(field, "expected RFC3339/ISO-8601 or unix milliseconds"))?;
            u64::try_from(parsed.timestamp_millis())
                .map_err(|_| bad_query(field, "timestamp out of range"))
        }
        _ => Err(bad_query(field, "expected RFC3339/ISO-8601 or unix milliseconds")),
    }
}

fn map_outcome(word: &str) -> Result<CallOutcome, Response> {
    match word {
        "ok" | "succeeded" => Ok(CallOutcome::Succeeded),
        "error" | "failed" => Ok(CallOutcome::Failed),
        "aborted" | "cancelled" => Ok(CallOutcome::Cancelled),
        // 旧面 incomplete ≈ 结算未观测；账本诚实词汇是 unknown。
        "incomplete" | "unknown" => Ok(CallOutcome::Unknown),
        other => Err(bad_query("outcome", format!("unknown outcome {other:?}"))),
    }
}

fn native_session_id(raw: String) -> String {
    raw.strip_prefix("native:").map(str::to_string).unwrap_or(raw)
}

impl ObservabilityFilterInput {
    fn into_filter(self) -> Result<UsageFilter, Response> {
        let since = self
            .since
            .map(|v| parse_instant(&v, "since"))
            .transpose()?;
        let until = self
            .until
            .map(|v| parse_instant(&v, "until"))
            .transpose()?;
        // until 不含：界面上语义为开区间，账本比较是闭区间——上界减 1ms。
        let until_exclusive = until.map(|v| v.saturating_sub(1));
        let mut purposes = Vec::new();
        for alias in [self.category, self.subsystem, self.call_purpose, self.purpose] {
            if let Some(v) = alias {
                purposes.extend(v.into_vec());
            }
        }
        purposes.sort();
        purposes.dedup();
        let mut models = Vec::new();
        for alias in [self.model, self.model_id] {
            if let Some(v) = alias {
                models.extend(v.into_vec());
            }
        }
        models.sort();
        models.dedup();
        let mut session_ids: Vec<String> = self
            .session_id
            .map(OneOrMany::into_vec)
            .unwrap_or_default()
            .into_iter()
            .map(native_session_id)
            .collect();
        if let Some(paths) = self.session_path {
            session_ids.extend(paths.into_vec().into_iter().map(native_session_id));
        }
        session_ids.sort();
        session_ids.dedup();
        let mut outcomes = Vec::new();
        for alias in [self.terminal_status, self.outcome] {
            if let Some(v) = alias {
                for word in v.into_vec() {
                    outcomes.push(map_outcome(&word)?);
                }
            }
        }
        outcomes.sort_by_key(|o| o.wire_name());
        outcomes.dedup();
        Ok(UsageFilter {
            owner_user_id: None,
            session_ids,
            run_ids: self.run_id.map(OneOrMany::into_vec).unwrap_or_default(),
            purposes,
            models,
            providers: self.provider.map(OneOrMany::into_vec).unwrap_or_default(),
            origins: self.origin.map(OneOrMany::into_vec).unwrap_or_default(),
            outcomes,
            recorded_from_unix_ms: since,
            recorded_to_unix_ms: until_exclusive,
        })
    }
}

// ---------------------------------------------------------------------------
// cursor：keyset（rowid），与 filter 指纹绑定——filter 变了 cursor 即失效。
// ---------------------------------------------------------------------------

fn filter_fingerprint(filter: &UsageFilter) -> String {
    let canonical = serde_json::json!({
        "sessionIds": filter.session_ids,
        "runIds": filter.run_ids,
        "purposes": filter.purposes,
        "models": filter.models,
        "providers": filter.providers,
        "origins": filter.origins,
        "outcomes": filter.outcomes.iter().map(|o| o.wire_name()).collect::<Vec<_>>(),
        "from": filter.recorded_from_unix_ms,
        "to": filter.recorded_to_unix_ms,
    });
    let digest = Sha256::digest(canonical.to_string().as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

fn encode_cursor(filter: &UsageFilter, last_rowid: i64) -> String {
    let payload = serde_json::json!({
        "v": 1,
        "fp": filter_fingerprint(filter),
        "r": last_rowid,
    });
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.to_string())
}

fn decode_cursor(filter: &UsageFilter, raw: &str) -> Result<Option<i64>, Response> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| bad_query("cursor", "cursor is not base64url"))?;
    if bytes.len() > 512 {
        return Err(bad_query("cursor", "cursor too long"));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| bad_query("cursor", "cursor is not the expected shape"))?;
    if value.get("v").and_then(|v| v.as_u64()) != Some(1)
        || value.get("fp").and_then(|v| v.as_str()) != Some(filter_fingerprint(filter).as_str())
    {
        return Err(bad_query("cursor", "cursor does not match the current filter"));
    }
    let rowid = value
        .get("r")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| bad_query("cursor", "cursor is missing its position"))?;
    Ok(Some(rowid))
}

// ---------------------------------------------------------------------------
// 投影：账本行 → 旧契约形状（缺的维度诚实空值）
// ---------------------------------------------------------------------------

fn iso(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn terminal_status(outcome: CallOutcome) -> Option<&'static str> {
    match outcome {
        CallOutcome::Succeeded => Some("ok"),
        CallOutcome::Failed => Some("error"),
        CallOutcome::Cancelled => Some("aborted"),
        CallOutcome::Unknown => None,
    }
}

fn record_to_list_item(record: &ModelCallUsageRecord, legacy_trace: Option<&(String, Option<String>)>) -> serde_json::Value {
    let usage_summary = record.usage.as_ref().map(|u| {
        let input = u.input_tokens;
        let output = u.output_tokens;
        let total = match (input, output) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        };
        serde_json::json!({
            "inputTokens": input,
            "outputTokens": output,
            "reasoningTokens": u.reasoning_tokens,
            "cacheReadTokens": u.cache_read_tokens,
            "cacheWriteTokens": u.cache_write_tokens,
            "totalTokens": total,
            "costTotal": serde_json::Value::Null,
        })
    });
    let usage_availability = if record.invalid_detail.is_some() {
        "corrupt"
    } else if record.usage.is_some() {
        "present"
    } else {
        "unknown"
    };
    let duration = match (record.started_at_unix_ms, record.settled_at_unix_ms) {
        (Some(a), Some(b)) if b >= a => Some(b - a),
        _ => None,
    };
    serde_json::json!({
        "callId": record.model_call_id,
        "traceId": legacy_trace.map(|t| t.0.clone()),
        "parentCallId": legacy_trace.and_then(|t| t.1.clone()),
        "startedAt": record.started_at_unix_ms.map(iso),
        "endedAt": record.settled_at_unix_ms.map(iso),
        "durationMs": duration,
        "terminalStatus": terminal_status(record.outcome),
        "outcome": record.outcome.wire_name(),
        "model": {
            "provider": record.provider,
            "modelId": record.model,
            "api": record.protocol,
        },
        "source": { "subsystem": record.purpose, "operation": record.origin },
        "attribution": {
            "sessionId": record.session_id,
            "sessionPath": record.session_id.as_ref().map(|s| format!("native:{s}")),
            "runId": record.run_id,
            "attempt": record.attempt,
        },
        "callPurpose": record.purpose,
        "origin": record.origin,
        "usage": {
            "availability": usage_availability,
            "summary": usage_summary,
        },
        "invalidDetail": record.invalid_detail,
        "attemptCount": record.transport_attempts,
        "providerRequestCount": record.transport_attempts,
        "payloadAvailability": "not_captured",
        "payloadRecordCount": 0,
        "interruptedByRestart": false,
        "recordedAt": record.started_at_unix_ms.or(record.settled_at_unix_ms).map(iso),
        "causation": {
            "parentRunId": record.parent_run_id,
            "causeRef": record.cause_ref,
            "parentToolCallId": record.parent_tool_call_id,
            "emittedToolCalls": record.emitted_tool_calls,
        },
        "costBasis": record.cost_basis,
    })
}

async fn load_legacy_trace(
    state: &ServiceState,
    call_id: &str,
) -> Result<Option<(String, Option<String>)>, Response> {
    state
        .storage()
        .legacy_trace_of(call_id)
        .await
        .map_err(|err| EndpointError::storage(&err).into_response())
}

// ---------------------------------------------------------------------------
// 路由
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryCallsInput {
    #[serde(default)]
    filter: Option<ObservabilityFilterInput>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    cursor: Option<String>,
}

async fn query_calls(
    State(state): State<ServiceState>,
    input: Result<axum::Json<QueryCallsInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let axum::Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let filter = match input.filter.unwrap_or_default().into_filter() {
        Ok(f) => f,
        Err(r) => return r,
    };
    let limit = input.limit.unwrap_or(PAGE_DEFAULT_LIMIT);
    if limit == 0 || limit > PAGE_MAX_LIMIT {
        return bad_query("limit", format!("limit must be 1..={PAGE_MAX_LIMIT}"));
    }
    let after = match input.cursor.as_deref() {
        None => None,
        Some(raw) => match decode_cursor(&filter, raw) {
            Ok(v) => v,
            Err(r) => return r,
        },
    };
    // 多取一行探测下一页。
    let page = match state
        .storage()
        .query_model_call_usage_page(filter.clone(), after, limit + 1)
        .await
    {
        Ok(p) => p,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    let UsagePage { mut rows } = page;
    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|(rowid, _)| encode_cursor(&filter, *rowid))
    } else {
        None
    };
    let mut calls = Vec::with_capacity(rows.len());
    for (_, record) in rows {
        let legacy = match load_legacy_trace(&state, &record.model_call_id).await {
            Ok(v) => v,
            Err(r) => return r,
        };
        calls.push(record_to_list_item(&record, legacy.as_ref()));
    }
    axum::Json(serde_json::json!({
        "calls": calls,
        "nextCursor": next_cursor,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryAggregateInput {
    #[serde(default)]
    filter: Option<ObservabilityFilterInput>,
    /// date/provider/model/purpose/session（sessionId 别名兼容）。
    #[serde(default)]
    group_by: Option<Vec<String>>,
}

fn parse_group_by(raw: &[String]) -> Result<Vec<UsageGroupBy>, Response> {
    let mut out = Vec::new();
    for word in raw {
        out.push(match word.as_str() {
            "date" => UsageGroupBy::Date,
            "provider" => UsageGroupBy::Provider,
            "model" | "modelId" => UsageGroupBy::Model,
            "purpose" | "category" | "subsystem" | "callPurpose" => UsageGroupBy::Purpose,
            "session" | "sessionId" => UsageGroupBy::Session,
            other => return Err(bad_query("groupBy", format!("unknown group dimension {other:?}"))),
        });
    }
    Ok(out)
}

async fn query_aggregate(
    State(state): State<ServiceState>,
    input: Result<axum::Json<QueryAggregateInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let axum::Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let filter = match input.filter.unwrap_or_default().into_filter() {
        Ok(f) => f,
        Err(r) => return r,
    };
    let group_by = match parse_group_by(&input.group_by.unwrap_or_default()) {
        Ok(g) => g,
        Err(r) => return r,
    };
    let rows = match state
        .storage()
        .aggregate_model_call_usage(filter, &group_by)
        .await
    {
        Ok(r) => r,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    let mut overall = serde_json::json!({
        "callCount": 0, "okCount": 0, "errorCount": 0, "cancelledCount": 0,
        "unknownCount": 0, "usageMissingCount": 0,
        "inputTokens": 0, "outputTokens": 0, "cacheReadTokens": 0,
        "cacheWriteTokens": 0, "reasoningTokens": 0, "transportAttempts": 0,
    });
    let mut groups = Vec::new();
    for (key, agg) in &rows {
        let metrics = serde_json::json!({
            "callCount": agg.call_count,
            "okCount": agg.ok_count,
            "errorCount": agg.error_count,
            "abortedCount": agg.cancelled_count,
            "cancelledCount": agg.cancelled_count,
            "unknownCount": agg.unknown_count,
            "usageMissingCount": agg.usage_missing_count,
            "usageAggregateAvailability": aggregate_availability(agg.call_count, agg.usage_missing_count),
            "inputTokens": agg.input_tokens,
            "outputTokens": agg.output_tokens,
            "cacheReadTokens": agg.cache_read_tokens,
            "cacheWriteTokens": agg.cache_write_tokens,
            "reasoningTokens": agg.reasoning_tokens,
            "transportAttempts": agg.transport_attempts,
        });
        // values 按维度名落位（旧契约：date/provider/modelId/purpose/sessionId）。
        let values: serde_json::Map<String, serde_json::Value> = group_by
            .iter()
            .zip(key.iter())
            .map(|(dim, v)| {
                let name = match dim {
                    UsageGroupBy::Date => "date",
                    UsageGroupBy::Provider => "provider",
                    UsageGroupBy::Model => "modelId",
                    UsageGroupBy::Purpose => "purpose",
                    UsageGroupBy::Session => "sessionId",
                };
                (
                    name.to_string(),
                    v.clone().map(serde_json::Value::from).unwrap_or(serde_json::Value::Null),
                )
            })
            .collect();
        accumulate(&mut overall, agg);
        groups.push(serde_json::json!({
            "key": key.iter().map(|v| v.clone().unwrap_or_else(|| "∅".to_string())).collect::<Vec<_>>().join("::"),
            "values": values,
            "metrics": metrics,
        }));
    }
    finalize_overall(&mut overall, &rows);
    axum::Json(serde_json::json!({ "groups": groups, "overall": overall })).into_response()
}

fn accumulate(overall: &mut serde_json::Value, agg: &lingxi_adapters::storage::run_store::UsageAggregate) {
    let map = overall.as_object_mut().unwrap();
    for (key, value) in [
        ("callCount", Some(agg.call_count)),
        ("okCount", Some(agg.ok_count)),
        ("errorCount", Some(agg.error_count)),
        ("cancelledCount", Some(agg.cancelled_count)),
        ("unknownCount", Some(agg.unknown_count)),
        ("usageMissingCount", Some(agg.usage_missing_count)),
        ("inputTokens", agg.input_tokens),
        ("outputTokens", agg.output_tokens),
        ("cacheReadTokens", agg.cache_read_tokens),
        ("cacheWriteTokens", agg.cache_write_tokens),
        ("reasoningTokens", agg.reasoning_tokens),
        ("transportAttempts", agg.transport_attempts),
    ] {
        if let Some(v) = value {
            let slot = map.get_mut(key).unwrap();
            *slot = serde_json::json!(slot.as_u64().unwrap_or(0) + v);
        }
    }
}

/// 旧契约词汇：complete=每行都有 usage；partial=部分有；全缺或无行=unknown。
fn aggregate_availability(call_count: u64, usage_missing_count: u64) -> &'static str {
    if call_count == 0 || usage_missing_count == call_count {
        "unknown"
    } else if usage_missing_count == 0 {
        "complete"
    } else {
        "partial"
    }
}

fn finalize_overall(
    overall: &mut serde_json::Value,
    rows: &[(Vec<Option<String>>, lingxi_adapters::storage::run_store::UsageAggregate)],
) {
    use lingxi_adapters::storage::run_store::UsageAggregate;
    // 全空时 token 合计保持 null（没有任何行报过数 ≠ 0）。
    let map = overall.as_object_mut().unwrap();
    let all_none = |pick: fn(&UsageAggregate) -> Option<u64>| {
        rows.iter().all(|(_, agg)| pick(agg).is_none())
    };
    let picks: [(&str, fn(&UsageAggregate) -> Option<u64>); 6] = [
        ("inputTokens", |a| a.input_tokens),
        ("outputTokens", |a| a.output_tokens),
        ("cacheReadTokens", |a| a.cache_read_tokens),
        ("cacheWriteTokens", |a| a.cache_write_tokens),
        ("reasoningTokens", |a| a.reasoning_tokens),
        ("transportAttempts", |a| a.transport_attempts),
    ];
    for (key, pick) in picks {
        if all_none(pick) {
            map.insert(key.to_string(), serde_json::Value::Null);
        }
    }
    if let Some(aborted) = map.remove("cancelledCount") {
        map.insert("abortedCount".to_string(), aborted.clone());
        map.insert("cancelledCount".to_string(), aborted);
    }
    let call_count = map.get("callCount").and_then(|v| v.as_u64()).unwrap_or(0);
    let missing = map.get("usageMissingCount").and_then(|v| v.as_u64()).unwrap_or(0);
    map.insert(
        "usageAggregateAvailability".to_string(),
        aggregate_availability(call_count, missing).into(),
    );
}

async fn call_detail(
    State(state): State<ServiceState>,
    RoutePath(call_id): RoutePath<String>,
) -> Response {
    // 详情按主键精确取。
    let record = match load_call_by_id(&state, &call_id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({ "error": "not_found", "callId": call_id })),
            )
                .into_response()
        }
        Err(r) => return r,
    };
    let legacy = match load_legacy_trace(&state, &call_id).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let mut siblings = Vec::new();
    if let Some((trace_id, _)) = &legacy {
        match state.storage().legacy_trace_calls(trace_id).await {
            Ok(rows) => {
                for (id, parent) in rows {
                    siblings.push(serde_json::json!({ "callId": id, "parentCallId": parent }));
                }
            }
            Err(err) => return EndpointError::storage(&err).into_response(),
        }
    }
    axum::Json(serde_json::json!({
        "call": record_to_list_item(&record, legacy.as_ref()),
        "legacyTrace": legacy.as_ref().map(|(trace_id, parent)| serde_json::json!({
            "traceId": trace_id,
            "parentCallId": parent,
            "calls": siblings,
        })),
        // 新内核行的因果链在账本自身（parent_tool_call_id → 父工具 → 父模型行）。
        "causation": {
            "parentToolCallId": record.parent_tool_call_id,
            "parentRunId": record.parent_run_id,
            "emittedToolCalls": record.emitted_tool_calls,
        },
    }))
    .into_response()
}

async fn load_call_by_id(
    state: &ServiceState,
    call_id: &str,
) -> Result<Option<ModelCallUsageRecord>, Response> {
    state
        .storage()
        .model_call_usage_by_id(call_id)
        .await
        .map_err(|err| EndpointError::storage(&err).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryTracesInput {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    cursor: Option<String>,
}

async fn query_traces(
    State(state): State<ServiceState>,
    input: Result<axum::Json<QueryTracesInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let axum::Json(_input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    // 轨迹面只覆盖导入的旧 trace（新内核的因果链在账本自身，不伪造 trace）。
    let rows = match state.storage().legacy_trace_list().await {
        Ok(v) => v,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    let traces: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(trace_id, call_count)| {
            serde_json::json!({
                "traceId": trace_id,
                "origin": "imported",
                "callCount": call_count,
            })
        })
        .collect();
    axum::Json(serde_json::json!({ "traces": traces, "nextCursor": null })).into_response()
}

async fn trace_detail(
    State(state): State<ServiceState>,
    RoutePath(trace_id): RoutePath<String>,
) -> Response {
    let rows = match state.storage().legacy_trace_calls(&trace_id).await {
        Ok(v) => v,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    if rows.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({ "error": "not_found", "traceId": trace_id })),
        )
            .into_response();
    }
    let mut calls = Vec::new();
    let mut edges = Vec::new();
    for (call_id, parent) in &rows {
        match load_call_by_id(&state, call_id).await {
            Ok(Some(record)) => calls.push(record_to_list_item(
                &record,
                Some(&(trace_id.clone(), parent.clone())),
            )),
            Ok(None) => {
                // 映射行在、账本金身不在 = 损坏，响亮报。
                return EndpointError::storage(&lingxi_kernel::ports::StorageError::Corrupted {
                    detail: format!("legacy trace row {call_id} has no ledger row"),
                })
                .into_response();
            }
            Err(r) => return r,
        }
        if let Some(parent) = parent {
            edges.push(serde_json::json!({ "parentCallId": parent, "childCallId": call_id }));
        }
    }
    axum::Json(serde_json::json!({
        "trace": { "traceId": trace_id, "origin": "imported", "callCount": calls.len() },
        "calls": calls,
        "edges": edges,
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// 导出（NDJSON 流：manifest + 每行一条调用）
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExportInput {
    #[serde(default)]
    filter: Option<ObservabilityFilterInput>,
    #[serde(default)]
    max_calls: Option<u32>,
}

async fn export_calls(
    State(state): State<ServiceState>,
    input: Result<axum::Json<ExportInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let axum::Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let filter = match input.filter.unwrap_or_default().into_filter() {
        Ok(f) => f,
        Err(r) => return r,
    };
    let max = input.max_calls.unwrap_or(EXPORT_DEFAULT_MAX);
    if max == 0 || max > EXPORT_HARD_MAX {
        return bad_query("maxCalls", format!("maxCalls must be 1..={EXPORT_HARD_MAX}"));
    }
    // 先合计判定上限（matchedCalls 用于 413 回执）。
    let totals = match state
        .storage()
        .aggregate_model_call_usage(filter.clone(), &[])
        .await
    {
        Ok(r) => r,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    let matched = totals.first().map(|(_, a)| a.call_count).unwrap_or(0);
    if matched > u64::from(max) {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            axum::Json(serde_json::json!({
                "error": "export_limit",
                "code": "limit_error",
                "matchedCalls": matched,
                "maxCalls": max,
            })),
        )
            .into_response();
    }
    // 全量拉取（已封顶）逐行序列化。账本行本身不含正文——导出天然无
    // 明文 prompt；脱敏纪律由「写入时不落正文」承担。
    let page = match state
        .storage()
        .query_model_call_usage_page(filter.clone(), None, max)
        .await
    {
        Ok(p) => p,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    let mut body = String::new();
    body.push_str(
        &serde_json::json!({
            "type": "manifest",
            "exportSchemaVersion": EXPORT_SCHEMA_VERSION,
            "exportedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "includePayloads": false,
            "totalCalls": page.rows.len(),
            "backfillSource": serde_json::Value::Null,
        })
        .to_string(),
    );
    body.push('\n');
    for (_, record) in &page.rows {
        let legacy = match state
            .storage()
            .legacy_trace_of(&record.model_call_id)
            .await
        {
            Ok(v) => v,
            Err(err) => return EndpointError::storage(&err).into_response(),
        };
        body.push_str(
            &serde_json::json!({
                "type": "model_call",
                "schemaVersion": EXPORT_SCHEMA_VERSION,
                "call": record_to_list_item(record, legacy.as_ref()),
                "payloads": [],
            })
            .to_string(),
        );
        body.push('\n');
    }
    let mut response = Response::new(Body::from(body));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("x-export-schema-version"),
        HeaderValue::from_str(&EXPORT_SCHEMA_VERSION.to_string()).unwrap(),
    );
    response
}

// ---------------------------------------------------------------------------
// 旧库导入（observability.sqlite v1..v7，只读打开，幂等，原 id 原 trace 保留）
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportInput {
    /// 旧 observability.sqlite 的绝对路径。
    path: String,
}

async fn import_legacy(
    State(state): State<ServiceState>,
    input: Result<axum::Json<ImportInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let axum::Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let path = Path::new(&input.path);
    if !path.is_absolute() {
        return bad_query("path", "path must be absolute");
    }
    if !path.is_file() {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({ "error": "not_found", "path": input.path })),
        )
            .into_response();
    }
    let rows = match read_legacy_store(path) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let total_traces = rows.iter().filter(|r| r.legacy_trace_id.is_some()).count();
    let now = state.clock().now_unix_ms();
    let stats = match state.storage().import_legacy_model_calls(rows, now).await {
        Ok(s) => s,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    axum::Json(serde_json::json!({
        "importedCalls": stats.inserted,
        "skippedExisting": stats.skipped_existing,
        "traceRows": total_traces,
    }))
    .into_response()
}

/// 只读打开旧库并映射成行。任何形状不符显式 400（不静默吞行）。
fn read_legacy_store(path: &Path) -> Result<Vec<LegacyUsageImportRow>, Response> {
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| bad_query("path", format!("cannot open legacy store read-only: {e}")))?;
    // 旧库 v2+ 才有 usage 投影表；v1 只有调用行。
    let has_usage: bool = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='model_call_usage'")
        .map_err(|e| bad_query("path", format!("cannot inspect legacy schema: {e}")))?
        .exists([])
        .map_err(|e| bad_query("path", format!("cannot inspect legacy schema: {e}")))?;
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
        .map_err(|e| bad_query("path", format!("legacy schema does not match v1..v7: {e}")))?;
    let mut rows = stmt
        .query([])
        .map_err(|e| bad_query("path", format!("legacy read failed: {e}")))?;
    let mut out = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| bad_query("path", format!("legacy read failed: {e}")))?
    {
        let get = |i: usize| -> Option<String> { row.get::<_, Option<String>>(i).ok().flatten() };
        let call_id = get(0)
            .ok_or_else(|| bad_query("path", "legacy row without call_id"))?;
        let started = get(9).map(|s| parse_legacy_instant(&s)).transpose()?;
        let ended = get(10).map(|s| parse_legacy_instant(&s)).transpose()?;
        let recorded = started.or(ended).unwrap_or(0);
        let outcome = match get(11).as_deref() {
            Some("ok") => CallOutcome::Succeeded,
            Some("error") => CallOutcome::Failed,
            Some("aborted") => CallOutcome::Cancelled,
            _ => CallOutcome::Unknown,
        };
        let to_u64 = |i: usize| -> Option<u64> { row.get::<_, Option<i64>>(i).ok().flatten().and_then(|v| u64::try_from(v).ok()) };
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
                purpose: get(7).or_else(|| get(6)).unwrap_or_else(|| "unknown".to_string()),
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

fn parse_legacy_instant(raw: &str) -> Result<u64, Response> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .and_then(|t| u64::try_from(t.timestamp_millis()).ok())
        .ok_or_else(|| bad_query("path", format!("legacy timestamp {raw:?} is not RFC3339")))
}

async fn observability_health(State(state): State<ServiceState>) -> Response {
    let totals = match state
        .storage()
        .aggregate_model_call_usage(UsageFilter::default(), &[])
        .await
    {
        Ok(r) => r,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    let call_count = totals.first().map(|(_, a)| a.call_count).unwrap_or(0);
    axum::Json(serde_json::json!({
        "recordingStatus": "active",
        "query": {
            "queryStatus": "ready",
            "schemaVersion": 1,
            "callCount": call_count,
        },
    }))
    .into_response()
}
