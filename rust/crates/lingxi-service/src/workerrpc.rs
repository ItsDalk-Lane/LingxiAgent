//! R04-T07: the minimal single-op worker RPC and its controlled host
//! callbacks.
//!
//! A worker is a CHILD PROCESS executing ONE allowlisted capability per
//! invocation (`单操作请求`): spawn → one request line → zero or more
//! controlled callbacks → one result → exit. There is deliberately NO
//! persistent worker session, NO cross-invocation state and NO way for a
//! worker to run an agent loop — every loop-level decision
//! (authorization, approval, model calls, run state) stays in the host.
//!
//! Envelope (JSON lines, length-capped on both sides):
//!
//! ```text
//! host→worker  {"kind":"request","id":"<csprng-hex>","op":...,
//!               "deadline_unix_ms":...,"max_output_bytes":...,
//!               "resources":[{"path":...,"op":"read|write"}],"args":{...}}
//! worker→host  {"kind":"callback","cb_id":...,"op":"model.complete",
//!               "purpose":...,"prompt":...,"max_output_tokens":...}
//! host→worker  {"kind":"callback_result","cb_id":...,"ok":false,
//!               "error":{"code":"model_capability_not_configured",...}}
//! worker→host  {"kind":"result","id":...,"ok":true,
//!               "content":[{"type":"text","text":...}],
//!               "structured":{...},"claimed_files":[...]}
//! host→worker  {"kind":"cancel","id":...}
//! ```
//!
//! Host enforcement (the worker is untrusted):
//! - `id` is host-minted CSPRNG hex, single-use: a result must echo it and
//!   only ONE result is accepted — a second result with the same id is a
//!   protocol violation (ticket reuse), not a credit.
//! - every line is length-capped ([`WorkerLimits`]); an oversized or
//!   malformed line kills the worker.
//! - `claimed_files` are verified against the call's granted scopes (+
//!   existence on disk) before they become local [`ResourceRef`]s — a
//!   worker claiming a path outside its grant is refused loudly (the
//!   forged-local-file-link case), never minted.
//! - model callbacks go through the host's [`WorkerModelPort`]: with no
//!   real ModelGateway (R05) the honest answer is
//!   `model_capability_not_configured` — the worker never receives
//!   credentials, keys or provider handles, and per-invocation budgets
//!   are enforced host-side before any provider would even be consulted.
//! - the environment is a whitelist ([`SAFE_WORKER_ENV_PASSTHROUGH`]) —
//!   the worker never inherits the service environment (credentials) or
//!   `HOME`; the optional T06 [`SandboxPort`] wraps the argv so the OS
//!   boundary, not trust, confines file/network access (同进程 Rust 模块
//!   不是安全沙盒).
//! - concurrency/start/recycle: a bounded semaphore caps simultaneous
//!   workers; spawn failures are loud; every worker is reaped, and the
//!   deadline/cancel/drop paths kill the worker's PROCESS GROUP on Unix
//!   (grandchildren included) with a bounded wait.

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;

use lingxi_kernel::ports::{
    ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess,
};
use lingxi_kernel::toolcatalog::{
    Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolCatalogError, ToolManifest, ToolOrigin, ToolRegistry, ToolTargetId,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{
    ContentBlock, ErrorCode, ProtocolError, ResourceId, ResourceKind, ResourceRef, ToolCallId,
    ToolSchemaDocument,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::inject::ServiceClock;
use crate::resourceaccess::{ResourceAccess, ResourceOp, ResourceScope};
use crate::sandbox::{SandboxCommandRequest, SandboxNetworkRequest, SandboxPort};

/// The ONLY variables a worker child inherits (the same posture as the
/// MCP stdio whitelist — no `HOME`, no service credentials; explicit
/// grant paths travel in the request envelope, not the environment).
pub const SAFE_WORKER_ENV_PASSTHROUGH: &[&str] =
    &["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "TMPDIR"];

/// Wire protocol discriminator.
pub const WORKER_RPC_PROTO: &str = "lingxi-worker-rpc/1";

/// Default cap of one host→worker request line.
pub const WORKER_MAX_REQUEST_BYTES: usize = 256 * 1024;

/// Default cap of one worker→host line.
pub const WORKER_MAX_LINE_BYTES: usize = 256 * 1024;

/// Default wall-clock deadline of one invocation.
pub const WORKER_DEFAULT_DEADLINE_MS: u64 = 60_000;

/// Bounded grace after a cancel/kill before the invocation is settled as
/// unconfirmed.
pub const WORKER_KILL_GRACE_MS: u64 = 2_000;

/// Max model callbacks per invocation (a worker that loops on callbacks
/// is refused at the cap — the budget is host-owned).
pub const WORKER_MAX_CALLBACKS_PER_CALL: u32 = 4;

// ── the host model port (the R05 boundary) ─────────────────────────────────

/// What a worker asks the host's ModelGateway for. The payload carries
/// NO credentials — resolving providers/keys is host-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerModelRequest {
    pub purpose: String,
    pub prompt: String,
    pub max_output_tokens: u32,
}

/// The host's reply to a model callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerModelReply {
    pub text: String,
}

/// Why the host refused a model callback. With no real ModelGateway (the
/// R04 shape) the answer is `CapabilityNotConfigured` — an honest
/// refusal, never a fake completion and never a leaked key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerModelRefusal {
    CapabilityNotConfigured,
    BudgetExceeded { detail: String },
    ProviderRefused { detail: String },
}

impl WorkerModelRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            WorkerModelRefusal::CapabilityNotConfigured => "model_capability_not_configured",
            WorkerModelRefusal::BudgetExceeded { .. } => "model_budget_exceeded",
            WorkerModelRefusal::ProviderRefused { .. } => "model_provider_refused",
        }
    }

    pub fn message(&self) -> String {
        match self {
            WorkerModelRefusal::CapabilityNotConfigured => {
                "no model capability is configured for workers in this build (R05 owns the \
                 real ModelGateway); the worker receives no credentials and no completion"
                    .to_string()
            }
            WorkerModelRefusal::BudgetExceeded { detail } => {
                format!("model callback budget exceeded: {detail}")
            }
            WorkerModelRefusal::ProviderRefused { detail } => {
                format!("model provider refused the callback: {detail}")
            }
        }
    }
}

/// The controlled host callback contract: a worker that needs model
/// capability calls BACK into this port. R05 will implement the real
/// gateway; until then [`UnconfiguredWorkerModel`] is the production
/// shape and refuses everything.
pub trait WorkerModelPort: Send + Sync {
    fn complete(
        &self,
        ctx: &RunContext,
        worker: &str,
        request: &WorkerModelRequest,
    ) -> Result<WorkerModelReply, WorkerModelRefusal>;
}

/// The R04 production shape: no model capability is configured. Every
/// callback is refused honestly.
pub struct UnconfiguredWorkerModel;

impl WorkerModelPort for UnconfiguredWorkerModel {
    fn complete(
        &self,
        _ctx: &RunContext,
        _worker: &str,
        _request: &WorkerModelRequest,
    ) -> Result<WorkerModelReply, WorkerModelRefusal> {
        Err(WorkerModelRefusal::CapabilityNotConfigured)
    }
}

/// The budget-enforcing wrapper: even when a real gateway exists (R05+),
/// per-invocation callback counts and output-token caps are enforced
/// HOST-SIDE before the inner port is consulted.
pub struct BoundedWorkerModel {
    inner: Option<Arc<dyn WorkerModelPort>>,
    max_callbacks: u32,
    max_output_tokens: u32,
    counters: std::sync::Mutex<BTreeMap<String, u32>>,
}

impl BoundedWorkerModel {
    pub fn new(
        inner: Option<Arc<dyn WorkerModelPort>>,
        max_callbacks: u32,
        max_output_tokens: u32,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner,
            max_callbacks,
            max_output_tokens,
            counters: std::sync::Mutex::new(BTreeMap::new()),
        })
    }

    /// Marks a fresh invocation (its callback budget starts at zero).
    pub fn begin_invocation(&self, invocation: &str) {
        self.counters
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(invocation.to_string(), 0);
    }
}

impl WorkerModelPort for BoundedWorkerModel {
    fn complete(
        &self,
        ctx: &RunContext,
        worker: &str,
        request: &WorkerModelRequest,
    ) -> Result<WorkerModelReply, WorkerModelRefusal> {
        // The invocation key is the run/worker identity the call runs
        // under (the executor begins a per-request budget too — the two
        // keys agree because the executor keys on its own request id and
        // passes `worker` = plugin id; the host-side cap holds either
        // way since BOTH are bounded).
        let invocation = format!("{}/{}", ctx.run_id, worker);
        let used = {
            let mut counters = self.counters.lock().unwrap_or_else(|p| p.into_inner());
            let used = counters.entry(invocation.clone()).or_insert(0);
            if *used >= self.max_callbacks {
                return Err(WorkerModelRefusal::BudgetExceeded {
                    detail: format!("{used} callbacks already used (cap {})", self.max_callbacks),
                });
            }
            *used += 1;
            *used
        };
        let _ = used;
        if request.max_output_tokens > self.max_output_tokens {
            return Err(WorkerModelRefusal::BudgetExceeded {
                detail: format!(
                    "requested {} output tokens over the {} cap",
                    request.max_output_tokens, self.max_output_tokens
                ),
            });
        }
        match &self.inner {
            None => Err(WorkerModelRefusal::CapabilityNotConfigured),
            Some(inner) => inner.complete(ctx, worker, request),
        }
    }
}

// ── wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerGrant {
    pub path: String,
    pub op: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerRequestLine {
    pub kind: String,
    pub proto: String,
    pub id: String,
    pub op: String,
    pub deadline_unix_ms: u64,
    pub max_output_bytes: u64,
    pub resources: Vec<WorkerGrant>,
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerCallbackLine {
    pub kind: String,
    pub cb_id: String,
    pub op: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerResultLine {
    pub kind: String,
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub content: Vec<WorkerContentLine>,
    #[serde(default)]
    pub structured: Option<serde_json::Value>,
    #[serde(default)]
    pub claimed_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerContentLine {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub text: String,
}

// ── limits and failures ─────────────────────────────────────────────────────

/// Bounds of one worker invocation (all enforced host-side).
#[derive(Debug, Clone)]
pub struct WorkerLimits {
    pub max_request_bytes: usize,
    pub max_line_bytes: usize,
    pub deadline_ms: u64,
    pub max_callbacks: u32,
    pub max_concurrent_workers: usize,
}

impl Default for WorkerLimits {
    fn default() -> Self {
        WorkerLimits {
            max_request_bytes: WORKER_MAX_REQUEST_BYTES,
            max_line_bytes: WORKER_MAX_LINE_BYTES,
            deadline_ms: WORKER_DEFAULT_DEADLINE_MS,
            max_callbacks: WORKER_MAX_CALLBACKS_PER_CALL,
            max_concurrent_workers: 4,
        }
    }
}

/// Why one worker invocation failed (stable codes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerFailure {
    SpawnFailed {
        detail: String,
    },
    SandboxRefused {
        detail: String,
    },
    ProtocolViolation {
        detail: String,
    },
    WorkerError {
        message: String,
    },
    DeadlineExceeded {
        detail: String,
    },
    ClaimedUnauthorizedPath {
        path: String,
    },
    /// R04-A15: the worker claimed a file as its deliverable and the claim
    /// failed artifact verification (missing / not a regular file /
    /// content contract). The invocation's success is converted to this
    /// explicit failure — the worker ran (side effects may exist), but the
    /// claimed artifact is NOT registered as a valid deliverable.
    ClaimedArtifactInvalid {
        path: String,
        code: &'static str,
        detail: String,
    },
}

impl WorkerFailure {
    pub fn code(&self) -> &'static str {
        match self {
            WorkerFailure::SpawnFailed { .. } => "worker_spawn_failed",
            WorkerFailure::SandboxRefused { .. } => "worker_sandbox_refused",
            WorkerFailure::ProtocolViolation { .. } => "worker_protocol_violation",
            WorkerFailure::WorkerError { .. } => "worker_error",
            WorkerFailure::DeadlineExceeded { .. } => "worker_deadline_exceeded",
            WorkerFailure::ClaimedUnauthorizedPath { .. } => "worker_claimed_unauthorized_path",
            WorkerFailure::ClaimedArtifactInvalid { .. } => "worker_claimed_artifact_invalid",
        }
    }

    pub fn message(&self) -> String {
        match self {
            WorkerFailure::SpawnFailed { detail } => {
                format!("worker spawn failed: {detail}")
            }
            WorkerFailure::SandboxRefused { detail } => {
                format!("worker sandbox refused the spawn: {detail}")
            }
            WorkerFailure::ProtocolViolation { detail } => {
                format!("worker protocol violation: {detail}")
            }
            WorkerFailure::WorkerError { message } => format!("worker error: {message}"),
            WorkerFailure::DeadlineExceeded { detail } => {
                format!("worker deadline exceeded: {detail}")
            }
            WorkerFailure::ClaimedUnauthorizedPath { path } => format!(
                "worker claimed a file outside its granted scopes: {path}; the claim is \
                 refused and no local resource is minted"
            ),
            WorkerFailure::ClaimedArtifactInvalid { path, code, detail } => format!(
                "worker claimed a deliverable that failed artifact verification ({code}): \
                 {path}: {detail}; the worker ran, but nothing is registered as delivered"
            ),
        }
    }

    fn to_protocol_error(&self) -> ProtocolError {
        let code = match self {
            WorkerFailure::SpawnFailed { .. } | WorkerFailure::WorkerError { .. } => {
                ErrorCode::UpstreamUnavailable
            }
            WorkerFailure::SandboxRefused { .. }
            | WorkerFailure::ClaimedUnauthorizedPath { .. } => ErrorCode::Forbidden,
            WorkerFailure::ProtocolViolation { .. } => ErrorCode::InvalidMessage,
            WorkerFailure::DeadlineExceeded { .. } => ErrorCode::UpstreamUnavailable,
            // A fake-success deliverable is a violation of the claimed
            // result contract — InvalidMessage, retryable: false.
            WorkerFailure::ClaimedArtifactInvalid { .. } => ErrorCode::InvalidMessage,
        };
        ProtocolError::new(code, self.message(), false).with_details(serde_json::Map::from_iter([
            ("code".to_string(), serde_json::json!(self.code())),
        ]))
    }
}

// ── the runtime (spawn / env / concurrency / sandbox / kill) ────────────────

/// Owns worker spawning: bounded concurrency, whitelisted env, optional
/// T06 sandbox wrap, Unix process-group kill.
pub struct WorkerRuntime {
    limits: WorkerLimits,
    semaphore: tokio::sync::Semaphore,
    sandbox: Option<Arc<dyn SandboxPort>>,
    clock: Arc<dyn ServiceClock>,
}

impl WorkerRuntime {
    pub fn new(
        limits: WorkerLimits,
        sandbox: Option<Arc<dyn SandboxPort>>,
        clock: Arc<dyn ServiceClock>,
    ) -> Arc<Self> {
        let permits = limits.max_concurrent_workers.max(1);
        Arc::new(Self {
            limits,
            semaphore: tokio::sync::Semaphore::new(permits),
            sandbox,
            clock,
        })
    }

    pub fn limits(&self) -> &WorkerLimits {
        &self.limits
    }

    /// Builds the tokio command for one worker invocation. The sandbox
    /// (when bound) wraps the argv FIRST — a refusal is a loud spawn
    /// failure, never an unwrapped run.
    fn build_command(
        &self,
        argv: &[String],
        env: &BTreeMap<String, String>,
        cwd: &std::path::Path,
    ) -> Result<tokio::process::Command, WorkerFailure> {
        let mut effective = argv.to_vec();
        if let Some(port) = &self.sandbox {
            let wrapped = port
                .wrap(SandboxCommandRequest {
                    argv: effective,
                    network: SandboxNetworkRequest::Contained,
                    cwd: Some(cwd.to_path_buf()),
                })
                .map_err(|refusal| WorkerFailure::SandboxRefused {
                    detail: refusal.to_string(),
                })?;
            effective = wrapped.argv;
        }
        let mut cmd = tokio::process::Command::new(&effective[0]);
        cmd.args(&effective[1..])
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child_env = BTreeMap::new();
        for name in SAFE_WORKER_ENV_PASSTHROUGH {
            if let Ok(value) = std::env::var(name) {
                child_env.insert((*name).to_string(), value);
            }
        }
        child_env.extend(env.clone());
        cmd.env_clear().envs(child_env);
        #[cfg(unix)]
        {
            // Own process group: a bounded kill reaches grandchildren too.
            cmd.process_group(0);
        }
        Ok(cmd)
    }
}

/// RAII ownership of one spawned worker. `kill_on_drop` is the last
/// resort; the explicit paths below are the primary reapers.
struct WorkerChild {
    child: tokio::process::Child,
    killed: bool,
}

impl WorkerChild {
    fn kill_group(&mut self) {
        if self.killed {
            return;
        }
        self.killed = true;
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            // The child is OUR unreaped child: the PID cannot have been
            // reused yet, so a group kill is safe. try_wait() was checked
            // by the caller (or the child is being dropped before wait).
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.start_kill();
        }
    }

    async fn kill_if_alive(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.kill_group();
        }
    }

    async fn wait_bounded(&mut self) -> Option<std::process::ExitStatus> {
        match tokio::time::timeout(
            std::time::Duration::from_millis(WORKER_KILL_GRACE_MS),
            self.child.wait(),
        )
        .await
        {
            Ok(status) => status.ok(),
            Err(_) => None,
        }
    }
}

// ── the tool spec and executor ──────────────────────────────────────────────

/// Everything the host needs to run one worker tool.
#[derive(Clone)]
pub struct WorkerToolSpec {
    /// The plugin namespace of the tool's identity
    /// (`tool:plugin:{plugin_id}:{local_name}`).
    pub plugin_id: String,
    /// The allowlisted single operation the worker executes.
    pub op: String,
    /// Model-facing local name of the tool.
    pub local_name: String,
    pub description: String,
    /// The tool's input schema (paths among the args are derived into
    /// granted scopes through `path_args`).
    pub input_schema: serde_json::Value,
    /// Argument keys whose string values are FILE PATHS subject to the
    /// resource grant.
    pub path_args: Vec<String>,
    /// The worker argv (command + args). No shell, no interpolation.
    pub argv: Vec<String>,
    /// Explicit extra env for the worker child (host-approved).
    pub env: BTreeMap<String, String>,
    /// The cwd of the worker child (inside the authorized workspace; the
    /// base for relative path args).
    pub cwd: std::path::PathBuf,
    /// The model callback port (the R05 boundary face).
    pub model: Arc<dyn WorkerModelPort>,
    /// R04-A15: the content contract every `claimed_files` entry must
    /// satisfy before it becomes a local ResourceRef. `None` = the basic
    /// contract (inside a grant + exists + regular file); a tool that
    /// promises structured output declares the stricter format/minimum.
    pub claimed_file_contract: Option<crate::artifactverify::ClaimedFileContract>,
}

/// The registered worker tool.
pub struct RegisteredWorkerTool {
    pub target: ToolTargetId,
    pub spec: WorkerToolSpec,
}

/// The `ToolExecutorPort` face of one worker tool. Reaches the process
/// ONLY through the T02 gateway (prepare → policy/approval → execute).
pub struct WorkerToolExecutor {
    runtime: Arc<WorkerRuntime>,
    spec: WorkerToolSpec,
    access: Arc<ResourceAccess>,
}

impl WorkerToolExecutor {
    pub fn new(
        runtime: Arc<WorkerRuntime>,
        spec: WorkerToolSpec,
        access: Arc<ResourceAccess>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            spec,
            access,
        })
    }

    /// Executor-side grant derivation (the T05 pattern: the executor
    /// re-derives from the CURRENT filesystem state). Only
    /// host-authorized scopes enter the envelope; claimed files are
    /// checked against the same set.
    fn derive_grants(
        &self,
        ctx: &RunContext,
        arguments: &serde_json::Value,
    ) -> Result<Vec<ResourceScope>, ProtocolError> {
        let mut grants = Vec::new();
        if let Some(map) = arguments.as_object() {
            for key in &self.spec.path_args {
                if let Some(path) = map.get(key).and_then(|v| v.as_str()) {
                    let scope = self
                        .access
                        .authorize(
                            ctx.principal.storage_kind(),
                            &ctx.principal.storage_subject(),
                            ctx.session_id.as_str(),
                            std::path::Path::new(path),
                            &self.spec.cwd,
                            ResourceOp::Read,
                        )
                        .map_err(|refusal| {
                            ProtocolError::new(
                                ErrorCode::Forbidden,
                                format!(
                                    "worker grant refused ({}): {} — the worker tool only \
                                     reads within the authorized workspace",
                                    refusal.code(),
                                    refusal.message
                                ),
                                false,
                            )
                        })?;
                    grants.push(scope);
                }
            }
        }
        Ok(grants)
    }
}

fn mint_request_id() -> Result<String, ProtocolError> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| {
        ProtocolError::new(
            ErrorCode::Internal,
            format!("worker request id entropy failed: {e}"),
            false,
        )
    })?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Reads ONE newline-terminated line while ENFORCING the byte cap DURING
/// the read: a hostile worker streaming an endless line (no newline,
/// gigabytes) cannot push host memory past the cap — the read aborts
/// with [`std::io::ErrorKind::InvalidData`] as soon as the limit is
/// crossed, instead of buffering the whole line first. (A plain
/// `read_line` would allocate whatever the worker cares to send and only
/// count it afterwards — an unbounded host liability.)
async fn read_line_bounded(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    limit: usize,
    line: &mut String,
) -> std::io::Result<usize> {
    let cap_error = |seen: usize| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("line of {seen}+ bytes exceeds the {limit} byte cap"),
        )
    };
    let mut bytes: Vec<u8> = Vec::new();
    let mut total = 0usize;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            // EOF — with or without a trailing newline, the line ends here.
            break;
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(pos) => {
                bytes.extend_from_slice(&available[..=pos]);
                total += pos + 1;
                reader.consume(pos + 1);
                if total > limit {
                    return Err(cap_error(total));
                }
                break;
            }
            None => {
                let len = available.len();
                bytes.extend_from_slice(available);
                total += len;
                reader.consume(len);
                if total > limit {
                    return Err(cap_error(total));
                }
            }
        }
    }
    // A non-UTF-8 line fails JSON parsing downstream as a malformed line
    // (the honest outcome for protocol garbage); lossy decoding never
    // panics.
    line.push_str(&String::from_utf8_lossy(&bytes));
    Ok(total)
}

impl ToolExecutorPort for WorkerToolExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let arguments = request.arguments.as_value().clone();
            let grants = match self.derive_grants(ctx, &arguments) {
                Ok(g) => g,
                Err(e) => {
                    return ToolExecutionResult::of_ctx(ctx, ToolOutcome::Failed { error: e });
                }
            };
            let request_id = match mint_request_id() {
                Ok(id) => id,
                Err(e) => {
                    return ToolExecutionResult::of_ctx(ctx, ToolOutcome::Failed { error: e });
                }
            };
            let limits = self.runtime.limits();
            let now = self.runtime.clock.now_unix_ms();
            let envelope = WorkerRequestLine {
                kind: "request".to_string(),
                proto: WORKER_RPC_PROTO.to_string(),
                id: request_id.clone(),
                op: self.spec.op.clone(),
                deadline_unix_ms: now.saturating_add(limits.deadline_ms),
                max_output_bytes: limits.max_line_bytes as u64,
                resources: grants
                    .iter()
                    .map(|s| WorkerGrant {
                        path: s.path.display().to_string(),
                        op: s.op.wire_name().to_string(),
                    })
                    .collect(),
                args: arguments.clone(),
            };
            let payload = match serde_json::to_string(&envelope) {
                Ok(p) => p,
                Err(e) => {
                    return ToolExecutionResult::of_ctx(
                        ctx,
                        ToolOutcome::Failed {
                            error: ProtocolError::new(
                                ErrorCode::Internal,
                                format!("worker request serialization failed: {e}"),
                                false,
                            ),
                        },
                    );
                }
            };
            if payload.len() > limits.max_request_bytes {
                return ToolExecutionResult::of_ctx(
                    ctx,
                    ToolOutcome::Failed {
                        error: ProtocolError::new(
                            ErrorCode::InvalidMessage,
                            format!(
                                "worker request exceeds the {} byte cap",
                                limits.max_request_bytes
                            ),
                            false,
                        ),
                    },
                );
            }

            // Bounded concurrency: acquire or fail loudly.
            let _permit = self.runtime.semaphore.acquire().await;

            let mut command =
                match self
                    .runtime
                    .build_command(&self.spec.argv, &self.spec.env, &self.spec.cwd)
                {
                    Ok(c) => c,
                    Err(failure) => {
                        return ToolExecutionResult::of_ctx(
                            ctx,
                            ToolOutcome::Failed {
                                error: failure.to_protocol_error(),
                            },
                        );
                    }
                };
            let mut child = match command.spawn() {
                Ok(c) => WorkerChild {
                    child: c,
                    killed: false,
                },
                Err(e) => {
                    return ToolExecutionResult::of_ctx(
                        ctx,
                        ToolOutcome::Failed {
                            error: WorkerFailure::SpawnFailed {
                                detail: format!("{e}"),
                            }
                            .to_protocol_error(),
                        },
                    );
                }
            };
            let mut stdin = child.child.stdin.take().expect("stdin piped");
            let stdout = child.child.stdout.take().expect("stdout piped");
            let mut reader = BufReader::new(stdout);
            let mut callback_count = 0u32;
            let deadline =
                tokio::time::Instant::now() + std::time::Duration::from_millis(limits.deadline_ms);

            // Send the single request line.
            let send = async {
                stdin.write_all(payload.as_bytes()).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await
            };
            if let Err(e) = send.await {
                child.kill_if_alive().await;
                let _ = child.wait_bounded().await;
                return ToolExecutionResult::of_ctx(
                    ctx,
                    ToolOutcome::Unknown {
                        reason: format!(
                            "worker stdin failed after spawn ({e}); the worker may have \
                             started — outcome unconfirmed"
                        ),
                    },
                );
            }

            // Read lines until one accepted result, the deadline, or a
            // protocol violation. Every read is byte-capped MID-READ (a
            // hostile endless line cannot grow host memory past the cap).
            let mut result_line: Option<WorkerResultLine> = None;
            let mut violation: Option<String> = None;
            loop {
                let mut line = String::new();
                match tokio::time::timeout_at(
                    deadline,
                    read_line_bounded(&mut reader, limits.max_line_bytes, &mut line),
                )
                .await
                {
                    Err(_) => {
                        // Deadline: best-effort protocol cancel, bounded
                        // kill, then the honest UNKNOWN (the worker may
                        // have performed side effects).
                        let _ = stdin
                            .write_all(
                                format!("{{\"kind\":\"cancel\",\"id\":\"{request_id}\"}}\n")
                                    .as_bytes(),
                            )
                            .await;
                        let _ = stdin.flush().await;
                        child.kill_if_alive().await;
                        let _ = child.wait_bounded().await;
                        return ToolExecutionResult::of_ctx(
                            ctx,
                            ToolOutcome::Unknown {
                                reason: WorkerFailure::DeadlineExceeded {
                                    detail: format!(
                                        "no result within {}ms; the worker was killed — \
                                         side effects unconfirmed",
                                        limits.deadline_ms
                                    ),
                                }
                                .message(),
                            },
                        );
                    }
                    Ok(Err(e)) if e.kind() == std::io::ErrorKind::InvalidData => {
                        // The line crossed the byte cap DURING the read:
                        // a loud violation, never an unbounded buffer.
                        violation = Some(e.to_string());
                        break;
                    }
                    Ok(Err(e)) => {
                        // stdout died before a result: the worker may
                        // have done work — UNKNOWN, never a clean fail.
                        child.kill_if_alive().await;
                        let _ = child.wait_bounded().await;
                        return ToolExecutionResult::of_ctx(
                            ctx,
                            ToolOutcome::Unknown {
                                reason: format!(
                                    "worker stdout closed before a result ({e}); side \
                                     effects unconfirmed"
                                ),
                            },
                        );
                    }
                    Ok(Ok(0)) => {
                        child.kill_if_alive().await;
                        let _ = child.wait_bounded().await;
                        return ToolExecutionResult::of_ctx(
                            ctx,
                            ToolOutcome::Unknown {
                                reason: "worker exited before sending a result; side \
                                         effects unconfirmed"
                                    .to_string(),
                            },
                        );
                    }
                    Ok(Ok(_n)) => {
                        let parsed: serde_json::Value = match serde_json::from_str(line.trim_end())
                        {
                            Ok(v) => v,
                            Err(e) => {
                                violation = Some(format!("malformed line: {e}"));
                                break;
                            }
                        };
                        match parsed.get("kind").and_then(|k| k.as_str()) {
                            Some("callback") => {
                                let cb: WorkerCallbackLine = match serde_json::from_value(parsed) {
                                    Ok(cb) => cb,
                                    Err(e) => {
                                        violation = Some(format!("malformed callback: {e}"));
                                        break;
                                    }
                                };
                                callback_count += 1;
                                if callback_count > limits.max_callbacks {
                                    violation = Some(format!(
                                        "more than {} model callbacks in one invocation",
                                        limits.max_callbacks
                                    ));
                                    break;
                                }
                                let answer = self.spec.model.complete(
                                    ctx,
                                    &self.spec.plugin_id,
                                    &WorkerModelRequest {
                                        purpose: cb.purpose,
                                        prompt: cb.prompt,
                                        max_output_tokens: cb.max_output_tokens,
                                    },
                                );
                                let reply_line = match answer {
                                    Ok(reply) => serde_json::json!({
                                        "kind": "callback_result",
                                        "cb_id": cb.cb_id,
                                        "ok": true,
                                        "text": reply.text,
                                    }),
                                    Err(refusal) => serde_json::json!({
                                        "kind": "callback_result",
                                        "cb_id": cb.cb_id,
                                        "ok": false,
                                        "error": {
                                            "code": refusal.code(),
                                            "message": refusal.message(),
                                        },
                                    }),
                                };
                                let serialized = serde_json::to_string(&reply_line)
                                    .expect("callback reply serializes");
                                if let Err(e) =
                                    stdin.write_all(format!("{serialized}\n").as_bytes()).await
                                {
                                    violation = Some(format!("callback reply write failed: {e}"));
                                    break;
                                }
                                let _ = stdin.flush().await;
                            }
                            Some("result") => {
                                let result: WorkerResultLine = match serde_json::from_value(parsed)
                                {
                                    Ok(r) => r,
                                    Err(e) => {
                                        violation = Some(format!("malformed result: {e}"));
                                        break;
                                    }
                                };
                                // The id is host-minted CSPRNG per
                                // invocation: a result echoing any other
                                // id (forged, stale from an earlier call)
                                // is refused — the ticket cannot be
                                // reused across calls.
                                if result.id != request_id {
                                    violation = Some(format!(
                                        "result id {:?} does not match the invocation id",
                                        result.id
                                    ));
                                    break;
                                }
                                // FIRST accepted result settles the
                                // invocation: the loop breaks and the
                                // worker is killed right after, so a
                                // second result line can never credit
                                // anything (single-credit semantics by
                                // construction).
                                result_line = Some(result);
                                break;
                            }
                            other => {
                                violation = Some(format!(
                                    "unexpected line kind {other:?} (only callback/result are \
                                     allowed)"
                                ));
                                break;
                            }
                        }
                    }
                }
            }

            // Reap the child before settling (no zombie workers).
            child.kill_if_alive().await;
            let _ = child.wait_bounded().await;
            drop(_permit);

            if let Some(detail) = violation {
                return ToolExecutionResult::of_ctx(
                    ctx,
                    ToolOutcome::Failed {
                        error: WorkerFailure::ProtocolViolation { detail }.to_protocol_error(),
                    },
                );
            }
            let Some(result) = result_line else {
                return ToolExecutionResult::of_ctx(
                    ctx,
                    ToolOutcome::Unknown {
                        reason: "worker invocation ended without a result; side effects \
                                 unconfirmed"
                            .to_string(),
                    },
                );
            };
            if !result.ok {
                return ToolExecutionResult::of_ctx(
                    ctx,
                    ToolOutcome::Failed {
                        error: WorkerFailure::WorkerError {
                            message: result.error.unwrap_or_default(),
                        }
                        .to_protocol_error(),
                    },
                );
            }
            // Claimed files: only artifacts that pass verification become
            // local ResourceRefs — inside a grant, existing, a REGULAR
            // file, and (when the tool declares one) satisfying the
            // claimed-file content contract. Anything else is a refused
            // claim: the worker ran, but nothing is registered as
            // delivered (the forged-local-file-link AND the empty-product
            // cases, R04-A15).
            let contract = self.spec.claimed_file_contract.unwrap_or_default();
            let mut resource_refs = Vec::new();
            for claimed in &result.claimed_files {
                let path = std::path::Path::new(claimed);
                if let Err(rejection) =
                    crate::artifactverify::verify_claimed_artifact(path, &grants, &contract)
                {
                    let failure = match &rejection {
                        crate::artifactverify::ArtifactRejection::OutOfScope { path } => {
                            WorkerFailure::ClaimedUnauthorizedPath { path: path.clone() }
                        }
                        other => WorkerFailure::ClaimedArtifactInvalid {
                            path: claimed.clone(),
                            code: other.code(),
                            detail: other.message(),
                        },
                    };
                    return ToolExecutionResult::of_ctx(
                        ctx,
                        ToolOutcome::Failed {
                            error: failure.to_protocol_error(),
                        },
                    );
                }
                let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                resource_refs.push(ResourceRef {
                    resource_id: ResourceId::new(format!("worker:{request_id}:{}", path.display())),
                    kind: ResourceKind::Artifact,
                    display_name: Some(path.display().to_string()),
                    uri: Some(format!("file://{}", path.display())),
                    digest: None,
                    size_bytes: Some(size),
                });
            }
            let mut content: Vec<ContentBlock> = result
                .content
                .into_iter()
                .map(|block| ContentBlock::Text { text: block.text })
                .collect();
            if content.is_empty() && result.structured.is_none() && resource_refs.is_empty() {
                content.push(ContentBlock::Text {
                    text: "[worker returned no content]".to_string(),
                });
            }
            if let Some(structured) = result.structured {
                content.push(ContentBlock::Text {
                    text: format!(
                        "[worker structured result: {}]",
                        serde_json::to_string(&structured)
                            .unwrap_or_else(|_| "<unserializable>".to_string())
                    ),
                });
            }
            let mut success = ToolSuccess::from_content(content);
            success.resource_refs = resource_refs;
            ToolExecutionResult::of_ctx(ctx, ToolOutcome::Success { result: success })
        })
    }
}

// ── registration (composition entry — production default does NOT call) ─────

/// Registers one worker tool on the registry + gateway. The tool is an
/// Execute-class target in the `ToolOrigin::Plugin` namespace with the
/// CONSERVATIVE recovery classification.
pub fn register_worker_tool(
    registry: &ToolRegistry,
    gateway: &crate::toolgateway::ToolInvocationGateway,
    runtime: &Arc<WorkerRuntime>,
    access: &Arc<ResourceAccess>,
    budget: &SchemaBudget,
    spec: WorkerToolSpec,
) -> Result<RegisteredWorkerTool, ToolCatalogError> {
    let manifest = ToolManifest {
        origin: ToolOrigin::Plugin {
            plugin_id: spec.plugin_id.clone(),
        },
        local_name: spec.local_name.clone(),
        display_name: spec.local_name.clone(),
        aliases: Vec::new(),
        version: "1".to_string(),
        description: spec.description.clone(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema: spec.input_schema.clone(),
        },
        output_schema: None,
        permission: PermissionContract {
            kind: PermissionKind::Execute,
            capability_base: format!("plugin.{}.invoke", spec.local_name),
        },
        availability: Availability::Deferred,
        timeout_ms: None,
        max_concurrency: Some(runtime.limits().max_concurrent_workers as u32),
        declared_permission: DeclaredPermission::Execute,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    };
    let receipt = registry.register(manifest, budget)?;
    let executor = WorkerToolExecutor::new(Arc::clone(runtime), spec.clone(), Arc::clone(access));
    gateway.bind_executor(
        receipt.target_id.clone(),
        executor as Arc<dyn ToolExecutorPort>,
        "R04-T07 worker RPC executor",
    );
    Ok(RegisteredWorkerTool {
        target: receipt.target_id,
        spec,
    })
}

// ── unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_kernel::Principal;
    use lingxi_protocol::{AttemptId, RunId, SessionId};

    fn ctx() -> RunContext {
        RunContext {
            principal: Principal::LocalUser,
            session_id: SessionId::new("s-1"),
            run_id: RunId::new("r-1"),
            attempt: AttemptId::new("a-1"),
            generation: 1,
        }
    }

    struct RecordingModel {
        replies: std::sync::Mutex<Vec<String>>,
    }

    impl WorkerModelPort for RecordingModel {
        fn complete(
            &self,
            _ctx: &RunContext,
            _worker: &str,
            _request: &WorkerModelRequest,
        ) -> Result<WorkerModelReply, WorkerModelRefusal> {
            self.replies.lock().unwrap().push("ok".to_string());
            Ok(WorkerModelReply {
                text: "ok".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn unconfigured_model_refuses_every_callback_honestly() {
        let port = UnconfiguredWorkerModel;
        let refusal = port
            .complete(
                &ctx(),
                "w",
                &WorkerModelRequest {
                    purpose: "summarize".into(),
                    prompt: "p".into(),
                    max_output_tokens: 16,
                },
            )
            .expect_err("unconfigured refuses");
        assert_eq!(refusal, WorkerModelRefusal::CapabilityNotConfigured);
        assert_eq!(refusal.code(), "model_capability_not_configured");
    }

    #[tokio::test]
    async fn bounded_model_enforces_the_callback_budget_host_side() {
        let inner = Arc::new(RecordingModel {
            replies: std::sync::Mutex::new(Vec::new()),
        });
        let port = BoundedWorkerModel::new(Some(inner.clone()), 2, 32);
        let request = WorkerModelRequest {
            purpose: "p".into(),
            prompt: "q".into(),
            max_output_tokens: 8,
        };
        assert!(port.complete(&ctx(), "w", &request).is_ok());
        assert!(port.complete(&ctx(), "w", &request).is_ok());
        let third = port
            .complete(&ctx(), "w", &request)
            .expect_err("cap reached");
        assert!(
            matches!(third, WorkerModelRefusal::BudgetExceeded { .. }),
            "{third:?}"
        );
        assert_eq!(inner.replies.lock().unwrap().len(), 2);
        // Token cap is enforced before the inner port is consulted.
        let oversized = WorkerModelRequest {
            purpose: "p".into(),
            prompt: "q".into(),
            max_output_tokens: 33,
        };
        let refusal = port
            .complete(&ctx(), "w2", &oversized)
            .expect_err("token cap");
        assert!(matches!(refusal, WorkerModelRefusal::BudgetExceeded { .. }));
        assert_eq!(inner.replies.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn bounded_model_without_inner_still_refuses_not_configured() {
        let port = BoundedWorkerModel::new(None, 4, 128);
        let refusal = port
            .complete(
                &ctx(),
                "w",
                &WorkerModelRequest {
                    purpose: "p".into(),
                    prompt: "q".into(),
                    max_output_tokens: 8,
                },
            )
            .expect_err("no inner gateway");
        assert_eq!(refusal, WorkerModelRefusal::CapabilityNotConfigured);
    }

    #[test]
    fn request_envelope_serializes_with_every_enforced_field() {
        let envelope = WorkerRequestLine {
            kind: "request".into(),
            proto: WORKER_RPC_PROTO.into(),
            id: "abc".into(),
            op: "doc.parse".into(),
            deadline_unix_ms: 1_000,
            max_output_bytes: 4096,
            resources: vec![WorkerGrant {
                path: "/tmp/x".into(),
                op: "read".into(),
            }],
            args: serde_json::json!({"a": 1}),
        };
        let line = serde_json::to_string(&envelope).expect("serializes");
        assert!(line.contains("\"deadline_unix_ms\":1000"));
        assert!(line.contains("\"resources\":[{\"path\":\"/tmp/x\",\"op\":\"read\"}]"));
    }
}
