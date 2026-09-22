#!/usr/bin/env node
/**
 * P07 热点隔离微基准：session-stream-store append/trim 在缓冲填满后的每事件成本。
 * 只读生产模块，不改实现；用于 HOTSPOT_REPORT 量化定位（splice(0,1) 头部移除
 * 在 maxEvents 饱和后变为 O(maxEvents)/append）。
 */
import { performance } from "node:perf_hooks";
import process from "node:process";
import console from "node:console";
import os from "node:os";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const store = await import(`${REPO}/server/session-stream-store.ts`);

const delta = { type: "text_delta", delta: "灵犀流式吞吐基准样本段落，包含中英 mixed content。" };

function measure(label, n, maxEvents) {
  const state = store.createSessionStreamState({ maxEvents });
  store.beginSessionStream(state, "bench");
  // 填满到饱和（触发稳态 trim）
  for (let i = 0; i < n; i++) store.appendSessionStreamEvent(state, delta);
  // 稳态：缓冲保持满，每次 append 都 trim 一次
  const t0 = performance.now();
  for (let i = 0; i < n; i++) store.appendSessionStreamEvent(state, delta);
  const ms = performance.now() - t0;
  return {
    label, n, maxEvents,
    steady_append_per_sec: Math.round(n / (ms / 1000)),
    steady_us_per_append: +(ms * 1000 / n).toFixed(3),
    dropped: state.droppedEvents,
    retained: state.events.length,
  };
}

const out = [];
out.push(measure("below-cap (no trim)", 2000, 5000));
out.push(measure("at-cap default (maxEvents=5000)", 20000, 5000));
out.push(measure("at-cap larger (maxEvents=20000)", 20000, 20000));

const report = {
  workload: "session-stream-store appendSessionStreamEvent 稳态吞吐（缓冲饱和后）",
  env: { node: process.version, os: `${os.type()} ${os.release()} ${os.arch()}` },
  results: out,
};
// P08（P07-F-B 修复）：默认输出路径带 run-id 后缀，复跑不再覆盖已留档样本；
// 显式 --out 仍按调用方精确路径写入。
const argv = process.argv.slice(2);
let outPath = path.join(REPO, "artifacts", "refactor-2026", "P07", "samples", `stream-store-trim-microbench-${new Date().toISOString().replace(/[-:T]/g, "").slice(0, 14)}.json`);
for (let i = 0; i < argv.length; i++) if (argv[i] === "--out") outPath = argv[++i];
fs.writeFileSync(outPath, JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
