#!/usr/bin/env node
/**
 * P00-T06 启动基准：冷启动 server（绕过 launch.js，隔离 LINGXI_HOME），
 * 测 spawn→server-info.json 出现的墙钟时间。每次运行全新隔离 HOME（确定性冷启动；
 * OS page cache 不清空——首启后的物理缓存热度在协议中如实标注）。
 * 输出 JSON：{ batch, runs: [{run, home, ms}], median_ms, p95_ms, min_ms, max_ms, env }。
 * 用法：node bench-startup.mjs --runs 10 --batch B1 --out <path.json>
 */
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import console from "node:console";
import { setTimeout } from "node:timers";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const P00 = path.join(REPO, "artifacts", "refactor-2026", "P00");

const argv = process.argv.slice(2);
let runs = 10, batch = "B1", out = path.join(P00, "samples", "startup-b1.json");
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--runs") runs = Number(argv[++i]);
  else if (argv[i] === "--batch") batch = argv[++i];
  else if (argv[i] === "--out") out = argv[++i];
}

const samples = [];
for (let r = 1; r <= runs; r++) {
  const home = path.join(P00, "isolated", `bench-boot-${batch}-${r}`);
  fs.rmSync(home, { recursive: true, force: true });
  fs.mkdirSync(home, { recursive: true });
  const t0 = performance.now();
  const child = spawn(process.execPath, ["server/main-full.ts"], {
    cwd: REPO,
    env: { ...process.env, LINGXI_HOME: home, LINGXI_PORT: "0", LINGXI_TOKEN: `p00-bench-${batch}-${r}` },
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
  await new Promise((res) => { child.on("exit", res); setTimeout(res, 5000).unref?.(); void res; });
  // 等待实际退出（带兜底）
  await new Promise((res) => { if (child.exitCode !== null) return res(); child.on("exit", () => res()); setTimeout(res, 5000); });
  samples.push({ run: r, home: path.relative(REPO, home), ms, ready: ok });
  fs.rmSync(home, { recursive: true, force: true });
}
const times = samples.filter((s) => s.ready).map((s) => s.ms).sort((a, b) => a - b);
const pick = (q) => times.length ? times[Math.min(times.length - 1, Math.floor(q * times.length))] : null;
const result = {
  workload: "server 冷启动（spawn→server-info.json 出现；全新隔离 HOME；OS page cache 不清空）",
  batch,
  runs: samples,
  ready_count: times.length,
  median_ms: pick(0.5),
  p95_ms: pick(0.95),
  min_ms: times[0] ?? null,
  max_ms: times[times.length - 1] ?? null,
  env: { node: process.version, os: `${os.type()} ${os.release()} ${os.arch()}`, cpus: os.cpus().length },
  seed_note: "输入为空 HOME + 固定入口 server/main-full.ts；无随机输入；端口由 server 自动选择",
};
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, JSON.stringify(result, null, 2));
console.log(`[bench-startup] batch=${batch} ready=${times.length}/${runs} median=${result.median_ms}ms p95=${result.p95_ms}ms out=${out}`);
process.exit(times.length === runs ? 0 : 1);
