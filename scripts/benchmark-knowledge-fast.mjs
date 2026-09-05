import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import crypto from "node:crypto";
import { pathToFileURL } from "node:url";
import { KnowledgeManager } from "../lib/knowledge/knowledge-manager.ts";
import { resolveKnowledgeChunkerConfig } from "../lib/knowledge/chunker.ts";

const STUDIO = "benchmark-studio";
const QUERIES = ["发布审批", "release.config.json", "20260903", "AuroraQuokka", "Risk Controls", "第七章"];

function textAt(index, size) {
  const special = index % 997 === 0 ? " 发布审批 release.config.json 20260903 AuroraQuokka Risk Controls 第七章" : "";
  return `# 章节 ${index % 97}\n干扰资料 ${index} / ${size}：普通流程、随机编号和文件说明。${special}`;
}

function openManager(home, size) {
  const remote = () => { throw new Error("性能测试禁止远程模型调用"); };
  return new KnowledgeManager({ lingxiHome: home, maxImportBytes: size * 1024,
    embedTextsForModel: remote, rerankForModel: remote });
}

async function seed(manager, size) {
  const notebook = manager.createNotebook({ studioId: STUDIO, name: "性能资料" });
  const texts = Array.from({ length: size }, (_, index) => textAt(index, size));
  const imported = await manager.importPastedText({ studioId: STUDIO, notebookId: notebook.id,
    displayName: "性能资料.txt", text: texts.join("\n") });
  const artifact = manager.store.beginParseArtifact({ studioId: STUDIO, contentSnapshotId: imported.snapshot.id,
    parserId: "benchmark-fixed-seed", parserVersion: "1", parserConfigHash: crypto.createHash("sha256").update(String(size)).digest("hex") });
  manager.store.completeParseArtifact({ studioId: STUDIO, parseArtifactId: artifact.id, status: "ready", warnings: [],
    semanticArtifactPath: `artifacts/${artifact.id}.json`, blocks: texts.map((text, ordinal) => ({ ordinal, text,
      locatorType: "text", locator: { headingPath: [`章节 ${ordinal % 97}`] } })) });
  // 每段独立章节，真实 v3 分块仍恰好一段一片；同时建立实际来源/章节投影。
  const blocks = manager.store.listArtifactBlocks({ studioId: STUDIO, parseArtifactId: artifact.id });
  const config = resolveKnowledgeChunkerConfig(blocks);
  manager.store.resolveNotebookRetrievalProfile({ studioId: STUDIO, notebookId: notebook.id, strategy: config.strategy });
  const indexed = manager.queryService.indexArtifactForIngestion(STUDIO, artifact.id);
  if (manager.indexStore.getReadyVariantMetadata({ parseArtifactId: artifact.id, chunkProfileHash: indexed.chunkerConfigId })?.chunkCount !== size) {
    throw new Error("基准生产索引片段数量与固定规模不符");
  }
  return notebook.id;
}

function turnScope(manager, notebookId) {
  return manager.createTurnScope({ studioId: STUDIO, notebookIds: [notebookId], sessionPath: "/benchmark/session.jsonl" });
}

function percentile(values, p) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * p) - 1)];
}

function summarize(samples) {
  const metric = key => ({ P50: percentile(samples.map(row => row[key]), .5),
    P95: percentile(samples.map(row => row[key]), .95), P99: percentile(samples.map(row => row[key]), .99) });
  return { scopeCompileMs: metric("scopeCompileMs"), ftsMs: metric("ftsMs"), spanExtractMs: metric("spanExtractMs"),
    packMs: metric("packMs"), totalMs: metric("totalMs") };
}

async function measure(manager, frozenScope, runs) {
  const samples = [];
  for (let index = 0; index < runs; index++) {
    const result = await manager.runFastKnowledgePipeline({ question: QUERIES[index % QUERIES.length], scope: frozenScope });
    samples.push({ ...result.timings, remoteModelCalls: result.stats.remoteModelCalls,
      returnedSpans: result.stats.injectedChunks, usedTokens: result.stats.usedTokens,
      retrievalResultCacheHit: result.stats.retrievalResultCacheHit === true });
  }
  return { samples, percentiles: summarize(samples), remoteModelCalls: Math.max(...samples.map(x => x.remoteModelCalls)),
    returnedSpans: Math.max(...samples.map(x => x.returnedSpans)), usedTokens: Math.max(...samples.map(x => x.usedTokens)) };
}

export async function runKnowledgeFastBenchmark({ sizes = [10_000, 100_000], hotRuns = 20, coldRuns = 5,
  enforce = process.env.LINGXI_ENFORCE_KNOWLEDGE_PERF === "1", outputPath = null } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-fast-benchmark-"));
  const results = [];
  try {
    for (const size of sizes) {
      const home = path.join(root, String(size));
      let manager = openManager(home, size);
      let notebookId;
      let hot;
      const coldSamples = [];
      try {
        notebookId = await seed(manager, size);
        const frozenScope = turnScope(manager, notebookId);
        // 首次调用只预热，不混入热缓存分位数。
        await measure(manager, frozenScope, 1);
        hot = await measure(manager, frozenScope, hotRuns);
      } finally { await manager.close(); }
      for (let index = 0; index < coldRuns; index++) {
        const start = performance.now();
        manager = openManager(home, size);
        try {
          const measured = await measure(manager, turnScope(manager, notebookId), 1);
          coldSamples.push({ ...measured.samples[0], totalMs: performance.now() - start });
        } finally { await manager.close(); }
      }
      const cold = { samples: coldSamples, percentiles: summarize(coldSamples),
        remoteModelCalls: Math.max(...coldSamples.map(x => x.remoteModelCalls)),
        returnedSpans: Math.max(...coldSamples.map(x => x.returnedSpans)), usedTokens: Math.max(...coldSamples.map(x => x.usedTokens)) };
      results.push({ size, hot, cold });
    }
    const report = { schemaVersion: 2, seed: "lingxi-knowledge-fast-v2-real-three-grain-index", platform: process.platform,
      cpu: os.cpus()[0]?.model ?? null, memoryBytes: os.totalmem(), osRelease: os.release(),
      arch: process.arch, node: process.version, generatedAt: new Date().toISOString(), coldDefinition: "fresh manager, databases, frozen scope and first query; OS page cache is not flushed", results };
    // 即使门禁失败也保留原始测量，供远程工作流上传核查。
    if (outputPath) { fs.mkdirSync(path.dirname(outputPath), { recursive: true }); fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`); }
    if (enforce) {
      for (const row of results) {
        const hotLimit = row.size <= 10_000 ? 800 : 1_200;
        if (row.hot.percentiles.totalMs.P95 > hotLimit) throw new Error(`${row.size} hot P95 ${row.hot.percentiles.totalMs.P95}ms > ${hotLimit}ms`);
        if (row.size >= 100_000 && row.cold.percentiles.totalMs.P95 > 1_500) throw new Error(`${row.size} cold P95 ${row.cold.percentiles.totalMs.P95}ms > 1500ms`);
        for (const sample of [...row.hot.samples, ...row.cold.samples]) {
          if (sample.remoteModelCalls !== 0 || sample.returnedSpans > 8 || sample.usedTokens > 2400) {
            throw new Error(`${row.size} result contract violated`);
          }
        }
      }
    }
    return report;
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const outputArg = process.argv.find(arg => arg.startsWith("--output="));
  const report = await runKnowledgeFastBenchmark({ outputPath: outputArg?.slice("--output=".length) || null,
    sizes: process.argv.includes("--million") ? [10_000, 100_000, 1_000_000] : [10_000, 100_000] });
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
}
