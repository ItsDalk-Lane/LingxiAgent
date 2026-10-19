//! 自动任务建议的真实存储验证：确认前零计划、身份边界、版本、并发与事务回滚。

use lingxi_adapters::storage::{
    automation::{self as jobs, AutomationJobInput},
    automation_suggestions::{self as drafts, Suggestion},
    RunDatabase, SessionRow, StoreOptions,
};
use lingxi_kernel::ports::StorageError;
use serde_json::{json, Value};
use std::path::PathBuf;

async fn fixture(tag: &str) -> (RunDatabase, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "lingxi-r07-suggestions-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("runs.db");
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    db.ensure_session_seed(
        [
            ("session", "user_local"),
            ("other-session", "user_local"),
            ("foreign-session", "foreign"),
        ]
        .into_iter()
        .map(|(id, owner)| SessionRow {
            session_id: id.into(),
            agent_id: "agent".into(),
            owner_user_id: owner.into(),
            title: "隔离建议测试".into(),
            created_at_unix_ms: 1,
        })
        .collect(),
    )
    .await
    .unwrap();
    (db, path)
}

fn suggestion() -> Suggestion {
    Suggestion{
        id:"suggestion".into(),studio_id:"studio".into(),session_id:"session".into(),owner_kind:"local_user".into(),owner_subject:"user_local".into(),expected_revision:None,job_id:None,
        job:AutomationJobInput{id:"job".into(),studio_id:"studio".into(),actor_id:"agent".into(),owner_kind:"local_user".into(),owner_subject:"user_local".into(),schema_version:4,config_json:json!({"type":"every","schedule":60_000,"prompt":"原始提醒","label":"合成任务","enabled":true,"schemaVersion":4,"origin":"cron","actorAgentId":"agent","executionContext":{"sourceSessionId":"session"},"createdBy":{"kind":"agent","sourceSessionId":"session"},"unknown":{"保留":true}}).to_string(),next_run_at:Some(60_000)},expires_at:1_000_000,
    }
}

async fn cleanup(db: RunDatabase, path: PathBuf) {
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

fn state(path: &std::path::Path, id: &str) -> (String, Option<String>) {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT state,applied_job_id FROM automation_suggestions_v1 WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
}

#[tokio::test]
async fn proposal_creates_no_job_and_explicit_application_preserves_source_and_unknown_fields() {
    let (db, path) = fixture("apply").await;
    let draft = suggestion();
    drafts::create(&db, &draft, 1).await.unwrap();
    drafts::create(&db, &draft, 2).await.unwrap();
    assert!(
        drafts::applied_job(&db, &draft.id, "studio", "session", "user_local")
            .await
            .unwrap()
            .is_none()
    );
    assert!(jobs::list_jobs(&db, None).await.unwrap().is_empty());
    assert_eq!(state(&path, &draft.id), ("pending".into(), None));
    let saved = drafts::get(&db, &draft.id, "studio", "session", "user_local", 10)
        .await
        .unwrap();
    assert_eq!(saved, draft);
    let mut prepared = draft.job.clone();
    let mut config: Value = serde_json::from_str(&prepared.config_json).unwrap();
    config["prompt"] = json!("用户编辑后的提醒");
    config["schedule"] = json!(120_000);
    config["label"] = json!("用户标题");
    config["model"] = json!({"provider":"test","id":"model"});
    prepared.config_json = config.to_string();
    prepared.next_run_at = Some(120_000);
    let applied = drafts::apply(&db, &saved, &prepared, 20).await.unwrap();
    assert_eq!(applied.config_revision, 1);
    assert_eq!(applied.config_json, prepared.config_json);
    assert_eq!(applied.next_run_at, Some(120_000));
    assert_eq!(
        state(&path, &draft.id),
        ("applied".into(), Some("job".into()))
    );
    assert_eq!(jobs::list_jobs(&db, None).await.unwrap().len(), 1);
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    let replay = drafts::get(&db, &draft.id, "studio", "session", "user_local", 2_000_000)
        .await
        .unwrap();
    assert_eq!(
        drafts::applied_job(&db, &draft.id, "studio", "session", "user_local")
            .await
            .unwrap(),
        Some(applied.clone())
    );
    assert_eq!(
        drafts::apply(&db, &replay, &prepared, 2_000_001)
            .await
            .unwrap(),
        applied
    );
    assert_eq!(jobs::list_jobs(&db, None).await.unwrap().len(), 1);
    cleanup(db, path).await;
}

#[tokio::test]
async fn pending_expiry_and_every_identity_dimension_are_enforced_at_get_and_apply() {
    let (db, path) = fixture("scope").await;
    let mut draft = suggestion();
    draft.expires_at = 100;
    drafts::create(&db, &draft, 1).await.unwrap();
    for (studio, session, owner) in [
        ("wrong-studio", "session", "user_local"),
        ("studio", "other-session", "user_local"),
        ("studio", "session", "other-user"),
    ] {
        assert!(drafts::get(&db, &draft.id, studio, session, owner, 2)
            .await
            .is_err());
        assert!(drafts::applied_job(&db, &draft.id, studio, session, owner)
            .await
            .is_err());
    }
    for field in ["studio", "session", "owner", "kind"] {
        let mut forged = draft.clone();
        match field {
            "studio" => forged.studio_id = "wrong-studio".into(),
            "session" => forged.session_id = "other-session".into(),
            "owner" => forged.owner_subject = "foreign".into(),
            _ => forged.owner_kind = "automation".into(),
        }
        assert!(drafts::apply(&db, &forged, &draft.job, 2).await.is_err());
    }
    assert!(
        drafts::get(&db, &draft.id, "studio", "session", "user_local", 100)
            .await
            .is_err()
    );
    assert!(drafts::apply(&db, &draft, &draft.job, 100).await.is_err());
    assert!(jobs::list_jobs(&db, None).await.unwrap().is_empty());
    assert_eq!(state(&path, &draft.id).0, "pending");
    cleanup(db, path).await;
}

#[tokio::test]
async fn session_revocation_or_archival_blocks_even_a_previously_loaded_suggestion() {
    let (db, path) = fixture("session-revoke").await;
    let draft = suggestion();
    drafts::create(&db, &draft, 1).await.unwrap();
    let loaded = drafts::get(&db, &draft.id, "studio", "session", "user_local", 2)
        .await
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE sessions SET owner_user_id='foreign' WHERE session_id='session'",
        [],
    )
    .unwrap();
    assert!(drafts::apply(&db, &loaded, &loaded.job, 3).await.is_err());
    conn.execute("UPDATE sessions SET owner_user_id='user_local',lifecycle='archived' WHERE session_id='session'",[]).unwrap();
    assert!(
        drafts::get(&db, &draft.id, "studio", "session", "user_local", 4)
            .await
            .is_err()
    );
    assert!(drafts::apply(&db, &loaded, &loaded.job, 4).await.is_err());
    assert!(jobs::list_jobs(&db, None).await.unwrap().is_empty());
    drop(conn);
    cleanup(db, path).await;
}

#[tokio::test]
async fn stale_update_cannot_forge_new_revision_or_turn_into_creation() {
    let (db, path) = fixture("revision").await;
    let mut draft = suggestion();
    jobs::create_job(&db, &draft.job, 1).await.unwrap();
    draft.job_id = Some("job".into());
    draft.expected_revision = Some(1);
    draft.job.config_json = serde_json::from_str::<Value>(&draft.job.config_json)
        .map(|mut config| {
            config["prompt"] = json!("建议更新");
            config.to_string()
        })
        .unwrap();
    drafts::create(&db, &draft, 2).await.unwrap();
    let mut current = draft.job.clone();
    current.config_json = serde_json::from_str::<Value>(&current.config_json)
        .map(|mut config| {
            config["prompt"] = json!("用户抢先编辑");
            config.to_string()
        })
        .unwrap();
    let edited = jobs::update_job(&db, "job", &current, 1, 3).await.unwrap();
    assert!(matches!(
        drafts::apply(&db, &draft, &draft.job, 4).await,
        Err(StorageError::Conflict { .. })
    ));
    let mut forged = draft.clone();
    forged.expected_revision = Some(2);
    assert!(drafts::apply(&db, &forged, &forged.job, 4).await.is_err());
    forged.job_id = None;
    forged.expected_revision = None;
    assert!(drafts::apply(&db, &forged, &forged.job, 4).await.is_err());
    assert_eq!(jobs::get_job(&db, "job").await.unwrap().unwrap(), edited);
    assert_eq!(state(&path, &draft.id).0, "pending");
    cleanup(db, path).await;
}

#[tokio::test]
async fn forged_suggestion_cannot_change_frozen_target_actor_source_or_operation() {
    let (db, path) = fixture("forged-draft").await;
    let draft = suggestion();
    drafts::create(&db, &draft, 1).await.unwrap();
    let mut victim = draft.job.clone();
    victim.id = "victim-job".into();
    jobs::create_job(&db, &victim, 1).await.unwrap();
    let mut forged = draft.clone();
    forged.job_id = Some(victim.id.clone());
    forged.expected_revision = Some(1);
    forged.job = victim.clone();
    assert!(drafts::apply(&db, &forged, &victim, 2).await.is_err());
    for field in ["id", "actor", "schema", "source", "expiry"] {
        let mut forged = draft.clone();
        match field {
            "id" => forged.job.id = "other-job".into(),
            "actor" => forged.job.actor_id = "other-agent".into(),
            "schema" => forged.job.schema_version = 5,
            "source" => {
                let mut config: Value = serde_json::from_str(&forged.job.config_json).unwrap();
                config["executionContext"] = json!({"sourceSessionId":"other-session"});
                forged.job.config_json = config.to_string();
            }
            _ => forged.expires_at += 1,
        }
        assert!(drafts::apply(&db, &forged, &forged.job, 2).await.is_err());
    }
    assert!(jobs::get_job(&db, "job").await.unwrap().is_none());
    assert_eq!(
        jobs::get_job(&db, "victim-job")
            .await
            .unwrap()
            .unwrap()
            .config_revision,
        1
    );
    cleanup(db, path).await;
}

#[tokio::test]
async fn prepared_job_cannot_replace_frozen_identity_source_or_unknown_fields() {
    let (db, path) = fixture("forged-prepared").await;
    let draft = suggestion();
    drafts::create(&db, &draft, 1).await.unwrap();
    for field in [
        "id",
        "studio",
        "actor",
        "ownerKind",
        "ownerSubject",
        "schema",
        "source",
        "createdBy",
        "unknown",
        "newField",
    ] {
        let mut prepared = draft.job.clone();
        match field {
            "id" => prepared.id = "other-job".into(),
            "studio" => prepared.studio_id = "other-studio".into(),
            "actor" => prepared.actor_id = "other-agent".into(),
            "ownerKind" => prepared.owner_kind = "device".into(),
            "ownerSubject" => prepared.owner_subject = "foreign".into(),
            "schema" => prepared.schema_version = 5,
            config_field => {
                let mut config: Value = serde_json::from_str(&prepared.config_json).unwrap();
                match config_field {
                    "source" => {
                        config["executionContext"] = json!({"sourceSessionId":"other-session"})
                    }
                    "createdBy" => config["createdBy"] = json!({"kind":"user"}),
                    "unknown" => {
                        config.as_object_mut().unwrap().remove("unknown");
                    }
                    _ => config["newPrivilege"] = json!(true),
                };
                prepared.config_json = config.to_string();
            }
        }
        assert!(
            drafts::apply(&db, &draft, &prepared, 2).await.is_err(),
            "冻结字段{field}应拒绝"
        );
    }
    assert!(jobs::list_jobs(&db, None).await.unwrap().is_empty());
    cleanup(db, path).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_apply_and_late_replay_create_one_job_and_one_receipt() {
    let (db, path) = fixture("concurrent").await;
    let draft = suggestion();
    drafts::create(&db, &draft, 1).await.unwrap();
    let mut attempts = Vec::new();
    for _ in 0..32 {
        let db = db.clone();
        let draft = draft.clone();
        attempts.push(tokio::spawn(async move {
            drafts::apply(&db, &draft, &draft.job, 2).await.unwrap()
        }));
    }
    for attempt in attempts {
        let result = attempt.await.unwrap();
        assert_eq!(result.id, "job");
        assert_eq!(result.config_revision, 1);
    }
    assert_eq!(jobs::list_jobs(&db, None).await.unwrap().len(), 1);
    assert_eq!(
        state(&path, &draft.id),
        ("applied".into(), Some("job".into()))
    );
    let mut later = draft.job.clone();
    let mut config: Value = serde_json::from_str(&later.config_json).unwrap();
    config["prompt"] = json!("二次应用不能覆盖首次结果");
    later.config_json = config.to_string();
    assert_eq!(
        drafts::apply(&db, &draft, &later, 3)
            .await
            .unwrap()
            .config_json,
        draft.job.config_json
    );
    jobs::delete_job(&db, "job", 1, 4).await.unwrap();
    assert!(drafts::apply(&db, &draft, &draft.job, 5).await.is_err());
    assert!(jobs::list_jobs(&db, None).await.unwrap().is_empty());
    cleanup(db, path).await;
}

#[tokio::test]
async fn receipt_failure_rolls_back_both_new_job_and_existing_job_update() {
    let (db, path) = fixture("rollback").await;
    let mut draft = suggestion();
    drafts::create(&db, &draft, 1).await.unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_suggestion_receipt BEFORE UPDATE OF state ON automation_suggestions_v1 BEGIN SELECT RAISE(ABORT,'injected receipt failure'); END;").unwrap();
    assert!(drafts::apply(&db, &draft, &draft.job, 2).await.is_err());
    assert!(jobs::get_job(&db, "job").await.unwrap().is_none());
    assert_eq!(state(&path, &draft.id), ("pending".into(), None));
    conn.execute_batch("DROP TRIGGER reject_suggestion_receipt")
        .unwrap();
    drafts::apply(&db, &draft, &draft.job, 3).await.unwrap();
    draft.id = "update-suggestion".into();
    draft.job_id = Some("job".into());
    draft.expected_revision = Some(1);
    let mut config: Value = serde_json::from_str(&draft.job.config_json).unwrap();
    config["prompt"] = json!("建议覆盖");
    draft.job.config_json = config.to_string();
    drafts::create(&db, &draft, 4).await.unwrap();
    let before = jobs::get_job(&db, "job").await.unwrap().unwrap();
    conn.execute_batch("CREATE TRIGGER reject_suggestion_receipt BEFORE UPDATE OF state ON automation_suggestions_v1 BEGIN SELECT RAISE(ABORT,'injected receipt failure'); END;").unwrap();
    assert!(drafts::apply(&db, &draft, &draft.job, 5).await.is_err());
    assert_eq!(jobs::get_job(&db, "job").await.unwrap().unwrap(), before);
    assert_eq!(state(&path, &draft.id), ("pending".into(), None));
    conn.execute_batch("DROP TRIGGER reject_suggestion_receipt")
        .unwrap();
    let applied = drafts::apply(&db, &draft, &draft.job, 6).await.unwrap();
    assert_eq!(applied.config_revision, 2);
    drop(conn);
    cleanup(db, path).await;
}

#[tokio::test]
async fn malformed_or_nonlocal_proposals_cannot_persist_and_duplicate_id_cannot_overwrite() {
    let (db, path) = fixture("proposal-validation").await;
    let original = suggestion();
    for field in [
        "owner", "kind", "session", "revision", "update", "target", "json",
    ] {
        let mut draft = original.clone();
        match field {
            "owner" => {
                draft.owner_subject = "foreign".into();
                draft.job.owner_subject = "foreign".into();
                draft.session_id = "foreign-session".into();
            }
            "kind" => {
                draft.owner_kind = "automation".into();
                draft.job.owner_kind = "automation".into();
            }
            "session" => draft.session_id = "missing".into(),
            "revision" => draft.expected_revision = Some(1),
            "update" => draft.job_id = Some("job".into()),
            "target" => {
                draft.job_id = Some("different".into());
                draft.expected_revision = Some(1);
            }
            _ => draft.job.config_json = "[]".into(),
        }
        assert!(drafts::create(&db, &draft, 1).await.is_err());
    }
    drafts::create(&db, &original, 2).await.unwrap();
    let mut changed = original.clone();
    changed.expires_at += 100;
    assert!(matches!(
        drafts::create(&db, &changed, 3).await,
        Err(StorageError::Conflict { .. })
    ));
    assert_eq!(
        drafts::get(&db, "suggestion", "studio", "session", "user_local", 4)
            .await
            .unwrap(),
        original
    );
    assert!(jobs::list_jobs(&db, None).await.unwrap().is_empty());
    cleanup(db, path).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_distinct_update_suggestions_cannot_overwrite_each_others_version() {
    let (db, path) = fixture("concurrent-versions").await;
    let mut first = suggestion();
    jobs::create_job(&db, &first.job, 1).await.unwrap();
    first.job_id = Some("job".into());
    first.expected_revision = Some(1);
    let mut second = first.clone();
    second.id = "second-suggestion".into();
    let mut config: Value = serde_json::from_str(&second.job.config_json).unwrap();
    config["prompt"] = json!("另一条建议");
    second.job.config_json = config.to_string();
    drafts::create(&db, &first, 2).await.unwrap();
    drafts::create(&db, &second, 2).await.unwrap();
    let mut attempts = Vec::new();
    for draft in [first, second] {
        let db = db.clone();
        attempts.push(tokio::spawn(async move {
            drafts::apply(&db, &draft, &draft.job, 3).await
        }));
    }
    let mut success = 0;
    let mut conflicts = 0;
    for attempt in attempts {
        match attempt.await.unwrap() {
            Ok(job) => {
                success += 1;
                assert_eq!(job.config_revision, 2);
            }
            Err(StorageError::Conflict { .. }) => conflicts += 1,
            other => panic!("unexpected concurrent apply: {other:?}"),
        }
    }
    assert_eq!((success, conflicts), (1, 1));
    assert_eq!(
        jobs::get_job(&db, "job")
            .await
            .unwrap()
            .unwrap()
            .config_revision,
        2
    );
    let states = [
        state(&path, "suggestion").0,
        state(&path, "second-suggestion").0,
    ];
    assert_eq!(states.iter().filter(|state| *state == "applied").count(), 1);
    assert_eq!(states.iter().filter(|state| *state == "pending").count(), 1);
    cleanup(db, path).await;
}
