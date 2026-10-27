#!/usr/bin/env node
/**
 * R00-T06｜旧系统 PTY 大输出基准（真实组件驱动）
 *
 * 旧系统事实（本轮实测）：主聊天会话经 mcp_call 调 run_code 返回
 * "no active session — run_code kernels are session-scoped"（主聊天
 * buildTools 不绑定 sessionRef，见 core/session-coordinator.ts:2201 与
 * core/engine.ts:4033 的降级链）。因此 PTY 负载改为直接驱动**真实组件栈**：
 *
 *   真实 node-pty backend（lib/terminal/node-pty-backend.ts，pty.spawn）
 *   → 真实 TerminalSessionManager（transcript 落盘）
 *   → 真实 terminal-ws-bridge（事件归并/节流）
 *   → 真实 TerminalOutputStream（desktop/src/react/services 渲染端流）
 *
 * 即 benchmark-terminal-ui.mjs 的同构 harness，但 backend 用真 node-pty、
 * 子进程用真实 yes|head 数据发生器（确定性 1MB 输出）。边界如实登记：
 * 未经过 WS chat 路由（该路由在本负载上不可用），组件与产品完全同源。
 *
 * 用法：node scripts/rust-tauri/r00-t06-bench-pty.mjs [--samples 32]
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import {
  collectHostInfo, isoNow, nowMs, sleep, stats, snapshotTree, sampleTreeWindow,
  collectTreePids, sigkillTree, writeJson, appendJsonl, waitForTreeExit,
} from "./r00-t06-lib.mjs";
import { TerminalSessionManager } from "../../lib/terminal/terminal-session-manager.ts";
import { createTerminalWsBridge } from "../../server/terminal-ws-bridge.ts";
import { createTerminalOutputStream } from "../../desktop/src/react/services/terminal-output-stream.ts";
import { createAsyncNodePtyBackend } from "../../lib/terminal/node-pty-backend.ts";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..", "..");
const OUT_DIR = path.join(ROOT, "artifacts", "rust-tauri", "R00", "T06", "raw", "server");
fs.mkdirSync(OUT_DIR, { recursive: true });
const RUN_ID = `pty-${new Date().toISOString().replace(/[:.]/g, "-")}`;

const argv = process.argv.slice(2);
const SAMPLES = Number(argv.includes("--samples") ? argv[argv.indexOf("--samples") + 1] : "32");
// 协议 §8 冻结负载：8192 行 × 128B（127 字符 + \n）= 1 MiB/样本。
// R1 修复（F06）：首轮夹具误用 48B/行（≈0.4MB），低于冻结的 ≈1MB；按任务书
// §6.1「冻结后不得因结果缩小负载」，将夹具提到协议值重测，不缩小协议。
// 旧 0.4MB 结果保留在 raw/（pty-2026-09-23T22-12-53-212Z）作过程记录。
const LINES = 8192;
const LINE_TEXT = ("pty-bench-1mib-" + "0123456789abcdefghijklmnopqrstuvwxyz".repeat(6)).slice(0, 127);
if (LINE_TEXT.length !== 127) throw new Error(`LINE_TEXT 长度 ${LINE_TEXT.length} ≠ 127（128B/行 = 1MiB/8192 行）`);

const summary = {
  runId: RUN_ID,
  task: "R00-T06 PTY leg",
  head: null,
  host: collectHostInfo({ repoRoot: ROOT }),
  samples: SAMPLES,
  output: { lines: LINES, lineBytes: LINE_TEXT.length + 1 },
  boundary: "真实 node-pty/TerminalSessionManager/terminal-ws-bridge/TerminalOutputStream；未经 WS chat 路由（旧系统主聊天 run_code 不可用，见报告）",
  phases: {},
};

async function main() {
  summary.head = summary.host.gitHead;
  const backendPromise = createAsyncNodePtyBackend();
  const rawPath = path.join(OUT_DIR, `pty-driver-${RUN_ID}.jsonl`);
  const rows = [];

  for (let i = 0; i < SAMPLES; i++) {
    const lingxiHome = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-r00t06-pty-"));
    const backend = await backendPromise;
    const expandedStream = createTerminalOutputStream();
    const bridgeRef = { current: null };
    let outputBatches = 0;
    let outputBytes = 0;
    let firstDeliveryAt = null;
    const sessionName = `bench-pty-${i}`;
    const manager = new TerminalSessionManager({
      lingxiHome,
      createBackend: () => backend,
      getSessionIdForPath: (sessionPath) => `sess_${path.basename(sessionPath, ".jsonl")}`,
      emitEvent: (event, sessionPath) => bridgeRef.current?.handleEvent(event, sessionPath),
    });
    bridgeRef.current = createTerminalWsBridge({
      terminalSessions: manager,
      resolveSessionId: (sessionPath) => `sess_${path.basename(sessionPath, ".jsonl")}`,
      broadcast: (message) => {
        if (message.type !== "terminal_output") return;
        if (firstDeliveryAt === null) firstDeliveryAt = performance.now();
        outputBatches += 1;
        outputBytes += message.chunks.reduce((sum, c) => sum + Buffer.byteLength(c.data, "utf8"), 0);
        expandedStream.handleChunks(message);
      },
    });
    const sessionPath = path.join(lingxiHome, "agents", "lingxi", "sessions", `${sessionName}.jsonl`);
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });

    const t0 = performance.now();
    // 真实子进程链：yes <line> | head -n N（确定性大输出，经真实 PTY）
    const started = await manager.start({
      sessionPath,
      cwd: lingxiHome,
      command: `/bin/zsh -c "/usr/bin/yes '${LINE_TEXT}' | /usr/bin/head -n ${LINES}; echo PTY-BENCH-DONE-MARKER"`,
      label: sessionName,
    });
    const spawnMs = performance.now() - t0;
    const terminalId = started.terminalId;

    // 等 manager 视角退出（list 的 entry status/exitCode）
    const exited = await new Promise((resolve) => {
      const start = performance.now();
      const iv = setInterval(() => {
        const listResult = manager.list(sessionPath) || { terminals: [] };
        const entry = (listResult.terminals || []).find((e) => e.terminalId === terminalId);
        const done = entry && entry.status !== "running" && entry.exitCode !== null && entry.exitCode !== undefined;
        if (done || performance.now() - start > 90000) { clearInterval(iv); resolve(!!done); }
      }, 5);
    });
    // 流静默 150ms 后统计
    await sleep(150);
    const deliveredAt = performance.now();
    const elapsedMs = deliveredAt - t0;

    // transcript 落盘核对
    let transcriptBytes = 0; let transcriptOk = false;
    try {
      const termRoot = path.join(lingxiHome, ".ephemeral", "terminal-sessions");
      for (const f of fs.readdirSync(termRoot)) {
        const fp = path.join(termRoot, f);
        if (fs.statSync(fp).isFile()) {
          transcriptBytes += fs.statSync(fp).size;
          if (fs.readFileSync(fp, "utf8").includes("PTY-BENCH-DONE-MARKER")) transcriptOk = true;
        }
      }
    } catch {}

    // 清理：close → 核对辅助进程（yes/head/zsh）全部消失
    try { manager.close({ sessionPath, terminalId }); } catch {}
    await sleep(300);
    const leftoverAfterClose = snapshotTree([process.pid]).procs.filter((p) => /yes|head|zsh/.test(p.comm) && p.pid !== process.pid);

    const row = {
      phase: "pty", i, ts: isoNow(),
      spawnMs,
      firstDeliveryMs: firstDeliveryAt !== null ? firstDeliveryAt - t0 : null,
      totalMs: elapsedMs,
      exitedCleanly: exited,
      outputBatches,
      outputBytes,
      expandedStreamChunks: expandedStream.getStats?.() ?? null,
      transcriptBytes,
      transcriptHasDoneMarker: transcriptOk,
      leftoverHelperPidsAfterClose: leftoverAfterClose.map((p) => `${p.pid}:${p.comm}`),
    };
    rows.push(row);
    appendJsonl(rawPath, row);
    if (!exited || !transcriptOk) {
      console.error(`[FAIL][pty#${i}] exited=${exited} transcriptOk=${transcriptOk} bytes=${transcriptBytes}`);
    }
    fs.rmSync(lingxiHome, { recursive: true, force: true });
    await sleep(300);
  }

  summary.phases.pty_big_output = {
    spawnMs: stats(rows.map((r) => r.spawnMs)),
    firstDeliveryMs: stats(rows.map((r) => r.firstDeliveryMs).filter(Number.isFinite)),
    totalMs: stats(rows.map((r) => r.totalMs)),
    outputBytes: stats(rows.map((r) => r.outputBytes)),
    transcriptBytes: stats(rows.map((r) => r.transcriptBytes)),
    transcriptVerified: rows.filter((r) => r.transcriptHasDoneMarker).length,
    leftoverHelperProcs: rows.filter((r) => r.leftoverHelperPidsAfterClose.length).length,
    errors: rows.filter((r) => !r.exitedCleanly).length,
    raw: path.relative(ROOT, rawPath),
  };
  writeJson(path.join(OUT_DIR, `summary-${RUN_ID}.json`), summary);
  const bad = rows.filter((r) => !r.exitedCleanly || !r.transcriptHasDoneMarker).length;
  console.log(JSON.stringify({ ok: bad === 0, bad, totalMs: summary.phases.pty_big_output.totalMs }, null, 2));
  process.exit(bad === 0 ? 0 : 1);
}

main().catch((err) => { console.error(err); process.exit(1); });
