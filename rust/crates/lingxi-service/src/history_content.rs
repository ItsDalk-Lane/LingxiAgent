//! R06 C2「会话历史与内容读取」缺失路由端口（GAP_DISPATCH_PLAN C2 块）。
//!
//! 逐叶语义源（端口式移植，不猜造）：
//! - `GET /lingxi/v1/sessions/{id}/find?q=` — 现役 `server/routes/sessions.ts`
//!   `/sessions/find`（:922-986）+ `lib/search/session-find.ts`：在指定会话
//!   历史内返回命中位置（displayable 序号）；内容经脱敏（模型侧信封剥离；
//!   Bridge 会话的时间标签剥离——见 [`sanitize_bridge_visible_text`]）。
//! - `POST /lingxi/v1/sessions/{id}/latest-user-message/replay` — 现役
//!   `sessions.ts:1515-1556` `/sessions/latest-user-message/replay`（→
//!   `core/session-turn-actions.ts replayLatestUserTurn`，latestUserOnly）：
//!   按版本（snapshotVersion 关联身份）与可选替换文字重放最近用户消息；
//!   运行中 409（现役 isSessionStreaming → session_busy；本端口由会话
//!   admission 持有权结构性保证——运行中的 run 持有会话 admission，
//!   retry 的 begin_command `try_acquire` 拒绝 → Busy → 409）。
//! - `GET /lingxi/v1/sessions/{id}/history-overview` — 现役
//!   `sessions.ts:1337-1381` + `server/history-read/index.ts
//!   readSessionHistoryOverview` + `overview.ts`：授权读概览；未就绪
//!   （revision 事实不可靠）返回 `available:false` + reason（200，非错误）。
//! - `GET /lingxi/v1/sessions/{id}/content/{contentId}` — 现役
//!   `sessions.ts:1260-1298` + `server/history-deferred-content.ts`：只在
//!   已授权会话历史内按 contentId 取大内容，找不到 404。端口适配：TS 的
//!   locator 是 base64url 消息坐标；Rust 历史的大内容以资源引用
//!   （`res_` id，HistoryItem 的 resource_refs / ContentBlock::ResourceRef）
//!   呈现，故 contentId = 资源 id，解析权 = 「该 id 必须出现在本会话的
//!   投影历史中」（locator 不是授权凭证——同一安全契约）。
//! - `GET /lingxi/v1/sessions/{id}/summary` — 现役 `sessions.ts:988-1011`
//!   `/sessions/summary`：经读取授权返回滚动摘要记录
//!   （`agents/{agentId}/memory/summaries/{sessionId}.json`，TS
//!   `lib/memory/session-summary.ts` 的落盘契约；summary.trim() 非空才算
//!   hasSummary）。
//! - `GET /lingxi/v1/sessions/{id}/conversation-map/turns` — 现役
//!   `server/routes/conversation-map.ts:172-209` + `foldMessagesIntoTurns`：
//!   读取会话消息并折叠成「一问一答」回合卡片。
//! - `POST /lingxi/v1/sessions/{id}/fresh-compact` — 现役
//!   `sessions.ts:2430-2452` + `core/engine.ts freshCompactDesktopSession`
//!   （:3145-3181）：压缩旧上下文（[`crate::compaction::CompactionService::
//!   compact_now`]，手动路径）后按当前 Agent 配置重建提示词工件
//!   （[`crate::context_compiler`] 现材料重编译）。压缩服务经 axum
//!   Extension 注入（lib.rs 组合根持有实例；本模块不新造）。
//! - `SessionCollabTools` — 现役 `lib/tools/session-tool.ts`（list/read/
//!   send/create 单工具多 action）：list/read 返回获授权会话；send/create
//!   产 TS 形状的确认草稿（`lib/session-collab/draft-store.ts`，一次性
//!   存储 + apply 投递 + create 半成功保留新会话 id）；投递端口
//!   [`SessionCollabDelivery`] 由宿主接线（见 trait 文档）。
//!
//! 显式挂起（C2 块内，无 Rust 宿主面，不在本模块伪造）：
//! - `/reset` 由 `crate::slash` 的统一派发器执行当前会话历史清除；
//!   Bridge 接管期间明确拒绝，不借用本模块归档/永久删除其他会话。
//! - `R00-T02-LA-FF4F06011B48`（读会话 sidecar 冻结系统提示词与工具名）：
//!   Rust 存储模型无会话级 promptSnapshot/toolNames sidecar（冻结面为
//!   per-run `model_context_snapshot`；GAP 明示不能用另一观察面替代），
//!   sidecar 写入面未移植前该读面无真实数据源。
//!
//! 已知端口偏差（显式标注，不静默）：
//! - find 的查询规范化：TS `normalizeSessionSearchText` 含 NFKC；Rust 锁定
//!   依赖树无 unicode-normalization，本端口做小写 + 空白折叠（全半角等价
//!   归一不可用）。TS 的 jieba 中文分词腿（`session-search-tokenizer.ts`）
//!   同样无 Rust 宿主：中文查询仍经整句归一匹配与空格分词命中，不分解词元。
//! - find/overview 的 displayRecords 口径：TS `isDisplayableHistoryMessage`
//!   按原始消息 content 判定（文本或内联图片）；Rust 投影项以非空文本为
//!   displayable（图片走资源引用面，不在消息文本内）。
//! - overview 的 taskDistribution：TS 按任务元数据 type 分类（media/
//!   workflow/subagent/other）；Rust 模型的权威任务事实只有 run_lineage
//!   子 run（subagent），workflow/media/other 无事实来源，如实报 0。
//! - 手动/fresh压缩经真实工具目录构建快照，并与刷新材料一起原子提交到模型档案。
//!
//! 路由注册由主控在 lib.rs 统一完成；测试在本模块配套测试文件内以
//! axum::Router 挂载（含鉴权 Extension 注入）。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lingxi_adapters::storage::canonical_message_store as cms;
use lingxi_adapters::storage::session_tree as tree_store;
use lingxi_adapters::storage::session_tree::MessageRow;
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::{StorageError, ToolExecutionResult, ToolExecutorPort, ToolOutcome};
use lingxi_kernel::RunContext;
use lingxi_protocol::history::{HistoryItem, RunAssociationState};
use lingxi_protocol::{ContentBlock, ErrorCode, ProtocolError, ToolCallId};
use serde_json::{json, Value};

use crate::cancel::CancelScope;
use crate::compaction::{CompactionOutcome, MidRunCompactionInput};
use crate::history_projection::{project_history_with_audits, ProjectionInput};
use crate::session_tree::{FileRollbackMode, SessionTreeError};
use crate::ws::parse_query_pairs;
use crate::{EndpointError, Principal, ServiceState};

/// 现役 find 查询长度上限（sessions.ts:75 `SESSION_SEARCH_QUERY_MAX_LENGTH`）。
pub const SESSION_FIND_QUERY_MAX_LENGTH: usize = 512;
/// 现役 find 命中列表上限（session-find.ts:7 `MAX_MATCHES`）。
const SESSION_FIND_MAX_MATCHES: usize = 500;
/// 现役概览推荐页大小（history-read/protocol.ts:37 `HISTORY_PROTOCOL_PAGE_LIMIT`）。
const HISTORY_PROTOCOL_PAGE_LIMIT: u64 = 100;
/// 现役概览分页常量（protocol.ts HistoryOverview.pagination）。
const HISTORY_LEGACY_DEFAULT_LIMIT: u64 = 50;
const HISTORY_MAX_LIMIT: u64 = 200;
/// 现役会话地图回合文本截断上限（conversation-map.ts:27）。
const TURN_TEXT_LIMIT: usize = 8000;

// ═══════════════ 共用：全分支投影（history.rs::project_items 的同构副本） ═══════════════
// history.rs 的投影流水线是私有函数；本模块按同一规则重建（候选 run 集合 →
// 实存校验 → lineage 闭包 → 页事件 → 跨页 user 锚探测 → 统一投影），保证
// find/overview/content/turns 与 /history 读到同一持久事实的同一投影。

struct ProjectedBranch {
    messages: Vec<MessageRow>,
    head_revision: u64,
    items: Vec<HistoryItem>,
}

async fn project_full_branch(
    db: &Arc<RunDatabase>,
    session_id: &str,
) -> Result<ProjectedBranch, StorageError> {
    let (messages, head_revision) =
        cms::read_full_branch(db, session_id, crate::history::MAX_HISTORY_LIMIT).await?;
    let mut candidates: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for message in &messages {
        for run_id in message
            .message_id
            .strip_prefix("user:")
            .into_iter()
            .chain(message.message_id.strip_suffix("-final"))
            .chain(message.run_id.iter().map(String::as_str))
        {
            if !run_id.is_empty() && seen.insert(run_id.to_string()) {
                candidates.push(run_id.to_string());
            }
        }
    }
    let include_empty_root = !messages.first().is_some_and(|m| {
        m.entry_type == "hana-session-branch-reset" && m.parent_message_id.is_none()
    });
    let audit_anchors =
        cms::read_unmessaged_terminal_anchors(db, session_id, &messages, include_empty_root)
            .await?;
    for (run_id, _) in &audit_anchors {
        if seen.insert(run_id.clone()) {
            candidates.push(run_id.clone());
        }
    }
    let existing_runs: HashSet<String> = cms::read_existing_run_ids(db, session_id, &candidates)
        .await?
        .into_iter()
        .collect();
    let lineage = cms::read_lineage_children(db, &candidates).await?;
    let mut all_runs = candidates;
    for (_, child) in &lineage {
        if seen.insert(child.clone()) {
            all_runs.push(child.clone());
        }
    }
    let events = cms::read_branch_events(db, session_id, &all_runs).await?;
    let user_keys: Vec<String> = all_runs.iter().map(|run| format!("user:{run}")).collect();
    let user_anchors_on_branch: HashSet<String> =
        cms::read_existing_message_ids(db, session_id, &user_keys)
            .await?
            .into_iter()
            .filter_map(|id| id.strip_prefix("user:").map(str::to_string))
            .collect();
    let items = project_history_with_audits(
        &ProjectionInput {
            messages: &messages,
            events: &events,
            existing_runs: &existing_runs,
            lineage: &lineage,
            user_anchors_on_branch: &user_anchors_on_branch,
        },
        &audit_anchors,
    );
    Ok(ProjectedBranch {
        messages,
        head_revision,
        items,
    })
}

// ═══════════════ 归属与生命周期闸（口径同 lib.rs 既有路由） ═══════════════

/// 会话归属闸：未知 404 / 跨主体 403（先于一切实体读取，不泄露存在性）。
/// Err 载荷是即用响应（axum 处理器惯用形），尺寸告警按仓库先例放行。
#[allow(clippy::result_large_err)]
async fn gate_owned(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
) -> Result<crate::sessions::SessionFacts, Response> {
    match state.sessions.get_for(principal, session_id).await {
        Ok(crate::sessions::SessionAccess::Ok(facts)) => Ok(facts),
        Ok(crate::sessions::SessionAccess::NotFound) => {
            Err(EndpointError::not_found().into_response())
        }
        Ok(crate::sessions::SessionAccess::Forbidden) => {
            Err(EndpointError::forbidden("cross_principal_access").into_response())
        }
        Err(err) => Err(EndpointError::storage(&err).into_response()),
    }
}

/// 运行写路径的生命周期闸（现役 assertManifestLifecycle "active" → 409；
/// lib.rs gate_session_tree_write 同口径——EndpointError::new 私有，经
/// resource_failure 生成同 status/reason 形状）。
#[allow(clippy::result_large_err)]
async fn gate_active(
    state: &ServiceState,
    session_id: &str,
) -> Result<tree_store::SessionTreeRow, Response> {
    match tree_store::get_session_row(state.storage(), session_id).await {
        Ok(Some(row)) if row.lifecycle == "active" => Ok(row),
        Ok(Some(row)) => Err(EndpointError::resource_failure(
            StatusCode::CONFLICT,
            "session_not_active",
            format!(
                "session lifecycle is {}, this operation requires active",
                row.lifecycle
            ),
        )
        .into_response()),
        Ok(None) => Err(EndpointError::not_found().into_response()),
        Err(err) => Err(EndpointError::storage(&err).into_response()),
    }
}

/// SessionTreeError → HTTP（lib.rs session_tree_error_response 的同构映射；
/// 该函数私有，本模块按同一 status/reason 表重建）。
fn session_tree_error(err: SessionTreeError) -> Response {
    match err {
        SessionTreeError::Busy => EndpointError::session_busy().into_response(),
        SessionTreeError::NotFound => EndpointError::not_found().into_response(),
        SessionTreeError::Forbidden => {
            EndpointError::forbidden("cross_principal_access").into_response()
        }
        SessionTreeError::InvalidTarget { detail } => {
            EndpointError::invalid_message(format!("invalid replay target: {detail}"))
                .with_cause("session.turn_target_invalid")
                .into_response()
        }
        SessionTreeError::FileRollbackDisabled => {
            EndpointError::forbidden("file_rollback_disabled").into_response()
        }
        other => EndpointError::storage(&StorageError::Internal {
            detail: format!("{other:?}"),
        })
        .into_response(),
    }
}

fn storage_error_response(err: &StorageError) -> Response {
    EndpointError::storage(err).into_response()
}

// ═══════════════ 4F1F69CCA760：会话内查找 ═══════════════

/// `GET /lingxi/v1/sessions/{session_id}/find?q=` — 指定会话内查找
/// （现役 `/sessions/find`；身份经路径段 session id 解析——Rust 模型的
/// 唯一定位符，等价于现役 sessionId→manifest / path 双入口）。
pub async fn session_find_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let mut q: Option<String> = None;
    for (key, value) in parse_query_pairs(query.as_deref().unwrap_or("")) {
        match key.as_str() {
            "q" => {
                if q.replace(value).is_some() {
                    return EndpointError::invalid_message("duplicate q query parameter")
                        .into_response();
                }
            }
            other => {
                return EndpointError::invalid_message(format!("unknown query key {other:?}"))
                    .into_response();
            }
        }
    }
    if let Err(resp) = gate_owned(&state, &principal, &session_id).await {
        return resp;
    }
    let query = q.unwrap_or_default().trim().to_string();
    if query.is_empty() {
        // 现役 :947-949：空查询返回空结果（非错误）。
        return Json(json!({
            "query": query,
            "revision": null,
            "total": 0,
            "bestIndex": null,
            "tokens": [],
            "matches": [],
            "truncated": false,
        }))
        .into_response();
    }
    if query.chars().count() > SESSION_FIND_QUERY_MAX_LENGTH {
        // 现役 :950-952：超长查询 400 + maxLength。
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "query_too_long",
                "maxLength": SESSION_FIND_QUERY_MAX_LENGTH,
            })),
        )
            .into_response();
    }
    let db = Arc::clone(state.storage());
    let projected = match project_full_branch(&db, &session_id).await {
        Ok(projected) => projected,
        Err(err) => return storage_error_response(&err),
    };
    let entries = collect_findable_entries(&projected.items);
    let result = find_in_session_messages(&entries, &query);
    Json(json!({
        "query": query,
        "revision": projected.head_revision.to_string(),
        "total": result.total,
        "bestIndex": result.best_index,
        "tokens": result.tokens,
        "matches": result.matches.iter().map(|m| json!({
            "index": m.index,
            "exact": m.exact,
            "snippet": m.snippet,
        })).collect::<Vec<_>>(),
        "truncated": result.truncated,
    }))
    .into_response()
}

/// 可查找条目（sessions.ts:271-296 collectFindableHistoryEntries 的投影项
/// 端口）：user/assistant 类 displayable 项推进 displayIdx；隐藏回合输入
/// 只推进序号不参与命中；文本经模型侧信封剥离。
pub struct FindEntry {
    pub index: u64,
    pub text: String,
}

pub fn collect_findable_entries(items: &[HistoryItem]) -> Vec<FindEntry> {
    let mut entries = Vec::new();
    let mut display_idx: u64 = 0;
    for item in items {
        match item {
            HistoryItem::UserMessage { text, .. } => {
                let current = display_idx;
                display_idx += 1;
                let content = strip_model_envelope_blocks(text);
                if is_hidden_turn_input_text(&content) {
                    continue;
                }
                let visible = strip_steer_prefix(&content);
                if !visible.trim().is_empty() {
                    entries.push(FindEntry {
                        index: current,
                        text: visible,
                    });
                }
            }
            HistoryItem::FinalMessage { content, .. } => {
                let current = display_idx;
                display_idx += 1;
                // stripThink:true 的等价物：投影已把 think 归一为独立
                // Reasoning 块，可见文本 = Text 块拼接。
                let raw = content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                let content = strip_model_envelope_blocks(&raw);
                if !content.trim().is_empty() {
                    entries.push(FindEntry {
                        index: current,
                        text: content,
                    });
                }
            }
            _ => {}
        }
    }
    entries
}

/// 隐藏回合输入（turn-input-presentation.ts HIDDEN_TURN_INPUT_RE）：
/// 后台结果 / 延迟任务 / 中断回执触发的系统回合输入，前端不渲染为用户
/// 节点——序号照常推进但不参与命中。
fn is_hidden_turn_input_text(content: &str) -> bool {
    content.contains("<hana-background-result")
        || content.contains("<hana-deferred-tasks")
        || content.contains("<hana-turn-interrupted")
}

/// 现役 find 的 user 文本前缀剥离（sessions.ts:261-262）：
/// 遗留插话前缀 + 回合时间标签前缀。
fn strip_steer_prefix(content: &str) -> String {
    let mut text = content;
    for prefix in ["（插话，无需 MOOD）\n", "(Interjection, no MOOD needed)\n"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest;
            break;
        }
    }
    // ^<t>[^<]*</t>\s*
    if let Some(rest) = text.strip_prefix("<t>") {
        if let Some(end) = rest.find("</t>") {
            let after = &rest[end + "</t>".len()..];
            return after.trim_start().to_string();
        }
    }
    text.to_string()
}

/// 模型侧信封剥离（session-reminders.ts stripSessionReminderBlocks 的
/// 行级端口）：[hana_reminder…]/[hana_reference]/[KnowledgeContext]/
/// [KnowledgeResearchContext] 块整段移除（未闭合块按 fail-closed 丢弃到
/// 文末），块后首个空行随块丢弃，尾部空行剪除。
pub fn strip_model_envelope_blocks(value: &str) -> String {
    let mut visible: Vec<&str> = Vec::new();
    let mut inside_reminder = false;
    let mut inside_reference = false;
    let mut knowledge_end: Option<&str> = None;
    let mut drop_separator = false;
    for line in value.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if inside_reference {
            if line == "[/hana_reference]" {
                inside_reference = false;
                drop_separator = true;
            }
            continue;
        }
        if inside_reminder {
            if line == "[/hana_reminder]" {
                inside_reminder = false;
                drop_separator = true;
            }
            continue;
        }
        if let Some(end) = knowledge_end {
            if line == end {
                knowledge_end = None;
                drop_separator = true;
            }
            continue;
        }
        if line == "[hana_reference]" {
            inside_reference = true;
            continue;
        }
        if line == "[KnowledgeContext]" {
            knowledge_end = Some("[/KnowledgeContext]");
            continue;
        }
        if line == "[KnowledgeResearchContext]" {
            knowledge_end = Some("[/KnowledgeResearchContext]");
            continue;
        }
        if is_reminder_header_line(line) {
            inside_reminder = true;
            continue;
        }
        if drop_separator && line.is_empty() {
            drop_separator = false;
            continue;
        }
        drop_separator = false;
        visible.push(line);
    }
    while visible.last().is_some_and(|line| line.is_empty()) {
        visible.pop();
    }
    visible.join("\n")
}

/// ^\[hana_reminder(?: at \d{4}-\d{2}-\d{2} \d{2}:\d{2})?\]$（session-reminders.ts:90）。
fn is_reminder_header_line(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("[hana_reminder") else {
        return false;
    };
    let Some(rest) = rest.strip_suffix(']') else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    let Some(tail) = rest.strip_prefix(" at ") else {
        return false;
    };
    // YYYY-MM-DD HH:MM
    let bytes = tail.as_bytes();
    if bytes.len() != 16 {
        return false;
    }
    bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b' '
        && bytes[11..13].iter().all(u8::is_ascii_digit)
        && bytes[13] == b':'
        && bytes[14..16].iter().all(u8::is_ascii_digit)
}

/// Bridge 可见文本脱敏（shared/bridge-visible-text.ts）：`<t> M-D HH:MM </t>`
/// 时间标签整段移除 + 行首空白剪除。现役只在 bridge 会话路径生效；Rust
/// 会话模型无 bridge 路径概念（无路径形态存储），路由层不可达——保留
/// 纯函数与单元证明，等 bridge 会话宿主落地后接线。
pub fn sanitize_bridge_visible_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("<t>") {
        let end_marker = "</t>";
        let after_open = &rest[start + 3..];
        let Some(close) = after_open.find(end_marker) else {
            break;
        };
        let tag = &after_open[..close];
        out.push_str(&rest[..start]);
        if !looks_like_bridge_time_tag(tag) {
            out.push_str(&rest[start..start + 3 + close + end_marker.len()]);
        }
        rest = &rest[start + 3 + close + end_marker.len()..];
    }
    out.push_str(rest);
    out.trim_start_matches([' ', '\t']).to_string()
}

/// `\d{1,2}-\d{1,2}\s+\d{1,2}:\d{2}`（BRIDGE_TIME_TAG_RE 的标签体；
/// TS 正则 `<t>\s*…\s*</t>` 允许标签体内首尾空白——先 trim 再匹配）。
fn looks_like_bridge_time_tag(tag: &str) -> bool {
    let tag = tag.trim();
    let mut chars = tag.chars().peekable();
    let mut digits = 0usize;
    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
        chars.next();
        digits += 1;
    }
    if !(1..=2).contains(&digits) || chars.next() != Some('-') {
        return false;
    }
    let mut digits = 0usize;
    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
        chars.next();
        digits += 1;
    }
    if !(1..=2).contains(&digits) {
        return false;
    }
    let mut saw_space = false;
    while chars.peek().is_some_and(|c: &char| c.is_whitespace()) {
        chars.next();
        saw_space = true;
    }
    if !saw_space {
        return false;
    }
    let mut digits = 0usize;
    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
        chars.next();
        digits += 1;
    }
    if !(1..=2).contains(&digits) || chars.next() != Some(':') {
        return false;
    }
    let mut minutes = 0usize;
    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
        chars.next();
        minutes += 1;
    }
    minutes == 2
}

/// 查询/文本归一（session-search-tokenizer.ts normalizeSessionSearchText；
/// NFKC 腿不可用——见模块偏差说明）。
fn normalize_search_text(value: &str) -> String {
    value
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 词元化（tokenizeSessionSearchQuery 的无 jieba 端口）：整句 + 空格分词
/// + ASCII 词；单 Han 字与纯标点丢弃（addToken 规则）。
fn tokenize_query(query: &str) -> Vec<String> {
    let normalized = normalize_search_text(query);
    if normalized.is_empty() {
        return Vec::new();
    }
    let mut terms: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let add = |terms: &mut Vec<String>, seen: &mut HashSet<String>, token: &str| {
        let value = normalize_search_text(token);
        if value.is_empty()
            || !value
                .chars()
                .any(|c| c.is_alphanumeric() && !c.is_whitespace())
        {
            return;
        }
        let single_han = value.chars().count() == 1 && value.chars().all(is_han);
        if single_han {
            return;
        }
        if seen.insert(value.clone()) {
            terms.push(value);
        }
    };
    add(&mut terms, &mut seen, &normalized);
    for term in normalized.split_whitespace() {
        add(&mut terms, &mut seen, term);
    }
    for word in ascii_words(&normalized) {
        add(&mut terms, &mut seen, &word);
    }
    terms
}

fn is_han(c: char) -> bool {
    // \p{Script=Han} 的字符区间近似（CJK 统一表意 + 扩展A + 兼容）。
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

/// [a-z0-9_][a-z0-9_.-]* （ASCII_WORD_RE，已归一小写）。
fn ascii_words(normalized: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for c in normalized.chars() {
        let continues = !current.is_empty()
            && (c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'));
        let starts =
            current.is_empty() && (c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if continues || starts {
            current.push(c);
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// isSearchableToken（session-search.ts:94-99）。
fn is_searchable_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let chars: Vec<char> = token.chars().collect();
    if chars.len() >= 2 && chars.iter().any(|c| is_han(*c)) {
        return true;
    }
    let ascii_shape = chars
        .iter()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
        && matches!(chars[0], 'a'..='z' | '0'..='9' | '_');
    if ascii_shape {
        return chars.len() >= 2;
    }
    chars.len() >= 3
}

/// buildSnippet（session-search.ts:126-140）。
fn build_snippet(text: &str, needle: &str) -> String {
    let raw: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if raw.is_empty() {
        return String::new();
    }
    let normalized_raw = normalize_search_text(&raw);
    let Some(index) = normalized_raw.find(needle) else {
        return raw.chars().take(120).collect();
    };
    // 归一坐标 ≈ 原文坐标（本端口归一不改变词内字符；近似切片）。
    let char_index = normalized_raw[..index].chars().count();
    let needle_len = needle.chars().count();
    let start = char_index.saturating_sub(36);
    let end = (char_index + needle_len + 72).min(raw.chars().count());
    let raw_chars: Vec<char> = raw.chars().collect();
    let slice: String = raw_chars[start..end].iter().collect();
    let prefix = if start > 0 { "..." } else { "" };
    let suffix = if end < raw_chars.len() { "..." } else { "" };
    format!("{prefix}{slice}{suffix}")
}

/// findInSessionMessages（session-find.ts:38-92）的端口：保序不排序，
/// exact 优先于 token 命中，bestIndex 按 (exact, score) 取最优。
pub struct SessionFindResult {
    pub total: usize,
    pub best_index: Option<u64>,
    pub tokens: Vec<String>,
    pub matches: Vec<SessionFindMatch>,
    pub truncated: bool,
}

pub struct SessionFindMatch {
    pub index: u64,
    pub exact: bool,
    pub snippet: String,
}

pub fn find_in_session_messages(entries: &[FindEntry], query: &str) -> SessionFindResult {
    let normalized_query = normalize_search_text(query);
    if normalized_query.is_empty() {
        return SessionFindResult {
            total: 0,
            best_index: None,
            tokens: Vec::new(),
            matches: Vec::new(),
            truncated: false,
        };
    }
    let tokens: Vec<String> = tokenize_query(&normalized_query)
        .into_iter()
        .filter(|token| token != &normalized_query)
        .filter(|token| is_searchable_token(token))
        .collect();
    let mut matches = Vec::new();
    let mut total = 0usize;
    let mut best_index: Option<u64> = None;
    let mut best_exact = false;
    let mut best_score: i64 = -1;
    for entry in entries {
        let normalized = normalize_search_text(&entry.text);
        if normalized.is_empty() {
            continue;
        }
        let exact = normalized.contains(&normalized_query);
        let mut score: i64 = 0;
        let mut matched_needle: Option<String> = None;
        if exact {
            score = 1000 + (normalized_query.chars().count() as i64 * 8).min(200);
            matched_needle = Some(normalized_query.clone());
        } else {
            for token in &tokens {
                if !normalized.contains(token.as_str()) {
                    continue;
                }
                if matched_needle.is_none() {
                    matched_needle = Some(token.clone());
                }
                score += 80 + (token.chars().count() as i64 * 8).min(60);
            }
        }
        if score <= 0 {
            continue;
        }
        let Some(needle) = matched_needle else {
            continue;
        };
        total += 1;
        if matches.len() < SESSION_FIND_MAX_MATCHES {
            matches.push(SessionFindMatch {
                index: entry.index,
                exact,
                snippet: build_snippet(&entry.text, &needle),
            });
        }
        let better = (exact && !best_exact) || (exact == best_exact && score > best_score);
        if better {
            best_exact = exact;
            best_score = score;
            best_index = Some(entry.index);
        }
    }
    let truncated = total > matches.len();
    SessionFindResult {
        total,
        best_index,
        tokens,
        matches,
        truncated,
    }
}

// ═══════════════ 4DD3F71FA2CD：最近用户消息重放 ═══════════════

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplayRequestBody {
    /// 必须是当前分支上最近一条用户消息（现役 sourceEntryId 契约：
    /// 不匹配 → 明确错误，不回退重放别的消息）。
    #[serde(default)]
    source_message_id: Option<String>,
    /// 客户端快照版本（desktop-session-submit.ts:123：undefined → 1；
    /// 有效域 = ≥1 的整数——输入关联身份，随响应回执）。
    #[serde(default)]
    snapshot_version: Option<u64>,
    /// 可选替换文字（现役 text → replacementText）；出现但空白 → 400。
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
}

/// `POST /lingxi/v1/sessions/{session_id}/latest-user-message/replay` —
/// 按版本与可选替换文字重放最近用户消息（现役 sessions.ts:1515-1556）。
/// 重置点复用既有 retry 机器（latestUserOnly = target 缺省解析最近 user
/// 回合）；运行中的会话持有 admission → Busy → 409。
pub async fn latest_user_message_replay_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: Result<Json<ReplayRequestBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(json) => json,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    // 现役 session-turn-actions.ts:367-369：替换文字出现但空白 = 明确错误。
    if req
        .text
        .as_deref()
        .is_some_and(|text| text.trim().is_empty())
    {
        return EndpointError::invalid_message("replacement text is required").into_response();
    }
    let snapshot_version = match req.snapshot_version {
        None => 1,
        Some(version) if version >= 1 => version,
        Some(_) => {
            return EndpointError::invalid_message("snapshotVersion must be an integer >= 1")
                .into_response();
        }
    };
    if let Err(resp) = gate_owned(&state, &principal, &session_id).await {
        return resp;
    }
    if let Err(resp) = gate_active(&state, &session_id).await {
        return resp;
    }
    // 最近用户回合解析（latestUserOnly）：与 retry_inner 同一规则。
    let db = Arc::clone(state.storage());
    let rows = match tree_store::list_branch_messages(&db, &session_id).await {
        Ok(rows) => rows,
        Err(err) => return storage_error_response(&err),
    };
    let nodes: Vec<lingxi_kernel::session_tree::BranchNode<'_>> = rows
        .iter()
        .filter(|m| m.entry_type == "message")
        .map(|m| lingxi_kernel::session_tree::BranchNode {
            message_id: m.message_id.as_str(),
            parent_message_id: m.parent_message_id.as_deref(),
            role: m.role.as_str(),
        })
        .collect();
    let point = match lingxi_kernel::session_tree::resolve_retry_reset_point(&nodes, None) {
        Ok(point) => point,
        Err(err) => return session_tree_error(SessionTreeError::from(err)),
    };
    if let Some(source) = req.source_message_id.as_deref() {
        if source != point.turn_input_message_id {
            // 现役 :402-404：请求的消息不是最近用户消息 → 明确错误。
            return EndpointError::invalid_message(
                "requested message is not the latest user message",
            )
            .with_cause("session.replay_not_latest")
            .into_response();
        }
    }
    // busy 闸由 retry 的 begin_command admission 承担（运行中的 run 持有
    // 会话 admission → Busy → 409 session_busy）。
    match state
        .session_tree()
        .retry_turn(
            &principal,
            &session_id,
            None,
            FileRollbackMode::None,
            false,
            req.request_id.as_deref(),
            state.clock().now_unix_ms(),
        )
        .await
    {
        Ok(mut outcome) => {
            if let Some(replacement) = req.text.as_deref() {
                match serde_json::from_str::<serde_json::Map<String, Value>>(
                    &outcome.turn_input_content_json,
                ) {
                    Ok(mut content) => {
                        content.insert("text".to_string(), Value::String(replacement.to_string()));
                        if let Ok(serialized) = serde_json::to_string(&content) {
                            outcome.turn_input_content_json = serialized;
                        }
                    }
                    Err(_) => {
                        // 非对象体不可替换：显式失败，不静默返回原文。
                        return EndpointError::invalid_message(
                            "the latest turn input content is not a replaceable text object",
                        )
                        .with_cause("session.replay_content_unreplaceable")
                        .into_response();
                    }
                }
            }
            Json(json!({
                "ok": true,
                "sessionId": outcome.session_id,
                "turnInputMessageId": outcome.turn_input_message_id,
                "turnInputContentJson": outcome.turn_input_content_json,
                "newHeadMessageId": outcome.new_head_message_id,
                "resetMarkerId": outcome.reset_marker_id,
                "snapshotVersion": snapshot_version,
            }))
            .into_response()
        }
        Err(err) => session_tree_error(err),
    }
}

// ═══════════════ CDBFE6616EB9：会话历史概览 ═══════════════

/// `GET /lingxi/v1/sessions/{session_id}/history-overview` — 授权读概览；
/// 未就绪 200 + available:false（现役 sessions.ts:1337-1381 /
/// history-read/overview.ts 计数规则）。revision 事实不可靠（分支无持久
/// 头行、仅能按 seq 兜底推断头部的 legacy 区）→ revision_unknown。
pub async fn session_history_overview_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
) -> Response {
    if let Err(resp) = gate_owned(&state, &principal, &session_id).await {
        return resp;
    }
    let db = Arc::clone(state.storage());
    // revision 事实：持久分支头行（TS 的 stat 签名等价物）。无头行且分支
    // 上确有消息 → 头部只能按 seq 兜底推断（legacy 区）→ revision_unknown。
    let head_row = match db
        .query_one_text(
            "SELECT revision FROM session_branch_heads WHERE session_id=?1",
            vec![session_id.clone()],
        )
        .await
    {
        Ok(value) => value,
        Err(err) => return storage_error_response(&err),
    };
    let projected = match project_full_branch(&db, &session_id).await {
        Ok(projected) => projected,
        Err(err) => return storage_error_response(&err),
    };
    let unavailable = |reason: &str| {
        overview_headers_response(json!({
            "schemaVersion": 1,
            "available": false,
            "reason": reason,
        }))
    };
    let head_revision = head_row.and_then(|text| text.parse::<u64>().ok());
    let revision = match head_revision {
        Some(_) => projected.head_revision,
        None if projected.messages.is_empty() => {
            // 空会话：空是已知事实（非未知）→ available:true + 零计数
            // （现役空 session 文件 stat 成功 → 目录 0 记录 → available:true）。
            0
        }
        None => return unavailable("revision_unknown"),
    };
    // referencedTasks：权威任务事实 = 分支 run 的 lineage 子 run（subagent）。
    let branch_runs = branch_run_ids(&projected.messages);
    let referenced_tasks = match cms::read_lineage_children(&db, &branch_runs).await {
        Ok(lineage) => {
            let parents: HashSet<&str> = branch_runs.iter().map(String::as_str).collect();
            lineage
                .into_iter()
                .filter(|(parent, _)| parents.contains(parent.as_str()))
                .map(|(_, child)| child)
                .collect::<HashSet<_>>()
        }
        Err(err) => return storage_error_response(&err),
    };
    let subagent_count = referenced_tasks.len();
    overview_headers_response(overview_body(
        &session_id,
        revision,
        &projected,
        subagent_count,
    ))
}

/// 分支消息行内的 run 身份（user: 前缀 / -final 后缀 / run_id 列）。
fn branch_run_ids(messages: &[MessageRow]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for message in messages {
        for run_id in message
            .message_id
            .strip_prefix("user:")
            .into_iter()
            .chain(message.message_id.strip_suffix("-final"))
            .chain(message.run_id.iter().map(String::as_str))
        {
            if !run_id.is_empty() && seen.insert(run_id.to_string()) {
                out.push(run_id.to_string());
            }
        }
    }
    out
}

/// 现役协议响应头（sessions.ts:1369-1373：无 ETag——条件快路径仅限普通
/// 消息页；概览私有 no-store）。
fn overview_headers_response(body: Value) -> Response {
    (
        StatusCode::OK,
        [
            ("lingxi-history-protocol", "1"),
            ("cache-control", "private, no-store"),
        ],
        Json(body),
    )
        .into_response()
}

fn overview_body(
    session_id: &str,
    revision: u64,
    projected: &ProjectedBranch,
    subagent_count: usize,
) -> Value {
    // displayRecords：user/assistant 类 displayable 项计数（含被前端隐藏
    // 但推进 display 序号的隐藏回合输入——TS 口径）。
    let mut display_records: u64 = 0;
    for item in &projected.items {
        match item {
            HistoryItem::UserMessage { text, .. } => {
                if !text.trim().is_empty() {
                    display_records += 1;
                }
            }
            HistoryItem::FinalMessage { content, .. } if !content.is_empty() => {
                display_records += 1;
            }
            _ => {}
        }
    }
    // runSizeDistribution：每 run 的 assistant display 记录数分桶
    //（overview.ts bucketOf：0 不入桶）；unknown 关联（fork 副本等）按
    // 消息单独成桶（无 run 身份可归属）。
    let mut run_sizes: HashMap<String, u64> = HashMap::new();
    for (ordinal, item) in projected.items.iter().enumerate() {
        if let HistoryItem::FinalMessage { run, content, .. } = item {
            if content.is_empty() {
                continue;
            }
            let key = match (&run.state, &run.run_id) {
                (RunAssociationState::Known, Some(run_id)) => run_id.clone(),
                _ => format!("unknown:{ordinal}"),
            };
            *run_sizes.entry(key).or_insert(0) += 1;
        }
    }
    let bucket_of = |count: u64| -> &'static str {
        if count <= 50 {
            "oneTo50"
        } else if count <= 200 {
            "from51To200"
        } else {
            "over200"
        }
    };
    let mut bucket_counts: HashMap<&'static str, u64> = HashMap::new();
    for size in run_sizes.values() {
        *bucket_counts.entry(bucket_of(*size)).or_insert(0) += 1;
    }
    json!({
        "schemaVersion": 1,
        "available": true,
        "sessionId": session_id,
        "revision": revision.to_string(),
        "counts": {
            "displayRecords": display_records,
            "sourceRecords": projected.messages.len(),
            "runsWithAssistant": run_sizes.len(),
            "referencedTasks": subagent_count,
        },
        "runSizeDistribution": {
            "oneTo50": bucket_counts.get("oneTo50").copied().unwrap_or(0),
            "from51To200": bucket_counts.get("from51To200").copied().unwrap_or(0),
            "over200": bucket_counts.get("over200").copied().unwrap_or(0),
        },
        "taskDistribution": {
            "subagent": subagent_count,
            "workflow": 0,
            "media": 0,
            "other": 0,
        },
        "pagination": {
            "legacyDefaultLimit": HISTORY_LEGACY_DEFAULT_LIMIT,
            "recommendedLimit": HISTORY_PROTOCOL_PAGE_LIMIT,
            "maxLimit": HISTORY_MAX_LIMIT,
            "estimatedPagesAtRecommendedLimit": if display_records > 0 {
                display_records.div_ceil(HISTORY_PROTOCOL_PAGE_LIMIT)
            } else {
                0
            },
        },
    })
}

// ═══════════════ ED089133C106：历史延迟内容（contentId） ═══════════════

/// `GET /lingxi/v1/sessions/{session_id}/content/{content_id}` — 只在已
/// 授权会话历史内按 contentId 取大内容（现役 sessions.ts:1260-1298）。
/// 端口适配：contentId = 资源 id（`res_`），解析权 = 该 id 出现在本会话
/// 投影历史的资源引用中（ToolCall result resource_refs / FinalMessage
/// ResourceRef 块）；不出现或资源不存在 → 404（TS "Historical content
/// not found"）。locator 从不是授权凭证。
pub async fn session_content_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path((session_id, content_id)): Path<(String, String)>,
) -> Response {
    if let Err(resp) = gate_owned(&state, &principal, &session_id).await {
        return resp;
    }
    let db = Arc::clone(state.storage());
    let projected = match project_full_branch(&db, &session_id).await {
        Ok(projected) => projected,
        Err(err) => return storage_error_response(&err),
    };
    let referenced = projected.items.iter().any(|item| match item {
        HistoryItem::ToolCall {
            result: Some(result),
            ..
        } => result
            .resource_refs
            .iter()
            .any(|r#ref| r#ref.resource_id.as_str() == content_id),
        HistoryItem::FinalMessage { content, .. } => content.iter().any(|block| match block {
            ContentBlock::ResourceRef { resource } => resource.resource_id.as_str() == content_id,
            _ => false,
        }),
        _ => false,
    });
    if !referenced {
        return EndpointError::not_found().into_response();
    }
    let resolved = match state.resources().resolve_content(&content_id).await {
        Ok(content) => content,
        Err(_) => return EndpointError::not_found().into_response(),
    };
    // 读取方式与现役资源内容路由一致（resources.rs std::fs::read）。
    let bytes = match std::fs::read(&resolved.file_path) {
        Ok(bytes) => bytes,
        Err(_) => return EndpointError::not_found().into_response(),
    };
    let (content, mime_type) = match String::from_utf8(bytes) {
        Ok(text) => (text, None),
        Err(err) => {
            use base64::Engine as _;
            (
                base64::engine::general_purpose::STANDARD.encode(err.into_bytes()),
                Some(resolved.mime.clone()),
            )
        }
    };
    Json(json!({
        "id": content_id,
        "kind": "resource",
        "content": content,
        "mimeType": mime_type,
    }))
    .into_response()
}

// ═══════════════ F0319FC4F93D：单会话滚动摘要 ═══════════════

/// `GET /lingxi/v1/sessions/{session_id}/summary` — 经读取授权返回滚动
/// 摘要记录（现役 sessions.ts:988-1011）。记录源 = TS 落盘契约
/// `agents/{agentId}/memory/summaries/{sessionId}.json`
/// （lib/memory/session-summary.ts；Rust 侧暂无写入面——摘要 ticker 未
/// 移植——读面按同一契约读盘，空/缺失如实报 hasSummary:false）。
pub async fn session_summary_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
) -> Response {
    let facts = match gate_owned(&state, &principal, &session_id).await {
        Ok(facts) => facts,
        Err(resp) => return resp,
    };
    let record = read_session_summary_record(
        state.config().data_home.as_path(),
        &facts.agent_id,
        &session_id,
    );
    let has_summary = record
        .as_ref()
        .and_then(|value| value.get("summary"))
        .and_then(Value::as_str)
        .is_some_and(|summary| !summary.trim().is_empty());
    let field = |key: &str| {
        record
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    Json(json!({
        "hasSummary": has_summary,
        "summary": if has_summary { field("summary") } else { None },
        "createdAt": field("created_at"),
        "updatedAt": field("updated_at"),
    }))
    .into_response()
}

/// 滚动摘要记录读取（session-summary.ts getSummary：文件缺失/解析失败 →
/// null，不抛错）。
pub fn read_session_summary_record(
    home: &std::path::Path,
    agent_id: &str,
    session_id: &str,
) -> Option<Value> {
    // TS _filePath：去掉 .jsonl 后缀（Rust 会话 id 本就无后缀）。
    let clean_id = session_id.strip_suffix(".jsonl").unwrap_or(session_id);
    let path = home
        .join("agents")
        .join(agent_id)
        .join("memory")
        .join("summaries")
        .join(format!("{clean_id}.json"));
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

// ═══════════════ F6A1A50E97E4：会话地图回合折叠 ═══════════════

/// `GET /lingxi/v1/sessions/{session_id}/conversation-map/turns` — 读取
/// 会话消息并折叠问答卡片（现役 conversation-map.ts:172-209 +
/// foldMessagesIntoTurns :99-167）。折叠规则：user 项开新回合；
/// assistant final 设答案；工具卡计 processCount；超限截断标注 truncated。
pub async fn conversation_map_turns_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
) -> Response {
    let facts = match gate_owned(&state, &principal, &session_id).await {
        Ok(facts) => facts,
        Err(resp) => return resp,
    };
    let db = Arc::clone(state.storage());
    let projected = match project_full_branch(&db, &session_id).await {
        Ok(projected) => projected,
        Err(err) => return storage_error_response(&err),
    };
    let turns = fold_history_items_into_turns(&projected.items);
    Json(json!({
        "sessionId": facts.session_id,
        "turns": turns,
    }))
    .into_response()
}

/// foldMessagesIntoTurns 的投影项端口。entryIds 收集各身份载体
/// （message_id / tool_call_id；TS 收集消息条目 id——Rust 投影的工具卡
/// 身份是稳定 toolCallId，同一语义）。
pub fn fold_history_items_into_turns(items: &[HistoryItem]) -> Vec<Value> {
    #[derive(Default)]
    struct Turn {
        question_entry_id: Option<String>,
        question: String,
        answer_entry_id: Option<String>,
        answer: String,
        process_count: u64,
        entry_ids: Vec<String>,
    }
    let mut turns: Vec<Turn> = Vec::new();
    for item in items {
        match item {
            HistoryItem::UserMessage {
                message_id, text, ..
            } => {
                let entry_ids = vec![message_id.clone()];
                turns.push(Turn {
                    question_entry_id: Some(message_id.clone()),
                    question: text.trim().to_string(),
                    entry_ids,
                    ..Turn::default()
                });
            }
            HistoryItem::FinalMessage {
                message_id,
                content,
                ..
            } => {
                if turns.is_empty() {
                    turns.push(Turn::default());
                }
                let turn = turns.last_mut().expect("非空");
                let text = content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if !text.trim().is_empty() {
                    turn.answer = text.trim().to_string();
                    turn.answer_entry_id = Some(message_id.clone());
                }
                turn.entry_ids.push(message_id.clone());
            }
            HistoryItem::ToolCall { tool_call_id, .. } => {
                if let Some(turn) = turns.last_mut() {
                    turn.process_count += 1;
                    turn.entry_ids.push(tool_call_id.clone());
                }
            }
            _ => {}
        }
    }
    turns
        .into_iter()
        .enumerate()
        .map(|(index, mut turn)| {
            let mut truncated = false;
            if turn.question.chars().count() > TURN_TEXT_LIMIT {
                turn.question = turn.question.chars().take(TURN_TEXT_LIMIT).collect();
                truncated = true;
            }
            if turn.answer.chars().count() > TURN_TEXT_LIMIT {
                turn.answer = turn.answer.chars().take(TURN_TEXT_LIMIT).collect();
                truncated = true;
            }
            json!({
                "turnIndex": index,
                "questionEntryId": turn.question_entry_id,
                "question": turn.question,
                "questionAt": null,
                "answerEntryId": turn.answer_entry_id,
                "answer": turn.answer,
                "answerAt": null,
                "processCount": turn.process_count,
                "entryIds": turn.entry_ids,
                "truncated": truncated,
            })
        })
        .collect()
}

// ═══════════════ D62A81C2AEB2：fresh-compact ═══════════════

/// `POST /lingxi/v1/sessions/{session_id}/fresh-compact` — 压缩旧上下文
/// 后按当前 Agent 配置重建提示词工件（现役 sessions.ts:2430-2452 →
/// engine.freshCompactDesktopSession :3145-3181）。压缩服务从 ServiceState
/// 取共享实例（组合根与 runs supervisor 同一 Arc）；未接真实模型链时
/// 显式 503 拒绝——不静默降级为仅重建。
pub async fn fresh_compact_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    manual_compact(&state, &principal, &session_id, &body, true).await
}
/// 手动压缩只跳过自动触发/enabled闸，提交与 fresh 共用同一原子来源合同。
pub async fn compact_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    manual_compact(&state, &principal, &session_id, &body, false).await
}
pub async fn cancel_compaction_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Path(session_id): Path<String>,
) -> Response {
    if let Err(response) = gate_owned(&state, &principal, &session_id).await {
        return response;
    }
    let Some(compaction) = state.compaction() else {
        return EndpointError::resource_failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "compaction_service_unavailable",
            "no real compaction service",
        )
        .into_response();
    };
    match compaction.cancel_manual(&session_id) {
        Ok(true) => Json(json!({"ok":true,"cancelled":true})).into_response(),
        Ok(false) => EndpointError::resource_failure(
            StatusCode::CONFLICT,
            "no_active_compaction",
            "no cancellable manual compaction",
        )
        .into_response(),
        Err(error) => storage_error_response(&error),
    }
}
struct ManualScopeGuard {
    service: Arc<crate::compaction::CompactionService>,
    session: String,
}
impl Drop for ManualScopeGuard {
    fn drop(&mut self) {
        self.service.finish_manual(&self.session);
    }
}
fn tool_snapshot_receipt(tools: &lingxi_kernel::model_exchange::ToolDeclarationSnapshot) -> Value {
    json!({"catalogGeneration":tools.catalog_generation,"declarations":tools.declarations.iter().map(|d|json!({"target":d.target,"wireName":d.wire_name,"description":d.description,"inputSchema":d.input_schema})).collect::<Vec<_>>()})
}
fn current_tools(
    state: &ServiceState,
) -> Result<lingxi_kernel::model_exchange::ToolDeclarationSnapshot, StorageError> {
    match state.tool_gateway() {
        Some(gateway) => lingxi_kernel::model_exchange::ToolDeclarationSnapshot::from_registry(
            gateway.registry(),
        )
        .map_err(|e| StorageError::InvalidRequest {
            detail: format!("compaction tools: {e}"),
        }),
        None => Ok(lingxi_kernel::model_exchange::ToolDeclarationSnapshot::empty()),
    }
}
/// 仅重建较早区域的可见文字；工具双方、推理与非文字载荷一起省略，尾部逐项保留。
fn local_compaction(
    exchange: &[lingxi_kernel::model_exchange::ExchangeItem],
    keep: u64,
    tokens_before: u64,
) -> Result<CompactionOutcome, StorageError> {
    use lingxi_kernel::model_exchange::ExchangeItem;
    let Some(plan) = lingxi_kernel::compaction::plan_compaction(exchange, keep).map_err(|e| {
        StorageError::InvalidRequest {
            detail: e.to_string(),
        }
    })?
    else {
        return Ok(CompactionOutcome::NotTriggered);
    };
    let mut sections=vec!["# Instant Simple Compaction Checkpoint".to_string(),"This checkpoint was rebuilt locally without a model request. Tool calls, tool results, hidden reasoning, and non-text payloads from the reconstructed region were omitted. The recent tail after this checkpoint remains verbatim.".to_string(),"## Reconstructed Verbatim Text".to_string()];
    for item in &exchange[..plan.cut_index] {
        match item {
            ExchangeItem::UserTurn { text } | ExchangeItem::UserMediaTurn { text, .. }
                if !text.is_empty() =>
            {
                sections.push(format!("### User\n{text}"))
            }
            ExchangeItem::AssistantTurn { content, .. } => {
                let text = content
                    .iter()
                    .filter_map(|b| match b {
                        lingxi_protocol::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    sections.push(format!("### Assistant\n{text}"));
                }
            }
            ExchangeItem::CompactionSummary { summary, .. } => {
                sections.push(format!("### Earlier Compaction Checkpoint\n{summary}"))
            }
            _ => {}
        }
    }
    sections.extend([
        "## In Progress".into(),
        "- Continue the active task from this checkpoint and the unchanged recent tail.".into(),
        "## Next Steps".into(),
        "1. Follow the newest retained user message and do not repeat completed work.".into(),
    ]);
    let mut after = vec![ExchangeItem::CompactionSummary {
        summary: sections.join("\n\n"),
        covered_items: u32::try_from(plan.cut_index).map_err(|_| StorageError::InvalidRequest {
            detail: "local compaction cut exceeds bound".into(),
        })?,
        mid_run: false,
    }];
    after.extend_from_slice(&exchange[plan.cut_index..]);
    Ok(CompactionOutcome::Compacted {
        exchange: after,
        plan,
        report: lingxi_kernel::compaction::TurnBudgetReport {
            system_tokens: 0,
            submission_tokens: 0,
            tool_declaration_tokens: 0,
            history_estimate_tokens: tokens_before,
            tail_estimate_tokens: 0,
            output_reserve_tokens: 0,
            usage_context_tokens: None,
            context_tokens: tokens_before,
            context_window: 0,
            ratio_bp: 0,
        },
    })
}
pub(crate) async fn manual_compact(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
    body: &[u8],
    fresh: bool,
) -> Response {
    compact_with_method(state, principal, session_id, body, fresh, false).await
}
/// 实验动作复用相同的归属、admission、取消及原子历史提交。
pub(crate) async fn instant_compact(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
) -> Response {
    let preferences = match state.memory().read_preferences_root() {
        Ok(value) => value,
        Err(error) => {
            return storage_error_response(&StorageError::InvalidRequest { detail: error })
        }
    };
    if preferences["experiments"]["session.instant_simple_compaction"] != true {
        return EndpointError::resource_failure(
            StatusCode::CONFLICT,
            "experiment_disabled",
            "instant simple compaction experiment is disabled",
        )
        .into_response();
    }
    compact_with_method(
        state,
        principal,
        session_id,
        br#"{"method":"instant_simple"}"#,
        false,
        true,
    )
    .await
}
async fn compact_with_method(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
    body: &[u8],
    fresh: bool,
    instant: bool,
) -> Response {
    let facts = match gate_owned(state, principal, session_id).await {
        Ok(facts) => facts,
        Err(response) => return response,
    };
    let payload: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(body) {
            Ok(value) => value,
            Err(e) => {
                return EndpointError::resource_failure(
                    StatusCode::BAD_REQUEST,
                    "invalid_compaction_input",
                    e.to_string(),
                )
                .into_response()
            }
        }
    };
    let Some(map) = payload.as_object() else {
        return EndpointError::resource_failure(
            StatusCode::BAD_REQUEST,
            "invalid_compaction_input",
            "body must be an object",
        )
        .into_response();
    };
    let method = if instant {
        "instant_simple"
    } else if fresh {
        "fresh-compact"
    } else {
        "compact"
    };
    if map
        .keys()
        .any(|key| !matches!(key.as_str(), "customInstructions" | "method"))
        || map
            .get("method")
            .is_some_and(|value| value.as_str() != Some(method))
        || map
            .get("customInstructions")
            .is_some_and(|value| value.as_str().is_none_or(|text| text.len() > 8192))
    {
        return EndpointError::resource_failure(
            StatusCode::BAD_REQUEST,
            "invalid_compaction_input",
            "unknown method/key or invalid customInstructions (max 8192 bytes)",
        )
        .into_response();
    }
    let Some(compaction) = state.compaction().cloned() else {
        return EndpointError::resource_failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "compaction_service_unavailable",
            "no real model chain",
        )
        .into_response();
    };
    let _permit = match state
        .storage()
        .session_admission()
        .try_acquire(&[session_id], usize::MAX)
    {
        Ok(permit) => permit,
        Err(_) => return EndpointError::session_busy().into_response(),
    };
    // admission之后复检生命周期，拒绝与归档/删除并发的旧快照。
    let row = match gate_active(state, session_id).await {
        Ok(row) => row,
        Err(response) => return response,
    };
    let settings = match compaction.settings_for(&facts.agent_id) {
        Ok(settings) => settings,
        Err(error) => return storage_error_response(&error),
    };
    let Some(compiler) = state.context_compiler() else {
        return EndpointError::resource_failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "context_compiler_unavailable",
            "no real context compiler",
        )
        .into_response();
    };
    let compiled = match compiler.compile_snapshot(
        session_id,
        &facts.agent_id,
        false,
        row.created_at_unix_ms.unsigned_abs(),
        row.memory_enabled,
    ) {
        Ok(compiled) => compiled,
        Err(error) => {
            return EndpointError::resource_failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "context_compile_failed",
                error.to_string(),
            )
            .into_response()
        }
    };
    let system_prompt = compiled.render().to_string();
    let observation = compiled.observation().clone();
    let rebuilt = json!({"renderTokens":observation["renderTokens"],"preambleTokens":observation["preambleTokens"],"materialSha256":observation["materialSha256"],"materialRevision":observation["materialRevision"]});
    let kernel_principal = crate::sessions::kernel_principal_of(principal);
    let history = crate::model_history::ModelHistory::new(Arc::clone(state.storage()));
    let (chain, pre_revision) = match cms::read_full_branch(
        state.storage(),
        session_id,
        crate::history::MAX_HISTORY_LIMIT,
    )
    .await
    {
        Ok(facts) => facts,
        Err(error) => return storage_error_response(&error),
    };
    let pre_head = chain.last().map(|row| row.message_id.clone());
    let exchange = match history
        .load(&kernel_principal, &facts.agent_id, session_id, "")
        .await
    {
        Ok(exchange) => exchange,
        Err(error) => return storage_error_response(&error),
    };
    let tools = match current_tools(state) {
        Ok(tools) => tools,
        Err(error) => return storage_error_response(&error),
    };
    let tool_receipt = tool_snapshot_receipt(&tools);
    let input = MidRunCompactionInput {
        exchange: &exchange,
        system_prompt: Some(&system_prompt),
        submission: "",
        tools: &tools,
        last_usage: None,
        tail_from: exchange.len(),
    };
    // action是真实手动操作身份；底层辅助执行器只复用关联容器，绝不写Run或用户输入。
    let nonce = format!(
        "{}:{}:{}:{}:{}",
        session_id,
        state.clock().now_unix_ms(),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        method
    );
    let action_id = format!(
        "compact-action-{}",
        lingxi_protocol::canon::sha256_hex(nonce.as_bytes())
    );
    let ctx = RunContext {
        principal: kernel_principal.clone(),
        session_id: lingxi_protocol::SessionId::new(session_id),
        run_id: lingxi_protocol::RunId::new(&action_id),
        attempt: lingxi_protocol::AttemptId::new("manual-action-correlation"),
        generation: 1,
    };
    let cancel = CancelScope::background_root(&action_id);
    let commit_phase = match compaction.register_manual_cancel(session_id, cancel.clone()) {
        Ok(phase) => phase,
        Err(error) => return storage_error_response(&error),
    };
    let _scope = ManualScopeGuard {
        service: compaction.clone(),
        session: session_id.into(),
    };
    state.app_events().publish(
        "compaction_accepted",
        json!({"sessionId":session_id,"actionId":action_id,"method":method}),
    );
    let tokens_before = lingxi_kernel::compaction::estimate_exchange_tokens(&exchange)
        .saturating_add(u64::from(lingxi_kernel::context::estimate_text_tokens(
            &system_prompt,
        )));
    let outcome = if instant {
        match local_compaction(&exchange, settings.keep_recent_tokens, tokens_before) {
            Ok(value) => value,
            Err(error) => return storage_error_response(&error),
        }
    } else {
        match compaction
            .compact_now_with_focus(
                &ctx,
                state.storage().as_ref(),
                &facts.agent_id,
                if fresh {
                    "session.fresh_compact"
                } else {
                    "session.manual_compact"
                },
                None,
                Some(action_id.clone()),
                &input,
                &cancel,
                0,
                payload["customInstructions"].as_str(),
            )
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => return storage_error_response(&error),
        }
    };
    let (tokens_after, context_window, noop_reason, degraded) = match &outcome {
        CompactionOutcome::Compacted {
            exchange: after,
            report,
            ..
        }
        | CompactionOutcome::HardTruncated {
            exchange: after,
            report,
            ..
        } => {
            if cancel.is_cancelled() {
                return EndpointError::resource_failure(
                    StatusCode::CONFLICT,
                    "compaction_cancelled",
                    "manual compaction cancelled before commit",
                )
                .into_response();
            }
            let confirm = match compiler.compile_snapshot(
                session_id,
                &facts.agent_id,
                false,
                row.created_at_unix_ms.unsigned_abs(),
                row.memory_enabled,
            ) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return EndpointError::resource_failure(
                        StatusCode::CONFLICT,
                        "compaction_context_changed",
                        error.to_string(),
                    )
                    .into_response()
                }
            };
            let confirm_tools = match current_tools(state) {
                Ok(tools) => tools,
                Err(error) => return storage_error_response(&error),
            };
            let confirm_settings = match compaction.settings_for(&facts.agent_id) {
                Ok(settings) => settings,
                Err(error) => return storage_error_response(&error),
            };
            if confirm.render() != system_prompt
                || tool_snapshot_receipt(&confirm_tools) != tool_receipt
                || confirm_settings != settings
            {
                return EndpointError::resource_failure(
                    StatusCode::CONFLICT,
                    "compaction_context_changed",
                    "materials/tools/settings changed during compaction; history preserved",
                )
                .into_response();
            }
            let final_compiler = Arc::clone(compiler);
            let final_memory = Arc::clone(state.memory());
            let final_compaction = Arc::clone(&compaction);
            let final_registry = state
                .tool_gateway()
                .map(|gateway| Arc::clone(gateway.registry()));
            let final_agent = facts.agent_id.clone();
            let final_session = session_id.to_string();
            let final_prompt = system_prompt.clone();
            let final_observation = observation.clone();
            let final_settings = settings.clone();
            let final_generation = tools.catalog_generation;
            let started = row.created_at_unix_ms.unsigned_abs();
            let memory_enabled = row.memory_enabled;
            let context_commit: crate::model_history::ManualContextCommit = Arc::new(
                move |write| {
                    let refused = || {
                        StorageError::Conflict {
                        detail: "materials/tools/settings changed before manual archive COMMIT; history preserved".into(),
                    }
                    };
                    let mut save = || {
                        final_compiler.with_material_commit(|| {
                            if instant
                                && final_memory
                                    .read_preferences_root()
                                    .map_err(|_| refused())?["experiments"]
                                    ["session.instant_simple_compaction"]
                                    != true
                            {
                                return Err(refused());
                            }
                            // 在拿到真实数据库写锁之后采集；栅栏一直保留到COMMIT结束。
                            // 直接文件编辑按既有双采集的最终稳定快照裁决，不沿用HTTP复检。
                            let snapshot = final_compiler
                                .compile_snapshot(
                                    &final_session,
                                    &final_agent,
                                    false,
                                    started,
                                    memory_enabled,
                                )
                                .map_err(|_| refused())?;
                            if snapshot.render() != final_prompt
                                || snapshot.observation()["materialSha256"]
                                    != final_observation["materialSha256"]
                                || snapshot.observation()["materialRevision"]
                                    != final_observation["materialRevision"]
                                || final_compaction
                                    .settings_for(&final_agent)
                                    .map_err(|_| refused())?
                                    != final_settings
                            {
                                return Err(refused());
                            }
                            write()
                        })
                    };
                    match &final_registry {
                        Some(registry) => registry
                            .with_catalog_generation(final_generation, &mut save)
                            .unwrap_or_else(|| Err(refused())),
                        None => save(),
                    }
                },
            );
            let commit = crate::model_history::ManualCompactionAction {
                principal: &kernel_principal,
                agent: &facts.agent_id,
                session: session_id,
                action_id: &action_id,
                pre_head,
                pre_revision,
                before: &exchange,
                after,
                refreshed_context: json!({"render":system_prompt,"observation":observation,"tools":tool_receipt,"fresh":fresh,"request":{"method":method,"customInstructions":payload["customInstructions"]},"runtimeMode":if instant{"lossy_local"}else{"cache_preserving"},"zeroModelRequest":instant}),
                cancel: cancel.clone(),
                commit_phase: commit_phase.clone(),
                now: state.clock().now_unix_ms(),
                context_commit,
            };
            if let Err(error) = history.record_manual_compaction(commit).await {
                if matches!(&error,StorageError::InvalidRequest{detail} if detail=="model_history: branch head changed before archive commit")
                {
                    return EndpointError::resource_failure(
                        StatusCode::CONFLICT,
                        "compaction_branch_changed",
                        "effective branch changed during compaction; history preserved",
                    )
                    .into_response();
                }
                if matches!(&error, StorageError::Conflict { .. }) {
                    return EndpointError::resource_failure(
                        StatusCode::CONFLICT,
                        "compaction_commit_refused",
                        error.to_string(),
                    )
                    .into_response();
                }
                return storage_error_response(&error);
            }
            (
                lingxi_kernel::compaction::estimate_exchange_tokens(after).saturating_add(
                    u64::from(lingxi_kernel::context::estimate_text_tokens(&system_prompt)),
                ),
                report.context_window,
                None,
                matches!(outcome, CompactionOutcome::HardTruncated { .. }),
            )
        }
        CompactionOutcome::NotTriggered => (tokens_before, 0, Some("nothing_to_compact"), false),
        CompactionOutcome::Cancelled => {
            return EndpointError::resource_failure(
                StatusCode::CONFLICT,
                "compaction_cancelled",
                "manual compaction cancelled; history preserved",
            )
            .into_response()
        }
        CompactionOutcome::Failed { detail } => {
            return EndpointError::resource_failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                if fresh {
                    "fresh_compact_failed"
                } else {
                    "compaction_failed"
                },
                detail.clone(),
            )
            .into_response()
        }
    };
    state.app_events().publish("compaction_succeeded",json!({"sessionId":session_id,"actionId":action_id,"noopReason":noop_reason,"degraded":degraded}));
    Json(json!({"ok":true,"fresh":fresh,"actionId":action_id,"tokensBefore":tokens_before,"tokensAfter":tokens_after,"contextWindow":if context_window>0{Value::from(context_window)}else{Value::Null},"noopReason":noop_reason,"degraded":degraded,"rebuiltPrompt":rebuilt})).into_response()
}

// ═══════════════ 5DCB0352EB08：跨会话协作模型工具（session tool） ═══════════════

/// 跨会话投递端口（lib/session-collab/delivery.ts deliverAgentMessage 的
/// 宿主接线点）：空闲目标走完整回合提交、忙目标走插入（steer）。生产
/// 实现需要 ServiceState 的执行链（port/events/supervisor）与服务主体，
/// 由 lib.rs 组合根在注册时注入；本模块不复制执行链。
pub trait SessionCollabDelivery: Send + Sync {
    /// 向目标会话投递一条消息（返回 Err = 明确失败原因）。
    fn deliver<'a>(
        &'a self,
        target_session_id: &'a str,
        message: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>;
    /// 建新会话（返回新会话 id）。
    fn create_session<'a>(
        &'a self,
        agent_id: &'a str,
        model: Option<&'a str>,
        title: Option<&'a str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + 'a>>;
}

/// 跨会话协作工具（lib/tools/session-tool.ts 端口）。list/read 直读真
/// 存储（主体过滤）；send/create 产确认草稿（TS 草稿语义：工具从不直接
/// 投递，用户确认后经 [`SessionCollabTools::apply_draft`] 执行）。
pub struct SessionCollabTools {
    state: ServiceState,
    delivery: Arc<dyn SessionCollabDelivery>,
    /// 可用模型键（"provider/id"；TS resolveAvailableModel 的输入）。
    models: Vec<String>,
    drafts: std::sync::Mutex<HashMap<String, SessionCollabDraft>>,
}

/// 一次性草稿（draft-store.ts 的结构化端口：apply 闭包以载荷+行动表达）。
#[derive(Debug, Clone)]
pub struct SessionCollabDraft {
    pub suggestion_id: String,
    /// send | create | create_retry_first_message。
    pub kind: String,
    pub source_session_id: String,
    pub payload: Value,
    pub created_at_unix_ms: u64,
}

/// apply 的结局（draft-store.ts apply + SessionCollabPartialSuccessError
/// 的形状端口）。
#[derive(Debug, Clone, PartialEq)]
pub enum DraftApplyOutcome {
    /// 草稿不存在（not-found）。
    Rejected { reason: String },
    /// send：已投递。
    Sent { target_session_id: String },
    /// create：会话已建 + 首条消息已投递。
    Created { session_id: String },
    /// create 半成功：会话已建、首条消息失败（保留新会话 id，草稿切换为
    /// 仅重试首条消息——retryable）。
    PartialSuccess { session_id: String, detail: String },
    /// 一般失败：草稿保留供重试。
    Failed { detail: String },
}

impl SessionCollabTools {
    pub fn new(
        state: ServiceState,
        delivery: Arc<dyn SessionCollabDelivery>,
        models: Vec<String>,
    ) -> Self {
        Self {
            state,
            delivery,
            models,
            drafts: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// 主体可见的会话行（跨会话范围受控：TS 经 engine.listSessions 的
    /// 授权投影；Rust 经会话行 + 主体归属）。
    async fn owned_session_rows(
        &self,
        principal: &lingxi_kernel::Principal,
    ) -> Result<Vec<lingxi_adapters::storage::SessionRow>, String> {
        let rows = self
            .state
            .storage()
            .list_sessions()
            .await
            .map_err(|err| err.to_string())?;
        Ok(rows
            .into_iter()
            .filter(|row| kernel_can_access(principal, &row.owner_user_id))
            .collect())
    }

    /// 工具 action 派发（session-tool.ts execute）。
    pub async fn run_action(&self, ctx: &RunContext, params: &Value) -> ToolOutcome {
        let action = params["action"].as_str().unwrap_or_default();
        match action {
            "?" => text_success(session_tool_handbook()),
            "list" => self.action_list(ctx, params).await,
            "read" => self.action_read(ctx, params).await,
            "send" => self.action_send(ctx, params).await,
            "create" => self.action_create(ctx, params).await,
            other => usage_error(other, format!("Unknown action: {other}")),
        }
    }

    async fn action_list(&self, ctx: &RunContext, params: &Value) -> ToolOutcome {
        let Ok(rows) = self.owned_session_rows(&ctx.principal).await else {
            return failed(
                ErrorCode::Internal,
                "session tool list failed: storage unavailable".to_string(),
            );
        };
        let query = params["query"].as_str().unwrap_or_default().trim();
        let mut rows = rows;
        if !query.is_empty() {
            let needle = query.to_lowercase();
            rows.retain(|row| row.title.to_lowercase().contains(&needle));
        }
        if rows.is_empty() {
            return text_success(if query.is_empty() {
                "No sessions.".to_string()
            } else {
                format!("No sessions matched \"{query}\".")
            });
        }
        let lines = rows
            .iter()
            .map(|row| {
                format!(
                    "- {} · {} · {}",
                    row.session_id,
                    if row.title.trim().is_empty() {
                        "(untitled)"
                    } else {
                        row.title.as_str()
                    },
                    row.agent_id
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        text_success(lines)
    }

    async fn action_read(&self, ctx: &RunContext, params: &Value) -> ToolOutcome {
        let session_id = params["sessionId"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        if session_id.is_empty() {
            return usage_error("read", "sessionId is required for read.");
        }
        let Ok(row) = tree_store::get_session_row(self.state.storage(), &session_id)
            .await
            .map_err(|err| err.to_string())
        else {
            return failed(
                ErrorCode::Internal,
                "session tool read failed: storage unavailable".to_string(),
            );
        };
        let Some(row) = row else {
            return usage_error("read", format!("Session not found: {session_id}"));
        };
        if !kernel_can_access(&ctx.principal, &row.owner_user_id) {
            // 范围受控：他人会话一律不可读（不呈现任何目标事实——与
            // 不存在同一措辞，不泄露存在性）。
            return usage_error("read", format!("Session not found: {session_id}"));
        }
        if params["mode"].as_str() != Some("transcript") {
            // summary 模式（session-tool.ts:147-161）：摘要从不按需生成；
            // 无记录即如实说明。
            let record = read_session_summary_record(
                self.state.config().data_home.as_path(),
                &row.agent_id,
                &session_id,
            );
            let summary = record
                .as_ref()
                .and_then(|value| value.get("summary"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if !summary.is_empty() {
                let updated = record
                    .as_ref()
                    .and_then(|value| value.get("updated_at"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                return text_success(format!(
                    "session {session_id} · agent {} · summary (updated {updated}):\n{summary}\n\nFor details use mode:\"transcript\".",
                    row.agent_id
                ));
            }
            return text_success(format!(
                "session {session_id} · agent {} — no summary exists for this session.\nUse mode:\"transcript\" to read the actual turns. (Summaries are never generated on demand.)",
                row.agent_id
            ));
        }
        let db = Arc::clone(self.state.storage());
        let Ok(projected) = project_full_branch(&db, &session_id).await else {
            return failed(
                ErrorCode::Internal,
                "session tool read failed: history unavailable".to_string(),
            );
        };
        let turns = fold_history_items_into_turns(&projected.items);
        if turns.is_empty() {
            return text_success(format!(
                "session {session_id} · agent {} — no turns on the current branch.",
                row.agent_id
            ));
        }
        let body = turns
            .iter()
            .map(|turn| {
                format!(
                    "Q: {}\nA: {}",
                    turn["question"].as_str().unwrap_or_default(),
                    turn["answer"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        text_success(format!(
            "session {session_id} · agent {} · transcript ({} turns):\n\n{body}",
            row.agent_id,
            turns.len()
        ))
    }

    async fn action_send(&self, ctx: &RunContext, params: &Value) -> ToolOutcome {
        let session_id = params["sessionId"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        let message = params["message"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        if session_id.is_empty() || message.is_empty() {
            return usage_error("send", "sessionId and message are required for send.");
        }
        let source_session_id = ctx.session_id.as_str().to_string();
        if source_session_id.is_empty() {
            return text_success("send requires an active desktop session context".to_string());
        }
        if source_session_id == session_id {
            return usage_error("send", "Refusing to send to the current session itself.");
        }
        let row = match tree_store::get_session_row(self.state.storage(), &session_id).await {
            Ok(Some(row)) => row,
            Ok(None) => {
                return usage_error("send", format!("Session not found: {session_id}"));
            }
            Err(err) => {
                return failed(
                    ErrorCode::Internal,
                    format!("session tool send failed: {err}"),
                );
            }
        };
        if !kernel_can_access(&ctx.principal, &row.owner_user_id) {
            return usage_error("send", format!("Session not found: {session_id}"));
        }
        let draft = self.create_draft(
            "send",
            &source_session_id,
            json!({"targetSessionId": session_id, "message": message}),
        );
        text_success(format!(
            "Draft created ({}); waiting for the user to confirm the card. The user may edit or reject it. Check the target session later with action:\"read\".",
            draft.suggestion_id
        ))
    }

    async fn action_create(&self, ctx: &RunContext, params: &Value) -> ToolOutcome {
        let message = params["message"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        let agent = params["agent"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        if agent.is_empty() || message.is_empty() {
            return usage_error("create", "agent and message are required for create.");
        }
        let source_session_id = ctx.session_id.as_str().to_string();
        if source_session_id.is_empty() {
            return text_success("create requires an active desktop session context".to_string());
        }
        // roster（TS resolveAgentFromRoster：id 精确命中或唯一 name 命中）。
        let summaries = match self.state.memory().list_agent_summaries() {
            Ok(summaries) => summaries,
            Err(err) => {
                return failed(
                    ErrorCode::Internal,
                    format!("session tool create failed: {}", err.message),
                );
            }
        };
        let by_id = summaries
            .iter()
            .find(|summary| summary["id"].as_str() == Some(agent.as_str()));
        let by_name: Vec<&Value> = summaries
            .iter()
            .filter(|summary| summary["name"].as_str() == Some(agent.as_str()))
            .collect();
        let target_agent = match by_id.or(match by_name.as_slice() {
            [only] => Some(*only),
            _ => None,
        }) {
            Some(agent) => agent["id"].as_str().unwrap_or_default().to_string(),
            None => {
                let roster = summaries
                    .iter()
                    .filter_map(|summary| {
                        let id = summary["id"].as_str()?;
                        let name = summary["name"].as_str().unwrap_or_default();
                        Some(if name.is_empty() || name == id {
                            format!("- {id}")
                        } else {
                            format!("- {id} ({name})")
                        })
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                return usage_error(
                    "create",
                    format!("Unknown agent \"{agent}\". Available agents:\n{roster}"),
                );
            }
        };
        let mut model_key: Option<String> = None;
        if let Some(model) = params["model"]
            .as_str()
            .map(str::trim)
            .filter(|model| !model.is_empty())
        {
            // 模型无效分开报告（TS resolveAvailableModel 抛
            // session_create_invalid_model）。
            if !self.models.iter().any(|key| key == model) {
                return usage_error(
                    "create",
                    format!(
                        "session_create_invalid_model: model is not currently available: {model}"
                    ),
                );
            }
            model_key = Some(model.to_string());
        }
        let draft = self.create_draft(
            "create",
            &source_session_id,
            json!({
                "agentId": target_agent,
                "model": model_key,
                "title": params["title"].as_str(),
                "firstMessage": message,
            }),
        );
        text_success(format!(
            "Draft created ({}); waiting for the user to confirm session creation for agent {}.",
            draft.suggestion_id, target_agent
        ))
    }

    fn create_draft(
        &self,
        kind: &str,
        source_session_id: &str,
        payload: Value,
    ) -> SessionCollabDraft {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let suggestion_id = format!(
            "session_{}_{}",
            self.state.clock().now_unix_ms(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let draft = SessionCollabDraft {
            suggestion_id: suggestion_id.clone(),
            kind: kind.to_string(),
            source_session_id: source_session_id.to_string(),
            payload,
            created_at_unix_ms: self.state.clock().now_unix_ms(),
        };
        self.drafts
            .lock()
            .expect("session collab draft store")
            .insert(suggestion_id, draft.clone());
        draft
    }

    /// 草稿确认执行（draft-store.ts apply）。create 半成功：会话已建而
    /// 首条消息失败 → 保留新会话 id，草稿切换为仅重试首条消息。
    pub async fn apply_draft(&self, suggestion_id: &str) -> DraftApplyOutcome {
        let draft = {
            let store = self.drafts.lock().expect("session collab draft store");
            match store.get(suggestion_id) {
                Some(draft) => draft.clone(),
                None => {
                    return DraftApplyOutcome::Rejected {
                        reason: "not-found".to_string(),
                    }
                }
            }
        };
        match draft.kind.as_str() {
            "send" => {
                let target = draft.payload["targetSessionId"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let message = draft.payload["message"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if target.is_empty() || message.is_empty() {
                    return DraftApplyOutcome::Failed {
                        detail: "send draft payload is incomplete".to_string(),
                    };
                }
                match self.delivery.deliver(&target, &message).await {
                    Ok(()) => {
                        self.drafts
                            .lock()
                            .expect("session collab draft store")
                            .remove(suggestion_id);
                        DraftApplyOutcome::Sent {
                            target_session_id: target,
                        }
                    }
                    Err(detail) => DraftApplyOutcome::Failed { detail },
                }
            }
            "create" => {
                let agent = draft.payload["agentId"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let first_message = draft.payload["firstMessage"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if agent.is_empty() || first_message.is_empty() {
                    return DraftApplyOutcome::Failed {
                        detail: "create draft payload is incomplete".to_string(),
                    };
                }
                let model = draft.payload["model"].as_str().map(str::to_string);
                let title = draft.payload["title"].as_str().map(str::to_string);
                // 确认前 agent/model 配置可能变化：以当前事实重验。
                let summaries = match self.state.memory().list_agent_summaries() {
                    Ok(summaries) => summaries,
                    Err(err) => {
                        return DraftApplyOutcome::Failed {
                            detail: format!("session_create_invalid_agent: {}", err.message),
                        }
                    }
                };
                if !summaries
                    .iter()
                    .any(|summary| summary["id"].as_str() == Some(agent.as_str()))
                {
                    return DraftApplyOutcome::Failed {
                        detail: format!(
                            "session_create_invalid_agent: agent is not currently available: {agent}"
                        ),
                    };
                }
                if let Some(model) = model.as_deref() {
                    if !self.models.iter().any(|key| key == model) {
                        return DraftApplyOutcome::Failed {
                            detail: format!(
                                "session_create_invalid_model: model is not currently available: {model}"
                            ),
                        };
                    }
                }
                let created = match self
                    .delivery
                    .create_session(&agent, model.as_deref(), title.as_deref())
                    .await
                {
                    Ok(session_id) => session_id,
                    Err(detail) => return DraftApplyOutcome::Failed { detail },
                };
                match self.delivery.deliver(&created, &first_message).await {
                    Ok(()) => {
                        self.drafts
                            .lock()
                            .expect("session collab draft store")
                            .remove(suggestion_id);
                        DraftApplyOutcome::Created {
                            session_id: created,
                        }
                    }
                    // 半成功：保留新会话，草稿切换为仅重试首条消息
                    //（draft-store.ts SessionCollabPartialSuccessError）。
                    Err(detail) => {
                        let mut payload = draft.payload.clone();
                        payload["model"] = Value::Null;
                        payload["title"] = Value::Null;
                        payload["retryFirstMessageOnly"] = json!(true);
                        payload["createdSessionId"] = json!(created);
                        self.drafts
                            .lock()
                            .expect("session collab draft store")
                            .insert(
                                suggestion_id.to_string(),
                                SessionCollabDraft {
                                    suggestion_id: suggestion_id.to_string(),
                                    kind: "create_retry_first_message".to_string(),
                                    source_session_id: draft.source_session_id,
                                    payload,
                                    created_at_unix_ms: self.state.clock().now_unix_ms(),
                                },
                            );
                        DraftApplyOutcome::PartialSuccess {
                            session_id: created,
                            detail,
                        }
                    }
                }
            }
            "create_retry_first_message" => {
                let session_id = draft.payload["createdSessionId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let first_message = draft.payload["firstMessage"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if session_id.is_empty() || first_message.is_empty() {
                    return DraftApplyOutcome::Failed {
                        detail: "retry draft payload is incomplete".to_string(),
                    };
                }
                match self.delivery.deliver(&session_id, &first_message).await {
                    Ok(()) => {
                        self.drafts
                            .lock()
                            .expect("session collab draft store")
                            .remove(suggestion_id);
                        DraftApplyOutcome::Created { session_id }
                    }
                    Err(detail) => DraftApplyOutcome::Failed { detail },
                }
            }
            other => DraftApplyOutcome::Rejected {
                reason: format!("unknown draft kind: {other}"),
            },
        }
    }
}

impl ToolExecutorPort for SessionCollabTools {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a lingxi_kernel::ports::ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let params = request.arguments.as_value().clone();
        Box::pin(async move {
            let outcome = self.run_action(ctx, &params).await;
            ToolExecutionResult::of_ctx(ctx, outcome)
        })
    }
}

/// 主体归属（sessions.rs can_access 的 kernel 侧镜像：LocalUser 全量，
/// 其余按 owner_user_id 相等；Automation 一律拒绝）。
fn kernel_can_access(principal: &lingxi_kernel::Principal, owner_user_id: &str) -> bool {
    match principal {
        lingxi_kernel::Principal::LocalUser => true,
        lingxi_kernel::Principal::Device { user_id, .. } => user_id == owner_user_id,
        lingxi_kernel::Principal::WebSession { account_id } => account_id == owner_user_id,
        lingxi_kernel::Principal::Automation { .. } => false,
    }
}

fn text_success(text: String) -> ToolOutcome {
    ToolOutcome::Success {
        result: lingxi_kernel::ports::ToolSuccess::from_content(vec![ContentBlock::Text { text }]),
    }
}

fn failed(code: ErrorCode, message: String) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(code, message, false),
    }
}

fn usage_error(action: &str, reason: impl Into<String>) -> ToolOutcome {
    failed(
        ErrorCode::InvalidMessage,
        format!(
            "session tool usage error (action {action}): {}",
            reason.into()
        ),
    )
}

/// 渐进式手册（session-collab/handbook.ts 的最小端口：action 面 + 输入
/// 纪律；正文按 TS 首屏指引同义改写）。
fn session_tool_handbook() -> String {
    "Cross-session collaboration. Actions:\n\
     - list [query]: list sessions you are authorized to see.\n\
     - read <sessionId> [mode=summary|transcript] [count]: read one session (summaries are never generated on demand).\n\
     - send <sessionId> <message>: create a confirmation card to deliver a message to that session (never to the current session itself).\n\
     - create <agent> <message> [model] [title]: create a confirmation card to open a new session for an agent and send the first message.\n\
     Identity discipline: sessionId in, sessionId out; paths never appear in tool output."
        .to_string()
}

// ═══════════════ 单元测试（纯函数面） ═══════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_envelope_stripping_matches_ts_shapes() {
        let raw = "第一行\n[hana_reminder]\n内部\n[/hana_reminder]\n\n第二行";
        assert_eq!(strip_model_envelope_blocks(raw), "第一行\n第二行");
        let reference = "前\n[hana_reference]\n参考\n[/hana_reference]\n后";
        assert_eq!(strip_model_envelope_blocks(reference), "前\n后");
        let knowledge = "前\n[KnowledgeContext]\n资料\n[/KnowledgeContext]\n后";
        assert_eq!(strip_model_envelope_blocks(knowledge), "前\n后");
        // 未闭合块 fail-closed 丢弃到文末。
        let unterminated = "前\n[hana_reference]\n未闭合";
        assert_eq!(strip_model_envelope_blocks(unterminated), "前");
        // 带时间戳的 reminder 头。
        let stamped = "a\n[hana_reminder at 2026-10-13 09:30]\nx\n[/hana_reminder]\nb";
        assert_eq!(strip_model_envelope_blocks(stamped), "a\nb");
    }

    #[test]
    fn bridge_time_tag_strip_matches_ts() {
        assert_eq!(
            sanitize_bridge_visible_text("  <t> 10-3 09:30 </t>你好"),
            "你好"
        );
        assert_eq!(
            sanitize_bridge_visible_text("保留 <t> 非时间标签"),
            "保留 <t> 非时间标签"
        );
    }

    #[test]
    fn find_scoring_matches_ts_contract() {
        let entries = vec![
            FindEntry {
                index: 0,
                text: "部署问题的第一句".to_string(),
            },
            FindEntry {
                index: 1,
                text: "部署流水线在 staging 挂了，需要重跑 deploy".to_string(),
            },
        ];
        let result = find_in_session_messages(&entries, "部署流水线");
        assert_eq!(result.total, 1);
        assert_eq!(result.best_index, Some(1));
        assert!(result.matches[0].exact);
        assert!(result.matches[0].snippet.contains("deploy"));
        // token 命中（非整句）。
        let token_result = find_in_session_messages(&entries, "staging 重跑");
        assert_eq!(token_result.total, 1);
        assert!(!token_result.matches[0].exact);
        // 无命中。
        let none = find_in_session_messages(&entries, "完全不相关");
        assert_eq!(none.total, 0);
        assert_eq!(none.best_index, None);
    }

    #[test]
    fn turn_folding_matches_conversation_map_shapes() {
        let items = vec![
            HistoryItem::UserMessage {
                message_id: "user:r1".to_string(),
                seq: 1,
                committed_at_unix_ms: 1,
                text: "问一".to_string(),
                run: lingxi_protocol::history::RunAssociation::known("r1"),
                legacy_fields: serde_json::Map::new(),
            },
            HistoryItem::ToolCall {
                tool_call_id: "r1-tc0001".to_string(),
                run: lingxi_protocol::history::RunAssociation::known("r1"),
                target: "read".to_string(),
                args_summary: None,
                args_digest: None,
                status: lingxi_protocol::history::HistoryToolStatus::Success,
                result: None,
                first_seq: 2,
            },
            HistoryItem::FinalMessage {
                message_id: "r1-final".to_string(),
                seq: 3,
                run: lingxi_protocol::history::RunAssociation::known("r1"),
                model_call_id: None,
                content: vec![ContentBlock::Text {
                    text: "答一".to_string(),
                }],
                committed_at_unix_ms: 3,
                legacy_fields: serde_json::Map::new(),
            },
        ];
        let turns = fold_history_items_into_turns(&items);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0]["question"], json!("问一"));
        assert_eq!(turns[0]["answer"], json!("答一"));
        assert_eq!(turns[0]["processCount"], json!(1));
        assert_eq!(
            turns[0]["entryIds"],
            json!(["user:r1", "r1-tc0001", "r1-final"])
        );
        assert_eq!(turns[0]["truncated"], json!(false));
    }

    #[test]
    fn steer_and_turn_tag_prefix_stripping() {
        assert_eq!(strip_steer_prefix("（插话，无需 MOOD）\n正文"), "正文");
        assert_eq!(strip_steer_prefix("<t>9-30 10:00</t> 正文"), "正文");
        assert_eq!(strip_steer_prefix("普通正文"), "普通正文");
    }
}
