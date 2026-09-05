/**
 * 用量/用时胶囊 · 任务1：query 层透出未缓存输入（inputUncachedTokens）。
 *
 * 走真实 SQLite writer/read service（同 tests/model-observability-query-truth-integrity.test.ts
 * 的 harness 形态），锁定：call 投影 usage.summary.inputUncachedTokens 读
 * model_call_usage.input_uncached_tokens；SQL NULL → null（不冒充 0）；负值按既有
 * USAGE_INTEGER_FIELDS 口径整行标 corrupt；Trace 聚合按 sumKnown 求和并跳过 null。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { createModelObservabilityTestHarness } from "../../../../../lib/llm/model-observability-testing.ts";
import { createModelCallRecorder } from "../../../../../lib/llm/model-call-recorder.ts";
import { createModelObservabilityQueryService } from "../../../../../lib/llm/model-observability-query.ts";
import { normalizeModelObservabilityQuery } from "../../../../../lib/llm/model-observability-query-types.ts";

const BASE = Date.UTC(2026, 8, 5, 0, 0, 0, 0);

describe("chat 用量胶囊 · query 层未缓存输入透出", () => {
  let home: string;
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;
  let service: ReturnType<typeof createModelObservabilityQueryService> | null;

  beforeEach(() => {
    home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-chat-uncached-"));
    harness = createModelObservabilityTestHarness({ lingxiHome: home });
    service = null;
  });

  afterEach(async () => {
    service?.close();
    await harness.close();
    harness.cleanup();
  });

  function seedCall(callId: string, sessionId: string) {
    const recorder = createModelCallRecorder({
      observer: harness.handle.observer,
      context: {
        callId,
        traceId: `mt_${callId}`,
        parentCallId: null,
        model: { provider: "openai", modelId: "pill-model", api: "responses" },
        source: { subsystem: "llm", operation: "chat", surface: "server", trigger: "user_turn" },
        attribution: { kind: "session", sessionId },
      },
      now: () => BASE,
    });
    recorder.beginLogicalCall({ details: { traceOrigin: "user_turn" } });
    recorder.beginAttempt({});
    recorder.endLogicalCall("ok");
  }

  function seedUsage(callId: string, values: {
    inputTotal: number | null;
    inputUncached: number | null;
    output: number | null;
    total: number | null;
  }) {
    const reader = harness.openReader();
    try {
      reader.db.prepare(
        `INSERT INTO model_call_usage (
           model_call_id, usage_status, input_total_tokens, input_uncached_tokens,
           output_total_tokens, total_tokens, created_at, updated_at
         ) VALUES (?, 'ok', ?, ?, ?, ?, ?, ?)`,
      ).run(
        callId,
        values.inputTotal,
        values.inputUncached,
        values.output,
        values.total,
        new Date(BASE).toISOString(),
        new Date(BASE).toISOString(),
      );
    } finally {
      reader.close();
    }
  }

  function queryCalls() {
    const normalized = normalizeModelObservabilityQuery({ filter: {}, limit: 100 });
    if (normalized.ok === false) throw new Error(normalized.error.message);
    const result = service!.queryCalls(normalized.value);
    if (result.ok === false) throw new Error(result.error.message);
    return result.value.calls;
  }

  it("call 投影 usage.summary 透出 inputUncachedTokens，与含缓存总输入分开", () => {
    seedCall("mc_pill_uncached", "session-pill");
    seedUsage("mc_pill_uncached", { inputTotal: 1000, inputUncached: 250, output: 40, total: 1040 });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });

    const calls = queryCalls();
    expect(calls).toHaveLength(1);
    expect(calls[0].callId).toBe("mc_pill_uncached");
    expect(calls[0].usage.availability).toBe("present");
    expect(calls[0].usage.summary?.inputTokens).toBe(1000);
    expect(calls[0].usage.summary?.inputUncachedTokens).toBe(250);
    expect(calls[0].usage.summary?.outputTokens).toBe(40);
    expect(calls[0].usage.summary?.totalTokens).toBe(1040);
  });

  it("input_uncached_tokens 为 SQL NULL 时投影为 null，不冒充 0", () => {
    seedCall("mc_pill_null", "session-pill");
    seedUsage("mc_pill_null", { inputTotal: 500, inputUncached: null, output: 20, total: 520 });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });

    const calls = queryCalls();
    expect(calls[0].usage.availability).toBe("present");
    expect(calls[0].usage.summary?.inputTokens).toBe(500);
    expect(calls[0].usage.summary?.inputUncachedTokens).toBeNull();
  });

  it("input_uncached_tokens 为负值时按 corrupt 处理，summary 整体置 null", () => {
    seedCall("mc_pill_corrupt", "session-pill");
    seedUsage("mc_pill_corrupt", { inputTotal: 100, inputUncached: -5, output: 10, total: 110 });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });

    const calls = queryCalls();
    expect(calls[0].usage.availability).toBe("corrupt");
    expect(calls[0].usage.summary).toBeNull();
  });

  it("无 usage 行的 call 投影 availability=unknown、summary=null（历史会话语义）", () => {
    seedCall("mc_pill_nousage", "session-pill");
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });

    const calls = queryCalls();
    expect(calls[0].usage.availability).toBe("unknown");
    expect(calls[0].usage.summary).toBeNull();
  });
});
