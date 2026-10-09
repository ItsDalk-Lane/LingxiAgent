//! R06-T02：mid-run 压缩服务——触发判定、切点规划、摘要生成、清洗校验、
//! 台账与失败语义的生产编排半区（IO 都在这里；纯函数在 kernel
//! `compaction` 模块）。
//!
//! 现役语义来源（逐条锚定，见 R06-T02 报告 §五）：
//! - **触发**：`core/session-compaction-runtime.ts`——turn 边界检查；
//!   真实 usage 总量 + usage 之后的尾部工具结果估算 ≥ FORCE 线（80%）或
//!   `window - reserve`（reserve = max(16384, ceil(window×20%))）；窗口
//!   未声明或 usage 不可得/为零 → 不触发（绝不凭估算触发）。
//! - **缓存保留**：压缩请求形状 = `[会话 system prompt, run submission,
//!   ...live exchange, 指令 user 消息]`——前缀与线上请求逐字节一致
//!   （`lib/llm/cache-preserving-compaction-agent-run.ts`）。
//! - **摘要调用**：经 [`AuxiliaryExecutor`]——路由 = summarize 槽显式配置
//!   优先、槽未配置回退 chat 路由（R3-F-01，§十#17：与现役
//!   `core/auxiliary-slots.ts` summarize 槽 `fallback:"chat"` 对齐；现役
//!   会话压缩摘要用会话模型本人 `session-compactor.ts:1987`，现役
//!   summarize 槽只服务 activity 摘要/autolearn），不绕过
//!   ModelGateway/CredentialService；模型回调工具 = 现役 placeholder
//!   恢复语义（`cache-preserving-compaction-agent-run.ts`
//!   `clonePlaceholderTools` + `shouldStopAfterTurn`）：首次应答的**单个**
//!   工具意图由 placeholder 结果应答后续跑（`tool_recovery` 阶段，工具
//!   从不真正执行）；第二次工具意图或一次多个调用 = 响亮失败
//!   （`toolViolation`）；一次 format_repair 后仍不合规 = 压缩失败。
//! - **失败语义**：压缩失败 = 保留原上下文 + 明确错误 + run 续跑
//!   （现役"压缩失败永不中断 run"）；原始交换是权威，摘要只以普通
//!   user 历史身份进入新交换（绝不获得系统指令优先级）。
//! - **台账**：每次摘要模型调用（含修复调用）落一行
//!   `auxiliary.summarize` usage 记录——成功、失败、取消都有账
//!   （R05 RR1 F21/F38 纪律）。

use std::sync::Arc;

use lingxi_adapters::models::auxiliary::{AuxiliaryExecutor, AuxiliaryRequest, AuxiliaryStep};
use lingxi_adapters::models::gateway::{ConfigModelGateway, ResolvedDispatch};
use lingxi_kernel::compaction::{
    self, CompactionPlan, CompactionPlanError, ContextBudget, TurnBudgetFacts, TurnBudgetReport,
};
use lingxi_kernel::context::estimate_text_tokens;
use lingxi_kernel::model_exchange::{
    AuxiliarySlot, ExchangeItem, ModelOperation, ModelRouteRequest, ProtocolFamily,
    RequestedToolCall, ToolDeclarationSnapshot,
};
use lingxi_kernel::ports::{ProviderDescriptor, StorageError, StoragePort};
use lingxi_kernel::usage::{CallOutcome, ModelCallUsage, ReportedUsage};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ContentBlock, ModelCallId, ToolCallId};

use crate::cancel::CancelScope;
use crate::inject::ServiceClock;
use crate::quotas::{QuotaManager, QuotaResource};

/// 压缩触发评估 + 执行的输入事实（driver 在 loop 顶采集）。
pub struct MidRunCompactionInput<'a> {
    /// 当前 run 的 live 交换（权威历史）。
    pub exchange: &'a [ExchangeItem],
    /// 会话冻结 artifact 的渲染文本（T01）；压缩请求的系统槽与系统分量。
    pub system_prompt: Option<&'a str>,
    /// 本 turn 的 submission（含 steering 汇合后文本）。
    pub submission: &'a str,
    /// 本 turn 的 live 工具声明快照（压缩请求携带——历史渲染的 wire 名
    /// 解析必需）。
    pub tools: &'a ToolDeclarationSnapshot,
    /// 最近一次已解析模型调用的 usage（Unknown/Invalid → None）。
    pub last_usage: Option<&'a ModelCallUsage>,
    /// 尾部起点：最后一个 AssistantTurn 之后的 exchange 下标（其后的
    /// ToolResult 是 usage 之后新进上下文的尾部）。
    pub tail_from: usize,
}

/// 一次 mid-run 压缩判定的结果。
#[derive(Debug)]
pub enum CompactionOutcome {
    /// 未触发：无窗口声明 / 无可用 usage / 未过线 / 无有益切点。
    /// （现役 `return false` 的各臂；run 原样续跑。）
    NotTriggered,
    /// 压缩成功：`exchange` 是新交换（[摘要项] + 保留区）。
    Compacted {
        exchange: Vec<ExchangeItem>,
        plan: CompactionPlan,
        report: TurnBudgetReport,
    },
    /// 摘要请求自身超窗的现役兜底（`session-compactor.ts:2064-2083` fit
    /// 检查 → `hardTruncateCachePreservingCompaction`）：诚实硬截断——
    /// 保留区逐字保留，摘要正文是 [`compaction::HARD_TRUNCATE_MARKER_TEXT`]
    /// 降级标注，**不调摘要模型**（天然消除每 turn 重试循环；现役
    /// native-fallback 臂在 Rust 无挂载面，统一落此臂——差异入 §十）。
    HardTruncated {
        exchange: Vec<ExchangeItem>,
        plan: CompactionPlan,
        report: TurnBudgetReport,
    },
    /// 压缩失败：保留原上下文 + 明确错误（现役语义——run 以原交换
    /// 续跑，绝不截断重要内容继续宣称成功）。
    Failed { detail: String },
    /// 取消在压缩途中获胜：台账行已落（attempts 未知——镜像 F38 的
    /// dropped-before-settlement 形状），driver 走既有取消结算路径。
    Cancelled,
}

/// 一次摘要模型调用的结算事实（成功与失败共用——台账不丢弃失败账）。
struct SummarySettle {
    usage_report: ReportedUsage,
    transport_attempts: Option<u32>,
    served_by: ProviderDescriptor,
    served_protocol: Option<String>,
    outcome: CallOutcome,
    started_at_unix_ms: u64,
    /// 该次调用发出的工具调用身份（placeholder 恢复臂：模型意图的
    /// 占位 id 随台账落行，与 chat 行的 emitted_tool_calls 同一纪律）。
    emitted_tool_calls: Vec<String>,
}

/// The production compaction service (任务书交付物 **CompactionService**).
pub struct CompactionService {
    auxiliary: Arc<AuxiliaryExecutor>,
    gateway: Arc<ConfigModelGateway>,
    quotas: Arc<QuotaManager>,
    clock: Arc<dyn ServiceClock>,
}

impl std::fmt::Debug for CompactionService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompactionService").finish_non_exhaustive()
    }
}

impl CompactionService {
    pub fn new(
        auxiliary: Arc<AuxiliaryExecutor>,
        gateway: Arc<ConfigModelGateway>,
        quotas: Arc<QuotaManager>,
        clock: Arc<dyn ServiceClock>,
    ) -> Self {
        Self {
            auxiliary,
            gateway,
            quotas,
            clock,
        }
    }

    /// turn 边界的压缩判定与执行（现役 session-compaction-runtime 的
    /// `compactIfNeeded` 形态）。失败永不抛出给 run——`Failed` 臂携带
    /// 可诊断错误，run 以原交换续跑；只有台账写入失败（存储层错误）
    /// 才以 `Err` 传播（与全局台账纪律一致）。
    #[allow(clippy::too_many_arguments)]
    pub async fn maybe_compact_mid_run<P: StoragePort>(
        &self,
        ctx: &RunContext,
        port: &P,
        agent_id: &str,
        origin: &'static str,
        parent_run_id: Option<String>,
        cause_ref: Option<String>,
        input: &MidRunCompactionInput<'_>,
        cancel: &CancelScope,
        compaction_seq: u32,
    ) -> Result<CompactionOutcome, StorageError> {
        // ── 1) 窗口解析（chat 路由的声明式 compat；F02 单快照读） ──
        let chat_dispatch = match self
            .gateway
            .resolve_dispatch(&ModelRouteRequest::for_operation(ModelOperation::Chat))
        {
            Ok(dispatch) => dispatch,
            Err(err) => {
                // 无 chat 绑定 = 无窗口可言：不触发（现役同一道门）。
                tracing::debug!(
                    run_id = %ctx.run_id,
                    "compaction skipped: chat route unresolvable ({err})"
                );
                return Ok(CompactionOutcome::NotTriggered);
            }
        };
        let window = chat_dispatch
            .compat
            .as_ref()
            .and_then(|compat| compat.context_window);
        let Some(budget) = ContextBudget::for_declared_window(window) else {
            return Ok(CompactionOutcome::NotTriggered);
        };
        // F-02：判定主信号的族口径——usage 来自 chat 路由族，其 wire input
        // 是否已含 cache 分量由 R05 映射表仲裁（included 绝不重复加计）。
        let usage_inclusion = usage_inclusion_of(chat_dispatch.route.protocol);

        // ── 2) 预算判定（分量账目随报告携带；判定只看真实信号） ──
        let tail_from = input.tail_from.min(input.exchange.len());
        let facts = TurnBudgetFacts {
            system_tokens: input
                .system_prompt
                .map(|text| u64::from(estimate_text_tokens(text)))
                .unwrap_or(0),
            submission_tokens: u64::from(estimate_text_tokens(input.submission)),
            tool_declaration_tokens: estimate_tool_declaration_tokens(input.tools),
            history_tokens: compaction::estimate_exchange_tokens(input.exchange),
            tail_tokens: compaction::estimate_exchange_tokens(&input.exchange[tail_from..]),
            last_usage: input.last_usage.cloned(),
            usage_inclusion,
        };
        let report = match budget.evaluate(&facts) {
            compaction::CompactionDecision::Unavailable { .. }
            | compaction::CompactionDecision::BelowThreshold { .. } => {
                return Ok(CompactionOutcome::NotTriggered);
            }
            compaction::CompactionDecision::Force { report } => report,
        };

        self.compact_exchange(
            ctx,
            port,
            agent_id,
            origin,
            parent_run_id,
            cause_ref,
            input,
            cancel,
            compaction_seq,
            &budget,
            report,
            // 现役：仅 run 内自动压缩的产物携带 MIDRUN_COMPACTION_NOTICE。
            true,
            // R3-F-01：回退链的 chat 腿复用本方法顶部的同代快照（绝不二次
            // 读取跨代混杂——R05 RR1 F02 纪律）。
            Some(&chat_dispatch),
        )
        .await
    }

    /// 手动压缩入口（现役 `/compact` → bridge `compactSession` → 同一
    /// `runCachePreservingCompactionForSession` 管线 的对应物）：跳过
    /// 阈值判定（用户明确点名要压），窗口未声明时按现役 reserve 公式
    /// （`max(16384, ceil(0×20%))` = 16384）与固定 keep_recent 执行。
    /// 窗口解析失败不视为错误（现役手动路径对 contextWindow 返回
    /// null 的形态）。失败语义与自动路径一致：`Failed` 携带可诊断
    /// 错误，调用方保留原历史。
    ///
    /// 会话命令面（slash 派发）不属于本任务范围——Rust 侧尚无其宿主
    /// （run 外历史面归 R06-T04）；本入口是触发源无关的执行面，未来
    /// 命令面直接调它。
    #[allow(clippy::too_many_arguments)]
    pub async fn compact_now<P: StoragePort>(
        &self,
        ctx: &RunContext,
        port: &P,
        agent_id: &str,
        origin: &'static str,
        parent_run_id: Option<String>,
        cause_ref: Option<String>,
        input: &MidRunCompactionInput<'_>,
        cancel: &CancelScope,
        compaction_seq: u32,
    ) -> Result<CompactionOutcome, StorageError> {
        let chat_dispatch = self
            .gateway
            .resolve_dispatch(&ModelRouteRequest::for_operation(ModelOperation::Chat))
            .ok();
        let window = chat_dispatch
            .as_ref()
            .and_then(|dispatch| dispatch.compat.clone())
            .and_then(|compat| compat.context_window)
            .unwrap_or(0);
        // 手动路径的账目口径与自动路径同一函数：族 inclusion 同样从 chat
        // 路由解析（路由不可解析时无 usage 事实可判，缺省 false/false）。
        let usage_inclusion = chat_dispatch
            .as_ref()
            .map(|dispatch| usage_inclusion_of(dispatch.route.protocol))
            .unwrap_or_default();
        let budget = ContextBudget {
            context_window: window,
            reserve_tokens: compaction::compute_reserve_tokens(window),
            keep_recent_tokens: compaction::KEEP_RECENT_TOKENS,
        };
        let tail_from = input.tail_from.min(input.exchange.len());
        let facts = TurnBudgetFacts {
            system_tokens: input
                .system_prompt
                .map(|text| u64::from(estimate_text_tokens(text)))
                .unwrap_or(0),
            submission_tokens: u64::from(estimate_text_tokens(input.submission)),
            tool_declaration_tokens: estimate_tool_declaration_tokens(input.tools),
            history_tokens: compaction::estimate_exchange_tokens(input.exchange),
            tail_tokens: compaction::estimate_exchange_tokens(&input.exchange[tail_from..]),
            last_usage: input.last_usage.cloned(),
            usage_inclusion,
        };
        let report = budget.report(&facts);
        self.compact_exchange(
            ctx,
            port,
            agent_id,
            origin,
            parent_run_id,
            cause_ref,
            input,
            cancel,
            compaction_seq,
            &budget,
            report,
            // 现役 `/compact`（bridge compactSession）不追加 mid-run 通知。
            false,
            chat_dispatch.as_ref(),
        )
        .await
    }

    /// 压缩执行体（触发源无关，自动/手动共用）：切点规划 → 摘要路由
    /// （summarize 槽显式覆盖 + chat 回退，§十#17）→ 配额 → 摘要调用
    /// （placeholder 单次恢复）→ 清洗校验（一次修复）→ apply。`mid_run`
    /// 区分触发源（现役：仅 mid-run 自动压缩的产物携带
    /// MIDRUN_COMPACTION_NOTICE）。`chat_dispatch` = 调用方在入口已解析的
    /// chat 路由同代快照（自动路径恒 Some；手动路径可能 None——回退需要时
    /// 再补一次解析）。
    #[allow(clippy::too_many_arguments)]
    async fn compact_exchange<P: StoragePort>(
        &self,
        ctx: &RunContext,
        port: &P,
        agent_id: &str,
        origin: &'static str,
        parent_run_id: Option<String>,
        cause_ref: Option<String>,
        input: &MidRunCompactionInput<'_>,
        cancel: &CancelScope,
        compaction_seq: u32,
        budget: &ContextBudget,
        report: TurnBudgetReport,
        mid_run: bool,
        chat_dispatch: Option<&ResolvedDispatch>,
    ) -> Result<CompactionOutcome, StorageError> {
        // ── 3) 切点规划（工具配对不可证明 = 响亮拒绝，run 续跑） ──
        let plan = match compaction::plan_compaction(input.exchange, budget.keep_recent_tokens) {
            Ok(Some(plan)) => plan,
            Ok(None) => return Ok(CompactionOutcome::NotTriggered),
            Err(CompactionPlanError::UnprovableToolPairs { detail }) => {
                tracing::warn!(
                    run_id = %ctx.run_id,
                    "compaction refused: {detail}; the run continues with the original exchange"
                );
                return Ok(CompactionOutcome::Failed {
                    detail: format!("tool pairs unprovable: {detail}"),
                });
            }
        };

        // ── 4) 生效摘要路由（R3-F-01，§十#17）：summarize 槽显式配置优先；
        // 槽未配置/不可路由 → 回退 chat 路由（与现役 auxiliary-slots.ts 的
        // summarize 槽 `fallback:"chat"` 对齐，且现役压缩摘要本就是会话模型
        // 本人——session-compactor.ts:1987 `model = session?.model`）。
        // 回退决策只发生在本调用方（显式、可声明、可测试）；执行器自身仍
        // 永不静默改道（R05 C07）。双缺 = 响亮失败，run 以原交换续跑。路由
        // 快照保留：族判定（max_tokens 线体分流）与 fit 检查（摘要模型
        // 窗口）都读它。 ──
        let summarize_resolution =
            self.gateway
                .resolve_dispatch(&ModelRouteRequest::for_operation(
                    ModelOperation::Auxiliary(AuxiliarySlot::Summarize),
                ));
        let (summary_operation, summary_label, effective_dispatch) = match summarize_resolution {
            Ok(dispatch) => (
                ModelOperation::Auxiliary(AuxiliarySlot::Summarize),
                "auxiliary slot summarize".to_string(),
                dispatch,
            ),
            Err(summarize_err) => {
                let chat = match chat_dispatch {
                    Some(dispatch) => Ok(dispatch.clone()),
                    None => self
                        .gateway
                        .resolve_dispatch(&ModelRouteRequest::for_operation(ModelOperation::Chat)),
                };
                match chat {
                    Ok(dispatch) => (
                        ModelOperation::Chat,
                        "the chat route (summarize-slot fallback)".to_string(),
                        dispatch,
                    ),
                    Err(chat_err) => {
                        let detail = format!(
                            "the summarize slot has no resolvable route ({summarize_err}) and \
                             the chat fallback route does not resolve either ({chat_err}); the \
                             run continues with the original exchange"
                        );
                        tracing::warn!(run_id = %ctx.run_id, "compaction failed: {detail}");
                        return Ok(CompactionOutcome::Failed { detail });
                    }
                }
            }
        };

        // ── 5) 摘要指令（提前构造：fit 检查的估算含指令分量）。
        // split_turn = 切点项为 AssistantTurn（现役 isSplitTurn 的 Rust
        // 等价形状——切点恒为组边界，切在 AssistantTurn 即发起该 turn
        // 的用户消息留在旧区；切在 CompactionSummary = 纪元边界，非
        // split-turn）。 ──
        let split_turn = matches!(
            input.exchange[plan.cut_index],
            ExchangeItem::AssistantTurn { .. }
        );
        let instruction =
            compaction::build_summary_instruction(&compaction::SummaryInstructionSpec {
                old_region_items: plan.cut_index,
                split_turn,
                custom_focus: None,
            });

        // ── 6) fit 检查（现役 session-compactor.ts:2064-2083，摘要模型
        // 窗口——现役即会话模型窗口，:1654/2064-2074 的 model 即 :1987 的
        // session.model；R3-F-01 修复后窗口随生效路由走）：估算 = 系统 +
        // submission + 历史 + 指令 + 工具声明 + 1024 缓冲 + 输出上限公式
        // 值；> floor(window×0.85) 或窗口未声明 → 诚实硬截断（不调摘要
        // 模型，标记摘要，绝不冒充成功）。生效路由窗口未声明时取回退后
        // 路由的窗口（R3-F-01：summarize 槽显式覆盖但只声明了模型未声明
        // 窗口 → 回退到 chat 路由窗口而非硬截断；两端皆未声明 → 0 → 现役
        // 同一硬截断臂）。 ──
        let summary_window = effective_dispatch
            .compat
            .as_ref()
            .and_then(|compat| compat.context_window)
            .or_else(|| {
                chat_dispatch
                    .and_then(|dispatch| dispatch.compat.as_ref())
                    .and_then(|compat| compat.context_window)
            })
            .unwrap_or(0);
        let estimated_total = input
            .system_prompt
            .map(|text| u64::from(estimate_text_tokens(text)))
            .unwrap_or(0)
            .saturating_add(u64::from(estimate_text_tokens(input.submission)))
            .saturating_add(compaction::estimate_exchange_tokens(input.exchange))
            .saturating_add(u64::from(estimate_text_tokens(&instruction)))
            .saturating_add(estimate_tool_declaration_tokens(input.tools))
            .saturating_add(compaction::COMPACTION_REQUEST_BUFFER_TOKENS)
            .saturating_add(u64::from(compaction::summary_output_cap(
                budget.reserve_tokens,
            )));
        if !compaction::cache_preserving_request_fits(estimated_total, summary_window) {
            // 无进展护栏（现役 computeHardTruncation `effectiveCutIndex<=0
            // → null` 的 Rust 对应物）：旧区只剩摘要/指令项时硬截断不产生
            // 任何新信息，每 turn 重试只会用同文标记替换同文标记——跳过，
            // run 以原交换续跑。
            let old_region_has_content = input.exchange[..plan.cut_index].iter().any(|item| {
                matches!(
                    item,
                    ExchangeItem::AssistantTurn { .. } | ExchangeItem::ToolResult { .. }
                )
            });
            if !old_region_has_content {
                tracing::warn!(
                    run_id = %ctx.run_id,
                    estimated_total,
                    summary_window,
                    "compaction skipped: the summarize request would exceed the window but \
                     the old region holds only prior summaries; the run continues with the \
                     original exchange"
                );
                return Ok(CompactionOutcome::NotTriggered);
            }
            let exchange = compaction::apply_plan(
                input.exchange,
                &plan,
                compaction::HARD_TRUNCATE_MARKER_TEXT.to_string(),
                mid_run,
            );
            tracing::warn!(
                run_id = %ctx.run_id,
                estimated_total,
                summary_window,
                cut_index = plan.cut_index,
                "compaction degraded to honest hard-truncation: the summarize request would \
                 exceed the summary model window; no model call was made"
            );
            return Ok(CompactionOutcome::HardTruncated {
                exchange,
                plan,
                report,
            });
        }

        // ── 7) 输出上限的族分流（R2-F-01 修复；现役 PROVIDER_DEFAULT 生产
        // 链 `normalizeCompactionProviderPayload` L313-352）：optional-cap
        // 族 → None（渲染器 skip_none，线上无键）；required-cap 族 →
        // Some(min(compat.max_tokens, compat.context_window))，双缺回退
        // max(512, floor(0.8×reserve)) 公式（现役同一兜底臂）。族判定与
        // 回填都读**生效路由**快照（R3-F-01：随回退自动跟随）。 ──
        let summarize_compat = effective_dispatch.compat.as_ref();
        let max_output_tokens = if compaction::output_cap_required(
            effective_dispatch.route.protocol,
            &effective_dispatch.route.provider,
            &effective_dispatch.route.endpoint,
            summarize_compat.and_then(|compat| compat.output_cap_required),
        ) {
            Some(compaction::required_output_cap(
                summarize_compat.and_then(|compat| compat.max_tokens),
                summarize_compat.and_then(|compat| compat.context_window),
                budget.reserve_tokens,
            ))
        } else {
            None
        };

        // ── 8) 配额（与主模型循环同一配额管理器；RAII 覆盖全部出口） ──
        let _permit = match self
            .quotas
            .acquire(QuotaResource::Model, agent_id, ctx.session_id.as_str())
            .await
        {
            Ok(permit) => permit,
            Err(failure) => {
                let detail = format!(
                    "the admission quota refused the compaction call ({failure}); the run \
                     continues with the original exchange"
                );
                tracing::warn!(run_id = %ctx.run_id, "compaction failed: {detail}");
                return Ok(CompactionOutcome::Failed { detail });
            }
        };

        // ── 9) 摘要调用（缓存保留形状；cancel-aware；placeholder 单次
        // 恢复——现役 tool_recovery：首次应答的单个工具意图由 placeholder
        // 结果应答后续跑，工具从不真正执行；第二次意图或一次多调用 =
        // 响亮失败） ──
        // 会话式 prior：与现役 context.messages 同一累积语义——工具意图
        // turn + placeholder 结果、草稿 turn、修复指令依次追加进同一会话。
        let mut prior = input.exchange.to_vec();
        prior.push(ExchangeItem::CompactionInstruction { text: instruction });
        let call_id = ModelCallId::new(format!("{}-compact-{compaction_seq}", ctx.run_id));
        let ledger = LedgerFacts {
            origin,
            parent_run_id: &parent_run_id,
            cause_ref: &cause_ref,
        };
        let request = self.summary_request(input, max_output_tokens, prior.clone());
        let first = self
            .settle_summary_call(
                ctx,
                port,
                &call_id,
                summary_operation,
                &summary_label,
                &request,
                cancel,
                &ledger,
            )
            .await?;
        let Some(settle) = first else {
            return Ok(CompactionOutcome::Cancelled);
        };
        let draft = match settle {
            SummaryCallResult::Text { text } => text,
            SummaryCallResult::Failed { detail } => {
                tracing::warn!(run_id = %ctx.run_id, "compaction failed: {detail}");
                return Ok(CompactionOutcome::Failed { detail });
            }
            SummaryCallResult::ToolIntent { calls, content } => {
                if calls.len() != 1 {
                    // 现役 toolViolation：「Compaction AgentRun tool intent
                    // ceiling exceeded」——一次多个调用不可恢复。
                    let detail = format!(
                        "the summarize model requested {} tool calls in one turn; the \
                         placeholder recovery answers exactly one call exactly once (the \
                         calls were NOT executed); the run continues with the original \
                         exchange",
                        calls.len()
                    );
                    tracing::warn!(run_id = %ctx.run_id, "compaction failed: {detail}");
                    return Ok(CompactionOutcome::Failed { detail });
                }
                // 恢复臂：把模型的工具意图 turn 与其 placeholder 结果追加进
                // 会话（工具从不执行——结果是宿主铸造的占位文本），再发
                // 恰好一次恢复调用。
                let call = calls.into_iter().next().expect("exactly one call");
                let provider_call_id = call.provider_call_id.clone();
                prior.push(ExchangeItem::AssistantTurn {
                    call: call_id.clone(),
                    content,
                    tool_calls: vec![call],
                    origin: None,
                });
                prior.push(ExchangeItem::ToolResult {
                    tool_call_id: ToolCallId::new(format!("{}-tc0001", call_id.as_str())),
                    provider_call_id,
                    outcome: lingxi_kernel::ports::ToolOutcome::success_text(
                        compaction::PLACEHOLDER_TOOL_RESULT_TEXT,
                    ),
                });
                let recovery_call =
                    ModelCallId::new(format!("{}-compact-{compaction_seq}-toolrec", ctx.run_id));
                let recovery_request =
                    self.summary_request(input, max_output_tokens, prior.clone());
                let recovered = self
                    .settle_summary_call(
                        ctx,
                        port,
                        &recovery_call,
                        summary_operation,
                        &summary_label,
                        &recovery_request,
                        cancel,
                        &ledger,
                    )
                    .await?;
                let Some(recovered) = recovered else {
                    return Ok(CompactionOutcome::Cancelled);
                };
                match recovered {
                    SummaryCallResult::Text { text } => text,
                    SummaryCallResult::ToolIntent { .. } => {
                        // 现役 toolViolation：「Tool intent appeared after
                        // the first placeholder recovery turn」。
                        let detail = "tool intent appeared after the first placeholder \
                                      recovery turn; the run continues with the original \
                                      exchange"
                            .to_string();
                        tracing::warn!(run_id = %ctx.run_id, "compaction failed: {detail}");
                        return Ok(CompactionOutcome::Failed { detail });
                    }
                    SummaryCallResult::Failed { detail } => {
                        tracing::warn!(run_id = %ctx.run_id, "compaction failed: {detail}");
                        return Ok(CompactionOutcome::Failed { detail });
                    }
                }
            }
        };

        // ── 10) 清洗 + 校验（一次 format_repair 后仍不合规 = 失败） ──
        let sanitized = compaction::sanitize_summary(&draft);
        match compaction::validate_summary(&sanitized.text, &sanitized.unmatched) {
            Ok(()) => {
                // 现役摘要后处理第一段（appendFileOperationContext）：fileOps
                // 段追加（无 details 时逐字不追加）。硬截断臂不追加（现役
                // computeHardTruncation 的 details 只有 reason/keepRecent）。
                let summary = compaction::append_file_operation_context(
                    &sanitized.text,
                    &input.exchange[..plan.cut_index],
                );
                let exchange = compaction::apply_plan(input.exchange, &plan, summary, mid_run);
                Ok(CompactionOutcome::Compacted {
                    exchange,
                    plan,
                    report,
                })
            }
            Err(issues) => {
                // R2-F-04 修复：repair 载荷与 prior 草稿 turn 用**当轮原始
                // 文本**（现役 agent-run.ts:601 `createRepairInstruction(
                // validation.issues, rawText)`，repair 会话中的草稿消息亦
                // 为原始 assistant message）；sanitize 只服务 validate 与
                // 最终落库。
                let repair = compaction::build_repair_instruction(&issues, &draft);
                // 现役修复形状：模型看到自己的草稿（assistant 消息）再收
                // 修复指令——同一会话（含可能的 placeholder 恢复段）+ 草稿
                // + 修复指令。
                prior.push(ExchangeItem::AssistantTurn {
                    call: call_id.clone(),
                    content: vec![ContentBlock::Text {
                        text: draft.clone(),
                    }],
                    tool_calls: Vec::new(),
                    origin: None,
                });
                prior.push(ExchangeItem::CompactionInstruction { text: repair });
                let repair_call =
                    ModelCallId::new(format!("{}-compact-{compaction_seq}-repair", ctx.run_id));
                let repair_request = self.summary_request(input, max_output_tokens, prior);
                let repaired = self
                    .settle_summary_call(
                        ctx,
                        port,
                        &repair_call,
                        summary_operation,
                        &summary_label,
                        &repair_request,
                        cancel,
                        &ledger,
                    )
                    .await?;
                let Some(repaired) = repaired else {
                    return Ok(CompactionOutcome::Cancelled);
                };
                match repaired {
                    SummaryCallResult::Text { text } => {
                        let sanitized = compaction::sanitize_summary(&text);
                        match compaction::validate_summary(&sanitized.text, &sanitized.unmatched) {
                            Ok(()) => {
                                let summary = compaction::append_file_operation_context(
                                    &sanitized.text,
                                    &input.exchange[..plan.cut_index],
                                );
                                let exchange =
                                    compaction::apply_plan(input.exchange, &plan, summary, mid_run);
                                Ok(CompactionOutcome::Compacted {
                                    exchange,
                                    plan,
                                    report,
                                })
                            }
                            Err(issues) => {
                                let detail = format!(
                                    "the repaired summary still fails validation: {}",
                                    issues.join("; ")
                                );
                                tracing::warn!(run_id = %ctx.run_id, "compaction failed: {detail}");
                                Ok(CompactionOutcome::Failed { detail })
                            }
                        }
                    }
                    SummaryCallResult::ToolIntent { .. } => {
                        // 现役：首次之后的任何工具意图（providerRequests > 1）
                        // 即违规——修复调用上的工具意图响亮失败。
                        let detail = "tool intent appeared after the first placeholder \
                                      recovery turn; the run continues with the original \
                                      exchange"
                            .to_string();
                        tracing::warn!(run_id = %ctx.run_id, "compaction failed: {detail}");
                        Ok(CompactionOutcome::Failed { detail })
                    }
                    SummaryCallResult::Failed { detail } => {
                        tracing::warn!(run_id = %ctx.run_id, "compaction repair failed: {detail}");
                        Ok(CompactionOutcome::Failed { detail })
                    }
                }
            }
        }
    }

    /// 一次摘要请求的构造（缓存保留形状的唯一构造点）。
    ///
    /// 形状 = 系统槽 + submission + 会话 prior + live 工具快照 + 按族分流的
    /// 输出上限（None = 线上不发上限字段，Some = required-cap 族的回填值）。
    /// `max_output_tokens` 由 `compact_exchange` 按族分流决策后直接透传。
    fn summary_request(
        &self,
        input: &MidRunCompactionInput<'_>,
        max_output_tokens: Option<u32>,
        prior: Vec<ExchangeItem>,
    ) -> AuxiliaryRequest {
        AuxiliaryRequest {
            prompt: input.submission.to_string(),
            images: Vec::new(),
            max_output_tokens,
            deadline_unix_ms: None,
            system_prompt: input.system_prompt.map(str::to_string),
            prior,
            tools: Some(input.tools.clone()),
        }
    }

    /// 一次摘要模型调用：执行 + 台账（每种结算都落行——R05 RR1 F21）。
    /// `Ok(None)` = 取消获胜（台账行 outcome=Cancelled，attempts 未知）。
    /// R3-F-01（§十#17）：`operation`/`label` = 调用方在步骤 4 显式决策的
    /// 生效路由身份（summarize 槽显式覆盖 或 chat 回退），透传给
    /// `complete_step_for_operation`；执行器自身永不回退。
    #[allow(clippy::too_many_arguments)]
    async fn settle_summary_call<P: StoragePort>(
        &self,
        ctx: &RunContext,
        port: &P,
        call_id: &ModelCallId,
        operation: ModelOperation,
        label: &str,
        request: &AuxiliaryRequest,
        cancel: &CancelScope,
        ledger: &LedgerFacts<'_>,
    ) -> Result<Option<SummaryCallResult>, StorageError> {
        let started_at_unix_ms = self.clock.now_unix_ms();
        let call = self
            .auxiliary
            .complete_step_for_operation(ctx, operation, label, call_id, request);
        tokio::pin!(call);
        let settle = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                // R05 RR1 F38 镜像：被取消的调用可能已有物理请求离开进程
                // ——台账行 outcome=Cancelled，attempts 未知（None），usage
                // 未知；绝不假装知道被丢弃的 future 的事实。
                let settle = SummarySettle {
                    usage_report: ReportedUsage::Unknown,
                    transport_attempts: None,
                    served_by: ProviderDescriptor {
                        provider: "unreported".to_string(),
                        model: "unreported".to_string(),
                        operation: "auxiliary.summarize".to_string(),
                    },
                    served_protocol: None,
                    outcome: CallOutcome::Cancelled,
                    started_at_unix_ms,
                    emitted_tool_calls: Vec::new(),
                };
                self.record_summary_row(ctx, port, call_id, &settle, ledger)
                    .await?;
                return Ok(None);
            }
            outcome = &mut call => outcome,
        };
        let (result, settle) = match settle {
            Ok(AuxiliaryStep::Text(outcome)) => (
                SummaryCallResult::Text { text: outcome.text },
                SummarySettle {
                    usage_report: outcome.usage_report,
                    transport_attempts: Some(outcome.transport_attempts),
                    served_by: outcome.served_by,
                    served_protocol: outcome.served_protocol,
                    outcome: CallOutcome::Succeeded,
                    started_at_unix_ms,
                    emitted_tool_calls: Vec::new(),
                },
            ),
            // 工具意图 turn 是已结算的 provider 应答：台账如实记 Succeeded
            // （传输+解析都成功；意图是否可接受是 service 层的判定），并
            // 带上宿主铸造的占位调用身份（emitted_tool_calls 纪律与 chat
            // 行一致）。工具从不执行。
            Ok(AuxiliaryStep::ToolRequests {
                requests,
                content,
                usage_report,
                served_protocol,
                transport_attempts,
                served_by,
            }) => {
                let calls: Vec<RequestedToolCall> = requests
                    .into_iter()
                    .enumerate()
                    .map(|(index, request)| RequestedToolCall {
                        tool_call_id: ToolCallId::new(format!(
                            "{}-tc{:04}",
                            call_id.as_str(),
                            index + 1
                        )),
                        provider_call_id: request.provider_call_id,
                        target: request.target,
                        arguments: request.arguments,
                        args_digest: request.args_digest,
                        args_summary: request.args_summary,
                    })
                    .collect();
                let emitted = calls
                    .iter()
                    .map(|call| call.tool_call_id.as_str().to_string())
                    .collect();
                (
                    SummaryCallResult::ToolIntent { calls, content },
                    SummarySettle {
                        usage_report,
                        transport_attempts: Some(transport_attempts),
                        served_by,
                        served_protocol,
                        outcome: CallOutcome::Succeeded,
                        started_at_unix_ms,
                        emitted_tool_calls: emitted,
                    },
                )
            }
            Err(failure) => (
                SummaryCallResult::Failed {
                    detail: failure.message.clone(),
                },
                SummarySettle {
                    usage_report: failure.usage_report,
                    transport_attempts: Some(failure.transport_attempts),
                    served_by: failure.served_by,
                    served_protocol: failure.served_protocol,
                    outcome: CallOutcome::Failed,
                    started_at_unix_ms,
                    emitted_tool_calls: Vec::new(),
                },
            ),
        };
        self.record_summary_row(ctx, port, call_id, &settle, ledger)
            .await?;
        Ok(Some(result))
    }

    /// 摘要调用的台账行（`auxiliary.summarize` purpose；与 workermodel
    /// 的回调台账同一纪律：身份是 RESOLVED 路由，usage 未知就是未知）。
    async fn record_summary_row<P: StoragePort>(
        &self,
        ctx: &RunContext,
        port: &P,
        call_id: &ModelCallId,
        settle: &SummarySettle,
        ledger: &LedgerFacts<'_>,
    ) -> Result<(), StorageError> {
        let record = lingxi_kernel::usage::ModelCallUsageRecord {
            session_id: Some(ctx.session_id.to_string()),
            run_id: Some(ctx.run_id.to_string()),
            attempt: Some(ctx.attempt.to_string()),
            model_call_id: call_id.as_str().to_string(),
            purpose: format!("auxiliary.{}", AuxiliarySlot::Summarize.config_key()),
            origin: ledger.origin.to_string(),
            parent_run_id: ledger.parent_run_id.clone(),
            cause_ref: ledger.cause_ref.clone(),
            parent_tool_call_id: None,
            provider: settle.served_by.provider.clone(),
            model: settle.served_by.model.clone(),
            protocol: settle
                .served_protocol
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            usage: match settle.usage_report.clone() {
                ReportedUsage::Known(usage) => Some(usage),
                _ => None,
            },
            invalid_detail: match &settle.usage_report {
                ReportedUsage::Invalid { detail } => Some(detail.clone()),
                _ => None,
            },
            transport_attempts: settle.transport_attempts,
            outcome: settle.outcome,
            started_at_unix_ms: Some(settle.started_at_unix_ms),
            settled_at_unix_ms: Some(self.clock.now_unix_ms()),
            emitted_tool_calls: settle.emitted_tool_calls.clone(),
            cost_basis: None,
        };
        port.record_model_call_usage(record, self.clock.now_unix_ms())
            .await
    }
}

/// 台账的因果上下文（driver 的 UsageLedgerContext 字段透传）。
struct LedgerFacts<'a> {
    origin: &'static str,
    parent_run_id: &'a Option<String>,
    cause_ref: &'a Option<String>,
}

/// 一次摘要调用的结果（台账已在 `settle_summary_call` 落行）。
enum SummaryCallResult {
    Text {
        text: String,
    },
    /// 模型回答了工具意图（未执行任何调用）：现役 `tool_recovery` 的
    /// 输入。`calls` 携带宿主铸造的占位 ToolCallId（台账
    /// emitted_tool_calls 与交换项配对都靠它）。
    ToolIntent {
        calls: Vec<RequestedToolCall>,
        content: Vec<ContentBlock>,
    },
    Failed {
        detail: String,
    },
}

/// 族的 usage inclusion 口径（F-02 修复：R05 RR1 F23 消费者纪律——
/// included 分量绝不重复加计）。族无映射（非 chat 族）时分量字段本就不
/// 存在 → false/false 不改变任何总数。
fn usage_inclusion_of(family: ProtocolFamily) -> compaction::CacheInclusion {
    let mapping = lingxi_adapters::models::usage::mapping_of(family);
    compaction::CacheInclusion {
        cache_read_in_input: mapping
            .and_then(|mapping| mapping.cache_read_included_in_input)
            .unwrap_or(false),
        cache_write_in_input: mapping
            .and_then(|mapping| mapping.cache_write_included_in_input)
            .unwrap_or(false),
    }
}

/// 工具声明快照的估算分量（wire 名 + 描述 + schema 文本；同
/// `estimate_text_tokens` 口径）。
fn estimate_tool_declaration_tokens(tools: &ToolDeclarationSnapshot) -> u64 {
    tools
        .declarations
        .iter()
        .map(|declaration| {
            u64::from(estimate_text_tokens(&declaration.wire_name))
                + u64::from(estimate_text_tokens(&declaration.description))
                + u64::from(estimate_text_tokens(
                    &declaration.input_schema.schema.to_string(),
                ))
        })
        .sum()
}
