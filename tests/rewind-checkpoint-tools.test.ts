/**
 * rewind 工具确认链测试：确认卡形状（含文件清单预览）、拒绝/超时不执行、
 * 确认后才调事务、事务失败如实上报、restoreFiles 降级提示。
 * checkpoint 工具：latest 滚动覆盖、具名冲突如实转达、list/drop。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

vi.mock("../lib/i18n.js", () => ({
  getLocale: () => "zh-CN",
  t: (key: string, values?: Record<string, unknown>) => {
    if (!values) return key;
    const suffix = Object.entries(values).map(([k, v]) => `${k}=${v}`).join(",");
    return `${key}:${suffix}`;
  },
}));

import { createRewindTool } from "../lib/tools/rewind-tool.ts";
import { createCheckpointTool } from "../lib/tools/checkpoint-tool.ts";
import { upsertSessionCheckpoint, listSessionCheckpoints } from "../core/session-checkpoints.ts";

const roots: string[] = [];
function freshSession() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-rewind-"));
  roots.push(root);
  return path.join(root, "s.jsonl");
}

function makeDeferred() {
  let resolve: any, reject: any;
  const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

describe("rewind 工具", () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { while (roots.length) fs.rmSync(roots.pop()!, { recursive: true, force: true }); });

  function makeDeps(opts: any = {}) {
    const deferred = makeDeferred();
    const confirmStore = { create: vi.fn(() => ({ confirmId: "c-1", promise: deferred.promise })) };
    const emitEvent = vi.fn();
    const rewindToCheckpoint = vi.fn(async () => ({ ok: true, discardedEntries: 12, fileRollbackReport: null }));
    const previewRestoreFiles = vi.fn(async () => opts.preview ?? null);
    const tool = createRewindTool({
      getSessionPath: () => opts.sessionPath ?? "/tmp/s.jsonl",
      getConfirmStore: () => (opts.noConfirmStore ? null : confirmStore),
      emitEvent,
      rewindToCheckpoint,
      previewRestoreFiles,
    });
    return { tool, confirmStore, emitEvent, rewindToCheckpoint, previewRestoreFiles, deferred };
  }

  it("发确认卡（含文件清单预览），确认后才执行事务", async () => {
    const { tool, emitEvent, rewindToCheckpoint, deferred } = makeDeps({
      preview: { available: true, degraded: false, fileCount: 2, files: [{ path: "a.ts", status: "M" }, { path: "b.ts", status: "A" }] },
    });
    const p = tool.execute("c1", { checkpoint: "v1", restoreFiles: true });
    await new Promise((r) => setImmediate(r)); // 等预览微任务走完、卡已发出
    expect(rewindToCheckpoint).not.toHaveBeenCalled();
    const card = emitEvent.mock.calls.find((c: any[]) => c[0]?.type === "session_confirmation");
    expect(card).toBeTruthy();
    expect(card[0].request.kind).toBe("rewind");
    expect(card[0].request.payload.files).toEqual(["~ changed a.ts", "+ new b.ts"]);
    deferred.resolve({ action: "confirmed" });
    const r: any = await p;
    expect(rewindToCheckpoint).toHaveBeenCalledWith({ sessionPath: "/tmp/s.jsonl", checkpointName: "v1", restoreFiles: true });
    expect(r.content[0].text).toContain("rewind.result.done");
    expect(r.details.rewind).toBe(true);
  });

  it("拒绝 / 超时：不执行事务，如实说明", async () => {
    for (const action of ["rejected", "timeout", "aborted"]) {
      const { tool, rewindToCheckpoint, deferred } = makeDeps();
      const p = tool.execute("c1", { checkpoint: "v1" });
      deferred.resolve({ action });
      const r: any = await p;
      expect(rewindToCheckpoint).not.toHaveBeenCalled();
      expect(r.details.rewind).toBe(false);
      expect(r.content[0].text).toContain("rewind.result");
    }
  });

  it("事务抛错 → isError 如实上报", async () => {
    const { tool, rewindToCheckpoint, deferred } = makeDeps();
    rewindToCheckpoint.mockRejectedValue(new Error("branch_commit_failed"));
    const p = tool.execute("c1", { checkpoint: "v1" });
    deferred.resolve({ action: "confirmed" });
    const r: any = await p;
    expect(r.isError).toBe(true);
    expect(r.content[0].text).toContain("branch_commit_failed");
  });

  it("restoreFiles 但快照不可用：卡里给降级提示，事务仍可对话回退", async () => {
    const { tool, emitEvent, deferred } = makeDeps({
      preview: { available: true, degraded: true, fileCount: 0, files: [] },
    });
    const p = tool.execute("c1", { checkpoint: "v1", restoreFiles: true });
    deferred.resolve({ action: "confirmed" });
    await p;
    const card = emitEvent.mock.calls.find((c: any[]) => c[0]?.type === "session_confirmation");
    expect(card[0].request.body).toContain("rewind.confirm.snapshotUnavailable");
  });

  it("无确认服务：诚实报不可用", async () => {
    const { tool } = makeDeps({ noConfirmStore: true });
    const r: any = await tool.execute("c1", { checkpoint: "v1" });
    expect(r.isError).toBe(true);
    expect(r.content[0].text).toContain("error.rewindUnavailable");
  });

  it("权限契约：恒 write（只读档拦截）", () => {
    const { tool } = makeDeps();
    expect(tool.sessionPermission.resolveInvocation({})).toEqual({
      action: "apply", kind: "write", capability: "rewind.apply",
    });
  });
});

describe("checkpoint 工具", () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { while (roots.length) fs.rmSync(roots.pop()!, { recursive: true, force: true }); });

  function makeDeps(sp: string) {
    return createCheckpointTool({
      getSessionPath: () => sp,
      captureSnapshot: async () => ({ commit: "deadbeef", degraded: false }),
      getLatestUserEntry: () => ({ id: "u-42", turnInputEntryId: "u-42" }),
      getMessageCount: () => 9,
    });
  }

  it("create 默认名 latest，重复 create 覆盖（滚动槽）", async () => {
    const sp = freshSession();
    const tool = makeDeps(sp);
    const r1: any = await tool.execute("c1", {});
    expect(r1.details.checkpoint).toBe("latest");
    expect(r1.content[0].text).toContain("workspace snapshot captured");
    const r2: any = await tool.execute("c2", {});
    expect(r2.details.checkpoint).toBe("latest");
    expect(listSessionCheckpoints(sp)).toHaveLength(1);
  });

  it("具名冲突：错误如实转达，不静默覆盖", async () => {
    const sp = freshSession();
    const tool = makeDeps(sp);
    await tool.execute("c1", { name: "v1" });
    const r: any = await tool.execute("c2", { name: "v1" });
    expect(r.isError).toBe(true);
    expect(r.content[0].text).toContain("already exists");
  });

  it("list / drop", async () => {
    const sp = freshSession();
    const tool = makeDeps(sp);
    await tool.execute("c1", { name: "v1" });
    await tool.execute("c2", { name: "v2" });
    const listed: any = await tool.execute("c3", { action: "list" });
    expect(listed.details.count).toBe(2);
    expect(listed.content[0].text).toContain("v1");
    const dropped: any = await tool.execute("c4", { action: "drop", name: "v1" });
    expect(dropped.details.dropped).toBe(true);
  });

  it("快照失败：degraded 如实说明（对话回滚仍可用）", async () => {
    const sp = freshSession();
    const tool = createCheckpointTool({
      getSessionPath: () => sp,
      captureSnapshot: async () => ({ commit: null, degraded: true }),
      getLatestUserEntry: () => ({ id: "u-1", turnInputEntryId: "u-1" }),
      getMessageCount: () => 1,
    });
    const r: any = await tool.execute("c1", { name: "v1" });
    expect(r.content[0].text).toContain("snapshot degraded");
  });

  it("权限契约：list=read；create/drop=write", () => {
    const tool = makeDeps("/tmp/s.jsonl");
    expect(tool.sessionPermission.resolveInvocation({ action: "list" }).kind).toBe("read");
    expect(tool.sessionPermission.resolveInvocation({ action: "create" }).kind).toBe("write");
    expect(tool.sessionPermission.resolveInvocation({ action: "drop" }).kind).toBe("write");
  });
});
