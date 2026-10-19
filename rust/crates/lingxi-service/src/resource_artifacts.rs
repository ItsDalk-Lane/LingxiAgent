//! 办公产物的内部字节入口。作为 resourceio 的子模块接线，不扩大普通 JSON/文字预算。
//! 预检只固定事实，不能当成权限；恢复时只能读取正式任务保存的原事实。
use super::*;
use crate::file_safety::{Snapshot, WriteError};
use crate::filetools::fsio::DirFd;
use serde::{Deserialize, Serialize};

const ARTIFACT_LIMIT: u64 = 50 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct ArtifactSnapshot {
    pub file_path: PathBuf,
    pub bytes: Vec<u8>,
    pub meta: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum FrozenResource {
    LocalFile {
        path: String,
    },
    Mount {
        #[serde(rename = "mountId")]
        mount_id: String,
        path: String,
    },
}
impl FrozenResource {
    fn from_ref(reference: &ResourceRef) -> Result<Self, ResourceIoError> {
        match reference {
            ResourceRef::LocalFile { path } => Ok(Self::LocalFile { path: path.clone() }),
            ResourceRef::Mount { mount_id, path } => Ok(Self::Mount {
                mount_id: mount_id.clone(),
                path: path.clone(),
            }),
            other => Err(ResourceIoError::capability_denied(
                "artifact_write",
                provider_id_for_ref(other),
            )),
        }
    }
    fn reference(&self) -> ResourceRef {
        match self {
            Self::LocalFile { path } => ResourceRef::LocalFile { path: path.clone() },
            Self::Mount { mount_id, path } => ResourceRef::Mount {
                mount_id: mount_id.clone(),
                path: path.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FrozenScope {
    principal_kind: String,
    principal_subject: String,
    session_id: String,
}
impl FrozenScope {
    fn of(ctx: &OpContext) -> Self {
        Self {
            principal_kind: ctx.principal_kind.into(),
            principal_subject: ctx.principal_subject.clone(),
            session_id: ctx.session_id.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FrozenDirectory {
    path: PathBuf,
    device: u64,
    /// 字符串保留平台完整 inode，不能经过 JS 浮点数截断。
    inode: String,
}
impl FrozenDirectory {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn read(path: &Path, held: &DirFd) -> Result<Self, ResourceIoError> {
        let (device, inode) = held
            .directory_identity()
            .map_err(|e| conflict(format!("目录身份不可确认：{e}")))?;
        if !held.same_directory(path).unwrap_or(false) {
            return Err(conflict("目录在预检期间改变"));
        }
        Ok(Self {
            path: path.into(),
            device,
            inode: inode.to_string(),
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn read(_path: &Path, _held: &DirFd) -> Result<Self, ResourceIoError> {
        Err(ResourceIoError::new(
            501,
            "artifact_directory_identity_unavailable",
            "当前平台缺少安全目录身份能力",
        ))
    }
    fn open(&self) -> Result<DirFd, ResourceIoError> {
        let held = DirFd::open(&self.path).map_err(|e| conflict(format!("原目录不可用：{e}")))?;
        if &Self::read(&self.path, &held)? != self {
            return Err(conflict("原目录已被替换"));
        }
        Ok(held)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrozenStamp {
    size: u64,
    modified: String,
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dev: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ino: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ctime: Option<[i64; 2]>,
}
impl FrozenStamp {
    fn read(value: &Value) -> Result<Self, ResourceIoError> {
        let stamp: Self =
            serde_json::from_value(value.clone()).map_err(|_| conflict("产物版本事实不完整"))?;
        if stamp.size > ARTIFACT_LIMIT
            || stamp.modified.is_empty()
            || stamp.sha256.len() != 64
            || !stamp.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || cfg!(unix) && (stamp.dev.is_none() || stamp.ino.is_none() || stamp.ctime.is_none())
        {
            return Err(conflict("产物版本事实无效"));
        }
        Ok(stamp)
    }
    fn value(&self) -> Result<Value, ResourceIoError> {
        serde_json::to_value(self).map_err(|e| ResourceIoError::internal(e.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum FrozenTarget {
    Missing,
    Existing { stamp: FrozenStamp },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArtifactWriteFacts {
    schema_version: u8,
    scope: FrozenScope,
    reference: FrozenResource,
    resource_key: String,
    file_path: PathBuf,
    overwrite: bool,
    target: FrozenTarget,
    ancestor: FrozenDirectory,
    missing_parents: Vec<String>,
    mount_root: Option<FrozenDirectory>,
}

/// 字段私有且无 Deserialize：模型/HTTP 不能把请求正文当成通过的预检。
#[derive(Clone, Debug)]
pub(crate) struct ArtifactWritePreflight {
    facts: ArtifactWriteFacts,
}
impl ArtifactWritePreflight {
    pub(crate) fn file_path(&self) -> &Path {
        &self.facts.file_path
    }

    /// 专用后台会话受理之前重核原动作。只允许实际会话号变化，原主体、
    /// 挂载、原文件版本和目录身份必须逐字一致，不能在付费后换新版本。
    pub(crate) fn require_same_target(&self, original: &Self) -> Result<(), ResourceIoError> {
        let mut current = self.facts.clone();
        current.scope.session_id = original.facts.scope.session_id.clone();
        if current == original.facts {
            Ok(())
        } else {
            Err(conflict("后台受理时原目标或版本已改变"))
        }
    }

    /// 仅由正式后台任务存储保存；不保存目录句柄，也不保存“允许写入”的授权。
    pub(crate) fn persistent_facts(&self) -> Result<Value, ResourceIoError> {
        serde_json::to_value(&self.facts).map_err(|e| ResourceIoError::internal(e.to_string()))
    }

    /// 调用方必须从真实任务记录重读原值；禁止转发模型或 HTTP 提供的 facts。
    /// 这里只恢复版本事实；publish 仍重核当前权限、主体和目录/文件身份。
    pub(crate) fn from_trusted_persistent_facts(value: &Value) -> Result<Self, ResourceIoError> {
        let facts: ArtifactWriteFacts =
            serde_json::from_value(value.clone()).map_err(|_| conflict("持久产物预检记录无效"))?;
        if facts.schema_version != 1
            || facts.scope.principal_kind.is_empty()
            || facts.scope.principal_subject.is_empty()
            || facts.resource_key != resource_key_for_ref(&facts.reference.reference())
            || !normal_absolute(&facts.file_path)
            || !normal_absolute(&facts.ancestor.path)
            || facts.ancestor.inode.parse::<u128>().is_err()
            || facts.missing_parents.iter().any(|s| !normal_name(s))
            || matches!(facts.reference, FrozenResource::Mount { .. }) != facts.mount_root.is_some()
        {
            return Err(conflict("持久产物预检身份不完整"));
        }
        let mut parent = facts.ancestor.path.clone();
        for name in &facts.missing_parents {
            parent.push(name);
        }
        if facts.file_path.parent() != Some(parent.as_path()) {
            return Err(conflict("持久产物预检父目录不匹配"));
        }
        if let FrozenTarget::Existing { stamp } = &facts.target {
            FrozenStamp::read(&stamp.value()?)?;
            if !facts.overwrite || !facts.missing_parents.is_empty() {
                return Err(conflict("持久产物覆盖记录不一致"));
            }
        }
        if let Some(root) = &facts.mount_root {
            if !normal_absolute(&root.path)
                || root.inode.parse::<u128>().is_err()
                || !facts.file_path.starts_with(&root.path)
            {
                return Err(conflict("持久挂载根事实无效"));
            }
        }
        Ok(Self { facts })
    }

    /// 文档改写必须对应此前读到的原正文，不能在渲染/付费后采纳一个更新版本。
    pub(crate) fn require_snapshot(&self, stamp: &Value) -> Result<(), ResourceIoError> {
        match &self.facts.target {
            FrozenTarget::Existing { stamp: expected }
                if expected == &FrozenStamp::read(stamp)? =>
            {
                Ok(())
            }
            _ => Err(conflict("文档在读取后、预检前已改变")),
        }
    }
}

fn conflict(message: impl Into<String>) -> ResourceIoError {
    ResourceIoError::new(409, "artifact_version_conflict", message)
}
fn normal_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|c| {
            matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
}
fn normal_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', '\0']) && name != "." && name != ".."
}
fn cancelled(cancel: &Option<crate::cancel::CancelScope>) -> Result<(), ResourceIoError> {
    if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
        Err(ResourceIoError::new(
            409,
            "resource_cancelled",
            "产物操作已取消",
        ))
    } else {
        Ok(())
    }
}

fn check_reference_session(input: &Value, ctx: &OpContext) -> Result<(), ResourceIoError> {
    if input
        .get("sessionId")
        .is_some_and(|id| !id.is_null() && id.as_str() != Some(ctx.session_id.as_str()))
    {
        return Err(ResourceIoError::new(
            403,
            "resource_access_denied",
            "资源引用声明了其他会话",
        ));
    }
    for key in ["resource", "ref", "target"] {
        if let Some(nested) = input.get(key).filter(|v| !v.is_null()) {
            if nested.is_object() {
                return check_reference_session(nested, ctx);
            }
            break;
        }
    }
    Ok(())
}
fn read_error(error: WriteError) -> ResourceIoError {
    match error {
        WriteError::Failed(ref message) if message == crate::file_safety::READ_BOUND_EXCEEDED => {
            ResourceIoError::new(413, "artifact_read_too_large", "办公输入超过50MiB")
        }
        WriteError::Conflict => conflict("读取时文件身份或版本改变"),
        other => ResourceIoError::new(
            400,
            "artifact_read_failed",
            format!("办公输入读取失败：{other}"),
        ),
    }
}
fn write_error(error: WriteError) -> ResourceIoError {
    match error {
        WriteError::Conflict => conflict("产物目标改变或写入权限已撤销"),
        WriteError::Failed(message) => ResourceIoError::new(400, "artifact_write_failed", message),
        WriteError::Unknown(message) => {
            ResourceIoError::new(409, "artifact_write_unknown", message)
        }
    }
}

/// 只读寻找最近存在的父目录；预检绝不创建缺失目录。
fn parent_fact(path: &Path) -> Result<(FrozenDirectory, Vec<String>), ResourceIoError> {
    let parent = path.parent().ok_or_else(|| conflict("产物没有父目录"))?;
    let mut ancestor = parent.to_path_buf();
    let mut missing = Vec::new();
    let held = loop {
        match DirFd::open(&ancestor) {
            Ok(held) => break held,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = ancestor
                    .file_name()
                    .and_then(|s| s.to_str())
                    .filter(|s| normal_name(s))
                    .ok_or_else(|| conflict("缺失父目录名称无效"))?;
                missing.push(name.to_owned());
                if !ancestor.pop() {
                    return Err(conflict("没有可固定的父目录"));
                }
            }
            Err(error) => return Err(conflict(format!("产物父目录不可固定：{error}"))),
        }
    };
    missing.reverse();
    Ok((FrozenDirectory::read(&ancestor, &held)?, missing))
}

fn metadata(path: &Path, snapshot: &Snapshot) -> Value {
    serde_json::json!({
        "resourceKey":local_key(path),"resource":WorkspacePlane::descriptor(path),
        "exists":true,"isDirectory":false,"filePath":path,
        "version":read_snapshot_version(snapshot),"artifactStamp":snapshot.stamp,
    })
}

impl ResourceIoService {
    async fn validate_media_reader(
        &self,
        ctx: &lingxi_kernel::RunContext,
        runs: &crate::runs::RunSupervisor,
        cancel: &crate::cancel::CancelScope,
    ) -> Result<(), ResourceIoError> {
        let denied = || {
            ResourceIoError::new(
                409,
                "media_reference_authority_unavailable",
                "媒体参考图读取缺少当前真实执行身份",
            )
        };
        let db = runs.owned_context_storage().ok_or_else(denied)?;
        if !Arc::ptr_eq(
            db.session_admission(),
            &self.inner.session_files.admission_gate(),
        ) || cancel.is_cancelled()
            || runs
                .run_scope(ctx.run_id.as_str())
                .is_none_or(|actual| !actual.same_scope(cancel))
        {
            return Err(denied());
        }
        let actual = db.query_one_text(
            "SELECT m.operation_id FROM media_jobs_v1 m JOIN runs r ON r.run_id=m.execution_run_id JOIN sessions s ON s.session_id=r.session_id JOIN run_lineage l ON l.run_id=r.run_id JOIN run_attempts a ON a.run_id=r.run_id JOIN runs p ON p.run_id=m.source_run_id WHERE r.run_id=?1 AND r.session_id=?2 AND m.session_id=r.session_id AND r.owner_kind=?3 AND r.owner_subject=?4 AND r.owner_kind=p.owner_kind AND r.owner_subject=p.owner_subject AND r.generation=?5 AND a.attempt=?6 AND a.generation=r.generation AND a.attempt=(SELECT attempt FROM run_attempts WHERE run_id=r.run_id ORDER BY started_at_unix_ms DESC,rowid DESC LIMIT 1) AND r.status='running' AND s.lifecycle='active' AND s.owner_user_id=m.owner_subject AND l.origin='media' AND l.parent_run_id=m.source_run_id AND l.cause_id=m.operation_id AND l.source_message_id=m.tool_call_id AND m.phase IN('submitting','submitted','generating','downloading')",
            vec![ctx.run_id.to_string(), ctx.session_id.to_string(), ctx.principal.storage_kind().into(), ctx.principal.storage_subject(), ctx.generation.to_string(), ctx.attempt.to_string()],
        ).await.map_err(|_| denied())?;
        if actual.is_none() {
            return Err(denied());
        }
        Ok(())
    }

    /// 固定媒体Run仅可读取本会话参考附件；不授本机路径、挂载或通用工具上下文。
    pub(crate) async fn read_media_reference_snapshot(
        &self,
        input: &Value,
        ctx: &lingxi_kernel::RunContext,
        runs: &crate::runs::RunSupervisor,
        cancel: &crate::cancel::CancelScope,
    ) -> Result<ArtifactSnapshot, ResourceIoError> {
        self.validate_media_reader(ctx, runs, cancel).await?;
        let op = OpContext {
            principal_kind: ctx.principal.storage_kind(),
            principal_subject: ctx.principal.storage_subject(),
            session_id: ctx.session_id.to_string(),
            source: "media_reference".into(),
            reason: None,
            session_path: None,
            emit: false,
            capture_origin: None,
            engine_plane: false,
        };
        check_reference_session(input, &op)?;
        let file_id = match normalize_resource_ref(input)? {
            ResourceRef::SessionFile {
                file_id,
                session_id,
            } if session_id
                .as_deref()
                .is_none_or(|s| s == ctx.session_id.as_str()) =>
            {
                file_id
            }
            ResourceRef::Resource { resource_id } => {
                crate::resources::file_id_from_session_file_resource_id(&resource_id)
                    .ok_or_else(|| {
                        ResourceIoError::capability_denied("media_reference", "resource")
                    })?
                    .to_owned()
            }
            other => {
                return Err(ResourceIoError::capability_denied(
                    "media_reference",
                    provider_id_for_ref(&other),
                ))
            }
        };
        let lease = Arc::new(self.inner.session_files.payload_lease().await);
        let file = self
            .inner
            .session_files
            .get_file(ctx.session_id.as_str(), &file_id)
            .await
            .map_err(|e| ResourceIoError::internal(e.to_string()))?
            .filter(|f| f.status == "available" && !f.is_directory)
            .ok_or_else(|| {
                ResourceIoError::new(403, "resource_access_denied", "媒体参考图不属于当前会话")
            })?;
        let reference = ResourceRef::SessionFile {
            file_id: file.file_id.clone(),
            session_id: Some(ctx.session_id.to_string()),
        };
        let (key, file, path) = self.resolve_session_file(&reference).await?;
        let path = PathBuf::from(path);
        let read_path = path.clone();
        let observer = self.inner.session_files.payload_read_observer();
        let thread_cancel = cancel.clone();
        let thread_lease = lease.clone();
        let child =
            runs.task_supervisor()
                .spawn_blocking_owned(
                    Some(ctx.run_id.to_string()),
                    format!("media_reference:{}", file.file_id),
                    move || {
                        let _lease = thread_lease;
                        if thread_cancel.is_cancelled() {
                            return Err(conflict("媒体参考图读取已取消"));
                        }
                        if let Some(observer) = observer {
                            observer();
                        }
                        let snapshot = Snapshot::read_bounded(&read_path, 20 * 1024 * 1024)
                            .map_err(|error| match error {
                                WriteError::Failed(ref message)
                                    if message == crate::file_safety::READ_BOUND_EXCEEDED =>
                                {
                                    ResourceIoError::new(
                                        413,
                                        "media_reference_too_large",
                                        "媒体参考图超过20MiB",
                                    )
                                }
                                other => read_error(other),
                            })?;
                        if thread_cancel.is_cancelled() || !snapshot.path_is_current(&read_path) {
                            return Err(conflict("媒体参考图读取被取消或版本改变"));
                        }
                        Ok(snapshot)
                    },
                )
                .map_err(|_| {
                    ResourceIoError::new(
                        503,
                        "media_reference_capacity",
                        "媒体参考图读取监督额度不足",
                    )
                })?;
        let snapshot = child.wait().await.map_err(|exit| {
            ResourceIoError::new(
                409,
                "media_reference_read_interrupted",
                format!("媒体参考图读取已退出：{}", exit.name()),
            )
        })??;
        self.validate_media_reader(ctx, runs, cancel).await?;
        let latest = self
            .inner
            .session_files
            .get_file(ctx.session_id.as_str(), &file.file_id)
            .await
            .map_err(|e| ResourceIoError::internal(e.to_string()))?
            .ok_or_else(|| conflict("媒体参考图登记已撤销"))?;
        if latest.status != "available"
            || latest.file_path != file.file_path
            || latest.real_path != file.real_path
            || latest.is_directory
            || !snapshot.path_is_current(&path)
        {
            return Err(conflict("媒体参考图登记或版本已改变"));
        }
        let mut meta = metadata(&path, &snapshot);
        meta["resourceKey"] = Value::String(key);
        meta["resource"] =
            Self::session_file_descriptor(&reference, &latest, &path.to_string_lossy());
        drop(lease);
        Ok(ArtifactSnapshot {
            file_path: path,
            bytes: snapshot.bytes,
            meta,
        })
    }

    /// 专用50MiB快照；普通 read/JSON 的20MiB限制保持原样。
    pub(crate) async fn read_artifact_snapshot(
        &self,
        input: &Value,
        ctx: &OpContext,
    ) -> Result<ArtifactSnapshot, ResourceIoError> {
        check_reference_session(input, ctx)?;
        let mut reference = normalize_resource_ref(input)?;
        let retained = self.blocking_permit(ctx)?;
        let cancel = self.operation_cancel(ctx)?;
        cancelled(&cancel)?;
        // 同一把租约同时覆盖阻塞读取和读后登记核验；取消等待不能先释放实际读线程的租约。
        let lease = Arc::new(self.inner.session_files.payload_lease().await);
        if let ResourceRef::Resource { resource_id } = &reference {
            let file_id = crate::resources::file_id_from_session_file_resource_id(resource_id)
                .ok_or_else(|| ResourceIoError::capability_denied("artifact_read", "resource"))?;
            reference = ResourceRef::SessionFile {
                file_id: file_id.into(),
                session_id: Some(ctx.session_id.clone()),
            };
        }
        match reference {
            ResourceRef::SessionFile {
                file_id,
                session_id,
            } => {
                if ctx.session_id.is_empty()
                    || session_id.as_deref().is_some_and(|s| s != ctx.session_id)
                {
                    return Err(ResourceIoError::new(
                        403,
                        "resource_access_denied",
                        "附件只限真实当前会话",
                    ));
                }
                let file = self
                    .inner
                    .session_files
                    .get_file(&ctx.session_id, &file_id)
                    .await
                    .map_err(|e| ResourceIoError::internal(e.to_string()))?
                    .filter(|f| f.status == "available" && !f.is_directory)
                    .ok_or_else(|| {
                        ResourceIoError::new(
                            403,
                            "resource_access_denied",
                            "附件不属于当前会话或不可用",
                        )
                    })?;
                // 必须固定实际fileId及sid；旧alias不能再走global resolver而误读源会话。
                let actual_ref = ResourceRef::SessionFile {
                    file_id: file.file_id.clone(),
                    session_id: Some(ctx.session_id.clone()),
                };
                let (key, actual, path) = self.resolve_session_file(&actual_ref).await?;
                let path = PathBuf::from(path);
                let observer = self.inner.session_files.payload_read_observer();
                let before_cancel = cancel.clone();
                let read_path = path.clone();
                let blocking_lease = Arc::clone(&lease);
                let snapshot = spawn_blocking_io(retained, move || {
                    let _lease = blocking_lease;
                    cancelled(&before_cancel)?;
                    if let Some(observer) = observer {
                        observer();
                    }
                    let snapshot = Snapshot::read_artifact(&read_path).map_err(read_error)?;
                    cancelled(&before_cancel)?;
                    if !snapshot.path_is_current(&read_path) {
                        return Err(conflict("登记附件在读取时改变"));
                    }
                    Ok(snapshot)
                })
                .await?;
                cancelled(&cancel)?;
                let latest = self
                    .inner
                    .session_files
                    .get_file(&ctx.session_id, &actual.file_id)
                    .await
                    .map_err(|e| ResourceIoError::internal(e.to_string()))?
                    .ok_or_else(|| conflict("附件登记在读取时撤销"))?;
                if latest.file_id != actual.file_id
                    || latest.file_path != actual.file_path
                    || latest.real_path != actual.real_path
                    || latest.status != "available"
                    || latest.is_directory
                    || !snapshot.path_is_current(&path)
                {
                    return Err(conflict("附件归属或路径在读取时改变"));
                }
                let mut meta = metadata(&path, &snapshot);
                meta["resourceKey"] = Value::String(key);
                meta["resource"] =
                    Self::session_file_descriptor(&actual_ref, &latest, &path.to_string_lossy());
                drop(lease);
                Ok(ArtifactSnapshot {
                    file_path: path,
                    bytes: snapshot.bytes,
                    meta,
                })
            }
            ResourceRef::LocalFile { .. } | ResourceRef::Mount { .. } => {
                let inner = self.inner.clone();
                let ctx = ctx.clone();
                spawn_blocking_io(retained, move || {
                    let _lease = lease;
                    cancelled(&cancel)?;
                    let files = inner.filesystem(&reference, "read", cancel.clone())?;
                    let plane = files
                        .workspace
                        .as_ref()
                        .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
                    let raw = files.local_path(&files.reference)?;
                    let path = plane.authorize(raw, ResourceOp::Read, &ctx)?;
                    if let Some(observer) = inner.session_files.payload_read_observer() {
                        observer();
                    }
                    let snapshot = Snapshot::read_artifact(&path).map_err(read_error)?;
                    cancelled(&cancel)?;
                    if plane.authorize(raw, ResourceOp::Read, &ctx)? != path
                        || !snapshot.path_is_current(&path)
                    {
                        return Err(conflict("办公输入路径或授权在读取时改变"));
                    }
                    let meta = files.map(metadata(&path, &snapshot))?;
                    Ok(ArtifactSnapshot {
                        file_path: path,
                        bytes: snapshot.bytes,
                        meta,
                    })
                })
                .await
            }
            other => Err(ResourceIoError::capability_denied(
                "artifact_read",
                provider_id_for_ref(&other),
            )),
        }
    }

    /// 在付费或渲染前调用；不mkdir、不临时写、不写来源/恢复日志、不发事件。
    pub(crate) async fn prepare_artifact_write(
        &self,
        input: &Value,
        overwrite: bool,
        ctx: &OpContext,
    ) -> Result<ArtifactWritePreflight, ResourceIoError> {
        let reference = normalize_resource_ref(input)?;
        let frozen_ref = FrozenResource::from_ref(&reference)?;
        let retained = self.blocking_permit(ctx)?;
        let cancel = self.operation_cancel(ctx)?;
        let lease = self.inner.session_files.payload_lease().await;
        let inner = self.inner.clone();
        let ctx = ctx.clone();
        spawn_blocking_io(retained, move || {
            let _lease = lease;
            cancelled(&cancel)?;
            let files = inner.filesystem(&reference, "write", cancel.clone())?;
            let plane = files
                .workspace
                .as_ref()
                .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
            let raw = files.local_path(&files.reference)?;
            let path = plane.authorize(raw, ResourceOp::Write, &ctx)?;
            if !normal_absolute(&path) || inner.file_safety.read().unwrap().is_protected(&path) {
                return Err(ResourceIoError::new(
                    403,
                    "resource_access_denied",
                    "产物目标受保护或路径不规范",
                ));
            }
            let (ancestor, missing_parents) = parent_fact(&path)?;
            let target = match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
                    if !overwrite {
                        return Err(ResourceIoError::target_already_exists(
                            &path.to_string_lossy(),
                        ));
                    }
                    let snapshot = Snapshot::read_artifact(&path).map_err(read_error)?;
                    if !snapshot.path_is_current(&path) {
                        return Err(conflict("预检目标改变"));
                    }
                    FrozenTarget::Existing {
                        stamp: FrozenStamp::read(&snapshot.stamp)?,
                    }
                }
                Ok(_) => {
                    return Err(ResourceIoError::new(
                        403,
                        "resource_access_denied",
                        "产物目标必须是普通文件",
                    ))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => FrozenTarget::Missing,
                Err(error) => return Err(conflict(format!("产物目标不可确认：{error}"))),
            };
            let mount_root = if let Some(mount) = &files.mount {
                mount.check(ResourceOp::Write)?;
                let held = DirFd::open(mount.root()).map_err(|e| conflict(e.to_string()))?;
                Some(FrozenDirectory::read(mount.root(), &held)?)
            } else {
                None
            };
            cancelled(&cancel)?;
            if plane.authorize(raw, ResourceOp::Write, &ctx)? != path {
                return Err(conflict("预检目标授权改变"));
            }
            ancestor.open()?;
            let facts = ArtifactWriteFacts {
                schema_version: 1,
                scope: FrozenScope::of(&ctx),
                reference: frozen_ref,
                resource_key: resource_key_for_ref(&reference),
                file_path: path,
                overwrite,
                target,
                ancestor,
                missing_parents,
                mount_root,
            };
            let result = ArtifactWritePreflight { facts };
            // 同一闭合校验也用于持久恢复，不存在另一套宽松字段解释。
            ArtifactWritePreflight::from_trusted_persistent_facts(&result.persistent_facts()?)
        })
        .await
    }

    /// 只接原预检对象；没有可覆盖冻结路径、版本或overwrite的新参数。
    pub(crate) async fn publish_artifact(
        &self,
        preflight: &ArtifactWritePreflight,
        bytes: Vec<u8>,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        if bytes.len() as u64 > ARTIFACT_LIMIT {
            return Err(ResourceIoError::new(
                413,
                "artifact_write_too_large",
                "办公产物超过50MiB",
            ));
        }
        if preflight.facts.scope != FrozenScope::of(ctx) {
            return Err(ResourceIoError::new(
                403,
                "resource_access_denied",
                "产物预检属于另一主体或会话",
            ));
        }
        let preflight = preflight.clone();
        let retained = self.blocking_permit(ctx)?;
        let cancel = self.operation_cancel(ctx)?;
        let lease = self.inner.session_files.payload_lease().await;
        let inner = self.inner.clone();
        let actual_ctx = ctx.clone();
        let result = spawn_blocking_io(retained, move || {
            let _lease = lease;
            let facts = &preflight.facts;
            let reference = facts.reference.reference();
            cancelled(&cancel)?;
            let files = inner.filesystem(&reference, "write", cancel.clone())?;
            let plane = files
                .workspace
                .as_ref()
                .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
            let raw = files.local_path(&files.reference)?;
            if plane.authorize(raw, ResourceOp::Write, &actual_ctx)? != facts.file_path {
                return Err(conflict("产物资源映射已改变"));
            }
            if let Some(expected) = &facts.mount_root {
                let mount = files
                    .mount
                    .as_ref()
                    .ok_or_else(|| conflict("原挂载已不可用"))?;
                mount.check(ResourceOp::Write)?;
                if mount.root() != expected.path {
                    return Err(conflict("原挂载根已改变"));
                }
                expected.open()?;
            }
            // 原来缺失的父目录若由外界创建，也不能悄悄改用新对象。
            let (ancestor, missing) = parent_fact(&facts.file_path)?;
            if ancestor != facts.ancestor || missing != facts.missing_parents {
                return Err(conflict("预检后的父目录结构已改变"));
            }
            let snapshot = match &facts.target {
                FrozenTarget::Existing { stamp } => {
                    let snapshot = Snapshot::read_artifact(&facts.file_path).map_err(read_error)?;
                    if FrozenStamp::read(&snapshot.stamp)? != *stamp {
                        return Err(conflict("原文件版本已改变"));
                    }
                    Some(snapshot)
                }
                FrozenTarget::Missing => {
                    match std::fs::symlink_metadata(&facts.file_path) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        _ => return Err(conflict("原本不存在的目标已出现或不可确认")),
                    }
                    None
                }
            };
            let mut held = facts.ancestor.open()?;
            let mut parent = facts.ancestor.path.clone();
            for name in &facts.missing_parents {
                cancelled(&cancel)?;
                if plane.authorize(raw, ResourceOp::Write, &actual_ctx)? != facts.file_path
                    || !held.same_directory(&parent).unwrap_or(false)
                {
                    return Err(conflict("创建产物目录前授权或目录改变"));
                }
                held = held
                    .new_child_directory(std::ffi::OsStr::new(name))
                    .map_err(|e| {
                        conflict(format!("新建产物父目录失败（此前可能已创建部分目录）：{e}"))
                    })?;
                parent.push(name);
            }
            let safety = inner.file_safety.read().unwrap().clone();
            let authorize = || {
                cancelled(&cancel).is_ok()
                    && plane
                        .authorize(raw, ResourceOp::Write, &actual_ctx)
                        .ok()
                        .as_deref()
                        == Some(facts.file_path.as_path())
                    && held.same_directory(&parent).unwrap_or(false)
                    && facts
                        .mount_root
                        .as_ref()
                        .is_none_or(|root| root.open().is_ok())
            };
            if !authorize() {
                return Err(conflict("产物提交前权限改变"));
            }
            let scope = crate::file_safety::witness_scope(
                actual_ctx.principal_kind,
                &actual_ctx.principal_subject,
                &actual_ctx.session_id,
            );
            match &snapshot {
                Some(snapshot) => {
                    safety.replace_artifact(&scope, &facts.file_path, snapshot, &bytes, authorize)
                }
                None => safety.create_artifact(&scope, &facts.file_path, &bytes, authorize),
            }
            .map_err(write_error)?;
            // 写后元数据必须来自同一确认版本，不能再stat把新版本配给旧产物。
            let confirmed = Snapshot::read_artifact(&facts.file_path).map_err(|e| {
                ResourceIoError::new(
                    409,
                    "artifact_write_unknown",
                    format!("产物已写入，最终版本未确认：{e}"),
                )
            })?;
            if !authorize()
                || confirmed.bytes != bytes
                || !safety.witnessed(&scope, &facts.file_path, &confirmed)
            {
                return Err(ResourceIoError::new(
                    409,
                    "artifact_write_unknown",
                    "产物已写入，但最终身份/授权/来源未确认",
                ));
            }
            let mut out = metadata(&facts.file_path, &confirmed);
            out["ok"] = Value::Bool(true);
            out["changeType"] = Value::String(
                if snapshot.is_some() {
                    "modified"
                } else {
                    "created"
                }
                .into(),
            );
            files.map(out).map_err(|error| {
                ResourceIoError::new(
                    409,
                    "artifact_write_unknown",
                    format!("产物已写入，但最终资源回执未确认：{error}"),
                )
            })
        })
        .await?;
        self.emit_changed(&result, ctx);
        self.capture_history(&result, ctx).await.map_err(|error| {
            ResourceIoError::new(
                409,
                "artifact_write_unknown",
                format!("产物已写入，但后续历史登记未确认：{error}"),
            )
        })?;
        Ok(result)
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lingxi-artifact-facts-{}",
            crate::auth::hex_random_public(8).unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        path.canonicalize().unwrap()
    }

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        workspace: PathBuf,
        db: Arc<lingxi_adapters::storage::RunDatabase>,
        files: Arc<SessionFileService>,
        io: ResourceIoService,
    }
    impl Fixture {
        async fn new() -> Self {
            let root = temp();
            let home = root.join("home");
            let workspace = root.join("workspace");
            std::fs::create_dir(&home).unwrap();
            std::fs::create_dir(&workspace).unwrap();
            let db = Arc::new(
                lingxi_adapters::storage::RunDatabase::open(
                    &home.join("runs.db"),
                    lingxi_adapters::storage::StoreOptions::default(),
                )
                .await
                .unwrap(),
            );
            db.ensure_session_seed(
                ["s", "fork", "other"]
                    .into_iter()
                    .map(|id| lingxi_adapters::storage::SessionRow {
                        session_id: id.into(),
                        agent_id: "a".into(),
                        owner_user_id: "user_local".into(),
                        title: id.into(),
                        created_at_unix_ms: 1,
                    })
                    .collect(),
            )
            .await
            .unwrap();
            let files = Arc::new(SessionFileService::new(home.clone(), db.clone(), || 1));
            let resources = Arc::new(ResourceService::new(
                files.clone(),
                "studio".into(),
                home.clone(),
                || 1,
            ));
            let access = Arc::new(ResourceAccess::new(std::slice::from_ref(&workspace)).unwrap());
            let io = ResourceIoService::new(
                files.clone(),
                resources,
                home.clone(),
                Some((access.clone(), workspace.clone())),
                250,
                Arc::new(|| 1),
            );
            io.set_mount_provider(Arc::new(crate::resource_mounts::MountProvider::new(
                &home,
                "studio".into(),
                access,
            )))
            .unwrap();
            Self {
                root,
                home,
                workspace,
                db,
                files,
                io,
            }
        }
        fn mount(&self, root: &Path, capabilities: &[&str]) {
            std::fs::write(self.home.join("studio-mounts.json"), serde_json::json!({
                "schemaVersion":1,"mounts":[{"schemaVersion":1,"mountId":"docs","hostStudioId":"studio","sourceKind":"storage","provider":"local_fs","rootLocator":{"path":root},"label":"文档","presentation":"folder","status":"active","capabilities":capabilities}]
            }).to_string()).unwrap();
        }
        async fn close(self) {
            self.db.close().await.unwrap();
            std::fs::remove_dir_all(self.root).unwrap();
        }
    }

    #[tokio::test]
    async fn actual_io_accepts_fifty_mib_without_expanding_text_or_json_and_preflight_is_read_only()
    {
        let fx = Fixture::new().await;
        let source = fx.workspace.join("large.bin");
        std::fs::File::create(&source)
            .unwrap()
            .set_len(ARTIFACT_LIMIT)
            .unwrap();
        let reference = serde_json::json!({"kind":"local-file","path":source});
        let ctx = OpContext::local_owner("s");
        let snapshot = fx
            .io
            .read_artifact_snapshot(&reference, &ctx)
            .await
            .unwrap();
        assert_eq!(snapshot.bytes.len(), ARTIFACT_LIMIT as usize);
        assert_eq!(
            snapshot.meta["artifactStamp"]["sha256"],
            lingxi_kernel::session_tree::sha256_hex(&snapshot.bytes)
        );
        assert_eq!(fx.io.read(&reference, &ctx).await.unwrap_err().status, 413);
        let target = fx.workspace.join("new/nested/output.bin");
        let target_ref = serde_json::json!({"kind":"local-file","path":target});
        let before_events = fx.io.events_since(0);
        let preflight = fx
            .io
            .prepare_artifact_write(&target_ref, false, &ctx)
            .await
            .unwrap();
        assert!(!fx.workspace.join("new").exists());
        assert!(!fx.home.join("file-write-witnesses").exists());
        assert!(!fx.home.join("file-recovery-records").exists());
        assert_eq!(fx.io.events_since(0), before_events);
        let restored = ArtifactWritePreflight::from_trusted_persistent_facts(
            &preflight.persistent_facts().unwrap(),
        )
        .unwrap();
        let result = fx
            .io
            .publish_artifact(&restored, snapshot.bytes, &ctx)
            .await
            .unwrap();
        assert_eq!(result["version"]["size"], ARTIFACT_LIMIT);
        assert_eq!(std::fs::metadata(&target).unwrap().len(), ARTIFACT_LIMIT);
        assert_eq!(
            result["artifactStamp"]["sha256"],
            snapshot.meta["artifactStamp"]["sha256"]
        );
        assert!(fx
            .io
            .write(
                &serde_json::json!({"path":fx.workspace.join("ordinary.txt")}),
                vec![0; 8 * 1024 * 1024 + 1],
                &ctx
            )
            .await
            .is_err());
        std::fs::File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_len(ARTIFACT_LIMIT + 1)
            .unwrap();
        assert_eq!(
            fx.io
                .read_artifact_snapshot(&reference, &ctx)
                .await
                .unwrap_err()
                .status,
            413
        );
        fx.close().await;
    }

    #[tokio::test]
    async fn durable_original_stamp_refuses_same_size_same_mtime_edit_without_any_write_effect() {
        let fx = Fixture::new().await;
        let target = fx.workspace.join("original.md");
        std::fs::write(&target, b"before").unwrap();
        let reference = serde_json::json!({"path":target});
        let ctx = OpContext::local_owner("s");
        let weak_before = fx.io.stat(&reference, &ctx).await.unwrap()["version"].clone();
        let modified = std::fs::metadata(&target).unwrap().modified().unwrap();
        let preflight = fx
            .io
            .prepare_artifact_write(&reference, true, &ctx)
            .await
            .unwrap();
        let saved = preflight.persistent_facts().unwrap();
        std::fs::write(&target, b"after!").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&target)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(
            fx.io.stat(&reference, &ctx).await.unwrap()["version"],
            weak_before
        );
        let restored = ArtifactWritePreflight::from_trusted_persistent_facts(&saved).unwrap();
        let events = fx.io.events_since(0);
        assert_eq!(
            fx.io
                .publish_artifact(&restored, b"generated".to_vec(), &ctx)
                .await
                .unwrap_err()
                .code,
            "artifact_version_conflict"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"after!");
        assert!(!fx.home.join("file-write-witnesses").exists());
        assert!(!fx.home.join("file-recovery-records").exists());
        assert_eq!(fx.io.events_since(0), events);
        fx.close().await;
    }

    #[tokio::test]
    async fn actual_mount_read_keeps_typed_write_scope_and_live_revocation() {
        let fx = Fixture::new().await;
        let mounted = fx.root.join("mounted");
        std::fs::create_dir(&mounted).unwrap();
        std::fs::File::create(mounted.join("large.bin"))
            .unwrap()
            .set_len(ARTIFACT_LIMIT)
            .unwrap();
        fx.mount(&mounted, &["read", "write"]);
        let ctx = OpContext::local_owner("s");
        let snapshot = fx
            .io
            .read_artifact_snapshot(
                &serde_json::json!({"kind":"mount","mountId":"docs","path":"large.bin"}),
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(snapshot.bytes.len(), ARTIFACT_LIMIT as usize);
        assert_eq!(snapshot.meta["resource"]["kind"], "mount");
        drop(snapshot);
        let target = serde_json::json!({"kind":"mount","mountId":"docs","path":"new/output.pdf"});
        let preflight = fx
            .io
            .prepare_artifact_write(&target, false, &ctx)
            .await
            .unwrap();
        fx.mount(&mounted, &["read"]);
        assert_eq!(
            fx.io
                .publish_artifact(&preflight, b"actual".to_vec(), &ctx)
                .await
                .unwrap_err()
                .status,
            403
        );
        assert!(!mounted.join("new").exists());
        assert!(fx
            .io
            .prepare_artifact_write(&target, false, &ctx)
            .await
            .is_err());
        assert!(
            fx.io
                .prepare_artifact_write(
                    &serde_json::json!({"path":mounted.join("outside-local.txt")}),
                    false,
                    &ctx
                )
                .await
                .is_err(),
            "不能把typed mount改成未授权本机路径"
        );
        fx.close().await;
    }

    #[tokio::test]
    async fn actual_fork_alias_reads_only_the_current_session_copy_and_foreign_ids_stay_denied() {
        let fx = Fixture::new().await;
        let source = fx
            .files
            .register_blob(
                "s",
                Some("source.png"),
                "image/png",
                b"\x89PNG\r\n\x1a\nsource-data",
                "user_upload",
            )
            .await
            .unwrap();
        let ids = fx
            .files
            .fork_session_files("s", "fork", std::slice::from_ref(&source.file_id))
            .await
            .unwrap();
        let fork = fx.files.get_file("fork", &ids[0]).await.unwrap().unwrap();
        assert_ne!(source.file_path, fork.file_path);
        let fork_bytes = b"\x89PNG\r\n\x1a\nforked-data";
        std::fs::write(&fork.file_path, fork_bytes).unwrap();
        let old_ref = serde_json::json!({"kind":"session-file","fileId":source.file_id});
        let read = fx
            .io
            .read_artifact_snapshot(&old_ref, &OpContext::local_owner("fork"))
            .await
            .unwrap();
        assert_eq!(read.bytes, fork_bytes);
        assert_eq!(read.meta["resource"]["fileId"], ids[0]);
        assert_eq!(read.meta["resource"]["sessionId"], "fork");
        assert_eq!(
            std::fs::read(&source.file_path).unwrap(),
            b"\x89PNG\r\n\x1a\nsource-data"
        );
        let resource = serde_json::json!({"kind":"resource","resourceId":crate::resources::resource_id_for_session_file_id(&source.file_id)});
        assert_eq!(
            fx.io
                .read_artifact_snapshot(&resource, &OpContext::local_owner("fork"))
                .await
                .unwrap()
                .bytes,
            fork_bytes
        );
        assert_eq!(
            fx.io
                .read_artifact_snapshot(&old_ref, &OpContext::local_owner("other"))
                .await
                .unwrap_err()
                .status,
            403
        );
        fx.close().await;
    }

    #[tokio::test]
    async fn replaced_parent_or_inserted_symlink_cannot_receive_a_frozen_artifact() {
        let fx = Fixture::new().await;
        let parent = fx.workspace.join("parent");
        std::fs::create_dir(&parent).unwrap();
        let reference = serde_json::json!({"path":parent.join("out.bin")});
        let ctx = OpContext::local_owner("s");
        let preflight = fx
            .io
            .prepare_artifact_write(&reference, false, &ctx)
            .await
            .unwrap();
        std::fs::rename(&parent, fx.workspace.join("original-parent")).unwrap();
        std::fs::create_dir(&parent).unwrap();
        assert!(fx
            .io
            .publish_artifact(&preflight, b"content".to_vec(), &ctx)
            .await
            .is_err());
        assert!(!parent.join("out.bin").exists());
        let second = fx
            .io
            .prepare_artifact_write(&reference, false, &ctx)
            .await
            .unwrap();
        std::fs::remove_dir(&parent).unwrap();
        std::os::unix::fs::symlink(fx.workspace.join("original-parent"), &parent).unwrap();
        assert!(fx
            .io
            .publish_artifact(&second, b"content".to_vec(), &ctx)
            .await
            .is_err());
        assert!(!fx.workspace.join("original-parent/out.bin").exists());
        fx.close().await;
    }

    #[test]
    fn missing_parent_preflight_is_read_only_and_detects_same_path_replacement() {
        let root = temp();
        let target = root.join("missing/nested/out.pdf");
        let (fact, missing) = parent_fact(&target).unwrap();
        assert_eq!(missing, ["missing", "nested"]);
        assert!(!root.join("missing").exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::rename(&root, root.with_extension("old")).unwrap();
        std::fs::create_dir(&root).unwrap();
        assert!(fact.open().is_err());
        std::fs::remove_dir(&root).unwrap();
        std::fs::remove_dir(root.with_extension("old")).unwrap();
    }

    #[test]
    fn persistent_facts_keep_original_content_hash_and_reject_changed_snapshot() {
        let root = temp();
        let path = root.join("source.bin");
        std::fs::write(&path, b"before").unwrap();
        let snapshot = Snapshot::read_artifact(&path).unwrap();
        let (ancestor, missing_parents) = parent_fact(&path).unwrap();
        let preflight = ArtifactWritePreflight {
            facts: ArtifactWriteFacts {
                schema_version: 1,
                scope: FrozenScope::of(&OpContext::local_owner("session")),
                reference: FrozenResource::LocalFile {
                    path: path.to_str().unwrap().into(),
                },
                resource_key: local_key(&path),
                file_path: path.clone(),
                overwrite: true,
                target: FrozenTarget::Existing {
                    stamp: FrozenStamp::read(&snapshot.stamp).unwrap(),
                },
                ancestor,
                missing_parents,
                mount_root: None,
            },
        };
        let saved = preflight.persistent_facts().unwrap();
        let restored = ArtifactWritePreflight::from_trusted_persistent_facts(&saved).unwrap();
        restored.require_snapshot(&snapshot.stamp).unwrap();
        std::fs::write(&path, b"after!").unwrap();
        assert!(restored
            .require_snapshot(&Snapshot::read_artifact(&path).unwrap().stamp)
            .is_err());
        let mut tampered = saved;
        tampered["allowed"] = Value::Bool(true);
        assert!(ArtifactWritePreflight::from_trusted_persistent_facts(&tampered).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resource_alias_cannot_hide_a_foreign_session_in_nested_input() {
        let ctx = OpContext::local_owner("current");
        assert!(check_reference_session(
            &serde_json::json!({"resource":{"kind":"resource","resourceId":"res_sf_example","sessionId":"foreign"}}),
            &ctx,
        ).is_err());
        assert!(check_reference_session(
            &serde_json::json!({"resource":{"kind":"session-file","fileId":"sf_example","sessionId":"current"}}),
            &ctx,
        ).is_ok());
    }
}
