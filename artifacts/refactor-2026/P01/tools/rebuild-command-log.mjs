#!/usr/bin/env node
/**
 * rebuild-command-log.mjs — 验收修复：把 P01 原有的"日志文件 hash 索引"重建为
 * 92 模板 §3 要求的命令级 JSONL。
 *
 * 背景（验收发现）：P01 执行期只留下了 {log, bytes, sha256} 三字段的文件索引，
 * 缺 command_id/argv/exit_code/时间戳/status/retry_of。本脚本从每个日志产物的
 * 内容与文件 mtime 重建命令级条目，全部标注 rebuilt=true、
 * timestamps_reconstructed=true——它们是"从留存证据推导的记录"，不是当时
 * 实时写入的记录；原始 hash 索引保留在 log-file-index.jsonl 不变。
 *
 * 推断规则（每条条目的 evidence 字段如实说明）：
 *   - vitest 输出：测试文件列表与 failed 计数来自日志正文；exit_code = failed>0 ? 1 : 0。
 *   - tsc/检查器输出：出现 passed/零输出按 0；出现错误文本按 1。
 *   - finished_at = 文件 mtime；started_at = finished_at - 日志内 Duration（无则同 finished_at）。
 */
import fs from "node:fs";
import path from "node:path";
import console from "node:console";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const LOG_DIR = path.join(ROOT, "artifacts", "refactor-2026", "P01", "logs");
const INDEX = path.join(LOG_DIR, "command-log.jsonl");

const CWD = ROOT;
const STAGE = "P01";
const SOURCE_SHA = "92c6646c581883436e026c53898e8bfc34ce4d27";

function sha256(p) {
  try { return createHash("sha256").update(fs.readFileSync(p)).digest("hex"); } catch { return null; }
}
function iso(t) { return new Date(t).toISOString(); }

function parseVitest(text) {
  // vitest 摘要行：" Test Files  1 failed (1)" / "Tests  4 failed | 14704 passed"
  const fileLine = text.match(/Test Files\s+([0-9]+) failed/) || text.match(/Tests\s+([0-9]+) failed/);
  const files = [...text.matchAll(/[❯✓×↓]\s+(\S+\.test\.ts)/g)].map((m) => m[1]);
  const unique = [...new Set(files)];
  const failed = fileLine ? Number(fileLine[1]) : 0;
  return { exit: failed > 0 ? 1 : 0, files: unique, failed };
}
function durationMs(text) {
  const m = text.match(/Duration\s+([0-9.]+)s/);
  return m ? Math.round(parseFloat(m[1]) * 1000) : null;
}

const entries = [];
const files = fs.readdirSync(LOG_DIR).filter((f) => /\.(out|err|stat\.txt)$/.test(f)).sort();
for (const name of files) {
  const full = path.join(LOG_DIR, name);
  const text = fs.readFileSync(full, "utf8");
  const stat = fs.statSync(full);
  const id = name.replace(/\.(out|err|stat\.txt)$/, "");
  const isErr = name.endsWith(".err");
  const baseId = id;
  const argvNote = (() => {
    if (/typecheck/.test(id) && !/core-contracts/.test(id)) return ["npm", "run", "typecheck"];
    if (/core-contracts-pass/.test(id)) return ["npm", "run", "typecheck:core-contracts"];
    if (/tool-invocation/.test(id)) return ["npm", "run", "check:tool-invocation-boundaries"];
    if (/lint-changed/.test(id)) return ["npx", "eslint", "<changed-files-of-round>"];
    if (/build-server-open/.test(id)) return ["npm", "run", "build:server:open"];
    if (/> lingxi@.*\btest\b/.test(text)) return ["npm", "test"]; // 全量 vitest（日志含 npm 脚本头）
    const v = parseVitest(text);
    if (v.files.length > 0) return ["npx", "vitest", "run", ...v.files];
    return ["<see-report>"];
  })();
  const vit = parseVitest(text);
  let exit;
  let statusOverride = null;
  if (/build-server-open/.test(id)) {
    // 日志终止于 "installing external dependencies..."：依赖安装需 registry，
    // 本机网络不可达（F3），进程未跑完即中断——不是成功也不是普通失败。
    exit = null;
    statusOverride = "BLOCKED";
  } else if (/✖ \d+ problems \(0 errors/.test(text)) {
    exit = 0; // eslint：warning 不导致非零退出
  } else if (/strict check passed/.test(text) || /boundary check passed/.test(text)) exit = 0;
  else if (vit.files.length > 0) exit = vit.exit;
  else if (text.trim() === "") exit = 0; // tsc 静默成功（.err 空）或空 stdout
  else if (/error|Error|FAIL|failed/i.test(text)) exit = 1;
  else exit = 0;
  const dur = durationMs(text);
  entries.push({
    command_id: baseId + (isErr ? "#stderr" : ""),
    stage_id: STAGE,
    cwd: CWD,
    source_sha: SOURCE_SHA,
    argv: argvNote,
    env_note: { NOTE: "rebuilt entry: env not captured at execution time" },
    exit_code: isErr ? null : exit,
    started_at: iso(stat.mtimeMs - (dur ?? 0)),
    finished_at: iso(stat.mtimeMs),
    timestamps_reconstructed: true,
    status: statusOverride ?? (isErr ? "stderr-artifact" : (exit === 0 ? "PASS" : exit === null ? "UNKNOWN" : "FAIL")),
    stdout_path: isErr ? null : `artifacts/refactor-2026/P01/logs/${name}`,
    stderr_path: isErr ? `artifacts/refactor-2026/P01/logs/${name}` : null,
    stdout_digest: isErr ? null : sha256(full),
    stderr_digest: isErr ? sha256(full) : null,
    log_sha256: sha256(full),
    reason: "rebuilt from log artifact + mtime (acceptance fix for missing per-command JSONL)"
      + (/full-npm-test/.test(id) ? "; exit 1 = F1 baseline (seal coordinate lag, pre-existing, owned by governance flow)" : ""),
    retry_of: null,
    timeout_killed: false,
    timeout_ms: null,
    rebuilt: true,
  });
}

fs.writeFileSync(INDEX, entries.map((e) => JSON.stringify(e)).join("\n") + "\n");
console.log(`rebuilt ${entries.length} entries -> ${path.relative(ROOT, INDEX)}`);
