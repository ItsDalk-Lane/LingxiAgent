import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { buildRuntimeEnvironment } from "./smoke-packaged-knowledge.mjs";
import { redactLogText, redactLogValue } from "../shared/log-redactor.ts";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const platform = process.platform, arch = process.arch;
const executable = platform === "darwin"
  ? path.join(root, "dist", arch === "arm64" ? "mac-arm64" : "mac", "Lingxi.app/Contents/MacOS/Lingxi")
  : platform === "win32" ? path.join(root, "dist/win-unpacked/Lingxi.exe")
    : path.join(root, "dist/linux-unpacked/lingxi");
assert.ok(fs.existsSync(executable), `Packaged desktop executable missing: ${executable}`);
const home = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-desktop-startup-"));
const userDataName = path.basename(home).replace(/^./, value => value.toUpperCase());
const appData = platform === "darwin" ? path.join(os.homedir(), "Library/Application Support")
  : platform === "win32" ? process.env.APPDATA : process.env.XDG_CONFIG_HOME || path.join(os.homedir(), ".config");
assert.ok(appData, "Desktop application data directory is unavailable");
const userData = path.join(appData, userDataName);
assert.ok(!fs.existsSync(userData), "Refusing to reuse an existing desktop test profile");
const startedAt = new Date().toISOString(), start = performance.now();
const report = { platform, arch, startedAt, status: "running", packaged: false, rendererReady: false, serverReady: false };
const reportPath = path.join(root, "artifacts", `knowledge-desktop-startup-${platform}-${arch}.json`);
const env = { ...buildRuntimeEnvironment(), LINGXI_HOME: home, LINGXI_PORT: "0" };
// 桌面包必须按桌面进程启动，不继承开发终端可能设置的 Node 模式。
delete env.ELECTRON_RUN_AS_NODE;
const child = spawn(executable, ["--remote-debugging-address=127.0.0.1", "--remote-debugging-port=0"], { env, stdio: ["ignore", "pipe", "pipe"] });
let spawnError;
child.on("error", error => { spawnError = error; });
let stderr = "";
child.stderr.on("data", data => { stderr = (stderr + data).slice(-8000); });
let stdout = "";
child.stdout.on("data", data => { stdout = (stdout + data).slice(-8000); });
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
function readJson(file) { try { return JSON.parse(fs.readFileSync(file, "utf8")); } catch { return null; } }
function launchEvents() {
  const file = path.join(home, "diagnostics/desktop-launch/renderer.log");
  if (!fs.existsSync(file)) return [];
  return fs.readFileSync(file, "utf8").split("\n").filter(Boolean).flatMap(line => {
    try { return [JSON.parse(line)]; } catch { return []; }
  });
}
async function inspectRenderer() {
  const active = path.join(userData, "DevToolsActivePort");
  if (!fs.existsSync(active)) return null;
  const port = Number(fs.readFileSync(active, "utf8").split("\n")[0]);
  if (!Number.isSafeInteger(port) || port <= 0) return null;
  const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(2000) })).json();
  const page = pages.find(page => page.type === "page" && /\/(onboarding|index)\.html/.test(page.url));
  if (!page) return null;
  const socket = new globalThis.WebSocket(page.webSocketDebuggerUrl);
  try {
    return await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Desktop renderer inspection timed out")), 2000);
      socket.addEventListener("error", () => { clearTimeout(timer); reject(new Error("Desktop renderer inspection failed")); });
      socket.addEventListener("open", () => socket.send(JSON.stringify({ id: 1, method: "Runtime.evaluate", params: {
        expression: '({ready:document.readyState, textLength:document.body?.innerText?.trim().length ?? 0, controls:document.querySelectorAll("button,input").length})', returnByValue: true,
      } })));
      socket.addEventListener("message", event => {
        const message = JSON.parse(String(event.data));
        if (message.id !== 1) return;
        clearTimeout(timer);
        if (message.error || message.result?.exceptionDetails) reject(new Error("Desktop renderer evaluation failed"));
        else resolve({ ...message.result.result.value, entryScreen: page.url.includes("onboarding") ? "onboarding" : "main" });
      });
    });
  } finally { socket.close(); }
}

async function stopPid(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 0) return;
  try { process.kill(pid, "SIGTERM"); } catch { return; }
  for (let attempt = 0; attempt < 100; attempt++) {
    try { process.kill(pid, 0); } catch { return; }
    await sleep(100);
  }
  try { process.kill(pid, "SIGKILL"); } catch { /* 进程已退出。 */ }
}
try {
  for (;;) {
    if (spawnError) throw spawnError;
    assert.equal(child.exitCode, null, `Packaged desktop exited before readiness: ${stderr}`);
    assert.equal(child.signalCode, null, `Packaged desktop terminated before readiness: ${stderr}`);
    const events = launchEvents();
    report.packaged = events.some(event => event.event === "desktop-launch-start" && event.details?.packaged === true);
    // 空资料目录首启应进入引导页；既检查页面加载，也检查真实 React 界面已有文字和交互控件。
    const renderer = await inspectRenderer();
    report.rendererReady = renderer?.ready === "complete" && renderer.textLength > 20 && renderer.controls > 0;
    if (renderer) report.renderer = renderer;
    assert.ok(!events.some(event => ["app-ready-timeout", "render-process-gone", "did-fail-load"].includes(event.event)),
      "Packaged desktop startup reported a failed renderer phase");
    const info = readJson(path.join(home, "server-info.json"));
    if (info?.port && info?.token) {
      try {
        const response = await fetch(`http://127.0.0.1:${info.port}/api/knowledge/notebooks`, {
          headers: { Authorization: `Bearer ${info.token}` }, signal: AbortSignal.timeout(2000),
        });
        report.serverReady = response.status === 200 && Array.isArray((await response.json()).notebooks);
      } catch { /* 等待真实服务端完成启动。 */ }
    }
    if (report.packaged && report.rendererReady && report.serverReady) break;
    assert.ok(performance.now() - start < 90_000, `Packaged desktop did not become ready: ${stderr}`);
    await sleep(200);
  }
  report.status = "passed";
} catch (error) {
  report.status = "failed";
  const redaction = { homeDir: os.homedir(), extraPaths: [home, userData] };
  report.error = redactLogText(error instanceof Error ? error.message : String(error), redaction);
  // 失败时保留有界、脱敏的实际启动诊断，清理临时资料后仍能定位失败阶段。
  report.launchEvents = redactLogValue(launchEvents().slice(-30), redaction);
  report.processOutput = redactLogText(stdout + "\n" + stderr, redaction);
  process.exitCode = 1;
} finally {
  report.durationMs = performance.now() - start;
  report.finishedAt = new Date().toISOString();
  // 只处理本次随机资料目录记录的子服务与本次启动的桌面进程。
  const serverPid = readJson(path.join(home, "server-info.json"))?.pid;
  await stopPid(child.pid);
  if (serverPid !== child.pid) await stopPid(serverPid);
  fs.mkdirSync(path.dirname(reportPath), { recursive: true });
  fs.writeFileSync(reportPath, JSON.stringify(report, null, 2) + "\n");
  fs.rmSync(home, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  fs.rmSync(userData, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  console.log(JSON.stringify(report));
}
