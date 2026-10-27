//! 已送达事实在所有供应商中保持普通历史位置，不伪造assistant回复或system指令。
use lingxi_adapters::models::{
    anthropic_messages, google_generative_ai, openai_codex_responses, openai_completions,
    openai_responses,
};
use lingxi_kernel::model_exchange::{
    render_host_notice, CredentialAuthKind, CredentialReference, ExchangeItem, ModelOperation,
    ModelTurnInput, ProtocolFamily, ResolvedModelRoute, ToolDeclarationSnapshot,
};
use serde_json::Value;
fn route(protocol: ProtocolFamily) -> ResolvedModelRoute {
    ResolvedModelRoute {
        provider: "fixture".into(),
        model: "fixture-model".into(),
        operation: ModelOperation::Chat,
        protocol,
        endpoint: "https://example.invalid/v1".into(),
        credential: CredentialReference {
            provider: "fixture".into(),
            auth: CredentialAuthKind::ApiKey,
        },
        config_generation: 1,
        group_id: None,
    }
}
fn input() -> ModelTurnInput {
    let mut input = ModelTurnInput::first_turn("current-input", ToolDeclarationSnapshot::empty());
    input.system_prompt = Some("private-system-unmodified".into());
    input.prior = vec![
        ExchangeItem::UserTurn {
            text: "before-notice".into(),
        },
        ExchangeItem::HostNotice {
            delivery_id: "delivery-actual".into(),
            text: "delivered-body".into(),
        },
        ExchangeItem::UserTurn {
            text: "after-notice".into(),
        },
        ExchangeItem::CurrentSubmission,
    ];
    input
}
fn texts(value: &Value, result: &mut Vec<String>) {
    match value {
        Value::String(s) => result.push(s.clone()),
        Value::Array(values) => {
            for v in values {
                texts(v, result)
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                texts(v, result)
            }
        }
        _ => {}
    }
}
#[test]
fn five_transports_keep_host_notice_at_its_real_position_and_out_of_system() {
    let input = input();
    let bodies = [
        openai_completions::render_chat_request(
            &input,
            &route(ProtocolFamily::OpenAiCompletions),
            false,
        )
        .unwrap(),
        openai_responses::render_responses_request(
            &input,
            &route(ProtocolFamily::OpenAiResponses),
            false,
        )
        .unwrap(),
        openai_codex_responses::render_codex_request(
            &input,
            &route(ProtocolFamily::OpenAiCodexResponses),
        )
        .unwrap(),
        anthropic_messages::render_messages_request(
            &input,
            &route(ProtocolFamily::AnthropicMessages),
            1024,
            false,
        )
        .unwrap(),
        google_generative_ai::render_generate_request(
            &input,
            &route(ProtocolFamily::GoogleGenerativeAi),
        )
        .unwrap(),
    ];
    let expected = render_host_notice("delivery-actual", "delivered-body");
    for body in bodies {
        let mut all = Vec::new();
        texts(&body, &mut all);
        let before = all.iter().position(|s| s == "before-notice").unwrap();
        let notice = all.iter().position(|s| s == &expected).unwrap();
        let after = all.iter().position(|s| s == "after-notice").unwrap();
        let current = all.iter().position(|s| s == "current-input").unwrap();
        assert!(before < notice && notice < after && after < current);
        assert_eq!(all.iter().filter(|s| *s == &expected).count(), 1);
        assert!(!body["system"].to_string().contains("delivered-body"));
        assert!(!body["instructions"].to_string().contains("delivered-body"));
        assert!(!body["systemInstruction"]
            .to_string()
            .contains("delivered-body"));
    }
}
