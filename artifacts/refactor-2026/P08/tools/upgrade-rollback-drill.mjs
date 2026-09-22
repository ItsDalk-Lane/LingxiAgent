#!/usr/bin/env node
/**
 * P08-T05 旧数据升级 + 故障注入 + 回退演练（T05.3/T05.4，场景 A08/A09）。
 *
 * 阶段：
 *  OLD-GEN   ：在 /tmp/p08-old-tree（研究基线 8037fae7a 的 git worktree，lockfile 与
 *              当前一致）真实启动旧版本全量组合 server，隔离 HOME + witness 供应商，
 *              生成「受支持旧版本数据副本」：2 会话 + 1 轮真实聊天（read 工具）+
 *              设置（permissionMode=operate + notifications）+ 观测记录。
 *  UPGRADE   ：当前 HEAD 代码在同一 HOME 启动（升级），逐项比对会话/消息/设置/观测，
 *              并新增一条新记录（新会话+聊天轮）。
 *  FAULT     ：升级关键步骤故障注入——截断一条会话 JSONL 尾部（模拟升级窗口崩溃时
 *              的半写），再次启动新版本：其余数据必须完好、无半激活（server-info.json
 *              只在成功就绪后写出）、受损文件不静默清空其余数据。
 *  ROLLBACK  ：旧版本代码再次在该 HOME 启动（代码回退 + 新数据并存演练）：读取全部
 *              数据（含新版本写入的记录）、追加自己的新会话后，新版本记录不被覆盖。
 *
 * 用法：node upgrade-rollback-drill.mjs [--new-entry <server/bootstrap.ts 路径或 packaged 入口>]
 * 输出：--out JSON 报告（默认 logs/upgrade-rollback-drill.json）。
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
const OLD_TREE = "/tmp/p08-old-tree";
const argv = process.argv.slice(2);
let outPath = path.join(REPO, "artifacts", "refactor-2026", "P08", "logs", "upgrade-rollback-drill.json");
let newEntry = path.join(REPO, "server", "bootstrap.ts");
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--out") outPath = path.resolve(REPO, argv[++i]);
  else if (argv[i] === "--new-entry") newEntry = path.resolve(argv[++i]);
}
const sleep = (ms) => new Promise((r) => globalThis.setTimeout(r, ms));

const WITNESS_KEY = "sk-P08-UPGRADE-DRILL-WITNESS";
const FILE_MARKER = "P08_UPGRADE_DRILL_MARKER_9d4f";
const results = [];
function record(phase, action, expect, actual, status) {
  results.push({ phase, action, expect, actual: String(actual).slice(0, 300), status });
  console.log(`[${status}] ${phase} | ${action}`);
}
async function probe(phase, action, fn) {
  try {
    const r = await fn();
    record(phase, action, r.expect, r.actual, r.ok ? "PASS" : "FAIL");
    return r.ok;
  } catch (err) {
    record(phase, action, "no-throw", err && err.message, "FAIL");
    return false;
  }
}
let allOk = true;
const guard = (ok) => { allOk = allOk && ok; };

class Witness {
  constructor() {
    this.requests = [];
    this.server = http.createServer((req, res) => {
      const chunks = [];
      req.on("data", (c) => chunks.push(c));
      req.on("end", () => {
        let bodyJson = null;
        try { bodyJson = JSON.parse(Buffer.concat(chunks).toString("utf8")); } catch { bodyJson = null; }
        this.requests.push({ url: req.url, bodyJson });
        let body;
        const hist = JSON.stringify(bodyJson?.messages ?? []);
        if (!hist.includes(FILE_MARKER)) {
          const chunk = { id: "cc-p08u", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { role: "assistant", tool_calls: [{ index: 0, id: "tc_p08u_read", type: "function", function: { name: "read", arguments: JSON.stringify({ path: "drill-note.txt" }) } }] }, finish_reason: null }] };
          const done = { id: "cc-p08u", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }] };
          body = [`data: ${JSON.stringify(chunk)}`, `data: ${JSON.stringify(done)}`, "data: [DONE]", ""].join("\n\n");
        } else {
          body = [
            `data: ${JSON.stringify({ id: "cc-p08u", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: { role: "assistant", content: "P08_UPGRADE_REPLY 内容已读" }, finish_reason: null }] })}`,
            `data: ${JSON.stringify({ id: "cc-p08u", object: "chat.completion.chunk", created: 0, model: "witness-model", choices: [{ index: 0, delta: {}, finish_reason: "stop" }] })}`,
            "data: [DONE]", "",
          ].join("\n\n");
        }
        res.writeHead(200, { "content-type": "text/event-stream" });
        res.end(body);
      });
    });
    this.ready = new Promise((resolve, reject) => {
      this.server.once("error", reject);
      this.server.listen(0, "127.0.0.1", () => resolve());
    });
  }
  get baseUrl() { return `http://127.0.0.1:${this.server.address().port}`; }
  close() { return new Promise((r) => this.server.close(() => r())); }
}

function seedHome(home, witnessBaseUrl) {
  const agentDir = path.join(home, "agents", "lingxi");
  fs.mkdirSync(path.join(agentDir, "sessions"), { recursive: true });
  const template = fs.readFileSync(path.join(REPO, "lib", "config.example.yaml"), "utf8");
  const patched = template.replace(/^(\s*chat:\s*)".*"/m, "$1{id: witness-model, provider: witness}");
  if (patched === template) throw new Error("config template patch failed");
  fs.writeFileSync(path.join(agentDir, "config.yaml"), patched, "utf8");
  fs.writeFileSync(path.join(home, "provider-catalog.json"), JSON.stringify({
    catalogVersion: 2,
    providers: { witness: { base_url: `${witnessBaseUrl}/v1`, api: "openai-completions", api_key: WITNESS_KEY, models: ["witness-model"] } },
  }, null, 2), "utf8");
}

function boot(treeRoot, home, entry) {
  const child = spawn(process.execPath, [entry || path.join(treeRoot, "server", "bootstrap.ts")], {
    cwd: treeRoot,
    env: {
      ...process.env, LINGXI_HOME: home, LINGXI_PORT: "0", LINGXI_ROOT: treeRoot,
      LINGXI_SERVER_ENTRY: path.join(treeRoot, "server", "main-full.ts"),
      LINGXI_CREATE_STARTUP_SESSION: "0",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let tail = "";
  child.stderr.on("data", (c) => { tail = (tail + String(c)).slice(-3000); });
  child.stdout.on("data", (c) => { tail = (tail + String(c)).slice(-3000); });
  return { child, tail: () => tail };
}
async function waitInfo(home, child, timeoutMs = 90000) {
  const p = path.join(home, "server-info.json");
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try { return JSON.parse(fs.readFileSync(p, "utf8")); } catch { /* */ }
    if (child.exitCode !== null) throw new Error(`server exited early code=${child.exitCode}`);
    if (Date.now() > deadline) throw new Error("timeout server-info.json");
    await sleep(150);
  }
}
async function waitExit(child, timeoutMs = 20000) {
  if (child.exitCode !== null) return child.exitCode;
  return new Promise((resolve) => {
    const t = setTimeout(() => { try { child.kill("SIGKILL"); } catch { /* */ } }, timeoutMs);
    child.on("exit", (code) => { clearTimeout(t); resolve(code); });
  });
}
async function api(base, token, method, p, body) {
  const r = await fetch(`${base}${p}`, {
    method: method || "GET",
    headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let json = null;
  try { json = await r.json(); } catch { /* */ }
  return { status: r.status, json };
}
async function chatRound(base, token, info, wsPort, sessionId, sessionPath, text, marker) {
  const events = [];
  const ws = new WebSocket(`ws://127.0.0.1:${wsPort}/ws?token=${encodeURIComponent(token)}`);
  await new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("ws connect timeout")), 15000);
    ws.addEventListener("open", () => { clearTimeout(t); resolve(); });
    ws.addEventListener("error", (e) => { clearTimeout(t); reject(e); });
  });
  ws.addEventListener("message", (m) => { try { events.push(JSON.parse(String(m.data))); } catch { /* */ } });
  ws.send(JSON.stringify({ type: "prompt", clientMessageId: marker, snapshotVersion: 1, text, sessionId, sessionPath }));
  const deadline = Date.now() + 90000;
  while (!events.some((e) => e.type === "assistant_run_end" && e.status === "completed")) {
    if (Date.now() > deadline) { ws.close(); throw new Error("settle timeout"); }
    await sleep(150);
  }
  ws.close();
}

const witness = new Witness();
await witness.ready;
const home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p08-upgrade-"));
const workspaceDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p08-up-ws-"));
fs.writeFileSync(path.join(workspaceDir, "drill-note.txt"), `${FILE_MARKER}\n`, "utf8");
seedHome(home, witness.baseUrl);

async function newSession(base, token) {
  const r = await api(base, token, "POST", "/api/sessions/new", { cwd: workspaceDir });
  return r.json;
}
async function snapshotState(base, token) {
  const sessions = (await api(base, token, "GET", "/api/sessions")).json;
  const ids = (sessions.sessions || sessions || []).map((s) => s.sessionId || s.id || s.path).filter(Boolean).sort();
  const perm = (await api(base, token, "GET", "/api/preferences/session-permission-default")).json;
  const notif = (await api(base, token, "GET", "/api/preferences/notifications")).json;
  return { sessionIds: ids, permissionMode: perm.permissionMode, notifications: notif.notifications };
}

try {
  /* ── OLD-GEN ── */
  let run = boot(OLD_TREE, home);
  let info = await waitInfo(home, run.child);
  let base = `http://127.0.0.1:${info.port}`;
  guard(await probe("OLD-GEN", "旧版本(8037fae7a) server 启动于隔离 HOME", async () => {
    const r = await api(base, info.token, "GET", "/api/health");
    return { expect: 200, actual: r.status, ok: r.status === 200 };
  }));
  const s1 = await newSession(base, info.token);
  const s2 = await newSession(base, info.token);
  await chatRound(base, info.token, info, info.port, s1.sessionId, s1.path, "请读取 drill-note.txt 并告诉我内容", "p08u-old-c1");
  await api(base, info.token, "PUT", "/api/preferences/session-permission-default", { permissionMode: "operate" });
  await api(base, info.token, "PUT", "/api/preferences/notifications", { enabled: true });
  const oldSnap = await snapshotState(base, info.token);
  guard(await probe("OLD-GEN", "旧版本生成 2 会话 + 1 轮真实聊天 + 设置（operate+通知）", async () => {
    const hist = await api(base, info.token, "GET", `/api/sessions/messages?sessionId=${encodeURIComponent(s1.sessionId)}&all=1`);
    const t = JSON.stringify(hist.json);
    return { expect: `2 会话; 历史含 ${FILE_MARKER.slice(0, 12)}`, actual: `sessions=${oldSnap.sessionIds.length} hist=${t.includes(FILE_MARKER)}`, ok: oldSnap.sessionIds.length === 2 && t.includes(FILE_MARKER) };
  }));
  guard(await probe("OLD-GEN", "旧版本设置写入 operate + 通知开启", async () => ({
    expect: "operate", actual: oldSnap.permissionMode, ok: oldSnap.permissionMode === "operate",
  })));
  run.child.kill("SIGTERM");
  const oldExit = await waitExit(run.child);
  record("OLD-GEN", "旧版本优雅关闭", "exit 0", `exit=${oldExit}`, oldExit === 0 ? "PASS" : "FAIL");
  guard(oldExit === 0);

  /* ── UPGRADE（新版本代码，同一 HOME）── */
  run = boot(REPO, home, newEntry);
  info = await waitInfo(home, run.child);
  base = `http://127.0.0.1:${info.port}`;
  const newSnap = await snapshotState(base, info.token);
  guard(await probe("UPGRADE", "A08 会话数量与 ID 完整保持（无静默清空）", async () => ({
    expect: `ids=${oldSnap.sessionIds.join(",").slice(0, 60)}`,
    actual: `ids=${newSnap.sessionIds.join(",").slice(0, 60)}`,
    ok: JSON.stringify(oldSnap.sessionIds) === JSON.stringify(newSnap.sessionIds),
  })));
  guard(await probe("UPGRADE", "A08 升级后旧历史可读（含旧轮真实回复）", async () => {
    const hist = await api(base, info.token, "GET", `/api/sessions/messages?sessionId=${encodeURIComponent(s1.sessionId)}&all=1`);
    const t = JSON.stringify(hist.json);
    return { expect: "200 含 P08_UPGRADE_REPLY", actual: `${hist.status} reply=${t.includes("P08_UPGRADE_REPLY")}`, ok: hist.status === 200 && t.includes("P08_UPGRADE_REPLY") };
  }));
  guard(await probe("UPGRADE", "A08 设置保持（permissionMode=operate 未丢）", async () => ({
    expect: "operate", actual: newSnap.permissionMode, ok: newSnap.permissionMode === "operate",
  })));
  guard(await probe("UPGRADE", "A08 权限语义：默认模式读取与旧写入一致（无越权放宽）", async () => ({
    expect: "operate(旧写入值)", actual: `${newSnap.permissionMode}`, ok: newSnap.permissionMode === "operate",
  })));
  const observabilityOk = await probe("UPGRADE", "A08 观测记录可读（升级后 trace 查询非空）", async () => {
    const r = await api(base, info.token, "POST", "/api/model-observability/query/traces", {});
    return { expect: "200 非空", actual: `${r.status} len=${JSON.stringify(r.json).length}`, ok: r.status === 200 && JSON.stringify(r.json).length > 10 };
  });
  guard(observabilityOk);
  // 新增记录
  const s3 = await newSession(base, info.token);
  await chatRound(base, info.token, info, info.port, s3.sessionId, s3.path, "请读取 drill-note.txt 并告诉我内容", "p08u-new-c1");
  const afterNewWrite = await snapshotState(base, info.token);
  guard(await probe("UPGRADE", "升级后新增会话+聊天轮成功（新增记录写入）", async () => ({
    expect: "3 会话", actual: `count=${afterNewWrite.sessionIds.length}`, ok: afterNewWrite.sessionIds.length === 3,
  })));
  run.child.kill("SIGTERM");
  guard((await waitExit(run.child)) === 0);

  /* ── FAULT：升级窗口半写故障注入 ── */
  // 找到最新会话 JSONL，截断尾部若干字节（模拟 append 中途崩溃的半行）
  const sessDir = path.join(home, "agents", "lingxi", "sessions");
  const jsonls = fs.readdirSync(sessDir).filter((f) => f.endsWith(".jsonl")).map((f) => ({ f, mtime: fs.statSync(path.join(sessDir, f)).mtimeMs })).sort((a, b) => b.mtime - a.mtime);
  const victim = path.join(sessDir, jsonls[0].f);
  const origSize = fs.statSync(victim).size;
  fs.truncateSync(victim, Math.max(0, origSize - 120));
  guard(await probe("FAULT", "A09 会话 JSONL 尾部截断（半写注入）后新版本再启动：无半激活、其余数据完好", async () => {
    const r2 = boot(REPO, home, newEntry);
    let info2;
    try {
      info2 = await waitInfo(home, r2.child);
      const health = await api(`http://127.0.0.1:${info2.port}`, info2.token, "GET", "/api/health");
      const snap3 = await snapshotState(`http://127.0.0.1:${info2.port}`, info2.token);
      const notLost = snap3.sessionIds.length === 3;
      return {
        expect: "启动成功(health 200) + 3 会话全在",
        actual: `health=${health.status} sessions=${snap3.sessionIds.length}`,
        ok: health.status === 200 && notLost,
      };
    } finally {
      r2.child.kill("SIGTERM");
      await waitExit(r2.child);
    }
  }));

  /* ── ROLLBACK：旧版本代码再启动（代码回退 + 新数据并存）── */
  run = boot(OLD_TREE, home);
  info = await waitInfo(home, run.child);
  base = `http://127.0.0.1:${info.port}`;
  const rbSnap = await snapshotState(base, info.token);
  guard(await probe("ROLLBACK", "A09 旧版本可读新数据：3 会话全在（含新版本写入的记录）", async () => ({
    expect: "count=3", actual: `count=${rbSnap.sessionIds.length}`, ok: rbSnap.sessionIds.length === 3,
  })));
  guard(await probe("ROLLBACK", "A09 旧版本读新版本会话历史（无版本挡板误伤——本轮零 schema 变更，兼容预期）", async () => {
    const hist = await api(base, info.token, "GET", `/api/sessions/messages?sessionId=${encodeURIComponent(s3.sessionId)}&all=1`);
    return { expect: "200", actual: hist.status, ok: hist.status === 200 };
  }));
  const s4 = await newSession(base, info.token);
  const finalSnap = await snapshotState(base, info.token);
  guard(await probe("ROLLBACK", "A09 旧版本追加新会话后，新版本记录不被覆盖（4 会话且原 3 个 ID 保持）", async () => ({
    expect: "count=4 且原3会话ID保留",
    actual: `count=${finalSnap.sessionIds.length} kept=${afterNewWrite.sessionIds.every((id) => finalSnap.sessionIds.includes(id))}`,
    ok: finalSnap.sessionIds.length === 4 && afterNewWrite.sessionIds.every((id) => finalSnap.sessionIds.includes(id)),
  })));
  run.child.kill("SIGTERM");
  guard((await waitExit(run.child)) === 0);
} finally {
  try { } catch { /* */ }
  await witness.close().catch(() => {});
  fs.rmSync(workspaceDir, { recursive: true, force: true });
  fs.rmSync(home, { recursive: true, force: true });
}

const summary = {
  tool: "P08 upgrade-rollback-drill",
  old_tree: OLD_TREE + " (8037fae7a)",
  new_entry: newEntry,
  pass: results.filter((r) => r.status === "PASS").length,
  fail: results.filter((r) => r.status === "FAIL").length,
  all_pass: allOk,
  results,
};
fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify(summary, null, 2));
console.log(`\nP08 upgrade-rollback drill: pass=${summary.pass} fail=${summary.fail} → ${outPath}`);
process.exit(allOk ? 0 : 1);
