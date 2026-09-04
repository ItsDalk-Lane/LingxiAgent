import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";

/**
 * Phase 2（§十一/§十二/§八十九.4-6）Query/Index 分离契约：
 * - ④ 查询只读：ChunkProfile / ChunkIndexVariant / VectorIndexVariant 集合不变；
 * - ⑤ 变体缺失：不现场建 chunk、不批量嵌入，KNOWLEDGE_INDEX_MISSING 降级留痕 + 幂等入队；
 * - ⑥ 非 ready 源显式留痕：needs_ocr / 解析在途 / 解析失败分类降级，终态不自动重建；
 * - 自愈（§十三）：KNOWLEDGE_INDEX_INVALID 损坏 → reset + 降级留痕 + 入队，后台重建恢复。
 */
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function tempHome() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-query-separation-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

const FAKE_MODEL_REF = { id: "emb-1", provider: "fake" };

/** 8 维确定性伪嵌入：记录每次调用的文本批，供"是否批量嵌入"断言。 */
function createManager(lingxiHome: string) {
  const embedCalls: string[][] = [];
  const manager = new KnowledgeManager({
    lingxiHome,
    embedTextsForModel: async (request) => {
      embedCalls.push([...request.texts]);
      return {
        vectors: request.texts.map((text) => {
          const vector = new Array(8).fill(0);
          vector[text.length % 8] = (text.length % 7) + 1;
          return vector;
        }),
        dimensions: 8,
        model: { provider: "fake", id: "emb-1", api: "openai", dimensions: 8 },
      };
    },
    canEmbedWithModel: () => true,
  });
  managers.push(manager);
  return { manager, embedCalls };
}

function novelText(chapterCount: number): string {
  const chapters: string[] = [];
  for (let index = 1; index <= chapterCount; index += 1) {
    const paragraph = "末日之后的城市在长夜里延伸，幸存者提着灯穿过废墟。".repeat(64);
    chapters.push(`第${index}章 长夜\n\n${paragraph}`);
  }
  return chapters.join("\n\n");
}

async function importAndParse(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  displayName: string,
) {
  const imported = await manager.importPastedText({
    studioId,
    notebookId,
    text: novelText(6),
    displayName,
  });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  return { imported, artifact };
}

/** 完整摄入（导入 → 解析 → 入队 → drain）。 */
async function ingestSource(
  manager: KnowledgeManager,
  studioId: string,
  notebookId: string,
  displayName = "小说.txt",
) {
  const { imported, artifact } = await importAndParse(manager, studioId, notebookId, displayName);
  manager.enqueueSourceIngestion({
    studioId,
    notebookId,
    sourceId: imported.source.id,
    artifactId: artifact.id,
  });
  await manager.ingestion.drainQueue();
  return { imported, artifact };
}

/**
 * §八十九.4 的快照面：查询不得改变的三张索引/配置表的身份集合。
 * 不含时间戳与计数列——身份集合不变即"查询未触碰索引构建面"。
 */
function captureIndexState(manager: KnowledgeManager) {
  const chunkProfiles = manager.store.db
    .prepare("SELECT id, profile_hash FROM chunk_profiles ORDER BY id")
    .all();
  const chunkIndexVariants = manager.indexStore.db
    .prepare(
      "SELECT id, parse_artifact_id, chunk_profile_hash, status, block_fingerprint"
      + " FROM chunk_index_variants ORDER BY id",
    )
    .all();
  const vectorIndexVariants = manager.vectorIndex.db
    .prepare("SELECT id, chunk_index_variant_id, model_key, status FROM vector_index_variants ORDER BY id")
    .all();
  const chunkCount = manager.indexStore.db
    .prepare("SELECT COUNT(*) AS count FROM knowledge_chunks")
    .get().count;
  const vectorCount = manager.vectorIndex.db
    .prepare("SELECT COUNT(*) AS count FROM chunk_vectors")
    .get().count;
  return { chunkProfiles, chunkIndexVariants, vectorIndexVariants, chunkCount, vectorCount };
}

function activeIngestionJobs(manager: KnowledgeManager, studioId: string, notebookId: string) {
  return manager.store.listIngestionJobs({
    studioId,
    notebookId,
    statuses: ["queued", "running", "pending_embedding"],
  });
}

describe("Phase 2 查询/索引分离", () => {
  it("④ 查询只读：ChunkProfile/ChunkIndexVariant/VectorIndexVariant 集合查询前后不变（含变体缺失场景）", async () => {
    const { manager } = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "小说" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 5000,
    });
    await ingestSource(manager, studioId, notebook.id, "甲.txt");

    const beforeQuery = captureIndexState(manager);
    expect(beforeQuery.chunkIndexVariants.length).toBeGreaterThan(0);
    expect(beforeQuery.vectorIndexVariants.length).toBeGreaterThan(0);

    const readyResult = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "幸存者在做什么",
    });
    expect(readyResult.degraded).toEqual([]);
    expect(captureIndexState(manager)).toEqual(beforeQuery);

    // 变体缺失场景：第二源只导入解析、不摄入。查询该 scope 降级但同样不写索引面。
    const second = await importAndParse(manager, studioId, notebook.id, "乙.txt");
    const beforeMissingQuery = captureIndexState(manager);
    expect(beforeMissingQuery).toEqual(beforeQuery); // 解析本身也不触碰索引面

    const missingResult = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "长夜",
    });
    expect(missingResult.degraded.some(entry => (
      entry.reason === "KNOWLEDGE_INDEX_MISSING" && entry.parseArtifactId === second.artifact.id
    ))).toBe(true);
    expect(captureIndexState(manager)).toEqual(beforeMissingQuery);
  });

  it("⑤ 首次查询（导入解析后未摄入）：不建 chunk、零嵌入，INDEX_MISSING 留痕 + 幂等入队后台构建", async () => {
    const { manager, embedCalls } = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "小说" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 5000,
    });
    const { imported, artifact } = await importAndParse(manager, studioId, notebook.id, "新书.txt");

    const first = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "幸存者",
    });
    // 变体缺失：无任何就绪 scope → 不执行查询嵌入（零嵌入调用），显式降级留痕。
    expect(embedCalls.length).toBe(0);
    expect(first.candidates).toEqual([]);
    expect(first.retrievalMode).toBe("fts");
    expect(first.retrievalModeRequested).toBe("hybrid"); // 笔记本配置了嵌入模型
    expect(first.degraded).toHaveLength(1);
    expect(first.degraded[0]).toMatchObject({
      reason: "KNOWLEDGE_INDEX_MISSING",
      parseArtifactId: artifact.id,
      notebookId: notebook.id,
      notebookName: "小说",
      sourceId: imported.source.id,
      sourceName: "新书.txt",
    });
    // 未建立任何 chunk 变体。
    expect(manager.indexStore.db.prepare(
      "SELECT COUNT(*) AS count FROM chunk_index_variants WHERE parse_artifact_id = ?",
    ).get(artifact.id).count).toBe(0);
    // 查询已返回仍未入队；下一轮事件循环才执行后台配置解析和持久化。
    expect(activeIngestionJobs(manager, studioId, notebook.id)).toHaveLength(0);
    await new Promise<void>(resolve => setImmediate(resolve));
    // 幂等入队：恰一个活跃后台构建 job。
    expect(activeIngestionJobs(manager, studioId, notebook.id)).toHaveLength(1);

    // 重复查询不重复排队（活跃 job 去重），降级留痕依旧。
    const second = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "长夜",
    });
    expect(second.degraded.some(entry => entry.reason === "KNOWLEDGE_INDEX_MISSING")).toBe(true);
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(activeIngestionJobs(manager, studioId, notebook.id)).toHaveLength(1);
    expect(embedCalls.length).toBe(0);

    // 后台构建完成后恢复 hybrid，降级清单清空。
    await manager.ingestion.drainQueue();
    const rebuilt = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "幸存者",
    });
    expect(rebuilt.retrievalMode).toBe("hybrid");
    expect(rebuilt.degraded).toEqual([]);
    expect(rebuilt.candidates.length).toBeGreaterThan(0);
  });

  it("⑥ 非 ready 源显式留痕：needs_ocr / 解析在途 / 解析失败分类降级，终态不自动重建", async () => {
    const { manager } = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "混合" });
    // 不配置嵌入模型：retrievalModeRequested 如实 "fts"。

    // needs_ocr：扫描 PDF 无文本层，解析终态。
    const scanned = await manager.importFile({
      studioId,
      notebookId: notebook.id,
      filePath: path.join(ROOT, "tests", "fixtures", "document-extract", "sample-scanned.pdf"),
    });
    const scannedArtifact = await manager.parseSource({ studioId, sourceId: scanned.source.id });
    expect(scannedArtifact.status).toBe("needs_ocr");

    // 解析在途：只导入、未解析。
    const pending = await manager.importPastedText({
      studioId,
      notebookId: notebook.id,
      text: novelText(2),
      displayName: "待解析.txt",
    });

    // 解析失败：显式终态（此处直接置位，等价于解析器报永久错误后的落库状态）。
    const failed = await importAndParse(manager, studioId, notebook.id, "失败.txt");
    manager.store.db.prepare("UPDATE parse_artifacts SET status = 'failed' WHERE id = ?")
      .run(failed.artifact.id);

    const result = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "幸存者",
    });
    expect(result.retrievalModeRequested).toBe("fts");
    expect(result.candidates).toEqual([]);
    const bySource = new Map(result.degraded.map(entry => [entry.sourceId, entry]));
    expect(bySource.get(scanned.source.id)).toMatchObject({
      reason: "KNOWLEDGE_SOURCE_NEEDS_OCR",
      parseArtifactId: scannedArtifact.id,
      notebookId: notebook.id,
    });
    expect(bySource.get(pending.source.id)).toMatchObject({
      reason: "KNOWLEDGE_INDEX_BUILDING",
      detail: "parse pending",
    });
    expect(bySource.get(failed.imported.source.id)).toMatchObject({
      reason: "KNOWLEDGE_INDEX_FAILED",
      parseArtifactId: failed.artifact.id,
    });
    // 三类均不自动入队：needs_ocr/解析在途由各自链路负责，INDEX_FAILED 是显式终态
    // （不自动重试，UI 手动 reingest）。
    expect(manager.store.listIngestionJobs({ studioId, notebookId: notebook.id })).toEqual([]);
  });

  it("自愈：FTS 索引损坏 → 查询不抛，reset + INDEX_MISSING 留痕 + 入队，后台重建后恢复（§十三）", async () => {
    const { manager } = createManager(tempHome());
    const studioId = "studio-a";
    const notebook = manager.createNotebook({ studioId, name: "小说" });
    manager.updateNotebookSettings({
      studioId,
      notebookId: notebook.id,
      embeddingModelRef: FAKE_MODEL_REF,
      chunkTargetChars: 5000,
    });
    const { artifact } = await ingestSource(manager, studioId, notebook.id);
    const healthy = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "幸存者",
    });
    expect(healthy.retrievalMode).toBe("hybrid");
    expect(healthy.candidates.length).toBeGreaterThan(0);

    // 制造 chunk 行损坏（spans_json 非法）：索引是缓存，查询触发自愈而非抛错。
    manager.indexStore.db.prepare("UPDATE knowledge_chunks SET spans_json = 'broken'").run();

    const recovered = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "幸存者",
    });
    expect(recovered.candidates).toEqual([]);
    expect(recovered.retrievalMode).toBe("fts");
    expect(recovered.retrievalModeRequested).toBe("hybrid");
    expect(recovered.degraded).toHaveLength(1);
    expect(recovered.degraded[0]).toMatchObject({
      reason: "KNOWLEDGE_INDEX_MISSING",
      parseArtifactId: artifact.id,
      notebookId: notebook.id,
    });
    expect(recovered.degraded[0].detail).toContain("index reset after corruption");
    // reset 后变体表清空；查询返回后再异步入队，不等待实际构建。
    expect(manager.indexStore.db.prepare("SELECT COUNT(*) AS count FROM chunk_index_variants").get().count).toBe(0);
    expect(activeIngestionJobs(manager, studioId, notebook.id)).toHaveLength(0);
    await new Promise<void>(resolve => setImmediate(resolve));
    expect(activeIngestionJobs(manager, studioId, notebook.id)).toHaveLength(1);

    // 后台摄入从 Block 事实重建索引，查询恢复。
    await manager.ingestion.drainQueue();
    const rebuilt = await manager.queryService.retrieveForNotebooks({
      studioId,
      notebookIds: [notebook.id],
      question: "幸存者",
    });
    expect(rebuilt.retrievalMode).toBe("hybrid");
    expect(rebuilt.degraded).toEqual([]);
    expect(rebuilt.candidates.length).toBeGreaterThan(0);
  });
});
