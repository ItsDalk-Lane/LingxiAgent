//! MCP 设置的单一管理者：请求/应答、私有保存、真实连接与助手隔离共用此入口。
#[path = "mcp_apps.rs"]
mod apps;
#[path = "mcp_oauth.rs"]
mod oauth;

use crate::{
    agents_admin::agent_config_readback as config,
    appevents::AppEventLog,
    auth::{AuthService, Principal, PrincipalKind},
    mcpbridge::{self, McpEndpoint, McpServer, RegisteredMcpServer},
    memory::MemoryService,
    toolgateway::{self, InvocationPermissionContext, ToolInvocationGateway},
    ServiceState,
};
use crate::{
    file_safety::{FileSafety, Snapshot, WriteError},
    filetools::fsio::DirFd,
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use lingxi_adapters::storage::agent_memory as am;
use lingxi_kernel::{
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest},
    subagent::SessionPermissionMode,
    toolcatalog::{Availability, SchemaBudget, ToolTargetId},
    RunContext,
};
use lingxi_protocol::{
    ErrorCode, McpConnectorBulkResult, McpConnectorImportResult, McpSettingsChange,
    McpSettingsFailure, McpSettingsRequest, McpSettingsResult, McpSettingsState, McpSettingsStatus,
    McpSettingsUpdate, ProtocolError, ToolCallId,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, OnceLock, RwLock, Weak,
    },
};
use tokio::sync::{Mutex, Notify};

pub const SETTINGS_TOPIC: &str = "mcp:settings-action";
pub const ACTIONS: &[(&str, &str, bool)] = &[
    ("mcp.global.enabled", "全局启用或停用 MCP", false),
    ("mcp.connector.add", "添加 MCP 连接器", false),
    ("mcp.connector.update", "修改 MCP 连接器", false),
    ("mcp.connector.remove", "移除 MCP 连接器", false),
    ("mcp.connector.start", "启动已配置的 MCP 连接器", false),
    ("mcp.connector.stop", "停止 MCP 连接器并保存停用意图", false),
    ("mcp.connector.refresh_tools", "刷新 MCP 工具目录", false),
    (
        "mcp.agent.connector.enable",
        "为当前助手启停 MCP 连接器",
        true,
    ),
    ("mcp.agent.tool.enable", "为当前助手启停指定 MCP 工具", true),
];
const CONNECT_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct McpSettingsError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub saved: bool,
    pub runtime_applied: bool,
    results: Vec<McpConnectorImportResult>,
}
impl McpSettingsError {
    fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            saved: false,
            runtime_applied: false,
            results: vec![],
        }
    }
    fn saved(mut self) -> Self {
        self.saved = true;
        self
    }
    pub fn value(&self, action: &str) -> Value {
        let mut value = json!(McpSettingsResult {
            settings_update: McpSettingsUpdate {
                status: if self.code == "mcp_cancelled" || self.code == "mcp_stopping" {
                    McpSettingsStatus::Cancelled
                } else {
                    McpSettingsStatus::Failed
                },
                action: action.into(),
                key: action.into(),
                title: "MCP 设置未全部完成".into(),
                summary: self.message.clone(),
                saved: self.saved,
                runtime_applied: self.runtime_applied,
                connector_id: None,
                agent_id: None,
                changes: vec![],
                error: Some(McpSettingsFailure {
                    code: self.code.clone(),
                    message: self.message.clone()
                })
            }
        });
        if !self.results.is_empty() {
            value["results"] = json!(self.results);
        }
        value
    }
}
impl std::fmt::Display for McpSettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for McpSettingsError {}
type Result<T> = std::result::Result<T, McpSettingsError>;
fn invalid(message: impl Into<String>) -> McpSettingsError {
    McpSettingsError::new(400, "invalid_mcp_settings", message)
}
fn storage(message: impl Into<String>) -> McpSettingsError {
    McpSettingsError::new(500, "mcp_settings_storage", message)
}
fn string(value: &Value, field: &str) -> Result<String> {
    value
        .as_str()
        .filter(|v| !v.trim().is_empty() && !v.contains('\0'))
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("{field} 必须为非空文本")))
}
fn flag(input: &Value) -> Result<bool> {
    input
        .get("enabled")
        .or_else(|| input.get("value"))
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid("enabled 必须为布尔值"))
}
fn id_of(input: &Value) -> Result<String> {
    let id = string(
        input
            .get("connectorId")
            .or_else(|| input.get("serverId"))
            .or_else(|| input.get("id"))
            .unwrap_or(&Value::Null),
        "connectorId",
    )?;
    validate_id(&id)?;
    Ok(id)
}
fn validate_id(id: &str) -> Result<()> {
    if id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        Err(invalid("连接器标识只允许字母、数字、点、横线和下划线"))
    } else {
        Ok(())
    }
}
fn masked(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && s.chars().all(|c| matches!(c, '*' | '•')))
}
fn secret_field(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("secret")
        || key.contains("token")
        || key == "headers"
        || key == "env"
        || key == "oauth"
        || key.contains("password")
        || key.contains("api_key")
        || key == "authorization"
}
fn public_config(config: &Value, include_connection: bool) -> Value {
    fn hide(value: &Value) -> Value {
        match value {
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| {
                        let projected = if secret_field(key) {
                            if let Some(fields) = value.as_object() {
                                json!(fields
                                    .keys()
                                    .map(|key| (key.clone(), json!("********")))
                                    .collect::<serde_json::Map<_, _>>())
                            } else {
                                json!("********")
                            }
                        } else {
                            hide(value)
                        };
                        (key.clone(), projected)
                    })
                    .collect(),
            ),
            Value::Array(values) => json!(values.iter().map(hide).collect::<Vec<_>>()),
            _ => value.clone(),
        }
    }
    let mut result = hide(config);
    result["enabled"] = json!(config["enabled"] == true);
    result["deferEnabled"] = json!(config["deferEnabled"] != false);
    result["builtinDeferEnabled"] = json!(config["builtinDeferEnabled"] == true);
    result["deferThreshold"] = config.get("deferThreshold").cloned().unwrap_or(json!(4));
    if !include_connection {
        // 无明确助手的旧状态面保留最小公开形状；只有本机拥有者的页面请求读取编辑资料。
        for field in ["connectors", "servers"] {
            if let Some(connectors) = result.get_mut(field).and_then(Value::as_array_mut) {
                for connector in connectors {
                    connector.as_object_mut().unwrap().retain(|key, _| {
                        matches!(
                            key.as_str(),
                            "id" | "name"
                                | "transport"
                                | "enabled"
                                | "running"
                                | "error"
                                | "status"
                                | "tools"
                        )
                    });
                }
            }
        }
    }
    result
}
struct FileVersion {
    snapshot: Option<Snapshot>,
    parent: Arc<DirFd>,
}
fn unmask(old: &Value, patch: &Value) -> Value {
    if masked(patch) {
        return old.clone();
    }
    match patch {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter_map(|(k, v)| {
                    let previous = old.get(k).unwrap_or(&Value::Null);
                    if masked(v) && previous.is_null() {
                        None
                    } else {
                        Some((k.clone(), unmask(previous, v)))
                    }
                })
                .collect(),
        ),
        _ => patch.clone(),
    }
}
fn merge(target: &mut Value, patch: &Value) {
    if let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) {
        for (k, v) in patch {
            if !matches!(k.as_str(), "env" | "headers" | "oauth")
                && v.is_object()
                && target.get(k).is_some_and(Value::is_object)
            {
                merge(target.get_mut(k).unwrap(), v)
            } else {
                target.insert(k.clone(), v.clone());
            }
        }
    }
}

#[derive(Clone)]
struct SettingsAuthority {
    auth: Arc<AuthService>,
    original: Principal,
}
impl SettingsAuthority {
    fn validate(&self) -> Result<()> {
        if !self.original.is_local_owner()
            || !self.original.has_scope("settings.write")
            || self.auth.validate_principal(&self.original).is_err()
        {
            return Err(McpSettingsError::new(
                403,
                "mcp_original_authority_revoked",
                "原始本机设置授权已失效，未执行设置变更",
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
struct PendingStart {
    sequence: u64,
    cancel: crate::cancel::CancelScope,
}

struct StartPauseGuard<'a>(&'a AtomicU64);
impl Drop for StartPauseGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct PendingStartGuard {
    manager: Weak<McpSettingsManager>,
    id: String,
    sequence: u64,
}
impl Drop for PendingStartGuard {
    fn drop(&mut self) {
        if let Some(manager) = self.manager.upgrade() {
            let mut pending = manager
                .pending_starts
                .write()
                .unwrap_or_else(|p| p.into_inner());
            if pending
                .get(&self.id)
                .is_some_and(|job| job.sequence == self.sequence)
            {
                pending.remove(&self.id);
            }
            drop(pending);
            manager.pending_changed.notify_waiters();
        }
    }
}

struct Runtime {
    running: BTreeMap<String, RegisteredMcpServer>,
    errors: BTreeMap<String, String>,
    receipts: VecDeque<Value>,
}
#[cfg(windows)]
type ReadDirectory = crate::discovery_tools::windows_readonly::DirFd;
#[cfg(not(windows))]
type ReadDirectory = DirFd;
type RegistrationObserver = Arc<dyn Fn(&ToolTargetId) + Send + Sync>;
type ManagementRequestObserver = Arc<dyn Fn(&str) + Send + Sync>;
pub struct McpSettingsManager {
    memory: Arc<MemoryService>,
    home_handle: Option<DirFd>,
    read_home: Option<ReadDirectory>,
    read_plugin: OnceLock<Arc<ReadDirectory>>,
    read_parent: OnceLock<Arc<ReadDirectory>>,
    plugin_handle: OnceLock<Arc<DirFd>>,
    directory_handle: OnceLock<Arc<DirFd>>,
    file_safety: FileSafety,
    source_write: std::sync::Mutex<()>,
    network: OnceLock<Weak<lingxi_adapters::models::network::NetworkPlane>>,
    oauth_flows: std::sync::Mutex<BTreeMap<String, Arc<oauth::OAuthFlow>>>,
    oauth_flow_epochs: RwLock<BTreeMap<String, u64>>,
    oauth_credential_epochs: RwLock<BTreeMap<String, u64>>,
    events: Arc<AppEventLog>,
    gateway: Option<Weak<ToolInvocationGateway>>,
    runtime: Mutex<Runtime>,
    tasks: OnceLock<Weak<crate::task_supervisor::TaskSupervisor>>,
    pending_starts: RwLock<BTreeMap<String, PendingStart>>,
    pending_sequence: AtomicU64,
    start_pauses: AtomicU64,
    pending_changed: Notify,
    live: RwLock<BTreeSet<String>>,
    live_servers: RwLock<BTreeMap<String, Weak<McpServer>>>,
    closing: AtomicBool,
    connector_epoch: AtomicU64,
    settings_epoch: AtomicU64,
    builtin_residents: OnceLock<BTreeSet<ToolTargetId>>,
    registration_observer: RwLock<Option<RegistrationObserver>>,
    management_request_observer: RwLock<Option<ManagementRequestObserver>>,
    stop: Notify,
}
impl McpSettingsManager {
    pub fn new(
        memory: Arc<MemoryService>,
        events: Arc<AppEventLog>,
        gateway: Option<Weak<ToolInvocationGateway>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            home_handle: DirFd::open(memory.home()).ok(),
            read_home: ReadDirectory::open(memory.home()).ok(),
            read_plugin: OnceLock::new(),
            read_parent: OnceLock::new(),
            plugin_handle: OnceLock::new(),
            directory_handle: OnceLock::new(),
            file_safety: FileSafety::new(memory.home()),
            source_write: std::sync::Mutex::new(()),
            network: OnceLock::new(),
            oauth_flows: std::sync::Mutex::new(BTreeMap::new()),
            oauth_flow_epochs: RwLock::new(BTreeMap::new()),
            oauth_credential_epochs: RwLock::new(BTreeMap::new()),
            memory,
            events,
            gateway,
            runtime: Mutex::new(Runtime {
                running: BTreeMap::new(),
                errors: BTreeMap::new(),
                receipts: VecDeque::new(),
            }),
            tasks: OnceLock::new(),
            pending_starts: RwLock::new(BTreeMap::new()),
            pending_sequence: AtomicU64::new(0),
            start_pauses: AtomicU64::new(0),
            pending_changed: Notify::new(),
            live: RwLock::new(BTreeSet::new()),
            live_servers: RwLock::new(BTreeMap::new()),
            closing: AtomicBool::new(false),
            connector_epoch: AtomicU64::new(0),
            settings_epoch: AtomicU64::new(0),
            builtin_residents: OnceLock::new(),
            registration_observer: RwLock::new(None),
            management_request_observer: RwLock::new(None),
            stop: Notify::new(),
        })
    }
    pub fn attach_network(
        &self,
        plane: Option<&Arc<lingxi_adapters::models::network::NetworkPlane>>,
    ) {
        if let Some(plane) = plane {
            let _ = self.network.set(Arc::downgrade(plane));
        }
    }
    /// 后台启动沿用服务唯一任务监管者，不持有强引用环。
    pub fn attach_tasks(&self, tasks: &Arc<crate::task_supervisor::TaskSupervisor>) {
        let _ = self.tasks.set(Arc::downgrade(tasks));
    }
    fn cancel_pending(&self, id: Option<&str>, reason: &str) {
        for (key, job) in self
            .pending_starts
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
        {
            if id.is_none_or(|id| key == id) {
                job.cancel.cancel(reason);
            }
        }
    }
    fn has_pending(&self, id: &str) -> bool {
        self.pending_starts
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(id)
    }
    /// 只观察正式注册中的确定时点；不能替换连接、权限或执行结果。
    #[doc(hidden)]
    pub fn set_registration_observer(&self, observer: Option<RegistrationObserver>) {
        *self
            .registration_observer
            .write()
            .unwrap_or_else(|p| p.into_inner()) = observer;
    }
    fn path(&self) -> PathBuf {
        self.memory.home().join("plugin-data/mcp/config.json")
    }
    /// 仅观测真实HTTP已经通过身份和形状检查；不能替换权限、锁或执行结果。
    #[doc(hidden)]
    pub fn set_management_request_observer(&self, observer: Option<ManagementRequestObserver>) {
        *self
            .management_request_observer
            .write()
            .unwrap_or_else(|p| p.into_inner()) = observer;
    }
    fn observe_management_request(&self, action: &str) {
        let observer = self
            .management_request_observer
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(observer) = observer {
            observer(action);
        }
    }
    fn directory(&self) -> Result<Arc<DirFd>> {
        let root = self
            .home_handle
            .as_ref()
            .ok_or_else(|| storage("MCP 原始目录句柄不可用"))?;
        if !root.same_directory(self.memory.home()).unwrap_or(false) {
            return Err(storage("MCP 数据目录身份已改变"));
        }
        if self.plugin_handle.get().is_none() {
            let opened = root
                .child_directory(std::ffi::OsStr::new("plugin-data"), true)
                .map_err(|_| storage("MCP plugin-data 不能为链接"))?;
            let _ = self.plugin_handle.set(Arc::new(opened));
        }
        let plugin = self.plugin_handle.get().unwrap();
        if !plugin
            .same_directory(&self.memory.home().join("plugin-data"))
            .unwrap_or(false)
        {
            return Err(storage("MCP plugin-data 目录身份已改变"));
        }
        if self.directory_handle.get().is_none() {
            let opened = plugin
                .child_directory(std::ffi::OsStr::new("mcp"), true)
                .map_err(|_| storage("MCP 状态目录不能为链接"))?;
            let _ = self.directory_handle.set(Arc::new(opened));
        }
        let directory = self.directory_handle.get().unwrap();
        if !directory
            .same_directory(self.path().parent().unwrap())
            .unwrap_or(false)
        {
            return Err(storage("MCP 状态目录身份已改变"));
        }
        Ok(directory.clone())
    }
    fn readonly_directory(
        path: &std::path::Path,
        slot: &OnceLock<Arc<ReadDirectory>>,
    ) -> Result<Option<Arc<ReadDirectory>>> {
        if let Some(directory) = slot.get() {
            if !directory.same_directory(path).unwrap_or(false) {
                return Err(storage("MCP 父目录身份已改变"));
            }
            return Ok(Some(directory.clone()));
        }
        let directory = match ReadDirectory::open(path) {
            Ok(directory) => Arc::new(directory),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(storage("MCP 只读目录不能为链接、reparse或非法路径")),
        };
        let _ = slot.set(directory);
        Ok(slot.get().cloned())
    }
    fn read(&self) -> Result<Value> {
        let home = self
            .read_home
            .as_ref()
            .ok_or_else(|| storage("MCP 原始只读目录不可用"))?;
        if !home.same_directory(self.memory.home()).unwrap_or(false) {
            return Err(storage("MCP 数据目录身份已改变"));
        }
        if Self::readonly_directory(&self.memory.home().join("plugin-data"), &self.read_plugin)?
            .is_none()
        {
            return Ok(Self::empty_document());
        }
        let Some(parent) =
            Self::readonly_directory(self.path().parent().unwrap(), &self.read_parent)?
        else {
            return Ok(Self::empty_document());
        };
        let path = self.path();
        let Some(file) = parent.open_attachment_nofollow(std::ffi::OsStr::new("config.json"))
        else {
            return Ok(Self::empty_document());
        };
        let mut file = file.map_err(|_| storage("MCP 配置不能为链接、reparse或特殊文件"))?;
        use std::io::Read as _;
        let before = file
            .metadata()
            .map_err(|_| storage("MCP 文件属性读取失败"))?;
        if !before.is_file() || before.len() > 1024 * 1024 {
            return Err(storage("MCP 配置必须为1MiB内普通文件"));
        }
        #[cfg(windows)]
        let identity = crate::discovery_tools::windows_readonly::file_identity(&file)
            .map_err(|_| storage("MCP 文件身份读取失败"))?;
        #[cfg(not(windows))]
        let identity = crate::filetools::fsio::file_identity(&file)
            .map_err(|_| storage("MCP 文件身份读取失败"))?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| storage("MCP 文件读取失败"))?;
        let after = file
            .metadata()
            .map_err(|_| storage("MCP 文件属性读取失败"))?;
        #[cfg(windows)]
        let final_identity = crate::discovery_tools::windows_readonly::file_identity(&file)
            .map_err(|_| storage("MCP 文件身份读取失败"))?;
        #[cfg(not(windows))]
        let final_identity = crate::filetools::fsio::file_identity(&file)
            .map_err(|_| storage("MCP 文件身份读取失败"))?;
        if bytes.len() > 1024 * 1024
            || identity != final_identity
            || before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
            || !parent
                .same_directory(path.parent().unwrap())
                .unwrap_or(false)
        {
            return Err(storage("MCP 文件读取期间已改变"));
        }
        Self::normalize_document(
            serde_json::from_slice(&bytes).map_err(|_| storage("MCP 配置损坏，拒绝覆盖"))?,
        )
    }
    fn empty_document() -> Value {
        json!({"schemaVersion":1,"global":{"mcp":{"enabled":false,"connectors":[]}},"agents":{},"sessions":{}})
    }
    fn read_versioned(&self) -> Result<(Value, FileVersion)> {
        self.read()?;
        let parent = self.directory()?;
        let path = self.path();
        let snapshot = match std::fs::symlink_metadata(&path) {
            Ok(_) => Some(
                Snapshot::read_bounded(&path, 1024 * 1024)
                    .map_err(|_| storage("MCP 文件必须为原目录中的有界普通文件"))?,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(storage("MCP 配置读取失败")),
        };
        let value = Self::normalize_document(match &snapshot {
            Some(snapshot) => serde_json::from_slice(&snapshot.bytes)
                .map_err(|_| storage("MCP 配置损坏，拒绝覆盖"))?,
            None => Self::empty_document(),
        })?;
        if !parent
            .same_directory(path.parent().unwrap())
            .unwrap_or(false)
        {
            return Err(storage("MCP 父目录身份已改变"));
        }
        Ok((value, FileVersion { snapshot, parent }))
    }
    fn normalize_document(mut value: Value) -> Result<Value> {
        if value.is_object()
            && !["schemaVersion", "global", "agents", "sessions"]
                .iter()
                .any(|key| value.get(key).is_some())
        {
            // 现役旧版配置把全局桶直接放在根部；迁移保留其完整内容。
            value = json!({"schemaVersion":1,"global":value,"agents":{},"sessions":{}});
        }
        if value
            .get("schemaVersion")
            .is_some_and(|v| v.as_u64() != Some(1))
            || ["agents", "sessions"]
                .iter()
                .any(|key| value.get(key).is_some_and(|v| !v.is_object()))
        {
            return Err(storage("MCP 配置版本或归属桶格式无效"));
        }
        if !value.is_object()
            || value.get("global").is_some_and(|v| !v.is_object())
            || value.pointer("/global/mcp").is_some_and(|v| !v.is_object())
        {
            return Err(storage("MCP 配置必须保留合法的 global.mcp 对象"));
        }
        Ok(value)
    }
    fn config(document: &Value) -> Result<Value> {
        let mut config = document
            .pointer("/global/mcp")
            .cloned()
            .unwrap_or(json!({}));
        let connectors = config
            .get("connectors")
            .or_else(|| config.get("servers"))
            .cloned()
            .unwrap_or(json!([]));
        if !connectors.is_array() {
            return Err(storage("MCP connectors/servers 必须为数组"));
        }
        for field in ["enabled", "deferEnabled", "builtinDeferEnabled"] {
            if config.get(field).is_some_and(|v| !v.is_boolean()) {
                return Err(storage("MCP 全局开关必须为布尔值"));
            }
        }
        if config
            .get("deferThreshold")
            .is_some_and(|v| !v.as_u64().is_some_and(|n| n > 0 && n <= 9007199254740991))
        {
            return Err(storage("MCP 延迟加载阈值无效"));
        }
        let mut ids = BTreeSet::new();
        for connector in connectors.as_array().unwrap() {
            Self::validate(connector).map_err(|_| storage("已保存的MCP连接器配置无效"))?;
            let id = string(&connector["id"], "connector.id")?;
            validate_id(&id)?;
            if !ids.insert(id.to_ascii_lowercase()) {
                return Err(storage("MCP 连接器标识重复"));
            }
        }
        config["connectors"] = connectors;
        Ok(config)
    }
    fn persist(&self, document: &mut Value, config: &Value, original: &FileVersion) -> Result<()> {
        let _writer = self
            .source_write
            .lock()
            .map_err(|_| storage("MCP Source写锁损坏"))?;
        self.persist_locked(document, config, original, &|| true)
    }
    fn persist_locked(
        &self,
        document: &mut Value,
        config: &Value,
        original: &FileVersion,
        extra_authorize: &dyn Fn() -> bool,
    ) -> Result<()> {
        if document.get("global").is_none() {
            document["global"] = json!({})
        }
        let mut config = config.clone();
        // 保留旧别名并保持同一内容，避免降级读取 servers 时把已删除项复活。
        if config.get("servers").is_some() {
            config["servers"] = config["connectors"].clone()
        }
        document["global"]["mcp"] = config;
        let bytes =
            serde_json::to_vec_pretty(document).map_err(|_| storage("MCP 配置序列化失败"))?;
        if bytes.len() > 1024 * 1024 {
            return Err(invalid("MCP 配置超过1MiB保存上限"));
        }
        let path = self.path();
        let authorize = || {
            if !extra_authorize() {
                return false;
            }
            let ancestry = self.plugin_handle.get().is_some_and(|plugin| {
                plugin
                    .same_directory(&self.memory.home().join("plugin-data"))
                    .unwrap_or(false)
            }) && self.read_plugin.get().is_none_or(|plugin| {
                plugin
                    .same_directory(&self.memory.home().join("plugin-data"))
                    .unwrap_or(false)
            });
            ancestry
                && self
                    .home_handle
                    .as_ref()
                    .is_some_and(|root| root.same_directory(self.memory.home()).unwrap_or(false))
                && original
                    .parent
                    .same_directory(path.parent().unwrap())
                    .unwrap_or(false)
        };
        let result = match &original.snapshot {
            Some(snapshot) => {
                self.file_safety
                    .replace_private("mcp-settings", &path, snapshot, &bytes, authorize)
            }
            None => self
                .file_safety
                .create("mcp-settings", &path, &bytes, authorize),
        };
        if result.is_ok() || matches!(&result, Err(WriteError::Unknown(_))) {
            self.settings_epoch.fetch_add(1, Ordering::SeqCst);
        }
        result.map_err(|error| match error {
            WriteError::Unknown(_) => McpSettingsError::new(
                500,
                "mcp_write_unconfirmed",
                "MCP 保存可能已发生，结果尚未确认",
            ),
            WriteError::Conflict => {
                McpSettingsError::new(409, "mcp_file_changed", "MCP 配置或目录已改变，请重新操作")
            }
            _ => storage("MCP 安全保存失败"),
        })
    }
    fn gateway(&self) -> Result<Arc<ToolInvocationGateway>> {
        self.gateway
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or_else(|| McpSettingsError::new(503, "mcp_gateway_unavailable", "工具网关未接通"))
    }
    fn agent(&self, agent: &str) -> Result<Value> {
        let dir = self
            .memory
            .agent_dir_checked(agent)
            .map_err(|_| invalid("目标助手不存在"))?;
        am::config::read_yaml_mapping_as_json(&dir.join("config.yaml"))
            .map_err(|_| storage("目标助手配置损坏"))?
            .ok_or_else(|| invalid("目标助手不存在"))
    }
    fn allowed(&self, agent: &str, id: &str, tool: Option<&str>) -> Result<()> {
        let config = Self::config(&self.read()?)?;
        if config["enabled"] != true
            || !self
                .live
                .read()
                .map_err(|_| storage("MCP 状态锁损坏"))?
                .contains(id)
        {
            return Err(McpSettingsError::new(
                409,
                "mcp_not_running",
                "MCP 连接器当前未运行",
            ));
        }
        let connector = config["connectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .ok_or_else(|| invalid("MCP 连接器不存在"))?;
        if connector["enabled"] == false {
            return Err(McpSettingsError::new(
                403,
                "mcp_connector_disabled",
                "MCP 连接器已停用",
            ));
        }
        let agent = self.agent(agent)?;
        let mcp = agent.get("mcp").cloned().unwrap_or(json!({}));
        if !mcp.is_object() {
            return Err(storage("助手MCP配置不是对象"));
        }
        let current = mcp.get("connectors").and_then(|v| v.get(id));
        let legacy = mcp.get("servers").and_then(|v| v.get(id));
        for entry in [current, legacy].into_iter().flatten() {
            if !entry.is_object()
                || entry.get("enabled").is_some_and(|v| !v.is_boolean())
                || entry.get("tools").is_some_and(|v| !v.is_object())
            {
                return Err(storage("助手MCP开关格式无效"));
            }
        }
        if current
            .and_then(|v| v.get("enabled"))
            .or_else(|| legacy.and_then(|v| v.get("enabled")))
            == Some(&Value::Bool(false))
        {
            return Err(McpSettingsError::new(
                403,
                "mcp_agent_disabled",
                "当前助手已禁用该MCP连接器",
            ));
        }
        if let Some(tool) = tool {
            let enabled = current
                .and_then(|v| v.get("tools"))
                .and_then(|v| v.get(tool))
                .or_else(|| {
                    legacy
                        .and_then(|v| v.get("tools"))
                        .and_then(|v| v.get(tool))
                });
            if enabled.is_some_and(|v| !v.is_boolean()) {
                return Err(storage("助手MCP工具开关格式无效"));
            }
            if enabled == Some(&Value::Bool(false)) {
                return Err(McpSettingsError::new(
                    403,
                    "mcp_agent_tool_disabled",
                    "当前助手已禁用该MCP工具",
                ));
            }
        }
        Ok(())
    }
    fn endpoint(connector: &Value) -> Result<McpEndpoint> {
        Self::validate_shape(connector)?;
        let transport = connector["transport"].as_str().unwrap_or(
            if connector["command"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
            {
                "stdio"
            } else {
                "remote"
            },
        );
        if transport != "stdio" {
            return Ok(McpEndpoint::Unsupported {
                transport: transport.into(),
            });
        }
        let command = string(&connector["command"], "command")?;
        let args = connector
            .get("args")
            .filter(|v| !v.is_null())
            .map(|v| {
                serde_json::from_value::<Vec<String>>(v.clone())
                    .map_err(|_| invalid("args 必须为文本数组"))
            })
            .transpose()?
            .unwrap_or_default();
        let env = connector
            .get("env")
            .filter(|v| !v.is_null())
            .map(|v| {
                serde_json::from_value::<BTreeMap<String, String>>(v.clone())
                    .map_err(|_| invalid("env 必须为文本映射"))
            })
            .transpose()?
            .unwrap_or_default();
        Ok(McpEndpoint::Stdio {
            command,
            args,
            env,
            cwd: connector["cwd"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        })
    }
    fn validate(connector: &Value) -> Result<()> {
        let _ = Self::endpoint(connector)?;
        let stdio = connector["transport"]
            .as_str()
            .map(|v| v == "stdio")
            .unwrap_or(
                connector["command"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty()),
            );
        if !stdio {
            let raw = connector
                .get("url")
                .or_else(|| connector.get("baseUrl"))
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("url 必需"))?;
            let url = reqwest::Url::parse(raw).map_err(|_| invalid("url 必须为有效HTTP地址"))?;
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(invalid("url 必须为不含用户名和密码的HTTP地址"));
            }
        }
        Ok(())
    }
    fn validate_shape(connector: &Value) -> Result<()> {
        let fields = connector
            .as_object()
            .ok_or_else(|| invalid("connector/patch 必须为对象"))?;
        for (key, value) in fields {
            match key.as_str() {
                "transport" if !value.is_null() => {
                    if !value.as_str().is_some_and(|v| {
                        matches!(v, "stdio" | "remote" | "streamable-http" | "sse")
                    }) {
                        return Err(invalid("transport 必须为 stdio/remote/streamable-http/sse"));
                    }
                }
                "id"
                | "name"
                | "description"
                | "cwd"
                | "command"
                | "url"
                | "baseUrl"
                | "authorizationToken"
                | "authorization_token"
                | "oauthClientId"
                | "oauthClientSecret"
                | "clientId"
                | "clientSecret" => {
                    if !value.is_null() && !value.as_str().is_some_and(|v| !v.contains('\0')) {
                        return Err(invalid(format!("{key} 必须为文本或null")));
                    }
                }
                "enabled" | "autoStart" | "enableGlobal" | "deferEnabled" => {
                    if !value.is_null() && !value.is_boolean() {
                        return Err(invalid(format!("{key} 必须为布尔值或null")));
                    }
                }
                "args" => {
                    if !value.is_null()
                        && !value.as_array().is_some_and(|args| {
                            args.iter()
                                .all(|v| v.as_str().is_some_and(|s| !s.contains('\0')))
                        })
                    {
                        return Err(invalid("args 必须为不含空字符的文本数组或null"));
                    }
                }
                "env" | "headers" => {
                    if !value.is_null()
                        && !masked(value)
                        && !value.as_object().is_some_and(|entries| {
                            entries.iter().all(|(name, value)| {
                                !name.contains('\0')
                                    && !name.contains('=')
                                    && value
                                        .as_str()
                                        .is_some_and(|s| !s.contains('\0') && s.len() <= 8192)
                            })
                        })
                    {
                        return Err(invalid(format!("{key} 必须为有界文本映射或null")));
                    }
                }
                "lifecycle" if !value.is_null() => {
                    if !value.as_str().is_some_and(|mode| {
                        matches!(mode, "eager" | "lazy" | "keep-alive" | "lazy-keep-alive")
                    }) {
                        return Err(invalid(
                            "lifecycle 必须为 eager/lazy/keep-alive/lazy-keep-alive",
                        ));
                    }
                }
                "authType" => {
                    if !value.is_null()
                        && !value
                            .as_str()
                            .is_some_and(|v| matches!(v, "none" | "bearer" | "oauth"))
                    {
                        return Err(invalid("authType 必须为none/bearer/oauth或null"));
                    }
                }
                "oauth" if !value.is_null() && !value.is_object() => {
                    return Err(invalid("oauth 必须为对象或null"));
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn authorize_patch(principal: &Principal, patch: &Value, old: &Value) -> Result<()> {
        if principal.is_local_owner() {
            return Ok(());
        }
        for (key, value) in patch
            .as_object()
            .ok_or_else(|| invalid("连接器补丁必须为对象"))?
        {
            if (secret_field(key)
                || matches!(
                    key.as_str(),
                    "command"
                        | "args"
                        | "cwd"
                        | "transport"
                        | "authType"
                        | "permissionMode"
                        | "toolPermissions"
                        | "trustReadOnlyHint"
                ))
                && value != old.get(key).unwrap_or(&Value::Null)
                && !masked(value)
            {
                return Err(McpSettingsError::new(
                    403,
                    "mcp_boundary_user_only",
                    "命令、环境变量、凭证和授权边界只能由本机拥有者在设置页修改",
                ));
            }
        }
        Ok(())
    }
    fn refresh_model_residents(&self, runtime: &Runtime) -> Result<()> {
        let config = Self::config(&self.read()?)?;
        let gateway = self.gateway()?;
        let live = runtime
            .running
            .values()
            .filter(|entry| entry.server.is_connected())
            .flat_map(|entry| entry.server.registered_targets().into_keys())
            .collect::<BTreeSet<_>>();
        // 现役按全源工具总数决定是否延迟，严格大于门槛才延迟；其他来源不受影响。
        let defer = config["deferEnabled"] != false
            && live.len() as u64 > config["deferThreshold"].as_u64().unwrap_or(4);
        let baseline = self.builtin_residents.get_or_init(|| {
            gateway
                .registry()
                .model_declaration_snapshot()
                .1
                .into_iter()
                .filter(|entry| {
                    matches!(
                        entry.listing.origin,
                        lingxi_kernel::toolcatalog::ToolOrigin::FirstParty
                    )
                })
                .map(|entry| entry.listing.target_id)
                .collect()
        });
        // 只收放本管理者接管前已常驻的内置声明；目录入口永远保留，权限不因声明变化放宽。
        for target in baseline {
            let keep = config["builtinDeferEnabled"] != true
                || ["mcp_search_tools", "mcp_describe_tool", "mcp_call"]
                    .iter()
                    .any(|name| target.as_str() == format!("tool:first-party:{name}"));
            let result = if keep {
                gateway.registry().add_model_resident(target)
            } else {
                gateway.registry().remove_model_resident(target)
            };
            result.map_err(|_| storage("内置工具延迟加载声明更新未确认"))?;
        }
        for entry in runtime.running.values() {
            for target in entry.server.registered_targets().keys() {
                let target_id = ToolTargetId::parse(target);
                let pinned = config["connectors"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|connector| connector["id"] == entry.server.id())
                    .and_then(|connector| connector.get("pinnedTools"))
                    .and_then(|pins| {
                        entry
                            .server
                            .registered_targets()
                            .get(target)
                            .and_then(|name| pins.get(name))
                    })
                    .is_some_and(|value| value == true);
                let resident =
                    config["enabled"] == true && live.contains(target) && (!defer || pinned);
                // SDK目录默认标记Deferred；只有资格列表和实际可用形状一致，模型才能收到声明。
                let availability = if resident {
                    Availability::Available
                } else {
                    Availability::Deferred
                };
                let current = gateway
                    .registry()
                    .describe(&target_id)
                    .map_err(|_| storage("MCP真实工具目录读取失败"))?
                    .availability;
                if current != availability {
                    gateway
                        .registry()
                        .set_availability(&target_id, availability)
                        .map_err(|_| storage("MCP真实工具形状更新未确认"))?;
                }
                let result = if resident {
                    gateway.registry().add_model_resident(&target_id)
                } else {
                    gateway.registry().remove_model_resident(&target_id)
                };
                result.map_err(|_| storage("MCP真实工具声明更新未确认"))?;
            }
        }
        Ok(())
    }
    async fn stop_connector(&self, runtime: &mut Runtime, id: &str) -> Result<()> {
        self.live
            .write()
            .map_err(|_| storage("MCP 状态锁损坏"))?
            .remove(id);
        self.live_servers
            .write()
            .map_err(|_| storage("MCP 连接状态锁损坏"))?
            .remove(id);
        if let Some(registered) = runtime.running.remove(id) {
            let mut failure = None;
            match self.gateway() {
                Ok(gateway) => {
                    for target in registered.server.registered_targets().keys() {
                        if gateway
                            .registry()
                            .uninstall(&ToolTargetId::parse(target))
                            .is_err()
                        {
                            failure = Some(storage("MCP 工具撤销未确认"));
                        }
                    }
                }
                Err(error) => failure = Some(error),
            }
            if let Err(error) = registered.server.disconnect_checked().await {
                self.events.publish(
                    "mcp-cleanup-error",
                    json!({"connectorId":id,"code":error.code(),"message":error.message()}),
                );
                failure = Some(McpSettingsError::new(
                    500,
                    "mcp_cleanup_unconfirmed",
                    format!("MCP 子进程或连接退出未确认：{}", error.message()),
                ));
            }
            if let Err(error) = self.refresh_model_residents(runtime) {
                if failure.is_none() {
                    failure = Some(error);
                }
            }
            if let Some(error) = failure {
                runtime.errors.insert(id.into(), error.message.clone());
                return Err(error);
            }
        }
        Ok(())
    }
    async fn start_connector(
        self: &Arc<Self>,
        runtime: &mut Runtime,
        connector: &Value,
        cancel: Option<&crate::cancel::CancelScope>,
        authority: Option<&SettingsAuthority>,
    ) -> Result<()> {
        let id = string(&connector["id"], "connector.id")?;
        if let Some(registered) = runtime.running.get(&id) {
            if registered.server.is_connected() {
                return Ok(());
            }
            // 已掉线的旧句柄不能被当作启动成功；先撤销旧目录，再建立真实连接。
            self.stop_connector(runtime, &id).await?;
        }
        let registered = match self.connect_connector(connector, cancel, authority).await {
            Ok(registered) => registered,
            Err(error) => {
                runtime.errors.insert(id, error.message.clone());
                return Err(error);
            }
        };
        if let Some(authority) = authority {
            if let Err(error) = authority.validate() {
                runtime.running.insert(id.clone(), registered);
                return match self.stop_connector(runtime, &id).await {
                    Ok(()) => {
                        runtime.errors.insert(id, error.message.clone());
                        Err(error)
                    }
                    Err(cleanup) => Err(cleanup),
                };
            }
        }
        self.live
            .write()
            .map_err(|_| storage("MCP 状态锁损坏"))?
            .insert(id.clone());
        runtime.errors.remove(&id);
        runtime.running.insert(id, registered);
        self.refresh_model_residents(runtime)?;
        Ok(())
    }
    fn queue_start(
        self: &Arc<Self>,
        connector: Value,
        authority: Option<&SettingsAuthority>,
        parent: Option<&crate::cancel::CancelScope>,
    ) -> Result<()> {
        let authority = authority.cloned().ok_or_else(|| {
            McpSettingsError::new(
                503,
                "mcp_start_authority_unavailable",
                "已保存，但后台启动缺少原始授权",
            )
        })?;
        authority.validate()?;
        let tasks = self.tasks.get().and_then(Weak::upgrade).ok_or_else(|| {
            McpSettingsError::new(
                503,
                "mcp_start_supervisor_unavailable",
                "已保存，但后台任务监管者不可用",
            )
        })?;
        let id = string(&connector["id"], "connector.id")?;
        let sequence = self.pending_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let cancel = match parent {
            Some(parent) => parent.child(
                format!("mcp-start:{id}"),
                crate::cancel::ScopeKind::Background,
            ),
            None => crate::cancel::CancelScope::background_root(&format!("mcp-start:{id}")),
        };
        {
            let mut pending = self
                .pending_starts
                .write()
                .map_err(|_| storage("MCP 启动队列锁损坏"))?;
            if pending.contains_key(&id) {
                return Ok(());
            }
            if self.closing.load(Ordering::SeqCst) {
                return Err(McpSettingsError::new(409, "mcp_stopping", "服务正在退出"));
            }
            if self.start_pauses.load(Ordering::SeqCst) != 0 {
                return Err(McpSettingsError::new(
                    409,
                    "mcp_start_paused",
                    "设置已保存，MCP 正在停用，未接受新的后台启动",
                ));
            }
            pending.insert(
                id.clone(),
                PendingStart {
                    sequence,
                    cancel: cancel.clone(),
                },
            );
        }
        let weak = Arc::downgrade(self);
        let guard = PendingStartGuard {
            manager: weak.clone(),
            id: id.clone(),
            sequence,
        };
        tasks
            .spawn_detached(format!("mcp-start:{id}:{sequence}"), async move {
                let _guard = guard;
                if let Some(manager) = weak.upgrade() {
                    manager
                        .finish_queued_start(connector, sequence, cancel, authority)
                        .await;
                }
            })
            .map_err(|_| {
                McpSettingsError::new(
                    503,
                    "mcp_start_queue_rejected",
                    "配置已保存，后台启动队列未接受任务",
                )
            })?;
        Ok(())
    }
    fn validate_pending_start(
        &self,
        connector: &Value,
        sequence: u64,
        cancel: &crate::cancel::CancelScope,
        authority: &SettingsAuthority,
    ) -> Result<()> {
        authority.validate()?;
        if self.closing.load(Ordering::SeqCst) || cancel.is_cancelled() {
            return Err(McpSettingsError::new(
                409,
                "mcp_cancelled",
                "后台启动已取消；已保存的配置保留",
            ));
        }
        let id = connector["id"].as_str().unwrap();
        if !self
            .pending_starts
            .read()
            .map_err(|_| storage("MCP 启动队列锁损坏"))?
            .get(id)
            .is_some_and(|job| job.sequence == sequence)
        {
            return Err(McpSettingsError::new(
                409,
                "mcp_start_changed",
                "后台启动已被替换",
            ));
        }
        let config = Self::config(&self.read()?)?;
        if config["enabled"] != true
            || !config["connectors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|current| current == connector)
        {
            return Err(McpSettingsError::new(
                409,
                "mcp_start_changed",
                "配置已改变；未沿用旧启动请求",
            ));
        }
        Ok(())
    }
    async fn finish_queued_start(
        self: &Arc<Self>,
        connector: Value,
        sequence: u64,
        cancel: crate::cancel::CancelScope,
        authority: SettingsAuthority,
    ) {
        let id = connector["id"].as_str().unwrap().to_owned();
        let initial = {
            let runtime = self.runtime.lock().await;
            self.validate_pending_start(&connector, sequence, &cancel, &authority)
                .map(|()| {
                    runtime
                        .running
                        .get(&id)
                        .is_some_and(|r| r.server.is_connected())
                })
        };
        let connected = match initial {
            Ok(true) => None,
            Ok(false) => Some(
                self.connect_connector(&connector, Some(&cancel), Some(&authority))
                    .await,
            ),
            Err(error) => Some(Err(error)),
        };
        let mut runtime = self.runtime.lock().await;
        let result = match connected {
            None => self.validate_pending_start(&connector, sequence, &cancel, &authority),
            Some(Err(error)) => Err(error),
            Some(Ok(registered)) => {
                // 同连接器的新启动必须等待旧任务完成清理，不能与旧目录交叉覆盖。
                let validation =
                    self.validate_pending_start(&connector, sequence, &cancel, &authority);
                runtime.running.insert(id.clone(), registered);
                if let Err(error) = validation {
                    match self.stop_connector(&mut runtime, &id).await {
                        Ok(()) => Err(error),
                        Err(cleanup) => Err(cleanup),
                    }
                } else {
                    self.live
                        .write()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(id.clone());
                    runtime.errors.remove(&id);
                    self.refresh_model_residents(&runtime)
                }
            }
        };
        let receipt = match result {
            Ok(()) => json!(McpSettingsResult {
                settings_update: McpSettingsUpdate {
                    status: McpSettingsStatus::Applied,
                    action: "mcp.connector.start".into(),
                    key: "mcp.connector.start".into(),
                    title: "后台连接已启动".into(),
                    summary: format!("{id} 已完成真实连接和工具目录同步"),
                    saved: true,
                    runtime_applied: true,
                    connector_id: Some(id.clone()),
                    agent_id: None,
                    changes: vec![],
                    error: None,
                }
            }),
            Err(mut error) => {
                runtime.errors.insert(id.clone(), error.message.clone());
                error.saved = true;
                let mut value = error.value("mcp.connector.start");
                value["settingsUpdate"]["connectorId"] = json!(id);
                value
            }
        };
        runtime.receipts.push_back(receipt.clone());
        if runtime.receipts.len() > 50 {
            runtime.receipts.pop_front();
        }
        self.events.publish("mcp-settings-result", receipt);
    }
    async fn wait_pending(&self, id: Option<&str>) -> Result<()> {
        let wait = async {
            loop {
                let changed = self.pending_changed.notified();
                let exists = {
                    let pending = self
                        .pending_starts
                        .read()
                        .unwrap_or_else(|p| p.into_inner());
                    match id {
                        Some(id) => pending.contains_key(id),
                        None => !pending.is_empty(),
                    }
                };
                if !exists {
                    break;
                }
                changed.await;
            }
        };
        tokio::time::timeout(CONNECT_BUDGET + std::time::Duration::from_secs(5), wait)
            .await
            .map_err(|_| {
                McpSettingsError::new(
                    500,
                    "mcp_cleanup_unconfirmed",
                    "旧后台启动结束尚未确认，未继续修改配置",
                )
            })
    }
    // 握手期间不占设置写锁；目录仅在复验后进入当前运行态。
    async fn connect_connector(
        self: &Arc<Self>,
        connector: &Value,
        cancel: Option<&crate::cancel::CancelScope>,
        authority: Option<&SettingsAuthority>,
    ) -> Result<RegisteredMcpServer> {
        let id = string(&connector["id"], "connector.id")?;
        let gateway = self.gateway()?;
        let deadline = tokio::time::Instant::now() + CONNECT_BUDGET;
        let endpoint = tokio::select! {
            result=tokio::time::timeout_at(deadline,oauth::runtime_endpoint(self,connector))=>match result {Ok(result)=>result?,Err(_)=>return Err(McpSettingsError::new(504,"mcp_connect_timeout","MCP连接准备超时；外端结果未知"))},
            _=async {match cancel {Some(scope)=>scope.cancelled().await,None=>std::future::pending().await}}=>return Err(McpSettingsError::new(409,"mcp_cancelled","MCP连接准备已取消")),
            error=async {loop {if let Some(authority)=authority {if let Err(error)=authority.validate(){break error;}tokio::time::sleep(std::time::Duration::from_millis(50)).await;}else{std::future::pending::<()>().await;}}}=>return Err(error),
            _=async {if !self.closing.load(Ordering::SeqCst){self.stop.notified().await;}}=>return Err(McpSettingsError::new(409,"mcp_stopping","服务正在退出")),
        };
        let server = McpServer::new(id.clone(), endpoint);
        self.live_servers
            .write()
            .map_err(|_| storage("MCP 连接状态锁损坏"))?
            .insert(id.clone(), Arc::downgrade(&server));
        if self.closing.load(Ordering::SeqCst) {
            return Err(McpSettingsError::new(409, "mcp_stopping", "服务正在退出"));
        }
        let budget = SchemaBudget::default();
        let epoch = self.connector_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let weak = Arc::downgrade(self);
        let binder_id = id.clone();
        let binder_server = server.clone();
        let connect = mcpbridge::register_mcp_server_with_binding(
            server.clone(),
            gateway.registry(),
            &gateway,
            &budget,
            move |gateway, target, inner| {
                let Some(manager) = weak.upgrade() else {
                    return;
                };
                let Some(tool) = binder_server
                    .registered_targets()
                    .get(target.as_str())
                    .cloned()
                else {
                    return;
                };
                let wrapper = Arc::new(ScopedExecutor {
                    manager: weak.clone(),
                    connector: binder_id.clone(),
                    tool: tool.clone(),
                    inner: inner.clone(),
                });
                let review_manager = weak.clone();
                let connector_id = binder_id.clone();
                gateway.bind_executor_with_review(target.clone(),wrapper,Arc::new(move|input,_args|{
                let manager=review_manager.upgrade().ok_or("MCP 管理器已退出")?;
                manager.allowed(input.agent_id,&connector_id,Some(&tool)).map_err(|e|e.to_string())?;
                Ok(json!({"summary":format!("助手 {} 调用 MCP 连接器 {} 的工具 {}",input.agent_id,connector_id,tool),"connectorId":connector_id,"toolName":tool,"agentId":input.agent_id,"sourceSessionId":input.session_id,"connectorEpoch":epoch.to_string()}))
            }),"MCP：首次绑定即冻结助手权限的官方SDK执行器");
                let observer = manager
                    .registration_observer
                    .read()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                if let Some(observer) = observer {
                    observer(target);
                }
            },
        );
        let cancelled = async {
            match cancel {
                Some(cancel) => cancel.cancelled().await,
                None => std::future::pending().await,
            }
        };
        let revoked = async {
            loop {
                if let Some(authority) = authority {
                    if let Err(error) = authority.validate() {
                        return error;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                } else {
                    std::future::pending::<()>().await;
                }
            }
        };
        let stopping = async {
            if !self.closing.load(Ordering::SeqCst) {
                self.stop.notified().await
            }
        };
        let result = tokio::select! {
            biased;
            error=revoked=>Err(error),
            _=cancelled=>Err(McpSettingsError::new(409,"mcp_cancelled","Run 已取消，MCP 启动未完成")),
            result=tokio::time::timeout_at(deadline,connect)=>match result{Ok(Ok(registered))=>Ok(registered),Ok(Err(error))=>Err(McpSettingsError::new(400,error.code(),if error.code()=="mcp_endpoint_unsupported"{"当前构建尚未接通此MCP传输协议"}else{"MCP 连接或初始化失败"})),Err(_)=>Err(McpSettingsError::new(504,"mcp_connect_timeout","MCP 连接初始化超时，启动意图已保存"))},
            _=stopping=>Err(McpSettingsError::new(409,"mcp_stopping","服务正在退出，未完成的 MCP 启动已取消")),
        };
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                if let Err(cleanup) = server.disconnect_checked().await {
                    let message = format!("MCP 初始化取消后清理未确认：{}", cleanup.message());
                    self.events.publish(
                        "mcp-cleanup-error",
                        json!({"connectorId":id,"code":cleanup.code(),"message":cleanup.message()}),
                    );
                    return Err(McpSettingsError::new(
                        500,
                        "mcp_cleanup_unconfirmed",
                        message,
                    ));
                }
                Err(error)
            }
        }
    }
    pub async fn restore(self: &Arc<Self>) -> Result<()> {
        let config = Self::config(&self.read()?)?;
        let mut runtime = self.runtime.lock().await;
        if config["enabled"] == true {
            for connector in config["connectors"].as_array().unwrap() {
                // 旧配置缺省为 keep-alive；两种按需模式不能在恢复时擅自启动。
                if connector["enabled"] != false
                    && !matches!(
                        connector["lifecycle"].as_str(),
                        Some("lazy" | "lazy-keep-alive")
                    )
                {
                    if let Err(error) = self
                        .start_connector(&mut runtime, connector, None, None)
                        .await
                    {
                        self.events.publish(
                            "mcp-connector-error",
                            json!({"connectorId":connector["id"],"code":error.code}),
                        );
                    }
                }
            }
        }
        if self.gateway.as_ref().and_then(Weak::upgrade).is_some() {
            self.refresh_model_residents(&runtime)?;
        }
        Ok(())
    }
    pub fn begin_stop(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.cancel_pending(None, "服务正在退出");
        oauth::cancel_all(self, "服务正在退出");
        // 同步快照不会被正在等待远端的异步管理请求挡住。
        for server in self
            .live_servers
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter_map(Weak::upgrade)
        {
            server.begin_stop();
        }
        self.stop.notify_waiters();
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.begin_stop();
        self.wait_pending(None).await?;
        oauth::drain(self).await?;
        let mut runtime = self.runtime.lock().await;
        let ids = runtime.running.keys().cloned().collect::<Vec<_>>();
        let mut failure = None;
        for id in ids {
            if let Err(error) = self.stop_connector(&mut runtime, &id).await {
                failure = Some(error)
            }
        }
        failure.map_or(Ok(()), Err)
    }
    pub async fn state(&self) -> Result<Value> {
        self.state_for_agent(None).await
    }
    pub async fn state_for_agent(&self, agent: Option<&str>) -> Result<Value> {
        let runtime = self.runtime.lock().await;
        let mut config = Self::config(&self.read()?)?;
        for connector in config["connectors"].as_array_mut().unwrap() {
            let id = connector["id"].as_str().unwrap().to_string();
            if !connector["name"].is_string() {
                connector["name"] = json!(id);
            }
            if !connector["transport"].is_string() {
                connector["transport"] = json!(if connector["command"]
                    .as_str()
                    .is_some_and(|v| !v.is_empty())
                {
                    "stdio"
                } else {
                    "remote"
                });
            }
            connector["running"] = json!(runtime
                .running
                .get(&id)
                .is_some_and(|r| r.server.is_connected()));
            connector["error"] = runtime
                .errors
                .get(&id)
                .map(|s| json!(s))
                .unwrap_or(Value::Null);
            connector["status"] = json!(if self.has_pending(&id) {
                "connecting"
            } else if connector["error"].is_string() {
                "failed"
            } else if connector["running"] == true {
                "running"
            } else {
                "stopped"
            });
            let mut tools = Vec::new();
            if let Some(registered) = runtime.running.get(&id) {
                for (target, name) in registered.server.registered_targets() {
                    if let Some(manifest) = registered.server.synced_manifest(&target) {
                        tools.push(json!({"name":name,"qualifiedName":target,"targetId":target,"capability":manifest.permission.capability_base,
                            "title":manifest.display_name,"description":manifest.description,
                            "annotations":{"readOnlyHint":matches!(manifest.declared_permission,lingxi_kernel::toolcatalog::DeclaredPermission::ReadOnly)}}));
                    }
                }
                connector["directoryWarnings"] = json!(registered.sync.refused);
            }
            connector["tools"] = json!(tools);
            // 未接通的远程认证不能由可编辑配置自行宣称已登录。
            if connector["authType"] == "oauth" {
                connector["authStatus"] = json!(oauth::credential_status(connector));
            }
        }
        let agent_config = match agent {
            Some(id) => {
                let value = self.agent(id)?.get("mcp").cloned().unwrap_or(json!({}));
                if !value.is_object()
                    || ["connectors", "servers"]
                        .iter()
                        .any(|key| value.get(key).is_some_and(|v| !v.is_object()))
                {
                    return Err(storage("目标助手MCP配置损坏"));
                }
                let mut merged = value.get("servers").cloned().unwrap_or(json!({}));
                merge(
                    &mut merged,
                    &value.get("connectors").cloned().unwrap_or(json!({})),
                );
                for entry in merged.as_object().unwrap().values() {
                    if !entry.is_object()
                        || entry.get("enabled").is_some_and(|v| !v.is_boolean())
                        || entry.get("tools").is_some_and(|v| {
                            !v.as_object()
                                .is_some_and(|m| m.values().all(Value::is_boolean))
                        })
                    {
                        return Err(storage("目标助手MCP工具权限损坏"));
                    }
                }
                let mut entries = merged.as_object().unwrap().iter().map(|(id,entry)|
                    (id.clone(),json!({"enabled":entry["enabled"]!=false,"tools":entry.get("tools").cloned().unwrap_or(json!({}))}))
                ).collect::<serde_json::Map<_,_>>();
                // 页面显示执行器真实的缺省允许规则，未写覆盖不能误显示成禁用。
                for connector in config["connectors"].as_array().unwrap() {
                    let id = connector["id"].as_str().unwrap();
                    let entry = entries
                        .entry(id.to_owned())
                        .or_insert_with(|| json!({"enabled":true,"tools":{}}));
                    for tool in connector["tools"].as_array().unwrap() {
                        if let Some(name) = tool["name"].as_str() {
                            entry["tools"]
                                .as_object_mut()
                                .unwrap()
                                .entry(name.to_owned())
                                .or_insert(json!(true));
                        }
                    }
                }
                json!({"connectors":entries})
            }
            None => json!({"connectors":{}}),
        };
        Ok(json!(McpSettingsState {
            config: public_config(&config, agent.is_some()),
            agent_id: agent.map(str::to_owned),
            agent_config,
            settings_results: runtime.receipts.iter().cloned().collect()
        }))
    }
    /// 对外只接受这个窄主题；真实处理器返回结果后才能称已应用。
    pub async fn request(
        self: &Arc<Self>,
        topic: &str,
        auth: &Arc<AuthService>,
        principal: &Principal,
        agent: Option<&str>,
        action: &str,
        payload: &Value,
    ) -> Result<Value> {
        self.request_inner(
            topic,
            principal,
            agent,
            action,
            payload,
            None,
            None,
            Some(SettingsAuthority {
                auth: auth.clone(),
                original: principal.clone(),
            }),
        )
        .await
    }
    // 身份、目标、冻结审阅、原始授权和取消信号来自不同可信入口，显式传递避免混入模型载荷。
    #[allow(clippy::too_many_arguments)]
    async fn request_inner(
        self: &Arc<Self>,
        topic: &str,
        principal: &Principal,
        agent: Option<&str>,
        action: &str,
        payload: &Value,
        cancel: Option<crate::cancel::CancelScope>,
        expected_review: Option<Value>,
        authority: Option<SettingsAuthority>,
    ) -> Result<Value> {
        if topic != SETTINGS_TOPIC {
            return Err(McpSettingsError::new(
                404,
                "no_handler",
                "没有此设置请求处理器",
            ));
        }
        if principal.kind != PrincipalKind::LocalUser || !principal.has_scope("settings.write") {
            return Err(McpSettingsError::new(
                403,
                "mcp_user_session_required",
                "MCP 设置仅接受有设置权限的本机用户",
            ));
        }
        if !ACTIONS.iter().any(|(key, _, _)| *key == action)
            && !matches!(action, "mcp.global.defer" | "mcp.connector.add_bulk")
        {
            return Err(invalid("未知 MCP 设置动作"));
        }
        if !payload.is_object() {
            return Err(invalid("MCP 设置载荷必须为对象"));
        }
        if self.closing.load(Ordering::SeqCst) {
            return Err(McpSettingsError::new(409, "mcp_stopping", "服务正在退出"));
        }
        // 已经批准的请求独立完成并留可查询回执；客户端断开不能制造半份配置或假成功。
        let manager = self.clone();
        let principal = principal.clone();
        let agent = agent.map(str::to_owned);
        let action = action.to_owned();
        let payload = payload.clone();
        tokio::spawn(async move {
            manager
                .handle(
                    &principal,
                    agent.as_deref(),
                    &action,
                    &payload,
                    cancel.as_ref(),
                    expected_review.as_ref(),
                    authority.as_ref(),
                )
                .await
        })
        .await
        .map_err(|_| storage("MCP 设置执行任务异常，结果未确认"))?
    }
    pub(crate) async fn request_tool(
        self: &Arc<Self>,
        auth: &Arc<AuthService>,
        ctx: &RunContext,
        call: &ToolCallId,
        action: &str,
        value: &Value,
    ) -> std::result::Result<Value, String> {
        let scope = toolgateway::trusted_settings_context(ctx, call)?;
        if !matches!(scope.permission(),InvocationPermissionContext::UserSession{mode}if mode!=SessionPermissionMode::ReadOnly)
        {
            return Err("MCP 设置需要本机可写交互会话".into());
        }
        let principal = auth.mint_local_settings_tool_principal(ctx, call)?;
        let lifetime = toolgateway::TOOL_LIFETIME
            .try_with(Clone::clone)
            .map_err(|_| "MCP 设置缺少真实Run生命周期")?;
        if lifetime.cancel.is_cancelled() {
            return Err("Run 已取消".into());
        }
        let payload = parse_payload(value).map_err(|e| e.to_string())?;
        if payload
            .get("agentId")
            .is_some_and(|id| id != scope.agent_id())
        {
            return Err("模型不能改写冻结的目标助手".into());
        }
        match self
            .request_inner(
                SETTINGS_TOPIC,
                &principal,
                Some(scope.agent_id()),
                action,
                &payload,
                Some(lifetime.cancel),
                Some(toolgateway::review_scope().ok_or("MCP 设置缺少实际冻结审批范围")?),
                Some(SettingsAuthority {
                    auth: auth.clone(),
                    original: scope
                        .settings_principal()
                        .ok_or("缺少原始设置授权")?
                        .clone(),
                }),
            )
            .await
        {
            Ok(value) => Ok(value),
            Err(error) => Ok(error.value(action)),
        }
    }
    pub(crate) fn review_tool(
        &self,
        agent: &str,
        action: &str,
        value: &Value,
    ) -> std::result::Result<Value, String> {
        if !ACTIONS.iter().any(|(key, _, _)| *key == action) {
            return Err("未知 MCP 设置动作".into());
        }
        if self.closing.load(Ordering::SeqCst) {
            return Err("MCP 管理器正在退出".into());
        }
        let agent_config = self.agent(agent).map_err(|e| e.to_string())?;
        let document = self.read().map_err(|e| e.to_string())?;
        let payload = parse_payload(value).map_err(|e| e.to_string())?;
        if payload.get("agentId").is_some_and(|id| id != agent) {
            return Err("模型不能改写冻结的目标助手".into());
        }
        // 私密材料只参与不可逆摘要，绝不进入审批投影或工具返回。
        let revision = lingxi_protocol::canon::canonical_sha256_hex(
            &json!({"config":document,"agent":agent_config}),
        );
        let label = ACTIONS.iter().find(|(key, _, _)| *key == action).unwrap().1;
        let selected = payload
            .get("connectorId")
            .or_else(|| payload.get("id"))
            .or_else(|| payload.get("connector").and_then(|row| row.get("id")));
        let connector = selected
            .map(|id| string(id, "connectorId"))
            .transpose()
            .map_err(|error| error.to_string())?;
        if let Some(id) = &connector {
            validate_id(id).map_err(|error| error.to_string())?;
        }
        let summary = match connector {
            Some(id) => format!("{label}（连接器 {id}；目标助手 {agent}）"),
            None => format!("{label}（目标助手 {agent}）"),
        };
        Ok(
            json!({"summary":summary,"topic":SETTINGS_TOPIC,"action":action,"agentId":agent,"configRevision":revision,"settingsEpoch":self.settings_epoch.load(Ordering::SeqCst).to_string()}),
        )
    }
    fn edited_connector(
        principal: &Principal,
        input: &Value,
        old: &Value,
    ) -> Result<(Value, bool)> {
        let mut patch = input.get("patch").unwrap_or(input).clone();
        Self::validate_shape(&patch)?;
        let fields = patch
            .as_object_mut()
            .ok_or_else(|| invalid("patch 必须为对象"))?;
        for key in [
            "connectorId",
            "serverId",
            "id",
            "patch",
            "value",
            "enabled",
            "autoStart",
        ] {
            fields.remove(key);
        }
        let patch = unmask(old, &patch);
        Self::authorize_patch(principal, &patch, old)?;
        let policy_only = patch.as_object().unwrap().keys().all(|field| {
            matches!(
                field.as_str(),
                "permissionMode" | "toolPermissions" | "trustReadOnlyHint" | "pinnedTools"
            )
        });
        let mut updated = old.clone();
        merge(&mut updated, &patch);
        Self::validate(&updated)?;
        Ok((updated, policy_only))
    }
    #[allow(clippy::too_many_arguments)]
    fn validate_guarded_request(
        &self,
        agent: Option<&str>,
        action: &str,
        input: &Value,
        cancel: Option<&crate::cancel::CancelScope>,
        expected_review: Option<&Value>,
        authority: Option<&SettingsAuthority>,
    ) -> Result<()> {
        if let Some(authority) = authority {
            authority.validate()?;
        }
        if self.closing.load(Ordering::SeqCst) {
            return Err(McpSettingsError::new(409, "mcp_stopping", "服务正在退出"));
        }
        if cancel.is_some_and(|scope| scope.is_cancelled()) {
            return Err(McpSettingsError::new(
                409,
                "mcp_cancelled",
                "Run 已取消，MCP 设置未执行",
            ));
        }
        if let Some(agent) = agent {
            self.agent(agent)?;
        }
        if let Some(expected) = expected_review {
            let current = self
                .review_tool(
                    agent.ok_or_else(|| invalid("缺少冻结的助手"))?,
                    action,
                    input,
                )
                .map_err(invalid)?;
            if &current != expected {
                return Err(McpSettingsError::new(
                    409,
                    "mcp_approval_changed",
                    "等待期间配置或目标助手已改变，原批准不能继续使用",
                ));
            }
        }
        Ok(())
    }
    // 写锁内分别重验原授权、审批材料和取消状态，沿用独立参数，不把可信控制字段藏入用户设置。
    #[allow(clippy::too_many_arguments)]
    async fn handle(
        self: &Arc<Self>,
        principal: &Principal,
        agent: Option<&str>,
        action: &str,
        input: &Value,
        cancel: Option<&crate::cancel::CancelScope>,
        expected_review: Option<&Value>,
        authority: Option<&SettingsAuthority>,
    ) -> Result<Value> {
        let mut _start_pause = None;
        let interrupts = matches!(
            action,
            "mcp.connector.stop" | "mcp.connector.remove" | "mcp.connector.update"
        ) || (action == "mcp.global.enabled" && !flag(input)?);
        if interrupts {
            // 取消本身也是副作用：先在同一写锁内验证完整目标、输入和冻结批准。
            let _runtime = self.runtime.lock().await;
            self.validate_guarded_request(
                agent,
                action,
                input,
                cancel,
                expected_review,
                authority,
            )?;
            let config = Self::config(&self.read()?)?;
            if action == "mcp.global.enabled" {
                self.start_pauses.fetch_add(1, Ordering::SeqCst);
                _start_pause = Some(StartPauseGuard(&self.start_pauses));
                self.cancel_pending(None, "MCP 已停用");
                self.observe_management_request("mcp.global.disable_pending");
            } else {
                let id = id_of(input)?;
                let connector = config["connectors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|connector| connector["id"] == id)
                    .ok_or_else(|| invalid("MCP 连接器不存在"))?;
                let invalidate_credentials = if action == "mcp.connector.update" {
                    let (_, policy_only) = Self::edited_connector(principal, input, connector)?;
                    !policy_only
                } else {
                    true
                };
                self.cancel_pending(Some(&id), "连接器设置或启用意图已改变");
                oauth::cancel_connector(
                    self,
                    &id,
                    "连接器设置或启用意图已改变",
                    invalidate_credentials,
                );
            }
        }
        match action {
            "mcp.global.enabled" if !flag(input)? => self.wait_pending(None).await?,
            "mcp.connector.stop"
            | "mcp.connector.remove"
            | "mcp.connector.update"
            | "mcp.connector.start"
            | "mcp.connector.refresh_tools" => self.wait_pending(Some(&id_of(input)?)).await?,
            _ => {}
        }
        let mut runtime = self.runtime.lock().await;
        self.validate_guarded_request(agent, action, input, cancel, expected_review, authority)?;
        let mut result = self
            .apply(
                &mut runtime,
                principal,
                agent,
                action,
                input,
                cancel,
                authority,
            )
            .await;
        if cancel.is_some_and(|cancel| cancel.is_cancelled()) {
            if let Ok(value) = &result {
                let mut error = McpSettingsError::new(
                    409,
                    "mcp_cancelled",
                    "Run 已取消；已完成的保存和运行时变更保留在回执中",
                );
                error.saved = value["settingsUpdate"]["saved"] == true;
                error.runtime_applied = value["settingsUpdate"]["runtimeApplied"] == true;
                result = Err(error);
            }
        }
        let receipt = match &result {
            Ok(value) => value.clone(),
            Err(error) => error.value(action),
        };
        runtime.receipts.push_back(receipt.clone());
        if runtime.receipts.len() > 50 {
            runtime.receipts.pop_front();
        }
        self.events.publish("mcp-settings-result", receipt);
        result
    }
    #[allow(clippy::too_many_arguments)]
    async fn apply(
        self: &Arc<Self>,
        runtime: &mut Runtime,
        principal: &Principal,
        agent: Option<&str>,
        action: &str,
        input: &Value,
        cancel: Option<&crate::cancel::CancelScope>,
        authority: Option<&SettingsAuthority>,
    ) -> Result<Value> {
        let (mut document, original) = self.read_versioned()?;
        let mut config = Self::config(&document)?;
        let previous_config = config.clone();
        let mut previous_agent_value = None;
        let mut connection_changed = false;
        let previously_running = id_of(input).ok().is_some_and(|id| {
            runtime
                .running
                .get(&id)
                .is_some_and(|r| r.server.is_connected())
        });
        let mut changed = false;
        let mut runtime_applied = false;
        let mut id = None;
        match action {
            "mcp.global.defer" => {
                self.gateway()?;
                let patch = input
                    .as_object()
                    .ok_or_else(|| invalid("延迟加载设置必须为对象"))?;
                if patch.is_empty()
                    || patch.keys().any(|key| {
                        !matches!(
                            key.as_str(),
                            "deferEnabled" | "deferThreshold" | "builtinDeferEnabled"
                        )
                    })
                {
                    return Err(invalid("只接受明确提供的延迟加载字段"));
                }
                for (key, value) in patch {
                    if key == "deferThreshold" {
                        if !value
                            .as_u64()
                            .is_some_and(|n| n > 0 && n <= 9007199254740991)
                        {
                            return Err(invalid("延迟阈值必须为正整数"));
                        }
                    } else if !value.is_boolean() {
                        return Err(invalid("延迟开关必须为布尔值"));
                    }
                    config[key] = value.clone();
                }
                self.persist(&mut document, &config, &original)?;
                changed = true;
                self.refresh_model_residents(runtime)
                    .map_err(McpSettingsError::saved)?;
                runtime_applied = true;
            }
            "mcp.connector.add_bulk" => {
                let rows = input
                    .get("connectors")
                    .and_then(Value::as_array)
                    .filter(|rows| !rows.is_empty() && rows.len() <= 64)
                    .ok_or_else(|| invalid("导入须提供1至64个连接器"))?;
                let mut added = Vec::new();
                // 每一项先在临时副本内完整验证；任何一项错误都不得留下半批配置。
                for (index, row) in rows.iter().enumerate() {
                    let prepared: Result<Value> = (|| {
                        Self::validate_shape(row)?;
                        Self::authorize_patch(principal, row, &json!({}))?;
                        let mut connector = unmask(&json!({}), row);
                        let fields = connector
                            .as_object_mut()
                            .ok_or_else(|| invalid("导入连接器必须为对象"))?;
                        for key in ["connectorId", "serverId", "enableGlobal", "value"] {
                            fields.remove(key);
                        }
                        let raw = connector
                            .get("id")
                            .or_else(|| connector.get("name"))
                            .and_then(Value::as_str)
                            .filter(|v| !v.is_empty())
                            .unwrap_or("connector");
                        let base = raw
                            .chars()
                            .map(|c| {
                                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                                    c
                                } else {
                                    '_'
                                }
                            })
                            .collect::<String>();
                        validate_id(&base)?;
                        let mut next = base.clone();
                        let mut suffix = 2;
                        while config["connectors"].as_array().unwrap().iter().any(|c| {
                            c["id"]
                                .as_str()
                                .is_some_and(|s| s.eq_ignore_ascii_case(&next))
                        }) {
                            next = format!("{base}_{suffix}");
                            suffix += 1;
                        }
                        connector["id"] = json!(next);
                        connector["enabled"] = json!(connector["enabled"] != false);
                        connector["autoStart"] = connector["enabled"].clone();
                        Self::validate(&connector)?;
                        Ok(connector)
                    })();
                    let connector = match prepared {
                        Ok(value) => value,
                        Err(mut error) => {
                            error.results = rows
                                .iter()
                                .enumerate()
                                .map(|(row, _)| McpConnectorImportResult {
                                    ok: false,
                                    id: None,
                                    error: Some(if row == index {
                                        error.message.clone()
                                    } else {
                                        "整批预检失败，此项未保存".into()
                                    }),
                                    saved: false,
                                    runtime_applied: false,
                                })
                                .collect();
                            return Err(error);
                        }
                    };
                    added.push(connector.clone());
                    config["connectors"].as_array_mut().unwrap().push(connector);
                }
                self.persist(&mut document, &config, &original)?;
                let mut results = Vec::new();
                for connector in added {
                    let id = connector["id"].as_str().unwrap().to_owned();
                    let should_start = config["enabled"] == true
                        && connector["enabled"] != false
                        && !matches!(
                            connector["lifecycle"].as_str(),
                            Some("lazy" | "lazy-keep-alive")
                        );
                    let result = if should_start {
                        self.queue_start(connector, authority, cancel)
                    } else {
                        Ok(())
                    };
                    results.push(McpConnectorImportResult {
                        ok: result.is_ok(),
                        id: Some(id),
                        error: result.err().map(|e| e.message),
                        saved: true,
                        runtime_applied: !should_start,
                    });
                }
                let applied = results.iter().all(|row| row.runtime_applied);
                let accepted = results.iter().all(|row| row.ok);
                return Ok(json!(McpConnectorBulkResult {
                    settings_update: McpSettingsUpdate {
                        status: if accepted {
                            McpSettingsStatus::Applied
                        } else {
                            McpSettingsStatus::Failed
                        },
                        action: action.into(),
                        key: action.into(),
                        title: if applied {
                            "连接器导入已完成"
                        } else if accepted {
                            "连接器已保存，后台启动已排队"
                        } else {
                            "连接器已保存，部分启动未排队"
                        }
                        .into(),
                        summary: format!(
                            "{}项已保存，{}项后台启动等待完成，{}项运行时处理完成",
                            results.len(),
                            results
                                .iter()
                                .filter(|row| row.ok && !row.runtime_applied)
                                .count(),
                            results.iter().filter(|row| row.runtime_applied).count()
                        ),
                        saved: true,
                        runtime_applied: applied,
                        connector_id: None,
                        agent_id: agent.map(str::to_owned),
                        changes: vec![],
                        error: None
                    },
                    results
                }));
            }
            "mcp.global.enabled" => {
                let enabled = flag(input)?;
                config["enabled"] = json!(enabled);
                self.persist(&mut document, &config, &original)?;
                changed = true;
                if !enabled {
                    for id in runtime.running.keys().cloned().collect::<Vec<_>>() {
                        self.stop_connector(runtime, &id)
                            .await
                            .map_err(McpSettingsError::saved)?
                    }
                }
                runtime_applied = true;
            }
            "mcp.connector.add" => {
                if input
                    .get("startIfEnabled")
                    .is_some_and(|value| !value.is_boolean())
                {
                    return Err(invalid("startIfEnabled须为布尔值"));
                }
                let mut connector = input.get("connector").unwrap_or(input).clone();
                Self::validate_shape(&connector)?;
                if input.get("enableGlobal").is_some_and(|v| !v.is_boolean()) {
                    return Err(invalid("enableGlobal 必须为布尔值"));
                }
                let fields = connector
                    .as_object_mut()
                    .ok_or_else(|| invalid("connector 必须为对象"))?;
                for key in [
                    "connector",
                    "connectorId",
                    "serverId",
                    "enableGlobal",
                    "enabled",
                    "value",
                    "startIfEnabled",
                ] {
                    fields.remove(key);
                }
                Self::authorize_patch(principal, &connector, &json!({}))?;
                let raw = connector
                    .get("id")
                    .or_else(|| connector.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("connector");
                let base = raw
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>();
                validate_id(&base)?;
                let mut next = base.clone();
                let mut index = 2;
                while config["connectors"].as_array().unwrap().iter().any(|c| {
                    c["id"]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case(&next))
                }) {
                    next = format!("{base}_{index}");
                    index += 1
                }
                connector["id"] = json!(next);
                let enabled = input["startIfEnabled"] != true || connector["enabled"] != false;
                connector["enabled"] = json!(enabled);
                connector["autoStart"] = json!(enabled);
                Self::validate(&connector)?;
                config["connectors"]
                    .as_array_mut()
                    .unwrap()
                    .push(connector.clone());
                if input["enableGlobal"] == true {
                    config["enabled"] = json!(true)
                }
                self.persist(&mut document, &config, &original)?;
                changed = true;
                id = Some(next);
                if input["startIfEnabled"] == true
                    && config["enabled"] == true
                    && enabled
                    && !matches!(
                        connector["lifecycle"].as_str(),
                        Some("lazy" | "lazy-keep-alive")
                    )
                {
                    self.queue_start(connector, authority, cancel)
                        .map_err(McpSettingsError::saved)?;
                }
            }
            "mcp.connector.update" => {
                let selected = id_of(input)?;
                let connectors = config["connectors"].as_array_mut().unwrap();
                let old = connectors
                    .iter_mut()
                    .find(|c| c["id"] == selected)
                    .ok_or_else(|| invalid("MCP 连接器不存在"))?;
                let (updated, policy_only) = Self::edited_connector(principal, input, old)?;
                *old = updated;
                // 普通编辑沿旧语义停止旧连接；纯授权/置顶更新不能重启正在执行的外端。
                connection_changed = !policy_only;
                self.persist(&mut document, &config, &original)?;
                changed = true;
                if connection_changed {
                    self.stop_connector(runtime, &selected)
                        .await
                        .map_err(McpSettingsError::saved)?;
                } else {
                    self.refresh_model_residents(runtime)
                        .map_err(McpSettingsError::saved)?;
                }
                id = Some(selected);
                runtime_applied = true;
            }
            "mcp.connector.remove" => {
                let selected = id_of(input)?;
                let connectors = config["connectors"].as_array_mut().unwrap();
                if !connectors.iter().any(|c| c["id"] == selected) {
                    return Err(invalid("MCP 连接器不存在"));
                }
                connectors.retain(|c| c["id"] != selected);
                self.persist(&mut document, &config, &original)?;
                changed = true;
                self.stop_connector(runtime, &selected)
                    .await
                    .map_err(McpSettingsError::saved)?;
                runtime.errors.remove(&selected);
                id = Some(selected);
                runtime_applied = true;
            }
            "mcp.connector.start" | "mcp.connector.stop" => {
                let selected = id_of(input)?;
                let enabled = action.ends_with("start");
                let connector = config["connectors"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|c| c["id"] == selected)
                    .ok_or_else(|| invalid("MCP 连接器不存在"))?;
                connector["enabled"] = json!(enabled);
                connector["autoStart"] = json!(enabled);
                let connector = connector.clone();
                self.persist(&mut document, &config, &original)?;
                changed = true;
                if enabled {
                    if config["enabled"] != true {
                        return Err(McpSettingsError::new(
                            409,
                            "mcp_global_disabled",
                            "启动意图已保存，但 MCP 全局开关仍关闭",
                        )
                        .saved());
                    }
                    self.start_connector(runtime, &connector, cancel, authority)
                        .await
                        .map_err(McpSettingsError::saved)?
                } else {
                    self.stop_connector(runtime, &selected)
                        .await
                        .map_err(McpSettingsError::saved)?
                }
                id = Some(selected);
                runtime_applied = true;
            }
            "mcp.connector.refresh_tools" => {
                let selected = id_of(input)?;
                if config["enabled"] != true {
                    return Err(invalid("MCP 全局开关关闭"));
                }
                if !runtime.running.contains_key(&selected) {
                    let connector = config["connectors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|c| c["id"] == selected)
                        .ok_or_else(|| invalid("MCP 连接器不存在"))?;
                    if connector["enabled"] == false {
                        return Err(invalid("连接器已停用"));
                    }
                    self.start_connector(runtime, connector, cancel, authority)
                        .await?
                } else {
                    // 新连接重新列目录并注册，确保旧准备句柄在刷新后不能继续执行。
                    self.stop_connector(runtime, &selected).await?;
                    let connector = config["connectors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|c| c["id"] == selected)
                        .ok_or_else(|| invalid("MCP 连接器不存在"))?;
                    self.start_connector(runtime, connector, cancel, authority)
                        .await?;
                }
                id = Some(selected);
                runtime_applied = true;
            }
            "mcp.agent.connector.enable" | "mcp.agent.tool.enable" => {
                let target = agent.ok_or_else(|| invalid("该动作必须有冻结的目标助手"))?;
                let selected = id_of(input)?;
                if !config["connectors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["id"] == selected)
                {
                    return Err(invalid("MCP 连接器不存在"));
                }
                let batch_tools = if action == "mcp.agent.tool.enable" {
                    input
                        .get("tools")
                        .map(|value| {
                            let tools = value
                                .as_object()
                                .filter(|tools| !tools.is_empty() && tools.len() <= 1024)
                                .ok_or_else(|| invalid("tools须为1至1024项布尔映射"))?;
                            if tools.iter().any(|(name, value)| {
                                name.is_empty()
                                    || name.len() > 256
                                    || name.contains('\0')
                                    || !value.is_boolean()
                            }) {
                                return Err(invalid("工具名须为有界文本，开关须为布尔值"));
                            }
                            Ok(value.clone())
                        })
                        .transpose()?
                } else {
                    None
                };
                let enabled = if batch_tools.is_some() {
                    true
                } else {
                    flag(input)?
                };
                let before = self.agent(target)?;
                let current = before
                    .pointer("/mcp/connectors")
                    .and_then(|v| v.get(&selected));
                let legacy = before
                    .pointer("/mcp/servers")
                    .and_then(|v| v.get(&selected));
                previous_agent_value = Some(if action == "mcp.agent.tool.enable" {
                    let name = input
                        .get("toolName")
                        .or_else(|| input.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    current
                        .and_then(|v| v.get("tools"))
                        .and_then(|v| v.get(name))
                        .or_else(|| {
                            legacy
                                .and_then(|v| v.get("tools"))
                                .and_then(|v| v.get(name))
                        })
                        .and_then(Value::as_bool)
                        .unwrap_or(true)
                } else {
                    current
                        .and_then(|v| v.get("enabled"))
                        .or_else(|| legacy.and_then(|v| v.get("enabled")))
                        .and_then(Value::as_bool)
                        .unwrap_or(true)
                });
                let mcp = before.get("mcp").cloned().unwrap_or(json!({}));
                if !mcp.is_object() {
                    return Err(storage("助手MCP配置不是对象"));
                }
                for field in ["connectors", "servers"] {
                    if mcp
                        .get(field)
                        .is_some_and(|value| !value.is_null() && !value.is_object())
                    {
                        return Err(storage("助手MCP连接器配置不是对象"));
                    }
                }
                let old = mcp.get("connectors").and_then(|v| v.get(&selected));
                if old.is_some_and(|v| !v.is_object()) {
                    return Err(storage("助手MCP单项配置不是对象"));
                }
                let entry = if action == "mcp.agent.tool.enable" {
                    if let Some(tools) = batch_tools {
                        json!({"tools":tools})
                    } else {
                        let name = string(
                            input
                                .get("toolName")
                                .or_else(|| input.get("name"))
                                .unwrap_or(&Value::Null),
                            "toolName",
                        )?;
                        if old
                            .and_then(|v| v.get("tools"))
                            .is_some_and(|v| !v.is_object())
                        {
                            return Err(storage("助手MCP工具开关不是对象"));
                        }
                        let mut tools = serde_json::Map::new();
                        tools.insert(name, json!(enabled));
                        json!({"tools":tools})
                    }
                } else {
                    json!({"enabled":enabled})
                };
                let mut connectors = serde_json::Map::new();
                connectors.insert(selected.clone(), entry);
                config::update_config_for_agent(
                    &self.memory,
                    Some(target),
                    &json!({"mcp":{"connectors":connectors}}),
                )
                .map_err(|_| storage("助手MCP设置安全保存失败"))?;
                self.settings_epoch.fetch_add(1, Ordering::SeqCst);
                changed = true;
                if let Some(registered) = runtime.running.get(&selected) {
                    let gateway = self.gateway()?;
                    for target in registered.server.registered_targets().keys() {
                        gateway
                            .registry()
                            .set_availability(&ToolTargetId::parse(target), Availability::Available)
                            .map_err(|_| storage("助手 MCP 工具撤销代次未确认").saved())?;
                    }
                }
                if runtime.running.contains_key(&selected) {
                    self.refresh_model_residents(runtime)
                        .map_err(|error| error.saved())?;
                }
                self.events
                    .publish("agent-updated", json!({"agentId":target,"mcp":true}));
                id = Some(selected);
                runtime_applied = true;
            }
            _ => return Err(invalid("未知 MCP 设置动作")),
        }
        let selected = id.as_deref().unwrap_or("MCP");
        let (title, summary, label, before, after) = match action {
            "mcp.global.defer" => (
                "MCP 延迟加载已更新".into(),
                "已保存实际提供的字段并更新真实声明目录。".into(),
                "延迟加载".into(),
                format!(
                    "defer={} threshold={} builtin={}",
                    previous_config["deferEnabled"] != false,
                    previous_config.get("deferThreshold").unwrap_or(&json!(4)),
                    previous_config["builtinDeferEnabled"] == true
                ),
                format!(
                    "defer={} threshold={} builtin={}",
                    config["deferEnabled"] != false,
                    config.get("deferThreshold").unwrap_or(&json!(4)),
                    config["builtinDeferEnabled"] == true
                ),
            ),
            "mcp.global.enabled" => {
                let enabled = flag(input)?;
                (
                    "MCP 全局设置已更新".to_string(),
                    if enabled {
                        "MCP 已启用，可启动已保存的连接器。"
                    } else {
                        "MCP 已关闭，运行中的连接器已停止。"
                    }
                    .to_string(),
                    "MCP".to_string(),
                    (previous_config["enabled"] == true).to_string(),
                    enabled.to_string(),
                )
            }
            "mcp.connector.add" => (
                "连接器已保存".into(),
                if runtime_applied {
                    format!("{selected} 已添加并完成连接。")
                } else {
                    if self.has_pending(selected) {
                        format!("{selected} 已添加，后台启动已排队；请查看后续连接状态。")
                    } else {
                        format!("{selected} 已添加，尚未启动。")
                    }
                },
                selected.into(),
                "未添加".into(),
                if runtime_applied {
                    "已保存并连接"
                } else {
                    "已保存，尚未启动"
                }
                .into(),
            ),
            "mcp.connector.update" => (
                "连接器设置已更新".into(),
                if connection_changed {
                    format!("{selected} 的设置已保存，旧连接已停止；需要时可重新启动。")
                } else {
                    format!("{selected} 的设置已保存，连接保持原状态。")
                },
                selected.into(),
                "原设置".into(),
                if connection_changed {
                    "新设置已保存，旧连接已停止"
                } else {
                    "新设置已保存，连接保持原状态"
                }
                .into(),
            ),
            "mcp.connector.remove" => (
                "连接器已移除".into(),
                format!("{selected} 的配置和运行连接已移除。"),
                selected.into(),
                "已添加".into(),
                "已移除".into(),
            ),
            "mcp.connector.start" => (
                "连接器已启动".into(),
                format!("{selected} 已完成连接并读取工具目录。"),
                selected.into(),
                if previously_running {
                    "运行中"
                } else {
                    "未运行"
                }
                .into(),
                "运行中".into(),
            ),
            "mcp.connector.stop" => (
                "连接器已停用".into(),
                format!("{selected} 已停止，重启应用后仍保持停用。"),
                selected.into(),
                if previously_running {
                    "运行中"
                } else {
                    "未运行"
                }
                .into(),
                "已停用，重启后仍停用".into(),
            ),
            "mcp.connector.refresh_tools" => {
                let count = runtime
                    .running
                    .get(selected)
                    .map(|r| r.server.registered_targets().len())
                    .unwrap_or(0);
                (
                    "工具目录已刷新".into(),
                    format!("已重新读取 {selected} 的 {count} 个工具。"),
                    format!("{selected} 工具目录"),
                    "原目录".into(),
                    format!("{count} 个工具"),
                )
            }
            "mcp.agent.connector.enable" | "mcp.agent.tool.enable" => {
                let enabled = if input.get("tools").is_some() {
                    true
                } else {
                    flag(input)?
                };
                let target = agent.unwrap_or_default();
                let label = if action == "mcp.agent.tool.enable" {
                    if let Some(tools) = input.get("tools").and_then(Value::as_object) {
                        format!("{selected}/{}个工具", tools.len())
                    } else {
                        format!(
                            "{selected}/{}",
                            input
                                .get("toolName")
                                .or_else(|| input.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                        )
                    }
                } else {
                    selected.to_string()
                };
                (
                    "助手的 MCP 设置已更新".into(),
                    format!(
                        "已为助手 {target} {} {label}。",
                        if enabled { "启用" } else { "停用" }
                    ),
                    label,
                    previous_agent_value.unwrap_or(true).to_string(),
                    input
                        .get("tools")
                        .map(Value::to_string)
                        .unwrap_or_else(|| enabled.to_string()),
                )
            }
            _ => return Err(invalid("未知MCP设置动作")),
        };
        Ok(json!(McpSettingsResult {
            settings_update: McpSettingsUpdate {
                status: McpSettingsStatus::Applied,
                action: action.into(),
                key: action.into(),
                title,
                summary,
                saved: changed,
                runtime_applied,
                connector_id: id,
                agent_id: agent.map(str::to_owned),
                changes: vec![McpSettingsChange {
                    key: action.into(),
                    label,
                    before,
                    after
                }],
                error: None
            }
        }))
    }
}

pub fn parse_payload(value: &Value) -> Result<Value> {
    match value {
        Value::Object(_) => Ok(value.clone()),
        Value::Bool(v) => Ok(json!({"enabled":v,"value":v})),
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                Ok(json!({}))
            } else if s == "true" || s == "false" {
                Ok(json!({"enabled":s=="true","value":s=="true"}))
            } else if s.starts_with('{') || s.starts_with('[') {
                let value: Value =
                    serde_json::from_str(s).map_err(|_| invalid("MCP 设置JSON格式无效"))?;
                if value.is_object() {
                    Ok(value)
                } else {
                    Ok(json!({"value":value}))
                }
            } else {
                Ok(json!({"value":s}))
            }
        }
        _ => Err(invalid("MCP 设置值必须为文本或对象")),
    }
}

struct ScopedExecutor {
    manager: Weak<McpSettingsManager>,
    connector: String,
    tool: String,
    inner: Arc<mcpbridge::McpToolExecutor>,
}
impl ToolExecutorPort for ScopedExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let allowed = (|| {
                let manager = self
                    .manager
                    .upgrade()
                    .ok_or_else(|| storage("MCP 管理器已退出"))?;
                let scope = toolgateway::trusted_settings_context(ctx, call).map_err(invalid)?;
                manager.allowed(scope.agent_id(), &self.connector, Some(&self.tool))
            })();
            match allowed {
                Ok(()) => self.inner.execute(ctx, call, request).await,
                Err(error) => ToolExecutionResult::of_ctx(
                    ctx,
                    ToolOutcome::Failed {
                        error: ProtocolError::new(ErrorCode::Forbidden, error.message, false),
                    },
                ),
            }
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpStateQuery {
    agent_id: Option<String>,
}
async fn state_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<McpStateQuery>,
) -> Response {
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"local_owner_required"})),
        )
            .into_response();
    }
    match state
        .mcp_settings()
        .state_for_agent(query.agent_id.as_deref())
        .await
    {
        Ok(value) => {
            if state.auth().validate_principal(&principal).is_err() {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({"error":"mcp_original_authority_revoked"})),
                )
                    .into_response();
            }
            Json(value).into_response()
        }
        Err(error) => (
            StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(error.value("mcp.state")),
        )
            .into_response(),
    }
}
async fn action_route(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    body: std::result::Result<Json<McpSettingsRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if !principal.is_local_owner() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"local_owner_required"})),
        )
            .into_response();
    }
    let body = match body {
        Ok(Json(body)) => body,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(invalid("MCP 管理请求格式无效").value("mcp.settings")),
            )
                .into_response()
        }
    };
    let action = body.action.wire_name();
    state.mcp_settings().observe_management_request(action);
    let _authority = state.media().authority_mutation_lease().await;
    let payload = body.payload;
    match state
        .mcp_settings()
        .request(
            SETTINGS_TOPIC,
            &state.auth,
            &principal,
            body.agent_id.as_deref(),
            action,
            &payload,
        )
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(error) => (
            StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(error.value(action)),
        )
            .into_response(),
    }
}
pub fn router() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/mcp/state", get(state_route))
        .route("/lingxi/v1/mcp/settings-action", post(action_route))
        .merge(apps::router())
        .merge(oauth::router())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::future::Future;
    #[tokio::test]
    async fn queued_approved_change_rechecks_actual_source_inside_runtime_lock() {
        let home = std::env::temp_dir().join(format!(
            "lingxi-mcp-queued-review-{}",
            am::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(home.join("agents/a/config.yaml"), "agent:\n  name: a\n").unwrap();
        let home = home.canonicalize().unwrap();
        let layout = crate::prepare_layout(&home).unwrap();
        let auth = Arc::new(AuthService::bootstrap(&layout, "queued-review").unwrap());
        let owner = auth
            .authenticate(
                Some(&format!("Bearer {}", auth.local_token())),
                None,
                false,
                crate::ConnectionKind::Local,
            )
            .unwrap();
        let manager = McpSettingsManager::new(
            Arc::new(MemoryService::new(home.clone(), layout.runtime_dir)),
            Arc::new(AppEventLog::new()),
            None,
        );
        manager.request(SETTINGS_TOPIC, &auth, &owner, None, "mcp.connector.add", &json!({"connector":{"id":"fixture","transport":"remote","url":"http://127.0.0.1:9/mcp"}})).await.unwrap();
        let payload = json!({"connectorId":"fixture"});
        let approved = manager
            .review_tool("a", "mcp.connector.remove", &payload)
            .unwrap();
        let error = {
            let mut guard = manager.runtime.lock().await;
            let queued = manager.handle(
                &owner,
                Some("a"),
                "mcp.connector.remove",
                &payload,
                None,
                Some(&approved),
                None,
            );
            tokio::pin!(queued);
            // 实际poll至Mutex等待，明确证明请求已经排队，不用sleep猜测顺序。
            std::future::poll_fn(|cx| match queued.as_mut().poll(cx) {
                std::task::Poll::Pending => std::task::Poll::Ready(()),
                std::task::Poll::Ready(result) => panic!("持锁期间旧批准不能执行：{result:?}"),
            })
            .await;
            // 在同一真实管理器持锁的写入口改变原材料，模拟先获锁的管理动作。
            manager
                .apply(
                    &mut guard,
                    &owner,
                    None,
                    "mcp.connector.update",
                    &json!({"connectorId":"fixture","patch":{"name":"changed-after-approval"}}),
                    None,
                    None,
                )
                .await
                .unwrap();
            drop(guard);
            queued.await.unwrap_err()
        };
        assert_eq!(error.code, "mcp_approval_changed");
        let current = McpSettingsManager::config(&manager.read().unwrap()).unwrap();
        assert_eq!(current["connectors"][0]["name"], "changed-after-approval");
        assert_eq!(
            current["connectors"][0]["id"], "fixture",
            "旧批准不得移除后来改变的连接器"
        );
        manager.shutdown().await.unwrap();
        drop(manager);
        std::fs::remove_dir_all(home).unwrap();
    }
    #[tokio::test]
    async fn queued_tool_authority_is_revalidated_after_the_real_runtime_lock_wait() {
        let home = std::env::temp_dir().join(format!(
            "lingxi-mcp-queued-authority-{}",
            am::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("agents/a")).unwrap();
        std::fs::write(home.join("agents/a/config.yaml"), "agent:\n  name: a\n").unwrap();
        let home = home.canonicalize().unwrap();
        let layout = crate::prepare_layout(&home).unwrap();
        let auth = Arc::new(AuthService::bootstrap(&layout, "queued-authority").unwrap());
        let owner = auth
            .authenticate(
                Some(&format!("Bearer {}", auth.local_token())),
                None,
                false,
                crate::ConnectionKind::Local,
            )
            .unwrap();
        let manager = McpSettingsManager::new(
            Arc::new(MemoryService::new(home.clone(), layout.runtime_dir.clone())),
            Arc::new(AppEventLog::new()),
            None,
        );
        manager.request(SETTINGS_TOPIC, &auth, &owner, None, "mcp.connector.add", &json!({"connector":{"id":"fixture","transport":"remote","url":"http://127.0.0.1:9/mcp"}})).await.unwrap();
        let payload = json!({"connectorId":"fixture"});
        let approved = manager
            .review_tool("a", "mcp.connector.remove", &payload)
            .unwrap();
        let authority = SettingsAuthority {
            auth: auth.clone(),
            original: owner.clone(),
        };
        let before = std::fs::read(manager.path()).unwrap();
        let error = {
            let guard = manager.runtime.lock().await;
            let queued = manager.handle(
                &owner,
                Some("a"),
                "mcp.connector.remove",
                &payload,
                None,
                Some(&approved),
                Some(&authority),
            );
            tokio::pin!(queued);
            std::future::poll_fn(|cx| match queued.as_mut().poll(cx) {
                std::task::Poll::Pending => std::task::Poll::Ready(()),
                std::task::Poll::Ready(result) => panic!("实际锁未释放：{result:?}"),
            })
            .await;
            let _replacement =
                AuthService::bootstrap(&layout, "replaced-while-waiting-runtime").unwrap();
            drop(guard);
            queued.await.unwrap_err()
        };
        assert_eq!(error.code, "mcp_original_authority_revoked");
        assert!(!error.saved);
        assert_eq!(std::fs::read(manager.path()).unwrap(), before);
        manager.shutdown().await.unwrap();
        drop(manager);
        std::fs::remove_dir_all(home).unwrap();
    }
}
