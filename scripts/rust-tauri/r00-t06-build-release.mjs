#!/usr/bin/env node
/**
 * R00-T06｜发布优化构建编排（可重复）
 *
 * 在当前 HEAD 上重建本任务性能测量所用的全部"发布优化"产物：
 *   1. 一次性 ed25519 测试签名密钥（OS 临时目录，绝不进入仓库/交付物；
 *      密钥仅用于本地 seed 签名自洽，不是发布密钥——AGENTS.md 红线：
 *      测试签名 key 不得当正式发布 key）
 *   2. build:client（main/preload/renderer/splash/theme 的 Vite 生产构建，
 *      LINGXI_SIGN_KEYSET 构建期替换 main bundle 内联 keyset）
 *   3. build:server（dist-server/<os>-<arch> 生产 bundle + 签名 seed 归档）
 *   4. fetch:bundled-bins / computer-use / speech helpers（幂等）
 *   5. verify:seed-kit（fail-closed 校验）
 *   6. electron-builder --dir（ad-hoc 签名，SKIP_NOTARIZE=true，不触外部服务）
 *
 * 产物：
 *   dist-server/<os>-<arch>/            —— server 腿被测对象（生产 bundle）
 *   dist/mac-<arch>/Lingxi.app          —— 桌面腿被测对象（--dir 打包）
 *   dist-server-artifact/<os>-<arch>/   —— seed 归档（分发占用测量）
 *
 * 用法：node scripts/rust-tauri/r00-t06-build-release.mjs [--workdir <dir>]
 * 退出码 0 = 全链成功；任一步失败非零退出并保留日志。
 */
import { execFileSync, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..", "..");

const args = process.argv.slice(2);
const workdirArgIdx = args.indexOf("--workdir");
const WORKDIR = workdirArgIdx >= 0
  ? path.resolve(args[workdirArgIdx + 1])
  : fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-r00-t06-build-"));
const KEY_DIR = path.join(WORKDIR, "keys");
const PRIV_KEY = path.join(KEY_DIR, "test-sign-key.pem");
const KEYSET = path.join(KEY_DIR, "test-keyset.json");
const OS_DIR_NAME = process.platform === "darwin" ? "mac" : process.platform;
// dist-server / dist-server-artifact 与 dist 同用 build 脚本的 osDirName 约定
// （darwin→mac）；此前只对 dist 归一化，导致 server/seed 盘点路径落空（R1-F02）。
const PLATFORM_ARCH = `${OS_DIR_NAME}-${process.arch}`;

function log(step, msg) {
  console.log(`[r00-t06-build][${step}] ${msg}`);
}

function run(step, cmd, cmdArgs, opts = {}) {
  const label = `${cmd} ${cmdArgs.join(" ")}`;
  log(step, `run: ${label}`);
  // 本机 npm 直连 registry 被拒（ECONNREFUSED 重试后落 stale cache，每包 ~70s）。
  // prefer-offline + 关 audit/fund：只用本地 npm 缓存，不发起 registry 探活。
  const npmOfflineEnv = {
    npm_config_prefer_offline: "true",
    npm_config_audit: "false",
    npm_config_fund: "false",
    npm_config_update_notifier: "false",
  };
  const res = spawnSync(cmd, cmdArgs, {
    cwd: ROOT,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, ...npmOfflineEnv, ...opts.env },
    maxBuffer: 64 * 1024 * 1024,
  });
  const out = (res.stdout || "") + (res.stderr || "");
  if (res.status !== 0) {
    process.stderr.write(out);
    throw new Error(`step ${step} failed (exit ${res.status}): ${label}`);
  }
  return { stdout: res.stdout || "", stderr: res.stderr || "", all: out };
}

function dirSizeBytes(p) {
  let total = 0;
  let entries = [];
  try { entries = fs.readdirSync(p, { withFileTypes: true }); } catch { return 0; }
  for (const e of entries) {
    const fp = path.join(p, e.name);
    if (e.isDirectory()) total += dirSizeBytes(fp);
    else { try { total += fs.statSync(fp).size; } catch {} }
  }
  return total;
}

/** du -sk 分配字节（APFS 分配口径；协议 §9 分发占用冻结口径，G3 对照只用它）。 */
function duDirBytes(p) {
  const res = spawnSync("du", ["-sk", p], { encoding: "utf8" });
  if (res.status !== 0) return null;
  return Number(res.stdout.trim().split(/\s+/)[0]) * 1024;
}

/**
 * 双口径盘点：logicalBytes（statSync 逻辑字节求和）与 duBytes（du -sk 分配字节）。
 * 两者不可混用：25% 迁移收益对照只取 duBytes（R1-F03）。
 */
function inventory(p) {
  const logicalBytes = dirSizeBytes(p);
  const duBytes = duDirBytes(p);
  return {
    path: path.relative(ROOT, p),
    logicalBytes,
    logicalBytesCaliber: "statSync size 逐文件求和（逻辑字节）",
    duBytes,
    duBytesCaliber: "du -sk（APFS 分配字节；协议 §9 分发占用冻结口径）",
    exists: fs.existsSync(p),
  };
}

function sha256File(p) {
  return createHash("sha256").update(fs.readFileSync(p)).digest("hex");
}

// ── 0. 环境快照 ──
const host = {
  startedAt: new Date().toISOString(),
  platform: process.platform,
  arch: process.arch,
  node: process.version,
  gitHead: execFileSync("git", ["rev-parse", "HEAD"], { cwd: ROOT, encoding: "utf8" }).trim(),
  gitBranch: execFileSync("git", ["rev-parse", "--abbrev-ref", "HEAD"], { cwd: ROOT, encoding: "utf8" }).trim(),
  gitDirty: execFileSync("git", ["status", "--porcelain"], { cwd: ROOT, encoding: "utf8" }).trim().split("\n").filter(Boolean),
};
log("0", `HEAD=${host.gitHead} branch=${host.gitBranch} dirty=${host.gitDirty.length} files`);

// ── 1. 一次性测试签名密钥 ──
fs.mkdirSync(KEY_DIR, { recursive: true });
if (!fs.existsSync(PRIV_KEY)) {
  const keygenOut = run("1", process.execPath, ["scripts/artifact-keygen.mjs", "--out", PRIV_KEY]).stdout;
  // stdout 是多行美化 JSON：{keyId, publicKey}
  let pubLine = null;
  try { pubLine = JSON.parse(keygenOut); } catch {}
  if (!pubLine || typeof pubLine.keyId !== "string" || typeof pubLine.publicKey !== "string") {
    throw new Error("artifact-keygen stdout 未包含 {keyId, publicKey} JSON");
  }
  fs.writeFileSync(KEYSET, JSON.stringify([{ keyId: pubLine.keyId, publicKey: pubLine.publicKey }], null, 2));
  log("1", `test key generated keyId=${pubLine.keyId} (throwaway, lives only under ${WORKDIR})`);
} else {
  log("1", "reusing existing test key");
}
const signEnv = {
  LINGXI_SIGN_KEY: PRIV_KEY,
  LINGXI_SIGN_KEYSET: KEYSET,
};

// ── 2. build:client ──
run("2", "npm", ["run", "build:client"], { env: signEnv });

// ── 3. build:server ──
run("3", "npm", ["run", "build:server"], { env: signEnv });

// ── 4. helpers / bundled bins ──
if (process.platform === "darwin") {
  run("4", "npm", ["run", "build:computer-use-helper"]);
  run("4", "npm", ["run", "build:speech-helper"]);
  run("4", "npm", ["run", "build:speech-permissions"]);
}
run("4", "npm", ["run", "fetch:bundled-bins"]);

// ── 5. verify:seed-kit ──
run("5", "npm", ["run", "verify:seed-kit"], { env: signEnv });

// ── 6. electron-builder --dir（ad-hoc，跳过公证，不触外部服务） ──
// 本机无外网（直连/代理均被拒）：electronDist 指向 node_modules 内已解包的
// Electron 发行目录（app-builder-lib 支持解包目录，逐字节来自同一 42.8.1）。
run("6", "npx", ["electron-builder", "--dir", "--config.electronDist=node_modules/electron/dist"], {
  env: { ...signEnv, CSC_IDENTITY_AUTO_DISCOVERY: "false", SKIP_NOTARIZE: "true" },
});

// ── 7. 产物盘点 ──
const serverDir = path.join(ROOT, "dist-server", PLATFORM_ARCH);
const appDir = path.join(ROOT, "dist", `${OS_DIR_NAME}-${process.arch}`, "Lingxi.app");
const seedArtifactDir = path.join(ROOT, "dist-server-artifact", PLATFORM_ARCH);
for (const dir of [serverDir, appDir, seedArtifactDir]) {
  if (!fs.existsSync(dir)) {
    throw new Error(`构建产物目录缺失：${dir}（此前 darwin→mac 路径 bug 的表现，见 R1-F02）`);
  }
}
const summary = {
  ...host,
  finishedAt: new Date().toISOString(),
  buildId: randomUUID(),
  workdir: WORKDIR,
  signKeyset: "throwaway ed25519（仅本机构建自洽，非发布密钥；密钥文件在 OS 临时目录，不进入交付物）",
  artifacts: {
    serverBundle: {
      ...inventory(serverDir),
      bundleSha256: fs.existsSync(path.join(serverDir, "bundle", "index.js"))
        ? sha256File(path.join(serverDir, "bundle", "index.js")) : null,
    },
    desktopApp: {
      ...inventory(appDir),
      mainBundleSha256: fs.existsSync(path.join(appDir, "Contents", "Resources", "app.asar"))
        ? sha256File(path.join(appDir, "Contents", "Resources", "app.asar")) : null,
    },
    seedArchiveDir: {
      ...inventory(seedArtifactDir),
      files: fs.existsSync(seedArtifactDir)
        ? fs.readdirSync(seedArtifactDir).map((f) => ({
          file: f,
          bytes: fs.statSync(path.join(seedArtifactDir, f)).size,
          sha256: sha256File(path.join(seedArtifactDir, f)),
        })) : [],
    },
  },
};
const outPath = path.join(ROOT, "artifacts", "rust-tauri", "R00", "T06", "raw", "build-release-summary.json");
fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify(summary, null, 2));
log("7", `summary → ${path.relative(ROOT, outPath)}`);
console.log(JSON.stringify({ ok: true, summaryPath: path.relative(ROOT, outPath), workdir: WORKDIR }));
