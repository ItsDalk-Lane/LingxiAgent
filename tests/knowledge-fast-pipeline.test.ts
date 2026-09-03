import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import {
  FastKnowledgePipeline,
  KNOWLEDGE_FAST_FTS_CANDIDATE_LIMIT,
  KNOWLEDGE_FAST_MAX_EVIDENCE_SPANS,
  KNOWLEDGE_FAST_PER_SPAN_MAX_TOKENS,
  KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS,
  KNOWLEDGE_FAST_TOTAL_DEADLINE_MS,
} from "../lib/knowledge/fast-knowledge-pipeline.ts";

const managers: KnowledgeManager[] = [];
const homes: string[] = [];
const studioId = "fast-pipeline-studio";
afterEach(() => {
  vi.restoreAllMocks();
  for (const manager of managers.splice(0)) manager.close();
  for (const home of homes.splice(0)) fs.rmSync(home, { recursive: true, force: true });
});

async function fixture() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-fast-pipeline-"));
  homes.push(home);
  const remote = vi.fn(() => { throw new Error("本地检索不得调用远程服务"); });
  const manager = new KnowledgeManager({ lingxiHome: home, embedTextsForModel: remote, rerankForModel: remote });
  managers.push(manager);
  const notebook = manager.createNotebook({ studioId, name: "本地资料" });
  const imported = await manager.importPastedText({
    studioId, notebookId: notebook.id, displayName: "发布规范.txt", text: "发布配置 release.config.json，版本号 12345，批准发布后归档。",
  });
  const artifact = await manager.parseSource({ studioId, sourceId: imported.source.id });
  manager.enqueueSourceIngestion({ studioId, notebookId: notebook.id, sourceId: imported.source.id, artifactId: artifact.id });
  await manager.ingestion.drainQueue();
  const scope = manager.createTurnScope({ studioId, sessionPath: "/tmp/fast-session.jsonl", notebookIds: [notebook.id] });
  const compiledScope = await manager.compileTurnScope(scope);
  return { manager, scope, compiledScope, remote };
}

describe("快速路径真实本地 FTS", () => {
  it.each(["发布", "release.config.json", "12345"])("中文、配置键和数字命中：%s", async query => {
    const { manager, compiledScope, remote } = await fixture();
    const hits = manager.queryService.searchCompiledScopeFts({ compiledScope, query, limit: 24 });
    expect(hits.length).toBeGreaterThan(0);
    expect(hits[0].text).toContain(query);
    expect(remote).not.toHaveBeenCalled();
  });

  it("重复索引 ID 不造成重复命中，SQL 限额和稳定次序有效", async () => {
    const { manager, compiledScope } = await fixture();
    const input = { chunkIndexVariantIds: [...compiledScope.readyChunkVariantIds, ...compiledScope.readyChunkVariantIds], query: "发布", limit: 1 };
    const hits = manager.indexStore.searchReadyVariantIds(input);
    expect(hits).toHaveLength(1);
    expect(manager.indexStore.searchReadyVariantIds(input)).toEqual(hits);
    expect(new Set(hits.map(hit => hit.id)).size).toBe(hits.length);
  });

  it("不访问未选变体，非 ready 变体不参与检索，字面查询不解释 FTS 指令", async () => {
    const { manager, compiledScope } = await fixture();
    expect(manager.indexStore.searchReadyVariantIds({ chunkIndexVariantIds: ["missing"], query: "发布", limit: 24 })).toEqual([]);
    expect(() => manager.queryService.searchCompiledScopeFts({ compiledScope, query: '" OR * NOT foo', limit: 24 })).not.toThrow();
    manager.indexStore.db.prepare("UPDATE chunk_index_variants SET status = 'building'").run();
    expect(manager.queryService.searchCompiledScopeFts({ compiledScope, query: "发布", limit: 24 })).toEqual([]);
  });
});

describe("快速管线阶段准入", () => {
  it("固定预算与任务书一致", () => {
    expect([KNOWLEDGE_FAST_TOTAL_DEADLINE_MS, KNOWLEDGE_FAST_FTS_CANDIDATE_LIMIT,
      KNOWLEDGE_FAST_MAX_EVIDENCE_SPANS, KNOWLEDGE_FAST_PER_SPAN_MAX_TOKENS, KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS])
      .toEqual([1200, 24, 8, 320, 2400]);
  });

  it("无结果明确说明缺少知识证据，不启动证据加工或任何远程增强", async () => {
    const { manager, scope, remote } = await fixture();
    const extractSpans = vi.fn(() => { throw new Error("零命中不能读取证据"); });
    const packEvidence = vi.fn(() => { throw new Error("零证据不能打包"); });
    const pipeline = manager.createFastKnowledgePipeline({ extractSpans, packEvidence });
    const result = await pipeline.run({ question: "zzzxxyynotpresent", scope });
    expect(result.block).toContain("本地快速检索未找到匹配证据");
    expect(result.stats).toMatchObject({ executionPath: "fast_local", ftsQueries: 1, remoteModelCalls: 0,
      vectorQueries: 0, rerankCalls: 0, injectedChunks: 0 });
    expect(extractSpans).not.toHaveBeenCalled();
    expect(packEvidence).not.toHaveBeenCalled();
    expect(remote).not.toHaveBeenCalled();
  });

  it("编译达到期限，不启动 FTS", async () => {
    const { manager, scope } = await fixture();
    let elapsed = 0;
    const search = vi.fn(() => { throw new Error("过期不得查询"); });
    const pipeline = new FastKnowledgePipeline({
      now: () => elapsed,
      compile: async input => { const compiled = await manager.compileTurnScope(input); elapsed = 1200; return compiled; },
      search,
      extractSpans: () => { throw new Error("过期不得提取"); },
      packEvidence: () => { throw new Error("过期不得打包"); },
    });
    const result = await pipeline.run({ question: "发布", scope });
    expect(result.stats).toMatchObject({ deadlineExceeded: true, ftsQueries: 0 });
    expect(search).not.toHaveBeenCalled();
  });

  it("同步 FTS 自身越过期限时保留已查候选，但不再启动提取或打包", async () => {
    const { manager, scope } = await fixture();
    let elapsed = 0;
    const extractSpans = vi.fn(() => { throw new Error("过期不得提取"); });
    const pipeline = new FastKnowledgePipeline({
      now: () => elapsed,
      compile: input => manager.compileTurnScope(input),
      search: input => { const hits = manager.queryService.searchCompiledScopeFts(input); elapsed = 1500; return hits; },
      extractSpans,
      packEvidence: () => { throw new Error("过期不得打包"); },
    });
    const result = await pipeline.run({ question: "发布", scope });
    expect(result.stats).toMatchObject({ deadlineExceeded: true, ftsQueries: 1, injectedChunks: 0 });
    expect(result.candidates.length).toBeGreaterThan(0);
    expect(result.evidence.entries).toEqual([]);
    expect(result.block).toContain("Deadline exceeded: yes");
    expect(extractSpans).not.toHaveBeenCalled();
  });

  it("查询后取消立即拒绝，既不提取证据也不产生可提交结果", async () => {
    const { manager, scope } = await fixture();
    const controller = new AbortController();
    const extractSpans = vi.fn(() => { throw new Error("取消后不得提取"); });
    const pipeline = new FastKnowledgePipeline({
      compile: input => manager.compileTurnScope(input),
      search: input => { const hits = manager.queryService.searchCompiledScopeFts(input); controller.abort(); return hits; },
      extractSpans,
      packEvidence: () => { throw new Error("取消后不得打包"); },
    });
    await expect(pipeline.run({ question: "发布", scope, signal: controller.signal })).rejects.toMatchObject({ name: "AbortError" });
    expect(extractSpans).not.toHaveBeenCalled();
  });

  it("入场前已取消不读取范围", async () => {
    const { scope } = await fixture();
    const controller = new AbortController();
    controller.abort();
    const compile = vi.fn(() => { throw new Error("已取消不得编译"); });
    const pipeline = new FastKnowledgePipeline({ compile,
      search: () => { throw new Error("已取消不得查询"); },
      extractSpans: () => { throw new Error("已取消不得提取"); },
      packEvidence: () => { throw new Error("已取消不得打包"); },
    });
    await expect(pipeline.run({ question: "发布", scope, signal: controller.signal })).rejects.toMatchObject({ name: "AbortError" });
    expect(compile).not.toHaveBeenCalled();
  });
});
