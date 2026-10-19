//! 即梦只允许显式配置的 CLI 与私有登录目录；宿主监督进程，SDK 包只做纯解析。
use crate::{
    media::MediaSpec,
    procsupervisor::{
        ExitFact, ProcessKind, ProcessOwner, ProcessSupervisor, RecordPhase, SpawnSpec,
        SupervisorLimits, TerminationReason,
    },
    ServiceClock,
};
use lingxi_kernel::{
    ports::{ToolExecutorPort, ToolOutcome, ToolRequest},
    toolcatalog::SchemaBudget,
    RunContext,
};
use lingxi_protocol::ToolCallId;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Clone)]
pub struct JimengLaunch {
    pub executable: PathBuf,
    pub helper_argv: Vec<String>,
    pub private_root: PathBuf,
    /// 目录必须在本执行器私有根内；不会采集或继承用户原 HOME。
    pub external_login_home: PathBuf,
    pub credential_grant_generation: u64,
}
pub struct JimengRuntime {
    launch: JimengLaunch,
    supervisor: Arc<ProcessSupervisor>,
    parser: Arc<crate::workerrpc::WorkerRuntime>,
    access: Arc<crate::resourceaccess::ResourceAccess>,
    cache: Mutex<Option<Value>>,
    discover: tokio::sync::Mutex<()>,
    revoked: AtomicBool,
}
impl JimengRuntime {
    pub fn new(
        mut launch: JimengLaunch,
        clock: Arc<dyn ServiceClock>,
    ) -> Result<Arc<Self>, String> {
        if !launch.executable.is_absolute()
            || !launch.executable.is_file()
            || launch.helper_argv.is_empty()
            || !Path::new(&launch.helper_argv[0]).is_absolute()
            || !launch.private_root.is_absolute()
            || !launch.external_login_home.is_absolute()
            || launch.credential_grant_generation == 0
        {
            return Err("即梦执行器必须显式配置可执行文件、私有目录和外部登录许可".into());
        }
        std::fs::create_dir_all(&launch.private_root).map_err(|_| "无法建立即梦私有目录")?;
        launch.private_root = launch
            .private_root
            .canonicalize()
            .map_err(|_| "即梦私有目录无效")?;
        if !launch.external_login_home.starts_with(&launch.private_root)
            || launch.external_login_home == launch.private_root
        {
            return Err("即梦登录目录必须位于独立执行器目录内".into());
        }
        std::fs::create_dir_all(&launch.external_login_home).map_err(|_| "无法建立即梦登录目录")?;
        if launch.external_login_home.canonicalize().ok().as_ref()
            != Some(&launch.external_login_home)
        {
            return Err("即梦登录目录不允许符号链接".into());
        }
        launch.executable = launch
            .executable
            .canonicalize()
            .map_err(|_| "即梦程序路径无效")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&launch.private_root, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "无法保护即梦私有目录")?;
            std::fs::set_permissions(
                &launch.external_login_home,
                std::fs::Permissions::from_mode(0o700),
            )
            .map_err(|_| "无法保护即梦登录目录")?;
        }
        let mut limits =
            SupervisorLimits::with_spill_dir(launch.private_root.join("process-spill"));
        limits.live_cap = 8;
        limits.settled_ring_cap = 32;
        limits.output_window_bytes = 2 * 1024 * 1024;
        limits.spill_cap_bytes = 2 * 1024 * 1024;
        limits.cleanup_timeout = Duration::from_secs(2);
        let supervisor = Arc::new(
            ProcessSupervisor::new(clock.clone(), limits).map_err(|_| "即梦进程监督器无法启动")?,
        );
        Ok(Arc::new(Self {
            launch,
            supervisor,
            parser: crate::workerrpc::WorkerRuntime::new(
                crate::workerrpc::WorkerLimits {
                    max_request_bytes: 10 * 1024 * 1024,
                    max_line_bytes: 1024 * 1024,
                    max_callbacks: 0,
                    deadline_ms: 30_000,
                    max_concurrent_workers: 4,
                },
                None,
                clock,
            ),
            access: Arc::new(
                crate::resourceaccess::ResourceAccess::new(&[])
                    .map_err(|_| "即梦解析资源边界无法启动")?,
            ),
            cache: Mutex::new(None),
            discover: tokio::sync::Mutex::new(()),
            revoked: AtomicBool::new(false),
        }))
    }
    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
    }
    fn grant(&self, generation: u64) -> Result<(), String> {
        if self.revoked.load(Ordering::Acquire)
            || generation != self.launch.credential_grant_generation
        {
            return Err("即梦外部登录许可已失效".into());
        }
        Ok(())
    }
    fn fingerprint(&self) -> Result<Value, String> {
        let metadata =
            std::fs::symlink_metadata(&self.launch.executable).map_err(|_| "即梦程序已不存在")?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("即梦程序已被替换".into());
        }
        let mtime_ms = metadata
            .modified()
            .map_err(|_| "即梦程序身份不可读取")?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "即梦程序时间无效")?
            .as_millis();
        // 指纹要作为工具参数经过正式预算校验：浮点会被安全整数边界拒绝，毫秒必须用整数。
        let mtime_ms = u64::try_from(mtime_ms).map_err(|_| "即梦程序时间无效")?;
        let mut value =
            json!({"path":self.launch.executable,"size":metadata.len(),"mtimeMs":mtime_ms});
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            value["device"] = json!(metadata.dev());
            value["inode"] = json!(metadata.ino());
            value["ctime"] = json!([metadata.ctime(), metadata.ctime_nsec()]);
        }
        Ok(value)
    }
    pub async fn refresh_capabilities(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Value, String> {
        self.grant(self.launch.credential_grant_generation)?;
        let _serial = self.discover.lock().await;
        self.grant(self.launch.credential_grant_generation)?;
        let before = self.fingerprint()?;
        if let Some(cached) = self
            .cache
            .lock()
            .map_err(|_| "即梦能力缓存不可读")?
            .as_ref()
        {
            if cached["hostFingerprint"] == before {
                return Ok(cached.clone());
            }
        }
        let version = self
            .command(
                ctx,
                call,
                vec!["--version".into()],
                false,
                self.launch.credential_grant_generation,
                &self.launch.private_root,
                cancel,
                30_000,
            )
            .await?;
        let mut outputs = serde_json::Map::new();
        for mode in ["text2image", "image2image", "text2video", "image2video"] {
            outputs.insert(
                mode.into(),
                json!(
                    self.command(
                        ctx,
                        call,
                        vec![mode.into(), "--help".into()],
                        false,
                        self.launch.credential_grant_generation,
                        &self.launch.private_root,
                        cancel,
                        30_000
                    )
                    .await?
                ),
            );
        }
        if self.fingerprint()? != before {
            return Err("即梦程序在能力探测期间发生变化".into());
        }
        let mut snapshot=self.helper(ctx,call,"jimeng.capabilities",json!({"outputs":outputs,"versionOutput":version,"fingerprint":{"path":before["path"],"mtimeMs":before["mtimeMs"],"size":before["size"]}})).await?;
        snapshot["hostFingerprint"] = before;
        snapshot["credentialGrantGeneration"] = json!(self.launch.credential_grant_generation);
        *self.cache.lock().map_err(|_| "即梦能力缓存不可写")? = Some(snapshot.clone());
        Ok(snapshot)
    }
    pub fn review_route(&self, kind: &str, spec: &MediaSpec) -> Result<Value, String> {
        self.grant(self.launch.credential_grant_generation)?;
        let cached = self
            .cache
            .lock()
            .map_err(|_| "即梦能力缓存不可读")?
            .clone()
            .ok_or("即梦能力尚未探测，请先查询媒体选项")?;
        if self.fingerprint()? != cached["hostFingerprint"] {
            return Err("即梦程序已更新，需要重新探测真实能力".into());
        }
        let group = match kind {
            "image" => "imageGeneration",
            "video" => "videoGeneration",
            _ => return Err("即梦不支持语音生成".into()),
        };
        let model = spec
            .model
            .as_deref()
            .or_else(|| cached["media"][group]["defaultModelId"].as_str())
            .ok_or("即梦没有实际发现的默认模型")?;
        let actual = cached["media"][group]["models"]
            .as_array()
            .and_then(|models| {
                models.iter().find(|m| {
                    m["id"] == model
                        || m["aliases"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|a| a == model))
                })
            })
            .ok_or("模型不在当前即梦能力列表中")?;
        Ok(
            json!({"version":1,"kind":kind,"provider":"jimeng-cli","model":actual["id"],"protocol":if kind=="image"{"jimeng-cli-images"}else{"jimeng-cli-videos"},"credentialSource":"external","credentialBoundary":"dreamina-cli-login","credentialGrantGeneration":self.launch.credential_grant_generation,"fingerprint":cached["hostFingerprint"],"cliVersion":cached["version"],"opaqueRemoteRequest":true}),
        )
    }
    pub fn describe(&self, kind: &str) -> Result<Value, String> {
        self.grant(self.launch.credential_grant_generation)?;
        let cached = self
            .cache
            .lock()
            .map_err(|_| "即梦能力缓存不可读")?
            .clone()
            .ok_or("即梦能力尚未探测")?;
        if self.fingerprint()? != cached["hostFingerprint"] {
            return Err("即梦程序已更新，需要重新探测真实能力".into());
        }
        let group = if kind == "image" {
            "imageGeneration"
        } else if kind == "video" {
            "videoGeneration"
        } else {
            return Err("即梦媒体类型不受支持".into());
        };
        Ok(
            json!({"providerId":"jimeng-cli","models":cached["media"][group]["models"],"defaultModelId":cached["media"][group]["defaultModelId"],"source":"actual_cli_version_and_four_mode_help","remoteCancel":"unsupported","credentialSource":"external"}),
        )
    }
    pub fn validate_route(&self, route: &Value) -> Result<u64, String> {
        let generation = route["credentialGrantGeneration"]
            .as_u64()
            .ok_or("即梦授权回执损坏")?;
        self.grant(generation)?;
        if route.as_object().is_none_or(|object| object.len() != 11)
            || route["version"] != 1
            || !matches!(route["kind"].as_str(), Some("image" | "video"))
            || route["protocol"]
                != if route["kind"] == "image" {
                    "jimeng-cli-images"
                } else {
                    "jimeng-cli-videos"
                }
            || route["model"].as_str().is_none_or(|model| {
                model.is_empty() || model.len() > 256 || model.chars().any(char::is_control)
            })
            || route["opaqueRemoteRequest"] != true
            || route["provider"] != "jimeng-cli"
            || route["credentialSource"] != "external"
            || route["credentialBoundary"] != "dreamina-cli-login"
            || route["fingerprint"] != self.fingerprint()?
        {
            return Err("即梦执行边界在批准后已改变".into());
        }
        Ok(generation)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn submit(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        route: &Value,
        spec: &MediaSpec,
        references: &[lingxi_adapters::models::operations::ImageReference],
        operation: &str,
        cancel: &Arc<AtomicBool>,
        progress: &crate::media::MediaProgress,
    ) -> Result<String, String> {
        let generation = self.validate_route(route)?;
        if spec.count.unwrap_or(1) != 1
            || !spec.options.is_empty()
            || spec.quality.is_some()
            || spec.format.is_some()
        {
            return Err("当前即梦CLI只支持已探测的单次生成参数".into());
        }
        let directory = self.job_directory(operation)?;
        let input = directory.join("input");
        std::fs::create_dir_all(&input).map_err(|_| "即梦参考图目录不可写")?;
        if input.canonicalize().ok().as_ref() != Some(&input) {
            return Err("即梦参考图目录被替换".into());
        }
        let mut paths = Vec::new();
        for (index, reference) in references.iter().enumerate() {
            use std::io::Write;
            let lingxi_adapters::models::operations::ImageReference::Bytes { bytes, mime, .. } =
                reference
            else {
                return Err("即梦参考图必须先由宿主读取当前会话文件".into());
            };
            let ext = match mime.as_str() {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/webp" => "webp",
                _ => return Err("即梦参考图格式不受支持".into()),
            };
            if bytes.len() > 20 * 1024 * 1024 {
                return Err("即梦参考图超出限制".into());
            }
            let path = input.join(format!("{index}.{ext}"));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| "即梦参考图无法建立独立私有副本")?;
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "即梦参考图无法保存私有副本")?;
            paths.push(path.to_string_lossy().into_owned());
        }
        let snapshot = self
            .cache
            .lock()
            .map_err(|_| "即梦能力缓存不可读")?
            .clone()
            .ok_or("即梦能力尚未探测")?;
        let plan=self.helper(ctx,call,"jimeng.submit-plan",json!({"kind":route["kind"],"snapshot":snapshot,"params":{"prompt":spec.prompt,"providerId":"jimeng-cli","modelId":route["model"],"image":paths,"ratio":spec.ratio,"resolution":spec.resolution,"duration":spec.duration,"mode":spec.mode}})).await?;
        let argv = plan["argv"]
            .as_array()
            .ok_or("即梦SDK没有返回参数计划")?
            .iter()
            .map(|arg| arg.as_str().map(String::from).ok_or("即梦参数计划损坏"))
            .collect::<Result<Vec<_>, _>>()?;
        self.command(
            ctx,
            call,
            vec!["user_credit".into()],
            true,
            generation,
            &directory,
            cancel,
            30_000,
        )
        .await
        .map_err(|_| "即梦登录状态不可用，请在专用登录目录完成授权；没有提交生成请求")?;
        progress.dispatched().await?;
        let output = self
            .command(
                ctx, call, argv, true, generation, &directory, cancel, 120_000,
            )
            .await?;
        let parsed = self
            .helper(ctx, call, "jimeng.parse-result", json!({"output":output}))
            .await?;
        let id = parsed["submitId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 4096 && !id.chars().any(char::is_control))
            .ok_or("即梦提交没有返回实际submit_id；结果未确认，不会重投")?;
        if parsed["status"] == "failed" {
            return Err("即梦报告生成请求失败".into());
        }
        Ok(id.into())
    }
    /// CLI 内部的查询及下载是不可拆分的外部边界；宿主仍单独核验实际下载文件。
    #[allow(clippy::too_many_arguments)]
    pub async fn query(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        route: &Value,
        remote_id: &str,
        operation: &str,
        cancel: &Arc<AtomicBool>,
    ) -> Result<Value, String> {
        let generation = self.validate_route(route)?;
        if remote_id.is_empty() || remote_id.len() > 4096 || remote_id.chars().any(char::is_control)
        {
            return Err("即梦远端任务编号损坏".into());
        }
        let directory = self.job_directory(operation)?;
        let output_dir = directory.join("output");
        std::fs::create_dir_all(&output_dir).map_err(|_| "即梦下载目录不可写")?;
        if output_dir.canonicalize().ok().as_ref() != Some(&output_dir) {
            return Err("即梦下载目录被替换".into());
        }
        let output = self
            .command(
                ctx,
                call,
                vec![
                    "query_result".into(),
                    "--submit_id".into(),
                    remote_id.into(),
                    "--download_dir".into(),
                    output_dir.to_string_lossy().into_owned(),
                ],
                true,
                generation,
                &directory,
                cancel,
                120_000,
            )
            .await?;
        let parsed = self
            .helper(ctx, call, "jimeng.parse-result", json!({"output":output}))
            .await?;
        match parsed["status"].as_str() {
            Some("failed") => Ok(json!({"status":"failed","message":"即梦报告媒体生成失败"})),
            Some("success") => {
                let mut files = Vec::new();
                for (index, entry) in std::fs::read_dir(&output_dir)
                    .map_err(|_| "即梦下载目录不可读")?
                    .enumerate()
                {
                    if index >= 128 {
                        return Err("即梦下载目录条目超出核验上限".into());
                    }
                    let entry = entry.map_err(|_| "即梦下载条目不可读")?;
                    let meta = entry.file_type().map_err(|_| "即梦下载条目信息不可读")?;
                    if !meta.is_file() || meta.is_symlink() {
                        return Err("即梦下载目录含非普通文件，拒绝交付".into());
                    }
                    let path = entry.path();
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if !matches!(
                        ext.as_str(),
                        "png" | "jpg" | "jpeg" | "webp" | "mp4" | "mov" | "webm"
                    ) {
                        continue;
                    }
                    if files.len() >= 10 {
                        return Err("即梦下载文件数超出限制".into());
                    }
                    files.push(json!({"path":path,"mime":match ext.as_str(){"png"=>"image/png","jpg"|"jpeg"=>"image/jpeg","webp"=>"image/webp","mp4"=>"video/mp4","mov"=>"video/quicktime",_=>"video/webm"}}));
                }
                if files.is_empty() {
                    return Err("即梦报告成功但没有下载实际媒体文件".into());
                }
                Ok(json!({"status":"completed","files":files}))
            }
            _ => Ok(json!({"status":"generating"})),
        }
    }
    fn job_directory(&self, operation: &str) -> Result<PathBuf, String> {
        let directory = self
            .launch
            .private_root
            .join("jobs")
            .join(lingxi_protocol::canon::sha256_hex(operation.as_bytes()));
        std::fs::create_dir_all(&directory).map_err(|_| "即梦任务目录不可写")?;
        if directory.canonicalize().ok().as_ref() != Some(&directory) {
            return Err("即梦任务目录被替换".into());
        }
        Ok(directory)
    }
    async fn helper(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        op: &str,
        args: Value,
    ) -> Result<Value, String> {
        let request = ToolRequest::from_effective_arguments(
            "tool:plugin:media:internal-jimeng-sdk",
            args,
            &SchemaBudget {
                max_arguments_bytes: 10 * 1024 * 1024,
                ..Default::default()
            },
        )
        .ok_or("即梦纯SDK参数超出限制")?;
        let executor = crate::workerrpc::WorkerToolExecutor::new(
            self.parser.clone(),
            crate::workerrpc::WorkerToolSpec {
                plugin_id: "media".into(),
                op: op.into(),
                local_name: "internal-jimeng-sdk".into(),
                description: "即梦纯SDK转换".into(),
                input_schema: json!({"type":"object"}),
                path_args: vec![],
                argv: self.launch.helper_argv.clone(),
                env: BTreeMap::new(),
                cwd: self.launch.private_root.clone(),
                model: Arc::new(crate::workerrpc::UnconfiguredWorkerModel),
                allowed_model_purposes: vec![],
                claimed_file_contract: None,
            },
            self.access.clone(),
        );
        match executor.execute(ctx, call, &request).await.outcome {
            ToolOutcome::Success { result } => {
                let text = result
                    .content
                    .iter()
                    .find_map(|block| match block {
                        lingxi_protocol::ContentBlock::Text { text } => text
                            .strip_prefix("[worker structured result: ")
                            .and_then(|text| text.strip_suffix(']')),
                        _ => None,
                    })
                    .ok_or("即梦纯SDK没有返回结构化结果")?;
                serde_json::from_str(text).map_err(|_| "即梦纯SDK结果损坏".into())
            }
            _ => Err("即梦纯SDK转换失败".into()),
        }
    }
    #[allow(clippy::too_many_arguments)]
    async fn command(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        args: Vec<String>,
        credentials: bool,
        generation: u64,
        cwd: &Path,
        cancel: &Arc<AtomicBool>,
        timeout_ms: u64,
    ) -> Result<String, String> {
        self.grant(generation)?;
        if cancel.load(Ordering::Acquire) {
            return Err("即梦操作已取消".into());
        }
        let mut argv = vec![self.launch.executable.to_string_lossy().into_owned()];
        argv.extend(args);
        let mut env = BTreeMap::new();
        env.insert("LANG".into(), "en_US.UTF-8".into());
        // 即使只探测 --help，CLI 也只能看到独立空目录，不继承操作系统用户 HOME。
        let home = if credentials {
            self.launch.external_login_home.clone()
        } else {
            self.launch.private_root.join("probe-home")
        };
        std::fs::create_dir_all(&home).map_err(|_| "即梦私有环境目录不可写")?;
        if home.canonicalize().ok().as_ref() != Some(&home) {
            return Err("即梦私有环境目录被替换".into());
        }
        env.insert("HOME".into(), home.to_string_lossy().into_owned());
        env.insert("USERPROFILE".into(), home.to_string_lossy().into_owned());
        for (key, subdirectory) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_DATA_HOME", "data"),
            ("APPDATA", "config"),
            ("LOCALAPPDATA", "data"),
        ] {
            env.insert(
                key.into(),
                home.join(subdirectory).to_string_lossy().into_owned(),
            );
        }
        let process = self
            .supervisor
            .spawn(SpawnSpec {
                argv,
                cwd: cwd.to_path_buf(),
                env,
                owner: ProcessOwner {
                    principal_kind: ctx.principal.storage_kind().into(),
                    principal_subject: ctx.principal.storage_subject(),
                    session_id: ctx.session_id.to_string(),
                    run_id: ctx.run_id.to_string(),
                    tool_call_id: call.clone(),
                },
                kind: ProcessKind::OneShot,
                cols: 80,
                rows: 24,
            })
            .await
            .map_err(|_| "即梦CLI进程无法启动")?;
        let mut guard = crate::exectools::ProcessOwnershipGuard::new(
            self.supervisor.clone(),
            process.id.clone(),
        );
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
        let phase = loop {
            tokio::select! {biased;phase=self.supervisor.wait_terminal(&process.id)=>break phase,_=tokio::time::sleep(Duration::from_millis(25))=>{if cancel.load(Ordering::Acquire)||self.grant(generation).is_err()||tokio::time::Instant::now()>=deadline{let receipt=self.supervisor.terminate(&process.id,TerminationReason::CallerDropped).await;guard.disarm();return Err(if matches!(receipt.outcome,crate::procsupervisor::TerminationOutcome::CleanupTimedOut){"即梦停止清理未确认"}else{"即梦本地进程已停止；远端结果未确认"}.into());}}}
        };
        guard.disarm();
        if !matches!(
            phase,
            Some(RecordPhase::Exited {
                fact: ExitFact::Code(0)
            })
        ) {
            return Err("即梦CLI执行失败，未回显原始登录信息或输出".into());
        }
        let record = self
            .supervisor
            .record(&process.id)
            .ok_or("即梦进程清理回执缺失")?;
        if !record.child_reaped
            || !record.drained_stdio
            || !record.reclaimed
            || record.pumps_alive != 0
        {
            return Err("即梦进程或输出回收尚未确认".into());
        }
        let snapshot = self
            .supervisor
            .output_snapshot(&process.id)
            .ok_or("即梦CLI输出未确认")?;
        if snapshot.total_bytes > 2 * 1024 * 1024
            || snapshot.total_bytes as usize > snapshot.window.len()
        {
            return Err("即梦CLI输出超出有界解析范围".into());
        }
        String::from_utf8(snapshot.window).map_err(|_| "即梦CLI输出不是有效文本".into())
    }
    pub async fn stop(&self) -> Result<(), String> {
        self.revoked.store(true, Ordering::Release);
        for receipt in self.supervisor.shutdown_all().await {
            if matches!(
                receipt.outcome,
                crate::procsupervisor::TerminationOutcome::CleanupTimedOut
            ) {
                return Err("即梦执行器清理尚未确认".into());
            }
        }
        Ok(())
    }
    pub async fn settle_cancelled_run(&self, run_id: &str) -> Result<(), String> {
        for handle in self.supervisor.live_handles() {
            if self
                .supervisor
                .record(&handle)
                .is_some_and(|r| r.owner.run_id == run_id)
            {
                let receipt = self
                    .supervisor
                    .terminate(&handle, TerminationReason::CallerDropped)
                    .await;
                if matches!(
                    receipt.outcome,
                    crate::procsupervisor::TerminationOutcome::CleanupTimedOut
                ) {
                    return Err("即梦执行任务清理尚未确认".into());
                }
            }
        }
        Ok(())
    }
}
