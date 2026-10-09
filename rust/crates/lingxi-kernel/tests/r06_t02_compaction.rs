//! R06-T02 kernel 测试：token 预算、压缩规划、摘要清洗/校验、指令模板——
//! 全部纯确定性（无 IO、无时钟、无网络）。
//!
//! 现役语义来源（逐字锚定，见 R06-T02 报告 §五）：
//! - 触发：`core/session-compaction-runtime.ts`（FORCE 线 80% / 动态 reserve
//!   = max(16384, ceil(window×20%)) / usage + 尾部工具结果估算）；
//! - 切点：`pi-agent-core` `findCutPoint`（回走累积 keepRecentTokens=20000，
//!   切点永不落在 toolResult）+ `core/session-compactor.ts`
//!   `completeToolTransactionTrimBoundaries`（配对缺失/重复 = unprovable）；
//! - 清洗/校验：`lib/llm/cache-preserving-compaction-agent-run.ts`
//!   `sanitizeSummary` / `validateSummary`（9 标题序列、一次 format_repair）；
//! - 摘要身份：`pi-agent-core` `messages.js`（compactionSummary → user 角色，
//!   前缀/后缀包装），绝不获得系统指令优先级。

use lingxi_kernel::compaction::*;
use lingxi_kernel::context::estimate_text_tokens;
use lingxi_kernel::model_exchange::{ExchangeItem, RequestedToolCall, TurnOrigin};
use lingxi_kernel::ports::ToolOutcome;
use lingxi_kernel::usage::ModelCallUsage;
use lingxi_protocol::{ContentBlock, ModelCallId, ToolCallId};

// ─── 夹具 ───

fn assistant_turn(seq: u32, text: &str, tool_calls: Vec<RequestedToolCall>) -> ExchangeItem {
    ExchangeItem::AssistantTurn {
        call: ModelCallId::new(format!("mc{seq:04}")),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        tool_calls,
        origin: Some(TurnOrigin {
            provider: "stub".to_string(),
            model: "stub-model".to_string(),
        }),
    }
}

fn reasoning_only_turn(seq: u32, text: &str) -> ExchangeItem {
    ExchangeItem::AssistantTurn {
        call: ModelCallId::new(format!("mc{seq:04}")),
        content: vec![ContentBlock::Reasoning {
            text: text.to_string(),
        }],
        tool_calls: Vec::new(),
        origin: None,
    }
}

fn requested_call(seq: u32, target: &str) -> RequestedToolCall {
    let request = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
        target,
        serde_json::json!({ "path": format!("/tmp/f{seq}.txt") }),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective arguments")
    .with_provider_call_id(format!("call_{seq:04}"));
    RequestedToolCall {
        tool_call_id: ToolCallId::new(format!("tc{seq:04}")),
        provider_call_id: request.provider_call_id.clone(),
        target: request.target.clone(),
        arguments: request.arguments.clone(),
        args_digest: request.args_digest.clone(),
        args_summary: None,
    }
}

fn tool_result(seq: u32, text: &str) -> ExchangeItem {
    ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new(format!("tc{seq:04}")),
        provider_call_id: Some(format!("call_{seq:04}")),
        outcome: ToolOutcome::Success {
            result: lingxi_kernel::ports::ToolSuccess {
                content: vec![ContentBlock::Text {
                    text: text.to_string(),
                }],
                resource_refs: Vec::new(),
                truncated: false,
                status: None,
                content_digest: format!("digest-{seq}"),
            },
        },
    }
}

/// 长会话夹具（kernel 侧）：`groups` 组 [assistant(带一个工具调用), toolResult]，
/// 每组正文约 `chunk` 字符。尾部追加一个未闭合（pending）工具调用当
/// `pending_tail` 为真。
fn long_exchange(groups: u32, chunk: usize, pending_tail: bool) -> Vec<ExchangeItem> {
    let mut exchange = Vec::new();
    for g in 0..groups {
        let seq = g * 2 + 1;
        let text = format!("assistant chunk {g} {}", "a".repeat(chunk));
        exchange.push(assistant_turn(
            seq,
            &text,
            vec![requested_call(seq, "read_file")],
        ));
        exchange.push(tool_result(
            seq,
            &format!("result {g} {}", "r".repeat(chunk)),
        ));
    }
    if pending_tail {
        let seq = groups * 2 + 1;
        exchange.push(assistant_turn(
            seq,
            "pending turn",
            vec![requested_call(seq, "read_file")],
        ));
    }
    exchange
}

/// 夹层长会话夹具：每组 = [assistant(带调用), assistant(无调用), toolResult]
/// ——owner 与 result 之间隔着另一个 assistant 组。drive_run 的 push 序使
/// owner-result 恒紧邻，但 fork/重试插入 turn（R06-T03）与恢复历史进交换
/// （R06-T04）将扩大交换来源：切点规划对夹层形状必须同样安全
/// （REVIEW-R06-T02-R1 F-01 的反例形状）。
fn sandwich_exchange(groups: u32, chunk: usize, seq_base: u32) -> Vec<ExchangeItem> {
    let mut exchange = Vec::new();
    for g in 0..groups {
        let seq = seq_base + g * 2;
        exchange.push(assistant_turn(
            seq,
            &format!("owner chunk {g} {}", "a".repeat(chunk)),
            vec![requested_call(seq, "read_file")],
        ));
        exchange.push(assistant_turn(
            seq + 1_000,
            &format!("filler chunk {g} {}", "b".repeat(chunk)),
            Vec::new(),
        ));
        exchange.push(tool_result(
            seq,
            &format!("result {g} {}", "r".repeat(chunk)),
        ));
    }
    exchange
}

/// 双向无孤儿断言：保留区每个 ToolResult 的归属 assistant 同在保留区；
/// 被摘要区每个 assistant 调用的结果不得落在保留区。
fn assert_plan_has_no_orphans(exchange: &[ExchangeItem], cut_index: usize) {
    let kept = &exchange[cut_index..];
    let summarized = &exchange[..cut_index];
    let kept_call_ids: std::collections::HashSet<&str> = kept
        .iter()
        .flat_map(|item| match item {
            ExchangeItem::AssistantTurn { tool_calls, .. } => tool_calls
                .iter()
                .map(|c| c.tool_call_id.as_str())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    for item in kept {
        if let ExchangeItem::ToolResult { tool_call_id, .. } = item {
            assert!(
                kept_call_ids.contains(tool_call_id.as_str()),
                "kept tool result {} must have its assistant turn kept",
                tool_call_id
            );
        }
    }
    let kept_result_ids: std::collections::HashSet<&str> = kept
        .iter()
        .filter_map(|item| match item {
            ExchangeItem::ToolResult { tool_call_id, .. } => Some(tool_call_id.as_str()),
            _ => None,
        })
        .collect();
    for item in summarized {
        if let ExchangeItem::AssistantTurn { tool_calls, .. } = item {
            for call in tool_calls {
                assert!(
                    !kept_result_ids.contains(call.tool_call_id.as_str()),
                    "summarized assistant call {} must not have its result kept",
                    call.tool_call_id
                );
            }
        }
    }
}

fn golden_summary() -> String {
    "## Goal\n完成用户请求\n\n## Constraints & Preferences\n- (none)\n\n\
     ## Progress\n### Done\n- [x] 读取文件\n\n### In Progress\n- [ ] 汇总\n\n\
     ### Blocked\n- 无\n\n## Key Decisions\n- **用真实工具**: 避免猜测\n\n\
     ## Next Steps\n1. 写出结论\n\n## Critical Context\n- (none)\n"
        .to_string()
}

// ─── reserve 与触发阈值 ───

#[test]
fn reserve_floor_and_proportional_ratio() {
    // 现役 computeCompactionReserveTokens = max(16384, ceil(window × 0.2))。
    assert_eq!(compute_reserve_tokens(0), 16_384);
    assert_eq!(compute_reserve_tokens(8_192), 16_384);
    assert_eq!(compute_reserve_tokens(81_920), 16_384); // 0.2×81920=16384 恰等
    assert_eq!(compute_reserve_tokens(100_000), 20_000);
    assert_eq!(compute_reserve_tokens(131_072), 26_215); // ceil(26214.4)
    assert_eq!(compute_reserve_tokens(1_000_000), 200_000);
}

#[test]
fn undeclared_or_zero_window_is_unavailable() {
    assert!(ContextBudget::for_declared_window(None).is_none());
    assert!(ContextBudget::for_declared_window(Some(0)).is_none());
}

#[test]
fn unknown_usage_never_triggers() {
    // 现役门槛：`if (!message.usage) return false;` — 没有真实 usage 信号
    // 时不凭估算触发。
    let budget = ContextBudget::for_declared_window(Some(100_000)).expect("window");
    let facts = TurnBudgetFacts {
        system_tokens: 2_000,
        submission_tokens: 100,
        tool_declaration_tokens: 500,
        history_tokens: 900_000, // 估算再大也不凭它触发
        tail_tokens: 0,
        last_usage: None,
        usage_inclusion: CacheInclusion::default(),
    };
    assert!(matches!(
        budget.evaluate(&facts),
        CompactionDecision::Unavailable { .. }
    ));
}

#[test]
fn zero_usage_never_triggers() {
    // 现役 `if (!usageTokens) return false;`
    let budget = ContextBudget::for_declared_window(Some(100_000)).expect("window");
    let facts = TurnBudgetFacts {
        system_tokens: 0,
        submission_tokens: 0,
        tool_declaration_tokens: 0,
        history_tokens: 0,
        tail_tokens: 0,
        last_usage: Some(ModelCallUsage::reported(0, 0)),
        usage_inclusion: CacheInclusion::default(),
    };
    assert!(matches!(
        budget.evaluate(&facts),
        CompactionDecision::Unavailable { .. }
    ));
}

#[test]
fn below_threshold_is_silent() {
    let budget = ContextBudget::for_declared_window(Some(100_000)).expect("window");
    let facts = TurnBudgetFacts {
        system_tokens: 1_000,
        submission_tokens: 100,
        tool_declaration_tokens: 200,
        history_tokens: 40_000,
        tail_tokens: 0,
        // 40% 窗口：ASK 线（50%）以下，绝不触发。
        last_usage: Some(ModelCallUsage::reported(39_900, 100)),
        usage_inclusion: CacheInclusion::default(),
    };
    match budget.evaluate(&facts) {
        CompactionDecision::BelowThreshold { report } => {
            assert_eq!(report.context_tokens, 40_000);
            assert_eq!(report.context_window, 100_000);
        }
        other => panic!("below line must not trigger: {other:?}"),
    }
}

#[test]
fn force_line_at_eighty_percent_of_large_window() {
    // 大窗口（reserve 占比 20% 与 FORCE 线一致）：ratio ≥ 0.8 即触发。
    let budget = ContextBudget::for_declared_window(Some(1_000_000)).expect("window");
    let facts = TurnBudgetFacts {
        system_tokens: 1_000,
        submission_tokens: 100,
        tool_declaration_tokens: 200,
        history_tokens: 700_000,
        tail_tokens: 0,
        last_usage: Some(ModelCallUsage::reported(799_000, 1_000)),
        usage_inclusion: CacheInclusion::default(),
    };
    assert!(matches!(
        budget.evaluate(&facts),
        CompactionDecision::Force { .. }
    ));
}

#[test]
fn reserve_line_fires_before_eighty_percent_on_small_windows() {
    // 现役双条件：`ratio >= 0.8 || contextTokens > window - reserve`。
    // 窗口 32768：reserve = 16384（地板），触发线 = 16384。
    // 20000 tokens：ratio≈0.61 < 0.8，但 20000 > 16384 → 触发。
    let budget = ContextBudget::for_declared_window(Some(32_768)).expect("window");
    let facts = TurnBudgetFacts {
        system_tokens: 500,
        submission_tokens: 50,
        tool_declaration_tokens: 100,
        history_tokens: 19_000,
        tail_tokens: 0,
        last_usage: Some(ModelCallUsage::reported(19_900, 100)),
        usage_inclusion: CacheInclusion::default(),
    };
    assert!(matches!(
        budget.evaluate(&facts),
        CompactionDecision::Force { .. }
    ));
}

#[test]
fn tail_tool_results_since_last_usage_count_toward_trigger() {
    // 现役：usage 先于本轮工具结果产生，尾部工具结果按估算计入。
    let budget = ContextBudget::for_declared_window(Some(100_000)).expect("window");
    let base = TurnBudgetFacts {
        system_tokens: 1_000,
        submission_tokens: 100,
        tool_declaration_tokens: 200,
        history_tokens: 70_000,
        tail_tokens: 0,
        // 70%：无尾部时不触发（reserve 线 = 80000）。
        last_usage: Some(ModelCallUsage::reported(69_900, 100)),
        usage_inclusion: CacheInclusion::default(),
    };
    assert!(matches!(
        budget.evaluate(&base),
        CompactionDecision::BelowThreshold { .. }
    ));
    let with_tail = TurnBudgetFacts {
        tail_tokens: 10_500, // 一波大工具输出：70000+10500=80500 > 80000
        ..base.clone()
    };
    assert!(matches!(
        budget.evaluate(&with_tail),
        CompactionDecision::Force { .. }
    ));
}

#[test]
fn budget_report_accounts_every_component() {
    // 任务书步骤②：预算为系统约束/当前请求/工具定义/历史/输出留空间，
    // 各分量在报告里可查（估算口径见报告 §五）。
    let budget = ContextBudget::for_declared_window(Some(100_000)).expect("window");
    let facts = TurnBudgetFacts {
        system_tokens: 2_000,
        submission_tokens: 300,
        tool_declaration_tokens: 700,
        history_tokens: 40_000,
        tail_tokens: 1_000,
        last_usage: Some(ModelCallUsage::reported(44_000, 1_000)),
        usage_inclusion: CacheInclusion::default(),
    };
    let CompactionDecision::BelowThreshold { report } = budget.evaluate(&facts) else {
        panic!("must be below threshold");
    };
    assert_eq!(report.system_tokens, 2_000);
    assert_eq!(report.submission_tokens, 300);
    assert_eq!(report.tool_declaration_tokens, 700);
    assert_eq!(report.history_estimate_tokens, 40_000);
    assert_eq!(report.tail_estimate_tokens, 1_000);
    assert_eq!(report.output_reserve_tokens, 20_000); // max(16384, 20%×100000)
    assert_eq!(report.usage_context_tokens, Some(45_000)); // 44000+1000
    assert_eq!(report.context_tokens, 46_000); // usage 总量 + 尾部估算
    assert_eq!(report.context_window, 100_000);
    assert!(report.ratio_bp > 0);
}

#[test]
fn cache_components_count_toward_usage_total() {
    // 现役 calculateContextTokens = input+output+cacheRead+cacheWrite，
    // 其中现役 input 是**扣过 cache 分量**的口径（pi-ai 规范化：
    // input = prompt_tokens − cacheRead − cacheWrite）。R05 统一结构的
    // input_tokens 是 wire 总量——included 族（OpenAI×3/Google）须先还原
    // input 半量再相加，否则 cache_read 双计（R05 RR1 F23 消费者纪律：
    // 旗标说 included 的分量绝不重复加计）。
    //
    // OpenAI 族形状：wire prompt=100_000（含 cached 30_000）、
    // completion=500 → 现役等价 = (100000−30000) + 500 + 30000 = 100_500。
    let openai_wire = ModelCallUsage {
        input_tokens: Some(100_000),
        output_tokens: Some(500),
        cache_read_tokens: Some(30_000),
        cache_write_tokens: None,
        reasoning_tokens: None,
        provenance: lingxi_kernel::usage::UsageProvenance::Reported,
    };
    let included = CacheInclusion {
        cache_read_in_input: true,
        cache_write_in_input: false,
    };
    assert_eq!(
        context_tokens_from_usage(&openai_wire, included),
        Some(100_500),
        "included cache_read counts exactly once"
    );
    // Anthropic 族形状：cache 分量是独立类别（included=false）→ 四分量
    // 全加：10 + 4 + 2000 + 1000 = 3014。
    let anthropic_wire = ModelCallUsage {
        input_tokens: Some(10),
        output_tokens: Some(4),
        cache_read_tokens: Some(2_000),
        cache_write_tokens: Some(1_000),
        reasoning_tokens: None,
        provenance: lingxi_kernel::usage::UsageProvenance::Reported,
    };
    assert_eq!(
        context_tokens_from_usage(&anthropic_wire, CacheInclusion::default()),
        Some(3_014),
        "separate cache categories all add"
    );
    // 全部分量缺失 = None（没有可用 usage 事实，绝非零）。
    assert_eq!(
        context_tokens_from_usage(
            &ModelCallUsage {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                provenance: lingxi_kernel::usage::UsageProvenance::Reported,
            },
            CacheInclusion::default()
        ),
        None
    );
}

#[test]
fn cache_write_inclusion_is_arbitrated_the_same_way() {
    // cache_write included 的假设族（现役五族均为 false——该维度是旗标
    // 驱动的对称纪律，不是为某族特设）：input=50_000 含 cache_write
    // 5_000 → (50000−5000) + 500 + 5000 = 50_500。
    let usage = ModelCallUsage {
        input_tokens: Some(50_000),
        output_tokens: Some(500),
        cache_read_tokens: None,
        cache_write_tokens: Some(5_000),
        reasoning_tokens: None,
        provenance: lingxi_kernel::usage::UsageProvenance::Reported,
    };
    let included = CacheInclusion {
        cache_read_in_input: false,
        cache_write_in_input: true,
    };
    assert_eq!(context_tokens_from_usage(&usage, included), Some(50_500));
    // 同一 usage 在独立口径（false）下全加：55_500。
    assert_eq!(
        context_tokens_from_usage(&usage, CacheInclusion::default()),
        Some(55_500)
    );
}

#[test]
fn budget_judgement_uses_the_family_arbitrated_total() {
    // F-02 服务链等价形状：window=100_000、reserve=20_000 → 触发线
    // 80_000。OpenAI 族 wire usage input=79_000（含 cache_read 30_000）、
    // output=100：现役等价 79_100 < 80_000 → 不触发；双计口径 109_100
    // 会误触发。
    let budget = ContextBudget::for_declared_window(Some(100_000)).expect("window");
    let usage = ModelCallUsage {
        input_tokens: Some(79_000),
        output_tokens: Some(100),
        cache_read_tokens: Some(30_000),
        cache_write_tokens: None,
        reasoning_tokens: None,
        provenance: lingxi_kernel::usage::UsageProvenance::Reported,
    };
    let facts = TurnBudgetFacts {
        system_tokens: 0,
        submission_tokens: 0,
        tool_declaration_tokens: 0,
        history_tokens: 0,
        tail_tokens: 0,
        last_usage: Some(usage.clone()),
        usage_inclusion: CacheInclusion {
            cache_read_in_input: true,
            cache_write_in_input: false,
        },
    };
    match budget.evaluate(&facts) {
        CompactionDecision::BelowThreshold { report } => {
            assert_eq!(report.usage_context_tokens, Some(79_100));
        }
        other => panic!("family-arbitrated total stays below the line: {other:?}"),
    }
    // 对照：同一 wire usage 若按独立口径（Anthropic 语义）则四分量全加
    // = 109_100 > 80_000 → 触发。族口径是判定的输入，不是全局常量。
    let facts_separate = TurnBudgetFacts {
        usage_inclusion: CacheInclusion::default(),
        ..facts
    };
    assert!(matches!(
        budget.evaluate(&facts_separate),
        CompactionDecision::Force { .. }
    ));
}

// ─── 切点规划（工具对完整性 + pending 保护） ───

#[test]
fn planner_cut_respects_keep_recent_budget() {
    // Rust 回退方向（保留区恒 ≥keep_recent；现役前跳可 <keepRecent）——§十#11。
    // 每组约 2×chunk 字符 ≈ chunk/2 token；keep_recent 取 3 组规模。
    let exchange = long_exchange(10, 4_000, false);
    let group_tokens =
        estimate_exchange_item_tokens(&exchange[0]) + estimate_exchange_item_tokens(&exchange[1]);
    let keep_recent = group_tokens * 3;
    let plan = plan_compaction(&exchange, keep_recent)
        .expect("pairs provable")
        .expect("a beneficial cut exists");
    // 保留区从某组起点开始，保留约 3 组（切点只落在组边界）。
    assert_eq!(plan.cut_index % 2, 0, "cut must land on a group boundary");
    let retained = exchange.len() - plan.cut_index;
    assert!(retained >= 6, "at least three groups retained: {retained}");
    assert!(retained <= 8, "not much more than three groups: {retained}");
    assert!(plan.cut_index > 0, "old region non-empty");
    assert_eq!(
        plan.summarized_tokens,
        estimate_exchange_tokens(&exchange[..plan.cut_index])
    );
}

#[test]
fn planner_cut_never_lands_on_a_tool_result() {
    // 现役 findCutPoint：合法切点集合不含 toolResult。
    let exchange = long_exchange(8, 8_000, false);
    let plan = plan_compaction(&exchange, 1) // keep_recent=1 → 尽量多切
        .expect("pairs provable")
        .expect("a cut exists");
    assert!(
        !matches!(exchange[plan.cut_index], ExchangeItem::ToolResult { .. }),
        "cut point must never be a tool result"
    );
}

#[test]
fn planner_keep_recent_one_keeps_only_the_last_group() {
    // keep_recent=1：切点尽可能靠后，但仍须落在组边界（最后一组完整保留）。
    let exchange = long_exchange(6, 1_000, false);
    let plan = plan_compaction(&exchange, 1)
        .expect("provable")
        .expect("cut exists");
    assert_eq!(plan.cut_index, exchange.len() - 2);
}

#[test]
fn planner_pending_tail_is_always_retained() {
    // R06-A03 前置形状：长历史尾部含未闭合工具调用。切点必须落在 pending
    // 组之前—— pending 调用逐字保留，其未来结果永不孤立。
    let exchange = long_exchange(8, 8_000, true);
    let pending_index = exchange.len() - 1;
    let plan = plan_compaction(&exchange, 1)
        .expect("provable")
        .expect("cut exists");
    assert!(
        plan.cut_index <= pending_index,
        "cut must keep the pending turn whole: cut={} pending={}",
        plan.cut_index,
        pending_index
    );
    // pending 组（assistant+其调用）必须整体落在保留区。
    let retained = &exchange[plan.cut_index..];
    let pending_kept = retained.iter().any(|item| {
        matches!(item, ExchangeItem::AssistantTurn { tool_calls, .. } if !tool_calls.is_empty())
            && matches!(item, ExchangeItem::AssistantTurn { call, .. } if call.as_str() == "mc0017")
    });
    assert!(pending_kept, "pending assistant turn must stay verbatim");
}

#[test]
fn planner_never_orphans_in_either_direction() {
    // 压缩后：保留区不得有没有归属 assistant 的 toolResult（孤立 result），
    // 被摘要区不得有结果落在保留区的 assistant（孤立调用）。两种夹具 ×
    // 多档 keep_recent：owner-result 恒紧邻形状与夹层形状（owner 与
    // result 之间隔着另一个 assistant 组——REVIEW-R06-T02-R1 F-01 的
    // 反例形状，旧实现只按项类型回退切点，会把 owner 压进摘要区而把
    // 其 result 留在保留区）。无可切（None）是合法答案：宁可不压，
    // 绝不制造孤儿。
    for exchange in [
        long_exchange(10, 4_000, false),
        sandwich_exchange(8, 4_000, 1),
    ] {
        for keep_recent in [1, 2_000, 6_000, 12_000, 20_000] {
            if let Some(plan) = plan_compaction(&exchange, keep_recent).expect("provable") {
                assert_plan_has_no_orphans(&exchange, plan.cut_index);
            }
        }
    }
}

#[test]
fn planner_sandwich_shape_refuses_the_orphaning_boundary() {
    // REVIEW-R06-T02-R1 探针 1 的精确形状：[A0(tc1), A1(无调用), R(tc1),
    // A2(tc2), R(tc2)]，keep_recent 恰好让累积在 R(tc1) 处跨界。旧实现
    // 回退到 A1（cut=1）→ 保留区携带孤儿 R(tc1)。修复后 cut=1 被孤儿
    // 校验否决，更早无合法边界 → Ok(None)（宁可不压）。
    let big = "a".repeat(4_000);
    let exchange = vec![
        assistant_turn(1, &big, vec![requested_call(1, "read_file")]),
        assistant_turn(2, &big, Vec::new()),
        tool_result(1, &big),
        assistant_turn(3, "tail", vec![requested_call(3, "read_file")]),
        tool_result(3, "tail"),
    ];
    assert_eq!(
        plan_compaction(&exchange, 2_000).expect("provable"),
        None,
        "the only reachable boundary would orphan tc0001 — no cut is the safe answer"
    );
}

#[test]
fn planner_falls_back_to_a_pair_whole_boundary_when_one_exists() {
    // 夹层形状 + 更长尾部：孤儿化边界被否决后应继续回退到更早的安全
    // 边界（owner 与 result 同区），而非放弃压缩。
    // 三组夹层 [owner, filler, result] ×3，每项 ≈1000 tokens；
    // keep_recent=3500 → 跨界在组 1 的 result（索引 5），候选 filler
    // （索引 4）因孤儿化被否决，回退到组 1 起点（索引 3）：保留区
    // [owner(tc0003), filler, R(tc0003), owner(tc0005), filler, R(tc0005)]
    // 配对完整。
    let exchange = sandwich_exchange(3, 4_000, 1);
    let plan = plan_compaction(&exchange, 3_500)
        .expect("provable")
        .expect("a pair-whole boundary exists");
    assert_eq!(
        plan.cut_index, 3,
        "the cut lands on the first pair-whole group boundary"
    );
    assert_plan_has_no_orphans(&exchange, plan.cut_index);
}

#[test]
fn a_compacted_exchange_stays_provable_on_the_next_pass() {
    // REVIEW 探针 3 的毒化级联：旧实现的孤儿化产物再次进入
    // plan_compaction 时配对证明必失败（该会话此后永不压缩）。修复后
    // 安全压缩产物再次规划：可 None 可 Some，绝不因孤儿而不可证明。
    let big = "a".repeat(4_000);
    let mut exchange = vec![
        assistant_turn(1, &big, vec![requested_call(1, "read_file")]),
        assistant_turn(2, &big, Vec::new()),
        tool_result(1, &big),
    ];
    // seq_base=11：与手工头部的 tc0001 错开（配对证明拒绝重复 id）。
    exchange.extend(sandwich_exchange(4, 4_000, 11));
    let plan = plan_compaction(&exchange, 2_500)
        .expect("provable")
        .expect("a safe cut exists in the long tail");
    assert_plan_has_no_orphans(&exchange, plan.cut_index);
    let mut second = apply_plan(&exchange, &plan, golden_summary(), true);
    // 追加更多历史使第二次压缩有可切空间。
    second.push(assistant_turn(
        41,
        &big,
        vec![requested_call(41, "read_file")],
    ));
    second.push(tool_result(41, &big));
    second.push(assistant_turn(
        43,
        &big,
        vec![requested_call(43, "read_file")],
    ));
    second.push(tool_result(43, &big));
    if let Some(plan2) =
        plan_compaction(&second, 1).expect("the compacted product must stay provable")
    {
        assert_plan_has_no_orphans(&second, plan2.cut_index);
    }
}

#[test]
fn planner_unprovable_pairs_fail_loudly() {
    // 结果找不到归属调用 → unprovable（现役
    // completeToolTransactionTrimBoundaries 的 missing id 分支）。
    let mut exchange = long_exchange(4, 1_000, false);
    exchange.push(ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new("tc9999"),
        provider_call_id: Some("call_9999".to_string()),
        outcome: ToolOutcome::Cancelled,
    });
    assert!(matches!(
        plan_compaction(&exchange, 1_000),
        Err(CompactionPlanError::UnprovableToolPairs { .. })
    ));
    // 同一调用的重复结果 → unprovable。
    let mut exchange = long_exchange(4, 1_000, false);
    exchange.insert(2, tool_result(1, "duplicate"));
    assert!(matches!(
        plan_compaction(&exchange, 1_000),
        Err(CompactionPlanError::UnprovableToolPairs { .. })
    ));
}

#[test]
fn planner_no_beneficial_cut_returns_none() {
    // 全部历史装得进 keep_recent → 无可切（不白调摘要模型）。
    let exchange = long_exchange(2, 200, false);
    assert_eq!(
        plan_compaction(&exchange, 1_000_000).expect("provable"),
        None
    );
    // 空交换同样无可切。
    assert_eq!(plan_compaction(&[], 1_000).expect("provable"), None);
}

#[test]
fn planner_previous_summary_is_a_valid_cut_boundary() {
    // 二次压缩：交换以 CompactionSummary 开头时，切点可以落在它之后；
    // 摘要项本身计入估算。
    let mut exchange = vec![ExchangeItem::CompactionSummary {
        summary: golden_summary(),
        covered_items: 10,
        mid_run: true,
    }];
    exchange.extend(long_exchange(8, 4_000, false));
    let plan = plan_compaction(&exchange, 1)
        .expect("provable")
        .expect("cut exists");
    assert!(plan.cut_index >= 1, "never cuts before the leading summary");
    assert!(matches!(
        exchange[plan.cut_index],
        ExchangeItem::AssistantTurn { .. }
    ));
}

// ─── 应用计划 ───

#[test]
fn apply_plan_replaces_old_region_with_one_summary_item() {
    let exchange = long_exchange(10, 4_000, false);
    let plan = plan_compaction(&exchange, 2_000)
        .expect("provable")
        .expect("cut exists");
    let retained_before: Vec<ExchangeItem> = exchange[plan.cut_index..].to_vec();
    let projected = apply_plan(&exchange, &plan, golden_summary(), true);
    assert_eq!(projected.len(), 1 + retained_before.len());
    match &projected[0] {
        ExchangeItem::CompactionSummary {
            summary,
            covered_items,
            mid_run,
        } => {
            assert_eq!(summary, &golden_summary());
            assert_eq!(*covered_items as usize, plan.cut_index);
            assert!(mid_run);
        }
        other => panic!("first item must be the summary: {other:?}"),
    }
    assert_eq!(
        &projected[1..],
        retained_before.as_slice(),
        "retained tail must stay byte-identical"
    );
}

#[test]
fn apply_plan_marks_the_notice_by_trigger_source() {
    // REVIEW-R06-T02-R1 F-04：摘要项的 mid_run 标记来自触发源——
    // 自动（run 内）压缩携带 MIDRUN notice（现役 runtime 行为）；手动
    // /compact 不带（现役 compactSession 不追加）。同一计划两种触发源
    // 产出不同的标记，其余部分逐字一致。
    let exchange = long_exchange(10, 4_000, false);
    let plan = plan_compaction(&exchange, 2_000)
        .expect("provable")
        .expect("cut exists");
    let automatic = apply_plan(&exchange, &plan, golden_summary(), true);
    let manual = apply_plan(&exchange, &plan, golden_summary(), false);
    let mid_run_of = |projected: &[ExchangeItem]| match &projected[0] {
        ExchangeItem::CompactionSummary { mid_run, .. } => *mid_run,
        other => panic!("first item must be the summary: {other:?}"),
    };
    assert!(
        mid_run_of(&automatic),
        "mid-run compaction carries the notice"
    );
    assert!(
        !mid_run_of(&manual),
        "a manual /compact product carries NO mid-run notice"
    );
    assert_eq!(
        automatic[1..],
        manual[1..],
        "the retained tail is identical regardless of the trigger"
    );
}

// ─── 清洗与校验 ───

#[test]
fn sanitize_strips_closed_narration_blocks() {
    let raw = format!(
        "{}<mood>开心</mood>\n<pulse intensity=\"high\">跳动</pulse>\n```reflect\n自省\n```\n尾部",
        golden_summary()
    );
    let clean = sanitize_summary(&raw);
    assert!(!clean.text.contains("<mood>"));
    assert!(!clean.text.contains("跳动"));
    assert!(!clean.text.contains("自省"));
    assert!(clean.text.contains("## Goal"));
    assert!(clean.text.ends_with("尾部"));
    assert!(clean.removed.contains(&"mood"));
    assert!(clean.removed.contains(&"pulse"));
    assert!(clean.removed.contains(&"reflect"));
    assert!(clean.unmatched.is_empty());
    validate_summary(&clean.text, &clean.unmatched).expect("golden summary stays valid");
}

#[test]
fn sanitize_flags_unmatched_narration_tags() {
    let raw = format!("{}<mood>未闭合的心情块", golden_summary());
    let clean = sanitize_summary(&raw);
    assert!(
        clean.unmatched.contains(&"mood"),
        "unclosed mood tag must be flagged, not silently kept"
    );
    let issues =
        validate_summary(&clean.text, &clean.unmatched).expect_err("unmatched tags invalidate");
    assert!(issues.iter().any(|issue| issue.contains("mood")));
}

#[test]
fn sanitize_collapses_blank_line_runs() {
    let clean = sanitize_summary("第一段\n\n\n\n\n第二段");
    assert_eq!(clean.text, "第一段\n\n第二段");
}

#[test]
fn validate_accepts_the_golden_summary() {
    validate_summary(&golden_summary(), &[]).expect("golden summary is valid");
}

#[test]
fn validate_rejects_empty_summary() {
    // R06-A04：空摘要永不覆盖历史（校验层兜底，AuxiliaryExecutor 另有
    // 空回答响亮失败）。
    let issues = validate_summary("   ", &[]).expect_err("empty is invalid");
    assert!(issues.iter().any(|issue| issue.contains("empty")));
}

#[test]
fn validate_rejects_missing_and_misordered_headings() {
    // 少一个标题。
    let missing = golden_summary().replace("## Next Steps\n1. 写出结论\n\n", "");
    let issues = validate_summary(&missing, &[]).expect_err("missing heading is invalid");
    assert!(issues.iter().any(|issue| issue.contains("9 structured")));
    // 顺序错。
    let swapped = golden_summary()
        .replace("## Goal", "## Key Decisions")
        .replacen(
            "## Key Decisions\n- **用真实工具**: 避免猜测",
            "## Goal\n完成用户请求",
            1,
        );
    let issues = validate_summary(&swapped, &[]).expect_err("misordered is invalid");
    assert!(issues
        .iter()
        .any(|issue| issue.contains("heading 1 must be")));
}

// ─── 指令模板 ───

#[test]
fn instruction_carries_incumbent_format_and_no_tool_rule() {
    // 边界按交换项数表述（现役按消息数，语义等价）——§十#1。
    let instruction = build_summary_instruction(&SummaryInstructionSpec {
        old_region_items: 12,
        split_turn: false,
        custom_focus: None,
    });
    // 现役 scopeLines 逐字锚点。
    assert!(instruction.contains("Internal compaction-only run."));
    assert!(instruction.contains("Do not call tools. Do not address the user."));
    assert!(instruction.contains("Do not output <mood>, <pulse>, <reflect>"));
    assert!(instruction.contains("Return only the exact structured checkpoint format below."));
    assert!(instruction
        .contains("Use recent-tail content only to understand continuity; never restate it"));
    assert!(instruction.contains("incorporate it from that position without duplicating"));
    // 边界以交换项数声明（线上消息索引不可在渲染前确知——报告 §五差异）。
    assert!(instruction.contains("12"));
    assert!(instruction.contains("Old region:"));
    assert!(instruction.contains("Retained boundary:"));
    // 结构化格式模板逐字（9 标题）。
    for heading in [
        "## Goal",
        "## Constraints & Preferences",
        "## Progress",
        "### Done",
        "### In Progress",
        "### Blocked",
        "## Key Decisions",
        "## Next Steps",
        "## Critical Context",
    ] {
        assert!(instruction.contains(heading), "instruction needs {heading}");
    }
    assert!(instruction.contains("Preserve exact file paths, function names, and error messages."));
    let custom = build_summary_instruction(&SummaryInstructionSpec {
        old_region_items: 3,
        split_turn: false,
        custom_focus: Some("关注文件修改".to_string()),
    });
    assert!(custom.contains("Additional focus for the checkpoint only: 关注文件修改"));
}

#[test]
fn repair_instruction_names_issues_and_carries_draft() {
    let instruction = build_repair_instruction(
        &["heading 1 must be \"## Goal\"".to_string()],
        "DRAFT-CONTENT",
    );
    assert!(instruction.contains("Internal compaction summary repair."));
    assert!(instruction.contains("Do not call tools. Do not address the user."));
    assert!(instruction.contains("heading 1 must be \"## Goal\""));
    assert!(instruction.contains("<draft-summary>\nDRAFT-CONTENT\n</draft-summary>"));
    for heading in [
        "## Goal",
        "## Constraints & Preferences",
        "## Progress",
        "### Done",
        "### In Progress",
        "### Blocked",
        "## Key Decisions",
        "## Next Steps",
        "## Critical Context",
    ] {
        assert!(instruction.contains(heading), "repair needs {heading}");
    }
}

// ─── 摘要输出上限 ───

#[test]
fn summary_output_cap_follows_the_incumbent_reserve_formula() {
    // 现役 getCachePreservingCompactionMaxTokens = max(512, floor(0.8×reserve))。
    // 公式只服务预算估算/BOUNDED/required 族兜底，非线体默认——§十#5。
    assert_eq!(summary_output_cap(20_000), 16_000);
    assert_eq!(summary_output_cap(16_384), 13_107); // floor(16384×0.8)
    assert_eq!(summary_output_cap(1_000), 800);
    assert_eq!(summary_output_cap(100), 512); // 512 下限
}

// ─── 估算口径 ───

#[test]
fn exchange_estimate_counts_blocks_calls_and_results() {
    // Rust 估算器对齐 hana CJK×1.1（现役切点/尾部链为 pi-sdk 纯 chars/4，
    // 方向保守）——§十#10。
    let item = assistant_turn(1, "abcd", vec![requested_call(1, "read_file")]);
    let tokens = estimate_exchange_item_tokens(&item);
    // 文本 4 字符→1 token；调用名 9 字符 + 参数 JSON 约 24 字符 → ~9 token。
    assert!((8..=20).contains(&tokens), "estimate sane: {tokens}");
    let result = tool_result(1, &"x".repeat(400));
    assert_eq!(estimate_exchange_item_tokens(&result), 100);
    // CJK 加权（与 T01 估算同源：CJK ×1.1；浮点 100×1.1=110.000…01 上取整为 111）。
    let cjk = tool_result(1, &"档".repeat(100));
    assert_eq!(estimate_exchange_item_tokens(&cjk), 111);
    // 摘要项 = 正文 + 现役前缀/后缀包装（线上渲染后包装同样占 token，
    // 与 pi estimateTokens 对转换后整条消息估算的口径一致）。
    let summary_item = ExchangeItem::CompactionSummary {
        summary: "s".repeat(400),
        covered_items: 3,
        mid_run: false,
    };
    let expected_summary_tokens = 100
        + u64::from(estimate_text_tokens(COMPACTION_SUMMARY_PREFIX))
        + u64::from(estimate_text_tokens(COMPACTION_SUMMARY_SUFFIX));
    assert_eq!(
        estimate_exchange_item_tokens(&summary_item),
        expected_summary_tokens
    );
    let reasoning = reasoning_only_turn(9, &"r".repeat(40));
    assert_eq!(estimate_exchange_item_tokens(&reasoning), 10);
}

// ─── 摘要身份的协议常量 ───

#[test]
fn summary_wrapper_and_notice_are_the_incumbent_texts() {
    assert!(COMPACTION_SUMMARY_PREFIX.starts_with(
        "The conversation history before this point was compacted into the following summary:"
    ));
    assert!(COMPACTION_SUMMARY_PREFIX.ends_with("<summary>\n"));
    assert_eq!(COMPACTION_SUMMARY_SUFFIX, "\n</summary>");
    assert!(MIDRUN_COMPACTION_NOTICE.starts_with("[System compaction notice — not a user message]"));
    assert!(MIDRUN_COMPACTION_NOTICE.contains("You are still mid-task."));
}

// ─── FIX-01（R2-F-01 + N-8）：输出上限的族分流（现役 output-budget.ts
// OUTPUT_CAP_CAPABILITIES 清单 + safeRequiredOutputCap） ───

#[test]
fn output_cap_required_matches_the_incumbent_capability_list() {
    use lingxi_kernel::model_exchange::ProtocolFamily as F;
    // (family, provider, endpoint, declared, expected)——行序即现役清单
    // 判定序（explicit-required → official-deepseek → anthropic-native →
    // bedrock-native → anthropic-messages → default-optional）。
    let matrix: &[(F, &str, &str, Option<bool>, bool)] = &[
        // explicit-required：声明 true 即必需（任何族/provider）。
        (
            F::OpenAiCompletions,
            "acme",
            "https://acme.example/v1",
            Some(true),
            true,
        ),
        (
            F::GoogleGenerativeAi,
            "google",
            "https://g.example",
            Some(true),
            true,
        ),
        // official-deepseek：provider 或官方端点 → optional（即使族是
        // anthropic-messages——deepseek 臂先于族臂）。
        (
            F::OpenAiCompletions,
            "deepseek",
            "https://api.deepseek.com/v1",
            None,
            false,
        ),
        (
            F::OpenAiCompletions,
            "acme",
            "https://api.deepseek.com",
            None,
            false,
        ),
        (
            F::AnthropicMessages,
            "deepseek",
            "https://api.deepseek.com/anthropic",
            None,
            false,
        ),
        // anthropic-native：provider 或官方端点 → required。
        (
            F::OpenAiCompletions,
            "anthropic",
            "https://api.anthropic.com/v1",
            None,
            true,
        ),
        (
            F::OpenAiCompletions,
            "acme",
            "https://api.anthropic.com",
            None,
            true,
        ),
        // bedrock-native。
        (
            F::OpenAiCompletions,
            "amazon-bedrock",
            "https://br.example",
            None,
            true,
        ),
        (
            F::OpenAiCompletions,
            "bedrock",
            "https://br.example",
            None,
            true,
        ),
        // anthropic-messages 族（任意 provider/端点）→ required。
        (
            F::AnthropicMessages,
            "acme",
            "https://acme.example",
            None,
            true,
        ),
        // default-optional：openai×3 / google / 其他。
        (
            F::OpenAiCompletions,
            "openai",
            "https://api.openai.com/v1",
            None,
            false,
        ),
        (
            F::OpenAiResponses,
            "openai",
            "https://api.openai.com/v1",
            None,
            false,
        ),
        (
            F::OpenAiCodexResponses,
            "openai",
            "https://chatgpt.com/backend-api",
            None,
            false,
        ),
        (
            F::GoogleGenerativeAi,
            "google",
            "https://generativelanguage.example",
            None,
            false,
        ),
        // 现役只认 `=== true` 的显式声明；Some(false) 落到族清单（不豁免）。
        (
            F::AnthropicMessages,
            "acme",
            "https://acme.example",
            Some(false),
            true,
        ),
        (
            F::OpenAiCompletions,
            "anthropic",
            "https://x.example",
            Some(false),
            true,
        ),
    ];
    for (family, provider, endpoint, declared, expected) in matrix {
        assert_eq!(
            output_cap_required(*family, provider, endpoint, *declared),
            *expected,
            "family={family:?} provider={provider} endpoint={endpoint} declared={declared:?}"
        );
    }
}

#[test]
fn required_output_cap_is_the_incumbent_min_with_formula_fallback() {
    // min(maxTokens, contextWindow)；缺一取一；两缺/非正回退公式
    // max(512, floor(0.8×reserve))；超大值钳到 u32::MAX。
    assert_eq!(
        required_output_cap(Some(8_000), Some(200_000), 16_384),
        8_000
    );
    assert_eq!(
        required_output_cap(Some(200_000), Some(128_000), 16_384),
        128_000
    );
    assert_eq!(required_output_cap(None, Some(64_000), 16_384), 64_000);
    assert_eq!(required_output_cap(Some(32_000), None, 16_384), 32_000);
    // 现役 positiveInteger：0 视为未声明。
    assert_eq!(required_output_cap(Some(0), Some(0), 16_384), 13_107);
    assert_eq!(required_output_cap(None, None, 16_384), 13_107);
    assert_eq!(required_output_cap(None, None, 512), 512);
    assert_eq!(required_output_cap(Some(u64::MAX), None, 16_384), u32::MAX);
}

// ─── FIX-05（R2-F-05）：split-turn scope 行两臂 ───

#[test]
fn split_turn_scope_line_rides_only_the_split_turn_arm() {
    const SPLIT_LINE: &str = "This is a split-turn compaction: preserve the original request \
                              and early progress needed to understand the retained suffix.";
    let split = build_summary_instruction(&SummaryInstructionSpec {
        old_region_items: 4,
        split_turn: true,
        custom_focus: None,
    });
    assert!(
        split.contains(SPLIT_LINE),
        "split-turn arm carries the line"
    );
    // 位置：recent-tail 提示之后（现役 410-414 的顺序）。
    let recent_tail = split.find("incorporate it from that position").unwrap();
    let split_at = split.find(SPLIT_LINE).unwrap();
    assert!(
        split_at > recent_tail,
        "the split-turn line follows the scope lines"
    );
    let epoch_boundary = build_summary_instruction(&SummaryInstructionSpec {
        old_region_items: 4,
        split_turn: false,
        custom_focus: None,
    });
    assert!(
        !epoch_boundary.contains(SPLIT_LINE),
        "an epoch-boundary cut (CompactionSummary) is NOT a split turn"
    );
}

// ─── FIX-07（N-1）：fit 检查（现役 shouldHardTruncate 的缓存保留臂） ───

#[test]
fn cache_preserving_request_fits_follows_the_085_window_threshold() {
    // fit 检查（摘要模型窗口 ×0.85；native-fallback 臂无挂载面）——§十#12。
    // 窗口未声明（0）→ 现役同一臂：超窗。
    assert!(!cache_preserving_request_fits(1, 0));
    // 阈值 = floor(window × 0.85)：恰好等于阈值通过，超 1 即超窗。
    assert!(cache_preserving_request_fits(85_000, 100_000));
    assert!(!cache_preserving_request_fits(85_001, 100_000));
    // floor 语义：100_001 × 0.85 = 85000.85 → 85000。
    assert!(cache_preserving_request_fits(85_000, 100_001));
    assert!(!cache_preserving_request_fits(85_001, 100_001));
}

// ─── FIX-02（R2-F-02）：fileOps 段（extractFileOperations /
// computeFileLists / appendFileOperationContext 的 Rust 形态） ───

fn file_call(seq: u32, wire: &str, path: &str) -> RequestedToolCall {
    let request = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
        format!("tool:first-party:{wire}"),
        serde_json::json!({ "path": path }),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective arguments")
    .with_provider_call_id(format!("fc{seq:04}"));
    RequestedToolCall {
        tool_call_id: ToolCallId::new(format!("ftc{seq:04}")),
        provider_call_id: request.provider_call_id.clone(),
        target: request.target.clone(),
        arguments: request.arguments.clone(),
        args_digest: request.args_digest,
        args_summary: None,
    }
}

#[test]
fn file_ops_compute_the_incumbent_lists() {
    let old = vec![
        assistant_turn(
            1,
            "a",
            vec![
                file_call(1, "read", "/b.txt"),
                file_call(2, "read", "/a.txt"),
            ],
        ),
        tool_result(1, "r1"),
        tool_result(2, "r2"),
        assistant_turn(
            2,
            "b",
            vec![
                file_call(3, "write", "/a.txt"),
                file_call(4, "edit", "/c.txt"),
            ],
        ),
        tool_result(3, "r3"),
        tool_result(4, "r4"),
        // 非文件工具与无 path 参数的调用不计。
        assistant_turn(3, "c", vec![requested_call(5, "tool:first-party:exec")]),
    ];
    let lists = extract_file_operations(&old);
    // modified = edited ∪ written = {/a.txt, /c.txt}；
    // readOnly = read − modified = {/b.txt}（/a.txt 被写过 → 移出只读）。
    assert_eq!(lists.read_only, vec!["/b.txt".to_string()]);
    assert_eq!(
        lists.modified,
        vec!["/a.txt".to_string(), "/c.txt".to_string()]
    );
}

#[test]
fn file_ops_sections_append_only_when_non_empty() {
    // fileOps 段迁移、其余四段归因 REG-02——§十#6。
    // 无 details：逐字不追加（现役 sections.length===0 臂）。
    let plain = vec![assistant_turn(1, "a", vec![])];
    assert_eq!(append_file_operation_context("SUMMARY", &plain), "SUMMARY");
    // 有 details：trimEnd + "\n\n" + 段（段格式逐字对齐现役）。
    let old = vec![
        assistant_turn(1, "a", vec![file_call(1, "read", "/r.txt")]),
        tool_result(1, "r"),
        assistant_turn(2, "b", vec![file_call(2, "edit", "/m.txt")]),
        tool_result(2, "r"),
    ];
    let enriched = append_file_operation_context("SUMMARY\n\n", &old);
    assert_eq!(
        enriched,
        "SUMMARY\n\n<read-files>\n/r.txt\n</read-files>\n\n<modified-files>\n/m.txt\n</modified-files>"
    );
}

#[test]
fn file_ops_chain_across_compaction_epochs() {
    // 跨轮续传（现役 extractFileOperations 的播种臂）：旧区最后一个摘要
    // 的段解析回集合——readFiles→read、modifiedFiles→edited。
    let previous = ExchangeItem::CompactionSummary {
        summary: "上一轮摘要正文\n\n<read-files>\n/old-read.txt\n</read-files>\n\n\
                  <modified-files>\n/old-mod.txt\n</modified-files>"
            .to_string(),
        covered_items: 5,
        mid_run: true,
    };
    let old = vec![
        previous,
        assistant_turn(1, "a", vec![file_call(1, "read", "/old-mod.txt")]),
        tool_result(1, "r"),
        assistant_turn(2, "b", vec![file_call(2, "read", "/new-read.txt")]),
        tool_result(2, "r"),
    ];
    let lists = extract_file_operations(&old);
    // /old-mod.txt 以 edited 播种 → 留在 modified（本轮又读了它也不降格）；
    // /old-read.txt 续传为只读；/new-read.txt 本轮新读。
    assert_eq!(
        lists.read_only,
        vec!["/new-read.txt".to_string(), "/old-read.txt".to_string()]
    );
    assert_eq!(lists.modified, vec!["/old-mod.txt".to_string()]);
    // 追加后的新摘要携带合并清单（续传闭环：下一轮还能解析回来）。
    let enriched = append_file_operation_context("S", &old);
    let round_trip = extract_file_operations(&[ExchangeItem::CompactionSummary {
        summary: enriched,
        covered_items: 3,
        mid_run: false,
    }]);
    assert_eq!(round_trip, lists);
}

// ─── 语义清单驱动（RC-3 防再发 §五-2）：每条现役语义项必须在代码面
// 存在（已迁移）或在报告 §十 声明。本测试锁死「已迁移」抽样的代码面
// 存在性；§十 声明侧由报告-测试交叉锁（FIX 集合落地脚本）核对。 ───

#[test]
fn incumbent_semantics_checklist_stays_wired() {
    // 每条 = (语义项, 现役锚点, 代码面探针)。探针失败 = 该语义项的实现
    // 被移除/改形而清单没更新——红。L1 会话级 32KB 截断 guard 未迁移
    // （per-tool 预算部分覆盖）——§十#14。
    // 1. FORCE 线 80%（compaction.js:144-148）
    assert_eq!(COMPACTION_FORCE_RATIO_BP, 8_000);
    // 2. reserve 地板（MIN_COMPACTION_RESERVE_TOKENS）
    assert_eq!(MIN_COMPACTION_RESERVE_TOKENS, 16_384);
    // 3. keep_recent（DEFAULT_COMPACTION_SETTINGS.keepRecentTokens）
    assert_eq!(KEEP_RECENT_TOKENS, 20_000);
    // 4. fit 阈值 0.85（DEFAULT_HARD_TRUNCATE_THRESHOLD）
    assert_eq!(HARD_TRUNCATE_THRESHOLD_BP, 8_500);
    // 5. 请求估算缓冲 1024（COMPACTION_REQUEST_BUFFER_TOKENS）
    assert_eq!(COMPACTION_REQUEST_BUFFER_TOKENS, 1_024);
    // 6. 硬截断标记文本（session-compactor.ts hardTruncate 默认 summary）
    assert!(HARD_TRUNCATE_MARKER_TEXT.contains("硬截断"));
    assert!(HARD_TRUNCATE_MARKER_TEXT.contains("hana-cache-preserving-compaction"));
    // 7. placeholder 应答文本（agent-run.ts clonePlaceholderTools）
    assert!(PLACEHOLDER_TOOL_RESULT_TEXT.contains("No live tool was executed"));
}
