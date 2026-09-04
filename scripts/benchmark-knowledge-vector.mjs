import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import crypto from "node:crypto";
import { performance } from "node:perf_hooks";
import { pathToFileURL } from "node:url";
import { PortableVectorIndexAdapter, knowledgeChunkIndexVariantId } from "../lib/knowledge/vector-index-adapter.ts";
import { createKnowledgeVectorSearchBackend } from "../lib/knowledge/vector-search-backend-factory.ts";
import { searchVectorBackend } from "../lib/knowledge/vector-search-backend.ts";

const DIMENSIONS = 64;
const LATENT_DIMENSIONS = 16;
const SEED = 0x1a2b3c4d;
const TOP_K = 10;
function randomGenerator(seed) {
  let state = seed >>> 0;
  return () => { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; return state / 4294967296; };
}
function vectorGenerator(seed) {
  const random = randomGenerator(seed);
  const projectionRandom = randomGenerator(SEED);
  const projection = Array.from({ length: DIMENSIONS }, () => Array.from({ length: LATENT_DIMENSIONS }, () => projectionRandom() * 2 - 1));
  return () => {
    const latent = Array.from({ length: LATENT_DIMENSIONS }, () => random() * 2 - 1);
    const vector = projection.map(row => row.reduce((sum, value, index) => sum + value * latent[index], 0));
    const norm = Math.hypot(...vector);
    return vector.map(value => Math.fround(value / norm));
  };
}
function percentiles(samples) {
  const ordered = [...samples].sort((a, b) => a - b);
  return { P50: ordered[Math.ceil(ordered.length * .5) - 1], P95: ordered[Math.ceil(ordered.length * .95) - 1],
    P99: ordered[Math.ceil(ordered.length * .99) - 1] };
}
function filesSize(root) {
  return fs.readdirSync(root, { recursive: true }).filter(name => name.endsWith(".usearch"))
    .reduce((total, name) => total + fs.statSync(path.join(root, name)).size, 0);
}

export async function runKnowledgeVectorBenchmark({ sizes = [10_000, 100_000], runs = 40, coldRuns = 3,
  enforce = process.env.LINGXI_ENFORCE_KNOWLEDGE_PERF === "1", outputPath = null } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-vector-benchmark-"));
  const results = [];
  try {
    for (const size of sizes) {
      const home = path.join(root, String(size));
      fs.mkdirSync(home);
      const portable = new PortableVectorIndexAdapter({ dbPath: path.join(home, "knowledge-vector.db") });
      const model = { provider: "benchmark", modelId: "fixed-projection-64", protocol: "fixture", dimensions: DIMENSIONS,
        key: crypto.createHash("sha256").update("knowledge-vector-benchmark-v1").digest("hex") };
      let backend;
      try {
        // 合成相关向量：16 维固定种子投影到 64 维；不声称代表真实模型的召回率。
        const next = vectorGenerator(SEED + 1);
        const { vectorIndexVariantId } = portable.buildOrReplaceArtifact({ parseArtifactId: "benchmark",
          chunkIndexVariantId: knowledgeChunkIndexVariantId("benchmark", "fixed"), model, chunkFingerprint: `fixed-${size}`,
          entries: Array.from({ length: size }, (_, ordinal) => ({ parseArtifactId: "benchmark", chunkId: `chunk-${ordinal}`,
            ordinal, vector: next() })) });
        const buildStart = performance.now();
        backend = createKnowledgeVectorSearchBackend({ indexesRoot: home, portable });
        await backend.whenIdle();
        const indexBuildMs = performance.now() - buildStart;
        const indexFileBytes = filesSize(path.join(home, "knowledge-ann"));
        const queryNext = vectorGenerator(SEED + 2);
        const queries = Array.from({ length: runs }, () => queryNext());
        const input = queryVector => ({ vectorIndexVariantIds: [vectorIndexVariantId], model, queryVector, limit: TOP_K });
        const cold = [];
        for (let run = 0; run < coldRuns; run++) {
          await backend.close();
          const start = performance.now();
          backend = createKnowledgeVectorSearchBackend({ indexesRoot: home, portable });
          const found = await searchVectorBackend(backend, input(queries[run % runs]));
          cold.push(performance.now() - start);
          assert.equal(found.vectorBackend, "hnsw", `cold search fell back: ${found.degradedReasons}`);
        }
        const samples = [];
        for (const query of queries) {
          const started = performance.now();
          const exact = portable.search(input(query));
          const exactMs = performance.now() - started;
          const annStart = performance.now();
          const approximate = await searchVectorBackend(backend, input(query));
          const hnswMs = performance.now() - annStart;
          assert.equal(approximate.vectorBackend, "hnsw", `warm search fell back: ${approximate.degradedReasons}`);
          assert.equal(exact.length, TOP_K); assert.equal(approximate.results.length, TOP_K);
          const ids = new Set(exact.map(result => result.chunkId));
          // 固定小章节只允许读取这组片段，记录补查耗时并复核返回身份没有越界。
          const sectionChunkIds = Array.from({ length: Math.min(size, 64) }, (_, index) => `chunk-${index}`);
          const sectionStart = performance.now();
          const sectionResults = portable.search({ ...input(query), chunkIds: sectionChunkIds });
          const sectionExactMs = performance.now() - sectionStart;
          assert.ok(sectionResults.every(result => sectionChunkIds.includes(result.chunkId)));
          const overlap = approximate.results.filter(result => ids.has(result.chunkId)).length / TOP_K;
          samples.push({ exactMs, hnswMs, overlap, sectionExactMs });
        }
        const exactMs = percentiles(samples.map(sample => sample.exactMs));
        const hnswMs = percentiles(samples.map(sample => sample.hnswMs));
        results.push({ size, indexBuildMs, indexFileBytes, coldLoadMs: percentiles(cold), coldSamplesMs: cold,
          exactMs, hnswMs, sectionExactMs: percentiles(samples.map(sample => sample.sectionExactMs)), sectionChunkCount: Math.min(size, 64),
          warmSearchMs: hnswMs, speedupP95: exactMs.P95 / hnswMs.P95,
          topKOverlap: samples.reduce((total, sample) => total + sample.overlap, 0) / samples.length, samples });
      } finally { if (backend) await backend.close(); portable.close(); }
    }
    const report = { schemaVersion: 2, seed: SEED, dimensions: DIMENSIONS, latentDimensions: LATENT_DIMENSIONS, topK: TOP_K,
      cpu: os.cpus()[0]?.model ?? null, memoryBytes: os.totalmem(), osRelease: os.release(),
      dataset: "fixed uniform latent vectors projected and normalized; independent queries; synthetic, not provider embeddings",
      coldDefinition: "new backend and first native graph load plus query; portable DB stays open; OS cache is not flushed",
      platform: process.platform, arch: process.arch, node: process.version, generatedAt: new Date().toISOString(),
      wallClockGatesEnforced: enforce, results };
    if (outputPath) { fs.mkdirSync(path.dirname(outputPath), { recursive: true }); fs.writeFileSync(outputPath, JSON.stringify(report, null, 2) + "\n"); }
    for (const result of results) {
      assert.ok(result.topKOverlap >= .95, `${result.size} top-k overlap ${result.topKOverlap} < .95`);
      if (enforce && result.size >= 100_000) {
        assert.ok(result.hnswMs.P95 <= 500, `HNSW P95 ${result.hnswMs.P95}ms > 500ms`);
        assert.ok(result.speedupP95 >= 5, `HNSW speedup ${result.speedupP95} < 5`);
      }
    }
    return report;
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const outputArg = process.argv.find(arg => arg.startsWith("--output="));
  console.log(JSON.stringify(await runKnowledgeVectorBenchmark({ outputPath: outputArg?.slice("--output=".length) || null }), null, 2));
}
