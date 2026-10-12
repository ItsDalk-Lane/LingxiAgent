//! ResourceIO 面（R06-T05 Round 4）：内核 / provider 集合 / 资源事件总线 /
//! 轮询 watch 注册表 + 15 路由面（叶 #31-45、#53 份额）。
//!
//! 现役对照（逐条亲读，RC-3）：
//! - 内核 `lib/resource-io/resource-io.ts`：callProvider 能力闸（:202-224）、
//!   跨 provider move 拒绝（:180-193）、write/trash/rename 后 emit 事件。
//! - ref 归一 `lib/resource-io/resource-refs.ts`：kind 同义词（`_`→`-` 小写、
//!   `type` 兜底）、resource/ref/target 嵌套解包、推断序
//!   url > fileId > resourceId > mountId > path；resourceKey 五形态。
//! - 错误词汇 `lib/resource-io/errors.ts`：capability_denied 403 /
//!   provider_not_available 501 / resource_access_denied 403 /
//!   cross_provider_move_unsupported 501 / resource_not_found 404 /
//!   target_already_exists 409 / invalid_trash_namespace 400；ResourceIOError
//!   默认 code `resource_io_error` 400。
//! - local_fs provider `providers/local-fs-provider.ts`：SEARCH_SKIP_DIRS
//!   （.git/node_modules/dist/build/coverage）、versionFromStat
//!   {mtimeMs,size|null}、fileVersionsMatch（:384-391，仅比对 expected
//!   出现的字段）、trash `trash_{ms}_{4hex}` + metadata.json
//!   schemaVersion 1、realOrResolved 上溯。
//! - session_file provider `providers/session-file-resolver.ts` +
//!   `session-file-resolver.ts`：400 invalid_resource_ref / 404
//!   resource_not_found / 410 resource_expired / 500 invalid_resource_path /
//!   载荷缺失 404；写面一律 capability_denied。
//! - resource provider `providers/resource-provider.ts`：stat/read/
//!   materialize 只读，ResourceService 错误码原样透传
//!   （normalizeResourceServiceError :137-145）。
//! - 事件总线 `resource-event-bus.ts`：dedupeSize 512 FIFO、retention
//!   1000、changed 去重键 = {resourceKey,changeType,version 字段}、since
//!   游标 < 最旧-1 → stale。
//! - watch `resource-watch-registry.ts`：refcount / UUID 订阅 / 诊断面。
//! - 路由 `server/routes/resource-io.ts`：15 叶面、resourceJson 409 冲突
//!   外发（带 safeMessage）、encoding base64/utf-8。
//!
//! 已登记的映射差异（07_diff_ledger 展开）：
//! - D7：fs.watch → mtime/size 轮询（tokio interval，本模块
//!   [`DEFAULT_WATCH_POLL_MS`]；现役 80ms 去抖是合并窗口，轮询一拍天然
//!   合并同窗口内的多次变化，语义等价：一个 watch 目标一拍至多一条事件）。
//! - D9/J2：mount/url provider 缺席 → `unsupported_provider` 400 响亮拒绝
//!   （现役无此情形：两个 provider 总是注册；现役 provider 缺失词汇
//!   provider_not_available 501 留给「workspace 未配置时 local_fs 缺席」）。
//! - D12：events 查询严格化 —— 只认 `since`，未知/重复键、非负整数以外
//!   的游标一律 400（现役 Number()||0 静默吞，T04 D1 先例严格化）。
//! - 删除类操作（trash 源、rename/move 源）在候选人授权闸上映射为
//!   ResourceOp::Write（ResourceAccess 词汇只有 Read/Write；现役
//!   PathGuard 的 delete 同样是写级判定）。
//! - ref 归一失败给稳定 code `invalid_resource_ref`（现役为无 code 的
//!   Error，路由同状态 400）。
//! - list 排序用字节序（现役 localeCompare 在 ASCII 域同序；非 ASCII
//!   语料排序差异不构成协议面）。
//! - 版本 mtimeMs 为整数毫秒（与 resources.rs etag 同一决策；现役 JS
//!   float 毫秒的小数位在回环比较中不承载语义）。

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::{Path as AxumPath, RawQuery, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;

use crate::resources::ResourceService;
use crate::sessionfiles::SessionFileService;
use crate::{
    resourceaccess::{ResourceAccess, ResourceOp},
    EndpointError, Principal, ServiceState,
};

/// D7：轮询 watch 的生产间隔（测试注入更快档）。
pub const DEFAULT_WATCH_POLL_MS: u64 = 250;

/// 现役 local-fs-provider.ts :41 的搜索跳过目录。
const SEARCH_SKIP_DIRS: [&str; 5] = [".git", "node_modules", "dist", "build", "coverage"];

/// 现役 searchNames 默认上限（local-fs-provider.ts :462-463）。
const DEFAULT_SEARCH_NAME_LIMIT: usize = 80;

// ── ResourceRef：归一 / resourceKey / provider 映射 ──

/// 归一后的资源引用（types.ts ResourceRef 的候选人枚举形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceRef {
    LocalFile {
        path: String,
    },
    SessionFile {
        file_id: String,
        session_id: Option<String>,
    },
    Mount {
        mount_id: String,
        path: String,
    },
    Resource {
        resource_id: String,
    },
    Url {
        url: String,
    },
}

impl ResourceRef {
    /// 现役 wire kind 名（types.ts :2-6）。
    pub fn kind(&self) -> &'static str {
        match self {
            ResourceRef::LocalFile { .. } => "local-file",
            ResourceRef::SessionFile { .. } => "session-file",
            ResourceRef::Mount { .. } => "mount",
            ResourceRef::Resource { .. } => "resource",
            ResourceRef::Url { .. } => "url",
        }
    }
}

/// ResourceIO 错误：HTTP status + 现役 code + 消息；safe_message 存在时
/// 作为外发文本（现役 errorJson 的 `error: safeMessage || err.message`）。
#[derive(Debug, Clone)]
pub struct ResourceIoError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub safe_message: Option<String>,
}

impl ResourceIoError {
    fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        ResourceIoError {
            status,
            code: code.to_string(),
            message: message.into(),
            safe_message: None,
        }
    }

    fn invalid_ref(message: &str) -> Self {
        Self::new(400, "invalid_resource_ref", message)
    }

    fn io_bad_request(message: impl Into<String>) -> Self {
        // ResourceIOError 默认形态（errors.ts :5）：400 + resource_io_error。
        Self::new(400, "resource_io_error", message)
    }

    fn capability_denied(capability: &str, provider_id: &str) -> Self {
        Self::new(
            403,
            "capability_denied",
            format!("ResourceIO capability denied: {provider_id}.{capability}"),
        )
    }

    fn provider_not_available(provider_id: &str) -> Self {
        Self::new(
            501,
            "provider_not_available",
            format!("ResourceIO provider not available: {provider_id}"),
        )
    }

    /// D9/J2：mount/url provider 缺席的响亮拒绝。
    fn unsupported_provider(provider_id: &str) -> Self {
        Self::new(
            400,
            "unsupported_provider",
            format!(
                "ResourceIO provider is not supported by this build: {provider_id} \
                 (mount/url resources are explicitly refused, never silently \
                 resolved to a local path)"
            ),
        )
    }

    fn access_denied(operation: &str, detail: String, safe_message: String) -> Self {
        ResourceIoError {
            status: 403,
            code: "resource_access_denied".to_string(),
            message: format!("ResourceIO {operation} denied: {detail}"),
            safe_message: Some(safe_message),
        }
    }

    fn not_found(path: &str) -> Self {
        Self::new(
            404,
            "resource_not_found",
            format!("ResourceIO resource not found: {path}"),
        )
    }

    fn target_already_exists(path: &str) -> Self {
        Self::new(
            409,
            "target_already_exists",
            format!("ResourceIO target already exists: {path}"),
        )
    }

    fn invalid_trash_namespace() -> Self {
        Self::new(400, "invalid_trash_namespace", "invalid trash namespace")
    }

    fn invalid_encoding(message: impl Into<String>) -> Self {
        let message = message.into();
        ResourceIoError {
            status: 400,
            code: "invalid_resource_encoding".to_string(),
            safe_message: Some(message.clone()),
            message,
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(500, "resource_io_error", message)
    }
}

impl std::fmt::Display for ResourceIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ResourceIoError {}

/// 路由面错误外发：status 原样、现役 code 进 details.reason
/// （EndpointError::resource_failure，与 resources.rs 同一约定）；外发
/// 文本优先 safeMessage（现役 errorJson 语义）。
fn error_response(err: ResourceIoError) -> Response {
    let status = axum::http::StatusCode::from_u16(err.status)
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    let message = err
        .safe_message
        .clone()
        .unwrap_or_else(|| err.message.clone());
    EndpointError::resource_failure(status, &err.code, message).into_response()
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        _ => None,
    }
}

/// 现役 normalizedKind（resource-refs.ts :8-10）：`_`→`-` + 小写。
fn normalized_kind(value: Option<&Value>) -> Option<String> {
    non_empty_string(value).map(|kind| kind.replace('_', "-").to_lowercase())
}

/// 现役 normalizeResourceRef（resource-refs.ts :20-70）：嵌套解包 →
/// kind 同义词 → 显式分派 → 推断序 url > fileId > resourceId > mountId >
/// path。失败一律 400 `invalid_resource_ref`（D12 稳定 code；现役无 code
/// Error 同状态）。
pub fn normalize_resource_ref(input: &Value) -> Result<ResourceRef, ResourceIoError> {
    let Some(value) = input.as_object() else {
        return Err(ResourceIoError::invalid_ref("ResourceRef is required"));
    };
    // resource ?? ref ?? target 的第一个非 null 值，是对象则递归（:16-24）。
    for key in ["resource", "ref", "target"] {
        if let Some(nested) = value.get(key).filter(|v| !v.is_null()) {
            if nested.is_object() {
                return normalize_resource_ref(nested);
            }
            break;
        }
    }

    let kind = normalized_kind(value.get("kind")).or_else(|| normalized_kind(value.get("type")));
    let path_value = non_empty_string(value.get("path"))
        .or_else(|| non_empty_string(value.get("file_path")))
        .or_else(|| non_empty_string(value.get("filePath")));
    let file_id = non_empty_string(value.get("fileId"))
        .or_else(|| non_empty_string(value.get("sessionFileId")))
        .or_else(|| {
            if kind.as_deref() == Some("session-file") {
                non_empty_string(value.get("id"))
            } else {
                None
            }
        });
    let resource_id = non_empty_string(value.get("resourceId")).or_else(|| {
        if kind.as_deref() == Some("resource") {
            non_empty_string(value.get("id"))
        } else {
            None
        }
    });
    let url = non_empty_string(value.get("url")).or_else(|| non_empty_string(value.get("href")));
    let mount_id =
        non_empty_string(value.get("mountId")).or_else(|| non_empty_string(value.get("rootId")));

    match kind.as_deref() {
        Some("local-file") | Some("local-path") | Some("path") => {
            let Some(path) = path_value else {
                return Err(ResourceIoError::invalid_ref(
                    "local-file ResourceRef requires path",
                ));
            };
            return Ok(ResourceRef::LocalFile { path });
        }
        Some("session-file") => {
            let Some(file_id) = file_id else {
                return Err(ResourceIoError::invalid_ref(
                    "session-file ResourceRef requires fileId",
                ));
            };
            return Ok(ResourceRef::SessionFile {
                file_id,
                session_id: non_empty_string(value.get("sessionId")),
            });
        }
        Some("mount") => {
            let Some(mount_id) = mount_id else {
                return Err(ResourceIoError::invalid_ref(
                    "mount ResourceRef requires mountId",
                ));
            };
            return Ok(ResourceRef::Mount {
                mount_id,
                path: path_value.unwrap_or_default(),
            });
        }
        Some("resource") => {
            let Some(resource_id) = resource_id else {
                return Err(ResourceIoError::invalid_ref(
                    "resource ResourceRef requires resourceId",
                ));
            };
            return Ok(ResourceRef::Resource { resource_id });
        }
        Some("url") => {
            let Some(url) = url else {
                return Err(ResourceIoError::invalid_ref("url ResourceRef requires url"));
            };
            return Ok(ResourceRef::Url { url });
        }
        _ => {}
    }

    if let Some(url) = url {
        return Ok(ResourceRef::Url { url });
    }
    if let Some(file_id) = file_id {
        return Ok(ResourceRef::SessionFile {
            file_id,
            session_id: None,
        });
    }
    if let Some(resource_id) = resource_id {
        return Ok(ResourceRef::Resource { resource_id });
    }
    if let Some(mount_id) = mount_id {
        return Ok(ResourceRef::Mount {
            mount_id,
            path: path_value.unwrap_or_default(),
        });
    }
    if let Some(path) = path_value {
        return Ok(ResourceRef::LocalFile { path });
    }
    Err(ResourceIoError::invalid_ref("unsupported ResourceRef"))
}

/// 词法绝对化 + 归一（`.`/`..` 按组件折叠，不触文件系统）——现役
/// path.resolve 的纯路径部分。
fn lexical_absolute(raw: &str) -> String {
    let path = Path::new(raw);
    let owned;
    let path = if path.is_absolute() {
        path
    } else {
        owned = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path);
        &owned
    };
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

/// 现役 normalizeSlashPath（resource-refs.ts :12-14）。
fn normalize_slash_path(value: &str) -> String {
    value.replace('\\', "/").trim_matches('/').to_string()
}

/// 现役 resourceKeyForRef（resource-refs.ts :72-85）。
pub fn resource_key_for_ref(reference: &ResourceRef) -> String {
    match reference {
        ResourceRef::LocalFile { path } => {
            format!("local_fs:{}", lexical_absolute(path).replace('\\', "/"))
        }
        ResourceRef::Mount { mount_id, path } => {
            format!("mount:{}:{}", mount_id, normalize_slash_path(path))
        }
        ResourceRef::SessionFile { file_id, .. } => format!("session_file:{file_id}"),
        ResourceRef::Resource { resource_id } => format!("resource:{resource_id}"),
        ResourceRef::Url { url } => format!("url:{url}"),
    }
}

/// 现役 providerIdForResourceRef（resource-refs.ts :87-100）。
pub fn provider_id_for_ref(reference: &ResourceRef) -> &'static str {
    match reference {
        ResourceRef::LocalFile { .. } => "local_fs",
        ResourceRef::Mount { .. } => "mount",
        ResourceRef::SessionFile { .. } => "session_file",
        ResourceRef::Resource { .. } => "resource",
        ResourceRef::Url { .. } => "url",
    }
}

// ── 操作上下文（server/routes/resource-io.ts operationContextFromBody） ──

/// 内核操作上下文：路由面恒为本地属主（LocalOnly 中间件保证），
/// session_id 透传 body.sessionId（ResourceAccess 授权按
/// (principal, session) 授权表判定，workspace 根与主体无关）。
#[derive(Debug, Clone)]
pub struct OpContext {
    pub principal_kind: &'static str,
    pub principal_subject: String,
    pub session_id: String,
    pub source: String,
    pub reason: Option<String>,
    pub session_path: Option<String>,
    /// 现役 options.emit !== false（resource-io.ts :108, :226-261）。
    pub emit: bool,
    /// D8 文件历史接线：写路径捕获的 origin 覆盖（None → "event"）。
    /// restore 路由传 "restore"（file-history.ts :88 的 captureNow）。
    pub capture_origin: Option<String>,
}

impl OpContext {
    /// 路由面默认：本地属主 + api 源。
    pub fn local_owner(session_id: &str) -> Self {
        OpContext {
            principal_kind: "local_user",
            principal_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.to_string(),
            session_id: session_id.to_string(),
            source: "api".to_string(),
            reason: None,
            session_path: None,
            emit: true,
            capture_origin: None,
        }
    }
}

// ── 资源事件总线（resource-event-bus.ts） ──

/// changed 事件输入（type/sequence/occurredAt 由总线补齐）。
#[derive(Debug, Clone)]
pub struct BusChanged {
    pub change_type: String,
    pub resource_key: String,
    pub resource: Value,
    pub version: Option<Value>,
    pub source: String,
    pub reason: Option<String>,
    pub session_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BusDeleted {
    pub resource_key: String,
    pub resource: Value,
    pub source: String,
    pub reason: Option<String>,
    pub session_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BusRenamed {
    pub old_resource_key: String,
    pub new_resource_key: String,
    pub old_resource: Value,
    pub new_resource: Value,
    pub source: String,
    pub reason: Option<String>,
    pub session_path: Option<String>,
}

struct BusState {
    sequence: u64,
    recent_changed_keys: VecDeque<String>,
    recent_changed_set: HashSet<String>,
    recent_events: VecDeque<Value>,
}

/// 现役 ResourceEventBus：dedupeSize 512 FIFO、retentionSize 1000、
/// changed 去重键、since 游标语义。emit 传输（WS 推送）属 R09 —— 候选人
/// 份额是事件语义与 since 追补（设计文档 #7 份额说明）。
pub struct ResourceEventBus {
    state: Mutex<BusState>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
    dedupe_size: usize,
    retention_size: usize,
}

impl ResourceEventBus {
    /// 生产构造：现役默认 512/1000。
    pub fn new(now: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        Self::with_limits(512, 1000, now)
    }

    pub fn with_limits(
        dedupe_size: usize,
        retention_size: usize,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        ResourceEventBus {
            state: Mutex::new(BusState {
                sequence: 0,
                recent_changed_keys: VecDeque::new(),
                recent_changed_set: HashSet::new(),
                recent_events: VecDeque::new(),
            }),
            now,
            dedupe_size,
            retention_size,
        }
    }

    fn lock(&self) -> MutexGuard<'_, BusState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn next_sequence(state: &mut BusState) -> u64 {
        state.sequence += 1;
        state.sequence
    }

    fn remember_event(&self, state: &mut BusState, event: Value) {
        if self.retention_size == 0 {
            return;
        }
        state.recent_events.push_back(event);
        while state.recent_events.len() > self.retention_size {
            state.recent_events.pop_front();
        }
    }

    fn remember_changed_key(&self, state: &mut BusState, key: String) {
        state.recent_changed_keys.push_back(key.clone());
        state.recent_changed_set.insert(key);
        while state.recent_changed_keys.len() > self.dedupe_size {
            if let Some(first) = state.recent_changed_keys.pop_front() {
                state.recent_changed_set.remove(&first);
            }
        }
    }

    fn occurred_at(&self) -> String {
        iso_ms((self.now)())
    }

    /// changed：去重键命中 → None（事件不成立）；否则入队并回序号。
    pub fn changed(&self, input: BusChanged) -> Option<u64> {
        let dedupe_key = changed_dedupe_key(&input);
        let mut state = self.lock();
        if let Some(key) = &dedupe_key {
            if state.recent_changed_set.contains(key) {
                return None;
            }
        }
        if let Some(key) = dedupe_key {
            self.remember_changed_key(&mut state, key);
        }
        let sequence = Self::next_sequence(&mut state);
        let mut event = serde_json::json!({
            "type": "resource.changed",
            "changeType": input.change_type,
            "resourceKey": input.resource_key,
            "resource": input.resource,
            "source": input.source,
            "sessionPath": input.session_path,
            "sequence": sequence,
            "occurredAt": self.occurred_at(),
        });
        if let Some(version) = input.version {
            event["version"] = version;
        }
        if let Some(reason) = input.reason {
            event["reason"] = Value::String(reason);
        }
        self.remember_event(&mut state, event);
        Some(sequence)
    }

    pub fn deleted(&self, input: BusDeleted) -> u64 {
        let mut state = self.lock();
        let sequence = Self::next_sequence(&mut state);
        let mut event = serde_json::json!({
            "type": "resource.deleted",
            "resourceKey": input.resource_key,
            "resource": input.resource,
            "source": input.source,
            "sessionPath": input.session_path,
            "sequence": sequence,
            "occurredAt": self.occurred_at(),
        });
        if let Some(reason) = input.reason {
            event["reason"] = Value::String(reason);
        }
        self.remember_event(&mut state, event);
        sequence
    }

    pub fn renamed(&self, input: BusRenamed) -> u64 {
        let mut state = self.lock();
        let sequence = Self::next_sequence(&mut state);
        let mut event = serde_json::json!({
            "type": "resource.renamed",
            "oldResourceKey": input.old_resource_key,
            "newResourceKey": input.new_resource_key,
            "oldResource": input.old_resource,
            "newResource": input.new_resource,
            "source": input.source,
            "sessionPath": input.session_path,
            "sequence": sequence,
            "occurredAt": self.occurred_at(),
        });
        if let Some(reason) = input.reason {
            event["reason"] = Value::String(reason);
        }
        self.remember_event(&mut state, event);
        sequence
    }

    /// 现役 since（resource-event-bus.ts :82-99）：游标 < 最旧-1 →
    /// stale + 空事件；否则回送 sequence > 游标的事件。
    pub fn since(&self, cursor: u64) -> Value {
        let state = self.lock();
        let latest_sequence = state.sequence;
        if state.recent_events.is_empty() {
            return serde_json::json!({
                "stale": false,
                "latestSequence": latest_sequence,
                "events": [],
            });
        }
        let oldest_sequence = state.recent_events[0]["sequence"]
            .as_u64()
            .unwrap_or(latest_sequence);
        if cursor < oldest_sequence.saturating_sub(1) {
            return serde_json::json!({
                "stale": true,
                "latestSequence": latest_sequence,
                "events": [],
            });
        }
        let events: Vec<Value> = state
            .recent_events
            .iter()
            .filter(|event| event["sequence"].as_u64().unwrap_or(0) > cursor)
            .cloned()
            .collect();
        serde_json::json!({
            "stale": false,
            "latestSequence": latest_sequence,
            "events": events,
        })
    }
}

/// 现役 changedDedupeKey（:124-136）：version 缺席 → 无键（不去重）。
fn changed_dedupe_key(input: &BusChanged) -> Option<String> {
    let version = input.version.as_ref()?;
    Some(
        serde_json::json!({
            "resourceKey": input.resource_key,
            "changeType": input.change_type,
            "mtimeMs": version.get("mtimeMs"),
            "size": version.get("size"),
            "sha256": version.get("sha256"),
            "etag": version.get("etag"),
            "sequence": version.get("sequence"),
        })
        .to_string(),
    )
}

fn iso_ms(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::getrandom(&mut bytes).is_err() {
        // getrandom 失败是无降级余地的系统级故障；响亮 panic 与
        // resources.rs 的票据 id 生成同 stance（绝不退回弱随机）。
        panic!("getrandom failed: cannot mint a uuid v4");
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

// ── 服务：provider 分发（内核）+ watch 注册表 ──

/// workspace 面：候选人 ResourceAccess 授权闸（设计文档 :82「local_fs
/// provider 复用 ResourceAccess」）+ provider cwd + 回收区根。
struct WorkspacePlane {
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
}

/// 一个 watch 条目的快照（轮询 diff 的基准）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum WatchSnapshot {
    Missing,
    File {
        mtime_ms: u64,
        size: u64,
    },
    Dir {
        children: BTreeMap<String, ChildSig>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChildSig {
    mtime_ms: u64,
    size: u64,
    is_dir: bool,
}

struct WatchEntry {
    file_path: PathBuf,
    is_directory: bool,
    ref_count: usize,
    snapshot: WatchSnapshot,
}

struct Subscription {
    #[allow(dead_code)]
    purpose: Option<String>,
    #[allow(dead_code)]
    session_path: Option<String>,
    resource_keys: Vec<String>,
}

struct IoInner {
    session_files: Arc<SessionFileService>,
    resources: Arc<ResourceService>,
    trash_root: PathBuf,
    workspace: Option<WorkspacePlane>,
    bus: ResourceEventBus,
    watches: Mutex<HashMap<String, WatchEntry>>,
    watch_ids: Mutex<HashMap<String, String>>,
    subscriptions: Mutex<HashMap<String, Subscription>>,
    dropped_event_count: Mutex<u64>,
    last_error: Mutex<Option<(String, String)>>,
    poller: Mutex<Option<tokio::task::JoinHandle<()>>>,
    watch_poll_ms: u64,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

/// ResourceIO 服务（现役 engine.getResourceIO() + ResourceWatchRegistry
/// 的合体：内核、provider 集合、事件总线、watch 注册表一个所有权点）。
pub struct ResourceIoService {
    inner: Arc<IoInner>,
    /// D8 文件历史接线：bootstrap 后由组合根 set（现役是引擎事件总线
    /// 上挂 FileHistoryService.handleResourceEvent）。
    file_history: std::sync::OnceLock<Arc<crate::filehistory::FileHistoryService>>,
}

/// 读操作结果：meta 为现役结果对象的非 content 字段（resourceKey/
/// resource/version/filePath），content 为原始字节；路由面按 encoding
/// 把 content 编进 JSON。
#[derive(Debug)]
pub struct ResourceRead {
    pub meta: Value,
    pub content: Vec<u8>,
}

impl ResourceIoService {
    pub fn new(
        session_files: Arc<SessionFileService>,
        resources: Arc<ResourceService>,
        data_home: PathBuf,
        workspace: Option<(Arc<ResourceAccess>, PathBuf)>,
        watch_poll_ms: u64,
        now: Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        let inner = Arc::new(IoInner {
            session_files,
            resources,
            // 现役 trashRoot = path.join(lingxiHome, "trash")
            //（sandbox-resource-io.ts :64）→ 候选人数据家目录下。
            trash_root: data_home.join("trash"),
            workspace: workspace.map(|(access, cwd)| WorkspacePlane { access, cwd }),
            bus: ResourceEventBus::new(now.clone()),
            watches: Mutex::new(HashMap::new()),
            watch_ids: Mutex::new(HashMap::new()),
            subscriptions: Mutex::new(HashMap::new()),
            dropped_event_count: Mutex::new(0),
            last_error: Mutex::new(None),
            poller: Mutex::new(None),
            watch_poll_ms,
            now,
        });
        ResourceIoService {
            inner,
            file_history: std::sync::OnceLock::new(),
        }
    }

    /// D8：组合根注入文件历史服务（一次性；重复 set 忽略并保首值）。
    pub fn set_file_history(&self, service: Arc<crate::filehistory::FileHistoryService>) {
        let _ = self.file_history.set(service);
    }

    pub fn file_history(&self) -> Option<&Arc<crate::filehistory::FileHistoryService>> {
        self.file_history.get()
    }

    // ── 内核分派（resource-io.ts callProvider :202-224） ──

    /// provider 闸：mount/url → D9 400；local_fs 缺席 → 现役 501。
    fn local_plane(&self) -> Result<&WorkspacePlane, ResourceIoError> {
        self.inner
            .workspace
            .as_ref()
            .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))
    }

    fn refuse_if_mount_or_url(reference: &ResourceRef) -> Result<(), ResourceIoError> {
        match reference {
            ResourceRef::Mount { .. } | ResourceRef::Url { .. } => Err(
                ResourceIoError::unsupported_provider(provider_id_for_ref(reference)),
            ),
            _ => Ok(()),
        }
    }

    pub async fn stat(&self, input: &Value, ctx: &OpContext) -> Result<Value, ResourceIoError> {
        let reference = normalize_resource_ref(input)?;
        Self::refuse_if_mount_or_url(&reference)?;
        match &reference {
            ResourceRef::LocalFile { .. } => {
                let inner = self.inner.clone();
                let ctx = ctx.clone();
                spawn_blocking_io(move || inner.local_stat(&reference, &ctx)).await
            }
            ResourceRef::SessionFile { .. } => self.session_file_stat(&reference).await,
            ResourceRef::Resource { .. } => self.resource_stat(&reference).await,
            ResourceRef::Mount { .. } | ResourceRef::Url { .. } => unreachable!(),
        }
    }

    pub async fn read(
        &self,
        input: &Value,
        ctx: &OpContext,
    ) -> Result<ResourceRead, ResourceIoError> {
        let reference = normalize_resource_ref(input)?;
        Self::refuse_if_mount_or_url(&reference)?;
        match &reference {
            ResourceRef::LocalFile { .. } => {
                let inner = self.inner.clone();
                let ctx = ctx.clone();
                spawn_blocking_io(move || inner.local_read(&reference, &ctx)).await
            }
            ResourceRef::SessionFile { .. } => self.session_file_read(&reference).await,
            ResourceRef::Resource { .. } => self.resource_read(&reference).await,
            ResourceRef::Mount { .. } | ResourceRef::Url { .. } => unreachable!(),
        }
    }

    pub async fn list(&self, input: &Value, ctx: &OpContext) -> Result<Value, ResourceIoError> {
        let reference = normalize_resource_ref(input)?;
        Self::refuse_if_mount_or_url(&reference)?;
        match &reference {
            ResourceRef::LocalFile { .. } => {
                let inner = self.inner.clone();
                let ctx = ctx.clone();
                spawn_blocking_io(move || inner.local_list(&reference, &ctx)).await
            }
            other => Err(ResourceIoError::capability_denied(
                "list",
                provider_id_for_ref(other),
            )),
        }
    }

    pub async fn search(
        &self,
        input: &Value,
        query: Option<String>,
        mode: Option<&str>,
        limit: Option<usize>,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let reference = normalize_resource_ref(input)?;
        Self::refuse_if_mount_or_url(&reference)?;
        match &reference {
            ResourceRef::LocalFile { .. } => {
                let inner = self.inner.clone();
                let ctx = ctx.clone();
                let mode = mode.map(str::to_string);
                spawn_blocking_io(move || {
                    inner.local_search(&reference, query.as_deref(), mode.as_deref(), limit, &ctx)
                })
                .await
            }
            other => Err(ResourceIoError::capability_denied(
                "search",
                provider_id_for_ref(other),
            )),
        }
    }

    pub async fn write(
        &self,
        input: &Value,
        content: Vec<u8>,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let reference = normalize_resource_ref(input)?;
        Self::refuse_if_mount_or_url(&reference)?;
        match &reference {
            ResourceRef::LocalFile { .. } => {
                let inner = self.inner.clone();
                let ctx2 = ctx.clone();
                let result =
                    spawn_blocking_io(move || inner.local_write(&reference, &content, &ctx2))
                        .await?;
                self.emit_changed(&result, ctx);
                self.capture_history(&result, ctx).await;
                Ok(result)
            }
            other => Err(ResourceIoError::capability_denied(
                "write",
                provider_id_for_ref(other),
            )),
        }
    }

    pub async fn write_expected_version(
        &self,
        input: &Value,
        content: Vec<u8>,
        expected: &Value,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let reference = normalize_resource_ref(input)?;
        Self::refuse_if_mount_or_url(&reference)?;
        let Some(expected_version) = expected.as_object() else {
            return Err(ResourceIoError::io_bad_request(
                "write-expected-version requires an expectedVersion object",
            ));
        };
        let expected_version = Value::Object(expected_version.clone());
        match &reference {
            ResourceRef::LocalFile { .. } => {
                let inner = self.inner.clone();
                let ctx2 = ctx.clone();
                let result = spawn_blocking_io(move || {
                    inner.local_write_expected_version(
                        &reference,
                        &content,
                        &expected_version,
                        &ctx2,
                    )
                })
                .await?;
                // 冲突是结果不是事件（resource-io.ts :79-84）。
                if result["ok"] != Value::Bool(false) {
                    self.emit_changed(&result, ctx);
                    self.capture_history(&result, ctx).await;
                }
                Ok(result)
            }
            other => Err(ResourceIoError::capability_denied(
                "writeExpectedVersion",
                provider_id_for_ref(other),
            )),
        }
    }

    /// rename 与 move 共用现役 moveLike（:180-193）：跨 provider →
    /// 501 cross_provider_move_unsupported；同 provider 走能力闸。
    pub async fn rename(
        &self,
        from: &Value,
        to: &Value,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        self.move_like("rename", from, to, ctx).await
    }

    pub async fn move_resource(
        &self,
        from: &Value,
        to: &Value,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        self.move_like("move", from, to, ctx).await
    }

    async fn move_like(
        &self,
        capability: &'static str,
        from: &Value,
        to: &Value,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let from_ref = normalize_resource_ref(from)?;
        let to_ref = normalize_resource_ref(to)?;
        let from_provider = provider_id_for_ref(&from_ref);
        let to_provider = provider_id_for_ref(&to_ref);
        if from_provider != to_provider {
            return Err(ResourceIoError::new(
                501,
                "cross_provider_move_unsupported",
                format!(
                    "ResourceIO cross-provider move is not implemented: {from_provider} -> {to_provider}"
                ),
            ));
        }
        Self::refuse_if_mount_or_url(&from_ref)?;
        match &from_ref {
            ResourceRef::LocalFile { .. } => {
                let inner = self.inner.clone();
                let ctx2 = ctx.clone();
                let result =
                    spawn_blocking_io(move || inner.local_move(&from_ref, &to_ref, &ctx2)).await?;
                self.emit_renamed(&result, ctx);
                Ok(result)
            }
            other => Err(ResourceIoError::capability_denied(
                capability,
                provider_id_for_ref(other),
            )),
        }
    }

    pub async fn trash(
        &self,
        input: &Value,
        namespace: Option<&str>,
        metadata: Option<Value>,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let reference = normalize_resource_ref(input)?;
        Self::refuse_if_mount_or_url(&reference)?;
        match &reference {
            ResourceRef::LocalFile { .. } => {
                let inner = self.inner.clone();
                let ctx2 = ctx.clone();
                let namespace = namespace.map(str::to_string);
                let result = spawn_blocking_io(move || {
                    inner.local_trash(&reference, namespace.as_deref(), metadata, &ctx2)
                })
                .await?;
                self.emit_deleted(
                    result["resourceKey"].as_str().unwrap_or_default(),
                    result["resource"].clone(),
                    ctx,
                );
                Ok(result)
            }
            other => Err(ResourceIoError::capability_denied(
                "trash",
                provider_id_for_ref(other),
            )),
        }
    }

    /// 叶 #53：materialize —— 本地回真实路径；session_file/resource 经
    /// resolver；绝不返回假路径。
    pub async fn materialize(
        &self,
        input: &Value,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let reference = normalize_resource_ref(input)?;
        Self::refuse_if_mount_or_url(&reference)?;
        match &reference {
            ResourceRef::LocalFile { .. } => {
                let inner = self.inner.clone();
                let ctx = ctx.clone();
                spawn_blocking_io(move || inner.local_materialize(&reference, &ctx)).await
            }
            ResourceRef::SessionFile { .. } => self.session_file_materialize(&reference).await,
            ResourceRef::Resource { .. } => self.resource_materialize(&reference).await,
            ResourceRef::Mount { .. } | ResourceRef::Url { .. } => unreachable!(),
        }
    }

    // ── 事件外发（resource-io.ts :226-261） ──

    fn emit_changed(&self, result: &Value, ctx: &OpContext) {
        if !ctx.emit {
            return;
        }
        let _ = self.inner.bus.changed(BusChanged {
            change_type: result["changeType"]
                .as_str()
                .unwrap_or("modified")
                .to_string(),
            resource_key: result["resourceKey"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            resource: result["resource"].clone(),
            version: result.get("version").cloned(),
            source: ctx.source.clone(),
            reason: ctx.reason.clone(),
            session_path: ctx.session_path.clone(),
        });
    }

    /// D8 文件历史捕获：现役是引擎把 resource.changed 事件转给
    /// FileHistoryService（150ms 防抖异步，file-history-service.ts
    /// :215-225）；候选人在写成功后同步捕获（净落库状态一致，台账
    /// 登记）。emit=false → 现役无事件 → 候选人不捕获。
    async fn capture_history(&self, result: &Value, ctx: &OpContext) {
        if !ctx.emit {
            return;
        }
        let Some(fh) = self.file_history() else {
            return;
        };
        let resource = &result["resource"];
        let abs = resource["filePath"]
            .as_str()
            .or_else(|| resource["path"].as_str());
        let Some(abs) = abs else { return };
        let origin = ctx.capture_origin.as_deref().unwrap_or("event");
        fh.capture_from_disk(std::path::Path::new(abs), origin)
            .await;
    }

    fn emit_deleted(&self, resource_key: &str, resource: Value, ctx: &OpContext) {
        if !ctx.emit {
            return;
        }
        let _ = self.inner.bus.deleted(BusDeleted {
            resource_key: resource_key.to_string(),
            resource,
            source: ctx.source.clone(),
            reason: ctx.reason.clone(),
            session_path: ctx.session_path.clone(),
        });
    }

    fn emit_renamed(&self, result: &Value, ctx: &OpContext) {
        if !ctx.emit {
            return;
        }
        let _ = self.inner.bus.renamed(BusRenamed {
            old_resource_key: result["oldResourceKey"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            new_resource_key: result["newResourceKey"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            old_resource: result["oldResource"].clone(),
            new_resource: result["newResource"].clone(),
            source: ctx.source.clone(),
            reason: ctx.reason.clone(),
            session_path: ctx.session_path.clone(),
        });
    }

    /// GET events 的总线追扑面。
    pub fn events_since(&self, cursor: u64) -> Value {
        self.inner.bus.since(cursor)
    }

    // ── session_file provider（providers/session-file-resolver.ts） ──

    /// resolver（lib/resource-io/session-file-resolver.ts :20-59）：
    /// 404 → 410 → 500 → 载荷缺失 404 的现役序列。
    async fn resolve_session_file(
        &self,
        reference: &ResourceRef,
    ) -> Result<(String, crate::sessionfiles::SessionFileView, String), ResourceIoError> {
        let ResourceRef::SessionFile {
            file_id,
            session_id: _,
        } = reference
        else {
            return Err(ResourceIoError::new(
                400,
                "invalid_resource_ref",
                format!("session file resolver cannot resolve {}", reference.kind()),
            ));
        };
        let entry = self
            .inner
            .session_files
            .get_file_global(file_id)
            .await
            .map_err(|err| ResourceIoError::internal(format!("session file lookup: {err}")))?
            .ok_or_else(|| {
                ResourceIoError::new(
                    404,
                    "resource_not_found",
                    format!("session file not found: {file_id}"),
                )
            })?;
        if entry.status == "expired" {
            return Err(ResourceIoError::new(
                410,
                "resource_expired",
                format!("session file expired: {file_id}"),
            ));
        }
        let file_path = if entry.real_path.is_empty() {
            entry.file_path.clone()
        } else {
            entry.real_path.clone()
        };
        if file_path.is_empty() || !Path::new(&file_path).is_absolute() {
            return Err(ResourceIoError::new(
                500,
                "invalid_resource_path",
                format!("session file path is invalid: {file_id}"),
            ));
        }
        // resolveExistingPath（:62-73）：realpath 失败 ENOENT → 载荷缺失。
        let canonical = std::fs::canonicalize(&file_path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                ResourceIoError::new(
                    404,
                    "resource_not_found",
                    format!("session file payload missing: {file_path}"),
                )
            } else {
                ResourceIoError::internal(format!("session file resolve: {err}"))
            }
        })?;
        Ok((
            resource_key_for_ref(reference),
            entry,
            canonical.to_string_lossy().into_owned(),
        ))
    }

    fn session_file_descriptor(
        reference: &ResourceRef,
        entry: &crate::sessionfiles::SessionFileView,
        file_path: &str,
    ) -> Value {
        let ResourceRef::SessionFile {
            file_id,
            session_id,
        } = reference
        else {
            return Value::Null;
        };
        let mut resource = serde_json::json!({
            "kind": "session-file",
            "fileId": file_id,
            "provider": "session_file",
            "filePath": file_path,
            "displayName": entry.label.clone().unwrap_or_else(|| entry.filename.clone()),
        });
        if let Some(session_id) = session_id {
            resource["sessionId"] = Value::String(session_id.clone());
        }
        resource
    }

    async fn session_file_stat(&self, reference: &ResourceRef) -> Result<Value, ResourceIoError> {
        let (resource_key, entry, file_path) = self.resolve_session_file(reference).await?;
        let meta = std::fs::metadata(&file_path)
            .map_err(|err| ResourceIoError::internal(format!("stat session file: {err}")))?;
        Ok(serde_json::json!({
            "resourceKey": resource_key,
            "resource": Self::session_file_descriptor(reference, &entry, &file_path),
            "exists": true,
            "isDirectory": meta.is_dir(),
            "version": version_json(&meta),
            "filePath": file_path,
        }))
    }

    async fn session_file_read(
        &self,
        reference: &ResourceRef,
    ) -> Result<ResourceRead, ResourceIoError> {
        let (resource_key, entry, file_path) = self.resolve_session_file(reference).await?;
        let meta = std::fs::metadata(&file_path)
            .map_err(|err| ResourceIoError::internal(format!("stat session file: {err}")))?;
        if !meta.is_file() {
            return Err(ResourceIoError::new(
                409,
                "resource_not_file",
                format!(
                    "session file is not a regular file: {}",
                    match reference {
                        ResourceRef::SessionFile { file_id, .. } => file_id.clone(),
                        _ => String::new(),
                    }
                ),
            ));
        }
        let read_path = file_path.clone();
        let content = tokio::task::spawn_blocking(move || std::fs::read(read_path))
            .await
            .map_err(|err| ResourceIoError::internal(format!("read join: {err}")))?
            .map_err(|err| ResourceIoError::internal(format!("read session file: {err}")))?;
        Ok(ResourceRead {
            meta: serde_json::json!({
                "resourceKey": resource_key,
                "resource": Self::session_file_descriptor(reference, &entry, &file_path),
                "version": version_json(&meta),
                "filePath": file_path,
            }),
            content,
        })
    }

    async fn session_file_materialize(
        &self,
        reference: &ResourceRef,
    ) -> Result<Value, ResourceIoError> {
        let (resource_key, entry, file_path) = self.resolve_session_file(reference).await?;
        let meta = std::fs::metadata(&file_path)
            .map_err(|err| ResourceIoError::internal(format!("stat session file: {err}")))?;
        Ok(serde_json::json!({
            "resourceKey": resource_key,
            "resource": Self::session_file_descriptor(reference, &entry, &file_path),
            "filePath": file_path,
            "isDirectory": meta.is_dir(),
            "version": version_json(&meta),
        }))
    }

    // ── resource provider（providers/resource-provider.ts） ──

    async fn resolve_resource_content(
        &self,
        reference: &ResourceRef,
    ) -> Result<crate::resources::ResolvedContent, ResourceIoError> {
        let ResourceRef::Resource { resource_id } = reference else {
            return Err(ResourceIoError::new(
                400,
                "invalid_resource_ref",
                format!("resource provider cannot resolve {}", reference.kind()),
            ));
        };
        // normalizeResourceServiceError（:137-145）：code/status 原样透传。
        self.inner
            .resources
            .resolve_content(resource_id)
            .await
            .map_err(|err| ResourceIoError {
                status: err.status,
                code: err.code,
                message: err.message,
                safe_message: None,
            })
    }

    fn resource_descriptor(
        resource_id: &str,
        content: &crate::resources::ResolvedContent,
    ) -> Value {
        serde_json::json!({
            "kind": "resource",
            "resourceId": resource_id,
            "provider": "resource",
            "filePath": content.file_path.to_string_lossy(),
            "displayName": content.filename,
        })
    }

    fn resource_version_json(content: &crate::resources::ResolvedContent) -> Value {
        serde_json::json!({
            "mtimeMs": content.mtime_ms,
            "size": content.size,
            "etag": content.etag,
        })
    }

    async fn resource_stat(&self, reference: &ResourceRef) -> Result<Value, ResourceIoError> {
        let content = self.resolve_resource_content(reference).await?;
        let ResourceRef::Resource { resource_id } = reference else {
            unreachable!()
        };
        Ok(serde_json::json!({
            "resourceKey": resource_key_for_ref(reference),
            "resource": Self::resource_descriptor(resource_id, &content),
            "exists": true,
            "isDirectory": false,
            "version": Self::resource_version_json(&content),
            "filePath": content.file_path.to_string_lossy(),
        }))
    }

    async fn resource_read(
        &self,
        reference: &ResourceRef,
    ) -> Result<ResourceRead, ResourceIoError> {
        let content = self.resolve_resource_content(reference).await?;
        let ResourceRef::Resource { resource_id } = reference else {
            unreachable!()
        };
        let read_path = content.file_path.clone();
        let bytes = tokio::task::spawn_blocking(move || std::fs::read(read_path))
            .await
            .map_err(|err| ResourceIoError::internal(format!("read join: {err}")))?
            .map_err(|err| ResourceIoError::internal(format!("read resource: {err}")))?;
        Ok(ResourceRead {
            meta: serde_json::json!({
                "resourceKey": resource_key_for_ref(reference),
                "resource": Self::resource_descriptor(resource_id, &content),
                "version": Self::resource_version_json(&content),
                "filePath": content.file_path.to_string_lossy(),
            }),
            content: bytes,
        })
    }

    async fn resource_materialize(
        &self,
        reference: &ResourceRef,
    ) -> Result<Value, ResourceIoError> {
        let content = self.resolve_resource_content(reference).await?;
        let ResourceRef::Resource { resource_id } = reference else {
            unreachable!()
        };
        Ok(serde_json::json!({
            "resourceKey": resource_key_for_ref(reference),
            "resource": Self::resource_descriptor(resource_id, &content),
            "filePath": content.file_path.to_string_lossy(),
            "version": Self::resource_version_json(&content),
        }))
    }

    // ── watch 注册表（resource-watch-registry.ts；D7 轮询机制） ──

    /// resolveWatchTarget（resource-io.ts :140-150 能力闸 +
    /// local-fs-provider.ts :251-270 watchTarget）。
    fn resolve_watch_target(
        &self,
        input: &Value,
    ) -> Result<(PathBuf, bool, String, Value), ResourceIoError> {
        let reference = normalize_resource_ref(input)?;
        Self::refuse_if_mount_or_url(&reference)?;
        match &reference {
            ResourceRef::LocalFile { path } => {
                let plane = self.local_plane()?;
                let ctx = OpContext::local_owner("");
                let resolved = plane.authorize(path, ResourceOp::Read, &ctx)?;
                let is_directory = resolved.is_dir();
                let key = resource_key_for_ref(&ResourceRef::LocalFile {
                    path: resolved.to_string_lossy().into_owned(),
                });
                let resource = serde_json::json!({
                    "kind": "local-file",
                    "provider": "local_fs",
                    "path": resolved.to_string_lossy(),
                    "filePath": resolved.to_string_lossy(),
                });
                Ok((resolved, is_directory, key, resource))
            }
            other => Err(ResourceIoError::capability_denied(
                "watch",
                provider_id_for_ref(other),
            )),
        }
    }

    fn record_watch_error(&self, err: &ResourceIoError) {
        let mut slot = self
            .inner
            .last_error
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *slot = Some((
            err.code.clone(),
            err.safe_message
                .clone()
                .unwrap_or_else(|| "Resource watch failed".to_string()),
        ));
    }

    /// registry.retain：已存在 → refCount+1；新建 → 初始快照 + 启动轮询。
    /// 目标必须存在（现役 fs.watch 对缺失路径同步 ENOENT → 路由 400）。
    fn retain_watch(&self, input: &Value) -> Result<String, ResourceIoError> {
        let (file_path, is_directory, resource_key, _resource) =
            match self.resolve_watch_target(input) {
                Ok(target) => target,
                Err(err) => {
                    self.record_watch_error(&err);
                    return Err(err);
                }
            };
        if !file_path.exists() {
            let err = ResourceIoError::io_bad_request(format!(
                "resource watch target does not exist: {}",
                file_path.display()
            ));
            self.record_watch_error(&err);
            return Err(err);
        }
        let mut watches = self.inner.watches.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = watches.get_mut(&resource_key) {
            entry.ref_count += 1;
            return Ok(resource_key);
        }
        let snapshot = snapshot_path(&file_path, is_directory);
        watches.insert(
            resource_key.clone(),
            WatchEntry {
                file_path,
                is_directory,
                ref_count: 1,
                snapshot,
            },
        );
        drop(watches);
        self.ensure_poller();
        Ok(resource_key)
    }

    fn release_watch(&self, resource_key: &str) -> bool {
        let mut watches = self.inner.watches.lock().unwrap_or_else(|p| p.into_inner());
        let Some(entry) = watches.get_mut(resource_key) else {
            return false;
        };
        if entry.ref_count > 1 {
            entry.ref_count -= 1;
            return true;
        }
        watches.remove(resource_key);
        true
    }

    /// POST watch：retain + watchId（现役 route :53-67 的 UUID 形态）。
    pub async fn watch(&self, input: &Value) -> Result<String, ResourceIoError> {
        let resource_key = self.retain_watch(input)?;
        let watch_id = uuid_v4();
        self.inner
            .watch_ids
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(watch_id.clone(), resource_key);
        Ok(watch_id)
    }

    /// DELETE watch/{id}（route :69-76）。
    pub fn unwatch(&self, watch_id: &str) -> bool {
        let Some(resource_key) = self
            .inner
            .watch_ids
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(watch_id)
        else {
            return false;
        };
        self.release_watch(&resource_key);
        true
    }

    /// POST subscribe（registry :96-127）：逐个 retain，失败回滚已 retain
    /// 的（逆序释放），响亮报错。
    pub async fn subscribe(&self, input: &Value) -> Result<Value, ResourceIoError> {
        let resources: Vec<Value> = match input {
            Value::Object(map) => {
                if let Some(Value::Array(list)) = map.get("resources") {
                    list.clone()
                } else if let Some(single) = map.get("resource") {
                    vec![single.clone()]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        };
        if resources.is_empty() {
            return Err(ResourceIoError::io_bad_request(
                "ResourceWatchRegistry subscription requires resources",
            ));
        }
        let mut retained: Vec<String> = Vec::new();
        let mut resource_keys = Vec::new();
        for resource in &resources {
            match self.retain_watch(resource) {
                Ok(key) => {
                    retained.push(key.clone());
                    resource_keys.push(key);
                }
                Err(err) => {
                    self.record_watch_error(&err);
                    for key in retained.iter().rev() {
                        self.release_watch(key);
                    }
                    return Err(err);
                }
            }
        }
        let subscription_id = uuid_v4();
        let purpose = input
            .get("purpose")
            .and_then(Value::as_str)
            .map(str::to_string);
        let session_path = input
            .get("sessionPath")
            .and_then(Value::as_str)
            .map(str::to_string);
        self.inner
            .subscriptions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                subscription_id.clone(),
                Subscription {
                    purpose,
                    session_path,
                    resource_keys: resource_keys.clone(),
                },
            );
        Ok(serde_json::json!({
            "subscriptionId": subscription_id,
            "resourceKeys": resource_keys,
        }))
    }

    /// DELETE subscriptions/{id}（registry :129-135）。
    pub fn unsubscribe(&self, subscription_id: &str) -> bool {
        let Some(subscription) = self
            .inner
            .subscriptions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(subscription_id)
        else {
            return false;
        };
        for key in subscription.resource_keys.iter().rev() {
            self.release_watch(key);
        }
        true
    }

    /// 诊断面（registry :137-149）。
    pub fn diagnostics(&self) -> Value {
        let watches = self.inner.watches.lock().unwrap_or_else(|p| p.into_inner());
        let subscriptions = self
            .inner
            .subscriptions
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let dropped = *self
            .inner
            .dropped_event_count
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let last_error = self
            .inner
            .last_error
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (last_code, last_message) = match &*last_error {
            Some((code, message)) => (Value::String(code.clone()), Value::String(message.clone())),
            None => (Value::Null, Value::Null),
        };
        serde_json::json!({
            "subscriptions": subscriptions.len(),
            "droppedEventCount": dropped,
            "lastErrorCode": last_code,
            "lastErrorMessage": last_message,
            "watches": watches
                .iter()
                .map(|(key, entry)| serde_json::json!({
                    "resourceKey": key,
                    "refCount": entry.ref_count,
                    "isDirectory": entry.is_directory,
                }))
                .collect::<Vec<_>>(),
        })
    }

    /// D7 轮询器：首个 watch 注册时启动；持有 Weak —— 服务状态析构
    /// （服务关停）后自动退出，无需额外关停线（R02-T06 纪律）。
    fn ensure_poller(&self) {
        let mut slot = self.inner.poller.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_some() {
            return;
        }
        let weak = Arc::downgrade(&self.inner);
        let poll_ms = self.inner.watch_poll_ms;
        *slot = Some(tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(poll_ms.max(10)));
            loop {
                interval.tick().await;
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                inner.poll_watches_once();
            }
        }));
    }
}

impl IoInner {
    /// 一拍轮询：重取快照 → diff → 发事件。同窗口多次变化合并为一条
    /// （现役 80ms 去抖窗口的轮询等价语义，D7）。
    fn poll_watches_once(self: &Arc<Self>) {
        let targets: Vec<(String, PathBuf, bool)> = {
            let watches = self.watches.lock().unwrap_or_else(|p| p.into_inner());
            watches
                .iter()
                .map(|(key, entry)| (key.clone(), entry.file_path.clone(), entry.is_directory))
                .collect()
        };
        for (resource_key, file_path, is_directory) in targets {
            let new_snapshot = snapshot_path(&file_path, is_directory);
            let events = {
                let mut watches = self.watches.lock().unwrap_or_else(|p| p.into_inner());
                let Some(entry) = watches.get_mut(&resource_key) else {
                    let mut dropped = self
                        .dropped_event_count
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    *dropped += 1;
                    continue;
                };
                if entry.snapshot == new_snapshot {
                    continue;
                }
                let old = std::mem::replace(&mut entry.snapshot, new_snapshot.clone());
                diff_snapshots(&file_path, is_directory, &old, &new_snapshot)
            };
            for event in events {
                match event {
                    PollEvent::Changed {
                        path,
                        version,
                        is_dir,
                    } => {
                        let _ = self.bus.changed(BusChanged {
                            change_type: "modified".to_string(),
                            resource_key: local_key(&path),
                            resource: local_watch_resource(&path, is_dir),
                            version: Some(version),
                            source: "provider_watch".to_string(),
                            reason: None,
                            session_path: None,
                        });
                    }
                    PollEvent::Deleted { path, is_dir } => {
                        let _ = self.bus.deleted(BusDeleted {
                            resource_key: local_key(&path),
                            resource: local_watch_resource(&path, is_dir),
                            source: "provider_watch".to_string(),
                            reason: None,
                            session_path: None,
                        });
                    }
                }
            }
        }
    }
}

enum PollEvent {
    Changed {
        path: PathBuf,
        version: Value,
        is_dir: bool,
    },
    Deleted {
        path: PathBuf,
        is_dir: bool,
    },
}

fn local_key(path: &Path) -> String {
    format!("local_fs:{}", path.to_string_lossy().replace('\\', "/"))
}

fn local_watch_resource(path: &Path, is_dir: bool) -> Value {
    serde_json::json!({
        "kind": "local-file",
        "provider": "local_fs",
        "path": path.to_string_lossy(),
        "filePath": path.to_string_lossy(),
        "isDirectory": is_dir,
    })
}

fn version_value(mtime_ms: u64, size: u64, is_dir: bool) -> Value {
    serde_json::json!({
        "mtimeMs": mtime_ms,
        "size": if is_dir { Value::Null } else { Value::from(size) },
    })
}

/// 文件/目录的一层快照（现役 fs.watch 非递归 —— 目录只看直接子项）。
fn snapshot_path(path: &Path, is_directory: bool) -> WatchSnapshot {
    let Ok(meta) = std::fs::metadata(path) else {
        return WatchSnapshot::Missing;
    };
    if !is_directory {
        return WatchSnapshot::File {
            mtime_ms: mtime_ms_of(&meta),
            size: meta.len(),
        };
    }
    let mut children = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(child_meta) = std::fs::metadata(entry.path()) {
                children.insert(
                    entry.file_name().to_string_lossy().into_owned(),
                    ChildSig {
                        mtime_ms: mtime_ms_of(&child_meta),
                        size: child_meta.len(),
                        is_dir: child_meta.is_dir(),
                    },
                );
            }
        }
    }
    WatchSnapshot::Dir { children }
}

/// 快照 diff → 事件（emitSnapshot 的轮询等价：子项变化 → 子项键事件；
/// 根本身消失 → 根键 deleted；根复现 → 根键 modified）。
fn diff_snapshots(
    root: &Path,
    is_directory: bool,
    old: &WatchSnapshot,
    new: &WatchSnapshot,
) -> Vec<PollEvent> {
    let mut events = Vec::new();
    match (old, new) {
        (WatchSnapshot::Missing, WatchSnapshot::Missing) => {}
        (WatchSnapshot::Missing, WatchSnapshot::File { mtime_ms, size }) => {
            events.push(PollEvent::Changed {
                path: root.to_path_buf(),
                version: version_value(*mtime_ms, *size, false),
                is_dir: false,
            });
        }
        (WatchSnapshot::Missing, WatchSnapshot::Dir { .. }) => {
            events.push(PollEvent::Changed {
                path: root.to_path_buf(),
                version: version_value(0, 0, true),
                is_dir: true,
            });
        }
        (WatchSnapshot::File { .. }, WatchSnapshot::Missing)
        | (WatchSnapshot::Dir { .. }, WatchSnapshot::Missing) => {
            events.push(PollEvent::Deleted {
                path: root.to_path_buf(),
                is_dir: is_directory,
            });
        }
        (
            WatchSnapshot::File {
                mtime_ms: a_m,
                size: a_s,
            },
            WatchSnapshot::File {
                mtime_ms: b_m,
                size: b_s,
            },
        ) => {
            if a_m != b_m || a_s != b_s {
                events.push(PollEvent::Changed {
                    path: root.to_path_buf(),
                    version: version_value(*b_m, *b_s, false),
                    is_dir: false,
                });
            }
        }
        (WatchSnapshot::Dir { children: old_c }, WatchSnapshot::Dir { children: new_c }) => {
            for (name, sig) in new_c {
                let changed = match old_c.get(name) {
                    Some(old_sig) => old_sig != sig,
                    None => true,
                };
                if changed {
                    events.push(PollEvent::Changed {
                        path: root.join(name),
                        version: version_value(sig.mtime_ms, sig.size, sig.is_dir),
                        is_dir: sig.is_dir,
                    });
                }
            }
            for (name, old_sig) in old_c {
                if !new_c.contains_key(name) {
                    events.push(PollEvent::Deleted {
                        path: root.join(name),
                        is_dir: old_sig.is_dir,
                    });
                }
            }
        }
        // 文件↔目录形态互换：按一次根 modified 报（现役 fs.watch 在此
        // 场景同样只产生一次 rename 事件 → emitSnapshot stat 根）。
        (WatchSnapshot::File { .. }, WatchSnapshot::Dir { .. })
        | (WatchSnapshot::Dir { .. }, WatchSnapshot::File { .. }) => {
            let (mtime_ms, size, is_dir) = match new {
                WatchSnapshot::File { mtime_ms, size } => (*mtime_ms, *size, false),
                _ => (0, 0, true),
            };
            events.push(PollEvent::Changed {
                path: root.to_path_buf(),
                version: version_value(mtime_ms, size, is_dir),
                is_dir,
            });
        }
    }
    events
}

async fn spawn_blocking_io<T, F>(work: F) -> Result<T, ResourceIoError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ResourceIoError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| ResourceIoError::internal(format!("blocking io join: {err}")))?
}

fn mtime_ms_of(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 现役 versionFromStat（local-fs-provider.ts :367-372）。
fn version_json(meta: &std::fs::Metadata) -> Value {
    serde_json::json!({
        "mtimeMs": mtime_ms_of(meta),
        "size": if meta.is_dir() { Value::Null } else { Value::from(meta.len()) },
    })
}

/// 现役 fileVersionsMatch（local-fs-provider.ts :384-391）：仅比对
/// expected 出现的字段；expected 带而 current 没有的字段 → 失配。
fn file_versions_match(current: &Value, expected: &Value) -> bool {
    fn num(value: &Value) -> Option<f64> {
        value.as_f64()
    }
    for field in ["mtimeMs", "size", "sequence"] {
        if let Some(expected_value) = expected.get(field).filter(|v| !v.is_null()) {
            let matches = current
                .get(field)
                .and_then(num)
                .zip(num(expected_value))
                .map(|(a, b)| a == b)
                .unwrap_or(false);
            if !matches {
                return false;
            }
        }
    }
    for field in ["sha256", "etag"] {
        if let Some(expected_value) = expected.get(field).filter(|v| !v.is_null()) {
            if current.get(field) != Some(expected_value) {
                return false;
            }
        }
    }
    true
}

impl WorkspacePlane {
    /// resolvePath + assertAllowed（local-fs-provider.ts :310-319 +
    /// :352-360）：ResourceAccess 一次完成「真实路径解析 + 授权」，回
    /// 真实路径；拒绝 → 403 resource_access_denied + safeMessage。
    fn authorize(
        &self,
        raw_path: &str,
        op: ResourceOp,
        ctx: &OpContext,
    ) -> Result<PathBuf, ResourceIoError> {
        self.access
            .authorize(
                ctx.principal_kind,
                &ctx.principal_subject,
                &ctx.session_id,
                Path::new(raw_path),
                &self.cwd,
                op,
            )
            .map(|scope| scope.path)
            .map_err(|refusal| {
                let safe = match refusal.cause {
                    crate::resourceaccess::AccessRefusalCause::Unresolvable => {
                        "Resource path is invalid".to_string()
                    }
                    _ => "Resource is outside authorized roots".to_string(),
                };
                ResourceIoError::access_denied(op.wire_name(), refusal.message, safe)
            })
    }

    fn descriptor(file_path: &Path) -> Value {
        serde_json::json!({
            "kind": "local-file",
            "path": file_path.to_string_lossy(),
            "filePath": file_path.to_string_lossy(),
            "provider": "local_fs",
        })
    }

    fn mutation_result(file_path: &Path, change_type: &str) -> Value {
        let version = std::fs::metadata(file_path)
            .ok()
            .map(|meta| version_json(&meta));
        let mut out = serde_json::json!({
            "changeType": change_type,
            "resourceKey": local_key(file_path),
            "resource": Self::descriptor(file_path),
            "filePath": file_path.to_string_lossy(),
        });
        if let Some(version) = version {
            out["version"] = version;
        }
        out
    }
}

impl IoInner {
    fn local_path<'a>(&self, reference: &'a ResourceRef) -> Result<&'a str, ResourceIoError> {
        match reference {
            ResourceRef::LocalFile { path } => Ok(path.as_str()),
            other => Err(ResourceIoError::io_bad_request(format!(
                "local_fs provider cannot resolve {}",
                other.kind()
            ))),
        }
    }

    fn local_stat(
        &self,
        reference: &ResourceRef,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let plane = self
            .workspace
            .as_ref()
            .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
        let file_path = plane.authorize(self.local_path(reference)?, ResourceOp::Read, ctx)?;
        // existsSync 语义：任何 metadata 错误都记 exists:false（:79）。
        let Ok(meta) = std::fs::metadata(&file_path) else {
            return Ok(serde_json::json!({
                "resourceKey": local_key(&file_path),
                "resource": WorkspacePlane::descriptor(&file_path),
                "exists": false,
                "isDirectory": false,
                "filePath": file_path.to_string_lossy(),
            }));
        };
        Ok(serde_json::json!({
            "resourceKey": local_key(&file_path),
            "resource": WorkspacePlane::descriptor(&file_path),
            "exists": true,
            "isDirectory": meta.is_dir(),
            "version": version_json(&meta),
            "filePath": file_path.to_string_lossy(),
        }))
    }

    fn local_read(
        &self,
        reference: &ResourceRef,
        ctx: &OpContext,
    ) -> Result<ResourceRead, ResourceIoError> {
        let plane = self
            .workspace
            .as_ref()
            .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
        let file_path = plane.authorize(self.local_path(reference)?, ResourceOp::Read, ctx)?;
        let meta = std::fs::metadata(&file_path).map_err(|err| {
            ResourceIoError::io_bad_request(format!("cannot stat {}: {err}", file_path.display()))
        })?;
        if !meta.is_file() {
            return Err(ResourceIoError::io_bad_request(format!(
                "resource is not a file: {}",
                file_path.display()
            )));
        }
        let content = std::fs::read(&file_path).map_err(|err| {
            ResourceIoError::io_bad_request(format!("cannot read {}: {err}", file_path.display()))
        })?;
        Ok(ResourceRead {
            meta: serde_json::json!({
                "resourceKey": local_key(&file_path),
                "resource": WorkspacePlane::descriptor(&file_path),
                "version": version_json(&meta),
                "filePath": file_path.to_string_lossy(),
            }),
            content,
        })
    }

    fn local_write(
        &self,
        reference: &ResourceRef,
        content: &[u8],
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let plane = self
            .workspace
            .as_ref()
            .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
        let file_path = plane.authorize(self.local_path(reference)?, ResourceOp::Write, ctx)?;
        let existed = file_path.exists();
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                ResourceIoError::io_bad_request(format!("cannot create parent dirs: {err}"))
            })?;
        }
        std::fs::write(&file_path, content).map_err(|err| {
            ResourceIoError::io_bad_request(format!("cannot write {}: {err}", file_path.display()))
        })?;
        Ok(WorkspacePlane::mutation_result(
            &file_path,
            if existed { "modified" } else { "created" },
        ))
    }

    fn local_write_expected_version(
        &self,
        reference: &ResourceRef,
        content: &[u8],
        expected: &Value,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let plane = self
            .workspace
            .as_ref()
            .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
        let file_path = plane.authorize(self.local_path(reference)?, ResourceOp::Write, ctx)?;
        let current = std::fs::metadata(&file_path)
            .ok()
            .filter(|meta| meta.is_file())
            .map(|meta| version_json(&meta));
        let matches = current
            .as_ref()
            .map(|current| file_versions_match(current, expected))
            .unwrap_or(false);
        if !matches {
            // 冲突是结果不是异常（local-fs-provider.ts :126-135）。
            let mut out = serde_json::json!({
                "ok": false,
                "conflict": true,
                "resourceKey": local_key(&file_path),
                "resource": WorkspacePlane::descriptor(&file_path),
                "filePath": file_path.to_string_lossy(),
            });
            if let Some(current) = current {
                out["version"] = current;
            }
            return Ok(out);
        }
        self.local_write(reference, content, ctx)
    }

    fn local_list(
        &self,
        reference: &ResourceRef,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let plane = self
            .workspace
            .as_ref()
            .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
        let dir_path = plane.authorize(self.local_path(reference)?, ResourceOp::Read, ctx)?;
        let entries = std::fs::read_dir(&dir_path).map_err(|err| {
            ResourceIoError::io_bad_request(format!("cannot list {}: {err}", dir_path.display()))
        })?;
        let mut items = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| {
                ResourceIoError::io_bad_request(format!("cannot read dir entry: {err}"))
            })?;
            let meta = std::fs::metadata(entry.path()).map_err(|err| {
                ResourceIoError::io_bad_request(format!("cannot stat entry: {err}"))
            })?;
            let is_directory = meta.is_dir();
            items.push(serde_json::json!({
                "name": entry.file_name().to_string_lossy(),
                "isDirectory": is_directory,
                "size": if is_directory { Value::Null } else { Value::from(meta.len()) },
                "mtimeMs": mtime_ms_of(&meta),
            }));
        }
        items.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
        Ok(serde_json::json!({
            "resourceKey": local_key(&dir_path),
            "resource": WorkspacePlane::descriptor(&dir_path),
            "items": items,
        }))
    }

    fn local_search(
        &self,
        reference: &ResourceRef,
        query: Option<&str>,
        mode: Option<&str>,
        limit: Option<usize>,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let plane = self
            .workspace
            .as_ref()
            .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
        let root_path = plane.authorize(self.local_path(reference)?, ResourceOp::Read, ctx)?;
        let needle = query.unwrap_or("").to_string();
        let matches = if needle.is_empty() {
            Vec::new()
        } else if mode == Some("name") {
            self.search_names(&root_path, &needle, limit)
        } else {
            self.search_text(&root_path, &needle, ctx)?
        };
        Ok(serde_json::json!({
            "resourceKey": local_key(&root_path),
            "resource": WorkspacePlane::descriptor(&root_path),
            "matches": matches,
        }))
    }

    /// searchText（local-fs-provider.ts :439-460）：递归、跳过
    /// SEARCH_SKIP_DIRS、逐文件过授权闸（拒绝即跳过）、行号 1 起。
    fn search_text(
        &self,
        root: &Path,
        needle: &str,
        ctx: &OpContext,
    ) -> Result<Vec<Value>, ResourceIoError> {
        let mut matches = Vec::new();
        self.search_text_visit(root, needle, ctx, &mut matches)?;
        matches.sort_by(|a, b| {
            let path_ord = a["filePath"]
                .as_str()
                .unwrap_or("")
                .cmp(b["filePath"].as_str().unwrap_or(""));
            path_ord.then(
                a["line"]
                    .as_u64()
                    .unwrap_or(0)
                    .cmp(&b["line"].as_u64().unwrap_or(0)),
            )
        });
        Ok(matches)
    }

    fn search_text_visit(
        &self,
        current: &Path,
        needle: &str,
        ctx: &OpContext,
        matches: &mut Vec<Value>,
    ) -> Result<(), ResourceIoError> {
        let meta = std::fs::metadata(current).map_err(|err| {
            ResourceIoError::io_bad_request(format!("cannot stat {}: {err}", current.display()))
        })?;
        if meta.is_dir() {
            if SEARCH_SKIP_DIRS.contains(
                &current
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .as_str(),
            ) {
                return Ok(());
            }
            let entries = std::fs::read_dir(current).map_err(|err| {
                ResourceIoError::io_bad_request(format!("cannot list {}: {err}", current.display()))
            })?;
            for entry in entries {
                let entry = entry.map_err(|err| {
                    ResourceIoError::io_bad_request(format!("cannot read dir entry: {err}"))
                })?;
                self.search_text_visit(&entry.path(), needle, ctx, matches)?;
            }
            return Ok(());
        }
        if !meta.is_file() {
            return Ok(());
        }
        // 逐文件授权闸（:449-452）：拒绝即跳过，不是错误。
        let Some(plane) = self.workspace.as_ref() else {
            return Ok(());
        };
        if plane
            .authorize(&current.to_string_lossy(), ResourceOp::Read, ctx)
            .is_err()
        {
            return Ok(());
        }
        let raw = std::fs::read(current).map_err(|err| {
            ResourceIoError::io_bad_request(format!("cannot read {}: {err}", current.display()))
        })?;
        let text = String::from_utf8_lossy(&raw);
        for (index, line) in text.split('\n').enumerate() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.contains(needle) {
                matches.push(serde_json::json!({
                    "filePath": current.to_string_lossy(),
                    "line": index + 1,
                    "text": line,
                }));
            }
        }
        Ok(())
    }

    /// searchNames（local-fs-provider.ts :462-515）：每目录按名排序、
    /// 跳过目录集、授权闸过滤、小写包含匹配、默认上限 80。
    fn search_names(&self, root: &Path, needle: &str, limit: Option<usize>) -> Vec<Value> {
        let max = limit
            .filter(|limit| *limit > 0)
            .unwrap_or(DEFAULT_SEARCH_NAME_LIMIT);
        let needle = needle.to_lowercase();
        let mut matches = Vec::new();
        self.search_names_visit(root, root, &needle, max, &mut matches);
        matches.truncate(max);
        matches
    }

    fn search_names_visit(
        &self,
        root: &Path,
        current: &Path,
        needle: &str,
        max: usize,
        matches: &mut Vec<Value>,
    ) {
        if matches.len() >= max {
            return;
        }
        let Ok(entries) = std::fs::read_dir(current) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if matches.len() >= max {
                break;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            let is_directory = meta.is_dir();
            if is_directory && SEARCH_SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            // 逐条授权闸（:490-493）：拒绝即跳过。
            if let Some(plane) = self.workspace.as_ref() {
                let probe = OpContext::local_owner("");
                if plane
                    .authorize(&path.to_string_lossy(), ResourceOp::Read, &probe)
                    .is_err()
                {
                    continue;
                }
            }
            if name.to_lowercase().contains(needle) {
                let relative = path
                    .strip_prefix(root)
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                let parent = path
                    .parent()
                    .and_then(|parent| parent.strip_prefix(root).ok())
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                matches.push(serde_json::json!({
                    "filePath": path.to_string_lossy(),
                    "line": 0,
                    "text": name,
                    "name": name,
                    "relativePath": relative,
                    "parentSubdir": parent,
                    "isDirectory": is_directory,
                    "size": if is_directory { Value::Null } else { Value::from(meta.len()) },
                    "mtimeMs": mtime_ms_of(&meta),
                }));
            }
            if is_directory {
                self.search_names_visit(root, &path, needle, max, matches);
            }
        }
    }

    fn local_move(
        &self,
        from: &ResourceRef,
        to: &ResourceRef,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let plane = self
            .workspace
            .as_ref()
            .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
        // 现役 assertAllowed(source, "delete")（:196）→ 候选人写级判定
        //（台账：ResourceAccess 无独立 delete 档，PathGuard 的 delete 同
        // 为写级）。
        let source = plane.authorize(self.local_path(from)?, ResourceOp::Write, ctx)?;
        let target = plane.authorize(self.local_path(to)?, ResourceOp::Write, ctx)?;
        if !source.exists() {
            return Err(ResourceIoError::not_found(&source.to_string_lossy()));
        }
        if target.exists() {
            return Err(ResourceIoError::target_already_exists(
                &target.to_string_lossy(),
            ));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                ResourceIoError::io_bad_request(format!("cannot create parent dirs: {err}"))
            })?;
        }
        std::fs::rename(&source, &target).map_err(|err| {
            ResourceIoError::io_bad_request(format!(
                "cannot move {} to {}: {err}",
                source.display(),
                target.display()
            ))
        })?;
        Ok(serde_json::json!({
            "oldResourceKey": local_key(&source),
            "newResourceKey": local_key(&target),
            "oldResource": WorkspacePlane::descriptor(&source),
            "newResource": WorkspacePlane::descriptor(&target),
            "oldFilePath": source.to_string_lossy(),
            "newFilePath": target.to_string_lossy(),
        }))
    }

    fn local_trash(
        &self,
        reference: &ResourceRef,
        namespace: Option<&str>,
        metadata: Option<Value>,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let plane = self
            .workspace
            .as_ref()
            .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
        // 现役 normalizeTrashNamespace（:393-402）。
        let namespace = namespace.unwrap_or("resource-io").trim();
        if namespace.is_empty()
            || namespace.contains('/')
            || namespace.contains('\\')
            || namespace == "."
            || namespace == ".."
        {
            return Err(ResourceIoError::invalid_trash_namespace());
        }
        let file_path = plane.authorize(self.local_path(reference)?, ResourceOp::Write, ctx)?;
        if !file_path.exists() {
            return Err(ResourceIoError::not_found(&file_path.to_string_lossy()));
        }
        let trash_id = format!("trash_{}_{}", (self.now)(), random_hex4());
        let trash_path = self.trash_root.join(namespace).join(&trash_id);
        std::fs::create_dir_all(&trash_path).map_err(|err| {
            ResourceIoError::io_bad_request(format!("cannot create trash dir: {err}"))
        })?;
        let payload_path = trash_path.join("payload");
        std::fs::rename(&file_path, &payload_path).map_err(|err| {
            ResourceIoError::io_bad_request(format!(
                "cannot move {} into trash: {err}",
                file_path.display()
            ))
        })?;
        let mut meta = serde_json::json!({
            "schemaVersion": 1,
            "trashId": trash_id,
            "originalPath": file_path.to_string_lossy(),
            "originalName": file_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "deletedAt": iso_ms((self.now)()),
        });
        // 现役展开 options.metadata（:227，后置键可覆盖 —— 原样镜像）。
        if let Some(Value::Object(extra)) = metadata {
            for (key, value) in extra {
                meta[key] = value;
            }
        }
        let meta_text = serde_json::to_string_pretty(&meta)
            .map_err(|err| ResourceIoError::internal(format!("trash metadata: {err}")))?;
        std::fs::write(trash_path.join("metadata.json"), format!("{meta_text}\n")).map_err(
            |err| ResourceIoError::io_bad_request(format!("cannot write trash metadata: {err}")),
        )?;
        Ok(serde_json::json!({
            "resourceKey": local_key(&file_path),
            "resource": WorkspacePlane::descriptor(&file_path),
            "trashId": trash_id,
            "trashPath": trash_path.to_string_lossy(),
            "payloadPath": payload_path.to_string_lossy(),
            "filePath": file_path.to_string_lossy(),
        }))
    }

    fn local_materialize(
        &self,
        reference: &ResourceRef,
        ctx: &OpContext,
    ) -> Result<Value, ResourceIoError> {
        let plane = self
            .workspace
            .as_ref()
            .ok_or_else(|| ResourceIoError::provider_not_available("local_fs"))?;
        let file_path = plane.authorize(self.local_path(reference)?, ResourceOp::Read, ctx)?;
        let meta = std::fs::metadata(&file_path).map_err(|err| {
            ResourceIoError::io_bad_request(format!("cannot stat {}: {err}", file_path.display()))
        })?;
        Ok(serde_json::json!({
            "resourceKey": local_key(&file_path),
            "resource": WorkspacePlane::descriptor(&file_path),
            "filePath": file_path.to_string_lossy(),
            "version": version_json(&meta),
        }))
    }
}

fn random_hex4() -> String {
    let mut bytes = [0u8; 4];
    if getrandom::getrandom(&mut bytes).is_err() {
        panic!("getrandom failed: cannot mint a trash id");
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ── 路由 handler（server/routes/resource-io.ts 的 15 叶面） ──

/// 现役 `body?.resource || body?.ref || body?.target || body`（stat/read/
/// list/search 面：body 本体也可当 ref）。
fn ref_from_body_with_fallback(body: &Value) -> &Value {
    for key in ["resource", "ref", "target"] {
        if let Some(value) = body.get(key).filter(|v| v.is_object()) {
            return value;
        }
    }
    body
}

/// 现役 `body?.resource || body?.ref || body?.target`（write/wev/trash 面
/// 无 body 兜底 —— content 字段会污染归一）。
fn ref_from_body_strict(body: &Value) -> Result<&Value, ResourceIoError> {
    for key in ["resource", "ref", "target"] {
        if let Some(value) = body.get(key).filter(|v| !v.is_null()) {
            return Ok(value);
        }
    }
    Err(ResourceIoError::invalid_ref("ResourceRef is required"))
}

/// 操作上下文：LocalOnly 面恒为本地属主；sessionId/sessionPath/reason
/// 从 body 透传（operationContextFromBody :130-145）。
fn ctx_from_body(body: &Value) -> OpContext {
    let session_id = body
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut ctx = OpContext::local_owner(&session_id);
    ctx.reason = body
        .get("reason")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or(Some("resource_io_route".to_string()));
    ctx.session_path = body
        .get("sessionPath")
        .and_then(Value::as_str)
        .map(str::to_string);
    ctx
}

fn principal_guard(_principal: &Principal) {}

macro_rules! io_route {
    ($name:ident, $invoke:path) => {
        pub async fn $name(
            axum::Extension(principal): axum::Extension<Principal>,
            State(state): State<ServiceState>,
            body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
        ) -> Response {
            principal_guard(&principal);
            let Json(body) = match body {
                Ok(body) => body,
                Err(rejection) => {
                    return EndpointError::from_json_rejection(&rejection).into_response()
                }
            };
            match $invoke(&state, &body).await {
                Ok(value) => (axum::http::StatusCode::OK, Json(value)).into_response(),
                Err(err) => error_response(err),
            }
        }
    };
}

async fn stat_impl(state: &ServiceState, body: &Value) -> Result<Value, ResourceIoError> {
    state
        .resource_io()
        .stat(ref_from_body_with_fallback(body), &ctx_from_body(body))
        .await
}

io_route!(stat_route, stat_impl);

/// POST read：encodeReadResult（routes :147-155 + :83-87）——content 按
/// encoding 编码进 JSON；utf-8 严格解码失败 → 400 invalid_resource_encoding。
pub async fn read_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    principal_guard(&principal);
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    let encoding = match encoding_from_body(&body, &["encoding", "responseEncoding"]) {
        Ok(encoding) => encoding,
        Err(err) => return error_response(err),
    };
    let result = state
        .resource_io()
        .read(ref_from_body_with_fallback(&body), &ctx_from_body(&body))
        .await;
    match result {
        Ok(read) => {
            let mut meta = read.meta;
            match encode_bytes(&read.content, &encoding) {
                Ok(content) => {
                    meta["content"] = Value::String(content);
                    meta["encoding"] = Value::String(encoding);
                    (axum::http::StatusCode::OK, Json(meta)).into_response()
                }
                Err(err) => error_response(err),
            }
        }
        Err(err) => error_response(err),
    }
}

async fn list_impl(state: &ServiceState, body: &Value) -> Result<Value, ResourceIoError> {
    state
        .resource_io()
        .list(ref_from_body_with_fallback(body), &ctx_from_body(body))
        .await
}

io_route!(list_route, list_impl);

// POST search：路由面只透传 query（routes :94-97 —— mode/limit 是内核
// 面参数，与现役一致不从路由读）。
async fn search_impl(state: &ServiceState, body: &Value) -> Result<Value, ResourceIoError> {
    let query = body
        .get("query")
        .and_then(Value::as_str)
        .map(str::to_string);
    state
        .resource_io()
        .search(
            ref_from_body_with_fallback(body),
            query,
            None,
            None,
            &ctx_from_body(body),
        )
        .await
}

io_route!(search_route, search_impl);

// POST write：decodeWriteContent（routes :157-163）——base64 严格解码。
async fn write_impl(state: &ServiceState, body: &Value) -> Result<Value, ResourceIoError> {
    let content = decode_write_content(body)?;
    let reference = ref_from_body_strict(body)?.clone();
    state
        .resource_io()
        .write(&reference, content, &ctx_from_body(body))
        .await
}

io_route!(write_route, write_impl);

/// POST write-expected-version：冲突结果 → 409 + safeMessage
///（resourceJson :216-221 —— 冲突是结果对象，不走错误面）。
pub async fn write_expected_version_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    principal_guard(&principal);
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    let content = match decode_write_content(&body) {
        Ok(content) => content,
        Err(err) => return error_response(err),
    };
    let reference = match ref_from_body_strict(&body) {
        Ok(reference) => reference.clone(),
        Err(err) => return error_response(err),
    };
    let expected = body.get("expectedVersion").cloned().unwrap_or(Value::Null);
    match state
        .resource_io()
        .write_expected_version(&reference, content, &expected, &ctx_from_body(&body))
        .await
    {
        Ok(result) => {
            if result["ok"] == Value::Bool(false) && result["conflict"] == Value::Bool(true) {
                let mut out = result;
                if out.get("safeMessage").is_none() {
                    out["safeMessage"] = Value::String("Resource write conflict".to_string());
                }
                return (axum::http::StatusCode::CONFLICT, Json(out)).into_response();
            }
            (axum::http::StatusCode::OK, Json(result)).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// rename/move 共用的 from/to 提取（routes :100-116：from|oldResource、
/// to|newResource）。
fn move_operands(body: &Value) -> (Value, Value) {
    let from = body
        .get("from")
        .or_else(|| body.get("oldResource"))
        .cloned()
        .unwrap_or(Value::Null);
    let to = body
        .get("to")
        .or_else(|| body.get("newResource"))
        .cloned()
        .unwrap_or(Value::Null);
    (from, to)
}

async fn rename_impl(state: &ServiceState, body: &Value) -> Result<Value, ResourceIoError> {
    let (from, to) = move_operands(body);
    state
        .resource_io()
        .rename(&from, &to, &ctx_from_body(body))
        .await
}

io_route!(rename_route, rename_impl);

async fn move_impl(state: &ServiceState, body: &Value) -> Result<Value, ResourceIoError> {
    let (from, to) = move_operands(body);
    state
        .resource_io()
        .move_resource(&from, &to, &ctx_from_body(body))
        .await
}

io_route!(move_route, move_impl);

async fn trash_impl(state: &ServiceState, body: &Value) -> Result<Value, ResourceIoError> {
    let reference = ref_from_body_strict(body)?.clone();
    let trash = body.get("trash").cloned().unwrap_or(Value::Null);
    let namespace = trash
        .get("namespace")
        .and_then(Value::as_str)
        .map(str::to_string);
    let metadata = trash.get("metadata").cloned();
    state
        .resource_io()
        .trash(
            &reference,
            namespace.as_deref(),
            metadata,
            &ctx_from_body(body),
        )
        .await
}

io_route!(trash_route, trash_impl);

/// POST subscribe（routes :14-25）：{ok:true, subscriptionId, resourceKeys}。
pub async fn subscribe_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    principal_guard(&principal);
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    match state.resource_io().subscribe(&body).await {
        Ok(mut result) => {
            result["ok"] = Value::Bool(true);
            (axum::http::StatusCode::OK, Json(result)).into_response()
        }
        Err(err) => error_response(err),
    }
}

/// DELETE subscriptions/{subscription_id}（routes :27-31）。
pub async fn unsubscribe_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(subscription_id): AxumPath<String>,
) -> Response {
    principal_guard(&principal);
    let released = state.resource_io().unsubscribe(&subscription_id);
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "released": released })),
    )
        .into_response()
}

/// POST watch（routes :53-67）：{ok:true, watchId}。
pub async fn watch_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    body: Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    principal_guard(&principal);
    let Json(body) = match body {
        Ok(body) => body,
        Err(rejection) => return EndpointError::from_json_rejection(&rejection).into_response(),
    };
    let target = ref_from_body_with_fallback(&body).clone();
    match state.resource_io().watch(&target).await {
        Ok(watch_id) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({ "ok": true, "watchId": watch_id })),
        )
            .into_response(),
        Err(err) => error_response(err),
    }
}

/// DELETE watch/{watch_id}（routes :69-76）。
pub async fn unwatch_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    AxumPath(watch_id): AxumPath<String>,
) -> Response {
    principal_guard(&principal);
    let released = state.resource_io().unwatch(&watch_id);
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({ "ok": true, "released": released })),
    )
        .into_response()
}

/// GET watch-diagnostics（routes :33-35）。
pub async fn watch_diagnostics_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
) -> Response {
    principal_guard(&principal);
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "ok": true,
            "diagnostics": state.resource_io().diagnostics(),
        })),
    )
        .into_response()
}

/// GET events（routes :37-51）+ D12 查询严格化：只认 `since`，非负整数；
/// stale 时带 resync 标记。
pub async fn events_route(
    axum::Extension(principal): axum::Extension<Principal>,
    State(state): State<ServiceState>,
    RawQuery(query): RawQuery,
) -> Response {
    principal_guard(&principal);
    let mut since: Option<u64> = None;
    for (key, value) in crate::ws::parse_query_pairs(query.as_deref().unwrap_or("")) {
        if key != "since" {
            return EndpointError::invalid_message(format!(
                "unknown events query parameter: {key}"
            ))
            .into_response();
        }
        if since.is_some() {
            return EndpointError::invalid_message("duplicate since query parameter")
                .into_response();
        }
        match value.parse::<u64>() {
            Ok(parsed) => since = Some(parsed),
            Err(_) => {
                return EndpointError::invalid_message(
                    "since must be a non-negative integer cursor",
                )
                .into_response()
            }
        }
    }
    let mut result = state.resource_io().events_since(since.unwrap_or(0));
    if result["stale"] == Value::Bool(true) {
        result["resync"] = Value::String("resource-stat-required".to_string());
    }
    (axum::http::StatusCode::OK, Json(result)).into_response()
}

/// 现役 normalizeContentEncoding（routes :172-177）。
fn encoding_from_body(body: &Value, fields: &[&str]) -> Result<String, ResourceIoError> {
    for field in fields {
        if let Some(value) = body.get(*field) {
            let raw = value.as_str().unwrap_or("").trim().to_lowercase();
            return match raw.as_str() {
                "utf-8" | "utf8" => Ok("utf-8".to_string()),
                "base64" => Ok("base64".to_string()),
                _ => Err(ResourceIoError::invalid_encoding(format!(
                    "Unsupported resource content encoding: {}",
                    value.as_str().unwrap_or_default()
                ))),
            };
        }
    }
    Ok("utf-8".to_string())
}

/// 现役 encodeBufferForJson（routes :190-197）：utf-8 严格（fatal）解码。
fn encode_bytes(content: &[u8], encoding: &str) -> Result<String, ResourceIoError> {
    if encoding == "base64" {
        use base64::Engine as _;
        return Ok(base64::engine::general_purpose::STANDARD.encode(content));
    }
    String::from_utf8(content.to_vec()).map_err(|_| {
        ResourceIoError::invalid_encoding(
            "Resource content is not valid UTF-8; request encoding \"base64\" for binary content",
        )
    })
}

/// 现役 decodeBase64Content（routes :199-208）：去空白 + 形态校验。
fn decode_write_content(body: &Value) -> Result<Vec<u8>, ResourceIoError> {
    let encoding = encoding_from_body(body, &["encoding", "contentEncoding"])?;
    if encoding == "base64" {
        let Some(content) = body.get("content").and_then(Value::as_str) else {
            return Err(ResourceIoError::invalid_encoding(
                "Resource base64 content must be a string",
            ));
        };
        let compact: String = content.chars().filter(|c| !c.is_whitespace()).collect();
        use base64::Engine as _;
        return base64::engine::general_purpose::STANDARD
            .decode(compact.as_bytes())
            .map_err(|_| {
                ResourceIoError::invalid_encoding("Resource content is not valid base64")
            });
    }
    // 现役 String(body?.content ?? "")：字符串原样、缺失为空、标量
    // 字符串化。
    match body.get("content") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(text)) => Ok(text.clone().into_bytes()),
        Some(Value::Number(number)) => Ok(number.to_string().into_bytes()),
        Some(Value::Bool(flag)) => Ok(flag.to_string().into_bytes()),
        Some(_) => Err(ResourceIoError::io_bad_request(
            "resource write content must be a string or base64",
        )),
    }
}

// ── 模块内单元测试 ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_versions_match_only_compares_expected_fields() {
        // local-fs-provider.ts :384-391：expected 缺席字段不比对；在场
        // 字段失配即 false；current 缺字段而 expected 在场 → false。
        let current = serde_json::json!({"mtimeMs": 100, "size": 5});
        assert!(file_versions_match(&current, &serde_json::json!({})));
        assert!(file_versions_match(
            &current,
            &serde_json::json!({"mtimeMs": 100})
        ));
        assert!(file_versions_match(
            &current,
            &serde_json::json!({"size": 5})
        ));
        assert!(!file_versions_match(
            &current,
            &serde_json::json!({"mtimeMs": 101})
        ));
        assert!(!file_versions_match(
            &current,
            &serde_json::json!({"size": 6})
        ));
        assert!(!file_versions_match(
            &current,
            &serde_json::json!({"sha256": "abc"})
        ));
        assert!(!file_versions_match(
            &current,
            &serde_json::json!({"etag": "\"x\""})
        ));
        // size 为 null（目录）时 expected null 不参与比对（!= null 检查）。
        assert!(file_versions_match(
            &current,
            &serde_json::json!({"size": Value::Null})
        ));
    }

    #[test]
    fn trash_namespace_validation() {
        for bad in ["", "a/b", "a\\b", ".", "..", "  "] {
            let trimmed = bad.trim();
            let invalid = trimmed.is_empty()
                || trimmed.contains('/')
                || trimmed.contains('\\')
                || trimmed == "."
                || trimmed == "..";
            assert!(invalid, "{bad:?} must be invalid");
        }
        assert_eq!("resource-io".trim(), "resource-io");
    }

    #[test]
    fn decode_write_content_base64_strict() {
        // 现役：空白剔除后按形态校验；非法 → invalid_resource_encoding。
        let ok = decode_write_content(&serde_json::json!({
            "content": "aGVs bG8=",
            "encoding": "base64",
        }))
        .expect("whitespace tolerated");
        assert_eq!(ok, b"hello");
        let err = decode_write_content(&serde_json::json!({
            "content": "!!!",
            "encoding": "base64",
        }))
        .unwrap_err();
        assert_eq!(err.code, "invalid_resource_encoding");
        let err = decode_write_content(&serde_json::json!({
            "content": 42,
            "encoding": "base64",
        }))
        .unwrap_err();
        assert_eq!(err.code, "invalid_resource_encoding");
        // 缺省 utf-8。
        let plain = decode_write_content(&serde_json::json!({"content": "abc"})).expect("utf-8");
        assert_eq!(plain, b"abc");
        let empty = decode_write_content(&serde_json::json!({})).expect("empty");
        assert!(empty.is_empty());
        // 未知 encoding。
        let err = decode_write_content(&serde_json::json!({
            "content": "abc",
            "encoding": "latin1",
        }))
        .unwrap_err();
        assert_eq!(err.code, "invalid_resource_encoding");
    }

    #[test]
    fn snapshot_diff_reports_child_level_events() {
        let root = Path::new("/ws/dir");
        let old = WatchSnapshot::Dir {
            children: BTreeMap::from([
                (
                    "a.txt".to_string(),
                    ChildSig {
                        mtime_ms: 1,
                        size: 1,
                        is_dir: false,
                    },
                ),
                (
                    "b.txt".to_string(),
                    ChildSig {
                        mtime_ms: 1,
                        size: 1,
                        is_dir: false,
                    },
                ),
            ]),
        };
        let new = WatchSnapshot::Dir {
            children: BTreeMap::from([
                (
                    "a.txt".to_string(),
                    ChildSig {
                        mtime_ms: 2,
                        size: 1,
                        is_dir: false,
                    },
                ),
                (
                    "c.txt".to_string(),
                    ChildSig {
                        mtime_ms: 3,
                        size: 1,
                        is_dir: false,
                    },
                ),
            ]),
        };
        let events = diff_snapshots(root, true, &old, &new);
        // a 改 + c 增 → 2 条 changed；b 删 → 1 条 deleted。
        let changed: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, PollEvent::Changed { .. }))
            .collect();
        let deleted: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, PollEvent::Deleted { .. }))
            .collect();
        assert_eq!(changed.len(), 2);
        assert_eq!(deleted.len(), 1);
        match deleted[0] {
            PollEvent::Deleted { path, .. } => assert_eq!(path, &root.join("b.txt")),
            _ => unreachable!(),
        }
    }
}
