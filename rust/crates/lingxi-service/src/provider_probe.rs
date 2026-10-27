//! 供应商探测的纯协议计划与目录解析；不读取秘密、不创建网络客户端。

use serde_json::{json, Map, Value};

pub const CATALOG_TIMEOUT_MS: u64 = 15_000;
pub const CONNECTION_TIMEOUT_MS: u64 = 10_000;
pub const HEALTH_TIMEOUT_MS: u64 = 15_000;
pub const OLLAMA_DETAIL_LIMIT: usize = 200;
pub const OLLAMA_DETAIL_CONCURRENCY: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbePlanKind {
    Catalog,
    Connection,
    ModelHealth,
}

/// 只包含公开协议参数；授权、凭证、代际和实际发送均由唯一协调器持有。
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderProbePlan {
    ControlPlane {
        method: &'static str,
        url: String,
        body: Option<Value>,
        timeout_ms: u64,
    },
    Generation {
        prompt: &'static str,
        max_tokens: u32,
        timeout_ms: u64,
        purpose: &'static str,
    },
    DeclaredCatalog,
    Skipped {
        reason: &'static str,
    },
    Unavailable {
        reason: &'static str,
    },
}

pub fn build_probe_plan(
    kind: ProbePlanKind,
    provider: &str,
    protocol: &str,
    endpoint: &str,
    _model: Option<&str>,
) -> Result<ProviderProbePlan, String> {
    if matches!(provider, "system-speech" | "volcengine-speech")
        || matches!(
            protocol,
            "system-speech" | "system-speech-recognition" | "volcengine-bigasr"
        )
    {
        return Ok(ProviderProbePlan::Unavailable {
            reason: "此供应商通过语音与本地能力接口验证，不支持通用文本或目录探测",
        });
    }
    if kind == ProbePlanKind::Catalog
        && matches!(
            provider,
            "openai-codex-oauth" | "openai-codex" | "xai-oauth"
        )
    {
        return Ok(ProviderProbePlan::DeclaredCatalog);
    }
    if protocol == "openai-codex-responses" {
        return Ok(ProviderProbePlan::Skipped {
            reason: "Codex Responses 不支持独立健康探测",
        });
    }
    if !matches!(
        protocol,
        "openai-completions" | "openai-responses" | "anthropic-messages" | "google-generative-ai"
    ) {
        return Err(format!("unsupported API protocol: {protocol}"));
    }
    if kind == ProbePlanKind::ModelHealth {
        return Ok(ProviderProbePlan::Generation {
            prompt: "Reply exactly OK.",
            max_tokens: 128,
            timeout_ms: HEALTH_TIMEOUT_MS,
            purpose: "model_health",
        });
    }
    if kind == ProbePlanKind::Connection && protocol == "anthropic-messages" {
        return Ok(ProviderProbePlan::Generation {
            prompt: ".",
            max_tokens: 1,
            timeout_ms: CONNECTION_TIMEOUT_MS,
            purpose: "provider_probe",
        });
    }
    let base = normalize_base_url(provider, endpoint, protocol)?;
    let url = if protocol == "anthropic-messages" {
        append_api_path(&base, "/v1/models?limit=1000")?
    } else {
        format!("{}/models", base.trim_end_matches('/'))
    };
    Ok(ProviderProbePlan::ControlPlane {
        method: "GET",
        url,
        body: None,
        timeout_ms: if kind == ProbePlanKind::Catalog {
            CATALOG_TIMEOUT_MS
        } else {
            CONNECTION_TIMEOUT_MS
        },
    })
}

pub fn append_api_path(base: &str, path: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(base).map_err(|_| "invalid provider base URL".to_string())?;
    validate_public_url(&url)?;
    let base_path = url.path().trim_end_matches('/');
    let (target, query) = path
        .split_once('?')
        .map_or((path, None), |(p, q)| (p, Some(q)));
    let next = if base_path
        .to_ascii_lowercase()
        .ends_with(&target.to_ascii_lowercase())
    {
        base_path.to_owned()
    } else if target.starts_with("/v1/") && base_path.to_ascii_lowercase().ends_with("/v1") {
        format!("{base_path}{}", &target[3..])
    } else {
        format!("{base_path}{target}")
    };
    url.set_path(&next);
    if let Some(query) = query {
        url.set_query(Some(query));
    }
    url.set_fragment(None);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn validate_public_url(url: &reqwest::Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("provider URL requires HTTP(S), a host and no userinfo".into());
    }
    Ok(())
}

pub fn normalize_base_url(
    provider: &str,
    endpoint: &str,
    protocol: &str,
) -> Result<String, String> {
    let mut url = reqwest::Url::parse(endpoint.trim())
        .map_err(|_| "base_url is required and must be a valid URL".to_string())?;
    validate_public_url(&url)?;
    let mut path = url.path().trim_end_matches('/').to_string();
    let suffixes: &[&str] = match protocol {
        "anthropic-messages" => &["/v1/messages", "/v1"],
        "openai-completions" => &["/chat/completions"],
        "openai-responses" | "openai-codex-responses" => &["/responses"],
        _ => &[],
    };
    for suffix in suffixes {
        if path.to_ascii_lowercase().ends_with(suffix) {
            path.truncate(path.len() - suffix.len());
            break;
        }
    }
    if provider == "opencode" {
        for suffix in ["/v1/messages", "/v1"] {
            if path.to_ascii_lowercase().ends_with(suffix) {
                path.truncate(path.len() - suffix.len());
                break;
            }
        }
        if protocol != "anthropic-messages" {
            path.push_str("/v1");
        }
    }
    if provider == "kimi-coding"
        && protocol == "openai-completions"
        && url.host_str() == Some("api.kimi.com")
        && matches!(path.as_str(), "" | "/coding")
    {
        path = "/coding/v1".into();
    }
    if provider == "ollama"
        && protocol == "openai-completions"
        && !path.to_ascii_lowercase().ends_with("/v1")
    {
        path.push_str("/v1");
    }
    if matches!(provider, "minimax" | "minimax-token-plan")
        && protocol == "anthropic-messages"
        && matches!(url.host_str(), Some("api.minimaxi.com" | "api.minimax.io"))
        && matches!(path.as_str(), "" | "/v1")
    {
        path = "/anthropic".into();
    }
    url.set_path(if path.is_empty() { "/" } else { &path });
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogDiscovery {
    pub models: Vec<Value>,
    pub ignored_models: Vec<String>,
}

fn positive_number(value: Option<&Value>) -> Value {
    value
        .filter(|v| v.as_f64().is_some_and(|n| n.is_finite() && n > 0.0))
        .cloned()
        .unwrap_or(Value::Null)
}

fn first_positive(row: &Value, fields: &[&str]) -> Value {
    fields
        .iter()
        .map(|key| positive_number(row.get(*key)))
        .find(|v| !v.is_null())
        .unwrap_or(Value::Null)
}

fn modality(value: Option<&Value>) -> Option<Value> {
    let values = value?.as_array()?;
    let out: Vec<Value> = ["text", "image", "video", "audio"]
        .iter()
        .filter(|name| values.iter().any(|v| v.as_str() == Some(**name)))
        .map(|name| json!(name))
        .collect();
    (!out.is_empty()).then_some(Value::Array(out))
}

pub fn filter_discovered_models(
    provider: &str,
    endpoint: &str,
    models: Vec<Value>,
) -> CatalogDiscovery {
    let official_deepseek = provider.eq_ignore_ascii_case("deepseek")
        || reqwest::Url::parse(endpoint)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .is_some_and(|host| host.eq_ignore_ascii_case("api.deepseek.com"));
    let mut accepted = Vec::new();
    let mut ignored = Vec::new();
    for model in models {
        let Some(id) = model
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
        else {
            continue;
        };
        if official_deepseek && id.trim().eq_ignore_ascii_case("deepseek") {
            if !ignored.iter().any(|known| known == id) {
                ignored.push(id.to_owned());
            }
        } else {
            accepted.push(model);
        }
    }
    CatalogDiscovery {
        models: accepted,
        ignored_models: ignored,
    }
}

pub fn parse_catalog(
    provider: &str,
    protocol: &str,
    endpoint: &str,
    payload: &Value,
) -> Result<CatalogDiscovery, String> {
    let field = if protocol == "google-generative-ai" {
        "models"
    } else {
        "data"
    };
    let rows = payload
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("remote model catalog has no {field} array"))?;
    let mut models = Vec::new();
    for row in rows {
        let id = if protocol == "google-generative-ai" {
            row.get("baseModelId").and_then(Value::as_str).or_else(|| {
                row.get("name")
                    .and_then(Value::as_str)
                    .map(|s| s.strip_prefix("models/").unwrap_or(s))
            })
        } else {
            row.get("id").and_then(Value::as_str)
        };
        let Some(id) = id.filter(|id| !id.trim().is_empty()) else {
            continue;
        };
        let (name, context, max_output) = match protocol {
            "anthropic-messages" => (
                row.get("display_name")
                    .and_then(Value::as_str)
                    .unwrap_or(id),
                first_positive(row, &["max_input_tokens"]),
                first_positive(row, &["max_tokens"]),
            ),
            "google-generative-ai" => (
                row.get("displayName").and_then(Value::as_str).unwrap_or(id),
                first_positive(row, &["inputTokenLimit"]),
                first_positive(row, &["outputTokenLimit"]),
            ),
            _ => (
                id,
                first_positive(
                    row,
                    &["context_length", "context_window", "max_context_length"],
                ),
                first_positive(row, &["max_completion_tokens", "max_output_tokens"]),
            ),
        };
        let mut model = json!({"id":id,"name":name,"context":context,"maxOutput":max_output});
        for (key, alternatives) in [
            ("inputs", &["inputs", "input_modalities"][..]),
            ("outputs", &["outputs", "output_modalities"][..]),
        ] {
            let value = alternatives
                .iter()
                .find_map(|key| row.get(*key))
                .or_else(|| {
                    row.get("architecture").and_then(|a| {
                        a.get(if key == "inputs" {
                            "input_modalities"
                        } else {
                            "output_modalities"
                        })
                    })
                });
            if let Some(value) = modality(value) {
                model[key] = value;
            }
        }
        if let Some(kind) = row.get("type").and_then(Value::as_str).filter(|kind| {
            matches!(
                *kind,
                "chat"
                    | "image"
                    | "video"
                    | "audio"
                    | "speech"
                    | "tts"
                    | "asr"
                    | "transcription"
                    | "speech_generation"
                    | "speech_recognition"
            )
        }) {
            model["type"] = json!(kind);
        }
        models.push(model);
    }
    Ok(filter_discovered_models(provider, endpoint, models))
}

pub fn build_ollama_detail_plan(endpoint: &str, model: &str) -> Result<ProviderProbePlan, String> {
    let base = normalize_base_url("ollama", endpoint, "openai-completions")?;
    let root = base.strip_suffix("/v1").unwrap_or(&base);
    Ok(ProviderProbePlan::ControlPlane {
        method: "POST",
        url: format!("{root}/api/show"),
        body: Some(json!({"model":model})),
        timeout_ms: CATALOG_TIMEOUT_MS,
    })
}

pub fn merge_ollama_details(model: &mut Value, detail: &Value) {
    let context = detail
        .get("model_info")
        .and_then(Value::as_object)
        .and_then(|map| {
            map.iter().find_map(|(key, value)| {
                let candidate = if key == "context_length" || key.ends_with(".context_length") {
                    Some(value)
                } else {
                    value.get("context_length")
                };
                candidate.and_then(Value::as_u64).filter(|n| *n > 0)
            })
        });
    if let Some(context) = context {
        model["context"] = json!(context);
    }
    if let Some(capabilities) = detail.get("capabilities").and_then(Value::as_array) {
        if model.get("toolUse").is_none()
            && capabilities
                .iter()
                .any(|c| c.as_str().is_some_and(|c| c.eq_ignore_ascii_case("tools")))
        {
            model["toolUse"] =
                json!({"supportsTools":true,"dialect":"openai","toolResultFormat":"message"});
        }
        if model.get("reasoning").is_none()
            && capabilities.iter().any(|c| {
                c.as_str()
                    .is_some_and(|c| c.eq_ignore_ascii_case("thinking"))
            })
        {
            model["reasoning"] = json!(true);
        }
    }
    // 名称推断仅兼容旧目录展示，不替代明确保存后的模型能力契约。
    if model.get("image").is_none() && model.get("vision").is_none() {
        let id = model["id"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let patterns = [
            r"(^|[\s/_.:-])(?:llava|bakllava)($|[\s/_.:-])",
            r"(^|[\s/_.:-])minicpm[-_.]?v($|[\s/_.:-]|\d)",
            r"(^|[\s/_.:-])moondream($|[\s/_.:-]|\d)",
            r"(^|[\s/_.:-])llama(?:3(?:\.2|p2)?|v3p2)?[\w_.:-]*vision($|[\s/_.:-])",
            r"(^|[\s/_.:-])phi[\w_.:-]*vision($|[\s/_.:-])",
            r"(^|[\s/_.:-])granite[\w_.:-]*vision($|[\s/_.:-])",
            r"(^|[\s/_.:-])qwen[\w_.:-]*(?:vl|vision)($|[\s/_.:-]|\d)",
            r"(^|[\s/_.:-])gemma[3-9]($|[\s/_.:-])",
        ];
        let looks_visual = patterns.iter().any(|pattern| {
            regex::Regex::new(pattern)
                .expect("固定的旧目录模式有效")
                .is_match(&id)
        });
        if looks_visual {
            model["image"] = json!(true);
        }
    }
}

/// 这里只解释协议响应；秘密清洗在协调器持有全部敏感材料的出口进行。
pub fn parse_connection_response(status: u16, content_type: &str, bytes: &[u8]) -> Value {
    if (200..300).contains(&status) {
        return json!({"ok":true,"status":status});
    }
    let fallback = format!("HTTP {status}");
    let message = if content_type.contains("application/json") {
        serde_json::from_slice::<Value>(bytes).ok().and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("error"))
                .or_else(|| v.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
    } else if content_type.starts_with("text/") && !content_type.contains("html") {
        Some(String::from_utf8_lossy(bytes).into_owned())
    } else {
        None
    };
    let message = message
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&fallback);
    json!({"ok":false,"status":status,"error":message.chars().take(500).collect::<String>()})
}

/// 字段级白名单归一化，不修改原模型或解析任何凭证。
pub fn normalize_model_patch(patch: &Value) -> Result<Value, String> {
    let source = patch
        .as_object()
        .ok_or_else(|| "model metadata must be an object".to_string())?;
    for key in source.keys() {
        if matches!(
            key.to_ascii_lowercase().as_str(),
            "api_key"
                | "apikey"
                | "token"
                | "access"
                | "accesstoken"
                | "access_token"
                | "refresh"
                | "refresh_token"
                | "refreshtoken"
                | "accountid"
                | "account_id"
                | "authorization"
                | "cookie"
                | "headers"
                | "resourceurl"
                | "resource_url"
                | "expires"
        ) {
            return Err(format!("模型条目不能保存凭证字段：{key}"));
        }
    }
    let mut out = Map::new();
    for key in [
        "name",
        "api",
        "context",
        "maxOutput",
        "outputIncludesThinking",
        "image",
        "video",
        "audio",
        "reasoning",
        "xhigh",
        "thinkingLevels",
        "thinkingLevelMap",
        "type",
        "defaultThinkingLevel",
        "web",
        "structuredOutput",
        "operations",
        "operationProtocol",
        "dimensions",
        "groupId",
        "compat",
        "toolUse",
        "visionCapabilities",
    ] {
        if let Some(value) = source.get(key) {
            out.insert(key.to_owned(), value.clone());
        }
    }
    for (old, new) in [
        ("contextWindow", "context"),
        ("maxTokens", "maxOutput"),
        ("maxOutputTokens", "maxOutput"),
    ] {
        if !out.contains_key(new) {
            if let Some(value) = source.get(old) {
                out.insert(new.into(), value.clone());
            }
        }
    }
    if !source.contains_key("inputs") && !source.contains_key("image") {
        if let Some(value) = source.get("vision") {
            out.insert("image".into(), value.clone());
        }
    }
    for key in ["inputs", "outputs"] {
        if let Some(value) = source.get(key) {
            let Some(values) = value.as_array() else {
                return Err(format!("{key} must be a non-empty modality array"));
            };
            if values.is_empty()
                || values
                    .iter()
                    .any(|v| !matches!(v.as_str(), Some("text" | "image" | "video" | "audio")))
            {
                return Err(format!("invalid {key} modality"));
            }
            out.insert(key.into(), modality(Some(value)).expect("已验证非空模态"));
        }
    }
    if out.contains_key("inputs") {
        for key in ["image", "video", "audio"] {
            out.remove(key);
        }
    }
    for key in ["context", "maxOutput", "dimensions"] {
        if let Some(value) = out.get(key) {
            if value.as_f64().is_none_or(|n| !n.is_finite() || n <= 0.0) {
                return Err(format!("{key} must be positive and finite"));
            }
        }
    }
    for key in [
        "image",
        "video",
        "audio",
        "reasoning",
        "xhigh",
        "web",
        "structuredOutput",
    ] {
        if out.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(format!("{key} must be boolean"));
        }
    }
    if out
        .get("outputIncludesThinking")
        .is_some_and(|v| !v.is_null() && !v.is_boolean())
    {
        return Err("outputIncludesThinking must be boolean or null".into());
    }
    Ok(Value::Object(out))
}

/// 固定产品声明；与 core/provider-registry.ts 的39个内置插件保持一一对应。
pub fn builtin_provider_definitions() -> Vec<Value> {
    let rows: &[(&str, &str, &str, &str, &str)] = &[
        (
            "agnes",
            "Agnes AI",
            "api-key",
            "openai-completions",
            "https://apihub.agnes-ai.com/v1",
        ),
        (
            "dashscope",
            "阿里云百炼 (DashScope)",
            "api-key",
            "openai-completions",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
        ),
        (
            "openai",
            "OpenAI",
            "api-key",
            "openai-completions",
            "https://api.openai.com/v1",
        ),
        (
            "anthropic",
            "Anthropic",
            "api-key",
            "anthropic-messages",
            "https://api.anthropic.com",
        ),
        (
            "deepseek",
            "DeepSeek",
            "api-key",
            "openai-completions",
            "https://api.deepseek.com",
        ),
        (
            "deepseek-responses",
            "DeepSeek (Responses)",
            "api-key",
            "openai-responses",
            "https://api.deepseek.com",
        ),
        (
            "gemini",
            "Google Gemini",
            "api-key",
            "google-generative-ai",
            "https://generativelanguage.googleapis.com/v1beta",
        ),
        (
            "openrouter",
            "OpenRouter",
            "api-key",
            "openai-completions",
            "https://openrouter.ai/api/v1",
        ),
        (
            "opencode",
            "OpenCode Zen",
            "api-key",
            "anthropic-messages",
            "https://opencode.ai/zen",
        ),
        (
            "opencode-go",
            "OpenCode Go",
            "api-key",
            "openai-completions",
            "https://opencode.ai/zen/go/v1",
        ),
        (
            "ollama",
            "Ollama (本地)",
            "none",
            "openai-completions",
            "http://localhost:11434/v1",
        ),
        (
            "minimax",
            "MiniMax",
            "api-key",
            "anthropic-messages",
            "https://api.minimaxi.com/anthropic",
        ),
        (
            "minimax-token-plan",
            "MiniMax Token Plan",
            "api-key",
            "anthropic-messages",
            "https://api.minimaxi.com/anthropic",
        ),
        (
            "openai-codex-oauth",
            "OpenAI Codex (OAuth)",
            "oauth",
            "openai-codex-responses",
            "https://chatgpt.com/backend-api",
        ),
        (
            "siliconflow",
            "SiliconFlow (硅基流动)",
            "api-key",
            "openai-completions",
            "https://api.siliconflow.cn/v1",
        ),
        (
            "zhipu",
            "智谱 AI (GLM)",
            "api-key",
            "openai-completions",
            "https://open.bigmodel.cn/api/paas/v4",
        ),
        (
            "moonshot",
            "Moonshot (Kimi)",
            "api-key",
            "openai-completions",
            "https://api.moonshot.cn/v1",
        ),
        (
            "baichuan",
            "百川智能",
            "api-key",
            "openai-completions",
            "https://api.baichuan-ai.com/v1",
        ),
        (
            "stepfun",
            "阶跃星辰 (StepFun)",
            "api-key",
            "openai-completions",
            "https://api.stepfun.com/v1",
        ),
        (
            "volcengine",
            "火山引擎 (豆包)",
            "api-key",
            "openai-completions",
            "https://ark.cn-beijing.volces.com/api/v3",
        ),
        (
            "volcengine-speech",
            "火山引擎语音 (BigASR)",
            "api-key",
            "volcengine-bigasr",
            "https://openspeech.bytedance.com",
        ),
        (
            "hunyuan",
            "腾讯混元",
            "api-key",
            "openai-completions",
            "https://api.hunyuan.cloud.tencent.com/v1",
        ),
        (
            "baidu-cloud",
            "百度智能云 (文心)",
            "api-key",
            "openai-completions",
            "https://qianfan.baidubce.com/v2",
        ),
        (
            "modelscope",
            "魔搭 (ModelScope)",
            "api-key",
            "openai-completions",
            "https://api-inference.modelscope.cn/v1",
        ),
        (
            "infini",
            "无问芯穹 (Infini)",
            "api-key",
            "openai-completions",
            "https://cloud.infini-ai.com/maas/v1",
        ),
        (
            "mimo",
            "Xiaomi (MiMo)",
            "api-key",
            "openai-completions",
            "https://api.xiaomimimo.com/v1",
        ),
        (
            "mimo-token-plan",
            "Xiaomi MiMo Token Plan",
            "api-key",
            "openai-completions",
            "https://token-plan-cn.xiaomimimo.com/v1",
        ),
        ("system-speech", "系统语音识别", "none", "system-speech", ""),
        (
            "groq",
            "Groq",
            "api-key",
            "openai-completions",
            "https://api.groq.com/openai/v1",
        ),
        (
            "together",
            "Together AI",
            "api-key",
            "openai-completions",
            "https://api.together.xyz/v1",
        ),
        (
            "fireworks",
            "Fireworks AI",
            "api-key",
            "openai-completions",
            "https://api.fireworks.ai/inference/v1",
        ),
        (
            "mistral",
            "Mistral AI",
            "api-key",
            "openai-completions",
            "https://api.mistral.ai/v1",
        ),
        (
            "perplexity",
            "Perplexity",
            "api-key",
            "openai-completions",
            "https://api.perplexity.ai",
        ),
        (
            "xai",
            "xAI (Grok)",
            "api-key",
            "openai-completions",
            "https://api.x.ai/v1",
        ),
        (
            "xai-oauth",
            "xAI Grok (OAuth)",
            "oauth",
            "openai-responses",
            "https://cli-chat-proxy.grok.com/v1",
        ),
        (
            "dashscope-coding",
            "百炼 Coding Plan",
            "api-key",
            "openai-completions",
            "https://coding.dashscope.aliyuncs.com/v1",
        ),
        (
            "kimi-coding",
            "Kimi Coding Plan",
            "api-key",
            "openai-completions",
            "https://api.kimi.com/coding/v1",
        ),
        (
            "volcengine-coding",
            "火山引擎 Coding Plan",
            "api-key",
            "openai-completions",
            "https://ark.cn-beijing.volces.com/api/coding/v3",
        ),
        (
            "zhipu-coding",
            "智谱 GLM Coding Plan",
            "api-key",
            "openai-completions",
            "https://api.z.ai/api/coding/paas/v4",
        ),
    ];
    rows.iter().map(|(id,name,auth,protocol,endpoint)| {
        let models = match *id {
            "openai-codex-oauth" => json!(["gpt-5.6-sol","gpt-5.6-terra","gpt-5.6-luna","gpt-5.5","gpt-5.4","gpt-5.4-mini","gpt-5.2"]),
            "xai-oauth" => json!([
                {"id":"grok-4.5","name":"Grok 4.5","api":"openai-responses","context":500000,"maxOutput":128000,"image":true,"reasoning":true},
                {"id":"grok-4.5-latest","name":"Grok 4.5 Latest","api":"openai-responses","context":500000,"maxOutput":128000,"image":true,"reasoning":true},
                {"id":"grok-build-latest","name":"Grok Build Latest","api":"openai-responses","context":500000,"maxOutput":128000,"image":true,"reasoning":true},
                {"id":"grok-4.3","name":"Grok 4.3","api":"openai-responses","context":1000000,"maxOutput":128000,"image":true,"reasoning":true}
            ]),
            _ => json!([]),
        };
        json!({"id":id,"displayName":name,"authType":auth,"api":protocol,"baseUrl":endpoint,"models":models,
            "runtimeProviderId":if *id == "openai-codex-oauth" {"openai-codex"} else {id},
            "authJsonKey":if *id == "openai-codex-oauth" {"openai-codex"} else {id}})
    }).collect()
}

pub fn builtin_provider_definition(id: &str) -> Option<Value> {
    let id = if id == "openai-codex" {
        "openai-codex-oauth"
    } else {
        id
    };
    builtin_provider_definitions()
        .into_iter()
        .find(|value| value["id"] == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_remote_data_cannot_replace_protocol_endpoint_or_credentials() {
        let found=parse_catalog("custom","openai-completions","http://localhost/v1",&json!({"data":[{"id":"m","api":"evil","endpoint":"https://evil.invalid","api_key":"must-not-escape","headers":{"Authorization":"secret"},"inputs":["image","text"],"context_length":4096}]})).unwrap();
        assert_eq!(found.models,json!([{"id":"m","name":"m","context":4096,"maxOutput":null,"inputs":["text","image"]}]).as_array().unwrap().clone());
        assert!(parse_catalog(
            "p",
            "google-generative-ai",
            "http://localhost",
            &json!({"data":[]})
        )
        .is_err());
        let filtered = parse_catalog(
            "deepseek",
            "openai-completions",
            "http://localhost",
            &json!({"data":[{"id":"deepseek"},{"id":"deepseek-v4-flash"}]}),
        )
        .unwrap();
        assert_eq!(filtered.ignored_models, vec!["deepseek"]);
        assert_eq!(filtered.models.len(), 1);
    }

    #[test]
    fn probes_distinguish_control_plane_generation_and_unavailable_lanes() {
        assert!(matches!(
            build_probe_plan(
                ProbePlanKind::Connection,
                "p",
                "anthropic-messages",
                "http://localhost",
                None
            )
            .unwrap(),
            ProviderProbePlan::Generation {
                prompt: ".",
                max_tokens: 1,
                timeout_ms: 10_000,
                ..
            }
        ));
        assert!(matches!(
            build_probe_plan(
                ProbePlanKind::ModelHealth,
                "p",
                "openai-completions",
                "http://localhost",
                Some("m")
            )
            .unwrap(),
            ProviderProbePlan::Generation {
                max_tokens: 128,
                timeout_ms: 15_000,
                ..
            }
        ));
        assert!(matches!(
            build_probe_plan(
                ProbePlanKind::Connection,
                "p",
                "openai-completions",
                "http://localhost/v1",
                None
            )
            .unwrap(),
            ProviderProbePlan::ControlPlane {
                method: "GET",
                timeout_ms: 10_000,
                ..
            }
        ));
        assert!(matches!(
            build_probe_plan(
                ProbePlanKind::Catalog,
                "openai-codex-oauth",
                "openai-codex-responses",
                "https://chatgpt.com/backend-api",
                None
            )
            .unwrap(),
            ProviderProbePlan::DeclaredCatalog
        ));
        assert!(matches!(
            build_probe_plan(
                ProbePlanKind::ModelHealth,
                "openai-codex-oauth",
                "openai-codex-responses",
                "https://chatgpt.com/backend-api",
                None
            )
            .unwrap(),
            ProviderProbePlan::Skipped { .. }
        ));
        for (provider, protocol) in [
            ("system-speech", "system-speech"),
            ("volcengine-speech", "volcengine-bigasr"),
        ] {
            assert!(matches!(
                build_probe_plan(ProbePlanKind::ModelHealth, provider, protocol, "", None).unwrap(),
                ProviderProbePlan::Unavailable { .. }
            ));
        }
    }

    #[test]
    fn endpoint_normalization_preserves_custom_origins_and_avoids_duplicate_v1() {
        assert_eq!(
            append_api_path("https://fixture.invalid/v1", "/v1/models?limit=1000").unwrap(),
            "https://fixture.invalid/v1/models?limit=1000"
        );
        assert_eq!(
            normalize_base_url(
                "minimax",
                "https://custom.invalid/v1/messages",
                "anthropic-messages"
            )
            .unwrap(),
            "https://custom.invalid"
        );
        assert_eq!(
            normalize_base_url(
                "minimax",
                "https://api.minimaxi.com/v1",
                "anthropic-messages"
            )
            .unwrap(),
            "https://api.minimaxi.com/anthropic"
        );
        assert_eq!(
            normalize_base_url("ollama", "http://localhost:11434", "openai-completions").unwrap(),
            "http://localhost:11434/v1"
        );
        assert!(normalize_base_url(
            "p",
            "https://user:secret@fixture.invalid",
            "openai-completions"
        )
        .is_err());
    }

    #[test]
    fn all_39_definitions_keep_declared_identity_protocol_and_keyless_boundaries() {
        let definitions = builtin_provider_definitions();
        assert_eq!(definitions.len(), 39);
        let ids: std::collections::BTreeSet<_> = definitions
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 39);
        assert_eq!(
            definitions
                .iter()
                .filter(|p| p["authType"] == "oauth")
                .count(),
            2
        );
        assert_eq!(
            definitions
                .iter()
                .filter(|p| p["authType"] == "none")
                .count(),
            2
        );
        for p in &definitions {
            assert!(lingxi_kernel::model_exchange::ProtocolFamily::parse(
                p["api"].as_str().unwrap()
            )
            .is_some());
        }
        assert_eq!(
            builtin_provider_definition("openai-codex").unwrap()["id"],
            "openai-codex-oauth"
        );
        assert_eq!(
            builtin_provider_definition("volcengine-speech").unwrap()["api"],
            "volcengine-bigasr"
        );
        assert_eq!(
            builtin_provider_definition("system-speech").unwrap()["baseUrl"],
            ""
        );
    }

    #[test]
    fn declared_catalog_matrix_uses_each_protocol_and_exact_probe_budget() {
        for definition in builtin_provider_definitions() {
            let provider = definition["id"].as_str().unwrap();
            let protocol = definition["api"].as_str().unwrap();
            let endpoint = definition["baseUrl"].as_str().unwrap();
            for kind in [
                ProbePlanKind::Catalog,
                ProbePlanKind::Connection,
                ProbePlanKind::ModelHealth,
            ] {
                let plan =
                    build_probe_plan(kind, provider, protocol, endpoint, Some("concrete-model"))
                        .unwrap_or_else(|e| panic!("{provider} {kind:?}: {e}"));
                match plan {
                    ProviderProbePlan::Unavailable { .. } => {
                        assert!(matches!(provider, "system-speech" | "volcengine-speech"))
                    }
                    ProviderProbePlan::DeclaredCatalog => assert!(
                        kind == ProbePlanKind::Catalog
                            && matches!(provider, "openai-codex-oauth" | "xai-oauth")
                    ),
                    ProviderProbePlan::Skipped { .. } => {
                        assert_eq!(protocol, "openai-codex-responses")
                    }
                    ProviderProbePlan::Generation {
                        timeout_ms,
                        max_tokens,
                        prompt,
                        ..
                    } => {
                        if kind == ProbePlanKind::Connection {
                            assert_eq!(protocol, "anthropic-messages");
                            assert_eq!((timeout_ms, max_tokens, prompt), (10_000, 1, "."));
                        } else {
                            assert_eq!(kind, ProbePlanKind::ModelHealth);
                            assert_eq!(
                                (timeout_ms, max_tokens, prompt),
                                (15_000, 128, "Reply exactly OK.")
                            );
                        }
                    }
                    ProviderProbePlan::ControlPlane {
                        timeout_ms, url, ..
                    } => {
                        assert!(kind != ProbePlanKind::ModelHealth);
                        assert_eq!(
                            timeout_ms,
                            if kind == ProbePlanKind::Connection {
                                10_000
                            } else {
                                15_000
                            }
                        );
                        assert!(url.contains("/models"));
                    }
                }
            }
        }
    }

    #[test]
    fn ollama_name_patterns_match_legacy_boundaries_and_preserve_overrides() {
        for id in [
            "llava:latest",
            "moondream2",
            "minicpm_v2",
            "gemma9:latest",
            "qwen2.5-vl:7b",
            "llama3.2-vision",
        ] {
            let mut model = json!({"id":id});
            merge_ollama_details(&mut model, &json!({}));
            assert_eq!(model["image"], true, "{id}");
        }
        for id in ["llavaland", "gemma30", "qwen3", "ordinary"] {
            let mut model = json!({"id":id});
            merge_ollama_details(&mut model, &json!({}));
            assert!(model.get("image").is_none(), "{id}");
        }
        let mut model = json!({"id":"llava:latest","image":false,"reasoning":false});
        merge_ollama_details(&mut model, &json!({"capabilities":["thinking","tools"]}));
        assert_eq!(model["image"], false);
        assert_eq!(model["reasoning"], false);
        assert_eq!(model["toolUse"]["supportsTools"], true);
    }

    #[test]
    fn model_metadata_normalizes_aliases_and_explicit_modality_without_overwriting_siblings() {
        let patch=normalize_model_patch(&json!({"contextWindow":8192,"maxTokens":128,"api":"anthropic-messages","inputs":["image","text","image"],"image":false,"vision":false,"outputIncludesThinking":null})).unwrap();
        assert_eq!(patch["context"], 8192);
        assert_eq!(patch["maxOutput"], 128);
        assert_eq!(patch["api"], "anthropic-messages");
        assert_eq!(patch["inputs"], json!(["text", "image"]));
        assert!(patch.get("image").is_none());
        assert!(normalize_model_patch(&json!({"inputs":[]})).is_err());
        assert!(normalize_model_patch(&json!({"maxOutput":0})).is_err());
        assert!(normalize_model_patch(&json!({"api_key":"forbidden"})).is_err());
        assert!(normalize_model_patch(&json!({"headers":{"Authorization":"forbidden"}})).is_err());
        let error = parse_connection_response(502, "text/html", b"<html>upstream outage</html>");
        assert_eq!(error["error"], "HTTP 502");
    }
}
