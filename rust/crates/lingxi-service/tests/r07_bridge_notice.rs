//! 真实持久回执之后才写宿主通知事实；不创建用户消息或虚构模型运行。
use lingxi_adapters::storage::{
    bridge as store, bridge_notice, session_tree, RunDatabase, SessionRow, StoreOptions,
};
use lingxi_kernel::{
    model_exchange::ExchangeItem,
    ports::{RunOutcome, StoragePort},
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, RunId, RunStatus, SessionId};
use lingxi_service::model_history::ModelHistory;
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
async fn fixture() -> (Arc<RunDatabase>, PathBuf, store::BridgeInbox) {
    let home = std::env::temp_dir().join(format!(
        "r07-bridge-notice-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let path = home.join("runs.db");
    let db = Arc::new(
        RunDatabase::open(&path, StoreOptions::default())
            .await
            .unwrap(),
    );
    store::put_connector(
        &db,
        &store::BridgeConnectorInput {
            id: "connector".into(),
            platform: "telegram".into(),
            studio_id: "studio".into(),
            actor_id: "agent".into(),
            owner_subject: "user_local".into(),
            owner_sender_id: Some("sender".into()),
            enabled: true,
            config_json: "{}".into(),
        },
        None,
        1,
    )
    .await
    .unwrap();
    let inbox = store::claim_inbox(
        &db,
        &store::BridgeInboxInput {
            platform: "telegram".into(),
            connector_id: "connector".into(),
            conversation_id: "verified-dm".into(),
            conversation_kind: store::ConversationKind::Dm,
            message_id: "owner-message".into(),
            sender_id: "sender".into(),
            payload_json: json!({"text":"提醒我","replyContext":{"isGroup":false}}).to_string(),
            attachments: vec![],
        },
        1,
        2,
    )
    .await
    .unwrap()
    .inbox;
    let sid = inbox.session_id.as_deref().unwrap();
    let subject = lingxi_protocol::bridge_subject("telegram", "connector", "sender").unwrap();
    db.ensure_session_seed(vec![SessionRow {
        session_id: sid.into(),
        agent_id: "agent".into(),
        owner_user_id: subject.clone(),
        title: "Bridge".into(),
        created_at_unix_ms: 2,
    }])
    .await
    .unwrap();
    store::prepare_inbox_session(&db, &inbox.id, 1, sid, &[], 3)
        .await
        .unwrap();
    let ctx = RunContext {
        principal: Principal::Bridge { subject },
        session_id: SessionId::new(sid),
        run_id: RunId::new("actual-bridge-message-run"),
        attempt: AttemptId::new("actual-bridge-message-run#a1"),
        generation: 1,
    };
    db.record_run_started(&ctx, 4).await.unwrap();
    db.record_run_lineage(
        &ctx,
        RunLineage {
            origin: RunOrigin::Bridge,
            parent_run_id: None,
            cause_id: Some(inbox.id.clone()),
            source_message_id: Some(inbox.input.message_id.clone()),
        },
        4,
    )
    .await
    .unwrap();
    store::bind_inbox(&db, &inbox.id, 1, sid, ctx.run_id.as_str(), 5)
        .await
        .unwrap();
    db.commit_run_outcome(
        &ctx,
        RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            key_events: vec![],
            final_message: None,
        },
        6,
    )
    .await
    .unwrap();
    // 主动通知由真实本机任务产生；owner桥接收据仅确定其实际私聊接收目标。
    db.ensure_session_seed(vec![SessionRow {
        session_id: "owner-notify-session".into(),
        agent_id: "agent".into(),
        owner_user_id: "user_local".into(),
        title: "本机通知来源".into(),
        created_at_unix_ms: 2,
    }])
    .await
    .unwrap();
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("owner-notify-session"),
        run_id: RunId::new("actual-notify-run"),
        attempt: AttemptId::new("actual-notify-run#a1"),
        generation: 1,
    };
    db.record_run_started(&ctx, 6).await.unwrap();
    db.record_run_lineage(
        &ctx,
        RunLineage {
            origin: RunOrigin::User,
            parent_run_id: None,
            source_message_id: None,
            cause_id: Some("request:actual-notify-fixture".into()),
        },
        6,
    )
    .await
    .unwrap();
    db.commit_run_outcome(
        &ctx,
        RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            key_events: vec![],
            final_message: None,
        },
        6,
    )
    .await
    .unwrap();
    (db, path, inbox)
}
async fn delivery(
    db: &RunDatabase,
    _inbox: &store::BridgeInbox,
    id: &str,
    status: store::BridgeOutboxStatus,
    policy: &str,
) {
    store::enqueue_outbox(
        db,
        &store::BridgeOutboxInput {
            delivery_id: id.into(),
            connector_id: "connector".into(),
            connector_generation: 1,
            inbox_id: None,
            manual_action_id: None,
            run_id: Some("actual-notify-run".into()),
            payload_json: json!({"text":"已到提醒时间","contextPolicy":policy}).to_string(),
            attachments: vec![],
        },
        7,
    )
    .await
    .unwrap();
    assert!(store::claim_outbox(db, id, 1, 8).await.unwrap().claimed);
    let receipt = if status == store::BridgeOutboxStatus::Confirmed {
        Some(store::BridgePlatformReceipt {
            platform: "telegram".into(),
            message_ids: vec![format!("platform-{id}")],
            raw_json: None,
        })
    } else {
        None
    };
    store::settle_outbox(db, id, status, receipt, None, 9)
        .await
        .unwrap();
}
async fn cleanup(db: Arc<RunDatabase>, path: PathBuf) {
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[tokio::test]
async fn only_confirmed_delivery_becomes_a_distinct_notice_and_survives_restart_once() {
    let (db, path, inbox) = fixture().await;
    for (id, status) in [
        ("unknown", store::BridgeOutboxStatus::Unknown),
        ("failed", store::BridgeOutboxStatus::Failed),
    ] {
        delivery(&db, &inbox, id, status, "record_when_delivered").await;
        assert!(
            lingxi_service::model_history::record_delivered_bridge_notice(&db, id, 10)
                .await
                .is_err()
        );
    }
    delivery(
        &db,
        &inbox,
        "none",
        store::BridgeOutboxStatus::Confirmed,
        "none",
    )
    .await;
    assert!(
        lingxi_service::model_history::record_delivered_bridge_notice(&db, "none", 10)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        session_tree::list_branch_messages(&db, inbox.session_id.as_deref().unwrap())
            .await
            .unwrap()
            .is_empty()
    );
    delivery(
        &db,
        &inbox,
        "confirmed",
        store::BridgeOutboxStatus::Confirmed,
        "record_when_delivered",
    )
    .await;
    let (a, b) = tokio::join!(
        lingxi_service::model_history::record_delivered_bridge_notice(&db, "confirmed", 10),
        lingxi_service::model_history::record_delivered_bridge_notice(&db, "confirmed", 10)
    );
    assert_eq!(a.unwrap(), b.unwrap());
    let rows = session_tree::list_branch_messages(&db, inbox.session_id.as_deref().unwrap())
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].role, "host_context");
    assert_eq!(rows[0].entry_type, bridge_notice::ENTRY_TYPE);
    assert!(rows[0].run_id.is_none());
    assert!(rows[0].model_call_id.is_none());
    let actor = Principal::Bridge {
        subject: lingxi_protocol::bridge_subject("telegram", "connector", "sender").unwrap(),
    };
    let history = ModelHistory::new(db.clone());
    assert_eq!(
        history
            .load(
                &actor,
                "agent",
                inbox.session_id.as_deref().unwrap(),
                "next-run"
            )
            .await
            .unwrap(),
        vec![ExchangeItem::HostNotice {
            delivery_id: "confirmed".into(),
            text: "已到提醒时间".into()
        }]
    );
    drop(history);
    db.close().await.unwrap();
    drop(db);
    let db = Arc::new(
        RunDatabase::open(&path, StoreOptions::default())
            .await
            .unwrap(),
    );
    lingxi_service::model_history::record_delivered_bridge_notice(&db, "confirmed", 11)
        .await
        .unwrap();
    assert_eq!(
        session_tree::list_branch_messages(&db, inbox.session_id.as_deref().unwrap())
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        db.query_one_text("SELECT COUNT(*) FROM runs", vec![])
            .await
            .unwrap()
            .as_deref(),
        Some("2")
    );
    cleanup(db, path).await;
}
#[tokio::test]
async fn notice_is_retained_at_the_fork_boundary_and_receipt_does_not_replay_after_clear() {
    let (db, path, inbox) = fixture().await;
    delivery(
        &db,
        &inbox,
        "fork",
        store::BridgeOutboxStatus::Confirmed,
        "record_when_delivered",
    )
    .await;
    lingxi_service::model_history::record_delivered_bridge_notice(&db, "fork", 10)
        .await
        .unwrap();
    let sid = inbox.session_id.as_deref().unwrap();
    let rows = session_tree::list_branch_messages(&db, sid).await.unwrap();
    session_tree::fork_session(
        &db,
        session_tree::ForkRequest {
            source_session_id: sid,
            new_session_id: "notice-fork",
            boundary_message_id: &rows[0].message_id,
            now_unix_ms: 11,
        },
    )
    .await
    .unwrap();
    let actor = Principal::Bridge {
        subject: lingxi_protocol::bridge_subject("telegram", "connector", "sender").unwrap(),
    };
    let history = ModelHistory::new(db.clone());
    assert!(
        matches!(history.load(&actor,"agent","notice-fork","next").await.unwrap().as_slice(),[ExchangeItem::HostNotice{delivery_id,..}] if delivery_id=="fork")
    );
    lingxi_adapters::storage::session_admin::clear_current_history(
        &db,
        sid,
        &actor.storage_subject(),
        12,
    )
    .await
    .unwrap();
    lingxi_service::model_history::record_delivered_bridge_notice(&db, "fork", 13)
        .await
        .unwrap();
    assert!(!session_tree::list_branch_messages(&db, sid)
        .await
        .unwrap()
        .iter()
        .any(|r| r.entry_type == bridge_notice::ENTRY_TYPE));
    drop(history);
    cleanup(db, path).await;
}
#[tokio::test]
async fn forged_or_changed_notice_material_is_refused_instead_of_becoming_user_text() {
    let (db, path, inbox) = fixture().await;
    delivery(
        &db,
        &inbox,
        "tamper",
        store::BridgeOutboxStatus::Confirmed,
        "record_when_delivered",
    )
    .await;
    lingxi_service::model_history::record_delivered_bridge_notice(&db, "tamper", 10)
        .await
        .unwrap();
    let external = rusqlite::Connection::open(&path).unwrap();
    external.execute("UPDATE bridge_outbox_v1 SET payload_json=json_set(payload_json,'$.text','被改写的消息') WHERE delivery_id='tamper'",[]).unwrap();
    drop(external);
    let actor = Principal::Bridge {
        subject: lingxi_protocol::bridge_subject("telegram", "connector", "sender").unwrap(),
    };
    let history = ModelHistory::new(db.clone());
    assert!(history
        .load(
            &actor,
            "agent",
            inbox.session_id.as_deref().unwrap(),
            "next"
        )
        .await
        .is_err());
    drop(history);
    cleanup(db, path).await;
}
