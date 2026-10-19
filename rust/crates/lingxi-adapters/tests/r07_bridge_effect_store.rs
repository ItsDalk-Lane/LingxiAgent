//! 等待提示、草稿序列、未知窗口和最终平台回执均由真实数据库验证。
use lingxi_adapters::storage::{
    bridge::*, bridge_effect::*, RunDatabase, SessionRow, StoreOptions,
};
use lingxi_kernel::{
    ports::{RunOutcome, StoragePort},
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, BridgeEffectStatus, RunId, RunStatus, SessionId};
use serde_json::json;
use std::path::PathBuf;

async fn fixture() -> (RunDatabase, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-bridge-effect-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("runs.db");
    (
        RunDatabase::open(&path, StoreOptions::default())
            .await
            .unwrap(),
        path,
    )
}
async fn cleanup(db: RunDatabase, path: PathBuf) {
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
fn cfg() -> BridgeConnectorInput {
    BridgeConnectorInput {
        id: "effect-connector".into(),
        platform: "telegram".into(),
        studio_id: "studio".into(),
        actor_id: "agent".into(),
        owner_subject: "user_local".into(),
        owner_sender_id: Some("owner".into()),
        enabled: true,
        config_json: "{}".into(),
    }
}
async fn source(db: &RunDatabase) -> BridgeInbox {
    source_for(db, "effect-connector", "actual-run").await
}
async fn source_for(db: &RunDatabase, connector: &str, run_id: &str) -> BridgeInbox {
    let mut config = cfg();
    config.id = connector.into();
    put_connector(db, &config, None, 1).await.unwrap();
    let inbox=claim_inbox(db,&BridgeInboxInput{platform:"telegram".into(),connector_id:connector.into(),conversation_id:"actual-dm".into(),conversation_kind:ConversationKind::Dm,message_id:"actual-message".into(),sender_id:"owner".into(),payload_json:json!({"text":"actual source","replyContext":{"messageId":"actual-message","contextToken":"private-synthetic-context"}}).to_string(),attachments:vec![]},1,2).await.unwrap().inbox;
    let subject = lingxi_protocol::bridge_subject("telegram", connector, "owner").unwrap();
    let sid = inbox.session_id.as_deref().unwrap();
    db.ensure_session_seed(vec![SessionRow {
        session_id: sid.into(),
        agent_id: "agent".into(),
        owner_user_id: subject.clone(),
        title: "Bridge".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .unwrap();
    prepare_inbox_session(db, &inbox.id, 1, sid, &[], 3)
        .await
        .unwrap();
    let ctx = RunContext {
        principal: Principal::Bridge { subject },
        session_id: SessionId::new(sid),
        run_id: RunId::new(run_id),
        attempt: AttemptId::new(format!("{run_id}#a1")),
        generation: 1,
    };
    db.record_run_started(&ctx, 4).await.unwrap();
    db.record_run_lineage(
        &ctx,
        RunLineage {
            parent_run_id: None,
            origin: RunOrigin::Bridge,
            source_message_id: Some(inbox.input.message_id.clone()),
            cause_id: Some(inbox.id.clone()),
        },
        4,
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
        5,
    )
    .await
    .unwrap();
    bind_inbox(db, &inbox.id, 1, sid, run_id, 6).await.unwrap()
}
fn input(
    stream: &BridgeEffectStream,
    key: &str,
    operation: BridgeEffectOperation,
    text: &str,
    delivery: Option<&str>,
) -> BridgeEffectInput {
    BridgeEffectInput {
        stream_id: stream.stream_id.clone(),
        request_key: key.into(),
        operation,
        text: text.into(),
        delivery_id: delivery.map(str::to_string),
    }
}
fn receipt(effect: &BridgeEffect, status: BridgeEffectStatus) -> BridgeEffectReceipt {
    let request = &effect.request;
    BridgeEffectReceipt {
        effect_id: request.effect_id.clone(),
        stream_id: request.stream_id.clone(),
        sequence: request.sequence,
        operation: request.operation,
        delivery_id: request.delivery_id.clone(),
        status,
        platform_ack: status == BridgeEffectStatus::Confirmed,
        platform_message_ids: if request.operation == BridgeEffectOperation::StreamFinish
            && status == BridgeEffectStatus::Confirmed
        {
            vec!["actual-platform-final".into()]
        } else {
            vec![]
        },
        error: if status == BridgeEffectStatus::Skipped {
            Some("platform did not perform effect".into())
        } else {
            None
        },
    }
}
async fn final_outbox(db: &RunDatabase, inbox: &BridgeInbox) -> BridgeOutbox {
    enqueue_outbox(
        db,
        &BridgeOutboxInput {
            delivery_id: "actual-final-delivery".into(),
            connector_id: inbox.input.connector_id.clone(),
            connector_generation: 1,
            inbox_id: Some(inbox.id.clone()),
            run_id: Some("actual-run".into()),
            manual_action_id: None,
            payload_json: json!({"text":"真实最终答复"}).to_string(),
            attachments: vec![],
        },
        30,
    )
    .await
    .unwrap();
    claim_outbox(db, "actual-final-delivery", 1, 31)
        .await
        .unwrap()
        .outbox
}

#[tokio::test]
async fn receipt_refresh_keys_and_sequence_survive_restart_and_cleanup_remains_scoped_after_revoke()
{
    let (db, path) = fixture().await;
    let inbox = source(&db).await;
    assert!(ensure_stream(
        &db,
        &inbox.id,
        "missing-run",
        BridgeEffectMode::ReceiptOnly,
        false,
        7
    )
    .await
    .is_err());
    let stream = ensure_stream(
        &db,
        &inbox.id,
        "actual-run",
        BridgeEffectMode::ReceiptOnly,
        false,
        8,
    )
    .await
    .unwrap();
    assert_eq!(
        ensure_stream(
            &db,
            &inbox.id,
            "actual-run",
            BridgeEffectMode::NativeStream,
            true,
            9
        )
        .await
        .unwrap(),
        stream
    );
    assert!(claim_effect(
        &db,
        &input(
            &stream,
            "wrong-mode",
            BridgeEffectOperation::StreamUpdate,
            "draft",
            None
        ),
        10
    )
    .await
    .is_err());
    let first_input = input(
        &stream,
        "receipt:1",
        BridgeEffectOperation::ReceiptStart,
        "正在处理",
        None,
    );
    let first = claim_effect(&db, &first_input, 11).await.unwrap();
    assert!(first.claimed);
    assert_eq!(first.effect.request.sequence, 1);
    assert!(!claim_effect(&db, &first_input, 12).await.unwrap().claimed);
    let mut conflict = first_input.clone();
    conflict.text = "different".into();
    assert!(claim_effect(&db, &conflict, 12).await.is_err());
    let mut unproven = receipt(&first.effect, BridgeEffectStatus::Confirmed);
    unproven.platform_ack = false;
    assert!(settle_effect(&db, &unproven, 13).await.is_err());
    settle_effect(
        &db,
        &receipt(&first.effect, BridgeEffectStatus::Confirmed),
        13,
    )
    .await
    .unwrap();
    let refresh = claim_effect(
        &db,
        &input(
            &stream,
            "receipt:2",
            BridgeEffectOperation::ReceiptStart,
            "正在处理",
            None,
        ),
        14,
    )
    .await
    .unwrap();
    assert!(refresh.claimed);
    assert_eq!(refresh.effect.request.sequence, 2);
    settle_effect(
        &db,
        &receipt(&refresh.effect, BridgeEffectStatus::Confirmed),
        15,
    )
    .await
    .unwrap();
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert!(!claim_effect(&db, &first_input, 16).await.unwrap().claimed);
    let mut changed = cfg();
    changed.enabled = false;
    put_connector(&db, &changed, Some(1), 17).await.unwrap();
    assert!(claim_effect(
        &db,
        &input(
            &stream,
            "receipt:3",
            BridgeEffectOperation::ReceiptStart,
            "正在处理",
            None
        ),
        18
    )
    .await
    .is_err());
    let stop = claim_effect(
        &db,
        &input(
            &stream,
            "stop",
            BridgeEffectOperation::ReceiptStop,
            "",
            None,
        ),
        19,
    )
    .await
    .unwrap();
    assert!(stop.claimed);
    assert_eq!(stop.effect.request.conversation_id, "actual-dm");
    assert_eq!(
        stop.effect.request.reply_context.context_token.as_deref(),
        Some("private-synthetic-context")
    );
    settle_effect(
        &db,
        &receipt(&stop.effect, BridgeEffectStatus::Confirmed),
        20,
    )
    .await
    .unwrap();
    assert!(
        get_stream(&db, &stream.stream_id)
            .await
            .unwrap()
            .unwrap()
            .closed
    );
    assert_eq!(db.total_runs().await.unwrap(), 1);
    cleanup(db, path).await;
}

#[tokio::test]
async fn native_finish_and_actual_outbox_receipt_commit_atomically_or_both_roll_back() {
    let (db, path) = fixture().await;
    let inbox = source(&db).await;
    let stream = ensure_stream(
        &db,
        &inbox.id,
        "actual-run",
        BridgeEffectMode::NativeStream,
        true,
        7,
    )
    .await
    .unwrap();
    let update = claim_effect(
        &db,
        &input(
            &stream,
            "update:10",
            BridgeEffectOperation::StreamUpdate,
            "草稿",
            None,
        ),
        8,
    )
    .await
    .unwrap()
    .effect;
    settle_effect(&db, &receipt(&update, BridgeEffectStatus::Confirmed), 9)
        .await
        .unwrap();
    let outbox = final_outbox(&db, &inbox).await;
    assert!(claim_effect(
        &db,
        &input(
            &stream,
            "bad-final",
            BridgeEffectOperation::StreamFinish,
            "伪造内容",
            Some(&outbox.input.delivery_id)
        ),
        32
    )
    .await
    .is_err());
    let finish = claim_effect(
        &db,
        &input(
            &stream,
            "finish:actual-final-delivery",
            BridgeEffectOperation::StreamFinish,
            "真实最终答复",
            Some(&outbox.input.delivery_id),
        ),
        33,
    )
    .await
    .unwrap()
    .effect;
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_finish_receipt BEFORE UPDATE ON bridge_effects_v1 WHEN NEW.operation='stream_finish' AND NEW.status='confirmed' BEGIN SELECT RAISE(ABORT,'injected receipt failure'); END;").unwrap();
    let real = receipt(&finish, BridgeEffectStatus::Confirmed);
    assert!(settle_effect(&db, &real, 34).await.is_err());
    assert_eq!(
        get_outbox(&db, &outbox.input.delivery_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        BridgeOutboxStatus::Started
    );
    assert_eq!(
        get_effect(&db, &finish.request.effect_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        StoredEffectStatus::Started
    );
    conn.execute_batch("DROP TRIGGER reject_finish_receipt;")
        .unwrap();
    drop(conn);
    let settled = settle_effect(&db, &real, 35).await.unwrap();
    assert_eq!(settled.status, StoredEffectStatus::Confirmed);
    assert_eq!(settle_effect(&db, &real, 36).await.unwrap(), settled);
    let delivered = get_outbox(&db, &outbox.input.delivery_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivered.status, BridgeOutboxStatus::Confirmed);
    assert_eq!(
        delivered.platform_receipt.unwrap().message_ids,
        vec!["actual-platform-final"]
    );
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(
        get_effect(&db, &finish.request.effect_id)
            .await
            .unwrap()
            .unwrap(),
        settled
    );
    assert!(
        get_stream(&db, &stream.stream_id)
            .await
            .unwrap()
            .unwrap()
            .closed
    );
    cleanup(db, path).await;
}

#[tokio::test]
async fn native_update_unknown_blocks_new_content_and_plain_mode_downgrade_but_allows_frozen_cleanup(
) {
    let (db, path) = fixture().await;
    let inbox = source(&db).await;
    let stream = ensure_stream(
        &db,
        &inbox.id,
        "actual-run",
        BridgeEffectMode::NativeStream,
        false,
        7,
    )
    .await
    .unwrap();
    let update_input = input(
        &stream,
        "update:1",
        BridgeEffectOperation::StreamUpdate,
        "可能已外发",
        None,
    );
    let update = claim_effect(&db, &update_input, 8).await.unwrap().effect;
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(recover_started_effects(&db, 9).await.unwrap(), 1);
    assert_eq!(recover_started_effects(&db, 10).await.unwrap(), 0);
    assert_eq!(
        get_effect(&db, &update.request.effect_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        StoredEffectStatus::Unknown
    );
    assert!(!claim_effect(&db, &update_input, 10).await.unwrap().claimed);
    let recovered = ensure_stream(
        &db,
        &inbox.id,
        "actual-run",
        BridgeEffectMode::ReceiptOnly,
        false,
        11,
    )
    .await
    .unwrap();
    assert_eq!(recovered.mode, BridgeEffectMode::NativeStream);
    assert!(recovered.unknown);
    assert!(claim_effect(
        &db,
        &input(
            &stream,
            "update:2",
            BridgeEffectOperation::StreamUpdate,
            "不可盲重发",
            None
        ),
        12
    )
    .await
    .is_err());
    let outbox = final_outbox(&db, &inbox).await;
    assert!(claim_effect(
        &db,
        &input(
            &stream,
            "finish",
            BridgeEffectOperation::StreamFinish,
            "真实最终答复",
            Some(&outbox.input.delivery_id)
        ),
        32
    )
    .await
    .is_err());
    let mut changed = cfg();
    changed.owner_sender_id = None;
    put_connector(&db, &changed, Some(1), 33).await.unwrap();
    let cancel = claim_effect(
        &db,
        &input(
            &stream,
            "cancel",
            BridgeEffectOperation::StreamCancel,
            "",
            None,
        ),
        34,
    )
    .await
    .unwrap()
    .effect;
    assert_eq!(cancel.request.conversation_id, "actual-dm");
    let skipped = settle_effect(&db, &receipt(&cancel, BridgeEffectStatus::Skipped), 35)
        .await
        .unwrap();
    assert_eq!(skipped.status, StoredEffectStatus::Skipped);
    assert!(
        get_stream(&db, &stream.stream_id)
            .await
            .unwrap()
            .unwrap()
            .unknown
    );
    cleanup(db, path).await;
}

#[tokio::test]
async fn concurrent_claims_have_one_external_lease_and_revisions_can_return_to_prior_text() {
    let (db, path) = fixture().await;
    let inbox = source(&db).await;
    let stream = ensure_stream(
        &db,
        &inbox.id,
        "actual-run",
        BridgeEffectMode::NativeStream,
        false,
        7,
    )
    .await
    .unwrap();
    let first = input(
        &stream,
        "event:1",
        BridgeEffectOperation::StreamUpdate,
        "A",
        None,
    );
    let (a, b) = tokio::join!(claim_effect(&db, &first, 8), claim_effect(&db, &first, 8));
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(usize::from(a.claimed) + usize::from(b.claimed), 1);
    assert_eq!(a.effect, b.effect);
    assert!(claim_effect(
        &db,
        &input(
            &stream,
            "event:2",
            BridgeEffectOperation::StreamUpdate,
            "B",
            None
        ),
        9
    )
    .await
    .is_err());
    settle_effect(&db, &receipt(&a.effect, BridgeEffectStatus::Confirmed), 10)
        .await
        .unwrap();
    let second = claim_effect(
        &db,
        &input(
            &stream,
            "event:2",
            BridgeEffectOperation::StreamUpdate,
            "B",
            None,
        ),
        11,
    )
    .await
    .unwrap()
    .effect;
    settle_effect(&db, &receipt(&second, BridgeEffectStatus::Confirmed), 12)
        .await
        .unwrap();
    let third = claim_effect(
        &db,
        &input(
            &stream,
            "event:3",
            BridgeEffectOperation::StreamUpdate,
            "A",
            None,
        ),
        13,
    )
    .await
    .unwrap();
    assert!(third.claimed);
    assert_eq!(third.effect.request.sequence, 3);
    cleanup(db, path).await;
}

#[tokio::test]
async fn restart_marks_confirmed_but_open_native_stream_unknown_without_recreating_it() {
    let (db, path) = fixture().await;
    let inbox = source(&db).await;
    assert!(find_stream(&db, &inbox.id, "actual-run")
        .await
        .unwrap()
        .is_none());
    let stream = ensure_stream(
        &db,
        &inbox.id,
        "actual-run",
        BridgeEffectMode::NativeStream,
        false,
        7,
    )
    .await
    .unwrap();
    let update = claim_effect(
        &db,
        &input(
            &stream,
            "update:confirmed",
            BridgeEffectOperation::StreamUpdate,
            "实际已确认草稿",
            None,
        ),
        8,
    )
    .await
    .unwrap()
    .effect;
    settle_effect(&db, &receipt(&update, BridgeEffectStatus::Confirmed), 9)
        .await
        .unwrap();
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(recover_started_effects(&db, 10).await.unwrap(), 0);
    let recovered = find_stream(&db, &inbox.id, "actual-run")
        .await
        .unwrap()
        .unwrap();
    assert!(recovered.unknown);
    assert_eq!(recovered.stream_id, stream.stream_id);
    assert!(claim_effect(
        &db,
        &input(
            &stream,
            "update:after-restart",
            BridgeEffectOperation::StreamUpdate,
            "不得重建",
            None
        ),
        11
    )
    .await
    .is_err());
    cleanup(db, path).await;
}

#[tokio::test]
async fn one_connector_exit_does_not_abandon_other_live_workers_and_receipt_unknown_allows_batch_final(
) {
    let (db, path) = fixture().await;
    let first = source(&db).await;
    let second = source_for(&db, "other-connector", "other-run").await;
    let a = ensure_stream(
        &db,
        &first.id,
        "actual-run",
        BridgeEffectMode::ReceiptOnly,
        false,
        7,
    )
    .await
    .unwrap();
    let b = ensure_stream(
        &db,
        &second.id,
        "other-run",
        BridgeEffectMode::NativeStream,
        false,
        7,
    )
    .await
    .unwrap();
    let effect_a = claim_effect(
        &db,
        &input(
            &a,
            "receipt:1",
            BridgeEffectOperation::ReceiptStart,
            "正在处理",
            None,
        ),
        8,
    )
    .await
    .unwrap()
    .effect;
    settle_effect(&db, &receipt(&effect_a, BridgeEffectStatus::Confirmed), 9)
        .await
        .unwrap();
    let effect_b = claim_effect(
        &db,
        &input(
            &b,
            "update:1",
            BridgeEffectOperation::StreamUpdate,
            "另一个真实草稿",
            None,
        ),
        8,
    )
    .await
    .unwrap()
    .effect;
    assert_eq!(
        abandon_connector_streams(&db, "effect-connector", 1, 10)
            .await
            .unwrap(),
        0
    );
    assert!(
        get_stream(&db, &a.stream_id)
            .await
            .unwrap()
            .unwrap()
            .unknown
    );
    assert!(
        !get_stream(&db, &b.stream_id)
            .await
            .unwrap()
            .unwrap()
            .unknown
    );
    assert_eq!(
        get_effect(&db, &effect_b.request.effect_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        StoredEffectStatus::Started
    );
    let batch = final_outbox(&db, &first).await;
    assert_eq!(
        validate_started_outbox(&db, &batch.input.delivery_id, 1)
            .await
            .unwrap()
            .status,
        BridgeOutboxStatus::Started
    );
    assert_eq!(
        abandon_connector_streams(&db, "other-connector", 2, 11)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        get_effect(&db, &effect_b.request.effect_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        StoredEffectStatus::Started
    );
    cleanup(db, path).await;
}

#[tokio::test]
async fn independent_typing_unknown_does_not_poison_native_content_but_text_receipts_remain_conservative(
) {
    let (db, path) = fixture().await;
    let inbox = source(&db).await;
    let stream = ensure_stream_with_receipt(
        &db,
        &inbox.id,
        "actual-run",
        BridgeEffectMode::NativeStream,
        false,
        true,
        7,
    )
    .await
    .unwrap();
    assert!(stream.receipt_independent);
    let start_input = input(
        &stream,
        "typing:1",
        BridgeEffectOperation::ReceiptStart,
        "正在处理",
        None,
    );
    let start = claim_effect(&db, &start_input, 8).await.unwrap().effect;
    settle_effect(&db, &receipt(&start, BridgeEffectStatus::Unknown), 9)
        .await
        .unwrap();
    assert!(
        !get_stream(&db, &stream.stream_id)
            .await
            .unwrap()
            .unwrap()
            .unknown
    );
    assert!(!claim_effect(&db, &start_input, 10).await.unwrap().claimed);
    let refresh = claim_effect(
        &db,
        &input(
            &stream,
            "typing:2",
            BridgeEffectOperation::ReceiptStart,
            "正在处理",
            None,
        ),
        11,
    )
    .await
    .unwrap()
    .effect;
    settle_effect(&db, &receipt(&refresh, BridgeEffectStatus::Confirmed), 12)
        .await
        .unwrap();
    let update = claim_effect(
        &db,
        &input(
            &stream,
            "update:3",
            BridgeEffectOperation::StreamUpdate,
            "真实草稿",
            None,
        ),
        13,
    )
    .await
    .unwrap()
    .effect;
    settle_effect(&db, &receipt(&update, BridgeEffectStatus::Confirmed), 14)
        .await
        .unwrap();
    let stop = claim_effect(
        &db,
        &input(
            &stream,
            "typing:stop",
            BridgeEffectOperation::ReceiptStop,
            "",
            None,
        ),
        15,
    )
    .await
    .unwrap()
    .effect;
    settle_effect(&db, &receipt(&stop, BridgeEffectStatus::Unknown), 16)
        .await
        .unwrap();
    assert!(
        !get_stream(&db, &stream.stream_id)
            .await
            .unwrap()
            .unwrap()
            .unknown
    );
    let outbox = final_outbox(&db, &inbox).await;
    let finish = claim_effect(
        &db,
        &input(
            &stream,
            "finish",
            BridgeEffectOperation::StreamFinish,
            "真实最终答复",
            Some(&outbox.input.delivery_id),
        ),
        32,
    )
    .await
    .unwrap()
    .effect;
    settle_effect(&db, &receipt(&finish, BridgeEffectStatus::Confirmed), 33)
        .await
        .unwrap();
    assert_eq!(
        get_outbox(&db, &outbox.input.delivery_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        BridgeOutboxStatus::Confirmed
    );
    cleanup(db, path).await;

    let (db, path) = fixture().await;
    let inbox = source(&db).await;
    let stream = ensure_stream(
        &db,
        &inbox.id,
        "actual-run",
        BridgeEffectMode::NativeStream,
        false,
        7,
    )
    .await
    .unwrap();
    let start = claim_effect(
        &db,
        &input(
            &stream,
            "text-receipt",
            BridgeEffectOperation::ReceiptStart,
            "等待文字可能已经发出",
            None,
        ),
        8,
    )
    .await
    .unwrap()
    .effect;
    settle_effect(&db, &receipt(&start, BridgeEffectStatus::Unknown), 9)
        .await
        .unwrap();
    let frozen = ensure_stream_with_receipt(
        &db,
        &inbox.id,
        "actual-run",
        BridgeEffectMode::NativeStream,
        false,
        true,
        10,
    )
    .await
    .unwrap();
    assert!(!frozen.receipt_independent);
    assert!(frozen.unknown);
    assert!(claim_effect(
        &db,
        &input(
            &stream,
            "update:2",
            BridgeEffectOperation::StreamUpdate,
            "不可盲目重发",
            None
        ),
        11
    )
    .await
    .is_err());
    cleanup(db, path).await;
}

#[tokio::test]
async fn v17_candidate_upgrade_keeps_all_old_receipts_and_defaults_typing_independence_to_false() {
    use lingxi_adapters::storage::migrations;
    let (db, path) = fixture().await;
    db.close().await.unwrap();
    std::fs::remove_file(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    for migration in migrations::MIGRATIONS
        .iter()
        .filter(|migration| migration.version <= 17)
    {
        conn.execute_batch(migration.sql).unwrap();
    }
    conn.execute_batch("CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY,name TEXT NOT NULL,fingerprint TEXT NOT NULL,applied_at_unix_ms INTEGER NOT NULL,applied_by TEXT NOT NULL);").unwrap();
    for migration in migrations::MIGRATIONS
        .iter()
        .filter(|migration| migration.version <= 17)
    {
        conn.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2,?3,1,'v17-candidate')",
            rusqlite::params![
                migration.version as i64,
                migration.name,
                migrations::fingerprint_sql(migration.sql)
            ],
        )
        .unwrap();
    }
    conn.pragma_update(None, "user_version", 17).unwrap();
    // 允许历史 Run 已清理但效果元数据仍保留，升级不能凭它重建任何 Run。
    conn.execute_batch("INSERT INTO bridge_connectors_v1 VALUES('historical','telegram','studio','agent','user_local','sender',1,1,'{}',1,1);
    INSERT INTO bridge_inbox_v1(id,platform,connector_id,conversation_id,conversation_kind,message_id,sender_id,role,actor_id,owner_subject,connector_generation,payload_json,attachments_json,status,request_id,created_at,updated_at) VALUES('old-inbox','telegram','historical','old-chat','dm','old-message','sender','owner','agent','user_local',1,'{}','[]','succeeded','bridge:old-inbox',1,1);
    INSERT INTO bridge_effect_streams_v1(stream_id,inbox_id,run_id,connector_id,connector_generation,mode,rich,conversation_id,reply_context_json,unknown,closed,last_sequence,created_at,updated_at) VALUES('old-stream','old-inbox','cleared-historical-run','historical',1,'native_stream',0,'old-chat','{}',0,1,7,1,1);").unwrap();
    let before = migrations::verify_receipts(&conn).unwrap();
    drop(conn);
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    let stored = get_stream(&db, "old-stream").await.unwrap().unwrap();
    assert!(!stored.receipt_independent);
    assert_eq!(stored.last_sequence, 7);
    assert!(stored.closed);
    assert_eq!(db.total_runs().await.unwrap(), 0);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let after = migrations::verify_receipts(&conn).unwrap();
    assert_eq!(&after[..17], before.as_slice());
    assert_eq!(after.len(), migrations::MIGRATIONS.len());
    drop(conn);
    cleanup(db, path).await;
}
