//! 当前桌面聊天协议适配现役Run；权限、费用、取消和工具仍由同一内核执行。
use crate::{Principal, ServiceState};
use base64::Engine as _;
use lingxi_kernel::{
    model_exchange::{InputImage, ModelTurnOptions},
    ports::StoragePort,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::{broadcast, mpsc};

const MAX_STREAMS: usize = 1024;
const MAX_REPLAY_FRAMES: usize = 4096;
const MAX_REPLAY_BYTES: usize = 4 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptRequest {
    pub session_id: String,
    #[serde(default)]
    pub session_path: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub stream_id: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub images: Vec<ImageRequest>,
    #[serde(default)]
    pub videos: Vec<Value>,
    #[serde(default)]
    pub audios: Vec<Value>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub client_message_id: Option<String>,
    #[serde(default)]
    pub snapshot_version: Option<u64>,
    #[serde(default)]
    pub client_attempt_id: Option<String>,
    #[serde(default)]
    pub ui_context: Option<Value>,
    #[serde(default)]
    pub display_message: Option<Value>,
    #[serde(default)]
    pub session_file_refs: Vec<Value>,
    #[serde(default)]
    pub session_refs: Vec<Value>,
    #[serde(default)]
    pub agent_review_requests: Vec<Value>,
    #[serde(default)]
    pub knowledge_refs: Option<Value>,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageRequest {
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub file_id: Option<String>,
    pub mime_type: String,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdentityRequest {
    pub session_id: String,
    #[serde(default)]
    pub session_path: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AbortRequest {
    pub session_id: String,
    #[serde(default)]
    pub session_path: Option<String>,
    #[serde(default)]
    pub stream_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResumeRequest {
    pub session_id: String,
    #[serde(default)]
    pub session_path: Option<String>,
    #[serde(default)]
    pub stream_id: Option<String>,
    #[serde(default)]
    pub since_seq: u64,
    #[serde(default)]
    pub resume_token: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SteerRequest {
    #[serde(default)]
    pub request_id: Option<String>,
    pub session_id: String,
    #[serde(default)]
    pub session_path: Option<String>,
    pub text: String,
    #[serde(default)]
    pub agent_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactRequest {
    pub session_id: String,
    #[serde(default)]
    pub session_path: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
}
use crate::chat_settings::{read_session_choice, write_session_choice, ChatChoice};
struct Replay {
    stream_id: Option<String>,
    next_seq: u64,
    frames: VecDeque<Value>,
    bytes: usize,
    is_streaming: bool,
    saw_text: bool,
    tool_names: HashMap<String, String>,
    text_segments: std::collections::HashSet<String>,
    input_entry_id: Option<String>,
}
impl Default for Replay {
    fn default() -> Self {
        Self {
            stream_id: None,
            next_seq: 1,
            frames: VecDeque::new(),
            bytes: 0,
            is_streaming: false,
            saw_text: false,
            tool_names: HashMap::new(),
            text_segments: std::collections::HashSet::new(),
            input_entry_id: None,
        }
    }
}
struct Pending {
    cancelled: Arc<AtomicBool>,
    run_id: Option<String>,
}
/// 所有缓存均有限；恢复缺口通过reset/truncated显式公开。
pub struct ChatWire {
    streams: Mutex<HashMap<String, Replay>>,
    pending: Mutex<HashMap<String, Pending>>,
    preparing: Mutex<HashMap<String, Arc<AtomicBool>>>,
    media_work: Arc<tokio::sync::Semaphore>,
    tx: broadcast::Sender<Value>,
}
impl ChatWire {
    pub fn new(_home: &Path) -> Self {
        let (tx, _) = broadcast::channel(512);
        Self {
            preparing: Mutex::new(HashMap::new()),
            media_work: Arc::new(tokio::sync::Semaphore::new(4)),
            streams: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            tx,
        }
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.tx.subscribe()
    }
    /// 会话创建是生命周期通知，不占用任何模型运行的序号或重放记录。
    pub(crate) fn publish_session_created(&self, id: &str, session: Value, at_unix_ms: u64) {
        let _ = self.tx.send(json!({
            "type": "session_created", "sessionId": id,
            "sessionPath": crate::session_locator::native(id), "sessionRefVersion": 2,
            "ts": at_unix_ms, "session": session
        }));
    }
    /// R07-T07-δ6：循环状态机变更广播（现役 chat.ts 的 loop_status 分支）。
    /// 循环状态不属于任何一条流——纯 broadcast，不进 replay 缓存，前端按
    /// sessionId 写 loopStatusBySession。fields 携带扁平业务字段
    /// （status/turnCount/maxTurns/pausedReason/prompt）。
    pub(crate) fn publish_loop_status(
        &self,
        id: &str,
        snapshot: lingxi_protocol::LoopStatusSnapshot,
        at_unix_ms: u64,
    ) {
        let mut fields = serde_json::to_value(snapshot).expect("循环状态只包含可序列化字段");
        fields["type"] = json!("loop_status");
        fields["sessionId"] = json!(id);
        fields["sessionPath"] = json!(crate::session_locator::native(id));
        fields["sessionRefVersion"] = json!(2);
        fields["ts"] = json!(at_unix_ms);
        let _ = self.tx.send(fields);
    }
    /// R07-T07-δ6：循环任务 prompt 的展示气泡（现役 session-coordinator
    /// recordLoopUserPrompt 的 session_user_message 事件）。无 run 上下文，
    /// 不占流序号；clientMessageId 恒 null（非客户端发起）。
    pub(crate) fn publish_loop_user_prompt(&self, id: &str, text: &str, at_unix_ms: u64) {
        let _ = self.tx.send(json!({
            "type": "session_user_message",
            "sessionId": id,
            "sessionPath": crate::session_locator::native(id),
            "sessionRefVersion": 2,
            "ts": at_unix_ms,
            "clientMessageId": Value::Null,
            "message": {"text": text, "timestamp": at_unix_ms},
        }));
    }
    fn publish(&self, id: &str, run: &str, mut frame: Value) {
        let mut streams = self.streams.lock().unwrap_or_else(|e| e.into_inner());
        // 入口先预留容量，活动观察器的条目不得被逐出。
        debug_assert!(
            streams.contains_key(id),
            "chat stream must be reserved before publication"
        );
        let stream = streams.entry(id.into()).or_default();
        if stream.stream_id.as_deref() != Some(run) {
            *stream = Replay {
                stream_id: Some(run.into()),
                ..Replay::default()
            };
        }
        let seq = stream.next_seq;
        stream.next_seq = stream.next_seq.saturating_add(1);
        frame["sessionId"] = json!(id);
        frame["sessionPath"] = json!(format!("native:{id}"));
        frame["sessionRefVersion"] = json!(2);
        frame["ts"] = json!(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0));
        frame["streamId"] = json!(run);
        frame["turnId"] = json!(run);
        frame["seq"] = json!(seq);
        if frame["type"] == "session_user_message" {
            stream.input_entry_id = frame["message"]["sourceEntryId"]
                .as_str()
                .map(str::to_string);
        }
        if matches!(
            frame["type"].as_str(),
            Some("assistant_run_start" | "assistant_run_end")
        ) {
            if let Some(input) = &stream.input_entry_id {
                frame["turnInputEntryId"] = json!(input);
            }
        }
        if frame["type"] == "status" {
            stream.is_streaming = frame["isStreaming"] == true;
        }
        if frame["type"] == "assistant_segment_start" && frame["kind"] == "text" {
            if let Some(segment) = frame["segmentId"].as_str() {
                stream.text_segments.insert(segment.into());
            }
        }
        if frame["type"] == "assistant_segment_delta"
            && frame["segmentId"]
                .as_str()
                .is_some_and(|id| stream.text_segments.contains(id))
        {
            stream.saw_text = true;
        }
        if frame["type"] == "tool_start" {
            if let (Some(key), Some(name)) = (frame["id"].as_str(), frame["name"].as_str()) {
                stream.tool_names.insert(key.into(), name.into());
            }
        }
        stream.bytes = stream.bytes.saturating_add(frame.to_string().len());
        stream.frames.push_back(frame.clone());
        while stream.frames.len() > MAX_REPLAY_FRAMES || stream.bytes > MAX_REPLAY_BYTES {
            if let Some(old) = stream.frames.pop_front() {
                stream.bytes = stream.bytes.saturating_sub(old.to_string().len());
            }
        }
        let _ = self.tx.send(frame);
    }
    fn project(&self, id: &str, event: &lingxi_protocol::EventEnvelope) {
        let wire = serde_json::to_value(event).expect("事件可序列化");
        let Some(run) = wire["runId"].as_str() else {
            return;
        };
        let payload = &wire["payload"];
        let kind = payload["type"].as_str().unwrap_or("");
        match kind {
            "run_state_changed"=>match payload["to"].as_str(){
                Some("running")=>{self.publish(id,run,json!({"type":"status","isStreaming":true}));self.publish(id,run,json!({"type":"assistant_run_start","runId":run}));},
                Some("failed"|"cancelled")=>{
                    self.publish(id,run,json!({"type":"assistant_run_end","runId":run,"reason":payload["reason"]}));
                    self.publish(id,run,json!({"type":"status","isStreaming":false,"aborted":payload["to"]=="cancelled"}));
                    if payload["to"]=="failed" {self.publish(id,run,json!({"type":"error","code":"run_failed","message":payload["reason"]}));}
                },
                Some("completed") if payload["reason"]!="completed.with_final"=>{self.publish(id,run,json!({"type":"assistant_run_end","runId":run}));self.publish(id,run,json!({"type":"status","isStreaming":false}));},
                _=>{}
            },
            "model_call_started"=>self.publish(id,run,json!({"type":"stage","phase":"model","modelCallId":payload["modelCallId"]})),
            "assistant_segment_start"|"assistant_segment_delta"|"assistant_segment_end"=>self.publish(id,run,payload.clone()),
            "tool_call_started"=>self.publish(id,run,json!({"type":"tool_start","id":payload["toolCall"]["toolCallId"],"name":payload["toolCall"]["target"],"args":{"summary":payload["toolCall"]["argsSummary"]}})),
            "tool_call_completed"=>{
                let name=self.streams.lock().unwrap_or_else(|e|e.into_inner()).get(id).and_then(|s|payload["toolCallId"].as_str().and_then(|key|s.tool_names.get(key))).cloned();
                self.publish(id,run,json!({"type":"tool_end","id":payload["toolCallId"],"name":name,"success":payload["result"]["status"]=="success","status":if payload["result"]["status"]=="success"{json!("succeeded")}else if payload["result"]["status"]=="unknown"{json!("unknown")}else{json!("failed")},"error":payload["result"]["error"]["message"],"details":payload["result"]["details"],"content":payload["result"]["content"]}));
            },
            "final_message_committed"=>{
                let saw=self.streams.lock().unwrap_or_else(|e|e.into_inner()).get(id).is_some_and(|s|s.saw_text);
                if !saw {for block in payload["message"]["content"].as_array().into_iter().flatten(){if block["type"]=="text" {self.publish(id,run,json!({"type":"assistant_segment_start","segmentId":"final","kind":"text","semanticPhase":"final_answer"}));self.publish(id,run,json!({"type":"assistant_segment_delta","segmentId":"final","delta":block["text"],"semanticPhase":"final_answer"}));self.publish(id,run,json!({"type":"assistant_segment_end","segmentId":"final","semanticPhase":"final_answer"}));}}}
                self.publish(id,run,json!({"type":"assistant_run_end","runId":run}));self.publish(id,run,json!({"type":"status","isStreaming":false}));
            },_=>{}
        }
    }
}

fn protocol_error(id: &str, code: &str, message: impl Into<String>) -> Value {
    json!({"type":"error","sessionId":id,"sessionPath":format!("native:{id}"),"code":code,"message":message.into()})
}
async fn owned(
    state: &ServiceState,
    p: &Principal,
    id: &str,
    path: Option<&str>,
    write: bool,
) -> Result<crate::sessions::SessionFacts, Value> {
    let resolved = crate::session_locator::resolve(state.storage(), Some(id), path)
        .await
        .map_err(|e| protocol_error(id, "session_identity_mismatch", e.to_string()))?;
    let facts = crate::get_session_owned(state, p, &resolved)
        .await
        .map_err(|_| protocol_error(id, "session_forbidden", "session unavailable or not owned"))?;
    if !p.has_scope("chat") {
        return Err(protocol_error(id, "forbidden", "chat scope required"));
    }
    if write {
        crate::gate_agent_not_deleted(state, &facts.agent_id)
            .map_err(|_| protocol_error(id, "agent_deleted", "agent deleted"))?;
        state
            .memory()
            .agent_dir_checked(&facts.agent_id)
            .map_err(|e| protocol_error(id, "agent_unavailable", e.message))?;
        let row = lingxi_adapters::storage::session_tree::get_session_row(state.storage(), id)
            .await
            .map_err(|e| protocol_error(id, "storage_error", e.to_string()))?;
        if row.is_none_or(|r| r.lifecycle != "active") {
            return Err(protocol_error(
                id,
                "session_not_active",
                "session not active",
            ));
        }
    }
    Ok(facts)
}
async fn reject(tx: &mpsc::Sender<Value>, req: &PromptRequest, error: Value) {
    let _ = tx.send(error.clone()).await;
    let _=tx.send(json!({"type":"input_rejected","outcome":"not_accepted","sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"clientMessageId":req.client_message_id,"snapshotVersion":req.snapshot_version,"clientAttemptId":req.client_attempt_id,"code":error["code"],"message":error["message"],"retryable":error["code"]=="session_busy"})).await;
}
// 与现役 shared/image-mime.ts 的 20MiB base64 字符上限一致。
const MAX_IMAGE_BYTES: u64 = 15 * 1024 * 1024;
pub(crate) fn image_bytes(id: &str, mime: &str, bytes: Vec<u8>) -> Result<InputImage, Value> {
    let error = |code, reason| protocol_error(id, code, reason);
    if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(error(
            "image_too_large",
            "image empty or exceeds the 20MiB base64 limit",
        ));
    }
    let valid = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    };
    if !valid {
        return Err(error(
            "invalid_image_content",
            "image bytes do not match MIME",
        ));
    }
    Ok(InputImage {
        bytes,
        mime: mime.into(),
    })
}
fn validate_images(req: &PromptRequest) -> Result<(), Value> {
    let error = |code, reason| protocol_error(&req.session_id, code, reason);
    if req.images.len() > 10 {
        return Err(error("max_images", "at most 10 images"));
    }
    if !req.videos.is_empty() {
        return Err(error(
            "unsupported_video_format",
            "current chat input has no video decoder; input was not accepted",
        ));
    }
    if !req.audios.is_empty() {
        return Err(error(
            "unsupported_audio_format",
            "current chat input has no audio decoder; input was not accepted",
        ));
    }
    for image in &req.images {
        if image.kind.as_deref().is_some_and(|kind| kind != "image") {
            return Err(error("unsupported_image_format", "invalid image type"));
        }
        if !matches!(
            image.mime_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp" | "image/gif"
        ) {
            return Err(error("unsupported_image_format", "unsupported image MIME"));
        }
        match (&image.data, &image.file_id) {
            (Some(data), None) => {
                if data.len() > (MAX_IMAGE_BYTES as usize).div_ceil(3) * 4 {
                    return Err(error(
                        "image_too_large",
                        "image exceeds the 20MiB base64 limit",
                    ));
                }
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|_| error("invalid_image_content", "invalid image base64"))?;
                image_bytes(&req.session_id, &image.mime_type, bytes)?;
            }
            (None, Some(id)) if !id.is_empty() => {}
            _ => {
                return Err(error(
                    "invalid_image_reference",
                    "exactly one of data or fileId is required",
                ));
            }
        }
    }
    Ok(())
}
/// 引用图片与文本文件共用会话登记和 payload 租约；不接受模型自报的磁盘路径。
async fn prepare_images(
    state: &ServiceState,
    req: &PromptRequest,
    cancelled: &Arc<AtomicBool>,
) -> Result<Vec<InputImage>, Value> {
    let mut images = Vec::new();
    for image in &req.images {
        if cancelled.load(Ordering::SeqCst) {
            return Err(protocol_error(
                &req.session_id,
                "user_abort",
                "input stopped during image preparation",
            ));
        }
        let bytes = if let Some(data) = &image.data {
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|_| {
                    protocol_error(
                        &req.session_id,
                        "invalid_image_content",
                        "invalid image base64",
                    )
                })?
        } else {
            let view = state
                .session_files()
                .get_file(&req.session_id, image.file_id.as_deref().unwrap_or(""))
                .await
                .map_err(|error| {
                    protocol_error(&req.session_id, "image_unavailable", error.to_string())
                })?
                .ok_or_else(|| {
                    protocol_error(
                        &req.session_id,
                        "image_not_owned",
                        "image is not owned by this session",
                    )
                })?;
            if view.status != "available" || view.is_directory {
                return Err(protocol_error(
                    &req.session_id,
                    "image_unavailable",
                    "image must be an available regular file",
                ));
            }
            let permit = state
                .chat_wire
                .media_work
                .clone()
                .try_acquire_owned()
                .map_err(|_| {
                    protocol_error(
                        &req.session_id,
                        "input_capacity",
                        "image preparation capacity exceeded",
                    )
                })?;
            let lease = state.session_files().payload_lease().await;
            let observer = state.session_files().payload_read_observer();
            let cancel = cancelled.clone();
            tokio::task::spawn_blocking(move || {
                let (_permit, _lease) = (permit, lease);
                if let Some(observer) = observer {
                    observer();
                }
                if cancel.load(Ordering::SeqCst) {
                    return Err("image preparation stopped".to_string());
                }
                crate::file_safety::Snapshot::read_bounded(
                    Path::new(&view.real_path),
                    MAX_IMAGE_BYTES,
                )
                .map(|snapshot| snapshot.bytes)
                .map_err(|error| match error {
                    crate::file_safety::WriteError::Failed(text)
                        if text == crate::file_safety::READ_BOUND_EXCEEDED =>
                    {
                        text
                    }
                    other => format!("image snapshot refused: {other:?}"),
                })
            })
            .await
            .map_err(|error| {
                protocol_error(&req.session_id, "image_read_failed", error.to_string())
            })?
            .map_err(|error| {
                protocol_error(
                    &req.session_id,
                    if cancelled.load(Ordering::SeqCst) {
                        "user_abort"
                    } else if error == crate::file_safety::READ_BOUND_EXCEEDED {
                        "image_too_large"
                    } else {
                        "image_read_failed"
                    },
                    error,
                )
            })?
        };
        if cancelled.load(Ordering::SeqCst) {
            return Err(protocol_error(
                &req.session_id,
                "user_abort",
                "input stopped during image preparation",
            ));
        }
        images.push(image_bytes(&req.session_id, &image.mime_type, bytes)?);
    }
    Ok(images)
}

fn prompt_from_steer(req: SteerRequest) -> PromptRequest {
    PromptRequest {
        session_id: req.session_id,
        session_path: req.session_path,
        agent_id: req.agent_id,
        stream_id: None,
        text: req.text,
        images: vec![],
        videos: vec![],
        audios: vec![],
        skills: vec![],
        client_message_id: req.request_id,
        snapshot_version: None,
        client_attempt_id: None,
        ui_context: None,
        display_message: None,
        session_file_refs: vec![],
        session_refs: vec![],
        agent_review_requests: vec![],
        knowledge_refs: None,
    }
}
fn knowledge(req: &PromptRequest) -> Result<Option<crate::sessions::KnowledgeSelection>, Value> {
    let Some(v) = req.knowledge_refs.as_ref() else {
        return Ok(None);
    };
    if !v.is_object()
        || v.as_object().is_some_and(|o| {
            o.keys()
                .any(|k| k != "notebookIds" && k != "sourceIds" && k != "mode")
        })
        || v.get("mode").is_some_and(|mode| mode != "auto")
    {
        return Err(protocol_error(
            &req.session_id,
            "invalid_knowledge_refs",
            "invalid knowledge selection",
        ));
    }
    let selection:crate::sessions::KnowledgeSelection=serde_json::from_value(json!({"notebookIds":v["notebookIds"],"sourceIds":v.get("sourceIds").cloned().unwrap_or(json!([]))})).map_err(|e|protocol_error(&req.session_id,"invalid_knowledge_refs",e.to_string()))?;
    selection
        .canonicalized()
        .map(Some)
        .map_err(|e| protocol_error(&req.session_id, "invalid_knowledge_refs", e.to_string()))
}
/// 准备期也有真实取消身份；占位一直保留到提交或失败，停止不能穿过空档。
struct Preparation {
    wire: Arc<ChatWire>,
    session: String,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Preparation {
    fn drop(&mut self) {
        if let Ok(mut rows) = self.wire.preparing.lock() {
            if rows
                .get(&self.session)
                .is_some_and(|value| Arc::ptr_eq(value, &self.cancelled))
            {
                rows.remove(&self.session);
            }
        }
    }
}
fn reserve_preparation(state: &ServiceState, id: &str) -> Result<Preparation, Value> {
    let mut rows = state
        .chat_wire
        .preparing
        .lock()
        .map_err(|_| protocol_error(id, "internal", "preparation lock poisoned"))?;
    if rows.contains_key(id) {
        return Err(protocol_error(
            id,
            "session_busy",
            "another input is being prepared",
        ));
    }
    if rows.len() >= MAX_STREAMS {
        return Err(protocol_error(
            id,
            "chat_capacity",
            "input preparation registry is full",
        ));
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    rows.insert(id.into(), cancelled.clone());
    Ok(Preparation {
        wire: state.chat_wire.clone(),
        session: id.into(),
        cancelled,
    })
}
fn pending_reserve(
    state: &ServiceState,
    id: &str,
    cancelled: Arc<AtomicBool>,
) -> Result<Arc<AtomicBool>, Value> {
    let mut pending = state
        .chat_wire
        .pending
        .lock()
        .map_err(|_| protocol_error(id, "internal", "pending lock poisoned"))?;
    if pending.contains_key(id) {
        return Err(protocol_error(
            id,
            "session_busy",
            "session has an active chat submission",
        ));
    }
    if pending.len() >= MAX_STREAMS {
        return Err(protocol_error(
            id,
            "chat_capacity",
            "chat submission registry full",
        ));
    }
    let mut streams = state
        .chat_wire
        .streams
        .lock()
        .map_err(|_| protocol_error(id, "internal", "stream lock poisoned"))?;
    if !streams.contains_key(id) && streams.len() >= MAX_STREAMS {
        let idle = streams
            .iter()
            .find(|(key, s)| !s.is_streaming && !pending.contains_key(*key))
            .map(|(id, _)| id.clone());
        if let Some(old) = idle {
            streams.remove(&old);
        } else {
            return Err(protocol_error(
                id,
                "chat_capacity",
                "chat replay registry full",
            ));
        }
    }
    streams.entry(id.into()).or_default();
    pending.insert(
        id.into(),
        Pending {
            cancelled: cancelled.clone(),
            run_id: None,
        },
    );
    Ok(cancelled)
}
fn release_pending(state: &ServiceState, id: &str, token: &Arc<AtomicBool>) {
    if let Ok(mut pending) = state.chat_wire.pending.lock() {
        if pending
            .get(id)
            .is_some_and(|p| Arc::ptr_eq(&p.cancelled, token))
        {
            pending.remove(id);
        }
    }
}
/// 接收只对实际会话生效，取消已排队提交会立即回闲，但持权直到真实任务清理。
pub async fn prompt(
    state: ServiceState,
    p: Principal,
    req: PromptRequest,
    interject: bool,
    tx: mpsc::Sender<Value>,
) {
    let facts = match owned(
        &state,
        &p,
        &req.session_id,
        req.session_path.as_deref(),
        true,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            reject(&tx, &req, e).await;
            return;
        }
    };
    if req
        .agent_id
        .as_ref()
        .is_some_and(|id| id != &facts.agent_id)
    {
        reject(
            &tx,
            &req,
            protocol_error(
                &req.session_id,
                "agent_identity_mismatch",
                "agent identity mismatch",
            ),
        )
        .await;
        return;
    }
    if let Err(e) = validate_images(&req) {
        reject(&tx, &req, e).await;
        return;
    }
    if req.text.trim().is_empty()
        && req.images.is_empty()
        && req.skills.is_empty()
        && req.session_file_refs.is_empty()
    {
        reject(
            &tx,
            &req,
            protocol_error(
                &req.session_id,
                "empty_input",
                "empty input was not submitted",
            ),
        )
        .await;
        return;
    }
    if !req.agent_review_requests.is_empty() {
        reject(
            &tx,
            &req,
            protocol_error(
                &req.session_id,
                "unsupported_input_context",
                "agent review is not connected to this chat input",
            ),
        )
        .await;
        return;
    }
    if req.text.len() > crate::limits::MAX_SUBMISSION_INPUT_BYTES
        || req.skills.len() > 64
        || req
            .skills
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 256)
    {
        reject(
            &tx,
            &req,
            protocol_error(
                &req.session_id,
                "invalid_input",
                "input or skills exceed bound",
            ),
        )
        .await;
        return;
    }
    let (session_reference_context, session_refs) =
        match prepare_session_references(&state, &p, &req).await {
            Ok(value) => value,
            Err(error) => {
                reject(&tx, &req, error).await;
                return;
            }
        };
    let quoted_text = match req
        .display_message
        .as_ref()
        .and_then(|value| value.get("quotedText"))
    {
        Some(Value::String(text)) if text.len() <= crate::limits::MAX_SUBMISSION_INPUT_BYTES => {
            Some(text.clone())
        }
        None => None,
        _ => {
            reject(
                &tx,
                &req,
                protocol_error(
                    &req.session_id,
                    "invalid_quote",
                    "quoted text must be bounded text",
                ),
            )
            .await;
            return;
        }
    };
    let selection = match knowledge(&req) {
        Ok(v) => v,
        Err(e) => {
            reject(&tx, &req, e).await;
            return;
        }
    };
    let preparation = match reserve_preparation(&state, &req.session_id) {
        Ok(value) => value,
        Err(error) => {
            reject(&tx, &req, error).await;
            return;
        }
    };
    let images = match prepare_images(&state, &req, &preparation.cancelled).await {
        Ok(value) => value,
        Err(error) => {
            reject(&tx, &req, error).await;
            return;
        }
    };
    let (file_context, files) = match prepare_files(&state, &req, &preparation.cancelled).await {
        Ok(value) => value,
        Err(error) => {
            reject(&tx, &req, error).await;
            return;
        }
    };
    let choice = match read_session_choice(&state, &facts) {
        Ok(v) => v,
        Err(e) => {
            reject(
                &tx,
                &req,
                protocol_error(&req.session_id, "chat_settings_error", e),
            )
            .await;
            return;
        }
    };
    let dispatch = match crate::chat_settings::dispatch_choice(&state, &choice) {
        Ok(value) => value,
        Err(error) => {
            reject(
                &tx,
                &req,
                protocol_error(&req.session_id, "model_not_available", error),
            )
            .await;
            return;
        }
    };
    if !images.is_empty() && dispatch.capabilities.image_input != Some(true) {
        reject(
            &tx,
            &req,
            protocol_error(
                &req.session_id,
                "image_input_not_available",
                "selected model has not declared image input",
            ),
        )
        .await;
        return;
    }
    if let Err(error) = crate::chat_settings::check_thinking(&state, &choice) {
        reject(
            &tx,
            &req,
            protocol_error(&req.session_id, "model_thinking_not_available", error),
        )
        .await;
        return;
    }
    let options = ModelTurnOptions {
        model: choice.model,
        thinking_level: choice.thinking_level,
    };
    if state.model_gateway().is_none() {
        reject(
            &tx,
            &req,
            protocol_error(
                &req.session_id,
                "model_not_configured",
                "chat model is not configured",
            ),
        )
        .await;
        return;
    }
    if interject {
        let live = match state.storage().list_active_runs().await {
            Ok(v) => v,
            Err(e) => {
                reject(
                    &tx,
                    &req,
                    protocol_error(&req.session_id, "storage_error", e.to_string()),
                )
                .await;
                return;
            }
        };
        if req.stream_id.as_ref().is_some_and(|expected| {
            !live
                .iter()
                .any(|run| run.session_id == req.session_id && &run.run_id == expected)
        }) {
            reject(
                &tx,
                &req,
                protocol_error(
                    &req.session_id,
                    "stale_stream",
                    "interjection target is no longer the active stream",
                ),
            )
            .await;
            return;
        }
        for run in live.into_iter().filter(|r| r.session_id == req.session_id) {
            match state
                .sessions
                .cancel_run_for(state.storage().as_ref(), state.runs(), &p, &run.run_id)
                .await
            {
                Ok(
                    crate::sessions::CancelRunOutcome::Accepted { .. }
                    | crate::sessions::CancelRunOutcome::AlreadyTerminal { .. },
                ) => {}
                other => {
                    reject(
                        &tx,
                        &req,
                        protocol_error(
                            &req.session_id,
                            "interject_cancel_refused",
                            format!("{other:?}"),
                        ),
                    )
                    .await;
                    return;
                }
            }
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                let row = state
                    .storage()
                    .load_run(&lingxi_protocol::RunId::new(&run.run_id))
                    .await;
                if matches!(row,Ok(Some(ref r)) if r.status.is_terminal()) {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    reject(
                        &tx,
                        &req,
                        protocol_error(
                            &req.session_id,
                            "interject_cleanup_pending",
                            "previous run cleanup has not completed",
                        ),
                    )
                    .await;
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
        // 旧流观察器只在看到真实终态后释放，不把取消受理冒称完成。
        for _ in 0..200 {
            if !state
                .chat_wire
                .pending
                .lock()
                .is_ok_and(|p| p.contains_key(&req.session_id))
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }
    let cancelled = match pending_reserve(&state, &req.session_id, preparation.cancelled.clone()) {
        Ok(v) => v,
        Err(e) => {
            reject(&tx, &req, e).await;
            return;
        }
    };
    let cursor = match state
        .storage()
        .stream_resume_slice(&req.session_id, lingxi_protocol::Seq::new(0), 1)
        .await
    {
        Ok(v) => v.head.map_or(0, |s| s.value()),
        Err(e) => {
            release_pending(&state, &req.session_id, &cancelled);
            reject(
                &tx,
                &req,
                protocol_error(&req.session_id, "storage_error", e.to_string()),
            )
            .await;
            return;
        }
    };
    let mut text = req.text.clone();
    if !session_reference_context.is_empty() {
        text.push_str("\n\n");
        text.push_str(&session_reference_context);
    }
    if !file_context.is_empty() {
        text.push_str("\n\n");
        text.push_str(&file_context);
    }
    if text.len() > crate::limits::MAX_SUBMISSION_INPUT_BYTES {
        release_pending(&state, &req.session_id, &cancelled);
        reject(
            &tx,
            &req,
            protocol_error(
                &req.session_id,
                "input_too_large",
                "prepared attachment text exceeds input limit",
            ),
        )
        .await;
        return;
    }
    if !req.skills.is_empty() {
        text = format!(
            "{}\n{text}",
            req.skills
                .iter()
                .map(|skill| format!("[Use skill: {skill}]"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    let submission = crate::sessions::ExecuteSubmission {
        input: &text,
        request_id: req.client_message_id.as_deref(),
        knowledge: selection.as_ref(),
    };
    if cancelled.load(Ordering::SeqCst) {
        release_pending(&state, &req.session_id, &cancelled);
        reject(
            &tx,
            &req,
            protocol_error(
                &req.session_id,
                "user_abort",
                "submission stopped before run start",
            ),
        )
        .await;
        return;
    }
    let accepted = state
        .sessions
        .execute_foreground_chat_for(
            state.storage(),
            state.events(),
            state.runs(),
            &state.background,
            &p,
            &req.session_id,
            &submission,
            state.clock().now_unix_ms(),
            Some(crate::runs::ChatRunInput {
                images,
                options,
                files,
                client_message_id: req.client_message_id.clone(),
                snapshot_version: req.snapshot_version,
                quoted_text,
                session_refs,
                display_text: Some(
                    req.display_message
                        .as_ref()
                        .and_then(|value| value.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or(&req.text)
                        .to_string(),
                ),
            }),
        )
        .await;
    let accepted = match accepted {
        Ok(v) => v,
        Err(e) => {
            release_pending(&state, &req.session_id, &cancelled);
            let code = match e {
                crate::sessions::SessionExecuteError::Busy => "session_busy",
                _ => "input_not_accepted",
            };
            reject(
                &tx,
                &req,
                protocol_error(&req.session_id, code, format!("{e:?}")),
            )
            .await;
            return;
        }
    };
    if let Ok(mut pending) = state.chat_wire.pending.lock() {
        if let Some(v) = pending.get_mut(&req.session_id) {
            v.run_id = Some(accepted.run_id.clone());
        }
    }
    if cancelled.load(Ordering::SeqCst) {
        let _ = state
            .sessions
            .cancel_run_for(state.storage().as_ref(), state.runs(), &p, &accepted.run_id)
            .await;
    }
    tokio::spawn(observe_run(
        state,
        p,
        req,
        accepted.run_id,
        cursor,
        cancelled,
        tx,
    ));
}

async fn observe_run(
    state: ServiceState,
    p: Principal,
    req: PromptRequest,
    run: String,
    mut after: u64,
    cancelled: Arc<AtomicBool>,
    tx: mpsc::Sender<Value>,
) {
    let mut user_sent = false;
    let mut terminal = false;
    loop {
        let slice = match state
            .storage()
            .stream_resume_slice(&req.session_id, lingxi_protocol::Seq::new(after), 256)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                let _ = tx
                    .send(protocol_error(
                        &req.session_id,
                        "stream_storage_error",
                        e.to_string(),
                    ))
                    .await;
                break;
            }
        };
        if slice
            .floor
            .is_some_and(|f| f.value() > after.saturating_add(1))
        {
            let _=tx.send(json!({"type":"stream_resume","sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"streamId":run,"sinceSeq":0,"nextSeq":1,"reset":true,"truncated":true,"isStreaming":true,"runtimeIsStreaming":true,"events":[]})).await;
        }
        if !user_sent {
            let id = format!("user:{run}");
            match state
                .storage()
                .query_one_text(
                    "SELECT content_json FROM messages WHERE session_id=?1 AND message_id=?2",
                    vec![req.session_id.clone(), id.clone()],
                )
                .await
            {
                Ok(Some(content)) => match serde_json::from_str::<Value>(&content) {
                    Ok(value) => {
                        let mut attachments =
                            value["attachments"].as_array().cloned().unwrap_or_default();
                        let image_count = value["images"].as_array().map_or(0, Vec::len);
                        for index in 0..image_count {
                            attachments.push(json!({"path":format!("native-image-pending:{id}:{index}"),"name":format!("图片 {}（读取中）",index+1),"isDir":false}));
                        }
                        state.chat_wire.publish(&req.session_id,&run,json!({"type":"session_user_message","clientMessageId":req.client_message_id,"snapshotVersion":req.snapshot_version,"message":{"id":id,"sourceEntryId":id,"clientMessageId":req.client_message_id,"snapshotVersion":req.snapshot_version,"text":value["text"],"attachments":attachments,"hasImages":image_count>0,"quotedText":value["quotedText"],"sessionRefs":value["sessionRefs"],"timestamp":state.clock().now_unix_ms()}}));
                        user_sent = true;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(protocol_error(
                                &req.session_id,
                                "stream_storage_error",
                                e.to_string(),
                            ))
                            .await;
                        break;
                    }
                },
                Ok(None) => {}
                Err(e) => {
                    let _ = tx
                        .send(protocol_error(
                            &req.session_id,
                            "stream_storage_error",
                            e.to_string(),
                        ))
                        .await;
                    break;
                }
            }
        }
        for event in &slice.events {
            after = event.seq.value();
            if event.run_id.as_ref().map(|r| r.as_str()) != Some(run.as_str()) {
                continue;
            }
            state.chat_wire.project(&req.session_id, event);
            let wire = serde_json::to_value(event).expect("事件可序列化");
            let payload = &wire["payload"];
            if payload["type"] == "final_message_committed"
                || (payload["type"] == "run_state_changed"
                    && (payload["to"] == "failed"
                        || payload["to"] == "cancelled"
                        || (payload["to"] == "completed"
                            && payload["reason"] != "completed.with_final")))
            {
                terminal = true;
            }
        }
        if terminal {
            break;
        }
        if owned(
            &state,
            &p,
            &req.session_id,
            req.session_path.as_deref(),
            false,
        )
        .await
        .is_err()
        {
            break;
        }
        if slice.head.is_some_and(|h| after < h.value()) {
            continue;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    release_pending(&state, &req.session_id, &cancelled);
}

pub async fn abort(
    state: &ServiceState,
    p: &Principal,
    req: AbortRequest,
    tx: &mpsc::Sender<Value>,
) {
    if let Err(e) = owned(
        state,
        p,
        &req.session_id,
        req.session_path.as_deref(),
        false,
    )
    .await
    {
        let _ = tx.send(e).await;
        return;
    }
    let live = match state.storage().list_active_runs().await {
        Ok(rows) => rows.into_iter().find(|r| r.session_id == req.session_id),
        Err(e) => {
            let _ = tx
                .send(protocol_error(
                    &req.session_id,
                    "storage_error",
                    e.to_string(),
                ))
                .await;
            return;
        }
    };
    let pending = state.chat_wire.pending.lock().ok().and_then(|p| {
        p.get(&req.session_id)
            .map(|p| (p.cancelled.clone(), p.run_id.clone()))
    });
    let stream = live
        .as_ref()
        .map(|r| r.run_id.clone())
        .or_else(|| pending.as_ref().and_then(|(_, r)| r.clone()));
    if req
        .stream_id
        .as_ref()
        .is_some_and(|requested| Some(requested) != stream.as_ref())
    {
        let _=tx.send(json!({"type":"abort_result","status":"rejected","reason":"stale_stream","sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"streamId":stream})).await;
        return;
    }
    let preparing = state
        .chat_wire
        .preparing
        .lock()
        .ok()
        .and_then(|rows| rows.get(&req.session_id).cloned());
    let mut accepted = false;
    if let Some(run) = live {
        match state
            .sessions
            .cancel_run_for(state.storage().as_ref(), state.runs(), p, &run.run_id)
            .await
        {
            Ok(crate::sessions::CancelRunOutcome::Accepted { .. }) => accepted = true,
            Ok(crate::sessions::CancelRunOutcome::AlreadyTerminal { .. }) => {}
            other => {
                let _=tx.send(json!({"type":"abort_result","status":"rejected","reason":format!("{other:?}"),"sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"streamId":stream})).await;
                return;
            }
        }
    }
    if let Some((cancelled, _)) = pending {
        cancelled.store(true, Ordering::SeqCst);
        accepted = true;
    }
    if let Some(cancelled) = preparing {
        cancelled.store(true, Ordering::SeqCst);
        accepted = true;
    }
    let _=tx.send(json!({"type":"abort_result","status":if accepted{"accepted"}else{"already_stopped"},"sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"streamId":stream})).await;
    // 受理停止就恢复输入空闲，真实Run终态另由持久事件到达，不伪造清理完成。
    let _=tx.send(json!({"type":"status","sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"streamId":stream,"isStreaming":false,"aborted":true,"reason":req.reason.unwrap_or_else(||"user_abort".into())})).await;
}

pub async fn resume(
    state: &ServiceState,
    p: &Principal,
    req: ResumeRequest,
    tx: &mpsc::Sender<Value>,
) {
    if let Err(e) = owned(
        state,
        p,
        &req.session_id,
        req.session_path.as_deref(),
        false,
    )
    .await
    {
        let _ = tx.send(e).await;
        return;
    }
    if req.since_seq > 9_007_199_254_740_991 {
        let _ = tx
            .send(protocol_error(
                &req.session_id,
                "invalid_sequence",
                "sinceSeq must be a safe integer",
            ))
            .await;
        return;
    }
    let active = match state.storage().list_active_runs().await {
        Ok(rows) => rows.into_iter().find(|r| r.session_id == req.session_id),
        Err(e) => {
            let _ = tx
                .send(protocol_error(
                    &req.session_id,
                    "storage_error",
                    e.to_string(),
                ))
                .await;
            return;
        }
    };
    let result = {
        let streams = state
            .chat_wire
            .streams
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match streams.get(&req.session_id) {
            Some(s) if s.stream_id.is_some() => {
                let reset = req
                    .stream_id
                    .as_ref()
                    .is_some_and(|r| Some(r) != s.stream_id.as_ref())
                    || req.since_seq >= s.next_seq;
                let since = if reset { 0 } else { req.since_seq };
                let floor = s
                    .frames
                    .front()
                    .and_then(|v| v["seq"].as_u64())
                    .unwrap_or(s.next_seq);
                let truncated = since.saturating_add(1) < floor;
                let frames = s
                    .frames
                    .iter()
                    .filter(|v| v["seq"].as_u64().is_some_and(|seq| seq > since))
                    .map(|v| json!({"seq":v["seq"],"event":v,"ts":v["ts"]}))
                    .collect::<Vec<_>>();
                json!({"type":"stream_resume","sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"streamId":s.stream_id,"sinceSeq":since,"nextSeq":s.next_seq,"reset":reset,"truncated":truncated,"isStreaming":s.is_streaming,"runtimeIsStreaming":active.is_some(),"events":frames,"resumeToken":req.resume_token})
            }
            _ => {
                json!({"type":"stream_resume","sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"streamId":active.as_ref().map(|r|&r.run_id),"sinceSeq":req.since_seq,"nextSeq":1,"reset":active.is_some(),"truncated":active.is_some(),"isStreaming":active.is_some(),"runtimeIsStreaming":active.is_some(),"events":[],"resumeToken":req.resume_token})
            }
        }
    };
    let _ = tx.send(result).await;
}

pub async fn steer(state: ServiceState, p: Principal, req: SteerRequest, tx: mpsc::Sender<Value>) {
    let facts = match owned(
        &state,
        &p,
        &req.session_id,
        req.session_path.as_deref(),
        true,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.send(e).await;
            return;
        }
    };
    if req
        .agent_id
        .as_ref()
        .is_some_and(|id| id != &facts.agent_id)
        || req.text.trim().is_empty()
        || req.text.len() > crate::limits::MAX_SUBMISSION_INPUT_BYTES
    {
        let _ = tx
            .send(protocol_error(
                &req.session_id,
                "invalid_steering",
                "invalid steering identity or text",
            ))
            .await;
        return;
    }
    match state
        .browsers()
        .replay_human_steer(&p, &req.session_id, &req.text, req.request_id.as_deref())
        .await
    {
        Ok(true) => {
            let _=tx.send(json!({"type":"steered","sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"replayed":true})).await;
            return;
        }
        Ok(false) => {}
        Err(error) => {
            let _ = tx
                .send(protocol_error(
                    &req.session_id,
                    "steering_refused",
                    error.to_string(),
                ))
                .await;
            return;
        }
    }
    let active = match state.storage().list_active_runs().await {
        Ok(runs) => runs.into_iter().find(|run| {
            run.session_id.as_str() == req.session_id
                && matches!(
                    run.status,
                    lingxi_protocol::RunStatus::Running
                        | lingxi_protocol::RunStatus::WaitingApproval
                )
        }),
        Err(error) => {
            let _ = tx
                .send(protocol_error(
                    &req.session_id,
                    "steering_refused",
                    error.to_string(),
                ))
                .await;
            return;
        }
    };
    let mut browser_steer = None;
    if let Some(run) = active {
        let lineage = match state
            .storage()
            .load_run_lineage(&lingxi_protocol::RunId::new(&run.run_id))
            .await
        {
            Ok(lineage) => lineage,
            Err(error) => {
                let _ = tx
                    .send(protocol_error(
                        &req.session_id,
                        "steering_refused",
                        error.to_string(),
                    ))
                    .await;
                return;
            }
        };
        if lineage.as_ref().is_some_and(|lineage| {
            lineage.origin == lingxi_kernel::subagent::RunOrigin::User
                && lineage.parent_run_id.is_none()
        }) {
            match state
                .browsers()
                .begin_human_steer(
                    &p,
                    &req.session_id,
                    run.run_id.as_str(),
                    &req.text,
                    req.request_id.as_deref(),
                )
                .await
            {
                Ok(crate::browser::HumanSteerClaim::New { row, operation }) => {
                    browser_steer = Some((*row, operation, run.run_id.to_string()))
                }
                Ok(crate::browser::HumanSteerClaim::Replayed) => {
                    let _ = tx
                        .send(json!({"type":"steered","sessionId":req.session_id,"replayed":true}))
                        .await;
                    return;
                }
                Err(error) => {
                    let _ = tx
                        .send(protocol_error(
                            &req.session_id,
                            "steering_refused",
                            error.to_string(),
                        ))
                        .await;
                    return;
                }
            }
        }
    }
    let outcome = state
        .sessions
        .steer_for(&p, &req.session_id, &req.text)
        .await;
    if let Some((row, operation, run)) = browser_steer {
        let accepted = matches!(
            &outcome,
            Ok(crate::session_supervisor::SteerOutcome::Accepted)
        );
        if let Err(error) = state
            .browsers()
            .finish_human_steer(row, &run, &operation, accepted)
            .await
        {
            let mut message = protocol_error(
                &req.session_id,
                "steering_browser_authority_unconfirmed",
                error.to_string(),
            );
            message["inputAccepted"] = json!(accepted);
            let _ = tx.send(message).await;
            return;
        }
    }
    match outcome {
        Ok(crate::session_supervisor::SteerOutcome::Accepted) => {
            let _=tx.send(json!({"type":"steered","sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id)})).await;
        }
        Ok(crate::session_supervisor::SteerOutcome::Miss) => {
            prompt(state, p, prompt_from_steer(req), false, tx).await
        }
        Err(error) => {
            let _ = tx
                .send(protocol_error(
                    &req.session_id,
                    "steering_refused",
                    format!("{error:?}"),
                ))
                .await;
        }
    }
}

pub async fn compact(
    state: &ServiceState,
    p: &Principal,
    req: CompactRequest,
    tx: &mpsc::Sender<Value>,
) {
    let result = |status: &str, reason: &str, message: Value| json!({"type":"compaction_result","sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"status":status,"reason":reason,"message":message,"mode":if req.method.as_deref()==Some("instant_simple"){"lossy_local"}else{"cache_preserving"}});
    if let Err(e) = owned(state, p, &req.session_id, req.session_path.as_deref(), true).await {
        let _ = tx
            .send(result(
                "failed",
                "session_unavailable",
                e["message"].clone(),
            ))
            .await;
        return;
    }
    if let Some(method) = req.method.as_deref() {
        if method != "instant_simple" {
            let _ = tx
                .send(result(
                    "failed",
                    "invalid_compaction_method",
                    json!("unsupported compaction method"),
                ))
                .await;
            return;
        }
        let preferences = match state.memory().read_preferences_root() {
            Ok(v) => v,
            Err(e) => {
                let _ = tx
                    .send(result("failed", "settings_unavailable", json!(e)))
                    .await;
                return;
            }
        };
        if preferences["experiments"]["session.instant_simple_compaction"] != true {
            let _ = tx
                .send(result(
                    "failed",
                    "experiment_disabled",
                    json!("instant simple compaction experiment is disabled"),
                ))
                .await;
            return;
        }
    }
    match state.storage().list_active_runs().await {
        Ok(rows) if rows.iter().any(|r| r.session_id == req.session_id) => {
            let _ = tx
                .send(result(
                    "failed",
                    "session_streaming",
                    json!("wait for current reply"),
                ))
                .await;
            return;
        }
        Err(e) => {
            let _ = tx
                .send(result("failed", "storage_error", json!(e.to_string())))
                .await;
            return;
        }
        _ => {}
    }
    if state.compaction().is_none() || state.context_compiler().is_none() {
        let _ = tx
            .send(result(
                "failed",
                "system_not_ready",
                json!("real compaction chain unavailable"),
            ))
            .await;
        return;
    }
    let _=tx.send(json!({"type":"compaction_accepted","sessionId":req.session_id,"sessionPath":format!("native:{}",req.session_id),"mode":if req.method.as_deref()==Some("instant_simple"){"lossy_local"}else{"cache_preserving"}})).await;
    let response = if req.method.as_deref() == Some("instant_simple") {
        crate::history::history_content::instant_compact(state, p, &req.session_id).await
    } else {
        crate::history::history_content::manual_compact(state, p, &req.session_id, b"{}", false)
            .await
    };
    let success = response.status().is_success();
    let bytes = match axum::body::to_bytes(response.into_body(), 1024 * 1024).await {
        Ok(v) => v,
        Err(e) => {
            let _ = tx
                .send(result(
                    "failed",
                    "invalid_compaction_result",
                    json!(e.to_string()),
                ))
                .await;
            return;
        }
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(v) if success => {
            let status = if v["noopReason"].as_str().is_some() {
                "noop"
            } else {
                "succeeded"
            };
            let _ = tx
                .send(result(
                    status,
                    v["noopReason"].as_str().unwrap_or(""),
                    json!(v.to_string()),
                ))
                .await;
        }
        Ok(v) => {
            let _ = tx
                .send(result("failed", "compaction_failed", json!(v.to_string())))
                .await;
        }
        Err(e) => {
            let _ = tx
                .send(result(
                    "failed",
                    "invalid_compaction_result",
                    json!(e.to_string()),
                ))
                .await;
        }
    }
}

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelSetRequest {
    pub provider: String,
    pub model_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelSwitchRequest {
    pub session_id: String,
    pub provider: String,
    pub model_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThinkingRequest {
    #[serde(default)]
    pub session_id: Option<String>,
    pub level: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThinkingQuery {
    #[serde(default)]
    pub session_id: Option<String>,
}
fn setting_failure(code: StatusCode, message: impl Into<String>) -> Response {
    (code, Json(json!({"ok":false,"error":message.into()}))).into_response()
}
fn owner_key(p: &Principal) -> &str {
    p.user_id.as_deref().unwrap_or(&p.principal_id)
}
// 沿用服务层直接返回 HTTP 拒绝的约定，避免为短期错误额外堆分配。
#[allow(clippy::result_large_err)]
async fn setting_choice(
    state: &ServiceState,
    p: &Principal,
    id: Option<&str>,
) -> Result<(String, ChatChoice), Response> {
    if !p.has_scope("chat") {
        return Err(setting_failure(
            StatusCode::FORBIDDEN,
            "chat scope required",
        ));
    }
    let (owner, agent_id) = match id {
        Some(id) => {
            let facts = owned(state, p, id, None, true)
                .await
                .map_err(|e| setting_failure(StatusCode::FORBIDDEN, e.to_string()))?;
            (facts.owner_user_id, Some(facts.agent_id))
        }
        None => (owner_key(p).into(), None),
    };
    let choice = crate::chat_settings::effective_choice_for_agent(
        state,
        &owner,
        id.unwrap_or(""),
        agent_id.as_deref(),
        None,
    )
    .map_err(|e| setting_failure(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok((owner, choice))
}
#[allow(clippy::result_large_err)]
fn validate_model(state: &ServiceState, provider: &str, model: &str) -> Result<(), Response> {
    if provider.trim().is_empty()
        || model.trim().is_empty()
        || provider.len() > 256
        || model.len() > 512
    {
        return Err(setting_failure(
            StatusCode::BAD_REQUEST,
            "invalid model identity",
        ));
    }
    let gateway = state.model_gateway().ok_or_else(|| {
        setting_failure(StatusCode::SERVICE_UNAVAILABLE, "model plane unavailable")
    })?;
    let dispatch = gateway
        .resolve_dispatch(&lingxi_kernel::model_exchange::ModelRouteRequest {
            operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
            provider: Some(provider.into()),
            model: Some(model.into()),
        })
        .map_err(|e| setting_failure(StatusCode::BAD_REQUEST, e.to_string()))?;
    if dispatch.capabilities.tools != Some(true) {
        return Err(setting_failure(
            StatusCode::BAD_REQUEST,
            "model has no declared chat tool support",
        ));
    }
    Ok(())
}
pub async fn models_route(
    axum::Extension(p): axum::Extension<Principal>,
    State(state): State<ServiceState>,
) -> Response {
    let (_, choice) = match setting_choice(&state, &p, None).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let response = crate::settings_ui::models_route(State(state.clone())).await;
    let status = response.status();
    let bytes = match axum::body::to_bytes(response.into_body(), 1024 * 1024).await {
        Ok(v) => v,
        Err(e) => return setting_failure(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if !status.is_success() {
        return (status, bytes).into_response();
    }
    let mut value: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return setting_failure(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let current = choice.model.or_else(|| {
        state.model_gateway().and_then(|g| {
            g.configured_routes()
                .get("chat")
                .map(|b| (b.provider.clone(), b.model.clone()))
        })
    });
    if let Some(rows) = value["models"].as_array_mut() {
        for row in rows {
            row["isCurrent"] = json!(current
                .as_ref()
                .is_some_and(|(provider, id)| row["id"] == *id && row["provider"] == *provider));
            if let Some(gateway) = state.model_gateway() {
                if let Ok(dispatch) =
                    gateway.resolve_dispatch(&lingxi_kernel::model_exchange::ModelRouteRequest {
                        operation: lingxi_kernel::model_exchange::ModelOperation::Chat,
                        provider: row["provider"].as_str().map(str::to_string),
                        model: row["id"].as_str().map(str::to_string),
                    })
                {
                    row["reasoning"] = json!(dispatch
                        .compat
                        .as_ref()
                        .and_then(|c| c.reasoning)
                        .unwrap_or(false));
                    row["contextWindow"] =
                        json!(dispatch.compat.as_ref().and_then(|c| c.context_window));
                }
            }
        }
    }
    Json(value).into_response()
}
pub async fn model_set_route(
    axum::Extension(p): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<ModelSetRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let req = match body {
        Ok(Json(v)) => v,
        Err(e) => return crate::EndpointError::from_json_rejection(&e).into_response(),
    };
    let (owner, mut choice) = match setting_choice(&state, &p, None).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    if let Err(e) = validate_model(&state, &req.provider, &req.model_id) {
        return e;
    }
    choice.model = Some((req.provider.clone(), req.model_id.clone()));
    choice.thinking_level = match crate::chat_settings::selected_thinking(&state, &choice) {
        Ok(level) => Some(level),
        Err(error) => return setting_failure(StatusCode::BAD_REQUEST, error),
    };
    if let Err(error) = crate::chat_settings::check_thinking(&state, &choice) {
        return setting_failure(StatusCode::BAD_REQUEST, error);
    }
    match crate::chat_settings::save_choice(state.memory().home(),&owner,None,choice.clone()){Ok(())=>Json(json!({"ok":true,"model":{"id":req.model_id,"provider":req.provider},"thinkingLevel":choice.thinking_level.unwrap_or_else(||"medium".into())})).into_response(),Err(e)=>setting_failure(StatusCode::INTERNAL_SERVER_ERROR,e)}
}
pub async fn model_switch_route(
    axum::Extension(p): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<ModelSwitchRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let req = match body {
        Ok(Json(v)) => v,
        Err(e) => return crate::EndpointError::from_json_rejection(&e).into_response(),
    };
    let (_owner, mut choice) = match setting_choice(&state, &p, Some(&req.session_id)).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    // 与真实 Run 建立共用准入，不能在检查空闲后被新 Run 插入并切掉其模型快照。
    let _permit = match state
        .storage()
        .session_admission()
        .try_acquire(&[&req.session_id], usize::MAX)
    {
        Ok(permit) => permit,
        Err(_) => {
            return setting_failure(StatusCode::CONFLICT, "cannot switch model while streaming");
        }
    };
    if state
        .chat_wire
        .pending
        .lock()
        .is_ok_and(|rows| rows.contains_key(&req.session_id))
    {
        return setting_failure(
            StatusCode::CONFLICT,
            "cannot switch model during a pending submission",
        );
    }
    match state.storage().list_active_runs().await {
        Ok(rows) if rows.iter().any(|r| r.session_id == req.session_id) => {
            return setting_failure(StatusCode::CONFLICT, "cannot switch model while streaming");
        }
        Err(e) => return setting_failure(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        _ => {}
    }
    if let Err(e) = validate_model(&state, &req.provider, &req.model_id) {
        return e;
    }
    choice.model = Some((req.provider.clone(), req.model_id.clone()));
    choice.thinking_level = match crate::chat_settings::selected_thinking(&state, &choice) {
        Ok(level) => Some(level),
        Err(error) => return setting_failure(StatusCode::BAD_REQUEST, error),
    };
    if let Err(error) = crate::chat_settings::check_thinking(&state, &choice) {
        return setting_failure(StatusCode::BAD_REQUEST, error);
    }
    match crate::get_session_owned(&state,&p,&req.session_id).await.map_err(|_|"session ownership changed".to_string()).and_then(|facts|write_session_choice(&state,&facts,choice.clone())){Ok(())=>Json(json!({"ok":true,"model":{"id":req.model_id,"provider":req.provider},"thinkingLevel":choice.thinking_level.unwrap_or_else(||"medium".into())})).into_response(),Err(e)=>setting_failure(StatusCode::INTERNAL_SERVER_ERROR,e)}
}
pub async fn thinking_get_route(
    axum::Extension(p): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    Query(req): Query<ThinkingQuery>,
) -> Response {
    match setting_choice(&state, &p, req.session_id.as_deref()).await {
        Ok((_, choice)) => {
            Json(json!({"thinkingLevel":choice.thinking_level.unwrap_or_else(||"medium".into())}))
                .into_response()
        }
        Err(e) => e,
    }
}
pub async fn thinking_set_route(
    axum::Extension(p): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<ThinkingRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let req = match body {
        Ok(Json(v)) => v,
        Err(e) => return crate::EndpointError::from_json_rejection(&e).into_response(),
    };
    let (owner, mut choice) = match setting_choice(&state, &p, req.session_id.as_deref()).await {
        Ok(v) => v,
        Err(e) => return e,
    };
    let level = match req.level.to_ascii_lowercase().as_str() {
        "off" => "off",
        "low" => "low",
        "medium" | "auto" => "medium",
        "high" => "high",
        "xhigh" => "xhigh",
        "max" | "ultracode" => "max",
        _ => return setting_failure(StatusCode::BAD_REQUEST, "unknown thinking level"),
    }
    .to_string();
    choice.thinking_level = Some(level.clone());
    if let Err(error) = crate::chat_settings::check_thinking(&state, &choice) {
        return setting_failure(StatusCode::BAD_REQUEST, error);
    }
    match crate::chat_settings::save_choice(
        state.memory().home(),
        &owner,
        req.session_id.as_deref(),
        choice,
    ) {
        Ok(()) => Json(json!({"ok":true,"thinkingLevel":level})).into_response(),
        Err(e) => setting_failure(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// 引用只认当前会话登记的文件；模型材料来自受租约保护、前后身份校验的真实字节。
pub(crate) async fn prepare_files(
    state: &ServiceState,
    req: &PromptRequest,
    cancelled: &Arc<AtomicBool>,
) -> Result<(String, Vec<Value>), Value> {
    if req.session_file_refs.len() > 32 {
        return Err(protocol_error(
            &req.session_id,
            "max_files",
            "at most 32 file references",
        ));
    }
    let mut context = Vec::new();
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for reference in &req.session_file_refs {
        if cancelled.load(Ordering::SeqCst) {
            return Err(protocol_error(
                &req.session_id,
                "user_abort",
                "input stopped during preparation",
            ));
        }
        let map = reference.as_object().ok_or_else(|| {
            protocol_error(
                &req.session_id,
                "invalid_file_reference",
                "file reference must be an object",
            )
        })?;
        if map.keys().any(|key| {
            !matches!(
                key.as_str(),
                "fileId" | "sessionId" | "sessionPath" | "label" | "kind"
            )
        }) {
            return Err(protocol_error(
                &req.session_id,
                "invalid_file_reference",
                "unknown file reference field",
            ));
        }
        let id = reference["fileId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                protocol_error(&req.session_id, "invalid_file_reference", "fileId required")
            })?;
        if map
            .get("sessionId")
            .is_some_and(|value| value.as_str() != Some(req.session_id.as_str()))
            || map.get("sessionPath").is_some_and(|value| {
                value.as_str() != Some(format!("native:{}", req.session_id).as_str())
            })
        {
            return Err(protocol_error(
                &req.session_id,
                "file_session_mismatch",
                "file reference belongs to another session",
            ));
        }
        if !seen.insert(id.to_string()) {
            continue;
        }
        let view = state
            .session_files()
            .get_file(&req.session_id, id)
            .await
            .map_err(|error| {
                protocol_error(&req.session_id, "file_unavailable", error.to_string())
            })?
            .ok_or_else(|| {
                protocol_error(
                    &req.session_id,
                    "file_not_owned",
                    "file unavailable or not owned by this session",
                )
            })?;
        if view.status != "available" || view.is_directory {
            return Err(protocol_error(
                &req.session_id,
                "file_unavailable",
                "file must be an available regular file",
            ));
        }
        if view.size_bytes > crate::file_safety::MAX_RECOVERABLE_FILE_BYTES {
            return Err(protocol_error(
                &req.session_id,
                "file_too_large",
                "safe attachment snapshot exceeds 8MiB",
            ));
        }
        let permit = state
            .chat_wire
            .media_work
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                protocol_error(
                    &req.session_id,
                    "input_capacity",
                    "attachment conversion capacity exceeded",
                )
            })?;
        let lease = state.session_files().payload_lease().await;
        let read_view = view.clone();
        let cancel = cancelled.clone();
        let observer = state.session_files().payload_read_observer();
        let text=tokio::task::spawn_blocking(move||->Result<String,String>{
            let (_permit,_lease)=(permit,lease);
            if cancel.load(Ordering::SeqCst){return Err("input stopped during preparation".into());}
            if let Some(observer)=observer{observer();}
            if cancel.load(Ordering::SeqCst){return Err("input stopped during preparation".into());}
            let path=std::path::Path::new(&read_view.real_path);
            let snapshot=crate::file_safety::Snapshot::read(path).map_err(|error|format!("attachment snapshot refused: {error:?}"))?;
            let extension=path.extension().and_then(|v|v.to_str()).unwrap_or("").to_ascii_lowercase();
            let text=match extension.as_str(){"docx"=>crate::office::docx(&snapshot.bytes)?.text,"xlsx"=>crate::office::xlsx(&snapshot.bytes)?.text,_=>{
                if snapshot.bytes.contains(&0){return Err("unsupported attachment content: binary data has no connected text decoder".into());}
                String::from_utf8(snapshot.bytes).map_err(|_|"unsupported attachment encoding; UTF-8 required".to_string())?
            }};
            if cancel.load(Ordering::SeqCst){return Err("input stopped during preparation".into());}
            Ok(text)
        }).await.map_err(|error|protocol_error(&req.session_id,"file_read_failed",error.to_string()))?.map_err(|error|protocol_error(&req.session_id,if cancelled.load(Ordering::SeqCst){"user_abort"}else{"file_read_failed"},error))?;
        context.push(format!(
            "[SessionFile] {}\n<attachment-content>\n{text}\n</attachment-content>",
            json!({"fileId":view.file_id,"sessionId":req.session_id,"filename":view.filename})
        ));
        if context.iter().map(String::len).sum::<usize>()
            > crate::limits::MAX_SUBMISSION_INPUT_BYTES
        {
            return Err(protocol_error(
                &req.session_id,
                "input_too_large",
                "attachment text exceeds input limit",
            ));
        }
        files.push(json!({"fileId":view.file_id,"path":view.real_path,"filePath":view.file_path,"name":view.label.as_deref().unwrap_or(&view.filename),"mimeType":view.mime,"isDir":false,"status":"available"}));
    }
    Ok((context.join("\n\n"), files))
}

/// 与现役桌面一致，只把已授权的会话 ID 交给模型；不暗中读取来源会话正文。
async fn prepare_session_references(
    state: &ServiceState,
    principal: &Principal,
    req: &PromptRequest,
) -> Result<(String, Vec<Value>), Value> {
    if req.session_refs.len() > 32 {
        return Err(protocol_error(
            &req.session_id,
            "invalid_session_reference",
            "at most 32 session references",
        ));
    }
    let mut normalized = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for value in &req.session_refs {
        let invalid = || {
            protocol_error(
                &req.session_id,
                "invalid_session_reference",
                "session reference must contain bounded sessionId and label",
            )
        };
        let object = value.as_object().ok_or_else(invalid)?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "sessionId" | "label"))
        {
            return Err(invalid());
        }
        let id = value["sessionId"]
            .as_str()
            .map(str::trim)
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .ok_or_else(invalid)?;
        let label = match object.get("label") {
            None => "Session",
            Some(Value::String(label)) if label.len() <= 512 => label.trim(),
            _ => return Err(invalid()),
        };
        crate::get_session_owned(state, principal, id)
            .await
            .map_err(|_| {
                protocol_error(
                    &req.session_id,
                    "session_reference_forbidden",
                    "referenced session unavailable or not owned",
                )
            })?;
        if seen.insert(id.to_owned()) {
            normalized.push(json!({"sessionId":id,"label":label}));
        }
    }
    if normalized.is_empty() {
        return Ok((String::new(), normalized));
    }
    let mut lines = vec!["[用户引用的 Session]".to_string()];
    lines.extend(normalized.iter().map(|reference| {
        format!(
            "- {}: {}",
            reference["label"].as_str().unwrap_or("Session"),
            reference["sessionId"].as_str().unwrap_or_default()
        )
    }));
    lines.push("这些只是 Session ID 引用。是否读取由你根据任务自行判断；需要时调用 session read/Resolver。".into());
    Ok((lines.join("\n"), normalized))
}
