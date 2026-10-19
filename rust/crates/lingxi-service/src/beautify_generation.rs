//! 明确用户动作驱动的封面作业。所有外部请求沿正式模型/媒体入口，不从模型参数铸造批准。
use crate::{
    appevents::AppEventLog,
    auth::{AuthService, CredentialKind, PrincipalKind},
    cancel::{CancelScope, ScopeKind},
    events::EventService,
    inject::{RequestIdGen, ServiceClock},
    management::ManagementState,
    media::{MediaService, MediaSpec},
    office_tools::{
        BeautifyFuture, BeautifyGenerationPort, BeautifyGenerationRequest, OfficeApi, OfficeError,
    },
    resourceio::{ArtifactWritePreflight, OpContext},
    runs::RunSupervisor,
    session_admin::SessionAdminService,
    task_supervisor::ChildHandle,
    Principal, ServiceState,
};
use lingxi_adapters::storage::{
    beautify::{self as store, CoverClaim, CoverJob, CoverPhase},
    media as media_store, session_tree, RunDatabase,
};
use lingxi_kernel::{
    ports::{StorageError, StoragePort, ToolOutcome},
    RunContext,
};
use lingxi_protocol::ToolCallId;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
    time::Duration,
};

pub struct BeautifyService {
    db: Arc<RunDatabase>,
    office: Weak<OfficeApi>,
    runs: Weak<RunSupervisor>,
    media: Weak<MediaService>,
    admin: Arc<SessionAdminService>,
    auth: Arc<AuthService>,
    management: Arc<ManagementState>,
    clock: Arc<dyn ServiceClock>,
    ids: Arc<dyn RequestIdGen>,
    events: Arc<EventService>,
    app_events: Arc<AppEventLog>,
    stopping: AtomicBool,
    self_ref: OnceLock<Weak<Self>>,
    drivers: Mutex<BTreeMap<String, ChildHandle<Result<(), String>>>>,
}
impl BeautifyService {
    pub fn from_state(state: &ServiceState, office: &Arc<OfficeApi>) -> Arc<Self> {
        let this = Arc::new(Self {
            db: state.storage().clone(),
            office: Arc::downgrade(office),
            runs: Arc::downgrade(state.runs()),
            media: Arc::downgrade(state.media()),
            admin: state.session_admin().clone(),
            auth: state.auth.clone(),
            management: state.management.clone(),
            clock: state.clock().clone(),
            ids: state.request_ids().clone(),
            events: state.events().clone(),
            app_events: state.app_events().clone(),
            stopping: AtomicBool::new(false),
            self_ref: OnceLock::new(),
            drivers: Mutex::new(BTreeMap::new()),
        });
        this.self_ref
            .set(Arc::downgrade(&this))
            .expect("首次登记自身弱引用");
        this
    }
    fn office(&self) -> Result<Arc<OfficeApi>, String> {
        self.office.upgrade().ok_or("办公服务已退出".into())
    }
    fn runs(&self) -> Result<Arc<RunSupervisor>, String> {
        self.runs.upgrade().ok_or("运行服务已退出".into())
    }
    fn source(&self, j: &CoverJob) -> Result<Principal, String> {
        let mut p: Principal =
            serde_json::from_value(j.principal.clone()).map_err(|_| "封面原始认证事实损坏")?;
        p.web_session_id = j.web_session_id.clone();
        self.validate_principal(&p)?;
        let actor = crate::sessions::kernel_principal_of(&p);
        if p.principal_id != j.principal_id
            || actor.storage_kind() != j.principal_kind
            || actor.storage_subject() != j.principal_subject
            || p.user_id.as_deref() != Some(&j.owner_subject)
        {
            return Err("封面原始身份发生变化".into());
        }
        Ok(p)
    }
    fn validate_principal(&self, p: &Principal) -> Result<(), String> {
        if (!p.is_local_owner()
            && p.kind != PrincipalKind::Device
            && p.credential_kind != CredentialKind::WebSession)
            || !p.has_scope("chat")
            || !p.has_scope("resources.write")
        {
            return Err("封面生成需要真实用户的聊天和资源写权限".into());
        }
        self.auth
            .validate_principal(p)
            .map_err(|_| "封面发起凭证已撤销")?;
        if p.credential_kind == CredentialKind::WebSession
            && !self
                .management
                .web_session_current(p, self.clock.now_unix_ms(), true)
                .map_err(|_| "封面网页会话无法复核")?
        {
            return Err("封面网页会话已撤销".into());
        }
        Ok(())
    }
    async fn current(
        &self,
        j: &CoverJob,
        ctx: &RunContext,
        cancel: &CancelScope,
    ) -> Result<(), String> {
        if self.stopping.load(Ordering::Acquire) || cancel.is_cancelled() {
            return Err("封面任务已取消".into());
        }
        let p = self.source(j)?;
        if crate::sessions::kernel_principal_of(&p) != ctx.principal {
            return Err("封面运行身份不匹配".into());
        }
        let session = session_tree::get_session_row(&self.db, &j.session_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("封面会话已删除")?;
        if session.lifecycle != "active"
            || session.owner_user_id != j.owner_subject
            || session.agent_id != j.agent_id
            || session.permission_mode.as_deref() == Some("read_only")
        {
            return Err("封面会话已关闭、只读或改属".into());
        }
        let office = self.office()?;
        office.enabled(&j.agent_id, "beautify")?;
        office.enabled(&j.agent_id, "media")?;
        let actual = self
            .db
            .load_run(&ctx.run_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("封面运行不存在")?;
        // 封面 Run 只由 admit_cover_job 开启单次 attempt（generation 1），没有重试
        // 或跨进程复活路径，世代不变量由结构保证；这里核对真实持久状态仍在运行。
        if actual.status != lingxi_protocol::RunStatus::Running {
            return Err("封面运行已过期".into());
        }
        Ok(())
    }
    fn emit(&self, j: &CoverJob) {
        self.app_events
            .publish("activity_update", json!({"activity":public_activity(j)}));
    }
    async fn update(
        &self,
        j: &CoverJob,
        phase: CoverPhase,
        state: Value,
    ) -> Result<CoverJob, String> {
        let j = store::transition(&self.db, j, phase, state, self.clock.now_unix_ms())
            .await
            .map_err(|e| e.to_string())?;
        self.emit(&j);
        Ok(j)
    }
    pub async fn recover(self: &Arc<Self>) -> Result<(), String> {
        for j in store::list_open(&self.db)
            .await
            .map_err(|e| e.to_string())?
        {
            let mut state = j.state.clone();
            state["error"] = json!("服务重启，原动作的付费或写入窗口未确认；不会自动重新提交");
            self.update(&j, CoverPhase::Unknown, state).await?;
        }
        Ok(())
    }
    pub fn begin_stop(&self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(runs) = self.runs.upgrade() {
            if let Ok(drivers) = self.drivers.lock() {
                for id in drivers.keys() {
                    let _ = runs.cancel_run(id, "封面服务退出");
                }
            }
        }
    }
    pub async fn shutdown(&self, budget: Duration) -> Result<Vec<String>, String> {
        self.stopping.store(true, Ordering::Release);
        // 显式降级（标注，与 media.stop 同口径）：存储队列已被外部先行关闭时
        // 持久作业表不可读——在役 run 取消由 shutdown Phase 2.5a 统一承载，
        // 存储状态由 managed close / Phase 3 如实上报；记日志后继续内存收尾。
        let jobs = match store::list_open(&self.db).await {
            Ok(jobs) => jobs,
            Err(StorageError::QueueClosed) => {
                tracing::error!(
                    "beautify shutdown: durable job table unreadable (queue already closed); \
                     continuing with in-memory teardown only"
                );
                Vec::new()
            }
            Err(error) => return Err(error.to_string()),
        };
        for j in jobs {
            if let Some(id) = j.run_id {
                self.runs()?.cancel_run(&id, "封面服务退出");
            }
        }
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            self.reap().await?;
            let pending = self
                .drivers
                .lock()
                .map_err(|_| "封面任务锁损坏")?
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            if pending.is_empty() || tokio::time::Instant::now() >= deadline {
                return Ok(pending);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    async fn reap(&self) -> Result<(), String> {
        let done = {
            let mut d = self.drivers.lock().map_err(|_| "封面任务锁损坏")?;
            let ids = d
                .iter()
                .filter(|(_, h)| h.is_finished())
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| d.remove(&id).map(|h| (id, h)))
                .collect::<Vec<_>>()
        };
        for (id, h) in done {
            match h.wait().await {
                Ok(Ok(())) => {}
                other => return Err(format!("封面任务{id}收尾未确认：{other:?}")),
            }
        }
        Ok(())
    }
    async fn submit(&self, request: BeautifyGenerationRequest) -> Result<Value, OfficeError> {
        self.validate_principal(&request.principal)
            .map_err(forbidden)?;
        if self.stopping.load(Ordering::Acquire) {
            return Err(unavailable("封面服务正在退出"));
        }
        self.reap().await.map_err(unavailable)?;
        if request.user_guidance.len() > 16000
            || !["light", "dark"].contains(&request.theme_tone.as_str())
        {
            return Err(OfficeError::bad("封面附加说明或主题无效"));
        }
        let office = self.office().map_err(unavailable)?;
        office
            .enabled(&request.agent_id, "beautify")
            .map_err(forbidden)?;
        office
            .enabled(&request.agent_id, "media")
            .map_err(forbidden)?;
        let media = self
            .media
            .upgrade()
            .ok_or_else(|| unavailable("媒体服务已退出"))?;
        let probe = MediaSpec::parse(
            "image",
            &json!({"prompt":"cover availability","count":1,"ratio":"3:2","resolution":"2k"}),
        )
        .map_err(OfficeError::bad)?;
        let route = media.review_route("image", &probe).map_err(unavailable)?;
        let req = request
            .request_id
            .unwrap_or_else(|| self.ids.next_request_id());
        if req.is_empty() || req.len() > 1024 || req.chars().any(char::is_control) {
            return Err(OfficeError::bad("invalid requestId"));
        }
        let id = format!(
            "beautify-{}",
            lingxi_protocol::canon::sha256_hex(
                json!([request.principal.principal_id, req])
                    .to_string()
                    .as_bytes()
            )
        );
        let input = json!({"target":request.target,"themeTone":request.theme_tone,"userGuidance":request.user_guidance,"mediaRoute":route});
        let digest = lingxi_protocol::canon::sha256_hex(input.to_string().as_bytes());
        // 已有动作先读原收据，目标后来变化或被删除也不能制造第二个付费任务。
        if let Some(old) = store::get(&self.db, &id).await.map_err(OfficeError::bad)? {
            if old.input_digest != digest || old.agent_id != request.agent_id {
                return Err(conflict("同requestId已绑定不同封面参数"));
            }
            self.source(&old).map_err(forbidden)?;
            return Ok(public_response(&old));
        }
        let actor = crate::sessions::kernel_principal_of(&request.principal);
        let original = OpContext {
            principal_kind: actor.storage_kind(),
            principal_subject: actor.storage_subject(),
            session_id: String::new(),
            source: "api".into(),
            reason: Some("desk.beautify.cover".into()),
            session_path: None,
            emit: true,
            capture_origin: None,
            engine_plane: false,
        };
        let snapshot = office
            .io
            .read_artifact_snapshot(&input["target"], &original)
            .await
            .map_err(OfficeError::resource)?;
        if snapshot
            .file_path
            .extension()
            .and_then(|s| s.to_str())
            .is_none_or(|s| !s.eq_ignore_ascii_case("md"))
            || snapshot.bytes.len() > 8 * 1024 * 1024
        {
            return Err(OfficeError::bad("封面目标必须是8MiB以内的Markdown"));
        }
        let frozen = office
            .io
            .prepare_artifact_write(&input["target"], true, &original)
            .await
            .map_err(OfficeError::resource)?;
        frozen
            .require_snapshot(&snapshot.meta["artifactStamp"])
            .map_err(OfficeError::resource)?;
        let now = self.clock.now_unix_ms();
        let job = CoverJob {
            id: id.clone(),
            request_id: req,
            principal_id: request.principal.principal_id.clone(),
            principal_kind: actor.storage_kind().into(),
            principal_subject: actor.storage_subject(),
            owner_subject: request
                .principal
                .user_id
                .clone()
                .ok_or_else(|| forbidden("真实用户不存在"))?,
            agent_id: request.agent_id,
            session_id: format!("cover-session-{}", &id[9..]),
            run_id: None,
            input_digest: digest,
            input,
            principal: serde_json::to_value(&request.principal).map_err(OfficeError::bad)?,
            web_session_id: request.principal.web_session_id.clone(),
            frozen_write: frozen.persistent_facts().map_err(OfficeError::resource)?,
            admitted_write: None,
            phase: CoverPhase::Queued,
            state: json!({}),
            revision: 1,
            created_at: now,
            updated_at: now,
        };
        let job = match store::claim(&self.db, job)
            .await
            .map_err(OfficeError::bad)?
        {
            CoverClaim::Existing(j) => return Ok(public_response(&j)),
            CoverClaim::New(j) => j,
        };
        let job = self
            .update(&job, CoverPhase::Starting, json!({}))
            .await
            .map_err(OfficeError::bad)?;
        if let Err(e) = self.launch(job.clone()).await {
            let mut state = job.state.clone();
            state["error"] = json!(e);
            let actual = store::get(&self.db, &job.id)
                .await
                .map_err(OfficeError::bad)?
                .ok_or_else(|| OfficeError::bad("封面作业不存在"))?;
            if !actual.phase.terminal() {
                self.update(&actual, CoverPhase::Failed, state)
                    .await
                    .map_err(OfficeError::bad)?;
            }
            return Err(unavailable(e));
        }
        Ok(public_response(
            &store::get(&self.db, &id)
                .await
                .map_err(OfficeError::bad)?
                .ok_or_else(|| OfficeError::bad("封面作业不存在"))?,
        ))
    }
    async fn launch(&self, job: CoverJob) -> Result<(), String> {
        if self.drivers.lock().map_err(|_| "封面任务锁损坏")?.len() >= 32 {
            return Err("封面后台任务已达上限".into());
        }
        self.admin
            .create_session(
                &job.session_id,
                &job.agent_id,
                &job.owner_subject,
                "Markdown cover",
                Some("ask"),
                &[],
                false,
                self.clock.now_unix_ms(),
            )
            .await
            .map_err(|e| format!("封面会话创建失败：{e:?}"))?;
        let principal = self.source(&job)?;
        let runs = self.runs()?;
        let permit = runs
            .admit_cover_job(
                &self.db,
                &self.events,
                &job,
                crate::sessions::kernel_principal_of(&principal),
                self.clock.now_unix_ms(),
            )
            .await?;
        let cell = Arc::new(Mutex::new(Some(permit)));
        let handoff = cell.clone();
        let this = self
            .self_ref
            .get()
            .and_then(Weak::upgrade)
            .ok_or("封面服务已退出")?;
        let driver_runs = runs.clone();
        let run_key = handoff
            .lock()
            .map_err(|_| "封面许可锁损坏")?
            .as_ref()
            .ok_or("封面许可缺失")?
            .context
            .run_id
            .to_string();
        let handle=runs.task_supervisor().spawn_detached(format!("cover:{}",job.id),async move{
   let permit=handoff.lock().map_err(|_|"封面许可锁损坏")?.take().ok_or("封面许可已消费")?;let ctx=permit.context.clone();let root=permit.cancel_scope.clone();
   let this_work=this.clone();let drive_job=job.clone();let drive_ctx=ctx.clone();let work_scope=root.child("cover_work".to_string(),ScopeKind::ChildRun);let work_cancel=root.clone();
   let child=driver_runs.task_supervisor().spawn_linked(ctx.run_id.as_str(),&work_scope,"cover fixed workflow".into(),async move{this_work.drive(drive_job,drive_ctx,work_cancel).await}).map_err(|e|e.to_string());
   let result=match child{Ok(child)=>{let mut wait=Box::pin(child.wait());loop{tokio::select!{result=&mut wait=>break result.map_err(|e|format!("封面执行退出：{}",e.name())).and_then(|r|r),_=tokio::time::sleep(Duration::from_millis(50))=>{if let Err(e)=this.current(&job,&ctx,&root).await{root.cancel(&e);}}}}},Err(e)=>Err(e)};
   if let Err(ref e)=result {if let Some(actual)=store::get(&this.db,&job.id).await.map_err(|e|e.to_string())?{if !actual.phase.terminal(){let mut s=actual.state.clone();s["error"]=json!(e);let phase=if root.is_cancelled(){CoverPhase::Cancelled}else if matches!(actual.phase,CoverPhase::Prompting|CoverPhase::MediaPreparing|CoverPhase::Applying){CoverPhase::Unknown}else{CoverPhase::Failed};this.update(&actual,phase,s).await?;}}}
   driver_runs.finish_cover_job(&this.db,&this.events,permit,result,this.clock.now_unix_ms()).await
  });
        match handle {
            Ok(h) => {
                self.drivers
                    .lock()
                    .map_err(|_| "封面任务锁损坏")?
                    .insert(run_key, h);
                Ok(())
            }
            Err(e) => {
                let permit = cell.lock().map_err(|_| "封面许可锁损坏")?.take();
                if let Some(p) = permit {
                    runs.finish_cover_job(
                        &self.db,
                        &self.events,
                        p,
                        Err("封面驱动未受理".into()),
                        self.clock.now_unix_ms(),
                    )
                    .await?;
                }
                Err(e.to_string())
            }
        }
    }
    async fn drive(
        &self,
        job: CoverJob,
        ctx: RunContext,
        cancel: CancelScope,
    ) -> Result<(), String> {
        self.current(&job, &ctx, &cancel).await?;
        let office = self.office()?;
        let context = OpContext {
            principal_kind: ctx.principal.storage_kind(),
            principal_subject: ctx.principal.storage_subject(),
            session_id: ctx.session_id.to_string(),
            source: "agent_tool".into(),
            reason: Some("用户明确发起封面生成".into()),
            session_path: Some(format!("native:{}", ctx.session_id)),
            emit: true,
            capture_origin: None,
            engine_plane: false,
        };
        let target = office
            .io
            .prepare_artifact_write(&job.input["target"], true, &context)
            .await
            .map_err(|e| e.to_string())?;
        target
            .require_same_target(
                &ArtifactWritePreflight::from_trusted_persistent_facts(&job.frozen_write)
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        let snapshot = office
            .io
            .read_artifact_snapshot(&job.input["target"], &context)
            .await
            .map_err(|e| e.to_string())?;
        target
            .require_snapshot(&snapshot.meta["artifactStamp"])
            .map_err(|e| e.to_string())?;
        let job = store::bind_run(
            &self.db,
            &job,
            ctx.run_id.as_str(),
            target.persistent_facts().map_err(|e| e.to_string())?,
            self.clock.now_unix_ms(),
        )
        .await
        .map_err(|e| e.to_string())?;
        let job = self
            .update(&job, CoverPhase::Prompting, job.state.clone())
            .await?;
        let doc = String::from_utf8(snapshot.bytes).map_err(|_| "Markdown不是UTF-8")?;
        let excerpt = doc.chars().take(24000).collect::<String>();
        let prompt = self
            .runs()?
            .cover_prompt(crate::runs::CoverPromptCall {
                db: &self.db,
                events: &self.events,
                ctx: &ctx,
                root: &cancel,
                agent: &job.agent_id,
                submission: json!({"themeTone":job.input["themeTone"],"userGuidance":job.input["userGuidance"],"documentExcerpt":excerpt,"excerptTruncated":doc.chars().count()>24000}).to_string(),
                now: self.clock.now_unix_ms(),
            })
            .await?;
        self.current(&job, &ctx, &cancel).await?;
        let mut state = job.state.clone();
        state["prompt"] = json!(prompt);
        let job = self.update(&job, CoverPhase::PromptReady, state).await?;
        let route = &job.input["mediaRoute"];
        let mut args =
            json!({"prompt":job.state["prompt"],"count":1,"ratio":"3:2","resolution":"2k"});
        for key in ["provider", "model"] {
            if let Some(v) = route[key].as_str() {
                args[key] = json!(v);
            }
        }
        let mut state = job.state.clone();
        state["mediaArgs"] = args;
        state["mediaRoute"] = route.clone();
        let job = self.update(&job, CoverPhase::MediaPreparing, state).await?;
        let outcome = self
            .runs()?
            .execute_cover_action(crate::runs::CoverActionCall {
                db: &self.db,
                job_id: &job.id,
                action: crate::runs::CoverAction::GenerateImage,
                ctx: &ctx,
                cancel: &cancel,
                guard: self,
                auth: &self.auth,
                management: &self.management,
                clock: self.clock.as_ref(),
            })
            .await?;
        if !matches!(outcome, ToolOutcome::Success { .. }) {
            return Err(format!("封面媒体未受理：{outcome:?}"));
        }
        let call = ToolCallId::new(format!("{}-cover-image", ctx.run_id));
        let operation = format!(
            "media-{}",
            lingxi_protocol::canon::sha256_hex(
                json!([ctx.run_id.as_str(), call.as_str(), "image"])
                    .to_string()
                    .as_bytes()
            )
        );
        let mut state = job.state.clone();
        state["mediaOperationId"] = json!(operation);
        let job = self.update(&job, CoverPhase::MediaPending, state).await?;
        let media = loop {
            self.current(&job, &ctx, &cancel).await?;
            let media = media_store::get(&self.db, &operation)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("封面媒体真实回执缺失")?;
            if media.phase == media_store::MediaPhase::Completed {
                break media;
            }
            if media.phase.terminal() {
                return Err(format!("封面图片未交付：{}", media.phase.as_str()));
            }
            tokio::select! {_=cancel.cancelled()=>return Err("封面已取消".into()),_=tokio::time::sleep(Duration::from_millis(50))=>{}}
        };
        let file = media.state["files"]
            .as_array()
            .filter(|files| files.len() == 1)
            .and_then(|files| files.first())
            .ok_or("封面媒体必须真实交付一张图片")?;
        let file_id = file["fileId"].as_str().ok_or("封面图片文件身份缺失")?;
        let mut state = job.state.clone();
        state["applyArgs"] = json!({"targetResource":job.input["target"],"imageResource":{"kind":"session-file","fileId":file_id,"sessionId":job.session_id}});
        let job = self.update(&job, CoverPhase::Applying, state).await?;
        let outcome = self
            .runs()?
            .execute_cover_action(crate::runs::CoverActionCall {
                db: &self.db,
                job_id: &job.id,
                action: crate::runs::CoverAction::Apply,
                ctx: &ctx,
                cancel: &cancel,
                guard: self,
                auth: &self.auth,
                management: &self.management,
                clock: self.clock.as_ref(),
            })
            .await?;
        let ToolOutcome::Success { result } = outcome else {
            return Err(format!("封面应用尚未确认：{outcome:?}"));
        };
        let mut state = job.state.clone();
        state["result"] = serde_json::to_value(result.details).map_err(|e| e.to_string())?;
        self.update(&job, CoverPhase::Completed, state).await?;
        Ok(())
    }
}
impl crate::runs::CoverActionGuard for BeautifyService {
    fn current_cover<'a>(
        &'a self,
        job: &'a CoverJob,
        ctx: &'a RunContext,
        cancel: &'a CancelScope,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(self.current(job, ctx, cancel))
    }
}
impl BeautifyGenerationPort for BeautifyService {
    fn status<'a>(&'a self, agent: &'a str) -> BeautifyFuture<'a, Value> {
        Box::pin(async move {
            let office = self.office().map_err(unavailable)?;
            office.enabled(agent, "beautify").map_err(forbidden)?;
            office.enabled(agent, "media").map_err(forbidden)?;
            let media = self
                .media
                .upgrade()
                .ok_or_else(|| unavailable("媒体服务已退出"))?;
            let spec = MediaSpec::parse(
                "image",
                &json!({"prompt":"cover availability","count":1,"ratio":"3:2","resolution":"2k"}),
            )
            .map_err(OfficeError::bad)?;
            match media.review_route("image", &spec) {
                Ok(_) => Ok(json!({"enabled":true,"available":true,"disabledReason":null})),
                Err(e) => Ok(
                    json!({"enabled":false,"available":true,"disabledReason":"image-generation-unavailable","message":e}),
                ),
            }
        })
    }
    fn generate<'a>(&'a self, request: BeautifyGenerationRequest) -> BeautifyFuture<'a, Value> {
        Box::pin(self.submit(request))
    }
}
fn forbidden(message: impl ToString) -> OfficeError {
    OfficeError {
        status: 403,
        code: "BEAUTIFY_FORBIDDEN",
        message: message.to_string(),
        effect_unknown: false,
    }
}
fn unavailable(message: impl ToString) -> OfficeError {
    OfficeError {
        status: 503,
        code: "BEAUTIFY_UNAVAILABLE",
        message: message.to_string(),
        effect_unknown: false,
    }
}
fn conflict(message: impl ToString) -> OfficeError {
    OfficeError {
        status: 409,
        code: "BEAUTIFY_CONFLICT",
        message: message.to_string(),
        effect_unknown: false,
    }
}
pub fn public_activity(j: &CoverJob) -> Value {
    json!({"id":j.id,"type":"beautify","label":"Markdown cover","status":match j.phase{CoverPhase::Completed=>"completed",CoverPhase::Failed=>"failed",CoverPhase::Cancelled=>"cancelled",CoverPhase::Unknown=>"unknown",_=>"running"},"agentId":j.agent_id,"sessionId":j.session_id,"sessionPath":format!("native:{}",j.session_id),"runId":j.run_id,"startedAt":j.created_at,"finishedAt":j.phase.terminal().then_some(j.updated_at),"summary":j.state["error"],"result":j.state["result"]})
}
fn public_response(j: &CoverJob) -> Value {
    json!({"ok":true,"activity":public_activity(j),"requestId":j.request_id,"jobId":j.id,"phase":j.phase.as_str(),"sessionId":j.session_id,"runId":j.run_id,"effectUnknown":j.phase==CoverPhase::Unknown})
}
