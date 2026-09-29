//! R03-T04 (property half): DUPLICATE, OUT-OF-ORDER and DELAYED event
//! deliveries against the REAL `RunDatabase` through the R03-T04 fence —
//! every stale delivery is refused for state purposes and leaves exactly
//! one audit trace; any ILLEGAL state sequence observed in the durable
//! state FAILS the test.
//!
//! Deterministic property loop (fixed xorshift seed, part of the test's
//! identity — the T01 `run_finalize_property` style): every round builds a
//! run (optionally with a retry attempt), assembles a delivery pool of
//! legit current-attempt events, superseded-attempt results (delayed /
//! duplicated), never-opened-attempt claims and illegal state transitions,
//! then submits them in a seeded random ORDER (out-of-order form) with the
//! settlement at a random position, asserting after EVERY delivery:
//!
//! - the durable status never goes backwards and a terminal state never
//!   flips (an illegal durable sequence fails the test);
//! - every stale-class delivery (superseded attempt / never-opened attempt
//!   / terminal run) is refused with `Conflict` AND leaves exactly one
//!   `stale_result_audit` row — the write is refused but the arrival is
//!   audited ("拒写但留审计痕迹");
//! - refused deliveries never move `last_event_seq`, never add key events
//!   and never create message rows — old results never pollute the
//!   current attempt or a settled run;
//! - illegal state-machine requests (terminal targets through the
//!   non-terminal surface, forbidden active legs) are loudly rejected;
//! - identical duplicate SETTLEMENTS replay idempotently (`newly_committed
//!   == false`) and conflicting settlements are diagnosed (the T01/A02
//!   floor under this property).
//!
//! Test tier: contract/service-integration against the real SQLite store
//! (no provider involved — this is the state machine + storage boundary).

use lingxi_adapters::storage::{RunDatabase, SessionRow, StoreOptions};
use lingxi_kernel::ports::{CommittedOutcome, KeyEvent, RunOutcome, StorageError, StoragePort};
use lingxi_kernel::{Principal, RunContext};
use lingxi_protocol::{
    EventId, EventPayload, KnownEventPayload, ModelCallCompletedPayload, ModelCallId, RunId,
    RunStateChangedPayload, RunStatus, SessionId,
};

/// Fixed seed — part of the test's identity (changing it changes the
/// generated schedules; failures must be reproduced against THIS seed).
const SEED: u64 = 0x5EED_0000_A07D_F00D;
/// Rounds (each against a fresh real database).
const ROUNDS: u32 = 60;

fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

fn temp_db(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lingxi-r03-t04-prop-{tag}-{}-{}",
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

fn ctx_of(run_id: &str, attempt: u32) -> RunContext {
    let run = RunId::new(run_id.to_string());
    RunContext {
        principal: Principal::LocalUser,
        session_id: SessionId::new("sess_a".to_string()),
        attempt: lingxi_kernel::attempt_id(&run, attempt),
        run_id: run,
        generation: 1,
    }
}

fn model_call_done_event(run_id: &str, call: &str) -> KeyEvent {
    KeyEvent {
        event_id: EventId::new(format!("{run_id}-{call}-done")),
        payload: EventPayload::Known(KnownEventPayload::ModelCallCompleted(
            ModelCallCompletedPayload {
                model_call_id: ModelCallId::new(format!("{run_id}-{call}")),
                usage: None,
            },
        )),
    }
}

fn completed_no_final(run_id: &str) -> RunOutcome {
    RunOutcome {
        status: RunStatus::Completed,
        reason: Some("completed.no_final.process_only".to_string()),
        key_events: vec![KeyEvent {
            event_id: EventId::new(format!("{run_id}-done")),
            payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                RunStateChangedPayload {
                    from: RunStatus::Running,
                    to: RunStatus::Completed,
                    reason: Some("completed.no_final.process_only".to_string()),
                },
            )),
        }],
        final_message: None,
    }
}

fn cancelled_outcome(run_id: &str) -> RunOutcome {
    RunOutcome {
        status: RunStatus::Cancelled,
        reason: Some("cancelled.requested".to_string()),
        key_events: vec![KeyEvent {
            event_id: EventId::new(format!("{run_id}-done")),
            payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                RunStateChangedPayload {
                    from: RunStatus::Cancelling,
                    to: RunStatus::Cancelled,
                    reason: Some("cancelled.requested".to_string()),
                },
            )),
        }],
        final_message: None,
    }
}

/// What one pool entry does when submitted.
enum Delivery {
    /// Legit mid-run events for the CURRENT attempt.
    CurrentAttemptEvent { call: String },
    /// A LATE result claiming the SUPERSEDED attempt (attempt1 after a
    /// retry opened attempt2) — delayed / duplicated forms.
    SupersededAttemptResult { call: String },
    /// A result claiming an attempt that NEVER opened.
    GhostAttemptResult,
    /// The run's settlement (completed no-final, or two-phase cancelled).
    Settle { cancelled: bool },
    /// An identical duplicate of the settlement (idempotent replay form).
    SettleDuplicate { cancelled: bool },
    /// A CONFLICTING settlement (different terminal) — must be diagnosed,
    /// never merged (T01/A02 floor under this property).
    SettleConflicting,
    /// Illegal state-machine requests: a terminal target through the
    /// non-terminal surface, and a forbidden active leg.
    IllegalStateChange { kind: u8 },
}

fn count(sql_row: Option<String>) -> i64 {
    sql_row.and_then(|v| v.parse().ok()).unwrap_or(-1)
}

async fn scalar(db: &RunDatabase, sql: &str, arg: String) -> i64 {
    count(
        db.query_one_text(sql, vec![arg])
            .await
            .expect("scalar query"),
    )
}

/// One property round. Returns its classified-delivery counters.
#[derive(Default, Debug, Clone, Copy)]
struct RoundCounters {
    current_ok: u32,
    stale_refused: u32,
    ghost_refused: u32,
    terminal_refused: u32,
    settle_first: u32,
    settle_replay: u32,
    settle_conflict: u32,
    illegal_rejected: u32,
}

async fn round(db: &RunDatabase, round_index: u32, rng: &mut u64) -> RoundCounters {
    let run_id = db
        .allocate_run_id(1_790_409_600_000 + round_index as u64)
        .expect("allocate run id");
    let mut counters = RoundCounters::default();

    // Start the run (attempt #1 durably open).
    db.record_run_started(&ctx_of(&run_id, 1), 1)
        .await
        .expect("run starts");

    // Maybe open attempt2 (the retry that supersedes attempt1).
    let attempt2_open = xorshift64(rng).is_multiple_of(2);
    if attempt2_open {
        db.record_attempt_started(&ctx_of(&run_id, 2), 10)
            .await
            .expect("attempt2 opens");
    }
    let current_attempt: u32 = if attempt2_open { 2 } else { 1 };

    // Assemble the delivery pool: duplicate / delayed / out-of-order forms.
    let mut pool = vec![
        Delivery::CurrentAttemptEvent {
            call: "mc0100".to_string(),
        },
        Delivery::CurrentAttemptEvent {
            call: "mc0101".to_string(),
        },
        Delivery::Settle {
            cancelled: xorshift64(rng).is_multiple_of(2),
        },
        Delivery::SettleDuplicate {
            cancelled: xorshift64(rng).is_multiple_of(2), // replay may name either shape
        },
        Delivery::SettleConflicting,
        Delivery::GhostAttemptResult,
        Delivery::IllegalStateChange { kind: 0 },
        Delivery::IllegalStateChange { kind: 1 },
    ];
    if attempt2_open {
        // Delayed + duplicated late results of the superseded attempt.
        pool.push(Delivery::SupersededAttemptResult {
            call: "mc0001".to_string(),
        });
        pool.push(Delivery::SupersededAttemptResult {
            call: "mc0001".to_string(),
        });
    }
    // Seeded shuffle (out-of-order form).
    for i in (1..pool.len()).rev() {
        let j = (xorshift64(rng) as usize) % (i + 1);
        pool.swap(i, j);
    }

    // Invariant baselines.
    for delivery in pool {
        // Snapshot the durable facts BEFORE the delivery.
        let record = db
            .load_run(&RunId::new(run_id.clone()))
            .await
            .unwrap()
            .expect("run row");
        let terminal = record.status.is_terminal();
        let events_before = scalar(
            db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
            run_id.clone(),
        )
        .await;
        let messages_before = scalar(
            db,
            "SELECT COUNT(*) FROM messages WHERE run_id = ?1",
            run_id.clone(),
        )
        .await;
        let audit_before = scalar(
            db,
            "SELECT COUNT(*) FROM stale_result_audit WHERE run_id = ?1",
            run_id.clone(),
        )
        .await;

        let outcome: Result<CommittedOutcome, StorageError> = match &delivery {
            Delivery::CurrentAttemptEvent { call } => {
                db.record_run_events(
                    &ctx_of(&run_id, current_attempt),
                    vec![model_call_done_event(&run_id, call)],
                    100,
                )
                .await
            }
            Delivery::SupersededAttemptResult { call } => {
                db.record_run_events(
                    &ctx_of(&run_id, 1),
                    vec![model_call_done_event(&run_id, call)],
                    110,
                )
                .await
            }
            Delivery::GhostAttemptResult => {
                db.record_run_events(
                    &ctx_of(&run_id, 7),
                    vec![model_call_done_event(&run_id, "mc0007")],
                    120,
                )
                .await
            }
            Delivery::Settle { cancelled } => {
                if *cancelled && !terminal {
                    // The two-phase contract: the durable cancelling leg
                    // precedes the cancelled finalize (only on a run that
                    // is still active — a settled run is never re-phased).
                    db.record_run_state_change(
                        &ctx_of(&run_id, current_attempt),
                        RunStatus::Running,
                        RunStatus::Cancelling,
                        Some("cancelled:user".to_string()),
                        130,
                    )
                    .await
                    .expect("the durable cancelling leg precedes the cancelled finalize");
                }
                let outcome_to_commit = if *cancelled {
                    cancelled_outcome(&run_id)
                } else {
                    completed_no_final(&run_id)
                };
                db.commit_run_outcome(&ctx_of(&run_id, current_attempt), outcome_to_commit, 131)
                    .await
            }
            Delivery::SettleDuplicate { cancelled } => {
                // An identical duplicate of whichever settlement owns the
                // terminal state — replays when the shapes match, conflicts
                // when they do not (either way: no second terminal).
                let outcome = if *cancelled {
                    cancelled_outcome(&run_id)
                } else {
                    completed_no_final(&run_id)
                };
                db.commit_run_outcome(&ctx_of(&run_id, current_attempt), outcome, 140)
                    .await
            }
            Delivery::SettleConflicting => {
                // A DIFFERENT terminal (failed) — must never replace the
                // first settlement.
                let conflicting = RunOutcome {
                    status: RunStatus::Failed,
                    reason: Some("failed.provider_error".to_string()),
                    key_events: vec![KeyEvent {
                        event_id: EventId::new(format!("{run_id}-done")),
                        payload: EventPayload::Known(KnownEventPayload::RunStateChanged(
                            RunStateChangedPayload {
                                from: RunStatus::Running,
                                to: RunStatus::Failed,
                                reason: Some("failed.provider_error".to_string()),
                            },
                        )),
                    }],
                    final_message: None,
                };
                db.commit_run_outcome(&ctx_of(&run_id, current_attempt), conflicting, 150)
                    .await
            }
            Delivery::IllegalStateChange { kind } => match kind {
                0 => {
                    db.record_run_state_change(
                        &ctx_of(&run_id, current_attempt),
                        RunStatus::Running,
                        RunStatus::Completed,
                        None,
                        160,
                    )
                    .await
                }
                _ => {
                    db.record_run_state_change(
                        &ctx_of(&run_id, current_attempt),
                        RunStatus::Cancelling,
                        RunStatus::Running,
                        None,
                        161,
                    )
                    .await
                }
            },
        };

        // Whether the delivery was refused — captured before the
        // classification match consumes the outcome.
        let refused = outcome.is_err();
        // Classify against the PRE-delivery durable state — the fence's
        // expected verdict is a function of that state, not of luck.
        match &delivery {
            Delivery::CurrentAttemptEvent { .. } => {
                if terminal {
                    // Delayed past the settlement: refused + audited.
                    assert!(
                        matches!(outcome, Err(StorageError::Conflict { .. })),
                        "round {round_index}: current-attempt delivery after terminal must \
                         conflict, got {outcome:?}"
                    );
                    counters.terminal_refused += 1;
                } else {
                    let committed = outcome.expect("current-attempt delivery commits");
                    assert!(committed.newly_committed);
                    counters.current_ok += 1;
                }
            }
            Delivery::SupersededAttemptResult { .. } => {
                let err = match outcome {
                    Err(StorageError::Conflict { detail }) => detail,
                    other => panic!(
                        "round {round_index}: superseded-attempt result must conflict, got \
                         {other:?}"
                    ),
                };
                if terminal {
                    assert!(err.contains("already terminal"), "detail: {err}");
                    counters.terminal_refused += 1;
                } else {
                    assert!(
                        err.contains("no longer the current attempt"),
                        "detail: {err}"
                    );
                    counters.stale_refused += 1;
                }
            }
            Delivery::GhostAttemptResult => {
                let err = match outcome {
                    Err(StorageError::Conflict { detail }) => detail,
                    other => panic!(
                        "round {round_index}: ghost-attempt claim must conflict, got {other:?}"
                    ),
                };
                if terminal {
                    assert!(err.contains("already terminal"), "detail: {err}");
                    counters.terminal_refused += 1;
                } else {
                    assert!(err.contains("never opened"), "detail: {err}");
                    counters.ghost_refused += 1;
                }
            }
            Delivery::Settle { .. } => match outcome {
                Ok(committed) => {
                    if committed.newly_committed {
                        counters.settle_first += 1;
                    } else {
                        // The identical settlement already arrived (a
                        // duplicate-class member won the order race):
                        // an idempotent replay.
                        counters.settle_replay += 1;
                    }
                }
                Err(_) => {
                    // Another settlement in the pool arrived first (the
                    // out-of-order form) or the two-phase contract
                    // rejected the request: the FIRST settlement owns the
                    // terminal; everything later is a diagnosed refusal.
                    counters.settle_conflict += 1;
                }
            },
            Delivery::SettleDuplicate { .. } => match outcome {
                Ok(committed) => {
                    if committed.newly_committed {
                        // The out-of-order form: the "duplicate" arrived
                        // before every other settlement — it IS the first.
                        counters.settle_first += 1;
                    } else {
                        // The identical-replay form: it arrived after the
                        // same settlement already owned the terminal.
                        counters.settle_replay += 1;
                    }
                }
                // Rejections are diagnosed settlement refusals: the
                // two-phase contract rejects a `cancelled` finalize
                // without its durable `cancelling` leg (InvalidRequest)
                // and a different first settlement owns the terminal
                // (Conflict). Never a second terminal either way.
                Err(_) => counters.settle_conflict += 1,
            },
            Delivery::SettleConflicting => match outcome {
                Err(StorageError::Conflict { .. }) => counters.settle_conflict += 1,
                Ok(committed) => {
                    // Legal ONLY as the first settlement of an unsettled run.
                    assert!(committed.newly_committed);
                    counters.settle_first += 1;
                }
                other => panic!("round {round_index}: conflicting settlement got {other:?}"),
            },
            Delivery::IllegalStateChange { .. } => {
                // Every illegal request is rejected LOUDLY — the form
                // depends on what the durable row holds when it arrives:
                // InvalidRequest (the kernel transition table rejects it),
                // the terminal Conflict (the run already settled) or the
                // stale-view Conflict (the claimed `from` disagrees with
                // the durable status). A success is the only failure.
                assert!(
                    outcome.is_err(),
                    "round {round_index}: illegal state-machine request must be rejected, \
                     got {outcome:?}"
                );
                counters.illegal_rejected += 1;
            }
        }

        // ── invariants after EVERY delivery ──
        // The stale classes are the ones the fence audits; settlement
        // conflicts are the T01 finalize domain (diagnosed, not audited).
        let stale_class = matches!(
            delivery,
            Delivery::CurrentAttemptEvent { .. }
                | Delivery::SupersededAttemptResult { .. }
                | Delivery::GhostAttemptResult
        );
        let after = db
            .load_run(&RunId::new(run_id.clone()))
            .await
            .unwrap()
            .expect("run row");
        // 1. Terminal states never flip; active→terminal happens at most
        //    once (an illegal durable sequence fails the test here).
        if terminal {
            assert_eq!(
                after.status, record.status,
                "round {round_index}: the terminal state {record:?} flipped to {after:?}"
            );
        }
        let events_after = scalar(
            db,
            "SELECT COUNT(*) FROM key_events WHERE run_id = ?1",
            run_id.clone(),
        )
        .await;
        let audit_after = scalar(
            db,
            "SELECT COUNT(*) FROM stale_result_audit WHERE run_id = ?1",
            run_id.clone(),
        )
        .await;
        let messages_after = scalar(
            db,
            "SELECT COUNT(*) FROM messages WHERE run_id = ?1",
            run_id.clone(),
        )
        .await;
        // 2. Refused stale deliveries move NOTHING except the audit count
        //    (exactly one audit row per refused arrival); every refused
        //    delivery of ANY class leaves the stream untouched.
        if refused {
            assert_eq!(
                events_after, events_before,
                "round {round_index}: a refused delivery changed the stream"
            );
            if stale_class {
                assert_eq!(
                    audit_after,
                    audit_before + 1,
                    "round {round_index}: a refused stale delivery must leave exactly one                      audit row"
                );
            } else {
                assert_eq!(
                    audit_after, audit_before,
                    "round {round_index}: non-stale refusals are not stale-result audits"
                );
            }
        }
        // 3. No delivery in this property ever creates a message row
        //    (settlements are final-less; stale content is never a body).
        assert_eq!(
            messages_after, messages_before,
            "round {round_index}: a delivery created a message row"
        );
        assert_eq!(messages_after, 0);
    }

    // The run settled exactly once (the pool guarantees ≥1 settlement
    // attempt succeeded before any terminal assertion could pass).
    let final_record = db
        .load_run(&RunId::new(run_id.clone()))
        .await
        .unwrap()
        .unwrap();
    assert!(
        final_record.status.is_terminal(),
        "round {round_index}: the pool always contains a settlement"
    );
    counters
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_out_of_order_and_delayed_results_fence_exactly_once() {
    let mut rng = SEED;
    let mut totals = RoundCounters::default();
    for round_index in 0..ROUNDS {
        let path = temp_db(&format!("r{round_index}"));
        let db = seeded_store(&path).await;
        let counters = round(&db, round_index, &mut rng).await;
        totals.current_ok += counters.current_ok;
        totals.stale_refused += counters.stale_refused;
        totals.ghost_refused += counters.ghost_refused;
        totals.terminal_refused += counters.terminal_refused;
        totals.settle_first += counters.settle_first;
        totals.settle_replay += counters.settle_replay;
        totals.settle_conflict += counters.settle_conflict;
        totals.illegal_rejected += counters.illegal_rejected;
        db.close().await.expect("close");
        let _ = std::fs::remove_file(&path);
        // Deterministic per-round witness.
        println!(
            "R03_T04_PROPERTY round={round_index} ok={} stale={} ghost={} terminal={} \
             settle={} replay={} conflict={} illegal={}",
            counters.current_ok,
            counters.stale_refused,
            counters.ghost_refused,
            counters.terminal_refused,
            counters.settle_first,
            counters.settle_replay,
            counters.settle_conflict,
            counters.illegal_rejected
        );
    }
    // The generator must actually exercise every class across the rounds
    // (0 hits would make the property vacuous).
    assert!(totals.current_ok > 0, "legit deliveries were exercised");
    assert!(
        totals.stale_refused > 0,
        "superseded-attempt refusals occurred"
    );
    assert!(totals.ghost_refused > 0, "ghost-attempt refusals occurred");
    assert!(
        totals.terminal_refused > 0,
        "post-terminal refusals occurred"
    );
    assert!(totals.settle_first >= ROUNDS, "every round settled");
    assert!(
        totals.settle_replay > 0,
        "idempotent settlement replays occurred"
    );
    assert!(
        totals.settle_conflict > 0,
        "conflicting settlements were diagnosed"
    );
    assert!(
        totals.illegal_rejected >= ROUNDS * 2,
        "every illegal state request was rejected"
    );
    println!("R03_T04_PROPERTY totals: {totals:?} (seed={SEED:#x}, rounds={ROUNDS})");

    // Machine-readable evidence (the established env-var pattern).
    if let Ok(path) = std::env::var("R03_T04_EVIDENCE") {
        let path = std::path::PathBuf::from(path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut doc = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(map) = doc.as_object_mut() {
            map.insert(
                "r03_t04_fencing_property".to_string(),
                serde_json::json!({
                    "seed": format!("{SEED:#x}"),
                    "rounds": ROUNDS,
                    "current_attempt_ok": totals.current_ok,
                    "superseded_attempt_refused": totals.stale_refused,
                    "ghost_attempt_refused": totals.ghost_refused,
                    "post_terminal_refused": totals.terminal_refused,
                    "settlements_first": totals.settle_first,
                    "settlement_replays": totals.settle_replay,
                    "settlement_conflicts": totals.settle_conflict,
                    "illegal_rejections": totals.illegal_rejected,
                }),
            );
        }
        let _ = std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap());
    }
}
