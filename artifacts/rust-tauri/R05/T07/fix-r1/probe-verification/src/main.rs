//! R05-T07 R01 independent-review adversarial probe (reviewer-owned).
//!
//! Question under test (review finding F1): when a STREAMING usage fragment
//! of a RunningTotal family (anthropic message_delta.usage / google
//! usageMetadata) carries a contract-VIOLATING number (negative / float /
//! string), does the accumulator mark the fact Invalid per C06 — or does it
//! silently drop the fragment so the finished fact downgrades to
//! partial/unknown?
//!
//! Control legs:
//! - the SAME violation on the openai-completions streaming path (raw-JSON
//!   splice) is expected to stay Invalid;
//! - the SAME violation in anthropic BUFFERED mode is expected Invalid;
//! - a valid anthropic stream is expected Reported (proves the harness
//!   drives the accumulators correctly).

use lingxi_kernel::ports::StoragePort as _;
use lingxi_adapters::models::anthropic_messages::MessagesStreamAccumulator;
use lingxi_adapters::models::google_generative_ai::GenerateStreamAccumulator;
use lingxi_adapters::models::openai_completions::ChatStreamAccumulator;
use lingxi_adapters::models::streaming::decode_complete_body;
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_kernel::usage::ReportedUsage;
use lingxi_kernel::usage::{ModelCallUsage, ModelCallUsageRecord, ModelUsageQuery, UsageProvenance};
use lingxi_protocol::ModelCallId;

fn finish_anthropic(body: &str) -> Result<ReportedUsage, String> {
    let events = decode_complete_body(body).map_err(|e| format!("sse decode: {e}"))?;
    let mut acc = MessagesStreamAccumulator::new();
    for event in &events {
        acc.handle_event(event).map_err(|e| format!("handle_event: {e}"))?;
    }
    let parsed = acc
        .finish(
            &ModelCallId::new("probe-mc0001"),
            &lingxi_kernel::model_exchange::ToolDeclarationSnapshot::empty(),
            &SchemaBudget::default(),
        )
        .map_err(|e| format!("finish: {e}"))?;
    Ok(parsed.usage_report)
}

fn finish_google(body: &str) -> Result<ReportedUsage, String> {
    let events = decode_complete_body(body).map_err(|e| format!("sse decode: {e}"))?;
    let mut acc = GenerateStreamAccumulator::new();
    for event in &events {
        acc.handle_event(event).map_err(|e| format!("handle_event: {e}"))?;
    }
    let parsed = acc
        .finish(
            &ModelCallId::new("probe-mc0001"),
            &lingxi_kernel::model_exchange::ToolDeclarationSnapshot::empty(),
            &SchemaBudget::default(),
        )
        .map_err(|e| format!("finish: {e}"))?;
    Ok(parsed.usage_report)
}

fn finish_openai(body: &str) -> Result<ReportedUsage, String> {
    let events = decode_complete_body(body).map_err(|e| format!("sse decode: {e}"))?;
    let mut acc = ChatStreamAccumulator::new();
    for event in &events {
        acc.handle_event(event).map_err(|e| format!("handle_event: {e}"))?;
    }
    let parsed = acc
        .finish(
            &ModelCallId::new("probe-mc0001"),
            &lingxi_kernel::model_exchange::ToolDeclarationSnapshot::empty(),
            &SchemaBudget::default(),
        )
        .map_err(|e| format!("finish: {e}"))?;
    Ok(parsed.usage_report)
}

fn anthropic_stream(delta_usage: &str) -> String {
    format!(
        "event: message_start\ndata: {}\n\n\
         event: content_block_start\ndata: {}\n\n\
         event: content_block_delta\ndata: {}\n\n\
         event: content_block_stop\ndata: {}\n\n\
         event: message_delta\ndata: {}\n\n\
         event: message_stop\ndata: {}\n\n",
        r#"{"type":"message_start","message":{"usage":{"input_tokens":25}}}"#,
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
        r#"{"type":"content_block_stop","index":0}"#,
        format!(
            r#"{{"type":"message_delta","delta":{{"stop_reason":"end_turn"}},"usage":{delta_usage}}}"#
        ),
        r#"{"type":"message_stop"}"#,
    )
}

fn google_stream(usage_metadata: &str) -> String {
    let mut body = String::new();
    body.push_str("data: ");
    body.push_str(r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"hi"}]}}]}"#);
    body.push_str("\n\ndata: ");
    body.push_str(r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"!"}]},"finishReason":"STOP"}],"usageMetadata":"#);
    body.push_str(usage_metadata);
    body.push_str("}\n\n");
    body
}

fn openai_stream(usage_json: &str) -> String {
    format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        r#"{"id":"x","choices":[{"index":0,"delta":{"role":"assistant","content":"hi"},"finish_reason":null}]}"#,
        format!(r#"{{"id":"x","choices":[{{"index":0,"delta":{{}},"finish_reason":"stop"}}],"usage":{usage_json}}}"#),
    )
}

fn describe(report: &Result<ReportedUsage, String>) -> String {
    match report {
        Err(err) => format!("LOUD-ERROR({err})"),
        Ok(ReportedUsage::Unknown) => "Unknown".to_string(),
        Ok(ReportedUsage::Invalid { detail }) => format!("Invalid({detail})"),
        Ok(ReportedUsage::Known(usage)) => format!(
            "Known(in={:?}, out={:?}, provenance={:?})",
            usage.input_tokens, usage.output_tokens, usage.provenance
        ),
    }
}

fn main() {
    println!("== P0 sanity: valid streams (harness proof) ==");
    let anthropic_valid = finish_anthropic(&anthropic_stream(r#"{"output_tokens":9}"#));
    println!("anthropic valid stream      -> {}", describe(&anthropic_valid));
    let google_valid = finish_google(&google_stream(
        r#"{"promptTokenCount":88,"candidatesTokenCount":19}"#,
    ));
    println!("google valid stream         -> {}", describe(&google_valid));

    println!("\n== P1 anthropic streaming, contract-violating usage fragments ==");
    for (label, usage) in [
        ("negative output_tokens", r#"{"output_tokens":-5}"#),
        ("float output_tokens", r#"{"output_tokens":1.5}"#),
        ("string output_tokens", r#"{"output_tokens":"9"}"#),
    ] {
        let report = finish_anthropic(&anthropic_stream(usage));
        println!("anthropic stream {label:26} -> {}", describe(&report));
    }
    // Negative in the INPUT half (message_start), valid output later.
    let mut body = String::new();
    body.push_str("event: message_start\ndata: ");
    body.push_str(r#"{"type":"message_start","message":{"usage":{"input_tokens":-3}}}"#);
    body.push_str("\n\n");
    body.push_str("event: message_stop\ndata: ");
    body.push_str(r#"{"type":"message_stop"}"#);
    body.push_str("\n\n");
    let report = finish_anthropic(&body);
    println!("anthropic stream negative input half   -> {}", describe(&report));

    println!("\n== P2 google streaming, contract-violating usageMetadata ==");
    for (label, usage) in [
        ("negative candidatesTokenCount", r#"{"promptTokenCount":88,"candidatesTokenCount":-7}"#),
        ("float promptTokenCount", r#"{"promptTokenCount":8.5,"candidatesTokenCount":7}"#),
        ("string candidatesTokenCount", r#"{"promptTokenCount":88,"candidatesTokenCount":"7"}"#),
    ] {
        let report = finish_google(&google_stream(usage));
        println!("google stream {label:31} -> {}", describe(&report));
    }

    println!("\n== P3 openai-completions streaming (control): same violations ==");
    for (label, usage) in [
        ("negative prompt_tokens", r#"{"prompt_tokens":-3,"completion_tokens":5}"#),
        ("float completion_tokens", r#"{"prompt_tokens":3,"completion_tokens":5.5}"#),
    ] {
        let report = finish_openai(&openai_stream(usage));
        println!("openai stream {label:26} -> {}", describe(&report));
    }

    println!("\n== P4 anthropic mixed: invalid fragment AFTER a valid one ==");
    // First delta valid (9), second delta violates (negative) — if the
    // invalid fragment is dropped, the finished fact silently keeps 9 and
    // reports Reported, hiding the violation entirely.
    let mut body = String::new();
    body.push_str("event: message_start\ndata: ");
    body.push_str(r#"{"type":"message_start","message":{"usage":{"input_tokens":25}}}"#);
    body.push_str("\n\n");
    for usage in [r#"{"output_tokens":9}"#, r#"{"output_tokens":-4}"#] {
        body.push_str("event: message_delta\ndata: ");
        body.push_str(&format!(
            r#"{{"type":"message_delta","delta":{{}},"usage":{usage}}}"#
        ));
        body.push_str("\n\n");
    }
    body.push_str("event: message_stop\ndata: ");
    body.push_str(r#"{"type":"message_stop"}"#);
    body.push_str("\n\n");
    let report = finish_anthropic(&body);
    println!("anthropic stream valid-then-invalid delta -> {}", describe(&report));

    println!("\n== P5 Estimated-state ledger round-trip (doc §10 claims coverage) ==");
    estimated_roundtrip();

    println!("\n== P6 FinalSnapshot fold: identical numbers, different provenance ==");
    // Latent edge: fold() compares FULL equality including provenance, so an
    // identical-numbers repeat with a different provenance is treated as a
    // conflict in FinalSnapshot mode (not an idempotent no-op).
    let mut folder = lingxi_kernel::usage::UsageFolder::new(
        lingxi_kernel::usage::UsageAggregationMode::FinalSnapshot,
    );
    let fact = |provenance| ModelCallUsage {
        input_tokens: Some(10),
        output_tokens: Some(3),
        cache_read_tokens: None,
        cache_write_tokens: None,
        reasoning_tokens: None,
        provenance,
    };
    folder.fold(fact(UsageProvenance::Reported)).expect("first");
    let outcome = folder.fold(fact(UsageProvenance::Estimated { basis: "chars/4".into() }));
    println!(
        "FinalSnapshot fold reported-then-estimated same numbers -> {:?}",
        outcome.map_err(|e| e.to_string())
    );
}

fn estimated_roundtrip() {
    let rt = tokio::runtime::Runtime::new().expect("probe runtime");
    rt.block_on(async {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r05t07-probe-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("probe dir");
        let db_path = dir.join("runs.db");
        let db = lingxi_adapters::storage::RunDatabase::open(
            &db_path,
            lingxi_adapters::storage::StoreOptions::default(),
        )
        .await
        .expect("open");
        let record = ModelCallUsageRecord {
            session_id: None,
            run_id: None,
            attempt: None,
            model_call_id: "probe-est-mc0001".to_string(),
            purpose: "chat".to_string(),
            origin: "user".to_string(),
            parent_run_id: None,
            cause_ref: None,
            provider: "p".to_string(),
            model: "m".to_string(),
            protocol: "openai-completions".to_string(),
            usage: Some(ModelCallUsage {
                input_tokens: Some(7),
                output_tokens: Some(2),
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                provenance: UsageProvenance::Estimated {
                    basis: "probe-basis".to_string(),
                },
            }),
            invalid_detail: None,
            transport_attempts: 1,
            cost_basis: None,
        };
        db.record_model_call_usage(record.clone(), 1).await.expect("write");
        let rows = db
            .query_model_call_usage(ModelUsageQuery::default())
            .await
            .expect("query");
        let got = rows.first().expect("one row");
        println!(
            "estimated round-trip -> usage_state preserved: {} (basis {:?})",
            matches!(
                got.usage.as_ref().and_then(|u| match &u.provenance {
                    UsageProvenance::Estimated { basis } => Some(basis.clone()),
                    _ => None,
                }),
                Some(basis) if basis == "probe-basis"
            ),
            got.usage.as_ref().map(|u| &u.provenance)
        );
        db.close().await.expect("close");
        let _ = std::fs::remove_dir_all(dir);
    });
}
