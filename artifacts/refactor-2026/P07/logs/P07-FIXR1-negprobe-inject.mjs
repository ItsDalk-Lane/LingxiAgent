#!/usr/bin/env node
// P07-FIXR1 负向注入副本（仅作断言有效性证明的运行证据，非交付工具）：在修复版基础上仅禁用外部追加。
/**
 * P07-T06 长运行资源 soak（BENCHMARK_PROTOCOL W5 峰值/稳态扩展）。
 *
 * 固定负载循环 N 个 batch，每批驱动真实组件的完整生命周期：
 *   1. session-stream-store：20 个会话 ×（begin → 7,000 事件 append（饱和 5,000
 *      ring 上限，触发真实 trim）→ resume 校验 → finish 清场）；
 *   2. TerminalSessionManager + terminal-ws-bridge（真实实现 + fake backend，与
 *      scripts/benchmark-terminal-ui.mjs 同构）：12 终端 start/输出/exit；
 *   3. HistoryDirectoryCache（真实实现 + 默认预算）：12 个会话目录构建/翻页，
 *      超过 maxSessions=8 触发 LRU 淘汰（12>8 槽颠簸：主循环每批全量重建）；
 *      每批对驻留文件外部追加一行并断言 probe 不再判 "valid"（外部追加实际判定
 *      append_candidate，目录保留非失效；读路径仅 valid 直接命中，其余走增量/
 *      全量重建。P07-FIXR1/F-C：旧断言 !== "fresh" 恒真，已废弃）。
 * 进程级每批采样：rss / heapUsed / external / arrayBuffers、getActiveResourcesInfo
 * 计数（定时器/句柄泄漏面）、磁盘目录字节。记录 GC 前数值为主（不用一次强制 GC
 * 后的数字当稳态；--expose-gc 时附带 GC 后读数单列）。
 *
 * 稳态判定：预热 2 批后，取第 3–5 批均值 vs 最后 3 批均值；rss/heap 增幅 ≤25%
 * 且无逐批单调递增 >5% → 无界增长未复现。
 *
 * 用法：node --expose-gc bench-soak-resources.mjs --batches 12
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import console from "node:console";
import { setTimeout } from "node:timers";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");

const argv = process.argv.slice(2);
let batches = 12;
let outPath = path.join(REPO, "artifacts", "refactor-2026", "P07", "samples", "soak-resources.json");
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--batches") batches = Number(argv[++i]);
  else if (argv[i] === "--out") outPath = argv[++i];
}

const { createSessionStreamState, beginSessionStream, appendSessionStreamEvent, finishSessionStream, resumeSessionStream } =
  await import(`${REPO}/server/session-stream-store.ts`);
const { TerminalSessionManager } = await import(`${REPO}/lib/terminal/terminal-session-manager.ts`);
const { createTerminalWsBridge } = await import(`${REPO}/server/terminal-ws-bridge.ts`);
const { createTerminalOutputStream } = await import(`${REPO}/desktop/src/react/services/terminal-output-stream.ts`);
const { HistoryDirectoryCache, historyDirectoryCacheKey } = await import(`${REPO}/server/history-read/cache.ts`);
const { scanHistoryFile } = await import(`${REPO}/server/history-read/scan.ts`);
const { buildHistoryDirectory } = await import(`${REPO}/server/history-read/directory.ts`);

function directoryBytes(root) {
  let total = 0;
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const p = path.join(root, entry.name);
    total += entry.isDirectory() ? directoryBytes(p) : fs.statSync(p).size;
  }
  return total;
}

function activeResourceCounts() {
  const counts = {};
  for (const name of process.getActiveResourcesInfo()) counts[name] = (counts[name] || 0) + 1;
  return counts;
}

function mean(a) { return a.reduce((s, v) => s + v, 0) / a.length; }

/* ── 组件 1：stream store 全生命周期 ──────────────────────────────────── */
function soakStreamStore(batch) {
  let retainedMax = 0, dropSum = 0, finishCleared = true, resumeOk = true;
  for (let s = 0; s < 20; s++) {
    const state = createSessionStreamState();
    const streamId = beginSessionStream(state);
    for (let i = 0; i < 7000; i++) {
      appendSessionStreamEvent(state, { type: "text_delta", delta: `soak-b${batch}-s${s}-i${i} 流式样本。` });
    }
    retainedMax = Math.max(retainedMax, state.events.length);
    dropSum += state.droppedEvents;
    const resumed = resumeSessionStream(state, { streamId, sinceSeq: 1 });
    // 饱和 trim 后早期事件被淘汰：truncated 必须为 true，replay 只含保留窗口
    if (!resumed.truncated) resumeOk = false;
    if (resumed.events.length !== state.events.length) resumeOk = false;
    finishSessionStream(state);
    if (state.events.length !== 0 || state.totalEventBytes !== 0) finishCleared = false;
  }
  return { retainedMax, dropSum, finishCleared, resumeOk };
}

/* ── 组件 2：终端会话（真实 manager/bridge + fake backend） ───────────── */
function makeFakeBackend() {
  const handles = [];
  return {
    handles,
    spawn(options) {
      const handle = {
        emit: (data) => options.onData(data),
        exit: (exitCode = 0) => options.onExit({ exitCode, signal: null }),
        write: () => {},
        kill: () => options.onExit({ exitCode: null, signal: "SIGTERM" }),
      };
      handles.push(handle);
      return handle;
    },
  };
}

async function soakTerminals(lingxiHome, batch) {
  const backend = makeFakeBackend();
  const expandedStream = createTerminalOutputStream();
  const bridgeRef = { current: null };
  const manager = new TerminalSessionManager({
    lingxiHome,
    createBackend: () => backend,
    getSessionIdForPath: (sessionPath) => `sess_${path.basename(sessionPath, ".jsonl")}`,
    emitEvent: (event, sessionPath) => bridgeRef.current?.handleEvent(event, sessionPath),
  });
  bridgeRef.current = createTerminalWsBridge({
    terminalSessions: manager,
    resolveSessionId: (sessionPath) => `sess_${path.basename(sessionPath, ".jsonl")}`,
    broadcast: (message) => { if (message.type === "terminal_output") expandedStream.handleChunks(message); },
  });
  const sessionPath = path.join(lingxiHome, "agents", "hana", "sessions", `soak-terminals-b${batch}.jsonl`);
  const chunk = "t".repeat(2 * 1024);
  for (let index = 0; index < 12; index++) {
    const terminal = await manager.start({
      sessionPath, agentId: "hana", cwd: lingxiHome,
      command: `soak-${batch}-${index}`, label: `soak-${batch}-${index}`,
    });
    const ref = { terminalId: terminal.terminalId, sessionId: terminal.sessionId, sessionPath };
    expandedStream.subscribe(ref, { onChunks: () => {} });
    expandedStream.handleTail({ type: "terminal_tail", ...ref, terminal, chunks: [], sinceSeq: null, lastSeq: 0, truncated: false });
    for (let r = 0; r < 32; r++) backend.handles[index].emit(chunk);
  }
  backend.handles.forEach((handle) => handle.exit(0));
  await new Promise((r) => setTimeout(r, 60));
  await manager.dispose?.();
  return { diskBytes: directoryBytes(path.join(lingxiHome, "agents")) };
}

/* ── 组件 3：HistoryDirectoryCache 淘汰与失效 ─────────────────────────── */
function makeCtx(p) {
  const stat = fs.statSync(p);
  return {
    sessionPath: p,
    sessionId: null,
    locatorPath: p,
    publicRevision: `${stat.size}:${stat.mtimeMs}`,
    fileIdentity: { size: stat.size, mtimeMs: stat.mtimeMs, dev: stat.dev, ino: stat.ino, ctimeMs: stat.ctimeMs },
    branchHeadRowExists: false,
    branchHeadRow: null,
  };
}

async function soakHistoryCache(batch, cache, sessionPaths) {
  let hits = 0, rebuilds = 0, probeAppendDetected = true;
  for (const p of sessionPaths) {
    const ctx = makeCtx(p);
    const key = historyDirectoryCacheKey(ctx);
    const existing = cache.get(key);
    if (existing) {
      hits += 1;
      continue;
    }
    // miss → 真实构建链：scanHistoryFile + buildHistoryDirectory + beginBuild/publish
    const scan = await scanHistoryFile(p, { capturedLength: fs.statSync(p).size });
    const result = await buildHistoryDirectory(scan, ctx);
    if (!result.directory) throw new Error(`soak: 目录构建失败 ${(result)?.reason}`);
    const lease = await cache.beginBuild(key);
    if (!lease) continue;
    const accepted = cache.publish(key, lease, result.directory, { locatorPath: p, released: result.released });
    if (accepted) rebuilds += 1;
  }
  // 热路径对照：对最后 6 个（仍在 8 槽内的）会话二次访问 → 应命中而非重建
  for (const p of sessionPaths.slice(6)) {
    const key = historyDirectoryCacheKey(makeCtx(p));
    if (cache.get(key)) hits += 1;
  }
  // 每批对最后一个文件（本批刚构建、当前驻留）外部追加一行。P07-FIXR1（F-C）：
  // HistoryProbeVerdict 判定集 = valid / append_candidate / branch_view_stale /
  // InvalidationReason（server/history-read/types.ts），不存在 "fresh"——旧断言
  // verdict !== "fresh" 恒真，不构成证明。有效断言 = 追加后 probe 不得判 "valid"：
  // 读路径（server/history-read/index.ts tryDirectoryOnce）仅在 verdict === "valid"
  // 时直接命中缓存目录，其余判定（含 append_candidate）走增量/全量重建，不会把
  // 追加前的旧快照当命中返回。外部追加未经过插桩写入，mutation epoch 不变，真实
  // 判定为 append_candidate（目录保留、非 invalidate）；每批的槽位重建由 12 会话
  // > 8 槽的 LRU 颠簸驱动，与本追加无关。
  const appendPath = sessionPaths[sessionPaths.length - 1];
  void appendPath; // NEGPROBE-INJECT（P07-FIXR1 断言有效性证明）：追加被禁用——文件未变，probe 应判 "valid"，F-C 新断言必须变红（allOk=false → exit 1）
  const appendKey = historyDirectoryCacheKey(makeCtx(appendPath));
  const verdict = cache.probe(appendKey, makeCtx(appendPath));
  if (verdict === "valid") probeAppendDetected = false;
  const stats = cache.stats();
  return {
    cache_sessions: stats.sessions,
    cache_hits: hits,
    cache_rebuilds: rebuilds,
    cache_evictions: stats.evictions,
    cache_resident_bytes: stats.residentBytes,
    probe_verdict_after_append: verdict,
    probe_append_detected: probeAppendDetected,
    within_budget: stats.sessions <= 8 && stats.residentBytes <= 64 * 1024 * 1024,
  };
}

/* ── 主流程 ───────────────────────────────────────────────────────────── */
const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p07-soak-"));
fs.mkdirSync(path.join(lingxiHome, "agents", "hana", "sessions"), { recursive: true });

// 历史夹具：12 个会话文件 × 600 条
const HEADER = JSON.stringify({ type: "session", version: 3, id: `soak-${Date.now()}`, cwd: "/tmp", timestamp: "2026-09-22T00:00:00Z" });
const sessionPaths = [];
for (let s = 0; s < 12; s++) {
  const p = path.join(lingxiHome, "agents", "hana", "sessions", `soak-hist-${s}.jsonl`);
  const lines = [HEADER];
  let parent = "u0";
  lines.push(JSON.stringify({ type: "message", id: "u0", parentId: null, timestamp: "2026-09-22T00:00:00Z", message: { role: "user", content: "开始" } }));
  for (let i = 1; i <= 600; i++) {
    lines.push(JSON.stringify({ type: "message", id: `a${i}`, parentId: parent, timestamp: "2026-09-22T00:00:00Z", message: { role: "assistant", content: [{ type: "text", text: `soak 会话 ${s} 第 ${i} 条历史正文样本内容。` }] } }));
    parent = `a${i}`;
  }
  fs.writeFileSync(p, lines.join("\n") + "\n", "utf8");
  sessionPaths.push(p);
}

const cache = new HistoryDirectoryCache({});
const batchRecords = [];
let allOk = true;
try {
  for (let b = 1; b <= batches; b++) {
    const stream = soakStreamStore(b);
    if (!stream.finishCleared || !stream.resumeOk || stream.retainedMax > 5000) allOk = false;
    const term = await soakTerminals(lingxiHome, b);
    const hist = await soakHistoryCache(b, cache, sessionPaths);
    if (!hist.within_budget || !hist.probe_append_detected) allOk = false;
    const mu = process.memoryUsage();
    const gcNote = typeof globalThis.gc === "function" ? (() => { globalThis.gc(); const after = process.memoryUsage(); return { heapUsedPostGc: after.heapUsed, rssPostGc: after.rss }; })() : null;
    batchRecords.push({
      batch: b,
      rss_kb: Math.round(mu.rss / 1024),
      heap_used_kb: Math.round(mu.heapUsed / 1024),
      external_kb: Math.round(mu.external / 1024),
      array_buffers_kb: Math.round(mu.arrayBuffers / 1024),
      active_resources: activeResourceCounts(),
      terminal_disk_bytes: term.diskBytes,
      stream_retained_max: stream.retainedMax,
      stream_drop_sum: stream.dropSum,
      stream_finish_cleared: stream.finishCleared,
      stream_resume_ok: stream.resumeOk,
      ...hist,
      ...(gcNote ? { post_gc: gcNote } : {}),
    });
    console.log(`[soak] batch=${b}/${batches} rss=${batchRecords[batchRecords.length - 1].rss_kb}KB heap=${batchRecords[batchRecords.length - 1].heap_used_kb}KB cache_sessions=${hist.cache_sessions} evictions=${hist.cache_evictions}`);
  }

  // 稳态比较：预热 2 批后取 3–5 批 vs 最后 3 批
  const warm = batchRecords.slice(2, 5);
  const tail = batchRecords.slice(-3);
  const cmp = (pick) => {
    const a = mean(warm.map(pick)); const b = mean(tail.map(pick));
    return { warm_mean: Math.round(a), tail_mean: Math.round(b), growth_pct: +(((b - a) / a) * 100).toFixed(1) };
  };
  const monotonicStrict = batchRecords.slice(2).every((r, i, arr) => i > 0 && r.rss_kb > arr[i - 1].rss_kb * 1.05);
  const verdict = {
    rss: cmp((r) => r.rss_kb),
    heap_used: cmp((r) => r.heap_used_kb),
    external: cmp((r) => r.external_kb),
    rss_monotonic_increase_gt5pct_each_batch: monotonicStrict,
    bounded_verdict: cmp((r) => r.rss_kb).growth_pct <= 25 && !monotonicStrict ? "NO_REPRODUCIBLE_UNBOUNDED_GROWTH" : "REVIEW",
  };
  const report = {
    workload: "长运行资源 soak：stream store 全生命周期 + 终端会话循环 + 历史目录缓存淘汰/失效；进程级轨迹",
    env: {
      node: process.version, os: `${os.type()} ${os.release()} ${os.arch()}`,
      expose_gc: typeof globalThis.gc === "function",
    },
    components_ok: allOk,
    verdict,
    batches: batchRecords,
    cache_final_stats: cache.stats(),
    note: "GC 后读数单列（若可用），不作为稳态判定依据；rss/heap 以 GC 前轨迹为准",
  };
  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, JSON.stringify(report, null, 2));
  console.log(`[soak] verdict=${verdict.bounded_verdict} rss_growth=${verdict.rss.growth_pct}% heap_growth=${verdict.heap_used.growth_pct}% out=${outPath}`);
  // P07-FIXR1（F-A）：process.exit 在 try 内会立即终止进程并跳过下方 finally 的临时
  // 目录清理（原版每次运行——含成功——都泄漏 ≈12MB 的 hana-p07-soak-* 目录）。改设
  // exitCode 让事件循环自然排空：finally 执行 cache.dispose + rmSync 后以同一退出码
  // 语义退出（成功 0 / 判定失败 1）。
  process.exitCode = allOk && verdict.bounded_verdict === "NO_REPRODUCIBLE_UNBOUNDED_GROWTH" ? 0 : 1;
} finally {
  try { cache.dispose?.(); } catch { /* best-effort */ }
  fs.rmSync(lingxiHome, { recursive: true, force: true });
}
