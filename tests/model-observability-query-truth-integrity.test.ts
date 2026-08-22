/**
 * Phase 10.1 Query Truth Integrity 失败测试。
 *
 * 这些用例刻意走真实 SQLite writer/read service，锁定：同字段 OR、NULL、
 * 缺失/损坏状态、Trace 选择与完整统计分离、部分载荷丢失和用量覆盖度。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createModelObservabilityQueryService } from "../lib/llm/model-observability-query.ts";
import {
  normalizeModelObservabilityAggregateQuery,
  normalizeModelObservabilityQuery,
  normalizeModelObservabilityTraceQuery,
} from "../lib/llm/model-observability-query-types.ts";
import { modelObservabilityDbPath, openModelObservabilityDatabase } from "../lib/llm/model-observability-schema.ts";

const BASE = Date.UTC(2026, 7, 22, 0, 0, 0, 0);

describe("Model Observatory Phase 10.1 Query Truth Integrity", () => {
  let home: string;
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;
  let service: ReturnType<typeof createModelObservabilityQueryService> | null;

  beforeEach(() => {
    home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-obs-truth-"));
    harness = createModelObservabilityTestHarness({ lingxiHome: home });
    service = null;
  });

  afterEach(async () => {
    service?.close();
    await harness.close();
    harness.cleanup();
  });

  function seedCall(options: {
    callId: string;
    traceId?: string;
    provider?: string;
    subsystem?: string;
    sessionId?: string;
    status?: "ok" | "error" | "aborted" | null;
    at?: number;
  }) {
    const recorder = createModelCallRecorder({
      observer: harness.handle.observer,
      context: {
        callId: options.callId,
        traceId: options.traceId ?? `mt_${options.callId}`,
        parentCallId: null,
        model: { provider: options.provider ?? "openai", modelId: "truth-model", api: "responses" },
        source: {
          subsystem: options.subsystem ?? "llm",
          operation: "truth-test",
          surface: "server",
          trigger: "user_turn",
        },
        attribution: { kind: "session", sessionId: options.sessionId ?? "session-a" },
      },
      now: () => options.at ?? BASE,
    });
    recorder.beginLogicalCall({});
    const attemptId = recorder.beginAttempt({});
    if (options.status === "error") {
      recorder.attemptError(new Error("truth-test-error"));
      recorder.logicalCallError(new Error("truth-test-error"));
      recorder.endLogicalCall("error");
    } else if (options.status === "aborted") {
      recorder.logicalCallAborted({});
      recorder.endLogicalCall("aborted");
    } else if (options.status !== null) {
      recorder.endLogicalCall("ok");
    }
    return { recorder, attemptId };
  }

  function seedPayload(callId: string, payload: unknown = { value: "captured" }) {
    harness.handle.sink?.handleModelCallPayloadRecord({
      schemaVersion: 1,
      kind: "semantic_request",
      capturedAt: new Date(BASE + 1000).toISOString(),
      callId,
      traceId: `mt_${callId}`,
      parentCallId: null,
      attemptId: null,
      providerRequestOrdinal: null,
      model: null,
      source: null,
      attribution: null,
      visibility: "full",
      fidelity: "runtime_exact",
      sanitization: { redacted: false, truncated: false, degraded: false },
      payload,
      semanticInputProvenance: { schemaVersion: 1, inputShape: "chat_context", sections: [] },
      providerRequestProvenance: null,
    } as never);
  }

  function withDb(run: (db: any) => void): void {
    const reader = harness.openReader();
    try {
      run(reader.db);
    } finally {
      reader.close();
    }
  }

  function callIds(filter: Record<string, unknown>): string[] {
    const normalized = normalizeModelObservabilityQuery({ filter, limit: 100 });
    if (normalized.ok === false) throw new Error(normalized.error.message);
    const result = service!.queryCalls(normalized.value);
    if (result.ok === false) throw new Error(result.error.message);
    return result.value.calls.map((call) => call.callId).sort();
  }

  it("terminalStatus 同字段求并集、跨字段求交集", () => {
    seedCall({ callId: "mc_ok", status: "ok", provider: "openai" });
    seedCall({ callId: "mc_error", status: "error", provider: "openai", at: BASE + 1 });
    seedCall({ callId: "mc_incomplete", status: null, provider: "openai", at: BASE + 2 });
    seedCall({ callId: "mc_aborted", status: "aborted", provider: "anthropic", at: BASE + 3 });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });

    expect(callIds({ terminalStatus: ["ok", "error"] })).toEqual(["mc_error", "mc_ok"]);
    expect(callIds({ terminalStatus: ["error", "incomplete"] })).toEqual(["mc_error", "mc_incomplete"]);
    expect(callIds({ terminalStatus: ["ok", "aborted", "incomplete"] }))
      .toEqual(["mc_aborted", "mc_incomplete", "mc_ok"]);
    expect(callIds({ terminalStatus: ["incomplete"] })).toEqual(["mc_incomplete"]);
    expect(callIds({ provider: ["anthropic"], terminalStatus: ["ok", "aborted", "incomplete"] }))
      .toEqual(["mc_aborted"]);
  });

  it("payloadAvailability 同字段求并集，并与 provider/session 求交集", () => {
    seedCall({ callId: "mc_present", provider: "openai", sessionId: "session-a" });
    seedCall({ callId: "mc_dropped", provider: "openai", sessionId: "session-a", at: BASE + 1 });
    seedCall({ callId: "mc_expired", provider: "anthropic", sessionId: "session-a", at: BASE + 2 });
    seedCall({ callId: "mc_not_captured", provider: "anthropic", sessionId: "session-b", at: BASE + 3 });
    seedCall({ callId: "mc_unknown", provider: "openai", sessionId: "session-b", at: BASE + 4 });
    seedPayload("mc_present");
    harness.flush();
    withDb((db) => {
      db.prepare("UPDATE model_calls SET payload_availability = 'dropped' WHERE call_id = 'mc_dropped'").run();
      db.prepare("UPDATE model_calls SET payload_availability = 'expired' WHERE call_id = 'mc_expired'").run();
      db.prepare("UPDATE model_calls SET payload_availability = 'not_captured' WHERE call_id = 'mc_not_captured'").run();
    });
    service = createModelObservabilityQueryService({ lingxiHome: home });

    expect(callIds({ payloadAvailability: ["present", "unknown"] }))
      .toEqual(["mc_present", "mc_unknown"]);
    expect(callIds({ payloadAvailability: ["present", "dropped"] }))
      .toEqual(["mc_dropped", "mc_present"]);
    expect(callIds({ payloadAvailability: ["expired", "not_captured"] }))
      .toEqual(["mc_expired", "mc_not_captured"]);
    expect(callIds({ payloadAvailability: ["present", "expired", "dropped", "unknown"] }))
      .toEqual(["mc_dropped", "mc_expired", "mc_present", "mc_unknown"]);
    expect(callIds({
      provider: ["openai"],
      sessionId: ["session-a"],
      payloadAvailability: ["present", "dropped", "unknown"],
    })).toEqual(["mc_dropped", "mc_present"]);
  });

  it("SQL NULL 保持 null，真实 0 与小数成本保持原值", () => {
    const { attemptId } = seedCall({ callId: "mc_nulls" });
    seedPayload("mc_nulls");
    harness.flush();
    withDb((db) => {
      db.prepare(`UPDATE model_calls SET provenance_section_count = NULL, provenance_opaque_count = NULL
                  WHERE call_id = 'mc_nulls'`).run();
      db.prepare(`UPDATE model_attempts SET http_status = NULL WHERE attempt_id = ?`).run(attemptId);
      db.prepare(`UPDATE payload_records SET provider_request_ordinal = NULL, record_char_count = NULL
                  WHERE call_id = 'mc_nulls'`).run();
      db.prepare(`INSERT INTO model_call_usage (
        model_call_id, usage_status, input_total_tokens, output_total_tokens, reasoning_tokens,
        cache_read_tokens, cache_write_tokens, total_tokens, cost_total, created_at, updated_at
      ) VALUES ('mc_nulls', 'ok', NULL, 0, NULL, NULL, NULL, NULL, 0.001,
        '2026-08-22T00:00:00.000Z', '2026-08-22T00:00:00.000Z')`).run();
    });
    service = createModelObservabilityQueryService({ lingxiHome: home });

    const detail = service.queryCallDetail("mc_nulls");
    if (detail.ok === false) throw new Error(detail.error.message);
    expect(detail.value.call.provenance.sectionCount).toBeNull();
    expect(detail.value.call.provenance.opaqueCount).toBeNull();
    expect(detail.value.attempts[0].httpStatus).toBeNull();
    expect(detail.value.payloadRecords[0].providerRequestOrdinal).toBeNull();
    expect(detail.value.payloadRecords[0].recordCharCount).toBeNull();
    expect(detail.value.call.usage.summary).toMatchObject({
      inputTokens: null,
      outputTokens: 0,
      reasoningTokens: null,
      cacheReadTokens: null,
      cacheWriteTokens: null,
      totalTokens: null,
      costTotal: 0.001,
    });
  });

  it("损坏的 usage 数字显式标为 corrupt，不在明细、Trace 或聚合中冒充 0", () => {
    seedCall({ callId: "mc_corrupt_usage", traceId: "mt_corrupt_usage" });
    harness.flush();
    withDb((db) => {
      db.prepare(`INSERT INTO model_call_usage (
        model_call_id, usage_status, input_total_tokens, output_total_tokens, total_tokens,
        cost_total, created_at, updated_at
      ) VALUES ('mc_corrupt_usage', 'ok', 'broken', 0, 'broken', 'broken',
        '2026-08-22T00:00:00.000Z', '2026-08-22T00:00:00.000Z')`).run();
    });
    service = createModelObservabilityQueryService({ lingxiHome: home });

    const detail = service.queryCallDetail("mc_corrupt_usage");
    if (detail.ok === false) throw new Error(detail.error.message);
    expect(detail.value.call.usage.availability).toBe("corrupt");
    expect(detail.value.call.usage.summary).toBeNull();

    const aggregateQuery = normalizeModelObservabilityAggregateQuery({ filter: {}, groupBy: [] });
    if (aggregateQuery.ok === false) throw new Error(aggregateQuery.error.message);
    const aggregate = service.queryAggregate(aggregateQuery.value);
    if (aggregate.ok === false) throw new Error(aggregate.error.message);
    expect(aggregate.value.overall.usageAggregateAvailability).toBe("corrupt");
    expect(aggregate.value.overall.usageCoveredCalls).toBe(0);
    expect(aggregate.value.overall.usageCorruptCalls).toBe(1);
    expect(aggregate.value.overall.usageUnknownCalls).toBe(0);
    expect(aggregate.value.overall.inputTokens).toBeNull();
    expect(aggregate.value.overall.outputTokens).toBe(0);
    expect(aggregate.value.overall.totalTokens).toBeNull();
    expect(aggregate.value.overall.costTotal).toBeNull();

    const trace = service.queryTraceDetail("mt_corrupt_usage");
    if (trace.ok === false) throw new Error(trace.error.message);
    expect(trace.value.usageAggregate.availability).toBe("corrupt");
    expect(trace.value.usageAggregate.corruptCalls).toBe(1);
    expect(trace.value.usageAggregate.summary).toBeNull();
  });

  it("完整性 key 缺失是 known zero，损坏 JSON 是 unknown/degraded", () => {
    seedCall({ callId: "mc_completeness" });
    harness.flush();
    withDb((db) => {
      db.prepare("DELETE FROM observability_meta WHERE key = 'droppedBlobs'").run();
    });
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const missing = service.getHealth();
    if (missing.ok === false) throw new Error(missing.error.message);
    expect((missing.value.dataCompleteness as any).status).toBe("known");
    expect((missing.value.dataCompleteness as any).droppedBlobs).toBe(0);

    withDb((db) => {
      db.prepare(`INSERT INTO observability_meta (key, value_json) VALUES ('droppedPayloadRecords', '{broken')
                  ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json`).run();
    });
    service.invalidate();
    const corrupt = service.getHealth();
    if (corrupt.ok === false) throw new Error(corrupt.error.message);
    expect(corrupt.value.queryStatus).toBe("degraded");
    expect((corrupt.value.dataCompleteness as any).status).toBe("unknown");
    expect((corrupt.value.dataCompleteness as any).droppedPayloadRecords).toBeNull();
  });

  it("普通调用缺少 usage row 是 unknown，不自动升级成 not_correlated", () => {
    seedCall({ callId: "mc_usage_unknown" });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const result = service.queryCallDetail("mc_usage_unknown");
    if (result.ok === false) throw new Error(result.error.message);
    expect(result.value.call.usage.availability).toBe("unknown");
    expect(result.value.call.usage.summary).toBeNull();
  });

  it("Trace filter 只选择 Trace，统计仍覆盖 Trace 的全部 Calls", () => {
    seedCall({ callId: "mc_trace_1", traceId: "mt_truth", provider: "openai", status: "ok" });
    seedCall({ callId: "mc_trace_2", traceId: "mt_truth", provider: "anthropic", status: "error", at: BASE + 1 });
    seedCall({ callId: "mc_trace_3", traceId: "mt_truth", provider: "anthropic", status: "ok", at: BASE + 2 });
    seedCall({ callId: "mc_other", traceId: "mt_other", provider: "anthropic", status: "ok", at: BASE + 3 });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });

    const normalized = normalizeModelObservabilityTraceQuery({ filter: { provider: ["openai"] }, limit: 50 });
    if (normalized.ok === false) throw new Error(normalized.error.message);
    const traces = service.queryTraces(normalized.value);
    if (traces.ok === false) throw new Error(traces.error.message);
    expect(traces.value.traces).toHaveLength(1);
    expect(traces.value.traces[0]).toMatchObject({
      traceId: "mt_truth",
      callCount: 3,
      terminalOk: 2,
      terminalError: 1,
      terminalAborted: 0,
      incomplete: 0,
    });
    const detail = service.queryTraceDetail("mt_truth");
    if (detail.ok === false) throw new Error(detail.error.message);
    expect(detail.value.calls).toHaveLength(3);
  });

  it("部分 payload 已保存且另有 drop 时，状态保持 dropped 且内容仍可查看", () => {
    seedCall({ callId: "mc_partial_payload" });
    seedPayload("mc_partial_payload", { kept: true });
    harness.flush();
    withDb((db) => {
      db.prepare("UPDATE model_calls SET payload_availability = 'dropped' WHERE call_id = 'mc_partial_payload'").run();
    });
    service = createModelObservabilityQueryService({ lingxiHome: home });

    const detail = service.queryCallDetail("mc_partial_payload");
    if (detail.ok === false) throw new Error(detail.error.message);
    expect(detail.value.call.payloadRecordCount).toBe(1);
    expect(detail.value.call.payloadAvailability).toBe("dropped");
    expect(callIds({ payloadAvailability: ["dropped"] })).toEqual(["mc_partial_payload"]);
    const payload = service.getPayloadRecord(detail.value.payloadRecords[0].id);
    if (payload.ok === false) throw new Error(payload.error.message);
    expect(payload.value.contentAvailable).toBe(true);
    expect(payload.value.payload).toEqual({ kept: true });
  });

  it("用量聚合区分 unknown、partial 和 projection_unavailable", async () => {
    for (let index = 0; index < 10; index += 1) {
      seedCall({ callId: `mc_aggregate_${index}`, traceId: "mt_aggregate", at: BASE + index });
    }
    harness.flush();
    withDb((db) => {
      const insert = db.prepare(`INSERT INTO model_call_usage (
        model_call_id, usage_status, input_total_tokens, output_total_tokens, total_tokens,
        created_at, updated_at
      ) VALUES (?, 'ok', 10, 5, 15, '2026-08-22T00:00:00.000Z', '2026-08-22T00:00:00.000Z')`);
      for (let index = 0; index < 5; index += 1) insert.run(`mc_aggregate_${index}`);
    });
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const normalized = normalizeModelObservabilityAggregateQuery({ filter: {}, groupBy: [] });
    if (normalized.ok === false) throw new Error(normalized.error.message);
    const partial = service.queryAggregate(normalized.value);
    if (partial.ok === false) throw new Error(partial.error.message);
    expect((partial.value.overall as any).usageAggregateAvailability).toBe("partial");
    expect(partial.value.overall.usageCoveredCalls).toBe(5);
    expect(partial.value.overall.totalTokens).toBe(75);

    withDb((db) => db.prepare("DELETE FROM model_call_usage").run());
    service.invalidate();
    const unknown = service.queryAggregate(normalized.value);
    if (unknown.ok === false) throw new Error(unknown.error.message);
    expect((unknown.value.overall as any).usageAggregateAvailability).toBe("unknown");
    expect(unknown.value.overall.totalTokens).toBeNull();

    service.close();
    service = null;
    await harness.close();
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.exec(`
      DROP INDEX IF EXISTS idx_model_call_usage_status;
      DROP INDEX IF EXISTS idx_model_calls_conversation;
      DROP TABLE IF EXISTS model_call_usage;
    `);
    db.pragma("user_version = 1");
    db.close();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const unavailable = service.queryAggregate(normalized.value);
    if (unavailable.ok === false) throw new Error(unavailable.error.message);
    expect((unavailable.value.overall as any).usageAggregateAvailability).toBe("projection_unavailable");
    expect(unavailable.value.overall.totalTokens).toBeNull();
  });

  it("IANA 日期分桶不会因窗口两端 offset 相同而漏掉中间两次切换", () => {
    seedCall({ callId: "mc_winter_start", at: Date.parse("2026-01-15T08:30:00.000Z") });
    seedCall({ callId: "mc_summer", at: Date.parse("2026-07-15T07:30:00.000Z") });
    seedCall({ callId: "mc_winter_end", at: Date.parse("2026-12-15T08:30:00.000Z") });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const normalized = normalizeModelObservabilityAggregateQuery({
      filter: {},
      groupBy: ["date"],
      dateBucket: { bucket: "day", timeZone: "America/Los_Angeles" },
    });
    if (normalized.ok === false) throw new Error(normalized.error.message);
    const result = service.queryAggregate(normalized.value);
    if (result.ok === false) throw new Error(result.error.message);
    expect(result.value.groups.map((group) => group.values.date).sort()).toEqual([
      "2026-01-15",
      "2026-07-15",
      "2026-12-15",
    ]);
  });

  it("IANA 日期分桶超过有界段数时显式失败，不静默复用旧 offset", () => {
    seedCall({ callId: "mc_old_range", at: Date.parse("2000-01-01T08:30:00.000Z") });
    seedCall({ callId: "mc_new_range", at: Date.parse("2026-12-31T08:30:00.000Z") });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const normalized = normalizeModelObservabilityAggregateQuery({
      filter: {},
      groupBy: ["date"],
      dateBucket: { bucket: "day", timeZone: "America/Los_Angeles" },
    });
    if (normalized.ok === false) throw new Error(normalized.error.message);
    const result = service.queryAggregate(normalized.value);
    expect(result).toEqual({
      ok: false,
      error: {
        code: "query_failed",
        message: "date bucket range exceeds the supported transition limit",
        reasonCode: "date_bucket_segment_limit_exceeded",
      },
    });
  });

  it("provenance JSON 损坏不会伪装成没有 provenance", () => {
    seedCall({ callId: "mc_corrupt_provenance" });
    seedPayload("mc_corrupt_provenance");
    harness.flush();
    withDb((db) => {
      db.prepare(`UPDATE payload_records
                  SET semantic_input_provenance_json = '{broken', provider_request_provenance_json = '[broken'
                  WHERE call_id = 'mc_corrupt_provenance'`).run();
    });
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const detail = service.queryCallDetail("mc_corrupt_provenance");
    if (detail.ok === false) throw new Error(detail.error.message);
    const payload = service.getPayloadRecord(detail.value.payloadRecords[0].id);
    if (payload.ok === false) throw new Error(payload.error.message);
    expect((payload.value as any).semanticInputProvenanceState).toBe("corrupt");
    expect((payload.value as any).providerRequestProvenanceState).toBe("corrupt");
    expect(payload.value.semanticInputProvenance).toBeNull();
    expect(payload.value.providerRequestProvenance).toBeNull();
  });

  it("payload JSON 损坏显式返回 corrupt，不伪装成空正文", () => {
    seedCall({ callId: "mc_corrupt_payload" });
    seedPayload("mc_corrupt_payload");
    harness.flush();
    withDb((db) => {
      db.prepare("UPDATE payload_records SET payload_json = '{broken' WHERE call_id = ?")
        .run("mc_corrupt_payload");
    });
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const detail = service.queryCallDetail("mc_corrupt_payload");
    if (detail.ok === false) throw new Error(detail.error.message);
    const payload = service.getPayloadRecord(detail.value.payloadRecords[0].id);
    if (payload.ok === false) throw new Error(payload.error.message);
    expect(payload.value.contentAvailable).toBe(false);
    expect(payload.value.contentState).toBe("corrupt");
    expect(payload.value.payload).toBeNull();
  });

  it("调用类别 JSON 损坏不会伪装成确实没有类别", () => {
    seedCall({ callId: "mc_corrupt_categories" });
    harness.flush();
    withDb((db) => {
      db.prepare("UPDATE model_calls SET provenance_categories_json = '{broken' WHERE call_id = ?")
        .run("mc_corrupt_categories");
    });
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const detail = service.queryCallDetail("mc_corrupt_categories");
    if (detail.ok === false) throw new Error(detail.error.message);
    expect(detail.value.call.provenance.categories).toEqual([]);
    expect(detail.value.call.provenance.categoriesState).toBe("corrupt");
  });
});
