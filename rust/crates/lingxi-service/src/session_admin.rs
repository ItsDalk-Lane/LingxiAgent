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

use lingxi_adapters::storage::session_admin as store;
use lingxi_adapters::storage::session_tree::SessionTreeRow;
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::StorageError;

/// 管理面错误：每一支都映射到响亮的 HTTP 结局。
#[derive(Debug, Clone, PartialEq)]
pub enum SessionAdminError {
    NotFound,
    InactiveMemorySession,
    LegacyFileConflict {
        detail: String,
    },
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
            StorageError::Conflict { detail } if detail.starts_with("legacy_session_conflict:") => {
                SessionAdminError::LegacyFileConflict {
                    detail: detail.clone(),
                }
            }
            StorageError::InvalidRequest { detail }
                if detail == "legacy_session_file_not_found" =>
            {
                SessionAdminError::NotFound
            }
            StorageError::Conflict { detail } if detail == "session_memory_inactive" => {
                SessionAdminError::InactiveMemorySession
            }
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

/// R06 授权目录变更动作（现役 PATCH /sessions/authorized-folders 的
/// add/remove/set 三分支）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderAction {
    Add(String),
    Remove(String),
    Set(Vec<String>),
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
    input_drafts: Option<Arc<crate::input_drafts::InputDraftsService>>,
}

impl SessionAdminService {
    pub fn new(storage: Arc<RunDatabase>) -> Self {
        Self {
            storage,
            session_files: None,
            context_compiler: None,
            input_drafts: None,
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

    /// R06 永久删除时同步清理该会话的输入草稿条目（现役删除要求文件与
    /// 草稿一并消失）。
    pub fn with_input_drafts(
        mut self,
        drafts: Arc<crate::input_drafts::InputDraftsService>,
    ) -> Self {
        self.input_drafts = Some(drafts);
        self
    }

    /// 永久删除后的草稿清理：失败不回滚行删除，显式报"清理待完成"。
    async fn cleanup_deleted_drafts(&self, session_id: &str) -> Result<(), SessionAdminError> {
        if let Some(drafts) = &self.input_drafts {
            drafts
                .remove_session_entries(&[session_id])
                .await
                .map_err(|err| {
                    SessionAdminError::Storage(StorageError::Io {
                        detail: format!("session deleted; draft cleanup pending: {err}"),
                    })
                })?;
        }
        Ok(())
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
            svc.restore_session_inner(&sid, now, None).await
        }))
        .await
        .map(|_| ())
    }
    pub async fn restore_session_checked(
        &self,
        sid: &str,
        now: u64,
        expected: lingxi_adapters::storage::legacy_session_files::ExpectedSource,
    ) -> Result<Option<std::path::PathBuf>, SessionAdminError> {
        let (svc, sid) = (self.clone(), sid.to_string());
        Self::join(tokio::spawn(async move {
            svc.restore_session_inner(&sid, now, Some(expected)).await
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
        let proactive = self
            .context_compiler
            .as_ref()
            .map(|compiler| compiler.new_session_proactive_delegation())
            .transpose()
            .map_err(|error| {
                SessionAdminError::Storage(StorageError::Io {
                    detail: format!("new session policy read failed: {error}"),
                })
            })?
            .unwrap_or(false);
        store::create_session_with_policy(
            &self.storage,
            session_id,
            agent_id,
            owner_user_id,
            title,
            permission_mode,
            authorized_folders,
            memory_enabled,
            now_unix_ms,
            proactive,
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
                        rolled_back: failure.rolled_back,
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
        expected: Option<lingxi_adapters::storage::legacy_session_files::ExpectedSource>,
    ) -> Result<Option<std::path::PathBuf>, SessionAdminError> {
        let _permit = self.acquire(&[session_id])?;
        match store::restore_session_checked(&self.storage, session_id, now_unix_ms, expected)
            .await?
        {
            store::LifecycleOutcome::Done => {
                self.reclaim_context(session_id);
                self.cleanup_deleted_payloads().await?;
                Ok(
                    lingxi_adapters::storage::legacy_session_files::restored_path(
                        &self.storage,
                        session_id,
                    )
                    .await?,
                )
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
                self.cleanup_deleted_drafts(session_id).await?;
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
        Ok(store::list_archived_sessions_with_preview(&self.storage, owner).await?)
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
        if row.archived_at_unix_ms.is_some() {
            return Err(SessionAdminError::InactiveMemorySession);
        }
        Ok(row.memory_enabled)
    }

    /// R06 会话授权目录读取（现役 getSessionFolderScope 的授权目录部分）：
    /// 仅活动会话（已归档 → 生命周期冲突）。
    pub async fn authorized_folders(
        &self,
        session_id: &str,
    ) -> Result<Vec<String>, SessionAdminError> {
        let row =
            lingxi_adapters::storage::session_tree::get_session_row(&self.storage, session_id)
                .await?
                .ok_or(SessionAdminError::NotFound)?;
        if row.archived_at_unix_ms.is_some() {
            return Err(SessionAdminError::WrongLifecycle {
                detail: "session_archived".into(),
            });
        }
        Ok(row.authorized_folders)
    }

    /// R06 授权目录变更：add 去重保位、remove 容忍缺席、set 整体替换。
    /// 目录存在性/类型校验与路径规范化由路由层先行完成（现役
    /// validateAuthorizedFolder/normalizeAuthorizedFolderPath 同口径）。
    pub async fn mutate_authorized_folders(
        &self,
        session_id: &str,
        action: FolderAction,
    ) -> Result<Vec<String>, SessionAdminError> {
        let (svc, sid) = (self.clone(), session_id.to_string());
        Self::join(tokio::spawn(async move {
            let row = lingxi_adapters::storage::session_tree::get_session_row(&svc.storage, &sid)
                .await?
                .ok_or(SessionAdminError::NotFound)?;
            if row.archived_at_unix_ms.is_some() {
                return Err(SessionAdminError::WrongLifecycle {
                    detail: "session_archived".into(),
                });
            }
            let mut folders = row.authorized_folders;
            match action {
                FolderAction::Add(folder) => {
                    if !folders.contains(&folder) {
                        folders.push(folder);
                    }
                }
                FolderAction::Remove(folder) => folders.retain(|item| *item != folder),
                FolderAction::Set(list) => folders = list,
            }
            store::set_authorized_folders(&svc.storage, &sid, &folders)
                .await?
                .ok_or(SessionAdminError::NotFound)
        }))
        .await
    }

    /// R06 v12 会话项目归属写入（分配 / null 清除），返回新归属。
    pub async fn set_session_project(
        &self,
        session_id: &str,
        project_id: Option<&str>,
    ) -> Result<Option<Option<String>>, SessionAdminError> {
        let (svc, sid) = (self.clone(), session_id.to_string());
        let project = project_id.map(str::to_string);
        Self::join(tokio::spawn(async move {
            match store::set_session_project(&svc.storage, &sid, project.as_deref()).await {
                Ok(value) => Ok(value),
                Err(StorageError::Conflict { .. }) => Err(SessionAdminError::WrongLifecycle {
                    detail: "session_archived".into(),
                }),
                Err(other) => Err(other.into()),
            }
        }))
        .await
    }

    /// R06 独立会话谱系锚点写入（不存在或非活动 → NotFound）。
    pub async fn set_session_lineage(
        &self,
        session_id: &str,
        parent_session_id: &str,
        lineage_depth: u32,
    ) -> Result<(), SessionAdminError> {
        let (svc, sid, parent) = (
            self.clone(),
            session_id.to_string(),
            parent_session_id.to_string(),
        );
        Self::join(tokio::spawn(async move {
            store::set_session_lineage(&svc.storage, &sid, &parent, lineage_depth)
                .await?
                .then_some(())
                .ok_or(SessionAdminError::NotFound)
        }))
        .await
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

    // ── R06 C1 收尾：工作台处置 / 孤儿清扫（现役 sessions.ts:2670/:2762）──

    /// R06 v13 工作台身份写入（分配或 null 清除），返回写入后的两列值。
    pub async fn set_session_workspace(
        &self,
        session_id: &str,
        workspace_mount_id: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<Option<(Option<String>, Option<String>)>, SessionAdminError> {
        let (svc, sid) = (self.clone(), session_id.to_string());
        let mount = workspace_mount_id.map(str::to_string);
        let owned_cwd = cwd.map(str::to_string);
        Self::join(tokio::spawn(async move {
            match store::set_session_workspace(
                &svc.storage,
                &sid,
                mount.as_deref(),
                owned_cwd.as_deref(),
            )
            .await
            {
                Ok(value) => Ok(value),
                Err(StorageError::Conflict { .. }) => Err(SessionAdminError::WrongLifecycle {
                    detail: "session_archived".into(),
                }),
                Err(other) => Err(other.into()),
            }
        }))
        .await
    }

    /// 工作台处置：把指定工作台名下的全部（本主体可见的）活动会话归档，
    /// 或归档后永久删除（action=delete，含输入草稿清理——草稿清理失败
    /// 不阻塞删除，对照现役 sessions.ts:2735 的 try/catch 吞错口径）。
    ///
    /// 匹配口径（现役 :2703-2711）：优先 workspaceMountId 严格相等；
    /// 会话无 mountId 时按 cwd 双形态并入（canonicalize 成功比 canonical、
    /// 否则 trim 相等——授权目录同口径；现役经 resolveSessionWorkspaceSelection
    /// 把 mount 解析成根路径再比，Rust 侧尚无挂载注册表写路径，mountId
    /// 只做直配，见路由层注释）。逐会话失败收集进 errors 不中断；
    /// 流式中（会话准入被占）跳过并计数。
    pub async fn dispose_workspace_sessions(
        &self,
        owner: Option<&str>,
        action: DisposalAction,
        mount_id: Option<&str>,
        cwd_identity: Option<&str>,
        now_unix_ms: u64,
    ) -> Result<WorkspaceDisposalView, SessionAdminError> {
        let (svc, owner) = (self.clone(), owner.map(str::to_string));
        let mount = mount_id.map(str::to_string);
        let cwd = cwd_identity.map(str::to_string);
        Self::join(tokio::spawn(async move {
            svc.dispose_workspace_sessions_inner(
                owner.as_deref(),
                action,
                mount.as_deref(),
                cwd.as_deref(),
                now_unix_ms,
            )
            .await
        }))
        .await
    }

    async fn dispose_workspace_sessions_inner(
        &self,
        owner: Option<&str>,
        action: DisposalAction,
        mount_id: Option<&str>,
        cwd_identity: Option<&str>,
        now_unix_ms: u64,
    ) -> Result<WorkspaceDisposalView, SessionAdminError> {
        let mount = mount_id.map(str::trim).filter(|v| !v.is_empty());
        let cwd = cwd_identity.map(str::trim).filter(|v| !v.is_empty());
        let rows = store::list_active_session_workspaces(&self.storage, owner).await?;
        let targets: Vec<store::SessionWorkspaceRow> = rows
            .into_iter()
            .filter(|row| {
                let row_mount = row
                    .workspace_mount_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty());
                if mount.is_some() && row_mount.is_some() {
                    return row_mount == mount;
                }
                let row_cwd = row.cwd.as_deref().map(str::trim).filter(|v| !v.is_empty());
                match (cwd, row_cwd) {
                    (Some(identity), Some(session_cwd)) => {
                        workspace_paths_equal(identity, session_cwd)
                    }
                    _ => false,
                }
            })
            .collect();
        let matched = targets.len();
        let mut disposed = 0usize;
        let mut skipped_streaming = 0usize;
        let mut errors: Vec<DisposalItemError> = Vec::new();
        for target in targets {
            let sid = target.session_id.clone();
            // 会话准入：被在跑 run 占用 = 流式中 → 跳过并计数（现役
            // isSessionStreaming 口径；批内持权防并发执行插入）。
            let _permit = match self.acquire(&[&sid]) {
                Ok(permit) => permit,
                Err(SessionAdminError::Busy) => {
                    skipped_streaming += 1;
                    continue;
                }
                Err(e) => {
                    errors.push(DisposalItemError::new(&sid, &e));
                    continue;
                }
            };
            let outcome = store::archive_session(&self.storage, &sid, None, &[], now_unix_ms).await;
            match outcome {
                Ok(_) => {
                    if action == DisposalAction::Delete {
                        match store::delete_archived_session(&self.storage, &sid).await {
                            Ok(store::LifecycleOutcome::Done) => {
                                // 草稿/冷缓存清理失败不阻塞删除（现役
                                // try/catch 吞错；行删除已提交，如实继续）。
                                let _ = self.cleanup_deleted_drafts(&sid).await;
                                let _ = self.cleanup_deleted_payloads().await;
                            }
                            // 竞态（并发恢复/删除）：归档已落，按现役
                            // ENOENT→error 条目口径上报，不中断其余会话。
                            Ok(_) => {
                                errors.push(DisposalItemError {
                                    path: sid.clone(),
                                    message: "archived_session_deleted_race".to_string(),
                                });
                                self.reclaim_context(&sid);
                                continue;
                            }
                            Err(source) => {
                                errors.push(DisposalItemError::new(
                                    &sid,
                                    &SessionAdminError::from(source),
                                ));
                                self.reclaim_context(&sid);
                                continue;
                            }
                        }
                    }
                    self.reclaim_context(&sid);
                    disposed += 1;
                }
                Err(source) => {
                    errors.push(DisposalItemError::new(
                        &sid,
                        &SessionAdminError::from(source),
                    ));
                }
            }
        }
        Ok(WorkspaceDisposalView {
            ok: true,
            action: match action {
                DisposalAction::Archive => "archive",
                DisposalAction::Delete => "delete",
            },
            matched,
            disposed,
            skipped_streaming,
            errors,
        })
    }

    /// 孤儿会话清扫：所属工作台已不存在（mount 被移除 / 磁盘目录被直接
    /// 删除）的活动会话静默自动归档（现役 :2762；不弹框，归档界面可见）。
    /// `active_mount_ids` 由路由层从 studio-mounts.json 读出（读失败按仅
    /// {"default"}——宁可不清扫也不误归档）；目录仍在磁盘上但未被引用的
    /// cwd 会话不清扫（换配置目录的残留可经重新打开找回，不算暗数据）。
    pub async fn sweep_orphaned_sessions(
        &self,
        owner: Option<&str>,
        active_mount_ids: &std::collections::BTreeSet<String>,
        now_unix_ms: u64,
    ) -> Result<SweepOrphanedView, SessionAdminError> {
        let (svc, owner) = (self.clone(), owner.map(str::to_string));
        let mounts = active_mount_ids.clone();
        Self::join(tokio::spawn(async move {
            svc.sweep_orphaned_sessions_inner(owner.as_deref(), &mounts, now_unix_ms)
                .await
        }))
        .await
    }

    async fn sweep_orphaned_sessions_inner(
        &self,
        owner: Option<&str>,
        active_mount_ids: &std::collections::BTreeSet<String>,
        now_unix_ms: u64,
    ) -> Result<SweepOrphanedView, SessionAdminError> {
        let rows = store::list_active_session_workspaces(&self.storage, owner).await?;
        let scanned = rows.len();
        let mut archived = 0usize;
        let mut skipped_streaming = 0usize;
        let mut errors: Vec<DisposalItemError> = Vec::new();
        for row in rows {
            let sid = row.session_id.clone();
            let row_mount = row
                .workspace_mount_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty());
            let is_orphan = match row_mount {
                Some(mount) => !active_mount_ids.contains(mount),
                None => {
                    // 无 mountId 的 cwd 形态：目录在磁盘上不存在才算孤儿
                    //（现役 pathExists 原样存在性检查，不规范化）。
                    match row.cwd.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                        Some(cwd) => !std::path::Path::new(cwd).exists(),
                        None => false,
                    }
                }
            };
            if !is_orphan {
                continue;
            }
            let _permit = match self.acquire(&[&sid]) {
                Ok(permit) => permit,
                Err(SessionAdminError::Busy) => {
                    skipped_streaming += 1;
                    continue;
                }
                Err(e) => {
                    errors.push(DisposalItemError::new(&sid, &e));
                    continue;
                }
            };
            match store::archive_session(&self.storage, &sid, None, &[], now_unix_ms).await {
                Ok(_) => {
                    self.reclaim_context(&sid);
                    archived += 1;
                }
                Err(source) => {
                    errors.push(DisposalItemError::new(
                        &sid,
                        &SessionAdminError::from(source),
                    ));
                }
            }
        }
        Ok(SweepOrphanedView {
            ok: true,
            scanned,
            archived,
            skipped_streaming,
            errors,
        })
    }
}

// ── R06 C1 收尾：工作台处置 / 孤儿清扫的 HTTP 面 ───────────────────────────
// 现役锚点：POST /sessions/workspace-disposal（sessions.ts:2670）与
// POST /sessions/sweep-orphaned-workspaces（:2762）。处理器为 pub，待主控
// 在 lib.rs 的 build_router 注册（注册片段见任务报告）；在此之前用
// [`workspace_disposal_routes`] 挂载（测试与集成同款鉴权链）。

/// 处置动作（现役 body.action："delete" 归档后永久删除，其余/缺省归档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisposalAction {
    Archive,
    Delete,
}

/// 逐会话失败条目（现役 errors[{path, message}]；path 字段承载会话 id——
/// Rust 侧会话无文件路径身份，以 id 为定位键）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DisposalItemError {
    pub path: String,
    pub message: String,
}

impl DisposalItemError {
    fn new(session_id: &str, err: &SessionAdminError) -> Self {
        Self {
            path: session_id.to_string(),
            message: disposal_error_message(err),
        }
    }
}

/// 失败条目的 message 文案（现役取 err.message；这里按错误族给稳定短语）。
fn disposal_error_message(err: &SessionAdminError) -> String {
    match err {
        SessionAdminError::NotFound => "session not found".to_string(),
        SessionAdminError::InactiveMemorySession => "session_memory_inactive".to_string(),
        SessionAdminError::LegacyFileConflict { detail } => {
            format!("session_file_conflict: {detail}")
        }
        SessionAdminError::WrongLifecycle { detail } => {
            format!("session_lifecycle_conflict: {detail}")
        }
        SessionAdminError::ChildrenPresent { child_count } => {
            format!("child_sessions_present:{child_count}")
        }
        SessionAdminError::IdentityRetired { session_id } => {
            format!("session_identity_retired: {session_id}")
        }
        SessionAdminError::SessionExists { session_id } => {
            format!("session_exists: {session_id}")
        }
        SessionAdminError::InvalidRequest { detail } => detail.clone(),
        SessionAdminError::Busy => "session_busy".to_string(),
        SessionAdminError::ArchiveFailed { source, .. } => format!("archive failed: {source}"),
        SessionAdminError::Storage(source) => format!("storage failure: {source}"),
    }
}

/// 处置结果视图（对照现役响应 {ok, action, matched, disposed,
/// skippedStreaming, errors}）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDisposalView {
    pub ok: bool,
    pub action: &'static str,
    pub matched: usize,
    pub disposed: usize,
    pub skipped_streaming: usize,
    pub errors: Vec<DisposalItemError>,
}

/// 清扫结果视图（对照现役 {ok, scanned, archived, skippedStreaming, errors}）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepOrphanedView {
    pub ok: bool,
    pub scanned: usize,
    pub archived: usize,
    pub skipped_streaming: usize,
    pub errors: Vec<DisposalItemError>,
}

/// cwd 双形态比较（授权目录同口径）：两侧 canonicalize 都成功比 canonical
/// （符号链接/相对形态归一）；任一失败（如工作台目录已被删除）退回 trim
/// 字符串相等——现役 lexical normalizeWorkspacePath 的最小忠实替身。
fn workspace_paths_equal(identity: &str, session_cwd: &str) -> bool {
    match (
        std::fs::canonicalize(identity.trim()),
        std::fs::canonicalize(session_cwd.trim()),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => identity.trim() == session_cwd.trim(),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceDisposalBody {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    workspace_mount_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

/// `POST /lingxi/v1/sessions/workspace-disposal` — 工作台处置（现役
/// sessions.ts:2670）：移除工作台前，把它名下全部会话归档或永久删除。
/// 身份口径与左栏作用域一致：mount 严格匹配 + 目录路径双形态（经挂载
/// 创建的带 workspaceMountId；历史 cwd 形态按路径并入），不漏任何一本账。
/// 偏差：现役对 mountId 经 resolveSessionWorkspaceSelection 解析出 native
/// 根路径用于 cwd 双形态匹配（:2694）；Rust 侧尚无挂载注册表写路径，
/// 无根可解析，mountId 只做直配，cwd 按请求给出的 cwd 身份匹配。
pub async fn workspace_disposal_route(
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
    axum::extract::State(state): axum::extract::State<crate::ServiceState>,
    body: Result<axum::Json<WorkspaceDisposalBody>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let axum::Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return crate::EndpointError::from_json_rejection(&rej).into_response(),
    };
    let action = if req.action.as_deref() == Some("delete") {
        DisposalAction::Delete
    } else {
        DisposalAction::Archive
    };
    let mount = req
        .workspace_mount_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let cwd = req.cwd.as_deref().map(str::trim).filter(|v| !v.is_empty());
    if mount.is_none() && cwd.is_none() {
        // 现役 :2687-2689 的原样错误体（非 EndpointError 信封）。
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": "missing workspace identity",
                "code": "missing_workspace_identity",
            })),
        )
            .into_response();
    }
    match state
        .session_admin()
        .dispose_workspace_sessions(
            crate::owner_filter(&principal),
            action,
            mount,
            cwd,
            state.clock().now_unix_ms(),
        )
        .await
    {
        Ok(view) => (axum::http::StatusCode::OK, axum::Json(view)).into_response(),
        Err(err) => crate::session_admin_error_response(err),
    }
}

/// 活动挂载集合 = {"default"} + {data_home}/studio-mounts.json 里
/// status=active 的 mountId（注册表格式对照 core/studio-mounts.ts 的
/// {schemaVersion, mounts:[...]}）。读失败/损坏按仅 {"default"}——
/// 宁可不清扫也不误归档（现役 :2773-2777 的 try/catch 口径；宽松解析：
/// 只要 mounts 数组可读即采纳 active 项，注册表整体校验留给写路径）。
fn active_studio_mount_ids(data_home: &std::path::Path) -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::from(["default".to_string()]);
    let Ok(bytes) = std::fs::read(data_home.join("studio-mounts.json")) else {
        return ids;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return ids;
    };
    let Some(mounts) = value.get("mounts").and_then(|m| m.as_array()) else {
        return ids;
    };
    for mount in mounts {
        if mount.get("status").and_then(|s| s.as_str()) == Some("active") {
            if let Some(id) = mount.get("mountId").and_then(|i| i.as_str()) {
                let id = id.trim();
                if !id.is_empty() {
                    ids.insert(id.to_string());
                }
            }
        }
    }
    ids
}

/// `POST /lingxi/v1/sessions/sweep-orphaned-workspaces` — 孤儿会话清扫
/// （现役 sessions.ts:2762）：所属工作台已不存在的会话静默自动归档。
/// 边界：目录仍在磁盘上但未被引用的 cwd 会话不清扫。现役路由不读
/// body，这里原样消费不解析。
pub async fn sweep_orphaned_route(
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
    axum::extract::State(state): axum::extract::State<crate::ServiceState>,
    _body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let active_mounts = active_studio_mount_ids(&state.config().data_home);
    match state
        .session_admin()
        .sweep_orphaned_sessions(
            crate::owner_filter(&principal),
            &active_mounts,
            state.clock().now_unix_ms(),
        )
        .await
    {
        Ok(view) => (axum::http::StatusCode::OK, axum::Json(view)).into_response(),
        Err(err) => crate::session_admin_error_response(err),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionWorkspaceBody {
    #[serde(default)]
    workspace_mount_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

/// `PUT /lingxi/v1/sessions/{id}/workspace` — 工作台身份写入（现役会话
/// 创建时携带 workspaceMountId/cwd 字段的独立小路由替身：lib.rs 的
/// create 路由签名不在本任务可改面内，落库通道先由本叶面提供，
/// trim 后空串/null 清除；仅活动会话可改）。
pub async fn put_session_workspace_route(
    axum::Extension(principal): axum::Extension<crate::auth::Principal>,
    axum::extract::State(state): axum::extract::State<crate::ServiceState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    body: Result<axum::Json<SessionWorkspaceBody>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let axum::Json(req) = match body {
        Ok(j) => j,
        Err(rej) => return crate::EndpointError::from_json_rejection(&rej).into_response(),
    };
    let session = match crate::get_session_owned(&state, &principal, &session_id).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = crate::gate_agent_not_deleted(&state, &session.agent_id) {
        return response;
    }
    match state
        .session_admin()
        .set_session_workspace(
            &session_id,
            req.workspace_mount_id.as_deref(),
            req.cwd.as_deref(),
        )
        .await
    {
        Ok(Some((mount, cwd))) => (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({
                "ok": true,
                "sessionId": session_id,
                "workspaceMountId": mount,
                "cwd": cwd,
            })),
        )
            .into_response(),
        Ok(None) => crate::EndpointError::not_found().into_response(),
        Err(err) => crate::session_admin_error_response(err),
    }
}

/// R06 C1 工作台处置路由挂载入口：三条路由 + 与 build_router 同款的
/// transport_guard → auth_guard 鉴权链（transport_guard 补 ConnectionKind，
/// auth_guard 鉴别主体并注入 Principal；服务须以
/// `into_make_service_with_connect_info::<SocketAddr>()` 启动）。
/// 鉴权分类：两 POST 面与 PUT 面尚未进 auth.rs 的 classify_route 表，
/// 按未知形状 fail-closed 到 LocalOnly（本地属主可用，无凭证 401）；
/// 主控注册 lib.rs 时应在 sessions POST 静态名表中补
/// "workspace-disposal" | "sweep-orphaned-workspaces" 与 /workspace
/// PUT 条目（对齐现役 sessions.write → chat scope 口径）。
pub fn workspace_disposal_routes(state: crate::ServiceState) -> axum::Router {
    axum::Router::new()
        .route(
            "/lingxi/v1/sessions/workspace-disposal",
            axum::routing::post(workspace_disposal_route),
        )
        .route(
            "/lingxi/v1/sessions/sweep-orphaned-workspaces",
            axum::routing::post(sweep_orphaned_route),
        )
        .route(
            "/lingxi/v1/sessions/{session_id}/workspace",
            axum::routing::put(put_session_workspace_route),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth_guard,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::transport_guard,
        ))
        .with_state(state)
}
