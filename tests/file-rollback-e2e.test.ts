/**
 * 回退时撤销文件改动 — 端到端（临时工作区 + 临时会话 + 真实影子快照）
 *
 * 走真实 core/session-turn-actions.retrySessionTurn：造三轮文件改动
 * （新文件 / 修改 / shell 删除）→ 连文件回退第一轮 → 三处全部还原、
 * 报告逐文件准确、对话分支正确截断并重发、ws 事件携带同一份报告。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import { retrySessionTurn } from "../core/session-turn-actions.ts";
import { WorkspaceSnapshotService, snapshotSidecarPath } from "../core/workspace-snapshots.ts";

let tmpRoot: string;
let lingxiHome: string;
let workspace: string;
let sessionPath: string;
let service: WorkspaceSnapshotService;

function abs(rel: string): string {
  return path.join(workspace, ...rel.split("/"));
}

function write(rel: string, content: string): void {
  fs.mkdirSync(path.dirname(abs(rel)), { recursive: true });
  fs.writeFileSync(abs(rel), content, "utf-8");
}

function read(rel: string): string {
  return fs.readFileSync(abs(rel), "utf-8");
}

function exists(rel: string): boolean {
  return fs.existsSync(abs(rel));
}

function buildSession() {
  const manager = SessionManager.inMemory("/workspace");
  const u1 = manager.appendMessage({ role: "user", content: [{ type: "text", text: "第一轮" }] } as any);
  const a1 = manager.appendMessage({ role: "assistant", content: [{ type: "text", text: "第一轮回答" }] } as any);
  const u2 = manager.appendMessage({ role: "user", content: [{ type: "text", text: "第二轮" }] } as any);
  manager.appendMessage({ role: "assistant", content: [{ type: "text", text: "第二轮回答" }] } as any);
  const u3 = manager.appendMessage({ role: "user", content: [{ type: "text", text: "第三轮" }] } as any);
  manager.appendMessage({ role: "assistant", content: [{ type: "text", text: "第三轮回答" }] } as any);
  return { manager, u1, a1, u2, u3 };
}

function buildEngine(manager: any, { enabled = true, resourceIO, fileHistory }: any = {}) {
  return {
    ensureSessionLoaded: vi.fn(async () => ({
      sessionManager: manager,
      agent: { replaceMessages: vi.fn(), state: { messages: [] } },
    })),
    isSessionStreaming: vi.fn(() => false),
    emitEvent: vi.fn(),
    setSessionBranchHead: vi.fn(),
    getSessionManifest: vi.fn(() => ({
      sessionId: "sess-rollback",
      lifecycle: "active",
      ownerAgentId: "hana",
      currentLocator: { path: sessionPath },
    })),
    resolveSessionOwnership: vi.fn(() => ({ agentId: "hana" })),
    getHomeCwd: vi.fn(() => workspace),
    lingxiHome,
    preferences: { getRollbackFileChanges: () => enabled },
    getResourceIO: () => resourceIO,
    getFileHistoryService: () => fileHistory,
  };
}

beforeEach(() => {
  tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "rollback-e2e-"));
  lingxiHome = path.join(tmpRoot, "lingxi");
  workspace = path.join(tmpRoot, "workspace");
  fs.mkdirSync(workspace, { recursive: true });
  sessionPath = path.join(tmpRoot, "sessions", "sess-rollback.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  fs.writeFileSync(sessionPath, "header\n");
  service = new WorkspaceSnapshotService({ lingxiHome });
});

afterEach(() => {
  fs.rmSync(tmpRoot, { recursive: true, force: true });
});

describe("回退时撤销文件改动 端到端", () => {
  it("三轮改动后连文件回退第一轮：三处全部还原、报告准确、对话截断重发", async () => {
    write("mod.txt", "v1\n");
    write("gone.txt", "keep me\n");

    const { manager, u1, u2, u3 } = buildSession();
    // ── 三轮输入提交前的拍照（绑定各自 turn input entry id）──
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: u1 });
    write("created.txt", "brand new\n");                 // write 新建
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: u2 });
    write("mod.txt", "v2 changed\n");                    // edit 修改
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: u3 });
    fs.rmSync(abs("gone.txt"));                          // shell 删除

    expect(exists("created.txt")).toBe(true);
    expect(read("mod.txt")).toBe("v2 changed\n");
    expect(exists("gone.txt")).toBe(false);

    const writes: string[] = [];
    const deletes: string[] = [];
    const resourceIO = {
      write: vi.fn(async (ref: { path: string }, content: any) => {
        writes.push(ref.path);
        fs.mkdirSync(path.dirname(ref.path), { recursive: true });
        fs.writeFileSync(ref.path, content);
      }),
      delete: vi.fn(async (ref: { path: string }) => {
        deletes.push(ref.path);
        fs.rmSync(ref.path, { force: true });
      }),
    };
    const fileHistory = { captureNow: vi.fn(async () => {}) };
    const engine = buildEngine(manager, { resourceIO, fileHistory });
    const submit = vi.fn(async (_engine: any, opts: any) => {
      const hookResult = opts.beforeInputSideEffects?.();
      if (hookResult && typeof hookResult.then === "function") throw new TypeError("hook must be sync");
      return { text: "重新回答", toolMedia: [] };
    });

    const result: any = await retrySessionTurn(engine, {
      sessionId: "sess-rollback",
      sessionPath,
      target: { role: "user", entryId: u1 },
      fileRollback: "workspace",
    }, { submit, invalidateDerivedState: vi.fn(() => null) });

    // ── 文件：三处全部还原 ──
    expect(exists("created.txt")).toBe(false);
    expect(read("mod.txt")).toBe("v1\n");
    expect(exists("gone.txt")).toBe(true);
    expect(read("gone.txt")).toBe("keep me\n");
    expect(deletes).toEqual([abs("created.txt")]);
    expect(writes.sort()).toEqual([abs("gone.txt"), abs("mod.txt")].sort());
    // 恢复动作经文件历史以 origin=restore 记录（可再反悔）
    expect(fileHistory.captureNow).toHaveBeenCalledWith(workspace, "gone.txt", "restore");
    expect(fileHistory.captureNow).toHaveBeenCalledWith(workspace, "mod.txt", "restore");

    // ── 报告：逐文件准确 ──
    const report = result.fileRollbackReport;
    expect(report).toBeTruthy();
    expect(report.ok).toBe(true);
    expect(report.degraded).toBe(false);
    expect(report.reason).toBeNull();
    expect(report.failures).toEqual([]);
    expect(report.files).toEqual([
      { path: "created.txt", change: "added", action: "deleted", source: "snapshot", ok: true },
      { path: "gone.txt", change: "deleted", action: "restored", source: "snapshot", ok: true },
      { path: "mod.txt", change: "modified", action: "restored", source: "snapshot", ok: true },
    ]);

    // ── 对话：分支截断到第一轮之前（U1 是首条 → resetLeaf）并重发 ──
    expect(engine.setSessionBranchHead).toHaveBeenCalledWith(sessionPath, { leafId: null, reason: "replay_rewind" });
    const branchAfter = manager.getBranch();
    expect(branchAfter.filter((entry: any) => entry.type === "message")).toEqual([]);
    expect(branchAfter.at(-1)).toMatchObject({ customType: "hana-session-branch-reset" });
    expect(submit).toHaveBeenCalledTimes(1);

    // ── ws 事件与 HTTP 响应携带同一份报告 ──
    const resetCall = engine.emitEvent.mock.calls.find((call: any[]) => call[0]?.type === "session_branch_reset");
    expect(resetCall?.[1]).toBe(sessionPath);
    expect(resetCall?.[0].messageId).toBe(u1);
    expect(resetCall?.[0].fileRollbackReport).toEqual(report);

    // ── 侧车：该轮快照绑定 turn input entry id ──
    const sidecar = JSON.parse(fs.readFileSync(snapshotSidecarPath(sessionPath), "utf-8"));
    expect(sidecar.snapshots.map((entry: any) => entry.turnInputEntryId)).toEqual([u1, u2, u3]);
    expect(sidecar.snapshots.every((entry: any) => entry.degraded === false)).toBe(true);
  });

  it("开关关闭时核心不恢复文件、不产出报告（API 层另有 4xx）", async () => {
    write("mod.txt", "v1\n");
    const { manager, u1 } = buildSession();
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: u1 });
    write("mod.txt", "v2\n");

    const engine = buildEngine(manager, { enabled: false });
    const submit = vi.fn(async (_engine: any, opts: any) => {
      opts.beforeInputSideEffects?.();
      return { text: null, toolMedia: [] };
    });

    const result: any = await retrySessionTurn(engine, {
      sessionId: "sess-rollback",
      sessionPath,
      target: { role: "user", entryId: u1 },
      fileRollback: "workspace",
    }, { submit, invalidateDerivedState: vi.fn(() => null) });

    expect(result.fileRollbackReport).toBeUndefined();
    expect(read("mod.txt")).toBe("v2\n");
    const resetCall = engine.emitEvent.mock.calls.find((call: any[]) => call[0]?.type === "session_branch_reset");
    expect(resetCall?.[0].fileRollbackReport).toBeUndefined();
  });

  it("个别文件恢复失败：对话回退照常，报告列出失败与原因", async () => {
    write("ok.txt", "ok\n");
    write("bad.txt", "bad\n");
    const { manager, u1 } = buildSession();
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: u1 });
    write("ok.txt", "ok2\n");
    write("bad.txt", "bad2\n");

    const resourceIO = {
      write: vi.fn(async (ref: { path: string }, content: any) => {
        if (ref.path.endsWith("bad.txt")) throw new Error("EACCES: permission denied");
        fs.writeFileSync(ref.path, content);
      }),
      delete: vi.fn(async () => {}),
    };
    const engine = buildEngine(manager, { resourceIO, fileHistory: { captureNow: vi.fn(async () => {}) } });
    const submit = vi.fn(async (_engine: any, opts: any) => {
      opts.beforeInputSideEffects?.();
      return { text: null, toolMedia: [] };
    });

    const result: any = await retrySessionTurn(engine, {
      sessionId: "sess-rollback",
      sessionPath,
      target: { role: "user", entryId: u1 },
      fileRollback: "workspace",
    }, { submit, invalidateDerivedState: vi.fn(() => null) });

    expect(result.fileRollbackReport.ok).toBe(false);
    expect(result.fileRollbackReport.reason).toBe("partial_failure");
    expect(result.fileRollbackReport.failures).toEqual([
      expect.objectContaining({ path: "bad.txt", ok: false, reason: expect.stringContaining("permission denied") }),
    ]);
    // 失败不阻塞对话回退
    expect(engine.setSessionBranchHead).toHaveBeenCalledWith(sessionPath, { leafId: null, reason: "replay_rewind" });
    expect(submit).toHaveBeenCalledTimes(1);
    // 成功文件已还原
    expect(read("ok.txt")).toBe("ok\n");
  });
});
