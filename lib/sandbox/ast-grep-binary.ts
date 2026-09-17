/**
 * ast-grep-binary.ts — ast-grep（sg）托管二进管的解析与按需下载。
 *
 * 分发渠道选 npm 而非 GitHub Releases：ast-grep 的 GitHub 最新 release 只带
 * 桌面 App 包（app-*.zip，体积大且不是 CLI），CLI 的官方分发是 npm 平台包
 * （@ast-grep/cli 的 optionalDependencies 指向 cli-darwin-arm64 等，包内
 * bin/sg 即二进制）。registry.npmjs.org 是普通 HTTPS、无鉴权、命名稳定，
 * 且 dist.integrity 自带 sha512——比 rg/fd 走 GitHub release 还多一层真校验。
 *
 * 落盘沿用 rg/fd 的托管目录 {lingxiHome}/runtime/pi-sdk/bin，env-deps 的
 * managed 探测（managedBinPath）因此天然能看见它。流程：查 latest 版本 →
 * 解析平台包 → 下载 tarball → sha512 校验 → 解出 bin/sg → 冒烟 `--version`
 * → chmod 落盘。离线（PI_OFFLINE）或任何一步失败：返回 null，调用方如实
 * 报错并指路环境依赖页——不静默降级。ensure 结果（含失败）进程内缓存，
 * 一次会话至多试一次网络。
 */
import { execFile, spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pipeline } from "node:stream/promises";
import { Readable } from "node:stream";
import { createWriteStream } from "node:fs";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

export const AST_GREP_NPM_PACKAGE = "@ast-grep/cli";
const VERSION_TIMEOUT_MS = 10_000;
const DOWNLOAD_TIMEOUT_MS = 120_000;
const SMOKE_TIMEOUT_MS = 15_000;

/** 进程内 ensure 缓存：managedBinDir → ensure Promise */
const ensureCache = new Map<string, Promise<{ path: string; version: string } | null>>();

/**
 * 真二进制名是 ast-grep。npm 包里还有个 412KB 的 `sg` 同名 stub（废弃别名，
 * 单独执行会 NotFound），所以托管/探测一律用本名，避免抓到 stub。
 */
function sgBinaryName(): string {
  return process.platform === "win32" ? "ast-grep.exe" : "ast-grep";
}

export function astGrepManagedPath(managedBinDir: string): string {
  return path.join(managedBinDir, sgBinaryName());
}

/**
 * npm optionalDependencies 里的平台包名（@ast-grep/cli 的现行命名）。
 * 与 GitHub release 的资产名是两套体系，别混用。
 */
export function astGrepPlatformPackage(platform: NodeJS.Platform, arch: string): string | null {
  const osPart = platform === "darwin" ? "darwin"
    : platform === "linux" ? "linux"
    : platform === "win32" ? "win32"
    : null;
  if (!osPart) return null;
  const archPart = arch === "arm64" ? "arm64" : arch === "x64" ? "x64" : arch === "ia32" ? "ia32" : null;
  if (!archPart) return null;
  const suffix = (osPart === "linux" || osPart === "win32") ? (osPart === "linux" ? "-gnu" : "-msvc") : "";
  return `@ast-grep/cli-${osPart}-${archPart}${suffix}`;
}

async function fetchJson(url: string): Promise<any> {
  const res = await fetch(url, { signal: AbortSignal.timeout(VERSION_TIMEOUT_MS) });
  if (!res.ok) throw new Error(`registry ${res.status}`);
  return res.json();
}

function findBinaryRecursively(dir: string, name: string): string | null {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      const hit = findBinaryRecursively(full, name);
      if (hit) return hit;
    } else if (entry.name === name) {
      return full;
    }
  }
  return null;
}

async function runSgVersion(sgPath: string): Promise<string> {
  const { stdout } = await execFileAsync(sgPath, ["--version"], { timeout: SMOKE_TIMEOUT_MS });
  return stdout.trim().split("\n")[0] || "unknown";
}

/**
 * 解析 sg：托管目录命中 → 绝对路径；PATH 上有 → 裸命令名；否则 null。
 * PATH 探测沿用 rg/fd 的 `--version` 存活测试。
 */
export function resolveAstGrepBinary(managedBinDir: string | null | undefined): string | null {
  if (managedBinDir) {
    const managed = astGrepManagedPath(managedBinDir);
    if (fs.existsSync(managed)) return managed;
  }
  try {
    const probe = process.platform === "win32" ? "ast-grep.exe" : "ast-grep";
    const probeResult = spawnSync(probe, ["--version"], { timeout: VERSION_TIMEOUT_MS, stdio: "ignore" });
    if (probeResult.error || probeResult.status !== 0) return null;
    return probe;
  } catch {
    return null;
  }
}

async function downloadManaged(managedBinDir: string, log?: { warn?: (msg: string) => void }): Promise<{ path: string; version: string } | null> {
  const pkg = astGrepPlatformPackage(process.platform, process.arch);
  if (!pkg) return null;
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-ast-grep-"));
  try {
    // 1) latest 版本 + 平台包版本锁定（两者版本号一致）
    const latest = await fetchJson(`https://registry.npmjs.org/${AST_GREP_NPM_PACKAGE}/latest`);
    const version = typeof latest?.version === "string" ? latest.version : "";
    if (!version) throw new Error("latest version missing");
    // 2) 平台包 dist（tarball + sha512 integrity）
    const encodedPkg = pkg.replace("/", "%2f");
    const pkgMeta = await fetchJson(`https://registry.npmjs.org/${encodedPkg}`);
    const dist = pkgMeta?.versions?.[version]?.dist;
    if (!dist?.tarball || !dist?.integrity) throw new Error(`platform package ${pkg}@${version} has no tarball/integrity`);
    // 3) 下载并 sha512 校验（integrity 形如 "sha512-<base64>"）
    const archivePath = path.join(tmpDir, "pkg.tgz");
    const res = await fetch(dist.tarball, {
      signal: AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS),
      headers: { "User-Agent": "lingxi-agent" },
    });
    if (!res.ok || !res.body) throw new Error(`download ${res.status}`);
    await pipeline(Readable.fromWeb(res.body as any), createWriteStream(archivePath));
    const integrity = String(dist.integrity);
    const algo = integrity.slice(0, integrity.indexOf("-"));
    const expected = integrity.slice(integrity.indexOf("-") + 1);
    if (algo !== "sha512") throw new Error(`unsupported integrity algo: ${algo}`);
    const actual = crypto.createHash("sha512").update(fs.readFileSync(archivePath)).digest("base64");
    if (actual !== expected) throw new Error("sha512 integrity mismatch");
    // 4) 解出 bin/sg（npm tgz 统一 gzip tar；win32 也走 tar，Win10+ 自带 bsdtar）
    const extractDir = path.join(tmpDir, "extract");
    fs.mkdirSync(extractDir, { recursive: true });
    await execFileAsync("tar", ["xzf", archivePath, "-C", extractDir]);
    const found = findBinaryRecursively(extractDir, sgBinaryName());
    if (!found) throw new Error("sg binary missing in package");
    // 5) 落盘 + 冒烟
    fs.mkdirSync(managedBinDir, { recursive: true });
    const dest = astGrepManagedPath(managedBinDir);
    fs.renameSync(found, dest);
    if (process.platform !== "win32") fs.chmodSync(dest, 0o755);
    return { path: dest, version: await runSgVersion(dest) };
  } catch (err) {
    log?.warn?.(`[ast-grep] managed download failed: ${(err as any)?.message || err}`);
    return null;
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

/**
 * 确保 sg 可用：已有 → 用（冒烟验真，坏文件重下）；缺 → 下载一次（离线跳过）；
 * 失败 → null（不抛）。结果（含失败）进程内缓存，同一会话不再重试网络。
 */
export function ensureAstGrepBinary(options: {
  managedBinDir: string | null | undefined;
  offline?: boolean;
  log?: { warn?: (msg: string) => void };
}): Promise<{ path: string; version: string } | null> {
  const key = options.managedBinDir || "<path-only>";
  const cached = ensureCache.get(key);
  if (cached) return cached;

  const task = (async () => {
    const existing = resolveAstGrepBinary(options.managedBinDir);
    if (existing) {
      try {
        return { path: existing, version: await runSgVersion(existing) };
      } catch {
        // 托管文件存在但执行失败（半截下载/架构不符）——删掉重下。
        if (options.managedBinDir && existing === astGrepManagedPath(options.managedBinDir)) {
          try { fs.rmSync(existing, { force: true }); } catch { /* 尽力 */ }
        } else {
          return { path: existing, version: "unknown" };
        }
      }
    }
    if (options.offline || !options.managedBinDir) return null;
    return downloadManaged(options.managedBinDir, options.log);
  })();

  ensureCache.set(key, task);
  // 失败结果也缓存，但别让 rejection 存进 map（防 unhandled）
  task.catch(() => {});
  return task;
}

/** 测试探针：清空进程内缓存。 */
export function resetAstGrepEnsureCache(): void {
  ensureCache.clear();
}
