//! 现役 server/routes/desk.ts 的工作台文件面（R07-β3）：
//! GET /lingxi/v1/desk/path、GET /lingxi/v1/desk/files、
//! GET /lingxi/v1/desk/search-files、GET|POST /lingxi/v1/desk/jian、
//! POST /lingxi/v1/desk/files（upload/create/mkdir/rename/move/movePaths/
//! remove）。
//!
//! 与现役对齐的关键语义：
//! - 批准目录门禁（desk.ts isApprovedDir + engine.ts isApprovedDeskDir
//!   :4924-4938）：根 = 当前助手 home_folder（缺失即弃，不惰性改写配置—
//!   —现役的 updateConfig 惰性清洗由候选人 GC 路径
//!   prune_missing_workspace_config 承担，登记偏差）→ 默认工作区根；
//!   焦点会话 cwd → 焦点会话 workspace_folders → 顶层 cwd_history。
//!   根与目标都按 _realPathForWorkspaceCheck（realpath，失败回退父目录
//!   realpath）解析；selectedAgentDeskRoots 一段按严格 realpath
//!   （desk.ts isInsideAnyRoot :53-58）。
//! - 三种错误面：守卫字符串 {error: string} HTTP 200（dirNotAllowed /
//!   invalid subdir / invalid path / noWorkspace）；deskRouteError
//!   {error:{code,message}} 显式状态；服务错误 {error: 映射后消息,
//!   code?} 状态 err.status||400。
//! - 查询值单次解码：现役在 Hono 已解码的 query 上再跑
//!   decodeURIComponent（双解码）；候选人取 axum 单次解码（常规路径输
//!   入语义等价，登记偏差）。
//! - POST /desk/files 的 upload 仅 local owner（403
//!   "upload by absolute path requires local owner"），逐项
//!   isSensitivePath 筛查。

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{
    resourceio::OpContext,
    workbench::{
        self, action_mkdir, action_move, action_move_paths, action_rename, action_safe_delete,
        action_write_text, copy_local_path_into_directory, files_for_directory, list_files,
        read_text, safe_delete_if_exists, search_files, WbError, WORKSPACE_SEARCH_LIMIT,
    },
    Principal, ServiceState,
};

// ── 路径安全（shared/path-security.ts + desk.ts 顶部助手）──

/// 现役 isInsidePath（desk.ts :41-51）：target 必须在 baseDir 内部
/// （realpath 后比较）；target 缺失时解析父目录 + 保留 basename。
fn is_inside_path(target: &Path, base_dir: &Path) -> bool {
    let Some(base) = workbench::real_path(base_dir) else {
        return false;
    };
    if let Some(resolved) = workbench::real_path(target) {
        return resolved == base || resolved.starts_with(base.join(""));
    }
    let Some(parent) = target.parent() else {
        return false;
    };
    let Some(parent_resolved) = workbench::real_path(parent) else {
        return false;
    };
    let Some(name) = target.file_name() else {
        return false;
    };
    let full = parent_resolved.join(name);
    full == base || full.starts_with(base.join(""))
}

/// 现役 isInsideAnyRoot（desk.ts :53-58）：目标与根都严格 realpath
/// （无父目录回退）。
pub(crate) fn is_inside_any_root(dir: &Path, roots: &[PathBuf]) -> bool {
    let Some(resolved) = workbench::real_path(dir) else {
        return false;
    };
    roots.iter().any(|root| match workbench::real_path(root) {
        Some(base) => resolved == base || resolved.starts_with(base.join("")),
        None => false,
    })
}

/// 现役 _realPathForWorkspaceCheck（engine.ts :4896-4907）：realpath，
/// 失败回退父目录 realpath，再失败 None。目标与根都用它——缺失根但父
/// 目录存在时父目录被当作根（现役即此行为，UX 准入检查而非沙箱边界）。
pub(crate) fn real_path_for_workspace_check(p: &Path) -> Option<PathBuf> {
    workbench::real_path(p).or_else(|| p.parent().and_then(workbench::real_path))
}

/// 现役 isSensitivePath（shared/path-security.ts :18-31）：非绝对 / 无法
/// 解析 → 敏感；~/.ssh、~/.gnupg、~/.aws、~/.config/gcloud、~/.kube 之下
/// → 敏感；realpath(lingxiHome) 之下 → 敏感。敏感目录本身按字符串比较
/// （现役不 realpath 它们）。
fn is_sensitive_path(value: &str, lingxi_home: &Path) -> bool {
    let path = Path::new(value);
    if !path.is_absolute() {
        return true;
    }
    let Some(resolved) = workbench::real_path(path) else {
        return true;
    };
    if let Some(home) = std::env::home_dir() {
        for directory in [".ssh", ".gnupg", ".aws", ".config/gcloud", ".kube"] {
            let sensitive = home.join(directory);
            if resolved == sensitive || resolved.starts_with(sensitive.join("")) {
                return true;
            }
        }
    }
    if let Some(real_home) = workbench::real_path(lingxi_home) {
        if resolved == real_home || resolved.starts_with(real_home.join("")) {
            return true;
        }
    }
    false
}

/// 现役 normalizeWorkspacePath（shared/workspace-history.ts :3-11）：
/// trim + \→/ + 剥尾斜杠（根与盘符例外）。
fn normalize_workspace_path(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let slashed = trimmed.replace('\\', "/");
    if slashed == "/" {
        return Some(slashed);
    }
    // /^[A-Za-z]:\/?$/ —— Windows 盘符根保留一个尾斜杠。
    let bytes = slashed.as_bytes();
    if (slashed.len() == 2 || (slashed.len() == 3 && slashed.ends_with('/')))
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (slashed.len() == 2 || bytes[2] == b'/')
    {
        return Some(if slashed.len() == 2 {
            format!("{slashed}/")
        } else {
            slashed
        });
    }
    let stripped = slashed.trim_end_matches('/');
    Some(if stripped.is_empty() {
        "/".to_string()
    } else {
        stripped.to_string()
    })
}

/// 现役 classifyWorkspacePathForGc（shared/workspace-persistence-gc.ts
/// :8-21）在 home_folder 解析上的应用：present/unknown → 保留；missing
/// （ENOENT/ENOTDIR）/invalid → 弃用。现役 missing 时惰性清洗配置——候
/// 选人不在读路径改写配置（登记偏差；GC 由
/// prune_missing_workspace_config 承担）。
fn existing_workspace_path(value: Option<String>) -> Option<PathBuf> {
    let normalized = normalize_workspace_path(value.as_deref()?.trim())?;
    let path = PathBuf::from(&normalized);
    match std::fs::metadata(&path) {
        Ok(_) => Some(path),
        Err(err) => match err.kind() {
            std::io::ErrorKind::NotFound => None,
            // ENOTDIR 在 std 里没有独立 kind；NotFound 之外的错误一律按
            // 现役 unknown 保留。
            _ => {
                if err.raw_os_error() == Some(20) {
                    None
                } else {
                    Some(path)
                }
            }
        },
    }
}

// ── 工作台根解析（config-coordinator + engine 门禁）──

/// 当前助手解析：current_agent_id → primary_agent_id（agents_admin
/// current_agent_preference）。
pub(crate) fn current_agent_id(state: &ServiceState) -> Option<String> {
    state.memory().current_agent_preference().ok().flatten()
}

/// 指定助手 config.yaml 的 desk 段（home_folder / cwd_history）。配置读
/// 取或解析失败按缺失处理（门禁回落到其余根）。
pub(crate) fn agent_desk_config(
    state: &ServiceState,
    agent_id: &str,
) -> (Option<String>, Vec<String>) {
    let dir = match state.memory().agent_dir_checked(agent_id) {
        Ok(dir) => dir,
        Err(_) => return (None, Vec::new()),
    };
    let text = match std::fs::read_to_string(dir.join("config.yaml")) {
        Ok(text) => text,
        Err(_) => return (None, Vec::new()),
    };
    let document =
        match lingxi_adapters::storage::agent_memory::config::AgentConfigDocument::parse(&text) {
            Ok(document) => document,
            Err(_) => return (None, Vec::new()),
        };
    let home = document.string("desk", "home_folder").ok().flatten();
    // γ 修复：现役 cwd_history 在 config.yaml 顶层（config.ts:233 /
    // sessions.ts:2114 / agents.ts:326 写顶层；engine.ts:4931 读顶层），
    // β3 误读 desk.cwd_history。
    let history = document
        .string_list("", "cwd_history")
        .ok()
        .flatten()
        .unwrap_or_default();
    (home, history)
}

/// 现役 getExplicitHomeFolder（config-coordinator.ts :151-164）。
fn explicit_home_folder(state: &ServiceState, agent_id: &str) -> Option<PathBuf> {
    let (home, _) = agent_desk_config(state, agent_id);
    existing_workspace_path(home)
}

/// 现役 getHomeFolder（:166-176）：显式 → 否则默认工作区（候选人的默认
/// 工作区 = 服务 workspace_root，对应 resolveDefaultWorkspacePath 的
/// 桌面默认 OH-WorkSpace）。
pub(crate) fn home_cwd(state: &ServiceState, agent_id: Option<&str>) -> Option<PathBuf> {
    if let Some(agent_id) = agent_id {
        if let Some(explicit) = explicit_home_folder(state, agent_id) {
            return Some(explicit);
        }
    }
    state.resource_io().workspace_root()
}

/// 现役 deskCwd（engine.ts :1930）：焦点会话 cwd → 否则 homeCwd。
pub(crate) fn desk_cwd(state: &ServiceState, agent_id: Option<&str>) -> Option<PathBuf> {
    if let Some(session) = current_session_id(state) {
        if let Ok(Some(meta)) = crate::session_ui::read(state.storage(), &session) {
            if let Some(cwd) = meta.cwd.filter(|cwd| !cwd.trim().is_empty()) {
                return Some(PathBuf::from(cwd));
            }
        }
    }
    home_cwd(state, agent_id)
}

fn current_session_id(state: &ServiceState) -> Option<String> {
    state
        .memory()
        .read_preferences_root()
        .ok()
        .and_then(|prefs| {
            prefs
                .get("current_session_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|id| !id.is_empty())
        })
}

/// 现役 isApprovedDeskDir 的根列表（engine.ts :4924-4938）：homeCwd、
/// deskCwd、焦点会话 workspace_folders、当前助手顶层 cwd_history
/// （engine.ts :4931 读 config.cwd_history 顶层键）。
pub(crate) fn approved_desk_roots(state: &ServiceState) -> Vec<PathBuf> {
    let agent = current_agent_id(state);
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = home_cwd(state, agent.as_deref()) {
        roots.push(home);
    }
    if let Some(desk) = desk_cwd(state, agent.as_deref()) {
        roots.push(desk);
    }
    if let Some(session) = current_session_id(state) {
        if let Ok(Some(meta)) = crate::session_ui::read(state.storage(), &session) {
            roots.extend(meta.workspace_folders.into_iter().map(PathBuf::from));
        }
    }
    if let Some(agent) = agent {
        let (_, history) = agent_desk_config(state, &agent);
        roots.extend(history.into_iter().map(PathBuf::from));
    }
    roots
}

/// 现役 selectedAgentDeskRoots（desk.ts :63-70）：指定 agentId 的
/// explicit + effective home。
pub(crate) fn selected_agent_desk_roots(state: &ServiceState, agent_id: &str) -> Vec<PathBuf> {
    let explicit = explicit_home_folder(state, agent_id);
    let home = home_cwd(state, Some(agent_id));
    [explicit, home].into_iter().flatten().collect()
}

/// 现役 isApprovedDir（desk.ts :72-88）：先 selectedAgentDeskRoots（严格
/// realpath），再 isApprovedDeskDir（父目录回退解析）。现役的
/// isApprovedWorkspaceDir 早退分支在 isApprovedDeskDir 存在时是死代码
/// （:79-86），候选人只保留生效分支。
fn is_approved_dir(state: &ServiceState, dir: &Path, agent_id: Option<&str>) -> bool {
    if let Some(agent_id) = agent_id {
        if is_inside_any_root(dir, &selected_agent_desk_roots(state, agent_id)) {
            return true;
        }
    }
    let Some(resolved) = real_path_for_workspace_check(dir) else {
        return false;
    };
    approved_desk_roots(state)
        .iter()
        .any(|root| match real_path_for_workspace_check(root) {
            Some(base) => resolved == base || resolved.starts_with(base.join("")),
            None => false,
        })
}

/// 现役 defaultDeskDir（desk.ts :90-92）：defaultDeskCwd || homeCwd ||
/// deskCwd——前两者同源（engine.ts :4892-4894），即 homeCwd。
fn default_desk_dir(state: &ServiceState) -> Option<PathBuf> {
    let agent = current_agent_id(state);
    home_cwd(state, agent.as_deref())
}

// ── i18n（error.dirNotAllowed / error.noWorkspace；lib/i18n.ts）──
//
// 现役 t() 从 desktop/src/locales/*.json 读包；候选人不随服务分发桌面
// 语言包，这里内嵌这两个键的五语言文案（现行文案的静态快照，登记偏差：
// 桌面改文案需同步此表）。resolveKey：zh-TW/zh-Hant→zh-TW，zh*→zh，
// ja*→ja，ko*→ko，否则 en；缺省 zh。

pub(crate) fn desk_locale_key(state: &ServiceState) -> &'static str {
    let locale = state
        .memory()
        .read_preferences_root()
        .ok()
        .and_then(|prefs| {
            prefs
                .get("locale")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    if locale.is_empty() {
        return "zh";
    }
    if locale == "zh-TW" || locale == "zh-Hant" {
        return "zh-TW";
    }
    if locale.starts_with("zh") {
        return "zh";
    }
    if locale.starts_with("ja") {
        return "ja";
    }
    if locale.starts_with("ko") {
        return "ko";
    }
    "en"
}

fn t_desk(state: &ServiceState, key: &str) -> &'static str {
    let table = match desk_locale_key(state) {
        "zh-TW" => &[
            ("dirNotAllowed", "目錄不在允許範圍內"),
            ("noWorkspace", "未設定工作台"),
        ][..],
        "ja" => &[
            ("dirNotAllowed", "ディレクトリが許可範囲外です"),
            ("noWorkspace", "ワークスペースが設定されていません"),
        ][..],
        "ko" => &[
            ("dirNotAllowed", "디렉토리가 허용 범위에 없어요"),
            ("noWorkspace", "워크스페이스가 설정되지 않았어요"),
        ][..],
        "en" => &[
            ("dirNotAllowed", "Directory not in allowed scope"),
            ("noWorkspace", "No workspace configured"),
        ][..],
        _ => &[
            ("dirNotAllowed", "目录不在允许范围内"),
            ("noWorkspace", "未设置工作台"),
        ][..],
    };
    table
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, text)| *text)
        // 现役 get() 在语言包缺键时回落英文兜底包（lib/i18n.ts :71-76）。
        .unwrap_or_else(|| {
            if key == "noWorkspace" {
                "No workspace configured"
            } else {
                "Directory not in allowed scope"
            }
        })
}

// ── 错误面（三种形态，见模块头注释）──

/// 守卫字符串：HTTP 200 + {error: string}。
fn desk_guard_error(message: &str) -> Response {
    Json(json!({ "error": message })).into_response()
}

/// 现役 deskRouteError = jsonRouteError（route-errors.ts）：
/// {error: {code, message}} + 显式状态。
fn desk_route_error(code: &str, message: &str, status: u16) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
        .into_response()
}

/// 现役 deskFileActionErrorMessage（desk.ts :138-144）：resource_not_found
/// → "not found"；target_already_exists → "target already exists"；
/// already_exists → "already exists"；其余 → err.message（候选人 WbError
/// detail 即现役 message/safeMessage）。状态 err.status || 400，附 code。
fn desk_service_error(err: &WbError) -> Response {
    let message = match err.code.as_str() {
        "resource_not_found" => "not found".to_string(),
        "target_already_exists" => "target already exists".to_string(),
        "already_exists" => "already exists".to_string(),
        _ => err.detail.clone(),
    };
    let status = StatusCode::from_u16(err.status).unwrap_or(StatusCode::BAD_REQUEST);
    (status, Json(json!({ "error": message, "code": err.code }))).into_response()
}

// ── 根构造（fileServiceForRequest(c, dir)：dir 覆盖为 default 根）──

fn desk_root(dir: &Path) -> Result<workbench::ResolvedRoot, WbError> {
    // 现役 _resolveRootInternal 对 default 根 mkdirSync recursive。
    std::fs::create_dir_all(dir).map_err(|err| {
        WbError::new(
            400,
            "file_action_failed",
            format!("cannot create workspace root: {err}"),
        )
    })?;
    Ok(workbench::ResolvedRoot::default_with_path(
        dir.to_path_buf(),
    ))
}

/// 现役 desk 路由的子目录守卫（substring 级）：含 \ 或 .. 即拒（200
/// "invalid subdir"），再 isInsidePath（200 "invalid path"）。
fn desk_subdir_guard(base: &Path, subdir: &str) -> Option<Response> {
    if !subdir.is_empty() && (subdir.contains('\\') || subdir.contains("..")) {
        return Some(desk_guard_error("invalid subdir"));
    }
    let target = if subdir.is_empty() {
        base.to_path_buf()
    } else {
        base.join(subdir)
    };
    if !is_inside_path(&target, base) {
        return Some(desk_guard_error("invalid path"));
    }
    None
}

/// dir 覆盖未获批准（唯一拒绝形态；调用方渲染 dirNotAllowed 守卫串）。
struct DirNotAllowed;

/// dir 查询/正文参数的解析 + 批准门禁。返回 Ok(Some(dir)) 可用、
/// Ok(None) 无工作台；Err(DirNotAllowed) 由调用方渲染守卫响应。非字符
/// 串非 null 的 dir（现役会 truthy 进门禁且必然不获批）同样拒绝。
fn resolve_desk_dir(
    state: &ServiceState,
    raw_dir: Option<&Value>,
    agent_id: Option<&str>,
) -> Result<Option<PathBuf>, DirNotAllowed> {
    let raw = match raw_dir {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(Value::String(_)) => None,
        Some(_) => return Err(DirNotAllowed),
    };
    let dir = match &raw {
        Some(raw) => PathBuf::from(raw),
        None => match default_desk_dir(state) {
            Some(dir) => dir,
            None => return Ok(None),
        },
    };
    if raw.is_some() && !is_approved_dir(state, &dir, agent_id) {
        return Err(DirNotAllowed);
    }
    Ok(Some(dir))
}

fn agent_id_of(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|id| !id.is_empty())
}

/// desk 面操作上下文：引擎面授权（现役 resourceIOForEngine 的
/// sandbox-off 引擎平面，见 OpContext::engine_plane）；desk 根可位于服
/// 务 workspace_root 之外，决定性控制在路由层门禁。
fn desk_ctx(body: &Value, reason: &str) -> OpContext {
    let mut ctx = workbench::op_ctx(body, reason);
    ctx.engine_plane = true;
    ctx
}

fn desk_read_ctx() -> OpContext {
    let mut ctx = OpContext::local_owner("");
    ctx.engine_plane = true;
    ctx
}

// ── 路由 ──

/// 现役 GET /desk/path（desk.ts :1416-1423）。
async fn desk_path(
    State(state): State<ServiceState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let agent_id = query.get("agentId").cloned();
    let raw_dir = query.get("dir").map(|v| Value::String(v.clone()));
    let dir = match resolve_desk_dir(&state, raw_dir.as_ref(), agent_id.as_deref()) {
        Ok(Some(dir)) => dir,
        Ok(None) => return Json(json!({ "path": Value::Null })).into_response(),
        Err(_) => return desk_guard_error(t_desk(&state, "dirNotAllowed")),
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        // 现役无 try/catch：mkdirSync 抛出落到框架 500。
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("cannot create workspace root: {err}") })),
        )
            .into_response();
    }
    Json(json!({ "path": dir.to_string_lossy() })).into_response()
}

/// 现役 GET /desk/files（desk.ts :1426-1444）。
async fn desk_files_list(
    State(state): State<ServiceState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let agent_id = query.get("agentId").cloned();
    let raw_dir = query.get("dir").map(|v| Value::String(v.clone()));
    let dir = match resolve_desk_dir(&state, raw_dir.as_ref(), agent_id.as_deref()) {
        Ok(Some(dir)) => dir,
        Ok(None) => {
            return Json(json!({ "files": [], "subdir": "", "basePath": Value::Null }))
                .into_response()
        }
        Err(_) => return desk_guard_error(t_desk(&state, "dirNotAllowed")),
    };
    let subdir = query.get("subdir").cloned().unwrap_or_default();
    if let Some(response) = desk_subdir_guard(&dir, &subdir) {
        return response;
    }
    let root = match desk_root(&dir) {
        Ok(root) => root,
        Err(err) => return desk_service_error(&err),
    };
    let ctx = desk_read_ctx();
    match list_files(&state, &root, &subdir, false, &ctx).await {
        Ok(payload) => Json(json!({
            "files": payload["files"].clone(),
            "subdir": subdir,
            "basePath": dir.to_string_lossy(),
        }))
        .into_response(),
        Err(err) => desk_service_error(&err),
    }
}

/// 现役 GET /desk/search-files（desk.ts :1447-1464）。
async fn desk_search_files(
    State(state): State<ServiceState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let agent_id = query.get("agentId").cloned();
    let raw_dir = query.get("dir").map(|v| Value::String(v.clone()));
    let raw_query = query.get("q").cloned().unwrap_or_default();
    let dir = match resolve_desk_dir(&state, raw_dir.as_ref(), agent_id.as_deref()) {
        Ok(Some(dir)) => dir,
        Ok(None) => {
            return Json(json!({
                "results": [],
                "basePath": Value::Null,
                "query": raw_query,
            }))
            .into_response()
        }
        Err(_) => return desk_guard_error(t_desk(&state, "dirNotAllowed")),
    };
    // Number.parseInt：解析失败/非正 → 默认 80；否则 min(limit, 80)。
    let limit = raw_query_limit(query.get("limit"));
    let root = match desk_root(&dir) {
        Ok(root) => root,
        Err(err) => return desk_service_error(&err),
    };
    let ctx = desk_read_ctx();
    // 现役先 listFiles 再 searchFiles（错误顺序一致）。
    if let Err(err) = list_files(&state, &root, "", false, &ctx).await {
        return desk_service_error(&err);
    }
    match search_files(&state, &root, &raw_query, false, &ctx).await {
        Ok(payload) => {
            let results = payload["results"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .take(limit)
                .collect::<Vec<_>>();
            Json(json!({
                "results": results,
                "basePath": dir.to_string_lossy(),
                "query": raw_query,
            }))
            .into_response()
        }
        Err(err) => {
            if err.code == "resource_not_found" {
                return Json(json!({
                    "results": [],
                    "basePath": dir.to_string_lossy(),
                    "query": raw_query,
                }))
                .into_response();
            }
            desk_service_error(&err)
        }
    }
}

/// 现役 Number.parseInt（desk.ts :1451-1454）：前导空白 + 可选符号 +
/// 前导数字前缀；解析失败/非正 → 默认 80；否则 min(limit, 80)。
fn raw_query_limit(raw: Option<&String>) -> usize {
    let parsed = raw.and_then(|value| parse_int_prefix(value));
    match parsed {
        Some(value) if value > 0 => (value as usize).min(WORKSPACE_SEARCH_LIMIT),
        _ => WORKSPACE_SEARCH_LIMIT,
    }
}

fn parse_int_prefix(raw: &str) -> Option<i64> {
    let trimmed = raw.trim_start();
    let (sign, rest) = if let Some(rest) = trimmed.strip_prefix('-') {
        (-1i64, rest)
    } else {
        (1i64, trimmed.strip_prefix('+').unwrap_or(trimmed))
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    // 超 i64 的输入在现役是大数 → min 后 80；候选人解析失败也回落 80。
    digits.parse::<i64>().ok().map(|value| sign * value)
}

/// 现役 GET /desk/jian（desk.ts :1467-1482）。
async fn desk_jian_read(
    State(state): State<ServiceState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let agent_id = query.get("agentId").cloned();
    let raw_dir = query.get("dir").map(|v| Value::String(v.clone()));
    let dir = match resolve_desk_dir(&state, raw_dir.as_ref(), agent_id.as_deref()) {
        Ok(Some(dir)) => dir,
        Ok(None) => return Json(json!({ "content": Value::Null })).into_response(),
        Err(_) => return desk_guard_error(t_desk(&state, "dirNotAllowed")),
    };
    let subdir = query.get("subdir").cloned().unwrap_or_default();
    if let Some(response) = desk_subdir_guard(&dir, &subdir) {
        return response;
    }
    let root = match desk_root(&dir) {
        Ok(root) => root,
        Err(err) => return desk_service_error(&err),
    };
    let ctx = desk_read_ctx();
    match read_text(&state, &root, &subdir, "jian.md", &ctx).await {
        Ok(payload) => {
            let content = if payload["exists"].as_bool().unwrap_or(false) {
                payload["content"].clone()
            } else {
                Value::Null
            };
            Json(json!({ "content": content })).into_response()
        }
        Err(err) => {
            if err.code == "resource_not_found" {
                return Json(json!({ "content": Value::Null })).into_response();
            }
            desk_service_error(&err)
        }
    }
}

/// 现役 POST /desk/jian（desk.ts :1485-1512）：内容空白 → safeDeleteIfExists
/// 删除；否则 writeText 覆盖。
async fn desk_jian_write(
    State(state): State<ServiceState>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return crate::EndpointError::from_json_rejection(&rejection).into_response()
        }
    };
    let agent_id = agent_id_of(body.get("agentId"));
    let raw_dir = body.get("dir");
    let dir = match resolve_desk_dir(&state, raw_dir, agent_id.as_deref()) {
        Ok(Some(dir)) => dir,
        Ok(None) => return desk_guard_error(t_desk(&state, "noWorkspace")),
        Err(_) => return desk_guard_error(t_desk(&state, "dirNotAllowed")),
    };
    let subdir = body
        .get("subdir")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if let Some(response) = desk_subdir_guard(&dir, &subdir) {
        return response;
    }
    let root = match desk_root(&dir) {
        Ok(root) => root,
        Err(err) => return desk_service_error(&err),
    };
    let content = body.get("content");
    let blank = match content {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        // 现役 content.trim() 对非字符串抛 TypeError 进外层 catch（400
        // {error: message}，无 code）。
        Some(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "content.trim is not a function" })),
            )
                .into_response()
        }
    };
    if blank {
        let ctx = desk_ctx(&body, "desk.jian.delete");
        return match safe_delete_if_exists(&state, &root, &subdir, "jian.md", false, &ctx).await {
            Ok(_) => {
                state.app_events().publish(
                    "jian_update",
                    json!({"dir":dir,"subdir":subdir,"content":Value::Null}),
                );
                Json(json!({ "ok": true, "content": Value::Null })).into_response()
            }
            Err(err) => desk_service_error(&err),
        };
    }
    let text = content
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let ctx = desk_ctx(&body, "desk.jian.write");
    let write_body = json!({
        "action": "writeText",
        "name": "jian.md",
        "content": text,
    });
    match action_write_text(&state, &root, &subdir, &write_body, false, &ctx, false).await {
        Ok(_) => {
            state.app_events().publish(
                "jian_update",
                json!({"dir":dir,"subdir":subdir,"content":text}),
            );
            Json(json!({ "ok": true, "content": text })).into_response()
        }
        Err(err) => desk_service_error(&err),
    }
}

/// 现役 isPlainEntryName（desk.ts :94-102）。
fn is_plain_entry_name(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
}

/// Node path.basename 的最小等价（尾随斜杠归一；根 → 空）。
fn basename(value: &str) -> String {
    Path::new(value)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 现役 POST /desk/files（desk.ts :1515-1687）。
async fn desk_files_action(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => {
            return crate::EndpointError::from_json_rejection(&rejection).into_response()
        }
    };
    let agent_id = agent_id_of(body.get("agentId"));
    let raw_dir = body.get("dir");
    let base_dir = match resolve_desk_dir(&state, raw_dir, agent_id.as_deref()) {
        Ok(Some(dir)) => dir,
        Ok(None) => return desk_guard_error(t_desk(&state, "noWorkspace")),
        Err(_) => return desk_guard_error(t_desk(&state, "dirNotAllowed")),
    };
    // 现役 fs.mkdirSync(baseDir, {recursive:true}) 在 switch 之前、try 之外。
    if let Err(err) = std::fs::create_dir_all(&base_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("cannot create workspace root: {err}") })),
        )
            .into_response();
    }
    let subdir = body
        .get("subdir")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if let Some(response) = desk_subdir_guard(&base_dir, &subdir) {
        return response;
    }
    let action = body.get("action").and_then(Value::as_str).unwrap_or("");
    let root = workbench::ResolvedRoot::default_with_path(base_dir.clone());
    let reason = |name: &str| format!("desk.files.{name}");

    match action {
        "upload" => {
            if !principal.is_local_owner() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({ "error": "upload by absolute path requires local owner" })),
                )
                    .into_response();
            }
            let paths = body.get("paths").and_then(Value::as_array).cloned();
            let Some(paths) = paths.filter(|list| !list.is_empty()) else {
                return desk_route_error("workspace_file_validation_failed", "paths required", 400);
            };
            let home = state.config().data_home.clone();
            let ctx = desk_ctx(&body, &reason("upload"));
            let mut results = Vec::new();
            for item in &paths {
                let Some(src) = item.as_str() else {
                    // 现役 path.isAbsolute 对非字符串抛 TypeError → 逐项
                    // catch；候选人按 invalid path 记账（消息登记偏差）。
                    results.push(json!({ "src": item, "error": "invalid path" }));
                    continue;
                };
                let src_path = Path::new(src);
                if !src_path.is_absolute() || !src_path.exists() {
                    results.push(json!({ "src": src, "error": "invalid path" }));
                    continue;
                }
                if is_sensitive_path(src, &home) {
                    results.push(json!({ "src": src, "error": "sensitive path blocked" }));
                    continue;
                }
                match copy_local_path_into_directory(&state, &root, &subdir, src, &ctx).await {
                    Ok((filename, _)) => {
                        results.push(json!({ "src": src, "name": filename }));
                    }
                    Err(err) => {
                        results.push(json!({ "src": src, "error": err.detail }));
                    }
                }
            }
            match files_for_directory(&state, &root, &subdir, false, &ctx).await {
                Ok(files) => {
                    Json(json!({ "ok": true, "results": results, "files": files })).into_response()
                }
                Err(err) => desk_service_error(&err),
            }
        }
        "create" => {
            let name = body.get("name").and_then(Value::as_str).unwrap_or("");
            // 现役 `!name || content === undefined`：content 键缺席才拒
            // （null 通过，服务层 String(null ?? "") = ""）。
            if name.is_empty() || body.get("content").is_none() {
                return desk_route_error(
                    "workspace_file_validation_failed",
                    "name and content required",
                    400,
                );
            }
            if !is_plain_entry_name(name) {
                return desk_guard_error("invalid name");
            }
            let ctx = desk_ctx(&body, &reason("create"));
            let write_body = json!({
                "action": "create",
                "name": name,
                "content": body.get("content").cloned().unwrap_or(Value::Null),
            });
            match action_write_text(&state, &root, &subdir, &write_body, false, &ctx, true).await {
                Ok(payload) => {
                    Json(json!({ "ok": true, "files": payload["files"].clone() })).into_response()
                }
                Err(err) => desk_service_error(&err),
            }
        }
        "mkdir" => {
            let name = body.get("name").and_then(Value::as_str).unwrap_or("");
            if name.is_empty() {
                return desk_route_error("workspace_file_validation_failed", "name required", 400);
            }
            if !is_plain_entry_name(name) {
                return desk_guard_error("invalid name");
            }
            let ctx = desk_ctx(&body, &reason("mkdir"));
            match action_mkdir(
                &state,
                &root,
                &subdir,
                &json!({ "name": name }),
                false,
                &ctx,
            )
            .await
            {
                Ok(payload) => {
                    Json(json!({ "ok": true, "files": payload["files"].clone() })).into_response()
                }
                Err(err) => desk_service_error(&err),
            }
        }
        "rename" => {
            let old_name = body.get("oldName").and_then(Value::as_str).unwrap_or("");
            let new_name = body.get("newName").and_then(Value::as_str).unwrap_or("");
            if old_name.is_empty() || new_name.is_empty() {
                return desk_route_error(
                    "workspace_file_validation_failed",
                    "oldName and newName required",
                    400,
                );
            }
            if !is_plain_entry_name(old_name) || !is_plain_entry_name(new_name) {
                return desk_guard_error("invalid name");
            }
            let ctx = desk_ctx(&body, &reason("rename"));
            let rename_body = json!({ "oldName": old_name, "newName": new_name });
            match action_rename(&state, &root, &subdir, &rename_body, false, &ctx).await {
                Ok(payload) => {
                    Json(json!({ "ok": true, "files": payload["files"].clone() })).into_response()
                }
                Err(err) => desk_service_error(&err),
            }
        }
        "move" => {
            let names = body.get("names").and_then(Value::as_array).cloned();
            let dest_folder = body.get("destFolder").and_then(Value::as_str).unwrap_or("");
            let Some(names) = names.filter(|list| !list.is_empty()) else {
                return desk_route_error(
                    "workspace_file_validation_failed",
                    "names[] and destFolder required",
                    400,
                );
            };
            if dest_folder.is_empty() {
                return desk_route_error(
                    "workspace_file_validation_failed",
                    "names[] and destFolder required",
                    400,
                );
            }
            if names.iter().any(|n| n.as_str() == Some(dest_folder)) {
                return desk_guard_error("cannot move folder into itself");
            }
            let dir = if subdir.is_empty() {
                base_dir.clone()
            } else {
                base_dir.join(&subdir)
            };
            let dest_dir = dir.join(basename(dest_folder));
            if !is_inside_path(&dest_dir, &dir) {
                return desk_guard_error("invalid destFolder");
            }
            if !dest_dir.is_dir() {
                return desk_guard_error("destFolder is not a directory");
            }
            let ctx = desk_ctx(&body, &reason("move"));
            let mut results = Vec::new();
            for item in &names {
                let raw_name = item.as_str().unwrap_or("");
                let move_body = json!({
                    "name": basename(raw_name),
                    "destSubdir": basename(dest_folder),
                });
                match action_move(&state, &root, &subdir, &move_body, false, &ctx).await {
                    Ok(_) => results.push(json!({ "name": item, "ok": true })),
                    Err(err) => results.push(json!({
                        "name": item,
                        "error": match err.code.as_str() {
                            "resource_not_found" => "not found".to_string(),
                            "target_already_exists" => "target already exists".to_string(),
                            "already_exists" => "already exists".to_string(),
                            _ => err.detail.clone(),
                        },
                    })),
                }
            }
            match files_for_directory(&state, &root, &subdir, false, &ctx).await {
                Ok(files) => {
                    Json(json!({ "ok": true, "results": results, "files": files })).into_response()
                }
                Err(err) => desk_service_error(&err),
            }
        }
        "movePaths" => {
            let items = body.get("items").and_then(Value::as_array).cloned();
            let Some(items) = items.filter(|list| !list.is_empty()) else {
                return desk_route_error(
                    "workspace_file_validation_failed",
                    "items[] required",
                    400,
                );
            };
            let dest_subdir = body
                .get("destSubdir")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let current_subdir = body
                .get("currentSubdir")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            // 现役 subdirToDir：substring 守卫 + isInsidePath。
            if !dest_subdir.is_empty() && (dest_subdir.contains('\\') || dest_subdir.contains(".."))
            {
                return desk_guard_error("invalid destSubdir");
            }
            let dest_dir = if dest_subdir.is_empty() {
                base_dir.clone()
            } else {
                base_dir.join(&dest_subdir)
            };
            if !is_inside_path(&dest_dir, &base_dir) {
                return desk_guard_error("invalid destSubdir");
            }
            if !dest_dir.is_dir() {
                return desk_guard_error("destSubdir is not a directory");
            }
            let ctx = desk_ctx(&body, &reason("move_paths"));
            let move_body = json!({
                "items": items,
                "destSubdir": dest_subdir,
                "currentSubdir": current_subdir,
                "createDestIfMissing": false,
            });
            match action_move_paths(&state, &root, &move_body, false, &ctx).await {
                Ok(payload) => Json(json!({
                    "ok": true,
                    "results": payload["results"].clone(),
                    "filesByPath": payload["filesByPath"].clone(),
                    "files": payload["files"].clone(),
                }))
                .into_response(),
                Err(err) => desk_service_error(&err),
            }
        }
        "remove" => {
            let name = body.get("name").and_then(Value::as_str).unwrap_or("");
            if name.is_empty() {
                return desk_route_error("workspace_file_validation_failed", "name required", 400);
            }
            let ctx = desk_ctx(&body, &reason("remove"));
            match action_safe_delete(
                &state,
                &root,
                &subdir,
                &json!({ "name": name }),
                false,
                &ctx,
            )
            .await
            {
                Ok(payload) => Json(json!({
                    "ok": true,
                    "trashId": payload["trashId"].clone(),
                    "files": payload["files"].clone(),
                }))
                .into_response(),
                Err(err) => desk_service_error(&err),
            }
        }
        other => {
            // 现役模板 `${action}`：undefined → "undefined"，非字符串按 JS
            // 字符串化。
            let rendered = match body.get("action") {
                None => "undefined".to_string(),
                Some(Value::String(text)) => text.clone(),
                Some(Value::Number(number)) => number.to_string(),
                Some(Value::Bool(flag)) => flag.to_string(),
                Some(_) => "[object Object]".to_string(),
            };
            let _ = other;
            desk_route_error(
                "unknown_workspace_file_action",
                &format!("unknown action: {rendered}"),
                400,
            )
        }
    }
}

pub fn router() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/desk/path", get(desk_path))
        .route(
            "/lingxi/v1/desk/files",
            get(desk_files_list).post(desk_files_action),
        )
        .route("/lingxi/v1/desk/search-files", get(desk_search_files))
        .route(
            "/lingxi/v1/desk/jian",
            get(desk_jian_read).post(desk_jian_write),
        )
}
