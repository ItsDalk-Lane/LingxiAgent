/**
 * Phase 7 Store Schema 测试（任务书 §九十四）：
 * fresh DB schema v1 / indexes / PRAGMA user_version / migration 幂等 /
 * 未知高版本 → 保留数据库禁用（绝不 DROP/重建）/ 损坏 → database_corrupt /
 * Unix 私有目录与文件权限（§七十八）。
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
import { installModelObservabilityPersistence } from "../lib/llm/model-observability-persistence.ts";

const EXPECTED_TABLES = [
  "observability_meta",
  "traces",
  "model_calls",
  "model_attempts",
  "payload_records",
  "blob_objects",
  "payload_blob_refs",
  "model_call_usage", // Phase 8 v2
  "source_identity_snapshots", // schema v4
];

const EXPECTED_INDEXES = [
  "idx_traces_last_seen",
  "idx_model_calls_trace",
  "idx_model_calls_started",
  "idx_model_calls_model",
  "idx_model_calls_subsystem",
  "idx_model_calls_terminal",
  "idx_model_calls_attribution_kind",
  "idx_model_calls_session",
  "idx_model_calls_agent",
  "idx_model_calls_task",
  "idx_model_attempts_call",
  "idx_payload_records_call",
  "idx_payload_records_call_kind",
  "idx_payload_records_attempt",
  "idx_payload_records_call_ordinal",
  "idx_payload_blob_refs_blob",
  "idx_blob_objects_state",
  "idx_blob_objects_created",
  "idx_model_call_usage_status", // Phase 8 v2
  "idx_model_calls_conversation", // Phase 8 v2
  "idx_source_identity_snapshots_updated", // schema v4
];

function makeTempHome(): string {
  return fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-schema-"));
}

describe("Model Observability Store Schema", () => {
  let home: string;

  beforeEach(() => {
    home = makeTempHome();
  });
  afterEach(() => {
    try { fs.rmSync(home, { recursive: true, force: true }); } catch { /* tmp */ }
  });

  it("fresh DB：建全部表 + 索引，user_version=SCHEMA_VERSION（v5 会话轨迹合并）", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(readModelObservabilitySchemaVersion(db)).toBe(MODEL_OBSERVABILITY_SCHEMA_VERSION);
      expect(MODEL_OBSERVABILITY_SCHEMA_VERSION).toBe(6);
      expect(db.prepare(`SELECT name FROM sqlite_master WHERE type='table' AND name='source_identity_snapshots'`).get()).toBeTruthy();
      const callColumns = db.prepare(`PRAGMA table_info(model_calls)`).all()
        .map((row: any) => row.name);
      expect(callColumns).toContain("usage_correlation_state");
      const tables = db.prepare(
        `SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name`,
      ).all().map((row: any) => row.name);
      for (const table of EXPECTED_TABLES) expect(tables).toContain(table);
      expect(tables).toHaveLength(EXPECTED_TABLES.length);
      const indexes = db.prepare(
        `SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%' ORDER BY name`,
      ).all().map((row: any) => row.name);
      for (const index of EXPECTED_INDEXES) expect(indexes).toContain(index);
      // WAL pragma 生效（多进程短暂并发基线，audit Q3/Q4）。
      expect(db.pragma("journal_mode", { simple: true })).toBe("wal");
      expect(db.pragma("secure_delete", { simple: true })).toBe(1);
    } finally {
      db.close();
    }
  });

  it("重复打开幂等：schema 不重复创建，user_version 不变", () => {
    const first = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    first.prepare(
      `INSERT INTO traces (trace_id, first_seen_at, last_seen_at, call_count, created_at, updated_at)
       VALUES ('mt_persist', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')`,
    ).run();
    first.close();
    const second = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(readModelObservabilitySchemaVersion(second)).toBe(MODEL_OBSERVABILITY_SCHEMA_VERSION);
      const row = second.prepare(`SELECT trace_id FROM traces`).get();
      expect(row).toMatchObject({ trace_id: "mt_persist" });
    } finally {
      second.close();
    }
  });

  it("未知高版本数据库：schema_newer，文件保留不重建（§二十七）", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.pragma("user_version = 7");
    db.close();
    const dbPath = modelObservabilityDbPath(home);
    const sizeBefore = fs.statSync(dbPath).size;
    expect(() => openModelObservabilityDatabase(dbPath)).toThrowError(ModelObservabilitySchemaError);
    try {
      openModelObservabilityDatabase(dbPath);
    } catch (error: any) {
      expect(error.reasonCode).toBe("schema_newer");
    }
    // 数据库原样保留（绝不 DROP/重建）。
    expect(fs.statSync(dbPath).size).toBe(sizeBefore);
    expect(fs.existsSync(dbPath)).toBe(true);
  });

  it("未知高版本经 installModelObservabilityPersistence：disabled handle，主程序不受影响", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.pragma("user_version = 99");
    db.close();
    const handle = installModelObservabilityPersistence({
      lingxiHome: home,
      policy: { enabled: true, persistPayloads: true },
    });
    expect(handle.getHealth().status).toBe("disabled");
    expect(handle.getHealth().storeDisabledReasonCode).toBe("schema_newer");
    // close 是 no-op，不 throw。
    expect(() => handle.flushSync()).not.toThrow();
  });

  it("非 SQLite 文件：database_corrupt，不删除文件（§二十九）", () => {
    const dbPath = modelObservabilityDbPath(home);
    fs.mkdirSync(path.dirname(dbPath), { recursive: true });
    fs.writeFileSync(dbPath, "this is definitely not a sqlite database".repeat(10));
    let reason = "";
    try {
      openModelObservabilityDatabase(dbPath);
    } catch (error: any) {
      reason = error instanceof ModelObservabilitySchemaError ? error.reasonCode : String(error);
    }
    expect(reason).toBe("database_corrupt");
    expect(fs.existsSync(dbPath)).toBe(true);
  });

  it("policy 默认 disabled：不创建任何文件（§七十九生产行为不变）", () => {
    const handle = installModelObservabilityPersistence({ lingxiHome: home });
    expect(handle.getHealth().status).toBe("disabled");
    expect(handle.getHealth().storeDisabledReasonCode).toBe("disabled_by_policy");
    expect(fs.existsSync(path.join(home, "model-observability"))).toBe(false);
  });

  it("v4→v6 迁移：同会话轨迹合并为一行、非会话轨迹不动、幂等", () => {
    // 造一个 v4 旧库：表结构与 v6 相同（v5/v6 纯数据整合无 DDL），只降 user_version。
    const seed = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    seed.pragma("user_version = 4");
    const insertTrace = seed.prepare(
      `INSERT INTO traces (trace_id, origin, first_seen_at, last_seen_at, call_count, created_at, updated_at)
       VALUES (?, ?, ?, ?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')`,
    );
    const insertCall = seed.prepare(
      `INSERT INTO model_calls (call_id, trace_id, session_id, started_at, ended_at, terminal_status)
       VALUES (?, ?, ?, ?, ?, ?)`,
    );
    // 会话 s1：三轮各自成轨迹；其中一轮是 pi ingress 落成的 origin=unknown
    // （2026-09-05 实测的桌面历史形态），v6 同样合并。
    insertTrace.run("mt_a", "user_turn", "2026-01-01T10:00:00Z", "2026-01-01T10:01:00Z", 1);
    insertTrace.run("mt_b", "unknown", "2026-01-02T10:00:00Z", "2026-01-02T10:05:00Z", 1);
    insertTrace.run("mt_b2", "unknown", "2026-01-02T11:00:00Z", "2026-01-02T11:05:00Z", 1);
    // 会话 s2 单轨迹；无会话归属的 background 轨迹（记忆/嵌入类）：不受影响。
    insertTrace.run("mt_c", "user_turn", "2026-01-03T10:00:00Z", "2026-01-03T10:01:00Z", 1);
    insertTrace.run("mt_bg", "background", "2026-01-04T10:00:00Z", "2026-01-04T10:01:00Z", 1);
    // origin 为空的 singleton 辅助调用：不是对话轨迹，不合并。
    insertTrace.run("mt_aux", null, "2026-01-05T10:00:00Z", "2026-01-05T10:01:00Z", 1);
    insertCall.run("mc_a1", "mt_a", "s1", "2026-01-01T10:00:00Z", "2026-01-01T10:01:00Z", "ok");
    insertCall.run("mc_b1", "mt_b", "s1", "2026-01-02T10:00:00Z", "2026-01-02T10:05:00Z", "error");
    insertCall.run("mc_b2", "mt_b2", "s1", "2026-01-02T11:00:00Z", "2026-01-02T11:05:00Z", "ok");
    insertCall.run("mc_c1", "mt_c", "s2", "2026-01-03T10:00:00Z", "2026-01-03T10:01:00Z", "ok");
    insertCall.run("mc_bg1", "mt_bg", null, "2026-01-04T10:00:00Z", "2026-01-04T10:01:00Z", "ok");
    insertCall.run("mc_aux1", "mt_aux", null, "2026-01-05T10:00:00Z", "2026-01-05T10:01:00Z", "ok");
    seed.close();

    const migrated = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(readModelObservabilitySchemaVersion(migrated)).toBe(6);
      const traces = migrated.prepare(`SELECT * FROM traces ORDER BY trace_id`).all();
      expect(traces.map((row: any) => row.trace_id)).toEqual(["mt_a", "mt_aux", "mt_bg", "mt_c"]);
      const merged = traces.find((row: any) => row.trace_id === "mt_a");
      expect(merged).toMatchObject({ origin: "user_turn", call_count: 3, last_seen_at: "2026-01-02T11:05:00Z" });
      const callTraceIds = migrated.prepare(
        `SELECT call_id, trace_id FROM model_calls ORDER BY call_id`,
      ).all();
      expect(callTraceIds).toEqual([
        { call_id: "mc_a1", trace_id: "mt_a" },
        { call_id: "mc_aux1", trace_id: "mt_aux" },
        { call_id: "mc_b1", trace_id: "mt_a" },
        { call_id: "mc_b2", trace_id: "mt_a" },
        { call_id: "mc_bg1", trace_id: "mt_bg" },
        { call_id: "mc_c1", trace_id: "mt_c" },
      ]);
    } finally {
      migrated.close();
    }

    // 幂等：再次打开数据不变。
    const again = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(readModelObservabilitySchemaVersion(again)).toBe(6);
      expect(again.prepare(`SELECT COUNT(*) AS n FROM traces`).get()).toEqual({ n: 4 });
      expect(again.prepare(`SELECT COUNT(*) AS n FROM model_calls WHERE trace_id = 'mt_a'`).get())
        .toEqual({ n: 3 });
    } finally {
      again.close();
    }
  });

  it("persistBlobs 不得脱离 persistPayloads（§八十三）", () => {
    const handle = installModelObservabilityPersistence({
      lingxiHome: home,
      policy: { enabled: true, persistPayloads: false, persistBlobs: true },
    });
    expect(handle.policy.persistPayloads).toBe(false);
    expect(handle.policy.persistBlobs).toBe(false);
  });

  (process.platform === "win32" ? it.skip : it)("Unix：私有目录 0700 + DB/WAL/SHM 0600（§七十七/七十八）", async () => {
    const handle = installModelObservabilityPersistence({
      lingxiHome: home,
      policy: { enabled: true, persistPayloads: true },
    });
    // 触发一次 flush 产生 WAL/SHM。
    handle.observer?.handleModelCallEvent({
      eventType: "logical_call_start",
      timestamp: new Date().toISOString(),
      callId: "mc_perm",
      attemptId: null,
      traceId: "mt_perm",
    });
    handle.flushSync();
    await handle.close();
    const storeDir = path.join(home, "model-observability");
    expect(fs.statSync(storeDir).mode & 0o777).toBe(0o700);
    const dbPath = modelObservabilityDbPath(home);
    expect(fs.statSync(dbPath).mode & 0o777).toBe(0o600);
    for (const suffix of ["-wal", "-shm"]) {
      if (fs.existsSync(`${dbPath}${suffix}`)) {
        expect(fs.statSync(`${dbPath}${suffix}`).mode & 0o777).toBe(0o600);
      }
    }
  });
});
