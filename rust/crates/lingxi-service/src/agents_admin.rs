//! 助手生命周期集合面（server/routes/agents.ts 的 Rust 端口核心子集）：
//! 列表 / 创建 / 切换 / 删除 / 主助手 / 排序。
//!
//! 端口边界（显式声明，不冒充完整）：
//! - 广播（agent-created / agent-switched / agent-deleted）尚未接入：Rust
//!   服务目前没有 app 事件面；相关原叶保持开放，不以静默省略冒充完成。
//! - deleteSkills 连带清理未实现：请求携带非空名单时显式返回 skipped
//!   （reason=not_implemented_in_port），技能目录保留（原叶缺省分支要求）。
//! - 切换响应的 sessionPath / workspaceFolders / authorizedFolders 不由
//!   服务端跟踪，返回 null / 空数组；可选 body.cwd 写入该助手 config.yaml
//!   的 desk.last_cwd 并去重合并 desk.cwd_history（端口适配：桌面壳知道
//!   实际 cwd，原实现取自引擎进程内状态）。
//! - 列表无服务端缓存：fresh=1/true 被接受并等价直读（原实现的缓存
//!   失效语义在无缓存端口上自然成立）。

// R06 A 块读回叶子模块（PUT config 供应商链 / current_status / experiments /
// observation）。模块声明挂在这里而不是 lib.rs：路由注册由主控统一，编译
// 单元声明沿用 agent_memory.rs 对 agent_config.rs 的既有 #[path] 模式。
#[path = "agent_config_readback.rs"]
pub mod agent_config_readback;

use std::path::PathBuf;
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

    pub(crate) fn write_preferences_root(&self, value: &Value) -> Result<(), String> {
        self.write_preferences(value).map_err(|err| err.message)
    }

    fn read_preferences_strict(&self) -> Result<Value, AgentLifecycleError> {
        let path = self.user_dir().join("preferences.json");
        let value = match std::fs::read(&path) {
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
        let text = serde_json::to_string_pretty(value).map_err(|err| {
            AgentLifecycleError::internal(format!("cannot encode preferences.json: {err}"))
        })?;
        am::atomic_write(&self.user_dir().join("preferences.json"), &text).map_err(|err| {
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
        if !dir.join("config.yaml").is_file() {
            return Err(AgentLifecycleError::from(am::MemoryError::AgentNotFound(
                format!("agent not found: {agent_id}"),
            )));
        }
        if dir.join(".deleted-agent.json").symlink_metadata().is_ok() {
            return Err(AgentLifecycleError::from(am::MemoryError::AgentNotFound(
                format!("agent not found: {agent_id}"),
            )));
        }
        Ok(dir)
    }

    fn ordered_live_agents(&self) -> Result<Vec<(String, PathBuf)>, AgentLifecycleError> {
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
        let existing: std::collections::HashSet<String> = self
            .scan_live_agent_dirs()?
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let base = id
            .map(str::to_string)
            .unwrap_or_else(|| slug_from_name(name));
        let agent_id = if id.is_some() {
            if existing.contains(&base)
                || self
                    .agents_root()
                    .join(&base)
                    .join(".deleted-agent.json")
                    .symlink_metadata()
                    .is_ok()
            {
                return Err(AgentLifecycleError::from(am::MemoryError::Conflict {
                    code: "agent_already_exists".into(),
                    message: format!("agent already exists: {base}"),
                }));
            }
            base
        } else {
            let mut candidate = if existing.contains(&base) {
                format!("{base}-2")
            } else {
                base.clone()
            };
            let mut suffix = 2;
            while existing.contains(&candidate)
                || self
                    .agents_root()
                    .join(&candidate)
                    .join(".deleted-agent.json")
                    .symlink_metadata()
                    .is_ok()
            {
                suffix += 1;
                candidate = format!("{base}-{suffix}");
            }
            candidate
        };

        let dir = self.agents_root().join(&agent_id);
        std::fs::create_dir_all(&dir).map_err(|err| {
            AgentLifecycleError::internal(format!("cannot create agent directory: {err}"))
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

    /// 删除助手；返回替代助手 id（当被删者是当前/主助手时已切换过去）。
    pub fn delete_agent(&self, agent_id: &str) -> Result<Option<String>, AgentLifecycleError> {
        let dir = self.agent_dir_checked(agent_id)?;
        let remaining = self.ordered_live_agents()?;
        let others: Vec<&(String, PathBuf)> =
            remaining.iter().filter(|(id, _)| id != agent_id).collect();
        if others.is_empty() {
            return Err(AgentLifecycleError::bad_request(
                "cannot delete the last agent",
            ));
        }
        let replacement = others.first().map(|(id, _)| id.clone());
        let was_referenced = {
            let preferences = self.read_preferences_strict()?;
            preferences["current_agent_id"].as_str() == Some(agent_id)
                || preferences["primary_agent_id"].as_str() == Some(agent_id)
        };
        std::fs::remove_dir_all(&dir).map_err(|err| {
            AgentLifecycleError::internal(format!("cannot delete agent directory: {err}"))
        })?;
        if replacement.is_some() && was_referenced {
            let _guard = preferences_locked();
            let mut preferences = self.read_preferences_strict()?;
            let mut changed = false;
            if preferences["current_agent_id"].as_str() == Some(agent_id) {
                preferences["current_agent_id"] = json!(replacement);
                changed = true;
            }
            if preferences["primary_agent_id"].as_str() == Some(agent_id) {
                preferences["primary_agent_id"] = json!(replacement);
                changed = true;
            }
            if changed {
                self.write_preferences(&preferences)?;
            }
            return Ok(replacement);
        }
        Ok(None)
    }

    /// 切换当前助手；可选 cwd 写入 desk.last_cwd 并去重合并 desk.cwd_history。
    pub fn switch_agent(
        &self,
        agent_id: &str,
        cwd: Option<&str>,
    ) -> Result<Value, AgentLifecycleError> {
        let dir = self.agent_dir_checked(agent_id)?;
        {
            let _guard = preferences_locked();
            let mut preferences = self.read_preferences_strict()?;
            preferences["current_agent_id"] = json!(agent_id);
            self.write_preferences(&preferences)?;
        }

        let config_path = dir.join("config.yaml");
        let text = std::fs::read_to_string(&config_path).map_err(|err| {
            AgentLifecycleError::internal(format!("cannot read config.yaml: {err}"))
        })?;
        let mut document = am::config::AgentConfigDocument::parse(&text)?;
        let mut history: Vec<String> = document
            .string_list("desk", "cwd_history")?
            .unwrap_or_default();
        let previous_cwd = document.string("desk", "last_cwd")?;
        let cwd = cwd
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or(previous_cwd);
        if let Some(cwd) = &cwd {
            history.retain(|item| item != cwd);
            history.push(cwd.clone());
            if history.len() > CWD_HISTORY_CAP {
                let drop = history.len() - CWD_HISTORY_CAP;
                history.drain(..drop);
            }
            document.set_string("desk", "last_cwd", cwd);
            document.set_string_list("desk", "cwd_history", &history);
            let updated = document.render()?;
            am::atomic_write(&config_path, &updated).map_err(|err| {
                AgentLifecycleError::internal(format!("cannot write config.yaml: {err}"))
            })?;
        }

        let scan = document.scan()?;
        Ok(json!({
            "ok": true,
            "agent": { "id": agent_id, "name": scan.agent_name.clone().unwrap_or_else(|| agent_id.to_string()) },
            "sessionPath": Value::Null,
            "cwd": cwd,
            "homeFolder": scan.desk_home_folder,
            "workspaceFolders": [],
            "authorizedFolders": [],
            "cwdHistory": history,
            "memoryMasterEnabled": scan.memory_enabled.unwrap_or(true),
        }))
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

pub async fn agents_switch_route(State(state): State<ServiceState>, body: Bytes) -> Response {
    let payload = match parse_body(&body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(id) = payload.get("id").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid id" })),
        )
            .into_response();
    };
    let cwd = payload.get("cwd").and_then(Value::as_str);
    match state.memory().switch_agent(id, cwd) {
        Ok(context) => {
            state.app_events().publish(
                "agent-switched",
                json!({
                    "agentId": context["agent"]["id"],
                    "agentName": context["agent"]["name"],
                    "cwd": context["cwd"],
                    "homeFolder": context["homeFolder"],
                    "memoryMasterEnabled": context["memoryMasterEnabled"],
                }),
            );
            Json(context).into_response()
        }
        Err(err) => err.response(),
    }
}

pub async fn agents_delete_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    body: Bytes,
) -> Response {
    let payload = match parse_body(&body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let requested = payload.get("deleteSkills").and_then(Value::as_array);
    match state.memory().delete_agent(&agent_id) {
        Ok(replacement) => {
            // 广播顺序与原实现一致：替代切换先于删除事件。
            if let Some(replacement) = replacement.as_deref() {
                if let Some(name) = state.memory().agent_display_name(replacement) {
                    state.app_events().publish(
                        "agent-switched",
                        json!({ "agentId": replacement, "agentName": name }),
                    );
                }
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
                let other_live = state.memory().other_live_agent_ids(&agent_id);
                let enabled_elsewhere: std::collections::HashSet<String> = other_live
                    .iter()
                    .flat_map(|other| state.memory().agent_enabled_skills(other))
                    .collect();
                // 删除动作复用 skills_service.delete_skill，调用视图取任一其他
                // 存活助手（删除前 ≥2 助手是硬前提，此处必非空；原实现用引擎级
                // 用户技能模型，端口以存活者视图承担只读判定）。
                let Some(view_agent) = other_live.first().cloned() else {
                    return Json(json!({
                        "ok": true,
                        "replacementAgentId": replacement,
                        "skillsDeleted": [],
                        "skillsSkipped": [{
                            "name": "*",
                            "reason": "removal_failed",
                            "detail": "no live view agent for skill removal",
                        }],
                    }))
                    .into_response();
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
            Json(json!({
                "ok": true,
                "replacementAgentId": replacement,
                "skillsDeleted": skills_deleted,
                "skillsSkipped": skills_skipped,
            }))
            .into_response()
        }
        Err(err) => err.response(),
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
