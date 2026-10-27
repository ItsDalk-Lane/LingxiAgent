#!/usr/bin/env node
/**
 * R00-A09｜夹具可重复性三次重放驱动器（断网隔离）。
 *
 * 用法：node scripts/rust-tauri/r00-t05-replay.mjs [--out <dir>]
 * 默认输出 artifacts/rust-tauri/R00/T05。
 *
 * 每次运行：
 *  - 以 LINGXI_MIGRATION_BLOCK_NETWORK=1 启动 vitest（测试进程内显式阻断
 *    net/tls/dns 外连；Hono app.request 为进程内调用，不走 socket）；
 *  - LINGXI_MIGRATION_REPLAY_OUT 指向 run-N 目录，测试把每个夹具的
 *    raw 与 normalized 结果写入该目录；
 *  - stdout/stderr 原样保存为 run-N/stdout.txt（.log 扩展名被 .gitignore 忽略，
 *    已用 git check-ignore 核实 .txt 可入库）。
 *
 * 三次结束后：对同名 normalized 文件做字节级对比并写出 diff 文件（预期为空），
 * 汇总各 run 的 raw/normalized SHA-256 到 REPLAY_SUMMARY.json。
 * 差异只允许来自 FIXTURE_MANIFEST.json 声明的允许变动字段（规范化后应无差异）。
 */

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..", "..");
const defaultOut = path.join(repoRoot, "artifacts", "rust-tauri", "R00", "T05");

const args = process.argv.slice(2);
let outRoot = defaultOut;
{
  const idx = args.indexOf("--out");
  if (idx >= 0 && args[idx + 1]) outRoot = path.resolve(args[idx + 1]);
}

const testFiles = [
  "tests/migration/r00-t05-replay.test.ts",
  "tests/migration/r00-a10-old-defect.test.ts",
];

function sha256(filePath) {
  return createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

function lineDiff(aText, bText) {
  const a = aText.split("\n");
  const b = bText.split("\n");
  const out = [];
  const max = Math.max(a.length, b.length);
  for (let i = 0; i < max; i += 1) {
    if (a[i] !== b[i]) {
      out.push(`- ${a[i] ?? ""}`);
      out.push(`+ ${b[i] ?? ""}`);
    }
  }
  return out.join("\n");
}

fs.mkdirSync(outRoot, { recursive: true });
const summary = {
  schema_version: 1,
  task_id: "R00-T05",
  acceptance: "R00-A09",
  driver: "scripts/rust-tauri/r00-t05-replay.mjs",
  network_isolation: "LINGXI_MIGRATION_BLOCK_NETWORK=1（vitest worker 内阻断 net/tls/dns 外连）",
  runs: [],
  diffs: [],
};

for (let n = 1; n <= 3; n += 1) {
  const runDir = path.join(outRoot, `run-${n}`);
  fs.mkdirSync(runDir, { recursive: true });
  const stdoutPath = path.join(runDir, "stdout.txt");
  let exitCode = 0;
  let stdout = "";
  try {
    stdout = execFileSync(
      process.platform === "win32" ? "npx.cmd" : "npx",
      ["vitest", "run", ...testFiles],
      {
        cwd: repoRoot,
        encoding: "utf8",
        env: {
          ...process.env,
          LINGXI_MIGRATION_REPLAY_OUT: runDir,
          LINGXI_MIGRATION_BLOCK_NETWORK: "1",
        },
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
  } catch (error) {
    exitCode = typeof error.status === "number" ? error.status : 1;
    stdout = `${error.stdout ?? ""}${error.stderr ?? ""}`;
  }
  fs.writeFileSync(stdoutPath, stdout, "utf8");

  const hashes = {};
  for (const name of fs.readdirSync(runDir).sort()) {
    if (name === "stdout.txt") continue;
    hashes[name] = sha256(path.join(runDir, name));
  }
  summary.runs.push({ run: n, exitCode, stdoutPath, fileHashes: hashes });
  console.log(`run-${n}: exit=${exitCode}, files=${Object.keys(hashes).length}`);
}

// 规范化结果两两对比（run1 为基准）
const run1Dir = path.join(outRoot, "run-1");
for (const other of [2, 3]) {
  const runDir = path.join(outRoot, `run-${other}`);
  const names = new Set([
    ...fs.readdirSync(run1Dir).filter((n) => n.endsWith(".normalized.json")),
    ...fs.readdirSync(runDir).filter((n) => n.endsWith(".normalized.json")),
  ]);
  let allEqual = true;
  const details = [];
  for (const name of [...names].sort()) {
    const aPath = path.join(run1Dir, name);
    const bPath = path.join(runDir, name);
    const aText = fs.existsSync(aPath) ? fs.readFileSync(aPath, "utf8") : "<missing>";
    const bText = fs.existsSync(bPath) ? fs.readFileSync(bPath, "utf8") : "<missing>";
    if (aText !== bText) {
      allEqual = false;
      details.push({ file: name, diff: lineDiff(aText, bText) });
    }
  }
  const diffPath = path.join(outRoot, `normalized-diff-run1-run${other}.txt`);
  const body = allEqual
    ? `normalized outputs identical between run-1 and run-${other} (${names.size} fixtures)\n`
    : JSON.stringify(details, null, 2) + "\n";
  fs.writeFileSync(diffPath, body, "utf8");
  summary.diffs.push({ pair: `run-1 vs run-${other}`, identical: allEqual, fixtures: names.size, diffPath });
  console.log(`diff run-1 vs run-${other}: identical=${allEqual} (${names.size} fixtures)`);
}

summary.allRunsExitedZero = summary.runs.every((r) => r.exitCode === 0);
summary.allNormalizedIdentical = summary.diffs.every((d) => d.identical);
summary.conclusion = summary.allRunsExitedZero && summary.allNormalizedIdentical
  ? "PASS-CANDIDATE: 三次重放退出码均为 0 且规范化输出逐字节一致（差异仅限声明允许字段，规范化后为零）"
  : "FAIL: 存在退出码非零或规范化输出不一致";
fs.writeFileSync(path.join(outRoot, "REPLAY_SUMMARY.json"), JSON.stringify(summary, null, 2) + "\n", "utf8");
console.log(summary.conclusion);
process.exitCode = summary.allRunsExitedZero && summary.allNormalizedIdentical ? 0 : 1;
