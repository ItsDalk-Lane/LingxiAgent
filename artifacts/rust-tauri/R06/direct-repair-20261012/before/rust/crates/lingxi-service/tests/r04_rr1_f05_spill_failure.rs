//! R04-RR1-F05 acceptance (C04): a spill WRITE failure is diagnosed
//! accurately — no ghost "full output" file is ever referenced, and the
//! in-memory result stays honest and distinct from the on-disk
//! availability.
//!
//! This test binary is DELIBERATELY dedicated to the failure injection:
//! the write error is produced by RLIMIT_FSIZE + an ignored SIGXFSZ in
//! THIS process (the controlled disk-full stand-in — `write(2)` returns
//! EFBIG, the same error class ENOSPC produces, once the file passes the
//! limit), which is process-global and therefore must not leak into
//! other test binaries' runs. The chain under test is otherwise the full
//! REAL one: REAL gateway (prepare → execute_prepared), REAL
//! `ProcessTools::run_exec_command`, REAL `ProcessSupervisor` with a
//! REAL `/bin/bash` child and a REAL spill file on disk.
//!
//! Test-double boundary: no model/provider doubles; all state lives in
//! unique temp dirs.

use std::path::PathBuf;
use std::sync::Arc;

use lingxi_kernel::ports::{ToolExecutionResult, ToolOutcome, ToolRequest, ToolRunStatus};
use lingxi_kernel::subagent::SessionPermissionMode;
use lingxi_kernel::toolcatalog::{SchemaBudget, ToolRegistry, ToolTargetId};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ToolCallId};
use lingxi_service::approval_service::ApprovalService;
use lingxi_service::exectools::register_process_tools;
use lingxi_service::inject::SystemClock;
use lingxi_service::procsupervisor::{ProcessSupervisor, SupervisorLimits};
use lingxi_service::resourceaccess::ResourceAccess;
use lingxi_service::toolgateway::{
    CallerSurface, InvocationPermissionContext, ToolInvocationGateway, ToolPolicyPort,
    DEFAULT_LIVE_PREPARED_CAP, DEFAULT_PREPARED_TTL_MS,
};
use serde_json::json;

const SPILL_FILE_LIMIT_BYTES: u64 = 32 * 1024;

fn budget() -> SchemaBudget {
    SchemaBudget::default()
}

struct SpillFailHarness {
    gateway: Arc<ToolInvocationGateway>,
    supervisor: Arc<ProcessSupervisor>,
    exec_target: ToolTargetId,
    #[allow(dead_code)]
    spill_dir: PathBuf,
}

fn test_dir(tag: &str) -> PathBuf {
    static DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "lingxi-r04rr1f05c04-{tag}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        seq
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Installs the controlled write-failure boundary in THIS process:
/// files beyond `SPILL_FILE_LIMIT_BYTES` fail their writes with EFBIG
/// (SIGXFSZ ignored so the error surfaces as an io::Error, the way a
/// full disk would). Returns the previous rlimit for restoration.
fn arm_file_size_limit() -> libc::rlimit {
    // SAFETY: signal disposition and rlimits of this test PROCESS only
    // (this binary exists solely for this injection).
    unsafe {
        let previous_handler = libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        assert_ne!(previous_handler, libc::SIG_ERR, "SIGXFSZ ignore armed");
        let mut old = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        assert_eq!(libc::getrlimit(libc::RLIMIT_FSIZE, &mut old), 0);
        let new_limit = libc::rlimit {
            rlim_cur: SPILL_FILE_LIMIT_BYTES,
            rlim_max: old.rlim_max.max(SPILL_FILE_LIMIT_BYTES),
        };
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &new_limit), 0);
        old
    }
}

/// SAFETY: restores the rlimit captured by [`arm_file_size_limit`].
unsafe fn restore_file_size_limit(old: libc::rlimit) {
    libc::setrlimit(libc::RLIMIT_FSIZE, &old);
}

fn spill_fail_harness(spill_dir: PathBuf) -> SpillFailHarness {
    let root = test_dir("root");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).expect("workspace");
    let limits = SupervisorLimits::with_spill_dir(spill_dir.clone());
    let registry = Arc::new(ToolRegistry::new());
    let access = Arc::new(ResourceAccess::new(std::slice::from_ref(&ws)).expect("access"));
    let approvals = Arc::new(ApprovalService::new(Arc::new(SystemClock)));
    let gateway = Arc::new(ToolInvocationGateway::new(
        Arc::clone(&registry),
        Arc::clone(&approvals) as Arc<dyn ToolPolicyPort>,
        Arc::new(SystemClock),
        budget(),
        DEFAULT_PREPARED_TTL_MS,
        DEFAULT_LIVE_PREPARED_CAP,
    ));
    let supervisor =
        Arc::new(ProcessSupervisor::new(Arc::new(SystemClock), limits).expect("supervisor"));
    let core = register_process_tools(
        &registry,
        gateway.as_ref(),
        Arc::clone(&supervisor),
        Arc::clone(&access),
        ws.clone(),
        None,
        Arc::new(SystemClock),
        &budget(),
    );
    SpillFailHarness {
        gateway,
        supervisor,
        exec_target: core.exec_target,
        spill_dir,
    }
}

fn kernel_ctx(session: &str, run: &str) -> RunContext {
    RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new(session.to_string()),
        run_id: lingxi_protocol::RunId::new(run.to_string()),
        attempt: lingxi_protocol::AttemptId::new(format!("{run}#a1")),
        generation: 1,
    }
}

async fn call_exec(
    h: &SpillFailHarness,
    ctx: &RunContext,
    call: &str,
    args: serde_json::Value,
) -> Result<ToolExecutionResult, lingxi_service::toolgateway::GatewayRefusal> {
    let request = ToolRequest::from_effective_arguments(h.exec_target.as_str(), args, &budget())
        .expect("effective request");
    let call_id = ToolCallId::new(call.to_string());
    let prepared = h.gateway.prepare_from_request(
        ctx,
        CallerSurface::UserRun,
        "agent",
        InvocationPermissionContext::UserSession {
            mode: SessionPermissionMode::Operate,
        },
        &call_id,
        &request,
    )?;
    h.gateway
        .execute_prepared(ctx, &call_id, &prepared.handle)
        .await
}

fn text_of(result: &ToolExecutionResult) -> String {
    match &result.outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn resource_ref_count(result: &ToolExecutionResult) -> usize {
    match &result.outcome {
        ToolOutcome::Success { result } => result.resource_refs.len(),
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

fn exited_code_of(result: &ToolExecutionResult) -> i64 {
    match &result.outcome {
        ToolOutcome::Success { result } => match result.status.as_deref() {
            Some(ToolRunStatus::Exited { code }) => *code,
            other => panic!("expected Exited, got {other:?}"),
        },
        other => panic!("expected a success outcome, got {other:?}"),
    }
}

/// The EFBIG leg: the spill write fails mid-stream (after the limit's
/// worth of bytes) while the process keeps producing output. The result
/// must: keep the in-memory content intact (head AND tail visible),
/// carry NO ResourceRef (the incomplete file is never a full-output
/// reference), and state the failure explicitly.
#[tokio::test]
async fn f05_c04_spill_write_failure_diagnoses_honestly_no_ghost_full_file() {
    let old_limit = arm_file_size_limit();
    let spill_dir = test_dir("efbig-spill");
    std::fs::create_dir_all(&spill_dir).expect("spill dir for the EFBIG leg");
    let h = spill_fail_harness(spill_dir.clone());
    let ctx = kernel_ctx("sess_f05_c04", "run_f05_c04");
    // 128 KiB of single-line output: the spill writes the first 32 KiB,
    // then every further write returns EFBIG. The AFTER_FAILURE marker
    // exists only past the failure point.
    let cmd = "printf 'HEAD_MARK|'; head -c 130000 /dev/zero | tr '\\0' x; \
               printf '|AFTER_FAILURE'";
    let result = call_exec(&h, &ctx, "call-f05c04", json!({"cmd": cmd}))
        .await
        .expect("the command itself succeeds — the failure is spill-side");
    assert_eq!(exited_code_of(&result), 0, "the process ran to completion");
    let text = text_of(&result);
    // In-memory facts intact: BOTH true ends visible.
    assert!(
        text.contains("HEAD_MARK|") && text.contains("|AFTER_FAILURE"),
        "the memory result is unaffected by the spill failure: {text}"
    );
    // No ghost full-output reference.
    assert_eq!(
        resource_ref_count(&result),
        0,
        "a failed spill is never referenced: {text}"
    );
    // The failure is stated, distinguishing memory from disk.
    assert!(
        text.contains("Full output: unavailable"),
        "the availability claim is explicit: {text}"
    );
    assert!(
        text.contains("spill write failed after"),
        "the failure diagnosis names the spill: {text}"
    );
    assert!(
        text.contains("in-memory result is unaffected"),
        "memory vs disk distinguished: {text}"
    );
    // The leftover file exists, is bounded by the failure point, holds
    // the stream's beginning but NOT the post-failure marker.
    let mut spill_files: Vec<PathBuf> = std::fs::read_dir(&spill_dir)
        .expect("spill dir readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert_eq!(spill_files.len(), 1, "exactly one spill leftover");
    let spill_path = spill_files.remove(0);
    let spill_len = std::fs::metadata(&spill_path).map(|m| m.len()).unwrap_or(0);
    assert!(
        spill_len <= SPILL_FILE_LIMIT_BYTES,
        "the failed spill stops at the failure point: {spill_len}"
    );
    let spill_bytes = std::fs::read(&spill_path).expect("leftover readable");
    assert!(
        spill_bytes.windows(10).any(|w| w == b"HEAD_MARK|"),
        "the leftover holds the stream's beginning (partial by fact)"
    );
    assert!(
        !spill_bytes.windows(14).any(|w| w == b"AFTER_FAILURE"),
        "the post-failure output is NOT in the file — it exists only in the \
         in-memory result (output continued after the failure)"
    );
    let _ = h.supervisor.shutdown_all().await;
    // SAFETY: restore this process's previous rlimit.
    unsafe { restore_file_size_limit(old_limit) };
}

/// The open-time failure leg (the 权限 family): a spill directory the
/// supervisor OWNS but where file creation is refused (mode 0o500 — the
/// same-user EACCES shape; the supervisor itself creates its spill dir,
/// so a merely-missing dir cannot occur on this path) yields
/// `spill: None` — the result states "no spill file was kept", keeps the
/// memory content, and references nothing.
#[tokio::test]
async fn f05_c04_unwritable_spill_dir_reports_no_spill_kept() {
    let spill_dir = test_dir("unwritable-spill");
    std::fs::create_dir_all(&spill_dir).expect("spill dir exists");
    std::fs::set_permissions(
        &spill_dir,
        std::os::unix::fs::PermissionsExt::from_mode(0o500),
    )
    .expect("read-only spill dir");
    let h = spill_fail_harness(spill_dir.clone());
    let ctx = kernel_ctx("sess_f05_c04b", "run_f05_c04b");
    let result = call_exec(
        &h,
        &ctx,
        "call-f05c04b",
        json!({"cmd": "printf 'HEAD_MARK|'; head -c 200000 /dev/zero | tr '\\0' x; printf '|TAIL_MARK'"}),
    )
    .await
    .expect("the command runs");
    assert_eq!(exited_code_of(&result), 0);
    let text = text_of(&result);
    assert!(
        text.contains("HEAD_MARK|") && text.contains("|TAIL_MARK"),
        "memory intact: {text}"
    );
    assert!(
        text.contains("Full output: unavailable (no spill file was kept)"),
        "the open-time absence is explicit: {text}"
    );
    assert_eq!(resource_ref_count(&result), 0, "nothing to reference");
    assert!(
        std::fs::read_dir(&spill_dir)
            .map(|d| d.count())
            .unwrap_or(0)
            == 0,
        "no leftover spill file was created"
    );
    let _ = h.supervisor.shutdown_all().await;
    // Restore writability so the teardown can remove the directory.
    let _ = std::fs::set_permissions(
        &spill_dir,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    );
    let _ = std::fs::remove_dir_all(&spill_dir);
}
