//! 真SQLite、事务、因果Run与重启；不调用旧Agent循环。
use lingxi_adapters::storage::{conversations_collab::*, session_admin, RunDatabase, StoreOptions};
use lingxi_kernel::{
    ports::StoragePort,
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, RunId, SessionId};
use serde_json::json;
use std::path::PathBuf;

async fn fixture(tag: &str) -> (RunDatabase, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "r07-collab-store-{tag}-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("runs.db");
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    for (sid, agent, owner) in [
        ("source", "agent", "user_local"),
        ("target", "other-agent", "user_local"),
        ("foreign", "agent", "another-user"),
    ] {
        session_admin::create_session(
            &db,
            sid,
            agent,
            owner,
            "真实存储夹具",
            Some("ask"),
            &[],
            true,
            1,
        )
        .await
        .unwrap();
    }
    record_run(&db, "source", "source-run", None, None).await;
    (db, path)
}
async fn record_run(
    db: &RunDatabase,
    sid: &str,
    id: &str,
    cause: Option<&str>,
    parent: Option<&str>,
) {
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new(sid),
        run_id: RunId::new(id),
        attempt: AttemptId::new(format!("{id}#a1")),
        generation: 1,
    };
    db.record_run_started(&ctx, 2).await.unwrap();
    db.record_run_lineage(
        &ctx,
        RunLineage {
            parent_run_id: parent.map(RunId::new),
            origin: if cause.is_some() {
                RunOrigin::SessionCollab
            } else {
                RunOrigin::User
            },
            source_message_id: None,
            cause_id: cause.map(str::to_owned),
        },
        2,
    )
    .await
    .unwrap();
}
fn input(id: &str, kind: CollabDraftKind) -> CollabDraftInput {
    let draft = match kind {
        CollabDraftKind::Send => json!({"targetSessionId":"target","message":"原始建议正文"}),
        CollabDraftKind::Create => {
            json!({"agentId":"other-agent","firstMessage":"创建后的第一条正文","title":"新会话","model":null})
        }
    };
    CollabDraftInput {
        id: id.into(),
        studio_id: "local".into(),
        owner_subject: "user_local".into(),
        source_session_id: "source".into(),
        source_run_id: "source-run".into(),
        source_tool_call_id: format!("tool-{id}"),
        kind,
        draft_json: draft.to_string(),
        expires_at: 10_000,
    }
}
async fn queued(db: &RunDatabase, id: &str) -> CollabDraft {
    let v = input(id, CollabDraftKind::Send);
    create_collab_draft(db, &v, 3).await.unwrap();
    claim_collab_apply(db, id, "local", "user_local", &v.draft_json, 4)
        .await
        .unwrap()
        .draft
}
async fn started(db: &RunDatabase, id: &str) -> CollabDraft {
    queued(db, id).await;
    let d = claim_collab_delivery(db, id, 5).await.unwrap().draft;
    bind_collab_target(db, id, d.attempt, "target", false, 6)
        .await
        .unwrap()
}
async fn cleanup(db: RunDatabase, path: PathBuf) {
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
fn sql(path: &std::path::Path, statement: &str) {
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch(statement)
        .unwrap();
}
async fn scalar(db: &RunDatabase, sql: &str) -> String {
    db.query_one_text(sql, vec![]).await.unwrap().unwrap()
}

#[tokio::test]
async fn suggestions_are_inert_and_real_source_identity_and_payload_are_required() {
    let (db, path) = fixture("inert").await;
    let v = input("card", CollabDraftKind::Create);
    let d = create_collab_draft(&db, &v, 3).await.unwrap();
    assert_eq!(d.state, CollabDraftState::Pending);
    assert_eq!(d.delivery_status, CollabDeliveryStatus::None);
    assert_eq!(
        scalar(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM sessions").await,
        "3"
    );
    assert_eq!(
        scalar(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM runs").await,
        "1"
    );
    let mut replay = v.clone();
    replay.expires_at = 50_000;
    assert_eq!(
        create_collab_draft(&db, &replay, 4)
            .await
            .unwrap()
            .input
            .expires_at,
        10_000
    );
    let mut forged = v.clone();
    forged.id = "forged".into();
    forged.source_run_id = "model-claimed-run".into();
    assert!(create_collab_draft(&db, &forged, 3).await.is_err());
    forged.source_run_id = v.source_run_id.clone();
    forged.source_tool_call_id = "fresh-call".into();
    forged.owner_subject = "another-user".into();
    assert!(create_collab_draft(&db, &forged, 3).await.is_err());
    let mut changed = v.clone();
    changed.draft_json = json!({"agentId":"other-agent","firstMessage":"不同正文"}).to_string();
    assert!(create_collab_draft(&db, &changed, 4).await.is_err());
    changed = v.clone();
    changed.id = "same-tool-new-id".into();
    assert!(create_collab_draft(&db, &changed, 4).await.is_err());
    assert!(normalize_collab_payload(
        &CollabDraftKind::Send,
        &json!({"targetSessionId":"target","message":"hi","owner":"forged"}).to_string()
    )
    .is_err());
    cleanup(db, path).await;
}
#[tokio::test]
async fn apply_auth_expiry_read_only_and_frozen_edits_cannot_be_bypassed() {
    let (db, path) = fixture("authority").await;
    let v = input("card", CollabDraftKind::Send);
    create_collab_draft(&db, &v, 3).await.unwrap();
    for (studio, owner) in [("different", "user_local"), ("local", "another-user")] {
        assert!(
            claim_collab_apply(&db, "card", studio, owner, &v.draft_json, 4)
                .await
                .is_err()
        );
    }
    assert!(
        claim_collab_apply(&db, "card", "local", "user_local", &v.draft_json, 10_001)
            .await
            .is_err()
    );
    let foreign = json!({"targetSessionId":"foreign","message":"不能借编辑越权"}).to_string();
    assert!(
        claim_collab_apply(&db, "card", "local", "user_local", &foreign, 4)
            .await
            .is_err()
    );
    sql(
        &path,
        "UPDATE sessions SET permission_mode='read_only' WHERE session_id='source'",
    );
    assert!(
        claim_collab_apply(&db, "card", "local", "user_local", &v.draft_json, 4)
            .await
            .is_err()
    );
    sql(
        &path,
        "UPDATE sessions SET permission_mode='ask' WHERE session_id='source'",
    );
    let edited = json!({"targetSessionId":"target","message":"用户明确编辑正文"}).to_string();
    let first = claim_collab_apply(&db, "card", "local", "user_local", &edited, 4)
        .await
        .unwrap();
    assert!(first.claimed);
    assert_eq!(
        first.draft.frozen_apply_json.as_deref(),
        Some(edited.as_str())
    );
    assert!(
        !claim_collab_apply(&db, "card", "local", "user_local", &edited, 5)
            .await
            .unwrap()
            .claimed
    );
    assert!(
        claim_collab_apply(&db, "card", "local", "user_local", &v.draft_json, 5)
            .await
            .is_err()
    );
    cleanup(db, path).await;
}
#[tokio::test]
async fn concurrent_apply_and_delivery_have_one_durable_lease() {
    let (db, path) = fixture("concurrent").await;
    let v = input("card", CollabDraftKind::Create);
    create_collab_draft(&db, &v, 3).await.unwrap();
    let (a, b) = tokio::join!(
        claim_collab_apply(&db, "card", "local", "user_local", &v.draft_json, 4),
        claim_collab_apply(&db, "card", "local", "user_local", &v.draft_json, 4)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(usize::from(a.claimed) + usize::from(b.claimed), 1);
    assert_eq!(a.draft.reserved_target_id, b.draft.reserved_target_id);
    let (a, b) = tokio::join!(
        claim_collab_delivery(&db, "card", 5),
        claim_collab_delivery(&db, "card", 5)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(usize::from(a.claimed) + usize::from(b.claimed), 1);
    assert_eq!(a.draft.attempt, b.draft.attempt);
    assert_eq!(a.draft.attempt, 1);
    cleanup(db, path).await;
}
#[tokio::test]
async fn create_reservation_and_partial_failure_retry_only_the_same_real_target() {
    let (db, path) = fixture("partial").await;
    let v = input("create", CollabDraftKind::Create);
    create_collab_draft(&db, &v, 3).await.unwrap();
    let q = claim_collab_apply(&db, "create", "local", "user_local", &v.draft_json, 4)
        .await
        .unwrap()
        .draft;
    let sid = q.reserved_target_id.unwrap();
    let d = claim_collab_delivery(&db, "create", 5).await.unwrap().draft;
    session_admin::create_session(
        &db,
        &sid,
        "other-agent",
        "user_local",
        "新会话",
        Some("ask"),
        &[],
        true,
        6,
    )
    .await
    .unwrap();
    bind_collab_target(&db, "create", d.attempt, &sid, true, 7)
        .await
        .unwrap();
    let failed = settle_collab_delivery(
        &db,
        "create",
        d.attempt,
        CollabDeliveryResult::FailedBeforeAcceptance {
            reason: "明确未受理".into(),
        },
        8,
    )
    .await
    .unwrap();
    assert_eq!(failed.state, CollabDraftState::Pending);
    assert!(failed.target_created);
    assert_eq!(collab_result(&failed)["retryable"], true);
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    let retry = claim_collab_apply(&db, "create", "local", "user_local", &v.draft_json, 9)
        .await
        .unwrap()
        .draft;
    assert_eq!(retry.reserved_target_id.as_deref(), Some(sid.as_str()));
    assert_eq!(retry.target_session_id.as_deref(), Some(sid.as_str()));
    assert!(retry.target_created);
    assert_eq!(
        scalar(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM sessions").await,
        "4"
    );
    let changed=json!({"agentId":"other-agent","firstMessage":"换正文绕过冻结","model":null,"title":"新会话"}).to_string();
    assert!(
        claim_collab_apply(&db, "create", "local", "user_local", &changed, 9)
            .await
            .is_err()
    );
    cleanup(db, path).await;
}
#[tokio::test]
async fn started_unknown_survives_restart_and_only_real_lineage_can_confirm_it() {
    let (db, path) = fixture("unknown").await;
    let d = started(&db, "card").await;
    settle_collab_delivery(
        &db,
        "card",
        d.attempt,
        CollabDeliveryResult::Unknown {
            reason: "进程未确认受理".into(),
        },
        7,
    )
    .await
    .unwrap();
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    let old = get_collab_draft(&db, "card").await.unwrap().unwrap();
    assert_eq!(old.delivery_status, CollabDeliveryStatus::Unknown);
    assert!(!claim_collab_delivery(&db, "card", 8).await.unwrap().claimed);
    assert_eq!(list_queued_collab_drafts(&db).await.unwrap().len(), 0);
    assert!(settle_collab_delivery(
        &db,
        "card",
        d.attempt,
        CollabDeliveryResult::Accepted {
            run_id: "pretend-run".into()
        },
        8
    )
    .await
    .is_err());
    assert!(settle_collab_delivery(
        &db,
        "card",
        d.attempt,
        CollabDeliveryResult::RetryQueued {
            reason: "blind retry".into()
        },
        8
    )
    .await
    .is_err());
    record_run(
        &db,
        "target",
        "actual-delivery",
        Some("card"),
        Some("source-run"),
    )
    .await;
    assert_eq!(
        get_collab_run(&db, "card").await.unwrap().unwrap().0,
        "actual-delivery"
    );
    let accepted = settle_collab_delivery(
        &db,
        "card",
        d.attempt,
        CollabDeliveryResult::Accepted {
            run_id: "actual-delivery".into(),
        },
        9,
    )
    .await
    .unwrap();
    assert_eq!(accepted.state, CollabDraftState::Applied);
    assert_eq!(collab_result(&accepted)["firstMessageAccepted"], true);
    assert_eq!(
        scalar(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM runs").await,
        "2"
    );
    cleanup(db, path).await;
}
#[tokio::test]
async fn accepted_receipt_rolls_back_on_storage_error_and_cannot_be_requeued() {
    let (db, path) = fixture("rollback").await;
    let d = started(&db, "card").await;
    record_run(
        &db,
        "target",
        "actual-delivery",
        Some("card"),
        Some("source-run"),
    )
    .await;
    sql(&path,"CREATE TRIGGER deny_collab_settle BEFORE UPDATE OF state ON session_collab_drafts_v1 WHEN NEW.state='applied' BEGIN SELECT RAISE(ABORT,'rollback proof'); END;");
    assert!(settle_collab_delivery(
        &db,
        "card",
        d.attempt,
        CollabDeliveryResult::Accepted {
            run_id: "actual-delivery".into()
        },
        7
    )
    .await
    .is_err());
    assert_eq!(
        get_collab_draft(&db, "card")
            .await
            .unwrap()
            .unwrap()
            .delivery_status,
        CollabDeliveryStatus::Started
    );
    sql(&path, "DROP TRIGGER deny_collab_settle");
    assert!(settle_collab_delivery(
        &db,
        "card",
        d.attempt,
        CollabDeliveryResult::FailedBeforeAcceptance {
            reason: "不能隐藏已受理Run".into()
        },
        8
    )
    .await
    .is_err());
    let accepted = settle_collab_delivery(
        &db,
        "card",
        d.attempt,
        CollabDeliveryResult::Accepted {
            run_id: "actual-delivery".into(),
        },
        9,
    )
    .await
    .unwrap();
    assert_eq!(accepted.accepted_run_id.as_deref(), Some("actual-delivery"));
    assert_eq!(
        settle_collab_delivery(
            &db,
            "card",
            d.attempt,
            CollabDeliveryResult::Accepted {
                run_id: "actual-delivery".into()
            },
            10
        )
        .await
        .unwrap()
        .accepted_run_id,
        accepted.accepted_run_id
    );
    cleanup(db, path).await;
}
#[tokio::test]
async fn decisions_are_atomic_immutable_host_facts_and_repeated_polling_does_not_append() {
    let (db, path) = fixture("decisions").await;
    queued(&db, "card").await;
    sql(&path,"CREATE TRIGGER deny_collab_decision BEFORE INSERT ON session_collab_decisions_v1 BEGIN SELECT RAISE(ABORT,'rollback decision'); END;");
    assert!(record_collab_decision(&db, "card", 5).await.is_err());
    assert_eq!(
        list_unrecorded_collab_decisions(&db).await.unwrap().len(),
        1
    );
    assert_eq!(
        scalar(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM messages").await,
        "0"
    );
    assert!(
        !get_collab_draft(&db, "card")
            .await
            .unwrap()
            .unwrap()
            .decision_persisted
    );
    sql(&path, "DROP TRIGGER deny_collab_decision");
    let q = record_collab_decision(&db, "card", 6)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q["result"]["deliveryStatus"], "queued");
    assert_eq!(q["frozenApply"]["targetSessionId"], "target");
    assert!(list_unrecorded_collab_decisions(&db)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        record_collab_decision(&db, "card", 7).await.unwrap(),
        Some(q.clone())
    );
    assert_eq!(
        scalar(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM messages").await,
        "1"
    );
    let d = claim_collab_delivery(&db, "card", 8).await.unwrap().draft;
    bind_collab_target(&db, "card", d.attempt, "target", false, 9)
        .await
        .unwrap();
    record_run(
        &db,
        "target",
        "actual-delivery",
        Some("card"),
        Some("source-run"),
    )
    .await;
    settle_collab_delivery(
        &db,
        "card",
        d.attempt,
        CollabDeliveryResult::Accepted {
            run_id: "actual-delivery".into(),
        },
        10,
    )
    .await
    .unwrap();
    assert_eq!(
        list_unrecorded_collab_decisions(&db).await.unwrap().len(),
        1
    );
    assert!(
        !get_collab_draft(&db, "card")
            .await
            .unwrap()
            .unwrap()
            .decision_persisted,
        "旧queued收据不能冒充最新accepted事实已经落盘"
    );
    let accepted = record_collab_decision(&db, "card", 11)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(accepted["result"]["deliveryStatus"], "accepted");
    assert!(list_unrecorded_collab_decisions(&db)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        collab_decisions_for_session(&db, "source").await.unwrap(),
        vec![accepted]
    );
    assert_eq!(scalar(&db,"SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE role='host_context' AND entry_type='hana-session-collab-decision'").await,"2");
    assert_eq!(
        scalar(
            &db,
            "SELECT CAST(COUNT(*) AS TEXT) FROM messages WHERE role='user'"
        )
        .await,
        "0"
    );
    assert_eq!(scalar(&db,"SELECT json_extract(content_json,'$.result.deliveryStatus') FROM messages ORDER BY seq LIMIT 1").await,"queued");
    cleanup(db, path).await;
}
#[tokio::test]
async fn rejection_never_uses_a_caller_supplied_source_or_reopens_a_started_draft() {
    let (db, path) = fixture("reject").await;
    let v = input("reject", CollabDraftKind::Send);
    create_collab_draft(&db, &v, 3).await.unwrap();
    assert!(
        reject_collab_draft(&db, "reject", "foreign-studio", "user_local", 4)
            .await
            .is_err()
    );
    assert_eq!(
        reject_collab_draft(&db, "reject", "local", "user_local", 4)
            .await
            .unwrap()
            .state,
        CollabDraftState::Rejected
    );
    assert_eq!(
        record_collab_decision(&db, "reject", 5)
            .await
            .unwrap()
            .unwrap()["status"],
        "rejected"
    );
    assert_eq!(
        reject_collab_draft(&db, "reject", "local", "user_local", 6)
            .await
            .unwrap()
            .state,
        CollabDraftState::Rejected
    );
    started(&db, "started").await;
    assert!(
        reject_collab_draft(&db, "started", "local", "user_local", 7)
            .await
            .is_err()
    );
    assert_eq!(
        scalar(&db, "SELECT session_id FROM session_collab_decisions_v1").await,
        "source"
    );
    cleanup(db, path).await;
}
