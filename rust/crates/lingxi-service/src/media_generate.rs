//! 工作台媒体动作 HTTP 面（现役 server/routes/media.ts 的生成/任务/产物/转写路由）。
//!
//! 生成不另建通道：HTTP 请求合成真实来源 Run（media_run::admit_media_request，
//! 与封面链同一先例），经同一工具网关 准备→意向持久→政策裁决→批准门→执行→回执，
//! 受理回执绝不冒充产物完成（A11：响应只带任务受理事实；response 投递的语音
//! 在终态到达后才回文件，失败如实报错）。
//!
//! 任务读面读 media_gallery 投影（与运行时作业同一写事务同步，不允许跨表漂移）；
//! 产物文件按所有者作用域解析到会话文件真实主机路径流式响应（Range 206/416
//! 与现役平价）；open 走可注入 opener（生产=系统打开，测试=录制替身），仅本地
//! 主人；ASR 转写经 OperationService::transcribe，取消/失败绝不记为成功。
//!
//! 登记的偏差（docs/rust-tauri/R07/T06_MEDIA_STATUS.md）：
//! - sessionId 必填（现役允许 sessionPath/sessionRef/response 无会话投递；Rust 面
//!   只接受 sessionId，会话归属与权限检查都建立在真实会话行上）。
//! - 一次调用产生一个运行作业（spec.count 承载张数），batchId=taskId=operation_id；
//!   现役的 count→N 任务扇出不复制。
//! - 重试铸新运行作业（运行身份反重复语义），工作台任务经 rekey 保留
//!   attempt/retry_count 连续性，响应携带新 taskId。
//! - 产物全量响应整读入内存（无流式），Range 窗口按请求精确读取。
//! - ASR 不支持 provider/model 覆盖（运营层只解析已配置默认路由；携带不同覆盖
//!   响亮 400，不静默改用默认），无 supersede 注册表（并发同文件转写各自完成）。
//! - 会话正忙（回合进行中）时 HTTP 生成返回 409 session_busy（会话准入是互斥的），
//!   现役允许并发；工具面（回合内生成）不受影响。
use crate::{
    agents_admin::agent_config_readback::scope_denied, runs::MEDIA_REQUEST_SESSION_BUSY,
    sessions::SessionAccess, EndpointError, Principal, ServiceState,
};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use lingxi_adapters::storage::{media as media_store, media_gallery, session_tree};
use lingxi_kernel::subagent::SessionPermissionMode;
use serde_json::{json, Map, Value};
use std::sync::Arc;

/// 产物"系统打开"的注入点：生产组合根给平台 opener，测试给录制替身。
pub type MediaFileOpener = Arc<dyn Fn(&std::path::Path) -> Result<(), String> + Send + Sync>;

/// 生产 opener：桌面系统处理器（macOS open / Windows start / 其他 xdg-open）。
/// 打开失败响亮返回，不冒充成功。
pub fn platform_media_file_opener() -> MediaFileOpener {
    Arc::new(|path: &std::path::Path| {
        let status = if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(path).status()
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("cmd")
                .args(["/C", "start", ""])
                .arg(path)
                .status()
        } else {
            std::process::Command::new("xdg-open").arg(path).status()
        };
        match status {
            Ok(exit) if exit.success() => Ok(()),
            Ok(exit) => Err(format!("system open exited with {exit}")),
            Err(error) => Err(format!("system open failed: {error}")),
        }
    })
}

// ── 会话归属与身份 ────────────────────────────────────────────────────────

/// 现役 owner() 闸的媒体面版本：路由归属检查 + 会话行一次取回（agent、权限
/// 模式、所有者都由持久行裁决，正文声明的身份不作数）。
async fn owned_session(
    state: &ServiceState,
    principal: &Principal,
    sid: &str,
) -> Result<session_tree::SessionTreeRow, Box<Response>> {
    match state.sessions().get_for(principal, sid).await {
        Ok(SessionAccess::Ok(_)) => {}
        Ok(SessionAccess::NotFound) => {
            return Err(Box::new(EndpointError::not_found().into_response()))
        }
        Ok(SessionAccess::Forbidden) => {
            return Err(Box::new(
                EndpointError::forbidden("cross_principal_access").into_response(),
            ))
        }
        Err(error) => return Err(Box::new(EndpointError::storage(&error).into_response())),
    }
    let session = session_tree::get_session_row(state.storage(), sid)
        .await
        .map_err(|error| Box::new(EndpointError::storage(&error).into_response()))?
        .ok_or_else(|| Box::new(EndpointError::not_found().into_response()))?;
    if session.lifecycle == "deleted" {
        return Err(Box::new(EndpointError::not_found().into_response()));
    }
    Ok(session)
}

fn owner_subject(principal: &Principal) -> String {
    crate::sessions::kernel_principal_of(principal).storage_subject()
}

// ── HTTP 来源生成的驱动（合成真实来源 Run → 同一工具网关） ────────────────

/// 把 HTTP 正文投影成正式工具参数：只放白名单键（工具 schema
/// additionalProperties:false，路由级字段 sessionId/delivery 不下发），
/// input 包裹与平铺两种现役形状都接受。count 数值夹取 1..=9（现役上界）。
fn generate_args(kind: &str, body: &Value) -> Result<Value, EndpointError> {
    let input = if body["input"].is_object() {
        &body["input"]
    } else {
        body
    };
    let per_kind: &[&str] = match kind {
        "image" => &[
            "count",
            "ratio",
            "resolution",
            "quality",
            "image",
            "referenceImages",
            "suggestedFilename",
        ],
        "video" => &["image", "duration", "ratio", "resolution"],
        "speech" => &["voice", "speed", "format"],
        _ => return Err(EndpointError::invalid_message("unsupported media kind")),
    };
    let mut args = Map::new();
    for key in ["prompt", "provider", "model", "mode", "options"]
        .iter()
        .chain(per_kind.iter())
    {
        if let Some(value) = input.get(*key) {
            if !value.is_null() {
                args.insert((*key).to_string(), value.clone());
            }
        }
    }
    if let Some(count) = args.get("count") {
        let Some(raw) = count.as_f64() else {
            return Err(EndpointError::invalid_message("count must be a number"));
        };
        if !raw.is_finite() {
            return Err(EndpointError::invalid_message("count must be a number"));
        }
        let clamped = raw.clamp(1.0, 9.0);
        args.insert("count".to_string(), json!(clamped as u64));
    }
    match args.get("prompt").and_then(Value::as_str) {
        Some(prompt) if !prompt.trim().is_empty() => {}
        _ => return Err(EndpointError::invalid_message("prompt is required")),
    }
    Ok(Value::Object(args))
}

/// 现役 normalizeMediaDelivery：只有显式 "response" 是 response，其余一律 session。
fn response_delivery(input: &Value) -> bool {
    let body = if input["input"].is_object() {
        &input["input"]
    } else {
        input
    };
    body["delivery"]["mode"].as_str() == Some("response")
        || body["deliveryMode"].as_str() == Some("response")
}

/// 一次 HTTP 发起的媒体生成：合成来源 Run（持久 started/lineage）→ 网关准备
/// → 意向先持久 → 政策裁决（Allowed 直行；NeedsApproval 经组合根批准门等待
/// 真实人工决定，拒绝/中止/无批准面都是零派发的如实回执）→ 执行 → 回执 →
/// 统一收尾。返回媒体受理回执（mediaGeneration 明细，tasks 状态 pending）。
async fn drive_generate(
    state: &ServiceState,
    principal: &Principal,
    session: &session_tree::SessionTreeRow,
    kind: &str,
    args: Value,
) -> Result<Value, EndpointError> {
    let action = crate::runs::MediaGenerateAction::parse(kind)
        .ok_or_else(|| EndpointError::invalid_message("unsupported media kind"))?;
    let mode = match session.permission_mode.as_deref() {
        None => SessionPermissionMode::Operate,
        Some(raw) => SessionPermissionMode::parse(raw).ok_or_else(|| {
            EndpointError::resource_failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "session_mode_corrupt",
                "session permission mode is not in the wire vocabulary",
            )
        })?,
    };
    if mode.is_read_only() {
        return Err(EndpointError::forbidden("media_generate_read_only_session"));
    }
    state.tool_gateway().ok_or_else(|| {
        EndpointError::resource_failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "media_gateway_unavailable",
            "媒体工具网关尚未配置",
        )
    })?;
    let now = state.clock().now_unix_ms();
    let request_id = state.request_ids().next_request_id();
    let kernel = crate::sessions::kernel_principal_of(principal);
    let permit = state
        .runs()
        .admit_media_request(
            state.storage(),
            state.events(),
            &session.session_id,
            &session.agent_id,
            kernel,
            &request_id,
            mode,
            now,
        )
        .await
        .map_err(|message| {
            if message == MEDIA_REQUEST_SESSION_BUSY {
                EndpointError::session_busy()
            } else {
                EndpointError::invalid_message(message)
            }
        })?;
    let db = state.storage().clone();
    let events = state.events().clone();
    let runs = state.runs().clone();
    let (finish_code, result) = runs
        .execute_media_generate(crate::runs::MediaGenerateCall {
            db: &db,
            events: &events,
            session,
            permit: &permit,
            request_id: &request_id,
            action,
            args,
            now,
        })
        .await;
    runs.finish_media_request(&db, &events, permit, finish_code, now)
        .await
        .map_err(EndpointError::invalid_message)?;
    result
}

// ── 生成路由 ──────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct GeneratePath {
    kind: String,
}

async fn generate_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(path): Path<GeneratePath>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    let kind = match path.kind.as_str() {
        "image" | "video" | "speech" => path.kind.as_str(),
        _ => return EndpointError::not_found().into_response(),
    };
    generate_inner(state, principal, kind, &body).await
}

/// POST /lingxi/v1/media/generate 的 kind 派发（现役 generateMedia 词表）。
async fn generate_dispatch_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    let kind_value = body["kind"]
        .as_str()
        .or(body["type"].as_str())
        .unwrap_or("");
    let kind = match kind_value {
        "image" | "image_generation" | "imageGeneration" => "image",
        "video" | "video_generation" | "videoGeneration" => "video",
        "speech_generation" | "speechGeneration" | "speech" => "speech",
        "audio" | "speech_recognition" | "transcription" | "asr" => {
            return transcribe_inner(state, principal, &body).await
        }
        _ => {
            return EndpointError::invalid_message(format!("unsupported media kind: {kind_value}"))
                .into_response()
        }
    };
    generate_inner(state, principal, kind, &body).await
}

async fn generate_inner(
    state: ServiceState,
    principal: Principal,
    kind: &str,
    body: &Value,
) -> Response {
    let Some(sid) = body["sessionId"].as_str() else {
        return EndpointError::invalid_message("sessionId is required").into_response();
    };
    let session = match owned_session(&state, &principal, sid).await {
        Ok(session) => session,
        Err(error) => return *error,
    };
    let args = match generate_args(kind, body) {
        Ok(args) => args,
        Err(error) => return error.into_response(),
    };
    let prompt = args["prompt"].as_str().unwrap_or_default().to_string();
    let wait_terminal = kind == "speech" && response_delivery(body);
    let accepted = match drive_generate(&state, &principal, &session, kind, args).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let task_id = accepted["batchId"].as_str().unwrap_or_default().to_string();
    if task_id.is_empty() {
        return EndpointError::resource_failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "media_receipt_missing",
            "媒体受理回执缺少任务编号",
        )
        .into_response();
    }
    let mut task = json!({"taskId":task_id});
    // A11：只有终态 completed 且文件真实登记后才在响应里带文件；response 投递
    // 的语音（朗读链）在终态前等待，失败/未知如实报错，绝不用 pending 冒充完成。
    if wait_terminal {
        match wait_speech_terminal(&state, &task_id).await {
            Ok(Some(files)) => {
                task["files"] = json!(files);
            }
            Ok(None) => {}
            Err(message) => return EndpointError::invalid_message(message).into_response(),
        }
    }
    Json(json!({
        "ok": true,
        "kind": kind,
        "batchId": task_id,
        "prompt": prompt,
        "delivery": {"mode": if wait_terminal { "response" } else { "session" }},
        "tasks": [task],
    }))
    .into_response()
}

/// response 投递的语音等待终态：completed → 真实文件名列表；failed/cancelled/
/// unknown → 如实错误；等待预算耗尽 → None（受理回执已返回，后台任务继续，
/// 不冒充完成也不谎报失败）。媒体执行仍在后台运行时本等待只是读投影。
async fn wait_speech_terminal(
    state: &ServiceState,
    operation_id: &str,
) -> Result<Option<Vec<String>>, String> {
    let budget = state.clock().now_unix_ms().saturating_add(110_000);
    loop {
        let job = media_store::get(state.storage(), operation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体作业已不存在")?;
        match job.phase {
            media_store::MediaPhase::Completed => {
                let mut files = Vec::new();
                if let Some(list) = job.state["files"].as_array() {
                    for file in list {
                        let filename = file["filename"].as_str().ok_or("媒体交付文件缺少名称")?;
                        files.push(filename.to_string());
                    }
                }
                if files.is_empty() {
                    return Err("媒体生成已完成但未交付文件".into());
                }
                return Ok(Some(files));
            }
            media_store::MediaPhase::Failed
            | media_store::MediaPhase::Cancelled
            | media_store::MediaPhase::Unknown => {
                let message = job.state["message"]
                    .as_str()
                    .unwrap_or("媒体生成未交付")
                    .to_string();
                return Err(message);
            }
            _ => {}
        }
        if state.clock().now_unix_ms() >= budget {
            return Ok(None);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

// ── 任务库读面与重试 ──────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct TasksQuery {
    filter: Option<String>,
}

async fn tasks_list(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<TasksQuery>,
) -> Response {
    if let Some(filter) = query.filter.as_deref() {
        if !matches!(filter, "favorited" | "images" | "videos") {
            return EndpointError::invalid_message("unsupported media task filter").into_response();
        }
    }
    let owner = owner_subject(&principal);
    match media_gallery::list_owner(state.storage(), &owner, query.filter.as_deref()).await {
        Ok(tasks) => Json(json!({
            "tasks": tasks.iter().map(|task| task.to_value()).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(error) => EndpointError::storage(&error).into_response(),
    }
}

async fn tasks_batch(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(batch_id): Path<String>,
) -> Response {
    let owner = owner_subject(&principal);
    match media_gallery::list_batch(state.storage(), &batch_id, &owner).await {
        Ok(tasks) => Json(json!({
            "tasks": tasks.iter().map(|task| task.to_value()).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(error) => EndpointError::storage(&error).into_response(),
    }
}

async fn tasks_get(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(task_id): Path<String>,
) -> Response {
    let owner = owner_subject(&principal);
    match media_gallery::get_owner(state.storage(), &task_id, &owner).await {
        Ok(Some(task)) => Json(json!({"task": task.to_value()})).into_response(),
        Ok(None) => EndpointError::not_found().into_response(),
        Err(error) => EndpointError::storage(&error).into_response(),
    }
}

/// 现役 retryImageTask（providers.manage 门）：仅图片任务、仅终态可重试；
/// 成功受理后工作台任务换绑到新运行作业（attempt/retry_count 连续），提交
/// 失败回到 failed 并记录原因（一次真实尝试发生过，计数不回退）。
async fn tasks_retry(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(task_id): Path<String>,
) -> Response {
    if scope_denied(Some(&principal), "providers.manage") {
        return EndpointError::forbidden("insufficient_scope").into_response();
    }
    let owner = owner_subject(&principal);
    let task = match media_gallery::get_owner(state.storage(), &task_id, &owner).await {
        Ok(Some(task)) => task,
        Ok(None) => return EndpointError::not_found().into_response(),
        Err(error) => return EndpointError::storage(&error).into_response(),
    };
    if task.kind != "image" {
        return EndpointError::invalid_message("only image tasks can be retried here")
            .into_response();
    }
    let job = match media_store::get(state.storage(), &task_id).await {
        Ok(Some(job)) => job,
        Ok(None) => return EndpointError::not_found().into_response(),
        Err(error) => return EndpointError::storage(&error).into_response(),
    };
    if job.session_id != task.session_id {
        return EndpointError::resource_failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "media_task_projection_diverged",
            "工作台任务与运行作业不一致",
        )
        .into_response();
    }
    let session = match owned_session(&state, &principal, &task.session_id).await {
        Ok(session) => session,
        Err(error) => return *error,
    };
    let now = state.clock().now_unix_ms();
    let retried = match media_gallery::begin_retry(state.storage(), &task_id, &owner, now).await {
        Ok(Some(task)) => task,
        Ok(None) => return EndpointError::not_found().into_response(),
        Err(lingxi_kernel::ports::StorageError::Conflict { detail }) => {
            return EndpointError::resource_failure(
                StatusCode::CONFLICT,
                "media_task_not_retryable",
                detail,
            )
            .into_response()
        }
        Err(error) => return EndpointError::storage(&error).into_response(),
    };
    match drive_generate(&state, &principal, &session, "image", job.spec.clone()).await {
        Ok(accepted) => {
            let new_task_id = accepted["batchId"].as_str().unwrap_or_default().to_string();
            if new_task_id.is_empty() {
                let _ = media_gallery::abort_retry(
                    state.storage(),
                    &task_id,
                    "重试受理回执缺少任务编号",
                    state.clock().now_unix_ms(),
                )
                .await;
                return EndpointError::resource_failure(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "media_receipt_missing",
                    "媒体受理回执缺少任务编号",
                )
                .into_response();
            }
            if let Err(error) =
                media_gallery::rekey_retry(state.storage(), &task_id, &new_task_id).await
            {
                let _ = media_gallery::abort_retry(
                    state.storage(),
                    &task_id,
                    &format!("重试换绑失败：{error}"),
                    state.clock().now_unix_ms(),
                )
                .await;
                return EndpointError::storage(&error).into_response();
            }
            Json(json!({
                "ok": true,
                "taskId": new_task_id,
                "placeholder": {
                    "type": "media_generation",
                    "taskId": new_task_id,
                    "kind": "image",
                    "batchId": new_task_id,
                    "prompt": retried.prompt,
                    "status": "pending",
                },
            }))
            .into_response()
        }
        Err(error) => {
            let message = error.error.message.clone();
            let _ = media_gallery::abort_retry(
                state.storage(),
                &task_id,
                &format!("重试提交失败：{message}"),
                state.clock().now_unix_ms(),
            )
            .await;
            error.into_response()
        }
    }
}

// ── 产物文件服务与系统打开 ────────────────────────────────────────────────

fn valid_generated_filename(filename: &str) -> bool {
    !filename.is_empty()
        && !filename.contains('/')
        && !filename.contains('\\')
        && filename != ".."
        && !filename.contains("..")
}

fn generated_mime(filename: &str) -> &'static str {
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "pcm" => "audio/pcm",
        _ => "application/octet-stream",
    }
}

/// 所有者作用域内把产物文件名解析为真实主机路径：gallery 投影的 files 条目
/// 携带 session fileId，真实路径只从会话文件登记取得（文件名本身永远不作
/// 为路径拼接输入）。同名文件取最新任务（与现役平铺目录后写覆盖平价）。
async fn resolve_generated(
    state: &ServiceState,
    owner: &str,
    filename: &str,
) -> Result<Option<String>, EndpointError> {
    let found = media_gallery::find_file_owner(state.storage(), owner, filename)
        .await
        .map_err(|error| EndpointError::storage(&error))?;
    let Some((file_id, _task_id)) = found else {
        return Ok(None);
    };
    let view = state
        .session_files()
        .get_file_global(&file_id)
        .await
        .map_err(|error| EndpointError::invalid_message(error.to_string()))?;
    Ok(view.map(|view| view.real_path))
}

async fn generated_serve(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(filename): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !valid_generated_filename(&filename) {
        return EndpointError::invalid_message("invalid filename").into_response();
    }
    let owner = owner_subject(&principal);
    let resolved = match resolve_generated(&state, &owner, &filename).await {
        Ok(resolved) => resolved,
        Err(error) => return error.into_response(),
    };
    let Some(real_path) = resolved else {
        return EndpointError::not_found().into_response();
    };
    let path = std::path::PathBuf::from(&real_path);
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => return EndpointError::not_found().into_response(),
    };
    let total = metadata.len();
    // MIME 由扩展名裁决（现役 MIME 表平价；登记表 stored mime 不参与服务头）。
    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let (status, start, end) = match range {
        Some(raw) => {
            let Some(body) = raw.strip_prefix("bytes=") else {
                return range_invalid();
            };
            let Some((left, right)) = body.split_once('-') else {
                return range_invalid();
            };
            let start = if left.is_empty() {
                0
            } else {
                match left.parse::<u64>() {
                    Ok(value) => value,
                    Err(_) => return range_invalid(),
                }
            };
            let end = if right.is_empty() {
                total.saturating_sub(1)
            } else {
                match right.parse::<u64>() {
                    Ok(value) => value,
                    Err(_) => return range_invalid(),
                }
            };
            if total == 0 || start > end || start >= total || end >= total {
                return range_invalid();
            }
            (StatusCode::PARTIAL_CONTENT, start, end)
        }
        None => (StatusCode::OK, 0, total.saturating_sub(1)),
    };
    let length = if total == 0 { 0 } else { end - start + 1 };
    let bytes = match read_window(&path, start, length).await {
        Ok(bytes) => bytes,
        Err(_) => return EndpointError::not_found().into_response(),
    };
    let mime = generated_mime(&filename);
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).unwrap_or(HeaderValue::from_static("0")),
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    if status == StatusCode::PARTIAL_CONTENT {
        headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{total}"))
                .unwrap_or(HeaderValue::from_static("bytes */0")),
        );
    }
    response
}

fn range_invalid() -> Response {
    EndpointError::resource_failure(
        StatusCode::RANGE_NOT_SATISFIABLE,
        "invalid_range",
        "invalid range",
    )
    .into_response()
}

/// 只读请求窗口（std::fs：本 crate 的 tokio 未开 fs feature，与全 crate 既有
/// 文件读取同一形态；大文件读取经 spawn_blocking 让出运行时线程）。
fn read_window_blocking(
    path: &std::path::Path,
    start: u64,
    length: u64,
) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let length = usize::try_from(length).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "media window too large")
    })?;
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut buffer = vec![0u8; length];
    file.read_exact(&mut buffer)?;
    Ok(buffer)
}

async fn read_window(path: &std::path::Path, start: u64, length: u64) -> std::io::Result<Vec<u8>> {
    let path = path.to_path_buf();
    match tokio::task::spawn_blocking(move || read_window_blocking(&path, start, length)).await {
        Ok(result) => result,
        Err(join) => Err(std::io::Error::other(format!(
            "media read task failed: {join}"
        ))),
    }
}

async fn generated_open(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Path(filename): Path<String>,
) -> Response {
    if !principal.is_local_owner() {
        return EndpointError::local_only().into_response();
    }
    if !valid_generated_filename(&filename) {
        return EndpointError::invalid_message("invalid filename").into_response();
    }
    let owner = owner_subject(&principal);
    let resolved = match resolve_generated(&state, &owner, &filename).await {
        Ok(resolved) => resolved,
        Err(error) => return error.into_response(),
    };
    let Some(real_path) = resolved else {
        return EndpointError::not_found().into_response();
    };
    let Some(opener) = state.media_file_opener().cloned() else {
        return EndpointError::resource_failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "media_opener_unavailable",
            "系统打开器未配置",
        )
        .into_response();
    };
    let path = std::path::PathBuf::from(real_path);
    match tokio::task::spawn_blocking(move || opener(&path)).await {
        Ok(Ok(())) => Json(json!({"ok": true})).into_response(),
        Ok(Err(message)) => EndpointError::resource_failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "media_open_failed",
            message,
        )
        .into_response(),
        Err(join) => EndpointError::resource_failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "media_open_failed",
            format!("opener task failed: {join}"),
        )
        .into_response(),
    }
}

// ── ASR 转写 ──────────────────────────────────────────────────────────────

/// 现役 _transcribeAudioWithinTrace 的 Rust 面：pending → ready/failed 两次
/// voice_transcription_update 事件；失败返回 200 + status:"failed"（错误以
/// "CODE: message" 前缀透出），路由级错误（缺参/文件不存在/未配置）走 400。
/// 取消/超时绝不记为成功（操作层只有真实成功才到 ready）。
async fn transcribe_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    transcribe_inner(state, principal, &body).await
}

async fn transcribe_inner(state: ServiceState, principal: Principal, body: &Value) -> Response {
    let (Some(sid), Some(file_id)) = (body["sessionId"].as_str(), body["fileId"].as_str()) else {
        return EndpointError::invalid_message(
            "sessionId and fileId are required for audio transcription",
        )
        .into_response();
    };
    let session = match owned_session(&state, &principal, sid).await {
        Ok(session) => session,
        Err(error) => return *error,
    };
    let operations = match state.operations() {
        Some(operations) => operations.clone(),
        None => {
            return EndpointError::invalid_message("speech recognition model is not configured")
                .into_response()
        }
    };
    let route = match operations.speech_recognition_route().await {
        Ok(route) => route,
        Err(failure) => return EndpointError::invalid_message(failure.message).into_response(),
    };
    let (provider, model, protocol) = route;
    // 运营层只解析已配置默认路由；携带不同覆盖响亮拒绝，不静默改用默认。
    let requested_provider = body["providerId"].as_str().or(body["provider"].as_str());
    let requested_model = body["modelId"].as_str().or(body["model"].as_str());
    if let Some(requested) = requested_provider {
        if requested != provider {
            return EndpointError::invalid_message(format!(
                "speech recognition provider override \"{requested}\" is not honored by this \
                 surface; configure the default model instead"
            ))
            .into_response();
        }
    }
    if let Some(requested) = requested_model {
        if requested != model {
            return EndpointError::invalid_message(format!(
                "speech recognition model override \"{requested}\" is not honored by this \
                 surface; configure the default model instead"
            ))
            .into_response();
        }
    }
    let language = body["language"].as_str().map(str::to_string);
    let file = match state.session_files().get_file(sid, file_id).await {
        Ok(Some(file)) => file,
        Ok(None) => {
            return EndpointError::invalid_message(format!("session file not found: {file_id}"))
                .into_response()
        }
        Err(error) => return EndpointError::invalid_message(error.to_string()).into_response(),
    };
    if file.is_directory {
        return EndpointError::invalid_message(format!("session file not found: {file_id}"))
            .into_response();
    }
    // 读取上限先在登记大小上响亮拒绝（不整读超限文件）；落盘内容与登记不一致时
    // 由 OperationService::transcribe 按实际字节数再次核验，绝不截断冒充。
    if file.size_bytes > crate::operations::AUDIO_INPUT_MAX_BYTES as u64 {
        return EndpointError::invalid_message(format!(
            "transcription audio exceeds the {} byte input cap; refused, never truncated",
            crate::operations::AUDIO_INPUT_MAX_BYTES
        ))
        .into_response();
    }
    let audio_path = std::path::PathBuf::from(&file.real_path);
    let bytes = match tokio::task::spawn_blocking(move || std::fs::read(&audio_path)).await {
        Ok(Ok(bytes)) => bytes,
        _ => {
            return EndpointError::invalid_message(format!("session file not found: {file_id}"))
                .into_response()
        }
    };
    let now_iso = || chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let created = now_iso();
    let mut pending = json!({
        "status": "pending",
        "providerId": provider,
        "modelId": model,
        "protocolId": protocol,
        "createdAt": created,
        "updatedAt": created,
    });
    if let Some(language) = language.as_deref() {
        pending["language"] = json!(language);
    }
    let _ = state.app_events().publish(
        "voice_transcription_update",
        json!({"sessionId": session.session_id, "fileId": file_id, "transcription": pending}),
    );
    let audio = crate::operations::HostAudio {
        bytes,
        mime: file.mime.clone(),
        filename: file.filename.clone(),
    };
    let updated = now_iso();
    let outcome = operations.transcribe(audio, language.clone(), None).await;
    let transcription = match outcome {
        Ok(receipt) => {
            let mut value = json!({
                "status": "ready",
                "text": receipt.outcome.text,
                "providerId": receipt.provider,
                "modelId": receipt.model,
                "protocolId": receipt.protocol,
                "createdAt": created,
                "updatedAt": updated,
            });
            if let Some(language) = language.as_deref() {
                value["language"] = json!(language);
            }
            if let Some(duration) = receipt.outcome.duration_ms {
                value["durationMs"] = json!(duration);
            }
            value
        }
        Err(failure) => {
            let mut value = json!({
                "status": "failed",
                "providerId": provider,
                "modelId": model,
                "protocolId": protocol,
                "error": format!("{}: {}", failure.code.wire_name(), failure.message),
                "createdAt": created,
                "updatedAt": updated,
            });
            if let Some(language) = language.as_deref() {
                value["language"] = json!(language);
            }
            value
        }
    };
    let _ = state.app_events().publish(
        "voice_transcription_update",
        json!({"sessionId": session.session_id, "fileId": file_id, "transcription": transcription}),
    );
    Json(transcription).into_response()
}

pub fn router() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/media/generate", post(generate_dispatch_route))
        .route("/lingxi/v1/media/{kind}/generate", post(generate_route))
        .route("/lingxi/v1/media/tasks", get(tasks_list))
        .route("/lingxi/v1/media/tasks/batch/{batchId}", get(tasks_batch))
        .route("/lingxi/v1/media/tasks/{taskId}", get(tasks_get))
        .route("/lingxi/v1/media/tasks/{taskId}/retry", post(tasks_retry))
        .route(
            "/lingxi/v1/media/generated/open/{filename}",
            post(generated_open),
        )
        .route(
            "/lingxi/v1/media/generated/{filename}",
            get(generated_serve),
        )
        .route("/lingxi/v1/media/asr/transcribe", post(transcribe_route))
}
