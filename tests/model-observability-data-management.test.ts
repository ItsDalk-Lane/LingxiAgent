/**
 * model-observability-data-management 测试：存储概况事实 + 窗口式删除
 * （多选天 = 多个单日窗口；整月/区间 = 起止窗口；空 windows = 全部）。
 * 真实临时 SQLite（最新 schema），blobStore 以桩替代（只计数）。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import {
  computeModelObservabilityStorage,
  deleteModelObservabilityData,
  validateModelObservabilityDeleteInput,
} from "../lib/llm/model-observability-data-management.ts";
import { modelObservabilityDbPath, openModelObservabilityDatabase } from "../lib/llm/model-observability-schema.ts";

let home: string;
let db: any;
let dbPath: string;

const blobStoreStub = {
  deleteBlobs: vi.fn((ids: string[]) => ids.length),
};

function seedTrace(traceId: string, lastSeenDay: string, calls: Array<{ callId: string; day: string; payloadChars: number }>) {
  db.prepare(
    `INSERT INTO traces (trace_id, first_seen_at, last_seen_at, call_count, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?)`,
  ).run(traceId, `${lastSeenDay}T08:00:00.000Z`, `${lastSeenDay}T09:00:00.000Z`, calls.length, `${lastSeenDay}T08:00:00.000Z`, `${lastSeenDay}T09:00:00.000Z`);
  for (const call of calls) {
    db.prepare(
      `INSERT INTO model_calls (call_id, trace_id, model_id, started_at, ended_at, terminal_status, payload_availability)
       VALUES (?, ?, ?, ?, ?, 'ok', 'captured')`,
    ).run(call.callId, traceId, "model-x", `${call.day}T08:30:00.000Z`, `${call.day}T08:31:00.000Z`);
    db.prepare(
      `INSERT INTO model_attempts (attempt_id, call_id, started_at) VALUES (?, ?, ?)`,
    ).run(`${call.callId}-a1`, call.callId, `${call.day}T08:30:00.000Z`);
    db.prepare(
      `INSERT INTO payload_records (call_id, kind, captured_at, visibility, fidelity, sanitization_status, redacted, truncated, degraded, record_char_count)
       VALUES (?, 'provider_request', ?, 'full', 'full', 'clean', 0, 0, 0, ?)`,
    ).run(call.callId, `${call.day}T08:30:00.000Z`, call.payloadChars);
    if (db.prepare(`SELECT 1 FROM sqlite_master WHERE type='table' AND name='model_call_usage'`).get()) {
      db.prepare(
        `INSERT INTO model_call_usage (model_call_id, usage_status, created_at, updated_at)
         VALUES (?, 'ok', ?, ?)`,
      ).run(call.callId, `${call.day}T08:31:00.000Z`, `${call.day}T08:31:00.000Z`);
    }
  }
}

beforeEach(() => {
  home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-datamgmt-"));
  dbPath = modelObservabilityDbPath(home);
  db = openModelObservabilityDatabase(dbPath);
  seedTrace("t-0911", "2026-09-11", [{ callId: "c1", day: "2026-09-11", payloadChars: 500 }]);
  seedTrace("t-0912", "2026-09-12", [{ callId: "c2", day: "2026-09-12", payloadChars: 800 }]);
  seedTrace("t-1015", "2026-10-15", [{ callId: "c3", day: "2026-10-15", payloadChars: 300 }]);
});

afterEach(() => {
  db.close();
  try { fs.rmSync(home, { recursive: true, force: true }); } catch { /* tmp */ }
});

describe("computeModelObservabilityStorage", () => {
  it("counts days/calls from real rows and reports sizes", () => {
    const overview = computeModelObservabilityStorage({ db, dbPath });
    expect(overview.days).toBe(3);
    expect(overview.oldestAt).toBe("2026-09-11T08:30:00.000Z");
    expect(overview.calls).toBe(3);
    expect(overview.sizes.payloadEstimateChars).toBe(1600);
    expect(overview.perDay.map((day) => day.date)).toEqual(["2026-09-11", "2026-09-12", "2026-10-15"]);
  });
});

describe("validateModelObservabilityDeleteInput", () => {
  it("accepts windows and rejects malformed input", () => {
    expect(validateModelObservabilityDeleteInput({ categories: ["trace"], windows: [] }).ok).toBe(true);
    expect(validateModelObservabilityDeleteInput({
      categories: ["trace"],
      windows: [{ from: "2026-09-01", to: "2026-09-30" }],
    }).ok).toBe(true);
    expect(validateModelObservabilityDeleteInput({}).ok).toBe(false);
    expect(validateModelObservabilityDeleteInput({ categories: [], windows: [] }).ok).toBe(false);
    expect(validateModelObservabilityDeleteInput({ categories: ["nope"], windows: [] }).ok).toBe(false);
    expect(validateModelObservabilityDeleteInput({ categories: ["trace"], windows: {} }).ok).toBe(false);
    expect(validateModelObservabilityDeleteInput({
      categories: ["trace"],
      windows: [{ from: "2026-9-1", to: "2026-09-30" }],
    }).ok).toBe(false);
    expect(validateModelObservabilityDeleteInput({
      categories: ["trace"],
      windows: [{ from: "2026-09-30", to: "2026-09-01" }],
    }).ok).toBe(false);
  });
});

describe("deleteModelObservabilityData", () => {
  const ctx = () => ({
    db,
    blobStore: blobStoreStub,
    markPayloadAvailability: vi.fn(),
    dbPath,
    now: () => "2026-10-17T00:00:00.000Z",
  });

  it("single-day window deletes the whole trace of that day only", () => {
    const stats = deleteModelObservabilityData(ctx(), {
      categories: ["trace"],
      windows: [{ from: "2026-09-11", to: "2026-09-11" }],
    });
    expect(stats.deletedTraces).toBe(1);
    expect(db.prepare(`SELECT trace_id FROM traces`).all().map((r: any) => r.trace_id))
      .toEqual(["t-0912", "t-1015"]);
    expect(db.prepare(`SELECT COUNT(*) AS n FROM model_attempts`).get().n).toBe(2);
  });

  it("multi-day windows delete multiple days in one operation", () => {
    const stats = deleteModelObservabilityData(ctx(), {
      categories: ["trace"],
      windows: [
        { from: "2026-09-11", to: "2026-09-11" },
        { from: "2026-10-15", to: "2026-10-15" },
      ],
    });
    expect(stats.deletedTraces).toBe(2);
    expect(db.prepare(`SELECT trace_id FROM traces`).get().trace_id).toBe("t-0912");
  });

  it("month window deletes every trace inside the month", () => {
    const stats = deleteModelObservabilityData(ctx(), {
      categories: ["trace"],
      windows: [{ from: "2026-09-01", to: "2026-09-30" }],
    });
    expect(stats.deletedTraces).toBe(2);
    expect(db.prepare(`SELECT trace_id FROM traces`).get().trace_id).toBe("t-1015");
  });

  it("overlapping windows do not break deletion (trace ids deduped)", () => {
    const stats = deleteModelObservabilityData(ctx(), {
      categories: ["trace"],
      windows: [
        { from: "2026-09-01", to: "2026-09-30" },
        { from: "2026-09-10", to: "2026-10-15" },
      ],
    });
    expect(stats.deletedTraces).toBe(3);
    expect(db.prepare(`SELECT COUNT(*) AS n FROM traces`).get().n).toBe(0);
  });

  it("payload-only window keeps metadata but drops bodies of that window", () => {
    const stats = deleteModelObservabilityData(ctx(), {
      categories: ["payload"],
      windows: [{ from: "2026-09-01", to: "2026-09-30" }],
    });
    expect(stats.deletedPayloadRecords).toBe(2);
    expect(db.prepare(`SELECT COUNT(*) AS n FROM payload_records`).get().n).toBe(1);
    expect(db.prepare(`SELECT COUNT(*) AS n FROM model_calls`).get().n).toBe(3);
  });

  it("media-only window deletes only unreferenced blobs inside the window", () => {
    db.prepare(
      `INSERT INTO blob_objects (blob_id, created_at, byte_length, media_type, state, relative_path)
       VALUES ('b-free', '2026-09-11T08:00:00.000Z', 10, 'image/png', 'ready', 'mb/b-free.bin')`,
    ).run();
    db.prepare(
      `INSERT INTO blob_objects (blob_id, created_at, byte_length, media_type, state, relative_path)
       VALUES ('b-kept', '2026-10-15T08:00:00.000Z', 20, 'image/png', 'ready', 'mb/b-kept.bin')`,
    ).run();
    const stats = deleteModelObservabilityData(ctx(), {
      categories: ["media"],
      windows: [{ from: "2026-09-01", to: "2026-09-30" }],
    });
    expect(stats.deletedBlobFiles).toBe(1);
    expect(blobStoreStub.deleteBlobs).toHaveBeenCalledWith(["b-free"]);
  });

  it("empty windows = all data, wiped and compacted", () => {
    const stats = deleteModelObservabilityData(ctx(), {
      categories: ["trace"],
      windows: [],
    });
    expect(stats.deletedTraces).toBe(3);
    expect(stats.compacted).toBe(true);
    expect(db.prepare(`SELECT COUNT(*) AS n FROM traces`).get().n).toBe(0);
  });
});
