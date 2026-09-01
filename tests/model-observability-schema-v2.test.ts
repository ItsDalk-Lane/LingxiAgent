/**
 * Model Observatory Schema v4 测试：
 * fresh v4 / v1→v4 / v2→v4 single-transaction migration + data
 * preservation / explicit usage correlation column / unknown higher schema /
 * rollback on migration failure / read-only v1+v2 compatibility。
 *
 * 历史 fixture 由生产库反向移除后续版本新增对象构造；只读打开不得迁移。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import {
  MODEL_OBSERVABILITY_SCHEMA_VERSION,
  ModelObservabilitySchemaError,
  migrateModelObservabilitySchema,
  modelObservabilityDbPath,
  openModelObservabilityDatabase,
  readModelObservabilitySchemaVersion,
} from "../lib/llm/model-observability-schema.ts";
import { openModelObservabilityReadDatabase } from "../lib/llm/model-observability-read-database.ts";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import { createModelCallRecorder } from "../lib/llm/model-call-recorder.ts";
import { createModelObservabilityQueryService } from "../lib/llm/model-observability-query.ts";
import { EMPTY_MODEL_OBSERVABILITY_FILTER } from "../lib/llm/model-observability-query-types.ts";

const MODEL = { provider: "anthropic", modelId: "claude-x", api: "anthropic-messages" };
const SOURCE = { subsystem: "llm", operation: "callText", surface: "server", trigger: "user_turn" };

function makeTempHome(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-v4-"));
}

/** 把 v4 库还原成 v2 形状（移除 v3 列和 v4 来源快照表）。 */
function downgradeToV2(db: any): void {
  db.exec(`
    DROP INDEX IF EXISTS idx_source_identity_snapshots_updated;
    DROP TABLE IF EXISTS source_identity_snapshots;
  `);
  const columns = db.prepare(`PRAGMA table_info(model_calls)`).all()
    .map((row: any) => row.name);
  if (columns.includes("usage_correlation_state")) {
    db.exec(`ALTER TABLE model_calls DROP COLUMN usage_correlation_state`);
  }
  db.pragma("user_version = 2");
}

/** 把 v4 库还原成 v1 形状（drop v2-v4 新增对象）。 */
function downgradeToV1(db: any): void {
  downgradeToV2(db);
  db.exec(`
    DROP INDEX IF EXISTS idx_model_call_usage_status;
    DROP INDEX IF EXISTS idx_model_calls_conversation;
    DROP TABLE IF EXISTS model_call_usage;
  `);
  db.pragma("user_version = 1");
}

describe("Model Observability Schema v4", () => {
  let home: string;

  beforeEach(() => {
    home = makeTempHome();
  });
  afterEach(() => {
    try { fs.rmSync(home, { recursive: true, force: true }); } catch { /* tmp */ }
  });

  it("fresh v4：usage projection、explicit correlation 与来源快照表存在", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(readModelObservabilitySchemaVersion(db)).toBe(4);
      expect(MODEL_OBSERVABILITY_SCHEMA_VERSION).toBe(4);
      const table = db.prepare(
        `SELECT 1 FROM sqlite_master WHERE type='table' AND name='model_call_usage'`,
      ).get();
      expect(table).not.toBeNull();
      const columns = db.prepare(`PRAGMA table_info(model_calls)`).all()
        .map((row: any) => row.name);
      expect(columns).toContain("usage_correlation_state");
      expect(db.prepare(
        `SELECT 1 FROM sqlite_master WHERE type='table' AND name='source_identity_snapshots'`,
      ).get()).not.toBeNull();
      expect(() => db.prepare(
        `INSERT INTO model_calls (call_id, usage_correlation_state) VALUES ('mc_invalid', 'guessed')`,
      ).run()).toThrow();
    } finally {
      db.close();
    }
  });

  it("v1 → v4 migration：v1 行原样保留，后续表与列一次完成", () => {
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

    // ③ write 侧打开 → 同一个 migration transaction 推进到 v4。
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(readModelObservabilitySchemaVersion(db)).toBe(4);
      const trace = db.prepare(`SELECT * FROM traces WHERE trace_id = 'mt_v1'`).get();
      expect(trace).toMatchObject({ trace_id: "mt_v1", origin: "user_turn" });
      const call = db.prepare(`SELECT * FROM model_calls WHERE call_id = 'mc_v1'`).get();
      expect(call).toMatchObject({
        call_id: "mc_v1",
        terminal_status: "ok",
        usage_correlation_state: null,
      });
      const usageTable = db.prepare(
        `SELECT 1 FROM sqlite_master WHERE type='table' AND name='model_call_usage'`,
      ).get();
      expect(usageTable).not.toBeNull();
    } finally {
      db.close();
    }
  });

  it("v2 只读兼容：可读 accounting，不添加后续对象，不改 user_version", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.prepare(
      `INSERT INTO model_calls (call_id, trace_id, terminal_status, persistence_completeness)
       VALUES ('mc_v2', 'mt_v2', 'ok', 'complete')`,
    ).run();
    downgradeToV2(db);
    db.close();

    const readOnly = openModelObservabilityReadDatabase(modelObservabilityDbPath(home));
    expect(readOnly.status).toBe("ready");
    expect(readOnly.schemaVersion).toBe(2);
    expect(readOnly.db.prepare(`SELECT call_id FROM model_calls`).get()).toMatchObject({ call_id: "mc_v2" });
    const columns = readOnly.db.prepare(`PRAGMA table_info(model_calls)`).all()
      .map((row: any) => row.name);
    expect(columns).not.toContain("usage_correlation_state");
    expect(readOnly.db.pragma("user_version", { simple: true })).toBe(2);
    readOnly.db.close();

    const query = createModelObservabilityQueryService({ lingxiHome: home });
    try {
      const result = query.queryCalls({
        filter: EMPTY_MODEL_OBSERVABILITY_FILTER,
        sort: "started_at_desc",
        limit: 50,
        cursor: null,
      });
      expect(result.ok).toBe(true);
      if (result.ok) {
        expect(result.value.calls[0].usage.availability).toBe("unknown");
      }
    } finally {
      query.close();
    }
  });

  it("v2 → v4 migration：既有 call/usage 行保留，新列默认 NULL", () => {
    const seed = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    seed.prepare(
      `INSERT INTO model_calls (call_id, trace_id, terminal_status, persistence_completeness)
       VALUES ('mc_v2_preserved', 'mt_v2', 'ok', 'complete')`,
    ).run();
    seed.prepare(
      `INSERT INTO model_call_usage (model_call_id, usage_status, total_tokens, created_at, updated_at)
       VALUES ('mc_v2_preserved', 'ok', 17, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:01.000Z')`,
    ).run();
    downgradeToV2(seed);
    seed.close();

    const migrated = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(readModelObservabilitySchemaVersion(migrated)).toBe(4);
      expect(migrated.prepare(`SELECT * FROM model_calls WHERE call_id = 'mc_v2_preserved'`).get())
        .toMatchObject({ call_id: "mc_v2_preserved", usage_correlation_state: null });
      expect(migrated.prepare(`SELECT total_tokens FROM model_call_usage WHERE model_call_id = 'mc_v2_preserved'`).get())
        .toMatchObject({ total_tokens: 17 });
    } finally {
      migrated.close();
    }
  });

  it("v2 → v4 migration failure：单事务 rollback，user_version 和既有数据保持 v2", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.prepare(
      `INSERT INTO model_calls (call_id, trace_id, terminal_status, persistence_completeness)
       VALUES ('mc_rollback', 'mt_rollback', 'ok', 'complete')`,
    ).run();
    downgradeToV2(db);
    // 在 ALTER TABLE 已执行后注入失败，验证 transaction 真的撤销新列。
    const failingAdapter = {
      transaction: db.transaction.bind(db),
      pragma: db.pragma.bind(db),
      exec(sql: string) {
        db.exec(sql);
        if (sql.includes("usage_correlation_state")) throw new Error("injected-v3-migration-failure");
      },
    };
    let migrationError: unknown = null;
    try {
      migrateModelObservabilitySchema(failingAdapter, 2);
    } catch (error) {
      migrationError = error;
    }
    expect(migrationError).toBeInstanceOf(ModelObservabilitySchemaError);
    expect((migrationError as ModelObservabilitySchemaError).reasonCode).toBe("migration_failed");
    expect(db.pragma("user_version", { simple: true })).toBe(2);
    const columns = db.prepare(`PRAGMA table_info(model_calls)`).all()
      .map((row: any) => row.name);
    expect(columns).not.toContain("usage_correlation_state");
    expect(db.prepare(`SELECT call_id FROM model_calls WHERE call_id = 'mc_rollback'`).get())
      .toMatchObject({ call_id: "mc_rollback" });
    db.close();

    // 失败没有留下脏状态；后续用真实 adapter 可正常完成迁移。
    const recovered = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    expect(readModelObservabilitySchemaVersion(recovered)).toBe(4);
    recovered.close();
    expect(fs.existsSync(modelObservabilityDbPath(home))).toBe(true);
  });

  it("unknown higher schema（v5）：write 打开抛 schema_newer，read 侧 unavailable", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.pragma("user_version = 5");
    db.close();
    expect(() => openModelObservabilityDatabase(modelObservabilityDbPath(home)))
      .toThrowError(ModelObservabilitySchemaError);
    const readOnly = openModelObservabilityReadDatabase(modelObservabilityDbPath(home));
    expect(readOnly.status).toBe("schema_newer");
    expect(readOnly.schemaVersion).toBe(5);
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
        expect(aggregate.value.overall.usageAggregateAvailability).toBe("projection_unavailable");
        expect(aggregate.value.overall.totalTokens).toBeNull();
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
