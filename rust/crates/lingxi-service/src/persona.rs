//! R06-T06 PersonaService: agent persona files (AGENTS.md / identity.md /
//! AGENTS.public.md) and avatars — the write faces of the T01 material chain
//! (writing these files changes what the next context compile reads).
//!
//! Anchors (RC-2; design doc §3):
//! - fallback chain: `core/persona-source.ts:58-73,87-116` (read side already
//!   lives in `context_compiler.rs:475`; this module shares the same file
//!   names and fallback order through the adapters store).
//! - routes: `server/routes/agents.ts` (+ `config.ts` ishiki aliases):
//!   GET/PUT agents-md (+ `/ishiki` alias), public-agents-md
//!   (+ `/public-ishiki`), identity.
//! - avatars: png/jpeg/webp, 15MB body cap; read chat scope; write
//!   settings.write (`route-security.ts:100-101,139-141`).

use lingxi_kernel::ports::StoragePort;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, RawQuery, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use serde_json::Value;

use lingxi_adapters::storage::agent_memory as am;
use lingxi_adapters::storage::agent_memory::{validate_agent_id, MemoryError};

use crate::ServiceState;

pub struct PersonaService {
    agents_root: PathBuf,
    product_dir: PathBuf,
    mutations: tokio::sync::Mutex<()>,
}

impl PersonaService {
    pub fn new(agents_root: PathBuf, product_dir: PathBuf) -> Self {
        Self {
            agents_root,
            product_dir,
            mutations: tokio::sync::Mutex::new(()),
        }
    }

    fn store(&self) -> am::PersonaFiles {
        am::PersonaFiles::new(&self.agents_root, &self.product_dir)
    }
}

// 路径必须仍在真实助手目录内；拒绝链接，不借头像或人格地址读写外部文件。
fn regular_or_absent(path: &Path) -> Result<(), MemoryError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(MemoryError::InvalidRequest(
            "persona path must not be a symbolic link".into(),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(MemoryError::Io(error.to_string())),
    }
}
fn persona_file_or_absent(path: &Path) -> Result<(), MemoryError> {
    regular_or_absent(path)?;
    if path.is_dir() {
        return Err(MemoryError::Io("persona file is a directory".into()));
    }
    Ok(())
}
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, MemoryError> {
    persona_file_or_absent(path)?;
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(MemoryError::Io(error.to_string())),
    }
}
/// 本操作创建的临时文件始终清理；落盘失败不泄漏半写正文。
fn write_persona_atomic(path: &Path, bytes: &[u8]) -> Result<(), MemoryError> {
    use std::io::Write as _;
    persona_file_or_absent(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| MemoryError::InvalidRequest("persona has no parent directory".into()))?;
    let temporary = parent.join(format!(
        ".persona-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut created = false;
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        created = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        if created {
            match std::fs::remove_file(&temporary) {
                Ok(()) => {}
                Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => {}
                Err(cleanup) => {
                    return Err(MemoryError::Io(format!(
                        "persona write failed: {error}; temporary cleanup failed: {cleanup}"
                    )))
                }
            }
        }
        return Err(MemoryError::Io(error.to_string()));
    }
    Ok(())
}
fn write_persona_pair(
    path: &Path,
    content: &str,
    description: Option<&str>,
    before: Option<&[u8]>,
) -> Result<(), MemoryError> {
    write_persona_atomic(path, content.as_bytes())?;
    if let Some(description) = description {
        if let Err(error) = write_persona_atomic(
            &path.parent().expect("人格目录").join("description.md"),
            description.as_bytes(),
        ) {
            match read_optional(path) {
                Ok(current) if current.as_deref() == Some(content.as_bytes()) => {},
                Ok(_) => return Err(MemoryError::Io(format!("description commit failed: {error}; persona changed by another writer, rollback refused"))),
                Err(precheck) => return Err(MemoryError::Io(format!("description commit failed: {error}; persona rollback precheck failed: {precheck}"))),
            }
            let rollback = match before {
                Some(bytes) => write_persona_atomic(path, bytes),
                None => std::fs::remove_file(path).map_err(|e| MemoryError::Io(e.to_string())),
            };
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback) => Err(MemoryError::Io(format!(
                    "description commit failed: {error}; persona rollback failed: {rollback}"
                ))),
            };
        }
    }
    Ok(())
}
fn agent_dir(state: &ServiceState, agent: &str) -> Result<PathBuf, MemoryError> {
    validate_agent_id(agent)?;
    let root = &state.persona().agents_root;
    regular_or_absent(root)?;
    let dir = root.join(agent);
    regular_or_absent(&dir)?;
    if !dir.is_dir() || dir.join(".deleted-agent.json").symlink_metadata().is_ok() {
        return Err(MemoryError::AgentNotFound(format!(
            "agent not found: {agent}"
        )));
    }
    Ok(dir)
}
/// 花名册消费者现读已生成的简介；来源校验行不属于用户可见正文。
pub(crate) fn read_description(dir: &Path) -> Result<Option<String>, MemoryError> {
    let path = dir.join("description.md");
    regular_or_absent(&path)?;
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let body = if text
                .lines()
                .next()
                .is_some_and(|line| line.starts_with("<!-- sourceHash:"))
            {
                text.split_once('\n').map(|(_, body)| body).unwrap_or("")
            } else {
                &text
            };
            let body = body.trim();
            Ok((!body.is_empty()).then(|| body.to_owned()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(MemoryError::Io(e.to_string())),
    }
}
fn publish_agent(state: &ServiceState, agent: &str) {
    state
        .app_events()
        .publish("agent-updated", json!({"agentId":agent}));
}

/// 所有实际参与简介的文件都从同一份字节快照解析，包含回落链上尚不存在的优先候选。
/// 模型结算后逐个复检；不能只检查本次写入的那一份人格正文。
struct DescriptionMaterials {
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
    source: String,
    locale: String,
    yuan: String,
}
impl DescriptionMaterials {
    fn text(&mut self, path: PathBuf) -> Result<Option<String>, MemoryError> {
        let bytes = match self.files.iter().find(|(captured, _)| captured == &path) {
            Some((_, bytes)) => bytes.clone(),
            None => {
                let bytes = read_optional(&path)?;
                self.files.push((path, bytes.clone()));
                bytes
            }
        };
        bytes
            .map(|bytes| {
                String::from_utf8(bytes).map_err(|error| {
                    MemoryError::Store(format!("invalid description source encoding: {error}"))
                })
            })
            .transpose()
    }
    fn template(
        &mut self,
        state: &ServiceState,
        kind: am::PersonaKind,
    ) -> Result<String, MemoryError> {
        let (folder, example) = if matches!(kind, am::PersonaKind::Identity) {
            ("identity-templates", "identity.example.md")
        } else {
            ("agents-templates", "agents.example.md")
        };
        let root = &state.persona().product_dir;
        for path in [
            root.join(folder).join(if self.locale.starts_with("zh") {
                format!("{}.md", self.yuan)
            } else {
                format!("en/{}.md", self.yuan)
            }),
            root.join(folder).join(format!("{}.md", self.yuan)),
            root.join(example),
        ] {
            if let Some(text) = self.text(path)? {
                if !text.is_empty() {
                    return Ok(text);
                }
            }
        }
        Ok(String::new())
    }
    fn capture(
        state: &ServiceState,
        agent: &str,
        kind: am::PersonaKind,
        content: &str,
    ) -> Result<Self, MemoryError> {
        let dir = agent_dir(state, agent)?;
        let mut snapshot = Self {
            files: Vec::new(),
            source: String::new(),
            locale: String::new(),
            yuan: String::new(),
        };
        let config_path = dir.join("config.yaml");
        persona_file_or_absent(&config_path)?;
        // 复用既有配置读取预算，不为快照先无界分配配置文件。
        let config_text = match am::config::read_config_text(&config_path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(MemoryError::Io(error.to_string())),
        };
        snapshot.files.push((
            config_path,
            config_text.as_ref().map(|text| text.as_bytes().to_vec()),
        ));
        let config =
            am::config::AgentConfigDocument::parse(config_text.as_deref().unwrap_or(""))?.scan()?;
        let preferences =
            match snapshot.text(state.memory().home().join("user/preferences.json"))? {
                Some(text) => serde_json::from_str::<Value>(&text)
                    .map_err(|error| MemoryError::Store(format!("invalid preferences: {error}")))?,
                None => json!({}),
            };
        if !preferences.is_object() {
            return Err(MemoryError::Store("preferences must be an object".into()));
        }
        let prefs = crate::context_compiler::scan_preferences(Some(&preferences));
        snapshot.locale = config
            .locale
            .filter(|value| !value.trim().is_empty())
            .or(prefs.locale.filter(|value| !value.trim().is_empty()))
            .unwrap_or_else(|| "en".into());
        snapshot.yuan = config.yuan.unwrap_or_else(|| "lingxi".into());
        let own_identity = snapshot.text(dir.join("identity.md"))?;
        let own_agents = snapshot.text(dir.join("AGENTS.md"))?;
        let identity = if matches!(kind, am::PersonaKind::Identity) && !content.is_empty() {
            content.to_string()
        } else if matches!(kind, am::PersonaKind::Identity) {
            snapshot.template(state, am::PersonaKind::Identity)?
        } else {
            match own_identity.filter(|text| !text.is_empty()) {
                Some(text) => text,
                None => snapshot.template(state, am::PersonaKind::Identity)?,
            }
        };
        let agents = if matches!(kind, am::PersonaKind::Agents) && !content.is_empty() {
            content.to_string()
        } else if matches!(kind, am::PersonaKind::Agents) {
            snapshot.template(state, am::PersonaKind::Agents)?
        } else {
            match own_agents.filter(|text| !text.is_empty()) {
                Some(text) => text,
                None => snapshot.template(state, am::PersonaKind::Agents)?,
            }
        };
        let user_name = prefs.user_name.unwrap_or_else(|| {
            if snapshot.locale.starts_with("zh") {
                "用户".into()
            } else {
                "User".into()
            }
        });
        snapshot.source = format!("{identity}\n\n{agents}")
            .replace(
                "{{agentName}}",
                config.agent_name.as_deref().unwrap_or("Lingxi"),
            )
            .replace("{{agentId}}", agent)
            .replace("{{userName}}", &user_name);
        Ok(snapshot)
    }
    fn verify(&self, state: &ServiceState, agent: &str) -> Result<(), MemoryError> {
        let conflict = |message: String| MemoryError::Conflict {
            code: "persona_changed".into(),
            message,
        };
        agent_dir(state, agent).map_err(|error| {
            conflict(format!(
                "description source became unavailable while generating description: {error}"
            ))
        })?;
        for (path, before) in &self.files {
            match read_optional(path) {
                Ok(current) if &current == before => {}
                Ok(_) => {
                    return Err(conflict(
                        "description source changed while generating description".into(),
                    ))
                }
                Err(error) => {
                    return Err(conflict(format!("description source became unavailable while generating description: {error}")));
                }
            }
        }
        Ok(())
    }
}
struct PreparedDescription {
    text: String,
    materials: DescriptionMaterials,
}

/// 从当前材料生成真正的花名册简介；模型失败发生在正文写入前，拒绝不会留下半套人格。
async fn description_for(
    state: &ServiceState,
    agent: &str,
    kind: am::PersonaKind,
    content: &str,
) -> Result<Option<PreparedDescription>, MemoryError> {
    use lingxi_adapters::models::auxiliary::{
        AuxiliaryExecutor, AuxiliaryFailure, AuxiliaryRequest, AuxiliaryStep,
    };
    use lingxi_kernel::model_exchange::{
        AuxiliarySlot, ModelGatewayError, ModelOperation, ModelRouteRequest,
    };
    use lingxi_kernel::usage::{CallOutcome, ReportedUsage};
    let Some(gateway) = state.model_gateway().cloned() else {
        return Ok(None);
    };
    let credentials = state
        .credential_service()
        .cloned()
        .ok_or_else(|| MemoryError::Store("description credentials unavailable".into()))?;
    let mut materials = DescriptionMaterials::capture(state, agent, kind, content)?;
    let hash = Sha256::digest(format!(
        "{}\n{}\n{}",
        materials.source, materials.yuan, materials.locale
    ))
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect::<String>();
    let existing = materials.text(agent_dir(state, agent)?.join("description.md"))?;
    materials.verify(state, agent)?;
    if let Some(text) = existing {
        if text.lines().next() == Some(format!("<!-- sourceHash: {hash} -->").as_str()) {
            return Ok(Some(PreparedDescription { text, materials }));
        }
    }
    // 与现役 summarize 槽同一显式 chat 回退；已配置槽的凭证/供应商失败不换路。
    let operation = match gateway.resolve_dispatch(&ModelRouteRequest::for_operation(
        ModelOperation::Auxiliary(AuxiliarySlot::Summarize),
    )) {
        Ok(_) => ModelOperation::Auxiliary(AuxiliarySlot::Summarize),
        Err(ModelGatewayError::RouteNotConfigured { .. }) => ModelOperation::Chat,
        Err(error) => {
            return Err(MemoryError::Store(format!(
                "description route failed: {error}"
            )))
        }
    };
    let network = state.network_plane().cloned().ok_or_else(|| {
        MemoryError::Store(
            "description network configuration unavailable; no provider request was sent".into(),
        )
    })?;
    let executor = AuxiliaryExecutor::new_with_network(
        gateway,
        credentials,
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
        network,
    )
    .map_err(|e| MemoryError::Store(e.to_string()))?;
    let _permit = state
        .runs()
        .quotas()
        .acquire(
            crate::quotas::QuotaResource::Model,
            agent,
            "persona-description",
        )
        .await
        .map_err(|e| MemoryError::Store(format!("description quota refused: {e}")))?;
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let action = format!(
        "persona-{agent}-{at}-{}",
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("persona-description"),
        run_id: lingxi_protocol::RunId::new(&action),
        attempt: lingxi_protocol::AttemptId::new(&action),
        generation: 1,
    };
    let instruction = if materials.locale.starts_with("zh") {
        "你是产品花名册的第三方编辑。根据助手人格写一段约100字的第三人称简介，涵盖人格、专长、沟通风格和适合任务。纯文本，不用markdown，不输出mood或内部标签，不解释。"
    } else {
        "Write a plain text third-person product roster description, about 100 characters, covering personality, expertise, communication and suitable tasks. No markdown, mood or internal tags. Output only the description."
    };
    let request = AuxiliaryRequest {
        prompt: materials.source.chars().take(3000).collect(),
        images: vec![],
        max_output_tokens: None,
        deadline_unix_ms: Some(at.saturating_add(30_000)),
        system_prompt: Some(instruction.into()),
        prior: vec![],
        tools: None,
    };
    let call = lingxi_protocol::ModelCallId::new(&action);
    let result = match executor
        .complete_step_for_operation(
            &ctx,
            operation,
            "persona description summarize/chat fallback",
            &call,
            &request,
        )
        .await
    {
        Ok(AuxiliaryStep::Text(answer)) => Ok(answer),
        Ok(AuxiliaryStep::ToolRequests {
            usage_report,
            transport_attempts,
            served_by,
            served_protocol,
            ..
        }) => Err(AuxiliaryFailure {
            code: lingxi_protocol::ErrorCode::InvalidMessage,
            message: "description requested tools; no action was executed".into(),
            retryable: false,
            usage_report,
            transport_attempts,
            served_by,
            served_protocol,
        }),
        Err(error) => Err(error),
    };
    let (usage, attempts, served, protocol, outcome) = match &result {
        Ok(answer) => (
            answer.usage_report.clone(),
            answer.transport_attempts,
            answer.served_by.clone(),
            answer.served_protocol.clone(),
            CallOutcome::Succeeded,
        ),
        Err(error) => (
            error.usage_report.clone(),
            error.transport_attempts,
            error.served_by.clone(),
            error.served_protocol.clone(),
            CallOutcome::Failed,
        ),
    };
    let settled = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    state
        .storage()
        .record_model_call_usage(
            lingxi_kernel::usage::ModelCallUsageRecord {
                session_id: None,
                run_id: None,
                attempt: None,
                model_call_id: action,
                purpose: "auxiliary.summarize".into(),
                origin: "persona.description".into(),
                parent_run_id: None,
                cause_ref: Some(format!("agent:{agent}")),
                parent_tool_call_id: None,
                provider: served.provider,
                model: served.model,
                protocol: protocol.unwrap_or_else(|| "unknown".into()),
                usage: if let ReportedUsage::Known(value) = usage.clone() {
                    Some(value)
                } else {
                    None
                },
                invalid_detail: if let ReportedUsage::Invalid { detail } = usage {
                    Some(detail)
                } else {
                    None
                },
                transport_attempts: Some(attempts),
                outcome,
                started_at_unix_ms: Some(at),
                settled_at_unix_ms: Some(settled),
                emitted_tool_calls: vec![],
                cost_basis: None,
            },
            settled,
        )
        .await
        .map_err(|e| MemoryError::Store(e.to_string()))?;
    let text = result
        .map_err(|e| MemoryError::Store(format!("description generation failed: {e}")))?
        .text;
    let text = text.trim().trim_matches('"').trim().to_string();
    if text.is_empty() || text.contains("<mood") || text.contains("<reflect") {
        return Err(MemoryError::Store(
            "description generation returned no usable plain text".into(),
        ));
    }
    Ok(Some(PreparedDescription {
        text: format!("<!-- sourceHash: {hash} -->\n{text}"),
        materials,
    }))
}

fn error_response(err: &MemoryError) -> Response {
    let (status, body): (StatusCode, Value) = match err {
        MemoryError::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, json!({ "error": msg })),
        MemoryError::AgentNotFound(msg) | MemoryError::NotFound(msg) => {
            (StatusCode::NOT_FOUND, json!({ "error": msg }))
        }
        MemoryError::Conflict { code, message } => (
            StatusCode::CONFLICT,
            json!({ "error": message, "code": code }),
        ),
        MemoryError::Io(detail) | MemoryError::Store(detail) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({ "error": detail }),
        ),
    };
    (status, Json(body)).into_response()
}

fn read_preferences(state: &ServiceState) -> Result<Value, MemoryError> {
    match std::fs::read(state.memory().home().join("user/preferences.json")) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|e| MemoryError::Store(format!("invalid preferences: {e}")))?;
            if !value.is_object() {
                return Err(MemoryError::Store("preferences must be an object".into()));
            }
            Ok(value)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(MemoryError::Io(e.to_string())),
    }
}
fn locale_of(state: &ServiceState, agent_id: &str) -> Result<String, MemoryError> {
    // config locale → prefs locale → en (shared chain with MemoryService).
    let home = state.memory().home().to_path_buf();
    let config_locale =
        am::config::read_agent_config(&home.join("agents").join(agent_id).join("config.yaml"))?
            .scan()?
            .locale;
    let preferences = read_preferences(state)?;
    let prefs_locale = crate::context_compiler::scan_preferences(Some(&preferences)).locale;
    Ok(config_locale
        .filter(|l| !l.trim().is_empty())
        .or(prefs_locale.filter(|l| !l.trim().is_empty()))
        .unwrap_or_else(|| "en".to_string()))
}

fn yuan_of(state: &ServiceState, agent_id: &str) -> Result<String, MemoryError> {
    let home = state.memory().home();
    Ok(
        am::config::read_agent_config(&home.join("agents").join(agent_id).join("config.yaml"))?
            .scan()?
            .yuan
            .unwrap_or_else(|| "lingxi".to_string()),
    )
}

macro_rules! persona_get_route {
    ($handler:ident, $kind:expr) => {
        /// GET persona file (fallback chain; fromTemplate flag included).
        pub async fn $handler(
            State(state): State<ServiceState>,
            AxumPath(agent_id): AxumPath<String>,
        ) -> Response {
            if let Err(err) = validate_agent_id(&agent_id) {
                if matches!($kind, am::PersonaKind::Agents) {
                    return (StatusCode::NOT_FOUND, Json(json!({ "error": err.to_string() })))
                        .into_response();
                }
                return error_response(&err);
            }
            let locale = match locale_of(&state, &agent_id) { Ok(value) => value, Err(err) => return error_response(&err) };
            let yuan = match yuan_of(&state, &agent_id) { Ok(value) => value, Err(err) => return error_response(&err) };
            match state.persona().store().resolve_persona(&agent_id, &yuan, &locale, $kind) {
                Ok((content, from_template)) => {
                    Json(json!({ "content": content, "fromTemplate": from_template })).into_response()
                }
                Err(err) => error_response(&err),
            }
        }
    };
}

macro_rules! persona_put_route {
    ($handler:ident, $kind:expr) => {
        /// PUT persona file (persist = user customization).
        pub async fn $handler(
            State(state): State<ServiceState>,
            AxumPath(agent_id): AxumPath<String>,
            body: Bytes,
        ) -> Response {
            if let Err(err) = validate_agent_id(&agent_id) {
                return error_response(&err);
            }
            let payload = match serde_json::from_slice::<Value>(&body) {
                Ok(value) => value,
                Err(err) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("invalid JSON body: {err}") })),
                    )
                        .into_response()
                }
            };
            let Some(content) = payload.get("content").and_then(Value::as_str) else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "content must be a string" })),
                )
                    .into_response();
            };
            let _lock = state.persona().mutations.lock().await;
            let dir = match agent_dir(&state, &agent_id) { Ok(dir)=>dir, Err(err)=>return error_response(&err) };
            let name = if matches!($kind, am::PersonaKind::Identity) { "identity.md" } else { "AGENTS.md" };
            if let Err(err) = persona_file_or_absent(&dir.join(name)) { return error_response(&err); }
            let before = match read_optional(&dir.join(name)) {Ok(bytes)=>bytes,Err(err)=>return error_response(&err)};
            let description = match description_for(&state, &agent_id, $kind, content).await { Ok(value)=>value, Err(err)=>return error_response(&err) };
            // 外部调用期间任一实际材料变化都拒绝；已发生的模型用量已正常入账。
            if let Some(prepared)=&description {if let Err(err)=prepared.materials.verify(&state,&agent_id){return error_response(&err);}}
            if let Err(err)=agent_dir(&state,&agent_id) {return error_response(&err);}
            match read_optional(&dir.join(name)) {Ok(current) if current==before=>{},Ok(_)=>return error_response(&MemoryError::Conflict{code:"persona_changed".into(),message:"persona changed while generating description".into()}),Err(err)=>return error_response(&err)}
            let _publication = am::ContextMaterialMutation::for_home(state.memory().home());
            match write_persona_pair(&dir.join(name),content,description.as_ref().map(|prepared|prepared.text.as_str()),before.as_deref()) {
                Ok(()) => {
                    publish_agent(&state, &agent_id);
                    Json(json!({ "ok": true, "descriptionRefresh": if description.is_some(){"updated"}else{"model_unavailable"} })).into_response()
                },
                Err(err) => error_response(&err),
            }
        }
    };
}

persona_get_route!(agents_md_get_route, am::PersonaKind::Agents);
persona_put_route!(agents_md_put_route, am::PersonaKind::Agents);
persona_get_route!(identity_get_route, am::PersonaKind::Identity);
persona_put_route!(identity_put_route, am::PersonaKind::Identity);

/// GET public persona (file-only chain, no template).
pub async fn public_agents_md_get_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Response {
    if let Err(err) = validate_agent_id(&agent_id) {
        return error_response(&err);
    }
    match state.persona().store().read_public(&agent_id) {
        Ok(content) => Json(json!({ "content": content })).into_response(),
        Err(err) => error_response(&err),
    }
}

/// PUT public persona.
pub async fn public_agents_md_put_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    body: Bytes,
) -> Response {
    if let Err(err) = validate_agent_id(&agent_id) {
        return error_response(&err);
    }
    let payload = match serde_json::from_slice::<Value>(&body) {
        Ok(value) => value,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("invalid JSON body: {err}") })),
            )
                .into_response()
        }
    };
    let Some(content) = payload.get("content").and_then(Value::as_str) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "content must be a string" })),
        )
            .into_response();
    };
    let _lock = state.persona().mutations.lock().await;
    let dir = match agent_dir(&state, &agent_id) {
        Ok(dir) => dir,
        Err(err) => return error_response(&err),
    };
    if let Err(err) = persona_file_or_absent(&dir.join("AGENTS.public.md")) {
        return error_response(&err);
    }
    let _publication = am::ContextMaterialMutation::for_home(state.memory().home());
    match write_persona_atomic(&dir.join("AGENTS.public.md"), content.as_bytes()) {
        Ok(()) => {
            publish_agent(&state, &agent_id);
            Json(json!({ "ok": true })).into_response()
        }
        Err(err) => error_response(&err),
    }
}

pub const MAX_AVATAR_BYTES: usize = 15 * 1024 * 1024;

const AVATAR_EXTS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
fn avatar_paths(
    state: &ServiceState,
    role: &str,
    agent: Option<&str>,
) -> Result<Vec<PathBuf>, MemoryError> {
    let (dir, legacy) = match role {
        "agent" => {
            let id =
                agent.ok_or_else(|| MemoryError::AgentNotFound("agentId is required".into()))?;
            let dir = agent_dir(state, id)?;
            (dir.join("avatars"), Some(dir))
        }
        "user" => {
            let dir = state.memory().home().join("user");
            regular_or_absent(&dir)?;
            (dir.join("avatars"), None)
        }
        _ => {
            return Err(MemoryError::InvalidRequest(
                "role must be agent or user".into(),
            ))
        }
    };
    regular_or_absent(&dir)?;
    let mut paths: Vec<PathBuf> = AVATAR_EXTS
        .iter()
        .map(|ext| dir.join(format!("{role}.{ext}")))
        .collect();
    // 保留已迁入候选根目录的旧头像兼容读；新写入统一使用现役 avatars 目录。
    if let Some(dir) = legacy {
        paths.extend(
            AVATAR_EXTS
                .iter()
                .map(|ext| dir.join(format!("avatar.{ext}"))),
        );
    }
    for path in &paths {
        regular_or_absent(path)?;
        if path.is_dir() {
            return Err(MemoryError::Io("avatar file is a directory".into()));
        }
    }
    Ok(paths)
}
fn read_avatar_paths(paths: &[PathBuf]) -> Result<Option<(&'static str, Vec<u8>)>, MemoryError> {
    for path in paths {
        match std::fs::read(path) {
            Ok(bytes) => {
                let mime = match path.extension().and_then(|s| s.to_str()) {
                    Some("png") => "image/png",
                    Some("webp") => "image/webp",
                    _ => "image/jpeg",
                };
                return Ok(Some((mime, bytes)));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(MemoryError::Io(err.to_string())),
        }
    }
    Ok(None)
}
fn avatar_response(
    found: Result<Option<(&'static str, Vec<u8>)>, MemoryError>,
    headers: &HeaderMap,
) -> Response {
    match found {
        Ok(Some((mime, bytes))) => {
            let mut digest = Sha256::new();
            digest.update(mime.as_bytes());
            digest.update(&bytes);
            let etag = format!(
                "\"{}\"",
                digest
                    .finalize()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
            if headers
                .get(header::IF_NONE_MATCH)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| {
                    v.split(',')
                        .any(|candidate| candidate.trim() == etag || candidate.trim() == "*")
                })
            {
                return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
            }
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime.to_string()),
                    (
                        header::CACHE_CONTROL,
                        "private, max-age=3600, must-revalidate".into(),
                    ),
                    (header::ETAG, etag),
                ],
                bytes,
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"avatar not found"})),
        )
            .into_response(),
        Err(err) => error_response(&err),
    }
}
fn decode_avatar(body: &[u8]) -> Result<(String, Vec<u8>), MemoryError> {
    if body.len() > MAX_AVATAR_BYTES {
        return Err(MemoryError::InvalidRequest(
            "avatar body exceeds the 15MB limit".into(),
        ));
    }
    let payload: Value =
        serde_json::from_slice(body).map_err(|e| MemoryError::InvalidRequest(e.to_string()))?;
    let url = payload
        .get("data")
        .or_else(|| payload.get("dataUrl"))
        .and_then(Value::as_str)
        .ok_or_else(|| MemoryError::InvalidRequest("data must be a base64 data URL".into()))?;
    let (mime, encoded) = url
        .strip_prefix("data:")
        .and_then(|s| s.split_once(";base64,"))
        .ok_or_else(|| MemoryError::InvalidRequest("invalid data URL".into()))?;
    let ext = match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        _ => {
            return Err(MemoryError::InvalidRequest(
                "avatar must be png, jpeg or webp".into(),
            ))
        }
    };
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| MemoryError::InvalidRequest("invalid base64 payload".into()))?;
    if bytes.is_empty() {
        return Err(MemoryError::InvalidRequest(
            "avatar payload is empty".into(),
        ));
    }
    Ok((ext.into(), bytes))
}
fn write_avatar_paths(paths: &[PathBuf], ext: &str, bytes: &[u8]) -> Result<(), MemoryError> {
    let destination = paths
        .iter()
        .take(4)
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some(ext))
        .expect("已校验头像格式");
    let parent = destination.parent().expect("头像目录");
    std::fs::create_dir_all(parent).map_err(|e| MemoryError::Io(e.to_string()))?;
    let before: Vec<(PathBuf, Vec<u8>)> = paths
        .iter()
        .filter(|p| p.is_file())
        .map(|p| std::fs::read(p).map(|b| (p.clone(), b)))
        .collect::<Result<_, _>>()
        .map_err(|e| MemoryError::Io(e.to_string()))?;
    let temporary = parent.join(format!(
        ".avatar-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut created = false;
    let result = (|| -> std::io::Result<()> {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        created = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        for path in paths {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        std::fs::rename(&temporary, destination)
    })();
    if let Err(error) = result {
        let mut compensation = Vec::new();
        for (path, bytes) in before {
            if let Err(e) = write_persona_atomic(&path, &bytes) {
                compensation.push(format!("avatar restoration failed: {e}"));
            }
        }
        if created {
            match std::fs::remove_file(&temporary) {
                Ok(()) => {}
                Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => {}
                Err(cleanup) => compensation.push(format!("temporary cleanup failed: {cleanup}")),
            }
        }
        return Err(MemoryError::Io(format!(
            "avatar write failed: {error}; rollback errors: {compensation:?}"
        )));
    }
    Ok(())
}
fn delete_avatar_paths(paths: &[PathBuf]) -> Result<(), MemoryError> {
    let before: Vec<(PathBuf, Vec<u8>)> = paths
        .iter()
        .map(|path| read_optional(path).map(|bytes| bytes.map(|bytes| (path.clone(), bytes))))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    for path in paths {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                let mut errors = Vec::new();
                for (file, bytes) in &before {
                    if !file.exists() {
                        if let Err(error) = write_persona_atomic(file, bytes) {
                            errors.push(error.to_string());
                        }
                    }
                }
                return Err(MemoryError::Io(format!(
                    "avatar delete failed: {e}; rollback errors: {errors:?}"
                )));
            }
        }
    }
    Ok(())
}
async fn post_avatar(
    state: &ServiceState,
    role: &str,
    agent: Option<&str>,
    body: &[u8],
) -> Response {
    if body.len() > MAX_AVATAR_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({"error":"avatar body exceeds the 15MB limit"})),
        )
            .into_response();
    }
    let (ext, bytes) = match decode_avatar(body) {
        Ok(v) => v,
        Err(e) => return error_response(&e),
    };
    let _lock = state.persona().mutations.lock().await;
    let paths = match avatar_paths(state, role, agent) {
        Ok(v) => v,
        Err(e) => return error_response(&e),
    };
    match write_avatar_paths(&paths, &ext, &bytes) {
        Ok(()) => {
            if role == "agent" {
                if let Some(id) = agent {
                    publish_agent(state, id);
                }
            }
            Json(json!({"ok":true,"format":ext,"ext":ext})).into_response()
        }
        Err(e) => error_response(&e),
    }
}
async fn delete_avatar(state: &ServiceState, role: &str, agent: Option<&str>) -> Response {
    let _lock = state.persona().mutations.lock().await;
    let paths = match avatar_paths(state, role, agent) {
        Ok(v) => v,
        Err(e) => return error_response(&e),
    };
    match delete_avatar_paths(&paths) {
        Ok(()) => {
            if role == "agent" {
                if let Some(id) = agent {
                    publish_agent(state, id);
                }
            }
            Json(json!({"ok":true})).into_response()
        }
        Err(e) => error_response(&e),
    }
}
pub async fn avatar_get_route(
    State(state): State<ServiceState>,
    AxumPath(agent): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    let _lock = state.persona().mutations.lock().await;
    let found = avatar_paths(&state, "agent", Some(&agent)).and_then(|p| read_avatar_paths(&p));
    avatar_response(found, &headers)
}
pub async fn avatar_post_route(
    State(state): State<ServiceState>,
    AxumPath(agent): AxumPath<String>,
    body: Bytes,
) -> Response {
    post_avatar(&state, "agent", Some(&agent), &body).await
}
pub async fn avatar_delete_route(
    State(state): State<ServiceState>,
    AxumPath(agent): AxumPath<String>,
) -> Response {
    delete_avatar(&state, "agent", Some(&agent)).await
}
pub async fn role_avatar_get_route(
    State(state): State<ServiceState>,
    AxumPath(role): AxumPath<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let _lock = state.persona().mutations.lock().await;
    let found = avatar_paths(&state, &role, query.get("agentId").map(String::as_str))
        .and_then(|p| read_avatar_paths(&p));
    avatar_response(found, &headers)
}
pub async fn role_avatar_post_route(
    State(state): State<ServiceState>,
    AxumPath(role): AxumPath<String>,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    post_avatar(
        &state,
        &role,
        query.get("agentId").map(String::as_str),
        &body,
    )
    .await
}
pub async fn role_avatar_delete_route(
    State(state): State<ServiceState>,
    AxumPath(role): AxumPath<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    delete_avatar(&state, &role, query.get("agentId").map(String::as_str)).await
}
pub async fn avatar_path_route(
    State(state): State<ServiceState>,
    AxumPath(agent): AxumPath<String>,
    RawQuery(_query): RawQuery,
) -> Response {
    let _lock = state.persona().mutations.lock().await;
    match avatar_paths(&state,"agent",Some(&agent)){Ok(paths)=>Json(json!({"path":paths.iter().find(|p|p.is_file()).map(|p|p.to_string_lossy().into_owned())})).into_response(),Err(e)=>error_response(&e)}
}
