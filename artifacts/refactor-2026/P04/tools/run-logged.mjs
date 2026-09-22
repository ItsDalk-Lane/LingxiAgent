#!/usr/bin/env node
/**
 * P04 命令日志记录器（复验收修复轮 2026-09-22 引入；与 P00/P02 工具同构）。
 * 用法：node run-logged.mjs --id <command_id> --reason "<原因>" [--cwd <dir>] [--retry-of <id>] -- <argv...>
 * 行为：执行 argv，stdout/stderr 原样写入 artifacts/refactor-2026/P04/logs/<id>.{out,err}，
 * 追加全格式 JSONL 条目（argv/cwd/source_sha/起止时间/exit_code/status/digest）。
 * 子进程 exit code 原样透传。此前 P04 既有条目为弱格式（无时间戳/digest），
 * 属已登记退化（P04_REPORT 复验收修复轮 #4）；历史条目不重建、不改动。
 */
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import process from "node:process";
import console from "node:console";
import { setTimeout, clearTimeout } from "node:timers";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const LOG_DIR = path.join(ROOT, "artifacts", "refactor-2026", "P04", "logs");
const INDEX = path.join(LOG_DIR, "command-log.jsonl");

const args = process.argv.slice(2);
if (!args[0] || args[0] === "--help") {
  console.error("usage: run-logged.mjs --id ID [--reason R] [--cwd D] [--timeout-ms N] [--retry-of ID] -- argv...");
  process.exit(64);
}
let id = "", reason = "", cwd = ROOT, timeoutMs = 600000, retryOf = null;
const argv = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === "--id") id = args[++i];
  else if (args[i] === "--reason") reason = args[++i];
  else if (args[i] === "--cwd") cwd = args[++i];
  else if (args[i] === "--timeout-ms") timeoutMs = Number(args[++i]);
  else if (args[i] === "--retry-of") retryOf = args[++i];
  else if (args[i] === "--") { for (let j = i + 1; j < args.length; j++) argv.push(args[j]); break; }
  else { console.error(`unknown arg ${args[i]}`); process.exit(64); }
}
if (!id || argv.length === 0) { console.error("--id and argv are required"); process.exit(64); }

const safeEnvKeys = ["PATH", "LANG", "LC_ALL", "TERM", "SHELL", "HOME", "CI", "LINGXI_HOME", "NODE_ENV", "NO_COLOR"];
const envNote = {};
for (const k of safeEnvKeys) if (process.env[k] !== undefined) envNote[k] = k === "HOME" ? "<redacted-user-home>" : process.env[k];

function digest(p) {
  try {
    const buf = fs.readFileSync(p);
    return `${path.relative(ROOT, p)} sha256=${createHash("sha256").update(buf).digest("hex")} bytes=${buf.length}`;
  } catch { return `${path.relative(ROOT, p)} (empty/missing)`; }
}

const stdoutPath = path.join(LOG_DIR, `${id}.out`);
const stderrPath = path.join(LOG_DIR, `${id}.err`);
const startedAt = new Date();
let sourceSha = "unknown";
try { sourceSha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: ROOT, encoding: "utf8" }).trim(); } catch {}

const child = spawn(argv[0], argv.slice(1), { cwd, env: process.env, stdio: ["ignore", "pipe", "pipe"] });
const out = fs.openSync(stdoutPath, "w");
const err = fs.openSync(stderrPath, "w");
child.stdout.on("data", (d) => fs.writeSync(out, d));
child.stderr.on("data", (d) => fs.writeSync(err, d));
let timedOut = false;
const timer = setTimeout(() => { timedOut = true; child.kill("SIGKILL"); }, timeoutMs);
const code = await new Promise((resolve) => {
  child.on("exit", (c, s) => resolve(c === null ? (s ? 137 : 1) : c));
  child.on("error", () => resolve(127));
});
clearTimeout(timer);
fs.closeSync(out); fs.closeSync(err);
const finishedAt = new Date();
const entry = {
  command_id: id, stage_id: "P04", cwd,
  source_sha: sourceSha, argv, env_note: envNote,
  started_at: startedAt.toISOString(), finished_at: finishedAt.toISOString(),
  timeout_ms: timeoutMs, timeout_killed: timedOut,
  exit_code: code, status: code === 0 ? "PASS" : (code === 127 ? "BLOCKED" : "FAIL"),
  stdout_path: path.relative(ROOT, stdoutPath), stderr_path: path.relative(ROOT, stderrPath),
  stdout_digest: digest(stdoutPath), stderr_digest: digest(stderrPath),
  reason, retry_of: retryOf,
};
fs.appendFileSync(INDEX, JSON.stringify(entry) + "\n");
console.log(`[run-logged] ${id} exit=${code}${timedOut ? " (TIMEOUT-KILLED)" : ""} out=${stdoutPath}`);
process.exit(code);
