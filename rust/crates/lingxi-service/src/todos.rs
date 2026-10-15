//! 当前 todo_write 已接正式工具执行口并复用本模块。activeForm/blockedReason
//! 是可选扩展：旧记录缺席时保持旧版本值，新完整清单纳入版本计算。
//! R06 G 块「计划/待办面板」后端——现役 `server/routes/sessions.ts`
//! todos 三路由（complete :1852 / cancel :1920 / dismiss :1987）的 Rust 端口，
//! 另含面板读。todo_write 的运行接线见 interactive_tools；slash 由独立入口负责。
//!
//! ## 端口语义对照（逐条对齐 sessions.ts；偏离处均给出理由）
//!
//! - **存储**：待办状态以会话日志条目（entry_type=`todo-state-v1`）追加，
//!   快照 `{version, todos:[{id,content,status}], removed, action}`。
//!   现役用自定义消息 `lingxi.todo_state` 携带
//!   `{action, source, todoVersion, removed, dismissed, todos}`；本端口按
//!   任务规格采用新条目类型与精简条目模型（`id` 为端口新增字段，不参与
//!   版本哈希——现役条目无 id）。两侧互不读对方的条目。
//! - **version**：现役 `computeTodoListVersion`（lib/tools/todo-compat.ts:115）
//!   对规范化条目 `[content, activeForm, status, blockedReason]` 的
//!   JSON 串做 FNV-1a（按 UTF-16 码元）得 `tv%08x`。本端口条目无
//!   activeForm/blockedReason，按缺席补空串同口径计算；读路径不信任
//!   存储的 version，一律由 todos 重算（现役同款）。
//! - **闸序**（complete/cancel/dismiss 共用）：
//!   body 400 → 归属（404/403）→ 助手已删除 409（现役
//!   rejectDeletedAgentSession 原样 `{error,reason:"agent_deleted"}`）→
//!   会话须活动（现役 `isActiveDesktopSessionPath` 403 的 Rust 侧映射：
//!   归档 → 409 `session_archived`，沿用本仓库 authorized-folders 口径）→
//!   **流式中 409（仅 complete/cancel；现役 dismiss 无流式闸——sessions.ts
//!   dismiss 路由不含 isSessionStreaming 检查，端口如实保留）** →
//!   变更串行（会话准入闸；现役由会话管理器文件锁承担，Rust 侧复用
//!   session_admin 的准入模式，撞车 → 409 session_busy）→
//!   **版本不匹配 409 `{error, code:"todo_version_mismatch"}`**（A17：
//!   客户端带了非空 version 且与当前快照不同才拒；不带版本沿用旧行为）。
//! - **complete**：仅「未收纳（removed）且有未完成项」时追加
//!   `complete_all` 记录（未完成→completed，已取消保持取消——现役
//!   completeTodoItems :92）；否则返回当前面板不重复写。
//! - **cancel**：同上，`cancel_remaining`（非终态→cancelled，终态保持——
//!   现役 cancelTodoItems :99）。
//! - **dismiss**：仅「已结束（finished）且未收纳」追加 `dismiss` 收纳记录
//!   （removed:true，todos 原样；历史保留，只改展示）；响应面板恒为
//!   `{removed:true, dismissed:true, todos:[]}`（现役 :2037 无条件返回）。
//! - **广播**：三路由在响应前广播 app 事件 `todos-changed`（现役每次都
//!   `emitEvent({type:"todo_update",...panel})` :1913/:1980/:2038——含
//!   幂等 no-op 分支；端口把会话流事件映射为 appevents 的全局事件，
//!   payload 携带 `{sessionId, agentId, panel}`，agentId 为空时按
//!   skills-changed 的空值先例置 null）。
//! - **面板负载**：现役 todoPanelPayloadFromSnapshot（todo-compat.ts:169）：
//!   removed/收纳/空清单 → `{removed:true, dismissed, todos:[]}`（无
//!   version 键）；否则 `{todos, version, finished, allCompleted,
//!   dismissed:false}`。
//! - **鉴权分类**：现役写路由 `sessions.write`、读走 `sessions.read`；Rust
//!   端点权限表按 session_admin 同类面统一 `Scope("chat")` + 逐请求归属
//!   闸（classify_route 的注册片段见模块尾部注释，由主控落 lib.rs）。
//! - **响应**：`{ok:true, panel}`（现役同款）。
//!
//! ## 集成约束（本文件未注册进 lib.rs 时的编译方式）
//!
//! 生产注册（`pub mod todos;` + 路由）由主控统一完成；在此之前测试以
//! `#[path = "../src/todos.rs"] mod todos;` 直接编译本文件（同
//! embedding_operation.rs 的隔离先例）。因此本文件**只引用
//! `crate::`/`lingxi_adapters::`/`lingxi_kernel::` 的公开面**，
//! 不使用 `crate::` 私有路径。

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{EndpointError, Principal, ServiceState, SessionAccess, SessionConcurrencyLimits};
use lingxi_adapters::storage::session_tree::AppendBranchEntry;

/// 会话日志条目类型（端口自有；现役自定义消息类型为
/// `lingxi.todo_state`，两侧互不读取对方条目）。
pub const TODO_STATE_ENTRY_TYPE: &str = "todo-state-v1";

/// 全部合法 todo 状态（现役 TODO_STATUSES）。
pub const TODO_STATUSES: [&str; 5] = [
    "pending",
    "in_progress",
    "blocked",
    "cancelled",
    "completed",
];

/// 终态集合：完成或取消都算"已结束"（现役 TERMINAL_TODO_STATUSES）。
fn is_terminal_status(status: &str) -> bool {
    matches!(status, "cancelled" | "completed")
}

const ACTION_COMPLETE_ALL: &str = "complete_all";
const ACTION_CANCEL_REMAINING: &str = "cancel_remaining";
const ACTION_DISMISS: &str = "dismiss";

/// app 事件名（现役 todo_update 会话流事件的 appevents 映射）。
pub const TODOS_CHANGED_EVENT: &str = "todos-changed";

// ── 条目模型 ───────────────────────────────────────────────────────────────

/// 单条待办；可选进行中文本和受阻原因随正式完整清单持久化。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: String,
    #[serde(default, rename = "activeForm", skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    #[serde(default, rename = "blockedReason", skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

/// todo-state-v1 条目载荷：`{version, todos, removed, action}`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TodoStateEntry {
    pub version: String,
    pub todos: Vec<TodoItem>,
    pub removed: bool,
    pub action: String,
}

/// 面板快照视图（从条目推导；对照现役 TodoSnapshot 的展示标志集）。
#[derive(Clone, Debug, PartialEq)]
pub struct TodoSnapshot {
    /// 有效清单（removed 时为空）。
    pub todos: Vec<TodoItem>,
    pub removed: bool,
    pub dismissed: bool,
    pub finished: bool,
    pub all_completed: bool,
    /// 由 todos 重算（读路径不信任存储 version——现役同款）。
    pub version: String,
}

impl TodoSnapshot {
    /// 空快照（无任何 todo 条目时的面板状态：removed、非收纳）。
    fn empty() -> Self {
        Self {
            todos: Vec::new(),
            removed: true,
            dismissed: false,
            finished: false,
            all_completed: false,
            version: compute_todo_list_version(&[]),
        }
    }

    /// 从存储条目推导快照。非法状态的条目按现役 migrateLegacyTodos 的
    /// 口径**丢弃**（不回填、不整体拒绝）；removed 显式为真或清单为空
    /// 均视为已收纳/清空（现役 v2：`todos.length===0 || dismissed`）。
    fn from_entry(entry: TodoStateEntry) -> Self {
        // 现役 v2 记录 dismissed 与 removed 同写（dismiss 条目
        // removed:true, dismissed:true）；端口条目以 action=="dismiss"
        // 且 removed 识别收纳。
        let todos: Vec<TodoItem> = entry
            .todos
            .into_iter()
            .filter(|item| TODO_STATUSES.contains(&item.status.as_str()))
            .collect();
        let removed = entry.removed || todos.is_empty();
        let dismissed = removed && entry.action == ACTION_DISMISS;
        let finished =
            !removed && !todos.is_empty() && todos.iter().all(|t| is_terminal_status(&t.status));
        let all_completed = finished && todos.iter().all(|t| t.status == "completed");
        Self {
            version: compute_todo_list_version(&todos),
            todos,
            removed,
            dismissed,
            finished,
            all_completed,
        }
    }
}

/// 清单版本（现役 computeTodoListVersion 的逐口径端口）：对规范化条目
/// `[content, activeForm, status, blockedReason]` 的 JSON 串做 32 位
/// FNV-1a（按 UTF-16 码元推进，`>>> 0` 对应 u32 环绕），输出
/// `tv` + 小写十六进制 8 位补零。本端口条目无 activeForm/blockedReason，
/// 按缺席补空串；`id` 不参与哈希（现役条目无 id）。
pub fn compute_todo_list_version(todos: &[TodoItem]) -> String {
    let canonical: Vec<[&str; 4]> = todos
        .iter()
        .map(|item| [item.content.as_str(), item.active_form.as_deref().unwrap_or(""), item.status.as_str(), item.blocked_reason.as_deref().unwrap_or("")])
        .collect();
    let Some(text) = serde_json::to_string(&canonical).ok() else {
        // 序列化 [String;4] 数组不会失败；若失败，退化为确定性的空串，
        // 保证同输入恒同版本（不引入随机性）。
        return "tv00000000".to_string();
    };
    let mut hash: u32 = 0x811c_9dc5;
    for unit in text.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_add(
            (hash << 1)
                .wrapping_add(hash << 4)
                .wrapping_add(hash << 7)
                .wrapping_add(hash << 8)
                .wrapping_add(hash << 24),
        );
    }
    format!("tv{hash:08x}")
}

/// 是否存在未完成项（现役 hasUnfinishedTodoItems :109：非终态即未完成）。
fn has_unfinished_todo_items(todos: &[TodoItem]) -> bool {
    todos.iter().any(|item| !is_terminal_status(&item.status))
}

/// 确认剩余任务已完成（现役 completeTodoItems :92）：未完成项改
/// completed，已取消项保持取消（不被改写）。
fn complete_todo_items(todos: Vec<TodoItem>) -> Vec<TodoItem> {
    todos
        .into_iter()
        .map(|mut item| {
            if item.status != "cancelled" {
                item.status = "completed".to_string();
            }
            item
        })
        .collect()
}

/// 取消剩余任务（现役 cancelTodoItems :99）：非终态项改 cancelled，
/// 终态项保持。
fn cancel_todo_items(todos: Vec<TodoItem>) -> Vec<TodoItem> {
    todos
        .into_iter()
        .map(|mut item| {
            if !is_terminal_status(&item.status) {
                item.status = "cancelled".to_string();
            }
            item
        })
        .collect()
}

/// 面板负载（现役 todoPanelPayloadFromSnapshot :169）：快照缺失、已移除
/// 或已收纳 → removed 负载（**无 version 键**）；否则完整负载。
pub(crate) fn panel_from_snapshot(snapshot: Option<&TodoSnapshot>) -> Value {
    let Some(snapshot) = snapshot else {
        return json!({"removed": true, "dismissed": false, "todos": []});
    };
    if snapshot.removed || snapshot.todos.is_empty() {
        return json!({"removed": true, "dismissed": snapshot.dismissed, "todos": []});
    }
    let todos = serde_json::to_value(&snapshot.todos).unwrap_or_else(|_| json!([]));
    json!({
        "todos": todos,
        "version": snapshot.version,
        "finished": snapshot.finished,
        "allCompleted": snapshot.all_completed,
        "dismissed": false,
    })
}

// ── 存储读写 ───────────────────────────────────────────────────────────────

/// 读取当前分支上最新的合法 todo 快照（新→旧逐条解析，坏载荷跳过
/// 继续向前——现役 extractLatestTodoSnapshot 的口径）。无条目 → None。
#[allow(clippy::result_large_err)]
pub(crate) async fn latest_todo_snapshot(
    state: &ServiceState,
    session_id: &str,
) -> Result<Option<TodoSnapshot>, Response> {
    let rows = lingxi_adapters::storage::canonical_message_store::read_branch_entries_by_type(
        state.storage(),
        session_id,
        TODO_STATE_ENTRY_TYPE,
    )
    .await
    .map_err(|error| EndpointError::storage(&error).into_response())?;
    for row in rows {
        if let Ok(entry) = serde_json::from_str::<TodoStateEntry>(&row.content_json) {
            return Ok(Some(TodoSnapshot::from_entry(entry)));
        }
        // 坏快照：日志可见、继续向前找更旧的合法快照（不静默吞）。
        tracing::warn!(
            session = session_id,
            entry = %row.message_id,
            "skipping malformed todo-state entry"
        );
    }
    Ok(None)
}

/// 追加一条 todo-state-v1 条目（当前分支头，事务内铸 id）。run/工具侧
/// 写入清单（排除项 B34F2B298BFA）与本模块路由共用此入口。
#[allow(clippy::result_large_err)]
pub async fn append_state_entry(
    state: &ServiceState,
    session_id: &str,
    entry: &TodoStateEntry,
    now_unix_ms: u64,
) -> Result<String, Response> {
    let content = serde_json::to_string(entry).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("cannot encode todo state entry: {error}")})),
        )
            .into_response()
    })?;
    lingxi_adapters::storage::session_tree::append_branch_entry(
        state.storage(),
        AppendBranchEntry {
            session_id,
            message_id: None,
            entry_type: TODO_STATE_ENTRY_TYPE,
            role: "custom",
            content_json: &content,
            now_unix_ms,
        },
    )
    .await
    .map_err(|error| EndpointError::storage(&error).into_response())
}

// ── 闸序 ───────────────────────────────────────────────────────────────────

/// 归属闸（现役 authorizeSessionRoute + fs.access 的 Rust 侧合并口径：
/// get_for 同时回答存在与归属）。
#[allow(clippy::result_large_err)]
async fn gate_session_owned(
    state: &ServiceState,
    principal: &Principal,
    session_id: &str,
) -> Result<crate::SessionFacts, Response> {
    match state.sessions().get_for(principal, session_id).await {
        Ok(SessionAccess::Ok(facts)) => Ok(facts),
        Ok(SessionAccess::NotFound) => Err(EndpointError::not_found().into_response()),
        Ok(SessionAccess::Forbidden) => {
            Err(EndpointError::forbidden("cross_principal_access").into_response())
        }
        Err(error) => Err(EndpointError::storage(&error).into_response()),
    }
}

/// 助手已删除 → 409（现役 rejectDeletedAgentSession 原样）；
/// 会话已归档 → 409 session_archived（现役 isActiveDesktopSessionPath 的
/// 归档侧映射，沿用 authorized-folders 端口口径）。
#[allow(clippy::result_large_err)]
async fn gate_session_active(
    state: &ServiceState,
    facts: &crate::SessionFacts,
) -> Result<(), Response> {
    match state.memory().is_agent_deleted_checked(&facts.agent_id) {
        Ok(true) => {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "agent_deleted", "reason": "agent_deleted"})),
            )
                .into_response())
        }
        Ok(false) => {}
        Err(error) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("agent deleted check failed: {error:?}")})),
            )
                .into_response())
        }
    }
    let row =
        lingxi_adapters::storage::session_tree::get_session_row(state.storage(), &facts.session_id)
            .await
            .map_err(|error| EndpointError::storage(&error).into_response())?;
    let Some(row) = row else {
        return Err(EndpointError::not_found().into_response());
    };
    if row.archived_at_unix_ms.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "session is archived", "reason": "session_archived"})),
        )
            .into_response());
    }
    Ok(())
}

// ── 路由 ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TodoActionBody {
    session_id: String,
    /// 客户端所见清单版本（A17）：非空且与当前快照不同 → 409。
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum TodoAction {
    Complete,
    Cancel,
    Dismiss,
}

/// `POST /lingxi/v1/sessions/todos/complete`（现役 :1852）。
pub async fn complete_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<TodoActionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    run_todo_action(principal, state, body, TodoAction::Complete).await
}

/// `POST /lingxi/v1/sessions/todos/cancel`（现役 :1920）。
pub async fn cancel_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<TodoActionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    run_todo_action(principal, state, body, TodoAction::Cancel).await
}

/// `POST /lingxi/v1/sessions/todos/dismiss`（现役 :1987）。
pub async fn dismiss_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<TodoActionBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    run_todo_action(principal, state, body, TodoAction::Dismiss).await
}

/// 三动作共用闸序与响应（对照现役三路由的逐段同构实现）。
async fn run_todo_action(
    principal: Principal,
    state: ServiceState,
    body: Result<Json<TodoActionBody>, axum::extract::rejection::JsonRejection>,
    action: TodoAction,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    let session_id = body.session_id.trim().to_string();
    if session_id.is_empty() {
        return EndpointError::invalid_message("sessionId is required").into_response();
    }
    // 归属 → 活动会话（助手未删除 / 未归档）。
    let facts = match gate_session_owned(&state, &principal, &session_id).await {
        Ok(facts) => facts,
        Err(response) => return response,
    };
    if let Err(response) = gate_session_active(&state, &facts).await {
        return response;
    }
    // 流式中 409（仅 complete/cancel；现役 dismiss 路由无此闸）。
    if matches!(action, TodoAction::Complete | TodoAction::Cancel)
        && state.sessions().session_supervisor().is_busy(&session_id)
    {
        let message = match action {
            TodoAction::Complete => "Cannot complete todos while session is streaming",
            _ => "Cannot cancel todos while session is streaming",
        };
        return (StatusCode::CONFLICT, Json(json!({"error": message}))).into_response();
    }
    // 变更串行：会话准入闸（现役由会话管理器串行承担；Rust 侧沿用
    // session_admin 的准入模式，撞车 409 session_busy，客户端重试）。
    // **仅 complete/cancel**：dismiss 现役无流式闸、收纳须在流式中可用，
    // 而运行租约持有的是同一准入面（supervisor 的 run 许可与存储准入
    // 同一登记），dismiss 若取操作权会在流式中被误判 Busy；其追加本身
    // 经存储单写队列原子化，无需额外操作权。
    let _permit = if matches!(action, TodoAction::Complete | TodoAction::Cancel) {
        match state.storage().session_admission().try_acquire(
            &[session_id.as_str()],
            SessionConcurrencyLimits::DEFAULT_REGISTRY_CAP,
        ) {
            Ok(permit) => Some(permit),
            Err(_) => {
                return EndpointError::session_busy().into_response();
            }
        }
    } else {
        None
    };
    // 当前快照。
    let snapshot = match latest_todo_snapshot(&state, &session_id).await {
        Ok(snapshot) => snapshot.unwrap_or_else(TodoSnapshot::empty),
        Err(response) => return response,
    };
    // 版本失配（A17）：客户端带了非空版本且与当前快照不同 → 拒绝。
    if let Some(client_version) = body
        .version
        .as_deref()
        .filter(|version| !version.is_empty())
    {
        if snapshot.version != client_version {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "The todo list was updated elsewhere. Refresh and try again.",
                    "code": "todo_version_mismatch",
                })),
            )
                .into_response();
        }
    }
    // 动作主体。
    let panel = match action {
        TodoAction::Complete | TodoAction::Cancel => {
            // 仅「未收纳且有未完成项」时追加记录；否则返回当前面板不重复写。
            if !snapshot.removed && has_unfinished_todo_items(&snapshot.todos) {
                let todos = match action {
                    TodoAction::Complete => complete_todo_items(snapshot.todos.clone()),
                    _ => cancel_todo_items(snapshot.todos.clone()),
                };
                let entry = TodoStateEntry {
                    version: compute_todo_list_version(&todos),
                    todos: todos.clone(),
                    removed: false,
                    action: match action {
                        TodoAction::Complete => ACTION_COMPLETE_ALL.to_string(),
                        _ => ACTION_CANCEL_REMAINING.to_string(),
                    },
                };
                if let Err(response) =
                    append_state_entry(&state, &session_id, &entry, state.clock().now_unix_ms())
                        .await
                {
                    return response;
                }
                panel_after_user_action(todos)
            } else {
                panel_from_snapshot(Some(&snapshot))
            }
        }
        TodoAction::Dismiss => {
            // 只收纳"已结束且未收纳"的清单；其他情况是幂等 no-op。
            // 收纳只改变当前展示，不改完成/取消结果；历史记录保留。
            if snapshot.finished && !snapshot.dismissed && !snapshot.removed {
                let entry = TodoStateEntry {
                    version: snapshot.version.clone(),
                    todos: snapshot.todos.clone(),
                    removed: true,
                    action: ACTION_DISMISS.to_string(),
                };
                if let Err(response) =
                    append_state_entry(&state, &session_id, &entry, state.clock().now_unix_ms())
                        .await
                {
                    return response;
                }
            }
            json!({"removed": true, "dismissed": true, "todos": []})
        }
    };
    // 广播（现役每次响应前都发 todo_update，含幂等 no-op 分支）。
    publish_todos_changed(&state, &facts, &session_id, &panel);
    Json(json!({"ok": true, "panel": panel})).into_response()
}

/// 用户收尾操作（完成/取消）后的面板（现役
/// resolveUserActionSnapshotFlags :119 + todoPanelPayloadFromSnapshot：
/// 全部终态 → finished；removed/dismissed 均为 false，保留收尾摘要）。
fn panel_after_user_action(todos: Vec<TodoItem>) -> Value {
    let finished = !todos.is_empty() && todos.iter().all(|t| is_terminal_status(&t.status));
    let all_completed = finished && todos.iter().all(|t| t.status == "completed");
    let snapshot = TodoSnapshot {
        version: compute_todo_list_version(&todos),
        todos,
        removed: false,
        dismissed: false,
        finished,
        all_completed,
    };
    panel_from_snapshot(Some(&snapshot))
}

fn publish_todos_changed(
    state: &ServiceState,
    facts: &crate::SessionFacts,
    session_id: &str,
    panel: &Value,
) {
    let agent_id = if facts.agent_id.is_empty() {
        Value::Null
    } else {
        json!(facts.agent_id)
    };
    state.app_events().publish(
        TODOS_CHANGED_EVENT,
        json!({
            "sessionId": session_id,
            "agentId": agent_id,
            "panel": panel,
        }),
    );
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TodoPanelQuery {
    session_id: String,
}

/// `GET /lingxi/v1/sessions/todos?sessionId=…` — 面板读（现役无独立 GET，
/// 面板经会话流事件/历史恢复；端口按任务规格补只读面，语义 = 当前
/// 快照的面板负载，闸序 = 归属 → 活动会话）。
pub async fn panel_route(
    Extension(principal): Extension<Principal>,
    State(state): State<ServiceState>,
    query: Result<Query<TodoPanelQuery>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(rejection) => {
            return EndpointError::invalid_message(format!("invalid todos query: {rejection}"))
                .into_response()
        }
    };
    let session_id = query.session_id.trim().to_string();
    if session_id.is_empty() {
        return EndpointError::invalid_message("sessionId is required").into_response();
    }
    let facts = match gate_session_owned(&state, &principal, &session_id).await {
        Ok(facts) => facts,
        Err(response) => return response,
    };
    if let Err(response) = gate_session_active(&state, &facts).await {
        return response;
    }
    let snapshot = match latest_todo_snapshot(&state, &session_id).await {
        Ok(snapshot) => snapshot,
        Err(response) => return response,
    };
    Json(json!({"ok": true, "panel": panel_from_snapshot(snapshot.as_ref())})).into_response()
}

// ── 留给主控的 lib.rs 注册片段（本文件不注册；测试内自行挂载）────────────
//
// ```text
// pub mod todos;  // lib.rs 模块声明区
//
// // 路由（lib.rs 的 Router 构建处，与 sessions 面同段）：
// .route("/lingxi/v1/sessions/todos", get(todos::panel_route))
// .route("/lingxi/v1/sessions/todos/complete", post(todos::complete_route))
// .route("/lingxi/v1/sessions/todos/cancel", post(todos::cancel_route))
// .route("/lingxi/v1/sessions/todos/dismiss", post(todos::dismiss_route))
//
// // 鉴权分类（auth.rs classify_route，/lingxi/v1/sessions/ 前缀段内）：
// if m == "POST" && matches!(rest, "todos/complete" | "todos/cancel" | "todos/dismiss") {
//     return RoutePolicy::Scope("chat");
// }
// if rest == "todos" && (m == "GET" || m == "HEAD") {
//     return RoutePolicy::Scope("chat");
// }
// ```
