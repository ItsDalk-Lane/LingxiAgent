//! REVIEWER-R06-T02-R2 的独立对抗探针（第二轮，全部为新构造，不重放 R1）。
//!
//! 探针清单：
//!   A — pending 组在历史**中段**（非尾部）：切点硬上界必须是它；
//!       pending 在下标 0 时无可行切点 → Ok(None)。
//!   B — CacheInclusion 经**真实** `USAGE_MAPPINGS` 接线（与 service
//!       `usage_inclusion_of` 同一解析式）：OpenAI×3/Google=cache_read
//!       included、Anthropic=独立；半截 usage 不虚构扣除；饱和减法不下溢。
//!   C — 级联二次压缩（摘要领头 + 夹层保留区）：产物必须仍可证明、
//!       cut≥1、无孤儿；同时行为级验证 F-04 的 mid_run 两臂。
//!   D — 触发边界精度：window=100_000 时 79_999 Below / 80_000 Force；
//!       window=3 退化形；window=0 / 全 None usage / 零 usage。
//!   E — keep_recent 切点方向实证：构造跨界项为大 ToolResult 的形状，
//!       同一估算口径下并行模拟现役 findCutPoint 算法（前跳到 ≥i 的
//!       第一个非 toolResult 边界）与 Rust plan_compaction（回退到
//!       ≤crossing 的组起点），输出两侧保留量对照（finding 证据，
//!       非 PASS 门）。

use lingxi_kernel::compaction::*;
use lingxi_kernel::model_exchange::{ExchangeItem, ProtocolFamily, RequestedToolCall, TurnOrigin};
use lingxi_kernel::ports::ToolOutcome;
use lingxi_kernel::usage::{ModelCallUsage, UsageProvenance};
use lingxi_protocol::{ContentBlock, ModelCallId, ToolCallId};

// ─── 夹具（自构造，不抄测试文件） ───

fn call(seq: u32, target: &str) -> RequestedToolCall {
    let request = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
        target,
        serde_json::json!({ "path": format!("/probe/{seq}.txt") }),
        &lingxi_kernel::toolcatalog::SchemaBudget::default(),
    )
    .expect("effective arguments")
    .with_provider_call_id(format!("pc{seq:04}"));
    RequestedToolCall {
        tool_call_id: ToolCallId::new(format!("tc{seq:04}")),
        provider_call_id: request.provider_call_id.clone(),
        target: request.target.clone(),
        arguments: request.arguments.clone(),
        args_digest: request.args_digest.clone(),
        args_summary: None,
    }
}

fn turn(seq: u32, text: &str, calls: Vec<RequestedToolCall>) -> ExchangeItem {
    ExchangeItem::AssistantTurn {
        call: ModelCallId::new(format!("mc{seq:04}")),
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
        tool_calls: calls,
        origin: Some(TurnOrigin {
            provider: "probe".to_string(),
            model: "probe-model".to_string(),
        }),
    }
}

fn result(seq: u32, text: &str) -> ExchangeItem {
    ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new(format!("tc{seq:04}")),
        provider_call_id: Some(format!("pc{seq:04}")),
        outcome: ToolOutcome::Success {
            result: lingxi_kernel::ports::ToolSuccess {
                content: vec![ContentBlock::Text {
                    text: text.to_string(),
                }],
                resource_refs: Vec::new(),
                truncated: false,
                status: None,
                content_digest: format!("probe-digest-{seq}"),
            },
        },
    }
}

/// 闭合组：[turn(带一个调用), result]。
fn closed_group(seq: u32, chunk: usize) -> Vec<ExchangeItem> {
    vec![
        turn(seq, &format!("turn {seq} {}", "t".repeat(chunk)), vec![call(seq, "read_file")]),
        result(seq, &format!("result {seq} {}", "r".repeat(chunk))),
    ]
}

/// 未闭合组：turn 带调用、无结果。
fn pending_group(seq: u32, chunk: usize) -> Vec<ExchangeItem> {
    vec![turn(seq, &format!("pending {seq} {}", "p".repeat(chunk)), vec![call(seq, "read_file")])]
}

fn usage(input: Option<u64>, output: Option<u64>, cr: Option<u64>, cw: Option<u64>) -> ModelCallUsage {
    ModelCallUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cr,
        cache_write_tokens: cw,
        reasoning_tokens: None,
        provenance: UsageProvenance::Reported,
    }
}

/// 与 service `usage_inclusion_of` 逐字同一的解析式（探针直接消费
/// 适配器层真实映射表，不绕过 USAGE_MAPPINGS）。
fn inclusion_of(family: ProtocolFamily) -> CacheInclusion {
    let mapping = lingxi_adapters::models::usage::mapping_of(family);
    CacheInclusion {
        cache_read_in_input: mapping
            .and_then(|m| m.cache_read_included_in_input)
            .unwrap_or(false),
        cache_write_in_input: mapping
            .and_then(|m| m.cache_write_included_in_input)
            .unwrap_or(false),
    }
}

static mut FAILURES: u32 = 0;

fn check(label: &str, ok: bool, detail: String) {
    #[allow(static_mut_refs)]
    unsafe {
        if !ok {
            FAILURES += 1;
        }
    }
    println!("[{}] {label} :: {detail}", if ok { "PASS" } else { "FAIL" });
}

// ─── Probe A：中段 pending 组是切点硬上界 ───

fn probe_a() {
    println!("== Probe A: mid-history pending group bounds the cut ==");
    // 形状：g0 闭合 | g1 PENDING | g2..g7 闭合且足够大（让跨界深入尾部）。
    let mut exchange = Vec::new();
    exchange.extend(closed_group(1, 400));
    exchange.extend(pending_group(2, 50)); // 下标 2
    for g in 3..=8u32 {
        exchange.extend(closed_group(g * 2 + 1, 6_000));
    }
    let pending_index = 2usize; // pending turn 的下标
    let plan = plan_compaction(&exchange, 5_000).expect("pairs provable");
    match plan {
        Some(plan) => {
            check(
                "A1 cut ≤ pending start",
                plan.cut_index <= pending_index,
                format!("cut_index={} pending_index={pending_index}", plan.cut_index),
            );
            check(
                "A2 pending group retained",
                plan.cut_index <= pending_index,
                "pending group never enters the summarized region".to_string(),
            );
        }
        None => check(
            "A1 cut ≤ pending start",
            false,
            "Ok(None) although a safe boundary at the pending group start exists".to_string(),
        ),
    }

    // pending 在下标 0：合法切点须 index>0 且 ≤0 → 不存在 → Ok(None)。
    let mut exchange0 = pending_group(11, 50);
    for g in 12..=16u32 {
        exchange0.extend(closed_group(g, 6_000));
    }
    let plan0 = plan_compaction(&exchange0, 5_000).expect("pairs provable");
    check(
        "A3 pending at index 0 → Ok(None)",
        plan0.is_none(),
        format!("plan0={plan0:?}"),
    );
}

// ─── Probe B：族 inclusion 经真实 USAGE_MAPPINGS 接线 ───

fn probe_b() {
    println!("== Probe B: family inclusion via the real USAGE_MAPPINGS ==");
    let openai = inclusion_of(ProtocolFamily::OpenAiCompletions);
    let responses = inclusion_of(ProtocolFamily::OpenAiResponses);
    let codex = inclusion_of(ProtocolFamily::OpenAiCodexResponses);
    let google = inclusion_of(ProtocolFamily::GoogleGenerativeAi);
    let anthropic = inclusion_of(ProtocolFamily::AnthropicMessages);
    check(
        "B0 flags",
        openai.cache_read_in_input
            && responses.cache_read_in_input
            && codex.cache_read_in_input
            && google.cache_read_in_input
            && !anthropic.cache_read_in_input
            && !openai.cache_write_in_input
            && !google.cache_write_in_input
            && !anthropic.cache_write_in_input,
        format!("openai={openai:?} google={google:?} anthropic={anthropic:?}"),
    );

    // F-02 形状：OpenAI wire input 含 cache_read → 扣了再加回 = 不双计。
    let wire = usage(Some(110_000), Some(500), Some(30_000), None);
    let openai_total = context_tokens_from_usage(&wire, openai);
    let anthropic_total = context_tokens_from_usage(&wire, anthropic);
    check(
        "B1 openai no double count",
        openai_total == Some(110_500),
        format!("openai={openai_total:?} (incumbent-equivalent 80000+500+30000=110500)"),
    );
    check(
        "B2 anthropic independent components",
        anthropic_total == Some(140_500),
        format!("anthropic={anthropic_total:?} (110000+500+30000)"),
    );

    // 半截 usage：input 缺失而 cache_read 在场 → 不虚构扣除。
    let half = usage(None, Some(100), Some(10_200), None);
    let half_openai = context_tokens_from_usage(&half, openai);
    check(
        "B3 half usage no fictitious deduction",
        half_openai == Some(10_300),
        format!("half_openai={half_openai:?} (0+100+10200, no invented input deduction)"),
    );

    // 饱和减法：input 50 < cache_read 10_000 → effective 0，不下溢不 panic。
    let weird = usage(Some(50), None, Some(10_000), None);
    let weird_total = context_tokens_from_usage(&weird, openai);
    check(
        "B4 saturating subtraction",
        weird_total == Some(10_000),
        format!("weird={weird_total:?} (0 effective input + 10000 cache_read)"),
    );

    // 全 None = None（没有 usage 事实，绝非零）。
    let none_total = context_tokens_from_usage(
        &usage(None, None, None, None),
        openai,
    );
    check("B5 all-None → None", none_total.is_none(), format!("{none_total:?}"));
}

// ─── Probe C：级联二次压缩 + mid_run 两臂 ───

fn probe_c() {
    println!("== Probe C: cascading compaction over a summary-led sandwich ==");
    // 第一轮：夹层形状（owner 与 result 之间隔一个无调用 turn）。
    let mut exchange = Vec::new();
    for g in 0..6u32 {
        let seq = g * 3 + 1;
        exchange.push(turn(seq, &format!("owner {g} {}", "o".repeat(3_000)), vec![call(seq, "read_file")]));
        exchange.push(turn(seq + 1, &format!("filler {g} {}", "f".repeat(3_000)), Vec::new()));
        exchange.push(result(seq, &format!("result {g} {}", "r".repeat(3_000))));
    }
    let plan1 = plan_compaction(&exchange, 8_000)
        .expect("first pass provable")
        .expect("first pass has a beneficial cut");
    let summary = "## Goal\nx\n\n## Constraints & Preferences\n- (none)\n\n## Progress\n### Done\n- [x] a\n\n### In Progress\n- [ ] b\n\n### Blocked\n- none\n\n## Key Decisions\n- **d**: r\n\n## Next Steps\n1. n\n\n## Critical Context\n- (none)".to_string();
    let compacted = apply_plan(&exchange, &plan1, summary.clone(), true);
    match &compacted[0] {
        ExchangeItem::CompactionSummary { mid_run, covered_items, .. } => {
            check(
                "C0 mid_run arm true",
                *mid_run && *covered_items as usize == plan1.cut_index,
                format!("mid_run={mid_run} covered={covered_items} cut={}", plan1.cut_index),
            );
        }
        other => check("C0 summary heads the product", false, format!("{other:?}")),
    }
    let compacted_manual = apply_plan(&exchange, &plan1, summary, false);
    match &compacted_manual[0] {
        ExchangeItem::CompactionSummary { mid_run, .. } => check(
            "C0b mid_run arm false",
            !*mid_run,
            format!("mid_run={mid_run}"),
        ),
        other => check("C0b summary heads the manual product", false, format!("{other:?}")),
    }

    // 第二轮：摘要领头 + 保留区（保留区仍 > keep_recent → 必须还能压）。
    let plan2 = plan_compaction(&compacted, 8_000);
    match plan2 {
        Ok(Some(plan2)) => {
            check(
                "C1 second pass cut ≥ 1",
                plan2.cut_index >= 1,
                format!("cut2={}", plan2.cut_index),
            );
            check(
                "C2 second cut on a group boundary",
                matches!(
                    compacted[plan2.cut_index],
                    ExchangeItem::AssistantTurn { .. } | ExchangeItem::CompactionSummary { .. }
                ),
                format!("item at cut2 = {:?}", compacted[plan2.cut_index]),
            );
            // 保留区无孤儿：保留区内每个 result 的 owner 也在保留区。
            let product2 = apply_plan(&compacted, &plan2, "s2".to_string(), true);
            let mut owners = std::collections::HashSet::new();
            for item in &product2 {
                if let ExchangeItem::AssistantTurn { tool_calls, .. } = item {
                    for c in tool_calls {
                        owners.insert(c.tool_call_id.as_str().to_string());
                    }
                }
            }
            let orphan = product2.iter().any(|item| match item {
                ExchangeItem::ToolResult { tool_call_id, .. } => {
                    !owners.contains(tool_call_id.as_str())
                }
                _ => false,
            });
            check("C3 second product orphan-free", !orphan, format!("orphan={orphan}"));
        }
        Ok(None) => check(
            "C1 second pass still plannable",
            false,
            "Ok(None) although retained region still exceeds keep_recent".to_string(),
        ),
        Err(err) => check(
            "C1 second pass provable",
            false,
            format!("cascade poisoned: {err}"),
        ),
    }
}

// ─── Probe D：触发边界精度 ───

fn probe_d() {
    println!("== Probe D: trigger boundary precision ==");
    let budget = ContextBudget::for_declared_window(Some(100_000)).expect("declared window");
    check(
        "D0 reserve derivation",
        budget.reserve_tokens == 20_000 && budget.keep_recent_tokens == 20_000,
        format!("reserve={} keep={}", budget.reserve_tokens, budget.keep_recent_tokens),
    );
    let facts_at = |tokens: u64| TurnBudgetFacts {
        system_tokens: 0,
        submission_tokens: 0,
        tool_declaration_tokens: 0,
        history_tokens: 0,
        tail_tokens: 0,
        last_usage: Some(usage(Some(tokens), Some(0), None, None)),
        usage_inclusion: CacheInclusion::default(),
    };
    // 注意：input=tokens、output=Some(0) → 总量 = tokens。
    let below = budget.evaluate(&facts_at(79_999));
    let at = budget.evaluate(&facts_at(80_000));
    check(
        "D1 79_999 below",
        matches!(below, CompactionDecision::BelowThreshold { .. }),
        format!("{below:?}"),
    );
    check(
        "D2 80_000 force (inclusive boundary)",
        matches!(at, CompactionDecision::Force { .. }),
        format!("{at:?}"),
    );

    // 退化小窗口：reserve 饱和 → 任何正 usage 都过 reserve 线。
    let tiny = ContextBudget::for_declared_window(Some(3)).expect("window 3");
    let tiny_eval = tiny.evaluate(&facts_at(1));
    check(
        "D3 tiny window saturates",
        tiny.reserve_tokens == 16_384 && matches!(tiny_eval, CompactionDecision::Force { .. }),
        format!("reserve={} eval={tiny_eval:?}", tiny.reserve_tokens),
    );

    check(
        "D4 window 0 → None",
        ContextBudget::for_declared_window(Some(0)).is_none(),
        "for_declared_window(Some(0))".to_string(),
    );
    let no_usage = budget.evaluate(&TurnBudgetFacts {
        last_usage: None,
        ..facts_at(0)
    });
    let zero_usage = budget.evaluate(&facts_at(0));
    check(
        "D5 no usage → Unavailable",
        matches!(no_usage, CompactionDecision::Unavailable { .. }),
        format!("{no_usage:?}"),
    );
    check(
        "D6 zero usage → Unavailable",
        matches!(zero_usage, CompactionDecision::Unavailable { .. }),
        format!("{zero_usage:?}"),
    );
}

// ─── Probe E：keep_recent 切点方向实证（finding 证据，非 PASS 门） ───

fn probe_e() {
    println!("== Probe E: keep_recent snap direction (incumbent simulation vs rust) ==");
    // 形状：尾部一个大 ToolResult 是跨界项。
    // 0:A0 1:R0 2:A1 3:R1(大) 4:A2 5:R2 6:A3 7:R3
    let mut exchange = Vec::new();
    exchange.extend(closed_group(1, 1_000)); // A0 R0 ≈ 500+500
    exchange.push(turn(3, &format!("owner {}", "o".repeat(200)), vec![call(3, "read_file")]));
    exchange.push(result(3, &"R".repeat(100_000))); // R1 ≈ 25_000 tokens
    exchange.extend(closed_group(5, 200));
    exchange.extend(closed_group(7, 200));
    let keep = 5_000u64;

    // Rust 实际产出。
    let rust_plan = plan_compaction(&exchange, keep)
        .expect("provable")
        .expect("beneficial cut exists");
    let rust_retained = rust_plan.retained_tokens;

    // 现役 findCutPoint 算法模拟（同一估算口径，只比算法方向）：
    // cutPoints = 非 ToolResult 项；从尾累积消息 token，跨界于 i 后取
    // 第一个 ≥ i 的合法切点。
    let cut_points: Vec<usize> = exchange
        .iter()
        .enumerate()
        .filter(|(_, item)| !matches!(item, ExchangeItem::ToolResult { .. }))
        .map(|(index, _)| index)
        .collect();
    let mut accumulated = 0u64;
    let mut crossing = None;
    for index in (0..exchange.len()).rev() {
        accumulated += estimate_exchange_item_tokens(&exchange[index]);
        if accumulated >= keep {
            crossing = Some(index);
            break;
        }
    }
    let crossing = crossing.expect("crossing exists");
    let incumbent_cut = cut_points
        .iter()
        .copied()
        .find(|index| *index >= crossing)
        .unwrap_or(cut_points[0]);
    let incumbent_retained = estimate_exchange_tokens(&exchange[incumbent_cut..]);

    println!(
        "  crossing_item_index={crossing} (is ToolResult: {})",
        matches!(exchange[crossing], ExchangeItem::ToolResult { .. })
    );
    println!(
        "  rust:        cut={} retained={rust_retained} (>= keep_recent {keep}: {})",
        rust_plan.cut_index,
        rust_retained >= keep
    );
    println!(
        "  incumbent:   cut={incumbent_cut} retained={incumbent_retained} (< keep_recent {keep}: {})",
        incumbent_retained < keep
    );
    println!(
        "  DIVERGENCE:  rust retains >= keep_recent (backward group snap); incumbent snaps forward past the crossing toolResult and can retain less."
    );
}

fn main() {
    probe_a();
    probe_b();
    probe_c();
    probe_d();
    probe_e();
    #[allow(static_mut_refs)]
    let failures = unsafe { FAILURES };
    if failures == 0 {
        println!("ALL PROBE GATES PASSED (E is evidence-only)");
    } else {
        println!("{failures} PROBE GATE(S) FAILED");
        std::process::exit(1);
    }
}
