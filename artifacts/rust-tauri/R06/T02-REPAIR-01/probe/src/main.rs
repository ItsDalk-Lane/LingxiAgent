//! REPAIR-R06-T02-R1 修复后探针（独立 crate，path 依赖产品 crate，不修改
//! 产品代码）。与 REVIEW-R06-T02-R1 的三个反例探针同一形状，断言修复后
//! 行为：
//!   探针 1（F-01）：夹层形状 [A0(tc1), A1, R(tc1), A2(tc2), R(tc2)] 在
//!     keep_recent=2000 下不再产出孤儿切点——唯一可达边界会孤儿化 →
//!     Ok(None)（宁可不压）。
//!   探针 2（F-02）：OpenAI 族口径（input 含 cache_read，旗标
//!     cache_read_included_in_input=Some(true)）下
//!     context_tokens_from_usage = 100_500（现役等价），Anthropic 族口径
//!     四分量全加。族旗标本体（USAGE_MAPPINGS）一并断言。
//!   探针 3（F-01 级联）：一次安全压缩的产物再次进入 plan_compaction 绝不
//!     因孤儿而不可证明（不再毒化该会话的未来压缩）。

use lingxi_kernel::compaction::{
    context_tokens_from_usage, plan_compaction, CacheInclusion,
};
use lingxi_kernel::model_exchange::{ExchangeItem, RequestedToolCall, TurnOrigin};
use lingxi_kernel::ports::ToolOutcome;
use lingxi_kernel::usage::ModelCallUsage;
use lingxi_protocol::{ContentBlock, ModelCallId, ToolCallId};

fn assistant(seq: u32, text: String, calls: Vec<RequestedToolCall>) -> ExchangeItem {
    ExchangeItem::AssistantTurn {
        call: ModelCallId::new(format!("mc{seq:04}")),
        content: vec![ContentBlock::Text { text }],
        tool_calls: calls,
        origin: Some(TurnOrigin {
            provider: "probe".to_string(),
            model: "probe-model".to_string(),
        }),
    }
}

fn call(seq: u32) -> RequestedToolCall {
    let request = lingxi_kernel::ports::ToolRequest::from_effective_arguments(
        "read_file",
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

fn result(seq: u32, text: String) -> ExchangeItem {
    ExchangeItem::ToolResult {
        tool_call_id: ToolCallId::new(format!("tc{seq:04}")),
        provider_call_id: Some(format!("call_{seq:04}")),
        outcome: ToolOutcome::Success {
            result: lingxi_kernel::ports::ToolSuccess {
                content: vec![ContentBlock::Text { text }],
                resource_refs: Vec::new(),
                truncated: false,
                status: None,
                content_digest: format!("digest-{seq}"),
            },
        },
    }
}

/// 双向无孤儿检查（与 kernel 回归测试的断言助手同一语义）。
fn orphans_in(exchange: &[ExchangeItem], cut_index: usize) -> Vec<String> {
    let kept = &exchange[cut_index..];
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
    let mut orphans = Vec::new();
    for item in kept {
        if let ExchangeItem::ToolResult { tool_call_id, .. } = item {
            if !kept_call_ids.contains(tool_call_id.as_str()) {
                orphans.push(tool_call_id.as_str().to_string());
            }
        }
    }
    orphans
}

fn main() {
    let mut failures = 0_u32;

    // ── 探针 1：审查者的精确夹层形状（修复前 cut=1 携带孤儿 R(tc0001)） ──
    let big = "a".repeat(4_000);
    let exchange = vec![
        assistant(1, big.clone(), vec![call(1)]),
        assistant(2, big.clone(), vec![]),
        result(1, big.clone()),
        assistant(3, "tail".to_string(), vec![call(3)]),
        result(3, "tail".to_string()),
    ];
    match plan_compaction(&exchange, 2_000) {
        Ok(None) => {
            println!("probe1: plan = None (the only reachable boundary would orphan tc0001)");
            println!("probe1 verdict: SAFE (no cut, no orphan)");
        }
        Ok(Some(plan)) => {
            let orphans = orphans_in(&exchange, plan.cut_index);
            if orphans.is_empty() {
                println!("probe1: cut_index = {} (pair-whole)", plan.cut_index);
                println!("probe1 verdict: SAFE (no orphan)");
            } else {
                println!("probe1: cut_index = {} ORPHANS {orphans:?}", plan.cut_index);
                println!("probe1 verdict: STILL VULNERABLE");
                failures += 1;
            }
        }
        Err(err) => {
            println!("probe1: unexpected planning error: {err:?}");
            println!("probe1 verdict: UNEXPECTED");
            failures += 1;
        }
    }

    // ── 探针 2：OpenAI 族口径不再双计 cache_read ──
    let usage = ModelCallUsage {
        input_tokens: Some(100_000),
        output_tokens: Some(500),
        cache_read_tokens: Some(30_000),
        cache_write_tokens: None,
        reasoning_tokens: None,
        provenance: lingxi_kernel::usage::UsageProvenance::Reported,
    };
    let openai_flag = lingxi_adapters::models::usage::mapping_of(
        lingxi_kernel::model_exchange::ProtocolFamily::OpenAiCompletions,
    )
    .and_then(|mapping| mapping.cache_read_included_in_input);
    println!("probe2: OpenAI mapping cache_read_included_in_input = {openai_flag:?}");
    let openai_total = context_tokens_from_usage(
        &usage,
        CacheInclusion {
            cache_read_in_input: openai_flag == Some(true),
            cache_write_in_input: false,
        },
    );
    println!("probe2: openai-family context_tokens = {openai_total:?} (incumbent = 100500)");
    if openai_flag == Some(true) && openai_total == Some(100_500) {
        println!("probe2 verdict: MATCH (no double-count)");
    } else {
        println!("probe2 verdict: STILL DOUBLE-COUNTS");
        failures += 1;
    }
    // Anthropic 族：cache 分量原生独立（Some(false)）→ 四分量全加。
    let anthropic_flag = lingxi_adapters::models::usage::mapping_of(
        lingxi_kernel::model_exchange::ProtocolFamily::AnthropicMessages,
    )
    .and_then(|mapping| mapping.cache_read_included_in_input);
    let anthropic_total = context_tokens_from_usage(
        &ModelCallUsage {
            input_tokens: Some(10),
            output_tokens: Some(4),
            cache_read_tokens: Some(2_000),
            cache_write_tokens: Some(1_000),
            reasoning_tokens: None,
            provenance: lingxi_kernel::usage::UsageProvenance::Reported,
        },
        CacheInclusion {
            cache_read_in_input: anthropic_flag == Some(true),
            cache_write_in_input: false,
        },
    );
    println!(
        "probe2: anthropic-family flag = {anthropic_flag:?}, context_tokens = {anthropic_total:?} (expect 3014)"
    );
    if anthropic_flag == Some(false) && anthropic_total == Some(3_014) {
        println!("probe2b verdict: MATCH (anthropic unchanged)");
    } else {
        println!("probe2b verdict: UNEXPECTED");
        failures += 1;
    }

    // ── 探针 3：安全压缩产物对二次压缩保持可证明（无毒化级联） ──
    // 修复后探针 1 形状不再可切（Ok(None)），改用「手工夹层头 + 4 组安全
    // 夹层」形状：第一次压缩安全落地，产物再次规划——可 None 可 Some，
    // 绝不 Err(UnprovableToolPairs)。
    let mut first = vec![
        assistant(1, big.clone(), vec![call(1)]),
        assistant(2, big.clone(), vec![]),
        result(1, big.clone()),
    ];
    for g in 0..4_u32 {
        let seq = 11 + g * 2;
        first.push(assistant(seq, big.clone(), vec![call(seq)]));
        first.push(assistant(seq + 1_000, big.clone(), vec![]));
        first.push(result(seq, big.clone()));
    }
    let plan = plan_compaction(&first, 2_500)
        .expect("provable")
        .expect("a safe cut exists in the long tail");
    let orphans = orphans_in(&first, plan.cut_index);
    assert!(orphans.is_empty(), "first cut must be pair-whole: {orphans:?}");
    println!("probe3: first cut_index = {} (pair-whole)", plan.cut_index);
    let mut second = vec![ExchangeItem::CompactionSummary {
        summary: "s".to_string(),
        covered_items: 1,
        mid_run: true,
    }];
    second.extend_from_slice(&first[plan.cut_index..]);
    second.push(assistant(41, big.clone(), vec![call(41)]));
    second.push(result(41, big.clone()));
    second.push(assistant(43, big.clone(), vec![call(43)]));
    second.push(result(43, big.clone()));
    match plan_compaction(&second, 1) {
        Err(err) => {
            println!("probe3: second compaction REFUSED: {err:?}");
            println!("probe3 verdict: STILL POISONED");
            failures += 1;
        }
        Ok(plan2) => {
            if let Some(plan2) = &plan2 {
                let orphans2 = orphans_in(&second, plan2.cut_index);
                assert!(orphans2.is_empty(), "second cut pair-whole: {orphans2:?}");
            }
            println!("probe3: second compaction planned cleanly: {:?}", plan2.map(|p| p.cut_index));
            println!("probe3 verdict: NO POISON CASCADE");
        }
    }

    if failures == 0 {
        println!("ALL PROBES SAFE");
    } else {
        println!("PROBE FAILURES: {failures}");
        std::process::exit(1);
    }
}
