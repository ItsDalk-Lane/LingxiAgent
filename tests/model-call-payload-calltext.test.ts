/**
 * Phase 6 MC-04 callText × Sensitive Payload Capture —— 标杆路径端到端。
 *
 * 覆盖（§一百三十五～§一百三十八/§一百五十一/§一百五十二/§一百三十）：
 * - 四层 capture（semantic_request / provider_request / provider_response /
 *   semantic_response）在 fake provider 下完整成立。
 * - 毒丸（Authorization/x-api-key/nested secret）不进 sink；普通 system/user/
 *   model 内容存活（§一百二十九）。
 * - Provider mapping 四协议（anthropic/openai-completions/openai-responses/
 *   codex）+ system merge remap + codex adapter_injected。
 * - Error matrix：HTTP 400/401/429/500 error body 安全捕获、network error 无
 *   provider_response、invalid JSON rawText 捕获。
 * - wire 等价：sink 开/关时发往 provider 的 body 逐字节一致；原请求对象不可变。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { callText } from "../core/llm-client.ts";
import { setModelCallPayloadSink } from "../lib/llm/model-call-payload-capture.ts";
import { createTestModelCallPayloadSink, installTestPayloadSink } from "../lib/llm/model-call-payload-testing.ts";
import { createSemanticInputProvenance, provenanceSection } from "../lib/llm/semantic-input-provenance.ts";

const MODEL = { id: "gpt-5-mini", provider: "openai", cost: null };
const BASE_URL = "https://example.test/v1";

const POISON_API_KEY = "sk-TOPSECRET-KEY-CALLTEXT-99887766";
const POISON_NESTED = "TOPSECRET_NESTED_BODY_SECRET_a1b2c3d4";
const POISON_BEARER = "TOPSECRET_CALLTEXT_BEARER_Z9Y8X7W6";
const NORMAL_SYSTEM = "NORMAL_SYSTEM_PROMPT_VISIBLE You are a helpful assistant.";
const NORMAL_USER = "NORMAL_USER_PROMPT 请帮我起个标题";
const NORMAL_OUTPUT = "NORMAL_MODEL_RESPONSE_VISIBLE 标题：灵犀";

function okFetchFor(api: string, headers: Record<string, string> = {}) {
  const usage = { prompt_tokens: 10, completion_tokens: 3, total_tokens: 13 };
  let body: Record<string, unknown>;
  if (api === "anthropic-messages") {
    body = { content: [{ type: "text", text: NORMAL_OUTPUT }], stop_reason: "end_turn", usage };
  } else if (api === "openai-responses" || api === "openai-codex-responses") {
    body = {
      output: [{ type: "message", role: "assistant", content: [{ type: "output_text", text: NORMAL_OUTPUT }] }],
      status: "completed",
      usage,
    };
  } else {
    body = { choices: [{ message: { content: NORMAL_OUTPUT }, finish_reason: "stop" }], usage };
  }
  return vi.fn(async () => new Response(JSON.stringify(body), { status: 200, headers }));
}

function okFetch(body: Record<string, unknown> = {}, headers: Record<string, string> = {}) {
  return vi.fn(async () => new Response(JSON.stringify({
    choices: [{ message: { content: NORMAL_OUTPUT }, finish_reason: "stop" }],
    usage: { prompt_tokens: 10, completion_tokens: 3, total_tokens: 13 },
    ...body,
  }), { status: 200, headers }));
}

function errorFetch(status: number, payload: unknown) {
  return vi.fn(async () => new Response(
    typeof payload === "string" ? payload : JSON.stringify(payload),
    { status, headers: { "Content-Type": "application/json" } },
  ));
}

function baseOptions(extra: Record<string, unknown> = {}) {
  return {
    baseUrl: BASE_URL,
    model: MODEL,
    apiKey: POISON_API_KEY,
    systemPrompt: NORMAL_SYSTEM,
    messages: [{ role: "user", content: NORMAL_USER }],
    ...extra,
  } as any;
}

describe("MC-04 callText × payload capture（四协议）", () => {
  let sink: ReturnType<typeof createTestModelCallPayloadSink>;

  beforeEach(() => {
    sink = installTestPayloadSink();
  });
  afterEach(() => {
    setModelCallPayloadSink(null);
    vi.unstubAllGlobals();
  });

  const protocols = [
    { api: "anthropic-messages", expected: { system: ["system"], messages: 1 } },
    { api: "openai-completions", expected: { system: ["messages", 0, "content"], messages: 2 } },
    { api: "openai-responses", expected: { system: ["instructions"], messages: 1 } },
  ];

  for (const { api, expected } of protocols) {
    it(`${api}：四层完整 + 毒丸替换 + 普通内容存活 + mapping`, async () => {
      vi.stubGlobal("fetch", okFetchFor(api));
      await callText(baseOptions({ api }));

      const [callId] = sink.callIds();
      expect(sink.sequenceForCall(callId)).toEqual([
        "semantic_request",
        "provider_request",
        "provider_response",
        "semantic_response",
      ]);

      // semantic_request：普通 system/user 存活
      const semantic = sink.semanticRequestForCall(callId)!;
      const semanticPayload = semantic.payload as any;
      expect(semanticPayload.inputShape).toBe("calltext");
      expect(semanticPayload.systemPrompt).toBe(NORMAL_SYSTEM);
      expect(semanticPayload.messages[0].content).toBe(NORMAL_USER);

      // provider_request：wire body 来自真实构造点；凭证替换
      const providerRequest = sink.providerRequestsForCall(callId)[0];
      expect(providerRequest.fidelity).toBe("runtime_exact");
      expect(providerRequest.visibility).toBe("full");
      const transport = (providerRequest.payload as any).transport;
      if (api === "anthropic-messages") {
        expect(transport.headers["x-api-key"]).toBe("<redacted:credential>");
      } else {
        expect(transport.headers.Authorization).toBe("<redacted:credential>");
      }
      expect(transport.headers["Content-Type"]).toBe("application/json");
      // mapping：systemPrompt → 真实 provider locator（§一百三十六）
      const provenance = providerRequest.providerRequestProvenance!;
      expect(provenance.protocol).toBe(api);
      const systemMapping = provenance.mappings.find((m) =>
        JSON.stringify(m.providerLocator?.path) === JSON.stringify(expected.system));
      expect(systemMapping).toBeDefined();
      // span 解析（§四十七：locator 作用于捕获副本）
      const systemText = api === "anthropic-messages"
        ? transport.body.system
        : (api === "openai-completions" ? transport.body.messages[0].content : transport.body.instructions);
      const span = systemMapping!.providerLocator!.span!;
      expect(systemText.slice(span.start, span.end)).toBe(NORMAL_SYSTEM);
      // messages[0] mapping
      const messageMapping = provenance.mappings.find((m) =>
        Array.isArray(m.providerLocator?.path)
        && m.providerLocator.path[0] === (api === "openai-responses" ? "input" : "messages")
        && m.providerLocator.path[1] === (api === "openai-completions" ? 1 : 0));
      expect(messageMapping).toBeDefined();

      // provider_response：parsed body（协议各自的正文位置）
      const providerResponse = sink.providerResponsesForCall(callId)[0];
      expect(providerResponse.fidelity).toBe("parsed_equivalent");
      const responseBody = (providerResponse.payload as any).body;
      const capturedOutput = api === "anthropic-messages"
        ? responseBody.content[0].text
        : (api === "openai-completions" ? responseBody.choices[0].message.content : responseBody.output[0].content[0].text);
      expect(capturedOutput).toBe(NORMAL_OUTPUT);

      // semantic_response：text + usage
      const semanticResponse = sink.semanticResponseForCall(callId)!;
      const responsePayload = semanticResponse.payload as any;
      expect(responsePayload.text).toBe(NORMAL_OUTPUT);
      expect(responsePayload.finishReason).toBe(
        api === "anthropic-messages" ? "end_turn" : (api === "openai-responses" ? "completed" : "stop"),
      );
      expect(responsePayload.completeness).toBe("complete");

      // 毒丸：API key 不进任何 record
      sink.assertNoSensitiveContent([POISON_API_KEY]);
    });
  }

  it("openai-codex-responses：空系统注入 adapter_injected + instructions mapping", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => {
      const payload = JSON.stringify({
        type: "response.completed",
        response: {
          output: [{ type: "message", role: "assistant", content: [{ type: "output_text", text: NORMAL_OUTPUT }] }],
          usage: { input: 1, output: 1 },
        },
      });
      const stream = new ReadableStream({
        start(controller) {
          controller.enqueue(new TextEncoder().encode(`data: ${payload}\n\n`));
          controller.close();
        },
      });
      return new Response(stream, { status: 200, headers: { "Content-Type": "text/event-stream" } });
    }));
    await callText(baseOptions({
      api: "openai-codex-responses",
      apiKey: null,
      systemPrompt: "",
      model: { id: "codex-test", provider: "openai", headers: { "chatgpt-account-id": "acct-77" } },
      messages: [{ role: "user", content: NORMAL_USER }],
    }));

    const [callId] = sink.callIds();
    const semantic = sink.semanticRequestForCall(callId)!;
    // 注入的默认 instruction 出现在语义层 systemPrompt（与 Phase 5 span 一致）
    expect((semantic.payload as any).systemPrompt).toContain("Hana's utility model");
    const provenance = semantic.semanticInputProvenance!;
    const injected = provenance.sections.find((s) => s.category === "adapter_injected");
    expect(injected).toBeDefined();
    const injectedText = (semantic.payload as any).systemPrompt.slice(injected!.locator.span!.start, injected!.locator.span!.end);
    expect(injectedText).toContain("Hana's utility model");

    const providerRequest = sink.providerRequestsForCall(callId)[0];
    const transport = (providerRequest.payload as any).transport;
    expect(transport.body.instructions).toContain("Hana's utility model");
    expect(transport.headers["chatgpt-account-id"]).toBe("acct-77"); // 非 secret 保留
    // SSE 响应 → stream_aggregate（§六十八）
    const providerResponse = sink.providerResponsesForCall(callId)[0];
    expect(providerResponse.fidelity).toBe("stream_aggregate");
  });

  it("system 消息 merge：caller provenance 随 merge remap 到 systemPrompt 根（§一百三十七）", async () => {
    vi.stubGlobal("fetch", okFetchFor("openai-responses"));
    const callerSystemText = "CUSTOM_SYSTEM_BLOCK_PART_1";
    // caller 按传入形状描述：system 消息 span + user 消息 index
    const callerProvenance = createSemanticInputProvenance("calltext", [
      provenanceSection(
        { root: "messages", path: [0], span: { start: 0, end: callerSystemText.length } },
        "session_instruction",
        { role: "system", precision: "exact" } as never,
      ),
      provenanceSection(
        { root: "messages", path: [1] },
        "current_user_input",
        { role: "user", precision: "exact" } as never,
      ),
    ]);
    await callText(baseOptions({
      api: "openai-responses",
      systemPrompt: "Base system. ",
      messages: [
        { role: "system", content: callerSystemText },
        { role: "user", content: NORMAL_USER },
      ],
      semanticInputProvenance: callerProvenance,
    }));
    const [callId] = sink.callIds();
    const semantic = sink.semanticRequestForCall(callId)!;
    const captured = semantic.payload as any;
    expect(captured.systemPrompt).toBe(`Base system. \n${callerSystemText}`);
    const provenance = semantic.semanticInputProvenance!;
    // merged system 内的 caller 段（provider mapping 到 instructions 的 span）
    const providerRequest = sink.providerRequestsForCall(callId)[0];
    const transport = (providerRequest.payload as any).transport;
    const mapping = providerRequest.providerRequestProvenance!.mappings
      .find((m) => m.providerLocator?.path[0] === "instructions"
        && transport.body.instructions.slice(m.providerLocator.span!.start, m.providerLocator.span!.end) === callerSystemText);
    expect(mapping).toBeDefined();
    void provenance;
  });

  it("HTTP 400/401/429/500：error body 安全捕获 + semantic_response 缺席", async () => {
    for (const status of [400, 401, 429, 500]) {
      sink.reset();
      vi.stubGlobal("fetch", errorFetch(status, {
        error: { message: `normal diagnostic for ${status}`, api_key: POISON_NESTED },
      }));
      await expect(callText(baseOptions({ api: "openai-completions" }))).rejects.toThrow();
      const [callId] = sink.callIds();
      expect(sink.semanticRequestForCall(callId)).not.toBeNull();
      expect(sink.providerRequestsForCall(callId)).toHaveLength(1);
      const response = sink.providerResponsesForCall(callId)[0];
      expect((response.payload as any).status).toBe(status);
      expect((response.payload as any).body.error.message).toBe(`normal diagnostic for ${status}`);
      expect(sink.semanticResponseForCall(callId)).toBeNull();
      sink.assertNoSensitiveContent([POISON_API_KEY, POISON_NESTED]);
    }
  });

  it("invalid JSON：rawText 以 runtime_exact 捕获（§六十七）", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("<html>not json</html>", { status: 502 })));
    await expect(callText(baseOptions({ api: "openai-completions" }))).rejects.toThrow();
    const [callId] = sink.callIds();
    const response = sink.providerResponsesForCall(callId)[0];
    expect(response.fidelity).toBe("runtime_exact");
    expect((response.payload as any).body).toContain("not json");
  });

  it("network error：无 provider_response record（§一百一十二）", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new TypeError("fetch failed"); }));
    await expect(callText(baseOptions({ api: "openai-completions" }))).rejects.toThrow();
    const [callId] = sink.callIds();
    expect(sink.providerRequestsForCall(callId)).toHaveLength(1);
    expect(sink.providerResponsesForCall(callId)).toHaveLength(0);
    expect(sink.semanticResponseForCall(callId)).toBeNull();
    expect(sink.sequenceForCall(callId)).toEqual(["semantic_request", "provider_request"]);
  });

  it("user abort：无 provider_response / semantic_response", async () => {
    const controller = new AbortController();
    vi.stubGlobal("fetch", vi.fn(async (_url: string, init: any) => {
      controller.abort();
      init.signal.throwIfAborted();
      throw new DOMException("aborted", "AbortError");
    }));
    await expect(callText(baseOptions({ api: "openai-completions", signal: controller.signal }))).rejects.toThrow();
    const [callId] = sink.callIds();
    expect(sink.providerResponsesForCall(callId)).toHaveLength(0);
    expect(sink.semanticResponseForCall(callId)).toBeNull();
  });

  it("sink 关闭：wire body 逐字节一致 + redactor 不运行（§一百三十/§一百三十三）", async () => {
    setModelCallPayloadSink(null);
    const bodies: string[] = [];
    const fetchOn = vi.fn(async (_url: any, init: any) => {
      bodies.push(init.body);
      return new Response(JSON.stringify({ choices: [{ message: { content: "ok" } }] }), { status: 200 });
    });
    const redaction = await import("../lib/llm/model-call-payload-redaction.ts");
    const spy = vi.spyOn(redaction, "sanitizeValueForCapture");
    vi.stubGlobal("fetch", fetchOn);
    await callText(baseOptions({ api: "openai-completions" }));
    expect(spy).not.toHaveBeenCalled();
    expect(sink.records).toHaveLength(0);
    const wireBodyWithoutSink = bodies[0];
    setModelCallPayloadSink(sink);
    vi.stubGlobal("fetch", fetchOn.mockClear());
    await callText(baseOptions({ api: "openai-completions" }));
    expect(fetchOn.mock.calls[0][1].body).toBe(wireBodyWithoutSink);
    expect(sink.records.length).toBeGreaterThan(0);
    spy.mockRestore();
  });

  it("sink throw：业务照常完成（§一百三十四）", async () => {
    setModelCallPayloadSink({
      handleModelCallPayloadRecord() { throw new Error("boom"); },
    });
    vi.stubGlobal("fetch", okFetch());
    const text = await callText(baseOptions({ api: "openai-completions" }));
    expect(text).toBe(NORMAL_OUTPUT);
  });

  it("用户文本中的 inline secret（Bearer/nested token）被脱敏，普通文本存活", async () => {
    vi.stubGlobal("fetch", okFetch());
    const secretUserText = `My key is Bearer ${POISON_BEARER} please help`;
    await callText(baseOptions({
      api: "openai-completions",
      messages: [{ role: "user", content: secretUserText }],
    }));
    const [callId] = sink.callIds();
    const semantic = sink.semanticRequestForCall(callId)!;
    const captured = semantic.payload as any;
    expect(captured.messages[0].content).toContain("My key is Bearer");
    expect(captured.messages[0].content).toContain("<redacted:secret>");
    expect(captured.systemPrompt).toBe(NORMAL_SYSTEM);
    sink.assertNoSensitiveContent([POISON_BEARER, POISON_API_KEY]);
  });
});
