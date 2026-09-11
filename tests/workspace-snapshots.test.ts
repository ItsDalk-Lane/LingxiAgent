/**
 * workspace-snapshots — 影子快照 / 恢复核心测试
 *
 * 覆盖任务书五情形：新建文件删除、修改还原、shell 删除文件还原、
 * 排除目录不进快照、拍照失败走 write/edit 备份兜底；外加解析器与侧车绑定。
 * 全部在真实临时工作区 + 真实影子 git 仓库上跑，不 mock 被测对象。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { execFile, execFileSync } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  DEFAULT_SNAPSHOT_EXCLUDES,
  WorkspaceSnapshotService,
  parseNameStatusZ,
  parseUntrackedZ,
  snapshotSidecarPath,
  workspaceSnapshotRepoDir,
} from "../core/workspace-snapshots.ts";
import { CheckpointStore } from "../lib/checkpoint-store.ts";

const execFileAsync = promisify(execFile);

let tmpRoot: string;
let lingxiHome: string;
let workspace: string;
let sessionsDir: string;
let sessionPath: string;
let service: WorkspaceSnapshotService;

function writeFile(rel: string, content: string): void {
  const abs = path.join(workspace, ...rel.split("/"));
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, content, "utf-8");
}

function readFile(rel: string): string {
  return fs.readFileSync(path.join(workspace, ...rel.split("/")), "utf-8");
}

function exists(rel: string): boolean {
  return fs.existsSync(path.join(workspace, ...rel.split("/")));
}

beforeEach(() => {
  tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "ws-snap-"));
  lingxiHome = path.join(tmpRoot, "lingxi");
  workspace = path.join(tmpRoot, "workspace");
  sessionsDir = path.join(tmpRoot, "sessions");
  fs.mkdirSync(workspace, { recursive: true });
  fs.mkdirSync(sessionsDir, { recursive: true });
  sessionPath = path.join(sessionsDir, "session-1.jsonl");
  service = new WorkspaceSnapshotService({ lingxiHome });
});

afterEach(() => {
  fs.rmSync(tmpRoot, { recursive: true, force: true });
});

describe("workspace-snapshots 解析器", () => {
  it("parseNameStatusZ 解析 A/M/D 序列", () => {
    const out = "M\0src/a.ts\0D\0old.txt\0A\0new.txt\0";
    expect(parseNameStatusZ(out)).toEqual([
      { path: "src/a.ts", status: "M" },
      { path: "old.txt", status: "D" },
      { path: "new.txt", status: "A" },
    ]);
  });

  it("parseUntrackedZ 只取未跟踪文件", () => {
    const out = "?? untracked.txt\0 M tracked.txt\0?? dir/new.js\0";
    expect(parseUntrackedZ(out)).toEqual(["untracked.txt", "dir/new.js"]);
  });
});

describe("workspace-snapshots 五情形", () => {
  it("情形1：新建文件在回退时被删除", async () => {
    writeFile("keep.txt", "keep\n");
    const record = await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });
    expect(record.degraded).toBe(false);
    expect(record.commit).toBeTruthy();

    writeFile("created-after.txt", "brand new\n");
    expect(exists("created-after.txt")).toBe(true);

    const report = await service.restoreTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });
    expect(report.ok).toBe(true);
    expect(report.degraded).toBe(false);
    expect(exists("created-after.txt")).toBe(false);
    expect(report.files).toEqual([
      { path: "created-after.txt", change: "added", action: "deleted", source: "snapshot", ok: true },
    ]);
    expect(exists("keep.txt")).toBe(true);
  });

  it("情形2：修改的文件还原到该轮开始前内容", async () => {
    writeFile("mod.txt", "before\n");
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });

    writeFile("mod.txt", "after\n");
    expect(readFile("mod.txt")).toBe("after\n");

    const report = await service.restoreTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });
    expect(report.ok).toBe(true);
    expect(readFile("mod.txt")).toBe("before\n");
    expect(report.files).toEqual([
      { path: "mod.txt", change: "modified", action: "restored", source: "snapshot", ok: true },
    ]);
  });

  it("情形3：shell 删除的文件在回退时还原", async () => {
    writeFile("gone.txt", "keep me\n");
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });

    // 模拟该轮里的 shell 删除
    execFileSync("rm", ["-f", path.join(workspace, "gone.txt")]);
    expect(exists("gone.txt")).toBe(false);

    const report = await service.restoreTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });
    expect(report.ok).toBe(true);
    expect(exists("gone.txt")).toBe(true);
    expect(readFile("gone.txt")).toBe("keep me\n");
    expect(report.files).toEqual([
      { path: "gone.txt", change: "deleted", action: "restored", source: "snapshot", ok: true },
    ]);
  });

  it("情形4：排除目录不进快照、回退也不重建", async () => {
    writeFile("src/app.ts", "export const a = 1;\n");
    writeFile("node_modules/pkg/index.js", "module.exports = {};\n");
    writeFile("dist/bundle.js", "bundled\n");
    writeFile("build/out.js", "built\n");
    writeFile(".git/config", "[core]\n");
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });

    const repoDir = workspaceSnapshotRepoDir(lingxiHome, workspace);
    const { stdout } = await execFileAsync("git", [`--git-dir=${repoDir}`, "ls-tree", "-r", "--name-only", "HEAD"]);
    const tracked = stdout.split("\n").map((line) => line.trim()).filter(Boolean);
    expect(tracked).toEqual(["src/app.ts"]);
    expect(tracked).not.toContain("node_modules/pkg/index.js");
    expect(tracked).not.toContain("dist/bundle.js");
    expect(tracked).not.toContain("build/out.js");
    expect(tracked).not.toContain(".git/config");
    // 排除清单确实落在影子仓库 info/exclude
    const exclude = fs.readFileSync(path.join(repoDir, "info", "exclude"), "utf-8");
    for (const pattern of DEFAULT_SNAPSHOT_EXCLUDES) expect(exclude).toContain(pattern);

    // 回退：任务里往排除目录新写的文件不该被当成「新增」而误删/误加
    writeFile("node_modules/pkg/extra.js", "x\n");
    const changes = await service.collectChanges(workspace, (await service.previewTurn({
      sessionPath,
      workspaceRoot: workspace,
      turnInputEntryId: "turn-1",
    })).commit!);
    expect(changes.map((entry) => entry.path)).not.toContain("node_modules/pkg/extra.js");
  });

  it("情形5：拍照失败降级为 write/edit 改前备份兜底", async () => {
    // 让影子仓库无法创建：lingxiHome 的父路径是一个普通文件
    const blocker = path.join(tmpRoot, "blocker");
    fs.writeFileSync(blocker, "not a directory\n");
    const brokenService = new WorkspaceSnapshotService({ lingxiHome: path.join(blocker, "lingxi") });

    writeFile("edit.txt", "v1\n");
    const record = await brokenService.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });
    expect(record.degraded).toBe(true);
    expect(record.commit).toBeNull();
    expect(typeof record.reason).toBe("string");

    // 该轮的 write/edit 改前备份（生产由 wrapWithCheckpoint 写入同一 CheckpointStore）
    const backupStore = new CheckpointStore(path.join(tmpRoot, "checkpoints"));
    const backupId = await backupStore.save({
      sessionPath,
      tool: "edit",
      source: "llm",
      reason: "tool-edit",
      filePath: path.join(workspace, "edit.txt"),
      maxSizeKb: 1024,
    });
    expect(backupId).toBeTruthy();

    writeFile("edit.txt", "v2-changed\n");
    const report = await brokenService.restoreTurn({
      sessionPath,
      workspaceRoot: workspace,
      turnInputEntryId: "turn-1",
      backupStore,
    });
    expect(report.degraded).toBe(true);
    expect(report.ok).toBe(true);
    expect(report.commit).toBeNull();
    expect(readFile("edit.txt")).toBe("v1\n");
    expect(report.files).toEqual([
      { path: "edit.txt", change: "modified", action: "restored", source: "backup", ok: true },
    ]);
    // 侧车记录该轮已降级（无 commit），供 UI 置灰提示
    const sidecar = JSON.parse(fs.readFileSync(snapshotSidecarPath(sessionPath), "utf-8"));
    expect(sidecar.snapshots[0].degraded).toBe(true);
    expect(sidecar.snapshots[0].turnInputEntryId).toBe("turn-1");
  });
});

describe("workspace-snapshots 绑定与预览", () => {
  it("两段式绑定：拍照后补绑 turnInputEntryId，可按 id 与时间兜底查回", async () => {
    writeFile("a.txt", "a\n");
    const record = await service.captureTurn({ sessionPath, workspaceRoot: workspace, label: "loop" });
    expect(record.turnInputEntryId).toBeNull();

    const bound = service.bindTurnInput({ sessionPath, capturedAt: record.capturedAt, turnInputEntryId: "turn-42" });
    expect(bound).toBe(true);
    expect(service.bindTurnInput({ sessionPath, capturedAt: record.capturedAt, turnInputEntryId: "turn-43" })).toBe(false);

    expect(service.getTurnSnapshot({ sessionPath, turnInputEntryId: "turn-42" })?.commit).toBe(record.commit);

    // 未绑定记录：按 createdAtHint 兜底
    const second = await service.captureTurn({ sessionPath, workspaceRoot: workspace });
    const resolved = service.getTurnSnapshot({
      sessionPath,
      turnInputEntryId: "turn-99",
      createdAtHint: second.capturedAt + 5,
    });
    expect(resolved?.commit).toBe(second.commit);
    expect(service.getTurnSnapshot({ sessionPath, turnInputEntryId: "missing" })).toBeNull();
  });

  it("previewTurn 给出受影响文件清单与数量，无检查点明确不可用", async () => {
    writeFile("p.txt", "p\n");
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });
    writeFile("p.txt", "p2\n");
    writeFile("q.txt", "q\n");

    const preview = await service.previewTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });
    expect(preview.available).toBe(true);
    expect(preview.degraded).toBe(false);
    expect(preview.fileCount).toBe(2);
    expect(preview.files.map((entry) => entry.path).sort()).toEqual(["p.txt", "q.txt"]);

    const missing = await service.previewTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "nope" });
    expect(missing).toEqual({ available: false, degraded: false, commit: null, reason: "no_checkpoint", files: [], fileCount: 0 });
  });

  it("回退经 ResourceIO 写回并用 origin=restore 记入文件历史", async () => {
    writeFile("io.txt", "original\n");
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });
    writeFile("io.txt", "changed\n");

    const writes: Array<{ path: string; content: string }> = [];
    const history: Array<{ rel: string; origin: string }> = [];
    const resourceIO = {
      write: async (ref: { path: string }, content: any) => { writes.push({ path: ref.path, content: Buffer.from(content).toString("utf-8") }); },
      delete: async () => {},
    };
    const report = await service.restoreTurn({
      sessionPath,
      workspaceRoot: workspace,
      turnInputEntryId: "turn-1",
      resourceIO,
      fileHistory: { captureNow: async (_root: string, rel: string, origin: "restore") => { history.push({ rel, origin }); } },
    });
    expect(report.ok).toBe(true);
    expect(writes).toEqual([{ path: path.join(workspace, "io.txt"), content: "original\n" }]);
    expect(history).toEqual([{ rel: "io.txt", origin: "restore" }]);
    // 默认 fs 写出不应发生（已交给 ResourceIO）；内容仍保持 changed
    expect(readFile("io.txt")).toBe("changed\n");
  });

  it("个别文件失败不阻塞其它文件，逐文件报告失败与原因", async () => {
    writeFile("ok.txt", "ok\n");
    writeFile("bad.txt", "bad\n");
    await service.captureTurn({ sessionPath, workspaceRoot: workspace, turnInputEntryId: "turn-1" });
    writeFile("ok.txt", "ok2\n");
    writeFile("bad.txt", "bad2\n");

    const resourceIO = {
      write: async (ref: { path: string }) => {
        if (ref.path.endsWith("bad.txt")) throw new Error("permission denied by test");
      },
      delete: async () => {},
    };
    const report = await service.restoreTurn({
      sessionPath,
      workspaceRoot: workspace,
      turnInputEntryId: "turn-1",
      resourceIO,
    });
    expect(report.ok).toBe(false);
    expect(report.reason).toBe("partial_failure");
    expect(report.failures).toHaveLength(1);
    expect(report.failures[0].path).toBe("bad.txt");
    expect(report.failures[0].reason).toContain("permission denied");
    const okEntry = report.files.find((entry) => entry.path === "ok.txt");
    expect(okEntry?.ok).toBe(true);
  });
});
