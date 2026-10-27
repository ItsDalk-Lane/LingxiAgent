//! update_settings：网关冻结本机交互身份后，搜索或修改现役14项设置。
//! scope来自shared/config-schema.ts；模型可调字典来自lib/tools/update-settings-tool.ts。
use crate::{
    agents_admin::agent_config_readback as config,
    appevents::AppEventLog,
    auth::{AuthService, Principal},
    memory::MemoryService,
    toolgateway::{self, InvocationPermissionContext, ToolInvocationGateway},
};
use lingxi_adapters::{models::gateway::ConfigModelGateway, storage::agent_memory as am};
use lingxi_kernel::{
    model_exchange::{ModelOperation, ModelRouteRequest},
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    subagent::SessionPermissionMode,
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Map, Value};
use std::sync::Arc;

pub struct SettingsToolDeps {
    pub memory: Arc<MemoryService>,
    pub auth: Arc<AuthService>,
    pub events: Arc<AppEventLog>,
    pub media: Arc<crate::media::MediaService>,
    pub bridge_media: Arc<crate::bridgemedia::BridgeMediaService>,
    pub models: Option<Arc<ConfigModelGateway>>,
    pub mcp: Arc<crate::mcp_settings::McpSettingsManager>,
}

struct Setting {
    key: &'static str,
    label: &'static str,
    kind: &'static str,
    scope: &'static str,
    terms: &'static str,
    options: &'static [&'static str],
}
const SETTINGS: &[Setting] = &[
    Setting {
        key: "file_backup",
        label: "文件备份保留天数",
        kind: "list",
        scope: "global",
        terms: "backup 备份 checkpoint retention 保留",
        options: &["1", "3", "7"],
    },
    Setting {
        key: "locale",
        label: "语言",
        kind: "list",
        scope: "global",
        terms: "language 国际化 言語 언어",
        options: &["zh-CN", "zh-TW", "ja", "ko", "en"],
    },
    Setting {
        key: "timezone",
        label: "时区",
        kind: "text",
        scope: "global",
        terms: "timezone time 时区",
        options: &[],
    },
    Setting {
        key: "bridge_media_public_base_url",
        label: "桥接媒体公开地址",
        kind: "text",
        scope: "global",
        terms: "bridge media public url 公网 文件发送 qq tunnel ngrok cloudflare",
        options: &[],
    },
    Setting {
        key: "computer_use.enabled",
        label: "电脑操作启用偏好",
        kind: "toggle",
        scope: "global",
        terms: "computer use 电脑控制 使用电脑 desktop gui",
        options: &[],
    },
    Setting {
        key: "thinking_level",
        label: "当前会话思考档位",
        kind: "list",
        scope: "session",
        terms: "reasoning 推理 思考 推論",
        options: &["off", "low", "medium", "high", "max", "xhigh"],
    },
    Setting {
        key: "memory.enabled",
        label: "记忆开关",
        kind: "toggle",
        scope: "agent",
        terms: "memory 记忆 enabled",
        options: &[],
    },
    Setting {
        key: "memory.facts",
        label: "可编辑记忆事实",
        kind: "text",
        scope: "agent",
        terms: "facts editable memory 记忆事实 重要事实 可编辑记忆",
        options: &[],
    },
    Setting {
        key: "experience.enabled",
        label: "经验开关",
        kind: "toggle",
        scope: "agent",
        terms: "experience 经验 成長 成长 経験 경험",
        options: &[],
    },
    Setting {
        key: "agent.name",
        label: "助手名称",
        kind: "text",
        scope: "agent",
        terms: "agent name 助手 名称 名字",
        options: &[],
    },
    Setting {
        key: "user.name",
        label: "用户名称",
        kind: "text",
        scope: "global",
        terms: "user name 用户 称呼 名字",
        options: &[],
    },
    Setting {
        key: "home_folder",
        label: "助手默认工作目录",
        kind: "text",
        scope: "agent",
        terms: "home folder workspace 工作目录 文件夹",
        options: &[],
    },
    Setting {
        key: "theme",
        label: "外观主题",
        kind: "list",
        scope: "global",
        terms: "theme dark light 暗色 亮色 外观 appearance 夜间 ダーク 다크",
        options: &[],
    },
    Setting {
        key: "models.chat",
        label: "助手默认聊天模型",
        kind: "list",
        scope: "agent",
        terms: "model 模型 モデル 모델 chat",
        options: &[],
    },
];

fn forbidden_key(key: &str) -> bool {
    matches!(key, "sandbox" | "sandbox_network")
        || key.starts_with("security")
        || key.starts_with("providers")
        || key.contains("api_key")
        || key.contains("secret")
        || key.contains("credential")
        || key.contains("permission")
        || key.contains("app_approvals")
}
fn nested_patch(key: &str, value: Value) -> Value {
    let mut value = value;
    for part in key.split('.').rev() {
        let mut parent = Map::new();
        parent.insert(part.into(), value);
        value = Value::Object(parent);
    }
    value
}
fn themes() -> Result<Vec<String>, String> {
    let data: Value = serde_json::from_str(include_str!(
        "../../../../desktop/src/shared/theme-registry-data.json"
    ))
    .map_err(|_| "主题注册数据损坏")?;
    let mut ids: Vec<_> = data["themes"]
        .as_object()
        .ok_or("主题注册表缺失")?
        .keys()
        .cloned()
        .collect();
    ids.push(
        data["autoOption"]["id"]
            .as_str()
            .ok_or("自动主题定义缺失")?
            .into(),
    );
    Ok(ids)
}
fn boolean(value: &Value) -> Result<bool, String> {
    match value {
        Value::Bool(v) => Ok(*v),
        Value::String(v) if v == "true" => Ok(true),
        Value::String(v) if v == "false" => Ok(false),
        _ => Err("开关值必须为true或false".into()),
    }
}
fn text(value: &Value) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "设置值必须为文本".into())
}

/// 安全全局子集，不能把任意GLOBAL_FIELDS当成模型可写白名单。
pub(crate) fn apply_global(
    memory: &MemoryService,
    events: &AppEventLog,
    principal: &Principal,
    patch: &Value,
) -> Result<Value, String> {
    if !principal.has_scope("settings.write") {
        return Err("settings.write required".into());
    }
    let mut secrets = Vec::new();
    config::collect_secret_patch_paths(patch, "", &mut secrets);
    if !secrets.is_empty() {
        return Err("模型设置工具不能修改秘密".into());
    }
    let (fields, rest) = config::split_by_scope(patch);
    if rest.as_object().is_none_or(|m| !m.is_empty())
        || fields.is_empty()
        || fields
            .iter()
            .any(|(key, _)| !matches!(*key, "locale" | "timezone" | "file_backup" | "user.name"))
    {
        return Err("该设置不是模型可调全局偏好".into());
    }
    let output = {
        let _guard = crate::agents_admin::preferences_locked();
        let mut preferences = memory.read_preferences_root()?;
        for (key, value) in &fields {
            config::apply_global_setter(&mut preferences, key, value)?;
        }
        memory.write_preferences_root(&preferences)?;
        let mut projected = Map::new();
        config::inject_global_fields(&mut projected, &preferences);
        Value::Object(projected)
    };
    for (key, value) in fields {
        let event = if key == "locale" {
            "locale-changed"
        } else {
            "config_changed"
        };
        let payload = if key == "locale" {
            json!({"locale":value})
        } else {
            json!({"global":true,"key":key,"value":value})
        };
        events.publish(event, payload);
    }
    Ok(config::mask_object_secrets(&output))
}

struct SettingsTools {
    deps: SettingsToolDeps,
}
impl SettingsTools {
    fn agent_config(&self, agent: &str) -> Result<Value, String> {
        let dir = self
            .deps
            .memory
            .agent_dir_checked(agent)
            .map_err(|e| e.to_string())?;
        am::config::read_yaml_mapping_as_json(&dir.join("config.yaml"))
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "目标助手配置不存在".into())
    }
    fn available_models(&self) -> Vec<String> {
        let Some(gateway) = self.deps.models.as_ref() else {
            return vec![];
        };
        let mut result = Vec::new();
        for (provider, models) in gateway.catalog_models() {
            for model in models {
                if gateway
                    .resolve_dispatch(&ModelRouteRequest {
                        operation: ModelOperation::Chat,
                        provider: Some(provider.clone()),
                        model: Some(model.id.clone()),
                    })
                    .is_ok()
                {
                    result.push(format!("{provider}/{}", model.id));
                }
            }
        }
        result
    }
    fn current(&self, key: &str, agent: &str, ctx: &RunContext) -> Result<Value, String> {
        let config = self.agent_config(agent)?;
        let prefs = self.deps.memory.read_preferences_root()?;
        Ok(match key {
            "file_backup" => prefs
                .pointer("/file_backup/retention_days")
                .cloned()
                .unwrap_or(json!(1)),
            "locale" => prefs.get("locale").cloned().unwrap_or(json!("zh-CN")),
            "timezone" => prefs.get("timezone").cloned().unwrap_or(Value::Null),
            "bridge_media_public_base_url" => prefs
                .pointer("/bridge/mediaPublicBaseUrl")
                .cloned()
                .unwrap_or(json!("")),
            "computer_use.enabled" => json!(crate::computer_use_settings::tool_enabled(
                &self.deps.memory
            )?),
            "thinking_level" => crate::chat_settings::choice(
                self.deps.memory.home(),
                &ctx.principal.storage_subject(),
                ctx.session_id.as_str(),
            )?
            .thinking_level
            .map_or_else(
                || {
                    prefs
                        .get("thinking_level")
                        .cloned()
                        .unwrap_or(json!("medium"))
                },
                |v| json!(v),
            ),
            "memory.enabled" => json!(
                self.deps
                    .memory
                    .read_flags(agent)
                    .map_err(|e| e.to_string())?
                    .memory_enabled
                    != Some(false)
            ),
            "memory.facts" => {
                let store = self
                    .deps
                    .memory
                    .compiled_store(agent)
                    .map_err(|e| e.to_string())?;
                // 与现役首次读取一致：幂等迁移旧editable-facts后读取规范事实，不吞掉损坏错误。
                store
                    .migrate_legacy_editable_facts()
                    .map_err(|e| e.to_string())?;
                json!(store.read_sections().map_err(|e| e.to_string())?.facts)
            }
            "experience.enabled" => json!(
                self.deps
                    .memory
                    .read_flags(agent)
                    .map_err(|e| e.to_string())?
                    .experience_enabled
                    == Some(true)
            ),
            "agent.name" => config
                .pointer("/agent/name")
                .cloned()
                .unwrap_or(json!(agent)),
            "user.name" => prefs.get("userName").cloned().unwrap_or(json!("")),
            "home_folder" => config
                .pointer("/desk/home_folder")
                .cloned()
                .unwrap_or(Value::Null),
            "theme" => prefs
                .pointer("/appearance/theme")
                .cloned()
                .unwrap_or(json!("auto")),
            "models.chat" => config
                .pointer("/models/chat")
                .cloned()
                .unwrap_or(Value::Null),
            _ => return Err("未知设置".into()),
        })
    }
    fn search(&self, query: &str, agent: &str, ctx: &RunContext) -> Result<Value, String> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Err("search需要query".into());
        }
        let mut rows = Vec::new();
        for (key, label, agent_scoped) in crate::mcp_settings::ACTIONS {
            if format!("{key} {label} mcp connector server tool 连接器 工具 启用 停止")
                .to_lowercase()
                .contains(&query)
            {
                rows.push(json!({"key":key,"label":label,"type":"text","scope":if *agent_scoped{"agent"}else{"global"},"currentValue":Value::Null,"requiresConnectorId":!key.ends_with("global.enabled")&&!key.ends_with("connector.add"),"modelEditable":true,"source":"lib/tools/update-settings-tool.ts MCP_SETTINGS_ACTIONS；同一MCP管理器执行，命令和秘密变更仍受用户专属边界限制"}));
            }
        }
        for setting in SETTINGS {
            let options: Vec<String> = match setting.key {
                "models.chat" => self.available_models(),
                "theme" => themes()?,
                _ => setting.options.iter().map(|v| (*v).into()).collect(),
            };
            let haystack = format!(
                "{} {} {} {}",
                setting.key,
                setting.label,
                setting.terms,
                options.join(" ")
            )
            .to_lowercase();
            if haystack.contains(&query) {
                rows.push(json!({"key":setting.key,"label":setting.label,"type":setting.kind,"scope":setting.scope,"currentValue":self.current(setting.key,agent,ctx)?,"options":options,"modelEditable":true,"source":"lib/tools/update-settings-tool.ts; scope: shared/config-schema.ts"}));
            }
        }
        if ["sandbox", "security", "安全", "沙箱"]
            .iter()
            .any(|key| query.contains(key))
        {
            rows.push(json!({"key":"security.user_only","label":"安全、沙箱与MCP秘密由用户在设置页修改","modelEditable":false}));
        }
        Ok(json!({"action":"search","settings":rows,"count":rows.len()}))
    }
    fn patch_agent(&self, agent: &str, patch: &Value) -> Result<(), String> {
        self.agent_config(agent)?;
        config::update_config_for_agent(&self.deps.memory, Some(agent), patch)?;
        let config = self.agent_config(agent)?;
        self.deps.events.publish("agent-updated",json!({"agentId":agent,"agentName":config.pointer("/agent/name").cloned().unwrap_or(Value::Null),"yuan":config.pointer("/agent/yuan").cloned().unwrap_or(Value::Null)}));
        Ok(())
    }
    async fn apply(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        agent: &str,
        key: &str,
        value: &Value,
    ) -> Result<Value, String> {
        if crate::mcp_settings::ACTIONS
            .iter()
            .any(|(action, _, _)| *action == key)
        {
            let _authority = self.deps.media.authority_mutation_lease().await;
            self.agent_config(agent)?;
            return self
                .deps
                .mcp
                .request_tool(&self.deps.auth, ctx, call, key, value)
                .await;
        }
        if key.trim() != key || forbidden_key(key) {
            return Err("安全边界、沙箱、供应商凭证及MCP设置只能由用户在设置页修改".into());
        }
        let definition = SETTINGS
            .iter()
            .find(|s| s.key == key)
            .ok_or("未知或不可由模型修改的设置")?;
        let _authority = self.deps.media.authority_mutation_lease().await;
        let principal = self
            .deps
            .auth
            .mint_local_settings_tool_principal(ctx, call)?;
        self.agent_config(agent)?;
        let before = self.current(key, agent, ctx)?;
        match key {
            "locale" | "timezone" | "user.name" => {
                let next = text(value)?;
                if key == "locale" && !definition.options.contains(&next.as_str()) {
                    return Err("不支持的语言".into());
                }
                if next.len() > 4096 || next.contains('\0') {
                    return Err("设置文本超出范围".into());
                }
                apply_global(
                    &self.deps.memory,
                    &self.deps.events,
                    &principal,
                    &nested_patch(key, json!(next)),
                )?;
            }
            "file_backup" => {
                let raw = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                if !definition.options.contains(&raw.as_str()) {
                    return Err("备份保留天数必须为1、3或7".into());
                }
                let days = raw.parse::<u64>().map_err(|_| "无效保留天数")?;
                apply_global(
                    &self.deps.memory,
                    &self.deps.events,
                    &principal,
                    &json!({"file_backup":{"retention_days":days}}),
                )?;
            }
            "computer_use.enabled" => {
                crate::computer_use_settings::tool_set_enabled(
                    &self.deps.memory,
                    &self.deps.events,
                    &self.deps.auth,
                    ctx,
                    call,
                    boolean(value)?,
                )?;
            }
            "bridge_media_public_base_url" => {
                let next = crate::bridgemedia::BridgeMediaService::validate_base_url(&text(value)?)
                    .map_err(|e| e.to_string())?;
                {
                    let _guard = crate::agents_admin::preferences_locked();
                    let mut prefs = self.deps.memory.read_preferences_root()?;
                    if prefs.get("bridge").is_some_and(|v| !v.is_object()) {
                        return Err("bridge偏好损坏".into());
                    }
                    if prefs.get("bridge").is_none() {
                        prefs["bridge"] = json!({});
                    }
                    if next.is_empty() {
                        prefs["bridge"]
                            .as_object_mut()
                            .expect("已确认对象")
                            .remove("mediaPublicBaseUrl");
                    } else {
                        prefs["bridge"]["mediaPublicBaseUrl"] = json!(next);
                    }
                    self.deps.memory.write_preferences_root(&prefs)?;
                }
                let active = if next.is_empty() {
                    std::env::var("LINGXI_BRIDGE_PUBLIC_BASE_URL").unwrap_or_default()
                } else {
                    next
                };
                self.deps
                    .bridge_media
                    .set_base_url(&active)
                    .map_err(|e| format!("设置已保存但运行发布失败：{e}"))?;
            }
            "theme" => {
                let next = text(value)?;
                if !themes()?.contains(&next) {
                    return Err("未知主题".into());
                }
                {
                    let _guard = crate::agents_admin::preferences_locked();
                    let mut prefs = self.deps.memory.read_preferences_root()?;
                    if prefs.get("appearance").is_some_and(|v| !v.is_object()) {
                        return Err("外观偏好损坏".into());
                    }
                    if prefs.get("appearance").is_none() {
                        prefs["appearance"] = json!({});
                    }
                    prefs["appearance"]["theme"] = json!(next);
                    self.deps.memory.write_preferences_root(&prefs)?;
                }
                self.deps
                    .events
                    .publish("theme-changed", json!({"theme":next}));
            }
            "memory.enabled" | "experience.enabled" => {
                let enabled = boolean(value)?;
                let mut flags = self
                    .deps
                    .memory
                    .read_flags(agent)
                    .map_err(|e| e.to_string())?;
                if key == "memory.enabled" {
                    flags.memory_enabled = Some(enabled);
                } else {
                    flags.experience_enabled = Some(enabled);
                }
                self.deps
                    .memory
                    .patch_flags_with_toggle(
                        agent,
                        flags,
                        (key == "memory.enabled").then_some(enabled),
                    )
                    .map_err(|e| e.to_string())?;
                if key == "memory.enabled" {
                    self.deps.events.publish(
                        "memory-master-changed",
                        json!({"agentId":agent,"enabled":enabled}),
                    );
                }
                let config = self.agent_config(agent)?;
                self.deps.events.publish("agent-updated",json!({"agentId":agent,"agentName":config.pointer("/agent/name").cloned().unwrap_or(Value::Null),"yuan":config.pointer("/agent/yuan").cloned().unwrap_or(Value::Null)}));
            }
            "memory.facts" => {
                let body = text(value)?;
                if body.len() > 2 * 1024 * 1024 {
                    return Err("记忆正文过大".into());
                }
                let store = self
                    .deps
                    .memory
                    .compiled_store(agent)
                    .map_err(|e| e.to_string())?;
                store
                    .migrate_legacy_editable_facts()
                    .map_err(|e| e.to_string())?;
                store.write_facts(&body).map_err(|e| e.to_string())?;
                self.patch_agent(agent, &json!({}))?;
            }
            "agent.name" => {
                let name = text(value)?;
                if name.trim().is_empty() || name.len() > 1024 {
                    return Err("助手名称为空或过长".into());
                }
                self.patch_agent(agent, &json!({"agent":{"name":name}}))?;
            }
            "home_folder" => {
                let folder = text(value)?;
                let folder = if folder.trim().is_empty() {
                    Value::Null
                } else {
                    let p = std::path::PathBuf::from(folder.trim());
                    if !p.is_absolute() || !p.is_dir() {
                        return Err("工作目录必须是实际存在的绝对目录".into());
                    }
                    json!(p.to_string_lossy())
                };
                self.patch_agent(agent, &json!({"desk":{"home_folder":folder}}))?;
                self.deps.events.publish("agent-workspace-changed",json!({"agentId":agent,"homeFolder":self.agent_config(agent)?.pointer("/desk/home_folder").cloned().unwrap_or(Value::Null)}));
            }
            "models.chat" => {
                let raw = text(value)?;
                let (provider, model) = raw
                    .split_once('/')
                    .filter(|(p, m)| !p.is_empty() && !m.is_empty())
                    .ok_or("models.chat需要provider/id")?;
                self.deps
                    .models
                    .as_ref()
                    .ok_or("模型面未配置")?
                    .resolve_dispatch(&ModelRouteRequest {
                        operation: ModelOperation::Chat,
                        provider: Some(provider.into()),
                        model: Some(model.into()),
                    })
                    .map_err(|e| e.to_string())?;
                self.patch_agent(
                    agent,
                    &json!({"models":{"chat":{"provider":provider,"id":model}}}),
                )?;
                self.deps
                    .events
                    .publish("models-changed", json!({"agentId":agent}));
            }
            "thinking_level" => {
                let level = text(value)?;
                if !definition.options.contains(&level.as_str()) {
                    return Err("未知思考档位".into());
                }
                let mut choice = crate::chat_settings::choice(
                    self.deps.memory.home(),
                    &ctx.principal.storage_subject(),
                    ctx.session_id.as_str(),
                )?;
                let gateway = self.deps.models.as_ref().ok_or("模型面未配置")?;
                let configured = self.agent_config(agent)?;
                let agent_model = configured.pointer("/models/chat");
                let selected = choice.model.clone().or_else(|| {
                    Some((
                        agent_model?.get("provider")?.as_str()?.into(),
                        agent_model?.get("id")?.as_str()?.into(),
                    ))
                });
                let dispatch = gateway
                    .resolve_dispatch(&ModelRouteRequest {
                        operation: ModelOperation::Chat,
                        provider: selected.as_ref().map(|m| m.0.clone()),
                        model: selected.as_ref().map(|m| m.1.clone()),
                    })
                    .map_err(|e| e.to_string())?;
                if level != "off"
                    && dispatch.compat.as_ref().and_then(|c| c.reasoning) != Some(true)
                {
                    return Err("所选模型未声明可调思考能力".into());
                }
                let call = lingxi_adapters::models::compat::CompatCall {
                    hints: dispatch.compat,
                    options: lingxi_adapters::models::compat::CompatOptions {
                        reasoning_level: Some(level.clone()),
                        ..Default::default()
                    },
                };
                lingxi_adapters::models::compat::apply_for_call(
                    json!({"model":dispatch.route.model,"messages":[]}),
                    &dispatch.route,
                    Some(&call),
                )
                .map_err(|e| e.message)?;
                choice.thinking_level = Some(level);
                crate::chat_settings::save_choice(
                    self.deps.memory.home(),
                    &ctx.principal.storage_subject(),
                    Some(ctx.session_id.as_str()),
                    choice,
                )?;
            }
            _ => return Err("该设置尚无真实执行入口".into()),
        }
        let after = self.current(key, agent, ctx)?;
        Ok(
            json!({"settingsUpdate":{"status":"applied","action":"core.apply","key":key,"title":format!("{}已更新",definition.label),"target":{"type":definition.scope,"id":if definition.scope=="agent"{agent}else{definition.scope}},"changes":[{"key":key,"label":definition.label,"before":before,"after":after}]},"settingKey":key,"cardType":definition.kind,"confirmed":true,"currentValue":before,"proposedValue":after,"label":definition.label}),
        )
    }
}

impl ToolExecutorPort for SettingsTools {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let result = async {
                let scope = toolgateway::trusted_settings_context(ctx, call)?;
                if ctx.principal != lingxi_kernel::Principal::LocalUser
                    || !matches!(
                        scope.permission(),
                        InvocationPermissionContext::UserSession { .. }
                    )
                {
                    return Err("设置工具当前仅接受本机交互会话；其它来源缺少原始设置授权".into());
                }
                let args = request.arguments.as_value();
                match args["action"].as_str() {
                    Some("search") => self.search(
                        args["query"].as_str().unwrap_or_default(),
                        scope.agent_id(),
                        ctx,
                    ),
                    Some("apply") => {
                        if matches!(
                            scope.permission(),
                            InvocationPermissionContext::UserSession {
                                mode: SessionPermissionMode::ReadOnly
                            }
                        ) {
                            return Err("只读会话不能修改设置".into());
                        }
                        self.apply(
                            ctx,
                            call,
                            scope.agent_id(),
                            args["key"].as_str().unwrap_or_default(),
                            args.get("value").ok_or("apply需要value")?,
                        )
                        .await
                    }
                    _ => Err("未知设置操作".into()),
                }
            }
            .await;
            let outcome = match result {
                Ok(value) => {
                    let mut result = ToolSuccess::text(value.to_string());
                    result.details = value.as_object().cloned();
                    ToolOutcome::Success { result }
                }
                Err(message) => ToolOutcome::Failed {
                    error: ProtocolError::new(ErrorCode::Forbidden, message, false),
                },
            };
            ToolExecutionResult::of_ctx(ctx, outcome)
        })
    }
}

pub fn register(
    gateway: &ToolInvocationGateway,
    budget: &SchemaBudget,
    deps: SettingsToolDeps,
) -> Result<(), String> {
    let receipt=gateway.registry().register(ToolManifest{
        origin:ToolOrigin::FirstParty,local_name:"update_settings".into(),display_name:"update_settings".into(),aliases:vec![],version:"1".into(),
        description:"修改灵犀设置。先用search查询实际设置和选项，apply修改明确设置。安全、沙箱和MCP秘密只能由用户在设置页修改。".into(),
        input_schema:ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema:json!({"type":"object","properties":{"action":{"type":"string","enum":["search","apply"]},"query":{"type":"string","maxLength":1024},"key":{"type":"string","maxLength":128},"value":{"type":"string","maxLength":2097152}},"required":["action"],"additionalProperties":false})},
        output_schema:None,permission:PermissionContract{kind:PermissionKind::ArgumentAwareSettings,capability_base:"update_settings".into()},availability:Availability::Available,timeout_ms:Some(30000),max_concurrency:None,declared_permission:DeclaredPermission::Execute,recovery:lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    },budget).map_err(|e|format!("{e:?}"))?;
    gateway
        .registry()
        .add_model_resident(&receipt.target_id)
        .map_err(|e| format!("{e:?}"))?;
    let mcp = Arc::downgrade(&deps.mcp);
    gateway.bind_executor_with_review(
        receipt.target_id,
        Arc::new(SettingsTools { deps }),
        Arc::new(move |input, args| {
            let args = args.as_value();
            let key = args["key"].as_str().unwrap_or_default();
            if crate::mcp_settings::ACTIONS
                .iter()
                .any(|(action, _, _)| *action == key)
            {
                mcp.upgrade().ok_or("MCP 管理器不可用")?.review_tool(
                    input.agent_id,
                    key,
                    args.get("value").unwrap_or(&Value::Null),
                )
            } else {
                let label = SETTINGS.iter().find(|setting| setting.key == key).map(|setting| setting.label).unwrap_or("未识别设置");
                Ok(json!({"summary":format!("修改助手 {} 的{}设置",input.agent_id,label),"agentId":input.agent_id,"setting":key,"action":args["action"]}))
            }
        }),
        "真实交互设置：冻结身份、受限全局偏好与目标助手配置",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn principal() -> Principal {
        Principal {
            schema_version: 1,
            principal_id: "test-narrow-settings".into(),
            kind: crate::auth::PrincipalKind::LocalUser,
            user_id: Some("user_local".into()),
            studio_id: None,
            server_node_id: None,
            device_id: None,
            credential_id: None,
            web_session_id: None,
            connection_kind: crate::auth::ConnectionKindSerde::Local,
            credential_kind: crate::auth::CredentialKind::None,
            trust_state: crate::auth::TrustState::Local,
            scopes: vec!["settings.write".into()],
        }
    }
    #[test]
    fn safe_global_apply_preserves_unknown_fields_and_refuses_secret_or_security_patches() {
        let home = std::env::temp_dir().join(format!(
            "lingxi-settings-safe-{}",
            am::random_token_suffix()
        ));
        std::fs::create_dir_all(home.join("user")).unwrap();
        let path = home.join("user/preferences.json");
        std::fs::write(
            &path,
            r#"{"custom":{"preserve":17},"locale":"zh-CN","sandbox":true}"#,
        )
        .unwrap();
        let memory = MemoryService::new(home.clone(), home.join("runtime"));
        let events = AppEventLog::new();
        let principal = principal();
        assert!(!principal.is_local_owner());
        apply_global(&memory, &events, &principal, &json!({"locale":"en"})).unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["custom"]["preserve"], 17);
        assert_eq!(saved["sandbox"], true);
        assert_eq!(saved["locale"], "en");
        assert!(events.since(0)["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "locale-changed" && e["payload"]["locale"] == "en"));
        let before = std::fs::read(&path).unwrap();
        for patch in [
            json!({"sandbox":false}),
            json!({"providers":{"p":{"api_key":"test-secret"}}}),
            json!({"user":{"name":"changed"},"security":{"allow":true}}),
        ] {
            assert!(apply_global(&memory, &events, &principal, &patch).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        std::fs::write(&path, b"{broken").unwrap();
        assert!(apply_global(&memory, &events, &principal, &json!({"locale":"ko"})).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
        std::fs::remove_dir_all(&home).unwrap();
    }
}
