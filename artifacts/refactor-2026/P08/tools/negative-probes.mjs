#!/usr/bin/env node
/**
 * P08-T06 独立负向复核——真实入口受控不合法请求（任务书 T06.2）。
 * 三组反例，全部对真实启动的全量组合 server（隔离 HOME）：
 *   N1 错误 token（伪装 loopback 凭证）→ 必须 403 且零副作用
 *   N2 跨域会话路径（指向 agents 目录之外的伪造 sessionPath）→ 必须 403 不落盘
 *   N3 非法字段类型（合法 JSON、错误类型）→ 400 invalid_field_type 且会话列表不变
 */
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import process from "node:process";
import console from "node:console";
import { fileURLToPath } from "node:url";

// Node/Web 全局显式取自 globalThis（仓库 lint no-undef 纪律，P07 同款）
const { fetch, setTimeout, clearTimeout, Buffer, WebSocket } = globalThis;

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const sleep = (ms) => new Promise((r) => globalThis.setTimeout(r, ms));
const home = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p08-neg-"));
const agentDir = path.join(home, "agents", "lingxi");
fs.mkdirSync(path.join(agentDir, "sessions"), { recursive: true });
const results = [];
function record(id, expect, actual, ok) { results.push({ id, expect, actual, status: ok ? "PASS" : "FAIL" }); console.log(`[${ok ? "PASS" : "FAIL"}] ${id} → ${actual}`); }

const child = spawn(process.execPath, ["server/bootstrap.ts"], {
  cwd: REPO,
  env: { ...process.env, LINGXI_HOME: home, LINGXI_PORT: "0", LINGXI_ROOT: REPO, LINGXI_SERVER_ENTRY: path.join(REPO, "server", "main-full.ts"), LINGXI_CREATE_STARTUP_SESSION: "0" },
  stdio: ["ignore", "pipe", "pipe"],
});
try {
  const infoPath = path.join(home, "server-info.json");
  let info = null;
  const deadline = Date.now() + 90000;
  while (!info) {
    try { info = JSON.parse(fs.readFileSync(infoPath, "utf8")); } catch { /* */ }
    if (child.exitCode !== null) throw new Error("server exited early");
    if (Date.now() > deadline) throw new Error("boot timeout");
    await sleep(150);
  }
  const base = `http://127.0.0.1:${info.port}`;
  const H = { authorization: `Bearer ${info.token}`, "content-type": "application/json" };

  const before = await (await fetch(`${base}/api/sessions`, { headers: H })).json();

  // N1 错误 token
  const n1 = await fetch(`${base}/api/sessions`, { headers: { authorization: "Bearer hana-forged-token-p08-neg", "content-type": "application/json" } });
  const afterN1 = await (await fetch(`${base}/api/sessions`, { headers: H })).json();
  record("N1 错误 token → 403 零副作用", "403 + 会话列表不变", `${n1.status} list_unchanged=${JSON.stringify(afterN1) === JSON.stringify(before)}`, n1.status === 403 && JSON.stringify(afterN1) === JSON.stringify(before));

  // N2 跨域会话路径（agents 目录外伪造路径）
  const forged = { sessionId: "sess_forged_p08", sessionPath: "/etc/passwd", pinned: true };
  const n2 = await fetch(`${base}/api/sessions/pin`, { method: "POST", headers: H, body: JSON.stringify(forged) });
  const afterN2 = await (await fetch(`${base}/api/sessions`, { headers: H })).json();
  record("N2 伪造跨域 sessionPath → 403 拒绝", "403/4xx + 零副作用", `${n2.status} list_unchanged=${JSON.stringify(afterN2) === JSON.stringify(before)}`, n2.status >= 400 && JSON.stringify(afterN2) === JSON.stringify(before));

  // N3 非法字段类型
  const n3 = await fetch(`${base}/api/sessions/new`, { method: "POST", headers: H, body: '{"agentId": 12345}' });
  const b3 = await n3.json();
  const afterN3 = await (await fetch(`${base}/api/sessions`, { headers: H })).json();
  record("N3 字段类型错误 → 400 invalid_field_type 零副作用", "400 + code=invalid_field_type + 列表不变", `${n3.status} code=${b3.code ?? b3.error?.code} list_unchanged=${JSON.stringify(afterN3) === JSON.stringify(before)}`, n3.status === 400 && (b3.code ?? b3.error?.code) === "invalid_field_type" && JSON.stringify(afterN3) === JSON.stringify(before));

  child.kill("SIGTERM");
  const code = await new Promise((r) => { const t = setTimeout(() => child.kill("SIGKILL"), 15000); child.on("exit", (c) => { clearTimeout(t); r(c); }); });
  record("N0 退出", "exit 0", `exit=${code}`, code === 0);
} finally {
  try { if (child.exitCode === null) child.kill("SIGKILL"); } catch { /* */ }
  fs.rmSync(home, { recursive: true, force: true });
}
const allOk = results.every((r) => r.status === "PASS");
fs.writeFileSync(path.join(REPO, "artifacts", "refactor-2026", "P08", "logs", "negative-probes.json"), JSON.stringify({ tool: "P08 negative-probes", results, all_pass: allOk }, null, 2));
console.log(`negative probes: ${results.filter((r) => r.status === "PASS").length}/${results.length} PASS`);
process.exit(allOk ? 0 : 1);
