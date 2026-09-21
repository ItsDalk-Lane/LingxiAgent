#!/usr/bin/env node
/**
 * P00-T06 / P00-A08 比较器：预承诺规则实现。
 * 规则（BENCHMARK_PROTOCOL.md §5）：
 *   1) 工作负载等价性：两批样本的 workload 描述与每批 workload_digest 必须一致；
 *      不等价（如假优化少处理数据）→ 判 INVALID，禁止比较。
 *   2) 等价时比较 median：|B2-B1|/B1 > 10% 且 B2 更差 → REGRESSION（阻塞）；
 *      B2 更快同等幅度 → IMPROVED（需正确性零容忍检查另行确认）；其余 COMPARABLE。
 * 用法：node bench-compare.mjs <b1.json> <b2.json>
 * 另附 --selftest：内置一份假优化样本（runs 减半）验证会被判 INVALID。
 */
import fs from "node:fs";
import process from "node:process";
import console from "node:console";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

const THRESHOLD = 0.10;

function digestOf(batch) {
  // 工作负载指纹：workload 文本 + 完成样本数 + seed_note（不含时间数值）
  return createHash("sha256").update(JSON.stringify({
    workload: batch.workload,
    n: batch.runs ? batch.runs.filter((r) => r.ready !== false).length : batch.count,
    seed: batch.seed_note ?? batch.seed ?? null,
  })).digest("hex");
}

function judge(b1, b2) {
  const d1 = digestOf(b1), d2 = digestOf(b2);
  if (d1 !== d2) {
    return { verdict: "INVALID", reason: `工作负载不等价（digest ${d1.slice(0, 8)} vs ${d2.slice(0, 8)}）：样本数/工作负载描述不同，禁止比较`, d1, d2 };
  }
  const m1 = b1.median_ms ?? b1.median, m2 = b2.median_ms ?? b2.median;
  if (typeof m1 !== "number" || typeof m2 !== "number") return { verdict: "INVALID", reason: "缺 median" };
  const delta = (m2 - m1) / m1;
  if (delta > THRESHOLD) return { verdict: "REGRESSION", delta_pct: +(delta * 100).toFixed(1), reason: `中位数劣化 ${(delta * 100).toFixed(1)}% > 10% 阈值` };
  if (delta < -THRESHOLD) return { verdict: "IMPROVED", delta_pct: +(delta * 100).toFixed(1), reason: `中位数改善 ${(-delta * 100).toFixed(1)}%；正确性零容忍检查另行确认后才能接受` };
  return { verdict: "COMPARABLE", delta_pct: +(delta * 100).toFixed(1), reason: `中位数差异 ${Math.abs(delta * 100).toFixed(1)}% ≤ 10%，在波动区间内` };
}

if (process.argv[2] === "--selftest") {
  const base = { workload: "server 冷启动（spawn→server-info.json 出现；全新隔离 HOME；OS page cache 不清空）", runs: [{ run: 1, ms: 100, ready: true }, { run: 2, ms: 110, ready: true }], median_ms: 105, seed_note: "输入为空 HOME + 固定入口 server/main-full.ts；无随机输入；端口由 server 自动选择" };
  const sameInputFaster = { ...base, runs: [{ run: 1, ms: 78, ready: true }, { run: 2, ms: 82, ready: true }], median_ms: 80 };
  const fakeOptimization = { ...base, runs: [{ run: 1, ms: 50, ready: true }], median_ms: 50 }; // 少一半样本=少处理数据
  const j1 = judge(base, sameInputFaster);
  const j2 = judge(base, fakeOptimization);
  console.log(JSON.stringify({ same_input: j1, fake_optimization: j2 }, null, 2));
  const ok = (j1.verdict === "IMPROVED" || j1.verdict === "COMPARABLE") && j2.verdict === "INVALID";
  console.log(`[bench-compare selftest] ${ok ? "PASS" : "FAIL"}：同输入给出可比判定（IMPROVED/COMPARABLE）、假优化被拒（INVALID）`);
  process.exit(ok ? 0 : 1);
}

const [f1, f2] = process.argv.slice(2);
if (!f1 || !f2) { console.error("usage: bench-compare.mjs <b1.json> <b2.json> | --selftest"); process.exit(64); }
const b1 = JSON.parse(fs.readFileSync(f1, "utf-8"));
const b2 = JSON.parse(fs.readFileSync(f2, "utf-8"));
const verdict = judge(b1, b2);
console.log(JSON.stringify(verdict, null, 2));
process.exit(verdict.verdict === "REGRESSION" || verdict.verdict === "INVALID" ? 1 : 0);
