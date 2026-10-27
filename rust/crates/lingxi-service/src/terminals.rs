//! D13 terminal WS subsystem (R08 closeout).
//!
//! The Rust kernel already owns the REAL PTY substrate
//! ([`crate::procsupervisor`]): `exec_command { tty: true }` registers a
//! `PersistentTerminal`, `write_stdin` writes to its master and delivers the
//! transcript, and the supervisor terminates the process group on the
//! explicit close path. This module is the WS-face translation of that
//! substrate: it answers the incumbent terminal frame family
//! (`terminal_snapshot_request` / `terminal_tail_request` /
//! `terminal_close_request`, `shared/terminal-ui-contract.ts`) from REAL
//! session-owned terminal records instead of the previous explicit
//! `terminal_unavailable` capability error.
//!
//! ## Why the transport-only answer was not acceptance
//!
//! The R08 closeout adjudication is explicit: "explicit unavailable is NOT
//! acceptance" for D13. The three leaves require the positive path —
//! snapshot contents, incremental tail output and a REAL termination — so
//! this module implements them against the supervisor instead of claiming a
//! capability gap.
//!
//! ## Authorization chain (shared with every WS business frame)
//!
//! 1. [`crate::session_locator::resolve`] resolves the frame's
//!    `sessionId`/`sessionPath` to ONE session (a mismatch between the two
//!    is a loud `session_identity_mismatch`, mirroring the incumbent
//!    `resolveWsSessionContext` rule);
//! 2. [`sessions::SessionStore::get_for`] authorizes the principal against
//!    that session (`session_not_found` / `session_forbidden`);
//! 3. the terminal list is then filtered by the record's REAL owner
//!    (principal kind+subject AND the authorized session id) — a terminal
//!    that belongs to another session is never listed, never tailed and
//!    never closed, and no content is disclosed on the rejection path.
//!
//! ## Frames (incumbent server shapes — `server/terminal-ws-bridge.ts`)
//!
//! - `terminal_snapshot {type, sessionId, sessionPath, terminals[]}`
//! - `terminal_tail {type, sessionId, sessionPath, terminalId, terminal,
//!   chunks[{seq,data}], sinceSeq, lastSeq, truncated}`
//! - `terminal_close_result {type, requestId?, sessionId, sessionPath,
//!   terminalId, status: "killed"|"already_stopped"|"rejected", reason?}`
//!
//! Errors use the explicit `{type:"error", code, message, sessionId?,
//! sessionPath?}` frame (the incumbent generic WS error shape). A rejected
//! close is never reported as `killed`/`already_stopped`: a termination that
//! was requested but whose exit was not observed is `rejected` with
//! `reason: "termination_unconfirmed"`, matching the supervisor's honest
//! `CleanupTimedOut` receipt.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::auth::Principal;
use crate::procsupervisor::{
    ExitFact, ProcessHandleId, ProcessOwner, ProcessSupervisor, RecordPhase, TerminationOutcome,
    TerminationReason, TerminalRecordView,
};
use crate::sessions::SessionAccess;

/// Incumbent `TERMINAL_TAIL_DEFAULT_MAX_BYTES`.
pub const TERMINAL_TAIL_DEFAULT_MAX_BYTES: usize = 64 * 1024;
/// Incumbent `TERMINAL_TAIL_DEFAULT_MAX_CHUNKS`.
pub const TERMINAL_TAIL_DEFAULT_MAX_CHUNKS: usize = 500;
/// Incumbent `TERMINAL_TAIL_HARD_MAX_BYTES`.
pub const TERMINAL_TAIL_HARD_MAX_BYTES: usize = 256 * 1024;
/// Incumbent `TERMINAL_TAIL_HARD_MAX_CHUNKS`.
pub const TERMINAL_TAIL_HARD_MAX_CHUNKS: usize = 2_000;

/// The authorized requester of one terminal frame — the REAL owner key derived
/// from the authenticated principal (never from the frame payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRequester {
    pub principal_kind: String,
    pub principal_subject: String,
}

impl TerminalRequester {
    pub fn from_principal(principal: &Principal) -> Self {
        let kernel = crate::sessions::kernel_principal_of(principal);
        Self {
            principal_kind: kernel.storage_kind().to_string(),
            principal_subject: kernel.storage_subject(),
        }
    }
}

/// One resolved+authorized session identity a terminal frame is answered for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSessionTarget {
    pub session_id: String,
    /// The canonical Rust locator (`native:<sessionId>`) surfaced back to the
    /// client — the same locator every other Rust WS frame carries.
    pub session_path: String,
}

impl TerminalSessionTarget {
    pub fn new(session_id: impl Into<String>) -> Self {
        let session_id = session_id.into();
        let session_path = crate::session_locator::native(&session_id);
        Self {
            session_id,
            session_path,
        }
    }
}

/// Resolves the frame's session identity and authorizes the principal.
///
/// `Ok(target)` is the only path that reaches terminal records; every
/// `Err` is the explicit error frame to send back (identity/authorization
/// failures never fall through to a content-bearing frame).
pub async fn resolve_target(
    state: &crate::ServiceState,
    principal: &Principal,
    session_id: Option<&str>,
    session_path: Option<&str>,
) -> Result<TerminalSessionTarget, Value> {
    let session_id = normalized(session_id);
    let session_path = normalized(session_path);
    if session_id.is_none() && session_path.is_none() {
        return Err(error_frame(
            "internal_contract",
            "terminal frame requires a session identity (sessionId or sessionPath)",
            None,
        ));
    }
    let resolved = match crate::session_locator::resolve(
        state.storage(),
        session_id,
        session_path,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(_) => {
            return Err(error_frame(
                "session_identity_mismatch",
                "sessionId and sessionPath do not resolve to one session",
                None,
            ))
        }
    };
    let target = TerminalSessionTarget::new(resolved);
    match state.sessions().get_for(principal, &target.session_id).await {
        Ok(SessionAccess::Ok(_)) => Ok(target),
        Ok(SessionAccess::NotFound) => Err(error_frame(
            "session_not_found",
            "session does not exist",
            Some(&target),
        )),
        Ok(SessionAccess::Forbidden) => Err(error_frame(
            "session_forbidden",
            "session is not owned by this principal",
            Some(&target),
        )),
        Err(_) => Err(error_frame(
            "session_identity_unresolved",
            "unable to resolve session ownership",
            Some(&target),
        )),
    }
}

/// The terminal surface over the real process supervisor.
///
/// `None` supervisor means the kernel was bootstrapped without the real tool
/// plane (no workspace/model plane): every frame gets the explicit
/// `terminal_unavailable` answer — never an empty snapshot that would read as
/// "there are no terminals".
pub struct TerminalHub {
    supervisor: Option<Arc<ProcessSupervisor>>,
}

impl TerminalHub {
    pub fn new(supervisor: Option<Arc<ProcessSupervisor>>) -> Self {
        Self { supervisor }
    }

    pub fn available(&self) -> bool {
        self.supervisor.is_some()
    }

    /// Answers a `terminal_snapshot_request` with the current terminal list of
    /// the authorized session.
    pub fn snapshot(
        &self,
        requester: &TerminalRequester,
        target: &TerminalSessionTarget,
    ) -> Value {
        let Some(supervisor) = self.supervisor.as_ref() else {
            return error_frame(
                "terminal_unavailable",
                "the process supervisor is not wired in this kernel configuration",
                Some(target),
            );
        };
        let terminals: Vec<Value> = supervisor
            .persistent_terminals()
            .iter()
            .filter(|record| owner_matches(record, requester, target))
            .map(|record| terminal_entry(record, target))
            .collect();
        json!({
            "type": "terminal_snapshot",
            "sessionId": target.session_id,
            "sessionPath": target.session_path,
            "terminals": terminals,
        })
    }

    /// Answers a `terminal_tail_request`: the increment after `sinceSeq` (or
    /// the recent bounded window without it) from the READ-ONLY terminal log.
    pub fn tail(
        &self,
        requester: &TerminalRequester,
        target: &TerminalSessionTarget,
        terminal_id: Option<&str>,
        since_seq: Option<&Value>,
    ) -> Value {
        let Some(supervisor) = self.supervisor.as_ref() else {
            return error_frame(
                "terminal_unavailable",
                "the process supervisor is not wired in this kernel configuration",
                Some(target),
            );
        };
        let Some(record) = self.owned_terminal(supervisor, requester, target, terminal_id) else {
            return error_frame(
                "terminal_not_found_or_mismatch",
                "terminal does not exist or belongs to another session",
                Some(target),
            );
        };
        let since = normalize_since_seq(since_seq);
        let Some(tail) = supervisor.terminal_tail(
            &record.id,
            since,
            TERMINAL_TAIL_DEFAULT_MAX_BYTES,
            TERMINAL_TAIL_DEFAULT_MAX_CHUNKS,
        ) else {
            return error_frame(
                "terminal_not_found_or_mismatch",
                "terminal transcript is unavailable for this process",
                Some(target),
            );
        };
        let chunks: Vec<Value> = tail
            .chunks
            .iter()
            .map(|chunk| {
                let mut value = json!({ "seq": chunk.seq, "data": chunk.data });
                if chunk.truncated_start {
                    // 单个历史块超过读取字节上限时只返回尾部并明确标记
                    // （incumbent TerminalTranscriptChunk.truncatedStart）。
                    value["truncatedStart"] = json!(true);
                }
                value
            })
            .collect();
        json!({
            "type": "terminal_tail",
            "sessionId": target.session_id,
            "sessionPath": target.session_path,
            "terminalId": record.id.as_str(),
            "terminal": terminal_entry(&record, target),
            "chunks": chunks,
            "sinceSeq": tail.since_seq,
            "lastSeq": tail.last_seq,
            "truncated": tail.truncated,
        })
    }

    /// Answers a `terminal_close_request`: terminate the owned terminal and
    /// report the REAL outcome.
    pub async fn close(
        &self,
        requester: &TerminalRequester,
        target: &TerminalSessionTarget,
        terminal_id: Option<&str>,
        request_id: Option<&str>,
    ) -> Value {
        let Some(supervisor) = self.supervisor.as_ref() else {
            return close_result(
                target,
                terminal_id,
                request_id,
                "rejected",
                Some("terminal_unavailable"),
            );
        };
        let Some(record) = self.owned_terminal(supervisor, requester, target, terminal_id) else {
            return close_result(
                target,
                terminal_id,
                request_id,
                "rejected",
                Some("terminal_not_found_or_mismatch"),
            );
        };
        let receipt = supervisor
            .terminate(&record.id, TerminationReason::Close)
            .await;
        match receipt.outcome {
            // A real exit was observed within the bound: the process is dead.
            TerminationOutcome::Terminated { .. } => close_result(
                target,
                Some(record.id.as_str()),
                request_id,
                "killed",
                None,
            ),
            // Already terminal — nothing was killed; say exactly that.
            TerminationOutcome::AlreadyTerminal(_) => close_result(
                target,
                Some(record.id.as_str()),
                request_id,
                "already_stopped",
                None,
            ),
            // The termination was requested but its exit was NOT observed:
            // rejected with the honest reason, never "stopped".
            TerminationOutcome::CleanupTimedOut => close_result(
                target,
                Some(record.id.as_str()),
                request_id,
                "rejected",
                Some("termination_unconfirmed"),
            ),
            TerminationOutcome::UnknownProcess => close_result(
                target,
                Some(record.id.as_str()),
                request_id,
                "rejected",
                Some("terminal_not_found_or_mismatch"),
            ),
        }
    }

    /// Resolves `terminalId` to a persistent terminal owned by THIS requester
    /// and THIS session. `None` covers unknown / malformed / non-terminal /
    /// foreign handles — the caller turns them into the opaque rejection (no
    /// ownership or content disclosure).
    fn owned_terminal(
        &self,
        supervisor: &Arc<ProcessSupervisor>,
        requester: &TerminalRequester,
        target: &TerminalSessionTarget,
        terminal_id: Option<&str>,
    ) -> Option<TerminalRecordView> {
        let raw = normalized(terminal_id)?;
        let id = ProcessHandleId::parse(raw)?;
        let record = supervisor.persistent_terminal(&id)?;
        owner_matches(&record, requester, target).then_some(record)
    }
}

/// The owner rule: the record must belong to the same principal AND the
/// authorized session. The record's owner facts are trusted spawn context
/// (never model payload — see `ProcessOwner`).
fn owner_matches(
    record: &TerminalRecordView,
    requester: &TerminalRequester,
    target: &TerminalSessionTarget,
) -> bool {
    owner_is(
        &record.owner,
        requester,
        &target.session_id,
    )
}

fn owner_is(
    owner: &ProcessOwner,
    requester: &TerminalRequester,
    session_id: &str,
) -> bool {
    owner.principal_kind == requester.principal_kind
        && owner.principal_subject == requester.principal_subject
        && owner.session_id == session_id
}

/// The public-wire terminal entry (incumbent `TerminalPublicEntry`). Only real
/// record facts are surfaced: unknown values stay null/empty, never invented.
fn terminal_entry(record: &TerminalRecordView, target: &TerminalSessionTarget) -> Value {
    let command = record.argv.join(" ");
    let label: String = command.chars().take(64).collect();
    let (status, exit_code, signal) = terminal_status(&record.phase);
    let mut entry = json!({
        "terminalId": record.id.as_str(),
        "sessionId": target.session_id,
        "sessionPath": target.session_path,
        // The owner record carries no agent id (the D13 surface has no use for
        // one); an empty string is honest "unknown", not a guessed agent.
        "agentId": "",
        "cwd": record.cwd.display().to_string(),
        "command": command,
        "label": label,
        "status": status,
        "seq": record.seq,
        "createdAt": record.spawned_at_unix_ms,
        "lastActivityAt": record.last_activity_at_unix_ms,
        "exitedAt": Value::Null,
        "exitCode": exit_code,
        "signal": signal,
        "transcriptPath": record
            .spill
            .as_ref()
            .map(|spill| spill.path.display().to_string())
            .unwrap_or_default(),
    });
    if !record.owner.tool_call_id.as_str().is_empty() {
        entry["toolCallId"] = json!(record.owner.tool_call_id.as_str());
    }
    entry
}

/// The record phase as the incumbent terminal status vocabulary
/// (`running|exited|killed|stale`).
fn terminal_status(phase: &RecordPhase) -> (&'static str, Option<i64>, Option<String>) {
    match phase {
        RecordPhase::Running | RecordPhase::Terminating { .. } => ("running", None, None),
        RecordPhase::Exited { fact } => ("exited", exit_code_of(*fact), signal_of(*fact)),
        RecordPhase::Terminated { fact, .. } => ("killed", exit_code_of(*fact), signal_of(*fact)),
        // Termination requested, exit NOT observed: "stale" is the incumbent
        // vocabulary for a record whose live state cannot be confirmed — it is
        // never reported as exited/killed.
        RecordPhase::CleanupTimedOut { .. } => ("stale", None, None),
    }
}

fn exit_code_of(fact: ExitFact) -> Option<i64> {
    fact.observed_status_code()
}

fn signal_of(fact: ExitFact) -> Option<String> {
    match fact {
        ExitFact::Signal(signal) => Some(format!("SIG{signal}")),
        _ => None,
    }
}

fn close_result(
    target: &TerminalSessionTarget,
    terminal_id: Option<&str>,
    request_id: Option<&str>,
    status: &str,
    reason: Option<&str>,
) -> Value {
    let mut frame = json!({
        "type": "terminal_close_result",
        "sessionId": target.session_id,
        "sessionPath": target.session_path,
        "terminalId": terminal_id.unwrap_or(""),
        "status": status,
    });
    if let Some(request_id) = request_id {
        frame["requestId"] = json!(request_id);
    }
    if let Some(reason) = reason {
        frame["reason"] = json!(reason);
    }
    frame
}

/// The explicit error frame (incumbent generic WS error shape), carrying the
/// authorized session identity when it is known.
pub fn error_frame(
    code: &str,
    message: &str,
    target: Option<&TerminalSessionTarget>,
) -> Value {
    let mut frame = json!({
        "type": "error",
        "code": code,
        "message": message,
    });
    if let Some(target) = target {
        frame["sessionId"] = json!(target.session_id);
        frame["sessionPath"] = json!(target.session_path);
    }
    frame
}

fn normalized(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Incumbent `normalizedSeq`: undefined/null/""/non-finite → `None` (latest
/// window); otherwise a floored non-negative integer.
fn normalize_since_seq(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    let raw = match value {
        Value::Null => return None,
        Value::Number(number) => number.as_f64(),
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            text.parse::<f64>().ok()
        }
        _ => return None,
    }?;
    if !raw.is_finite() {
        return None;
    }
    Some(raw.max(0.0).floor() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procsupervisor::SupervisorLimits;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn test_supervisor() -> Arc<ProcessSupervisor> {
        let spill = std::env::temp_dir().join(format!(
            "lingxi-terminals-unit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Arc::new(
            ProcessSupervisor::new(
                Arc::new(crate::inject::SystemClock),
                SupervisorLimits::with_spill_dir(spill),
            )
            .expect("test supervisor"),
        )
    }

    fn test_requester() -> TerminalRequester {
        TerminalRequester {
            principal_kind: "local_user".to_string(),
            principal_subject: "user_local".to_string(),
        }
    }

    fn test_runtime() -> tokio::runtime::Runtime {
        // Multi-thread: the pty reader is a background task and must keep
        // running while the synchronous test polls the read-only view.
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn spawn_bash(
        runtime: &tokio::runtime::Runtime,
        supervisor: &Arc<ProcessSupervisor>,
        session_id: &str,
        argv: Vec<String>,
    ) -> ProcessHandleId {
        let spec = crate::procsupervisor::SpawnSpec {
            argv,
            cwd: PathBuf::from("/tmp"),
            env: Default::default(),
            owner: ProcessOwner {
                principal_kind: "local_user".to_string(),
                principal_subject: "user_local".to_string(),
                session_id: session_id.to_string(),
                run_id: "run-unit".to_string(),
                tool_call_id: lingxi_protocol::ToolCallId::new("call-unit".to_string()),
            },
            kind: crate::procsupervisor::ProcessKind::PersistentTerminal,
            cols: 80,
            rows: 24,
        };
        runtime
            .block_on(supervisor.spawn(spec))
            .expect("pty spawn")
            .id
    }

    fn wait_for<F: Fn() -> bool>(what: &str, budget: Duration, cond: F) {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            if cond() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("timeout waiting for {what}");
    }

    #[test]
    fn requester_key_matches_the_run_context_identity() {
        // The frame requester derives the same storage vocabulary the tool
        // plane writes into ProcessOwner (spawn-time trusted context).
        let requester = test_requester();
        assert_eq!(requester.principal_kind, "local_user");
        assert_eq!(requester.principal_subject, "user_local");
    }

    #[test]
    fn snapshot_and_tail_expose_real_pty_output_and_reject_foreign_sessions() {
        let runtime = test_runtime();
        let supervisor = test_supervisor();
        let hub = TerminalHub::new(Some(Arc::clone(&supervisor)));
        let target = TerminalSessionTarget::new("sess-unit");
        let id = spawn_bash(
            &runtime,
            &supervisor,
            "sess-unit",
            vec!["/bin/bash".into(), "-i".into()],
        );
        // A real interactive shell: write through the SAME pty master the
        // write_stdin tool uses, then wait for the read-only view to see it.
        runtime
            .block_on(supervisor.pty_write(&id, b"printf 'R08-TERM-UNIT\\n'\n"))
            .expect("pty write");
        wait_for("pty output", Duration::from_secs(10), || {
            hub.tail(
                &test_requester(),
                &target,
                Some(id.as_str()),
                None,
            )["chunks"]
                .as_array()
                .is_some_and(|chunks| {
                    chunks.iter().any(|chunk| {
                        chunk["data"]
                            .as_str()
                            .is_some_and(|data| data.contains("R08-TERM-UNIT"))
                    })
                })
        });
        let snapshot = hub.snapshot(&test_requester(), &target);
        assert_eq!(snapshot["type"], "terminal_snapshot");
        let terminals = snapshot["terminals"].as_array().expect("terminals array");
        assert_eq!(terminals.len(), 1, "only this session's terminal is listed");
        assert_eq!(terminals[0]["terminalId"], id.as_str());
        assert_eq!(terminals[0]["status"], "running");
        // A read-only tail does not consume the write_stdin delivery cursor:
        // the same window can be read again with identical content, and the
        // write_stdin delivery that follows still carries the output.
        let first = hub.tail(&test_requester(), &target, Some(id.as_str()), None);
        let last_seq = first["lastSeq"].as_u64().expect("lastSeq");
        assert!(last_seq >= 1);
        let again = hub.tail(
            &test_requester(),
            &target,
            Some(id.as_str()),
            Some(&json!(0)),
        );
        assert_eq!(
            again["lastSeq"].as_u64().expect("lastSeq"),
            last_seq,
            "a terminal-panel read must not advance the delivery cursor"
        );
        assert!(again["truncated"].is_boolean());
        let delivery = runtime
            .block_on(supervisor.pty_deliver(&id))
            .expect("delivery");
        assert!(
            delivery.text.contains("R08-TERM-UNIT"),
            "terminal-panel reads must not consume the write_stdin cursor"
        );
        // Another session's identity must not see or tail this terminal.
        let foreign = TerminalSessionTarget::new("sess-other");
        assert_eq!(
            hub.snapshot(&test_requester(), &foreign)["terminals"]
                .as_array()
                .expect("terminals array")
                .len(),
            0
        );
        assert_eq!(
            hub.tail(&test_requester(), &foreign, Some(id.as_str()), None)["type"],
            "error"
        );
        // An unknown terminal id is an explicit rejection, never fabricated.
        assert_eq!(
            hub.tail(&test_requester(), &target, Some("proc:not-a-handle"), None)["type"],
            "error"
        );
        // Real termination: the close receipt reports killed and a second
        // close reports already_stopped (no false "killed" on a dead record).
        let closed = runtime.block_on(hub.close(
            &test_requester(),
            &target,
            Some(id.as_str()),
            Some("unit-req"),
        ));
        assert_eq!(closed["type"], "terminal_close_result");
        assert_eq!(closed["status"], "killed");
        assert_eq!(closed["requestId"], "unit-req");
        let again_closed = runtime.block_on(hub.close(
            &test_requester(),
            &target,
            Some(id.as_str()),
            None,
        ));
        assert_eq!(again_closed["status"], "already_stopped");
        // Closing a foreign session's terminal is rejected without disclosure.
        let id2 = spawn_bash(
            &runtime,
            &supervisor,
            "sess-unit",
            vec!["/bin/bash".into(), "-c".into(), "sleep 30".into()],
        );
        let foreign_close = runtime.block_on(hub.close(
            &test_requester(),
            &foreign,
            Some(id2.as_str()),
            None,
        ));
        assert_eq!(foreign_close["status"], "rejected");
        let unknown_close = runtime.block_on(hub.close(
            &test_requester(),
            &target,
            Some("proc:00000000000000000000000000000000"),
            None,
        ));
        assert_eq!(unknown_close["status"], "rejected");
        runtime
            .block_on(supervisor.terminate(&id2, TerminationReason::Close));
    }

    #[test]
    fn a_wire_without_a_supervisor_is_an_explicit_capability_error() {
        let hub = TerminalHub::new(None);
        let target = TerminalSessionTarget::new("sess-unit");
        let snapshot = hub.snapshot(&test_requester(), &target);
        assert_eq!(snapshot["type"], "error");
        assert_eq!(snapshot["code"], "terminal_unavailable");
        let runtime = test_runtime();
        let closed = runtime.block_on(hub.close(
            &test_requester(),
            &target,
            Some("proc:00000000000000000000000000000000"),
            None,
        ));
        assert_eq!(closed["status"], "rejected");
        assert_eq!(closed["reason"], "terminal_unavailable");
    }

    #[test]
    fn since_seq_normalization_matches_the_incumbent_rule() {
        assert_eq!(normalize_since_seq(None), None);
        assert_eq!(normalize_since_seq(Some(&Value::Null)), None);
        assert_eq!(normalize_since_seq(Some(&json!(""))), None);
        assert_eq!(normalize_since_seq(Some(&json!("abc"))), None);
        assert_eq!(normalize_since_seq(Some(&json!(-3))), Some(0));
        assert_eq!(normalize_since_seq(Some(&json!(4.9))), Some(4));
        assert_eq!(normalize_since_seq(Some(&json!("7"))), Some(7));
    }

    #[test]
    fn terminal_status_never_fabricates_an_exit() {
        let (status, code, signal) = terminal_status(&RecordPhase::CleanupTimedOut {
            reason: TerminationReason::Close,
        });
        assert_eq!(status, "stale");
        assert_eq!(code, None);
        assert_eq!(signal, None);
        let (status, code, _) = terminal_status(&RecordPhase::Terminated {
            reason: TerminationReason::Close,
            fact: ExitFact::Signal(9),
        });
        assert_eq!(status, "killed");
        assert_eq!(code, Some(137));
    }
}
