/**
 * rewindToCheckpoint 端到端（阶段二·8）：三轮对话中途留具名存档点，
 * 第三轮跑偏后 rewind（连文件还原）——验证对话分支截断到存档那轮之前、
 * 被丢内容确实消失、影子仓库文件还原、session_branch_reset 事件带报告、
 * 事务不重发消息。harness 镜像 tests/file-rollback-e2e.test.ts。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import { rewindToCheckpoint } from "../core/session-turn-actions.ts";
import { WorkspaceSnapshotService } from "../core/workspace-snapshots.ts";
import { upsertSessionCheckpoint, getSessionCheckpoint } from "../core/session-checkpoints.ts";

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
  manager.appendMessage({ role: "assistant", content: [{ type: "text", text: "第一轮回答" }] } as any);
  const u2 = manager.appendMessage({ role: "user", content: [{ type: "text", text: "第二轮" }] } as any);
  manager.appendMessage({ role: "assistant", content: [{ type: "text", text: "第二轮回答" }] } as any);
  const u3 = manager.appendMessage({ role: "user", content: [{ type: "text", text: "第三轮（跑偏）" }] } as any);
  manager.appendMessage({ role: "assistant", content: [{ type: "text", text: "第三轮回答" }] } as any);
  return { manager, u1, u2, u3 };
}

function buildEngine(manager: any, { resourceIO, fileHistory }: any) {
  return {
    ensureSessionLoaded: vi.fn(async () => ({
      sessionManager: manager,
      agent: { replaceMessages: vi.fn(), state: { messages: [] } },
    })),
    isSessionStreaming: vi.fn(() => false),
    emitEvent: vi.fn(),
    setSessionBranchHead: vi.fn(),
    getSessionManifest: vi.fn(() => ({
      sessionId: "sess-rewind",
      lifecycle: "active",
      ownerAgentId: "hana",
      currentLocator: { path: sessionPath },
    })),
    resolveSessionOwnership: vi.fn(() => ({ agentId: "hana" })),
    getHomeCwd: vi.fn(() => workspace),
    lingxiHome,
    preferences: { getRollbackFileChanges: () => true },
    getResourceIO: () => resourceIO,
    getFileHistoryService: () => fileHistory,
  };
}

beforeEach(() => {
  tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "rewind-e2e-"));
  lingxiHome = path.join(tmpRoot, "lingxi");
  workspace = path.join(tmpRoot, "workspace");
  fs.mkdirSync(workspace, { recursive: true });
  sessionPath = path.join(tmpRoot, "sessions", "sess-rewind.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  fs.writeFileSync(sessionPath, "header\n");
  service = new WorkspaceSnapshotService({ lingxiHome });
});

afterEach(() => {
  fs.rmSync(tmpRoot, { recursive: true, force: true });
});

describe("rewindToCheckpoint 端到端", () => {
  it("存档点后跑偏一轮 → rewind：对话截断 + 文件还原 + 事件带报告 + 不重发", async () => {
    const { manager, u2, u3 } = buildSession();
    // 第二轮输入前拍照 + 留存档点（锚 u2）
    write("base.txt", "v1\n");
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: u2, label: "checkpoint:mid" });
    upsertSessionCheckpoint(sessionPath, {
      name: "mid",
      target: { role: "user", entryId: u2 },
      turnInputEntryId: u2,
      snapshotCommit: null,
      messageCount: 2,
    });
    // 第三轮「跑偏」：改文件 + 加垃圾文件
    write("base.txt", "v2 broken\n");
    write("junk.txt", "junk\n");
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: u3 });

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

    const result: any = await rewindToCheckpoint(engine, {
      sessionId: "sess-rewind",
      sessionPath,
      checkpointName: "mid",
      restoreFiles: true,
    }, { invalidateDerivedState: vi.fn(() => null) });

    // 对话：截断到 u2 之前（第三轮两条 + 第二轮两条被丢）
    expect(result.ok).toBe(true);
    expect(result.discardedEntries).toBeGreaterThanOrEqual(2);
    const branchAfter = manager.getBranch();
    const idsAfter = branchAfter.map((e: any) => e.id);
    expect(idsAfter).not.toContain(u3);
    // 第二轮也被丢（回到 u2 尚未发出的时点）
    expect(idsAfter).not.toContain(u2);

    // 文件：v1 还原、junk 删除
    expect(read("base.txt")).toBe("v1\n");
    expect(exists("junk.txt")).toBe(false);

    // 事件：branch_reset 携带 checkpoint 名与逐文件报告
    const resetEvent = (engine.emitEvent as any).mock.calls.find((c: any[]) => c[0]?.type === "session_branch_reset");
    expect(resetEvent).toBeTruthy();
    expect(resetEvent[0].checkpoint).toBe("mid");
    expect(resetEvent[0].fileRollbackReport?.ok).toBe(true);

    // 侧车存档点仍在（可再次 rewind）
    expect(getSessionCheckpoint(sessionPath, "mid")).toBeTruthy();
  });

  it("存档点不存在：如实报错，会话不动", async () => {
    const { manager } = buildSession();
    const engine = buildEngine(manager, { resourceIO: {}, fileHistory: {} });
    await expect(rewindToCheckpoint(engine, {
      sessionId: "sess-rewind",
      sessionPath,
      checkpointName: "nope",
    }, { invalidateDerivedState: vi.fn(() => null) })).rejects.toThrow(/not found/);
  });

  it("restoreFiles=false：不动文件，只回对话", async () => {
    const { manager, u2 } = buildSession();
    write("base.txt", "v1\n");
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: u2 });
    upsertSessionCheckpoint(sessionPath, { name: "cp", target: { role: "user", entryId: u2 }, turnInputEntryId: u2, messageCount: 2 });
    write("base.txt", "v2 changed\n");

    const engine = buildEngine(manager, { resourceIO: { write: vi.fn(), delete: vi.fn() }, fileHistory: {} });
    const result: any = await rewindToCheckpoint(engine, {
      sessionId: "sess-rewind", sessionPath, checkpointName: "cp", restoreFiles: false,
    }, { invalidateDerivedState: vi.fn(() => null) });
    expect(result.ok).toBe(true);
    expect(read("base.txt")).toBe("v2 changed\n"); // 文件保持
  });
});
