//! loopx.rs — 循环的状态机、记账、护栏与单槽闹钟（R07-T07 δ4/δ5；
//! 现役 lib/loop/{loop-store,loop-messages,loop-controller,alarm-service}.ts 平移）。
//!
//! 只在 turn 与 turn 的缝隙里工作：经 run 层的钩子观测轮生命周期（每轮迭代
//! 顶 = turn_start，provider 失败 = message_end error，run finalize = turn_end），
//! 绝不介入轮内部。护栏（轮数预算、连续失败）与守恒检查（运行中的循环必须
//! 「有闹钟 ∨ 有活跃后台工作」）是本层的硬保证，不依赖模型自觉。
//!
//! 循环以 sessionId 为键（与现役桌面桥接统一）。Rust run 层天然持 sessionId，
//! 现役的 sessionPath→sessionId 解析（_resolveKeyForSessionPath）在 Rust 侧
//! 退化为恒等（T07_STATUS δ5 登记）。
//!
//! 投递语义映射（登记项）：现役 display:false custom_message + triggerTurn；
//! Rust = 模型可见归档落库 + 空闲时 background drive；会话忙 → busy（闹钟
//! 60s 顺延）；followUp 模式不存在（桌面连续轮概念），映射为 busy。
//! MAX_TIMER_DELAY_MS=2^31-1 链式续接是 JS setTimeout 产物，tokio 无此限，
//! 登记不移植（wakeAt 语义不变）。

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 现役 LOOP_DEFAULT_LIMITS 原值。
pub const LOOP_DEFAULT_LIMITS: LoopLimits = LoopLimits {
    max_turns: 50,
    max_consecutive_failures: 3,
    min_delay_sec: 60,
    guarded_min_delay_sec: 1200,
    fallback_delay_sec: 1200,
};

/// 现役 fallback / boot recovery 的闹钟 reason 常量。
pub const FALLBACK_ALARM_REASON: &str =
    "loop invariant fallback: the previous turn scheduled no alarm and left no live background work";
pub const BOOT_RECOVERY_REASON: &str = "loop recovery after restart";

/// 投递重试 / busy 顺延 / 过期 jitter 的时间表（现役常量；测试可整体替换为
/// 毫秒级，不改默认语义）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopTimings {
    pub delivery_retry_delays_ms: Vec<u64>,
    pub busy_defer_ms: u64,
    pub max_busy_deferrals: u32,
    pub overdue_jitter_min_ms: u64,
    pub overdue_jitter_max_ms: u64,
}

impl Default for LoopTimings {
    fn default() -> Self {
        Self {
            delivery_retry_delays_ms: vec![5_000, 25_000, 125_000],
            busy_defer_ms: 60_000,
            max_busy_deferrals: 30,
            overdue_jitter_min_ms: 1_000,
            overdue_jitter_max_ms: 5_000,
        }
    }
}

/// 现役 loop 状态词表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoopStatus {
    Running,
    Paused,
    Stopped,
    Completed,
}

impl LoopStatus {
    pub fn wire_name(self) -> &'static str {
        match self {
            LoopStatus::Running => "running",
            LoopStatus::Paused => "paused",
            LoopStatus::Stopped => "stopped",
            LoopStatus::Completed => "completed",
        }
    }
    pub fn is_active(self) -> bool {
        matches!(self, LoopStatus::Running | LoopStatus::Paused)
    }
}

/// 现役 limits 形状（camelCase JSON）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopLimits {
    pub max_turns: u32,
    pub max_consecutive_failures: u32,
    pub min_delay_sec: u64,
    pub guarded_min_delay_sec: u64,
    pub fallback_delay_sec: u64,
}

/// 现役 alarm 槽形状。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopAlarmSlot {
    #[serde(rename = "wakeAt")]
    pub wake_at: u64,
    pub reason: String,
}

/// 现役 target 形状：δ 只承接 desktop 面（桥接 loop 依赖现役桥挂钩，
/// Rust 桥运行时无对应物——T07_STATUS δ4 登记偏差）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopTarget {
    pub kind: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
}

impl LoopTarget {
    pub fn desktop(session_id: &str) -> Self {
        Self {
            kind: "desktop".to_string(),
            session_id: session_id.to_string(),
        }
    }
}

/// 现役 loop 记录逐字段同形（字段序 = 现役对象字面量序）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopRecord {
    pub key: String,
    pub target: LoopTarget,
    pub status: LoopStatus,
    pub prompt: String,
    pub turn_count: u32,
    pub consecutive_failures: u32,
    pub alarm: Option<LoopAlarmSlot>,
    pub limits: LoopLimits,
    pub paused_reason: Option<String>,
    pub completed_summary: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// 现役 LoopError（code + message）。
#[derive(Debug, Clone, PartialEq)]
pub struct LoopError {
    pub code: String,
    pub message: String,
}

impl LoopError {
    pub fn new(code: &str, message: String) -> Self {
        Self {
            code: code.to_string(),
            message,
        }
    }
    pub fn loop_target_reset() -> Self {
        Self::new(
            "loop_target_reset",
            "loop target session was reset".to_string(),
        )
    }
    fn is_target_reset(&self) -> bool {
        self.code == "loop_target_reset"
    }
}

impl std::fmt::Display for LoopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

// ── 存储（现役 loop-store.ts）────────────────────────────────────

/// 循环状态持久化：只负责状态 CRUD 与原子落盘；调度在 alarm，记账与护栏
/// 在 controller。每个会话至多一个循环记录。内部 Vec 保序 = 现役 Map 的
/// 插入序（落盘 loops[] 序字节平价；查找为线性——循环数 = 会话数，极小）。
pub struct LoopStore {
    path: PathBuf,
    loops: Mutex<Vec<LoopRecord>>,
    now: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl LoopStore {
    /// 加载：ENOENT → 空；JSON 损坏 → 改名 `.corrupt-<ts>` 保留现场并
    /// tracing::error，以空态继续。能解析但形状/版本/记录不合法时明确拒绝，
    /// 原件留在原路径，避免后续空状态保存覆盖未识别的数据。
    pub fn load(path: PathBuf, now: Box<dyn Fn() -> u64 + Send + Sync>) -> std::io::Result<Self> {
        let mut loops: Vec<LoopRecord> = Vec::new();
        match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(parsed) => {
                    let invalid = |detail: &str| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("loop state rejected; original retained: {detail}"),
                        )
                    };
                    if !parsed.is_object()
                        || parsed.get("schemaVersion").and_then(Value::as_u64) != Some(1)
                    {
                        return Err(invalid("missing or unsupported schemaVersion"));
                    }
                    let entries = parsed
                        .get("loops")
                        .and_then(Value::as_array)
                        .ok_or_else(|| invalid("loops must be an array"))?;
                    let mut keys = HashSet::new();
                    for entry in entries {
                        let record = serde_json::from_value::<LoopRecord>(entry.clone())
                            .map_err(|error| invalid(&format!("malformed record: {error}")))?;
                        if record.key.trim().is_empty()
                            || record.target.session_id != record.key
                            || record.target.kind != "desktop"
                        {
                            return Err(invalid("record key or target is invalid"));
                        }
                        if !keys.insert(record.key.clone()) {
                            return Err(invalid("duplicate loop key"));
                        }
                        loops.push(record);
                    }
                }
                Err(_) => {
                    let base = format!("{}.corrupt-{}", path.display(), (now)());
                    let mut backup = PathBuf::from(&base);
                    let mut suffix = 0u64;
                    while backup.exists() {
                        suffix += 1;
                        backup = PathBuf::from(format!("{base}-{suffix}"));
                    }
                    std::fs::rename(&path, &backup)?;
                    tracing::error!(
                        backup = %backup.display(),
                        "[loop-store] corrupt state file moved; starting empty"
                    );
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        Ok(Self {
            path,
            loops: Mutex::new(loops),
            now,
        })
    }

    fn save_locked(&self, loops: &[LoopRecord]) -> Result<(), LoopError> {
        let write = || -> Result<(), std::io::Error> {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            #[derive(Serialize)]
            struct LoopStateFile<'a> {
                #[serde(rename = "schemaVersion")]
                schema_version: u32,
                loops: &'a [LoopRecord],
            }
            let mut body = serde_json::to_vec_pretty(&LoopStateFile {
                schema_version: 1,
                loops,
            })?;
            body.push(b'\n');
            crate::paths::atomic_write(&self.path, &body)
        };
        write().map_err(|error| {
            LoopError::new(
                "loop_state_write_failed",
                format!("循环状态保存失败：{error}"),
            )
        })
    }

    pub fn get(&self, key: &str) -> Option<LoopRecord> {
        self.loops
            .lock()
            .ok()?
            .iter()
            .find(|record| record.key == key)
            .cloned()
    }

    /// 现役 create：active（running|paused）排他 → loop_already_active。
    pub fn create(
        &self,
        target: &LoopTarget,
        prompt: &str,
        limits_override: Option<LoopLimits>,
    ) -> Result<LoopRecord, LoopError> {
        let key = target.session_id.clone();
        if key.trim().is_empty() {
            return Err(LoopError::new(
                "loop-store",
                "loop-store: target.sessionId is required".to_string(),
            ));
        }
        let mut loops = self
            .loops
            .lock()
            .map_err(|_| LoopError::new("lock", "loop-store lock poisoned".to_string()))?;
        if let Some(existing) = loops.iter().find(|record| record.key == key) {
            if existing.status.is_active() {
                return Err(LoopError::new(
                    "loop_already_active",
                    format!("loop_already_active: {key}"),
                ));
            }
        }
        let now = (self.now)();
        let record = LoopRecord {
            key: key.clone(),
            target: target.clone(),
            status: LoopStatus::Running,
            prompt: prompt.to_string(),
            turn_count: 0,
            consecutive_failures: 0,
            alarm: None,
            limits: limits_override.unwrap_or(LOOP_DEFAULT_LIMITS),
            paused_reason: None,
            completed_summary: None,
            created_at: now,
            updated_at: now,
        };
        let mut next_loops = loops.clone();
        // 现役 Map.set 语义：已存在的键原位替换（保插入序），新键追加。
        if let Some(slot) = next_loops.iter_mut().find(|existing| existing.key == key) {
            *slot = record.clone();
        } else {
            next_loops.push(record.clone());
        }
        self.save_locked(&next_loops)?;
        *loops = next_loops;
        Ok(record)
    }

    /// 现役 update：patch 由调用方以闭包给出（Rust 无对象展开）；
    /// updatedAt 自动刷新。
    pub fn update(
        &self,
        key: &str,
        patch: impl FnOnce(&mut LoopRecord),
    ) -> Result<LoopRecord, LoopError> {
        let mut loops = self
            .loops
            .lock()
            .map_err(|_| LoopError::new("lock", "loop-store lock poisoned".to_string()))?;
        let now = (self.now)();
        let mut next_loops = loops.clone();
        let Some(record) = next_loops.iter_mut().find(|record| record.key == key) else {
            return Err(LoopError::new("no_loop", format!("no_loop: {key}")));
        };
        patch(record);
        record.updated_at = now;
        let next = record.clone();
        self.save_locked(&next_loops)?;
        *loops = next_loops;
        Ok(next)
    }

    pub fn remove(&self, key: &str) -> Result<(), LoopError> {
        let mut loops = self
            .loops
            .lock()
            .map_err(|_| LoopError::new("lock", "loop-store lock poisoned".into()))?;
        let mut next = loops.clone();
        next.retain(|record| record.key != key);
        if next.len() != loops.len() {
            self.save_locked(&next)?;
            *loops = next;
        }
        Ok(())
    }

    pub fn list_by_status(&self, status: LoopStatus) -> Vec<LoopRecord> {
        self.loops
            .lock()
            .map(|loops| {
                loops
                    .iter()
                    .filter(|record| record.status == status)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ── 消息构造器（现役 loop-messages.ts）────────────────────────────

pub const LOOP_TURN_MESSAGE_TYPE: &str = "loop-turn";
pub const LOOP_NOTICE_MESSAGE_TYPE: &str = "loop-notice";
pub const LOOP_USER_PROMPT_MESSAGE_TYPE: &str = "loop-user-prompt";

/// 注入消息形状：customType 由投递面决定落库形式（Rust 无 custom entry；
/// content 为模型可见文本，details 为结构化伴随）。
#[derive(Debug, Clone, PartialEq)]
pub struct LoopMessage {
    pub custom_type: &'static str,
    pub content: String,
    pub details: Value,
    run_identity: Option<LoopRunIdentity>,
}

impl LoopMessage {
    /// 只克隆投递时已经固定的身份，使用方不能按当前会话重新领取。
    pub fn run_identity(&self) -> Option<LoopRunIdentity> {
        self.run_identity.clone()
    }
}

pub fn build_loop_kickoff_message(record: &LoopRecord) -> LoopMessage {
    let content = [
        "<hana-loop kind=\"kickoff\">",
        "This session is now in recurring-loop mode.",
        &format!("Task: {}", record.prompt),
        "",
        "Each loop turn: do the work or the check now. Then take EXACTLY one of:",
        "1. Keep working, including dispatching background work as usual — its completion wakes this session automatically.",
        "2. Call loop_control {action:\"schedule\", delay_seconds, reason} to set the single fallback alarm for the next check. Use it only for external state the system cannot observe, or as a long hang-protection fallback. Never schedule short alarms to poll background work.",
        "3. Call loop_control {action:\"complete\", reason} once the task's goal is achieved — this ends the loop.",
        "",
        &format!(
            "Budget: {} loop turns; {} consecutive failed turns pause the loop automatically.",
            record.limits.max_turns, record.limits.max_consecutive_failures
        ),
        "</hana-loop>",
    ]
    .join("\n");
    LoopMessage {
        custom_type: LOOP_TURN_MESSAGE_TYPE,
        run_identity: None,
        content,
        details: json!({
            "schemaVersion": 1,
            "kind": "kickoff",
            "prompt": record.prompt,
            "turnCount": record.turn_count,
            "maxTurns": record.limits.max_turns,
        }),
    }
}

pub fn build_loop_wakeup_message(record: &LoopRecord, reason: &str) -> LoopMessage {
    let content = [
        "<hana-loop kind=\"wakeup\">",
        &format!(
            "Scheduled wakeup fired. Reason: {}",
            if reason.is_empty() { "(none recorded)" } else { reason }
        ),
        &format!("Loop task: {}", record.prompt),
        &format!(
            "Progress: loop turn {}/{}.",
            record.turn_count, record.limits.max_turns
        ),
        "Continue the task now; then schedule the next wakeup, or call loop_control {action:\"complete\"} if the goal is achieved.",
        "</hana-loop>",
    ]
    .join("\n");
    LoopMessage {
        custom_type: LOOP_TURN_MESSAGE_TYPE,
        run_identity: None,
        content,
        details: json!({
            "schemaVersion": 1,
            "kind": "wakeup",
            "prompt": record.prompt,
            "reason": if reason.is_empty() { Value::Null } else { json!(reason) },
            "turnCount": record.turn_count,
            "maxTurns": record.limits.max_turns,
        }),
    }
}

pub fn build_loop_notice_message(text: &str) -> LoopMessage {
    LoopMessage {
        custom_type: LOOP_NOTICE_MESSAGE_TYPE,
        run_identity: None,
        content: text.to_string(),
        details: json!({"schemaVersion": 1, "kind": "notice"}),
    }
}

// ── 控制器（现役 loop-controller.ts）──────────────────────────────

/// 现役投递模式词表（followUp 在 Rust 桌面无对应——投递面不产生它；
/// busy/suppressed/notifyOnly/stopped 由投递面或重置路径返回）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMode {
    TriggerTurn,
    FollowUp,
    Busy,
    Suppressed,
    NotifyOnly,
    Stopped,
}

/// 投递结果（现役 {mode} 形状）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    pub mode: DeliveryMode,
}

impl Delivery {
    pub fn mode(mode: DeliveryMode) -> Self {
        Self { mode }
    }
}

pub type BoxFutureSend<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// (target, message) => Delivery 投递闭包。
pub type DeliverLoopMessage = Arc<
    dyn Fn(LoopTarget, LoopMessage) -> BoxFutureSend<Result<Delivery, LoopError>> + Send + Sync,
>;
/// (target, message/prompt)：只落记录不触发轮的闭包。
pub type RecordOnly =
    Arc<dyn Fn(LoopTarget, LoopMessage) -> BoxFutureSend<Result<(), String>> + Send + Sync>;
/// (target, prompt)：展示型 user 气泡记录闭包。
pub type RecordUserPrompt =
    Arc<dyn Fn(LoopTarget, String) -> BoxFutureSend<Result<(), String>> + Send + Sync>;
/// (target, loop)：状态变更广播闭包。
pub type EmitLoopStatus = Arc<dyn Fn(&LoopTarget, &LoopRecord) + Send + Sync>;
/// (target) => bool 的异步探测闭包：后台工作守恒与可投递性都要查库
/// （现役 hasLiveBackgroundWork = taskRegistry 活跃 + deferred 待投递，
/// Rust 对应 TaskTools::list 的三源；现役 isRunnableSessionPath 对应
/// 会话行存在性），同步签名承载不了。
pub type TargetProbe = Arc<dyn Fn(LoopTarget) -> BoxFutureSend<bool> + Send + Sync>;

/// 控制器宿主挂钩（现役构造参数的 Rust 形态；全部为 Arc 闭包以便装配层
/// 一次性绑定）。会话身份统一为 LoopTarget（desktop: sessionId）。
#[derive(Clone)]
pub struct LoopHostHooks {
    /// (target) => bool：目标会话是否有活跃后台工作（守恒检查的一翼）。
    pub has_live_background_work: TargetProbe,
    /// (target, message) => Delivery；会话换代（reset/new）时须以
    /// code == "loop_target_reset" 的 LoopError 上抛。
    pub deliver_loop_message: DeliverLoopMessage,
    /// (target, message)：只落记录不触发轮。
    pub record_notice: RecordOnly,
    /// (target, prompt)：把用户任务 prompt 记成展示型 user 气泡（不进模型输入）。
    pub record_user_prompt: RecordUserPrompt,
    /// (target) => bool：目标会话是否在连续轮中（followUp 串未收束）。
    pub is_target_mid_stream: Arc<dyn Fn(&LoopTarget) -> bool + Send + Sync>,
    /// (target) => bool：目标会话是否仍可投递（recoverAtBoot 用）。
    pub is_target_runnable: TargetProbe,
    /// (target, loop)：状态变更广播（loop_status）。
    pub emit_loop_status: EmitLoopStatus,
}

pub type StorageErrorHandler = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// 只在本进程使用的循环身份；旧 await 持有旧 Arc，不能认领后续新循环。
type LoopGeneration = Arc<tokio::sync::watch::Sender<bool>>;
type ControlLifecycle = (LoopRecord, LoopGeneration, Option<Arc<()>>);

/// 由投递控制器签发的原始身份；不序列化、不接受HTTP或模型字段构造。
#[derive(Clone)]
pub struct LoopRunIdentity {
    session_id: String,
    generation: LoopGeneration,
    issuer: Arc<()>,
}

impl LoopRunIdentity {
    pub fn is_for_session(&self, session: &str) -> bool {
        self.session_id == session
    }
}

impl std::fmt::Debug for LoopRunIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoopRunIdentity")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

impl PartialEq for LoopRunIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && Arc::ptr_eq(&self.generation, &other.generation)
            && Arc::ptr_eq(&self.issuer, &other.issuer)
    }
}
impl Eq for LoopRunIdentity {}

/// 同会话的父任务、子任务及旧循环必须分别归属，不能共享一份失败标记。
struct MainRunObservation {
    session_id: String,
    generation: Option<LoopGeneration>,
    is_loop_origin: bool,
    has_error: bool,
    token: Arc<()>,
}

/// 已真正创建的主任务若未确认终态就退出驱动，不能悄悄继续循环。
pub struct LoopRunObserverGuard {
    controller: std::sync::Weak<LoopController>,
    session_id: String,
    run_id: String,
    token: Arc<()>,
}

impl Drop for LoopRunObserverGuard {
    fn drop(&mut self) {
        if let Some(controller) = self.controller.upgrade() {
            controller.abandon_main_run_owned(&self.session_id, &self.run_id, Some(&self.token));
        }
    }
}

pub struct LoopController {
    store: Arc<LoopStore>,
    storage_error_handler: Mutex<Option<StorageErrorHandler>>,
    hooks: LoopHostHooks,
    alarm: Mutex<Option<Arc<LoopAlarmService>>>,
    /// 状态切换与投递领取共用短同步边界，任何外部 await 均在锁外。
    lifecycles: Mutex<HashMap<String, LoopGeneration>>,
    identity_issuer: Arc<()>,
    pending_loop_turn: Mutex<HashSet<String>>,
    /// 每次投递独占本会话的领取；取消清理只可触碰自己的 token。
    delivery_claims: Mutex<HashMap<String, Arc<()>>>,
    main_runs: Mutex<HashMap<String, MainRunObservation>>,
    disposed: std::sync::atomic::AtomicBool,
    /// 落盘未确认时只在本进程阻止自动推进；不把它冒充已保存的暂停状态。
    storage_blocks: Mutex<HashMap<String, String>>,
}

/// Future 被取消或丢弃时也释放领取；旧 guard 不能清理后来新建的领取。
struct LoopDeliveryClaim<'a> {
    controller: &'a LoopController,
    key: &'a str,
    token: Arc<()>,
    settled: bool,
}

impl Drop for LoopDeliveryClaim<'_> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let _lifecycles = self
            .controller
            .lifecycles
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let mut claims = self
            .controller
            .delivery_claims
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if claims
            .get(self.key)
            .is_some_and(|current| Arc::ptr_eq(current, &self.token))
        {
            claims.remove(self.key);
            self.controller
                .pending_loop_turn
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(self.key);
        }
    }
}

impl LoopController {
    pub fn new(store: Arc<LoopStore>, hooks: LoopHostHooks) -> Arc<Self> {
        Arc::new(Self {
            store,
            storage_error_handler: Mutex::new(None),
            hooks,
            alarm: Mutex::new(None),
            lifecycles: Mutex::new(HashMap::new()),
            identity_issuer: Arc::new(()),
            pending_loop_turn: Mutex::new(HashSet::new()),
            delivery_claims: Mutex::new(HashMap::new()),
            main_runs: Mutex::new(HashMap::new()),
            disposed: std::sync::atomic::AtomicBool::new(false),
            storage_blocks: Mutex::new(HashMap::new()),
        })
    }

    /// 现役 attachAlarm：相互引用，装配层一次性绑定；重复绑定是程序错误。
    pub fn attach_alarm(self: &Arc<Self>, alarm: Arc<LoopAlarmService>) {
        let mut slot = self.alarm.lock().expect("alarm slot");
        if slot.is_some() {
            panic!("loop-controller: alarm already attached");
        }
        *slot = Some(alarm);
    }

    fn alarm(&self) -> Option<Arc<LoopAlarmService>> {
        self.alarm.lock().ok().and_then(|slot| slot.clone())
    }

    pub fn store(&self) -> &Arc<LoopStore> {
        &self.store
    }

    // ── 命令面 ────────────────────────────────────────────────

    pub async fn start(&self, target: &LoopTarget, prompt: &str) -> Result<LoopRecord, LoopError> {
        let key = target.session_id.clone();
        let generation = {
            let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
            if self
                .store
                .get(&key)
                .is_some_and(|record| record.status.is_active())
            {
                return Err(LoopError::new(
                    "loop_already_active",
                    "该会话已有活跃循环，先 /loop stop 再启动新的".to_string(),
                ));
            }
            self.store.create(target, prompt, None)?;
            Self::advance_lifecycle(&mut lifecycles, &key)
        };
        self.emit_status(&key);
        // 先记展示型用户气泡；等待期间的停止、暂停或新启动会取代本次启动。
        let hook = Arc::clone(&self.hooks.record_user_prompt);
        let prompt_result = tokio::select! {
            biased;
            _ = Self::wait_lifecycle_end(&generation) => return Err(Self::superseded_error()),
            result = hook(target.clone(), prompt.to_string()) => result,
        };
        if let Err(err) = prompt_result {
            tracing::warn!("[loop] record user prompt failed for {key}: {err}");
        }
        let record = self.current_lifecycle(&key, &generation)?;
        match self
            .inject_loop_turn(
                &key,
                target,
                build_loop_kickoff_message(&record),
                &generation,
            )
            .await
        {
            Ok(_) => self.current_lifecycle(&key, &generation),
            Err(err) => {
                if err.is_target_reset() {
                    self.stop_for_reset(&key, &generation).await;
                }
                Err(err)
            }
        }
    }

    pub async fn stop(&self, target: &LoopTarget) -> Result<LoopRecord, LoopError> {
        let key = target.session_id.clone();
        let stopped = {
            let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
            self.require_loop(&key)?;
            Self::advance_lifecycle(&mut lifecycles, &key);
            self.clear_runtime_marks(&key);
            let stopped = self.cancel_update_or_block(&key, |record| {
                record.status = LoopStatus::Stopped;
                record.paused_reason = None;
            })?;
            self.storage_blocks
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&key);
            stopped
        };
        self.emit_status(&key);
        Ok(stopped)
    }

    pub async fn pause(
        &self,
        target: &LoopTarget,
        reason_code: &str,
        notice_text: Option<String>,
    ) -> Result<LoopRecord, LoopError> {
        let key = target.session_id.clone();
        self.require_loop(&key)?;
        self.pause_inner(&key, reason_code, notice_text).await
    }

    pub async fn resume(&self, target: &LoopTarget) -> Result<LoopRecord, LoopError> {
        let key = target.session_id.clone();
        let (fresh, generation) = {
            let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
            let record = self.require_loop(&key)?;
            let storage_blocked = self.storage_blocked(&key);
            if record.status != LoopStatus::Paused && !storage_blocked {
                return Err(LoopError::new(
                    "loop_not_paused",
                    format!(
                        "循环当前状态为 {}，只有 paused 可 resume",
                        record.status.wire_name()
                    ),
                ));
            }
            let reset_turns = record.paused_reason.as_deref() == Some("budget_exhausted");
            let fresh = self.update_or_block(&key, |record| {
                record.status = LoopStatus::Running;
                record.paused_reason = None;
                record.consecutive_failures = 0;
                if reset_turns {
                    record.turn_count = 0;
                }
            })?;
            self.storage_blocks
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&key);
            let generation = Self::advance_lifecycle(&mut lifecycles, &key);
            (fresh, generation)
        };
        self.emit_status(&key);
        match self
            .inject_loop_turn(
                &key,
                &fresh.target,
                build_loop_wakeup_message(&fresh, "loop resumed"),
                &generation,
            )
            .await
        {
            Ok(_) => self.current_lifecycle(&key, &generation),
            Err(err) => {
                if err.is_target_reset() {
                    self.stop_for_reset(&key, &generation).await;
                }
                Err(err)
            }
        }
    }

    pub fn status_for_target(&self, target: &LoopTarget) -> Option<LoopRecord> {
        self.store.get(&target.session_id)
    }

    // ── 工具面 ────────────────────────────────────────────────

    pub fn tool_status(&self, session_id: &str) -> Option<LoopRecord> {
        self.store.get(session_id)
    }

    pub async fn tool_schedule(
        &self,
        session: &str,
        delay_sec: u64,
        reason: &str,
    ) -> Result<ScheduleOutcome, LoopError> {
        self.tool_schedule_scoped(session, None, delay_sec, reason)
            .await
    }

    pub async fn tool_schedule_for_run(
        &self,
        session: &str,
        run_id: &str,
        delay_sec: u64,
        reason: &str,
    ) -> Result<ScheduleOutcome, LoopError> {
        self.tool_schedule_scoped(session, Some(run_id), delay_sec, reason)
            .await
    }

    async fn tool_schedule_scoped(
        &self,
        session: &str,
        run_id: Option<&str>,
        delay_sec: u64,
        reason: &str,
    ) -> Result<ScheduleOutcome, LoopError> {
        let (record, generation, owner) = {
            let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
            self.control_lifecycle(&mut lifecycles, session, run_id)?
        };
        let alarm = self
            .alarm()
            .ok_or_else(|| LoopError::new("alarm", "loop alarm service unavailable".to_owned()))?;
        let live_work = tokio::select! {
            biased;
            _ = Self::wait_lifecycle_end(&generation) => return Err(Self::superseded_error()),
            live_work = (self.hooks.has_live_background_work)(record.target.clone()) => live_work,
        };
        let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        self.require_lifecycle(&lifecycles, session, &generation)?;
        let owner_is_current = owner.as_ref().is_none_or(|owner| {
            run_id.is_some_and(|run_id| {
                self.main_runs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(run_id)
                    .is_some_and(|run| run.session_id == session && Arc::ptr_eq(owner, &run.token))
            })
        });
        if !owner_is_current {
            return Err(Self::superseded_error());
        }
        alarm.schedule(session, delay_sec, reason, live_work)
    }

    pub async fn tool_complete(
        &self,
        session: &str,
        summary: &str,
    ) -> Result<LoopRecord, LoopError> {
        self.tool_complete_scoped(session, None, summary)
    }

    pub async fn tool_complete_for_run(
        &self,
        session: &str,
        run_id: &str,
        summary: &str,
    ) -> Result<LoopRecord, LoopError> {
        self.tool_complete_scoped(session, Some(run_id), summary)
    }

    fn tool_complete_scoped(
        &self,
        session: &str,
        run_id: Option<&str>,
        summary: &str,
    ) -> Result<LoopRecord, LoopError> {
        let completed = {
            let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
            self.control_lifecycle(&mut lifecycles, session, run_id)?;
            Self::advance_lifecycle(&mut lifecycles, session);
            self.clear_runtime_marks(session);
            let completed = self.cancel_update_or_block(session, |record| {
                record.status = LoopStatus::Completed;
                record.completed_summary = (!summary.is_empty()).then(|| summary.to_owned());
            })?;
            self.storage_blocks
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(session);
            completed
        };
        self.emit_status(session);
        Ok(completed)
    }

    fn control_lifecycle(
        &self,
        lifecycles: &mut HashMap<String, LoopGeneration>,
        session: &str,
        run_id: Option<&str>,
    ) -> Result<ControlLifecycle, LoopError> {
        if let Some(run_id) = run_id {
            let runs = self.main_runs.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(run) = runs.get(run_id) {
                if run.session_id != session {
                    return Err(Self::superseded_error());
                }
                if run.is_loop_origin {
                    let generation = run.generation.clone().ok_or_else(Self::superseded_error)?;
                    let record = self.require_lifecycle(lifecycles, session, &generation)?;
                    return Ok((record, generation, Some(run.token.clone())));
                }
            }
        }
        if self.store.get(session).is_none() {
            return Err(LoopError::new(
                "no_active_loop",
                "该会话没有活跃循环".to_owned(),
            ));
        }
        let record = self.require_running(session)?;
        let generation = lifecycles
            .entry(session.to_owned())
            .or_insert_with(Self::new_lifecycle)
            .clone();
        Ok((record, generation, None))
    }

    // ── 闹钟回调面 ────────────────────────────────────────────

    pub async fn deliver_wakeup_turn(&self, key: &str, reason: &str) -> Delivery {
        if self.storage_blocked(key) {
            return Delivery::mode(DeliveryMode::Stopped);
        }
        let Ok((record, generation)) = self.claim_current_lifecycle(key) else {
            return Delivery::mode(DeliveryMode::Suppressed);
        };
        match self
            .inject_loop_turn(
                key,
                &record.target,
                build_loop_wakeup_message(&record, reason),
                &generation,
            )
            .await
        {
            Ok(delivery) => delivery,
            Err(err) => {
                if err.is_target_reset() {
                    self.stop_for_reset(key, &generation).await;
                    return Delivery::mode(DeliveryMode::Stopped);
                }
                if err.code == "loop_lifecycle_superseded" {
                    return Delivery::mode(DeliveryMode::Stopped);
                }
                Delivery::mode(DeliveryMode::Suppressed)
            }
        }
    }

    pub fn pause_for_delivery_failure(
        self: &Arc<Self>,
        key: &str,
        err: &str,
        generation: Option<tokio::task::Id>,
    ) {
        let message =
            format!("（循环唤醒投递失败已重试耗尽，循环暂停：{err}。可用 /loop resume 恢复）");
        let this = Arc::clone(self);
        let key = key.to_string();
        // 现役 fire-and-forget（void this._pause(...)）。
        tokio::spawn(async move {
            if let Err(error) = this
                .pause_inner_generation(&key, "wakeup_delivery_failed", Some(message), generation)
                .await
            {
                tracing::error!(%error,key,"循环暂停未确认，保持本进程阻断，须明确恢复");
            }
        });
    }

    // ── 主任务观察与实际空闲检查 ───────────────────────────────

    /// 来源与代次必须同时匹配；这里只验证原身份，绝不领取当前身份替代它。
    pub fn validate_loop_identity(
        &self,
        session: &str,
        identity: &LoopRunIdentity,
    ) -> Result<(), LoopError> {
        self.validate_loop_identity_source(session, identity)?;
        let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        self.require_lifecycle(&lifecycles, session, &identity.generation)
            .map(|_| ())
    }

    fn validate_loop_identity_source(
        &self,
        session: &str,
        identity: &LoopRunIdentity,
    ) -> Result<(), LoopError> {
        if !identity.is_for_session(session)
            || !Arc::ptr_eq(&self.identity_issuer, &identity.issuer)
        {
            return Err(LoopError::new(
                "invalid_loop_run_identity",
                "循环投递身份与会话或服务来源不匹配".to_owned(),
            ));
        }
        Ok(())
    }

    /// 即使登记时已换代，也只保留原身份供取消收尾；绝不认领后来新建的循环。
    pub fn register_loop_run(
        self: &Arc<Self>,
        session: &str,
        run_id: &str,
        identity: &LoopRunIdentity,
    ) -> Option<LoopRunObserverGuard> {
        self.validate_loop_identity_source(session, identity).ok()?;
        let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        if self.is_disposed() {
            return None;
        }
        let mut runs = self.main_runs.lock().unwrap_or_else(|p| p.into_inner());
        if runs.contains_key(run_id) {
            return None;
        }
        if self
            .require_lifecycle(&lifecycles, session, &identity.generation)
            .is_ok()
        {
            self.pending_loop_turn
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(session);
        }
        let token = Arc::new(());
        runs.insert(
            run_id.to_owned(),
            MainRunObservation {
                session_id: session.to_owned(),
                generation: Some(identity.generation.clone()),
                is_loop_origin: true,
                has_error: false,
                token: token.clone(),
            },
        );
        Some(LoopRunObserverGuard {
            controller: Arc::downgrade(self),
            session_id: session.to_owned(),
            run_id: run_id.to_owned(),
            token,
        })
    }

    /// 只由已核准的主任务在持久创建后登记；子任务不能调用此入口。
    #[cfg(test)]
    pub fn register_main_run(
        self: &Arc<Self>,
        session: &str,
        run_id: &str,
        is_loop_origin: bool,
    ) -> Option<LoopRunObserverGuard> {
        let token = self.register_main_run_state(session, run_id, is_loop_origin)?;
        Some(LoopRunObserverGuard {
            controller: Arc::downgrade(self),
            session_id: session.to_owned(),
            run_id: run_id.to_owned(),
            token,
        })
    }

    #[cfg(test)]
    fn register_main_run_state(
        &self,
        session: &str,
        run_id: &str,
        is_loop_origin: bool,
    ) -> Option<Arc<()>> {
        let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        if self.disposed.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        let mut runs = self.main_runs.lock().unwrap_or_else(|p| p.into_inner());
        if runs.contains_key(run_id) {
            return None;
        }
        // 没有活动循环也冻结 None；以后创建循环时不能让旧任务重新认领。
        let generation = self.require_running(session).ok().map(|_| {
            lifecycles
                .entry(session.to_owned())
                .or_insert_with(Self::new_lifecycle)
                .clone()
        });
        if is_loop_origin && generation.is_some() {
            self.pending_loop_turn
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(session);
        }
        let token = Arc::new(());
        runs.insert(
            run_id.to_owned(),
            MainRunObservation {
                session_id: session.to_owned(),
                generation,
                is_loop_origin,
                has_error: false,
                token: token.clone(),
            },
        );
        Some(token)
    }

    /// 模型新一轮只重置本主任务的标记，不能触碰共享会话中的其它任务。
    pub fn on_run_model_turn_start(&self, session: &str, run_id: &str) {
        self.update_main_run_flag(session, run_id, false);
    }

    pub fn on_run_message_error(&self, session: &str, run_id: &str) {
        self.update_main_run_flag(session, run_id, true);
    }

    fn update_main_run_flag(&self, session: &str, run_id: &str, has_error: bool) {
        let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        let mut runs = self.main_runs.lock().unwrap_or_else(|p| p.into_inner());
        let Some(run) = runs.get_mut(run_id).filter(|run| run.session_id == session) else {
            return;
        };
        let Some(generation) = run.generation.as_ref() else {
            return;
        };
        if self
            .require_lifecycle(&lifecycles, session, generation)
            .is_ok()
        {
            run.has_error = has_error;
        }
    }

    /// 只在真实终态首次确认落盘后消费该主任务；此处不假设会话许可已经释放。
    pub async fn on_run_end(&self, session: &str, run_id: &str, aborted: bool) {
        let key = session.to_owned();
        let (fresh, generation, is_loop_turn) = {
            let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
            let mut runs = self.main_runs.lock().unwrap_or_else(|p| p.into_inner());
            if !runs
                .get(run_id)
                .is_some_and(|run| run.session_id == session)
            {
                return;
            }
            let run = runs.remove(run_id).expect("刚核对的主任务");
            let Some(generation) = run.generation else {
                return;
            };
            let Ok(record) = self.require_lifecycle(&lifecycles, session, &generation) else {
                return;
            };
            let fresh = if run.is_loop_origin {
                let ok = !aborted && !run.has_error;
                match self.update_or_block(session, |record| {
                    record.turn_count = record.turn_count.saturating_add(1);
                    record.consecutive_failures = if ok {
                        0
                    } else {
                        record.consecutive_failures.saturating_add(1)
                    };
                }) {
                    Ok(fresh) => fresh,
                    Err(error) => {
                        tracing::error!(%error,session,run_id,"循环主任务计数未保存，已阻止自动推进");
                        return;
                    }
                }
            } else {
                record
            };
            (fresh, generation, run.is_loop_origin)
        };
        if is_loop_turn {
            self.emit_status(session);
        }
        if fresh.turn_count >= fresh.limits.max_turns {
            let max = fresh.limits.max_turns;
            let _ = self
                .pause_inner_claim(
                    &key,
                    "budget_exhausted",
                    Some(format!(
                        "（循环已用完 {max} 轮预算，自动暂停。/loop resume 可重置并继续）"
                    )),
                    None,
                    Some(&generation),
                )
                .await;
            return;
        }
        if fresh.consecutive_failures >= fresh.limits.max_consecutive_failures {
            let failures = fresh.consecutive_failures;
            let _ = self
                .pause_inner_claim(
                    &key,
                    "consecutive_failures",
                    Some(format!(
                        "（循环连续 {failures} 轮失败，自动暂停。排查后可 /loop resume）"
                    )),
                    None,
                    Some(&generation),
                )
                .await;
        }
    }

    pub fn abandon_main_run(&self, session: &str, run_id: &str) {
        self.abandon_main_run_owned(session, run_id, None);
    }

    fn abandon_main_run_owned(&self, session: &str, run_id: &str, expected: Option<&Arc<()>>) {
        let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        let mut runs = self.main_runs.lock().unwrap_or_else(|p| p.into_inner());
        if !runs.get(run_id).is_some_and(|run| {
            run.session_id == session && expected.is_none_or(|token| Arc::ptr_eq(token, &run.token))
        }) {
            return;
        }
        let run = runs.remove(run_id).expect("刚核对的主任务");
        let Some(generation) = run.generation else {
            return;
        };
        if self
            .require_lifecycle(&lifecycles, session, &generation)
            .is_err()
        {
            return;
        }
        Self::advance_lifecycle(&mut lifecycles, session);
        self.block_storage(session, &LoopError::new("loop_run_outcome_unconfirmed", format!("主任务 {run_id} 的真实终态未确认，已在本进程停止自动推进；请核对实际任务后再明确恢复")));
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed.load(std::sync::atomic::Ordering::Acquire)
    }

    /// 最后的释放通知不能因派发容量不足而静默消失；仅阻断本机，不假称暂停已落盘。
    pub fn on_idle_dispatch_failed(&self, session: &str, detail: &str) {
        let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        if self.require_running(session).is_err() {
            return;
        }
        Self::advance_lifecycle(&mut lifecycles, session);
        self.block_storage(session, &LoopError::new("loop_idle_dispatch_failed", format!("会话已释放，但循环空闲检查未能派发，已在本进程停止自动推进：{detail}；请核对后明确恢复")));
    }

    /// 由真实许可释放通知触发。前后都读真实忙态，重复通知不会覆盖用户新约。
    pub async fn on_session_idle(&self, session: &str) {
        let Ok((record, generation)) = self.claim_current_lifecycle(session) else {
            return;
        };
        if record.alarm.is_some() || (self.hooks.is_target_mid_stream)(&record.target) {
            return;
        }
        let live_work = tokio::select! {
            biased;
            _ = Self::wait_lifecycle_end(&generation) => return,
            live_work = (self.hooks.has_live_background_work)(record.target.clone()) => live_work,
        };
        let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        let Ok(current) = self.require_lifecycle(&lifecycles, session, &generation) else {
            return;
        };
        if live_work
            || current.alarm.is_some()
            || (self.hooks.is_target_mid_stream)(&current.target)
        {
            return;
        }
        if let Some(alarm) = self.alarm() {
            if let Err(error) = alarm.schedule(
                session,
                current.limits.fallback_delay_sec,
                FALLBACK_ALARM_REASON,
                false,
            ) {
                self.block_storage(session, &error);
            }
        }
    }

    // 旧会话裸钩子只作为单元夹具适配；生产仅使用上述按真实 run_id 的接口。
    #[cfg(test)]
    fn on_turn_start(&self, session: &str) {
        let is_loop = self.pending_loop_turn.lock().unwrap().contains(session);
        let run_id = format!("unit-loop:{session}");
        let _ = self.register_main_run_state(session, &run_id, is_loop);
        self.on_run_model_turn_start(session, &run_id);
    }

    #[cfg(test)]
    fn on_message_end(&self, session: &str, stop_reason: &str) {
        if stop_reason == "error" {
            self.on_run_message_error(session, &format!("unit-loop:{session}"));
        }
    }

    #[cfg(test)]
    async fn on_turn_end(&self, session: &str, aborted: bool) {
        let run_id = format!("unit-loop:{session}");
        let _ = self.register_main_run_state(session, &run_id, false);
        self.on_run_end(session, &run_id, aborted).await;
        self.on_session_idle(session).await;
    }

    // ── 启动恢复 / 收尾 ───────────────────────────────────────

    pub async fn recover_at_boot(self: &Arc<Self>) {
        // 恢复只处理进入本轮恢复时的身份，不能在旧探测返回后重新认领用户新建的循环。
        let candidates: Vec<_> = self
            .store
            .list_by_status(LoopStatus::Running)
            .into_iter()
            .filter_map(|record| self.claim_current_lifecycle(&record.key).ok())
            .collect();
        for (record, generation) in &candidates {
            let runnable = tokio::select! {
                biased;
                _ = Self::wait_lifecycle_end(generation) => continue,
                runnable = (self.hooks.is_target_runnable)(record.target.clone()) => runnable,
            };
            if !runnable {
                let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
                if self
                    .require_lifecycle(&lifecycles, &record.key, generation)
                    .is_err()
                {
                    continue;
                }
                Self::advance_lifecycle(&mut lifecycles, &record.key);
                self.clear_runtime_marks(&record.key);
                let _ = self.cancel_update_or_block(&record.key, |record| {
                    record.status = LoopStatus::Stopped;
                    record.paused_reason = Some("target_unavailable".to_string());
                });
            }
        }
        if let Some(alarm) = self.alarm() {
            for (record, generation) in &candidates {
                let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
                let Ok(current) = self.require_lifecycle(&lifecycles, &record.key, generation)
                else {
                    continue;
                };
                if let Some(slot) = current.alarm {
                    let now = alarm.now();
                    let wake_at = if slot.wake_at <= now {
                        now.saturating_add(alarm.timings.overdue_jitter_min_ms)
                            .saturating_add(overdue_jitter_span(&alarm.timings))
                    } else {
                        slot.wake_at
                    };
                    alarm.arm(&current.key, wake_at);
                }
            }
        }
        for (record, generation) in &candidates {
            let Ok(current) = self.current_lifecycle(&record.key, generation) else {
                continue;
            };
            if current.alarm.is_some() {
                continue;
            }
            if let Some(alarm) = self.alarm() {
                let live_work = tokio::select! {
                    biased;
                    _ = Self::wait_lifecycle_end(generation) => continue,
                    live_work = (self.hooks.has_live_background_work)(current.target.clone()) => live_work,
                };
                let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
                let Ok(current) = self.require_lifecycle(&lifecycles, &record.key, generation)
                else {
                    continue;
                };
                if current.alarm.is_some() {
                    continue;
                }
                if let Err(err) = alarm.schedule(
                    &current.key,
                    current.limits.fallback_delay_sec,
                    BOOT_RECOVERY_REASON,
                    live_work,
                ) {
                    self.block_storage(&current.key, &err);
                }
            }
        }
    }

    pub fn dispose(&self) {
        self.disposed
            .store(true, std::sync::atomic::Ordering::Release);
        let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        for generation in lifecycles.values() {
            generation.send_replace(true);
        }
        lifecycles.clear();
        if let Ok(mut set) = self.pending_loop_turn.lock() {
            set.clear();
        }
        self.delivery_claims
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.main_runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    /// 测试专用：模拟「下一轮由循环触发」的 pending 标记。
    #[cfg(test)]
    pub fn mark_pending_loop_turn_for_test(&self, key: &str) {
        if let Ok(mut set) = self.pending_loop_turn.lock() {
            set.insert(key.to_string());
        }
    }

    // ── 内部 ─────────────────────────────────────────────────

    pub fn set_storage_error_handler(&self, handler: StorageErrorHandler) {
        *self
            .storage_error_handler
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(handler);
    }

    /// 本机运行阻断的原因；不代表磁盘已经成功保存为 paused。
    pub fn storage_error(&self, key: &str) -> Option<String> {
        self.storage_blocks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(key)
            .cloned()
    }

    fn storage_blocked(&self, key: &str) -> bool {
        self.storage_error(key).is_some()
    }

    fn block_storage(&self, key: &str, error: &LoopError) {
        self.storage_blocks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_owned(), error.to_string());
        self.clear_runtime_marks(key);
        if let Some(alarm) = self.alarm() {
            alarm.cancel(key);
        }
        tracing::error!(%error,key,"循环执行或状态未确认；本进程已停止自动推进，核对实际任务与存储后须 /loop resume 明确重试；磁盘状态未声明暂停成功");
        let handler = self
            .storage_error_handler
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(handler) = handler {
            handler(key, &error.to_string());
        }
    }

    fn cancel_update_or_block(
        &self,
        key: &str,
        patch: impl FnOnce(&mut LoopRecord),
    ) -> Result<LoopRecord, LoopError> {
        let result = if let Some(alarm) = self.alarm() {
            alarm.cancel_and_update(key, patch)
        } else {
            self.store.update(key, patch)
        };
        result.inspect_err(|error| self.block_storage(key, error))
    }

    fn update_or_block(
        &self,
        key: &str,
        patch: impl FnOnce(&mut LoopRecord),
    ) -> Result<LoopRecord, LoopError> {
        self.store
            .update(key, patch)
            .inspect_err(|error| self.block_storage(key, error))
    }

    fn advance_lifecycle(
        lifecycles: &mut HashMap<String, LoopGeneration>,
        key: &str,
    ) -> LoopGeneration {
        let generation = Self::new_lifecycle();
        if let Some(previous) = lifecycles.insert(key.to_owned(), generation.clone()) {
            previous.send_replace(true);
        }
        generation
    }

    fn new_lifecycle() -> LoopGeneration {
        let (cancel, _) = tokio::sync::watch::channel(false);
        Arc::new(cancel)
    }

    fn superseded_error() -> LoopError {
        LoopError::new(
            "loop_lifecycle_superseded",
            "本次循环操作已被后续停止、暂停或重新启动取代".to_owned(),
        )
    }

    async fn wait_lifecycle_end(generation: &LoopGeneration) {
        let mut cancel = generation.subscribe();
        loop {
            if *cancel.borrow_and_update() {
                return;
            }
            if cancel.changed().await.is_err() {
                return;
            }
        }
    }

    fn require_lifecycle(
        &self,
        lifecycles: &HashMap<String, LoopGeneration>,
        key: &str,
        generation: &LoopGeneration,
    ) -> Result<LoopRecord, LoopError> {
        if !lifecycles
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, generation))
        {
            return Err(Self::superseded_error());
        }
        self.require_running(key)
    }

    fn current_lifecycle(
        &self,
        key: &str,
        generation: &LoopGeneration,
    ) -> Result<LoopRecord, LoopError> {
        let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        self.require_lifecycle(&lifecycles, key, generation)
    }

    fn claim_current_lifecycle(
        &self,
        key: &str,
    ) -> Result<(LoopRecord, LoopGeneration), LoopError> {
        let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        let record = self.require_running(key)?;
        let generation = lifecycles
            .entry(key.to_owned())
            .or_insert_with(Self::new_lifecycle)
            .clone();
        Ok((record, generation))
    }

    fn require_loop(&self, key: &str) -> Result<LoopRecord, LoopError> {
        self.store
            .get(key)
            .ok_or_else(|| LoopError::new("no_loop", "该会话没有循环".to_string()))
    }

    fn require_running(&self, key: &str) -> Result<LoopRecord, LoopError> {
        if self.disposed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(LoopError::new(
                "loop_service_stopped",
                "循环服务已退出".to_owned(),
            ));
        }
        if self.storage_blocked(key) {
            return Err(LoopError::new(
                "loop_state_unconfirmed",
                "循环状态保存未确认，已停止自动推进；修复存储后用 /loop resume 重试".into(),
            ));
        }
        let record = self.require_loop(key)?;
        if record.status != LoopStatus::Running {
            return Err(LoopError::new(
                "loop_not_running",
                format!("循环当前状态为 {}", record.status.wire_name()),
            ));
        }
        Ok(record)
    }

    fn clear_runtime_marks(&self, key: &str) {
        if let Ok(mut set) = self.pending_loop_turn.lock() {
            set.remove(key);
        }
        self.delivery_claims
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(key);
        // 主任务身份保留到其真实终态；冻结的旧代次使晚到帧自动失效。
    }

    fn emit_status(&self, key: &str) {
        let Some(record) = self.store.get(key) else {
            return;
        };
        let emit = Arc::clone(&self.hooks.emit_loop_status);
        // 可选回调，panic 不影响循环本身（现役 try/catch 语义）。
        let target = record.target.clone();
        let snapshot = record.clone();
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            emit(&target, &snapshot);
        }))
        .unwrap_or_else(|_| tracing::warn!("[loop] emit status panicked for {key}"));
    }

    async fn stop_for_reset(&self, key: &str, generation: &LoopGeneration) {
        let result = {
            let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
            if self
                .require_lifecycle(&lifecycles, key, generation)
                .is_err()
            {
                return;
            }
            Self::advance_lifecycle(&mut lifecycles, key);
            self.clear_runtime_marks(key);
            self.cancel_update_or_block(key, |record| {
                record.status = LoopStatus::Stopped;
                record.paused_reason = Some("session_reset".to_string());
            })
        };
        if let Ok(record) = result {
            self.emit_status(key);
            let hook = Arc::clone(&self.hooks.record_notice);
            if let Err(err) = hook(
                record.target.clone(),
                build_loop_notice_message("（会话已重置，循环终止。需要的话在新会话里重新 /loop）"),
            )
            .await
            {
                tracing::warn!("[loop] reset notice failed for {key}: {err}");
            }
        }
    }

    async fn pause_inner(
        &self,
        key: &str,
        reason_code: &str,
        notice_text: Option<String>,
    ) -> Result<LoopRecord, LoopError> {
        self.pause_inner_generation(key, reason_code, notice_text, None)
            .await?
            .ok_or_else(|| LoopError::new("no_loop", "该会话没有循环".to_owned()))
    }

    async fn pause_inner_generation(
        &self,
        key: &str,
        reason_code: &str,
        notice_text: Option<String>,
        generation: Option<tokio::task::Id>,
    ) -> Result<Option<LoopRecord>, LoopError> {
        self.pause_inner_claim(key, reason_code, notice_text, generation, None)
            .await
    }

    async fn pause_inner_claim(
        &self,
        key: &str,
        reason_code: &str,
        notice_text: Option<String>,
        generation: Option<tokio::task::Id>,
        expected_lifecycle: Option<&LoopGeneration>,
    ) -> Result<Option<LoopRecord>, LoopError> {
        let paused = {
            let mut lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
            if expected_lifecycle
                .is_some_and(|expected| self.require_lifecycle(&lifecycles, key, expected).is_err())
            {
                return Ok(None);
            }
            let patch = |record: &mut LoopRecord| {
                record.status = LoopStatus::Paused;
                record.paused_reason = Some(reason_code.to_string());
            };
            let result = if let Some(alarm) = self.alarm() {
                alarm.cancel_and_update_generation(key, generation, patch)
            } else if generation.is_some() {
                Ok(None)
            } else {
                self.store.update(key, patch).map(Some)
            };
            let paused = match result {
                Ok(Some(paused)) => paused,
                Ok(None) => return Ok(None),
                Err(error) => {
                    Self::advance_lifecycle(&mut lifecycles, key);
                    self.block_storage(key, &error);
                    return Err(error);
                }
            };
            Self::advance_lifecycle(&mut lifecycles, key);
            self.clear_runtime_marks(key);
            paused
        };
        self.emit_status(key);
        if let Some(text) = notice_text {
            let hook = Arc::clone(&self.hooks.record_notice);
            if let Err(err) = hook(paused.target.clone(), build_loop_notice_message(&text)).await {
                tracing::warn!("[loop] pause notice failed for {key}: {err}");
            }
        }
        Ok(Some(paused))
    }

    async fn inject_loop_turn(
        &self,
        key: &str,
        target: &LoopTarget,
        mut message: LoopMessage,
        generation: &LoopGeneration,
    ) -> Result<Delivery, LoopError> {
        let (hook, mut claim) = {
            let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
            self.require_lifecycle(&lifecycles, key, generation)?;
            message.run_identity = Some(LoopRunIdentity {
                session_id: key.to_owned(),
                generation: generation.clone(),
                issuer: self.identity_issuer.clone(),
            });
            let mut claims = self
                .delivery_claims
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let mut pending = self
                .pending_loop_turn
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if claims.contains_key(key) || pending.contains(key) {
                // 已受理但尚未开轮也算占用，后来的 Busy 不能删除先前的预算标记。
                return Ok(Delivery::mode(DeliveryMode::Busy));
            }
            let token = Arc::new(());
            claims.insert(key.to_owned(), token.clone());
            pending.insert(key.to_owned());
            (
                Arc::clone(&self.hooks.deliver_loop_message),
                LoopDeliveryClaim {
                    controller: self,
                    key,
                    token,
                    settled: false,
                },
            )
        };
        let result = tokio::select! {
            biased;
            _ = Self::wait_lifecycle_end(generation) => return Err(Self::superseded_error()),
            result = hook(target.clone(), message) => result,
        };
        // claim 先于此锁声明，提前返回时会先释放锁，再运行其取消清理，避免重入死锁。
        let lifecycles = self.lifecycles.lock().unwrap_or_else(|p| p.into_inner());
        self.require_lifecycle(&lifecycles, key, generation)?;
        {
            let mut claims = self
                .delivery_claims
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if !claims
                .get(key)
                .is_some_and(|current| Arc::ptr_eq(current, &claim.token))
            {
                return Err(Self::superseded_error());
            }
            claims.remove(key);
            claim.settled = true;
        }
        let delivery = match result {
            Ok(delivery) => delivery,
            Err(error) => {
                self.pending_loop_turn
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(key);
                return Err(error);
            }
        };
        match delivery.mode {
            DeliveryMode::FollowUp => {
                self.pending_loop_turn
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(key);
                // Rust正式开轮由真实主任务登记，不能按会话伪造一个active身份。
            }
            DeliveryMode::TriggerTurn => {}
            _ => {
                self.pending_loop_turn
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(key);
            }
        }
        Ok(delivery)
    }
}

// ── 闹钟（现役 alarm-service.ts）──────────────────────────────────

/// 现役 schedule 的返回（{wakeAt, effectiveDelaySec}）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleOutcome {
    pub wake_at: u64,
    pub effective_delay_sec: u64,
}

/// (key, reason) => Delivery：闹钟到点的唤醒投递闭包。
pub type DeliverWakeup = Arc<dyn Fn(String, String) -> BoxFutureSend<Delivery> + Send + Sync>;
/// (key, error)：投递重试耗尽的处置闭包。
pub type OnDeliveryExhausted = Arc<dyn Fn(String, String, Option<tokio::task::Id>) + Send + Sync>;

/// 循环级单槽闹钟（键 = 循环键，即 sessionId）。
///
/// 兜底唤醒的定时层：每个循环至多一个待触发闹钟，新约替换旧约。后台工作
/// 完成的事件驱动投递是主唤醒路径；闹钟只兜「事件永远不来」的底，因此
/// 存在活跃后台任务时拒绝短闹钟。
pub struct LoopAlarmService {
    store: Arc<LoopStore>,
    deliver_wakeup: DeliverWakeup,
    on_delivery_exhausted: OnDeliveryExhausted,
    timings: LoopTimings,
    now: Box<dyn Fn() -> u64 + Send + Sync>,
    /// 仅包住同步领取/改约/取消，不持有此锁等待模型或退避。
    mutation_gate: Mutex<()>,
    timers: Mutex<HashMap<String, tokio::task::AbortHandle>>,
    /// 任务结束后仍保留领取代号，迟到的耗尽回调须在状态写入前核对。
    delivery_generations: Mutex<HashMap<String, tokio::task::Id>>,
    #[cfg(test)]
    mutation_hook: Mutex<Option<AlarmMutationHook>>,
    busy_deferrals: Mutex<HashMap<String, u32>>,
    closed: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
type AlarmMutationHook = Arc<dyn Fn(&str) + Send + Sync>;

/// 投递和退避期间仍归闹钟所有；停止时能中断正在进行的等待。
struct AlarmTaskGuard {
    service: std::sync::Weak<LoopAlarmService>,
    key: String,
    id: tokio::task::Id,
}
impl Drop for AlarmTaskGuard {
    fn drop(&mut self) {
        if let Some(service) = self.service.upgrade() {
            let mut timers = service.timers.lock().unwrap_or_else(|p| p.into_inner());
            if timers.get(&self.key).is_some_and(|h| h.id() == self.id) {
                timers.remove(&self.key);
            }
        }
    }
}

impl LoopAlarmService {
    pub fn new(
        store: Arc<LoopStore>,
        deliver_wakeup: DeliverWakeup,
        on_delivery_exhausted: OnDeliveryExhausted,
        timings: LoopTimings,
        now: Box<dyn Fn() -> u64 + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            deliver_wakeup,
            on_delivery_exhausted,
            timings,
            now,
            mutation_gate: Mutex::new(()),
            timers: Mutex::new(HashMap::new()),
            delivery_generations: Mutex::new(HashMap::new()),
            #[cfg(test)]
            mutation_hook: Mutex::new(None),
            busy_deferrals: Mutex::new(HashMap::new()),
            closed: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn now(&self) -> u64 {
        (self.now)()
    }

    /// 现役 schedule：钳 minDelaySec；活跃后台工作时短闹钟拒
    /// short_alarm_with_live_work；新约替换旧约。后台工作探测需查库，
    /// 由调用方（控制器，异步上下文）算好后以 has_live_work 传入。
    pub fn schedule(
        self: &Arc<Self>,
        key: &str,
        delay_sec: u64,
        reason: &str,
        has_live_work: bool,
    ) -> Result<ScheduleOutcome, LoopError> {
        let _mutation = self.mutation_gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(LoopError::new(
                "loop_alarm_stopped",
                "循环闹钟已停止".into(),
            ));
        }
        let Some(record) = self.store.get(key) else {
            return Err(LoopError::new(
                "no_active_loop",
                format!("no_active_loop: {key}"),
            ));
        };
        if record.status != LoopStatus::Running {
            return Err(LoopError::new(
                "loop_not_running",
                format!("loop_not_running: status={}", record.status.wire_name()),
            ));
        }
        let limits = record.limits;
        let effective_delay_sec = delay_sec.max(limits.min_delay_sec);
        if has_live_work && effective_delay_sec < limits.guarded_min_delay_sec {
            return Err(LoopError::new(
                "short_alarm_with_live_work",
                format!(
                    "Background work in this session wakes it automatically on completion; do not schedule alarms to poll it. While background work is live, alarms must be long fallbacks (>= {}s).",
                    limits.guarded_min_delay_sec
                ),
            ));
        }
        let wake_at = self
            .now()
            .saturating_add(effective_delay_sec.saturating_mul(1000));
        self.store.update(key, |record| {
            record.alarm = Some(LoopAlarmSlot {
                wake_at,
                reason: reason.to_string(),
            });
        })?;
        if let Ok(mut deferrals) = self.busy_deferrals.lock() {
            deferrals.remove(key);
        }
        #[cfg(test)]
        self.test_mutation_step("schedule_persisted");
        self.arm_locked(key, wake_at);
        Ok(ScheduleOutcome {
            wake_at,
            effective_delay_sec,
        })
    }

    /// 与状态变更一同取消：stop/pause 不能在两步之间被并发改约插入。
    fn cancel_and_update(
        &self,
        key: &str,
        patch: impl FnOnce(&mut LoopRecord),
    ) -> Result<LoopRecord, LoopError> {
        self.cancel_and_update_generation(key, None, patch)?
            .ok_or_else(|| LoopError::new("no_loop", "该会话没有循环".to_owned()))
    }

    fn cancel_and_update_generation(
        &self,
        key: &str,
        generation: Option<tokio::task::Id>,
        patch: impl FnOnce(&mut LoopRecord),
    ) -> Result<Option<LoopRecord>, LoopError> {
        let _mutation = self.mutation_gate.lock().unwrap_or_else(|p| p.into_inner());
        if generation.is_some_and(|id| {
            self.closed.load(std::sync::atomic::Ordering::Acquire)
                || self
                    .delivery_generations
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(key)
                    != Some(&id)
        }) {
            return Ok(None);
        }
        self.delivery_generations
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(key);
        self.clear_timer(key);
        self.busy_deferrals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(key);
        self.store
            .update(key, |record| {
                record.alarm = None;
                patch(record)
            })
            .map(Some)
    }

    pub fn cancel(&self, key: &str) {
        let _mutation = self.mutation_gate.lock().unwrap_or_else(|p| p.into_inner());
        self.delivery_generations
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(key);
        self.clear_timer(key);
        self.busy_deferrals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(key);
        if self
            .store
            .get(key)
            .is_some_and(|record| record.alarm.is_some())
        {
            if let Err(error) = self.store.update(key, |record| record.alarm = None) {
                tracing::error!(%error,key,"循环闹钟已在本进程取消，但磁盘清槽未确认");
            }
        }
    }

    /// 现役 rehydrate：running 循环的未过期闹钟重新武装；过期的加
    /// 1-5s jitter 立即兜底（防惊群）。
    pub fn rehydrate(self: &Arc<Self>) {
        for record in self.store.list_by_status(LoopStatus::Running) {
            let Some(slot) = record.alarm else {
                continue;
            };
            let now = self.now();
            let wake_at = if slot.wake_at <= now {
                now + self.timings.overdue_jitter_min_ms + overdue_jitter_span(&self.timings)
            } else {
                slot.wake_at
            };
            self.arm(&record.key, wake_at);
        }
    }

    pub fn dispose(&self) {
        let _mutation = self.mutation_gate.lock().unwrap_or_else(|p| p.into_inner());
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
        self.delivery_generations
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        if let Ok(mut timers) = self.timers.lock() {
            for (_, handle) in timers.drain() {
                handle.abort();
            }
        }
        if let Ok(mut deferrals) = self.busy_deferrals.lock() {
            deferrals.clear();
        }
    }

    /// 测试观测面：当前武装的闹钟键集合。
    #[cfg(test)]
    pub fn armed_keys(&self) -> Vec<String> {
        self.timers
            .lock()
            .map(|timers| timers.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn clear_timer(&self, key: &str) {
        if let Ok(mut timers) = self.timers.lock() {
            if let Some(handle) = timers.remove(key) {
                handle.abort();
            }
        }
    }

    fn arm(self: &Arc<Self>, key: &str, wake_at: u64) {
        let _mutation = self.mutation_gate.lock().unwrap_or_else(|p| p.into_inner());
        if !self
            .store
            .get(key)
            .is_some_and(|record| record.status == LoopStatus::Running && record.alarm.is_some())
        {
            return;
        }
        self.arm_locked(key, wake_at);
    }

    fn arm_locked(self: &Arc<Self>, key: &str, wake_at: u64) {
        let mut timers = self.timers.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(previous) = timers.remove(key) {
            previous.abort();
        }
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        let mut delay_ms = wake_at.saturating_sub(self.now());
        let this = Arc::clone(self);
        let key_owned = key.to_string();
        // 先登记再启动，零延迟任务也不能从清理登记中漏走。
        let (start, ready) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            if ready.await.is_err() {
                return;
            }
            let _guard = AlarmTaskGuard {
                service: Arc::downgrade(&this),
                key: key_owned.clone(),
                id: tokio::task::id(),
            };
            while delay_ms > 0 {
                let chunk = delay_ms.min(86_400_000);
                tokio::time::sleep(std::time::Duration::from_millis(chunk)).await;
                delay_ms -= chunk;
            }
            this.fire_owned(&key_owned, Some(tokio::task::id())).await;
        });
        self.delivery_generations
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_string(), handle.id());
        timers.insert(key.to_string(), handle.abort_handle());
        let _ = start.send(());
    }

    /// 现役 _fire：先清槽再投递；busy → 顺延；suppressed → 按失败重试；
    /// 抛错 → 按退避重试；耗尽 → fail-closed 交给钩子暂停循环。
    #[cfg(test)]
    async fn fire(self: &Arc<Self>, key: &str) {
        self.fire_owned(key, None).await;
    }

    fn owns_timer(&self, key: &str, expected: Option<tokio::task::Id>) -> bool {
        expected.is_none_or(|id| {
            self.timers
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(key)
                .is_some_and(|handle| handle.id() == id)
        })
    }

    #[cfg(test)]
    fn test_mutation_step(&self, step: &str) {
        let hook = self
            .mutation_hook
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(hook) = hook {
            hook(step);
        }
    }

    async fn fire_owned(self: &Arc<Self>, key: &str, expected: Option<tokio::task::Id>) {
        let claim = {
            let _mutation = self.mutation_gate.lock().unwrap_or_else(|p| p.into_inner());
            if self.closed.load(std::sync::atomic::Ordering::Acquire)
                || !self.owns_timer(key, expected)
            {
                return;
            }
            let Some(record) = self.store.get(key) else {
                return;
            };
            if record.status != LoopStatus::Running {
                return;
            }
            let Some(slot) = record.alarm else {
                return;
            };
            #[cfg(test)]
            self.test_mutation_step("fire_read_slot");
            self.store
                .update(key, |record| record.alarm = None)
                .map(|_| slot.reason)
        };
        let reason = match claim {
            Ok(reason) => reason,
            Err(error) => {
                tracing::error!(%error,key,"循环闹钟未确认清槽，停止投递");
                (self.on_delivery_exhausted)(key.to_owned(), error.to_string(), expected);
                return;
            }
        };
        let mut last_error = String::new();
        let retries = self.timings.delivery_retry_delays_ms.len();
        for attempt in 0..=retries {
            if !self.owns_timer(key, expected) {
                return;
            }
            let delivery = (self.deliver_wakeup)(key.to_owned(), reason.clone()).await;
            if !self.owns_timer(key, expected) {
                return;
            }
            match delivery.mode {
                DeliveryMode::Busy => {
                    self.defer_busy_owned(key, &reason, expected);
                    return;
                }
                mode if mode != DeliveryMode::Suppressed => {
                    self.busy_deferrals
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .remove(key);
                    return;
                }
                _ => last_error = "wakeup delivery suppressed".to_owned(),
            }
            if attempt < retries {
                tokio::time::sleep(std::time::Duration::from_millis(
                    self.timings.delivery_retry_delays_ms[attempt],
                ))
                .await;
            }
        }
        if self.owns_timer(key, expected) {
            tracing::warn!("[loop-alarm] wakeup delivery exhausted for {key}: {last_error}");
            (self.on_delivery_exhausted)(key.to_owned(), last_error, expected);
        }
    }

    #[cfg(test)]
    fn defer_busy(self: &Arc<Self>, key: &str, reason: &str) {
        self.defer_busy_owned(key, reason, None);
    }

    fn defer_busy_owned(
        self: &Arc<Self>,
        key: &str,
        reason: &str,
        expected: Option<tokio::task::Id>,
    ) {
        let failure = {
            let _mutation = self.mutation_gate.lock().unwrap_or_else(|p| p.into_inner());
            if self.closed.load(std::sync::atomic::Ordering::Acquire)
                || !self.owns_timer(key, expected)
            {
                return;
            }
            if !self
                .store
                .get(key)
                .is_some_and(|record| record.status == LoopStatus::Running)
            {
                return;
            }
            let count = {
                let mut deferrals = self
                    .busy_deferrals
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                let count = deferrals.get(key).copied().unwrap_or(0) + 1;
                deferrals.insert(key.to_owned(), count);
                count
            };
            if count > self.timings.max_busy_deferrals {
                self.busy_deferrals
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(key);
                Some("wakeup deferred too long: target stayed busy".to_owned())
            } else {
                let wake_at = self.now().saturating_add(self.timings.busy_defer_ms);
                match self.store.update(key, |record| {
                    record.alarm = Some(LoopAlarmSlot {
                        wake_at,
                        reason: reason.to_owned(),
                    })
                }) {
                    Ok(_) => {
                        self.arm_locked(key, wake_at);
                        None
                    }
                    Err(error) => {
                        tracing::error!(%error,key,"循环忙碌顺延未落盘，停止武装");
                        Some(error.to_string())
                    }
                }
            }
        };
        if let Some(error) = failure {
            (self.on_delivery_exhausted)(key.to_owned(), error, expected);
        }
    }
}

/// 过期 jitter：现役 1000 + floor(random()*4000)。随机源用 auth 的公共随机
/// 助手（非安全语义，惊群打散而已）；随机不可用时取中点，不阻断恢复。
fn overdue_jitter_span(timings: &LoopTimings) -> u64 {
    let span = timings
        .overdue_jitter_max_ms
        .saturating_sub(timings.overdue_jitter_min_ms);
    if span == 0 {
        return 0;
    }
    let random = crate::auth::hex_random_public(4)
        .ok()
        .and_then(|hex| u32::from_str_radix(&hex, 16).ok());
    match random {
        Some(value) => u64::from(value) % span,
        None => span / 2,
    }
}

// ── 纯函数与状态机单测（现役 vitest 口径）─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_path(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("lingxi-r07-loop-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root.join(".ephemeral").join("loop-state.json")
    }

    fn manual_clock(start: u64) -> (Arc<AtomicU64>, Box<dyn Fn() -> u64 + Send + Sync>) {
        let clock = Arc::new(AtomicU64::new(start));
        let handle = Arc::clone(&clock);
        (clock, Box::new(move || handle.load(Ordering::SeqCst)))
    }

    fn load_store(path: &Path, now: u64) -> LoopStore {
        let (_clock, now_fn) = manual_clock(now);
        LoopStore::load(path.to_path_buf(), now_fn).expect("store loads")
    }

    #[test]
    fn store_crud_and_active_exclusivity() {
        let path = test_path("crud");
        let store = load_store(&path, 1_000);
        let target = LoopTarget::desktop("sess-1");
        let created = store
            .create(&target, "watch the build", None)
            .expect("create");
        assert_eq!(created.key, "sess-1");
        assert_eq!(created.status, LoopStatus::Running);
        assert_eq!(created.limits, LOOP_DEFAULT_LIMITS);
        assert_eq!(created.created_at, 1_000);

        // active（running）排他。
        let err = store.create(&target, "again", None).unwrap_err();
        assert_eq!(err.code, "loop_already_active");
        assert_eq!(err.message, "loop_already_active: sess-1");

        // paused 也排他。
        store
            .update("sess-1", |record| {
                record.status = LoopStatus::Paused;
                record.paused_reason = Some("user".to_string());
            })
            .unwrap();
        assert!(store.create(&target, "again", None).is_err());

        // stopped 后可重建（现役 Map.set 原位替换）。
        store
            .update("sess-1", |record| {
                record.status = LoopStatus::Stopped;
            })
            .unwrap();
        let other = LoopTarget::desktop("sess-2");
        store.create(&other, "second", None).unwrap();
        let recreated = store.create(&target, "restarted", None).expect("recreate");
        assert_eq!(recreated.prompt, "restarted");
        assert_eq!(recreated.turn_count, 0);
        // 原位替换：sess-1 仍在 sess-2 前。
        let all = store.list_by_status(LoopStatus::Running);
        assert_eq!(all[0].key, "sess-1");
        assert_eq!(all[1].key, "sess-2");

        // update 不存在 → no_loop。
        let err = store.update("missing", |_| {}).unwrap_err();
        assert_eq!(err.code, "no_loop");
        assert_eq!(err.message, "no_loop: missing");

        store.remove("sess-2").unwrap();
        assert!(store.get("sess-2").is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn store_file_shape_two_space_trailing_newline_and_reload() {
        let path = test_path("shape");
        let store = load_store(&path, 7);
        store
            .create(&LoopTarget::desktop("s-shape"), "p", None)
            .unwrap();
        let raw = std::fs::read_to_string(&path).expect("state file");
        assert!(raw.ends_with("\n"), "trailing newline");
        assert!(
            raw.contains("\n  \"schemaVersion\": 1,\n  \"loops\": ["),
            "2-space indent"
        );
        assert!(
            raw.contains("\"sessionId\": \"s-shape\""),
            "camelCase target"
        );

        // 重新加载：记录回来（现役 _load 只收 key+target 俱全的行）。
        let reloaded = load_store(&path, 8);
        let record = reloaded.get("s-shape").expect("reloaded");
        assert_eq!(record.prompt, "p");
        assert_eq!(record.limits, LOOP_DEFAULT_LIMITS);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn store_corrupt_file_is_moved_aside_and_starts_empty() {
        let path = test_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        let store = load_store(&path, 42);
        assert!(store.get("anything").is_none());
        let backup = PathBuf::from(format!("{}.corrupt-42", path.display()));
        assert!(
            backup.exists(),
            "corrupt file renamed with the injected clock"
        );
        assert!(!path.exists(), "original path cleared for a fresh write");
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn kickoff_wakeup_notice_content_is_byte_exact() {
        let record = LoopRecord {
            key: "s".to_string(),
            target: LoopTarget::desktop("s"),
            status: LoopStatus::Running,
            prompt: "盯一下构建".to_string(),
            turn_count: 3,
            consecutive_failures: 0,
            alarm: None,
            limits: LOOP_DEFAULT_LIMITS,
            paused_reason: None,
            completed_summary: None,
            created_at: 0,
            updated_at: 0,
        };
        let kickoff = build_loop_kickoff_message(&record);
        assert_eq!(kickoff.custom_type, "loop-turn");
        assert_eq!(
            kickoff.content,
            concat!(
                "<hana-loop kind=\"kickoff\">\n",
                "This session is now in recurring-loop mode.\n",
                "Task: 盯一下构建\n",
                "\n",
                "Each loop turn: do the work or the check now. Then take EXACTLY one of:\n",
                "1. Keep working, including dispatching background work as usual — its completion wakes this session automatically.\n",
                "2. Call loop_control {action:\"schedule\", delay_seconds, reason} to set the single fallback alarm for the next check. Use it only for external state the system cannot observe, or as a long hang-protection fallback. Never schedule short alarms to poll background work.\n",
                "3. Call loop_control {action:\"complete\", reason} once the task's goal is achieved — this ends the loop.\n",
                "\n",
                "Budget: 50 loop turns; 3 consecutive failed turns pause the loop automatically.\n",
                "</hana-loop>"
            )
        );
        assert_eq!(
            kickoff.details,
            json!({"schemaVersion":1,"kind":"kickoff","prompt":"盯一下构建","turnCount":3,"maxTurns":50})
        );

        let wakeup = build_loop_wakeup_message(&record, "check the queue");
        assert_eq!(
            wakeup.content,
            concat!(
                "<hana-loop kind=\"wakeup\">\n",
                "Scheduled wakeup fired. Reason: check the queue\n",
                "Loop task: 盯一下构建\n",
                "Progress: loop turn 3/50.\n",
                "Continue the task now; then schedule the next wakeup, or call loop_control {action:\"complete\"} if the goal is achieved.\n",
                "</hana-loop>"
            )
        );
        assert_eq!(wakeup.details["reason"], json!("check the queue"));

        let empty_reason = build_loop_wakeup_message(&record, "");
        assert!(empty_reason.content.contains("Reason: (none recorded)"));
        assert_eq!(empty_reason.details["reason"], json!(null));

        let notice = build_loop_notice_message("hello");
        assert_eq!(notice.custom_type, "loop-notice");
        assert_eq!(notice.content, "hello");
        assert_eq!(notice.details, json!({"schemaVersion":1,"kind":"notice"}));
    }

    // ── 控制器/闹钟的异步状态机测试 ──

    struct FakeHost {
        deliveries: Mutex<Vec<DeliveryMode>>,
        notices: Mutex<Vec<String>>,
        user_prompts: Mutex<Vec<String>>,
        statuses: Mutex<Vec<LoopStatus>>,
        live_work: std::sync::atomic::AtomicBool,
        mid_stream: std::sync::atomic::AtomicBool,
        runnable: std::sync::atomic::AtomicBool,
    }

    impl Default for FakeHost {
        fn default() -> Self {
            Self {
                deliveries: Mutex::new(Vec::new()),
                notices: Mutex::new(Vec::new()),
                user_prompts: Mutex::new(Vec::new()),
                statuses: Mutex::new(Vec::new()),
                live_work: std::sync::atomic::AtomicBool::new(false),
                mid_stream: std::sync::atomic::AtomicBool::new(false),
                runnable: std::sync::atomic::AtomicBool::new(true),
            }
        }
    }

    fn hooks_for(host: Arc<FakeHost>, scripted: Arc<Mutex<Vec<DeliveryMode>>>) -> LoopHostHooks {
        let deliver_host = Arc::clone(&host);
        let notice_host = Arc::clone(&host);
        let prompt_host = Arc::clone(&host);
        let status_host = Arc::clone(&host);
        let mid_host = Arc::clone(&host);
        let live_host = Arc::clone(&host);
        let run_host = Arc::clone(&host);
        LoopHostHooks {
            has_live_background_work: Arc::new(move |_| {
                let live = live_host
                    .live_work
                    .load(std::sync::atomic::Ordering::SeqCst);
                Box::pin(async move { live })
            }),
            deliver_loop_message: Arc::new(move |_target, _message| {
                let scripted = Arc::clone(&scripted);
                let deliver_host = Arc::clone(&deliver_host);
                Box::pin(async move {
                    let mode = scripted.lock().unwrap().remove(0);
                    deliver_host.deliveries.lock().unwrap().push(mode);
                    Ok(Delivery::mode(mode))
                })
            }),
            record_notice: Arc::new(move |_target, message| {
                let notice_host = Arc::clone(&notice_host);
                Box::pin(async move {
                    notice_host.notices.lock().unwrap().push(message.content);
                    Ok(())
                })
            }),
            record_user_prompt: Arc::new(move |_target, prompt| {
                let prompt_host = Arc::clone(&prompt_host);
                Box::pin(async move {
                    prompt_host.user_prompts.lock().unwrap().push(prompt);
                    Ok(())
                })
            }),
            is_target_mid_stream: Arc::new(move |_| {
                mid_host
                    .mid_stream
                    .load(std::sync::atomic::Ordering::SeqCst)
            }),
            is_target_runnable: Arc::new(move |_| {
                let runnable = run_host.runnable.load(std::sync::atomic::Ordering::SeqCst);
                Box::pin(async move { runnable })
            }),
            emit_loop_status: Arc::new(move |_target, record| {
                status_host.statuses.lock().unwrap().push(record.status);
            }),
        }
    }

    type IdentityCapture = Arc<Mutex<Vec<LoopRunIdentity>>>;

    fn identity_capture_hooks() -> (LoopHostHooks, IdentityCapture) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let mut hooks = hooks_for(
            Arc::new(FakeHost::default()),
            Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn; 8])),
        );
        let deliver = hooks.deliver_loop_message.clone();
        hooks.deliver_loop_message = Arc::new({
            let captured = captured.clone();
            move |target, message| {
                captured
                    .lock()
                    .unwrap()
                    .push(message.run_identity().expect("正式投递必须持原身份"));
                deliver(target, message)
            }
        });
        (hooks, captured)
    }

    fn tiny_timings() -> LoopTimings {
        LoopTimings {
            delivery_retry_delays_ms: vec![1, 1, 1],
            busy_defer_ms: 1,
            max_busy_deferrals: 2,
            overdue_jitter_min_ms: 1,
            overdue_jitter_max_ms: 2,
        }
    }

    fn attach_alarm(
        controller: &Arc<LoopController>,
        store: &Arc<LoopStore>,
    ) -> Arc<LoopAlarmService> {
        let weak = Arc::downgrade(controller);
        let alarm = LoopAlarmService::new(
            Arc::clone(store),
            Arc::new(move |key, reason| {
                let weak = weak.clone();
                Box::pin(async move {
                    let controller = weak.upgrade().expect("controller alive");
                    controller.deliver_wakeup_turn(&key, &reason).await
                })
            }),
            Arc::new({
                let controller = Arc::downgrade(controller);
                move |key, err, generation| {
                    if let Some(controller) = controller.upgrade() {
                        controller.pause_for_delivery_failure(&key, &err, generation);
                    }
                }
            }),
            tiny_timings(),
            Box::new(|| 1_000),
        );
        controller.attach_alarm(Arc::clone(&alarm));
        alarm
    }

    #[tokio::test]
    async fn blocked_start_is_superseded_by_stop_pause_resume_or_restart() {
        for action in ["stop", "pause", "resume", "restart"] {
            let path = test_path(&format!("start-lifecycle-{action}"));
            let store = Arc::new(load_store(&path, 1_000));
            let host = Arc::new(FakeHost::default());
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let mut hooks = hooks_for(
                host.clone(),
                Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn])),
            );
            hooks.record_user_prompt = Arc::new({
                let entered = entered.clone();
                let release = release.clone();
                move |_, prompt| {
                    let entered = entered.clone();
                    let release = release.clone();
                    Box::pin(async move {
                        if prompt == "old start" {
                            entered.notify_one();
                            release.notified().await;
                        }
                        Ok(())
                    })
                }
            });
            let controller = LoopController::new(store.clone(), hooks);
            let alarm = attach_alarm(&controller, &store);
            let old_start = tokio::spawn({
                let controller = controller.clone();
                async move {
                    controller
                        .start(&LoopTarget::desktop("s"), "old start")
                        .await
                }
            });
            tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            let target = LoopTarget::desktop("s");
            match action {
                "pause" => {
                    controller.pause(&target, "user", None).await.unwrap();
                }
                "resume" => {
                    controller.pause(&target, "user", None).await.unwrap();
                    controller.resume(&target).await.unwrap();
                }
                "restart" => {
                    controller.stop(&target).await.unwrap();
                    controller.start(&target, "new start").await.unwrap();
                }
                _ => {
                    controller.stop(&target).await.unwrap();
                }
            }
            // 无需释放旧记录钩子，停止本身就应使启动返回明确的取代结果。
            let error = tokio::time::timeout(std::time::Duration::from_secs(2), old_start)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert_eq!(error.code, "loop_lifecycle_superseded", "{action}");
            release.notify_waiters();
            let record = store.get("s").unwrap();
            match action {
                "stop" => assert_eq!(record.status, LoopStatus::Stopped),
                "pause" => assert_eq!(record.status, LoopStatus::Paused),
                "restart" => {
                    assert_eq!(record.status, LoopStatus::Running);
                    assert_eq!(record.prompt, "new start");
                }
                _ => assert_eq!(record.status, LoopStatus::Running),
            }
            let new_turn = matches!(action, "resume" | "restart");
            assert_eq!(host.deliveries.lock().unwrap().len(), usize::from(new_turn));
            assert_eq!(
                controller.pending_loop_turn.lock().unwrap().contains("s"),
                new_turn
            );
            assert!(controller.main_runs.lock().unwrap().is_empty());
            assert!(alarm.armed_keys().is_empty());
            alarm.dispose();
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }

    #[tokio::test]
    async fn blocked_resume_delivery_cannot_clear_or_stop_the_new_loop() {
        let path = test_path("resume-delivery-lifecycle");
        let store = Arc::new(load_store(&path, 1_000));
        let target = LoopTarget::desktop("s");
        store.create(&target, "old", None).unwrap();
        let host = Arc::new(FakeHost::default());
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(AtomicU64::new(0));
        let mut hooks = hooks_for(host.clone(), Arc::new(Mutex::new(vec![])));
        hooks.deliver_loop_message = Arc::new({
            let entered = entered.clone();
            let release = release.clone();
            let calls = calls.clone();
            let host = host.clone();
            move |_, _| {
                let entered = entered.clone();
                let release = release.clone();
                let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
                let host = host.clone();
                Box::pin(async move {
                    if first {
                        entered.notify_one();
                        release.notified().await;
                        return Err(LoopError::loop_target_reset());
                    }
                    host.deliveries
                        .lock()
                        .unwrap()
                        .push(DeliveryMode::TriggerTurn);
                    Ok(Delivery::mode(DeliveryMode::TriggerTurn))
                })
            }
        });
        let controller = LoopController::new(store.clone(), hooks);
        let alarm = attach_alarm(&controller, &store);
        controller.pause(&target, "user", None).await.unwrap();
        let old_resume = tokio::spawn({
            let controller = controller.clone();
            async move { controller.resume(&LoopTarget::desktop("s")).await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        assert!(controller.pending_loop_turn.lock().unwrap().contains("s"));
        controller.stop(&target).await.unwrap();
        controller.start(&target, "new loop").await.unwrap();
        assert!(controller.pending_loop_turn.lock().unwrap().contains("s"));
        // 即使旧钩子会返回会话重置错误，也不能终止已经换代的新循环。
        release.notify_waiters();
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), old_resume)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, "loop_lifecycle_superseded");
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Running);
        assert_eq!(store.get("s").unwrap().prompt, "new loop");
        assert!(controller.pending_loop_turn.lock().unwrap().contains("s"));
        assert_eq!(host.deliveries.lock().unwrap().len(), 1);
        assert!(controller.delivery_claims.lock().unwrap().is_empty());
        assert!(host.notices.lock().unwrap().is_empty());
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn blocked_live_work_query_cannot_schedule_for_a_restarted_loop() {
        let path = test_path("schedule-lifecycle");
        let store = Arc::new(load_store(&path, 1_000));
        let target = LoopTarget::desktop("s");
        store.create(&target, "old", None).unwrap();
        let host = Arc::new(FakeHost::default());
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let mut hooks = hooks_for(host, Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn])));
        hooks.has_live_background_work = Arc::new({
            let entered = entered.clone();
            let release = release.clone();
            move |_| {
                let entered = entered.clone();
                let release = release.clone();
                Box::pin(async move {
                    entered.notify_one();
                    release.notified().await;
                    false
                })
            }
        });
        let controller = LoopController::new(store.clone(), hooks);
        let alarm = attach_alarm(&controller, &store);
        let old_schedule = tokio::spawn({
            let controller = controller.clone();
            async move { controller.tool_schedule("s", 60, "old alarm").await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        controller.stop(&target).await.unwrap();
        controller.start(&target, "new loop").await.unwrap();
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), old_schedule)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, "loop_lifecycle_superseded");
        release.notify_waiters();
        assert!(store.get("s").unwrap().alarm.is_none());
        assert!(alarm.armed_keys().is_empty());
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn automatic_queries_cannot_mutate_a_new_lifecycle_or_replace_a_new_alarm() {
        for phase in [
            "turn-end",
            "boot-runnable",
            "boot-live",
            "turn-end-new-alarm",
        ] {
            let path = test_path(&format!("automatic-lifecycle-{phase}"));
            let store = Arc::new(load_store(&path, 1_000));
            let target = LoopTarget::desktop("s");
            store.create(&target, "old", None).unwrap();
            let host = Arc::new(FakeHost::default());
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let mut hooks = hooks_for(host, Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn])));
            let query: TargetProbe = Arc::new({
                let entered = entered.clone();
                let release = release.clone();
                move |_| {
                    let entered = entered.clone();
                    let release = release.clone();
                    Box::pin(async move {
                        entered.notify_one();
                        release.notified().await;
                        false
                    })
                }
            });
            if phase == "boot-runnable" {
                hooks.is_target_runnable = query;
            } else {
                hooks.has_live_background_work = query;
            }
            let controller = LoopController::new(store.clone(), hooks);
            let alarm = attach_alarm(&controller, &store);
            let old_work = tokio::spawn({
                let controller = controller.clone();
                async move {
                    if phase.starts_with("turn-end") {
                        controller.on_turn_end("s", false).await;
                    } else {
                        controller.recover_at_boot().await;
                    }
                }
            });
            tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            if phase == "turn-end-new-alarm" {
                // 同一代次等待期间，用户已续约，也不能用旧的空槽快照覆盖它。
                alarm.schedule("s", 60, "user alarm", false).unwrap();
                release.notify_waiters();
            } else {
                controller.stop(&target).await.unwrap();
                controller.start(&target, "new loop").await.unwrap();
            }
            tokio::time::timeout(std::time::Duration::from_secs(2), old_work)
                .await
                .unwrap()
                .unwrap();
            release.notify_waiters();
            assert_eq!(store.get("s").unwrap().status, LoopStatus::Running);
            if phase == "turn-end-new-alarm" {
                assert_eq!(store.get("s").unwrap().alarm.unwrap().reason, "user alarm");
            } else {
                assert_eq!(store.get("s").unwrap().prompt, "new loop");
                assert!(store.get("s").unwrap().alarm.is_none());
                assert!(alarm.armed_keys().is_empty());
                assert!(controller.pending_loop_turn.lock().unwrap().contains("s"));
            }
            alarm.dispose();
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }

    #[tokio::test]
    async fn same_lifecycle_busy_delivery_cannot_erase_an_accepted_turn_marker() {
        let path = test_path("delivery-pending-owner");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(AtomicU64::new(0));
        let mut hooks = hooks_for(Arc::new(FakeHost::default()), Arc::new(Mutex::new(vec![])));
        hooks.deliver_loop_message = Arc::new({
            let entered = entered.clone();
            let release = release.clone();
            let calls = calls.clone();
            move |_, _| {
                let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
                let entered = entered.clone();
                let release = release.clone();
                Box::pin(async move {
                    if first {
                        entered.notify_one();
                        release.notified().await;
                        Ok(Delivery::mode(DeliveryMode::TriggerTurn))
                    } else {
                        Ok(Delivery::mode(DeliveryMode::Busy))
                    }
                })
            }
        });
        let controller = LoopController::new(store.clone(), hooks);
        let alarm = attach_alarm(&controller, &store);
        let first = tokio::spawn({
            let controller = controller.clone();
            async move { controller.deliver_wakeup_turn("s", "first").await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        assert_eq!(
            controller.deliver_wakeup_turn("s", "second").await.mode,
            DeliveryMode::Busy
        );
        assert!(controller.pending_loop_turn.lock().unwrap().contains("s"));
        assert_eq!(controller.delivery_claims.lock().unwrap().len(), 1);
        release.notify_waiters();
        assert_eq!(first.await.unwrap().mode, DeliveryMode::TriggerTurn);
        // 受理回执已到但真正开轮尚未发生时，也不能把先前 pending 当成自己的清掉。
        assert_eq!(
            controller.deliver_wakeup_turn("s", "third").await.mode,
            DeliveryMode::Busy
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(controller.delivery_claims.lock().unwrap().is_empty());
        controller.on_turn_start("s");
        controller.on_turn_end("s", false).await;
        assert_eq!(store.get("s").unwrap().turn_count, 1);
        assert!(controller.pending_loop_turn.lock().unwrap().is_empty());
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn dropping_delivery_wait_reclaims_only_its_own_pending_and_allows_retry() {
        let path = test_path("delivery-claim-drop");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(AtomicU64::new(0));
        let mut hooks = hooks_for(Arc::new(FakeHost::default()), Arc::new(Mutex::new(vec![])));
        hooks.deliver_loop_message = Arc::new({
            let entered = entered.clone();
            let calls = calls.clone();
            move |_, _| {
                let first = calls.fetch_add(1, Ordering::SeqCst) == 0;
                let entered = entered.clone();
                Box::pin(async move {
                    if first {
                        entered.notify_one();
                        std::future::pending::<()>().await;
                    }
                    Ok(Delivery::mode(DeliveryMode::TriggerTurn))
                })
            }
        });
        let controller = LoopController::new(store.clone(), hooks);
        let alarm = attach_alarm(&controller, &store);
        let first = tokio::spawn({
            let controller = controller.clone();
            async move { controller.deliver_wakeup_turn("s", "cancelled wait").await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        assert!(controller.delivery_claims.lock().unwrap().is_empty());
        assert!(controller.pending_loop_turn.lock().unwrap().is_empty());
        assert_eq!(
            controller.deliver_wakeup_turn("s", "retry").await.mode,
            DeliveryMode::TriggerTurn
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(controller.delivery_claims.lock().unwrap().is_empty());
        assert!(controller.pending_loop_turn.lock().unwrap().contains("s"));
        controller.on_turn_start("s");
        controller.on_turn_end("s", false).await;
        assert_eq!(store.get("s").unwrap().turn_count, 1);
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn main_run_ownership_separates_children_errors_and_foreground_turns() {
        let path = test_path("main-run-owner");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        let host = Arc::new(FakeHost::default());
        let controller =
            LoopController::new(store.clone(), hooks_for(host, Arc::new(Mutex::new(vec![]))));
        let alarm = attach_alarm(&controller, &store);
        controller.mark_pending_loop_turn_for_test("s");
        let parent = controller.register_main_run("s", "parent", true).unwrap();
        controller.on_run_model_turn_start("s", "parent");
        controller.on_run_message_error("s", "parent");
        // 子任务没有受信主任务登记；同session的任何帧都不能重置或消费父任务。
        controller.on_run_model_turn_start("s", "child");
        controller.on_run_message_error("s", "child");
        controller.on_run_end("s", "child", false).await;
        assert_eq!(store.get("s").unwrap().turn_count, 0);
        controller.on_run_end("s", "parent", false).await;
        controller.on_run_end("s", "parent", false).await;
        drop(parent);
        assert_eq!(store.get("s").unwrap().turn_count, 1);
        assert_eq!(store.get("s").unwrap().consecutive_failures, 1);
        assert!(controller.storage_error("s").is_none());
        assert!(
            store.get("s").unwrap().alarm.is_none(),
            "结算时不假定permit已经释放"
        );
        controller.mark_pending_loop_turn_for_test("s");
        let foreground = controller
            .register_main_run("s", "foreground", false)
            .unwrap();
        assert!(
            controller.pending_loop_turn.lock().unwrap().contains("s"),
            "普通用户任务不能抢循环pending"
        );
        controller.on_run_model_turn_start("s", "foreground");
        controller.on_run_message_error("s", "foreground");
        controller.on_run_end("s", "foreground", false).await;
        drop(foreground);
        assert_eq!(store.get("s").unwrap().turn_count, 1);
        assert_eq!(store.get("s").unwrap().consecutive_failures, 1);
        assert!(controller.main_runs.lock().unwrap().is_empty());
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn old_main_run_frames_never_rebind_after_stop_and_restart() {
        let path = test_path("main-run-frozen-generation");
        let store = Arc::new(load_store(&path, 1_000));
        let target = LoopTarget::desktop("s");
        store.create(&target, "old", None).unwrap();
        let controller = LoopController::new(
            store.clone(),
            hooks_for(
                Arc::new(FakeHost::default()),
                Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn])),
            ),
        );
        let alarm = attach_alarm(&controller, &store);
        let old = controller.register_main_run("s", "old-run", true).unwrap();
        controller.on_run_model_turn_start("s", "old-run");
        controller.stop(&target).await.unwrap();
        controller.start(&target, "new").await.unwrap();
        let new = controller.register_main_run("s", "new-run", true).unwrap();
        controller.on_run_model_turn_start("s", "new-run");
        controller.on_run_message_error("s", "new-run");
        controller.on_run_model_turn_start("s", "old-run");
        controller.on_run_end("s", "old-run", true).await;
        drop(old);
        assert_eq!(store.get("s").unwrap().turn_count, 0);
        assert!(controller.storage_error("s").is_none());
        controller.on_run_end("s", "new-run", false).await;
        drop(new);
        assert_eq!(store.get("s").unwrap().turn_count, 1);
        assert_eq!(store.get("s").unwrap().consecutive_failures, 1);
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn observer_drop_blocks_unconfirmed_main_run_but_never_a_new_loop() {
        for had_loop in [false, true] {
            let path = test_path(&format!("main-run-guard-{had_loop}"));
            let store = Arc::new(load_store(&path, 1_000));
            let target = LoopTarget::desktop("s");
            if had_loop {
                store.create(&target, "old", None).unwrap();
            }
            let controller = LoopController::new(
                store.clone(),
                hooks_for(
                    Arc::new(FakeHost::default()),
                    Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn])),
                ),
            );
            let alarm = attach_alarm(&controller, &store);
            let old = controller.register_main_run("s", "old", true).unwrap();
            if had_loop {
                controller.stop(&target).await.unwrap();
            }
            controller.start(&target, "new").await.unwrap();
            drop(old);
            assert!(
                controller.storage_error("s").is_none(),
                "旧任务或无循环时开始的任务不能阻断新循环"
            );
            let current = controller
                .register_main_run("s", "unconfirmed", true)
                .unwrap();
            drop(current);
            assert!(controller
                .storage_error("s")
                .unwrap()
                .contains("真实终态未确认"));
            assert_eq!(store.get("s").unwrap().status, LoopStatus::Running);
            assert_eq!(store.get("s").unwrap().turn_count, 0);
            assert!(store.get("s").unwrap().alarm.is_none());
            controller.on_session_idle("s").await;
            assert!(alarm.armed_keys().is_empty());
            alarm.dispose();
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }

    #[tokio::test]
    async fn idle_checks_wait_for_real_release_and_dispose_fences_late_notifications() {
        let path = test_path("idle-real-busy");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        let host = Arc::new(FakeHost::default());
        host.mid_stream.store(true, Ordering::SeqCst);
        let controller = LoopController::new(
            store.clone(),
            hooks_for(host.clone(), Arc::new(Mutex::new(vec![]))),
        );
        let alarm = attach_alarm(&controller, &store);
        let owner = controller.register_main_run("s", "run", true).unwrap();
        controller.on_run_end("s", "run", false).await;
        drop(owner);
        assert_eq!(store.get("s").unwrap().turn_count, 1);
        assert!(store.get("s").unwrap().alarm.is_none());
        controller.on_session_idle("s").await;
        assert!(store.get("s").unwrap().alarm.is_none());
        host.mid_stream.store(false, Ordering::SeqCst);
        controller.on_session_idle("s").await;
        assert_eq!(
            store.get("s").unwrap().alarm.unwrap().reason,
            FALLBACK_ALARM_REASON
        );
        let timer = alarm.timers.lock().unwrap().get("s").unwrap().id();
        controller.on_session_idle("s").await;
        assert_eq!(alarm.timers.lock().unwrap().get("s").unwrap().id(), timer);
        alarm.cancel("s");
        controller.dispose();
        assert!(controller.is_disposed());
        controller.on_session_idle("s").await;
        controller.on_idle_dispatch_failed("s", "after exit");
        assert!(controller
            .register_main_run("s", "after-exit", true)
            .is_none());
        assert!(controller.storage_error("s").is_none());
        assert!(store.get("s").unwrap().alarm.is_none());
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn idle_rechecks_busy_after_query_and_reports_dispatch_failure_honestly() {
        let path = test_path("idle-recheck-and-failure");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        let host = Arc::new(FakeHost::default());
        let mut hooks = hooks_for(host.clone(), Arc::new(Mutex::new(vec![])));
        hooks.has_live_background_work = Arc::new({
            let host = host.clone();
            move |_| {
                host.mid_stream.store(true, Ordering::SeqCst);
                Box::pin(async { false })
            }
        });
        let controller = LoopController::new(store.clone(), hooks);
        let alarm = attach_alarm(&controller, &store);
        controller.on_session_idle("s").await;
        assert!(
            store.get("s").unwrap().alarm.is_none(),
            "后台查询期间变忙，不能沿用旧空闲判断"
        );
        controller.on_idle_dispatch_failed("s", "task supervisor capacity full");
        let error = controller.storage_error("s").unwrap();
        assert!(error.contains("空闲检查未能派发"));
        assert!(error.contains("capacity full"));
        assert!(!error.contains("保存失败"));
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Running);
        assert!(store.get("s").unwrap().alarm.is_none());
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn injected_identity_never_rebinds_delayed_registration_or_old_control() {
        let path = test_path("source-identity-frozen");
        let store = Arc::new(load_store(&path, 1_000));
        let target = LoopTarget::desktop("s");
        let (hooks, captured) = identity_capture_hooks();
        let controller = LoopController::new(store.clone(), hooks);
        let alarm = attach_alarm(&controller, &store);
        controller.start(&target, "old").await.unwrap();
        let old_identity = captured.lock().unwrap()[0].clone();
        assert_eq!(old_identity, old_identity.clone());
        assert!(old_identity.is_for_session("s"));
        assert!(!old_identity.is_for_session("other"));
        assert!(
            build_loop_kickoff_message(&store.get("s").unwrap())
                .run_identity()
                .is_none(),
            "消息文本构造器无权签发身份"
        );
        controller
            .validate_loop_identity("s", &old_identity)
            .unwrap();
        assert_eq!(
            controller
                .validate_loop_identity("other", &old_identity)
                .unwrap_err()
                .code,
            "invalid_loop_run_identity"
        );
        let (other_hooks, _) = identity_capture_hooks();
        let other_root = LoopController::new(store.clone(), other_hooks);
        assert_eq!(
            other_root
                .validate_loop_identity("s", &old_identity)
                .unwrap_err()
                .code,
            "invalid_loop_run_identity"
        );
        assert!(other_root
            .register_loop_run("s", "foreign", &old_identity)
            .is_none());
        controller.stop(&target).await.unwrap();
        controller.start(&target, "new").await.unwrap();
        let new_identity = captured.lock().unwrap()[1].clone();
        assert_ne!(old_identity, new_identity);
        assert_eq!(
            controller
                .validate_loop_identity("s", &old_identity)
                .unwrap_err()
                .code,
            "loop_lifecycle_superseded"
        );
        controller
            .validate_loop_identity("s", &new_identity)
            .unwrap();
        // 模拟旧Run的durable started迟到：只能冻结旧proof，不能偷领新pending。
        let old_guard = controller
            .register_loop_run("s", "late-old", &old_identity)
            .unwrap();
        assert!(controller.pending_loop_turn.lock().unwrap().contains("s"));
        assert_eq!(
            controller
                .tool_schedule_for_run("s", "late-old", 60, "old")
                .await
                .unwrap_err()
                .code,
            "loop_lifecycle_superseded"
        );
        assert_eq!(
            controller
                .tool_complete_for_run("s", "late-old", "old")
                .await
                .unwrap_err()
                .code,
            "loop_lifecycle_superseded"
        );
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Running);
        assert_eq!(store.get("s").unwrap().prompt, "new");
        assert!(store.get("s").unwrap().alarm.is_none());
        controller.on_run_end("s", "late-old", true).await;
        drop(old_guard);
        assert_eq!(store.get("s").unwrap().turn_count, 0);
        assert!(controller.storage_error("s").is_none());
        let new_guard = controller
            .register_loop_run("s", "current", &new_identity)
            .unwrap();
        controller
            .tool_schedule_for_run("s", "current", 60, "new")
            .await
            .unwrap();
        assert_eq!(store.get("s").unwrap().alarm.unwrap().reason, "new");
        controller
            .tool_complete_for_run("s", "current", "done")
            .await
            .unwrap();
        // 自己complete使本代结束，真实Run仍可正常给出最终回复并消费观察登记。
        controller.on_run_end("s", "current", false).await;
        drop(new_guard);
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Completed);
        assert_eq!(
            store.get("s").unwrap().completed_summary.as_deref(),
            Some("done")
        );
        assert!(controller.storage_error("s").is_none());
        assert!(controller.main_runs.lock().unwrap().is_empty());
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn scoped_schedule_rechecks_original_run_after_query_and_terminal() {
        for action in ["restart", "finish"] {
            let path = test_path(&format!("control-source-await-{action}"));
            let store = Arc::new(load_store(&path, 1_000));
            let target = LoopTarget::desktop("s");
            let (mut hooks, captured) = identity_capture_hooks();
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            hooks.has_live_background_work = Arc::new({
                let entered = entered.clone();
                let release = release.clone();
                move |_| {
                    let entered = entered.clone();
                    let release = release.clone();
                    Box::pin(async move {
                        entered.notify_one();
                        release.notified().await;
                        false
                    })
                }
            });
            let controller = LoopController::new(store.clone(), hooks);
            let alarm = attach_alarm(&controller, &store);
            controller.start(&target, "old").await.unwrap();
            let identity = captured.lock().unwrap()[0].clone();
            let guard = controller.register_loop_run("s", "old", &identity).unwrap();
            let schedule = tokio::spawn({
                let controller = controller.clone();
                async move {
                    controller
                        .tool_schedule_for_run("s", "old", 60, "late")
                        .await
                }
            });
            tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            if action == "restart" {
                controller.stop(&target).await.unwrap();
                controller.start(&target, "new").await.unwrap();
            } else {
                controller.on_run_end("s", "old", false).await;
            }
            release.notify_waiters();
            let error = tokio::time::timeout(std::time::Duration::from_secs(2), schedule)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert_eq!(error.code, "loop_lifecycle_superseded");
            drop(guard);
            assert!(store.get("s").unwrap().alarm.is_none());
            assert!(controller.storage_error("s").is_none());
            alarm.dispose();
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }

    #[tokio::test]
    async fn controller_start_kickoffs_and_stop_clears_marks() {
        let path = test_path("ctrl-start");
        let store = Arc::new(load_store(&path, 100));
        let host = Arc::new(FakeHost::default());
        let scripted = Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let _alarm = attach_alarm(&controller, &store);

        let target = LoopTarget::desktop("sess-a");
        let record = controller.start(&target, "任务甲").await.expect("start");
        assert_eq!(record.status, LoopStatus::Running);
        // 先推 running、再记 user 气泡、再 kickoff 投递（现役顺序）。
        assert_eq!(
            host.user_prompts.lock().unwrap().as_slice(),
            &["任务甲".to_string()]
        );
        assert_eq!(
            host.deliveries.lock().unwrap().as_slice(),
            &[DeliveryMode::TriggerTurn]
        );
        assert!(host.statuses.lock().unwrap().contains(&LoopStatus::Running));

        // active 排他，文案逐字。
        let err = controller.start(&target, "再来").await.unwrap_err();
        assert_eq!(err.code, "loop_already_active");
        assert_eq!(err.message, "该会话已有活跃循环，先 /loop stop 再启动新的");

        let stopped = controller.stop(&target).await.expect("stop");
        assert_eq!(stopped.status, LoopStatus::Stopped);
        assert!(stopped.paused_reason.is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn controller_pause_resume_and_reset_turns_after_budget() {
        let path = test_path("ctrl-resume");
        let store = Arc::new(load_store(&path, 100));
        let host = Arc::new(FakeHost::default());
        let scripted = Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let _alarm = attach_alarm(&controller, &store);
        let target = LoopTarget::desktop("sess-b");
        controller.start(&target, "任务乙").await.unwrap();

        // 未暂停不能 resume。
        let err = controller.resume(&target).await.unwrap_err();
        assert_eq!(err.code, "loop_not_paused");
        assert_eq!(err.message, "循环当前状态为 running，只有 paused 可 resume");

        controller.pause(&target, "user", None).await.unwrap();
        // 手工把 pausedReason 调成 budget_exhausted 并塞满轮数，验证 resume 复位。
        store
            .update("sess-b", |record| {
                record.paused_reason = Some("budget_exhausted".to_string());
                record.turn_count = 50;
                record.consecutive_failures = 2;
            })
            .unwrap();
        scripted.lock().unwrap().push(DeliveryMode::TriggerTurn);
        let resumed = controller.resume(&target).await.expect("resume");
        assert_eq!(resumed.status, LoopStatus::Running);
        assert_eq!(
            resumed.turn_count, 0,
            "budget_exhausted resume resets the turn budget"
        );
        assert_eq!(resumed.consecutive_failures, 0);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn turn_accounting_and_invariant_fallback_alarm() {
        let path = test_path("ctrl-turns");
        let store = Arc::new(load_store(&path, 100));
        let host = Arc::new(FakeHost::default());
        let scripted = Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let alarm = attach_alarm(&controller, &store);
        let target = LoopTarget::desktop("sess-c");
        controller.start(&target, "任务丙").await.unwrap();

        // kickoff 投递（triggerTurn）留 pending；轮开始时转入 active。
        controller.on_turn_start("sess-c");
        controller.on_turn_end("sess-c", false).await;
        let record = store.get("sess-c").unwrap();
        assert_eq!(record.turn_count, 1, "loop turn counted");
        assert_eq!(record.consecutive_failures, 0);
        // 无闹钟且无后台工作 → 守恒兜底闹钟补上。
        let record = store.get("sess-c").unwrap();
        let slot = record.alarm.expect("fallback alarm armed");
        assert_eq!(slot.reason, FALLBACK_ALARM_REASON);
        assert_eq!(slot.wake_at, 1_000 + 1200 * 1000);
        alarm.dispose();

        // 失败轮：message_end error → 连续失败 +1；resume 后 active 标记不再 → 不计轮。
        scripted.lock().unwrap().push(DeliveryMode::TriggerTurn);
        store
            .update("sess-c", |record| {
                record.alarm = None;
            })
            .unwrap();
        controller.mark_pending_loop_turn_for_test("sess-c");
        controller.on_turn_start("sess-c");
        controller.on_message_end("sess-c", "error");
        // 有兜底闹钟已清（上面手动清空），注入后台工作避免再起兜底。
        controller.on_turn_end("sess-c", false).await;
        let record = store.get("sess-c").unwrap();
        assert_eq!(record.consecutive_failures, 1);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn budget_exhaustion_pauses_with_notice() {
        let path = test_path("ctrl-budget");
        let store = Arc::new(load_store(&path, 100));
        let host = Arc::new(FakeHost::default());
        let scripted = Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let _alarm = attach_alarm(&controller, &store);
        let target = LoopTarget::desktop("sess-d");
        controller.start(&target, "任务丁").await.unwrap();
        store
            .update("sess-d", |record| {
                record.turn_count = 49;
            })
            .unwrap();

        controller.mark_pending_loop_turn_for_test("sess-d");
        controller.on_turn_start("sess-d");
        controller.on_turn_end("sess-d", false).await;
        let record = store.get("sess-d").unwrap();
        assert_eq!(record.status, LoopStatus::Paused);
        assert_eq!(record.paused_reason.as_deref(), Some("budget_exhausted"));
        assert_eq!(
            host.notices.lock().unwrap().as_slice(),
            &["（循环已用完 50 轮预算，自动暂停。/loop resume 可重置并继续）".to_string()]
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn consecutive_failures_pause_with_notice() {
        let path = test_path("ctrl-failures");
        let store = Arc::new(load_store(&path, 100));
        let host = Arc::new(FakeHost::default());
        let scripted = Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let _alarm = attach_alarm(&controller, &store);
        let target = LoopTarget::desktop("sess-e");
        controller.start(&target, "任务戊").await.unwrap();
        store
            .update("sess-e", |record| {
                record.consecutive_failures = 2;
            })
            .unwrap();

        controller.mark_pending_loop_turn_for_test("sess-e");
        controller.on_turn_start("sess-e");
        controller.on_message_end("sess-e", "error");
        controller.on_turn_end("sess-e", false).await;
        let record = store.get("sess-e").unwrap();
        assert_eq!(record.status, LoopStatus::Paused);
        assert_eq!(
            record.paused_reason.as_deref(),
            Some("consecutive_failures")
        );
        assert_eq!(
            host.notices.lock().unwrap().as_slice(),
            &["（循环连续 3 轮失败，自动暂停。排查后可 /loop resume）".to_string()]
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn tool_complete_and_schedule_errors() {
        let path = test_path("ctrl-tools");
        let store = Arc::new(load_store(&path, 100));
        let host = Arc::new(FakeHost::default());
        let scripted = Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let _alarm = attach_alarm(&controller, &store);

        // 无循环：schedule/complete 报 no_active_loop。
        let err = controller.tool_schedule("none", 60, "").await.unwrap_err();
        assert_eq!(err.code, "no_active_loop");
        assert_eq!(err.message, "该会话没有活跃循环");
        let err = controller.tool_complete("none", "").await.unwrap_err();
        assert_eq!(err.code, "no_active_loop");

        let target = LoopTarget::desktop("sess-f");
        controller.start(&target, "任务己").await.unwrap();

        // 短闹钟无后台工作：钳到 minDelaySec=60。
        let outcome = controller
            .tool_schedule("sess-f", 5, "看下队列")
            .await
            .expect("schedule");
        assert_eq!(outcome.effective_delay_sec, 60);
        assert_eq!(outcome.wake_at, 1_000 + 60_000);
        let slot = store.get("sess-f").unwrap().alarm.unwrap();
        assert_eq!(slot.reason, "看下队列");

        let completed = controller
            .tool_complete("sess-f", "全部绿")
            .await
            .expect("complete");
        assert_eq!(completed.status, LoopStatus::Completed);
        assert_eq!(completed.completed_summary.as_deref(), Some("全部绿"));
        assert!(
            store.get("sess-f").unwrap().alarm.is_none(),
            "complete cancels the alarm slot"
        );
        // completed 后 schedule 报 loop_not_running。
        let err = controller
            .tool_schedule("sess-f", 60, "")
            .await
            .unwrap_err();
        assert_eq!(err.code, "loop_not_running");
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn alarm_rejects_short_alarm_with_live_work() {
        let path = test_path("alarm-live");
        let store = Arc::new(load_store(&path, 100));
        let host = Arc::new(FakeHost::default());
        host.live_work
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let scripted = Arc::new(Mutex::new(vec![DeliveryMode::TriggerTurn]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let _alarm = attach_alarm(&controller, &store);
        let target = LoopTarget::desktop("sess-g");
        controller.start(&target, "任务庚").await.unwrap();

        let err = controller
            .tool_schedule("sess-g", 300, "poll")
            .await
            .unwrap_err();
        assert_eq!(err.code, "short_alarm_with_live_work");
        assert_eq!(
            err.message,
            "Background work in this session wakes it automatically on completion; do not schedule alarms to poll it. While background work is live, alarms must be long fallbacks (>= 1200s)."
        );
        // 长兜底允许。
        let ok = controller
            .tool_schedule("sess-g", 1200, "fallback")
            .await
            .expect("long alarm ok");
        assert_eq!(ok.effective_delay_sec, 1200);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn alarm_fires_clears_slot_and_delivers() {
        let path = test_path("alarm-fire");
        let store = Arc::new(load_store(&path, 100));
        let host = Arc::new(FakeHost::default());
        let scripted = Arc::new(Mutex::new(vec![
            DeliveryMode::TriggerTurn, // kickoff
            DeliveryMode::TriggerTurn, // wakeup
        ]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let alarm = attach_alarm(&controller, &store);
        let target = LoopTarget::desktop("sess-h");
        controller.start(&target, "任务辛").await.unwrap();
        controller.on_turn_start("sess-h");

        // 手工武装一个立即到期的闹钟（绕开 minDelay 钳制直接调 store+arm）。
        store
            .update("sess-h", |record| {
                record.alarm = Some(LoopAlarmSlot {
                    wake_at: 1,
                    reason: "到点了".to_string(),
                });
            })
            .unwrap();
        alarm.arm("sess-h", 1);
        // 等闹钟触发（真实 tokio 定时器，1ms 内到期）。
        for _ in 0..200 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            if host.deliveries.lock().unwrap().len() >= 2 {
                break;
            }
        }
        assert_eq!(host.deliveries.lock().unwrap().len(), 2, "wakeup delivered");
        assert!(
            store.get("sess-h").unwrap().alarm.is_none(),
            "slot cleared before delivery"
        );
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn alarm_retry_exhaustion_pauses_loop() {
        let path = test_path("alarm-exhaust");
        let store = Arc::new(load_store(&path, 100));
        let host = Arc::new(FakeHost::default());
        let scripted = Arc::new(Mutex::new(vec![
            DeliveryMode::TriggerTurn, // kickoff
            DeliveryMode::Suppressed,
            DeliveryMode::Suppressed,
            DeliveryMode::Suppressed,
            DeliveryMode::Suppressed, // 1 + 3 次重试全 suppressed
        ]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let alarm = attach_alarm(&controller, &store);
        let target = LoopTarget::desktop("sess-i");
        controller.start(&target, "任务壬").await.unwrap();
        controller.on_turn_start("sess-i");
        store
            .update("sess-i", |record| {
                record.alarm = Some(LoopAlarmSlot {
                    wake_at: 1,
                    reason: "r".to_string(),
                });
            })
            .unwrap();
        alarm.arm("sess-i", 1);
        // 重试间隔 1ms（tiny_timings），等耗尽钩子暂停循环。
        for _ in 0..400 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            if store.get("sess-i").unwrap().status == LoopStatus::Paused {
                break;
            }
        }
        let record = store.get("sess-i").unwrap();
        assert_eq!(record.status, LoopStatus::Paused);
        assert_eq!(
            record.paused_reason.as_deref(),
            Some("wakeup_delivery_failed")
        );
        assert!(
            host.notices
                .lock()
                .unwrap()
                .iter()
                .any(|text| text.contains("循环唤醒投递失败已重试耗尽")),
            "exhaustion notice recorded: {:?}",
            host.notices.lock().unwrap()
        );
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn alarm_busy_defers_and_restores_slot() {
        let path = test_path("alarm-busy");
        let store = Arc::new(load_store(&path, 100));
        let host = Arc::new(FakeHost::default());
        let scripted = Arc::new(Mutex::new(vec![
            DeliveryMode::TriggerTurn, // kickoff
            DeliveryMode::Busy,
            DeliveryMode::TriggerTurn, // 顺延后的唤醒
        ]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let alarm = attach_alarm(&controller, &store);
        let target = LoopTarget::desktop("sess-j");
        controller.start(&target, "任务癸").await.unwrap();
        controller.on_turn_start("sess-j");
        store
            .update("sess-j", |record| {
                record.alarm = Some(LoopAlarmSlot {
                    wake_at: 1,
                    reason: "r".to_string(),
                });
            })
            .unwrap();
        alarm.arm("sess-j", 1);
        // busy 顺延 1ms（tiny），之后唤醒成功。
        for _ in 0..400 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            if host.deliveries.lock().unwrap().len() >= 3 {
                break;
            }
        }
        assert_eq!(host.deliveries.lock().unwrap().len(), 3);
        // busy 顺延写回过槽（守恒检查依赖），最终唤醒清槽由 fire 完成。
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[tokio::test]
    async fn recover_at_boot_stops_unrunnable_and_arms_fallback() {
        let path = test_path("boot");
        let store = Arc::new(load_store(&path, 100));
        store
            .create(&LoopTarget::desktop("dead"), "d", None)
            .unwrap();
        store
            .create(&LoopTarget::desktop("alive"), "a", None)
            .unwrap();
        let host = Arc::new(FakeHost::default());
        host.runnable
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let scripted = Arc::new(Mutex::new(vec![]));
        let controller = LoopController::new(
            Arc::clone(&store),
            hooks_for(Arc::clone(&host), Arc::clone(&scripted)),
        );
        let alarm = attach_alarm(&controller, &store);
        controller.recover_at_boot().await;
        assert_eq!(store.get("dead").unwrap().status, LoopStatus::Stopped);
        assert_eq!(
            store.get("dead").unwrap().paused_reason.as_deref(),
            Some("target_unavailable")
        );

        // 可投递但无闹钟 → 兜底闹钟（BOOT_RECOVERY_REASON）。
        host.runnable
            .store(true, std::sync::atomic::Ordering::SeqCst);
        store
            .update("alive", |record| {
                record.status = LoopStatus::Running;
                record.paused_reason = None;
            })
            .unwrap();
        controller.recover_at_boot().await;
        let record = store.get("alive").unwrap();
        assert_eq!(record.status, LoopStatus::Running);
        let slot = record.alarm.expect("boot fallback alarm");
        assert_eq!(slot.reason, BOOT_RECOVERY_REASON);
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
    #[tokio::test]
    async fn cancel_stops_delivery_retry_instead_of_only_the_timer() {
        let path = test_path("cancel-retry");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(
                &LoopTarget::desktop("s"),
                "watch",
                Some(LoopLimits {
                    min_delay_sec: 0,
                    ..LOOP_DEFAULT_LIMITS
                }),
            )
            .unwrap();
        let calls = Arc::new(AtomicU64::new(0));
        let attempted = Arc::new(tokio::sync::Notify::new());
        let alarm = LoopAlarmService::new(
            store.clone(),
            Arc::new({
                let calls = calls.clone();
                let attempted = attempted.clone();
                move |_, _| {
                    let calls = calls.clone();
                    let attempted = attempted.clone();
                    Box::pin(async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        attempted.notify_one();
                        Delivery::mode(DeliveryMode::Suppressed)
                    })
                }
            }),
            Arc::new(|_, _, _| panic!("取消后不得耗尽重试")),
            LoopTimings {
                delivery_retry_delays_ms: vec![100, 100],
                ..tiny_timings()
            },
            Box::new(|| 1_000),
        );
        alarm.schedule("s", 0, "check", false).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), attempted.notified())
            .await
            .unwrap();
        alarm.cancel("s");
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(alarm.armed_keys().is_empty());
        assert!(store.get("s").unwrap().alarm.is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn dispose_aborts_live_delivery_and_refuses_new_alarm() {
        struct Dropped(Arc<AtomicU64>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let path = test_path("dispose-live");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(
                &LoopTarget::desktop("s"),
                "watch",
                Some(LoopLimits {
                    min_delay_sec: 0,
                    ..LOOP_DEFAULT_LIMITS
                }),
            )
            .unwrap();
        let started = Arc::new(tokio::sync::Notify::new());
        let dropped = Arc::new(AtomicU64::new(0));
        let alarm = LoopAlarmService::new(
            store.clone(),
            Arc::new({
                let started = started.clone();
                let dropped = dropped.clone();
                move |_, _| {
                    let started = started.clone();
                    let dropped = dropped.clone();
                    Box::pin(async move {
                        let _guard = Dropped(dropped);
                        started.notify_one();
                        std::future::pending::<Delivery>().await
                    })
                }
            }),
            Arc::new(|_, _, _| {}),
            tiny_timings(),
            Box::new(|| 1_000),
        );
        alarm.schedule("s", 0, "check", false).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), started.notified())
            .await
            .unwrap();
        alarm.dispose();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while dropped.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(alarm.armed_keys().is_empty());
        assert_eq!(
            alarm.schedule("s", 0, "again", false).unwrap_err().code,
            "loop_alarm_stopped"
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn huge_alarm_delay_does_not_overflow_or_leave_an_unowned_timer() {
        let path = test_path("huge-alarm");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        let alarm = LoopAlarmService::new(
            store,
            Arc::new(|_, _| Box::pin(async { panic!("远期闹钟不能立即触发") })),
            Arc::new(|_, _, _| {}),
            tiny_timings(),
            Box::new(|| 1_000),
        );
        assert_eq!(
            alarm.schedule("s", u64::MAX, "far", false).unwrap().wake_at,
            u64::MAX
        );
        tokio::task::yield_now().await;
        alarm.dispose();
        assert!(alarm.armed_keys().is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn state_write_failure_does_not_commit_in_memory_or_claim_success() {
        let path = test_path("state-write-failure");
        let store = load_store(&path, 1_000);
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            store.update("s", |r| r.turn_count = 9).unwrap_err().code,
            "loop_state_write_failed"
        );
        assert_eq!(store.get("s").unwrap().turn_count, 0);
        assert!(store
            .create(&LoopTarget::desktop("other"), "watch", None)
            .is_err());
        assert!(store.get("other").is_none());
        assert!(store.remove("s").is_err());
        assert!(store.get("s").is_some());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
    #[tokio::test]
    async fn alarm_storage_failure_never_delivers_or_arms_unrecorded_work() {
        let path = test_path("alarm-storage-failure");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        store
            .update("s", |record| {
                record.alarm = Some(LoopAlarmSlot {
                    wake_at: 1_000,
                    reason: "retry".into(),
                })
            })
            .unwrap();
        let deliveries = Arc::new(AtomicU64::new(0));
        let failures = Arc::new(AtomicU64::new(0));
        let alarm = LoopAlarmService::new(
            store,
            Arc::new({
                let deliveries = deliveries.clone();
                move |_, _| {
                    let deliveries = deliveries.clone();
                    Box::pin(async move {
                        deliveries.fetch_add(1, Ordering::SeqCst);
                        Delivery::mode(DeliveryMode::TriggerTurn)
                    })
                }
            }),
            Arc::new({
                let failures = failures.clone();
                move |_, _, _| {
                    failures.fetch_add(1, Ordering::SeqCst);
                }
            }),
            tiny_timings(),
            Box::new(|| 1_000),
        );
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        alarm.fire("s").await;
        alarm.defer_busy("s", "busy");
        assert_eq!(deliveries.load(Ordering::SeqCst), 0);
        assert_eq!(failures.load(Ordering::SeqCst), 2);
        assert!(alarm.armed_keys().is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn turn_accounting_write_failure_blocks_automatic_work_until_explicit_resume() {
        let path = test_path("turn-write-block");
        let store = Arc::new(load_store(&path, 1_000));
        let host = Arc::new(FakeHost::default());
        let scripted = Arc::new(Mutex::new(vec![
            DeliveryMode::TriggerTurn,
            DeliveryMode::TriggerTurn,
        ]));
        let controller = LoopController::new(store.clone(), hooks_for(host.clone(), scripted));
        let alarm = attach_alarm(&controller, &store);
        let target = LoopTarget::desktop("s");
        controller.start(&target, "watch").await.unwrap();
        controller.on_turn_start("s");
        let statuses_before = host.statuses.lock().unwrap().len();
        // 真正制造写前失败；内存保留最后已保存状态，不假报 paused 或已计数。
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        controller.on_turn_end("s", false).await;
        assert!(controller.storage_blocked("s"));
        assert!(controller
            .storage_error("s")
            .unwrap()
            .contains("循环状态保存失败"));
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Running);
        assert_eq!(store.get("s").unwrap().turn_count, 0);
        assert_eq!(host.statuses.lock().unwrap().len(), statuses_before);
        assert!(alarm.armed_keys().is_empty());
        assert_eq!(
            controller.deliver_wakeup_turn("s", "retry").await.mode,
            DeliveryMode::Stopped
        );
        assert_eq!(
            controller
                .tool_schedule("s", 60, "retry")
                .await
                .unwrap_err()
                .code,
            "loop_state_unconfirmed"
        );
        // 存储恢复本身不会解除阻断，也不会偷跑；仅明确 resume 可重新保存并推进。
        std::fs::remove_dir(&path).unwrap();
        controller.on_turn_end("s", false).await;
        assert!(controller.storage_blocked("s"));
        assert_eq!(host.deliveries.lock().unwrap().len(), 1);
        controller.resume(&target).await.unwrap();
        assert!(!controller.storage_blocked("s"));
        assert!(controller.storage_error("s").is_none());
        assert_eq!(host.deliveries.lock().unwrap().len(), 2);
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn unavailable_target_with_unsaved_boot_stop_never_rehydrates_old_alarm() {
        let path = test_path("boot-stop-write-block");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        store
            .update("s", |record| {
                record.alarm = Some(LoopAlarmSlot {
                    wake_at: 2_000,
                    reason: "old".into(),
                })
            })
            .unwrap();
        let host = Arc::new(FakeHost::default());
        host.runnable.store(false, Ordering::SeqCst);
        let controller = LoopController::new(
            store.clone(),
            hooks_for(host.clone(), Arc::new(Mutex::new(vec![]))),
        );
        let alarm = attach_alarm(&controller, &store);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        controller.recover_at_boot().await;
        assert!(controller.storage_blocked("s"));
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Running);
        assert!(alarm.armed_keys().is_empty());
        assert!(host.deliveries.lock().unwrap().is_empty());
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn schedule_registration_and_stop_share_one_mutation_boundary() {
        let path = test_path("schedule-stop-barrier");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        let host = Arc::new(FakeHost::default());
        let controller = LoopController::new(
            store.clone(),
            hooks_for(host.clone(), Arc::new(Mutex::new(vec![]))),
        );
        let alarm = attach_alarm(&controller, &store);
        let release = Arc::new(std::sync::Barrier::new(2));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        *alarm.mutation_hook.lock().unwrap() = Some(Arc::new({
            let release = release.clone();
            move |step| {
                if step == "schedule_persisted" {
                    entered_tx.send(()).unwrap();
                    release.wait();
                }
            }
        }));
        let runtime = tokio::runtime::Handle::current();
        let schedule_thread = std::thread::spawn({
            let alarm = alarm.clone();
            let runtime = runtime.clone();
            move || {
                let _runtime = runtime.enter();
                alarm.schedule("s", 60, "pending registration", false)
            }
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let (stop_started_tx, stop_started_rx) = std::sync::mpsc::channel();
        let (stop_done_tx, stop_done_rx) = std::sync::mpsc::channel();
        let stop_thread = std::thread::spawn({
            let controller = controller.clone();
            move || {
                stop_started_tx.send(()).unwrap();
                let result = runtime.block_on(controller.stop(&LoopTarget::desktop("s")));
                stop_done_tx.send(()).unwrap();
                result
            }
        });
        stop_started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        // 在落盘与登记之间停住：停止必须等整个改约完成，随后一并回收。
        let stopped_too_early = stop_done_rx
            .recv_timeout(std::time::Duration::from_millis(30))
            .is_ok();
        release.wait();
        schedule_thread.join().unwrap().unwrap();
        stop_thread.join().unwrap().unwrap();
        assert!(!stopped_too_early);
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Stopped);
        assert!(store.get("s").unwrap().alarm.is_none());
        assert!(alarm.armed_keys().is_empty());
        assert_eq!(
            alarm
                .schedule("s", 60, "after stop", false)
                .unwrap_err()
                .code,
            "loop_not_running"
        );
        assert!(host.deliveries.lock().unwrap().is_empty());
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fire_claim_cannot_clear_a_concurrently_replaced_slot() {
        let path = test_path("fire-schedule-barrier");
        let store = Arc::new(load_store(&path, 1_000));
        let limits = LoopLimits {
            min_delay_sec: 0,
            ..LOOP_DEFAULT_LIMITS
        };
        store
            .create(&LoopTarget::desktop("s"), "watch", Some(limits))
            .unwrap();
        let alarm = LoopAlarmService::new(
            store.clone(),
            Arc::new(|_, _| Box::pin(std::future::pending::<Delivery>())),
            Arc::new(|_, _, _| panic!("替换不能耗尽旧闹钟投递")),
            tiny_timings(),
            Box::new(|| 1_000),
        );
        let release = Arc::new(std::sync::Barrier::new(2));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        *alarm.mutation_hook.lock().unwrap() = Some(Arc::new({
            let release = release.clone();
            move |step| {
                if step == "fire_read_slot" {
                    entered_tx.send(()).unwrap();
                    release.wait();
                }
            }
        }));
        alarm.schedule("s", 0, "old", false).unwrap();
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let (replace_started_tx, replace_started_rx) = std::sync::mpsc::channel();
        let (replace_done_tx, replace_done_rx) = std::sync::mpsc::channel();
        let replacement = std::thread::spawn({
            let alarm = alarm.clone();
            let runtime = tokio::runtime::Handle::current();
            move || {
                let _runtime = runtime.enter();
                replace_started_tx.send(()).unwrap();
                let result = alarm.schedule("s", 60, "new", false);
                replace_done_tx.send(()).unwrap();
                result
            }
        });
        replace_started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        // 在读旧槽与清槽之间停住：新约不能先落盘再被旧任务抹掉。
        let replaced_too_early = replace_done_rx
            .recv_timeout(std::time::Duration::from_millis(30))
            .is_ok();
        release.wait();
        replacement.join().unwrap().unwrap();
        assert!(!replaced_too_early);
        assert_eq!(store.get("s").unwrap().alarm.unwrap().reason, "new");
        assert_eq!(alarm.armed_keys(), vec!["s".to_string()]);
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn replaced_timer_identity_cannot_claim_or_defer_the_new_slot() {
        let path = test_path("replaced-timer-identity");
        let store = Arc::new(load_store(&path, 1_000));
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        let alarm = LoopAlarmService::new(
            store.clone(),
            Arc::new(|_, _| Box::pin(async { panic!("旧计时任务不得投递") })),
            Arc::new(|_, _, _| panic!("旧计时任务不得暂停新约")),
            tiny_timings(),
            Box::new(|| 1_000),
        );
        alarm.schedule("s", 60, "old", false).unwrap();
        let old_id = alarm.timers.lock().unwrap().get("s").unwrap().id();
        alarm.schedule("s", 120, "new", false).unwrap();
        let new_id = alarm.timers.lock().unwrap().get("s").unwrap().id();
        assert_ne!(old_id, new_id);
        alarm.fire_owned("s", Some(old_id)).await;
        alarm.defer_busy_owned("s", "old", Some(old_id));
        assert_eq!(store.get("s").unwrap().alarm.unwrap().reason, "new");
        assert_eq!(alarm.timers.lock().unwrap().get("s").unwrap().id(), new_id);
        assert!(alarm.busy_deferrals.lock().unwrap().is_empty());
        alarm.cancel("s");
        alarm.fire_owned("s", Some(new_id)).await;
        assert!(store.get("s").unwrap().alarm.is_none());
        assert!(alarm.armed_keys().is_empty());
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn late_exhaustion_cannot_pause_a_new_schedule_or_restarted_loop() {
        let path = test_path("late-exhaustion-generation");
        let store = Arc::new(load_store(&path, 1_000));
        let limits = LoopLimits {
            min_delay_sec: 0,
            ..LOOP_DEFAULT_LIMITS
        };
        let target = LoopTarget::desktop("s");
        store.create(&target, "watch", Some(limits)).unwrap();
        let host = Arc::new(FakeHost::default());
        let controller = LoopController::new(
            store.clone(),
            hooks_for(
                host.clone(),
                Arc::new(Mutex::new(vec![
                    DeliveryMode::TriggerTurn,
                    DeliveryMode::TriggerTurn,
                ])),
            ),
        );
        let (exhausted_tx, mut exhausted_rx) = tokio::sync::mpsc::unbounded_channel();
        let alarm = LoopAlarmService::new(
            store.clone(),
            Arc::new(|_, _| Box::pin(async { Delivery::mode(DeliveryMode::Suppressed) })),
            Arc::new(move |key, error, generation| {
                exhausted_tx.send((key, error, generation)).unwrap();
            }),
            LoopTimings {
                delivery_retry_delays_ms: vec![],
                ..tiny_timings()
            },
            Box::new(|| 1_000),
        );
        controller.attach_alarm(alarm.clone());
        alarm.schedule("s", 0, "first", false).unwrap();
        let (key, error, first_generation) =
            tokio::time::timeout(std::time::Duration::from_secs(2), exhausted_rx.recv())
                .await
                .unwrap()
                .unwrap();
        assert!(first_generation.is_some());
        assert!(
            alarm.armed_keys().is_empty(),
            "耗尽计时任务已经结束，领取代号仍须可核对"
        );
        alarm.schedule("s", 60, "new schedule", false).unwrap();
        let statuses_before = host.statuses.lock().unwrap().len();
        // 将耗尽回调精确停留到新约保存之后，再执行异步暂停的实际处理体。
        assert!(controller
            .pause_inner_generation(
                &key,
                "wakeup_delivery_failed",
                Some(error.clone()),
                first_generation
            )
            .await
            .unwrap()
            .is_none());
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Running);
        assert_eq!(
            store.get("s").unwrap().alarm.unwrap().reason,
            "new schedule"
        );
        assert_eq!(host.statuses.lock().unwrap().len(), statuses_before);
        assert!(host.notices.lock().unwrap().is_empty());

        alarm.schedule("s", 0, "before resume", false).unwrap();
        let (_, _, before_resume_generation) =
            tokio::time::timeout(std::time::Duration::from_secs(2), exhausted_rx.recv())
                .await
                .unwrap()
                .unwrap();
        controller.pause(&target, "user", None).await.unwrap();
        controller.resume(&target).await.unwrap();
        assert!(controller
            .pause_inner_generation(
                "s",
                "wakeup_delivery_failed",
                Some(error.clone()),
                before_resume_generation
            )
            .await
            .unwrap()
            .is_none());
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Running);
        controller.stop(&target).await.unwrap();
        controller.start(&target, "new loop").await.unwrap();
        assert!(controller
            .pause_inner_generation(
                "s",
                "wakeup_delivery_failed",
                Some(error.clone()),
                before_resume_generation
            )
            .await
            .unwrap()
            .is_none());
        assert_eq!(store.get("s").unwrap().prompt, "new loop");
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Running);
        assert!(host.notices.lock().unwrap().is_empty());

        // 新循环当前代号的真实耗尽仍然暂停并通知，不因计时任务退出而漏处理。
        store
            .update("s", |record| record.limits.min_delay_sec = 0)
            .unwrap();
        alarm.schedule("s", 0, "current", false).unwrap();
        let (_, _, current_generation) =
            tokio::time::timeout(std::time::Duration::from_secs(2), exhausted_rx.recv())
                .await
                .unwrap()
                .unwrap();
        assert!(alarm.armed_keys().is_empty());
        assert!(controller
            .pause_inner_generation(
                "s",
                "wakeup_delivery_failed",
                Some(error),
                current_generation
            )
            .await
            .unwrap()
            .is_some());
        assert_eq!(store.get("s").unwrap().status, LoopStatus::Paused);
        assert_eq!(host.notices.lock().unwrap().len(), 1);
        alarm.dispose();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn valid_json_with_bad_shape_version_or_records_is_rejected_without_touching_original() {
        let path = test_path("load-shape-rejected");
        let store = load_store(&path, 1_000);
        store
            .create(&LoopTarget::desktop("s"), "watch", None)
            .unwrap();
        let valid: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let record = valid["loops"][0].clone();
        let mut missing_field = record.clone();
        missing_field.as_object_mut().unwrap().remove("status");
        let mut wrong_target = record.clone();
        wrong_target["target"]["sessionId"] = json!("other");
        let mut empty_key = record.clone();
        empty_key["key"] = json!("");
        let cases = vec![
            json!({}),
            json!([]),
            json!({"schemaVersion":1}),
            json!({"schemaVersion":1,"loops":null}),
            json!({"schemaVersion":1,"loops":{}}),
            json!({"schemaVersion":2,"loops":[]}),
            json!({"schemaVersion":1,"loops":[record.clone(),record]}),
            json!({"schemaVersion":1,"loops":[missing_field]}),
            json!({"schemaVersion":1,"loops":[wrong_target]}),
            json!({"schemaVersion":1,"loops":[empty_key]}),
        ];
        for data in cases {
            let original = serde_json::to_vec(&data).unwrap();
            std::fs::write(&path, &original).unwrap();
            let result = LoopStore::load(path.clone(), Box::new(|| 1_000));
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("损坏形状不能作为空循环接受: {data}"),
            };
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
            assert!(error.to_string().contains("original retained"));
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
        std::fs::write(&path, serde_json::to_vec(&valid).unwrap()).unwrap();
        let restored = load_store(&path, 1_000);
        assert_eq!(restored.get("s").unwrap().prompt, "watch");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
