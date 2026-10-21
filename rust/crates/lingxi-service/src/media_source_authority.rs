//! 媒体固定后台任务持续重读来源权限；父任务正常结束不取消已批准生成，撤权立即停止交付。
use lingxi_adapters::{
    conversation_files::ConversationFiles,
    storage::{
        bridge as bridge_store, bridge_reads, conversations as phone_store, media::MediaJob,
        session_tree, RunDatabase,
    },
};
use lingxi_kernel::{
    ports::StoragePort,
    subagent::{RunOrigin, SessionPermissionMode},
};
use lingxi_protocol::{BridgeConversationKind, BridgeRole, RunId};
use std::{collections::BTreeSet, path::Path};

pub(super) async fn validate(db: &RunDatabase, job: &MediaJob, home: &Path) -> Result<(), String> {
    let bad = || "媒体来源权限已撤销或无法确认".to_owned();
    let mode =
        SessionPermissionMode::parse(job.state["sourcePermissionMode"].as_str().ok_or_else(bad)?)
            .ok_or_else(bad)?;
    if mode == SessionPermissionMode::ReadOnly {
        return Err(bad());
    }
    let facts=db.query_one_text("SELECT json_object('kind',owner_kind,'subject',owner_subject) FROM runs WHERE run_id=?1 AND session_id=?2",vec![job.source_run_id.clone(),job.session_id.clone()]).await.map_err(|_|bad())?.ok_or_else(bad)?;
    let facts: serde_json::Value = serde_json::from_str(&facts).map_err(|_| bad())?;
    let principal = lingxi_kernel::recovery::principal_from_storage_facts(
        facts["kind"].as_str().ok_or_else(bad)?,
        facts["subject"].as_str().ok_or_else(bad)?,
    )
    .map_err(|_| bad())?;
    let mut id = RunId::new(job.source_run_id.clone());
    let mut seen = BTreeSet::new();
    loop {
        if seen.len() >= 64 || !seen.insert(id.to_string()) {
            return Err(bad());
        }
        let actual = db.load_run(&id).await.map_err(|_| bad())?.ok_or_else(bad)?;
        if !bridge_reads::has_run_identity(
            db,
            id.as_str(),
            actual.session_id.as_str(),
            principal.storage_kind(),
            &principal.storage_subject(),
        )
        .await
        .map_err(|_| bad())?
        {
            return Err(bad());
        }
        let failed=db.query_one_text("SELECT run_id FROM runs WHERE run_id=?1 AND status IN('failed','cancelled','interrupted_needs_attention')",vec![id.to_string()]).await.map_err(|_|bad())?;
        if failed.is_some() {
            return Err(bad());
        }
        let lineage = db
            .load_run_lineage(&id)
            .await
            .map_err(|_| bad())?
            .ok_or_else(bad)?;
        match lineage.origin {
            RunOrigin::Bridge => {
                if lineage.parent_run_id.is_some() {
                    return Err(bad());
                }
                let inbox =
                    bridge_store::get_inbox(db, lineage.cause_id.as_deref().ok_or_else(bad)?)
                        .await
                        .map_err(|_| bad())?
                        .ok_or_else(bad)?;
                let expected = crate::sessions::kernel_principal_of(
                    &crate::auth::principal_for_bridge_inbox(&inbox).map_err(|_| bad())?,
                );
                // Guest 的正式工具策略不允许生成；这里再以持久角色收窄后台来源。
                if !inbox.prepared
                    || inbox.role != BridgeRole::Owner
                    || inbox.owner_subject != lingxi_kernel::LOCAL_OWNER_SUBJECT
                    || expected != principal
                    || !matches!(
                        inbox.status,
                        bridge_store::BridgeInboxStatus::Claimed
                            | bridge_store::BridgeInboxStatus::Running
                            | bridge_store::BridgeInboxStatus::Succeeded
                    )
                    || lineage.source_message_id.as_deref() != Some(inbox.input.message_id.as_str())
                {
                    return Err(bad());
                }
                let (actual_run, sid, _) = bridge_store::get_inbox_run(db, &inbox.id)
                    .await
                    .map_err(|_| bad())?
                    .ok_or_else(bad)?;
                if actual_run != id.as_str()
                    || inbox.session_id.as_deref() != Some(sid.as_str())
                    || sid != job.session_id
                {
                    return Err(bad());
                }
                let session = session_tree::get_session_row(db, &sid)
                    .await
                    .map_err(|_| bad())?
                    .ok_or_else(bad)?;
                if session.lifecycle != "active"
                    || session.agent_id != inbox.actor_id
                    || !principal.can_access_session(&session.owner_user_id, &sid)
                {
                    return Err(bad());
                }
                let connector = bridge_store::get_connector(db, &inbox.input.connector_id)
                    .await
                    .map_err(|_| bad())?
                    .ok_or_else(bad)?;
                let role = if inbox.input.conversation_kind == BridgeConversationKind::Dm
                    && connector.input.owner_sender_id.as_deref()
                        == Some(inbox.input.sender_id.as_str())
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
                    || role != inbox.role
                    || crate::bridge_permission::read_current(Some(home))
                        .map_err(|_| bad())?
                        .narrows(mode)
                {
                    return Err(bad());
                }
                return Ok(());
            }
            RunOrigin::Channel | RunOrigin::Dm => {
                if principal != lingxi_kernel::Principal::LocalUser {
                    return Err(bad());
                }
                let delivery =
                    phone_store::get_delivery(db, lineage.cause_id.as_deref().ok_or_else(bad)?)
                        .await
                        .map_err(|_| bad())?
                        .ok_or_else(bad)?;
                let (actual_run, sid, status) =
                    phone_store::get_delivery_run(db, &delivery.input.delivery_id)
                        .await
                        .map_err(|_| bad())?
                        .ok_or_else(bad)?;
                if actual_run != id.as_str()
                    || lineage.parent_run_id.as_ref().map(|r| r.as_str())
                        != delivery.input.source_run_id.as_deref()
                    || lineage.source_message_id.as_deref()
                        != Some(delivery.input.source_message_id.as_str())
                    || matches!(
                        status.as_str(),
                        "failed" | "cancelled" | "interrupted_needs_attention"
                    )
                    || !matches!(
                        delivery.status,
                        phone_store::PhoneDeliveryStatus::Claimed
                            | phone_store::PhoneDeliveryStatus::Running
                            | phone_store::PhoneDeliveryStatus::Completed
                    )
                {
                    return Err(bad());
                }
                return phone(db, &delivery, &sid, home, mode).await;
            }
            RunOrigin::Subagent | RunOrigin::SessionCollab => {
                if lineage.origin == RunOrigin::SessionCollab {
                    let cause = lineage.cause_id.as_deref().ok_or_else(bad)?;
                    let draft =
                        lingxi_adapters::storage::conversations_collab::get_collab_draft(db, cause)
                            .await
                            .map_err(|_| bad())?
                            .ok_or_else(bad)?;
                    let (actual_run, _, _) =
                        lingxi_adapters::storage::conversations_collab::get_collab_run(db, cause)
                            .await
                            .map_err(|_| bad())?
                            .ok_or_else(bad)?;
                    if actual_run != id.as_str()
                        || lineage.parent_run_id.as_ref().map(|r| r.as_str())
                            != Some(draft.input.source_run_id.as_str())
                    {
                        return Err(bad());
                    }
                }
                id = lineage.parent_run_id.ok_or_else(bad)?;
            }
            RunOrigin::Media => return Err(bad()),
            RunOrigin::Loop => {
                // Loop 来源的正式持久授权 = 落盘循环记录本体：run 必须是
                // 循环根（无父 run）、主体是本机用户、cause_id 逐字等于目标
                // 会话，且会话仍活跃；循环记录存在且状态活跃（Running/
                // Paused）。stop/complete 原子写终态，持续重查的下一拍即拒
                // ——撤权立即停止交付；记录缺失/损坏/版本不符同样拒。
                if lineage.parent_run_id.is_some()
                    || principal != lingxi_kernel::Principal::LocalUser
                    || lineage.cause_id.as_deref() != Some(actual.session_id.as_str())
                    || actual.session_id.as_str() != job.session_id
                {
                    return Err(bad());
                }
                let session = session_tree::get_session_row(db, actual.session_id.as_str())
                    .await
                    .map_err(|_| bad())?
                    .ok_or_else(bad)?;
                if session.lifecycle != "active" {
                    return Err(bad());
                }
                let raw = std::fs::read_to_string(home.join(".ephemeral").join("loop-state.json"))
                    .map_err(|_| bad())?;
                let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|_| bad())?;
                if parsed
                    .get("schemaVersion")
                    .and_then(serde_json::Value::as_u64)
                    != Some(1)
                {
                    return Err(bad());
                }
                let loops = parsed
                    .get("loops")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(bad)?;
                let mut active = false;
                for entry in loops {
                    let record: crate::loopx::LoopRecord =
                        serde_json::from_value(entry.clone()).map_err(|_| bad())?;
                    if record.key == actual.session_id.as_str() {
                        active = record.status.is_active();
                        break;
                    }
                }
                if !active {
                    return Err(bad());
                }
                return Ok(());
            }
            RunOrigin::Beautify => {
                // 封面后台任务的正式持久授权 = 持久化封面作业本体：
                // run/会话/主体逐字一致，且受理时冻结写事实已绑定（付费前完成）。
                let cause = lineage.cause_id.as_deref().ok_or_else(bad)?;
                let cover = lingxi_adapters::storage::beautify::get(db, cause)
                    .await
                    .map_err(|_| bad())?
                    .ok_or_else(bad)?;
                if cover.run_id.as_deref() != Some(id.as_str())
                    || cover.session_id != actual.session_id.as_str()
                    || cover.principal_kind != principal.storage_kind()
                    || cover.principal_subject != principal.storage_subject()
                    || cover.admitted_write.is_none()
                    || matches!(
                        cover.phase,
                        lingxi_adapters::storage::beautify::CoverPhase::Failed
                            | lingxi_adapters::storage::beautify::CoverPhase::Cancelled
                            | lingxi_adapters::storage::beautify::CoverPhase::Unknown
                    )
                {
                    return Err(bad());
                }
                return Ok(());
            }
            _ => {
                if !matches!(lineage.origin.wire_name(), "user" | "cron" | "heartbeat") {
                    return Err("媒体后台来源尚未绑定正式持久授权".into());
                }
                if principal.bridge_actor_subject().is_some() {
                    return Err(bad());
                }
                return Ok(());
            }
        }
    }
}
async fn phone(
    db: &RunDatabase,
    d: &phone_store::PhoneDelivery,
    sid: &str,
    home: &Path,
    mode: SessionPermissionMode,
) -> Result<(), String> {
    let bad = || "媒体来源频道或私聊权限已撤销或无法确认".to_owned();
    let session = session_tree::get_session_row(db, sid)
        .await
        .map_err(|_| bad())?
        .ok_or_else(bad)?;
    if session.lifecycle != "active"
        || session.agent_id != d.input.actor_id
        || session.owner_user_id != d.input.owner_subject
    {
        return Err(bad());
    }
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
        crate::phone_settings::PhoneSettings::read(&doc.meta, members.len()).map_err(|_| bad())?
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
        crate::phone_settings::PhoneSettings::read_dm(&doc.meta).map_err(|_| bad())?
    };
    let binding = phone_store::get_phone_binding(
        db,
        &d.input.actor_id,
        &d.input.conversation_id,
        &d.input.owner_subject,
    )
    .await
    .map_err(|_| bad())?
    .ok_or_else(bad)?;
    if binding.input.session_id != sid
        || d.input.input_json["bindingEpoch"]
            .as_u64()
            .is_some_and(|epoch| epoch != binding.epoch)
        || (settings.mode == "read_only" && mode != SessionPermissionMode::ReadOnly)
    {
        return Err(bad());
    }
    Ok(())
}
