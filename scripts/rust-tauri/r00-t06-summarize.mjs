#!/usr/bin/env node
/**
 * R00-T06｜汇总 raw → BASELINE_BENCHMARK.json
 *
 * 读取 raw/server 与 raw/desktop 最新（或指定 --run）的 summary + stub journal，
 * 1) 关联 stub 服务端时间线，分离“模型等待 vs 本地开销”；
 * 2) 合并两腿指标、环境、构建摘要与原始数据指针；
 * 3) 写 artifacts/rust-tauri/R00/T06/BASELINE_BENCHMARK.json。
 *
 * BASELINE_BENCHMARK.json 是 G1（旧 Electron+Node）在冻结协议下的基线事实；
 * 不包含任何 Rust 数据（该时点不存在），也不做任何合格判断（那是
 * PERFORMANCE_THRESHOLDS.json 的事）。
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { collectHostInfo, stats, readJson, writeJson } from "./r00-t06-lib.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..", "..");
const T06_DIR = path.join(ROOT, "artifacts", "rust-tauri", "R00", "T06");

const argv = process.argv.slice(2);
function argVal(name) {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 ? argv[i + 1] : null;
}

function latestRun(dir, prefix) {
  if (!fs.existsSync(dir)) return null;
  const cands = fs.readdirSync(dir).filter((f) => f.startsWith(prefix) && f.endsWith(".json"))
    .sort();
  return cands.length ? path.join(dir, cands[cands.length - 1]) : null;
}

function loadJsonl(p) {
  if (!p || !fs.existsSync(p)) return [];
  return fs.readFileSync(p, "utf8").split("\n").filter(Boolean).map((l) => {
    try { return JSON.parse(l); } catch { return null; }
  }).filter(Boolean);
}

function main() {
  const serverRun = argVal("server-run");
  const desktopRun = argVal("desktop-run");
  const ptyRun = argVal("pty-run");
  const serverDir = path.join(T06_DIR, "raw", "server");
  const desktopDir = path.join(T06_DIR, "raw", "desktop");

  const serverSummaryPath = serverRun
    ? path.join(serverDir, `summary-run-${serverRun.replace(/^run-/, "")}.json`)
    : latestRun(serverDir, "summary-run-");
  const desktopSummaryPath = desktopRun
    ? path.join(desktopDir, `summary-run-${desktopRun.replace(/^run-/, "")}.json`)
    : latestRun(desktopDir, "summary-run-");
  const ptySummaryPath = ptyRun
    ? path.join(serverDir, `summary-pty-${ptyRun.replace(/^pty-/, "")}.json`)
    : latestRun(serverDir, "summary-pty-");
  const buildSummaryPath = path.join(T06_DIR, "raw", "build-release-summary.json");

  if (!fs.existsSync(buildSummaryPath)) {
    throw new Error("missing build-release-summary.json（先跑 r00-t06-build-release.mjs）");
  }
  const build = readJson(buildSummaryPath);
  const server = serverSummaryPath && fs.existsSync(serverSummaryPath) ? readJson(serverSummaryPath) : null;
  const desktop = desktopSummaryPath && fs.existsSync(desktopSummaryPath) ? readJson(desktopSummaryPath) : null;
  const pty = ptySummaryPath && fs.existsSync(ptySummaryPath) ? readJson(ptySummaryPath) : null;

  // ── stub journal 关联：本地开销 = 客户端端到端 − stub 服务时间 ──
  function scanJournal(dir) {
    if (!fs.existsSync(dir)) return [];
    const files = fs.readdirSync(dir).filter((f) => f.startsWith("stub-journal-") && f.endsWith(".jsonl")).sort();
    return files.length ? loadJsonl(path.join(dir, files[files.length - 1])) : [];
  }
  const serverJournal = scanJournal(serverDir);
  const desktopJournal = scanJournal(desktopDir);

  function correlate(rows, journal, clientField) {
    const out = [];
    for (const r of rows) {
      const j = journal.find((e) => e.marker === r.marker && e.serviceMs != null && e.hanging !== true);
      if (j && Number.isFinite(r[clientField]) && Number.isFinite(j.serviceMs)) {
        out.push({
          marker: r.marker,
          clientMs: r[clientField],
          stubServiceMs: j.serviceMs,
          localOverheadMs: r[clientField] - j.serviceMs,
          requestBytes: j.requestBytes,
          messageCount: j.messageCount,
          toolCount: j.toolCount,
          hasSystemPrompt: j.hasSystemPrompt,
        });
      }
    }
    return out;
  }

  const streamRows = server ? loadJsonl(path.join(ROOT, server.phases.stream_request_fast?.raw || "/dev/null")) : [];
  const streamCorr = correlate(
    streamRows.filter((r) => Number.isFinite(r.runEndMs)),
    serverJournal, "runEndMs",
  );

  // R1-F04 口径澄清：stub journal 里带 bench-stream marker 的 665/666B 请求是
  // 「对话标题生成器」侧线调用（system≈403B、无 tools），不是主聊天请求。
  // 主聊天请求是 stream=true 且 tools 数组非空的行：stream 相 = mode:fast 的
  // 32 行；cancel 相 = mode:hang 的行（hang 模式下同一请求在停住与客户端断开
  // 时各记一次，按 seq 去重后每样本 1 行）。
  const mainChatFastRows = serverJournal.filter((e) => e.mode === "fast" && e.stream === true && Number.isFinite(e.toolCount) && e.toolCount > 0);
  const hangBySeq = new Map();
  for (const e of serverJournal) {
    if (e.mode === "hang" && e.stream === true && Number.isFinite(e.toolCount) && e.toolCount > 0) hangBySeq.set(e.seq, e);
  }
  const mainChatHangRows = [...hangBySeq.values()];

  const baseline = {
    schema: "lingxi-rust-tauri/R00-T06/BASELINE_BENCHMARK/1",
    task: "R00-T06",
    group: "G1-old-electron-node",
    protocolDoc: "docs/rust-tauri/R00/R00-T06_PROTOCOL.md",
    generatedAt: new Date().toISOString(),
    build: {
      head: build.gitHead,
      branch: build.gitBranch,
      buildId: build.buildId,
      builtAt: build.finishedAt,
      rebuilt: build.rebuilt ?? null,
      recomputedAt: build.recomputedAt ?? null,
      recomputeReason: build.recomputeReason ?? null,
      serverBundle: {
        path: build.artifacts.serverBundle.path,
        logicalBytes: build.artifacts.serverBundle.logicalBytes ?? build.artifacts.serverBundle.bytes ?? null,
        duBytes: build.artifacts.serverBundle.duBytes ?? null,
        bundleSha256: build.artifacts.serverBundle.bundleSha256,
      },
      desktopApp: {
        path: build.artifacts.desktopApp.path,
        logicalBytes: build.artifacts.desktopApp.logicalBytes ?? build.artifacts.desktopApp.bytes ?? null,
        duBytes: build.artifacts.desktopApp.duBytes ?? null,
        mainBundleSha256: build.artifacts.desktopApp.mainBundleSha256,
      },
      seedArchiveDir: {
        path: build.artifacts.seedArchiveDir.path,
        logicalBytes: build.artifacts.seedArchiveDir.logicalBytes ?? build.artifacts.seedArchiveDir.bytes ?? null,
        duBytes: build.artifacts.seedArchiveDir.duBytes ?? null,
        files: build.artifacts.seedArchiveDir.files ?? [],
      },
      calibersNote: "logicalBytes = statSync size 逐文件求和（逻辑字节）；duBytes = du -sk（APFS 分配字节）。协议 §9 分发占用与 G3 25% 收益对照只用 duBytes；desktopApp.duBytes 与 desktopLeg.distributionSize.lingxiApp.bytes 同口径（修复轮实测同值互证）。禁止跨口径相减或混算。",
      signKeyset: build.signKeyset,
    },
    host: server?.host || desktop?.host || collectHostInfo({ repoRoot: ROOT }),
    measuredPlatform: {
      platform: "darwin",
      arch: "arm64",
      status: "MEASURED",
    },
    pendingPlatforms: [
      { platform: "darwin", arch: "x64", status: "PENDING_MEASUREMENT" },
      { platform: "win32", arch: "x64", status: "PENDING_MEASUREMENT" },
      { platform: "linux", arch: "x64", status: "PENDING_MEASUREMENT" },
    ],
    serverLeg: server ? {
      runId: server.runId,
      samples: server.samples,
      startupCold: server.phases.startup_cold || null,
      startupWarm: server.phases.startup_warm || null,
      idleMemory: server.phases.idle_memory || null,
      historyLongFirstPage: server.phases.history_long_first_page || null,
      historyLongEtag304: server.phases.history_long_etag_304 || null,
      historyShortFirstPage: server.phases.history_short_first_page || null,
      historyLongFullWalk: server.phases.history_long_full_walk || null,
      streamRequestFast: server.phases.stream_request_fast || null,
      cancelHangAbort: server.phases.cancel_hang_abort || null,
      finalShutdown: server.phases.final_shutdown || null,
    } : null,
    ptyLeg: pty ? {
      runId: pty.runId,
      samples: pty.samples,
      boundary: pty.boundary,
      output: pty.output,
      ptyBigOutput: pty.phases.pty_big_output || null,
    } : null,
    desktopLeg: desktop ? {
      runId: desktop.runId,
      samples: desktop.samples,
      firstBootTemplate: desktop.phases.first_boot_template || null,
      startupCold: desktop.phases.startup_cold || null,
      startupWarm: desktop.phases.startup_warm || null,
      idleMemory: desktop.phases.idle_memory || null,
      browserMulti: desktop.phases.browser_multi || null,
      pdfConvert: desktop.phases.pdf_convert || null,
      distributionSize: desktop.phases.distribution_size || null,
      finalShutdown: desktop.phases.final_shutdown || null,
    } : null,
    modelWaitSeparation: {
      method: "客户端端到端 − stub 服务端时间（fast 模式，stub 零附加延迟）",
      streamRunEnd: {
        n: streamCorr.length,
        clientMs: stats(streamCorr.map((r) => r.clientMs)),
        stubServiceMs: stats(streamCorr.map((r) => r.stubServiceMs)),
        localOverheadMs: stats(streamCorr.map((r) => r.localOverheadMs)),
        fixedPrefix: {
          note: "R1-F04 口径澄清：本块（stub journal 中带 bench-stream marker 的 665/666B 请求）是每样本首轮后的「对话标题生成器」侧线调用（system≈403B、无 tools 数组、messageCount=2），不是主聊天请求——marker 出现在其用户消息里（含首轮对话摘要）因此被 marker 关联命中。",
          requestBytes: { ...stats(streamCorr.map((r) => r.requestBytes)), unit: "bytes" },
          messageCount: { ...stats(streamCorr.map((r) => r.messageCount)), unit: "count" },
          toolCount: { ...stats(streamCorr.map((r) => r.toolCount)), unit: "count" },
          hasSystemPromptAll: streamCorr.every((r) => r.hasSystemPrompt),
        },
        mainChatRequest: {
          note: "主聊天请求（stream=true 且 tools 数组非空）。固定前缀实测（R1-F04 探针，双通道一致）：system 消息 10,626B（UTF-8）+ 7 个可见工具 schema JSON 10,287B；产品诊断 LINGXI_CACHE_CONTRACT_DEBUG=1 的 cache_contract systemPromptBytes 同值 10,626。证据：raw/server/fixed-prefix-probe-*.json 与 scripts/rust-tauri/r00-t06-probe-fixed-prefix.mjs。",
          streamPhaseFast: {
            requestBytes: { ...stats(mainChatFastRows.map((r) => r.requestBytes)), unit: "bytes" },
            messageCount: { ...stats(mainChatFastRows.map((r) => r.messageCount)), unit: "count" },
            toolCount: { ...stats(mainChatFastRows.map((r) => r.toolCount)), unit: "count" },
            n: mainChatFastRows.length,
          },
          cancelPhaseHang: {
            requestBytes: { ...stats(mainChatHangRows.map((r) => r.requestBytes)), unit: "bytes" },
            messageCount: { ...stats(mainChatHangRows.map((r) => r.messageCount)), unit: "count" },
            toolCount: { ...stats(mainChatHangRows.map((r) => r.toolCount)), unit: "count" },
            n: mainChatHangRows.length,
            note: "hang 模式同一请求在停住与客户端断开时各记一次，按 seq 去重后统计",
          },
          probeDecomposition: {
            systemMessageBytes: 10626,
            toolsJsonBytes: 10287,
            toolCount: 7,
            source: "fixed-prefix-probe（真实会话路径实测，与 cache contract 诊断一致）",
          },
        },
      },
      realProviderMeasurements: { calls: 0, note: "本轮零真实供应商调用；真实供应商测量待授权后单列" },
    },
    rawData: {
      serverDir: "artifacts/rust-tauri/R00/T06/raw/server",
      desktopDir: "artifacts/rust-tauri/R00/T06/raw/desktop",
      buildSummary: "artifacts/rust-tauri/R00/T06/raw/build-release-summary.json",
      serverSummary: serverSummaryPath ? path.relative(ROOT, serverSummaryPath) : null,
      desktopSummary: desktopSummaryPath ? path.relative(ROOT, desktopSummaryPath) : null,
    },
  };

  const out = path.join(T06_DIR, "BASELINE_BENCHMARK.json");
  writeJson(out, baseline);
  console.log(JSON.stringify({ ok: true, out: path.relative(ROOT, out) }, null, 2));
}

main();
