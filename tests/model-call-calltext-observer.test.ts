/**
 * MC-04 callText × ModelCallObserver — Fake Provider 运行时证明。
 *
 * 覆盖任务书 Scenario A–D：success / provider error / network error / abort，
 * 另加 timeout、HTTP 429、usage missing、observer 故障旁路、ledger 关联与
 * 无双计验证。全部经 vi.stubGlobal("fetch") 的可控假 Provider，不打真实网络。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { callText } from "../core/llm-client.ts";
import { createUsageLedger } from "../lib/llm/usage-ledger.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";

const MODEL = { id: "gpt-5-mini", provider: "openai", cost: { input: 1, output: 10, cacheRead: 0.1, cacheWrite: 1.25 } };
const BASE_URL = "https://example.test/v1";
const USAGE_CONTEXT = {
  source: { subsystem: "utility", operation: "title", surface: "system", trigger: "tool" },
  attribution: { kind: "session", agentId: "agent-1", sessionPath: "/sessions/a.jsonl" },
};

function okFetch(body: Record<string, unknown> = {}, headers: Record<string, string> = {}) {
  return vi.fn(async () => new Response(JSON.stringify({
    choices: [{ message: { content: "Title" }, finish_reason: "stop" }],
    usage: { prompt_tokens: 10, completion_tokens: 3, total_tokens: 13 },
    ...body,
  }), { status: 200, headers }));
}

function errorFetch(status: number, payload: unknown, headers: Record<string, string> = {}) {
  return vi.fn(async () => new Response(
    typeof payload === "string" ? payload : JSON.stringify(payload),
    { status, headers },
  ));
}

function baseOptions(extra: Record<string, unknown> = {}) {
  return {
    api: "openai-completions",
    baseUrl: BASE_URL,
    model: MODEL,
    systemPrompt: "TOPSECRET_SYSTEM_PROMPT",
    messages: [{ role: "user", content: "TOPSECRET_USER_PROMPT" }],
    usageContext: USAGE_CONTEXT,
    ...extra,
  } as any;
}

describe("MC-04 callText × ModelCallObserver", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;

  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
  });
  afterEach(() => {
    setModelCallObserver(null);
    vi.unstubAllGlobals();
  });

  it("Scenario A: 成功调用的完整生命周期 + 身份/归属/metadata", async () => {
    vi.stubGlobal("fetch", okFetch({}, { "x-request-id": "req-provider-1" }));
    const ledger = createUsageLedger({});

    const text = await callText(baseOptions({ usageLedger: ledger }));

    expect(text).toBe("Title");
    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "provider_response_received",
      "semantic_response_completed",
      "logical_call_end",
    ]);
    const [callId] = observer.callIds();
    const [attemptId] = observer.attemptIds();
    expect(callId).toMatch(/^mc_/);
    expect(attemptId).toMatch(/^ma_/);
    expect(attemptId).not.toBe(callId);

    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start).toMatchObject({
      callId,
      model: { provider: "openai", modelId: "gpt-5-mini", api: "openai-completions" },
      source: { subsystem: "utility", operation: "title", surface: "system", trigger: "tool" },
      attribution: { kind: "session", agentId: "agent-1", sessionPath: "/sessions/a.jsonl" },
    });
    expect(start.details).toMatchObject({ path: "callText" });

    // provider_request_prepared：只有结构 metadata，绝无正文
    const prepared = observer.eventsOfType("provider_request_prepared")[0];
    expect(prepared.attemptId).toBe(attemptId);
    expect(prepared.details).toMatchObject({
      protocol: "openai-completions",
      streaming: false,
      messageCount: 2, // system + user
      hasSystemPrompt: true,
      hasImages: false,
    });
    expect(typeof prepared.details?.inputByteEstimate).toBe("number");
    expect(JSON.stringify(prepared)).not.toContain("TOPSECRET_USER_PROMPT");
    expect(JSON.stringify(prepared)).not.toContain("TOPSECRET_SYSTEM_PROMPT");

    const received = observer.eventsOfType("provider_response_received")[0];
    expect(received).toMatchObject({
      attemptId,
      providerRequestId: "req-provider-1",
      details: { httpStatus: 200 },
    });

    const semantic = observer.eventsOfType("semantic_response_completed")[0];
    expect(semantic.details).toMatchObject({ stopReason: "stop", usagePresent: true });

    expect(observer.events.at(-1)).toMatchObject({ eventType: "logical_call_end", status: "ok" });

    // Ledger 关联：同一次调用，metadata.{modelCallId,traceId,parentCallId} ===
    // observer 身份，且只记一条（无双计）。Phase 4 起 traceId 恒非空（§二十二）。
    const entries = ledger.list({}).entries;
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({ status: "ok" });
    expect(entries[0].metadata).toEqual({
      modelCallId: callId,
      traceId: observer.callIdentity(callId)!.traceId,
      parentCallId: null,
    });
    expect(entries[0].metadata!.traceId).toMatch(/^mt_/);
  });

  it("Scenario B: Provider HTTP error（400/500）有完整错误终态", async () => {
    vi.stubGlobal("fetch", errorFetch(400, { error: { message: "bad request" }, request_id: "req-err-1" }));
    const ledger = createUsageLedger({});

    await expect(callText(baseOptions({ usageLedger: ledger }))).rejects.toThrow("bad request");

    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "provider_response_received", // HTTP response 到达了（只是状态非 2xx）
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
    const received = observer.eventsOfType("provider_response_received")[0];
    expect(received.details).toMatchObject({ httpStatus: 400 });
    // headers 没有 id 时，回落到 provider error body 的 request_id
    const attemptError = observer.eventsOfType("attempt_error")[0];
    expect(attemptError.providerRequestId).toBe("req-err-1");
    expect(observer.events.at(-1)).toMatchObject({ status: "error" });

    // 全文 dump 不泄露 prompt 正文
    expect(JSON.stringify(observer.events)).not.toContain("TOPSECRET_USER_PROMPT");

    const entries = ledger.list({}).entries;
    expect(entries).toHaveLength(1);
    expect(entries[0].status).toBe("error");
  });

  it("Scenario B2: HTTP 429 保持 LLM_RATE_LIMITED 业务行为", async () => {
    vi.stubGlobal("fetch", errorFetch(429, { error: { message: "slow down" } }));
    await expect(callText(baseOptions())).rejects.toMatchObject({ code: "LLM_RATE_LIMITED" });
    expect(observer.sequence()).toContain("attempt_error");
    expect(observer.sequence()).toContain("logical_call_error");
    expect(observer.events.at(-1)).toMatchObject({ eventType: "logical_call_end", status: "error" });
  });

  it("Scenario C: network error（fetch 直接抛）没有 provider_response_received", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => { throw new TypeError("fetch failed: socket hang up"); }));

    await expect(callText(baseOptions())).rejects.toThrow("socket hang up");

    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
    expect(observer.eventsOfType("provider_response_received")).toHaveLength(0);
    expect(observer.eventsOfType("attempt_error")[0].details).toMatchObject({
      errorKind: "provider_or_network",
    });
  });

  it("Scenario C2: timeout 与 user abort 明确区分（LLM_TIMEOUT → errorKind=timeout）", async () => {
    vi.stubGlobal("fetch", vi.fn((_url: any, init: any) => new Promise((_resolve, reject) => {
      init?.signal?.addEventListener("abort", () => {
        const err = new Error("The operation timed out");
        err.name = "TimeoutError";
        reject(err);
      });
    })));

    await expect(callText(baseOptions({ timeoutMs: 30 }))).rejects.toMatchObject({ code: "LLM_TIMEOUT" });

    expect(observer.eventsOfType("logical_call_aborted")).toHaveLength(0);
    expect(observer.eventsOfType("logical_call_error")[0].details).toMatchObject({ errorKind: "timeout" });
    expect(observer.events.at(-1)).toMatchObject({ status: "error" });
  });

  it("Scenario D: 用户 abort → logical_call_aborted，业务错误行为不变", async () => {
    const controller = new AbortController();
    vi.stubGlobal("fetch", vi.fn((_url: any, init: any) => new Promise((_resolve, reject) => {
      init?.signal?.addEventListener("abort", () => {
        const err = new Error("This operation was aborted");
        err.name = "AbortError";
        reject(err);
      });
    })));
    const ledger = createUsageLedger({});

    const pending = callText(baseOptions({ signal: controller.signal, usageLedger: ledger }));
    setTimeout(() => controller.abort(), 5);
    // 业务侧仍然抛 AbortError（与接入前一致）
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });

    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "logical_call_aborted",
      "logical_call_end",
    ]);
    expect(observer.events.at(-1)).toMatchObject({ status: "aborted" });

    const entries = ledger.list({}).entries;
    expect(entries).toHaveLength(1);
    expect(entries[0].status).toBe("aborted");
  });

  it("usage missing：Provider 不返回 usage 时 lifecycle 仍完整", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      choices: [{ message: { content: "ok" }, finish_reason: "stop" }],
    }), { status: 200 })));
    const ledger = createUsageLedger({});

    await callText(baseOptions({ usageLedger: ledger }));

    expect(observer.sequence()).toContain("semantic_response_completed");
    expect(observer.eventsOfType("semantic_response_completed")[0].details)
      .toMatchObject({ usagePresent: false });
    expect(observer.events.at(-1)).toMatchObject({ status: "ok" });
    expect(ledger.list({}).entries[0].status).toBe("usage_missing");
  });

  it("observer handler 抛异常不改变调用结果（成功与失败两条路径）", async () => {
    setModelCallObserver({
      handleModelCallEvent() { throw new Error("observer exploded"); },
    });
    vi.stubGlobal("fetch", okFetch());
    await expect(callText(baseOptions())).resolves.toBe("Title");

    vi.stubGlobal("fetch", errorFetch(500, { error: { message: "boom" } }));
    await expect(callText(baseOptions())).rejects.toThrow("boom");
  });

  it("observer 缺失（noop 默认）时调用与 ledger 行为不变", async () => {
    setModelCallObserver(null);
    vi.stubGlobal("fetch", okFetch());
    const ledger = createUsageLedger({});
    await expect(callText(baseOptions({ usageLedger: ledger }))).resolves.toBe("Title");
    expect(ledger.list({}).entries).toHaveLength(1);
  });

  it("business retry（两次 callText）= 两个独立 logical call", async () => {
    vi.stubGlobal("fetch", okFetch());
    await callText(baseOptions());
    await callText(baseOptions());
    expect(observer.callIds()).toHaveLength(2);
    expect(observer.eventsOfType("logical_call_end")).toHaveLength(2);
  });

  it("Phase 4 trace 解析：无 explicit/无 scope → singleton traceId（非空）+ parent=null 不猜；explicit 优先", async () => {
    vi.stubGlobal("fetch", okFetch());
    await callText(baseOptions());
    const defaultStart = observer.eventsOfType("logical_call_start")[0];
    // §二十二：独立 callText 形成单 call 的 singleton trace——traceId 恒非空；
    // §二十三：无 parent 事实 → null，不猜。
    expect(defaultStart.traceId).toMatch(/^mt_/);
    expect(defaultStart.parentCallId).toBeNull();

    observer.reset();
    await callText(baseOptions({
      modelCallContext: { traceId: "mt_test_trace", parentCallId: "mc_parent" },
    }));
    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.traceId).toBe("mt_test_trace");
    expect(start.parentCallId).toBe("mc_parent");
  });
});
