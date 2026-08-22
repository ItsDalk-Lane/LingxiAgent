/**
 * Phase 2.5 安全收口 — Safety A–E（任务书 §五十七）。
 *
 * Safety A  Provider raw error body 毒丸不进 Observer 序列化结果
 * Safety B  details 毒丸键被 metadata safety gate 拒绝/剥离（fail closed）
 * Safety C  providerRequestId 超长/异常值不进入 Observer
 * Safety D  logical_call_end 后 recorder 一切方法为 silent no-op
 * Safety E  observer handler throw 业务仍正常（旁路不变量）
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { callText } from "../core/llm-client.ts";
import {
  extractProviderRequestId,
  sanitizeModelCallDetails,
  sanitizeProviderRequestId,
  setModelCallObserver,
} from "../lib/llm/model-call-observer.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";

const POISON = "TOP_SECRET_PROVIDER_RESPONSE_8F91C2";
const CONTEXT = {
  model: { provider: "openai", modelId: "gpt-5-mini", api: "openai-completions" },
  source: { subsystem: "utility", operation: "title", surface: "system", trigger: "tool" },
  attribution: { kind: "session" },
} as const;

function baseOptions(extra: Record<string, unknown> = {}) {
  return {
    api: "openai-completions",
    baseUrl: "https://example.test/v1",
    model: { id: "gpt-5-mini", provider: "openai" },
    systemPrompt: "TOPSECRET_SYSTEM_PROMPT",
    messages: [{ role: "user", content: "TOPSECRET_USER_PROMPT" }],
    ...extra,
  } as any;
}

describe("Phase 2.5 metadata safety gate", () => {
  it("Safety A: Provider error body 毒丸（message/detail/raw body）不进 Observer", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    // 三种 provider 错误正文形态：结构化 error.message、error.detail、非 JSON raw text。
    const bodies = [
      JSON.stringify({ error: { message: POISON, detail: POISON }, request_id: "req-poison-1" }),
      POISON,
      `<html>${POISON}</html>`,
    ];
    for (const body of bodies) {
      observer.reset();
      vi.stubGlobal("fetch", vi.fn(async () => new Response(body, {
        status: 500,
        headers: { "Content-Type": "application/json" },
      })));
      // 业务错误信息保持既有行为：raw body fallback 会出现在 AppError.message。
      await expect(callText(baseOptions())).rejects.toThrow();
      expect(JSON.stringify(observer.events)).not.toContain(POISON);
      expect(observer.eventsOfType("logical_call_end")[0]).toMatchObject({ status: "error" });
      // 错误事件只留结构事实：name + null message（唯一例外：markModelCallSafeMessage
      // 显式标记的内部固定文案，如 invalid-JSON——不含任何 Provider 内容）。
      for (const event of observer.eventsOfType("attempt_error").concat(observer.eventsOfType("logical_call_error"))) {
        const message = event.error?.message ?? null;
        const isInternalSafeMessage = message !== null
          && (message.startsWith("LLM returned invalid JSON") || message.startsWith("模型未回复正文"));
        expect(isInternalSafeMessage || message === null).toBe(true);
        expect(message ?? "").not.toContain(POISON);
        expect(typeof event.error?.name).toBe("string");
      }
    }
    vi.unstubAllGlobals();
  });

  it("Safety B: details 毒丸键（prompt/messages/authorization/body/responseText…）被剥离", () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const recorder = createModelCallRecorder({ context: CONTEXT });
    recorder.beginLogicalCall({
      details: {
        path: "callText",
        prompt: "TOPSECRET_PROMPT",
        systemPrompt: "TOPSECRET_SYSTEM",
        messages: [{ role: "user", content: "x" }],
        messageCount: 2,
        authorization: "Bearer TOPSECRET_TOKEN",
        rawBody: POISON,
        responseBody: POISON,
        responseText: POISON,
        apiKey: "sk-topsecret",
        toolResult: "secret tool output",
        headers: { "x-api-key": "secret" },
        base64: "TOPSECRET_BASE64",
        stdout: "TOPSECRET_STDOUT",
        commandArgs: ["--prompt", "TOPSECRET_PROMPT"],
        hasText: true,
        protocol: "openai-completions",
      } as Record<string, unknown>,
    });
    const serialized = JSON.stringify(observer.events);
    expect(serialized).not.toContain("TOPSECRET");
    expect(serialized).not.toContain("Bearer ");
    expect(serialized).not.toContain("sk-topsecret");
    const details = observer.eventsOfType("logical_call_start")[0].details;
    // 安全键保留：整键匹配，hasText/messageCount 不受 text/messages 影响。
    expect(details).toMatchObject({ path: "callText", messageCount: 2, hasText: true, protocol: "openai-completions" });
    // 毒丸键被 fail-closed 丢弃。
    for (const banned of ["prompt", "systemPrompt", "messages", "authorization", "rawBody", "responseBody", "responseText", "apiKey", "toolResult", "headers", "base64", "stdout", "commandArgs"]) {
      expect(details).not.toHaveProperty(banned);
    }
  });

  it("Safety B2: 键归一化后大小写/分隔符变体同样被拒（raw_body/RESPONSE-TEXT/Command_Args）", () => {
    const sanitized = sanitizeModelCallDetails({
      raw_body: POISON,
      "RESPONSE-TEXT": POISON,
      Command_Args: ["--secret"],
      "API-KEY": "secret",
      // 安全字段的不同写法不受影响
      hasSystemPrompt: true,
      providerRequestOrdinal: 2,
    });
    expect(JSON.stringify(sanitized)).not.toContain(POISON);
    expect(JSON.stringify(sanitized)).not.toContain("secret");
    expect(sanitized).toEqual({ hasSystemPrompt: true, providerRequestOrdinal: 2 });
  });

  it("Safety B3: 值形状 gate——非原始类型剥离、超长字符串截断、嵌套深度受限", () => {
    const sanitized = sanitizeModelCallDetails({
      fn: () => {},
      big: "x".repeat(5_000),
      nested: { ok: 1, deep: { deeper: { deepest: "x" } } },
      arr: [1, 2, 3],
      usage: { input: 10, output: 3 },
    });
    expect(sanitized).not.toHaveProperty("fn");
    expect((sananizedLength(sanitized?.big as string))).toBeLessThanOrEqual(300);
    expect(sanitized?.nested).toEqual({ ok: 1 }); // depth-2 之外的 deep 被丢弃
    expect(sanitized?.arr).toEqual([1, 2, 3]);
    expect(sanitized?.usage).toEqual({ input: 10, output: 3 });
    function sananizedLength(value: string | undefined): number {
      return typeof value === "string" ? value.length : Number.MAX_SAFE_INTEGER;
    }
  });

  it("Safety C: providerRequestId 异常值不进入 Observer（string only / trim / 长度上限）", () => {
    expect(sanitizeProviderRequestId(null)).toBeNull();
    expect(sanitizeProviderRequestId(12345)).toBeNull();
    expect(sanitizeProviderRequestId("  req-1  ")).toBe("req-1");
    expect(sanitizeProviderRequestId("")).toBeNull();
    // 恶意 Provider 塞超长内容 → 整体丢弃，不截断保留
    const huge = "x".repeat(5_000);
    expect(sanitizeProviderRequestId(huge)).toBeNull();
    expect(extractProviderRequestId({ "x-request-id": huge })).toBeNull();

    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const recorder = createModelCallRecorder({ context: CONTEXT });
    recorder.beginLogicalCall();
    recorder.beginAttempt();
    recorder.providerResponseReceived({ httpStatus: 200, providerRequestId: huge });
    recorder.endLogicalCall("ok");
    expect(JSON.stringify(observer.events)).not.toContain("xxxx");
    expect(observer.eventsOfType("provider_response_received")[0].providerRequestId).toBeNull();
  });

  it("Safety D: logical_call_end 后晚到事件全部 silent no-op、不影响业务", () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const recorder = createModelCallRecorder({ context: CONTEXT });
    recorder.beginLogicalCall();
    recorder.beginAttempt();
    recorder.endLogicalCall("ok");

    expect(() => {
      recorder.beginLogicalCall();
      recorder.beginAttempt();
      recorder.providerRequestPrepared({ details: { messageCount: 1 } });
      recorder.providerResponseReceived({ httpStatus: 200 });
      recorder.semanticResponseCompleted({ details: { stopReason: "stop" } });
      recorder.attemptError(new Error(POISON));
      recorder.logicalCallError(new Error(POISON));
      recorder.logicalCallAborted();
      recorder.endLogicalCall("error");
    }).not.toThrow();

    // 只剩 end 之前的 3 个事件；logical_call_end 恰好一次
    expect(observer.sequence()).toEqual(["logical_call_start", "attempt_start", "logical_call_end"]);
    expect(observer.eventsOfType("logical_call_end")).toHaveLength(1);
    expect(observer.events.at(-1)).toMatchObject({ status: "ok" });
    expect(JSON.stringify(observer.events)).not.toContain(POISON);
  });

  it("Safety E: observer handler throw 时业务仍正常（成功与失败路径）", async () => {
    setModelCallObserver({
      handleModelCallEvent() { throw new Error("observer exploded"); },
    });
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      choices: [{ message: { content: "ok" }, finish_reason: "stop" }],
    }), { status: 200 })));
    await expect(callText(baseOptions())).resolves.toBe("ok");

    vi.stubGlobal("fetch", vi.fn(async () => new Response(POISON, { status: 500 })));
    await expect(callText(baseOptions())).rejects.toThrow();
    vi.unstubAllGlobals();
  });
});

describe("Phase 2.5 安全契约（recorder 出口统一执行）", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;
  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
  });
  afterEach(() => {
    setModelCallObserver(null);
    vi.unstubAllGlobals();
  });

  it("同一 recorder 支持多 attempt（Codex 401 refresh 形态）且 attemptErrored 状态跟踪", () => {
    const recorder = createModelCallRecorder({ context: CONTEXT });
    recorder.beginLogicalCall();
    const attemptA = recorder.beginAttempt({ details: { attemptVisibility: "exact" } });
    expect(recorder.attemptErrored).toBe(false);
    recorder.attemptError(new Error("HTTP 401"));
    expect(recorder.attemptErrored).toBe(true);
    const attemptB = recorder.beginAttempt({ details: { attemptVisibility: "exact" } });
    expect(recorder.attemptErrored).toBe(false); // 新 attempt 重置
    recorder.providerResponseReceived({ httpStatus: 200 });
    recorder.semanticResponseCompleted({ details: { fileCount: 1 } });
    recorder.endLogicalCall("ok");

    expect(attemptA).not.toBe(attemptB);
    const attemptIds = observer.attemptIds();
    expect(attemptIds).toEqual([attemptA, attemptB]);
    const callIds = observer.callIds();
    expect(callIds).toHaveLength(1);
    // 事件序：attempt_error 属于 A，response 属于 B
    const errorEvent = observer.eventsOfType("attempt_error")[0];
    expect(errorEvent.attemptId).toBe(attemptA);
    expect(observer.eventsOfType("provider_response_received")[0].attemptId).toBe(attemptB);
    // 401 attempt_error 不携带 provider 正文
    expect(errorEvent.error).toEqual({ name: "Error", message: null, code: null });
  });
});
