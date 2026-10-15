//! 沿用 studio-mounts.json 的本机文件夹挂载；其它旧提供者仍明确不可用。
use crate::{
    filetools::fsio::DirFd,
    resourceaccess::{ResourceAccess, ResourceOp},
    resourceio::{ResourceIoError, ResourceRef},
};
use serde::Deserialize;
use serde_json::{json, Value};
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
