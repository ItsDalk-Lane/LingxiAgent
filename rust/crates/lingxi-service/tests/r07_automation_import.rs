//! 旧自动任务只读导入：真实服务身份与SQLite，不启动旧内核或访问真实用户目录。
use lingxi_adapters::storage::{automation as store, automation_import as raw_import};
use lingxi_service::{
    automation::JobApi,
    automation_import::{import_current_studio, ImportStatus},
    inject::{ManualClock, ServiceClock},
    prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

struct Harness {
    home: PathBuf,
    state: ServiceState,
    api: JobApi,
}
impl Harness {
    async fn open(home: Option<PathBuf>) -> Self {
        let home = home.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "lingxi-r07-import-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
        std::fs::write(
            home.join("agents/lingxi/config.yaml"),
            "agent:\n  name: 合成迁移助手\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let home = home.canonicalize().unwrap();
        let layout = prepare_layout(&home).unwrap();
        let clock = Arc::new(ManualClock::new(1_800_000_000_000));
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 1000,
            },
            &layout,
            ServiceDeps {
                clock: clock.clone(),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        if state
            .storage()
            .get_session("source")
            .await
            .unwrap()
            .is_none()
        {
            state
                .session_admin()
                .create_session(
                    "source",
                    "lingxi",
                    lingxi_kernel::LOCAL_OWNER_SUBJECT,
                    "合成来源",
                    Some("read"),
                    &[],
                    true,
                    clock.now_unix_ms(),
                )
                .await
                .unwrap();
        }
        let api = JobApi::from_state(&state);
        Self { home, state, api }
    }
    fn studio_desk(&self) -> PathBuf {
        self.home
            .join("studios")
            .join(&self.api.studio_id)
            .join("desk")
    }
    fn write_studio(&self, jobs: Value) -> PathBuf {
        let path = self.studio_desk().join("cron-jobs.json");
        write_json(
            &path,
            &json!({"storeRevision":9,"nextNum":8,"customDocument":{"keep":true},"jobs":jobs}),
        );
        path
    }
    async fn close(self) -> PathBuf {
        self.state.storage().close().await.unwrap();
        self.home
    }
}
fn write_json(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn old_job(id: &str) -> Value {
    let mut job = json!({"id":id,"schemaVersion":4,"configRevision":7,"type":"every","schedule":60_000,"actorAgentId":"lingxi","enabled":true,"prompt":"保留合成任务","label":"原任务","model":{"provider":"test","id":"same-name"},"createdAt":"2026-01-01T00:00:00Z","lastRunAt":"2026-01-02T00:00:00Z","nextRunAt":"2027-01-15T08:00:00Z","executionContext":{"sourceSessionId":"source","sourceSessionPath":"/untrusted/source.jsonl","cwd":"/untrusted/secret","workspaceFolders":["/untrusted/secret"],"authorizedFolders":["/untrusted/secret"],"customContext":{"keep":true}},"unknownField":{"keep":[1,2,3]}});
    job["executor"] = json!({"kind":"agent_session","workerUnknown":{"keep":true}});
    job
}

#[tokio::test]
async fn imports_unknown_fields_cursor_revision_and_raw_history_without_rewriting_sources() {
    let h = Harness::open(None).await;
    let path = h.write_studio(json!([old_job("studio_job_7")]));
    let history_path = h.studio_desk().join("cron-runs/studio_job_7.jsonl");
    std::fs::create_dir_all(history_path.parent().unwrap()).unwrap();
    let history="{\"status\":\"success\",\"opaque\":{\"保留\":7}}\n{\"status\":\"error\",\"error\":\"synthetic\"}\n";
    std::fs::write(&history_path, history).unwrap();
    let original = std::fs::read(&path).unwrap();
    let report = import_current_studio(&h.api).await;
    assert_eq!((report.imported, report.blocked), (1, 0), "{report:?}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_to_string(&history_path).unwrap(), history);
    let job = store::get_job(h.state.storage(), "studio_job_7")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.config_revision, 7);
    assert_eq!(job.actor_id, "lingxi");
    assert_eq!(job.created_at, 1_767_225_600_000);
    assert_eq!(job.updated_at, 1_767_312_000_000);
    let config: Value = serde_json::from_str(&job.config_json).unwrap();
    assert_eq!(config["unknownField"], json!({"keep":[1,2,3]}));
    assert_eq!(config["executor"]["workerUnknown"], json!({"keep":true}));
    assert_eq!(
        config["executionContext"]["customContext"],
        json!({"keep":true})
    );
    assert!(config["executionContext"]["cwd"].is_null());
    assert_eq!(config["executionContext"]["authorizedFolders"], json!([]));
    assert_eq!(config["executionContext"]["sourceSessionId"], "source");
    assert_eq!(
        config["legacyImport"]["rustR07"]["sourceJob"],
        old_job("studio_job_7")
    );
    assert_eq!(config["legacyImport"]["rustR07"]["historyJsonl"], history);
    assert_eq!(
        config["legacyImport"]["rustR07"]["documentMetadata"]["customDocument"],
        json!({"keep":true})
    );
    assert_eq!(h.state.storage().total_runs().await.unwrap(), 0);
    assert!(store::list_trigger_history(h.state.storage(), None, 10)
        .await
        .unwrap()
        .is_empty());
    let home = h.close().await;
    let h = Harness::open(Some(home)).await;
    let report = import_current_studio(&h.api).await;
    assert_eq!(
        (report.imported, report.already_present, report.blocked),
        (0, 1, 0),
        "{report:?}"
    );
    assert_eq!(
        store::get_job(h.state.storage(), "studio_job_7")
            .await
            .unwrap()
            .unwrap(),
        job
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn import_replay_does_not_overwrite_user_edits_or_resurrect_soft_deleted_jobs() {
    let h = Harness::open(None).await;
    h.write_studio(json!([old_job("old")]));
    assert_eq!(import_current_studio(&h.api).await.imported, 1);
    let prior = store::get_job(h.state.storage(), "old")
        .await
        .unwrap()
        .unwrap();
    let mut config: Value = serde_json::from_str(&prior.config_json).unwrap();
    config["label"] = json!("用户编辑");
    let edited = store::update_job(
        h.state.storage(),
        "old",
        &store::AutomationJobInput {
            id: prior.id.clone(),
            studio_id: prior.studio_id.clone(),
            actor_id: prior.actor_id.clone(),
            owner_kind: prior.owner_kind.clone(),
            owner_subject: prior.owner_subject.clone(),
            schema_version: prior.schema_version,
            config_json: config.to_string(),
            next_run_at: None,
        },
        7,
        1_800_000_000_001,
    )
    .await
    .unwrap();
    assert_eq!(import_current_studio(&h.api).await.already_present, 1);
    assert_eq!(
        store::get_job(h.state.storage(), "old")
            .await
            .unwrap()
            .unwrap(),
        edited
    );
    store::delete_job(h.state.storage(), "old", 8, 1_800_000_000_002)
        .await
        .unwrap();
    let report = import_current_studio(&h.api).await;
    assert_eq!(
        (report.imported, report.already_present, report.blocked),
        (0, 1, 0),
        "{report:?}"
    );
    assert!(store::get_job(h.state.storage(), "old")
        .await
        .unwrap()
        .is_none());
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn unresolved_paths_revoked_sessions_and_unknown_history_are_explicitly_blocked() {
    let h = Harness::open(None).await;
    let mut path_only = old_job("path-only");
    path_only["executionContext"]
        .as_object_mut()
        .unwrap()
        .remove("sourceSessionId");
    let mut unknown = old_job("unknown-history");
    unknown["unknownField"] = json!("retained in original source");
    let mut missing_actor = old_job("missing-actor");
    missing_actor["actorAgentId"] = json!("not-installed");
    let path = h.write_studio(json!([path_only, unknown, missing_actor]));
    let original = std::fs::read(&path).unwrap();
    let history = h.studio_desk().join("cron-runs/unknown-history.jsonl");
    std::fs::create_dir_all(history.parent().unwrap()).unwrap();
    std::fs::write(history, "{\"status\":\"unknown\"}\n").unwrap();
    let report = import_current_studio(&h.api).await;
    assert_eq!((report.imported, report.blocked), (0, 3), "{report:?}");
    assert!(report
        .entries
        .iter()
        .all(|entry| entry.status == ImportStatus::Blocked && entry.detail.is_some()));
    assert_eq!(std::fs::read(path).unwrap(), original);
    h.write_studio(json!([old_job("archived")]));
    h.state
        .session_admin()
        .archive_session("source", None, 1_800_000_000_001)
        .await
        .unwrap();
    let report = import_current_studio(&h.api).await;
    assert_eq!(report.blocked, 1, "{report:?}");
    assert!(store::list_jobs(h.state.storage(), None)
        .await
        .unwrap()
        .is_empty());
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn duplicate_or_colliding_ids_cannot_choose_or_overwrite_an_existing_job() {
    let h = Harness::open(None).await;
    let mut second = old_job("duplicate");
    second["prompt"] = json!("不能选择任意一份");
    h.write_studio(json!([old_job("duplicate"), second]));
    let report = import_current_studio(&h.api).await;
    assert_eq!((report.imported, report.blocked), (0, 2), "{report:?}");
    let input = store::AutomationJobInput {
        id: "occupied".into(),
        studio_id: h.api.studio_id.clone(),
        actor_id: "lingxi".into(),
        owner_kind: "local_user".into(),
        owner_subject: "user_local".into(),
        schema_version: 4,
        config_json: json!({"type":"every","enabled":false,"prompt":"new"}).to_string(),
        next_run_at: None,
    };
    let original = store::create_job(h.state.storage(), &input, 10)
        .await
        .unwrap();
    h.write_studio(json!([old_job("occupied")]));
    let report = import_current_studio(&h.api).await;
    assert_eq!((report.imported, report.blocked), (0, 1), "{report:?}");
    assert_eq!(
        store::get_job(h.state.storage(), "occupied")
            .await
            .unwrap()
            .unwrap(),
        original
    );
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn legacy_agent_jobs_use_verified_directory_identity_and_studio_legacy_refs_deduplicate() {
    let h = Harness::open(None).await;
    let mut studio = old_job("studio_job_1");
    studio["legacyRef"] = json!({"agentId":"lingxi","jobId":"job_1"});
    h.write_studio(json!([studio]));
    let legacy_path = h.home.join("agents/lingxi/desk/cron-jobs.json");
    let mut legacy = old_job("job_1");
    legacy["actorAgentId"] = json!("attacker");
    legacy["executionContext"] = json!({"kind":"legacy_agent_home","sourceSessionPath":null});
    let mut legacy2 = legacy.clone();
    legacy2["id"] = json!("job_2");
    write_json(&legacy_path, &json!({"jobs":[legacy,legacy2],"nextNum":3}));
    let before = std::fs::read(&legacy_path).unwrap();
    let report = import_current_studio(&h.api).await;
    assert_eq!((report.imported, report.blocked), (2, 0), "{report:?}");
    assert_eq!(
        report
            .entries
            .iter()
            .filter(|entry| entry.status == ImportStatus::SkippedLegacyCovered)
            .count(),
        1
    );
    let jobs = store::list_jobs(h.state.storage(), None).await.unwrap();
    assert_eq!(jobs.len(), 2);
    let legacy_job = jobs
        .iter()
        .find(|job| job.id.starts_with("legacy-job-"))
        .unwrap();
    assert_eq!(legacy_job.actor_id, "lingxi");
    let config: Value = serde_json::from_str(&legacy_job.config_json).unwrap();
    assert_eq!(
        config["legacyImport"]["rustR07"]["sourceJob"]["actorAgentId"],
        "attacker"
    );
    assert_eq!(std::fs::read(&legacy_path).unwrap(), before);
    assert_eq!(import_current_studio(&h.api).await.imported, 0);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn read_only_tmp_recovery_and_old_plugin_notifications_become_agent_prompts() {
    let h = Harness::open(None).await;
    let mut notify = old_job("notify");
    notify["prompt"] = json!("");
    notify["executor"] = json!({"kind":"direct_action","action":"notify","params":{"title":"合成通知","body":"合成正文","contextPolicy":"none"}});
    let mut plugin = old_job("plugin");
    plugin["prompt"] = json!("");
    plugin["executor"] = json!({"kind":"plugin_action","pluginId":"office","actionId":"html-to-pdf","params":{"input":"合成材料"}});
    let main = h.studio_desk().join("cron-jobs.json");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    std::fs::write(&main, "corrupt source preserved").unwrap();
    let tmp = PathBuf::from(format!("{}.tmp", main.display()));
    write_json(&tmp, &json!({"jobs":[notify,plugin]}));
    let main_before = std::fs::read(&main).unwrap();
    let tmp_before = std::fs::read(&tmp).unwrap();
    let report = import_current_studio(&h.api).await;
    assert_eq!((report.imported, report.blocked), (2, 0), "{report:?}");
    assert!(report.entries.iter().all(|entry| entry
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains(".tmp"))));
    for (id, expected) in [
        ("notify", "请调用 notify 工具"),
        ("plugin", "插件动作：office/html-to-pdf"),
    ] {
        let job = store::get_job(h.state.storage(), id)
            .await
            .unwrap()
            .unwrap();
        let config: Value = serde_json::from_str(&job.config_json).unwrap();
        assert_eq!(config["executor"]["kind"], "agent_session");
        assert!(config["prompt"].as_str().unwrap().contains(expected));
    }
    assert_eq!(std::fs::read(&main).unwrap(), main_before);
    assert_eq!(std::fs::read(&tmp).unwrap(), tmp_before);
    assert_eq!(h.state.storage().total_runs().await.unwrap(), 0);
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn atomic_import_competition_has_one_insert_and_preserves_legacy_revision() {
    let h = Harness::open(None).await;
    let input = store::AutomationJobInput {
        id: "stable".into(),
        studio_id: h.api.studio_id.clone(),
        actor_id: "lingxi".into(),
        owner_kind: "local_user".into(),
        owner_subject: "user_local".into(),
        schema_version: 4,
        config_json: json!({"type":"every","enabled":false}).to_string(),
        next_run_at: None,
    };
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let db = h.state.storage().clone();
        let input = input.clone();
        tasks.push(tokio::spawn(async move {
            raw_import::import_job(&db, &input, 27, 10, 20)
                .await
                .unwrap()
        }));
    }
    let mut inserted = 0;
    for task in tasks {
        let outcome = task.await.unwrap();
        inserted += usize::from(outcome.imported);
        assert_eq!(outcome.job.config_revision, 27);
        assert_eq!((outcome.job.created_at, outcome.job.updated_at), (10, 20));
    }
    assert_eq!(inserted, 1);
    store::delete_job(h.state.storage(), "stable", 27, 30)
        .await
        .unwrap();
    assert!(
        !raw_import::import_job(h.state.storage(), &input, 27, 10, 20)
            .await
            .unwrap()
            .imported
    );
    assert!(store::get_job(h.state.storage(), "stable")
        .await
        .unwrap()
        .is_none());
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_history_is_rejected_without_reading_or_modifying_the_target() {
    let h = Harness::open(None).await;
    h.write_studio(json!([old_job("linked")]));
    let target = h.home.join("private-sentinel");
    std::fs::write(&target, "private sentinel").unwrap();
    let link = h.studio_desk().join("cron-runs/linked.jsonl");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let report = import_current_studio(&h.api).await;
    assert_eq!((report.imported, report.blocked), (0, 1), "{report:?}");
    assert!(report.entries[0]
        .detail
        .as_deref()
        .unwrap()
        .contains("symlink"));
    assert_eq!(std::fs::read_to_string(target).unwrap(), "private sentinel");
    assert!(store::list_jobs(h.state.storage(), None)
        .await
        .unwrap()
        .is_empty());
    let root = h.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
