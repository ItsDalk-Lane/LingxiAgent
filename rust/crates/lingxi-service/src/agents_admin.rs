//! 助手生命周期集合面（server/routes/agents.ts 的 Rust 端口核心子集）：
//! 列表 / 创建 / 切换 / 删除 / 主助手 / 排序。
//!
//! 端口边界（显式声明，不冒充完整）：
//! - 助手事件及deleteSkills清理已接入；先删除助手，再复核独占技能并逐项真实删除。
//!   非独占或清理失败返回skillsSkipped，成功项才计入skillsDeleted。
//! - 切换复用焦点会话创建流程，返回真实会话、工作区及权限，并记录助手目录历史。
//! - 列表无服务端缓存：fresh=1/true 被接受并等价直读（原实现的缓存
//!   失效语义在无缓存端口上自然成立）。

// R06 A 块读回叶子模块（PUT config 供应商链 / current_status / experiments /
// observation）。模块声明挂在这里而不是 lib.rs：路由注册由主控统一，编译
// 单元声明沿用 agent_memory.rs 对 agent_config.rs 的既有 #[path] 模式。
#[path = "agent_config_readback.rs"]
pub mod agent_config_readback;
#[path = "agent_deletions.rs"]
pub mod agent_deletions;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use lingxi_adapters::storage::agent_memory as am;
use lingxi_adapters::storage::agent_memory::validate_agent_id;
use serde_json::{json, Map, Value};

use crate::memory::MemoryService;
use crate::ServiceState;

/// 助手生命周期写路径共享的偏好互斥：与 workspace-rollback 的 tokio 锁同
/// 目的（串行化 preferences.json 读改写），此处为同步存储函数使用 std 锁。
static PREFERENCES_LOCK: Mutex<()> = Mutex::new(());

const CWD_HISTORY_CAP: usize = 50;

pub struct AgentLifecycleError {
    status: StatusCode,
    pub(crate) message: String,
}

impl AgentLifecycleError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    pub(crate) fn is_missing(&self) -> bool {
        self.status == StatusCode::NOT_FOUND
    }

    pub(crate) fn response(&self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

/// 用户可见文本即 message（响应体 `error` 字段的同一字符串）；日志/调用
/// 方格式化（如 history_content 的失败明细）用同一口径，不另造第二套。
impl std::fmt::Display for AgentLifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl From<am::MemoryError> for AgentLifecycleError {
    fn from(err: am::MemoryError) -> Self {
        match err {
            am::MemoryError::InvalidRequest(detail) => Self::bad_request(detail),
            am::MemoryError::AgentNotFound(detail) | am::MemoryError::NotFound(detail) => Self {
                status: StatusCode::NOT_FOUND,
                message: detail,
            },
            am::MemoryError::Conflict { message, .. } => Self {
                status: StatusCode::CONFLICT,
                message,
            },
            other => Self::internal(format!("agent storage failure: {other}")),
        }
    }
}

pub(crate) fn preferences_locked() -> MutexGuard<'static, ()> {
    PREFERENCES_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn yaml_scalar(text: &str) -> String {
    // JSON 字符串转义是 YAML 双引号标量的合法子集；避免手写转义漏洞。
    serde_json::to_string(text).expect("字符串总可序列化")
}

impl MemoryService {
    /// 严格读取 user/preferences.json；缺失视为空对象，损坏显式报错。
    pub(crate) fn read_preferences_root(&self) -> Result<Value, String> {
        self.read_preferences_strict().map_err(|err| err.message)
    }

    /// 执行前撤权检查复用同一严格读法，只返回宿主状态，不把偏好正文交给模型。
    pub(crate) fn read_preferences_root_at_home(home: &Path) -> Result<Value, String> {
        Self::read_preferences_strict_path(&home.join("user").join("preferences.json"))
            .map_err(|err| err.message)
    }

    pub(crate) fn write_preferences_root(&self, value: &Value) -> Result<(), String> {
        self.write_preferences(value).map_err(|err| err.message)
    }

    /// 复用同一偏好writer，在实际文件队列等待后和rename提交前重验原始授权。
    pub(crate) fn write_preferences_root_authorized(
        &self,
        value: &Value,
        authorize: impl Fn() -> bool,
    ) -> Result<(), String> {
        self.write_preferences_authorized(value, authorize)
            .map_err(|err| err.message)
    }

    fn read_preferences_strict(&self) -> Result<Value, AgentLifecycleError> {
        let path = self.user_dir().join("preferences.json");
        Self::read_preferences_strict_path(&path)
    }

    fn read_preferences_strict_path(path: &Path) -> Result<Value, AgentLifecycleError> {
        let value = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(|err| {
                AgentLifecycleError::internal(format!("invalid preferences.json: {err}"))
            })?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Value::Object(Map::new()),
            Err(err) => {
                return Err(AgentLifecycleError::internal(format!(
                    "cannot read preferences.json: {err}"
                )));
            }
        };
        if !value.is_object() {
            return Err(AgentLifecycleError::internal(
                "preferences.json must contain an object",
            ));
        }
        for key in ["primary_agent_id", "current_agent_id"] {
            if let Some(existing) = value.get(key) {
                if !existing.is_null() && !existing.is_string() {
                    return Err(AgentLifecycleError::internal(format!(
                        "saved {key} must be a string"
                    )));
                }
            }
        }
        if let Some(order) = value.get("agent_order") {
            if !order.is_null()
                && !(order.is_array()
                    && order
                        .as_array()
                        .expect("已判数组")
                        .iter()
                        .all(|item| item.is_string()))
            {
                return Err(AgentLifecycleError::internal(
                    "saved agent_order must be an array of strings",
                ));
            }
        }
        Ok(value)
    }

    fn write_preferences(&self, value: &Value) -> Result<(), AgentLifecycleError> {
        self.write_preferences_authorized(value, || true)
    }

    fn write_preferences_authorized(
        &self,
        value: &Value,
        authorize: impl Fn() -> bool,
    ) -> Result<(), AgentLifecycleError> {
        let text = serde_json::to_string_pretty(value).map_err(|err| {
            AgentLifecycleError::internal(format!("cannot encode preferences.json: {err}"))
        })?;
        am::atomic_write_authorized(&self.user_dir().join("preferences.json"), &text, authorize)
            .map_err(|err| {
                AgentLifecycleError::internal(format!("cannot save preferences.json: {err}"))
            })
    }

    /// 已登记且未删除的助手（目录名 + 目录路径），按目录名稳定排序。
    /// agents 根目录尚未建立（全新 home）视为空集，与首次创建语义衔接。
    fn scan_live_agent_dirs(&self) -> Result<Vec<(String, PathBuf)>, AgentLifecycleError> {
        let root = self.agents_root();
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                return Err(AgentLifecycleError::internal(format!(
                    "cannot read agents root: {err}"
                )));
            }
        };
        let mut out = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| {
                AgentLifecycleError::internal(format!("cannot read agents root entry: {err}"))
            })?;
            let name = entry.file_name().to_string_lossy().to_string();
            if validate_agent_id(&name).is_err() {
                continue;
            }
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            if dir.join(".deleted-agent.json").symlink_metadata().is_ok() {
                continue;
            }
            if !dir.join("config.yaml").is_file() {
                continue;
            }
            out.push((name, dir));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    pub(crate) fn agent_dir_checked(&self, agent_id: &str) -> Result<PathBuf, AgentLifecycleError> {
        validate_agent_id(agent_id)?;
        let dir = self.agents_root().join(agent_id);
        // 名字合法不代表路径仍属于原home；逐组件不跟随链接并核对最终普通文件。
        let checked = (|| -> std::io::Result<()> {
            if dir.canonicalize()? != dir {
                return Err(std::io::Error::other("agent directory contains a link"));
            }
            let held = crate::filetools::fsio::DirFd::open(&dir)?;
            let file = held
                .open_file_nofollow(std::ffi::OsStr::new("config.yaml"))
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))??;
            if !file.metadata()?.is_file() || !held.same_directory(&dir)? {
                return Err(std::io::Error::other("agent configuration path changed"));
            }
            Ok(())
        })();
        if checked.is_err() {
            return Err(AgentLifecycleError::from(am::MemoryError::AgentNotFound(
                format!("agent not found or unsafe configuration path: {agent_id}"),
            )));
        }
        if dir.join(".deleted-agent.json").symlink_metadata().is_ok() {
            return Err(AgentLifecycleError::from(am::MemoryError::AgentNotFound(
                format!("agent not found: {agent_id}"),
            )));
        }
        Ok(dir)
    }

    pub(crate) fn ordered_live_agents(
        &self,
    ) -> Result<Vec<(String, PathBuf)>, AgentLifecycleError> {
        let mut live = self.scan_live_agent_dirs()?;
        let preferences = self.read_preferences_strict()?;
        let order = preferences["agent_order"].as_array();
        if let Some(order) = order {
            let position: std::collections::HashMap<&str, usize> = order
                .iter()
                .filter_map(|item| item.as_str())
                .enumerate()
                .map(|(index, id)| (id, index))
                .collect();
            live.sort_by(|a, b| {
                let pa = position.get(a.0.as_str()).copied().unwrap_or(usize::MAX);
                let pb = position.get(b.0.as_str()).copied().unwrap_or(usize::MAX);
                pa.cmp(&pb).then_with(|| a.0.cmp(&b.0))
            });
        }
        Ok(live)
    }

    pub fn list_agent_summaries(&self) -> Result<Vec<Value>, AgentLifecycleError> {
        let preferences = self.read_preferences_strict()?;
        let primary = preferences["primary_agent_id"].as_str();
        let current = preferences["current_agent_id"].as_str();
        let mut agents = Vec::new();
        for (id, dir) in self.ordered_live_agents()? {
            let scan = am::config::read_agent_config(&dir.join("config.yaml"))?
                .scan()
                .map_err(|err| {
                    AgentLifecycleError::internal(format!(
                        "invalid config.yaml for agent {id}: {err}"
                    ))
                })?;
            // 列表现读简介与头像，不持有写入前的花名册缓存。
            let description =
                crate::persona::read_description(&dir).map_err(AgentLifecycleError::from)?;
            let avatar = ["png", "jpg", "jpeg", "webp"].iter().any(|ext| {
                dir.join("avatars").join(format!("agent.{ext}")).is_file()
                    || dir.join(format!("avatar.{ext}")).is_file()
            });
            agents.push(json!({
                "id": id,
                "name": scan.agent_name.unwrap_or_else(|| id.clone()),
                "description": description,
                "hasAvatar": avatar,
                "yuan": scan.yuan,
                "locale": scan.locale,
                "memoryEnabled": scan.memory_enabled.unwrap_or(true),
                "homeFolder": scan.desk_home_folder,
                "effectiveHomeFolder": scan.desk_home_folder,
                "isPrimary": primary == Some(id.as_str()),
                "isCurrent": current == Some(id.as_str()),
            }));
        }
        Ok(agents)
    }

    /// 其他存活助手的 id（排除指定助手）——deleteSkills 孤儿集计算用。
    pub fn other_live_agent_ids(&self, exclude: &str) -> Vec<String> {
        self.scan_live_agent_dirs()
            .map(|live| {
                live.into_iter()
                    .filter(|(id, _)| id != exclude)
                    .map(|(id, _)| id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 指定助手 config.yaml 的 enabled 技能列表；读取失败返回空（孤儿判定
    /// 保守起见应把失败当"可能启用"，此处由调用方结合其他证据处理）。
    pub fn agent_enabled_skills(&self, agent_id: &str) -> Vec<String> {
        let Ok(text) =
            std::fs::read_to_string(self.agents_root().join(agent_id).join("config.yaml"))
        else {
            return Vec::new();
        };
        am::AgentConfigPatch::read_skill_lists(&text)
            .map(|(enabled, _)| enabled)
            .unwrap_or_default()
    }

    /// 助手显示名：config.yaml 的 agent.name，缺失回退 id；读失败返回 None。
    pub fn agent_display_name(&self, agent_id: &str) -> Option<String> {
        let dir = self.agents_root().join(agent_id);
        let scan = am::config::read_agent_config(&dir.join("config.yaml"))
            .ok()?
            .scan()
            .ok()?;
        Some(scan.agent_name.unwrap_or_else(|| agent_id.to_string()))
    }

    pub fn create_agent(
        &self,
        name: &str,
        id: Option<&str>,
        yuan: Option<&str>,
    ) -> Result<(String, String), AgentLifecycleError> {
        let _deletions = agent_deletions::lock_deletions();
        let reserved = self.reserved_deleted_agent_ids()?;
        let name = name.trim();
        if name.is_empty() {
            return Err(AgentLifecycleError::bad_request("name is required"));
        }
        if let Some(id) = id {
            validate_agent_id(id)?;
        }
        std::fs::create_dir_all(self.agents_root()).map_err(|err| {
            AgentLifecycleError::internal(format!("cannot create agents root: {err}"))
        })?;
        let mut existing: std::collections::HashSet<String> = self
            .scan_live_agent_dirs()?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        existing.extend(reserved);
        let base = id
            .map(str::to_string)
            .unwrap_or_else(|| slug_from_name(name));
        // 目录本身也代表占用：大小写别名、未完成配置和链接不能被当作空位置复用。
        let occupied = |candidate: &str| -> Result<bool, AgentLifecycleError> {
            if existing.contains(candidate) {
                return Ok(true);
            }
            match std::fs::symlink_metadata(self.agents_root().join(candidate)) {
                Ok(_) => Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(error) => Err(AgentLifecycleError::internal(format!(
                    "cannot inspect agent directory before creation: {error}"
                ))),
            }
        };
        let agent_id = if id.is_some() {
            if occupied(&base)? {
                return Err(AgentLifecycleError::from(am::MemoryError::Conflict {
                    code: "agent_already_exists".into(),
                    message: format!("agent already exists: {base}"),
                }));
            }
            base
        } else {
            let mut candidate = base.clone();
            let mut suffix = 2;
            while occupied(&candidate)? {
                candidate = format!("{base}-{suffix}");
                suffix += 1;
            }
            candidate
        };

        let dir = self.agents_root().join(&agent_id);
        // 最后一步必须原子新建；检查后新出现的目录也不能被复用或覆盖。
        std::fs::create_dir(&dir).map_err(|err| {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                AgentLifecycleError::from(am::MemoryError::Conflict {
                    code: "agent_already_exists".into(),
                    message: format!("agent directory already exists: {agent_id}"),
                })
            } else {
                AgentLifecycleError::internal(format!("cannot create agent directory: {err}"))
            }
        })?;
        let mut config = String::new();
        config.push_str("agent:\n");
        config.push_str(&format!("  name: {}\n", yaml_scalar(name)));
        if let Some(yuan) = yuan.filter(|value| !value.trim().is_empty()) {
            config.push_str(&format!("  yuan: {}\n", yaml_scalar(yuan)));
        }
        am::atomic_write(&dir.join("config.yaml"), &config).map_err(|err| {
            AgentLifecycleError::internal(format!("cannot write agent config.yaml: {err}"))
        })?;
        Ok((agent_id, name.to_string()))
    }

    pub fn set_primary_agent(&self, agent_id: &str) -> Result<(), AgentLifecycleError> {
        self.agent_dir_checked(agent_id)?;
        let _guard = preferences_locked();
        let mut preferences = self.read_preferences_strict()?;
        preferences["primary_agent_id"] = json!(agent_id);
        self.write_preferences(&preferences)
    }

    /// R06 焦点会话（现役 switchSession 的服务端焦点指针端口）：会话归属与
    /// 生命周期校验由路由层完成，这里只持久化 current_session_id。
    pub fn set_current_session(&self, session_id: &str) -> Result<(), AgentLifecycleError> {
        let _guard = preferences_locked();
        let mut preferences = self.read_preferences_strict()?;
        preferences["current_session_id"] = json!(session_id);
        self.write_preferences(&preferences)
    }

    /// 焦点会话和助手在同一次偏好原子写入中发布；不替换其它偏好字段。
    pub(crate) fn set_current_focus(
        &self,
        agent_id: &str,
        session_id: &str,
    ) -> Result<(), AgentLifecycleError> {
        self.agent_dir_checked(agent_id)?;
        let _guard = preferences_locked();
        let mut preferences = self.read_preferences_strict()?;
        preferences["current_agent_id"] = json!(agent_id);
        preferences["current_session_id"] = json!(session_id);
        self.write_preferences(&preferences)
    }

    /// 新建焦点会话沿现役约定记住工作目录；配置失败必须交回调用方。
    pub(crate) fn record_session_workspace(
        &self,
        agent_id: &str,
        cwd: &str,
        focus_session: Option<&str>,
    ) -> Result<Vec<String>, AgentLifecycleError> {
        let path = self.agent_dir_checked(agent_id)?.join("config.yaml");
        self.dream().save_config(agent_id, None, || {
            let text = std::fs::read_to_string(&path).map_err(|error| {
                AgentLifecycleError::internal(format!("cannot read config.yaml: {error}"))
            })?;
            let mut document = am::config::AgentConfigDocument::parse(&text)?;
            let mut history = document
                .string_list("desk", "cwd_history")?
                .unwrap_or_default();
            history.retain(|entry| entry != cwd);
            history.push(cwd.to_string());
            if history.len() > CWD_HISTORY_CAP {
                history.drain(..history.len() - CWD_HISTORY_CAP);
            }
            document.set_string("desk", "last_cwd", cwd);
            document.set_string_list("desk", "cwd_history", &history);
            am::atomic_write(&path, &document.render()?).map_err(|error| {
                AgentLifecycleError::internal(format!("cannot remember session workspace: {error}"))
            })?;
            if let Some(session) = focus_session {
                if let Err(error) = self.set_current_focus(agent_id, session) {
                    // 同一配置锁内撤回目录变更；焦点写失败不能留下本次历史。
                    return match am::atomic_write(&path, &text) {
                        Ok(()) => Err(error),
                        Err(rollback) => Err(AgentLifecycleError::internal(format!(
                            "焦点提交失败：{error}；目录历史回退失败：{rollback}"
                        ))),
                    };
                }
            }
            Ok(history)
        })
    }

    /// 当前助手解析顺序：current_agent_id → primary_agent_id → None。
    pub fn current_agent_preference(&self) -> Result<Option<String>, AgentLifecycleError> {
        let preferences = self.read_preferences_strict()?;
        Ok(preferences["current_agent_id"]
            .as_str()
            .map(str::to_string)
            .or_else(|| preferences["primary_agent_id"].as_str().map(str::to_string)))
    }

    pub fn save_agent_order(&self, order: &[String]) -> Result<(), AgentLifecycleError> {
        let live: std::collections::HashSet<String> = self
            .scan_live_agent_dirs()?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let mut seen = std::collections::HashSet::new();
        for id in order {
            validate_agent_id(id)?;
            if !seen.insert(id.clone()) {
                return Err(AgentLifecycleError::bad_request(format!(
                    "duplicate agent id: {id}"
                )));
            }
            if !live.contains(id) {
                return Err(AgentLifecycleError::bad_request(format!(
                    "agent not found: {id}"
                )));
            }
        }
        let _guard = preferences_locked();
        let mut preferences = self.read_preferences_strict()?;
        preferences["agent_order"] = json!(order);
        self.write_preferences(&preferences)
    }
}

fn slug_from_name(name: &str) -> String {
    let mut slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while slug.starts_with('-') {
        slug.remove(0);
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug = "agent".to_string();
    }
    slug.truncate(48);
    slug
}

// ── HTTP 处理器 ─────────────────────────────────────────────────────────────

#[allow(clippy::result_large_err)]
fn parse_body(body: &Bytes) -> Result<Value, Response> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice::<Value>(body).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("invalid JSON body: {err}") })),
        )
            .into_response()
    })
}

pub async fn agents_list_route(
    State(state): State<ServiceState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    // 端口无列表缓存：fresh 与否都直读磁盘（见模块注释）。
    let _fresh = matches!(
        query.get("fresh").map(String::as_str),
        Some("1") | Some("true")
    );
    match state.memory().list_agent_summaries() {
        Ok(agents) => Json(json!({ "agents": agents })).into_response(),
        Err(err) => err.response(),
    }
}

pub async fn agents_create_route(State(state): State<ServiceState>, body: Bytes) -> Response {
    let payload = match parse_body(&body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(name) = payload.get("name").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "name is required" })),
        )
            .into_response();
    };
    let id = payload.get("id").and_then(Value::as_str);
    let yuan = payload.get("yuan").and_then(Value::as_str);
    match state.memory().create_agent(name, id, yuan) {
        Ok((agent_id, agent_name)) => {
            state.app_events().publish(
                "agent-created",
                json!({ "agentId": agent_id, "name": agent_name }),
            );
            Json(json!({ "ok": true, "id": agent_id, "name": agent_name })).into_response()
        }
        Err(err) => err.response(),
    }
}

pub async fn agents_switch_route(
    axum::Extension(principal): axum::Extension<crate::Principal>,
    State(state): State<ServiceState>,
    body: Bytes,
) -> Response {
    let payload = match parse_body(&body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    // 完整请求的会话创建/焦点提交不因HTTP等待者断开而中途丢弃。
    match tokio::spawn(async move { switch_agent(&state, &principal, &payload).await }).await {
        Ok(response) => response,
        Err(error) => {
            AgentLifecycleError::internal(format!("助手切换任务失败：{error}")).response()
        }
    }
}

async fn switch_agent(
    state: &ServiceState,
    principal: &crate::Principal,
    payload: &Value,
) -> Response {
    let focus = state.focus_mutation.lock().await;
    match switch_agent_locked(state, principal, payload, &focus).await {
        Ok(context) => Json(context).into_response(),
        Err(response) => *response,
    }
}

async fn switch_agent_locked(
    state: &ServiceState,
    principal: &crate::Principal,
    payload: &Value,
    focus: &tokio::sync::MutexGuard<'_, ()>,
) -> Result<Value, Box<Response>> {
    let Some(id) = payload.get("id").and_then(Value::as_str) else {
        return Err((AgentLifecycleError::bad_request("invalid id").response()).into());
    };
    if payload
        .get("cwd")
        .is_some_and(|cwd| !cwd.is_null() && !cwd.is_string())
    {
        return Err((AgentLifecycleError::bad_request("cwd must be a string").response()).into());
    }
    // 调用者持有现有焦点锁，覆盖来源读取、创建及广播。
    let dir = match state.memory().agent_dir_checked(id) {
        Ok(dir) => dir,
        Err(error) => return Err((error.response()).into()),
    };
    let config = match am::config::read_agent_config(&dir.join("config.yaml")) {
        Ok(config) => config,
        Err(error) => return Err((AgentLifecycleError::from(error).response()).into()),
    };
    let scan = match config.scan() {
        Ok(scan) => scan,
        Err(error) => return Err((AgentLifecycleError::from(error).response()).into()),
    };
    // 目标助手的完整模型身份先验真，再交原创建流程冻结到会话；不借用全局模型充数。
    let model = match config.model_reference("models", "chat") {
        Ok(value) => value,
        Err(error) => return Err((AgentLifecycleError::from(error).response()).into()),
    };
    if let Some(pair) = model.as_ref() {
        let choice = crate::chat_settings::ChatChoice {
            model: Some(pair.clone()),
            thinking_level: None,
        };
        if let Err(error) = crate::chat_settings::dispatch_choice(state, &choice) {
            return Err((AgentLifecycleError {
                status: StatusCode::CONFLICT,
                message: format!("agent_model_not_available: {error}"),
            }
            .response())
            .into());
        }
    }
    // 配置与历史在创建之前校验，不能先改current_agent再在后续失败。
    if let Err(error) = config.string_list("desk", "cwd_history") {
        return Err((AgentLifecycleError::from(error).response()).into());
    }
    let preferences = match state.memory().read_preferences_strict() {
        Ok(value) => value,
        Err(error) => return Err((error.response()).into()),
    };
    let mut previous_cwd = None;
    let mut mode = None;
    if let Some(sid) = preferences
        .get("current_session_id")
        .and_then(Value::as_str)
    {
        crate::get_session_owned(state, principal, sid).await?;
        let row =
            match lingxi_adapters::storage::session_tree::get_session_row(state.storage(), sid)
                .await
            {
                Ok(Some(row)) => row,
                Ok(None) => return Err((crate::EndpointError::not_found().into_response()).into()),
                Err(error) => {
                    return Err((crate::EndpointError::storage(&error).into_response()).into())
                }
            };
        mode = row.permission_mode;
        previous_cwd = match crate::session_ui::read(state.storage(), sid) {
            Ok(metadata) => metadata.and_then(|metadata| metadata.cwd),
            Err(error) => {
                return Err((crate::EndpointError::storage(&error).into_response()).into())
            }
        };
    }
    let requested = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|cwd| !cwd.is_empty());
    let cwd = requested
        .map(str::to_string)
        .or_else(|| {
            scan.desk_home_folder
                .clone()
                .filter(|cwd| !cwd.trim().is_empty())
        })
        .or(previous_cwd);
    let body = crate::session_focus::NewBody {
        agent_id: Some(id.into()),
        current_agent_id: None,
        current_session_path: None,
        cwd,
        workspace_folders: Vec::new(),
        workspace_mount_id: None,
        project_id: None,
        memory_enabled: Some(scan.memory_enabled.unwrap_or(true)),
        thinking_level: None,
        permission_mode: mode,
        title: None,
        model,
        record_workspace_history: None,
        forked_from_session_id: None,
    };
    let created =
        match crate::session_focus::create_locked(state, principal, body, true, focus).await {
            Ok(value) => value,
            Err(response) => return Err(response),
        };
    // 创建流程已记录真实目录；广播和响应使用同一份已提交会话上下文。
    let context = json!({
        "ok":true, "agent":{"id":id,"name":created["agentName"]},
        "sessionId":created["sessionId"], "sessionPath":created["path"],
        "cwd":created["cwd"], "homeFolder":scan.desk_home_folder,
        "workspaceFolders":created["workspaceFolders"], "authorizedFolders":created["authorizedFolders"],
        "cwdHistory":created["cwdHistory"], "memoryMasterEnabled":scan.memory_enabled.unwrap_or(true),
        "permissionMode":created["permissionMode"], "currentModelProvider":created["currentModelProvider"],
        "currentModelId":created["currentModelId"], "thinkingLevel":created["thinkingLevel"],
    });
    let mut event = context.clone();
    event["agentId"] = json!(id);
    event["agentName"] = created["agentName"].clone();
    state.app_events().publish("agent-switched", event);
    Ok(context)
}

pub async fn agents_delete_route(
    axum::Extension(principal): axum::Extension<crate::Principal>,
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    body: Bytes,
) -> Response {
    let payload = match parse_body(&body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match tokio::spawn(async move { delete_agent(&state, &principal, &agent_id, &payload).await })
        .await
    {
        Ok(response) => response,
        Err(error) => {
            AgentLifecycleError::internal(format!("助手删除任务失败：{error}")).response()
        }
    }
}

async fn delete_agent(
    state: &ServiceState,
    principal: &crate::Principal,
    agent_id: &str,
    payload: &Value,
) -> Response {
    let focus = state.focus_mutation.lock().await;
    if let Err(error) = state.memory().agent_dir_checked(agent_id) {
        return error.response();
    }
    let preferences = match state.memory().read_preferences_strict() {
        Ok(value) => value,
        Err(error) => return error.response(),
    };
    let live = match state.memory().ordered_live_agents() {
        Ok(value) => value,
        Err(error) => return error.response(),
    };
    let candidates: Vec<_> = live.iter().filter(|(id, _)| id != agent_id).collect();
    let Some(replacement) = candidates
        .iter()
        .find(|(id, _)| preferences["primary_agent_id"] == id.as_str())
        .or_else(|| candidates.first())
    else {
        return AgentLifecycleError::bad_request("cannot delete the last agent").response();
    };
    let current = preferences["current_agent_id"]
        .as_str()
        .or_else(|| preferences["primary_agent_id"].as_str());
    let mut focused_agent = None;
    if let Some(sid) = preferences["current_session_id"].as_str() {
        focused_agent = match state.storage().get_session(sid).await {
            Ok(value) => value.map(|session| session.agent_id),
            Err(error) => return crate::EndpointError::storage(&error).into_response(),
        };
    }
    // 必须先成功创建替代助手真实焦点会话，再进入原不可逆删除/清理收据流程。
    // 已有旧会话和费用归属保持原样，不拿旧会话改助手充数。
    let replacement_context = if current == Some(agent_id)
        || focused_agent.as_deref() == Some(agent_id)
    {
        match switch_agent_locked(state, principal, &json!({"id":replacement.0}), &focus).await {
            Ok(context) => Some(context),
            Err(response) => return *response,
        }
    } else {
        None
    };
    // 人物卡应用与删除共用异步门，不能在逻辑删除之后由分段导入重建公共目录。
    // 请求正文已收齐并校验；清理重试使用独立收据入口，不经本路由重入。
    let _application = crate::character_cards::CARD_APPLICATION_GATE.lock().await;
    let requested = payload.get("deleteSkills").and_then(Value::as_array);
    match state.memory().delete_agent(agent_id) {
        Ok(mut deletion) => {
            // 真正切换已广播完整上下文；删除结果保留同一份会话信息。
            if let Some(context) = replacement_context.as_ref() {
                // 原收据仍如实表示未修复的引用；已提交焦点另给完整上下文。
                if deletion["referencesPending"] != true {
                    deletion["replacementAgentId"] = context["agent"]["id"].clone();
                }
                deletion["replacementSwitchResult"] = context.clone();
            }
            state
                .app_events()
                .publish("agent-deleted", json!({ "agentId": agent_id }));
            // 连带技能清理（现役 deleteSkills 语义）：请求名单先与"其他存活
            // 助手均未启用"的孤儿集求交，非孤儿跳过并如实回报；助手删除
            // 成功后才动技能，单项失败只记 skipped 不让整单失败。删除动作
            // 复用 skills_service.delete_skill，调用视图取替代助手（原实现
            // 用引擎级用户技能模型，端口以替代者视图承担只读判定）。
            let mut skills_deleted: Vec<String> = Vec::new();
            let mut skills_skipped: Vec<Value> = Vec::new();
            if let Some(names) = requested {
                let other_live = state.memory().other_live_agent_ids(agent_id);
                let enabled_elsewhere: std::collections::HashSet<String> = other_live
                    .iter()
                    .flat_map(|other| state.memory().agent_enabled_skills(other))
                    .collect();
                // 删除动作复用 skills_service.delete_skill，调用视图取任一其他
                // 存活助手（删除前 ≥2 助手是硬前提，此处必非空；原实现用引擎级
                // 用户技能模型，端口以存活者视图承担只读判定）。
                let Some(view_agent) = other_live.first().cloned() else {
                    deletion["skillsDeleted"] = json!([]);
                    deletion["skillsSkipped"] = json!([{"name":"*","reason":"removal_failed","detail":"no live view agent for skill removal"}]);
                    return Json(deletion).into_response();
                };
                for raw in names.iter().filter_map(|item| item.as_str()) {
                    if enabled_elsewhere.contains(raw) {
                        skills_skipped.push(json!({ "name": raw, "reason": "not_orphan" }));
                        continue;
                    }
                    match state.skills().delete_skill(
                        &view_agent,
                        raw,
                        state.skills().workspace_root().as_deref(),
                    ) {
                        Ok(_) => skills_deleted.push(raw.to_string()),
                        Err(_) => {
                            skills_skipped.push(json!({ "name": raw, "reason": "removal_failed" }))
                        }
                    }
                }
                if !skills_deleted.is_empty() {
                    state
                        .app_events()
                        .publish("skills-changed", json!({ "agentId": Value::Null }));
                }
            }
            deletion["skillsDeleted"] = json!(skills_deleted);
            deletion["skillsSkipped"] = json!(skills_skipped);
            Json(deletion).into_response()
        }
        Err(err) => match replacement_context {
            Some(context) => (err.status, Json(json!({"error":err.message,"replacementAgentId":context["agent"]["id"],"replacementSwitchResult":context}))).into_response(),
            None => err.response(),
        },
    }
}

pub async fn agents_primary_route(State(state): State<ServiceState>, body: Bytes) -> Response {
    let payload = match parse_body(&body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let id = payload.get("id").and_then(Value::as_str);
    let Some(id) = id.filter(|value| !value.trim().is_empty()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "id is required" })),
        )
            .into_response();
    };
    match state.memory().set_primary_agent(id) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(err) => err.response(),
    }
}

pub async fn agents_order_route(State(state): State<ServiceState>, body: Bytes) -> Response {
    let payload = match parse_body(&body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(order) = payload.get("order").and_then(Value::as_array) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "order must be an array" })),
        )
            .into_response();
    };
    let ids: Vec<String> = order
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect();
    if ids.len() != order.len() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "order must be an array of agent ids" })),
        )
            .into_response();
    }
    match state.memory().save_agent_order(&ids) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(err) => err.response(),
    }
}
