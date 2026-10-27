//! SessionAdmin 的活动升格与真实运行共用同一会话许可，调用方中断不拆开事务。
use lingxi_adapters::storage::automation as jobs;
use lingxi_kernel::{
    ports::{RunOutcome, StoragePort},
    subagent::{RunLineage, RunOrigin},
    RunContext,
};
use lingxi_protocol::{AttemptId, RunId, RunStatus, SessionId};
use lingxi_service::{
    prepare_layout, session_admin::SessionAdminError, ConnectionKind, HomeSource, NetworkMode,
    ServiceConfig, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::json;

#[tokio::test]
async fn session_admin_refuses_live_lease_then_promotes_same_identity_after_release() {
    let home = std::env::temp_dir().join(format!(
        "lingxi-r07-promote-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
    std::fs::write(
        home.join("agents/lingxi/config.yaml"),
        "agent:\n  name: 合成\nmemory:\n  enabled: false\n",
    )
    .unwrap();
    let home = home.canonicalize().unwrap();
    let layout = prepare_layout(&home).unwrap();
    let state = ServiceState::bootstrap(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: 1000,
        },
        &layout,
    )
    .await
    .unwrap();
    state
        .session_admin()
        .create_session(
            "activity",
            "lingxi",
            "user_local",
            "合成活动",
            Some("read"),
            &[],
            true,
            1,
        )
        .await
        .unwrap();
    jobs::create_job(
        state.storage(),
        &jobs::AutomationJobInput {
            id: "job".into(),
            studio_id: "studio".into(),
            actor_id: "lingxi".into(),
            owner_kind: "local_user".into(),
            owner_subject: "user_local".into(),
            schema_version: 4,
            config_json: json!({"type":"every","actorAgentId":"lingxi","enabled":true}).to_string(),
            next_run_at: Some(10),
        },
        1,
    )
    .await
    .unwrap();
    jobs::claim_trigger(
        state.storage(),
        &jobs::TriggerInput {
            trigger_id: "trigger".into(),
            job_id: "job".into(),
            due_at: 10,
            config_revision: 1,
        },
        10,
    )
    .await
    .unwrap();
    let ctx = RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: SessionId::new("activity"),
        run_id: RunId::new("run"),
        attempt: AttemptId::new("run#a1"),
        generation: 1,
    };
    state.storage().record_run_started(&ctx, 10).await.unwrap();
    state
        .storage()
        .record_run_lineage(
            &ctx,
            RunLineage {
                parent_run_id: None,
                origin: RunOrigin::Cron,
                source_message_id: None,
                cause_id: Some("trigger".into()),
            },
            10,
        )
        .await
        .unwrap();
    jobs::bind_run(state.storage(), "trigger", "activity", "run", 11)
        .await
        .unwrap();
    state
        .storage()
        .commit_run_outcome(
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
    let token = state.auth().local_token();
    let principal = state
        .auth()
        .authenticate(
            Some(&format!("Bearer {token}")),
            None,
            false,
            ConnectionKind::Local,
        )
        .unwrap();
    let held = state
        .storage()
        .session_admission()
        .try_acquire(&["activity"], 512)
        .unwrap();
    assert!(matches!(
        state
            .session_admin()
            .promote_automation_activity(&principal, "trigger", 20)
            .await,
        Err(SessionAdminError::Busy)
    ));
    assert!(
        lingxi_adapters::storage::automation_activity::get_promotion(state.storage(), "trigger")
            .await
            .unwrap()
            .is_none()
    );
    drop(held);
    assert_eq!(
        state
            .session_admin()
            .promote_automation_activity(&principal, "trigger", 21)
            .await
            .unwrap(),
        "activity"
    );
    assert_eq!(
        state
            .session_admin()
            .promote_automation_activity(&principal, "trigger", 22)
            .await
            .unwrap(),
        "activity"
    );
    assert!(state
        .storage()
        .list_sessions()
        .await
        .unwrap()
        .iter()
        .any(|row| row.session_id == "activity"));
    assert_eq!(state.storage().total_runs().await.unwrap(), 1);
    state.storage().close().await.unwrap();
    std::fs::remove_dir_all(home).unwrap();
}
