//! R06-T02：token 预算、压缩规划与摘要校验——纯确定性 kernel 半区
//! （无 IO、无时钟、无网络；同输入同输出）。
//!
//! 本模块是现役压缩语义的 Rust 迁移，逐条锚定来源：
//!
//! - **触发阈值**（`core/session-compaction-runtime.ts`）：FORCE 线 = 窗口
//!   的 80%；动态 reserve = `max(16384, ceil(window × 20%))`；触发条件 =
//!   `ratio >= 0.8 || contextTokens > window - reserve`；contextTokens =
//!   最近一次模型调用的真实 usage 总量（现役 `calculateContextTokens` =
//!   input+output+cacheRead+cacheWrite，其中现役 input 是**扣过 cache
//!   分量**的口径——对 R05 统一结构须按族 inclusion 旗标先还原 input 半量
//!   再相加，included 分量绝不重复加计，R05 RR1 F23 消费者纪律）+ 该调用
//!   之后新进上下文的工具结果估算。窗口未声明或 usage 不可得时**不触发**
//!   （现役 `return false`）。
//! - **切点规划**（pi-agent-core `findCutPoint` +
//!   `core/session-compactor.ts` `completeToolTransactionTrimBoundaries`）：
//!   从尾向前累积估算直到 keepRecentTokens（现役默认 20000），切点只落在
//!   assistant 组边界，永不落在 toolResult；工具调用/结果配对必须可证明
//!   （缺失归属/重复结果 = unprovable，响亮拒绝）；未闭合（pending）工具
//!   调用所在组必须整体保留——否则其未来结果将成为孤立 tool result
//!   （R06-A03）。
//! - **摘要身份**（pi-agent-core `messages.js` `convertToLlm`）：压缩摘要
//!   以普通 user 角色消息进入历史（前缀/后缀包装），绝不获得系统指令
//!   优先级。
//! - **清洗与校验**
//!   （`lib/llm/cache-preserving-compaction-agent-run.ts` 的
//!   `sanitizeSummary`/`validateSummary`）：剥离 mood/pulse/reflect 闭合
//!   块（XML 与围栏两种形态），未闭合残留标签是校验问题而非静默保留；
//!   摘要必须按序携带恰好 9 个结构化标题。
//!
//! token 估算口径（任务书步骤②"明确保守估算并标来源"）：复用
//! [`crate::context::estimate_text_tokens`]——与现役 `estimateTokens`
//! 同源的 chars/4 启发式，叠加 T01 的 CJK×1.1 加权（对中文更保守、
//! 更准确）。锁文件冻结，无真实 tokenizer 可用。

use std::collections::HashMap;
use std::fmt::Write as _;

use lingxi_protocol::ContentBlock;

use crate::context::estimate_text_tokens;
use crate::model_exchange::ExchangeItem;
use crate::usage::ModelCallUsage;

// ─── 冻结常量（现役值） ───

/// ASK 线（`shared/compaction-thresholds.ts` `COMPACTION_ASK_RATIO`）。
/// 以基点（万分之一）表示避免浮点。Rust 无头服务没有可询问的渲染端——
/// ASK 语义在 R06-T02 不实现（现役它只发事件、不阻塞）；该常量保留为
/// 观测报告的分档锚点。
pub const COMPACTION_ASK_RATIO_BP: u64 = 5_000;
/// FORCE 线（`COMPACTION_FORCE_RATIO = 0.8`）：到达即无条件压缩。
pub const COMPACTION_FORCE_RATIO_BP: u64 = 8_000;
/// reserve 绝对地板（`MIN_COMPACTION_RESERVE_TOKENS`）。
pub const MIN_COMPACTION_RESERVE_TOKENS: u64 = 16_384;
/// 保留尾部规模（现役 `DEFAULT_COMPACTION_SETTINGS.keepRecentTokens`）。
pub const KEEP_RECENT_TOKENS: u64 = 20_000;

/// 摘要消息的现役前缀（`COMPACTION_SUMMARY_PREFIX`，逐字）。
pub const COMPACTION_SUMMARY_PREFIX: &str =
    "The conversation history before this point was compacted into the following summary:\n\n<summary>\n";
/// 摘要消息的现役后缀（`COMPACTION_SUMMARY_SUFFIX`，逐字）。
pub const COMPACTION_SUMMARY_SUFFIX: &str = "\n</summary>";

/// 现役 mid-run 压缩通知（`MIDRUN_COMPACTION_NOTICE`，逐字）。措辞让模型
/// 把它当作机器记账而非用户指令。
pub const MIDRUN_COMPACTION_NOTICE: &str = "[System compaction notice — not a user message]\nThe conversation history above was compacted while you were actively working on the user's task. You are still mid-task. Continue the work described in the summary's \"In Progress\" and \"Next Steps\" sections without pausing to ask for confirmation, and do not redo work already listed as done. If any newer user message appears after this notice, it takes precedence over this notice.";

/// 摘要的 9 个结构化标题（`SUMMARY_HEADING_SEQUENCE`，逐字逐序）。
pub const SUMMARY_HEADING_SEQUENCE: [&str; 9] = [
    "## Goal",
    "## Constraints & Preferences",
    "## Progress",
    "### Done",
    "### In Progress",
    "### Blocked",
    "## Key Decisions",
    "## Next Steps",
    "## Critical Context",
];

/// 内部叙事标签词汇（`INTERNAL_NARRATION_TYPES`）。
pub const INTERNAL_NARRATION_TAGS: [&str; 3] = ["mood", "pulse", "reflect"];

/// 现役 placeholder 工具的应答文本（`cache-preserving-compaction-agent-run.ts`
/// `clonePlaceholderTools`，逐字）：摘要模型的首次单工具意图得到这条占位
/// 结果（工具从不真正执行），随后必须输出结构化摘要。
pub const PLACEHOLDER_TOOL_RESULT_TEXT: &str = "Tool intent was preserved for protocol continuity. No live tool was executed. Continue by returning the structured compaction summary without tools.";

/// 摘要项渲染为线上 user 消息的正文（现役 convertToLlm 的包装，逐字）。
pub fn render_summary_message_text(summary: &str) -> String {
    format!("{COMPACTION_SUMMARY_PREFIX}{summary}{COMPACTION_SUMMARY_SUFFIX}")
}

// ─── reserve / usage / 估算 ───

/// 现役 `computeCompactionReserveTokens`：窗口的 20% 与绝对地板取大。
pub fn compute_reserve_tokens(context_window: u64) -> u64 {
    // ceil(window × 0.2) = ceil(window / 5)，整数上取整。
    let proportional = context_window.div_ceil(5);
    proportional.max(MIN_COMPACTION_RESERVE_TOKENS)
}

/// 族级 inclusion 口径（R05 `USAGE_MAPPINGS` 的
/// `cache_read_included_in_input` / `cache_write_included_in_input` 旗标：
/// `Some(true)` → true；`Some(false)` 或 `None`（无该分量字段）→ false）。
///
/// 消费者纪律（R05 RR1 F23）：旗标说 included 的分量**绝不重复加计**。
/// R05 统一结构的 `input_tokens` 是 wire 总量——OpenAI×3/Google 族的
/// wire input 已含 cache_read（included=true）；Anthropic 族的 cache
/// 分量是独立类别（included=false）。判定侧不知道族口径就会把
/// OpenAI 族的 cache_read 双计（input 一次、分量又一次）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheInclusion {
    /// wire input 总量已含 cache_read 分量（OpenAI×3 / Google 族）。
    pub cache_read_in_input: bool,
    /// wire input 总量已含 cache_write 分量（现役五族均为 false）。
    pub cache_write_in_input: bool,
}

/// 现役 `calculateContextTokens`：`input + output + cacheRead + cacheWrite`。
/// 注意两边 **input 分量口径不同**：现役 pi-ai 规范化的 input 已扣除
/// cache 分量（`input = prompt_tokens − cacheRead − cacheWrite`），而 R05
/// 统一结构的 `input_tokens` 是 wire 总量（included 族含 cache）。因此先按
/// 族 inclusion 旗标把 input 还原为现役口径，再四分量相加——两种写法算术
/// 等价（included 分量扣了再加回），但对「input 缺失而分量在场」的半截
/// usage 不虚构扣除。全部分量缺失 = None（"没有可用 usage 事实"，绝非零）。
pub fn context_tokens_from_usage(usage: &ModelCallUsage, inclusion: CacheInclusion) -> Option<u64> {
    let mut total: u64 = 0;
    let mut any = false;
    if let Some(input) = usage.input_tokens {
        // 还原现役的 cache-exclusive input 半量；分量缺失（None）无可扣。
        let mut effective = input;
        if inclusion.cache_read_in_input {
            if let Some(cache) = usage.cache_read_tokens {
                effective = effective.saturating_sub(cache);
            }
        }
        if inclusion.cache_write_in_input {
            if let Some(cache) = usage.cache_write_tokens {
                effective = effective.saturating_sub(cache);
            }
        }
        total = total.saturating_add(effective);
        any = true;
    }
    for value in [
        usage.output_tokens,
        usage.cache_read_tokens,
        usage.cache_write_tokens,
    ]
    .into_iter()
    .flatten()
    {
        total = total.saturating_add(value);
        any = true;
    }
    any.then_some(total)
}

/// 估算单个交换项的 token 数（估算口径见模块头）。与现役
/// `estimateTokens` 对齐：assistant = 文本+思考+调用(名+参数JSON)；
/// toolResult = 内容字符；摘要项 = 摘要正文。
pub fn estimate_exchange_item_tokens(item: &ExchangeItem) -> u64 {
    match item {
        ExchangeItem::AssistantTurn {
            content,
            tool_calls,
            ..
        } => {
            let mut tokens: u64 = 0;
            for block in content {
                tokens = tokens.saturating_add(match block {
                    ContentBlock::Text { text } | ContentBlock::Reasoning { text } => {
                        u64::from(estimate_text_tokens(text))
                    }
                    // 线上渲染的占位文本（openai 族 `[resource: uri]`）。
                    ContentBlock::ResourceRef { resource } => {
                        u64::from(estimate_text_tokens(
                            &resource.uri.clone().unwrap_or_default(),
                        )) + 3
                    }
                    ContentBlock::Opaque { data, .. } => {
                        u64::from(estimate_text_tokens(&data.to_string()))
                    }
                });
            }
            for call in tool_calls {
                // 现役：name.length + JSON.stringify(arguments).length。
                tokens = tokens
                    .saturating_add(u64::from(estimate_text_tokens(&call.target)))
                    .saturating_add(u64::from(estimate_text_tokens(
                        &call.arguments.as_value().to_string(),
                    )));
            }
            tokens
        }
        ExchangeItem::ToolResult { outcome, .. } => estimate_outcome_tokens(outcome),
        ExchangeItem::CompactionSummary {
            summary, mid_run, ..
        } => {
            let mut tokens = u64::from(estimate_text_tokens(summary))
                + u64::from(estimate_text_tokens(COMPACTION_SUMMARY_PREFIX))
                + u64::from(estimate_text_tokens(COMPACTION_SUMMARY_SUFFIX));
            if *mid_run {
                tokens += u64::from(estimate_text_tokens(MIDRUN_COMPACTION_NOTICE));
            }
            tokens
        }
        ExchangeItem::CompactionInstruction { text } => u64::from(estimate_text_tokens(text)),
    }
}

/// 一个工具结果的估算：内容块 + 错误文本（失败状态也占上下文）。
fn estimate_outcome_tokens(outcome: &crate::ports::ToolOutcome) -> u64 {
    use crate::ports::ToolOutcome;
    match outcome {
        ToolOutcome::Success { result } => result
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => u64::from(estimate_text_tokens(text)),
                ContentBlock::Reasoning { text } => u64::from(estimate_text_tokens(text)),
                ContentBlock::ResourceRef { resource } => {
                    u64::from(estimate_text_tokens(
                        &resource.uri.clone().unwrap_or_default(),
                    )) + 3
                }
                ContentBlock::Opaque { data, .. } => {
                    u64::from(estimate_text_tokens(&data.to_string()))
                }
            })
            .sum(),
        ToolOutcome::Failed { error } => u64::from(estimate_text_tokens(&error.message)),
        ToolOutcome::Cancelled => 1,
        ToolOutcome::Unknown { reason } => u64::from(estimate_text_tokens(reason)),
    }
}

/// 一段交换的估算总量。
pub fn estimate_exchange_tokens(items: &[ExchangeItem]) -> u64 {
    items.iter().map(estimate_exchange_item_tokens).sum()
}

/// 摘要输出 token 上限（现役 `getCachePreservingCompactionMaxTokens`，
/// 逐字公式 `max(512, floor(0.8 × reserve))`——512 下限保留；现役
/// 不与路由声明的 maxTokens 取 min，本实现同）。
///
/// 用途锚定（R2-F-01 改锚）：该公式在现役只服务**预算估算 / BOUNDED
/// 政策 / required-cap 族兜底**三处，**不是**线体默认——PROVIDER_DEFAULT
/// 生产链（`session-compactor.ts:1867-1876` → `normalizeCompaction
/// ProviderPayload` L313-352）对 optional-cap 族**删除全部输出上限
/// 字段**，对 required-cap 族回填 `min(model.maxTokens‖maxOutput,
/// contextWindow)`。线体分流见 [`output_cap_required`] 与
/// [`required_output_cap`]；本公式在 Rust 服务 fit 检查的预算估算与
/// required 族的双缺兜底。
pub fn summary_output_cap(reserve_tokens: u64) -> u32 {
    let cap = (reserve_tokens.saturating_mul(4) / 5).max(512);
    u32::try_from(cap).unwrap_or(u32::MAX)
}

/// 摘要请求的输出上限是否**协议必需**（现役 `core/provider-compat/
/// output-budget.ts` `OUTPUT_CAP_CAPABILITIES` 清单的 Rust 形态，判定
/// 顺序逐字对齐）：
/// 1. `explicit-required`：声明式 compat `outputCapRequired === true`；
/// 2. `official-deepseek`：provider 为 deepseek 或端点含
///    `api.deepseek.com` → **非必需**（optional）；
/// 3. `anthropic-native`：provider 为 anthropic 或端点含
///    `api.anthropic.com` → 必需；
/// 4. `bedrock-native`：provider ∈ {amazon-bedrock, bedrock} → 必需；
/// 5. `anthropic-messages`：协议族 = AnthropicMessages → 必需；
/// 6. 其余 → 非必需（default-optional：openai×3/google 等）。
///
/// `declared` 即 RouteCompatHints 的 `output_cap_required`（N-8 补上的
/// 表达面；`Some(false)` 不豁免后续清单判定——现役同样只认 `=== true`
/// 的显式声明，其余声明值落到族清单）。
pub fn output_cap_required(
    family: crate::model_exchange::ProtocolFamily,
    provider: &str,
    endpoint: &str,
    declared: Option<bool>,
) -> bool {
    if declared == Some(true) {
        return true;
    }
    if provider == "deepseek" || endpoint.contains("api.deepseek.com") {
        return false;
    }
    if provider == "anthropic" || endpoint.contains("api.anthropic.com") {
        return true;
    }
    if provider == "amazon-bedrock" || provider == "bedrock" {
        return true;
    }
    family == crate::model_exchange::ProtocolFamily::AnthropicMessages
}

/// required-cap 族的线体回填值（现役 `safeRequiredOutputCap`，
/// `session-compactor.ts:298-305`）：`min(model.maxTokens‖maxOutput,
/// contextWindow)` 的正整数候选取最小；两值皆缺（或非正）时回退
/// `max(512, floor(0.8 × reserve))` 公式（现役同一兜底臂
/// `positiveInteger(boundedMaxTokens) ?? 1`，Rust 公式恒 ≥512）。
/// 现役 `maxTokens‖maxOutput` 双字段在 Rust 由 RouteCompatHints 的
/// `max_tokens` 单字段表达（compat 映射面仅此一槽）。
pub fn required_output_cap(
    model_max_tokens: Option<u64>,
    context_window: Option<u64>,
    reserve_tokens: u64,
) -> u32 {
    let candidate = [model_max_tokens, context_window]
        .into_iter()
        .flatten()
        .filter(|value| *value > 0)
        .min();
    match candidate {
        Some(value) => u32::try_from(value).unwrap_or(u32::MAX),
        None => summary_output_cap(reserve_tokens),
    }
}

/// 现役 `COMPACTION_REQUEST_BUFFER_TOKENS`（=1024）：fit 检查估算中
/// 为请求包装/渲染损耗预留的固定缓冲。
pub const COMPACTION_REQUEST_BUFFER_TOKENS: u64 = 1_024;

/// 现役硬截断阈值（`DEFAULT_HARD_TRUNCATE_THRESHOLD = 0.85`）：摘要请求
/// 估算总量 > floor(window × 0.85) 即判定自身超窗。基点表示避免浮点。
pub const HARD_TRUNCATE_THRESHOLD_BP: u64 = 8_500;

/// 摘要请求自身超窗的现役兜底标记摘要（`session-compactor.ts`
/// `hardTruncateCachePreservingCompaction` 默认 summary，逐字）——
/// 诚实降级：保留区逐字保留、不调摘要模型、绝不冒充模型摘要成功。
/// （现役 guard-ext L93 另有一处措辞变体「摘要请求本身会超限」，同一
/// 语义、同一降级形状；Rust 取共享管线 session-compactor 一版。）
pub const HARD_TRUNCATE_MARKER_TEXT: &str =
    "[由于对话过长且压缩请求本身会超限，早期对话历史已被硬截断（hana-cache-preserving-compaction）]";

/// 现役 `shouldHardTruncateCachePreservingCompaction` 的缓存保留臂判定
/// （Rust 形态）：窗口未声明（≤0）→ 超窗（现役同一臂）；否则
/// `estimated_total > floor(window × 0.85)` → 超窗。现役的
/// native-fallback 臂（A 超 B 不超 → 走原生压缩）在 Rust 无挂载面
/// （原生压缩路径未迁移），统一落入硬截断——差异已入报告 §十。
pub fn cache_preserving_request_fits(estimated_total: u64, context_window: u64) -> bool {
    if context_window == 0 {
        return false;
    }
    let threshold = context_window.saturating_mul(HARD_TRUNCATE_THRESHOLD_BP) / 10_000;
    estimated_total <= threshold
}

// ─── 预算器（ContextBudgeter） ───

/// 一个 run 的上下文预算（任务书交付物 **ContextBudgeter**）。窗口来自
/// 路由绑定的声明式 compat（`contextWindow`）；reserve/keepRecent 按
/// 现役公式派生。窗口未声明 = 无预算可管（现役同一道门：
/// `if (!(contextWindow > 0)) return false;`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    pub context_window: u64,
    /// 输出保留（兼触发线之一）：max(16384, ceil(window×20%))。
    pub reserve_tokens: u64,
    /// 压缩后保留尾部的目标规模。
    pub keep_recent_tokens: u64,
}

impl ContextBudget {
    /// 窗口声明缺失或为 0 → None（不触发、不估算触发，现役语义）。
    pub fn for_declared_window(context_window: Option<u64>) -> Option<Self> {
        let window = context_window.filter(|window| *window > 0)?;
        Some(Self {
            context_window: window,
            reserve_tokens: compute_reserve_tokens(window),
            keep_recent_tokens: KEEP_RECENT_TOKENS,
        })
    }

    /// 分量报告（判定无关）：usage 与估算的完整账目。手动压缩路径
    /// （跳过判定）与 [`Self::evaluate`] 共用同一构造——两路径的账目
    /// 口径逐字段一致。
    pub fn report(&self, facts: &TurnBudgetFacts) -> TurnBudgetReport {
        let usage_context_tokens = facts
            .last_usage
            .as_ref()
            .and_then(|usage| context_tokens_from_usage(usage, facts.usage_inclusion));
        let context_tokens = usage_context_tokens
            .unwrap_or(0)
            .saturating_add(facts.tail_tokens);
        let ratio_bp = if self.context_window == 0 {
            0
        } else {
            context_tokens
                .saturating_mul(10_000)
                .saturating_div(self.context_window)
        };
        TurnBudgetReport {
            system_tokens: facts.system_tokens,
            submission_tokens: facts.submission_tokens,
            tool_declaration_tokens: facts.tool_declaration_tokens,
            history_estimate_tokens: facts.history_tokens,
            tail_estimate_tokens: facts.tail_tokens,
            output_reserve_tokens: self.reserve_tokens,
            usage_context_tokens,
            context_tokens,
            context_window: self.context_window,
            ratio_bp,
        }
    }

    /// 评估一次 turn 边界的预算。判定只看真实信号（usage 总量 + 尾部
    /// 工具结果估算）；完整分量拆分随报告返回（任务书步骤②的留空间
    /// 账目）。
    pub fn evaluate(&self, facts: &TurnBudgetFacts) -> CompactionDecision {
        let report = self.report(facts);
        let Some(usage_tokens) = report.usage_context_tokens else {
            return CompactionDecision::Unavailable {
                reason: "no usable usage fact from the last resolved model call",
            };
        };
        if usage_tokens == 0 {
            return CompactionDecision::Unavailable {
                reason: "the last resolved model call reported zero context tokens",
            };
        }
        // 现役：`ratio >= COMPACTION_FORCE_RATIO || shouldCompact(...)`，
        // shouldCompact = contextTokens > window - reserveTokens。
        let over_reserve_line =
            report.context_tokens > self.context_window.saturating_sub(self.reserve_tokens);
        if report.ratio_bp >= COMPACTION_FORCE_RATIO_BP || over_reserve_line {
            CompactionDecision::Force { report }
        } else {
            CompactionDecision::BelowThreshold { report }
        }
    }
}

/// 一次 turn 边界评估的输入事实。usage 为主信号；各估算分量（系统约束/
/// 当前请求/工具定义/历史）随报告携带供诊断与审计。
#[derive(Debug, Clone)]
pub struct TurnBudgetFacts {
    /// 系统约束（T01 冻结 artifact 渲染文本的估算）。
    pub system_tokens: u64,
    /// 当前请求（submission）估算。当前目标永不被压缩——它不在可压区。
    pub submission_tokens: u64,
    /// 工具定义快照估算。
    pub tool_declaration_tokens: u64,
    /// 全量历史（交换）估算。
    pub history_tokens: u64,
    /// 最近一次 usage 之后新进上下文的尾部估算（工具结果）。
    pub tail_tokens: u64,
    /// 最近一次已解析模型调用的 usage（Unknown/Invalid → None）。
    pub last_usage: Option<ModelCallUsage>,
    /// `last_usage` 所属族的 inclusion 口径（其 wire input 是否已含
    /// cache 分量；service 从 chat 路由的协议族经 R05 `USAGE_MAPPINGS`
    /// 解析）。判定主信号的族口径——缺失/未知族 = false/false（分量
    /// 独立口径，不扣不加）。
    pub usage_inclusion: CacheInclusion,
}

/// 预算评估报告：分量账目 + 判定依据（触发时的可诊断事实）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnBudgetReport {
    pub system_tokens: u64,
    pub submission_tokens: u64,
    pub tool_declaration_tokens: u64,
    pub history_estimate_tokens: u64,
    pub tail_estimate_tokens: u64,
    pub output_reserve_tokens: u64,
    pub usage_context_tokens: Option<u64>,
    pub context_tokens: u64,
    pub context_window: u64,
    /// context_tokens / window，基点（万分之一）。
    pub ratio_bp: u64,
}

/// 预算判定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionDecision {
    /// 不具备判定条件（无窗口声明 / 无可用 usage）——现役 `return false`。
    Unavailable { reason: &'static str },
    /// 未过线。
    BelowThreshold { report: TurnBudgetReport },
    /// 过 FORCE 线或 reserve 线：立即压缩。
    Force { report: TurnBudgetReport },
}

// ─── 切点规划 ───

/// 一次压缩计划：保留区从 `cut_index` 开始，[0, cut) 进入摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionPlan {
    pub cut_index: usize,
    /// 被摘要区的估算 token 量。
    pub summarized_tokens: u64,
    /// 保留区的估算 token 量。
    pub retained_tokens: u64,
}

/// 规划失败：工具调用/结果配对不可证明（现役
/// `completeToolTransactionTrimBoundaries` 的 unprovable 分支）——响亮
/// 拒绝，绝不猜着切。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactionPlanError {
    UnprovableToolPairs { detail: String },
}

impl std::fmt::Display for CompactionPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnprovableToolPairs { detail } => {
                write!(f, "tool transaction trim boundaries unprovable: {detail}")
            }
        }
    }
}

impl std::error::Error for CompactionPlanError {}

/// 规划一次压缩（现役 `findCutPoint` + 配对证明的 Rust 形态）。
///
/// 返回 `Ok(None)` = 没有有益的切点（历史装得进 keep_recent，或交换为
/// 空/只有一个不可拆的组）——调用方不得为此白调摘要模型。
///
/// 保证：
/// 1. 切点只落在 assistant 组边界或既有压缩摘要之后，永不落在
///    toolResult 上；
/// 2. 保留区内每个 toolResult 的归属 assistant turn 同在保留区（无孤立
///    result）；被摘要区每个 assistant turn 的结果同在被摘要区（无孤立
///    调用）；
/// 3. 未闭合（pending）工具调用所在组整体保留（R06-A03：其未来结果
///    永不孤立）。
pub fn plan_compaction(
    exchange: &[ExchangeItem],
    keep_recent_tokens: u64,
) -> Result<Option<CompactionPlan>, CompactionPlanError> {
    if exchange.is_empty() {
        return Ok(None);
    }
    // ── 配对证明（先证明，后规划） ──
    // tool_call_id → 归属 assistant 组的下标。
    let mut owner_of: HashMap<&str, usize> = HashMap::new();
    for (index, item) in exchange.iter().enumerate() {
        if let ExchangeItem::AssistantTurn { tool_calls, .. } = item {
            for call in tool_calls {
                if owner_of.insert(call.tool_call_id.as_str(), index).is_some() {
                    return Err(CompactionPlanError::UnprovableToolPairs {
                        detail: format!(
                            "tool call id {} is claimed by two assistant turns",
                            call.tool_call_id
                        ),
                    });
                }
            }
        }
    }
    let mut result_count_of: HashMap<&str, usize> = HashMap::new();
    // 每个 assistant 组的下标 → 其结果是否全部齐备（pending 检测）。
    let mut answered_calls: HashMap<&str, usize> = HashMap::new();
    for (index, item) in exchange.iter().enumerate() {
        match item {
            ExchangeItem::ToolResult { tool_call_id, .. } => {
                let Some(owner) = owner_of.get(tool_call_id.as_str()) else {
                    return Err(CompactionPlanError::UnprovableToolPairs {
                        detail: format!(
                            "tool result {tool_call_id} has no owning assistant turn in this exchange"
                        ),
                    });
                };
                if *owner >= index {
                    return Err(CompactionPlanError::UnprovableToolPairs {
                        detail: format!(
                            "tool result {tool_call_id} precedes its owning assistant turn"
                        ),
                    });
                }
                let count = result_count_of.entry(tool_call_id.as_str()).or_insert(0);
                *count += 1;
                if *count > 1 {
                    return Err(CompactionPlanError::UnprovableToolPairs {
                        detail: format!("tool call {tool_call_id} has duplicate results"),
                    });
                }
                *answered_calls.entry(tool_call_id.as_str()).or_insert(0) += 1;
            }
            ExchangeItem::AssistantTurn { .. } | ExchangeItem::CompactionSummary { .. } => {}
            ExchangeItem::CompactionInstruction { .. } => {
                return Err(CompactionPlanError::UnprovableToolPairs {
                    detail: "a compaction instruction item must never enter a live exchange"
                        .to_string(),
                });
            }
        }
    }
    // pending 组 = 含未闭合调用的 assistant 组；最早 pending 组下标是切点
    // 的硬上界（pending 组必须整体保留）。
    let mut earliest_pending: Option<usize> = None;
    for (index, item) in exchange.iter().enumerate() {
        if let ExchangeItem::AssistantTurn { tool_calls, .. } = item {
            let pending = tool_calls
                .iter()
                .any(|call| !answered_calls.contains_key(call.tool_call_id.as_str()));
            if pending {
                earliest_pending = Some(match earliest_pending {
                    Some(earlier) => earlier.min(index),
                    None => index,
                });
            }
        }
    }

    // ── 合法切点（组边界）：assistant turn 或既有压缩摘要 ──
    let valid_cut = |index: usize| -> bool {
        matches!(
            exchange[index],
            ExchangeItem::AssistantTurn { .. } | ExchangeItem::CompactionSummary { .. }
        ) && index > 0
    };

    // ── 保留区无孤儿（保证 2 的实现）：suffix_min_owner[i] = 位置 ≥ i 的
    // 全部 ToolResult 的归属 assistant 组下标的最小值（None = i 之后无任何
    // ToolResult）。切点 cut 合法的另一必要条件：suffix_min_owner[cut] ≥ cut
    // ——否则保留区携带 owner 已被摘要的孤立 result（渲染上线即协议违例；
    // 且该产物再次进入本函数时配对证明必失败，毒化此后所有压缩）。
    // 配对证明已通过 ⇒ 每个 ToolResult 在 owner_of 中必有归属，直接索引。
    let mut suffix_min_owner: Vec<Option<usize>> = vec![None; exchange.len() + 1];
    for index in (0..exchange.len()).rev() {
        let mut best = suffix_min_owner[index + 1];
        if let ExchangeItem::ToolResult { tool_call_id, .. } = &exchange[index] {
            let owner = owner_of[tool_call_id.as_str()];
            best = Some(best.map_or(owner, |known: usize| known.min(owner)));
        }
        suffix_min_owner[index] = best;
    }
    let retained_has_no_orphans =
        |cut: usize| -> bool { suffix_min_owner[cut].is_none_or(|min_owner| min_owner >= cut) };

    // ── 从尾向前累积，找到 keep_recent 的跨界位置（现役 findCutPoint） ──
    let mut accumulated: u64 = 0;
    let mut crossing: Option<usize> = None;
    for index in (0..exchange.len()).rev() {
        accumulated = accumulated.saturating_add(estimate_exchange_item_tokens(&exchange[index]));
        if accumulated >= keep_recent_tokens {
            crossing = Some(index);
            break;
        }
    }
    let Some(crossing) = crossing else {
        // 全部历史装得进 keep_recent：没有有益的切点。
        return Ok(None);
    };
    // 切点方向与现役**不同**（R2-F-03 改锚）：现役 findCutPoint 是**前跳**
    // （compaction.js L266-273——从尾累积到 ≥keepRecent 后取第一个 ≥crossing
    // 的合法切点，跨界项留在旧区，保留区可 <keepRecent）；本实现是**回退**
    // ——跨界项计入保留区，切点落在 ≤crossing 的最近合法组边界，保留区恒
    // ≥keep_recent。回退由 Rust 的组边界+无孤儿约束自然导出（前跳会越过
    // 跨界 ToolResult 制造孤儿），功能上更强（保留更多），方向分叉已入
    // 报告 §十 差异清单。候选切点须同时满足：
    // (a) 组边界（valid_cut）；(b) 不超过最早 pending 组（含）——pending
    // 组必须整体保留；(c) 保留区无孤儿 tool result（owner 同区）。三个
    // 条件在 [1, crossing] 上从后向前取第一个全满足的候选；一个都没有 =
    // 没有安全切点（Ok(None)，宁可不压，绝不制造孤立 result）。
    let pending_bound = earliest_pending.unwrap_or(usize::MAX);
    let cut = (1..=crossing).rev().find(|index| {
        *index <= pending_bound && valid_cut(*index) && retained_has_no_orphans(*index)
    });
    let Some(cut_index) = cut else {
        // 找不到更早的安全切点（如 pending 组之前无任何组边界，或任何
        // 组边界都会把某 owner 与其 result 切开）。
        return Ok(None);
    };
    if cut_index == 0 {
        return Ok(None);
    }
    let summarized_tokens = estimate_exchange_tokens(&exchange[..cut_index]);
    let retained_tokens = estimate_exchange_tokens(&exchange[cut_index..]);
    if summarized_tokens == 0 {
        return Ok(None);
    }
    Ok(Some(CompactionPlan {
        cut_index,
        summarized_tokens,
        retained_tokens,
    }))
}

/// 应用计划：返回新交换 = [摘要项] + 保留区（逐字）。原交换不被修改。
/// `mid_run` = 触发源语义（现役：仅 run 内自动压缩追加
/// `MIDRUN_COMPACTION_NOTICE`；`/compact` 手动压缩不追加——
/// `bridge-session-manager.ts compactSession` 无 notice，notice 只在
/// `session-compaction-runtime` 的 mid-run 路径追加）。
pub fn apply_plan(
    exchange: &[ExchangeItem],
    plan: &CompactionPlan,
    summary: String,
    mid_run: bool,
) -> Vec<ExchangeItem> {
    let mut projected = Vec::with_capacity(exchange.len() - plan.cut_index + 1);
    projected.push(ExchangeItem::CompactionSummary {
        summary,
        covered_items: u32::try_from(plan.cut_index).unwrap_or(u32::MAX),
        mid_run,
    });
    projected.extend_from_slice(&exchange[plan.cut_index..]);
    projected
}

// ─── fileOps enrichment（现役摘要后处理第一段） ───

/// 文件操作清单（现役 `computeFileLists` 的输出形状）：已排序的
/// 「只读」与「已修改」两个路径列表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileOperationLists {
    /// read − modified（排序）。
    pub read_only: Vec<String>,
    /// edited ∪ written（排序）。
    pub modified: Vec<String>,
}

/// 从旧区提取文件操作（现役 `extractFileOperations`，
/// `compaction.js:16-41` + `compaction-utils.js` 的 Rust 形态）：
/// - 扫旧区全部 AssistantTurn 的工具调用，wire 名 ∈ {read, write, edit}
///   且 `arguments.path` 为非空字符串的计入对应集合。Rust 交换层的
///   `target` 是注册表身份（如 `tool:first-party:read`），末段即现役
///   `block.name` 的 wire 名等价物（first-party 工具 local_name =
///   wire 名）。
/// - 跨轮续传：旧区**最后一个** CompactionSummary 的正文中由本函数
///   此前追加的 `<read-files>`/`<modified-files>` 段被解析回集合并
///   播种（现役从 entry.details 读结构化数据；Rust 摘要项无 details
///   字段，段即数据面——写读同源，取最后一次出现防模型正文撞名）。
///   播种语义逐字对齐现役：readFiles→read、modifiedFiles→**edited**。
pub fn extract_file_operations(old_region: &[ExchangeItem]) -> FileOperationLists {
    let mut read = std::collections::BTreeSet::new();
    let mut written = std::collections::BTreeSet::new();
    let mut edited = std::collections::BTreeSet::new();
    // 续传播种：旧区最后一个摘要项的段（本函数此前写入，格式可知）。
    if let Some(ExchangeItem::CompactionSummary { summary, .. }) = old_region
        .iter()
        .rev()
        .find(|item| matches!(item, ExchangeItem::CompactionSummary { .. }))
    {
        for path in parse_file_operation_section(summary, "read-files") {
            read.insert(path);
        }
        for path in parse_file_operation_section(summary, "modified-files") {
            edited.insert(path);
        }
    }
    for item in old_region {
        let ExchangeItem::AssistantTurn { tool_calls, .. } = item else {
            continue;
        };
        for call in tool_calls {
            let Some(path) = call
                .arguments
                .as_value()
                .get("path")
                .and_then(|value| value.as_str())
                .filter(|path| !path.is_empty())
            else {
                continue;
            };
            match call.target.rsplit(':').next() {
                Some("read") => {
                    read.insert(path.to_string());
                }
                Some("write") => {
                    written.insert(path.to_string());
                }
                Some("edit") => {
                    edited.insert(path.to_string());
                }
                _ => {}
            }
        }
    }
    // 现役 computeFileLists：modified = edited ∪ written；
    // readOnly = read − modified；两列表均排序（BTreeSet 已序）。
    let modified: std::collections::BTreeSet<String> = edited.into_iter().chain(written).collect();
    let read_only = read
        .into_iter()
        .filter(|path| !modified.contains(path))
        .collect();
    FileOperationLists {
        read_only,
        modified: modified.into_iter().collect(),
    }
}

/// 解析摘要正文末尾的 `<read-files>`/`<modified-files>` 段（本模块写入
/// 格式的逆运算；取**最后一次**出现——段由本函数追加在正文末尾，模型
/// 正文里若撞名只认最后一处）。逐行匹配，标签独占一行。
fn parse_file_operation_section(text: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut paths = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line == open {
            inside = true;
            paths.clear();
        } else if line == close {
            inside = false;
        } else if inside && !line.is_empty() {
            paths.push(line.to_string());
        }
    }
    paths
}

/// 现役 `appendFileOperationContext`（`session-compactor.ts:1893` 追加链
/// 的第一段，无外部依赖的那段）：清单均空 → 正文逐字原样返回；否则
/// `summary.trimEnd() + "\n\n" + 段（"\n\n" 连接）`，段格式逐字对齐
/// 现役 `formatFileOperations`（`<read-files>\n…\n</read-files>` /
/// `<modified-files>\n…\n</modified-files>`）。
/// `old_region` = 本次被摘要的交换区（含可能的上一摘要——跨轮续传）。
pub fn append_file_operation_context(summary: &str, old_region: &[ExchangeItem]) -> String {
    let lists = extract_file_operations(old_region);
    let mut sections = Vec::new();
    if !lists.read_only.is_empty() {
        sections.push(format!(
            "<read-files>\n{}\n</read-files>",
            lists.read_only.join("\n")
        ));
    }
    if !lists.modified.is_empty() {
        sections.push(format!(
            "<modified-files>\n{}\n</modified-files>",
            lists.modified.join("\n")
        ));
    }
    if sections.is_empty() {
        return summary.to_string();
    }
    format!("{}\n\n{}", summary.trim_end(), sections.join("\n\n"))
}

// ─── 摘要指令与修复指令（现役模板） ───

/// 摘要指令的边界参数。
#[derive(Debug, Clone)]
pub struct SummaryInstructionSpec {
    /// 被摘要区的交换项数（边界以项数声明：线上消息索引要等家族渲染器
    /// 渲染后才能确知，而同一指令要服务全部五个族——项数是确定、诚实、
    /// 与族无关的边界表述；见 R06-T02 报告差异清单）。
    pub old_region_items: usize,
    /// 现役 `preparation.isSplitTurn`（`session-compactor.ts:410-414`）：
    /// 切点落在 turn 中段（发起该 turn 的用户消息留在旧区）。Rust 切点
    /// 恒为组边界，等价形状 = 切点项为 AssistantTurn（R2-F-05 裁决）。
    pub split_turn: bool,
    /// 现役 customInstructions（"Additional focus …"）。
    pub custom_focus: Option<String>,
}

/// 现役 `buildCachePreservingCompactionInstructionValue` 的 Rust 形态：
/// 同一份结构化检查点模板，边界以交换项数表述。
pub fn build_summary_instruction(spec: &SummaryInstructionSpec) -> String {
    let mut lines = vec![
        "Internal compaction-only run.".to_string(),
        "Do not call tools. Do not address the user.".to_string(),
        "Do not output <mood>, <pulse>, <reflect>, or any other internal narration.".to_string(),
        "Return only the exact structured checkpoint format below.".to_string(),
        format!(
            "Old region: the first {} exchange item(s) of the conversation above (counting each \
             assistant message and each tool result as one item), beginning after the opening \
             user message. Summarize only that old region.",
            spec.old_region_items
        ),
        format!(
            "Retained boundary: exchange item #{} and everything after it. Those messages remain \
             verbatim in the session.",
            spec.old_region_items
        ),
        "Use recent-tail content only to understand continuity; never restate it as though it \
         will be removed."
            .to_string(),
        "If the live prefix begins with an existing compaction checkpoint, incorporate it from \
         that position without duplicating it."
            .to_string(),
    ];
    // 现役 session-compactor.ts:410-414 条件行（逐字），位置在
    // recent-tail 提示之后、customInstructions 之前。
    if spec.split_turn {
        lines.push(
            "This is a split-turn compaction: preserve the original request and early progress \
             needed to understand the retained suffix."
                .to_string(),
        );
    }
    if let Some(focus) = &spec.custom_focus {
        lines.push(format!("Additional focus for the checkpoint only: {focus}"));
    }
    let scope = lines.join("\n");
    format!(
        "{scope}\n\nUse this EXACT format:\n\n\
         ## Goal\n\
         [What is the user trying to accomplish? Can be multiple items if the session covers \
         different tasks.]\n\n\
         ## Constraints & Preferences\n\
         - [Any constraints, preferences, or requirements mentioned by user]\n\
         - [Or \"(none)\" if none were mentioned]\n\n\
         ## Progress\n\
         ### Done\n\
         - [x] [Completed tasks/changes]\n\n\
         ### In Progress\n\
         - [ ] [Current work]\n\n\
         ### Blocked\n\
         - [Issues preventing progress, if any]\n\n\
         ## Key Decisions\n\
         - **[Decision]**: [Brief rationale]\n\n\
         ## Next Steps\n\
         1. [Ordered list of what should happen next]\n\n\
         ## Critical Context\n\
         - [Only old-region context needed to continue from the retained suffix]\n\
         - [Or \"(none)\" if not applicable]\n\n\
         Keep each section concise. Preserve exact file paths, function names, and error messages."
    )
}

/// 现役 `createRepairInstruction` 的 Rust 形态：一次格式修复的指令
/// （问题清单 + 草稿原文）。
pub fn build_repair_instruction(issues: &[String], draft: &str) -> String {
    let mut text = String::from(
        "Internal compaction summary repair.\n\n\
         The previous draft cannot be accepted.\n\n\
         Do not call tools. Do not address the user.\n\n",
    );
    let _ = writeln!(text, "Validation failures:");
    for issue in issues {
        let _ = writeln!(text, "- {issue}");
    }
    let _ = write!(
        text,
        "\nReturn only the repaired summary with these level-two headings exactly once and in \
         order:\n## Goal\n## Constraints & Preferences\n## Progress\n## Key Decisions\n\
         ## Next Steps\n## Critical Context\n\n\
         Inside \"## Progress\", use exactly these level-three headings once and in order:\n\
         ### Done\n### In Progress\n### Blocked\n\n\
         <draft-summary>\n{draft}\n</draft-summary>"
    );
    text
}

// ─── 清洗（sanitize）与校验（validate） ───

/// 清洗结果：正文 + 被剥离的标签类别 + 未闭合残留。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedSummary {
    pub text: String,
    pub removed: Vec<&'static str>,
    pub unmatched: Vec<&'static str>,
}

/// 现役 `sanitizeSummary`：剥离 mood/pulse/reflect 的闭合块（XML 与围栏
/// 两种形态），折叠 3+ 连续空行为一个空行；未闭合残留标签记入
/// `unmatched`（交由校验判失败，绝不静默保留）。
pub fn sanitize_summary(raw: &str) -> SanitizedSummary {
    let mut text = raw.to_string();
    let mut removed: Vec<&'static str> = Vec::new();
    for tag in INTERNAL_NARRATION_TAGS {
        if strip_closed_xml_blocks(&mut text, tag) && !removed.contains(&tag) {
            removed.push(tag);
        }
        if strip_fenced_blocks(&mut text, tag) && !removed.contains(&tag) {
            removed.push(tag);
        }
    }
    text = collapse_blank_line_runs(&text);
    let mut unmatched: Vec<&'static str> = Vec::new();
    let lowered = text.to_lowercase();
    for tag in INTERNAL_NARRATION_TAGS {
        let open = format!("<{tag}");
        let close = format!("</{tag}");
        let fence = format!("```{tag}");
        if contains_word_boundary(&lowered, &open)
            || contains_word_boundary(&lowered, &close)
            || contains_word_boundary(&lowered, &fence)
        {
            unmatched.push(tag);
        }
    }
    SanitizedSummary {
        text: text.trim().to_string(),
        removed,
        unmatched,
    }
}

/// `needle`（如 `<mood`）是否在 `haystack` 中出现且标签名后紧跟词边界
/// （非字母数字或 `>`/空白）——避免 `<moodlight>` 误命中 `mood`。
fn contains_word_boundary(haystack: &str, needle: &str) -> bool {
    let mut start = 0;
    while let Some(found) = haystack[start..].find(needle) {
        let at = start + found;
        let after = &haystack[at + needle.len()..];
        let boundary = after
            .chars()
            .next()
            .map(|ch| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
            .unwrap_or(true);
        if boundary {
            return true;
        }
        start = at + needle.len();
    }
    false
}

/// 剥离 `<tag ...>...</tag>` 闭合块（大小写不敏感；开标签可带属性；
/// 非贪婪——第一个闭合标签结束该块，与现役正则一致）。返回是否剥离过。
fn strip_closed_xml_blocks(text: &mut String, tag: &str) -> bool {
    let mut stripped = false;
    loop {
        let lowered = text.to_lowercase();
        let open = format!("<{tag}");
        let Some(open_at) = find_word_boundary(&lowered, &open, 0) else {
            return stripped;
        };
        // 开标签必须闭合于 `>`，否则视为未闭合残留（交给 unmatched）。
        let Some(open_end_rel) = lowered[open_at..].find('>') else {
            return stripped;
        };
        let content_start = open_at + open_end_rel + 1;
        let close = format!("</{tag}");
        let Some(close_at) = find_word_boundary(&lowered, &close, content_start) else {
            return stripped;
        };
        let Some(close_end_rel) = lowered[close_at..].find('>') else {
            return stripped;
        };
        let block_end = close_at + close_end_rel + 1;
        text.replace_range(open_at..block_end, "");
        stripped = true;
    }
}

fn find_word_boundary(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let mut start = from;
    while let Some(found) = haystack[start..].find(needle) {
        let at = start + found;
        let after = &haystack[at + needle.len()..];
        let boundary = after
            .chars()
            .next()
            .map(|ch| !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_')
            .unwrap_or(true);
        if boundary {
            return Some(at);
        }
        start = at + needle.len();
    }
    None
}

/// 剥离 ```tag ... ``` 围栏块（大小写不敏感，行首形态）。返回是否剥离过。
fn strip_fenced_blocks(text: &mut String, tag: &str) -> bool {
    let mut stripped = false;
    let fence_open = format!("```{tag}");
    loop {
        let lowered = text.to_lowercase();
        let Some(open_at) = find_word_boundary(&lowered, &fence_open, 0) else {
            return stripped;
        };
        // 开围栏行结束位置。
        let Some(open_line_end_rel) = lowered[open_at..].find('\n') else {
            return stripped; // 未闭合（没有换行就没有闭围栏）
        };
        let body_start = open_at + open_line_end_rel + 1;
        // 闭围栏 = 之后第一个以 ``` 开头的行。
        let mut cursor = body_start;
        let mut close_end = None;
        while cursor <= lowered.len() {
            let line_end = lowered[cursor..]
                .find('\n')
                .map(|rel| cursor + rel)
                .unwrap_or(lowered.len());
            if lowered[cursor..line_end].trim_start().starts_with("```") {
                close_end = Some(if line_end < lowered.len() {
                    line_end + 1
                } else {
                    line_end
                });
                break;
            }
            if line_end >= lowered.len() {
                break;
            }
            cursor = line_end + 1;
        }
        let Some(end) = close_end else {
            return stripped; // 未闭合围栏 → unmatched 处理
        };
        text.replace_range(open_at..end, "");
        stripped = true;
    }
}

/// 折叠 3+ 连续换行（行间可含空白）为恰好一个空行（现役
/// `/\r?\n[ \t]*\r?\n[ \t]*\r?\n+/g → "\n\n"`）。
fn collapse_blank_line_runs(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n");
    let mut out = String::with_capacity(normalized.len());
    let lines = normalized.split('\n');
    let mut blank_run = 0_usize;
    for line in lines {
        if line.trim().is_empty() {
            blank_run += 1;
            continue;
        }
        if blank_run > 0 {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            blank_run = 0;
        } else if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    // 尾部空白行随 trim 消失（调用方 trim）。
    if blank_run > 0 && !out.is_empty() {
        out.push_str("\n\n");
    }
    out
}

/// 现役 `validateSummary`：非空、无未闭合叙事标签、恰好 9 个结构化标题
/// 按序出现。返回问题清单（空 = 通过）。
pub fn validate_summary(text: &str, unmatched: &[&'static str]) -> Result<(), Vec<String>> {
    let mut issues = Vec::new();
    if text.trim().is_empty() {
        issues.push("summary is empty".to_string());
    }
    if !unmatched.is_empty() {
        issues.push(format!(
            "unmatched internal narration tag(s): {}",
            unmatched.join(", ")
        ));
    }
    let headings = extract_headings(text);
    if headings.len() != SUMMARY_HEADING_SEQUENCE.len() {
        issues.push(format!(
            "expected {} structured headings, received {}",
            SUMMARY_HEADING_SEQUENCE.len(),
            headings.len()
        ));
    }
    for (index, expected) in SUMMARY_HEADING_SEQUENCE.iter().enumerate() {
        if headings.get(index).map(String::as_str) != Some(*expected) {
            issues.push(format!("heading {} must be \"{expected}\"", index + 1));
        }
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

/// 提取标题行（现役正则 `/^(#{2,3})[ \t]+(.+?)[ \t]*#*[ \t]*$/gm` 的
/// 手工形态：行首 2-3 个 `#`，空白，正文，尾部 `#` 与空白剥离）。
fn extract_headings(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let hashes = line.chars().take_while(|ch| *ch == '#').count();
            if !(2..=3).contains(&hashes) {
                return None;
            }
            let rest = &line[hashes..];
            if !rest.starts_with(' ') && !rest.starts_with('\t') {
                return None;
            }
            let content = rest.trim_start_matches([' ', '\t']);
            // 尾部 `#` 与空白剥离（闭合式标题 "## Goal ##"）。
            let content = content
                .trim_end_matches([' ', '\t'])
                .trim_end_matches('#')
                .trim_end_matches([' ', '\t']);
            if content.is_empty() {
                return None;
            }
            Some(format!("{} {}", "#".repeat(hashes), content))
        })
        .collect()
}
