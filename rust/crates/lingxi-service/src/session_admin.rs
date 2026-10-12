//! R06-T03 SessionAdminService：会话管理面（创建/改名/置顶/归档/恢复/
//! 永久删除/清理/搜索/记忆开关）的服务编排层。
//!
//! 现役对照锚点（逐条亲验于 `server/routes/sessions.ts`）：
//! rename:2493 / cleanup:2517（maxAgeDays 默认 90）/ archived:2555 /
//! archive:2569（child_sessions_present 409、archive_children 递归且流式跳过 /
//! detach_children 摘直接子对话）/ restore:2813（仅归档态）/
//! archived-delete:2882（仅归档态）/ pin:1014 / pin-order:1052（整体重编号、
//! 重复 id 400）/ search:857（query 上限 512，title/content 两阶段）/
//! memory GET:1103 PATCH:1135。
//!
//! 归属闸在路由层（get_for）统一执行；本层只负责语义编排与响亮失败。

use std::sync::Arc;

use serde::Serialize;

use lingxi_adapters::storage::RunDatabase;
use lingxi_adapters::storage::session_admin as store;
use lingxi_adapters::storage::session_tree::SessionTreeRow;
use lingxi_kernel::ports::StorageError;

/// 管理面错误：每一支都映射到响亮的 HTTP 结局。
#[derive(Debug, Clone, PartialEq)]
pub enum SessionAdminError {
    NotFound,
    /// 生命周期态不允许（如未归档却永久删除/恢复，已归档却再归档）→ 409。
    WrongLifecycle {
        detail: String,
    },
    /// 子对话存在且未指定处置策略 → 409 child_sessions_present（现役同款）。
    ChildrenPresent {
        child_count: usize,
    },
    /// 会话 id 撞库（对照现役 sessions.ts:587 active_session_conflict → 409；
    /// REPAIR-R1 观察项同类入口：不再落 Internal 500）。
    /// 实际删除操作墓碑持有旧业务身份，明确冲突而不是永久忙。
    IdentityRetired {
        session_id: String,
    },
    SessionExists {
        session_id: String,
    },
    /// 请求体语义非法（重复 id、查询过长、目录不可解析等）→ 400。
    InvalidRequest {
        detail: String,
    },
    /// 会话忙，拒绝永久删除/归档（保护在跑 run 的存储不被抽走）→ 409。
    Busy,
    ArchiveFailed {
        source: StorageError,
        view: ArchiveView,
        rolled_back: bool,
    },
    Storage(StorageError),
}

impl From<StorageError> for SessionAdminError {
    fn from(e: StorageError) -> Self {
        match &e {
            StorageError::Conflict { detail } if detail.starts_with("child_sessions_present") => {
                let n = detail
                    .rsplit(':')
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(0);
                SessionAdminError::ChildrenPresent { child_count: n }
            }
            StorageError::Conflict { detail }
                if detail.starts_with("session_identity_retired:") =>
            {
                SessionAdminError::IdentityRetired {
                    session_id: detail
                        .trim_start_matches("session_identity_retired:")
                        .to_string(),
                }
            }
            StorageError::Conflict { detail } if detail.starts_with("session_exists:") => {
                SessionAdminError::SessionExists {
                    session_id: detail.trim_start_matches("session_exists:").to_string(),
                }
            }
            StorageError::InvalidRequest { detail } => SessionAdminError::InvalidRequest {
                detail: detail.clone(),
            },
            _ => SessionAdminError::Storage(e),
        }
    }
}

/// 归档结果视图（对照现役响应字段）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveView {
    pub session_id: String,
    pub archived_children: usize,
    pub detached_children: usize,
    pub skipped_streaming_children: usize,
    pub errors: usize,
}

/// 清理结果视图（对照现役 {deleted, maxAgeDays}）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupView {
    pub deleted: usize,
    pub max_age_days: u64,
    /// 流式中而跳过的归档会话数（现役 cleanup 不含此项——归档文件在磁盘，
    /// 现役按 mtime 删；本实现保护在跑 run，显式跳过并如实上报）。
    pub skipped_busy: usize,
}

/// 置顶结果视图（对照现役 {pinnedAt, pinOrder}）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinView {
    pub session_id: String,
    pub pinned_at_unix_ms: Option<i64>,
    pub pin_order: Option<i64>,
}

/// 现役搜索查询上限（sessions.ts 的 SESSION_SEARCH_QUERY_MAX_LENGTH=512）。
pub const SESSION_SEARCH_QUERY_MAX_CHARS: usize = 512;

/// SessionAdminService：管理面编排。
#[derive(Clone)]
pub struct SessionAdminService {
    storage: Arc<RunDatabase>,
    context_compiler: Option<Arc<crate::context_compiler::ContextCompilerService>>,
    session_files: Option<Arc<crate::sessionfiles::SessionFileService>>,
}

impl SessionAdminService {
    pub fn new(storage: Arc<RunDatabase>) -> Self {
        Self {
            storage,
            session_files: None,
            context_compiler: None,
        }
    }

    /// 正式管理面接通同一托管清扫器；物理失败的行仍在库中，重启可恢复。
    pub fn with_session_files(
        mut self,
        files: Arc<crate::sessionfiles::SessionFileService>,
    ) -> Self {
        self.session_files = Some(files);
        self
    }

    async fn cleanup_deleted_payloads(&self) -> Result<(), SessionAdminError> {
        if let Some(files) = &self.session_files {
            files.cleanup_cold_sessions().await.map_err(|err| {
                SessionAdminError::Storage(StorageError::Io {
                    detail: format!("session deleted; owned cache cleanup pending: {err}"),
                })
            })?;
        }
        Ok(())
    }

    pub fn with_context_compiler(
        mut self,
        compiler: Option<Arc<crate::context_compiler::ContextCompilerService>>,
    ) -> Self {
        self.context_compiler = compiler;
        self
    }
    fn reclaim_context(&self, sid: &str) {
        if let Some(compiler) = &self.context_compiler {
            compiler.invalidate_session(sid);
        }
    }

    fn acquire(
        &self,
        ids: &[&str],
    ) -> Result<lingxi_kernel::session_admission::SessionPermit, SessionAdminError> {
        self.storage
            .session_admission()
            .try_acquire(
                ids,
                crate::session_supervisor::SessionConcurrencyLimits::DEFAULT_REGISTRY_CAP,
            )
            .map_err(|_| SessionAdminError::Busy)
    }
    async fn join<T: Send + 'static>(
        task: tokio::task::JoinHandle<Result<T, SessionAdminError>>,
    ) -> Result<T, SessionAdminError> {
        task.await.map_err(|e| {
            SessionAdminError::Storage(StorageError::Internal {
                detail: format!("session admin operation task: {e}"),
            })
        })?
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn create_session(
        &self,
        sid: &str,
        agent: &str,
        owner: &str,
        title: &str,
        mode: Option<&str>,
        folders: &[String],
        memory: bool,
        now: u64,
    ) -> Result<(), SessionAdminError> {
        let (svc, sid, agent, owner, title, mode, folders) = (
            self.clone(),
            sid.to_string(),
            agent.to_string(),
            owner.to_string(),
            title.to_string(),
            mode.map(str::to_string),
            folders.to_vec(),
        );
        Self::join(tokio::spawn(async move {
            svc.create_session_inner(
                &sid,
                &agent,
                &owner,
                &title,
                mode.as_deref(),
                &folders,
                memory,
                now,
            )
            .await
        }))
        .await
    }
    pub async fn rename_session(&self, sid: &str, title: &str) -> Result<(), SessionAdminError> {
        let (svc, sid, title) = (self.clone(), sid.to_string(), title.to_string());
        Self::join(tokio::spawn(async move {
            svc.rename_session_inner(&sid, &title).await
        }))
        .await
    }
    pub async fn set_pinned(
        &self,
        sid: &str,
        pinned: bool,
        now: u64,
    ) -> Result<PinView, SessionAdminError> {
        let (svc, sid) = (self.clone(), sid.to_string());
        Self::join(tokio::spawn(async move {
            svc.set_pinned_inner(&sid, pinned, now).await
        }))
        .await
    }
    pub async fn set_pin_order(&self, ids: &[String], now: u64) -> Result<(), SessionAdminError> {
        let (svc, ids) = (self.clone(), ids.to_vec());
        Self::join(tokio::spawn(async move {
            svc.set_pin_order_inner(&ids, now).await
        }))
        .await
    }
    pub async fn archive_session(
        &self,
        sid: &str,
        mode: Option<store::ChildMode>,
        now: u64,
    ) -> Result<ArchiveView, SessionAdminError> {
        let (svc, sid) = (self.clone(), sid.to_string());
        Self::join(tokio::spawn(async move {
            svc.archive_session_inner(&sid, mode, now).await
        }))
        .await
    }
    pub async fn restore_session(&self, sid: &str, now: u64) -> Result<(), SessionAdminError> {
        let (svc, sid) = (self.clone(), sid.to_string());
        Self::join(tokio::spawn(async move {
            svc.restore_session_inner(&sid, now).await
        }))
        .await
    }
    pub async fn delete_archived_session(&self, sid: &str) -> Result<(), SessionAdminError> {
        let (svc, sid) = (self.clone(), sid.to_string());
        Self::join(tokio::spawn(async move {
            svc.delete_archived_session_inner(&sid).await
        }))
        .await
    }
    pub async fn cleanup_archived(
        &self,
        owner: Option<&str>,
        days: u64,
        now: u64,
    ) -> Result<CleanupView, SessionAdminError> {
        let (svc, owner) = (self.clone(), owner.map(str::to_string));
        Self::join(tokio::spawn(async move {
            svc.cleanup_archived_inner(owner.as_deref(), days, now)
                .await
        }))
        .await
    }
    pub async fn set_memory_enabled(
        &self,
        sid: &str,
        enabled: bool,
    ) -> Result<bool, SessionAdminError> {
        let (svc, sid) = (self.clone(), sid.to_string());
        Self::join(tokio::spawn(async move {
            svc.set_memory_enabled_inner(&sid, enabled).await
        }))
        .await
    }
    /// 创建新会话（顶层）。授权目录逐条须为可解析的现存目录
    /// （现役 validateAuthorizedFolder 的现存目录校验），否则响亮 400。
    #[allow(clippy::too_many_arguments)]
    async fn create_session_inner(
        &self,
        session_id: &str,
        agent_id: &str,
        owner_user_id: &str,
        title: &str,
        permission_mode: Option<&str>,
        authorized_folders: &[String],
        memory_enabled: bool,
        now_unix_ms: u64,
    ) -> Result<(), SessionAdminError> {
        let _permit = self.acquire(&[session_id])?;
        for folder in authorized_folders {
            let meta =
                std::fs::metadata(folder).map_err(|e| SessionAdminError::InvalidRequest {
                    detail: format!("authorized folder {folder} not resolvable: {e}"),
                })?;
            if !meta.is_dir() {
                return Err(SessionAdminError::InvalidRequest {
                    detail: format!("authorized folder {folder} is not a directory"),
                });
            }
        }
        store::create_session(
            &self.storage,
            session_id,
            agent_id,
            owner_user_id,
            title,
            permission_mode,
            authorized_folders,
            memory_enabled,
            now_unix_ms,
        )
        .await?;
        Ok(())
    }

    /// 改名（false → 404）。
    async fn rename_session_inner(
        &self,
        session_id: &str,
        title: &str,
    ) -> Result<(), SessionAdminError> {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Err(SessionAdminError::InvalidRequest {
                detail: "title must be non-empty".to_string(),
            });
        }
        let ok = store::rename_session(&self.storage, session_id, trimmed).await?;
        if !ok {
            return Err(SessionAdminError::NotFound);
        }
        Ok(())
    }

    /// 置顶/取消置顶（None → 404）。
    async fn set_pinned_inner(
        &self,
        session_id: &str,
        pinned: bool,
        now_unix_ms: u64,
    ) -> Result<PinView, SessionAdminError> {
        let (pinned_at, pin_order) =
            store::set_pinned(&self.storage, session_id, pinned, now_unix_ms)
                .await?
                .ok_or(SessionAdminError::NotFound)?;
        Ok(PinView {
            session_id: session_id.to_string(),
            pinned_at_unix_ms: pinned_at,
            pin_order,
        })
    }

    /// 置顶区整体重编号；重复 id 响亮 400（现役 session_pin_order_duplicate）。
    async fn set_pin_order_inner(
        &self,
        ordered_session_ids: &[String],
        now_unix_ms: u64,
    ) -> Result<(), SessionAdminError> {
        let mut seen = std::collections::HashSet::new();
        for id in ordered_session_ids {
            if !seen.insert(id.as_str()) {
                return Err(SessionAdminError::InvalidRequest {
                    detail: format!("session_pin_order_duplicate: {id}"),
                });
            }
        }
        store::set_pin_order(&self.storage, ordered_session_ids, now_unix_ms).await?;
        Ok(())
    }

    /// 归档（含子对话策略）。`is_busy` 由路由层从 supervisor 注入：
    /// 目标自身忙 → 409；archive_children 时流式后代跳过并计数。
    async fn archive_session_inner(
        &self,
        session_id: &str,
        child_mode: Option<store::ChildMode>,
        now_unix_ms: u64,
    ) -> Result<ArchiveView, SessionAdminError> {
        let _permit = self.acquire(&[session_id])?;
        let mut descendants = store::list_descendant_sessions(&self.storage, session_id)
            .await
            .map_err(|source| Self::archive_read_error(session_id, source, 0))?;
        descendants.sort();
        let mut child_permits = Vec::new();
        let mut skip = Vec::new();
        for id in &descendants {
            match self.acquire(&[id]) {
                Ok(permit) => child_permits.push(permit),
                Err(SessionAdminError::Busy)
                    if child_mode == Some(store::ChildMode::ArchiveChildren) =>
                {
                    skip.push(id.clone())
                }
                Err(e) => return Err(e),
            }
        }
        // 取得后代后再次取谱系；并发新增的后代不落入未加保护的写集。
        let mut current_descendants = store::list_descendant_sessions(&self.storage, session_id)
            .await
            .map_err(|source| Self::archive_read_error(session_id, source, skip.len()))?;
        current_descendants.sort();
        if descendants != current_descendants {
            return Err(SessionAdminError::Busy);
        }
        let outcome = match store::archive_session_counted(
            &self.storage,
            session_id,
            child_mode,
            &skip,
            now_unix_ms,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(failure) => {
                let mapped = SessionAdminError::from(failure.source);
                return Err(match mapped {
                    SessionAdminError::Storage(source) => SessionAdminError::ArchiveFailed {
                        source,
                        view: ArchiveView {
                            session_id: session_id.to_string(),
                            archived_children: failure.outcome.archived_children,
                            detached_children: failure.outcome.detached_children,
                            skipped_streaming_children: failure.outcome.skipped_streaming_children,
                            errors: failure.outcome.errors,
                        },
                        rolled_back: true,
                    },
                    other => other,
                });
            }
        };
        self.reclaim_context(session_id);
        for id in descendants {
            if !skip.contains(&id) {
                self.reclaim_context(&id);
            }
        }
        Ok(ArchiveView {
            session_id: session_id.to_string(),
            archived_children: outcome.archived_children,
            detached_children: outcome.detached_children,
            skipped_streaming_children: outcome.skipped_streaming_children,
            errors: outcome.errors,
        })
    }

    fn archive_read_error(
        session_id: &str,
        source: StorageError,
        skipped: usize,
    ) -> SessionAdminError {
        match SessionAdminError::from(source) {
            SessionAdminError::Storage(source) => SessionAdminError::ArchiveFailed {
                source,
                view: ArchiveView {
                    session_id: session_id.to_string(),
                    archived_children: 0,
                    detached_children: 0,
                    skipped_streaming_children: skipped,
                    errors: 1,
                },
                // 写事务尚未开始，不能声称实际执行了事务回滚。
                rolled_back: false,
            },
            other => other,
        }
    }

    /// 恢复归档会话（WrongLifecycle → 409；NotFound → 404）。
    async fn restore_session_inner(
        &self,
        session_id: &str,
        now_unix_ms: u64,
    ) -> Result<(), SessionAdminError> {
        let _permit = self.acquire(&[session_id])?;
        match store::restore_session(&self.storage, session_id, now_unix_ms).await? {
            store::LifecycleOutcome::Done => {
                self.reclaim_context(session_id);
                self.cleanup_deleted_payloads().await
            }
            store::LifecycleOutcome::NotFound => Err(SessionAdminError::NotFound),
            store::LifecycleOutcome::WrongLifecycle => Err(SessionAdminError::WrongLifecycle {
                detail: "restore requires an archived session".to_string(),
            }),
        }
    }

    /// 永久删除（仅归档态；忙会话拒绝，保护在跑 run 的存储）。
    async fn delete_archived_session_inner(
        &self,
        session_id: &str,
    ) -> Result<(), SessionAdminError> {
        let _permit = self.acquire(&[session_id])?;
        match store::delete_archived_session(&self.storage, session_id).await? {
            store::LifecycleOutcome::Done => {
                self.reclaim_context(session_id);
                self.cleanup_deleted_payloads().await
            }
            store::LifecycleOutcome::NotFound => Err(SessionAdminError::NotFound),
            store::LifecycleOutcome::WrongLifecycle => Err(SessionAdminError::WrongLifecycle {
                detail: "permanent delete requires an archived session".to_string(),
            }),
        }
    }

    /// 清理过期归档（现役 maxAgeDays 默认 90；流式中的跳过并计数）。
    async fn cleanup_archived_inner(
        &self,
        owner: Option<&str>,
        max_age_days: u64,
        now_unix_ms: u64,
    ) -> Result<CleanupView, SessionAdminError> {
        let cutoff = now_unix_ms.saturating_sub(max_age_days.saturating_mul(86_400_000));
        let archived = store::list_archived_sessions(&self.storage, owner).await?;
        let mut deleted = 0usize;
        let mut skipped_busy = 0usize;
        for row in archived {
            let archived_at = row.archived_at_unix_ms.unwrap_or(row.created_at_unix_ms);
            if archived_at as u64 >= cutoff {
                continue;
            }
            let _permit = match self.acquire(&[&row.session_id]) {
                Ok(p) => p,
                Err(SessionAdminError::Busy) => {
                    skipped_busy += 1;
                    continue;
                }
                Err(e) => return Err(e),
            };
            // 持操作权后重查归档时刻与归属，避免沿旧清单删除刚恢复/重新归档的会话。
            let current = lingxi_adapters::storage::session_tree::get_session_row(
                &self.storage,
                &row.session_id,
            )
            .await?;
            let Some(current) = current else { continue };
            if current.lifecycle != "archived"
                || current
                    .archived_at_unix_ms
                    .unwrap_or(current.created_at_unix_ms) as u64
                    >= cutoff
                || owner.is_some_and(|o| o != current.owner_user_id)
            {
                continue;
            }
            match store::delete_archived_session(&self.storage, &row.session_id).await? {
                store::LifecycleOutcome::Done => {
                    self.cleanup_deleted_payloads().await?;
                    self.reclaim_context(&row.session_id);
                    deleted += 1;
                }
                // 竞态（清理中被恢复/删除）：如实跳过，不算删成也不算错。
                store::LifecycleOutcome::NotFound | store::LifecycleOutcome::WrongLifecycle => {}
            }
        }
        Ok(CleanupView {
            deleted,
            max_age_days,
            skipped_busy,
        })
    }

    /// 列出已归档会话。
    pub async fn list_archived(
        &self,
        owner: Option<&str>,
    ) -> Result<Vec<store::SessionSummaryRow>, SessionAdminError> {
        Ok(store::list_archived_sessions(&self.storage, owner).await?)
    }

    /// 搜索（query 上限 512 字符 → 响亮 400；对照现役 query_too_long）。
    pub async fn search(
        &self,
        owner: Option<&str>,
        query: &str,
        phase: &str,
        limit: usize,
    ) -> Result<Vec<store::SearchHit>, SessionAdminError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        if trimmed.chars().count() > SESSION_SEARCH_QUERY_MAX_CHARS {
            return Err(SessionAdminError::InvalidRequest {
                detail: format!("query_too_long: max {SESSION_SEARCH_QUERY_MAX_CHARS} characters"),
            });
        }
        if phase != "title" && phase != "content" {
            return Err(SessionAdminError::InvalidRequest {
                detail: format!("unknown search phase {phase}"),
            });
        }
        // LIKE 转义（ESCAPE '\'）：% _ \ 逐条转义，杜绝通配符注入。
        let mut pat = String::with_capacity(trimmed.len() + 2);
        pat.push('%');
        for ch in trimmed.chars() {
            if matches!(ch, '%' | '_' | '\\') {
                pat.push('\\');
            }
            pat.push(ch);
        }
        pat.push('%');
        Ok(store::search_sessions(&self.storage, owner, &pat, phase, limit).await?)
    }

    /// 记忆开关读取（None → 404）。
    pub async fn get_memory_enabled(&self, session_id: &str) -> Result<bool, SessionAdminError> {
        let row =
            lingxi_adapters::storage::session_tree::get_session_row(&self.storage, session_id)
                .await?
                .ok_or(SessionAdminError::NotFound)?;
        Ok(row.memory_enabled)
    }

    /// 记忆开关写入（None → 404）。
    async fn set_memory_enabled_inner(
        &self,
        session_id: &str,
        enabled: bool,
    ) -> Result<bool, SessionAdminError> {
        store::set_memory_enabled(&self.storage, session_id, enabled)
            .await?
            .ok_or(SessionAdminError::NotFound)
    }

    /// 会话行全列投影（find / 详情扩展用；None → 404）。
    pub async fn get_session_row(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionTreeRow>, SessionAdminError> {
        Ok(
            lingxi_adapters::storage::session_tree::get_session_row(&self.storage, session_id)
                .await?,
        )
    }
}
