#!/usr/bin/env node
/**
 * R00-T06｜旧系统（Electron+Node）server 腿基准
 *
 * 被测对象：dist-server/<os>-<arch> 发布优化 bundle（生产入口 hana-server wrapper，
 * 自带 Node 运行时）。全部请求走真实 HTTP/WS 到 127.0.0.1，模型协议由确定性本地
 * stub 提供（模型等待与本地开销分离，见 r00-t06-stub-provider.mjs）。
 *
 * 阶段（每阶段原始逐样本数据落 raw/）：
 *   startup.cold / startup.warm —— spawn→server-info.json→/api/health 200，
 *       然后 SIGTERM→整树退出（同时得关闭窗口样本）。
 *   idle.memory —— 就绪+10s 稳定后进程树 RSS 采样窗（10×500ms）。
 *   history.* —— 真实 GET /api/sessions/messages：长会话(1000条)首页/翻页/304、
 *       短会话首页，各 ≥30 样本。
 *   stream.request —— 真实 WS /ws prompt→stub 零延迟流式→run 结束，≥30 样本；
 *       记录首事件/首增量/结束时刻，与 stub 服务端日志按 marker 关联。
 *   cancel.hang —— stub hang 模式流中 abort→abort_result，≥30 样本。
 *
 * （PTY 大输出阶段原计划经 run_code 工具链压测，实测旧系统主聊天会话
 * run_code 不可用（"no active session"），已移至 r00-t06-bench-pty.mjs
 * 以真实组件栈直驱，见协议 §9 与报告 §7。）
 *
 * 用法：node scripts/rust-tauri/r00-t06-bench-server.mjs [--samples 32] [--phases a,b,c]
 * 任何阶段失败：该阶段标 FAIL 并继续可独立阶段；退出码非零当且仅当有 FAIL。
 */
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import WebSocket from "ws";
import {
  collectHostInfo, isoNow, nowMs, sleep, stats, sampleTreeWindow,
  snapshotTree, waitForTreeExit, sigkillTree, writeJson, appendJsonl, sha256File,
} from "./r00-t06-lib.mjs";
import { startStubProvider } from "./r00-t06-stub-provider.mjs";
import { buildPristineHome, cloneHome, FIXTURE_SPEC } from "./r00-t06-fixture.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..", "..");
// dist-server 目录取 build-server 的 osDirName 约定：darwin→mac
const PLATFORM_ARCH = `${process.platform === "darwin" ? "mac" : process.platform}-${process.arch}`;
const OS_DIR = PLATFORM_ARCH.split("-")[0];

// ── CLI ──
const argv = process.argv.slice(2);
function argVal(name, dflt) {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 ? argv[i + 1] : dflt;
}
const SAMPLES = Number(argVal("samples", "32"));
const PHASES = (argVal("phases", "startup,idle,history,stream,cancel") || "").split(",");
const OUT_DIR = path.join(ROOT, "artifacts", "rust-tauri", "R00", "T06", "raw", "server");
const SERVER_WRAPPER = path.join(ROOT, "dist-server", PLATFORM_ARCH, "hana-server");
const TOKEN = "r00t06-not-a-secret";

fs.mkdirSync(OUT_DIR, { recursive: true });
const RUN_ID = `run-${new Date().toISOString().replace(/[:.]/g, "-")}`;

const failures = [];
function fail(phase, err) {
  const msg = `${phase}: ${err?.stack || err}`;
  failures.push(msg);
  console.error(`[FAIL][${phase}] ${msg}`);
}

// ── 进程管理 ──
const spawned = new Set(); // {pid, name}
async function spawnServer(homePath, { owner = "standalone" } = {}) {
  if (!fs.existsSync(SERVER_WRAPPER)) {
    throw new Error(`missing release server bundle: ${SERVER_WRAPPER}（先跑 r00-t06-build-release.mjs）`);
  }
  const child = spawn(SERVER_WRAPPER, [], {
    cwd: path.dirname(SERVER_WRAPPER),
    env: {
      // PATH 置入 bundle 自带 node 运行时目录（与 hana-server wrapper 自身使用的
      // 同一运行时），保持确定性。原为 run_code 阶段而设；该阶段已移至
      // bench-pty.mjs，此 env 保留以与已冻结的各 run 测量条件一致。
      PATH: [
        path.join(path.dirname(SERVER_WRAPPER), "node"),
        path.dirname(SERVER_WRAPPER),
        "/usr/bin:/bin:/usr/sbin:/sbin",
      ].join(":"),
      HOME: homePath, // 隔离：不继承真实 HOME 的 ~/.lingxi 等指针
      LINGXI_HOME: homePath,
      LINGXI_PORT: "0",
      LINGXI_TOKEN: TOKEN,
      LINGXI_CREATE_STARTUP_SESSION: "0",
      LINGXI_SERVER_OWNER: owner,
      TMPDIR: os.tmpdir(),
    },
    stdio: ["ignore", "pipe", "pipe"],
    detached: false,
  });
  const logs = [];
  child.stdout.on("data", (c) => logs.push(c.toString()));
  child.stderr.on("data", (c) => logs.push(c.toString()));
  const entry = { pid: child.pid, child, name: "hana-server", homePath, logs };
  spawned.add(entry);
  child.on("exit", () => spawned.delete(entry));
  return entry;
}

async function stopServer(entry, { sig = "SIGTERM", maxWaitMs = 20000 } = {}) {
  if (!entry || spawned.has(entry) === false && !entry.child) return null;
  const t0 = nowMs();
  try { entry.child.kill(sig); } catch {}
  const exitMs = await waitForTreeExit(entry.pid, maxWaitMs, 50);
  if (exitMs === null) {
    sigkillTree(entry.pid);
    await waitForTreeExit(entry.pid, 10000, 50);
  }
  return { signalMs: exitMs, forceKilled: exitMs === null };
}

/** server-info.json + /api/health 双就绪判定（协议冻结信号）。 */
async function waitReady(entry, { timeoutMs = 60000, pollMs = 25 } = {}) {
  const infoPath = path.join(entry.homePath, "server-info.json");
  const t0 = nowMs();
  const deadlines = { info: null, health: null };
  const startedAt = Date.now();
  const deadline = t0 + timeoutMs;
  let info = null;
  while (nowMs() < deadline) {
    if (deadlines.info === null && fs.existsSync(infoPath)) {
      try {
        const parsed = JSON.parse(fs.readFileSync(infoPath, "utf8"));
        if (parsed?.port && parsed?.pid === entry.pid) {
          info = parsed;
          deadlines.info = nowMs() - t0;
        }
      } catch {}
    }
    if (info && deadlines.health === null) {
      try {
        const res = await fetch(`http://127.0.0.1:${info.port}/api/health`, {
          headers: { Authorization: `Bearer ${TOKEN}` },
          signal: AbortSignal.timeout(2000),
        });
        if (res.ok) {
          const body = await res.json().catch(() => null);
          if (body?.status === "ok") deadlines.health = nowMs() - t0;
        }
      } catch {}
    }
    if (deadlines.info !== null && deadlines.health !== null) {
      return { tInfoMs: deadlines.info, tHealthMs: deadlines.health, info, startedAt };
    }
    await sleep(pollMs);
  }
  throw new Error(`server not ready in ${timeoutMs}ms: ${JSON.stringify({
    info: deadlines.info, health: deadlines.health, logTail: entry.logs.join("").slice(-2000),
  })}`);
}

// ── WS 聊天驱动（真实 /ws 协议） ──
function wsChat({ port, sessionPath }) {
  return new Promise((resolve) => {
    const events = [];
    const t = { open: null, closed: null };
    const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`, {
      headers: { Authorization: `Bearer ${TOKEN}` },
    });
    const mark = (ev) => {
      ev.__recvMs = nowMs();
      events.push(ev);
    };
    ws.on("open", () => { t.open = nowMs(); resolve({ ws, events, t, mark }); });
    ws.on("message", (raw) => {
      try { mark(JSON.parse(raw.toString())); } catch { mark({ type: "_unparsable" }); }
    });
    ws.on("error", () => {});
    ws.on("close", () => { t.closed = nowMs(); });
  });
}

function wsSend(ws, obj) {
  ws.send(JSON.stringify(obj));
}

function findEvent(events, pred) {
  return events.find(pred) || null;
}

async function newSession(port) {
  const res = await fetch(`http://127.0.0.1:${port}/api/sessions/new`, {
    method: "POST",
    headers: { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" },
    body: JSON.stringify({ memoryEnabled: false, permissionMode: "operate" }),
    signal: AbortSignal.timeout(15000),
  });
  if (!res.ok) throw new Error(`sessions/new ${res.status}: ${await res.text()}`);
  return res.json();
}

// ── 主流程 ──
const summary = {
  runId: RUN_ID,
  task: "R00-T06 server leg",
  head: null,
  host: collectHostInfo({ repoRoot: ROOT }),
  samples: SAMPLES,
  serverWrapper: path.relative(ROOT, SERVER_WRAPPER),
  serverBundleSha256: null,
  stub: null,
  phases: {},
};

async function main() {
  summary.head = summary.host.gitHead;

  // stub provider
  const stub = await startStubProvider({ journalPath: path.join(OUT_DIR, `stub-journal-${RUN_ID}.jsonl`) });
  summary.stub = { baseUrl: stub.baseUrl, config: (await (await fetch(stub.baseUrl.replace(/\/v1$/, "/info"))).json()).config };

  // pristine fixture
  const pristine = path.join(OUT_DIR, "pristine-home");
  fs.rmSync(pristine, { recursive: true, force: true });
  const fixture = buildPristineHome({ dest: pristine, stubBaseUrl: stub.baseUrl });
  summary.fixture = fixture;
  try {
    summary.serverBundleSha256 = sha256File(path.join(ROOT, "dist-server", PLATFORM_ARCH, "bundle", "index.js"));
  } catch {}

  const rawStartup = path.join(OUT_DIR, `startup-${RUN_ID}.jsonl`);
  const rawHistory = path.join(OUT_DIR, `history-${RUN_ID}.jsonl`);
  const rawStream = path.join(OUT_DIR, `stream-${RUN_ID}.jsonl`);
  const rawCancel = path.join(OUT_DIR, `cancel-${RUN_ID}.jsonl`);

  // ══ 阶段：startup（cold/warm 各 SAMPLES 个独立样本） ══
  if (PHASES.includes("startup")) {
    for (const kind of ["cold", "warm"]) {
      const rows = [];
      for (let i = 0; i < SAMPLES; i++) {
        const home = cloneHome(pristine, kind === "cold" ? "cold" : "warm");
        const t0 = nowMs();
        const entry = await spawnServer(home);
        let row;
        try {
          const ready = await waitReady(entry);
          const stop = await stopServer(entry);
          row = {
            phase: `startup.${kind}`, i, ts: isoNow(),
            home,
            t_spawn_to_info_ms: ready.tInfoMs,
            t_spawn_to_health_ms: ready.tHealthMs,
            shutdown_sigterm_tree_exit_ms: stop.signalMs,
            shutdown_force_killed: stop.forceKilled,
          };
        } catch (err) {
          await stopServer(entry).catch(() => sigkillTree(entry.pid));
          row = { phase: `startup.${kind}`, i, ts: isoNow(), error: String(err?.message || err) };
        }
        rows.push(row);
        appendJsonl(rawStartup, row);
        if (row.error) { fail(`startup.${kind}#${i}`, row.error); break; }
        fs.rmSync(home, { recursive: true, force: true });
        await sleep(kind === "cold" ? 5000 : 400);
      }
      summary.phases[`startup_${kind}`] = {
        t_spawn_to_info_ms: stats(rows.map((r) => r.t_spawn_to_info_ms).filter(Number.isFinite)),
        t_spawn_to_health_ms: stats(rows.map((r) => r.t_spawn_to_health_ms).filter(Number.isFinite)),
        shutdown_sigterm_tree_exit_ms: stats(rows.map((r) => r.shutdown_sigterm_tree_exit_ms).filter(Number.isFinite)),
        errors: rows.filter((r) => r.error).length,
        raw: path.relative(ROOT, rawStartup),
      };
      console.log(`[startup.${kind}] info=${JSON.stringify(summary.phases[`startup_${kind}`].t_spawn_to_info_ms)}`);
    }
  }

  // 长驻 server 供后续阶段
  let live = null;
  let port = null;
  async function ensureLive() {
    if (live && spawned.has(live)) return;
    const home = cloneHome(pristine, "live");
    live = await spawnServer(home);
    const ready = await waitReady(live);
    port = ready.info.port;
    live.__ready = ready;
    live.homePath = home;
    console.log(`[live] server pid=${live.pid} port=${port} home=${home}`);
  }

  // ══ 阶段：idle memory ══
  if (PHASES.includes("idle")) {
    try {
      await ensureLive();
      await sleep(10000); // 就绪后稳定
      const win = await sampleTreeWindow([live.pid], { samples: 10, intervalMs: 500, label: "server-idle" });
      summary.phases.idle_memory = {
        rssMedianKb: win.rssMedianKb,
        rssMaxKb: win.rssMaxKb,
        rssSeriesKb: win.rssSeriesKb,
        procCountByFrame: win.procCountByFrame,
        raw: path.relative(ROOT, path.join(OUT_DIR, `idle-tree-${RUN_ID}.json`)),
      };
      writeJson(path.join(OUT_DIR, `idle-tree-${RUN_ID}.json`), win);
      console.log(`[idle] rssMedian=${win.rssMedianKb}KB procs=${win.procCountByFrame.join(",")}`);
    } catch (err) { fail("idle", err); }
  }

  // ══ 阶段：history load ══
  if (PHASES.includes("history")) {
    try {
      await ensureLive();
      const base = `http://127.0.0.1:${port}/api/sessions/messages`;
      // 历史读取的 path 必须落在活 server 自己的 LINGXI_HOME 内（路由做
      // isValidSessionPath 域校验，pristine 路径会被 403）——克隆时已随
      // pristine 带入 bench 会话文件，这里解析到活 home 的绝对路径。
      const longPath = path.join(live.homePath, "agents", FIXTURE_SPEC.agentId, "sessions", "bench-long-1000.jsonl");
      const shortPath = path.join(live.homePath, "agents", FIXTURE_SPEC.agentId, "sessions", "bench-short-20.jsonl");

      async function timedGet(url, headers = {}) {
        const t0 = nowMs();
        const res = await fetch(url, { headers: { Authorization: `Bearer ${TOKEN}`, ...headers }, signal: AbortSignal.timeout(30000) });
        const ttfb = nowMs() - t0;
        const body = await res.text();
        const total = nowMs() - t0;
        return { status: res.status, etag: res.headers.get("etag"), ttfbMs: ttfb, totalMs: total, bodyBytes: Buffer.byteLength(body), body };
      }

      // 长会话首页 ×SAMPLES
      const rows = [];
      let etag = null;
      for (let i = 0; i < SAMPLES; i++) {
        const r = await timedGet(`${base}?path=${encodeURIComponent(longPath)}&limit=50`);
        etag = etag || r.etag;
        rows.push({ kind: "long-first-page", i, ts: isoNow(), ...pick(r, ["status", "ttfbMs", "totalMs", "bodyBytes"]) });
        appendJsonl(rawHistory, rows[rows.length - 1]);
        if (r.status !== 200) { fail(`history.long#${i}`, `status ${r.status}`); break; }
      }
      summary.phases.history_long_first_page = stats(rows.map((r) => r.totalMs));

      // 304 条件 GET ×SAMPLES
      const rows304 = [];
      for (let i = 0; i < SAMPLES && etag; i++) {
        const r = await timedGet(`${base}?path=${encodeURIComponent(longPath)}&limit=50`, { "If-None-Match": etag });
        rows304.push({ kind: "long-etag-304", i, ts: isoNow(), ...pick(r, ["status", "ttfbMs", "totalMs", "bodyBytes"]) });
        appendJsonl(rawHistory, rows304[rows304.length - 1]);
        if (r.status !== 304) { fail(`history.304#${i}`, `status ${r.status}`); break; }
      }
      summary.phases.history_long_etag_304 = {
        ...stats(rows304.map((r) => r.totalMs)),
        statuses: [...new Set(rows304.map((r) => r.status))],
      };

      // 短会话首页 ×SAMPLES
      const rowsShort = [];
      for (let i = 0; i < SAMPLES; i++) {
        const r = await timedGet(`${base}?path=${encodeURIComponent(shortPath)}&limit=50`);
        rowsShort.push({ kind: "short-first-page", i, ts: isoNow(), ...pick(r, ["status", "ttfbMs", "totalMs", "bodyBytes"]) });
        appendJsonl(rawHistory, rowsShort[rowsShort.length - 1]);
        if (r.status !== 200) { fail(`history.short#${i}`, `status ${r.status}`); break; }
      }
      summary.phases.history_short_first_page = stats(rowsShort.map((r) => r.totalMs));

      // 全量翻页 ×3（顺序页样本，另列不混入首页统计）
      const walkRows = [];
      for (let w = 0; w < 3; w++) {
        let before = null; let pages = 0; let bytes = 0; const t0 = nowMs();
        for (;;) {
          const u = `${base}?path=${encodeURIComponent(longPath)}&limit=50${before !== null ? `&before=${before}` : ""}`;
          const r = await timedGet(u);
          if (r.status !== 200) { fail(`history.walk#w${w}p${pages}`, `status ${r.status}`); break; }
          const j = JSON.parse(r.body);
          bytes += r.bodyBytes; pages += 1;
          // nextBefore 以字符串返回（"951"）：先转数值再判定
          const nb = Number(j.nextBefore);
          if (!j.hasMore || !Number.isFinite(nb)) break;
          before = nb;
          if (pages > 100) { fail("history.walk", "page cap"); break; }
        }
        walkRows.push({ kind: "long-full-walk", w, pages, bytes, totalMs: nowMs() - t0, ts: isoNow() });
        appendJsonl(rawHistory, walkRows[walkRows.length - 1]);
      }
      summary.phases.history_long_full_walk = {
        pages: walkRows.map((r) => r.pages),
        totalMsStats: stats(walkRows.map((r) => r.totalMs)),
      };
      console.log(`[history] long=${JSON.stringify(summary.phases.history_long_first_page)} 304=${JSON.stringify(summary.phases.history_long_etag_304?.statuses)}`);
    } catch (err) { fail("history", err); }
  }

  // ══ 阶段：stream request（stub fast：零模型等待） ══
  if (PHASES.includes("stream")) {
    try {
      await ensureLive();
      await fetch(stub.controlUrl("fast"));
      const rows = [];
      for (let i = 0; i < SAMPLES; i++) {
        const s = await newSession(port);
        const chat = await wsChat({ port, sessionPath: s.path });
        const tPrompt = nowMs();
        wsSend(chat.ws, {
          type: "prompt",
          text: `bench-stream-${i} direct answer, no tools.`,
          sessionPath: s.path,
        });
        const done = new Promise((resolve) => {
          const iv = setInterval(() => {
            const firstAny = findEvent(chat.events, (e) => e.type && e.type !== "status" && e.__recvMs > tPrompt);
            const firstDelta = findEvent(chat.events, (e) => /delta/i.test(e.type || "") && e.__recvMs > tPrompt);
            const runEnd = findEvent(chat.events, (e) => (e.type === "assistant_run_end" || e.type === "turn_end" || e.type === "agent_settled") && e.__recvMs > tPrompt);
            if (runEnd) {
              clearInterval(iv);
              resolve({
                firstEventMs: firstAny ? firstAny.__recvMs - tPrompt : null,
                firstDeltaMs: firstDelta ? firstDelta.__recvMs - tPrompt : null,
                runEndMs: runEnd.__recvMs - tPrompt,
                eventTypes: [...new Set(chat.events.map((e) => e.type))],
                aborted: false,
              });
            }
          }, 5);
          setTimeout(() => { clearInterval(iv); resolve({ timeout: true, eventTypes: [...new Set(chat.events.map((e) => e.type))] }); }, 60000);
        });
        const result = await done;
        const row = {
          phase: "stream.request", kind: "fast", i, ts: isoNow(),
          sessionPath: s.path, marker: `bench-stream-${i}`,
          t_ws_open_ms: chat.t.open !== null ? chat.t.open - tPrompt : null,
          ...result,
        };
        rows.push(row);
        appendJsonl(rawStream, row);
        try { chat.ws.close(); } catch {}
        if (result.timeout || result.aborted) { fail(`stream#${i}`, JSON.stringify(result).slice(0, 300)); break; }
        await sleep(200);
      }
      summary.phases.stream_request_fast = {
        firstDeltaMs: stats(rows.map((r) => r.firstDeltaMs).filter(Number.isFinite)),
        runEndMs: stats(rows.map((r) => r.runEndMs).filter(Number.isFinite)),
        errors: rows.filter((r) => r.timeout).length,
        raw: path.relative(ROOT, rawStream),
      };
      console.log(`[stream] firstDelta=${JSON.stringify(summary.phases.stream_request_fast.firstDeltaMs)} runEnd=${JSON.stringify(summary.phases.stream_request_fast.runEndMs)}`);
    } catch (err) { fail("stream", err); }
  }

  // ══ 阶段：cancel during hang ══
  if (PHASES.includes("cancel")) {
    try {
      await ensureLive();
      await fetch(stub.controlUrl("hang"));
      const rows = [];
      for (let i = 0; i < SAMPLES; i++) {
        const s = await newSession(port);
        const chat = await wsChat({ port, sessionPath: s.path });
        const tPrompt = nowMs();
        wsSend(chat.ws, { type: "prompt", text: `bench-cancel-${i} hold the stream.`, sessionPath: s.path });
        // 等到首个增量（确认已进入流中）
        const firstDelta = await new Promise((resolve) => {
          const iv = setInterval(() => {
            const d = findEvent(chat.events, (e) => /delta/i.test(e.type || "") && e.__recvMs > tPrompt);
            if (d) { clearInterval(iv); resolve(d.__recvMs - tPrompt); }
          }, 5);
          setTimeout(() => { clearInterval(iv); resolve(null); }, 30000);
        });
        if (firstDelta === null) {
          const row = { phase: "cancel", i, ts: isoNow(), error: "no first delta within 30s", eventTypes: [...new Set(chat.events.map((e) => e.type))] };
          rows.push(row); appendJsonl(rawCancel, row); fail(`cancel#${i}`, row.error);
          try { chat.ws.close(); } catch {}
          break;
        }
        const tAbort = nowMs();
        wsSend(chat.ws, { type: "abort", sessionPath: s.path });
        const abortResult = await new Promise((resolve) => {
          const iv = setInterval(() => {
            const ar = findEvent(chat.events, (e) => e.type === "abort_result" && e.__recvMs > tAbort);
            if (ar) { clearInterval(iv); resolve(ar); }
          }, 2);
          setTimeout(() => { clearInterval(iv); resolve(null); }, 15000);
        });
        const tAbortResult = abortResult ? abortResult.__recvMs - tAbort : null;
        // 等流停（status isStreaming false 或 run_end）
        const stopped = await new Promise((resolve) => {
          const iv = setInterval(() => {
            const st = findEvent(chat.events, (e) => (e.type === "status" && e.isStreaming === false && e.__recvMs > tAbort)
              || ((e.type === "assistant_run_end" || e.type === "turn_end") && e.__recvMs > tAbort));
            if (st) { clearInterval(iv); resolve(st.__recvMs - tAbort); }
          }, 5);
          setTimeout(() => { clearInterval(iv); resolve(null); }, 20000);
        });
        const row = {
          phase: "cancel", kind: "hang-abort", i, ts: isoNow(),
          firstDeltaMs: firstDelta,
          t_abort_to_abort_result_ms: tAbortResult,
          abortStatus: abortResult?.status ?? null,
          t_abort_to_stream_stopped_ms: stopped,
          marker: `bench-cancel-${i}`,
        };
        rows.push(row); appendJsonl(rawCancel, row);
        try { chat.ws.close(); } catch {}
        if (tAbortResult === null) { fail(`cancel#${i}`, "abort_result not received"); break; }
        await sleep(250);
      }
      await fetch(stub.controlUrl("fast"));
      summary.phases.cancel_hang_abort = {
        t_abort_to_abort_result_ms: stats(rows.map((r) => r.t_abort_to_abort_result_ms).filter(Number.isFinite)),
        t_abort_to_stream_stopped_ms: stats(rows.map((r) => r.t_abort_to_stream_stopped_ms).filter(Number.isFinite)),
        abortStatuses: [...new Set(rows.map((r) => r.abortStatus))],
        errors: rows.filter((r) => r.error).length,
        raw: path.relative(ROOT, rawCancel),
      };
      console.log(`[cancel] abortResult=${JSON.stringify(summary.phases.cancel_hang_abort.t_abort_to_abort_result_ms)}`);
    } catch (err) { fail("cancel", err); }
  }

  // 收尾：长驻 server 优雅关闭 + 终树快照
  if (live) {
    const before = snapshotTree([live.pid], "before-shutdown");
    const stop = await stopServer(live);
    const after = snapshotTree([live.pid], "after-shutdown");
    summary.phases.final_shutdown = {
      sigtermTreeExitMs: stop?.signalMs ?? null,
      forceKilled: stop?.forceKilled ?? null,
      procsBefore: before.procs.length,
      procsAfter: after.procs.length,
      leftoverPids: after.procs.map((p) => p.pid),
    };
    console.log(`[shutdown] exit=${stop?.signalMs}ms leftover=${summary.phases.final_shutdown.leftoverPids.length}`);
  }

  writeJson(path.join(OUT_DIR, `summary-${RUN_ID}.json`), summary);
  console.log(JSON.stringify({ ok: failures.length === 0, failures: failures.length, runId: RUN_ID, summaryPath: path.relative(ROOT, path.join(OUT_DIR, `summary-${RUN_ID}.json`)) }, null, 2));
  process.exit(failures.length ? 1 : 0);
}

function pick(obj, keys) {
  const o = {};
  for (const k of keys) o[k] = obj[k];
  return o;
}

process.on("exit", () => {
  // 兜底清理：任何本脚本启动且仍存活的 server 树强杀（只杀本脚本产物）
  for (const entry of [...spawned]) {
    try { sigkillTree(entry.pid); } catch {}
  }
});

main().catch((err) => { fail("main", err); process.exit(1); });
