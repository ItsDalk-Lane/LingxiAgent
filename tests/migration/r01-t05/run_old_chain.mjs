#!/usr/bin/env node
/**
 * R01-T05 旧链驱动：以隔离 harness 跑一次生产 office-pdf-helper。
 *
 * 用法：node run_old_chain.mjs --job <job.json> --log <logfile>
 * 行为：
 *  -  spawn node_modules/.bin/electron old_chain_harness.cjs --hana-office-html-to-pdf <job>
 *  -  硬看门狗 = job.timeoutMs + 20s，超时 SIGTERM→SIGKILL 整棵进程树（按进程组）
 *  -  输出一行 JSON 到 stdout：{exit_code, signal, wall_ms, timed_out, output_exists, output_size}
 *  -  stderr/stdout 全文落 --log
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "../../..");

function arg(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : null;
}

const jobPath = path.resolve(arg("--job"));
const logPath = arg("--log") ? path.resolve(arg("--log")) : null;
const job = JSON.parse(fs.readFileSync(jobPath, "utf-8"));
const hardTimeoutMs = (Number(job.timeoutMs) || 60000) + 20000;

const electron = path.join(REPO, "node_modules", ".bin", "electron");
const harness = path.join(HERE, "old_chain_harness.cjs");

const env = { ...process.env };
delete env.ELECTRON_RUN_AS_NODE;
// 代理环境变量已死，但显式清掉，防意外泄漏进被测进程
for (const k of ["all_proxy", "ALL_PROXY", "http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY"]) delete env[k];

const t0 = Date.now();
const child = spawn(electron, [harness, "--hana-office-html-to-pdf", jobPath], {
  env,
  detached: true, // 独立进程组，看门狗可整组杀
  stdio: ["ignore", "pipe", "pipe"],
});

let log = "";
const append = (chunk) => { log += String(chunk); };
child.stdout.on("data", append);
child.stderr.on("data", append);

let timedOut = false;
const watchdog = setTimeout(() => {
  timedOut = true;
  try { process.kill(-child.pid, "SIGTERM"); } catch { try { child.kill("SIGTERM"); } catch {} }
  setTimeout(() => {
    try { process.kill(-child.pid, "SIGKILL"); } catch { try { child.kill("SIGKILL"); } catch {} }
  }, 3000).unref();
}, hardTimeoutMs);
watchdog.unref();

const result = await new Promise((resolve) => {
  child.on("error", (err) => resolve({ spawn_error: String(err) }));
  child.on("close", (code, signal) => resolve({ exit_code: code, signal: signal || null }));
});
clearTimeout(watchdog);

const out = {
  job: jobPath,
  exit_code: result.exit_code ?? null,
  signal: result.signal ?? null,
  spawn_error: result.spawn_error ?? null,
  wall_ms: Date.now() - t0,
  timed_out: timedOut,
  output_exists: fs.existsSync(job.outputPath),
  output_size: fs.existsSync(job.outputPath) ? fs.statSync(job.outputPath).size : 0,
};
if (logPath) {
  fs.mkdirSync(path.dirname(logPath), { recursive: true });
  fs.writeFileSync(logPath, log);
}
console.log(JSON.stringify(out));
process.exit(timedOut ? 124 : (result.exit_code ?? 1));
