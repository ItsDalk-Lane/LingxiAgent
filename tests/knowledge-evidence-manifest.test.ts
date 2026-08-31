/**
 * EvidenceManifest 轻量持久化（任务书 §六十七，schema v15）：
 * - 普通轮写入：injector 真跑（真实 KnowledgeManager 检索链路）→ 身份链
 *   round-trip——由 manifest 能还原该轮读的是哪个 snapshot/variant/chunks
 *   （含 block spans 与 [KN] 引用标签；hybrid 轮带向量变体身份）；
 * - 服务端复核：scope 外 source / 冻结 snapshot 不一致 → 显式拒绝；
 * - GC/deleteSource：被 manifest 引用的源跳过/拒绝（manifest 无 TTL 前全部保留）；
 * - 写入失败不阻断会话提交（desktop-session-submit 显式 warn）。
 * 只存身份链与标签：断言 manifest 序列化结果不含 chunk 正文/模型输出。
 */
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { isKnowledgeError } from "../lib/knowledge/errors.ts";
import { resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";
import { knowledgeChunkIndexVariantId } from "../lib/knowledge/knowledge-index-store.ts";
import { knowledgeVectorIndexVariantId } from "../lib/knowledge/vector-index-adapter.ts";
import { KnowledgeManager, type KnowledgeManagerOptions } from "../lib/knowledge/knowledge-manager.ts";
import { KnowledgeStore } from "../lib/knowledge/knowledge-store.ts";
import {
  buildKnowledgeContextInjection,
  type KnowledgeInjectionEvidence,
} from "../lib/knowledge/knowledge-context-injector.ts";
import type { KnowledgeCoveragePlan } from "../lib/knowledge/knowledge-coverage-planner.ts";
import type { RetrieveForNotebooksResult } from "../lib/knowledge/knowledge-query-service.ts";
import type { KnowledgeBlockDraft } from "../lib/knowledge/source-adapters.ts";
import { LingxiEngine } from "../core/engine.ts";
import { submitDesktopSessionMessage } from "../core/desktop-session-submit.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];
const stores: KnowledgeStore[] = [];
const FAKE_MODEL_REF = { id: "emb-1", provider: "fake" };

function tempHome(prefix: string) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const store of stores.splice(0)) store.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

/** 真实 manager（fake 嵌入 8 维，hybrid 检索可跑通，向量变体真实就位）。 */
function createManager(options: Partial<KnowledgeManagerOptions> = {}) {
  const manager = new KnowledgeManager({
    lingxiHome: tempHome("lingxi-evidence-manifest-"),
    embedTextsForModel: async ({ texts }) => ({
      vectors: texts.map(text => {
        const vector = new Array(8).fill(0);
        vector[text.length % 8] = (text.length % 7) + 1;
        return vector;
      }),
      dimensions: 8,
      model: { provider: "fake", id: "emb-1", api: "openai", dimensions: 8 },
    }),
    canEmbedWithModel: () => true,
    ...options,
  });
  managers.push(manager);
  return manager;
}

async function importTextSource(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  text: string,
) {
  const imported = await manager.importPastedText({ studioId, notebookId, text, displayName: "证据源.txt" });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  manager.enqueueSourceIngestion({ studioId, notebookId, sourceId: imported.source.id, artifactId: artifact.id });
  await manager.ingestion.drainQueue();
  return { imported, artifact };
}

/** 与 engine.buildKnowledgeContextInjection 同构的注入接线（真实检索门面 + 冻结集合）。 */
async function injectWithManager(manager: KnowledgeManager, input: {
  studioId: string;
  scopeId: string;
  notebookIds: string[];
  question: string;
}) {
  const scope = manager.getTurnScope({ scopeId: input.scopeId })!;
  const frozenArtifacts = new Map(scope.sources.map(source => [source.sourceId, {
    contentSnapshotId: source.contentSnapshotId,
    parseArtifactId: source.parseArtifactId,
  }]));
  return await buildKnowledgeContextInjection({
    question: input.question,
    mode: "detailed",
    scopeId: input.scopeId,
    deps: {
      decomposeModel: null,
      retrieve: ({ query }) => manager.queryService.retrieveForNotebooks({
        studioId: input.studioId,
        notebookIds: input.notebookIds,
        question: query,
        frozenArtifacts,
      }),
      readNeighborChunks: ({ anchor, ordinals }) => manager.queryService.readAdjacentChunks({
        studioId: input.studioId,
        anchor: {
          notebookId: anchor.notebookId,
          notebookName: anchor.notebookName,
          sourceId: anchor.sourceId,
          sourceName: anchor.sourceName,
          parseArtifactId: anchor.parseArtifactId,
          chunkIndexVariantId: anchor.chunkIndexVariantId,
        },
        ordinals,
      }),
    },
  });
}

function chunkProfileHashOf(manager: KnowledgeManager, studioId: string, artifactId: string) {
  const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifactId });
  // 与生命周期测试同一口径：嵌入上下文未接线时自动分块回退 8192×0.8=6553
  // （摄入侧与查询侧同源解析，见 resolveEffectiveChunkTargetChars）。
  return resolveKnowledgeChunkerConfig(blocks, { targetChars: 6553 }).configId;
}

/** engine 门面方法（只依赖 _knowledge/_runtimeContext）：stub this 直接绑定测试。 */
function engineRecordManifest(
  manager: KnowledgeManager,
  studioId: string,
  input: Parameters<LingxiEngine["recordKnowledgeEvidenceManifest"]>[0],
) {
  return LingxiEngine.prototype.recordKnowledgeEvidenceManifest.call(
    { _knowledge: manager, _runtimeContext: { studioId } } as unknown as LingxiEngine,
    input,
  );
}

// ── 普通轮：身份链完整 round-trip ─────────────────────────────────────

describe("EvidenceManifest：普通轮写入（round-trip）", () => {
  it("injector 真跑 → manifest 还原该轮读取的 snapshot/variant/chunks/spans/标签；不含正文", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const text = "苹果项目的交付日期是九月十五日。\n火星项目的预算是八百万元。\n金星项目的负责人尚未确定。";
    const { imported, artifact } = await importTextSource(manager, studioId, notebook.id, text);
    const snapshotId = manager.store.getLatestContentSnapshotForSource({
      studioId, sourceId: imported.source.id,
    }).id;

    const scope = manager.createTurnScope({
      studioId,
      sessionPath: "/sessions/evman-1.jsonl",
      turnId: "turn-evman-1",
      notebookIds: [notebook.id],
    });
    const { block, stats, evidence } = await injectWithManager(manager, {
      studioId,
      scopeId: scope.id,
      notebookIds: [notebook.id],
      question: "苹果项目什么时候交付？",
    });

    // 注入真实发生（hybrid 检索命中且注入），stats 带 scopeId。
    expect(block).toContain("[KnowledgeContext]");
    expect(stats.scopeId).toBe(scope.id);
    expect(stats.retrievalMode).toBe("hybrid");
    expect(stats.injectedChunks).toBeGreaterThan(0);
    expect(evidence.entries.length).toBeGreaterThan(0);

    // engine 门面写入（服务端复核 + 落库）。
    engineRecordManifest(manager, studioId, {
      sessionPath: "/sessions/evman-1.jsonl",
      stats,
      evidence,
    });

    const manifest = manager.getEvidenceManifestByScope({ scopeId: scope.id })!;
    expect(manifest).not.toBeNull();
    expect(manifest.turnScopeId).toBe(scope.id);
    expect(manifest.turnId).toBe("turn-evman-1");
    expect(manifest.sessionPath).toBe("/sessions/evman-1.jsonl");
    expect(manifest.notebookIds).toEqual([notebook.id]);
    // 普通轮未接 coverage planner：档位空（不伪造）。
    expect(manifest.coverageMode).toBeNull();
    expect(manifest.executedCoverageMode).toBeNull();
    expect(manifest.coverageRunId).toBeNull();
    // byTurn 同一 manifest。
    expect(manager.getEvidenceManifestByTurn({ turnId: "turn-evman-1" })!.id).toBe(manifest.id);

    // ── 身份链还原 ──
    expect(manifest.entries.length).toBe(1);
    const entry = manifest.entries[0];
    const profileHash = chunkProfileHashOf(manager, studioId, artifact.id);
    const civId = knowledgeChunkIndexVariantId(artifact.id, profileHash);
    expect(entry.sourceId).toBe(imported.source.id);
    expect(entry.contentSnapshotId).toBe(snapshotId);
    expect(entry.parseArtifactId).toBe(artifact.id);
    expect(entry.chunkProfileHash).toBe(profileHash);
    expect(entry.chunkIndexVariantId).toBe(civId);
    // hybrid 轮：向量变体身份 = f(civ, modelKey)，可由 manifest 复原
    //（modelKey = sha256([provider, modelId, protocol, dimensions])，与写入侧同源）。
    const modelKey = crypto.createHash("sha256")
      .update(JSON.stringify(["fake", "emb-1", "openai", 8]), "utf8")
      .digest("hex");
    expect(entry.vectorIndexVariantIds).toEqual([
      knowledgeVectorIndexVariantId(civId, modelKey),
    ]);

    // chunk ids ⊆ 该变体真实 chunk 集；由 manifest 可回读原文（版本还原）。
    const variantChunks = manager.indexStore.listVariantChunks(civId);
    const variantChunkIds = new Set(variantChunks.map(chunk => chunk.id));
    for (const chunkId of entry.chunkIds) expect(variantChunkIds.has(chunkId)).toBe(true);
    expect(entry.chunkIds.length).toBeGreaterThan(0);
    // block spans 指向真实 knowledge_blocks 行（blockId 可回查）。
    const blocks = manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
    const blockIds = new Set(blocks.map(item => item.id));
    for (const spanGroup of entry.blockSpans) {
      expect(entry.chunkIds.includes(spanGroup.chunkId) || entry.neighborChunkIds.includes(spanGroup.chunkId)).toBe(true);
      expect(spanGroup.spans.length).toBeGreaterThan(0);
      for (const span of spanGroup.spans) expect(blockIds.has(span.blockId)).toBe(true);
    }
    // 引用标签与注入块 [KN] 编号一致。
    expect(entry.citationLabels.length).toBeGreaterThan(0);
    for (const label of entry.citationLabels) expect(label).toMatch(/^K\d+$/);
    const blockNumbers = entry.citationLabels.map(label => Number(label.slice(1)));
    expect(blockNumbers).toEqual([...blockNumbers].sort((left, right) => left - right));

    // 只存身份链：manifest 序列化不含 chunk 正文（红线：不存正文/模型输出）。
    const serialized = JSON.stringify(manifest);
    expect(serialized).not.toContain("苹果项目的交付日期");
    expect(serialized).not.toContain("金星项目的负责人");
    // 身份字段白名单之外无自由文本载荷（无 text/body/statement 字段）。
    for (const entry of manifest.entries) {
      for (const spanGroup of entry.blockSpans) {
        for (const span of spanGroup.spans) {
          expect(Object.keys(span).sort()).toEqual([
            "blockEndOffset", "blockId", "blockStartOffset", "chunkEndOffset", "chunkStartOffset",
          ]);
        }
      }
    }
  });

  it("邻接扩展块进 neighborChunkIds（context-only，不计检索命中）", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const lines = Array.from({ length: 8 }, (_, index) => `第${index}行事实：内容各不相同编号${index}。`);
    const { imported } = await importTextSource(manager, studioId, notebook.id, lines.join("\n"));
    const scope = manager.createTurnScope({
      studioId,
      sessionPath: "/sessions/evman-2.jsonl",
      turnId: "turn-evman-2",
      notebookIds: [notebook.id],
    });
    const { stats, evidence } = await injectWithManager(manager, {
      studioId,
      scopeId: scope.id,
      notebookIds: [notebook.id],
      question: "第3行事实的内容是什么？",
    });
    engineRecordManifest(manager, studioId, {
      sessionPath: "/sessions/evman-2.jsonl",
      stats,
      evidence,
    });
    const manifest = manager.getEvidenceManifestByScope({ scopeId: scope.id })!;
    const entry = manifest.entries.find(item => item.sourceId === imported.source.id)!;
    // 邻接块（若发生）与锚点不重叠，且都可在变体 chunk 集内还原。
    const anchors = new Set(entry.chunkIds);
    for (const neighborId of entry.neighborChunkIds) expect(anchors.has(neighborId)).toBe(false);
    if (stats.neighborExpansionCount && stats.neighborExpansionCount > 0) {
      expect(entry.neighborChunkIds.length).toBeGreaterThan(0);
    }
    const civId = entry.chunkIndexVariantId!;
    const variantChunkIds = new Set(manager.indexStore.listVariantChunks(civId).map(chunk => chunk.id));
    for (const chunkId of [...entry.chunkIds, ...entry.neighborChunkIds]) {
      expect(variantChunkIds.has(chunkId)).toBe(true);
    }
  });

  it("滚动注入轮：manifest 记全部部分条目（模型读过即入身份链；标签为全局 [KN]）", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const text = Array.from({ length: 8 }, (_, index) => `第${index}条长事实：${"滚动证据正文。".repeat(90)}`).join("\n");
    const { imported, artifact } = await importTextSource(manager, studioId, notebook.id, text);
    const scope = manager.createTurnScope({
      studioId,
      sessionPath: "/sessions/evman-4.jsonl",
      turnId: "turn-evman-4",
      notebookIds: [notebook.id],
    });
    const scopeFrozen = manager.getTurnScope({ scopeId: scope.id })!;
    const frozenArtifacts = new Map(scopeFrozen.sources.map(source => [source.sourceId, {
      contentSnapshotId: source.contentSnapshotId,
      parseArtifactId: source.parseArtifactId,
    }]));
    const { stats, evidence } = await buildKnowledgeContextInjection({
      question: "全部长事实的要点是什么？",
      mode: "detailed",
      scopeId: scope.id,
      budgetTokens: 2_000,
      deps: {
        decomposeModel: null,
        rollupModel: async ({ round }) => `第${round}部分中间笔记。`,
        retrieve: ({ query }) => manager.queryService.retrieveForNotebooks({
          studioId,
          notebookIds: [notebook.id],
          question: query,
          frozenArtifacts,
        }),
      },
    });
    // 超预算触发滚动注入（蒸馏路径已移除）。
    expect(stats.rollup).toBeDefined();
    expect((stats.rollup?.parts ?? 0)).toBeGreaterThanOrEqual(2);
    expect(evidence.entries.length).toBeGreaterThan(0);

    engineRecordManifest(manager, studioId, {
      sessionPath: "/sessions/evman-4.jsonl",
      stats,
      evidence,
    });
    const manifest = manager.getEvidenceManifestByScope({ scopeId: scope.id })!;
    const entry = manifest.entries.find(item => item.sourceId === imported.source.id)!;
    // 全部部分条目都进身份链（中间轮 + 最终轮模型都读过）。
    expect(entry.chunkIds.length).toBeGreaterThan(0);
    expect(entry.neighborChunkIds).toEqual([]);
    expect(entry.parseArtifactId).toBe(artifact.id);
    // 标签为跨部分全局编号（K1 起、连续）。
    expect(entry.citationLabels).toContain("K1");
    // manifest 可还原：chunk ids 全部在该变体内。
    const variantChunks = new Set(manager.indexStore.listVariantChunks(entry.chunkIndexVariantId!).map(chunk => chunk.id));
    for (const chunkId of entry.chunkIds) expect(variantChunks.has(chunkId)).toBe(true);
  });

  it("服务端复核：scope 外 source / 冻结 snapshot 不一致 → 显式拒绝，不落库", async () => {
    const manager = createManager();
    const studioId = "studio-a";
    const notebookA = manager.createNotebook({ studioId, name: "A" });
    const notebookB = manager.createNotebook({ studioId, name: "B" });
    const { imported, artifact } = await importTextSource(manager, studioId, notebookA.id, "A 本事实。");
    const { imported: importedB } = await importTextSource(manager, studioId, notebookB.id, "B 本事实。");
    void artifact;
    const scope = manager.createTurnScope({
      studioId,
      sessionPath: "/sessions/evman-3.jsonl",
      turnId: "turn-evman-3",
      notebookIds: [notebookA.id],
    });
    const snapshotB = manager.store.getLatestContentSnapshotForSource({
      studioId, sourceId: importedB.source.id,
    });
    const civ = "civ_" + "0".repeat(32);
    // source 不在冻结集合。
    expect(() => manager.insertEvidenceManifest({
      turnScopeId: scope.id,
      entries: [{
        ordinal: 0,
        sourceId: importedB.source.id,
        contentSnapshotId: snapshotB.id,
        parseArtifactId: null,
        chunkProfileHash: null,
        chunkIndexVariantId: null,
        vectorIndexVariantIds: [],
        chunkIds: [],
        neighborChunkIds: [],
        blockSpans: [],
        citationLabels: [],
      }],
    })).toThrow(/outside the frozen turn scope/);
    // snapshot 与冻结行不一致。
    expect(() => manager.insertEvidenceManifest({
      turnScopeId: scope.id,
      entries: [{
        ordinal: 0,
        sourceId: imported.source.id,
        contentSnapshotId: snapshotB.id,
        parseArtifactId: null,
        chunkProfileHash: null,
        chunkIndexVariantId: civ,
        vectorIndexVariantIds: [],
        chunkIds: [],
        neighborChunkIds: [],
        blockSpans: [],
        citationLabels: [],
      }],
    })).toThrow(/does not match the frozen scope snapshot/);
    // 均未落库。
    expect(manager.getEvidenceManifestByScope({ scopeId: scope.id })).toBeNull();
  });
});


// ── GC / deleteSource：manifest 引用保护 ─────────────────────────────

describe("EvidenceManifest：GC 与 deleteSource 引用保护", () => {
  it("orphan GC 遇 manifest 引用跳过（evidence-manifest-referenced）；无引用源照常清理", async () => {
    const manager = createManager({ orphanRetentionMs: 1 });
    const studioId = "studio-gc";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    manager.updateNotebookSettings({ studioId, notebookId: notebook.id, embeddingModelRef: FAKE_MODEL_REF });
    const { imported: withManifest } = await importTextSource(manager, studioId, notebook.id, "被 manifest 引用的事实。");
    const { imported: withoutManifest } = await importTextSource(manager, studioId, notebook.id, "无 manifest 引用的事实。");

    // 只有源 A 产生 manifest（条目级引用：身份链记录了 A 的冻结版本）。
    const scopeA = manager.createTurnScope({
      studioId,
      sessionPath: "/sessions/evman-gc.jsonl",
      turnId: "turn-evman-gc",
      notebookIds: [notebook.id],
    });
    const frozenA = scopeA.sources.find(source => source.sourceId === withManifest.source.id)!;
    manager.insertEvidenceManifest({
      turnScopeId: scopeA.id,
      entries: [{
        ordinal: 0,
        sourceId: withManifest.source.id,
        contentSnapshotId: frozenA.contentSnapshotId,
        parseArtifactId: frozenA.parseArtifactId,
        chunkProfileHash: null,
        chunkIndexVariantId: null,
        vectorIndexVariantIds: [],
        chunkIds: [],
        neighborChunkIds: [],
        blockSpans: [],
        citationLabels: [],
      }],
    });
    // 关闭 scope（隔离 active-turn-scope 干扰，单测 manifest 引用检查本身；
    // closed scope 行保留——manifest 的关联仍指向它）。
    manager.closeTurnScope({ scopeId: scopeA.id });

    manager.removeSourceFromNotebook({ studioId, notebookId: notebook.id, sourceId: withManifest.source.id });
    manager.removeSourceFromNotebook({ studioId, notebookId: notebook.id, sourceId: withoutManifest.source.id });
    await new Promise(resolve => setTimeout(resolve, 5));

    const report = manager.runOrphanSourceGc();
    expect(report.skipped).toEqual([
      { sourceId: withManifest.source.id, reason: "evidence-manifest-referenced" },
    ]);
    expect(report.purged).toEqual([withoutManifest.source.id]);
    // 被引用源物理痕迹保留。
    expect(manager.store.db.prepare(`SELECT COUNT(*) AS count FROM sources WHERE id = ?`).get(withManifest.source.id))
      .toMatchObject({ count: 1 });
  });

  it("deleteSource 遇 manifest 引用显式 409 拒绝；无引用源照常删除", async () => {
    const manager = createManager();
    const studioId = "studio-del";
    const notebook = manager.createNotebook({ studioId, name: "资料" });
    const { imported: withManifest } = await importTextSource(manager, studioId, notebook.id, "被引用源事实。");
    const { imported: withoutManifest } = await importTextSource(manager, studioId, notebook.id, "可删除源事实。");
    const scope = manager.createTurnScope({
      studioId,
      sessionPath: "/sessions/evman-del.jsonl",
      turnId: "turn-evman-del",
      notebookIds: [notebook.id],
    });
    const frozen = scope.sources.find(source => source.sourceId === withManifest.source.id)!;
    manager.insertEvidenceManifest({
      turnScopeId: scope.id,
      entries: [{
        ordinal: 0,
        sourceId: withManifest.source.id,
        contentSnapshotId: frozen.contentSnapshotId,
        parseArtifactId: frozen.parseArtifactId,
        chunkProfileHash: null,
        chunkIndexVariantId: null,
        vectorIndexVariantIds: [],
        chunkIds: [],
        neighborChunkIds: [],
        blockSpans: [],
        citationLabels: [],
      }],
    });
    // 关闭 scope（排除 active-turn-scope 干扰，隔离 manifest 检查）。
    manager.closeTurnScope({ scopeId: scope.id });

    await expect(manager.deleteSource({ studioId, sourceId: withManifest.source.id }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_CONFLICT" });
    expect(isKnowledgeError(
      await manager.deleteSource({ studioId, sourceId: withManifest.source.id }).catch(error => error),
    )).toBe(true);
    // 拒绝路径不产生任何清理。
    expect(manager.getSource({ studioId, sourceId: withManifest.source.id }).deletedAt).toBeNull();

    const deleted = await manager.deleteSource({ studioId, sourceId: withoutManifest.source.id });
    expect(deleted.source.deletedAt).not.toBeNull();
  });
});

// ── 写入失败不阻断提交 ───────────────────────────────────────────────

describe("EvidenceManifest：写入失败不阻断会话提交", () => {
  it("engine 门面抛错 → 提交照常完成并显式 warn；无 scopeId 不调用门面", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      const session = {
        subscribe: () => () => {},
        prompt: async () => {},
        model: null,
        sessionManager: { appendCustomEntry: vi.fn() },
      };
      const stats = {
        mode: "detailed" as const,
        scopeId: "kts_scope-1",
        retrievalMode: "hybrid" as const,
        subQueries: [],
        subQueryHits: [],
        degraded: false,
        fusedChunks: 1,
        injectedChunks: 1,
        truncated: false,
        usedTokens: 10,
        budgetTokens: 6000,
      };
      const evidence: KnowledgeInjectionEvidence = { entries: [], searchedVectorVariants: [] };
      const recordManifest = vi.fn(() => {
        throw new Error("KNOWLEDGE_CONFLICT: injected failure");
      });
      const engine = {
        ensureSessionLoaded: vi.fn(async () => session),
        promptSession: vi.fn(async (...args: any[]) => { void args; }),
        emitEvent: vi.fn(),
        setUiContext: vi.fn(),
        buildKnowledgeContextInjection: vi.fn(async () => ({
          block: "[KnowledgeContext]\ninjected\n[/KnowledgeContext]",
          stats,
          evidence,
        })),
        recordKnowledgeEvidenceManifest: recordManifest,
      };

      const result = await submitDesktopSessionMessage(engine, {
        sessionPath: "/tmp/evman-desk.jsonl",
        text: "苹果什么时候交付",
        displayMessage: { text: "苹果什么时候交付" },
        knowledgeRefs: { notebookIds: ["nb-1"], mode: "detailed" },
      });

      // 提交完成（不因 manifest 写入失败阻断）。
      expect(result.text).toBeNull();
      expect(engine.promptSession).toHaveBeenCalled();
      // manifest 持久化被调用过一次（prompt 路径），失败转为显式 warn。
      expect(recordManifest).toHaveBeenCalledTimes(1);
      expect(warn).toHaveBeenCalledWith(
        expect.stringContaining("knowledge evidence manifest write failed for /tmp/evman-desk.jsonl"),
      );
      // 用户消息照常投影（stats 仍在）。
      const userMessage = engine.emitEvent.mock.calls
        .find(([event]) => event?.type === "session_user_message")?.[0].message;
      expect(userMessage.knowledgeRetrieval).toBe(stats);

      // 无 scopeId（降级/旧调用方）：不调用门面、不告警。
      warn.mockClear();
      const recordNoScope = vi.fn();
      const engineNoScope = {
        ...engine,
        recordKnowledgeEvidenceManifest: recordNoScope,
        buildKnowledgeContextInjection: vi.fn(async () => ({
          block: "[KnowledgeContext]\ninjected\n[/KnowledgeContext]",
          stats: { ...stats, scopeId: undefined },
          evidence,
        })),
      };
      await submitDesktopSessionMessage(engineNoScope, {
        sessionPath: "/tmp/evman-desk2.jsonl",
        text: "无 scope 追问",
        displayMessage: { text: "无 scope 追问" },
        knowledgeRefs: { notebookIds: ["nb-1"], mode: "detailed" },
      });
      expect(recordNoScope).not.toHaveBeenCalled();
      expect(warn).not.toHaveBeenCalledWith(expect.stringContaining("evidence manifest"));
    } finally {
      warn.mockRestore();
    }
  });
});
