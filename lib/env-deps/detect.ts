/**
 * 环境依赖探测器。
 *
 * - binary/runtime：execFile(bin, versionArgs) 带 3s 超时，成功即已安装；
 *   路径经 `which`/`where` 尽力解析（失败不阻塞）。
 * - managed：先查托管目录（与 ripgrep/fd 同一机制），找不到再回落 PATH。
 * - 项目感知：对工作区根做浅层扫描（深度≤2、条目封顶），命中信号文件
 *   即标 neededByProject——「你的项目需要它但没装」就来自这里。
 *
 * 结果带 60s 内存缓存；路由与启动自检共用同一入口。
 * 探测永不抛错：任何失败都收敛为 status:"missing" + note。
 */

import { execFile } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { resolveLingxiPiSdkManagedBinDir } from "../../shared/hana-runtime-paths.ts";
import { findBundledBin } from "../bundled-bins.ts";
import { ENV_DEP_ENTRIES, type EnvDepEntry } from "./registry.ts";

const PROBE_TIMEOUT_MS = 3000;
const SCAN_MAX_DEPTH = 2;
const SCAN_MAX_ENTRIES = 2000;
const CACHE_TTL_MS = 60_000;

export interface EnvDepStatus {
  id: string;
  label: string;
  kind: EnvDepEntry["kind"];
  status: "installed" | "missing";
  /** managed 类：true 表示来自应用托管目录，false 表示来自 PATH */
  managed?: boolean;
  version?: string;
  path?: string;
  requiredBy: string[];
  neededByProject: boolean;
  installHint?: string;
  note?: string;
}

export interface EnvDepsReport {
  checkedAt: string;
  durationMs: number;
  deps: EnvDepStatus[];
  summary: {
    total: number;
    installed: number;
    missing: number;
    /** 项目需要但未安装的依赖 id */
    projectMissing: string[];
  };
}

type ProbeFn = (bin: string, args: string[]) => Promise<{ ok: boolean; version?: string; path?: string; note?: string }>;

function extractVersion(output: string): string | undefined {
  const firstLine = output.split(/\r?\n/, 1)[0] || "";
  const m = firstLine.match(/(\d+\.\d+(?:\.\d+)?(?:[-\w.]*)?)/);
  return m ? m[1] : (firstLine.trim().slice(0, 60) || undefined);
}

/** 默认探测器：真实 execFile。测试可注入假的。 */
function defaultProbe(): ProbeFn {
  return (bin, args) => new Promise(resolve => {
    let child;
    try {
      child = execFile(bin, args, { timeout: PROBE_TIMEOUT_MS, maxBuffer: 64 * 1024 }, (err, stdout, stderr) => {
        if (err) {
          resolve({ ok: false, note: err.killed ? "probe_timeout" : "not_found" });
          return;
        }
        resolve({ ok: true, version: extractVersion(`${stdout}\n${stderr}`) });
      });
    } catch {
      resolve({ ok: false, note: "spawn_failed" });
      return;
    }
    child.on("error", () => resolve({ ok: false, note: "spawn_failed" }));
  });
}

function probeBinPath(bin: string): Promise<string | undefined> {
  const cmd = process.platform === "win32" ? "where" : "which";
  return new Promise(resolve => {
    try {
      execFile(cmd, [bin], { timeout: PROBE_TIMEOUT_MS, maxBuffer: 16 * 1024 }, (err, stdout) => {
        if (err) return resolve(undefined);
        resolve(stdout.split(/\r?\n/, 1)[0]?.trim() || undefined);
      });
    } catch {
      resolve(undefined);
    }
  });
}

function managedBinPath(binName: string): string | null {
  try {
    const dir = resolveLingxiPiSdkManagedBinDir();
    const candidates = process.platform === "win32"
      ? [`${binName}.exe`, binName]
      : [binName, `${binName}.exe`];
    for (const name of candidates) {
      const p = path.join(dir, name);
      try {
        fs.accessSync(p, fs.constants.X_OK);
        return p;
      } catch { /* try next */ }
    }
    // 托管目录可能按版本号建子目录，浅扫一层
    let entries: fs.Dirent[] = [];
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return null; }
    for (const ent of entries) {
      if (!ent.isDirectory()) continue;
      for (const name of candidates) {
        const p = path.join(dir, ent.name, name);
        try {
          fs.accessSync(p, fs.constants.X_OK);
          return p;
        } catch { /* try next */ }
      }
    }
  } catch { /* managed dir unavailable */ }
  return null;
}

/** 信号匹配：精确文件名 或 *.ext 后缀。 */
function signalMatches(fileName: string, signal: string): boolean {
  if (signal.startsWith("*.")) return fileName.endsWith(signal.slice(1));
  return fileName === signal;
}

/** 浅层扫描工作区，命中任一信号即返回 true。封顶防御巨大目录。 */
function scanProjectSignals(roots: string[], signals: string[]): boolean {
  if (signals.length === 0) return false;
  let visited = 0;
  const walk = (dir: string, depth: number): boolean => {
    if (depth > SCAN_MAX_DEPTH || visited >= SCAN_MAX_ENTRIES) return false;
    let entries: fs.Dirent[];
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return false;
    }
    for (const ent of entries) {
      if (visited++ >= SCAN_MAX_ENTRIES) return false;
      if (ent.name === "node_modules" || ent.name === ".git" || ent.name.startsWith(".") && ent.name !== ".github") continue;
      if (ent.isFile() && signals.some(s => signalMatches(ent.name, s))) return true;
      if (ent.isDirectory() && walk(path.join(dir, ent.name), depth + 1)) return true;
    }
    return false;
  };
  return roots.some(root => root && typeof root === "string" && walk(root, 0));
}

function pickInstallHint(entry: EnvDepEntry): string | undefined {
  if (!entry.installHint) return undefined;
  const plat = process.platform as "darwin" | "win32" | "linux";
  return entry.installHint[plat] ?? entry.installHint.other;
}

let _cache: { key: string; at: number; report: EnvDepsReport } | null = null;

export interface DetectOptions {
  /** 工作区根列表（用于项目信号扫描）；空数组跳过项目感知 */
  workspaceRoots?: string[];
  force?: boolean;
  /** 测试注入点：替换真实 execFile 探测 */
  probe?: ProbeFn;
  /** 测试注入点：替换托管二进制查找 */
  findManaged?: (binName: string) => string | null;
  /** 测试注入点：替换内置二进制查找（随安装包分发的拷贝） */
  findBundled?: (binName: string) => string | null;
  /** 测试注入点：替换项目信号扫描 */
  scanSignals?: (roots: string[], signals: string[]) => boolean;
}

export async function detectEnvDeps(options: DetectOptions = {}): Promise<EnvDepsReport> {
  const roots = (options.workspaceRoots ?? []).filter(Boolean);
  const cacheKey = roots.join("|");
  if (!options.force && !options.probe && _cache && _cache.key === cacheKey && Date.now() - _cache.at < CACHE_TTL_MS) {
    return _cache.report;
  }
  const startedAt = Date.now();
  const probe = options.probe ?? defaultProbe();
  const findManaged = options.findManaged ?? managedBinPath;
  const findBundled = options.findBundled ?? ((binName: string) => findBundledBin(binName));
  const scanSignals = options.scanSignals ?? scanProjectSignals;

  const deps: EnvDepStatus[] = await Promise.all(ENV_DEP_ENTRIES.map(async entry => {
    const neededByProject = scanSignals(roots, entry.projectSignals ?? []);
    const base = {
      id: entry.id,
      label: entry.label,
      kind: entry.kind,
      requiredBy: entry.requiredBy,
      neededByProject,
      installHint: pickInstallHint(entry),
    };

    // managed 类先查内置（随安装包分发），再查托管目录
    if (entry.kind === "managed" && entry.managedBinName) {
      const managedPath = findBundled(entry.managedBinName) ?? findManaged(entry.managedBinName);
      if (managedPath) {
        const r = await probe(managedPath, entry.versionArgs);
        return { ...base, status: "installed" as const, managed: true, path: managedPath, version: r.version };
      }
    }

    // PATH 探测：候选命令按顺序，第一个成功即收
    for (const bin of entry.binaries) {
      const r = await probe(bin, entry.versionArgs);
      if (r.ok) {
        const binPath = await probeBinPath(bin);
        return { ...base, status: "installed" as const, managed: false, version: r.version, path: binPath };
      }
    }
    return { ...base, status: "missing" as const };
  }));

  const missing = deps.filter(d => d.status === "missing");
  const report: EnvDepsReport = {
    checkedAt: new Date().toISOString(),
    durationMs: Date.now() - startedAt,
    deps,
    summary: {
      total: deps.length,
      installed: deps.length - missing.length,
      missing: missing.length,
      projectMissing: missing.filter(d => d.neededByProject).map(d => d.id),
    },
  };
  if (!options.probe) {
    _cache = { key: cacheKey, at: Date.now(), report };
  }
  return report;
}

/** 仅测试用：清缓存。 */
export function _resetEnvDepsCacheForTest(): void {
  _cache = null;
}
