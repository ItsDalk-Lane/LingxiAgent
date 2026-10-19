//! 模型设置的唯一保存、发布与授权探测入口。
use crate::auth::Principal;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use lingxi_kernel::model_exchange::ModelOperation;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone)]
pub enum ModelSettingsChange {
    ProviderPatch {
        provider: String,
        patch: Value,
    },
    ProviderRemove {
        provider: String,
    },
    ModelPatch {
        provider: String,
        model: String,
        patch: Value,
    },
    ModelRemove {
        provider: String,
        model: String,
    },
    SetBinding {
        operation: ModelOperation,
        selection: Option<ModelSelection>,
    },
}
#[derive(Debug, Clone, Serialize)]
pub struct ModelSelection {
    pub provider: String,
    pub model: String,
}
pub use lingxi_protocol::ModelConfigurationApplyReport as ModelApplyReport;
#[derive(Debug, Clone, Serialize)]
pub struct ModelApplyError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub report: ModelApplyReport,
}
impl ModelApplyError {
    pub fn into_response(self) -> Response {
        (
            StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(json!({"error":self.message,"code":self.code,"result":self.report})),
        )
            .into_response()
    }
}
impl std::fmt::Display for ModelApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for ModelApplyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderProbeKind {
    Catalog,
    Connection,
    ModelHealth,
}
/// 由 HTTP 适配者按字段是否存在构造；Some(Null) 不能折叠为未提供。
#[derive(Clone, Default)]
pub struct ProviderProbeDraft {
    pub endpoint: Option<String>,
    pub protocol: Option<String>,
    pub api_key: Option<String>,
    pub headers: Option<Value>,
}
#[derive(Clone)]
pub struct ProviderProbeRequest {
    pub kind: ProviderProbeKind,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub draft: Option<ProviderProbeDraft>,
}
#[derive(Debug, Clone)]
pub struct ProviderProbeReport {
    pub value: Value,
}

type AuthorizationObserver = Arc<dyn Fn(&str) + Send + Sync>;
#[derive(Clone)]
struct ModelAuthority {
    principal: Principal,
    auth: Arc<crate::auth::AuthService>,
    management: Arc<crate::management::ManagementState>,
    clock: Arc<dyn crate::inject::ServiceClock>,
}
impl ModelAuthority {
    fn valid(&self) -> bool {
        self.auth.validate_principal(&self.principal).is_ok()
            && (self.principal.credential_kind != crate::auth::CredentialKind::WebSession
                || self
                    .management
                    .web_session_current(&self.principal, self.clock.now_unix_ms(), true)
                    .unwrap_or(false))
    }
}

pub struct ModelConfiguration {
    pub(crate) activation: Arc<AtomicBool>,
    home: PathBuf,
    runtime_dir: PathBuf,
    source: std::sync::RwLock<Option<crate::config::ModelPlaneSource>>,
    published: std::sync::RwLock<Value>,
    mutation: tokio::sync::Mutex<()>,
    gateway: Option<Arc<lingxi_adapters::models::gateway::ConfigModelGateway>>,
    credentials: Option<Arc<crate::credentials::CredentialService>>,
    network: Option<Arc<lingxi_adapters::models::network::NetworkPlane>>,
    operations: Option<Arc<crate::operations::OperationService>>,
    management: Arc<crate::management::ManagementState>,
    auth: Arc<crate::auth::AuthService>,
    authorization_observer: std::sync::RwLock<Option<AuthorizationObserver>>,
    clock: Arc<dyn crate::inject::ServiceClock>,
    safety: crate::file_safety::FileSafety,
    storage: Arc<lingxi_adapters::storage::RunDatabase>,
    tasks: Arc<crate::task_supervisor::TaskSupervisor>,
    quotas: Arc<crate::quotas::QuotaManager>,
    cache_lock: Arc<std::sync::Mutex<()>>,
}
impl ModelConfiguration {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        home: PathBuf,
        runtime_dir: PathBuf,
        source: Option<crate::config::ModelPlaneSource>,
        activation: Arc<AtomicBool>,
        gateway: Option<Arc<lingxi_adapters::models::gateway::ConfigModelGateway>>,
        credentials: Option<Arc<crate::credentials::CredentialService>>,
        network: Option<Arc<lingxi_adapters::models::network::NetworkPlane>>,
        operations: Option<Arc<crate::operations::OperationService>>,
        management: Arc<crate::management::ManagementState>,
        auth: Arc<crate::auth::AuthService>,
        clock: Arc<dyn crate::inject::ServiceClock>,
        storage: Arc<lingxi_adapters::storage::RunDatabase>,
        tasks: Arc<crate::task_supervisor::TaskSupervisor>,
        quotas: Arc<crate::quotas::QuotaManager>,
    ) -> Result<Self, String> {
        let published = if let Some(source) = &source {
            Self::read_document(source).map_err(|e| e.to_string())?.0
        } else {
            json!({"providers":{},"models":{}})
        };
        Ok(Self {
            activation,
            safety: crate::file_safety::FileSafety::new(&home),
            home,
            runtime_dir,
            source: std::sync::RwLock::new(source),
            published: std::sync::RwLock::new(published),
            mutation: tokio::sync::Mutex::new(()),
            gateway,
            credentials,
            network,
            operations,
            management,
            auth,
            authorization_observer: std::sync::RwLock::new(None),
            clock,
            storage,
            tasks,
            quotas,
            cache_lock: Arc::new(std::sync::Mutex::new(())),
        })
    }
    /// 仅通知真实授权/排队/写入时点；不能替换身份、锁、保存或网络结果。
    #[doc(hidden)]
    pub fn set_authorization_observer(&self, observer: Option<AuthorizationObserver>) {
        *self
            .authorization_observer
            .write()
            .unwrap_or_else(|p| p.into_inner()) = observer;
    }
    fn observe_authorization(&self, phase: &str) {
        let observer = self
            .authorization_observer
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(observer) = observer {
            observer(phase);
        }
    }
    /// 检查夹具只能持有正式写锁，不能伪造已经排队或替换实际执行。
    #[doc(hidden)]
    pub async fn mutation_lease_for_observation(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.mutation.lock().await
    }
    fn authority(&self, principal: &Principal) -> ModelAuthority {
        ModelAuthority {
            principal: principal.clone(),
            auth: self.auth.clone(),
            management: self.management.clone(),
            clock: self.clock.clone(),
        }
    }
    fn validate_principal(&self, principal: &Principal) -> Result<(), ModelApplyError> {
        if self.authority(principal).valid() {
            Ok(())
        } else {
            Err(self.error(
                403,
                "model_authority_revoked",
                "模型设置原始身份已撤销、过期或权限已改变",
            ))
        }
    }
    pub fn is_active(&self) -> bool {
        self.gateway.is_some() && self.activation.load(Ordering::Acquire)
    }
    pub fn source(&self) -> Option<crate::config::ModelPlaneSource> {
        self.source
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    /// 登录管理沿用同一快照中的精确供应商别名，不猜测或借用其它供应商凭证。
    pub async fn canonical_provider_id(&self, requested: &str) -> String {
        let _lock = self.mutation.lock().await;
        let document = self.published.read().unwrap_or_else(|p| p.into_inner());
        provider_in_document(&document, requested)
    }
    fn report(&self) -> ModelApplyReport {
        ModelApplyReport {
            generation: self
                .gateway
                .as_ref()
                .map_or(0, |g| g.upcoming_generation().saturating_sub(1)),
            activated: self.is_active(),
            ..Default::default()
        }
    }
    fn error(&self, status: u16, code: &str, message: impl Into<String>) -> ModelApplyError {
        ModelApplyError {
            status,
            code: code.into(),
            message: message.into(),
            report: self.report(),
        }
    }
    fn require(&self, principal: &Principal, scope: &str) -> Result<(), ModelApplyError> {
        self.validate_principal(principal)?;
        if principal.is_local_owner() || principal.has_scope(scope) {
            Ok(())
        } else {
            Err(self.error(403, "insufficient_scope", format!("缺少权限：{scope}")))
        }
    }
    fn plane(
        document: &Value,
        embedded: bool,
    ) -> Result<lingxi_adapters::models::config::ModelPlaneConfig, String> {
        let value = if embedded {
            json!({"providers":document.get("providers").cloned().unwrap_or(json!({})),"models":document.get("models").cloned().unwrap_or(json!({}))})
        } else {
            document.clone()
        };
        lingxi_adapters::models::config::ModelPlaneConfig::from_value(value)
            .map_err(|e| e.to_string())
    }
    fn read_document(
        source: &crate::config::ModelPlaneSource,
    ) -> Result<(Value, crate::file_safety::Snapshot), String> {
        let path = source.path();
        let snapshot = crate::file_safety::Snapshot::read(path).map_err(|e| e.to_string())?;
        let document: Value = serde_json::from_slice(&snapshot.bytes).map_err(|e| e.to_string())?;
        let embedded = matches!(
            source,
            crate::config::ModelPlaneSource::EmbeddedInConfigFile { .. }
        );
        if embedded {
            crate::config::service_config_from_value(path, &document).map_err(|e| e.to_string())?;
            if document.get("providers").is_none() && document.get("models").is_none() {
                return Err("config file no longer carries providers/models sections; removing the model plane requires a restart".into());
            }
        }
        Self::plane(&document, embedded)?;
        Ok((document, snapshot))
    }
    fn require_changes(
        &self,
        principal: &Principal,
        changes: &[ModelSettingsChange],
    ) -> Result<(), ModelApplyError> {
        self.validate_principal(principal)?;
        for change in changes {
            match change {
                ModelSettingsChange::SetBinding { .. } => {
                    self.require(principal, "settings.write")?
                }
                _ => self.require(principal, "providers.manage")?,
            }
            let patch = match change {
                ModelSettingsChange::ProviderPatch { patch, .. }
                | ModelSettingsChange::ModelPatch { patch, .. } => Some(patch),
                _ => None,
            };
            if patch.is_some_and(has_secret_edit) {
                self.require(principal, "secrets.write")?;
            }
        }
        Ok(())
    }
    pub async fn save(
        &self,
        principal: &Principal,
        changes: &[ModelSettingsChange],
    ) -> Result<ModelApplyReport, ModelApplyError> {
        self.save_inner(principal, changes, None).await
    }
    /// 依快照编辑时，锁内复核同一代次，防止并发删除后的旧编辑把模型复活。
    pub(crate) async fn save_at_generation(
        &self,
        principal: &Principal,
        changes: &[ModelSettingsChange],
        expected_generation: u64,
    ) -> Result<ModelApplyReport, ModelApplyError> {
        self.save_inner(principal, changes, Some(expected_generation))
            .await
    }
    async fn save_inner(
        &self,
        principal: &Principal,
        changes: &[ModelSettingsChange],
        expected_generation: Option<u64>,
    ) -> Result<ModelApplyReport, ModelApplyError> {
        self.require_changes(principal, changes)?;
        if changes.is_empty() {
            return Ok(self.report());
        }
        self.observe_authorization("save.queued");
        let _lock = self.mutation.lock().await;
        self.require_changes(principal, changes)?;
        if expected_generation.is_some_and(|expected| expected != self.report().generation) {
            return Err(self.error(
                409,
                "model_generation_changed",
                "模型配置已被其他操作更新，请重新读取后再编辑",
            ));
        }
        let source = self
            .source()
            .unwrap_or_else(|| crate::config::ModelPlaneSource::ModelsFile {
                path: self.runtime_dir.join(crate::config::MODELS_FILE_NAME),
            });
        let existed = source
            .path()
            .try_exists()
            .map_err(|e| self.error(409, "model_source_unreadable", e.to_string()))?;
        let (mut document, before) = if existed {
            let (doc, snapshot) = Self::read_document(&source)
                .map_err(|e| self.error(409, "model_source_invalid", e))?;
            (doc, Some(snapshot))
        } else {
            if self.source().is_some() {
                return Err(self.error(
                    409,
                    "model_source_missing",
                    "原模型配置文件已被移除，拒绝创建替代文件",
                ));
            }
            (json!({"providers":{},"models":{}}), None)
        };
        let mut cleared = Vec::new();
        let changes = canonical_changes(&document, changes);
        apply_changes(&mut document, &changes, &mut cleared)
            .map_err(|e| self.error(400, "invalid_model_settings", e))?;
        let embedded = matches!(
            source,
            crate::config::ModelPlaneSource::EmbeddedInConfigFile { .. }
        );
        let plane = Self::plane(&document, embedded)
            .map_err(|e| self.error(400, "invalid_model_settings", e))?;
        if let Some(binding) = &plane.models.vision {
            let entry = plane
                .providers
                .get(&binding.provider)
                .expect("validated provider");
            let model = entry
                .models
                .as_ref()
                .and_then(|models| models.iter().find(|model| model.id == binding.model));
            let declared = model
                .and_then(|model| model.capabilities.as_ref())
                .or(binding.capabilities.as_ref());
            let protocol = model
                .and_then(|model| model.protocol.as_deref())
                .unwrap_or(&entry.protocol);
            if declared.and_then(|cap| cap.image_input) != Some(true)
                || !lingxi_kernel::model_exchange::ProtocolFamily::parse(protocol)
                    .is_some_and(|p| p.serves(ModelOperation::Chat.operation_class()))
            {
                return Err(self.error(
                    400,
                    "vision_capability_required",
                    "视觉辅助模型必须明确支持图像输入和文本生成",
                ));
            }
        }
        self.preflight(&plane)?;
        let bytes = serde_json::to_vec_pretty(&document)
            .map_err(|e| self.error(400, "invalid_model_settings", e.to_string()))?;
        let result = if let Some(snapshot) = &before {
            self.safety.replace_private(
                "model-configuration",
                source.path(),
                snapshot,
                &bytes,
                || {
                    self.observe_authorization("save.authorize");
                    self.require_changes(principal, &changes).is_ok()
                },
            )
        } else {
            self.safety
                .create("model-configuration", source.path(), &bytes, || {
                    self.observe_authorization("save.authorize");
                    self.require_changes(principal, &changes).is_ok()
                })
        };
        if let Err(error) = result {
            // 写入结果未知时保留其事实，不能把可能已发生的写入伪装成撤权前零副作用。
            if !matches!(error, crate::file_safety::WriteError::Unknown(_)) {
                self.require_changes(principal, &changes)?;
            }
            let mut err = self.error(409, "model_source_write_failed", error.to_string());
            if matches!(error, crate::file_safety::WriteError::Unknown(_)) {
                err.code = "model_source_write_unconfirmed".into();
            }
            return Err(err);
        }
        *self.source.write().unwrap_or_else(|p| p.into_inner()) = Some(source);
        let report = ModelApplyReport {
            saved: true,
            cleared_bindings: cleared,
            ..self.report()
        };
        self.publish(plane, document, report, "models.save").await
    }
    /// 启动与后续发布使用同一代理优先规则，保留来源中的可信 CA。
    pub fn initial_network_policy(
        home: &std::path::Path,
        base: &lingxi_adapters::models::network::NetworkConfigSection,
    ) -> Result<lingxi_adapters::models::network::NetworkPolicy, String> {
        let prefs = crate::memory::MemoryService::read_preferences_root_at_home(home)?;
        crate::settings_configuration::effective_network_policy(base, &prefs)
    }
    pub async fn refresh_network_preferences(
        &self,
        principal: &Principal,
    ) -> Result<ModelApplyReport, ModelApplyError> {
        self.require(principal, "settings.write")?;
        self.observe_authorization("network.queued");
        let _lock = self.mutation.lock().await;
        self.require(principal, "settings.write")?;
        let document = self
            .published
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let embedded = self.source().is_some_and(|source| {
            matches!(
                source,
                crate::config::ModelPlaneSource::EmbeddedInConfigFile { .. }
            )
        });
        let plane = Self::plane(&document, embedded)
            .map_err(|e| self.error(409, "model_config_invalid", e))?;
        let policy = Self::initial_network_policy(&self.home, &plane.network)
            .map_err(|e| self.error(400, "invalid_network_preferences", e))?;
        let network = self.network.as_ref().ok_or_else(|| {
            self.error(
                503,
                "network_plane_unavailable",
                "此宿主未装配共享网络配置链",
            )
        })?;
        network.apply(policy);
        let report = ModelApplyReport {
            saved: true,
            applied: true,
            projection_confirmed: true,
            ..self.report()
        };
        self.management
            .record_model_change(
                "network.preferences",
                report.generation,
                self.clock.now_unix_ms(),
            )
            .map_err(|message| ModelApplyError {
                status: 500,
                code: "network_audit_unconfirmed".into(),
                message,
                report: report.clone(),
            })?;
        Ok(ModelApplyReport {
            audit_recorded: true,
            ..report
        })
    }
    fn preflight(
        &self,
        plane: &lingxi_adapters::models::config::ModelPlaneConfig,
    ) -> Result<(), ModelApplyError> {
        if self.gateway.is_none() || self.credentials.is_none() {
            return Err(self.error(
                503,
                "model_configuration_unavailable",
                "此注入宿主未装配真实模型配置链",
            ));
        }
        for (id, provider) in &plane.providers {
            if let Some(models) = &provider.models {
                for model in models {
                    let official_deepseek = id.eq_ignore_ascii_case("deepseek")
                        || reqwest::Url::parse(&provider.endpoint)
                            .ok()
                            .and_then(|url| url.host_str().map(str::to_string))
                            .as_deref()
                            == Some("api.deepseek.com");
                    if official_deepseek && model.id.trim().eq_ignore_ascii_case("deepseek") {
                        return Err(self.error(
                            400,
                            "invalid_model_metadata",
                            "deepseek 是供应商编号，必须选择具体模型编号",
                        ));
                    }
                    let metadata = serde_json::to_value(model)
                        .map_err(|e| self.error(400, "invalid_model_metadata", e.to_string()))?;
                    crate::agents_admin::agent_config_readback::validate_model_metadata(
                        id, &metadata,
                    )
                    .map_err(|e| self.error(400, "invalid_model_metadata", e))?;
                }
            }
        }
        Self::initial_network_policy(&self.home, &plane.network)
            .map_err(|e| self.error(400, "invalid_network_preferences", e))?;
        let endpoints = plane
            .providers
            .values()
            .filter(|p| !p.endpoint.is_empty())
            .map(|p| p.endpoint.clone())
            .collect::<Vec<_>>();
        lingxi_adapters::models::egress::prepare_provider_origins(&endpoints)
            .map_err(|e| self.error(400, "invalid_model_egress", e.to_string()))?;
        Ok(())
    }
    async fn publish(
        &self,
        plane: lingxi_adapters::models::config::ModelPlaneConfig,
        document: Value,
        mut report: ModelApplyReport,
        action: &str,
    ) -> Result<ModelApplyReport, ModelApplyError> {
        let endpoints = plane
            .providers
            .values()
            .filter(|p| !p.endpoint.is_empty())
            .map(|p| p.endpoint.clone())
            .collect::<Vec<_>>();
        let origins = lingxi_adapters::models::egress::prepare_provider_origins(&endpoints)
            .map_err(|e| self.error(400, "invalid_model_egress", e.to_string()))?;
        let network_policy =
            Self::initial_network_policy(&self.home, &plane.network).map_err(|message| {
                ModelApplyError {
                    status: 400,
                    code: "invalid_network_preferences".into(),
                    message,
                    report: report.clone(),
                }
            })?;
        let gateway = self.gateway.as_ref().expect("preflight gateway");
        let next = gateway.upcoming_generation();
        self.credentials
            .as_ref()
            .expect("preflight credentials")
            .reload(&plane, next)
            .await;
        report.generation = gateway.reload(plane.clone());
        if let Some(network) = &self.network {
            network.apply(network_policy);
        }
        self.activation.store(true, Ordering::Release);
        report.applied = true;
        report.activated = true;
        if let Some(operations) = &self.operations {
            operations.publish_egress_origins(origins);
        }
        let projection = self.write_compatibility_projection(&document);
        *self.published.write().unwrap_or_else(|p| p.into_inner()) = document.clone();
        if let Err(error) = projection {
            return Err(ModelApplyError {
                status: 500,
                code: "model_projection_unconfirmed".into(),
                message: error,
                report,
            });
        }
        report.projection_confirmed = true;
        if let Err(error) =
            self.management
                .record_model_change(action, report.generation, self.clock.now_unix_ms())
        {
            return Err(ModelApplyError {
                status: 500,
                code: "model_audit_unconfirmed".into(),
                message: error,
                report,
            });
        }
        report.audit_recorded = true;
        Ok(report)
    }
    pub async fn reload(&self, principal: &Principal) -> Result<ModelApplyReport, ModelApplyError> {
        self.validate_principal(principal)?;
        if !principal.is_local_owner() {
            return Err(self.error(403, "local_only", "模型配置重载仅限本机主人"));
        }
        self.observe_authorization("reload.queued");
        let _lock = self.mutation.lock().await;
        self.validate_principal(principal)?;
        let source = self
            .source()
            .ok_or_else(|| self.error(404, "model_plane_unconfigured", "未配置模型来源"))?;
        let (document, _) =
            Self::read_document(&source).map_err(|e| self.error(409, "model_config_invalid", e))?;
        let plane = Self::plane(
            &document,
            matches!(
                source,
                crate::config::ModelPlaneSource::EmbeddedInConfigFile { .. }
            ),
        )
        .map_err(|e| self.error(409, "model_config_invalid", e))?;
        self.preflight(&plane)?;
        self.publish(plane, document, self.report(), "models.reload")
            .await
    }
    fn provider_views(document: &Value, masked: bool) -> Value {
        let mut out = serde_json::Map::new();
        for (id, provider) in document["providers"].as_object().into_iter().flatten() {
            let mut view = provider["metadata"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            view.remove("logged_in");
            view.remove("has_credentials");
            view.remove("credential_state");
            if masked {
                view = crate::agents_admin::agent_config_readback::mask_object_secrets(
                    &Value::Object(view),
                )
                .as_object()
                .cloned()
                .unwrap_or_default();
            }
            view.insert("api".into(), provider["protocol"].clone());
            view.insert("base_url".into(), provider["endpoint"].clone());
            let key = provider["auth"]["apiKey"].as_str().unwrap_or("");
            view.insert(
                "api_key".into(),
                json!(if masked && !key.is_empty() {
                    "********"
                } else {
                    key
                }),
            );
            let headers = provider["headers"]
                .as_object()
                .map(|headers| {
                    headers
                        .iter()
                        .map(|(key, value)| {
                            (
                                key.clone(),
                                if masked {
                                    json!("********")
                                } else {
                                    value.clone()
                                },
                            )
                        })
                        .collect::<serde_json::Map<_, _>>()
                })
                .unwrap_or_default();
            view.insert("headers".into(), Value::Object(headers));
            view.insert(
                "models".into(),
                provider.get("models").cloned().unwrap_or(json!([])),
            );
            view.insert(
                "auth_type".into(),
                json!(if provider["auth"]["kind"] == "oauth" {
                    "oauth"
                } else if provider["auth"]["kind"] == "none" {
                    "none"
                } else {
                    "api-key"
                }),
            );
            out.insert(id.clone(), Value::Object(view));
        }
        Value::Object(out)
    }
    fn write_compatibility_projection(&self, document: &Value) -> Result<(), String> {
        let providers = Self::provider_views(document, false);
        // 兼容目录由唯一发布者派生，不能反向覆盖真实 Source 或形成第二次 apply。
        let previous = self
            .published
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let removed = previous["providers"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(id, _)| document["providers"].get(*id).is_none())
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        crate::agents_admin::agent_config_readback::publish_legacy_provider_projection(
            &self.home,
            providers.as_object().expect("provider views"),
            &removed,
        )
    }
    pub async fn snapshot(&self, principal: &Principal) -> Result<Value, ModelApplyError> {
        self.validate_principal(principal)?;
        if !principal.is_local_owner()
            && !principal.has_scope("settings.read")
            && !principal.has_scope("providers.manage")
            && !principal.has_scope("chat")
        {
            return Err(self.error(403, "insufficient_scope", "缺少模型设置读取权限"));
        }
        self.observe_authorization("snapshot.queued");
        let _lock = self.mutation.lock().await;
        self.validate_principal(principal)?;
        let document = self
            .published
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let mut providers = Self::provider_views(&document, true);
        if let Some(gateway) = &self.gateway {
            let catalog = gateway.catalog_models();
            for (id, protocol, endpoint) in gateway.configured_provider_views() {
                if providers.get(&id).is_none() {
                    providers[&id] = json!({"api":protocol,"base_url":endpoint,"api_key":"","headers":{},"models":[]});
                }
                if let Some(models) = catalog.get(&id) {
                    providers[&id]["models"] = json!(models);
                }
            }
        }
        if let Some(credentials) = &self.credentials {
            for status in credentials.status().await {
                if let Some(provider) = providers.get_mut(&status.provider) {
                    // 登录状态只取实时凭证账本，用户可编辑元数据不能冒充已登录。
                    provider["logged_in"] = json!(status.logged_in);
                    provider["credential_state"] = json!(status.state);
                    provider["credential_generation"] = json!(status.generation.to_string());
                }
            }
        }
        // 目录会重新拼入模型扩展字段，必须在全部读面拼装完成后统一遮盖。
        providers = crate::agents_admin::agent_config_readback::mask_object_secrets(&providers);
        let models = self
            .gateway
            .as_ref()
            .map(|g| json!(g.configured_routes()))
            .unwrap_or(json!({}));
        Ok(
            json!({"providers":providers,"models":models,"generation":self.report().generation.to_string(),"activated":self.is_active()}),
        )
    }
    pub async fn saved_api_key(
        &self,
        principal: &Principal,
        provider: &str,
    ) -> Result<String, ModelApplyError> {
        self.require(principal, "providers.manage")?;
        self.require(principal, "secrets.write")?;
        self.observe_authorization("saved_api_key.queued");
        let _lock = self.mutation.lock().await;
        self.require(principal, "providers.manage")?;
        self.require(principal, "secrets.write")?;
        let source = self
            .source()
            .ok_or_else(|| self.error(404, "model_plane_unconfigured", "未配置模型来源"))?;
        let (document, _) =
            Self::read_document(&source).map_err(|e| self.error(409, "model_source_invalid", e))?;
        let provider = provider_in_document(&document, provider);
        if document["providers"].get(&provider).is_none() {
            return Err(self.error(404, "provider_not_found", "供应商不存在"));
        }
        Ok(document["providers"][&provider]["auth"]["apiKey"]
            .as_str()
            .unwrap_or("")
            .into())
    }
    fn require_probe(
        &self,
        principal: &Principal,
        request: &ProviderProbeRequest,
    ) -> Result<(), ModelApplyError> {
        self.validate_principal(principal)?;
        if request.kind == ProviderProbeKind::ModelHealth {
            if !principal.is_studio_owner() {
                return Err(self.error(403, "owner_required", "模型健康检查仅限主人"));
            }
        } else {
            self.require(principal, "providers.manage")?;
        }
        if let Some(draft) = &request.draft {
            if draft
                .api_key
                .as_deref()
                .is_some_and(|key| !key.is_empty() && key.trim() != "********")
                || draft.headers.as_ref().is_some_and(|headers| {
                    headers.as_object().is_some_and(|headers| {
                        headers.values().any(|v| {
                            !v.is_null() && v.as_str().is_none_or(|s| s.trim() != "********")
                        })
                    })
                })
            {
                self.require(principal, "secrets.write")?;
            }
        }
        Ok(())
    }
    pub async fn probe(
        &self,
        principal: &Principal,
        request: ProviderProbeRequest,
    ) -> Result<ProviderProbeReport, ModelApplyError> {
        self.require_probe(principal, &request)?;
        self.observe_authorization("probe.queued");
        let prepared = {
            let _lock = self.mutation.lock().await;
            self.require_probe(principal, &request)?;
            self.prepare_probe(principal, request)?
        };
        // 连接断开只丢等待，实际探测及用量结算仍归既有监督器，且受固定总期限限制。
        let task = self
            .tasks
            .spawn_detached("model-provider-probe".into(), async move {
                prepared.execute().await
            })
            .map_err(|_| self.error(503, "probe_capacity", "供应商探测任务容量不足"))?;
        task.wait().await.map_err(|exit| {
            self.error(
                503,
                "probe_task_failed",
                format!("供应商探测未确认结束：{exit:?}"),
            )
        })?
    }
    fn prepare_probe(
        &self,
        principal: &Principal,
        request: ProviderProbeRequest,
    ) -> Result<PreparedProviderProbe, ModelApplyError> {
        use lingxi_adapters::models::{
            config::ModelPlaneConfig, credentials::ProviderCredentialPort,
            gateway::ConfigModelGateway,
        };
        use lingxi_kernel::model_exchange::{ModelGatewayPort, ModelRouteRequest};
        let shared = self
            .gateway
            .as_ref()
            .ok_or_else(|| self.error(503, "model_configuration_unavailable", "真实模型链未装配"))?
            .clone();
        let generation = shared.upcoming_generation().saturating_sub(1);
        let document = self
            .published
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let provider = provider_in_document(
            &document,
            request.provider.as_deref().unwrap_or("request-draft"),
        );
        let old = document["providers"]
            .get(&provider)
            .cloned()
            .unwrap_or(json!({}));
        let draft = request.draft.clone().unwrap_or_default();
        let plain_key = draft
            .api_key
            .as_deref()
            .is_some_and(|key| !key.trim().is_empty() && key.trim() != "********");
        let draft_headers = draft.headers.as_ref().is_some_and(|headers| {
            headers.as_object().is_some_and(|headers| {
                headers.values().any(|value| {
                    !value.is_null() && value.as_str().is_none_or(|s| s.trim() != "********")
                })
            })
        });
        let request_draft = plain_key || draft_headers;
        if request.kind == ProviderProbeKind::ModelHealth && request.draft.is_some() {
            return Err(self.error(
                400,
                "invalid_probe",
                "模型健康检查必须使用明确已保存目录模型",
            ));
        }
        let mut patch = serde_json::Map::new();
        if let Some(endpoint) = &draft.endpoint {
            if !endpoint.is_empty() {
                patch.insert("base_url".into(), json!(endpoint));
            }
        }
        if let Some(protocol) = &draft.protocol {
            if !protocol.is_empty() {
                patch.insert("api".into(), json!(protocol));
            }
        }
        if let Some(headers) = &draft.headers {
            patch.insert("headers".into(), headers.clone());
        }
        if let Some(key) = &draft.api_key {
            if !key.is_empty() {
                let key = if request.kind == ProviderProbeKind::Connection {
                    key.chars()
                        .filter(|c| c.is_ascii() && !c.is_ascii_control())
                        .collect::<String>()
                } else {
                    key.clone()
                };
                patch.insert("api_key".into(), json!(key.trim()));
            }
        }
        let mut source = old;
        if request_draft {
            // 明文草稿只能使用本次材料；掩码也不能从另一来源补回保存密钥。
            source["auth"] = json!({"kind":"none"});
            source["headers"] = json!({});
        }
        let mut provider_config = normalize_provider(&provider, &source, &Value::Object(patch))
            .map_err(|e| self.error(400, "invalid_probe", e))?;
        if request_draft && draft.headers.is_none() {
            provider_config["headers"] = json!({});
        }
        let protocol = provider_config["protocol"]
            .as_str()
            .ok_or_else(|| self.error(400, "invalid_probe", "协议缺失"))?
            .to_string();
        let endpoint = provider_config["endpoint"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let model = request
            .model
            .clone()
            .or_else(|| {
                provider_config["models"]
                    .as_array()
                    .and_then(|models| models.first())
                    .and_then(|m| m["id"].as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| {
                if protocol == "anthropic-messages" {
                    "claude-sonnet-4-6".into()
                } else {
                    "__provider_catalog__".into()
                }
            });
        let plan = crate::provider_probe::build_probe_plan(
            match request.kind {
                ProviderProbeKind::Catalog => crate::provider_probe::ProbePlanKind::Catalog,
                ProviderProbeKind::Connection => crate::provider_probe::ProbePlanKind::Connection,
                ProviderProbeKind::ModelHealth => crate::provider_probe::ProbePlanKind::ModelHealth,
            },
            &provider,
            &protocol,
            &endpoint,
            Some(&model),
        )
        .map_err(|e| self.error(400, "invalid_probe", e))?;
        let mut route_request = ModelRouteRequest::for_operation(ModelOperation::Chat);
        route_request.provider = Some(provider.clone());
        route_request.model = Some(model.clone());
        let network = self
            .network
            .clone()
            .unwrap_or_else(lingxi_adapters::models::network::NetworkPlane::direct_isolated);
        let (gateway, credentials, route) = if request.kind == ProviderProbeKind::ModelHealth {
            let route = shared
                .resolve_route(&route_request)
                .map_err(|e| self.error(400, "model_not_found", e.to_string()))?;
            (
                shared.clone(),
                self.credentials.clone().ok_or_else(|| {
                    self.error(503, "model_configuration_unavailable", "凭证出口未装配")
                })? as Arc<dyn ProviderCredentialPort>,
                route,
            )
        } else if matches!(
            plan,
            crate::provider_probe::ProviderProbePlan::Skipped { .. }
                | crate::provider_probe::ProviderProbePlan::Unavailable { .. }
                | crate::provider_probe::ProviderProbePlan::DeclaredCatalog
        ) {
            // 不发送分支不需要伪造端点或凭证。
            return Ok(PreparedProviderProbe {
                request,
                provider,
                model,
                protocol,
                endpoint,
                plan,
                route: None,
                gateway: shared.clone(),
                shared,
                generation,
                credentials: None,
                network,
                storage: self.storage.clone(),
                quotas: self.quotas.clone(),
                clock: self.clock.clone(),
                home: self.home.clone(),
                cache_lock: Arc::clone(&self.cache_lock),
                principal: crate::sessions::kernel_principal_of(principal),
                authority: self.authority(principal),
            });
        } else {
            provider_config["models"] = json!([{"id":model}]);
            let config = ModelPlaneConfig::from_value(
                json!({"providers":{(provider.clone()):provider_config},"models":{}}),
            )
            .map_err(|e| self.error(400, "invalid_probe", e.to_string()))?;
            let probe_gateway = Arc::new(ConfigModelGateway::from_validated_generation(
                config.clone(),
                if request_draft { 1 } else { generation },
            ));
            let port: Arc<dyn ProviderCredentialPort> = if request_draft {
                Arc::new(
                    crate::credentials::CredentialService::bootstrap_with_network(
                        &config,
                        &self.runtime_dir,
                        self.clock.clone(),
                        Some(network.clone()),
                    )
                    .map_err(|_| self.error(400, "invalid_probe", "草稿凭证初始化失败"))?,
                )
            } else {
                self.credentials.clone().ok_or_else(|| {
                    self.error(503, "model_configuration_unavailable", "凭证出口未装配")
                })?
            };
            let route = probe_gateway
                .resolve_route(&route_request)
                .map_err(|e| self.error(400, "invalid_probe", e.to_string()))?;
            (probe_gateway, port, route)
        };
        Ok(PreparedProviderProbe {
            request,
            provider,
            model,
            protocol,
            endpoint,
            plan,
            route: Some(route),
            gateway,
            shared,
            generation,
            credentials: Some(credentials),
            network,
            storage: self.storage.clone(),
            quotas: self.quotas.clone(),
            clock: self.clock.clone(),
            home: self.home.clone(),
            cache_lock: Arc::clone(&self.cache_lock),
            principal: crate::sessions::kernel_principal_of(principal),
            authority: self.authority(principal),
        })
    }
}

fn has_secret_edit(value: &Value) -> bool {
    let mut paths = Vec::new();
    crate::agents_admin::agent_config_readback::collect_secret_patch_paths(value, "", &mut paths);
    if !paths.is_empty() {
        return true;
    }
    // 沿用原有 authHeader.value 的秘密写权限判断。
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            (key == "value" && value.as_str().is_none_or(|s| s.trim() != "********"))
                || has_secret_edit(value)
        }),
        Value::Array(items) => items.iter().any(has_secret_edit),
        _ => false,
    }
}

/// 遮盖占位符只能取同一对象、同一路径的旧值，不能将星号写进来源。
fn resolve_masked_metadata(patch: &Value, old: &Value) -> Value {
    match patch {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let previous = old.get(key).unwrap_or(&Value::Null);
                    let next = if crate::agents_admin::agent_config_readback::is_secret_key(key) {
                        if value.as_str().is_some_and(|text| text.trim() == "********") {
                            previous
                                .as_str()
                                .map(|text| json!(text))
                                .unwrap_or(json!(""))
                        } else {
                            value.clone()
                        }
                    } else if matches!(key.as_str(), "headers" | "api_keys" | "search_api_keys") {
                        match value.as_object() {
                            Some(entries) => Value::Object(
                                entries
                                    .iter()
                                    .map(|(name, secret)| {
                                        let saved = previous.get(name).unwrap_or(&Value::Null);
                                        let secret = if secret
                                            .as_str()
                                            .is_some_and(|text| text.trim() == "********")
                                        {
                                            saved
                                                .as_str()
                                                .map(|text| json!(text))
                                                .unwrap_or(json!(""))
                                        } else {
                                            secret.clone()
                                        };
                                        (name.clone(), secret)
                                    })
                                    .collect(),
                            ),
                            None => value.clone(),
                        }
                    } else {
                        resolve_masked_metadata(value, previous)
                    };
                    (key.clone(), next)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    resolve_masked_metadata(item, old.get(index).unwrap_or(&Value::Null))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}
/// 仅插件明示的 Codex 显示身份和执行身份相互定位，保留现有 Source 的实际键。
fn provider_in_document(document: &Value, requested: &str) -> String {
    if document["providers"].get(requested).is_some() {
        return requested.into();
    }
    match requested {
        "openai-codex-oauth" => "openai-codex".into(),
        "openai-codex" if document["providers"].get("openai-codex-oauth").is_some() => {
            "openai-codex-oauth".into()
        }
        _ => requested.into(),
    }
}
fn canonical_changes(
    document: &Value,
    changes: &[ModelSettingsChange],
) -> Vec<ModelSettingsChange> {
    changes
        .iter()
        .cloned()
        .map(|change| match change {
            ModelSettingsChange::ProviderPatch { provider, patch } => {
                ModelSettingsChange::ProviderPatch {
                    provider: provider_in_document(document, &provider),
                    patch,
                }
            }
            ModelSettingsChange::ProviderRemove { provider } => {
                ModelSettingsChange::ProviderRemove {
                    provider: provider_in_document(document, &provider),
                }
            }
            ModelSettingsChange::ModelPatch {
                provider,
                model,
                patch,
            } => ModelSettingsChange::ModelPatch {
                provider: provider_in_document(document, &provider),
                model,
                patch,
            },
            ModelSettingsChange::ModelRemove { provider, model } => {
                ModelSettingsChange::ModelRemove {
                    provider: provider_in_document(document, &provider),
                    model,
                }
            }
            ModelSettingsChange::SetBinding {
                operation,
                selection,
            } => ModelSettingsChange::SetBinding {
                operation,
                selection: selection.map(|mut selection| {
                    selection.provider = provider_in_document(document, &selection.provider);
                    selection
                }),
            },
        })
        .collect()
}
fn apply_changes(
    document: &mut Value,
    changes: &[ModelSettingsChange],
    cleared: &mut Vec<String>,
) -> Result<(), String> {
    if !document.is_object() {
        return Err("配置必须是对象".into());
    }
    if document.get("providers").is_none() {
        document["providers"] = json!({});
    }
    if document.get("models").is_none() {
        document["models"] = json!({});
    }
    let legacy = document["providers"]
        .as_object()
        .ok_or("供应商表损坏")?
        .iter()
        .filter(|(_, p)| p.get("models").is_none())
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for provider in legacy {
        let mut models = Vec::new();
        for binding in document["models"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, binding)| binding)
            .filter(|binding| binding["provider"] == provider)
        {
            let mut model = binding.as_object().cloned().ok_or("旧模型绑定损坏")?;
            let id = model.remove("model").ok_or("旧模型编号缺失")?;
            model.remove("provider");
            model.insert("id".into(), id);
            if !models.iter().any(|m: &Value| m["id"] == model["id"]) {
                models.push(Value::Object(model));
            }
        }
        document["providers"][&provider]["models"] = json!(models);
    }
    for change in changes {
        match change {
            ModelSettingsChange::ProviderPatch { provider, patch } => {
                if provider.trim().is_empty() {
                    return Err("供应商编号不能为空".into());
                }
                let existing = document["providers"]
                    .get(provider)
                    .cloned()
                    .unwrap_or(json!({}));
                document["providers"][provider] = normalize_provider(provider, &existing, patch)?;
            }
            ModelSettingsChange::ProviderRemove { provider } => {
                document["providers"]
                    .as_object_mut()
                    .ok_or("供应商表损坏")?
                    .remove(provider);
            }
            ModelSettingsChange::ModelPatch {
                provider,
                model,
                patch,
            } => {
                let entry = document["providers"]
                    .get_mut(provider)
                    .ok_or("供应商不存在")?;
                if entry.get("models").is_none() {
                    entry["models"] = json!([]);
                }
                let models = entry["models"].as_array_mut().ok_or("模型目录损坏")?;
                let index = models
                    .iter()
                    .position(|m| m.as_str() == Some(model) || m["id"] == model.as_str());
                let old = index
                    .map(|i| models[i].clone())
                    .unwrap_or(json!({"id":model}));
                let next = normalize_model(model, &old, patch)?;
                if let Some(index) = index {
                    models[index] = next;
                } else {
                    models.push(next);
                }
            }
            ModelSettingsChange::ModelRemove { provider, model } => {
                if let Some(entry) = document["providers"].get_mut(provider) {
                    if let Some(models) = entry["models"].as_array_mut() {
                        models.retain(|m| m.as_str() != Some(model) && m["id"] != model.as_str());
                    } else {
                        entry["models"] = json!([]);
                    }
                }
            }
            ModelSettingsChange::SetBinding {
                operation,
                selection,
            } => {
                document["models"][operation_key(*operation)] = selection
                    .as_ref()
                    .map(|m| json!({"provider":m.provider,"model":m.model}))
                    .unwrap_or(Value::Null);
            }
        }
    }
    let providers = document["providers"].as_object().ok_or("供应商表损坏")?;
    let stale = document["models"]
        .as_object()
        .ok_or("模型绑定表损坏")?
        .iter()
        .filter_map(|(slot, binding)| {
            if binding.is_null() {
                return None;
            }
            let provider = binding["provider"].as_str().unwrap_or("");
            let model = binding["model"].as_str().unwrap_or("");
            let valid = providers.get(provider).is_some_and(|p| {
                p.get("models").is_none_or(|models| {
                    models
                        .as_array()
                        .is_some_and(|list| list.iter().any(|m| m["id"] == model))
                })
            });
            (!valid).then(|| slot.clone())
        })
        .collect::<Vec<_>>();
    for change in changes {
        if let ModelSettingsChange::SetBinding {
            operation,
            selection: Some(_),
        } = change
        {
            if stale.contains(&operation_key(*operation)) {
                return Err("选择的模型不在有效目录中".into());
            }
        }
    }
    for slot in stale {
        document["models"][&slot] = Value::Null;
        cleared.push(slot);
    }
    Ok(())
}
fn operation_key(operation: ModelOperation) -> String {
    if operation == ModelOperation::SpeechRecognition {
        return "speechRecognition".into();
    }
    let description = operation.describe();
    description
        .strip_prefix("auxiliary.")
        .unwrap_or(&description)
        .to_string()
}
fn normalize_provider(id: &str, old: &Value, patch: &Value) -> Result<Value, String> {
    let patch = patch.as_object().ok_or("供应商补丁必须是对象")?;
    let definition = crate::provider_probe::builtin_provider_definition(id).unwrap_or(json!({}));
    let mut out = old.as_object().cloned().unwrap_or_default();
    let mut metadata = out
        .get("metadata")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for field in ["protocol", "api"] {
        if let Some(value) = patch.get(field) {
            let protocol = value.as_str().ok_or("供应商协议必须是字符串")?;
            if lingxi_kernel::model_exchange::ProtocolFamily::parse(protocol).is_none() {
                return Err("供应商协议无效".into());
            }
        }
    }
    for field in ["endpoint", "base_url"] {
        if patch.get(field).is_some_and(|value| !value.is_string()) {
            return Err(format!("{field}必须是字符串"));
        }
    }
    let protocol = patch
        .get("protocol")
        .or_else(|| patch.get("api"))
        .and_then(Value::as_str)
        .or_else(|| old["protocol"].as_str())
        .or_else(|| definition["api"].as_str())
        // 旧自定义供应商创建请求省略api时，沿用其既有OpenAI兼容协议默认。
        .unwrap_or("openai-completions");
    let endpoint = patch
        .get("endpoint")
        .or_else(|| patch.get("base_url"))
        .and_then(Value::as_str)
        .or_else(|| old["endpoint"].as_str())
        .or_else(|| definition["baseUrl"].as_str())
        .unwrap_or("");
    out.insert("protocol".into(), json!(protocol));
    out.insert("endpoint".into(), json!(endpoint));
    let mut auth = patch
        .get("auth")
        .map(|auth| resolve_masked_metadata(auth, &old["auth"]))
        .or_else(|| old.get("auth").cloned())
        .unwrap_or_else(|| json!({"kind":"none"}));
    if auth["kind"] == "authHeader"
        && auth["value"]
            .as_str()
            .is_some_and(|value| value.trim() == "********")
    {
        let same_header = auth["header"]
            .as_str()
            .zip(old["auth"]["header"].as_str())
            .is_some_and(|(next, previous)| next.eq_ignore_ascii_case(previous));
        if old["auth"]["kind"] != "authHeader" || !same_header {
            return Err("遮盖的请求头凭证没有对应原值".into());
        }
        auth["value"] = old["auth"]["value"].clone();
    }
    if let Some(key) = patch.get("api_key") {
        let key = key.as_str().ok_or("api_key必须是字符串")?;
        if key.trim() != "********" {
            auth = if key.is_empty() {
                json!({"kind":"none"})
            } else {
                json!({"kind":"apiKey","apiKey":key})
            };
        }
    }
    out.insert("auth".into(), auth);
    if let Some(headers) = patch.get("headers") {
        // null保留既有显式清空语义；损坏类型不能被误当成空映射。
        if !headers.is_null() && !headers.is_object() {
            return Err("headers必须是对象或null".into());
        }
        let old_headers = old["headers"].as_object();
        let mut normalized = serde_json::Map::new();
        for (name, value) in headers.as_object().into_iter().flatten() {
            let name = name.trim();
            if value.is_null() {
                continue;
            }
            let value = match value {
                Value::String(value) => value.clone(),
                Value::Number(_) | Value::Bool(_) => value.to_string(),
                _ => return Err("header值必须是字符串或简单标量".into()),
            };
            if value.is_empty() {
                continue;
            }
            let value = if value.trim() == "********" {
                let Some(saved) = old_headers
                    .and_then(|map| map.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)))
                    .and_then(|(_, value)| value.as_str())
                else {
                    continue;
                };
                saved.to_string()
            } else {
                value
            };
            normalized.insert(name.to_ascii_lowercase(), json!(value));
        }
        out.insert("headers".into(), Value::Object(normalized));
    }
    if let Some(models) = patch.get("models") {
        let models = models
            .as_array()
            .ok_or("models必须是列表")?
            .iter()
            .map(|model| {
                let id = model
                    .as_str()
                    .or_else(|| model["id"].as_str())
                    .ok_or("模型编号缺失")?;
                // 按模型编号找原值；目录重排不能串用其他模型的秘密。
                let previous = old["models"]
                    .as_array()
                    .and_then(|models| models.iter().find(|saved| saved["id"] == id))
                    .unwrap_or(&Value::Null);
                normalize_model(
                    id,
                    previous,
                    if model.is_string() {
                        &Value::Null
                    } else {
                        model
                    },
                )
            })
            .collect::<Result<Vec<_>, String>>()?;
        out.insert("models".into(), json!(models));
    } else if !out.contains_key("models") {
        out.insert("models".into(), json!([]));
    }
    for (key, value) in patch {
        if !matches!(
            key.as_str(),
            "api"
                | "protocol"
                | "endpoint"
                | "base_url"
                | "auth"
                | "api_key"
                | "headers"
                | "models"
                | "seed_default_models"
        ) {
            metadata.insert(
                key.clone(),
                resolve_masked_metadata(value, &old["metadata"][key]),
            );
        }
    }
    out.insert(
        "metadata".into(),
        resolve_masked_metadata(&Value::Object(metadata), &old["metadata"]),
    );
    Ok(Value::Object(out))
}
fn normalize_model(id: &str, old: &Value, patch: &Value) -> Result<Value, String> {
    let mut out = old.as_object().cloned().unwrap_or_default();
    if !patch.is_null() {
        for (key, value) in patch.as_object().ok_or("模型补丁必须是对象")? {
            if matches!(
                key.to_ascii_lowercase().as_str(),
                "auth"
                    | "headers"
                    | "api_key"
                    | "apikey"
                    | "base_url"
                    | "endpoint"
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
                    | "resourceurl"
                    | "resource_url"
                    | "expires"
            ) {
                return Err("模型元数据不能携带凭证或地址".into());
            }
            if key == "outputIncludesThinking" && value.is_null() {
                out.remove(key);
            } else {
                out.insert(key.clone(), resolve_masked_metadata(value, &old[key]));
            }
        }
    }
    if id.trim().is_empty() {
        return Err("模型编号不能为空".into());
    }
    out.insert("id".into(), json!(id));
    for (alias, key) in [
        ("contextWindow", "context"),
        ("maxTokens", "maxOutput"),
        ("maxOutputTokens", "maxOutput"),
    ] {
        if patch.get(key).is_none() {
            if let Some(value) = patch.get(alias).cloned() {
                out.insert(key.into(), value);
            }
        }
    }
    for key in ["context", "maxOutput", "dimensions"] {
        if let Some(value) = out.get(key) {
            if !value.as_f64().is_some_and(|n| n.is_finite() && n > 0.0) {
                return Err(format!("{key}必须为正数"));
            }
        }
    }
    if let Some(protocol) = out
        .get("operationProtocol")
        .or_else(|| out.get("api"))
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        out.insert("protocol".into(), json!(protocol));
    }
    for field in ["inputs", "outputs"] {
        if let Some(value) = out.get(field) {
            let items = value
                .as_array()
                .ok_or_else(|| format!("{field}必须为列表"))?;
            if items.is_empty()
                || !items.iter().all(|value| {
                    value
                        .as_str()
                        .is_some_and(|value| ["text", "image", "video", "audio"].contains(&value))
                })
            {
                return Err(format!("{field}必须是非空且有效的模态列表"));
            }
            let normalized = ["text", "image", "video", "audio"]
                .into_iter()
                .filter(|item| items.iter().any(|value| value.as_str() == Some(item)))
                .collect::<Vec<_>>();
            out.insert(field.into(), json!(normalized));
        }
    }
    if let Some(inputs) = out.get("inputs").cloned() {
        out.insert("input".into(), inputs);
        for key in ["image", "video", "audio", "vision"] {
            out.remove(key);
        }
    }
    if let Some(contract) = out.get("toolUse") {
        let contract = contract.as_object().ok_or("toolUse必须为对象")?;
        let support = contract
            .get("supportsTools")
            .and_then(Value::as_bool)
            .ok_or("toolUse.supportsTools必须为布尔值")?;
        let dialect = contract
            .get("dialect")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let format = contract
            .get("toolResultFormat")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !["openai", "anthropic", "gemini", "mistral", "none"].contains(&dialect.as_str())
            || !["message", "content_block", "part"].contains(&format.as_str())
        {
            return Err("toolUse包含无效的调用或结果格式".into());
        }
        let mut normalized =
            json!({"supportsTools":support,"dialect":dialect,"toolResultFormat":format});
        for field in [
            "supportsParallelToolCalls",
            "supportsForcedToolChoice",
            "supportsServerTools",
        ] {
            if let Some(value) = contract.get(field) {
                if !value.is_boolean() {
                    return Err(format!("toolUse.{field}必须为布尔值"));
                }
                normalized[field] = value.clone();
            }
        }
        out.insert("toolUse".into(), normalized);
    }
    let image = out
        .get("input")
        .and_then(Value::as_array)
        .map(|i| i.iter().any(|v| v == "image"))
        .or_else(|| {
            out.get("image")
                .or_else(|| out.get("vision"))
                .and_then(Value::as_bool)
        });
    let tools = out
        .get("toolUse")
        .and_then(|v| v.get("supportsTools"))
        .and_then(Value::as_bool);
    if image.is_some() || tools.is_some() {
        out.insert(
            "capabilities".into(),
            json!({"imageInput":image,"tools":tools}),
        );
    }
    let mut compat = out
        .get("compat")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if patch
        .get("outputIncludesThinking")
        .is_some_and(Value::is_null)
    {
        compat.remove("outputIncludesThinking");
    }
    for (from, to) in [
        ("context", "contextWindow"),
        ("maxOutput", "maxTokens"),
        ("reasoning", "reasoning"),
        ("thinkingLevelMap", "thinkingLevelMap"),
        ("outputIncludesThinking", "outputIncludesThinking"),
    ] {
        if let Some(value) = out.get(from) {
            compat.insert(to.into(), value.clone());
        }
    }
    if !compat.is_empty() {
        out.insert("compat".into(), Value::Object(compat));
    } else {
        out.remove("compat");
    }
    Ok(Value::Object(out))
}

/// 一次受限探测的真实授权快照。材料和执行器不离开本模块。
struct PreparedProviderProbe {
    request: ProviderProbeRequest,
    provider: String,
    model: String,
    protocol: String,
    endpoint: String,
    plan: crate::provider_probe::ProviderProbePlan,
    route: Option<lingxi_kernel::model_exchange::ResolvedModelRoute>,
    gateway: Arc<lingxi_adapters::models::gateway::ConfigModelGateway>,
    shared: Arc<lingxi_adapters::models::gateway::ConfigModelGateway>,
    generation: u64,
    credentials: Option<Arc<dyn lingxi_adapters::models::credentials::ProviderCredentialPort>>,
    network: Arc<lingxi_adapters::models::network::NetworkPlane>,
    storage: Arc<lingxi_adapters::storage::RunDatabase>,
    quotas: Arc<crate::quotas::QuotaManager>,
    clock: Arc<dyn crate::inject::ServiceClock>,
    home: PathBuf,
    principal: lingxi_kernel::Principal,
    authority: ModelAuthority,
    cache_lock: Arc<std::sync::Mutex<()>>,
}
impl PreparedProviderProbe {
    fn error(&self, status: u16, code: &str, message: impl Into<String>) -> ModelApplyError {
        ModelApplyError {
            status,
            code: code.into(),
            message: message.into(),
            report: ModelApplyReport {
                generation: self.generation,
                ..Default::default()
            },
        }
    }
    fn current(&self) -> Result<(), ModelApplyError> {
        if !self.authority.valid() {
            return Err(self.error(
                403,
                "model_authority_revoked",
                "供应商探测原始身份已撤销、过期或权限已改变",
            ));
        }
        if self.shared.upcoming_generation().saturating_sub(1) != self.generation {
            Err(self.error(409, "stale_probe", "探测准备后模型配置已改变，请重新操作"))
        } else {
            Ok(())
        }
    }
    async fn execute(self) -> Result<ProviderProbeReport, ModelApplyError> {
        use crate::provider_probe::ProviderProbePlan;
        self.current()?;
        match self.plan.clone() {
            ProviderProbePlan::Skipped { reason } => Ok(ProviderProbeReport {
                value: json!({"ok":true,"status":0,"skipped":true,"reason":reason}),
            }),
            ProviderProbePlan::Unavailable { reason } => Ok(ProviderProbeReport {
                value: json!({"ok":false,"status":0,"unavailable":true,"error":reason}),
            }),
            ProviderProbePlan::DeclaredCatalog => {
                let definition = crate::provider_probe::builtin_provider_definition(&self.provider)
                    .unwrap_or(json!({}));
                let models = if self.shared.provider_ids().contains(&self.provider) {
                    self.shared
                        .catalog_models()
                        .get(&self.provider)
                        .map(|models| models.iter().map(|model| json!(model)).collect::<Vec<_>>())
                        .unwrap_or_default()
                } else {
                    definition["models"].as_array().cloned().unwrap_or_default()
                };
                self.cache_catalog(&models, &[])?;
                Ok(ProviderProbeReport {
                    value: json!({"models":models,"source":"hana-catalog"}),
                })
            }
            ProviderProbePlan::Generation {
                prompt,
                max_tokens,
                timeout_ms,
                purpose,
            } => self.generate(prompt, max_tokens, timeout_ms, purpose).await,
            ProviderProbePlan::ControlPlane {
                method,
                url,
                body,
                timeout_ms,
            } => {
                let result = tokio::time::timeout(
                    std::time::Duration::from_millis(timeout_ms),
                    self.control(method, &url, body),
                )
                .await
                .map_err(|_| self.error(504, "probe_timeout", "供应商探测已超过总时限"))??;
                Ok(ProviderProbeReport { value: result })
            }
        }
    }
    fn cache_catalog(&self, models: &[Value], ignored: &[String]) -> Result<(), ModelApplyError> {
        let _lock = self.cache_lock.lock().unwrap_or_else(|p| p.into_inner());
        self.current()?;
        let path = self.home.join("models-cache.json");
        let before = if path
            .try_exists()
            .map_err(|_| self.error(500, "discovery_cache_unreadable", "发现缓存不可读取"))?
        {
            Some(crate::file_safety::Snapshot::read(&path).map_err(|_| {
                self.error(
                    409,
                    "discovery_cache_invalid",
                    "发现缓存损坏、过大或已被替换，未覆盖原件",
                )
            })?)
        } else {
            None
        };
        let mut cache = match &before {
            Some(snapshot) => serde_json::from_slice::<Value>(&snapshot.bytes).map_err(|_| {
                self.error(
                    409,
                    "discovery_cache_invalid",
                    "已有发现缓存损坏，未覆盖原件",
                )
            })?,
            None => json!({}),
        };
        if !cache.is_object() {
            return Err(self.error(
                409,
                "discovery_cache_invalid",
                "发现缓存必须是对象，未覆盖原件",
            ));
        }
        cache[&self.provider] =
            json!({"models":models,"ignoredModels":ignored,"fetchedAt":self.clock.now_unix_ms()});
        let bytes = serde_json::to_vec_pretty(&cache)
            .map_err(|_| self.error(500, "discovery_cache_failed", "发现缓存编码失败"))?;
        let safety = crate::file_safety::FileSafety::new(&self.home);
        let result = match &before {
            Some(snapshot) => {
                safety.replace("provider-discovery-cache", &path, snapshot, &bytes, || {
                    self.current().is_ok()
                })
            }
            None => safety.create("provider-discovery-cache", &path, &bytes, || {
                self.current().is_ok()
            }),
        };
        result.map_err(|_| {
            self.error(
                500,
                "discovery_cache_failed",
                "目录已获取，但发现缓存保存未确认",
            )
        })
    }
    async fn request_control(
        &self,
        method: &str,
        url: &str,
        body: Option<Value>,
        auth: &lingxi_adapters::models::credentials::ApplicableAuth,
    ) -> Result<(u16, String, Vec<u8>), ModelApplyError> {
        use lingxi_adapters::models::dispatch::{apply_auth, BearerStyle, HttpTimeouts};
        self.current()?;
        let target = reqwest::Url::parse(url)
            .map_err(|_| self.error(400, "invalid_probe_target", "探测地址无效"))?;
        let origin = reqwest::Url::parse(&self.endpoint)
            .map_err(|_| self.error(400, "invalid_probe_target", "供应商地址无效"))?;
        if target.origin() != origin.origin() || !matches!(method, "GET" | "POST") {
            return Err(self.error(
                400,
                "invalid_probe_target",
                "探测只能访问已授权供应商的同一地址来源",
            ));
        }
        let client = self
            .network
            .client_handle(HttpTimeouts::default())
            .and_then(|client| client.client())
            .map_err(|_| {
                self.error(503, "probe_network_unavailable", "模型网络策略客户端不可用")
            })?;
        let mut request = if method == "GET" {
            client.get(target)
        } else {
            client.post(target)
        };
        if let Some(body) = body {
            request = request.json(&body);
        }
        if self.protocol == "anthropic-messages" {
            request = request.header("anthropic-version", "2023-06-01");
        }
        let style = match self.protocol.as_str() {
            "anthropic-messages" => BearerStyle::NamedHeader("x-api-key"),
            "google-generative-ai" => BearerStyle::NamedHeader("x-goog-api-key"),
            _ => BearerStyle::AuthorizationBearer,
        };
        let response = apply_auth(request, auth, style)
            .send()
            .await
            .map_err(|error| {
                self.error(
                    502,
                    "probe_transport_failed",
                    lingxi_adapters::models::dispatch::scrubbed_excerpt(&error.to_string(), auth),
                )
            })?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if response
            .content_length()
            .is_some_and(|n| n > 4 * 1024 * 1024)
        {
            return Err(self.error(502, "probe_response_too_large", "供应商目录响应超过4MiB"));
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| self.error(502, "probe_body_failed", "供应商响应读取失败"))?
        {
            if bytes.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
                return Err(self.error(502, "probe_response_too_large", "供应商目录响应超过4MiB"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((status, content_type, bytes))
    }
    async fn control(
        &self,
        method: &str,
        url: &str,
        body: Option<Value>,
    ) -> Result<Value, ModelApplyError> {
        let auth = self
            .credentials
            .as_ref()
            .expect("prepared credentials")
            .resolve(self.route.as_ref().expect("prepared route"))
            .await
            .map_err(|_| {
                self.error(
                    401,
                    "probe_credential_unavailable",
                    "供应商凭证不可用或需要重新登录",
                )
            })?;
        self.current()?;
        let (status, content_type, bytes) = self.request_control(method, url, body, &auth).await?;
        if self.request.kind == ProviderProbeKind::Connection {
            let clean = lingxi_adapters::models::dispatch::scrubbed_excerpt(
                &String::from_utf8_lossy(&bytes),
                &auth,
            );
            let visible = if (200..300).contains(&status) {
                bytes
            } else {
                clean.into_bytes()
            };
            return Ok(crate::provider_probe::parse_connection_response(
                status,
                &content_type,
                &visible,
            ));
        }
        if !(200..300).contains(&status) {
            return Err(self.error(
                400,
                "provider_catalog_failed",
                format!(
                    "供应商目录返回HTTP {status}：{}",
                    lingxi_adapters::models::dispatch::scrubbed_excerpt(
                        &String::from_utf8_lossy(&bytes),
                        &auth
                    )
                ),
            ));
        }
        let value = serde_json::from_slice::<Value>(&bytes)
            .map_err(|_| self.error(502, "provider_catalog_invalid", "供应商目录不是有效JSON"))?;
        let discovery = crate::provider_probe::parse_catalog(
            &self.provider,
            &self.protocol,
            &self.endpoint,
            &value,
        )
        .map_err(|e| self.error(502, "provider_catalog_invalid", e))?;
        let mut models = discovery.models;
        if self.provider == "ollama" {
            use futures::StreamExt;
            let candidates = models
                .iter()
                .enumerate()
                .take(200)
                .filter_map(|(index, model)| model["id"].as_str().map(|id| (index, id.to_string())))
                .collect::<Vec<_>>();
            let results = futures::stream::iter(candidates.into_iter().map(|(index, id)| {
                let auth = &auth;
                async move {
                    let plan =
                        crate::provider_probe::build_ollama_detail_plan(&self.endpoint, &id)?;
                    let crate::provider_probe::ProviderProbePlan::ControlPlane {
                        method,
                        url,
                        body,
                        ..
                    } = plan
                    else {
                        return Err("Ollama detail plan is not control-plane".to_string());
                    };
                    let (status, _, bytes) = self
                        .request_control(method, &url, body, auth)
                        .await
                        .map_err(|e| e.message)?;
                    if !(200..300).contains(&status) {
                        return Err(format!("Ollama模型{id}详情返回HTTP {status}"));
                    }
                    serde_json::from_slice::<Value>(&bytes)
                        .map(|detail| (index, detail))
                        .map_err(|_| "Ollama详情不是有效JSON".to_string())
                }
            }))
            .buffer_unordered(6)
            .collect::<Vec<_>>()
            .await;
            let mut diagnostics = Vec::new();
            for result in results {
                match result {
                    Ok((index, detail)) => {
                        crate::provider_probe::merge_ollama_details(&mut models[index], &detail)
                    }
                    Err(error) => diagnostics.push(error),
                }
            }
            self.cache_catalog(&models, &discovery.ignored_models)?;
            return Ok(
                json!({"models":models,"ignoredModels":discovery.ignored_models,"diagnostics":diagnostics}),
            );
        }
        self.cache_catalog(&models, &discovery.ignored_models)?;
        Ok(json!({"models":models,"ignoredModels":discovery.ignored_models}))
    }
    async fn generate(
        &self,
        prompt: &str,
        max_tokens: u32,
        timeout_ms: u64,
        purpose: &str,
    ) -> Result<ProviderProbeReport, ModelApplyError> {
        use lingxi_kernel::{
            model_exchange::{ModelTurnInput, ModelTurnOptions, ToolDeclarationSnapshot},
            ports::{ProviderTurn, StoragePort},
            usage::{CallOutcome, ModelCallUsageRecord, ReportedUsage},
        };
        let call_id = format!(
            "probe-{}",
            crate::auth::hex_random_public(16).map_err(|_| self.error(
                500,
                "probe_identity_failed",
                "探测身份创建失败"
            ))?
        );
        let ctx = lingxi_kernel::RunContext {
            principal: self.principal.clone(),
            session_id: lingxi_protocol::SessionId::new("settings-model-probe"),
            run_id: lingxi_protocol::RunId::new(call_id.clone()),
            attempt: lingxi_protocol::AttemptId::new(format!("{call_id}#1")),
            generation: 1,
        };
        let started = self.clock.now_unix_ms();
        let deadline = lingxi_adapters::models::dispatch::unix_ms_now().saturating_add(timeout_ms);
        let input = ModelTurnInput {
            submission: prompt.into(),
            system_prompt: None,
            turn: 1,
            prior: vec![],
            tools: ToolDeclarationSnapshot::empty(),
            deadline_unix_ms: Some(deadline),
            images: vec![],
            max_output_tokens: Some(max_tokens),
        };
        let options = Some(ModelTurnOptions {
            model: Some((self.provider.clone(), self.model.clone())),
            thinking_level: None,
        });
        let provider = lingxi_adapters::models::provider::GatewayedProvider::new_with_network(
            self.gateway.clone(),
            self.credentials.clone().expect("prepared credentials"),
            Default::default(),
            self.network.clone(),
        )
        .map_err(|_| self.error(503, "probe_adapter_unavailable", "真实模型协议执行器不可用"))?;
        let mut entered = false;
        let operation = async {
            let _permit = self
                .quotas
                .acquire(
                    crate::quotas::QuotaResource::Model,
                    "settings-model-probe",
                    &self.provider,
                )
                .await
                .map_err(|e| self.error(503, "probe_model_capacity", e.to_string()))?;
            self.current()?;
            entered = true;
            Ok::<_, ModelApplyError>(
                provider
                    .next_turn_for_operation_with_options(
                        &ctx,
                        &lingxi_protocol::ModelCallId::new(call_id.clone()),
                        ModelOperation::Chat,
                        &input,
                        &ProbeSink,
                        &options,
                    )
                    .await,
            )
        };
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), operation).await;
        let (report, attempts, outcome, response, served_protocol) = match result {
            Ok(Ok(result)) => {
                let ok = matches!(result.turn, ProviderTurn::Final { .. });
                let value = match &result.turn {
                    ProviderTurn::Final { .. } => {
                        json!({"ok":true,"status":200,"provider":self.provider})
                    }
                    ProviderTurn::Failed { error, .. } => {
                        json!({"ok":false,"status":0,"error":error.message,"code":error.code.wire_name()})
                    }
                    _ => json!({"ok":false,"status":0,"error":"探测未返回可用文本"}),
                };
                (
                    result.usage_report,
                    Some(result.transport_attempts),
                    if ok {
                        CallOutcome::Succeeded
                    } else {
                        CallOutcome::Failed
                    },
                    Ok(ProviderProbeReport { value }),
                    result
                        .served_protocol
                        .unwrap_or_else(|| self.protocol.clone()),
                )
            }
            Ok(Err(error)) => (
                ReportedUsage::Unknown,
                Some(0),
                CallOutcome::Failed,
                Err(error),
                self.protocol.clone(),
            ),
            Err(_) => (
                ReportedUsage::Unknown,
                if entered { None } else { Some(0) },
                CallOutcome::Cancelled,
                Err(self.error(
                    504,
                    "probe_timeout",
                    "探测超时，已停止本地等待；远端是否生成结果无法确认",
                )),
                self.protocol.clone(),
            ),
        };
        let row = ModelCallUsageRecord {
            session_id: None,
            run_id: None,
            attempt: None,
            model_call_id: call_id,
            purpose: purpose.into(),
            origin: "health_check".into(),
            parent_run_id: None,
            cause_ref: Some("auxiliary/model_health/settings/user".into()),
            parent_tool_call_id: None,
            provider: self.provider.clone(),
            model: self.model.clone(),
            protocol: served_protocol,
            usage: match &report {
                ReportedUsage::Known(usage) => Some(usage.clone()),
                _ => None,
            },
            invalid_detail: match report {
                ReportedUsage::Invalid { detail } => Some(detail),
                _ => None,
            },
            transport_attempts: attempts,
            outcome,
            started_at_unix_ms: Some(started),
            settled_at_unix_ms: Some(self.clock.now_unix_ms()),
            emitted_tool_calls: vec![],
            cost_basis: None,
        };
        self.storage
            .record_model_call_usage(row, self.clock.now_unix_ms())
            .await
            .map_err(|_| {
                self.error(
                    500,
                    "probe_usage_unconfirmed",
                    "真实生成可能已执行，但调用用量保存未确认",
                )
            })?;
        response
    }
}
struct ProbeSink;
impl lingxi_kernel::ports::TurnDeltaSink for ProbeSink {
    fn emit<'a>(
        &'a self,
        _delta: lingxi_kernel::ports::ModelTurnDelta,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(), lingxi_kernel::ports::TurnDeltaSinkClosed>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(()) })
    }
}
