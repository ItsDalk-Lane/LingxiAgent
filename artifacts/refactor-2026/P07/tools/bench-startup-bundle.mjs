#!/usr/bin/env node
/**
 * P07-T02 产物形态（bundle）启动计时：node dist-server/<plat>/bootstrap.js（生产
 * 打包 server 的真实启动入口）在隔离 HOME 冷启动，测 spawn→server-info.json。
 * 与源码形态（server/main-full.ts，W1 口径）同机成对，用于 STARTUP_CRITICAL_PATH
 * 的「模块编译成本在产物形态中消失」验证。
 * 用法：node bench-startup-bundle.mjs --runs 10
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import console from "node:console";
import { performance } from "node:perf_hooks";
import { setTimeout } from "node:timers";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const P07 = path.join(REPO, "artifacts", "refactor-2026", "P07");

const argv = process.argv.slice(2);
let runs = 10, batch = "P07BUNDLE";
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--runs") runs = Number(argv[++i]);
  else if (argv[i] === "--batch") batch = argv[++i];
}

const BOOTSTRAP = path.join(REPO, "dist-server", "mac-arm64", "bootstrap.js");
if (!fs.existsSync(BOOTSTRAP)) { console.error("bundle bootstrap 不存在（先 node scripts/build-server.mjs）"); process.exit(2); }

const samples = [];
for (let r = 1; r <= runs; r++) {
  const home = path.join(P07, "isolated", `bench-boot-${batch}-${r}`);
  fs.rmSync(home, { recursive: true, force: true });
  fs.mkdirSync(home, { recursive: true });
  const t0 = performance.now();
  const child = spawn(process.execPath, [BOOTSTRAP], {
    cwd: REPO,
    env: { ...process.env, LINGXI_HOME: home, LINGXI_PORT: "0", LINGXI_TOKEN: `p07-bench-${batch}-${r}` },
    stdio: ["ignore", "ignore", "ignore"],
  });
  const infoPath = path.join(home, "server-info.json");
  const deadline = Date.now() + 90_000;
  let ok = false;
  while (Date.now() < deadline) {
    try { JSON.parse(fs.readFileSync(infoPath, "utf-8")); ok = true; break; } catch { await new Promise((res) => setTimeout(res, 50)); }
  }
  const ms = Math.round(performance.now() - t0);
  child.kill("SIGTERM");
  await new Promise((res) => { if (child.exitCode !== null) return res(); child.on("exit", () => res()); setTimeout(res, 5000); });
  samples.push({ run: r, home: path.relative(REPO, home), ms, ready: ok });
  fs.rmSync(home, { recursive: true, force: true });
}
const times = samples.filter((s) => s.ready).map((s) => s.ms).sort((a, b) => a - b);
const pick = (q) => times.length ? times[Math.min(times.length - 1, Math.floor(q * times.length))] : null;
const result = {
  workload: "server 冷启动（bundle 形态：node dist-server/mac-arm64/bootstrap.js → server-info.json；全新隔离 HOME；OS page cache 不清空）",
  batch,
  runs: samples,
  ready_count: times.length,
  median_ms: pick(0.5),
  p95_ms: pick(0.95),
  min_ms: times[0] ?? null,
  max_ms: times[times.length - 1] ?? null,
  env: { node: process.version, os: `${os.type()} ${os.release()} ${os.arch()}`, cpus: os.cpus().length },
  seed_note: "输入为空 HOME + 打包 bundle 入口；无随机输入；与源码形态（W1）同机同口径成对",
};
// P08（P07-F-B 修复）：默认输出路径带 run-id 后缀，复跑不再覆盖已留档样本；
// 显式 --out 仍按调用方精确路径写入（FIXR1 复跑惯例保持不变）。
let out = path.join(P07, "samples", `startup-bundle-${new Date().toISOString().replace(/[-:T]/g, "").slice(0, 14)}.json`);
for (let i = 0; i < argv.length; i++) if (argv[i] === "--out") out = argv[++i];
fs.writeFileSync(out, JSON.stringify(result, null, 2));
console.log(`[bench-startup-bundle] ready=${times.length}/${runs} median=${result.median_ms}ms p95=${result.p95_ms}ms out=${out}`);
process.exit(times.length === runs ? 0 : 1);
