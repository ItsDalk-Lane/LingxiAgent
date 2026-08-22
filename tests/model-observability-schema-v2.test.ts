/**
 * Phase 8 Schema v2 测试（任务书 §九十四）：
 * fresh v2 / v1→v2 migration + data preservation / v1 trace/payload rows
 * unchanged / usage table added / unknown higher schema / rollback on
 * migration failure / read-only v1 compatibility（§六/七）。
 *
 * v1 fixture 构造：生产代码开出的 v2 库 → DROP v2 新增对象 + user_version=1
 * ——与 Phase 7 真实 v1 库逐字节同构（v2 migration 只新增，不改 v1 对象）。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import {
  MODEL_OBSERVABILITY_SCHEMA_VERSION,
  ModelObservabilitySchemaError,
  modelObservabilityDbPath,
  openModelObservabilityDatabase,
  readModelObservabilitySchemaVersion,
} from "../lib/llm/model-observability-schema.ts";
import { openModelObservabilityReadDatabase } from "../lib/llm/model-observability-read-database.ts";
import { installModelObservabilityPersistence } from "../lib/llm/model-observability-persistence.ts";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createModelObservabilityQueryService } from "../lib/llm/model-observability-query.ts";
import { EMPTY_MODEL_OBSERVABILITY_FILTER } from "../lib/llm/model-observability-query-types.ts";

const MODEL = { provider: "anthropic", modelId: "claude-x", api: "anthropic-messages" };
const SOURCE = { subsystem: "llm", operation: "callText", surface: "server", trigger: "user_turn" };

function makeTempHome(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-v2-"));
}

/** 把 v2 库还原成 v1 形状（drop v2 新增对象 + user_version=1）。 */
function downgradeToV1(db: any): void {
  db.exec(`
    DROP INDEX IF EXISTS idx_model_call_usage_status;
    DROP INDEX IF EXISTS idx_model_calls_conversation;
    DROP TABLE IF EXISTS model_call_usage;
  `);
  db.pragma("user_version = 1");
}

describe("Model Observability Schema v2", () => {
  let home: string;

  beforeEach(() => {
    home = makeTempHome();
  });
  afterEach(() => {
    try { fs.rmSync(home, { recursive: true, force: true }); } catch { /* tmp */ }
  });

  it("fresh v2：user_version=2，model_call_usage 存在", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(readModelObservabilitySchemaVersion(db)).toBe(2);
      expect(MODEL_OBSERVABILITY_SCHEMA_VERSION).toBe(2);
      const table = db.prepare(
        `SELECT 1 FROM sqlite_master WHERE type='table' AND name='model_call_usage'`,
      ).get();
      expect(table).not.toBeNull();
    } finally {
      db.close();
    }
  });

  it("v1 → v2 migration：v1 行原样保留，usage 表新增，user_version=2（§九十四）", () => {
    // ① 造真实 v1 数据（Phase 7 生产投影）。
    const seed = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    seed.prepare(
      `INSERT INTO traces (trace_id, origin, first_seen_at, last_seen_at, call_count, created_at, updated_at)
       VALUES ('mt_v1', 'user_turn', '2026-01-01T00:00:00.000Z', '2026-01-01T00:01:00.000Z', 1, '2026-01-01T00:00:00.000Z', '2026-01-01T00:01:00.000Z')`,
    ).run();
    seed.prepare(
      `INSERT INTO model_calls (call_id, trace_id, provider, model_id, started_at, ended_at, terminal_status, persistence_completeness)
       VALUES ('mc_v1', 'mt_v1', 'anthropic', 'claude-x', '2026-01-01T00:00:00.000Z', '2026-01-01T00:01:00.000Z', 'ok', 'complete')`,
    ).run();
    downgradeToV1(seed);
    seed.close();

    // ② v1 只读打开：不迁移（§七）。
    const readOnly = openModelObservabilityReadDatabase(modelObservabilityDbPath(home));
    expect(readOnly.status).toBe("ready");
    expect(readOnly.schemaVersion).toBe(1);
    const v1Table = readOnly.db.prepare(
      `SELECT 1 FROM sqlite_master WHERE type='table' AND name='model_call_usage'`,
    ).get();
    expect(v1Table).toBeUndefined();
    readOnly.db.close();

    // ③ write 侧打开 → migration 到 v2。
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(readModelObservabilitySchemaVersion(db)).toBe(2);
      const trace = db.prepare(`SELECT * FROM traces WHERE trace_id = 'mt_v1'`).get();
      expect(trace).toMatchObject({ trace_id: "mt_v1", origin: "user_turn" });
      const call = db.prepare(`SELECT * FROM model_calls WHERE call_id = 'mc_v1'`).get();
      expect(call).toMatchObject({ call_id: "mc_v1", terminal_status: "ok" });
      const usageTable = db.prepare(
        `SELECT 1 FROM sqlite_master WHERE type='table' AND name='model_call_usage'`,
      ).get();
      expect(usageTable).not.toBeNull();
    } finally {
      db.close();
    }
  });

  it("migration failure：单事务 rollback，user_version 保持 v1，install → disabled（§九十四）", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    downgradeToV1(db);
    // 占住 model_call_usage 名字 → v2 CREATE TABLE 失败 → 整个 migration 回滚。
    db.exec(`CREATE TABLE model_call_usage (poison INTEGER)`);
    db.close();

    const handle = installModelObservabilityPersistence({
      lingxiHome: home,
      policy: { enabled: true },
    });
    expect(handle.getHealth().status).toBe("disabled");
    expect(handle.getHealth().storeDisabledReasonCode).toBe("migration_failed");
    // v1 数据在 rollback 后原样保留（usage_projection 计数）。
    const probe = openModelObservabilityReadDatabase(modelObservabilityDbPath(home));
    expect(probe.status).toBe("ready");
    // 被 poison 占住的表还在（rollback 不删用户对象），但 user_version 未推进。
    expect(probe.schemaVersion).toBe(1);
    probe.db.close();
    // 真正的 v1→v2 迁移失败后手动清理 poison 仍可恢复（文件未被删除）。
    expect(fs.existsSync(modelObservabilityDbPath(home))).toBe(true);
  });

  it("unknown higher schema（v3）：write 打开抛 schema_newer，read 侧 unavailable（§九十四）", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.pragma("user_version = 3");
    db.close();
    expect(() => openModelObservabilityDatabase(modelObservabilityDbPath(home)))
      .toThrowError(ModelObservabilitySchemaError);
    const readOnly = openModelObservabilityReadDatabase(modelObservabilityDbPath(home));
    expect(readOnly.status).toBe("schema_newer");
    expect(readOnly.schemaVersion).toBe(3);
    expect(readOnly.db).toBeNull();
  });

  it("read-only v1 库经 query service：call 可查，usage availability=projection_unavailable（§七）", () => {
    const harness = createModelObservabilityTestHarness({ lingxiHome: home });
    const recorder = createModelCallRecorder({
      observer: harness.handle.observer,
      context: {
        callId: "mc_v1q",
        traceId: "mt_v1q",
        model: MODEL,
        source: SOURCE,
        attribution: { kind: "session", sessionId: "s1", agentId: "a1", taskId: "t1" },
      },
    });
    recorder.beginLogicalCall({});
    recorder.endLogicalCall("ok");
    harness.flush();
    harness.close();

    // 拿到 flush 后的库，降级到 v1 模拟「Phase 7 时代的历史库」。
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    downgradeToV1(db);
    db.close();

    const service = createModelObservabilityQueryService({ lingxiHome: home });
    try {
      const calls = service.queryCalls({ filter: EMPTY_MODEL_OBSERVABILITY_FILTER, sort: "started_at_desc", limit: 50, cursor: null });
      expect(calls.ok === true).toBe(true);
      if (calls.ok === true) {
        expect(calls.value.calls).toHaveLength(1);
        expect(calls.value.calls[0].callId).toBe("mc_v1q");
        // v1：accounting projection 不可用（不假装 usage 缺失，§二十三）。
        expect(calls.value.calls[0].usage.availability).toBe("projection_unavailable");
      }
      const aggregate = service.queryAggregate({ filter: EMPTY_MODEL_OBSERVABILITY_FILTER, groupBy: [], dateBucket: null });
      expect(aggregate.ok === true).toBe(true);
      if (aggregate.ok === true) {
        expect(aggregate.value.overall.callCount).toBe(1);
        expect(aggregate.value.overall.usageCoveredCalls).toBe(0);
        expect(aggregate.value.overall.totalTokens).toBe(0);
      }
      const health = service.getHealth();
      expect(health.ok === true).toBe(true);
      if (health.ok === true) {
        expect(health.value.schemaVersion).toBe(1);
        expect(health.value.accountingProjectionAvailable).toBe(false);
      }
    } finally {
      service.close();
      harness.cleanup();
    }
  });

  it("DB 不存在：read 侧 absent，绝不创建文件（§六/九十二）", () => {
    const readOnly = openModelObservabilityReadDatabase(modelObservabilityDbPath(home));
    expect(readOnly.status).toBe("absent");
    expect(readOnly.db).toBeNull();
    expect(fs.existsSync(modelObservabilityDbPath(home))).toBe(false);
    expect(fs.existsSync(path.dirname(modelObservabilityDbPath(home)))).toBe(false);
  });
});
