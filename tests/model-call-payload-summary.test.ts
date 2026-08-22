/**
 * Phase 6 MC-05 probe / MC-10 direct summary × Sensitive Payload Capture +
 * 控制面 0 record 回归（§八十五/§八十六/§一百零二/§一百零三/§一百零四/
 * §一百五十/§一百五十七）。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { probeProvider } from "../lib/llm/provider-client.ts";
import { generateDiaryCompactionSummary } from "../lib/diary/diary-writer.ts";
import { runWithNewModelTrace } from "../lib/llm/model-trace-scope.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { setModelCallPayloadSink } from "../lib/llm/model-call-payload-capture.ts";
import { createTestModelCallPayloadSink, installTestPayloadSink } from "../lib/llm/model-call-payload-testing.ts";
import { createUsageLedger } from "../lib/llm/usage-ledger.ts";

const POISON_PROBE_KEY = "sk-PROBE-SECRET-KEY-8F91C2";
const NORMAL_SUMMARY = "NORMAL_SUMMARY_TEXT_VISIBLE 今日素材摘要";
const NORMAL_DIARY_INPUT = "NORMAL_DIARY_INPUT 玩家打了第一关";

function probeCredentialBoundary(apiKey = POISON_PROBE_KEY) {
  return { consume: () => ({ apiKey, headers: {} }) };
}

const USAGE_CONTEXT = {
  source: { subsystem: "provider-management", operation: "connectivity-probe", surface: "settings", trigger: "user" },
  attribution: { kind: "provider", providerId: "anthropic-test" },
};

const MODEL = {
  id: "test-model",
  provider: "test-provider",
  api: "openai-completions",
  baseUrl: "https://example.test/v1",
  maxTokens: 8192,
  input: ["text"],
  cost: { input: 1, output: 2, cacheRead: 0.1, cacheWrite: 1.25, total: 0 },
};

function completionsOkFetch(content = NORMAL_SUMMARY) {
  const sseBody = [
    `data: ${JSON.stringify({ id: "chatcmpl-1", object: "chat.completion.chunk", created: 0, model: "test-model", choices: [{ index: 0, delta: { role: "assistant", content }, finish_reason: null }] })}`,
    "",
    `data: ${JSON.stringify({ id: "chatcmpl-1", object: "chat.completion.chunk", created: 0, model: "test-model", choices: [{ index: 0, delta: {}, finish_reason: "stop" }], usage: { prompt_tokens: 10, completion_tokens: 4, total_tokens: 14 } })}`,
    "",
    "data: [DONE]",
    "",
  ].join("\n");
  return vi.fn(async () => new Response(sseBody, { status: 200, headers: { "content-type": "text/event-stream" } }));
}

describe("MC-05 provider probe × payload capture", () => {
  let sink: ReturnType<typeof createTestModelCallPayloadSink>;
  beforeEach(() => {
    sink = installTestPayloadSink();
    setModelCallObserver({ handleModelCallEvent() { /* 已有独立覆盖 */ } });
  });
  afterEach(() => {
    setModelCallObserver(null);
    setModelCallPayloadSink(null);
    vi.unstubAllGlobals();
  });

  it("anthropic generation probe：四层 capture；固定 prompt '.' 允许捕获（§八十六）", async () => {
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200, headers: { "x-request-id": "probe-req-1" } }));
    vi.stubGlobal("fetch", fetchMock);
    const ledger = createUsageLedger({});

    const result = await probeProvider({
      providerId: "anthropic-test",
      baseUrl: "https://probe.test",
      api: "anthropic-messages",
      credentialBoundary: probeCredentialBoundary(),
      usageLedger: ledger,
      usageContext: USAGE_CONTEXT,
    } as any);
    expect(result.ok).toBe(true);

    const [callId] = sink.callIds();
    expect(sink.sequenceForCall(callId)).toEqual([
      "semantic_request",
      "provider_request",
      "provider_response",
      "semantic_response",
    ]);
    // semantic request：固定占位消息正文可捕获
    const semantic = sink.semanticRequestForCall(callId)!;
    expect((semantic.payload as any).messages[0].content).toBe(".");
    // provider request：最小生成 body + x-api-key 替换
    const transport = (sink.providerRequestsForCall(callId)[0].payload as any).transport;
    expect(transport.body.max_tokens).toBe(1);
    expect(transport.body.messages[0].content).toBe(".");
    expect(transport.headers["x-api-key"]).toBe("<redacted:credential>");
    // provider response：成功不读 body → 诚实 metadata_only
    const response = sink.providerResponsesForCall(callId)[0];
    expect(response.visibility).toBe("metadata_only");
    expect(response.fidelity).toBe("metadata_only");
    // semantic response：probeAccepted
    const semanticResponse = sink.semanticResponseForCall(callId)!;
    expect((semanticResponse.payload as any).structuredOutput).toMatchObject({ probeAccepted: true });
    sink.assertNoSensitiveContent([POISON_PROBE_KEY]);
  });

  it("probe HTTP 401：error body 安全捕获（§一百五十二）", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(
      JSON.stringify({ error: { message: "invalid api key provided", type: "authentication_error" } }),
      { status: 401, headers: { "content-type": "application/json" } },
    )));
    const result = await probeProvider({
      providerId: "anthropic-test",
      baseUrl: "https://probe.test",
      api: "anthropic-messages",
      credentialBoundary: probeCredentialBoundary(),
      usageLedger: createUsageLedger({}),
      usageContext: USAGE_CONTEXT,
    } as any);
    expect(result.ok).toBe(false);
    const [callId] = sink.callIds();
    const response = sink.providerResponsesForCall(callId)[0];
    expect((response.payload as any).status).toBe(401);
    expect((response.payload as any).body.error.type).toBe("authentication_error");
    expect(sink.semanticResponseForCall(callId)).toBeNull();
  });

  it("GET /models（非 anthropic）= CONTROL_PLANE：0 payload record（§一百五十七）", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ data: [] }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    await probeProvider({
      providerId: "openai-test",
      baseUrl: "https://probe.test/v1",
      api: "openai-completions",
      credentialBoundary: probeCredentialBoundary(),
      usageLedger: createUsageLedger({}),
      usageContext: USAGE_CONTEXT,
    } as any);
    expect(sink.records).toHaveLength(0);
    expect(fetchMock).toHaveBeenCalled();
  });
});

describe("MC-10 diary direct summary × payload capture", () => {
  let sink: ReturnType<typeof createTestModelCallPayloadSink>;
  beforeEach(() => {
    sink = installTestPayloadSink();
    setModelCallObserver({ handleModelCallEvent() { /* 已有独立覆盖 */ } });
  });
  afterEach(() => {
    setModelCallObserver(null);
    setModelCallPayloadSink(null);
    vi.unstubAllGlobals();
  });

  async function flushTerminal() {
    await new Promise((resolve) => setImmediate(resolve));
    await new Promise((resolve) => setImmediate(resolve));
  }

  it("三元组全参捕获 + provider wire 显式 unavailable + summary 正文捕获（§一百零二～§一百零四）", async () => {
    vi.stubGlobal("fetch", completionsOkFetch(NORMAL_SUMMARY));
    const summary = await runWithNewModelTrace({ origin: "diary" }, () =>
      generateDiaryCompactionSummary({
        messages: [{ role: "user", content: [{ type: "text", text: NORMAL_DIARY_INPUT }], timestamp: Date.now() }] as any,
        model: MODEL as any,
        apiKey: "test-key",
        headers: undefined,
        previousSummary: "",
        usageLedger: createUsageLedger({}),
        agentId: "agent-1",
      }),
    );
    await flushTerminal();
    expect(summary).toContain(NORMAL_SUMMARY);

    const [callId] = sink.callIds();
    expect(sink.sequenceForCall(callId)).toEqual([
      "semantic_request",
      "provider_request",
      "provider_response",
      "semantic_response",
    ]);
    // semantic request：messages 全参 + 正文可见
    const semantic = sink.semanticRequestForCall(callId)!;
    const payload = semantic.payload as any;
    expect(payload.inputShape).toBe("pi_direct_summary");
    expect(JSON.stringify(payload.messages)).toContain(NORMAL_DIARY_INPUT);
    // provider wire：显式 unavailable（pi 0.84.1 summarizer options 无 hook）
    const [req, res] = [
      sink.recordsOfKind(callId, "provider_request")[0],
      sink.recordsOfKind(callId, "provider_response")[0],
    ];
    expect(req.visibility).toBe("unavailable");
    expect(req.payload).toBeNull();
    expect(res.visibility).toBe("unavailable");
    // semantic response：实际 summary 字符串
    expect((sink.semanticResponseForCall(callId)!.payload as any).text).toContain(NORMAL_SUMMARY);
  });

  it("provider 网络失败：无 semantic_response（§一百五十五）", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new TypeError("fetch failed"); }));
    await expect(runWithNewModelTrace({ origin: "diary" }, () =>
      generateDiaryCompactionSummary({
        messages: [{ role: "user", content: [{ type: "text", text: NORMAL_DIARY_INPUT }], timestamp: Date.now() }] as any,
        model: MODEL as any,
        apiKey: "test-key",
        headers: undefined,
        previousSummary: "",
        usageLedger: createUsageLedger({}),
        agentId: "agent-1",
      }),
    )).rejects.toThrow();
    const [callId] = sink.callIds();
    expect(sink.semanticResponseForCall(callId)).toBeNull();
  });
});
