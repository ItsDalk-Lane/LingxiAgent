#!/usr/bin/env node
/**
 * P08-T05 打包产物形态冒烟（A07 干净安装·非开发模式）：
 * 从 dist/mac-arm64/Lingxi.app/Contents/Resources/seed/ 提取的 server-*.tar.gz
 * 种子树（自带 node v24.15.0 + bundle + node_modules，与开发树零耦合），用其
 * hana-server 启动脚本在隔离 HOME 真实启动，走真实业务入口：
 * health → sessions/new → WS prompt（read 工具真实往返）→ 历史回读 → 优雅退出。
 * 用法：node packaged-server-smoke.mjs --seed <seed-dir> --out <json>
 */
import { spawn } from "node:child_process";
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
const argv = process.argv.slice(2);
let seedDir = "/tmp/p08-seed-extract";
let outPath = path.join(REPO, "artifacts", "refactor-2026", "P08", "logs", "packaged-server-smoke.json");
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--seed") seedDir = argv[++i];
  else if (argv[i] === "--out") outPath = path.resolve(REPO, argv[++i]);
}
const sleep = (ms) => new Promise((r) => globalThis.setTimeout(r, ms));
const WITNESS_KEY = "sk-P08-PACKAGED-SMOKE-WITNESS";
const FILE_MARKER = "P08_PACKAGED_SMOKE_MARKER_3e8a";
const results = [];
function record(action, expect, actual, status) { results.push({ action, expect, actual: String(actual).slice(0, 300), status }); console.log(`[${status}] ${action}`); }
let allOk = true;
const guard = (ok) => { allOk = allOk && ok; };
async function probe(action, fn) {
  try { const r = await fn(); record(action, r.expect, r.actual, r.ok ? "PASS" : "FAIL"); return r.ok; }
  catch (err) { record(action, "no-throw", err && err.message, "FAIL"); return false; }
}

class Witness {
  constructor() {
    this.server = http.createServer((req, res) => {
      const chunks = [];
      req.on("data", (c) => chunks.push(c));
      req.on("end", () => {
        let bodyJson = null;
        try { bodyJson = JSON.parse(Buffer.concat(chunks).toString("utf8")); } catch { bodyJson = null; }
        const hist = JSON.stringify(bodyJson?.messages ?? []);
        let body;
        if (!hist.includes(FILE_MARKER)) {
          const chunk = { id: "cc-p08p", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { role: "assistant", tool_calls: [{ index: 0, id: "tc_p08p", type: "function", function: { name: "read", arguments: JSON.stringify({ path: "packaged-note.txt" }) } }] }, finish_reason: null }] };
          const done = { id: "cc-p08p", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }] };
          body = [`data: ${JSON.stringify(chunk)}`, `data: ${JSON.stringify(done)}`, "data: [DONE]", ""].join("\n\n");
        } else {
          body = [
            `data: ${JSON.stringify({ id: "cc-p08p", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { role: "assistant", content: "P08_PACKAGED_REPLY 内容已读" }, finish_reason: null }] })}`,
            `data: ${JSON.stringify({ id: "cc-p08p", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "stop" }] })}`,
            "data: [DONE]", "",
          ].join("\n\n");
        }
        res.writeHead(200, { "content-type": "text/event-stream" });
        res.end(body);
      });
    });
    this.ready = new Promise((resolve, reject) => { this.server.once("error", reject); this.server.listen(0, "127.0.0.1", () => resolve()); });
  }
  get baseUrl() { return `http://127.0.0.1:${this.server.address().port}`; }
  close() { return new Promise((r) => this.server.close(() => r())); }
}

const witness = new Witness();
await witness.ready;
const home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p08-packaged-"));
const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p08-pkg-ws-"));
fs.writeFileSync(path.join(workspaceDir, "packaged-note.txt"), `${FILE_MARKER}\n`, "utf8");
const agentDir = path.join(home, "agents", "lingxi");
fs.mkdirSync(path.join(agentDir, "sessions"), { recursive: true });
const template = fs.readFileSync(path.join(REPO, "lib", "config.example.yaml"), "utf8");
fs.writeFileSync(path.join(agentDir, "config.yaml"), template.replace(/^(\s*chat:\s*)".*"/m, "$1{id: witness-model, provider: witness}"), "utf8");
fs.writeFileSync(path.join(home, "provider-catalog.json"), JSON.stringify({
  catalogVersion: 2,
  providers: { witness: { base_url: `${witness.baseUrl}/v1`, api: "openai-completions", api_key: WITNESS_KEY, models: ["witness-model"] } },
}, null, 2), "utf8");

let child;
try {
  child = spawn(path.join(seedDir, "hana-server"), { cwd: seedDir, env: { ...process.env, LINGXI_HOME: home, LINGXI_PORT: "0", LINGXI_CREATE_STARTUP_SESSION: "0" }, stdio: ["ignore", "pipe", "pipe"] });
  let tail = "";
  child.stdout.on("data", (c) => { tail += String(c); fs.appendFileSync(outPath+".server.log",c); });
  child.stderr.on("data", (c) => { tail += String(c); fs.appendFileSync(outPath+".server.log",c); });
  const infoPath = path.join(home, "server-info.json");
  const deadline = Date.now() + 90000;
  let info = null;
  while (!info) {
    try { info = JSON.parse(fs.readFileSync(infoPath, "utf8")); } catch { /* */ }
    if (child.exitCode !== null) throw new Error(`packaged server exited early code=${child.exitCode} tail=${tail.slice(-500)}`);
    if (Date.now() > deadline) throw new Error("timeout server-info.json");
    await sleep(150);
  }
  const base = `http://127.0.0.1:${info.port}`;
  const H = { authorization: `Bearer ${info.token}`, "content-type": "application/json" };
  guard(await probe("打包产物形态：seed 提取树（自带 node）真实启动 → server-info.json 就绪", async () => ({
    expect: "pid+token", actual: `pid=${info.pid} port=${info.port}`, ok: !!info.token,
  })));
  guard(await probe("GET /api/health（鉴权 200 / 无凭证 403）", async () => {
    const no = await fetch(`${base}/api/health`);
    const okr = await fetch(`${base}/api/health`, { headers: H });
    return { expect: "403 / 200", actual: `${no.status} / ${okr.status}`, ok: no.status === 403 && okr.status === 200 };
  }));
  guard(await probe("POST /api/sessions/new → sess_", async () => {
    const r = await fetch(`${base}/api/sessions/new`, { method: "POST", headers: H, body: JSON.stringify({ cwd: workspaceDir }) });
    const b = await r.json();
    globalThis.__sess = b;
    return { expect: "200 sess_", actual: `${r.status} ${b.sessionId}`, ok: r.status === 200 && /^sess_/.test(b.sessionId || "") };
  }));
  guard(await probe("WS prompt read 工具真实往返 + 流式回复（打包形态完整聊天链）", async () => {
    const s = globalThis.__sess;
    const events = [];
    const ws = new WebSocket(`ws://127.0.0.1:${info.port}/ws?token=${encodeURIComponent(info.token)}`);
    await new Promise((resolve, reject) => {
      const t = setTimeout(() => reject(new Error("ws timeout")), 15000);
      ws.addEventListener("open", () => { clearTimeout(t); resolve(); });
      ws.addEventListener("error", (e) => { clearTimeout(t); reject(e); });
    });
    ws.addEventListener("message", (m) => { try { events.push(JSON.parse(String(m.data))); } catch { /* */ } });
    ws.send(JSON.stringify({ type: "prompt", clientMessageId: "p08-pkg-c1", snapshotVersion: 1, text: "请读取 packaged-note.txt 并告诉我内容", sessionId: s.sessionId, sessionPath: s.path }));
    const dl = Date.now() + 90000;
    while (!events.some((e) => e.type === "assistant_run_end" && e.status === "completed")) {
      if (Date.now() > dl) { ws.close(); throw new Error("settle timeout"); }
      await sleep(150);
    }
    ws.close();
    const tools = events.filter((e) => String(e.type).startsWith("tool_")).length;
    return { expect: "≥2 tool 事件", actual: `tool_events=${tools}`, ok: tools >= 2 };
  }));
  guard(await probe("历史回读（含 P08_PACKAGED_REPLY）", async () => {
    const s = globalThis.__sess;
    const r = await fetch(`${base}/api/sessions/messages?sessionId=${encodeURIComponent(s.sessionId)}&all=1`, { headers: H });
    const t = JSON.stringify(await r.json());
    return { expect: "200 含回复", actual: `${r.status} reply=${t.includes("P08_PACKAGED_REPLY")}`, ok: r.status === 200 && t.includes("P08_PACKAGED_REPLY") };
  }));
  guard(await probe("GET /api/skills?agentId=lingxi（首启技能同步后可列）", async () => {
    const r = await fetch(`${base}/api/skills?agentId=lingxi`, { headers: H });
    return { expect: 200, actual: r.status, ok: r.status === 200 };
  }));
  child.kill("SIGTERM");
  const code = await new Promise((resolve) => {
    const t = setTimeout(() => { try { child.kill("SIGKILL"); } catch { /* */ } }, 20000);
    child.on("exit", (c) => { clearTimeout(t); resolve(c); });
  });
  record("优雅退出（SIGTERM → exit 0）", "exit 0", `exit=${code}`, code === 0 ? "PASS" : "FAIL");
  guard(code === 0);
} finally {
  try { if (child && child.exitCode === null) { child.kill("SIGKILL"); } } catch { /* */ }
  await witness.close().catch(() => {});
  fs.rmSync(workspaceDir, { recursive: true, force: true });
  fs.rmSync(home, { recursive: true, force: true });
}
const summary = { executedAt: new Date().toISOString(), runtimeNode: fs.readFileSync(path.join(seedDir,"package.json"),"utf8").slice(0,300), tool: "P08 packaged-server-smoke", seed: seedDir, pass: results.filter((r) => r.status === "PASS").length, fail: results.filter((r) => r.status === "FAIL").length, all_pass: allOk, results };
fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify(summary, null, 2));
console.log(`\npackaged smoke: pass=${summary.pass} fail=${summary.fail} → ${outPath}`);
process.exit(allOk ? 0 : 1);
