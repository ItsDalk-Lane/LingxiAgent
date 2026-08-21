/**
 * MC-05 Provider Connectivity Probe × ModelCallObserver（§十八～§二十一）。
 *
 * - Anthropic 分支：POST /v1/messages 是真实最小生成调用 → 1 logical call +
 *   1 exact attempt + ledger entry（metadata.modelCallId 关联）。
 * - 其它协议分支：GET /models 是模型目录发现（CONTROL_PLANE）→ 0 observer
 *   事件、0 usage ledger entry。
 * - 固定 prompt（"."）不进事件；Provider error body 毒丸不进事件。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { probeProvider } from "../lib/llm/provider-client.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";
import { createUsageLedger } from "../lib/llm/usage-ledger.ts";

const POISON = "TOP_SECRET_PROVIDER_RESPONSE_8F91C2";

function probeCredentialBoundary(apiKey = "sk-probe-key") {
  return {
    consume: () => ({ apiKey, headers: {} }),
  };
}

const USAGE_CONTEXT = {
  source: { subsystem: "provider-management", operation: "connectivity-probe", surface: "settings", trigger: "user" },
  attribution: { kind: "provider", providerId: "anthropic-test" },
};

describe("MC-05 provider probe × ModelCallObserver", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;

  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
  });
  afterEach(() => {
    setModelCallObserver(null);
    vi.unstubAllGlobals();
  });

  it("Anthropic generation probe：1 logical call + 1 exact attempt + ledger 关联", async () => {
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200, headers: { "x-request-id": "probe-req-1" } }));
    vi.stubGlobal("fetch", fetchMock);
    const ledger = createUsageLedger({});

    const result = await probeProvider({
      providerId: "anthropic-test",
      baseUrl: "https://probe.test",
      api: "anthropic-messages",
      credentialBoundary: probeCredentialBoundary(),
      modelId: "claude-sonnet-4-6",
      usageLedger: ledger,
      usageContext: USAGE_CONTEXT,
    } as any);

    expect(result).toEqual({ ok: true, status: 200 });
    const callId = observer.callIds()[0];
    observer.assertLifecycle(callId, [
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "provider_response_received",
      "semantic_response_completed",
      "logical_call_end",
    ]);
    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.details).toMatchObject({ path: "provider_probe", probeKind: "generation", protocol: "anthropic-messages" });
    expect(start.model).toEqual({ provider: "anthropic-test", modelId: "claude-sonnet-4-6", api: "anthropic-messages" });
    expect(start.source).toMatchObject({ subsystem: "provider-management", operation: "connectivity-probe" });

    const attempt = observer.eventsOfType("attempt_start")[0];
    expect(attempt.details).toMatchObject({ attemptVisibility: "exact", providerWireVisibility: "request_response" });
    const received = observer.eventsOfType("provider_response_received")[0];
    expect(received).toMatchObject({ providerRequestId: "probe-req-1", details: { httpStatus: 200 } });
    expect(observer.events.at(-1)).toMatchObject({ status: "ok" });

    // 请求体固定 "."：事件序列化不含 prompt 正文（连固定值也不进）。
    expect(JSON.stringify(observer.events)).not.toContain('"."');
    // ledger：1 entry + modelCallId 关联
    const entries = ledger.list({}).entries;
    expect(entries).toHaveLength(1);
    expect(entries[0].metadata).toMatchObject({ modelCallId: callId });
  });

  it("Anthropic probe 失败（HTTP 500 + 毒丸 error body）：错误终态 + 毒丸不泄漏", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(
      JSON.stringify({ error: { message: POISON, detail: POISON } }),
      { status: 500, headers: { "Content-Type": "application/json" } },
    )));
    const ledger = createUsageLedger({});

    const result = await probeProvider({
      providerId: "anthropic-test",
      baseUrl: "https://probe.test",
      api: "anthropic-messages",
      credentialBoundary: probeCredentialBoundary(),
      usageLedger: ledger,
      usageContext: USAGE_CONTEXT,
    } as any);

    // 业务行为不变：非 2xx → ok:false + 截断的 provider message（给设置页）
    expect(result.ok).toBe(false);
    expect(result.status).toBe(500);
    expect(result.error).toContain(POISON);

    // Observer：attempt_error + logical_call_error + end(error)，正文不进事件
    const callId = observer.callIds()[0];
    observer.assertLifecycle(callId, [
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "provider_response_received",
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
    expect(JSON.stringify(observer.events)).not.toContain(POISON);
    expect(observer.eventsOfType("attempt_error")[0].error?.message).toBeNull();
    expect(observer.events.at(-1)).toMatchObject({ status: "error" });
    // withModelRequestAccounting 对 ok:false recordError → ledger 一条 error entry
    expect(ledger.list({}).entries[0].status).toBe("error");
  });

  it("GET /models 探测（非生成协议）：0 ModelCall 事件 + 0 ledger entry", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ data: [] }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const ledger = createUsageLedger({});

    const result = await probeProvider({
      providerId: "custom-openai",
      baseUrl: "https://probe.test/v1",
      api: "openai-completions",
      credentialBoundary: probeCredentialBoundary(),
      usageLedger: ledger,
      usageContext: USAGE_CONTEXT,
    } as any);

    expect(result).toEqual({ ok: true, status: 200 });
    expect(fetchMock).toHaveBeenCalledWith("https://probe.test/v1/models", expect.anything());
    // 控制面红线（§二十一）：模型目录发现不是 Model Call。
    expect(observer.events).toHaveLength(0);
    expect(ledger.list({}).entries).toHaveLength(0);
  });

  it("GET /models 探测失败也不产生 Model Call 事实", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("nope", { status: 503 })));
    const ledger = createUsageLedger({});

    const result = await probeProvider({
      providerId: "custom-openai",
      baseUrl: "https://probe.test/v1",
      api: "openai-completions",
      credentialBoundary: probeCredentialBoundary(),
      usageLedger: ledger,
      usageContext: USAGE_CONTEXT,
    } as any);

    expect(result.ok).toBe(false);
    expect(observer.events).toHaveLength(0);
    expect(ledger.list({}).entries).toHaveLength(0);
  });

  it("Anthropic probe 网络失败：attempt_error(network) + logical error + rethrow", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new TypeError("fetch failed"); }));
    const ledger = createUsageLedger({});

    await expect(probeProvider({
      providerId: "anthropic-test",
      baseUrl: "https://probe.test",
      api: "anthropic-messages",
      credentialBoundary: probeCredentialBoundary(),
      usageLedger: ledger,
      usageContext: USAGE_CONTEXT,
    } as any)).rejects.toThrow("fetch failed");

    const callId = observer.callIds()[0];
    observer.assertLifecycle(callId, [
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
    expect(observer.eventsOfType("attempt_error")[0].details).toMatchObject({ errorKind: "network" });
    expect(ledger.list({}).entries[0].status).toBe("error");
  });

  it("probe apiKey 不进事件序列化", async () => {
    const apiKey = "sk-VERY-SECRET-PROBE-KEY-8F91";
    vi.stubGlobal("fetch", vi.fn(async () => new Response("{}", { status: 200 })));

    await probeProvider({
      providerId: "anthropic-test",
      baseUrl: "https://probe.test",
      api: "anthropic-messages",
      credentialBoundary: probeCredentialBoundary(apiKey),
      usageContext: USAGE_CONTEXT,
    } as any);

    expect(JSON.stringify(observer.events)).not.toContain(apiKey);
  });
});
