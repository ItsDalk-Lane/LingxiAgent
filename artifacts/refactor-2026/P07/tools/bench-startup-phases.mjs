#!/usr/bin/env node
/**
 * P07 启动阶段计时 + CPU 剖析采集器（可复现工具）。
 * 在隔离 HOME 内冷启动真实 server（server/main-full.ts），逐行打时间戳输出阶段日志，
 * 并对子进程启用 V8 --cpu-prof（SIGTERM 优雅退出后落盘 .cpuprofile）。
 * 用法：node bench-startup-phases.mjs [--prof] [--out-dir artifacts/refactor-2026/P07/samples]
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import console from "node:console";
import { setTimeout } from "node:timers";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const argv = process.argv.slice(2);
const withProf = argv.includes("--prof");
const outDirIndex = argv.indexOf("--out-dir");
const outDirArg = outDirIndex !== -1 ? argv[outDirIndex + 1] : undefined;
const outDir = path.resolve(REPO, outDirArg ?? path.join("artifacts", "refactor-2026", "P07", "samples"));

const home = path.join(outDir, "isolated", `startup-phases-${Date.now()}`);
fs.rmSync(home, { recursive: true, force: true });
fs.mkdirSync(home, { recursive: true });

const profArgs = withProf ? ["--cpu-prof", `--cpu-prof-dir=${outDir}/cpu-prof`, "--cpu-prof-name=child-startup.cpuprofile"] : [];
const t0 = Date.now();
const child = spawn(process.execPath, [...profArgs, "server/main-full.ts"], {
  cwd: REPO,
  env: { ...process.env, LINGXI_HOME: home, LINGXI_PORT: "0", LINGXI_TOKEN: "p07-startup-phases" },
  stdio: ["ignore", "pipe", "pipe"],
});
const stamp = (buf) => buf.toString().split("\n").filter(Boolean).map((l) => `+${Date.now() - t0}ms ${l}`).join("\n");
child.stdout.on("data", (d) => process.stdout.write(stamp(d) + "\n"));
child.stderr.on("data", (d) => process.stdout.write(stamp(d) + "\n"));

const infoPath = path.join(home, "server-info.json");
const deadline = Date.now() + 90_000;
let ready = false;
while (Date.now() < deadline) {
  try { JSON.parse(fs.readFileSync(infoPath, "utf-8")); ready = true; break; } catch { await new Promise((r) => setTimeout(r, 10)); }
}
console.log(`+${Date.now() - t0}ms [READY server-info.json visible] ready=${ready}`);
await new Promise((r) => setTimeout(r, 200));
child.kill("SIGTERM");
const code = await new Promise((r) => child.on("exit", r));
fs.rmSync(home, { recursive: true, force: true });
console.log(`[bench-startup-phases] child_exit=${code} ready=${ready}`);
process.exit(code === 0 && ready ? 0 : 1);
