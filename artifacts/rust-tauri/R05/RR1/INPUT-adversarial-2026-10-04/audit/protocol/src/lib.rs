//! Read-only audit harness. Path dependencies reference the unmodified source
//! at d80737b6cb9186c8a18c0f35923aac00249d45c3. No real model APIs are called.
#[cfg(test)]
mod probes {
    use lingxi_adapters::models::{anthropic_messages as ant, google_generative_ai as goog,
        openai_completions as chat, openai_responses as resp, compat};
    use lingxi_adapters::models::streaming::SseEvent;
    use lingxi_kernel::model_exchange::*;
    use lingxi_kernel::ports::{ProviderTurn, ToolOutcome, ModelTurnDelta};
    use lingxi_kernel::toolcatalog::SchemaBudget;
    use lingxi_protocol::{ModelCallId, ToolCallId, ContentBlock, AssistantPhase};
    use serde_json::{json, Value};

    fn route(protocol: ProtocolFamily, provider: &str, model: &str) -> ResolvedModelRoute {
        ResolvedModelRoute { provider: provider.into(), model: model.into(),
            operation: ModelOperation::Chat, protocol, endpoint: format!("https://{provider}.example.test/v1"),
            credential: CredentialReference { provider: provider.into(), auth: CredentialAuthKind::ApiKey },
            config_generation: 1, group_id: None }
    }
    fn snapshot() -> ToolDeclarationSnapshot {
        ToolDeclarationSnapshot { catalog_generation: 3, declarations: vec![ToolDeclaration {
            target: "tool:first-party:read".into(), wire_name: "read".into(), description: "Read a file".into(),
            input_schema: lingxi_protocol::ToolSchemaDocument { dialect: "json-schema/2020-12".into(),
                schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}) },
        }] }
    }
    fn ev(v: Value) -> SseEvent { SseEvent { event: None, data: v.to_string() } }
    fn done() -> SseEvent { SseEvent { event: None, data: "[DONE]".into() } }
    fn call() -> ModelCallId { ModelCallId::new("audit-mc0001") }
    fn exchange(turn: ProviderTurn) -> ModelTurnInput {
        let (requests, content) = match turn {
            ProviderTurn::ToolRequests { requests, content } => (requests, content),
            other => panic!("expected tools: {other:?}"),
        };
        let mut input = ModelTurnInput::first_turn("read files", snapshot());
        let calls: Vec<_> = requests.iter().enumerate().map(|(i, request)| RequestedToolCall {
            tool_call_id: ToolCallId::new(format!("audit-tc{i:04}")),
            provider_call_id: request.provider_call_id.clone(), target: request.target.clone(),
            arguments: request.arguments.clone(), args_digest: request.args_digest.clone(), args_summary: None,
        }).collect();
        input.prior.push(ExchangeItem::AssistantTurn { call: call(), content, tool_calls: calls.clone() });
        for c in calls { input.prior.push(ExchangeItem::ToolResult { tool_call_id: c.tool_call_id,
            provider_call_id: c.provider_call_id, outcome: ToolOutcome::success_text("actual file bytes") }); }
        input
    }

    #[test]
    fn anthropic_documented_empty_signature_placeholder_accepts_final_signature() {
        let mut a = ant::MessagesStreamAccumulator::new();
        // Exact shape from Claude streaming docs: thinking starts with signature:"".
        a.handle_event(&ev(json!({"type":"message_start","message":{"content":[]}}))).unwrap();
        a.handle_event(&ev(json!({"type":"content_block_start","index":0,
            "content_block":{"type":"thinking","thinking":"","signature":""}}))).unwrap();
        a.handle_event(&ev(json!({"type":"content_block_delta","index":0,
            "delta":{"type":"thinking_delta","thinking":"check the file"}}))).unwrap();
        let result = a.handle_event(&ev(json!({"type":"content_block_delta","index":0,
            "delta":{"type":"signature_delta","signature":"valid-provider-signature"}})));
        assert!(result.is_ok(), "standard signature_delta must replace initial placeholder, actual={result:?}");
    }

    #[test]
    fn anthropic_empty_thinking_preserves_signature_on_next_request() {
        let original = json!({"type":"thinking","thinking":"","signature":"opaque-sig"});
        let body = json!({"content":[original.clone(),{"type":"tool_use","id":"t1","name":"read","input":{"path":"a"}}],"stop_reason":"tool_use"});
        let parsed = ant::parse_messages_response(&call(), &snapshot(), &body, &SchemaBudget::default()).unwrap();
        let rendered = ant::render_messages_request(&exchange(parsed.turn), &route(ProtocolFamily::AnthropicMessages,"anthropic-a","claude"),16384,false).unwrap();
        assert_eq!(rendered["messages"][1]["content"][0], original, "empty thinking can carry required state: {rendered}");
    }

    #[test]
    fn google_function_call_signature_survives_buffered_and_streamed_round_trip() {
        let original = json!({"functionCall":{"id":"g1","name":"read","args":{"path":"a"}},"thoughtSignature":"signed-function-part"});
        let body = json!({"candidates":[{"content":{"role":"model","parts":[original.clone()]},"finishReason":"STOP"}]});
        let parsed = goog::parse_generate_response(&call(), &snapshot(), &body, &SchemaBudget::default()).unwrap();
        let mut stream = goog::GenerateStreamAccumulator::new();
        stream.handle_event(&ev(body)).unwrap();
        let streamed = stream.finish(&call(), &snapshot(), &SchemaBudget::default()).unwrap();
        let r = route(ProtocolFamily::GoogleGenerativeAi,"google-a","gemini");
        let b1 = goog::render_generate_request(&exchange(parsed.turn), &r).unwrap();
        let b2 = goog::render_generate_request(&exchange(streamed.turn), &r).unwrap();
        println!("buffered={b1}\nstreamed={b2}");
        assert_eq!(b1, b2, "same production accumulator and buffered parser");
        assert_eq!(b1["contents"][1]["parts"][0], original, "required thoughtSignature must stay on function part");
    }

    #[test]
    fn google_parallel_results_form_one_reply_turn() {
        let body = json!({"candidates":[{"content":{"parts":[
            {"functionCall":{"id":"g1","name":"read","args":{"path":"a"}}},
            {"functionCall":{"id":"g2","name":"read","args":{"path":"b"}}}
        ]},"finishReason":"STOP"}]});
        let parsed = goog::parse_generate_response(&call(), &snapshot(), &body, &SchemaBudget::default()).unwrap();
        let rendered = goog::render_generate_request(&exchange(parsed.turn), &route(ProtocolFamily::GoogleGenerativeAi,"google-a","gemini")).unwrap();
        println!("parallel_result_payload={rendered}");
        assert_eq!(rendered["contents"].as_array().unwrap().len(), 3, "user -> model(two calls) -> user(two functionResponse parts)");
        assert_eq!(rendered["contents"][2]["parts"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn deepseek_stream_reasoning_replays_through_actual_renderer_and_compat() {
        let mut stream = chat::ChatStreamAccumulator::new();
        stream.handle_event(&ev(json!({"choices":[{"index":0,"delta":{"reasoning_content":"original provider reasoning"}}]}))).unwrap();
        stream.handle_event(&ev(json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"ds1","function":{"name":"read","arguments":"{\"path\":\"a\"}"}}]},"finish_reason":"tool_calls"}]}))).unwrap();
        stream.handle_event(&done()).unwrap();
        let parsed = stream.finish(&call(), &snapshot(), &SchemaBudget::default()).unwrap();
        let input = exchange(parsed.turn);
        assert!(matches!(&input.prior[0], ExchangeItem::AssistantTurn{content,..} if content.iter().any(|b| matches!(b,ContentBlock::Reasoning{text} if text=="original provider reasoning"))));
        let r = route(ProtocolFamily::OpenAiCompletions,"deepseek","deepseek-reasoner");
        let rendered = chat::render_chat_request(&input, &r, true).unwrap();
        let compatible = compat::apply_for_call(rendered, &r, Some(&compat::CompatCall::default())).unwrap();
        println!("actual_compatible_payload={compatible}");
        assert_eq!(compatible["messages"][1]["reasoning_content"], json!("original provider reasoning"));
    }

    #[test]
    fn duplicate_provider_ids_with_conflicting_arguments_reject_whole_batch() {
        let bodies = [
            json!({"choices":[{"message":{"tool_calls":[{"id":"same","function":{"name":"read","arguments":"{\"path\":\"a\"}"}},{"id":"same","function":{"name":"read","arguments":"{\"path\":\"b\"}"}}]},"finish_reason":"tool_calls"}]}),
            json!({"content":[{"type":"tool_use","id":"same","name":"read","input":{"path":"a"}},{"type":"tool_use","id":"same","name":"read","input":{"path":"b"}}],"stop_reason":"tool_use"}),
            json!({"status":"completed","output":[{"type":"function_call","call_id":"same","name":"read","arguments":"{\"path\":\"a\"}"},{"type":"function_call","call_id":"same","name":"read","arguments":"{\"path\":\"b\"}"}]}),
            json!({"candidates":[{"content":{"parts":[{"functionCall":{"id":"same","name":"read","args":{"path":"a"}}},{"functionCall":{"id":"same","name":"read","args":{"path":"b"}}}]},"finishReason":"STOP"}]}),
        ];
        let budget = SchemaBudget::default();
        let results = [chat::parse_chat_response(&call(),&snapshot(),&bodies[0],&budget), ant::parse_messages_response(&call(),&snapshot(),&bodies[1],&budget),resp::parse_responses_response(&call(),&snapshot(),&bodies[2],&budget),goog::parse_generate_response(&call(),&snapshot(),&bodies[3],&budget)];
        let summary: Vec<_> = results.iter().map(|r| match r { Ok(p) => format!("{:?}",p.turn), Err(e) => format!("rejected: {e:?}") }).collect();
        println!("duplicate_id_results={summary:#?}");
        assert!(results.iter().all(|r| r.is_err()), "conflicting same-ID calls must never be admitted");
    }

    #[test]
    fn anthropic_open_tool_block_cannot_close_as_a_completed_batch() {
        let mut a = ant::MessagesStreamAccumulator::new();
        for frame in [json!({"type":"message_start","message":{"content":[]}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"read","input":{}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"a\"}"}}),
            // Deliberately no content_block_stop / message_delta stop_reason.
            json!({"type":"message_stop"})] {
            a.handle_event(&ev(frame)).unwrap();
        }
        let r = a.finish(&call(),&snapshot(),&SchemaBudget::default());
        assert!(r.is_err(), "unclosed tool block is not a trustworthy terminal: actual={r:?}");
    }

    #[test]
    fn openai_done_without_finish_reason_does_not_make_final() {
        let mut a=chat::ChatStreamAccumulator::new();
        a.handle_event(&ev(json!({"choices":[{"index":0,"delta":{"content":"only partial"}}]}))).unwrap();
        a.handle_event(&done()).unwrap();
        let r=a.finish(&call(),&snapshot(),&SchemaBudget::default());
        assert!(!matches!(r, Ok(ref p) if matches!(p.turn,ProviderTurn::Final{..})), "no normal choice finish reason: actual={r:?}");
    }

    #[test]
    fn provider_opaque_state_never_replays_into_another_provider_of_same_family() {
        let body=json!({"content":[{"type":"thinking","thinking":"private-to-provider-a","signature":"provider-a-sig"},{"type":"tool_use","id":"a1","name":"read","input":{"path":"a"}}],"stop_reason":"tool_use"});
        let parsed=ant::parse_messages_response(&call(),&snapshot(),&body,&SchemaBudget::default()).unwrap();
        let from_a=exchange(parsed.turn);
        let to_b=ant::render_messages_request(&from_a,&route(ProtocolFamily::AnthropicMessages,"provider-b","other-model"),16384,true);
        println!("different_provider_result={to_b:?}");
        assert!(!matches!(to_b,Ok(ref b) if b.to_string().contains("provider-a-sig")),"family tag alone must not authorize cross-provider signature replay");
    }

    #[test]
    fn unclassified_live_text_is_not_persisted_as_final_answer() {
        use lingxi_service::streaming_norm::{DeltaNormalizer,NormEvent};
        let mut n=DeltaNormalizer::new(1);
        let events=n.feed(&ModelTurnDelta::Text("I will inspect the file now".into()));
        println!("pre_terminal_events={events:?}");
        assert!(!events.iter().any(|e| matches!(e,NormEvent::ModelDelta{phase:AssistantPhase::FinalAnswer,..})),"no terminal or semantic phase received yet");
    }

    #[test]
    fn thinking_only_and_opaque_only_do_not_form_final_answers() {
        let budget=SchemaBudget::default();
        let mut chat_stream=chat::ChatStreamAccumulator::new();
        chat_stream.handle_event(&ev(json!({"choices":[{"index":0,"delta":{"reasoning_content":"process only"},"finish_reason":"stop"}]}))).unwrap();
        chat_stream.handle_event(&done()).unwrap();
        let results=[
            chat_stream.finish(&call(),&snapshot(),&budget).unwrap(),
            ant::parse_messages_response(&call(),&snapshot(),&json!({"content":[{"type":"thinking","thinking":"process only","signature":"sig"}],"stop_reason":"end_turn"}),&budget).unwrap(),
            goog::parse_generate_response(&call(),&snapshot(),&json!({"candidates":[{"content":{"parts":[{"text":"process only","thought":true}]},"finishReason":"STOP"}]}),&budget).unwrap(),
            resp::parse_responses_response(&call(),&snapshot(),&json!({"status":"completed","output":[{"type":"reasoning","id":"r1","summary":[{"type":"summary_text","text":"process only"}],"encrypted_content":"opaque"}]}),&budget).unwrap(),
            lingxi_adapters::models::openai_codex_responses::parse_codex_response(&call(),&snapshot(),&json!({"status":"completed","output":[{"type":"reasoning","id":"r1","summary":[],"encrypted_content":"opaque"}]}),&budget).unwrap(),
        ];
        for (i,r) in results.iter().enumerate() {println!("process_only_family_{i}={:?}",r.turn);}
        assert!(results.iter().all(|r| !matches!(r.turn,ProviderTurn::Final{..})), "a completed request carrying only non-answer state is not a final answer");
    }

    #[test]
    fn responses_text_reasoning_and_tools_keep_original_relative_order() {
        let body=json!({"status":"completed","output":[
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"first text"}]},
            {"type":"reasoning","id":"r1","summary":[{"type":"summary_text","text":"then thought"}],"encrypted_content":"sig"},
            {"type":"function_call","call_id":"t1","name":"read","arguments":"{\"path\":\"a\"}"}
        ]});
        let parsed=resp::parse_responses_response(&call(),&snapshot(),&body,&SchemaBudget::default()).unwrap();
        let rendered=resp::render_responses_request(&exchange(parsed.turn),&route(ProtocolFamily::OpenAiResponses,"openai-a","model"),true).unwrap();
        println!("responses_replay_order={rendered}");
        assert_eq!(rendered["input"][1]["type"],json!("message"),"text preceded reasoning in the original response");
        assert_eq!(rendered["input"][2]["type"],json!("reasoning"));
    }

    #[test]
    fn positive_controls_normal_text_final_and_signature_without_placeholder_work() {
        let mut a=ant::MessagesStreamAccumulator::new();
        for frame in [json!({"type":"message_start","message":{"content":[]}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"process"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":"answer"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
            json!({"type":"message_stop"})] { a.handle_event(&ev(frame)).unwrap(); }
        assert!(matches!(a.finish(&call(),&snapshot(),&SchemaBudget::default()).unwrap().turn,ProviderTurn::Final{..}));
        let mut a=chat::ChatStreamAccumulator::new();
        a.handle_event(&ev(json!({"choices":[{"index":0,"delta":{"content":"real answer"},"finish_reason":"stop"}]}))).unwrap();
        a.handle_event(&done()).unwrap();
        assert!(matches!(a.finish(&call(),&snapshot(),&SchemaBudget::default()).unwrap().turn,ProviderTurn::Final{..}));
    }
}
