#!/usr/bin/env node
/**
 * fetch-bundled-binaries.cjs — 构建期抓取随软件分发的内置二进制。
 *
 * 把 rg（代码搜索）、fd（文件查找）、ast-grep（AST 结构化工具）抓到
 * bundled-bin/{mac|win|linux}-{arch}/ 暂存目录，electron-builder 顶层
 * extraResources 按 "bundled-bin/${os}-${arch}/" 装进 app Resources，
 * 运行时由 lib/bundled-bins.ts 定位（内置 → 托管目录 → PATH）。
 *
 * 用法：node scripts/fetch-bundled-binaries.cjs [platform] [arch] [--force]
 * - platform/arch 显式传参（darwin|win32|linux × arm64|x64），默认取当前
 *   进程值。CI 必须显式传 matrix 值——mac x64 job 等交叉场景不能依赖宿主。
 * - 已暂存且冒烟通过的跳过（幂等，可重复打包）；--force 强制重抓。
 * - 下载源与运行时托管下载同一渠道：rg/fd 走 GitHub Releases（资产命名与
 *   lib/pi-sdk/search-tools.ts 的 TOOL_CONFIGS 一致），ast-grep 走 npm
 *   平台包（@ast-grep/cli-*，sha512 校验，真二进制名 ast-grep，包里的 sg
 *   是 412KB 废弃 stub）。
 * - 校验失败、下载失败一律非零退出（fail-closed），绝不静默装出一个空目录。
 */
const { execFileSync, spawnSync } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const NETWORK_TIMEOUT_MS = 10_000;
const SMOKE_TIMEOUT_MS = 15_000;
const MAGIC = {
  machO: Buffer.from([0xcf, 0xfe, 0xed, 0xfa]), // MH_MAGIC_64
  elf: Buffer.from([0x7f, 0x45, 0x4c, 0x46]),
  pe: Buffer.from([0x4d, 0x5a]), // MZ
};
const MIN_BINARY_BYTES = 500_000;

function fail(msg) {
  console.error(`[bundled-bins] ${msg}`);
  process.exit(1);
}

/**
 * 跨设备落位：Windows 托管机 TEMP 在 C:、工作区在 D:，renameSync 跨盘符
 * 抛 EXDEV（v0.1.40-experimental.1 第二次 tag 的 Windows 腿实锤）；
 * 回退 copy+delete，语义与 rename 一致（目标不存在、覆盖无残留）。
 */
function moveAcrossDevices(src, dest) {
  try {
    fs.renameSync(src, dest);
  } catch (err) {
    if (err?.code !== "EXDEV") throw err;
    fs.copyFileSync(src, dest);
    fs.rmSync(src, { force: true });
  }
}

function parseArgs(argv) {
  const positional = [];
  let force = false;
  for (const a of argv) {
    if (a === "--force") force = true;
    else positional.push(a);
  }
  const platform = positional[0] || process.platform;
  const arch = positional[1] || process.arch;
  if (!["darwin", "win32", "linux"].includes(platform)) fail(`unsupported platform: ${platform}`);
  if (!["arm64", "x64"].includes(arch)) fail(`unsupported arch: ${arch}`);
  return { platform, arch, force };
}

function githubLatestTag(repo) {
  // GitHub Actions 托管机共享出口 IP，匿名 API 限流（60 次/小时）经常整段 403，
  // CI 里必须带 GITHUB_TOKEN（workflow 的 fetch 步骤注入）；本地无 token 照旧匿名。
  const authHeader = process.env.GITHUB_TOKEN
    ? `process.env.GITHUB_TOKEN && (headers.Authorization = "Bearer " + process.env.GITHUB_TOKEN);`
    : "";
  const res = spawnSync("node", ["-e", `
    const headers = { "User-Agent": "lingxi-bundled-bins" };
    ${authHeader}
    fetch("https://api.github.com/repos/${repo}/releases/latest", {
      headers,
      signal: AbortSignal.timeout(${NETWORK_TIMEOUT_MS}),
    }).then(r => { if (!r.ok) throw new Error("HTTP " + r.status); return r.json(); })
      .then(j => process.stdout.write(String(j.tag_name || "")))
      .catch(e => { console.error(e.message); process.exit(1); });
  `], { encoding: "utf8", timeout: NETWORK_TIMEOUT_MS + 5_000 });
  if (res.status !== 0 || !res.stdout.trim()) fail(`GitHub latest tag for ${repo}: ${res.stderr || res.status}`);
  return res.stdout.trim();
}

function downloadTo(url, dest) {
  const authHeader = process.env.GITHUB_TOKEN
    ? `process.env.GITHUB_TOKEN && (headers.Authorization = "Bearer " + process.env.GITHUB_TOKEN);`
    : "";
  execFileSync("node", ["-e", `
    const fs = require("node:fs");
    const { Readable } = require("node:stream");
    const { pipeline } = require("node:stream/promises");
    const headers = { "User-Agent": "lingxi-bundled-bins" };
    ${authHeader}
    fetch(process.argv[1], {
      headers,
      signal: AbortSignal.timeout(180_000),
      redirect: "follow",
    }).then(r => {
      if (!r.ok || !r.body) throw new Error("HTTP " + r.status);
      return pipeline(Readable.fromWeb(r.body), fs.createWriteStream(process.argv[2]));
    }).then(() => {
      if (!fs.existsSync(process.argv[2]) || fs.statSync(process.argv[2]).size === 0) {
        throw new Error("empty download");
      }
    }).then(() => process.exit(0)).catch(e => { console.error(e.message); process.exit(1); });
  `, url, dest], { stdio: ["ignore", "inherit", "inherit"], timeout: 200_000 });
}

function extractArchive(archivePath, extractDir) {
  fs.mkdirSync(extractDir, { recursive: true });
  // tar（bsdtar）统一处理 .tar.gz 与 .zip——Win10+ 自带 bsdtar，与运行时下载同一路径
  execFileSync("tar", ["xf", archivePath, "-C", extractDir], { stdio: "ignore", timeout: 120_000 });
}

function findFileRecursively(dir, name) {
  const stack = [dir];
  while (stack.length) {
    const cur = stack.pop();
    let entries;
    try { entries = fs.readdirSync(cur, { withFileTypes: true }); } catch { continue; }
    for (const ent of entries) {
      const full = path.join(cur, ent.name);
      if (ent.isFile() && ent.name === name) return full;
      if (ent.isDirectory()) stack.push(full);
    }
  }
  return null;
}

function looksLikeBinary(filePath) {
  const stat = fs.statSync(filePath);
  if (stat.size < MIN_BINARY_BYTES) return false;
  const fd = fs.openSync(filePath, "r");
  try {
    const head = Buffer.alloc(4);
    fs.readSync(fd, head, 0, 4, 0);
    return MAGIC.machO.equals(head) || MAGIC.elf.equals(head) || head.subarray(0, 2).equals(MAGIC.pe);
  } finally {
    fs.closeSync(fd);
  }
}

/** 宿主能跑就跑 --version；跑不了（交叉架构）退回魔数+体积核验。 */
function smokeOrVerify(binaryPath, platform, arch) {
  const sameArch = process.platform === platform && os.arch() === arch;
  if (sameArch) {
    const res = spawnSync(binaryPath, ["--version"], { timeout: SMOKE_TIMEOUT_MS, stdio: "pipe" });
    if (res.error || res.status !== 0) fail(`smoke --version failed for ${binaryPath}: ${res.stderr || res.error}`);
    console.log(`[bundled-bins] smoke ok: ${path.basename(binaryPath)} ${(res.stdout || "").toString().trim()}`);
    return;
  }
  if (!looksLikeBinary(binaryPath)) fail(`cross-arch magic/size check failed for ${binaryPath}`);
  console.log(`[bundled-bins] cross-arch verified (magic+size): ${binaryPath}`);
}

// 与 lib/pi-sdk/search-tools.ts TOOL_CONFIGS 同源；改资产命名两处必须同步
function rgAssetName(version, platform, arch) {
  const a = arch === "arm64" ? "aarch64" : "x86_64";
  if (platform === "darwin") return `ripgrep-${version}-${a}-apple-darwin.tar.gz`;
  if (platform === "linux") return arch === "arm64"
    ? `ripgrep-${version}-aarch64-unknown-linux-gnu.tar.gz`
    : `ripgrep-${version}-x86_64-unknown-linux-musl.tar.gz`;
  return `ripgrep-${version}-${a}-pc-windows-msvc.zip`;
}

function fdAssetName(version, platform, arch) {
  const a = arch === "arm64" ? "aarch64" : "x86_64";
  if (platform === "darwin") return `fd-v${version}-${a}-apple-darwin.tar.gz`;
  if (platform === "linux") return `fd-v${version}-${a}-unknown-linux-gnu.tar.gz`;
  return `fd-v${version}-${a}-pc-windows-msvc.zip`;
}

function astGrepPlatformPackage(platform, arch) {
  const osPart = platform === "darwin" ? "darwin" : platform === "linux" ? "linux" : platform === "win32" ? "win32" : null;
  const archPart = arch === "arm64" ? "arm64" : arch === "x64" ? "x64" : null;
  if (!osPart || !archPart) return null;
  const suffix = (osPart === "linux" || osPart === "win32") ? (osPart === "linux" ? "-gnu" : "-msvc") : "";
  return `@ast-grep/cli-${osPart}-${archPart}${suffix}`;
}

function fetchGithubTool({ repo, tagPrefix, binaryName, assetName }, tmpDir) {
  const version = githubLatestTag(repo).replace(/^v/, "");
  const asset = assetName(version);
  const url = `https://github.com/${repo}/releases/download/${tagPrefix}${version}/${asset}`;
  const archivePath = path.join(tmpDir, asset);
  downloadTo(url, archivePath);
  const extractDir = path.join(tmpDir, `extract_${binaryName}`);
  extractArchive(archivePath, extractDir);
  // 目标平台档案里的可执行名：win 是 .exe，unix 是裸名——两个候选都试
  const candidates = [
    `${binaryName}.exe`,
    binaryName,
  ].map(name => [
    path.join(extractDir, asset.replace(/\.(tar\.gz|zip)$/, ""), name),
    path.join(extractDir, name),
    findFileRecursively(extractDir, name),
  ]).flat().filter(p => p && fs.existsSync(p));
  if (candidates.length === 0) fail(`${binaryName} binary not found in ${asset}`);
  return { found: candidates[0], version };
}

function fetchNpmPlatformPackage({ packageName }, tmpDir, targetPlatform) {
  const registryJson = name => `https://registry.npmjs.org/${name.replace("/", "%2f")}`;
  const getJson = url => {
    const res = spawnSync("node", ["-e", `
      fetch(process.argv[1], { signal: AbortSignal.timeout(${NETWORK_TIMEOUT_MS}) })
        .then(r => { if (!r.ok) throw new Error("HTTP " + r.status); return r.json(); })
        .then(j => process.stdout.write(JSON.stringify(j)))
        .catch(e => { console.error(e.message); process.exit(1); });
    `, url], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024, timeout: NETWORK_TIMEOUT_MS + 15_000 });
    if (res.status !== 0) fail(`npm metadata for ${packageName}: ${res.stderr}`);
    return JSON.parse(res.stdout);
  };
  const latest = getJson(`https://registry.npmjs.org/${packageName.replace("/", "%2f")}/latest`);
  const version = latest.version;
  if (!version) fail(`no latest version for ${packageName}`);
  const dist = getJson(registryJson(packageName))?.versions?.[version]?.dist;
  if (!dist?.tarball || !dist?.integrity) fail(`${packageName}@${version} has no tarball/integrity`);
  const archivePath = path.join(tmpDir, "ast-grep.tgz");
  downloadTo(dist.tarball, archivePath);
  const integrity = String(dist.integrity);
  const actual = crypto.createHash("sha512").update(fs.readFileSync(archivePath)).digest("base64");
  if (integrity.slice(0, integrity.indexOf("-")) !== "sha512" || actual !== integrity.slice(integrity.indexOf("-") + 1)) {
    fail(`sha512 integrity mismatch for ${packageName}@${version}`);
  }
  const extractDir = path.join(tmpDir, "extract_ast_grep");
  extractArchive(archivePath, extractDir);
  const binaryFileName = targetPlatform === "win32" ? "ast-grep.exe" : "ast-grep";
  const found = findFileRecursively(extractDir, binaryFileName);
  if (!found) fail(`ast-grep binary missing in ${packageName}@${version}`);
  return { found, version };
}

function main() {
  const { platform, arch, force } = parseArgs(process.argv.slice(2));
  const builderOs = platform === "darwin" ? "mac" : platform === "win32" ? "win" : "linux";
  const stageDir = path.resolve(__dirname, "..", "bundled-bin", `${builderOs}-${arch}`);
  const binExt = platform === "win32" ? ".exe" : "";
  const targets = [
    { id: "rg", binaryFileName: `rg${binExt}`, source: "github", repo: "BurntSushi/ripgrep", tagPrefix: "", assetName: v => rgAssetName(v, platform, arch) },
    { id: "fd", binaryFileName: `fd${binExt}`, source: "github", repo: "sharkdp/fd", tagPrefix: "v", assetName: v => fdAssetName(v, platform, arch) },
    { id: "ast-grep", binaryFileName: `ast-grep${binExt}`, source: "npm", packageName: astGrepPlatformPackage(platform, arch) },
  ];

  fs.mkdirSync(stageDir, { recursive: true });
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-bundled-bins-"));
  try {
    for (const t of targets) {
      const dest = path.join(stageDir, t.binaryFileName);
      if (!force && fs.existsSync(dest)) {
        try {
          smokeOrVerify(dest, platform, arch);
          console.log(`[bundled-bins] ${t.id}: 已暂存，跳过（--force 重抓）`);
          continue;
        } catch { /* 暂存损坏则重抓 */ }
      }
      if (t.source === "npm" && !t.packageName) fail(`ast-grep has no platform package for ${platform}/${arch}`);
      const fetched = t.source === "github"
        ? fetchGithubTool({ repo: t.repo, tagPrefix: t.tagPrefix, binaryName: t.binaryFileName.replace(/\.exe$/, ""), assetName: t.assetName }, tmpDir)
        : fetchNpmPlatformPackage({ packageName: t.packageName }, tmpDir, platform);
      moveAcrossDevices(fetched.found, dest);
      if (platform !== "win32") fs.chmodSync(dest, 0o755);
      smokeOrVerify(dest, platform, arch);
      console.log(`[bundled-bins] ${t.id} ${fetched.version} → ${dest}`);
    }
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
  console.log(`[bundled-bins] done: ${stageDir}`);
}

main();
