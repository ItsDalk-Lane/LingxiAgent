/**
 * Phase 8 Unified Query 测试（任务书 §九十六～一百零八）：
 * filters / AND-OR / pagination keyset / same timestamp / NULL started_at /
 * cursor tamper / SQL injection / group by / accounting coverage / trace
 * graph（orphan + cycle）/ call detail（MC-06 codex 401）/ opaque-unavailable
 * / payload corrupt fail-safe / date bucket 时区 / EXPLAIN QUERY PLAN。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createModelObservabilityQueryService } from "../lib/llm/model-observability-query.ts";
import {
  encodeModelObservabilityCallCursor,
  normalizeModelObservabilityAggregateQuery,
  normalizeModelObservabilityQuery,
  normalizeModelObservabilityTraceQuery,
} from "../lib/llm/model-observability-query-types.ts";
import { openModelObservabilityDatabase, modelObservabilityDbPath } from "../lib/llm/model-observability-schema.ts";

const T = (day: number, hour = 0, minute = 0) => Date.UTC(2026, 7, day, hour, minute, 0, 0);

function usageFixture(callId: string, overrides: Record<string, unknown> = {}) {
  return {
    schemaVersion: 1,
    requestId: `llm_${callId}`,
    startedAt: "2026-08-01T00:00:00.000Z",
    endedAt: "2026-08-01T00:00:01.000Z",
    durationMs: 1000,
    status: "ok",
    source: { subsystem: "llm", operation: "callText" },
    attribution: { kind: "session" },
    metadata: { modelCallId: callId, traceId: null, parentCallId: null },
    model: { provider: "openai", modelId: "gpt-x", api: "responses" },
    usage: {
      costTotal: 0.01,
      input: { totalTokens: 100, uncachedTokens: 60 },
      output: { totalTokens: 40, reasoningTokens: 10 },
      cache: { readTokens: 40, writeTokens: 10, hit: true, created: true, hitRatio: 0.4, support: "reported" },
      totalTokens: 150,
    },
    rawUsageShape: "input,output",
    error: null,
    ...overrides,
  };
}

describe("Model Observability Unified Query", () => {
  let home: string;
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;
  let service: ReturnType<typeof createModelObservabilityQueryService>;
  let clockMs: number;

  beforeEach(() => {
    home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-query-"));
    clockMs = T(1);
    harness = createModelObservabilityTestHarness({ lingxiHome: home });
  });
  afterEach(async () => {
    service?.close?.();
    await harness.close();
    harness.cleanup();
  });

  /** recorder fixture：时间可控、维度可控。 */
  function seedCall(options: {
    id: string;
    trace: string | null;
    parent?: string | null;
    at: number;
    provider?: string;
    modelId?: string;
    subsystem?: string;
    operation?: string;
    status?: "ok" | "error" | "aborted" | null;
    session?: string;
    agent?: string;
    task?: string;
    conversation?: string;
    attributionKind?: string;
    withAttempt?: boolean;
    details?: Record<string, unknown>;
  }) {
    clockMs = options.at;
    const recorder = createModelCallRecorder({
      observer: harness.handle.observer,
      context: {
        callId: options.id,
        traceId: options.trace,
        parentCallId: options.parent ?? null,
        model: { provider: options.provider ?? "openai", modelId: options.modelId ?? "gpt-x", api: "responses" },
        source: {
          subsystem: options.subsystem ?? "llm",
          operation: options.operation ?? "callText",
          surface: "server",
          trigger: "user_turn",
        },
        attribution: {
          kind: options.attributionKind ?? "session",
          sessionId: options.session ?? "s1",
          conversationId: options.conversation ?? "c1",
          conversationType: "dm",
          agentId: options.agent ?? "a1",
          taskId: options.task ?? "t1",
        },
      },
      now: () => clockMs,
    });
    recorder.beginLogicalCall({ details: options.details ?? { traceOrigin: "user_turn" } });
    if (options.withAttempt !== false) recorder.beginAttempt({});
    if (options.status === "error") {
      recorder.attemptError(new Error("boom"));
      recorder.logicalCallError(new Error("boom"));
      recorder.endLogicalCall("error");
    } else if (options.status === "aborted") {
      recorder.logicalCallAborted({});
      recorder.endLogicalCall("aborted");
    } else if (options.status === null) {
      // incomplete：不 end（模拟 crash / partial）。
    } else {
      recorder.endLogicalCall("ok");
    }
    return recorder;
  }

  function seedFixture() {
    // Day1: openai/llm（mt_a 树：C1→C2,C3）+ anthropic/memory error
    seedCall({ id: "mc_a1", trace: "mt_a", at: T(1, 1), provider: "openai", modelId: "gpt-4o", subsystem: "llm", session: "s1", agent: "a1", task: "t1", conversation: "c1" });
    seedCall({ id: "mc_a2", trace: "mt_a", parent: "mc_a1", at: T(1, 2), provider: "openai", modelId: "gpt-4o", subsystem: "llm", session: "s1", agent: "a1", task: "t1", conversation: "c1" });
    seedCall({ id: "mc_a3", trace: "mt_a", parent: "mc_a1", at: T(1, 3), provider: "openai", modelId: "gpt-4o", subsystem: "llm", status: "aborted", session: "s1", agent: "a1", task: "t1", conversation: "c1" });
    seedCall({ id: "mc_m1", trace: "mt_m", at: T(1, 4), provider: "anthropic", modelId: "claude-x", subsystem: "memory", status: "error", session: "s2", agent: "a2", task: "t2", conversation: "c2" });
    // Day2: volcengine/media ok + openai/llm error
    seedCall({ id: "mc_v1", trace: "mt_v", at: T(2, 1), provider: "volcengine", modelId: "doubao", subsystem: "media", session: "s2", agent: "a2", task: "t2", conversation: "c2" });
    seedCall({ id: "mc_b1", trace: "mt_b", at: T(2, 2), provider: "openai", modelId: "gpt-4o", subsystem: "llm", status: "error", session: "s1", agent: "a1", task: "t1", conversation: "c1" });
    // Day3: incomplete call（crash）
    seedCall({ id: "mc_i1", trace: "mt_i", at: T(3, 1), provider: "anthropic", modelId: "claude-x", subsystem: "speech", status: null, session: "s2", agent: "a2", task: "t2", conversation: "c2" });
    harness.flush();
  }

  function wireUsage(entries: unknown[]) {
    harness.handle.initializeAccounting({
      listLedgerEntries: () => entries,
      subscribeUsage: () => () => { /* 测试无 live 流 */ },
    });
    harness.flush();
  }

  function query(body: Record<string, unknown>) {
    const normalized = normalizeModelObservabilityQuery(body);
    if (normalized.ok === false) throw new Error(`normalize failed: ${normalized.error.message}`);
    return service.queryCalls(normalized.value);
  }

  /** query + ok 解包 + id 排序列表（typecheck 下的规范 narrowing）。 */
  function ids(body: Record<string, unknown>): string[] {
    const result = query(body);
    if (result.ok !== true) throw new Error(result.error.message);
    return result.value.calls.map((c) => c.callId).sort();
  }

  function idList(body: Record<string, unknown>): string[] {
    const result = query(body);
    if (result.ok !== true) throw new Error(result.error.message);
    return result.value.calls.map((c) => c.callId);
  }

  /** normalize + ok 解包（typecheck narrowing 规范）。 */
  function normalizedQueryOf(body: Record<string, unknown>) {
    const normalized = normalizeModelObservabilityQuery(body);
    if (normalized.ok !== true) throw new Error(normalized.error.message);
    return normalized.value;
  }

  function count(body: Record<string, unknown>): number {
    const result = query(body);
    if (result.ok !== true) throw new Error(result.error.message);
    return result.value.calls.length;
  }

  function tracesQuery(body: Record<string, unknown>) {
    const normalized = normalizeModelObservabilityTraceQuery(body);
    if (normalized.ok === false) throw new Error(`normalize failed: ${normalized.error.message}`);
    return service.queryTraces(normalized.value);
  }

  /* ── Filters（§九十六）────────────────────────────────────────────── */

  it("core filters：provider / category(=subsystem) / session / task / trace / status（§九十六）", () => {
    seedFixture();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    expect(ids({ filter: { provider: "openai" } })).toEqual(["mc_a1", "mc_a2", "mc_a3", "mc_b1"]);
    // category ≡ subsystem alias（§十九）。
    expect(idList({ filter: { category: "memory" } })).toEqual(["mc_m1"]);
    expect(idList({ filter: { subsystem: "memory" } })).toEqual(["mc_m1"]);
    expect(ids({ filter: { sessionId: "s2" } })).toEqual(["mc_i1", "mc_m1", "mc_v1"]);
    expect(ids({ filter: { taskId: "t1" } })).toEqual(["mc_a1", "mc_a2", "mc_a3", "mc_b1"]);
    expect(ids({ filter: { traceId: "mt_a" } })).toEqual(["mc_a1", "mc_a2", "mc_a3"]);
    expect(ids({ filter: { terminalStatus: ["error"] } })).toEqual(["mc_b1", "mc_m1"]);
    // incomplete 伪值 → terminal_status IS NULL。
    expect(idList({ filter: { terminalStatus: ["incomplete"] } })).toEqual(["mc_i1"]);
    // date window：since inclusive / until exclusive（§四十四）。
    expect(ids({
      filter: { since: new Date(T(2)).toISOString(), until: new Date(T(3)).toISOString() },
    })).toEqual(["mc_b1", "mc_v1"]);
  });

  it("AND/OR 组合：provider=[openai,anthropic] AND category=llm AND status=error（§九十七/二十）", () => {
    seedFixture();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    expect(idList({
      filter: { provider: ["openai", "anthropic"], category: "llm", terminalStatus: "error" },
    })).toEqual(["mc_b1"]);
  });

  it("pagination：100 calls / pageSize 17 连续翻页，无重复无遗漏，末页 cursor=null（§九十八）", () => {
    for (let i = 0; i < 100; i++) {
      seedCall({ id: `mc_p${String(i).padStart(3, "0")}`, trace: `mt_p${i % 7}`, at: T(10, 0, i) });
    }
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const seen: string[] = [];
    let cursor: string | null = null;
    let pages = 0;
    for (;;) {
      const page = query({ filter: {}, limit: 17, cursor });
      expect(page.ok === true).toBe(true);
      if (page.ok !== true) break;
      seen.push(...page.value.calls.map((c) => c.callId));
      cursor = page.value.nextCursor;
      pages += 1;
      if (!cursor) break;
      expect(pages).toBeLessThan(20);
    }
    expect(seen).toHaveLength(100);
    expect(new Set(seen).size).toBe(100);
    expect(pages).toBe(6);
  });

  it("same timestamp：大量同 started_at 依赖 callId tie-break 仍无重复/遗漏（§九十九）", () => {
    for (let i = 0; i < 40; i++) {
      seedCall({ id: `mc_s${String(i).padStart(2, "0")}`, trace: "mt_same", at: T(11, 5, 0) });
    }
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const seen: string[] = [];
    let cursor: string | null = null;
    for (;;) {
      const page = query({ filter: {}, limit: 7, cursor });
      if (page.ok !== true) throw new Error("query failed");
      seen.push(...page.value.calls.map((c) => c.callId));
      cursor = page.value.nextCursor;
      if (!cursor) break;
    }
    expect(seen).toHaveLength(40);
    expect(new Set(seen).size).toBe(40);
  });

  it("NULL started_at：稳定排在最后且分页终止（§一百）", () => {
    seedFixture();
    // payload-first shell：不经 logical_call_start，直接投 payload record →
    // callShellFromIdentity（started_at NULL）。
    clockMs = T(1, 23);
    harness.handle.sink?.handleModelCallPayloadRecord({
      schemaVersion: 1,
      kind: "provider_request",
      capturedAt: new Date(clockMs).toISOString(),
      callId: "mc_shell",
      traceId: "mt_shell",
      parentCallId: null,
      attemptId: null,
      providerRequestOrdinal: null,
      model: { provider: "openai", modelId: "gpt-x", api: "responses" },
      source: { subsystem: "llm", operation: "callText", surface: "server", trigger: "user_turn" },
      attribution: { kind: "session", sessionId: "s1" },
      visibility: "full",
      fidelity: "runtime_exact",
      sanitization: { redacted: false, truncated: false, degraded: false },
      payload: { transport: { method: "POST" } },
      semanticInputProvenance: null,
      providerRequestProvenance: null,
    } as never);
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const seen: string[] = [];
    let cursor: string | null = null;
    let guard = 0;
    for (;;) {
      guard += 1;
      expect(guard).toBeLessThan(20);
      const page = query({ filter: {}, limit: 3, cursor });
      if (page.ok !== true) throw new Error("query failed");
      seen.push(...page.value.calls.map((c) => c.callId));
      cursor = page.value.nextCursor;
      if (!cursor) break;
    }
    // shell（NULL started_at）排最后。
    expect(seen[seen.length - 1]).toBe("mc_shell");
    expect(new Set(seen).size).toBe(seen.length);
    expect(seen).toHaveLength(8);
  });

  it("cursor tamper：损坏 base64 / 换 filter 复用 / 过长 → invalid_cursor，不 SQL error（§一百零一）", () => {
    seedFixture();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const first = query({ filter: { provider: "openai" }, limit: 2 });
    if (first.ok !== true) throw new Error("first failed");
    expect(first.value.nextCursor).not.toBeNull();

    // 损坏 base64。
    const corrupted = service.queryCalls({
      ...normalizedQueryOf({ filter: { provider: "openai" }, limit: 2 }),
      cursor: "!!!not-base64!!!",
    });
    expect(corrupted.ok === false && corrupted.error.code).toBe("invalid_cursor");
    // 换 filter 后复用旧 cursor（§二十六 fingerprint 绑定）。
    const reused = service.queryCalls({
      ...normalizedQueryOf({ filter: { provider: "anthropic" }, limit: 2 }),
      cursor: first.value.nextCursor,
    });
    expect(reused.ok === false && reused.error.code).toBe("invalid_cursor");
    // 过长 cursor。
    const oversized = service.queryCalls({
      ...normalizedQueryOf({ filter: {}, limit: 2 }),
      cursor: `${first.value.nextCursor}${"A".repeat(600)}`,
    });
    expect(oversized.ok === false && oversized.error.code).toBe("invalid_cursor");
    // 伪造 version。
    const forged = Buffer.from(JSON.stringify({ v: 99, kind: "calls", fp: "0", s: null, c: "mc_a1" })).toString("base64url");
    const badVersion = service.queryCalls({
      ...normalizedQueryOf({ filter: {}, limit: 2 }),
      cursor: forged,
    });
    expect(badVersion.ok === false && badVersion.error.code).toBe("invalid_cursor");
  });

  it("SQL injection：dimension 闭集拒绝 / value 绑定参数 / DB 结构不受影响（§一百零二）", () => {
    seedFixture();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    // groupBy 注入 → normalize 拒绝。
    const badGroup = normalizeModelObservabilityAggregateQuery({
      filter: {},
      groupBy: ["provider); DROP TABLE model_calls; --"],
    });
    expect(badGroup.ok).toBe(false);
    // sort 注入 → 拒绝。
    const badSort = normalizeModelObservabilityQuery({ sort: "call_id; DROP TABLE traces" });
    expect(badSort.ok).toBe(false);
    // value 注入 → 按字面量绑定，查不到但无害。
    expect(count({ filter: { modelId: "x' OR '1'='1" } })).toBe(0);
    expect(count({ filter: { sessionId: "s1' UNION SELECT 1 --" } })).toBe(0);
    // unknown filter field → 400 语义。
    expect(normalizeModelObservabilityQuery({ filter: { hack: 1 } }).ok).toBe(false);
    // DB 结构完好。
    const health = service.getHealth();
    if (health.ok !== true) throw new Error("health failed");
    expect(health.value.callCount).toBe(7);
  });

  /* ── Group By（§一百零三/一百零四）────────────────────────────────── */

  it("accounting coverage：10 calls（7 usage ok + 2 usage_missing + 1 无关联）统计真实（§一百零四）", () => {
    for (let i = 0; i < 10; i++) {
      seedCall({ id: `mc_cov${i}`, trace: "mt_cov", at: T(12, i) });
    }
    harness.flush();
    const entries: unknown[] = [];
    for (let i = 0; i < 7; i++) {
      entries.push(usageFixture(`mc_cov${i}`));
    }
    for (let i = 7; i < 9; i++) {
      entries.push(usageFixture(`mc_cov${i}`, { status: "usage_missing", usage: null }));
    }
    // 第 10 条无 modelCallId（MC-03 形状）→ 不投影。
    entries.push(usageFixture("mc_orphan", { metadata: null }));
    wireUsage(entries);
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const normalizedAggregate = normalizeModelObservabilityAggregateQuery({ filter: {}, groupBy: [] });
    if (normalizedAggregate.ok !== true) throw new Error(normalizedAggregate.error.message);
    const aggregate = service.queryAggregate(normalizedAggregate.value);
    if (aggregate.ok !== true) throw new Error("aggregate failed");
    expect(aggregate.value.overall.callCount).toBe(10);
    expect(aggregate.value.overall.usageCoveredCalls).toBe(9);
    expect(aggregate.value.overall.usageMissingCalls).toBe(2);
    expect(aggregate.value.overall.inputTokens).toBe(7 * 100);
    expect(aggregate.value.overall.totalTokens).toBe(7 * 150);
    expect(aggregate.value.overall.costTotal).toBeCloseTo(7 * 0.01, 9);
    expect(aggregate.value.overall.cacheHitCount).toBe(7);
  });

  it("group by：date / model / category / session / task / status + 多级（§一百零三）", () => {
    seedFixture();
    wireUsage([
      usageFixture("mc_a1", { usage: { costTotal: 0.02, input: { totalTokens: 200, uncachedTokens: 100 }, output: { totalTokens: 50 }, cache: { readTokens: 100, writeTokens: 0, hit: false, created: false, support: "reported" }, totalTokens: 350 } }),
      usageFixture("mc_v1", { metadata: { modelCallId: "mc_v1" } }),
    ]);
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const agg = (body: Record<string, unknown>) => {
      const normalized = normalizeModelObservabilityAggregateQuery(body);
      if (normalized.ok === false) throw new Error(normalized.error.message);
      const result = service.queryAggregate(normalized.value);
      if (result.ok !== true) throw new Error(result.error.message);
      return result.value;
    };

    const byDate = agg({ filter: {}, groupBy: ["date"], dateBucket: { bucket: "day", utcOffsetMinutes: 0 } });
    expect(byDate.groups.map((g) => g.values.date).sort()).toEqual(["2026-08-01", "2026-08-02", "2026-08-03"]);
    const day1 = byDate.groups.find((g) => g.values.date === "2026-08-01");
    expect(day1?.metrics.callCount).toBe(4);
    expect(day1?.metrics.traceCount).toBe(2);

    const byModel = agg({ filter: {}, groupBy: ["model"] });
    const gpt = byModel.groups.find((g) => g.values.provider === "openai");
    expect(gpt?.values.modelId).toBe("gpt-4o");
    expect(gpt?.metrics.callCount).toBe(4);
    expect(gpt?.metrics.inputTokens).toBe(200); // 只有 mc_a1 有 usage
    expect(gpt?.metrics.costTotal).toBeCloseTo(0.02, 9);

    const byCategory = agg({ filter: {}, groupBy: ["category"] });
    expect(byCategory.groups.map((g) => g.values.category).sort())
      .toEqual(["llm", "media", "memory", "speech"].sort());

    const bySession = agg({ filter: {}, groupBy: ["session"] });
    expect(bySession.groups.find((g) => g.values.session === "s1")?.metrics.callCount).toBe(4);

    const byTask = agg({ filter: {}, groupBy: ["task"] });
    expect(byTask.groups.find((g) => g.values.task === "t1")?.metrics.callCount).toBe(4);

    const byStatus = agg({ filter: {}, groupBy: ["status"] });
    const okBucket = byStatus.groups.find((g) => g.values.status === "ok");
    expect(okBucket?.metrics.callCount).toBe(3);
    const incompleteBucket = byStatus.groups.find((g) => g.values.status === null);
    expect(incompleteBucket?.metrics.callCount).toBe(1);

    // 多级：category + model（§四十）。
    const twoLevel = agg({ filter: {}, groupBy: ["category", "model"] });
    // llm/openai + memory/anthropic + media/volcengine + speech/anthropic = 4 组合。
    expect(twoLevel.groups.length).toBe(4);
    const llmOpenai = twoLevel.groups.find((g) => g.values.category === "llm" && g.values.provider === "openai");
    expect(llmOpenai?.metrics.callCount).toBe(4);
    // date + model。
    const dateModel = agg({ filter: {}, groupBy: ["date", "model"], dateBucket: { bucket: "day", utcOffsetMinutes: 0 } });
    expect(dateModel.groups.some((g) => g.values.date === "2026-08-02" && g.values.provider === "volcengine")).toBe(true);
  });

  it("date bucket 时区：同一 query 在不同 utcOffsetMinutes 得到确定分组（§四十三）", () => {
    // 23:30 UTC（8 月 1 日）→ UTC+480 偏移下落在 8 月 2 日。
    seedCall({ id: "mc_tz", trace: "mt_tz", at: Date.UTC(2026, 7, 1, 23, 30) });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const agg = (offset: number) => {
      const normalized = normalizeModelObservabilityAggregateQuery({
        filter: {}, groupBy: ["date"], dateBucket: { bucket: "day", utcOffsetMinutes: offset },
      });
      if (normalized.ok === false) throw new Error(normalized.error.message);
      const result = service.queryAggregate(normalized.value);
      if (result.ok !== true) throw new Error(result.error.message);
      return result.value.groups[0]?.values.date;
    };
    expect(agg(0)).toBe("2026-08-01");
    expect(agg(480)).toBe("2026-08-02");
    expect(agg(-330)).toBe("2026-08-01");
  });

  /* ── Trace list / detail（§一百零五）──────────────────────────────── */

  it("trace list：filter 语义 = trace 内至少一条 call 命中；keyset 分页（§二十八）", () => {
    seedFixture();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const traces = tracesQuery({ filter: { provider: "openai" }, limit: 50 });
    if (traces.ok !== true) throw new Error("trace query failed");
    expect(traces.value.traces.map((t) => t.traceId).sort()).toEqual(["mt_a", "mt_b"].sort());
    const mtA = traces.value.traces.find((t) => t.traceId === "mt_a");
    expect(mtA).toMatchObject({ callCount: 3, terminalOk: 2, terminalAborted: 1 });
    // trace filter by session：s2 只命中 mt_m/mt_v/mt_i。
    const s2Traces = tracesQuery({ filter: { sessionId: "s2" }, limit: 50 });
    if (s2Traces.ok !== true) throw new Error("trace query failed");
    expect(s2Traces.value.traces.map((t) => t.traceId).sort()).toEqual(["mt_i", "mt_m", "mt_v"].sort());
  });

  it("trace detail：roots/edges/orphan 诚实 + cycle → degraded 不 crash（§三十/三十一）", () => {
    seedFixture();
    // orphan parent：C4 的 parent 指向不存在的 call。
    seedCall({ id: "mc_o1", trace: "mt_o", parent: "mc_missing", at: T(4, 1) });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });

    const tree = service.queryTraceDetail("mt_a");
    if (tree.ok !== true) throw new Error("trace detail failed");
    expect(tree.value.roots).toEqual([{ callId: "mc_a1", orphanParent: false }]);
    expect(tree.value.edges.sort((a, b) => a.childCallId.localeCompare(b.childCallId))).toEqual([
      { parentCallId: "mc_a1", childCallId: "mc_a2" },
      { parentCallId: "mc_a1", childCallId: "mc_a3" },
    ]);
    expect(tree.value.orphanEdges).toEqual([]);
    expect(tree.value.graphIntegrity).toBe("ok");

    const orphanTrace = service.queryTraceDetail("mt_o");
    if (orphanTrace.ok !== true) throw new Error("orphan trace failed");
    expect(orphanTrace.value.roots).toEqual([{ callId: "mc_o1", orphanParent: true }]);
    expect(orphanTrace.value.orphanEdges).toEqual([{ childCallId: "mc_o1", missingParentCallId: "mc_missing" }]);
    expect(orphanTrace.value.graphIntegrity).toBe("ok");

    // cycle 损坏 fixture：直接 UPDATE parent 造环（模拟损坏/手工修改）。
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.prepare(`UPDATE model_calls SET parent_call_id = 'mc_a3' WHERE call_id = 'mc_a1'`).run();
    db.prepare(`UPDATE model_calls SET parent_call_id = 'mc_a1' WHERE call_id = 'mc_a2'`).run();
    db.prepare(`UPDATE model_calls SET parent_call_id = 'mc_a2' WHERE call_id = 'mc_a3'`).run();
    db.close();
    service.invalidate();
    const cyclic = service.queryTraceDetail("mt_a");
    if (cyclic.ok !== true) throw new Error("cyclic trace failed");
    expect(cyclic.value.graphIntegrity).toBe("degraded");
    expect(cyclic.value.calls).toHaveLength(3);
  });

  /* ── Call detail（§一百零六 MC-06 codex 401）──────────────────────── */

  it("call detail：MC-06 形状 = 1 call + 2 attempts + 2 provider_request ordinals + 2 provider_response（§三十三/一百零六）", () => {
    clockMs = T(5, 1);
    const recorder = createModelCallRecorder({
      observer: harness.handle.observer,
      context: {
        callId: "mc_codex401",
        traceId: "mt_codex",
        parentCallId: null,
        model: { provider: "openai", modelId: "gpt-5-codex", api: "responses" },
        source: { subsystem: "media", operation: "image", surface: "server", trigger: "tool" },
        attribution: { kind: "session", sessionId: "s1" },
      },
      now: () => clockMs,
    });
    recorder.beginLogicalCall({ details: { traceOrigin: "tool" } });
    // attempt 1 → 401。
    recorder.beginAttempt({});
    const attempt1 = recorder.currentAttemptId;
    recorder.providerRequestPrepared({});
    recorder.providerResponseReceived({ httpStatus: 401, providerRequestId: "req_1" });
    recorder.attemptError(Object.assign(new Error("401"), { code: "401" }));
    // attempt 2 → 401。
    recorder.beginAttempt({});
    const attempt2 = recorder.currentAttemptId;
    recorder.providerRequestPrepared({});
    recorder.providerResponseReceived({ httpStatus: 401, providerRequestId: "req_2" });
    recorder.attemptError(Object.assign(new Error("401"), { code: "401" }));
    recorder.logicalCallError(Object.assign(new Error("codex image failed"), { code: "401" }));
    recorder.endLogicalCall("error");

    const emitPayload = (kind: string, ordinal: number | null, payload: unknown, visibility = "full") => {
      harness.handle.sink?.handleModelCallPayloadRecord({
        schemaVersion: 1,
        kind,
        capturedAt: new Date(clockMs).toISOString(),
        callId: "mc_codex401",
        traceId: "mt_codex",
        parentCallId: null,
        attemptId: ordinal === 1 ? attempt1 : attempt2,
        providerRequestOrdinal: ordinal,
        model: { provider: "openai", modelId: "gpt-5-codex", api: "responses" },
        source: { subsystem: "media", operation: "image" },
        attribution: { kind: "session", sessionId: "s1" },
        visibility,
        fidelity: "runtime_exact",
        sanitization: { redacted: false, truncated: false, degraded: false },
        payload,
        semanticInputProvenance: null,
        providerRequestProvenance: null,
      } as never);
    };
    emitPayload("semantic_request", null, { inputShape: "media_prompt", prompt: "a cat" });
    emitPayload("provider_request", 1, { transport: { method: "POST", url: "https://api/x" } });
    emitPayload("provider_request", 2, { transport: { method: "POST", url: "https://api/x" } });
    emitPayload("provider_response", 1, { status: 401, body: { error: "unauthorized" } });
    emitPayload("provider_response", 2, { status: 401, body: { error: "unauthorized" } });
    emitPayload("semantic_response", null, { status: "error", errorName: "ProviderError" });
    harness.flush();

    service = createModelObservabilityQueryService({ lingxiHome: home });
    const detail = service.queryCallDetail("mc_codex401");
    if (detail.ok !== true) throw new Error("call detail failed");
    expect(detail.value.call.attemptCount).toBe(2);
    expect(detail.value.call.providerRequestCount).toBe(2);
    expect(detail.value.attempts).toHaveLength(2);
    expect(detail.value.payloadRecords).toHaveLength(6);
    const ordinals = detail.value.payloadRecords
      .filter((p) => p.kind === "provider_request")
      .map((p) => p.providerRequestOrdinal)
      .sort();
    expect(ordinals).toEqual([1, 2]);
    expect(detail.value.payloadRecords.filter((p) => p.kind === "provider_response")).toHaveLength(2);
    expect(detail.value.payloadRecords.filter((p) => p.kind === "semantic_request")).toHaveLength(1);
    expect(detail.value.payloadRecords.filter((p) => p.kind === "semantic_response")).toHaveLength(1);
    // payload 正文不默认 inline（§三十四）。
    expect((detail.value.payloadRecords[0] as Record<string, unknown>).payload).toBeUndefined();
  });

  /* ── Payload truth / exact retrieval（§一百零七/一百零八）─────────── */

  it("opaque / unavailable：contentAvailable=false，不冒充空对象（§八十七/一百零七/一百零八）", () => {
    seedCall({ id: "mc_op", trace: "mt_op", at: T(6, 1) });
    clockMs = T(6, 2);
    const emit = (kind: string, visibility: string, payload: unknown) => {
      harness.handle.sink?.handleModelCallPayloadRecord({
        schemaVersion: 1, kind, capturedAt: new Date(clockMs).toISOString(), callId: "mc_op",
        traceId: "mt_op", parentCallId: null, attemptId: null, providerRequestOrdinal: null,
        model: null, source: null, attribution: null, visibility, fidelity: "opaque",
        sanitization: { redacted: false, truncated: false, degraded: false },
        payload, semanticInputProvenance: null, providerRequestProvenance: null,
      } as never);
    };
    emit("provider_request", "opaque", null); // MC-07 CLI 形状
    emit("provider_response", "unavailable", null); // MC-10 形状
    emit("semantic_request", "full", { inputShape: "chat_context" });
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const list = query({ filter: { callId: "mc_op" } });
    if (list.ok !== true) throw new Error("query failed");
    expect(list.value.calls[0].payloadAvailability).toBe("present");
    expect(list.value.calls[0].payloadRecordCount).toBe(3);
    const records = list.value.calls[0];
    void records;
    const detail = service.queryCallDetail("mc_op");
    if (detail.ok !== true) throw new Error("detail failed");
    const opaqueId = detail.value.payloadRecords.find((p) => p.visibility === "opaque")?.id;
    const unavailableId = detail.value.payloadRecords.find((p) => p.visibility === "unavailable")?.id;
    if (opaqueId == null || unavailableId == null) throw new Error("records missing");
    const opaque = service.getPayloadRecord(opaqueId);
    if (opaque.ok !== true) throw new Error("opaque retrieval failed");
    expect(opaque.value.contentAvailable).toBe(false);
    expect(opaque.value.contentState).toBe("opaque_or_unavailable");
    expect(opaque.value.payload).toBeNull();
    const unavailable = service.getPayloadRecord(unavailableId);
    if (unavailable.ok !== true) throw new Error("unavailable retrieval failed");
    expect(unavailable.value.contentState).toBe("opaque_or_unavailable");
  });

  it("payload JSON 损坏：contentState=corrupt，不 crash、不返回 raw string（§三十六）", () => {
    seedCall({ id: "mc_corrupt", trace: "mt_corrupt", at: T(7, 1) });
    clockMs = T(7, 2);
    harness.handle.sink?.handleModelCallPayloadRecord({
      schemaVersion: 1, kind: "provider_response", capturedAt: new Date(clockMs).toISOString(),
      callId: "mc_corrupt", traceId: "mt_corrupt", parentCallId: null, attemptId: null,
      providerRequestOrdinal: null, model: null, source: null, attribution: null,
      visibility: "full", fidelity: "runtime_exact",
      sanitization: { redacted: false, truncated: false, degraded: false },
      payload: { status: 200 }, semanticInputProvenance: null, providerRequestProvenance: null,
    } as never);
    harness.flush();
    // 手工损坏（模拟磁盘损坏/未来版本写坏）。
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.prepare(`UPDATE payload_records SET payload_json = '{"broken": ' WHERE call_id = ?`).run("mc_corrupt");
    db.close();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const detail = service.queryCallDetail("mc_corrupt");
    if (detail.ok !== true) throw new Error("detail failed");
    const recordId = detail.value.payloadRecords[0].id;
    const retrieved = service.getPayloadRecord(recordId);
    if (retrieved.ok !== true) throw new Error("retrieval failed");
    expect(retrieved.value.contentState).toBe("corrupt");
    expect(retrieved.value.contentAvailable).toBe(false);
    expect(retrieved.value.payload).toBeNull();
  });

  it("payload availability 真相：present / unknown / dropped（§三十七）；hasPayload filter", () => {
    seedFixture();
    wireUsage([]);
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const all = query({ filter: {} });
    if (all.ok !== true) throw new Error("query failed");
    const byId = new Map(all.value.calls.map((c) => [c.callId, c.payloadAvailability]));
    expect(byId.get("mc_a1")).toBe("unknown"); // 无 payload row、列 NULL
    expect(count({ filter: { hasPayload: false } })).toBe(7);
    // 显式 dropped 标记。
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.prepare(`UPDATE model_calls SET payload_availability = 'dropped' WHERE call_id = ?`).run("mc_a1");
    db.close();
    service.invalidate();
    const dropped = query({ filter: { callId: "mc_a1" } });
    if (dropped.ok !== true) throw new Error("query failed");
    expect(dropped.value.calls[0].payloadAvailability).toBe("dropped");
    expect(idList({ filter: { payloadAvailability: ["dropped"] } })).toEqual(["mc_a1"]);
  });

  it("usage availability：present / not_correlated（§二十三/二十四）", () => {
    seedFixture();
    wireUsage([
      usageFixture("mc_a1"),
      usageFixture("mc_m1", { status: "usage_missing", usage: null }),
    ]);
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const all = query({ filter: {} });
    if (all.ok !== true) throw new Error("query failed");
    const byId = new Map(all.value.calls.map((c) => [c.callId, c.usage]));
    expect(byId.get("mc_a1")).toMatchObject({ availability: "present", status: "ok" });
    expect(byId.get("mc_a1")?.summary?.inputTokens).toBe(100);
    // logical ok + usage_missing 完全合法（§二十四 两字段分开）。
    expect(byId.get("mc_m1")).toMatchObject({ availability: "present", status: "usage_missing" });
    expect(byId.get("mc_v1")).toMatchObject({ availability: "not_correlated" });
  });

  /* ── EXPLAIN QUERY PLAN + 宽松性能 guard（§四十七/一百二十一）──────── */

  it("EXPLAIN QUERY PLAN：核心 fixture 走 index，非全表 scan（§四十七）", () => {
    for (let i = 0; i < 600; i++) {
      seedCall({
        id: `mc_perf${i}`,
        trace: `mt_perf${i % 40}`,
        at: T(20, Math.floor(i / 60), i % 60),
        provider: i % 2 === 0 ? "openai" : "anthropic",
        modelId: i % 2 === 0 ? "gpt-4o" : "claude-x",
        subsystem: ["llm", "memory", "media"][i % 3],
        session: `s${i % 5}`,
        agent: `a${i % 3}`,
        task: `t${i % 4}`,
      });
    }
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    // 通过 aggregate 的同型 SQL 检查 plan：直接对 read 连接跑 EXPLAIN。
    const plan = (sql: string) => {
      const health = service.getHealth();
      void health;
      const readDb = openModelObservabilityDatabase(modelObservabilityDbPath(home));
      try {
        return readDb.prepare(`EXPLAIN QUERY PLAN ${sql}`).all().map((r: any) => String(r.detail)).join(" | ");
      } finally {
        readDb.close();
      }
    };
    expect(plan(`SELECT * FROM model_calls WHERE started_at >= '2026-08-01' AND started_at < '2026-09-01'`))
      .toContain("idx_model_calls_started");
    expect(plan(`SELECT * FROM model_calls WHERE trace_id = 'mt_perf1'`)).toContain("idx_model_calls_trace");
    expect(plan(`SELECT * FROM model_calls WHERE provider = 'openai' AND model_id = 'gpt-4o'`))
      .toContain("idx_model_calls_model");
    expect(plan(`SELECT * FROM model_calls WHERE subsystem = 'llm' AND operation = 'callText'`))
      .toContain("idx_model_calls_subsystem");
    expect(plan(`SELECT * FROM model_calls WHERE session_id = 's1'`)).toContain("idx_model_calls_session");
    expect(plan(`SELECT * FROM model_calls WHERE agent_id = 'a1'`)).toContain("idx_model_calls_agent");
    expect(plan(`SELECT * FROM model_calls WHERE task_id = 't1'`)).toContain("idx_model_calls_task");
    expect(plan(`SELECT * FROM model_calls WHERE terminal_status = 'ok'`)).toContain("idx_model_calls_terminal");
    expect(plan(`SELECT * FROM model_calls WHERE conversation_id = 'c1'`)).toContain("idx_model_calls_conversation");
  });

  it("10k calls 性能：query page / filter / aggregate 在宽松上限内完成（§一百二十一）", () => {
    for (let i = 0; i < 10_000; i++) {
      seedCall({
        id: `mc_bulk${i}`,
        trace: `mt_bulk${i % 500}`,
        at: T(1, 0, 0) + i * 60_000,
        provider: i % 2 === 0 ? "openai" : "anthropic",
        subsystem: ["llm", "memory", "media"][i % 3],
        session: `s${i % 20}`,
        task: `t${i % 10}`,
      });
      // 队列 cap 4096：分批 flush（生产语义：setImmediate/interval 定期 drain）。
      if (i % 500 === 499) harness.flush();
    }
    harness.flush();
    service = createModelObservabilityQueryService({ lingxiHome: home });
    const started = Date.now();
    expect(count({ filter: { provider: "openai", subsystem: "llm" }, })).toBe(50);
    const normalizedAggregate = normalizeModelObservabilityAggregateQuery({ filter: {}, groupBy: ["provider"] });
    if (normalizedAggregate.ok !== true) throw new Error(normalizedAggregate.error.message);
    const aggregate = service.queryAggregate(normalizedAggregate.value);
    expect(aggregate.ok === true && aggregate.value.overall.callCount).toBe(10_000);
    const elapsed = Date.now() - started;
    // 宽松 guard（防 flaky）：不做严格 wall-clock 断言以外的任何假设。
    expect(elapsed).toBeLessThan(10_000);
  });
});
