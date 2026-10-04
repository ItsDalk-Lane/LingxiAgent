//! R05-T06 adapter-level evidence: the media & auxiliary OPERATION plane
//! (embedding / rerank / image / video / speech / transcription dialects).
//!
//! C01 (fidelity): every dialect's plan is asserted against the incumbent
//! TypeScript sources (`core/model-operation-client.ts`,
//! `core/media-adapters/*.ts`, `core/speech-recognition/adapters.ts`) —
//! URL byte-exact, headers (case-insensitive names) exact, JSON bodies
//! `serde_json::Value`-equal, and the configuration/refusal-surface error
//! texts VERBATIM. Golden size values (e.g. 2K+3:2 → 2048x1360) are
//! mirrored from the incumbent's `resolution-tiers.ts` search and were
//! recomputed independently.
//!
//! C02 (behavioral honesty): loopback raw-TCP stubs are the far end of
//! the wire (the same posture as r05_t05_timeouts.rs): the openai image
//! happy path proves bytes-out = bytes-in; the minimax 200-embedded
//! envelope fails loudly; the bigasr status HEADER gates the answer; a
//! 32 MiB+1 body trips the mid-read cap with exactly ONE request on the
//! wire; the agnes video query falls back to the legacy endpoint only
//! under the incumbent's exact conditions; the codex SSE aggregate
//! collects the image result.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_adapters::models::credentials::ApplicableAuth;
use lingxi_adapters::models::operations::embedding::{self, EmbeddingInputType, EmbeddingRequest};
use lingxi_adapters::models::operations::image::{self, ImageRequest};
use lingxi_adapters::models::operations::rerank::{self, RerankRequest};
use lingxi_adapters::models::operations::speech::{self, SpeechRequest};
use lingxi_adapters::models::operations::tiers::{
    self, FlexibleConstraints, OpenAiSizeInput, OpenAiSizeOptions,
};
use lingxi_adapters::models::operations::transcribe::{self, TranscriptionRequest};
use lingxi_adapters::models::operations::video::{self, AgnesVideoQuery, VideoRequest};
use lingxi_adapters::models::operations::{
    ImageReference, ImageSubmitOutcome, MediaProductRef, OperationDispatcher, OperationRequestPlan,
    TaskPollOutcome,
};
use lingxi_kernel::model_exchange::{
    CredentialAuthKind, CredentialReference, MediaGenerationKind, ModelOperation, ProtocolFamily,
    ResolvedModelRoute,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ── shared assembly ──────────────────────────────────────────────────────────

fn route_with(
    protocol: &str,
    endpoint: &str,
    model: &str,
    operation: ModelOperation,
    group_id: Option<&str>,
) -> ResolvedModelRoute {
    ResolvedModelRoute {
        provider: "stub.provider".to_string(),
        model: model.to_string(),
        operation,
        protocol: ProtocolFamily::parse(protocol).expect("a known protocol family"),
        endpoint: endpoint.to_string(),
        credential: CredentialReference {
            provider: "stub.provider".to_string(),
            auth: CredentialAuthKind::ApiKey,
        },
        config_generation: 1,
        group_id: group_id.map(str::to_string),
    }
}

fn embed_route(endpoint: &str, model: &str, protocol: &str) -> ResolvedModelRoute {
    route_with(protocol, endpoint, model, ModelOperation::Embedding, None)
}

fn rerank_route(endpoint: &str, model: &str, protocol: &str) -> ResolvedModelRoute {
    route_with(protocol, endpoint, model, ModelOperation::Rerank, None)
}

fn image_route(endpoint: &str, model: &str, protocol: &str) -> ResolvedModelRoute {
    route_with(
        protocol,
        endpoint,
        model,
        ModelOperation::MediaGeneration {
            kind: MediaGenerationKind::Image,
        },
        None,
    )
}

fn video_route(endpoint: &str, model: &str) -> ResolvedModelRoute {
    route_with(
        "agnes-videos",
        endpoint,
        model,
        ModelOperation::MediaGeneration {
            kind: MediaGenerationKind::Video,
        },
        None,
    )
}

fn speech_route(endpoint: &str, model: &str, protocol: &str) -> ResolvedModelRoute {
    route_with(
        protocol,
        endpoint,
        model,
        ModelOperation::MediaGeneration {
            kind: MediaGenerationKind::Speech,
        },
        None,
    )
}

fn transcribe_route(endpoint: &str, model: &str, protocol: &str) -> ResolvedModelRoute {
    route_with(
        protocol,
        endpoint,
        model,
        ModelOperation::SpeechRecognition,
        None,
    )
}

fn bearer() -> ApplicableAuth {
    ApplicableAuth::Bearer("sk-test".to_string())
}

fn no_auth() -> ApplicableAuth {
    ApplicableAuth::None
}

/// The plan's headers as a lowercased-name map (header NAME case is not
/// semantic; values are asserted byte-exact).
fn headers(plan: &OperationRequestPlan) -> BTreeMap<String, String> {
    plan.headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
        .collect()
}

fn body_json(plan: &OperationRequestPlan) -> serde_json::Value {
    serde_json::from_slice(&plan.body).expect("a JSON dialect body parses")
}

fn err_text(result: Result<OperationRequestPlan, lingxi_protocol::ProtocolError>) -> String {
    result.expect_err("the build must refuse").message
}

fn empty_defaults() -> serde_json::Value {
    serde_json::json!({})
}

fn image_req(prompt: &str) -> ImageRequest {
    ImageRequest {
        prompt: prompt.to_string(),
        ..Default::default()
    }
}

// ── C01: tiers (resolution-tiers.ts) ─────────────────────────────────────────

fn openai_size(
    size: Option<&str>,
    resolution: Option<&str>,
    ratio: Option<&str>,
    options: OpenAiSizeOptions,
) -> Result<Option<String>, lingxi_protocol::ProtocolError> {
    tiers::resolve_openai_image_size(&OpenAiSizeInput {
        size,
        resolution,
        ratio,
        provider_defaults: &empty_defaults(),
        options,
    })
}

fn flexible_options() -> OpenAiSizeOptions<'static> {
    OpenAiSizeOptions {
        source_name: "OpenAI image",
        flexible: true,
        supported_ratios: Some(&tiers::OPENAI_FLEXIBLE_IMAGE_RATIOS),
        supported_resolutions: Some(&tiers::OPENAI_FLEXIBLE_RESOLUTION_TIERS),
        default_ratio: Some("3:2"),
        default_resolution: Some("2K"),
        constraints: FlexibleConstraints::default(),
    }
}

fn codex_options() -> OpenAiSizeOptions<'static> {
    OpenAiSizeOptions {
        source_name: "Codex image",
        flexible: true,
        supported_ratios: Some(&tiers::OPENAI_FLEXIBLE_IMAGE_RATIOS),
        supported_resolutions: Some(&tiers::CODEX_IMAGE_RESOLUTION_TIERS),
        default_ratio: Some("3:2"),
        default_resolution: Some("2K"),
        constraints: FlexibleConstraints {
            max_edge: Some(2048),
            max_pixels: Some(2048 * 2048),
            ..FlexibleConstraints::default()
        },
    }
}

#[test]
fn c01_tiers_golden_sizes_match_the_incumbent_search() {
    // Independently recomputed from resolution-tiers.ts (the width-stepped
    // search with longEdgeError → ratioError → pixelScore ordering).
    let cases: &[(&str, &str, &str)] = &[
        ("2k", "3:2", "2048x1360"),
        ("1k", "16:9", "1072x624"),
        ("4k", "1:1", "2880x2880"),
        ("1k", "1:1", "1024x1024"),
        ("2k", "1:1", "2048x2048"),
        ("4k", "21:9", "3840x1648"),
        ("1k", "9:16", "608x1088"),
    ];
    for (tier, ratio, expected) in cases {
        assert_eq!(
            tiers::nearest_openai_flexible_size(
                tier,
                Some(ratio),
                "OpenAI image",
                FlexibleConstraints::default()
            )
            .expect("a supported tier+ratio resolves"),
            *expected,
            "{tier} {ratio}"
        );
    }
    // The codex constraint bag (maxEdge 2048 / maxPixels 2048²).
    let codex = FlexibleConstraints {
        max_edge: Some(2048),
        max_pixels: Some(2048 * 2048),
        ..FlexibleConstraints::default()
    };
    let codex_cases: &[(&str, &str, &str)] = &[
        ("2k", "3:2", "2048x1360"),
        ("1k", "1:1", "1024x1024"),
        ("2k", "9:16", "1152x2048"),
        ("4k", "21:9", "2048x880"),
        ("1k", "3:2", "1024x688"),
    ];
    for (tier, ratio, expected) in codex_cases {
        assert_eq!(
            tiers::nearest_openai_flexible_size(tier, Some(ratio), "Codex image", codex)
                .expect("a supported tier+ratio resolves"),
            *expected,
            "codex {tier} {ratio}"
        );
    }
    // The standard (dall-e-3-class) nearest-size table.
    for (ratio, expected) in [
        ("1:1", "1024x1024"),
        ("3:2", "1536x1024"),
        ("2:3", "1024x1536"),
        ("16:9", "1536x1024"),
    ] {
        assert_eq!(
            tiers::nearest_openai_standard_size(Some(ratio), "OpenAI image").expect("standard"),
            expected
        );
    }
}

#[test]
fn c01_tiers_refusal_texts_are_verbatim() {
    // normalizeResolutionTier: the literal `image ` prefix (NOT the
    // caller's sourceName — the incumbent quirk).
    assert_eq!(
        tiers::normalize_resolution_tier(Some("3K"), "resolution")
            .expect_err("3K refuses")
            .message,
        "image resolution \"3K\" is unsupported"
    );
    assert_eq!(
        tiers::normalize_resolution_tier(Some("5k"), "size")
            .expect_err("5k refuses")
            .message,
        "image size \"5k\" is unsupported"
    );
    // The supported-tier assert lists the RAW supported values.
    let err = openai_size(None, Some("4K"), Some("3:2"), codex_options()).expect_err("4K refuses");
    assert_eq!(
        err.message,
        "Codex image resolution \"4K\" is unsupported; supported resolutions: 1K, 2K"
    );
    // Flexible pixel-size validations, in the incumbent's check order.
    let err = openai_size(Some("1000x1024"), None, None, flexible_options())
        .expect_err("non-multiple-of-16 refuses");
    assert_eq!(
        err.message,
        "OpenAI image size \"1000x1024\" is unsupported: width and height must be multiples of 16"
    );
    let err = openai_size(Some("3904x1024"), None, None, flexible_options())
        .expect_err("over-max-edge refuses");
    assert_eq!(
        err.message,
        "OpenAI image size \"3904x1024\" is unsupported: maximum edge is 3840px"
    );
    let err = openai_size(Some("3840x1024"), None, None, flexible_options())
        .expect_err("over-max-ratio refuses");
    assert_eq!(
        err.message,
        "OpenAI image size \"3840x1024\" is unsupported: aspect ratio exceeds 3:1"
    );
    // 624x1024 = 638,976 px is genuinely below the 655,360 floor (the
    // boundary value 640x1024 itself passes — the incumbent check is strict
    // `<`/`>`, so the exact floor is legal).
    let err = openai_size(Some("624x1024"), None, None, flexible_options())
        .expect_err("under-min-pixels refuses");
    assert_eq!(
        err.message,
        "OpenAI image size \"624x1024\" is unsupported: total pixels must be between 655360 \
         and 8294400"
    );
    // A non-flexible (standard) explicit pixel size outside the table.
    let standard = OpenAiSizeOptions {
        flexible: false,
        supported_ratios: Some(&tiers::OPENAI_STANDARD_IMAGE_RATIOS),
        supported_resolutions: Some(&tiers::OPENAI_STANDARD_RESOLUTION_TIERS),
        default_ratio: Some("3:2"),
        default_resolution: Some("1K"),
        ..flexible_options()
    };
    let err = openai_size(Some("1000x1000"), None, None, standard).expect_err("refuses");
    assert_eq!(
        err.message,
        "OpenAI image size \"1000x1000\" is unsupported"
    );
    // An unsupported ratio names the RAW value.
    let err =
        openai_size(None, Some("1K"), Some("5:4"), flexible_options()).expect_err("ratio refuses");
    assert_eq!(err.message, "OpenAI image ratio \"5:4\" is unsupported");
}

// ── C01: embedding (model-operation-client.ts) ───────────────────────────────

fn embed_req(inputs: &[&str]) -> EmbeddingRequest {
    EmbeddingRequest {
        inputs: inputs.iter().map(|s| s.to_string()).collect(),
        dimensions: None,
        context_window: None,
        input_type: EmbeddingInputType::Document,
    }
}

#[test]
fn c01_embedding_openai_wire() {
    let route = embed_route(
        "https://api.openai.com/v1",
        "text-embedding-3-large",
        "openai-embeddings",
    );
    let mut request = embed_req(&["hello", "world"]);
    request.dimensions = Some(256);
    let plan = embedding::build_embedding(&route, &bearer(), &request).expect("build");
    assert_eq!(plan.method, "POST");
    assert_eq!(plan.url, "https://api.openai.com/v1/embeddings");
    assert_eq!(
        headers(&plan),
        BTreeMap::from([
            ("content-type".to_string(), "application/json".to_string()),
            ("authorization".to_string(), "Bearer sk-test".to_string()),
        ])
    );
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "text-embedding-3-large",
            "input": ["hello", "world"],
            "encoding_format": "float",
            "dimensions": 256,
        })
    );
    // operationUrl: a base already ending in /embeddings is kept as-is.
    let route = embed_route("https://proxy.test/embeddings", "m", "openai-embeddings");
    let plan = embedding::build_embedding(&route, &bearer(), &embed_req(&["x"])).expect("build");
    assert_eq!(plan.url, "https://proxy.test/embeddings");
}

#[test]
fn c01_embedding_ollama_wire() {
    // /v1 stripped, /api/embed appended; NO authorization header (the
    // keyless local contract strips it).
    let route = embed_route(
        "http://localhost:11434/v1",
        "qwen3-embedding:8b",
        "ollama-embed",
    );
    let plan = embedding::build_embedding(&route, &bearer(), &embed_req(&["hi"])).expect("build");
    assert_eq!(plan.url, "http://localhost:11434/api/embed");
    assert_eq!(
        headers(&plan),
        BTreeMap::from([("content-type".to_string(), "application/json".to_string())])
    );
    // num_ctx: derived = ceil((2*1.6+512)/1024)*1024 = 1024 → clamp floor 2048.
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "qwen3-embedding:8b",
            "input": ["hi"],
            "options": {"num_ctx": 2048},
        })
    );

    // A 2000-char input derives 4096; dimensions pass through.
    let long = "a".repeat(2000);
    let mut request = embed_req(&["x"]);
    request.inputs = vec![long];
    request.dimensions = Some(1024);
    let plan = embedding::build_embedding(&route, &bearer(), &request).expect("build");
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "qwen3-embedding:8b",
            "input": ["a".repeat(2000)],
            "dimensions": 1024,
            "options": {"num_ctx": 4096},
        })
    );

    // A base ending in /api gains only /embed; a declared window wins.
    let route = embed_route("http://localhost:11434/api", "m", "ollama-embed");
    let mut request = embed_req(&["x"]);
    request.context_window = Some(8192);
    let plan = embedding::build_embedding(&route, &no_auth(), &request).expect("build");
    assert_eq!(plan.url, "http://localhost:11434/api/embed");
    assert_eq!(
        body_json(&plan)["options"]["num_ctx"],
        serde_json::json!(8192)
    );
}

#[test]
fn c01_embedding_gemini_voyage_minimax_wire() {
    // gemini: the model id rides the URL VERBATIM (no percent-encoding).
    let route = embed_route(
        "https://generativelanguage.googleapis.com/v1beta",
        "text-embedding-004",
        "gemini-embed",
    );
    let mut request = embed_req(&["hello"]);
    request.dimensions = Some(768);
    let plan = embedding::build_embedding(&route, &bearer(), &request).expect("build");
    assert_eq!(
        plan.url,
        "https://generativelanguage.googleapis.com/v1beta/models/text-embedding-004:batchEmbedContents"
    );
    assert_eq!(headers(&plan)["x-goog-api-key"], "sk-test");
    assert!(!headers(&plan).contains_key("authorization"));
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "requests": [{
                "model": "models/text-embedding-004",
                "content": {"parts": [{"text": "hello"}]},
                "outputDimensionality": 768,
            }]
        })
    );

    // voyage: {base}/v1/embeddings + input_type + dimensions passthrough.
    let route = embed_route("https://api.voyageai.com", "voyage-3", "voyage-embeddings");
    let mut request = embed_req(&["hello"]);
    request.input_type = EmbeddingInputType::Query;
    request.dimensions = Some(512);
    let plan = embedding::build_embedding(&route, &bearer(), &request).expect("build");
    assert_eq!(plan.url, "https://api.voyageai.com/v1/embeddings");
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "voyage-3",
            "input": ["hello"],
            "input_type": "query",
            "dimensions": 512,
        })
    );

    // minimax: ORIGIN-ONLY url + the encodeURIComponent'd GroupId query.
    let mut route = embed_route(
        "https://api.minimaxi.com/anthropic",
        "embo-01",
        "minimax-embeddings",
    );
    route.group_id = Some("grp 1".to_string());
    let plan =
        embedding::build_embedding(&route, &bearer(), &embed_req(&["hello"])).expect("build");
    assert_eq!(
        plan.url,
        "https://api.minimaxi.com/v1/embeddings?GroupId=grp%201"
    );
    assert_eq!(
        body_json(&plan),
        serde_json::json!({"model": "embo-01", "texts": ["hello"], "type": "db"})
    );

    // The GroupId refusal is verbatim (and an EMPTY group id refuses too).
    route.group_id = None;
    assert_eq!(
        err_text(embedding::build_embedding(
            &route,
            &bearer(),
            &embed_req(&["x"])
        )),
        "MiniMax embeddings require a GroupId configured on the model entry (settings > \
         providers > model > GroupId)"
    );
    route.group_id = Some(String::new());
    assert_eq!(
        err_text(embedding::build_embedding(
            &route,
            &bearer(),
            &embed_req(&["x"])
        )),
        "MiniMax embeddings require a GroupId configured on the model entry (settings > \
         providers > model > GroupId)"
    );
}

#[test]
fn c01_embedding_validation_texts_are_verbatim() {
    let route = embed_route("https://api.openai.com/v1", "m", "openai-embeddings");
    // Count.
    assert_eq!(
        err_text(embedding::build_embedding(
            &route,
            &bearer(),
            &embed_req(&[])
        )),
        "texts must contain between 1 and 128 items"
    );
    let too_many = EmbeddingRequest {
        inputs: (0..129).map(|i| format!("t{i}")).collect(),
        ..embed_req(&[])
    };
    assert_eq!(
        err_text(embedding::build_embedding(&route, &bearer(), &too_many)),
        "texts must contain between 1 and 128 items"
    );
    // Per-item RAW UTF-16 length: 16001 astral chars = 32002 UTF-16 code
    // units > 32000, while chars().count() is only 16001 (the incumbent's
    // String.length semantics, not a code-point count).
    let astral = "\u{1D11E}".repeat(16_001);
    let request = EmbeddingRequest {
        inputs: vec![astral],
        ..embed_req(&[])
    };
    assert_eq!(
        err_text(embedding::build_embedding(&route, &bearer(), &request)),
        "texts[0] must be a non-empty string no longer than 32000 characters"
    );
    // Empty-after-trim.
    let request = EmbeddingRequest {
        inputs: vec!["   ".to_string()],
        ..embed_req(&[])
    };
    assert_eq!(
        err_text(embedding::build_embedding(&route, &bearer(), &request)),
        "texts[0] must be a non-empty string no longer than 32000 characters"
    );
    // The RAW total cap.
    let request = EmbeddingRequest {
        inputs: (0..16).map(|_| "a".repeat(32_000)).collect(),
        ..embed_req(&[])
    };
    assert_eq!(
        err_text(embedding::build_embedding(&route, &bearer(), &request)),
        "texts exceeds the total character limit"
    );
    // dimensions / contextWindow.
    let mut request = embed_req(&["x"]);
    request.dimensions = Some(0);
    assert_eq!(
        err_text(embedding::build_embedding(&route, &bearer(), &request)),
        "dimensions must be a positive integer"
    );
    let mut request = embed_req(&["x"]);
    request.context_window = Some(1_048_577);
    assert_eq!(
        err_text(embedding::build_embedding(&route, &bearer(), &request)),
        "contextWindow must be a positive integer"
    );
}

#[test]
fn c01_embedding_parse_contracts() {
    // openai: rows reorder by index; usage verbatim when an object.
    let route = embed_route("https://api.openai.com/v1", "m", "openai-embeddings");
    let request = embed_req(&["a", "b"]);
    let body = serde_json::json!({
        "data": [
            {"index": 1, "embedding": [0.25, 0.5]},
            {"index": 0, "embedding": [1.0, 2.0]},
        ],
        "usage": {"prompt_tokens": 3, "total_tokens": 3},
    });
    let outcome = embedding::parse_embedding(route.protocol, &request, &body).expect("parse");
    assert_eq!(outcome.vectors, vec![vec![1.0, 2.0], vec![0.25, 0.5]]);
    assert_eq!(
        outcome.usage,
        Some(serde_json::json!({"prompt_tokens": 3, "total_tokens": 3}))
    );
    // A non-object usage drops out (the incumbent's typeof check).
    let body = serde_json::json!({
        "data": [
            {"index": 0, "embedding": [1.0]},
            {"index": 1, "embedding": [2.0]},
        ],
        "usage": "weird",
    });
    let outcome = embedding::parse_embedding(route.protocol, &request, &body).expect("parse");
    assert_eq!(outcome.usage, None);

    // voyage: a missing index falls back to the row position.
    let body = serde_json::json!({
        "data": [{"embedding": [0.5]}, {"embedding": [0.25]}],
    });
    let outcome = embedding::parse_embedding(ProtocolFamily::VoyageEmbeddings, &request, &body)
        .expect("voyage positional");
    assert_eq!(outcome.vectors, vec![vec![0.5], vec![0.25]]);

    // minimax: a NUMBER status_code != 0 fails with the verbatim text; a
    // STRING status code does NOT fail (the incumbent's typeof check).
    let body = serde_json::json!({
        "base_resp": {"status_code": 1000, "status_msg": "boom"},
        "vectors": [[1.0], [2.0]],
    });
    let err = embedding::parse_embedding(ProtocolFamily::MinimaxEmbeddings, &request, &body)
        .expect_err("1000 fails");
    assert_eq!(err.message, "MiniMax embeddings failed: boom");
    let body = serde_json::json!({
        "base_resp": {"status_code": "1000", "status_msg": "boom"},
        "vectors": [[1.0], [2.0]],
        "total_tokens": 12,
    });
    let outcome = embedding::parse_embedding(ProtocolFamily::MinimaxEmbeddings, &request, &body)
        .expect("a string status code passes");
    assert_eq!(outcome.vectors, vec![vec![1.0], vec![2.0]]);
    // The minimax usage is SYNTHESIZED from the top-level total_tokens.
    assert_eq!(outcome.usage, Some(serde_json::json!({"total_tokens": 12})));

    // The requested dimensionality must be honored.
    let mut request = embed_req(&["a"]);
    request.dimensions = Some(3);
    let body = serde_json::json!({"data": [{"index": 0, "embedding": [1.0, 2.0]}]});
    let err = embedding::parse_embedding(route.protocol, &request, &body)
        .expect_err("dimension mismatch refuses");
    assert_eq!(
        err.message,
        "embedding dimensionality 2 does not honor the requested 3"
    );
}

// ── C01: speech wire (core/media-adapters/speech.ts) ────────────────────────

fn speech_req(text: &str) -> SpeechRequest {
    SpeechRequest {
        text: text.to_string(),
        voice: None,
        speed: None,
        format: None,
    }
}

#[test]
fn c01_speech_openai_wire() {
    let route = speech_route(
        "https://api.openai.test/v1",
        "gpt-4o-mini-tts",
        "openai-audio-speech",
    );
    let plan = speech::build_speech(&route, &bearer(), &speech_req("你好 world")).expect("plan");
    assert_eq!(plan.method, "POST");
    assert_eq!(plan.url, "https://api.openai.test/v1/audio/speech");
    assert_eq!(
        headers(&plan),
        BTreeMap::from([
            ("content-type".to_string(), "application/json".to_string()),
            ("authorization".to_string(), "Bearer sk-test".to_string()),
        ])
    );
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "gpt-4o-mini-tts",
            "input": "你好 world",
            "voice": "alloy",
            "response_format": "mp3",
            "speed": 1,
        })
    );
    // The effective format feeds the product MIME (never the response's
    // content-type); an out-of-set format falls back to mp3; speed clamps
    // into [0.25, 4.0].
    let mut request = speech_req("hi");
    request.format = Some("wav".to_string());
    request.voice = Some("  ".to_string());
    request.speed = Some(0.1);
    let plan = speech::build_speech(&route, &bearer(), &request).expect("plan");
    assert_eq!(body_json(&plan)["response_format"], "wav");
    assert_eq!(body_json(&plan)["voice"], "alloy");
    assert_eq!(body_json(&plan)["speed"], 0.25);
    assert_eq!(speech::effective_openai_speech_format(Some("wav")), "wav");
    assert_eq!(speech::effective_openai_speech_format(Some("m4a")), "mp3");
    assert_eq!(speech::openai_speech_mime_for_format("wav"), "audio/wav");
    // An empty text refuses verbatim (the media manager's guard).
    let err = err_text(speech::build_speech(&route, &bearer(), &speech_req("")));
    assert_eq!(err, "prompt is required");
}

#[test]
fn c01_speech_minimax_wire_and_group_id_refusal() {
    let route = speech_route(
        "https://api.minimax.test/anything",
        "speech-02-hd",
        "minimax-tts",
    );
    let refusal = err_text(speech::build_speech(&route, &bearer(), &speech_req("你好")));
    assert_eq!(
        refusal,
        "MiniMax speech requires a GroupId configured on the model entry (settings > providers \
         > model > GroupId)"
    );
    let mut route = route;
    route.group_id = Some("gp-77/7".to_string());
    let plan = speech::build_speech(&route, &bearer(), &speech_req("你好")).expect("plan");
    // Origin-only base + encodeURIComponent'd GroupId.
    assert_eq!(
        plan.url,
        "https://api.minimax.test/v1/t2a_v2?GroupId=gp-77%2F7"
    );
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "speech-02-hd",
            "text": "你好",
            "voice_setting": {"voice_id": "male-qn-qingse", "speed": 1},
            "audio_setting": {"format": "mp3"},
        })
    );
}

#[test]
fn c01_speech_dashscope_wire() {
    let route = speech_route(
        "https://dash.example/compatible-mode/v1",
        "qwen-tts",
        "dashscope-qwen-tts",
    );
    let plan = speech::build_speech(&route, &bearer(), &speech_req("read me")).expect("plan");
    assert_eq!(
        plan.url,
        "https://dash.example/api/v1/services/aigc/multimodal-generation/generation"
    );
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "qwen-tts",
            "input": {"text": "read me", "voice": "Cherry"},
        })
    );
}

#[test]
fn c01_minimax_speech_parse_hex_envelope_and_refusals() {
    // hex audio decodes STRICTLY to bytes.
    let body = serde_json::json!({
        "base_resp": {"status_code": 0},
        "data": {"audio": "00ff10"},
    });
    let product = speech::parse_minimax_speech(Some("flac"), &body).expect("parse");
    assert_eq!(
        product,
        MediaProductRef::Bytes {
            bytes: vec![0x00, 0xff, 0x10],
            mime: "audio/flac".to_string(),
        }
    );
    // A NUMBER status_code != 0 fails with the incumbent's text.
    let body = serde_json::json!({
        "base_resp": {"status_code": 1001, "status_msg": "quota exceeded"},
        "data": {"audio": "00"},
    });
    let err = speech::parse_minimax_speech(None, &body).expect_err("envelope fails");
    assert_eq!(err.message, "MiniMax speech failed: quota exceeded");
    // A STRING status code does NOT fail (the typeof check, ported).
    let body = serde_json::json!({
        "base_resp": {"status_code": "1001"},
        "data": {"audio": "ff"},
    });
    assert!(speech::parse_minimax_speech(None, &body).is_ok());
    // Empty audio and odd-length hex refuse.
    let body = serde_json::json!({"data": {"audio": ""}});
    let err = speech::parse_minimax_speech(None, &body).expect_err("empty audio");
    assert_eq!(err.message, "MiniMax speech returned no audio data");
    let body = serde_json::json!({"data": {"audio": "0f0"}});
    assert!(speech::parse_minimax_speech(None, &body).is_err());
    // The dashscope speech parse surfaces the SIGNED URL for the service's
    // egress-guarded download (never fetched here).
    let body = serde_json::json!({"output": {"audio": {"url": "https://signed.example/a.wav"}}});
    assert_eq!(
        speech::parse_dashscope_speech(&body).expect("parse"),
        MediaProductRef::Url {
            url: "https://signed.example/a.wav".to_string(),
            mime_hint: Some("audio/wav".to_string()),
        }
    );
}

// ── C01: transcription wire ──────────────────────────────────────────────────

fn audio_req() -> TranscriptionRequest {
    TranscriptionRequest {
        audio: vec![1, 2, 3, 4],
        mime: "audio/wav".to_string(),
        filename: "clip.wav".to_string(),
        language: Some("zh".to_string()),
    }
}

#[test]
fn c01_transcribe_openai_multipart_wire() {
    let route = transcribe_route(
        "https://api.openai.test/v1",
        "whisper-1",
        "openai-audio-transcriptions",
    );
    let plan = transcribe::build_transcription(&route, &bearer(), &audio_req()).expect("plan");
    assert_eq!(plan.url, "https://api.openai.test/v1/audio/transcriptions");
    let content_type = headers(&plan)["content-type"].clone();
    assert!(content_type.starts_with("multipart/form-data; boundary="));
    let body = String::from_utf8_lossy(&plan.body).into_owned();
    assert!(body.contains("name=\"model\"\r\n\r\nwhisper-1"), "{body}");
    assert!(body.contains("name=\"language\"\r\n\r\nzh"), "{body}");
    assert!(
        body.contains("filename=\"clip.wav\"\r\nContent-Type: audio/wav"),
        "{body}"
    );
    assert_eq!(headers(&plan)["authorization"], "Bearer sk-test");
    // The parse: `String(text || "").trim()`.
    let outcome = transcribe::parse_transcription(
        ProtocolFamily::OpenAiAudioTranscriptions,
        &serde_json::json!({"text": "  转写结果  "}),
    )
    .expect("parse");
    assert_eq!(outcome.text, "转写结果");
    assert_eq!(outcome.duration_ms, None);
}

#[test]
fn c01_transcribe_mimo_and_dashscope_wire() {
    let route = transcribe_route(
        "https://mimo.example/v1",
        "mimo-asr",
        "mimo-chat-completions-asr",
    );
    let plan = transcribe::build_transcription(&route, &bearer(), &audio_req()).expect("plan");
    assert_eq!(plan.url, "https://mimo.example/v1/chat/completions");
    // `api-key:` named header, NOT a bearer.
    assert_eq!(headers(&plan)["api-key"], "sk-test");
    let body = body_json(&plan);
    let data_url = body
        .pointer("/messages/0/content/0/input_audio/data")
        .and_then(|v| v.as_str())
        .expect("the data URL rides the envelope");
    assert!(data_url.starts_with("data:audio/wav;base64,"), "{data_url}");
    assert_eq!(
        body.pointer("/asr_options/language")
            .and_then(|v| v.as_str()),
        Some("zh")
    );
    assert!(body.get("stream").is_none(), "mimo carries no stream field");
    // An empty language falls back to "auto".
    let mut request = audio_req();
    request.language = Some(String::new());
    let plan = transcribe::build_transcription(&route, &bearer(), &request).expect("plan");
    assert_eq!(
        body_json(&plan)
            .pointer("/asr_options/language")
            .and_then(|v| v.as_str()),
        Some("auto")
    );

    let route = transcribe_route(
        "https://dash.example/compatible-mode/v1",
        "qwen3-asr-flash",
        "dashscope-qwen-asr-chat",
    );
    let plan = transcribe::build_transcription(&route, &bearer(), &audio_req()).expect("plan");
    // The transcribe plane is a PLAIN concat over the trimmed base (no
    // compatible-mode rewrite — the incumbent speech-recognition adapters
    // do not use the chat plane's join).
    assert_eq!(
        plan.url,
        "https://dash.example/compatible-mode/v1/chat/completions"
    );
    assert_eq!(headers(&plan)["authorization"], "Bearer sk-test");
    let body = body_json(&plan);
    assert_eq!(body["stream"], false);
    assert_eq!(body["asr_options"]["language"], "zh");
    assert_eq!(body["asr_options"]["enable_itn"], false);
    // The chat-content ladder: message wins, delta is the fallback.
    let outcome = transcribe::parse_transcription(
        ProtocolFamily::DashscopeQwenAsrChat,
        &serde_json::json!({
            "choices": [{"message": {"content": " message text "}, "delta": {"content": "delta"}}]
        }),
    )
    .expect("parse");
    assert_eq!(outcome.text, "message text");
    let outcome = transcribe::parse_transcription(
        ProtocolFamily::MimoChatCompletionsAsr,
        &serde_json::json!({
            "choices": [{"message": {"content": null}, "delta": {"content": " delta text "}}]
        }),
    )
    .expect("parse");
    assert_eq!(outcome.text, "delta text");
}

#[test]
fn c01_transcribe_bigasr_wire() {
    let route = transcribe_route("https://bigasr.example", "bigmodel", "volcengine-bigasr");
    let plan = transcribe::build_transcription(&route, &bearer(), &audio_req()).expect("plan");
    assert_eq!(
        plan.url,
        "https://bigasr.example/api/v3/auc/bigmodel/recognize/flash"
    );
    let head = headers(&plan);
    assert_eq!(head["x-api-key"], "sk-test");
    assert_eq!(head["x-api-resource-id"], "volc.bigasr.auc_turbo");
    assert_eq!(head["x-api-sequence"], "-1");
    assert!(
        head["x-api-request-id"].contains('-'),
        "a uuid-shaped request id: {}",
        head["x-api-request-id"]
    );
    let body = body_json(&plan);
    assert_eq!(
        body.pointer("/user/uid"),
        Some(&serde_json::json!("sk-test"))
    );
    use base64::Engine as _;
    assert_eq!(
        body.pointer("/audio/data").and_then(|v| v.as_str()),
        Some(
            base64::engine::general_purpose::STANDARD
                .encode([1u8, 2, 3, 4])
                .as_str()
        )
    );
    // The duration coercion: a JSON null reports 0.0 (Number(null) === 0).
    let outcome = transcribe::parse_transcription(
        ProtocolFamily::VolcengineBigAsr,
        &serde_json::json!({
            "result": {"text": " bigasr text "},
            "audio_info": {"duration": null},
        }),
    )
    .expect("parse");
    assert_eq!(outcome.text, "bigasr text");
    assert_eq!(outcome.duration_ms, Some(0.0));
}

// ── C01: image wire (families) ───────────────────────────────────────────────

#[test]
fn c01_image_openai_wire_and_parse() {
    let route = image_route("https://api.openai.test/v1", "gpt-image-2", "openai-images");
    let plan = image::build_image_submit(
        &route,
        &bearer(),
        &image_req("a red cube"),
        &empty_defaults(),
    )
    .expect("plan");
    assert_eq!(plan.url, "https://api.openai.test/v1/images/generations");
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "gpt-image-2",
            "prompt": "a red cube",
            "n": 1,
            "output_format": "jpeg",
            // The flexible default tier+ratio: 2K + 3:2.
            "size": "2048x1360",
        })
    );
    // dall-e-3: response_format b64_json, the three-size ratio table.
    let route = image_route("https://api.openai.test/v1", "dall-e-3", "openai-images");
    let mut request = image_req("poster");
    request.ratio = Some("16:9".to_string());
    let plan =
        image::build_image_submit(&route, &bearer(), &request, &empty_defaults()).expect("plan");
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "dall-e-3",
            "prompt": "poster",
            "n": 1,
            "response_format": "b64_json",
            "size": "1792x1024",
        })
    );
    // The parse: b64_json decodes STRICTLY with the format-derived MIME.
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode([9u8, 9, 9]);
    let body = serde_json::json!({"data": [{"b64_json": b64}]});
    let outcome =
        image::parse_image_submit(&route, &request, &empty_defaults(), &body).expect("parse");
    assert_eq!(
        outcome,
        ImageSubmitOutcome::Done {
            products: vec![MediaProductRef::Bytes {
                bytes: vec![9, 9, 9],
                mime: "image/png".to_string(),
            }]
        }
    );
    let err = image::parse_image_submit(
        &route,
        &request,
        &empty_defaults(),
        &serde_json::json!({"data": []}),
    )
    .expect_err("no images refuses");
    assert_eq!(err.message, "API returned no images");
}

#[test]
fn c01_image_volcengine_wire_and_parse() {
    let route = image_route(
        "https://ark.example/api/v3",
        "doubao-seedream-4-0-250828",
        "volcengine-images",
    );
    let plan =
        image::build_image_submit(&route, &bearer(), &image_req("山与湖"), &empty_defaults())
            .expect("plan");
    assert_eq!(plan.url, "https://ark.example/api/v3/images/generations");
    // seedream-4-0: no output_format knob, watermark defaults false, the
    // default tier is 4K at 3:2.
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "doubao-seedream-4-0-250828",
            "prompt": "山与湖",
            "response_format": "b64_json",
            "size": "3840x2560",
            "watermark": false,
        })
    );
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode([5u8, 6]);
    let outcome = image::parse_image_submit(
        &route,
        &image_req("山与湖"),
        &empty_defaults(),
        &serde_json::json!({"data": [{"b64_json": b64}]}),
    )
    .expect("parse");
    assert_eq!(
        outcome,
        ImageSubmitOutcome::Done {
            products: vec![MediaProductRef::Bytes {
                bytes: vec![5, 6],
                mime: "image/jpeg".to_string(),
            }]
        }
    );
}

#[test]
fn c01_image_minimax_refusals_envelope_and_parse() {
    let route = image_route("https://api.minimax.test", "image-01", "minimax-images");
    let mut request = image_req("x");
    request.size = Some("1024x1024".to_string());
    let err = err_text(image::build_image_submit(
        &route,
        &bearer(),
        &request,
        &empty_defaults(),
    ));
    assert_eq!(err, "MiniMax image size/resolution is unsupported");
    let mut request = image_req("x");
    request.ratio = Some("5:4".to_string());
    let err = err_text(image::build_image_submit(
        &route,
        &bearer(),
        &request,
        &empty_defaults(),
    ));
    assert_eq!(err, "MiniMax image ratio \"5:4\" is unsupported");
    // The plan: origin-normalized /v1 base + response_format base64.
    let plan = image::build_image_submit(&route, &bearer(), &image_req("x"), &empty_defaults())
        .expect("plan");
    assert_eq!(plan.url, "https://api.minimax.test/v1/image_generation");
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "image-01",
            "prompt": "x",
            "response_format": "base64",
            "aspect_ratio": "3:2",
        })
    );
    // The 200-embedded envelope fails loudly with the incumbent's text
    // (through the family dispatch).
    let err = image::parse_image_submit(
        &route,
        &image_req("x"),
        &empty_defaults(),
        &serde_json::json!({
            "base_resp": {"status_code": 1000, "status_msg": "content filtered"},
        }),
    )
    .expect_err("envelope refuses");
    assert_eq!(err.message, "MiniMax API error 1000: content filtered");
    // A URL answer surfaces as a Url product (the service downloads it).
    let outcome = image::parse_image_submit(
        &route,
        &image_req("x"),
        &empty_defaults(),
        &serde_json::json!({
            "data": {"image_urls": ["https://cdn.example/a.jpeg"]},
        }),
    )
    .expect("parse");
    assert_eq!(
        outcome,
        ImageSubmitOutcome::Done {
            products: vec![MediaProductRef::Url {
                url: "https://cdn.example/a.jpeg".to_string(),
                mime_hint: None,
            }]
        }
    );
}

#[test]
fn c01_image_dashscope_submit_pending_and_query_states() {
    let route = image_route(
        "https://dash.example/compatible-mode/v1",
        "wan2.6-t2i",
        "dashscope-images",
    );
    let plan =
        image::build_image_submit(&route, &bearer(), &image_req("雾中灯塔"), &empty_defaults())
            .expect("plan");
    assert_eq!(
        plan.url,
        "https://dash.example/api/v1/services/aigc/image-generation/generation"
    );
    assert_eq!(headers(&plan)["x-dashscope-async"], "enable");
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "wan2.6-t2i",
            "input": {"messages": [{"role": "user", "content": [{"text": "雾中灯塔"}]}]},
            "parameters": {"n": 1, "size": "2K", "aspect_ratio": "3:2"},
        })
    );
    // The submit answer: a task id (no images) is honestly Pending.
    let outcome = image::parse_image_submit(
        &route,
        &image_req("雾中灯塔"),
        &empty_defaults(),
        &serde_json::json!({
            "output": {"task_id": "tsk-7", "task_status": "PENDING"},
        }),
    )
    .expect("parse");
    assert_eq!(
        outcome,
        ImageSubmitOutcome::Pending {
            task_id: "tsk-7".to_string()
        }
    );
    // Neither images nor a task id refuses (never a fabricated id).
    let err = image::parse_image_submit(
        &route,
        &image_req("雾中灯塔"),
        &empty_defaults(),
        &serde_json::json!({
            "output": {"task_status": "PENDING"},
        }),
    )
    .expect_err("no task id refuses");
    assert!(err
        .message
        .contains("refusing instead of tracking a fabricated local id"));
    // The query plan.
    let plan = image::build_dashscope_image_query(&route, &bearer(), "tsk-7");
    assert_eq!(plan.url, "https://dash.example/api/v1/tasks/tsk-7");
    assert_eq!(headers(&plan)["authorization"], "Bearer sk-test");
    // The query states: non-terminal → Pending; FAILED → the provider's
    // message; SUCCEEDED with urls → Done; a terminal status WITHOUT
    // images stays honestly Pending.
    assert_eq!(
        image::parse_dashscope_image_query(&serde_json::json!({
            "output": {"task_status": "RUNNING"},
        }))
        .expect("parse"),
        TaskPollOutcome::Pending
    );
    assert_eq!(
        image::parse_dashscope_image_query(&serde_json::json!({
            "output": {"task_status": "FAILED"},
            "message": "InternalError",
        }))
        .expect("parse"),
        TaskPollOutcome::Failed {
            reason: "InternalError".to_string()
        }
    );
    assert_eq!(
        image::parse_dashscope_image_query(&serde_json::json!({
            "output": {"task_status": "SUCCEEDED"},
        }))
        .expect("parse"),
        TaskPollOutcome::Pending
    );
    assert_eq!(
        image::parse_dashscope_image_query(&serde_json::json!({
            "output": {"task_status": "SUCCEEDED", "results": [{"url": "https://oss.example/i.png"}]},
        }))
        .expect("parse"),
        TaskPollOutcome::Done {
            products: vec![MediaProductRef::Url {
                url: "https://oss.example/i.png".to_string(),
                mime_hint: None,
            }]
        }
    );
}

#[test]
fn c01_image_gemini_wire_and_remote_url_refusal() {
    let route = image_route(
        "https://gemini.example/v1beta",
        "gemini-3-pro-image-preview",
        "gemini-generate-content-image",
    );
    let mut request = image_req("portrait");
    request.references = vec![ImageReference::Bytes {
        bytes: vec![1, 2, 3, 4],
        mime: "image/png".to_string(),
        filename: "ref.png".to_string(),
    }];
    let plan =
        image::build_image_submit(&route, &bearer(), &request, &empty_defaults()).expect("plan");
    assert_eq!(
        plan.url,
        "https://gemini.example/v1beta/models/gemini-3-pro-image-preview:generateContent"
    );
    let body = body_json(&plan);
    assert_eq!(body["contents"][0]["parts"][0]["text"], "portrait");
    assert_eq!(
        body["contents"][0]["parts"][1]["inline_data"]["mime_type"],
        "image/png"
    );
    assert!(body["generationConfig"]["imageConfig"]["aspectRatio"]
        .as_str()
        .is_some());
    // A remote URL reaching the gemini dialect is the loud wiring refusal
    // (the SERVICE must pre-fetch through the egress guard).
    let mut request = image_req("portrait");
    request.references = vec![ImageReference::RemoteUrl(
        "https://remote.example/x.png".to_string(),
    )];
    let err = err_text(image::build_image_submit(
        &route,
        &bearer(),
        &request,
        &empty_defaults(),
    ));
    assert!(err.contains("egress guard"), "{err}");
    // The parse: inline_data bytes with the response's own MIME (through
    // the family dispatch).
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode([7u8]);
    let outcome = image::parse_image_submit(
        &route,
        &image_req("portrait"),
        &empty_defaults(),
        &serde_json::json!({
            "candidates": [{"content": {"parts": [
                {"thought": true, "text": "thinking…"},
                {"inlineData": {"mimeType": "image/png", "data": b64}},
            ]}}],
        }),
    )
    .expect("parse");
    assert_eq!(
        outcome,
        ImageSubmitOutcome::Done {
            products: vec![MediaProductRef::Bytes {
                bytes: vec![7],
                mime: "image/png".to_string(),
            }]
        }
    );
}

#[test]
fn c01_image_agnes_wire() {
    let route = image_route("https://agnes.example/v1", "agnes-image", "agnes-images");
    let plan = image::build_image_submit(
        &route,
        &bearer(),
        &image_req("still life"),
        &empty_defaults(),
    )
    .expect("plan");
    assert_eq!(plan.url, "https://agnes.example/v1/images/generations");
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "agnes-image",
            "prompt": "still life",
            "extra_body": {"response_format": "b64_json"},
            "size": "1152x768",
        })
    );
}

#[test]
fn c01_image_codex_sse_aggregate_parses() {
    // The codex SSE aggregate collects the image result and refuses an
    // empty aggregate with the incumbent's text.
    let sse = concat!(
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"image_generation_call\"}}\n\n",
        "event: response.output_image.delta\n",
        "data: {\"type\":\"response.output_image.delta\",\"delta\":\"QUJD\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"image_generation_call\",\"result\":\"QUJD\"}]}}\n\n",
    );
    let outcome = image::parse_openai_codex_image_sse(&image_req("x"), &empty_defaults(), sse)
        .expect("parse");
    assert_eq!(
        outcome,
        vec![MediaProductRef::Bytes {
            bytes: b"ABC".to_vec(),
            mime: "image/png".to_string(),
        }]
    );
    let err = image::parse_openai_codex_image_sse(
        &image_req("x"),
        &empty_defaults(),
        "data: {\"type\":\"response.completed\"}\n\n",
    )
    .expect_err("no images refuses");
    assert_eq!(err.message, "API returned no images");
}

// ── C01: video wire ──────────────────────────────────────────────────────────

#[test]
fn c01_video_submit_wire_and_parse() {
    let route = video_route("https://agnes.example/v1", "agnes-video");
    let plan = video::build_video_submit(
        &route,
        &bearer(),
        &VideoRequest {
            prompt: "a slow pan".to_string(),
            ..Default::default()
        },
        &empty_defaults(),
    )
    .expect("plan");
    assert_eq!(plan.url, "https://agnes.example/v1/videos");
    // Defaults: 3:2 → 1152x768; 5 s × 24 fps + 1 = 121 frames (8n+1).
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "agnes-video",
            "prompt": "a slow pan",
            "width": 1152,
            "height": 768,
            "frame_rate": 24,
            "num_frames": 121,
        })
    );
    // The frame rules refuse verbatim.
    let mut request = VideoRequest {
        prompt: "x".to_string(),
        ..Default::default()
    };
    request.num_frames = Some(120.0);
    let err = err_text(video::build_video_submit(
        &route,
        &bearer(),
        &request,
        &empty_defaults(),
    ));
    assert_eq!(
        err,
        "Agnes video num_frames \"120\" is unsupported; it must be 8n+1 between 81 and 441"
    );
    // The submit parse: the tracker id and the provider-facing id.
    let outcome = video::parse_video_submit(&serde_json::json!({
        "task_id": "track-1", "video_id": "vid-9",
    }))
    .expect("parse");
    assert_eq!(outcome.task_id, "track-1");
    assert_eq!(outcome.provider_task_id, "vid-9");
    let err = video::parse_video_submit(&serde_json::json!({"status": "ok"}))
        .expect_err("no task id refuses");
    assert!(err
        .message
        .contains("refusing instead of tracking a fabricated local id"));
    // The query states.
    assert_eq!(
        video::parse_agnes_video_query_body(&serde_json::json!({"status": "processing"})),
        TaskPollOutcome::Pending
    );
    // completed WITHOUT a video URL stays honestly pending.
    assert_eq!(
        video::parse_agnes_video_query_body(&serde_json::json!({"status": "completed"})),
        TaskPollOutcome::Pending
    );
    assert_eq!(
        video::parse_agnes_video_query_body(&serde_json::json!({
            "status": "failed", "error": {"message": "moderation"},
        })),
        TaskPollOutcome::Failed {
            reason: "moderation".to_string()
        }
    );
    assert_eq!(
        video::parse_agnes_video_query_body(&serde_json::json!({
            "status": "succeeded", "video_url": "https://cdn.example/v.mp4",
        })),
        TaskPollOutcome::Done {
            products: vec![MediaProductRef::Url {
                url: "https://cdn.example/v.mp4".to_string(),
                mime_hint: None,
            }]
        }
    );
}

// ── C01: rerank wire ─────────────────────────────────────────────────────────

#[test]
fn c01_rerank_wire_and_parse() {
    let route = rerank_route("https://api.cohere.test/v1", "rerank-v3.5", "cohere-rerank");
    let request = RerankRequest {
        query: "什么是灵犀".to_string(),
        documents: vec!["文档一".to_string(), "doc two".to_string()],
        top_n: None,
    };
    let plan = rerank::build_rerank(&route, &bearer(), &request).expect("plan");
    // The singular /rerank through the operationUrl dedup.
    assert_eq!(plan.url, "https://api.cohere.test/v1/rerank");
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "rerank-v3.5",
            "query": "什么是灵犀",
            "documents": ["文档一", "doc two"],
            "top_n": 2,
            "return_documents": false,
        })
    );
    // The dashscope model-prefix split: gte-rerank* takes the NATIVE
    // nested endpoint (origin + /api/v1/services/...); everything else
    // takes the compatible rewrite with the PLURAL /reranks.
    let route = rerank_route(
        "https://dash.example/compatible-mode/v1",
        "gte-rerank-v2",
        "dashscope-rerank",
    );
    let plan = rerank::build_rerank(&route, &bearer(), &request).expect("plan");
    assert_eq!(
        plan.url,
        "https://dash.example/api/v1/services/rerank/text-rerank/text-rerank"
    );
    assert_eq!(
        body_json(&plan),
        serde_json::json!({
            "model": "gte-rerank-v2",
            "input": {"query": "什么是灵犀", "documents": ["文档一", "doc two"]},
            "parameters": {"top_n": 2, "return_documents": false},
        })
    );
    let route = rerank_route(
        "https://dash.example/compatible-mode/v1",
        "qwen-rerank",
        "dashscope-rerank",
    );
    let plan = rerank::build_rerank(&route, &bearer(), &request).expect("plan");
    assert_eq!(plan.url, "https://dash.example/compatible-api/v1/reranks");
    // The parse: score-descending, index-ascending order; the dashscope
    // native nesting folds up.
    let body = serde_json::json!({
        "results": [
            {"index": 1, "relevance_score": 0.9},
            {"index": 0, "relevance_score": 0.9},
        ]
    });
    let outcome = rerank::parse_rerank(ProtocolFamily::CohereRerank, 2, 2, &body).expect("parse");
    assert_eq!(
        outcome.results,
        vec![
            rerank::RerankHit {
                index: 0,
                score: 0.9
            },
            rerank::RerankHit {
                index: 1,
                score: 0.9
            },
        ]
    );
    let body = serde_json::json!({
        "output": {"results": [{"index": 0, "relevance_score": 0.25}]},
        "usage": {"total_tokens": 9},
    });
    let outcome =
        rerank::parse_rerank(ProtocolFamily::DashscopeRerank, 1, 1, &body).expect("parse");
    assert_eq!(
        outcome.results,
        vec![rerank::RerankHit {
            index: 0,
            score: 0.25
        }]
    );
    assert_eq!(outcome.usage, Some(serde_json::json!({"total_tokens": 9})));
    // The bounds refuse verbatim.
    let mut bad = request.clone();
    bad.top_n = Some(3);
    let err = err_text(rerank::build_rerank(&route, &bearer(), &bad));
    assert_eq!(err, "topN must be within the document count");
}

// ── C02: behavioral honesty over loopback stubs ──────────────────────────────

/// One scripted per-path answer (the first matching path PREFIX wins).
struct PathStub {
    endpoint: String,
    hits: Arc<AtomicUsize>,
    paths: Arc<Mutex<Vec<String>>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

/// One scripted per-path answer: (path prefix, status, extra headers,
/// body).
type PathScript = Vec<(String, u16, Vec<(String, String)>, String)>;

impl PathStub {
    async fn start(script: PathScript) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("stub bind");
        let addr = listener.local_addr().expect("stub addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let script = Arc::new(script);
        let task_hits = Arc::clone(&hits);
        let task_paths: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let paths = Arc::clone(&task_paths);
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((mut socket, _)) = accepted else { break };
                let script = Arc::clone(&script);
                let hits = Arc::clone(&task_hits);
                let seen = Arc::clone(&task_paths);
                tokio::spawn(async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let head = read_head(&mut socket).await;
                    seen.lock().expect("paths").push(head.0.clone());
                    for (prefix, status, extra_headers, body) in script.iter() {
                        if head.0.starts_with(prefix.as_str()) {
                            let reason = if (200..300).contains(status) {
                                "OK"
                            } else {
                                "Error"
                            };
                            let mut response = format!(
                                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                                body.len()
                            );
                            for (name, value) in extra_headers {
                                response.push_str(&format!("{name}: {value}\r\n"));
                            }
                            response.push_str("\r\n");
                            response.push_str(body);
                            let _ = socket.write_all(response.as_bytes()).await;
                            let _ = socket.shutdown().await;
                            return;
                        }
                    }
                    panic!("stub: unscripted path {}", head.0);
                });
            }
        });
        Self {
            endpoint: format!("http://{addr}"),
            hits,
            paths,
            shutdown: Some(shutdown),
            task,
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn paths(&self) -> Vec<String> {
        self.paths.lock().expect("paths").clone()
    }

    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

/// Reads the request head (path + headers), then the declared body.
async fn read_head(socket: &mut tokio::net::TcpStream) -> (String, Vec<(String, String)>) {
    let mut raw = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut chunk))
            .await
            .expect("stub read stalled")
            .expect("stub read");
        if read == 0 {
            panic!("stub: connection closed before headers completed");
        }
        raw.extend_from_slice(&chunk[..read]);
        if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        assert!(raw.len() < 256 * 1024, "stub: header block too large");
    };
    let head = String::from_utf8(raw[..header_end].to_vec()).expect("utf8 headers");
    let mut lines = head.split("\r\n");
    let path = lines
        .next()
        .expect("request line")
        .split_whitespace()
        .nth(1)
        .expect("path")
        .to_string();
    let mut header_pairs = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().expect("numeric content-length");
            }
            header_pairs.push((name, value));
        }
    }
    let body_start = header_end + 4;
    while raw.len() - body_start < content_length {
        let mut chunk = [0_u8; 64 * 1024];
        let read = tokio::time::timeout(Duration::from_secs(30), socket.read(&mut chunk))
            .await
            .expect("stub body read stalled")
            .expect("stub body read");
        if read == 0 {
            panic!("stub: connection closed mid-body");
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    (path, header_pairs)
}

#[tokio::test]
async fn c02_openai_image_roundtrip_bytes_out_bytes_in() {
    use base64::Engine as _;
    let product_bytes = [7u8, 8, 9, 10];
    let b64 = base64::engine::general_purpose::STANDARD.encode(product_bytes);
    let body = format!("{{\"data\":[{{\"b64_json\":\"{b64}\"}}]}}");
    let stub = PathStub::start(vec![(
        "/images/generations".to_string(),
        200,
        Vec::new(),
        body,
    )])
    .await;
    let route = image_route(&stub.endpoint, "dall-e-3", "openai-images");
    let dispatcher = OperationDispatcher::new().expect("dispatcher");
    let plan = image::build_image_submit(
        &route,
        &bearer(),
        &image_req("wire proof"),
        &empty_defaults(),
    )
    .expect("plan");
    let answered = dispatcher
        .execute_json(&plan, None, &bearer())
        .await
        .expect("the stub answers");
    let outcome = image::parse_image_submit(
        &route,
        &image_req("wire proof"),
        &empty_defaults(),
        &answered,
    )
    .expect("parse");
    assert_eq!(
        outcome,
        ImageSubmitOutcome::Done {
            products: vec![MediaProductRef::Bytes {
                bytes: product_bytes.to_vec(),
                mime: "image/png".to_string(),
            }]
        }
    );
    assert_eq!(stub.hits(), 1);
    stub.stop().await;
}

#[tokio::test]
async fn c02_bigasr_status_header_gates_the_answer() {
    // 200 + a non-OK X-Api-Status-Code HEADER refuses; the header absent
    // (or the OK code) passes.
    let ok_body = "{\"result\":{\"text\":\"hdr ok\"},\"audio_info\":{\"duration\":1200}}";
    let stub = PathStub::start(vec![
        (
            "/bad/".to_string(),
            200,
            vec![("x-api-status-code".to_string(), "20010004".to_string())],
            ok_body.to_string(),
        ),
        (
            "/good/".to_string(),
            200,
            vec![("x-api-status-code".to_string(), "20000000".to_string())],
            ok_body.to_string(),
        ),
        ("/none/".to_string(), 200, Vec::new(), ok_body.to_string()),
    ])
    .await;
    let dispatcher = OperationDispatcher::new().expect("dispatcher");
    let host = stub.endpoint.trim_start_matches("http://").to_string();
    for (suffix, must_fail) in [("/bad/", true), ("/good/", false), ("/none/", false)] {
        let route = transcribe_route(
            &format!("http://{host}{suffix}"),
            "bigmodel",
            "volcengine-bigasr",
        );
        let plan = transcribe::build_transcription(&route, &bearer(), &audio_req()).expect("plan");
        let (body, response_headers) = dispatcher
            .execute_json_full(&plan, None, &bearer())
            .await
            .expect("the stub answers");
        let gated = transcribe::bigasr_status_ok(&response_headers);
        if must_fail {
            let err = gated.expect_err("the status header refuses");
            assert_eq!(err.message, "Volcengine transcription failed: 20010004");
        } else {
            gated.expect("the status header passes");
            let outcome = transcribe::parse_transcription(route.protocol, &body).expect("parse");
            assert_eq!(outcome.text, "hdr ok");
            assert_eq!(outcome.duration_ms, Some(1200.0));
        }
    }
    assert_eq!(stub.hits(), 3);
    stub.stop().await;
}

#[tokio::test]
async fn c02_operation_body_cap_trips_mid_read_with_one_request() {
    // 32 MiB + 1 byte trips the JSON cap DURING the read — the error says
    // refused-never-truncated and exactly ONE request hit the wire.
    let oversized = "x".repeat(32 * 1024 * 1024 + 1);
    let stub = PathStub::start(vec![(
        "/images/generations".to_string(),
        200,
        Vec::new(),
        oversized,
    )])
    .await;
    let route = image_route(&stub.endpoint, "dall-e-3", "openai-images");
    let dispatcher = OperationDispatcher::new().expect("dispatcher");
    let plan = image::build_image_submit(
        &route,
        &bearer(),
        &image_req("cap proof"),
        &empty_defaults(),
    )
    .expect("plan");
    let (error, _retryable) = dispatcher
        .execute_json(&plan, None, &bearer())
        .await
        .expect_err("the cap refuses");
    assert!(
        error
            .message
            .contains("exceeded the 33554432 byte cap mid-read"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("never truncated"),
        "{}",
        error.message
    );
    assert_eq!(stub.hits(), 1, "no second request after the cap");
    stub.stop().await;
}

#[tokio::test]
async fn c02_agnes_video_query_legacy_fallback_only_under_exact_conditions() {
    // Primary 2xx → answered, no legacy request; primary non-2xx + a
    // DISTINCT legacy id → the legacy endpoint answers; primary non-2xx
    // with NO fallback → the classified error.
    let done = "{\"status\":\"completed\",\"video_url\":\"https://cdn.example/v.mp4\"}".to_string();
    let stub = PathStub::start(vec![
        ("/agnesapi".to_string(), 200, Vec::new(), done.clone()),
        (
            "/only-primary/v1/videos/".to_string(),
            200,
            Vec::new(),
            done.clone(),
        ),
        (
            "/only-primary/".to_string(),
            500,
            Vec::new(),
            "{\"error\":\"boom\"}".to_string(),
        ),
    ])
    .await;
    let host = stub.endpoint.trim_start_matches("http://").to_string();
    let dispatcher = OperationDispatcher::new().expect("dispatcher");

    // 1) A 2xx primary answer settles without any legacy request.
    let route = video_route(&format!("http://{host}"), "agnes-video");
    let outcome = video::execute_agnes_video_query(
        &dispatcher,
        &route,
        &bearer(),
        &AgnesVideoQuery {
            task_id: "t-1".to_string(),
            legacy_task_id: Some("legacy-1".to_string()),
            model_name: None,
        },
        None,
    )
    .await
    .expect("primary answers");
    assert_eq!(
        outcome,
        TaskPollOutcome::Done {
            products: vec![MediaProductRef::Url {
                url: "https://cdn.example/v.mp4".to_string(),
                mime_hint: None,
            }]
        }
    );
    assert_eq!(stub.paths(), vec!["/agnesapi?video_id=t-1".to_string()]);

    // 2) A non-2xx primary + a distinct legacy id retries the legacy path
    //    (the legacy base is the /only-primary override + /v1).
    let route = video_route(&format!("http://{host}/only-primary"), "agnes-video");
    let outcome = video::execute_agnes_video_query(
        &dispatcher,
        &route,
        &bearer(),
        &AgnesVideoQuery {
            task_id: "t-2".to_string(),
            legacy_task_id: Some("legacy-2".to_string()),
            model_name: Some("agnes-video".to_string()),
        },
        None,
    )
    .await
    .expect("the legacy fallback answers");
    assert!(matches!(outcome, TaskPollOutcome::Done { .. }));
    assert_eq!(stub.paths().len(), 3);

    // 3) A non-2xx primary WITHOUT a fallback is the classified error.
    let (error, _retryable) = video::execute_agnes_video_query(
        &dispatcher,
        &route,
        &bearer(),
        &AgnesVideoQuery {
            task_id: "t-3".to_string(),
            legacy_task_id: None,
            model_name: None,
        },
        None,
    )
    .await
    .expect_err("no fallback classifies");
    assert!(
        error.message.contains("boom") || error.message.contains("500"),
        "{}",
        error.message
    );
    assert_eq!(stub.hits(), 4);
    stub.stop().await;
}
