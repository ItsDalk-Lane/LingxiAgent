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
//! - `reconciliation=1`（现役 sessions.ts:1405-1432 的端口映射）：严格证据
//!   等级读取——内容读取前后采样活动运行，页尾附 `reconciliation`
//!   证明块（runRevision / complete / runStatus：unknown|running|
//!   reconciled_idle），不参与条件 GET（现役 E01 304 资格同口径）。
//! - 每页附现役 `/sessions/messages` 契约的待办与修订投影：`todos` /
//!   `todoPanel`（与 todos 面板读同一存储快照）与 `revision`（与 ETag
//!   同源字符串）。
//! - ETag `"r{headRevision}-e{eventRevision}"`：无游标请求携带匹配 If-None-Match → 304；
//!   分支头或真实审计事件推进即失效。条件判定先于消息读取（索引定位）。
//! - 归属闸与会话读相同：未知会话 404、跨主体 403（先于一切参数后的
//!   实体读取；ETag 判定也在闸后，不向无权限者泄露存在性）。

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

// R06 C2 块（会话历史与内容读取缺失路由）的实现模块。模块声明挂在这里
// 而不是 lib.rs（沿用 agents_admin.rs 对 agent_config_readback.rs 的既有
// #[path] 模式）：lib.rs 的路由注册由主控统一收口，本模块的处理器为 pub。
#[path = "history_content.rs"]
pub mod history_content;

use lingxi_adapters::storage::canonical_message_store as cms;
use lingxi_adapters::storage::session_tree::MessageRow;
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::StorageError;
use lingxi_protocol::history::{
    HistoryCursor, HistoryExport, HistoryItem, HistoryPage, HistoryPageMeta,
    CANONICAL_HISTORY_SCHEMA,
};
use lingxi_protocol::Cursor;

use crate::history_projection::{project_history_with_audits, ProjectionInput};
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
    // 现役 E01 304 资格：all / reconciliation 分支不参与条件 GET——它们
    // 是严格证据等级读取（全量投影/协调证明），不是可复用缓存页。
    let head_revision = match cms::read_branch_head_revision(&db, &session_id).await {
        Ok(revision) => revision,
        Err(err) => return storage_error_response(&err),
    };
    let event_revision = match cms::read_history_event_revision(&db, &session_id).await {
        Ok(revision) => revision,
        Err(err) => return storage_error_response(&err),
    };
    let etag = etag_of(head_revision, event_revision);
    if params.before.is_none()
        && !params.all
        && !params.reconciliation
        && if_none_match_matches(&headers, &etag)
    {
        return not_modified_response(&etag);
    }
    // reconciliation=1 的前置运行快照（I12 严格证据等级：内容读取前采样
    // 活动运行；与读取后的二次采样对比得出 runStatus）。
    let runs_before = if params.reconciliation {
        match sample_active_runs(&state, &session_id).await {
            Ok(sample) => Some(sample),
            Err(err) => return err,
        }
    } else {
        None
    };
    if params.all {
        let (messages, head_revision) =
            match cms::read_full_branch(&db, &session_id, MAX_HISTORY_LIMIT).await {
                Ok(full) => full,
                Err(err) => return storage_error_response(&err),
            };
        let items = match project_items(&db, &session_id, &messages, true).await {
            Ok(items) => items,
            Err(err) => return storage_error_response(&err),
        };
        return history_page_response(
            &state,
            session_id,
            head_revision,
            items,
            &etag,
            HistoryPageMeta {
                // 全量入口：页元信息如实标记无后续页；limit 报告内部分页大小。
                limit: MAX_HISTORY_LIMIT,
                has_more: false,
                next_before: None,
            },
            runs_before.zip(Some(true)),
        )
        .await;
    }
    let page = match cms::read_branch_page(&db, &session_id, params.before, params.limit).await {
        Ok(page) => page,
        Err(err) => return storage_error_response(&err),
    };
    let head_revision = page.head_revision;
    let items = match project_items(&db, &session_id, &page.messages, !page.has_more).await {
        Ok(items) => items,
        Err(err) => return storage_error_response(&err),
    };
    let page_complete = !page.has_more;
    history_page_response(
        &state,
        session_id,
        head_revision,
        items,
        &etag,
        HistoryPageMeta {
            limit: params.limit,
            has_more: page.has_more,
            next_before: page.next_before.map(|cursor| cursor.encode()),
        },
        runs_before.zip(Some(page_complete)),
    )
    .await
}

/// reconciliation 采样的最小运行快照（内容读取前后各一次）。
struct RunSample {
    /// 该会话当前是否存在活动运行（queued/running/waiting_approval/
    /// cancelling —— 与 run 库活动行同口径）。
    active: bool,
    /// 采样时的修订点（分支头 revision）。
    revision: u64,
}

/// 读取该会话当前的活动运行 + 分支头修订（reconciliation 前后采样）。
#[allow(clippy::result_large_err)] // 同 gate_session 的响应直携纪律。
async fn sample_active_runs(state: &ServiceState, session_id: &str) -> Result<RunSample, Response> {
    let runs = state
        .storage()
        .list_active_runs()
        .await
        .map_err(|err| EndpointError::storage(&err).into_response())?;
    let active = runs.iter().any(|run| run.session_id == session_id);
    let revision = cms::read_branch_head_revision(state.storage(), session_id)
        .await
        .map_err(|err| EndpointError::storage(&err).into_response())?;
    Ok(RunSample { active, revision })
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
    let event_revision = match cms::read_history_event_revision(&db, &session_id).await {
        Ok(revision) => revision,
        Err(err) => return storage_error_response(&err),
    };
    let etag = etag_of(head_revision, event_revision);
    if if_none_match_matches(&headers, &etag) {
        return not_modified_response(&etag);
    }
    let (messages, head_revision) =
        match cms::read_full_branch(&db, &session_id, MAX_HISTORY_LIMIT).await {
            Ok(full) => full,
            Err(err) => return storage_error_response(&err),
        };
    let items = match project_items(&db, &session_id, &messages, true).await {
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
    set_etag(&mut response, &etag);
    response
}

// ── 内部流水线 ──

struct HistoryParams {
    before: Option<HistoryCursor>,
    limit: u32,
    all: bool,
    /// 现役 `reconciliation=1`（sessions.ts:1405）：严格证据等级读取——
    /// 前后采样活动运行并返回 reconciliation 证明块，不参与条件 GET。
    reconciliation: bool,
}

/// 严格查询解析：只认 before/limit/all/reconciliation，重复或未知键一律 400
/// （镜像 events 页的闭合查询纪律）。
fn parse_history_query(query: &str) -> Result<HistoryParams, EndpointError> {
    let mut before: Option<HistoryCursor> = None;
    let mut limit: Option<u32> = None;
    let mut all = false;
    let mut reconciliation = false;
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
            "reconciliation" => {
                if reconciliation {
                    return Err(EndpointError::invalid_message(
                        "duplicate reconciliation query parameter",
                    ));
                }
                if value != "1" {
                    return Err(EndpointError::invalid_message(
                        "reconciliation accepts only the value 1",
                    ));
                }
                reconciliation = true;
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
        reconciliation,
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
    reached_root: bool,
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
    // 根重置标记隔开旧空根；有锚的运行只在该锚所在页出现一次。
    let include_empty_root = reached_root
        && !messages.first().is_some_and(|m| {
            m.entry_type == "hana-session-branch-reset" && m.parent_message_id.is_none()
        });
    let audit_anchors =
        cms::read_unmessaged_terminal_anchors(db, session_id, messages, include_empty_root).await?;
    for (run_id, _) in &audit_anchors {
        push(run_id);
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
    Ok(project_history_with_audits(
        &ProjectionInput {
            messages,
            events: &events,
            existing_runs: &existing_runs,
            lineage: &lineage,
            user_anchors_on_branch: &user_anchors_on_branch,
        },
        &audit_anchors,
    ))
}

// ── 响应构造 ──

fn etag_of(head_revision: u64, event_revision: u64) -> String {
    format!("\"r{head_revision}-e{event_revision}\"")
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

/// 历史页响应构造：统一 `HistoryPage`（schema 化主体）+ 现役
/// `/sessions/messages` 契约的补充投影字段（R06-R2 原义补全）——
/// `revision`（修订点，与 ETag 同材料）、`todos` / `todoPanel`
/// （待办投影，与 todos 面板读同一存储快照）、以及 `reconciliation=1`
/// 时的协调证明块（runRevision / complete / runStatus）。
///
/// 这些字段是**附加键**：主体仍是 schema 化 HistoryPage（deny_unknown_fields
/// 只约束反序列化路径，不影响本响应的 JSON 附加字段）；值断言经
/// serde_json::Value 读取不受影响。
async fn history_page_response(
    state: &ServiceState,
    session_id: String,
    head_revision: u64,
    items: Vec<HistoryItem>,
    etag: &str,
    page: HistoryPageMeta,
    reconciliation: Option<(RunSample, bool)>,
) -> Response {
    let body = HistoryPage {
        schema_version: CANONICAL_HISTORY_SCHEMA.to_string(),
        session_id: session_id.clone(),
        head_revision,
        items,
        page,
    };
    let mut value = serde_json::to_value(&body).expect("HistoryPage serialization is infallible");
    let object = value
        .as_object_mut()
        .expect("HistoryPage serializes to an object");
    // 修订点（现役 revision：客户端缓存对比材料；这里 = ETag 同源字符串）。
    object.insert(
        "revision".into(),
        serde_json::Value::String(etag.to_string()),
    );
    // 待办投影（与 GET /sessions/todos 同一存储读取，读取失败不静默降级）。
    let todos_projection = match crate::todos::latest_todo_snapshot(state, &session_id).await {
        Ok(snapshot) => snapshot,
        Err(response) => return response,
    };
    let panel = crate::todos::panel_from_snapshot(todos_projection.as_ref());
    let todos = panel.get("todos").cloned().unwrap_or(serde_json::json!([]));
    object.insert("todoPanel".into(), panel);
    object.insert("todos".into(), todos);
    if let Some((before, page_complete)) = reconciliation {
        let after = match sample_active_runs(state, &session_id).await {
            Ok(sample) => sample,
            Err(response) => return response,
        };
        // 现役 runStatus 口径（sessions.ts:1427-1432 的端口映射）：
        // 页不完整或读取期间修订推进 → unknown（宁缺勿假）；任一采样见
        // 活动运行 → running；前后均 idle 且修订稳定 → reconciled_idle。
        let run_status = if !page_complete || before.revision != after.revision {
            "unknown"
        } else if before.active || after.active {
            "running"
        } else {
            "reconciled_idle"
        };
        object.insert(
            "reconciliation".into(),
            serde_json::json!({
                "sessionId": session_id,
                "runRevision": after.revision.to_string(),
                "complete": page_complete,
                "runStatus": run_status,
            }),
        );
    }
    let mut response = (StatusCode::OK, Json(value)).into_response();
    set_etag(&mut response, etag);
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
