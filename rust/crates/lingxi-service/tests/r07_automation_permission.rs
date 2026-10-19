//! 自动化沿真实网关判权；隔离文件验证只读继承与工作区边界不能被自动档放宽。
use std::path::PathBuf;
use std::sync::Arc;

use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{
    tool_target_id, Availability, DeclaredPermission, PermissionContract, PermissionKind,
    SchemaBudget, ToolManifest, ToolOrigin, ToolRegistry, ToolTargetRef,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{AttemptId, RunId, SessionId, ToolCallId, ToolSchemaDocument};
use lingxi_service::approval_service::{
    resolve_approval_policy, ApprovalPolicy, ApprovalService, ACTION_BLOCKED_BY_READ_ONLY,
    TOOL_APPROVAL_UNAVAILABLE,
};
use lingxi_service::filetools::{register_core_file_tools, NoopChangeLog};
use lingxi_service::inject::ManualClock;
use lingxi_service::resourceaccess::{ResourceAccess, ResourceOp, ResourceScope};
use lingxi_service::toolgateway::{
    CallerSurface, FailClosedPolicy, InvocationEntry, InvocationPermissionContext,
    InvocationPolicyMaterial, InvocationRequest, PolicyAdjudicationInput, PolicyVerdict,
    ToolInvocationGateway,
};
use serde_json::{json, Value};

const NOW: u64 = 1_792_000_000_000;

fn automation(mode: SessionPermissionMode) -> InvocationPermissionContext {
    InvocationPermissionContext::Automation { mode }
}

fn facts(
    name: &str,
    kind: PermissionKind,
    mode: SessionPermissionMode,
    origin: ToolOrigin,
    arguments: Value,
) -> (PolicyAdjudicationInput, InvocationPolicyMaterial) {
    (
        PolicyAdjudicationInput {
            entry: InvocationEntry::Direct,
            target_id: tool_target_id(&origin, name).unwrap().to_string(),
            local_name: name.into(),
            permission_kind: kind,
            capability_base: format!("{name}.execute"),
            args_digest_hex: lingxi_protocol::digest_arguments(&arguments).hex,
            principal_kind: "local_user",
            principal_subject: "user_local".into(),
            session_id: "r07-auto-session".into(),
            run_id: "r07-auto-run".into(),
            agent_id: "r07-agent".into(),
            permission_context: automation(mode),
        },
        InvocationPolicyMaterial {
            origin,
            declared_permission: DeclaredPermission::Execute,
            effective_arguments: arguments,
            resources: vec![],
        },
    )
}

fn service() -> ApprovalService {
    ApprovalService::new(Arc::new(ManualClock::new(NOW)))
}

fn denied_code(verdict: PolicyVerdict, expected: &str) {
    match verdict {
        PolicyVerdict::Denied { code, .. } => assert_eq!(code, expected),
        other => panic!("expected explicit zero-dispatch refusal, got {other:?}"),
    }
}

#[test]
fn auto_allows_known_routine_operations_with_bound_arguments() {
    let approvals = service();
    for (name, arguments) in [
        ("exec_command", json!({"argv":["pwd"]})),
        ("todo_write", json!({"todos":[]})),
        ("patrol_update_log", json!({"note":"无需行动"})),
        ("jian_update_status", json!({"status":"completed"})),
        ("pin_memory", json!({"content":"用户偏好"})),
        (
            "record_experience",
            json!({"category":"工作","content":"已核对"}),
        ),
        ("goal", json!({"action":"pause"})),
        ("terminal", json!({"action":"close","terminal_id":"owned"})),
    ] {
        let (input, material) = facts(
            name,
            PermissionKind::Execute,
            SessionPermissionMode::Ask,
            ToolOrigin::FirstParty,
            arguments,
        );
        assert_eq!(
            approvals.adjudicate_invocation(&input, &material),
            PolicyVerdict::Allowed,
            "{name}"
        );
    }
}

#[test]
fn auto_requires_real_automatic_review_for_boundaries_and_unknown_operations() {
    let approvals = service();
    for name in [
        "notify",
        "dm",
        "channel",
        "browser",
        "install_skill",
        "learn_lesson",
        "update_settings",
        "write_stdin",
        "show_card",
        "unknown_tool",
    ] {
        let (input, material) = facts(
            name,
            PermissionKind::Execute,
            SessionPermissionMode::Ask,
            ToolOrigin::FirstParty,
            json!({}),
        );
        denied_code(
            approvals.adjudicate_invocation(&input, &material),
            TOOL_APPROVAL_UNAVAILABLE,
        );
    }
    for action in ["start", "write", "unknown"] {
        let (input, material) = facts(
            "terminal",
            PermissionKind::Execute,
            SessionPermissionMode::Ask,
            ToolOrigin::FirstParty,
            json!({"action":action}),
        );
        denied_code(
            approvals.adjudicate_invocation(&input, &material),
            TOOL_APPROVAL_UNAVAILABLE,
        );
    }
}

#[test]
fn unattended_review_and_user_questions_never_wait_even_in_operate() {
    let approvals = service();
    for mode in [
        SessionPermissionMode::Ask,
        SessionPermissionMode::Operate,
        SessionPermissionMode::ReadOnly,
    ] {
        let (input, material) = facts(
            "unpin_memory",
            PermissionKind::Review,
            mode,
            ToolOrigin::FirstParty,
            json!({"id":"one"}),
        );
        denied_code(
            approvals.adjudicate_invocation(&input, &material),
            if mode == SessionPermissionMode::ReadOnly {
                ACTION_BLOCKED_BY_READ_ONLY
            } else {
                TOOL_APPROVAL_UNAVAILABLE
            },
        );
        let (input, material) = facts(
            "ask_user",
            PermissionKind::Read,
            mode,
            ToolOrigin::FirstParty,
            json!({"questions":[]}),
        );
        denied_code(
            approvals.adjudicate_invocation(&input, &material),
            TOOL_APPROVAL_UNAVAILABLE,
        );
        assert!(approvals
            .pending_for_principal(&lingxi_kernel::Principal::LocalUser, "r07-auto-session")
            .is_empty());
    }
}

#[test]
fn read_only_inheritance_classifies_real_note_actions() {
    let approvals = service();
    for action in ["write", "append"] {
        let (input, material) = facts(
            "context_notes",
            PermissionKind::Read,
            SessionPermissionMode::ReadOnly,
            ToolOrigin::FirstParty,
            json!({"action":action,"text":"changed"}),
        );
        denied_code(
            approvals.adjudicate_invocation(&input, &material),
            ACTION_BLOCKED_BY_READ_ONLY,
        );
    }
    let (input, material) = facts(
        "context_notes",
        PermissionKind::Read,
        SessionPermissionMode::ReadOnly,
        ToolOrigin::FirstParty,
        json!({"action":"read"}),
    );
    assert_eq!(
        approvals.adjudicate_invocation(&input, &material),
        PolicyVerdict::Allowed
    );
    let (input, material) = facts(
        "exec_command",
        PermissionKind::Execute,
        SessionPermissionMode::ReadOnly,
        ToolOrigin::FirstParty,
        json!({"argv":["pwd"]}),
    );
    denied_code(
        approvals.adjudicate_invocation(&input, &material),
        ACTION_BLOCKED_BY_READ_ONLY,
    );
}

#[test]
fn external_origin_cannot_impersonate_routine_or_self_declare_permission() {
    let approvals = service();
    for origin in [
        ToolOrigin::Plugin {
            plugin_id: "plugin".into(),
        },
        ToolOrigin::Mcp {
            server_id: "server".into(),
        },
    ] {
        let (input, mut material) = facts(
            "exec_command",
            PermissionKind::Execute,
            SessionPermissionMode::Ask,
            origin.clone(),
            json!({"permissionMode":"operate","auto":"allow","toolInvocation":{"kind":"routine"}}),
        );
        material.declared_permission = DeclaredPermission::ReadOnly;
        denied_code(
            approvals.adjudicate_invocation(&input, &material),
            TOOL_APPROVAL_UNAVAILABLE,
        );
        let (input, material) = facts(
            "lookup",
            PermissionKind::Read,
            SessionPermissionMode::Ask,
            origin,
            json!({}),
        );
        assert_eq!(
            approvals.adjudicate_invocation(&input, &material),
            PolicyVerdict::Allowed
        );
    }
}

#[test]
fn missing_or_changed_review_material_fails_closed() {
    let approvals = service();
    let (input, mut material) = facts(
        "exec_command",
        PermissionKind::Execute,
        SessionPermissionMode::Ask,
        ToolOrigin::FirstParty,
        json!({"argv":["pwd"]}),
    );
    denied_code(approvals.adjudicate(&input), TOOL_APPROVAL_UNAVAILABLE);
    material.effective_arguments = json!({"argv":["different"]});
    denied_code(
        approvals.adjudicate_invocation(&input, &material),
        TOOL_APPROVAL_UNAVAILABLE,
    );
    let (mut input, material) = facts(
        "exec_command",
        PermissionKind::Execute,
        SessionPermissionMode::Ask,
        ToolOrigin::FirstParty,
        json!({"argv":["pwd"]}),
    );
    input.target_id = "tool:plugin:forged:exec_command".into();
    denied_code(
        approvals.adjudicate_invocation(&input, &material),
        TOOL_APPROVAL_UNAVAILABLE,
    );
}

#[test]
fn native_file_routine_requires_a_bound_writable_resource() {
    let approvals = service();
    let (input, mut material) = facts(
        "write",
        PermissionKind::Execute,
        SessionPermissionMode::Ask,
        ToolOrigin::FirstParty,
        json!({"path":"inside.txt","content":"ok"}),
    );
    denied_code(
        approvals.adjudicate_invocation(&input, &material),
        TOOL_APPROVAL_UNAVAILABLE,
    );
    material.resources.push(ResourceScope::new(
        PathBuf::from("/isolated/inside.txt"),
        ResourceOp::Read,
    ));
    denied_code(
        approvals.adjudicate_invocation(&input, &material),
        TOOL_APPROVAL_UNAVAILABLE,
    );
    material.resources.push(ResourceScope::new(
        PathBuf::from("/isolated/inside.txt"),
        ResourceOp::Write,
    ));
    assert_eq!(
        approvals.adjudicate_invocation(&input, &material),
        PolicyVerdict::Allowed
    );
}

#[test]
fn automation_policy_resolution_keeps_interactive_ask_distinct() {
    assert_eq!(
        resolve_approval_policy(&automation(SessionPermissionMode::Ask)),
        ApprovalPolicy::DenyOnPrompt
    );
    assert_eq!(
        resolve_approval_policy(&automation(SessionPermissionMode::Operate)),
        ApprovalPolicy::Never
    );
    assert_eq!(
        resolve_approval_policy(&automation(SessionPermissionMode::ReadOnly)),
        ApprovalPolicy::DenyOnPrompt
    );
    assert_eq!(
        resolve_approval_policy(&InvocationPermissionContext::UserSession {
            mode: SessionPermissionMode::Ask
        }),
        ApprovalPolicy::Interactive
    );
}

fn manifest(name: &str, kind: PermissionKind) -> ToolManifest {
    ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: name.into(),
        display_name: name.into(),
        aliases: vec![],
        version: "1".into(),
        description: "隔离后台权限测试".into(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".into(),
            schema: json!({"type":"object","properties":{"content":{"type":"string","default":"bound-default"}},"additionalProperties":false}),
        },
        output_schema: None,
        permission: PermissionContract {
            kind,
            capability_base: format!("{name}.execute"),
        },
        availability: Availability::Available,
        timeout_ms: Some(1000),
        max_concurrency: Some(1),
        declared_permission: DeclaredPermission::Execute,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    }
}

fn context() -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("r07-auto-session"),
        run_id: RunId::new("r07-auto-run"),
        attempt: AttemptId::new("r07-auto-run#a1"),
        generation: 1,
    }
}

fn request(
    ctx: &RunContext,
    name: &str,
    mode: SessionPermissionMode,
    args: Value,
) -> InvocationRequest {
    InvocationRequest::from_trusted_entry(
        ctx,
        CallerSurface::UserRun,
        "r07-agent",
        automation(mode),
        ToolTargetRef::ByName { name: name.into() },
        None,
        args,
        ToolCallId::new(format!("call-{name}")),
    )
}

#[test]
fn real_gateway_supplies_schema_normalized_parameters_and_never_parks_background() {
    let registry = Arc::new(ToolRegistry::new());
    let budget = SchemaBudget::default();
    registry
        .register(manifest("pin_memory", PermissionKind::Execute), &budget)
        .unwrap();
    registry
        .register(manifest("notify", PermissionKind::Execute), &budget)
        .unwrap();
    let clock = Arc::new(ManualClock::new(NOW));
    let approvals = Arc::new(ApprovalService::new(clock.clone()));
    let gateway = ToolInvocationGateway::new(
        registry.clone(),
        approvals,
        clock.clone(),
        budget,
        60_000,
        16,
    );
    let ctx = context();
    let prepared = gateway
        .prepare(request(
            &ctx,
            "pin_memory",
            SessionPermissionMode::Ask,
            json!({}),
        ))
        .unwrap();
    assert_eq!(prepared.policy, PolicyVerdict::Allowed);
    assert_eq!(
        prepared.args_digest_hex,
        lingxi_protocol::digest_arguments(&json!({"content":"bound-default"})).hex
    );
    let refusal = gateway
        .prepare(request(
            &ctx,
            "notify",
            SessionPermissionMode::Ask,
            json!({}),
        ))
        .unwrap_err();
    assert!(refusal
        .to_tool_error()
        .message
        .contains(TOOL_APPROVAL_UNAVAILABLE));
    // 未配置真实策略的旧端口也不能使后台进入人工等待。
    let gateway = ToolInvocationGateway::new(
        registry,
        Arc::new(FailClosedPolicy),
        clock,
        budget,
        60_000,
        16,
    );
    assert!(
        matches!(gateway.prepare(request(&ctx,"pin_memory",SessionPermissionMode::Ask,json!({}))),Err(lingxi_service::toolgateway::GatewayRefusal::PolicyDenied{code,..}) if code==TOOL_APPROVAL_UNAVAILABLE)
    );
}

struct IsolatedRoot(PathBuf);
impl IsolatedRoot {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "lingxi-r07-automation-permission-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for IsolatedRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn real_native_write_respects_inherited_read_only_and_workspace_boundary() {
    let root = IsolatedRoot::new();
    let workspace = root.0.join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let workspace = std::fs::canonicalize(workspace).unwrap();
    let registry = Arc::new(ToolRegistry::new());
    let clock = Arc::new(ManualClock::new(NOW));
    let approvals = Arc::new(ApprovalService::new(clock.clone()));
    let budget = SchemaBudget::default();
    let gateway = ToolInvocationGateway::new(
        registry.clone(),
        approvals,
        clock.clone(),
        budget,
        60_000,
        16,
    );
    register_core_file_tools(
        &registry,
        &gateway,
        Arc::new(ResourceAccess::new(std::slice::from_ref(&workspace)).unwrap()),
        workspace.clone(),
        clock,
        Arc::new(NoopChangeLog),
        &budget,
    );
    let ctx = context();
    let inside = workspace.join("inside.txt");
    let prepared = gateway
        .prepare(request(
            &ctx,
            "write",
            SessionPermissionMode::Ask,
            json!({"path":inside,"content":"allowed"}),
        ))
        .unwrap();
    assert_eq!(prepared.policy, PolicyVerdict::Allowed);
    let outcome = gateway
        .execute_prepared(&ctx, &ToolCallId::new("call-write"), &prepared.handle)
        .await
        .unwrap();
    assert!(matches!(
        outcome.outcome,
        lingxi_kernel::ports::ToolOutcome::Success { .. }
    ));
    assert_eq!(std::fs::read_to_string(&inside).unwrap(), "allowed");
    assert!(
        matches!(gateway.prepare(request(&ctx,"write",SessionPermissionMode::ReadOnly,json!({"path":inside,"content":"forbidden"}))),Err(lingxi_service::toolgateway::GatewayRefusal::PolicyDenied{code,..}) if code==ACTION_BLOCKED_BY_READ_ONLY)
    );
    assert_eq!(std::fs::read_to_string(&inside).unwrap(), "allowed");
    let outside = root.0.join("outside.txt");
    for mode in [SessionPermissionMode::Ask, SessionPermissionMode::Operate] {
        assert!(matches!(
            gateway.prepare(request(
                &ctx,
                "write",
                mode,
                json!({"path":outside,"content":"escape"})
            )),
            Err(lingxi_service::toolgateway::GatewayRefusal::ResourceScopeDenied { .. })
        ));
    }
    assert!(!outside.exists());
}

/// 只替代外部意图模型的输出；授权绑定、失效和消费仍由真实网关执行。
struct AuthorizedReviewer {
    calls: std::sync::atomic::AtomicUsize,
    change_registry: Option<Arc<ToolRegistry>>,
}
impl lingxi_service::automation_review::AutoReviewPort for AuthorizedReviewer {
    fn review<'a>(
        &'a self,
        _request: &'a lingxi_service::automation_review::AutoReviewRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = lingxi_service::automation_review::AutoReviewDecision>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if let Some(registry) = &self.change_registry {
                let mut changed = manifest("notify", PermissionKind::Execute);
                changed.version = "changed-during-review".into();
                registry
                    .update(
                        &tool_target_id(&ToolOrigin::FirstParty, "notify").unwrap(),
                        changed,
                        &SchemaBudget::default(),
                    )
                    .unwrap();
            }
            lingxi_service::automation_review::AutoReviewDecision::Allowed {
                reason: "测试模型确认本次授权".into(),
                scope_relation: "exact".into(),
                evidence_ids: vec!["u0".into()],
            }
        })
    }
}

fn tool_request(content: &str) -> lingxi_kernel::ports::ToolRequest {
    lingxi_kernel::ports::ToolRequest::from_effective_arguments(
        "tool:first-party:notify",
        json!({"content":content}),
        &SchemaBudget::default(),
    )
    .unwrap()
}

#[tokio::test]
async fn automatic_review_grant_is_bound_to_one_call_and_consumed_once() {
    let registry = Arc::new(ToolRegistry::new());
    registry
        .register(
            manifest("notify", PermissionKind::Execute),
            &SchemaBudget::default(),
        )
        .unwrap();
    let clock = Arc::new(ManualClock::new(NOW));
    let gateway = ToolInvocationGateway::new(
        registry,
        Arc::new(ApprovalService::new(clock.clone())),
        clock,
        SchemaBudget::default(),
        60_000,
        16,
    );
    let reviewer = Arc::new(AuthorizedReviewer {
        calls: Default::default(),
        change_registry: None,
    });
    gateway.set_automatic_reviewer(reviewer.clone());
    let ctx = context();
    let call = ToolCallId::new("review-call");
    let permission = automation(SessionPermissionMode::Ask);
    let request = tool_request("one reminder");
    gateway
        .review_and_validate_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &request,
            None,
            &[],
            None,
        )
        .await
        .unwrap();
    gateway
        .validate_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &request,
            None,
        )
        .unwrap();
    assert!(gateway
        .validate_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &ToolCallId::new("different-call"),
            &request,
            None
        )
        .is_err());
    assert!(gateway
        .validate_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &tool_request("changed reminder"),
            None
        )
        .is_err());
    let other = RunContext {
        run_id: RunId::new("different-run"),
        ..ctx.clone()
    };
    assert!(gateway
        .validate_from_request_pinned(
            &other,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &request,
            None
        )
        .is_err());
    let prepared = gateway
        .prepare_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &request,
            None,
        )
        .unwrap();
    assert_eq!(prepared.policy, PolicyVerdict::Allowed);
    assert!(gateway
        .prepare_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &request,
            None
        )
        .is_err());
    assert!(gateway
        .review_and_validate_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &request,
            None,
            &[],
            None
        )
        .await
        .is_err());
    assert_eq!(reviewer.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn automatic_review_grants_expire_and_cannot_survive_registry_changes() {
    let registry = Arc::new(ToolRegistry::new());
    registry
        .register(
            manifest("notify", PermissionKind::Execute),
            &SchemaBudget::default(),
        )
        .unwrap();
    let clock = Arc::new(ManualClock::new(NOW));
    let gateway = ToolInvocationGateway::new(
        registry.clone(),
        Arc::new(ApprovalService::new(clock.clone())),
        clock.clone(),
        SchemaBudget::default(),
        60_000,
        16,
    );
    gateway.set_automatic_reviewer(Arc::new(AuthorizedReviewer {
        calls: Default::default(),
        change_registry: None,
    }));
    let ctx = context();
    let call = ToolCallId::new("expiry-call");
    let permission = automation(SessionPermissionMode::Ask);
    let request = tool_request("one reminder");
    gateway
        .review_and_validate_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &request,
            None,
            &[],
            None,
        )
        .await
        .unwrap();
    clock.advance(60_001);
    assert!(gateway
        .validate_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &request,
            None
        )
        .is_err());
    gateway.set_automatic_reviewer(Arc::new(AuthorizedReviewer {
        calls: Default::default(),
        change_registry: Some(registry),
    }));
    assert!(gateway
        .review_and_validate_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &request,
            None,
            &[],
            None
        )
        .await
        .is_err());
    assert!(gateway
        .validate_from_request_pinned(
            &ctx,
            CallerSurface::UserRun,
            "r07-agent",
            permission,
            &call,
            &request,
            None
        )
        .is_err());
}

fn authorization_review_request() -> lingxi_service::automation_review::AutoReviewRequest {
    let (input, material) = facts(
        "notify",
        PermissionKind::Execute,
        SessionPermissionMode::Ask,
        ToolOrigin::FirstParty,
        json!({"body":"记得喝水","url":"https://example.test/?token=secret"}),
    );
    lingxi_service::automation_review::AutoReviewRequest {
        ctx: context(),
        tool_call_id: ToolCallId::new("review"),
        input,
        material,
        evidence: vec![lingxi_service::automation_review::AuthorizationEvidence {
            id: "u0".into(),
            text: "定时提醒我喝水".into(),
        }],
        explicit_user_authorization: Some("定时提醒我喝水".into()),
    }
}

#[test]
fn real_reviewer_parser_rejects_broader_missing_or_fabricated_authorization() {
    use lingxi_service::automation_review::{
        parse_review_output, AutoReviewDecision, TOOL_APPROVAL_DENIED,
    };
    let request = authorization_review_request();
    for scope in ["exact", "contained"] {
        assert!(matches!(parse_review_output(&json!({"verdict":"authorized","scopeRelation":scope,"evidenceIds":["u0"],"reason":"用户保存的任务"}).to_string(),&request).unwrap(),AutoReviewDecision::Allowed{..}));
    }
    for (verdict, scope, ids) in [
        ("authorized", "broader", vec!["u0"]),
        ("ambiguous", "exact", vec!["u0"]),
        ("authorized", "exact", vec!["invented"]),
        ("authorized", "exact", vec![]),
    ] {
        assert!(
            matches!(parse_review_output(&json!({"verdict":verdict,"scopeRelation":scope,"evidenceIds":ids}).to_string(),&request).unwrap(),AutoReviewDecision::Denied{code,..} if code==TOOL_APPROVAL_UNAVAILABLE)
        );
    }
    assert!(
        matches!(parse_review_output(r#"{"verdict":"not_authorized","scopeRelation":"unrelated","evidenceIds":["u0"]}"#,&request).unwrap(),AutoReviewDecision::Denied{code,..} if code==TOOL_APPROVAL_DENIED)
    );
    let mut implicit = request.clone();
    implicit.explicit_user_authorization = None;
    assert!(
        matches!(parse_review_output(r#"{"verdict":"authorized","scopeRelation":"exact","evidenceIds":["u0"]}"#,&implicit).unwrap(),AutoReviewDecision::Denied{code,..} if code==TOOL_APPROVAL_UNAVAILABLE)
    );
    assert!(parse_review_output(r#"{"action":"allow"}"#, &request).is_err());
    assert!(parse_review_output("invalid JSON", &request).is_err());
}

#[test]
fn reviewer_payload_is_bounded_redacted_and_identity_checked() {
    use lingxi_service::automation_review::build_review_payload;
    let mut request = authorization_review_request();
    request.evidence[0].text = format!("用户授权 {}", "长".repeat(20_000));
    let payload = build_review_payload(&request).unwrap();
    assert!(payload.chars().count() < 12_000);
    assert!(!payload.contains("token=secret"));
    request.ctx.run_id = RunId::new("forged-run");
    assert!(build_review_payload(&request).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_approval_slot_uses_frozen_trigger_evidence_and_accounts_format_retry() {
    use lingxi_adapters::storage::{
        automation as store, run_store::SessionRow, RunDatabase, StoreOptions,
    };
    use lingxi_kernel::ports::StoragePort;
    use lingxi_service::automation_review::{
        AutoReviewDecision, AutoReviewPort, ModelAutoReviewer,
    };
    let root = IsolatedRoot::new();
    let db = Arc::new(
        RunDatabase::open(&root.0.join("runs.db"), StoreOptions::default())
            .await
            .unwrap(),
    );
    let ctx = context();
    db.ensure_session_seed(vec![SessionRow {
        session_id: ctx.session_id.to_string(),
        agent_id: "r07-agent".into(),
        owner_user_id: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
        title: "隔离审核".into(),
        created_at_unix_ms: NOW as i64,
    }])
    .await
    .unwrap();
    db.record_run_started(&ctx, NOW).await.unwrap();
    let job = store::AutomationJobInput {
        id: "review-job".into(),
        studio_id: "studio".into(),
        actor_id: "r07-agent".into(),
        owner_kind: "local_user".into(),
        owner_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
        schema_version: 4,
        config_json:
            json!({"actorAgentId":"r07-agent","prompt":"按时通过桌面提醒我喝水","enabled":true})
                .to_string(),
        next_run_at: Some(NOW as i64),
    };
    store::create_job(&db, &job, NOW as i64).await.unwrap();
    store::claim_trigger(
        &db,
        &store::TriggerInput {
            trigger_id: "trigger-review-test".into(),
            job_id: job.id.clone(),
            due_at: NOW as i64,
            config_revision: 1,
        },
        NOW as i64,
    )
    .await
    .unwrap();
    db.record_run_lineage(
        &ctx,
        lingxi_kernel::subagent::RunLineage {
            parent_run_id: None,
            origin: lingxi_kernel::subagent::RunOrigin::Cron,
            source_message_id: None,
            cause_id: Some("trigger-review-test".into()),
        },
        NOW,
    )
    .await
    .unwrap();
    // 修改当前计划，审核必须仍使用领取触发时的旧正文。
    let changed = store::AutomationJobInput {
        config_json:
            json!({"actorAgentId":"r07-agent","prompt":"新的计划正文，不属于本轮","enabled":true})
                .to_string(),
        ..job
    };
    store::update_job(&db, &changed.id, &changed, 1, NOW as i64 + 1)
        .await
        .unwrap();
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let seen_handler = seen.clone();
    let router=axum::Router::new().route("/v1/chat/completions",axum::routing::post(move |axum::Json(body):axum::Json<Value>| {
        let seen=seen_handler.clone();
        async move {
            let mut calls=seen.lock().unwrap();calls.push(body);
            let text=if calls.len()==1 {"invalid JSON".into()} else {json!({"verdict":"authorized","scopeRelation":"exact","evidenceIds":["trigger-review-test"],"reason":"用户保存的桌面提醒"}).to_string()};
            let mut output=String::new();
            for frame in [json!({"id":"approval-fixture","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),json!({"id":"approval-fixture","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),json!({"id":"approval-fixture","choices":[],"usage":{"prompt_tokens":17,"completion_tokens":5}})] {output.push_str(&format!("data: {frame}\n\n"));}
            output.push_str("data: [DONE]\n\n");
            ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],output)
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let plane=lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&json!({"providers":{"stub":{"protocol":"openai-completions","endpoint":endpoint,"auth":{"kind":"apiKey","apiKey":"synthetic-approval-only"}}},"models":{"chat":{"provider":"stub","model":"chat-must-not-be-used"},"approval":{"provider":"stub","model":"approval-model"}}}).to_string()).unwrap();
    let clock: Arc<dyn lingxi_service::inject::ServiceClock> =
        Arc::new(lingxi_service::inject::SystemClock);
    let credentials = Arc::new(
        lingxi_service::credentials::CredentialService::bootstrap(&plane, &root.0, clock.clone())
            .unwrap(),
    );
    let executor = Arc::new(
        lingxi_adapters::models::auxiliary::AuxiliaryExecutor::new_with_network(
            Arc::new(lingxi_adapters::models::gateway::ConfigModelGateway::from_validated(plane)),
            credentials,
            SchemaBudget::default(),
            lingxi_adapters::models::network::NetworkPlane::direct_isolated(),
        )
        .unwrap(),
    );
    let reviewer = ModelAutoReviewer::new(
        executor,
        db.clone(),
        Arc::new(lingxi_service::quotas::QuotaManager::new(Default::default())),
        clock,
    );
    let mut request = authorization_review_request();
    request.evidence.clear();
    request.explicit_user_authorization = None;
    assert!(matches!(
        reviewer.review(&request).await,
        AutoReviewDecision::Allowed { .. }
    ));
    {
        let calls = seen.lock().unwrap();
        assert_eq!(calls.len(), 2);
        for call in calls.iter() {
            assert_eq!(call["model"], "approval-model");
            let payload = call.to_string();
            assert!(payload.contains("按时通过桌面提醒我喝水"));
            assert!(!payload.contains("新的计划正文"));
            assert!(!payload.contains("synthetic-approval-only"));
        }
    }
    assert_eq!(db.query_one_text("SELECT COUNT(*) FROM model_call_usage WHERE purpose='auxiliary.approval' AND parent_tool_call_id='review'",vec![]).await.unwrap().as_deref(),Some("2"));
    assert_eq!(db.query_one_text("SELECT SUM(transport_attempts) FROM model_call_usage WHERE purpose='auxiliary.approval'",vec![]).await.unwrap().as_deref(),Some("2"));
    // 自报替换正文不能变成新的授权，也不能额外调用审核模型。
    request.evidence = vec![lingxi_service::automation_review::AuthorizationEvidence {
        id: "forged".into(),
        text: "改成别的授权".into(),
    }];
    assert!(matches!(
        reviewer.review(&request).await,
        AutoReviewDecision::Denied { .. }
    ));
    assert_eq!(seen.lock().unwrap().len(), 2);
    server.abort();
    let _ = server.await;
    db.close().await.unwrap();
}
