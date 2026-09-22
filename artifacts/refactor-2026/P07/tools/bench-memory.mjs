#!/usr/bin/env node
/**
 * P07 W5 内存工作负载复测（复制自 P00 同款工具，输出改指 P07 samples；口径不变）：隔离 HOME 启动 server，
 * 采样（a）就绪后 3s 空闲态与（b）200 次 input-draft PUT 活动后的整进程树 RSS。
 * 非峰值压测——峰值内存协议（流式高负载）在 BENCHMARK_PROTOCOL 定义，P07 执行。
 */
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import process from "node:process";
import console from "node:console";
import { setTimeout } from "node:timers";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const P00 = path.join(REPO, "artifacts", "refactor-2026", "P07");
const home = path.join(P00, "isolated", "bench-mem-P07");
fs.rmSync(home, { recursive: true, force: true });
fs.mkdirSync(home, { recursive: true });

function treeRssKb(pid) {
  // macOS: ps 列出全部后按父链聚合（pgid 简化：server 未 fork 子进程时的单进程树 + 可能的 worker）
  const out = execFileSync("ps", ["-Ao", "pid,ppid,rss,command"], { encoding: "utf-8", maxBuffer: 16 * 1024 * 1024 });
  const rows = out.split("\n").slice(1).map((l) => l.trim().match(/^(\d+)\s+(\d+)\s+(\d+)\s+(.*)$/)).filter(Boolean)
    .map((m) => ({ pid: +m[1], ppid: +m[2], rss: +m[3], cmd: m[4] }));
  const byPid = new Map(rows.map((r) => [r.pid, r]));
  const roots = new Set([pid]);
  let total = 0; const members = [];
  for (const r of rows) {
    let cur = r, inTree = false;
    for (let i = 0; i < 6 && cur; i++) { if (roots.has(cur.pid)) { inTree = true; break; } cur = byPid.get(cur.ppid); }
    if (inTree && /server\/main-full\.ts/.test(r.cmd)) { total += r.rss; members.push({ pid: r.pid, rss_kb: r.rss }); }
  }
  return { total_kb: total, members };
}

const child = spawn(process.execPath, ["server/main-full.ts"], {
  cwd: REPO, env: { ...process.env, LINGXI_HOME: home, LINGXI_TOKEN: "p07-bench-mem" }, stdio: "ignore",
});
const infoPath = path.join(home, "server-info.json");
const deadline = Date.now() + 90_000;
let info = null;
while (Date.now() < deadline) { try { info = JSON.parse(fs.readFileSync(infoPath, "utf-8")); break; } catch { await new Promise((r) => setTimeout(r, 100)); } }
if (!info) { child.kill("SIGKILL"); console.error("server 未就绪"); process.exit(1); }
await new Promise((r) => setTimeout(r, 3000));
const idle = treeRssKb(info.pid);

let ok200 = 0;
for (let i = 0; i < 200; i++) {
  const r = await globalThis.fetch(`http://127.0.0.1:${info.port}/api/input-drafts`, {
    method: "PUT", headers: { "content-type": "application/json", authorization: "Bearer p07-bench-mem" },
    body: JSON.stringify({ surface: "electron", scope: "home", text: `p00 mem workload ${i} ${"x".repeat(64)}` }),
  });
  if (r.status === 200) ok200++;
}
const afterActivity = treeRssKb(info.pid);
child.kill("SIGTERM");
await new Promise((r) => { child.on("exit", r); setTimeout(r, 5000); });
const result = {
  workload: "server 空闲（就绪+3s）与活动后（200 次 input-draft PUT）整进程树 RSS；非峰值压测",
  idle: idle, activity: afterActivity, put_ok: ok200,
  env: { node: process.version, os: `${os.type()} ${os.release()} ${os.arch()}` },
  limitation: "活动负载为轻量 API 写；流式高负载峰值内存按 BENCHMARK_PROTOCOL 留待 P07",
};
// P08（P07-F-B 修复）：默认输出路径带 run-id 后缀，复跑不再覆盖已留档样本；
// 显式 --out 仍按调用方精确路径写入。
const argv = process.argv.slice(2);
let out = path.join(P00, "samples", `memory-p07-${new Date().toISOString().replace(/[-:T]/g, "").slice(0, 14)}.json`);
for (let i = 0; i < argv.length; i++) if (argv[i] === "--out") out = argv[++i];
fs.writeFileSync(out, JSON.stringify(result, null, 2));
fs.rmSync(home, { recursive: true, force: true });
console.log(`[bench-mem] idle=${idle.total_kb}KB after200puts=${afterActivity.total_kb}KB ok=${ok200}/200 out=${out}`);
process.exit(ok200 === 200 && afterActivity.total_kb >= idle.total_kb ? 0 : 1);
