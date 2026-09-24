#!/usr/bin/env node
/**
 * R00-T06｜旧系统（Electron+Node）桌面腿基准
 *
 * 被测对象：electron-builder --dir 打包的 Lingxi.app（发布优化：asar + 生产
 * bundle + ad-hoc 测试签名；非可发布安装包，见协议的构建模式边界）。
 *
 * 关键真实路径：
 * - 启动 = 打包入口 → bootstrap → main.bundle.cjs → prepareArtifactBoot
 *   （首启解包 seed 到 LINGXI_HOME/artifacts）→ spawn 生产 server → 窗口
 *   → renderer init → IPC app-ready（诊断日志 renderer.log 追加，外部可观察）。
 * - 浏览器多实例 = agent 工具链：WS prompt → 模型替身返回 tool_use
 *   （browser start / navigate）→ BrowserManager → /internal/browser
 *   browser-cmd → Electron 真实 WebContentsView 渲染本地确定性页面。
 * - PDF 转换 = 打包二进制自身以 --hana-office-html-to-pdf helper 模式执行
 *   真实 Chromium printToPDF（与 office 插件 spawn 的是同一入口/文件）。
 * - 进程树口径：每次采样从 Electron 主进程 PID + server PID 双根重走树，
 *   主进程、GPU/网络/utility、全部 renderer（含浏览器视图）、server 及其
 *   子进程同窗口计入（R00-A11）。
 *
 * 阶段：template（首启解包，单独记录，不入稳态系列）、startup.cold/warm、
 * idle.memory、browser.multi（5 实例）、pdf.convert、distribution.size。
 *
 * 用法：node scripts/rust-tauri/r00-t06-bench-desktop.mjs [--samples 32] [--phases ...]
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import WebSocket from "ws";
import {
  collectHostInfo, isoNow, nowMs, sleep, stats, sampleTreeWindow,
  snapshotTree, waitForPidExit, waitForTreeExit, sigkillTree, writeJson, appendJsonl,
} from "./r00-t06-lib.mjs";
import { startStubProvider } from "./r00-t06-stub-provider.mjs";
import { buildPristineHome } from "./r00-t06-fixture.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..", "..");
const ARCH = process.arch;
const APP_BIN = path.join(ROOT, "dist", `mac-${ARCH}`, "Lingxi.app", "Contents", "MacOS", "Lingxi");
const OUT_DIR = path.join(ROOT, "artifacts", "rust-tauri", "R00", "T06", "raw", "desktop");
const TOKEN_HINT = null; // 桌面腿 token 由 Electron 主进程持有；我们经 server-info.json 读取

const argv = process.argv.slice(2);
function argVal(name, dflt) {
  const i = argv.indexOf(`--${name}`);
  return i >= 0 ? argv[i + 1] : dflt;
}
const SAMPLES = Number(argVal("samples", "32"));
const PHASES = (argVal("phases", "template,startup,idle,browser,pdf,size") || "").split(",");

fs.mkdirSync(OUT_DIR, { recursive: true });
const RUN_ID = `run-${new Date().toISOString().replace(/[:.]/g, "-")}`;
const failures = [];
let live = null;
let port = null;
let token = null;
function fail(phase, err) {
  failures.push(`${phase}: ${err?.stack || err}`);
  console.error(`[FAIL][${phase}] ${err?.stack || err}`);
}

const summary = {
  runId: RUN_ID,
  task: "R00-T06 desktop leg",
  head: null,
  host: collectHostInfo({ repoRoot: ROOT }),
  appBin: path.relative(ROOT, APP_BIN),
  samples: SAMPLES,
  phases: {},
};

// ── 应用生命周期 ──
function appEnv(homePath) {
  return {
    PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
    HOME: homePath,
    LINGXI_HOME: homePath,
    // 更新检查指向本地死端口：产品自带 env 开关，避免任何真实外呼
    LINGXI_UPDATE_FEED_URL: "http://127.0.0.1:9/feed/",
    TMPDIR: os.tmpdir(),
  };
}

function launchApp(homePath) {
  const child = spawn(APP_BIN, [], {
    env: appEnv(homePath),
    stdio: ["ignore", "pipe", "pipe"],
  });
  const logs = [];
  child.stdout?.on("data", (c) => logs.push(c.toString()));
  child.stderr?.on("data", (c) => logs.push(c.toString()));
  return { child, pid: child.pid, homePath, logs };
}

/** 读 renderer.log：以 details.pid==本次进程 的 desktop-launch-start 为锚，等其后的 app-ready。 */
async function waitAppReady(entry, { timeoutMs = 90000, pollMs = 25 } = {}) {
  const logPath = path.join(entry.homePath, "diagnostics", "desktop-launch", "renderer.log");
  const t0 = nowMs();
  let startSeenAt = null;
  const deadline = t0 + timeoutMs;
  while (nowMs() < deadline) {
    let text = "";
    try { text = fs.readFileSync(logPath, "utf8"); } catch {}
    if (startSeenAt === null) {
      // 模板克隆会带上一次的 renderer.log（含旧 app-ready）——必须锚定本次
      // 进程 pid 的 desktop-launch-start（每次启动 reset 重写该文件）。
      for (const line of text.split("\n")) {
        try {
          const ev = JSON.parse(line);
          if (ev.event === "desktop-launch-start" && ev.details?.pid === entry.pid) {
            startSeenAt = nowMs() - t0;
            break;
          }
        } catch {}
      }
    }
    if (startSeenAt !== null && text.includes('"app-ready"')) {
      return { tLaunchStartLineMs: startSeenAt, tAppReadyMs: nowMs() - t0 };
    }
    await sleep(pollMs);
  }
  throw new Error(`app-ready not seen in ${timeoutMs}ms; logTail=${entry.logs.join("").slice(-1500)}`);
}

async function readServerInfo(homePath, { timeoutMs = 60000 } = {}) {
  const p = path.join(homePath, "server-info.json");
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const info = JSON.parse(fs.readFileSync(p, "utf8"));
      if (info?.port && info?.pid) return info;
    } catch {}
    await sleep(50);
  }
  throw new Error("server-info.json not ready");
}

/** 退出 App：SIGTERM 主进程 + 等主树与 desktop-owned server 都消失。 */
async function quitApp(entry, { maxWaitMs = 30000 } = {}) {
  let serverPid = null;
  try {
    const info = JSON.parse(fs.readFileSync(path.join(entry.homePath, "server-info.json"), "utf8"));
    serverPid = info?.pid ?? null;
  } catch {}
  // SIGTERM 前捕获整树（主进程 helper + server）——主进程退出后后代被 reparent，
  // 事后按根遍历会漏掉残留 helper，改为核对「捕获到的全部 PID 消失」。
  const treeBefore = snapshotTree([entry.pid, ...(serverPid ? [serverPid] : [])], "quit-before");
  const watchedPids = treeBefore.procs.map((p) => p.pid);
  const t0 = nowMs();
  try { entry.child.kill("SIGTERM"); } catch {}
  async function waitAllGone(ms) {
    const deadline = nowMs() + ms;
    for (;;) {
      const alive = watchedPids.filter((p) => { try { process.kill(p, 0); return true; } catch { return false; } });
      if (!alive.length) return true;
      if (nowMs() >= deadline) return false;
      await sleep(50);
    }
  }
  let allGone = await waitAllGone(maxWaitMs);
  let force = false;
  if (!allGone) {
    force = true;
    for (const p of watchedPids) { try { process.kill(p, "SIGKILL"); } catch {} }
    allGone = await waitAllGone(10000);
  }
  const leftover = watchedPids.filter((p) => { try { process.kill(p, 0); return true; } catch { return false; } });
  return {
    watchedPidCount: watchedPids.length,
    teardownAllGoneMs: allGone ? nowMs() - t0 : null,
    forced: force,
    leftoverMainPids: leftover,
    leftoverServerPids: [],
  };
}

// ── server REST / WS（浏览器负载用） ──
async function api(port, token, pathname, init = {}) {
  return fetch(`http://127.0.0.1:${port}${pathname}`, {
    ...init,
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json", ...(init.headers || {}) },
    signal: AbortSignal.timeout(30000),
  });
}

function wsChat({ port, token, sessionPath }) {
  return new Promise((resolve, reject) => {
    const events = [];
    const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`, { headers: { Authorization: `Bearer ${token}` } });
    const t = { open: null };
    const to = setTimeout(() => { try { ws.close(); } catch {}; reject(new Error("ws open timeout")); }, 15000);
    ws.on("open", () => { clearTimeout(to); t.open = nowMs(); resolve({ ws, events, t }); });
    ws.on("message", (raw) => {
      try { const ev = JSON.parse(raw.toString()); ev.__recvMs = nowMs(); events.push(ev); } catch {}
    });
    ws.on("error", () => {});
  });
}

async function driveToolTurn({ port, token, sessionPath, text, timeoutMs = 90000 }) {
  const chat = await wsChat({ port, token, sessionPath });
  const t0 = nowMs();
  chat.ws.send(JSON.stringify({ type: "prompt", text, sessionPath }));
  const result = await new Promise((resolve) => {
    const iv = setInterval(() => {
      const end = chat.events.find((e) => (e.type === "assistant_run_end" || e.type === "turn_end" || e.type === "agent_settled") && e.__recvMs > t0);
      if (end) {
        clearInterval(iv);
        const toolStart = chat.events.find((e) => e.type === "tool_start" && e.__recvMs > t0);
        const toolEnd = chat.events.find((e) => e.type === "tool_end" && e.__recvMs > t0);
        const errs = chat.events.filter((e) => /error/i.test(e.type || ""));
        resolve({
          toolStartMs: toolStart ? toolStart.__recvMs - t0 : null,
          toolEndMs: toolEnd ? toolEnd.__recvMs - t0 : null,
          runEndMs: end.__recvMs - t0,
          toolNames: chat.events.filter((e) => e.type === "tool_start").map((e) => e.tool || e.name || e.toolName).filter(Boolean),
          errorEvents: errs.slice(0, 5).map((e) => `${e.type}:${String(e.message || e.error || "").slice(0, 160)}`),
          eventTypes: [...new Set(chat.events.map((e) => e.type))],
        });
      }
    }, 5);
    setTimeout(() => { clearInterval(iv); resolve({ timeout: true, eventTypes: [...new Set(chat.events.map((e) => e.type))] }); }, timeoutMs);
  });
  try { chat.ws.close(); } catch {}
  return result;
}

/** APFS clonefile 复制稳态模板（每样本一致的磁盘状态，近乎零拷贝成本）。 */
function cloneTemplate(templateDir, tag) {
  const dest = fs.mkdtempSync(path.join(os.tmpdir(), `lingxi-r00t06-dt-${tag}-`));
  const res = spawnSync("cp", ["-Rc", `${templateDir}/.`, dest], { encoding: "utf8" });
  if (res.status !== 0) {
    // 文件系统不支持 clonefile 时退回普通复制
    const res2 = spawnSync("cp", ["-R", `${templateDir}/.`, dest], { encoding: "utf8" });
    if (res2.status !== 0) throw new Error(`cloneTemplate failed: ${res2.stderr || res.stderr}`);
  }
  return dest;
}

async function main() {
  summary.head = summary.host.gitHead;
  if (!fs.existsSync(APP_BIN)) {
    throw new Error(`missing packaged app: ${APP_BIN}（先跑 r00-t06-build-release.mjs）`);
  }

  // 固定端口：稳态模板的 provider-catalog 在 template 阶段写入 stub 地址，
  // 后续运行的 stub 必须监听同一端口（随机端口会让模板指向失效地址）。
  const STUB_PORT = Number(argVal("stub-port", "47613"));
  let stub;
  try {
    stub = await startStubProvider({ port: STUB_PORT, journalPath: path.join(OUT_DIR, `stub-journal-${RUN_ID}.jsonl`) });
  } catch (err) {
    throw new Error(`stub 固定端口 ${STUB_PORT} 绑定失败：${err?.message}（清理占用后重试）`);
  }
  summary.stub = { baseUrl: stub.baseUrl, fixedPort: STUB_PORT };

  // pristine fixture（无 artifacts；首启会解包 seed）
  const pristine = path.join(OUT_DIR, "pristine-home");
  fs.rmSync(pristine, { recursive: true, force: true });
  buildPristineHome({ dest: pristine, stubBaseUrl: stub.baseUrl });

  const rawStartup = path.join(OUT_DIR, `startup-${RUN_ID}.jsonl`);
  const rawBrowser = path.join(OUT_DIR, `browser-${RUN_ID}.jsonl`);
  const rawPdf = path.join(OUT_DIR, `pdf-${RUN_ID}.jsonl`);

  let templateDir = null;

  // ══ 阶段：template（真实首启：解包 seed + 首次 artifact boot） ══
  if (PHASES.includes("template")) {
    const home = cloneTemplate(pristine, "firstboot");
    const entry = launchApp(home);
    try {
      const ready = await waitAppReady(entry, { timeoutMs: 180000 });
      const info = await readServerInfo(home);
      const artifactsDir = path.join(home, "artifacts");
      let artifactsBytes = 0;
      const du = spawnSync("du", ["-sk", artifactsDir], { encoding: "utf8" });
      if (du.status === 0) artifactsBytes = Number(du.stdout.trim().split(/\s+/)[0]) * 1024;
      summary.phases.first_boot_template = {
        tLaunchStartLineMs: ready.tLaunchStartLineMs,
        tAppReadyMs: ready.tAppReadyMs,
        serverPid: info.pid,
        artifactsUnpackedBytes: artifactsBytes,
        note: "首启解包 seed 计时单独记录，不计入稳态启动系列",
      };
      await sleep(5000); // 让首启后的后台任务安定
      const q = await quitApp(entry);
      summary.phases.first_boot_template.quit = q;
      if (q.leftoverMainPids.length || q.leftoverServerPids.length) {
        fail("template.quit", `leftover pids: ${q.leftoverMainPids}/${q.leftoverServerPids}`);
      }
      templateDir = path.join(OUT_DIR, "steady-template-home");
      fs.rmSync(templateDir, { recursive: true, force: true });
      fs.mkdirSync(path.dirname(templateDir), { recursive: true });
      const cp = spawnSync("cp", ["-Rc", `${home}/.`, templateDir], { encoding: "utf8" });
      if (cp.status !== 0) {
        const cp2 = spawnSync("cp", ["-R", `${home}/.`, templateDir], { encoding: "utf8" });
        if (cp2.status !== 0) throw new Error(`steady template copy failed: ${cp2.stderr}`);
      }
      console.log(`[template] firstBootAppReady=${ready.tAppReadyMs}ms; steady template ready`);
    } catch (err) {
      await quitApp(entry).catch(() => sigkillTree(entry.pid));
      fail("template", err);
    } finally {
      fs.rmSync(home, { recursive: true, force: true });
    }
  } else {
    templateDir = path.join(OUT_DIR, "steady-template-home");
    if (!fs.existsSync(templateDir)) throw new Error("missing steady template; run --phases template first");
  }

  // ══ 阶段：startup（cold 5s / warm ≤0.5s） ══
  if (PHASES.includes("startup")) {
    for (const kind of ["cold", "warm"]) {
      const rows = [];
      for (let i = 0; i < SAMPLES; i++) {
        const home = cloneTemplate(templateDir, kind);
        const entry = launchApp(home);
        let row;
        try {
          const ready = await waitAppReady(entry);
          const info = await readServerInfo(home);
          const q = await quitApp(entry);
          row = {
            phase: `startup.${kind}`, i, ts: isoNow(),
            t_launch_start_line_ms: ready.tLaunchStartLineMs,
            t_app_ready_ms: ready.tAppReadyMs,
            server_pid: info.pid,
            quit: q,
          };
          if (q.leftoverMainPids.length || q.leftoverServerPids.length) {
            row.error = `leftover processes after quit: main=${q.leftoverMainPids} server=${q.leftoverServerPids}`;
          }
        } catch (err) {
          await quitApp(entry).catch(() => sigkillTree(entry.pid));
          row = { phase: `startup.${kind}`, i, ts: isoNow(), error: String(err?.message || err).slice(0, 500) };
        }
        rows.push(row);
        appendJsonl(rawStartup, row);
        fs.rmSync(home, { recursive: true, force: true });
        if (row.error) { fail(`startup.${kind}#${i}`, row.error); break; }
        await sleep(kind === "cold" ? 5000 : 400);
      }
      summary.phases[`startup_${kind}`] = {
        t_app_ready_ms: stats(rows.map((r) => r.t_app_ready_ms).filter(Number.isFinite)),
        t_launch_start_line_ms: stats(rows.map((r) => r.t_launch_start_line_ms).filter(Number.isFinite)),
        quit_tree_all_gone_ms: stats(rows.map((r) => r.quit?.teardownAllGoneMs).filter(Number.isFinite)),
        errors: rows.filter((r) => r.error).length,
        raw: path.relative(ROOT, rawStartup),
      };
      console.log(`[startup.${kind}] appReady=${JSON.stringify(summary.phases[`startup_${kind}`].t_app_ready_ms)}`);
    }
  }

  // 长驻实例（live/port/token 在模块层声明，供 exit 兜底清理）
  async function ensureLive() {
    if (live) return;
    const home = cloneTemplate(templateDir, "live");
    live = launchApp(home);
    await waitAppReady(live);
    const info = await readServerInfo(home);
    port = info.port;
    token = info.token;
    live.serverPid = info.pid;
    console.log(`[live] app pid=${live.pid} server pid=${info.pid} port=${port}`);
  }

  // ══ 阶段：idle memory（app + server 双根树） ══
  if (PHASES.includes("idle")) {
    try {
      await ensureLive();
      await sleep(10000);
      const win = await sampleTreeWindow([live.pid, live.serverPid], { samples: 10, intervalMs: 500, label: "desktop-idle" });
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

  // ══ 阶段：浏览器多实例（真实 agent 工具链 → WebContentsView） ══
  if (PHASES.includes("browser")) {
    try {
      await ensureLive();
      const rows = [];
      const SESSIONS = 5; // BrowserManager MAX_INSTANCES
      // 5 个会话 × (start + navigate 本地页) —— 全部走真实 WS/工具/权限/命令链路
      for (let i = 0; i < SESSIONS; i++) {
        const created = await api(port, token, "/api/sessions/new", {
          method: "POST",
          body: JSON.stringify({ memoryEnabled: false, permissionMode: "operate" }),
        });
        if (!created.ok) throw new Error(`sessions/new ${created.status}: ${await created.text()}`);
        const s = await created.json();
        const startRes = await driveToolTurn({
          port, token, sessionPath: s.path,
          text: `bench-browser-start-${i} TOOL:mcp_call:${JSON.stringify({ tool: "browser", arguments: { action: "start" } })}`,
        });
        const navRes = await driveToolTurn({
          port, token, sessionPath: s.path,
          text: `bench-browser-nav-${i} TOOL:mcp_call:${JSON.stringify({ tool: "browser", arguments: { action: "navigate", url: stub.pageUrl(`p${i}`) } })}`,
        });
        const row = {
          phase: "browser", i, ts: isoNow(), sessionPath: s.path,
          start: startRes, navigate: navRes,
          stubPagesServed: (await (await fetch(stub.pageUrl("probe"))).text()).length > 0 ? undefined : 0,
        };
        rows.push(row);
        appendJsonl(rawBrowser, row);
        if (startRes.timeout || navRes.timeout || (navRes.errorEvents || []).length) {
          fail(`browser#${i}`, JSON.stringify({ startRes, navRes }).slice(0, 400));
        }
        await sleep(1000);
      }
      // 视图稳定后采样整树（A11）
      await sleep(8000);
      const win = await sampleTreeWindow([live.pid, live.serverPid], { samples: 10, intervalMs: 500, label: "desktop-browser-5views" });
      summary.phases.browser_multi = {
        sessions: SESSIONS,
        pages: Array.from({ length: SESSIONS }, (_, i) => stub.pageUrl(`p${i}`)),
        startToolEndMs: stats(rows.map((r) => r.start.toolEndMs).filter(Number.isFinite)),
        navigateToolEndMs: stats(rows.map((r) => r.navigate.toolEndMs).filter(Number.isFinite)),
        memoryWithViews: {
          rssMedianKb: win.rssMedianKb,
          rssMaxKb: win.rssMaxKb,
          procCountByFrame: win.procCountByFrame,
        },
        raw: path.relative(ROOT, path.join(OUT_DIR, `browser-tree-${RUN_ID}.json`)),
        rawRows: path.relative(ROOT, rawBrowser),
      };
      writeJson(path.join(OUT_DIR, `browser-tree-${RUN_ID}.json`), win);
      console.log(`[browser] navigate=${JSON.stringify(summary.phases.browser_multi.navigateToolEndMs)} rss=${win.rssMedianKb}KB`);
    } catch (err) { fail("browser", err); }
  }

  // ══ 阶段：PDF 转换（打包二进制 helper 模式，真实 printToPDF） ══
  if (PHASES.includes("pdf")) {
    try {
      // 确定性 HTML：目标 200KB（按 unit 字符数取整 + 中文多字节），
      // 实际字节数固定为 239,073B（协议 §8 已如实注明，R1-F06）
      const unit = "<p>灵犀 PDF 基准 the quick brown fox 0123456789</p>\n";
      const target = 200 * 1024;
      const html = `<!doctype html><html><head><meta charset="utf-8"><title>bench</title></head><body><h1>bench-pdf</h1>${unit.repeat(Math.ceil(target / unit.length))}</body></html>`;
      const rows = [];
      for (let i = 0; i < SAMPLES; i++) {
        const work = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-r00t06-pdf-"));
        const htmlPath = path.join(work, "bench.html");
        const outPath = path.join(work, "bench.pdf");
        const jobPath = path.join(work, "job.json");
        fs.writeFileSync(htmlPath, html);
        fs.writeFileSync(jobPath, JSON.stringify({ htmlPath, outputPath: outPath, pageSize: "A4", printBackground: true }));
        const t0 = nowMs();
        const res = spawnSync(APP_BIN, ["--hana-office-html-to-pdf", jobPath], {
          encoding: "buffer",
          timeout: 120000,
          env: {
            PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
            HOME: work,
            TMPDIR: os.tmpdir(),
            LINGXI_UPDATE_FEED_URL: "http://127.0.0.1:9/feed/",
            // 与产品链路同源：server（office 插件）spawn helper 时注入激活
            // renderer 目录（main.cjs → LINGXI_RENDERER_DIST → helper env）。
            LINGXI_RENDERER_DIST: path.join(ROOT, "desktop", "dist-renderer"),
          },
        });
        const durMs = nowMs() - t0;
        let okPdf = false; let pdfBytes = 0;
        try {
          const buf = fs.readFileSync(outPath);
          pdfBytes = buf.length;
          okPdf = buf.subarray(0, 5).toString("utf8") === "%PDF-";
        } catch {}
        const row = {
          phase: "pdf", i, ts: isoNow(),
          spawnToExitMs: durMs,
          exitStatus: res.status,
          okPdf, pdfBytes,
          stderrHead: res.stderr ? res.stderr.subarray(0, 300).toString("utf8") : "",
        };
        rows.push(row);
        appendJsonl(rawPdf, row);
        fs.rmSync(work, { recursive: true, force: true });
        if (res.status !== 0 || !okPdf) { fail(`pdf#${i}`, `status=${res.status} okPdf=${okPdf} stderr=${row.stderrHead}`); break; }
      }
      // 一次采样中的 helper RSS（转换中途快照）
      summary.phases.pdf_convert = {
        spawnToExitMs: stats(rows.map((r) => r.spawnToExitMs).filter(Number.isFinite)),
        pdfBytes: stats(rows.map((r) => r.pdfBytes).filter(Number.isFinite)),
        errors: rows.filter((r) => !r.okPdf).length,
        raw: path.relative(ROOT, rawPdf),
      };
      console.log(`[pdf] ${JSON.stringify(summary.phases.pdf_convert.spawnToExitMs)}`);
    } catch (err) { fail("pdf", err); }
  }

  // ══ 阶段：分发占用（du -sk 分配字节，协议 §9 冻结口径） ══
  if (PHASES.includes("size")) {
    try {
      // dist-server / dist-server-artifact 与 dist 同用 darwin→mac 目录约定（R1-F02）
      const OS_DIR = process.platform === "darwin" ? "mac" : process.platform;
      const sizes = {};
      for (const [key, p] of [
        ["lingxiApp", path.join(ROOT, "dist", `${OS_DIR}-${ARCH}`, "Lingxi.app")],
        ["serverBundle", path.join(ROOT, "dist-server", `${OS_DIR}-${ARCH}`)],
        ["seedArtifactDir", path.join(ROOT, "dist-server-artifact", `${OS_DIR}-${ARCH}`)],
      ]) {
        if (!fs.existsSync(p)) {
          // 不再静默省略：产物缺失本身就是要暴露的失败
          sizes[key] = { path: path.relative(ROOT, p), error: "missing" };
          fail("size", `missing artifact dir: ${p}`);
          continue;
        }
        const du = spawnSync("du", ["-sk", p], { encoding: "utf8" });
        if (du.status !== 0) {
          sizes[key] = { path: path.relative(ROOT, p), error: `du exit ${du.status}: ${(du.stderr || "").trim()}` };
          fail("size", `du failed for ${p}: ${(du.stderr || "").trim()}`);
          continue;
        }
        sizes[key] = { path: path.relative(ROOT, p), bytes: Number(du.stdout.trim().split(/\s+/)[0]) * 1024 };
      }
      summary.phases.distribution_size = sizes;
      console.log(`[size] ${JSON.stringify(sizes)}`);
    } catch (err) { fail("size", err); }
  }

  if (live) {
    const q = await quitApp(live);
    summary.phases.final_shutdown = q;
  }

  writeJson(path.join(OUT_DIR, `summary-${RUN_ID}.json`), summary);
  console.log(JSON.stringify({ ok: failures.length === 0, failures: failures.length, runId: RUN_ID }, null, 2));
  process.exit(failures.length ? 1 : 0);
}

process.on("exit", () => {
  if (live) {
    try { sigkillTree(live.pid); } catch {}
    try { sigkillTree(live.serverPid); } catch {}
  }
});

main().catch((err) => { fail("main", err); process.exit(1); });
