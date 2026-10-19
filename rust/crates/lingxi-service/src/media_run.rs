//! 固定媒体执行沿真实 Run 状态机收尾；不进入模型回合，也不发放通用工具许可。
use super::*;
use lingxi_adapters::storage::{media as media_store, session_tree, RunDatabase};
use lingxi_kernel::session_admission::SessionPermit;
use lingxi_protocol::ToolCallId;

pub struct MediaExecutionPermit {
    pub context: lingxi_kernel::RunContext,
    pub cancel_scope: CancelScope,
    supervisor: Arc<RunSupervisor>,
    entry: Arc<RunCancelEntry>,
    _driver: crate::task_supervisor::DriverClaim,
    finalized: bool,
}
impl Drop for MediaExecutionPermit {
    fn drop(&mut self) {
        if !self.finalized {
            self.cancel_scope.cancel("媒体驱动未完成统一收尾");
            self.entry.advance_phase(CancelPhase::Abandoned {
                reason: "媒体驱动退出但未确认最终写入，保留真实活动状态供重启核对".into(),
            });
        }
        self.supervisor
            .cancel
            .deregister(self.context.run_id.as_str());
    }
}
impl RunSupervisor {
    /// 只有同一数据根、正在执行的正式工具回执可以孵化固定媒体执行。
    #[allow(clippy::too_many_arguments)]
    pub async fn admit_media_job(
        self: &Arc<Self>,
        db: &Arc<RunDatabase>,
        events: &Arc<EventService>,
        parent: &lingxi_kernel::RunContext,
        parent_call: &ToolCallId,
        operation_id: &str,
        args_digest: &str,
        now: u64,
    ) -> Result<MediaExecutionPermit, String> {
        let job = media_store::get(db, operation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体作业不存在")?;
        if job.session_id != parent.session_id.as_str()
            || job.source_run_id != parent.run_id.as_str()
            || job.tool_call_id != parent_call.as_str()
            || job.request_digest != args_digest
            || job.phase != media_store::MediaPhase::Submitting
            || job.execution_run_id.is_some()
        {
            return Err("媒体来源与正式作业不一致".into());
        }
        let session = session_tree::get_session_row(db, &job.session_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体会话不存在")?;
        let gate = db.session_admission();
        if self
            .storage_admission
            .as_ref()
            .is_none_or(|bound| !Arc::ptr_eq(bound, gate))
        {
            return Err("媒体执行的数据根与运行宿主不一致".into());
        }
        let _admission = self
            .tasks
            .parent_run_permit(
                parent.run_id.as_str(),
                &job.session_id,
                &session.agent_id,
                &parent.principal,
                gate,
            )
            .map_err(|e| e.to_string())?
            .ok_or("媒体来源缺少真实受理许可")?;
        let parent_scope = self
            .run_scope(parent.run_id.as_str())
            .ok_or("媒体来源任务已退出")?;
        if parent_scope.is_cancelled() {
            return Err("媒体来源任务正在取消".into());
        }
        let actual = db.query_one_text(
            "SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id JOIN run_attempts a ON a.run_id=r.run_id JOIN invocation_journal j ON j.run_id=r.run_id AND j.journal_id=?7 WHERE r.run_id=?1 AND r.session_id=?2 AND r.owner_kind=?3 AND r.owner_subject=?4 AND s.owner_user_id=?10 AND s.lifecycle='active' AND r.status='running' AND r.generation=?5 AND a.attempt=?6 AND a.generation=r.generation AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1) AND j.phase='started' AND j.attempt=a.attempt AND j.generation=r.generation AND j.session_id=r.session_id AND j.owner_kind=r.owner_kind AND j.owner_subject=r.owner_subject AND j.args_digest=?8 AND j.target=?9",
            vec![parent.run_id.to_string(),job.session_id.clone(),parent.principal.storage_kind().into(),parent.principal.storage_subject(),parent.generation.to_string(),parent.attempt.to_string(),parent_call.to_string(),args_digest.into(),format!("tool:plugin:media:generate-{}",job.kind),job.owner_subject.clone()],
        ).await.map_err(|e| e.to_string())?;
        if actual.is_none()
            || session.lifecycle != "active"
            || session.owner_user_id != job.owner_subject
            || !parent
                .principal
                .can_access_session(&session.owner_user_id, &job.session_id)
        {
            return Err("媒体来源任务、权限或正式调用回执已改变".into());
        }
        self.start_media_run(
            db,
            events,
            &job,
            parent.principal.clone(),
            Some(&parent_scope),
            now,
        )
        .await
    }

    /// 已确认远端任务仅恢复查询；不会沿此入口再次提交生成请求。
    pub async fn admit_media_resume(
        self: &Arc<Self>,
        db: &Arc<RunDatabase>,
        events: &Arc<EventService>,
        job: &media_store::MediaJob,
        now: u64,
    ) -> Result<MediaExecutionPermit, String> {
        let current = media_store::get(db, &job.operation_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体恢复作业不存在")?;
        if current.revision != job.revision
            || current.execution_run_id != job.execution_run_id
            || !matches!(
                current.phase,
                media_store::MediaPhase::Submitted | media_store::MediaPhase::Generating
            )
            || current.state["tracker"].as_str().is_none_or(str::is_empty)
        {
            return Err("媒体恢复事实已改变或没有远端任务回执".into());
        }
        let gate = db.session_admission();
        if self
            .storage_admission
            .as_ref()
            .is_none_or(|bound| !Arc::ptr_eq(bound, gate))
        {
            return Err("媒体恢复的数据根与运行宿主不一致".into());
        }
        // 只在核对与登记期间持有会话许可，不占用远端生成的整个等待时间。
        let _admission = gate
            .try_acquire(&[&job.session_id], usize::MAX)
            .map_err(|_| "媒体会话当前忙碌，恢复尚未受理")?;
        let session = session_tree::get_session_row(db, &job.session_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体恢复会话不存在")?;
        if session.lifecycle != "active" || session.owner_user_id != job.owner_subject {
            return Err("媒体恢复会话已撤权或关闭".into());
        }
        let source = db.query_one_text(
            "SELECT r.run_id FROM runs r JOIN sessions s ON s.session_id=r.session_id JOIN invocation_journal j ON j.run_id=r.run_id AND j.journal_id=?4 WHERE r.run_id=?1 AND r.session_id=?2 AND s.owner_user_id=?3 AND j.owner_subject=r.owner_subject AND j.owner_kind=r.owner_kind AND j.session_id=r.session_id AND j.args_digest=?5 AND j.target=?6 AND j.phase IN ('started','succeeded','unknown')",
            vec![job.source_run_id.clone(),job.session_id.clone(),job.owner_subject.clone(),job.tool_call_id.clone(),job.request_digest.clone(),format!("tool:plugin:media:generate-{}",job.kind)],
        ).await.map_err(|e| e.to_string())?;
        if source.is_none() {
            return Err("媒体恢复缺少真实来源调用".into());
        }
        let source_identity = db.query_one_text(
            "SELECT json_object('kind',r.owner_kind,'subject',r.owner_subject) FROM runs r JOIN sessions s ON s.session_id=r.session_id WHERE r.run_id=?1 AND r.session_id=?2 AND s.owner_user_id=?3",
            vec![job.source_run_id.clone(), job.session_id.clone(), job.owner_subject.clone()],
        ).await.map_err(|e| e.to_string())?.ok_or("媒体恢复来源身份不存在")?;
        let identity: serde_json::Value =
            serde_json::from_str(&source_identity).map_err(|_| "媒体恢复来源身份损坏")?;
        let principal = lingxi_kernel::recovery::principal_from_storage_facts(
            identity["kind"].as_str().ok_or("媒体来源身份类型不存在")?,
            identity["subject"]
                .as_str()
                .ok_or("媒体来源身份主体不存在")?,
        )
        .map_err(|_| "媒体恢复来源身份格式损坏")?;
        if !principal.can_access_session(&session.owner_user_id, &job.session_id) {
            return Err("媒体恢复来源已不能访问该会话".into());
        }
        self.start_media_run(db, events, job, principal, None, now)
            .await
    }

    async fn start_media_run(
        self: &Arc<Self>,
        db: &Arc<RunDatabase>,
        events: &Arc<EventService>,
        job: &media_store::MediaJob,
        principal: lingxi_kernel::Principal,
        parent_scope: Option<&CancelScope>,
        now: u64,
    ) -> Result<MediaExecutionPermit, String> {
        let run_id = RunId::new(RunDatabase::allocate_run_id(db, now).map_err(|e| e.to_string())?);
        let driver = self
            .tasks
            .claim_driver(run_id.as_str())
            .map_err(|e| e.to_string())?;
        let entry = match parent_scope {
            Some(scope) => self.cancel.register_linked(run_id.as_str(), scope),
            None => self.cancel.register(run_id.as_str()),
        };
        let ctx = lingxi_kernel::RunContext {
            principal,
            session_id: lingxi_protocol::SessionId::new(job.session_id.clone()),
            run_id: run_id.clone(),
            attempt: attempt_id(&run_id, 1),
            generation: 1,
        };
        let permit = MediaExecutionPermit {
            context: ctx,
            cancel_scope: entry.scope.clone(),
            supervisor: self.clone(),
            entry,
            _driver: driver,
            finalized: false,
        };
        RunStateMachine::transition(RunStatus::Queued, RunStatus::Running)
            .map_err(|e| e.reason.to_owned())?;
        let started = db
            .record_run_started(&permit.context, now)
            .await
            .map_err(|e| e.to_string())?;
        events.publish_committed(&started.events);
        if let Err(error) = db
            .record_run_lineage(
                &permit.context,
                RunLineage {
                    parent_run_id: Some(RunId::new(job.source_run_id.clone())),
                    origin: RunOrigin::Media,
                    source_message_id: Some(job.tool_call_id.clone()),
                    cause_id: Some(job.operation_id.clone()),
                },
                now,
            )
            .await
        {
            self.finish_media_job(db, events, permit, Err("媒体因果登记失败".into()), now)
                .await?;
            return Err(error.to_string());
        }
        Ok(permit)
    }

    pub async fn finish_media_job(
        &self,
        db: &Arc<RunDatabase>,
        events: &Arc<EventService>,
        mut permit: MediaExecutionPermit,
        result: Result<(), String>,
        now: u64,
    ) -> Result<(), String> {
        if !std::ptr::eq(self, permit.supervisor.as_ref()) {
            return Err("媒体收尾凭据不属于当前运行宿主".into());
        }
        let finish = match result {
            Ok(()) => RunFinish::CompletedWithoutFinal {
                cause: NoFinalCause::ProcessOnly,
            },
            Err(_) => RunFinish::Failed {
                cause: FailureCause::ProviderFailed {
                    code: "media_execution_failed".into(),
                    retryable: false,
                },
            },
        };
        self.adjudicated_finalize(
            db.as_ref(),
            events,
            &permit.context,
            &permit.entry,
            RunStatus::Running,
            finish,
            now,
        )
        .await
        .map_err(|e| format!("{e:?}"))?;
        permit.finalized = true;
        Ok(())
    }

    /// R07-T06：HTTP 工作台发起的媒体生成合成真实来源 Run（与封面链同一
    /// 先例：专属会话受理 + 注册许可 + 取消域 + 持久 started/lineage），生成
    /// 调用经同一工具网关准备/批准/执行，绝不是绕过运行层的直接受理。
    /// 返回的凭据持有会话许可直到 finish_media_request；媒体执行子运行在
    /// 受理时经 parent_run_permit 核对本来源。
    #[allow(clippy::too_many_arguments)]
    pub async fn admit_media_request(
        self: &Arc<Self>,
        db: &Arc<RunDatabase>,
        events: &Arc<EventService>,
        session_id: &str,
        agent_id: &str,
        principal: lingxi_kernel::Principal,
        request_id: &str,
        mode: lingxi_kernel::subagent::SessionPermissionMode,
        now: u64,
    ) -> Result<MediaRequestPermit, String> {
        let gate = db.session_admission();
        if self
            .storage_admission
            .as_ref()
            .is_none_or(|bound| !Arc::ptr_eq(bound, gate))
        {
            return Err("媒体来源的数据根与运行宿主不匹配".into());
        }
        let admission = Arc::new(
            gate.try_acquire(&[session_id], usize::MAX)
                .map_err(|_| MEDIA_REQUEST_SESSION_BUSY.to_string())?,
        );
        let session = session_tree::get_session_row(db, session_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("媒体来源会话不存在")?;
        if session.lifecycle != "active"
            || session.agent_id != agent_id
            || session.owner_user_id != principal.storage_subject()
            || session.permission_mode.as_deref() == Some("read_only")
            || !principal.can_access_session(&session.owner_user_id, session_id)
        {
            return Err("媒体来源会话身份或权限已改变".into());
        }
        let run = RunId::new(RunDatabase::allocate_run_id(db, now).map_err(|e| e.to_string())?);
        let driver = self
            .tasks
            .claim_driver(run.as_str())
            .map_err(|e| e.to_string())?;
        let entry = self.cancel.register(run.as_str());
        let context = lingxi_kernel::RunContext {
            principal,
            session_id: lingxi_protocol::SessionId::new(session_id),
            run_id: run.clone(),
            attempt: attempt_id(&run, 1),
            generation: 1,
        };
        let authorization = DriveAuthorization::user_submission(Some(request_id), mode);
        let lineage = authorization.lineage.clone();
        let registration = self
            .tasks
            .retain_authorized_run_permit(
                run.as_str(),
                session_id,
                agent_id,
                &context.principal,
                &authorization,
                admission.clone(),
            )
            .map_err(|e| e.to_string())?;
        self.tasks
            .bind_run_scope(run.as_str(), &entry.scope)
            .map_err(|e| e.to_string())?;
        let permit = MediaRequestPermit {
            context,
            cancel_scope: entry.scope.clone(),
            entry,
            supervisor: self.clone(),
            _driver: driver,
            _registration: registration,
            _admission: admission,
            finalized: false,
        };
        let committed = db
            .record_run_started(&permit.context, now)
            .await
            .map_err(|e| e.to_string())?;
        events.publish_committed(&committed.events);
        db.record_run_lineage(&permit.context, lineage, now)
            .await
            .map_err(|e| e.to_string())?;
        Ok(permit)
    }

    /// HTTP 来源 Run 的统一收尾：受理成功是过程完成（产物由后台媒体执行
    /// 继续交付，A11 —— 受理绝不冒充产物完成）；拒绝/失败如实落 Failed。
    pub async fn finish_media_request(
        &self,
        db: &Arc<RunDatabase>,
        events: &Arc<EventService>,
        mut permit: MediaRequestPermit,
        result: Result<(), String>,
        now: u64,
    ) -> Result<(), String> {
        if !std::ptr::eq(self, permit.supervisor.as_ref()) {
            return Err("媒体来源收尾凭据不属于当前运行宿主".into());
        }
        let finish = match &result {
            Ok(()) => RunFinish::CompletedWithoutFinal {
                cause: NoFinalCause::ProcessOnly,
            },
            Err(code) => RunFinish::Failed {
                cause: FailureCause::ProviderFailed {
                    code: code.clone(),
                    retryable: false,
                },
            },
        };
        self.adjudicated_finalize(
            db.as_ref(),
            events,
            &permit.context,
            &permit.entry,
            RunStatus::Running,
            finish,
            now,
        )
        .await
        .map_err(|e| format!("{e:?}"))?;
        permit.finalized = true;
        Ok(())
    }

    /// 批准等待的状态观测（与回合驱动同一词汇）：进入/离开 waiting_approval
    /// 各一次持久状态迁移，供运行面如实展示"正在等待人工决定"。
    pub async fn media_request_wait_approval(
        &self,
        db: &Arc<RunDatabase>,
        events: &Arc<EventService>,
        ctx: &lingxi_kernel::RunContext,
        now: u64,
    ) -> Result<(), String> {
        self.persist_state_change(
            db.as_ref(),
            events,
            ctx,
            RunStatus::Running,
            RunStatus::WaitingApproval,
            Some("approval_required".to_string()),
            now,
        )
        .await
        .map_err(|e| format!("{e:?}"))
    }

    pub async fn media_request_resume_running(
        &self,
        db: &Arc<RunDatabase>,
        events: &Arc<EventService>,
        ctx: &lingxi_kernel::RunContext,
        now: u64,
    ) -> Result<(), String> {
        self.persist_state_change(
            db.as_ref(),
            events,
            ctx,
            RunStatus::WaitingApproval,
            RunStatus::Running,
            Some("approval_resolved".to_string()),
            now,
        )
        .await
        .map_err(|e| format!("{e:?}"))
    }
}

/// 来源 Run 受理时与回合驱动同一语义的会话忙文案常量（路由层按此映射 409）。
pub(crate) const MEDIA_REQUEST_SESSION_BUSY: &str = "媒体来源会话正在使用，稍后再试";

/// HTTP 来源媒体 Run 的执行凭据；与封面凭据同形：许可/注册/取消域随凭据
/// 持有，未统一收尾的丢弃是取消 + Abandoned，不是静默成功。
pub struct MediaRequestPermit {
    pub context: lingxi_kernel::RunContext,
    pub cancel_scope: CancelScope,
    entry: Arc<RunCancelEntry>,
    supervisor: Arc<RunSupervisor>,
    _driver: crate::task_supervisor::DriverClaim,
    _registration: crate::task_supervisor::RunPermitRegistration,
    _admission: Arc<SessionPermit>,
    finalized: bool,
}
impl Drop for MediaRequestPermit {
    fn drop(&mut self) {
        if !self.finalized {
            self.cancel_scope.cancel("媒体来源驱动未完成统一收尾");
            self.entry.advance_phase(CancelPhase::Abandoned {
                reason: "媒体来源驱动退出，外部结果尚未统一确认".into(),
            });
        }
        self.supervisor
            .cancel
            .deregister(self.context.run_id.as_str());
    }
}
