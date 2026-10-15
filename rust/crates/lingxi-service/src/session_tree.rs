//! R06-T03 SessionService：会话树 / 分支 / fork / 重试 / 回退 / 检查点的
//! 服务编排层。挂在与现役同一条「传输守卫 → 认证 → 路由授权 → 资源归属」链之后
//! （资源归属闸在路由层经 `SessionStore::get_for` 执行，本层不重复）。
//!
//! 现役对照锚点（逐条亲验）：
//! - fork 入口与闸：`server/routes/sessions.ts:1768`（busy 409、深度 409、权限 403）。
//! - retry 入口：`server/routes/sessions.ts:1558`；目标解析
//!   `core/session-turn-actions.ts:78-165`（resolveSessionNodeTarget）。
//! - rewind 语义：`core/session-turn-actions.ts:189-344`（分支头回移 + reset 标记；
//!   checkpoint 目标与 retry 同套解析）。
//! - A06 冲突检测加固（报告差异 D1 + REPAIR-R1 FINDING-05 裁决）：现役盲写回
//!   `core/workspace-snapshots.ts:584-620`/`lib/checkpoint-store.ts:94-108`；
//!   本实现内容级回滚——无冲突文件真实写回存档字节，逐文件三档判定
//!   （restored/conflicted/skipped/failed），外部修改（含删除）的文件保留
//!   用户修改绝不覆盖，冲突不阻塞分支回移，收据逐文件如实标注。
//!
//! 服务端目标解析（RC-1 关键修复）：retry/rewind 的重置点**只信服务端解析**——
//! 客户端给 targetMessageId，服务端从当前分支消息链解析出重置点；客户端直接
//! 传 new_head 的口径被废止（否则客户端可伪造分支历史）。
//!
//! 事件契约：fork/rewind/retry 提交成功后经 EventHub 发布 `session_created` /
//! `session_branch_reset`（Unknown 透传，lingxi.wire v1 开放词汇）；提交失败
//! 不发布（提交后发布）。

use std::sync::Arc;

use lingxi_adapters::storage::session_operations::{self, Begin, Operation, Prepare};
use serde::{Deserialize, Serialize};

use lingxi_adapters::storage::session_tree as store;
use lingxi_adapters::storage::RunDatabase;
use lingxi_kernel::ports::StorageError;
use lingxi_kernel::session_tree as logic;

use crate::auth::Principal;
use crate::events::EventService;

/// 服务错误：每一支都是响亮的、调用方可见的结局，绝不静默降级为成功。
#[derive(Debug, Clone, PartialEq)]
pub enum SessionTreeError {
    NotFound,
    Forbidden,
    /// 会话忙（fork/retry/rewind 互斥，现役 session_busy → 409）。
    Busy,
    /// 谱系深度超限（现役 session_fork_depth_limit → 409）。
    ForkDepthLimit,
    /// fork 目标不在当前分支（现役 session_fork_target_invalid → 400）。
    ForkTargetInvalid,
    /// retry/rewind 目标解析失败（目标不在当前分支 / 无前置用户回合 → 400）。
    InvalidTarget {
        detail: String,
    },
    /// 检查点重名冲突（现役非 latest 冲突拒绝 → 409）。
    CheckpointConflict {
        name: String,
    },
    /// 会话 id 撞库（fork new_session_id / 创建会话；对照现役
    /// sessions.ts:587 active_session_conflict → 409；REPAIR-R1 观察项：
    /// 不再落 Internal 500）。
    /// 实际删除操作墓碑持有旧业务身份，明确冲突而不是永久忙。
    IdentityRetired {
        session_id: String,
    },
    SessionExists {
        session_id: String,
    },
    /// 回退偏好未开（现役 file_rollback_disabled → 403）。
    FileRollbackDisabled,
    /// 检查点文件路径越出会话授权目录（→ 403）。
    FileNotAuthorized {
        file_path: String,
    },
    Storage(StorageError),
}

impl From<StorageError> for SessionTreeError {
    fn from(e: StorageError) -> Self {
        match &e {
            StorageError::InvalidRequest { detail } if detail.contains("fork_depth_limit") => {
                SessionTreeError::ForkDepthLimit
            }
            StorageError::InvalidRequest { detail }
                if detail.contains("not on the current branch") =>
            {
                SessionTreeError::ForkTargetInvalid
            }
            StorageError::Conflict { detail }
                if detail.starts_with("session_identity_retired:") =>
            {
                SessionTreeError::IdentityRetired {
                    session_id: detail
                        .trim_start_matches("session_identity_retired:")
                        .to_string(),
                }
            }
            StorageError::Conflict { detail } if detail.starts_with("session_exists:") => {
                SessionTreeError::SessionExists {
                    session_id: detail.trim_start_matches("session_exists:").to_string(),
                }
            }
            StorageError::Conflict { detail } if detail.contains("checkpoint") => {
                SessionTreeError::CheckpointConflict {
                    name: detail.clone(),
                }
            }
            _ => SessionTreeError::Storage(e),
        }
    }
}

impl From<logic::ResolveTargetError> for SessionTreeError {
    fn from(e: logic::ResolveTargetError) -> Self {
        let detail = match e {
            logic::ResolveTargetError::NotOnBranch => {
                "requested session node is not on the active branch".to_string()
            }
            logic::ResolveTargetError::NoPrecedingUserTurn => {
                "assistant node has no preceding turn input on the active branch".to_string()
            }
            logic::ResolveTargetError::NoUserTurn => "no latest user message to replay".to_string(),
        };
        SessionTreeError::InvalidTarget { detail }
    }
}

/// fork 结果视图。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkView {
    pub session_id: String,
    pub parent_session_id: String,
    pub fork_point_message_id: String,
    pub lineage_depth: u32,
    pub permission_mode: Option<String>,
    pub authorized_folders: Vec<String>,
}

/// retry 的文件回退模式（现役 sessions.ts:1584-1607 契约：
/// 缺省/none = 不回退；workspace = 回滚工作区文件，需偏好开启否则 403）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRollbackMode {
    None,
    Workspace,
}

/// 内容级文件恢复报告（rewind 收据与 retry fileRollbackReport 共用形状——
/// REPAIR-R1 FINDING-05/02 同一套恢复机制）。逐文件如实归档：
/// restored=真实写回存档字节；conflicted=外部修改（含删除），保留用户文件；
/// skipped=已是目标态无需写回；failed=写回/校验 IO 失败，reason 标明可能已写入。
/// `ok` = 无冲突无失败且 reason 为空；`reason` 承载整体级说明
/// （如 retry 锚点无检查点 → "no_checkpoint"，对照现役 restoreTurn 同款）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRestoreReport {
    pub ok: bool,
    pub reason: Option<String>,
    pub restored: Vec<String>,
    pub conflicted: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<String>,
    #[serde(default)]
    pub recovery_records: Vec<serde_json::Value>,
}

impl FileRestoreReport {
    fn no_checkpoint() -> Self {
        Self {
            ok: false,
            reason: Some("no_checkpoint".to_string()),
            restored: Vec::new(),
            conflicted: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
            recovery_records: Vec::new(),
        }
    }
}

/// retry 结果视图（D6 两段式：本路由重置分支并返回回合输入，
/// 客户端随后经 execute 提交为全新 run——不覆盖旧结果）。
/// `file_rollback_report`：fileRollback=workspace 时的逐文件恢复报告
/// （对照现役 session-turn-actions.ts 的 fileRollbackReport；未请求时缺省）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryOutcome {
    pub session_id: String,
    pub turn_input_message_id: String,
    /// 回合输入消息内容（`{"text": ...}`），供客户端重新提交。
    pub turn_input_content_json: String,
    pub new_head_message_id: Option<String>,
    pub reset_marker_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_rollback_report: Option<FileRestoreReport>,
}

/// rewind 回退收据：逐文件如实列结局，外部副作用恒 `not_rolled_back`。
/// REPAIR-R1 FINDING-05（管理者裁决）：内容级回滚——无冲突文件真实写回
/// 存档内容；逐文件三档判定（restored/conflicted/skipped/failed），
/// 冲突文件保留用户修改，分支照常回移（设计稿 A06 原语义），绝不伪称全部撤销。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewindReceipt {
    pub session_id: String,
    pub checkpoint: String,
    pub turn_input_message_id: String,
    pub new_head_message_id: Option<String>,
    pub reset_marker_id: String,
    /// 真实写回存档内容的文件（当前内容是被系统见证过的版本）。
    pub restored: Vec<String>,
    /// 外部修改（含删除）而拒绝覆盖的文件——用户修改原样保留。
    pub conflicted: Vec<String>,
    /// 已在检查点状态、无需写回的文件。
    pub skipped: Vec<String>,
    /// 写回/校验 IO 失败的文件；可能已写入时保留明确原因和恢复记录。
    pub failed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_restore_reason: Option<String>,
    #[serde(default)]
    pub recovery_records: Vec<serde_json::Value>,
    /// 外部副作用（网络/bash）绝不回滚，也绝不伪称已回滚。
    pub external_effects: String,
}

/// rewind 预览（只读）：若执行将发生的分支重置点与逐文件判定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewindPreview {
    pub session_id: String,
    pub checkpoint: String,
    pub turn_input_message_id: String,
    pub new_head_message_id: Option<String>,
    pub files: Vec<FilePreview>,
}

/// 预览中的单文件判定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePreview {
    pub file_path: String,
    /// skipped = 当前与检查点一致（执行时不写回）；restored = 执行时将写回
    /// 存档内容；conflicted = 外部修改或已删除（执行时保留用户文件）。
    pub verdict: String,
}

/// rewind/retry 共用的内容级文件恢复（REPAIR-R1 FINDING-05/02 同一机制）：
/// 逐文件三档判定（kernel judge_file_restore），Restore 档真实写回存档字节
/// 并复读校验；Conflicted/失败绝不覆盖现状。返回逐文件如实报告。
async fn restore_checkpoint_files(
    storage: &RunDatabase,
    session_id: &str,
    checkpoint_id: &str,
    operation: &Operation,
    principal: &Principal,
    safety: Arc<crate::file_safety::FileSafety>,
    access: Option<Arc<crate::resourceaccess::ResourceAccess>>,
) -> Result<FileRestoreReport, SessionTreeError> {
    session_operations::arm_files(storage, session_id, operation).await?;
    let versions = store::list_file_versions(storage, checkpoint_id).await?;
    let contents = store::list_file_contents(storage, checkpoint_id).await?;
    let session = session_id.to_owned();
    let principal = crate::sessions::kernel_principal_of(principal);
    // 与授权变更共用数据库写队列：操作期间不会接受旧授权的并行更新。
    store::with_current_file_authorization(storage, session_id, move |roots| {
        let scope = crate::file_safety::witness_scope(
            principal.storage_kind(),
            &principal.storage_subject(),
            &session,
        );
        let mut report = FileRestoreReport {
            ok: true,
            reason: None,
            restored: vec![],
            conflicted: vec![],
            skipped: vec![],
            failed: vec![],
            recovery_records: vec![],
        };
        for v in versions {
            let path = std::path::Path::new(&v.file_path);
            let authorize = || {
                current_file_access(&roots, &principal, &session, path)
                    && shared_file_access(access.as_deref(), &principal, &session, path)
            };
            if !authorize() || safety.is_protected(path) {
                report.conflicted.push(v.file_path);
                continue;
            }
            let snapshot = match crate::file_safety::Snapshot::read(path) {
                Ok(s) => s,
                Err(_) => {
                    report.conflicted.push(v.file_path);
                    continue;
                }
            };
            let Some(content) = contents.iter().find(|c| c.file_path == v.file_path) else {
                report.conflicted.push(v.file_path);
                continue;
            };
            if content.content.len() as u64 != v.size_bytes as u64
                || logic::sha256_hex(&content.content) != v.sha256
            {
                report.failed.push(v.file_path);
                continue;
            }
            if logic::sha256_hex(&snapshot.bytes) == v.sha256 {
                report.skipped.push(v.file_path);
                continue;
            }
            if !safety.witnessed(&scope, path, &snapshot) {
                report.conflicted.push(v.file_path);
                continue;
            }
            let result = safety.replace(&scope, path, &snapshot, &content.content, authorize);
            if let Some(record) = safety.last_recovery(path) {
                report.recovery_records.push(record);
            }
            match result {
                Ok(()) => report.restored.push(v.file_path),
                Err(crate::file_safety::WriteError::Conflict) => {
                    report.conflicted.push(v.file_path)
                }
                Err(crate::file_safety::WriteError::Failed(_)) => report.failed.push(v.file_path),
                Err(crate::file_safety::WriteError::Unknown(_)) => {
                    report.reason = Some("write_may_have_occurred".into());
                    report.failed.push(v.file_path);
                }
            }
        }
        report.ok = report.conflicted.is_empty() && report.failed.is_empty();
        Ok(report)
    })
    .await
    .map_err(SessionTreeError::from)
}

fn current_file_access(
    roots: &[String],
    principal: &lingxi_kernel::Principal,
    session: &str,
    path: &std::path::Path,
) -> bool {
    let roots: Vec<std::path::PathBuf> = roots.iter().map(std::path::PathBuf::from).collect();
    // 授权根自身被换成链接也拒绝，不能重新解析后扩大原授权。
    if roots
        .iter()
        .any(|p| p.canonicalize().ok().as_deref() != Some(p.as_path()))
    {
        return false;
    }
    crate::resourceaccess::ResourceAccess::new(&roots)
        .ok()
        .and_then(|a| {
            a.authorize(
                principal.storage_kind(),
                &principal.storage_subject(),
                session,
                path,
                std::path::Path::new("/"),
                crate::resourceaccess::ResourceOp::Write,
            )
            .ok()
        })
        .map(|s| s.path == path)
        .unwrap_or(false)
}

fn shared_file_access(
    access: Option<&crate::resourceaccess::ResourceAccess>,
    principal: &lingxi_kernel::Principal,
    session: &str,
    path: &std::path::Path,
) -> bool {
    access
        .map(|a| {
            a.authorize(
                principal.storage_kind(),
                &principal.storage_subject(),
                session,
                path,
                std::path::Path::new("/"),
                crate::resourceaccess::ResourceOp::Write,
            )
            .map(|s| s.path == path)
            .unwrap_or(false)
        })
        .unwrap_or(true)
}

#[derive(Clone)]
pub struct SessionService {
    storage: Arc<RunDatabase>,
    events: Arc<EventService>,
    file_safety: Arc<crate::file_safety::FileSafety>,
    resource_access: Option<Arc<crate::resourceaccess::ResourceAccess>>,
    probe: Arc<std::sync::Mutex<Option<Arc<dyn SessionOperationProbe>>>>,
    session_files: Option<Arc<crate::sessionfiles::SessionFileService>>,
}

/// 故障/竞态观察点只暂停时序，不替换准入、存储或业务逻辑。
pub trait SessionOperationProbe: Send + Sync {
    fn reached<'a>(
        &'a self,
        kind: &'a str,
        sid: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;
}
/// rewind 请求（打包以控制参数个数）。
pub struct RewindRequest<'a> {
    pub checkpoint_name: &'a str,
    pub restore_files: bool,
    pub file_rollback_enabled: bool,
    pub principal: &'a Principal,
    pub request_id: Option<&'a str>,
    pub now_unix_ms: u64,
}

/// 当前分支的真实消息节点链（root→…→head；不含 reset 标记条目）。
async fn branch_nodes(
    db: &RunDatabase,
    session_id: &str,
) -> Result<Vec<(String, Option<String>, String)>, SessionTreeError> {
    let rows = store::list_branch_messages(db, session_id)
        .await
        .map_err(SessionTreeError::from)?;
    Ok(rows
        .into_iter()
        .filter(|m| m.entry_type == "message")
        .map(|m| (m.message_id, m.parent_message_id, m.role))
        .collect())
}

/// 把存储层消息链转成纯逻辑节点切片。
fn as_logic_nodes(rows: &[(String, Option<String>, String)]) -> Vec<logic::BranchNode<'_>> {
    rows.iter()
        .map(|(id, parent, role)| logic::BranchNode {
            message_id: id.as_str(),
            parent_message_id: parent.as_deref(),
            role: role.as_str(),
        })
        .collect()
}

/// 分支初始化在同一新 SID 操作权内准备，数据库提交失败时只撤回自己的预备资料。
struct PreparedForkUi {
    home: std::path::PathBuf,
    owner: String,
    session: String,
    metadata: bool,
    choice: bool,
}
impl PreparedForkUi {
    fn prepare(
        storage: &RunDatabase,
        home: &std::path::Path,
        owner: &str,
        source: &str,
        session: &str,
    ) -> Result<Self, StorageError> {
        let io = |detail| StorageError::Io { detail };
        if crate::session_ui::read(storage, session)?.is_some() {
            return Err(StorageError::Conflict {
                detail: format!("session_exists:{session}"),
            });
        }
        let choice = crate::chat_settings::choice(home, owner, source).map_err(io)?;
        let metadata = crate::session_ui::read(storage, source)?;
        let mut prepared = Self {
            home: home.into(),
            owner: owner.into(),
            session: session.into(),
            metadata: false,
            choice: false,
        };
        if let Some(mut metadata) = metadata {
            metadata.session_id = session.into();
            crate::session_ui::save(storage, &metadata)?;
            prepared.metadata = true;
        }
        if let Err(error) =
            crate::chat_settings::save_new_session_choice(home, owner, session, choice)
        {
            let cleanup = prepared.rollback(storage);
            return Err(io(format!(
                "分支设置初始化失败：{error}{}",
                cleanup.map_or_else(|error| format!("；清理失败：{error}"), |_| String::new())
            )));
        }
        prepared.choice = true;
        Ok(prepared)
    }
    fn rollback(self, storage: &RunDatabase) -> Result<(), String> {
        let mut errors = Vec::new();
        if self.choice {
            if let Err(error) = crate::chat_settings::remove_new_session_choice(
                &self.home,
                &self.owner,
                &self.session,
            ) {
                errors.push(error);
            }
        }
        if self.metadata {
            if let Err(error) = crate::session_ui::remove_uncreated(storage, &self.session) {
                errors.push(format!("{error:?}"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }
}

impl SessionService {
    pub fn new(storage: Arc<RunDatabase>, events: Arc<EventService>) -> Self {
        Self {
            storage: Arc::clone(&storage),
            events,
            session_files: None,
            file_safety: Arc::new(crate::file_safety::FileSafety::new(
                storage
                    .db_path()
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(".")),
            )),
            resource_access: None,
            probe: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn with_session_files(
        mut self,
        files: Arc<crate::sessionfiles::SessionFileService>,
    ) -> Self {
        self.session_files = Some(files);
        self
    }

    pub fn with_file_safety(
        mut self,
        safety: Arc<crate::file_safety::FileSafety>,
        access: Option<Arc<crate::resourceaccess::ResourceAccess>>,
    ) -> Self {
        self.file_safety = safety;
        self.resource_access = access;
        self
    }

    pub fn set_operation_probe(&self, probe: Option<Arc<dyn SessionOperationProbe>>) {
        *self.probe.lock().unwrap_or_else(|p| p.into_inner()) = probe;
    }
    async fn operation_observed(&self, kind: &str, sid: &str) {
        let probe = self.probe.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(probe) = probe {
            probe.reached(kind, sid).await;
        }
    }
    /// 先取得真实操作权，随后重读权限和生命周期；重复收据仍须当前归属允许。
    #[allow(clippy::too_many_arguments)]
    async fn begin_command(
        &self,
        principal: &Principal,
        sid: &str,
        ids: &[&str],
        kind: &str,
        request_id: Option<&str>,
        input: serde_json::Value,
        _file_effects: bool,
        now: u64,
        expected_review: Option<&serde_json::Value>,
    ) -> Result<(lingxi_kernel::session_admission::SessionPermit, Begin), SessionTreeError> {
        // 先按归属筛选，不向未授权主体泄漏忙状态；取得权后仍重读。
        let before = store::get_session_row(&self.storage, sid)
            .await?
            .ok_or(SessionTreeError::NotFound)?;
        if !crate::sessions::SessionStore::can_access(principal, &before.owner_user_id) {
            return Err(SessionTreeError::Forbidden);
        }
        let permit = self
            .storage
            .session_admission()
            .try_acquire(
                ids,
                crate::session_supervisor::SessionConcurrencyLimits::DEFAULT_REGISTRY_CAP,
            )
            .map_err(|_| SessionTreeError::Busy)?;
        let row = store::get_session_row(&self.storage, sid)
            .await?
            .ok_or(SessionTreeError::NotFound)?;
        if !crate::sessions::SessionStore::can_access(principal, &row.owner_user_id) {
            return Err(SessionTreeError::Forbidden);
        }
        if let Some(expected) = expected_review {
            let kernel = crate::sessions::kernel_principal_of(principal);
            let current = crate::sessiontools::review(
                &self.storage,
                &crate::toolgateway::ReviewInput {
                    agent_id: &row.agent_id,
                    session_id: sid,
                    principal_kind: kernel.storage_kind(),
                    principal_subject: &kernel.storage_subject(),
                },
                &input,
            )
            .map_err(|detail| SessionTreeError::InvalidTarget { detail })?;
            if &current != expected {
                return Err(SessionTreeError::InvalidTarget {
                    detail: "confirmed checkpoint changed; a fresh user confirmation is required"
                        .into(),
                });
            }
        }
        let request_id = request_id
            .map(crate::dedup::validate_request_id)
            .transpose()
            .map_err(|detail| SessionTreeError::InvalidTarget { detail })?;
        let mut random = [0u8; 16];
        getrandom::getrandom(&mut random).map_err(|e| {
            SessionTreeError::Storage(StorageError::Internal {
                detail: format!("operation identity: {e}"),
            })
        })?;
        let opid = format!(
            "session-op:{}",
            random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let kp = crate::sessions::kernel_principal_of(principal);
        let begin = session_operations::begin(
            &self.storage,
            Prepare {
                session_id: sid,
                owner_kind: kp.storage_kind(),
                owner_subject: &kp.storage_subject(),
                operation_id: &opid,
                request_id: request_id.as_deref(),
                kind,
                digest: &logic::sha256_hex(input.to_string().as_bytes()),
                file_effects: false,
                now,
            },
        )
        .await?;
        if matches!(&begin, Begin::Fresh(_)) {
            self.operation_observed(kind, sid).await;
        }
        Ok((permit, begin))
    }

    /// 操作任务拥有lease，客户端取消等待不能让已入队写入失去保护。
    #[allow(clippy::too_many_arguments)]
    pub async fn fork_session(
        &self,
        principal: &Principal,
        source: &str,
        new_id: &str,
        boundary: &str,
        request_id: Option<&str>,
        now: u64,
    ) -> Result<ForkView, SessionTreeError> {
        let (svc, p, source, new_id, boundary, rid) = (
            self.clone(),
            principal.clone(),
            source.to_string(),
            new_id.to_string(),
            boundary.to_string(),
            request_id.map(str::to_string),
        );
        tokio::spawn(async move {
            svc.fork_inner(&p, &source, &new_id, &boundary, rid.as_deref(), now)
                .await
        })
        .await
        .map_err(|e| {
            SessionTreeError::Storage(StorageError::Internal {
                detail: format!("fork operation task: {e}"),
            })
        })?
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn retry_turn(
        &self,
        principal: &Principal,
        sid: &str,
        target: Option<&str>,
        rollback: FileRollbackMode,
        enabled: bool,
        request_id: Option<&str>,
        now: u64,
    ) -> Result<RetryOutcome, SessionTreeError> {
        let (svc, p, sid, target, rid) = (
            self.clone(),
            principal.clone(),
            sid.to_string(),
            target.map(str::to_string),
            request_id.map(str::to_string),
        );
        tokio::spawn(async move {
            svc.retry_inner(
                &p,
                &sid,
                target.as_deref(),
                rollback,
                enabled,
                rid.as_deref(),
                now,
            )
            .await
        })
        .await
        .map_err(|e| {
            SessionTreeError::Storage(StorageError::Internal {
                detail: format!("retry operation task: {e}"),
            })
        })?
    }
    pub async fn rewind_to_checkpoint(
        &self,
        sid: &str,
        req: RewindRequest<'_>,
    ) -> Result<RewindReceipt, SessionTreeError> {
        self.rewind_owned(sid, req, None).await
    }

    pub(crate) async fn rewind_reviewed(
        &self,
        sid: &str,
        req: RewindRequest<'_>,
        review: &serde_json::Value,
    ) -> Result<RewindReceipt, SessionTreeError> {
        self.rewind_owned(sid, req, Some(review.clone())).await
    }

    async fn rewind_owned(
        &self,
        sid: &str,
        req: RewindRequest<'_>,
        review: Option<serde_json::Value>,
    ) -> Result<RewindReceipt, SessionTreeError> {
        let (svc, p, sid, checkpoint, rid) = (
            self.clone(),
            req.principal.clone(),
            sid.to_string(),
            req.checkpoint_name.to_string(),
            req.request_id.map(str::to_string),
        );
        let (restore_files, file_rollback_enabled, now_unix_ms) = (
            req.restore_files,
            req.file_rollback_enabled,
            req.now_unix_ms,
        );
        tokio::spawn(async move {
            svc.rewind_inner(
                &sid,
                RewindRequest {
                    principal: &p,
                    checkpoint_name: &checkpoint,
                    request_id: rid.as_deref(),
                    restore_files,
                    file_rollback_enabled,
                    now_unix_ms,
                },
                review,
            )
            .await
        })
        .await
        .map_err(|e| {
            SessionTreeError::Storage(StorageError::Internal {
                detail: format!("rewind operation task: {e}"),
            })
        })?
    }

    /// fork：busy/深度/目标闸 → 复制保留段 → 发布 `session_created`。
    /// 授权目录与权限档继承快照，绝不扩大（安全红线）。
    async fn fork_inner(
        &self,
        principal: &Principal,
        source_session_id: &str,
        new_session_id: &str,
        boundary_message_id: &str,
        request_id: Option<&str>,
        now_unix_ms: u64,
    ) -> Result<ForkView, SessionTreeError> {
        let (_permit,begin)=self.begin_command(principal,source_session_id,&[source_session_id,new_session_id],"fork",request_id,serde_json::json!({"newSessionId":new_session_id,"boundaryMessageId":boundary_message_id}),false,now_unix_ms,None).await?;
        let operation = match begin {
            Begin::Replay(value) => {
                return serde_json::from_value(value).map_err(|e| {
                    SessionTreeError::Storage(StorageError::Corrupted {
                        detail: e.to_string(),
                    })
                })
            }
            Begin::Fresh(op) => op,
        };
        let source = store::get_session_row(&self.storage, source_session_id)
            .await?
            .ok_or(SessionTreeError::NotFound)?;
        crate::model_history::ModelHistory::new(Arc::clone(&self.storage))
            .validate_fork_prefix(
                &crate::sessions::kernel_principal_of(principal),
                &source.agent_id,
                source_session_id,
                boundary_message_id,
            )
            .await?;
        if store::get_session_row(&self.storage, new_session_id)
            .await?
            .is_some()
        {
            return Err(SessionTreeError::SessionExists {
                session_id: new_session_id.into(),
            });
        }
        let proactive = self
            .session_files
            .as_ref()
            .map(|files| {
                crate::context_compiler::proactive_delegation_from_user_dir(
                    &files.home().join("user"),
                )
            })
            .transpose()
            .map_err(|error| {
                SessionTreeError::Storage(StorageError::Io {
                    detail: format!("new fork policy read failed: {error}"),
                })
            })?
            .unwrap_or(false);
        let prepared = self
            .session_files
            .as_ref()
            .map(|files| {
                PreparedForkUi::prepare(
                    &self.storage,
                    files.home(),
                    &source.owner_user_id,
                    source_session_id,
                    new_session_id,
                )
            })
            .transpose()?;
        let committed = store::fork_session_committed_with_files_and_policy(
            &self.storage,
            store::ForkRequest {
                source_session_id,
                new_session_id,
                boundary_message_id,
                now_unix_ms,
            },
            Some(operation),
            self.session_files
                .as_ref()
                .map(|files| files.home().to_path_buf()),
            proactive,
        )
        .await;
        let (outcome, envelope) = match committed {
            Ok(value) => value,
            Err(error) => {
                // 不确定是否已经落盘时保留初始化，不能抹掉可能成功会话的资料。
                match store::get_session_row(&self.storage, new_session_id).await {
                    Ok(None) => {
                        if let Some(prepared) = prepared {
                            if let Err(cleanup) = prepared.rollback(&self.storage) {
                                return Err(SessionTreeError::Storage(StorageError::Io {
                                    detail: format!(
                                        "分支提交失败：{error:?}；初始化清理失败：{cleanup}"
                                    ),
                                }));
                            }
                        }
                    }
                    _ => {
                        return Err(SessionTreeError::Storage(StorageError::Io {
                            detail: format!("分支提交结果待核对，初始化已保留：{error:?}"),
                        }))
                    }
                }
                return Err(error.into());
            }
        };
        self.events.publish_committed(&[envelope]);
        Ok(ForkView {
            session_id: outcome.session_id,
            parent_session_id: outcome.parent_session_id,
            fork_point_message_id: outcome.fork_point_message_id,
            lineage_depth: outcome.lineage_depth,
            permission_mode: outcome.permission_mode,
            authorized_folders: outcome.authorized_folders,
        })
    }

    /// retry：服务端从 `target_message_id` 解析重置点（user→该回合；
    /// assistant→其前方最近 user 回合；None→最近 user 回合，对照现役
    /// latestUserOnly），分支头回移 + reset 标记 + 发布 session_branch_reset。
    /// 回合输入内容随响应返回，由客户端经 execute 提交为全新 run（D6）。
    /// `file_rollback`（REPAIR-R1 FINDING-02，现役 sessions.ts:1584-1607 契约）：
    /// Workspace 且偏好未开 → 403 file_rollback_disabled；Workspace 时在分支
    /// 重置前对工作区做内容级回滚（与 rewind 同一恢复机制；锚点 = 回合输入
    /// 消息对应的检查点，无检查点 → no_checkpoint 报告且**不阻塞**分支重置，
    /// 对照现役 restoreTurn 的 no_checkpoint 与逐文件失败不阻塞 commit）。
    #[allow(clippy::too_many_arguments)]
    async fn retry_inner(
        &self,
        principal: &Principal,
        session_id: &str,
        target_message_id: Option<&str>,
        file_rollback: FileRollbackMode,
        file_rollback_enabled: bool,
        request_id: Option<&str>,
        now_unix_ms: u64,
    ) -> Result<RetryOutcome, SessionTreeError> {
        if file_rollback == FileRollbackMode::Workspace && !file_rollback_enabled {
            return Err(SessionTreeError::FileRollbackDisabled);
        }
        let (_permit,begin)=self.begin_command(principal,session_id,&[session_id],"retry",request_id,serde_json::json!({"targetMessageId":target_message_id,"fileRollback":format!("{file_rollback:?}")}),file_rollback==FileRollbackMode::Workspace,now_unix_ms,None).await?;
        let operation = match begin {
            Begin::Replay(value) => {
                return serde_json::from_value(value).map_err(|e| {
                    SessionTreeError::Storage(StorageError::Corrupted {
                        detail: e.to_string(),
                    })
                })
            }
            Begin::Fresh(op) => op,
        };
        let rows = store::list_branch_messages(&self.storage, session_id)
            .await
            .map_err(SessionTreeError::from)?;
        let nodes: Vec<(String, Option<String>, String)> = rows
            .iter()
            .filter(|m| m.entry_type == "message")
            .map(|m| {
                (
                    m.message_id.clone(),
                    m.parent_message_id.clone(),
                    m.role.clone(),
                )
            })
            .collect();
        let point = logic::resolve_retry_reset_point(&as_logic_nodes(&nodes), target_message_id)?;
        let content_json = rows
            .iter()
            .find(|m| m.message_id == point.turn_input_message_id)
            .map(|m| m.content_json.clone())
            .ok_or(SessionTreeError::NotFound)?;

        // 文件回退在分支重置之前执行（对照现役 session-turn-actions.ts:451-460：
        // performWorkspaceFileRollback 先于 commitRetryBranch）。
        let file_rollback_report = if file_rollback == FileRollbackMode::Workspace {
            let anchor = store::find_checkpoint_by_turn_input(
                &self.storage,
                session_id,
                &point.turn_input_message_id,
            )
            .await
            .map_err(SessionTreeError::from)?;
            let report = match anchor {
                Some(cp) => {
                    restore_checkpoint_files(
                        &self.storage,
                        session_id,
                        &cp.checkpoint_id,
                        &operation,
                        principal,
                        Arc::clone(&self.file_safety),
                        self.resource_access.clone(),
                    )
                    .await?
                }
                None => FileRestoreReport::no_checkpoint(),
            };
            Some(report)
        } else {
            None
        };

        let mut fields = serde_json::Map::new();
        fields.insert("sessionId".into(), session_id.into());
        fields.insert(
            "messageId".into(),
            match &point.new_head_message_id {
                Some(h) => h.clone().into(),
                None => serde_json::Value::Null,
            },
        );
        if let Some(report) = &file_rollback_report {
            // 对照现役 session_branch_reset 事件携带 fileRollbackReport。
            fields.insert(
                "fileRollbackReport".into(),
                serde_json::to_value(report).map_err(|e| {
                    SessionTreeError::Storage(StorageError::Internal {
                        detail: format!("serialize fileRollbackReport: {e}"),
                    })
                })?,
            );
        }
        let receipt = serde_json::json!({"sessionId":session_id,"turnInputMessageId":point.turn_input_message_id,"turnInputContentJson":content_json,"newHeadMessageId":point.new_head_message_id,"resetMarkerId":"","fileRollbackReport":file_rollback_report});
        let (marker_id, envelope) = store::reset_branch_head_committed(
            &self.storage,
            store::ResetBranchHead {
                session_id,
                new_head_message_id: point.new_head_message_id.as_deref(),
                reason: "retry",
                source_message_id: Some(&point.turn_input_message_id),
                now_unix_ms,
            },
            Some(operation),
            fields,
            receipt,
        )
        .await?;
        self.events.publish_committed(&[envelope]);
        Ok(RetryOutcome {
            session_id: session_id.to_string(),
            turn_input_message_id: point.turn_input_message_id,
            turn_input_content_json: content_json,
            new_head_message_id: point.new_head_message_id,
            reset_marker_id: marker_id,
            file_rollback_report,
        })
    }

    /// rewind：checkpoint 目标经与 retry 同套解析得出重置点；
    /// `restore_files` 需偏好已开（否则 403）。REPAIR-R1 FINDING-05 内容级回滚：
    /// 逐文件三档判定——当前已是目标态 → skipped；当前内容被系统见证过
    /// （如模型后续回合改过并被检查点记录）→ 真实写回存档字节（restored）；
    /// 系统从未见过的版本或文件被删（外部修改）→ conflicted，该文件保留
    /// 用户修改。冲突不阻塞分支回移（设计稿 A06 原语义），收据逐文件如实
    /// 标注，绝不伪称全部撤销；外部副作用恒 not_rolled_back。
    async fn rewind_inner(
        &self,
        session_id: &str,
        req: RewindRequest<'_>,
        expected_review: Option<serde_json::Value>,
    ) -> Result<RewindReceipt, SessionTreeError> {
        let RewindRequest {
            checkpoint_name,
            restore_files,
            file_rollback_enabled,
            principal,
            request_id,
            now_unix_ms,
        } = req;
        if restore_files && !file_rollback_enabled {
            return Err(SessionTreeError::FileRollbackDisabled);
        }
        let (_permit, begin) = self
            .begin_command(
                principal,
                session_id,
                &[session_id],
                "rewind",
                request_id,
                serde_json::json!({"checkpoint":checkpoint_name,"restoreFiles":restore_files}),
                restore_files,
                now_unix_ms,
                expected_review.as_ref(),
            )
            .await?;
        let operation = match begin {
            Begin::Replay(value) => {
                return serde_json::from_value(value).map_err(|e| {
                    SessionTreeError::Storage(StorageError::Corrupted {
                        detail: e.to_string(),
                    })
                })
            }
            Begin::Fresh(op) => op,
        };
        let cp = store::get_checkpoint(&self.storage, session_id, checkpoint_name)
            .await
            .map_err(SessionTreeError::from)?
            .ok_or(SessionTreeError::NotFound)?;

        // 目标解析：checkpoint 记录的目标消息经 retry 同套逻辑得出重置点。
        let nodes = branch_nodes(&self.storage, session_id).await?;
        let point =
            logic::resolve_retry_reset_point(&as_logic_nodes(&nodes), Some(&cp.target_message_id))?;

        // 内容级文件恢复（rewind/retry 共用机制）；restore_files=false 时
        // 只回分支不动文件。
        let file_report = if restore_files {
            restore_checkpoint_files(
                &self.storage,
                session_id,
                &cp.checkpoint_id,
                &operation,
                principal,
                Arc::clone(&self.file_safety),
                self.resource_access.clone(),
            )
            .await?
        } else {
            FileRestoreReport {
                ok: true,
                reason: None,
                restored: Vec::new(),
                conflicted: Vec::new(),
                skipped: Vec::new(),
                failed: Vec::new(),
                recovery_records: Vec::new(),
            }
        };

        self.operation_observed("rewind:after-files", session_id)
            .await;

        // 分支头回移 + reset 标记（同事务；旧历史不删）。文件冲突不阻塞
        // 分支回移——收据如实标注 conflicted，用户文件原样保留。
        // 提交成功后发布 session_branch_reset。
        let mut fields = serde_json::Map::new();
        fields.insert("sessionId".into(), session_id.into());
        fields.insert("checkpoint".into(), checkpoint_name.into());
        fields.insert(
            "messageId".into(),
            match &point.new_head_message_id {
                Some(h) => h.clone().into(),
                None => serde_json::Value::Null,
            },
        );
        let receipt = serde_json::json!({"sessionId":session_id,"checkpoint":checkpoint_name,"turnInputMessageId":point.turn_input_message_id,"newHeadMessageId":point.new_head_message_id,"resetMarkerId":"","restored":file_report.restored,"conflicted":file_report.conflicted,"skipped":file_report.skipped,"failed":file_report.failed,"fileRestoreReason":file_report.reason,"recoveryRecords":file_report.recovery_records,"externalEffects":"not_rolled_back"});
        let (marker_id, envelope) = store::reset_branch_head_committed(
            &self.storage,
            store::ResetBranchHead {
                session_id,
                new_head_message_id: point.new_head_message_id.as_deref(),
                reason: "checkpoint_rewind",
                source_message_id: Some(&point.turn_input_message_id),
                now_unix_ms,
            },
            Some(operation),
            fields,
            receipt,
        )
        .await?;
        self.events.publish_committed(&[envelope]);

        Ok(RewindReceipt {
            session_id: session_id.to_string(),
            checkpoint: checkpoint_name.to_string(),
            turn_input_message_id: point.turn_input_message_id,
            new_head_message_id: point.new_head_message_id,
            reset_marker_id: marker_id,
            restored: file_report.restored,
            conflicted: file_report.conflicted,
            skipped: file_report.skipped,
            failed: file_report.failed,
            file_restore_reason: file_report.reason,
            recovery_records: file_report.recovery_records,
            external_effects: "not_rolled_back".to_string(),
        })
    }

    /// rewind 预览（只读）：解析重置点并逐文件给出三档判定
    /// （skipped/restored/conflicted，与执行同口径），不写任何状态、
    /// 不发布任何事件。偏好闸与执行同口径（restore_files 需偏好已开）。
    pub async fn preview_rewind(
        &self,
        principal: &Principal,
        session_id: &str,
        checkpoint_name: &str,
        restore_files: bool,
        file_rollback_enabled: bool,
    ) -> Result<RewindPreview, SessionTreeError> {
        if restore_files && !file_rollback_enabled {
            return Err(SessionTreeError::FileRollbackDisabled);
        }
        let cp = store::get_checkpoint(&self.storage, session_id, checkpoint_name)
            .await
            .map_err(SessionTreeError::from)?
            .ok_or(SessionTreeError::NotFound)?;
        let nodes = branch_nodes(&self.storage, session_id).await?;
        let point =
            logic::resolve_retry_reset_point(&as_logic_nodes(&nodes), Some(&cp.target_message_id))?;
        let mut files = Vec::new();
        if restore_files {
            let versions = store::list_file_versions(&self.storage, &cp.checkpoint_id)
                .await
                .map_err(SessionTreeError::from)?;
            let contents = store::list_file_contents(&self.storage, &cp.checkpoint_id).await?;
            let session = store::get_session_row(&self.storage, session_id)
                .await?
                .ok_or(SessionTreeError::NotFound)?;
            let kernel = crate::sessions::kernel_principal_of(principal);
            for v in versions {
                let path = std::path::Path::new(&v.file_path);
                let verdict = if self.file_safety.is_protected(path)
                    || !current_file_access(&session.authorized_folders, &kernel, session_id, path)
                    || !shared_file_access(
                        self.resource_access.as_deref(),
                        &kernel,
                        session_id,
                        path,
                    ) {
                    "conflicted"
                } else {
                    match crate::file_safety::Snapshot::read(path) {
                        Ok(snapshot) => {
                            match contents.iter().find(|c| c.file_path == v.file_path) {
                                Some(content)
                                    if logic::sha256_hex(&content.content) == v.sha256
                                        && content.content.len() as u64 == v.size_bytes as u64 =>
                                {
                                    if logic::sha256_hex(&snapshot.bytes) == v.sha256 {
                                        "skipped"
                                    } else if self.file_safety.witnessed(
                                        &crate::file_safety::witness_scope(
                                            kernel.storage_kind(),
                                            &kernel.storage_subject(),
                                            session_id,
                                        ),
                                        path,
                                        &snapshot,
                                    ) {
                                        "restored"
                                    } else {
                                        "conflicted"
                                    }
                                }
                                _ => "failed",
                            }
                        }
                        Err(_) => "conflicted",
                    }
                };
                files.push(FilePreview {
                    file_path: v.file_path,
                    verdict: verdict.into(),
                });
            }
        }
        Ok(RewindPreview {
            session_id: session_id.to_string(),
            checkpoint: checkpoint_name.to_string(),
            turn_input_message_id: point.turn_input_message_id,
            new_head_message_id: point.new_head_message_id,
            files,
        })
    }

    /// 读取当前分支历史投影（含 reset 标记）。
    pub async fn branch_history(
        &self,
        session_id: &str,
    ) -> Result<Vec<store::MessageRow>, SessionTreeError> {
        store::list_branch_messages(&self.storage, session_id)
            .await
            .map_err(SessionTreeError::from)
    }

    /// 创建具名检查点（latest 覆盖/其余冲突/上限 200）。
    /// - `target_message_id` 必须在当前分支上（服务端校验，400 响亮失败）；
    /// - `file_paths` 逐条必须在会话授权目录之下（canonicalize 双侧后组件级
    ///   前缀判定，越界 403）；读得出内容才记录版本（读不出 400）。
    /// - `message_count` 服务端计算 = 分支链上目标位置（含）之前的真实消息数。
    /// - REPAIR-R1 FINDING-01/05/07：文件版本与内容字节随 upsert **同一写事务**
    ///   原子落库（latest 覆盖 = 子行随行替换，无残留无 PK 冲突）；
    ///   `turn_input_message_id` 回填为目标消息的回合输入（retry fileRollback
    ///   锚点，对照现役按 turnInputEntryId 取回合快照）。
    async fn create_checkpoint_inner(
        &self,
        principal: &Principal,
        session_id: &str,
        point: (&str, &str),
        file_paths: &[String],
        now_unix_ms: u64,
        inherited: Option<Arc<lingxi_kernel::session_admission::SessionPermit>>,
    ) -> Result<store::CheckpointRow, SessionTreeError> {
        let (name, target_message_id) = point;
        let row = store::get_session_row(&self.storage, session_id)
            .await?
            .ok_or(SessionTreeError::NotFound)?;
        if !crate::sessions::SessionStore::can_access(principal, &row.owner_user_id) {
            return Err(SessionTreeError::Forbidden);
        }
        // 工具只能继承可信当前 Run 的同一数据根会话门；HTTP 仍独立取得准入。
        let _permit = match inherited {
            Some(permit)
                if permit.belongs_to(self.storage.session_admission())
                    && permit.covers(session_id) =>
            {
                permit
            }
            Some(_) => return Err(SessionTreeError::Forbidden),
            None => Arc::new(
                self.storage
                    .session_admission()
                    .try_acquire(
                        &[session_id],
                        crate::session_supervisor::SessionConcurrencyLimits::DEFAULT_REGISTRY_CAP,
                    )
                    .map_err(|_| SessionTreeError::Busy)?,
            ),
        };
        let current = store::get_session_row(&self.storage, session_id)
            .await?
            .ok_or(SessionTreeError::NotFound)?;
        if !crate::sessions::SessionStore::can_access(principal, &current.owner_user_id) {
            return Err(SessionTreeError::Forbidden);
        }
        if current.lifecycle != "active" {
            return Err(SessionTreeError::Busy);
        }
        let nodes = branch_nodes(&self.storage, session_id).await?;
        let pos = nodes
            .iter()
            .position(|(id, _, _)| id == target_message_id)
            .ok_or(SessionTreeError::InvalidTarget {
                detail: format!(
                    "checkpoint target {target_message_id} is not on the active branch"
                ),
            })?;
        let message_count = (pos + 1) as u32;
        // 回合锚点：目标消息的回合输入（assistant 目标 → 前方最近 user 回合；
        // 解析失败不阻塞检查点创建——锚点仅影响 retry fileRollback 的命中）。
        let turn_input_message_id =
            logic::resolve_retry_reset_point(&as_logic_nodes(&nodes), Some(target_message_id))
                .ok()
                .map(|p| p.turn_input_message_id);

        // 文件版本记录：授权目录闸 + 内容哈希 + 内容字节（恢复存档）。
        let mut files: Vec<store::CheckpointFileSpec> = Vec::new();
        if !file_paths.is_empty() {
            let session = store::get_session_row(&self.storage, session_id)
                .await
                .map_err(SessionTreeError::from)?
                .ok_or(SessionTreeError::NotFound)?;
            // 双侧 canonicalize：授权根与文件路径都解析符号链接后再比。
            let mut canon_folders = Vec::new();
            for f in &session.authorized_folders {
                let canon = std::fs::canonicalize(f).map_err(|e| {
                    SessionTreeError::Storage(StorageError::InvalidRequest {
                        detail: format!("authorized folder {f} not resolvable: {e}"),
                    })
                })?;
                canon_folders.push(canon.to_string_lossy().to_string());
            }
            for path in file_paths {
                let canon =
                    std::fs::canonicalize(path).map_err(|e| SessionTreeError::InvalidTarget {
                        detail: format!("checkpoint file {path} not readable: {e}"),
                    })?;
                if !logic::path_within_folders(&canon, &canon_folders) {
                    return Err(SessionTreeError::FileNotAuthorized {
                        file_path: path.clone(),
                    });
                }
                let kernel = crate::sessions::kernel_principal_of(principal);
                if self.file_safety.is_protected(&canon)
                    || !current_file_access(
                        &session.authorized_folders,
                        &kernel,
                        session_id,
                        &canon,
                    )
                    || !shared_file_access(
                        self.resource_access.as_deref(),
                        &kernel,
                        session_id,
                        &canon,
                    )
                {
                    return Err(SessionTreeError::FileNotAuthorized {
                        file_path: path.clone(),
                    });
                }
                let bytes = crate::file_safety::Snapshot::read(&canon)
                    .map(|s| s.bytes)
                    .map_err(|e| SessionTreeError::InvalidTarget {
                        detail: format!("checkpoint file {path} not readable: {e}"),
                    })?;
                // 对照现役 checkpoint-store 的 utf-8/base64 双态标记；
                // BLOB 始终存原始字节，该标记仅描述内容形态。
                let encoding = if std::str::from_utf8(&bytes).is_ok() {
                    "utf-8"
                } else {
                    "base64"
                };
                files.push(store::CheckpointFileSpec {
                    file_path: canon.to_string_lossy().to_string(),
                    sha256: logic::sha256_hex(&bytes),
                    size_bytes: bytes.len() as u64,
                    content: bytes,
                    encoding: encoding.to_string(),
                });
            }
        }

        store::upsert_checkpoint(
            &self.storage,
            store::UpsertCheckpoint {
                session_id,
                name,
                target_message_id,
                turn_input_message_id: turn_input_message_id.as_deref(),
                message_count,
                now_unix_ms,
                files,
            },
        )
        .await
        .map_err(SessionTreeError::from)
    }

    pub async fn create_checkpoint(
        &self,
        principal: &Principal,
        sid: &str,
        name: &str,
        target: &str,
        files: &[String],
        now: u64,
    ) -> Result<store::CheckpointRow, SessionTreeError> {
        let (svc, principal, sid, name, target, files) = (
            self.clone(),
            principal.clone(),
            sid.to_string(),
            name.to_string(),
            target.to_string(),
            files.to_vec(),
        );
        tokio::spawn(async move {
            svc.create_checkpoint_inner(&principal, &sid, (&name, &target), &files, now, None)
                .await
        })
        .await
        .map_err(|e| {
            SessionTreeError::Storage(StorageError::Internal {
                detail: e.to_string(),
            })
        })?
    }
    /// 当前工具由监督器核对 Run 身份并继承既有会话许可，不开放 HTTP 免锁入口。
    pub(crate) async fn tool_create_checkpoint(
        &self,
        principal: &Principal,
        ctx: &lingxi_kernel::RunContext,
        tasks: &crate::task_supervisor::TaskSupervisor,
        name: &str,
        files: &[String],
        now: u64,
    ) -> Result<store::CheckpointRow, SessionTreeError> {
        let row = store::get_session_row(&self.storage, ctx.session_id.as_str())
            .await?
            .ok_or(SessionTreeError::NotFound)?;
        let permit = tasks
            .parent_run_permit(
                ctx.run_id.as_str(),
                ctx.session_id.as_str(),
                &row.agent_id,
                &ctx.principal,
                self.storage.session_admission(),
            )?
            .ok_or(SessionTreeError::Forbidden)?;
        let nodes = branch_nodes(&self.storage, ctx.session_id.as_str()).await?;
        let target = nodes
            .iter()
            .rev()
            .find(|(_, _, role)| role == "user")
            .map(|(id, _, _)| id.clone())
            .ok_or_else(|| SessionTreeError::InvalidTarget {
                detail: "no user turn on the active branch".into(),
            })?;
        self.create_checkpoint_inner(
            principal,
            ctx.session_id.as_str(),
            (name, &target),
            files,
            now,
            Some(permit),
        )
        .await
    }

    pub(crate) async fn tool_drop_checkpoint(
        &self,
        ctx: &lingxi_kernel::RunContext,
        tasks: &crate::task_supervisor::TaskSupervisor,
        name: &str,
    ) -> Result<bool, SessionTreeError> {
        let row = store::get_session_row(&self.storage, ctx.session_id.as_str())
            .await?
            .ok_or(SessionTreeError::NotFound)?;
        let _permit = tasks
            .parent_run_permit(
                ctx.run_id.as_str(),
                ctx.session_id.as_str(),
                &row.agent_id,
                &ctx.principal,
                self.storage.session_admission(),
            )?
            .ok_or(SessionTreeError::Forbidden)?;
        store::delete_checkpoint(&self.storage, ctx.session_id.as_str(), name)
            .await
            .map_err(SessionTreeError::from)
    }

    pub async fn delete_checkpoint(&self, sid: &str, name: &str) -> Result<bool, SessionTreeError> {
        let (svc, sid, name) = (self.clone(), sid.to_string(), name.to_string());
        tokio::spawn(async move { svc.delete_checkpoint_inner(&sid, &name).await })
            .await
            .map_err(|e| {
                SessionTreeError::Storage(StorageError::Internal {
                    detail: e.to_string(),
                })
            })?
    }
    /// 列出会话的具名检查点（创建时间升序）。
    pub async fn list_checkpoints(
        &self,
        session_id: &str,
    ) -> Result<Vec<store::CheckpointRow>, SessionTreeError> {
        store::list_checkpoints(&self.storage, session_id)
            .await
            .map_err(SessionTreeError::from)
    }

    /// 删除具名检查点（不存在 → false → 路由 404）。
    async fn delete_checkpoint_inner(
        &self,
        session_id: &str,
        name: &str,
    ) -> Result<bool, SessionTreeError> {
        let _permit = self
            .storage
            .session_admission()
            .try_acquire(
                &[session_id],
                crate::session_supervisor::SessionConcurrencyLimits::DEFAULT_REGISTRY_CAP,
            )
            .map_err(|_| SessionTreeError::Busy)?;
        store::delete_checkpoint(&self.storage, session_id, name)
            .await
            .map_err(SessionTreeError::from)
    }
}

/// R06-T03 用户消息落盘器：把 user-origin run 的输入写进消息树
/// （parent = 当前分支头；content_json 为 `{"text": ...}`）。挂在
/// `RunSupervisor::drive_run` 的 record_run_started 提交之后。
pub struct DbUserMessageRecorder {
    storage: Arc<RunDatabase>,
}

impl DbUserMessageRecorder {
    pub fn new(storage: Arc<RunDatabase>) -> Self {
        Self { storage }
    }
}

impl crate::runs::UserMessageRecorder for DbUserMessageRecorder {
    fn record_user_message<'a>(
        &'a self,
        session_id: &'a str,
        run_id: &'a str,
        input: &'a str,
        now: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StorageError>> + Send + 'a>>
    {
        self.record_user_message_with_chat(session_id, run_id, input, None, now)
    }
    fn record_user_message_with_chat<'a>(
        &'a self,
        session_id: &'a str,
        run_id: &'a str,
        input: &'a str,
        chat: Option<&'a crate::runs::ChatRunInput>,
        now_unix_ms: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StorageError>> + Send + 'a>>
    {
        Box::pin(async move {
            // 当前分支头作为 parent（保持 session/message 稳定 ID 的链式结构）。
            let parent = store::list_branch_messages(&self.storage, session_id)
                .await?
                .last()
                .map(|m| m.message_id.clone());
            let mut content = serde_json::json!({"text":chat.and_then(|value|value.display_text.as_deref()).unwrap_or(input)});
            if let Some(chat) = chat {
                if content["text"] != input {
                    content["modelText"] = serde_json::json!(input);
                }
                if !chat.images.is_empty() {
                    content["images"] = crate::model_history::encode_images(&chat.images);
                }
                if !chat.files.is_empty() {
                    content["attachments"] = serde_json::json!(chat.files);
                }
                if let Some(id) = &chat.client_message_id {
                    content["clientMessageId"] = serde_json::json!(id);
                }
                if let Some(version) = chat.snapshot_version {
                    content["snapshotVersion"] = serde_json::json!(version);
                }
            }
            let content_json = content.to_string();
            // run_id = NULL：用户输入消息不归属任何 run 行（v8 run_id 可空但
            // 保 REFERENCES——REPAIR-R1 FINDING-06）；run 归属经消息 id
            // `user:{run_id}` 保持可追溯，裸 run_id 证据查询
            // （`WHERE run_id=?`）也不会把用户输入误读为 run 的最终消息。
            store::append_message(
                &self.storage,
                store::AppendMessage {
                    session_id,
                    message_id: &format!("user:{run_id}"),
                    parent_message_id: parent.as_deref(),
                    run_id: None,
                    role: "user",
                    content_json: &content_json,
                    entry_type: "message",
                    model_call_id: None,
                    now_unix_ms,
                },
            )
            .await
        })
    }
}
