//! 桥接运行在实际派发前重读正式因果和授权事实；校验失败由驱动统一取消收尾。
use crate::runs::{DriveAuthorization, RunGrant};
use lingxi_adapters::storage::{bridge as store, bridge_reads, session_tree, RunDatabase};
use lingxi_kernel::{ports::StoragePort, subagent::RunOrigin, RunContext};
use lingxi_protocol::{BridgeConversationKind, BridgeRole};

pub(crate) async fn validate(
    storage: Option<&RunDatabase>,
    authorization: &DriveAuthorization,
    ctx: &RunContext,
    agent: &str,
    home: Option<&std::path::Path>,
) -> Result<(), String> {
    let bridge_principal = ctx.principal.bridge_actor_subject().is_some();
    if !bridge_principal && !matches!(authorization.grant, RunGrant::Bridge { .. }) {
        return Ok(());
    }
    let invalid = || "bridge_run_authority_revoked_or_unavailable".to_string();
    let db = storage.ok_or_else(invalid)?;
    if !bridge_principal
        || !matches!(
            authorization.grant,
            RunGrant::Bridge { .. } | RunGrant::Subagent { .. }
        )
    {
        return Err(invalid());
    }
    // 子任务沿真实父谱系追溯到原收件；每级身份、会话和边界都不能由模型参数替代。
    let mut root_run = ctx.run_id.clone();
    let mut lineage = db
        .load_run_lineage(&root_run)
        .await
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if lineage != authorization.lineage {
        return Err(invalid());
    }
    let delegated = matches!(authorization.grant, RunGrant::Subagent { .. });
    let mut visited = std::collections::BTreeSet::new();
    loop {
        if visited.len() >= 64
            || !visited.insert(root_run.to_string())
            || !bridge_reads::has_run_identity(
                db,
                root_run.as_str(),
                ctx.session_id.as_str(),
                ctx.principal.storage_kind(),
                &ctx.principal.storage_subject(),
            )
            .await
            .map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        if lineage.origin == RunOrigin::Bridge && lineage.parent_run_id.is_none() {
            break;
        }
        if !delegated || lineage.origin != RunOrigin::Subagent {
            return Err(invalid());
        }
        root_run = lineage.parent_run_id.ok_or_else(invalid)?;
        lineage = db
            .load_run_lineage(&root_run)
            .await
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
    }
    let cause = lineage
        .cause_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or_else(invalid)?;
    let inbox = store::get_inbox(db, cause)
        .await
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    let expected = crate::auth::principal_for_bridge_inbox(&inbox).map_err(|_| invalid())?;
    let role = match authorization.grant {
        RunGrant::Bridge { role } if !delegated => role,
        RunGrant::Subagent { .. } if inbox.role == BridgeRole::Owner => inbox.role,
        _ => return Err(invalid()),
    };
    if !inbox.prepared
        || (!delegated && inbox.actor_id != agent)
        || inbox.owner_subject != lingxi_kernel::LOCAL_OWNER_SUBJECT
        || inbox.role != role
        || !(matches!(
            inbox.status,
            store::BridgeInboxStatus::Claimed | store::BridgeInboxStatus::Running
        ) || (delegated && inbox.status == store::BridgeInboxStatus::Succeeded))
        || inbox.session_id.as_deref() != Some(ctx.session_id.as_str())
        || lineage.source_message_id.as_deref() != Some(inbox.input.message_id.as_str())
        || crate::sessions::kernel_principal_of(&expected) != ctx.principal
    {
        return Err(invalid());
    }
    // get_inbox_run 校验真实 Run 的来源、会话、主人和接管范围；不能依赖尚未写回的 run_id。
    let (actual_run, actual_session, _) = store::get_inbox_run(db, cause)
        .await
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if actual_run != root_run.as_str() || actual_session != ctx.session_id.as_str() {
        return Err(invalid());
    }
    let session = session_tree::get_session_row(db, ctx.session_id.as_str())
        .await
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if session.lifecycle != "active"
        || session.agent_id != inbox.actor_id
        || !ctx
            .principal
            .can_access_session(&session.owner_user_id, ctx.session_id.as_str())
    {
        return Err(invalid());
    }
    // 放在最后读取，使准备、审计或排队期间发生的配置变化在派发前被观察到。
    let connector = store::get_connector(db, &inbox.input.connector_id)
        .await
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    let current_role = if inbox.input.conversation_kind == BridgeConversationKind::Dm
        && connector.input.owner_sender_id.as_deref() == Some(inbox.input.sender_id.as_str())
    {
        BridgeRole::Owner
    } else {
        BridgeRole::Guest
    };
    if !connector.input.enabled
        || connector.generation != inbox.connector_generation
        || connector.input.actor_id != inbox.actor_id
        || connector.input.platform != inbox.input.platform
        || connector.input.owner_subject != inbox.owner_subject
        || current_role != role
    {
        return Err(invalid());
    }
    // 最后同步读取模式，覆盖模型、审阅或数据库检查等待期间的权限收窄。
    if crate::bridge_permission::read_current(home)
        .map_err(|_| invalid())?
        .narrows(authorization.session_mode)
    {
        return Err(invalid());
    }
    Ok(())
}
