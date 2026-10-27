/**
 * R00-T06｜共享测量库
 *
 * 冻结口径（见 docs/rust-tauri/R00/R00-T06_PROTOCOL.md）：
 * - 进程内存：macOS 按 `ps -o rss=`（KB）逐进程读取，进程树求和，不去重共享页。
 *   macOS 无 PSS；RSS 求和是保守上界（Chromium 系多进程共享页会被重复计入），
 *   每样本同时保留逐进程明细，供未来换口径复算。
 * - 进程树归属：每次采样都从根 PID 重新 `pgrep -P` 递归遍历，
 *   与 ps 快照同窗口核对父链（R00-A11：主进程与全部辅助进程同一采样窗口计入）。
 * - 统计：n、mean、median（线性插值）、p95（nearest-rank）、min、max、
 *   stdev（样本）、CV；原始逐样本数据始终落盘。
 */
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export const NS_PER_SEC = 1e9;

export function nowMs() {
  return Number(process.hrtime.bigint() / 10000n) / 100; // 0.01ms 精度
}

export function isoNow() {
  return new Date().toISOString();
}

// ── 宿主环境快照 ──
export function collectHostInfo({ repoRoot = process.cwd() } = {}) {
  const info = {
    capturedAt: isoNow(),
    platform: process.platform,
    arch: process.arch,
    nodeVersion: process.version,
    totalMemBytes: os.totalmem(),
    cpuModel: null,
    cpuCores: os.cpus().length,
    hwModel: null,
    osVersion: `${os.type()} ${os.release()}`,
    swVers: null,
    power: {},
    loadavg: os.loadavg(),
    uptimeSec: Math.round(os.uptime()),
  };
  try {
    info.cpuModel = execFileSync("sysctl", ["-n", "machdep.cpu.brand_string"], { encoding: "utf8" }).trim();
    info.hwModel = execFileSync("sysctl", ["-n", "hw.model"], { encoding: "utf8" }).trim();
  } catch {}
  if (process.platform === "darwin") {
    try {
      info.swVers = execFileSync("sw_vers", { encoding: "utf8" })
        .split("\n").filter(Boolean).map((l) => l.trim()).join("; ");
      const pmset = execFileSync("pmset", ["-g"], { encoding: "utf8" });
      const lowPower = pmset.match(/lowpowermode\s+(\d+)/);
      info.power.lowPowerMode = lowPower ? lowPower[1] === "1" : null;
      try {
        const batt = execFileSync("pmset", ["-g", "batt"], { encoding: "utf8" });
        const src = batt.match(/Now drawing from '([^']+)'/);
        info.power.source = src ? src[1] : null;
      } catch {}
    } catch {}
  }
  try {
    info.gitHead = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim();
    info.gitBranch = execFileSync("git", ["rev-parse", "--abbrev-ref", "HEAD"], { cwd: repoRoot, encoding: "utf8" }).trim();
  } catch {}
  return info;
}

// ── 进程树 ──
function psField(pids) {
  if (!pids.length) return [];
  const out = spawnSync("ps", ["-o", "pid=,ppid=,rss=,vsz=,pcpu=,etime=,comm=", "-p", pids.join(",")], {
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  });
  if (out.status !== 0) return [];
  return out.stdout.split("\n").filter(Boolean).map((line) => {
    const parts = line.trim().split(/\s+/);
    if (parts.length < 7) return null;
    return {
      pid: Number(parts[0]),
      ppid: Number(parts[1]),
      rssKb: Number(parts[2]),
      vszKb: Number(parts[3]),
      pcpu: Number(parts[4]),
      etime: parts[5],
      comm: parts.slice(6).join(" "),
    };
  }).filter(Boolean);
}

/** 递归收集 rootPid 的全部后代 PID（含自身）。每采样重新遍历。 */
export function collectTreePids(rootPids) {
  const seen = new Set();
  const frontier = [...rootPids];
  while (frontier.length) {
    const pid = frontier.pop();
    if (seen.has(pid)) continue;
    seen.add(pid);
    let children = [];
    try {
      const out = execFileSync("pgrep", ["-P", String(pid)], { encoding: "utf8" });
      children = out.split("\n").filter(Boolean).map(Number).filter(Number.isFinite);
    } catch { /* 无子进程或进程已退出 */ }
    frontier.push(...children);
  }
  return [...seen];
}

/**
 * 一次进程树快照：树遍历 + ps 同窗口读数。
 * 返回 { ts, roots, pids, procs, treeRssKb }；进程消失的 PID 从 procs 中剔除
 * 并记进 vanished（采样窗口内退出）。
 */
export function snapshotTree(rootPids, label = "") {
  const pids = collectTreePids(rootPids);
  const procs = psField(pids);
  const treeRssKb = procs.reduce((s, p) => s + p.rssKb, 0);
  return {
    ts: isoNow(),
    label,
    roots: [...rootPids],
    pids,
    procs,
    treeRssKb,
    vanished: pids.filter((p) => !procs.some((q) => q.pid === p)),
  };
}

/**
 * 固定采样窗口：samples 次、intervalMs 间隔的连续快照。
 * R00-A11：同一窗口内主进程与全部辅助进程一并计入。
 */
export async function sampleTreeWindow(rootPids, { samples = 10, intervalMs = 500, label = "" } = {}) {
  const frames = [];
  for (let i = 0; i < samples; i++) {
    frames.push(snapshotTree(rootPids, `${label}#${i}`));
    if (i < samples - 1) await sleep(intervalMs);
  }
  const rssSeries = frames.map((f) => f.treeRssKb);
  return {
    label,
    startedAt: frames[0]?.ts ?? null,
    endedAt: frames[frames.length - 1]?.ts ?? null,
    frames,
    rssSeriesKb: rssSeries,
    rssMedianKb: median(rssSeries),
    rssMaxKb: Math.max(...rssSeries, 0),
    procCountByFrame: frames.map((f) => f.procs.length),
  };
}

// ── 统计 ──
export function percentile(sorted, p) {
  if (!sorted.length) return null;
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[Math.max(0, idx)];
}

export function median(values) {
  if (!values?.length) return null;
  const s = [...values].sort((a, b) => a - b);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

export function stats(values) {
  const nums = (values || []).filter((v) => Number.isFinite(v)).map(Number);
  if (!nums.length) return { n: 0 };
  const sorted = [...nums].sort((a, b) => a - b);
  const n = nums.length;
  const mean = nums.reduce((s, v) => s + v, 0) / n;
  const variance = n > 1 ? nums.reduce((s, v) => s + (v - mean) ** 2, 0) / (n - 1) : 0;
  const stdev = Math.sqrt(variance);
  return {
    n,
    mean,
    median: median(nums),
    p95: percentile(sorted, 95),
    min: sorted[0],
    max: sorted[n - 1],
    stdev,
    cv: mean !== 0 ? stdev / Math.abs(mean) : null,
    unit: "ms",
  };
}

// ── 杂项 ──
export function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

export function sha256File(p) {
  return createHash("sha256").update(fs.readFileSync(p)).digest("hex");
}

export function sha256Bytes(buf) {
  return createHash("sha256").update(buf).digest("hex");
}

export function writeJson(p, obj) {
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, JSON.stringify(obj, null, 2));
}

export function appendJsonl(p, obj) {
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.appendFileSync(p, `${JSON.stringify(obj)}\n`);
}

export function readJson(p) {
  return JSON.parse(fs.readFileSync(p, "utf8"));
}

/** 等待 PID 退出；返回耗时 ms。maxWaitMs 到期返回 null。 */
export async function waitForPidExit(pid, maxWaitMs, pollMs = 50) {
  const t0 = nowMs();
  const deadline = t0 + maxWaitMs;
  for (;;) {
    const alive = (() => {
      try { process.kill(pid, 0); return true; } catch { return false; }
    })();
    if (!alive) return nowMs() - t0;
    if (nowMs() >= deadline) return null;
    await sleep(pollMs);
  }
}

/** 等待整棵进程树（含后代）退出；返回耗时 ms 或 null。 */
export async function waitForTreeExit(rootPid, maxWaitMs, pollMs = 50) {
  const t0 = nowMs();
  const deadline = t0 + maxWaitMs;
  for (;;) {
    const pids = collectTreePids([rootPid]);
    const alive = pids.filter((pid) => {
      try { process.kill(pid, 0); return true; } catch { return false; }
    });
    if (!alive.length) return nowMs() - t0;
    if (nowMs() >= deadline) return null;
    await sleep(pollMs);
  }
}

export function sigkillTree(rootPid) {
  const pids = collectTreePids([rootPid]);
  for (const pid of pids) {
    try { process.kill(pid, "SIGKILL"); } catch {}
  }
  return pids;
}
