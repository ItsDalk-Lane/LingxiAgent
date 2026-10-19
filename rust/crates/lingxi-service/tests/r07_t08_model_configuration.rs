//! 真实配置来源、凭证与网关快照的同批发布；模型请求只到自有回环响应端。
use lingxi_adapters::models::{
    config::ModelPlaneConfig, credentials::ProviderCredentialPort, gateway::ConfigModelGateway,
    network::NetworkPlane,
};
use lingxi_kernel::model_exchange::{
    AuxiliarySlot, ModelGatewayPort, ModelOperation, ModelRouteRequest,
};
use lingxi_service::{
    config::ModelPlaneSource,
    model_configuration::{ModelSelection, ModelSettingsChange},
    prepare_layout, HomeSource, NetworkMode, Principal, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

struct Harness {
    home: PathBuf,
    source: PathBuf,
    state: ServiceState,
    gateway: Arc<ConfigModelGateway>,
    credentials: Arc<lingxi_service::credentials::CredentialService>,
    principal: Principal,
}
impl Harness {
    async fn boot(embedded: bool) -> Self {
        Self::boot_source(embedded, false).await
    }
    async fn boot_source(embedded: bool, existing_models_file: bool) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lingxi-r07-t08-model-core-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(&home).unwrap();
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let source = if embedded {
            home.join("service.json")
        } else {
            layout.runtime_dir.join("models.json")
        };
        let document = if embedded {
            json!({"providers":{},"models":{},"home":home,"workspace":home})
        } else {
            json!({"providers":{},"models":{}})
        };
        if embedded || existing_models_file {
            std::fs::write(&source, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        }
        let plane = ModelPlaneConfig::from_value(json!({"providers":{},"models":{}})).unwrap();
        let gateway = Arc::new(ConfigModelGateway::from_validated(plane.clone()));
        let clock = Arc::new(lingxi_service::inject::SystemClock);
        let credentials = Arc::new(
            lingxi_service::credentials::CredentialService::bootstrap(
                &plane,
                &layout.runtime_dir,
                clock.clone(),
            )
            .unwrap(),
        );
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
            ServiceDeps {
                model_gateway: Some(gateway.clone()),
                credential_service: Some(credentials.clone()),
                network_plane: Some(NetworkPlane::direct_isolated()),
                model_plane_activation: Some(Arc::new(AtomicBool::new(false))),
                model_plane_source: if embedded {
                    Some(ModelPlaneSource::EmbeddedInConfigFile {
                        path: source.clone(),
                    })
                } else if existing_models_file {
                    Some(ModelPlaneSource::ModelsFile {
                        path: source.clone(),
                    })
                } else {
                    None
                },
                clock,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let principal = state
            .auth()
            .authenticate(
                Some(&format!("Bearer {}", state.auth().local_token())),
                None,
                false,
                lingxi_service::ConnectionKind::Local,
            )
            .unwrap();
        Self {
            home,
            source,
            state,
            gateway,
            credentials,
            principal,
        }
    }
    async fn close(self) {
        self.state.storage().close().await.unwrap();
        drop(self.state);
        std::fs::remove_dir_all(self.home).unwrap();
    }
    async fn save(
        &self,
        changes: &[ModelSettingsChange],
    ) -> lingxi_service::model_configuration::ModelApplyReport {
        self.state
            .model_configuration()
            .save(&self.principal, changes)
            .await
            .unwrap()
    }
    async fn seed(&self) {
        self.save(&[ModelSettingsChange::ProviderPatch{provider:"fixture".into(),patch:json!({"api":"openai-completions","base_url":"http://127.0.0.1:9/v1","api_key":"primary-secret","headers":{"X-Custom":"custom-secret"},"models":[{"id":"summary","inputs":["text"],"toolUse":{"supportsTools":true,"dialect":"openai","toolResultFormat":"message"}},{"id":"knowledge","api":"anthropic-messages","inputs":["text"],"contextWindow":8192,"maxTokens":256,"extension":{"kept":true}}]})}]).await;
    }
}
fn binding(operation: ModelOperation, model: &str) -> ModelSettingsChange {
    ModelSettingsChange::SetBinding {
        operation,
        selection: Some(ModelSelection {
            provider: "fixture".into(),
            model: model.into(),
        }),
    }
}
fn pinned(model: &str) -> ModelRouteRequest {
    let mut r = ModelRouteRequest::for_operation(ModelOperation::Chat);
    r.provider = Some("fixture".into());
    r.model = Some(model.into());
    r
}

#[cfg(unix)]
#[tokio::test]
async fn existing_embedded_and_models_file_sources_and_retained_originals_become_private_when_saving_first_key(
) {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    for embedded in [true, false] {
        let h = Harness::boot_source(embedded, true).await;
        assert!(h.state.model_configuration().source().is_some());
        let mut document: Value =
            serde_json::from_slice(&std::fs::read(&h.source).unwrap()).unwrap();
        // 顶层配置是封闭合同；扩展内容位于现役允许的供应商metadata内。
        document["providers"]["private-fixture"] = json!({
            "protocol":"openai-completions", "endpoint":"http://127.0.0.1:9/v1",
            "auth":{"kind":"none"}, "models":[{"id":"private-model"}],
            "metadata":{"future":{"preserve":{"enabled":true,"version":19}}}
        });
        let original = serde_json::to_vec_pretty(&document).unwrap();
        std::fs::write(&h.source, &original).unwrap();
        std::fs::set_permissions(&h.source, std::fs::Permissions::from_mode(0o644)).unwrap();
        let before = std::fs::metadata(&h.source).unwrap();
        assert_eq!(before.permissions().mode() & 0o777, 0o644);
        let report = h.save(&[ModelSettingsChange::ProviderPatch {provider:"private-fixture".into(),patch:json!({"api":"openai-completions","base_url":"http://127.0.0.1:9/v1","api_key":"synthetic-private-source-key","models":[{"id":"private-model","inputs":["text"]}]})}]).await;
        assert!(report.saved && report.applied && report.projection_confirmed);
        let current = std::fs::metadata(&h.source).unwrap();
        assert_eq!(
            current.permissions().mode() & 0o777,
            0o600,
            "含新秘密的Source必须私密：embedded={embedded}"
        );
        assert_ne!(current.ino(), before.ino(), "真实原子替换应保留原件inode");
        let saved: Value = serde_json::from_slice(&std::fs::read(&h.source).unwrap()).unwrap();
        assert_eq!(
            saved["providers"]["private-fixture"]["metadata"]["future"],
            document["providers"]["private-fixture"]["metadata"]["future"]
        );
        assert_eq!(
            saved["providers"]["private-fixture"]["auth"]["apiKey"],
            "synthetic-private-source-key"
        );
        if embedded {
            assert_eq!(saved["home"], document["home"]);
            assert_eq!(saved["workspace"], document["workspace"]);
        }
        let records: Vec<Value> = std::fs::read_dir(h.home.join("file-recovery-records"))
            .unwrap()
            .map(|entry| {
                serde_json::from_slice(&std::fs::read(entry.unwrap().path()).unwrap()).unwrap()
            })
            .filter(|record: &Value| {
                record["scope"] == "model-configuration" && record["filePath"] == json!(h.source)
            })
            .collect();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record["status"], "completed_original_retained");
        assert_eq!(record["retainedMode"], 0o600);
        assert!(record["retainedPrivateStamp"].is_object());
        let retained = record["retainedPath"].as_str().unwrap();
        let original_metadata = std::fs::metadata(retained).unwrap();
        assert_eq!(original_metadata.ino(), before.ino());
        assert_eq!(original_metadata.dev(), before.dev());
        assert_eq!(
            original_metadata.permissions().mode() & 0o777,
            0o600,
            "原件也不能继承0644：embedded={embedded}"
        );
        assert_eq!(std::fs::read(retained).unwrap(), original);
        h.close().await;
    }
}

#[tokio::test]
async fn legacy_custom_provider_omitted_protocol_first_save_dispatches_openai_and_explicit_invalid_never_falls_back(
) {
    use lingxi_kernel::ports::StoragePort as _;
    use lingxi_service::model_configuration::{ProviderProbeKind, ProviderProbeRequest};
    let h = Harness::boot(false).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let received = observed.clone();
    let app = axum::Router::new().route("/v1/chat/completions", axum::routing::post(move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<Value>| {
        let received = received.clone();
        async move {
            received.lock().unwrap().push((headers, body));
            let frame = json!({"id":"legacy-protocol","choices":[{"index":0,"delta":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}});
            ([("content-type", "text/event-stream")], format!("data: {frame}\n\ndata: [DONE]\n\n"))
        }
    }));
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let patch = json!({"base_url":endpoint,"api_key":"synthetic-legacy-key","models":[{"id":"legacy-chat","inputs":["text"]}]});
    // 缺省是旧格式的正常语义；显式给错不能借默认或已有有效配置变成成功。
    for field in ["api", "protocol"] {
        for value in [
            Value::Null,
            json!(""),
            json!("unknown-protocol"),
            json!(7),
            json!({}),
        ] {
            let mut invalid = patch.clone();
            invalid[field] = value;
            let error = h
                .state
                .model_configuration()
                .save(
                    &h.principal,
                    &[ModelSettingsChange::ProviderPatch {
                        provider: "fixture".into(),
                        patch: invalid,
                    }],
                )
                .await
                .unwrap_err();
            assert_eq!(error.status, 400, "{error}");
            assert!(!error.report.saved && !error.report.applied);
            assert!(!h.source.exists());
            assert!(h.state.model_gateway().is_none());
        }
    }
    let error = h
        .state
        .model_configuration()
        .save(
            &h.principal,
            &[ModelSettingsChange::ProviderPatch {
                provider: "missing-endpoint".into(),
                patch: json!({"api_key":"synthetic-key","models":["legacy-chat"]}),
            }],
        )
        .await
        .unwrap_err();
    assert_eq!(error.status, 400);
    assert!(!error.report.saved && !error.report.applied && !h.source.exists());
    let report = h
        .save(&[ModelSettingsChange::ProviderPatch {
            provider: "fixture".into(),
            patch,
        }])
        .await;
    assert!(report.saved && report.applied && report.activated);
    let saved = std::fs::read(&h.source).unwrap();
    let document: Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(
        document["providers"]["fixture"]["protocol"],
        "openai-completions"
    );
    let generation = h.gateway.upcoming_generation();
    for field in ["api", "protocol"] {
        for value in [
            Value::Null,
            json!(""),
            json!("unknown-protocol"),
            json!(7),
            json!({}),
        ] {
            let mut invalid = json!({});
            invalid[field] = value;
            let error = h
                .state
                .model_configuration()
                .save(
                    &h.principal,
                    &[ModelSettingsChange::ProviderPatch {
                        provider: "fixture".into(),
                        patch: invalid,
                    }],
                )
                .await
                .unwrap_err();
            assert_eq!(error.status, 400);
            assert!(!error.report.saved && !error.report.applied);
            assert_eq!(std::fs::read(&h.source).unwrap(), saved);
            assert_eq!(h.gateway.upcoming_generation(), generation);
        }
    }
    let result = h
        .state
        .model_configuration()
        .probe(
            &h.principal,
            ProviderProbeRequest {
                kind: ProviderProbeKind::ModelHealth,
                provider: Some("fixture".into()),
                model: Some("legacy-chat".into()),
                draft: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(result.value["ok"], true, "{}", result.value);
    {
        let requests = observed.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].0.get("authorization").unwrap(),
            "Bearer synthetic-legacy-key"
        );
        assert_eq!(requests[0].1["model"], "legacy-chat");
        assert!(requests[0].1["messages"].is_array());
    }
    let usage = h
        .state
        .storage()
        .query_model_call_usage(Default::default())
        .await
        .unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].protocol, "openai-completions");
    assert_eq!(usage[0].provider, "fixture");
    assert_eq!(usage[0].model, "legacy-chat");
    assert_eq!(usage[0].transport_attempts, Some(1));
    let _ = stop.send(());
    server.await.unwrap();
    h.close().await;
}

#[tokio::test]
async fn first_save_activates_shared_catalog_credentials_and_independent_knowledge_without_restart()
{
    let h = Harness::boot(false).await;
    assert!(h.state.model_gateway().is_none());
    h.seed().await;
    assert!(h.state.model_gateway().is_some());
    let report = h
        .save(&[
            binding(
                ModelOperation::Auxiliary(AuxiliarySlot::Summarize),
                "summary",
            ),
            binding(
                ModelOperation::Auxiliary(AuxiliarySlot::Knowledge),
                "knowledge",
            ),
        ])
        .await;
    assert!(
        report.saved
            && report.applied
            && report.activated
            && report.projection_confirmed
            && report.audit_recorded
    );
    let summary = h
        .gateway
        .resolve_route(&ModelRouteRequest::for_operation(
            ModelOperation::Auxiliary(AuxiliarySlot::Summarize),
        ))
        .unwrap();
    let knowledge = h
        .gateway
        .resolve_route(&ModelRouteRequest::for_operation(
            ModelOperation::Auxiliary(AuxiliarySlot::Knowledge),
        ))
        .unwrap();
    assert_eq!(summary.model, "summary");
    assert_eq!(knowledge.model, "knowledge");
    assert_eq!(summary.protocol.config_name(), "openai-completions");
    assert_eq!(knowledge.protocol.config_name(), "anthropic-messages");
    let auth = h.credentials.resolve(&knowledge).await.unwrap();
    let headers = auth.header_values("authorization", "Bearer ");
    assert_eq!(
        headers.get("authorization").map(String::as_str),
        Some("Bearer primary-secret")
    );
    assert_eq!(
        headers.get("x-custom").map(String::as_str),
        Some("custom-secret")
    );
    let snapshot = h
        .state
        .model_configuration()
        .snapshot(&h.principal)
        .await
        .unwrap();
    assert_eq!(
        snapshot["providers"]["fixture"]["api"],
        "openai-completions"
    );
    assert!(!snapshot.to_string().contains("primary-secret"));
    assert!(!snapshot.to_string().contains("custom-secret"));
    let source: Value = serde_json::from_slice(&std::fs::read(&h.source).unwrap()).unwrap();
    assert_eq!(
        source["providers"]["fixture"]["models"][1]["extension"]["kept"],
        true
    );
    h.close().await;
}

#[tokio::test]
async fn embedded_source_preserves_envelope_rejects_secret_metadata_before_write_and_reload_recovers(
) {
    let h = Harness::boot(true).await;
    h.seed().await;
    let before = std::fs::read(&h.source).unwrap();
    let generation = h.gateway.upcoming_generation();
    for field in [
        "api_key",
        "apikey",
        "token",
        "access",
        "accessToken",
        "access_token",
        "refresh",
        "refresh_token",
        "refreshToken",
        "accountId",
        "account_id",
        "authorization",
        "cookie",
        "headers",
        "resourceUrl",
        "resource_url",
        "expires",
    ] {
        let mut patch = json!({});
        patch[field] = json!("must-never-save");
        let result = h
            .state
            .model_configuration()
            .save(
                &h.principal,
                &[ModelSettingsChange::ModelPatch {
                    provider: "fixture".into(),
                    model: "knowledge".into(),
                    patch,
                }],
            )
            .await;
        assert!(result.is_err(), "secret metadata accepted: {field}");
        assert_eq!(
            std::fs::read(&h.source).unwrap(),
            before,
            "source changed for {field}"
        );
        assert_eq!(h.gateway.upcoming_generation(), generation);
    }
    let doc: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(doc["home"], json!(h.home));
    assert_eq!(doc["workspace"], json!(h.home));
    std::fs::write(&h.source, b"{broken").unwrap();
    assert!(h
        .state
        .model_configuration()
        .reload(&h.principal)
        .await
        .is_err());
    assert_eq!(h.gateway.upcoming_generation(), generation);
    assert!(h.gateway.resolve_route(&pinned("knowledge")).is_ok());
    std::fs::write(&h.source, &before).unwrap();
    let report = h
        .state
        .model_configuration()
        .reload(&h.principal)
        .await
        .unwrap();
    assert!(!report.saved && report.applied);
    assert!(h.gateway.upcoming_generation() > generation);
    h.close().await;
}

#[tokio::test]
async fn malformed_headers_never_clear_saved_settings_or_publish_a_generation() {
    use lingxi_service::model_configuration::{
        ProviderProbeDraft, ProviderProbeKind, ProviderProbeRequest,
    };
    for embedded in [false, true] {
        let h = Harness::boot(embedded).await;
        h.seed().await;
        let before = std::fs::read(&h.source).unwrap();
        let generation = h.gateway.upcoming_generation();
        for invalid in [json!(false), json!([]), json!("bad"), json!(42)] {
            let error = h
                .state
                .model_configuration()
                .save(
                    &h.principal,
                    &[ModelSettingsChange::ProviderPatch {
                        provider: "fixture".into(),
                        patch: json!({"headers": invalid}),
                    }],
                )
                .await
                .expect_err("损坏的请求头必须拒绝，不能清空原值");
            assert_eq!(error.status, 400);
            assert_eq!(error.code, "invalid_model_settings");
            let error = h
                .state
                .model_configuration()
                .probe(
                    &h.principal,
                    ProviderProbeRequest {
                        kind: ProviderProbeKind::Connection,
                        provider: Some("fixture".into()),
                        model: Some("summary".into()),
                        draft: Some(ProviderProbeDraft {
                            headers: Some(invalid),
                            ..Default::default()
                        }),
                    },
                )
                .await
                .expect_err("损坏的请求头不得启动供应商探测");
            assert_eq!(error.status, 400);
            assert_eq!(std::fs::read(&h.source).unwrap(), before);
            assert_eq!(h.gateway.upcoming_generation(), generation);
            let route = h.gateway.resolve_route(&pinned("summary")).unwrap();
            assert_eq!(
                h.credentials
                    .resolve(&route)
                    .await
                    .unwrap()
                    .header_values("authorization", "Bearer ")
                    .get("x-custom")
                    .map(String::as_str),
                Some("custom-secret")
            );
        }
        h.close().await;
    }
}

#[tokio::test]
async fn malformed_provider_endpoint_is_rejected_without_falling_back_to_saved_address() {
    let h = Harness::boot(false).await;
    h.seed().await;
    let before = std::fs::read(&h.source).unwrap();
    let generation = h.gateway.upcoming_generation();
    for field in ["endpoint", "base_url"] {
        for invalid in [json!(null), json!(false), json!([]), json!({}), json!(42)] {
            let mut patch = json!({});
            patch[field] = invalid;
            let error = h
                .state
                .model_configuration()
                .save(
                    &h.principal,
                    &[ModelSettingsChange::ProviderPatch {
                        provider: "fixture".into(),
                        patch,
                    }],
                )
                .await
                .expect_err("显式损坏的地址不能静默回退到旧值并报告已保存");
            assert_eq!(error.status, 400);
            assert_eq!(std::fs::read(&h.source).unwrap(), before);
            assert_eq!(h.gateway.upcoming_generation(), generation);
            assert_eq!(
                h.gateway
                    .resolve_route(&pinned("summary"))
                    .unwrap()
                    .endpoint,
                "http://127.0.0.1:9/v1"
            );
        }
    }
    h.close().await;
}

#[tokio::test]
async fn headers_null_clears_only_headers_and_deleted_catalog_cannot_borrow_old_binding_or_credentials(
) {
    let h = Harness::boot(false).await;
    h.seed().await;
    h.save(&[binding(ModelOperation::Chat, "knowledge")]).await;
    let old = h.gateway.resolve_route(&pinned("knowledge")).unwrap();
    h.save(&[ModelSettingsChange::ProviderPatch {
        provider: "fixture".into(),
        patch: json!({"display_name":"renamed"}),
    }])
    .await;
    let now = h.gateway.resolve_route(&pinned("knowledge")).unwrap();
    assert_eq!(
        h.credentials
            .resolve(&now)
            .await
            .unwrap()
            .header_values("authorization", "Bearer ")
            .get("x-custom")
            .map(String::as_str),
        Some("custom-secret")
    );
    h.save(&[ModelSettingsChange::ProviderPatch {
        provider: "fixture".into(),
        patch: json!({"headers":null}),
    }])
    .await;
    let current = h.gateway.resolve_route(&pinned("knowledge")).unwrap();
    let auth = h.credentials.resolve(&current).await.unwrap();
    assert!(!auth
        .header_values("authorization", "Bearer ")
        .contains_key("x-custom"));
    assert_eq!(auth.bearer_token(), Some("primary-secret"));
    assert!(h.credentials.resolve(&old).await.is_err());
    let report = h
        .save(&[ModelSettingsChange::ModelRemove {
            provider: "fixture".into(),
            model: "knowledge".into(),
        }])
        .await;
    assert_eq!(report.cleared_bindings, vec!["chat"]);
    assert!(h.gateway.resolve_route(&pinned("knowledge")).is_err());
    assert!(h
        .gateway
        .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat))
        .is_err());
    assert!(h.gateway.resolve_route(&pinned("summary")).is_ok());
    h.close().await;
}

#[tokio::test]
async fn operation_catalog_tags_select_independent_dialects_and_invalid_metadata_never_publishes() {
    let h = Harness::boot(false).await;
    h.save(&[ModelSettingsChange::ProviderPatch { provider:"fixture".into(), patch:json!({"api":"openai-completions","base_url":"http://127.0.0.1:9/v1","models":[{"id":"data","operations":["embedding","rerank"],"inputs":["text"]},{"id":"text","inputs":["text"]}]}) }, binding(ModelOperation::Embedding,"data"),binding(ModelOperation::Rerank,"data")]).await;
    let embedding = h
        .gateway
        .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Embedding))
        .unwrap();
    let rerank = h
        .gateway
        .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Rerank))
        .unwrap();
    assert_eq!(embedding.protocol.config_name(), "openai-embeddings");
    assert_eq!(rerank.protocol.config_name(), "cohere-rerank");
    assert!(
        h.gateway.resolve_route(&pinned("data")).is_err(),
        "操作目录不能偷用文本协议"
    );
    let mut text = pinned("text");
    text.operation = ModelOperation::Embedding;
    assert!(
        h.gateway.resolve_route(&text).is_err(),
        "普通文本模型没有向量能力"
    );
    let before = std::fs::read(&h.source).unwrap();
    for patch in [
        json!({"inputs":[]}),
        json!({"outputs":["invalid"]}),
        json!({"toolUse":{"supportsTools":true}}),
        json!({"operations":["chat"]}),
    ] {
        assert!(h
            .state
            .model_configuration()
            .save(
                &h.principal,
                &[ModelSettingsChange::ModelPatch {
                    provider: "fixture".into(),
                    model: "text".into(),
                    patch
                }]
            )
            .await
            .is_err());
        assert_eq!(std::fs::read(&h.source).unwrap(), before);
    }
    h.save(&[ModelSettingsChange::ModelPatch{provider:"fixture".into(),model:"text".into(),patch:json!({"inputs":["image","text","image"],"contextWindow":4096,"maxTokens":256,"outputIncludesThinking":true})}]).await;
    h.save(&[ModelSettingsChange::ModelPatch {
        provider: "fixture".into(),
        model: "text".into(),
        patch: json!({"contextWindow":8192,"maxOutputTokens":512,"outputIncludesThinking":null}),
    }])
    .await;
    let snapshot = h
        .state
        .model_configuration()
        .snapshot(&h.principal)
        .await
        .unwrap();
    let model = snapshot["providers"]["fixture"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "text")
        .unwrap();
    assert_eq!(model["inputs"], json!(["text", "image"]));
    assert_eq!(model["compat"]["contextWindow"], 8192);
    assert_eq!(model["compat"]["maxTokens"], 512);
    assert!(model["compat"]
        .get("outputIncludesThinking")
        .is_none_or(Value::is_null));
    h.close().await;
}

#[tokio::test]
async fn compatibility_projection_failure_reports_applied_snapshot_and_reload_can_confirm_it() {
    let h = Harness::boot(false).await;
    h.seed().await;
    let catalog = h.home.join("provider-catalog.json");
    let catalog_before = std::fs::read(&catalog).unwrap();
    std::fs::write(&catalog, b"{broken compatibility catalog").unwrap();
    let error = h
        .state
        .model_configuration()
        .save(
            &h.principal,
            &[ModelSettingsChange::ModelPatch {
                provider: "fixture".into(),
                model: "knowledge".into(),
                patch: json!({"contextWindow":12345}),
            }],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "model_projection_unconfirmed");
    assert!(error.report.saved && error.report.applied && error.report.activated);
    assert!(!error.report.projection_confirmed);
    let snapshot = h
        .state
        .model_configuration()
        .snapshot(&h.principal)
        .await
        .unwrap();
    assert_eq!(snapshot["generation"], error.report.generation.to_string());
    let model = snapshot["providers"]["fixture"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["id"] == "knowledge")
        .unwrap();
    assert_eq!(
        model["compat"]["contextWindow"], 12345,
        "错误回执后不能混读旧目录与新网关"
    );
    let source: Value = serde_json::from_slice(&std::fs::read(&h.source).unwrap()).unwrap();
    assert_eq!(
        source["providers"]["fixture"]["models"][1]["context"],
        12345
    );
    std::fs::write(&catalog, &catalog_before).unwrap();
    let confirmed = h
        .state
        .model_configuration()
        .reload(&h.principal)
        .await
        .unwrap();
    assert!(!confirmed.saved && confirmed.applied && confirmed.projection_confirmed);
    let before = h.gateway.upcoming_generation();
    let backup = h.source.with_extension("saved.json");
    std::fs::rename(&h.source, &backup).unwrap();
    let error = h
        .state
        .model_configuration()
        .save(
            &h.principal,
            &[ModelSettingsChange::ProviderPatch {
                provider: "fixture".into(),
                patch: json!({"display_name":"must-not-publish"}),
            }],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "model_source_missing");
    assert!(!error.report.saved && !error.report.applied);
    assert!(!h.source.exists());
    assert_eq!(h.gateway.upcoming_generation(), before);
    std::fs::rename(backup, &h.source).unwrap();
    h.close().await;
}

#[tokio::test]
async fn explicit_codex_display_alias_edits_one_source_and_login_state_comes_from_credentials() {
    let h = Harness::boot(false).await;
    h.save(&[ModelSettingsChange::ProviderPatch { provider:"openai-codex-oauth".into(),patch:json!({"api":"openai-codex-responses","base_url":"http://127.0.0.1:9/backend-api","auth":{"kind":"oauth","flow":"deviceCode","clientId":"local-fixture","tokenEndpoint":"http://127.0.0.1:9/token","deviceAuthorizationEndpoint":"http://127.0.0.1:9/device"},"logged_in":true,"models":[{"id":"codex-model","inputs":["text"]}]}) }]).await;
    let snapshot = h
        .state
        .model_configuration()
        .snapshot(&h.principal)
        .await
        .unwrap();
    assert_eq!(
        snapshot["providers"]["openai-codex"]["logged_in"], false,
        "用户元数据不能伪造真实OAuth登录"
    );
    assert_eq!(
        snapshot["providers"]["openai-codex"]["credential_state"],
        "not_logged_in"
    );
    assert!(snapshot["providers"].get("openai-codex-oauth").is_none());
    let summary = lingxi_service::provider_routes::summary_payload(&snapshot).unwrap();
    assert_eq!(summary["providers"]["openai-codex-oauth"]["type"], "oauth");
    assert_eq!(
        summary["providers"]["openai-codex-oauth"]["logged_in"],
        false
    );
    assert!(
        summary["providers"].get("openai-codex").is_none(),
        "显式显示别名不能多造一项API-key供应商"
    );
    assert_eq!(
        h.state
            .model_configuration()
            .saved_api_key(&h.principal, "openai-codex-oauth")
            .await
            .unwrap(),
        ""
    );
    h.save(&[ModelSettingsChange::ModelPatch {
        provider: "openai-codex-oauth".into(),
        model: "codex-model".into(),
        patch: json!({"contextWindow":16384}),
    }])
    .await;
    let mut alias = ModelRouteRequest::for_operation(ModelOperation::Chat);
    alias.provider = Some("openai-codex-oauth".into());
    alias.model = Some("codex-model".into());
    let route = h.gateway.resolve_route(&alias).unwrap();
    assert_eq!(route.provider, "openai-codex");
    let doc: Value = serde_json::from_slice(&std::fs::read(&h.source).unwrap()).unwrap();
    assert_eq!(doc["providers"].as_object().unwrap().len(), 1);
    assert_eq!(
        doc["providers"]["openai-codex"]["models"][0]["context"],
        16384
    );
    h.save(&[ModelSettingsChange::ModelRemove {
        provider: "openai-codex-oauth".into(),
        model: "codex-model".into(),
    }])
    .await;
    assert!(h.gateway.resolve_route(&alias).is_err());
    h.save(&[ModelSettingsChange::ProviderRemove {
        provider: "openai-codex-oauth".into(),
    }])
    .await;
    assert!(h
        .state
        .model_configuration()
        .snapshot(&h.principal)
        .await
        .unwrap()["providers"]
        .as_object()
        .unwrap()
        .is_empty());
    h.close().await;
}
