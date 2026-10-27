//! Bridge 每日 fresh-compact 宿主：只消费真实名册，持久收据先取得资格再执行。
//! 同日成功不重复；失败一小时后可重试，重启留下的 started/unknown 不自动重放。
use crate::slash::DailyBridgeTarget;
use crate::ServiceState;
use chrono::Timelike;
use lingxi_adapters::storage::{automation as store, session_tree};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const DAILY_JOB_SPACING_MS: u64 = 5_000;
const REVOCATION_CHECK_MS: u64 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetGuard {
    permission: Option<String>,
    folders: Vec<String>,
    config_hash: String,
    bridge_preferences_hash: String,
    bridge_actor: Option<DailyBridgeActor>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct DailyBridgeActor {
    platform: String,
    connector_id: String,
    sender_id: String,
    generation: u64,
    config_hash: String,
}

/// 从当前配置和真实入站收据导出身份；slash 名册本身不授予 owner 权限。
async fn actual_bridge_actor(
    state: &ServiceState,
    target: &DailyBridgeTarget,
) -> Result<Option<DailyBridgeActor>, String> {
    let raw = state.storage().query_one_text(
        "SELECT json_object('platform',i.platform,'connectorId',i.connector_id,'senderId',i.sender_id,'generation',c.generation,'config',c.config_json) FROM bridge_inbox_v1 i JOIN bridge_connectors_v1 c ON c.id=i.connector_id WHERE i.session_id=?1 AND i.owner_subject='user_local' AND i.actor_id=?2 AND i.role='owner' AND i.conversation_kind='dm' AND c.enabled=1 AND c.generation=i.connector_generation AND c.platform=i.platform AND c.actor_id=i.actor_id AND c.owner_sender_id=i.sender_id AND c.owner_subject='user_local' ORDER BY i.created_at DESC,i.id DESC LIMIT 1",
        vec![target.session_id.clone(),target.agent_id.clone()]
    ).await.map_err(|e|e.to_string())?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let facts: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let text = |name: &str| {
        facts[name]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("daily Bridge {name} missing"))
    };
    let actor = DailyBridgeActor {
        platform: text("platform")?,
        connector_id: text("connectorId")?,
        sender_id: text("senderId")?,
        generation: facts["generation"]
            .as_u64()
            .ok_or("daily Bridge generation missing")?,
        config_hash: lingxi_kernel::session_tree::sha256_hex(text("config")?.as_bytes()),
    };
    if lingxi_protocol::bridge_subject(&actor.platform, &actor.connector_id, &actor.sender_id)
        .map_err(str::to_owned)?
        != target.owner_user_id
    {
        return Ok(None);
    }
    Ok(Some(actor))
}

/// 与原程序一致，本地凌晨04:00以前仍属于前一个逻辑日。
pub fn daily_logical_date(now: u64) -> Result<String, String> {
    let millis = i64::try_from(now).map_err(|_| "daily clock outside supported range")?;
    let local = chrono::DateTime::from_timestamp_millis(millis)
        .ok_or("daily clock outside supported range")?
        .with_timezone(&chrono::Local);
    let date = if local.hour() < 4 {
        local
            .date_naive()
            .pred_opt()
            .ok_or("daily date underflow")?
    } else {
        local.date_naive()
    };
    Ok(date.format("%Y-%m-%d").to_string())
}

fn unix_millis(now: u64) -> Result<i64, String> {
    i64::try_from(now).map_err(|_| "daily clock outside SQLite range".into())
}

fn stopping(state: &ServiceState, stop: &AtomicBool) -> bool {
    stop.load(Ordering::Acquire)
        || state.submission_intake().is_closed()
        || *state.ws_shutdown().subscribe().borrow()
}

async fn target_guard(
    state: &ServiceState,
    target: &DailyBridgeTarget,
    allow_busy: bool,
) -> Result<Option<TargetGuard>, String> {
    if !state.slash.daily_target_is_current(target)? {
        return Ok(None);
    }
    if state
        .memory()
        .is_agent_deleted_checked(&target.agent_id)
        .map_err(|e| e.to_string())?
    {
        return Ok(None);
    }
    let Some(row) = session_tree::get_session_row(state.storage(), &target.session_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if row.lifecycle != "active"
        || row.owner_user_id != target.owner_user_id
        || row.agent_id != target.agent_id
    {
        return Ok(None);
    }
    let bridge_actor = if row.owner_user_id == crate::auth::LOCAL_OWNER_USER_ID {
        None
    } else if lingxi_protocol::valid_bridge_subject(&row.owner_user_id) {
        let Some(actor) = actual_bridge_actor(state, target).await? else {
            return Ok(None);
        };
        Some(actor)
    } else {
        return Ok(None);
    };
    if !allow_busy
        && state
            .sessions()
            .session_supervisor()
            .is_busy(&target.session_id)
    {
        return Ok(None);
    }
    // 保留原读配置的类型检查；缺失或损坏助手不能当成默认助手继续执行。
    let dir = state
        .memory()
        .agent_dir_checked(&target.agent_id)
        .map_err(|e| e.to_string())?;
    let document =
        lingxi_adapters::storage::agent_memory::config::read_agent_config(&dir.join("config.yaml"))
            .map_err(|e| e.to_string())?;
    document.scan().map_err(|e| e.to_string())?;
    let rendered = document.render().map_err(|e| e.to_string())?;
    let preferences = state
        .memory()
        .read_preferences_root()
        .map_err(|e| e.to_string())?;
    let bridge = serde_json::to_vec(&preferences["bridge"]).map_err(|e| e.to_string())?;
    Ok(Some(TargetGuard {
        permission: row.permission_mode,
        folders: row.authorized_folders,
        config_hash: lingxi_kernel::session_tree::sha256_hex(rendered.as_bytes()),
        bridge_preferences_hash: lingxi_kernel::session_tree::sha256_hex(&bridge),
        bridge_actor,
    }))
}

async fn space_between_targets(state: &ServiceState, stop: &AtomicBool) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(DAILY_JOB_SPACING_MS);
    loop {
        if stopping(state, stop) {
            return false;
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return true;
        }
        tokio::time::sleep((deadline - now).min(Duration::from_millis(REVOCATION_CHECK_MS))).await;
    }
}

async fn compact_target(
    state: &ServiceState,
    target: &DailyBridgeTarget,
    guard: &TargetGuard,
    stop: &AtomicBool,
) -> Result<(u16, Value, Option<String>), String> {
    if stopping(state, stop) || target_guard(state, target, false).await?.as_ref() != Some(guard) {
        return Ok((
            409,
            json!({"ok":false,"error":"daily_target_changed_before_execution"}),
            None,
        ));
    }
    let principal = match &guard.bridge_actor {
        Some(actor) => crate::auth::Principal::bridge_actor(
            &actor.platform,
            &actor.connector_id,
            &actor.sender_id,
        )?,
        None => state.auth().scheduler_principal(),
    };
    let response = crate::history::history_content::manual_compact(
        state,
        &principal,
        &target.session_id,
        b"{}",
        true,
    );
    tokio::pin!(response);
    let mut interval = tokio::time::interval(Duration::from_millis(REVOCATION_CHECK_MS));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut cancellation = None;
    let response = loop {
        tokio::select! {
            biased;
            response = &mut response => break response,
            _ = interval.tick() => {
                let reason = if stopping(state,stop) {
                    Some("daily host stopped".to_string())
                } else if cancellation.is_none() {
                    match target_guard(state,target,true).await {
                        Ok(Some(current)) if &current==guard => None,
                        Ok(_) => Some("daily target configuration, permission or lifecycle changed".to_string()),
                        Err(error) => Some(format!("daily target recheck failed: {error}")),
                    }
                } else {None};
                if cancellation.is_none() {
                    if let Some(reason)=reason {
                        cancellation=Some(reason);
                    }
                }
                if cancellation.is_some() {
                    // 取消请求不等于实际退出；始终保留并等原压缩future的真实结算。
                    if let Some(compaction)=state.compaction() {
                        if let Err(error)=compaction.cancel_manual(&target.session_id) {
                            tracing::error!(%error,session_id=%target.session_id,"daily cancellation request failed; waiting for original settlement");
                        }
                    }
                }
            }
        }
    };
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let body = serde_json::from_slice(&bytes).map_err(|e| format!("daily response JSON: {e}"))?;
    Ok((status, body, cancellation))
}

pub(crate) async fn check_daily(state: &ServiceState, now: u64) -> Result<Value, String> {
    check_daily_cancellable(state, now, &AtomicBool::new(false)).await
}

/// 独立daily lane使用同一个停止标志；停止后不再领取下一项。
pub(crate) async fn check_daily_cancellable(
    state: &ServiceState,
    now: u64,
    stop: &AtomicBool,
) -> Result<Value, String> {
    let check_started = tokio::time::Instant::now();
    let logical_date = daily_logical_date(now)?;
    let mut compacted = 0_u64;
    let mut failed = 0_u64;
    let mut stale_remaining = 0_u64;
    let mut already_satisfied = 0_u64;
    let mut unknown = 0_u64;
    let mut skipped = 0_u64;
    let mut outcomes = Vec::new();
    let mut ran_target = false;
    let targets = state.slash.daily_owner_targets()?;
    let target_count = targets.len();
    for target in targets {
        if stopping(state, stop) {
            break;
        }
        let guard = match target_guard(state, &target, false).await {
            Ok(Some(guard)) => guard,
            Ok(None) => {
                skipped += 1;
                if state.slash.daily_target_is_current(&target)?
                    && state
                        .sessions()
                        .session_supervisor()
                        .is_busy(&target.session_id)
                {
                    // 正在对话只是本轮让路；仍有整理工作时一小时后再检查。
                    stale_remaining += 1;
                }
                continue;
            }
            Err(error) => {
                failed += 1;
                stale_remaining += 1;
                outcomes.push(json!({"sessionId":target.session_id,"agentId":target.agent_id,"error":error,"started":false}));
                continue;
            }
        };
        let existing = store::get_daily(state.storage(), &target.session_id, &logical_date)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(receipt) = &existing {
            match receipt.status {
                store::DailyStatus::Succeeded => {
                    already_satisfied += 1;
                    continue;
                }
                store::DailyStatus::Started | store::DailyStatus::Unknown => {
                    unknown += 1;
                    stale_remaining += 1;
                    continue;
                }
                store::DailyStatus::Failed => {}
            }
        }
        if ran_target && !space_between_targets(state, stop).await {
            break;
        }
        // 五秒间隔中可能撤回权限；领取前重新读取本轮实际目标。
        if stopping(state, stop) {
            break;
        }
        let unchanged = match target_guard(state, &target, false).await {
            Ok(current) => current.as_ref() == Some(&guard),
            Err(error) => {
                failed += 1;
                stale_remaining += 1;
                outcomes.push(json!({"sessionId":target.session_id,"agentId":target.agent_id,"error":error,"started":false}));
                continue;
            }
        };
        if !unchanged {
            skipped += 1;
            if state.slash.daily_target_is_current(&target)?
                && state
                    .sessions()
                    .session_supervisor()
                    .is_busy(&target.session_id)
            {
                stale_remaining += 1;
            }
            continue;
        }
        let claim = store::claim_daily(
            state.storage(),
            &target.session_id,
            &logical_date,
            unix_millis(now.saturating_add(check_started.elapsed().as_millis() as u64))?,
        )
        .await
        .map_err(|e| e.to_string())?;
        if !claim.claimed {
            match claim.receipt.status {
                store::DailyStatus::Succeeded => already_satisfied += 1,
                store::DailyStatus::Failed => stale_remaining += 1,
                store::DailyStatus::Started | store::DailyStatus::Unknown => {
                    unknown += 1;
                    stale_remaining += 1;
                }
            }
            continue;
        }
        ran_target = true;
        let result = compact_target(state, &target, &guard, stop).await;
        let (status, receipt_result, error, event) = match result {
            Ok((http, body, _cancellation)) if (200..300).contains(&http) && body["ok"] == true => {
                compacted += 1;
                (store::DailyStatus::Succeeded, Some(body), None, "succeeded")
            }
            Ok((http, body, cancellation)) => {
                failed += 1;
                stale_remaining += 1;
                let was_cancelled = cancellation.is_some()
                    || body["error"] == "compaction_cancelled"
                    || body["reason"] == "compaction_cancelled";
                let error =
                    cancellation.unwrap_or_else(|| format!("fresh-compact HTTP {http}: {body}"));
                if was_cancelled {
                    unknown += 1;
                    (
                        store::DailyStatus::Unknown,
                        Some(body),
                        Some(error),
                        "unknown",
                    )
                } else {
                    (
                        store::DailyStatus::Failed,
                        Some(body),
                        Some(error),
                        "failed",
                    )
                }
            }
            Err(error) => {
                failed += 1;
                stale_remaining += 1;
                unknown += 1;
                (store::DailyStatus::Unknown, None, Some(error), "unknown")
            }
        };
        let receipt = store::settle_daily(
            state.storage(),
            &target.session_id,
            &logical_date,
            claim.receipt.attempt,
            status,
            receipt_result,
            error,
            unix_millis(now.saturating_add(check_started.elapsed().as_millis() as u64))?,
        )
        .await
        .map_err(|e| e.to_string())?;
        state.app_events().publish(
            "automation_daily_update",
            json!({"sessionId":target.session_id,"agentId":target.agent_id,"logicalDate":logical_date,"status":event}),
        );
        outcomes.push(
            json!({"sessionId":target.session_id,"agentId":target.agent_id,"receipt":receipt}),
        );
    }
    Ok(json!({
        "logicalDate":logical_date,
        "bridgeCompacted":compacted,
        "phoneCompacted":0,
        "failed":failed,
        "staleRemaining":stale_remaining,
        "alreadySatisfied":already_satisfied,
        "unknown":unknown,
        "skipped":skipped,
        "targetCount":target_count,
        "cancelled":stopping(state,stop),
        "results":outcomes,
    }))
}

/// 可信宿主探针直接走同一真实检查；不提供HTTP入口，也不接受目标列表或伪身份。
#[doc(hidden)]
pub async fn check_daily_once(state: &ServiceState, now: u64) -> Result<Value, String> {
    check_daily(state, now).await
}
