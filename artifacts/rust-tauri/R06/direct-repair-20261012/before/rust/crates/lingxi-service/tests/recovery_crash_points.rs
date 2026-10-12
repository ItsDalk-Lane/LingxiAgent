//! R03-T07 permanent crash-point test set — R03-A13 运行中重启诚实呈现,
//! with REAL process-level kills (SIGKILL) at the critical persistence
//! boundaries (阶段书怎么做 4 + 总控细化: spawn 测试二进制 + kill -9,
//! 崩溃前后轨迹落盘, 可重放 seed).
//!
//! Topology: the parent test spawns a CHILD process (`std::env::current_exe`
//! — the same test binary) filtered to [`crash_child_boundary_harness`]
//! with `LINGXI_R03_T07_CHILD=1`. The child drives ONE run through the REAL
//! composition root (`ServiceState::bootstrap_with_deps` with
//! response-production doubles) over the REAL `RunDatabase`, wrapped in a
//! [`GatedPort`]: a test-local `StoragePort` decorator that delegates every
//! write to the real database and PARKS (forever, after writing a marker
//! file) exactly at the configured boundary, before or after the delegated
//! durable write. The parent observes the marker — the boundary is
//! reached and every write before it is committed — then `Child::kill()`s
//! the child (SIGKILL on unix, TerminateProcess on Windows): a genuine
//! process-level kill with no cleanup, no flush, no drop.
//!
//! The parent then re-opens the same data root through a FRESH production
//! `ServiceState::bootstrap` — whose startup recovery scan (R03-T07) runs
//! — and asserts the honest post-restart state per case: no blank rows,
//! no fake successes, the explainable interrupted/cancelled terminals, no
//! fabricated final replies, and no unconsented re-execution of external
//! effects.
//!
//! Crash-point matrix (the durable boundary ↔ expected restart truth):
//!
//! | case | park point | durable facts at kill | restart truth |
//! |---|---|---|---|
//! | run_started_before | before `record_run_started` | nothing durable | no run row at all; scan finds nothing |
//! | run_started_after | after it | run row `running` | recoverable_wait → interrupted |
//! | intent_before | before `record_invocation_intent` | run row, no journal | recoverable_wait → interrupted |
//! | intent_after | after it | journal `prepared` (never dispatched) | recoverable_wait → interrupted |
//! | started_before | before `advance(started)` | journal `authorized` (never dispatched) | recoverable_wait → interrupted |
//! | started_after | after it | journal `started`, executor never spawned, external count 0 | unknown → needs_attention → interrupted |
//! | external_after | after the external op, before the response | journal `started`, external count 1 | unknown → needs_attention → interrupted; external STILL 1 |
//! | receipt_before | before `record_invocation_receipt` | journal `started`, external count 1 | unknown → needs_attention → interrupted; external STILL 1 |
//! | receipt_after | after it | journal `succeeded` + dedup | recoverable_wait → interrupted |
//! | finalize_before | before `commit_run_outcome` | run active, journal settled | recoverable_wait → interrupted |
//! | finalize_after | after it | run `completed` (terminal) | 终态不复活: scan skips, nothing rewritten |
//!
//! Every case leaves a replayable SEED (fixed session/input/requestId +
//! the boundary config) and pre/post traces under `R03_T07_TRACE_DIR`
//! when set (evidence capture); the tests themselves are hermetic (temp
//! dirs, children killed AND waited — no residue).

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    InvocationPhase, KeyEvent, ProviderDescriptor, ProviderTurnResult, RunOutcome, StorageError,
    StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, TurnDeltaSink,
    TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage, RunId, ToolCallId};
use lingxi_service::{ExecuteSubmission, ServiceDeps, ServiceState};

#[path = "recovery_support/harness.rs"]
mod harness;

use harness::{config_for, owner_principal, synthetic_home};

const CHILD_MODE_ENV: &str = "LINGXI_R03_T07_CHILD";
const CASE_ENV: &str = "LINGXI_R03_T07_CASE";
const HOME_ENV: &str = "LINGXI_R03_T07_HOME";
const MARKER_ENV: &str = "LINGXI_R03_T07_MARKER";
const EXTERNAL_ENV: &str = "LINGXI_R03_T07_EXTERNAL";
const TRACE_DIR_ENV: &str = "R03_T07_TRACE_DIR";

/// The parking boundary (which durable write the kill lands around).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Boundary {
    RunStarted,
    Intent,
    Started,
    External,
    Receipt,
    Finalize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Before,
    After,
}

/// One crash-point case: where the child parks, and what the honest
/// restart must show.
struct Case {
    name: &'static str,
    boundary: Boundary,
    side: Side,
    /// Rows expected in `runs` after the kill.
    expect_run_rows: usize,
    /// Expected durable status after the kill (queried pre-restart by the
    /// parent for the pre-trace; `None` when no row exists).
    expect_kill_status: Option<&'static str>,
    /// Expected journal phases after the kill (in order).
    expect_kill_phases: &'static [&'static str],
    /// External executions the child performed before parking.
    expect_external_at_kill: usize,
    /// External executions after the restart (recovery must not re-execute).
    expect_external_after_restart: usize,
    /// The expected recovery category (`None` = nothing scanned).
    expect_category: Option<&'static str>,
    /// Expected terminal status after the restart scan (`None` = the scan
    /// found nothing / the row was already terminal and stays as killed).
    expect_restart_status: Option<&'static str>,
}

const SESSION: &str = "sess_local_alpha";
const INPUT: &str = "r03-t07 crash-point probe (fixed replay input)";
const REQUEST_ID: &str = "crash-point-1";
const TARGET: &str = "notify.double";

const CASES: &[Case] = &[
    Case {
        name: "run_started_before",
        boundary: Boundary::RunStarted,
        side: Side::Before,
        expect_run_rows: 0,
        expect_kill_status: None,
        expect_kill_phases: &[],
        expect_external_at_kill: 0,
        expect_external_after_restart: 0,
        expect_category: None,
        expect_restart_status: None,
    },
    Case {
        name: "run_started_after",
        boundary: Boundary::RunStarted,
        side: Side::After,
        expect_run_rows: 1,
        expect_kill_status: Some("running"),
        expect_kill_phases: &[],
        expect_external_at_kill: 0,
        expect_external_after_restart: 0,
        expect_category: Some("recoverable_wait"),
        expect_restart_status: Some("interrupted_needs_attention"),
    },
    Case {
        name: "intent_before",
        boundary: Boundary::Intent,
        side: Side::Before,
        expect_run_rows: 1,
        expect_kill_status: Some("running"),
        expect_kill_phases: &[],
        expect_external_at_kill: 0,
        expect_external_after_restart: 0,
        expect_category: Some("recoverable_wait"),
        expect_restart_status: Some("interrupted_needs_attention"),
    },
    Case {
        name: "intent_after",
        boundary: Boundary::Intent,
        side: Side::After,
        expect_run_rows: 1,
        expect_kill_status: Some("running"),
        expect_kill_phases: &["prepared"],
        expect_external_at_kill: 0,
        expect_external_after_restart: 0,
        expect_category: Some("recoverable_wait"),
        expect_restart_status: Some("interrupted_needs_attention"),
    },
    Case {
        name: "started_before",
        boundary: Boundary::Started,
        side: Side::Before,
        expect_run_rows: 1,
        expect_kill_status: Some("running"),
        expect_kill_phases: &["authorized"],
        expect_external_at_kill: 0,
        expect_external_after_restart: 0,
        expect_category: Some("recoverable_wait"),
        expect_restart_status: Some("interrupted_needs_attention"),
    },
    Case {
        name: "started_after",
        boundary: Boundary::Started,
        side: Side::After,
        expect_run_rows: 1,
        expect_kill_status: Some("running"),
        expect_kill_phases: &["started"],
        expect_external_at_kill: 0,
        expect_external_after_restart: 0,
        expect_category: Some("interrupted_needs_attention"),
        expect_restart_status: Some("interrupted_needs_attention"),
    },
    Case {
        name: "external_after",
        boundary: Boundary::External,
        side: Side::After,
        expect_run_rows: 1,
        expect_kill_status: Some("running"),
        expect_kill_phases: &["started"],
        expect_external_at_kill: 1,
        expect_external_after_restart: 1,
        expect_category: Some("interrupted_needs_attention"),
        expect_restart_status: Some("interrupted_needs_attention"),
    },
    Case {
        name: "receipt_before",
        boundary: Boundary::Receipt,
        side: Side::Before,
        expect_run_rows: 1,
        expect_kill_status: Some("running"),
        expect_kill_phases: &["started"],
        expect_external_at_kill: 1,
        expect_external_after_restart: 1,
        expect_category: Some("interrupted_needs_attention"),
        expect_restart_status: Some("interrupted_needs_attention"),
    },
    Case {
        name: "receipt_after",
        boundary: Boundary::Receipt,
        side: Side::After,
        expect_run_rows: 1,
        expect_kill_status: Some("running"),
        expect_kill_phases: &["succeeded"],
        expect_external_at_kill: 1,
        expect_external_after_restart: 1,
        expect_category: Some("recoverable_wait"),
        expect_restart_status: Some("interrupted_needs_attention"),
    },
    Case {
        name: "finalize_before",
        boundary: Boundary::Finalize,
        side: Side::Before,
        expect_run_rows: 1,
        expect_kill_status: Some("running"),
        expect_kill_phases: &["succeeded"],
        expect_external_at_kill: 1,
        expect_external_after_restart: 1,
        expect_category: Some("recoverable_wait"),
        expect_restart_status: Some("interrupted_needs_attention"),
    },
    Case {
        name: "finalize_after",
        boundary: Boundary::Finalize,
        side: Side::After,
        expect_run_rows: 1,
        expect_kill_status: Some("completed"),
        expect_kill_phases: &["succeeded"],
        expect_external_at_kill: 1,
        expect_external_after_restart: 1,
        expect_category: None,
        expect_restart_status: Some("completed"),
    },
];

// ── the gated port decorator (the injected controlled fault point) ───────────

struct GatedPort {
    inner: Arc<lingxi_adapters::storage::RunDatabase>,
    boundary: Boundary,
    side: Side,
    marker: PathBuf,
    run_hint: String,
}

impl GatedPort {
    async fn hit(&self, boundary: Boundary) {
        if boundary != self.boundary {
            return;
        }
        // The marker IS the crash-point trace: which boundary, which side,
        // which process — written BEFORE the park so the parent only kills
        // a process that is genuinely sitting at the boundary.
        let line = serde_json::json!({
            "boundary": format!("{:?}", boundary),
            "side": format!("{:?}", self.side),
            "run_hint": self.run_hint,
            "pid": std::process::id(),
            "at_unix_ms": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        });
        let _ = std::fs::write(&self.marker, serde_json::to_vec_pretty(&line).unwrap());
        // Park forever: the drive TASK sits at the boundary (no worker
        // thread is blocked) until the parent's SIGKILL lands — a real
        // kill: no drop, no flush, no unwind.
        std::future::pending::<()>().await;
    }

    async fn hit_before(&self, boundary: Boundary) {
        if self.side == Side::Before {
            self.hit(boundary).await;
        }
    }

    async fn hit_after(&self, boundary: Boundary) {
        if self.side == Side::After {
            self.hit(boundary).await;
        }
    }
}

impl StoragePort for GatedPort {
    async fn record_run_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> Result<lingxi_kernel::ports::CommittedOutcome, StorageError> {
        self.hit_before(Boundary::RunStarted).await;
        let out = self.inner.record_run_started(ctx, now_unix_ms).await?;
        self.hit_after(Boundary::RunStarted).await;
        Ok(out)
    }

    async fn commit_run_outcome(
        &self,
        ctx: &RunContext,
        outcome: RunOutcome,
        now_unix_ms: u64,
    ) -> Result<lingxi_kernel::ports::CommittedOutcome, StorageError> {
        self.hit_before(Boundary::Finalize).await;
        let out = self
            .inner
            .commit_run_outcome(ctx, outcome, now_unix_ms)
            .await?;
        self.hit_after(Boundary::Finalize).await;
        Ok(out)
    }

    async fn load_run(
        &self,
        run_id: &RunId,
    ) -> Result<Option<lingxi_kernel::ports::RunRecord>, StorageError> {
        self.inner.load_run(run_id).await
    }

    async fn record_run_events(
        &self,
        ctx: &RunContext,
        events: Vec<KeyEvent>,
        now_unix_ms: u64,
    ) -> Result<lingxi_kernel::ports::CommittedOutcome, StorageError> {
        self.inner.record_run_events(ctx, events, now_unix_ms).await
    }

    async fn record_stale_result(
        &self,
        ctx: &RunContext,
        refused: lingxi_kernel::ports::StaleResultFact,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        self.inner
            .record_stale_result(ctx, refused, now_unix_ms)
            .await
    }

    async fn record_attempt_started(
        &self,
        ctx: &RunContext,
        now_unix_ms: u64,
    ) -> Result<lingxi_kernel::ports::CommittedOutcome, StorageError> {
        self.inner.record_attempt_started(ctx, now_unix_ms).await
    }

    async fn record_run_state_change(
        &self,
        ctx: &RunContext,
        from: lingxi_protocol::RunStatus,
        to: lingxi_protocol::RunStatus,
        reason: Option<String>,
        now_unix_ms: u64,
    ) -> Result<lingxi_kernel::ports::CommittedOutcome, StorageError> {
        self.inner
            .record_run_state_change(ctx, from, to, reason, now_unix_ms)
            .await
    }

    async fn record_invocation_intent(
        &self,
        ctx: &RunContext,
        intent: lingxi_kernel::ports::InvocationIntent,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        self.hit_before(Boundary::Intent).await;
        self.inner
            .record_invocation_intent(ctx, intent, now_unix_ms)
            .await?;
        self.hit_after(Boundary::Intent).await;
        Ok(())
    }

    async fn advance_invocation(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        to: InvocationPhase,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        if to == InvocationPhase::Started {
            self.hit_before(Boundary::Started).await;
        }
        self.inner
            .advance_invocation(ctx, journal_id, to, now_unix_ms)
            .await?;
        if to == InvocationPhase::Started {
            self.hit_after(Boundary::Started).await;
        }
        Ok(())
    }

    async fn record_invocation_receipt(
        &self,
        ctx: &RunContext,
        journal_id: &ToolCallId,
        receipt: lingxi_kernel::ports::InvocationReceipt,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        self.hit_before(Boundary::Receipt).await;
        self.inner
            .record_invocation_receipt(ctx, journal_id, receipt, now_unix_ms)
            .await?;
        self.hit_after(Boundary::Receipt).await;
        Ok(())
    }

    async fn record_invocation_unknown(
        &self,
        journal_id: &ToolCallId,
        detail: String,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        self.inner
            .record_invocation_unknown(journal_id, detail, now_unix_ms)
            .await
    }

    async fn load_invocation_journal(
        &self,
        run_id: &RunId,
    ) -> Result<Vec<lingxi_kernel::ports::InvocationJournalEntry>, StorageError> {
        self.inner.load_invocation_journal(run_id).await
    }

    async fn record_run_lineage(
        &self,
        ctx: &RunContext,
        lineage: lingxi_kernel::subagent::RunLineage,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        self.inner
            .record_run_lineage(ctx, lineage, now_unix_ms)
            .await
    }

    async fn load_run_lineage(
        &self,
        run_id: &RunId,
    ) -> Result<Option<lingxi_kernel::subagent::RunLineage>, StorageError> {
        self.inner.load_run_lineage(run_id).await
    }

    async fn record_model_call_usage(
        &self,
        record: lingxi_kernel::usage::ModelCallUsageRecord,
        now_unix_ms: u64,
    ) -> Result<(), StorageError> {
        self.inner
            .record_model_call_usage(record, now_unix_ms)
            .await
    }

    async fn query_model_call_usage(
        &self,
        query: lingxi_kernel::usage::ModelUsageQuery,
    ) -> Result<Vec<lingxi_kernel::usage::ModelCallUsageRecord>, StorageError> {
        self.inner.query_model_call_usage(query).await
    }
}

// ── the child's response-production doubles ──────────────────────────────────

struct OneToolThenFinalProvider;

impl TurnProviderPort for OneToolThenFinalProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.crash-points".to_string(),
            model: "stub.model".to_string(),
            operation: "chat".to_string(),
        }
    }

    fn next_turn<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ModelCallId,
        input: &'a ModelTurnInput,

        _deltas: &'a dyn TurnDeltaSink,
    ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        let turn = input.turn;
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            let out = if turn == 1 {
                lingxi_kernel::ports::ProviderTurn::ToolRequests {
                    content: Vec::new(),
                    requests: vec![ToolRequest::from_effective_arguments(
                        TARGET,
                        serde_json::json!({
                            "target": TARGET, "payload": "fixed",
                        }),
                        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
                    )
                    .expect("effective tool request")
                    .with_summary("crash-point tool")],
                }
            } else {
                lingxi_kernel::ports::ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".to_string(),
                        content: vec![ContentBlock::Text {
                            text: "final after the tool".to_string(),
                        }],
                        model_call_id: None,
                    },
                }
            };
            ProviderTurnResult::of_ctx(&ctx_at_issue, out)
        })
    }
}

/// The external-system double: a file-backed request log under the shared
/// evidence directory (one JSON line per EXECUTED operation — the counter
/// that must never grow without consent). Optionally parks AFTER the
/// durable external effect (the `External` boundary) instead of returning.
struct ExternalTool {
    external_dir: PathBuf,
    park_after: bool,
    marker: PathBuf,
}

impl ExternalTool {
    fn execute_external(&self) -> String {
        let log = self.external_dir.join("requests.log");
        let executed_so_far = std::fs::read_to_string(&log)
            .map(|raw| raw.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0);
        let digest = format!("digest-{}", executed_so_far + 1);
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .expect("open external log");
        writeln!(
            file,
            "{}",
            serde_json::json!({ "executed": true, "digest": digest })
        )
        .expect("append external log");
        digest
    }
}

impl ToolExecutorPort for ExternalTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        _request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let digest = self.execute_external();
        let park = self.park_after;
        let marker = self.marker.clone();
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            if park {
                let line = serde_json::json!({
                    "boundary": "External",
                    "side": "After",
                    "external_digest": digest,
                    "pid": std::process::id(),
                });
                let _ = std::fs::write(&marker, serde_json::to_vec_pretty(&line).unwrap());
                // The external effect is durable; the response never
                // arrives (the process dies first).
                std::future::pending::<()>().await;
            }
            let outcome = ToolOutcome::success_text(digest);
            ToolExecutionResult::of_ctx(&ctx_at_issue, outcome)
        })
    }
}

fn external_count(external_dir: &Path) -> usize {
    std::fs::read_to_string(external_dir.join("requests.log"))
        .map(|raw| raw.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

// ── the child harness (runs only under CHILD_MODE_ENV) ───────────────────────

/// The child entry. Standalone (no env): returns immediately — the parent
/// drives the matrix; this harness only ever does work in child mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crash_child_boundary_harness() {
    if std::env::var(CHILD_MODE_ENV).is_err() {
        return;
    }
    let case_name = std::env::var(CASE_ENV).expect("child case");
    let case = CASES
        .iter()
        .find(|c| c.name == case_name)
        .unwrap_or_else(|| panic!("unknown case {case_name}"));
    let home = PathBuf::from(std::env::var(HOME_ENV).expect("child home"));
    let marker = PathBuf::from(std::env::var(MARKER_ENV).expect("child marker"));
    let external_dir = PathBuf::from(std::env::var(EXTERNAL_ENV).expect("child external dir"));

    let layout = lingxi_service::prepare_layout(&home).expect("child layout");
    let tools: Arc<dyn ToolExecutorPort> = Arc::new(ExternalTool {
        external_dir,
        park_after: case.boundary == Boundary::External,
        marker: marker.clone(),
    });
    let state = ServiceState::bootstrap_with_deps(
        config_for(&home),
        &layout,
        ServiceDeps {
            turn_provider: Some(Arc::new(OneToolThenFinalProvider)),
            tool_executor: Some(tools),
            ..ServiceDeps::default()
        },
    )
    .await
    .expect("child bootstrap");

    let gated = Arc::new(GatedPort {
        inner: Arc::clone(state.storage()),
        boundary: case.boundary,
        side: case.side,
        marker,
        run_hint: format!("{case_name}:{}", std::process::id()),
    });
    let submission = ExecuteSubmission {
        input: INPUT,
        request_id: Some(REQUEST_ID),
    };
    // The submission runs on its own task: the gate parks INSIDE it at the
    // boundary while this (main) task also parks — the process stays alive
    // at the boundary until the parent's SIGKILL.
    let drive = tokio::spawn({
        let state = state.clone();
        let gated = Arc::clone(&gated);
        async move {
            let _ = state
                .sessions()
                .execute_submission_for(
                    gated.as_ref(),
                    state.events(),
                    state.runs(),
                    &owner_principal(),
                    SESSION,
                    &submission,
                    1_790_409_600_000,
                )
                .await;
        }
    });
    let _drive = drive; // parked or finished (finalize_after); keep alive.
    std::future::pending::<()>().await;
}

// ── the parent: the kill -9 matrix ───────────────────────────────────────────

/// R03-A13: for every crash-point case, kill -9 a real child process at
/// the boundary, restart over the same data root and verify the honest
/// recovery presentation (轨迹: pre/post traces + seed per case).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn r03_a13_kill9_crash_matrix_honest_restart_presentation() {
    if std::env::var(CHILD_MODE_ENV).is_ok() {
        return; // never double-run inside a child
    }
    let trace_dir = std::env::var(TRACE_DIR_ENV).ok().map(PathBuf::from);
    if let Some(dir) = &trace_dir {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut summary = Vec::new();
    for case in CASES {
        let home = synthetic_home(&format!("kill9-{}", case.name));
        let external_dir = synthetic_home(&format!("kill9-ext-{}", case.name));
        std::fs::create_dir_all(&external_dir).expect("external dir");
        let marker = external_dir.join("boundary.marker");

        // The replay SEED (deterministic inputs of this case).
        let seed = serde_json::json!({
            "case": case.name,
            "boundary": format!("{:?}", case.boundary),
            "side": format!("{:?}", case.side),
            "session": SESSION,
            "input": INPUT,
            "request_id": REQUEST_ID,
            "tool_target": TARGET,
        });
        if let Some(dir) = &trace_dir {
            let _ = std::fs::write(
                dir.join(format!("{}.seed.json", case.name)),
                serde_json::to_vec_pretty(&seed).unwrap(),
            );
        }

        // Spawn the real child (this very test binary, filtered to the
        // child harness) and wait until it is parked AT the boundary.
        let exe = std::env::current_exe().expect("current test binary");
        let mut child = std::process::Command::new(exe)
            .arg("--exact")
            .arg("crash_child_boundary_harness")
            .arg("--test-threads=1")
            .env(CHILD_MODE_ENV, "1")
            .env(CASE_ENV, case.name)
            .env(HOME_ENV, &home)
            .env(MARKER_ENV, &marker)
            .env(EXTERNAL_ENV, &external_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn crash child");
        let boundary_reached = harness::wait_until(20_000, || marker.exists()).await;
        assert!(
            boundary_reached,
            "case {}: child never reached the crash boundary",
            case.name
        );

        // Pre-kill trace: the durable facts as they survived at the
        // boundary (read from the database FILE with an independent
        // rusqlite handle — WAL committed facts, no service involvement).
        let pre = pre_kill_facts(&home, &external_dir);
        assert_eq!(
            pre.run_rows, case.expect_run_rows,
            "case {}: pre run rows",
            case.name
        );
        if let (Some(expected), Some(actual)) = (case.expect_kill_status, pre.status.as_deref()) {
            assert_eq!(actual, expected, "case {}: pre status", case.name);
        }
        assert_eq!(
            pre.journal_phases, case.expect_kill_phases,
            "case {}: pre journal phases",
            case.name
        );
        assert_eq!(
            external_count(&external_dir),
            case.expect_external_at_kill,
            "case {}: external executions at the boundary",
            case.name
        );

        // THE KILL: SIGKILL — no cleanup, no flush, no drop.
        child.kill().expect("kill -9 the child");
        let exited = child.wait().expect("reap the child");
        assert!(!exited.success(), "the killed child cannot report success");

        // THE RESTART: a fresh production bootstrap over the same data
        // root — the startup recovery scan runs inside it.
        let layout = lingxi_service::prepare_layout(&home).expect("restart layout");
        let state = ServiceState::bootstrap(config_for(&home), &layout)
            .await
            .expect_or_case(case.name);
        let scan = state.recovery_report().expect("the scan ran at bootstrap");
        match case.expect_category {
            None => assert_eq!(
                scan.scanned, 0,
                "case {}: nothing was recoverable",
                case.name
            ),
            Some(category) => {
                assert_eq!(scan.scanned, 1, "case {}: scanned", case.name);
                let outcome = &scan.outcomes[0];
                assert_eq!(
                    outcome.category.name(),
                    category,
                    "case {}: recovery category",
                    case.name
                );
                assert!(
                    outcome.user_reason.contains("service restarted"),
                    "case {}: the reason explains the restart: {}",
                    case.name,
                    outcome.user_reason
                );
                assert!(
                    !outcome.next_actions.is_empty(),
                    "case {}: executable next actions are exposed",
                    case.name
                );
            }
        }
        // The durable post-restart truth.
        let post = post_restart_facts(&state, &external_dir).await;
        assert_eq!(
            post.run_rows, case.expect_run_rows,
            "case {}: post run rows",
            case.name
        );
        assert_eq!(
            post.status.as_deref(),
            case.expect_restart_status,
            "case {}: post-restart status (no blank, no fake success)",
            case.name
        );
        if case.expect_restart_status == Some("interrupted_needs_attention") {
            assert_eq!(
                post.terminal_reason.as_deref(),
                Some("interrupted_needs_attention.recovery_unsafe"),
                "case {}: stable row reason",
                case.name
            );
            assert!(
                post.final_messages == 0,
                "case {}: NO fabricated final model reply",
                case.name
            );
            assert!(
                post.recovery_event_reason
                    .as_deref()
                    .is_some_and(|r| r.contains("service restarted")),
                "case {}: the terminal event carries the user-visible reason",
                case.name
            );
        }
        // started-without-receipt cases: the journal verdict is unknown.
        if case.name == "started_after"
            || case.name == "external_after"
            || case.name == "receipt_before"
        {
            assert_eq!(
                post.journal_phases.as_slice(),
                &["unknown"],
                "case {}: the crash window is classified unknown",
                case.name
            );
            assert_eq!(
                post.journal_receipt_outcome.as_deref(),
                Some("unknown"),
                "case {}: the receipt reads unknown",
                case.name
            );
        }
        // External effects are NEVER re-executed by the restart.
        assert_eq!(
            external_count(&external_dir),
            case.expect_external_after_restart,
            "case {}: external executions after the restart (no unconsented re-execution)",
            case.name
        );

        // Traces + evidence.
        if let Some(dir) = &trace_dir {
            let _ = std::fs::write(
                dir.join(format!("{}.pre.json", case.name)),
                serde_json::to_vec_pretty(&pre.trace).unwrap(),
            );
            let _ = std::fs::write(
                dir.join(format!("{}.post.json", case.name)),
                serde_json::to_vec_pretty(&post.trace).unwrap(),
            );
        }
        summary.push(serde_json::json!({
            "case": case.name,
            "pre_status": pre.status,
            "restart_status": post.status,
            "category": case.expect_category,
            "external_at_kill": external_count(&external_dir),
        }));
        state.storage().close().await.expect("close restart");
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&external_dir);
    }
    harness::write_evidence(
        "r03_a13_kill9_matrix",
        serde_json::json!({ "cases": summary }),
    );
    println!(
        "R03_A13_TRACE: kill9_matrix cases={} all_honest=true",
        summary.len()
    );
}

// ── trace readers ────────────────────────────────────────────────────────────

struct KillFacts {
    run_rows: usize,
    status: Option<String>,
    journal_phases: Vec<String>,
    trace: serde_json::Value,
}

/// Reads the durable facts from the database FILE (independent rusqlite
/// handle; WAL-committed data is visible).
fn pre_kill_facts(home: &Path, external_dir: &Path) -> KillFacts {
    let db = home.join("lingxi-service").join("data").join("runs.db");
    let (run_rows, status) = if db.exists() {
        let conn = rusqlite::Connection::open(&db).expect("open pre-kill");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))
            .unwrap_or(0);
        let status: Option<String> = conn
            .query_row("SELECT status FROM runs", [], |r| r.get(0))
            .ok();
        drop(conn);
        (rows, status)
    } else {
        (0, None)
    };
    let journal_phases = read_journal_phases(&db);
    KillFacts {
        run_rows: run_rows as usize,
        status: status.clone(),
        journal_phases: journal_phases.clone(),
        trace: serde_json::json!({
            "run_rows": run_rows,
            "status": status,
            "journal_phases": journal_phases,
            "external_executed": external_count(external_dir),
            "marker": std::fs::read_to_string(
                external_dir.join("boundary.marker")
            ).ok(),
        }),
    }
}

fn read_journal_phases(db: &Path) -> Vec<String> {
    if !db.exists() {
        return Vec::new();
    }
    let conn = rusqlite::Connection::open(db).expect("open journal reader");
    let mut stmt = conn
        .prepare("SELECT phase FROM invocation_journal ORDER BY rowid")
        .expect("journal query");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("journal rows");
    rows.filter_map(|r| r.ok()).collect()
}

struct PostFacts {
    run_rows: usize,
    status: Option<String>,
    terminal_reason: Option<String>,
    journal_phases: Vec<String>,
    journal_receipt_outcome: Option<String>,
    final_messages: usize,
    recovery_event_reason: Option<String>,
    trace: serde_json::Value,
}

async fn post_restart_facts(state: &ServiceState, external_dir: &Path) -> PostFacts {
    async fn q(
        state: &ServiceState,
        sql: &str,
        arg: String,
    ) -> Result<Option<String>, lingxi_kernel::ports::StorageError> {
        state.storage().query_one_text(sql, vec![arg]).await
    }
    let run_rows = state.storage().total_runs().await.unwrap_or(0) as usize;
    let run_id: Option<String> = q(
        state,
        "SELECT run_id FROM runs WHERE session_id = ?1 ORDER BY created_at_unix_ms LIMIT 1",
        SESSION.to_string(),
    )
    .await
    .ok()
    .flatten();
    let (status, terminal_reason, recovery_event_reason) = match &run_id {
        Some(id) => (
            q(
                state,
                "SELECT status FROM runs WHERE run_id = ?1",
                id.clone(),
            )
            .await
            .ok()
            .flatten(),
            q(
                state,
                "SELECT terminal_reason FROM runs WHERE run_id = ?1",
                id.clone(),
            )
            .await
            .ok()
            .flatten(),
            q(
                state,
                "SELECT payload_json FROM key_events WHERE run_id = ?1 AND event_type = \
                 'run_state_changed' ORDER BY seq DESC LIMIT 1",
                id.clone(),
            )
            .await
            .ok()
            .flatten(),
        ),
        None => (None, None, None),
    };
    let final_messages: Option<String> = match &run_id {
        Some(id) => q(
            state,
            "SELECT COUNT(*) FROM messages WHERE run_id = ?1",
            id.clone(),
        )
        .await
        .ok()
        .flatten(),
        None => None,
    };
    let journal = match &run_id {
        Some(id) => state
            .storage()
            .load_invocation_journal(&RunId::new(id.clone()))
            .await
            .expect("journal"),
        None => Vec::new(),
    };
    let journal_phases: Vec<String> = journal
        .iter()
        .map(|e| e.phase.wire_name().to_string())
        .collect();
    let journal_receipt_outcome = journal.first().and_then(|e| {
        e.receipt
            .as_ref()
            .map(|r| r.outcome.wire_name().to_string())
    });
    PostFacts {
        run_rows,
        status: status.clone(),
        terminal_reason: terminal_reason.clone(),
        journal_phases: journal_phases.clone(),
        journal_receipt_outcome: journal_receipt_outcome.clone(),
        final_messages: final_messages
            .as_deref()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0),
        recovery_event_reason: recovery_event_reason.clone(),
        trace: serde_json::json!({
            "run_rows": run_rows,
            "run_id": run_id,
            "status": status,
            "terminal_reason": terminal_reason,
            "journal_phases": journal_phases,
            "journal_receipt_outcome": journal_receipt_outcome,
            "final_messages": final_messages,
            "recovery_event_reason": recovery_event_reason,
            "external_executed": external_count(external_dir),
        }),
    }
}

// A tiny helper so bootstrap failures name the case.
trait ExpectOrCase<T> {
    fn expect_or_case(self, case: &str) -> T;
}

impl<T, E: std::fmt::Debug> ExpectOrCase<T> for Result<T, E> {
    fn expect_or_case(self, case: &str) -> T {
        match self {
            Ok(value) => value,
            Err(err) => panic!("case {case}: bootstrap failed: {err:?}"),
        }
    }
}
