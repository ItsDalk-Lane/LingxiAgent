#!/usr/bin/env node
/**
 * P07 W3 Layer 2 + W4 取消时延基准（BENCHMARK_PROTOCOL 实验受限项补全）。
 *
 * W3 Layer 2（模型流式解析层）：真实 createAgentSession（Pi SDK）+ 本地 HTTP
 *   供应商替身（tests/helpers/model-observability-scenario-harness.ts 的
 *   startFakeProviderWitness —— 本地随机端口真实 HTTP server）。witness 以 SSE
 *   投递 N 个 delta，测 prompt() 全程墙钟与逐事件解析吞吐。不含 observability
 *   持久化（其开销边界已有 tests/model-observability-e2e-concurrency-perf.test.ts
 *   S37 数量级守卫，本基准如实单列）。
 *
 * W4（取消响应时延）：三个规格 ——
 *   S1 prompt 后 500ms 取消（首字节前，witness 延迟 15s 响应）
 *   S2 流中 5s 取消（sse-bytes 分片长流，interChunkDelayMs=250）
 *   S3 工具执行中取消（真实 spawnAndStream 子进程树 + 独立进程组哨兵，复用
 *      tests/p02-cancellation-edges.test.ts A06 形状）
 *   正确性零容忍：取消后 witness 无新增请求计数、哨兵存活、子进程树退出。
 *
 * 用法：node bench-stream-model-layer.mjs --out artifacts/refactor-2026/P07/samples/stream-model-layer.json
 */
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import process from "node:process";
import console from "node:console";
import { performance } from "node:perf_hooks";
import { spawn } from "node:child_process";
import { setTimeout } from "node:timers";
import { Buffer } from "node:buffer";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");

const argv = process.argv.slice(2);
let out = path.join(REPO, "artifacts", "refactor-2026", "P07", "samples", "stream-model-layer.json");
for (let i = 0; i < argv.length; i++) if (argv[i] === "--out") out = argv[++i];

const { startFakeProviderWitness } = await import(`${REPO}/tests/helpers/model-observability-scenario-harness.ts`);
const { SessionManager, DefaultResourceLoader, ModelRuntime } = await import("@earendil-works/pi-coding-agent");
const { createAgentSession } = await import(`${REPO}/lib/pi-sdk/index.ts`);
const { spawnAndStream } = await import(`${REPO}/lib/sandbox/exec-helper.ts`);
// AbortController 是 Node 全局（无 node: 内建模块导出）；显式取 globalThis 引用满足 lint no-undef
const { AbortController } = globalThis;

function stats(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const pick = (q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
  return { n: sorted.length, median: +pick(0.5).toFixed(1), p95: +pick(0.95).toFixed(1), min: +sorted[0].toFixed(1), max: +sorted[sorted.length - 1].toFixed(1) };
}

/** N 个 delta 的 OpenAI Chat Completions SSE body（每个 delta 独立 data: 块）。 */
function sseBodyWithDeltas(n, deltaText) {
  const events = [];
  for (let i = 0; i < n; i++) {
    events.push({ id: `chatcmpl-w3l2-${i}`, object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { role: i === 0 ? "assistant" : undefined, content: deltaText }, finish_reason: null }] });
  }
  events.push({ id: "chatcmpl-w3l2-end", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "stop" }], usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 } });
  return [...events.map((event) => `data: ${JSON.stringify(event)}`), "data: [DONE]", ""].join("\n\n");
}

async function createWitnessRuntimeSession(witness, lingxiHome) {
  const runtime = await ModelRuntime.create({ modelsPath: null, refreshOnCreate: false });
  runtime.registerProvider("p07-witness-provider", {
    name: "P07 Witness Provider",
    baseUrl: `${witness.baseUrl}/v1`,
    api: "openai-completions",
    apiKey: "sk-P07-WITNESS-SYNTHETIC-KEY",
    authHeader: true,
  });
  const baseLoader = new DefaultResourceLoader({
    cwd: lingxiHome, agentDir: lingxiHome,
    noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
  });
  await baseLoader.reload();
  const created = await createAgentSession({
    model: {
      id: "p07-witness-model", provider: "p07-witness-provider", api: "openai-completions",
      baseUrl: `${witness.baseUrl}/v1`, maxTokens: 32768, input: ["text"],
      cost: { input: 1, output: 2, cacheRead: 0, cacheWrite: 0, total: 0 },
    },
    modelRuntime: runtime,
    sessionManager: SessionManager.inMemory(),
    resourceLoader: baseLoader,
    cwd: lingxiHome,
    tools: [],
  });
  return created.session;
}

/* ── W3 Layer 2 ───────────────────────────────────────────────────────── */
async function w3Layer2(witness) {
  const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p07-w3l2-"));
  try {
    const deltaText = "流式解析层基准 delta，mixed EN content。";
    const results = {};
    for (const n of [200, 2000, 20000]) {
      const body = sseBodyWithDeltas(n, deltaText);
      // 预热 1 + 样本 30（协议 §2.4；启动类 ≥10、连续类 ≥30——本项为连续指标）
      const runs = [];
      for (let r = 0; r < 31; r++) {
        const session = await createWitnessRuntimeSession(witness, lingxiHome);
        witness.scriptNext({ kind: "sse", body });
        const t0 = performance.now();
        await session.prompt("P07_W3L2_INPUT");
        const wallMs = performance.now() - t0;
        // 输出等价性：读 session.messages 最后一条 assistant 消息正文长度
        // （session-coordinator 同款消费面：Array.isArray(session.messages)）。
        const messages = Array.isArray(session?.messages) ? session.messages
          : (Array.isArray(session?.agent?.state?.messages) ? session.agent.state.messages : []);
        const lastAssistant = [...messages].reverse().find((m) => m?.role === "assistant");
        const text = typeof lastAssistant?.content === "string" ? lastAssistant.content
          : Array.isArray(lastAssistant?.content)
            ? lastAssistant.content.map((b) => (typeof b?.text === "string" ? b.text : "")).join("")
            : "";
        const ok = text.length === n * deltaText.length;
        runs.push({ wallMs, ok, replyLen: text.length, expectLen: n * deltaText.length });
        await session.dispose?.();
        if (!ok) throw new Error(`W3L2 n=${n} 输出不等价：${text.length} != ${n * deltaText.length}`);
      }
      const warm = runs.slice(1);
      results[`n${n}`] = {
        runs: warm.length,
        prompt_wall_ms: stats(warm.map((r) => r.wallMs)),
        deltas_per_sec: stats(warm.map((r) => Math.round(n / (r.wallMs / 1000)))),
        output_equivalence: `${warm.every((r) => r.ok) ? "all-equal" : "MISMATCH"}`,
      };
      console.log(`[W3-layer2] n=${n} median=${results[`n${n}`].prompt_wall_ms.median}ms (${results[`n${n}`].deltas_per_sec.median} deltas/s)`);
    }
    return results;
  } finally {
    fs.rmSync(lingxiHome, { recursive: true, force: true });
  }
}

/* ── W4 取消时延 ──────────────────────────────────────────────────────── */
async function w4Cancel(witness) {
  const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p07-w4-"));
  const results = {};
  try {
    // S1：prompt 后 500ms 取消（首字节前）。witness 延迟 15s。
    {
      const samples = [];
      for (let r = 0; r < 11; r++) {
        const session = await createWitnessRuntimeSession(witness, lingxiHome);
        witness.scriptNext({ kind: "sse", body: sseBodyWithDeltas(50, "LATE_REPLY_"), delayMs: 15000 });
        const reqCountBefore = witness.requestCount();
        const promptDone = session.prompt("P07_W4_S1").then(
          () => ({ outcome: "resolved" }),
          (err) => ({ outcome: "rejected", name: err?.name, msg: String(err?.message || err).slice(0, 120) }),
        );
        await new Promise((res) => setTimeout(res, 500));
        const tAbort = performance.now();
        const abortPromise = Promise.resolve(session.abort()).catch(() => {});
        const done = await promptDone;
        const settleMs = performance.now() - tAbort;
        await abortPromise;
        await session.dispose?.();
        samples.push({ settleMs, outcome: done.outcome, name: done.name ?? null });
        if (witness.requestCount() !== reqCountBefore + 1) throw new Error("S1 witness 请求计数异常");
      }
      const warm = samples.slice(1);
      results.s1_pre_first_byte_abort_at_500ms = {
        runs: warm.length,
        abort_to_settle_ms: stats(warm.map((s) => s.settleMs)),
        outcomes: [...new Set(warm.map((s) => `${s.outcome}:${s.name ?? ""}`))],
        witness_request_delta_per_run: 1,
      };
      console.log(`[W4-S1] abort→settle median=${results.s1_pre_first_byte_abort_at_500ms.abort_to_settle_ms.median}ms`);
    }

    // S2：流中 5s 取消。sse-bytes 长流（400 片 × 250ms = 100s 名义时长）。
    {
      const deltaChunk = Buffer.from(`data: ${JSON.stringify({ id: "w4s2", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { content: "S2_" }, finish_reason: null }] })}\n\n`, "utf8");
      const doneChunk = Buffer.from(`data: ${JSON.stringify({ id: "w4s2", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "stop" }] })}\n\ndata: [DONE]\n\n`, "utf8");
      const chunks = Array.from({ length: 400 }, () => deltaChunk);
      const samples = [];
      for (let r = 0; r < 6; r++) {
        const session = await createWitnessRuntimeSession(witness, lingxiHome);
        witness.scriptNext({ kind: "sse-bytes", chunks: [...chunks, doneChunk], interChunkDelayMs: 250 });
        const reqCountBefore = witness.requestCount();
        const promptDone = session.prompt("P07_W4_S2").then(
          () => ({ outcome: "resolved" }),
          (err) => ({ outcome: "rejected", name: err?.name }),
        );
        await new Promise((res) => setTimeout(res, 5000));
        const tAbort = performance.now();
        const abortPromise = Promise.resolve(session.abort()).catch(() => {});
        const done = await promptDone;
        const settleMs = performance.now() - tAbort;
        await abortPromise;
        await session.dispose?.();
        samples.push({ settleMs, outcome: done.outcome });
        if (witness.requestCount() !== reqCountBefore + 1) throw new Error("S2 witness 请求计数异常");
      }
      results.s2_mid_stream_abort_at_5s = {
        runs: samples.length,
        abort_to_settle_ms: stats(samples.map((s) => s.settleMs)),
        outcomes: [...new Set(samples.map((s) => s.outcome))],
        note: "每轮含 5s 流等待（名义总时长约 100s 流在 5s 处被取消）；样本 6 轮（时长受限于协议场景，如实登记）",
      };
      console.log(`[W4-S2] abort→settle median=${results.s2_mid_stream_abort_at_5s.abort_to_settle_ms.median}ms`);
    }
    return results;
  } finally {
    fs.rmSync(lingxiHome, { recursive: true, force: true });
  }
}

/* ── W4 S3：工具执行中取消（真实子进程树，不经 witness） ───────────────── */
async function w4S3ToolCancel() {
  const controller = new AbortController();
  const treeScript = `
    const { spawn } = require("node:child_process");
    const grandchild = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" });
    process.stdout.write(JSON.stringify({ grandchildPid: grandchild.pid }) + "\\n");
    setInterval(() => {}, 1000);
  `;
  const sentinel = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { detached: true, stdio: "ignore" });
  const chunks = [];
  const execution = spawnAndStream(process.execPath, ["-e", treeScript], {
    cwd: REPO, env: process.env,
    onData: (buf) => chunks.push(buf),
    signal: controller.signal,
    timeout: 0,
  }).then(
    (value) => ({ outcome: "resolved", value }),
    (error) => ({ outcome: "rejected", error }),
  );
  // 等孙进程 pid 握手
  let grandchildPid = null;
  const deadline = Date.now() + 10000;
  while (Date.now() < deadline) {
    const text = Buffer.concat(chunks).toString("utf8");
    const match = text.match(/"grandchildPid":(\d+)/);
    if (match) { grandchildPid = Number(match[1]); break; }
    await new Promise((r) => setTimeout(r, 25));
  }
  if (!grandchildPid) throw new Error("S3 孙进程 pid 握手超时");
  await new Promise((r) => setTimeout(r, 500)); // 让任务树进入稳态运行（"工具执行中"）
  const tAbort = performance.now();
  controller.abort();
  const done = await execution;
  const settleMs = performance.now() - tAbort;
  await new Promise((r) => setTimeout(r, 300)); // 给进程表一点收敛时间
  const alive = (pid) => { try { process.kill(pid, 0); return true; } catch (err) { return err?.code === "EPERM"; } };
  const grandchildAlive = alive(grandchildPid);
  const sentinelAlive = alive(sentinel.pid);
  try { process.kill(-sentinel.pid, "SIGKILL"); } catch { /* 独立进程组清理 */ }
  try { process.kill(sentinel.pid, "SIGKILL"); } catch { /* 已退出 */ }
  return {
    s3_tool_exec_abort: {
      abort_to_settle_ms: +settleMs.toFixed(1),
      outcome: done.outcome,
      error_name: done.outcome === "rejected" ? done.error?.name : null,
      grandchild_exited: !grandchildAlive,
      sentinel_survived: sentinelAlive,
      correctness: (!grandchildAlive && sentinelAlive) ? "PASS" : "FAIL",
    },
  };
}

/* ── 主流程 ───────────────────────────────────────────────────────────── */
const witness = await startFakeProviderWitness();
let report;
try {
  const w3 = await w3Layer2(witness);
  const w4sessions = await w4Cancel(witness);
  const w4s3 = await w4S3ToolCancel();
  report = {
    w3_layer2_model_stream: {
      workload: "真实 Pi agent session + 本地 HTTP 供应商替身（scenario harness witness）：SSE N delta → prompt() 全程墙钟",
      results: w3,
    },
    w4_cancel_latency: {
      workload: "取消响应时延：abort→prompt settled（S1/S2）与 abort→子进程树退出（S3）",
      ...w4sessions,
      ...w4s3,
    },
    env: { node: process.version, os: `${os.type()} ${os.release()} ${os.arch()}` },
  };
} finally {
  await witness.close();
}
fs.mkdirSync(path.dirname(out), { recursive: true });
fs.writeFileSync(out, JSON.stringify(report, null, 2));
console.log(`[bench] out=${out}`);
const s3ok = report.w4_cancel_latency.s3_tool_exec_abort.correctness === "PASS";
process.exit(s3ok ? 0 : 1);
