/**
 * 会话存档点侧车测试：CRUD、latest 覆盖语义、具名冲突拒绝、lazy 窗口化、
 * 损坏侧车容错。
 */
import { afterEach, describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  listSessionCheckpoints,
  getSessionCheckpoint,
  upsertSessionCheckpoint,
  dropSessionCheckpoint,
  checkpointSidecarPath,
  SESSION_CHECKPOINT_MAX_RECORDS,
} from "../core/session-checkpoints.ts";

const roots: string[] = [];
function freshSession() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-ckpt-"));
  roots.push(root);
  return path.join(root, "s.jsonl");
}

afterEach(() => {
  while (roots.length) fs.rmSync(roots.pop()!, { recursive: true, force: true });
});

describe("session-checkpoints 侧车", () => {
  it("create → list → get → drop 全链", () => {
    const sp = freshSession();
    const rec = upsertSessionCheckpoint(sp, {
      name: "before-refactor",
      target: { role: "user", entryId: "e1" },
      turnInputEntryId: "e1",
      snapshotCommit: "abc123",
      messageCount: 7,
    });
    expect(rec.name).toBe("before-refactor");
    expect(listSessionCheckpoints(sp)).toHaveLength(1);
    expect(getSessionCheckpoint(sp, "before-refactor")?.target.entryId).toBe("e1");
    expect(dropSessionCheckpoint(sp, "before-refactor")).toBe(true);
    expect(dropSessionCheckpoint(sp, "before-refactor")).toBe(false);
    expect(listSessionCheckpoints(sp)).toHaveLength(0);
  });

  it("latest 保留名重复 create 覆盖；其他名字冲突拒绝", () => {
    const sp = freshSession();
    upsertSessionCheckpoint(sp, { name: "latest", target: { role: "user", entryId: "e1" }, turnInputEntryId: "e1" });
    upsertSessionCheckpoint(sp, { name: "latest", target: { role: "user", entryId: "e2" }, turnInputEntryId: "e2" });
    expect(listSessionCheckpoints(sp)).toHaveLength(1);
    expect(getSessionCheckpoint(sp, "latest")?.target.entryId).toBe("e2");

    upsertSessionCheckpoint(sp, { name: "v1", target: { role: "user", entryId: "e1" }, turnInputEntryId: "e1" });
    expect(() => upsertSessionCheckpoint(sp, { name: "v1", target: { role: "user", entryId: "e3" }, turnInputEntryId: "e3" }))
      .toThrow(/already exists/);
    expect(getSessionCheckpoint(sp, "v1")?.target.entryId).toBe("e1");
  });

  it("超出上限 lazy 裁最老的非 latest 记录（latest 保留）", () => {
    const sp = freshSession();
    for (let i = 0; i < SESSION_CHECKPOINT_MAX_RECORDS + 10; i++) {
      upsertSessionCheckpoint(sp, { name: `cp-${i}`, target: { role: "user", entryId: `e${i}` }, turnInputEntryId: `e${i}` });
    }
    upsertSessionCheckpoint(sp, { name: "latest", target: { role: "user", entryId: "latest-e" }, turnInputEntryId: "latest-e" });
    const records = listSessionCheckpoints(sp);
    expect(records.length).toBeLessThanOrEqual(SESSION_CHECKPOINT_MAX_RECORDS + 1);
    expect(getSessionCheckpoint(sp, "latest")?.target.entryId).toBe("latest-e");
    // 最老的 cp-0 被裁，最新的还在
    expect(getSessionCheckpoint(sp, "cp-0")).toBeNull();
    expect(getSessionCheckpoint(sp, `cp-${SESSION_CHECKPOINT_MAX_RECORDS + 9}`)).toBeTruthy();
  });

  it("损坏/缺失侧车读作空清单（不抛）", () => {
    const sp = freshSession();
    expect(listSessionCheckpoints(sp)).toEqual([]);
    fs.writeFileSync(checkpointSidecarPath(sp), "{not json");
    expect(listSessionCheckpoints(sp)).toEqual([]);
  });
});
