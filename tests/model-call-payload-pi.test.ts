/**
 * Phase 6 Pi 路径 × Sensitive Payload Capture（MC-01/02/03，§一百三十九～§一百四十三）。
 *
 * 覆盖：
 *   - MC-01 semantic request/response capture（streamFn 边界 context + assembled
 *     message 的 text/thinking/toolCall）。
 *   - Provider hook 真实 capture：before_provider_request 的 event.payload =
 *     最终 body（runtime_exact）；after_provider_response 仅 status+headers
 *     （metadata_only）——hook fidelity 用 fake provider 锁定（§七十四/§七十五/§一百四十一）。
 *   - MC-02/03：options 无 onPayload → provider wire 显式 unavailable（§一百零三）。
 *   - google 协议：response hook 结构性缺失 → provider_response unavailable。
 *   - aborted partial：assembled 内容存在 → completeness=partial（§八十/§一百五十四）。
 *   - redacted_thinking：只保留结构标记（§一百一十）。
 */
import { afterEach, describe, expect, it } from "vitest";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import {
  installModelCallStreamObserver,
} from "../lib/pi-sdk/model-call-stream-observer.ts";
import { createModelCallObserverExtension } from "../lib/extensions/model-call-observer-ext.ts";
import { runWithModelCallScope } from "../lib/llm/model-call-scope.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { setModelCallPayloadSink } from "../lib/llm/model-call-payload-capture.ts";
import { createTestModelCallPayloadSink, installTestPayloadSink } from "../lib/llm/model-call-payload-testing.ts";

const POISON_API_KEY = "sk-PI-TOPSECRET-KEY-4455667788";
const NORMAL_SYSTEM = "NORMAL_PI_SYSTEM_PROMPT persona text";
const NORMAL_USER = "NORMAL_PI_USER_INPUT 今天天气怎么样";
const NORMAL_ASSISTANT = "NORMAL_PI_ASSISTANT_REPLY 今天晴。";

function assistantMessage(overrides: Record<string, any> = {}) {
  return {
    role: "assistant",
    content: [{ type: "text", text: NORMAL_ASSISTANT }],
    api: "openai-completions",
    provider: "test-provider",
    model: "test-model",
    usage: { input: 10, output: 5, cacheRead: 0, cacheWrite: 0, totalTokens: 15 },
    stopReason: "stop",
    timestamp: Date.now(),
    ...overrides,
  };
}

function streamOf(message: any) {
  const stream = createAssistantMessageEventStream();
  queueMicrotask(() => {
    if (message.stopReason === "error" || message.stopReason === "aborted") {
      stream.push({ type: "error", reason: message.stopReason, error: message } as any);
    } else {
      stream.push({ type: "done", reason: message.stopReason, message } as any);
    }
    stream.end();
  });
  return stream;
}

function fakeSession(streamFunction: any, overrides: Record<string, any> = {}) {
  return {
    agent: { streamFunction },
    sessionManager: {
      getSessionId: () => "sess-1",
      getSessionFile: () => "/tmp/sess-1.jsonl",
    },
    isCompacting: false,
    ...overrides,
  };
}

const MODEL = { id: "test-model", provider: "test-provider", api: "openai-completions" };

async function flushTerminal() {
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
}

describe("MC-01 Pi chat × payload capture", () => {
  let sink: ReturnType<typeof createTestModelCallPayloadSink>;
  afterEach(() => {
    setModelCallObserver(null);
    setModelCallPayloadSink(null);
  });

  it("semantic_request + semantic_response 捕获；正文经 Redactor，identity 对齐", async () => {
    sink = installTestPayloadSink();
    setModelCallObserver({ handleModelCallEvent() { /* observer 不参与本断言 */ } });
    const session = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(MODEL, {
      systemPrompt: NORMAL_SYSTEM,
      messages: [
        { role: "user", content: NORMAL_USER },
        { role: "toolResult", toolName: "weather", content: [{ type: "text", text: "sunny" }] },
      ],
      tools: [{ name: "weather", description: "get weather", parameters: { type: "object" } }],
    }, { onPayload: async () => undefined, onResponse: async () => undefined });
    await stream.result();
    await flushTerminal();

    const [callId] = sink.callIds();
    const semantic = sink.semanticRequestForCall(callId)!;
    const payload = semantic.payload as any;
    expect(payload.inputShape).toBe("chat_context");
    expect(payload.systemPrompt).toBe(NORMAL_SYSTEM);
    expect(payload.messages[0].content).toBe(NORMAL_USER);
    expect(payload.messages[1].toolName).toBe("weather");
    expect(payload.tools[0].name).toBe("weather");
    expect(semantic.semanticInputProvenance?.inputShape).toBe("chat_context");

    const response = sink.semanticResponseForCall(callId)!;
    const responsePayload = response.payload as any;
    expect(responsePayload.text).toBe(NORMAL_ASSISTANT);
    expect(responsePayload.finishReason).toBe("stop");
    expect(responsePayload.completeness).toBe("complete");

    // 无 provider hook 触发（fake streamFn 不调 onPayload）→ 该 options 有
    // onPayload → 不标 unavailable；但 hook 未实际触发 → 无 provider_request record
    expect(sink.providerRequestsForCall(callId)).toHaveLength(0);
  });

  it("before_provider_request：event.payload = 最终 body（runtime_exact）；after = metadata_only（§一百四十/§一百四十一）", async () => {
    sink = installTestPayloadSink();
    const extension = createModelCallObserverExtension();
    const handlers: Record<string, (event: any) => any> = {};
    extension({ on: (name: string, fn: any) => { handlers[name] = fn; } });

    const wireBody = {
      model: "test-model",
      messages: [{ role: "user", content: NORMAL_USER }],
      system: [{ type: "text", text: NORMAL_SYSTEM }],
      stream: true,
      // payload 不含凭证（vendor SDK fetch 层拼装）——但假如 provider 把 key
      // 塞进 body（防御纵深），redactor 也必须替换
      metadata: { api_key: POISON_API_KEY },
    };
    let capturedCallId: string | null = null;
    const session = fakeSession(async (model: any, context: any, options: any) => {
      // 模拟 pi-ai adapter：调用 onPayload（= extension 的 before_provider_request）
      await options?.onPayload?.(wireBody, model);
      await options?.onResponse?.({ status: 200, headers: { "x-request-id": "req-pi-1", "set-cookie": "TOPSECRET_SET_COOKIE_SESSION=1" } }, model);
      capturedCallId = (await import("../lib/llm/model-call-scope.ts")).currentModelCallScope()?.callId ?? null;
      return streamOf(assistantMessage());
    });
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(MODEL, {
      systemPrompt: NORMAL_SYSTEM,
      messages: [{ role: "user", content: NORMAL_USER }],
    }, {
      onPayload: async (payload: unknown, model: unknown) => handlers["before_provider_request"]({ type: "before_provider_request", payload }),
      onResponse: async (response: unknown, model: unknown) => { void model; handlers["after_provider_response"]({ type: "after_provider_response", ...response as object }); },
    });
    await stream.result();
    await flushTerminal();

    const callId = sink.callIds()[0];
    expect(capturedCallId).toBe(callId);

    const providerRequest = sink.providerRequestsForCall(callId)[0];
    expect(providerRequest.fidelity).toBe("runtime_exact");
    expect(providerRequest.providerRequestOrdinal).toBe(1);
    const body = (providerRequest.payload as any).transport.body;
    // 捕获的是 hook payload 本体（不是重建）：字段逐项对齐
    expect(body.model).toBe("test-model");
    expect(body.stream).toBe(true);
    expect(body.system[0].text).toBe(NORMAL_SYSTEM);
    // hook 不暴露 headers/endpoint → transport 只有 body（诚实）
    expect((providerRequest.payload as any).transport.headers).toBeUndefined();
    expect((providerRequest.payload as any).transport.url).toBeUndefined();
    // body 内 credential（防御纵深）替换
    expect(body.metadata.api_key).toBe("<redacted:credential>");

    const providerResponse = sink.providerResponsesForCall(callId)[0];
    expect(providerResponse.fidelity).toBe("metadata_only");
    expect(providerResponse.visibility).toBe("metadata_only");
    const responsePayload = providerResponse.payload as any;
    expect(responsePayload.status).toBe(200);
    expect(responsePayload.headers["x-request-id"]).toBe("req-pi-1");
    expect(responsePayload.headers["set-cookie"]).toBe("<redacted:credential>");

    sink.assertNoSensitiveContent([POISON_API_KEY, "TOPSECRET_SET_COOKIE_SESSION"]);
  });

  it("MC-02/MC-03：options 无 onPayload → provider wire 显式 unavailable（§一百零三/§一百四十二）", async () => {
    sink = installTestPayloadSink();
    const session = fakeSession(async () => streamOf(assistantMessage()), { isCompacting: true });
    installModelCallStreamObserver(session);

    // MC-03 native summarization：无 onPayload
    const stream = await session.agent.streamFunction(MODEL, {
      systemPrompt: "summarize the conversation",
      messages: [{ role: "user", content: NORMAL_USER }],
    }, {});
    await stream.result();
    await flushTerminal();

    const [callId] = sink.callIds();
    const [req, res] = [
      sink.recordsOfKind(callId, "provider_request")[0],
      sink.recordsOfKind(callId, "provider_response")[0],
    ];
    expect(req.visibility).toBe("unavailable");
    expect(req.payload).toBeNull();
    expect(res.visibility).toBe("unavailable");
    // semantic request 仍捕获（§八十三：provenance 精度低 ≠ payload 不可见）
    expect(sink.semanticRequestForCall(callId)).not.toBeNull();
    expect(sink.semanticResponseForCall(callId)).not.toBeNull();
  });

  it("MC-02：显式 ALS scope 的 callId 接管 capture session（同身份）", async () => {
    sink = installTestPayloadSink();
    const session = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);

    const stream = await runWithModelCallScope({
      callId: "mc_explicit_scope_1",
      source: { subsystem: "compaction", operation: "compact", surface: "desktop", trigger: "system" },
      attribution: { kind: "session", agentId: "agent-2" },
    }, () => session.agent.streamFunction(MODEL, {
      systemPrompt: NORMAL_SYSTEM,
      messages: [{ role: "user", content: NORMAL_USER }],
    }, {}));
    await stream.result();
    await flushTerminal();

    const callId = sink.callIds()[0];
    expect(callId).toBe("mc_explicit_scope_1");
    expect(sink.semanticRequestForCall(callId)).not.toBeNull();
  });

  it("google 协议：response hook 结构性缺失 → provider_response unavailable（audit §1.2）", async () => {
    sink = installTestPayloadSink();
    const googleModel = { id: "gemini-test", provider: "google", api: "google-generative-ai" };
    const session = fakeSession(async (model: any) => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(googleModel, {
      systemPrompt: NORMAL_SYSTEM,
      messages: [{ role: "user", content: NORMAL_USER }],
    }, { onPayload: async () => undefined, onResponse: async () => undefined });
    await stream.result();
    await flushTerminal();

    const [callId] = sink.callIds();
    const responseRecords = sink.recordsOfKind(callId, "provider_response");
    expect(responseRecords).toHaveLength(1);
    expect(responseRecords[0].visibility).toBe("unavailable");
    expect(responseRecords[0].fidelity).toBe("opaque");
  });

  it("aborted partial：assembled 内容存在 → completeness=partial（§八十/§一百五十四）", async () => {
    sink = installTestPayloadSink();
    const session = fakeSession(async () => streamOf(assistantMessage({
      stopReason: "aborted",
      content: [{ type: "text", text: NORMAL_ASSISTANT }],
    })));
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(MODEL, {
      systemPrompt: NORMAL_SYSTEM,
      messages: [{ role: "user", content: NORMAL_USER }],
    }, {});
    await stream.result();
    await flushTerminal();

    const [callId] = sink.callIds();
    const response = sink.semanticResponseForCall(callId)!;
    expect((response.payload as any).completeness).toBe("partial");
    expect((response.payload as any).text).toBe(NORMAL_ASSISTANT);
  });

  it("toolCall + thinking + redacted_thinking 捕获（§一百零七/§一百一十）", async () => {
    sink = installTestPayloadSink();
    const session = fakeSession(async () => streamOf(assistantMessage({
      content: [
        { type: "thinking", thinking: "NORMAL_PI_REASONING let me check weather" },
        { type: "toolCall", id: "tc_1", name: "weather", arguments: { city: "北京" } },
        { type: "redacted_thinking", data: "ENCRYPTED_TOPSECRET_REDACTED_BLOB" },
        { type: "text", text: NORMAL_ASSISTANT },
      ],
    })));
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(MODEL, {
      systemPrompt: NORMAL_SYSTEM,
      messages: [{ role: "user", content: NORMAL_USER }],
    }, {});
    await stream.result();
    await flushTerminal();

    const [callId] = sink.callIds();
    const response = sink.semanticResponseForCall(callId)!;
    const payload = response.payload as any;
    expect(payload.text).toBe(NORMAL_ASSISTANT);
    expect(payload.reasoning).toContain("NORMAL_PI_REASONING");
    expect(payload.toolCalls).toHaveLength(1);
    expect(payload.toolCalls[0]).toMatchObject({ name: "weather", id: "tc_1" });
    expect(payload.toolCalls[0].arguments.city).toBe("北京");
    // redacted_thinking：结构标记、加密数据不保存
    expect(payload.reasoning).toContain("[redacted_thinking]");
    sink.assertNoSensitiveContent(["ENCRYPTED_TOPSECRET_REDACTED_BLOB"]);
  });

  it("sink 关闭：Pi 路径不产生 record（快路径）", async () => {
    const freshSink = installTestPayloadSink();
    setModelCallPayloadSink(null);
    const session = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);
    const stream = await session.agent.streamFunction(MODEL, {
      systemPrompt: NORMAL_SYSTEM,
      messages: [{ role: "user", content: NORMAL_USER }],
    }, {});
    await stream.result();
    await flushTerminal();
    expect(freshSink.records).toHaveLength(0);
  });
});
