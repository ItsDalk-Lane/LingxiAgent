//! R03-A02 (property half): duplicate and out-of-order conflicting
//! settlements against ONE run settle EXACTLY ONCE — through the REAL
//! `RunDatabase` finalize transaction (the same single finalize path the
//! service run driver uses via `commit_run_outcome`).
//!
//! Deterministic property loop (fixed xorshift seed, part of the test's
//! identity): every iteration builds a random arrival order of settlement
//! submissions — identical duplicates of the eventual winner interleaved
//! with conflicting settlements (different final messages, different
//! reasons, different statuses) — and asserts, by RESULT COUNTING:
//!
//! - exactly ONE `newly_committed == true` across all submissions;
//! - every later identical submission is an `Ok(newly_committed == false)`
//!   idempotent replay that writes NOTHING;
//! - every conflicting submission is a diagnosed `StorageError::Conflict`;
//! - the durable terminal state (status / terminal_reason / final message
//!   row) always equals the FIRST settlement to arrive and never flips;
//! - the terminal `run_state_changed` event exists exactly once.
//!
//! Test tier: contract/service-integration against the real SQLite store
//! (no provider involved — this is the state machine + storage boundary).

use lingxi_adapters::storage::{RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::ports::{CommittedOutcome, KeyEvent, RunOutcome, StorageError, StoragePort};
use lingxi_kernel::{FinalizeSettlement, Principal, RunContext, RunStateMachine};
use lingxi_protocol::{
    ContentBlock, EventId, EventPayload, KnownEventPayload, ModelCallId, NormalizedMessage, RunId,
    RunStateChangedPayload, RunStatus, SessionId,
};

fn temp_db(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lingxi-r03-a02-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

async fn seeded_store(path: &std::path::Path) -> RunDatabase {
    let db = RunDatabase::open(path, StoreOptions::default())
        .await
        .expect("open");
    db.ensure_session_seed(vec![SessionRow {
        session_id: "sess_a".into(),
        agent_id: "lingxi".into(),
        owner_user_id: "user_local".into(),
        title: "t".into(),
        created_at_unix_ms: 1,
    }])
    .await
    .expect("seed");
    db
}

fn ctx(run_id: &str) -> RunContext {
    let run = RunId::new(run_id.to_string());
    RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sess_a".to_string()),
        attempt: lingxi_kernel::attempt_id(&run, 1),
        run_id: run,
        generation: 1,
    }
}

fn final_message(text: &str) -> NormalizedMessage {
    NormalizedMessage {
        role: "assistant".to_string(),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        model_call_id: Some(ModelCallId::new("mc-1")),
    }
}

/// The settlement pool: settlements that are LEGAL as the first finalize
/// from `running`. `index` 0..4; each is a distinct settlement, and
/// duplicates of the same index are byte-identical (the "相同 settled 消息
/// 重复"). Event ids carry the run id exactly like the real driver's
/// `{run_id}-done` (event_id is a global primary key).
fn settlement(run_id: &str, index: usize) -> RunOutcome {
    let (status, reason, message) = match index {
        0 => (
            RunStatus::Completed,
            Some("completed.with_final".to_string()),
            Some(final_message("answer-a")),
        ),
        1 => (
            RunStatus::Completed,
            Some("completed.with_final".to_string()),
            Some(final_message("answer-b")),
        ),
        2 => (
            RunStatus::Completed,
            Some("completed.no_final.empty_reply".to_string()),
            None,
        ),
        3 => (
            RunStatus::Failed,
            Some("failed.provider_error".to_string()),
            None,
        ),
        _ => (
            RunStatus::InterruptedNeedsAttention,
            Some("interrupted_needs_attention.recovery_unsafe".to_string()),
            None,
        ),
    };
    RunOutcome {
        status,
        reason: reason.clone(),
        key_events: vec![KeyEvent {
            event_id: EventId::new(format!("{run_id}-done")),
            payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                RunStateChangedPayload {
                    from: RunStatus::Running,
                    to: status,
                    reason,
                },
            )),
        }],
        final_message: message,
    }
}

/// Deterministic xorshift (seed fixed below; changing it changes the test
/// case population, which is a deliberate act).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

const SEED: u64 = 0x5EED_0000_C0FF_EE01;
const ITERATIONS: u64 = 200;
const SUBMISSIONS_PER_RUN: usize = 10;

// ── R03-T08 / A16: replayable-failure machinery (long-term regression) ──────
//
// Same contract as the T04 fencing property: `R03_A02_PROPERTY_SEED`
// (hex `0x…` or decimal) overrides the seed for verbatim replay of a
// captured failing schedule; a thread-local-aware panic hook prints the
// machine-greppable `R03_A02_PROPERTY_FAILURE seed=0x…` line (stdout AND
// stderr) and records the seed to `R03_A02_PROPERTY_SEED_FILE` when set.
// No natural anomaly has ever been observed under the default seed; the
// mechanism is proven by an isolated mutation probe (T08 evidence).

thread_local! {
    static PROPERTY_SEED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn effective_seed() -> u64 {
    let seed = match std::env::var("R03_A02_PROPERTY_SEED") {
        Ok(raw) => {
            let trimmed = raw.trim();
            let parsed = if let Some(hex) = trimmed.strip_prefix("0x") {
                u64::from_str_radix(hex, 16)
            } else {
                trimmed.parse::<u64>()
            };
            parsed.unwrap_or_else(|err| panic!("invalid R03_A02_PROPERTY_SEED {raw:?}: {err}"))
        }
        Err(_) => SEED,
    };
    PROPERTY_SEED.with(|cell| cell.set(seed));
    install_seed_capture_hook();
    seed
}

fn install_seed_capture_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let seed = PROPERTY_SEED.with(std::cell::Cell::get);
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let line = format!("R03_A02_PROPERTY_FAILURE seed={seed:#x} location={location}");
        println!("{line}");
        eprintln!("{line}");
        if let Ok(out) = std::env::var("R03_A02_PROPERTY_SEED_FILE") {
            let _ = std::fs::write(&out, format!("{seed:#x}\n"));
        }
        previous(info);
    }));
}

#[tokio::test]
async fn r03_a02_duplicate_and_out_of_order_settlements_settle_exactly_once() {
    let seed = effective_seed();
    let path = temp_db("property");
    let db = seeded_store(&path).await;
    let mut rng = Rng(seed);

    // Aggregated result counts over the whole property run (the acceptance
    // evidence: "只持久一次终态/结算" + "冲突记录可诊断").
    let mut total_first_commits: u64 = 0;
    let mut total_replays: u64 = 0;
    let mut total_conflicts: u64 = 0;
    let total_unexpected: u64 = 0;
    let mut runs_settled: u64 = 0;

    for iteration in 0..ITERATIONS {
        let run_id = db.allocate_run_id(1_000 + iteration).expect("allocate");
        let run_ctx = ctx(&run_id);
        db.record_run_started(&run_ctx, 2_000).await.expect("start");

        // Random arrival order: SUBMISSIONS_PER_RUN submissions, each a
        // random settlement index. Duplicates (same index repeated) model
        // the identical settled message arriving again; different indices
        // model conflicting messages out of order.
        let mut arrivals = Vec::with_capacity(SUBMISSIONS_PER_RUN);
        for _ in 0..SUBMISSIONS_PER_RUN {
            arrivals.push(rng.below(5) as usize);
        }

        let mut winner: Option<usize> = None;
        let mut commits = 0u64;
        let mut replays = 0u64;
        let mut conflicts = 0u64;
        for (step, index) in arrivals.iter().enumerate() {
            let outcome = settlement(&run_id, *index);
            match db
                .commit_run_outcome(&run_ctx, outcome, 3_000 + step as u64)
                .await
            {
                Ok(CommittedOutcome {
                    newly_committed, ..
                }) => {
                    if newly_committed {
                        commits += 1;
                        assert!(
                            winner.is_none(),
                            "iteration {iteration}: a second commit landed after the first"
                        );
                        winner = Some(*index);
                    } else {
                        replays += 1;
                        assert_eq!(
                            Some(*index),
                            winner,
                            "iteration {iteration}: replay accepted for index {} which is not \
                             the winner {:?}",
                            *index,
                            winner
                        );
                    }
                }
                Err(StorageError::Conflict { detail }) => {
                    conflicts += 1;
                    // The conflict must be diagnosable: it names the
                    // already-terminal run.
                    assert!(
                        detail.contains(&run_id) && detail.contains("already terminal"),
                        "iteration {iteration}: weak conflict diagnosis: {detail}"
                    );
                    assert_ne!(Some(*index), winner);
                }
                Err(other) => {
                    // Any other outcome fails the property immediately; the
                    // aggregate counter stays a pure result count.
                    panic!("iteration {iteration}: unexpected error {other:?}");
                }
            }
        }
        // Exactly one settlement persisted, whatever the arrival order.
        assert_eq!(commits, 1, "iteration {iteration}: arrivals {arrivals:?}");
        assert_eq!(
            replays + conflicts,
            (SUBMISSIONS_PER_RUN - 1) as u64,
            "iteration {iteration}: every later submission is a replay or a conflict"
        );
        runs_settled += 1;
        total_first_commits += commits;
        total_replays += replays;
        total_conflicts += conflicts;

        // Durable state: the run holds the WINNER's settlement and nothing
        // on top of it.
        let winner_outcome = settlement(&run_id, winner.expect("winner exists"));
        let status: String = db
            .query_one_text(
                "SELECT status FROM runs WHERE run_id = ?1",
                vec![run_id.clone()],
            )
            .await
            .unwrap()
            .expect("run row");
        assert_eq!(status, winner_outcome.status.wire_name());
        let reason: Option<String> = db
            .query_one_text(
                "SELECT terminal_reason FROM runs WHERE run_id = ?1",
                vec![run_id.clone()],
            )
            .await
            .unwrap();
        assert_eq!(reason, winner_outcome.reason);
        let final_rows: i64 = db
            .query_one_text(
                "SELECT COUNT(*) FROM messages WHERE run_id = ?1",
                vec![run_id.clone()],
            )
            .await
            .unwrap()
            .and_then(|v| v.parse().ok())
            .unwrap();
        assert_eq!(
            final_rows,
            if winner_outcome.final_message.is_some() {
                1
            } else {
                0
            },
            "iteration {iteration}: exactly the winner's final message row exists"
        );
        if let Some(message) = &winner_outcome.final_message {
            let content = db
                .query_one_text(
                    "SELECT content_json FROM messages WHERE run_id = ?1",
                    vec![run_id.clone()],
                )
                .await
                .unwrap()
                .expect("winner final message row");
            assert!(content.contains(&match &message.content[0] {
                ContentBlock::Text { text } => text.clone(),
                _ => unreachable!("test builds text blocks"),
            }));
        }
        // The terminal transition event exists EXACTLY once.
        let terminal_events: i64 = db
            .query_one_text(
                "SELECT COUNT(*) FROM key_events WHERE run_id = ?1 AND event_type = 'run_state_changed'",
                vec![run_id.clone()],
            )
            .await
            .unwrap()
            .and_then(|v| v.parse().ok())
            .unwrap();
        assert_eq!(
            terminal_events, 2,
            "iteration {iteration}: creation + the ONE terminal transition"
        );
    }

    assert_eq!(total_unexpected, 0);
    println!(
        "R03_A02_PROPERTY seed={seed:#x} iterations={ITERATIONS} runs_settled={runs_settled} \
         first_commits={total_first_commits} idempotent_replays={total_replays} \
         diagnosed_conflicts={total_conflicts} unexpected={total_unexpected}"
    );
    assert_eq!(total_first_commits, ITERATIONS);
    assert!(
        total_replays > 0,
        "the random orders must include duplicates"
    );
    assert!(
        total_conflicts > 0,
        "the random orders must include conflicts"
    );

    // Write the machine-readable result counting when the harness asks for it
    // (evidence file; the stdout line above is the human summary).
    if let Ok(out) = std::env::var("R03_A02_PROPERTY_EVIDENCE") {
        let path = std::path::PathBuf::from(out);
        std::fs::create_dir_all(path.parent().expect("parent")).unwrap();
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema": "lingxi.r03-a02-finalize-property.v1",
                "seed": seed,
                "iterations": ITERATIONS,
                "submissionsPerRun": SUBMISSIONS_PER_RUN,
                "runsSettled": runs_settled,
                "firstCommits": total_first_commits,
                "idempotentReplays": total_replays,
                "diagnosedConflicts": total_conflicts,
                "unexpectedResults": total_unexpected,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    db.close().await.expect("close");
    let _ = std::fs::remove_file(&path);
}

/// The same property against the kernel's pure finalize decision with an
/// adversarial twist: a settlement that is NOT legal from `running` (a
/// direct cancelled finalize) arriving out of order must be rejected
/// without touching the run, and the run still settles exactly once.
#[tokio::test]
async fn out_of_order_illegal_settlement_is_rejected_and_run_still_settles_once() {
    let path = temp_db("illegal");
    let db = seeded_store(&path).await;
    let run_id = db.allocate_run_id(9_000).expect("allocate");
    let run_ctx = ctx(&run_id);
    db.record_run_started(&run_ctx, 9_001).await.expect("start");

    // Direct running -> cancelled skips the cancelling phase: illegal.
    let cancelled = RunOutcome {
        status: RunStatus::Cancelled,
        reason: Some("cancelled.requested".to_string()),
        key_events: Vec::new(),
        final_message: None,
    };
    match db.commit_run_outcome(&run_ctx, cancelled, 9_002).await {
        Err(StorageError::InvalidRequest { detail }) => {
            assert!(detail.contains("finalize"), "{detail}");
        }
        other => panic!("illegal direct cancel must be InvalidRequest, got {other:?}"),
    }
    // Run still active; the legal settlement then settles exactly once.
    let settled = db
        .commit_run_outcome(&run_ctx, settlement(&run_id, 0), 9_003)
        .await
        .expect("legal finalize");
    assert!(settled.newly_committed);
    // And the identical duplicate replays.
    let replay = db
        .commit_run_outcome(&run_ctx, settlement(&run_id, 0), 9_004)
        .await
        .expect("identical duplicate replays");
    assert!(!replay.newly_committed);

    // The kernel verdict agrees with the storage verdict for the same
    // settlement shape (same decision core both sides).
    let requested = FinalizeSettlement {
        status: RunStatus::Completed,
        reason: Some("completed.with_final".to_string()),
        final_message: Some(final_message("answer-a")),
    };
    let stored = FinalizeSettlement {
        status: RunStatus::Completed,
        reason: Some("completed.with_final".to_string()),
        final_message: Some(final_message("answer-a")),
    };
    assert!(matches!(
        RunStateMachine::finalize(RunStatus::Completed, Some(&stored), &requested),
        Ok(lingxi_kernel::FinalizeVerdict::IdempotentReplay { .. })
    ));

    db.close().await.expect("close");
    let _ = std::fs::remove_file(&path);
}
