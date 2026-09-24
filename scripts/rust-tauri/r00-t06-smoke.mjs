// 一次性冒烟：验证 stub/fixture/启动就绪/HTTP/WS/工具指令全链路（不入基准样本）
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import WebSocket from "ws";
import { nowMs, sleep } from "./r00-t06-lib.mjs";
import { startStubProvider } from "./r00-t06-stub-provider.mjs";
import { buildPristineHome, cloneHome } from "./r00-t06-fixture.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..", "..");
const WRAP = path.join(ROOT, "dist-server", "mac-arm64", "hana-server");
const TOKEN = "r00t06-not-a-secret";

const stub = await startStubProvider({});
console.log("stub:", stub.baseUrl);

const pristine = fs.mkdtempSync(path.join(os.tmpdir(), "r00t06-smoke-pristine-"));
buildPristineHome({ dest: pristine, stubBaseUrl: stub.baseUrl });
console.log("fixture ok, config.yaml chat 段:");
console.log(fs.readFileSync(path.join(pristine, "agents/lingxi/config.yaml"), "utf8").match(/models:[\s\S]{0,200}/)?.[0]);

const home = cloneHome(pristine, "smoke");
const child = spawn(WRAP, [], {
  cwd: path.dirname(WRAP),
  env: {
    PATH: "/usr/bin:/bin:/usr/sbin:/sbin", HOME: home, LINGXI_HOME: home,
    LINGXI_PORT: "0", LINGXI_TOKEN: TOKEN, LINGXI_CREATE_STARTUP_SESSION: "0",
  },
  stdio: ["ignore", "pipe", "pipe"],
});
child.stdout.on("data", (c) => process.stdout.write(`[srv] ${c}`));
child.stderr.on("data", (c) => process.stderr.write(`[srv-err] ${c}`));

const infoPath = path.join(home, "server-info.json");
let info = null;
for (let i = 0; i < 2400 && !info; i++) {
  try {
    const p = JSON.parse(fs.readFileSync(infoPath, "utf8"));
    if (p?.port && p?.pid === child.pid) info = p;
  } catch {}
  await sleep(25);
}
if (!info) { child.kill("SIGKILL"); throw new Error("no server-info.json"); }
console.log("ready port=", info.port);

let health = null;
for (let i = 0; i < 100 && !health; i++) {
  try {
    const r = await fetch(`http://127.0.0.1:${info.port}/api/health`, { headers: { Authorization: `Bearer ${TOKEN}` } });
    if (r.ok) health = await r.json();
  } catch {}
  await sleep(100);
}
console.log("health:", JSON.stringify(health).slice(0, 300));

// 建会话（可能 409 no_available_model → 需修 fixture）
const res = await fetch(`http://127.0.0.1:${info.port}/api/sessions/new`, {
  method: "POST",
  headers: { Authorization: `Bearer ${TOKEN}`, "Content-Type": "application/json" },
  body: JSON.stringify({ memoryEnabled: false, permissionMode: "operate" }),
});
const newSessionText = await res.text();
console.log("sessions/new:", res.status, newSessionText.slice(0, 300));
if (!res.ok) { child.kill("SIGTERM"); process.exit(1); }
const s = JSON.parse(newSessionText);

// WS 流式一轮
const ws = new WebSocket(`ws://127.0.0.1:${info.port}/ws`, { headers: { Authorization: `Bearer ${TOKEN}` } });
const events = [];
ws.on("message", (raw) => { try { const e = JSON.parse(raw.toString()); e.__t = nowMs(); events.push(e); } catch {} });
await new Promise((r) => ws.once("open", r));
const t0 = nowMs();
ws.send(JSON.stringify({ type: "prompt", text: "bench-stream-0 direct answer, no tools.", sessionPath: s.path }));
let done = false;
setTimeout(() => { if (!done) { console.log("TIMEOUT; events:", [...new Set(events.map((e) => e.type))]); } }, 45000);
const iv = setInterval(() => {
  const end = events.find((e) => (e.type === "assistant_run_end" || e.type === "turn_end" || e.type === "agent_settled") && e.__t > t0);
  if (end) {
    done = true; clearInterval(iv);
    const firstDelta = events.find((e) => /delta/i.test(e.type || "") && e.__t > t0);
    console.log("STREAM OK firstDelta=", firstDelta ? firstDelta.__t - t0 : null, "end=", end.__t - t0);
    console.log("eventTypes:", [...new Set(events.map((e) => e.type))]);
    // 工具链冒烟：经 mcp_call 桥驱动 run_code（真实工具网关 + node-pty）
    const tTool = nowMs();
    ws.send(JSON.stringify({
      type: "prompt",
      text: `bench-tool-0 TOOL:mcp_call:${JSON.stringify({ tool: "run_code", arguments: { action: "run", language: "node", code: "console.log('pty-smoke-ok-'+'x'.repeat(64))" } })}`,
      sessionPath: s.path,
    }));
    const iv2 = setInterval(() => {
      const end2 = events.find((e) => (e.type === "assistant_run_end" || e.type === "turn_end" || e.type === "agent_settled") && e.__t > tTool);
      if (end2) {
        clearInterval(iv2);
        const toolStart = events.find((e) => e.type === "tool_start" && e.__t > tTool);
        const toolEnd = events.find((e) => e.type === "tool_end" && e.__t > tTool);
        const toolResult = events.filter((e) => /tool/.test(e.type || "") && e.__t > tTool).slice(0, 6);
        console.log("TOOL OK start=", toolStart ? toolStart.__t - tTool : null, "end=", toolEnd ? toolEnd.__t - tTool : null);
        console.log("toolEndDetail:", JSON.stringify(toolEnd).slice(0, 800));
        console.log("toolEvents:", toolResult.map((e) => e.type));
        ws.close(); child.kill("SIGTERM");
        setTimeout(() => process.exit(0), 1500);
      }
    }, 10);
    setTimeout(() => {
      console.log("TOOL TIMEOUT; first tool_end + last events:");
      const firstToolEnd = events.find((e) => e.type === "tool_end" && e.__t > tTool);
      if (firstToolEnd) console.log("  first tool_end:", JSON.stringify(firstToolEnd).slice(0, 700));
      for (const e of events.filter((e) => e.__t > tTool).slice(-10)) console.log(" ", e.type, JSON.stringify(e).slice(0, 120));
      try { child.kill("SIGKILL"); } catch {}
      process.exit(1);
    }, 60000);
  }
}, 10);
