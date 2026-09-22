#!/usr/bin/env node
/**
 * ACC counter-example 1 (P08 independent acceptance): session JSONL mid-file corruption.
 * Variant beyond the round's drill (which truncated 120 bytes off the TAIL of the NEWEST session):
 *  (a) garbage bytes stamped in the MIDDLE of a session JSONL (torn write in the middle, not tail)
 *  (b) truncation of the OLDEST session to a mid-line offset
 * Expectations (current HEAD, dev-tree entry):
 *  - server reaches ready (server-info.json) — no half-activation, no crash loop
 *  - the OTHER sessions remain listed and their history readable (no silent wipe)
 *  - no permission/config side effects (permissionMode preserved)
 * All dirs under /tmp; witness provider local; cleanup on exit.
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import http from "node:http";
import process from "node:process";
import console from "node:console";
const { fetch, setTimeout, WebSocket } = globalThis;

const REPO = "/Users/study_superior/Desktop/Code/LingxiAgent";
const sleep = (ms) => new Promise((r) => globalThis.setTimeout(r, ms));
const FILE_MARKER = "ACC_CE1_MARKER_a51f";
const results = [];
function record(id, desc, ok, detail) { results.push({ id, desc, ok, detail }); console.log(`[${ok ? "PASS" : "UNEXPECTED"}] ${id}: ${desc} → ${detail}`); }

class Witness {
  constructor() {
    this.server = http.createServer((req, res) => {
      const chunks = []; req.on("data", (c) => chunks.push(c));
      req.on("end", () => {
        let b = null; try { b = JSON.parse(Buffer.concat(chunks).toString("utf8")); } catch { /* */ }
        const hist = JSON.stringify(b?.messages ?? []);
        let body;
        if (!hist.includes(FILE_MARKER)) {
          const chunk = { id: "cc-acc1", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { role: "assistant", tool_calls: [{ index: 0, id: "tc_acc1", type: "function", function: { name: "read", arguments: JSON.stringify({ path: "ce1-note.txt" }) } }] }, finish_reason: null }] };
          const done = { id: "cc-acc1", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }] };
          body = [`data: ${JSON.stringify(chunk)}`, `data: ${JSON.stringify(done)}`, "data: [DONE]", ""].join("\n\n");
        } else {
          body = [`data: ${JSON.stringify({ id: "cc-acc1", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { role: "assistant", content: "ACC_CE1_REPLY 已读" }, finish_reason: null }] })}`, `data: ${JSON.stringify({ id: "cc-acc1", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "stop" }] })}`, "data: [DONE]", ""].join("\n\n");
        }
        res.writeHead(200, { "content-type": "text/event-stream" });
        res.end(body);
      });
    });
    this.ready = new Promise((res, rej) => { this.server.once("error", rej); this.server.listen(0, "127.0.0.1", () => res()); });
  }
  get baseUrl() { return `http://127.0.0.1:${this.server.address().port}`; }
  close() { return new Promise((r) => this.server.close(() => r())); }
}

const witness = new Witness();
await witness.ready;
const home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-acc-ce1-"));
const ws = fs.mkdtempSync(path.join(os.tmpdir(), "hana-acc-ce1-ws-"));
fs.writeFileSync(path.join(ws, "ce1-note.txt"), `${FILE_MARKER}\n`, "utf8");
const agentDir = path.join(home, "agents", "lingxi");
fs.mkdirSync(path.join(agentDir, "sessions"), { recursive: true });
const template = fs.readFileSync(path.join(REPO, "lib", "config.example.yaml"), "utf8");
fs.writeFileSync(path.join(agentDir, "config.yaml"), template.replace(/^(\s*chat:\s*)".*"/m, "$1{id: witness-model, provider: witness}"), "utf8");
fs.writeFileSync(path.join(home, "provider-catalog.json"), JSON.stringify({ catalogVersion: 2, providers: { witness: { base_url: `${witness.baseUrl}/v1`, api: "openai-completions", api_key: "sk-ACC-CE1", models: ["witness-model"] } } }, null, 2), "utf8");

function boot() {
  const child = spawn(process.execPath, [path.join(REPO, "server", "bootstrap.ts")], {
    cwd: REPO, env: { ...process.env, LINGXI_HOME: home, LINGXI_PORT: "0", LINGXI_ROOT: REPO, LINGXI_SERVER_ENTRY: path.join(REPO, "server", "main-full.ts"), LINGXI_CREATE_STARTUP_SESSION: "0" }, stdio: ["ignore", "pipe", "pipe"],
  });
  let tail = "";
  child.stdout.on("data", (c) => { tail = (tail + String(c)).slice(-2000); });
  child.stderr.on("data", (c) => { tail = (tail + String(c)).slice(-2000); });
  return { child, tail: () => tail };
}
async function waitInfo(child, tail) {
  const p = path.join(home, "server-info.json");
  const deadline = Date.now() + 90000;
  for (;;) {
    try { return JSON.parse(fs.readFileSync(p, "utf8")); } catch { /* */ }
    if (child.exitCode !== null) throw new Error(`exited early code=${child.exitCode} tail=${tail().slice(-300)}`);
    if (Date.now() > deadline) throw new Error("timeout server-info");
    await sleep(150);
  }
}
async function api(base, token, method, p, body) {
  const r = await fetch(`${base}${p}`, { method: method || "GET", headers: { authorization: `Bearer ${token}`, "content-type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
  let json = null; try { json = await r.json(); } catch { /* */ }
  return { status: r.status, json };
}
async function chatRound(base, token, port, sessionId, sessionPath) {
  const events = [];
  const w = new WebSocket(`ws://127.0.0.1:${port}/ws?token=${encodeURIComponent(token)}`);
  await new Promise((res, rej) => { const t = setTimeout(() => rej(new Error("ws timeout")), 15000); w.addEventListener("open", () => { clearTimeout(t); res(); }); w.addEventListener("error", (e) => { clearTimeout(t); rej(e); }); });
  w.addEventListener("message", (m) => { try { events.push(JSON.parse(String(m.data))); } catch { /* */ } });
  w.send(JSON.stringify({ type: "prompt", clientMessageId: "acc-ce1-c1", snapshotVersion: 1, text: "请读取 ce1-note.txt 并告诉我内容", sessionId, sessionPath }));
  const dl = Date.now() + 90000;
  while (!events.some((e) => e.type === "assistant_run_end" && e.status === "completed")) { if (Date.now() > dl) { w.close(); throw new Error("settle timeout"); } await sleep(150); }
  w.close();
}
function sessionIds(json) { return (json.sessions || json || []).map((s) => s.sessionId || s.id || s.path).filter(Boolean).sort(); }

let run, info, base;
try {
  /* Phase 1: create 3 sessions, each with a real chat round */
  run = boot();
  info = await waitInfo(run.child, run.tail);
  base = `http://127.0.0.1:${info.port}`;
  const mk = async () => (await api(base, info.token, "POST", "/api/sessions/new", { cwd: ws })).json;
  const A = await mk(), B = await mk(), C = await mk();
  await chatRound(base, info.token, info.port, A.sessionId, A.path);
  await chatRound(base, info.token, info.port, B.sessionId, B.path);
  await chatRound(base, info.token, info.port, C.sessionId, C.path);
  await api(base, info.token, "PUT", "/api/preferences/session-permission-default", { permissionMode: "operate" });
  const beforeIds = sessionIds((await api(base, info.token, "GET", "/api/sessions")).json);
  run.child.kill("SIGTERM");
  await new Promise((r) => { const t = setTimeout(() => { try { run.child.kill("SIGKILL"); } catch { /* */ } }, 20000); run.child.on("exit", () => { clearTimeout(t); r(); }); });
  record("CE-1-setup", "3 会话各 1 轮真实聊天 + operate 设置（基线建立）", beforeIds.length === 3, `sessions=${beforeIds.length}`);

  /* Phase 2: corrupt */
  const sessDir = path.join(home, "agents", "lingxi", "sessions");
  const jsonls = fs.readdirSync(sessDir).filter((f) => f.endsWith(".jsonl")).map((f) => ({ f, mtime: fs.statSync(path.join(sessDir, f)).mtimeMs })).sort((a, b) => a.mtime - b.mtime);
  const oldest = path.join(sessDir, jsonls[0].f);   // A (first chat)
  const middle = path.join(sessDir, jsonls[1].f);   // B
  // (a) garbage in the MIDDLE of B's JSONL
  const bufB = fs.readFileSync(middle);
  const mid = Math.floor(bufB.length / 2);
  for (let i = 0; i < 64 && mid + i < bufB.length; i++) bufB[mid + i] = 0xef; // non-UTF8 garbage bytes
  fs.writeFileSync(middle, bufB);
  // (b) truncate OLDEST to a mid-line offset (drop tail half)
  const bufA = fs.readFileSync(oldest);
  const cut = Math.floor(bufA.length / 3);
  fs.truncateSync(oldest, cut);

  /* Phase 3: boot again on same HOME */
  run = boot();
  info = await waitInfo(run.child, run.tail);
  base = `http://127.0.0.1:${info.port}`;
  const health = await api(base, info.token, "GET", "/api/health");
  record("CE-1a", "中部垃圾字节+最旧会话半行截断后再启动：就绪且 health 200（无半激活/崩溃循环）", health.status === 200, `health=${health.status}`);
  const afterIds = sessionIds((await api(base, info.token, "GET", "/api/sessions")).json);
  record("CE-1b", "会话清单无静默清空（3 会话仍在）", afterIds.length === 3, `sessions=${afterIds.length}`);
  const histC = await api(base, info.token, "GET", `/api/sessions/messages?sessionId=${encodeURIComponent(C.sessionId)}&all=1`);
  const tC = JSON.stringify(histC.json);
  record("CE-1c", "未受损会话 C 历史完整可读（含真实回复）", histC.status === 200 && tC.includes("ACC_CE1_REPLY"), `status=${histC.status} reply=${tC.includes("ACC_CE1_REPLY")}`);
  const perm = (await api(base, info.token, "GET", "/api/preferences/session-permission-default")).json;
  record("CE-1d", "设置保持（permissionMode=operate，损坏未波及配置）", perm.permissionMode === "operate", `mode=${perm.permissionMode}`);
  const obs = await api(base, info.token, "POST", "/api/model-observability/query/traces", {});
  record("CE-1e", "观测记录仍可查（非空）", obs.status === 200 && JSON.stringify(obs.json).length > 10, `status=${obs.status}`);
  run.child.kill("SIGTERM");
  await new Promise((r) => { const t = setTimeout(() => { try { run.child.kill("SIGKILL"); } catch { /* */ } }, 20000); run.child.on("exit", () => { clearTimeout(t); r(); }); });
} finally {
  try { if (run && run.child.exitCode === null) run.child.kill("SIGKILL"); } catch { /* */ }
  await witness.close().catch(() => {});
  fs.rmSync(ws, { recursive: true, force: true });
  fs.rmSync(home, { recursive: true, force: true });
}
const okCount = results.filter((r) => r.ok).length;
fs.writeFileSync("/tmp/acc-ce1-results.json", JSON.stringify({ tool: "acc-ce1-jsonl-corrupt", all_ok: okCount === results.length, results }, null, 2));
console.log(`\nCE-1 summary: ${okCount}/${results.length} behaved as expected`);
process.exit(okCount === results.length ? 0 : 1);
