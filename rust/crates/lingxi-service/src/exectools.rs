//! Native `exec_command` / `write_stdin` executors (R04-T05).
//!
//! REAL executors behind the T02 gateway, porting the incumbent
//! semantics (`lib/exec-command/*`, `lib/sandbox/exec-helper.ts`,
//! `lib/terminal/terminal-session-manager.ts`) onto the real
//! [`crate::procsupervisor::ProcessSupervisor`]:
//!
//! # exec_command
//! - Two command forms: structured `argv` (NO shell — the default answer
//!   to "非 shell 操作用结构化 argv") or a shell `cmd` string executed
//!   as `["/bin/bash", "-c", cmd]` (the incumbent's expressive shell
//!   path; safety comes from authorization + the T06 sandbox surface,
//!   NEVER from a keyword blacklist).
//! - `workdir` (default the tool cwd) is judged by the T04
//!   [`crate::resourceaccess::ResourceAccess`] at PREPARE (via the
//!   gateway resource extractor) and RE-CHECKED at the executor — the
//!   cwd of a spawned process is an authorized directory, judged by its
//!   real target (symlinks followed).
//! - Environment: EXPLICIT whitelist passthrough only
//!   ([`SAFE_ENV_PASSTHROUGH`]) plus bounded caller-provided `env`
//!   entries. The server's full environment (tokens, secrets) is NEVER
//!   inherited — the incumbent's `env ?? process.env` is the gap this
//!   closes (registered mapping decision).
//! - One-shot result: bounded output window, head+tail truncation with
//!   the incumbent's notice vocabulary, exit code (`128+signal` for
//!   signal deaths), timeout watchdog (default 120 s, clamped to 600 s
//!   like the incumbent) that runs the supervisor's bounded termination
//!   chain, and a full-output SPILL reference (ResourceRef) when the
//!   output exceeded the window.
//! - `tty: true` starts a PTY terminal and RETURNS IMMEDIATELY with
//!   `ToolRunStatus::Running { handle }` — "started" never masquerades
//!   as completion. The terminal's lifetime is registered as a
//!   persistent terminal (see the supervisor module docs).
//! - A normally-exited command may leave backgrounded grandchildren in
//!   its process group (the incumbent's documented behavior — `cmd &`
//!   survives); the supervisor's audit notes it. CANCELLATION kills the
//!   whole group.
//!
//! # write_stdin
//! - `{ process_id, chars }` — writes `chars` to the terminal's master
//!   (empty `chars` = poll) and delivers the transcript accumulated
//!   since the previous delivery (UTF-8 boundary-safe).
//! - Authorization: the CALLER (trusted run context) must be the
//!   terminal's owning principal AND session — a foreign session's
//!   `write_stdin` is refused (`WRITE_STDIN_NOT_OWNED`); unknown or
//!   malformed handles are refused (`WRITE_STDIN_UNKNOWN_PROCESS`);
//!   non-terminal handles (one-shot commands) are refused
//!   (`WRITE_STDIN_NOT_INTERACTIVE`).
//!
//! # Cancellation wiring (the A09 chain)
//! The executor future runs inside the run driver's CALL-level
//! supervision; a run cancellation drops it at an await point. The
//! [`ProcessOwnershipGuard`] drop guard then initiates the supervisor's
//! `terminate_detached` chain (killpg synchronously, bounded tail on a
//! supervisor-owned task) — process ownership is never lost with the
//! future, and tokio `kill_on_drop` is never the guarantee.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lingxi_kernel::ports::{
    ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolRunStatus, ToolSuccess,
};
use lingxi_kernel::toolcatalog::{
    EffectiveArguments, SchemaBudget, ToolManifest, ToolRegistry, ToolTargetId,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ProtocolError, ResourceId, ResourceKind, ResourceRef, ToolCallId,
    ToolSchemaDocument,
};
use sha2::{Digest as _, Sha256};

use crate::inject::ServiceClock;
use crate::procsupervisor::{
    ExitFact, ProcessHandleId, ProcessKind, ProcessOwner, ProcessSupervisor, RecordPhase,
    SpawnSpec, SpillInfo, TerminationReason,
};
use crate::resourceaccess::{ResourceAccess, ResourceOp, ResourceScope};
use crate::toolgateway::{ResourceExtractionInput, ResourceExtractor, ToolInvocationGateway};

/// Incumbent `EXEC_COMMAND_DEFAULT_TIMEOUT_SECONDS`.
pub const EXEC_DEFAULT_TIMEOUT_SECONDS: u64 = 120;
/// Incumbent `EXEC_COMMAND_MAX_TIMEOUT_SECONDS` (values above clamp).
pub const EXEC_MAX_TIMEOUT_SECONDS: u64 = 600;
/// Incumbent result budget (50 KiB) and line budget (2000).
pub const EXEC_RESULT_MAX_BYTES: usize = 50 * 1024;
pub const EXEC_RESULT_MAX_LINES: usize = 2000;
/// Default PTY dimensions (the incumbent's `cols = 80, rows = 24`).
pub const PTY_DEFAULT_COLS: u16 = 80;
pub const PTY_DEFAULT_ROWS: u16 = 24;
/// Bounded caller-provided env map.
const ENV_MAX_ENTRIES: usize = 32;
const ENV_KEY_MAX_BYTES: usize = 64;
const ENV_VALUE_MAX_BYTES: usize = 8 * 1024;

/// The explicit environment passthrough whitelist. Everything else must
/// come from the caller's `env` argument — the server's environment is
/// never inherited wholesale (secrets included).
pub const SAFE_ENV_PASSTHROUGH: &[&str] = &[
    "PATH", "HOME", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "TZ", "TMPDIR", "SHELL",
];

// ── error vocabulary (stable codes embedded in messages) ───────────────────

pub const EXEC_INVALID_PARAMS: &str = "EXEC_COMMAND_INVALID_PARAMS";
pub const EXEC_SPAWN_FAILED: &str = "EXEC_SPAWN_FAILED";
pub const WRITE_STDIN_PROCESS_ID_REQUIRED: &str = "WRITE_STDIN_PROCESS_ID_REQUIRED";
pub const WRITE_STDIN_UNKNOWN_PROCESS: &str = "WRITE_STDIN_UNKNOWN_PROCESS";
pub const WRITE_STDIN_NOT_OWNED: &str = "WRITE_STDIN_NOT_OWNED";
pub const WRITE_STDIN_NOT_INTERACTIVE: &str = "WRITE_STDIN_NOT_INTERACTIVE";

fn failed(code: ErrorCode, message: String) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(code, message, false),
    }
}

/// A successful text outcome with process status and derived digest.
fn text_success(
    text: String,
    truncated: bool,
    resource_refs: Vec<ResourceRef>,
    status: Option<ToolRunStatus>,
) -> ToolOutcome {
    let content = vec![ContentBlock::Text { text }];
    let value = serde_json::to_value(&content).expect("content blocks serialize");
    let canonical = lingxi_protocol::canon::canonical_json_bytes(&value);
    ToolOutcome::Success {
        result: ToolSuccess {
            content_digest: hex(&Sha256::digest(&canonical)),
            content,
            resource_refs,
            truncated,
            status,
        },
    }
}

fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

// ── parameters ──────────────────────────────────────────────────────────────

/// The validated effective parameters of one exec_command call.
#[derive(Debug, Clone)]
pub struct ExecParams {
    /// The resolved argv (structured form, or `["/bin/bash", "-c", cmd]`).
    pub argv: Vec<String>,
    /// Whether the shell form was used.
    pub shell_form: bool,
    /// The original cmd string (audit).
    pub cmd: Option<String>,
    pub workdir: PathBuf,
    pub timeout_seconds: u64,
    pub timeout_clamped: bool,
    pub max_output_bytes: usize,
    pub env: BTreeMap<String, String>,
    pub tty: bool,
    pub cols: u16,
    pub rows: u16,
}

fn parse_exec_params(args: &serde_json::Value) -> Result<ExecParams, ToolOutcome> {
    let invalid = |detail: String| {
        failed(
            ErrorCode::InvalidMessage,
            format!("{EXEC_INVALID_PARAMS}: {detail}"),
        )
    };
    let cmd = args
        .get("cmd")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let argv = args
        .get("argv")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(|s| s.to_string())
                        .ok_or_else(|| "argv entries must be strings".to_string())
                })
                .collect::<Result<Vec<String>, String>>()
        })
        .transpose()
        .map_err(invalid)?;
    let argv = match (cmd.as_deref(), argv) {
        (Some(cmd), None) if !cmd.trim().is_empty() => {
            vec![
                "/bin/bash".to_string(),
                "-c".to_string(),
                cmd.trim_end().to_string(),
            ]
        }
        (None, Some(argv)) if !argv.is_empty() => argv,
        (Some(_cmd), Some(_)) => {
            return Err(invalid(
                "pass either `cmd` (shell string) or `argv` (structured), not both".to_string(),
            ))
        }
        (Some(_), None) => {
            return Err(invalid("cmd must be a non-empty string".to_string()));
        }
        (None, Some(_)) => {
            return Err(invalid(
                "argv must be a non-empty array of non-empty strings".to_string(),
            ));
        }
        (None, None) => {
            return Err(invalid(
                "exec_command requires either a non-empty cmd string or a non-empty argv array"
                    .to_string(),
            ))
        }
    };
    if argv.iter().any(|a| a.is_empty()) {
        return Err(invalid(
            "argv must be a non-empty array of non-empty strings".to_string(),
        ));
    }
    let workdir = args
        .get("workdir")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let raw_timeout = args.get("timeout_seconds").and_then(|v| v.as_u64());
    let (timeout_seconds, timeout_clamped) = match raw_timeout {
        None | Some(0) => (EXEC_DEFAULT_TIMEOUT_SECONDS, false),
        Some(secs) => {
            if secs > EXEC_MAX_TIMEOUT_SECONDS {
                (EXEC_MAX_TIMEOUT_SECONDS, true)
            } else {
                (secs, false)
            }
        }
    };
    let max_output_bytes = args
        .get("max_output_bytes")
        .and_then(|v| v.as_u64())
        .map(|v| (v as usize).max(1000))
        .unwrap_or(EXEC_RESULT_MAX_BYTES);
    let mut env = BTreeMap::new();
    if let Some(map) = args.get("env").and_then(|v| v.as_object()) {
        if map.len() > ENV_MAX_ENTRIES {
            return Err(invalid(format!(
                "env accepts at most {ENV_MAX_ENTRIES} entries"
            )));
        }
        for (key, value) in map {
            let Some(value) = value.as_str() else {
                return Err(invalid(format!(
                    "env values must be strings (env.{key} is not)"
                )));
            };
            if key.len() > ENV_KEY_MAX_BYTES {
                return Err(invalid(format!(
                    "env key {key:?} exceeds {ENV_KEY_MAX_BYTES} bytes"
                )));
            }
            if value.len() > ENV_VALUE_MAX_BYTES {
                return Err(invalid(format!(
                    "env value for {key:?} exceeds {ENV_VALUE_MAX_BYTES} bytes"
                )));
            }
            if key.is_empty() || key.contains('=') || key.contains('\0') {
                return Err(invalid(format!(
                    "env key {key:?} is empty or contains '=' / NUL"
                )));
            }
            env.insert(key.clone(), value.to_string());
        }
    }
    let tty = args.get("tty").and_then(|v| v.as_bool()).unwrap_or(false);
    let int_of = |field: &str, lo: i64, hi: i64| -> Result<Option<u16>, ToolOutcome> {
        match args.get(field).and_then(|v| v.as_i64()) {
            None => Ok(None),
            Some(v) if (lo..=hi).contains(&v) => Ok(Some(v as u16)),
            Some(v) => Err(invalid(format!(
                "{field} must be an integer within {lo}..={hi} (got {v})"
            ))),
        }
    };
    let cols = int_of("cols", 1, 1024)?.unwrap_or(PTY_DEFAULT_COLS);
    let rows = int_of("rows", 1, 1024)?.unwrap_or(PTY_DEFAULT_ROWS);
    Ok(ExecParams {
        argv,
        shell_form: cmd.is_some(),
        cmd,
        workdir: workdir.unwrap_or_default(),
        timeout_seconds,
        timeout_clamped,
        max_output_bytes,
        env,
        tty,
        cols,
        rows,
    })
}

/// Builds the child environment: whitelist passthrough + caller extras.
/// The caller's extras may override whitelisted keys explicitly.
pub fn build_child_env(extra: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for key in SAFE_ENV_PASSTHROUGH {
        if let Ok(value) = std::env::var(key) {
            env.insert((*key).to_string(), value);
        }
    }
    for (key, value) in extra {
        env.insert(key.clone(), value.clone());
    }
    env
}

// ── output rendering (the incumbent's head+tail truncation) ───────────────

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Largest index `<= wanted` that sits on a UTF-8 character boundary.
fn floor_char_boundary(text: &str, wanted: usize) -> usize {
    let wanted = wanted.min(text.len());
    let mut idx = wanted;
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Smallest index `>= wanted` that sits on a UTF-8 character boundary.
fn ceil_char_boundary(text: &str, wanted: usize) -> usize {
    let mut idx = wanted.min(text.len());
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

/// Decodes the rolling window: a leading partial sequence (the window
/// starts mid-character after evictions) is skipped with a replacement
/// marker — the incumbent's continuation-byte skip.
fn decode_window(window: &[u8]) -> String {
    let mut start = 0usize;
    while start < window.len() && (window[start] & 0xC0) == 0x80 {
        start += 1;
    }
    let mut text = String::from_utf8_lossy(&window[start..]).into_owned();
    if start > 0 && !text.is_empty() {
        let mut prefixed = String::with_capacity(text.len() + 1);
        prefixed.push('\u{FFFD}');
        prefixed.push_str(&text);
        text = prefixed;
    }
    text
}

struct HeadTail {
    content: String,
    truncated: bool,
    total_lines: usize,
    head_lines: usize,
    tail_lines: usize,
}

/// The incumbent `truncateHeadTail`: keep the head and the tail, mark the
/// omitted middle precisely.
fn truncate_head_tail(text: &str, max_lines: usize, max_bytes: usize) -> HeadTail {
    let total_bytes = text.len();
    let lines: Vec<&str> = text.split('\n').collect();
    let total_lines = lines.len();
    if total_lines <= max_lines && total_bytes <= max_bytes {
        return HeadTail {
            content: text.to_string(),
            truncated: false,
            total_lines,
            head_lines: total_lines,
            tail_lines: 0,
        };
    }
    let head_line_budget = (max_lines / 2).max(1);
    let tail_line_budget = max_lines.saturating_sub(head_line_budget).max(1);
    let head_byte_budget = max_bytes / 2;
    let tail_byte_budget = max_bytes - head_byte_budget;

    let mut head_lines: Vec<&str> = Vec::new();
    let mut head_bytes = 0usize;
    for line in lines.iter() {
        if head_lines.len() >= head_line_budget {
            break;
        }
        let line_bytes = line.len() + usize::from(!head_lines.is_empty());
        if head_bytes + line_bytes > head_byte_budget {
            break;
        }
        head_bytes += line_bytes;
        head_lines.push(line);
    }
    let mut tail_lines: Vec<&str> = Vec::new();
    let mut tail_bytes = 0usize;
    for line in lines.iter().rev() {
        if tail_lines.len() >= tail_line_budget {
            break;
        }
        if lines.len() - tail_lines.len() <= head_lines.len() {
            break;
        }
        let line_bytes = line.len() + usize::from(!tail_lines.is_empty());
        if tail_bytes + line_bytes > tail_byte_budget {
            break;
        }
        tail_bytes += line_bytes;
        tail_lines.push(line);
    }
    let tail_start = lines.len() - tail_lines.len();
    if tail_start <= head_lines.len() {
        // The two segments met: the overflow is small — fall back to a
        // byte-split head/tail with a precise omission marker (always on
        // UTF-8 character boundaries).
        let head_part = &text[..floor_char_boundary(text, head_byte_budget)];
        let tail_from = ceil_char_boundary(text, total_bytes.saturating_sub(tail_byte_budget));
        let tail_part = &text[tail_from..];
        let omitted = total_bytes
            .saturating_sub(head_part.len())
            .saturating_sub(tail_part.len());
        let content = format!(
            "{head_part}\n[... {} omitted ...]\n{tail_part}",
            format_size(omitted as u64)
        );
        return HeadTail {
            content,
            truncated: true,
            total_lines,
            head_lines: head_part.split('\n').count(),
            tail_lines: tail_part.split('\n').count(),
        };
    }
    let omitted_lines = tail_start - head_lines.len();
    let omitted_bytes: usize = lines[head_lines.len()..tail_start]
        .iter()
        .map(|l| l.len() + 1)
        .sum();
    let content = format!(
        "{}\n[... {} lines / {} omitted ...]\n{}",
        head_lines.join("\n"),
        omitted_lines,
        format_size(omitted_bytes as u64),
        tail_lines.join("\n")
    );
    HeadTail {
        content,
        truncated: true,
        total_lines,
        head_lines: head_lines.len(),
        tail_lines: tail_lines.len(),
    }
}

fn spill_resource_ref(spill: &SpillInfo) -> ResourceRef {
    ResourceRef {
        resource_id: ResourceId::new(format!(
            "exec-output:{}",
            spill
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )),
        kind: ResourceKind::Artifact,
        display_name: Some("exec_command full output".to_string()),
        uri: Some(format!("file://{}", spill.path.display())),
        digest: None,
        size_bytes: Some(spill.bytes_written),
    }
}

// ── the ownership drop guard ────────────────────────────────────────────────

/// Initiates the supervisor's bounded termination when the executor
/// future is dropped without settling (run cancellation). Disarmed on
/// every normal completion path.
pub struct ProcessOwnershipGuard {
    supervisor: Arc<ProcessSupervisor>,
    id: Option<ProcessHandleId>,
}

impl ProcessOwnershipGuard {
    pub fn new(supervisor: Arc<ProcessSupervisor>, id: ProcessHandleId) -> Self {
        Self {
            supervisor,
            id: Some(id),
        }
    }

    pub fn disarm(&mut self) {
        self.id = None;
    }
}

impl Drop for ProcessOwnershipGuard {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.supervisor
                .terminate_detached(&id, TerminationReason::CallerDropped);
        }
    }
}

// ── the tools ───────────────────────────────────────────────────────────────

pub struct ProcessTools {
    supervisor: Arc<ProcessSupervisor>,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
}

impl ProcessTools {
    pub fn new(
        supervisor: Arc<ProcessSupervisor>,
        access: Arc<ResourceAccess>,
        cwd: PathBuf,
    ) -> Self {
        Self {
            supervisor,
            access,
            cwd,
        }
    }

    pub fn supervisor(&self) -> &Arc<ProcessSupervisor> {
        &self.supervisor
    }

    fn owner_of(ctx: &RunContext, call: &ToolCallId) -> ProcessOwner {
        ProcessOwner {
            principal_kind: ctx.principal.storage_kind().to_string(),
            principal_subject: ctx.principal.storage_subject(),
            session_id: ctx.session_id.to_string(),
            run_id: ctx.run_id.to_string(),
            tool_call_id: call.clone(),
        }
    }

    /// Executor-side cwd re-check (the prepare-time derivation already
    /// authorized the same scope; this is the near-dispatch re-derivation
    /// the file tools perform).
    fn authorize_cwd(
        &self,
        ctx: &RunContext,
        workdir: &Path,
    ) -> Result<ResourceScope, ToolOutcome> {
        let path = if workdir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            workdir
        };
        self.access
            .authorize(
                ctx.principal.storage_kind(),
                &ctx.principal.storage_subject(),
                &ctx.session_id.to_string(),
                path,
                &self.cwd,
                ResourceOp::Read,
            )
            .map_err(|refusal| failed(ErrorCode::Forbidden, format!("{refusal}")))
    }

    pub async fn run_exec_command(
        &self,
        ctx: &RunContext,
        call: &ToolCallId,
        args: &serde_json::Value,
    ) -> ToolOutcome {
        let params = match parse_exec_params(args) {
            Ok(params) => params,
            Err(outcome) => return outcome,
        };
        let cwd_scope = match self.authorize_cwd(ctx, &params.workdir) {
            Ok(scope) => scope,
            Err(outcome) => return outcome,
        };
        let cwd = cwd_scope.path.clone();
        let env = build_child_env(&params.env);
        let owner = Self::owner_of(ctx, call);
        let spec = SpawnSpec {
            argv: params.argv.clone(),
            cwd: cwd.clone(),
            env,
            owner,
            kind: if params.tty {
                ProcessKind::PersistentTerminal
            } else {
                ProcessKind::OneShot
            },
            cols: params.cols,
            rows: params.rows,
        };
        let spawned = match self.supervisor.spawn(spec).await {
            Ok(spawned) => spawned,
            Err(failure) => {
                return failed(
                    ErrorCode::UpstreamUnavailable,
                    format!("{EXEC_SPAWN_FAILED}: {failure}"),
                )
            }
        };
        if params.tty {
            return text_success(
                format!(
                    "Interactive process started (pty).\nprocess_id: {}\nstatus: running\n\
                     cwd: {}\nUse write_stdin (process_id, chars) to interact and read output; \
                     the process keeps running across tool calls until it exits or is closed.",
                    spawned.id,
                    cwd.display()
                ),
                false,
                Vec::new(),
                Some(ToolRunStatus::Running {
                    handle: spawned.id.to_string(),
                }),
            );
        }
        // One-shot: guard ownership, wait for exit or the timeout.
        let mut guard =
            ProcessOwnershipGuard::new(Arc::clone(&self.supervisor), spawned.id.clone());
        let timeout_at =
            tokio::time::Instant::now() + Duration::from_secs(params.timeout_seconds.max(1));
        let phase = tokio::select! {
            biased;
            phase = self.supervisor.wait_terminal(&spawned.id) => phase,
            _ = tokio::time::sleep_until(timeout_at) => {
                let receipt = self
                    .supervisor
                    .terminate(&spawned.id, TerminationReason::Timeout)
                    .await;
                let fact = match receipt.outcome {
                    crate::procsupervisor::TerminationOutcome::Terminated { fact, .. } => fact,
                    _ => ExitFact::Signal(9),
                };
                Some(RecordPhase::Terminated {
                    reason: TerminationReason::Timeout,
                    fact,
                })
            }
        };
        guard.disarm();
        let phase = match phase {
            Some(phase) => phase,
            None => {
                return failed(
                    ErrorCode::Internal,
                    "the process record vanished while waiting for the exit".to_string(),
                )
            }
        };
        let snapshot = self
            .supervisor
            .output_snapshot(&spawned.id)
            .unwrap_or_else(empty_output_snapshot);
        let text = decode_window(&snapshot.window);
        let head_tail = truncate_head_tail(&text, EXEC_RESULT_MAX_LINES, params.max_output_bytes);
        let fact = match &phase {
            RecordPhase::Exited { fact } | RecordPhase::Terminated { fact, .. } => *fact,
            RecordPhase::CleanupTimedOut { .. } => ExitFact::Signal(9),
            _ => ExitFact::Code(-1),
        };
        let timed_out = matches!(
            &phase,
            RecordPhase::Terminated {
                reason: TerminationReason::Timeout,
                ..
            }
        );
        let mut body = if head_tail.content.trim().is_empty() {
            "(no output)".to_string()
        } else {
            head_tail.content.clone()
        };
        let truncated = head_tail.truncated;
        if truncated {
            let notice = format!(
                "\n\n[Showing first {} and last {} of {} lines. Full output: {}]",
                head_tail.head_lines,
                head_tail.tail_lines,
                head_tail.total_lines,
                snapshot
                    .spill
                    .as_ref()
                    .map(|s| s.path.display().to_string())
                    .unwrap_or_else(|| "unavailable".to_string())
            );
            body.push_str(&notice);
        }
        if params.timeout_clamped {
            body.push_str(&format!(
                "\n\n[timeout clamped to {EXEC_MAX_TIMEOUT_SECONDS}s]"
            ));
        }
        let mut resource_refs = Vec::new();
        if let Some(spill) = snapshot.spill.as_ref().filter(|s| s.bytes_written > 0) {
            resource_refs.push(spill_resource_ref(spill));
        }
        if timed_out {
            body.push_str(&format!(
                "\n\nCommand timed out after {} seconds{}. For long-running work pass \
                 timeout=<seconds> (max {EXEC_MAX_TIMEOUT_SECONDS}), or run with tty=true and \
                 continue via write_stdin.",
                params.timeout_seconds,
                if params.timeout_seconds == EXEC_DEFAULT_TIMEOUT_SECONDS {
                    " (default timeout)"
                } else {
                    ""
                }
            ));
            return text_success(
                body,
                truncated,
                resource_refs,
                Some(ToolRunStatus::Exited {
                    code: fact.status_code(),
                }),
            );
        }
        let code = fact.status_code();
        if code != 0 {
            body.push_str(&format!("\n\nCommand exited with code {code}"));
        }
        text_success(
            body,
            truncated,
            resource_refs,
            Some(ToolRunStatus::Exited { code }),
        )
    }

    pub async fn run_write_stdin(&self, ctx: &RunContext, args: &serde_json::Value) -> ToolOutcome {
        let Some(process_id) = args
            .get("process_id")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        else {
            return failed(
                ErrorCode::InvalidMessage,
                format!("{WRITE_STDIN_PROCESS_ID_REQUIRED}: write_stdin requires process_id"),
            );
        };
        // Handle format gate: a forged/garbage id never reaches the registry.
        let Some(id) = ProcessHandleId::parse(&process_id) else {
            return failed(
                ErrorCode::NotFound,
                format!(
                    "{WRITE_STDIN_UNKNOWN_PROCESS}: process_id is not a process handle minted \
                     by this service"
                ),
            );
        };
        let Some(record) = self.supervisor.record(&id) else {
            return failed(
                ErrorCode::NotFound,
                format!(
                    "{WRITE_STDIN_UNKNOWN_PROCESS}: process_id is not a process handle minted \
                     by this service"
                ),
            );
        };
        if !matches!(record.kind, ProcessKind::PersistentTerminal) {
            return failed(
                ErrorCode::InvalidMessage,
                format!(
                    "{WRITE_STDIN_NOT_INTERACTIVE}: process {} is a one-shot command, not an \
                     interactive terminal",
                    process_id
                ),
            );
        }
        // Caller + session authorization (trusted context only).
        if record.owner.principal_kind != ctx.principal.storage_kind()
            || record.owner.principal_subject != ctx.principal.storage_subject()
            || record.owner.session_id != ctx.session_id.to_string()
        {
            return failed(
                ErrorCode::Forbidden,
                format!(
                    "{WRITE_STDIN_NOT_OWNED}: terminal {} belongs to another session; \
                     write_stdin is limited to the session that started the process",
                    process_id
                ),
            );
        }
        let chars = args.get("chars").and_then(|v| v.as_str()).unwrap_or("");
        if !chars.is_empty() {
            if let Err(err) = self.supervisor.pty_write(&id, chars.as_bytes()).await {
                let phase = self.supervisor.phase_of(&id);
                return failed(
                    ErrorCode::Conflict,
                    format!("write_stdin failed: {err} (phase: {phase:?})"),
                );
            }
        }
        let delivery = self
            .supervisor
            .pty_deliver(&id)
            .await
            .expect("the record exists");
        let phase = self.supervisor.phase_of(&id).expect("the record exists");
        let status_line = match &phase {
            RecordPhase::Running | RecordPhase::Terminating { .. } => "running".to_string(),
            RecordPhase::Exited { fact } => format!("exited ({})", fact.describe()),
            RecordPhase::Terminated { reason, fact } => {
                format!("terminated ({}; {})", reason.wire_name(), fact.describe())
            }
            RecordPhase::CleanupTimedOut { reason } => {
                format!("cleanup_timed_out ({}; kill was sent)", reason.wire_name())
            }
        };
        let mut lines = vec![
            format!("process_id: {}", record.id),
            format!("status: {status_line}"),
        ];
        if !delivery.text.is_empty() {
            lines.push(format!("output:\n{}", delivery.text));
        }
        if delivery.truncated {
            lines.push(format!(
                "[transcript truncated: {} undelivered bytes were dropped by the bounded \
                 ring{}]",
                delivery.dropped_undelivered_bytes,
                delivery
                    .spill
                    .as_ref()
                    .map(|s| format!("; full transcript: {}", s.path.display()))
                    .unwrap_or_default()
            ));
        }
        if let Some(spill) = delivery.spill.as_ref().filter(|s| s.bytes_written > 0) {
            lines.push(format!("transcript_path: {}", spill.path.display()));
        }
        let run_status = match &phase {
            RecordPhase::Running | RecordPhase::Terminating { .. } => {
                Some(ToolRunStatus::Running {
                    handle: record.id.to_string(),
                })
            }
            RecordPhase::Exited { fact } | RecordPhase::Terminated { fact, .. } => {
                Some(ToolRunStatus::Exited {
                    code: fact.status_code(),
                })
            }
            RecordPhase::CleanupTimedOut { .. } => Some(ToolRunStatus::Exited { code: 137 }),
        };
        let text = lines.join("\n");
        text_success(text, delivery.truncated, Vec::new(), run_status)
    }
}

// A fresh empty snapshot for the vanished-record edge (never in practice).
fn empty_output_snapshot() -> crate::procsupervisor::OutputSnapshot {
    crate::procsupervisor::OutputSnapshot {
        window: Vec::new(),
        total_bytes: 0,
        stdout_bytes: 0,
        stderr_bytes: 0,
        spill: None,
    }
}

// ── registration (the real process tools) ──────────────────────────────────

/// The registered identities of the native process tools.
#[derive(Clone)]
pub struct CoreProcessTools {
    pub exec_target: ToolTargetId,
    pub write_stdin_target: ToolTargetId,
    pub tools: Arc<ProcessTools>,
}

fn process_manifest(
    local_name: &str,
    capability: &str,
    description: &str,
    schema: serde_json::Value,
) -> ToolManifest {
    use lingxi_kernel::invocation::ToolRecoveryCapability;
    use lingxi_kernel::toolcatalog::{DeclaredPermission, PermissionContract, ToolOrigin};
    ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: local_name.to_string(),
        display_name: local_name.to_string(),
        aliases: Vec::new(),
        version: "1.0.0".to_string(),
        description: description.to_string(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema,
        },
        output_schema: None,
        permission: PermissionContract {
            kind: lingxi_kernel::toolcatalog::PermissionKind::Execute,
            capability_base: capability.to_string(),
        },
        availability: lingxi_kernel::toolcatalog::Availability::Available,
        timeout_ms: None,
        max_concurrency: None,
        declared_permission: DeclaredPermission::Execute,
        recovery: ToolRecoveryCapability::CONSERVATIVE,
    }
}

/// Registers the REAL exec_command / write_stdin tools on the registry
/// and binds their executors on the gateway. Composition entry — the
/// production default bootstrap does NOT call it (no production default
/// changes in R04).
pub fn register_process_tools(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    supervisor: Arc<ProcessSupervisor>,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    _clock: Arc<dyn ServiceClock>,
    budget: &SchemaBudget,
) -> CoreProcessTools {
    let tools = Arc::new(ProcessTools::new(
        Arc::clone(&supervisor),
        Arc::clone(&access),
        cwd.clone(),
    ));
    let exec_manifest = process_manifest(
        "exec_command",
        "exec_command.run",
        "Native Rust exec_command (R04-T05): run a command with structured argv or a shell \
         string; bounded output with head+tail truncation, exit codes, timeout, and PTY \
         terminals via tty=true (returns a process handle — started, not finished)",
        serde_json::json!({
            "type": "object",
            "properties": {
                "cmd": {"type": "string", "description": "Shell command string (executed via /bin/bash -c). Mutually exclusive with argv."},
                "argv": {"type": "array", "items": {"type": "string"}, "description": "Structured argv (no shell). Mutually exclusive with cmd."},
                "workdir": {"type": "string"},
                "timeout_seconds": {"type": "integer", "minimum": 1},
                "max_output_bytes": {"type": "integer", "minimum": 1000},
                "env": {"type": "object"},
                "tty": {"type": "boolean"},
                "cols": {"type": "integer", "minimum": 1, "maximum": 1024},
                "rows": {"type": "integer", "minimum": 1, "maximum": 1024}
            },
            "additionalProperties": false
        }),
    );
    let write_stdin_manifest = process_manifest(
        "write_stdin",
        "write_stdin.io",
        "Native Rust write_stdin (R04-T05): write to an interactive terminal started by \
         exec_command tty=true and read its new output; limited to the session that owns \
         the terminal",
        serde_json::json!({
            "type": "object",
            "properties": {
                "process_id": {"type": "string", "minLength": 1},
                "chars": {"type": "string"}
            },
            "required": ["process_id"],
            "additionalProperties": false
        }),
    );
    let exec_target = registry
        .register(exec_manifest, budget)
        .expect("the exec_command tool registers")
        .target_id;
    let write_stdin_target = registry
        .register(write_stdin_manifest, budget)
        .expect("the write_stdin tool registers")
        .target_id;
    gateway.bind_executor_with_resources(
        exec_target.clone(),
        Arc::new(CoreProcessExecutor {
            tools: Arc::clone(&tools),
            kind: ProcessToolKind::ExecCommand,
        }),
        Some(ProcessTools::resource_extractor(
            Arc::clone(&access),
            cwd.clone(),
        )),
        "R04-T05 native exec_command executor (real processes, process-group cleanup)",
    );
    gateway.bind_executor(
        write_stdin_target.clone(),
        Arc::new(CoreProcessExecutor {
            tools: Arc::clone(&tools),
            kind: ProcessToolKind::WriteStdin,
        }),
        "R04-T05 native write_stdin executor (owner-checked terminal I/O)",
    );
    CoreProcessTools {
        exec_target,
        write_stdin_target,
        tools,
    }
}

impl ProcessTools {
    /// The prepare-time resource derivation: the workdir (or the tool
    /// cwd) must be an authorized directory (Read op — a process runs
    /// FROM there).
    pub fn resource_extractor(access: Arc<ResourceAccess>, cwd: PathBuf) -> ResourceExtractor {
        Arc::new(
            move |input: &ResourceExtractionInput, args: &EffectiveArguments| {
                let workdir = args
                    .as_value()
                    .get("workdir")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| cwd.clone());
                let scope = access
                    .authorize(
                        input.principal_kind,
                        &input.principal_subject,
                        &input.session_id,
                        &workdir,
                        &cwd,
                        ResourceOp::Read,
                    )
                    .map_err(|refusal| format!("{refusal}"))?;
                Ok(vec![scope])
            },
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ProcessToolKind {
    ExecCommand,
    WriteStdin,
}

/// One executor bound to one target. Dispatches through the T02 gateway
/// ONLY (boundary check B1).
pub struct CoreProcessExecutor {
    tools: Arc<ProcessTools>,
    kind: ProcessToolKind,
}

impl ToolExecutorPort for CoreProcessExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let tools = Arc::clone(&self.tools);
        let kind = self.kind;
        let ctx = ctx.clone();
        let call = call.clone();
        let args = request.arguments.as_value().clone();
        Box::pin(async move {
            let outcome = match kind {
                ProcessToolKind::ExecCommand => tools.run_exec_command(&ctx, &call, &args).await,
                ProcessToolKind::WriteStdin => tools.run_write_stdin(&ctx, &args).await,
            };
            ToolExecutionResult::of_ctx(&ctx, outcome)
        })
    }
}
