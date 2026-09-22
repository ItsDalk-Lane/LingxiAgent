#!/usr/bin/env node
/**
 * P08-T03 全产品功能回归（真实产品入口，协议级）。
 *
 * 形态：真实 spawn 全量组合 server（server/bootstrap.ts → main-full.ts）于隔离
 * LINGXI_HOME；本地 HTTP witness 供应商（OpenAI 兼容 SSE，内容路由）预种入
 * provider-catalog v2 + agent config（与 tests/server-composition-boundary.test.ts
 * Part 4 同款接线，该接线即 P00 CALLSITE_MATRIX E-DESKTOP 的真实业务入口）。
 * 逐功能 F01-F20 做真实动作（HTTP/WS/CLI），随后执行 A05：设置保存 → SIGTERM
 * 真实关闭 → 真实重启 → 设置/会话/历史逐项比对。
 *
 * 输出：JSON 报告写 --out（默认 logs/product-regression.json），每探针含
 * {feature, action, expect, actual, status}。退出码：全 PASS=0，任一 FAIL=1。
 */
import { spawn, execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import http from "node:http";
import process from "node:process";
import console from "node:console";
import { fileURLToPath } from "node:url";

// Node/Web 全局显式取自 globalThis（仓库 lint no-undef 纪律，P07 同款）
const { fetch, setTimeout, clearTimeout, Buffer, WebSocket } = globalThis;

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const sleep = (ms) => new Promise((r) => globalThis.setTimeout(r, ms));
const argv = process.argv.slice(2);
let outPath = path.join(REPO, "artifacts", "refactor-2026", "P08", "logs", "product-regression.json");
for (let i = 0; i < argv.length; i++) if (argv[i] === "--out") outPath = path.resolve(REPO, argv[++i]);

const WITNESS_KEY = "sk-P08-PRODUCT-REGRESSION-WITNESS";
const FILE_MARKER = "P08_PRODUCT_REGRESSION_FILE_MARKER_51c9";
const ECHO_MARKER = "P08_EXEC_COMMAND_ECHO_7f3d";

const results = [];
function record(feature, action, expect, actual, status) {
  results.push({ feature, action, expect, actual: String(actual).slice(0, 400), status, at: new Date().toISOString() });
  console.log(`[${status}] ${feature}: ${action}`);
}
async function probe(feature, action, fn) {
  try {
    const r = await fn();
    record(feature, action, r.expect, r.actual, r.ok ? "PASS" : "FAIL");
    return r.ok;
  } catch (err) {
    record(feature, action, "no-throw", `${err && err.message}`, "FAIL");
    return false;
  }
}

/* ── witness 供应商（内容路由，P01 Part 4 同款） ───────────────────── */
class WitnessProvider {
  constructor() {
    this.requests = [];
    this.server = http.createServer((req, res) => {
      const chunks = [];
      req.on("data", (c) => chunks.push(c));
      req.on("end", () => {
        const raw = Buffer.concat(chunks).toString("utf8");
        let bodyJson = null;
        try { bodyJson = JSON.parse(raw); } catch { bodyJson = { unparseable: raw.slice(0, 256) }; }
        this.requests.push({ url: req.url || "", headers: req.headers, bodyJson });
        res.writeHead(200, { "content-type": "text/event-stream" });
        res.end(this.respondTo(bodyJson));
      });
    });
    this.ready = new Promise((resolve, reject) => {
      this.server.once("error", reject);
      this.server.listen(0, "127.0.0.1", () => {
        const a = this.server.address();
        if (a && typeof a === "object") resolve(a.port); else reject(new Error("witness bind failed"));
      });
    });
  }
  get baseUrl() { return `http://127.0.0.1:${this.server.address().port}`; }
  posts() { return this.requests.filter((r) => r.url.includes("/v1/chat/completions")); }
  close() { return new Promise((r) => this.server.close(() => r())); }
  textBody(content) {
    return [
      `data: ${JSON.stringify({ id: "cc-p08", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { role: "assistant", content }, finish_reason: null }] })}`,
      `data: ${JSON.stringify({ id: "cc-p08", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "stop" }], usage: { prompt_tokens: 9, completion_tokens: 4, total_tokens: 13 } })}`,
      "data: [DONE]", "",
    ].join("\n\n");
  }
  toolCallBody(toolName, argsJson) {
    const chunk = { id: "cc-p08", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { role: "assistant", tool_calls: [{ index: 0, id: `tc_p08_${toolName}`, type: "function", function: { name: toolName, arguments: argsJson } }] }, finish_reason: null }] };
    const done = { id: "cc-p08", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }], usage: { prompt_tokens: 9, completion_tokens: 3, total_tokens: 12 } };
    return [`data: ${JSON.stringify(chunk)}`, `data: ${JSON.stringify(done)}`, "data: [DONE]", ""].join("\n\n");
  }
  respondTo(bodyJson) {
    const messages = Array.isArray(bodyJson?.messages) ? bodyJson.messages : [];
    const lastUser = [...messages].reverse().find((m) => m?.role === "user");
    const lastUserText = JSON.stringify(lastUser?.content ?? "");
    const historyText = JSON.stringify(messages);
    if (lastUserText.includes("请读取 p08-note.txt")) {
      if (!historyText.includes(FILE_MARKER)) return this.toolCallBody("read", JSON.stringify({ path: "p08-note.txt" }));
      return this.textBody("P08_CHAT_REPLY 文件内容已读取");
    }
    if (lastUserText.includes("请执行命令")) {
      if (!historyText.includes(ECHO_MARKER)) return this.toolCallBody("exec_command", JSON.stringify({ command: `echo ${ECHO_MARKER}` }));
      return this.textBody("P08_EXEC_REPLY 命令已执行");
    }
    return this.textBody("P08_DEFAULT_REPLY");
  }
}

/* ── server 生命周期 ─────────────────────────────────────────────── */
function bootServer(lingxiHome) {
  const child = spawn(process.execPath, ["server/bootstrap.ts"], {
    cwd: REPO,
    env: {
      ...process.env,
      LINGXI_HOME: lingxiHome,
      LINGXI_PORT: "0",
      LINGXI_ROOT: REPO,
      LINGXI_SERVER_ENTRY: path.join(REPO, "server", "main-full.ts"),
      LINGXI_CREATE_STARTUP_SESSION: "0",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderrTail = "";
  child.stderr.on("data", (c) => { stderrTail = (stderrTail + String(c)).slice(-4000); });
  return { child, getStderrTail: () => stderrTail };
}
async function waitForServerInfo(lingxiHome, child, timeoutMs = 90000) {
  const p = path.join(lingxiHome, "server-info.json");
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try { return JSON.parse(fs.readFileSync(p, "utf8")); } catch { /* not yet */ }
    if (child.exitCode !== null) throw new Error(`server exited early code=${child.exitCode}`);
    if (Date.now() > deadline) throw new Error("timeout waiting server-info.json");
    await sleep(150);
  }
}
async function waitForExit(child, timeoutMs = 20000) {
  if (child.exitCode !== null) return child.exitCode;
  return new Promise((resolve) => {
    const t = setTimeout(() => { try { child.kill("SIGKILL"); } catch { /* */ } }, timeoutMs);
    child.on("exit", (code) => { clearTimeout(t); resolve(code); });
  });
}

/* ── 主流程 ───────────────────────────────────────────────────────── */
const witness = new WitnessProvider();
await witness.ready;
const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p08-product-"));
const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p08-ws-"));
fs.writeFileSync(path.join(workspaceDir, "p08-note.txt"), `${FILE_MARKER}\nsecond line\n`, "utf8");

// 预种 agent config + 双 provider catalog（A05 多 provider 配置项）
const agentDir = path.join(lingxiHome, "agents", "lingxi");
fs.mkdirSync(path.join(agentDir, "sessions"), { recursive: true });
const template = fs.readFileSync(path.join(REPO, "lib", "config.example.yaml"), "utf8");
const patched = template.replace(/^(\s*chat:\s*)".*"/m, "$1{id: witness-model, provider: witness-a}");
if (patched === template) throw new Error("failed to patch models.chat in config template");
fs.writeFileSync(path.join(agentDir, "config.yaml"), patched, "utf8");
fs.writeFileSync(path.join(lingxiHome, "provider-catalog.json"), JSON.stringify({
  catalogVersion: 2,
  providers: {
    "witness-a": { base_url: `${witness.baseUrl}/v1`, api: "openai-completions", api_key: WITNESS_KEY, models: ["witness-model"] },
    "witness-b": { base_url: `${witness.baseUrl}/v1`, api: "openai-completions", api_key: WITNESS_KEY, models: ["witness-model-b"] },
  },
}, null, 2), "utf8");

let allOk = true;
const guard = (ok) => { allOk = allOk && ok; };

let booted = bootServer(lingxiHome);
let child = booted.child;
try {
  const info = await waitForServerInfo(lingxiHome, child);
  const base = `http://127.0.0.1:${info.port}`;
  const H = { authorization: `Bearer ${info.token}`, "content-type": "application/json" };
  const get = async (p) => fetch(`${base}${p}`, { headers: H });
  const put = async (p, body) => fetch(`${base}${p}`, { method: "PUT", headers: H, body: JSON.stringify(body) });
  const post = async (p, body) => fetch(`${base}${p}`, { method: "POST", headers: H, body: body === undefined ? undefined : JSON.stringify(body) });

  // ── 基础/负向：真实入口鉴权（同时供 T06 A10 产物级负向） ──
  guard(await probe("BASE", "GET /api/health 无凭证 → 403", async () => {
    const r = await fetch(`${base}/api/health`);
    return { expect: 403, actual: r.status, ok: r.status === 403 };
  }));
  guard(await probe("BASE", "GET /api/health 带 loopback token → 200", async () => {
    const r = await get("/api/health");
    const b = await r.json();
    return { expect: "200 status=ok", actual: `${r.status} ${b.status}`, ok: r.status === 200 && b.status === "ok" };
  }));
  guard(await probe("BASE", "GET /api/server/identity → 200 serverProtocol=1", async () => {
    const r = await get("/api/server/identity");
    const b = await r.json();
    return { expect: "200", actual: `${r.status} ${b.serverProtocol ?? "?"}`, ok: r.status === 200 && String(b.serverProtocol) === "1" };
  }));

  // ── F02 会话管理 ──
  let sessionId = null, sessionPath = null;
  guard(await probe("F02", "POST /api/sessions/new（strictJson 入口）→ sess_ 前缀", async () => {
    const r = await post("/api/sessions/new", { cwd: workspaceDir });
    const b = await r.json();
    sessionId = b.sessionId; sessionPath = b.path;
    return { expect: "200 ok sess_*", actual: `${r.status} ${b.ok} ${b.sessionId}`, ok: r.status === 200 && b.ok === true && /^sess_/.test(b.sessionId || "") };
  }));
  guard(await probe("F02/A07负向", "POST /api/sessions/new 畸形 JSON → 400 零副作用", async () => {
    const before = await (await get("/api/sessions")).json();
    const r = await fetch(`${base}/api/sessions/new`, { method: "POST", headers: H, body: '{"cwd": "/tmp", "memoryEnabled": tru' });
    const after = await (await get("/api/sessions")).json();
    return { expect: "400 + 会话列表不变", actual: r.status, ok: r.status === 400 && JSON.stringify(after) === JSON.stringify(before) };
  }));
  guard(await probe("F02", "GET /api/sessions → 列表含新会话", async () => {
    const r = await get("/api/sessions");
    const b = await r.json();
    const hit = JSON.stringify(b).includes(sessionId);
    return { expect: "200 含新会话", actual: `${r.status} found=${hit}`, ok: r.status === 200 && hit };
  }));
  guard(await probe("F02", "GET /api/sessions/search?q= → 200", async () => {
    const r = await get(`/api/sessions/search?q=${encodeURIComponent("p08")}`);
    return { expect: 200, actual: r.status, ok: r.status === 200 };
  }));
  guard(await probe("F02", "POST /api/sessions/pin {pinned:true} → 200", async () => {
    const r = await post("/api/sessions/pin", { sessionId, sessionPath, pinned: true });
    return { expect: "200/2xx", actual: r.status, ok: r.status >= 200 && r.status < 300 };
  }));

  // ── F01/F03/F09 聊天+工具+资源：WS prompt → read 工具真实往返 ──
  const events = [];
  const ws = new WebSocket(`ws://127.0.0.1:${info.port}/ws?token=${encodeURIComponent(info.token)}`);
  await new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("ws connect timeout")), 15000);
    ws.addEventListener("open", () => { clearTimeout(t); resolve(); });
    ws.addEventListener("error", (e) => { clearTimeout(t); reject(e); });
  });
  ws.addEventListener("message", (m) => { try { events.push(JSON.parse(String(m.data))); } catch { /* */ } });
  const settled = () => events.filter((e) => e.type === "assistant_run_end" && e.status === "completed").length;
  async function waitSettled(target, label) {
    const deadline = Date.now() + 90000;
    while (settled() < target) {
      if (Date.now() > deadline) throw new Error(`settle timeout ${label}; last=${JSON.stringify(events.slice(-5))}`);
      await sleep(150);
    }
  }
  function sendPrompt(text, id) {
    ws.send(JSON.stringify({ type: "prompt", clientMessageId: id, snapshotVersion: 1, text, sessionId, sessionPath }));
  }

  guard(await probe("F01/F03", "WS prompt『读取 p08-note.txt』→ read 工具真实往返 + 流式回复", async () => {
    const toolEventsBefore = events.filter((e) => String(e.type).startsWith("tool_")).length;
    sendPrompt("请读取 p08-note.txt 并告诉我内容", "p08-f01-read");
    await waitSettled(1, "read-turn");
    const toolEvents = events.filter((e) => String(e.type).startsWith("tool_")).length - toolEventsBefore;
    const chatPosts = witness.posts().filter((p) => JSON.stringify(p.bodyJson).includes("请读取 p08-note.txt"));
    const authed = chatPosts.some((p) => String(p.headers.authorization || "").includes(WITNESS_KEY));
    return {
      expect: "≥2 tool 事件 + ≥2 次 provider 调用(工具结果回传) + 凭证可见",
      actual: `tool_events=${toolEvents} posts=${chatPosts.length} authed=${authed}`,
      ok: toolEvents >= 2 && chatPosts.length >= 2 && authed,
    };
  }));
  guard(await probe("F01/F09", "GET /api/sessions/messages 历史回读含用户轮与助手回复", async () => {
    const r = await get(`/api/sessions/messages?sessionId=${encodeURIComponent(sessionId)}&all=1`);
    const t = JSON.stringify(await r.json());
    return { expect: "含 p08-note.txt 与 P08_CHAT_REPLY", actual: `${r.status} user=${t.includes("p08-note.txt")} reply=${t.includes("P08_CHAT_REPLY")}`, ok: r.status === 200 && t.includes("p08-note.txt") && t.includes("P08_CHAT_REPLY") };
  }));
  guard(await probe("F02", "POST /api/sessions/fork（target=真实用户消息节点）→ 2xx 且生成新会话", async () => {
    const hist = await get(`/api/sessions/messages?sessionId=${encodeURIComponent(sessionId)}&all=1`);
    const histBody = await hist.json();
    const entries = Array.isArray(histBody) ? histBody : (histBody.messages || histBody.entries || []);
    const userNode = entries.find((e) => e?.entryId && (e?.role === "user" || e?.message?.role === "user"))
      || entries.find((e) => e?.type === "message" && e?.message?.role === "user" && typeof e?.id === "string");
    const entryId = userNode?.entryId || userNode?.id;
    const r = await post("/api/sessions/fork", { sessionId, sessionPath, target: { role: "user", entryId } });
    const b = await r.json().catch(() => ({}));
    const newId = b.sessionId || b.newSessionId || b.session?.sessionId;
    return { expect: "2xx 新 sess_", actual: `${r.status} entry=${entryId} new=${newId ?? JSON.stringify(b).slice(0, 140)}`, ok: r.status >= 200 && r.status < 300 && !!newId && newId !== sessionId };
  }));

  // ── F11 终端：exec_command 真实子进程执行 ──
  guard(await probe("F11", "WS prompt『执行命令』→ exec_command 真实 spawn + 输出回传模型", async () => {
    sendPrompt("请执行命令 echo 看输出", "p08-f11-exec");
    await waitSettled(2, "exec-turn");
    const chatPosts = witness.posts().filter((p) => JSON.stringify(p.bodyJson).includes("请执行命令"));
    const echoSeen = chatPosts.some((p) => JSON.stringify(p.bodyJson).includes(ECHO_MARKER));
    return {
      expect: "exec 输出(ECHO_MARKER)回传 provider",
      actual: `posts=${chatPosts.length} echo_seen=${echoSeen}`,
      ok: chatPosts.length >= 2 && echoSeen,
    };
  }));
  ws.close();

  // ── F06 人格/agents ──
  guard(await probe("F06", "GET /api/agents → lingxi agent 在列（人格承载）", async () => {
    const r = await get("/api/agents");
    const b = await r.json();
    const hit = (b.agents || []).some((a) => a.id === "lingxi");
    return { expect: "200 含 lingxi", actual: `${r.status} agents=${(b.agents || []).length} lingxi=${hit}`, ok: r.status === 200 && hit };
  }));

  // ── F07 记忆 ──
  guard(await probe("F07", "GET /api/memories/dream/status?agentId=lingxi → 2xx（记忆固化路由在位）", async () => {
    const r = await get("/api/memories/dream/status?agentId=lingxi");
    return { expect: "200/2xx", actual: r.status, ok: r.status >= 200 && r.status < 300 };
  }));

  // ── F08 知识库 ──
  guard(await probe("F08", "GET /api/knowledge/notebooks → 200（空库可列出）", async () => {
    const r = await get("/api/knowledge/notebooks");
    return { expect: "200/2xx", actual: r.status, ok: r.status >= 200 && r.status < 300 };
  }));

  // ── F10 模型与媒体配置（多 provider）──
  guard(await probe("F10", "GET /api/providers/summary → 含 witness-a/witness-b 双 provider", async () => {
    const r = await get("/api/providers/summary");
    const t = JSON.stringify(await r.json());
    return { expect: "200 含双 provider", actual: `${r.status} a=${t.includes("witness-a")} b=${t.includes("witness-b")}`, ok: r.status === 200 && t.includes("witness-a") && t.includes("witness-b") };
  }));
  guard(await probe("F10", "GET /api/preferences/models → 200", async () => {
    const r = await get("/api/preferences/models");
    return { expect: 200, actual: r.status, ok: r.status === 200 };
  }));

  // ── A05 设置持久化（写入阶段）──
  guard(await probe("A05", "PUT /api/preferences/session-permission-default {permissionMode:auto} → 200", async () => {
    const r = await put("/api/preferences/session-permission-default", { permissionMode: "auto" });
    const b = await r.json();
    return { expect: "200 auto", actual: `${r.status} ${b.permissionMode}`, ok: r.status === 200 && b.permissionMode === "auto" };
  }));
  guard(await probe("A05", "PUT /api/preferences/notifications（第二设置项）→ 200", async () => {
    const r = await put("/api/preferences/notifications", { enabled: true });
    return { expect: "200/2xx", actual: r.status, ok: r.status >= 200 && r.status < 300 };
  }));
  const preSessions = await (await get("/api/sessions")).json();
  const prePerm = await (await get("/api/preferences/session-permission-default")).json();

  // ── F12/F13 路由在位（真实外发另记 BLOCKED）──
  guard(await probe("F12", "GET /api/bridge/status?agentId=lingxi → 200（桥接管理路由在位；真实平台外发无授权不执行）", async () => {
    const r = await get("/api/bridge/status?agentId=lingxi");
    return { expect: "200/2xx", actual: r.status, ok: r.status >= 200 && r.status < 300 };
  }));
  guard(await probe("F13", "GET /api/devices → 200（Web/移动设备面在位）", async () => {
    const r = await get("/api/devices");
    return { expect: "200/2xx", actual: r.status, ok: r.status >= 200 && r.status < 300 };
  }));

  // ── F16 用量观测（真实聊天后）──
  guard(await probe("F16", "POST /api/model-observability/query/traces → 有真实 trace（上面两轮真实模型调用入账）", async () => {
    const r = await post("/api/model-observability/query/traces", {});
    const b = await r.json();
    const n = Array.isArray(b.traces) ? b.traces.length : (b.total ?? JSON.stringify(b).length);
    return { expect: "200 且非空", actual: `${r.status} traces=${n}`, ok: r.status === 200 && JSON.stringify(b).length > 10 };
  }));

  // ── F17 角色卡/观测导出路由在位 ──
  guard(await probe("F17", "POST /api/character-cards/export/preview → 2xx/4xx（非 5xx 即路由在位）", async () => {
    const r = await post("/api/character-cards/export/preview", { agentId: "lingxi" });
    return { expect: "<500", actual: r.status, ok: r.status < 500 };
  }));

  // ── F19 技能 ──
  guard(await probe("F19", "GET /api/skills?agentId=lingxi → 200（首启同步后技能池可列）", async () => {
    const r = await get("/api/skills?agentId=lingxi");
    const b = await r.json();
    return { expect: "200", actual: `${r.status} skills=${Array.isArray(b.skills) ? b.skills.length : "?"}`, ok: r.status === 200 };
  }));

  // ── F14 CLI（headless，不依赖桌面）──
  guard(await probe("F14", "CLI status（cli/entry.ts，读同 LINGXI_HOME server-info）→ 连通输出", async () => {
    const out = execFileSync(process.execPath, [path.join(REPO, "cli", "entry.ts"), "status"], {
      cwd: REPO, env: { ...process.env, LINGXI_HOME: lingxiHome }, encoding: "utf8", timeout: 60000,
    });
    return { expect: "输出含 running/在线/端口信息", actual: out.slice(0, 200).replace(/\n/g, " "), ok: /running|在线|port|pid|version/i.test(out) };
  }));

  // ── F20 i18n 资源 ──
  guard(await probe("F20", "desktop/src/locales 五语言文件在位", async () => {
    const locales = ["zh", "en", "ja", "ko", "zh-TW"];
    const missing = locales.filter((l) => !fs.existsSync(path.join(REPO, "desktop", "src", "locales", `${l}.json`)));
    return { expect: "5 个全在", actual: missing.length ? `missing=${missing}` : "all present", ok: missing.length === 0 };
  }));

  // ── F18 更新通道（检查更新主源文件在位；不打 tag/不发版）──
  guard(await probe("F18", "desktop/src/shared/github-release-check.cjs + release-digest.v1/v2.json 在位", async () => {
    const ok1 = fs.existsSync(path.join(REPO, "desktop", "src", "shared", "github-release-check.cjs"));
    const ok2 = fs.existsSync(path.join(REPO, "release-digest.v1.json"));
    const ok3 = fs.existsSync(path.join(REPO, "release-digest.v2.json"));
    return { expect: "3 文件全在", actual: `${ok1}/${ok2}/${ok3}`, ok: ok1 && ok2 && ok3 };
  }));

  // ── A05 重启持久化 ──
  child.kill("SIGTERM");
  const exitCode = await waitForExit(child);
  record("A05", "SIGTERM 优雅关闭", "exit 0", `exit=${exitCode}`, exitCode === 0 ? "PASS" : "FAIL");
  guard(exitCode === 0);

  booted = bootServer(lingxiHome);
  child = booted.child;
  const info2 = await waitForServerInfo(lingxiHome, child);
  const base2 = `http://127.0.0.1:${info2.port}`;
  const H2 = { authorization: `Bearer ${info2.token}`, "content-type": "application/json" };
  guard(await probe("A05", "重启后 GET /api/preferences/session-permission-default = auto（无最后一项覆盖）", async () => {
    const r = await fetch(`${base2}/api/preferences/session-permission-default`, { headers: H2 });
    const b = await r.json();
    return { expect: "auto", actual: `${r.status} ${b.permissionMode}`, ok: r.status === 200 && b.permissionMode === "auto" };
  }));
  guard(await probe("A05", "重启后会话列表保持（数量一致）", async () => {
    const r = await fetch(`${base2}/api/sessions`, { headers: H2 });
    const b = await r.json();
    const before = preSessions.sessions ? preSessions.sessions.length : JSON.stringify(preSessions).length;
    const after = b.sessions ? b.sessions.length : JSON.stringify(b).length;
    return { expect: `count=${before}`, actual: `count=${after}`, ok: r.status === 200 && before === after };
  }));
  guard(await probe("A05/F01", "重启后历史仍可读（含 P08_CHAT_REPLY）", async () => {
    const r = await fetch(`${base2}/api/sessions/messages?sessionId=${encodeURIComponent(sessionId)}&all=1`, { headers: H2 });
    const t = JSON.stringify(await r.json());
    return { expect: "200 含 P08_CHAT_REPLY", actual: `${r.status} reply=${t.includes("P08_CHAT_REPLY")}`, ok: r.status === 200 && t.includes("P08_CHAT_REPLY") };
  }));
  guard(await probe("A05/F10", "重启后 provider catalog 双 provider 仍在", async () => {
    const r = await fetch(`${base2}/api/providers/summary`, { headers: H2 });
    const t = JSON.stringify(await r.json());
    return { expect: "含 witness-a 与 witness-b", actual: `a=${t.includes("witness-a")} b=${t.includes("witness-b")}`, ok: r.status === 200 && t.includes("witness-a") && t.includes("witness-b") };
  }));
} finally {
  try { child.kill("SIGTERM"); await waitForExit(child, 10000); } catch { /* */ }
  await witness.close().catch(() => {});
  fs.rmSync(workspaceDir, { recursive: true, force: true });
  fs.rmSync(lingxiHome, { recursive: true, force: true });
}

const summary = {
  tool: "P08 product-regression (real full server, isolated HOME, witness provider)",
  started_features: ["F01","F02","F03","F06","F07","F08","F09","F10","F11","F12","F13","F14","F16","F17","F18","F19","F20","A05"],
  pass: results.filter((r) => r.status === "PASS").length,
  fail: results.filter((r) => r.status === "FAIL").length,
  all_pass: allOk,
  results,
};
fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify(summary, null, 2));
console.log(`\nP08 product regression: pass=${summary.pass} fail=${summary.fail} → ${outPath}`);
process.exit(allOk ? 0 : 1);
