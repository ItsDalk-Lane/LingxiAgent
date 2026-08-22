/**
 * Phase 8 Accounting Projection 测试（任务书 §九十五）：
 * modelCallId 关联投影 / 幂等 upsert / usage_missing 状态保留 / token-cache-cost
 * 精确性 / 无 modelCallId 不猜 / error.message 毒丸不入库 / bounded ledger
 * backfill 幂等（§十五）。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import {
  backfillModelCallUsageFromLedgerEntries,
  createModelObservabilityAccountingProjection,
  modelCallUsageRowFromLedgerEntry,
  MODEL_OBSERVABILITY_USAGE_BACKFILL_META_KEY,
} from "../lib/llm/model-observability-accounting-projection.ts";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { openModelObservabilityDatabase, modelObservabilityDbPath } from "../lib/llm/model-observability-schema.ts";

function ledgerEntry(overrides: Record<string, unknown> = {}) {
  return {
    schemaVersion: 1,
    requestId: "llm_req_1",
    startedAt: "2026-08-01T00:00:00.000Z",
    endedAt: "2026-08-01T00:00:02.500Z",
    durationMs: 2500,
    status: "ok",
    source: { subsystem: "llm", operation: "callText" },
    attribution: { kind: "session", sessionId: "s1", agentId: "a1" },
    metadata: { modelCallId: "mc_p1", traceId: "mt_p1", parentCallId: null },
    model: { provider: "anthropic", modelId: "claude-x", api: "anthropic-messages" },
    usage: {
      costTotal: 0.0125,
      input: { totalTokens: 1200, uncachedTokens: 700 },
      output: { totalTokens: 300, reasoningTokens: 80 },
      cache: {
        readTokens: 500,
        writeTokens: 64,
        missTokens: null,
        hit: true,
        created: true,
        hitRatio: 0.4167,
        support: "reported",
      },
      totalTokens: 1064,
    },
    rawUsageShape: "input,output,cacheRead",
    error: null,
    ...overrides,
  };
}

describe("Model Observability Accounting Projection", () => {
  let home: string;

  beforeEach(() => {
    home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-acct-"));
  });
  afterEach(() => {
    try { fs.rmSync(home, { recursive: true, force: true }); } catch { /* tmp */ }
  });

  function openProjection() {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    const projection = createModelObservabilityAccountingProjection({ db });
    return { db, projection };
  }

  it("带 modelCallId 的 entry → projection row；token/cache/cost 精确（§九十五）", () => {
    const { db, projection } = openProjection();
    try {
      expect(projection.upsertLedgerEntry(ledgerEntry())).toBe(true);
      const row = db.prepare(`SELECT * FROM model_call_usage WHERE model_call_id = ?`).get("mc_p1");
      expect(row).toMatchObject({
        model_call_id: "mc_p1",
        usage_request_id: "llm_req_1",
        usage_status: "ok",
        input_total_tokens: 1200,
        input_uncached_tokens: 700,
        output_total_tokens: 300,
        reasoning_tokens: 80,
        cache_read_tokens: 500,
        cache_write_tokens: 64,
        cache_hit: 1,
        cache_created: 1,
        total_tokens: 1064,
        raw_usage_shape: "input,output,cacheRead",
      });
      expect(Number(row.cache_hit_ratio)).toBeCloseTo(0.4167, 4);
      expect(Number(row.cost_total)).toBeCloseTo(0.0125, 8);
    } finally {
      db.close();
    }
  });

  it("同一 modelCallId 重复进入 → 一行（幂等 upsert，§十四）", () => {
    const { db, projection } = openProjection();
    try {
      projection.upsertLedgerEntry(ledgerEntry());
      projection.upsertLedgerEntry(ledgerEntry({ durationMs: 9999, status: "error" }));
      const rows = db.prepare(`SELECT * FROM model_call_usage`).all();
      expect(rows).toHaveLength(1);
      // 后进覆盖（latest wins），不产生 duplicate。
      expect(rows[0]).toMatchObject({ usage_status: "error", duration_ms: 9999 });
    } finally {
      db.close();
    }
  });

  it("usage_missing：usage=null 时 numeric 全 NULL、status 保留（§二十四/九十五）", () => {
    const { db, projection } = openProjection();
    try {
      projection.upsertLedgerEntry(ledgerEntry({ usage: null, status: "usage_missing" }));
      const row = db.prepare(`SELECT * FROM model_call_usage WHERE model_call_id = ?`).get("mc_p1");
      expect(row.usage_status).toBe("usage_missing");
      expect(row.input_total_tokens).toBeNull();
      expect(row.total_tokens).toBeNull();
      expect(row.cost_total).toBeNull();
    } finally {
      db.close();
    }
  });

  it("无 metadata.modelCallId → 不投影（不通过时间/modelId 猜，§十三）", () => {
    const { db, projection } = openProjection();
    try {
      expect(projection.upsertLedgerEntry(ledgerEntry({ metadata: null }))).toBe(false);
      expect(projection.upsertLedgerEntry(ledgerEntry({ metadata: { traceId: "mt_x" } }))).toBe(false);
      expect(db.prepare(`SELECT COUNT(*) AS n FROM model_call_usage`).get().n).toBe(0);
    } finally {
      db.close();
    }
  });

  it("error.message 毒丸绝不入库（§十一；byte 级扫描）", () => {
    const { db, projection } = openProjection();
    try {
      projection.upsertLedgerEntry(ledgerEntry({
        status: "error",
        error: { name: "ProviderError", message: "sk-live-poison SECRETBearer abc" },
      }));
      const row = db
        .prepare(`SELECT * FROM model_call_usage WHERE model_call_id = ?`)
        .get("mc_p1");
      expect(row).not.toBeNull();
      // 逐列断言：不存在任何 error 文本列。
      for (const [column, value] of Object.entries(row)) {
        expect(String(value)).not.toContain("sk-live-poison");
        expect(String(value)).not.toContain("SECRETBearer");
      }
      const columns = Object.keys(row);
      expect(columns.some((c) => /error/i.test(c))).toBe(false);
    } finally {
      db.close();
    }
  });

  it("bounded ledger backfill：幂等 + meta 标记 + 不声称完整历史（§十五）", () => {
    const { db, projection } = openProjection();
    try {
      const entries = [
        ledgerEntry(),
        ledgerEntry({
          requestId: "llm_req_2",
          metadata: { modelCallId: "mc_p2" },
          usage: null,
          status: "usage_missing",
        }),
        ledgerEntry({ requestId: "llm_req_3", metadata: null }),
      ];
      const first = backfillModelCallUsageFromLedgerEntries(projection, entries, db);
      expect(first.projected).toBe(2);
      expect(first.skipped).toBe(1);
      const marker = db
        .prepare(`SELECT value_json FROM observability_meta WHERE key = ?`)
        .get(MODEL_OBSERVABILITY_USAGE_BACKFILL_META_KEY);
      expect(marker).not.toBeNull();
      // 再跑一遍：仍是 2 行（幂等），marker 已存在。
      backfillModelCallUsageFromLedgerEntries(projection, entries, db);
      expect(db.prepare(`SELECT COUNT(*) AS n FROM model_call_usage`).get().n).toBe(2);
    } finally {
      db.close();
    }
  });

  it("row transform 纯函数：损坏 usage / 异常类型 fail-safe", () => {
    expect(modelCallUsageRowFromLedgerEntry(null)).toBeNull();
    expect(modelCallUsageRowFromLedgerEntry({ metadata: { modelCallId: "" } })).toBeNull();
    const row = modelCallUsageRowFromLedgerEntry(ledgerEntry({
      usage: { input: "garbage", cache: { hit: "yes" } },
      durationMs: Number.NaN,
    }));
    expect(row).not.toBeNull();
    expect(row.input_total_tokens).toBeNull();
    expect(row.cache_hit).toBeNull();
    expect(row.duration_ms).toBeNull();
  });

  it("live ingestion：handle.initializeAccounting 经 llm_usage 事件写 projection（§十四）", async () => {
    const harness = createModelObservabilityTestHarness({ lingxiHome: home });
    try {
      const ledgerEntries: unknown[] = [];
      let consumer: ((entry: unknown) => void) | null = null;
      const report = harness.handle.initializeAccounting({
        listLedgerEntries: () => ledgerEntries,
        subscribeUsage: (fn) => {
          consumer = fn;
          return () => { consumer = null; };
        },
      });
      expect(report).not.toBeNull();
      // 初始 ledger 空：无 backfill。
      expect(report.backfilled).toBe(0);
      // live entry 经 llm_usage 事件进入。
      ledgerEntries.push(ledgerEntry());
      consumer?.({ type: "llm_usage", entry: ledgerEntry() });
      harness.flush();
      const reader = harness.openReader();
      try {
        const row = reader.db.prepare(`SELECT * FROM model_call_usage`).all();
        expect(row).toHaveLength(1);
        expect(row[0].model_call_id).toBe("mc_p1");
      } finally {
        reader.close();
      }
      await harness.close();
    } finally {
      harness.cleanup();
    }
  });

  it("retention：trace 删除时 usage projection 随之删除（§十六）", async () => {
    const harness = createModelObservabilityTestHarness({ lingxiHome: home });
    try {
      let consumer: ((entry: unknown) => void) | null = null;
      harness.handle.initializeAccounting({
        listLedgerEntries: () => [ledgerEntry()],
        subscribeUsage: (fn) => {
          consumer = fn;
          return () => { consumer = null; };
        },
      });
      void consumer;
      harness.flush();
      // 显式触发 maintenance（traceMaxAgeMs=1ms → 全部过期删除）。
      harness.handle.runMaintenance();
      const reader = harness.openReader();
      try {
        expect(reader.db.prepare(`SELECT COUNT(*) AS n FROM model_call_usage`).get().n).toBe(0);
        expect(reader.db.prepare(`SELECT COUNT(*) AS n FROM traces`).get().n).toBe(0);
      } finally {
        reader.close();
      }
      await harness.close();
    } finally {
      harness.cleanup();
    }
  });
});
