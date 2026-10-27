//! R07-T07-δ8 workflow 编排侧——现役 `lib/tools/workflow-tool.ts` +
//! `lib/workflow/host-api.ts`（宿主半）+ `lib/workflow/journal.ts` +
//! `lib/workflow/run-limits.ts` + `lib/workflow/node-folder-scope.ts` 的端口；
//! 沙箱内半（bootstrap/host API/runtime contract）在 `workflowjs.rs`。
//!
//! 结构：`WorkflowToolExecutor` 只做静态校验与派单（runId 即回，后台执行）；
//! `Orchestrator::drive_script` 是后台主体——专用 OS 线程跑 boa 沙箱，主运行时
//! 经 `HostRequest` 通道受理 agent()/workflow()/log()/phase()/step；节点 =
//! 父会话上的子 run（`RunGrant::Subagent`，spawn_child 同款许可/作用域/
//! spawn_linked 仪式，减 thread store）。journal / 无进展 watchdog / 总时长 /
//! budget / maxTotal backstop / 重试分类全部按现役语义平移；「报什么就是什么」
//! （failWith 先置理由再 abort）。偏差与不可达分支登记于
//! docs/rust-tauri/R07/T07_STATUS.md δ8 节（含 _syncRun 结构不可达、写作用域
//! structured_output 工具通道映射、ActivityHub 不移植）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use lingxi_kernel::ports::{
    StoragePort, ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess,
};
use lingxi_kernel::subagent::{
    resolve_subagent_access, AccessRequest, RunLineage, RunOrigin, SessionPermissionMode,
    ToolAccessTier,
};
use lingxi_kernel::toolcatalog::{
    Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolManifest, ToolOrigin, ToolRegistry,
};
use lingxi_kernel::usage::ModelUsageQuery;
use lingxi_kernel::{Principal, RunContext, RunFinish};
use lingxi_protocol::{
    ContentBlock, ErrorCode, ProtocolError, RunId, ToolCallId, ToolSchemaDocument,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{Notify, Semaphore};

use lingxi_adapters::storage::RunDatabase;

use crate::appevents::AppEventLog;
use crate::cancel::{CancelScope, ScopeKind};
use crate::events::EventService;
use crate::inject::ServiceClock;
use crate::runs::{ChatRunInput, DriveAuthorization, RunGrant, RunSupervisor, WorkflowNodeScope};
use crate::session_supervisor::SessionSupervisor;
use crate::subagents::SubagentRuntime;
use crate::toolgateway::ToolInvocationGateway;
use crate::workflowjs::{self, HostRequest, SandboxGlobals};

// ── 常量（现役逐字） ─────────────────────────────────────────────────────

/// 现役 concurrency.ts 调用点字面量（createLimiter 的 maxTotal 兜底）。
const AGENT_TOTAL_BACKSTOP: u64 = 1000;
/// 现役 sandbox.ts raceDeadline 的 slack：脚本 promise 收尾宽限。
const SCRIPT_DEADLINE_SLACK_MS: u64 = 60_000;
/// 现役 host-api.ts MAX_NODE_RETRIES。
const MAX_NODE_RETRIES: u64 = 5;

/// 现役 workflow-tool.ts WORKFLOW_DESCRIPTION（10 行逐字，模型契约不改）。
const WORKFLOW_DESCRIPTION: &str = "Run a deterministic JavaScript orchestration script that delegates all real work to workflow agent() nodes.\nUse this for controlled fan-out, cross-verification, staged synthesis, or dynamic loops where each item must be handled.\nThe script must start with: export const meta = { name: string, description: string }.\nAvailable globals: agent(prompt, opts), parallel(thunks), pipeline(items, ...stages), workflow(script, args), phase(title), log(message), budget, args.\nagent() signature is agent(prompt, { label?, model?, agentType?, access?: \"read\"|\"write\", writeFolders?, schema?, toolFilter?, retries? }).\nRuntime model: per-node timeout 15min (transient node failures retry twice with backoff; override with agent() retries / limits.nodeRetries), the whole run fails only when NO node makes progress for 10min or after 4h total; on failure the message carries resumeFromRunId so a fixed re-dispatch replays completed nodes from cache. Concurrency defaults to 16 (limits.maxConcurrent, max 64).\nLeast-privilege rule: every node must either run read-only (access:\"read\") or declare writeFolders — the narrowest absolute existing folders it writes to. Write-capable nodes without writeFolders are rejected at start. writeFolders entries must stay inside the parent session folder scope; the first entry becomes the node cwd. Parallel write nodes should declare disjoint writeFolders so they cannot clobber each other. Reads keep following the sandbox global read-only contract.\nAlways await agent(): const result = await agent('task prompt', { access: 'read', agentType: 'lingxi' }); agent() does not return { result }.\nTo choose a target agent, use opts.agentType. Do not pass task in opts; put complete task instructions in the first prompt argument.\nThe script cannot import modules or access require/process/fs/net. To read/write files or run tools, ask an agent() node to do it.";

// ── task 身份（现役 lib/tasks/task-identity.ts mint） ────────────────────

fn base36(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(char::from_digit((n % 36) as u32, 36).expect("radix 36 digit"));
        n /= 36;
    }
    digits.iter().rev().collect()
}

/// 现役 defaultRandom：Math.random().toString(36).slice(2,8) —— 6 个 base36
/// 字符。随机源换系统 CSPRNG（现役本就非确定，形状一致即可）；熵源失败
/// 响亮拒绝派单（不静默降级）。
fn rand6() -> Result<String, String> {
    let mut buf = [0u8; 6];
    getrandom::getrandom(&mut buf)
        .map_err(|err| format!("workflow task id 需要系统随机源: {err}"))?;
    Ok(buf
        .iter()
        .map(|b| char::from_digit((b % 36) as u32, 36).expect("radix 36 digit"))
        .collect())
}

/// task_{kind}_{base36 now}_{base36 seq}_{rand6}；kind 归一化小写去非
/// [a-z0-9]→"gen"（normalizeKind 平移）。
fn mint_task_id(kind: &str, now_ms: u64, seq: u64, rand: &str) -> String {
    let slug: String = kind
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let slug = if slug.is_empty() {
        "gen"
    } else {
        slug.as_str()
    };
    format!("task_{slug}_{}_{}_{}", base36(now_ms), base36(seq), rand)
}

// ── run limits（现役 run-limits.ts normalizeRunLimits/CLAMPS） ───────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunLimits {
    pub node_timeout_ms: u64,
    pub idle_timeout_ms: u64,
    pub total_timeout_ms: u64,
    pub max_concurrent: usize,
    pub node_retries: u64,
}

const DEFAULT_RUN_LIMITS: RunLimits = RunLimits {
    node_timeout_ms: 15 * 60_000,
    idle_timeout_ms: 10 * 60_000,
    total_timeout_ms: 4 * 60 * 60_000,
    max_concurrent: 16,
    node_retries: 2,
};

/// Math.floor + clamp；非有限数字/非数字 → 默认（现役逐键语义）。
fn clamp_floor(value: Option<&Value>, lo: u64, hi: u64, default: u64) -> u64 {
    let Some(v) = value.and_then(Value::as_f64) else {
        return default;
    };
    if !v.is_finite() {
        return default;
    }
    v.floor().clamp(lo as f64, hi as f64) as u64
}

pub(crate) fn normalize_run_limits(raw: Option<&Value>) -> RunLimits {
    let source = raw.filter(|v| v.is_object());
    let get = |key: &str| source.and_then(|o| o.get(key));
    RunLimits {
        node_timeout_ms: clamp_floor(
            get("nodeTimeoutMs"),
            60_000,
            60 * 60_000,
            DEFAULT_RUN_LIMITS.node_timeout_ms,
        ),
        idle_timeout_ms: clamp_floor(
            get("idleTimeoutMs"),
            60_000,
            60 * 60_000,
            DEFAULT_RUN_LIMITS.idle_timeout_ms,
        ),
        total_timeout_ms: clamp_floor(
            get("totalTimeoutMs"),
            5 * 60_000,
            12 * 60 * 60_000,
            DEFAULT_RUN_LIMITS.total_timeout_ms,
        ),
        max_concurrent: clamp_floor(
            get("maxConcurrent"),
            1,
            64,
            DEFAULT_RUN_LIMITS.max_concurrent as u64,
        ) as usize,
        node_retries: clamp_floor(get("nodeRetries"), 0, 5, DEFAULT_RUN_LIMITS.node_retries),
    }
}

// ── 节点失败重试分类（现役 NON_RETRYABLE_CODES / NON_RETRYABLE_PATTERNS） ──

const NON_RETRYABLE_CODES: [&str; 8] = [
    "WRITE_FOLDERS_INVALID",
    "WRITE_FOLDERS_CONFLICT_WITH_READ",
    "WRITE_FOLDERS_REQUIRED",
    "WRITE_FOLDER_NOT_FOUND",
    "WRITE_FOLDER_NOT_DIRECTORY",
    "WRITE_FOLDER_OUTSIDE_PARENT_SCOPE",
    "PARENT_SCOPE_UNAVAILABLE",
    "SUBAGENT_WRITE_DENIED_BY_PARENT_READ_ONLY",
];

fn non_retryable_patterns() -> &'static [regex::Regex] {
    static PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let case_insensitive = regex::RegexBuilder::new(r"已中止|被中止|\baborted\b")
            .case_insensitive(true)
            .build()
            .expect("static regex");
        let rest = [
            r"预算耗尽",
            r"超出 agent 总数上限",
            r"unsupported option",
            r"access must be",
            r"agentType .* (was not found|cannot be resolved)",
            r"writeFolders",
            r"requires a non-empty prompt",
        ]
        .into_iter()
        .map(|p| regex::Regex::new(p).expect("static regex"));
        std::iter::once(case_insensitive).chain(rest).collect()
    })
}

/// 现役 isRetryableNodeError：默认可重试（未知错误按瞬时故障）；命中 code
/// 表或文案正则 → 不可重试。
pub(crate) fn is_retryable_node_error(message: &str, code: Option<&str>) -> bool {
    if let Some(code) = code {
        if NON_RETRYABLE_CODES.contains(&code) {
            return false;
        }
    }
    !non_retryable_patterns()
        .iter()
        .any(|re| re.is_match(message))
}

/// 退避：第 1 次约 5s、之后约 20s，±20% jitter（宿主侧允许随机）。
pub(crate) fn retry_delay_ms(attempt: u64) -> u64 {
    let base: u64 = if attempt <= 1 { 5_000 } else { 20_000 };
    let jitter = base as f64 * 0.2;
    let mut buf = [0u8; 8];
    let r = if getrandom::getrandom(&mut buf).is_ok() {
        u64::from_le_bytes(buf) as f64 / u64::MAX as f64
    } else {
        // 熵源失败不退化流程：取 jitter 中点并留痕（显式标注的降级）。
        tracing::warn!("workflow retry jitter: system entropy unavailable, using midpoint");
        0.5
    };
    (base as f64 - jitter + r * jitter * 2.0).round().max(0.0) as u64
}

// ── journal（现役 journal.ts 全量平移） ──────────────────────────────────

#[derive(Debug, Clone)]
struct JournalEntry {
    key: String,
    status: String,
    result: Value,
}

/// 断点文件属于文本状态；拒绝超大或稀疏损坏输入，不按不可信长度分配内存。
const MAX_JOURNAL_BYTES: u64 = 8 * 1024 * 1024;

/// 断点续跑日志：JSONL 按 nodeSeq 索引；`invalidated_after` 初值 = 现役
/// Infinity（u64::MAX 等价）。path 为 None = 纯内存（现役同）。
#[derive(Clone, PartialEq, Eq)]
struct JournalSnapshot {
    identity: crate::filetools::fsio::FileIdentity,
    sha256: String,
}

impl JournalSnapshot {
    fn receipt(&self) -> Value {
        #[cfg(unix)]
        let file_id = json!({"device":self.identity.dev,"inode":self.identity.ino});
        #[cfg(windows)]
        let file_id = Value::Null;
        json!({"size":self.identity.size,"sha256":self.sha256,"fileId":file_id,"modified":format!("{:?}",self.identity.modified)})
    }
}

pub(crate) struct WorkflowJournal {
    path: Option<PathBuf>,
    file: Option<std::fs::File>,
    expected: Option<JournalSnapshot>,
    entries: HashMap<u64, JournalEntry>,
    invalidated_after: u64,
    replay_hits: u64,
}

impl WorkflowJournal {
    pub(crate) fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            file: None,
            expected: None,
            entries: HashMap::new(),
            invalidated_after: u64::MAX,
            replay_hits: 0,
        }
    }

    /// 恢复涉及真实副作用，缺失、损坏和 IO 失败必须拒绝，不能悄悄全重跑。
    pub(crate) fn load(path: PathBuf) -> Result<Self, String> {
        let (snapshot, bytes) = Self::snapshot(&path, None)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|error| format!("workflow journal is not UTF-8: {error}"))?;
        let mut journal = Self::new(Some(path));
        journal.expected = Some(snapshot);
        for (index, line) in text.lines().enumerate() {
            if line.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line).map_err(|error| {
                format!("workflow journal line {} is corrupt: {error}", index + 1)
            })?;
            let seq = value
                .get("nodeSeq")
                .and_then(Value::as_u64)
                .ok_or("workflow journal node sequence is missing")?;
            let key = value
                .get("key")
                .and_then(Value::as_str)
                .filter(|key| !key.is_empty())
                .ok_or("workflow journal key is missing")?;
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .filter(|status| matches!(*status, "ok" | "error"))
                .ok_or("workflow journal status is invalid")?;
            let result = value
                .get("result")
                .cloned()
                .ok_or("workflow journal result is missing")?;
            journal.entries.insert(
                seq,
                JournalEntry {
                    key: key.into(),
                    status: status.into(),
                    result,
                },
            );
        }
        Ok(journal)
    }

    /// 从真实文件句柄读取，并再次核对路径仍指向同一份文件。
    fn snapshot(path: &Path, limit: Option<u64>) -> Result<(JournalSnapshot, Vec<u8>), String> {
        use crate::filetools::fsio::{DirFd, FileIdentity};
        use std::io::Read as _;
        let parent = path.parent().ok_or("workflow journal parent is missing")?;
        let name = path.file_name().ok_or("workflow journal name is missing")?;
        let parent = parent.canonicalize().map_err(|error| error.to_string())?;
        let dir = DirFd::open(&parent).map_err(|error| error.to_string())?;
        let open = || {
            dir.open_attachment_nofollow(name)
                .ok_or_else(|| "workflow journal is missing".to_string())?
                .map_err(|error| error.to_string())
        };
        let mut file = open()?;
        if !file
            .metadata()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            return Err("workflow journal is not a regular file".into());
        }
        let before = FileIdentity::of(&file).map_err(|error| error.to_string())?;
        if before.size > MAX_JOURNAL_BYTES {
            return Err("workflow journal exceeds the 8 MiB safety limit".into());
        }
        if limit.is_some_and(|limit| before.size > limit) {
            return Err("workflow journal length changed unexpectedly".into());
        }
        let mut bytes = Vec::new();
        // 多读一个字节即可发现追加竞态，避免被无限增长的文件拖住清理。
        file.by_ref()
            .take(before.size.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        let after = FileIdentity::of(&file).map_err(|error| error.to_string())?;
        let at_path = FileIdentity::of(&open()?).map_err(|error| error.to_string())?;
        if before != after || after != at_path || bytes.len() as u64 != after.size {
            return Err("workflow journal changed while being read".into());
        }
        Ok((
            JournalSnapshot {
                identity: after,
                sha256: lingxi_kernel::session_tree::sha256_hex(&bytes),
            },
            bytes,
        ))
    }

    fn initialize(&mut self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .append(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("workflow journal creation failed: {error}"))?;
        file.sync_all().map_err(|error| error.to_string())?;
        let (snapshot, _) = Self::snapshot(path, Some(0))?;
        if crate::filetools::fsio::FileIdentity::of(&file).map_err(|error| error.to_string())?
            != snapshot.identity
        {
            return Err("workflow journal was replaced during creation".into());
        }
        self.file = Some(file);
        self.expected = Some(snapshot);
        Ok(())
    }

    fn confirmed_receipt(&self) -> Result<Value, String> {
        let Some(path) = &self.path else {
            return Ok(Value::Null);
        };
        let expected = self
            .expected
            .as_ref()
            .ok_or("workflow journal is not initialized")?;
        let (current, _) = Self::snapshot(path, Some(expected.identity.size))?;
        if &current != expected {
            return Err("workflow journal continuity is unconfirmed".into());
        }
        Ok(current.receipt())
    }

    /// 命中 → Some(result)；未命中/key 漂移/status!=ok →
    /// invalidated_after = seq-1 后返 None（该位置及之后全部重跑）。
    pub(crate) fn try_replay(&mut self, node_seq: u64, key: &str) -> Option<Value> {
        if node_seq > self.invalidated_after {
            return None;
        }
        match self.entries.get(&node_seq) {
            Some(entry) if entry.key == key && entry.status == "ok" => {
                self.replay_hits += 1;
                Some(entry.result.clone())
            }
            _ => {
                self.invalidated_after = node_seq.saturating_sub(1);
                None
            }
        }
    }

    /// 写盘确认后才进入缓存；失败保留原始文件并明确阻止自动恢复。
    pub(crate) fn record(
        &mut self,
        node_seq: u64,
        key: &str,
        result: Value,
        status: &str,
        extra: Option<Value>,
        now_ms: u64,
    ) -> Result<(), String> {
        let mut line = serde_json::Map::new();
        line.insert("nodeSeq".into(), json!(node_seq));
        line.insert("key".into(), json!(key));
        line.insert("result".into(), result.clone());
        line.insert("status".into(), json!(status));
        line.insert("ts".into(), json!(now_ms));
        if let Some(Value::Object(map)) = extra {
            line.extend(map);
        }
        if let Some(path) = &self.path {
            use std::io::Write as _;
            let expected = self
                .expected
                .as_ref()
                .ok_or("workflow journal is not initialized")?;
            let (current, mut content) = Self::snapshot(path, Some(expected.identity.size))?;
            if &current != expected {
                return Err("workflow journal was deleted, truncated, replaced or modified".into());
            }
            let file = self
                .file
                .as_mut()
                .ok_or("workflow journal has no original write handle")?;
            if crate::filetools::fsio::FileIdentity::of(file).map_err(|error| error.to_string())?
                != expected.identity
            {
                return Err("workflow journal original handle no longer matches".into());
            }
            let mut bytes =
                serde_json::to_vec(&Value::Object(line)).map_err(|error| error.to_string())?;
            bytes.push(b'\n');
            if expected.identity.size.saturating_add(bytes.len() as u64) > MAX_JOURNAL_BYTES {
                return Err("workflow journal append exceeds the 8 MiB safety limit".into());
            }
            file.write_all(&bytes)
                .and_then(|()| file.sync_data())
                .map_err(|error| error.to_string())?;
            content.extend_from_slice(&bytes);
            let (after, _) = Self::snapshot(path, Some(content.len() as u64))?;
            if after.identity
                != crate::filetools::fsio::FileIdentity::of(file)
                    .map_err(|error| error.to_string())?
                || after.identity.size != content.len() as u64
                || after.sha256 != lingxi_kernel::session_tree::sha256_hex(&content)
            {
                return Err("workflow journal continuity was lost during append".into());
            }
            self.expected = Some(after);
        }
        self.entries.insert(
            node_seq,
            JournalEntry {
                key: key.into(),
                status: status.into(),
                result,
            },
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn replay_hits(&self) -> u64 {
        self.replay_hits
    }
    pub(crate) fn total_entries(&self) -> usize {
        self.entries.len()
    }
    pub(crate) fn has_entries(&self) -> bool {
        !self.entries.is_empty()
    }
}

/// sha256hex(payload)[:16]——payload 串由 bootstrap 用真 JSON.stringify
/// 构造（{p, o: sanitized}，与现役逐字节同形），原生只哈希。
pub(crate) fn compute_journal_key(payload: &str) -> String {
    let digest = Sha256::digest(payload.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

// ── 写作用域校验（现役 node-folder-scope.ts 的原生半：存在性/包含；
//    shape 与 default-deny 已由 bootstrap 在调用点同步拒绝） ─────────────

#[derive(Debug)]
pub(crate) struct FolderScopeError {
    pub code: &'static str,
    pub message: String,
}

impl FolderScopeError {
    fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }
}

/// realpath 失败退回词法 resolve（现役 realOrNull ?? path.resolve；只影响
/// 包含判断从而 fail-closed，不放大权限）。
fn real_or_resolve(raw: &str) -> PathBuf {
    std::fs::canonicalize(raw).unwrap_or_else(|_| normalize_lexical(raw))
}

fn normalize_lexical(raw: &str) -> PathBuf {
    let path = Path::new(raw);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut out = PathBuf::new();
    for part in absolute.components() {
        use std::path::Component;
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 父会话写根：cwd + workspace_folders + authorized_folders，realpath-or-
/// resolve（登记映射：现役只取 sandboxFolders，Rust 侧父 scope 由会话
/// 元数据 + 授权目录承担，见 δ8 设计节）。
fn parent_folder_roots(
    cwd: Option<&str>,
    workspace: &[String],
    authorized: &[String],
) -> Vec<PathBuf> {
    let mut raw: Vec<&str> = Vec::new();
    if let Some(cwd) = cwd {
        raw.push(cwd);
    }
    raw.extend(workspace.iter().map(String::as_str));
    raw.extend(authorized.iter().map(String::as_str));
    raw.into_iter()
        .filter(|s| !s.trim().is_empty())
        .map(real_or_resolve)
        .collect()
}

/// resolveNodeFolderScope 平移（错误码逐字；dedupe/收窄不落节点沙箱——
/// 登记偏差，校验仍 fail-closed）。`originals` 是消息回填用的未 trim
/// 原文（现役 `${raw}` 语义）。
fn assert_write_scope_contained(
    folders: &[String],
    originals: &[String],
    parent_roots: &[PathBuf],
) -> Result<(), FolderScopeError> {
    if parent_roots.is_empty() {
        return Err(FolderScopeError::new(
            "PARENT_SCOPE_UNAVAILABLE",
            "workflow agent() writeFolders requires the parent session folder scope, which is unavailable here.".to_string(),
        ));
    }
    for (trimmed, raw) in folders.iter().zip(originals.iter()) {
        let real = std::fs::canonicalize(trimmed).map_err(|_| {
            FolderScopeError::new(
                "WRITE_FOLDER_NOT_FOUND",
                format!("workflow agent() writeFolders entry does not exist: {raw}"),
            )
        })?;
        let is_dir = std::fs::metadata(&real)
            .map(|m| m.is_dir())
            .unwrap_or(false);
        if !is_dir {
            return Err(FolderScopeError::new(
                "WRITE_FOLDER_NOT_DIRECTORY",
                format!("workflow agent() writeFolders entry is not a directory: {raw}"),
            ));
        }
        if !parent_roots.iter().any(|base| real.starts_with(base)) {
            return Err(FolderScopeError::new(
                "WRITE_FOLDER_OUTSIDE_PARENT_SCOPE",
                format!("workflow agent() writeFolders entry escapes the parent session folder scope: {raw}"),
            ));
        }
    }
    Ok(())
}

/// 原生侧复验（脚本可直达 __native_*，校验以原生为准）：shape 已在
/// bootstrap 同步拒绝过一遍，这里 fail-closed 复跑全套。
fn validate_write_folders(value: &Value, parent_roots: &[PathBuf]) -> Result<(), FolderScopeError> {
    let Some(list) = value.as_array() else {
        return Err(FolderScopeError::new(
            "WRITE_FOLDERS_INVALID",
            "workflow agent() writeFolders must be a non-empty array of absolute folder paths."
                .to_string(),
        ));
    };
    if list.is_empty() {
        return Err(FolderScopeError::new(
            "WRITE_FOLDERS_INVALID",
            "workflow agent() writeFolders must be a non-empty array of absolute folder paths."
                .to_string(),
        ));
    }
    let mut trimmed_list = Vec::with_capacity(list.len());
    let mut originals = Vec::with_capacity(list.len());
    for raw in list {
        let invalid = || {
            FolderScopeError::new(
                "WRITE_FOLDERS_INVALID",
                format!(
                    "workflow agent() writeFolders entries must be absolute paths; got: {}",
                    serde_json::to_string(raw).unwrap_or_default()
                ),
            )
        };
        let Some(s) = raw.as_str() else {
            return Err(invalid());
        };
        let trimmed = s.trim();
        if trimmed.is_empty() || !Path::new(trimmed).is_absolute() {
            return Err(invalid());
        }
        trimmed_list.push(trimmed.to_string());
        originals.push(s.to_string());
    }
    assert_write_scope_contained(&trimmed_list, &originals, parent_roots)
}

// ── 并发闸（现役 createLimiter：信号量 + total 只增不减的 backstop） ─────

struct WorkflowLimiter {
    sem: Arc<Semaphore>,
    total: AtomicU64,
    max_total: u64,
}

impl WorkflowLimiter {
    fn new(max_concurrent: usize, max_total: u64) -> Self {
        Self {
            sem: Arc::new(Semaphore::new(max_concurrent)),
            total: AtomicU64::new(0),
            max_total,
        }
    }

    /// total++ 先于槽位等待；超上限即拒（文案逐字）。槽位等待挂 scope
    /// 取消（现役队列等待不响应 abort、靠 thunk 首行 signal 检查兜底；
    /// Rust 直接挂取消树，结局一致、响应更快——实施注记）。
    async fn acquire(
        &self,
        scope: &CancelScope,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, NodeFailure> {
        let total = self.total.fetch_add(1, Ordering::Relaxed) + 1;
        if total > self.max_total {
            return Err(NodeFailure::msg(format!(
                "workflow 超出 agent 总数上限 {}（防失控 backstop）",
                self.max_total
            )));
        }
        let sem = Arc::clone(&self.sem);
        tokio::select! {
            permit = sem.acquire_owned() => {
                Ok(permit.expect("workflow limiter semaphore is never closed"))
            }
            _ = scope.cancelled() => Err(NodeFailure::msg("workflow 已中止")),
        }
    }
}

// ── 节点失败 ─────────────────────────────────────────────────────────────

#[derive(Debug)]
struct NodeFailure {
    message: String,
    code: Option<&'static str>,
}

impl NodeFailure {
    fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
        }
    }
    fn coded(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
        }
    }
    fn envelope(&self) -> String {
        let mut map = serde_json::Map::new();
        map.insert("ok".to_string(), json!(false));
        map.insert("error".to_string(), json!(self.message));
        if let Some(code) = self.code {
            map.insert("code".to_string(), json!(code));
        }
        serde_json::to_string(&Value::Object(map)).unwrap_or_default()
    }
}

fn ok_envelope(result: Value) -> String {
    serde_json::to_string(&json!({ "ok": true, "result": result })).unwrap_or_default()
}

/// RunFinish → 节点结果文本（CompletedWithFinal 取终稿文本块 join " "；
/// 无终稿 = 空串，与现役 replyText ?? "" 同义）。
fn map_finish(finish: RunFinish) -> Result<String, NodeFailure> {
    match finish {
        RunFinish::CompletedWithFinal { message } => Ok(message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")),
        RunFinish::CompletedWithoutFinal { .. } => Ok(String::new()),
        RunFinish::Failed { cause } => {
            // 简短节点文案保持现役重试判定；完整失败事实另外留痕，避免只剩 provider_error。
            tracing::warn!(failure = ?cause, "workflow child run failed before a successful node result");
            Err(NodeFailure::msg(format!(
                "agent 失败: {}",
                cause.reason_segment()
            )))
        }
        RunFinish::Cancelled { detail } => Err(NodeFailure::msg(detail)),
        RunFinish::InterruptedNeedsAttention { detail } => {
            Err(NodeFailure::msg(format!("agent 失败: {detail}")))
        }
    }
}

// ── 服务与编排器 ─────────────────────────────────────────────────────────

const ACTIVITY_TTL_MS: u64 = 72 * 60 * 60 * 1000;

fn prune_activities(entries: &mut serde_json::Map<String, Value>, now: u64) {
    entries.retain(|_, entry| {
        entry
            .get("finishedAt")
            .and_then(Value::as_u64)
            .or_else(|| entry.get("startedAt").and_then(Value::as_u64))
            .is_none_or(|timestamp| timestamp >= now.saturating_sub(ACTIVITY_TTL_MS))
    });
}

fn save_activities(path: &Path, entries: &serde_json::Map<String, Value>) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(&json!({"schemaVersion":1,"entries":entries}))
        .map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    crate::paths::atomic_write(path, &bytes).map_err(|error| error.to_string())
}

fn load_activities(path: &Path, now: u64) -> Result<serde_json::Map<String, Value>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(serde_json::Map::new())
        }
        Err(error) => return Err(format!("workflow activity read failed: {error}")),
    };
    let raw: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("workflow activity is corrupt (preserved): {error}"))?;
    if raw["schemaVersion"] != 1 {
        return Err("workflow activity schema is unsupported (preserved)".into());
    }
    let mut entries = raw["entries"]
        .as_object()
        .cloned()
        .ok_or("workflow activity entries are malformed (preserved)")?;
    prune_activities(&mut entries, now);
    for entry in entries.values_mut() {
        if entry["status"] == "running" {
            entry["status"] = json!("failed");
            entry["reason"] = json!("process_interrupted");
            entry["finishedAt"] = json!(now);
        }
    }
    save_activities(path, &entries)?;
    Ok(entries)
}

/// 新节点和每次工具调用都复核权威会话；旧快照只可继续收窄，不能抵消撤权。
pub(crate) async fn validate_node_authority(
    storage: &RunDatabase,
    session: &str,
    scope: &WorkflowNodeScope,
    mode: SessionPermissionMode,
) -> Result<(), String> {
    let row = lingxi_adapters::storage::session_tree::get_session_row(storage, session)
        .await
        .map_err(|error| error.to_string())?
        .filter(|row| row.archived_at_unix_ms.is_none())
        .ok_or("workflow parent session is unavailable")?;
    let current_mode = row
        .permission_mode
        .as_deref()
        .and_then(SessionPermissionMode::parse)
        .unwrap_or(SessionPermissionMode::Ask);
    if (current_mode == SessionPermissionMode::ReadOnly && mode != SessionPermissionMode::ReadOnly)
        || (current_mode == SessionPermissionMode::Ask && mode == SessionPermissionMode::Operate)
    {
        return Err(
            "workflow parent permission was reduced; re-dispatch under current permissions".into(),
        );
    }
    let meta = crate::session_ui::read(storage, session).map_err(|error| error.to_string())?;
    let roots = parent_folder_roots(
        meta.as_ref().and_then(|meta| meta.cwd.as_deref()),
        &meta
            .as_ref()
            .map_or_else(Vec::new, |meta| meta.workspace_folders.clone()),
        &row.authorized_folders,
    );
    let folders: Vec<String> = scope
        .write_roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();
    if !folders.is_empty() {
        assert_write_scope_contained(&folders, &folders, &roots).map_err(|error| error.message)?;
    }
    Ok(())
}

/// workflow 引擎服务：工具执行体 + 后台 orchestrator 的全部宿主依赖。
pub struct WorkflowService {
    storage: Arc<RunDatabase>,
    events: Arc<EventService>,
    runs: Arc<RunSupervisor>,
    sessions: Arc<SessionSupervisor>,
    subagents: Arc<SubagentRuntime>,
    app_events: Arc<AppEventLog>,
    agents_root: PathBuf,
    clock: Arc<dyn ServiceClock>,
    id_seq: AtomicU64,
    active: Mutex<WorkflowRegistry>,
    activity_path: PathBuf,
    activities: Mutex<Result<serde_json::Map<String, Value>, String>>,
    loop_controller: Mutex<std::sync::Weak<crate::loopx::LoopController>>,
}

#[derive(Default)]
struct WorkflowRegistry {
    stopping: bool,
    tasks: HashMap<String, ActiveWorkflow>,
}

struct ActiveWorkflow {
    session_id: String,
    calling_run_id: String,
    scope: CancelScope,
}

/// 即使后台主体 panic，注册也会摘除，避免残留“仍在运行”的假状态。
struct WorkflowRegistration {
    service: Arc<WorkflowService>,
    task_id: String,
}

impl Drop for WorkflowRegistration {
    fn drop(&mut self) {
        self.service
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .tasks
            .remove(&self.task_id);
    }
}

impl WorkflowService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        storage: Arc<RunDatabase>,
        events: Arc<EventService>,
        runs: Arc<RunSupervisor>,
        sessions: Arc<SessionSupervisor>,
        subagents: Arc<SubagentRuntime>,
        app_events: Arc<AppEventLog>,
        agents_root: PathBuf,
        clock: Arc<dyn ServiceClock>,
    ) -> Self {
        let activity_path = agents_root
            .parent()
            .unwrap_or(&agents_root)
            .join("workflow-activity.json");
        let activities = load_activities(&activity_path, clock.now_unix_ms());
        Self {
            storage,
            events,
            runs,
            sessions,
            subagents,
            app_events,
            agents_root,
            clock,
            id_seq: AtomicU64::new(0),
            active: Mutex::new(WorkflowRegistry::default()),
            activity_path,
            activities: Mutex::new(activities),
            loop_controller: Mutex::new(std::sync::Weak::new()),
        }
    }

    pub fn attach_loop_controller(&self, controller: &Arc<crate::loopx::LoopController>) {
        *self
            .loop_controller
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Arc::downgrade(controller);
    }

    /// 关闭入口与取消登记中的所有工作在同一锁下完成，退出期间不能漏接新任务。
    pub fn begin_stop(&self) {
        let mut registry = self.active.lock().unwrap_or_else(|p| p.into_inner());
        registry.stopping = true;
        for task in registry.tasks.values() {
            task.scope.cancel("workflow service shutdown");
        }
    }

    pub fn cancel_for_session(&self, session_id: &str) -> Vec<String> {
        let registry = self.active.lock().unwrap_or_else(|p| p.into_inner());
        registry
            .tasks
            .iter()
            .filter(|(_, task)| task.session_id == session_id)
            .map(|(id, task)| {
                task.scope.cancel("workflow session cancelled");
                id.clone()
            })
            .collect()
    }

    /// 普通 Run 的取消只影响它直接派出的工作流，不扫同会话其他后台任务。
    pub fn cancel_for_run(&self, run_id: &str) -> Vec<String> {
        let registry = self.active.lock().unwrap_or_else(|p| p.into_inner());
        registry
            .tasks
            .iter()
            .filter(|(_, task)| task.calling_run_id == run_id)
            .map(|(id, task)| {
                task.scope.cancel("workflow parent run cancelled");
                id.clone()
            })
            .collect()
    }

    pub fn has_active_for_session(&self, session: &str) -> bool {
        self.active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .tasks
            .values()
            .any(|task| task.session_id == session)
    }

    pub fn active_count(&self) -> usize {
        self.active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .tasks
            .len()
    }

    async fn wait_for_tasks(&self, session_id: Option<&str>, budget: Duration) -> Vec<String> {
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            let pending: Vec<String> = self
                .active
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .tasks
                .iter()
                .filter(|(_, task)| session_id.is_none_or(|id| task.session_id == id))
                .map(|(id, _)| id.clone())
                .collect();
            if pending.is_empty() || tokio::time::Instant::now() >= deadline {
                return pending;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    /// 返回尚未确认退出的任务；清理完成以前不伪称取消成功。
    pub async fn cancel_session(
        &self,
        session_id: &str,
        budget: Duration,
    ) -> Result<Vec<String>, String> {
        self.cancel_for_session(session_id);
        Ok(self.wait_for_tasks(Some(session_id), budget).await)
    }

    pub async fn stop(&self, budget: Duration) -> Result<Vec<String>, String> {
        self.begin_stop();
        Ok(self.wait_for_tasks(None, budget).await)
    }

    /// 与现役活动卡相同的持久形状，先写穿再广播，重启快照从这里读取。
    pub fn activities_for_session(&self, session: &str) -> Result<Vec<Value>, String> {
        let guard = self.activities.lock().unwrap_or_else(|p| p.into_inner());
        let entries = guard.as_ref().map_err(Clone::clone)?;
        Ok(entries
            .values()
            .filter(|entry| entry["sessionId"] == session)
            .cloned()
            .collect())
    }

    fn activity(&self, session: &str, id: &str, kind: &str, patch: Value) -> Result<(), String> {
        let entry = {
            let mut guard = self.activities.lock().unwrap_or_else(|p| p.into_inner());
            let entries = guard.as_mut().map_err(|error| error.clone())?;
            let mut next = entries.clone();
            prune_activities(&mut next, self.clock.now_unix_ms());
            let entry = next.entry(id).or_insert_with(
                || json!({"id":id,"kind":kind,"startedAt":self.clock.now_unix_ms()}),
            );
            let object = entry
                .as_object_mut()
                .ok_or("workflow activity is malformed")?;
            object.insert("sessionId".into(), json!(session));
            object.insert(
                "sessionPath".into(),
                json!(crate::session_locator::native(session)),
            );
            if let Some(patch) = patch.as_object() {
                object.extend(patch.clone());
            }
            // 快照与实时事件经过同一公共接口校验；扩展字段由flatten原样保留。
            let typed: lingxi_protocol::AgentActivityEntry = serde_json::from_value(entry.clone())
                .map_err(|error| format!("workflow activity violates public contract: {error}"))?;
            *entry = serde_json::to_value(typed).map_err(|error| error.to_string())?;
            let entry = entry.clone();
            if let Err(error) = save_activities(&self.activity_path, &next) {
                // 旧running快照不能继续冒充当前事实；取消仍从独立在册范围执行。
                *guard = Err(format!("workflow activity receipt is unconfirmed: {error}"));
                return Err(error);
            }
            *entries = next;
            entry
        };
        self.app_events
            .publish("agent_activity", json!({"entry":entry}));
        Ok(())
    }

    pub fn remove_session_activities(&self, session: &str) -> Result<(), String> {
        let mut guard = self.activities.lock().unwrap_or_else(|p| p.into_inner());
        let entries = guard.as_mut().map_err(|error| error.clone())?;
        let mut next = entries.clone();
        next.retain(|_, entry| entry["sessionId"] != session);
        if let Err(error) = save_activities(&self.activity_path, &next) {
            *guard = Err(format!("workflow activity cleanup is unconfirmed: {error}"));
            return Err(error);
        }
        *entries = next;
        Ok(())
    }

    pub fn list_for_session(&self, session: &str) -> Result<Vec<Value>, String> {
        Ok(self.activities_for_session(session)?.into_iter()
            .filter(|entry| entry["kind"] == "workflow")
            .map(|entry| {
                let status = match entry["status"].as_str() { Some("running") => "pending", Some("done") => "resolved", _ => "failed" };
                json!({"taskId":entry["id"],"status":status,"type":"workflow","deferredAtUnixMs":entry["startedAt"],"parentSessionId":session,"reason":entry["reason"],"result":entry["result"],"meta":{"type":"workflow","runId":entry["id"]}})
            }).collect())
    }

    pub async fn cancel_task(
        &self,
        session: &str,
        task: &str,
        budget: Duration,
    ) -> Result<Option<Value>, String> {
        // 活动记录损坏也必须能取消真实工作，不能先查询磁盘回执再决定是否发取消。
        let requested = {
            let registry = self.active.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(active) = registry
                .tasks
                .get(task)
                .filter(|active| active.session_id == session)
            {
                active.scope.cancel("workflow task cancelled");
                true
            } else {
                false
            }
        };
        if !requested {
            let entry = self
                .activities_for_session(session)?
                .into_iter()
                .find(|entry| entry["id"] == task && entry["kind"] == "workflow");
            return Ok(entry.map(|entry| {
                if entry["status"] == "running" {
                    json!({"status":"rejected","reason":"activity_state_unconfirmed"})
                } else {
                    json!({"status":"already_stopped"})
                }
            }));
        }
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            let live = self
                .active
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .tasks
                .contains_key(task);
            if !live {
                let entries = self.activities_for_session(session).map_err(|error| {
                    format!("取消已请求且任务资源已回收，但活动回执未确认：{error}")
                })?;
                let entry = entries.into_iter().find(|entry| entry["id"] == task);
                return Ok(Some(
                    match entry.as_ref().and_then(|entry| entry["status"].as_str()) {
                        Some("aborted") => json!({"status":"aborted"}),
                        Some("done" | "failed") => json!({"status":"already_stopped"}),
                        _ => json!({"status":"rejected","reason":"activity_state_unconfirmed"}),
                    },
                ));
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(Some(
                    json!({"status":"rejected","reason":"cleanup_unconfirmed"}),
                ));
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    fn publish_progress(&self, session_id: &str, task_id: &str, extra: Value) {
        let mut payload = serde_json::Map::new();
        payload.insert("taskId".to_string(), json!(task_id));
        payload.insert("sessionId".to_string(), json!(session_id));
        if let Value::Object(map) = extra {
            for (k, v) in map {
                payload.insert(k, v);
            }
        }
        self.app_events
            .publish("workflow_progress", Value::Object(payload));
    }

    /// 预算 spent：usage 账本会话行 in-memory 滤 cause_ref==taskId，五个
    /// Option 分量求和（δ 设计登记；查询失败 fail-soft 用最近快照 + warn）。
    async fn query_spent(&self, session_id: &str, task_id: &str, snapshot: &AtomicU64) -> u64 {
        let query = ModelUsageQuery {
            owner_user_id: None,
            session_id: Some(session_id.to_string()),
            run_id: None,
            purpose: None,
            model: None,
            recorded_from_unix_ms: None,
            recorded_to_unix_ms: None,
            ..ModelUsageQuery::default()
        };
        match self.storage.query_model_call_usage(query).await {
            Ok(records) => {
                let spent = records
                    .iter()
                    .filter(|r| r.cause_ref.as_deref() == Some(task_id))
                    .map(|r| {
                        r.usage.as_ref().map_or(0, |u| {
                            [
                                u.input_tokens,
                                u.output_tokens,
                                u.cache_read_tokens,
                                u.cache_write_tokens,
                                u.reasoning_tokens,
                            ]
                            .into_iter()
                            .flatten()
                            .sum::<u64>()
                        })
                    })
                    .sum::<u64>();
                snapshot.store(spent, Ordering::Relaxed);
                spent
            }
            Err(err) => {
                tracing::warn!("workflow budget query failed (fail-soft to snapshot): {err}");
                snapshot.load(Ordering::Relaxed)
            }
        }
    }

    /// 工具执行体：静态校验 → 派单 → runId 即回（现役 dispatch 路径）。
    async fn dispatch(self: &Arc<Self>, ctx: &RunContext, arguments: &Value) -> ToolOutcome {
        let script = arguments
            .get("script")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let (meta, _stripped) = match workflowjs::extract_meta(&script) {
            Ok(pair) => pair,
            Err(message) => {
                return failed(format!("Workflow script invalid: {message}"));
            }
        };
        // declarativeNodesUnsupported（文案逐字）。
        if meta.raw.get("nodes").is_some_and(Value::is_array) {
            return failed(
                "workflow meta.nodes is declarative metadata and is not executable yet; use agent()/parallel()/phase()/log() in the script body.",
            );
        }

        let session_id = ctx.session_id.as_str().to_string();
        let row = match lingxi_adapters::storage::session_tree::get_session_row(
            self.storage.as_ref(),
            &session_id,
        )
        .await
        {
            Ok(Some(row)) => row,
            Ok(None) => {
                // 现役此处分支进 _syncRun；Rust 组合根下会话行对聊天轮恒存在，
                // 缺失即响亮失败（登记：_syncRun 结构不可达，见 δ8 节）。
                return failed("Workflow dispatch failed: parent session row is unavailable");
            }
            Err(err) => {
                return failed(format!("Workflow dispatch failed: {err}"));
            }
        };
        let agent_id = row.agent_id.clone();
        let parent_mode = self.sessions.permission_mode(&session_id);

        // 父会话写根：会话 cwd + workspace_folders + authorized_folders。
        // 元数据读取失败 fail-soft 到 authorized_folders（warn 留痕）。
        let ui_meta = match crate::session_ui::read(self.storage.as_ref(), &session_id) {
            Ok(meta) => meta,
            Err(err) => {
                tracing::warn!("workflow parent folder scope: session metadata read failed: {err}");
                None
            }
        };
        let session_cwd = ui_meta.as_ref().and_then(|m| m.cwd.clone());
        let workspace_folders = ui_meta
            .as_ref()
            .map_or_else(Vec::new, |m| m.workspace_folders.clone());
        let parent_roots = parent_folder_roots(
            session_cwd.as_deref(),
            &workspace_folders,
            &row.authorized_folders,
        );

        let now_ms = self.clock.now_unix_ms();
        let seq = self.id_seq.fetch_add(1, Ordering::Relaxed) + 1;
        let rand = match rand6() {
            Ok(rand) => rand,
            Err(message) => return failed(message),
        };
        let task_id = mint_task_id("workflow", now_ms, seq, &rand);

        // journal + resume（resumeFromRunId 只允许 [A-Za-z0-9_-]——任务 id
        // 词表；防路径逃逸，比现役 path.join 严格，登记硬化注记）。
        let journal_dir = self.agents_root.join(&agent_id).join("workflow-journals");
        let mut journal = WorkflowJournal::new(Some(journal_dir.join(format!("{task_id}.jsonl"))));
        let mut replay = None;
        if let Some(resume) = arguments.get("resumeFromRunId").and_then(Value::as_str) {
            if resume.is_empty()
                || !resume
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return failed("Workflow resume failed: invalid task identity");
            }
            // 文件名安全不等于拥有该任务；缓存包含原节点结果，必须先核验真实归属。
            let prior = match self.activities_for_session(&session_id) {
                Ok(entries) => entries.into_iter().find(|entry| {
                    entry["id"] == resume
                        && entry["kind"] == "workflow"
                        && entry["agentId"] == agent_id
                }),
                Err(error) => return failed(format!("Workflow resume failed: {error}")),
            };
            if prior.is_none() {
                return failed(
                    "Workflow resume failed: source task does not belong to this session and agent",
                );
            }
            if prior
                .as_ref()
                .is_some_and(|entry| entry["resumeSafe"] != true)
            {
                if prior
                    .as_ref()
                    .is_some_and(|entry| entry["resumeBlockedReason"] == "nested_workflow")
                {
                    return failed("Workflow resume failed: source task contains nested workflows; automatic resume is disabled to prevent repeating completed operations");
                }
                return failed("Workflow resume failed: source journal was not confirmed durable; verify completed operations before re-dispatch");
            }
            let old = match WorkflowJournal::load(journal_dir.join(format!("{resume}.jsonl"))) {
                Ok(journal) => journal,
                Err(error) => {
                    return failed(format!(
                        "Workflow resume failed: {error}; automatic re-execution is refused"
                    ))
                }
            };
            let actual_receipt = match old.confirmed_receipt() {
                Ok(receipt) => receipt,
                Err(error) => {
                    return failed(format!(
                        "Workflow resume failed: {error}; automatic re-execution is refused"
                    ))
                }
            };
            if Some(&actual_receipt)
                != prior
                    .as_ref()
                    .and_then(|entry| entry.get("journalReceipt"))
                    .filter(|receipt| receipt.is_object())
            {
                return failed("Workflow resume failed: source journal continuity does not match its durable receipt; automatic re-execution is refused");
            }
            if old.has_entries() {
                self.publish_progress(&session_id, &task_id, json!({"message":format!("Resuming from {} cached entries", old.total_entries())}));
            }
            replay = Some(old);
        }

        let limits = normalize_run_limits(arguments.get("limits"));
        // params.args?.budgetTokens ?? null；分数按 Math.round 同义（goal
        // 预算先例），负数 → 0（现役 remaining()<=0 立拒的等价效果）。
        let budget_total = arguments
            .get("args")
            .and_then(|a| a.get("budgetTokens"))
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .map(|v| v.round().max(0.0) as u64);

        if let Err(error) = journal.initialize() {
            return failed(error);
        }
        let parent_permit = match self.runs.task_supervisor().parent_run_permit(
            ctx.run_id.as_str(),
            &session_id,
            &agent_id,
            &ctx.principal,
            self.storage.session_admission(),
        ) {
            Ok(permit) => permit,
            Err(error) => {
                return failed(format!(
                    "Workflow dispatch failed: parent admission authority: {error}"
                ));
            }
        };
        let scope = CancelScope::background_root(&format!("workflow:{task_id}"));
        {
            let mut registry = self.active.lock().unwrap_or_else(|p| p.into_inner());
            if registry.stopping {
                return failed("Workflow dispatch failed: service is stopping");
            }
            if let Err(error) = self.activity(
                &session_id,
                &task_id,
                "workflow",
                json!({"status":"running","summary":meta.name,"agentId":agent_id,"resumeSafe":false}),
            ) {
                return failed(format!(
                    "Workflow dispatch failed: activity persistence: {error}"
                ));
            }
            registry.tasks.insert(
                task_id.clone(),
                ActiveWorkflow {
                    session_id: session_id.clone(),
                    calling_run_id: ctx.run_id.to_string(),
                    scope: scope.clone(),
                },
            );
        }
        let registration = WorkflowRegistration {
            service: Arc::clone(self),
            task_id: task_id.clone(),
        };
        let orchestrator = Orchestrator {
            service: Arc::clone(self),
            task_id: task_id.clone(),
            session_id,
            agent_id,
            principal: ctx.principal.clone(),
            calling_run_id: ctx.run_id.clone(),
            parent_mode,
            parent_roots,
            limits,
            budget_total,
            budget_spent: Arc::new(AtomicU64::new(0)),
            node_seq: Arc::new(AtomicU64::new(0)),
            session_permit: Mutex::new(
                parent_permit
                    .as_ref()
                    .map(Arc::downgrade)
                    .unwrap_or_default(),
            ),
            journal: Arc::new(Mutex::new(journal)),
            replay: Arc::new(Mutex::new(replay)),
            limiter: WorkflowLimiter::new(limits.max_concurrent, AGENT_TOTAL_BACKSTOP),
            scope,
            children: Mutex::new(Vec::new()),
            resume_safe: AtomicBool::new(true),
            resume_blocked_reason: Mutex::new(None),
            abort_reason: Arc::new(Mutex::new(None)),
            feed: Arc::new(Notify::new()),
        };
        let started_at = now_ms;
        let args = arguments.get("args").cloned().unwrap_or(Value::Null);
        tokio::spawn(async move {
            let _registration = registration;
            orchestrator.run(script, args).await;
        });

        let text = format!(
            "Workflow \"{}\" has been dispatched for background execution (task {task_id}). Results will automatically return to the conversation when complete.",
            meta.name
        );
        success(
            text,
            json!({
                "taskId": task_id,
                "runId": task_id,
                "workflow": meta.name,
                "streamStatus": "running",
                "startedAt": started_at,
            }),
        )
    }
}

/// failWith：先置理由（首写胜）再 abort——「报什么就是什么」。
fn fail_with(state: &Mutex<Option<String>>, scope: &CancelScope, reason: String) {
    {
        let mut guard = state.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_some() {
            return;
        }
        *guard = Some(reason.clone());
    }
    scope.cancel(&reason);
}

/// 一次 workflow 运行的编排状态（外层与嵌套层共享 limiter/scope/budget/
/// journal——现役嵌套共享语义平移）。
struct Orchestrator {
    service: Arc<WorkflowService>,
    task_id: String,
    session_id: String,
    /// 调用方 agent（节点默认身份；usage/上下文归属与 subagent 同规）。
    agent_id: String,
    principal: Principal,
    calling_run_id: RunId,
    parent_mode: SessionPermissionMode,
    parent_roots: Vec<PathBuf>,
    limits: RunLimits,
    budget_total: Option<u64>,
    budget_spent: Arc<AtomicU64>,
    node_seq: Arc<AtomicU64>,
    session_permit: Mutex<std::sync::Weak<lingxi_kernel::session_admission::SessionPermit>>,
    journal: Arc<Mutex<WorkflowJournal>>,
    replay: Arc<Mutex<Option<WorkflowJournal>>>,
    limiter: WorkflowLimiter,
    scope: CancelScope,
    children: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    resume_safe: AtomicBool,
    resume_blocked_reason: Mutex<Option<String>>,
    abort_reason: Arc<Mutex<Option<String>>>,
    feed: Arc<Notify>,
}

impl Orchestrator {
    /// 后台主体：watchdog + 总时长定时器 + 脚本驱动 + 终态回灌。
    async fn run(self, script: String, args: Value) {
        let this = Arc::new(self);
        // 无进展 watchdog：节点/step/log 事件喂狗；饿死判死整条 run。
        let watchdog = {
            let feed = Arc::clone(&this.feed);
            let scope = this.scope.clone();
            let abort = Arc::clone(&this.abort_reason);
            let idle_ms = this.limits.idle_timeout_ms;
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = feed.notified() => {}
                        _ = scope.cancelled() => break,
                        _ = tokio::time::sleep(Duration::from_millis(idle_ms)) => {
                            fail_with(
                                &abort,
                                &scope,
                                format!("workflow 空转超时：{idle_ms}ms 内没有任何节点进展"),
                            );
                            break;
                        }
                    }
                }
            })
        };
        let total_timer = {
            let scope = this.scope.clone();
            let abort = Arc::clone(&this.abort_reason);
            let total_ms = this.limits.total_timeout_ms;
            tokio::spawn(async move {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(total_ms)) => {
                        fail_with(&abort, &scope, format!("workflow 总时长超限（{total_ms}ms）"));
                    }
                    _ = scope.cancelled() => {}
                }
            })
        };

        let mut result = this.drive_script(script, args, true).await;
        let was_cancelled = this.scope.is_cancelled();
        watchdog.abort();
        total_timer.abort();
        // 脚本失败或提前返回时，一起停止已发出的并行节点，再确认其真实收尾。
        this.scope.cancel("workflow script finished");
        loop {
            let children =
                std::mem::take(&mut *this.children.lock().unwrap_or_else(|p| p.into_inner()));
            if children.is_empty() {
                break;
            }
            for child in children {
                if let Err(error) = child.await {
                    tracing::error!("workflow {} child cleanup failed: {error}", this.task_id);
                }
            }
        }

        let journal_receipt = this
            .journal
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .confirmed_receipt();
        if let Err(error) = &journal_receipt {
            this.journal_failure(error.clone());
            result = Err(error.clone());
        }
        let status = if result.is_ok() {
            "done"
        } else if was_cancelled
            && this
                .abort_reason
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_none()
        {
            "aborted"
        } else {
            "failed"
        };
        let resume_blocked_reason = this
            .resume_blocked_reason
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let final_patch = json!({"status":status,"finishedAt":this.service.clock.now_unix_ms(),"result":result.as_ref().ok(),"reason":result.as_ref().err(),"resumeSafe":this.resume_safe.load(Ordering::Acquire),"resumeBlockedReason":resume_blocked_reason,"journalReceipt":journal_receipt.ok()});
        let activity_warning = match this.service.activity(
            &this.session_id,
            &this.task_id,
            "workflow",
            final_patch,
        ) {
            Ok(()) => None,
            Err(error) => {
                tracing::error!(
                    "workflow {} final activity persistence failed: {error}",
                    this.task_id
                );
                Some(format!("\n\n[activity-receipt-unconfirmed] 活动记录未确认保存，界面状态暂不可核验；实际结果以本条消息为准。{error}"))
            }
        };
        if let Ok(entries) = this.service.activities_for_session(&this.session_id) {
            for entry in entries.into_iter().filter(|entry| {
                entry["parentTaskId"] == this.task_id && entry["status"] == "running"
            }) {
                if let Some(id) = entry["id"].as_str() {
                    let _ = this.service.activity(
                        &this.session_id,
                        id,
                        "workflow_step",
                        json!({"status":"aborted","finishedAt":this.service.clock.now_unix_ms()}),
                    );
                }
            }
        }

        // abortReason 优先：脚本 promise 的 reject 只是 abort 的回声。
        match result {
            Ok(value) => {
                let mut text = workflow_result_to_text(&value);
                if resume_blocked_reason.as_deref() == Some("nested_workflow") {
                    text.push_str("\n\n此任务包含嵌套工作流，不能自动断点恢复；如需重新执行，请先核对已产生的结果。");
                }
                if let Some(warning) = &activity_warning {
                    text.push_str(warning);
                }
                let delivery = format!(
                    "[workflow-result task={} status=done]\n\n{text}",
                    this.task_id
                );
                if let Err(err) = this
                    .service
                    .sessions
                    .deliver_retained(&this.session_id, &delivery)
                {
                    tracing::error!("workflow {} result delivery failed: {err}", this.task_id);
                }
            }
            Err(err) => {
                let cause = this
                    .abort_reason
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone()
                    .unwrap_or(err);
                let reason = if this.resume_safe.load(Ordering::Acquire)
                    && activity_warning.is_none()
                {
                    format!("{cause}。可用 resumeFromRunId: \"{}\" 重发修正后的 workflow，已完成节点会命中缓存瞬时返回。", this.task_id)
                } else if resume_blocked_reason.as_deref() == Some("nested_workflow") {
                    format!("{cause}。此任务包含嵌套工作流，不能自动断点恢复；请先核对已完成节点产生的结果。")
                } else if !this.resume_safe.load(Ordering::Acquire) {
                    format!("{cause}。断点未确认保存，不能自动恢复；请先核对已完成节点产生的结果。")
                } else {
                    format!("{cause}。活动回执未确认保存，请先修复记录存储并核对已完成节点的结果，再决定如何恢复。")
                };
                let reason = format!("{reason}{}", activity_warning.as_deref().unwrap_or(""));
                let delivery = format!(
                    "[workflow-result task={} status=failed]\n\n{reason}",
                    this.task_id
                );
                if let Err(err) = this
                    .service
                    .sessions
                    .deliver_retained(&this.session_id, &delivery)
                {
                    tracing::error!("workflow {} failure delivery failed: {err}", this.task_id);
                }
            }
        }
        // 先确认此工作流完全退出，再让循环查看“还有后台工作吗”的真实结果。
        this.service
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .tasks
            .remove(&this.task_id);
        if status != "aborted" && !this.service.sessions.is_busy(&this.session_id) {
            let controller = this
                .service
                .loop_controller
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .upgrade();
            if let Some(controller) = controller {
                if controller
                    .tool_status(&this.session_id)
                    .is_some_and(|record| record.status == crate::loopx::LoopStatus::Running)
                {
                    // 同一个完成事件只投递一次；会话竞态变忙由控制器返回Busy，不重投。
                    let _ = controller
                        .deliver_wakeup_turn(&this.session_id, "background workflow completed")
                        .await;
                }
            }
        }
    }

    /// 驱动一个脚本层（外层或嵌套）：专用线程跑沙箱，主循环受理宿主请求。
    /// 嵌套层共享 limiter/scope/budget/journal，nesting=false。
    async fn drive_script(
        self: &Arc<Self>,
        script: String,
        args: Value,
        nesting: bool,
    ) -> Result<Value, String> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HostRequest>();
        let globals = SandboxGlobals {
            node_seq: Arc::clone(&self.node_seq),
            args,
            budget_total: self.budget_total,
            parent_mode: Some(self.parent_mode.wire_name().to_string()),
            nesting,
        };
        let thread_scope = self.scope.clone();
        let spent = Arc::clone(&self.budget_spent);
        let deadline_ms = self.limits.total_timeout_ms + SCRIPT_DEADLINE_SLACK_MS;
        let script_join = tokio::task::spawn_blocking(move || {
            workflowjs::run_workflow_script(&script, globals, tx, spent, thread_scope, deadline_ms)
        });

        // 发送侧随沙箱线程结束而 drop（thread_local 清场 + 每次调用的克隆
        // 用完即弃）——rx 收 None 即脚本已收场。
        while let Some(request) = rx.recv().await {
            self.handle_host_request(request, nesting);
        }

        match script_join.await {
            Ok(Ok(run)) => Ok(run.result),
            Ok(Err(message)) => Err(message),
            Err(join_err) => Err(format!("workflow 脚本线程异常: {join_err}")),
        }
    }

    /// 受理一条宿主请求。同步函数边界是有意的：嵌套臂 spawn 的闭包回调
    /// `drive_script`，若直接写在 `drive_script` 体内会让其 future 的
    /// Send 判定与闭包互相递归（rustc 无法归纳）；隔一层同步函数后，
    /// 外层 future 的 Send 不再依赖闭包类型。
    fn handle_host_request(self: &Arc<Self>, request: HostRequest, nesting: bool) {
        match request {
            HostRequest::Agent(agent_req) => {
                let orch = Arc::clone(self);
                let handle = tokio::spawn(async move {
                    let workflowjs::AgentRequest {
                        seq,
                        prompt,
                        opts_json,
                        key_payload,
                        reply,
                    } = agent_req;
                    let activity_id = format!("{}:node-{seq}", orch.task_id);
                    let opts = serde_json::from_str::<Value>(&opts_json).unwrap_or(Value::Null);
                    if let Err(error) = orch.service.activity(&orch.session_id, &activity_id, "workflow_agent", json!({"status":"running","parentTaskId":orch.task_id,"summary":prompt,"label":opts["label"],"access":opts["access"]})) {
                        let _ = reply.send(NodeFailure::msg(format!("workflow activity persistence failed: {error}")).envelope());
                        return;
                    }
                    let envelope = orch
                        .handle_agent(seq, &prompt, &opts_json, &key_payload)
                        .await;
                    let parsed = serde_json::from_str::<Value>(&envelope).unwrap_or(Value::Null);
                    if let Err(error) = orch.service.activity(&orch.session_id, &activity_id, "workflow_agent", json!({"status":if parsed["ok"] == true {"done"} else if orch.scope.is_cancelled() {"aborted"} else {"failed"},"finishedAt":orch.service.clock.now_unix_ms(),"reason":parsed["error"]})) {
                        orch.scope.cancel("workflow activity persistence failed");
                        tracing::error!("workflow node activity persistence failed: {error}");
                    }
                    let _ = reply.send(envelope);
                });
                self.children
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(handle);
            }
            HostRequest::Nested {
                script,
                args_json,
                reply,
            } => {
                if !nesting {
                    // 结构不可达（嵌套层不注册 __native_workflow）；防御性
                    // 响亮拒绝而非静默（文案逐字）。
                    let _ =
                        reply.send(NodeFailure::msg("当前环境不支持 workflow 嵌套调用").envelope());
                    return;
                }
                // 共享运行期编号只能保证唯一；嵌套并行顺序尚无稳定回放契约，执行前明确停用恢复。
                self.resume_safe.store(false, Ordering::Release);
                *self
                    .resume_blocked_reason
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = Some("nested_workflow".into());
                self.replay.lock().unwrap_or_else(|p| p.into_inner()).take();
                if let Err(error) = self.service.activity(
                    &self.session_id,
                    &self.task_id,
                    "workflow",
                    json!({"resumeSafe":false,"resumeBlockedReason":"nested_workflow"}),
                ) {
                    self.scope
                        .cancel("nested workflow recovery guard could not be persisted");
                    let _ = reply.send(
                        NodeFailure::msg(format!(
                            "nested workflow recovery guard persistence failed: {error}"
                        ))
                        .envelope(),
                    );
                    return;
                }
                let orch = Arc::clone(self);
                let handle = tokio::spawn(async move {
                    let child_args =
                        serde_json::from_str::<Value>(&args_json).unwrap_or(Value::Null);
                    let envelope = match orch.drive_script(script, child_args, false).await {
                        Ok(result) => ok_envelope(result),
                        Err(error) => NodeFailure::msg(error).envelope(),
                    };
                    let _ = reply.send(envelope);
                });
                self.children
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(handle);
            }
            HostRequest::Log { message } => {
                // 现役 log() 走 onAgentEvent（喂狗）+ onProgress。
                self.feed.notify_one();
                self.service.publish_progress(
                    &self.session_id,
                    &self.task_id,
                    json!({ "message": message }),
                );
            }
            HostRequest::Phase { title } => {
                // 现役 phase 只发 onProgress，不喂狗。
                self.service.publish_progress(
                    &self.session_id,
                    &self.task_id,
                    json!({ "title": title }),
                );
            }
            HostRequest::Step { kind, phase, seq } => {
                self.feed.notify_one();
                let id = format!("{}:step-{seq}", self.task_id);
                let terminal = phase != "start";
                if let Err(error) = self.service.activity(&self.session_id, &id, "workflow_step", json!({"parentTaskId":self.task_id,"stepKind":kind,"status":if phase == "start" {"running"} else if phase == "done" {"done"} else {"failed"},"finishedAt":if terminal {Some(self.service.clock.now_unix_ms())} else {None}})) {
                    self.scope.cancel("workflow step activity persistence failed");
                    tracing::error!("workflow step activity persistence failed: {error}");
                }
            }
        }
    }

    fn journal_failure(&self, error: String) -> String {
        self.resume_safe.store(false, Ordering::Release);
        *self
            .resume_blocked_reason
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some("journal_unconfirmed".into());
        let reason = format!("workflow node result could not be saved durably: {error}; completed effects have not been rolled back");
        fail_with(&self.abort_reason, &self.scope, reason.clone());
        NodeFailure::msg(reason).envelope()
    }

    /// 一个 agent() 节点的完整生命周期：回放 → limiter → 中止/预算 →
    /// opts 解析校验 → 重试循环 → journal 入账 → 回信封。
    async fn handle_agent(
        &self,
        seq: u64,
        prompt: &str,
        opts_json: &str,
        key_payload: &str,
    ) -> String {
        let node_id = format!("node-{seq}");
        let key = compute_journal_key(key_payload);

        // journal 回放：cache hit 不消耗 limiter 槽，瞬时返回。
        {
            let mut replay = self.replay.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(journal) = replay.as_mut() {
                if let Some(cached) = journal.try_replay(seq, &key) {
                    drop(replay);
                    self.feed.notify_one(); // start
                    self.feed.notify_one(); // done
                    let now = self.service.clock.now_unix_ms();
                    let recorded = self
                        .journal
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .record(seq, &key, cached.clone(), "ok", None, now);
                    if let Err(error) = recorded {
                        return self.journal_failure(error);
                    }
                    return ok_envelope(cached);
                }
            }
        }

        let _permit = match self.limiter.acquire(&self.scope).await {
            Ok(permit) => permit,
            Err(failure) => return failure.envelope(),
        };
        if self.scope.is_cancelled() {
            return NodeFailure::msg("workflow 已中止").envelope();
        }

        // budget 硬上限：spent >= total 拒新节点（文案逐字）。
        if let Some(total) = self.budget_total {
            let spent = self
                .service
                .query_spent(&self.session_id, &self.task_id, &self.budget_spent)
                .await;
            if spent >= total {
                return NodeFailure::msg(format!("workflow token 预算耗尽（已用 {spent}）"))
                    .envelope();
            }
        }

        let opts = serde_json::from_str::<Value>(opts_json).unwrap_or_else(|_| json!({}));
        let label = opts
            .get("label")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        // access 分层（attenuation 拒绝文案+code 逐字，不可重试）。
        let access = AccessRequest::parse(opts.get("access").and_then(Value::as_str));
        let tier = match resolve_subagent_access(access, self.parent_mode) {
            Ok(tier) => tier,
            Err(denied) => return NodeFailure::coded(denied.code, denied.message).envelope(),
        };

        // 原生入口也可被脚本直接调用，不能仅依赖 JS 包装器的形状检查。
        let folders = opts.get("writeFolders").filter(|value| !value.is_null());
        if folders.is_some() && tier == ToolAccessTier::ReadOnly {
            return NodeFailure::coded("WRITE_FOLDERS_CONFLICT_WITH_READ", "workflow agent() writeFolders declares a write scope and cannot be combined with access:\"read\".").envelope();
        }
        if folders.is_none() && tier != ToolAccessTier::ReadOnly {
            return NodeFailure::coded(
                "WRITE_FOLDERS_REQUIRED",
                format!(
                    "workflow node \"{node_id}\" is write-capable but declares no writeFolders."
                ),
            )
            .envelope();
        }
        // writeFolders 收窄校验（shape 复验 + 存在/包含 fail-closed）。
        if let Some(folders) = folders {
            if let Err(err) = validate_write_folders(folders, &self.parent_roots) {
                return NodeFailure::coded(err.code, err.message).envelope();
            }
        }

        let write_roots = match folders
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|raw| std::fs::canonicalize(raw.trim()))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()
        {
            Ok(Some(roots)) => roots,
            Ok(None) => Vec::new(),
            Err(error) => {
                return NodeFailure::msg(format!("workflow write folder changed: {error}"))
                    .envelope()
            }
        };
        let tool_filter = match opts.get("toolFilter").filter(|value| !value.is_null()) {
            None => None,
            Some(Value::Array(names)) if names.iter().all(Value::is_string) => Some(
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
            ),
            Some(_) => {
                return NodeFailure::msg("workflow toolFilter must be an array of tool names")
                    .envelope()
            }
        };
        let node_scope = WorkflowNodeScope {
            write_roots,
            tool_filter,
        };

        // model 覆写：首个 / 拆分；任一半缺失 → modelNotFound 文案（现役
        // session-coordinator.ts:2263 以原始 ref 回填 id）。
        let model_override = match opts.get("model").and_then(Value::as_str) {
            Some(raw) => {
                let trimmed = raw.trim();
                let pair = trimmed.find('/').and_then(|idx| {
                    (idx > 0 && idx < trimmed.len() - 1)
                        .then(|| (trimmed[..idx].to_string(), trimmed[idx + 1..].to_string()))
                });
                match pair {
                    Some(pair) => Some(pair),
                    None => {
                        return NodeFailure::msg(format!("Model not found: {raw}")).envelope();
                    }
                }
            }
            None => None,
        };

        // agentType 解析（花名册 find id||name；未找到文案逐字）。
        let node_agent = match opts.get("agentType").and_then(Value::as_str) {
            Some(query) if !query.is_empty() => match self.resolve_agent_type(query) {
                Ok(id) => id,
                Err(failure) => return failure.envelope(),
            },
            _ => self.agent_id.clone(),
        };

        // schema → prompt 后缀（登记偏差：无 extraCustomTools 通道，结果按
        // JSON 解析尝试，失败回退原文）。
        let has_schema = opts.get("schema").is_some();
        let final_prompt = if has_schema {
            format!(
                "{prompt}\n\n完成后必须调用一次 structured_output 工具，返回严格符合所需 schema 的结果。"
            )
        } else {
            prompt.to_string()
        };

        // maxRetries = min(opts.retries ?? limits.nodeRetries, MAX_NODE_RETRIES)。
        let retries_raw = opts
            .get("retries")
            .and_then(Value::as_u64)
            .unwrap_or(self.limits.node_retries);
        let max_retries = retries_raw.min(MAX_NODE_RETRIES);

        self.feed.notify_one(); // start 事件
        let mut last_failure: Option<NodeFailure> = None;
        let mut attempt = 1u64;
        loop {
            if self.scope.is_cancelled() {
                let failure = last_failure.unwrap_or_else(|| NodeFailure::msg("workflow 已中止"));
                return failure.envelope();
            }
            match self
                .attempt_node(
                    seq,
                    &node_agent,
                    &final_prompt,
                    model_override.clone(),
                    tier,
                    node_scope.clone(),
                )
                .await
            {
                Ok(text) => {
                    let result = if has_schema {
                        serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text))
                    } else {
                        Value::String(text)
                    };
                    self.feed.notify_one(); // done 事件
                    let now = self.service.clock.now_unix_ms();
                    let recorded = self
                        .journal
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .record(
                            seq,
                            &key,
                            result.clone(),
                            "ok",
                            Some(json!({ "attempts": attempt })),
                            now,
                        );
                    if let Err(error) = recorded {
                        return self.journal_failure(error);
                    }
                    // 脚本内 budget.spent() 读快照：节点完成即刷新（执行面
                    // 强制仍在派发时实时查询——登记注记）。
                    if self.budget_total.is_some() {
                        self.service
                            .query_spent(&self.session_id, &self.task_id, &self.budget_spent)
                            .await;
                    }
                    return ok_envelope(result);
                }
                Err(failure) => {
                    let retryable = !self.scope.is_cancelled()
                        && attempt <= max_retries
                        && is_retryable_node_error(&failure.message, failure.code);
                    if !retryable {
                        self.feed.notify_one(); // fail 事件
                        let now = self.service.clock.now_unix_ms();
                        let recorded = self
                            .journal
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .record(
                                seq,
                                &key,
                                Value::Null,
                                "error",
                                Some(json!({ "attempts": attempt, "error": failure.message })),
                                now,
                            );
                        if let Err(error) = recorded {
                            return self.journal_failure(error);
                        }
                        return failure.envelope();
                    }
                    let delay = retry_delay_ms(attempt);
                    self.service.publish_progress(
                        &self.session_id,
                        &self.task_id,
                        json!({ "message": format!(
                            "节点 {} 第 {attempt} 次失败，{}s 后重试: {}",
                            label.as_deref().unwrap_or(&node_id),
                            (delay as f64 / 1000.0).round() as u64,
                            failure.message.chars().take(120).collect::<String>()
                        ) }),
                    );
                    last_failure = Some(failure);
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
                        _ = self.scope.cancelled() => {}
                    }
                    attempt += 1;
                }
            }
        }
        // 现役循环外 `throw lastErr` 分支结构不可达（return 覆盖全部路径）。
    }

    /// 单次尝试：子 run 派发仪式（spawn_child 同款许可/作用域/
    /// spawn_linked，减 thread store），超时链父 scope。
    async fn attempt_node(
        &self,
        seq: u64,
        node_agent: &str,
        prompt: &str,
        model_override: Option<(String, String)>,
        tier: ToolAccessTier,
        node_scope: WorkflowNodeScope,
    ) -> Result<String, NodeFailure> {
        // 每个新节点都复核会话生命周期和最新写根，排队期间撤权不能沿用旧快照。
        let row = lingxi_adapters::storage::session_tree::get_session_row(
            self.service.storage.as_ref(),
            &self.session_id,
        )
        .await
        .map_err(|error| NodeFailure::msg(error.to_string()))?
        .filter(|row| row.archived_at_unix_ms.is_none())
        .ok_or_else(|| NodeFailure::msg("workflow parent session is unavailable"))?;
        let ui_meta = crate::session_ui::read(self.service.storage.as_ref(), &self.session_id)
            .map_err(|error| NodeFailure::msg(error.to_string()))?;
        let current_roots = parent_folder_roots(
            ui_meta.as_ref().and_then(|meta| meta.cwd.as_deref()),
            &ui_meta
                .as_ref()
                .map_or_else(Vec::new, |meta| meta.workspace_folders.clone()),
            &row.authorized_folders,
        );
        let folders: Vec<String> = node_scope
            .write_roots
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect();
        if !folders.is_empty() {
            assert_write_scope_contained(&folders, &folders, &current_roots)
                .map_err(|error| NodeFailure::coded(error.code, error.message))?;
        }
        let current_mode = self.service.sessions.permission_mode(&self.session_id);
        if current_mode == SessionPermissionMode::ReadOnly && tier != ToolAccessTier::ReadOnly {
            return Err(NodeFailure::msg("workflow parent session is now read-only"));
        }
        let now_ms = self.service.clock.now_unix_ms();
        let child_run_id = self
            .service
            .storage
            .allocate_run_id(now_ms)
            .map_err(|err| NodeFailure::msg(format!("{err}")))?;

        // 仅共享仍被实际执行持有的许可；弱引用不占住脚本空闲间隙，兄弟节点也不互抢。
        let permit = {
            let mut shared = self
                .session_permit
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(permit) = shared.upgrade() {
                permit
            } else {
                let gate = self.service.storage.session_admission();
                let permit = Arc::new(
                    gate.try_acquire(&[self.session_id.as_str()], usize::MAX)
                        .map_err(|_| NodeFailure::msg("session_busy"))?,
                );
                *shared = Arc::downgrade(&permit);
                permit
            }
        };

        let chat_input = model_override.map(|(provider, id)| ChatRunInput {
            images: Vec::new(),
            files: Vec::new(),
            client_message_id: None,
            snapshot_version: None,
            display_text: None,
            quoted_text: None,
            session_refs: Vec::new(),
            options: lingxi_kernel::model_exchange::ModelTurnOptions {
                model: Some((provider, id)),
                thinking_level: None,
            },
        });
        let authorization = DriveAuthorization {
            foreground_mcp_session_permissions: false,
            settings_principal: None,
            loop_identity: None,
            workflow_scope: Some(node_scope),
            chat_input,
            knowledge_selection: None,
            lineage: RunLineage {
                parent_run_id: Some(self.calling_run_id.clone()),
                origin: RunOrigin::Subagent,
                source_message_id: None,
                cause_id: Some(self.task_id.clone()),
            },
            grant: RunGrant::Subagent { tier },
            session_mode: self.parent_mode,
        };
        let registration = self
            .service
            .runs
            .task_supervisor()
            .retain_authorized_run_permit(
                &child_run_id,
                &self.session_id,
                node_agent,
                &self.principal,
                &authorization,
                Arc::clone(&permit),
            )
            .map_err(|err| NodeFailure::msg(format!("{err}")))?;

        let node_scope = self
            .scope
            .child(format!("child_run:{child_run_id}"), ScopeKind::ChildRun);
        let drive_scope = node_scope.clone();
        let runs = Arc::clone(&self.service.runs);
        let storage = Arc::clone(&self.service.storage);
        let events = Arc::clone(&self.service.events);
        let principal = self.principal.clone();
        let session_id = self.session_id.clone();
        let agent = node_agent.to_string();
        let run_id = child_run_id.clone();
        let input = prompt.to_string();
        let lane = format!("{}::workflow::{}::{seq}", self.session_id, self.task_id);
        let node_timeout = Duration::from_millis(self.limits.node_timeout_ms);
        let node_ms = self.limits.node_timeout_ms;
        let drive = async move {
            let _registration = registration;
            let _permit = permit;
            let fut = runs.drive_run(
                storage.as_ref(),
                events.as_ref(),
                &principal,
                &session_id,
                &agent,
                &run_id,
                &input,
                1,
                now_ms,
                None,
                Some(&drive_scope),
                authorization,
                None,
                &lane,
            );
            let mut fut = Box::pin(fut);
            // 节点超时锚定真实启动（spawn 即开始轮询）；超时只杀本节点——
            // 措辞避开「中止/aborted」（现役 run-limits 注释钉住；文本逐字：
            // 全角左括号、无右括号）。
            match tokio::time::timeout(node_timeout, fut.as_mut()).await {
                Ok(result) => Ok(result),
                Err(_elapsed) => {
                    drive_scope.cancel("workflow node timeout");
                    let _ = fut.as_mut().await;
                    Err(format!("节点超时（{node_ms}ms）：节点未在时限内完成"))
                }
            }
        };
        let handle = self
            .service
            .runs
            .task_supervisor()
            .spawn_linked(
                // 与现役子任务一致：外层驱动归属派单Run，不能登记到自己，
                // 否则子Run取消时drain_run会等待正在收尾的自身。
                self.calling_run_id.as_str(),
                &node_scope,
                format!("child_run:{child_run_id}"),
                drive,
            )
            .map_err(|err| NodeFailure::msg(format!("workflow node spawn rejected: {err:?}")))?;
        let outcome = handle
            .wait()
            .await
            .map_err(|exit| NodeFailure::msg(format!("workflow node task exit: {exit:?}")))?;
        match outcome {
            Ok(Ok(finish)) => map_finish(finish),
            Ok(Err(err)) => Err(NodeFailure::msg(err.to_string())),
            Err(message) => Err(NodeFailure::msg(message)),
        }
    }

    /// 现役 resolveAgentId：全花名册 find id||name（含调用方自身）。
    fn resolve_agent_type(&self, query: &str) -> Result<String, NodeFailure> {
        let roster = self.service.subagents.workflow_roster().map_err(|err| {
            NodeFailure::msg(format!(
                "workflow agentType \"{query}\" cannot be resolved in this environment. ({err})"
            ))
        })?;
        for entry in roster {
            let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
            let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
            if id == query || name == query {
                return Ok(id.to_string());
            }
        }
        Err(NodeFailure::msg(format!(
            "workflow agentType \"{query}\" was not found."
        )))
    }
}

/// 现役 workflowResultToText：string 直通，其余 JSON.stringify(x, null, 2)
/// 同形（serde_json 两空格缩进；to_json 已保证可序列化，undefined 分支
/// 结构不可达）。
fn workflow_result_to_text(result: &Value) -> String {
    match result {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| "null".to_string()),
    }
}

// ── 工具登记 ─────────────────────────────────────────────────────────────

fn failed(message: impl Into<String>) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(ErrorCode::InvalidMessage, message.into(), false),
    }
}

fn success(text: impl Into<String>, details: Value) -> ToolOutcome {
    let mut result = ToolSuccess::text(text.into());
    result.details = details.as_object().cloned();
    ToolOutcome::Success { result }
}

struct WorkflowToolExecutor {
    // 服务拥有运行宿主和网关；执行器只能弱关联，避免服务退出后整条执行链被循环引用保留。
    service: std::sync::Weak<WorkflowService>,
}

impl ToolExecutorPort for WorkflowToolExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let Some(service) = self.service.upgrade() else {
                return ToolExecutionResult::of_ctx(
                    ctx,
                    ToolOutcome::Failed {
                        error: ProtocolError::new(
                            ErrorCode::UpstreamUnavailable,
                            "workflow service has stopped; dispatch was not accepted",
                            false,
                        ),
                    },
                );
            };
            ToolExecutionResult::of_ctx(
                ctx,
                service.dispatch(ctx, request.arguments.as_value()).await,
            )
        })
    }
}

/// 与其他正式工具同口径登记：网关 prepare → 权限/批准 → 执行。
/// 权限：现役未声明 sessionPermission（routine 默认）→ Execute +
/// "workflow.run"（δ8 设计登记）。
pub fn register_workflow_tool(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    service: Arc<WorkflowService>,
    budget: &SchemaBudget,
) {
    let manifest = ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: "workflow".into(),
        display_name: "Workflow".into(),
        aliases: Vec::new(),
        version: "1.0.0".into(),
        description: WORKFLOW_DESCRIPTION.into(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".into(),
            schema: json!({
                "type": "object",
                "properties": {
                    "script": {"type": "string", "description": "Orchestration script, must start with export const meta = {...}"},
                    "args": {"description": "Arguments passed to the script's args global. Pass { budgetTokens: N } to set a token budget ceiling."},
                    "resumeFromRunId": {"type": "string", "description": "Previous workflow runId (taskId) to resume from — cached agent nodes with unchanged prompt+opts return instantly, first change onward re-executes."},
                    "limits": {
                        "type": "object",
                        "properties": {
                            "nodeTimeoutMs": {"type": "number"},
                            "idleTimeoutMs": {"type": "number"},
                            "totalTimeoutMs": {"type": "number"},
                            "maxConcurrent": {"type": "number"},
                            "nodeRetries": {"type": "number"},
                        },
                        "description": "Resource limits (clamped to safe ranges). Defaults: node 15min, idle 10min, total 4h, concurrency 16, retries 2.",
                    },
                },
                "required": ["script"],
            }),
        },
        output_schema: None,
        permission: PermissionContract {
            kind: PermissionKind::Execute,
            capability_base: "workflow.run".into(),
        },
        availability: Availability::Available,
        timeout_ms: Some(30_000),
        max_concurrency: None,
        declared_permission: DeclaredPermission::Execute,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    };
    let registered = registry
        .register(manifest, budget)
        .expect("workflow 工具必须登记成功");
    gateway.bind_executor(
        registered.target_id,
        Arc::new(WorkflowToolExecutor {
            service: Arc::downgrade(&service),
        }),
        "JS 编排脚本的受控 fan-out（后台执行）",
    );
}

// ── 测试 ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lingxi-r07-workflow-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[tokio::test]
    async fn workflow_executor_refuses_dispatch_after_its_service_has_exited() {
        let executor = WorkflowToolExecutor {
            service: std::sync::Weak::new(),
        };
        let ctx = RunContext {
            principal: Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("closed-workflow-session"),
            run_id: RunId::new("closed-workflow-run"),
            attempt: lingxi_protocol::AttemptId::new("closed-workflow-run#a1"),
            generation: 1,
        };
        let request = ToolRequest::from_effective_arguments("workflow", json!({"script":"export const meta={name:'closed',description:'d'}; return 'must not dispatch';"}), &SchemaBudget::default()).unwrap();
        let call = ToolCallId::new("closed-workflow-call");
        let result = executor.execute(&ctx, &call, &request).await;
        assert_eq!(
            result.fence,
            lingxi_kernel::ports::ResultFence::of_ctx(&ctx)
        );
        let ToolOutcome::Failed { error } = result.outcome else {
            panic!("已退出的服务不得返回受理成功");
        };
        assert_eq!(error.code, ErrorCode::UpstreamUnavailable);
        assert!(!error.retryable);
        assert!(error.message.contains("dispatch was not accepted"));
    }

    #[test]
    fn activity_restart_prunes_expired_and_preserves_unknown_fields_and_corruption() {
        let root = std::env::temp_dir().join(format!("workflow-activity-{}", rand6().unwrap()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("workflow-activity.json");
        let now = ACTIVITY_TTL_MS * 2;
        let entries = json!({"schemaVersion":1,"entries":{
            "running":{"id":"running","sessionId":"s","status":"running","startedAt":now,"futureField":{"keep":true}},
            "old":{"id":"old","status":"done","startedAt":1,"finishedAt":2}
        }});
        std::fs::write(&path, entries.to_string()).unwrap();
        let loaded = load_activities(&path, now).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["running"]["status"], "failed");
        assert_eq!(loaded["running"]["reason"], "process_interrupted");
        assert_eq!(loaded["running"]["futureField"]["keep"], true);
        assert_eq!(load_activities(&path, now).unwrap(), loaded);
        std::fs::write(&path, "{broken").unwrap();
        assert!(load_activities(&path, now).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mint_task_id_shape_and_kind_normalization() {
        let id = mint_task_id("workflow", 1_800_000_000_000, 7, "abc123");
        assert!(id.starts_with("task_workflow_"));
        let parts: Vec<&str> = id.split('_').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[2], base36(1_800_000_000_000));
        assert_eq!(parts[3], "7");
        assert_eq!(parts[4], "abc123");
        assert!(mint_task_id("Work Flow!!", 1, 1, "x").starts_with("task_workflow_"));
        assert!(mint_task_id("***", 1, 1, "x").starts_with("task_gen_"));
    }

    #[test]
    fn base36_matches_js_tostring36() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(base36(1_800_000_000_000), "mywpiww0"); // Date.now().toString(36) 同形
    }

    #[test]
    fn run_limits_defaults_and_clamps() {
        let defaults = normalize_run_limits(None);
        assert_eq!(defaults, DEFAULT_RUN_LIMITS);
        // 非对象（数组）→ 默认
        assert_eq!(normalize_run_limits(Some(&json!([]))), DEFAULT_RUN_LIMITS);
        let clamped = normalize_run_limits(Some(&json!({
            "nodeTimeoutMs": 1,          // 低于下限 → 60_000
            "idleTimeoutMs": 99_999_999, // 高于上限 → 3_600_000
            "totalTimeoutMs": 60_000.9,  // floor → 60_000 → 低于下限 → 300_000
            "maxConcurrent": 0,          // → 1
            "nodeRetries": 99,           // → 5
        })));
        assert_eq!(clamped.node_timeout_ms, 60_000);
        assert_eq!(clamped.idle_timeout_ms, 3_600_000);
        assert_eq!(clamped.total_timeout_ms, 300_000);
        assert_eq!(clamped.max_concurrent, 1);
        assert_eq!(clamped.node_retries, 5);
        // 非数字 → 该键默认
        let partial = normalize_run_limits(Some(&json!({ "nodeTimeoutMs": "fast" })));
        assert_eq!(partial.node_timeout_ms, DEFAULT_RUN_LIMITS.node_timeout_ms);
    }

    #[test]
    fn retry_classification_table() {
        // 8 个 code 全不可重试
        for code in NON_RETRYABLE_CODES {
            assert!(!is_retryable_node_error("anything", Some(code)), "{code}");
        }
        // 8 组文案正则
        assert!(!is_retryable_node_error("workflow 已中止", None));
        assert!(!is_retryable_node_error("task was aborted", None));
        assert!(!is_retryable_node_error(
            "workflow token 预算耗尽（已用 9）",
            None
        ));
        assert!(!is_retryable_node_error(
            "workflow 超出 agent 总数上限 1000（防失控 backstop）",
            None
        ));
        assert!(!is_retryable_node_error(
            "workflow agent() unsupported option \"x\"",
            None
        ));
        assert!(!is_retryable_node_error(
            "workflow agent() access must be \"read\" or \"write\".",
            None
        ));
        assert!(!is_retryable_node_error(
            "workflow agentType \"foo\" was not found.",
            None
        ));
        assert!(!is_retryable_node_error("writeFolders must be ...", None));
        assert!(!is_retryable_node_error(
            "agent() requires a non-empty prompt",
            None
        ));
        // 默认可重试（瞬时故障）
        assert!(is_retryable_node_error("provider 502", None));
        assert!(is_retryable_node_error("session_busy", None));
        // 节点超时措辞必须可重试（钉住：避开「中止/aborted」）
        assert!(is_retryable_node_error(
            "节点超时（900000ms）：节点未在时限内完成",
            None
        ));
    }

    #[test]
    fn retry_delay_bounds() {
        for _ in 0..64 {
            let first = retry_delay_ms(1);
            assert!((4_000..=6_000).contains(&first), "{first}");
            let later = retry_delay_ms(3);
            assert!((16_000..=24_000).contains(&later), "{later}");
        }
    }

    #[test]
    fn journal_roundtrip_replay_and_invalidation() {
        let dir = temp_dir("journal");
        let path = dir.join("run1.jsonl");
        let mut journal = WorkflowJournal::new(Some(path.clone()));
        journal.initialize().unwrap();
        journal
            .record(
                1,
                "k1",
                json!("r1"),
                "ok",
                Some(json!({"attempts": 1})),
                100,
            )
            .unwrap();
        journal
            .record(2, "k2", json!({"a": 1}), "ok", None, 200)
            .unwrap();
        journal
            .record(
                3,
                "k3",
                Value::Null,
                "error",
                Some(json!({"attempts": 3, "error": "x"})),
                300,
            )
            .unwrap();
        assert!(journal.has_entries());
        assert_eq!(journal.total_entries(), 3);

        let mut loaded = WorkflowJournal::load(path.clone()).unwrap();
        assert_eq!(loaded.total_entries(), 3);
        // 命中
        assert_eq!(loaded.try_replay(1, "k1"), Some(json!("r1")));
        assert_eq!(loaded.replay_hits(), 1);
        // key 漂移 → invalidated_after = 1，后续位置全部拒绝
        assert_eq!(loaded.try_replay(2, "kX"), None);
        assert_eq!(loaded.try_replay(3, "k3"), None);
        // status=error 同样视为漂移点
        let mut loaded = WorkflowJournal::load(path).unwrap();
        assert_eq!(loaded.try_replay(3, "k3"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_load_refuses_malformed_lines_without_erasing_evidence() {
        let dir = temp_dir("journal-corrupt");
        let path = dir.join("run2.jsonl");
        std::fs::write(
            &path,
            "{\"nodeSeq\":1,\"key\":\"k1\",\"result\":\"r1\",\"status\":\"ok\",\"ts\":1}\n\
             not-json\n\
             {\"noSeq\":true}\n\
             {\"nodeSeq\":2,\"key\":\"k2\",\"result\":null,\"status\":\"error\",\"ts\":2}\n",
        )
        .expect("write");
        let raw = std::fs::read(&path).unwrap();
        assert!(WorkflowJournal::load(path.clone()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), raw);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_write_failure_does_not_create_a_replayable_result() {
        let dir = temp_dir("journal-write-failure");
        let mut journal = WorkflowJournal::new(Some(dir.clone()));
        assert!(journal
            .record(1, "key", json!("already-produced"), "ok", None, 1)
            .is_err());
        assert!(!journal.has_entries());
        assert!(WorkflowJournal::load(dir.join("missing")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn journal_refuses_deleted_truncated_and_replaced_files_without_recreating_them() {
        for fault in ["delete", "truncate", "replace"] {
            let dir = temp_dir(&format!("journal-continuity-{fault}"));
            let path = dir.join("run.jsonl");
            let mut journal = WorkflowJournal::new(Some(path.clone()));
            journal.initialize().unwrap();
            journal
                .record(1, "first", json!("landed-first"), "ok", None, 1)
                .unwrap();
            let original = std::fs::read(&path).unwrap();
            match fault {
                "delete" => std::fs::remove_file(&path).unwrap(),
                "truncate" => std::fs::write(&path, b"").unwrap(),
                "replace" => {
                    std::fs::rename(&path, dir.join("original.jsonl")).unwrap();
                    std::fs::write(&path, &original).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                journal
                    .record(2, "second", json!("landed-second"), "ok", None, 2)
                    .is_err(),
                "{fault}"
            );
            assert_eq!(journal.total_entries(), 1, "{fault}");
            assert!(journal.confirmed_receipt().is_err(), "{fault}");
            match fault {
                "delete" => assert!(!path.exists()),
                "truncate" => assert_eq!(std::fs::read(&path).unwrap(), b""),
                "replace" => assert_eq!(std::fs::read(&path).unwrap(), original),
                _ => unreachable!(),
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn journal_refuses_oversized_sparse_input_before_reading_or_mutating() {
        let dir = temp_dir("journal-size-bound");
        let path = dir.join("oversized.jsonl");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.set_len(1024 * 1024 * 1024 * 1024).unwrap();
        let error = WorkflowJournal::load(path.clone())
            .err()
            .expect("超大稀疏文件必须拒绝");
        assert!(error.contains("8 MiB safety limit"), "{error}");
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            1024 * 1024 * 1024 * 1024
        );
        let mut journal = WorkflowJournal::new(Some(dir.join("append.jsonl")));
        journal.initialize().unwrap();
        let error = journal
            .record(
                1,
                "large",
                json!("x".repeat(MAX_JOURNAL_BYTES as usize)),
                "ok",
                None,
                1,
            )
            .unwrap_err();
        assert!(error.contains("8 MiB safety limit"), "{error}");
        assert_eq!(
            std::fs::metadata(dir.join("append.jsonl")).unwrap().len(),
            0
        );
        assert!(!journal.has_entries());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn journal_key_is_16_hex() {
        let key = compute_journal_key("{\"p\":\"task\",\"o\":{}}");
        assert_eq!(key.len(), 16);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn folder_scope_matrix() {
        let dir = temp_dir("scope");
        let inside = dir.join("inner");
        std::fs::create_dir_all(&inside).expect("mkdir");
        let file = dir.join("a-file");
        std::fs::write(&file, b"x").expect("file");
        let roots = vec![std::fs::canonicalize(&dir).expect("real")];
        let canon = |p: &Path| p.to_string_lossy().to_string();

        // 空 roots → PARENT_SCOPE_UNAVAILABLE
        let err = validate_write_folders(&json!([canon(&inside)]), &[]).unwrap_err();
        assert_eq!(err.code, "PARENT_SCOPE_UNAVAILABLE");
        // 不存在 → NOT_FOUND
        let missing = dir.join("missing");
        let err = validate_write_folders(&json!([canon(&missing)]), &roots).unwrap_err();
        assert_eq!(err.code, "WRITE_FOLDER_NOT_FOUND");
        assert!(err.message.contains("does not exist"));
        // 文件 → NOT_DIRECTORY
        let err = validate_write_folders(&json!([canon(&file)]), &roots).unwrap_err();
        assert_eq!(err.code, "WRITE_FOLDER_NOT_DIRECTORY");
        // 越界 → OUTSIDE
        let err = validate_write_folders(&json!(["/"]), &roots).unwrap_err();
        assert_eq!(err.code, "WRITE_FOLDER_OUTSIDE_PARENT_SCOPE");
        // 相对路径 → INVALID（原生复验）
        let err = validate_write_folders(&json!(["relative/path"]), &roots).unwrap_err();
        assert_eq!(err.code, "WRITE_FOLDERS_INVALID");
        // 空数组 → INVALID
        let err = validate_write_folders(&json!([]), &roots).unwrap_err();
        assert_eq!(err.code, "WRITE_FOLDERS_INVALID");
        // 合法内部目录 → 通过
        validate_write_folders(&json!([canon(&inside)]), &roots).expect("inside ok");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn result_to_text_passthrough_and_pretty() {
        assert_eq!(workflow_result_to_text(&json!("plain")), "plain");
        assert_eq!(
            workflow_result_to_text(&json!({"a": 1})),
            "{\n  \"a\": 1\n}"
        );
        assert_eq!(workflow_result_to_text(&Value::Null), "null");
    }

    #[test]
    fn meta_extraction_including_malicious() {
        // 正常 + export default 剥离
        let (meta, stripped) = workflowjs::extract_meta(
            "export const meta = { name: \"w\", description: \"d\" };\nexport default async function () { return 1; }",
        )
        .expect("meta");
        assert_eq!(meta.name, "w");
        assert!(!stripped.contains("export default"));
        // 空脚本 / 无 marker / 未闭合 / 缺字段——文案逐字
        assert_eq!(
            workflowjs::extract_meta("").unwrap_err(),
            "workflow script 不能为空"
        );
        assert_eq!(
            workflowjs::extract_meta("const meta = {}").unwrap_err(),
            "workflow script 必须以 export const meta = {...} 开头"
        );
        assert_eq!(
            workflowjs::extract_meta("export const meta = { name: \"w\"").unwrap_err(),
            "workflow meta 对象字面量未闭合"
        );
        assert_eq!(
            workflowjs::extract_meta("export const meta = { name: \"w\" }").unwrap_err(),
            "workflow meta 必须含 name 和 description 字符串"
        );
        // 字符串字面量内的花括号不干扰配对
        let (meta, _) =
            workflowjs::extract_meta("export const meta = { name: \"}\", description: \"d\" };")
                .expect("brace in string");
        assert_eq!(meta.name, "}");
    }

    #[test]
    fn node_timeout_text_is_byte_exact_and_retryable() {
        // 全角左括号、无右括号（现役 quirk，不得「修正」）。
        let text = format!("节点超时（{}ms）：节点未在时限内完成", 900_000);
        assert_eq!(text, "节点超时（900000ms）：节点未在时限内完成");
        assert!(!text.contains(')'));
        assert!(is_retryable_node_error(&text, None));
    }
}
