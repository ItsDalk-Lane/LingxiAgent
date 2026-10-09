//! REVIEWER-R06-T02-R1 反例探针（只读审查代码，独立 crate，不修改产品代码）。
//! 探针 1：plan_compaction 是否会把「owner 在被压区、result 在保留区」的
//! 孤立 tool result 放上线（A03 核心保护是否真实存在）。
//! 探针 2：OpenAI 族口径（input 已含 cache_read）下
//! context_tokens_from_usage 是否双计 cache_read。

use lingxi_kernel::compaction::{context_tokens_from_usage, plan_compaction};
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

fn main() {
    // ── 探针 1：owner 与 result 之间隔着另一个 assistant turn ──
    // 交换形状：
    //   0: A0（call tc1）           ← tc1 的 owner
    //   1: A1（无调用，大文本）      ← 介于 owner 与 result 之间的另一组
    //   2: R(tc1)（大文本）          ← tc1 的结果
    //   3: A2（call tc2）
    //   4: R(tc2)
    // 配对证明可通过（每个 result 都有 owner 且 owner 在前、无重复）。
    // keep_recent 取恰好让累积在 R(tc1) 处跨界 → cut 回退到 A1（index 1）
    // → 保留区 = [A1, R(tc1), A2, R(tc2)]，R(tc1) 的 owner A0 在被压区。
    let big = "a".repeat(4_000);
    let exchange = vec![
        assistant(1, big.clone(), vec![call(1)]),
        assistant(2, big.clone(), vec![]),
        result(1, big.clone()),
        assistant(3, "tail".to_string(), vec![call(3)]),
        result(3, "tail".to_string()),
    ];
    let plan = plan_compaction(&exchange, 2_000)
        .expect("pairs are provable per the planner's own rules")
        .expect("a cut exists");
    println!("probe1: cut_index = {}", plan.cut_index);
    let kept = &exchange[plan.cut_index..];
    let summarized = &exchange[..plan.cut_index];
    let kept_call_ids: std::collections::HashSet<String> = kept
        .iter()
        .filter_map(|item| match item {
            ExchangeItem::AssistantTurn { tool_calls, .. } => {
                Some(tool_calls.iter().map(|c| c.tool_call_id.as_str().to_string()))
            }
            _ => None,
        })
        .flatten()
        .collect();
    let mut orphan_in_kept = false;
    for item in kept {
        if let ExchangeItem::ToolResult { tool_call_id, .. } = item {
            if !kept_call_ids.contains(tool_call_id.as_str()) {
                orphan_in_kept = true;
                println!(
                    "probe1: ORPHANED tool result {} lands in the RETAINED region (its owner is summarized away)",
                    tool_call_id.as_str()
                );
            }
        }
    }
    println!(
        "probe1: summarized items = {:?}",
        summarized
            .iter()
            .map(|i| match i {
                ExchangeItem::AssistantTurn { call, .. } => format!("A:{}", call.as_str()),
                ExchangeItem::ToolResult { tool_call_id, .. } =>
                    format!("R:{}", tool_call_id.as_str()),
                _ => "other".to_string(),
            })
            .collect::<Vec<_>>()
    );
    println!(
        "probe1: kept items = {:?}",
        kept.iter()
            .map(|i| match i {
                ExchangeItem::AssistantTurn { call, .. } => format!("A:{}", call.as_str()),
                ExchangeItem::ToolResult { tool_call_id, .. } =>
                    format!("R:{}", tool_call_id.as_str()),
                _ => "other".to_string(),
            })
            .collect::<Vec<_>>()
    );
    println!("probe1 verdict: {}", if orphan_in_kept { "VULNERABLE" } else { "SAFE" });

    // ── 探针 2：OpenAI 族口径的 usage 双计 ──
    // R05 统一结构：OpenAI 族 input_tokens 含 cache_read（USAGE_MAPPINGS
    // cache_read_included_in_input = Some(true)，decode_family_usage 不扣除）。
    // 现役 pi-ai 的 OpenAI 规范化：input = prompt_tokens − cacheRead − cacheWrite，
    // calculateContextTokens = input+output+cacheRead+cacheWrite = 无重复。
    // Rust context_tokens_from_usage 四分量全加：
    let usage = ModelCallUsage {
        // OpenAI wire: prompt_tokens=100_000（含 cached 30_000），completion=500。
        input_tokens: Some(100_000),
        output_tokens: Some(500),
        cache_read_tokens: Some(30_000),
        cache_write_tokens: None,
        reasoning_tokens: None,
        provenance: lingxi_kernel::usage::UsageProvenance::Reported,
    };
    let rust_total = context_tokens_from_usage(&usage);
    // 现役等价口径：input 扣除 cache 后四分量加 = 70_000 + 500 + 30_000 = 100_500。
    let incumbent_total = 100_000u64 - 30_000 + 500 + 30_000;
    println!("probe2: rust context_tokens = {rust_total:?}, incumbent-equivalent = {incumbent_total}");
    println!(
        "probe2 verdict: {}",
        if rust_total == Some(incumbent_total) { "MATCH" } else { "DOUBLE-COUNTS cache_read" }
    );

    // ── 探针 3：孤儿化压缩产物对二次压缩的毒化 ──
    probe3_second_compaction_after_orphaned_cut();
}

#[allow(dead_code)]
fn probe3_second_compaction_after_orphaned_cut() {
    // 探针 3：探针 1 的产物（含孤儿 R(tc1) 的压缩后交换）再次进入
    // plan_compaction —— 配对证明应响亮失败（孤儿 result 的 owner 已被摘要），
    // 即：第一次压缩制造孤儿后，该会话此后再也无法压缩（永久 Failed）。
    let big = "a".repeat(4_000);
    let exchange = vec![
        assistant(1, big.clone(), vec![call(1)]),
        assistant(2, big.clone(), vec![]),
        result(1, big.clone()),
        assistant(3, "tail".to_string(), vec![call(3)]),
        result(3, "tail".to_string()),
    ];
    let plan = plan_compaction(&exchange, 2_000)
        .expect("provable")
        .expect("cut exists");
    // 第一次压缩被接受（摘要通过校验）后的新交换：
    let mut second = vec![ExchangeItem::CompactionSummary {
        summary: "s".to_string(),
        covered_items: 1,
        mid_run: true,
    }];
    second.extend_from_slice(&exchange[plan.cut_index..]);
    // 追加更多历史使第二次压缩有可切空间：
    second.push(assistant(5, big.clone(), vec![call(5)]));
    second.push(result(5, big.clone()));
    second.push(assistant(7, big.clone(), vec![call(7)]));
    second.push(result(7, big.clone()));
    match plan_compaction(&second, 2_000) {
        Err(lingxi_kernel::compaction::CompactionPlanError::UnprovableToolPairs { detail }) => {
            println!("probe3: second compaction REFUSED as unprovable: {detail}");
            println!("probe3 verdict: ORPHAN POISONS ALL FUTURE COMPACTIONS");
        }
        Ok(plan2) => {
            println!("probe3: second compaction planned: {plan2:?}");
            println!("probe3 verdict: second compaction still plannable");
        }
    }
}
