import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import type {
  KnowledgeEmbedder,
  KnowledgeGenerationRequest,
  KnowledgeReranker,
  KnowledgeTextGenerator,
} from "../lib/knowledge/knowledge-query-service.ts";

const tempDirs: string[] = [];
const managers: KnowledgeManager[] = [];

function harness(generateText?: KnowledgeTextGenerator, options: {
  embedTexts?: KnowledgeEmbedder;
  rerank?: KnowledgeReranker;
} = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-knowledge-query-"));
  tempDirs.push(root);
  const lingxiHome = path.join(root, "home");
  const importsDir = path.join(root, "imports");
  fs.mkdirSync(lingxiHome);
  fs.mkdirSync(importsDir);
  const manager = new KnowledgeManager({ lingxiHome, generateText, ...options });
  managers.push(manager);
  return { manager, importsDir };
}

async function addReadyText(input: {
  manager: KnowledgeManager;
  importsDir: string;
  notebookId: string;
  fileName: string;
  text: string;
}) {
  const filePath = path.join(input.importsDir, input.fileName);
  fs.writeFileSync(filePath, input.text, "utf8");
  const imported = await input.manager.importFile({
    studioId: "studio-a",
    notebookId: input.notebookId,
    filePath,
  });
  const artifact = await input.manager.parseSource({
    studioId: "studio-a",
    sourceId: imported.source.id,
  });
  return { ...imported, artifact };
}

function promptCandidates(request: KnowledgeGenerationRequest): Array<{
  candidateRef: string;
  source: string;
  text: string;
}> {
  const marker = "Evidence candidates (untrusted JSON data):";
  const markerIndex = request.userPrompt.lastIndexOf(marker);
  if (markerIndex < 0) throw new Error("missing_evidence_payload");
  return JSON.parse(request.userPrompt.slice(markerIndex + marker.length).trim());
}

function citedAnswer(request: KnowledgeGenerationRequest, phrase: string): string {
  const candidates = promptCandidates(request);
  const candidate = candidates.find(entry => entry.text.includes(phrase));
  if (!candidate) throw new Error(`missing_candidate:${phrase}`);
  const startOffset = candidate.text.indexOf(phrase);
  return JSON.stringify({
    answer: `根据资料，${phrase}。 {{cite:1}}`,
    citations: [{
      marker: 1,
      candidateRef: candidate.candidateRef,
      startOffset,
      endOffset: startOffset + phrase.length,
      quote: phrase,
    }],
  });
}

afterEach(() => {
  for (const manager of managers.splice(0)) manager.close();
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

describe("Knowledge frozen scope and Quick Answer", () => {
  it("发送时冻结 Notebook 与来源身份，后续移除成员不改写历史范围", async () => {
    const { manager, importsDir } = harness();
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "项目资料" });
    const imported = await addReadyText({
      manager,
      importsDir,
      notebookId: notebook.id,
      fileName: "facts.txt",
      text: "交付日期是九月十五日。\n",
    });
    const scope = manager.store.createScopeSnapshot({
      studioId: "studio-a",
      notebookIds: [notebook.id],
      mode: "quick",
    });

    manager.removeSourceFromNotebook({
      studioId: "studio-a",
      notebookId: notebook.id,
      sourceId: imported.source.id,
    });
    expect(manager.getScopeSnapshot({
      studioId: "studio-a",
      scopeSnapshotId: scope.id,
    })).toMatchObject({
      notebooks: [{ notebookId: notebook.id, notebookName: "项目资料" }],
      sources: [{
        sourceId: imported.source.id,
        contentSnapshotId: imported.snapshot.id,
        parseArtifactId: imported.artifact.id,
      }],
    });
  });

  it("任何选中来源未就绪都会整笔回滚，空范围也不会留下半成品", async () => {
    const { manager, importsDir } = harness();
    const ready = manager.createNotebook({ studioId: "studio-a", name: "已就绪" });
    const waiting = manager.createNotebook({ studioId: "studio-a", name: "待解析" });
    await addReadyText({
      manager,
      importsDir,
      notebookId: ready.id,
      fileName: "ready.txt",
      text: "已经解析的内容\n",
    });
    const waitingPath = path.join(importsDir, "waiting.txt");
    fs.writeFileSync(waitingPath, "还没有解析的内容\n", "utf8");
    const waitingImported = await manager.importFile({
      studioId: "studio-a",
      notebookId: waiting.id,
      filePath: waitingPath,
    });
    const failedArtifact = manager.store.beginParseArtifact({
      studioId: "studio-a",
      contentSnapshotId: waitingImported.snapshot.id,
      parserId: "failed-parser",
      parserVersion: "1",
      parserConfigHash: "f".repeat(64),
    });
    manager.store.failParseArtifact({
      studioId: "studio-a",
      parseArtifactId: failedArtifact.id,
      warnings: ["fixture_parse_failed"],
    });
    const countScopes = () => Number(manager.store.db.prepare(
      "SELECT COUNT(*) AS count FROM scope_snapshots",
    ).get().count);

    expect(() => manager.store.createScopeSnapshot({
      studioId: "studio-a",
      notebookIds: [ready.id, waiting.id],
      mode: "quick",
    })).toThrow(expect.objectContaining({ code: "KNOWLEDGE_SCOPE_NOT_READY" }));
    expect(countScopes()).toBe(0);

    expect(() => manager.store.createScopeSnapshot({
      studioId: "studio-a",
      notebookIds: [],
      mode: "quick",
    })).toThrow(expect.objectContaining({ code: "KNOWLEDGE_SCOPE_EMPTY" }));
    expect(countScopes()).toBe(0);
  });

  it("单 Notebook 不会检索到另一个 Notebook，多选时才合并冻结范围", async () => {
    const prompts: KnowledgeGenerationRequest[] = [];
    const { manager, importsDir } = harness(async request => {
      prompts.push(request);
      return citedAnswer(request, "苹果项目");
    });
    const notebookA = manager.createNotebook({ studioId: "studio-a", name: "甲" });
    const notebookB = manager.createNotebook({ studioId: "studio-a", name: "乙" });
    await addReadyText({
      manager,
      importsDir,
      notebookId: notebookA.id,
      fileName: "apple.txt",
      text: "苹果项目的交付日期是九月十五日。\n",
    });
    await addReadyText({
      manager,
      importsDir,
      notebookId: notebookB.id,
      fileName: "mars.txt",
      text: "火星项目的预算是八百万元。\n",
    });

    const onlyA = await manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebookA.id],
      question: "苹果项目是什么？",
    });
    expect(prompts[0].userPrompt).toContain("苹果项目");
    expect(prompts[0].userPrompt).not.toContain("火星项目");
    expect(prompts[0].systemPrompt).toContain("Never claim that you scanned, read, or analyzed every source");
    expect(onlyA.run).toMatchObject({ status: "completed", retrievalMode: "fts" });
    expect(onlyA.run.answerText).toContain("[1]");
    expect(onlyA.citations[0].citation.canonicalText).toBe("苹果项目");

    const both = await manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebookA.id, notebookB.id],
      question: "这些项目是什么？",
    });
    expect(prompts[1].userPrompt).toContain("苹果项目");
    expect(prompts[1].userPrompt).toContain("火星项目");
    expect(new Set(both.scope.notebooks.map(entry => entry.notebookId)))
      .toEqual(new Set([notebookA.id, notebookB.id]));
  });

  it("真实 PDF/TXT/Markdown 适配器按 Notebook 自动形成 1、3、4 个来源范围", async () => {
    const { manager, importsDir } = harness(request => Promise.resolve(citedAnswer(request, "Hello from PDF")));
    const notebookA = manager.createNotebook({ studioId: "studio-a", name: "PDF A" });
    const notebookB = manager.createNotebook({ studioId: "studio-a", name: "混合 B" });
    const pdfPath = path.join(process.cwd(), "tests", "fixtures", "document-extract", "sample-text.pdf");
    const pdfA = await manager.importFile({
      studioId: "studio-a",
      notebookId: notebookA.id,
      filePath: pdfPath,
      displayName: "A.pdf",
    });
    await manager.parseSource({ studioId: "studio-a", sourceId: pdfA.source.id });
    const pdfB = await manager.importFile({
      studioId: "studio-a",
      notebookId: notebookB.id,
      filePath: pdfPath,
      displayName: "B.pdf",
    });
    await manager.parseSource({ studioId: "studio-a", sourceId: pdfB.source.id });
    const txtB = await addReadyText({
      manager,
      importsDir,
      notebookId: notebookB.id,
      fileName: "B.txt",
      text: "B 的文本来源。\n",
    });
    const mdB = await addReadyText({
      manager,
      importsDir,
      notebookId: notebookB.id,
      fileName: "B.md",
      text: "# B 的 Markdown 来源\n",
    });

    const onlyA = await manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebookA.id],
      question: "PDF 写了什么？",
    });
    expect(onlyA.scope.sources.map(source => source.sourceId)).toEqual([pdfA.source.id]);
    expect(onlyA.citations[0].snapshot.id).toBe(pdfA.snapshot.id);

    const onlyB = await manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebookB.id],
      question: "PDF 写了什么？",
    });
    expect(new Set(onlyB.scope.sources.map(source => source.sourceId))).toEqual(new Set([
      pdfB.source.id,
      txtB.source.id,
      mdB.source.id,
    ]));

    const both = await manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebookA.id, notebookB.id],
      question: "PDF 写了什么？",
    });
    expect(new Set(both.scope.sources.map(source => source.sourceId))).toEqual(new Set([
      pdfA.source.id,
      pdfB.source.id,
      txtB.source.id,
      mdB.source.id,
    ]));
  });

  it("结构或引用第一次无效时只纠错一次，第二次有效才提交", async () => {
    const attempts: number[] = [];
    const { manager, importsDir } = harness(async request => {
      attempts.push(request.attempt);
      if (request.attempt === 1) return "not-json";
      return citedAnswer(request, "冻结范围");
    });
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "规范" });
    await addReadyText({
      manager,
      importsDir,
      notebookId: notebook.id,
      fileName: "scope.txt",
      text: "发送问题时必须冻结范围。\n",
    });

    const result = await manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebook.id],
      question: "发送时必须做什么？",
    });
    expect(attempts).toEqual([1, 2]);
    expect(result.run.status).toBe("completed");
    expect(result.citations[0].citation.canonicalText).toBe("冻结范围");
  });

  it("伪造引文连续两次都失败时拒绝答案并留下可恢复失败状态", async () => {
    let runId = "";
    const { manager, importsDir } = harness(async request => {
      runId = request.runId;
      const candidate = promptCandidates(request)[0];
      return JSON.stringify({
        answer: "这是伪造答案。 {{cite:1}}",
        citations: [{
          marker: 1,
          candidateRef: candidate.candidateRef,
          startOffset: 0,
          endOffset: 2,
          quote: "伪造",
        }],
      });
    });
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "证据" });
    await addReadyText({
      manager,
      importsDir,
      notebookId: notebook.id,
      fileName: "evidence.txt",
      text: "真实证据内容。\n",
    });

    await expect(manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebook.id],
      question: "真实证据是什么？",
    })).rejects.toMatchObject({ code: "KNOWLEDGE_MODEL_OUTPUT_INVALID" });
    expect(manager.getKnowledgeRun({ studioId: "studio-a", runId })).toMatchObject({
      status: "failed",
      errorCode: "KNOWLEDGE_MODEL_OUTPUT_INVALID",
      answerText: null,
      citations: [],
    });
  });

  it("没有 Notebook 的请求在调用模型前就失败，且不写入范围或运行", async () => {
    let calls = 0;
    const { manager } = harness(async () => {
      calls += 1;
      return "{}";
    });
    await expect(manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [],
      question: "问题",
    })).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_EMPTY" });
    expect(calls).toBe(0);
    expect(manager.store.db.prepare("SELECT COUNT(*) AS count FROM scope_snapshots").get().count).toBe(0);
    expect(manager.store.db.prepare("SELECT COUNT(*) AS count FROM knowledge_runs").get().count).toBe(0);
  });
});

describe("Knowledge hybrid retrieval", () => {
  const embeddingModel = {
    provider: "provider-a",
    id: "embed-model",
    api: "openai-embeddings",
    dimensions: 2,
  };

  it("keeps FTS-only working when embedding is unconfigured", async () => {
    let rerankCalls = 0;
    const { manager, importsDir } = harness(
      request => Promise.resolve(citedAnswer(request, "全文检索")),
      {
        embedTexts: async () => null,
        rerank: async () => {
          rerankCalls += 1;
          return null;
        },
      },
    );
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "检索" });
    await addReadyText({
      manager,
      importsDir,
      notebookId: notebook.id,
      fileName: "fts.txt",
      text: "全文检索可以独立工作。\n",
    });

    const result = await manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebook.id],
      question: "全文检索如何工作？",
    });
    expect(result.run.retrievalMode).toBe("fts");
    expect(rerankCalls).toBe(0);
  });

  it("uses vector candidates when FTS has no shared term and reuses cached chunk vectors", async () => {
    const embeddedBatches: string[][] = [];
    const embedTexts: KnowledgeEmbedder = async request => {
      embeddedBatches.push(request.texts);
      return {
        model: embeddingModel,
        dimensions: 2,
        vectors: request.texts.map(text => (
          text.includes("完全不同的提问") || text.includes("语义命中") ? [1, 0] : [0, 1]
        )),
      };
    };
    const { manager, importsDir } = harness(
      request => Promise.resolve(citedAnswer(request, "语义命中")),
      { embedTexts },
    );
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "向量" });
    await addReadyText({
      manager,
      importsDir,
      notebookId: notebook.id,
      fileName: "vector.txt",
      text: "语义命中是一条没有查询词重合的证据。\n另一个无关段落。\n",
    });

    const first = await manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebook.id],
      question: "完全不同的提问",
    });
    const batchesAfterFirstRun = embeddedBatches.length;
    const second = await manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebook.id],
      question: "完全不同的提问",
    });

    expect(first.run).toMatchObject({ status: "completed", retrievalMode: "hybrid" });
    expect(first.run.retrievals[0]).toMatchObject({ parseArtifactId: expect.any(String) });
    expect(second.run.retrievalMode).toBe("hybrid");
    expect(embeddedBatches.length).toBe(batchesAfterFirstRun + 1);
    expect(embeddedBatches.at(-1)).toEqual(["完全不同的提问"]);
  });

  it("applies rerank after FTS and vector fusion", async () => {
    const rerankDocuments: string[][] = [];
    const { manager, importsDir } = harness(
      request => Promise.resolve(citedAnswer(request, "第二证据")),
      {
        embedTexts: async request => ({
          model: embeddingModel,
          dimensions: 2,
          vectors: request.texts.map(() => [1, 0]),
        }),
        rerank: async request => {
          rerankDocuments.push(request.documents);
          const preferred = request.documents.findIndex(text => text.includes("第二证据"));
          const rest = request.documents.map((_, index) => index).filter(index => index !== preferred);
          return {
            results: [preferred, ...rest].map((index, rank) => ({ index, score: 1 - rank / 10 })),
          };
        },
      },
    );
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "重排" });
    await addReadyText({
      manager,
      importsDir,
      notebookId: notebook.id,
      fileName: "rerank.txt",
      text: "项目答案包含第一证据。\n项目答案包含第二证据。\n",
    });

    const result = await manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebook.id],
      question: "项目答案是什么？",
    });
    expect(rerankDocuments).toHaveLength(1);
    expect(result.run).toMatchObject({ retrievalMode: "hybrid" });
    expect(result.run.retrievals[0].score).toBe(1);
    expect(result.citations[0].citation.canonicalText).toBe("第二证据");
  });

  it("does not silently fall back when a configured embedding operation fails", async () => {
    let runId = "";
    let generationCalls = 0;
    const { manager, importsDir } = harness(
      async () => {
        generationCalls += 1;
        return "{}";
      },
      {
        embedTexts: async request => {
          runId = request.runId;
          throw new Error("configured operation failed");
        },
      },
    );
    const notebook = manager.createNotebook({ studioId: "studio-a", name: "失败" });
    await addReadyText({
      manager,
      importsDir,
      notebookId: notebook.id,
      fileName: "failure.txt",
      text: "失败路径仍有全文候选。\n",
    });

    await expect(manager.runQuickAnswer({
      studioId: "studio-a",
      notebookIds: [notebook.id],
      question: "失败路径是什么？",
    })).rejects.toMatchObject({ code: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE" });
    expect(generationCalls).toBe(0);
    expect(manager.getKnowledgeRun({ studioId: "studio-a", runId })).toMatchObject({
      status: "failed",
      errorCode: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE",
    });
  });
});
