//! F07：目录声明和历史工具对的生产契约。
use lingxi_kernel::{model_exchange::*, toolcatalog::*};
use lingxi_protocol::ToolSchemaDocument;
use serde_json::json;

fn manifest() -> ToolManifest {
    ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: "read".into(),
        display_name: "读文件展示名".into(),
        aliases: vec![],
        version: "1.0.0".into(),
        description: "UNIQUE-F07-CONSTRAINT 参数必须使用 path，不能用 filename。".into(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".into(),
            schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        },
        output_schema: None,
        permission: PermissionContract {
            kind: PermissionKind::Read,
            capability_base: "read".into(),
        },
        availability: Availability::Available,
        timeout_ms: None,
        max_concurrency: None,
        declared_permission: DeclaredPermission::ReadOnly,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    }
}
#[test]
fn normal_name_schema_and_generation_positive() {
    let registry = ToolRegistry::new();
    let receipt = registry
        .register(manifest(), &SchemaBudget::default())
        .unwrap();
    let snapshot = ToolDeclarationSnapshot::from_registry(&registry).unwrap();
    assert_eq!(snapshot.catalog_generation, receipt.catalog_generation);
    assert_eq!(
        snapshot.target_of_wire_name("read"),
        Some(receipt.target_id.as_str())
    );
    assert_eq!(
        snapshot.declarations[0].input_schema.schema["required"],
        json!(["path"])
    );
}
#[test]
fn declaration_keeps_full_formal_description() {
    let registry = ToolRegistry::new();
    registry
        .register(manifest(), &SchemaBudget::default())
        .unwrap();
    let snapshot = ToolDeclarationSnapshot::from_registry(&registry).unwrap();
    assert_eq!(
        snapshot.declarations[0].description,
        manifest().description,
        "正式说明不能退化为展示名"
    );
}

#[test]
fn run_activation_does_not_change_shared_residents_or_enable_disabled_tools() {
    let registry = ToolRegistry::new();
    let receipt = registry
        .register(manifest(), &SchemaBudget::default())
        .unwrap();
    registry.configure_model_residents(std::collections::BTreeSet::new());
    let generation = registry.catalog_generation();
    let activated = std::collections::BTreeSet::from([receipt.target_id.clone()]);
    assert!(ToolDeclarationSnapshot::from_registry(&registry)
        .unwrap()
        .declarations
        .is_empty());
    let snapshot =
        ToolDeclarationSnapshot::from_registry_with_activated_targets(&registry, &activated)
            .unwrap();
    assert_eq!(snapshot.declarations.len(), 1);
    assert_eq!(snapshot.declarations[0].description, manifest().description);
    assert_eq!(
        snapshot.declarations[0].input_schema,
        manifest().input_schema
    );
    assert_eq!(snapshot.catalog_generation, generation);
    assert_eq!(registry.catalog_generation(), generation);
    assert!(ToolDeclarationSnapshot::from_registry(&registry)
        .unwrap()
        .declarations
        .is_empty());
    let mut disabled = manifest();
    disabled.availability = Availability::Disabled {
        reason: "用户禁用".into(),
    };
    registry
        .update(&receipt.target_id, disabled, &SchemaBudget::default())
        .unwrap();
    assert!(
        ToolDeclarationSnapshot::from_registry_with_activated_targets(&registry, &activated)
            .unwrap()
            .declarations
            .is_empty()
    );
    assert_eq!(
        snapshot.declarations.len(),
        1,
        "已发送历史声明保持冻结，后续授权仍由执行入口复核"
    );
}

use lingxi_kernel::ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest};
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{AttemptId, ModelCallId, RunId, SessionId, ToolCallId};
use lingxi_service::toolgateway::{
    CallerSurface, FailClosedPolicy, InvocationPermissionContext, ToolInvocationGateway,
};
use std::sync::Arc;

#[test]
fn trusted_resident_addition_preserves_default_policy_and_real_target_boundaries() {
    let (registry, _gateway) = world();
    let defaults = lingxi_service::toolcatalog_bridge::RESIDENT_NAMES;
    assert_eq!(
        defaults,
        [
            "read",
            "write",
            "edit",
            "exec_command",
            "mcp_search_tools",
            "mcp_describe_tool",
            "mcp_call"
        ]
    );
    for name in ["read", "write", "edit", "exec_command"] {
        let mut tool = manifest();
        tool.local_name = name.into();
        registry.register(tool, &SchemaBudget::default()).unwrap();
    }
    let mut worker = manifest();
    worker.origin = ToolOrigin::Plugin {
        plugin_id: "configured".into(),
    };
    worker.local_name = "probe".into();
    let worker = registry
        .register(worker, &SchemaBudget::default())
        .unwrap()
        .target_id;
    let mut unrelated = manifest();
    unrelated.local_name = "unrelated".into();
    let unrelated = registry
        .register(unrelated, &SchemaBudget::default())
        .unwrap()
        .target_id;
    let before = ToolDeclarationSnapshot::from_registry(&registry).unwrap();
    assert_eq!(before.declarations.len(), 7);
    assert!(before.bind_target(worker.as_str()).is_none());
    let generation = registry.catalog_generation();
    let missing = ToolTargetId::parse("tool:plugin:missing:probe");
    assert!(matches!(
        registry.add_model_resident(&missing),
        Err(ToolCatalogError::TargetNotFound { .. })
    ));
    assert_eq!(registry.catalog_generation(), generation);
    registry.add_model_resident(&worker).unwrap();
    assert_eq!(registry.catalog_generation(), generation + 1);
    registry.add_model_resident(&worker).unwrap();
    assert_eq!(registry.catalog_generation(), generation + 1);
    let sent = ToolDeclarationSnapshot::from_registry(&registry).unwrap();
    assert_eq!(sent.declarations.len(), 8);
    assert!(sent.bind_target(unrelated.as_str()).is_none());
    let declaration = sent
        .declarations
        .iter()
        .find(|d| d.target == worker.as_str())
        .unwrap();
    assert_eq!(declaration.input_schema, manifest().input_schema);
    registry
        .set_availability(&worker, Availability::Deferred)
        .unwrap();
    let deferred_generation = registry.catalog_generation();
    registry.add_model_resident(&worker).unwrap();
    assert_eq!(registry.catalog_generation(), deferred_generation);
    assert!(ToolDeclarationSnapshot::from_registry(&registry)
        .unwrap()
        .bind_target(worker.as_str())
        .is_none());
    registry
        .set_availability(
            &worker,
            Availability::Disabled {
                reason: "明确禁用".into(),
            },
        )
        .unwrap();
    let disabled_generation = registry.catalog_generation();
    registry.add_model_resident(&worker).unwrap();
    assert_eq!(registry.catalog_generation(), disabled_generation);
    assert!(ToolDeclarationSnapshot::from_registry(&registry)
        .unwrap()
        .bind_target(worker.as_str())
        .is_none());
    assert!(registry
        .prepare_invocation(
            &ToolTargetRef::ByTargetId {
                target_id: worker.clone()
            },
            None,
            &json!({"path":"x"}),
            &SchemaBudget::default()
        )
        .is_err());
    assert!(
        sent.bind_target(worker.as_str()).is_some(),
        "旧发送快照仍冻结"
    );
}

#[test]
fn trusted_resident_addition_keeps_unconfigured_registry_unrestricted() {
    let registry = ToolRegistry::new();
    let first = registry
        .register(manifest(), &SchemaBudget::default())
        .unwrap()
        .target_id;
    let mut other = manifest();
    other.local_name = "other".into();
    registry.register(other, &SchemaBudget::default()).unwrap();
    let generation = registry.catalog_generation();
    registry.add_model_resident(&first).unwrap();
    assert_eq!(registry.catalog_generation(), generation);
    assert_eq!(
        ToolDeclarationSnapshot::from_registry(&registry)
            .unwrap()
            .declarations
            .len(),
        2
    );
}

fn route(protocol: ProtocolFamily) -> ResolvedModelRoute {
    ResolvedModelRoute {
        provider: "fixture".into(),
        model: "fixture-model".into(),
        operation: ModelOperation::Chat,
        protocol,
        endpoint: "http://127.0.0.1:1/v1".into(),
        credential: CredentialReference {
            provider: "fixture".into(),
            auth: CredentialAuthKind::None,
        },
        config_generation: 1,
        group_id: None,
    }
}
fn render(
    input: &ModelTurnInput,
    family: ProtocolFamily,
) -> Result<serde_json::Value, lingxi_protocol::ProtocolError> {
    use lingxi_adapters::models::*;
    let route = route(family);
    match family {
        ProtocolFamily::OpenAiCompletions => {
            openai_completions::render_chat_request(input, &route, false)
        }
        ProtocolFamily::OpenAiResponses => {
            openai_responses::render_responses_request(input, &route, false)
        }
        ProtocolFamily::OpenAiCodexResponses => {
            openai_codex_responses::render_codex_request(input, &route)
        }
        ProtocolFamily::AnthropicMessages => {
            anthropic_messages::render_messages_request(input, &route, 1024, false)
        }
        ProtocolFamily::GoogleGenerativeAi => {
            google_generative_ai::render_generate_request(input, &route)
        }
        _ => panic!("不是本次五协议范围"),
    }
}
fn families() -> [ProtocolFamily; 5] {
    [
        ProtocolFamily::OpenAiCompletions,
        ProtocolFamily::OpenAiResponses,
        ProtocolFamily::OpenAiCodexResponses,
        ProtocolFamily::AnthropicMessages,
        ProtocolFamily::GoogleGenerativeAi,
    ]
}
#[test]
fn first_layer_c03_five_protocols_keep_actual_full_description_schema_name_generation() {
    let registry = ToolRegistry::new();
    let receipt = registry
        .register(manifest(), &SchemaBudget::default())
        .unwrap();
    let snapshot = ToolDeclarationSnapshot::from_registry(&registry).unwrap();
    for family in families() {
        let input = ModelTurnInput::first_turn("确认", snapshot.clone());
        let wire = render(&input, family).unwrap();
        let tools = if family == ProtocolFamily::GoogleGenerativeAi {
            &wire["tools"][0]["functionDeclarations"]
        } else {
            &wire["tools"]
        };
        let d = if family == ProtocolFamily::OpenAiCompletions {
            &tools[0]["function"]
        } else {
            &tools[0]
        };
        assert_eq!(d["name"], "read");
        assert_eq!(d["description"], manifest().description);
        let schema = match family {
            ProtocolFamily::AnthropicMessages => &d["input_schema"],
            ProtocolFamily::GoogleGenerativeAi => &d["parametersJsonSchema"],
            _ => &d["parameters"],
        };
        assert_eq!(*schema, manifest().input_schema.schema);
        assert_eq!(input.tools.catalog_generation, receipt.catalog_generation);
        println!(
            "{}",
            json!({"case":"F07-C03","family":format!("{family:?}"),"wire":wire,"schemaValid":true,"semanticParamsCorrect":null,"tokenCount":null,"catalogGeneration":input.tools.catalog_generation})
        );
    }
}
fn context(run: &str) -> RunContext {
    RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("own-synthetic-session"),
        run_id: RunId::new(run),
        attempt: AttemptId::new(format!("{run}#a1")),
        generation: 1,
    }
}
fn permission() -> InvocationPermissionContext {
    InvocationPermissionContext::UserSession {
        mode: lingxi_kernel::subagent::SessionPermissionMode::Operate,
    }
}
struct Echo;
impl ToolExecutorPort for Echo {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            ToolExecutionResult::of_ctx(
                ctx,
                ToolOutcome::success_text(
                    json!({"target":request.target,"arguments":request.arguments.as_value()})
                        .to_string(),
                ),
            )
        })
    }
}
fn world() -> (Arc<ToolRegistry>, ToolInvocationGateway) {
    let registry = Arc::new(ToolRegistry::new());
    let gateway = ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::new(FailClosedPolicy),
        Arc::new(lingxi_service::inject::ManualClock::new(1700000000000)),
        SchemaBudget::default(),
        60000,
        128,
    );
    lingxi_service::toolcatalog_bridge::register(&registry, &gateway, &SchemaBudget::default());
    (registry, gateway)
}
async fn dispatch(
    gateway: &ToolInvocationGateway,
    ctx: &RunContext,
    request: &ToolRequest,
) -> ToolOutcome {
    let id = ToolCallId::new(format!("{}-{}", ctx.run_id, request.target));
    let sent = ToolDeclarationSnapshot::from_registry(gateway.registry()).unwrap();
    let (actual, pin) = gateway
        .normalize_model_request(
            ctx,
            request,
            Some(CatalogPin {
                catalog_generation: sent.catalog_generation,
            }),
        )
        .unwrap();
    gateway
        .validate_from_request_pinned(
            ctx,
            CallerSurface::UserRun,
            "own-agent",
            permission(),
            &id,
            &actual,
            pin,
        )
        .unwrap();
    let prepared = gateway
        .prepare_from_request_pinned(
            ctx,
            CallerSurface::UserRun,
            "own-agent",
            permission(),
            &id,
            &actual,
            pin,
        )
        .unwrap();
    gateway
        .execute_prepared(ctx, &id, &prepared.handle)
        .await
        .unwrap()
        .outcome
}
fn request(name: &str, args: serde_json::Value) -> ToolRequest {
    ToolRequest::from_effective_arguments(
        format!("tool:first-party:{name}"),
        args,
        &SchemaBudget::default(),
    )
    .unwrap()
}
fn text(outcome: ToolOutcome) -> serde_json::Value {
    let ToolOutcome::Success { result } = outcome else {
        panic!("调用未成功：{outcome:?}")
    };
    let lingxi_protocol::ContentBlock::Text { text } = &result.content[0] else {
        panic!("非文本结果")
    };
    serde_json::from_str(text).unwrap()
}
#[tokio::test]
async fn first_layer_c02_real_catalog_discovers_describes_dispatches_new_registration() {
    let (registry, gateway) = world();
    let ctx = context("catalog-normal");
    let _lease = gateway.catalog_run_lease(&ctx).unwrap();
    let mut tool = manifest();
    tool.local_name = "future_stagefiles_fixture".into();
    tool.display_name = "新工具".into();
    tool.availability = Availability::Deferred;
    let receipt = registry
        .register(tool.clone(), &SchemaBudget::default())
        .unwrap();
    gateway.bind_executor(
        receipt.target_id.clone(),
        Arc::new(Echo),
        "合成目标仅替换外部执行结果，真实目录与网关保留",
    );
    let sent = ToolDeclarationSnapshot::from_registry(&registry).unwrap();
    assert!(!sent
        .declarations
        .iter()
        .any(|d| d.target == receipt.target_id.as_str()));
    let search = text(
        dispatch(
            &gateway,
            &ctx,
            &request(
                "mcp_search_tools",
                json!({"query":"future_stagefiles_fixture"}),
            ),
        )
        .await,
    );
    assert_eq!(search["tools"][0]["targetId"], receipt.target_id.as_str());
    let described = text(
        dispatch(
            &gateway,
            &ctx,
            &request(
                "mcp_describe_tool",
                json!({"name":search["tools"][0]["tool"],"server":search["tools"][0]["server"]}),
            ),
        )
        .await,
    );
    assert_eq!(described["description"], tool.description);
    assert_eq!(described["inputSchema"], tool.input_schema.schema);
    assert_eq!(
        described["catalogGeneration"],
        registry.catalog_generation()
    );
    let outcome=text(dispatch(&gateway,&ctx,&request("mcp_call",json!({"tool":search["tools"][0]["tool"],"server":search["tools"][0]["server"],"arguments":{"path":"synthetic.txt"}}))).await);
    assert_eq!(outcome["target"], receipt.target_id.as_str());
    assert_eq!(outcome["arguments"], json!({"path":"synthetic.txt"}));
    println!(
        "{}",
        json!({"case":"F07-C02","boundary":"in-process actual registry/gateway; synthetic target executor","search":search,"describe":described,"result":outcome,"physicalToolCalls":3,"modelCalls":null})
    );
}
#[test]
fn first_layer_c01_residency_is_independent_of_registration_and_callability() {
    let (registry, _gateway) = world();
    for name in [
        "read",
        "write",
        "edit",
        "exec_command",
        "knowledge_search",
        "search_memory",
        "skills_list",
    ] {
        let mut tool = manifest();
        tool.local_name = name.into();
        registry.register(tool, &SchemaBudget::default()).unwrap();
    }
    let sent = ToolDeclarationSnapshot::from_registry(&registry).unwrap();
    let names: std::collections::BTreeSet<_> = sent
        .declarations
        .iter()
        .map(|d| d.wire_name.as_str())
        .collect();
    assert_eq!(
        names,
        lingxi_service::toolcatalog_bridge::RESIDENT_NAMES
            .into_iter()
            .collect()
    );
    assert!(registry
        .prepare_invocation(
            &ToolTargetRef::ByName {
                name: "knowledge_search".into()
            },
            None,
            &json!({"path":"synthetic.txt"}),
            &SchemaBudget::default()
        )
        .is_ok());
    println!(
        "{}",
        json!({"case":"F07-C01-IN_PROCESS","names":names,"actualBinaryWire":"BLOCKED_LOOPBACK_BIND_EPERM","tokenCount":null})
    );
}
#[test]
fn first_layer_c04_withdrawal_updates_current_declarations_preserves_old_five_protocol_pairs() {
    let registry = ToolRegistry::new();
    let receipt = registry
        .register(manifest(), &SchemaBudget::default())
        .unwrap();
    let sent = ToolDeclarationSnapshot::from_registry(&registry).unwrap();
    let req = request("read", json!({"path":"old-authorized.txt"}))
        .with_provider_call_id("original-provider-call");
    let call = RequestedToolCall {
        declaration_binding: sent.bind_target(&req.target),
        tool_call_id: ToolCallId::new("original-host-call"),
        provider_call_id: req.provider_call_id.clone(),
        target: req.target.clone(),
        arguments: req.arguments.clone(),
        args_digest: req.args_digest.clone(),
        args_summary: None,
    };
    registry
        .set_availability(
            &receipt.target_id,
            Availability::Disabled {
                reason: "合成撤权".into(),
            },
        )
        .unwrap();
    let current = ToolDeclarationSnapshot::from_registry(&registry).unwrap();
    assert!(current.declarations.is_empty());
    assert!(registry
        .prepare_invocation(
            &ToolTargetRef::ByTargetId {
                target_id: receipt.target_id.clone()
            },
            None,
            req.arguments.as_value(),
            &SchemaBudget::default()
        )
        .is_err());
    for family in families() {
        let mut input = ModelTurnInput::first_turn("继续", current.clone());
        input.prior = vec![
            ExchangeItem::AssistantTurn {
                call: ModelCallId::new("original-model-call"),
                content: vec![],
                tool_calls: vec![call.clone()],
                origin: None,
            },
            ExchangeItem::ToolResult {
                tool_call_id: call.tool_call_id.clone(),
                provider_call_id: call.provider_call_id.clone(),
                outcome: ToolOutcome::success_text("OLD-RESULT-NONCE"),
            },
        ];
        let wire = render(&input, family).unwrap();
        assert!(wire.to_string().contains("OLD-RESULT-NONCE"));
        assert!(wire.to_string().contains("original-provider-call"));
        println!(
            "{}",
            json!({"case":"F07-C04","family":format!("{family:?}"),"wire":wire,"currentGeneration":current.catalog_generation,"historicalGeneration":sent.catalog_generation,"executionRefused":true})
        );
    }
}

#[tokio::test]
async fn on_demand_calls_require_this_run_description_and_current_generation() {
    let (registry, gateway) = world();
    let ctx = context("catalog-pinned");
    let _lease = gateway.catalog_run_lease(&ctx).unwrap();
    let mut tool = manifest();
    tool.local_name = "knowledge_fixture".into();
    tool.availability = Availability::Deferred;
    let receipt = registry.register(tool, &SchemaBudget::default()).unwrap();
    gateway.bind_executor(
        receipt.target_id.clone(),
        Arc::new(Echo),
        "本项仅检查目录身份与撤权",
    );
    let wrapped = request(
        "mcp_call",
        json!({"tool":"knowledge_fixture","arguments":{"path":"synthetic.txt"}}),
    );
    let current_pin = || {
        Some(CatalogPin {
            catalog_generation: registry.catalog_generation(),
        })
    };
    // 参数中自报目标不能代替当前运行读取过的正式说明。
    assert!(gateway
        .normalize_model_request(&ctx, &wrapped, current_pin())
        .is_err());
    text(
        dispatch(
            &gateway,
            &ctx,
            &request("mcp_describe_tool", json!({"name":"knowledge_fixture"})),
        )
        .await,
    );
    assert!(gateway
        .normalize_model_request(&ctx, &wrapped, current_pin())
        .is_ok());
    assert!(gateway
        .normalize_model_request(&context("different-run"), &wrapped, current_pin())
        .is_err());
    // 目录变化后旧说明失效；重新读取也不能使已撤销目标恢复执行权限。
    registry
        .set_availability(
            &receipt.target_id,
            Availability::Disabled {
                reason: "已撤销".into(),
            },
        )
        .unwrap();
    assert!(gateway
        .normalize_model_request(&ctx, &wrapped, current_pin())
        .is_err());
    text(
        dispatch(
            &gateway,
            &ctx,
            &request("mcp_describe_tool", json!({"name":"knowledge_fixture"})),
        )
        .await,
    );
    assert!(gateway
        .normalize_model_request(&ctx, &wrapped, current_pin())
        .is_err());
}
