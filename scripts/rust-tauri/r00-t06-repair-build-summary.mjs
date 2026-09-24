#!/usr/bin/env node
/**
 * R00-T06｜R1 修复（F02）：从盘上原始被测产物重算 build-release-summary 指纹
 *
 * 背景：r00-t06-build-release.mjs 首轮存在 darwin→mac 目录归一化缺失（仅 dist
 * 归一化，dist-server / dist-server-artifact 未归一化），导致原
 * build-release-summary.json 的 serverBundle（bytes=0 / bundleSha256=null）与
 * seedArchiveDir（空）失真。修复轮不改产物、不重建构建：被测产物自原始构建
 * （builtAt=2026-09-23T21:38Z）起一直在盘、未被触碰，本脚本用修正后的路径与
 * 双口径（statSync 逻辑字节 + du 分配字节）重新盘点，并回填原 summary。
 *
 * 字节一致性守卫（不通过即非零退出，绝不写入猜测值）：
 *   重算的 dist-server bundle/index.js SHA-256 必须等于最终 server run
 *   （run-2026-09-23T23-12-26-989Z）逐样本记录的 serverBundleSha256
 *   （5095cc7f…）——证明盘上产物与被测字节一致；app.asar SHA-256 必须与
 *   原 summary 记录一致（该块首轮路径正确，本就真实）。
 *
 * 用法：node scripts/rust-tauri/r00-t06-repair-build-summary.mjs
 * 输出：覆写 artifacts/rust-tauri/R00/T06/raw/build-release-summary.json
 *   （保留原 buildId/startedAt/finishedAt/git 快照；新增 rebuilt=false、
 *    recomputedAt、recomputeReason、双口径字段；原失真字段值在
 *    repairNote 中留档对照。）
 */
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..", "..");
const OS_DIR = process.platform === "darwin" ? "mac" : process.platform;
const PA = `${OS_DIR}-${process.arch}`;

const SUMMARY_PATH = path.join(ROOT, "artifacts", "rust-tauri", "R00", "T06", "raw", "build-release-summary.json");
const SERVER_RUN_SUMMARY = path.join(ROOT, "artifacts", "rust-tauri", "R00", "T06", "raw", "server", "summary-run-2026-09-23T23-12-26-989Z.json");

function fail(msg) {
  console.error(`[repair-build-summary][FAIL] ${msg}`);
  process.exit(1);
}

function sha256File(p) {
  const res = spawnSync("shasum", ["-a", "256", p], { encoding: "utf8" });
  if (res.status !== 0) fail(`shasum failed: ${p}`);
  return res.stdout.trim().split(/\s+/)[0];
}

function dirSizeBytes(p) {
  let total = 0;
  const entries = fs.readdirSync(p, { withFileTypes: true });
  for (const e of entries) {
    const fp = path.join(p, e.name);
    if (e.isDirectory()) total += dirSizeBytes(fp);
    else total += fs.statSync(fp).size;
  }
  return total;
}

function duDirBytes(p) {
  const res = spawnSync("du", ["-sk", p], { encoding: "utf8" });
  if (res.status !== 0) fail(`du failed: ${p}`);
  return Number(res.stdout.trim().split(/\s+/)[0]) * 1024;
}

function statMtimeIso(p) {
  return new Date(fs.statSync(p).mtimeMs).toISOString();
}

// ── 0. 载入原始 summary（保留其真实构建元数据） ──
if (!fs.existsSync(SUMMARY_PATH)) fail("missing build-release-summary.json");
const original = JSON.parse(fs.readFileSync(SUMMARY_PATH, "utf8"));

// ── 1. 守卫：盘上产物目录存在且与被测字节一致 ──
const serverDir = path.join(ROOT, "dist-server", PA);
const appDir = path.join(ROOT, "dist", PA, "Lingxi.app");
const seedDir = path.join(ROOT, "dist-server-artifact", PA);
for (const d of [serverDir, appDir, seedDir]) {
  if (!fs.existsSync(d)) fail(`artifact dir missing: ${d}（原始构建产物不在盘，无法不重建地重算；需另获授权重建并重测）`);
}

const serverBundlePath = path.join(serverDir, "bundle", "index.js");
const recomputedBundleSha = sha256File(serverBundlePath);
const measuredRunSummary = JSON.parse(fs.readFileSync(SERVER_RUN_SUMMARY, "utf8"));
const measuredBundleSha = measuredRunSummary.serverBundleSha256;
if (recomputedBundleSha !== measuredBundleSha) {
  fail(`byte-identity guard failed: on-disk bundle ${recomputedBundleSha} ≠ final server run recorded ${measuredBundleSha}`);
}
const originalAsarSha = original.artifacts?.desktopApp?.mainBundleSha256 ?? null;
const recomputedAsarSha = sha256File(path.join(appDir, "Contents", "Resources", "app.asar"));
if (originalAsarSha && recomputedAsarSha !== originalAsarSha) {
  fail(`byte-identity guard failed: on-disk app.asar ${recomputedAsarSha} ≠ original summary ${originalAsarSha}`);
}

// ── 2. 双口径重盘点 ──
function inventory(p) {
  return {
    path: path.relative(ROOT, p),
    logicalBytes: dirSizeBytes(p),
    logicalBytesCaliber: "statSync size 逐文件求和（逻辑字节）",
    duBytes: duDirBytes(p),
    duBytesCaliber: "du -sk（APFS 分配字节；协议 §9 分发占用冻结口径，G3 25% 收益对照只用它）",
    exists: true,
  };
}

const seedFiles = fs.readdirSync(seedDir).map((f) => ({
  file: f,
  bytes: fs.statSync(path.join(seedDir, f)).size,
  sha256: sha256File(path.join(seedDir, f)),
}));

const repaired = {
  ...original,
  recomputedAt: new Date().toISOString(),
  rebuilt: false,
  recomputeReason: "R1 修复 F02：原 summary 因 darwin→mac 路径 bug 将 serverBundle/seedArchiveDir 记为 0/null/空；产物未重建（rebuilt=false，盘上即 2026-09-23T21:38Z 原始构建），指纹与双口径大小自此重算。字节一致性守卫：重算 server bundle SHA-256 == 最终 server run（run-2026-09-23T23-12-26-989Z）逐样本记录值；app.asar SHA-256 == 原 summary 记录值。",
  artifactMtimes: {
    serverBundleIndexJs: statMtimeIso(serverBundlePath),
    appAsar: statMtimeIso(path.join(appDir, "Contents", "Resources", "app.asar")),
    seedArchiveDir: statMtimeIso(seedDir),
  },
  repairNote: {
    originalServerBundle: original.artifacts?.serverBundle ?? null,
    originalSeedArchiveDir: original.artifacts?.seedArchiveDir ?? null,
    reason: "原始值由路径 bug 产生（dist-server/darwin-arm64 不存在 → 0/null），非真实测量，留档仅为对照。",
  },
  artifacts: {
    serverBundle: {
      ...inventory(serverDir),
      bundleSha256: recomputedBundleSha,
    },
    desktopApp: {
      ...inventory(appDir),
      mainBundleSha256: recomputedAsarSha,
    },
    seedArchiveDir: {
      ...inventory(seedDir),
      files: seedFiles,
    },
  },
};

fs.writeFileSync(SUMMARY_PATH, JSON.stringify(repaired, null, 2));
console.log(JSON.stringify({
  ok: true,
  summaryPath: path.relative(ROOT, SUMMARY_PATH),
  guards: { serverBundleShaMatchesFinalRun: true, asarShaMatchesOriginalSummary: true },
  serverBundle: { logicalBytes: repaired.artifacts.serverBundle.logicalBytes, duBytes: repaired.artifacts.serverBundle.duBytes, bundleSha256: recomputedBundleSha },
  desktopApp: { logicalBytes: repaired.artifacts.desktopApp.logicalBytes, duBytes: repaired.artifacts.desktopApp.duBytes, mainBundleSha256: recomputedAsarSha },
  seedArchiveDir: { logicalBytes: repaired.artifacts.seedArchiveDir.logicalBytes, duBytes: repaired.artifacts.seedArchiveDir.duBytes, files: seedFiles.length },
}, null, 2));
