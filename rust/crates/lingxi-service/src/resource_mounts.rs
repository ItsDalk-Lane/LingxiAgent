//! 沿用 studio-mounts.json 的本机文件夹挂载；其它旧提供者仍明确不可用。
use crate::{
    filetools::fsio::DirFd,
    resourceaccess::{ResourceAccess, ResourceOp},
    resourceio::{ResourceIoError, ResourceRef},
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registry {
    schema_version: u64,
    mounts: Vec<Mount>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mount {
    schema_version: u64,
    mount_id: String,
    host_studio_id: String,
    source_kind: String,
    provider: Option<String>,
    root_locator: Option<Value>,
    label: String,
    presentation: String,
    status: String,
    capabilities: Vec<String>,
    grant_id: Option<String>,
    source_studio_id: Option<String>,
    source_resource_id: Option<String>,
}
pub struct MountProvider {
    home: PathBuf,
    studio: String,
    access: Arc<ResourceAccess>,
}
#[derive(Clone)]
pub(crate) struct MountLease {
    provider: Arc<MountProvider>,
    id: String,
    root: PathBuf,
    root_handle: Arc<DirFd>,
    relative: String,
    capability: String,
    cancel: Option<crate::cancel::CancelScope>,
    pub(crate) access: Arc<ResourceAccess>,
}
fn error(status: u16, code: &str, message: impl Into<String>) -> ResourceIoError {
    ResourceIoError::new(status, code, message)
}
fn normalize_path(path: &str) -> Result<String, ResourceIoError> {
    let path = path.replace('\\', "/");
    let path = path.trim_matches('/');
    if path.contains('\0')
        || (!path.is_empty()
            && path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == ".."))
        || (path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic())
    {
        return Err(error(400, "invalid_path", "挂载路径无效"));
    }
    Ok(path.into())
}
impl MountProvider {
    pub fn new(home: &Path, studio: String, access: Arc<ResourceAccess>) -> Self {
        Self {
            home: home.into(),
            studio,
            access,
        }
    }
    fn mount(&self, id: &str) -> Result<Mount, ResourceIoError> {
        let path = self.home.join("studio-mounts.json");
        if std::fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            return Err(ResourceIoError::unsupported_provider("mount"));
        }
        let snapshot =
            crate::file_safety::Snapshot::read_bounded(&path, 2 * 1024 * 1024).map_err(|e| {
                error(
                    503,
                    "provider_not_available",
                    format!("挂载登记不可读：{e}"),
                )
            })?;
        let registry: Registry = serde_json::from_slice(&snapshot.bytes)
            .map_err(|e| error(503, "provider_not_available", format!("挂载登记损坏：{e}")))?;
        if registry.schema_version != 1 || registry.mounts.len() > 4096 {
            return Err(error(
                503,
                "provider_not_available",
                "挂载登记版本或数量无效",
            ));
        }
        let mut ids = std::collections::HashSet::new();
        for m in &registry.mounts {
            let valid = m.schema_version == 1
                && !m.mount_id.trim().is_empty()
                && !m.host_studio_id.trim().is_empty()
                && !m.label.trim().is_empty()
                && ["folder", "external_panel", "linked_studio"].contains(&m.presentation.as_str())
                && ["active", "disabled"].contains(&m.status.as_str())
                && !m.capabilities.is_empty()
                && m.capabilities.iter().all(|c| {
                    ["list", "read", "write", "watch", "materialize", "execute"]
                        .contains(&c.as_str())
                })
                && match m.source_kind.as_str() {
                    "storage" => m.provider.as_deref().is_some_and(|p| {
                        ["local_fs", "webdav", "s3", "google_drive", "hana_cloud"].contains(&p)
                    }),
                    "studio" => {
                        m.source_studio_id
                            .as_deref()
                            .is_some_and(|s| !s.trim().is_empty() && s != m.host_studio_id)
                            && m.source_resource_id
                                .as_deref()
                                .is_some_and(|s| !s.trim().is_empty())
                            && m.grant_id.as_deref().is_some_and(|s| !s.trim().is_empty())
                    }
                    _ => false,
                };
            if !valid || !ids.insert(m.mount_id.clone()) {
                return Err(error(503, "provider_not_available", "挂载登记内容无效"));
            }
        }
        registry
            .mounts
            .into_iter()
            .find(|m| m.mount_id == id && m.host_studio_id == self.studio && m.status == "active")
            .ok_or_else(|| {
                error(
                    404,
                    "resource_not_found",
                    format!("挂载不存在或已停用：{id}"),
                )
            })
    }
    fn local_root(m: &Mount, required: &str) -> Result<PathBuf, ResourceIoError> {
        if m.source_kind != "storage" || m.provider.as_deref() != Some("local_fs") {
            return Err(error(
                501,
                "provider_not_available",
                format!(
                    "挂载提供者不可用：{}",
                    m.provider.as_deref().unwrap_or(&m.source_kind)
                ),
            ));
        }
        if !m.capabilities.iter().any(|c| c == required) {
            return Err(ResourceIoError::capability_denied(required, "mount"));
        }
        let root = m
            .root_locator
            .as_ref()
            .and_then(|v| v.get("path"))
            .and_then(Value::as_str)
            .map(Path::new)
            .filter(|p| p.is_absolute())
            .ok_or_else(|| error(500, "invalid_mount_root", "挂载根目录无效"))?;
        let root = root
            .canonicalize()
            .map_err(|e| error(500, "invalid_mount_root", format!("挂载根目录不可读：{e}")))?;
        if !root.is_dir() {
            return Err(error(500, "invalid_mount_root", "挂载根不是目录"));
        }
        Ok(root)
    }
    pub(crate) fn resolve(
        self: &Arc<Self>,
        reference: &ResourceRef,
        capability: &str,
        cancel: Option<crate::cancel::CancelScope>,
    ) -> Result<MountLease, ResourceIoError> {
        let ResourceRef::Mount { mount_id, path } = reference else {
            return Err(error(400, "invalid_resource_ref", "不是挂载引用"));
        };
        let relative = normalize_path(path)?;
        let mount = self.mount(mount_id)?;
        let root = Self::local_root(&mount, capability)?;
        let root_handle = Arc::new(DirFd::open(&root).map_err(|e| {
            error(
                409,
                "mount_root_changed",
                format!("挂载根身份不可确认：{e}"),
            )
        })?);
        let access = Arc::new(
            self.access
                .isolated_to(&root)
                .map_err(|e| error(403, "resource_access_denied", e.to_string()))?,
        );
        Ok(MountLease {
            provider: self.clone(),
            id: mount_id.clone(),
            root,
            root_handle,
            relative,
            capability: capability.into(),
            cancel,
            access,
        })
    }
}
impl MountLease {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn target(&self) -> PathBuf {
        self.root.join(&self.relative)
    }
    pub(crate) fn check(&self, op: ResourceOp) -> Result<(), ResourceIoError> {
        if self.cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Err(error(409, "resource_cancelled", "资源操作已取消"));
        }
        let mount = self.provider.mount(&self.id)?;
        let root = MountProvider::local_root(&mount, &self.capability)?;
        MountProvider::local_root(
            &mount,
            if op == ResourceOp::Write {
                "write"
            } else {
                "read"
            },
        )?;
        if root != self.root || !self.root_handle.same_directory(&self.root).unwrap_or(false) {
            return Err(error(409, "mount_root_changed", "挂载根身份已改变"));
        }
        Ok(())
    }
    pub(crate) fn relative_ref(&self, path: &Path) -> Result<ResourceRef, ResourceIoError> {
        let rel = path
            .strip_prefix(&self.root)
            .map_err(|_| error(403, "resource_access_denied", "资源越出挂载根"))?;
        Ok(ResourceRef::Mount {
            mount_id: self.id.clone(),
            path: rel.to_string_lossy().replace('\\', "/"),
        })
    }
    pub(crate) fn descriptor(&self, path: &Path) -> Result<Value, ResourceIoError> {
        let ResourceRef::Mount {
            mount_id,
            path: relative,
        } = self.relative_ref(path)?
        else {
            unreachable!()
        };
        Ok(
            json!({"kind":"mount","provider":"mount","mountId":mount_id,"path":relative,"filePath":path.to_string_lossy()}),
        )
    }
    pub(crate) fn key(&self, path: &Path) -> Result<String, ResourceIoError> {
        Ok(crate::resourceio::resource_key_for_ref(
            &self.relative_ref(path)?,
        ))
    }
    pub(crate) fn sibling(&self, reference: &ResourceRef) -> Result<ResourceRef, ResourceIoError> {
        let ResourceRef::Mount { mount_id, path } = reference else {
            return Err(error(
                501,
                "cross_provider_move_unsupported",
                "挂载移动必须在同一个挂载内",
            ));
        };
        if mount_id != &self.id {
            return Err(error(
                501,
                "cross_provider_move_unsupported",
                "挂载移动必须在同一个挂载内",
            ));
        }
        Ok(ResourceRef::LocalFile {
            path: self
                .root
                .join(normalize_path(path)?)
                .to_string_lossy()
                .into_owned(),
        })
    }
    pub(crate) fn map_result(&self, mut value: Value) -> Result<Value, ResourceIoError> {
        let file = value
            .get("filePath")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| self.target());
        value["resourceKey"] = json!(self.key(&file)?);
        value["resource"] = self.descriptor(&file)?;
        Ok(value)
    }
    pub(crate) fn map_move(&self, mut value: Value) -> Result<Value, ResourceIoError> {
        for (path, key, resource) in [
            ("oldFilePath", "oldResourceKey", "oldResource"),
            ("newFilePath", "newResourceKey", "newResource"),
        ] {
            let file = value
                .get(path)
                .and_then(Value::as_str)
                .ok_or_else(|| error(500, "resource_io_error", "移动缺少真实路径"))?;
            let file = PathBuf::from(file);
            value[key] = json!(self.key(&file)?);
            value[resource] = self.descriptor(&file)?;
        }
        Ok(value)
    }
}

// ── 注册表写路径（现役 core/studio-mounts.ts 平价）──
//
// 校验/规范化/原子写逐条对齐 ensureStudioMountRegistry / loadStudioMountRegistry /
// upsertStudioMount / disableStudioMount / listStudioMountsForStudio /
// validateStudioMount(Registry) / normalizeMountCapabilities：
// - 文件缺失时创建空登记（读路径 MountProvider 的 unsupported_provider 语义只服务
//   于挂载解析，不服务于工作台列表面）；
// - upsert 规范化只保留已知键并丢掉未知键；disable 保留该挂载的全部既有键；
// - 写盘为 JSON 两空格缩进 + 尾换行 + 原子替换（tmp+rename）。

const SOURCE_KINDS: [&str; 2] = ["storage", "studio"];
const STORAGE_PROVIDERS: [&str; 5] = ["local_fs", "webdav", "s3", "google_drive", "hana_cloud"];
const PRESENTATIONS: [&str; 3] = ["folder", "external_panel", "linked_studio"];
const STATUSES: [&str; 2] = ["active", "disabled"];
const CAPABILITIES: [&str; 6] = ["list", "read", "write", "watch", "materialize", "execute"];
/// 与 MountProvider::mount 的读取上限一致：写出不得超过读取可承载。
const REGISTRY_MOUNT_CAP: usize = 4096;

fn registry_error(message: impl Into<String>) -> ResourceIoError {
    // 现役校验失败抛裸 Error，路由层落 400 studio_workspace_failed。
    error(400, "studio_workspace_failed", message)
}

fn iso_now(now_ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(now_ms as i64)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn non_empty(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| !s.trim().is_empty())
}

/// validateStudioMount 的校验段（不写回规范化结果）。
fn validate_mount(value: &Value) -> Result<(), ResourceIoError> {
    let obj = value
        .as_object()
        .ok_or_else(|| registry_error("invalid studio mount: expected object"))?;
    if value.get("schemaVersion") != Some(&json!(1)) {
        return Err(registry_error(
            "invalid studio mount: schemaVersion must be 1",
        ));
    }
    for field in ["mountId", "hostStudioId", "label"] {
        if non_empty(&value[field]).is_none() {
            return Err(registry_error(format!("{field} required")));
        }
    }
    let source_kind = value["sourceKind"].as_str().unwrap_or("");
    if !SOURCE_KINDS.contains(&source_kind) {
        return Err(registry_error(format!(
            "sourceKind must be one of {}",
            SOURCE_KINDS.join(", ")
        )));
    }
    if !PRESENTATIONS.contains(&value["presentation"].as_str().unwrap_or("")) {
        return Err(registry_error(format!(
            "presentation must be one of {}",
            PRESENTATIONS.join(", ")
        )));
    }
    if !STATUSES.contains(&value["status"].as_str().unwrap_or("")) {
        return Err(registry_error(format!(
            "status must be one of {}",
            STATUSES.join(", ")
        )));
    }
    let capabilities = value["capabilities"]
        .as_array()
        .filter(|list| !list.is_empty())
        .ok_or_else(|| registry_error("capabilities must be a non-empty array"))?;
    for capability in capabilities {
        if !CAPABILITIES.contains(&capability.as_str().unwrap_or("")) {
            return Err(registry_error(format!(
                "unknown mount capability: {}",
                capability.as_str().unwrap_or("<non-string>")
            )));
        }
    }
    if source_kind == "storage" {
        if !STORAGE_PROVIDERS.contains(&value["provider"].as_str().unwrap_or("")) {
            return Err(registry_error(format!(
                "provider must be one of {}",
                STORAGE_PROVIDERS.join(", ")
            )));
        }
    } else {
        for field in ["sourceStudioId", "sourceResourceId", "grantId"] {
            if non_empty(&value[field]).is_none() {
                return Err(registry_error(format!("{field} required")));
            }
        }
        if value["sourceStudioId"] == value["hostStudioId"] {
            return Err(registry_error(
                "Studio mount cannot point at its own hostStudioId",
            ));
        }
    }
    let _ = obj;
    Ok(())
}

fn validate_registry(value: &Value) -> Result<(), ResourceIoError> {
    if !value.is_object() {
        return Err(registry_error(
            "invalid studio-mounts.json: expected object",
        ));
    }
    if value.get("schemaVersion") != Some(&json!(1)) {
        return Err(registry_error(
            "invalid studio-mounts.json: schemaVersion must be 1",
        ));
    }
    let mounts = value["mounts"]
        .as_array()
        .ok_or_else(|| registry_error("invalid studio-mounts.json: mounts must be an array"))?;
    if mounts.len() > REGISTRY_MOUNT_CAP {
        return Err(registry_error(
            "invalid studio-mounts.json: mounts too many",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for mount in mounts {
        validate_mount(mount)?;
        if !seen.insert(mount["mountId"].as_str().unwrap_or("").to_owned()) {
            return Err(registry_error(format!(
                "invalid studio-mounts.json: duplicate mountId {}",
                mount["mountId"].as_str().unwrap_or("")
            )));
        }
    }
    Ok(())
}

/// validateStudioMount 的规范化段：只保留已知键，capabilities 按目录序去重。
fn normalize_mount_for_write(value: &Value, now: &str) -> Result<Value, ResourceIoError> {
    let capabilities = value["capabilities"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    for capability in &capabilities {
        if let Some(c) = capability.as_str() {
            seen.insert(c.to_owned());
        }
    }
    let normalized_caps: Vec<Value> = CAPABILITIES
        .iter()
        .filter(|c| seen.contains(**c))
        .map(|c| json!(c))
        .collect();
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(1));
    for field in [
        "mountId",
        "hostStudioId",
        "sourceKind",
        "label",
        "presentation",
    ] {
        out.insert(field.into(), value[field].clone());
    }
    out.insert("capabilities".into(), json!(normalized_caps));
    out.insert(
        "grantId".into(),
        value.get("grantId").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "status".into(),
        value
            .get("status")
            .filter(|s| !s.is_null())
            .cloned()
            .unwrap_or_else(|| json!("active")),
    );
    out.insert(
        "createdAt".into(),
        value
            .get("createdAt")
            .and_then(|v| v.as_str().map(String::from))
            .map(Value::String)
            .unwrap_or_else(|| json!(now)),
    );
    out.insert(
        "updatedAt".into(),
        value
            .get("updatedAt")
            .and_then(|v| v.as_str().map(String::from))
            .map(Value::String)
            .unwrap_or_else(|| json!(now)),
    );
    if value["sourceKind"].as_str() == Some("storage") {
        out.insert("provider".into(), value["provider"].clone());
        if let Some(locator) = value.get("rootLocator") {
            out.insert("rootLocator".into(), locator.clone());
        }
    } else {
        for field in ["sourceStudioId", "sourceResourceId"] {
            out.insert(field.into(), value[field].clone());
        }
    }
    let out = Value::Object(out);
    validate_mount(&out)?;
    Ok(out)
}

/// studio-mounts.json 的读写台（现役 studio-mounts.ts 的加载/ upsert / disable / 列举）。
pub(crate) struct MountRegistry<'a> {
    home: &'a Path,
}

impl<'a> MountRegistry<'a> {
    pub(crate) fn new(home: &'a Path) -> Self {
        Self { home }
    }
    fn path(&self) -> PathBuf {
        self.home.join("studio-mounts.json")
    }
    fn read_present(&self) -> Result<Option<Value>, ResourceIoError> {
        let path = self.path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(registry_error(format!(
                    "failed to read studio-mounts.json: {e}"
                )))
            }
        };
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| registry_error(format!("invalid studio-mounts.json: {e}")))?;
        Ok(Some(value))
    }
    fn write(&self, registry: &Value) -> Result<(), ResourceIoError> {
        validate_registry(registry)?;
        let mut text = serde_json::to_string_pretty(registry)
            .map_err(|e| registry_error(format!("登记序列化失败：{e}")))?;
        text.push('\n');
        crate::paths::atomic_write(&self.path(), text.as_bytes())
            .map_err(|e| registry_error(format!("failed to write studio-mounts.json: {e}")))
    }
    /// ensureStudioMountRegistry + loadStudioMountRegistry：缺失则创建空登记。
    pub(crate) fn load_or_create(&self, now_ms: u64) -> Result<Value, ResourceIoError> {
        if let Some(current) = self.read_present()? {
            validate_registry(&current)?;
            return Ok(current);
        }
        let now = iso_now(now_ms);
        let created = json!({"schemaVersion":1,"mounts":[],"createdAt":now,"updatedAt":now});
        self.write(&created)?;
        Ok(created)
    }
    pub(crate) fn list_for_studio(
        &self,
        studio: &str,
        now_ms: u64,
    ) -> Result<Vec<Value>, ResourceIoError> {
        if studio.trim().is_empty() {
            return Err(registry_error("hostStudioId required"));
        }
        let registry = self.load_or_create(now_ms)?;
        Ok(registry["mounts"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|mount| mount["hostStudioId"].as_str() == Some(studio))
            .collect())
    }
    /// upsertStudioMount：同 mountId 覆盖（createdAt 沿用旧值），否则追加。
    pub(crate) fn upsert(&self, mount: Value, now_ms: u64) -> Result<Value, ResourceIoError> {
        let now = iso_now(now_ms);
        let mut mount = mount;
        mount["schemaVersion"] = json!(1);
        if mount.get("status").is_none_or(Value::is_null) {
            mount["status"] = json!("active");
        }
        let normalized = normalize_mount_for_write(&mount, &now)?;
        let mut registry = self.load_or_create(now_ms)?;
        let mounts = registry["mounts"]
            .as_array_mut()
            .ok_or_else(|| registry_error("invalid studio-mounts.json: mounts must be an array"))?;
        let existing = mounts
            .iter()
            .position(|item| item["mountId"] == normalized["mountId"]);
        let stored = if let Some(index) = existing {
            let created_at = mounts[index]
                .get("createdAt")
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| normalized["createdAt"].as_str().unwrap_or(&now).into());
            let mut next = normalized.clone();
            next["createdAt"] = json!(created_at);
            next["updatedAt"] = json!(now);
            mounts[index] = next.clone();
            next
        } else {
            mounts.push(normalized.clone());
            normalized
        };
        registry["updatedAt"] = json!(now);
        self.write(&registry)?;
        Ok(stored)
    }
    /// disableStudioMount：仅翻 status=disabled，保留该挂载全部既有键。
    pub(crate) fn disable(
        &self,
        mount_id: &str,
        host_studio: Option<&str>,
        now_ms: u64,
    ) -> Result<Value, ResourceIoError> {
        if mount_id.trim().is_empty() {
            return Err(registry_error("mountId required"));
        }
        let now = iso_now(now_ms);
        let mut registry = self.load_or_create(now_ms)?;
        let mounts = registry["mounts"]
            .as_array_mut()
            .ok_or_else(|| registry_error("invalid studio-mounts.json: mounts must be an array"))?;
        let Some(index) = mounts.iter().position(|item| {
            item["mountId"].as_str() == Some(mount_id)
                && host_studio.is_none_or(|studio| item["hostStudioId"].as_str() == Some(studio))
        }) else {
            return Err(registry_error(format!(
                "studio mount not found: {mount_id}"
            )));
        };
        mounts[index]["status"] = json!("disabled");
        mounts[index]["updatedAt"] = json!(now);
        let stored = mounts[index].clone();
        registry["updatedAt"] = json!(now);
        self.write(&registry)?;
        Ok(stored)
    }
}

/// localFsMountId：sha256(`${studioId}\0${rootPath}`) 前 16 个 hex。
pub(crate) fn local_fs_mount_id(studio: &str, root_path: &str) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(format!("{studio}\0{root_path}").as_bytes());
    let mut hex = String::with_capacity(16);
    for byte in &digest[..8] {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("local_fs_{hex}")
}
