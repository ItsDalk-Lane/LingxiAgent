import { afterEach, describe, expect, it, vi } from "vitest";
import {
  createModelCallIdentityFactory,
  mintModelAttemptId,
  mintModelCallId,
} from "../lib/llm/model-call-identity.ts";
import {
  MODEL_CALL_EVENT_TYPES,
  NOOP_MODEL_CALL_OBSERVER,
  getModelCallObserver,
  modelCallFieldsFromUsageContext,
  normalizeModelCallError,
  safeEmitModelCallEvent,
  setModelCallObserver,
} from "../lib/llm/model-call-observer.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";

const CONTEXT = {
  model: { provider: "openai", modelId: "gpt-5-mini", api: "openai-completions" },
  source: { subsystem: "utility", operation: "title", surface: "system", trigger: "tool" },
  attribution: { kind: "session", agentId: "agent-1", sessionPath: "/sessions/a.jsonl" },
} as const;

/**
 * 模拟一条最小业务请求路径：observer 只是旁路，返回值只由业务逻辑决定。
 * observer 通过全局注册表注入（生产默认 noop）。
 */
async function simulatedRequestPath({ fail = false }: { fail?: boolean } = {}) {
  const recorder = createModelCallRecorder({ context: CONTEXT });
  recorder.beginLogicalCall();
  recorder.beginAttempt();
  recorder.providerRequestPrepared({ details: { messageCount: 1 } });
  if (fail) {
    const err = new Error("boom");
    recorder.attemptError(err);
    recorder.logicalCallError(err);
    recorder.endLogicalCall("error");
    throw err;
  }
  recorder.providerResponseReceived({ httpStatus: 200 });
  recorder.semanticResponseCompleted({ details: { stopReason: "stop", usagePresent: true } });
  recorder.endLogicalCall("ok");
  return "business-result";
}

describe("ModelCallObserver contract", () => {
  afterEach(() => {
    setModelCallObserver(null);
  });

  it("Test 1: observer 不存在（noop 默认）时请求路径正常工作", async () => {
    expect(getModelCallObserver()).toBe(NOOP_MODEL_CALL_OBSERVER);
    await expect(simulatedRequestPath()).resolves.toBe("business-result");
    await expect(simulatedRequestPath({ fail: true })).rejects.toThrow("boom");
  });

  it("Test 2: observer handler 抛异常时请求路径仍然正常工作", async () => {
    setModelCallObserver({
      handleModelCallEvent() {
        throw new Error("observer exploded");
      },
    });
    await expect(simulatedRequestPath()).resolves.toBe("business-result");
    await expect(simulatedRequestPath({ fail: true })).rejects.toThrow("boom");
  });

  it("Test 3: 普通 logical call 的完整生命周期共享同一 callId", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);

    await simulatedRequestPath();

    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "provider_response_received",
      "semantic_response_completed",
      "logical_call_end",
    ]);
    const callIds = observer.callIds();
    expect(callIds).toHaveLength(1);
    expect(callIds[0]).toMatch(/^mc_/);
    // 每个事件都带稳定身份与归属
    for (const event of observer.events) {
      expect(event.callId).toBe(callIds[0]);
      expect(event.model).toEqual(CONTEXT.model);
      expect(event.source).toEqual(CONTEXT.source);
      expect(event.attribution).toEqual(CONTEXT.attribution);
      expect(event.timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    }
    // start 不绑定 attempt；之后的事件共享同一 attemptId
    expect(observer.events[0].attemptId).toBeNull();
    const attemptIds = observer.attemptIds();
    expect(attemptIds).toHaveLength(1);
    expect(attemptIds[0]).toMatch(/^ma_/);
    expect(attemptIds[0]).not.toBe(callIds[0]);
    expect(observer.events.at(-1)).toMatchObject({ eventType: "logical_call_end", status: "ok" });
  });

  it("Test 4: 两个 attempt（retry）共用 callId、各自独立 attemptId", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const recorder = createModelCallRecorder({ context: CONTEXT });

    recorder.beginLogicalCall();
    const attemptA = recorder.beginAttempt();
    recorder.attemptError(new Error("HTTP 429"));
    const attemptB = recorder.beginAttempt();
    recorder.providerResponseReceived({ httpStatus: 200 });
    recorder.semanticResponseCompleted({ details: { stopReason: "stop" } });
    recorder.endLogicalCall("ok");

    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "attempt_error",
      "attempt_start",
      "provider_response_received",
      "semantic_response_completed",
      "logical_call_end",
    ]);
    expect(observer.callIds()).toHaveLength(1);
    expect(attemptA).not.toBe(attemptB);
    expect(observer.attemptIds()).toEqual([attemptA, attemptB]);
    // attempt A 的 attempt_error 归属 A，B 的后续事件归属 B
    expect(observer.events[2]).toMatchObject({ eventType: "attempt_error", attemptId: attemptA });
    expect(observer.events[4]).toMatchObject({ eventType: "provider_response_received", attemptId: attemptB });
  });

  it("Test 5: abort 生命周期", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const recorder = createModelCallRecorder({ context: CONTEXT });

    recorder.beginLogicalCall();
    recorder.beginAttempt();
    recorder.logicalCallAborted();
    recorder.endLogicalCall("aborted");

    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "logical_call_aborted",
      "logical_call_end",
    ]);
    expect(observer.events.at(-1)).toMatchObject({ status: "aborted" });
  });

  it("Test 6: error 生命周期", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const recorder = createModelCallRecorder({ context: CONTEXT });

    const err = new Error("network down");
    recorder.beginLogicalCall();
    recorder.beginAttempt();
    recorder.attemptError(err);
    recorder.logicalCallError(err);
    recorder.endLogicalCall("error");

    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
    expect(observer.events.at(-1)).toMatchObject({ status: "error" });
    expect(observer.events[2].error).toEqual({ name: "Error", message: "network down" });
  });

  it("endLogicalCall 恰好投递一次（幂等）", () => {
    const observer = createTestModelCallObserver();
    const recorder = createModelCallRecorder({ observer, context: CONTEXT });
    recorder.beginLogicalCall();
    recorder.endLogicalCall("ok");
    recorder.endLogicalCall("error");
    recorder.endLogicalCall("aborted");
    expect(observer.eventsOfType("logical_call_end")).toHaveLength(1);
    expect(observer.eventsOfType("logical_call_end")[0].status).toBe("ok");
    expect(recorder.ended).toBe(true);
  });

  it("safeEmitModelCallEvent 吞掉 handler 异常与非法 observer", () => {
    expect(() => safeEmitModelCallEvent(null, {} as any)).not.toThrow();
    expect(() => safeEmitModelCallEvent({} as any, {} as any)).not.toThrow();
    expect(() => safeEmitModelCallEvent({
      handleModelCallEvent() { throw new Error("x"); },
    }, { eventType: "logical_call_start", callId: "mc_x", timestamp: "t" } as any)).not.toThrow();
  });

  it("事件契约类型集合固定", () => {
    expect([...MODEL_CALL_EVENT_TYPES]).toEqual([
      "logical_call_start",
      "attempt_start",
      "provider_request_prepared",
      "provider_response_received",
      "semantic_response_completed",
      "attempt_error",
      "logical_call_error",
      "logical_call_aborted",
      "logical_call_end",
    ]);
  });
});

describe("model call identity", () => {
  it("默认工厂：进程内不碰撞，前缀语义清晰", () => {
    const ids = new Set(Array.from({ length: 500 }, () => mintModelCallId()));
    expect(ids.size).toBe(500);
    expect(mintModelCallId()).toMatch(/^mc_/);
    expect(mintModelAttemptId()).toMatch(/^ma_/);
  });

  it("注入确定性源后可复现", () => {
    const factory = createModelCallIdentityFactory({
      now: () => 1_700_000_000_000,
      random: () => "testrand",
    });
    expect(factory.mintCallId()).toBe(`mc_${(1_700_000_000_000).toString(36)}_1_testrand`);
    expect(factory.mintAttemptId()).toBe(`ma_${(1_700_000_000_000).toString(36)}_2_testrand`);
    expect(factory.mintTraceId()).toBe(`mt_${(1_700_000_000_000).toString(36)}_3_testrand`);
  });

  it("recorder 支持调用方显式接管 callId（ledger 关联场景）", () => {
    const observer = createTestModelCallObserver();
    const recorder = createModelCallRecorder({
      observer,
      context: { ...CONTEXT, callId: "mc_explicit" },
    });
    expect(recorder.callId).toBe("mc_explicit");
    recorder.beginLogicalCall();
    expect(observer.events[0].callId).toBe("mc_explicit");
  });
});

describe("context helpers", () => {
  it("normalizeModelCallError 只保留 name + 截断 message", () => {
    expect(normalizeModelCallError(null)).toEqual({ name: null, message: null });
    expect(normalizeModelCallError(new TypeError("bad"))).toEqual({ name: "TypeError", message: "bad" });
    const long = normalizeModelCallError(new Error("x".repeat(5_000)));
    expect(long.message!.length).toBeLessThan(1_100);
    expect(long.message).toContain("[truncated]");
  });

  it("modelCallFieldsFromUsageContext 复用 usage-context 归一化，不为 unknown 造假", () => {
    const mapped = modelCallFieldsFromUsageContext({
      source: { subsystem: "session", operation: "reply", surface: "desktop", trigger: "user" },
      attribution: { kind: "session", agentId: "a1" },
    });
    expect(mapped.source.subsystem).toBe("session");
    expect(mapped.attribution).toMatchObject({ kind: "session", agentId: "a1" });

    const unknown = modelCallFieldsFromUsageContext("approval_reviewer_authorization");
    expect(unknown.source).toMatchObject({
      subsystem: "unknown",
      operation: "unknown",
      surface: "unknown",
      trigger: "unknown",
    });
    expect(unknown.attribution).toEqual({ kind: "unknown" });
    expect(modelCallFieldsFromUsageContext(null).attribution).toEqual({ kind: "unknown" });
  });
});
