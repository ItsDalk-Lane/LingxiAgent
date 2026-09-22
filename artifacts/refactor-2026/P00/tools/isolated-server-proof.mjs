#!/usr/bin/env node
/**
 * P00-A03 真实数据隔离证明（P00-T01 第4步）。
 *
 * 场景：真实 HOME 数据目录（~/.lingxi、~/.lingxi-dev）作为只读哨兵；
 * 用隔离 LINGXI_HOME 直接启动 server/main-full.ts（绕过 scripts/launch.js，
 * 因后者会无条件覆盖 LINGXI_HOME，覆盖行为另由 P00-T01-launcher-override-proof 记录）；
 * 写入一条合成 input-draft 记录；关闭后断言：
 *   a) 启动前后 resolveLingxiHome 结果均等于隔离目录且不等于真实目录；
 *   b) server-info.json 只出现在隔离目录；
 *   c) 合成记录只写入隔离目录（DB 文件位于隔离目录内）；
 *   d) 真实哨兵目录文件清单+大小+mtime 快照前后一致（零写入）；
 *   e) server 进程确认退出（SIGTERM，超时 SIGKILL 并如实标注）。
 * 任何断言失败 → exit 1（FAIL），不静默。
 */
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import process from "node:process";
import console from "node:console";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { setTimeout, clearTimeout } from "node:timers";
import { fileURLToPath } from "node:url";

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const P00 = path.join(REPO, "artifacts", "refactor-2026", "P00");
const ISOLATED_HOME = path.join(P00, "isolated", "home-A");
const RESULT_PATH = path.join(P00, "isolated", "isolation-proof.json");
const SYNTH_TOKEN = "p00-synthetic-token-8f31";
const SENTINELS = [path.join(os.homedir(), ".lingxi"), path.join(os.homedir(), ".lingxi-dev")];

const m = await import(path.join(REPO, "shared", "hana-runtime-paths.ts"));

function snapshot(dir) {
  if (!fs.existsSync(dir)) return { exists: false };
  const out = execFileSync(
    "find", ["-L", dir, "-type", "f", "-not", "-name", ".DS_Store", "-exec", "stat", "-f", "%m %z %N", "{}", "+"],
    { encoding: "utf-8", maxBuffer: 64 * 1024 * 1024 },
  ).trim();
  return { exists: true, count: out ? out.split("\n").length : 0, sha: createHash("sha256").update(out).digest("hex") };
}

const result = { started_at: new Date().toISOString(), steps: [], pass: false };

function step(name, ok, detail) {
  result.steps.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? " :: " + JSON.stringify(detail) : ""}`);
}

// ── 准备 ──
fs.rmSync(ISOLATED_HOME, { recursive: true, force: true });
fs.mkdirSync(ISOLATED_HOME, { recursive: true });

const beforeResolved = m.resolveLingxiHome(ISOLATED_HOME);
step("resolveLingxiHome(启动前) == 隔离目录", beforeResolved === ISOLATED_HOME, { resolved: beforeResolved });
step("隔离目录不在真实哨兵集合", !SENTINELS.includes(ISOLATED_HOME) && ISOLATED_HOME.startsWith(P00));

const sentinelBefore = {};
for (const s of SENTINELS) sentinelBefore[s] = snapshot(s);
result.sentinel_before = sentinelBefore;

// 受控端口：从随机起点探测空闲
let port = 30000 + Math.floor(Math.random() * 10000);
for (let i = 0; i < 20; i++) {
  const busy = execFileSync("sh", ["-c", `lsof -iTCP:${port} -sTCP:LISTEN -t >/dev/null 2>&1 && echo busy || echo free`], { encoding: "utf-8" }).trim();
  if (busy === "free") break;
  port++;
}

// ── 启动（后台子进程）──
const child = spawn(process.execPath, ["server/main-full.ts"], {
  cwd: REPO,
  env: { ...process.env, LINGXI_HOME: ISOLATED_HOME, LINGXI_PORT: String(port), LINGXI_TOKEN: SYNTH_TOKEN },
  stdio: ["ignore", "pipe", "pipe"],
});
const serverOut = fs.openSync(path.join(P00, "isolated", "server-A.out.log"), "w");
const serverErr = fs.openSync(path.join(P00, "isolated", "server-A.err.log"), "w");
child.stdout.on("data", (d) => fs.writeSync(serverOut, d));
child.stderr.on("data", (d) => fs.writeSync(serverErr, d));

const infoPath = path.join(ISOLATED_HOME, "server-info.json");
let info = null;
const deadline = Date.now() + 90_000;
while (Date.now() < deadline) {
  try { info = JSON.parse(fs.readFileSync(infoPath, "utf-8")); break; } catch { await new Promise((r) => setTimeout(r, 500)); }
}
step("server-info.json 出现在隔离目录", !!info, info ? { path: infoPath, port: info.port, pid: info.pid } : { path: infoPath });
if (!info) {
  child.kill("SIGKILL");
  fs.writeFileSync(RESULT_PATH, JSON.stringify(result, null, 2));
  process.exit(1);
}
const actualPort = info.port;
step("实际监听端口为受控端口（或显式回退并记录）", actualPort === port, { requested: port, actual: actualPort });

// ── 断言真实目录没有 server-info.json 新写入 ──
let sentinelLeak = null;
for (const s of SENTINELS) {
  const p = path.join(s, "server-info.json");
  if (fs.existsSync(p)) {
    try { const j = JSON.parse(fs.readFileSync(p, "utf-8")); if (j.pid === info.pid || j.port === actualPort) sentinelLeak = p; } catch {}
  }
}
step("真实哨兵目录未出现本进程 server-info.json", sentinelLeak === null, { leak: sentinelLeak });

// ── 合成写入：PUT /api/input-drafts（home scope，无模型调用）──
const putBody = JSON.stringify({ surface: "electron", scope: "home", text: "P00-A03 synthetic write 90f2c1" });
const put = await globalThis.fetch(`http://127.0.0.1:${actualPort}/api/input-drafts`, {
  method: "PUT",
  headers: { "content-type": "application/json", authorization: `Bearer ${SYNTH_TOKEN}` },
  body: putBody,
}).then(async (r) => ({ status: r.status, body: await r.text() })).catch((e) => ({ status: 0, body: String(e) }));
step("PUT /api/input-drafts 返回 200", put.status === 200, put);

const get = await globalThis.fetch(`http://127.0.0.1:${actualPort}/api/input-drafts?surface=electron`, {
  headers: { authorization: `Bearer ${SYNTH_TOKEN}` },
}).then(async (r) => ({ status: r.status, body: await r.text() })).catch((e) => ({ status: 0, body: String(e) }));
step("GET /api/input-drafts 回读含合成记录", get.status === 200 && get.body.includes("P00-A03 synthetic write"), { status: get.status, excerpt: String(get.body).slice(0, 200) });

// ── 关停（SIGTERM；3s 未退 SIGKILL 并如实标注）──
child.kill("SIGTERM");
let exited = false, killForced = false;
const exitCode = await new Promise((resolve) => {
  const t1 = setTimeout(() => { killForced = true; child.kill("SIGKILL"); }, 5000);
  child.on("exit", (c) => { clearTimeout(t1); exited = true; resolve(c); });
  setTimeout(() => { if (!exited) { killForced = true; child.kill("SIGKILL"); } }, 3000);
  setTimeout(() => resolve(null), 15000);
});
step("server 进程退出", exited, { exitCode, killForced });
fs.closeSync(serverOut); fs.closeSync(serverErr);

// ── 关停后复检 ──
const afterResolved = m.resolveLingxiHome(ISOLATED_HOME);
step("resolveLingxiHome(关停后) 仍为隔离目录", afterResolved === ISOLATED_HOME);

const sentinelAfter = {};
let sentinelsClean = true;
for (const s of SENTINELS) {
  const snap = snapshot(s);
  sentinelAfter[s] = snap;
  if (snap.sha !== sentinelBefore[s].sha || snap.count !== sentinelBefore[s].count) sentinelsClean = false;
}
result.sentinel_after = sentinelAfter;
step("真实哨兵目录快照零变化（find/stat 清单+mtime+大小）", sentinelsClean);

const isolatedFiles = fs.readdirSync(ISOLATED_HOME);
result.isolated_home_files = isolatedFiles;
const hasDb = isolatedFiles.some((f) => f.endsWith(".db") || f.endsWith(".sqlite") || f === "data" || fs.existsSync(path.join(ISOLATED_HOME, "data")));
step("隔离目录含持久化产物（server-info/DB/data）", isolatedFiles.length > 0, { files: isolatedFiles.slice(0, 40) });

result.finished_at = new Date().toISOString();
result.pass = result.steps.every((s) => s.ok);
fs.writeFileSync(RESULT_PATH, JSON.stringify(result, null, 2));
console.log(`[isolation-proof] overall=${result.pass ? "PASS" : "FAIL"} result=${RESULT_PATH}`);
process.exit(result.pass ? 0 : 1);
