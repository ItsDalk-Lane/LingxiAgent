//! R06 A 块剩余后端叶子：`GET/PUT /agents/{id}/config`（GET 为完整设置读面）、
//! `PUT /agents/{id}/config` 供应商读回链、
//! current_status 只读面、experiments 读/写、cache-snapshot observation 读。
//!
//! 语义源（端口式移植，不猜造）：
//! - `server/routes/agents.ts:636-799` — PUT config 主链（scope 分流 →
//!   providers 目录写 → 内联凭证转存 → onProviderChanged（models.json 投影
//!   + 焦点刷新）→ agentPartial 落盘 → updateConfig 运行时刷新）。
//! - `core/provider-catalog.ts` / `core/local-provider-plugin-store.ts` /
//!   `core/provider-registry.ts`（saveProvider/removeProvider）— 权威目录
//!   `provider-catalog.json`、条件 `provider-plugins/` 插件文件。
//! - `core/model-sync.ts` syncModels — `models.json` 派生投影。
//! - `shared/config-scope.ts` + `core/preferences-manager.ts` — 全局字段经
//!   setter 写 `user/preferences.json`。
//! - `lib/tools/current-status-tool.ts` — list/get 协议的状态键目录。
//! - `server/routes/agents.ts:557-634` — GET config（GC 工作区历史 → 读盘
//!   → experience 归一 → `_raw` → 全局字段注入 → 供应商摘要 → availableTools
//!   → 递归掩码）。端口边界：无运行时 agent/插件注册表，availableTools =
//!   core+standard+可选开关（beautify/office 需插件工具注册，端口无 → 不列；
//!   computer 需 Computer Use 启用，端口无 → 隐藏）；供应商 base_url/api 取目录
//!   与插件定义值，无内置默认 URL 表（不回退 registry 默认）；config.yaml
//!   违反端口配置校验时显式 500（不静默降级）；目录读取失败 → `providers: {}`
//!   并记 warn 日志；读目录不触发遗留迁移落盘。
//! - `server/routes/experiments.ts` + `lib/experiments/registry.ts` +
//!   `lib/memory/cache-snapshot-observation.ts`。
//!
//! 端口边界（显式声明，不冒充完整）：
//! - `models.json` 投影只含结构性决策（哪些 provider 进入、baseUrl/api/
//!   apiKey 字段形状、显式 type/outputs 的 chat 过滤、模型条目显式字段）；
//!   known-models.json / Pi 内置目录元数据、模态兼容层不在本端口
//!   （模型平面属 R05 语义源）。与现役一致：内容不变且权限正确不重写。
//! - 运行时 agent 实例在端口中不存在："已加载" ≡ config.yaml 在磁盘上可
//!   解析（现役启动即全量 config-load）。`updateConfig({agentId})` 在命名
//!   agent 的 config 消失时回退焦点，无焦点显式失败——与现役
//!   `getAgentById(id) || getAgent()` 的回退链同构，但不存在"懒加载未注
//!   册"分支。
//! - `recordSecurityAuditEvent` 无端口侧路由级审计汇（security_audit.rs
//!   尚无 route 事件面），本路由不记录审计事件，不伪造。
//! - 迁移遗留 `added-models.yaml` 时不写 migration-backups 备份目录
//!   （全新 home 无遗留文件；备份面不在本叶子范围）。
//! - 命名时区算术（chrono-tz 不在锁内）沿用 context_compiler 的已知差距：
//!   localDateTime/utcOffset 用系统本地时区计算，timezone 返回偏好里配置
//!   的名字、缺失时为 null（显式未知，不猜造系统 IANA 名）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

// ── secret custody（shared/secret-custody.ts 子集）──────────────────────────

const MASKED_SECRET: &str = "********";

fn is_masked_secret_value(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.trim() == MASKED_SECRET)
}

/// resolveSecretPatch（agents.ts:700-705，secretKeys 固定 ["api_key"]）：
/// 掩码值解析回既有密文，其余键原样。
fn resolve_secret_patch(patch: &Value, existing: &Value) -> Value {
    let empty = Map::new();
    let source = patch.as_object().unwrap_or(&empty);
    let saved = existing.as_object().unwrap_or(&empty);
    let mut out = Map::new();
    for (key, value) in source {
        let resolved = if key == "api_key" && is_masked_secret_value(value) {
            saved
                .get(key)
                .cloned()
                .unwrap_or(Value::String(String::new()))
        } else {
            value.clone()
        };
        out.insert(key.clone(), resolved);
    }
    Value::Object(out)
}

/// writeSecretFileSync（shared/secret-fs.ts）：tmp（0600）→ rename。权限
/// 收紧失败记录并继续（存储用户数据是主职责），Windows 无 POSIX 位。
fn write_secret_file(path: &Path, text: &str) -> std::io::Result<()> {
    let _publication = am::ContextMaterialMutation::for_path(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let uniq = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let tmp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string()),
        uniq
    ));
    std::fs::write(&tmp, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(err) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)) {
            tracing::warn!(%err, path = %tmp.display(), "cannot tighten secret file mode; continuing");
        }
    }
    std::fs::rename(&tmp, path)
}

#[cfg(unix)]
fn secret_file_mode_ok(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o777 == 0o600)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn secret_file_mode_ok(_path: &Path) -> bool {
    true
}

// ── provider catalog（core/provider-catalog.ts）────────────────────────────

const PROVIDER_CATALOG_VERSION: u64 = 2;

fn catalog_path(home: &Path) -> PathBuf {
    home.join("provider-catalog.json")
}

fn legacy_added_models_path(home: &Path) -> PathBuf {
    home.join("added-models.yaml")
}

fn normalize_deleted_providers(value: Option<&Value>) -> Vec<String> {
    let Some(list) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for item in list {
        let Some(id) = item.as_str().map(str::trim).filter(|id| !id.is_empty()) else {
            continue;
        };
        if seen.insert(id.to_string()) {
            out.push(id.to_string());
        }
    }
    out
}

fn normalize_provider_map(value: Option<&Value>) -> Map<String, Value> {
    let mut providers = Map::new();
    let Some(map) = value.and_then(Value::as_object) else {
        return providers;
    };
    for (provider_id, config) in map {
        let id = provider_id.trim();
        if id.is_empty() {
            continue;
        }
        providers.insert(
            id.to_string(),
            if config.is_object() {
                config.clone()
            } else {
                json!({ "_config_error": "malformed_provider_config" })
            },
        );
    }
    providers
}

struct ProviderCatalog {
    providers: Map<String, Value>,
    meta: Map<String, Value>,
}

impl ProviderCatalog {
    fn deleted_providers(&self) -> Vec<String> {
        normalize_deleted_providers(self.meta.get("deletedProviders"))
    }

    /// normalizeProviderCatalog + save：固定 v2 形状落盘（0600）。
    fn save(&self, home: &Path) -> Result<(), String> {
        let mut meta = self.meta.clone();
        let deleted = normalize_deleted_providers(meta.get("deletedProviders"));
        if deleted.is_empty() {
            meta.remove("deletedProviders");
        } else {
            meta.insert("deletedProviders".into(), json!(deleted));
        }
        let document = json!({
            "catalogVersion": PROVIDER_CATALOG_VERSION,
            "providers": Value::Object(self.providers.clone()),
            "capabilities": { "search": { "providers": [] } },
            "meta": Value::Object(meta),
        });
        let text = format!(
            "{}\n",
            serde_json::to_string_pretty(&document)
                .map_err(|err| format!("cannot encode provider catalog: {err}"))?
        );
        write_secret_file(&catalog_path(home), &text)
            .map_err(|err| format!("cannot save provider catalog: {err}"))
    }
}

/// load（provider-catalog.ts:117-121）：读现役目录；缺失走遗留迁移
/// （added-models.yaml → catalog；无遗留则物化空目录——与现役 load 的
/// 副作用一致：首次读取即建立 provider-catalog.json）。
fn load_catalog(home: &Path) -> Result<ProviderCatalog, String> {
    load_catalog_with(home, true)
}

/// `persist=false` 时遗留目录迁移只在内存中完成、不落盘（只读路由用，
/// 读取不产生写副作用）。
fn load_catalog_with(home: &Path, persist: bool) -> Result<ProviderCatalog, String> {
    let read = std::fs::read(catalog_path(home));
    match read {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            let parsed: Value = serde_json::from_str(text)
                .map_err(|err| format!("cannot parse provider catalog: {err}"))?;
            if parsed["catalogVersion"].as_u64() != Some(PROVIDER_CATALOG_VERSION) {
                return Err(format!(
                    "Unsupported provider catalog version: {}",
                    parsed["catalogVersion"]
                ));
            }
            let meta = parsed
                .get("meta")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            Ok(ProviderCatalog {
                providers: normalize_provider_map(parsed.get("providers")),
                meta,
            })
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // _migrateLegacyCatalog：遗留 added-models.yaml（缺失 → 空）。
            let legacy = am::config::read_yaml_mapping_as_json(&legacy_added_models_path(home))
                .map_err(|err| format!("cannot read legacy added-models.yaml: {err}"))?;
            let now = am::now_iso();
            let mut meta = Map::new();
            meta.insert("migratedAt".into(), json!(now));
            meta.insert("migrationSource".into(), json!("added-models.yaml"));
            let deleted = normalize_deleted_providers(
                legacy.as_ref().and_then(|v| v.get("_deleted_providers")),
            );
            if !deleted.is_empty() {
                meta.insert("deletedProviders".into(), json!(deleted));
            }
            let catalog = ProviderCatalog {
                providers: normalize_provider_map(legacy.as_ref().and_then(|v| v.get("providers"))),
                meta,
            };
            if persist {
                catalog.save(home)?;
            }
            Ok(catalog)
        }
        Err(err) => Err(format!("cannot read provider catalog: {err}")),
    }
}

// ── local provider plugin store（core/local-provider-plugin-store.ts）──────

const PROVIDER_DEFINITION_KEYS: [&str; 14] = [
    "id",
    "display_name",
    "displayName",
    "auth_type",
    "authType",
    "base_url",
    "baseUrl",
    "defaultBaseUrl",
    "api",
    "defaultApi",
    "models",
    "runtime",
    "capabilities",
    "description",
];

fn is_safe_plugin_id(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && id != "."
        && id != ".."
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains('\0')
        && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn is_safe_plugin_provider_id(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && id != "."
        && id != ".."
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains('\0')
}

fn plugin_storage_id(provider_id: &str) -> Option<String> {
    let id = provider_id.trim();
    if !is_safe_plugin_provider_id(id) {
        return None;
    }
    if is_safe_plugin_id(id) {
        return Some(id.to_string());
    }
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(id.as_bytes());
    Some(format!(
        "provider-{}",
        digest
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
}

fn plugin_root(home: &Path) -> PathBuf {
    home.join("provider-plugins")
}

fn plugin_provider_dir(home: &Path, provider_id: &str) -> Option<PathBuf> {
    plugin_storage_id(provider_id).map(|storage| plugin_root(home).join(storage))
}

fn plugin_provider_json(home: &Path, provider_id: &str) -> Option<PathBuf> {
    let storage = plugin_storage_id(provider_id)?;
    Some(
        plugin_root(home)
            .join(&storage)
            .join("providers")
            .join(format!("{storage}.json")),
    )
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn normalize_auth_type(value: &Value) -> &'static str {
    match value.as_str() {
        Some("api-key") => "api-key",
        Some("oauth") => "oauth",
        Some("none") => "none",
        Some("optional") => "optional",
        _ => "api-key",
    }
}

fn normalize_models_list(value: Option<&Value>) -> Vec<Value> {
    let Some(list) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut models = Vec::new();
    for model in list {
        if let Some(id) = model.as_str().map(str::trim).filter(|id| !id.is_empty()) {
            models.push(Value::String(id.to_string()));
        } else if let Some(id) = model
            .as_object()
            .and_then(|entry| entry.get("id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            let mut entry = model.clone();
            entry
                .as_object_mut()
                .expect("已判对象")
                .insert("id".into(), Value::String(id.to_string()));
            models.push(entry);
        }
    }
    models
}

fn model_id_of(model: &Value) -> String {
    model
        .as_object()
        .and_then(|entry| entry.get("id"))
        .and_then(Value::as_str)
        .or_else(|| model.as_str())
        .unwrap_or("")
        .to_string()
}

/// mergeProviderModelEntries：按 id 合并两份模型列表（overlay 优先）。
fn merge_provider_model_entries(base: Option<&Value>, overlay: Option<&Value>) -> Vec<Value> {
    let base = normalize_models_list(base);
    let overlay = normalize_models_list(overlay);
    if base.is_empty() {
        return overlay;
    }
    if overlay.is_empty() {
        return base;
    }
    let mut by_id: BTreeMap<String, Value> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for model in base {
        let id = model_id_of(&model).trim().to_string();
        if id.is_empty() {
            continue;
        }
        if !by_id.contains_key(&id) {
            order.push(id.clone());
        }
        by_id.insert(id, model);
    }
    for model in overlay {
        let id = model_id_of(&model).trim().to_string();
        if id.is_empty() {
            continue;
        }
        let merged = merge_model_entries(by_id.get(&id), &model);
        if !by_id.contains_key(&id) {
            order.push(id.clone());
        }
        by_id.insert(id, merged);
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

fn merge_model_entries(base: Option<&Value>, overlay: &Value) -> Value {
    let base_obj = base.filter(|v| v.is_object());
    let overlay_obj = overlay.as_object();
    let id = model_id_of(overlay).trim().to_string();
    let id = if id.is_empty() {
        base.map(model_id_of).unwrap_or_default().trim().to_string()
    } else {
        id
    };
    if id.is_empty() {
        return Value::Null;
    }
    match (base_obj, overlay_obj) {
        (None, None) => Value::String(id),
        _ => {
            let mut merged = base_obj.cloned().unwrap_or_else(|| json!({ "id": id }));
            if let (Some(target), Some(source)) = (merged.as_object_mut(), overlay_obj) {
                for (key, value) in source {
                    target.insert(key.clone(), value.clone());
                }
            }
            merged
                .as_object_mut()
                .expect("已判对象")
                .insert("id".into(), Value::String(id));
            merged
        }
    }
}

/// replaceProviderModelEntries：next 为空 → []；base 为空 → next；否则按
/// id 用 base 条目补全 next 的对象形状。
fn replace_provider_model_entries(base: Option<&Value>, next: Option<&Value>) -> Vec<Value> {
    let base = normalize_models_list(base);
    let next = normalize_models_list(next);
    if base.is_empty() {
        return next;
    }
    if next.is_empty() {
        return Vec::new();
    }
    let base_by_id: BTreeMap<String, Value> = base
        .into_iter()
        .map(|model| (model_id_of(&model).trim().to_string(), model))
        .collect();
    next.into_iter()
        .map(|model| {
            let id = model_id_of(&model);
            let id = id.trim();
            let merged = merge_model_entries(base_by_id.get(id), &model);
            if merged.is_null() {
                model
            } else {
                merged
            }
        })
        .collect()
}

/// normalizeLocalProviderPlugin：定义字段规范化为插件形状。
fn normalize_local_provider_plugin(provider_id: &str, config: &Value) -> Value {
    let id = provider_id.trim().to_string();
    let models = normalize_models_list(config.get("models"));
    let mut plugin = Map::new();
    plugin.insert("id".into(), json!(id));
    let display_name = non_empty_string(config.get("displayName"))
        .or_else(|| non_empty_string(config.get("display_name")))
        .unwrap_or_else(|| id.clone());
    plugin.insert("displayName".into(), json!(display_name));
    plugin.insert(
        "authType".into(),
        json!(normalize_auth_type(
            config
                .get("authType")
                .or_else(|| config.get("auth_type"))
                .unwrap_or(&Value::Null)
        )),
    );
    let default_base_url = non_empty_string(config.get("defaultBaseUrl"))
        .or_else(|| non_empty_string(config.get("baseUrl")))
        .or_else(|| non_empty_string(config.get("base_url")))
        .unwrap_or_default();
    plugin.insert("defaultBaseUrl".into(), json!(default_base_url));
    let default_api = non_empty_string(config.get("defaultApi"))
        .or_else(|| non_empty_string(config.get("api")))
        .unwrap_or_else(|| "openai-completions".to_string());
    plugin.insert("defaultApi".into(), json!(default_api));
    if !models.is_empty() {
        plugin.insert("models".into(), Value::Array(models));
    }
    if let Some(runtime) = config.get("runtime").filter(|v| v.is_object()) {
        plugin.insert("runtime".into(), runtime.clone());
    }
    let mut capabilities = config.get("capabilities").cloned().unwrap_or(json!({}));
    if let Some(media_obj) = config
        .get("media")
        .filter(|v| v.is_object())
        .and_then(Value::as_object)
    {
        let media_entry = capabilities
            .as_object_mut()
            .expect("capabilities 已确保对象")
            .entry("media")
            .or_insert_with(|| json!({}));
        if let Some(target_media) = media_entry.as_object_mut() {
            for (key, value) in media_obj {
                target_media.insert(key.clone(), value.clone());
            }
        }
    }
    if capabilities.as_object().is_some_and(|m| !m.is_empty()) {
        plugin.insert("capabilities".into(), capabilities);
    }
    for key in ["description", "docsUrl", "homepage", "icon"] {
        if let Some(text) = non_empty_string(config.get(key)) {
            plugin.insert(key.into(), json!(text));
        }
    }
    Value::Object(plugin)
}

/// providerPluginToCatalogDefinition：插件 → 目录定义字段（snake_case）。
fn plugin_catalog_definition(plugin: &Value) -> Value {
    let mut definition = Map::new();
    definition.insert(
        "display_name".into(),
        json!(plugin
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or(plugin.get("id").and_then(Value::as_str).unwrap_or(""))),
    );
    definition.insert(
        "auth_type".into(),
        json!(normalize_auth_type(
            plugin.get("authType").unwrap_or(&Value::Null)
        )),
    );
    definition.insert(
        "base_url".into(),
        json!(plugin
            .get("defaultBaseUrl")
            .and_then(Value::as_str)
            .unwrap_or("")),
    );
    definition.insert(
        "api".into(),
        json!(plugin
            .get("defaultApi")
            .and_then(Value::as_str)
            .unwrap_or("openai-completions")),
    );
    for key in ["models", "runtime", "capabilities"] {
        if let Some(value) = plugin.get(key) {
            definition.insert(key.into(), value.clone());
        }
    }
    Value::Object(definition)
}

/// splitLocalProviderConfig：定义字段进插件文件，其余 overlay 进目录。
fn split_local_provider_config(
    provider_id: &str,
    config: &Value,
    existing_plugin: Option<&Value>,
) -> (Value, Value) {
    let existing_definition = existing_plugin
        .map(plugin_catalog_definition)
        .unwrap_or_else(|| json!({}));
    let raw = config.as_object().cloned().unwrap_or_default();
    let mut definition_fields = Map::new();
    for (key, value) in &raw {
        if PROVIDER_DEFINITION_KEYS.contains(&key.as_str()) {
            definition_fields.insert(key.clone(), value.clone());
        }
    }
    if definition_fields.contains_key("models") {
        definition_fields.insert(
            "models".into(),
            Value::Array(replace_provider_model_entries(
                existing_definition.get("models"),
                definition_fields.get("models"),
            )),
        );
    }
    let mut merged = existing_definition.as_object().cloned().unwrap_or_default();
    for (key, value) in definition_fields {
        merged.insert(key, value);
    }
    let plugin = normalize_local_provider_plugin(provider_id, &Value::Object(merged));
    let mut overlay = Map::new();
    for (key, value) in raw {
        if PROVIDER_DEFINITION_KEYS.contains(&key.as_str()) {
            continue;
        }
        overlay.insert(key, value);
    }
    (plugin, Value::Object(overlay))
}

fn write_local_provider_plugin(
    home: &Path,
    provider_id: &str,
    config: &Value,
) -> Result<Value, String> {
    let storage = plugin_storage_id(provider_id)
        .ok_or_else(|| format!("Invalid local provider plugin id: {provider_id}"))?;
    let plugin = normalize_local_provider_plugin(provider_id, config);
    let dir = plugin_root(home).join(&storage);
    let manifest = json!({
        "id": storage,
        "type": "provider-plugin",
        "schemaVersion": 1,
        "provider": provider_id.trim(),
    });
    let provider_path = dir.join("providers").join(format!("{storage}.json"));
    write_secret_file(
        &dir.join("manifest.json"),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?
        ),
    )
    .map_err(|err| format!("cannot write plugin manifest: {err}"))?;
    write_secret_file(
        &provider_path,
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&plugin).map_err(|err| err.to_string())?
        ),
    )
    .map_err(|err| format!("cannot write plugin provider file: {err}"))?;
    let mut with_source = plugin.as_object().cloned().unwrap_or_default();
    with_source.insert("source".into(), json!({ "kind": "local-provider-plugin" }));
    Ok(Value::Object(with_source))
}

fn remove_local_provider_plugin(home: &Path, provider_id: &str) {
    if !is_safe_plugin_provider_id(provider_id) {
        return;
    }
    let Some(dir) = plugin_provider_dir(home, provider_id) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    let legacy = plugin_root(home).join(provider_id.trim());
    if legacy != dir {
        let _ = std::fs::remove_dir_all(&legacy);
    }
}

fn has_local_plugin(home: &Path, provider_id: &str) -> bool {
    plugin_provider_json(home, provider_id).is_some_and(|path| path.is_file())
}

// ── provider registry（core/provider-registry.ts saveProvider/remove）──────

/// 现役 BUILTIN_PLUGINS 的 id 集（lib/providers/*.ts 逐文件核对）。
const BUILTIN_PROVIDER_IDS: [&str; 39] = [
    "agnes",
    "dashscope",
    "openai",
    "anthropic",
    "deepseek",
    "deepseek-responses",
    "gemini",
    "openrouter",
    "opencode",
    "opencode-go",
    "ollama",
    "minimax",
    "minimax-token-plan",
    "openai-codex-oauth",
    "siliconflow",
    "zhipu",
    "moonshot",
    "baichuan",
    "stepfun",
    "volcengine",
    "volcengine-speech",
    "hunyuan",
    "baidu-cloud",
    "modelscope",
    "infini",
    "mimo",
    "mimo-token-plan",
    "system-speech",
    "groq",
    "together",
    "fireworks",
    "mistral",
    "perplexity",
    "xai",
    "xai-oauth",
    "dashscope-coding",
    "kimi-coding",
    "volcengine-coding",
    "zhipu-coding",
];

/// saveProvider（provider-registry.ts:2160-2192）：合并既有 → 内置不写插件
/// 文件，本地自定义/未注册插件把定义字段写进 provider-plugins 并只在目录
/// 留 overlay；从 deletedProviders 摘除后整目录落盘。
fn registry_save_provider(home: &Path, provider_id: &str, data: &Value) -> Result<(), String> {
    let mut catalog = load_catalog(home)?;
    let existing = catalog
        .providers
        .get(provider_id)
        .cloned()
        .unwrap_or_else(|| json!({}));
    // seed_default_models 已废弃：payload 携带也剥离，不持久化。
    let incoming = data.as_object().cloned().unwrap_or_default();
    let mut next_provider = existing.as_object().cloned().unwrap_or_default();
    for (key, value) in incoming {
        if key != "seed_default_models" {
            next_provider.insert(key, value);
        }
    }
    let next_provider = Value::Object(next_provider);

    let is_builtin = BUILTIN_PROVIDER_IDS.contains(&provider_id);
    let has_local = has_local_plugin(home, provider_id);
    // 端口内不存在外部注册插件：非内置即"本地或未注册"，一律本地插件。
    let stored = if !is_builtin {
        let existing_plugin = if has_local {
            let storage = plugin_storage_id(provider_id)
                .ok_or_else(|| format!("Invalid local provider plugin id: {provider_id}"))?;
            am::read_json_file(
                &plugin_root(home)
                    .join(&storage)
                    .join("providers")
                    .join(format!("{storage}.json")),
            )
        } else {
            None
        };
        let (plugin, overlay) =
            split_local_provider_config(provider_id, &next_provider, existing_plugin.as_ref());
        // _writeLocalProviderPlugin 在写文件前校验插件模型表。
        validate_provider_models(
            provider_id,
            plugin
                .get("defaultBaseUrl")
                .and_then(Value::as_str)
                .unwrap_or(""),
            plugin.get("models"),
        )?;
        write_local_provider_plugin(home, provider_id, &next_provider)?;
        overlay
    } else {
        // 内置插件：runtime 校验与 OAuth base_url 白名单不在此端口
        // （模型平面 R05 拥有运行时校验）；模型元数据校验仍然执行。
        validate_provider_models(
            provider_id,
            next_provider
                .get("base_url")
                .and_then(Value::as_str)
                .unwrap_or(""),
            next_provider.get("models"),
        )?;
        next_provider
    };

    let deleted: Vec<String> = catalog
        .deleted_providers()
        .into_iter()
        .filter(|id| id != provider_id)
        .collect();
    catalog
        .meta
        .insert("deletedProviders".into(), json!(deleted));
    catalog.providers.insert(provider_id.to_string(), stored);
    catalog.save(home)
}

/// remove（provider-registry.ts:1668-1691）：目录条目删除；本地插件连文件
/// 删除；deletedProviders 追加；内置声明保留（内置集在代码里，不在目录）。
/// 目录与插件都不存在时提前返回，不改目录文件。
fn registry_remove_provider(home: &Path, provider_id: &str) -> Result<(), String> {
    let mut catalog = load_catalog(home)?;
    let has_catalog_entry = catalog.providers.contains_key(provider_id);
    let has_local = has_local_plugin(home, provider_id);
    if !has_catalog_entry && !has_local {
        return Ok(());
    }
    if has_catalog_entry {
        catalog.providers.remove(provider_id);
    }
    if has_local {
        remove_local_provider_plugin(home, provider_id);
    }
    let mut deleted = catalog.deleted_providers();
    if !deleted.iter().any(|id| id == provider_id) {
        deleted.push(provider_id.to_string());
    }
    catalog
        .meta
        .insert("deletedProviders".into(), json!(deleted));
    catalog.save(home)
}

/// getAllProvidersRaw 的读侧合并（provider-registry.ts reload +
/// _mergeRawProviderConfig）：本地插件的定义字段（display_name/base_url/
/// api/models…）与目录 overlay 合并，模型表按 id merge。内置条目即目录值。
fn merged_catalog_providers(home: &Path) -> Result<Map<String, Value>, String> {
    merged_catalog_providers_with(home, true)
}

fn merged_catalog_providers_with(home: &Path, persist: bool) -> Result<Map<String, Value>, String> {
    let catalog = load_catalog_with(home, persist)?;
    let mut merged = Map::new();
    for (name, overlay) in &catalog.providers {
        if !has_local_plugin(home, name) {
            merged.insert(name.clone(), overlay.clone());
            continue;
        }
        let storage = plugin_storage_id(name)
            .ok_or_else(|| format!("Invalid local provider plugin id: {name}"))?;
        let plugin = am::read_json_file(
            &plugin_root(home)
                .join(&storage)
                .join("providers")
                .join(format!("{storage}.json")),
        )
        .unwrap_or_else(|| json!({}));
        let definition = plugin_catalog_definition(&plugin);
        let mut combined = definition.as_object().cloned().unwrap_or_default();
        for (key, value) in overlay.as_object().cloned().unwrap_or_default() {
            combined.insert(key, value);
        }
        if definition.get("models").is_some() || overlay.get("models").is_some() {
            combined.insert(
                "models".into(),
                Value::Array(merge_provider_model_entries(
                    definition.get("models"),
                    overlay.get("models"),
                )),
            );
        }
        merged.insert(name.clone(), Value::Object(combined));
    }
    Ok(merged)
}

// ── 模型元数据校验（shared/provider-model-validation.ts 核心子集）──────────

const MODEL_CREDENTIAL_FIELDS: [&str; 17] = [
    "api_key",
    "apikey",
    "token",
    "access",
    "accesstoken",
    "access_token",
    "refresh",
    "refresh_token",
    "refreshtoken",
    "accountid",
    "account_id",
    "authorization",
    "cookie",
    "headers",
    "resourceurl",
    "resource_url",
    "expires",
];

fn model_metadata_error(provider_id: &str, model_id: &str, field: &str, detail: &str) -> String {
    format!("Invalid {field} for model \"{provider_id}/{model_id}\": {detail}")
}

fn is_valid_modality_list(value: &Value) -> bool {
    let Some(list) = value.as_array() else {
        return false;
    };
    !list.is_empty()
        && list
            .iter()
            .all(|item| matches!(item.as_str(), Some("text" | "image" | "video" | "audio")))
}

fn validate_model_metadata(provider_id: &str, model: &Value) -> Result<(), String> {
    let Some(entry) = model.as_object() else {
        return Ok(());
    };
    let raw_id = entry
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let model_id = raw_id.unwrap_or("unknown").to_string();
    for field in entry.keys() {
        if MODEL_CREDENTIAL_FIELDS.contains(&field.to_lowercase().as_str()) {
            return Err(model_metadata_error(
                provider_id,
                &model_id,
                field,
                "credentials belong to the provider or AuthStorage, not a model entry",
            ));
        }
    }
    if let Some(api) = entry.get("api") {
        if !non_empty_string(Some(api)).is_some() {
            return Err(model_metadata_error(
                provider_id,
                &model_id,
                "api",
                "expected a non-empty string",
            ));
        }
    }
    for field in [
        "context",
        "contextWindow",
        "maxOutput",
        "maxTokens",
        "maxOutputTokens",
        "dimensions",
    ] {
        if let Some(value) = entry.get(field) {
            let ok = value.as_f64().is_some_and(|n| n.is_finite() && n > 0.0);
            if !ok {
                return Err(model_metadata_error(
                    provider_id,
                    &model_id,
                    field,
                    "expected a positive finite number",
                ));
            }
        }
    }
    for field in ["web", "structuredOutput"] {
        if let Some(value) = entry.get(field) {
            if !value.is_boolean() {
                return Err(model_metadata_error(
                    provider_id,
                    &model_id,
                    field,
                    "expected a boolean",
                ));
            }
        }
    }
    if let Some(value) = entry.get("outputIncludesThinking") {
        if !value.is_boolean() && !value.is_null() {
            return Err(model_metadata_error(
                provider_id,
                &model_id,
                "outputIncludesThinking",
                "expected a boolean or null",
            ));
        }
    }
    for field in ["inputs", "outputs"] {
        if let Some(value) = entry.get(field) {
            if !is_valid_modality_list(value) {
                return Err(model_metadata_error(
                    provider_id,
                    &model_id,
                    field,
                    "expected a non-empty modality array with only known values (text|image|video|audio)",
                ));
            }
        }
    }
    Ok(())
}

/// validateProviderModels（保存路径）：逐条元数据校验 + 官方 DeepSeek 保留
/// 模型 id 拒绝（"deepseek" 不是模型 id；官方判定含 base_url 主机）。
fn validate_provider_models(
    provider_id: &str,
    base_url: &str,
    models: Option<&Value>,
) -> Result<(), String> {
    let Some(list) = models.and_then(Value::as_array) else {
        return Ok(());
    };
    for model in list {
        validate_model_metadata(provider_id, model)?;
    }
    let is_official_deepseek = provider_id.eq_ignore_ascii_case("deepseek")
        || hostname_of(base_url).as_deref() == Some("api.deepseek.com");
    for model in list {
        if is_official_deepseek && model_id_of(model).trim().eq_ignore_ascii_case("deepseek") {
            let id = model_id_of(model).trim().to_string();
            return Err(format!(
                "Invalid model id \"{id}\" for provider \"{provider_id}\": \"{id}\" is a provider id, not a model id. Use a concrete model id such as deepseek-v4-pro or deepseek-v4-flash."
            ));
        }
    }
    Ok(())
}

// ── URL 字符串手术（lib/llm/provider-client.ts 的 base_url 规范）───────────

fn split_url(raw: &str) -> Option<(String, String)> {
    let (scheme, rest) = raw.split_once("://")?;
    if scheme.is_empty() || rest.is_empty() {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    if authority.is_empty() {
        return None;
    }
    let path = rest[end..]
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_string();
    Some((format!("{scheme}://{authority}"), path))
}

fn hostname_of(raw: &str) -> Option<String> {
    let (_, rest) = raw.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        return None;
    }
    Some(
        host.trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase(),
    )
}

fn strip_terminal_provider_path(raw: &str, suffixes: &[&str]) -> String {
    match split_url(raw.trim()) {
        Some((origin, path)) => {
            let mut path = path.trim_end_matches('/').to_string();
            for suffix in suffixes {
                if path
                    .to_ascii_lowercase()
                    .ends_with(&suffix.to_ascii_lowercase())
                {
                    path = path[..path.len() - suffix.len()]
                        .trim_end_matches('/')
                        .to_string();
                    break;
                }
            }
            if path.is_empty() {
                origin
            } else {
                format!("{origin}{path}")
            }
        }
        None => {
            let mut normalized = raw.trim().trim_end_matches('/').to_string();
            for suffix in suffixes {
                if normalized
                    .to_ascii_lowercase()
                    .ends_with(&suffix.to_ascii_lowercase())
                {
                    let cut = normalized.len().saturating_sub(suffix.len());
                    normalized = normalized[..cut].trim_end_matches('/').to_string();
                    break;
                }
            }
            normalized
        }
    }
}

fn normalize_exact_endpoint_for_sdk(base: &str, api: &str) -> String {
    if api == "anthropic-messages" {
        return strip_terminal_provider_path(base, &["/v1/messages", "/v1"]);
    }
    if api == "openai-completions" {
        return strip_terminal_provider_path(base, &["/chat/completions"]);
    }
    if api == "openai-responses" || api == "openai-codex-responses" {
        return strip_terminal_provider_path(base, &["/responses"]);
    }
    base.trim().trim_end_matches('/').to_string()
}

fn is_official_kimi_coding_base_url(base: &str) -> bool {
    match (hostname_of(base), split_url(base.trim())) {
        (Some(host), Some((_, path))) => {
            host == "api.kimi.com"
                && matches!(path.trim_end_matches('/'), "" | "/coding" | "/coding/v1")
        }
        _ => base.trim().trim_end_matches('/') == "https://api.kimi.com/coding",
    }
}

/// normalizeProviderBaseUrlForApi 的端口（opencode / kimi-coding / ollama /
/// minimax 分支 + SDK 终点剥离）。
fn normalize_provider_base_url_for_api(provider: &str, base_url: &str, api: &str) -> String {
    let raw = base_url.trim();
    if raw.is_empty() {
        return raw.to_string();
    }
    if provider == "opencode" {
        let root = strip_terminal_provider_path(raw, &["/v1/messages", "/v1"]);
        return if api == "anthropic-messages" {
            root
        } else {
            append_provider_api_path(&root, "/v1")
        };
    }
    let sdk_base = normalize_exact_endpoint_for_sdk(raw, api);
    if provider == "kimi-coding" && api == "openai-completions" {
        if hostname_of(&sdk_base).as_deref() != Some("api.kimi.com") {
            return sdk_base;
        }
        return match split_url(&sdk_base) {
            Some((origin, path)) => {
                let pathname = path.trim_end_matches('/');
                if pathname == "/coding/v1" {
                    sdk_base
                } else if pathname.is_empty() || pathname == "/coding" {
                    format!("{origin}/coding/v1")
                } else {
                    sdk_base
                }
            }
            None => {
                if sdk_base == "https://api.kimi.com/coding" {
                    "https://api.kimi.com/coding/v1".to_string()
                } else {
                    sdk_base
                }
            }
        };
    }
    if provider == "ollama" && api == "openai-completions" {
        return match split_url(&sdk_base) {
            Some((origin, path)) => {
                let pathname = path.trim_end_matches('/');
                if pathname.to_ascii_lowercase().ends_with("/v1") {
                    format!("{origin}{pathname}")
                } else {
                    format!("{origin}{pathname}/v1")
                }
            }
            None => {
                if sdk_base.to_ascii_lowercase().ends_with("/v1") {
                    sdk_base
                } else {
                    format!("{sdk_base}/v1")
                }
            }
        };
    }
    if !matches!(provider, "minimax" | "minimax-token-plan") || api != "anthropic-messages" {
        return sdk_base;
    }
    let Some((origin, path)) = split_url(&sdk_base) else {
        return sdk_base;
    };
    let host = hostname_of(&sdk_base);
    if host.as_deref() != Some("api.minimaxi.com") && host.as_deref() != Some("api.minimax.io") {
        return sdk_base;
    }
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.first() == Some(&"anthropic") {
        return sdk_base;
    }
    if parts.is_empty() || (parts.len() == 1 && parts[0] == "v1") {
        return format!("{origin}/anthropic");
    }
    sdk_base
}

fn append_provider_api_path(base: &str, api_path: &str) -> String {
    let raw_base = base.trim();
    let normalized_api_path = format!("/{}", api_path.trim_start_matches('/'));
    match split_url(raw_base) {
        Some((origin, path)) => {
            let base_path = path.trim_end_matches('/');
            let target = normalized_api_path.trim_end_matches('/');
            if base_path
                .to_ascii_lowercase()
                .ends_with(&target.to_ascii_lowercase())
            {
                return strip_terminal_provider_path(raw_base, &[]);
            }
            let joined =
                if target.starts_with("/v1/") && base_path.to_ascii_lowercase().ends_with("/v1") {
                    format!("{base_path}{}", &target[3..])
                } else {
                    format!("{base_path}{target}")
                };
            format!("{origin}{joined}")
        }
        None => {
            let base2 = raw_base.trim_end_matches('/');
            if base2
                .to_ascii_lowercase()
                .ends_with(&normalized_api_path.to_ascii_lowercase())
            {
                return base2.to_string();
            }
            if normalized_api_path.starts_with("/v1/")
                && base2.to_ascii_lowercase().ends_with("/v1")
            {
                return format!("{base2}{}", &normalized_api_path[3..]);
            }
            format!("{base2}{normalized_api_path}")
        }
    }
}

// ── provider auth（shared/provider-auth.ts 子集）────────────────────────────

const FORBIDDEN_PROVIDER_HEADERS: [&str; 9] = [
    "accept-encoding",
    "connection",
    "content-length",
    "expect",
    "host",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn is_valid_provider_header_name(name: &str) -> bool {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^[!#$%&'*+.^_`|~0-9A-Za-z-]+$").expect("固定 HTTP 头名模式")
    });
    let key = name.trim().to_ascii_lowercase();
    !key.is_empty() && RE.is_match(&key) && !FORBIDDEN_PROVIDER_HEADERS.contains(&key.as_str())
}

/// normalizeProviderHeaders：过滤非法/被禁头名，大小写不敏感去重（先到先得）。
fn normalize_provider_headers(headers: Option<&Value>) -> Map<String, Value> {
    let mut by_lower: BTreeMap<String, (String, String)> = BTreeMap::new();
    let Some(map) = headers.and_then(Value::as_object) else {
        return Map::new();
    };
    for (raw_name, raw_value) in map {
        let name = raw_name.trim();
        if !is_valid_provider_header_name(name) {
            continue;
        }
        let Some(value) = raw_value.as_str() else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        by_lower
            .entry(name.to_ascii_lowercase())
            .or_insert_with(|| (name.to_string(), value.to_string()));
    }
    by_lower
        .into_values()
        .map(|(name, value)| (name, Value::String(value)))
        .collect()
}

fn provider_credential_allows_missing_api_key(auth_type: Option<&Value>, base_url: &str) -> bool {
    let normalized = normalize_auth_type(auth_type.unwrap_or(&Value::Null));
    if matches!(normalized, "none" | "optional") {
        return true;
    }
    hostname_of(base_url)
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"))
}

// ── models.json 投影（core/model-sync.ts syncModels 结构性子集）────────────

fn models_json_path(home: &Path) -> PathBuf {
    home.join("models.json")
}

fn humanize_name(id: &str) -> String {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"-(\d{6})$").expect("固定日期后缀模式"));
    let name = RE.replace(id, "").to_string();
    let name = {
        // [-_] → 空格；词首字母大写；数字-数字间空格变点。
        let mut out = String::new();
        let mut at_word_start = true;
        for ch in name.chars() {
            if ch == '-' || ch == '_' {
                out.push(' ');
                at_word_start = true;
            } else {
                if at_word_start {
                    out.extend(ch.to_uppercase());
                } else {
                    out.push(ch);
                }
                at_word_start = false;
            }
        }
        out
    };
    static DIGITS: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(\d) (\d)").expect("固定数字间隔模式"));
    DIGITS.replace_all(&name, "${1}.${2}").to_string()
}

/// filterChatModelEntries（known-models/Pi 目录不在端口：仅显式 type /
/// outputs 过滤；默认 type=chat）。
fn filter_chat_model_entries(models: &[Value]) -> Vec<Value> {
    models
        .iter()
        .filter(|model| {
            let entry = model.as_object();
            if let Some(outputs) = entry
                .and_then(|e| e.get("outputs"))
                .and_then(Value::as_array)
            {
                if outputs.iter().any(|item| item == &json!("text")) {
                    return true;
                }
            }
            let model_type = entry
                .and_then(|e| e.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("chat");
            model_type == "chat"
        })
        .cloned()
        .collect()
}

/// buildModelEntry 的显式字段子集（无 known-models/Pi 元数据增益）。
fn build_model_entry(
    model: &Value,
    provider: &str,
    provider_api: &str,
    provider_base_url: &str,
) -> Value {
    let entry = model.as_object().cloned().unwrap_or_default();
    let id = model_id_of(model).trim().to_string();
    let mut out = Map::new();
    out.insert("id".into(), json!(id.clone()));
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| humanize_name(&id));
    out.insert("name".into(), json!(name));
    let model_api = entry
        .get("api")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| provider_api.to_string());
    out.insert("api".into(), json!(model_api.clone()));
    let model_base_url =
        normalize_provider_base_url_for_api(provider, provider_base_url, &model_api);
    out.insert("baseUrl".into(), json!(model_base_url));
    if let Some(context) = entry
        .get("context")
        .or_else(|| entry.get("contextWindow"))
        .filter(|v| v.is_number())
    {
        out.insert("contextWindow".into(), context.clone());
    }
    let max_output = entry
        .get("maxOutput")
        .or_else(|| entry.get("maxTokens"))
        .or_else(|| entry.get("maxOutputTokens"));
    if let Some(max_output) = max_output {
        out.insert("maxTokens".into(), max_output.clone());
    }
    if let Some(value) = entry.get("outputIncludesThinking") {
        out.insert(
            "outputIncludesThinking".into(),
            json!(value == &json!(true)),
        );
    }
    out.insert("provider".into(), json!(provider));
    Value::Object(out)
}

/// syncModels：单向投影 catalog providers → models.json。内容不变且权限
/// 正确时不重写（返回 false）；有变化或权限错误时重写（0600）。
fn sync_models_json(home: &Path, providers: &Map<String, Value>) -> Result<bool, String> {
    let mut new_providers = Map::new();
    for (name, provider) in providers {
        let base_url = provider
            .get("base_url")
            .and_then(Value::as_str)
            .unwrap_or("");
        if base_url.is_empty() {
            continue;
        }
        let models = provider.get("models").and_then(Value::as_array);
        if models.is_none_or(|list| list.is_empty()) {
            continue;
        }
        validate_provider_models(name, base_url, provider.get("models"))?;
        let api_key = provider
            .get("api_key")
            .and_then(Value::as_str)
            .unwrap_or("");
        let has_literal_api_key = !api_key.is_empty();
        let headers = normalize_provider_headers(provider.get("headers"));
        let has_headers = !headers.is_empty();
        if !has_literal_api_key
            && !has_headers
            && !provider_credential_allows_missing_api_key(provider.get("auth_type"), base_url)
        {
            continue;
        }
        let configured_api = provider
            .get("api")
            .and_then(Value::as_str)
            .unwrap_or("openai-completions");
        let effective_api = if name == "kimi-coding" && is_official_kimi_coding_base_url(base_url) {
            "openai-completions".to_string()
        } else {
            configured_api.to_string()
        };
        let effective_base_url =
            normalize_provider_base_url_for_api(name, base_url, &effective_api);
        let chat_models = filter_chat_model_entries(models.expect("已判非空"));
        let mut custom_models = Vec::new();
        for model in &chat_models {
            custom_models.push(build_model_entry(
                model,
                name,
                &effective_api,
                &effective_base_url,
            ));
        }
        let mut provider_config = Map::new();
        provider_config.insert("baseUrl".into(), json!(effective_base_url));
        provider_config.insert("api".into(), json!(effective_api));
        let effective_api_key = if !api_key.is_empty() {
            api_key.to_string()
        } else if has_headers {
            "headers".to_string()
        } else {
            "local".to_string()
        };
        provider_config.insert(
            "apiKey".into(),
            json!(if has_literal_api_key {
                format!("hana-runtime-api-key:{name}")
            } else {
                effective_api_key
            }),
        );
        if has_headers {
            provider_config.insert("headers".into(), Value::Object(headers));
        }
        if !custom_models.is_empty() {
            provider_config.insert("models".into(), Value::Array(custom_models));
        }
        new_providers.insert(name.clone(), Value::Object(provider_config));
    }
    let document = json!({ "providers": Value::Object(new_providers) });
    let new_text = format!(
        "{}\n",
        serde_json::to_string_pretty(&document).map_err(|err| err.to_string())?
    );
    let path = models_json_path(home);
    match std::fs::read_to_string(&path) {
        Ok(old_text) if old_text == new_text => {
            if !secret_file_mode_ok(&path) {
                write_secret_file(&path, &new_text)
                    .map_err(|err| format!("cannot rewrite models.json for permissions: {err}"))?;
            }
            Ok(false)
        }
        _ => {
            write_secret_file(&path, &new_text)
                .map_err(|err| format!("cannot write models.json: {err}"))?;
            Ok(true)
        }
    }
}

// ── config scope 分流（shared/config-scope.ts + preferences-manager setters）─

/// CONFIG_SCHEMA 中 scope=global 且有 setter 的全部字段（config-schema.ts
/// 单一事实来源的逐条移植；未列出字段默认 agent scope）。
const GLOBAL_FIELDS: &[&str] = &[
    "locale",
    "user.name",
    "timezone",
    "sandbox",
    "sandbox_network",
    "hardware_acceleration",
    "file_backup",
    "update_channel",
    "auto_check_updates",
    "keep_awake",
    "thinking_level",
    "editor",
    "capabilities.learn_skills",
    "desk.heartbeat_master",
    "channels.enabled",
    "bridge.permissionMode",
    "bridge.readOnly",
    "bridge.receiptEnabled",
    "bridge.richStreamingEnabled",
    "automation.permissionMode",
    "network_proxy",
];

/// splitByScope：全局字段抽走（返回 schema 键与值），剩余为 agent partial。
fn split_by_scope(payload: &Value) -> (Vec<(&'static str, Value)>, Value) {
    let mut agent = payload.as_object().cloned().unwrap_or_default();
    let mut global = Vec::new();
    for field in GLOBAL_FIELDS {
        let Some((parent, child)) = field.split_once('.') else {
            if let Some(value) = agent.remove(*field) {
                global.push((*field, value));
            }
            continue;
        };
        let Some(parent_map) = agent.get_mut(parent).and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(value) = parent_map.remove(child) else {
            continue;
        };
        let parent_now_empty = agent
            .get(parent)
            .and_then(Value::as_object)
            .is_some_and(|m| m.is_empty());
        if parent_now_empty {
            agent.remove(parent);
        }
        global.push((*field, value));
    }
    (global, Value::Object(agent))
}

fn coerce_bool_like(value: &Value) -> Value {
    // setSandbox/setSandboxNetwork：字符串 "true"→true，其余按真值。
    if let Some(text) = value.as_str() {
        Value::Bool(text == "true")
    } else {
        Value::Bool(truthy(value))
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64().is_some_and(|n| n != 0.0),
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
    }
}

fn falsy_string_or_empty(value: &Value) -> Value {
    // setLocale/setTimezone：`value || ""`。
    if truthy(value) {
        value.clone()
    } else {
        Value::String(String::new())
    }
}

fn normalize_thinking_level(value: &Value) -> Value {
    let lowered = value.as_str().map(str::to_lowercase).unwrap_or_default();
    let canonical = match lowered.as_str() {
        "auto" => Some("medium"),
        "ultracode" => Some("max"),
        "off" | "low" | "medium" | "high" | "xhigh" | "max" => Some(lowered.as_str()),
        _ => None,
    };
    Value::String(canonical.unwrap_or("medium").to_string())
}

fn clamp_number(value: Option<&Value>, fallback: f64, min: f64, max: f64, decimals: u32) -> f64 {
    let parsed = value.and_then(|v| {
        v.as_f64().or_else(|| {
            v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse::<f64>().ok())
        })
    });
    let Some(parsed) = parsed else {
        return fallback;
    };
    if !parsed.is_finite() {
        return fallback;
    }
    let clamped = parsed.clamp(min, max);
    if decimals > 0 {
        let factor = 10f64.powi(decimals as i32);
        (clamped * factor).round() / factor
    } else {
        clamped.round()
    }
}

fn normalize_font_preset(value: Option<&Value>, fallback: &str) -> String {
    value
        .and_then(Value::as_str)
        .filter(|v| matches!(*v, "follow" | "serif" | "sans"))
        .map(str::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

fn normalize_content_width(value: Option<&Value>, fallback: Value) -> Value {
    if value.and_then(Value::as_str) == Some("unlimited") {
        return Value::String("unlimited".into());
    }
    let parsed = value.and_then(|v| {
        v.as_f64().or_else(|| {
            v.as_str()
                .map(str::trim)
                .and_then(|s| s.parse::<f64>().ok())
        })
    });
    match parsed {
        Some(n) if (n - 640.0).abs() < f64::EPSILON => json!(640),
        Some(n) if (n - 720.0).abs() < f64::EPSILON => json!(720),
        Some(n) if (n - 800.0).abs() < f64::EPSILON => json!(800),
        _ => fallback,
    }
}

/// normalizeEditorTypography + mergeEditorTypography（shared/editor-typography.ts）。
fn merge_editor_typography(base: Option<&Value>, patch: Option<&Value>) -> Value {
    let current = normalize_editor_typography(base);
    let source = patch
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or(json!({}));
    let merged_top = {
        let mut out = current.clone();
        if let (Some(target), Some(src)) = (out.as_object_mut(), source.as_object()) {
            for (key, value) in src {
                target.insert(key.clone(), value.clone());
            }
        }
        out
    };
    normalize_editor_typography(Some(&merged_top))
}

fn normalize_editor_typography(value: Option<&Value>) -> Value {
    let source = value
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or(json!({}));
    let markdown = source
        .get("markdown")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or(json!({}));
    let md = markdown.as_object().cloned().unwrap_or_default();
    let get = |key: &str| md.get(key);
    json!({
        "markdown": {
            "fontPreset": normalize_font_preset(get("fontPreset"), "follow"),
            "bodyFontSize": clamp_number(get("bodyFontSize"), 16.0, 12.0, 24.0, 0),
            "heading1FontSize": clamp_number(get("heading1FontSize"), 28.0, 16.0, 40.0, 0),
            "heading2FontSize": clamp_number(get("heading2FontSize"), 21.0, 15.0, 34.0, 0),
            "heading3FontSize": clamp_number(get("heading3FontSize"), 18.0, 14.0, 30.0, 0),
            "heading4FontSize": clamp_number(get("heading4FontSize"), 16.0, 13.0, 28.0, 0),
            "heading5FontSize": clamp_number(get("heading5FontSize"), 15.0, 12.0, 26.0, 0),
            "heading6FontSize": clamp_number(get("heading6FontSize"), 14.0, 12.0, 24.0, 0),
            "lineHeight": clamp_number(get("lineHeight"), 1.5, 1.2, 2.2, 2),
            "contentPadding": clamp_number(get("contentPadding"), 24.0, 0.0, 64.0, 0),
            "contentWidth": normalize_content_width(get("contentWidth"), json!(720)),
        }
    })
}

fn normalize_bridge_permission_mode(value: &Value) -> &'static str {
    if let Some(mode) = value.as_str() {
        if matches!(mode, "auto" | "operate" | "read_only") {
            return match mode {
                "auto" => "auto",
                "operate" => "operate",
                _ => "read_only",
            };
        }
    }
    if value
        .as_object()
        .is_some_and(|m| m.get("readOnly") == Some(&json!(true)))
    {
        return "read_only";
    }
    "auto"
}

/// 网络代理 strict 归一（shared/network-proxy.ts）；manual 无任何代理 URL
/// 或 URL 非法时抛错（现役 setNetworkProxy strict 行为）。
fn normalize_network_proxy_strict(value: &Value) -> Result<Value, String> {
    normalize_network_proxy(value, true)
}

/// normalizeNetworkProxyConfig（strict=false 为读侧宽松归一：非法模式回
/// system、非法/无主机 URL 清空、带路径的 URL 去路径、manual 无任何代理
/// 回默认配置）。
fn normalize_network_proxy(value: &Value, strict: bool) -> Result<Value, String> {
    let input = value.as_object();
    let mode = input
        .and_then(|m| m.get("mode"))
        .and_then(Value::as_str)
        .filter(|m| matches!(*m, "system" | "manual" | "direct"));
    let mode = match mode {
        Some(mode) => mode,
        None => {
            if let Some(raw) = input.and_then(|m| m.get("mode")) {
                if strict && !raw.is_null() {
                    return Err("network proxy mode must be system, manual, or direct".into());
                }
            }
            "system"
        }
    };
    let default_no_proxy = "localhost, 127.0.0.1, ::1";
    let no_proxy = match input.and_then(|m| m.get("noProxy")) {
        None => default_no_proxy.to_string(),
        Some(list) if list.is_array() => list
            .as_array()
            .expect("已判数组")
            .iter()
            .filter_map(|v| v.as_str().map(str::trim).filter(|s| !s.is_empty()))
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other
            .as_str()
            .unwrap_or(default_no_proxy)
            .split_whitespace()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
    };
    if mode != "manual" {
        return Ok(json!({
            "mode": mode,
            "httpProxy": "",
            "httpsProxy": "",
            "wsProxy": "",
            "wssProxy": "",
            "noProxy": no_proxy,
        }));
    }
    let normalize_url = |field: &str, value: Option<&Value>| -> Result<String, String> {
        let raw = value
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if raw.is_empty() {
            return Ok(String::new());
        }
        let lenient_empty = |message: String| -> Result<String, String> {
            if strict {
                Err(message)
            } else {
                Ok(String::new())
            }
        };
        let Some((scheme, rest)) = raw.split_once("://") else {
            return lenient_empty(format!("{field} must be a valid proxy URL"));
        };
        if rest.is_empty() || rest.contains(['?', '#']) {
            return lenient_empty(format!("{field} must be a valid proxy URL"));
        }
        if !matches!(scheme, "http" | "https" | "socks" | "socks5") {
            return lenient_empty(format!("{field} must use http, https, socks, or socks5"));
        }
        let (authority, path) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, ""),
        };
        if authority.is_empty() {
            return lenient_empty(format!("{field} must include a host"));
        }
        if !path.is_empty() && path != "/" {
            if strict {
                return Err(format!("{field} must not include a path"));
            }
            return Ok(format!("{scheme}://{authority}"));
        }
        Ok(raw.trim_end_matches('/').to_string())
    };
    let get = |key: &str| input.and_then(|m| m.get(key));
    let normalized = json!({
        "mode": "manual",
        "httpProxy": normalize_url("httpProxy", get("httpProxy"))?,
        "httpsProxy": normalize_url("httpsProxy", get("httpsProxy"))?,
        "wsProxy": normalize_url("wsProxy", get("wsProxy"))?,
        "wssProxy": normalize_url("wssProxy", get("wssProxy"))?,
        "noProxy": no_proxy,
    });
    let has_any = ["httpProxy", "httpsProxy", "wsProxy", "wssProxy"]
        .iter()
        .any(|key| normalized[key].as_str().is_some_and(|v| !v.is_empty()));
    if !has_any {
        if !strict {
            return Ok(json!({
                "mode": "system",
                "httpProxy": "",
                "httpsProxy": "",
                "wsProxy": "",
                "wssProxy": "",
                "noProxy": default_no_proxy,
            }));
        }
        return Err("manual network proxy requires at least one proxy URL".into());
    }
    Ok(normalized)
}

/// 逐字段调用 preferences-manager 对应 setter（写入前的内存偏好树）。
/// 返回 Err 时对应现役 setter 抛错 → 路由 500。
fn apply_global_setter(prefs: &mut Value, key: &str, value: &Value) -> Result<(), String> {
    fn ensure_object<'a>(prefs: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
        if !prefs.get(key).is_some_and(Value::is_object) {
            prefs[key] = json!({});
        }
        prefs
            .as_object_mut()
            .expect("偏好根为对象")
            .get_mut(key)
            .and_then(Value::as_object_mut)
            .expect("已确保对象")
    }
    match key {
        "locale" => prefs["locale"] = falsy_string_or_empty(value),
        "user.name" => {
            prefs["userName"] = Value::String(
                value
                    .as_str()
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_string(),
            )
        }
        "timezone" => prefs["timezone"] = falsy_string_or_empty(value),
        "sandbox" => prefs["sandbox"] = coerce_bool_like(value),
        "sandbox_network" => prefs["sandbox_network"] = coerce_bool_like(value),
        "hardware_acceleration" => {
            let enabled = match value.as_str() {
                Some(text) => !matches!(
                    text.trim().to_lowercase().as_str(),
                    "false" | "0" | "off" | "no" | "disabled"
                ),
                None => truthy(value),
            };
            prefs["hardware_acceleration"] = Value::Bool(enabled);
        }
        "file_backup" => {
            let mut merged = prefs
                .get("file_backup")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if let Some(patch) = value.as_object() {
                for (k, v) in patch {
                    merged.insert(k.clone(), v.clone());
                }
            }
            merged.remove("enabled");
            merged.remove("max_file_size_kb");
            let days = merged
                .get("retention_days")
                .and_then(Value::as_f64)
                .filter(|n| *n > 0.0);
            prefs["file_backup"] = json!({ "retention_days": days.unwrap_or(1.0) });
        }
        "update_channel" => {
            prefs["update_channel"] = if value.as_str() == Some("beta") {
                json!("beta")
            } else {
                json!("stable")
            }
        }
        "auto_check_updates" => {
            prefs["auto_check_updates"] = Value::Bool(value != &Value::Bool(false))
        }
        "keep_awake" => prefs["keep_awake"] = Value::Bool(value == &Value::Bool(true)),
        "thinking_level" => prefs["thinking_level"] = normalize_thinking_level(value),
        "editor" => {
            let current = prefs.get("editor");
            prefs["editor"] = merge_editor_typography(current, Some(value));
        }
        "capabilities.learn_skills" => {
            let mut merged = prefs
                .get("learn_skills")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if let Some(patch) = value.as_object() {
                for (k, v) in patch {
                    merged.insert(k.clone(), v.clone());
                }
            }
            prefs["learn_skills"] = Value::Object(merged);
        }
        "desk.heartbeat_master" => prefs["heartbeat_master"] = Value::Bool(truthy(value)),
        "channels.enabled" => prefs["channels_enabled"] = Value::Bool(truthy(value)),
        "bridge.permissionMode" => {
            let normalized = normalize_bridge_permission_mode(value);
            let bridge = ensure_object(prefs, "bridge");
            bridge.insert("permissionMode".into(), json!(normalized));
            if normalized == "read_only" {
                bridge.insert("readOnly".into(), json!(true));
            } else {
                bridge.remove("readOnly");
            }
        }
        "bridge.readOnly" => {
            let bridge = ensure_object(prefs, "bridge");
            if truthy(value) {
                bridge.insert("readOnly".into(), json!(true));
                if let Some(mode) = bridge.get("permissionMode") {
                    if mode.as_str() == Some("read_only") {
                        bridge.insert("permissionMode".into(), json!("read_only"));
                    }
                }
                if !bridge.contains_key("permissionMode") {
                    bridge.insert("permissionMode".into(), json!("read_only"));
                }
            } else {
                bridge.remove("readOnly");
                if bridge.get("permissionMode").and_then(Value::as_str) == Some("read_only") {
                    bridge.insert("permissionMode".into(), json!("auto"));
                }
            }
            if bridge.is_empty() {
                prefs
                    .as_object_mut()
                    .expect("偏好根为对象")
                    .remove("bridge");
            }
        }
        "bridge.receiptEnabled" | "bridge.richStreamingEnabled" => {
            let bridge = ensure_object(prefs, "bridge");
            let flag_key = if key == "bridge.receiptEnabled" {
                "receiptEnabled"
            } else {
                "richStreamingEnabled"
            };
            if value == &Value::Bool(false) {
                bridge.insert(flag_key.into(), json!(false));
            } else {
                bridge.remove(flag_key);
            }
            if bridge.is_empty() {
                prefs
                    .as_object_mut()
                    .expect("偏好根为对象")
                    .remove("bridge");
            }
        }
        "automation.permissionMode" => {
            let normalized = normalize_bridge_permission_mode(value);
            let automation = ensure_object(prefs, "automation");
            automation.insert("permissionMode".into(), json!(normalized));
        }
        "network_proxy" => {
            prefs["network_proxy"] = normalize_network_proxy_strict(value)?;
        }
        other => return Err(format!("unknown global config key: {other}")),
    }
    Ok(())
}

// ── 路由级共享 ──────────────────────────────────────────────────────────────

fn bad_request(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message.into() })),
    )
        .into_response()
}

fn internal_error(message: impl Into<String>) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": message.into() })),
    )
        .into_response()
}

#[allow(clippy::result_large_err)]
fn parse_body_object(body: &Bytes) -> Result<Value, Response> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    let parsed: Value = serde_json::from_slice(body).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("invalid JSON body: {err}") })),
        )
            .into_response()
    })?;
    if !parsed.is_object() {
        return Err(bad_request("invalid JSON body"));
    }
    Ok(parsed)
}

// ── PUT /agents/{id}/config（agents.ts:636-799）──────────────────────────────

const OPTIONAL_TOOL_NAMES: [&str; 19] = [
    "automation",
    "ast_edit",
    "ast_grep",
    "checkpoint",
    "goal",
    "context_notes",
    "beautify",
    "browser",
    "lsp",
    "install_skill",
    "learn_lesson",
    "office",
    "rewind",
    "run_code",
    "run_tools",
    "security_scan",
    "session",
    "update_settings",
    "workflow",
];

fn has_inline_provider_credential_patch(block: Option<&Value>) -> bool {
    block.is_some_and(|block| {
        block
            .as_object()
            .is_some_and(|map| map.contains_key("api_key") || map.contains_key("base_url"))
    })
}

/// hasProviderMutationPatch（agents.ts:124-128）：providers 块或 api /
/// embedding_api 内联凭证补丁。
fn has_provider_mutation_patch(partial: &Value) -> bool {
    if partial.get("providers").is_some() {
        return true;
    }
    ["api", "embedding_api"]
        .iter()
        .any(|key| has_inline_provider_credential_patch(partial.get(key)))
}

/// collectSecretPatchPaths（secretKeys=["api_key"]）：未掩码 api_key 的
/// 补丁路径（越权即 403，路径随错误返回）。
fn collect_secret_patch_paths(value: &Value, prefix: &str, paths: &mut Vec<String>) {
    let Some(map) = value.as_object() else {
        return;
    };
    for (key, entry) in map {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if key == "api_key" {
            if !is_masked_secret_value(entry) {
                paths.push(path);
            }
            continue;
        }
        collect_secret_patch_paths(entry, &path, paths);
    }
}

fn scope_denied(principal: Option<&crate::auth::Principal>, scope: &str) -> bool {
    principal.is_some_and(|principal| !principal.has_scope(scope))
}

/// buildInlineProviderCredentialUpdate（server/routes/provider-credentials.ts）。
fn build_inline_provider_credential_update(
    block: &Value,
    fallback_provider: &str,
    existing_provider: &Value,
) -> (String, Value) {
    let provider = block
        .get("provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback_provider.trim().to_string());
    let mut update = Map::new();
    if let Some(api_key) = block.get("api_key") {
        update.insert(
            "api_key".into(),
            if is_masked_secret_value(api_key) {
                existing_provider
                    .get("api_key")
                    .cloned()
                    .unwrap_or(Value::String(String::new()))
            } else {
                api_key
                    .as_str()
                    .map(|v| Value::String(v.to_string()))
                    .unwrap_or(Value::String(String::new()))
            },
        );
    }
    if let Some(base_url) = block.get("base_url") {
        update.insert(
            "base_url".into(),
            Value::String(base_url.as_str().unwrap_or("").to_string()),
        );
    }
    (provider, Value::Object(update))
}

/// saveConfig（config-loader.ts:114-134）：读盘 → deepMerge partial → 渲染
/// → 0600 原子写。partial 为空对象时仍重渲染落盘（焦点刷新路径）。
fn save_agent_config_merge(config_path: &Path, partial: &Value) -> Result<(), String> {
    let text = std::fs::read_to_string(config_path)
        .map_err(|err| format!("cannot read config.yaml: {err}"))?;
    let mut document = am::config::AgentConfigDocument::parse(&text)
        .map_err(|err| format!("invalid config.yaml: {err}"))?;
    document
        .deep_merge(partial)
        .map_err(|err| format!("cannot merge config patch: {err}"))?;
    let rendered = document
        .render()
        .map_err(|err| format!("cannot render config.yaml: {err}"))?;
    write_secret_file(config_path, &rendered)
        .map_err(|err| format!("cannot write config.yaml: {err}"))
}

/// assertAgentConfigPatchYuan（core/yuan-registry.ts）：agent.yuan 必须是
/// product_dir/yuan 下已注册的模板键；空值回退 "lingxi"。
#[allow(clippy::result_large_err)]
fn assert_patch_yuan(state: &ServiceState, agent_partial: &Value) -> Result<(), Response> {
    let Some(yuan) = agent_partial
        .get("agent")
        .and_then(|agent| agent.get("yuan"))
    else {
        return Ok(());
    };
    let raw = match yuan.as_str() {
        Some(text) => text,
        None => return Err(bad_request(format!("Invalid yuan {yuan}: expected string"))),
    };
    let key = raw.trim();
    let key = if key.is_empty() { "lingxi" } else { key };
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(bad_request(format!(
            "Invalid yuan \"{key}\": expected a template key"
        )));
    }
    let product_dir = crate::context_compiler::resolve_product_dir(
        std::env::var("LINGXI_PRODUCT_DIR").ok().as_deref(),
        &std::env::current_exe().unwrap_or_else(|_| state.memory().home().join("lingxi-service")),
        state.memory().home(),
    );
    let yuan_dir = product_dir.join("yuan");
    let known = ["", "en"]
        .iter()
        .any(|sub| yuan_dir.join(sub).join(format!("{key}.md")).is_file());
    if !known {
        return Err(bad_request(format!(
            "Invalid yuan \"{key}\": template not found in lib/yuan"
        )));
    }
    Ok(())
}

/// 焦点助手解析：current_agent_id → primary_agent_id → None（引擎焦点指针
/// 的磁盘投影）。
fn focus_agent_id(memory: &MemoryService) -> Result<Option<String>, String> {
    memory.current_agent_preference().map_err(|err| err.message)
}

/// updateConfig({agentId}) 端口：命名 agent 的 config 可解析 → 目标即它；
/// 否则回退焦点；两者皆无 → 显式失败（现役 getAgentById || getAgent 链）。
fn update_config_for_agent(
    memory: &MemoryService,
    agent_id: Option<&str>,
    partial: &Value,
) -> Result<(), String> {
    let target = match agent_id {
        Some(id) if memory.agents_root().join(id).join("config.yaml").is_file() => id.to_string(),
        _ => match focus_agent_id(memory)? {
            Some(focus) => focus,
            None => {
                return Err(
                    "no agent runtime available for config refresh (no focus agent)".to_string(),
                );
            }
        },
    };
    let config_path = memory.agents_root().join(&target).join("config.yaml");
    save_agent_config_merge(&config_path, partial)
}

pub async fn agent_config_put_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
    principal_extension: Option<axum::Extension<crate::auth::Principal>>,
    body: Bytes,
) -> Response {
    let principal = principal_extension.map(|axum::Extension(principal)| principal);
    if validate_agent_id(&agent_id).is_err() {
        return bad_request(format!("Invalid agent ID: {agent_id}"));
    }
    let memory = state.memory();
    let Ok(agent_dir) = memory.agent_dir_checked(&agent_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("agent not found: {agent_id}") })),
        )
            .into_response();
    };
    let partial = match parse_body_object(&body) {
        Ok(value) => value,
        Err(response) => return response,
    };

    // capability-guard 端口：settings.write 必需；供应商变更另需
    // providers.manage；未掩码 api_key 另需 secrets.write。
    if scope_denied(principal.as_ref(), "settings.write") {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "insufficient_scope", "scope": "settings.write" })),
        )
            .into_response();
    }
    if has_provider_mutation_patch(&partial) && scope_denied(principal.as_ref(), "providers.manage")
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "insufficient_scope", "scope": "providers.manage" })),
        )
            .into_response();
    }
    let mut secret_fields = Vec::new();
    collect_secret_patch_paths(&partial, "", &mut secret_fields);
    if !secret_fields.is_empty() && scope_denied(principal.as_ref(), "secrets.write") {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "secret_write_scope_required",
                "scope": "secrets.write",
                "fields": secret_fields,
            })),
        )
            .into_response();
    }

    if let Err(detail) = crate::compaction::CompactionSettings::from_config(&partial) {
        return bad_request(detail);
    }
    // 白名单校验：tools.disabled 只允许 OPTIONAL_TOOL_NAMES。
    if let Some(disabled) = partial
        .get("tools")
        .and_then(|tools| tools.get("disabled"))
        .filter(|v| !v.is_null())
    {
        let Some(list) = disabled.as_array() else {
            return bad_request("tools.disabled must be an array");
        };
        let invalid: Vec<String> = list
            .iter()
            .filter_map(Value::as_str)
            .filter(|name| !OPTIONAL_TOOL_NAMES.contains(name))
            .map(str::to_string)
            .collect();
        if !invalid.is_empty() {
            return bad_request(format!(
                "Invalid tool names in tools.disabled: {}. Only optional tools can be disabled.",
                invalid.join(", ")
            ));
        }
    }
    if let Some(enabled) = partial
        .get("experience")
        .and_then(|experience| experience.get("enabled"))
        .filter(|v| !v.is_null())
    {
        if !enabled.is_boolean() {
            return bad_request("experience.enabled must be a boolean");
        }
    }
    if let Some(auto) = partial
        .get("memory")
        .and_then(|memory_block| memory_block.get("dream"))
        .and_then(|dream| dream.get("auto_enabled"))
        .filter(|v| !v.is_null())
    {
        if !auto.is_boolean() {
            return bad_request("memory.dream.auto_enabled must be a boolean");
        }
    }
    if let Some(workspace_context) = partial.get("workspace_context").filter(|v| !v.is_null()) {
        let Some(map) = workspace_context.as_object() else {
            return bad_request("workspace_context must be an object");
        };
        for field in [
            "discover_project_skills",
            "discover_compatible_project_skills",
            "inject_custom_file",
        ] {
            if let Some(value) = map.get(field) {
                if !value.is_boolean() {
                    return bad_request(format!("workspace_context.{field} must be a boolean"));
                }
            }
        }
        if let Some(name) = map.get("custom_file_name") {
            let Some(name) = name.as_str() else {
                return bad_request("workspace_context.custom_file_name must be a string");
            };
            let trimmed = name.trim();
            if trimmed == "." || trimmed == ".." || trimmed.contains('/') || trimmed.contains('\\')
            {
                return bad_request(
                    "workspace_context.custom_file_name must be a plain file name without path separators",
                );
            }
        }
    }

    // ── schema 驱动的全局字段分流：全局 setter 写 preferences.json ──
    let (global_fields, mut agent_partial) = split_by_scope(&partial);
    if !global_fields.is_empty() {
        let _guard = crate::agents_admin::preferences_locked();
        let mut preferences = match memory.read_preferences_root() {
            Ok(root) => root,
            Err(message) => return internal_error(message),
        };
        for (key, value) in &global_fields {
            if let Err(message) = apply_global_setter(&mut preferences, key, value) {
                return internal_error(message);
            }
        }
        if let Err(message) = memory.write_preferences_root(&preferences) {
            return internal_error(message);
        }
    }

    let home = memory.home().to_path_buf();

    // ── providers 块 → 全局 provider 目录（逐项处理，后项失败保留前项）──
    let mut providers_changed = false;
    let requested_providers = agent_partial
        .get("providers")
        .and_then(Value::as_object)
        .cloned();
    if let Some(requested) = requested_providers {
        // 转存前的既有目录快照（getAllProvidersRaw 的读取时机）。
        let existing_catalog = load_catalog(&home);
        for (name, data) in &requested {
            let existing = existing_catalog
                .as_ref()
                .ok()
                .and_then(|catalog| catalog.providers.get(name).cloned())
                .unwrap_or_else(|| json!({}));
            let outcome = if data.is_null() {
                registry_remove_provider(&home, name)
            } else {
                let resolved = resolve_secret_patch(data, &existing);
                registry_save_provider(&home, name, &resolved)
            };
            // 部分成功语义：先前目录/插件变更保留，HTTP 如实报错。
            if let Err(message) = outcome {
                return internal_error(message);
            }
        }
        agent_partial
            .as_object_mut()
            .expect("分流后仍是对象")
            .remove("providers");
        providers_changed = true;
    }

    // ── 内联 API 凭证 → 目录转存，请求助手 YAML 只留清空值 ──
    for block_name in ["api", "embedding_api"] {
        let Some(block) = agent_partial.get(block_name).cloned() else {
            continue;
        };
        if !has_inline_provider_credential_patch(Some(&block)) {
            continue;
        }
        // fallback provider：请求助手 config.yaml 同名块的 provider 值
        // （agentCfg[blockName]?.provider，agents.ts:717-721）。
        let fallback_provider =
            am::config::read_yaml_mapping_as_json(&agent_dir.join("config.yaml"))
                .ok()
                .flatten()
                .and_then(|config| {
                    config
                        .get(block_name)?
                        .get("provider")?
                        .as_str()
                        .map(str::to_string)
                })
                .unwrap_or_default();
        let provider_key = block
            .get("provider")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| fallback_provider.trim().to_string());
        if provider_key.is_empty() {
            return bad_request(format!(
                "{block_name}.provider is required when saving credentials"
            ));
        }
        let existing = merged_catalog_providers(&home)
            .ok()
            .and_then(|mut merged| merged.remove(&provider_key))
            .unwrap_or_else(|| json!({}));
        let (_, update) =
            build_inline_provider_credential_update(&block, &fallback_provider, &existing);
        if let Err(message) = registry_save_provider(&home, &provider_key, &update) {
            return internal_error(message);
        }
        // clearInlineProviderCredentialFields：请求助手 YAML 的内联秘密清空。
        let cleared = agent_partial
            .get_mut(block_name)
            .and_then(Value::as_object_mut)
            .expect("已判对象");
        cleared.insert("api_key".into(), Value::String(String::new()));
        cleared.insert("base_url".into(), Value::String(String::new()));
        providers_changed = true;
    }

    // ── providers 变更后：models.json 投影 + 焦点刷新（onProviderChanged +
    //    engine.updateConfig({})）。失败时目录已变，不宣称原子回滚。──
    if providers_changed {
        let merged = match merged_catalog_providers(&home) {
            Ok(merged) => merged,
            Err(message) => return internal_error(message),
        };
        if let Err(message) = sync_models_json(&home, &merged) {
            return internal_error(message);
        }
        match focus_agent_id(memory) {
            Ok(Some(_)) => {
                if let Err(message) = update_config_for_agent(memory, None, &json!({})) {
                    return internal_error(message);
                }
            }
            Ok(None) => {
                // 无焦点时无 ID 刷新可失败：显式报错，不静默跳过。
                return internal_error("provider refresh has no focus agent to update config for");
            }
            Err(message) => return internal_error(message),
        }
    }

    if agent_partial.as_object().is_some_and(|map| map.is_empty()) {
        emit_agent_config_app_events(
            &state,
            &agent_id,
            &global_fields,
            &agent_partial,
            providers_changed,
        );
        return Json(json!({ "ok": true })).into_response();
    }

    // 记忆总开关时间戳。
    if let Some(memory_block) = agent_partial
        .get_mut("memory")
        .and_then(Value::as_object_mut)
    {
        if let Some(enabled) = memory_block.get("enabled").cloned() {
            let now = am::now_iso();
            if enabled == Value::Bool(false) {
                memory_block.insert("disabledSince".into(), json!(now));
            } else {
                memory_block.insert("reenableAt".into(), json!(now));
            }
        }
    }

    if let Err(response) = assert_patch_yuan(&state, &agent_partial) {
        return response;
    }

    // 显式保存请求助手 A。
    let config_path = agent_dir.join("config.yaml");
    if let Err(message) = memory.dream().save_config(
        &agent_id,
        agent_partial
            .pointer("/memory/enabled")
            .and_then(Value::as_bool),
        || save_agent_config_merge(&config_path, &agent_partial),
    ) {
        return internal_error(message);
    }
    // updateConfig(agentPartial, {agentId})：运行时刷新（端口=重读合并）。
    if let Err(message) = update_config_for_agent(memory, Some(&agent_id), &agent_partial) {
        return internal_error(message);
    }

    emit_agent_config_app_events(
        &state,
        &agent_id,
        &global_fields,
        &agent_partial,
        providers_changed,
    );
    Json(json!({ "ok": true })).into_response()
}

/// emitAgentConfigAppEvents（agents.ts:195-263）的端口：models-changed /
/// agent-updated / agent-workspace-changed / memory-master-changed /
/// locale-changed / editor-typography-changed / network-proxy-changed /
/// keep-awake-changed / skills-changed。
fn emit_agent_config_app_events(
    state: &ServiceState,
    agent_id: &str,
    global_fields: &[(&'static str, Value)],
    agent_partial: &Value,
    providers_changed: bool,
) {
    let events = state.app_events();
    let has = |key: &str, child: Option<&str>| match child {
        None => agent_partial.get(key).is_some(),
        Some(child) => agent_partial
            .get(key)
            .and_then(Value::as_object)
            .is_some_and(|map| map.contains_key(child)),
    };
    if providers_changed || has("api", None) || has("embedding_api", None) || has("models", None) {
        events.publish("models-changed", json!({ "agentId": agent_id }));
    }
    let agent_section = agent_partial.get("agent").and_then(Value::as_object);
    if agent_section.is_some_and(|m| m.contains_key("name") || m.contains_key("yuan")) {
        events.publish(
            "agent-updated",
            json!({
                "agentId": agent_id,
                "agentName": agent_section.and_then(|m| m.get("name")).cloned().unwrap_or(Value::Null),
                "yuan": agent_section.and_then(|m| m.get("yuan")).cloned().unwrap_or(Value::Null),
            }),
        );
    }
    if has("desk", Some("home_folder")) {
        events.publish(
            "agent-workspace-changed",
            json!({
                "agentId": agent_id,
                "homeFolder": agent_partial
                    .pointer("/desk/home_folder")
                    .cloned()
                    .filter(truthy)
                    .unwrap_or(Value::Null),
            }),
        );
    }
    if has("memory", Some("enabled")) {
        events.publish(
            "memory-master-changed",
            json!({
                "agentId": agent_id,
                "enabled": agent_partial.pointer("/memory/enabled") != Some(&Value::Bool(false)),
            }),
        );
    }
    let global_value = |key: &str| {
        global_fields
            .iter()
            .find(|(field, _)| *field == key)
            .map(|(_, value)| value.clone())
    };
    if let Some(locale) = global_value("locale") {
        events.publish("locale-changed", json!({ "locale": locale }));
    }
    if let Some(editor) = global_value("editor") {
        events.publish("editor-typography-changed", json!({ "editor": editor }));
    }
    if let Some(proxy) = global_value("network_proxy") {
        events.publish("network-proxy-changed", json!({ "network_proxy": proxy }));
    }
    if let Some(keep_awake) = global_value("keep_awake") {
        events.publish(
            "keep-awake-changed",
            json!({ "keep_awake": keep_awake == Value::Bool(true) }),
        );
    }
    let has_workspace_skill_policy = agent_partial
        .get("workspace_context")
        .and_then(Value::as_object)
        .is_some_and(|map| {
            map.contains_key("discover_project_skills")
                || map.contains_key("discover_compatible_project_skills")
        });
    if agent_partial.get("skills").is_some() || has_workspace_skill_policy {
        events.publish("skills-changed", json!({ "agentId": agent_id }));
    }
}

// ── GET /agents/{id}/config（agents.ts:557-634）──────────────────────────────

/// DEFAULT_SECRET_KEYS（shared/secret-custody.ts）：精确键名注册表，不做
/// 后缀/子串匹配。
const SECRET_KEYS: [&str; 16] = [
    "api_key",
    "apiKey",
    "token",
    "botToken",
    "webhookToken",
    "robotToken",
    "appSecret",
    "clientSecret",
    "corpSecret",
    "webhookSecret",
    "oauthClientSecret",
    "suiteSecret",
    "secret",
    "password",
    "accessToken",
    "refreshToken",
];

/// maskSecretValue：非空字符串 → `********`，其余（含空串/非字符串）→ ""。
fn mask_secret_value(value: &Value) -> Value {
    match value.as_str() {
        Some(text) if !text.is_empty() => Value::String(MASKED_SECRET.to_string()),
        _ => Value::String(String::new()),
    }
}

/// maskObjectSecrets：递归掩码全部密钥字段（数组逐项递归）。
fn mask_object_secrets(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(mask_object_secrets).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, entry)| {
                    let masked = if SECRET_KEYS.contains(&key.as_str()) {
                        mask_secret_value(entry)
                    } else {
                        mask_object_secrets(entry)
                    };
                    (key.clone(), masked)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// JS 真值（`{}`/`[]` 为真，与 `truthy` 的写侧口径不同）。
fn js_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(v)) => *v,
        Some(Value::Number(v)) => v.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(v)) => !v.is_empty(),
        Some(_) => true,
    }
}

/// `value || ""`。
fn or_empty_string(value: Option<&Value>) -> Value {
    match value {
        Some(v) if js_truthy(Some(v)) => v.clone(),
        _ => Value::String(String::new()),
    }
}

/// normalizeWorkspacePath（shared/workspace-history.ts）。
fn normalize_workspace_path(value: Option<&Value>) -> Option<String> {
    let trimmed = value?.as_str()?.trim();
    if trimmed.is_empty() {
        return None;
    }
    let slashed = trimmed.replace('\\', "/");
    if slashed == "/" {
        return Some(slashed);
    }
    let bytes = slashed.as_bytes();
    let drive_root = (bytes.len() == 2 || (bytes.len() == 3 && bytes[2] == b'/'))
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':';
    if drive_root {
        return Some(if slashed.ends_with('/') {
            slashed
        } else {
            format!("{slashed}/")
        });
    }
    Some(slashed.trim_end_matches('/').to_string())
}

const WORKSPACE_HISTORY_LIMIT: usize = 10;

/// mergeWorkspaceHistory(existing, [])：规范化、去重、按上限截断。
fn merge_workspace_history(existing: &[Value]) -> Vec<String> {
    let mut next: Vec<String> = Vec::new();
    for value in existing {
        let Some(normalized) = normalize_workspace_path(Some(value)) else {
            continue;
        };
        if !next.contains(&normalized) {
            next.push(normalized);
        }
    }
    next.truncate(WORKSPACE_HISTORY_LIMIT);
    next
}

/// classifyWorkspacePathForGc：只有确定"不存在"（ENOENT/ENOTDIR）才算
/// missing；权限错误等未知状态保留。
fn workspace_path_is_missing(path: &str) -> bool {
    match std::fs::metadata(path) {
        Ok(_) => false,
        Err(err) => matches!(
            err.kind(),
            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
        ),
    }
}

/// pruneMissingWorkspaceConfig（shared/workspace-persistence-gc.ts）：
/// cwd_history 丢弃已消失目录，last_cwd / desk.home_folder 消失则置 null
/// （saveConfig 的 null 删键语义）。
fn prune_missing_workspace_config(config: &Value) -> Option<Value> {
    let mut patch = Map::new();
    if let Some(history) = config.get("cwd_history").and_then(Value::as_array) {
        let current = merge_workspace_history(history);
        let kept: Vec<String> = current
            .iter()
            .filter(|item| !workspace_path_is_missing(item))
            .cloned()
            .collect();
        if kept != current {
            patch.insert("cwd_history".into(), json!(kept));
        }
    }
    if let Some(last) = normalize_workspace_path(config.get("last_cwd")) {
        if workspace_path_is_missing(&last) {
            patch.insert("last_cwd".into(), Value::Null);
        }
    }
    if let Some(home) =
        normalize_workspace_path(config.get("desk").and_then(|d| d.get("home_folder")))
    {
        if workspace_path_is_missing(&home) {
            patch.insert("desk".into(), json!({ "home_folder": null }));
        }
    }
    if patch.is_empty() {
        None
    } else {
        Some(Value::Object(patch))
    }
}

/// gcWorkspacePersistence({agentId}) 端口：请求助手 config.yaml 的工作区
/// 历史 + preferences.json 的 workspace_ui_state 里已消失的根目录。
/// workspace_ui_state 只删 workspaces 的已消失键（端口没有该状态的
/// 写入面，不重新归一化其余内容）。无变化不落盘。
fn gc_workspace_persistence(memory: &MemoryService, agent_dir: &Path) -> Result<(), String> {
    let config_path = agent_dir.join("config.yaml");
    let config = am::config::read_yaml_mapping_as_json(&config_path)
        .map_err(|err| format!("cannot read config.yaml: {err}"))?
        .unwrap_or_else(|| json!({}));
    if let Some(patch) = prune_missing_workspace_config(&config) {
        save_agent_config_merge(&config_path, &patch)?;
    }

    let _guard = crate::agents_admin::preferences_locked();
    let mut preferences = memory.read_preferences_root()?;
    let mut changed = false;
    if let Some(workspaces) = preferences
        .get_mut("workspace_ui_state")
        .and_then(|state| state.get_mut("workspaces"))
        .and_then(Value::as_object_mut)
    {
        let before = workspaces.len();
        workspaces.retain(|workspace, _| {
            normalize_workspace_path(Some(&Value::String(workspace.clone())))
                .is_none_or(|path| !workspace_path_is_missing(&path))
        });
        changed = workspaces.len() != before;
    }
    if changed {
        memory.write_preferences_root(&preferences)?;
    }
    Ok(())
}

fn pref_bool_not_false(prefs: &Value, key: &str) -> Value {
    Value::Bool(prefs.get(key) != Some(&Value::Bool(false)))
}

fn permission_mode_from(block: Option<&Value>, allow_read_only_flag: bool) -> Value {
    let block = block.and_then(Value::as_object);
    if let Some(mode) = block
        .and_then(|m| m.get("permissionMode"))
        .and_then(Value::as_str)
    {
        if matches!(mode, "auto" | "operate" | "read_only") {
            return json!(mode);
        }
    }
    if allow_read_only_flag && block.is_some_and(|m| m.get("readOnly") == Some(&Value::Bool(true)))
    {
        return json!("read_only");
    }
    json!("auto")
}

/// CONFIG_SCHEMA 全局字段 getter（core/preferences-manager.ts 各 getter
/// 的读侧口径，默认值/归一一致）。
fn global_field_value(prefs: &Value, key: &str) -> Value {
    match key {
        "locale" => or_empty_string(prefs.get("locale")),
        "user.name" => or_empty_string(prefs.get("userName")),
        "timezone" => or_empty_string(prefs.get("timezone")),
        "sandbox" => pref_bool_not_false(prefs, "sandbox"),
        // 现役：win32 无网络隔离，恒为 true。
        "sandbox_network" => {
            if cfg!(windows) {
                Value::Bool(true)
            } else {
                pref_bool_not_false(prefs, "sandbox_network")
            }
        }
        "hardware_acceleration" => pref_bool_not_false(prefs, "hardware_acceleration"),
        "file_backup" => {
            let days = prefs
                .get("file_backup")
                .and_then(|cfg| cfg.get("retention_days"))
                .and_then(|v| match v {
                    Value::Number(n) => n.as_f64(),
                    Value::String(text) => text.trim().parse::<f64>().ok(),
                    _ => None,
                })
                .filter(|n| n.is_finite() && *n > 0.0);
            json!({ "retention_days": days.unwrap_or(1.0) })
        }
        "update_channel" => match prefs.get("update_channel") {
            value if js_truthy(value) => value.cloned().unwrap_or(Value::Null),
            _ => json!("stable"),
        },
        "auto_check_updates" => pref_bool_not_false(prefs, "auto_check_updates"),
        "keep_awake" => Value::Bool(prefs.get("keep_awake") == Some(&Value::Bool(true))),
        "thinking_level" => {
            normalize_thinking_level(prefs.get("thinking_level").unwrap_or(&Value::Null))
        }
        "editor" => normalize_editor_typography(prefs.get("editor")),
        "capabilities.learn_skills" => match prefs.get("learn_skills") {
            value if js_truthy(value) => value.cloned().unwrap_or(Value::Null),
            _ => json!({ "enabled": true, "safety_review": true }),
        },
        "desk.heartbeat_master" => pref_bool_not_false(prefs, "heartbeat_master"),
        "channels.enabled" => {
            Value::Bool(prefs.get("channels_enabled") == Some(&Value::Bool(true)))
        }
        "bridge.permissionMode" => permission_mode_from(prefs.get("bridge"), true),
        "bridge.readOnly" => {
            Value::Bool(permission_mode_from(prefs.get("bridge"), true) == json!("read_only"))
        }
        "bridge.receiptEnabled" => Value::Bool(
            prefs.get("bridge").and_then(|b| b.get("receiptEnabled")) != Some(&Value::Bool(false)),
        ),
        "bridge.richStreamingEnabled" => Value::Bool(
            prefs
                .get("bridge")
                .and_then(|b| b.get("richStreamingEnabled"))
                != Some(&Value::Bool(false)),
        ),
        "automation.permissionMode" => permission_mode_from(prefs.get("automation"), false),
        "network_proxy" => {
            normalize_network_proxy(prefs.get("network_proxy").unwrap_or(&Value::Null), false)
                .expect("宽松归一不会失败")
        }
        other => unreachable!("未登记的全局字段 {other}"),
    }
}

/// injectGlobalFields（shared/config-scope.ts）：全局字段覆盖到 config
/// 对象（嵌套父节点不是对象时先置为 {}）。
fn inject_global_fields(config: &mut Map<String, Value>, prefs: &Value) {
    for field in GLOBAL_FIELDS {
        let value = global_field_value(prefs, field);
        match field.split_once('.') {
            None => {
                config.insert((*field).to_string(), value);
            }
            Some((parent, child)) => {
                let slot = config
                    .entry(parent.to_string())
                    .or_insert_with(|| json!({}));
                if !slot.is_object() {
                    *slot = json!({});
                }
                slot.as_object_mut()
                    .expect("已确保对象")
                    .insert(child.to_string(), value);
            }
        }
    }
}

/// normalizeExperienceConfigForResponse：experience 对象化，enabled 只认
/// 布尔 true。
fn normalize_experience_for_response(config: &mut Map<String, Value>) {
    let mut experience = config
        .get("experience")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let enabled = experience.get("enabled") == Some(&Value::Bool(true));
    experience.insert("enabled".into(), Value::Bool(enabled));
    config.insert("experience".into(), Value::Object(experience));
}

/// 供应商摘要（base_url / api / 掩码 api_key / models / model_count）。
/// 目录读取失败 → `{}`（现役 catch 分支），失败原因记日志而不伪装成空目录。
fn providers_summary(home: &Path) -> Value {
    let providers = match merged_catalog_providers_with(home, false) {
        Ok(providers) => providers,
        Err(message) => {
            tracing::warn!(%message, "provider catalog unreadable; GET agent config returns empty providers");
            return json!({});
        }
    };
    let mut entries = Map::new();
    for (name, raw) in &providers {
        let models = raw.get("models").and_then(Value::as_array);
        let text = |key: &str| {
            raw.get(key)
                .filter(|v| js_truthy(Some(v)))
                .cloned()
                .unwrap_or_else(|| Value::String(String::new()))
        };
        entries.insert(
            name.clone(),
            json!({
                "base_url": text("base_url"),
                "api": text("api"),
                "api_key": mask_secret_value(raw.get("api_key").unwrap_or(&Value::Null)),
                "models": models.cloned().unwrap_or_default(),
                "model_count": models.map_or(0, Vec::len),
            }),
        );
    }
    Value::Object(entries)
}

const CORE_TOOL_NAMES: [&str; 12] = [
    "read",
    "write",
    "edit",
    "exec_command",
    "write_stdin",
    "grep",
    "find",
    "ls",
    "search_memory",
    "pin_memory",
    "unpin_memory",
    "web_search",
];

const STANDARD_TOOL_NAMES: [&str; 26] = [
    "web_fetch",
    "todo_write",
    "notify",
    "stage_files",
    "file",
    "materialize",
    "subagent",
    "subagent_reply",
    "subagent_close",
    "channel",
    "record_experience",
    "recall_experience",
    "tenet_propose",
    "check_pending_tasks",
    "current_status",
    "ask_user",
    "session_folders",
    "stop_task",
    "hana_card_guide",
    "show_card",
    "loop_control",
    "knowledge_search",
    "knowledge_read",
    "knowledge_outline",
    "knowledge_grep",
    "knowledge_manage",
];

/// 插件承载的可选类别：只有对应插件工具真的注册才出现在设置面。
const PLUGIN_BACKED_OPTIONAL_TOOLS: [&str; 2] = ["beautify", "office"];

/// computeSettingsAvailableToolNames（shared/tool-categories.ts）+
/// hideDisabledGlobalToolsForSettings 的端口：设置面的稳定工具面 =
/// 常驻内置（core + standard）∪ 全部可选开关。端口没有运行时 agent 实例，
/// 也没有插件工具注册表（pluginTools 恒空 → beautify/office 不出现）；
/// 全局工具 computer 仅在 Computer Use 启用时出现，端口无该能力 → 隐藏。
/// 结果只取决于路径 id 对应的助手，与焦点无关。
fn settings_available_tools() -> Vec<&'static str> {
    let mut result: Vec<&'static str> = Vec::new();
    for name in CORE_TOOL_NAMES
        .iter()
        .chain(STANDARD_TOOL_NAMES.iter())
        .chain(OPTIONAL_TOOL_NAMES.iter())
    {
        if PLUGIN_BACKED_OPTIONAL_TOOLS.contains(name) || result.contains(name) {
            continue;
        }
        result.push(*name);
    }
    result
}

fn agent_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "agent not found" })),
    )
        .into_response()
}

/// GET /lingxi/v1/agents/{agent_id}/config（`settings.read` 由路由策略层
/// 把关，与 config-flags 读面同级）。目标助手只由路径 id 决定，焦点不参与。
pub async fn agent_config_get_route(
    State(state): State<ServiceState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Response {
    if validate_agent_id(&agent_id).is_err() {
        return agent_not_found();
    }
    let memory = state.memory();
    let Ok(agent_dir) = memory.agent_dir_checked(&agent_id) else {
        return agent_not_found();
    };

    // 先清掉已消失目录的工作区历史，再读（任何入口读到的 config 一致）。
    if let Err(message) = gc_workspace_persistence(memory, &agent_dir) {
        return internal_error(message);
    }

    // 直接读盘解析，不经任何缓存。
    let config = match am::config::read_yaml_mapping_as_json(&agent_dir.join("config.yaml")) {
        Ok(config) => config.unwrap_or_else(|| json!({})),
        Err(err) => return internal_error(format!("cannot read config.yaml: {err}")),
    };
    let Value::Object(mut config) = config else {
        return internal_error("config.yaml must contain a mapping");
    };

    normalize_experience_for_response(&mut config);
    let raw_block = |name: &str| {
        let block = config.get(name);
        json!({
            "provider": or_empty_string(block.and_then(|b| b.get("provider"))),
            "base_url": or_empty_string(block.and_then(|b| b.get("base_url"))),
        })
    };
    let raw = json!({ "api": raw_block("api"), "embedding_api": raw_block("embedding_api") });
    config.insert("_raw".into(), raw);

    let preferences = match memory.read_preferences_root() {
        Ok(preferences) => preferences,
        Err(message) => return internal_error(message),
    };
    inject_global_fields(&mut config, &preferences);

    config.insert("providers".into(), providers_summary(memory.home()));
    config.insert("availableTools".into(), json!(settings_available_tools()));

    Json(mask_object_secrets(&Value::Object(config))).into_response()
}

// ── current_status 只读面（lib/tools/current-status-tool.ts list/get）───────

const STATUS_KEYS: [(&str, &str); 9] = [
    (
        "time",
        "Current real time, configured timezone, local datetime, and UTC offset.",
    ),
    (
        "logical_date",
        "Logical date. The day starts at 04:00 in the configured timezone.",
    ),
    (
        "agent",
        "Current agent identity: stable id and display name.",
    ),
    (
        "model",
        "Current session model when available; otherwise the current selected chat model.",
    ),
    ("session", "Current session pointer and its runtime facts."),
    (
        "session_files",
        "Current session's registered files: file ids, labels, lifecycle metadata, and local paths.",
    ),
    (
        "session_folders",
        "Current session folder scope: cwd, workspace folders, authorized folders.",
    ),
    (
        "bridge_context",
        "Current Bridge platform context. Returns isBridgeSession=false outside Bridge sessions.",
    ),
    (
        "subagents",
        "Open direct subagent instances for the current session.",
    ),
];

fn configured_timezone(memory: &MemoryService) -> Value {
    memory
        .read_preferences_root()
        .ok()
        .and_then(|prefs| {
            prefs
                .get("timezone")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|tz| !tz.is_empty())
                .map(|tz| json!(tz))
        })
        .unwrap_or(Value::Null)
}

fn status_time(memory: &MemoryService) -> Value {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    json!({
        "time": {
            "iso": now
                .to_utc()
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "timezone": configured_timezone(memory),
            "localDateTime": format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                now.year(),
                now.month(),
                now.day(),
                now.hour(),
                now.minute(),
                now.second()
            ),
            "utcOffset": now.format("%:z").to_string(),
        }
    })
}

fn status_logical_date(memory: &MemoryService) -> Value {
    use chrono::{Datelike, Local, Timelike};
    const DAY_BOUNDARY_HOUR: u32 = 4;
    let mut now = Local::now();
    if now.hour() < DAY_BOUNDARY_HOUR {
        now -= chrono::Duration::hours(DAY_BOUNDARY_HOUR as i64);
    }
    json!({
        "logical_date": {
            "date": format!("{:04}-{:02}-{:02}", now.year(), now.month(), now.day()),
            "timezone": configured_timezone(memory),
            "dayBoundaryHour": DAY_BOUNDARY_HOUR,
        }
    })
}

fn status_agent(memory: &MemoryService) -> Value {
    let resolved = focus_agent_id(memory)
        .ok()
        .flatten()
        .map(|id| {
            let name = std::fs::read_to_string(memory.agents_root().join(&id).join("config.yaml"))
                .ok()
                .and_then(|text| {
                    am::config::AgentConfigDocument::parse(&text)
                        .ok()
                        .and_then(|doc| doc.scan().ok())
                        .and_then(|scan| scan.agent_name)
                })
                .unwrap_or_else(|| id.clone());
            json!({ "id": id, "name": name })
        })
        .unwrap_or(json!({ "id": null, "name": null }));
    json!({ "agent": resolved })
}

/// 模型值与运行时一致：焦点 agent config.yaml 的 models.chat 复合键
/// （migration #5 后为 {id, provider}）；读不到逐级回退，最终显式 null
/// （不猜造供应商结果）。config 以成熟 YAML 解析器整读为 JSON 树后取值。
fn status_model(memory: &MemoryService, _session_id: Option<&str>) -> Value {
    let chat = focus_agent_id(memory).ok().flatten().and_then(|id| {
        let config = am::config::read_yaml_mapping_as_json(
            &memory.agents_root().join(&id).join("config.yaml"),
        )
        .ok()
        .flatten()?;
        config.pointer("/models/chat").cloned()
    });
    let resolved = match chat {
        Some(value) => match &value {
            Value::String(id) => json!({ "id": id, "provider": null, "name": id }),
            Value::Object(map) => json!({
                "id": map.get("id").cloned().unwrap_or(Value::Null),
                "provider": map.get("provider").cloned().unwrap_or(Value::Null),
                "name": map.get("name").or_else(|| map.get("id")).cloned().unwrap_or(Value::Null),
            }),
            _ => json!({ "id": null, "provider": null, "name": null }),
        },
        None => json!({ "id": null, "provider": null, "name": null }),
    };
    json!({ "model": resolved })
}

fn current_session_id(memory: &MemoryService) -> Option<String> {
    memory.read_preferences_root().ok().and_then(|prefs| {
        prefs
            .get("current_session_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|id| !id.is_empty())
    })
}

pub async fn current_status_route(
    State(state): State<ServiceState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let action = query.get("action").map(String::as_str).unwrap_or("list");
    let memory = state.memory();
    if action == "list" {
        return Json(json!({
            "available": STATUS_KEYS
                .iter()
                .map(|(key, description)| json!({ "key": key, "description": description }))
                .collect::<Vec<_>>(),
            "usage": "Call action=list to discover available status keys. Then call action=get with the smallest necessary key.",
        }))
        .into_response();
    }
    if action != "get" {
        return bad_request("current_status action must be list or get");
    }
    let Some(key) = query.get("key").map(|k| k.trim()).filter(|k| !k.is_empty()) else {
        return bad_request(
            "current_status get requires a key. Call action=list to see available keys.",
        );
    };
    if !STATUS_KEYS.iter().any(|(known, _)| *known == key) {
        return bad_request(format!(
            "Unknown status key: {key}. Call action=list to see available keys."
        ));
    }
    let session_id = current_session_id(memory);
    let payload = match key {
        "time" => status_time(memory),
        "logical_date" => status_logical_date(memory),
        "agent" => status_agent(memory),
        "model" => status_model(memory, session_id.as_deref()),
        "session" => {
            let session_value = match session_id.as_deref() {
                None => json!({ "sessionId": null, "runtimeAvailable": false }),
                Some(id) => match state.sessions().run_count(id).await {
                    Ok(run_count) => {
                        json!({ "sessionId": id, "runCount": run_count, "runtimeAvailable": true })
                    }
                    Err(err) => {
                        return internal_error(format!("session runtime unavailable: {err}"));
                    }
                },
            };
            json!({ "session": session_value })
        }
        "session_files" => match session_id.as_deref() {
            None => {
                json!({ "session_files": { "sessionId": null, "registryAvailable": true, "files": [] } })
            }
            Some(id) => {
                use lingxi_adapters::storage::session_files::{
                    SessionFileOrigin, SessionFileStatus,
                };
                match lingxi_adapters::storage::session_files::list_session_files_for_owner(
                    state.storage(),
                    id,
                )
                .await
                {
                    Ok(rows) => json!({
                        "session_files": {
                            "sessionId": id,
                            "registryAvailable": true,
                            "files": rows.iter().map(|row| {
                                let writable = matches!(
                                    row.origin,
                                    SessionFileOrigin::AgentWrite | SessionFileOrigin::AgentEdit
                                ) && !row.is_directory
                                    && std::path::Path::new(&row.file_path).is_absolute()
                                    && row.status != SessionFileStatus::Expired;
                                json!({
                                    "fileId": row.file_id,
                                    "sessionFileRef": { "kind": "session-file", "fileId": row.file_id },
                                    "writableLocalRef": if writable {
                                        json!({ "kind": "local-file", "path": row.file_path })
                                    } else {
                                        Value::Null
                                    },
                                    "label": row.label,
                                    "filename": row.filename,
                                    "mime": row.mime,
                                    "size": row.size_bytes,
                                    "origin": row.origin.wire(),
                                    "storageKind": row.storage_kind.wire(),
                                    "status": row.status.wire(),
                                    "createdAt": row.registered_at_ms,
                                    "isDirectory": row.is_directory,
                                })
                            }).collect::<Vec<_>>(),
                        }
                    }),
                    Err(err) => {
                        return internal_error(format!("session file registry unavailable: {err}"));
                    }
                }
            }
        },
        "session_folders" => {
            // 会话文件夹 scope（cwd/工作区/授权目录）由会话运行时持有；端口
            // 服务不跟踪会话 cwd，显式缺席（空列表 + null cwd），不猜造。
            json!({
                "session_folders": {
                    "sessionPath": null,
                    "cwd": null,
                    "workspaceFolders": [],
                    "authorizedFolders": [],
                    "sandboxFolders": [],
                }
            })
        }
        "bridge_context" => {
            // 端口服务没有 Bridge 会话上下文源：非 Bridge 会话的真值形状。
            json!({ "bridge_context": { "isBridgeSession": false } })
        }
        "subagents" => {
            let open = match session_id.as_deref() {
                None => Vec::new(),
                Some(id) => state.subagents().threads_of(id),
            };
            json!({
                "subagents": {
                    "sessionId": session_id,
                    "open": open.iter().map(|thread| json!({
                        "threadId": &thread.thread_id,
                        "label": &thread.label,
                        "status": format!("{:?}", thread.status),
                        "lastRunStatus": &thread.last_run_status,
                        "busy": thread.busy,
                    })).collect::<Vec<_>>(),
                }
            })
        }
        _ => unreachable!("键目录已核对"),
    };
    Json(payload).into_response()
}

// ── experiments（server/routes/experiments.ts + lib/experiments/registry.ts）─

const COMPACTION_MODE_EXPERIMENT_ID: &str = "session.compaction_mode";
const INSTANT_SIMPLE_COMPACTION_EXPERIMENT_ID: &str = "session.instant_simple_compaction";
const DEEPSEEK_ROLEPLAY_EXPERIMENT_ID: &str = "provider.deepseek_roleplay_reasoning_patch";
const PROACTIVE_SUBAGENT_EXPERIMENT_ID: &str = "subagent.proactive_delegation";
const CACHE_SNAPSHOT_EXPERIMENT_ID: &str = "memory.cache_snapshot_reflection";

fn experiment_definitions() -> Vec<Value> {
    vec![
        json!({
            "id": COMPACTION_MODE_EXPERIMENT_ID,
            "titleKey": "settings.experiments.compaction.title",
            "descriptionKey": "settings.experiments.compaction.description",
            "owner": "session",
            "scope": "global",
            "defaultValue": "auto",
            "valueSchema": {
                "type": "enum",
                "presentation": { "type": "select" },
                "options": [
                    { "value": "auto", "labelKey": "settings.experiments.compaction.auto" },
                    { "value": "cache_preserving", "labelKey": "settings.experiments.compaction.cachePreserving" },
                    { "value": "pi_compatible", "labelKey": "settings.experiments.compaction.piCompatible" },
                ],
            },
            "status": "beta",
            "risk": "low",
            "restartPolicy": "immediate",
        }),
        json!({
            "id": INSTANT_SIMPLE_COMPACTION_EXPERIMENT_ID,
            "titleKey": "settings.experiments.instantSimpleCompaction.title",
            "descriptionKey": "settings.experiments.instantSimpleCompaction.description",
            "owner": "session",
            "scope": "global",
            "defaultValue": false,
            "valueSchema": { "type": "boolean", "presentation": { "type": "toggle" } },
            "status": "alpha",
            "risk": "medium",
            "restartPolicy": "immediate",
        }),
        json!({
            "id": DEEPSEEK_ROLEPLAY_EXPERIMENT_ID,
            "titleKey": "settings.experiments.deepseekRoleplay.title",
            "descriptionKey": "settings.experiments.deepseekRoleplay.description",
            "owner": "provider",
            "scope": "global",
            "defaultValue": false,
            "valueSchema": { "type": "boolean", "presentation": { "type": "toggle" } },
            "status": "alpha",
            "risk": "medium",
            "restartPolicy": "new_session",
        }),
        json!({
            "id": PROACTIVE_SUBAGENT_EXPERIMENT_ID,
            "titleKey": "settings.experiments.proactiveSubagent.title",
            "descriptionKey": "settings.experiments.proactiveSubagent.description",
            "owner": "session",
            "scope": "global",
            "defaultValue": false,
            "valueSchema": { "type": "boolean", "presentation": { "type": "toggle" } },
            "status": "beta",
            "risk": "low",
            "restartPolicy": "new_session",
        }),
    ]
}

fn stored_experiment_value(memory: &MemoryService, id: &str) -> Option<Value> {
    memory
        .read_preferences_root()
        .ok()
        .and_then(|prefs| prefs.get("experiments")?.get(id).cloned())
}

/// getResolvedInstantSimpleCompactionEnabled（shared/compaction-mode.ts）。
fn resolved_instant_simple_compaction(memory: &MemoryService) -> Value {
    let stored = stored_experiment_value(memory, INSTANT_SIMPLE_COMPACTION_EXPERIMENT_ID);
    if let Some(stored) = stored {
        if stored.is_boolean() {
            return stored;
        }
        return Value::Bool(false);
    }
    Value::Bool(
        stored_experiment_value(memory, COMPACTION_MODE_EXPERIMENT_ID)
            == Some(json!("lossy_local")),
    )
}

fn resolved_experiment_value(memory: &MemoryService, definition: &Value) -> Value {
    let id = definition["id"].as_str().unwrap_or_default();
    if id == CACHE_SNAPSHOT_EXPERIMENT_ID {
        return json!("off");
    }
    if id == INSTANT_SIMPLE_COMPACTION_EXPERIMENT_ID {
        return resolved_instant_simple_compaction(memory);
    }
    match stored_experiment_value(memory, id) {
        None => definition["defaultValue"].clone(),
        Some(stored) => normalize_experiment_value(definition, &stored)
            .unwrap_or_else(|_| definition["defaultValue"].clone()),
    }
}

/// normalizeExperimentValue（shared/experiments-schema.ts）。
fn normalize_experiment_value(definition: &Value, value: &Value) -> Result<Value, String> {
    let id = definition["id"].as_str().unwrap_or_default();
    let schema_type = definition
        .pointer("/valueSchema/type")
        .and_then(Value::as_str)
        .unwrap_or("boolean");
    match schema_type {
        "boolean" => Ok(Value::Bool(value == &Value::Bool(true))),
        "number" => value
            .as_f64()
            .filter(|n| n.is_finite())
            .map(|n| json!(n))
            .ok_or_else(|| format!("invalid experiment value for {id}")),
        "enum" => {
            let allowed: Vec<&str> = definition
                .pointer("/valueSchema/options")
                .and_then(Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .filter_map(|option| option.get("value").and_then(Value::as_str))
                        .collect()
                })
                .unwrap_or_default();
            value
                .as_str()
                .filter(|v| allowed.contains(v))
                .map(|v| json!(v))
                .ok_or_else(|| format!("invalid experiment value for {id}"))
        }
        _ => Err(format!("invalid experiment value schema for {id}")),
    }
}

pub async fn experiments_list_route(State(state): State<ServiceState>) -> Response {
    let memory = state.memory();
    let experiments: Vec<Value> = experiment_definitions()
        .into_iter()
        .map(|mut definition| {
            let value = resolved_experiment_value(memory, &definition);
            definition
                .as_object_mut()
                .expect("定义为对象")
                .insert("value".into(), value);
            definition
        })
        .collect();
    Json(json!({ "experiments": experiments })).into_response()
}

pub async fn experiment_patch_route(
    State(state): State<ServiceState>,
    AxumPath(experiment_id): AxumPath<String>,
    principal_extension: Option<axum::Extension<crate::auth::Principal>>,
    body: Bytes,
) -> Response {
    let principal = principal_extension.map(|axum::Extension(principal)| principal);
    // denyWithoutScope(c, "settings.write")：实验值写入是设置面。
    if scope_denied(principal.as_ref(), "settings.write") {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "insufficient_scope", "scope": "settings.write" })),
        )
            .into_response();
    }
    let payload = match parse_body_object(&body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let value = payload.get("value").cloned().unwrap_or(Value::Null);
    let memory = state.memory();
    // setExperimentValue（registry.ts:191-206）。
    let normalized = if experiment_id == CACHE_SNAPSHOT_EXPERIMENT_ID {
        json!("off")
    } else {
        let Some(definition) = experiment_definitions()
            .into_iter()
            .find(|definition| definition["id"].as_str() == Some(experiment_id.as_str()))
        else {
            return bad_request(format!("unknown experiment id: {experiment_id}"));
        };
        if definition["scope"].as_str() != Some("global") {
            return bad_request(format!(
                "experiment {experiment_id} requires {} scope",
                definition["scope"].as_str().unwrap_or("agent")
            ));
        }
        match normalize_experiment_value(&definition, &value) {
            Ok(normalized) => normalized,
            Err(message) => return bad_request(message),
        }
    };
    {
        let _guard = crate::agents_admin::preferences_locked();
        let mut preferences = match memory.read_preferences_root() {
            Ok(root) => root,
            Err(message) => return internal_error(message),
        };
        if !preferences.get("experiments").is_some_and(Value::is_object) {
            preferences["experiments"] = json!({});
        }
        preferences
            .as_object_mut()
            .expect("偏好根为对象")
            .get_mut("experiments")
            .and_then(Value::as_object_mut)
            .expect("已确保对象")
            .insert(experiment_id.clone(), normalized.clone());
        if let Err(message) = memory.write_preferences_root(&preferences) {
            return internal_error(message);
        }
    }
    Json(json!({ "ok": true, "value": normalized })).into_response()
}

// ── cache-snapshot observation 读（experiments.ts:53-60 +
//    lib/memory/cache-snapshot-observation.ts）───────────────────────────────

fn observation_relative_path() -> PathBuf {
    PathBuf::from("experiments")
        .join("cache-snapshot-reflection")
        .join("latest.json")
}

fn finite_number(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(0.0)
}

fn normalize_observation(raw: &Value) -> Result<Value, String> {
    if !raw.is_object() {
        return Err("cache snapshot observation must be an object".into());
    }
    let status = match raw.get("status").and_then(Value::as_str) {
        Some("success" | "failed" | "skipped") => {
            raw.get("status").and_then(Value::as_str).unwrap()
        }
        _ => "failed",
    };
    let mode = match raw.get("mode").and_then(Value::as_str) {
        Some("shadow" | "write") => raw.get("mode").and_then(Value::as_str).unwrap(),
        _ => "shadow",
    };
    let created_at = raw
        .get("createdAt")
        .and_then(Value::as_str)
        .filter(|text| chrono::DateTime::parse_from_rfc3339(text).is_ok())
        .map(str::to_string)
        .unwrap_or_else(am::now_iso);
    let diagnostics =
        raw.get("diagnostics")
            .filter(|v| v.is_object())
            .map(|diag| {
                let request_model = diag.get("requestModel").filter(|v| v.is_object()).map(
                    |model| {
                        json!({
                            "id": model.get("id").and_then(Value::as_str).unwrap_or(""),
                            "provider": model.get("provider").and_then(Value::as_str).unwrap_or(""),
                            "api": model.get("api").and_then(Value::as_str).unwrap_or(""),
                            "hasBaseUrl": model.get("hasBaseUrl") == Some(&json!(true)),
                            "hasQuirks": model.get("hasQuirks") == Some(&json!(true)),
                        })
                    },
                );
                json!({
                    "errorName": diag.get("errorName").and_then(Value::as_str).unwrap_or(""),
                    "stack": diag
                        .get("stack")
                        .and_then(Value::as_array)
                        .map(|lines| {
                            lines
                                .iter()
                                .take(4)
                                .map(|line| Value::String(line.as_str().unwrap_or("").to_string()))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    "requestModel": request_model,
                })
            });
    let usage = raw.get("usage");
    Ok(json!({
        "version": 1,
        "agentId": raw.get("agentId").and_then(Value::as_str).unwrap_or(""),
        "sessionPath": raw.get("sessionPath").and_then(Value::as_str).unwrap_or(""),
        "trigger": raw.get("trigger").and_then(Value::as_str).unwrap_or("unknown"),
        "createdAt": created_at,
        "mode": mode,
        "status": status,
        "reason": raw.get("reason").and_then(Value::as_str).unwrap_or(""),
        "usage": {
            "model": usage.and_then(|u| u.get("model")).and_then(Value::as_str).unwrap_or(""),
            "cachedTokens": finite_number(usage.and_then(|u| u.get("cachedTokens"))),
            "missTokens": finite_number(usage.and_then(|u| u.get("missTokens"))),
            "latencyMs": finite_number(usage.and_then(|u| u.get("latencyMs"))),
        },
        "summaryPreview": raw.get("summaryPreview").and_then(Value::as_str).unwrap_or(""),
        "memoryMdPreview": raw.get("memoryMdPreview").and_then(Value::as_str).unwrap_or(""),
        "baseMemoryMdHash": raw.get("baseMemoryMdHash").and_then(Value::as_str).unwrap_or(""),
        "cacheStrategy": match raw.get("cacheStrategy").and_then(Value::as_str) {
            None => "cache_recovery",
            Some("") => "cache_recovery",
            Some(strategy)
                if matches!(
                    strategy,
                    "session_snapshot" | "utility_template" | "cache_recovery"
                ) =>
            {
                strategy
            }
            Some(strategy) => {
                return Err(format!("unknown cache strategy: {strategy}"));
            }
        },
        "strict": raw.get("strict") == Some(&json!(true)),
        "cachePrefixHash": raw.get("cachePrefixHash").and_then(Value::as_str).unwrap_or(""),
        "parentCachePrefixHash": raw.get("parentCachePrefixHash").and_then(Value::as_str).unwrap_or(""),
        "contractDiffs": raw.get("contractDiffs").and_then(Value::as_array).cloned().unwrap_or_default(),
        "degradeReason": raw.get("degradeReason").and_then(Value::as_str).unwrap_or(""),
        "diagnostics": diagnostics,
    }))
}

/// normalizeAgentId（experiments.ts:14-21）：非空、无路径段、非 ./..。
/// 与 agentExists 不同，本读面不要求助手存在（读不到即 observation null）。
#[allow(clippy::result_large_err)]
fn normalize_observation_agent_id(value: Option<&str>) -> Result<String, Response> {
    let id = value.unwrap_or_default().trim();
    if id.is_empty() {
        return Err(bad_request("agentId is required"));
    }
    if id.contains('/') || id.contains('\\') || id == "." || id == ".." {
        return Err(bad_request("agentId is invalid"));
    }
    Ok(id.to_string())
}

pub async fn cache_snapshot_observation_get_route(
    State(state): State<ServiceState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let agent_id = match normalize_observation_agent_id(query.get("agentId").map(String::as_str)) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let path = state
        .memory()
        .agents_root()
        .join(&agent_id)
        .join(observation_relative_path());
    match std::fs::read(&path) {
        Ok(bytes) => {
            let parsed: Value = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(err) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("{err}") })),
                    )
                        .into_response();
                }
            };
            match normalize_observation(&parsed) {
                Ok(observation) => Json(json!({ "observation": observation })).into_response(),
                Err(message) => bad_request(message),
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Json(json!({ "observation": Value::Null })).into_response()
        }
        Err(err) => bad_request(format!("{err}")),
    }
}

/// `DELETE /lingxi/v1/experiments/memory/cache-snapshot-reflection/observation?agentId=`
/// — 现役 experiments.ts:62-69 + cache-snapshot-observation.ts
/// deleteCacheSnapshotObservation：settings.write 门（classify_route 承担）
/// → unlink latest.json → `{ok:true, deleted:true|false}`（ENOENT=false）。
/// 真实 IO 失败响亮 500，不虚报 deleted。
pub async fn cache_snapshot_observation_delete_route(
    State(state): State<ServiceState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let agent_id = match normalize_observation_agent_id(query.get("agentId").map(String::as_str)) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let path = state
        .memory()
        .agents_root()
        .join(&agent_id)
        .join(observation_relative_path());
    match std::fs::remove_file(&path) {
        Ok(()) => Json(json!({ "ok": true, "deleted": true })).into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Json(json!({ "ok": true, "deleted": false })).into_response()
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("delete observation failed: {err}") })),
        )
            .into_response(),
    }
}
