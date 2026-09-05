/**
 * KnowledgeTurnScope（任务书 §二十/§四十三/§九十一，Phase 4）：
 * - store：schema v11 两表、创建时同事务冻结（选中 notebooks → 活跃
 *   memberships → 每源最新 snapshot/artifact）、轮级 supersede、close 幂等；
 * - 检索：retrieveForNotebooks 给定 frozenArtifacts 时锚定冻结版本，
 *   watcher 轮内产生的新版本不参与本轮检索。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeManager } from "./fixtures/knowledge-legacy/legacy-query-service.ts";
import { KnowledgeError } from "../lib/knowledge/errors.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-turn-scope-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(async () => {
  for (const manager of managers.splice(0)) await manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

const SESSION = "/tmp/lingxi-turn-scope-test/session.jsonl";

function createManager() {
  const manager = new KnowledgeManager({ lingxiHome: tempHome() });
  managers.push(manager);
  return manager;
}

async function importText(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  text: string,
  displayName: string,
) {
  const imported = await manager.importPastedText({ studioId, notebookId, text, displayName });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  return { imported, artifact };
}

function frozenMap(scope: { sources: Array<{
  sourceId: string;
  contentSnapshotId: string;
  parseArtifactId: string | null;
}> }) {
  return new Map(scope.sources.map(source => [source.sourceId, {
    contentSnapshotId: source.contentSnapshotId,
    parseArtifactId: source.parseArtifactId,
  }]));
}

describe("KnowledgeTurnScope（store 层）", () => {
  it("创建即冻结：选中 notebooks 的活跃 membership × 每源最新 snapshot/artifact", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "A" });
    const notebookB = manager.createNotebook({ studioId, name: "B" });
    const notebookC = manager.createNotebook({ studioId, name: "C（未选中）" });
    const a1 = await importText(manager, studioId, notebookA.id, "A 的源一。", "a1.txt");
    const shared = await importText(manager, studioId, notebookA.id, "A 与 C 共享的源。", "shared.txt");
    manager.addSourceToNotebook({ studioId, notebookId: notebookC.id, sourceId: shared.imported.source.id });
    await importText(manager, studioId, notebookC.id, "C 独有。", "c.txt");

    const scope = manager.createTurnScope({
      studioId,
      sessionPath: SESSION,
      turnId: "msg_client_1",
      notebookIds: [notebookA.id, notebookB.id],
    });
    expect(scope.status).toBe("active");
    expect(scope.turnId).toBe("msg_client_1");
    expect(scope.sessionPath).toBe(SESSION);
    expect(scope.studioId).toBe(studioId);
    expect(scope.notebookIds).toEqual([notebookA.id, notebookB.id]);
    // 冻结集合 = A/B 的活跃源；C 独有的源不在其中。
    expect(scope.sources.map(source => source.sourceId).sort()).toEqual(
      [a1.imported.source.id, shared.imported.source.id].sort(),
    );
    const sharedRow = scope.sources.find(source => source.sourceId === shared.imported.source.id)!;
    expect(sharedRow.notebookIds).toEqual([notebookA.id]);
    expect(sharedRow.contentSnapshotId).toBe(shared.imported.snapshot.id);
    expect(sharedRow.parseArtifactId).toBe(shared.artifact.id);

    // getTurnScope 读回同一冻结事实；校验助手一致。
    const loaded = manager.getTurnScope({ scopeId: scope.id })!;
    expect(loaded).toEqual(scope);
    expect(manager.isSourceInTurnScope({ scopeId: scope.id, sourceId: a1.imported.source.id })).toBe(true);
    expect(manager.isSourceInTurnScope({ scopeId: scope.id, sourceId: "src_c" })).toBe(false);
    expect(manager.getTurnScopeFrozenSource({ scopeId: scope.id, sourceId: shared.imported.source.id }))
      .toEqual(sharedRow);
    expect(manager.getTurnScope({ scopeId: "kts_missing" })).toBeNull();
  });

  it("轮级 supersede：同会话新 scope 关闭旧 scope；其他会话不受影响", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "A" });
    await importText(manager, studioId, notebook.id, "内容。", "a.txt");

    const first = manager.createTurnScope({ studioId, sessionPath: SESSION, notebookIds: [notebook.id] });
    const otherSession = manager.createTurnScope({
      studioId,
      sessionPath: "/tmp/lingxi-turn-scope-test/other.jsonl",
      notebookIds: [notebook.id],
    });
    const second = manager.createTurnScope({ studioId, sessionPath: SESSION, notebookIds: [notebook.id] });

    expect(manager.getTurnScope({ scopeId: first.id })!.status).toBe("closed");
    expect(manager.getTurnScope({ scopeId: second.id })!.status).toBe("active");
    expect(manager.getTurnScope({ scopeId: otherSession.id })!.status).toBe("active");
    // turnId 缺省时 store 生成稳定标识。
    expect(first.turnId).toMatch(/^turn_/);
  });

  it("closeTurnScope 幂等；closed 行保留（追溯语义，GC 留给 Phase 5）", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "A" });
    await importText(manager, studioId, notebook.id, "内容。", "a.txt");
    const scope = manager.createTurnScope({ studioId, sessionPath: SESSION, notebookIds: [notebook.id] });

    expect(manager.closeTurnScope({ scopeId: scope.id })!.status).toBe("closed");
    expect(manager.closeTurnScope({ scopeId: scope.id })!.status).toBe("closed");
    // 行与冻结源集合保留可读（EvidenceManifest 追溯）。
    expect(manager.getTurnScope({ scopeId: scope.id })!.sources).toHaveLength(1);
    expect(manager.closeTurnScope({ scopeId: "kts_missing" })).toBeNull();
  });

  it("冻结后 membership 移除不影响 scope 的冻结事实", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "A" });
    const { imported } = await importText(manager, studioId, notebook.id, "内容。", "a.txt");
    const scope = manager.createTurnScope({ studioId, sessionPath: SESSION, notebookIds: [notebook.id] });

    manager.removeSourceFromNotebook({ studioId, notebookId: notebook.id, sourceId: imported.source.id });
    const loaded = manager.getTurnScope({ scopeId: scope.id })!;
    expect(loaded.sources).toHaveLength(1);
    expect(loaded.sources[0].sourceId).toBe(imported.source.id);
  });

  it("未知/空 notebookIds 显式拒绝（不静默建空 scope）", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "A" });
    // createTurnScope 是同步 store 调用：同步抛错（显式拒绝）。
    expect(() => manager.createTurnScope({
      studioId,
      sessionPath: SESSION,
      notebookIds: [notebook.id, "nb_missing"],
    })).toThrowError(expect.objectContaining({ code: "KNOWLEDGE_NOT_FOUND" }));
    expect(() => manager.createTurnScope({
      studioId,
      sessionPath: SESSION,
      notebookIds: [],
    })).toThrowError(KnowledgeError);
    // 失败不残留 scope 行。
    expect(manager.getTurnScope({ scopeId: "kts_missing" })).toBeNull();
  });
});

describe("KnowledgeTurnScope（注入链路冻结检索，§四十三）", () => {
  it("源在轮内更新后，带 frozenArtifacts 的检索仍命中冻结的 V1 artifact", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const filesDir = tempHome();
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    const filePath = path.join(filesDir, "笔记.md");
    fs.writeFileSync(filePath, "# 第一版\n\n苹果项目九月交付。\n", "utf8");
    const imported = await manager.importFile({ studioId, notebookId: notebook.id, filePath });
    const artifactV1 = await manager.parseSource({ studioId, sourceId: imported.source.id });
    const chunkTargetChars = manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id });
    manager.queryService.indexArtifactForIngestion(studioId, artifactV1.id, { targetChars: chunkTargetChars });

    const scope = manager.createTurnScope({ studioId, sessionPath: SESSION, notebookIds: [notebook.id] });
    expect(scope.sources[0].parseArtifactId).toBe(artifactV1.id);

    // watcher 路径：V2 snapshot + artifact（旧版本保留）。
    fs.writeFileSync(filePath, "# 第二版\n\n苹果项目十月交付。\n", "utf8");
    const refreshed = await manager.refreshFileSource({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
    });
    const artifactV2 = refreshed.parseArtifact!;
    expect(artifactV2.id).not.toBe(artifactV1.id);
    manager.queryService.indexArtifactForIngestion(studioId, artifactV2.id, { targetChars: chunkTargetChars });

    // 冻结检索：候选与分片清单都来自 V1 artifact（内容不含十月）。
    const frozen = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "苹果项目 交付",
      frozenArtifacts: frozenMap(scope),
    });
    expect(frozen.sources.map(source => source.parseArtifactId)).toEqual([artifactV1.id]);
    expect(frozen.candidates.length).toBeGreaterThan(0);
    expect(frozen.candidates.every(chunk => chunk.parseArtifactId === artifactV1.id)).toBe(true);
    expect(frozen.candidates.map(chunk => chunk.text).join("\n")).not.toContain("十月");

    // 对照：不带冻结集合时命中最新 V2。
    const latest = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "苹果项目 交付",
    });
    expect(latest.sources.map(source => source.parseArtifactId)).toEqual([artifactV2.id]);
    expect(latest.candidates.every(chunk => chunk.parseArtifactId === artifactV2.id)).toBe(true);
  });

  it("冻结集合之外的源（冻结后新加入 membership）不参与本轮检索", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    const first = await importText(manager, studioId, notebook.id, "苹果项目九月交付。", "a.txt");
    manager.queryService.indexArtifactForIngestion(studioId, first.artifact.id, {
      targetChars: manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id }),
    });
    const scope = manager.createTurnScope({ studioId, sessionPath: SESSION, notebookIds: [notebook.id] });

    // 冻结后才加入 notebook 的源：本轮不可见。
    const late = await importText(manager, studioId, notebook.id, "苹果项目十月交付。", "late.txt");
    manager.queryService.indexArtifactForIngestion(studioId, late.artifact.id, {
      targetChars: manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id }),
    });

    const frozen = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "苹果项目",
      frozenArtifacts: frozenMap(scope),
    });
    expect(frozen.sources.map(source => source.sourceId)).toEqual([first.imported.source.id]);
    expect(frozen.candidates.every(chunk => chunk.parseArtifactId === first.artifact.id)).toBe(true);

    const latest = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "苹果项目",
    });
    expect(latest.sources.map(source => source.sourceId).sort()).toEqual(
      [first.imported.source.id, late.imported.source.id].sort(),
    );
  });
});
