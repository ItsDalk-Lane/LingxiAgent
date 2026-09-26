//! R02-T05 service-level acceptance tests (integration layer): real
//! storage, real hub, real concurrent writers — no mocked core.
//!
//! - **R02-A09 快照与订阅无空隙**: writers keep committing real runs
//!   through the real storage port while the test subscribes at every
//!   controlled commit boundary (fixed scheduling: single-thread runtime,
//!   yield-stepped advancement of the observed durable head, zero sleeps).
//!   The merged view (snapshot cut + live delivery) is compared
//!   event-by-event against the durable log read straight from storage:
//!   no missing key event, no duplicate, seq coverage strictly
//!   contiguous, boundary explicitly reported.
//! - **R02-A10 缓存过期可恢复**: a cursor whose events were truncated
//!   (real retention purge through the product surface) gets the explicit
//!   `snapshot_required` directive (not an empty stream, not a silent
//!   replay); rebuilding (snapshot + resubscribe + live) equals the
//!   current authoritative state. Negatives: stale streamId, future /
//!   forged cursor, malformed cursor, unknown stream, cross-principal.
//! - **WS surface**: the whole protocol rides the R02-T03 channel —
//!   ticket-authenticated upgrade, lingxi.wire handshake, control frames,
//!   live event push after an authenticated execute, connection staying
//!   open across `snapshot_required` for the resubscribe.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use lingxi_service::ws::WsFrame;
use lingxi_service::{
    events::{SubscribeOutcome, SubscribeReject},
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceError, ServiceState,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

use lingxi_service::events::{SubscriptionFrame, SubscriptionGuard};

// ── harness ────────────────────────────────────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lingxi-r02t05-svc-{tag}-{}", std::process::id()))
}

fn config_for(home: &std::path::Path) -> ServiceConfig {
    ServiceConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static addr"),
        data_home: home.to_path_buf(),
        home_source: HomeSource::Cli,
        network_mode: NetworkMode::Loopback,
        shutdown_timeout_ms: lingxi_service::DEFAULT_SHUTDOWN_TIMEOUT_MS,
    }
}

fn owner_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_local".to_string(),
        kind: lingxi_service::PrincipalKind::LocalUser,
        user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: None,
        credential_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
    }
}

fn foreign_principal() -> lingxi_service::Principal {
    lingxi_service::Principal {
        schema_version: 1,
        principal_id: "principal_device_user_remote_b".to_string(),
        kind: lingxi_service::PrincipalKind::Device,
        user_id: Some("user_remote_b".to_string()),
        studio_id: None,
        server_node_id: None,
        device_id: Some("device_b".to_string()),
        credential_id: Some("cred_b".to_string()),
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Lan,
        credential_kind: lingxi_service::CredentialKind::DeviceCredential,
        trust_state: lingxi_service::TrustState::Lan,
        scopes: vec!["chat".to_string()],
    }
}

async fn boot(tag: &str) -> (ServiceState, PathBuf) {
    let home = synthetic_home(tag);
    let _ = std::fs::remove_dir_all(&home);
    let layout = prepare_layout(&home).expect("layout");
    let state = ServiceState::bootstrap(config_for(&home), &layout)
        .await
        .expect("bootstrap");
    (state, home)
}

async fn teardown(state: &ServiceState, home: &std::path::Path) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

/// One real execute through the running state (real port + real hub).
async fn execute(state: &ServiceState, input: &str, now_ms: u64) -> String {
    let storage = Arc::clone(state.storage());
    state
        .sessions()
        .execute_for(
            storage.as_ref(),
            state.events(),
            &owner_principal(),
            "sess_local_alpha",
            input,
            now_ms,
        )
        .await
        .expect("execute")
        .run_id
}

use lingxi_kernel::ports::EventStorePort;

/// Full durable log of the stream straight from storage (the authority the
/// merged views are compared against).
async fn authority(state: &ServiceState, stream: &str) -> Vec<lingxi_protocol::EventEnvelope> {
    state
        .storage()
        .stream_events_after(stream, lingxi_protocol::Seq::new(0), 10_000)
        .await
        .expect("authority read")
}

// ── R02-A09: snapshot + subscription without a gap ──────────────────────────

/// Fixed-scheduling race matrix: subscribe at EVERY commit boundary k of
/// the run (k = observed durable head when the subscribe begins), with
/// real concurrent writers on a single-thread runtime (deterministic task
/// interleaving driven by explicit yields; no sleeps anywhere).
#[tokio::test(flavor = "current_thread")]
async fn a09_snapshot_and_subscription_have_no_gap_at_every_commit_boundary() {
    const WRITERS: usize = 2;
    const RUNS_PER_WRITER: usize = 2;
    const TOTAL_EVENTS: u64 = (WRITERS * RUNS_PER_WRITER * 2) as u64; // 2 events per run

    for k in 0..=TOTAL_EVENTS {
        let (state, home) = boot(&format!("a09-k{k}")).await;
        let state = Arc::new(state);
        let writer_state = Arc::clone(&state);
        let mut writers = Vec::new();
        for w in 0..WRITERS {
            let ws = Arc::clone(&writer_state);
            writers.push(tokio::spawn(async move {
                for r in 0..RUNS_PER_WRITER {
                    let run = execute(
                        &ws,
                        &format!("w{w}-r{r}"),
                        5_000 + (w as u64) * 10 + r as u64,
                    )
                    .await;
                    assert!(!run.is_empty());
                }
            }));
        }

        // Advance until the durable head reaches k (fixed scheduling: the
        // await points of the head read let the writer tasks run).
        //
        // now_ms must be unique per (writer, run): the session run id is
        // derived from (now_ms, total_runs+1) and the counter read is
        // racy under concurrency — equal timestamps make two concurrent
        // executes collide on one run id, which the T04 storage layer
        // correctly absorbs as an idempotent replay (one durable run, not
        // two). Distinct timestamps keep the writers writing DISTINCT
        // runs, which is what the 8-event total below assumes.
        while state
            .events()
            .stream_head("sess_local_alpha")
            .await
            .expect("head")
            .map(|s| s.value())
            .unwrap_or(0)
            < k
        {
            tokio::task::yield_now().await;
        }

        // THE RACE POINT: subscribe exactly at boundary k while writers
        // keep committing.
        let started = match state
            .events()
            .subscribe(&owner_principal(), "sess_local_alpha", None)
            .await
            .expect("subscribe at boundary {k}")
        {
            SubscribeOutcome::Started { cut, subscription } => (cut, subscription),
            SubscribeOutcome::RequiresSnapshot(required) => {
                panic!("fresh subscribe must never require a snapshot: {required:?}")
            }
        };
        let (cut, subscription) = started;
        let boundary = cut.snapshot_seq;
        assert!(
            boundary.value() >= k,
            "explicit boundary must cover the observed head (k={k}, got {boundary})"
        );
        // Boundary is exactly the last cut event's seq (explicit cut).
        if let Some(last) = cut.events.last() {
            assert_eq!(last.seq, boundary);
        }

        // Let the writers finish; every event is published before their
        // futures resolve.
        for writer in writers {
            writer.await.expect("writer finished");
        }

        // Drain the live delivery (all frames are already enqueued —
        // publication is synchronous with commit resolution).
        let mut live = Vec::new();
        loop {
            let mut progressed = false;
            while let Some(frame) = subscription.mailbox().try_recv() {
                progressed = true;
                match frame {
                    SubscriptionFrame::Event(envelope) => live.push(*envelope),
                    SubscriptionFrame::SnapshotRequired { reason } => {
                        panic!("no overflow at these sizes, got detach: {reason:?}")
                    }
                }
            }
            if !progressed {
                break;
            }
        }

        // Merge snapshot cut + live tail; overlap dedup must already be
        // server-side, so a duplicate seq here is a protocol violation.
        let mut merged: Vec<lingxi_protocol::EventEnvelope> = cut.events.clone();
        merged.extend(live.iter().cloned());
        let mut seen_seqs = std::collections::BTreeSet::new();
        for envelope in &merged {
            assert!(
                seen_seqs.insert(envelope.seq.value()),
                "duplicate seq {} in merged view (boundary {boundary})",
                envelope.seq
            );
        }
        let seqs: Vec<u64> = merged.iter().map(|e| e.seq.value()).collect();
        assert_eq!(
            seqs.first().copied(),
            Some(1),
            "merged view starts at seq 1 (k={k})"
        );
        assert!(
            seqs.windows(2).all(|w| w[0] + 1 == w[1]),
            "merged seqs must be contiguous (k={k}): {seqs:?}"
        );
        assert_eq!(
            seqs.last().copied(),
            Some(TOTAL_EVENTS),
            "merged view reaches the final head (k={k}): {seqs:?}"
        );

        // Per-event comparison against the authoritative durable log.
        let authority = authority(&state, "sess_local_alpha").await;
        assert_eq!(merged.len(), authority.len(), "count mismatch (k={k})");
        for (client, durable) in merged.iter().zip(authority.iter()) {
            assert_eq!(client, durable, "event mismatch (k={k})");
        }
        // Terminal facts come from storage only: every run_state_changed →
        // completed envelope equals its durable row (covered by the
        // equality above; spot-check the terminal pair explicitly).
        assert_eq!(
            merged
                .iter()
                .filter(|e| e.event_type == "run_state_changed")
                .count(),
            authority
                .iter()
                .filter(|e| e.event_type == "run_state_changed")
                .count(),
            "terminal transitions are exactly the durable ones"
        );

        drop(subscription);
        teardown(&state, &home).await;
    }
}

/// THE F01 REPAIR REGRESSION at the production join (multi_thread — the
/// production `#[tokio::main]` flavor; the deterministic structured-
/// scheduling counterpart of this window lives in the events.rs unit
/// tests). A real writer commits runs back-to-back through the real
/// storage port while the test subscribes back-to-back through the REAL
/// `EventService::subscribe`. Every publication that lands inside a
/// subscription's join window (register → durable page read →
/// release_hold) buffers in that subscriber's hold with a seq ABOVE the
/// cut the read is about to pin, and must be flushed at release_hold.
/// Before the repair the hold-advanced dedup watermark suppressed that
/// flush and silently lost the events (REVIEW-R1 F01 / probe A: no detach,
/// no snapshot_required, no dropped_deltas — the merged view just had a
/// hole). The invariant asserted here is the R02-A09 guarantee itself and
/// holds for EVERY interleaving after the repair: merged view
/// (snapshot cut + live tail) == the durable authority, contiguous,
/// duplicate-free, with the join seam exactly at snapshot_seq + 1.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a09_join_window_race_under_concurrent_writer_loses_nothing() {
    let (state, home) = boot("a09-join-race").await;
    let state = Arc::new(state);

    const SEED_EXECUTES: usize = 240; // 480 events: every raced page read scans ~480 rows (long read = wide window)
    const RACED_SUBSCRIBES: usize = 12; // +12 events = 492 < page_limit_max(500): never page-capped
    const TOTAL_EVENTS: u64 = (SEED_EXECUTES * 2 + RACED_SUBSCRIBES) as u64;

    for i in 0..SEED_EXECUTES {
        execute(&state, &format!("seed-{i}"), 1_000 + i as u64).await;
    }

    // A second INDEPENDENT connection to the same database file (WAL): its
    // commits do not queue behind the subscriber's page read on the
    // single-worker queue of the state's connection, so a commit can really
    // complete INSIDE the read's execution window. (Through the state's own
    // queue a post-snapshot commit's reply is always ordered after the
    // read's reply, giving release_hold a structural head start — the
    // window is real but un-hittable through one connection, which is why
    // the REVIEW-R1 stress runs measured 0 holes uncapped.)
    let db_path = home.join("lingxi-service").join("data").join("runs.db");
    let raced_storage = Arc::new(
        lingxi_adapters::storage::RunDatabase::open(
            &db_path,
            lingxi_adapters::storage::StoreOptions::default(),
        )
        .await
        .expect("second storage connection"),
    );

    // STRUCTURED SCHEDULING POINT (no sleeps, no timers): for every
    // subscribe, a watcher spins on `hub.subscriber_stats(expected_id)`
    // with `yield_now` — the id comes into existence at EXACTLY the
    // register(hold) step of `EventService::subscribe` — and fires a real
    // commit on the second connection the moment it appears. The writer's
    // commit therefore races the subscribe's durable page read by
    // construction (causally tied to the registration, not to timing
    // luck): every event committed after the read's snapshot and published
    // before release_hold buffers in the hold with a seq ABOVE the cut and
    // must be flushed at the join. Before the REVIEW-R1 F01 repair the
    // hold-advanced dedup watermark suppressed that flush and silently
    // dropped the event (no detach, no directive, no loss counter).
    let mut writer_handles = Vec::new();
    let mut subs: Vec<(u64, Vec<lingxi_protocol::EventEnvelope>, SubscriptionGuard)> = Vec::new();
    for k in 0..RACED_SUBSCRIBES {
        let (fire_tx, fire_rx) = tokio::sync::oneshot::channel::<()>();
        {
            let hub = Arc::clone(state.events().hub());
            tokio::spawn(async move {
                loop {
                    if hub.subscriber_stats(k as u64).is_some() {
                        let _ = fire_tx.send(());
                        return;
                    }
                    tokio::task::yield_now().await;
                }
            });
        }
        {
            let storage2 = Arc::clone(&raced_storage);
            let events = Arc::clone(state.events());
            writer_handles.push(tokio::spawn(async move {
                let _ = fire_rx.await;
                let ctx = lingxi_kernel::RunContext {
                    principal: lingxi_kernel::Principal::LocalUser,
                    session_id: lingxi_protocol::SessionId::new("sess_local_alpha".to_string()),
                    run_id: lingxi_protocol::RunId::new(format!("run_joinrace_{k}")),
                    attempt: lingxi_protocol::AttemptId::new(format!("run_joinrace_{k}#a1")),
                    generation: 1,
                };
                let committed = lingxi_kernel::ports::StoragePort::record_run_started(
                    storage2.as_ref(),
                    &ctx,
                    900_000u64 + k as u64,
                )
                .await
                .expect("raced commit");
                // Publication strictly after the commit returned Ok (the
                // production contract; the test plays the writer role).
                events.publish_committed(&committed.events);
            }));
        }

        match state
            .events()
            .subscribe(&owner_principal(), "sess_local_alpha", None)
            .await
            .expect("subscribe answers")
        {
            SubscribeOutcome::Started { cut, subscription } => {
                assert_eq!(
                    subscription.subscriber_id(),
                    k as u64,
                    "subscriber id mapping (the watcher keys on it)"
                );
                assert!(
                    cut.next_cursor.is_none(),
                    "page cap must not fire (total events < 500)"
                );
                subs.push((cut.snapshot_seq.value(), cut.events, subscription));
            }
            SubscribeOutcome::RequiresSnapshot(required) => {
                panic!("fresh subscribe must never require a snapshot: {required:?}")
            }
        }
    }
    for writer in writer_handles {
        writer.await.expect("raced writer finished");
    }

    // All publications are synchronous with the writers' commit
    // resolutions, so every frame is already enqueued.
    let final_head = state
        .events()
        .stream_head("sess_local_alpha")
        .await
        .expect("head")
        .expect("non-empty")
        .value();
    assert_eq!(final_head, TOTAL_EVENTS);
    let authority = authority(&state, "sess_local_alpha").await;
    assert_eq!(authority.len(), TOTAL_EVENTS as usize);

    // THE INVARIANT (R02-A09): a subscription either joins AIRTIGHT (live
    // tail starts exactly at snapshot_seq+1, runs contiguously to the
    // final head, merged view == the durable authority event-by-event) or
    // it was told LOUDLY (snapshot_required detach). A hole without a
    // detach signal is the silent loss the repair removes.
    for (index, (snapshot_seq, cut_events, subscription)) in subs.iter().enumerate() {
        let mut live: Vec<lingxi_protocol::EventEnvelope> = Vec::new();
        let mut detached = false;
        while let Some(frame) = subscription.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(envelope) => {
                    assert!(
                        !detached,
                        "sub {index}: event delivered after the detach signal"
                    );
                    live.push(*envelope);
                }
                SubscriptionFrame::SnapshotRequired { .. } => detached = true,
            }
        }
        if detached {
            // Loud end: the client rebuilds from a snapshot. The tail up to
            // the detach must still have joined seamlessly.
            for w in live.windows(2) {
                assert_eq!(
                    w[1].seq.value(),
                    w[0].seq.value() + 1,
                    "sub {index}: hole inside the live tail before the detach"
                );
            }
            if let Some(first) = live.first() {
                assert_eq!(
                    first.seq.value(),
                    snapshot_seq + 1,
                    "sub {index}: join hole at the seam before the detach (cut={snapshot_seq})"
                );
            }
            continue;
        }
        match live.first() {
            Some(first) => assert_eq!(
                first.seq.value(),
                snapshot_seq + 1,
                "sub {index}: SILENT join hole at the seam (cut={snapshot_seq})"
            ),
            None => assert_eq!(
                *snapshot_seq, final_head,
                "sub {index}: silent stall before the final head with no live tail"
            ),
        }
        for w in live.windows(2) {
            assert_eq!(
                w[1].seq.value(),
                w[0].seq.value() + 1,
                "sub {index}: SILENT hole inside the live tail"
            );
        }
        // The merged view is the authority: no missing key event, no
        // duplicate, event-by-event equal.
        let mut merged: Vec<lingxi_protocol::EventEnvelope> = cut_events.clone();
        merged.extend(live);
        let seqs: Vec<u64> = merged.iter().map(|e| e.seq.value()).collect();
        let expected: Vec<u64> = (1..=TOTAL_EVENTS).collect();
        assert_eq!(
            seqs, expected,
            "sub {index}: merged view must be exactly 1..={TOTAL_EVENTS} (cut={snapshot_seq})"
        );
        for (client, durable) in merged.iter().zip(authority.iter()) {
            assert_eq!(client, durable, "sub {index}: event mismatch vs authority");
        }
    }

    drop(subs);
    teardown(&state, &home).await;
}

/// Two simultaneous subscribers see the same gap-free merged views (all
/// clients consume the same business facts).
#[tokio::test(flavor = "current_thread")]
async fn a09_parallel_subscribers_see_identical_views() {
    let (state, home) = boot("a09-parallel").await;
    let state = Arc::new(state);
    for _ in 0..3 {
        execute(&state, "pre", 1).await;
    }
    let mut subs = Vec::new();
    for _ in 0..2 {
        match state
            .events()
            .subscribe(&owner_principal(), "sess_local_alpha", None)
            .await
            .expect("subscribe")
        {
            SubscribeOutcome::Started { cut, subscription } => subs.push((cut, subscription)),
            SubscribeOutcome::RequiresSnapshot(r) => panic!("unexpected {r:?}"),
        }
    }
    for i in 0..3 {
        execute(&state, "live", 10 + i).await;
    }
    let authority = authority(&state, "sess_local_alpha").await;
    for (index, (cut, subscription)) in subs.iter().enumerate() {
        let mut live = Vec::new();
        while let Some(frame) = subscription.mailbox().try_recv() {
            match frame {
                SubscriptionFrame::Event(e) => live.push(*e),
                SubscriptionFrame::SnapshotRequired { reason } => {
                    panic!("sub {index} detached: {reason:?}")
                }
            }
        }
        let mut merged = cut.events.clone();
        merged.extend(live);
        assert_eq!(
            merged, authority,
            "subscriber {index} view must equal the authority"
        );
    }
    teardown(&state, &home).await;
}

// ── R02-A10: expired cursor recovery + negatives ────────────────────────────

#[tokio::test]
async fn a10_expired_cursor_gets_explicit_snapshot_required_and_rebuild_matches_authority() {
    let (state, home) = boot("a10-expired").await;
    // 5 runs → 10 durable events, seqs 1..=10.
    for i in 0..5 {
        execute(&state, "seed", 100 + i).await;
    }
    let head = state
        .events()
        .stream_head("sess_local_alpha")
        .await
        .expect("head")
        .expect("non-empty");
    assert_eq!(head.value(), 10);

    // A real page with a small limit issues a REAL cursor at seq 4.
    let page = state
        .events()
        .events_page(&owner_principal(), "sess_local_alpha", None, Some(4))
        .await
        .expect("page");
    assert_eq!(page.events.len(), 4);
    assert_eq!(page.snapshot_seq.value(), 4);
    let old_cursor = page.next_cursor.expect("next cursor issued");

    // Retention actually truncates: everything before seq 7 is gone.
    let removed = state
        .events()
        .purge_events_before("sess_local_alpha", lingxi_protocol::Seq::new(7))
        .await
        .expect("purge");
    assert_eq!(removed, 6);

    // Resume with the now-expired cursor: the EXPLICIT directive — not an
    // empty stream, not a silent replay.
    match state
        .events()
        .subscribe(
            &owner_principal(),
            "sess_local_alpha",
            Some(old_cursor.clone()),
        )
        .await
        .expect("subscribe must answer")
    {
        SubscribeOutcome::RequiresSnapshot(required) => {
            assert_eq!(required.reason, "events_truncated");
            assert_eq!(required.floor, Some(lingxi_protocol::Seq::new(7)));
            assert_eq!(required.stream_id, "sess_local_alpha");
        }
        SubscribeOutcome::Started { .. } => {
            panic!("expired cursor must NOT silently start a subscription")
        }
    }
    // The HTTP face maps the same condition to cursor_expired.
    match state
        .events()
        .events_page(
            &owner_principal(),
            "sess_local_alpha",
            Some(old_cursor),
            Some(10),
        )
        .await
    {
        Err(lingxi_service::events::SubscribePageError::CursorExpired(required)) => {
            assert_eq!(required.floor, Some(lingxi_protocol::Seq::new(7)));
        }
        other => panic!("HTTP page must map expired cursor, got {other:?}"),
    }

    // Rebuild: fresh snapshot (starts at the retention floor) + live
    // continuation of NEW commits.
    let (cut, subscription) = match state
        .events()
        .subscribe(&owner_principal(), "sess_local_alpha", None)
        .await
        .expect("rebuild subscribe")
    {
        SubscribeOutcome::Started { cut, subscription } => (cut, subscription),
        SubscribeOutcome::RequiresSnapshot(r) => panic!("rebuild must start: {r:?}"),
    };
    assert_eq!(cut.events.first().map(|e| e.seq.value()), Some(7));
    execute(&state, "after-rebuild", 200).await;
    let mut live = Vec::new();
    while let Some(frame) = subscription.mailbox().try_recv() {
        match frame {
            SubscriptionFrame::Event(e) => live.push(*e),
            SubscriptionFrame::SnapshotRequired { reason } => panic!("detach {reason:?}"),
        }
    }
    let mut rebuilt = cut.events.clone();
    rebuilt.extend(live);
    // The rebuilt view equals the CURRENT authority (post-truncation
    // storage): recovery to the authoritative state, with the truncation
    // loss confined to what retention actually removed.
    let authority = authority(&state, "sess_local_alpha").await;
    assert_eq!(rebuilt, authority);
    assert_eq!(
        rebuilt.first().map(|e| e.seq.value()),
        Some(7),
        "rebuild starts at the floor"
    );
    assert_eq!(rebuilt.len(), 6, "7..=12 after one post-rebuild run");
    assert!(
        !rebuilt.is_empty(),
        "never an empty stream for the rebuild answer"
    );
    drop(subscription);
    teardown(&state, &home).await;
}

#[tokio::test]
async fn a10_negative_rejections_are_loud() {
    let (state, home) = boot("a10-negative").await;
    for i in 0..3 {
        execute(&state, "seed", 300 + i).await;
    }
    let head = state
        .events()
        .stream_head("sess_local_alpha")
        .await
        .expect("head")
        .expect("non-empty");
    let events = state.events();

    // Future (forged with a valid checksum) cursor: rejected.
    let future = lingxi_service::events::SubscribeCursor {
        stream_id: "sess_local_alpha".to_string(),
        seq: lingxi_protocol::Seq::new(head.value() + 5),
    }
    .encode();
    match events
        .subscribe(&owner_principal(), "sess_local_alpha", Some(future))
        .await
    {
        Err(SubscribeReject::FutureCursor { seq, head, .. }) => {
            assert_eq!(seq.value(), head.value() + 5);
        }
        other => panic!("future cursor must be rejected, got {other:?}"),
    }

    // Malformed cursor: rejected with the machine reason.
    match events
        .subscribe(
            &owner_principal(),
            "sess_local_alpha",
            Some(lingxi_protocol::Cursor::new("garbage!!")),
        )
        .await
    {
        Err(SubscribeReject::MalformedCursor { .. }) => {}
        other => panic!("malformed cursor must be rejected, got {other:?}"),
    }

    // Cursor of another stream used on this stream: stale-stream rejection.
    let foreign_cursor = lingxi_service::events::SubscribeCursor {
        stream_id: "sess_local_beta".to_string(),
        seq: lingxi_protocol::Seq::new(1),
    }
    .encode();
    match events
        .subscribe(&owner_principal(), "sess_local_alpha", Some(foreign_cursor))
        .await
    {
        Err(SubscribeReject::StaleStreamCursor {
            stream_id,
            cursor_stream,
        }) => {
            assert_eq!(stream_id, "sess_local_alpha");
            assert_eq!(cursor_stream, "sess_local_beta");
        }
        other => panic!("stale stream cursor must be rejected, got {other:?}"),
    }

    // Stale (unknown) stream id: not found, never an empty live stream.
    match events
        .subscribe(&owner_principal(), "stream_never_existed", None)
        .await
    {
        Err(SubscribeReject::StreamNotFound { stream_id }) => {
            assert_eq!(stream_id, "stream_never_existed");
        }
        other => panic!("unknown stream must be rejected, got {other:?}"),
    }

    // Cross-principal: forbidden through the same ownership rule.
    match events
        .subscribe(&foreign_principal(), "sess_local_alpha", None)
        .await
    {
        Err(SubscribeReject::Forbidden { .. }) => {}
        other => panic!("cross-principal subscribe must be forbidden, got {other:?}"),
    }

    teardown(&state, &home).await;
}

/// REVIEW-R1 F03 repair regression: after retention purges the stream
/// EMPTY, a stale-but-valid cursor is the A10 rebuild directive (floor =
/// None — nothing is retained), not a "future cursor" rejection. Both
/// faces use the same vocabulary (WS/service = RequiresSnapshot, HTTP =
/// 409 cursor_expired), and a forged cursor on the emptied stream gets the
/// same loud directive. The rebuild on the emptied stream is an explicit
/// EMPTY cut — the truthful state, never a rejection, never silence.
#[tokio::test]
async fn a10_purge_all_stale_cursor_gets_rebuild_directive_not_future_cursor() {
    let (state, home) = boot("a10-purge-all").await;
    for i in 0..2 {
        execute(&state, "seed", 400 + i).await;
    }
    let events = state.events();

    // A real page issues a REAL cursor into the events that are about to
    // be purged (cursor pins seq 1).
    let page = events
        .events_page(&owner_principal(), "sess_local_alpha", None, Some(1))
        .await
        .expect("page");
    assert_eq!(page.snapshot_seq.value(), 1);
    let stale_cursor = page.next_cursor.expect("next cursor issued");

    // Purge EVERYTHING through the product surface (before_seq beyond the
    // head): the stream collapses to no floor and no head.
    let removed = events
        .purge_events_before("sess_local_alpha", lingxi_protocol::Seq::new(999))
        .await
        .expect("purge");
    assert_eq!(removed, 4);
    assert_eq!(
        events.stream_head("sess_local_alpha").await.expect("head"),
        None,
        "purge-all must collapse the head"
    );

    // Stale cursor over the service face: the rebuild directive with NO
    // floor — before the repair this came back as FutureCursor (4409),
    // sending clients down the wrong recovery path.
    match events
        .subscribe(
            &owner_principal(),
            "sess_local_alpha",
            Some(stale_cursor.clone()),
        )
        .await
        .expect("subscribe must answer")
    {
        SubscribeOutcome::RequiresSnapshot(required) => {
            assert_eq!(required.reason, "events_truncated");
            assert_eq!(required.floor, None, "nothing is retained: no floor");
            assert_eq!(required.stream_id, "sess_local_alpha");
        }
        other => panic!("purge-all stale cursor must be a rebuild directive, got {other:?}"),
    }
    // HTTP face: the same condition maps to 409 cursor_expired.
    match events
        .events_page(
            &owner_principal(),
            "sess_local_alpha",
            Some(stale_cursor),
            Some(10),
        )
        .await
    {
        Err(lingxi_service::events::SubscribePageError::CursorExpired(required)) => {
            assert_eq!(required.floor, None);
        }
        other => panic!("HTTP page must map the purge-all stale cursor, got {other:?}"),
    }

    // A forged future cursor on the emptied stream gets the SAME loud
    // vocabulary (it cannot be honored either; the client rebuilds).
    let forged = lingxi_service::events::SubscribeCursor {
        stream_id: "sess_local_alpha".to_string(),
        seq: lingxi_protocol::Seq::new(42),
    }
    .encode();
    match events
        .subscribe(&owner_principal(), "sess_local_alpha", Some(forged))
        .await
        .expect("subscribe must answer")
    {
        SubscribeOutcome::RequiresSnapshot(required) => {
            assert_eq!(required.floor, None);
        }
        other => panic!("forged cursor on an emptied stream must be loud, got {other:?}"),
    }

    // Rebuild on the emptied stream: an explicit EMPTY cut (mode snapshot,
    // boundary 0) — the truthful state, never a rejection.
    match events
        .subscribe(&owner_principal(), "sess_local_alpha", None)
        .await
        .expect("rebuild subscribe")
    {
        SubscribeOutcome::Started { cut, subscription } => {
            assert!(cut.events.is_empty(), "the stream holds no events");
            assert_eq!(cut.snapshot_seq.value(), 0);
            assert_eq!(cut.next_cursor, None);
            drop(subscription);
        }
        other => panic!("rebuild on an emptied stream must start, got {other:?}"),
    }

    teardown(&state, &home).await;
}

/// Duplicate publication (idempotent replay path) delivers each event
/// exactly once to subscribers — the server-side half of eventId/seq
/// dedup (R02-A10 negative).
#[tokio::test]
async fn a10_duplicate_event_publication_is_deduped() {
    let (state, home) = boot("a10-dedup").await;
    execute(&state, "seed", 1).await;
    let (cut, subscription) = match state
        .events()
        .subscribe(&owner_principal(), "sess_local_alpha", None)
        .await
        .expect("subscribe")
    {
        SubscribeOutcome::Started { cut, subscription } => (cut, subscription),
        SubscribeOutcome::RequiresSnapshot(r) => panic!("{r:?}"),
    };
    // Replay the committed envelopes through the publication entry again
    // (what an idempotent commit replay hands over).
    let envelopes = authority(&state, "sess_local_alpha").await;
    state.events().publish_committed(&envelopes);
    state.events().publish_committed(&envelopes);
    let mut live = Vec::new();
    while let Some(frame) = subscription.mailbox().try_recv() {
        match frame {
            SubscriptionFrame::Event(e) => live.push(*e),
            SubscriptionFrame::SnapshotRequired { reason } => panic!("{reason:?}"),
        }
    }
    assert!(
        live.is_empty(),
        "duplicate publication must not re-deliver (cut boundary {})",
        cut.snapshot_seq
    );
    let stats = state
        .events()
        .hub()
        .subscriber_stats(subscription.subscriber_id())
        .expect("still subscribed");
    assert_eq!(stats.detached, None);
    drop(subscription);
    teardown(&state, &home).await;
}

// ── WS surface (R02-T03 channel + R02-T05 protocol) ─────────────────────────

struct TestServer {
    addr: SocketAddr,
    state: Arc<ServiceState>,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    handle: tokio::task::JoinHandle<Result<(), ServiceError>>,
    home: PathBuf,
}

async fn start_ws_server(tag: &str) -> TestServer {
    let (state, home) = boot(tag).await;
    let state = Arc::new(state);
    let token = state.auth().local_token().to_string();
    let config = state.config().clone();
    let serve_state = Arc::clone(&state);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
    let handle = tokio::spawn(async move {
        run(
            (*serve_state).clone(),
            async {
                let _ = stop_rx.await;
            },
            |addr| {
                let _ = ready_tx.send(addr);
            },
        )
        .await
    });
    let _ = config;
    let addr = ready_rx.await.expect("readiness");
    TestServer {
        addr,
        state,
        token,
        stop: stop_tx,
        handle,
        home,
    }
}

async fn stop_server(server: TestServer) {
    server.stop.send(()).expect("server still listening");
    tokio::time::timeout(std::time::Duration::from_secs(10), server.handle)
        .await
        .expect("server shuts down")
        .expect("join")
        .expect("clean serve");
    server.state.storage().close().await.expect("close");
    let _ = std::fs::remove_dir_all(&server.home);
}

/// Raw HTTP (kept minimal: the WS + page flows under test need GET/POST
/// with a bearer header and query strings).
async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    bearer: &str,
    body: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let payload = body.unwrap_or("").as_bytes();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {bearer}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write http");
    stream.write_all(payload).await.expect("write body");
    let mut out = Vec::new();
    stream.read_to_end(&mut out).await.expect("read http");
    let text = String::from_utf8_lossy(&out).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

fn client_ws_key() -> String {
    use base64::Engine as _;
    let mut bytes = [0u8; 16];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for (i, slot) in bytes.iter_mut().enumerate() {
        *slot = (((nanos >> (i * 4)) & 0xff) as u8).wrapping_add((i as u8).wrapping_mul(31));
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

async fn ws_upgrade_bearer(addr: SocketAddr, bearer: &str) -> TcpStream {
    let mut stream = TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    let key = client_ws_key();
    let head = format!(
        "GET /lingxi/v1/ws HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\n\
         Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\
         Authorization: Bearer {bearer}\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).await.expect("upgrade");
    let mut seen = Vec::new();
    let mut byte = [0u8; 1];
    while stream.read_exact(&mut byte).await.is_ok() {
        seen.push(byte[0]);
        if seen.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    assert!(
        seen.starts_with(b"HTTP/1.1 101"),
        "upgrade failed: {}",
        String::from_utf8_lossy(&seen)
    );
    stream
}

async fn client_ws_send(stream: &mut TcpStream, payload: &[u8]) {
    let mask: [u8; 4] = [0x11, 0x22, 0x33, 0x44];
    let mut out = vec![0x81];
    if payload.len() < 126 {
        out.push(0x80 | payload.len() as u8);
    } else {
        out.push(0x80 | 126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    let masked: Vec<u8> = payload
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ mask[i % 4])
        .collect();
    out.extend_from_slice(&masked);
    stream.write_all(&out).await.expect("write client frame");
}

async fn client_ws_read(stream: &mut TcpStream) -> WsFrame {
    lingxi_service::ws::read_ws_frame(stream)
        .await
        .expect("read server frame")
        .expect("server frame")
}

fn frame_text(frame: &WsFrame) -> String {
    match frame {
        WsFrame::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        other => panic!("expected text frame, got {other:?}"),
    }
}

/// Upgraded + lingxi.wire-handshaken connection.
async fn ws_connect(server: &TestServer) -> TcpStream {
    let mut stream = ws_upgrade_bearer(server.addr, &server.token).await;
    let hello = serde_json::json!({
        "protocol": "lingxi.wire",
        "clientKind": "test",
        "clientVersion": "0",
        "protocolMin": 1,
        "protocolMax": 1,
    });
    client_ws_send(&mut stream, hello.to_string().as_bytes()).await;
    let hello_back = frame_text(&client_ws_read(&mut stream).await);
    assert!(
        hello_back.contains("lingxi.wire"),
        "server hello {hello_back}"
    );
    stream
}

fn subscribe_frame(stream_id: &str, cursor: Option<&str>) -> String {
    let mut value = serde_json::json!({"type": "subscribe_events", "streamId": stream_id});
    if let Some(cursor) = cursor {
        value["cursor"] = serde_json::Value::String(cursor.to_string());
    }
    value.to_string()
}

/// Reads frames until `n` event envelopes arrived (control frames are
/// returned too, in order).
async fn read_until_events(
    stream: &mut TcpStream,
    n: usize,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let mut controls = Vec::new();
    let mut events = Vec::new();
    while events.len() < n {
        let text = frame_text(&client_ws_read(stream).await);
        let value: serde_json::Value = serde_json::from_str(&text).expect("frame json");
        if value["frameKind"] == "control" {
            controls.push(value);
        } else {
            assert!(
                value.get("eventId").is_some() && value.get("seq").is_some(),
                "event frame must be a bare envelope: {text}"
            );
            events.push(value);
        }
    }
    (controls, events)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_subscribe_snapshot_live_and_rejections() {
    let server = start_ws_server("ws-events").await;
    // Seed 2 runs → 4 events.
    for i in 0..2 {
        let (status, body) = http(
            server.addr,
            "POST",
            "/lingxi/v1/sessions/sess_local_alpha/execute",
            &server.token,
            Some(&format!(r#"{{"input":"seed {i}"}}"#)),
        )
        .await;
        assert_eq!(status, 200, "execute failed: {body}");
    }

    // 1. Subscribe without a cursor: control frame with the explicit
    //    boundary, then the snapshot cut as bare envelopes.
    let mut stream = ws_connect(&server).await;
    client_ws_send(
        &mut stream,
        subscribe_frame("sess_local_alpha", None).as_bytes(),
    )
    .await;
    let (controls, events) = read_until_events(&mut stream, 4).await;
    assert_eq!(controls.len(), 1, "exactly one subscribed control frame");
    assert_eq!(controls[0]["type"], "subscribed");
    assert_eq!(controls[0]["mode"], "snapshot");
    assert_eq!(controls[0]["snapshotSeq"], serde_json::json!("4"));
    assert_eq!(events.len(), 4);
    assert_eq!(events[0]["seq"], serde_json::json!("1"));
    assert_eq!(events[3]["seq"], serde_json::json!("4"));

    // 2. Authenticated execute pushes the new events live.
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/sessions/sess_local_alpha/execute",
        &server.token,
        Some(r#"{"input":"live"}"#),
    )
    .await;
    assert_eq!(status, 200, "live execute failed: {body}");
    let (controls, events) = read_until_events(&mut stream, 2).await;
    assert!(controls.is_empty(), "no control frames mid-stream");
    assert_eq!(events[0]["seq"], serde_json::json!("5"));
    assert_eq!(events[1]["seq"], serde_json::json!("6"));

    // 3. Duplicate subscribe on the same connection: rejected + closed.
    client_ws_send(
        &mut stream,
        subscribe_frame("sess_local_alpha", None).as_bytes(),
    )
    .await;
    let error_text = frame_text(&client_ws_read(&mut stream).await);
    assert!(error_text.contains("already_subscribed"), "{error_text}");
    match client_ws_read(&mut stream).await {
        WsFrame::Close(4409, _) => {}
        other => panic!("expected close 4409, got {other:?}"),
    }
    drop(stream);

    // 4. Future cursor on a fresh connection: invalid_message close.
    let mut stream = ws_connect(&server).await;
    let future = lingxi_service::events::SubscribeCursor {
        stream_id: "sess_local_alpha".to_string(),
        seq: lingxi_protocol::Seq::new(999),
    }
    .encode();
    client_ws_send(
        &mut stream,
        subscribe_frame("sess_local_alpha", Some(future.as_str())).as_bytes(),
    )
    .await;
    let error_text = frame_text(&client_ws_read(&mut stream).await);
    assert!(error_text.contains("future_cursor"), "{error_text}");
    match client_ws_read(&mut stream).await {
        WsFrame::Close(4409, _) => {}
        other => panic!("expected close 4409, got {other:?}"),
    }
    drop(stream);

    // 5. Unknown stream: not_found close.
    let mut stream = ws_connect(&server).await;
    client_ws_send(
        &mut stream,
        subscribe_frame("stream_unknown", None).as_bytes(),
    )
    .await;
    let error_text = frame_text(&client_ws_read(&mut stream).await);
    assert!(error_text.contains("stream_not_found"), "{error_text}");
    match client_ws_read(&mut stream).await {
        WsFrame::Close(4404, _) => {}
        other => panic!("expected close 4404, got {other:?}"),
    }
    drop(stream);

    // 6. Expired cursor over WS: the snapshot_required control frame keeps
    //    the connection OPEN for the resubscribe (proven by a follow-up
    //    session_read answer), then the resubscribe delivers the rebuilt
    //    cut from the retention floor.
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/events?limit=2",
        &server.token,
        None,
    )
    .await;
    assert_eq!(status, 200, "page failed: {body}");
    let page: serde_json::Value = serde_json::from_str(&body).expect("page json");
    let old_cursor = page["nextCursor"].as_str().expect("cursor").to_string();
    server
        .state
        .events()
        .purge_events_before("sess_local_alpha", lingxi_protocol::Seq::new(4))
        .await
        .expect("purge");
    let mut stream = ws_connect(&server).await;
    client_ws_send(
        &mut stream,
        subscribe_frame("sess_local_alpha", Some(old_cursor.as_str())).as_bytes(),
    )
    .await;
    let text = frame_text(&client_ws_read(&mut stream).await);
    let directive: serde_json::Value = serde_json::from_str(&text).expect("directive json");
    assert_eq!(directive["frameKind"], "control");
    assert_eq!(directive["type"], "snapshot_required");
    assert_eq!(directive["floorSeq"], serde_json::json!("4"));
    assert_eq!(directive["reason"], "events_truncated");
    // Connection still alive: a session_read on the same socket answers.
    client_ws_send(
        &mut stream,
        serde_json::json!({"type": "session_read", "sessionId": "sess_local_alpha"})
            .to_string()
            .as_bytes(),
    )
    .await;
    let answer = frame_text(&client_ws_read(&mut stream).await);
    assert!(
        answer.contains("runCount"),
        "socket still serving: {answer}"
    );
    // Resubscribe after the rebuild: fresh cut starts at the floor. The
    // retained log here is seqs 4,5,6 (six committed events, the purge
    // removed 1..=3) — exactly three envelopes in the rebuilt cut.
    client_ws_send(
        &mut stream,
        subscribe_frame("sess_local_alpha", None).as_bytes(),
    )
    .await;
    let (controls, events) = read_until_events(&mut stream, 3).await;
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0]["type"], "subscribed");
    assert_eq!(controls[0]["snapshotSeq"], serde_json::json!("6"));
    let seqs: Vec<&serde_json::Value> = events.iter().map(|e| &e["seq"]).collect();
    assert_eq!(seqs, vec!["4", "5", "6"], "rebuilt cut = floor..head");
    drop(stream);

    // 7. HTTP page endpoint: expired cursor → 409 cursor_expired with the
    //    machine directive in details.
    let (status, body) = http(
        server.addr,
        "GET",
        &format!(
            "/lingxi/v1/sessions/sess_local_alpha/events?cursor={}",
            url_encoding_of(&old_cursor)
        ),
        &server.token,
        None,
    )
    .await;
    assert_eq!(status, 409, "expired cursor page: {body}");
    let error: serde_json::Value = serde_json::from_str(&body).expect("error json");
    assert_eq!(error["code"], "cursor_expired");
    assert_eq!(error["details"]["reason"], "snapshot_required");
    assert_eq!(error["details"]["floorSeq"], serde_json::json!("4"));

    // 8. Cross-principal page read: 403 through the same ownership chain.
    //    (Device credential minted through the local-only management
    //    surface, then used over HTTP.)
    let (status, body) = http(
        server.addr,
        "POST",
        "/lingxi/v1/devices/credentials",
        &server.token,
        Some(r#"{"userId":"user_remote_b","scopes":["chat"]}"#),
    )
    .await;
    assert_eq!(status, 201, "credential issue failed: {body}");
    let issued: serde_json::Value = serde_json::from_str(&body).expect("issued json");
    let secret = issued["secret"].as_str().expect("secret").to_string();
    let (status, body) = http(
        server.addr,
        "GET",
        "/lingxi/v1/sessions/sess_local_alpha/events",
        &secret,
        None,
    )
    .await;
    assert_eq!(
        status, 403,
        "cross-principal page must be forbidden: {body}"
    );

    stop_server(server).await;
}

/// Minimal percent-encoding for the cursor in a query string
/// (base64url alphabet is safe except '='; NO_PAD avoids it entirely, but
/// be defensive anyway).
fn url_encoding_of(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
