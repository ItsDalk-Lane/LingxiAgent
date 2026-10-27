//! 媒体认领与恢复只使用正式数据库写队列，不替换内部存储。
use lingxi_adapters::storage::{media::*, RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::{
    ports::StoragePort,
    subagent::{RunLineage, RunOrigin},
    Principal, RunContext,
};
use lingxi_protocol::{AttemptId, RunId, SessionId};
use serde_json::json;
async fn fixture() -> (RunDatabase, std::path::PathBuf, MediaJob) {
    let root = std::env::temp_dir().join(format!(
        "r07-media-store-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("runs.db");
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    db.ensure_session_seed(vec![SessionRow {
        session_id: "s".into(),
        agent_id: "a".into(),
        owner_user_id: "user_local".into(),
        title: "媒体回执".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .unwrap();
    run(&db, "parent", RunOrigin::User, None, None, None).await;
    let job = MediaJob {
        operation_id: "media-op".into(),
        session_id: "s".into(),
        owner_subject: "user_local".into(),
        source_run_id: "parent".into(),
        execution_run_id: None,
        tool_call_id: "call".into(),
        kind: "image".into(),
        provider: "fixture".into(),
        request_digest: "a".repeat(64),
        spec: json!({"prompt":"合成数据"}),
        state: json!({"remoteOutcome":"not_yet_confirmed"}),
        phase: MediaPhase::Submitting,
        revision: 1,
        created_at: 2,
        updated_at: 2,
    };
    (db, path, job)
}
async fn run(
    db: &RunDatabase,
    id: &str,
    origin: RunOrigin,
    parent: Option<&str>,
    cause: Option<&str>,
    call: Option<&str>,
) {
    let context = RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("s"),
        run_id: RunId::new(id),
        attempt: AttemptId::new(format!("{id}-attempt")),
        generation: 1,
    };
    db.record_run_started(&context, 2).await.unwrap();
    db.record_run_lineage(
        &context,
        RunLineage {
            parent_run_id: parent.map(RunId::new),
            origin,
            source_message_id: call.map(String::from),
            cause_id: cause.map(String::from),
        },
        2,
    )
    .await
    .unwrap();
}
#[tokio::test]
async fn media_claim_never_repeats_a_dispatched_job_and_changed_input_is_rejected() {
    let (db, path, job) = fixture().await;
    assert!(matches!(
        claim(&db, job.clone()).await.unwrap(),
        MediaClaim::New(_)
    ));
    assert!(matches!(
        claim(&db, job.clone()).await.unwrap(),
        MediaClaim::Existing(_)
    ));
    let mut changed = job.clone();
    changed.request_digest = "b".repeat(64);
    assert!(claim(&db, changed).await.is_err());
    let submitted = transition(
        &db,
        &job,
        MediaPhase::Submitted,
        json!({"tracker":"remote-actual"}),
        3,
    )
    .await
    .unwrap();
    assert_eq!(submitted.revision, 2);
    assert!(
        transition(&db, &job, MediaPhase::Failed, json!({}), 4)
            .await
            .is_err(),
        "旧执行者不能覆盖新回执"
    );
    let downloading = transition(
        &db,
        &submitted,
        MediaPhase::Downloading,
        json!({"tracker":"remote-actual"}),
        4,
    )
    .await
    .unwrap();
    let completed = transition(
        &db,
        &downloading,
        MediaPhase::Completed,
        json!({"files":[{"fileId":"sf_actual"}]}),
        5,
    )
    .await
    .unwrap();
    assert!(
        transition(&db, &completed, MediaPhase::Cancelled, json!({}), 6)
            .await
            .is_err(),
        "已确认交付不被晚取消改写"
    );
    assert!(list_open(&db).await.unwrap().is_empty());
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
#[tokio::test]
async fn only_matching_actual_media_run_can_bind_and_resume_has_a_compare_and_swap_fence() {
    let (db, path, job) = fixture().await;
    claim(&db, job.clone()).await.unwrap();
    run(&db, "wrong", RunOrigin::User, None, None, None).await;
    assert!(bind_execution(&db, &job, "wrong", 3).await.is_err());
    run(
        &db,
        "media-one",
        RunOrigin::Media,
        Some("parent"),
        Some("media-op"),
        Some("call"),
    )
    .await;
    let bound = bind_execution(&db, &job, "media-one", 3).await.unwrap();
    assert_eq!(bound.execution_run_id.as_deref(), Some("media-one"));
    run(
        &db,
        "media-two",
        RunOrigin::Media,
        Some("parent"),
        Some("media-op"),
        Some("call"),
    )
    .await;
    assert!(
        bind_execution(&db, &job, "media-two", 4).await.is_err(),
        "两个恢复者只能一个绑定"
    );
    let submitted = transition(
        &db,
        &bound,
        MediaPhase::Submitted,
        json!({"tracker":"actual-provider-job","task":{"version":1}}),
        4,
    )
    .await
    .unwrap();
    db.close().await.unwrap();
    let db = RunDatabase::open(&path, StoreOptions::default())
        .await
        .unwrap();
    let restored = get(&db, &submitted.operation_id).await.unwrap().unwrap();
    assert_eq!(restored.state["tracker"], "actual-provider-job");
    assert!(
        matches!(
            claim(&db, job.clone()).await.unwrap(),
            MediaClaim::Existing(_)
        ),
        "重启重新认领只能取原回执"
    );
    let resumed = bind_execution(&db, &restored, "media-two", 5)
        .await
        .unwrap();
    assert_eq!(resumed.execution_run_id.as_deref(), Some("media-two"));
    assert_eq!(resumed.phase, MediaPhase::Submitted);
    db.close().await.unwrap();
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn publication_commits_the_real_file_and_partial_job_receipt_together_before_cancellation() {
    use lingxi_adapters::storage::session_files as sf;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let (db, path, job) = fixture().await;
    claim(&db, job.clone()).await.unwrap();
    run(
        &db,
        "actual-media",
        RunOrigin::Media,
        Some("parent"),
        Some("media-op"),
        Some("call"),
    )
    .await;
    let bound = bind_execution(&db, &job, "actual-media", 3).await.unwrap();
    let downloading = transition(
        &db,
        &bound,
        MediaPhase::Downloading,
        json!({"remoteOutcome":"completed","files":[]}),
        4,
    )
    .await
    .unwrap();
    let root = path.parent().unwrap();
    let row = |id: &str, index: u32| sf::SessionFileRow {
        file_id: id.into(),
        owner_session_id: "s".into(),
        owner_key: "id:s".into(),
        source_key: Some(format!("media:artifact:{index}")),
        identity_key: root
            .join(format!("{id}.png"))
            .to_string_lossy()
            .into_owned(),
        file_path: root
            .join(format!("{id}.png"))
            .to_string_lossy()
            .into_owned(),
        real_path: root
            .join(format!("{id}.png"))
            .to_string_lossy()
            .into_owned(),
        storage_kind: sf::StorageKind::ManagedCache,
        status: sf::SessionFileStatus::Available,
        label: Some(id.into()),
        filename: format!("{id}.png"),
        mime: "image/png".into(),
        size_bytes: 8,
        mtime_ms: 5,
        is_directory: false,
        file_kind: sf::FileKind::Image,
        origin: sf::SessionFileOrigin::AgentArtifact,
        registered_at_ms: 5,
        updated_at_ms: 5,
        expires_at_ms: None,
        legacy_file_ids: vec![],
        legacy_file_paths: vec![],
    };
    let gate = Arc::new(tokio::sync::RwLock::new(()));
    let cancel = Arc::new(AtomicBool::new(false));
    let actual_root = Arc::new(AtomicBool::new(false));
    let scope = actual_root.clone();
    sf::register_owned_media_blob(
        &db,
        row("sf_first", 0),
        b"\x89PNG\r\n\x1a\n".to_vec(),
        db.file_payload_leases().read_owned().await,
        MediaPublicationGuard {
            job: downloading.clone(),
            cancel: cancel.clone(),
            execution_cancelled: Arc::new(move || scope.load(Ordering::Acquire)),
            authority_lease: gate.clone().read_owned().await,
            artifact_index: 0,
        },
    )
    .await
    .unwrap();
    let partial = get(&db, &job.operation_id).await.unwrap().unwrap();
    assert_eq!(partial.state["files"][0]["fileId"], "sf_first");
    assert_eq!(partial.revision, downloading.revision + 1);
    assert_eq!(
        std::fs::read(root.join("sf_first.png")).unwrap(),
        b"\x89PNG\r\n\x1a\n"
    );
    let cancelled = transition(
        &db,
        &partial,
        MediaPhase::Cancelled,
        partial.state.clone(),
        6,
    )
    .await
    .unwrap();
    cancel.store(true, Ordering::Release);
    actual_root.store(true, Ordering::Release);
    let scope = actual_root.clone();
    assert!(sf::register_owned_media_blob(
        &db,
        row("sf_second", 1),
        b"\x89PNG\r\n\x1a\n".to_vec(),
        db.file_payload_leases().read_owned().await,
        MediaPublicationGuard {
            job: cancelled,
            cancel,
            execution_cancelled: Arc::new(move || scope.load(Ordering::Acquire)),
            authority_lease: gate.read_owned().await,
            artifact_index: 1
        }
    )
    .await
    .is_err());
    let actual = get(&db, &job.operation_id).await.unwrap().unwrap();
    assert_eq!(actual.phase, MediaPhase::Cancelled);
    assert_eq!(
        actual.state["files"].as_array().unwrap().len(),
        1,
        "取消不虚构撤回已经实际交付的文件"
    );
    assert!(!root.join("sf_second.png").exists());
    db.close().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
