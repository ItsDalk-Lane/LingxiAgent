/**
 * R00-T06｜确定性本地模型协议服务（stub provider）
 *
 * 职责（R00-T06 协议 §模型等待分离）：
 * - 实现旧系统 openai-completions 协议（POST {base}/chat/completions，SSE 流式
 *   与非流式），返回**确定性**内容：同一请求输入 → 同一输出字节，无随机源。
 * - 逐请求记录服务端时间线（收到请求 / 首字节 / 末字节）与请求体规模
 *   （messages 数、工具数、请求字节、system 提示存在性）——客户端测得的端到端
 *   时延减去 stub 服务时间即旧系统本地开销；模型等待按模式显式模拟并单列。
 * - 三种模式（/control 切换，切换本身记入日志）：
 *     fast    —— 零延迟全速输出（默认；测本地开销）
 *     delayed —— 固定 250ms 首字节延迟 + 每 chunk 10ms（模拟模型思考/生成等待）
 *     hang    —— 输出 5 个 chunk 后停住不结束（供取消测试：请求停留在流中）
 * - /page/<name>：供浏览器视图加载的确定性本地 HTML（无外网）。
 * - /info：端口/模式/累计计数；/journal：原始请求日志；/journal?reset=1 清空。
 *
 * 全部数据合成；不调用任何真实供应商；监听 127.0.0.1。
 */
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isoNow, appendJsonl } from "./r00-t06-lib.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// ── 确定性输出语料（固定表，非随机） ──
const PANGRAM = "灵犀基线测量：the quick brown fox jumps over the lazy dog 0123456789";
export const STUB_CONFIG = {
  chunkCount: 24,
  chunkChars: 64,
  delayedFirstByteMs: 250,
  delayedInterChunkMs: 10,
  hangChunksBeforeStall: 5,
};

function chunkText(seq) {
  // 确定性：chunk i 的内容只依赖 i
  let s = "";
  for (let i = 0; i < STUB_CONFIG.chunkChars; i++) {
    s += PANGRAM[(seq * STUB_CONFIG.chunkChars + i) % PANGRAM.length];
  }
  return s;
}

export function fullReplyText() {
  return Array.from({ length: STUB_CONFIG.chunkCount }, (_, i) => chunkText(i)).join("");
}

// ── 服务状态 ──
const state = {
  mode: "fast",
  startedAt: isoNow(),
  requestSeq: 0,
  pagesServed: 0,
  modeChanges: [],
  journalPath: null,
};

function sseChunk(payload) {
  return `data: ${JSON.stringify(payload)}\n\n`;
}

function chatCompletionsChunk({ model, delta = {}, finishReason = null }) {
  return {
    id: "chatcmpl-r00t06-deterministic",
    object: "chat.completion.chunk",
    created: 1760000000,
    model: model || "bench-fast",
    choices: [{ index: 0, delta, finish_reason: finishReason }],
  };
}

function chatCompletionsFull({ model, text }) {
  return {
    id: "chatcmpl-r00t06-deterministic",
    object: "chat.completion",
    created: 1760000000,
    model: model || "bench-fast",
    choices: [{
      index: 0,
      message: { role: "assistant", content: text },
      finish_reason: "stop",
    }],
    usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
  };
}

// ── 工具指令支持（浏览器多实例负载经真实 agent 工具链驱动） ──
// 请求最后一条 user 消息里带 `TOOL:<name>:<json-args>` 时，stub 返回 tool_calls。
// 同一 marker 只发一次指令：工具结果回传后的下一轮返回纯文本收尾，
// 模拟真实模型“调用→看结果→总结”行为，避免确定性重试把 loop guard 打爆。
const toolDirectivesSent = new Set();

function parseToolDirective(messages) {
  const lastUser = [...(messages || [])].reverse().find((m) => m?.role === "user");
  const text = typeof lastUser?.content === "string"
    ? lastUser.content
    : Array.isArray(lastUser?.content)
      ? lastUser.content.filter((b) => typeof b?.text === "string").map((b) => b.text).join(" ")
      : "";
  const m = text.match(/TOOL:([A-Za-z0-9_.-]+):(\{.*\})/);
  if (!m) return null;
  let args;
  try { args = JSON.parse(m[2]); } catch { return null; }
  const markerM = text.match(/bench-[a-z]+-\d+/);
  const marker = markerM ? markerM[0] : text.slice(0, 64);
  if (toolDirectivesSent.has(marker)) return null; // 已发过 → 纯文本收尾
  toolDirectivesSent.add(marker);
  return { name: m[1], args };
}

async function handleChatCompletions(req, res, bodyBuf) {
  const seq = ++state.requestSeq;
  const tReq = Date.now();
  let body = null;
  try { body = JSON.parse(bodyBuf.toString("utf8")); } catch {}

  const record = {
    seq,
    ts: isoNow(),
    tReqReceivedMs: tReq,
    mode: state.mode,
    model: body?.model ?? null,
    stream: body?.stream === true,
    requestBytes: bodyBuf.length,
    messageCount: Array.isArray(body?.messages) ? body.messages.length : null,
    toolCount: Array.isArray(body?.tools) ? body.tools.length : null,
    hasSystemPrompt: (body?.messages || []).some((m) => m?.role === "system"),
    toolDirective: null,
    marker: null,
  };
  const directive = parseToolDirective(body?.messages);
  if (directive) record.toolDirective = directive;

  // 从最后一条 user 消息提取 harness 样本标记 bench-<kind>-<i>
  const lastUserText = typeof (body?.messages || []).filter((m) => m?.role === "user").slice(-1)[0]?.content === "string"
    ? (body?.messages || []).filter((m) => m?.role === "user").slice(-1)[0].content
    : "";
  const marker = lastUserText.match(/bench-[a-z]+-\d+/);
  if (marker) record.marker = marker[0];

  const mode = state.mode;
  const wantsStream = body?.stream === true;

  if (mode === "delayed") {
    await new Promise((r) => setTimeout(r, STUB_CONFIG.delayedFirstByteMs));
  }

  const tFirst = Date.now();
  res.writeHead(200, {
    "Content-Type": wantsStream ? "text/event-stream" : "application/json",
    "Cache-Control": "no-store",
  });

  if (!wantsStream) {
    const payload = directive
      ? chatCompletionsFull({ model: body?.model, text: "" })
      : chatCompletionsFull({ model: body?.model, text: fullReplyText() });
    if (directive) {
      payload.choices[0].message.content = null;
      payload.choices[0].message.tool_calls = [{
        id: `call-r00t06-${seq}`,
        type: "function",
        function: { name: directive.name, arguments: JSON.stringify(directive.args) },
      }];
      payload.choices[0].finish_reason = "tool_calls";
    }
    res.end(JSON.stringify(payload));
    record.tFirstByteMs = tFirst;
    record.tLastByteMs = Date.now();
    record.outBytes = Buffer.byteLength(JSON.stringify(payload));
    finishRecord(record);
    return;
  }

  // 流式
  let outBytes = 0;
  const write = (payload) => {
    const s = sseChunk(payload);
    outBytes += Buffer.byteLength(s);
    res.write(s);
  };
  if (directive) {
    write(chatCompletionsChunk({ model: body?.model, delta: { role: "assistant" } }));
    write(chatCompletionsChunk({
      model: body?.model,
      delta: {
        tool_calls: [{
          index: 0,
          id: `call-r00t06-${seq}`,
          type: "function",
          function: { name: directive.name, arguments: JSON.stringify(directive.args) },
        }],
      },
    }));
    write(chatCompletionsChunk({ model: body?.model, delta: {}, finishReason: "tool_calls" }));
  } else {
    const total = mode === "hang" ? STUB_CONFIG.hangChunksBeforeStall : STUB_CONFIG.chunkCount;
    write(chatCompletionsChunk({ model: body?.model, delta: { role: "assistant" } }));
    for (let i = 0; i < total; i++) {
      write(chatCompletionsChunk({ model: body?.model, delta: { content: chunkText(i) } }));
      if (mode === "delayed") await new Promise((r) => setTimeout(r, STUB_CONFIG.delayedInterChunkMs));
    }
    if (mode === "hang") {
      record.tFirstByteMs = tFirst;
      record.hanging = true;
      finishRecord(record);
      // 挂住连接直到客户端断开（取消测试用）
      req.on("close", () => {
        record.tClientCloseMs = Date.now();
        record.tLastByteMs = record.tClientCloseMs;
        record.outBytes = outBytes;
        try { res.end(); } catch {}
        if (state.journalPath) {
          try { appendJsonl(state.journalPath, record); } catch {}
        }
      });
      return; // 不发 finish_reason、不发 [DONE]
    }
    write(chatCompletionsChunk({ model: body?.model, delta: {}, finishReason: "stop" }));
  }
  res.write("data: [DONE]\n\n");
  res.end();
  record.tFirstByteMs = tFirst;
  record.tLastByteMs = Date.now();
  record.outBytes = outBytes;
  finishRecord(record);
}

function finishRecord(record) {
  record.serviceMs = record.tLastByteMs - record.tReqReceivedMs;
  if (state.journalPath) appendJsonl(state.journalPath, record);
}

// ── 确定性本地页面（浏览器视图负载，无外网） ──
function pageHtml(name) {
  // ~512KB 确定性标记文本；内容只依赖 name
  const head = `<!doctype html><html><head><meta charset="utf-8"><title>${name}</title></head><body><h1>${name}</h1><pre>`;
  const tail = `</pre></body></html>`;
  const targetBytes = 512 * 1024;
  const fillLen = Math.max(0, targetBytes - head.length - tail.length);
  const unit = `bench-page/${name}/0123456789abcdefghijklmnopqrstuvwxyz\n`;
  const fill = unit.repeat(Math.ceil(fillLen / unit.length)).slice(0, fillLen);
  return head + fill + tail;
}

export async function startStubProvider({ host = "127.0.0.1", port = 0, journalPath } = {}) {
  state.journalPath = journalPath || null;
  if (journalPath) {
    fs.mkdirSync(path.dirname(journalPath), { recursive: true });
    fs.writeFileSync(journalPath, ""); // 每轮清空重记
  }

  const server = http.createServer((req, res) => {
    const url = new URL(req.url, `http://${host}`);
    if (req.method === "POST" && url.pathname.endsWith("/chat/completions")) {
      const chunks = [];
      req.on("data", (c) => chunks.push(c));
      req.on("end", () => {
        handleChatCompletions(req, res, Buffer.concat(chunks)).catch((err) => {
          try { res.writeHead(500); res.end(String(err?.stack || err)); } catch {}
        });
      });
      return;
    }
    if (req.method === "GET" && url.pathname.startsWith("/page/")) {
      const name = url.pathname.slice("/page/".length).replace(/[^A-Za-z0-9_.-]/g, "_") || "index";
      const body = pageHtml(name);
      state.pagesServed += 1;
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" });
      res.end(body);
      return;
    }
    if (req.method === "GET" && url.pathname === "/control") {
      const mode = url.searchParams.get("mode");
      if (["fast", "delayed", "hang"].includes(mode)) {
        state.mode = mode;
        state.modeChanges.push({ ts: isoNow(), mode, bySeq: state.requestSeq });
      }
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ ok: true, mode: state.mode }));
      return;
    }
    if (req.method === "GET" && url.pathname === "/info") {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({
        mode: state.mode, startedAt: state.startedAt, requestSeq: state.requestSeq,
        pagesServed: state.pagesServed, modeChanges: state.modeChanges,
        config: STUB_CONFIG,
      }));
      return;
    }
    res.writeHead(404, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ error: "not_found", path: url.pathname }));
  });

  await new Promise((resolve) => server.listen(port, host, resolve));
  const addr = server.address();
  return {
    server,
    host: addr.address,
    port: addr.port,
    baseUrl: `http://${addr.address}:${addr.port}/v1`,
    pageUrl: (name) => `http://${addr.address}:${addr.port}/page/${name}`,
    controlUrl: (mode) => `http://${addr.address}:${addr.port}/control?mode=${mode}`,
    async close() {
      await new Promise((resolve) => server.close(resolve));
    },
  };
}
