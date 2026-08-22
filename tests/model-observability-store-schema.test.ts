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

  it("fresh DB：建 v1 全部表 + 索引，user_version=1", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    try {
      expect(readModelObservabilitySchemaVersion(db)).toBe(MODEL_OBSERVABILITY_SCHEMA_VERSION);
      expect(MODEL_OBSERVABILITY_SCHEMA_VERSION).toBe(1);
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
      expect(readModelObservabilitySchemaVersion(second)).toBe(1);
      const row = second.prepare(`SELECT trace_id FROM traces`).get();
      expect(row).toMatchObject({ trace_id: "mt_persist" });
    } finally {
      second.close();
    }
  });

  it("未知高版本数据库：schema_newer，文件保留不重建（§二十七）", () => {
    const db = openModelObservabilityDatabase(modelObservabilityDbPath(home));
    db.pragma("user_version = 5");
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
