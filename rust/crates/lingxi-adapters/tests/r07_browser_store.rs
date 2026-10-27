//! 浏览器回执、授权代次与重启均走正式数据库队列。
use lingxi_adapters::storage::{browser::*, RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::{
    browser::BrowserScope,
    ports::StoragePort,
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, RunId, SessionId};
use serde_json::json;
async fn fixture() -> (RunDatabase, std::path::PathBuf, BrowserScope) {
    let dir = std::env::temp_dir().join(format!(
        "r07-browser-store-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("runs.db");
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap_or_else(|e| panic!("browser fixture {}: {e:?}", path.display()));
    db.ensure_session_seed(vec![SessionRow {
        session_id: "s".into(),
        agent_id: "a".into(),
        owner_user_id: "user_local".into(),
        title: "浏览器隔离测试".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .unwrap();
    let row = ensure(&db, "s", "user_local", "node", 1).await.unwrap();
    let scope = BrowserScope {
        session_id: row.session_id,
        owner_subject: row.owner_subject,
        epoch: row.epoch,
        execution_node: row.execution_node,
        human: true,
    };
    (db, path, scope)
}
async fn run(db: &RunDatabase, id: &str, origin: RunOrigin) {
    let ctx = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("s"),
        run_id: RunId::new(id),
        attempt: AttemptId::new(format!("{id}-attempt")),
        generation: 1,
    };
    db.record_run_started(&ctx, 2).await.unwrap();
    db.record_run_lineage(
        &ctx,
        RunLineage {
            parent_run_id: None,
            origin,
            source_message_id: None,
            cause_id: None,
        },
        2,
    )
    .await
    .unwrap();
}
#[tokio::test]
async fn actual_receipts_deduplicate_reject_changed_input_and_recover_unknown_without_reexecution()
{
    let (db, path, scope) = fixture().await;
    let input = json!({"action":"navigate","url":"https://example.test"});
    assert!(matches!(
        claim_command(&db, &scope, "op", &input, false, 2)
            .await
            .unwrap(),
        CommandClaim::Started
    ));
    assert!(matches!(
        claim_command(&db, &scope, "op", &input, false, 3)
            .await
            .unwrap(),
        CommandClaim::Unknown(_)
    ));
    assert!(
        claim_command(&db, &scope, "op", &json!({"action":"stop"}), false, 3)
            .await
            .is_err()
    );
    finish_command(&db, "op", Some(json!({"actual":true})), None, false, 4)
        .await
        .unwrap();
    assert!(
        matches!(claim_command(&db,&scope,"op",&input,false,5).await.unwrap(),CommandClaim::Confirmed(v) if v["actual"]==true)
    );
    assert!(matches!(
        claim_command(&db, &scope, "pending", &input, false, 6)
            .await
            .unwrap(),
        CommandClaim::Started
    ));
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    recover_commands(&db, 7).await.unwrap();
    assert!(matches!(
        claim_command(&db, &scope, "pending", &input, false, 8)
            .await
            .unwrap(),
        CommandClaim::Unknown(_)
    ));
    assert!(matches!(
        claim_command(&db, &scope, "op", &input, false, 8)
            .await
            .unwrap(),
        CommandClaim::Confirmed(_)
    ));
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[tokio::test]
async fn emergency_stop_requires_new_actual_user_run_and_old_request_replay_cannot_revoke_again() {
    let (db, path, scope) = fixture().await;
    run(&db, "old", RunOrigin::User).await;
    let input = json!({"action":"emergency_stop"});
    claim_command(&db, &scope, "stop", &input, true, 3)
        .await
        .unwrap();
    let row = revoke(&db, "s", "user_local", 3).await.unwrap().unwrap();
    assert!(row.revoked);
    assert_eq!(
        revoke(&db, "s", "user_local", 4)
            .await
            .unwrap()
            .unwrap()
            .epoch,
        row.epoch
    );
    assert!(!release_for_user_run(&db, "s", "old", 4).await.unwrap());
    run(&db, "background", RunOrigin::Cron).await;
    assert!(!release_for_user_run(&db, "s", "background", 5)
        .await
        .unwrap());
    let revoked = BrowserScope {
        epoch: row.epoch,
        ..scope.clone()
    };
    assert!(claim_command(
        &db,
        &revoked,
        "forged",
        &json!({"action":"evaluate","expression":"emergency_stop"}),
        false,
        6
    )
    .await
    .is_err());
    finish_command(&db, "stop", Some(json!({"revoked":true})), None, false, 7)
        .await
        .unwrap();
    run(&db, "new", RunOrigin::User).await;
    assert!(release_for_user_run(&db, "s", "new", 8).await.unwrap());
    let row = get(&db, "s").await.unwrap().unwrap();
    let current = BrowserScope {
        epoch: row.epoch,
        ..scope
    };
    assert!(matches!(
        claim_command(&db, &current, "stop", &input, true, 9)
            .await
            .unwrap(),
        CommandClaim::Confirmed(_)
    ));
    assert!(!get(&db, "s").await.unwrap().unwrap().revoked);
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[tokio::test]
async fn downloads_can_confirm_unknown_once_without_overwriting_actual_receipt() {
    let (db, path, scope) = fixture().await;
    record_download(&db, &scope, "guid", None, None, Some("未确认"), 2)
        .await
        .unwrap();
    record_download(&db, &scope, "guid", None, Some("actual-file"), None, 3)
        .await
        .unwrap();
    record_download(
        &db,
        &BrowserScope {
            owner_subject: "other".into(),
            ..scope.clone()
        },
        "guid",
        None,
        Some("foreign-file"),
        None,
        4,
    )
    .await
    .unwrap();
    record_download(&db, &scope, "guid", None, None, Some("旧失败"), 5)
        .await
        .unwrap();
    assert_eq!(
        db.query_one_text(
            "SELECT file_id FROM browser_downloads_v1 WHERE download_id=?1",
            vec!["guid".into()]
        )
        .await
        .unwrap()
        .as_deref(),
        Some("actual-file")
    );
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
