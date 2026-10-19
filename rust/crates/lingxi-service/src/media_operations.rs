//! 媒体业务只冻结并恢复现役 operation 路由，不另建凭据或网络出口。
use super::{
    MediaTaskKind, MediaTaskRecord, OperationCallContext, OperationFailure, OperationService,
    MEDIA_TASK_OWNER,
};
use lingxi_adapters::models::credentials::{sanitize_diagnostic, ApplicableAuth};
use lingxi_kernel::model_exchange::{
    CredentialAuthKind, CredentialReference, MediaGenerationKind, ModelGatewayPort, ModelOperation,
    ModelRouteRequest, ProtocolFamily, ResolvedModelRoute,
};
use lingxi_protocol::ErrorCode;
use serde_json::{json, Value};
use std::{
    future::Future,
    sync::{Arc, Mutex},
};

#[path = "media_metadata.rs"]
mod metadata;

tokio::task_local! {
    static FROZEN_MEDIA_ROUTE:MediaRouteSnapshot;
    static MEDIA_PROGRESS:crate::media::MediaProgress;
    static MEDIA_PRINCIPAL:lingxi_kernel::Principal;
    static MEDIA_MATERIALS:Arc<Mutex<Vec<String>>>;
}
#[derive(Debug, Clone)]
pub struct MediaRouteSnapshot {
    pub(crate) route: ResolvedModelRoute,
}
fn failure(message: impl Into<String>) -> OperationFailure {
    OperationFailure {
        code: ErrorCode::InvalidMessage,
        message: message.into(),
        retryable: false,
    }
}
fn kind_operation(kind: &str) -> Result<ModelOperation, OperationFailure> {
    Ok(ModelOperation::MediaGeneration {
        kind: match kind {
            "image" => MediaGenerationKind::Image,
            "video" => MediaGenerationKind::Video,
            "speech" => MediaGenerationKind::Speech,
            _ => return Err(failure("invalid media kind")),
        },
    })
}
impl MediaRouteSnapshot {
    pub fn provider(&self) -> &str {
        &self.route.provider
    }
    pub fn to_value(&self) -> Value {
        let kind = match self.route.operation {
            ModelOperation::MediaGeneration {
                kind: MediaGenerationKind::Image,
            } => "image",
            ModelOperation::MediaGeneration {
                kind: MediaGenerationKind::Video,
            } => "video",
            ModelOperation::MediaGeneration {
                kind: MediaGenerationKind::Speech,
            } => "speech",
            _ => "invalid",
        };
        let auth = match self.route.credential.auth {
            CredentialAuthKind::ApiKey => "apiKey",
            CredentialAuthKind::AuthHeader => "authHeader",
            CredentialAuthKind::OAuth => "oauth",
            CredentialAuthKind::None => "none",
        };
        json!({"version":1,"kind":kind,"provider":self.route.provider,"model":self.route.model,"protocol":self.route.protocol.config_name(),"endpoint":self.route.endpoint,"credentialAuth":auth,"generation":self.route.config_generation,"groupId":self.route.group_id})
    }
    pub fn from_value(value: &Value) -> Result<Self, OperationFailure> {
        let map = value
            .as_object()
            .ok_or_else(|| failure("media route damaged"))?;
        if map.len() != 9 || value["version"] != 1 {
            return Err(failure("media route version or fields invalid"));
        }
        let required = |key: &str| {
            value[key]
                .as_str()
                .filter(|v| !v.is_empty() && v.len() <= 8192)
                .map(String::from)
                .ok_or_else(|| failure("media route field damaged"))
        };
        let provider = required("provider")?;
        let operation = kind_operation(&required("kind")?)?;
        let protocol = ProtocolFamily::parse(&required("protocol")?)
            .ok_or_else(|| failure("media protocol unavailable"))?;
        if !protocol.serves(operation.operation_class()) {
            return Err(failure("media protocol does not serve operation"));
        }
        let endpoint = required("endpoint")?;
        let url = reqwest::Url::parse(&endpoint).map_err(|_| failure("media endpoint damaged"))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host_str().is_none()
        {
            return Err(failure("media endpoint invalid"));
        }
        let auth = match required("credentialAuth")?.as_str() {
            "apiKey" => CredentialAuthKind::ApiKey,
            "authHeader" => CredentialAuthKind::AuthHeader,
            "oauth" => CredentialAuthKind::OAuth,
            "none" => CredentialAuthKind::None,
            _ => return Err(failure("media credential reference damaged")),
        };
        let group_id = if value["groupId"].is_null() {
            None
        } else {
            Some(required("groupId")?)
        };
        Ok(Self {
            route: ResolvedModelRoute {
                provider: provider.clone(),
                model: required("model")?,
                operation,
                protocol,
                endpoint,
                credential: CredentialReference { provider, auth },
                config_generation: value["generation"]
                    .as_u64()
                    .filter(|v| *v > 0)
                    .ok_or_else(|| failure("media generation damaged"))?,
                group_id,
            },
        })
    }
}
impl OperationService {
    pub(crate) async fn settle_media_run(&self, execution: &str) -> Result<(), String> {
        let Some(supervisor) = self.process_supervisor.as_ref() else {
            return Ok(());
        };
        for id in supervisor.live_handles() {
            if supervisor
                .record(&id)
                .is_some_and(|record| record.owner.run_id == execution)
            {
                let receipt = supervisor
                    .terminate(&id, crate::procsupervisor::TerminationReason::CallerDropped)
                    .await;
                if matches!(
                    receipt.outcome,
                    crate::procsupervisor::TerminationOutcome::CleanupTimedOut
                ) {
                    return Err("媒体执行进程清理尚未确认".into());
                }
            }
        }
        Ok(())
    }
    pub fn media_route(
        &self,
        kind: &str,
        provider: Option<String>,
        mut model: Option<String>,
    ) -> Result<MediaRouteSnapshot, OperationFailure> {
        if let (Some(provider), Some(selected)) = (provider.as_ref(), model.as_ref()) {
            let catalog = self.gateway.catalog_models();
            let models = catalog
                .get(provider)
                .ok_or_else(|| failure("媒体供应商未登记"))?;
            if !models.iter().any(|entry| entry.id == *selected) {
                // 静态别名只规范同一供应商 Source 内的真实键，不能授予额外模型。
                let protocol = self
                    .gateway
                    .configured_provider_views()
                    .into_iter()
                    .find(|(id, _, _)| id == provider)
                    .map(|(_, protocol, _)| protocol)
                    .ok_or_else(|| failure("媒体供应商未登记"))?;
                let known = metadata_models(&protocol, kind)?;
                let canonical = known
                    .iter()
                    .find(|entry| {
                        entry["aliases"].as_array().is_some_and(|aliases| {
                            aliases.iter().any(|alias| alias == selected.as_str())
                        })
                    })
                    .and_then(|entry| entry["id"].as_str())
                    .filter(|id| models.iter().any(|entry| entry.id == *id))
                    .ok_or_else(|| failure("媒体模型未在实际 Source 目录中登记"))?;
                model = Some(canonical.into());
            }
        }
        let request = ModelRouteRequest {
            operation: kind_operation(kind)?,
            provider,
            model,
        };
        let route = self
            .gateway
            .resolve_route(&request)
            .map_err(|e| failure(e.to_string()))?;
        Ok(MediaRouteSnapshot { route })
    }
    pub fn validate_media_spec(
        &self,
        kind: &str,
        spec: &crate::media::MediaSpec,
        route: &MediaRouteSnapshot,
    ) -> Result<(), OperationFailure> {
        if kind == "speech" {
            return Ok(());
        }
        let models = metadata_models(route.route.protocol.config_name(), kind)?;
        let Some(model) = models.iter().find(|model| model["id"] == route.route.model) else {
            return Ok(());
        };
        let references = usize::from(spec.image.is_some()) + spec.reference_images.len();
        let mode_id = match (kind, references > 0) {
            ("image", false) => "text2image",
            ("image", true) => "image2image",
            ("video", false) => "text2video",
            ("video", true) => "image2video",
            _ => return Err(failure("媒体类型无效")),
        };
        let mode = model["modes"]
            .as_array()
            .and_then(|modes| modes.iter().find(|mode| mode["id"] == mode_id))
            .ok_or_else(|| failure("此模型没有声明所需生成模式"))?;
        let limits = &mode["inputLimits"]["referenceImages"];
        if limits["min"]
            .as_u64()
            .is_some_and(|min| (references as u64) < min)
            || limits["max"]
                .as_u64()
                .is_some_and(|max| (references as u64) > max)
        {
            return Err(failure("参考图数量不符合实际模型模式约束"));
        }
        let properties = mode["parameterSchema"]["properties"]
            .as_object()
            .ok_or_else(|| failure("实际媒体模式缺少参数声明"))?;
        let mut parameters = spec.options.clone();
        for (name, value) in [
            ("ratio", spec.ratio.as_ref()),
            ("resolution", spec.resolution.as_ref()),
            ("quality", spec.quality.as_ref()),
            ("format", spec.format.as_ref()),
        ] {
            if let Some(value) = value {
                let name = if name == "resolution" && properties.contains_key("video_resolution") {
                    "video_resolution"
                } else if name == "resolution" && properties.contains_key("resolution_type") {
                    "resolution_type"
                } else {
                    name
                };
                parameters.insert(name.into(), json!(value));
            }
        }
        if let Some(count) = spec.count {
            parameters.insert("n".into(), json!(count));
        }
        if let Some(duration) = spec.duration {
            parameters.insert("duration".into(), json!(duration));
        }
        for (key, value) in parameters {
            let Some(schema) = properties.get(&key) else {
                if key == "responses_model"
                    && route.route.protocol == ProtocolFamily::OpenAiCodexResponsesImage
                {
                    continue;
                }
                return Err(failure(format!("此模型模式未声明媒体参数：{key}")));
            };
            let typed = match schema["type"].as_str() {
                Some("string") => value.is_string(),
                Some("boolean") => value.is_boolean(),
                Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
                Some("number") => value.as_f64().is_some_and(f64::is_finite),
                _ => false,
            };
            if !typed
                || schema["enum"]
                    .as_array()
                    .is_some_and(|values| !values.contains(&value))
            {
                return Err(failure(format!("媒体参数不符合模型声明：{key}")));
            }
            if let Some(number) = value.as_f64() {
                if schema["minimum"].as_f64().is_some_and(|min| number < min)
                    || schema["maximum"].as_f64().is_some_and(|max| number > max)
                {
                    return Err(failure(format!("媒体参数超出模型范围：{key}")));
                }
            }
        }
        Ok(())
    }
    pub fn describe_media(
        &self,
        kind: &str,
        provider: Option<String>,
        model: Option<String>,
        mode: Option<String>,
    ) -> Result<Value, OperationFailure> {
        let operation = kind_operation(kind)?;
        if model.is_some() && provider.is_none() {
            return Err(failure("查询指定模型时必须同时指定供应商"));
        }
        if mode.as_deref().is_some_and(|m| {
            !matches!(
                (kind, m),
                ("image", "text2image" | "image2image") | ("video", "text2video" | "image2video")
            )
        }) {
            return Err(failure("媒体模式不受支持"));
        }
        let default = self
            .gateway
            .resolve_route(&ModelRouteRequest::for_operation(operation))
            .ok();
        let configured = self.gateway.configured_routes();
        let views = self.gateway.configured_provider_views();
        if provider
            .as_ref()
            .is_some_and(|id| !views.iter().any(|(actual, _, _)| actual == id))
        {
            return Err(failure("媒体供应商未登记"));
        }
        let mut providers = Vec::new();
        for (id, protocol, _) in views {
            if provider.as_ref().is_some_and(|wanted| wanted != &id) {
                continue;
            }
            let supported = ProtocolFamily::parse(&protocol)
                .is_some_and(|family| family.serves(operation.operation_class()));
            if !supported {
                continue;
            }
            let mut models = metadata_models(&protocol, kind)?;
            let binding = configured
                .get(kind)
                .filter(|binding| binding.provider == id);
            if let Some(binding) = binding {
                if !models.iter().any(|m| m["id"] == binding.model) {
                    models.insert(0,json!({"id":binding.model,"name":binding.model,"protocolId":protocol,"inputs":[],"outputs":[kind],"modes":[],"metadataKnown":false,"constraintMessage":"此模型由当前配置明确登记；现役声明没有额外约束，服务不推测。"}));
                }
            }
            if provider.is_some() {
                let chosen = if let Some(wanted) = model.as_ref() {
                    models.iter().find(|m| {
                        m["id"] == wanted.as_str()
                            || m["aliases"].as_array().is_some_and(|aliases| {
                                aliases.iter().any(|alias| alias == wanted.as_str())
                            })
                    })
                } else {
                    binding
                        .and_then(|binding| models.iter().find(|m| m["id"] == binding.model))
                        .or_else(|| models.first())
                }
                .ok_or_else(|| failure("媒体模型未登记或此供应商没有可用模型声明"))?;
                let modes = chosen["modes"].as_array();
                let selected_mode = if let Some(mode) = mode.as_ref() {
                    Some(
                        modes
                            .and_then(|modes| modes.iter().find(|m| m["id"] == mode.as_str()))
                            .ok_or_else(|| failure("此模型未声明指定生成模式"))?
                            .clone(),
                    )
                } else {
                    modes.and_then(|modes| modes.first()).cloned()
                };
                providers.push(json!({"providerId":id,"displayName":id,"model":chosen,"mode":selected_mode,"source":"current_provider_binding_and_incumbent_pure_declarations"}));
            } else if !models.is_empty() {
                providers.push(json!({"providerId":id,"displayName":id,"models":models,"source":"current_provider_binding_and_incumbent_pure_declarations"}));
            }
        }
        if providers.is_empty() {
            return Err(failure("当前没有登记支持此媒体类型的供应商与模型"));
        }
        let mut summary = json!({"kind":kind,"selection":{"defaultConfigured":default.is_some(),"overrideRequired":false,"selectionPolicy":"configured_default","defaultInvocation":{"provider":"omit","model":"omit","mode":"omit_unless_needed","options":"omit_unless_needed"}},"providers":providers,"instruction":"这些选项来自当前实际配置和现役纯供应商声明。普通生成省略 provider/model；失败不会自动切换供应商。"});
        if provider.is_some() {
            let selected = summary["providers"][0].clone();
            summary["providerId"] = selected["providerId"].clone();
            summary["providerName"] = selected["displayName"].clone();
            summary["model"] = selected["model"].clone();
            summary["mode"] = selected["mode"].clone();
        }
        Ok(summary)
    }
    pub async fn with_media_job<F, T>(
        &self,
        route: MediaRouteSnapshot,
        progress: crate::media::MediaProgress,
        context: OperationCallContext,
        principal: lingxi_kernel::Principal,
        future: F,
    ) -> Result<T, String>
    where
        F: Future<Output = Result<T, String>>,
    {
        let materials = Arc::new(Mutex::new(Vec::new()));
        let result = MEDIA_MATERIALS
            .scope(
                materials.clone(),
                FROZEN_MEDIA_ROUTE.scope(
                    route,
                    MEDIA_PROGRESS.scope(
                        progress,
                        MEDIA_PRINCIPAL.scope(principal, MEDIA_TASK_OWNER.scope(context, future)),
                    ),
                ),
            )
            .await;
        result.map_err(|error| {
            let materials = materials.lock().expect("媒体诊断材料锁");
            sanitize_diagnostic(
                &error,
                &materials.iter().map(String::as_str).collect::<Vec<_>>(),
                1024,
            )
        })
    }
    pub fn media_snapshot(&self, tracker: &str) -> Result<Value, OperationFailure> {
        let record = self
            .tasks
            .lock()
            .expect("媒体任务登记锁")
            .get(tracker)
            .cloned()
            .ok_or_else(|| failure("media tracker missing"))?;
        let owner = record
            .owner
            .as_ref()
            .ok_or_else(|| failure("media tracker has no actual owner"))?;
        let kind = match record.kind {
            MediaTaskKind::Image => "image",
            MediaTaskKind::Video => "video",
        };
        Ok(
            json!({"version":1,"tracker":tracker,"providerJobId":record.provider_job_id,"legacyJobId":record.legacy_job_id,"kind":kind,"route":MediaRouteSnapshot{route:record.route.clone()}.to_value(),"owner":{"sessionId":owner.session_id,"runId":owner.run_id,"causeRef":owner.cause_ref}}),
        )
    }
    pub(crate) fn restore_media_snapshot(
        &self,
        value: &Value,
        context: &OperationCallContext,
        previous_execution: &str,
    ) -> Result<(), OperationFailure> {
        if value["version"] != 1
            || value["owner"]["sessionId"] != context.session_id
            || value["owner"]["runId"] != previous_execution
            || value["owner"]["causeRef"] != json!(context.cause_ref)
        {
            return Err(failure("media restore ownership mismatch"));
        }
        let text = |key: &str| {
            value[key]
                .as_str()
                .filter(|v| !v.is_empty() && v.len() <= 4096 && !v.chars().any(char::is_control))
                .map(String::from)
                .ok_or_else(|| failure("media task receipt damaged"))
        };
        let tracker = text("tracker")?;
        let provider_job_id = text("providerJobId")?;
        let kind = match text("kind")?.as_str() {
            "image" => MediaTaskKind::Image,
            "video" => MediaTaskKind::Video,
            _ => return Err(failure("media task kind damaged")),
        };
        let route = MediaRouteSnapshot::from_value(&value["route"])?;
        let legacy_job_id = if value["legacyJobId"].is_null() {
            None
        } else {
            Some(text("legacyJobId")?)
        };
        let mut tasks = self.tasks.lock().expect("媒体任务登记锁");
        if let Some(old) = tasks.get(&tracker) {
            let same_cause = old.owner.as_ref().is_some_and(|owner| {
                owner.session_id == context.session_id
                    && owner.cause_ref == context.cause_ref
                    && (owner.run_id == previous_execution || owner == context)
            });
            if !same_cause || old.provider_job_id != provider_job_id || old.route != route.route {
                return Err(failure("media tracker restore collision"));
            }
            if old.owner.as_ref() == Some(context) {
                return Ok(());
            }
        } else if tasks.len() >= super::MEDIA_TASK_REGISTRY_CAP {
            return Err(failure("media tracker registry full"));
        }
        let mut record = MediaTaskRecord::new(
            kind,
            tracker.clone(),
            provider_job_id,
            legacy_job_id,
            route.route,
        );
        record.owner = Some(context.clone());
        tasks.insert(tracker, Arc::new(record));
        Ok(())
    }
}
pub(super) fn frozen_route(
    operation: ModelOperation,
) -> Result<Option<ResolvedModelRoute>, OperationFailure> {
    let route = FROZEN_MEDIA_ROUTE.try_with(|r| r.route.clone()).ok();
    if route.as_ref().is_some_and(|r| r.operation != operation) {
        return Err(failure("media operation differs from its approved route"));
    }
    Ok(route)
}
pub(super) async fn before_media_download() -> Result<(), OperationFailure> {
    if let Ok(progress) = MEDIA_PROGRESS.try_with(Clone::clone) {
        progress.downloading().await.map_err(failure)?;
    }
    Ok(())
}
pub(super) async fn before_media_dispatch() -> Result<(), OperationFailure> {
    if let Ok(progress) = MEDIA_PROGRESS.try_with(Clone::clone) {
        progress.dispatched().await.map_err(failure)?;
    }
    Ok(())
}
pub(super) fn media_process_owner(
    fallback: crate::procsupervisor::ProcessOwner,
) -> Result<crate::procsupervisor::ProcessOwner, OperationFailure> {
    let Some(context) = MEDIA_TASK_OWNER.try_with(Clone::clone).ok() else {
        return Ok(fallback);
    };
    let principal = MEDIA_PRINCIPAL
        .try_with(Clone::clone)
        .map_err(|_| failure("media process has no actual authorized principal"))?;
    Ok(crate::procsupervisor::ProcessOwner {
        principal_kind: principal.storage_kind().into(),
        principal_subject: principal.storage_subject(),
        session_id: context.session_id,
        run_id: context.run_id,
        tool_call_id: lingxi_protocol::ToolCallId::new(
            context
                .cause_ref
                .ok_or_else(|| failure("media process has no actual tool cause"))?,
        ),
    })
}
pub(super) fn context_or_current(
    context: Option<&OperationCallContext>,
) -> Option<OperationCallContext> {
    context
        .cloned()
        .or_else(|| MEDIA_TASK_OWNER.try_with(Clone::clone).ok())
}
pub(super) fn capture_media_auth(auth: &ApplicableAuth) -> Result<(), OperationFailure> {
    MEDIA_MATERIALS
        .try_with(|all| {
            let mut all = all.lock().expect("媒体诊断材料锁");
            for material in auth.materials() {
                if !material.is_empty() && !all.iter().any(|old| old == material) {
                    if all.len() >= 64
                        || all
                            .iter()
                            .map(String::len)
                            .sum::<usize>()
                            .saturating_add(material.len())
                            > 1024 * 1024
                    {
                        return Err(failure(
                            "media credential rotations exceeded the bounded job budget",
                        ));
                    }
                    all.push(material.to_owned());
                }
            }
            Ok(())
        })
        .unwrap_or(Ok(()))
}
pub(super) fn scrub_media_poll(
    outcome: lingxi_adapters::models::operations::TaskPollOutcome,
    auth: &ApplicableAuth,
) -> lingxi_adapters::models::operations::TaskPollOutcome {
    use lingxi_adapters::models::operations::TaskPollOutcome;
    match outcome {
        TaskPollOutcome::Failed { reason } => TaskPollOutcome::Failed {
            reason: lingxi_adapters::models::dispatch::scrubbed_excerpt(&reason, auth),
        },
        other => other,
    }
}

static DECLARATIONS: std::sync::OnceLock<Result<Value, String>> = std::sync::OnceLock::new();
fn declarations() -> Result<&'static Value, OperationFailure> {
    DECLARATIONS
        .get_or_init(|| {
            serde_json::from_str(metadata::DECLARATIONS_JSON)
                .map_err(|_| "媒体声明生成物损坏".to_owned())
        })
        .as_ref()
        .map_err(failure)
}

/// 管理面只读内置媒体声明（generate-media-metadata.mjs 生成物）：返回某能力键
/// 下每个供应商的 {providerId, displayName, capability block}。键缺席的供应商跳过。
pub(crate) fn media_declaration_catalog(
    capability_key: &str,
) -> Result<Vec<(String, String, Value)>, OperationFailure> {
    let mut out = Vec::new();
    for provider in declarations()?
        .as_array()
        .ok_or_else(|| failure("媒体声明生成物不是列表"))?
    {
        let Some(block) = provider["media"][capability_key].as_object() else {
            continue;
        };
        let id = provider["providerId"]
            .as_str()
            .ok_or_else(|| failure("媒体声明供应商缺少 id"))?
            .to_owned();
        let display = provider["displayName"].as_str().unwrap_or(&id).to_owned();
        out.push((id, display, Value::Object(block.clone())));
    }
    Ok(out)
}

fn metadata_models(protocol: &str, kind: &str) -> Result<Vec<Value>, OperationFailure> {
    let key = if kind == "image" {
        "imageGeneration"
    } else if kind == "video" {
        "videoGeneration"
    } else {
        return Ok(vec![]);
    };
    let mut models = Vec::new();
    for provider in declarations()?
        .as_array()
        .ok_or_else(|| failure("媒体声明生成物不是列表"))?
    {
        if let Some(entries) = provider["media"][key]["models"].as_array() {
            for entry in entries {
                if entry["protocolId"] == protocol
                    && !models.iter().any(|old: &Value| old["id"] == entry["id"])
                {
                    let mut model = entry.clone();
                    model["name"] = entry
                        .get("displayName")
                        .or_else(|| entry.get("name"))
                        .unwrap_or(&entry["id"])
                        .clone();
                    models.push(model);
                }
            }
        }
    }
    Ok(models)
}
