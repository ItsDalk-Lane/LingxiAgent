import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createKnowledgeReadTool } from "../lib/tools/knowledge-read-tool.ts";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";
import type { KnowledgeTurnScope } from "../lib/knowledge/types.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-read-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(async () => {
  vi.restoreAllMocks();
  for (const manager of managers.splice(0)) await manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

/** 主会话身份：scope 绑定该 sessionPath，工具执行上下文与之匹配才允许读取。 */
const MAIN_SESSION_PATH = "/tmp/lingxi-knowledge-read-test/main-session.jsonl";
const MAIN_SESSION = { sessionPath: MAIN_SESSION_PATH, scopeOwnerSessionPath: MAIN_SESSION_PATH };

/**
 * 按 owning notebook 的 RetrievalProfile 锚定列出索引 chunk（schema v2：
 * chunk 挂在 ChunkIndexVariant 上，与工具内部的读片锚点同一解析链）。
 */
function listProfileChunks(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  artifactId: string,
) {
  const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifactId });
  const strategy = resolveKnowledgeChunkerConfig(blocks, {
    targetChars: manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId }),
  }).strategy;
  const { chunkProfile } = manager.store.resolveNotebookRetrievalProfile({ studioId, notebookId, strategy });
  const variant = manager.indexStore.resolveChunkIndexVariant(artifactId, chunkProfile.profileHash);
  return variant ? manager.indexStore.listVariantChunks(variant.id) : [];
}

async function setupReadySource(options: {
  studioId?: string;
  text?: string;
  index?: boolean;
} = {}) {
  const studioId = options.studioId ?? "studio-a";
  const manager = new KnowledgeManager({ lingxiHome: tempHome() });
  managers.push(manager);
  const notebook = manager.createNotebook({ studioId, name: "资料" });
  const imported = await manager.importPastedText({
    studioId,
    notebookId: notebook.id,
    text: options.text ?? [
      "第一章 苹果项目的交付日期是九月十五日。",
      "火星项目的预算是八百万元，负责人是李雷。",
      "蓝山项目仍在风险评估阶段。",
    ].join("\n"),
    displayName: "项目.txt",
  });
  const artifact = options.index === false
    ? null
    : await manager.parseSource({ studioId, sourceId: imported.source.id });
  if (artifact) {
    // 与摄入管线同一身份：按 owning notebook 的生效分块配置建变体（否则工具侧
    // 按笔记本 profile 解析出的变体不存在，读片会显式报 NOT_READY）。
    manager.queryService.indexArtifactForIngestion(studioId, artifact.id, {
      targetChars: manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id }),
    });
    // 统一目录使用摄入阶段登记的笔记本分块身份，样本也完成同一步。
    listProfileChunks(manager, studioId, notebook.id, artifact.id);
  }
  return { manager, studioId, notebook, imported, artifact };
}

/** 为选中 notebooks 创建本轮 TurnScope（绑定主会话）。 */
function createScope(
  manager: KnowledgeManager,
  studioId: string,
  notebookIds: string[],
): KnowledgeTurnScope {
  return manager.createTurnScope({
    studioId,
    sessionPath: MAIN_SESSION_PATH,
    notebookIds,
  });
}

function makeTool(
  manager: KnowledgeManager,
  studioId: string,
  sessionContext: { sessionPath: string | null; scopeOwnerSessionPath: string | null } = MAIN_SESSION,
) {
  return createKnowledgeReadTool({
    getKnowledge: () => manager,
    getStudioId: () => studioId,
    resolveSessionContext: () => sessionContext,
  });
}

function parseResult(result: any) {
  expect(result?.isError).toBeFalsy();
  return JSON.parse(result.content[0].text);
}

describe("knowledge_read 工具（KnowledgeTurnScope 契约）", () => {
  it("按 ordinal 范围读片（1-based、双闭区间、附总 chunk 数与冻结身份）", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource();
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeTool(manager, studioId);
    const artifact = (await manager.parseSource({ studioId, sourceId: imported.source.id }))!;
    const total = listProfileChunks(manager, studioId, notebook.id, artifact.id).length;
    expect(total).toBeGreaterThan(0);

    const payload = parseResult(await tool.execute("call-1", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
      toOrdinal: 1,
    }));
    expect(payload.mode).toBe("ordinal-range");
    expect(payload.totalChunks).toBe(total);
    expect(payload.spans.length).toBeGreaterThan(0);
    expect(payload.requestedRange).toEqual([1, 1]);
    expect(payload.spans[0].text).toContain("苹果项目");
    // 读取锚定 scope 冻结的 snapshot/artifact 身份（§四十三）。
    expect(payload.scopeId).toBe(scope.id);
    expect(payload.parseArtifactId).toBe(artifact.id);
    expect(payload.contentSnapshotId).toBe(scope.sources[0].contentSnapshotId);
    expect(payload.notebookId).toBe(notebook.id);

    const all = parseResult(await tool.execute("call-2", {
      scopeId: scope.id,
      sourceId: imported.source.id,
    }));
    expect(all.spans.length).toBeGreaterThan(0);
    expect(all.spans.every(span => span.citationMarkdown && span.blockId)).toBe(true);
    expect(all.totalChunks).toBe(total);
  });

  it("按 query 检索该源（返回匹配片与 retrievalMode）", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource();
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeTool(manager, studioId);
    const unified = vi.spyOn(manager.searchService, "searchWithEvidence");
    expect("retrieveForArtifacts" in manager.queryService).toBe(false);
    const legacy = vi.fn(() => { throw new Error("不得进入已退役查询"); });
    Object.assign(manager.queryService, { retrieveForArtifacts: legacy });
    const payload = parseResult(await tool.execute("call-1", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      query: "火星 预算",
    }));
    expect(unified.mock.calls[0][0]).toMatchObject({ sourceIds: [imported.source.id], notebookIds: [notebook.id], limit: 12, channel: "hybrid" });
    expect(legacy).not.toHaveBeenCalled();
    expect(payload.mode).toBe("search");
    expect(payload.retrievalMode).toBe("fts");
    expect(payload.spans.length).toBeGreaterThan(0);
    expect(payload.spans.map(span => span.text).join("\n")).toContain("火星");
  });

  it("越界与超额范围显式报错（带合法 ordinal 范围）", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource();
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeTool(manager, studioId);
    const artifact = (await manager.parseSource({ studioId, sourceId: imported.source.id }))!;
    const total = listProfileChunks(manager, studioId, notebook.id, artifact.id).length;

    const outOfBounds = await tool.execute("call-1", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: total + 5,
      toOrdinal: total + 6,
    });
    expect(outOfBounds.isError).toBe(true);
    expect(outOfBounds.content[0].text).toContain(`ordinals 1-${total}`);

    const tooMany = await tool.execute("call-2", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
      toOrdinal: 42,
    });
    expect(tooMany.isError).toBe(true);
    expect(tooMany.content[0].text).toContain("at most 40 chunks");

    const inverted = await tool.execute("call-3", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 3,
      toOrdinal: 1,
    });
    expect(inverted.isError).toBe(true);
    expect(inverted.content[0].text).toContain("toOrdinal must be >= fromOrdinal");
  });

  it("scopeId 缺失 / 伪造 / 已关闭 → KNOWLEDGE_SCOPE_VIOLATION，不回落全 studio 行为", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource();
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeTool(manager, studioId);

    const missing = await tool.execute("call-1", { sourceId: imported.source.id, fromOrdinal: 1 });
    expect(missing.isError).toBe(true);
    expect(missing.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");

    const forged = await tool.execute("call-2", {
      scopeId: "kts_forged",
      sourceId: imported.source.id,
      fromOrdinal: 1,
    });
    expect(forged.isError).toBe(true);
    expect(forged.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");

    manager.closeTurnScope({ scopeId: scope.id });
    const closed = await tool.execute("call-3", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
    });
    expect(closed.isError).toBe(true);
    expect(closed.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");
  });

  it("scope 之外的源与伪造 notebookId 显式拒绝（选中 A，C 同 studio 不可读）", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource();
    // 同 studio 的未选中笔记本 C，持有自己的独有源。
    const notebookC = manager.createNotebook({ studioId, name: "未选中" });
    const importedC = await manager.importPastedText({
      studioId,
      notebookId: notebookC.id,
      text: "C 的私有内容，不应被本轮读取。",
      displayName: "C.txt",
    });
    await manager.parseSource({ studioId, sourceId: importedC.source.id });
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeTool(manager, studioId);

    // C 独有 source：不在 scope 冻结集合内。
    const outsideSource = await tool.execute("call-1", {
      scopeId: scope.id,
      sourceId: importedC.source.id,
      fromOrdinal: 1,
    });
    expect(outsideSource.isError).toBe(true);
    expect(outsideSource.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");

    // 伪造 notebookId=C 读 scope 内的源：C 不在选中集合。
    const forgedNotebook = await tool.execute("call-2", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      notebookId: notebookC.id,
      fromOrdinal: 1,
    });
    expect(forgedNotebook.isError).toBe(true);
    expect(forgedNotebook.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");

    // notebookId 指向选中集合内但不引用该源的笔记本：同样拒绝。
    const notebookB = manager.createNotebook({ studioId, name: "选中但无此源" });
    const scopeAB = createScope(manager, studioId, [notebook.id, notebookB.id]);
    const mismatch = await tool.execute("call-3", {
      scopeId: scopeAB.id,
      sourceId: imported.source.id,
      notebookId: notebookB.id,
      fromOrdinal: 1,
    });
    expect(mismatch.isError).toBe(true);
    expect(mismatch.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");

    // notebookId 正确指向引用该源的选中笔记本时可读。
    const ok = await tool.execute("call-4", {
      scopeId: scopeAB.id,
      sourceId: imported.source.id,
      notebookId: notebook.id,
      fromOrdinal: 1,
    });
    expect(ok.isError).toBeFalsy();
  });

  it("共享源：X 同属于 A 和 C，选中 A 时读 X 合法且按 A 的配置路由", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource();
    const notebookC = manager.createNotebook({ studioId, name: "C" });
    manager.addSourceToNotebook({ studioId, notebookId: notebookC.id, sourceId: imported.source.id });
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeTool(manager, studioId);

    const payload = parseResult(await tool.execute("call-1", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
    }));
    expect(payload.notebookId).toBe(notebook.id);
    // 伪造 notebookId=C（引用该源但未选中）→ 拒绝。
    const forged = await tool.execute("call-2", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      notebookId: notebookC.id,
      fromOrdinal: 1,
    });
    expect(forged.isError).toBe(true);
    expect(forged.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");
  });

  it("跨 session / 跨 studio 的 scope 显式拒绝", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource();
    const scope = createScope(manager, studioId, [notebook.id]);

    // 其他会话持有该 scopeId：会话归属不匹配。
    const otherSession = makeTool(manager, studioId, {
      sessionPath: "/tmp/lingxi-knowledge-read-test/other-session.jsonl",
      scopeOwnerSessionPath: "/tmp/lingxi-knowledge-read-test/other-session.jsonl",
    });
    const crossSession = await otherSession.execute("call-1", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
    });
    expect(crossSession.isError).toBe(true);
    expect(crossSession.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");

    // 其他 studio 的运行时：studio 不匹配（即使 scopeId 真实存在）。
    const otherStudio = makeTool(manager, "studio-b");
    const crossStudio = await otherStudio.execute("call-2", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
    });
    expect(crossStudio.isError).toBe(true);
    expect(crossStudio.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");
  });

  it("subagent 子会话继承父会话 scope（scope 只能缩小）；父会话不符则拒绝", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource();
    const scope = createScope(manager, studioId, [notebook.id]);
    const childSessionPath = "/tmp/lingxi-knowledge-read-test/subagent-child.jsonl";
    // executeIsolated 链路：子会话的 manifest provenance 指向父会话——
    // 这里以 resolveSessionContext 闭包模拟该解析结果（同 agent.ts 接线契约）。
    const subagent = makeTool(manager, studioId, {
      sessionPath: childSessionPath,
      scopeOwnerSessionPath: MAIN_SESSION_PATH,
    });
    const ok = await subagent.execute("call-1", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
    });
    expect(ok.isError).toBeFalsy();

    // 父会话不是 scope 持有会话：拒绝（子会话不得读父 scope 之外的源）。
    const stranger = makeTool(manager, studioId, {
      sessionPath: childSessionPath,
      scopeOwnerSessionPath: "/tmp/lingxi-knowledge-read-test/stranger.jsonl",
    });
    const denied = await stranger.execute("call-2", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
    });
    expect(denied.isError).toBe(true);
    expect(denied.content[0].text).toContain("KNOWLEDGE_SCOPE_VIOLATION");
  });

  it("冻结语义：watcher 产生 V2 后，本轮仍读冻结的 V1 snapshot/artifact（§四十三）", async () => {
    const studioId = "studio-a";
    const manager = new KnowledgeManager({ lingxiHome: tempHome() });
    managers.push(manager);
    const filesDir = tempHome();
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    const filePath = path.join(filesDir, "笔记.md");
    fs.writeFileSync(filePath, "# 第一版\n\n苹果项目九月交付。\n", "utf8");
    const imported = await manager.importFile({ studioId, notebookId: notebook.id, filePath });
    const artifactV1 = await manager.parseSource({ studioId, sourceId: imported.source.id });
    manager.queryService.indexArtifactForIngestion(studioId, artifactV1.id, {
      targetChars: manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id }),
    });
    const scope = createScope(manager, studioId, [notebook.id]);
    expect(scope.sources[0].parseArtifactId).toBe(artifactV1.id);

    // watcher 路径：源文件变化 → 新 snapshot V2 + 新 artifact（旧快照保留）。
    fs.writeFileSync(filePath, "# 第二版\n\n苹果项目十月交付。\n", "utf8");
    const refreshed = await manager.refreshFileSource({
      studioId,
      notebookId: notebook.id,
      sourceId: imported.source.id,
    });
    expect(refreshed.changed).toBe(true);
    expect(refreshed.parseArtifact!.id).not.toBe(artifactV1.id);

    const tool = makeTool(manager, studioId);
    const payload = parseResult(await tool.execute("call-1", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
      toOrdinal: 40,
    }));
    // 本轮仍读冻结的 V1：身份与内容都是旧版本。
    expect(payload.parseArtifactId).toBe(artifactV1.id);
    expect(payload.contentSnapshotId).toBe(scope.sources[0].contentSnapshotId);
    expect(payload.spans.map(chunk => chunk.text).join("\n")).toContain("九月");
    expect(payload.spans.map(chunk => chunk.text).join("\n")).not.toContain("十月");

    // 新一轮 scope 才冻结到 V2。
    const scopeNext = createScope(manager, studioId, [notebook.id]);
    expect(scopeNext.sources[0].parseArtifactId).toBe(refreshed.parseArtifact!.id);
  });

  it("未解析/未就绪的源显式报错而不是返回空", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource({ index: false });
    const scope = createScope(manager, studioId, [notebook.id]);
    const tool = makeTool(manager, studioId);
    const notReady = await tool.execute("call-1", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
    });
    expect(notReady.isError).toBe(true);
    expect(notReady.content[0].text).toContain("KNOWLEDGE_PARSE_NOT_READY");
  });

  it("无 scope 会话上下文的 surface 显式报 KNOWLEDGE_MODEL_UNAVAILABLE（不静默放行）", async () => {
    const { manager, studioId, imported, notebook } = await setupReadySource();
    const scope = createScope(manager, studioId, [notebook.id]);
    // resolveSessionContext 缺省（CLI 等无会话 surface）。
    const tool = createKnowledgeReadTool({
      getKnowledge: () => manager,
      getStudioId: () => studioId,
    });
    const result = await tool.execute("call-1", {
      scopeId: scope.id,
      sourceId: imported.source.id,
      fromOrdinal: 1,
    });
    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("KNOWLEDGE_MODEL_UNAVAILABLE");
  });

  it("Knowledge 不可用时显式报错", async () => {
    const tool = createKnowledgeReadTool({
      getKnowledge: () => null,
      getStudioId: () => "studio-a",
    });
    const result = await tool.execute("call-1", {
      scopeId: "kts_1",
      sourceId: "src_1",
      fromOrdinal: 1,
    });
    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain("unavailable");
  });
});
