#!/usr/bin/env node
/**
 * R00-T06｜补充：office 插件 PDF 链路（真实产品链，A11 证据补强）
 *
 * 首轮桌面腿的 PDF helper 由 harness 直接以产品同一入口 spawn；本脚本补测
 * **完整产品链**：WS prompt → 模型替身 tool_use → mcp_call 延迟目录 →
 * office_html-to-pdf 插件工具 → server spawn 打包二进制 helper（真实
 * Chromium printToPDF）→ SessionFile 注册 → tool 结果回传。
 *
 * 测量：prompt→tool_end（含 helper spawn+转换全程）、输出 %PDF 校验、
 * 首样本转换期间 server 进程树采样（helper 应以 server 子进程出现——A11：
 * 主程序与全部辅助进程同窗计入）。
 *
 * 用法：node scripts/rust-tauri/r00-t06-bench-office-pdf.mjs [--samples 32]
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import WebSocket from "ws";
import {
  collectHostInfo, isoNow, nowMs, sleep, stats, snapshotTree, sampleTreeWindow,
  waitForTreeExit, sigkillTree, writeJson, appendJsonl,
} from "./r00-t06-lib.mjs";
import { startStubProvider } from "./r00-t06-stub-provider.mjs";
import { buildPristineHome, cloneHome } from "./r00-t06-fixture.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..", "..");
const PLATFORM_ARCH = `${process.platform === "darwin" ? "mac" : process.platform}-${process.arch}`;
const SERVER_WRAPPER = path.join(ROOT, "dist-server", PLATFORM_ARCH, "hana-server");
const APP_BIN = path.join(ROOT, "dist", `mac-${process.arch}`, "Lingxi.app", "Contents", "MacOS", "Lingxi");
const RENDERER_DIST = path.join(ROOT, "desktop", "dist-renderer");
const OUT_DIR = path.join(ROOT, "artifacts", "rust-tauri", "R00", "T06", "raw", "server");
const TOKEN = "r00t06-not-a-secret";

const argv = process.argv.slice(2);
const SAMPLES = Number(argv.includes("--samples") ? argv[argv.indexOf("--samples") + 1] : "32");
const RUN_ID = `office-${new Date().toISOString().replace(/[:.]/g, "-")}`;
fs.mkdirSync(OUT_DIR, { recursive: true });

// ~200KB 确定性 HTML（与桌面腿 PDF 负载同规模）
const unit = "<p>灵犀 PDF 基准 the quick brown fox 0123456789</p>\n";
const target = 200 * 1024;
const BENCH_HTML = `<!doctype html><html><head><meta charset="utf-8"><title>bench</title></head><body><h1>bench-office-pdf</h1>${unit.repeat(Math.ceil(target / unit.length))}</body></html>`;

async function waitReady(home, child, { timeoutMs = 60000 } = {}) {
  const infoPath = path.join(home, "server-info.json");
  const t0 = nowMs();
  let info = null;
  while (nowMs() - t0 < timeoutMs) {
    try {
      const p = JSON.parse(fs.readFileSync(infoPath, "utf8"));
      if (p?.port && p.pid === child.pid) info = p;
    } catch {}
    if (info) {
      try {
        const r = await fetch(`http://127.0.0.1:${info.port}/api/health`, { headers: { Authorization: `Bearer ${TOKEN}` }, signal: AbortSignal.timeout(2000) });
        if (r.ok && (await r.json().catch(() => null))?.status === "ok") return info;
      } catch {}
    }
    await sleep(25);
  }
  throw new Error("server not ready");
}

async function main() {
  if (!fs.existsSync(APP_BIN)) throw new Error(`missing packaged app: ${APP_BIN}`);
  const stub = await startStubProvider({ journalPath: path.join(OUT_DIR, `stub-journal-${RUN_ID}.jsonl`) });
  const pristine = path.join(OUT_DIR, "pristine-home-office");
  fs.rmSync(pristine, { recursive: true, force: true });
  buildPristineHome({ dest: pristine, stubBaseUrl: stub.baseUrl });
  const home = cloneHome(pristine, "office");

  // server env：产品合同同构——office 插件经 LINGXI_OFFICE_PDF_HELPER_EXEC 找
  // helper 二进制，helper 需要 LINGXI_RENDERER_DIST（字体 CSS）。
  const server = spawn(SERVER_WRAPPER, [], {
    cwd: path.dirname(SERVER_WRAPPER),
    env: {
      PATH: [path.join(path.dirname(SERVER_WRAPPER), "node"), path.dirname(SERVER_WRAPPER), "/usr/bin:/bin:/usr/sbin:/sbin"].join(":"),
      HOME: home,
      LINGXI_HOME: home,
      LINGXI_PORT: "0",
      LINGXI_TOKEN: TOKEN,
      LINGXI_CREATE_STARTUP_SESSION: "0",
      LINGXI_OFFICE_PDF_HELPER_EXEC: APP_BIN,
      LINGXI_RENDERER_DIST: RENDERER_DIST,
      TMPDIR: os.tmpdir(),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logs = [];
  server.stdout.on("data", (c) => logs.push(c.toString()));
  server.stderr.on("data", (c) => logs.push(c.toString()));

  const info = await waitReady(home, server);
  const port = info.port;
  console.log(`[live] pid=${server.pid} port=${port}`);
  await fetch(stub.controlUrl("fast"));
  // 旧系统事实：POST /api/sessions/new 的 permissionMode 不落到会话（实测仍
  // auto）。产品路径是渲染端调 POST /api/plan-mode 切模式——office 工具为静态
  // review 权限，auto 下走自动审批评审器（需辅助模型，未配置则 fail-closed）。
  // 这里按产品同一路径切到 operate（classifyDeclaredToolPermission 对 operate
  // 放行），与桌面端用户打开 operate 开关等价。
  const pm = await fetch(`http://127.0.0.1:${port}/api/plan-mode`, {
    method: "POST",
    headers: { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" },
    body: JSON.stringify({ mode: "operate" }),
  });
  const pmBody = await pm.json().catch(() => null);
  console.log(`[plan-mode] status=${pm.status} mode=${pmBody?.mode}`);
  if (pmBody?.mode !== "operate") throw new Error(`failed to set operate mode: ${JSON.stringify(pmBody)}`);

  const rawPath = path.join(OUT_DIR, `office-pdf-${RUN_ID}.jsonl`);
  const rows = [];
  let treeDuringConvert = null;

  const pdfDir = path.join(home, "plugin-data", "office", "generated");
  for (let i = 0; i < SAMPLES; i++) {
    const before = new Set((() => { try { return fs.readdirSync(pdfDir); } catch { return []; } })());
    const sres = await fetch(`http://127.0.0.1:${port}/api/sessions/new`, {
      method: "POST",
      headers: { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" },
      body: JSON.stringify({ memoryEnabled: false, permissionMode: "operate" }),
    });
    if (!sres.ok) throw new Error(`sessions/new ${sres.status}`);
    const s = await sres.json();

    const events = [];
    const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`, { headers: { Authorization: `Bearer ${TOKEN}` } });
    await new Promise((r, j) => { ws.once("open", r); ws.once("error", j); setTimeout(() => j(new Error("ws open timeout")), 15000); });
    ws.on("message", (raw) => { try { const e = JSON.parse(raw.toString()); e.__t = nowMs(); events.push(e); } catch {} });

    const directive = { tool: "office_html-to-pdf", arguments: { html: BENCH_HTML, filename: `bench-office-${i}.pdf` } };
    const t0 = nowMs();
    ws.send(JSON.stringify({ type: "prompt", text: `bench-office-${i} TOOL:mcp_call:${JSON.stringify(directive)}`, sessionPath: s.path }));

    // 首样本：转换期间并行采样 server 树（helper 应作为子进程出现）
    let sampling = null;
    if (i === 0) {
      sampling = sampleTreeWindow([server.pid], { samples: 8, intervalMs: 300, label: "office-convert" })
        .then((w) => { treeDuringConvert = w; });
    }

    const result = await new Promise((resolve) => {
      const iv = setInterval(() => {
        const end = events.find((e) => (e.type === "assistant_run_end" || e.type === "turn_end" || e.type === "agent_settled") && e.__t > t0);
        if (end) {
          clearInterval(iv);
          const toolStart = events.find((e) => e.type === "tool_start" && e.__t > t0);
          const toolEnd = events.find((e) => e.type === "tool_end" && e.__t > t0);
          resolve({
            toolStartMs: toolStart ? toolStart.__t - t0 : null,
            toolEndMs: toolEnd ? toolEnd.__t - t0 : null,
            runEndMs: end.__t - t0,
            toolStatus: toolEnd?.status ?? null,
            toolError: toolEnd?.error ?? null,
            outputDetail: toolEnd?.details?.office ?? null,
            eventTypes: [...new Set(events.map((e) => e.type))],
          });
        }
      }, 5);
      setTimeout(() => { clearInterval(iv); resolve({ timeout: true, eventTypes: [...new Set(events.map((e) => e.type))] }); }, 120000);
    });
    if (sampling) await sampling;
    try { ws.close(); } catch {}

    // 输出校验：tool_end 事件不带输出明细；PDF 落在 plugin-data/office/generated，
    // 以「本样本新增文件」核对 %PDF 头与字节数。
    let okPdf = false; let pdfBytes = 0; let outputPath = null;
    try {
      const after = fs.readdirSync(pdfDir).filter((f) => !before.has(f));
      if (after.length === 1) {
        outputPath = path.join(pdfDir, after[0]);
        const buf = fs.readFileSync(outputPath);
        pdfBytes = buf.length;
        okPdf = buf.subarray(0, 5).toString("utf8") === "%PDF-";
      }
    } catch {}
    const row = {
      phase: "office-pdf-chain", i, ts: isoNow(), marker: `bench-office-${i}`,
      sessionPath: s.path, ...result, okPdf, pdfBytes, outputPath,
    };
    rows.push(row);
    appendJsonl(rawPath, row);
    if (result.timeout || !okPdf) {
      console.error(`[FAIL][office#${i}]`, JSON.stringify({ ...result, okPdf, pdfBytes }).slice(0, 400), "srvTail:", logs.join("").slice(-400));
      break;
    }
    await sleep(250);
  }

  // 退出清理
  const t0 = nowMs();
  try { server.kill("SIGTERM"); } catch {}
  const exitMs = await waitForTreeExit(server.pid, 20000, 50);
  if (exitMs === null) sigkillTree(server.pid);

  const summary = {
    runId: RUN_ID,
    task: "R00-T06 supplementary office PDF chain",
    head: collectHostInfo({ repoRoot: ROOT }).gitHead,
    samples: SAMPLES,
    boundary: "真实产品链：WS→mcp_call→office_html-to-pdf→server spawn 打包二进制 helper（真实 printToPDF）；输入 200KB 确定性 HTML；helper exec 经 LINGXI_OFFICE_PDF_HELPER_EXEC 指向打包 Lingxi（与产品内 server→helper 合同一致）",
    htmlBytes: Buffer.byteLength(BENCH_HTML),
    phases: {
      office_pdf_chain: {
        toolEndMs: stats(rows.map((r) => r.toolEndMs).filter(Number.isFinite)),
        runEndMs: stats(rows.map((r) => r.runEndMs).filter(Number.isFinite)),
        pdfBytes: stats(rows.map((r) => r.pdfBytes).filter(Number.isFinite)),
        okPdf: rows.filter((r) => r.okPdf).length,
        errors: rows.filter((r) => r.timeout || !r.okPdf).length,
        treeDuringFirstConvert: treeDuringConvert ? {
          procCountByFrame: treeDuringConvert.procCountByFrame,
          rssSeriesKb: treeDuringConvert.rssSeriesKb,
          procsSeen: [...new Set(treeDuringConvert.frames.flatMap((f) => f.procs.map((p) => `${p.pid}:${p.comm}`)))],
          raw: "（内嵌本 summary）",
        } : null,
        raw: path.relative(ROOT, rawPath),
      },
    },
    shutdown: { sigtermTreeExitMs: exitMs },
  };
  writeJson(path.join(OUT_DIR, `summary-${RUN_ID}.json`), summary);
  fs.rmSync(pristine, { recursive: true, force: true });
  fs.rmSync(home, { recursive: true, force: true });
  const bad = summary.phases.office_pdf_chain.errors;
  console.log(JSON.stringify({ ok: bad === 0, errors: bad, toolEnd: summary.phases.office_pdf_chain.toolEndMs, treeProcs: summary.phases.office_pdf_chain.treeDuringFirstConvert?.procCountByFrame }, null, 2));
  process.exit(bad === 0 ? 0 : 1);
}

main().catch((err) => { console.error(err); process.exit(1); });
