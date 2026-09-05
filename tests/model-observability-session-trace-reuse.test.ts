/**
 * 会话级轨迹复用（产品口径 2026-09-05）端到端验证：
 *   - 同一会话两次 prompt → 同一 traceId；轨迹列表仅一行，callCount/终态计数累加；
 *   - findReusableSessionTraceId：命中最近 user_turn 轨迹；非 user_turn 不复用；
 *     未知会话 → null；
 *   - 真实生产 wiring（installModelObservabilityPersistence + query service），
 *     无 mock。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { createModelObservabilityQueryService } from "../lib/llm/model-observability-query.ts";
import { normalizeModelObservabilityTraceQuery } from "../lib/llm/model-observability-query-types.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { resolveModelTraceContext, runWithModelTraceRoot } from "../lib/llm/model-trace-scope.ts";

const T = (day: number, minute = 0) => Date.UTC(2026, 8, day, 10, minute, 0, 0);

describe("会话级轨迹复用（同会话多轮 → 同一轨迹）", () => {
  let home: string;
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;
  let service: ReturnType<typeof createModelObservabilityQueryService>;
  let clockMs: number;

  beforeEach(() => {
    home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-session-trace-"));
    harness = createModelObservabilityTestHarness({ lingxiHome: home });
    service = createModelObservabilityQueryService({ lingxiHome: home });
    clockMs = T(1);
  });
  afterEach(async () => {
    service?.close?.();
    await harness.close();
    harness.cleanup();
  });

  function queryTraces(minCallCount: number | null = 1) {
    const normalized = normalizeModelObservabilityTraceQuery({ filter: {}, minCallCount });
    if (normalized.ok !== true) throw new Error(`normalize failed: ${JSON.stringify(normalized)}`);
    const page = service.queryTraces(normalized.value);
    if (page.ok !== true) throw new Error(`queryTraces failed: ${JSON.stringify(page)}`);
    return page.value;
  }

  /** 模拟 session-coordinator.prompt()：查复用 id → runWithModelTraceRoot → 单次模型调用。 */
  function runUserTurn(options: {
    sessionId: string;
    callId: string;
    status?: "ok" | "error";
  }) {
    const at = clockMs;
    return runWithModelTraceRoot(
      {
        origin: "user_turn",
        refs: { sessionId: options.sessionId },
        reuseTraceId: harness.handle.findReusableSessionTraceId(options.sessionId),
      },
      () => {
        const resolved = resolveModelTraceContext();
        const recorder = createModelCallRecorder({
          observer: harness.handle.observer,
          context: {
            callId: options.callId,
            traceId: resolved.traceId,
            parentCallId: resolved.parentCallId,
            model: { provider: "openai", modelId: "gpt-x", api: "responses" },
            source: { subsystem: "session", operation: "reply", surface: "desktop", trigger: "user_turn" },
            attribution: { kind: "session", sessionId: options.sessionId },
          },
          now: () => at,
        });
        recorder.beginLogicalCall({ details: { traceOrigin: "user_turn" } });
        recorder.beginAttempt({});
        recorder.providerResponseReceived({ httpStatus: 200 });
        if (options.status === "error") recorder.endLogicalCall("error");
        else recorder.endLogicalCall("ok");
        return { traceId: resolved.traceId, parentCallId: resolved.parentCallId };
      },
    );
  }

  it("同会话第二轮复用第一轮 traceId；列表一行、计数累加", () => {
    clockMs = T(1);
    const turn1 = runUserTurn({ sessionId: "sess_a", callId: "mc_t1_c1" });
    clockMs = T(1, 5);
    const turn2 = runUserTurn({ sessionId: "sess_a", callId: "mc_t2_c1" });

    expect(turn1.traceId).toMatch(/^mt_/);
    expect(turn2.traceId).toBe(turn1.traceId);
    // 复用轮是同轨迹内的新根调用：不跨轮伪造因果
    expect(turn1.parentCallId).toBeNull();
    expect(turn2.parentCallId).toBeNull();

    harness.flush();
    const page = queryTraces();
    expect(page.traces).toHaveLength(1);
    const trace = page.traces[0]!;
    expect(trace.traceId).toBe(turn1.traceId);
    expect(trace.origin).toBe("user_turn");
    expect(trace.callCount).toBe(2);
    expect(trace.terminalOk).toBe(2);
    expect(trace.terminalError).toBe(0);
    expect(trace.lastSeenAt >= trace.firstSeenAt).toBe(true);

    // 响应到达事实进入详情（请求计时「响应到达/生成/吞吐」的数据源）。
    const detail = service.queryTraceDetail(turn1.traceId);
    if (detail.ok !== true) throw new Error(`queryTraceDetail failed: ${JSON.stringify(detail)}`);
    const replayedCall = detail.value.calls.find((call) => call.callId === "mc_t2_c1");
    expect(replayedCall?.firstResponseAt).not.toBeNull();
  });

  it("不同会话各自成轨迹，互不复用", () => {
    clockMs = T(1);
    const a = runUserTurn({ sessionId: "sess_a", callId: "mc_a_c1" });
    clockMs = T(1, 10);
    const b = runUserTurn({ sessionId: "sess_b", callId: "mc_b_c1" });

    expect(b.traceId).not.toBe(a.traceId);
    harness.flush();
    const page = queryTraces();
    expect(page.traces.map((trace) => trace.traceId).sort()).toEqual([a.traceId, b.traceId].sort());
  });

  it("ingress 形态（origin=unknown）的桌面轮同样被复用并入列表", () => {
    // 2026-09-05 实测：桌面 turn 历史上经 pi ingress 落成 origin=unknown——
    // 复用口径必须按「调用归属会话」而不是 origin='user_turn'。
    clockMs = T(1);
    const turn1 = runWithModelTraceRoot(
      { origin: "unknown", refs: { sessionId: "sess_ing" } },
      () => {
        const resolved = resolveModelTraceContext();
        const recorder = createModelCallRecorder({
          observer: harness.handle.observer,
          context: {
            callId: "mc_ing_c1",
            traceId: resolved.traceId,
            parentCallId: resolved.parentCallId,
            model: { provider: "openai", modelId: "gpt-x", api: "responses" },
            source: { subsystem: "session", operation: "reply", surface: "desktop", trigger: "user" },
            attribution: { kind: "session", sessionId: "sess_ing" },
          },
          now: () => clockMs,
        });
        recorder.beginLogicalCall({ details: { traceOrigin: "unknown" } });
        recorder.endLogicalCall("ok");
        return { traceId: resolved.traceId };
      },
    );
    clockMs = T(1, 5);
    const turn2 = runUserTurn({ sessionId: "sess_ing", callId: "mc_ing_c2" });
    expect(turn2.traceId).toBe(turn1.traceId);

    harness.flush();
    expect(harness.handle.findReusableSessionTraceId("sess_ing")).toBe(turn1.traceId);
    const page = queryTraces();
    const row = page.traces.find((trace) => trace.traceId === turn1.traceId);
    expect(row).toMatchObject({ callCount: 2, terminalOk: 2 });
  });

  it("singleton 辅助调用（无任务根）不复用、默认不进轨迹列表", () => {
    clockMs = T(1);
    const turn1 = runUserTurn({ sessionId: "sess_mix", callId: "mc_mix_c1" });
    clockMs = T(1, 2);
    // 无任何 trace 根：llm-client 单例兜底（origin 空、details 无 traceOrigin）。
    const singleton = resolveModelTraceContext();
    const recorder = createModelCallRecorder({
      observer: harness.handle.observer,
      context: {
        callId: "mc_mix_aux",
        traceId: singleton.traceId,
        parentCallId: null,
        model: { provider: "openai", modelId: "gpt-x", api: "responses" },
        source: { subsystem: "auxiliary", operation: "translate_skill_names", surface: "system", trigger: "startup" },
        attribution: { kind: "auxiliary", sessionId: "sess_mix" },
      },
      now: () => clockMs,
    });
    recorder.beginLogicalCall({});
    recorder.endLogicalCall("ok");
    harness.flush();

    // singleton 即使挂着 sessionId 也不作为复用目标（origin IS NOT NULL 口径）。
    expect(harness.handle.findReusableSessionTraceId("sess_mix")).toBe(turn1.traceId);

    // 默认列表：只显示会话轨迹；includeSingleton 才显示辅助轨迹。
    const page = queryTraces();
    expect(page.traces.map((trace) => trace.traceId)).toEqual([turn1.traceId]);
    const withSingleton = normalizeModelObservabilityTraceQuery({ filter: {}, minCallCount: 1, includeSingleton: true });
    if (withSingleton.ok !== true) throw new Error(`normalize failed: ${JSON.stringify(withSingleton)}`);
    const singletonPage = service.queryTraces(withSingleton.value);
    if (singletonPage.ok !== true) throw new Error(`queryTraces failed: ${JSON.stringify(singletonPage)}`);
    expect(singletonPage.value.traces.map((trace) => trace.traceId).sort())
      .toEqual([turn1.traceId, singleton.traceId].sort());
  });

  it("findReusableSessionTraceId：未知会话 → null", () => {
    expect(harness.handle.findReusableSessionTraceId("sess_unknown")).toBeNull();
    expect(harness.handle.findReusableSessionTraceId(null)).toBeNull();
    expect(harness.handle.findReusableSessionTraceId("")).toBeNull();
  });

  it("失败轮次计入 terminalError，复用查找仍命中", () => {
    clockMs = T(1);
    const turn1 = runUserTurn({ sessionId: "sess_err", callId: "mc_e1" });
    clockMs = T(1, 3);
    const turn2 = runUserTurn({ sessionId: "sess_err", callId: "mc_e2", status: "error" });
    expect(turn2.traceId).toBe(turn1.traceId);

    harness.flush();
    expect(harness.handle.findReusableSessionTraceId("sess_err")).toBe(turn1.traceId);
    const page = queryTraces();
    expect(page.traces[0]).toMatchObject({
      traceId: turn1.traceId,
      callCount: 2,
      terminalOk: 1,
      terminalError: 1,
    });
  });
});
