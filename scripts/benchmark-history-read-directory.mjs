/**
 * 历史分页读取基准（任务 A05）—— 独立大基准入口，不进入默认 `npx vitest run`。
 *
 * 口径：真实服务端路由（Hono 内存进程内 `app.request`），真实 v3 夹具文件 +
 * 真实 SessionManager/现行分支入口 + 真实 manifest store（复用 A03 harness 构造）。
 * 不含网络耗时；OS page cache 未清空，不代表物理冷盘。
 *
 * 用法（与任务书一致）：
 *   node scripts/benchmark-history-read-directory.mjs \
 *     --phase A --sizes 1000,10000 --page-size 50 --seed 20260910 \
 *     --output artifacts/history-read-directory/baseline
 *
 *  - --phase A 本次实现；D/F 在对应阶段扩展（请求未实现 phase 以非零退出码报错）。
 *  - --phase B（B08）：同参数运行目录快路径（Batch 4 生产接线）——冷首页=全新环境
 *    （全新 cache 实例）首次请求；热页=同环境后续请求（目录命中）；完整翻页沿
 *    nextBefore 至终点。工作量硬断言（§7.1）：热页 fullFileReadCalls=0、
 *    fullHistoryProjectionCount=0、零回退；jsonlParseCount 与 metadata 访问量为
 *    O(K+|Dpage|)（跨规模同量级）。§7.2 阈值对照 A 基线（baseline/summary-a.json）
 *    逐项判定并如实记录。内存（§7.4）由 tests/history-read-directory-memory.test.ts
 *    承接，本脚本记录每请求内存采样与目录驻留估算（cache.stats().residentBytes）。
 *  - 未知参数报错（exit 2）；不使用 --passWithNoTests；不吞退出码。
 *  - 每规模：≥3 次冷首页（每次全新环境=显式重置），预热后完整翻页提供 ≥20 个可比
 *    页面样本（首/中/末覆盖），至少一次沿 nextBefore 翻阅至终点，校验页数与输出唯一性。
 *  - 超时/中断：保留已完成样本，summary 标记 completed=false，以非零退出码结束，不外推。
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE_AUDIT = path.join(ROOT, "artifacts", "history-read-directory", "fixture-audit.json");
const ENVIRONMENT = path.join(ROOT, "artifacts", "history-read-directory", "environment.json");

const IMPLEMENTED_PHASES = new Set(["A", "B", "C", "D", "F"]);
const KNOWN_PHASES = new Set(["A", "B", "C", "D", "E", "F"]);
/** 目录快路径 phase（B/C 同一生产实现；D01 复测 + 压力边界样本） */
const DIRECTORY_PHASES = new Set(["B", "C", "D", "F"]);

function parseArgs(argv) {
  const args = {
    phase: null,
    sizes: [1000, 10000],
    pageSize: 50,
    seed: 20260910,
    output: null,
    coldRuns: 3,
    walks: 1,
  };
  const valueArgs = new Set(["--phase", "--sizes", "--page-size", "--seed", "--output", "--cold-runs", "--walks"]);
  for (let i = 2; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!valueArgs.has(arg)) {
      console.error(`未知参数：${arg}（支持：${[...valueArgs].join(" ")}）`);
      process.exit(2);
    }
    const value = argv[i + 1];
    if (value === undefined) {
      console.error(`参数 ${arg} 缺少值`);
      process.exit(2);
    }
    switch (arg) {
      case "--phase": args.phase = value.toUpperCase(); break;
      case "--sizes": args.sizes = value.split(",").map((s) => Number(s.trim())); break;
      case "--page-size": args.pageSize = Number(value); break;
      case "--seed": args.seed = Number(value); break;
      case "--output": args.output = value; break;
      case "--cold-runs": args.coldRuns = Number(value); break;
      case "--walks": args.walks = Number(value); break;
    }
    i += 1;
  }
  if (!args.phase) {
    console.error("缺少 --phase（本次实现 A；D/F 将在对应阶段扩展）");
    process.exit(2);
  }
  if (!KNOWN_PHASES.has(args.phase)) {
    console.error(`未知 phase：${args.phase}（允许：A B C D E F）`);
    process.exit(2);
  }
  if (!IMPLEMENTED_PHASES.has(args.phase)) {
    console.error(`phase ${args.phase} 的基准入口尚未实现（本次实现 A；将在 ${args.phase} 阶段扩展本脚本）`);
    process.exit(3);
  }
  if (!args.output) {
    console.error("缺少 --output（例如 artifacts/history-read-directory/baseline）");
    process.exit(2);
  }
  if (!Array.isArray(args.sizes) || !args.sizes.length || args.sizes.some((n) => !Number.isInteger(n) || n < 1)) {
    console.error("--sizes 必须为正整数列表");
    process.exit(2);
  }
  if (!Number.isInteger(args.pageSize) || args.pageSize < 1 || args.pageSize > 200) {
    console.error("--page-size 必须为 1..200 的整数（生产上限 200）");
    process.exit(2);
  }
  if (!Number.isInteger(args.seed)) {
    console.error("--seed 必须为整数");
    process.exit(2);
  }
  if (!Number.isInteger(args.coldRuns) || args.coldRuns < 3) {
    console.error("--cold-runs 必须 ≥3（任务书要求每规模至少 3 次冷首页）");
    process.exit(2);
  }
  if (!Number.isInteger(args.walks) || args.walks < 1) {
    console.error("--walks 必须 ≥1");
    process.exit(2);
  }
  return args;
}

const args = parseArgs(process.argv);
const outDir = path.resolve(ROOT, args.output);
fs.mkdirSync(outDir, { recursive: true });

const { createHistoryReadCounters } = await import("./lib/history-read-counters.mjs");
const { installModuleWrappers } = await import("./lib/history-read-instrumentation.mjs");
const fx = await import("./lib/history-read-fixture.mjs");
const { HistoryDirectoryCache } = await import("../server/history-read/cache.ts");

const fixtureBytesByN = {};
for (const n of args.sizes) fixtureBytesByN[n] = fx.buildLongRunFixtureBytes(n);
const auditCheck = fx.verifyFixtureAgainstAudit(fixtureBytesByN, FIXTURE_AUDIT);

const counters = createHistoryReadCounters({ label: `history-read-benchmark-${args.phase}` });
counters.install();
const uninstallHooks = installModuleWrappers();
const mods = await fx.loadProductionModules();

const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-history-benchmark-"));
const runStartedAt = new Date().toISOString();
const failures = [];
let interrupted = false;
const requestWriters = new Map(); // n → { file, stream-ish: string[] }（结束后一次性落盘）

function recordRequest(n, record) {
  if (!requestWriters.has(n)) requestWriters.set(n, []);
  requestWriters.get(n).push(JSON.stringify(record));
}

function flushRequests(phase) {
  for (const [n, lines] of requestWriters) {
    const file = path.join(outDir, `requests-${phase.toLowerCase()}-n${n}.jsonl`);
    fs.writeFileSync(file, lines.length ? `${lines.join("\n")}\n` : "", "utf8");
  }
}

function makeTmpDir(prefix) {
  return fs.mkdtempSync(path.join(tmpRoot, `${prefix}-`));
}

function freshEnv(n) {
  const envRoot = fs.mkdtempSync(path.join(tmpRoot, `env-${n}-`));
  const env = fx.createBenchEnvironment({ mods, counters, n, rootDir: envRoot, fixtureBytes: fixtureBytesByN[n] });
  env.envRoot = envRoot;
  if (DIRECTORY_PHASES.has(args.phase)) {
    // B08/C05/D01：每环境注入独立 cache 实例（冷=全新缓存；可观测 stats 供工作量断言）
    env.historyCache = new HistoryDirectoryCache();
  }
  return env;
}

function attachEnv(env) {
  return fx.attachBenchApp(env, { mods, counters, engineOverrides: env.historyCache ? { historyReadCache: env.historyCache } : {} });
}

function cacheDelta(env, before) {
  if (!env.historyCache) return null;
  const after = env.historyCache.stats();
  return {
    builds: after.builds - before.builds,
    hits: after.hits - before.hits,
    invalidations: Object.entries(after.invalidations).reduce((sum, [reason, n]) => sum + n - (before.invalidations[reason] ?? 0), 0),
    incrementalUpdates: after.incrementalUpdates - before.incrementalUpdates,
    incrementalFailures: after.incrementalFailures - before.incrementalFailures,
    branchViewRebuilds: after.branchViewRebuilds - before.branchViewRebuilds,
    residentBytes: after.residentBytes,
    sessions: after.sessions,
  };
}

function identityMsOf(gauges) {
  let ms = 0;
  for (const [name, entry] of Object.entries(gauges.scanCalls ?? {})) {
    if (name.startsWith("identity.")) ms += entry.ms;
  }
  return ms;
}

/** 单请求执行 + 全仪表记录（任务书 A05 仪表清单；B08 增加目录 cache 仪表与硬断言）
 *  meta：场景夹具的真实规模覆盖（不传时按固定结构夹具公式推导）。 */
async function measuredRequest({ env, n, fixtureId, requestKind, before, limit, all, cacheState, meta = {} }) {
  const cacheBefore = env.historyCache ? env.historyCache.stats() : null;
  const h = counters.beginRequest({
    fixtureId,
    seed: args.seed,
    phase: args.phase,
    requestKind,
    before,
    limit,
    sessionPath: env.sessionPath,
  });
  const t0 = process.hrtime.bigint();
  const res = await h.run(() =>
    env.app.request(fx.messagesUrl(env.sessionPath, { before, limit, all })),
  );
  const serverTotalMs = Number(process.hrtime.bigint() - t0) / 1e6;
  const rec = counters.endRequest(h);
  const rawBody = await res.text();
  if (res.status !== 200) {
    throw new Error(`基准请求 ${requestKind}（n=${n} before=${before}）返回 ${res.status}：${rawBody.slice(0, 200)}`);
  }
  const data = JSON.parse(rawBody);
  const g = counters.snapshotOf(rec.gauges);
  const identityMs = identityMsOf(g);
  const exclusive = identityMs + g.readMs + g.parseMs + g.serializeMs;
  const cacheDeltaB = DIRECTORY_PHASES.has(args.phase) && env.historyCache ? cacheDelta(env, cacheBefore) : null;
  const legacyFallbackSignature = g.fullFileReadCalls >= 2 && (g.branchOpenCalls > 0 || g.getBranchCalls > 0);
  const cacheStateB = !cacheDeltaB ? null
    : legacyFallbackSignature ? "legacy-fallback"
      : cacheDeltaB.incrementalUpdates > 0 ? "append-update"
        : cacheDeltaB.builds > 0 ? "cold-build"
          : cacheDeltaB.hits > 0 ? "hot-hit"
            : "unknown";
  const record = {
    requestId: rec.meta.requestId,
    phase: args.phase,
    fixtureId,
    seed: args.seed,
    requestKind,
    before,
    limit,
    all: all || undefined,
    cacheState,
    // 文件 / 结构事实（D01 场景夹具传 meta 覆盖；未知维度如实记 null，不虚构公式）
    fileBytes: meta.fileBytes ?? fixtureBytesByN[n]?.length ?? null,
    physicalEntryCount: meta.physicalEntryCount ?? 2 * n + 1,
    branchEntryCount: meta.branchEntryCount ?? 2 * n,
    sourceCount: meta.sourceCount ?? 2 * n,
    displayCount: meta.displayCount ?? n + 1,
    pageRecordCount: (data.messages ?? []).length,
    // 目录仪表（阶段 A 旧路径：无目录 → 各计数恒 0；B/C：目录构建/命中/增量/回退按 delta 记录）
    cacheStateNote: args.phase === "A" ? "no_directory" : `directory(${cacheStateB})`,
    buildCount: cacheDeltaB ? cacheDeltaB.builds : 0,
    appendUpdateCount: cacheDeltaB ? cacheDeltaB.incrementalUpdates : 0,
    dependencyRecordCount: null,
    coalescingExtraBytes: null,
    directoryMs: 0,
    hydrateMs: 0,
    externalStateMs: 0,
    fallbackReason: g.openThrows > 0 || g.getBranchThrows > 0 ? "branch_entry_threw" : null,
    // B08 目录 cache 仪表
    ...(cacheDeltaB ? {
      cacheBuilds: cacheDeltaB.builds,
      cacheHits: cacheDeltaB.hits,
      cacheInvalidations: cacheDeltaB.invalidations,
      cacheResidentBytes: cacheDeltaB.residentBytes,
      cacheSessions: cacheDeltaB.sessions,
      cacheStateB,
    } : {}),
    // 读取仪表
    readCalls: g.readCalls,
    fullFileReadCalls: g.fullFileReadCalls,
    fullFileReadViaReadApi: g.fullFileReadViaReadApi,
    fullFileReadViaFdEpisode: g.fullFileReadViaFdEpisode,
    sessionFullFileReadCalls: g.sessionFullFileReadCalls,
    shortReadEvents: g.shortReadEvents,
    eofReads: g.eofReads,
    logicalReadBytes: g.logicalReadBytes,
    sessionFileReadBytes: g.sessionFileReadBytes,
    jsonlParseCount: g.jsonlParseCount,
    jsonlParseMs: g.jsonlParseMs,
    fullHistoryProjectionCount: g.fullHistoryProjectionCount,
    metadataVisitedCount: g.metadataVisitedCount,
    metadataScanCalls: g.metadataScanCalls,
    branchOpenCalls: g.branchOpenCalls,
    getBranchCalls: g.getBranchCalls,
    openThrows: g.openThrows,
    getBranchThrows: g.getBranchThrows,
    writes: { writeCalls: g.writeCalls, sessionFileWriteCalls: g.sessionFileWriteCalls, repairBackupCopyCalls: g.repairBackupCopyCalls },
    // 分项时间（互斥口径：readMs=fs；parseMs=JSON.parse 全部（含 jsonlParseMs，嵌套不另加）；serializeMs=响应 stringify；identityMs=身份解析）
    identityMs,
    readMs: g.readMs,
    parseMs: g.parseMs,
    serializeMs: g.serializeMs,
    serverTotalMs,
    residualMs: serverTotalMs - exclusive,
    responseUtf8Bytes: g.responseUtf8Bytes,
    rawBodyBytes: Buffer.byteLength(rawBody, "utf8"),
    // 内存采样点：每请求前后（process.memoryUsage）
    memory: {
      heapUsedBefore: rec.memoryBefore.heapUsed,
      heapUsedAfter: rec.memoryAfter.heapUsed,
      heapUsedDelta: rec.memoryAfter.heapUsed - rec.memoryBefore.heapUsed,
      externalBefore: rec.memoryBefore.external,
      externalAfter: rec.memoryAfter.external,
      externalDelta: rec.memoryAfter.external - rec.memoryBefore.external,
      arrayBuffersBefore: rec.memoryBefore.arrayBuffers,
      arrayBuffersAfter: rec.memoryAfter.arrayBuffers,
      arrayBuffersDelta: rec.memoryAfter.arrayBuffers - rec.memoryBefore.arrayBuffers,
      rssBefore: rec.memoryBefore.rss,
      rssAfter: rec.memoryAfter.rss,
      rssDelta: rec.memoryAfter.rss - rec.memoryBefore.rss,
    },
    // 校验
    hasMore: data.hasMore,
    nextBefore: data.nextBefore,
    firstEntryId: (data.messages ?? [])[0]?.id ?? null,
    messageIds: (data.messages ?? []).map((m) => m.id),
    tracker: {
      openCalls: env.tracker.openCalls,
      getBranchCalls: env.tracker.getBranchCalls,
      openThrows: env.tracker.openThrows,
      getBranchThrows: env.tracker.getBranchThrows,
    },
  };
  recordRequest(n, record);
  if (record.fallbackReason) {
    failures.push(`n=${n} ${requestKind}：分支入口抛错（fallback=${record.fallbackReason}），正常路径 fallback 必须为 0`);
  }
  if (DIRECTORY_PHASES.has(args.phase) && cacheStateB === "hot-hit") {
    // §7.1 工作量硬断言（稳定热命中页，all=1/legacy 除外）
    if (g.fullFileReadCalls !== 0) {
      failures.push(`n=${n} ${requestKind}：热页 fullFileReadCalls=${g.fullFileReadCalls} ≠ 0（§7.1）`);
    }
    if (g.fullHistoryProjectionCount !== 0) {
      failures.push(`n=${n} ${requestKind}：热页 fullHistoryProjectionCount=${g.fullHistoryProjectionCount} ≠ 0（§7.1）`);
    }
    if (g.branchOpenCalls > 0 || g.getBranchCalls > 0) {
      failures.push(`n=${n} ${requestKind}：热页出现分支入口调用（open=${g.branchOpenCalls} getBranch=${g.getBranchCalls}）→ 疑似回退（§7.1 fallbackCount=0）`);
    }
    const parseBound = 2 * args.pageSize + 8;
    if (g.jsonlParseCount > parseBound) {
      failures.push(`n=${n} ${requestKind}：热页 jsonlParseCount=${g.jsonlParseCount} > 窗口记录上界 ${parseBound}（≈2K+依赖，非 O(K)）`);
    }
  }
  return record;
}

function percentile(values, p) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[Math.max(0, idx)];
}

function statsOf(values) {
  if (!values.length) return { n: 0, p50: null, p95: null, max: null, mean: null };
  return {
    n: values.length,
    p50: percentile(values, 50),
    p95: percentile(values, 95),
    max: percentile(values, 100),
    mean: values.reduce((s, v) => s + v, 0) / values.length,
  };
}

/** 冷首页 ×N（每次全新环境=显式重置） */
async function coldRuns(n, fixtureId) {
  const records = [];
  for (let i = 1; i <= args.coldRuns; i += 1) {
    const env = freshEnv(n);
    try {
      await attachEnv(env);
      records.push(
        await measuredRequest({
          env, n, fixtureId,
          requestKind: `cold-${i}`,
          before: null, limit: args.pageSize, all: false,
          cacheState: "cold(process-reset; OS page cache warm)",
        }),
      );
    } finally {
      fx.teardownBenchEnvironment(env);
      fs.rmSync(env.envRoot, { recursive: true, force: true });
    }
  }
  return records;
}

/** 完整翻页 walk：首页沿 nextBefore 至终点；页 2..N 为热样本；末尾 all=1 对照 */
async function warmWalk(n, fixtureId) {
  const env = freshEnv(n);
  try {
    await attachEnv(env);
    const walkRecords = [];
    let cursor = null;
    let page = 0;
    const walkWallStart = process.hrtime.bigint();
    for (;;) {
      page += 1;
      walkRecords.push(
        await measuredRequest({
          env, n, fixtureId,
          requestKind: page === 1 ? "walk-first" : `walk-page-${page}`,
          before: cursor, limit: args.pageSize, all: false,
          cacheState: page === 1 ? "warm-walk-first" : "warm-walk-page",
        }),
      );
      const last = walkRecords[walkRecords.length - 1];
      if (!last.hasMore || last.nextBefore == null) break;
      cursor = last.nextBefore;
      if (page > n + 10) throw new Error(`walk 翻页失控（n=${n}）`);
    }
    const walkWallMs = Number(process.hrtime.bigint() - walkWallStart) / 1e6;
    // all=1 对照（唯一性断言基准）
    const allRecord = await measuredRequest({
      env, n, fixtureId,
      requestKind: "walk-all", before: null, limit: null, all: true,
      cacheState: "warm-walk-all",
    });
    const allIds = new Set(allRecord.messageIds);
    const walkIds = walkRecords.flatMap((r) => r.messageIds);
    const seen = new Set();
    let overlap = 0;
    for (const id of walkIds) {
      if (seen.has(id)) overlap += 1;
      seen.add(id);
    }
    let missing = 0;
    for (const id of allIds) if (!seen.has(id)) missing += 1;
    if (walkRecords.length !== Math.ceil((n + 1) / args.pageSize)) {
      failures.push(`n=${n}：walk 页数 ${walkRecords.length} ≠ 理论 ${Math.ceil((n + 1) / args.pageSize)}`);
    }
    if (overlap !== 0) failures.push(`n=${n}：walk 页间重叠 ${overlap} 条`);
    if (missing !== 0) failures.push(`n=${n}：walk 相对 all=1 缺页 ${missing} 条`);
    if (walkIds.length !== n + 1) failures.push(`n=${n}：walk 记录数 ${walkIds.length} ≠ display ${n + 1}`);
    return {
      walkRecords,
      allRecord,
      uniqueness: {
        pages: walkRecords.length,
        records: walkIds.length,
        unique: seen.size,
        overlap,
        missingVersusAll: missing,
        pageFirstEntryIds: walkRecords.map((r) => r.firstEntryId),
        allCount: allRecord.messageIds.length,
      },
      walkWallMs,
      walkServerTotalMs: walkRecords.reduce((s, r) => s + r.serverTotalMs, 0),
    };
  } finally {
    fx.teardownBenchEnvironment(env);
    fs.rmSync(env.envRoot, { recursive: true, force: true });
  }
}

// ── 主流程 ──
const summary = {
  task: {
    A: "A05 性能基线（阶段 A，性能改造前的旧读取路径）",
    B: "A05 阶段 B：历史读取目录快路径（B06/B07 生产接线 + B08 冷构建）",
    C: "A05 阶段 C：可信追加增量更新与真实路由验证（C03 增量扫描 / C04 受影响关系 / C05 增量性能）",
    D: "A05 阶段 D：服务端阶段回归与性能复测（D01 目录快路径复测 + 压力边界样本，生产实现与 B/C 相同）",
    F: "F02 最终固定页大小（50）服务端复测：verification-f（A→D→F 对照终点）",
  }[args.phase],
  phase: args.phase,
  generatedAt: runStartedAt,
  completed: false,
  gitHead: "1d42b7405c76292f617291e3a01cd2f3ef5efd04",
  gitBranch: "fix/pending-sep10",
  seed: args.seed,
  pageSize: args.pageSize,
  coldRuns: args.coldRuns,
  walks: args.walks,
  cli: process.argv.slice(2).join(" "),
  measurementScope: "Hono 内存进程内（app.request）；非网络耗时；OS page cache 未清空；包装层计时开销计入对应分项（readMs/parseMs 为上界）",
  timingModel: args.phase === "A"
    ? "互斥分项：identityMs+readMs+parseMs+serializeMs 可加；serverTotalMs 独立实测；差值=residualMs（含投影/路由循环/GC/未归因部分）。jsonlParseMs ⊂ parseMs（嵌套，不重复相加）。directoryMs/hydrateMs/externalStateMs 阶段 A 恒 0（无目录；引擎无外部 store；路由内联 hydrate 无法在不改生产代码情况下单列，归入 residual）"
    : "互斥分项：identityMs+readMs+parseMs+serializeMs 可加；serverTotalMs 独立实测；差值=residualMs（含投影/目录定位/合并/GC/未归因部分）。jsonlParseMs ⊂ parseMs（嵌套，不重复相加）。目录仪表（buildCount/appendUpdateCount/cacheStateNote）按 cache stats delta 逐请求记录；增量请求的 parsedRecords 只含新增/续读记录",
  fixtureAuditCrossCheck: auditCheck,
  sizes: {},
  failures,
};

function markInterrupted(signal) {
  if (interrupted) return;
  interrupted = true;
  failures.push(`interrupted by ${signal}；已完成样本已保留，总时标为未完成，不外推`);
  summary.completed = false;
  summary.interruptedBy = signal;
  flushRequests(args.phase);
  fs.writeFileSync(path.join(outDir, `summary-${args.phase.toLowerCase()}.json`), `${JSON.stringify(summary, null, 2)}\n`, "utf8");
  console.error(`[A05] 收到 ${signal}：保留已完成样本，标记未完成并退出`);
  cleanupAll();
  process.exit(130);
}
process.on("SIGINT", () => markInterrupted("SIGINT"));
process.on("SIGTERM", () => markInterrupted("SIGTERM"));

function cleanupAll() {
  uninstallHooks();
  try { counters.uninstall(); } catch {}
  try { fs.rmSync(tmpRoot, { recursive: true, force: true }); } catch {}
}

const sizeResults = {};
try {
  // 进程级预热（排除一次性模块装载；独立环境，样本丢弃）
  {
    const env = freshEnv(args.sizes[0]);
    try {
      await attachEnv(env);
      const h = counters.beginRequest({ phase: args.phase, requestKind: "process-warmup (excluded)", sessionPath: env.sessionPath });
      const res = await h.run(() => env.app.request(fx.messagesUrl(env.sessionPath)));
      counters.endRequest(h);
      await res.text();
    } finally {
      fx.teardownBenchEnvironment(env);
      fs.rmSync(env.envRoot, { recursive: true, force: true });
    }
  }

  for (const n of args.sizes) {
    const fixtureId = `longrun-n${n}-seed${args.seed}`;
    const cold = await coldRuns(n, fixtureId);
    const walks = [];
    for (let w = 1; w <= args.walks; w += 1) walks.push(await warmWalk(n, fixtureId));
    const warmSamples = walks.flatMap((w) => w.walkRecords.filter((r) => r.requestKind !== "walk-first"));
    if (warmSamples.length < 20) {
      failures.push(`n=${n}：热样本 ${warmSamples.length} < 20`);
    }
    // 覆盖检查：热样本必须含首/中/末（walk 本身覆盖全部页，首/末在 walk 中，这里校验样本页位置分布）
    sizeResults[n] = {
      fixtureId,
      displayCount: n + 1,
      pages: Math.ceil((n + 1) / args.pageSize),
      cold: {
        records: cold.map((r) => ({ requestKind: r.requestKind, serverTotalMs: r.serverTotalMs, fullFileReadCalls: r.fullFileReadCalls, jsonlParseCount: r.jsonlParseCount, sessionFileReadBytes: r.sessionFileReadBytes })),
        serverTotalMs: statsOf(cold.map((r) => r.serverTotalMs)),
      },
      warm: {
        sampleCount: warmSamples.length,
        coverage: "walk 页 2..N（含中/末；首页样本由 walk-first 单列）",
        serverTotalMs: statsOf(warmSamples.map((r) => r.serverTotalMs)),
        readMs: statsOf(warmSamples.map((r) => r.readMs)),
        parseMs: statsOf(warmSamples.map((r) => r.parseMs)),
        identityMs: statsOf(warmSamples.map((r) => r.identityMs)),
        serializeMs: statsOf(warmSamples.map((r) => r.serializeMs)),
        residualMs: statsOf(warmSamples.map((r) => r.residualMs)),
        fullFileReadCalls: statsOf(warmSamples.map((r) => r.fullFileReadCalls)),
        jsonlParseCount: statsOf(warmSamples.map((r) => r.jsonlParseCount)),
        metadataVisitedCount: statsOf(warmSamples.map((r) => r.metadataVisitedCount)),
        responseUtf8Bytes: statsOf(warmSamples.map((r) => r.responseUtf8Bytes)),
        heapUsedDelta: statsOf(warmSamples.map((r) => r.memory.heapUsedDelta)),
      },
      walks: walks.map((w) => ({
        pages: w.uniqueness.pages,
        records: w.uniqueness.records,
        unique: w.uniqueness.unique,
        overlap: w.uniqueness.overlap,
        missingVersusAll: w.uniqueness.missingVersusAll,
        pageFirstEntryIds: w.uniqueness.pageFirstEntryIds,
        walkWallMs: w.walkWallMs,
        walkServerTotalMs: w.walkServerTotalMs,
      })),
      fullWalk: walks[0],
    };
    const coldP50 = sizeResults[n].cold.serverTotalMs.p50;
    const warmP50 = sizeResults[n].warm.serverTotalMs.p50;
    console.log(
      `[bench] n=${n}: 冷首页 p50=${coldP50?.toFixed(1)}ms（${cold.length} 次） | 热页 p50=${warmP50?.toFixed(1)}ms p95=${sizeResults[n].warm.serverTotalMs.p95?.toFixed(1)}ms（${warmSamples.length} 样本） | ` +
      `完整翻页 ${walks[0].uniqueness.pages} 页 wall=${walks[0].walkWallMs.toFixed(0)}ms | fullFileReadCalls/请求=${walks[0].walkRecords[1].fullFileReadCalls} jsonlParseCount/请求=${walks[0].walkRecords[1].jsonlParseCount}`,
    );
  }

  // B08：§7.2 阈值对照（A 基线）、§7.1 跨规模元数据同量级证明
  let thresholds = null;
  if (DIRECTORY_PHASES.has(args.phase)) {
    const meta1k = sizeResults[1000]?.warm.metadataVisitedCount.p50 ?? null;
    const meta10k = sizeResults[10000]?.warm.metadataVisitedCount.p50 ?? null;
    if (meta1k != null && meta10k != null && meta1k > 0) {
      const ratio = meta10k / meta1k;
      if (ratio > 10) {
        failures.push(`§7.1：10k 热页 metadataVisitedCount p50（${meta10k}）超过 1k（${meta1k}）10 倍（ratio=${ratio.toFixed(2)}）——元数据访问非同量级`);
      }
    }
    const baselinePath = path.join(ROOT, "artifacts", "history-read-directory", "baseline", "summary-a.json");
    const baselineA = fs.existsSync(baselinePath) ? JSON.parse(fs.readFileSync(baselinePath, "utf8")) : null;
    const getBaseline = (n, kind) => baselineA?.sizes?.[String(n)]?.[kind]?.serverTotalMs?.p50 ?? null;
    const items = [];
    for (const [nStr, r] of Object.entries(sizeResults)) {
      const n = Number(nStr);
      const bCold = getBaseline(n, "cold");
      if (bCold != null) {
        const limit = 1.25 * bCold + 25;
        const actual = r.cold.serverTotalMs.p50;
        items.push({ metric: `cold p50 n=${n} ≤ 1.25×baseline+25ms`, baseline: bCold, limit, actual, pass: actual <= limit });
      }
    }
    {
      const p1 = sizeResults[1000]?.warm.serverTotalMs.p50 ?? null;
      const p10 = sizeResults[10000]?.warm.serverTotalMs.p50 ?? null;
      if (p1 != null && p10 != null) {
        const limit = 3 * Math.max(p1, 5);
        items.push({ metric: "hot p50 n=10k ≤ 3×max(p50 n=1k, 5ms)", baseline: p1, limit, actual: p10, pass: p10 <= limit });
      }
    }
    {
      const w1 = sizeResults[1000]?.walks?.[0]?.walkServerTotalMs ?? null;
      const w10 = sizeResults[10000]?.walks?.[0]?.walkServerTotalMs ?? null;
      if (w1 != null && w10 != null) {
        const limit = 15 * w1;
        items.push({ metric: "全翻 total_10k / total_1k ≤ 15", baseline: w1, limit, actual: w10, pass: w10 <= limit });
      }
    }
    thresholds = {
      baselineSource: baselineA ? "artifacts/history-read-directory/baseline/summary-a.json" : "baseline/summary-a.json 缺失（跳过对照）",
      items,
      allPassed: items.every((i) => i.pass),
    };
    for (const item of items) {
      console.log(`[threshold] ${item.pass ? "PASS" : "FAIL"} ${item.metric}: actual=${typeof item.actual === "number" ? item.actual.toFixed(2) : item.actual} limit=${typeof item.limit === "number" ? item.limit.toFixed(2) : item.limit}`);
    }
  }
  summary.thresholds = thresholds;

  // ── C05：可信追加增量验收（每规模：热目录上追加 1/10/100 条与一条 200KB 大工具结果）──
  let appendScenarios = null;
  if (args.phase === "C") {
    appendScenarios = {};
    for (const n of args.sizes) {
      const fixtureId = `longrun-n${n}-seed${args.seed}`;
      const env = freshEnv(n);
      try {
        await attachEnv(env);
        if (!env.historyCache) throw new Error("phase C 需要 historyCache 注入");
        // 预热：冷构建一次
        await measuredRequest({ env, n, fixtureId, requestKind: "c-warm", before: null, limit: args.pageSize, all: false, cacheState: "cold-build" });
        const scenarios = [];
        const batches = [
          { label: "append-1", count: 1 },
          { label: "append-10", count: 10 },
          { label: "append-100", count: 100 },
          { label: "append-big-tool-result", count: 1, bigToolResult: 200 * 1024 },
        ];
        let suffix = 0;
        let parentId = `a${n}`;
        for (const batch of batches) {
          const sizeBefore = fs.statSync(env.sessionPath).size;
          const statsBefore = env.historyCache.stats();
          const appendedLines = [];
          let appendedBytes = 0;
          for (let i = 0; i < batch.count; i += 1) {
            suffix += 1;
            const id = `x${suffix}`;
            const message = batch.bigToolResult
              ? { role: "assistant", content: [{ type: "text", text: `BIG-${suffix}-` + "y".repeat(Math.max(0, batch.bigToolResult - 32)) }] }
              : { role: "assistant", content: `增量追加 ${suffix}` };
            const existing = fs.readFileSync(env.sessionPath);
            const needsSep = existing.length > 0 && existing[existing.length - 1] !== 0x0a;
            const line = (needsSep ? "\n" : "") + JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T11:00:00Z", message }) + "\n";
            fs.appendFileSync(env.sessionPath, line);
            appendedBytes += Buffer.byteLength(line, "utf8");
            appendedLines.push(id);
            parentId = id;
          }
          void appendedLines;
          const rec = await measuredRequest({
            env, n, fixtureId,
            requestKind: `c-${batch.label}`,
            before: null, limit: args.pageSize, all: false,
            cacheState: "append-update",
          });
          const statsAfter = env.historyCache.stats();
          scenarios.push({
            label: batch.label,
            appendedBytes,
            appendedEntries: batch.count,
            parsedRecords: rec.jsonlParseCount,
            logicalReadBytes: rec.logicalReadBytes,
            incrementalUpdates: statsAfter.incrementalUpdates - statsBefore.incrementalUpdates,
            incrementalFailures: statsAfter.incrementalFailures - statsBefore.incrementalFailures,
            updateServerTotalMs: rec.serverTotalMs,
            pageRecordCount: rec.pageRecordCount,
          });
        }
        // 超长开放 Run 逐条追加 20 次：每次更新时间应与 Run 长度无关
        const flat = [];
        const openLines = [JSON.stringify({ type: "session", version: 3, id: `open-${n}`, cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" }),
          JSON.stringify({ type: "message", id: "u-open", parentId: null, timestamp: "2026-09-10T10:00:00Z", message: { role: "user", content: "开放 Run" } })];
        let openParent = "u-open";
        for (let i = 1; i <= 500; i += 1) {
          const id = `o${i}`;
          openLines.push(JSON.stringify({ type: "message", id, parentId: openParent, timestamp: "2026-09-10T10:00:00Z", message: { role: "assistant", content: `开放 ${i}` } }));
          openParent = id;
        }
        const openEnvRoot = fs.mkdtempSync(path.join(tmpRoot, `env-open-${n}-`));
        const openEnv = fx.createBenchEnvironment({ mods, counters, n, rootDir: openEnvRoot, fixtureBytes: Buffer.from(openLines.join("\n") + "\n", "utf8") });
        openEnv.envRoot = openEnvRoot;
        try {
          await attachEnv(openEnv);
          await measuredRequest({ env: openEnv, n, fixtureId: `open-${n}`, requestKind: "open-warm", before: null, limit: args.pageSize, all: false, cacheState: "cold-build" });
          for (let k = 1; k <= 20; k += 1) {
            const id = `oa${k}`;
            fs.appendFileSync(openEnv.sessionPath, JSON.stringify({ type: "message", id, parentId: openParent, timestamp: "2026-09-10T10:01:00Z", message: { role: "assistant", content: `逐条 ${k}` } }) + "\n");
            openParent = id;
            const t0 = process.hrtime.bigint();
            await measuredRequest({ env: openEnv, n, fixtureId: `open-${n}`, requestKind: `open-append-${k}`, before: null, limit: args.pageSize, all: false, cacheState: "append-update" });
            flat.push({ id, suffix: k, durationMs: Number(process.hrtime.bigint() - t0) / 1e6 });
          }
          // 断言：单次追加更新耗时不随 Run 长度增长（20 次的 max ≤ 4 × min，且有界）
          const durations = flat.map((f) => f.durationMs);
          const spread = Math.max(...durations) / Math.min(...durations);
          if (spread > 8) failures.push(`n=${n}：开放 Run 逐条追加 20 次耗时离散度 ${spread.toFixed(1)}× > 8×（疑似 O(Run) 改写）`);
          appendScenarios[`open-run-${n}`] = { appends: 20, durationsMs: durations, spread: Number(spread.toFixed(2)) };
        } finally {
          fx.teardownBenchEnvironment(openEnv);
          fs.rmSync(openEnvRoot, { recursive: true, force: true });
        }
        appendScenarios[`n${n}`] = scenarios;
        const hit = scenarios.reduce((s, x) => s + x.incrementalUpdates, 0);
        const failCount = scenarios.reduce((s, x) => s + x.incrementalFailures, 0);
        console.log(`[bench-C] n=${n}: 追加批次 ${scenarios.length}，增量命中 ${hit}，失败 ${failCount}`);
      } finally {
        fx.teardownBenchEnvironment(env);
        fs.rmSync(env.envRoot, { recursive: true, force: true });
      }
    }
  }
  summary.appendScenarios = appendScenarios;

  // ── D01：压力边界样本（证明边界，不替代固定结构可比样本）──
  // 大工具结果 / 多 custom / 多 Run / 分支抛弃记录多 / 会话级输出较大：
  // 每样本走真实路由冷构建 + 首/中/末热页，断言热页零整文件读、零回退、
  // 解析数与规模无关（O(窗口)），并记录目录驻留。
  if (args.phase === "D") {
    const scenariosOut = {};
    const buildScenarioBytes = (name) => {
      const header = JSON.stringify({ type: "session", version: 3, id: `scen-${name}-${args.seed}`, cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" });
      const msg = (id, parentId, message) => JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T10:00:00Z", message });
      const custom = (id, parentId, customType, data) => JSON.stringify({ type: "custom", id, parentId, timestamp: "2026-09-10T10:00:00Z", customType, data });
      const lines = [header];
      let parent = null;
      const push = (line) => { lines.push(line); };
      const chain = (id) => { const p = parent; parent = id; return p; };
      if (name === "big-tool-result") {
        // n=120 链，中间一条 toolResult 携带 200KB 文本（工具结果正文不在目录驻留）
        for (let i = 1; i <= 120; i += 1) {
          const p1 = chain(`a${i}`);
          push(msg(`a${i}`, p1, { role: "assistant", content: `回复 ${i}` }));
          if (i === 60) {
            const p2 = chain(`r60`);
            push(msg(`r60`, p2, { role: "toolResult", content: [{ type: "text", text: `BIG-RESULT-${"y".repeat(200 * 1024)}` }] }));
          }
        }
      } else if (name === "many-customs") {
        // n=240：assistant 与 custom（display!==false 的 custom_message 形态走可显示路径）
        for (let i = 1; i <= 240; i += 1) {
          const p1 = chain(`a${i}`);
          push(msg(`a${i}`, p1, { role: "assistant", content: `回复 ${i}` }));
          const p2 = chain(`c${i}`);
          push(custom(`c${i}`, p2, "tool_progress", { label: `步骤 ${i}`, percent: i % 100 }));
        }
      } else if (name === "multi-run") {
        // 200 组 user→assistant：每 user 开新 Run → 200 个 Run 边界
        for (let i = 1; i <= 200; i += 1) {
          const p1 = chain(`u${i}`);
          push(msg(`u${i}`, p1, { role: "user", content: `问题 ${i}` }));
          const p2 = chain(`a${i}`);
          push(msg(`a${i}`, p2, { role: "assistant", content: `回答 ${i}` }));
        }
      } else if (name === "many-discarded") {
        // 主链 300 条 + 挂在 a1 下的 300 条抛弃支链（head=主链叶，支链不入当前视图）
        for (let i = 1; i <= 300; i += 1) {
          const p1 = chain(`a${i}`);
          push(msg(`a${i}`, p1, { role: "assistant", content: `主链 ${i}` }));
        }
        let sideParent = "a1";
        for (let i = 1; i <= 300; i += 1) {
          push(msg(`b${i}`, sideParent, { role: "assistant", content: `抛弃支链 ${i}` }));
          sideParent = `b${i}`;
        }
      } else if (name === "large-output") {
        // n=200，每条 ~4KiB 文本 → 每页（50 条）输出体量大
        for (let i = 1; i <= 200; i += 1) {
          const p1 = chain(`a${i}`);
          push(msg(`a${i}`, p1, { role: "assistant", content: `大输出-${i}-` + "z".repeat(4096) }));
        }
      } else {
        throw new Error(`未知 scenario：${name}`);
      }
      return Buffer.from(lines.join("\n") + "\n", "utf8");
    };
    const scenarioNames = ["big-tool-result", "many-customs", "multi-run", "many-discarded", "large-output"];
    for (const name of scenarioNames) {
      const scenarioRoot = fs.mkdtempSync(path.join(tmpRoot, `scen-${name}-`));
      const fixtureBytes = buildScenarioBytes(name);
      const meta = {
        fileBytes: fixtureBytes.length,
        physicalEntryCount: fixtureBytes.toString("utf8").split("\n").filter((l) => l.trim() !== "").length,
        branchEntryCount: null,
        sourceCount: null,
        displayCount: null,
      };
      const env = fx.createBenchEnvironment({ mods, counters, n: 0, rootDir: scenarioRoot, fixtureBytes });
      env.envRoot = scenarioRoot;
      env.historyCache = new HistoryDirectoryCache();
      try {
        await attachEnv(env);
        const req = (extra) => measuredRequest({ env, n: 0, fixtureId: `scen-${name}`, requestKind: `scen-${name}-${extra.kind}`, before: extra.before, limit: args.pageSize, all: false, cacheState: extra.cacheState, meta });
        const cold = await req({ kind: "cold", before: null, cacheState: "cold-build" });
        const residentBytes = env.historyCache.stats().residentBytes;
        const probes = [];
        for (let k = 0; k < 3; k += 1) {
          const before = k === 0 ? null : (probes[k - 1].nextBefore ?? null);
          if (k > 0 && before == null) break; // 已到末页
          probes.push(await req({ kind: "hot", before, cacheState: "hot-hit" }));
        }
        const hotParse = probes.map((p) => p.jsonlParseCount);
        scenariosOut[name] = {
          displayCount: (cold.messageIds ?? []).length,
          pageRecordCount: cold.pageRecordCount,
          coldServerTotalMs: Number(cold.serverTotalMs.toFixed(2)),
          coldJsonlParseCount: cold.jsonlParseCount,
          hotJsonlParseCounts: hotParse,
          hotFullFileReadCalls: probes.reduce((s, p) => s + p.fullFileReadCalls, 0),
          hotFallbacks: probes.reduce((s, p) => s + (p.fallbackReason ? 1 : 0), 0),
          directoryResidentBytes: residentBytes,
        };
        if (probes.some((p) => p.fullFileReadCalls !== 0)) failures.push(`scenario ${name}：热页出现整文件读（§7.1）`);
        if (probes.some((p) => p.fallbackReason)) failures.push(`scenario ${name}：热页回退 ${JSON.stringify(probes.map((p) => p.fallbackReason))}`);
        console.log(`[bench-D] scenario=${name}: cold=${scenariosOut[name].coldServerTotalMs}ms hotParse=[${hotParse.join(",")}] resident=${(residentBytes / 1048576).toFixed(2)}MiB`);
      } finally {
        fx.teardownBenchEnvironment(env);
        fs.rmSync(scenarioRoot, { recursive: true, force: true });
      }
    }
    summary.scenarios = scenariosOut;
  }

  summary.completed = failures.length === 0;
  summary.sizes = sizeResults;

  // 环境快照：A01 environment.json + 运行时 heap / cpu
  const runtimeEnv = {
    capturedAt: new Date().toISOString(),
    node: process.version,
    platform: process.platform,
    arch: process.arch,
    pid: process.pid,
    cpus: os.cpus().length,
    cpuModel: os.cpus()[0]?.model ?? null,
    loadavg: os.loadavg(),
    totalmem: os.totalmem(),
    freemem: os.freemem(),
    memoryUsage: process.memoryUsage(),
    uptimeSec: process.uptime(),
  };
  summary.environment = {
    a01: fs.existsSync(ENVIRONMENT) ? JSON.parse(fs.readFileSync(ENVIRONMENT, "utf8")) : { note: "environment.json 不存在" },
    runtime: runtimeEnv,
  };

  flushRequests(args.phase);
  fs.writeFileSync(path.join(outDir, `summary-${args.phase.toLowerCase()}.json`), `${JSON.stringify(summary, null, 2)}\n`, "utf8");

  // baseline-summary.md
  const md = [
    `# phase ${args.phase} 摘要` + ({
      A: " — 旧读取路径性能基线",
      B: " — 历史读取目录快路径（B06/B07 生产接线 + B08 冷构建）",
      C: " — 历史读取可信追加增量更新与真实路由验证（C03/C04/C05）",
      D: " — 服务端阶段复测与压力边界样本（D01，生产实现与 B/C 相同）",
    }[args.phase]),
    "",
    `- 生成：${runStartedAt}｜HEAD \`${summary.gitHead}\`｜分支 \`${summary.gitBranch}\`｜seed ${args.seed}｜页大小 ${args.pageSize}`,
    `- 口径：${summary.measurementScope}`,
    `- 时间模型：${summary.timingModel}`,
    `- 夹具：A03 合法长 Run 夹具（1 文件头 + 1 user + N assistant + N-1 toolResult；display=N+1；页数=ceil((N+1)/${args.pageSize})），与 fixture-audit.json sha256 交叉校验：${auditCheck.auditAvailable ? "一致" : "跳过（无审计文件）"}`,
    "",
    "## 关键数字",
    "",
    "| 规模 | 冷首页 p50 / p95 / max (ms) | 热页 p50 / p95 / max (ms) | 热样本数 | 完整翻页（页 / wall ms / 累计 serverTotal ms） | 唯一性 |",
    "|---|---|---|---|---|---|",
  ];
  for (const [nStr, r] of Object.entries(sizeResults)) {
    const w = r.fullWalk;
    const c = r.cold.serverTotalMs;
    const wm = r.warm.serverTotalMs;
    md.push(
      `| n=${nStr} | ${c.p50?.toFixed(1)} / ${c.p95?.toFixed(1)} / ${c.max?.toFixed(1)} | ${wm.p50?.toFixed(2)} / ${wm.p95?.toFixed(2)} / ${wm.max?.toFixed(2)} | ${r.warm.sampleCount} | ${w.uniqueness.pages} / ${w.walkWallMs.toFixed(0)} / ${w.walkServerTotalMs.toFixed(0)} | overlap=${w.uniqueness.overlap} missing=${w.uniqueness.missingVersusAll} |`,
    );
  }
  md.push(
    "",
    args.phase === "A" ? "## 每请求工作量（热页，阶段 A 旧路径）" : "## 每请求工作量（热页，目录路径）",
    "",
    "| 规模 | fullFileReadCalls | sessionFileReadBytes | jsonlParseCount | fullHistoryProjectionCount | metadataVisitedCount | readMs p50 | parseMs p50 | identityMs p50 | residualMs p50 |",
    "|---|---|---|---|---|---|---|---|---|---|",
  );
  for (const [nStr, r] of Object.entries(sizeResults)) {
    const wm = r.warm;
    md.push(
      `| n=${nStr} | ${wm.fullFileReadCalls.p50} | ${(sizeResults[nStr].fullWalk.walkRecords[1].sessionFileReadBytes / 1024).toFixed(0)} KiB | ${wm.jsonlParseCount.p50} | ${sizeResults[nStr].fullWalk.walkRecords[1].fullHistoryProjectionCount} | ${wm.metadataVisitedCount.p50} | ${wm.readMs.p50?.toFixed(2)} | ${wm.parseMs.p50?.toFixed(2)} | ${wm.identityMs.p50?.toFixed(3)} | ${wm.residualMs.p50?.toFixed(2)} |`,
    );
  }
  if (args.phase === "C" && summary.appendScenarios) {
    md.push(
      "",
      "## C05 可信追加增量（真实路由，热目录追加）",
      "",
      "| 规模 | 批次 | appendedBytes | appendedEntries | parsedRecords | incrementalUpdates | incrementalFailures | updateServerTotalMs | 页记录数 |",
      "|---|---|---|---|---|---|---|---|---|",
    );
    for (const [key, scenarios] of Object.entries(summary.appendScenarios)) {
      if (!/^n\d+$/.test(key) || !Array.isArray(scenarios)) continue;
      for (const s of scenarios) {
        md.push(`| ${key.slice(1)} | ${s.label} | ${s.appendedBytes} | ${s.appendedEntries} | ${s.parsedRecords} | ${s.incrementalUpdates} | ${s.incrementalFailures} | ${s.updateServerTotalMs.toFixed(2)} | ${s.pageRecordCount} |`);
      }
    }
    for (const [key, run] of Object.entries(summary.appendScenarios)) {
      if (!key.startsWith("open-run-") || !run || !Array.isArray(run.durationsMs)) continue;
      md.push("", `开放 Run（${key.slice(9)} 规模独立会话，500 条）逐条追加 ${run.appends} 次：单次 serverTotalMs 离散度 max/min = ${run.spread}×（判定线 ≤8×，无 O(Run) 批量改写证据）`);
    }
  }
  if (args.phase === "D" && summary.scenarios) {
    md.push(
      "",
      "## D01 压力边界样本（真实路由，冷构建 + 首/中/末热页）",
      "",
      "| 场景 | 首页记录数 | 冷构建 ms | 冷解析数 | 热页解析数 | 热页整文件读 | 热页回退 | 目录驻留 MiB |",
      "|---|---|---|---|---|---|---|---|",
    );
    for (const [name, s] of Object.entries(summary.scenarios)) {
      md.push(`| ${name} | ${s.pageRecordCount} | ${s.coldServerTotalMs} | ${s.coldJsonlParseCount} | ${s.hotJsonlParseCounts.join(" / ")} | ${s.hotFullFileReadCalls} | ${s.hotFallbacks} | ${(s.directoryResidentBytes / 1048576).toFixed(2)} |`);
    }
    md.push("", "场景说明：big-tool-result=单条 200KiB 工具结果正文（目录无正文驻留）；many-customs=assistant+custom 交错；multi-run=200 组 user→assistant（200 个 Run 边界）；many-discarded=300 主链+300 抛弃支链（head=主链叶）；large-output=每条 ~4KiB 输出（页体量大）。");
  }
  md.push(
    "",
    "口径备注：",
    "",
    ...(args.phase === "A" ? [
      "- `fullFileReadCalls=2`/请求 = repair 同步整文件读（read-api） + SDK loadEntriesFromFile 循环全量装载（fd-episode）；readSessionHeader 4KiB 有界头扫描与 looksLikePiSessionFile 512B 探测不计入整文件读（计入 readCalls/logicalReadBytes）。",
      "- `jsonlParseCount` = JSON.parse 包装层对会话 JSONL 形态（`{\"type\":…` 开头）行的计数 = repair 行解析 + SDK 行解析 + 头解析；1k 每请求 4004 = 2×2001+2，10k 每请求 40004 = 2×20001+2。",
      "- `fullHistoryProjectionCount` = getBranch 调用数（旧路径 getBranch→projectBranchHistory 1:1 代理口径；fallback 路径不调 getBranch）。",
      "- `metadataVisitedCount` = 被包装的 5 个全数组扫描入口（origin/collab/modelCallRef/toolOutcomes/todos）访问的条目总数（下界；路由内联预扫描未计入，见 A02 read-path-map §1）。",
    ] : [
      "- 热命中请求 `fullFileReadCalls=0`、`jsonlParseCount` ≈ 2×页大小+依赖（目录定位 + 窗口读取，与历史总规模无关）；冷构建/降级请求含整文件扫描与全量解析，如实按请求记录。",
      "- 每请求记录 `cacheStateNote`/`buildCount`/`appendUpdateCount`（cache stats delta）：append-update = 可信追加走增量更新；cold-build = 目录构建（含增量失败后的全量重建）；legacy-fallback = 目录不可用回退旧路径；hot-hit = 目录命中。",
      "- `appendUpdateCount>0` 时 `parsedRecords` 只含增量续读/新增记录（O(新增)；尾记录重读允许），与规模无关是 C05 验收口径。",
    ]),
    "- `heapUsedDelta` 等内存采样为每请求前后 process.memoryUsage 差值（采样点定义：请求发出前 / 响应返回后）。",
    "",
    failures.length ? `**失败/异常项（${failures.length}）**：` : "**失败/异常项**：无",
    ...failures.map((f) => `- ${f}`),
    "",
  );
  fs.writeFileSync(path.join(outDir, `summary-${args.phase.toLowerCase()}.md`), md.join("\n"), "utf8");

  console.log(`A05 基准完成：${summary.completed ? "全部校验通过" : `存在 ${failures.length} 项失败`}；输出 ${path.relative(ROOT, outDir)}`);
  if (!summary.completed) process.exitCode = 1;
} catch (error) {
  console.error("[A05] 基准失败：", error);
  failures.push(`fatal: ${error?.stack ?? error}`);
  summary.completed = false;
  summary.fatal = String(error?.message ?? error);
  flushRequests(args.phase);
  fs.writeFileSync(path.join(outDir, `summary-${args.phase.toLowerCase()}.json`), `${JSON.stringify(summary, null, 2)}\n`, "utf8");
  process.exitCode = 1;
} finally {
  cleanupAll();
}
