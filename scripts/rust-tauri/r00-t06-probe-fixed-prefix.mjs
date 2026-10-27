#!/usr/bin/env node
/**
 * R00-T06｜R1 修复（F04）：固定前缀 10,620B 的可复现真实会话路径证据采集
 *
 * 背景：阈值 fixed_prefix_overhead.baseline_g1.system_prompt_bytes=10620 声称取自
 * 「会话缓存契约诊断」，但该诊断输出未落盘、全部 artifacts 无原始记录；且 stub
 * journal 显示不同请求体规模差异巨大（stream 相 666B / cancel 相 28,590B），指标
 * 的会话类型与字节口径不明确（R1-F04）。
 *
 * 本探针在同一冻结夹具/发布优化产物上补采两级证据（全部合成数据，零真实供应商）：
 *   1. wire 级：自建最小确定性 openai-completions 应答器（与 stub 同协议）作为
 *      provider 端点，逐请求记录请求体分解——requestBytes / messageCount /
 *      toolCount / systemMessageBytes（首条 system 消息 UTF-8 字节）/
 *      toolsJsonBytes（tools 数组 JSON 字节）/ lastUserBytes，以及消息角色与
 *      内容形态（用于解释 666B 与 28,590B 的构成）。
 *   2. 会话级：真实打包 server（dist-server/mac-arm64/hana-server）以
 *      LINGXI_CACHE_CONTRACT_DEBUG=1 运行（产品自带诊断开关，
 *      core/session-coordinator.ts:380），捕获 cache_contract_renew /
 *      cache_contract_check 日志行中的 systemPromptBytes（= 会话最终 system
 *      prompt 的 UTF-8 字节，lib/llm/cache-prefix-contract.ts:81 的字节定义）。
 *
 * 复现两类基准会话（与 bench-server 相同入口/参数）：
 *   A. stream 型：POST /api/sessions/new {memoryEnabled:false, permissionMode:"operate"}
 *      → WS prompt "bench-stream-0 direct answer, no tools."（fast 应答）。
 *   B. cancel 型：新会话 → WS prompt "bench-cancel-0 hold the stream."（hang 应答）
 *      → 等 5 chunk 后 WS abort → abort_result。
 *
 * 用法：node scripts/rust-tauri/r00-t06-probe-fixed-prefix.mjs
 * 输出：artifacts/rust-tauri/R00/T06/raw/server/fixed-prefix-probe-<ts>.json
 */
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import WebSocket from "ws";
import { collectHostInfo, isoNow, nowMs, sleep, writeJson, waitForTreeExit, sigkillTree } from "./r00-t06-lib.mjs";
import { buildPristineHome } from "./r00-t06-fixture.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..", "..");
const OS_DIR = process.platform === "darwin" ? "mac" : process.platform;
const SERVER_WRAPPER = path.join(ROOT, "dist-server", `${OS_DIR}-${process.arch}`, "hana-server");
const TOKEN = "r00t06-not-a-secret";
const OUT_DIR = path.join(ROOT, "artifacts", "rust-tauri", "R00", "T06", "raw", "server");
fs.mkdirSync(OUT_DIR, { recursive: true });

// ── 确定性应答语料（与 r00-t06-stub-provider 同表，非随机） ──
const PANGRAM = "灵犀基线测量：the quick brown fox jumps over the lazy dog 0123456789";
const CHUNKS = 24, CHUNK_CHARS = 64;
function chunkText(seq) {
  let s = "";
  for (let i = 0; i < CHUNK_CHARS; i++) s += PANGRAM[(seq * CHUNK_CHARS + i) % PANGRAM.length];
  return s;
}
function fullText() {
  return Array.from({ length: CHUNKS }, (_, i) => chunkText(i)).join("");
}

// ── probe：最小 openai-completions 应答器 + 请求体分解记录 ──
const probeRecords = [];
let probeMode = "fast";
const state = { seq: 0, toolDirectivesSent: new Set() };

function messageBytes(m) {
  return Buffer.byteLength(typeof m?.content === "string" ? m.content : JSON.stringify(m?.content ?? ""), "utf8");
}
function contentShape(m) {
  const c = m?.content;
  if (typeof c === "string") return "string";
  if (Array.isArray(c)) return `array[${c.length}]:${c.map((b) => b?.type ?? typeof b).join("|").slice(0, 60)}`;
  return c === null || c === undefined ? "empty" : typeof c;
}
function excerpt(s, n = 100) {
  const t = typeof s === "string" ? s : JSON.stringify(s ?? "");
  return t.slice(0, n).replace(/\s+/g, " ");
}

async function handleChat(req, res, bodyBuf, url) {
  const seq = ++state.seq;
  const tReq = Date.now();
  let body = null;
  try { body = JSON.parse(bodyBuf.toString("utf8")); } catch {}
  const messages = Array.isArray(body?.messages) ? body.messages : [];
  const sys = messages.find((m) => m?.role === "system") ?? null;
  const lastUser = [...messages].reverse().find((m) => m?.role === "user") ?? null;
  const toolsJson = Array.isArray(body?.tools) ? JSON.stringify(body.tools) : null;

  // 工具指令（与 stub 同契约：TOOL:<name>:<json> → tool_calls 一轮，之后纯文本收尾）
  let directive = null;
  {
    const text = typeof lastUser?.content === "string" ? lastUser.content : "";
    const m = text.match(/TOOL:([A-Za-z0-9_.-]+):(\{.*\})/);
    const markerM = text.match(/bench-[a-z]+-\d+/);
    const marker = markerM ? markerM[0] : null;
    if (m) {
      try {
        const args = JSON.parse(m[2]);
        if (!state.toolDirectivesSent.has(marker)) { state.toolDirectivesSent.add(marker); directive = { name: m[1], args }; }
      } catch {}
    }
  }

  const record = {
    seq, ts: isoNow(), tReqReceivedMs: tReq,
    mode: probeMode,
    endpoint: url.pathname,
    stream: body?.stream === true,
    model: body?.model ?? null,
    requestBytes: bodyBuf.length,
    messageCount: messages.length,
    roles: messages.map((m) => `${m?.role}(${contentShape(m)},${messageBytes(m)}B)`),
    toolCount: Array.isArray(body?.tools) ? body.tools.length : null,
    toolsJsonBytes: toolsJson ? Buffer.byteLength(toolsJson, "utf8") : null,
    toolNames: Array.isArray(body?.tools) ? body.tools.map((t) => t?.function?.name ?? t?.name).filter(Boolean) : null,
    hasSystemPrompt: !!sys,
    systemMessageBytes: sys ? messageBytes(sys) : null,
    systemMessageExcerpt: sys ? excerpt(sys.content) : null,
    lastUserBytes: lastUser ? messageBytes(lastUser) : null,
    lastUserExcerpt: lastUser ? excerpt(lastUser.content, 80) : null,
    marker: (typeof lastUser?.content === "string" ? lastUser.content.match(/bench-[a-z]+-\d+/)?.[0] : null) ?? null,
    toolDirective: directive,
  };
  probeRecords.push(record);

  const wantsStream = body?.stream === true;
  if (probeMode === "delayed") await sleep(250);
  const tFirst = Date.now();
  res.writeHead(200, {
    "Content-Type": wantsStream ? "text/event-stream" : "application/json",
    "Cache-Control": "no-store",
  });
  const sse = (p) => res.write(`data: ${JSON.stringify(p)}\n\n`);
  const chunk = (delta = {}, finishReason = null) => ({
    id: "chatcmpl-r00t06-probe", object: "chat.completion.chunk", created: 1760000000,
    model: body?.model || "bench-fast", choices: [{ index: 0, delta, finish_reason: finishReason }],
  });

  if (directive) {
    if (wantsStream) {
      sse(chunk({ role: "assistant" }));
      sse(chunk({ tool_calls: [{ index: 0, id: `call-probe-${seq}`, type: "function", function: { name: directive.name, arguments: JSON.stringify(directive.args) } }] }));
      sse(chunk({}, "tool_calls"));
      res.write("data: [DONE]\n\n"); res.end();
    } else {
      const payload = {
        id: "chatcmpl-r00t06-probe", object: "chat.completion", created: 1760000000, model: body?.model || "bench-fast",
        choices: [{ index: 0, message: { role: "assistant", content: null, tool_calls: [{ id: `call-probe-${seq}`, type: "function", function: { name: directive.name, arguments: JSON.stringify(directive.args) } }] }, finish_reason: "tool_calls" }],
        usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
      };
      res.end(JSON.stringify(payload));
    }
  } else if (wantsStream) {
    const total = probeMode === "hang" ? 5 : CHUNKS;
    sse(chunk({ role: "assistant" }));
    for (let i = 0; i < total; i++) { sse(chunk({ content: chunkText(i) })); if (probeMode === "delayed") await sleep(10); }
    if (probeMode === "hang") {
      record.tFirstByteMs = tFirst; record.hanging = true;
      req.on("close", () => { record.tClientCloseMs = Date.now(); try { res.end(); } catch {} });
      return; // 停住不结束（取消负载）
    }
    sse(chunk({}, "stop"));
    res.write("data: [DONE]\n\n"); res.end();
  } else {
    const payload = {
      id: "chatcmpl-r00t06-probe", object: "chat.completion", created: 1760000000, model: body?.model || "bench-fast",
      choices: [{ index: 0, message: { role: "assistant", content: fullText() }, finish_reason: "stop" }],
      usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
    };
    res.end(JSON.stringify(payload));
  }
  record.tFirstByteMs = tFirst;
  record.tLastByteMs = Date.now();
}

const probe = http.createServer((req, res) => {
  const url = new URL(req.url, "http://127.0.0.1");
  if (req.method === "POST" && url.pathname.endsWith("/chat/completions")) {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => handleChat(req, res, Buffer.concat(chunks), url).catch((e) => { try { res.writeHead(500); res.end(String(e?.stack || e)); } catch {} }));
    return;
  }
  if (req.method === "GET" && url.pathname === "/control") {
    const mode = url.searchParams.get("mode");
    if (["fast", "hang"].includes(mode)) probeMode = mode;
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ ok: true, mode: probeMode }));
    return;
  }
  res.writeHead(404); res.end("{}");
});

// ── 主流程 ──
const evidence = {
  probe: "R00-T06 R1 修复 F04：固定前缀真实会话路径证据",
  capturedAt: isoNow(),
  head: null, host: collectHostInfo({ repoRoot: ROOT }),
  serverWrapper: path.relative(ROOT, SERVER_WRAPPER),
  sessionTypes: [
    "A stream 型：POST /api/sessions/new {memoryEnabled:false,permissionMode:\"operate\"} → WS /ws prompt 'bench-stream-0 direct answer, no tools.'（fast）",
    "B cancel 型：同入口新会话 → WS prompt 'bench-cancel-0 hold the stream.'（hang）→ 首 5 chunk 后 WS abort",
  ],
  byteDefinitions: {
    systemPromptBytes: "会话最终 system prompt 的 UTF-8 字节（产品诊断 LINGXI_CACHE_CONTRACT_DEBUG=1 的 cache_contract_* 日志，lib/llm/cache-prefix-contract.ts:81）",
    systemMessageBytes: "wire 请求首条 role=system 消息 content 的 UTF-8 字节（探针测量）",
    requestBytes: "HTTP 请求体总字节（探针测量，与 stub journal requestBytes 同口径）",
    toolsJsonBytes: "wire 请求 tools 数组的 JSON 序列化字节（探针测量）",
  },
  cacheContractLines: [],
  probeRecords: null,
  notes: [],
};

function extractCacheContract(logText) {
  const out = [];
  for (const line of logText.split("\n")) {
    const idx = line.indexOf("cache_contract_");
    if (idx >= 0) out.push(line.slice(idx)); // 完整行不截断：systemPromptBytes 在 JSON 行尾
  }
  return out;
}

function wsChat(port) {
  return new Promise((resolve) => {
    const events = [];
    const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`, { headers: { Authorization: `Bearer ${TOKEN}` } });
    const mark = (ev) => { ev.__recvMs = nowMs(); events.push(ev); };
    ws.on("open", () => resolve({ ws, events, mark }));
    ws.on("message", (raw) => { try { mark(JSON.parse(raw.toString())); } catch { mark({ type: "_unparsable" }); } });
    ws.on("error", () => {});
  });
}
const findEv = (events, pred) => events.find(pred) || null;

async function main() {
  evidence.head = evidence.host.gitHead;
  if (!fs.existsSync(SERVER_WRAPPER)) throw new Error(`missing ${SERVER_WRAPPER}`);

  await new Promise((r) => probe.listen(0, "127.0.0.1", r));
  const pAddr = probe.address();
  const probeBase = `http://${pAddr.address}:${pAddr.port}/v1`;

  const home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-r00t06-fpprobe-"));
  buildPristineHome({ dest: home, stubBaseUrl: probeBase });

  const child = spawn(SERVER_WRAPPER, [], {
    cwd: path.dirname(SERVER_WRAPPER),
    env: {
      PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
      HOME: home, LINGXI_HOME: home,
      LINGXI_PORT: "0", LINGXI_TOKEN: TOKEN,
      LINGXI_CREATE_STARTUP_SESSION: "0",
      LINGXI_CACHE_CONTRACT_DEBUG: "1",
      TMPDIR: os.tmpdir(),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logBuf = [];
  child.stdout.on("data", (c) => logBuf.push(c.toString()));
  child.stderr.on("data", (c) => logBuf.push(c.toString()));

  try {
    // 就绪（与 bench-server 同双信号）
    const infoPath = path.join(home, "server-info.json");
    let info = null;
    const t0 = Date.now();
    while (Date.now() - t0 < 60000 && !info) {
      try {
        const p = JSON.parse(fs.readFileSync(infoPath, "utf8"));
        if (p?.port && p?.pid === child.pid) info = p;
      } catch {}
      await sleep(25);
    }
    if (!info) throw new Error("server-info.json not ready");
    let ok = false;
    while (Date.now() - t0 < 60000 && !ok) {
      try {
        const r = await fetch(`http://127.0.0.1:${info.port}/api/health`, { headers: { Authorization: `Bearer ${TOKEN}` } });
        if (r.ok) ok = true;
      } catch {}
      await sleep(50);
    }

    const newSession = async () => {
      const res = await fetch(`http://127.0.0.1:${info.port}/api/sessions/new`, {
        method: "POST",
        headers: { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" },
        body: JSON.stringify({ memoryEnabled: false, permissionMode: "operate" }),
        signal: AbortSignal.timeout(15000),
      });
      if (!res.ok) throw new Error(`sessions/new ${res.status}: ${await res.text()}`);
      return res.json();
    };

    // ── A. stream 型会话 ──
    await fetch(`http://${pAddr.address}:${pAddr.port}/control?mode=fast`);
    const sA = await newSession();
    const chatA = await wsChat(info.port);
    const tA = nowMs();
    chatA.ws.send(JSON.stringify({ type: "prompt", text: "bench-stream-0 direct answer, no tools.", sessionPath: sA.path }));
    await new Promise((resolve) => {
      const iv = setInterval(() => {
        const end = findEv(chatA.events, (e) => (e.type === "assistant_run_end" || e.type === "turn_end" || e.type === "agent_settled") && e.__recvMs > tA);
        if (end) { clearInterval(iv); resolve(); }
      }, 5);
      setTimeout(() => { clearInterval(iv); resolve(); }, 60000);
    });
    await sleep(1500); // 等会话侧线调用（如标题/摘要）落完
    try { chatA.ws.close(); } catch {}

    // ── B. cancel 型会话（hang 中 abort） ──
    await fetch(`http://${pAddr.address}:${pAddr.port}/control?mode=hang`);
    const sB = await newSession();
    const chatB = await wsChat(info.port);
    const tB = nowMs();
    chatB.ws.send(JSON.stringify({ type: "prompt", text: "bench-cancel-0 hold the stream.", sessionPath: sB.path }));
    const firstDelta = await new Promise((resolve) => {
      const iv = setInterval(() => {
        const d = findEv(chatB.events, (e) => /delta/i.test(e.type || "") && e.__recvMs > tB);
        if (d) { clearInterval(iv); resolve(d.__recvMs - tB); }
      }, 5);
      setTimeout(() => { clearInterval(iv); resolve(null); }, 30000);
    });
    if (firstDelta === null) throw new Error("cancel session: no first delta");
    chatB.ws.send(JSON.stringify({ type: "abort", sessionPath: sB.path }));
    await new Promise((resolve) => {
      const iv = setInterval(() => {
        const ar = findEv(chatB.events, (e) => e.type === "abort_result" && e.__recvMs > tB);
        if (ar) { clearInterval(iv); resolve(ar); }
      }, 2);
      setTimeout(() => { clearInterval(iv); resolve(null); }, 15000);
    });
    await sleep(1500);
    try { chatB.ws.close(); } catch {}
    await fetch(`http://${pAddr.address}:${pAddr.port}/control?mode=fast`);

    evidence.cacheContractLines = extractCacheContract(logBuf.join(""));
    evidence.probeRecords = probeRecords;
    evidence.serverLogTail = logBuf.join("").slice(-4000);
    evidence.leftover = null;
  } finally {
    try { child.kill("SIGTERM"); } catch {}
    const exitMs = await waitForTreeExit(child.pid, 20000, 50);
    if (exitMs === null) { sigkillTree(child.pid); evidence.cleanupForceKilled = true; }
    else evidence.cleanupSigtermTreeExitMs = exitMs;
    await new Promise((r) => probe.close(r));
    fs.rmSync(home, { recursive: true, force: true });
    const after = spawnSync("ps", ["-o", "pid=,comm=", "-ax"], { encoding: "utf8" });
    evidence.leftover = after.stdout.split("\n").filter((l) => /hana-server|lingxi/.test(l) && !/probe-fixed-prefix/.test(l)).slice(0, 5);
  }

  const out = path.join(OUT_DIR, `fixed-prefix-probe-${new Date().toISOString().replace(/[:.]/g, "-")}.json`);
  writeJson(out, evidence);
  console.log(JSON.stringify({
    ok: true,
    out: path.relative(ROOT, out),
    requests: probeRecords.length,
    cacheContractLines: evidence.cacheContractLines.length,
    summary: probeRecords.map((r) => ({
      seq: r.seq, stream: r.stream, requestBytes: r.requestBytes, msgCount: r.messageCount,
      toolCount: r.toolCount, sysBytes: r.systemMessageBytes, toolsJsonBytes: r.toolsJsonBytes,
      marker: r.marker, roles: r.roles,
    })),
  }, null, 2));
}

main().catch((err) => { console.error(err); process.exit(1); });
