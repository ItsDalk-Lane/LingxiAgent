//! R03 repair G05/F06 integration: the EXECUTION payload of a submission
//! is the byte-honest full input — never a truncated projection (the
//! pre-fix code passed `input.chars().take(2000).collect()` into
//! `drive_run`/`spawn_background_drive`, silently dropping the tail of
//! every legal long request while the dedup digest covered the FULL
//! input, so "judged the same request" and "actually executed" diverged).
//!
//! Cases R03-FIX-F06-C01 (boundary lengths + tail-directive integrity,
//! foreground AND background) and R03-FIX-F06-C02 (Unicode / CRLF /
//! combining-character payloads; dedup digest corresponds to the content
//! actually executed; same-prefix-different-tail requests never merge).
//!
//! The ONLY observation channel is the provider double below, which
//! RECORDS the exact input text it receives per model call. The reply is
//! a FIXED constant that carries no information about the input — no
//! assertion in this file may be inferred from reply text. The admission
//! chain, session gate, requestId dedup, run supervisor, task supervisor
//! and the SQLite run database are the REAL production composition (the
//! same shape as the accepted admission_dedup_consistency suite).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::model_exchange::ModelTurnInput;
use lingxi_kernel::ports::{
    ProviderDescriptor, ProviderTurn, ProviderTurnResult, ToolExecutionResult, ToolExecutorPort,
    ToolOutcome, TurnDeltaSink, TurnProviderPort,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};

use lingxi_service::{
    prepare_layout, ExecuteAccepted, ExecuteSubmission, HomeSource, NetworkMode, ServiceConfig,
    ServiceDeps, ServiceState, SessionExecuteError,
};

const NOW_MS: u64 = 1_790_409_600_000;
const FIXED_REPLY: &str = "stub-final (fixed; carries no input information)";

// ── the recording double (records actual inputs; fixed replies) ─────────────

struct RecordingProvider {
    /// EXACT full input text of every model call, in arrival order.
    inputs: std::sync::Mutex<Vec<String>>,
}

impl RecordingProvider {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inputs: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.inputs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn call_count(&self) -> usize {
        self.inputs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

impl TurnProviderPort for RecordingProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "stub.g05-recorder".to_string(),
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
        let input = input.submission.as_str();
        // Record the EXACT bytes received — the only observation channel.
        self.inputs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(input.to_string());
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            ProviderTurnResult::of_ctx(
                &ctx_at_issue,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".to_string(),
                        content: vec![ContentBlock::Text {
                            text: FIXED_REPLY.to_string(),
                        }],
                        model_call_id: None,
                    },
                },
            )
        })
    }
}

/// Tool double that only COUNTS dispatches (the provider script above
/// never requests tools; a nonzero count would prove an unexpected
/// dispatch).
struct CountingTool {
    dispatches: std::sync::atomic::AtomicUsize,
}

impl ToolExecutorPort for CountingTool {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a lingxi_protocol::ToolCallId,
        _request: &'a lingxi_kernel::ports::ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        self.dispatches
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let ctx_at_issue = ctx.clone();
        Box::pin(async move {
            ToolExecutionResult::of_ctx(
                &ctx_at_issue,
                ToolOutcome::success_text("noop".to_string()),
            )
        })
    }
}

// ── harness (the REAL service composition) ──────────────────────────────────

fn synthetic_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r03-g05-fidelity-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn config_for(home: &Path) -> ServiceConfig {
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
        web_session_id: None,
        connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
        credential_kind: lingxi_service::CredentialKind::LoopbackToken,
        trust_state: lingxi_service::TrustState::Local,
        scopes: vec!["chat".to_string()],
    }
}

async fn boot_on(
    home: &Path,
    provider: Arc<RecordingProvider>,
    tool: Arc<CountingTool>,
) -> ServiceState {
    let layout = prepare_layout(home).expect("layout");
    let deps = ServiceDeps {
        turn_provider: Some(provider),
        tool_executor: Some(tool),
        ..ServiceDeps::default()
    };
    ServiceState::bootstrap_with_deps(config_for(home), &layout, deps)
        .await
        .expect("bootstrap")
}

async fn teardown(state: &ServiceState, home: &Path) {
    state.storage().close().await.expect("close storage");
    let _ = std::fs::remove_dir_all(home);
}

async fn submit_fg_plain(
    state: &ServiceState,
    session: &'static str,
    input: &str,
) -> Result<ExecuteAccepted, SessionExecuteError> {
    state
        .sessions()
        .execute_for(
            state.storage().as_ref(),
            state.events(),
            state.runs(),
            &owner_principal(),
            session,
            input,
            NOW_MS,
        )
        .await
}

async fn submit_fg<P: lingxi_kernel::ports::StoragePort>(
    state: &ServiceState,
    port: &P,
    session: &'static str,
    input: &str,
    request_id: Option<&str>,
) -> Result<ExecuteAccepted, SessionExecuteError> {
    let submission = ExecuteSubmission { input, request_id };
    state
        .sessions()
        .execute_submission_for(
            port,
            state.events(),
            state.runs(),
            &owner_principal(),
            session,
            &submission,
            NOW_MS,
        )
        .await
}

async fn submit_bg(
    state: &ServiceState,
    session: &'static str,
    input: &str,
    request_id: &str,
) -> Result<ExecuteAccepted, SessionExecuteError> {
    let submission = ExecuteSubmission {
        input,
        request_id: Some(request_id),
    };
    let storage = Arc::clone(state.storage());
    let events = Arc::clone(state.events());
    let runs = Arc::clone(state.runs());
    let background = Arc::clone(state.background());
    state
        .sessions()
        .execute_background_for(
            &storage,
            &events,
            &runs,
            &background,
            &owner_principal(),
            session,
            &submission,
            NOW_MS,
        )
        .await
}

async fn query_text(state: &ServiceState, sql: &str, arg: &str) -> Option<String> {
    state
        .storage()
        .query_one_text(sql, vec![arg.to_string()])
        .await
        .expect("query")
}

async fn session_runs(state: &ServiceState, session: &str) -> usize {
    query_text(
        state,
        "SELECT COUNT(*) FROM runs WHERE session_id = ?1",
        session,
    )
    .await
    .and_then(|v| v.parse().ok())
    .unwrap_or(0)
}

async fn wait_terminal(state: &ServiceState, run_id: &str) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let status = query_text(state, "SELECT status FROM runs WHERE run_id = ?1", run_id)
            .await
            .unwrap_or_else(|| "missing".to_string());
        if matches!(status.as_str(), "completed" | "failed" | "cancelled") {
            return status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for the background run to settle (status: {status})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
}

// ── C01 payload builders ─────────────────────────────────────────────────────

/// A boundary-length input whose TAIL DIRECTIVE OPPOSES the prefix
/// directive (adversarial: only the actually-delivered tail decides the
/// "real" instruction; the prefix says ALPHA, the tail says OMEGA). The
/// provider double records the raw input, so the assertion needs no
/// reply-text inference at all.
fn boundary_input(len: usize) -> String {
    let tail = format!("|TAIL_DIRECTIVE_OMEGA_{len}_7QXZ_END");
    assert!(
        len > tail.len() + 128,
        "boundary_input needs room for the opposing prefix"
    );
    let mut input = String::new();
    let unit = "IGNORE_TAIL_USE_ONLY_ALPHA_K3. ";
    while input.len() + unit.len() + tail.len() <= len {
        input.push_str(unit);
    }
    while input.len() + tail.len() < len {
        input.push('A');
    }
    input.push_str(&tail);
    assert_eq!(input.chars().count(), len, "exact ASCII char length");
    input
}

/// The long mixed-Unicode payload (C02): Chinese, emoji (non-BMP),
/// combining marks and CRLF, well past 2000 chars.
fn unicode_input(units: usize) -> String {
    let mut out = String::new();
    for i in 0..units {
        out.push_str("中文长输入段落_");
        out.push_str("🦊🚀💡");
        // Combining sequences: e + U+0301, a + U+0308 (NFD forms that
        // MUST survive byte-honest to the provider).
        out.push('e');
        out.push('\u{0301}');
        out.push('a');
        out.push('\u{0308}');
        out.push_str("\r\nCRLF-换行\t");
        out.push_str(&format!("行{i:04}；"));
        out.push('\n');
    }
    out
}

// ── R03-FIX-F06-C01: boundary lengths, tail intact, both entries ────────────

/// Adversarial (C02): a combining sequence STRADDLING the historical
/// 2000-char cut. The pre-fix `chars().take(2000)` projection kept the
/// base letter and DROPPED its combining accent (and everything after),
/// silently changing the text at the boundary. The fix must deliver the
/// decomposed pair intact.
#[tokio::test]
async fn c02_adversarial_combining_pair_straddling_the_historical_cut() {
    let home = synthetic_home("c02-cut-straddle");
    let provider = RecordingProvider::new();
    let tool = Arc::new(CountingTool {
        dispatches: std::sync::atomic::AtomicUsize::new(0),
    });
    let state = boot_on(&home, provider.clone(), tool.clone()).await;

    // Char 2000 is the base 'e', char 2001 its U+0301 accent; the OMEGA
    // tail directive follows — all past the cut the old code applied.
    let mut input = String::with_capacity(2100);
    for _ in 0..1999 {
        input.push('p');
    }
    input.push('e');
    input.push('\u{0301}');
    input.push_str("|TAIL_OMEGA_PAST_THE_CUT_8WK4");
    assert_eq!(input.chars().count(), 1999 + 1 + 1 + 29);
    assert_eq!(input.chars().nth(1999), Some('e'));
    assert_eq!(input.chars().nth(2000), Some('\u{0301}'));

    submit_fg_plain(&state, "sess_local_alpha", &input)
        .await
        .expect("admission");
    assert_eq!(
        provider.calls().last().unwrap(),
        &input,
        "the combining pair at the historical cut and the tail directive \
         after it must arrive intact"
    );
    teardown(&state, &home).await;
}

#[tokio::test]
async fn c01_foreground_boundary_lengths_deliver_the_full_input() {
    let home = synthetic_home("c01-fg");
    let provider = RecordingProvider::new();
    let tool = Arc::new(CountingTool {
        dispatches: std::sync::atomic::AtomicUsize::new(0),
    });
    let state = boot_on(&home, provider.clone(), tool.clone()).await;

    // 1999 / 2000 / 2001 (the old silent cut sat exactly at 2000 chars)
    // and clearly-longer-but-in-budget payloads (3000: bigger than any
    // historical LOG bound; 8000: a genuinely long legal request).
    for len in [1999usize, 2000, 2001, 3000, 8000] {
        let input = boundary_input(len);
        let before = provider.call_count();
        let accepted = submit_fg_plain(&state, "sess_local_alpha", &input)
            .await
            .unwrap_or_else(|err| panic!("len {len} must be admitted, got {err:?}"));
        assert!(!accepted.replayed, "fresh submissions only");
        let calls = provider.calls();
        assert_eq!(calls.len(), before + 1, "exactly one model call for {len}");
        assert_eq!(
            calls.last().unwrap(),
            &input,
            "len {len}: the model turn must receive the FULL input — the OMEGA tail \
             directive is part of the task, not optional decoration"
        );
    }
    assert_eq!(
        session_runs(&state, "sess_local_alpha").await,
        5,
        "exactly five durable runs"
    );
    assert_eq!(
        tool.dispatches.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the recording provider script requests no tools"
    );
    teardown(&state, &home).await;
}

#[tokio::test]
async fn c01_background_boundary_lengths_deliver_the_full_input() {
    let home = synthetic_home("c01-bg");
    let provider = RecordingProvider::new();
    let tool = Arc::new(CountingTool {
        dispatches: std::sync::atomic::AtomicUsize::new(0),
    });
    let state = boot_on(&home, provider.clone(), tool.clone()).await;

    for (idx, len) in [1999usize, 2000, 2001, 3000, 8000].into_iter().enumerate() {
        let input = boundary_input(len);
        let request_id = format!("g05-c01-bg-{idx}");
        let accepted = submit_bg(&state, "sess_local_beta", &input, &request_id)
            .await
            .unwrap_or_else(|err| panic!("len {len} must be admitted, got {err:?}"));
        assert!(!accepted.replayed);
        // The detached drive settles on its own; wait for the durable
        // terminal before the next submission (busy gate).
        let status = wait_terminal(&state, &accepted.run_id).await;
        assert_eq!(status, "completed", "len {len}");
        let calls = provider.calls();
        assert_eq!(
            calls.len(),
            idx + 1,
            "exactly one model call per background submission"
        );
        assert_eq!(
            calls.last().unwrap(),
            &input,
            "len {len}: the BACKGROUND entry must deliver the FULL input too \
             (same rule as the foreground)"
        );
    }
    assert_eq!(session_runs(&state, "sess_local_beta").await, 5);
    assert_eq!(tool.dispatches.load(std::sync::atomic::Ordering::SeqCst), 0);
    teardown(&state, &home).await;
}

// ── R03-FIX-F06-C02: Unicode fidelity + digest correspondence ───────────────

#[tokio::test]
async fn c02_unicode_payloads_byte_honest_on_both_entries() {
    let home = synthetic_home("c02-fidelity");
    let provider = RecordingProvider::new();
    let tool = Arc::new(CountingTool {
        dispatches: std::sync::atomic::AtomicUsize::new(0),
    });
    let state = boot_on(&home, provider.clone(), tool.clone()).await;

    let input = unicode_input(160);
    assert!(
        input.chars().count() > 2000,
        "the payload must be past the historical silent cut"
    );
    // Sanity: the payload really exercises multi-byte, non-BMP and
    // combining sequences.
    assert!(input.contains('🦊'));
    assert!(input.contains("e\u{0301}"));
    assert!(input.contains("\r\n"));

    // Foreground.
    submit_fg_plain(&state, "sess_local_alpha", &input)
        .await
        .expect("foreground unicode admission");
    assert_eq!(
        provider.calls().last().unwrap(),
        &input,
        "foreground: the execution payload is byte-honest — NO normalization \
         (CRLF stays CRLF, combining marks stay decomposed, non-BMP emoji intact)"
    );

    // Background.
    let accepted = submit_bg(&state, "sess_local_alpha", &input, "g05-c02-bg-1")
        .await
        .expect("background unicode admission");
    let status = wait_terminal(&state, &accepted.run_id).await;
    assert_eq!(status, "completed");
    assert_eq!(
        provider.calls().last().unwrap(),
        &input,
        "background: the execution payload is byte-honest — identical rule \
         to the foreground entry"
    );
    assert_eq!(provider.call_count(), 2);
    teardown(&state, &home).await;
}

#[tokio::test]
async fn c02_dedup_digest_corresponds_to_the_content_actually_executed() {
    let home = synthetic_home("c02-digest");
    let provider = RecordingProvider::new();
    let tool = Arc::new(CountingTool {
        dispatches: std::sync::atomic::AtomicUsize::new(0),
    });
    let state = boot_on(&home, provider.clone(), tool.clone()).await;

    // One long mixed payload; the tail carries a unique directive.
    let base = unicode_input(150);
    let tail = "|TAIL_OMEGA_UNIQ_42";
    let first = format!("{base}{tail}");

    let storage = Arc::clone(state.storage());
    let accepted = submit_fg(
        &state,
        storage.as_ref(),
        "sess_local_alpha",
        &first,
        Some("g05-c02-key-1"),
    )
    .await
    .expect("admission with an explicit id");
    assert!(!accepted.replayed);
    assert_eq!(
        provider.calls().last().unwrap(),
        &first,
        "the executed content is the full submission"
    );

    // Same id + byte-identical content → idempotent REPLAY, nothing
    // re-executed (provider count unchanged).
    let replay = submit_fg(
        &state,
        storage.as_ref(),
        "sess_local_alpha",
        &first,
        Some("g05-c02-key-1"),
    )
    .await
    .expect("replay");
    assert!(replay.replayed);
    assert_eq!(replay.run_id, accepted.run_id);
    assert_eq!(provider.call_count(), 1, "replays execute nothing");

    // The DECLARED digest normalization (CRLF→LF only): same logical
    // submission typed on a different platform replays.
    let lf_only = first.replace("\r\n", "\n");
    let replay_crlf = submit_fg(
        &state,
        storage.as_ref(),
        "sess_local_alpha",
        &lf_only,
        Some("g05-c02-key-1"),
    )
    .await
    .expect("CRLF/LF normalization keeps the replay");
    assert!(
        replay_crlf.replayed,
        "CRLF→LF is the declared normalization"
    );
    assert_eq!(provider.call_count(), 1);

    // Adversarial: SAME id, same FIRST 2000 chars, DIFFERENT tail → an
    // explicit DuplicateRequestConflict (the digest covers the FULL
    // input; the two requests are NOT the same and must not merge).
    let conflicting_tail = "|TAIL_OMEGA_UNIQ_43_DIFFERENT";
    let mut conflict_source = first.clone();
    conflict_source.truncate(first.len() - tail.len());
    let conflicting = format!("{conflict_source}{conflicting_tail}");
    assert!(
        conflicting.chars().take(2000).eq(first.chars().take(2000)),
        "setup: both variants share the first 2000 chars"
    );
    assert_ne!(conflicting, first);
    let err = submit_fg(
        &state,
        storage.as_ref(),
        "sess_local_alpha",
        &conflicting,
        Some("g05-c02-key-1"),
    )
    .await
    .expect_err("same id + different full content must conflict");
    assert!(
        matches!(err, SessionExecuteError::DuplicateRequestConflict { .. }),
        "expected DuplicateRequestConflict, got {err:?}"
    );
    assert_eq!(provider.call_count(), 1, "the conflict executes nothing");

    // Two DIFFERENT ids with the same first 2000 chars and different
    // tails → BOTH execute, each with its OWN full content (no
    // cross-merge). This pins "dedup 摘要对应实际执行内容": what the
    // digest judged is exactly what ran.
    let second_variant = format!("{conflict_source}|TAIL_OMEGA_UNIQ_44_SECOND");
    let accepted_b = submit_fg(
        &state,
        storage.as_ref(),
        "sess_local_alpha",
        &second_variant,
        Some("g05-c02-key-2"),
    )
    .await
    .expect("a fresh id admits the sibling content");
    assert!(!accepted_b.replayed);
    assert_ne!(accepted_b.run_id, accepted.run_id);
    let calls = provider.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], first);
    assert_eq!(calls[1], second_variant);
    // The digest difference is observable exactly at the tail (the
    // normalized digests of the executed payloads differ).
    let d_first = lingxi_service::dedup::normalized_request_digest_hex(&calls[0]);
    let d_second = lingxi_service::dedup::normalized_request_digest_hex(&calls[1]);
    let d_conflict = lingxi_service::dedup::normalized_request_digest_hex(&conflicting);
    assert_ne!(d_first, d_second);
    assert_ne!(d_first, d_conflict);
    assert_eq!(
        session_runs(&state, "sess_local_alpha").await,
        2,
        "exactly two durable runs for two distinct full requests"
    );
    teardown(&state, &home).await;
}
