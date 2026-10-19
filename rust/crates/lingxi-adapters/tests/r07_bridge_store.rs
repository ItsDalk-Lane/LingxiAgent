//! Bridge 数据层使用真实数据库、真实运行因果链与重启，验证重复消息和外发未知窗口。
use lingxi_adapters::storage::{bridge::*, session_files, RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::{
    ports::{RunOutcome, StoragePort},
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, RunId, RunStatus, SessionId};
use serde_json::json;
use std::path::PathBuf;

async fn fixture(tag: &str) -> (RunDatabase, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r07-bridge-{tag}-{}",
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
fn connector(platform: &str) -> BridgeConnectorInput {
    BridgeConnectorInput {
        id: format!("{platform}-connector"),
        platform: platform.into(),
        studio_id: "studio".into(),
        actor_id: "agent".into(),
        owner_subject: "user_local".into(),
        owner_sender_id: Some("owner".into()),
        enabled: true,
        config_json: "{\"future\":{\"keep\":true}}".into(),
    }
}
fn message(platform: &str, id: &str) -> BridgeInboxInput {
    BridgeInboxInput{platform:platform.into(),connector_id:format!("{platform}-connector"),conversation_id:"verified-chat".into(),conversation_kind:ConversationKind::Dm,message_id:id.into(),sender_id:"owner".into(),payload_json:json!({"text":"合成输入","role":"owner","ownerSubject":"forged","replyContext":{"messageId":id,"contextToken":"synthetic-private-context"}}).to_string(),attachments:vec![]}
}
async fn seed(db: &RunDatabase, sid: &str, owner: &str) {
    db.ensure_session_seed(vec![SessionRow {
        session_id: sid.into(),
        agent_id: "agent".into(),
        owner_user_id: owner.into(),
        title: "合成".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .unwrap();
}
async fn prepare(db: &RunDatabase, inbox: &BridgeInbox, _sid: &str) {
    let sid = inbox.session_id.as_deref().unwrap();
    let subject = lingxi_protocol::bridge_subject(
        &inbox.input.platform,
        &inbox.input.connector_id,
        &inbox.input.sender_id,
    )
    .unwrap();
    seed(db, sid, &subject).await;
    prepare_inbox_session(db, &inbox.id, inbox.connector_generation, sid, &[], 20)
        .await
        .unwrap();
}
async fn run(
    db: &RunDatabase,
    inbox: &BridgeInbox,
    _sid: &str,
    id: &str,
    terminal: bool,
) -> RunContext {
    let sid = inbox.session_id.as_deref().unwrap();
    let subject = lingxi_protocol::bridge_subject(
        &inbox.input.platform,
        &inbox.input.connector_id,
        &inbox.input.sender_id,
    )
    .unwrap();
    let ctx = RunContext {
        principal: Principal::Bridge { subject },
        session_id: SessionId::new(sid),
        run_id: RunId::new(id),
        attempt: AttemptId::new(format!("{id}#a1")),
        generation: 1,
    };
    db.record_run_started(&ctx, 21).await.unwrap();
    db.record_run_lineage(
        &ctx,
        RunLineage {
            parent_run_id: None,
            origin: RunOrigin::Bridge,
            source_message_id: Some(inbox.input.message_id.clone()),
            cause_id: Some(inbox.id.clone()),
        },
        21,
    )
    .await
    .unwrap();
    if terminal {
        db.commit_run_outcome(
            &ctx,
            RunOutcome {
                status: RunStatus::Completed,
                reason: None,
                key_events: vec![],
                final_message: None,
            },
            22,
        )
        .await
        .unwrap();
    }
    ctx
}
fn reply(inbox: &BridgeInbox, run: &str, id: &str) -> BridgeOutboxInput {
    BridgeOutboxInput {
        delivery_id: id.into(),
        connector_id: inbox.input.connector_id.clone(),
        connector_generation: inbox.connector_generation,
        inbox_id: Some(inbox.id.clone()),
        manual_action_id: None,
        run_id: Some(run.into()),
        payload_json: json!({"text":"合成回复"}).to_string(),
        attachments: vec![],
    }
}
async fn bound(db: &RunDatabase) -> BridgeInbox {
    put_connector(db, &connector("telegram"), None, 1)
        .await
        .unwrap();
    let inbox = claim_inbox(db, &message("telegram", "msg"), 1, 2)
        .await
        .unwrap()
        .inbox;
    prepare(db, &inbox, "bridge-session").await;
    run(db, &inbox, "bridge-session", "bridge-run", true).await;
    bind_inbox(
        db,
        &inbox.id,
        1,
        inbox.session_id.as_deref().unwrap(),
        "bridge-run",
        23,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn five_platform_roles_are_host_derived_and_message_replays_survive_reconfiguration() {
    let (db, path) = fixture("roles").await;
    for platform in ["telegram", "feishu", "dingtalk", "qq", "wechat"] {
        let mut cfg = connector(platform);
        put_connector(&db, &cfg, None, 1).await.unwrap();
        let input = message(platform, "original");
        let first = claim_inbox(&db, &input, 1, 2).await.unwrap();
        assert!(first.claimed);
        assert_eq!(first.inbox.role, BridgeRole::Owner);
        let mut guest = message(platform, "guest");
        guest.sender_id = "stranger".into();
        assert_eq!(
            claim_inbox(&db, &guest, 1, 3).await.unwrap().inbox.role,
            BridgeRole::Guest
        );
        guest.message_id = "group".into();
        guest.sender_id = "owner".into();
        guest.conversation_kind = ConversationKind::Group;
        guest.conversation_id = "group-chat".into();
        assert_eq!(
            claim_inbox(&db, &guest, 1, 4).await.unwrap().inbox.role,
            BridgeRole::Guest
        );
        cfg.owner_sender_id = None;
        let updated = put_connector(&db, &cfg, Some(1), 5).await.unwrap();
        assert_eq!(updated.generation, 2);
        assert_eq!(updated.input.config_json, cfg.config_json);
        let replay = claim_inbox(&db, &input, 1, 6).await.unwrap();
        assert!(!replay.claimed);
        assert_eq!(replay.inbox.id, first.inbox.id);
        assert_eq!(replay.inbox.role, first.inbox.role);
        assert_eq!(replay.inbox.connector_generation, 1);
        assert_eq!(replay.inbox.status, BridgeInboxStatus::Unknown);
        assert!(validate_inbox_claim(&db, &first.inbox.id, 1).await.is_err());
        assert!(
            claim_inbox(&db, &message(platform, "old-worker-new-message"), 1, 6)
                .await
                .is_err()
        );
        assert_eq!(
            claim_inbox(&db, &message(platform, "no-owner"), 2, 6)
                .await
                .unwrap()
                .inbox
                .role,
            BridgeRole::Guest
        );
        let mut changed = input.clone();
        changed.payload_json = json!({"text":"changed"}).to_string();
        assert!(claim_inbox(&db, &changed, 2, 7).await.is_err());
        assert!(put_connector(&db, &cfg, Some(1), 7).await.is_err());
    }
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert!(
        !claim_inbox(&db, &message("wechat", "original"), 2, 8)
            .await
            .unwrap()
            .claimed
    );
    cleanup(db, path).await;
}

#[tokio::test]
async fn concurrent_claims_and_config_edits_are_atomic() {
    let (db, path) = fixture("concurrent").await;
    let cfg = connector("telegram");
    put_connector(&db, &cfg, None, 1).await.unwrap();
    let other = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    let input = message("telegram", "same");
    let (a, b) = tokio::join!(
        claim_inbox(&db, &input, 1, 2),
        claim_inbox(&other, &input, 1, 2)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a.claimed, b.claimed);
    assert_eq!(a.inbox, b.inbox);
    let (a, b) = tokio::join!(
        put_connector(&db, &cfg, Some(1), 3),
        put_connector(&other, &cfg, Some(1), 3)
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert!(list_pending_inbox(&db).await.unwrap().is_empty());
    assert_eq!(
        get_inbox(&db, &inbox_id(&input))
            .await
            .unwrap()
            .unwrap()
            .status,
        BridgeInboxStatus::Unknown
    );
    other.close().await.unwrap();
    cleanup(db, path).await;
}

#[tokio::test]
async fn run_binding_checks_frozen_subject_and_exact_real_message_lineage() {
    let (db, path) = fixture("lineage").await;
    let inbox = bound(&db).await;
    assert_eq!(inbox.status, BridgeInboxStatus::Running);
    assert_eq!(
        get_inbox_run(&db, &inbox.id).await.unwrap(),
        Some((
            "bridge-run".into(),
            inbox.session_id.clone().unwrap(),
            "completed".into()
        ))
    );
    assert!(bind_inbox(&db, &inbox.id, 1, "wrong", "bridge-run", 24)
        .await
        .is_err());
    let settled = settle_inbox(
        &db,
        &inbox.id,
        BridgeInboxStatus::Succeeded,
        Some(json!({"ok":true})),
        None,
        25,
    )
    .await
    .unwrap();
    assert_eq!(settled.status, BridgeInboxStatus::Succeeded);
    assert!(settle_inbox(
        &db,
        &inbox.id,
        BridgeInboxStatus::Failed,
        None,
        Some("forged".into()),
        26
    )
    .await
    .is_err());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE run_lineage SET source_message_id='different' WHERE run_id='bridge-run'",
        [],
    )
    .unwrap();
    assert!(get_inbox_run(&db, &inbox.id).await.is_err());
    drop(conn);
    cleanup(db, path).await;
}

#[tokio::test]
async fn restart_preserves_prepared_queue_and_recovers_durable_run_before_bind_without_replay() {
    let (db, path) = fixture("restart").await;
    put_connector(&db, &connector("telegram"), None, 1)
        .await
        .unwrap();
    let queued = claim_inbox(&db, &message("telegram", "queued"), 1, 2)
        .await
        .unwrap()
        .inbox;
    prepare(&db, &queued, "queued-session").await;
    let unprepared = claim_inbox(&db, &message("telegram", "unprepared"), 1, 3)
        .await
        .unwrap()
        .inbox;
    let durable = claim_inbox(&db, &message("telegram", "durable"), 1, 4)
        .await
        .unwrap()
        .inbox;
    prepare(&db, &durable, "durable-session").await;
    run(&db, &durable, "durable-session", "durable-run", true).await;
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(recover_inbox(&db, 30).await.unwrap(), 2);
    assert_eq!(
        get_inbox(&db, &queued.id).await.unwrap().unwrap().status,
        BridgeInboxStatus::Claimed
    );
    assert_eq!(
        get_inbox(&db, &unprepared.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        BridgeInboxStatus::Unknown
    );
    let actual = get_inbox(&db, &durable.id).await.unwrap().unwrap();
    assert_eq!(actual.run_id.as_deref(), Some("durable-run"));
    assert_eq!(actual.status, BridgeInboxStatus::Running);
    assert!(
        !claim_inbox(&db, &message("telegram", "durable"), 1, 31)
            .await
            .unwrap()
            .claimed
    );
    assert_eq!(db.total_runs().await.unwrap(), 1);
    cleanup(db, path).await;
}

#[tokio::test]
async fn outbox_has_one_send_lease_and_requires_real_receipt_or_stays_unknown_after_restart() {
    let (db, path) = fixture("outbox").await;
    let inbox = bound(&db).await;
    let input = reply(&inbox, "bridge-run", "delivery");
    assert!(enqueue_outbox(&db, &input, 24).await.unwrap().claimed);
    assert!(!enqueue_outbox(&db, &input, 25).await.unwrap().claimed);
    let other = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        claim_outbox(&db, "delivery", 1, 26),
        claim_outbox(&other, "delivery", 1, 26)
    );
    assert_ne!(a.unwrap().claimed, b.unwrap().claimed);
    assert!(settle_outbox(
        &db,
        "delivery",
        BridgeOutboxStatus::Confirmed,
        None,
        None,
        27
    )
    .await
    .is_err());
    other.close().await.unwrap();
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(recover_started_outbox(&db, 28).await.unwrap(), 1);
    assert_eq!(
        get_outbox(&db, "delivery").await.unwrap().unwrap().status,
        BridgeOutboxStatus::Unknown
    );
    assert!(!claim_outbox(&db, "delivery", 1, 29).await.unwrap().claimed);
    assert!(list_pending_outbox(&db).await.unwrap().is_empty());
    let confirm = reply(&inbox, "bridge-run", "confirmed");
    enqueue_outbox(&db, &confirm, 30).await.unwrap();
    claim_outbox(&db, "confirmed", 1, 31).await.unwrap();
    let receipt = BridgePlatformReceipt {
        platform: "telegram".into(),
        message_ids: vec!["real-platform-message-1".into()],
        raw_json: Some("{\"ok\":true}".into()),
    };
    let done = settle_outbox(
        &db,
        "confirmed",
        BridgeOutboxStatus::Confirmed,
        Some(receipt.clone()),
        None,
        32,
    )
    .await
    .unwrap();
    assert_eq!(done.platform_receipt, Some(receipt.clone()));
    assert_eq!(
        settle_outbox(
            &db,
            "confirmed",
            BridgeOutboxStatus::Confirmed,
            Some(receipt),
            None,
            33
        )
        .await
        .unwrap(),
        done
    );
    let mut changed = confirm;
    changed.payload_json = "{\"text\":\"changed\"}".into();
    assert!(enqueue_outbox(&db, &changed, 34).await.is_err());
    cleanup(db, path).await;
}

#[tokio::test]
async fn owner_notification_target_and_context_are_frozen_from_actual_owner_dm_and_revoke_stops_queue(
) {
    let (db, path) = fixture("owner-notify").await;
    let inbox = bound(&db).await;
    seed(&db, "local", "user_local").await;
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("local"),
        run_id: RunId::new("local-run"),
        attempt: AttemptId::new("local-run#a1"),
        generation: 1,
    };
    db.record_run_started(&ctx, 24).await.unwrap();
    let mut input = reply(&inbox, "local-run", "notify");
    input.inbox_id = None;
    let queued = enqueue_outbox(&db, &input, 25).await.unwrap().outbox;
    assert_eq!(queued.target_inbox_id, inbox.id);
    assert_eq!(queued.conversation_id, "verified-chat");
    assert!(queued
        .reply_context_json
        .contains("synthetic-private-context"));
    let mut later = message("telegram", "later");
    later.conversation_id = "other-verified-chat".into();
    claim_inbox(&db, &later, 1, 26).await.unwrap();
    assert!(claim_outbox(&db, "notify", 1, 27).await.unwrap().claimed);
    let revoke = reply(&inbox, "bridge-run", "revoked");
    enqueue_outbox(&db, &revoke, 28).await.unwrap();
    let mut cfg = connector("telegram");
    cfg.enabled = false;
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_bridge_revoke BEFORE UPDATE ON bridge_outbox_v1 BEGIN SELECT RAISE(ABORT,'injected revoke failure'); END;").unwrap();
    assert!(put_connector(&db, &cfg, Some(1), 29).await.is_err());
    let unchanged = get_connector(&db, &cfg.id).await.unwrap().unwrap();
    assert_eq!(unchanged.generation, 1);
    assert!(unchanged.input.enabled);
    assert_eq!(
        get_outbox(&db, "revoked").await.unwrap().unwrap().status,
        BridgeOutboxStatus::Queued
    );
    conn.execute_batch("DROP TRIGGER reject_bridge_revoke;")
        .unwrap();
    drop(conn);
    put_connector(&db, &cfg, Some(1), 29).await.unwrap();
    assert_eq!(
        get_outbox(&db, "revoked").await.unwrap().unwrap().status,
        BridgeOutboxStatus::Failed
    );
    assert!(!claim_outbox(&db, "revoked", 1, 30).await.unwrap().claimed);
    assert!(
        enqueue_outbox(&db, &reply(&inbox, "bridge-run", "new-revoked"), 30)
            .await
            .is_err()
    );
    cleanup(db, path).await;
}

#[tokio::test]
async fn attachments_need_real_owned_files_and_failed_prepare_rolls_back() {
    let (db, path) = fixture("files").await;
    put_connector(&db, &connector("telegram"), None, 1)
        .await
        .unwrap();
    let attachment = BridgeAttachment {
        attachment_id: "att".into(),
        kind: BridgeAttachmentKind::File,
        platform_ref: Some("platform-file-id".into()),
        filename: Some("input.txt".into()),
        mime: Some("text/plain".into()),
        size_bytes: Some(4),
    };
    let mut input = message("telegram", "file");
    input.attachments = vec![attachment];
    let mut bad = input.clone();
    bad.attachments[0].platform_ref = Some("/etc/passwd".into());
    assert!(claim_inbox(&db, &bad, 1, 2).await.is_err());
    let inbox = claim_inbox(&db, &input, 1, 2).await.unwrap().inbox;
    let subject =
        lingxi_protocol::bridge_subject("telegram", &input.connector_id, "owner").unwrap();
    let sid = inbox.session_id.as_deref().unwrap();
    seed(&db, sid, &subject).await;
    assert!(prepare_inbox_session(&db, &inbox.id, 1, sid, &[], 3)
        .await
        .is_err());
    assert!(!get_inbox(&db, &inbox.id).await.unwrap().unwrap().prepared);
    let file = path.parent().unwrap().join("input.txt");
    std::fs::write(&file, b"data").unwrap();
    let row = session_files::SessionFileRow {
        file_id: "sf_real".into(),
        owner_session_id: sid.into(),
        owner_key: format!("session:{sid}"),
        source_key: Some("bridge:att".into()),
        identity_key: file.to_string_lossy().into_owned(),
        file_path: file.to_string_lossy().into_owned(),
        real_path: file.to_string_lossy().into_owned(),
        storage_kind: session_files::StorageKind::ManagedCache,
        status: session_files::SessionFileStatus::Available,
        label: None,
        filename: "input.txt".into(),
        mime: "text/plain".into(),
        size_bytes: 4,
        mtime_ms: 1,
        is_directory: false,
        file_kind: session_files::FileKind::Document,
        origin: session_files::SessionFileOrigin::BridgeInbound,
        registered_at_ms: 1,
        updated_at_ms: 1,
        expires_at_ms: None,
        legacy_file_ids: vec![],
        legacy_file_paths: vec![],
    };
    session_files::insert_session_file(&db, &row).await.unwrap();
    let files = [BridgeInboxFile {
        attachment_id: "att".into(),
        file_id: "sf_real".into(),
    }];
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_bridge_prepare BEFORE UPDATE ON bridge_inbox_v1 BEGIN SELECT RAISE(ABORT,'injected prepare failure'); END;").unwrap();
    assert!(prepare_inbox_session(&db, &inbox.id, 1, sid, &files, 4)
        .await
        .is_err());
    assert!(!get_inbox(&db, &inbox.id).await.unwrap().unwrap().prepared);
    conn.execute_batch("DROP TRIGGER fail_bridge_prepare;")
        .unwrap();
    drop(conn);
    let done = prepare_inbox_session(&db, &inbox.id, 1, sid, &files, 5)
        .await
        .unwrap();
    assert!(done.prepared);
    assert_eq!(done.resolved_files, files);
    assert_eq!(recover_inbox(&db, 6).await.unwrap(), 0);
    session_files::update_session_file_status(
        &db,
        "sf_real",
        session_files::SessionFileStatus::Missing,
        None,
        7,
    )
    .await
    .unwrap();
    assert_eq!(recover_inbox(&db, 8).await.unwrap(), 1);
    assert_eq!(
        get_inbox(&db, &inbox.id).await.unwrap().unwrap().status,
        BridgeInboxStatus::Unknown
    );
    cleanup(db, path).await;
}

#[tokio::test]
async fn conversation_reset_is_atomic_keeps_metadata_and_pins_replayed_inbox_to_original_epoch() {
    let (db, path) = fixture("reset").await;
    put_connector(&db, &connector("telegram"), None, 1)
        .await
        .unwrap();
    let mut input = message("telegram", "before-reset");
    let mut payload: serde_json::Value = serde_json::from_str(&input.payload_json).unwrap();
    payload["displayName"] = json!("合成昵称");
    payload["avatarUrl"] = json!("https://example.invalid/avatar");
    input.payload_json = payload.to_string();
    let old = claim_inbox(&db, &input, 1, 2).await.unwrap().inbox;
    let resolved = resolve_conversation(&db, &old.id, 1, 3).await.unwrap();
    assert_eq!(resolved.epoch, 1);
    assert!(db
        .get_session(&resolved.session_id)
        .await
        .unwrap()
        .is_none());
    prepare(&db, &old, "ignored").await;
    let ctx = run(&db, &old, "ignored", "live", false).await;
    assert!(reset_conversation(
        &db,
        &input.connector_id,
        &input.conversation_id,
        &input.sender_id,
        1,
        4
    )
    .await
    .is_err());
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
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("INSERT INTO session_file_refs(file_id,session_id,ref_kind,created_at_ms) VALUES('existing-file',?1,'typed_object',5)",[&resolved.session_id]).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_bridge_reset BEFORE UPDATE ON bridge_conversations_v1 BEGIN SELECT RAISE(ABORT,'injected reset failure'); END;").unwrap();
    assert!(reset_conversation(
        &db,
        &input.connector_id,
        &input.conversation_id,
        &input.sender_id,
        1,
        6
    )
    .await
    .is_err());
    assert_eq!(
        session_files::list_refs_for_session(&db, &resolved.session_id)
            .await
            .unwrap()
            .len(),
        1
    );
    conn.execute_batch("DROP TRIGGER reject_bridge_reset;")
        .unwrap();
    drop(conn);
    let reset = reset_conversation(
        &db,
        &input.connector_id,
        &input.conversation_id,
        &input.sender_id,
        1,
        7,
    )
    .await
    .unwrap();
    assert_eq!(reset.epoch, 2);
    assert_eq!(reset.current_session_id, None);
    assert_eq!(reset.metadata["displayName"], "合成昵称");
    assert_eq!(
        reset.metadata["avatarUrl"],
        "https://example.invalid/avatar"
    );
    assert!(
        session_files::list_refs_for_session(&db, &resolved.session_id)
            .await
            .unwrap()
            .is_empty()
    );
    let replay = claim_inbox(&db, &input, 1, 8).await.unwrap();
    assert!(!replay.claimed);
    assert_eq!(replay.inbox.session_id, Some(resolved.session_id.clone()));
    assert_eq!(
        resolve_conversation(&db, &old.id, 1, 8).await.unwrap(),
        resolved
    );
    let mut next = message("telegram", "after-reset");
    next.payload_json = "{\"text\":\"after\"}".into();
    let next = claim_inbox(&db, &next, 1, 9).await.unwrap().inbox;
    let next_resolved = resolve_conversation(&db, &next.id, 1, 10).await.unwrap();
    assert_eq!(next_resolved.epoch, 2);
    assert_ne!(next_resolved.session_id, resolved.session_id);
    let list = list_conversations(&db, &input.connector_id, 20)
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].metadata["displayName"], "合成昵称");
    assert_eq!(db.total_runs().await.unwrap(), 1);
    cleanup(db, path).await;
}

#[tokio::test]
async fn explicit_owner_takeover_is_one_inbox_scope_and_never_relaxes_ordinary_bridge_binding() {
    let (db, path) = fixture("takeover").await;
    put_connector(&db, &connector("telegram"), None, 1)
        .await
        .unwrap();
    seed(&db, "desktop", "user_local").await;
    seed(&db, "other-desktop", "user_local").await;
    let owner = claim_inbox(&db, &message("telegram", "takeover"), 1, 2)
        .await
        .unwrap()
        .inbox;
    assert!(prepare_inbox_session(&db, &owner.id, 1, "desktop", &[], 3)
        .await
        .is_err());
    let prepared = prepare_inbox_takeover(&db, &owner.id, 1, "desktop", &[], 4)
        .await
        .unwrap();
    assert!(prepared.prepared_takeover);
    assert_eq!(prepared.session_id.as_deref(), Some("desktop"));
    assert!(
        prepare_inbox_takeover(&db, &owner.id, 1, "other-desktop", &[], 5)
            .await
            .is_err()
    );
    let actor =
        lingxi_protocol::bridge_subject("telegram", &owner.input.connector_id, "owner").unwrap();
    let scoped = RunContext {
        principal: Principal::BridgeTakeover {
            subject: actor,
            session_id: "desktop".into(),
        },
        session_id: SessionId::new("desktop"),
        run_id: RunId::new("takeover-run"),
        attempt: AttemptId::new("takeover-run#a1"),
        generation: 1,
    };
    db.record_run_started(&scoped, 6).await.unwrap();
    db.record_run_lineage(
        &scoped,
        RunLineage {
            parent_run_id: None,
            origin: RunOrigin::Bridge,
            source_message_id: Some(owner.input.message_id.clone()),
            cause_id: Some(owner.id.clone()),
        },
        6,
    )
    .await
    .unwrap();
    bind_inbox(&db, &owner.id, 1, "desktop", "takeover-run", 7)
        .await
        .unwrap();
    assert!(reset_conversation(
        &db,
        &owner.input.connector_id,
        &owner.input.conversation_id,
        &owner.input.sender_id,
        1,
        8
    )
    .await
    .is_err());
    let mut guest = message("telegram", "guest-takeover");
    guest.sender_id = "stranger".into();
    let guest = claim_inbox(&db, &guest, 1, 9).await.unwrap().inbox;
    assert!(
        prepare_inbox_takeover(&db, &guest.id, 1, "other-desktop", &[], 10)
            .await
            .is_err()
    );
    let ordinary = claim_inbox(&db, &message("telegram", "ordinary-next"), 1, 11)
        .await
        .unwrap()
        .inbox;
    assert!(!ordinary.prepared_takeover);
    assert_ne!(ordinary.session_id.as_deref(), Some("desktop"));
    assert_eq!(
        list_conversations(&db, &owner.input.connector_id, 20)
            .await
            .unwrap()
            .into_iter()
            .find(|row| row.sender_id == "owner")
            .unwrap()
            .current_session_id,
        ordinary.session_id
    );
    db.commit_run_outcome(
        &scoped,
        RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            key_events: vec![],
            final_message: None,
        },
        12,
    )
    .await
    .unwrap();
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    recover_inbox(&db, 13).await.unwrap();
    assert_eq!(
        get_inbox_run(&db, &owner.id).await.unwrap().unwrap().0,
        "takeover-run"
    );
    let out = enqueue_outbox(&db, &reply(&prepared, "takeover-run", "takeover-reply"), 14)
        .await
        .unwrap()
        .outbox;
    assert_eq!(out.conversation_id, owner.input.conversation_id);
    cleanup(db, path).await;
}

#[tokio::test]
async fn commands_use_durable_receipts_without_fake_runs_and_recover_unqueued_reply() {
    let (db, path) = fixture("commands").await;
    put_connector(&db, &connector("telegram"), None, 1)
        .await
        .unwrap();
    let mut input = message("telegram", "command");
    input.payload_json = json!({"text":"/rc","replyContext":{"messageId":"command"}}).to_string();
    let inbox = claim_inbox(&db, &input, 1, 2).await.unwrap().inbox;
    assert!(claim_command(&db, &inbox.id, 1, "reset", 3).await.is_err());
    let other = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        claim_command(&db, &inbox.id, 1, "rc", 4),
        claim_command(&other, &inbox.id, 1, "rc", 4)
    );
    assert_ne!(a.unwrap().claimed, b.unwrap().claimed);
    other.close().await.unwrap();
    assert_eq!(
        get_inbox(&db, &inbox.id).await.unwrap().unwrap().status,
        BridgeInboxStatus::Command
    );
    assert!(list_pending_inbox(&db).await.unwrap().is_empty());
    assert_eq!(db.total_runs().await.unwrap(), 0);
    let output = BridgeOutboxInput {
        delivery_id: format!("command:{}", inbox.id),
        connector_id: input.connector_id.clone(),
        connector_generation: 1,
        inbox_id: Some(inbox.id.clone()),
        manual_action_id: None,
        run_id: None,
        payload_json: "{\"text\":\"实际命令答复\"}".into(),
        attachments: vec![],
    };
    assert!(enqueue_outbox(&db, &output, 5).await.is_err());
    settle_command(
        &db,
        &inbox.id,
        BridgeCommandStatus::Confirmed,
        Some(json!({"type":"rc","text":"实际命令答复"})),
        None,
        6,
    )
    .await
    .unwrap();
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(recover_started_commands(&db, 7).await.unwrap(), 0);
    assert_eq!(list_pending_command_replies(&db).await.unwrap().len(), 1);
    assert!(
        !claim_command(&db, &inbox.id, 1, "rc", 8)
            .await
            .unwrap()
            .claimed
    );
    let queued = enqueue_outbox(&db, &output, 9).await.unwrap().outbox;
    assert_eq!(queued.input.run_id, None);
    assert!(list_pending_command_replies(&db).await.unwrap().is_empty());
    assert_eq!(db.total_runs().await.unwrap(), 0);
    claim_outbox(&db, &output.delivery_id, 1, 10).await.unwrap();
    recover_started_outbox(&db, 11).await.unwrap();
    assert!(
        !claim_outbox(&db, &output.delivery_id, 1, 12)
            .await
            .unwrap()
            .claimed
    );
    let mut second = message("telegram", "unknown-command");
    second.payload_json = "{\"text\":\"/exitrc\"}".into();
    let second = claim_inbox(&db, &second, 1, 13).await.unwrap().inbox;
    claim_command(&db, &second.id, 1, "exitrc", 14)
        .await
        .unwrap();
    assert_eq!(recover_started_commands(&db, 15).await.unwrap(), 1);
    assert_eq!(
        get_command(&db, &second.id).await.unwrap().unwrap().status,
        BridgeCommandStatus::Unknown
    );
    assert!(
        !claim_command(&db, &second.id, 1, "exitrc", 16)
            .await
            .unwrap()
            .claimed
    );
    let mut selection = message("telegram", "selection");
    selection.payload_json = "{\"text\":\"2\"}".into();
    let selection = claim_inbox(&db, &selection, 1, 17).await.unwrap().inbox;
    assert!(
        claim_command(&db, &selection.id, 1, "rc", 18)
            .await
            .unwrap()
            .claimed
    );
    cleanup(db, path).await;
}

async fn local_file(
    db: &RunDatabase,
    path: &std::path::Path,
    sid: &str,
    id: &str,
) -> session_files::SessionFileRow {
    let actual = path.parent().unwrap().join(format!("{id}.txt"));
    std::fs::write(&actual, b"manual bytes").unwrap();
    let row = session_files::SessionFileRow {
        file_id: id.into(),
        owner_session_id: sid.into(),
        owner_key: format!("session:{sid}"),
        source_key: Some(id.into()),
        identity_key: actual.to_string_lossy().into_owned(),
        file_path: actual.to_string_lossy().into_owned(),
        real_path: actual.to_string_lossy().into_owned(),
        storage_kind: session_files::StorageKind::ManagedCache,
        status: session_files::SessionFileStatus::Available,
        label: None,
        filename: format!("{id}.txt"),
        mime: "text/plain".into(),
        size_bytes: 12,
        mtime_ms: 1,
        is_directory: false,
        file_kind: session_files::FileKind::Document,
        origin: session_files::SessionFileOrigin::UserUpload,
        registered_at_ms: 1,
        updated_at_ms: 1,
        expires_at_ms: None,
        legacy_file_ids: vec![],
        legacy_file_paths: vec![],
    };
    session_files::insert_session_file(db, &row).await.unwrap();
    row
}
#[tokio::test]
async fn manual_send_has_real_user_file_authority_and_atomic_outbox_without_fake_run_or_inbound() {
    let (db, path) = fixture("manual").await;
    let target = bound(&db).await;
    seed(&db, "local-source", "user_local").await;
    local_file(&db, &path, "local-source", "sf_manual").await;
    let input = BridgeManualActionInput {
        id: "client-operation-1".into(),
        connector_id: target.input.connector_id.clone(),
        connector_generation: 1,
        target_inbox_id: target.id.clone(),
        source_session_id: "local-source".into(),
        file_id: "sf_manual".into(),
        content_sha256: lingxi_kernel::session_tree::sha256_hex(b"manual bytes"),
        label: None,
    };
    let first = enqueue_manual_action(&db, &input, 25).await.unwrap();
    assert!(first.created);
    assert_eq!(first.outbox.input.run_id, None);
    assert_eq!(first.outbox.input.inbox_id, None);
    assert_eq!(
        first.outbox.input.manual_action_id.as_deref(),
        Some("client-operation-1")
    );
    assert_eq!(first.outbox.input.attachments[0].size_bytes, Some(12));
    assert!(
        !enqueue_manual_action(&db, &input, 26)
            .await
            .unwrap()
            .created
    );
    let mut changed = input.clone();
    changed.content_sha256 = "0".repeat(64);
    assert!(enqueue_manual_action(&db, &changed, 26).await.is_err());
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM bridge_inbox_v1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(db.total_runs().await.unwrap(), 1);
    conn.execute_batch("CREATE TRIGGER reject_manual_queue BEFORE INSERT ON bridge_outbox_v1 BEGIN SELECT RAISE(ABORT,'injected manual queue failure'); END;").unwrap();
    let mut second = input.clone();
    second.id = "second-action".into();
    assert!(enqueue_manual_action(&db, &second, 27).await.is_err());
    assert!(get_manual_action(&db, "second-action")
        .await
        .unwrap()
        .is_none());
    conn.execute_batch("DROP TRIGGER reject_manual_queue;")
        .unwrap();
    drop(conn);
    let mut forged = input.clone();
    forged.id = "foreign-file".into();
    forged.source_session_id = target.session_id.clone().unwrap();
    assert!(enqueue_manual_action(&db, &forged, 28).await.is_err());
    let mut qq = connector("qq");
    put_connector(&db, &qq, None, 29).await.unwrap();
    let mut group = message("qq", "group-document");
    group.conversation_kind = ConversationKind::Group;
    let group = claim_inbox(&db, &group, 1, 30).await.unwrap().inbox;
    let mut unsupported = input.clone();
    unsupported.id = "qq-document".into();
    unsupported.connector_id = qq.id.clone();
    unsupported.target_inbox_id = group.id;
    assert!(enqueue_manual_action(&db, &unsupported, 31).await.is_err());
    assert!(get_manual_action(&db, "qq-document")
        .await
        .unwrap()
        .is_none());
    qq.enabled = false;
    put_connector(&db, &qq, Some(1), 32).await.unwrap();
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(
        get_manual_action(&db, &input.id).await.unwrap().unwrap(),
        first.action
    );
    assert_eq!(
        claim_outbox(&db, &first.outbox.input.delivery_id, 1, 33)
            .await
            .unwrap()
            .outbox
            .input
            .manual_action_id,
        Some(input.id)
    );
    cleanup(db, path).await;
}

#[tokio::test]
async fn authority_role_changes_isolate_context_and_unproven_legacy_context_is_never_reused() {
    let (db, path) = fixture("role-context").await;
    let mut cfg = connector("telegram");
    put_connector(&db, &cfg, None, 1).await.unwrap();
    let mut initial = message("telegram", "owner-first");
    initial.payload_json = json!({"text":"owner context","displayName":"保存名称","avatarUrl":"https://example.invalid/avatar"}).to_string();
    let owner = claim_inbox(&db, &initial, 1, 2).await.unwrap().inbox;
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE bridge_conversations_v1 SET metadata_json=json_set(metadata_json,'$.future',json('{\"keep\":true}'))", []).unwrap();
    cfg.owner_sender_id = None;
    put_connector(&db, &cfg, Some(1), 3).await.unwrap();
    let mut guest_input = message("telegram", "same-sender-now-guest");
    guest_input.payload_json =
        json!({"text":"guest context","authorityRole":"owner","authorityVersion":1}).to_string();
    let guest = claim_inbox(&db, &guest_input, 2, 4).await.unwrap().inbox;
    assert_eq!(guest.role, BridgeRole::Guest);
    assert_eq!(guest.conversation_epoch, Some(2));
    assert_ne!(guest.session_id, owner.session_id);
    let row = list_conversations(&db, &cfg.id, 20)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(row.epoch, 2);
    assert_eq!(row.metadata["authorityRole"], "guest");
    assert_eq!(row.metadata["displayName"], "保存名称");
    assert_eq!(row.metadata["future"]["keep"], true);
    cfg.owner_sender_id = Some("owner".into());
    put_connector(&db, &cfg, Some(2), 5).await.unwrap();
    let restored = claim_inbox(&db, &message("telegram", "restored-owner"), 3, 6)
        .await
        .unwrap()
        .inbox;
    assert_eq!(restored.role, BridgeRole::Owner);
    assert_eq!(restored.conversation_epoch, Some(3));
    assert_ne!(restored.session_id, owner.session_id);
    assert_ne!(restored.session_id, guest.session_id);
    let same_role = claim_inbox(&db, &message("telegram", "owner-continuation"), 3, 7)
        .await
        .unwrap()
        .inbox;
    assert_eq!(same_role.session_id, restored.session_id);
    let duplicate = claim_inbox(&db, &initial, 1, 8).await.unwrap();
    assert!(!duplicate.claimed);
    assert_eq!(duplicate.inbox.session_id, owner.session_id);
    assert_eq!(duplicate.inbox.conversation_epoch, Some(1));

    let mut legacy_input = message("telegram", "legacy-before-role-marker");
    legacy_input.conversation_id = "legacy-chat".into();
    let legacy = claim_inbox(&db, &legacy_input, 3, 9).await.unwrap().inbox;
    // 模拟旧版本有上下文但没有可信角色标记或冻结epoch证据，不能据自报身份恢复权限。
    conn.execute(
        "UPDATE bridge_inbox_v1 SET conversation_epoch=NULL WHERE id=?1",
        [&legacy.id],
    )
    .unwrap();
    conn.execute("UPDATE bridge_conversations_v1 SET metadata_json=json_remove(metadata_json,'$.authorityVersion','$.authorityRole') WHERE conversation_id='legacy-chat'", []).unwrap();
    legacy_input.message_id = "legacy-next-message".into();
    let isolated = claim_inbox(&db, &legacy_input, 3, 10).await.unwrap().inbox;
    assert_eq!(isolated.conversation_epoch, Some(2));
    assert_ne!(isolated.session_id, legacy.session_id);
    drop(conn);
    cleanup(db, path).await;
}

#[tokio::test]
async fn started_manual_delivery_rechecks_live_authority_without_reclaiming_or_mutating_receipt() {
    let (db, path) = fixture("started-authority").await;
    let target = bound(&db).await;
    seed(&db, "local-source", "user_local").await;
    local_file(&db, &path, "local-source", "sf_started").await;
    let action = enqueue_manual_action(
        &db,
        &BridgeManualActionInput {
            id: "started-action".into(),
            connector_id: target.input.connector_id.clone(),
            connector_generation: 1,
            target_inbox_id: target.id.clone(),
            source_session_id: "local-source".into(),
            file_id: "sf_started".into(),
            content_sha256: lingxi_kernel::session_tree::sha256_hex(b"manual bytes"),
            label: None,
        },
        25,
    )
    .await
    .unwrap();
    let id = &action.outbox.input.delivery_id;
    assert!(validate_started_outbox(&db, id, 1).await.is_err());
    let started = claim_outbox(&db, id, 1, 26).await.unwrap().outbox;
    assert_eq!(validate_started_outbox(&db, id, 1).await.unwrap(), started);
    assert!(validate_started_outbox(&db, id, 2).await.is_err());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE session_files SET status='missing' WHERE file_id='sf_started'",
        [],
    )
    .unwrap();
    assert!(validate_started_outbox(&db, id, 1).await.is_err());
    assert_eq!(get_outbox(&db, id).await.unwrap().unwrap(), started);
    // 已领取不能再领取；这一结果不能代替真实外发前的权限核验。
    assert!(!claim_outbox(&db, id, 1, 27).await.unwrap().claimed);
    conn.execute(
        "UPDATE session_files SET status='available' WHERE file_id='sf_started'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE sessions SET owner_user_id='other-user' WHERE session_id='local-source'",
        [],
    )
    .unwrap();
    assert!(validate_started_outbox(&db, id, 1).await.is_err());
    conn.execute(
        "UPDATE sessions SET owner_user_id='user_local' WHERE session_id='local-source'",
        [],
    )
    .unwrap();
    assert_eq!(validate_started_outbox(&db, id, 1).await.unwrap(), started);
    let cfg = connector("telegram");
    put_connector(&db, &cfg, Some(1), 28).await.unwrap();
    assert!(validate_started_outbox(&db, id, 1).await.is_err());
    assert_eq!(get_outbox(&db, id).await.unwrap().unwrap(), started);
    settle_outbox(
        &db,
        id,
        BridgeOutboxStatus::Unknown,
        None,
        Some("revoked before send".into()),
        29,
    )
    .await
    .unwrap();
    assert!(validate_started_outbox(&db, id, 1).await.is_err());
    drop(conn);
    cleanup(db, path).await;
}

#[tokio::test]
async fn delivery_context_is_frozen_in_enqueue_transaction_and_duplicate_does_not_follow_reset() {
    let (db, path) = fixture("context").await;
    let target = bound(&db).await;
    seed(&db, "local-notify", "user_local").await;
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("local-notify"),
        run_id: RunId::new("notify-run"),
        attempt: AttemptId::new("notify-run#a1"),
        generation: 1,
    };
    db.record_run_started(&ctx, 24).await.unwrap();
    let mut input = reply(&target, "notify-run", "notify-context");
    input.inbox_id = None;
    input.payload_json=json!({"text":"actual notice","contextPolicy":"record_when_delivered","sourceSessionId":"local-notify","toolCallId":"actual-call"}).to_string();
    let first = enqueue_outbox(&db, &input, 25).await.unwrap().outbox;
    let frozen: serde_json::Value = serde_json::from_str(&first.input.payload_json).unwrap();
    assert_eq!(
        frozen["contextSessionId"],
        target.session_id.clone().unwrap()
    );
    assert_eq!(frozen["contextEpoch"], 1);
    reset_conversation(
        &db,
        &target.input.connector_id,
        &target.input.conversation_id,
        &target.input.sender_id,
        1,
        26,
    )
    .await
    .unwrap();
    let next = claim_inbox(&db, &message("telegram", "new-context"), 1, 27)
        .await
        .unwrap()
        .inbox;
    prepare(&db, &next, "ignored").await;
    assert_ne!(next.session_id, target.session_id);
    let replay = enqueue_outbox(&db, &input, 28).await.unwrap();
    assert!(!replay.claimed);
    assert_eq!(replay.outbox, first);
    let mut forged = input.clone();
    let mut payload: serde_json::Value = serde_json::from_str(&forged.payload_json).unwrap();
    payload["contextSessionId"] = json!(next.session_id.unwrap());
    payload["contextEpoch"] = json!(2);
    forged.payload_json = payload.to_string();
    assert!(enqueue_outbox(&db, &forged, 29).await.is_err());
    cleanup(db, path).await;
}
