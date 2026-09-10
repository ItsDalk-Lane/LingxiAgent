/**
 * 阶段 B 差分工具 —— 同源抽取（B01）/ 目录缓存改造的生产行为等价验证。
 *
 * 复用 A04 参考采集的 harness（scripts/lib/history-read-fixture.mjs）：每个请求都从
 * 同一原始夹具字节复制独立文件（全新临时目录 + 全新空 manifest store + 全新 Hono app），
 * 对冻结的参考输出（reference-outputs/，R1–R5 归一化）按相同规则重放比对：
 *   - sha256Normalized 逐字节一致；
 *   - 关键字段清单一致（messages 数、首末条 id/sourceIndex/role、blocks 数、todos、
 *     hasMore、nextBefore、sessionFiles 数、revisionSize、deferred 描述符数）。
 * 任何字段差异都是真实语义差异 —— 修抽取，不改参考输出或归一化规则。
 *
 * 用法：
 *   node scripts/diff-history-read-directory-phase-b.mjs --mode full --sizes 1000,10000
 *   node scripts/diff-history-read-directory-phase-b.mjs --mode full --mode cold --mode hot --sizes 1000,10000
 *
 *   --mode full   无缓存全量模式（disableCache DI：目录路径关闭，走 legacy 全量链）
 *   --mode cold   目录冷构建模式（每个请求独立环境，必为冷构建）
 *   --mode hot    目录热命中模式（同一环境连发两遍同请求，第二遍必命中目录；
 *                 两遍响应都与冻结参考逐字节比对）
 *   --sizes       夹具规模列表（默认 1000,10000）
 *   --seed        夹具种子（默认 20260910，与参考输出一致）
 *   --manifest    参考清单路径（默认 artifacts/history-read-directory/reference-outputs/reference-manifest.json）
 *   --report      差分报告输出路径（默认 stdout；指定时同时落盘 JSON）
 */

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE_AUDIT = path.join(ROOT, "artifacts", "history-read-directory", "fixture-audit.json");

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const ALLOWED_FIXTURE_TIMES = new Set(["09:00:00", "10:00:00"]);
const ISO_TIME_RE = /\d{4}-\d{2}-\d{2}T(\d{2}:\d{2}:\d{2})Z/g;
const SUPPORTED_MODES = new Set(["full", "cold", "hot"]);

// ── CLI（未知参数报错、不吞退出码，与 A04 采集脚本同风格）──
function parseArgs(argv) {
  const args = { modes: [], sizes: [1000, 10000], seed: 20260910, manifest: null, report: null };
  const known = new Set(["--mode", "--sizes", "--seed", "--manifest", "--report", "--help"]);
  for (let i = 2; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help") {
      console.log("Usage: node scripts/diff-history-read-directory-phase-b.mjs --mode full --mode cold --mode hot --sizes 1000,10000 [--seed 20260910] [--manifest <path>] [--report <path>]");
      process.exit(0);
    }
    if (!known.has(arg)) {
      console.error(`未知参数：${arg}（支持：--mode --sizes --seed --manifest --report --help）`);
      process.exit(2);
    }
    const value = argv[i + 1];
    if (value === undefined) {
      console.error(`参数 ${arg} 缺少值`);
      process.exit(2);
    }
    if (arg === "--mode") {
      if (!SUPPORTED_MODES.has(value)) {
        console.error(`不支持的 --mode：${value}（支持：full / cold / hot）`);
        process.exit(2);
      }
      if (!args.modes.includes(value)) args.modes.push(value);
    } else if (arg === "--sizes") args.sizes = value.split(",").map((s) => Number(s.trim()));
    else if (arg === "--seed") args.seed = Number(value);
    else if (arg === "--manifest") args.manifest = value;
    else args.report = value;
    i += 1;
  }
  if (!args.modes.length) args.modes = ["full"];
  if (!args.sizes.length || args.sizes.some((n) => !Number.isInteger(n) || n < 1)) {
    console.error("--sizes 必须为正整数列表");
    process.exit(2);
  }
  if (!Number.isInteger(args.seed)) {
    console.error("--seed 必须为整数");
    process.exit(2);
  }
  return args;
}

const args = parseArgs(process.argv);
const manifestPath = args.manifest
  ? path.resolve(ROOT, args.manifest)
  : path.join(ROOT, "artifacts", "history-read-directory", "reference-outputs", "reference-manifest.json");
if (!fs.existsSync(manifestPath)) {
  console.error(`参考清单不存在：${manifestPath}`);
  process.exit(3);
}
const referenceManifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
if (referenceManifest.seed !== args.seed) {
  console.error(`参考清单 seed=${referenceManifest.seed} 与 --seed ${args.seed} 不一致（跨 seed 的参考不可比）`);
  process.exit(3);
}

// ── 归一化（reference-outputs/normalization-rules.md R1–R5 的实现，与
//    scripts/collect-history-read-directory-reference.mjs 逐字一致）──
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
      throw new Error(`归一化后输出出现非夹具固定时间戳（${match[0]}）——运行污染，按 R4 报错`);
    }
  }
}

function sha256NormalizedOf(data, tmpRoot, ctx) {
  const normalized = normalizeValue(data, tmpRoot, ctx, "$");
  const normalizedText = `${JSON.stringify(normalized, null, 2)}\n`;
  assertNoClockLeak(normalizedText);
  return {
    sha256: createHash("sha256").update(normalizedText, "utf8").digest("hex"),
    normalized,
  };
}

function deferredDescriptorCount(messages) {
  let count = 0;
  const walkDeferred = (v) => {
    if (Array.isArray(v)) { v.forEach(walkDeferred); return; }
    if (v !== null && typeof v === "object") {
      if (typeof v.kind === "string" && Number.isFinite(v.size) && v.available === true && typeof v.id === "string") count += 1;
      Object.values(v).forEach(walkDeferred);
    }
  };
  walkDeferred(messages ?? []);
  return count;
}

function keyFieldsOf(data) {
  const messages = data.messages ?? [];
  const first = messages[0] ?? null;
  const last = messages[messages.length - 1] ?? null;
  return {
    messageCount: messages.length,
    firstMessage: first ? { id: first.id, sourceIndex: first.sourceIndex, role: first.role } : null,
    lastMessage: last ? { id: last.id, sourceIndex: last.sourceIndex, role: last.role } : null,
    blockCount: (data.blocks ?? []).length,
    todos: data.todos === null ? null : (data.todos ?? []).length,
    hasMore: data.hasMore,
    nextBefore: data.nextBefore,
    sessionFilesCount: (data.sessionFiles ?? []).length,
    revisionSize: Number(String(data.revision ?? "0:").split(":")[0]),
    deferredDescriptorCount: deferredDescriptorCount(messages),
  };
}

/** 找出两个归一化 JSON 的第一个差异路径（sha 不一致时的诊断输出）。 */
function firstDiffPath(a, b, at = "$") {
  if (a === b) return null;
  if (typeof a !== typeof b || a === null || b === null || Array.isArray(a) !== Array.isArray(b)) return `${at}: ${JSON.stringify(a)} !== ${JSON.stringify(b)}`;
  if (Array.isArray(a)) {
    if (a.length !== b.length) return `${at}: 数组长度 ${a.length} !== ${b.length}`;
    for (let i = 0; i < a.length; i += 1) {
      const d = firstDiffPath(a[i], b[i], `${at}[${i}]`);
      if (d) return d;
    }
    return null;
  }
  if (typeof a === "object") {
    const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
    for (const key of keys) {
      if (!(key in a)) return `${at}.${key}: 仅新输出存在 ${JSON.stringify(b[key])}`;
      if (!(key in b)) return `${at}.${key}: 仅参考存在 ${JSON.stringify(a[key])}`;
      const d = firstDiffPath(a[key], b[key], `${at}.${key}`);
      if (d) return d;
    }
    return null;
  }
  return `${at}: ${JSON.stringify(a)} !== ${JSON.stringify(b)}`;
}

// 与 A04 requestSetFor 同一口径的固定请求集合。
function requestSetFor(n) {
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

// ── 主流程 ──
const fx = await import("./lib/history-read-fixture.mjs");

const fixtureBytesByN = {};
for (const n of args.sizes) fixtureBytesByN[n] = fx.buildLongRunFixtureBytes(n);
const auditCheck = fx.verifyFixtureAgainstAudit(fixtureBytesByN, FIXTURE_AUDIT);

const mods = await fx.loadProductionModules();
const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-history-diff-"));

function referenceEntryFor(n, req) {
  const fixtureId = `longrun-n${n}-seed${args.seed}`;
  return referenceManifest.entries.find((e) => e.fixtureId === fixtureId && e.requestLabel === req.label) || null;
}

async function replayOneRequest(env, { req, refEntry, n }) {
  const url = fx.messagesUrl(env.sessionPath, { before: req.before, limit: req.limit, all: req.all });
  const res = await env.app.request(url);
  const rawBody = await res.text();
  if (res.status !== 200) {
    throw new Error(`重放请求 n=${n} ${req.label} 返回 ${res.status}：${rawBody.slice(0, 200)}`);
  }
  const postSha256 = fx.sha256(fs.readFileSync(env.sessionPath));
  const fileUnchanged = postSha256 === env.preSha256;

  const data = JSON.parse(rawBody);
  const ctx = { normalizedFields: [], revisionNormalized: false };
  const { sha256, normalized } = sha256NormalizedOf(data, env.envRoot, ctx);

  const fields = keyFieldsOf(data);
  const refFields = refEntry.keyFields;
  const fieldDiffs = [];
  for (const [key, value] of Object.entries(fields)) {
    if (JSON.stringify(refFields[key]) !== JSON.stringify(value)) {
      fieldDiffs.push({ field: key, reference: refFields[key], actual: value });
    }
  }
  const shaMatch = sha256 === refEntry.sha256Normalized;
  const diff = shaMatch ? null : firstDiffPath(
    JSON.parse(fs.readFileSync(path.join(path.dirname(manifestPath), refEntry.output), "utf8")),
    normalized,
  );
  return {
    ok: shaMatch && fieldDiffs.length === 0 && fileUnchanged,
    shaMatch,
    fieldDiffs,
    fileUnchanged,
    sha256Normalized: sha256,
    firstDiff: diff,
  };
}

async function replayRequest({ n, req, refEntry, mode }) {
  // full/cold：独立环境单请求；hot：同一环境连发两遍同请求（第一遍冷构建、
  // 第二遍必命中目录），两遍响应都与冻结参考比对。
  const envRoot = fs.mkdtempSync(path.join(tmpRoot, `env-${n}-`));
  const env = fx.createBenchEnvironment({ mods, counters: null, n, rootDir: envRoot, fixtureBytes: fixtureBytesByN[n] });
  env.envRoot = envRoot;
  const runs = [];
  try {
    const engineOverrides = mode === "full" ? { historyReadDisableCache: true } : {};
    await fx.attachBenchApp(env, { mods, counters: null, engineOverrides });
    const attempts = mode === "hot" ? 2 : 1;
    for (let attempt = 1; attempt <= attempts; attempt += 1) {
      const result = await replayOneRequest(env, { req, refEntry, n });
      runs.push({ attempt, ...result });
    }
  } finally {
    fx.teardownBenchEnvironment(env);
    fs.rmSync(env.envRoot, { recursive: true, force: true });
  }
  const ok = runs.every((run) => run.ok);
  const bad = runs.find((run) => !run.ok) ?? runs[runs.length - 1];
  return {
    fixtureId: refEntry.fixtureId,
    label: req.label,
    mode,
    ok,
    shaMatch: runs.every((run) => run.shaMatch),
    fieldDiffs: bad.fieldDiffs,
    fileUnchanged: runs.every((run) => run.fileUnchanged),
    sha256Normalized: bad.sha256Normalized,
    referenceSha256Normalized: refEntry.sha256Normalized,
    firstDiff: bad.firstDiff,
    runs: runs.length,
  };
}

const results = [];
let failed = 0;
try {
  console.log(`阶段 B 差分：modes=${args.modes.join("/")} sizes=${args.sizes.join(",")} seed=${args.seed}`);
  console.log(`参考清单：${path.relative(ROOT, manifestPath)}（gitHead=${referenceManifest.gitHead}）`);
  for (const mode of args.modes) {
    for (const n of args.sizes) {
      for (const req of requestSetFor(n)) {
        const refEntry = referenceEntryFor(n, req);
        if (!refEntry) {
          console.error(`参考清单缺少条目：n=${n} ${req.label}（先运行 A04 采集脚本补齐参考）`);
          process.exit(3);
        }
        const result = await replayRequest({ n, req, refEntry, mode });
        results.push(result);
        if (!result.ok) failed += 1;
        const tag = result.ok ? "PASS" : "FAIL";
        console.log(
          `[${tag}] mode=${mode} n=${n} ${req.label}: sha=${result.shaMatch ? "=" : "≠"} fields=${result.fieldDiffs.length === 0 ? "=" : "≠"} ` +
            `fileUnchanged=${result.fileUnchanged} sha256Normalized=${result.sha256Normalized.slice(0, 12)}…`,
        );
        if (!result.ok) {
          for (const d of result.fieldDiffs) {
            console.error(`    字段差异 ${d.field}: 参考=${JSON.stringify(d.reference)} 实际=${JSON.stringify(d.actual)}`);
          }
          if (result.firstDiff) console.error(`    首个归一化差异：${result.firstDiff}`);
        }
      }
    }
  }

  const summary = {
    task: "阶段 B 目录路径差分（普通分页 vs A04 冻结参考；full=disableCache 全量 / cold=冷构建 / hot=热命中）",
    generatedAt: new Date().toISOString(),
    modes: args.modes,
    seed: args.seed,
    sizes: args.sizes,
    manifest: path.relative(ROOT, manifestPath),
    referenceGitHead: referenceManifest.gitHead,
    fixtureAuditCrossCheck: auditCheck,
    total: results.length,
    passed: results.length - failed,
    failed,
    allIdentical: failed === 0,
    results,
  };
  const summaryText = JSON.stringify(summary, null, 2);
  if (args.report) {
    const reportPath = path.resolve(ROOT, args.report);
    fs.mkdirSync(path.dirname(reportPath), { recursive: true });
    fs.writeFileSync(reportPath, `${summaryText}\n`, "utf8");
    console.log(`差分报告：${path.relative(ROOT, reportPath)}`);
  }
  if (failed > 0) {
    console.error(`差分失败：${failed}/${results.length} 条不一致 —— 修抽取，不得改参考输出或归一化规则`);
    process.exitCode = 1;
  } else {
    console.log(`差分通过：${results.length}/${results.length} sha256Normalized 逐字节一致`);
  }
} catch (error) {
  console.error("[diff] 运行失败：", error);
  process.exitCode = 1;
} finally {
  try {
    fs.rmSync(tmpRoot, { recursive: true, force: true });
  } catch {
    console.warn(`[diff] 临时目录清理失败（保留待查）：${tmpRoot}`);
  }
}
