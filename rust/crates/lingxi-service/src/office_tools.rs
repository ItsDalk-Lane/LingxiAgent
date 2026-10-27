//! 内置办公与美化工具使用真实任务身份、当前助手开关和资源权限；不装载旧 Agent loop。
use crate::{
    memory::MemoryService,
    resourceio::{OpContext, ResourceIoService},
    sessionfiles::SessionFileService,
    toolgateway::{ReviewInput, ToolInvocationGateway},
    ServiceState,
};
use lingxi_adapters::storage::{session_tree, RunDatabase};
use lingxi_kernel::{
    browser::{BrowserPort, PdfScope},
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
};

#[derive(Clone, Debug)]
pub struct OfficeAssets {
    pub themes_dir: PathBuf,
    pub cover_gallery_dir: PathBuf,
}
#[derive(Clone, Debug)]
pub struct OfficeWorkerLaunch {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
}
#[derive(Clone)]
pub struct BeautifyGenerationRequest {
    pub principal: crate::Principal,
    pub agent_id: String,
    pub target: Value,
    pub theme_tone: String,
    pub user_guidance: String,
    pub request_id: Option<String>,
}
pub type BeautifyFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, OfficeError>> + Send + 'a>>;
pub trait BeautifyGenerationPort: Send + Sync {
    fn status<'a>(&'a self, agent: &'a str) -> BeautifyFuture<'a, Value>;
    fn generate<'a>(&'a self, request: BeautifyGenerationRequest) -> BeautifyFuture<'a, Value>;
}
#[derive(Default)]
struct GuideTracking {
    sessions: BTreeMap<String, BTreeSet<String>>,
    order: VecDeque<String>,
}
#[derive(Clone)]
pub struct OfficeApi {
    pub(crate) storage: Arc<RunDatabase>,
    pub(crate) memory: Arc<MemoryService>,
    pub(crate) files: Arc<SessionFileService>,
    pub(crate) io: Arc<ResourceIoService>,
    pub(crate) assets: Option<OfficeAssets>,
    browser: Arc<dyn BrowserPort>,
    node: String,
    guide: Arc<Mutex<GuideTracking>>,
    document_worker: Arc<Mutex<Option<OfficeWorkerLaunch>>>,
    worker_runtime: Arc<crate::workerrpc::WorkerRuntime>,
    worker_access: Arc<crate::resourceaccess::ResourceAccess>,
    generation: Arc<Mutex<Option<Arc<dyn BeautifyGenerationPort>>>>,
}
#[derive(Debug)]
pub struct OfficeError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
    pub effect_unknown: bool,
}
impl OfficeError {
    pub(crate) fn bad(message: impl ToString) -> Self {
        Self {
            status: 400,
            code: "OFFICE_INPUT",
            message: message.to_string(),
            effect_unknown: false,
        }
    }
    pub(crate) fn resource(error: crate::resourceio::ResourceIoError) -> Self {
        Self {
            status: error.status,
            code: if error.code == "artifact_write_unknown" {
                "OFFICE_OUTCOME_UNKNOWN"
            } else if error.status == 403 {
                "OFFICE_FORBIDDEN"
            } else if error.status == 409 {
                "OFFICE_CONFLICT"
            } else {
                "OFFICE_INPUT"
            },
            effect_unknown: error.code == "artifact_write_unknown",
            message: error.to_string(),
        }
    }
    fn cancelled() -> Self {
        Self {
            status: 409,
            code: "OFFICE_CANCELLED",
            message: "办公操作已取消".into(),
            effect_unknown: false,
        }
    }
    fn unknown(message: impl ToString) -> Self {
        Self {
            status: 409,
            code: "OFFICE_OUTCOME_UNKNOWN",
            message: message.to_string(),
            effect_unknown: true,
        }
    }
    fn partial(message: impl ToString) -> Self {
        Self {
            status: 409,
            code: "OFFICE_PARTIAL_SUCCESS",
            message: message.to_string(),
            effect_unknown: false,
        }
    }
}
impl std::fmt::Display for OfficeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl From<String> for OfficeError {
    fn from(message: String) -> Self {
        Self::bad(message)
    }
}
impl From<&str> for OfficeError {
    fn from(message: &str) -> Self {
        Self::bad(message)
    }
}
fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} is required"))
}
fn op(ctx: &RunContext) -> OpContext {
    OpContext {
        principal_kind: ctx.principal.storage_kind(),
        principal_subject: ctx.principal.storage_subject(),
        session_id: ctx.session_id.to_string(),
        source: "agent_tool".into(),
        reason: Some("builtin office/beautify".into()),
        session_path: Some(format!("native:{}", ctx.session_id)),
        emit: true,
        capture_origin: None,
        engine_plane: false,
    }
}
impl OfficeApi {
    pub fn from_state(state: &ServiceState) -> Arc<Self> {
        Arc::new(Self {
            storage: state.storage().clone(),
            memory: state.memory().clone(),
            files: state.session_files().clone(),
            io: state.resource_io().clone(),
            assets: state.office_assets().cloned(),
            browser: state.browsers().port().clone(),
            node: state.management.identity_ids().0,
            guide: Arc::new(Mutex::new(GuideTracking::default())),
            document_worker: Arc::new(Mutex::new(None)),
            worker_runtime: crate::workerrpc::WorkerRuntime::new(
                crate::workerrpc::WorkerLimits {
                    max_request_bytes: 72 * 1024 * 1024,
                    max_line_bytes: 16 * 1024 * 1024,
                    max_callbacks: 0,
                    ..Default::default()
                },
                None,
                state.clock().clone(),
            ),
            worker_access: Arc::new(
                crate::resourceaccess::ResourceAccess::new(&[])
                    .expect("空资源许可不会产生路径错误"),
            ),
            generation: Arc::new(Mutex::new(None)),
        })
    }
    pub fn set_beautify_generation(
        &self,
        port: Arc<dyn BeautifyGenerationPort>,
    ) -> Result<(), String> {
        *self
            .generation
            .lock()
            .map_err(|_| "Beautify runtime unavailable")? = Some(port);
        Ok(())
    }
    pub(crate) fn generation(
        &self,
    ) -> Result<Option<Arc<dyn BeautifyGenerationPort>>, OfficeError> {
        Ok(self
            .generation
            .lock()
            .map_err(|_| OfficeError::bad("Beautify runtime unavailable"))?
            .clone())
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
            return Err(format!("{plugin} is disabled for the actual source agent"));
        }
        Ok(())
    }
    async fn caller(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        request: &ToolRequest,
        plugin: &str,
    ) -> Result<String, String> {
        let row=self.storage.query_one_text("SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id JOIN run_attempts a ON a.run_id=r.run_id JOIN invocation_journal j ON j.run_id=r.run_id AND j.journal_id=?7 WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND r.generation=?5 AND a.attempt=?6 AND a.generation=r.generation AND r.status='running' AND s.lifecycle='active' AND j.phase='started' AND j.session_id=r.session_id AND j.owner_kind=r.owner_kind AND j.owner_subject=r.owner_subject AND j.target=?8 AND j.args_digest=?9 AND j.generation=r.generation AND j.attempt=a.attempt AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1)",vec![ctx.run_id.to_string(),ctx.session_id.to_string(),ctx.principal.storage_kind().into(),ctx.principal.storage_subject(),ctx.generation.to_string(),ctx.attempt.to_string(),call.to_string(),request.target.clone(),request.args_digest.hex.clone()]).await.map_err(|e|e.to_string())?;
        if row.is_none() {
            return Err("current live Run and journal identity required".into());
        }
        let session = session_tree::get_session_row(&self.storage, ctx.session_id.as_str())
            .await
            .map_err(|e| e.to_string())?
            .ok_or("source session missing")?;
        if !ctx
            .principal
            .can_access_session(&session.owner_user_id, ctx.session_id.as_str())
        {
            return Err("source session ownership changed".into());
        }
        let bound = crate::toolgateway::review_scope()
            .ok_or("Office requires the gateway-bound source identity")?;
        if bound["sourceSessionId"] != ctx.session_id.as_str()
            || bound["actorId"] != session.agent_id
            || bound["principalKind"] != ctx.principal.storage_kind()
            || bound["principalSubject"] != ctx.principal.storage_subject()
        {
            return Err("Office source identity changed since gateway authorization".into());
        }
        self.enabled(&session.agent_id, plugin)?;
        self.files
            .ensure_stage_session(ctx)
            .await
            .map_err(|e| e.to_string())?;
        Ok(session.agent_id)
    }
    pub(crate) async fn read_resource(
        &self,
        reference: &Value,
        context: &OpContext,
    ) -> Result<(Option<PathBuf>, Vec<u8>, Value), String> {
        let snapshot: crate::resourceio::ArtifactSnapshot = self
            .io
            .read_artifact_snapshot(reference, context)
            .await
            .map_err(|e| e.to_string())?;
        Ok((Some(snapshot.file_path), snapshot.bytes, snapshot.meta))
    }
    pub fn set_document_worker(&self, launch: OfficeWorkerLaunch) -> Result<(), String> {
        if launch.argv.is_empty()
            || !Path::new(&launch.argv[0]).is_absolute()
            || !launch.cwd.is_absolute()
            || !launch.cwd.is_dir()
        {
            return Err("Office worker requires trusted absolute executable/cwd".into());
        }
        *self
            .document_worker
            .lock()
            .map_err(|_| "Office worker configuration unavailable")? = Some(launch);
        Ok(())
    }
    async fn document(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        source_request: &ToolRequest,
        args: &Value,
        context: &OpContext,
    ) -> Result<Value, OfficeError> {
        use base64::Engine as _;
        let launch = self
            .document_worker
            .lock()
            .map_err(|_| "Office worker configuration unavailable")?
            .clone()
            .ok_or("supervised Office document worker is unavailable")?;
        let reference = if let Some(resource) = args.get("resource") {
            resource.clone()
        } else {
            json!({"kind":"local-file","path":text(args,"filePath")?})
        };
        let (path, bytes, _) = self.read_resource(&reference, context).await?;
        let path = path.ok_or("Office document has no actual materialized identity")?;
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Office filename is invalid")?;
        let bytes = if matches!(
            path.extension()
                .and_then(|s| s.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("docx" | "xlsx" | "xlsm")
        ) {
            tokio::task::spawn_blocking(move || {
                validate_package(&bytes)?;
                Ok::<_, String>(bytes)
            })
            .await
            .map_err(|_| "Office archive validation was interrupted")??
        } else {
            bytes
        };
        self.caller(ctx, call, source_request, "office").await?;
        let mut options = args
            .as_object()
            .cloned()
            .ok_or("Office arguments must be an object")?;
        options.remove("resource");
        options.remove("filePath");
        let input = json!({"bytesBase64":base64::engine::general_purpose::STANDARD.encode(bytes),"filename":filename,"inputOptions":options});
        let budget = SchemaBudget {
            max_arguments_bytes: 72 * 1024 * 1024,
            ..SchemaBudget::default()
        };
        let request = ToolRequest::from_effective_arguments(
            "tool:plugin:office:read-document",
            input,
            &budget,
        )
        .ok_or("Office internal bytes request exceeds its dedicated budget")?;
        let spec = crate::workerrpc::WorkerToolSpec {
            plugin_id: "office".into(),
            op: "office.read-document".into(),
            local_name: "read-document".into(),
            description: "Read authorized document bytes".into(),
            input_schema: json!({"type":"object"}),
            path_args: vec![],
            argv: launch.argv,
            env: BTreeMap::new(),
            cwd: launch.cwd,
            model: Arc::new(crate::workerrpc::UnconfiguredWorkerModel),
            allowed_model_purposes: vec![],
            claimed_file_contract: None,
        };
        let executor = crate::workerrpc::WorkerToolExecutor::new(
            self.worker_runtime.clone(),
            spec,
            self.worker_access.clone(),
        );
        match executor.execute(ctx, call, &request).await.outcome {
            ToolOutcome::Success { result } => {
                let raw = result
                    .content
                    .iter()
                    .find_map(|block| match block {
                        lingxi_protocol::ContentBlock::Text { text } => Some(text),
                        _ => None,
                    })
                    .ok_or("Office worker returned no document")?;
                let mut value: Value = serde_json::from_str(raw)
                    .map_err(|_| "Office worker returned invalid document JSON")?;
                if !value.is_object()
                    || !["docx", "xlsx", "pdf", "text", "html"]
                        .contains(&value["kind"].as_str().unwrap_or(""))
                {
                    return Err("Office worker returned an unknown document type".into());
                }
                self.caller(ctx, call, source_request, "office").await?;
                value["filePath"] = json!(path);
                value["filename"] = json!(filename);
                Ok(value)
            }
            ToolOutcome::Cancelled => Err(OfficeError::cancelled()),
            ToolOutcome::Failed { error } => Err(OfficeError::bad(error.message)),
            ToolOutcome::Unknown { reason } => Err(OfficeError::unknown(reason)),
        }
    }
    pub(crate) async fn apply_cover(
        &self,
        args: &Value,
        context: &OpContext,
        operation: &str,
    ) -> Result<Value, OfficeError> {
        self.apply_cover_checked(args, context, operation, None, None)
            .await
    }
    /// frozen_target：真实后台任务必须保留付费前的原预检；恢复后也不能重新采纳当前版本。
    async fn apply_cover_checked(
        &self,
        args: &Value,
        context: &OpContext,
        operation: &str,
        caller: Option<(&RunContext, &ToolCallId, &ToolRequest)>,
        frozen_target: Option<&crate::resourceio::ArtifactWritePreflight>,
    ) -> Result<Value, OfficeError> {
        let target = if let Some(reference) = args.get("targetResource") {
            reference.clone()
        } else {
            json!({"kind":"local-file","path":args.get("targetFilePath").or_else(||args.get("filePath")).and_then(Value::as_str).ok_or("target Markdown path required")?})
        };
        let (path, markdown, meta) = self.read_resource(&target, context).await?;
        let path = path.ok_or("Markdown source must have a resolved path")?;
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_none_or(|s| !s.eq_ignore_ascii_case("md"))
        {
            return Err("cover target must be Markdown".into());
        }
        // Markdown仍保留普通文字写预算；较大图片仅走窄产物入口。
        if markdown.len() as u64 > crate::file_safety::MAX_RECOVERABLE_FILE_BYTES {
            return Err("cover Markdown exceeds the ordinary 8MiB text limit".into());
        }
        let target_preflight = match frozen_target {
            Some(target) => target.clone(),
            None => self
                .io
                .prepare_artifact_write(&target, true, context)
                .await
                .map_err(OfficeError::resource)?,
        };
        if target_preflight.file_path() != path {
            return Err("frozen cover target path does not match the actual document".into());
        }
        target_preflight
            .require_snapshot(&meta["artifactStamp"])
            .map_err(OfficeError::resource)?;
        let source = String::from_utf8(markdown).map_err(|_| "Markdown is not UTF-8")?;
        let (mut front, body) = frontmatter(&source)?;
        let (image_path, bytes) = if let Some(encoded) = args["image"]["contentBase64"].as_str() {
            use base64::Engine as _;
            let encoded = if encoded.starts_with("data:image/") {
                encoded.split_once(',').ok_or("invalid image data URL")?.1
            } else {
                encoded
            };
            if encoded.len() > 25 * 1024 * 1024 * 4 / 3 + 4 {
                return Err("cover upload exceeds 25MiB".into());
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| "invalid cover image bytes")?;
            (
                PathBuf::from(args["image"]["filename"].as_str().unwrap_or("cover.png")),
                bytes,
            )
        } else if let Some(resource) = args.get("imageResource") {
            // 后台封面链以会话文件引用交付生成图；读取仍走正式资源授权。
            let (actual, bytes, _) = self.read_resource(resource, context).await?;
            (actual.ok_or("cover image has no local identity")?, bytes)
        } else {
            let image = args
                .get("generatedFilePath")
                .or_else(|| args.get("imageFilePath"))
                .and_then(Value::as_str)
                .ok_or("an existing cover image is required")?;
            let (actual, bytes, _) = self
                .read_resource(&json!({"kind":"local-file","path":image}), context)
                .await?;
            (actual.ok_or("cover image has no local identity")?, bytes)
        };
        if bytes.is_empty() || bytes.len() > 25 * 1024 * 1024 {
            return Err("invalid cover image size".into());
        }
        let ext = image_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ![
            "png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg", "tif", "tiff", "heic",
            "heif",
        ]
        .contains(&ext.as_str())
        {
            return Err("unsupported cover image format".into());
        }
        let name = format!(
            "{}-cover-{}.{}",
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|stem| stem
                    .chars()
                    .map(|c| if c.is_alphanumeric() || matches!(c, '_' | '-') {
                        c
                    } else {
                        '-'
                    })
                    .collect::<String>())
                .filter(|stem| !stem.trim_matches('-').is_empty())
                .unwrap_or_else(|| "document".into()),
            &lingxi_protocol::canon::sha256_hex(operation.as_bytes())[..24],
            ext
        );
        let relative = format!(
            "{}/{name}",
            self.localized(None, "plugin.beautify.attachmentDirName")?
        );
        let mut attachment = target.clone();
        if attachment["kind"] == "mount" {
            let original = attachment["path"]
                .as_str()
                .ok_or("mount target path required")?;
            attachment["path"] = json!(Path::new(original)
                .parent()
                .unwrap_or(Path::new(""))
                .join(&relative)
                .to_string_lossy());
        } else {
            attachment = json!({"kind":"local-file","path":path.parent().ok_or("Markdown parent unavailable")?.join(&relative)});
        }
        let dimensions = image_dimensions(&bytes);
        let width = args["pixelWidth"]
            .as_u64()
            .or(dimensions.map(|d| d.0 as u64));
        let height = args["pixelHeight"]
            .as_u64()
            .or(dimensions.map(|d| d.1 as u64));
        let mut cover = json!({"image":relative,"displayWidth":100,"displayHeight":320,"positionX":50,"positionY":50});
        if let (Some(w), Some(h)) = (width.filter(|n| *n > 0), height.filter(|n| *n > 0)) {
            let g = gcd(w, h);
            cover["pixelWidth"] = json!(w);
            cover["pixelHeight"] = json!(h);
            cover["actualRatio"] = json!(format!("{}:{}", w / g, h / g));
        }
        // 同一真实调用的产物名称稳定；已有相同图片可读取确认，绝不覆盖不同内容。
        let existing = self
            .io
            .stat(&attachment, context)
            .await
            .map_err(|e| e.to_string())?;
        if let Some((ctx, call, request)) = caller {
            self.caller(ctx, call, request, "beautify").await?;
        }
        if existing["exists"] == true {
            let (_, actual, _) = self.read_resource(&attachment, context).await?;
            if actual != bytes {
                return Err("cover operation conflicts with an existing different image".into());
            }
        } else {
            let attachment_preflight = self
                .io
                .prepare_artifact_write(&attachment, false, context)
                .await
                .map_err(OfficeError::resource)?;
            let written = self
                .io
                .publish_artifact(&attachment_preflight, bytes, context)
                .await
                .map_err(OfficeError::resource)?;
            if written["ok"] == false {
                return Err(OfficeError::unknown(
                    "cover attachment write was not confirmed",
                ));
            }
        }
        front.insert(
            serde_yaml_ng::Value::String("cover".into()),
            serde_yaml_ng::to_value(&cover).map_err(|e| e.to_string())?,
        );
        let yaml = serde_yaml_ng::to_string(&front).map_err(|e| e.to_string())?;
        let updated = format!("---\n{}---\n{body}", yaml.trim_start_matches("---\n"));
        if let Some((ctx, call, request)) = caller {
            self.caller(ctx, call, request, "beautify")
                .await
                .map_err(|e| {
                    OfficeError::partial(format!(
                        "cover image was copied; Markdown update refused: {e}"
                    ))
                })?;
        }
        if updated.len() as u64 > crate::file_safety::MAX_RECOVERABLE_FILE_BYTES {
            return Err(OfficeError::partial(
                "cover image was copied; updated Markdown exceeds 8MiB",
            ));
        }
        let written = self
            .io
            .publish_artifact(&target_preflight, updated.into_bytes(), context)
            .await
            .map_err(|error| {
                if error.code == "artifact_write_unknown" {
                    OfficeError::unknown(format!(
                        "cover image was copied; Markdown write unconfirmed: {error}"
                    ))
                } else {
                    OfficeError::partial(format!(
                        "cover image was copied; Markdown was not updated: {error}"
                    ))
                }
            })?;
        if written["ok"] == false {
            return Err(OfficeError::partial(
                "cover image was copied but Markdown changed; update was not applied",
            ));
        }
        let materialized = self
            .io
            .materialize(&attachment, context)
            .await
            .map_err(|e| e.to_string())?;
        Ok(json!({"cover":cover,"markdownFilePath":path,"attachmentPath":materialized["filePath"]}))
    }
    async fn pdf(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        request: &ToolRequest,
        args: &Value,
        context: &OpContext,
    ) -> Result<Value, OfficeError> {
        let operation = format!(
            "office-pdf-{}",
            lingxi_protocol::canon::sha256_hex(json!([ctx.run_id, call]).to_string().as_bytes())
        );
        let output_preflight = match args["outputPath"].as_str() {
            Some(path) => Some(
                self.io
                    .prepare_artifact_write(
                        &json!({"kind":"local-file","path":path}),
                        args["overwrite"] == true,
                        context,
                    )
                    .await
                    .map_err(OfficeError::resource)?,
            ),
            None => None,
        };
        let job = crate::office_pdf::collect(self, args, context, &operation).await?;
        let cancel = CancelFlag(Arc::new(AtomicBool::new(false)));
        let scope = crate::toolgateway::TOOL_LIFETIME
            .try_with(|l| l.cancel.clone())
            .ok();
        let cancellation = async {
            match &scope {
                Some(scope) => scope.cancelled().await,
                None => std::future::pending().await,
            }
        };
        self.caller(ctx, call, request, "office").await?;
        let actual_session = session_tree::get_session_row(&self.storage, ctx.session_id.as_str())
            .await
            .map_err(|e| e.to_string())?
            .filter(|s| s.lifecycle == "active")
            .ok_or("PDF source session is unavailable")?;
        if !ctx
            .principal
            .can_access_session(&actual_session.owner_user_id, ctx.session_id.as_str())
        {
            return Err("PDF source session ownership changed".into());
        }
        let future = self.browser.render_pdf(
            PdfScope {
                session_id: ctx.session_id.to_string(),
                owner_subject: actual_session.owner_user_id,
                execution_node: self.node.clone(),
            },
            job,
            cancel.0.clone(),
        );
        tokio::pin!(future);
        let rendered = tokio::select! {biased;_ = cancellation=>{
            cancel.0.store(true,Ordering::Release);
            if let Err(error)=future.await { if error.effect_unknown { return Err(OfficeError::unknown(error)); } }
            return Err(OfficeError::cancelled());
        },result=&mut future=>result.map_err(|e| if e.effect_unknown { OfficeError::unknown(e) } else { OfficeError::bad(e) })?};
        if rendered.bytes.len() < 16
            || rendered.bytes.len() > 50 * 1024 * 1024
            || !rendered.bytes.starts_with(b"%PDF-")
            || !rendered.bytes[rendered.bytes.len().saturating_sub(2048)..]
                .windows(5)
                .any(|w| w == b"%%EOF")
            || lingxi_protocol::canon::sha256_hex(&rendered.bytes) != rendered.sha256
        {
            return Err("renderer returned an invalid or unconfirmed PDF".into());
        }
        let mut filename = args["filename"]
            .as_str()
            .unwrap_or("document.pdf")
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("document.pdf")
            .to_owned();
        if !filename.to_ascii_lowercase().ends_with(".pdf") {
            filename.push_str(".pdf");
        }
        self.caller(ctx, call, request, "office").await?;
        let file = if let Some(preflight) = output_preflight {
            let value = self
                .io
                .publish_artifact(&preflight, rendered.bytes, context)
                .await
                .map_err(OfficeError::resource)?;
            if value["ok"] != true {
                return Err(OfficeError::unknown(
                    "PDF publication has no confirmed result",
                ));
            }
            self.files
                .register_file(
                    ctx.session_id.as_str(),
                    preflight.file_path(),
                    Some(&operation),
                    Some(&filename),
                    "agent_artifact",
                )
                .await
                .map_err(|e| {
                    OfficeError::unknown(format!(
                        "PDF was written but SessionFile registration is unconfirmed: {e}"
                    ))
                })?
        } else {
            self.files
                .register_artifact_blob(
                    ctx.session_id.as_str(),
                    Some(&filename),
                    "application/pdf",
                    &rendered.bytes,
                )
                .await
                .map_err(|e| e.to_string())?
        };
        Ok(
            json!({"kind":"html-to-pdf","engine":rendered.renderer,"outputPath":file.file_path,"size":file.size_bytes,"sessionFile":file}),
        )
    }
}

fn frontmatter(text: &str) -> Result<(serde_yaml_ng::Mapping, String), String> {
    if let Some(rest) = text
        .strip_prefix("---\r\n")
        .or_else(|| text.strip_prefix("---\n"))
    {
        let delimiter = regex::Regex::new(r"(?m)^---\r?$").unwrap();
        let end = delimiter
            .find(rest)
            .ok_or("Markdown frontmatter is incomplete")?;
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(&rest[..end.start()]).map_err(|e| e.to_string())?;
        let map = if value.is_null() {
            serde_yaml_ng::Mapping::new()
        } else {
            value
                .as_mapping()
                .cloned()
                .ok_or("Markdown frontmatter must be an object")?
        };
        Ok((
            map,
            rest[end.end()..]
                .strip_prefix('\n')
                .unwrap_or(&rest[end.end()..])
                .to_owned(),
        ))
    } else {
        Ok((serde_yaml_ng::Mapping::new(), text.into()))
    }
}
fn validate_package(bytes: &[u8]) -> Result<(), String> {
    use std::io::{Cursor, Read};
    // 复用现役 Office 的 4096成员、单项20MiB/总80MiB预算；只读检查，不展开为宿主文件。
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > 4096 {
        return Err("Office archive has too many members".into());
    }
    let mut total = 0u64;
    let mut names = BTreeSet::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = member
            .enclosed_name()
            .ok_or("Office archive escapes its package")?;
        if name.as_os_str().is_empty()
            || !names.insert(name)
            || member.name().map_err(|e| e.to_string())?.contains('\\')
            || member
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("Office archive contains ambiguous or linked members".into());
        }
        let limit = 20 * 1024 * 1024;
        if member.size() > limit {
            return Err("Office archive member exceeds 20MiB".into());
        }
        total = total
            .checked_add(member.size())
            .filter(|total| *total <= 80 * 1024 * 1024)
            .ok_or("Office archive expands beyond 80MiB")?;
        if member.is_dir() {
            continue;
        }
        let actual = std::io::copy(&mut member.by_ref().take(limit + 1), &mut std::io::sink())
            .map_err(|e| e.to_string())?;
        if actual != member.size() || actual > limit {
            return Err("Office archive expanded bytes differ from its bounded size".into());
        }
    }
    Ok(())
}
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a.max(1)
}
fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        return Some((
            u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        ));
    }
    if (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) && bytes.len() >= 10 {
        return Some((
            u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32,
            u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32,
        ));
    }
    if bytes.starts_with(b"\xff\xd8") {
        let mut offset = 2usize;
        while offset + 9 < bytes.len().min(64 * 1024) {
            if bytes[offset] != 0xff {
                break;
            }
            let marker = bytes[offset + 1];
            if [0xc0, 0xc1, 0xc2].contains(&marker) {
                return Some((
                    u16::from_be_bytes(bytes[offset + 7..offset + 9].try_into().ok()?) as u32,
                    u16::from_be_bytes(bytes[offset + 5..offset + 7].try_into().ok()?) as u32,
                ));
            }
            let length =
                u16::from_be_bytes(bytes[offset + 2..offset + 4].try_into().ok()?) as usize;
            if length < 2 {
                break;
            }
            offset = offset.checked_add(length + 2)?;
        }
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        match bytes.get(12..16)? {
            b"VP8X" if bytes.len() >= 30 => {
                let le24 = |at| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], 0]);
                return Some((1 + le24(24), 1 + le24(27)));
            }
            b"VP8 " if bytes.len() >= 30 => {
                return Some((
                    (u16::from_le_bytes(bytes[26..28].try_into().ok()?) & 0x3fff) as u32,
                    (u16::from_le_bytes(bytes[28..30].try_into().ok()?) & 0x3fff) as u32,
                ))
            }
            b"VP8L" if bytes.len() >= 25 => {
                let bits = u32::from_le_bytes(bytes[21..25].try_into().ok()?);
                return Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1));
            }
            _ => {}
        }
    }
    None
}

impl OfficeApi {
    fn localized(&self, agent: Option<&str>, path: &str) -> Result<String, String> {
        static DATA: OnceLock<[Value; 5]> = OnceLock::new();
        let data = DATA.get_or_init(|| {
            [
                include_str!("../../../../desktop/src/locales/en.json"),
                include_str!("../../../../desktop/src/locales/zh.json"),
                include_str!("../../../../desktop/src/locales/zh-TW.json"),
                include_str!("../../../../desktop/src/locales/ja.json"),
                include_str!("../../../../desktop/src/locales/ko.json"),
            ]
            .map(|raw| serde_json::from_str(raw).expect("内置语言文件必须有效"))
        });
        let configured = agent
            .map(|agent| {
                let path = self.memory.config_path(agent).map_err(|e| e.to_string())?;
                lingxi_adapters::storage::agent_memory::config::read_agent_config(&path)
                    .and_then(|config| config.scan())
                    .map(|flags| flags.locale)
                    .map_err(|e| e.to_string())
            })
            .transpose()?
            .flatten();
        let preferences = self.memory.home().join("user/preferences.json");
        let prefs: Value = match std::fs::read(preferences) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
            Err(e) => return Err(e.to_string()),
        };
        let locale = configured
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or(prefs["locale"].as_str())
            .unwrap_or("en");
        let index = if matches!(locale, "zh-TW" | "zh-Hant") {
            2
        } else if locale.starts_with("zh") {
            1
        } else if locale.starts_with("ja") {
            3
        } else if locale.starts_with("ko") {
            4
        } else {
            0
        };
        let lookup = |index: usize| {
            path.split('.')
                .fold(&data[index], |value, key| &value[key])
                .as_str()
        };
        lookup(index)
            .or_else(|| lookup(0))
            .map(str::to_owned)
            .ok_or_else(|| format!("bundled Office text is unavailable: {path}"))
    }
}
fn guide(
    api: &OfficeApi,
    name: &str,
    args: &Value,
    session: &str,
    agent: &str,
) -> Result<Value, String> {
    let tr = |path: &str| api.localized(Some(agent), path);
    match name {
        "list-capabilities" => Ok(json!({"text":tr("toolDef.listCapabilities.text")?,
            "capabilities":[{"id":"markdown-cover","target":"markdown","tools":["beautify_get-cover-style-guide","beautify_create-cover","beautify_apply-cover-candidate"],"imageRatio":"3:2","responsibility":"apply-existing-image"},
            {"id":"html-style-guide","target":"html","tools":["beautify_get-html-style-guide"],"responsibility":"style-guide-only"}]})),
        "get-cover-style-guide" => {
            let tone = if args["themeTone"] == "dark" {
                "dark"
            } else {
                "light"
            };
            let extra = args["userGuidance"].as_str().unwrap_or("").trim();
            let extra = if extra.is_empty() {
                String::new()
            } else {
                tr("plugin.beautify.userGuidancePrefix")?.replace("{guidance}", extra)
            };
            let workflow = (1..=5)
                .map(|step| tr(&format!("toolDef.getCoverStyleGuide.workflow.step{step}")))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(
                json!({"text":format!("{}\n{}\n{}",tr("plugin.beautify.coverStyleGuide")?,tr(&format!("plugin.beautify.themeTone.{tone}"))?,extra),"version":"2026-05-26","themeTone":tone,"workflow":workflow}),
            )
        }
        "get-html-style-guide" => {
            let section = args["section"].as_str().unwrap_or("");
            if section.is_empty() {
                return Ok(
                    json!({"text":tr("plugin.beautify.htmlStyleGuide.router")?,"kind":"router","version":"2026-06-10"}),
                );
            }
            let key = if section == "anti-patterns" {
                "antiPatterns"
            } else {
                section
            };
            if ![
                "color",
                "typography",
                "layout",
                "components",
                "imagery",
                "motion",
                "antiPatterns",
            ]
            .contains(&key)
            {
                return Err("invalid HTML style section".into());
            }
            let mut tracking = api.guide.lock().map_err(|_| "guide state unavailable")?;
            let missing = ["color", "typography"]
                .into_iter()
                .filter(|s| {
                    tracking
                        .sessions
                        .get(session)
                        .is_none_or(|read| !read.contains(*s))
                })
                .collect::<Vec<_>>();
            if !["color", "typography"].contains(&section) && !missing.is_empty() {
                return Ok(
                    json!({"text":tr("plugin.beautify.htmlStyleGuide.mustReadFirst")?.replace("{section}",section).replace("{missing}",&missing.join(", ")),"kind":"must-read-first","missingRequired":missing,"version":"2026-06-10"}),
                );
            }
            if !tracking.sessions.contains_key(session) {
                if tracking.sessions.len() >= 200 {
                    if let Some(first) = tracking.order.pop_front() {
                        tracking.sessions.remove(&first);
                    }
                }
                tracking.order.push_back(session.into());
            }
            let read = tracking.sessions.entry(session.into()).or_default();
            read.insert(section.into());
            Ok(
                json!({"text":tr(&format!("plugin.beautify.htmlStyleGuide.{key}"))?,"kind":"section","section":section,"readSections":read,"version":"2026-06-10"}),
            )
        }
        _ => Err("unknown Beautify guide".into()),
    }
}
struct CancelFlag(Arc<AtomicBool>);
impl Drop for CancelFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn office_capabilities() -> Value {
    json!({"text":"Office reads DOCX/XLSX/XLSM, paged PDF and text documents, and exports HTML to PDF with controlled Chromium. Scanned PDF OCR and Office editing are unsupported.",
        "capabilities":[{"id":"read-document","tool":"office_read-document","supportedInputs":[".docx",".xlsx",".xlsm",".pdf",".txt",".md",".csv",".tsv",".html"],
            "outputs":["text","markdown/text for pdf","html for docx","json for xlsx"],"pdf":{"pageRange":true,"maxPages":true,"ocr":false,"math":"best-effort text extraction; formulas may degrade"},
            "limitations":["pptx reading and Office editing are unsupported","PDF tables are best-effort text","scanned PDF OCR is unsupported"],"sideEffects":false},
            {"id":"html-to-pdf","tool":"office_html-to-pdf","supportedInputs":["inline html","absolute htmlPath"],"output":".pdf SessionFile","engine":"controlled Chromium Page.printToPDF","sideEffects":true}]})
}
struct Executor {
    api: Arc<OfficeApi>,
    plugin: &'static str,
    name: &'static str,
}
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let result = async {
                let target = format!("tool:plugin:{}:{}", self.plugin, self.name);
                if request.target != target
                    || !request.arguments.digest_matches(&request.args_digest)
                {
                    return Err("exact gateway-bound builtin target and digest required".into());
                }
                let agent = self.api.caller(ctx, call, request, self.plugin).await?;
                let args = request.arguments.as_value();
                let context = op(ctx);
                let value = match (self.plugin, self.name) {
                    ("office", "list-capabilities") => office_capabilities(),
                    ("office", "read-document") => {
                        self.api
                            .document(ctx, call, request, args, &context)
                            .await?
                    }
                    ("office", "html-to-pdf") => {
                        self.api.pdf(ctx, call, request, args, &context).await?
                    }
                    ("beautify", "create-cover" | "apply-cover-candidate") => {
                        // 后台封面任务的应用步骤必须执行受理时授权冻结的写入预检：
                        // 媒体生成期间目标文档若变化，require_snapshot 响亮失败，
                        // 不能静默采纳当前版本。普通交互调用无封面作业，维持现预检。
                        let frozen = match lingxi_adapters::storage::beautify::get_for_run(
                            &self.api.storage,
                            ctx.run_id.as_str(),
                        )
                        .await
                        {
                            Ok(Some(job)) => {
                                use lingxi_adapters::storage::beautify::CoverPhase;
                                if job.phase != CoverPhase::Applying {
                                    return Err("封面作业阶段与应用动作不匹配".into());
                                }
                                let facts = job
                                    .admitted_write
                                    .ok_or("封面应用缺少受理时冻结的写入事实")?;
                                Some(
                                    crate::resourceio::ArtifactWritePreflight::from_trusted_persistent_facts(
                                        &facts,
                                    )
                                    .map_err(|e| e.to_string())?,
                                )
                            }
                            Ok(None) => None,
                            Err(e) => return Err(e.to_string().into()),
                        };
                        self.api
                            .apply_cover_checked(
                                args,
                                &context,
                                &format!(
                                    "office-tool-{}",
                                    lingxi_protocol::canon::sha256_hex(
                                        json!([ctx.run_id, call]).to_string().as_bytes()
                                    )
                                ),
                                Some((ctx, call, request)),
                                frozen.as_ref(),
                            )
                            .await?
                    }
                    ("beautify", name) => {
                        guide(&self.api, name, args, ctx.session_id.as_str(), &agent)?
                    }
                    _ => return Err("unknown builtin operation".into()),
                };
                let text = value["content"]
                    .as_str()
                    .or_else(|| value["text"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| serde_json::to_string_pretty(&value).expect("内置工具结果"));
                let mut success = ToolSuccess::text(text);
                let mut details = serde_json::Map::new();
                if self.name == "list-capabilities"
                    || (self.plugin == "beautify"
                        && !matches!(self.name, "create-cover" | "apply-cover-candidate"))
                {
                    details = value.as_object().cloned().unwrap_or_default();
                    details.remove("text");
                } else {
                    details.insert(
                        if self.plugin == "office" {
                            "office"
                        } else {
                            "beautifyCover"
                        }
                        .into(),
                        value,
                    );
                }
                success.details = Some(details);
                Ok::<_, OfficeError>(success)
            }
            .await;
            ToolExecutionResult::of_ctx(
                ctx,
                match result {
                    Ok(result) => ToolOutcome::Success { result },
                    Err(error) if error.code == "OFFICE_CANCELLED" => ToolOutcome::Cancelled,
                    Err(error) if error.effect_unknown => ToolOutcome::Unknown {
                        reason: error.message,
                    },
                    Err(error) => ToolOutcome::Failed {
                        error: ProtocolError::new(ErrorCode::Forbidden, error.message, false),
                    },
                },
            )
        })
    }
}
fn review(
    api: &OfficeApi,
    input: &ReviewInput<'_>,
    plugin: &str,
    args: &Value,
) -> Result<Value, String> {
    api.enabled(input.agent_id, plugin)?;
    Ok(
        json!({"sourceSessionId":input.session_id,"actorId":input.agent_id,"principalKind":input.principal_kind,"principalSubject":input.principal_subject,"summary":format!("{plugin} 指定资源操作"),"input":args}),
    )
}
pub fn register(
    gateway: &ToolInvocationGateway,
    api: Arc<OfficeApi>,
    budget: &SchemaBudget,
) -> Result<(), String> {
    let entries: [(&str, &str, bool); 8] = [
        ("office", "list-capabilities", false),
        ("office", "read-document", false),
        ("office", "html-to-pdf", true),
        ("beautify", "list-capabilities", false),
        ("beautify", "get-cover-style-guide", false),
        ("beautify", "get-html-style-guide", false),
        ("beautify", "create-cover", true),
        ("beautify", "apply-cover-candidate", true),
    ];
    for (plugin, name, write) in entries {
        let plugin = if plugin == "office" {
            "office"
        } else {
            "beautify"
        };
        let schema = schema(plugin, name);
        let receipt = gateway
            .registry()
            .register(
                ToolManifest {
                    origin: ToolOrigin::Plugin {
                        plugin_id: plugin.into(),
                    },
                    local_name: name.into(),
                    display_name: format!("{plugin}_{name}"),
                    aliases: vec![],
                    version: "0.1.0".into(),
                    description: format!(
                        "内置 {plugin} {name}；身份来自当前真实会话，遵守助手开关与资源权限。"
                    ),
                    input_schema: ToolSchemaDocument {
                        dialect: "json-schema/2020-12".into(),
                        schema,
                    },
                    output_schema: None,
                    permission: PermissionContract {
                        kind: if write {
                            PermissionKind::Review
                        } else {
                            PermissionKind::Read
                        },
                        capability_base: format!("{plugin}.{name}"),
                    },
                    availability: Availability::Deferred,
                    timeout_ms: Some(300000),
                    max_concurrency: Some(4),
                    declared_permission: if write {
                        DeclaredPermission::Other
                    } else {
                        DeclaredPermission::ReadOnly
                    },
                    recovery: if write {
                        lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE
                    } else {
                        lingxi_kernel::invocation::ToolRecoveryCapability {
                            read_only: true,
                            honors_idempotency_key: false,
                            externally_verifiable: false,
                        }
                    },
                },
                budget,
            )
            .map_err(|e| format!("{e:?}"))?;
        let current = api.clone();
        gateway.bind_executor_with_review(
            receipt.target_id,
            Arc::new(Executor {
                api: api.clone(),
                plugin,
                name,
            }),
            Arc::new(move |input, args| review(&current, input, plugin, args.as_value())),
            "内置工具真实助手开关、资源与审批绑定",
        );
    }
    Ok(())
}
fn schema(plugin: &str, name: &str) -> Value {
    let string = json!({"type":"string"});
    let number = json!({"type":"integer","minimum":1});
    let properties = match (plugin, name) {
        ("office", "read-document") => {
            json!({"resource":{"type":"object"},"filePath":string,"outputFormat":{"type":"string","enum":["text","markdown","html","json"]},"maxChars":number,"pageRange":string,"maxPages":number,"sheetLimit":number,"rowLimit":number,"columnLimit":number})
        }
        ("office", "html-to-pdf") => {
            json!({"html":string,"htmlPath":string,"filename":string,"outputPath":string,"overwrite":{"type":"boolean"},"pageSize":string,"landscape":{"type":"boolean"},"printBackground":{"type":"boolean"},"preferCSSPageSize":{"type":"boolean"},"margins":{"type":"object","description":"四边边距单位为英寸；小数请用精确十进制字符串，如 top: \"0.25\"。","properties":{"top":{"type":["integer","string"],"minimum":0},"bottom":{"type":["integer","string"],"minimum":0},"left":{"type":["integer","string"],"minimum":0},"right":{"type":["integer","string"],"minimum":0},"marginType":{"type":"string","enum":["custom"]}},"additionalProperties":false},"scale":{"type":["integer","string"],"minimum":1,"maximum":2,"description":"缩放0.1到2；小数请用字符串，如 \"0.8\"。"},"viewportWidth":number,"viewportHeight":number,"allowJavaScript":{"type":"boolean"},"embedLingxiFonts":{"type":"boolean"},"settleMs":{"type":"integer","minimum":0},"timeoutMs":number})
        }
        ("beautify", "get-cover-style-guide") => {
            json!({"themeTone":{"type":"string","enum":["light","dark"]},"userGuidance":string})
        }
        ("beautify", "get-html-style-guide") => {
            json!({"section":{"type":"string","enum":["color","typography","layout","components","imagery","motion","anti-patterns"]}})
        }
        ("beautify", "create-cover" | "apply-cover-candidate") => {
            json!({"targetFilePath":string,"filePath":string,"targetResource":{"type":"object"},"imageResource":{"type":"object"},"generatedFilePath":string,"imageFilePath":string,"pixelWidth":number,"pixelHeight":number})
        }
        _ => json!({}),
    };
    json!({"type":"object","properties":properties,"additionalProperties":false})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn package(name: &str, bytes: &[u8]) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        archive.write_all(bytes).unwrap();
        archive.finish().unwrap().into_inner()
    }
    #[test]
    fn document_package_is_never_expanded_outside_its_bounded_archive() {
        assert!(
            validate_package(&package("word/document.xml", b"<document>safe</document>")).is_ok()
        );
        assert!(validate_package(&package("../outside.xml", b"private")).is_err());
        assert!(validate_package(&package("word\\document.xml", b"ambiguous")).is_err());
        assert!(validate_package(&package(
            "word/document.xml",
            &vec![b'x'; 20 * 1024 * 1024 + 1]
        ))
        .is_err());
        // 符号链接成员与 ../ 同属逃逸面：unix_mode 链接位必须响亮拒绝。
        // zip crate 的 unix_permissions 只保留 0o777 权限位、丢弃文件类型位，
        // 构造链接成员必须直接写 external_attributes 的高 16 位。
        let mut linked = zip::ZipWriter::new(Cursor::new(Vec::new()));
        linked
            .start_file(
                "word/media/link.png",
                zip::write::SimpleFileOptions::default().external_attributes(0o120777 << 16),
            )
            .unwrap();
        linked.write_all(b"secret").unwrap();
        let linked = linked.finish().unwrap().into_inner();
        assert!(validate_package(&linked).is_err());
    }
    #[test]
    fn markdown_frontmatter_keeps_unrelated_fields_and_rejects_ambiguous_shapes() {
        let (mapping, body) =
            frontmatter("---\ntitle: Kept\nfuture:\n  answer: 42\n---\n# unchanged\n").unwrap();
        assert_eq!(
            mapping[&serde_yaml_ng::Value::String("title".into())].as_str(),
            Some("Kept")
        );
        assert_eq!(body, "# unchanged\n");
        assert!(frontmatter("---\n- array\n---\nbody").is_err());
    }
}
