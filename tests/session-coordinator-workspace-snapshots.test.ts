/**
 * session-coordinator 的「回退时撤销文件改动」拍照挂点测试。
 *
 * 直接驱动 _captureWorkspaceTurnSnapshot / _bindWorkspaceTurnSnapshot：
 * 开关关闭零开销、开关开启拍照并两段式绑定本轮 turn input entry id、
 * 自定义消息轮输入同样绑定。真实临时工作区 + 真实影子 git 仓库。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { SessionCoordinator } from "../core/session-coordinator.ts";
import { WorkspaceSnapshotService } from "../core/workspace-snapshots.ts";

let tmpRoot: string;
let lingxiHome: string;
let workspace: string;
let sessionPath: string;
let coordinator: SessionCoordinator;

function makeManager(branch: any[]) {
  return { getBranch: () => branch };
}

beforeEach(() => {
  tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "coordinator-snap-"));
  lingxiHome = path.join(tmpRoot, "lingxi");
  workspace = path.join(tmpRoot, "workspace");
  fs.mkdirSync(workspace, { recursive: true });
  fs.writeFileSync(path.join(workspace, "seed.txt"), "seed\n", "utf-8");
  sessionPath = path.join(tmpRoot, "sessions", "sess.jsonl");
  coordinator = new SessionCoordinator({});
});

afterEach(() => {
  fs.rmSync(tmpRoot, { recursive: true, force: true });
});

describe("SessionCoordinator 工作区快照挂点", () => {
  it("开关关闭时不拍照、不写侧车", async () => {
    const entry = { agentId: "hana", session: { sessionManager: makeManager([{ id: "u0", type: "message" }]) } };
    const engine = {
      lingxiHome,
      preferences: { getRollbackFileChanges: () => false },
      getHomeCwd: () => workspace,
    };

    const capture = await coordinator._captureWorkspaceTurnSnapshot(engine, sessionPath, entry);

    expect(capture).toBeNull();
    expect(fs.existsSync(`${sessionPath}.snapshots.json`)).toBe(false);
  });

  it("开关开启时拍照并绑定本轮 user turn input entry id", async () => {
    const branch: any[] = [{ id: "u0", type: "message", message: { role: "user" } }];
    const entry = { agentId: "hana", session: { sessionManager: makeManager(branch) } };
    const engine = {
      lingxiHome,
      preferences: { getRollbackFileChanges: () => true },
      getHomeCwd: () => workspace,
    };

    const capture = await coordinator._captureWorkspaceTurnSnapshot(engine, sessionPath, entry);
    expect(capture).toBeTruthy();

    // 回合启动后新铸的 entry（含一条展示型 custom 与真正的 user 输入）
    branch.push({ id: "presentation-1", type: "custom", customType: "hana-message-presentation" });
    branch.push({ id: "u1", type: "message", message: { role: "user" } });
    branch.push({ id: "a1", type: "message", message: { role: "assistant" } });
    coordinator._bindWorkspaceTurnSnapshot(capture, sessionPath, entry.session.sessionManager);

    const sidecar = JSON.parse(fs.readFileSync(`${sessionPath}.snapshots.json`, "utf-8"));
    expect(sidecar.snapshots).toHaveLength(1);
    expect(sidecar.snapshots[0].turnInputEntryId).toBe("u1");
    expect(sidecar.snapshots[0].degraded).toBe(false);
    expect(sidecar.snapshots[0].commit).toMatch(/^[0-9a-f]{40}$/);
    // commit 确实落在影子仓库、且不改动用户工作区
    expect(fs.existsSync(path.join(workspace, ".git"))).toBe(false);
    expect(fs.existsSync(path.join(lingxiHome, "workspace-snapshots"))).toBe(true);
  });

  it("开关开启时自定义消息轮输入也能绑定，且可按记录恢复", async () => {
    const branch: any[] = [];
    const entry = { agentId: "hana", session: { sessionManager: makeManager(branch) } };
    const engine = {
      lingxiHome,
      preferences: { getRollbackFileChanges: () => true },
      getHomeCwd: () => workspace,
    };

    const capture = await coordinator._captureWorkspaceTurnSnapshot(engine, sessionPath, entry);
    branch.push({
      id: "custom-1",
      type: "custom_message",
      customType: "hana-background-result",
      content: '<hana-background-result task-id="task-1" status="success"></hana-background-result>',
    });
    coordinator._bindWorkspaceTurnSnapshot(capture, sessionPath, entry.session.sessionManager);

    const sidecar = JSON.parse(fs.readFileSync(`${sessionPath}.snapshots.json`, "utf-8"));
    expect(sidecar.snapshots[0].turnInputEntryId).toBe("custom-1");

    // 端到端可用：改一个文件后按该轮恢复
    const service = new WorkspaceSnapshotService({ lingxiHome });
    fs.writeFileSync(path.join(workspace, "seed.txt"), "changed\n", "utf-8");
    const report = await service.restoreTurn({
      sessionPath,
      workspaceRoot: workspace,
      turnInputEntryId: "custom-1",
    });
    expect(report.ok).toBe(true);
    expect(fs.readFileSync(path.join(workspace, "seed.txt"), "utf-8")).toBe("seed\n");
  });

  it("没有工作区时跳过拍照", async () => {
    const entry = { agentId: null, session: { sessionManager: makeManager([]) } };
    const engine = {
      lingxiHome,
      preferences: { getRollbackFileChanges: () => true },
      getHomeCwd: () => null,
    };

    expect(await coordinator._captureWorkspaceTurnSnapshot(engine, sessionPath, entry)).toBeNull();
    expect(fs.existsSync(`${sessionPath}.snapshots.json`)).toBe(false);
  });
});
