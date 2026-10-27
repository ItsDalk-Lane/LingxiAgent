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
use lingxi_adapters::storage::run_store::{LegacyUsageImportRow, UsageGroupBy, UsagePage};
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
/// 原版默认保留策略（model-observability-preferences.ts）。
const DEFAULT_TRACE_DAYS: i64 = 180;
const DEFAULT_PAYLOAD_DAYS: i64 = 30;
const DEFAULT_BLOB_DAYS: i64 = 30;

pub(crate) fn routes() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/observability/health", get(observability_health))
        .route(
            "/lingxi/v1/observability/settings",
            get(get_settings).put(update_settings),
        )
        .route("/lingxi/v1/observability/storage", get(storage_overview))
        .route(
            "/lingxi/v1/observability/maintenance/delete",
            post(maintenance_delete),
        )
        .route("/lingxi/v1/observability/query/calls", post(query_calls))
        .route(
            "/lingxi/v1/observability/query/aggregate",
            post(query_aggregate),
        )
        .route("/lingxi/v1/observability/query/traces", post(query_traces))
        .route("/lingxi/v1/observability/calls/{call_id}", get(call_detail))
        .route(
            "/lingxi/v1/observability/traces/{trace_id}",
            get(trace_detail),
        )
        .route(
            "/lingxi/v1/observability/payloads/{record_id}",
            get(payload_record),
        )
        .route(
            "/lingxi/v1/observability/blobs/{blob_id}",
            get(blob_head).head(blob_head),
        )
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
        _ => Err(bad_query(
            field,
            "expected RFC3339/ISO-8601 or unix milliseconds",
        )),
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
    raw.strip_prefix("native:")
        .map(str::to_string)
        .unwrap_or(raw)
}

impl ObservabilityFilterInput {
    fn into_filter(self) -> Result<UsageFilter, Response> {
        let since = self.since.map(|v| parse_instant(&v, "since")).transpose()?;
        let until = self.until.map(|v| parse_instant(&v, "until")).transpose()?;
        // until 不含：界面上语义为开区间，账本比较是闭区间——上界减 1ms。
        let until_exclusive = until.map(|v| v.saturating_sub(1));
        let mut purposes = Vec::new();
        for alias in [
            self.category,
            self.subsystem,
            self.call_purpose,
            self.purpose,
        ] {
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
        return Err(bad_query(
            "cursor",
            "cursor does not match the current filter",
        ));
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

fn record_to_list_item(
    record: &ModelCallUsageRecord,
    legacy_trace: Option<&(String, Option<String>)>,
) -> serde_json::Value {
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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryAggregateInput {
    #[serde(default)]
    filter: Option<ObservabilityFilterInput>,
    /// date/provider/model/purpose/session（sessionId 别名兼容）。
    #[serde(default)]
    group_by: Option<Vec<String>>,
    /// date 维度的日桶边界；groupBy 含 date 时可给。
    #[serde(default)]
    date_bucket: Option<DateBucketInput>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DateBucketInput {
    bucket: Option<String>,
    /// 本地时区偏移（分钟）；与 timeZone 二选一。
    #[serde(default)]
    utc_offset_minutes: Option<i64>,
    /// IANA 时区：本构建未链接 tz 数据库——显式 400，绝不静默错桶。
    #[serde(default)]
    time_zone: Option<String>,
}

/// dateBucket 校验 → 供 date 维度使用的偏移分钟数。
fn parse_date_bucket(
    input: Option<DateBucketInput>,
    group_by: &[UsageGroupBy],
) -> Result<i64, Response> {
    let Some(bucket) = input else {
        return Ok(0);
    };
    if !group_by.contains(&UsageGroupBy::Date) {
        return Err(bad_query(
            "dateBucket",
            "dateBucket requires groupBy dimension \"date\"",
        ));
    }
    match bucket.bucket.as_deref() {
        Some("day") | None => {}
        other => {
            return Err(bad_query(
                "dateBucket",
                format!("unsupported bucket {other:?} (only \"day\")"),
            ))
        }
    }
    if bucket.utc_offset_minutes.is_some() && bucket.time_zone.is_some() {
        return Err(bad_query(
            "dateBucket",
            "utcOffsetMinutes and timeZone are mutually exclusive",
        ));
    }
    if let Some(zone) = bucket.time_zone.as_deref() {
        let _ = zone;
        return Err(bad_query(
            "dateBucket",
            "timeZone bucketting requires an IANA tz database not linked in this build; pass utcOffsetMinutes instead",
        ));
    }
    match bucket.utc_offset_minutes {
        None => Ok(0),
        Some(offset) if (-1440..=1440).contains(&offset) => Ok(offset),
        Some(_) => Err(bad_query(
            "dateBucket",
            "utcOffsetMinutes must be within -1440..=1440",
        )),
    }
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
            other => {
                return Err(bad_query(
                    "groupBy",
                    format!("unknown group dimension {other:?}"),
                ))
            }
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
    let date_offset = match parse_date_bucket(input.date_bucket, &group_by) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let rows = match state
        .storage()
        .aggregate_model_call_usage_offset(filter, &group_by, date_offset)
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
                    v.clone()
                        .map(serde_json::Value::from)
                        .unwrap_or(serde_json::Value::Null),
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

fn accumulate(
    overall: &mut serde_json::Value,
    agg: &lingxi_adapters::storage::run_store::UsageAggregate,
) {
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
    rows: &[(
        Vec<Option<String>>,
        lingxi_adapters::storage::run_store::UsageAggregate,
    )],
) {
    use lingxi_adapters::storage::run_store::UsageAggregate;
    // 全空时 token 合计保持 null（没有任何行报过数 ≠ 0）。
    let map = overall.as_object_mut().unwrap();
    let all_none =
        |pick: fn(&UsageAggregate) -> Option<u64>| rows.iter().all(|(_, agg)| pick(agg).is_none());
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
    let missing = map
        .get("usageMissingCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
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
    // 因果链：parent = emit 过本行 parent_tool_call_id 的账本行；
    // children = parent_tool_call_id 命中本行 emitted 集合的行。
    let parent_call = match &record.parent_tool_call_id {
        Some(tool) => match state.storage().usage_call_emitting(tool).await {
            Ok(Some(parent)) => {
                let parent_legacy = match load_legacy_trace(&state, &parent.model_call_id).await {
                    Ok(v) => v,
                    Err(r) => return r,
                };
                Some(serde_json::json!({
                    "callId": parent.model_call_id,
                    "startedAt": parent.started_at_unix_ms.map(iso),
                    "terminalStatus": terminal_status(parent.outcome),
                    "traceId": parent_legacy.as_ref().map(|(t, _)| t.clone()),
                    "modelId": parent.model,
                }))
            }
            Ok(None) => None,
            Err(err) => return EndpointError::storage(&err).into_response(),
        },
        None => None,
    };
    let child_calls = match state
        .storage()
        .usage_calls_with_parent(&record.emitted_tool_calls)
        .await
    {
        Ok(children) => {
            let mut out = Vec::new();
            for child in children {
                let child_legacy = match load_legacy_trace(&state, &child.model_call_id).await {
                    Ok(v) => v,
                    Err(r) => return r,
                };
                out.push(serde_json::json!({
                    "callId": child.model_call_id,
                    "startedAt": child.started_at_unix_ms.map(iso),
                    "terminalStatus": terminal_status(child.outcome),
                    "traceId": child_legacy.as_ref().map(|(t, _)| t.clone()),
                    "modelId": child.model,
                }));
            }
            out
        }
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    axum::Json(serde_json::json!({
        "call": record_to_list_item(&record, legacy.as_ref()),
        "trace": legacy.as_ref().map(|(trace_id, _)| serde_json::json!({
            "traceId": trace_id,
            "origin": "imported",
            "firstSeenAt": serde_json::Value::Null,
            "lastSeenAt": serde_json::Value::Null,
        })),
        "legacyTrace": legacy.as_ref().map(|(trace_id, parent)| serde_json::json!({
            "traceId": trace_id,
            "parentCallId": parent,
            "calls": siblings,
        })),
        "parentCall": parent_call,
        "childCalls": child_calls,
        // 运行时不捕获 attempt 级与 payload 副本（观测账本只有汇总行）：
        // 显式空集合 + 闭集状态，不伪造层级内容。
        "attempts": [],
        "payloadRecords": [],
        "payloadAvailability": "not_captured",
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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryTracesInput {
    #[serde(default)]
    filter: Option<ObservabilityFilterInput>,
    /// 轨迹自身维度（会话轨迹 origin = 会话内最早调用的 origin）。
    #[serde(default)]
    origin: Option<String>,
    /// 只保留 call 总数 ≥ 该值的轨迹（1..=1000）。
    #[serde(default)]
    min_call_count: Option<u32>,
    /// 是否包含无会话的 singleton 辅助轨迹（默认 false）。
    #[serde(default)]
    include_singleton: Option<bool>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    cursor: Option<String>,
}

/// 轨迹查询 cursor：与 (filter 指纹, origin, minCallCount, includeSingleton)
/// 绑定，携带 keyset 位置 (first_seen_ms, trace_id)。
fn decode_trace_cursor(
    filter: &UsageFilter,
    origin: &Option<String>,
    min_calls: u32,
    include_singleton: bool,
    raw: &str,
) -> Result<Option<(i64, String)>, Response> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .map_err(|_| bad_query("cursor", "cursor is not base64url"))?;
    if bytes.len() > 512 {
        return Err(bad_query("cursor", "cursor too long"));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| bad_query("cursor", "cursor is not the expected shape"))?;
    let fingerprint_input = serde_json::json!({
        "fp": filter_fingerprint(filter),
        "origin": origin,
        "minCalls": min_calls,
        "includeSingleton": include_singleton,
    });
    if value.get("v").and_then(|v| v.as_u64()) != Some(1)
        || value.get("fp") != fingerprint_input.get("fp")
        || value.get("origin") != fingerprint_input.get("origin")
        || value.get("minCalls") != fingerprint_input.get("minCalls")
        || value.get("includeSingleton") != fingerprint_input.get("includeSingleton")
    {
        return Err(bad_query(
            "cursor",
            "cursor does not match the current trace query",
        ));
    }
    let seen = value
        .get("s")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| bad_query("cursor", "cursor is missing its position"))?;
    let trace = value
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or_else(|| bad_query("cursor", "cursor is missing its trace position"))?
        .to_string();
    Ok(Some((seen, trace)))
}

fn encode_trace_cursor(
    filter: &UsageFilter,
    origin: &Option<String>,
    min_calls: u32,
    include_singleton: bool,
    seen: i64,
    trace: &str,
) -> String {
    let json = serde_json::json!({
        "v": 1,
        "fp": filter_fingerprint(filter),
        "origin": origin,
        "minCalls": min_calls,
        "includeSingleton": include_singleton,
        "s": seen,
        "t": trace,
    });
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json.to_string().as_bytes())
}

async fn query_traces(
    State(state): State<ServiceState>,
    input: Result<axum::Json<QueryTracesInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let axum::Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    let filter = match input.filter.unwrap_or_default().into_filter() {
        Ok(f) => f,
        Err(r) => return r,
    };
    let origin = match input.origin {
        Some(raw) => {
            let trimmed = raw.trim().to_string();
            if trimmed.is_empty() || trimmed.len() > 64 {
                return bad_query("origin", "origin must be a bounded string");
            }
            Some(trimmed)
        }
        None => None,
    };
    let min_calls = input.min_call_count.unwrap_or(1);
    if !(1..=1000).contains(&min_calls) {
        return bad_query("minCallCount", "minCallCount must be an integer in 1..1000");
    }
    let include_singleton = input.include_singleton.unwrap_or(false);
    let limit = input.limit.unwrap_or(PAGE_DEFAULT_LIMIT);
    if limit == 0 || limit > PAGE_MAX_LIMIT {
        return bad_query("limit", format!("limit must be 1..={PAGE_MAX_LIMIT}"));
    }
    let after = match input.cursor.as_deref() {
        None => None,
        Some(raw) => match decode_trace_cursor(&filter, &origin, min_calls, include_singleton, raw)
        {
            Ok(v) => v,
            Err(r) => return r,
        },
    };
    // 多取一行探测下一页。
    let mut rows = match state
        .storage()
        .session_trace_page(
            filter.clone(),
            origin.as_deref(),
            u64::from(min_calls),
            include_singleton,
            after,
            limit + 1,
        )
        .await
    {
        Ok(r) => r,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    let next_cursor = if rows.len() > limit as usize {
        rows.truncate(limit as usize);
        rows.last().map(|row| {
            encode_trace_cursor(
                &filter,
                &origin,
                min_calls,
                include_singleton,
                row.first_seen_ms,
                &row.session_id,
            )
        })
    } else {
        None
    };
    let traces: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|row| {
            serde_json::json!({
                "traceId": row.session_id,
                "origin": row.origin,
                "firstSeenAt": iso(u64::try_from(row.first_seen_ms).unwrap_or(0)),
                "lastSeenAt": iso(u64::try_from(row.last_seen_ms).unwrap_or(0)),
                "callCount": row.call_count,
                "terminalOk": row.ok_count,
                "terminalError": row.error_count,
                "terminalAborted": row.cancelled_count,
                "incomplete": row.unknown_count,
            })
        })
        .collect();
    axum::Json(serde_json::json!({ "traces": traces, "nextCursor": next_cursor })).into_response()
}

async fn trace_detail(
    State(state): State<ServiceState>,
    RoutePath(trace_id): RoutePath<String>,
) -> Response {
    if let Some(session_id) = trace_id.strip_prefix("session:") {
        return session_trace_detail(&state, session_id).await;
    }
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
    let mut orphan_edges = Vec::new();
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
            if rows.iter().any(|(id, _)| id == parent) {
                edges.push(serde_json::json!({ "parentCallId": parent, "childCallId": call_id }));
            } else {
                orphan_edges.push(
                    serde_json::json!({ "childCallId": call_id, "missingParentCallId": parent }),
                );
            }
        }
    }
    let known: std::collections::HashSet<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
    let roots: Vec<serde_json::Value> = rows
        .iter()
        .filter(|(_id, parent)| {
            parent.is_none() || parent.as_deref().is_some_and(|p| !known.contains(p))
        })
        .map(|(id, parent)| serde_json::json!({ "callId": id, "orphanParent": parent.is_some() }))
        .collect();
    axum::Json(serde_json::json!({
        "trace": { "traceId": trace_id, "origin": "imported", "callCount": calls.len() },
        "calls": calls,
        "roots": roots,
        "edges": edges,
        "orphanEdges": orphan_edges,
        "graphIntegrity": if orphan_edges.is_empty() { "ok" } else { "degraded" },
        "usageAggregate": {
            "availability": "unknown",
            "coveredCalls": 0,
            "corruptCalls": 0,
            "notCorrelatedCalls": 0,
            "unknownCalls": calls.len(),
            "totalCalls": calls.len(),
            "summary": serde_json::Value::Null,
        },
        "payloadCompleteness": {
            "present": 0, "expired": 0, "dropped": 0, "notCaptured": calls.len(), "unknown": 0,
        },
        "dataCompleteness": {
            "status": "known", "droppedTraceEvents": 0, "droppedPayloadRecords": 0,
            "droppedBlobs": 0, "interruptedByRestartCalls": 0,
        },
    }))
    .into_response()
}

/// 会话轨迹详情：新内核因果链按会话聚合（A20 新旧关联——会话内调用带
/// legacy 导入映射时 traceId/parentCallId 原样呈现，不重编号）。
async fn session_trace_detail(state: &ServiceState, session_id: &str) -> Response {
    let call_ids = match state.storage().usage_call_ids_for_session(session_id).await {
        Ok(v) => v,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    if call_ids.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "error": "not_found", "traceId": format!("session:{session_id}")
            })),
        )
            .into_response();
    }
    let mut calls: Vec<(ModelCallUsageRecord, Option<(String, Option<String>)>)> = Vec::new();
    for call_id in &call_ids {
        match load_call_by_id(state, call_id).await {
            Ok(Some(record)) => {
                let legacy = match load_legacy_trace(state, call_id).await {
                    Ok(v) => v,
                    Err(r) => return r,
                };
                calls.push((record, legacy));
            }
            Ok(None) => {
                return EndpointError::storage(&lingxi_kernel::ports::StorageError::Corrupted {
                    detail: format!("session trace row {call_id} has no ledger row"),
                })
                .into_response();
            }
            Err(r) => return r,
        }
    }
    // 因果边：本会话内 emit → parent_tool_call_id 命中的行对。
    let mut emitted: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for (record, _) in &calls {
        for tool in &record.emitted_tool_calls {
            emitted.insert(tool.as_str(), record.model_call_id.as_str());
        }
    }
    let mut edges = Vec::new();
    let mut orphan_edges = Vec::new();
    let mut roots = Vec::new();
    for (record, _) in &calls {
        let parent = record
            .parent_tool_call_id
            .as_deref()
            .and_then(|t| emitted.get(t).copied());
        let trace_ref = format!("session:{session_id}");
        let _ = trace_ref;
        match parent {
            Some(parent_id) => edges.push(serde_json::json!({
                "parentCallId": parent_id, "childCallId": record.model_call_id
            })),
            None => {
                if record.parent_tool_call_id.is_some() {
                    orphan_edges.push(serde_json::json!({
                        "childCallId": record.model_call_id,
                        "missingParentCallId": record.parent_tool_call_id,
                    }));
                }
                roots.push(serde_json::json!({
                    "callId": record.model_call_id, "orphanParent": record.parent_tool_call_id.is_some()
                }));
            }
        }
    }
    let call_count = calls.len();
    let first = calls.first().and_then(|(r, _)| r.started_at_unix_ms);
    let last = calls
        .last()
        .and_then(|(r, _)| r.settled_at_unix_ms.or(r.started_at_unix_ms));
    let origin = calls.first().and_then(|(r, _)| Some(r.origin.clone()));
    let items: Vec<serde_json::Value> = calls
        .iter()
        .map(|(record, legacy)| {
            let mut item = record_to_list_item(record, legacy.as_ref());
            if let Some(object) = item.as_object_mut() {
                object.insert(
                    "traceId".to_string(),
                    serde_json::json!(format!("session:{session_id}")),
                );
            }
            item
        })
        .collect();
    axum::Json(serde_json::json!({
        "trace": {
            "traceId": format!("session:{session_id}"),
            "origin": origin,
            "firstSeenAt": first.map(iso),
            "lastSeenAt": last.map(iso),
            "callCount": call_count,
        },
        "calls": items,
        "roots": roots,
        "edges": edges,
        "orphanEdges": orphan_edges,
        "graphIntegrity": if orphan_edges.is_empty() { "ok" } else { "degraded" },
        "usageAggregate": {
            "availability": "unknown",
            "coveredCalls": 0,
            "corruptCalls": 0,
            "notCorrelatedCalls": 0,
            "unknownCalls": call_count,
            "totalCalls": call_count,
            "summary": serde_json::Value::Null,
        },
        "payloadCompleteness": {
            "present": 0, "expired": 0, "dropped": 0, "notCaptured": call_count, "unknown": 0,
        },
        "dataCompleteness": {
            "status": "known", "droppedTraceEvents": 0, "droppedPayloadRecords": 0,
            "droppedBlobs": 0, "interruptedByRestartCalls": 0,
        },
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// 导出（NDJSON 流：manifest + 每行一条调用）
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExportQueryInput {
    #[serde(default)]
    filter: Option<ObservabilityFilterInput>,
    /// 账本面按 rowid 稳定序导出；接受 sort 字段仅作形状兼容（值被忽略
    /// 前 400 校验词表，与查询面一致）。
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExportInput {
    #[serde(default)]
    query: Option<ExportQueryInput>,
    /// 系统不存在 includeRaw；includePayloads 在本构建恒为空载荷导出
    /// （运行时不捕获 payload 副本——显式空，不伪造）。
    #[serde(default)]
    include_payloads: Option<bool>,
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
    if let Some(sort) = input.query.as_ref().and_then(|q| q.sort.as_deref()) {
        if !matches!(
            sort,
            "started_at" | "startedAt" | "-started_at" | "-startedAt" | "callId" | "-callId"
        ) {
            return bad_query("sort", format!("unknown sort key {sort:?}"));
        }
    }
    let query = input.query;
    let filter = match query
        .as_ref()
        .and_then(|q| q.filter.clone())
        .unwrap_or_default()
        .into_filter()
    {
        Ok(f) => f,
        Err(r) => return r,
    };
    let include_payloads = input.include_payloads.unwrap_or(false);
    let max = input.max_calls.unwrap_or(EXPORT_DEFAULT_MAX);
    if max == 0 || max > EXPORT_HARD_MAX {
        return bad_query(
            "maxCalls",
            format!("maxCalls must be 1..={EXPORT_HARD_MAX}"),
        );
    }
    // query.limit 只收紧导出量，不放宽 maxCalls 上限。
    let max = match query.as_ref().and_then(|q| q.limit) {
        Some(limit) if limit == 0 || limit > EXPORT_HARD_MAX => {
            return bad_query("limit", format!("limit must be 1..={EXPORT_HARD_MAX}"));
        }
        Some(limit) => max.min(limit),
        None => max,
    };
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
            "includePayloads": include_payloads,
            "storageSchemaVersion": 1,
            "dataCompleteness": { "status": "known", "droppedTraceEvents": 0, "droppedPayloadRecords": 0, "droppedBlobs": 0, "interruptedByRestartCalls": 0 },
            "totalCalls": page.rows.len(),
            "backfillSource": serde_json::Value::Null,
        })
        .to_string(),
    );
    body.push('\n');
    for (_, record) in &page.rows {
        let legacy = match state.storage().legacy_trace_of(&record.model_call_id).await {
            Ok(v) => v,
            Err(err) => return EndpointError::storage(&err).into_response(),
        };
        body.push_str(
            &serde_json::json!({
                "type": "model_call",
                "schemaVersion": EXPORT_SCHEMA_VERSION,
                "call": record_to_list_item(record, legacy.as_ref()),
                "trace": legacy.as_ref().map(|(trace_id, parent)| serde_json::json!({
                    "traceId": trace_id,
                    "parentCallId": parent,
                })),
                "attempts": [],
                "usage": {
                    "usageState": record.usage.as_ref().map(|_| "present").unwrap_or("unknown"),
                    "summary": record.usage.as_ref().map(|u| serde_json::json!({
                        "inputTokens": u.input_tokens,
                        "outputTokens": u.output_tokens,
                        "reasoningTokens": u.reasoning_tokens,
                        "cacheReadTokens": u.cache_read_tokens,
                        "cacheWriteTokens": u.cache_write_tokens,
                    })),
                },
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
/// R08-T04 B3：实现下沉至 adapters `legacy_import::observability`（CLI 与
/// 端点共用同一条读链，不发明第二实现）；此处仅做错误形状包装，端点行为
/// （400 措辞与行映射）逐字节不变。
fn read_legacy_store(path: &Path) -> Result<Vec<LegacyUsageImportRow>, Response> {
    lingxi_adapters::storage::legacy_import::observability::read_legacy_observability_rows(path)
        .map_err(|e| bad_query("path", e))
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
        "storeDisabledReasonCode": serde_json::Value::Null,
        // 恒真面：账本无条件记录（对齐旧面 immutable always-enabled）。
        "persistTraceMetadata": true,
        // 本构建不捕获 payload/blob 副本——显式 false，不宣称。
        "persistPayloads": false,
        "persistBlobs": false,
        "queuedTraceEvents": 0,
        "queuedPayloadRecords": 0,
        "queuedBlobs": 0,
        "queuedUsageEntries": 0,
        "droppedTraceEvents": 0,
        "droppedPayloadRecords": 0,
        "droppedBlobs": 0,
        "droppedUsageEntries": 0,
        "writeFailures": 0,
        "maintenanceErrors": 0,
        "lastSuccessfulFlushAt": serde_json::Value::Null,
        "interruptedByRestartCalls": 0,
        "atRestEncryption": false,
        "query": {
            "queryStatus": "ready",
            "queryStatusReason": serde_json::Value::Null,
            "schemaVersion": 1,
            "accountingProjectionAvailable": true,
            "oldestCallAt": serde_json::Value::Null,
            "newestCallAt": serde_json::Value::Null,
            "callCount": call_count,
            "traceCount": 0,
            "payloadRecordCount": 0,
            "usageProjectionCount": call_count,
            "dataCompleteness": {
                "status": "known", "droppedTraceEvents": 0, "droppedPayloadRecords": 0,
                "droppedBlobs": 0, "interruptedByRestartCalls": 0,
            },
        },
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// 设置（retention 持久化；恒真面 immutable；对齐旧面 Phase 9 §五十八～六十二）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ObservabilityRetention {
    #[serde(default = "default_trace_days")]
    trace_days: i64,
    #[serde(default = "default_payload_days")]
    payload_days: i64,
    #[serde(default = "default_blob_days")]
    blob_days: i64,
}
fn default_trace_days() -> i64 {
    DEFAULT_TRACE_DAYS
}
fn default_payload_days() -> i64 {
    DEFAULT_PAYLOAD_DAYS
}
fn default_blob_days() -> i64 {
    DEFAULT_BLOB_DAYS
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ObservabilitySettingsFile {
    #[serde(default = "settings_schema_version")]
    schema_version: u32,
    #[serde(default)]
    retention: ObservabilityRetention,
}
fn settings_schema_version() -> u32 {
    1
}

impl Default for ObservabilitySettingsFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            retention: ObservabilityRetention::default(),
        }
    }
}

impl Default for ObservabilityRetention {
    fn default() -> Self {
        Self {
            trace_days: DEFAULT_TRACE_DAYS,
            payload_days: DEFAULT_PAYLOAD_DAYS,
            blob_days: DEFAULT_BLOB_DAYS,
        }
    }
}

fn settings_path(state: &ServiceState) -> std::path::PathBuf {
    state
        .config()
        .data_home
        .join("model-observability-settings.json")
}

fn load_settings_file(state: &ServiceState) -> Result<ObservabilitySettingsFile, Response> {
    let path = settings_path(state);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({
                    "error": "query_failed",
                    "code": "settings_unreadable",
                    "message": format!("model observability settings damaged: {err}"),
                })),
            )
                .into_response()
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(ObservabilitySettingsFile::default())
        }
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({
                "error": "query_failed",
                "code": "settings_unreadable",
                "message": format!("model observability settings unreadable: {err}"),
            })),
        )
            .into_response()),
    }
}

fn settings_response(desired: &ObservabilitySettingsFile) -> serde_json::Value {
    serde_json::json!({
        "desired": {
            // 恒真面：引擎无条件记账本；payload/blob 本构建无捕获链。
            "enabled": true,
            "persistTraceMetadata": true,
            "persistPayloads": false,
            "persistBlobs": false,
            "retention": {
                "traceDays": desired.retention.trace_days,
                "payloadDays": desired.retention.payload_days,
                "blobDays": desired.retention.blob_days,
            },
        },
        "effective": {
            "recordingStatus": "active",
            "storeDisabledReasonCode": serde_json::Value::Null,
            "persistTraceMetadata": true,
            "persistPayloads": false,
            "persistBlobs": false,
            "schemaVersion": 1,
        },
        "cryptographicallyEncryptedAtRest": false,
    })
}

async fn get_settings(State(state): State<ServiceState>) -> Response {
    let desired = match load_settings_file(&state) {
        Ok(v) => v,
        Err(r) => return r,
    };
    axum::Json(settings_response(&desired)).into_response()
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsUpdateInput {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    persist_trace_metadata: Option<bool>,
    #[serde(default)]
    persist_payloads: Option<bool>,
    #[serde(default)]
    persist_blobs: Option<bool>,
    #[serde(default)]
    retention: Option<serde_json::Value>,
}

fn bad_setting(field: &'static str, message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        axum::Json(serde_json::json!({
            "error": "invalid_query",
            "code": "invalid_filter",
            "field": field,
            "message": message.into(),
        })),
    )
        .into_response()
}

/// retention 天数合法域（原版语义：有限正数）。
fn retention_day(value: &serde_json::Value, field: &'static str) -> Result<i64, Response> {
    let n = value
        .as_i64()
        .ok_or_else(|| bad_setting(field, format!("{field} must be an integer")))?;
    if !(1..=36_500).contains(&n) {
        return Err(bad_setting(field, format!("{field} must be in 1..=36500")));
    }
    Ok(n)
}

async fn update_settings(
    State(state): State<ServiceState>,
    input: Result<axum::Json<SettingsUpdateInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let axum::Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    // 恒真面拒绝关闭；payload/blob 本构建恒关（置 true 同样 409——不能
    // 声称开启不存在的捕获链）。
    for (field, value) in [
        ("enabled", input.enabled),
        ("persistTraceMetadata", input.persist_trace_metadata),
    ] {
        match value {
            Some(false) => {
                return (
                    StatusCode::CONFLICT,
                    axum::Json(serde_json::json!({
                        "error": "immutable_setting", "code": "immutable_setting",
                        "field": field,
                        "message": format!("{field} is always enabled and cannot be turned off"),
                    })),
                )
                    .into_response()
            }
            Some(true) | None => {}
        }
    }
    for (field, value) in [
        ("persistPayloads", input.persist_payloads),
        ("persistBlobs", input.persist_blobs),
    ] {
        if value.is_some() {
            return (
                StatusCode::CONFLICT,
                axum::Json(serde_json::json!({
                    "error": "immutable_setting", "code": "immutable_setting",
                    "field": field,
                    "message": format!("{field} is fixed in this build (no capture pipeline) and cannot be changed"),
                })),
            )
                .into_response();
        }
    }
    let mut desired = match load_settings_file(&state) {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Some(retention) = input.retention {
        let Some(object) = retention.as_object() else {
            return bad_setting("retention", "retention must be an object");
        };
        for key in object.keys() {
            if !matches!(key.as_str(), "traceDays" | "payloadDays" | "blobDays") {
                return bad_setting("retention", format!("unknown retention field {key:?}"));
            }
        }
        if let Some(value) = object.get("traceDays") {
            desired.retention.trace_days = match retention_day(value, "traceDays") {
                Ok(v) => v,
                Err(r) => return r,
            };
        }
        if let Some(value) = object.get("payloadDays") {
            desired.retention.payload_days = match retention_day(value, "payloadDays") {
                Ok(v) => v,
                Err(r) => return r,
            };
        }
        if let Some(value) = object.get("blobDays") {
            desired.retention.blob_days = match retention_day(value, "blobDays") {
                Ok(v) => v,
                Err(r) => return r,
            };
        }
    }
    let path = settings_path(&state);
    let body = serde_json::to_string_pretty(&desired).unwrap_or_default();
    if let Err(err) = std::fs::write(&path, body) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({
                "error": "query_failed", "code": "settings_unwritable",
                "message": format!("model observability settings could not be saved: {err}"),
            })),
        )
            .into_response();
    }
    let mut response = settings_response(&desired);
    if let Some(object) = response.as_object_mut() {
        object.insert(
            "queryHealth".to_string(),
            serde_json::json!({
                "queryStatus": "ready", "schemaVersion": 1,
            }),
        );
        object.insert("queryError".to_string(), serde_json::Value::Null);
    }
    axum::Json(response).into_response()
}

// ---------------------------------------------------------------------------
// 存储概况与手动删除（「设置」子页；本地日历日 = 服务器本地时区）
// ---------------------------------------------------------------------------

fn local_utc_offset_minutes() -> i64 {
    chrono::Local::now()
        .offset()
        .local_minus_utc()
        .saturating_div(60) as i64
}

async fn storage_overview(State(state): State<ServiceState>) -> Response {
    let days = match state
        .storage()
        .usage_daily_overview(local_utc_offset_minutes())
        .await
    {
        Ok(v) => v,
        Err(err) => return EndpointError::storage(&err).into_response(),
    };
    let db_path = state.storage().db_path().to_path_buf();
    let database_bytes: u64 = ["", "-wal", "-shm"]
        .iter()
        .map(|suffix| {
            let mut path = db_path.as_os_str().to_os_string();
            path.push(suffix);
            std::fs::metadata(std::path::PathBuf::from(path))
                .map(|meta| meta.len())
                .unwrap_or(0)
        })
        .sum();
    let calls: u64 = days.iter().map(|day| day.calls).sum();
    let per_day: Vec<serde_json::Value> = days
        .iter()
        .map(|day| {
            serde_json::json!({
                "date": day.date,
                "calls": day.calls,
                // 本构建无 payload/blob 存储：0 是真实事实。
                "mediaBytes": 0,
                "payloadChars": 0,
            })
        })
        .collect();
    axum::Json(serde_json::json!({
        "days": days.len(),
        "oldestAt": days.first().map(|d| iso(u64::try_from(d.oldest_ms).unwrap_or(0))),
        "newestAt": days.last().map(|d| iso(u64::try_from(d.newest_ms).unwrap_or(0))),
        "calls": calls,
        "sizes": {
            "databaseBytes": database_bytes,
            "mediaBytes": 0,
            "payloadEstimateChars": 0,
        },
        "perDay": per_day,
    }))
    .into_response()
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MaintenanceDeleteInput {
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    windows: Vec<DeleteWindowInput>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteWindowInput {
    from: String,
    to: String,
}

async fn maintenance_delete(
    State(state): State<ServiceState>,
    input: Result<axum::Json<MaintenanceDeleteInput>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let axum::Json(input) = match input {
        Ok(v) => v,
        Err(e) => return EndpointError::from_json_rejection(&e).into_response(),
    };
    if input.categories.is_empty()
        || !input
            .categories
            .iter()
            .all(|c| matches!(c.as_str(), "trace" | "payload" | "media"))
    {
        return bad_query(
            "categories",
            "categories must be a non-empty array of trace/payload/media",
        );
    }
    if input.windows.len() > 366 {
        return bad_query("windows", "too many windows (max 366)");
    }
    let mut windows: Vec<(String, String)> = Vec::new();
    for window in &input.windows {
        let valid = |raw: &str| {
            raw.len() == 10
                && raw.as_bytes()[4] == b'-'
                && raw.as_bytes()[7] == b'-'
                && chrono::DateTime::parse_from_rfc3339(&format!("{raw}T00:00:00Z")).is_ok()
        };
        if !valid(&window.from) || !valid(&window.to) {
            return bad_query("windows", "window from/to must be YYYY-MM-DD");
        }
        if window.from > window.to {
            return bad_query("windows", "window from must be <= to");
        }
        windows.push((window.from.clone(), window.to.clone()));
    }
    let wants_trace = input.categories.iter().any(|c| c == "trace");
    // payload/media 类别在本构建无对应存储：请求合法执行、计数如实为 0。
    let mut deleted_traces: u64 = 0;
    if wants_trace {
        let sessions = match state
            .storage()
            .session_trace_ids_in_windows(local_utc_offset_minutes(), &windows)
            .await
        {
            Ok(v) => v,
            Err(err) => return EndpointError::storage(&err).into_response(),
        };
        let legacy = match state
            .storage()
            .legacy_trace_ids_in_windows(local_utc_offset_minutes(), &windows)
            .await
        {
            Ok(v) => v,
            Err(err) => return EndpointError::storage(&err).into_response(),
        };
        // legacy trace 的行可能同时属于被删会话——先去重避免重复计数。
        let legacy_only: Vec<String> = legacy
            .into_iter()
            .filter(|trace| !sessions.iter().any(|s| s == trace))
            .collect();
        deleted_traces = (sessions.len() + legacy_only.len()) as u64;
        if windows.is_empty() {
            deleted_traces += state.storage().orphan_usage_count().await.unwrap_or(0);
        }
        match state
            .storage()
            .delete_observability_traces(&sessions, &legacy_only, windows.is_empty())
            .await
        {
            Ok(_) => {}
            Err(err) => return EndpointError::storage(&err).into_response(),
        }
    }
    // 删除后压实（SQLite VACUUM 标准做法；失败如实呈现不中断）。
    let compacted = state.storage().vacuum().await.is_ok();
    axum::Json(serde_json::json!({
        "deletedTraces": deleted_traces,
        "deletedPayloadRecords": 0,
        "deletedBlobFiles": 0,
        "compacted": compacted,
        "ranAt": chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    }))
    .into_response()
}

// ---------------------------------------------------------------------------
// Payload / Blob exact retrieval（本构建无捕获存储：显式 404/400，不伪造）
// ---------------------------------------------------------------------------

async fn payload_record(
    State(_state): State<ServiceState>,
    RoutePath(record_id): RoutePath<String>,
) -> Response {
    let Ok(id) = record_id.parse::<u64>() else {
        return bad_query("recordId", "payload record id must be a positive integer");
    };
    if id == 0 || id > 9_007_199_254_740_991 {
        return bad_query("recordId", "payload record id must be a positive integer");
    }
    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({
            "error": "not_found", "code": "not_found",
            "message": "this build records no payload copies; the ledger is metadata-only",
        })),
    )
        .into_response()
}

fn valid_blob_id(blob_id: &str) -> bool {
    let rest = blob_id.strip_prefix("mb_").unwrap_or("");
    (4..=96).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric())
}

async fn blob_head(
    State(_state): State<ServiceState>,
    RoutePath(blob_id): RoutePath<String>,
) -> Response {
    if !valid_blob_id(&blob_id) {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": "invalid_blob_id", "code": "invalid_blob_id",
                "message": "blob id must match mb_[A-Za-z0-9]{4,96}",
            })),
        )
            .into_response();
    }
    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({
            "error": "blob_missing", "code": "blob_missing",
            "message": "this build stores no observation blobs",
        })),
    )
        .into_response()
}
