//! 活动升格保留同一会话和运行；内部会话隐藏依靠真实登记或因果链。
use lingxi_adapters::storage::{
    automation as jobs, automation_activity as activity, session_admin, RunDatabase, SessionRow,
    StoreOptions,
};
use lingxi_kernel::{
    ports::{RunOutcome, StorageError, StoragePort},
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, RunId, RunStatus, SessionId};
use serde_json::json;
use std::path::PathBuf;

async fn fixture(tag: &str) -> (RunDatabase, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r07-activity-{tag}-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("runs.db");
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    db.ensure_session_seed(
        [
            "hidden",
            "scope",
            "activity-user-choice",
            "heartbeat-scope-user-choice",
            "staged",
        ]
        .into_iter()
        .map(|sid| SessionRow {
            session_id: sid.into(),
            agent_id: "agent".into(),
            owner_user_id: "user_local".into(),
            title: "合成会话".into(),
            created_at_unix_ms: 1,
        })
        .collect(),
    )
    .await
    .unwrap();
    (db, path)
}
async fn cleanup(db: RunDatabase, path: PathBuf) {
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
async fn claim(db: &RunDatabase, id: &str) {
    jobs::create_job(
        db,
        &jobs::AutomationJobInput {
            id: format!("job-{id}"),
            studio_id: "studio".into(),
            actor_id: "agent".into(),
            owner_kind: "local_user".into(),
            owner_subject: "user_local".into(),
            schema_version: 4,
            config_json:
                json!({"type":"every","actorAgentId":"agent","enabled":true,"prompt":"合成"})
                    .to_string(),
            next_run_at: Some(10),
        },
        1,
    )
    .await
    .unwrap();
    jobs::claim_trigger(
        db,
        &jobs::TriggerInput {
            trigger_id: id.into(),
            job_id: format!("job-{id}"),
            due_at: 10,
            config_revision: 1,
        },
        10,
    )
    .await
    .unwrap();
}
async fn run(db: &RunDatabase, trigger: &str, sid: &str, terminal: bool) -> RunContext {
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new(sid),
        run_id: RunId::new(format!("run-{trigger}")),
        attempt: AttemptId::new(format!("run-{trigger}#a1")),
        generation: 1,
    };
    db.record_run_started(&ctx, 10).await.unwrap();
    db.record_run_lineage(
        &ctx,
        RunLineage {
            parent_run_id: None,
            origin: RunOrigin::Cron,
            source_message_id: None,
            cause_id: Some(trigger.into()),
        },
        10,
    )
    .await
    .unwrap();
    jobs::bind_run(db, trigger, sid, ctx.run_id.as_str(), 11)
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
            12,
        )
        .await
        .unwrap();
        jobs::settle_trigger(
            db,
            trigger,
            jobs::TriggerStatus::Succeeded,
            Some(json!({"original":"unchanged"})),
            None,
            None,
            13,
        )
        .await
        .unwrap();
    }
    ctx
}
async fn visible(db: &RunDatabase) -> Vec<String> {
    db.list_sessions()
        .await
        .unwrap()
        .into_iter()
        .map(|session| session.session_id)
        .collect()
}

#[tokio::test]
async fn real_activity_hidden_until_promotion_and_user_named_prefixes_stay_visible() {
    let (db, path) = fixture("visibility").await;
    claim(&db, "trigger").await;
    run(&db, "trigger", "hidden", true).await;
    assert!(!visible(&db).await.contains(&"hidden".into()));
    assert!(visible(&db).await.contains(&"activity-user-choice".into()));
    assert!(visible(&db)
        .await
        .contains(&"heartbeat-scope-user-choice".into()));
    let before = jobs::get_trigger(&db, "trigger").await.unwrap();
    let runs = db.total_runs().await.unwrap();
    let result = activity::promote(&db, "trigger", "hidden", 20)
        .await
        .unwrap();
    assert_eq!(result.session_id, "hidden");
    assert!(visible(&db).await.contains(&"hidden".into()));
    assert_eq!(
        activity::promote(&db, "trigger", "hidden", 30)
            .await
            .unwrap(),
        result
    );
    assert_eq!(jobs::get_trigger(&db, "trigger").await.unwrap(), before);
    assert_eq!(db.total_runs().await.unwrap(), runs);
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(
        activity::get_promotion(&db, "trigger").await.unwrap(),
        Some(result)
    );
    assert!(visible(&db).await.contains(&"hidden".into()));
    cleanup(db, path).await;
}

#[tokio::test]
async fn internal_registration_hides_pre_dispatch_and_verified_heartbeat_scope_only() {
    let (db, path) = fixture("internal").await;
    claim(&db, "staged-trigger").await;
    assert!(
        !activity::is_heartbeat_scope(&db, "heartbeat-scope-user-choice", "agent")
            .await
            .unwrap()
    );
    activity::register_activity_session(&db, "staged-trigger", "staged", "agent", 20)
        .await
        .unwrap();
    activity::register_heartbeat_scope(&db, "scope", "agent", 20)
        .await
        .unwrap();
    assert!(!visible(&db).await.contains(&"staged".into()));
    assert!(!visible(&db).await.contains(&"scope".into()));
    assert!(visible(&db)
        .await
        .contains(&"heartbeat-scope-user-choice".into()));
    assert!(activity::is_heartbeat_scope(&db, "scope", "agent")
        .await
        .unwrap());
    assert!(!activity::is_heartbeat_scope(&db, "scope", "other-agent")
        .await
        .unwrap());
    assert!(
        activity::register_activity_session(&db, "staged-trigger", "scope", "agent", 21)
            .await
            .is_err()
    );
    assert!(activity::register_activity_session(
        &db,
        "staged-trigger",
        "staged",
        "other-agent",
        21
    )
    .await
    .is_err());
    assert!(activity::promote(&db, "staged-trigger", "staged", 22)
        .await
        .is_err());
    cleanup(db, path).await;
}

#[tokio::test]
async fn busy_wrong_target_and_revoked_owner_cannot_promote() {
    let (db, path) = fixture("reject").await;
    claim(&db, "trigger").await;
    let ctx = run(&db, "trigger", "hidden", false).await;
    assert!(
        matches!(activity::promote(&db,"trigger","hidden",20).await,Err(StorageError::Conflict{ref detail})if detail=="session_busy")
    );
    assert!(activity::promote(&db, "trigger", "scope", 20)
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
        21,
    )
    .await
    .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE sessions SET owner_user_id='foreign' WHERE session_id='hidden'",
        [],
    )
    .unwrap();
    assert!(activity::promote(&db, "trigger", "hidden", 22)
        .await
        .is_err());
    assert!(activity::get_promotion(&db, "trigger")
        .await
        .unwrap()
        .is_none());
    drop(conn);
    cleanup(db, path).await;
}

#[tokio::test]
async fn receipt_failure_rolls_back_without_changing_trigger_or_session() {
    let (db, path) = fixture("rollback").await;
    claim(&db, "trigger").await;
    run(&db, "trigger", "hidden", true).await;
    let before = jobs::get_trigger(&db, "trigger").await.unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_activity_promotion BEFORE INSERT ON automation_activity_promotions_v1 BEGIN SELECT RAISE(ABORT,'injected promotion failure'); END;").unwrap();
    assert!(activity::promote(&db, "trigger", "hidden", 20)
        .await
        .is_err());
    assert!(activity::get_promotion(&db, "trigger")
        .await
        .unwrap()
        .is_none());
    assert_eq!(jobs::get_trigger(&db, "trigger").await.unwrap(), before);
    assert!(!visible(&db).await.contains(&"hidden".into()));
    conn.execute_batch("DROP TRIGGER reject_activity_promotion")
        .unwrap();
    activity::promote(&db, "trigger", "hidden", 21)
        .await
        .unwrap();
    drop(conn);
    cleanup(db, path).await;
}

#[tokio::test]
async fn parallel_promotion_is_one_fact_and_heartbeat_resource_scope_cannot_be_promoted() {
    let (db, path) = fixture("parallel").await;
    claim(&db, "trigger").await;
    run(&db, "trigger", "hidden", true).await;
    let other = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    let (first, second) = tokio::join!(
        activity::promote(&db, "trigger", "hidden", 20),
        activity::promote(&other, "trigger", "hidden", 21)
    );
    assert_eq!(first.unwrap(), second.unwrap());
    assert_eq!(db.total_runs().await.unwrap(), 1);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM automation_activity_promotions_v1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    drop(conn);
    other.close().await.unwrap();

    activity::register_heartbeat_scope(&db, "scope", "agent", 22)
        .await
        .unwrap();
    claim(&db, "heartbeat-trigger").await;
    run(&db, "heartbeat-trigger", "scope", true).await;
    assert!(matches!(
        activity::promote(&db, "heartbeat-trigger", "scope", 23).await,
        Err(StorageError::Conflict { detail }) if detail == "automation_activity_internal_scope_mismatch"
    ));
    assert!(activity::get_promotion(&db, "heartbeat-trigger")
        .await
        .unwrap()
        .is_none());
    assert!(!visible(&db).await.contains(&"scope".into()));
    cleanup(db, path).await;
}

#[tokio::test]
async fn legitimate_session_deletion_cleans_new_foreign_keys_and_preserves_job_history() {
    let (db, path) = fixture("delete").await;
    claim(&db, "trigger").await;
    let ctx = run(&db, "trigger", "hidden", true).await;
    activity::register_activity_session(&db, "trigger", "hidden", "agent", 14)
        .await
        .unwrap();
    activity::promote(&db, "trigger", "hidden", 20)
        .await
        .unwrap();
    jobs::claim_daily(&db, "hidden", "2026-10-16", 21)
        .await
        .unwrap();
    jobs::reserve_notification(
        &db,
        &jobs::NotificationInput {
            delivery_id: "delivery".into(),
            trigger_id: Some("trigger".into()),
            run_id: Some(ctx.run_id.to_string()),
            channel: "desktop".into(),
            recipient: "user_local".into(),
            payload_json: json!({"text":"synthetic"}).to_string(),
        },
        22,
    )
    .await
    .unwrap();
    session_admin::archive_session(&db, "hidden", None, &[], 23)
        .await
        .unwrap();
    assert_eq!(
        session_admin::delete_archived_session(&db, "hidden")
            .await
            .unwrap(),
        session_admin::LifecycleOutcome::Done
    );
    assert!(db.get_session("hidden").await.unwrap().is_none());
    assert!(activity::get_promotion(&db, "trigger")
        .await
        .unwrap()
        .is_none());
    assert!(jobs::get_notification(&db, "delivery")
        .await
        .unwrap()
        .is_none());
    assert!(jobs::get_job(&db, "job-trigger").await.unwrap().is_some());
    assert!(jobs::get_trigger(&db, "trigger").await.unwrap().is_some());
    cleanup(db, path).await;
}

#[tokio::test]
async fn timeout_claim_uses_real_run_age_is_idempotent_and_never_relabels_completed_runs() {
    let (db, path) = fixture("timeout").await;
    claim(&db, "old").await;
    run(&db, "old", "hidden", false).await;
    assert!(!activity::claim_timeout(&db, "old", 1_200_009, 1_200_000)
        .await
        .unwrap());
    assert!(activity::claim_timeout(&db, "old", 1_200_010, 1_200_000)
        .await
        .unwrap());
    assert!(!activity::claim_timeout(&db, "old", 1_200_011, 1_200_000)
        .await
        .unwrap());
    let outcome = jobs::settle_trigger(
        &db,
        "old",
        jobs::TriggerStatus::Cancelled,
        None,
        None,
        None,
        1_200_020,
    )
    .await
    .unwrap();
    assert_eq!(outcome.trigger.error.as_deref(), Some("timeout"));
    jobs::settle_trigger(
        &db,
        "old",
        jobs::TriggerStatus::Cancelled,
        None,
        None,
        None,
        1_200_020,
    )
    .await
    .unwrap();
    claim(&db, "complete").await;
    run(&db, "complete", "staged", true).await;
    assert!(
        !activity::claim_timeout(&db, "complete", 1_200_030, 1_200_000)
            .await
            .unwrap()
    );
    assert_eq!(
        jobs::get_trigger(&db, "complete")
            .await
            .unwrap()
            .unwrap()
            .error,
        None
    );
    // 超时领取后恰好完成时，取消没有改变完成事实，不能在活动页假报超时失败。
    claim(&db, "race").await;
    let context = run(&db, "race", "scope", false).await;
    assert!(activity::claim_timeout(&db, "race", 1_200_010, 1_200_000)
        .await
        .unwrap());
    db.commit_run_outcome(
        &context,
        RunOutcome {
            status: RunStatus::Completed,
            reason: None,
            key_events: vec![],
            final_message: None,
        },
        1_200_011,
    )
    .await
    .unwrap();
    let settled = jobs::settle_trigger(
        &db,
        "race",
        jobs::TriggerStatus::Succeeded,
        None,
        None,
        None,
        1_200_012,
    )
    .await
    .unwrap();
    assert_eq!(settled.trigger.error, None);
    assert_eq!(settled.trigger.status, jobs::TriggerStatus::Succeeded);
    cleanup(db, path).await;
}
