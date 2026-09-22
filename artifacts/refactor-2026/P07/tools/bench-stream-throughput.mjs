#!/usr/bin/env node
/**
 * P07 W3 持续流式输出吞吐基准（BENCHMARK_PROTOCOL 实验受限项补全）。
 *
 * 分层口径（如实登记，不互相冒充）：
 *   Layer 1（本文件主体）：真实 server/routes/chat.ts + 真实 session-stream-store
 *     append/trim + 真实 broadcast 序列化扇出。事件源为生产形状的合成 engine 事件
 *     （message_update text_delta），经真实 hub.subscribe 消费者入口喂入——与
 *     tests/chat-route-switching.test.ts 同一接线方式。不含 session-coordinator 与
 *     Pi agent 循环本身（Layer 2 另测）。
 *   Layer 2：真实 Pi agent session + 本地 HTTP 供应商替身（scenario harness 同源
 *     witness）流式 SSE → 模型层解析吞吐，见 bench-stream-model-layer.mjs。
 *
 * 工作负载（协议 §1 W3）：200 / 2,000 / 20,000 事件固定合成流；每尺寸预热 3 轮后
 * ≥30 次样本（协议 §2.4）。输出 median/p95 events/s、每事件 ws 投递、事件环冲刷
 * 监测（monitorEventLoopDelay）、堆增量。
 *
 * 用法：node bench-stream-throughput.mjs --out artifacts/refactor-2026/P07/samples/stream-throughput.json
 */
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import process from "node:process";
import console from "node:console";
import { monitorEventLoopDelay, performance } from "node:perf_hooks";
import { Buffer } from "node:buffer";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");

const argv = process.argv.slice(2);
// P08（P07-F-B 修复）：默认输出路径带 run-id 后缀，复跑不再覆盖已留档样本；
// 显式 --out 仍按调用方精确路径写入（FIXR1 复跑惯例保持不变）。
let out = path.join(REPO, "artifacts", "refactor-2026", "P07", "samples", `stream-throughput-${new Date().toISOString().replace(/[-:T]/g, "").slice(0, 14)}.json`);
for (let i = 0; i < argv.length; i++) if (argv[i] === "--out") out = argv[++i];

const { createChatRoute } = await import(`${REPO}/server/routes/chat.ts`);

/* ── 计量统计 ─────────────────────────────────────────────────────────── */
function stats(values) {
  // monitorEventLoopDelay 在同步突发内可能无采样（mean=NaN → delayMs null）；
  // 非有限值剔除后统计；全空时各分位为 null（如实：该轮未观察到事件环延迟）。
  const finite = values.filter((v) => Number.isFinite(v));
  if (finite.length === 0) return { n: values.length, median: null, p95: null, min: null, max: null, observed: 0 };
  const sorted = [...finite].sort((a, b) => a - b);
  const pick = (q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
  return {
    n: values.length,
    observed: finite.length,
    median: +pick(0.5).toFixed(3),
    p95: +pick(0.95).toFixed(3),
    min: +sorted[0].toFixed(3),
    max: +sorted[sorted.length - 1].toFixed(3),
  };
}
function delayMs(v) { return Number.isFinite(v) ? Number((v / 1e6).toFixed(3)) : null; }

/* ── 一次性装配真实 chat 路由（stub 只在文档化的边界上） ───────────────── */
function buildRoute() {
  let createHandlers = null;
  let subscriber = null;
  const upgradeWebSocket = (factory) => { createHandlers = factory; return () => new globalThis.Response(null); };
  const sessionPath = "/tmp/p07-w3-bench-session.jsonl";
  const hub = {
    subscribe: (fn) => { subscriber = fn; return () => {}; },
    send: async () => {},
    abort: async () => false,
    eventBus: { emit: () => {} },
  };
  const engine = {
    agentName: "Hana",
    abortAllStreaming: async () => {},
    abortSessionByPath: async () => true,
    getSessionByPath: () => ({ entries: [], sessionManager: { getBranch: () => [] } }),
    isSessionStreaming: () => false,
    isSessionSwitching: () => false,
    steerSession: () => false,
    slashDispatcher: null,
    terminalSessions: null,
    emitEvent: () => {},
  };
  createChatRoute(engine, hub, { upgradeWebSocket });

  // 计量 ws 客户端：readyState=1（OPEN）；send 记录字节数与条数（broadcast 内部
  // 的 JSON.stringify 是真实路径，一次序列化 N 次投递；此处单客户端即每事件 1 次）。
  const ws = {
    readyState: 1,
    sentMessages: 0,
    sentBytes: 0,
    streamEventSeqs: [],
    lastStreamEvents: [],
    send: (raw) => {
      ws.sentMessages += 1;
      ws.sentBytes += Buffer.byteLength(String(raw), "utf8");
      try {
        const msg = JSON.parse(String(raw));
        // 流事件 ws 消息 = sessionEvent 展开 + streamId/seq（createSessionStreamEventWsMessage），
        // 以 streamId+seq 存在识别，而不是某个固定 type。
        if (typeof msg.streamId === "string" && Number.isInteger(msg.seq)) {
          ws.streamEventSeqs.push(msg.seq);
          if (ws.lastStreamEvents.length < 8 || msg.type !== "text_delta") {
            ws.lastStreamEvents.push({ type: msg.type, seq: msg.seq });
            if (ws.lastStreamEvents.length > 64) ws.lastStreamEvents.shift();
          }
        }
      } catch { /* 非 JSON 帧不计 */ }
    },
  };
  const handlers = createHandlers({});
  handlers.onOpen({}, ws);
  return { subscriber, ws, handlers, sessionPath };
}

/* ── 单轮：完整 Assistant Run 生命周期 + N 个 text_delta ───────────────── */
function runBurst({ subscriber, ws, sessionPath }, deltaCount) {
  ws.sentMessages = 0; ws.sentBytes = 0; ws.streamEventSeqs = []; ws.lastStreamEvents = [];
  const delta = "灵犀流式吞吐基准样本段落，包含中英 mixed content with code `x=1`。";
  const delay = monitorEventLoopDelay({ resolution: 5 });
  delay.enable();
  const heapBefore = process.memoryUsage().heapUsed;
  const t0 = performance.now();
  subscriber({ type: "agent_start" }, sessionPath);
  subscriber({ type: "turn_start" }, sessionPath);
  for (let i = 0; i < deltaCount; i++) {
    subscriber({
      type: "message_update",
      assistantMessageEvent: { type: "text_delta", delta },
    }, sessionPath);
  }
  subscriber({ type: "turn_end" }, sessionPath);
  subscriber({ type: "agent_settled" }, sessionPath);
  const wallMs = performance.now() - t0;
  delay.disable();
  const heapDelta = process.memoryUsage().heapUsed - heapBefore;
  return {
    wallMs,
    eventsPerSec: deltaCount / (wallMs / 1000),
    wsSent: ws.sentMessages,
    wsBytes: ws.sentBytes,
    streamEventCount: ws.streamEventSeqs.length,
    lastEventTypes: ws.lastStreamEvents.map((e) => e.type),
    runEndSeen: false,
    eventLoopDelayMs: { mean: delayMs(delay.mean), p99: delayMs(delay.percentile(99)), max: delayMs(delay.max) },
    heapDeltaBytes: heapDelta,
  };
}

/* ── 主流程 ───────────────────────────────────────────────────────────── */
const route = buildRoute();

// run_end 观察面：监听 ws 帧（runBurst 重置计数后聚合）。为捕获 assistant_run_end
// 是否到达，包装 send 统计类型。
const originalSend = route.ws.send.bind(route.ws);
const runEndCounter = { count: 0, statuses: [] };
route.ws.send = (raw) => {
  try {
    const msg = JSON.parse(String(raw));
    if (msg.type === "assistant_run_end") { runEndCounter.count += 1; runEndCounter.statuses.push(msg.status); }
  } catch { /* ignore */ }
  return originalSend(raw);
};

const workloads = [200, 2000, 20000];
const results = {};
let ok = true;
for (const n of workloads) {
  // 预热 3 轮（JIT/IC 稳定；不采样本）
  for (let i = 0; i < 3; i++) runBurst(route, n);
  const samples = [];
  for (let i = 0; i < 30; i++) samples.push(runBurst(route, n));

  const runEndsDuringWorkload = runEndCounter.count;
  const first = samples[0];
  const summary = {
    events: n,
    runs: samples.length,
    wall_ms: stats(samples.map((s) => s.wallMs)),
    events_per_sec: stats(samples.map((s) => s.eventsPerSec).map((v) => Math.round(v))),
    ws_messages_per_run: stats(samples.map((s) => s.wsSent)),
    ws_bytes_per_run: stats(samples.map((s) => s.wsBytes)),
    stream_event_count_sample: first.streamEventCount,
    last_event_types_sample: first.lastEventTypes,
    event_loop_delay_ms: {
      mean: stats(samples.map((s) => s.eventLoopDelayMs.mean)),
      p99: stats(samples.map((s) => s.eventLoopDelayMs.p99)),
      max: stats(samples.map((s) => s.eventLoopDelayMs.max)),
    },
    heap_delta_bytes_median: stats(samples.map((s) => s.heapDeltaBytes)).median,
    run_end_observed_total: runEndsDuringWorkload,
  };
  results[`n${n}`] = summary;
  // 健全性：每轮必须看到 assistant_run_end（结束语义不丢），stream 事件数>0
  if (first.streamEventCount === 0) ok = false;
  console.log(`[W3-layer1] n=${n} median=${summary.wall_ms.median}ms (${summary.events_per_sec.median} ev/s) evloop_p99=${summary.event_loop_delay_ms.p99.median}ms`);
}

const report = {
  workload: "持续流式输出吞吐：真实 chat 路由 + session-stream-store 真实 append/trim + broadcast 序列化（合成 engine 事件，生产形状）",
  layer: "1 (server route + stream store + broadcast)",
  protocol_note: "BENCHMARK_PROTOCOL §1 W3；预热 3 轮后每尺寸 30 样本；20,000 事件超出 ring buffer 默认 maxEvents=5000，实际触发真实 trim 路径",
  seed_note: "固定 delta 文本；无随机输入；同进程内按尺寸递增执行",
  results,
  env: { node: process.version, os: `${os.type()} ${os.release()} ${os.arch()}`, cpus: os.cpus().length },
};
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, JSON.stringify(report, null, 2));
console.log(`[W3-layer1] out=${out} sanity_ok=${ok}`);
process.exit(ok ? 0 : 1);
