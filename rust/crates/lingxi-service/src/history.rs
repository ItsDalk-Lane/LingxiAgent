//! R06-T04 统一历史读面：`GET /lingxi/v1/sessions/{id}/history`（分页）与
//! `GET /lingxi/v1/sessions/{id}/export`（全量导出）——交付物③。
//!
//! 两个面共用同一条流水线：CanonicalMessageStore 分页/全量读取 →
//! [`crate::history_projection::project_history`] 投影。四方式（实时/重开/
//! 重连/导出）语义一致由「同一投影函数 × 同一持久事实」结构性保证
//! （任务书步骤②），前端不独立解析模型结束或补造最终答案。
//!
//! 入口语义对照现役 `server/routes/sessions.ts:1415-1510`（RC-3）：
//! - `limit` 默认 50、上限 200（超上限钳制；非十进制/0/负数/重复 → 400）。
//! - `before` 是不透明游标：严格解码失败或引用不存在的本会话消息 → 400，
//!   绝不静默当作首页。
//! - `all=1`：服务端逐页遍历到底后一次返回（与客户端逐页前插装配逐项
//!   一致——同一分页路径，`export_equals_paged_concat` 锁定）。
//! - ETag `"r{headRevision}"`：无游标请求携带匹配 If-None-Match → 304；
//!   分支头推进（revision 变化）即失效。条件判定先于消息读取（O(1) 头读）。
//! - 归属闸与会话读相同：未知会话 404、跨主体 403（先于一切参数后的
//!   实体读取；ETag 判定也在闸后，不向无权限者泄露存在性）。

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use lingxi_adapters::storage::canonical_message_store as cms;
use lingxi_adapters::storage::session_tree::MessageRow;
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::StorageError;
use lingxi_protocol::history::{
    HistoryCursor, HistoryExport, HistoryItem, HistoryPage, HistoryPageMeta,
    CANONICAL_HISTORY_SCHEMA,
};
use lingxi_protocol::Cursor;

use crate::history_projection::{project_history, ProjectionInput};
use crate::sessions;
use crate::ws::parse_query_pairs;
use crate::{EndpointError, Principal, ServiceState};

/// 现役默认页大小（sessions.ts:1415-1419 `min(N || 50, 200)`）。
pub const DEFAULT_HISTORY_LIMIT: u32 = 50;
/// 现役页大小上限。
pub const MAX_HISTORY_LIMIT: u32 = 200;

/// `GET /lingxi/v1/sessions/{session_id}/history` — 统一历史分页投影。
pub async fn session_history_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let params = match parse_history_query(query.as_deref().unwrap_or("")) {
        Ok(params) => params,
        Err(err) => return err.into_response(),
    };
    if let Some(denial) = gate_session(&state, &principal, &session_id).await {
        return denial;
    }
    let db = Arc::clone(state.storage());
    // ETag 条件请求（仅无游标入口；游标页是历史切片，不做条件化）。
    let head_revision = match cms::read_branch_head_revision(&db, &session_id).await {
        Ok(revision) => revision,
        Err(err) => return storage_error_response(&err),
    };
    let etag = etag_of(head_revision);
    if params.before.is_none() && if_none_match_matches(&headers, &etag) {
        return not_modified_response(&etag);
    }
    if params.all {
        let (messages, head_revision) =
            match cms::read_full_branch(&db, &session_id, MAX_HISTORY_LIMIT).await {
                Ok(full) => full,
                Err(err) => return storage_error_response(&err),
            };
        let items = match project_items(&db, &session_id, &messages).await {
            Ok(items) => items,
            Err(err) => return storage_error_response(&err),
        };
        return page_response(
            session_id,
            head_revision,
            items,
            HistoryPageMeta {
                // 全量入口：页元信息如实标记无后续页；limit 报告内部分页大小。
                limit: MAX_HISTORY_LIMIT,
                has_more: false,
                next_before: None,
            },
        );
    }
    let page = match cms::read_branch_page(&db, &session_id, params.before, params.limit).await {
        Ok(page) => page,
        Err(err) => return storage_error_response(&err),
    };
    let head_revision = page.head_revision;
    let items = match project_items(&db, &session_id, &page.messages).await {
        Ok(items) => items,
        Err(err) => return storage_error_response(&err),
    };
    page_response(
        session_id,
        head_revision,
        items,
        HistoryPageMeta {
            limit: params.limit,
            has_more: page.has_more,
            next_before: page.next_before.map(|cursor| cursor.encode()),
        },
    )
}

/// `GET /lingxi/v1/sessions/{session_id}/export` — 全量导出（全局链序
/// 旧→新；与客户端逐页前插装配逐项一致：同一分页器、同一投影函数）。
pub async fn session_export_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    if !query.as_deref().unwrap_or("").is_empty() {
        return EndpointError::invalid_message("export takes no query parameters")
            .with_cause("history.unexpected_query")
            .into_response();
    }
    if let Some(denial) = gate_session(&state, &principal, &session_id).await {
        return denial;
    }
    let db = Arc::clone(state.storage());
    let head_revision = match cms::read_branch_head_revision(&db, &session_id).await {
        Ok(revision) => revision,
        Err(err) => return storage_error_response(&err),
    };
    let etag = etag_of(head_revision);
    if if_none_match_matches(&headers, &etag) {
        return not_modified_response(&etag);
    }
    let (messages, head_revision) =
        match cms::read_full_branch(&db, &session_id, MAX_HISTORY_LIMIT).await {
            Ok(full) => full,
            Err(err) => return storage_error_response(&err),
        };
    let items = match project_items(&db, &session_id, &messages).await {
        Ok(items) => items,
        Err(err) => return storage_error_response(&err),
    };
    let body = HistoryExport {
        schema_version: CANONICAL_HISTORY_SCHEMA.to_string(),
        session_id,
        head_revision,
        items,
    };
    let mut response = (StatusCode::OK, Json(body)).into_response();
    set_etag(&mut response, &etag_of(head_revision));
    response
}

// ── 内部流水线 ──

struct HistoryParams {
    before: Option<HistoryCursor>,
    limit: u32,
    all: bool,
}

/// 严格查询解析：只认 before/limit/all，重复或未知键一律 400
/// （镜像 events 页的闭合查询纪律）。
fn parse_history_query(query: &str) -> Result<HistoryParams, EndpointError> {
    let mut before: Option<HistoryCursor> = None;
    let mut limit: Option<u32> = None;
    let mut all = false;
    for (key, value) in parse_query_pairs(query) {
        match key.as_str() {
            "before" => {
                if before.is_some() {
                    return Err(EndpointError::invalid_message(
                        "duplicate before query parameter",
                    ));
                }
                let cursor = HistoryCursor::decode(&Cursor::new(value)).map_err(|detail| {
                    EndpointError::invalid_message(format!("malformed before cursor: {detail}"))
                        .with_cause("history.malformed_cursor")
                })?;
                before = Some(cursor);
            }
            "limit" => {
                if limit.is_some() {
                    return Err(EndpointError::invalid_message(
                        "duplicate limit query parameter",
                    ));
                }
                let parsed = value.parse::<u32>().map_err(|_| {
                    EndpointError::invalid_message("limit must be a positive decimal integer")
                })?;
                if parsed == 0 {
                    return Err(EndpointError::invalid_message(
                        "limit must be a positive decimal integer",
                    ));
                }
                limit = Some(parsed.min(MAX_HISTORY_LIMIT));
            }
            "all" => {
                if value != "1" {
                    return Err(EndpointError::invalid_message(
                        "all accepts only the value 1",
                    ));
                }
                all = true;
            }
            other => {
                return Err(EndpointError::invalid_message(format!(
                    "unknown query parameter {other:?}"
                )));
            }
        }
    }
    Ok(HistoryParams {
        before,
        limit: limit.unwrap_or(DEFAULT_HISTORY_LIMIT),
        all,
    })
}

/// 归属闸（与 branch_history_route 同一现役 can_access 语义）。
async fn gate_session(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
) -> Option<Response> {
    match state.sessions.get_for(principal, session_id).await {
        Ok(sessions::SessionAccess::Ok(_)) => None,
        Ok(sessions::SessionAccess::NotFound) => Some(EndpointError::not_found().into_response()),
        Ok(sessions::SessionAccess::Forbidden) => {
            Some(EndpointError::forbidden("cross_principal_access").into_response())
        }
        Err(err) => Some(EndpointError::storage(&err).into_response()),
    }
}

/// 一页（或全量）消息 → 投影 items：候选 run 集合 → 实存校验 → lineage
/// 闭包 → 页事件 → 跨页 user 锚探测 → 统一投影。
async fn project_items(
    db: &Arc<RunDatabase>,
    session_id: &str,
    messages: &[MessageRow],
) -> Result<Vec<HistoryItem>, StorageError> {
    // 候选 run：页内 id 约定（user: 前缀 / -final 后缀）+ run_id 列。
    let mut candidates: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut push = |run_id: &str| {
        if !run_id.is_empty() && seen.insert(run_id.to_string()) {
            candidates.push(run_id.to_string());
        }
    };
    for message in messages {
        if let Some(run_id) = message.message_id.strip_prefix("user:") {
            push(run_id);
        }
        if let Some(run_id) = message.message_id.strip_suffix("-final") {
            push(run_id);
        }
        if let Some(run_id) = &message.run_id {
            push(run_id);
        }
    }
    let existing_runs: HashSet<String> = cms::read_existing_run_ids(db, session_id, &candidates)
        .await?
        .into_iter()
        .collect();
    // lineage 闭包（store 内迭代；含子代理 run，其过程项并入最近页内锚定
    // 祖先组）。
    let lineage = cms::read_lineage_children(db, &candidates).await?;
    let mut all_runs = candidates;
    for (_, child) in &lineage {
        if seen.insert(child.clone()) {
            all_runs.push(child.clone());
        }
    }
    let events = cms::read_branch_events(db, session_id, &all_runs).await?;
    // 跨页 user 锚探测：分支上存在 `user:{run}` 的 run（决定 final 锚是否
    // 携带 run 组项——每 run 恰好出现一次）。
    let user_keys: Vec<String> = all_runs.iter().map(|run| format!("user:{run}")).collect();
    let user_anchors_on_branch: HashSet<String> =
        cms::read_existing_message_ids(db, session_id, &user_keys)
            .await?
            .into_iter()
            .filter_map(|id| id.strip_prefix("user:").map(str::to_string))
            .collect();
    Ok(project_history(&ProjectionInput {
        messages,
        events: &events,
        existing_runs: &existing_runs,
        lineage: &lineage,
        user_anchors_on_branch: &user_anchors_on_branch,
    }))
}

// ── 响应构造 ──

fn etag_of(head_revision: u64) -> String {
    format!("\"r{head_revision}\"")
}

fn if_none_match_matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(axum::http::header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            // If-None-Match 可以是逗号分隔列表；严格匹配当前 ETag。
            value.split(',').any(|candidate| candidate.trim() == etag)
        })
}

fn set_etag(response: &mut Response, etag: &str) {
    if let Ok(value) = HeaderValue::from_str(etag) {
        response
            .headers_mut()
            .insert(axum::http::header::ETAG, value);
    }
}

fn not_modified_response(etag: &str) -> Response {
    let mut response = StatusCode::NOT_MODIFIED.into_response();
    set_etag(&mut response, etag);
    response
}

fn page_response(
    session_id: String,
    head_revision: u64,
    items: Vec<HistoryItem>,
    page: HistoryPageMeta,
) -> Response {
    let body = HistoryPage {
        schema_version: CANONICAL_HISTORY_SCHEMA.to_string(),
        session_id,
        head_revision,
        items,
        page,
    };
    let mut response = (StatusCode::OK, Json(body)).into_response();
    set_etag(&mut response, &etag_of(head_revision));
    response
}

/// 游标引用不存在/seq 不吻合（InvalidRequest）→ 400；其余存储错误走
/// 现役 storage 映射（响亮 5xx，绝不静默降级）。
fn storage_error_response(err: &StorageError) -> Response {
    match err {
        StorageError::InvalidRequest { detail } => EndpointError::invalid_message(detail.clone())
            .with_cause("history.invalid_cursor")
            .into_response(),
        other => EndpointError::storage(other).into_response(),
    }
}
