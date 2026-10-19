//! 用真实子进程证明临时打印的归属与未知清理；不把构造内存状态当作进程事实。
use super::{startup_tests::Fixture, *};
use lingxi_kernel::browser::{PdfAsset, PdfPrintSettings};

fn scope(fixture: &Fixture, id: &str) -> PdfScope {
    PdfScope {
        session_id: id.into(),
        owner_subject: "user_local".into(),
        execution_node: fixture.scope(id).execution_node,
    }
}
fn job() -> PdfRenderJob {
    PdfRenderJob {
        html: "<!doctype html><meta charset='utf-8'><p>真实打印生命周期</p>".into(),
        base_url: "https://lingxi-pdf.invalid/lifecycle/index.html".into(),
        assets: Vec::new(),
        settings: PdfPrintSettings {
            landscape: false,
            print_background: true,
            prefer_css_page_size: true,
            paper_width: 8.27,
            paper_height: 11.69,
            margin_top: 0.4,
            margin_bottom: 0.4,
            margin_left: 0.4,
            margin_right: 0.4,
            page_ranges: String::new(),
            scale: 1.0,
        },
        allow_javascript: false,
        viewport_width: 1280,
        viewport_height: 900,
        settle_ms: 0,
        timeout_ms: 60_000,
    }
}
fn pending(fixture: &Fixture) -> Arc<Live> {
    let pool = fixture.host.core.pool.lock().unwrap();
    assert!(pool.stopping);
    assert!(pool.entries.is_empty(), "不得补造业务workspace");
    assert_eq!(pool.pending_cleanup.len(), 1);
    assert_eq!(pool.pdf.len(), 1);
    assert_eq!(pool.auxiliary.len(), 1);
    assert_eq!(pool.aux, 0);
    let lease = pool.pdf.values().next().unwrap();
    assert!(!lease.busy);
    assert!(!lease.cleaned);
    let live = lease.live.as_ref().unwrap().clone();
    assert!(Arc::ptr_eq(&live, &pool.pending_cleanup[0]));
    live
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn pdf_initialization_unknown_retains_scope_process_and_profile_until_retry() {
    let fixture = Fixture::new();
    fixture
        .control
        .fail_live_initialize
        .store(true, Ordering::Release);
    fixture.control.unknown_stops.store(1, Ordering::Release);
    let source = scope(&fixture, "pdf-startup");
    let cause = fixture
        .host
        .render_pdf(source.clone(), job(), Arc::new(AtomicBool::new(false)))
        .await
        .unwrap_err();
    assert_eq!(cause.code, "browser_cleanup_unknown");
    assert!(cause.effect_unknown);
    let live = pending(&fixture);
    fixture.alive(&live);
    assert_eq!(fixture.host.pdf_scopes().unwrap(), vec![source.clone()]);
    let path = live.download_dir.parent().unwrap().to_path_buf();
    assert!(path.is_dir());
    fixture.host.cancel_pdf_scope(source).await.unwrap();
    fixture.assert_reaped();
    assert!(live.process.is_reaped());
    assert!(!path.exists());
    assert!(fixture.host.pdf_scopes().unwrap().is_empty());
    assert!(fixture.host.workspaces().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn pdf_cancel_startup_gap_unknown_keeps_late_owner_and_wrong_owner_cannot_clear_it() {
    let fixture = Fixture::new();
    fixture
        .control
        .pause_after_spawn
        .store(true, Ordering::Release);
    fixture.control.unknown_stops.store(1, Ordering::Release);
    let source = scope(&fixture, "pdf-startup-gap");
    let owned = source.clone();
    let host = fixture.host.clone();
    let task = tokio::spawn(async move {
        host.render_pdf(owned, job(), Arc::new(AtomicBool::new(false)))
            .await
    });
    fixture.wait_spawn_paused().await;
    assert_eq!(fixture.host.pdf_scopes().unwrap(), vec![source.clone()]);
    let cause = tokio::time::timeout(
        Duration::from_secs(6),
        fixture.host.cancel_pdf_scope(source.clone()),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(cause.code, "browser_cleanup_unknown");
    assert!(cause.effect_unknown);
    {
        let pool = fixture.host.core.pool.lock().unwrap();
        let lease = pool.pdf.values().next().unwrap();
        assert!(lease.busy);
        assert!(lease.live.is_none());
        assert!(lease.cancel.load(Ordering::Acquire));
    }
    fixture.control.release_spawn.store(true, Ordering::Release);
    assert!(task.await.unwrap().unwrap_err().effect_unknown);
    let live = pending(&fixture);
    fixture.alive(&live);
    let path = live.download_dir.parent().unwrap().to_path_buf();
    assert!(path.is_dir());
    let mut wrong = source.clone();
    wrong.owner_subject = "other-owner".into();
    fixture.host.cancel_pdf_scope(wrong).await.unwrap();
    assert_eq!(fixture.host.pdf_scopes().unwrap(), vec![source.clone()]);
    assert!(path.is_dir());
    assert!(!live.process.is_reaped());
    fixture.host.cancel_pdf_scope(source).await.unwrap();
    fixture.assert_reaped();
    assert!(!path.exists());
    assert!(fixture.host.pdf_scopes().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn invalid_pdf_or_wrong_node_never_spawns_or_reserves_a_lease() {
    let fixture = Fixture::new();
    let mut wrong = scope(&fixture, "wrong-node");
    wrong.execution_node = "some-other-service".into();
    assert_eq!(
        fixture
            .host
            .render_pdf(wrong, job(), Arc::new(AtomicBool::new(false)))
            .await
            .unwrap_err()
            .code,
        "browser_wrong_node"
    );
    let mut bad = job();
    bad.assets.push(PdfAsset {
        url: "file:///not-authorized/document".into(),
        mime_type: "text/plain".into(),
        bytes: Vec::new(),
    });
    assert_eq!(
        fixture
            .host
            .render_pdf(
                scope(&fixture, "bad-assets"),
                bad,
                Arc::new(AtomicBool::new(false))
            )
            .await
            .unwrap_err()
            .code,
        "PDF_INVALID_JOB"
    );
    assert!(fixture.control.spawned_pids.lock().unwrap().is_empty());
    assert!(fixture.host.pdf_scopes().unwrap().is_empty());
    assert!(fixture.host.workspaces().unwrap().is_empty());
    assert_eq!(fixture.host.core.pool.lock().unwrap().aux, 0);
}

#[tokio::test]
#[ignore = "需要真实Chromium；R07门禁显式运行"]
async fn browser_revoke_cancels_matching_pdf_without_conflating_run_and_workspace_epochs() {
    let fixture = Fixture::new();
    fixture.start("shared-session").await.unwrap();
    let source = scope(&fixture, "shared-session");
    let owned = source.clone();
    let host = fixture.host.clone();
    let mut source_job = job();
    source_job.html = "<script>while(true){}</script>".into();
    source_job.allow_javascript = true;
    let task = tokio::spawn(async move {
        host.render_pdf(owned, source_job, Arc::new(AtomicBool::new(false)))
            .await
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.host.pdf_scopes().unwrap().is_empty() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let mut revoked = fixture.scope("shared-session");
    revoked.epoch = 2;
    fixture.host.revoke(revoked).await.unwrap();
    assert!(task.await.unwrap().is_err());
    fixture.assert_reaped();
    assert!(fixture.host.pdf_scopes().unwrap().is_empty());
    let workspaces = fixture.host.workspaces().unwrap();
    assert_eq!(workspaces.len(), 1);
    assert!(workspaces[0].revoked);
    assert!(!workspaces[0].running);
    assert!(workspaces[0].tabs.is_empty());
    assert_eq!(
        std::fs::read_dir(fixture.root.join("aux")).unwrap().count(),
        0
    );
}
