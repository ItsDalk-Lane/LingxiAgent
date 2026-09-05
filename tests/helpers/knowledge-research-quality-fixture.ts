import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import type { KnowledgeResearchProgress } from "../../shared/knowledge-research.ts";
import { estimateTextTokens } from "../../lib/llm/estimate-text-tokens.ts";
import { LingxiEngine } from "../../core/engine.ts";
import { resolveKnowledgeChunkerConfig } from "../../lib/knowledge/chunker.ts";
import { createResearchAgentFixture, type ResearchModelTurn } from "./knowledge-research-agent-fixture.ts";

export interface QualityCorpus {
  question: string;
  notebooks: Array<{ key: string; name: string; selected: boolean;
    sources: Array<{ key: string; name: string; text: string; status?: "needs_ocr" | "failed" }> }>;
}

/** 在既有真实会话与工具夹具上导入指定资料；不写入任何预期需求或证据。 */
export async function createResearchQualityFixture(name: string, driver: (turn: ResearchModelTurn) => Promise<unknown>, suppliedCorpus?: QualityCorpus) {
  const corpus = suppliedCorpus ?? JSON.parse(fs.readFileSync(new URL(`../fixtures/knowledge-research/${name}.json`, import.meta.url), "utf8")) as QualityCorpus;
  const filesRoot = fs.mkdtempSync(path.join(os.tmpdir(), "research-quality-files-"));
  const invocations: Array<{ role: "root" | "worker"; name: string; params: Record<string, unknown>; result: Record<string, unknown>; durationMs: number }> = [];
  const modelTurns: Array<{ role: string; surface: string; durationMs: number; estimatedInputTokens: number }> = [];
  const progress: Array<{ event: KnowledgeResearchProgress; elapsedMs: number }> = [];
  let runStarted = 0;
  const assertionErrors: Error[] = [];
  const f = await createResearchAgentFixture(async turn => {
    const started = performance.now();
    try { return await driver({ ...turn, call: async (toolName, params) => {
      const toolStarted = performance.now();
      const result = await turn.call(toolName, params);
      invocations.push({ role: turn.role, name: toolName, params, result, durationMs: performance.now() - toolStarted });
      return result;
    } }); } catch (error) {
      // 模型异常允许被生产编排收口，测试断言失败则必须交还测试运行器。
      if (error instanceof Error && error.name === "AssertionError") assertionErrors.push(error);
      throw error;
    } finally {
      modelTurns.push({ role: turn.role, surface: String(turn.options.surface), durationMs: performance.now() - started,
        estimatedInputTokens: estimateTextTokens(turn.prompt) });
    }
  }, corpus.question);
  try {
    const studioId = f.request.compiledScope.studioId;
    const notebookIds: string[] = [];
    const sources: Record<string, { sourceId: string; parseArtifactId: string; text: string; selected: boolean }> = {};
    for (const entry of corpus.notebooks) {
      const notebook = f.manager.createNotebook({ studioId, name: entry.name });
      if (entry.selected) notebookIds.push(notebook.id);
      for (const document of entry.sources) {
        const filePath = path.join(filesRoot, `${document.key}.md`);
        fs.writeFileSync(filePath, document.text);
        const imported = await f.manager.importFile({ studioId, notebookId: notebook.id, filePath, displayName: document.name });
        const artifact = await f.manager.parseSource({ studioId, sourceId: imported.source.id });
        const blocks = f.manager.store.listArtifactBlocks({ studioId, parseArtifactId: artifact.id });
        const targetChars = f.manager.getNotebookEffectiveChunkTargetChars({ studioId, notebookId: notebook.id });
        f.manager.store.resolveNotebookRetrievalProfile({ studioId, notebookId: notebook.id,
          strategy: resolveKnowledgeChunkerConfig(blocks, { targetChars }).strategy });
        f.manager.queryService.indexArtifactForIngestion(studioId, artifact.id, { targetChars });
        if (document.status === "needs_ocr") f.manager.store.completeParseArtifact({ studioId, parseArtifactId: artifact.id,
          status: "needs_ocr", blocks: [], warnings: ["needs_ocr"], semanticArtifactPath: `artifacts/${artifact.id}.json` });
        else if (document.status === "failed") f.manager.store.failParseArtifact({ studioId, parseArtifactId: artifact.id });
        sources[document.key] = { sourceId: imported.source.id, parseArtifactId: artifact.id,
          text: document.text, selected: entry.selected };
      }
    }
    const engine = Object.assign(Object.create(LingxiEngine.prototype) as LingxiEngine, {
      _knowledge: f.manager, _runtimeContext: { studioId },
      getSessionIdForPath: (sessionPath: string) => f.manifests.resolveByLocatorPath(sessionPath)?.sessionId ?? null,
      getSessionManifest: (sessionId: string) => f.manifests.getBySessionId(sessionId),
      executeIsolated: f.executeIsolated,
      emitEvent: (event: KnowledgeResearchProgress) => { progress.push({ event, elapsedMs: performance.now() - runStarted }); },
    });
    return { ...f, corpus, sources, invocations, engine, modelTurns, progress,
      run: async () => {
        runStarted = performance.now();
        const result = await engine.buildDetailedKnowledgeResearchContext({ question: corpus.question,
          knowledgeRefs: { mode: "detailed", notebookIds }, sessionId: f.request.parentSessionId,
          sessionPath: f.request.parentSessionPath, agentId: f.request.agentId, turnId: `quality-${name}` })
          .catch(error => { throw assertionErrors[0] ?? error; });
        if (assertionErrors.length) throw assertionErrors[0];
        return result;
      },
      async close() { await f.close(); fs.rmSync(filesRoot, { recursive: true, force: true }); },
    };
  } catch (error) { await f.close(); fs.rmSync(filesRoot, { recursive: true, force: true }); throw error; }
}

/** 遍历实际返回的阅读范围，章节资料也只能使用其中包含的原句。 */
export async function readQualityQuote(turn: ResearchModelTurn, needId: string, sourceId: string,
  quote: string, relation: "supports" | "contradicts" | "context" = "supports", ordinal?: number) {
  const read = await turn.call("knowledge_read", { scopeId: turn.scopeId, sourceId,
    ...(ordinal === undefined ? {} : { fromOrdinal: ordinal, toOrdinal: ordinal }) });
  const span = (read.chunks ?? []).flatMap((chunk: { spans: Array<{ text: string; receiptId: string }> }) => chunk.spans)
    .find((item: { text: string }) => item.text.includes(quote));
  if (!span) throw new Error("质量资料的实际阅读结果不包含目标原句");
  return turn.call("knowledge_research_update", { runId: turn.runId,
    linkEvidence: [{ needId, receiptId: span.receiptId, quote, relation, rationale: "已阅读的冻结资料原句" }] });
}
