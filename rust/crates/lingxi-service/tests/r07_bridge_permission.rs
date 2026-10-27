//! 正式网关隔离验证：桥接访客无工具，owner 仍不能操作浏览器与电脑。
use lingxi_kernel::{
    subagent::SessionPermissionMode,
    toolcatalog::{
        tool_target_id, Availability, DeclaredPermission, PermissionContract, PermissionKind,
        SchemaBudget, ToolManifest, ToolOrigin, ToolRegistry, ToolTargetRef,
    },
    RunContext,
};
use lingxi_protocol::{AttemptId, BridgeRole, RunId, SessionId, ToolCallId, ToolSchemaDocument};
use lingxi_service::{
    approval_service::{ApprovalService, ACTION_BLOCKED_BY_READ_ONLY, TOOL_APPROVAL_UNAVAILABLE},
    inject::ManualClock,
    toolgateway::{
        CallerSurface, InvocationEntry, InvocationPermissionContext, InvocationPolicyMaterial,
        InvocationRequest, PolicyAdjudicationInput, PolicyVerdict, ToolInvocationGateway,
    },
};
use serde_json::{json, Value};
use std::sync::Arc;
fn permission(role: BridgeRole, mode: SessionPermissionMode) -> InvocationPermissionContext {
    InvocationPermissionContext::Bridge { role, mode }
}
fn facts(
    name: &str,
    kind: PermissionKind,
    role: BridgeRole,
    mode: SessionPermissionMode,
    args: Value,
) -> (PolicyAdjudicationInput, InvocationPolicyMaterial) {
    (
        PolicyAdjudicationInput {
            entry: InvocationEntry::Direct,
            target_id: tool_target_id(&ToolOrigin::FirstParty, name)
                .unwrap()
                .to_string(),
            local_name: name.into(),
            permission_kind: kind,
            capability_base: format!("{name}.execute"),
            args_digest_hex: lingxi_protocol::digest_arguments(&args).hex,
            principal_kind: "bridge",
            principal_subject: lingxi_protocol::bridge_subject("qq", "c", "s").unwrap(),
            session_id: "bridge-session".into(),
            run_id: "bridge-run".into(),
            agent_id: "agent".into(),
            permission_context: permission(role, mode),
        },
        InvocationPolicyMaterial {
            origin: ToolOrigin::FirstParty,
            declared_permission: DeclaredPermission::Execute,
            effective_arguments: args,
            resources: vec![],
        },
    )
}
#[test]
fn guest_has_no_tools_and_owner_keeps_read_only_and_no_human_approval() {
    let approvals = ApprovalService::new(Arc::new(ManualClock::new(1)));
    for mode in [
        SessionPermissionMode::Ask,
        SessionPermissionMode::Operate,
        SessionPermissionMode::ReadOnly,
    ] {
        let (input, material) = facts(
            "read",
            PermissionKind::Read,
            BridgeRole::Guest,
            mode,
            json!({}),
        );
        assert!(matches!(
            approvals.adjudicate_invocation(&input, &material),
            PolicyVerdict::Denied { .. }
        ));
    }
    let (input, material) = facts(
        "pin_memory",
        PermissionKind::Execute,
        BridgeRole::Owner,
        SessionPermissionMode::ReadOnly,
        json!({"content":"禁止写入"}),
    );
    assert!(
        matches!(approvals.adjudicate_invocation(&input,&material),PolicyVerdict::Denied{code,..} if code==ACTION_BLOCKED_BY_READ_ONLY)
    );
    let (input, material) = facts(
        "review",
        PermissionKind::Review,
        BridgeRole::Owner,
        SessionPermissionMode::Operate,
        json!({}),
    );
    assert!(
        matches!(approvals.adjudicate_invocation(&input,&material),PolicyVerdict::Denied{code,..} if code==TOOL_APPROVAL_UNAVAILABLE)
    );
    let (input, material) = facts(
        "pin_memory",
        PermissionKind::Execute,
        BridgeRole::Owner,
        SessionPermissionMode::Ask,
        json!({"content":"明确常规动作"}),
    );
    assert_eq!(
        approvals.adjudicate_invocation(&input, &material),
        PolicyVerdict::Allowed
    );
    let (input, material) = facts(
        "notify",
        PermissionKind::Execute,
        BridgeRole::Owner,
        SessionPermissionMode::Ask,
        json!({"text":"敏感外发","role":"owner","explicitAuthorization":true}),
    );
    assert!(
        matches!(approvals.adjudicate_invocation(&input,&material),PolicyVerdict::Denied{code,..} if code==TOOL_APPROVAL_UNAVAILABLE)
    );
}
fn manifest(name: &str) -> ToolManifest {
    ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: name.into(),
        display_name: name.into(),
        aliases: vec![],
        version: "1".into(),
        description: "隔离桥接权限验证".into(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".into(),
            schema: json!({"type":"object","additionalProperties":false}),
        },
        output_schema: None,
        permission: PermissionContract {
            kind: PermissionKind::Execute,
            capability_base: format!("{name}.execute"),
        },
        availability: Availability::Available,
        timeout_ms: Some(1000),
        max_concurrency: Some(1),
        declared_permission: DeclaredPermission::Execute,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    }
}
#[test]
fn real_gateway_bans_gui_before_an_allowing_policy_and_rejects_identity_context_mismatch() {
    let registry = Arc::new(ToolRegistry::new());
    for name in ["browser_click", "computer", "pin_memory"] {
        registry
            .register(manifest(name), &SchemaBudget::default())
            .unwrap();
    }
    let gateway = ToolInvocationGateway::new(
        registry,
        Arc::new(ApprovalService::new(Arc::new(ManualClock::new(1)))),
        Arc::new(ManualClock::new(1)),
        SchemaBudget::default(),
        60_000,
        16,
    );
    let ctx = RunContext {
        principal: lingxi_kernel::Principal::Bridge {
            subject: lingxi_protocol::bridge_subject("qq", "c", "s").unwrap(),
        },
        session_id: SessionId::new("bridge-session"),
        run_id: RunId::new("bridge-run"),
        attempt: AttemptId::new("bridge-run#a1"),
        generation: 1,
    };
    for (name, role) in [
        ("browser_click", BridgeRole::Owner),
        ("computer", BridgeRole::Owner),
        ("pin_memory", BridgeRole::Guest),
    ] {
        let req = InvocationRequest::from_trusted_entry(
            &ctx,
            CallerSurface::UserRun,
            "agent",
            permission(role, SessionPermissionMode::Operate),
            ToolTargetRef::ByName { name: name.into() },
            None,
            json!({}),
            ToolCallId::new(format!("call-{name}")),
        );
        assert!(gateway.prepare(req).is_err(), "{name}");
    }
    let req = InvocationRequest::from_trusted_entry(
        &ctx,
        CallerSurface::UserRun,
        "agent",
        InvocationPermissionContext::UserSession {
            mode: SessionPermissionMode::Operate,
        },
        ToolTargetRef::ByName {
            name: "pin_memory".into(),
        },
        None,
        json!({}),
        ToolCallId::new("forged-context"),
    );
    assert!(gateway.prepare(req).is_err());
}
