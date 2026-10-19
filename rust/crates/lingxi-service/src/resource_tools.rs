//! 模型文件与暂存工具：目标身份来自真实调用，会话附件不能跨会话借用。
use crate::{
    file_safety::{FileSafety, Snapshot},
    resourceaccess::{ResourceAccess, ResourceOp},
    resourceio::{OpContext, ResourceIoService},
    sessionfiles::SessionFileService,
};
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::{
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin, ToolRegistry,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct ResourceTools {
    storage: Arc<RunDatabase>,
    files: Arc<SessionFileService>,
    io: Arc<ResourceIoService>,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    safety: Arc<FileSafety>,
}
impl ResourceTools {
    pub fn new(
        storage: Arc<RunDatabase>,
        files: Arc<SessionFileService>,
        io: Arc<ResourceIoService>,
        access: Arc<ResourceAccess>,
        cwd: PathBuf,
        safety: Arc<FileSafety>,
    ) -> Self {
        Self {
            storage,
            files,
            io,
            access,
            cwd,
            safety,
        }
    }
    fn context(ctx: &RunContext) -> OpContext {
        OpContext {
            principal_kind: ctx.principal.storage_kind(),
            principal_subject: ctx.principal.storage_subject(),
            session_id: ctx.session_id.to_string(),
            source: "agent_tool".into(),
            reason: Some("file resource tool".into()),
            session_path: None,
            emit: true,
            capture_origin: None,
            engine_plane: false,
        }
    }
    fn cwd(&self) -> PathBuf {
        crate::toolgateway::workspace().unwrap_or_else(|| self.cwd.clone())
    }
    async fn caller(&self, ctx: &RunContext) -> Result<(), String> {
        self.files
            .ensure_stage_session(ctx)
            .await
            .map_err(|e| e.to_string())?;
        let owned=self.storage.query_one_text("SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id JOIN run_attempts a ON a.run_id=r.run_id WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND s.owner_user_id=?4 AND r.generation=?5 AND a.attempt=?6 AND a.generation=r.generation AND r.status IN ('running','waiting_approval') AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1)", vec![ctx.run_id.to_string(),ctx.session_id.to_string(),ctx.principal.storage_kind().into(),ctx.principal.storage_subject(),ctx.generation.to_string(),ctx.attempt.to_string()]).await.map_err(|e|e.to_string())?;
        if owned.is_none() {
            return Err("resource tool requires an active owned Run".into());
        }
        Ok(())
    }
    async fn reference(&self, ctx: &RunContext, args: &Value) -> Result<Value, String> {
        let mut value = args
            .get("resource")
            .or_else(|| args.get("source"))
            .or_else(|| args.get("ref"))
            .unwrap_or(args)
            .clone();
        for scope in [args, &value] {
            if scope
                .get("sessionId")
                .and_then(Value::as_str)
                .is_some_and(|id| id != ctx.session_id.as_str())
            {
                return Err("session identity mismatch".into());
            }
            if let Some(path) = scope.get("sessionPath").and_then(Value::as_str) {
                if path != format!("native:{}", ctx.session_id) {
                    let id = lingxi_adapters::storage::legacy_session_files::session_id_for_path(
                        &self.storage,
                        Path::new(path),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    if id != ctx.session_id.as_str() {
                        return Err("session path identity mismatch".into());
                    }
                }
            }
        }
        let mut file_id = value
            .get("fileId")
            .and_then(Value::as_str)
            .map(str::to_string);
        if value["kind"] == "resource" {
            file_id = crate::resources::file_id_from_session_file_resource_id(
                value["resourceId"].as_str().ok_or("resourceId required")?,
            )
            .map(str::to_string);
            if file_id.is_none() {
                return Err("resource identity is not an owned session file".into());
            }
        }
        if let Some(file_id) = file_id {
            let file = self
                .files
                .get_file(ctx.session_id.as_str(), &file_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("file is unavailable in this session")?;
            if file.status != "available" {
                return Err("session file is not readable".into());
            }
            return Ok(json!({"kind":"session-file","fileId":file.file_id}));
        }
        if value.get("url").and_then(Value::as_str).is_some() {
            value["kind"] = json!("url");
            return Ok(value);
        }
        if value["kind"] == "mount" {
            if value.get("fileId").is_some() || value.get("realPath").is_some() {
                return Err("mount reference cannot contain a session-file identity".into());
            }
            let actual = self
                .io
                .mount_claim_path(&value, &Self::context(ctx))
                .map_err(|e| e.to_string())?;
            crate::toolgateway::require_bound_resource(&actual, ResourceOp::Read)?;
            if value
                .get("filePath")
                .is_some_and(|path| path.as_str() != actual.to_str())
            {
                return Err("mount path claim mismatch".into());
            }
            return Ok(value);
        }
        let raw = value
            .get("path")
            .and_then(Value::as_str)
            .ok_or("target_required: pass fileId, path or resource")?;
        let scope = self
            .access
            .authorize(
                ctx.principal.storage_kind(),
                &ctx.principal.storage_subject(),
                ctx.session_id.as_str(),
                Path::new(raw),
                &self.cwd(),
                ResourceOp::Read,
            )
            .map_err(|e| e.to_string())?;
        Ok(json!({"kind":"local-file","path":scope.path}))
    }
    fn scopes(
        &self,
        caller: &crate::toolgateway::ResourceExtractionInput,
        args: &Value,
    ) -> Result<Vec<crate::resourceaccess::ResourceScope>, String> {
        let value = args
            .get("resource")
            .or_else(|| args.get("source"))
            .or_else(|| args.get("ref"))
            .unwrap_or(args);
        let cwd = self.cwd();
        let mut scopes = Vec::new();
        if value["kind"] == "mount" {
            // 审批绑定当时登记解析出的真实来源；执行前仍由同一提取器重新核对。
            let context = OpContext {
                principal_kind: caller.principal_kind,
                principal_subject: caller.principal_subject.clone(),
                session_id: caller.session_id.clone(),
                source: "agent_tool".into(),
                reason: Some("file resource approval".into()),
                session_path: None,
                emit: true,
                capture_origin: None,
                engine_plane: false,
            };
            let path = self
                .io
                .mount_claim_path(value, &context)
                .map_err(|e| e.to_string())?;
            scopes.push(crate::resourceaccess::ResourceScope::new(
                path,
                ResourceOp::Read,
            ));
        } else if let Some(path) = value.get("path").and_then(Value::as_str) {
            scopes.push(
                self.access
                    .authorize(
                        caller.principal_kind,
                        &caller.principal_subject,
                        &caller.session_id,
                        Path::new(path),
                        &cwd,
                        ResourceOp::Read,
                    )
                    .map_err(|e| e.to_string())?,
            );
        }
        if args["action"] == "copy" {
            let target = args.get("target").cloned().unwrap_or(json!({}));
            let raw = args
                .get("targetPath")
                .or_else(|| target.get("path"))
                .or_else(|| target.get("targetPath"))
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(
                        args.get("targetDir")
                            .or_else(|| target.get("dir"))
                            .or_else(|| target.get("targetDir"))
                            .and_then(Value::as_str)
                            .unwrap_or("."),
                    )
                });
            let scope = self
                .access
                .authorize(
                    caller.principal_kind,
                    &caller.principal_subject,
                    &caller.session_id,
                    &raw,
                    &cwd,
                    ResourceOp::Write,
                )
                .map_err(|e| e.to_string())?;
            if !scope.path.starts_with(&cwd) {
                return Err("copy target must be inside current working directory".into());
            }
            scopes.push(scope);
        }
        Ok(scopes)
    }
    async fn track_path(&self, ctx: &RunContext, value: &Value) -> Result<(), String> {
        // 挂载原地路径继续受挂载授权管理，不能另铸可绕过撤权的永久文件ID。
        // 复制到工作区的新副本由下方真实copy提交独立登记。
        if value["resource"]["kind"] == "mount" {
            return Ok(());
        }
        if let Some(path) = value.get("filePath").and_then(Value::as_str) {
            self.files
                .register_file(
                    ctx.session_id.as_str(),
                    Path::new(path),
                    None,
                    None,
                    "unknown",
                )
                .await
                .map_err(|e| format!("resource_path_registration_failed: {e}"))?;
        }
        Ok(())
    }
    fn check_mount_result(reference: &Value, result: &Value) -> Result<(), String> {
        if reference["kind"] == "mount" {
            let path = result["filePath"]
                .as_str()
                .ok_or("mount result has no local path")?;
            crate::toolgateway::require_bound_resource(Path::new(path), ResourceOp::Read)?;
        }
        Ok(())
    }
    async fn invoke(&self, ctx: &RunContext, request: &ToolRequest) -> Result<Value, String> {
        self.caller(ctx).await?;
        let args = request.arguments.as_value();
        let reference = self.reference(ctx, args).await?;
        let op = Self::context(ctx);
        if request.target.as_str() == "tool:first-party:materialize" {
            let value = self
                .io
                .materialize(&reference, &op)
                .await
                .map_err(|e| e.to_string())?;
            Self::check_mount_result(&reference, &value)?;
            self.track_path(ctx, &value).await?;
            if reference["kind"] == "session-file" {
                // 归属已由 reference 核对；只授予此次真实文件版本，不开放附件目录。
                let path = PathBuf::from(
                    value["filePath"]
                        .as_str()
                        .ok_or("session file has no local path")?,
                );
                let retained = crate::task_supervisor::retain_blocking_run_permit(ctx)
                    .map_err(|e| e.to_string())?;
                let cancel = crate::task_supervisor::blocking_run_cancel_scope(ctx)
                    .map_err(|e| e.to_string())?;
                let lease = self.files.payload_lease().await;
                let access = self.access.clone();
                let caller = ctx.clone();
                tokio::task::spawn_blocking(move || {
                    let _retained = retained;
                    let _lease = lease;
                    if cancel.as_ref().is_some_and(|scope| scope.is_cancelled()) {
                        return Err("session file materialization cancelled".to_string());
                    }
                    let snapshot = Snapshot::read(&path).map_err(|error| error.to_string())?;
                    access.grant_materialized_file(
                        caller.principal.storage_kind(),
                        &caller.principal.storage_subject(),
                        caller.session_id.as_str(),
                        &path,
                        &snapshot,
                    )
                })
                .await
                .map_err(|error| error.to_string())??;
            }
            return Ok(value);
        }
        let action = args["action"].as_str().ok_or("action required")?;
        if action == "stat" {
            let value = self
                .io
                .stat(&reference, &op)
                .await
                .map_err(|e| e.to_string())?;
            if value["exists"] == false {
                return Err("file does not exist".into());
            }
            Self::check_mount_result(&reference, &value)?;
            self.track_path(ctx, &value).await?;
            return Ok(value);
        }
        // 先解析可信路径，再由有界无链接句柄读取；不使用会无限读取的通用read接口。
        let materialized = self
            .io
            .materialize(&reference, &op)
            .await
            .map_err(|e| e.to_string())?;
        Self::check_mount_result(&reference, &materialized)?;
        if materialized["isDirectory"] == true {
            return Err("file action requires a regular file, not a directory".into());
        }
        let source = PathBuf::from(
            materialized["filePath"]
                .as_str()
                .ok_or("resource has no local path")?,
        );
        let snapshot = if reference["kind"] == "mount" {
            let value = self
                .io
                .read_filesystem_snapshot(&reference, &op)
                .await
                .map_err(|e| e.to_string())?;
            if value.0 != source {
                return Err("mount source changed after materialization".into());
            }
            value
        } else {
            let retained = crate::task_supervisor::retain_blocking_run_permit(ctx)
                .map_err(|e| e.to_string())?;
            let cancel = crate::task_supervisor::blocking_run_cancel_scope(ctx)
                .map_err(|e| e.to_string())?;
            let lease = self.files.payload_lease().await;
            let path_authority = if reference["kind"] == "local-file" || reference["kind"] == "url"
            {
                Some((Arc::clone(&self.access), Self::context(ctx), self.cwd()))
            } else {
                None
            };
            tokio::task::spawn_blocking(move || {
                let _retained = retained;
                let _lease = lease;
                if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
                    return Err("file read cancelled".to_string());
                }
                let snapshot = Snapshot::read(&source).map_err(|e| e.to_string())?;
                if let Some((access, op, cwd)) = path_authority {
                    access
                        .authorize(
                            op.principal_kind,
                            &op.principal_subject,
                            &op.session_id,
                            &source,
                            &cwd,
                            ResourceOp::Read,
                        )
                        .map_err(|e| e.to_string())?;
                    access.validate_materialized_snapshot(
                        op.principal_kind,
                        &op.principal_subject,
                        &op.session_id,
                        &source,
                        &snapshot,
                    )?;
                }
                Ok((source, snapshot))
            })
            .await
            .map_err(|e| e.to_string())??
        };
        if action == "extract" {
            let (path, snapshot) = snapshot;
            let format = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let bytes = snapshot.bytes;
            let retained = crate::task_supervisor::retain_blocking_run_permit(ctx)
                .map_err(|e| e.to_string())?;
            let cancel = crate::task_supervisor::blocking_run_cancel_scope(ctx)
                .map_err(|e| e.to_string())?;
            let parsed = tokio::task::spawn_blocking(move || {
                let _retained = retained;
                if cancel.as_ref().is_some_and(|scope| scope.is_cancelled()) {
                    return Err("file extraction cancelled".to_string());
                }
                match format.as_str() {
                    "docx" => {
                        crate::office::docx(&bytes).map(|d| (d.text, "docx".to_string(), 0usize))
                    }
                    "xlsx" => crate::office::xlsx_for_read(&bytes)
                        .map(|(text, omitted)| (text, "xlsx".to_string(), omitted)),
                    _ => std::str::from_utf8(&bytes)
                        .map(|s| {
                            (
                                s.trim_start_matches('\u{feff}').to_string(),
                                "text".to_string(),
                                0usize,
                            )
                        })
                        .map_err(|_| {
                            "document format is not supported by the configured extractor"
                                .to_string()
                        }),
                }
            })
            .await
            .map_err(|e| e.to_string())??;
            let total = parsed.0.chars().count();
            let truncated = total > 200_000 || parsed.2 > 0;
            let mut text = parsed.0.chars().take(200_000).collect::<String>();
            if total > 200_000 {
                text.push_str(&format!(
                    "\n[Truncated: {} characters omitted]",
                    total - 200_000
                ));
            }
            self.track_path(ctx, &materialized).await?;
            let mut result = json!({"text":text,"source":{"filePath":path,"filename":path.file_name().map(|n|n.to_string_lossy()),"resourceKey":materialized["resourceKey"],"version":snapshot.stamp},"format":parsed.1,"totalChars":total,"truncated":truncated,"omittedSheets":parsed.2});
            if reference["kind"] == "mount" {
                result["source"]["resource"] = materialized["resource"].clone();
            }
            return Ok(result);
        }
        if action != "copy" {
            return Err("unsupported file action".into());
        }
        let cwd = self.cwd();
        let target = args.get("target").cloned().unwrap_or(json!({}));
        let filename = args
            .get("filename")
            .or_else(|| target.get("filename"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                snapshot
                    .0
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            });
        if Path::new(&filename).components().count() != 1 || filename == "." || filename == ".." {
            return Err("invalid copy filename".into());
        }
        let raw = args
            .get("targetPath")
            .or_else(|| target.get("path"))
            .or_else(|| target.get("targetPath"))
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(
                    args.get("targetDir")
                        .or_else(|| target.get("dir"))
                        .or_else(|| target.get("targetDir"))
                        .and_then(Value::as_str)
                        .unwrap_or("."),
                )
                .join(filename)
            });
        let scope = self
            .access
            .authorize(
                op.principal_kind,
                &op.principal_subject,
                &op.session_id,
                &raw,
                &cwd,
                ResourceOp::Write,
            )
            .map_err(|e| e.to_string())?;
        if !scope.path.starts_with(&cwd) {
            return Err("copy target must be inside current working directory".into());
        }
        let policy = args
            .get("conflictPolicy")
            .and_then(Value::as_str)
            .unwrap_or("fail")
            .to_string();
        if !["fail", "rename", "overwrite"].contains(&policy.as_str()) {
            return Err("invalid conflictPolicy".into());
        }
        let access = Arc::clone(&self.access);
        let safety = Arc::clone(&self.safety);
        let mut path = scope.path;
        let retained =
            crate::task_supervisor::retain_blocking_run_permit(ctx).map_err(|e| e.to_string())?;
        let cancel =
            crate::task_supervisor::blocking_run_cancel_scope(ctx).map_err(|e| e.to_string())?;
        let lease = self.files.payload_lease().await;
        let copied = tokio::task::spawn_blocking(move || {
            let _retained = retained;
            let active = |path: &Path| {
                !cancel.as_ref().is_some_and(|c| c.is_cancelled())
                    && access
                        .authorize(
                            op.principal_kind,
                            &op.principal_subject,
                            &op.session_id,
                            path,
                            &cwd,
                            ResourceOp::Write,
                        )
                        .is_ok_and(|scope| scope.path == path && path.starts_with(&cwd))
            };
            // 沿工作区目录句柄逐级创建，链接或权限改变都会在发布前拒绝。
            let parent = path.parent().ok_or("copy target has no parent")?;
            let mut dir = crate::filetools::fsio::DirFd::open(&cwd).map_err(|e| e.to_string())?;
            let mut current = cwd.clone();
            for component in parent
                .strip_prefix(&cwd)
                .map_err(|e| e.to_string())?
                .components()
            {
                let std::path::Component::Normal(name) = component else {
                    return Err("invalid copy directory".into());
                };
                current.push(name);
                if !active(&current) {
                    return Err("copy directory is no longer authorized".into());
                }
                dir = dir.child_directory(name, true).map_err(|e| e.to_string())?;
            }
            if !dir.same_directory(parent).map_err(|e| e.to_string())? {
                return Err("copy directory changed".into());
            }
            let bytes = &snapshot.1.bytes;
            let original = path.clone();
            for suffix in 0..1000 {
                if path.exists() {
                    if policy == "overwrite" {
                        let before = Snapshot::read(&path).map_err(|e| e.to_string())?;
                        safety
                            .replace(&op.session_id, &path, &before, bytes, || active(&path))
                            .map_err(|e| e.to_string())?;
                        return Ok((path, lease));
                    }
                    if policy == "fail" {
                        return Err("copy destination already exists".to_string());
                    }
                } else {
                    match safety.create(&op.session_id, &path, bytes, || active(&path)) {
                        Ok(()) => return Ok((path, lease)),
                        Err(crate::file_safety::WriteError::Conflict)
                            if policy == "rename" && path.exists() => {}
                        Err(e) => return Err(e.to_string()),
                    }
                }
                let stem = raw_file_stem(&original);
                let ext = original
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy()))
                    .unwrap_or_default();
                path = path.with_file_name(format!("{stem} ({}){ext}", suffix + 1));
            }
            Err("copy rename attempts exhausted".into())
        })
        .await
        .map_err(|e| e.to_string())??;
        let file = self
            .files
            .register_file_under_lease(
                ctx.session_id.as_str(),
                &copied.0,
                None,
                None,
                "file_copy",
                &copied.1,
            )
            .await
            .map_err(|e| format!("copy_written_registration_failed: {e}"))?;
        Ok(
            json!({"filePath":copied.0,"file":file,"sessionFile":file,"media":{"items":[{"type":"session_file","fileId":file.file_id,"sessionId":file.session_id,"filePath":file.file_path,"filename":file.filename,"mime":file.mime,"size":file.size_bytes,"kind":file.kind}]}}),
        )
    }
}
fn raw_file_stem(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
struct Executor(Arc<ResourceTools>);
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let outcome = match self.0.invoke(ctx, request).await {
                Ok(value) => {
                    let mut result = ToolSuccess::text(value.to_string());
                    result.truncated = value["truncated"] == true;
                    result.details = value.as_object().cloned();
                    ToolOutcome::Success { result }
                }
                Err(error) => ToolOutcome::Failed {
                    error: ProtocolError::new(ErrorCode::InvalidMessage, error, false),
                },
            };
            ToolExecutionResult::of_ctx(ctx, outcome)
        })
    }
}
pub fn register(
    registry: &ToolRegistry,
    gateway: &crate::toolgateway::ToolInvocationGateway,
    tools: Arc<ResourceTools>,
    budget: &SchemaBudget,
) {
    register_selected(registry, gateway, tools, budget, true);
}
/// 无工作区时只登记暂存读取入口，不向模型新增复制等写操作。
pub fn register_materialize(
    registry: &ToolRegistry,
    gateway: &crate::toolgateway::ToolInvocationGateway,
    tools: Arc<ResourceTools>,
    budget: &SchemaBudget,
) {
    register_selected(registry, gateway, tools, budget, false);
}
fn register_selected(
    registry: &ToolRegistry,
    gateway: &crate::toolgateway::ToolInvocationGateway,
    tools: Arc<ResourceTools>,
    budget: &SchemaBudget,
    include_file: bool,
) {
    for (name,kind,description) in [("file",PermissionKind::ArgumentAwareFile,"Inspect metadata, copy without overwriting by default, or extract document text with its source. Office docx/xlsx and UTF-8 text are available; unsupported binary formats fail explicitly."),("materialize",PermissionKind::Read,"Resolve an owned readable resource to its actual absolute local path. Remote HTTP resources are copied with bounded public-network access, without modifying their source.")] {
        if name=="file"&&!include_file{continue;}
        let mut properties=json!({"fileId":{"type":"string"},"sessionId":{"type":"string"},"sessionPath":{"type":"string"},"resource":{"type":"object","additionalProperties":true},"path":{"type":"string"}});
        if name=="file" {for(k,v) in json!({"action":{"type":"string","enum":["stat","copy","extract"]},"source":{"type":"object","additionalProperties":true},"ref":{"type":"object","additionalProperties":true},"target":{"type":"object","additionalProperties":true},"targetPath":{"type":"string"},"targetDir":{"type":"string"},"filename":{"type":"string"},"conflictPolicy":{"type":"string","enum":["fail","rename","overwrite"]}}).as_object().unwrap() {properties[k]=v.clone();}}
        let mut schema=json!({"type":"object","properties":properties,"additionalProperties":false});if name=="file" {schema["required"]=json!(["action"]);}
        let manifest=ToolManifest{origin:ToolOrigin::FirstParty,local_name:name.into(),display_name:name.into(),aliases:vec![],version:"1.0.0".into(),description:description.into(),input_schema:ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema},output_schema:None,permission:PermissionContract{kind,capability_base:format!("{name}.resolve")},availability:Availability::Available,timeout_ms:Some(30_000),max_concurrency:Some(4),declared_permission:if name=="file" {DeclaredPermission::Execute}else{DeclaredPermission::None},recovery:lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE};
        let registered=registry.register(manifest,budget).expect("资源工具必须登记成功");let scoped=Arc::clone(&tools);gateway.bind_executor_with_resources(registered.target_id,Arc::new(Executor(Arc::clone(&tools))),Some(Arc::new(move |caller,args|scoped.scopes(caller,args.as_value()))),"真实资源文件工具");
    }
}
