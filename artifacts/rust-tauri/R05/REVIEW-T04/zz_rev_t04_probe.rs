//! REVIEW-T04 R01 reviewer probes (NOT product code — written by the
//! independent reviewer, deleted after the run; source + output archived
//! under artifacts/rust-tauri/R05/REVIEW-T04/).
//!
//! Probe A (C01, reviewer-seeded): the C01 fragmentation-invariance harness
//! re-run with the REVIEWER's own PRNG (SplitMix64, not the implementer's
//! xorshift) and the REVIEWER's own seeds — proving the property is not an
//! artifact of the implementer's seed choice.
//!
//! Probe B (C12, reviewer-driven, adapter level): a raw-TCP gated SSE server
//! sends 200 + the FIRST frame, then holds the stream open behind a barrier.
//! The production `dispatch::drive_sse_stream` (real reqwest incremental
//! read) must deliver the first delta to the sink WHILE the barrier holds —
//! no full-buffer-then-reslice.

use lingxi_adapters::models::dispatch::{drive_sse_stream, FamilyStreamDrive};
use lingxi_adapters::models::openai_completions::ChatStreamAccumulator;
use lingxi_adapters::models::streaming::{SseDecoder, SseEvent};
use lingxi_adapters::models::{
    anthropic_messages::MessagesStreamAccumulator,
    openai_completions::ChatStreamAccumulator as OpenAiAcc,
};
use lingxi_kernel::model_exchange::{ToolDeclaration, ToolDeclarationSnapshot};
use lingxi_kernel::ports::{ModelTurnDelta, ProviderTurn, TurnDeltaSink, TurnDeltaSinkClosed};
use lingxi_kernel::toolcatalog::SchemaBudget;
use lingxi_protocol::{ModelCallId, ToolSchemaDocument, UsageRecord};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── fixtures (copied from the product test; the probe is self-contained) ──

fn snapshot() -> ToolDeclarationSnapshot {
    let tool = |target: &str, wire: &str, schema: serde_json::Value| ToolDeclaration {
        target: target.to_string(),
        wire_name: wire.to_string(),
        description: format!("{wire} tool"),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".to_string(),
            schema,
        },
    };
    ToolDeclarationSnapshot {
        catalog_generation: 7,
        declarations: vec![
            tool(
                "tool:first-party:read",
                "read",
                serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            ),
            tool(
                "tool:first-party:write",
                "write",
                serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
            ),
        ],
    }
}

fn call() -> ModelCallId {
    ModelCallId::new("run_revt04-mc0001")
}

fn anthropic_c01_frames() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"type":"message_start","message":{"id":"msg_c01","model":"m","usage":{"input_tokens":41}}}),
        serde_json::json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"先想一下"}}),
        serde_json::json!({"type":"ping"}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"🤔 方案"}}),
        serde_json::json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-C01-签名"}}),
        serde_json::json!({"type":"content_block_stop","index":0}),
        serde_json::json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        serde_json::json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"回答："}}),
        serde_json::json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"你好，世界 🌍"}}),
        serde_json::json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"！"}}),
        serde_json::json!({"type":"content_block_stop","index":1}),
        serde_json::json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_c01_a","name":"read"}}),
        serde_json::json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}),
        serde_json::json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"note-一.txt\"}"}}),
        serde_json::json!({"type":"content_block_stop","index":2}),
        serde_json::json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"toolu_c01_b","name":"write"}}),
        serde_json::json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"o.txt\",\"content\":\"x\"}"}}),
        serde_json::json!({"type":"content_block_stop","index":3}),
        serde_json::json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":17}}),
        serde_json::json!({"type":"message_stop"}),
    ]
}

fn openai_c01_frames() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"先推理"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"content":"正文一 🚀"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"read","arguments":"{\"path\":"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_b","type":"function","function":{"name":"write","arguments":"{\"path\":\"o.txt\","}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"note-二.txt\"}"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"reasoning_content":"再推理 🧠"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"\"content\":\"y\"}"}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{"content":"正文二"},"finish_reason":null}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        serde_json::json!({"id":"chatcmpl-c01","choices":[],"usage":{"prompt_tokens":29,"completion_tokens":13}}),
    ]
}

fn anthropic_body(frames: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for frame in frames {
        let ty = frame["type"].as_str().expect("typed frame");
        body.push_str(&format!("event: {ty}\ndata: {frame}\n\n"));
    }
    body
}

fn openai_body(frames: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    body
}

// ── the reviewer's own PRNG (SplitMix64) and seeds ─────────────────────────

struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

struct StreamOutcome {
    deltas: Vec<ModelTurnDelta>,
    turn: ProviderTurn,
    usage: Option<UsageRecord>,
}

fn decode_fragmented(body: &[u8], cuts: &[usize], label: &str) -> Vec<SseEvent> {
    let mut decoder = SseDecoder::new();
    let mut events = Vec::new();
    let mut at = 0;
    for cut in cuts.iter().copied().chain(std::iter::once(body.len())) {
        assert!(at <= cut && cut <= body.len(), "{label}: cut order");
        let chunk = &body[at..cut];
        at = cut;
        let produced = decoder
            .feed(chunk)
            .unwrap_or_else(|err| panic!("{label}: decode failed at byte {cut}: {err}"));
        events.extend(produced);
    }
    let finish = decoder
        .finish()
        .unwrap_or_else(|err| panic!("{label}: finish failed: {err}"));
    assert!(
        !finish.discarded_partial,
        "{label}: a complete fixture never discards a partial frame"
    );
    events.extend(finish.events);
    events
}

fn accumulate_anthropic(events: &[SseEvent], label: &str) -> StreamOutcome {
    let mut accumulator = MessagesStreamAccumulator::new();
    let mut deltas = Vec::new();
    for event in events {
        let emitted = accumulator
            .handle_event(event)
            .unwrap_or_else(|err| panic!("{label}: accumulate failed: {err}"));
        deltas.extend(emitted);
    }
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .unwrap_or_else(|err| panic!("{label}: close failed: {err}"));
    StreamOutcome {
        deltas,
        turn: parsed.turn,
        usage: parsed.usage,
    }
}

fn accumulate_openai(events: &[SseEvent], label: &str) -> StreamOutcome {
    let mut accumulator = OpenAiAcc::new();
    let mut deltas = Vec::new();
    for event in events {
        let emitted = accumulator
            .handle_event(event)
            .unwrap_or_else(|err| panic!("{label}: accumulate failed: {err}"));
        deltas.extend(emitted);
    }
    let parsed = accumulator
        .finish(&call(), &snapshot(), &SchemaBudget::default())
        .unwrap_or_else(|err| panic!("{label}: close failed: {err}"));
    StreamOutcome {
        deltas,
        turn: parsed.turn,
        usage: parsed.usage,
    }
}

/// Reviewer-seeded C01: one-shot reference vs reviewer's random multi-cuts
/// (SplitMix64, chunk sizes 1..=57, 300 rounds per seed, TWO reviewer seeds).
#[test]
fn rev_c01_fragmentation_invariance_reviewer_seeds() {
    for (family, body, anthropic) in [
        ("anthropic", anthropic_body(&anthropic_c01_frames()), true),
        ("openai", openai_body(&openai_c01_frames()), false),
    ] {
        let run = |cuts: &[usize], label: &str| {
            let events = decode_fragmented(body.as_bytes(), cuts, label);
            if anthropic {
                accumulate_anthropic(&events, label)
            } else {
                accumulate_openai(&events, label)
            }
        };
        let reference = run(&[body.len()], "one-shot");
        for seed in [0x5EED_2026_1003_0001_u64, 0xBADA_55DE_ADBE_EF99_u64] {
            let mut rng = SplitMix64(seed);
            for round in 0..300 {
                let mut cuts = Vec::new();
                let mut at = 0usize;
                while at < body.len() {
                    at += 1 + (rng.next() % 57) as usize;
                    cuts.push(at.min(body.len()));
                }
                let label = format!("{family}-reviewer-seed-{seed:016x}-round{round}");
                let outcome = run(&cuts, &label);
                assert_eq!(reference.deltas, outcome.deltas, "{label}: deltas diverge");
                assert_eq!(reference.turn, outcome.turn, "{label}: turn diverges");
                assert_eq!(reference.usage, outcome.usage, "{label}: usage diverges");
            }
        }
        // Semantic pins on the reference (same fixture expectations as the
        // product test, independently re-stated).
        match &reference.turn {
            ProviderTurn::ToolRequests { requests, .. } => assert_eq!(requests.len(), 2),
            other => panic!("{family}: expected ToolRequests, got {other:?}"),
        }
    }
}

// ── Probe B: reviewer-driven anti-fake-streaming at the wire level ─────────

struct RecordingSink {
    seen: std::sync::Mutex<Vec<ModelTurnDelta>>,
    first: tokio::sync::Notify,
}

impl TurnDeltaSink for RecordingSink {
    fn emit<'a>(
        &'a self,
        delta: ModelTurnDelta,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), TurnDeltaSinkClosed>> + Send + 'a>,
    > {
        Box::pin(async move {
            let mut seen = self.seen.lock().expect("seen");
            let was_empty = seen.is_empty();
            seen.push(delta);
            drop(seen);
            if was_empty {
                self.first.notify_one();
            }
            Ok(())
        })
    }
}

/// A gated raw-TCP SSE server: 200 + first frame flushed, then HELD until
/// the reviewer releases the barrier. Proves the production read path
/// (reqwest chunk → SseDecoder → accumulator → sink) delivers the first
/// delta BEFORE the response body completes.
#[tokio::test]
async fn rev_c12_first_delta_arrives_while_the_stream_is_held_open() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let flushed = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let (srv_flushed, srv_release) = (flushed.clone(), release.clone());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        // Read the request head (bounded; test equipment).
        let mut raw = Vec::new();
        loop {
            let mut chunk = [0_u8; 2048];
            let n = tokio::time::timeout(std::time::Duration::from_secs(10), socket.read(&mut chunk))
                .await
                .expect("server read stalled")
                .expect("server read");
            if n == 0 {
                panic!("client closed before the request head completed");
            }
            raw.extend_from_slice(&chunk[..n]);
            if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
            assert!(raw.len() < 64 * 1024, "request head too large");
        }
        // No Content-Length: the body is close-delimited — the production
        // shape of the product's own GatedSse stub.
        let first_frame = "data: {\"id\":\"x\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"评审首片 🧪\"},\"finish_reason\":null}]}\n\n";
        socket
            .write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{first_frame}")
                    .as_bytes(),
            )
            .await
            .expect("write head+first");
        socket.flush().await.expect("flush");
        srv_flushed.notify_one();
        // HOLD until the reviewer releases. If the client went away, fail.
        srv_release.notified().await;
        let rest = "data: {\"id\":\"x\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"屏障后\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"x\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        socket.write_all(rest.as_bytes()).await.expect("write rest");
        let _ = socket.shutdown().await;
    });

    let sink = RecordingSink {
        seen: std::sync::Mutex::new(Vec::new()),
        first: tokio::sync::Notify::new(),
    };
    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{addr}/stream"))
        .send()
        .await
        .expect("response");
    assert_eq!(response.status(), 200);
    let mut drive = FamilyStreamDrive {
        accumulator: ChatStreamAccumulator::new(),
        sink: &sink,
    };
    flushed.notified().await;
    // No tokio::spawn (the drive borrows the on-stack sink, so it is not
    // 'static). Pin the drive future and race it against the first-delta
    // notification: if the drive future completes before any delta arrives
    // (biased first arm), the stream either ended early or was buffered —
    // both are exactly the fake-streaming failure this probe hunts.
    let mut drive_fut = std::pin::pin!(drive_sse_stream(response, &mut drive));
    tokio::select! {
        biased;
        r = &mut drive_fut => panic!(
            "drive settled before the first delta while the barrier held: {r:?}"
        ),
        arrived = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            sink.first.notified(),
        ) => {
            arrived.expect(
                "the first delta reached the sink while the stream was held open",
            );
        }
    }
    // THE PROOF: the first delta arrived while the server still holds the
    // barrier (rest structurally unsent), and the drive is still pending.
    {
        let seen = sink.seen.lock().expect("seen");
        assert!(
            matches!(&seen[0], ModelTurnDelta::Text(t) if t.contains("评审首片")),
            "first delta is the pre-barrier content: {seen:?}"
        );
    }

    release.notify_one();
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), &mut drive_fut)
        .await
        .expect("drive settles");
    result.expect("clean stream drives Ok");
    server.await.expect("server task");
    let seen = sink.seen.lock().expect("seen");
    let texts: Vec<&str> = seen
        .iter()
        .filter_map(|d| match d {
            ModelTurnDelta::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["评审首片 🧪", "屏障后"], "pre/post barrier order");
}
