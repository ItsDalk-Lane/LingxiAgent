#!/usr/bin/env node
/**
 * P00-A06「证据关联」：交付清单检查器。
 * 校验 docs/refactor-2026/P00/ 下结构化文件的任务/场景条目是否都有
 * 命令证据或生产入口；缺失则 exit 1 并逐条指出缺失字段（阻止该项通过）。
 * 用法：node evidence-check.mjs
 */
import fs from "node:fs";
import process from "node:process";
import console from "node:console";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DOCS = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..", "docs", "refactor-2026", "P00");
const problems = [];

function mustExist(rel) {
  if (!fs.existsSync(path.join(DOCS, rel))) problems.push(`缺少交付文件: docs/refactor-2026/P00/${rel}`);
}
function loadJson(rel) {
  try { return JSON.parse(fs.readFileSync(path.join(DOCS, rel), "utf-8")); } catch (e) { problems.push(`${rel} 解析失败: ${e.message}`); return null; }
}

for (const f of ["BASELINE.json", "SCOPE.json", "CALLSITE_MATRIX.json", "ACCEPTANCE_MAP.json", "BASELINE_TESTS.json", "PROMPT_BASELINE.json", "STAGE_DEPENDENCIES.json", "P00_RESULT.json"]) mustExist(f);
for (const f of ["FEATURE_MATRIX.md", "ENTRYPOINT_MATRIX.md", "OWNERSHIP_MAP.md", "BASELINE_FAILURES.md", "BENCHMARK_PROTOCOL.md", "REFACTOR_BACKLOG.md", "NEXT_STAGE_HANDOFF.md", "P00_REPORT.md", "WORKTREE_SAFETY.md"]) mustExist(f);

const logsIdx = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "logs", "command-log.jsonl");
const commands = fs.existsSync(logsIdx) ? fs.readFileSync(logsIdx, "utf-8").trim().split("\n").filter(Boolean).map((l) => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean) : [];
const commandIds = new Set(commands.map((c) => c.command_id));

const result = loadJson("P00_RESULT.json");
if (result) {
  if (!Array.isArray(result.tasks) || result.tasks.length < 8) problems.push("P00_RESULT.json: tasks 少于 8 项");
  for (const t of result.tasks || []) {
    for (const field of ["id", "implementation_action", "status", "evidence"]) {
      if (t[field] === undefined || t[field] === null || (Array.isArray(t[field]) && t[field].length === 0)) {
        problems.push(`任务 ${t.id || "?"}: 缺字段 ${field}`);
      }
    }
    if (t.status === "PASS" || t.status === "UNCHANGED_VERIFIED") {
      const ids = (t.test_ids || []).filter((s) => typeof s === "string" && /^P00-/.test(s));
      if (ids.length === 0) problems.push(`任务 ${t.id}: PASS/UNCHANGED_VERIFIED 但 test_ids 无命令ID`);
      for (const id of ids) if (!commandIds.has(id)) problems.push(`任务 ${t.id}: test_ids 引用的命令 ${id} 不存在于 command-log.jsonl`);
    }
  }
  for (const c of result.acceptance_cases || []) {
    if (!c.id || !c.status) problems.push(`acceptance_case 条目缺 id/status`);
    if (c.status === "PASS" && (!c.command_ids || c.command_ids.length === 0)) problems.push(`场景 ${c.id}: PASS 但无 command_ids`);
    for (const cid of c.command_ids || []) if (!commandIds.has(cid)) problems.push(`场景 ${c.id}: 引用的命令 ${cid} 不存在于 command-log.jsonl`);
  }
}

const acc = loadJson("ACCEPTANCE_MAP.json");
if (acc) {
  const ids = (acc.cases || []).map((c) => c.id);
  for (const expected of ["P00-A01","P00-A02","P00-A03","P00-A04","P00-A05","P00-A06","P00-A07","P00-A08","P00-A09","P00-A10"]) {
    if (!ids.includes(expected)) problems.push(`ACCEPTANCE_MAP.json 缺场景 ${expected}`);
  }
}

if (problems.length) {
  console.error(`evidence-check FAIL (${problems.length}):`);
  for (const p of problems) console.error("  - " + p);
  process.exit(1);
}
console.log(`evidence-check PASS: 交付文件齐全；PASS 条目均有命令/入口证据且命令ID可回溯（日志 ${commands.length} 条）`);
