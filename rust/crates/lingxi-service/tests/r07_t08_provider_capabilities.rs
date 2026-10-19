//! 对照原供应商纯声明核对完整能力集合；不把声明检查当作供应商执行成功。
use std::collections::BTreeSet;

use lingxi_service::{
    provider_probe::builtin_provider_definitions, provider_routes::summary_payload,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};

#[test]
fn all_builtin_summary_media_bindings_exactly_preserve_original_declarations_and_lanes() {
    // 独立期望来自lib/providers及原Registry映射，不调用被测媒体绑定函数生成。
    // 36条原叶及额外3身份的逐行来源见provider-capability-expectations1；原账不改。
    type Expected<'a> = (&'a str, &'a [(&'a str, &'a str, Option<&'a str>)]);
    let expected: &[Expected<'_>] = &[
        (
            "agnes",
            &[
                ("imageGeneration", "agnes", None),
                ("videoGeneration", "agnes", None),
            ],
        ),
        ("anthropic", &[]),
        ("baichuan", &[]),
        ("baidu-cloud", &[]),
        (
            "dashscope",
            &[
                ("imageGeneration", "dashscope", None),
                ("speechGeneration", "dashscope", None),
                ("speechRecognition", "dashscope", None),
            ],
        ),
        ("dashscope-coding", &[]),
        ("deepseek", &[]),
        ("deepseek-responses", &[]),
        ("fireworks", &[]),
        ("gemini", &[("imageGeneration", "gemini", None)]),
        ("groq", &[("speechGeneration", "groq", None)]),
        ("hunyuan", &[]),
        ("infini", &[]),
        ("kimi-coding", &[]),
        ("mimo", &[("speechRecognition", "mimo", None)]),
        (
            "mimo-token-plan",
            &[("speechRecognition", "mimo-token-plan", None)],
        ),
        (
            "minimax",
            &[
                ("imageGeneration", "minimax", None),
                ("speechGeneration", "minimax", None),
            ],
        ),
        // 原叶写“仅聊天”，但原manifest及Registry均注册此图像lane；保留既有能力和原矛盾。
        (
            "minimax-token-plan",
            &[("imageGeneration", "minimax", Some("minimax-token-plan"))],
        ),
        ("mistral", &[]),
        ("modelscope", &[]),
        ("moonshot", &[]),
        ("ollama", &[]),
        (
            "openai",
            &[
                ("imageGeneration", "openai", None),
                ("speechGeneration", "openai", None),
                ("speechRecognition", "openai", None),
            ],
        ),
        (
            "openai-codex-oauth",
            &[("imageGeneration", "openai-codex-oauth", Some("codex-oauth"))],
        ),
        ("opencode", &[]),
        ("opencode-go", &[]),
        ("openrouter", &[]),
        ("perplexity", &[]),
        ("siliconflow", &[("speechGeneration", "siliconflow", None)]),
        ("stepfun", &[]),
        (
            "system-speech",
            &[
                ("speechGeneration", "system-speech", None),
                ("speechRecognition", "system-speech", None),
            ],
        ),
        ("together", &[]),
        ("volcengine", &[("imageGeneration", "volcengine", None)]),
        (
            "volcengine-coding",
            &[("imageGeneration", "volcengine", Some("volcengine-coding"))],
        ),
        (
            "volcengine-speech",
            &[("speechRecognition", "volcengine-speech", None)],
        ),
        ("xai", &[]),
        ("xai-oauth", &[]),
        ("zhipu", &[]),
        ("zhipu-coding", &[]),
    ];
    let expected_ids = expected.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>();
    let definitions = builtin_provider_definitions();
    let actual_ids = definitions
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(expected_ids.len(), 39);
    assert_eq!(actual_ids, expected_ids);
    let summary = summary_payload(&json!({"providers":{}})).unwrap();
    let providers = summary["providers"].as_object().unwrap();
    assert_eq!(providers.len(), 39);
    let mut differences = Vec::new();
    for (id, bindings) in expected {
        let mut wanted = bindings
            .iter()
            .map(|(capability, runtime, lane)| {
                let mut value = json!({"capability":capability,"runtime_provider_id":runtime});
                if let Some(lane) = lane {
                    value["credential_lane_id"] = json!(lane);
                }
                value
            })
            .collect::<Vec<_>>();
        let mut actual = providers[*id]["media_capability_bindings"]
            .as_array()
            .unwrap()
            .clone();
        wanted.sort_by_key(Value::to_string);
        actual.sort_by_key(Value::to_string);
        if actual != wanted {
            differences.push(json!({"provider":id,"expected":wanted,"actual":actual}));
        }
    }
    assert!(
        differences.is_empty(),
        "能力集合或凭据lane不一致：{differences:?}"
    );
}
