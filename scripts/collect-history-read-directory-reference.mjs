/**
 * A04 可信参考结果采集 —— 生产参考是 A03 合法夹具下、性能改造前的实际行为。
 *
 *  - 使用 A03 修正后的合法夹具（真实 v3 文件 + 真实分支服务 + 真实 manifest store，
 *    复用 scripts/lib/history-read-fixture.mjs 的 harness，与 A03 测试构造字节级一致）。
 *  - 防污染：每个请求都从同一原始夹具字节**复制出独立文件**（全新临时目录 + 全新空
 *    manifest store + 全新 Hono app），记录读取前后文件 sha256；若生产读取改变了文件
 *    （旧读取函数的隐式修复行为），如实记录。
 *  - 归一化：严格按 reference-outputs/normalization-rules.md（脚本启动时校验该文件存在）。
 *  - 保留端到端分页拼接断言：对 1k 从首页沿 nextBefore 完整翻阅至终点，与 all=1 的
 *    记录集合逐项对照（零重叠、零缺页）。
 *
 * 用法：
 *   node scripts/collect-history-read-directory-reference.mjs \
 *     --sizes 1000,10000 --seed 20260910 \
 *     --out artifacts/history-read-directory/reference-outputs
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE_AUDIT = path.join(ROOT, "artifacts", "history-read-directory", "fixture-audit.json");
const ENVIRONMENT = path.join(ROOT, "artifacts", "history-read-directory", "environment.json");

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const ALLOWED_FIXTURE_TIMES = new Set(["09:00:00", "10:00:00"]);
const ISO_TIME_RE = /\d{4}-\d{2}-\d{2}T(\d{2}:\d{2}:\d{2})Z/g;

// ── CLI（未知参数报错、不吞退出码）──
function parseArgs(argv) {
  const args = { sizes: [1000, 10000], seed: 20260910, out: null };
  const known = new Set(["--sizes", "--seed", "--out", "--help"]);
  for (let i = 2; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help") {
      console.log("Usage: node scripts/collect-history-read-directory-reference.mjs --sizes 1000,10000 --seed 20260910 --out <dir>");
      process.exit(0);
    }
    if (!known.has(arg)) {
      console.error(`未知参数：${arg}（支持：--sizes --seed --out --help）`);
      process.exit(2);
    }
    const value = argv[i + 1];
    if (value === undefined) {
      console.error(`参数 ${arg} 缺少值`);
      process.exit(2);
    }
    if (arg === "--sizes") args.sizes = value.split(",").map((s) => Number(s.trim()));
    else if (arg === "--seed") args.seed = Number(value);
    else args.out = value;
    i += 1;
  }
  if (!args.out) {
    console.error("缺少 --out（例如 artifacts/history-read-directory/reference-outputs）");
    process.exit(2);
  }
  if (!args.sizes.length || args.sizes.some((n) => !Number.isInteger(n) || n < 1)) {
    console.error("--sizes 必须为正整 数列表");
    process.exit(2);
  }
  if (!Number.isInteger(args.seed)) {
    console.error("--seed 必须为整数");
    process.exit(2);
  }
  return args;
}

const args = parseArgs(process.argv);
const outDir = path.resolve(ROOT, args.out);
const rulesPath = path.join(outDir, "normalization-rules.md");
if (!fs.existsSync(rulesPath)) {
  console.error(`归一化规则文件不存在：${rulesPath}（A04 要求先于采集制定规则，拒绝在无规则状态下运行）`);
  process.exit(3);
}

fs.mkdirSync(outDir, { recursive: true });

// ── 归一化（normalization-rules.md R1–R5 的实现）──
function normalizeValue(value, tmpRoot, ctx, at) {
  if (typeof value === "string") {
    let v = value;
    if (tmpRoot) v = v.split(tmpRoot).join("<TMPROOT>");
    if (UUID_RE.test(v)) {
      ctx.normalizedFields.push(at);
      return "<UUID>";
    }
    return v;
  }
  if (Array.isArray(value)) {
    return value.map((v, i) => normalizeValue(v, tmpRoot, ctx, `${at}[${i}]`));
  }
  if (value !== null && typeof value === "object") {
    const out = {};
    for (const [key, v] of Object.entries(value)) {
      if (key === "revision" && typeof v === "string") {
        const m = /^(\d+):\d+(\.\d+)?$/.exec(v);
        if (m) {
          out[key] = `${m[1]}:<mtimeMs>`;
          ctx.revisionNormalized = true;
          continue;
        }
      }
      out[key] = normalizeValue(v, tmpRoot, ctx, `${at}.${key}`);
    }
    return out;
  }
  return value;
}

function assertNoClockLeak(text) {
  // R4：仅允许夹具固定时间戳 09:00:00 / 10:00:00
  for (const match of text.matchAll(ISO_TIME_RE)) {
    if (!ALLOWED_FIXTURE_TIMES.has(match[1])) {
      throw new Error(`归一化后输出出现非夹具固定时间戳（${match[0]}）——采集污染，按 R4 报错`);
    }
  }
}

// ── 主流程 ──
const { createHistoryReadCounters } = await import("./lib/history-read-counters.mjs");
const { installModuleWrappers } = await import("./lib/history-read-instrumentation.mjs");
const fx = await import("./lib/history-read-fixture.mjs");

const fixtureBytesByN = {};
for (const n of args.sizes) fixtureBytesByN[n] = fx.buildLongRunFixtureBytes(n);
const auditCheck = fx.verifyFixtureAgainstAudit(fixtureBytesByN, FIXTURE_AUDIT);

const counters = createHistoryReadCounters({ label: "history-read-reference" });
counters.install();
const uninstallHooks = installModuleWrappers();
const mods = await fx.loadProductionModules();

const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-history-reference-"));
let requestSeq = 0;
const manifestEntries = [];
const walkEvidence = {};
let interrupted = false;

function freshEnv(n) {
  const envRoot = fs.mkdtempSync(path.join(tmpRoot, `env-${n}-`));
  const env = fx.createBenchEnvironment({ mods, counters, n, rootDir: envRoot, fixtureBytes: fixtureBytesByN[n] });
  env.envRoot = envRoot;
  return env;
}

async function runReferenceRequest({ n, label, requestKind, before, limit, all = false }) {
  requestSeq += 1;
  const fixtureId = `longrun-n${n}-seed${args.seed}`;
  const env = freshEnv(n);
  try {
    await fx.attachBenchApp(env, { mods, counters });
    const url = fx.messagesUrl(env.sessionPath, { before, limit, all });
    const h = counters.beginRequest({
      fixtureId,
      seed: args.seed,
      phase: "A04-reference",
      requestKind,
      before,
      limit,
      sessionPath: env.sessionPath,
    });
    const t0 = process.hrtime.bigint();
    const res = await h.run(() => env.app.request(url));
    const serverTotalMs = Number(process.hrtime.bigint() - t0) / 1e6;
    const rec = counters.endRequest(h);
    const rawBody = await res.text();

    if (res.status !== 200) {
      throw new Error(`参考请求 ${label}（n=${n}）返回 ${res.status}：${rawBody.slice(0, 200)}`);
    }
    const postSha256 = fx.sha256(fs.readFileSync(env.sessionPath));
    const repairBackupExists = fs.existsSync(`${env.sessionPath}.repair.json`);

    const data = JSON.parse(rawBody);
    const ctx = { normalizedFields: [], revisionNormalized: false };
    const normalized = normalizeValue(data, env.envRoot, ctx, "$");
    const normalizedText = `${JSON.stringify(normalized, null, 2)}\n`;
    assertNoClockLeak(normalizedText);
    const { createHash } = await import("node:crypto");
    const sha256Normalized = createHash("sha256").update(normalizedText, "utf8").digest("hex");

    const fileBase = `n${n}__${label}`;
    fs.writeFileSync(path.join(outDir, `${fileBase}.json`), normalizedText, "utf8");

    const deferredCount = (() => {
      let count = 0;
      const walkDeferred = (v) => {
        if (Array.isArray(v)) { v.forEach(walkDeferred); return; }
        if (v !== null && typeof v === "object") {
          if (typeof v.kind === "string" && Number.isFinite(v.size) && v.available === true && typeof v.id === "string") count += 1;
          Object.values(v).forEach(walkDeferred);
        }
      };
      walkDeferred(data.messages ?? []);
      return count;
    })();
    const messages = data.messages ?? [];
    const first = messages[0] ?? null;
    const last = messages[messages.length - 1] ?? null;
    const gauges = counters.snapshotOf(rec.gauges);
    const entry = {
      fixtureId,
      seed: args.seed,
      requestSeq,
      requestLabel: label,
      requestKind,
      before,
      limit,
      all,
      urlTemplate: "path=<TMPROOT masked>",
      status: res.status,
      output: `${fileBase}.json`,
      sha256Normalized,
      keyFields: {
        messageCount: messages.length,
        firstMessage: first ? { id: first.id, sourceIndex: first.sourceIndex, role: first.role } : null,
        lastMessage: last ? { id: last.id, sourceIndex: last.sourceIndex, role: last.role } : null,
        blockCount: (data.blocks ?? []).length,
        todos: data.todos === null ? null : (data.todos ?? []).length,
        hasMore: data.hasMore,
        nextBefore: data.nextBefore,
        sessionFilesCount: (data.sessionFiles ?? []).length,
        revisionSize: Number(String(data.revision ?? "0:").split(":")[0]),
        revisionNormalized: ctx.revisionNormalized,
        deferredDescriptorCount: deferredCount,
        responseUtf8Bytes: Buffer.byteLength(rawBody, "utf8"),
      },
      antiPollution: {
        fixtureSha256: env.preSha256,
        fileBytesBefore: fixtureBytesByN[n].length,
        fileSha256After: postSha256,
        fileUnchanged: postSha256 === env.preSha256,
        repairBackupCreated: repairBackupExists,
        sessionFileWriteCalls: gauges.sessionFileWriteCalls,
      },
      gauges: {
        readCalls: gauges.readCalls,
        fullFileReadCalls: gauges.fullFileReadCalls,
        fullFileReadViaReadApi: gauges.fullFileReadViaReadApi,
        fullFileReadViaFdEpisode: gauges.fullFileReadViaFdEpisode,
        sessionFullFileReadCalls: gauges.sessionFullFileReadCalls,
        logicalReadBytes: gauges.logicalReadBytes,
        sessionFileReadBytes: gauges.sessionFileReadBytes,
        jsonlParseCount: gauges.jsonlParseCount,
        fullHistoryProjectionCount: gauges.fullHistoryProjectionCount,
        metadataVisitedCount: gauges.metadataVisitedCount,
        branchOpenCalls: gauges.branchOpenCalls,
        getBranchCalls: gauges.getBranchCalls,
        openThrows: gauges.openThrows,
        getBranchThrows: gauges.getBranchThrows,
        fallbackReason: gauges.openThrows > 0 || gauges.getBranchThrows > 0 ? "branch_entry_threw" : null,
        serializeMs: gauges.serializeMs,
        responseUtf8Bytes: gauges.responseUtf8Bytes,
      },
      memory: {
        heapUsedBefore: rec.memoryBefore.heapUsed,
        heapUsedAfter: rec.memoryAfter.heapUsed,
        externalBefore: rec.memoryBefore.external,
        externalAfter: rec.memoryAfter.external,
        arrayBuffersBefore: rec.memoryBefore.arrayBuffers,
        arrayBuffersAfter: rec.memoryAfter.arrayBuffers,
        rssBefore: rec.memoryBefore.rss,
        rssAfter: rec.memoryAfter.rss,
      },
      timing: { serverTotalMs, normalizedFields: ctx.normalizedFields },
      physicalEntryCount: 2 * n + 1,
      branchEntryCount: 2 * n,
      sourceCount: 2 * n,
      displayCount: n + 1,
      cacheState: "no_directory (旧读取路径无目录缓存)",
      fallbackReason: gauges.openThrows > 0 || gauges.getBranchThrows > 0 ? "branch_entry_threw" : null,
    };
    manifestEntries.push(entry);
    console.log(
      `[ref] n=${n} ${label}: status=${res.status} messages=${messages.length} nextBefore=${data.nextBefore} ` +
        `fullFileReadCalls=${gauges.fullFileReadCalls} jsonlParseCount=${gauges.jsonlParseCount} ` +
        `fileUnchanged=${entry.antiPollution.fileUnchanged} sha256=${sha256Normalized.slice(0, 12)}…`,
    );
    return data;
  } finally {
    fx.teardownBenchEnvironment(env);
    fs.rmSync(env.envRoot, { recursive: true, force: true });
  }
}

/** 从首页沿 nextBefore 完整翻阅至终点（A04 §4 端到端分页拼接断言 + A05 完整翻页口径）。 */
async function fullWalk(n, { recordGauges = true } = {}) {
  const fixtureId = `longrun-n${n}-seed${args.seed}`;
  const env = freshEnv(n);
  try {
    await fx.attachBenchApp(env, { mods, counters });
    const pages = [];
    const allIdsPerRequest = [];
    let cursor = null;
    let page = 0;
    const pageGauges = [];
    for (;;) {
      page += 1;
      requestSeq += 1;
      const h = counters.beginRequest({
        fixtureId,
        seed: args.seed,
        phase: "A04-reference-walk",
        requestKind: page === 1 ? "walk-first" : "walk-page",
        before: cursor,
        limit: 50,
        sessionPath: env.sessionPath,
      });
      const t0 = process.hrtime.bigint();
      const res = await h.run(() =>
        env.app.request(fx.messagesUrl(env.sessionPath, { before: cursor, limit: 50 })),
      );
      const serverTotalMs = Number(process.hrtime.bigint() - t0) / 1e6;
      const rec = counters.endRequest(h);
      const data = await res.json();
      if (res.status !== 200) throw new Error(`walk 请求（n=${n} 第 ${page} 页）返回 ${res.status}`);
      const ids = (data.messages ?? []).map((m) => m.id);
      allIdsPerRequest.push(ids);
      pages.push({
        page,
        before: cursor,
        records: ids.length,
        firstEntryId: ids[0] ?? null,
        lastEntryId: ids[ids.length - 1] ?? null,
        hasMore: data.hasMore,
        nextBefore: data.nextBefore,
        serverTotalMs,
      });
      const g = counters.snapshotOf(rec.gauges);
      if (recordGauges) {
        pageGauges.push({
          page,
          readCalls: g.readCalls,
          fullFileReadCalls: g.fullFileReadCalls,
          jsonlParseCount: g.jsonlParseCount,
          fullHistoryProjectionCount: g.fullHistoryProjectionCount,
          metadataVisitedCount: g.metadataVisitedCount,
          sessionReadBytes: g.sessionFileReadBytes,
        });
      }
      if (!data.hasMore || data.nextBefore == null) break;
      cursor = data.nextBefore;
      if (page > n + 10) throw new Error(`walk 翻页失控（n=${n}）`);
    }
    // all=1 对照
    requestSeq += 1;
    const h = counters.beginRequest({
      fixtureId,
      seed: args.seed,
      phase: "A04-reference-walk",
      requestKind: "walk-all",
      before: null,
      limit: null,
      sessionPath: env.sessionPath,
    });
    const res = await h.run(() => env.app.request(fx.messagesUrl(env.sessionPath, { all: true })));
    counters.endRequest(h);
    const allData = await res.json();
    if (res.status !== 200) throw new Error(`walk all=1（n=${n}）返回 ${res.status}`);
    const allIds = (allData.messages ?? []).map((m) => m.id);
    const walkedIds = allIdsPerRequest.flat();
    // 页序从新到旧、页内从旧到新：反转摊平后必须与 all=1 完全一致
    const walkedReversed = [...allIdsPerRequest].reverse().flat();
    const identical = walkedReversed.length === allIds.length && walkedReversed.every((id, i) => id === allIds[i]);
    const idSets = allIdsPerRequest.map((ids) => new Set(ids));
    let overlap = 0;
    for (let i = 0; i < idSets.length; i += 1) {
      for (let j = i + 1; j < idSets.length; j += 1) {
        for (const id of idSets[i]) if (idSets[j].has(id)) overlap += 1;
      }
    }
    const evidence = {
      fixtureId,
      pageCount: pages.length,
      expectedPages: Math.ceil((n + 1) / 50),
      recordsWalked: walkedIds.length,
      uniqueIds: new Set(walkedIds).size,
      overlapCount: overlap,
      identicalToAll: identical,
      allCount: allIds.length,
      displayCount: n + 1,
      pageFirstEntryIds: pages.map((p) => p.firstEntryId),
      pages,
      pageGauges,
      walkFileShaUnchanged: fx.sha256(fs.readFileSync(env.sessionPath)) === env.preSha256,
    };
    if (evidence.pageCount !== evidence.expectedPages) {
      throw new Error(`walk 页数 ${evidence.pageCount} ≠ 理论 ${evidence.expectedPages}（n=${n}）`);
    }
    if (overlap !== 0 || !identical || walkedIds.length !== n + 1) {
      throw new Error(`walk 唯一性/等价断言失败（n=${n}）：overlap=${overlap} identical=${identical}`);
    }
    walkEvidence[`n${n}`] = evidence;
    const walkTotalMs = pages.reduce((s, p) => s + p.serverTotalMs, 0);
    console.log(
      `[walk] n=${n}: ${pages.length} 页 / ${walkedIds.length} 条 / overlap=0 / all=1 等价=true / 总时(累计 serverTotalMs)=${walkTotalMs.toFixed(1)}ms`,
    );
    return evidence;
  } finally {
    fx.teardownBenchEnvironment(env);
    fs.rmSync(env.envRoot, { recursive: true, force: true });
  }
}

function requestSetFor(n) {
  // 固定请求集合：首页（before 缺省）、中间页、末页（hasMore=false）；1k 额外采集 all=1。
  // 中间页 = 第 ceil(pages/2) 页：before_k = display - 50*(k-1)（页 k 覆盖 [before-50, before)）。
  const display = n + 1;
  const pages = Math.ceil(display / 50);
  const middlePage = Math.ceil(pages / 2);
  const middleBefore = display - 50 * (middlePage - 1);
  return [
    { label: "first-page", requestKind: "first", before: null, limit: 50, all: false },
    { label: "middle-page", requestKind: "middle", before: middleBefore, limit: 50, all: false },
    { label: "last-page", requestKind: "last", before: 50, limit: 50, all: false },
    ...(n === 1000 ? [{ label: "all", requestKind: "all", before: null, limit: null, all: true }] : []),
  ];
}

try {
  console.log(`A04 参考采集：sizes=${args.sizes.join(",")} seed=${args.seed} out=${path.relative(ROOT, outDir)}`);
  for (const n of args.sizes) {
    for (const req of requestSetFor(n)) {
      await runReferenceRequest({ n, ...req });
    }
    await fullWalk(n);
  }

  // 汇总 manifest
  const environment = fs.existsSync(ENVIRONMENT)
    ? JSON.parse(fs.readFileSync(ENVIRONMENT, "utf8"))
    : { note: "environment.json 不存在" };
  const manifest = {
    task: "A04 参考输出（性能改造前生产行为快照）",
    generatedAt: new Date().toISOString(),
    gitHead: "1d42b7405c76292f617291e3a01cd2f3ef5efd04",
    gitBranch: "fix/pending-sep10",
    seed: args.seed,
    sizes: args.sizes,
    normalizationRules: "normalization-rules.md（先于本次采集制定）",
    harness: "scripts/lib/history-read-fixture.mjs（A03 writeLongRunSession/buildApp 字节级一致移植；每个请求独立临时副本 + 全新空 manifest store + 全新 Hono app）",
    scopeNote: "Hono 内存进程内请求（app.request），不含网络耗时；OS page cache 未清空，不代表物理冷盘",
    fixtureAuditCrossCheck: auditCheck,
    entries: manifestEntries,
    walkEvidence,
  };
  fs.writeFileSync(path.join(outDir, "reference-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`, "utf8");

  // 人类可读摘要
  const lines = [
    "# reference-outputs 摘要（A04）",
    "",
    `- 生成时间：${manifest.generatedAt}；HEAD \`${manifest.gitHead}\`；seed ${args.seed}`,
    "- 口径：Hono 内存进程内（`app.request`），非网络耗时；旧读取路径（无目录缓存）。",
    "- 防污染：每请求独立夹具副本；见各条目 `antiPollution`（含前后 sha256）。",
    "",
    "| 规模 | 请求 | messages | hasMore | nextBefore | fullFileReadCalls | jsonlParseCount | fullHistoryProjection | 文件未变 | sha256(归一化) 前 12 |",
    "|---|---|---|---|---|---|---|---|---|---|",
  ];
  for (const e of manifestEntries) {
    lines.push(
      `| n=${e.displayCount - 1} | ${e.requestLabel} | ${e.keyFields.messageCount} | ${e.keyFields.hasMore} | ${e.keyFields.nextBefore} | ${e.gauges.fullFileReadCalls} | ${e.gauges.jsonlParseCount} | ${e.gauges.fullHistoryProjectionCount} | ${e.antiPollution.fileUnchanged} | ${e.sha256Normalized.slice(0, 12)} |`,
    );
  }
  for (const [k, ev] of Object.entries(walkEvidence)) {
    lines.push("", `## ${k} 完整翻页`, "", `- 页数 ${ev.pageCount}（理论 ${ev.expectedPages}），记录 ${ev.recordsWalked} 条，唯一 ${ev.uniqueIds}，重叠 ${ev.overlapCount}，与 all=1 等价：${ev.identicalToAll}`);
  }
  lines.push("", "字段级明细见 `reference-manifest.json`；归一化规则见 `normalization-rules.md`。", "");
  fs.writeFileSync(path.join(outDir, "reference-summary.md"), lines.join("\n"), "utf8");
  console.log(`A04 参考采集完成：${manifestEntries.length} 份参考输出 + ${Object.keys(walkEvidence).length} 份完整翻页证据`);
} catch (error) {
  console.error("[A04] 采集失败：", error);
  process.exitCode = 1;
} finally {
  uninstallHooks();
  counters.uninstall();
  try {
    fs.rmSync(tmpRoot, { recursive: true, force: true });
  } catch {
    console.warn(`[A04] 临时目录清理失败（保留待查）：${tmpRoot}`);
  }
}

