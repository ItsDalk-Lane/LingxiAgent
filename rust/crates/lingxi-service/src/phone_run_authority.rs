//! 手机执行沿真实谱系重读成员、开关与会话绑定；模型载荷不能制造后台身份。
use crate::{
    phone_settings::PhoneSettings,
    runs::{DriveAuthorization, RunGrant},
};
use lingxi_adapters::{
    conversation_files::ConversationFiles,
    storage::{conversations as store, session_tree, RunDatabase},
};
use lingxi_kernel::{ports::StoragePort, subagent::RunOrigin, RunContext};

pub(crate) async fn scope(
    db: &RunDatabase,
    authorization: &DriveAuthorization,
    ctx: &RunContext,
    admitted_parentless_subagent: bool,
) -> Result<Option<store::PhoneDelivery>, String> {
    let bad = || "phone_run_authority_revoked_or_unavailable".to_string();
    let mut id = ctx.run_id.clone();
    let mut lineage = db
        .load_run_lineage(&id)
        .await
        .map_err(|_| bad())?
        .ok_or_else(bad)?;
    if lineage != authorization.lineage {
        return Err(bad());
    }
    // 历史受限驱动允许无父子任务，但必须由统一驱动核实真实预登记许可。
    // 有父链、后台来源或任何来源标记都不能借此跳过逐级追溯。
    if admitted_parentless_subagent
        && matches!(authorization.grant, RunGrant::Subagent { .. })
        && lineage.origin == RunOrigin::Subagent
        && lineage.parent_run_id.is_none()
        && lineage.source_message_id.is_none()
        && lineage.cause_id.is_none()
    {
        return Ok(None);
    }
    let mut seen = std::collections::BTreeSet::new();
    let collab_entry = authorization.lineage.origin == RunOrigin::SessionCollab;
    loop {
        if !seen.insert(id.to_string()) || seen.len() > 64 {
            return Err(bad());
        }
        if matches!(lineage.origin, RunOrigin::Channel | RunOrigin::Dm) {
            if ctx.principal != lingxi_kernel::Principal::LocalUser
                || !(matches!(
                    authorization.grant,
                    RunGrant::Phone | RunGrant::Subagent { .. }
                ) || (collab_entry && authorization.grant == RunGrant::Full))
            {
                return Err(bad());
            }
            let delivery = store::get_delivery(db, lineage.cause_id.as_deref().ok_or_else(bad)?)
                .await
                .map_err(|_| bad())?
                .ok_or_else(bad)?;
            let (actual, sid, _) = store::get_delivery_run(db, &delivery.input.delivery_id)
                .await
                .map_err(|_| bad())?
                .ok_or_else(bad)?;
            if actual != id.as_str()
                || delivery.input.source_run_id.as_deref()
                    != lineage.parent_run_id.as_ref().map(|r| r.as_str())
                || lineage.source_message_id.as_deref() != Some(&delivery.input.source_message_id)
            {
                return Err(bad());
            }
            let root_session = session_tree::get_session_row(db, &sid)
                .await
                .map_err(|_| bad())?
                .ok_or_else(bad)?;
            if root_session.agent_id != delivery.input.actor_id
                || root_session.owner_user_id != delivery.input.owner_subject
                || root_session.lifecycle != "active"
            {
                return Err(bad());
            }
            return Ok(Some(delivery));
        }
        if !matches!(
            lineage.origin,
            RunOrigin::Subagent | RunOrigin::SessionCollab
        ) {
            if matches!(authorization.grant, RunGrant::Phone) {
                return Err(bad());
            }
            return Ok(None);
        }
        if lineage.origin == RunOrigin::SessionCollab {
            let cause = lineage.cause_id.as_deref().ok_or_else(bad)?;
            let draft = lingxi_adapters::storage::conversations_collab::get_collab_draft(db, cause)
                .await
                .map_err(|_| bad())?
                .ok_or_else(bad)?;
            let actual = lingxi_adapters::storage::conversations_collab::get_collab_run(db, cause)
                .await
                .map_err(|_| bad())?
                .ok_or_else(bad)?;
            if actual.0 != id.as_str()
                || lineage.parent_run_id.as_ref().map(|r| r.as_str())
                    != Some(draft.input.source_run_id.as_str())
            {
                return Err(bad());
            }
        }
        // 每一级后代须保留同一主人和真实会话，不信任仅有parent id的伪谱系。
        let parent = lineage.parent_run_id.ok_or_else(bad)?;
        let actual = db.load_run(&id).await.map_err(|_| bad())?.ok_or_else(bad)?;
        if id == ctx.run_id && actual.session_id != ctx.session_id {
            return Err(bad());
        }
        let valid = lingxi_adapters::storage::bridge_reads::has_run_identity(
            db,
            id.as_str(),
            actual.session_id.as_str(),
            ctx.principal.storage_kind(),
            &ctx.principal.storage_subject(),
        )
        .await
        .map_err(|_| bad())?;
        if !valid {
            return Err(bad());
        }
        id = parent;
        lineage = db
            .load_run_lineage(&id)
            .await
            .map_err(|_| bad())?
            .ok_or_else(bad)?;
    }
}
pub(crate) async fn validate(
    db: Option<&RunDatabase>,
    authorization: &DriveAuthorization,
    ctx: &RunContext,
    home: Option<&std::path::Path>,
    admitted_parentless_subagent: bool,
) -> Result<bool, String> {
    let Some(db) = db else {
        return if matches!(authorization.grant, RunGrant::Phone) {
            Err("phone storage unavailable".into())
        } else {
            Ok(false)
        };
    };
    let Some(d) = scope(db, authorization, ctx, admitted_parentless_subagent).await? else {
        return Ok(false);
    };
    let bad = || "phone_run_authority_revoked_or_unavailable".to_string();
    let home = home.ok_or_else(bad)?;
    let prefs =
        crate::memory::MemoryService::read_preferences_root_at_home(home).map_err(|_| bad())?;
    if prefs["channels_enabled"] != true {
        return Err(bad());
    }
    let files = ConversationFiles::new(home.to_path_buf()).map_err(|_| bad())?;
    let settings = if d.input.conversation_type == "channel" {
        let doc = files
            .read_channel(&d.input.conversation_id)
            .map_err(|_| bad())?;
        let members = crate::conversations::members(&doc).map_err(|_| bad())?;
        if !members.contains(&d.input.actor_id)
            || crate::conversations::digest(serde_json::json!(members)) != d.input.membership_digest
        {
            return Err(bad());
        }
        PhoneSettings::read(&doc.meta, members.len()).map_err(|_| bad())?
    } else {
        let peer = d
            .input
            .conversation_id
            .strip_prefix("dm:")
            .ok_or_else(bad)?;
        for actor in [&d.input.actor_id, peer] {
            let dir = home.join("agents").join(actor);
            if !dir.is_dir()
                || dir.join(".deleted-agent.json").symlink_metadata().is_ok()
                || !dir.join("config.yaml").is_file()
                || std::fs::symlink_metadata(&dir)
                    .map_err(|_| bad())?
                    .file_type()
                    .is_symlink()
            {
                return Err(bad());
            }
        }
        let doc = files
            .read_phone(&d.input.actor_id, &d.input.conversation_id)
            .map_err(|_| bad())?;
        if crate::conversations::dm_scope_digest(&d.input.actor_id, peer, &doc.meta)
            != d.input.membership_digest
        {
            return Err(bad());
        }
        PhoneSettings::read_dm(&doc.meta).map_err(|_| bad())?
    };
    let binding = store::get_phone_binding(
        db,
        &d.input.actor_id,
        &d.input.conversation_id,
        &d.input.owner_subject,
    )
    .await
    .map_err(|_| bad())?
    .ok_or_else(bad)?;
    let (root_run, sid, root_status) = store::get_delivery_run(db, &d.input.delivery_id)
        .await
        .map_err(|_| bad())?
        .ok_or_else(bad)?;
    if binding.input.session_id != sid
        || d.input.input_json["bindingEpoch"]
            .as_u64()
            .is_some_and(|e| e != binding.epoch)
        || (settings.mode == "read_only"
            && authorization.session_mode
                != lingxi_kernel::subagent::SessionPermissionMode::ReadOnly)
    {
        return Err(bad());
    }
    if matches!(
        root_status.as_str(),
        "failed" | "cancelled" | "interrupted_needs_attention"
    ) {
        return Err(bad());
    }
    if !matches!(
        d.status,
        store::PhoneDeliveryStatus::Claimed | store::PhoneDeliveryStatus::Running
    ) && !(d.status == store::PhoneDeliveryStatus::Completed && root_run != ctx.run_id.as_str())
    {
        return Err(bad());
    }
    Ok(true)
}
