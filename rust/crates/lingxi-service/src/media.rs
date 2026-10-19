//! 媒体工具的持久作业；正式运行、外部调用与交付物使用现有宿主链。
use crate::{
    operations::{
        ImageGenerationOutcome, MediaTaskStatus, OperationCallContext, OperationService,
        RegisteredProduct,
    },
    sessionfiles::SessionFileService,
    ServiceClock, ServiceState,
};
use lingxi_adapters::{
    models::operations::{
        image::ImageRequest, speech::SpeechRequest, video::VideoRequest, ImageReference,
    },
    storage::{
        media as store,
        media::{MediaClaim, MediaJob, MediaPhase},
        session_tree, RunDatabase,
    },
};
use lingxi_kernel::{
    ports::{StorageError, ToolRequest},
    RunContext,
};
use lingxi_protocol::ToolCallId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
    time::Duration,
};

#[path = "media_source_authority.rs"]
mod source_authority;

pub const MEDIA_JOB_TIMEOUT_MS: u64 = 20 * 60 * 1000;
pub const MEDIA_JOB_CAP: usize = 32;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionImage {
    pub kind: String,
    pub file_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaSpec {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<SessionImage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_images: Vec<SessionImage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_filename: Option<String>,
    #[serde(default)]
    pub options: BTreeMap<String, Value>,
}
impl MediaSpec {
    pub fn parse(kind: &str, value: &Value) -> Result<Self, String> {
        // 现役高级选项允许同名参数；用户明确给出的顶层值优先。
        let mut normalized = value.clone();
        if let Some(input) = normalized.as_object_mut() {
            if let Some(mut options) = input.get("options").and_then(Value::as_object).cloned() {
                let aliases = if kind == "image" {
                    vec![
                        ("n", "count"),
                        ("ratio", "ratio"),
                        ("resolution", "resolution"),
                        ("resolution_type", "resolution"),
                        ("quality", "quality"),
                        ("format", "format"),
                        ("mode", "mode"),
                    ]
                } else if kind == "video" {
                    vec![
                        ("duration", "duration"),
                        ("ratio", "ratio"),
                        ("resolution", "resolution"),
                        ("video_resolution", "resolution"),
                        ("mode", "mode"),
                    ]
                } else {
                    vec![]
                };
                for (key, field) in aliases {
                    if let Some(value) = options.remove(key) {
                        let valid = match field {
                            "count" => value.as_u64().is_some_and(|v| (1..=10).contains(&v)),
                            "duration" => value
                                .as_f64()
                                .is_some_and(|v| v.is_finite() && v > 0.0 && v <= 600.0),
                            _ => value.as_str().is_some_and(|v| {
                                !v.is_empty() && v.len() <= 256 && !v.chars().any(char::is_control)
                            }),
                        };
                        if !valid {
                            return Err("媒体高级选项格式不正确".into());
                        }
                        if input.get(field).is_none_or(Value::is_null) {
                            input.insert(field.into(), value);
                        }
                    }
                }
                input.insert("options".into(), json!(options));
            }
        }
        let spec: Self =
            serde_json::from_value(normalized).map_err(|_| "媒体参数格式不正确或含不支持的字段")?;
        if spec.prompt.trim().is_empty()
            || spec.prompt.encode_utf16().count() > 32_000
            || !matches!(kind, "image" | "video" | "speech")
            || spec.count.is_some_and(|n| !(1..=10).contains(&n))
            || spec.reference_images.len() > 10
        {
            return Err("媒体提示词、数量或参考图超出支持范围".into());
        }
        for value in [
            &spec.provider,
            &spec.model,
            &spec.mode,
            &spec.ratio,
            &spec.resolution,
            &spec.quality,
            &spec.voice,
            &spec.format,
        ]
        .into_iter()
        .flatten()
        {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err("媒体选项无效".into());
            }
        }
        if spec.model.is_some() && spec.provider.is_none() {
            return Err("指定模型时必须同时指定供应商".into());
        }
        if spec
            .duration
            .is_some_and(|v| !v.is_finite() || v <= 0.0 || v > 600.0)
            || spec
                .speed
                .is_some_and(|v| !v.is_finite() || v <= 0.0 || v > 4.0)
        {
            return Err("媒体时长或语速无效".into());
        }
        for reference in spec.image.iter().chain(&spec.reference_images) {
            if reference.kind != "session_file"
                || !reference.file_id.starts_with("sf_")
                || reference.file_id.len() > 256
            {
                return Err("参考图必须是当前会话的文件".into());
            }
        }
        if let Some(name) = &spec.suggested_filename {
            if name.is_empty()
                || name.len() > 200
                || name == "."
                || name == ".."
                || name
                    .chars()
                    .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
            {
                return Err("媒体文件名无效".into());
            }
        }
        const KNOWN: &[&str] = &[
            "size",
            "format",
            "style",
            "background",
            "output_compression",
            "moderation",
            "watermark",
            "guidance_scale",
            "seed",
            "negative_prompt",
            "prompt_extend",
            "prompt_optimizer",
            "width",
            "height",
            "responses_model",
            "frame_rate",
            "num_frames",
        ];
        if spec.options.keys().any(|k| !KNOWN.contains(&k.as_str()))
            || serde_json::to_vec(&spec.options)
                .map_err(|_| "媒体选项无法编码")?
                .len()
                > 8192
        {
            return Err("媒体高级选项不受支持".into());
        }
        if kind == "video"
            && spec.options.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "size" | "width" | "height" | "frame_rate" | "num_frames"
                )
            })
        {
            return Err("视频高级选项含当前执行链不支持的字段".into());
        }
        if kind == "image"
            && spec
                .options
                .keys()
                .any(|key| matches!(key.as_str(), "frame_rate" | "num_frames"))
        {
            return Err("图片生成不接受视频帧参数".into());
        }
        for (key, value) in &spec.options {
            let valid = match key.as_str() {
                "size" | "format" | "style" | "background" | "moderation" | "negative_prompt"
                | "responses_model" => value.as_str().is_some_and(|v| {
                    !v.is_empty() && v.len() <= 8192 && !v.chars().any(char::is_control)
                }),
                "watermark" | "prompt_extend" | "prompt_optimizer" => value.is_boolean(),
                "output_compression" | "width" | "height" => {
                    value.as_u64().is_some_and(|n| n <= u32::MAX as u64)
                }
                "seed" => value.as_i64().is_some(),
                "guidance_scale" | "frame_rate" | "num_frames" => {
                    value.as_f64().is_some_and(f64::is_finite)
                }
                _ => false,
            };
            if !valid {
                return Err(format!("媒体高级参数 {key} 的值无效"));
            }
        }
        let refs = usize::from(spec.image.is_some()) + spec.reference_images.len();
        if refs > 10 {
            return Err("参考图合计不能超过十张".into());
        }
        let inferred = match (kind, refs > 0) {
            ("image", false) => "text2image",
            ("image", true) => "image2image",
            ("video", false) => "text2video",
            ("video", true) => "image2video",
            _ => "speech",
        };
        if spec.mode.as_deref().is_some_and(|mode| mode != inferred) {
            return Err("媒体模式与实际参考图不一致".into());
        }
        if kind == "video"
            && (refs > 1
                || spec.count.is_some()
                || spec.quality.is_some()
                || spec.voice.is_some()
                || spec.speed.is_some()
                || spec.format.is_some())
        {
            return Err("视频参数不受支持".into());
        }
        if kind == "image"
            && (spec.duration.is_some() || spec.voice.is_some() || spec.speed.is_some())
        {
            return Err("图片参数不受支持".into());
        }
        if kind == "speech"
            && (spec.image.is_some()
                || !spec.reference_images.is_empty()
                || spec.count.is_some()
                || spec.duration.is_some()
                || !spec.options.is_empty())
        {
            return Err("语音生成不接受图片、数量、时长或高级图片选项".into());
        }
        Ok(spec)
    }
}
type Active = Arc<Mutex<BTreeMap<String, Arc<AtomicBool>>>>;
struct ActiveGuard {
    id: String,
    active: Active,
    armed: bool,
}
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if self.armed {
            self.active.lock().expect("媒体活动锁").remove(&self.id);
        }
    }
}
pub struct MediaService {
    pub(crate) db: Arc<RunDatabase>,
    pub(crate) files: Arc<SessionFileService>,
    io: Arc<crate::resourceio::ResourceIoService>,
    memory: Arc<crate::memory::MemoryService>,
    operations: Option<Arc<OperationService>>,
    jimeng: OnceLock<Arc<crate::media_jimeng::JimengRuntime>>,
    clock: Arc<dyn ServiceClock>,
    events: Arc<crate::appevents::AppEventLog>,
    run_events: Arc<crate::events::EventService>,
    runs: Weak<crate::runs::RunSupervisor>,
    active: Active,
    drivers: Mutex<BTreeMap<String, crate::task_supervisor::ChildHandle<Result<(), String>>>>,
    cleanup_failures: Mutex<BTreeMap<String, String>>,
    stopping: AtomicBool,
    authority_gate: Arc<tokio::sync::RwLock<()>>,
}
impl MediaService {
    pub fn new(state: &ServiceState) -> Arc<Self> {
        Arc::new(Self {
            db: state.storage().clone(),
            files: state.session_files().clone(),
            io: state.resource_io().clone(),
            memory: state.memory().clone(),
            operations: state.wired_operations().cloned(),
            jimeng: OnceLock::new(),
            clock: state.clock().clone(),
            events: state.app_events().clone(),
            run_events: state.events().clone(),
            runs: Arc::downgrade(state.runs()),
            active: Arc::new(Mutex::new(BTreeMap::new())),
            drivers: Mutex::new(BTreeMap::new()),
            cleanup_failures: Mutex::new(BTreeMap::new()),
            stopping: AtomicBool::new(false),
            authority_gate: Arc::new(tokio::sync::RwLock::new(())),
        })
    }
    /// 正常权限设置写链在落盘与取消期间持有此租约，和媒体最终发布使用同一门。
    /// 受信宿主与集成检查共用真实修改锁，不新增任何对外授权入口。
    #[doc(hidden)]
    pub async fn authority_mutation_lease(&self) -> tokio::sync::OwnedRwLockWriteGuard<()> {
        self.authority_gate.clone().write_owned().await
    }
    /// 组合根仅在宿主已配置独立程序与外部登录许可后接入；模型参数不能设置执行器。
    pub fn set_jimeng(
        &self,
        runtime: Arc<crate::media_jimeng::JimengRuntime>,
    ) -> Result<(), String> {
        self.jimeng
            .set(runtime)
            .map_err(|_| "即梦执行器已配置".into())
    }
    pub fn enabled(&self, agent: &str, plugin: &str) -> Result<(), String> {
        let path = self.memory.config_path(agent).map_err(|e| e.to_string())?;
        let config = lingxi_adapters::storage::agent_memory::config::read_agent_config(&path)
            .map_err(|e| e.to_string())?;
        if config
            .string_list("tools", "disabled")
            .map_err(|e| e.to_string())?
            .unwrap_or_default()
            .iter()
            .any(|v| v == plugin)
        {
            return Err(format!("{plugin} 已停用"));
        }
        Ok(())
    }
    pub fn review_route(&self, kind: &str, spec: &MediaSpec) -> Result<Value, String> {
        if spec.provider.as_deref() == Some("jimeng-cli") {
            return self
                .jimeng
                .get()
                .ok_or("即梦 CLI 外部登录许可尚未配置")?
                .review_route(kind, spec);
        }
        self.operations
            .as_ref()
            .ok_or("媒体生成服务尚未配置")?
            .media_route(kind, spec.provider.clone(), spec.model.clone())
            .and_then(|route| {
                self.operations
                    .as_ref()
                    .expect("已核媒体服务")
                    .validate_media_spec(kind, spec, &route)?;
                Ok(route)
            })
            .map(|route| route.to_value())
            .map_err(|e| e.to_string())
    }
    pub async fn describe_options(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        args: &Value,
    ) -> Result<Value, String> {
        let kind = args["kind"].as_str().ok_or("媒体类型未提供")?;
        if !matches!(kind, "image" | "video") {
            return Err("媒体类型不受支持".into());
        }
        if args["provider"] == "jimeng-cli" {
            let runtime = self.jimeng.get().ok_or("即梦 CLI 外部登录许可尚未配置")?;
            self.enabled(
                &session_tree::get_session_row(&self.db, ctx.session_id.as_str())
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("媒体会话不存在")?
                    .agent_id,
                "jimeng-cli",
            )?;
            runtime
                .refresh_capabilities(ctx, call, &Arc::new(AtomicBool::new(false)))
                .await?;
            let description = runtime.describe(kind)?;
            let models = description["models"]
                .as_array()
                .ok_or("即梦真实模型能力损坏")?;
            let wanted = args["model"]
                .as_str()
                .or_else(|| description["defaultModelId"].as_str())
                .ok_or("即梦没有已确认默认模型")?;
            let model = models
                .iter()
                .find(|model| {
                    model["id"] == wanted
                        || model["aliases"]
                            .as_array()
                            .is_some_and(|aliases| aliases.iter().any(|alias| alias == wanted))
                })
                .ok_or("模型不在实际即梦能力列表中")?;
            let mode = args["mode"].as_str().unwrap_or(if kind == "image" {
                "text2image"
            } else {
                "text2video"
            });
            let selected_mode = model["modes"]
                .as_array()
                .and_then(|modes| modes.iter().find(|selected| selected["id"] == mode))
                .ok_or("此即梦模型不支持指定生成模式")?;
            return Ok(
                json!({"kind":kind,"providerId":"jimeng-cli","providerName":"即梦 CLI","model":model,"mode":selected_mode,"providers":[description],"selection":{"selectionPolicy":"configured_default","overrideRequired":false,"defaultConfigured":false},"remoteDownload":"query_result 内部执行下载；宿主单独核验实际文件"}),
            );
        }
        self.operations
            .as_ref()
            .ok_or("媒体生成服务尚未配置")?
            .describe_media(
                kind,
                args["provider"].as_str().map(String::from),
                args["model"].as_str().map(String::from),
                args["mode"].as_str().map(String::from),
            )
            .map_err(|e| e.to_string())
    }
    pub async fn caller(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        request: &ToolRequest,
    ) -> Result<String, String> {
        if request.arguments.digest() != request.args_digest {
            return Err("媒体调用参数与批准内容不一致".into());
        }
        let actual=self.db.query_one_text("SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id JOIN run_attempts a ON a.run_id=r.run_id JOIN invocation_journal j ON j.run_id=r.run_id AND j.journal_id=?7 WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND r.generation=?5 AND a.attempt=?6 AND a.generation=r.generation AND r.status='running' AND s.lifecycle='active' AND j.phase='started' AND j.session_id=r.session_id AND j.owner_kind=r.owner_kind AND j.owner_subject=r.owner_subject AND j.target=?8 AND j.args_digest=?9 AND j.generation=r.generation AND j.attempt=a.attempt AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1)",vec![ctx.run_id.to_string(),ctx.session_id.to_string(),ctx.principal.storage_kind().into(),ctx.principal.storage_subject(),ctx.generation.to_string(),ctx.attempt.to_string(),call.to_string(),request.target.clone(),request.args_digest.hex.clone()]).await.map_err(|e|e.to_string())?;
        if actual.is_none() {
            return Err("媒体调用缺少当前真实任务与执行回执".into());
        }
        let session = session_tree::get_session_row(&self.db, ctx.session_id.as_str())
            .await
            .map_err(|e| e.to_string())?
            .ok_or("会话不存在")?;
        if !ctx
            .principal
            .can_access_session(&session.owner_user_id, ctx.session_id.as_str())
        {
            return Err("会话访问权限已改变".into());
        }
        self.enabled(&session.agent_id, "media")?;
        self.files
            .ensure_stage_session(ctx)
            .await
            .map_err(|e| e.to_string())?;
        Ok(session.owner_user_id)
    }
    pub async fn submit(
        self: &Arc<Self>,
        ctx: &RunContext,
        call: &ToolCallId,
        request: &ToolRequest,
        kind: &str,
    ) -> Result<Value, String> {
        if self.stopping.load(Ordering::Acquire) {
            return Err("媒体服务正在退出，未受理新生成请求".into());
        }
        if request.target != format!("tool:plugin:media:generate-{kind}") {
            return Err("媒体动作与正式调用目标不一致".into());
        }
        let owner = self.caller(ctx, call, request).await?;
        let spec = MediaSpec::parse(kind, request.arguments.as_value())?;
        let route = self.review_route(kind, &spec)?;
        let bound = crate::toolgateway::review_scope().ok_or("媒体生成缺少正式批准范围")?;
        if bound["mediaRoute"] != route {
            return Err("媒体默认模型或供应商在确认后已改变，请重新确认".into());
        }
        if route["provider"] == "jimeng-cli" {
            let session = session_tree::get_session_row(&self.db, ctx.session_id.as_str())
                .await
                .map_err(|e| e.to_string())?
                .ok_or("媒体会话不存在")?;
            self.enabled(&session.agent_id, "jimeng-cli")?;
        }
        let session = session_tree::get_session_row(&self.db, ctx.session_id.as_str())
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体会话不存在")?;
        let source_mode = self
            .runs
            .upgrade()
            .ok_or("媒体运行服务已关闭")?
            .task_supervisor()
            .parent_session_mode(
                ctx.run_id.as_str(),
                ctx.session_id.as_str(),
                &session.agent_id,
                &ctx.principal,
                self.db.session_admission(),
            )
            .map_err(|e| e.to_string())?
            .ok_or("媒体来源缺少实际冻结权限许可")?
            .wire_name();
        if source_mode == "read_only" || session.permission_mode.as_deref() == Some("read_only") {
            return Err("当前会话只读，未提交媒体生成".into());
        }
        let operation = format!(
            "media-{}",
            lingxi_protocol::canon::sha256_hex(
                json!([ctx.run_id.as_str(), call.as_str(), kind])
                    .to_string()
                    .as_bytes()
            )
        );
        let now = self.clock.now_unix_ms();
        let job = MediaJob {
            operation_id: operation.clone(),
            session_id: ctx.session_id.to_string(),
            owner_subject: owner,
            source_run_id: ctx.run_id.to_string(),
            execution_run_id: None,
            tool_call_id: call.to_string(),
            kind: kind.into(),
            provider: route["provider"]
                .as_str()
                .ok_or("媒体路由供应商不存在")?
                .into(),
            request_digest: request.args_digest.hex.clone(),
            spec: serde_json::to_value(spec).map_err(|_| "媒体参数无法保存")?,
            state: json!({"route":route,"sourcePermissionMode":source_mode,"remoteOutcome":"not_yet_confirmed"}),
            phase: MediaPhase::Submitting,
            revision: 1,
            created_at: now,
            updated_at: now,
        };
        let job = match store::claim(&self.db, job)
            .await
            .map_err(|e| e.to_string())?
        {
            MediaClaim::Existing(job) => return public_job(&job),
            MediaClaim::New(job) => job,
        };
        match self.launch(job.clone(), Some((ctx, call))).await {
            Ok(job) => public_job(&job),
            Err(error) => {
                self.fail_open(&job.operation_id, &error).await?;
                let actual = store::get(&self.db, &job.operation_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("媒体作业已不存在")?;
                public_job(&actual)
            }
        }
    }
    async fn launch(
        self: &Arc<Self>,
        job: MediaJob,
        parent: Option<(&RunContext, &ToolCallId)>,
    ) -> Result<MediaJob, String> {
        if self.stopping.load(Ordering::Acquire) {
            return Err("媒体服务正在退出".into());
        }
        self.reap_finished().await?;
        let runs = self.runs.upgrade().ok_or("运行服务已关闭")?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut active = self.active.lock().expect("媒体活动锁");
            if active.len() >= MEDIA_JOB_CAP {
                return Err("媒体后台任务已达上限".into());
            }
            if active.contains_key(&job.operation_id) {
                return Err("媒体任务已经在运行".into());
            }
            active.insert(job.operation_id.clone(), cancel.clone());
        }
        // 受理等待被上游取消时也释放本地占位；真正业务权力来自正式媒体 Run。
        let mut reservation = ActiveGuard {
            id: job.operation_id.clone(),
            active: self.active.clone(),
            armed: true,
        };
        let permit = match parent {
            Some((ctx, call)) => {
                runs.admit_media_job(
                    &self.db,
                    &self.run_events,
                    ctx,
                    call,
                    &job.operation_id,
                    &job.request_digest,
                    self.clock.now_unix_ms(),
                )
                .await?
            }
            None => {
                runs.admit_media_resume(&self.db, &self.run_events, &job, self.clock.now_unix_ms())
                    .await?
            }
        };
        let job = match store::bind_execution(
            &self.db,
            &job,
            permit.context.run_id.as_str(),
            self.clock.now_unix_ms(),
        )
        .await
        {
            Ok(job) => job,
            Err(error) => {
                runs.finish_media_job(
                    &self.db,
                    &self.run_events,
                    permit,
                    Err("媒体执行绑定失败".into()),
                    self.clock.now_unix_ms(),
                )
                .await?;
                return Err(error.to_string());
            }
        };
        // 监督器拒绝注册时，调用者仍保有凭据并走统一收尾。
        let permit = Arc::new(Mutex::new(Some(permit)));
        let task_permit = permit.clone();
        let this = self.clone();
        let driver_runs = runs.clone();
        let drive_job = job.clone();
        let id = job.operation_id.clone();
        let handle = runs
            .task_supervisor()
            .spawn_detached(format!("media:{id}"), async move {
                let permit = task_permit
                    .lock()
                    .expect("媒体执行凭据锁")
                    .take()
                    .ok_or("媒体执行凭据已被消费")?;
                let _guard = ActiveGuard {
                    id: drive_job.operation_id.clone(),
                    active: this.active.clone(),
                    armed: true,
                };
                let result = this
                    .drive(drive_job.clone(), permit.context.clone(), cancel)
                    .await;
                let receipt = match &result {
                    Ok(()) => Ok(()),
                    Err(error) => this.fail_open(&drive_job.operation_id, error).await,
                };
                let finish = driver_runs
                    .finish_media_job(
                        &this.db,
                        &this.run_events,
                        permit,
                        result,
                        this.clock.now_unix_ms(),
                    )
                    .await;
                receipt?;
                finish
            });
        let handle = match handle {
            Ok(handle) => handle,
            Err(error) => {
                let pending = permit.lock().expect("媒体执行凭据锁").take();
                if let Some(pending) = pending {
                    runs.finish_media_job(
                        &self.db,
                        &self.run_events,
                        pending,
                        Err("媒体驱动未受理".into()),
                        self.clock.now_unix_ms(),
                    )
                    .await?;
                }
                return Err(error.to_string());
            }
        };
        self.drivers.lock().expect("媒体驱动锁").insert(id, handle);
        reservation.armed = false;
        Ok(job)
    }
    pub async fn reap_finished(&self) -> Result<(), String> {
        self.reap_selected(None).await
    }
    async fn reap_selected(
        &self,
        selected: Option<&std::collections::BTreeSet<String>>,
    ) -> Result<(), String> {
        let finished = {
            let mut drivers = self.drivers.lock().expect("媒体驱动锁");
            let ids = drivers
                .iter()
                .filter(|(id, h)| {
                    h.is_finished() && selected.is_none_or(|selected| selected.contains(*id))
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| drivers.remove(&id).map(|h| (id, h)))
                .collect::<Vec<_>>()
        };
        for (id, handle) in finished {
            let error = match handle.wait().await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(format!("媒体作业 {id} 统一收尾失败：{error}")),
                Err(exit) => {
                    let reason = format!("媒体执行退出，收尾未确认：{}", exit.name());
                    let update = self.fail_open(&id, &reason).await;
                    Some(match update {
                        Ok(()) => reason,
                        Err(error) => format!("{reason}；持久回执写入失败：{error}"),
                    })
                }
            };
            if let Some(error) = error {
                self.cleanup_failures
                    .lock()
                    .expect("媒体清理结果锁")
                    .insert(id, error);
            }
        }
        // 已移除的线程句柄不能抹去未确认事实；后续退出或会话清理仍须明确失败。
        let errors = self
            .cleanup_failures
            .lock()
            .expect("媒体清理结果锁")
            .iter()
            .filter(|(id, _)| selected.is_none_or(|selected| selected.contains(*id)))
            .map(|(_, message)| message.clone())
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }
    pub async fn recover(self: &Arc<Self>) -> Result<(), String> {
        for job in store::list_open(&self.db)
            .await
            .map_err(|e| e.to_string())?
        {
            if job.phase == MediaPhase::Submitting || job.phase == MediaPhase::Downloading {
                let mut state = job.state.clone();
                let not_submitted = job.phase == MediaPhase::Submitting
                    && job.state["dispatchStarted"] != true
                    && job.state["tracker"].as_str().is_none();
                if not_submitted {
                    state["errorCode"] = json!("MEDIA_RESTART_NOT_SUBMITTED");
                    state["message"] =
                        json!("服务重启前尚未发出生成请求；已关闭此任务，不会自动重新提交");
                    state["remoteOutcome"] = json!("not_submitted");
                    self.update(&job, MediaPhase::Failed, state).await?;
                } else {
                    state["errorCode"] = json!("MEDIA_RESTART_UNKNOWN");
                    state["message"] = json!(if job.phase == MediaPhase::Downloading {
                        "远端生成已确认完成；重启时本地交付未确认，已登记文件仍保留，不会重复提交"
                    } else {
                        "服务重启前未拿到提交回执，远端结果未确认，不会重复提交"
                    });
                    if job.phase == MediaPhase::Downloading {
                        state["localDeliveryUnknown"] = json!(true);
                    }
                    if state["remoteOutcome"] != "completed" {
                        state["remoteOutcome"] = json!("unknown");
                    }
                    self.update(&job, MediaPhase::Unknown, state).await?;
                }
            } else {
                self.launch(job, None).await?;
            }
        }
        Ok(())
    }
    pub async fn get_owned(
        &self,
        id: &str,
        sid: &str,
        owner: &str,
    ) -> Result<Option<Value>, String> {
        let job = store::get(&self.db, id).await.map_err(|e| e.to_string())?;
        job.filter(|j| j.session_id == sid && j.owner_subject == owner)
            .map(|job| public_job(&job))
            .transpose()
    }
    pub async fn list_owned(&self, sid: &str, owner: &str) -> Result<Vec<Value>, String> {
        store::list_session(&self.db, sid, owner)
            .await
            .map_err(|e| e.to_string())?
            .iter()
            .map(public_job)
            .collect()
    }
    pub async fn cancel_owned(&self, id: &str, sid: &str, owner: &str) -> Result<Value, String> {
        loop {
            let job = store::get(&self.db, id)
                .await
                .map_err(|e| e.to_string())?
                .filter(|job| job.session_id == sid && job.owner_subject == owner)
                .ok_or("媒体任务不存在")?;
            if job.phase.terminal() {
                return public_job(&job);
            }
            let mut state = job.state.clone();
            state["localDeliveryStopped"] = json!(true);
            state["remoteCancel"] = json!("unsupported");
            let completed = job.state["remoteOutcome"] == "completed";
            let not_submitted =
                job.phase == MediaPhase::Submitting && job.state["dispatchStarted"] != true;
            state["remoteOutcome"] = json!(if completed {
                "completed"
            } else if not_submitted {
                "not_submitted"
            } else {
                "unknown"
            });
            state["message"] = json!(if completed {
                "已停止后续本地交付；远端生成已完成，已交付文件仍保留"
            } else if not_submitted {
                "生成尚未提交，已取消本地任务"
            } else {
                "已停止本地等待与交付；供应商端任务可能继续，远端结果未确认"
            });
            // 先线性化持久取消；发布事务会核对此终态，不能在返回取消后再交付。
            match self.update(&job, MediaPhase::Cancelled, state).await {
                Ok(cancelled) => {
                    if let Some(flag) = self.active.lock().expect("媒体活动锁").get(id) {
                        flag.store(true, Ordering::Release);
                    }
                    if let (Some(runs), Some(run)) =
                        (self.runs.upgrade(), job.execution_run_id.as_deref())
                    {
                        let _ = runs.cancel_run(run, "media job cancelled by owner");
                    }
                    if let (Some(operations), Some(task)) =
                        (&self.operations, job.state["tracker"].as_str())
                    {
                        let _ = operations.cancel_media_task(task);
                    }
                    return public_job(&cancelled);
                }
                Err(error) => {
                    let actual = store::get(&self.db, id)
                        .await
                        .map_err(|e| e.to_string())?
                        .ok_or("媒体任务不存在")?;
                    if actual.revision == job.revision {
                        return Err(error);
                    }
                    // 新阶段已先落库时重读再取消；真正终态只回放原回执。
                }
            }
        }
    }
    /// 普通任务停止也先落媒体取消事实，再触发真实执行根的取消。
    pub async fn cancel_from_run(&self, run_id: &str) -> Result<(), String> {
        let runs = self.runs.upgrade().ok_or("媒体执行监督器已退出")?;
        let root = runs
            .run_scope(run_id)
            .ok_or("待停止任务没有真实取消作用域")?;
        let jobs = store::list_open(&self.db)
            .await
            .map_err(|e| e.to_string())?;
        for job in jobs {
            let linked = job.source_run_id == run_id
                || job.execution_run_id.as_deref() == Some(run_id)
                || job
                    .execution_run_id
                    .as_deref()
                    .and_then(|execution| runs.run_scope(execution))
                    .is_some_and(|scope| scope.descends_from(&root));
            if linked {
                self.cancel_owned(&job.operation_id, &job.session_id, &job.owner_subject)
                    .await?;
            }
        }
        Ok(())
    }
    async fn authority(&self, job: &MediaJob, cancel: &AtomicBool) -> Result<(), String> {
        if cancel.load(Ordering::Acquire) {
            return Err("媒体任务已取消；远端结果未确认".into());
        }
        let session = session_tree::get_session_row(&self.db, &job.session_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体会话已删除")?;
        if session.owner_user_id != job.owner_subject
            || session.lifecycle != "active"
            || session.permission_mode.as_deref() == Some("read_only")
        {
            return Err("媒体会话权限或状态已改变".into());
        }
        self.enabled(&session.agent_id, "media")?;
        source_authority::validate(&self.db, job, self.memory.home()).await?;
        if job.provider == "jimeng-cli" {
            self.enabled(&session.agent_id, "jimeng-cli")?;
            self.jimeng
                .get()
                .ok_or("即梦执行器已不可用")?
                .validate_route(&job.state["route"])?;
        }
        let execution = job
            .execution_run_id
            .as_ref()
            .ok_or("媒体作业尚未绑定正式执行任务")?;
        let actual=self.db.query_one_text("SELECT r.run_id FROM runs r JOIN run_lineage l ON l.run_id=r.run_id JOIN sessions s ON s.session_id=r.session_id JOIN media_jobs_v1 j ON j.execution_run_id=r.run_id JOIN runs p ON p.run_id=j.source_run_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_subject=p.owner_subject AND r.owner_kind=p.owner_kind AND s.owner_user_id=?3 AND s.lifecycle='active' AND r.status='running' AND l.origin='media' AND l.parent_run_id=?4 AND l.cause_id=?5 AND l.source_message_id=?6 AND j.operation_id=?5 AND j.owner_subject=?3 AND j.phase IN('submitting','submitted','generating','downloading')",vec![execution.clone(),job.session_id.clone(),job.owner_subject.clone(),job.source_run_id.clone(),job.operation_id.clone(),job.tool_call_id.clone()]).await.map_err(|e|e.to_string())?;
        if actual.is_none() {
            return Err("媒体后台任务执行权已结束或改变；远端结果未确认".into());
        }
        let runs = self.runs.upgrade().ok_or("媒体运行服务已关闭")?;
        let scope = runs
            .run_scope(execution)
            .ok_or("媒体执行任务的受控生命周期已缺失")?;
        if scope.is_cancelled() {
            return Err("媒体执行任务已取消；远端结果未确认".into());
        }
        Ok(())
    }
    async fn update(
        &self,
        job: &MediaJob,
        phase: MediaPhase,
        state: Value,
    ) -> Result<MediaJob, String> {
        let next = store::transition(&self.db, job, phase, state, self.clock.now_unix_ms())
            .await
            .map_err(|e| e.to_string())?;
        let _ = self.events.publish(
            "media_generation_update",
            json!({"sessionId":job.session_id,"mediaGeneration":public_job(&next)?}),
        );
        Ok(next)
    }
    async fn fail_open(&self, id: &str, error: &str) -> Result<(), String> {
        let Some(job) = store::get(&self.db, id).await.map_err(|e| e.to_string())? else {
            return Ok(());
        };
        if job.phase.terminal() {
            return Ok(());
        }
        let mut state = job.state.clone();
        if self.stopping.load(Ordering::Acquire)
            && matches!(job.phase, MediaPhase::Submitted | MediaPhase::Generating)
            && job.state["tracker"].as_str().is_some()
        {
            state["pausedForRestart"] = json!(true);
            state["message"] =
                json!("服务退出时保留已确认的远端任务编号；重启只查询结果，不重复提交");
            self.update(&job, job.phase.clone(), state).await?;
            return Ok(());
        }
        let dispatched = job.state["dispatchStarted"].as_bool().unwrap_or(false)
            || job.phase != MediaPhase::Submitting;
        state["errorCode"] = json!(if dispatched {
            "MEDIA_RESULT_UNKNOWN"
        } else {
            "MEDIA_NOT_DISPATCHED"
        });
        state["message"] = json!(error);
        state["remoteOutcome"] = json!(if job.state["remoteOutcome"] == "completed" {
            "completed"
        } else if dispatched {
            "unknown"
        } else {
            "not_submitted"
        });
        if let Err(error) = self
            .update(
                &job,
                if dispatched {
                    MediaPhase::Unknown
                } else {
                    MediaPhase::Failed
                },
                state,
            )
            .await
        {
            let current = store::get(&self.db, id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("媒体作业不存在")?;
            if !current.phase.terminal() {
                return Err(error);
            }
        }
        Ok(())
    }
    /// 会话归档或删除前只清理本会话的实际作业，不改变其他会话或全局开关。
    pub async fn cancel_session(&self, sid: &str, budget: Duration) -> Result<Vec<String>, String> {
        let deadline = tokio::time::Instant::now() + budget;
        let Some(session) = session_tree::get_session_row(&self.db, sid)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(vec![]);
        };
        let registered = self
            .drivers
            .lock()
            .expect("媒体驱动锁")
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let registered = registered
            .into_iter()
            .chain(
                self.cleanup_failures
                    .lock()
                    .expect("媒体清理结果锁")
                    .keys()
                    .cloned(),
            )
            .collect::<std::collections::BTreeSet<_>>();
        let mut jobs = store::list_session_unsettled(&self.db, sid, &session.owner_user_id)
            .await
            .map_err(|e| e.to_string())?;
        for id in registered {
            if jobs.iter().any(|job| job.operation_id == id) {
                continue;
            }
            if let Some(job) = store::get(&self.db, &id).await.map_err(|e| e.to_string())? {
                if job.session_id == sid {
                    jobs.push(job);
                }
            }
        }
        let selected = jobs
            .iter()
            .map(|job| job.operation_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for job in &jobs {
            if job.owner_subject != session.owner_user_id {
                return Err("会话媒体作业归属不一致，清理未确认".into());
            }
            self.cancel_owned(&job.operation_id, sid, &session.owner_user_id)
                .await?;
        }
        loop {
            self.reap_selected(Some(&selected)).await?;
            let mut pending = Vec::new();
            for job in &jobs {
                let live = self
                    .drivers
                    .lock()
                    .expect("媒体驱动锁")
                    .contains_key(&job.operation_id);
                let actual = if let Some(execution) = job.execution_run_id.as_ref() {
                    self.db.query_one_text("SELECT run_id FROM runs WHERE run_id=?1 AND status NOT IN('completed','failed','cancelled','interrupted_needs_attention')",vec![execution.clone()]).await.map_err(|e|e.to_string())?.is_some()
                } else {
                    false
                };
                if live || actual {
                    pending.push(job.operation_id.clone());
                }
            }
            if pending.is_empty() || tokio::time::Instant::now() >= deadline {
                return Ok(pending);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    /// 退出信号同步关闭受理与交付；不另启脱离统一退出预算的执行器。
    pub fn begin_stop(&self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(runtime) = self.jimeng.get() {
            runtime.revoke();
        }
        for flag in self.active.lock().expect("媒体活动锁").values() {
            flag.store(true, Ordering::Release);
        }
    }
    pub async fn stop(&self, budget: Duration) -> Result<Vec<String>, String> {
        self.begin_stop();
        // 显式降级（标注）：存储队列已被外部先行关闭时（如存储故障演练），
        // 持久作业表不可读——在役 run 的取消由 shutdown Phase 2.5a 在
        // RunSupervisor 层统一承载，存储状态由 managed close / Phase 3 如实
        // 上报；此处响亮记日志后继续内存侧收尾，不让关停双重报错。
        let jobs = match store::list_open(&self.db).await {
            Ok(jobs) => jobs,
            Err(StorageError::QueueClosed) => {
                tracing::error!(
                    "media stop: durable job table unreadable (queue already closed); \
                     continuing with in-memory teardown only"
                );
                Vec::new()
            }
            Err(error) => return Err(error.to_string()),
        };
        for job in jobs {
            if let Some(flag) = self
                .active
                .lock()
                .expect("媒体活动锁")
                .get(&job.operation_id)
            {
                flag.store(true, Ordering::Release);
            }
            if let (Some(runs), Some(execution)) = (self.runs.upgrade(), job.execution_run_id) {
                let _ = runs.cancel_run(&execution, "media service shutdown");
            }
        }
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            self.reap_finished().await?;
            let unfinished = self
                .drivers
                .lock()
                .expect("媒体驱动锁")
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            if unfinished.is_empty() {
                if let Some(runtime) = self.jimeng.get() {
                    runtime.stop().await?;
                }
                return Ok(unfinished);
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(unfinished);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    async fn references(
        &self,
        job: &MediaJob,
        spec: &MediaSpec,
        execution: &RunContext,
    ) -> Result<Vec<ImageReference>, String> {
        let runs = self.runs.upgrade().ok_or("媒体执行监督器已退出")?;
        let actual_cancel = runs
            .run_scope(execution.run_id.as_str())
            .ok_or("媒体执行任务没有真实取消作用域")?;
        let mut total = 0usize;
        let mut out = Vec::new();
        for reference in spec.image.iter().chain(&spec.reference_images) {
            let file = self
                .files
                .get_file(&job.session_id, &reference.file_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("参考图不存在或不属于当前会话")?;
            if !matches!(
                file.mime.as_str(),
                "image/png" | "image/jpeg" | "image/webp"
            ) {
                return Err("参考文件不是支持的图片".into());
            }
            if file.status != "available" || file.size_bytes > 20 * 1024 * 1024 {
                return Err("参考图已不可用或超出大小限制".into());
            }
            let bytes = self.io.read_media_reference_snapshot(
                &json!({"kind":"session-file","fileId":file.file_id,"sessionId":job.session_id}),
                execution, runs.as_ref(), &actual_cancel,
            ).await.map_err(|_| "参考图无法通过当前媒体执行权限读取")?.bytes;
            total = total.checked_add(bytes.len()).ok_or("参考图大小溢出")?;
            if total > 40 * 1024 * 1024 || bytes.len() as u64 != file.size_bytes {
                return Err("参考图大小超出限制或已改变".into());
            }
            out.push(ImageReference::Bytes {
                bytes,
                mime: file.mime,
                filename: file.filename,
            });
        }
        Ok(out)
    }
    async fn drive_jimeng(
        self: &Arc<Self>,
        mut job: MediaJob,
        execution: RunContext,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let runtime = self.jimeng.get().ok_or("即梦执行器尚未配置")?.clone();
        let spec: MediaSpec =
            serde_json::from_value(job.spec.clone()).map_err(|_| "即梦恢复参数损坏")?;
        let progress = MediaProgress {
            service: Arc::downgrade(self),
            operation_id: job.operation_id.clone(),
            cancel: cancel.clone(),
        };
        let call = ToolCallId::new(job.tool_call_id.clone());
        let deadline = job
            .created_at
            .checked_add(MEDIA_JOB_TIMEOUT_MS)
            .ok_or("即梦期限溢出")?;
        let monitor_job = job.clone();
        let monitor_cancel = cancel.clone();
        let drive = async {
            if job.phase == MediaPhase::Submitting {
                let references = self.references(&job, &spec, &execution).await?;
                let remote = runtime
                    .submit(
                        &execution,
                        &call,
                        &job.state["route"],
                        &spec,
                        &references,
                        &job.operation_id,
                        &cancel,
                        &progress,
                    )
                    .await?;
                job = store::get(&self.db, &job.operation_id)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or("即梦作业不存在")?;
                let mut state = job.state.clone();
                state["tracker"] = json!(remote);
                state["remoteOutcome"] = json!("accepted");
                state["opaqueRemoteDownload"] = json!(true);
                job = self.update(&job, MediaPhase::Submitted, state).await?;
            }
            loop {
                self.authority(&job, &cancel).await?;
                if self.clock.now_unix_ms() >= deadline {
                    return Err("即梦任务超时；远端结果未确认".into());
                }
                let remote = job.state["tracker"]
                    .as_str()
                    .ok_or("即梦真实submit_id回执缺失")?;
                let result = runtime
                    .query(
                        &execution,
                        &call,
                        &job.state["route"],
                        remote,
                        &job.operation_id,
                        &cancel,
                    )
                    .await?;
                match result["status"].as_str() {
                    Some("completed") => {
                        // CLI 自身将查询与下载合并；这里只表示宿主开始核验已下载文件。
                        progress.downloading().await?;
                        let actual = result["files"].as_array().ok_or("即梦文件回执损坏")?;
                        let mut products = Vec::new();
                        for file in actual {
                            self.authority(&job, &cancel).await?;
                            let path = std::path::PathBuf::from(
                                file["path"].as_str().ok_or("即梦文件路径损坏")?,
                            );
                            let mime = file["mime"].as_str().ok_or("即梦文件格式损坏")?.to_owned();
                            let cancel_read = cancel.clone();
                            let handle = self
                                .runs
                                .upgrade()
                                .ok_or("媒体运行服务已关闭")?
                                .task_supervisor()
                                .spawn_blocking_owned(
                                    Some(execution.run_id.to_string()),
                                    "media:verify-jimeng-product".into(),
                                    move || capture_cli_product(path, mime, &cancel_read),
                                )
                                .map_err(|e| e.to_string())?;
                            products.push(
                                handle
                                    .wait()
                                    .await
                                    .map_err(|_| "即梦文件核验清理未确认")??,
                            );
                        }
                        return self.deliver(&job, &spec, products, &cancel).await;
                    }
                    Some("failed") => {
                        let mut state = job.state.clone();
                        state["message"] = json!("即梦报告生成失败");
                        state["remoteOutcome"] = json!("failed");
                        self.update(&job, MediaPhase::Failed, state).await?;
                        return Err("供应商已确认媒体生成失败".into());
                    }
                    Some("generating") => {
                        if job.phase == MediaPhase::Submitted {
                            job = self
                                .update(&job, MediaPhase::Generating, job.state.clone())
                                .await?;
                        }
                    }
                    _ => return Err("即梦查询返回未知状态，远端结果未确认".into()),
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        };
        let monitor = async {
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if let Err(error) = self.authority(&monitor_job, &monitor_cancel).await {
                    monitor_cancel.store(true, Ordering::Release);
                    if let (Some(runs), Some(id)) =
                        (self.runs.upgrade(), monitor_job.execution_run_id.as_deref())
                    {
                        let _ = runs.cancel_run(id, "media source authority revoked");
                    }
                    return Err(error);
                }
                if self.clock.now_unix_ms() >= deadline {
                    monitor_cancel.store(true, Ordering::Release);
                    if let (Some(runs), Some(id)) =
                        (self.runs.upgrade(), monitor_job.execution_run_id.as_deref())
                    {
                        let _ = runs.cancel_run(id, "media operation timed out");
                    }
                    return Err::<(), String>("即梦任务超时；远端结果未确认".into());
                }
            }
        };
        let result = {
            tokio::pin!(drive);
            tokio::select! {biased;result=&mut drive=>result,error=monitor=>error}
        };
        // 丢弃等待并不等于进程停止；必须收到真实进程清理回执才退出固定 Run。
        if result.is_err() {
            runtime
                .settle_cancelled_run(execution.run_id.as_str())
                .await?;
        }
        result
    }
    async fn drive(
        self: &Arc<Self>,
        mut job: MediaJob,
        execution: RunContext,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), String> {
        self.authority(&job, &cancel).await?;
        if execution.run_id.as_str()
            != job
                .execution_run_id
                .as_deref()
                .ok_or("媒体执行任务不存在")?
            || execution.session_id.as_str() != job.session_id
        {
            return Err("媒体驱动与真实执行任务不一致".into());
        }
        if job.provider == "jimeng-cli" {
            return self.drive_jimeng(job, execution, cancel).await;
        }
        let operations = self.operations.as_ref().ok_or("媒体服务未配置")?;
        let spec: MediaSpec =
            serde_json::from_value(job.spec.clone()).map_err(|_| "媒体恢复参数损坏")?;
        let route = crate::operations::MediaRouteSnapshot::from_value(&job.state["route"])
            .map_err(|e| e.to_string())?;
        let progress = MediaProgress {
            service: Arc::downgrade(self),
            operation_id: job.operation_id.clone(),
            cancel: cancel.clone(),
        };
        let context = OperationCallContext {
            session_id: job.session_id.clone(),
            run_id: job.execution_run_id.clone().ok_or("媒体执行任务不存在")?,
            attempt: Some(execution.attempt.to_string()),
            cause_ref: Some(job.tool_call_id.clone()),
        };
        let deadline = job
            .created_at
            .checked_add(MEDIA_JOB_TIMEOUT_MS)
            .ok_or("媒体期限溢出")?;
        let monitor_job = job.clone();
        let monitor_cancel = cancel.clone();
        let run = async {
            if job.phase == MediaPhase::Submitting {
                let references = self.references(&job, &spec, &execution).await?;
                let products = match job.kind.as_str() {
                    "image" => match operations
                        .generate_image_for_context(
                            image_request(&spec, references)?,
                            Some(deadline),
                            &context,
                        )
                        .await
                        .map_err(|e| e.to_string())?
                    {
                        ImageGenerationOutcome::Done { products } => Some(products),
                        ImageGenerationOutcome::Pending { task_id } => {
                            job = self.accepted(job, &task_id).await?;
                            None
                        }
                    },
                    "video" => {
                        let accepted = operations
                            .submit_video_for_context(
                                video_request(&spec, references)?,
                                Some(deadline),
                                &context,
                            )
                            .await
                            .map_err(|e| e.to_string())?;
                        job = self.accepted(job, &accepted.task_id).await?;
                        None
                    }
                    "speech" => Some(vec![operations
                        .synthesize_speech(
                            SpeechRequest {
                                text: spec.prompt.clone(),
                                voice: spec.voice.clone(),
                                speed: spec.speed,
                                format: spec.format.clone(),
                            },
                            Some(deadline),
                        )
                        .await
                        .map_err(|e| e.to_string())?]),
                    _ => return Err("媒体类型损坏".into()),
                };
                if let Some(products) = products {
                    return self.deliver(&job, &spec, products, &cancel).await;
                }
            } else {
                let previous = job.state["task"]["owner"]["runId"]
                    .as_str()
                    .ok_or("媒体恢复缺少旧执行因果")?;
                let actual=self.db.query_one_text("SELECT r.run_id FROM runs r JOIN run_lineage l ON l.run_id=r.run_id JOIN runs p ON p.run_id=l.parent_run_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=p.owner_kind AND r.owner_subject=p.owner_subject AND l.origin='media' AND l.parent_run_id=?3 AND l.cause_id=?4 AND l.source_message_id=?5",vec![previous.into(),job.session_id.clone(),job.source_run_id.clone(),job.operation_id.clone(),job.tool_call_id.clone()]).await.map_err(|e|e.to_string())?;
                if actual.is_none() {
                    return Err("媒体恢复旧执行因果不一致".into());
                }
                operations
                    .restore_media_snapshot(&job.state["task"], &context, previous)
                    .map_err(|e| e.to_string())?;
            }
            loop {
                self.authority(&job, &cancel).await?;
                if self.clock.now_unix_ms() >= deadline {
                    return Err("媒体任务超时；供应商端结果未确认".into());
                }
                let tracker = job.state["tracker"]
                    .as_str()
                    .ok_or("媒体远端任务回执缺失")?;
                let status = if job.kind == "image" {
                    operations.query_media_task(tracker, Some(deadline)).await
                } else {
                    operations.query_video(tracker, Some(deadline)).await
                }
                .map_err(|e| e.to_string())?;
                match status {
                    MediaTaskStatus::Completed { products } => {
                        return self.deliver(&job, &spec, products, &cancel).await
                    }
                    MediaTaskStatus::Failed { reason } => {
                        job = store::get(&self.db, &job.operation_id)
                            .await
                            .map_err(|e| e.to_string())?
                            .ok_or("媒体任务不存在")?;
                        let mut state = job.state.clone();
                        state["message"] = json!(reason);
                        state["remoteOutcome"] = json!("failed");
                        self.update(&job, MediaPhase::Failed, state).await?;
                        return Err("供应商已确认媒体生成失败".into());
                    }
                    MediaTaskStatus::CancelledLocally { .. } => {
                        return Err("媒体任务已停止；远端结果未确认".into())
                    }
                    MediaTaskStatus::Generating => {
                        if job.phase == MediaPhase::Submitted {
                            job = self
                                .update(&job, MediaPhase::Generating, job.state.clone())
                                .await?;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        };
        // 外层只在协作清理结束后保存未知；不会把超时当成已停止远端任务。
        let monitor = async {
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                if let Err(error) = self.authority(&monitor_job, &monitor_cancel).await {
                    monitor_cancel.store(true, Ordering::Release);
                    if let (Some(runs), Some(id)) =
                        (self.runs.upgrade(), monitor_job.execution_run_id.as_deref())
                    {
                        let _ = runs.cancel_run(id, "media source authority revoked");
                    }
                    return Err(error);
                }
                if self.clock.now_unix_ms() >= deadline {
                    monitor_cancel.store(true, Ordering::Release);
                    if let (Some(runs), Some(id)) =
                        (self.runs.upgrade(), monitor_job.execution_run_id.as_deref())
                    {
                        let _ = runs.cancel_run(id, "media operation timed out");
                    }
                    return Err::<(), String>("媒体任务超时；供应商端结果未确认".into());
                }
            }
        };
        let joined = async {
            tokio::pin!(run);
            tokio::select! {biased;result=&mut run=>result,error=monitor=>error}
        };
        let result = operations
            .with_media_job(
                route,
                progress,
                context.clone(),
                execution.principal.clone(),
                joined,
            )
            .await;
        if result.is_err() {
            operations
                .settle_media_run(execution.run_id.as_str())
                .await?;
            if let Some(current) = store::get(&self.db, &monitor_job.operation_id)
                .await
                .map_err(|e| e.to_string())?
            {
                if let Some(tracker) = current.state["tracker"].as_str() {
                    let _ = operations.cancel_media_task(tracker);
                }
            }
        }
        result
    }
    async fn accepted(&self, job: MediaJob, tracker: &str) -> Result<MediaJob, String> {
        let job = store::get(&self.db, &job.operation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体任务不存在")?;
        let operations = self.operations.as_ref().ok_or("媒体服务未配置")?;
        let mut state = job.state.clone();
        state["tracker"] = json!(tracker);
        state["task"] = operations
            .media_snapshot(tracker)
            .map_err(|e| e.to_string())?;
        state["remoteOutcome"] = json!("accepted");
        self.update(&job, MediaPhase::Submitted, state).await
    }
    async fn deliver(
        &self,
        job: &MediaJob,
        spec: &MediaSpec,
        products: Vec<RegisteredProduct>,
        cancel: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        self.authority(job, cancel).await?;
        let mut current = store::get(&self.db, &job.operation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体任务不存在")?;
        if current.phase != MediaPhase::Downloading {
            let mut state = current.state.clone();
            state["remoteOutcome"] = json!("completed");
            current = self
                .update(&current, MediaPhase::Downloading, state)
                .await?;
        }
        let mut total = 0usize;
        for (index, product) in products.iter().enumerate() {
            self.authority(&current, cancel).await?;
            let prefix = match current.kind.as_str() {
                "image" => "image/",
                "video" => "video/",
                "speech" => "audio/",
                _ => return Err("媒体交付类型损坏".into()),
            };
            if !product.mime.starts_with(prefix) {
                return Err("供应商交付的文件类型与已批准媒体类型不一致".into());
            }
            let owned_product = product.clone();
            let read_cancel = cancel.clone();
            let handle = self
                .runs
                .upgrade()
                .ok_or("媒体运行服务已关闭")?
                .task_supervisor()
                .spawn_blocking_owned(
                    current.execution_run_id.clone(),
                    "media:read-owned-product".into(),
                    move || read_product(&owned_product, &read_cancel),
                )
                .map_err(|e| e.to_string())?;
            let bytes = handle.wait().await.map_err(|_| "媒体读取清理尚未确认")??;
            total = total.checked_add(bytes.len()).ok_or("媒体大小溢出")?;
            if bytes.is_empty()
                || total > 100 * 1024 * 1024
                || lingxi_protocol::canon::sha256_hex(&bytes) != product.sha256
            {
                return Err("生成的媒体内容为空、损坏或超出大小限制".into());
            }
            let name = spec.suggested_filename.clone().unwrap_or_else(|| {
                format!(
                    "{}-{}-{index}.{}",
                    current.kind,
                    &current.operation_id[6..22],
                    extension(&product.mime)
                )
            });
            let authority_lease = self.authority_gate.clone().read_owned().await;
            self.authority(&current, cancel).await?;
            current = store::get(&self.db, &current.operation_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("媒体作业不存在")?;
            let execution = current
                .execution_run_id
                .as_ref()
                .ok_or("媒体发布缺少实际执行任务")?;
            let scope = self
                .runs
                .upgrade()
                .ok_or("媒体运行服务已关闭")?
                .run_scope(execution)
                .ok_or("媒体执行任务取消根已缺失")?;
            let guard = store::MediaPublicationGuard {
                job: current.clone(),
                cancel: cancel.clone(),
                execution_cancelled: Arc::new(move || scope.is_cancelled()),
                authority_lease,
                artifact_index: u32::try_from(index).map_err(|_| "媒体文件序号溢出")?,
            };
            self.files
                .register_media_blob(guard, Some(&name), &product.mime, &bytes)
                .await
                .map_err(|e| e.to_string())?;
            // 取消可以在文件事务之后发生；重读回执保留该事务已经提交的实际部分文件。
            current = store::get(&self.db, &current.operation_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("媒体文件已写入但作业回执不存在")?;
            let _ = self.events.publish(
                "media_generation_update",
                json!({"sessionId":current.session_id,"mediaGeneration":public_job(&current)?}),
            );
        }
        if current.state["files"].as_array().is_none_or(Vec::is_empty) {
            return Err("供应商未交付媒体文件".into());
        }
        self.authority(&current, cancel).await?;
        let mut state = current.state.clone();
        state["remoteOutcome"] = json!("completed");
        state["message"] = json!("媒体已生成并交付到当前会话");
        self.update(&current, MediaPhase::Completed, state).await?;
        Ok(())
    }
}
#[derive(Clone)]
pub struct MediaProgress {
    service: Weak<MediaService>,
    operation_id: String,
    cancel: Arc<AtomicBool>,
}
impl MediaProgress {
    pub async fn dispatched(&self) -> Result<(), String> {
        let service = self.service.upgrade().ok_or("媒体服务已关闭")?;
        let job = store::get(&service.db, &self.operation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体任务不存在")?;
        service.authority(&job, &self.cancel).await?;
        if job.phase == MediaPhase::Submitting && job.state["dispatchStarted"] != true {
            let mut state = job.state.clone();
            state["dispatchStarted"] = json!(true);
            service.update(&job, MediaPhase::Submitting, state).await?;
        }
        Ok(())
    }
    pub async fn downloading(&self) -> Result<(), String> {
        let service = self.service.upgrade().ok_or("媒体服务已关闭")?;
        let job = store::get(&service.db, &self.operation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体任务不存在")?;
        service.authority(&job, &self.cancel).await?;
        if job.phase != MediaPhase::Downloading {
            let mut state = job.state.clone();
            state["remoteOutcome"] = json!("completed");
            service.update(&job, MediaPhase::Downloading, state).await?;
        }
        Ok(())
    }
}
pub(crate) fn public_job(job: &MediaJob) -> Result<Value, String> {
    use lingxi_protocol::{
        MediaArtifact, MediaJobPhase, MediaJobReceipt, MediaKind, MediaRemoteCancel,
        MediaRemoteOutcome,
    };
    let bad = || "媒体公开回执损坏".to_owned();
    let kind = match job.kind.as_str() {
        "image" => MediaKind::Image,
        "video" => MediaKind::Video,
        "speech" => MediaKind::Speech,
        _ => return Err(bad()),
    };
    let status = match job.phase {
        MediaPhase::Submitting => MediaJobPhase::Submitting,
        MediaPhase::Submitted => MediaJobPhase::Submitted,
        MediaPhase::Generating => MediaJobPhase::Generating,
        MediaPhase::Downloading => MediaJobPhase::Downloading,
        MediaPhase::Completed => MediaJobPhase::Completed,
        MediaPhase::Failed => MediaJobPhase::Failed,
        MediaPhase::Cancelled => MediaJobPhase::Cancelled,
        MediaPhase::Unknown => MediaJobPhase::Unknown,
    };
    let mut files = Vec::new();
    if let Some(actual) = job.state.get("files") {
        for file in actual.as_array().ok_or_else(bad)? {
            let text = |key: &str| file[key].as_str().map(String::from).ok_or_else(bad);
            files.push(MediaArtifact {
                file_id: text("fileId")?,
                filename: text("filename")?,
                mime: text("mime")?,
                size_bytes: file["sizeBytes"].as_u64().ok_or_else(bad)?,
            });
        }
    }
    let remote_outcome = match job.state["remoteOutcome"].as_str() {
        Some("not_yet_confirmed") => MediaRemoteOutcome::NotYetConfirmed,
        Some("not_submitted") => MediaRemoteOutcome::NotSubmitted,
        Some("accepted") => MediaRemoteOutcome::Accepted,
        Some("completed") => MediaRemoteOutcome::Completed,
        Some("failed") => MediaRemoteOutcome::Failed,
        Some("unknown") => MediaRemoteOutcome::Unknown,
        None if job.state["remoteOutcome"].is_null() => MediaRemoteOutcome::Unknown,
        _ => return Err(bad()),
    };
    let remote_cancel = match job.state["remoteCancel"].as_str() {
        Some("unsupported") => Some(MediaRemoteCancel::Unsupported),
        Some("confirmed") => Some(MediaRemoteCancel::Confirmed),
        Some("unknown") => Some(MediaRemoteCancel::Unknown),
        None if job.state["remoteCancel"].is_null() => None,
        _ => return Err(bad()),
    };
    let optional = |key: &str| {
        if job.state[key].is_null() {
            Ok(None)
        } else {
            job.state[key]
                .as_str()
                .map(|v| Some(v.to_owned()))
                .ok_or_else(bad)
        }
    };
    let receipt = MediaJobReceipt {
        task_id: job.operation_id.clone(),
        kind,
        provider: job.provider.clone(),
        status,
        source_run_id: job.source_run_id.clone(),
        execution_run_id: job.execution_run_id.clone(),
        revision: job.revision,
        files,
        remote_outcome,
        remote_cancel,
        local_delivery_stopped: match &job.state["localDeliveryStopped"] {
            Value::Null => false,
            Value::Bool(value) => *value,
            _ => return Err(bad()),
        },
        message: optional("message")?,
        error_code: optional("errorCode")?,
        created_at_ms: job.created_at,
        updated_at_ms: job.updated_at,
    };
    serde_json::to_value(receipt).map_err(|_| bad())
}
fn extension(mime: &str) -> &str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "audio/flac" => "flac",
        "audio/ogg" => "opus",
        "audio/aac" => "aac",
        "audio/mp4" => "m4a",
        _ => "bin",
    }
}
fn read_product(product: &RegisteredProduct, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    read_product_inner(product, cancel, Some(&product.sha256))
}
fn capture_cli_product(
    path: std::path::PathBuf,
    mime: String,
    cancel: &AtomicBool,
) -> Result<RegisteredProduct, String> {
    let size = std::fs::symlink_metadata(&path)
        .map_err(|_| "即梦产物不存在")?
        .len();
    let mut product = RegisteredProduct {
        resource_id: String::new(),
        path,
        mime,
        size_bytes: size,
        sha256: String::new(),
    };
    let bytes = read_product_inner(&product, cancel, None)?;
    let valid = match product.mime.as_str() {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "video/mp4" | "video/quicktime" => bytes.get(4..8) == Some(b"ftyp"),
        "video/webm" => bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
        _ => false,
    };
    if !valid {
        return Err("即梦产物内容与文件格式不一致".into());
    }
    product.sha256 = lingxi_protocol::canon::sha256_hex(&bytes);
    product.resource_id = format!("jimeng-product-{}", product.sha256);
    Ok(product)
}
fn read_product_inner(
    product: &RegisteredProduct,
    cancel: &AtomicBool,
    digest: Option<&str>,
) -> Result<Vec<u8>, String> {
    use crate::filetools::fsio::{DirFd, FileIdentity};
    use std::io::Read;
    if cancel.load(Ordering::Acquire)
        || !product.path.is_absolute()
        || product.path.canonicalize().ok().as_ref() != Some(&product.path)
    {
        return Err("媒体产物已取消或路径被替换".into());
    }
    let directory = DirFd::open(product.path.parent().ok_or("媒体产物目录不存在")?)
        .map_err(|_| "媒体产物目录不可读取")?;
    let name = product.path.file_name().ok_or("媒体产物名称不存在")?;
    let mut file = directory
        .open_attachment_nofollow(name)
        .ok_or("媒体产物不存在")?
        .map_err(|_| "媒体产物不能通过安全句柄读取")?;
    let identity = FileIdentity::of(&file).map_err(|_| "媒体产物信息无法读取")?;
    if !file
        .metadata()
        .map_err(|_| "媒体产物信息无法读取")?
        .is_file()
        || identity.size != product.size_bytes
        || identity.size > 50 * 1024 * 1024
    {
        return Err("媒体产物大小无效".into());
    }
    let mut bytes =
        Vec::with_capacity(usize::try_from(identity.size).map_err(|_| "媒体产物大小溢出")?);
    let mut chunk = [0u8; 32768];
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err("媒体产物读取已取消".into());
        }
        let n = file.read(&mut chunk).map_err(|_| "媒体产物读取失败")?;
        if n == 0 {
            break;
        }
        if bytes.len().saturating_add(n) > 50 * 1024 * 1024 {
            return Err("媒体产物读取超出限制".into());
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    let reopened = directory
        .open_attachment_nofollow(name)
        .ok_or("媒体产物读取期间消失")?
        .map_err(|_| "媒体产物读取期间被替换")?;
    if FileIdentity::of(&file).map_err(|_| "媒体产物信息改变")? != identity
        || FileIdentity::of(&reopened).map_err(|_| "媒体产物信息改变")? != identity
        || digest.is_some_and(|digest| lingxi_protocol::canon::sha256_hex(&bytes) != digest)
    {
        return Err("媒体产物读取期间改变或损坏".into());
    }
    Ok(bytes)
}
fn image_request(
    spec: &MediaSpec,
    references: Vec<ImageReference>,
) -> Result<ImageRequest, String> {
    let o = &spec.options;
    let string = |k: &str| o.get(k).and_then(Value::as_str).map(String::from);
    Ok(ImageRequest {
        prompt: spec.prompt.clone(),
        size: string("size"),
        resolution: spec.resolution.clone(),
        ratio: spec.ratio.clone(),
        n: spec.count,
        format: spec.format.clone().or_else(|| string("format")),
        quality: spec.quality.clone(),
        style: string("style"),
        background: string("background"),
        output_compression: o
            .get("output_compression")
            .and_then(Value::as_u64)
            .map(|v| u32::try_from(v).map_err(|_| "图片压缩参数溢出"))
            .transpose()?,
        moderation: string("moderation"),
        watermark: o.get("watermark").and_then(Value::as_bool),
        guidance_scale: o.get("guidance_scale").and_then(Value::as_f64),
        seed: o.get("seed").and_then(Value::as_i64),
        negative_prompt: string("negative_prompt"),
        prompt_extend: o.get("prompt_extend").and_then(Value::as_bool),
        prompt_optimizer: o.get("prompt_optimizer").and_then(Value::as_bool),
        width: o
            .get("width")
            .and_then(Value::as_u64)
            .map(|v| u32::try_from(v).map_err(|_| "图片宽度溢出"))
            .transpose()?,
        height: o
            .get("height")
            .and_then(Value::as_u64)
            .map(|v| u32::try_from(v).map_err(|_| "图片高度溢出"))
            .transpose()?,
        responses_model: string("responses_model"),
        references,
    })
}
fn video_request(
    spec: &MediaSpec,
    references: Vec<ImageReference>,
) -> Result<VideoRequest, String> {
    Ok(VideoRequest {
        prompt: spec.prompt.clone(),
        resolution: spec.resolution.clone(),
        size: spec
            .options
            .get("size")
            .and_then(Value::as_str)
            .map(String::from),
        width: spec.options.get("width").and_then(Value::as_f64),
        height: spec.options.get("height").and_then(Value::as_f64),
        ratio: spec.ratio.clone(),
        frame_rate: spec.options.get("frame_rate").and_then(Value::as_f64),
        num_frames: spec.options.get("num_frames").and_then(Value::as_f64),
        duration: spec.duration,
        references,
    })
}
