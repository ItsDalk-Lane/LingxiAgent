//! R07 自动化持久层：真实数据库、并发竞争、重启与事务失败窗口。

use lingxi_adapters::storage::{automation::*, RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::{
    ports::{StorageError, StoragePort},
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, RunId, SessionId};
use serde_json::json;
use std::path::PathBuf;

async fn fixture(tag: &str) -> (RunDatabase, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r07-automation-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("runs.db");
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    db.ensure_session_seed(vec![SessionRow {
        session_id: "session".into(),
        agent_id: "agent".into(),
        owner_user_id: "user_local".into(),
        title: "隔离自动化测试".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .unwrap();
    (db, path)
}

fn job() -> AutomationJobInput {
    AutomationJobInput {
        id: "job".into(),
        studio_id: "studio".into(),
        actor_id: "agent".into(),
        owner_kind: "local_user".into(),
        owner_subject: "user_local".into(),
        schema_version: 1,
        config_json: "{ \"enabled\":true, \"unknown\":{\"保持\":[1,2]} }".into(),
        next_run_at: Some(100),
    }
}

fn trigger(id: &str) -> TriggerInput {
    TriggerInput {
        trigger_id: id.into(),
        job_id: "job".into(),
        due_at: 100,
        config_revision: 1,
    }
}

fn notification(id: &str) -> NotificationInput {
    NotificationInput {
        delivery_id: id.into(),
        trigger_id: Some("trigger".into()),
        run_id: None,
        channel: "desktop".into(),
        recipient: "user_local".into(),
        payload_json: "{ \"text\": \"合成提醒\", \"unknown\":true }".into(),
    }
}

async fn seed_run(db: &RunDatabase, run: &str, cause: Option<&str>, origin: RunOrigin) {
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("session"),
        run_id: RunId::new(run),
        attempt: AttemptId::new(format!("{run}-attempt")),
        generation: 1,
    };
    db.record_run_started(&ctx, 10).await.unwrap();
    db.record_run_lineage(
        &ctx,
        RunLineage {
            parent_run_id: None,
            origin,
            source_message_id: None,
            cause_id: cause.map(str::to_owned),
        },
        10,
    )
    .await
    .unwrap();
}

async fn cleanup(db: RunDatabase, path: PathBuf) {
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn reopen_preserves_raw_configuration_trigger_and_confirmed_delivery() {
    let (db, path) = fixture("reopen").await;
    let input = job();
    let saved = create_job(&db, &input, 1).await.unwrap();
    assert_eq!(saved.config_json, input.config_json);
    assert_eq!(saved.config_revision, 1);
    assert!(list_due_jobs(&db, 99, 10).await.unwrap().is_empty());
    assert_eq!(list_due_jobs(&db, 100, 10).await.unwrap().len(), 1);
    assert!(
        claim_trigger(&db, &trigger("trigger"), 100)
            .await
            .unwrap()
            .created
    );
    let delivery = notification("delivery");
    assert!(
        reserve_notification(&db, &delivery, 101)
            .await
            .unwrap()
            .reserved
    );
    assert!(
        claim_notification(&db, "delivery", 102)
            .await
            .unwrap()
            .reserved
    );
    settle_notification(
        &db,
        "delivery",
        NotificationStatus::Confirmed,
        Some(json!({"platformReceipt":"receipt-1"})),
        None,
        103,
    )
    .await
    .unwrap();
    settle_trigger(
        &db,
        "trigger",
        TriggerStatus::Succeeded,
        Some(json!({"delivered":true})),
        None,
        Some(200),
        104,
    )
    .await
    .unwrap();
    db.close().await.unwrap();
    let reopened = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert_eq!(
        get_job(&reopened, "job")
            .await
            .unwrap()
            .unwrap()
            .config_json,
        input.config_json
    );
    assert_eq!(
        get_job(&reopened, "job")
            .await
            .unwrap()
            .unwrap()
            .next_run_at,
        Some(200)
    );
    let duplicate = claim_trigger(&reopened, &trigger("trigger"), 300)
        .await
        .unwrap();
    assert!(!duplicate.created);
    assert_eq!(duplicate.trigger.status, TriggerStatus::Succeeded);
    assert!(
        !reserve_notification(&reopened, &delivery, 300)
            .await
            .unwrap()
            .reserved
    );
    assert!(
        !claim_notification(&reopened, "delivery", 300)
            .await
            .unwrap()
            .reserved
    );
    assert_eq!(
        list_notifications(&reopened, "trigger")
            .await
            .unwrap()
            .len(),
        1
    );
    cleanup(reopened, path).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_claims_and_configuration_updates_have_one_winner() {
    let (db, path) = fixture("concurrent").await;
    create_job(&db, &job(), 1).await.unwrap();
    let mut claims = Vec::new();
    for index in 0..32 {
        let db = db.clone();
        claims.push(tokio::spawn(async move {
            claim_trigger(&db, &trigger(&format!("trigger-{index}")), 100)
                .await
                .unwrap()
        }));
    }
    let mut created = 0;
    let mut actual = std::collections::HashSet::new();
    for claim in claims {
        let claim = claim.await.unwrap();
        created += usize::from(claim.created);
        actual.insert(claim.trigger.trigger_id);
    }
    assert_eq!(created, 1);
    assert_eq!(actual.len(), 1);
    assert_eq!(list_pending_triggers(&db).await.unwrap().len(), 1);
    let mut updates = Vec::new();
    for index in 0..16 {
        let db = db.clone();
        updates.push(tokio::spawn(async move {
            let mut input = job();
            input.config_json = json!({"enabled":true,"writer":index}).to_string();
            update_job(&db, "job", &input, 1, 200).await
        }));
    }
    let mut successful = 0;
    for update in updates {
        match update.await.unwrap() {
            Ok(job) => {
                successful += 1;
                assert_eq!(job.config_revision, 2)
            }
            Err(StorageError::Conflict { .. }) => {}
            other => panic!("unexpected update: {other:?}"),
        }
    }
    assert_eq!(successful, 1);
    cleanup(db, path).await;
}

#[tokio::test]
async fn stale_trigger_cannot_overwrite_edited_or_deleted_job_and_history_survives() {
    let (db, path) = fixture("revision").await;
    create_job(&db, &job(), 1).await.unwrap();
    claim_trigger(&db, &trigger("trigger"), 100).await.unwrap();
    let mut input = job();
    input.next_run_at = Some(999);
    input.config_json = json!({"enabled":true,"prompt":"new authorization"}).to_string();
    update_job(&db, "job", &input, 1, 101).await.unwrap();
    let outcome = settle_trigger(
        &db,
        "trigger",
        TriggerStatus::Succeeded,
        None,
        None,
        Some(200),
        102,
    )
    .await
    .unwrap();
    assert!(!outcome.cursor_advanced);
    assert_eq!(
        outcome.trigger.config_json,
        job().config_json,
        "触发冻结旧配置而非读取后来编辑的授权"
    );
    assert_eq!(
        get_job(&db, "job").await.unwrap().unwrap().next_run_at,
        Some(999)
    );
    assert!(matches!(
        settle_trigger(
            &db,
            "trigger",
            TriggerStatus::Failed,
            None,
            Some("conflict".into()),
            Some(300),
            103
        )
        .await,
        Err(StorageError::Conflict { .. })
    ));
    let deleted = delete_job(&db, "job", 2, 104).await.unwrap();
    assert_eq!(deleted.config_revision, 3);
    assert!(get_job(&db, "job").await.unwrap().is_none());
    assert!(list_jobs(&db, None).await.unwrap().is_empty());
    assert_eq!(
        list_trigger_history(&db, Some("job"), 10)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        create_job(&db, &job(), 105).await,
        Err(StorageError::Conflict { .. })
    ));
    assert!(
        !claim_trigger(&db, &trigger("trigger"), 105)
            .await
            .unwrap()
            .created
    );
    let mut fresh = trigger("new");
    fresh.config_revision = 3;
    fresh.due_at = 999;
    assert!(matches!(
        claim_trigger(&db, &fresh, 105).await,
        Err(StorageError::Conflict { .. })
    ));
    cleanup(db, path).await;
}

#[tokio::test]
async fn cursor_write_failure_rolls_back_trigger_settlement_as_one_transaction() {
    let (db, path) = fixture("rollback").await;
    create_job(&db, &job(), 1).await.unwrap();
    claim_trigger(&db, &trigger("trigger"), 100).await.unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_automation_cursor BEFORE UPDATE OF next_run_at ON automation_jobs_v1 BEGIN SELECT RAISE(ABORT,'injected cursor write failure'); END;").unwrap();
    assert!(settle_trigger(
        &db,
        "trigger",
        TriggerStatus::Succeeded,
        Some(json!({"result":true})),
        None,
        Some(200),
        101
    )
    .await
    .is_err());
    assert_eq!(
        get_trigger(&db, "trigger").await.unwrap().unwrap().status,
        TriggerStatus::Claimed
    );
    assert_eq!(
        get_job(&db, "job").await.unwrap().unwrap().next_run_at,
        Some(100)
    );
    conn.execute_batch("DROP TRIGGER reject_automation_cursor")
        .unwrap();
    drop(conn);
    assert!(
        settle_trigger(
            &db,
            "trigger",
            TriggerStatus::Succeeded,
            Some(json!({"result":true})),
            None,
            Some(200),
            102
        )
        .await
        .unwrap()
        .cursor_advanced
    );
    cleanup(db, path).await;
}

#[tokio::test]
async fn crash_before_binding_recovers_only_unique_durable_causal_run() {
    let (db, path) = fixture("lineage").await;
    create_job(&db, &job(), 1).await.unwrap();
    claim_trigger(&db, &trigger("trigger"), 100).await.unwrap();
    seed_run(&db, "run-1", Some("trigger"), RunOrigin::Cron).await;
    seed_run(&db, "unrelated", Some("other"), RunOrigin::Cron).await;
    assert!(bind_run(&db, "trigger", "session", "unrelated", 101)
        .await
        .is_err());
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    let found = get_trigger_run(&db, "trigger").await.unwrap().unwrap();
    assert_eq!(found.0, "run-1");
    assert_eq!(found.1, "session");
    assert_eq!(found.2, "running");
    let bound = bind_run(&db, "trigger", "session", "run-1", 102)
        .await
        .unwrap();
    assert_eq!(bound.status, TriggerStatus::Running);
    assert_eq!(
        bind_run(&db, "trigger", "session", "run-1", 103)
            .await
            .unwrap(),
        bound
    );
    seed_run(&db, "run-2", Some("trigger"), RunOrigin::Heartbeat).await;
    assert!(matches!(
        get_trigger_run(&db, "trigger").await,
        Err(StorageError::Conflict { .. })
    ));
    cleanup(db, path).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notification_claim_is_unique_and_unacknowledged_delivery_never_requeues() {
    let (db, path) = fixture("notification").await;
    create_job(&db, &job(), 1).await.unwrap();
    claim_trigger(&db, &trigger("trigger"), 100).await.unwrap();
    let input = notification("delivery");
    reserve_notification(&db, &input, 101).await.unwrap();
    assert_eq!(list_pending_notifications(&db).await.unwrap().len(), 1);
    assert!(settle_notification(
        &db,
        "delivery",
        NotificationStatus::Confirmed,
        None,
        None,
        102
    )
    .await
    .is_err());
    let mut claims = Vec::new();
    for _ in 0..16 {
        let db = db.clone();
        claims.push(tokio::spawn(async move {
            claim_notification(&db, "delivery", 102)
                .await
                .unwrap()
                .reserved
        }));
    }
    let mut claimed = 0;
    for claim in claims {
        claimed += usize::from(claim.await.unwrap());
    }
    assert_eq!(claimed, 1);
    assert!(list_pending_notifications(&db).await.unwrap().is_empty());
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert!(
        !claim_notification(&db, "delivery", 103)
            .await
            .unwrap()
            .reserved
    );
    assert_eq!(recover_started_notifications(&db, 104).await.unwrap(), 1);
    assert_eq!(recover_started_notifications(&db, 105).await.unwrap(), 0);
    assert_eq!(
        get_notification(&db, "delivery")
            .await
            .unwrap()
            .unwrap()
            .status,
        NotificationStatus::Unknown
    );
    assert!(
        !reserve_notification(&db, &input, 106)
            .await
            .unwrap()
            .reserved
    );
    assert!(
        !claim_notification(&db, "delivery", 106)
            .await
            .unwrap()
            .reserved
    );
    assert!(list_pending_notifications(&db).await.unwrap().is_empty());
    let mut altered = input;
    altered.payload_json = "{\"text\":\"different\"}".into();
    assert!(matches!(
        reserve_notification(&db, &altered, 106).await,
        Err(StorageError::Conflict { .. })
    ));
    cleanup(db, path).await;
}

#[tokio::test]
async fn ordinary_user_notification_uses_real_run_without_fabricated_trigger() {
    let (db, path) = fixture("user-notify").await;
    let mut input = notification("user-delivery");
    input.trigger_id = None;
    assert!(reserve_notification(&db, &input, 1).await.is_err());
    input.run_id = Some("missing".into());
    assert!(reserve_notification(&db, &input, 1).await.is_err());
    seed_run(&db, "user-run", Some("request:user"), RunOrigin::User).await;
    input.run_id = Some("user-run".into());
    assert!(reserve_notification(&db, &input, 2).await.unwrap().reserved);
    assert!(
        claim_notification(&db, "user-delivery", 3)
            .await
            .unwrap()
            .reserved
    );
    settle_notification(
        &db,
        "user-delivery",
        NotificationStatus::Confirmed,
        Some(json!({"displayed":true})),
        None,
        4,
    )
    .await
    .unwrap();
    create_job(&db, &job(), 5).await.unwrap();
    claim_trigger(&db, &trigger("trigger"), 100).await.unwrap();
    input.delivery_id = "bad-association".into();
    input.trigger_id = Some("trigger".into());
    assert!(matches!(
        reserve_notification(&db, &input, 101).await,
        Err(StorageError::Conflict { .. })
    ));
    cleanup(db, path).await;
}

#[tokio::test]
async fn daily_known_failure_retries_after_one_hour_but_started_and_success_survive_restart() {
    let (db, path) = fixture("daily").await;
    let first = claim_daily(&db, "session", "2026-10-15", 100)
        .await
        .unwrap();
    assert!(first.claimed);
    assert_eq!(first.receipt.attempt, 1);
    assert!(
        !claim_daily(&db, "session", "2026-10-15", 200)
            .await
            .unwrap()
            .claimed
    );
    settle_daily(
        &db,
        "session",
        "2026-10-15",
        1,
        DailyStatus::Failed,
        None,
        Some("known preflight failure".into()),
        300,
    )
    .await
    .unwrap();
    let early = claim_daily(&db, "session", "2026-10-15", 3_600_299)
        .await
        .unwrap();
    assert!(!early.claimed);
    assert_eq!(early.retry_after, Some(3_600_300));
    let second = claim_daily(&db, "session", "2026-10-15", 3_600_300)
        .await
        .unwrap();
    assert!(second.claimed);
    assert_eq!(second.receipt.attempt, 2);
    assert!(matches!(
        settle_daily(
            &db,
            "session",
            "2026-10-15",
            1,
            DailyStatus::Succeeded,
            None,
            None,
            3_600_301
        )
        .await,
        Err(StorageError::Conflict { .. })
    ));
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    assert!(
        !claim_daily(&db, "session", "2026-10-15", 9_000_000)
            .await
            .unwrap()
            .claimed
    );
    settle_daily(
        &db,
        "session",
        "2026-10-15",
        2,
        DailyStatus::Succeeded,
        Some(json!({"compacted":true})),
        None,
        9_000_001,
    )
    .await
    .unwrap();
    assert!(
        !claim_daily(&db, "session", "2026-10-15", 20_000_000)
            .await
            .unwrap()
            .claimed
    );
    assert!(
        claim_daily(&db, "session", "2026-10-16", 20_000_000)
            .await
            .unwrap()
            .claimed
    );
    assert!(
        claim_daily(&db, "missing-session", "2026-10-16", 20_000_000)
            .await
            .is_err()
    );
    assert!(claim_daily(&db, "session", "not-a-date", 20_000_000)
        .await
        .is_err());
    assert!(claim_daily(&db, "session", "2026-02-29", 20_000_000)
        .await
        .is_err());
    assert!(claim_daily(&db, "session", "2026-99-99", 20_000_000)
        .await
        .is_err());
    cleanup(db, path).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn daily_concurrent_claim_has_one_attempt_and_unknown_never_retries() {
    let (db, path) = fixture("daily-concurrent").await;
    let mut claims = Vec::new();
    for _ in 0..16 {
        let db = db.clone();
        claims.push(tokio::spawn(async move {
            claim_daily(&db, "session", "2026-10-16", 100)
                .await
                .unwrap()
                .claimed
        }));
    }
    let mut count = 0;
    for claim in claims {
        count += usize::from(claim.await.unwrap());
    }
    assert_eq!(count, 1);
    settle_daily(
        &db,
        "session",
        "2026-10-16",
        1,
        DailyStatus::Unknown,
        None,
        Some("external result unknown".into()),
        200,
    )
    .await
    .unwrap();
    assert!(
        !claim_daily(&db, "session", "2026-10-16", 100_000_000)
            .await
            .unwrap()
            .claimed
    );
    assert_eq!(
        get_daily(&db, "session", "2026-10-16")
            .await
            .unwrap()
            .unwrap()
            .attempt,
        1
    );
    cleanup(db, path).await;
}

#[tokio::test]
async fn malformed_configuration_and_owner_changes_leave_original_job_untouched() {
    let (db, path) = fixture("invalid-input").await;
    let mut bad = job();
    bad.config_json = "{broken".into();
    assert!(create_job(&db, &bad, 1).await.is_err());
    assert!(list_jobs(&db, None).await.unwrap().is_empty());
    create_job(&db, &job(), 2).await.unwrap();
    let mut owner = job();
    owner.owner_subject = "other-user".into();
    assert!(matches!(
        update_job(&db, "job", &owner, 1, 3).await,
        Err(StorageError::Conflict { .. })
    ));
    let mut studio = job();
    studio.studio_id = "other-studio".into();
    assert!(matches!(
        update_job(&db, "job", &studio, 1, 3).await,
        Err(StorageError::Conflict { .. })
    ));
    assert_eq!(
        get_job(&db, "job").await.unwrap().unwrap().config_revision,
        1
    );
    assert_eq!(
        get_job(&db, "job").await.unwrap().unwrap().config_json,
        job().config_json
    );
    claim_trigger(&db, &trigger("trigger"), 100).await.unwrap();
    let mut reused = trigger("trigger");
    reused.due_at = 200;
    assert!(matches!(
        claim_trigger(&db, &reused, 200).await,
        Err(StorageError::Conflict { .. })
    ));
    cleanup(db, path).await;
}

#[tokio::test]
async fn one_shot_terminal_states_disable_same_revision_but_skip_does_not_advance() {
    let (db, path) = fixture("one-shot").await;
    for (index, status) in [
        TriggerStatus::Succeeded,
        TriggerStatus::Failed,
        TriggerStatus::Cancelled,
        TriggerStatus::Unknown,
        TriggerStatus::Skipped,
    ]
    .into_iter()
    .enumerate()
    {
        let mut input = job();
        input.id = format!("at-{index}");
        input.config_json = json!({"type":"at","enabled":true,"unknown":{"保留":7}}).to_string();
        create_job(&db, &input, 1).await.unwrap();
        let claim = TriggerInput {
            trigger_id: format!("trigger-{index}"),
            job_id: input.id.clone(),
            due_at: 100,
            config_revision: 1,
        };
        claim_trigger(&db, &claim, 100).await.unwrap();
        let outcome = settle_trigger(&db, &claim.trigger_id, status, None, None, Some(200), 101)
            .await
            .unwrap();
        let stored = get_job(&db, &input.id).await.unwrap().unwrap();
        let config: serde_json::Value = serde_json::from_str(&stored.config_json).unwrap();
        assert_eq!(config["unknown"], json!({"保留":7}));
        assert_eq!(stored.config_revision, 1, "运行收口不冒充用户编辑增代");
        if status == TriggerStatus::Skipped {
            assert!(!outcome.cursor_advanced);
            assert_eq!(config["enabled"], true);
            assert_eq!(stored.next_run_at, Some(100));
        } else {
            assert!(outcome.cursor_advanced);
            assert_eq!(config["enabled"], false);
            assert_eq!(stored.next_run_at, None);
        }
    }
    cleanup(db, path).await;
}

#[tokio::test]
async fn one_shot_old_revision_cannot_disable_edited_configuration() {
    let (db, path) = fixture("one-shot-stale").await;
    let mut input = job();
    input.config_json = json!({"type":"at","enabled":true,"prompt":"old"}).to_string();
    create_job(&db, &input, 1).await.unwrap();
    claim_trigger(&db, &trigger("trigger"), 100).await.unwrap();
    input.next_run_at = Some(999);
    input.config_json = json!({"type":"at","enabled":true,"prompt":"new","unknown":1}).to_string();
    update_job(&db, "job", &input, 1, 101).await.unwrap();
    let outcome = settle_trigger(
        &db,
        "trigger",
        TriggerStatus::Unknown,
        None,
        Some("external result unknown".into()),
        None,
        102,
    )
    .await
    .unwrap();
    assert!(!outcome.cursor_advanced);
    let stored = get_job(&db, "job").await.unwrap().unwrap();
    assert_eq!(stored.config_json, input.config_json);
    assert_eq!(stored.next_run_at, Some(999));
    assert_eq!(stored.config_revision, 2);
    cleanup(db, path).await;
}
