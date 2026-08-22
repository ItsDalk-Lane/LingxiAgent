/**
 * Phase 7 Trace Projection 测试（任务书 §九十五/九十六/一百零六/一百一十二）：
 * Trace T1{C1(C2,C3)} 树投影 / attempt A1+A2 独立性与时间戳 / crash 未完成
 * call 不伪造终态 / restart roundtrip 完整恢复。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { installModelObservabilityPersistence } from "../lib/llm/model-observability-persistence.ts";

const MODEL = { provider: "anthropic", modelId: "claude-x", api: "anthropic-messages" };
const SOURCE = { subsystem: "llm", operation: "callText", surface: "server", trigger: "user_turn" };

function attribution(overrides: Record<string, unknown> = {}) {
  return {
    kind: "session",
    sessionId: "sess-1",
    sessionPath: "/home/agents/yuan/sessions/sess-1.jsonl",
    conversationId: "conv-1",
    conversationType: "dm",
    agentId: "yuan",
    taskId: null,
    ...overrides,
  };
}

describe("Model Observability Trace Projection", () => {
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;

  beforeEach(() => {
    harness = createModelObservabilityTestHarness();
  });
  afterEach(async () => {
    await harness.close();
    harness.cleanup();
  });

  it("Trace T1：C1(root) + C2/C3(children)，树结构与 attribution 全部投影正确（§九十五）", () => {
    const traceId = "mt_tree1";
    const mkRecorder = (callId: string, parentCallId: string | null) => createModelCallRecorder({
      observer: harness.handle.observer,
      identity: { mintCallId: () => callId, mintAttemptId: () => `ma_${callId}_a`, mintTraceId: () => traceId },
      context: {
        callId, traceId, parentCallId,
        model: MODEL, source: SOURCE,
        attribution: attribution(callId === "mc_c1" ? {} : { childAgentId: "helper", childSessionId: "sess-2" }),
      },
    });

    const c1 = mkRecorder("mc_c1", null);
    c1.beginLogicalCall({ details: { traceOrigin: "user_turn", callPurpose: "chat" } });
    c1.beginAttempt();
    c1.providerResponseReceived({ httpStatus: 200, providerRequestId: "req_c1" });
    c1.semanticResponseCompleted({ details: { stopReason: "tool_use" } });
    c1.endLogicalCall("ok");

    const c2 = mkRecorder("mc_c2", "mc_c1");
    c2.beginLogicalCall({});
    c2.beginAttempt();
    c2.endLogicalCall("ok");

    const c3 = mkRecorder("mc_c3", "mc_c1");
    c3.beginLogicalCall({});
    c3.beginAttempt();
    c3.logicalCallAborted({});
    c3.endLogicalCall("aborted");

    harness.flush();

    const reader = harness.openReader();
    try {
      const trace = reader.traceStore.getTrace(traceId);
      expect(trace).not.toBeNull();
      expect(trace.origin).toBe("user_turn");
      expect(trace.call_count).toBe(3);
      expect(String(trace.first_seen_at) <= String(trace.last_seen_at)).toBe(true);

      const row1 = reader.traceStore.getCall("mc_c1");
      expect(row1).toMatchObject({
        call_id: "mc_c1",
        trace_id: traceId,
        parent_call_id: null,
        provider: "anthropic",
        model_id: "claude-x",
        api: "anthropic-messages",
        subsystem: "llm",
        operation: "callText",
        attribution_kind: "session",
        session_id: "sess-1",
        agent_id: "yuan",
        conversation_id: "conv-1",
        call_purpose: "chat",
        terminal_status: "ok",
        persistence_completeness: "complete",
      });
      expect(row1.started_at).not.toBeNull();
      expect(row1.semantic_completed_at).not.toBeNull();
      expect(row1.ended_at).not.toBeNull();
      expect(row1.interrupted_by_restart).toBe(0);

      const row2 = reader.traceStore.getCall("mc_c2");
      expect(row2.parent_call_id).toBe("mc_c1");
      expect(row2.child_agent_id).toBe("helper");
      expect(row2.terminal_status).toBe("ok");

      const row3 = reader.traceStore.getCall("mc_c3");
      expect(row3.parent_call_id).toBe("mc_c1");
      expect(row3.terminal_status).toBe("aborted");
    } finally {
      reader.close();
    }
  });

  it("同一 call 两个 attempt：独立 rows、时间戳与 providerRequestId/httpStatus 各归各位（§九十六）", () => {
    const rec = createModelCallRecorder({
      observer: harness.handle.observer,
      identity: {
        mintCallId: () => "mc_two_attempts",
        mintAttemptId: (() => {
          let n = 0;
          return () => `ma_att_${++n}`;
        })(),
        mintTraceId: () => "mt_att",
      },
      context: { traceId: "mt_att", model: MODEL, source: SOURCE, attribution: attribution() },
    });
    rec.beginLogicalCall({});
    const a1 = rec.beginAttempt({ details: { attemptVisibility: "exact", providerWireVisibility: "request_response" } });
    rec.providerRequestPrepared({ details: { messageCount: 5 } });
    rec.attemptError(Object.assign(new Error("429 rate limited"), { code: "E429" }));
    const a2 = rec.beginAttempt({ details: { attemptVisibility: "exact", providerWireVisibility: "request_response" } });
    rec.providerRequestPrepared({ details: { messageCount: 5 } });
    rec.providerResponseReceived({ httpStatus: 200, providerRequestId: "req_retry_ok" });
    rec.endLogicalCall("ok");
    harness.flush();

    const reader = harness.openReader();
    try {
      const attempts = reader.traceStore.getAttempts("mc_two_attempts");
      expect(attempts).toHaveLength(2);
      const first = attempts.find((a) => a.attempt_id === a1);
      const second = attempts.find((a) => a.attempt_id === a2);
      expect(first).toBeDefined();
      expect(second).toBeDefined();
      expect(first.call_id).toBe("mc_two_attempts");
      expect(first.request_prepared_at).not.toBeNull();
      expect(first.response_received_at).toBeNull();
      expect(first.error_at).not.toBeNull();
      expect(first.error_name).toBe("Error");
      expect(first.error_code).toBe("E429");
      expect(first.attempt_visibility).toBe("exact");
      expect(first.provider_wire_visibility).toBe("request_response");
      expect(second.response_received_at).not.toBeNull();
      expect(second.http_status).toBe(200);
      expect(second.provider_request_id).toBe("req_retry_ok");
      expect(second.error_at).toBeNull();
    } finally {
      reader.close();
    }
  });

  it("crash 语义：start+attempt+provider_request 后不发 end → 重开后 terminal_status 保持 NULL + interrupted_by_restart=1（§一百零六/四十六/四十七）", async () => {
    const rec = createModelCallRecorder({
      observer: harness.handle.observer,
      identity: {
        mintCallId: () => "mc_crashed",
        mintAttemptId: () => "ma_crashed_1",
        mintTraceId: () => "mt_crash",
      },
      context: { traceId: "mt_crash", model: MODEL, source: SOURCE, attribution: attribution() },
    });
    rec.beginLogicalCall({});
    rec.beginAttempt();
    rec.providerRequestPrepared({ details: { messageCount: 2 } });
    harness.flush();
    // 模拟 crash：不 endLogicalCall，直接 close。之后真实 restart（重新 install）
    // 触发 Startup Reconciliation（§四十六）。
    await harness.close();

    const restarted = installModelObservabilityPersistence({
      lingxiHome: harness.lingxiHome,
      policy: { enabled: true, persistPayloads: true },
    });
    try {
      expect(restarted.getHealth().interruptedByRestartCalls).toBe(1);
      const reader = harness.openReader();
      try {
        const call = reader.traceStore.getCall("mc_crashed");
        expect(call).not.toBeNull();
        expect(call.terminal_status).toBeNull();
        expect(call.ended_at).toBeNull();
        expect(call.interrupted_by_restart).toBe(1);
        const attempts = reader.traceStore.getAttempts("mc_crashed");
        expect(attempts).toHaveLength(1);
        expect(attempts[0].request_prepared_at).not.toBeNull();
      } finally {
        reader.close();
      }
    } finally {
      await restarted.close();
    }
  });

  it("Restart Roundtrip：write→flush→close→重开→完整恢复 Trace/Call/Attempt（§一百一十二）", async () => {
    const rec = createModelCallRecorder({
      observer: harness.handle.observer,
      identity: {
        mintCallId: () => "mc_rt",
        mintAttemptId: () => "ma_rt_1",
        mintTraceId: () => "mt_rt",
      },
      context: { traceId: "mt_rt", model: MODEL, source: SOURCE, attribution: attribution() },
    });
    rec.beginLogicalCall({ details: { traceOrigin: "slash_command" } });
    rec.beginAttempt();
    rec.providerResponseReceived({ httpStatus: 200, providerRequestId: "req_rt" });
    rec.endLogicalCall("ok");
    harness.flush();
    await harness.close();

    const reader = harness.openReader();
    try {
      const trace = reader.traceStore.getTrace("mt_rt");
      expect(trace).not.toBeNull();
      expect(trace.origin).toBe("slash_command");
      const call = reader.traceStore.getCall("mc_rt");
      expect(call.terminal_status).toBe("ok");
      expect(call.persistence_completeness).toBe("complete");
      const attempts = reader.traceStore.getAttempts("mc_rt");
      expect(attempts).toHaveLength(1);
      expect(attempts[0].provider_request_id).toBe("req_rt");
    } finally {
      reader.close();
    }
  });

  it("logical_call_start 溢出被丢弃后 attempt_start 仍建 call shell（诚实 partial，不虚构 started_at，§二十三/四十）", () => {
    const rec = createModelCallRecorder({
      observer: harness.handle.observer,
      identity: {
        mintCallId: () => "mc_orphan_attempt",
        mintAttemptId: () => "ma_orphan_1",
        mintTraceId: () => "mt_orphan",
      },
      context: { traceId: "mt_orphan", model: MODEL, source: SOURCE, attribution: attribution() },
    });
    // 只发 attempt_start（模拟 start 事件被队列丢弃/缺失）。
    rec.beginAttempt();
    harness.flush();
    const reader = harness.openReader();
    try {
      const call = reader.traceStore.getCall("mc_orphan_attempt");
      expect(call).not.toBeNull();
      expect(call.started_at).toBeNull();
      expect(call.persistence_completeness).toBe("partial");
      expect(reader.traceStore.getAttempts("mc_orphan_attempt")).toHaveLength(1);
    } finally {
      reader.close();
    }
  });
});
