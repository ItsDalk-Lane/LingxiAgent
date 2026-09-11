/**
 * smoke-history-protocol.mjs — E08 真实链路 smoke（Node HTTP loopback）。
 *
 * 对真实 Hono 路由（D01 同一 app 构造）做端到端断言：
 *  1. 消息 200：协议/缓存/ETag 头存在；
 *  2. 条件请求 If-None-Match → 304：无正文、无 Content-Length、ETag/私有策略在；
 *  3. 概览 200：available:true + pagination.recommendedLimit 与消息头声明同源（=100）；
 *  4. CORS：带 Origin 的请求回显 Allow-Origin、Expose-Headers 含 ETag/协议头；
 *     OPTIONS 预检的 Allow-Headers 含 If-None-Match。
 *
 * 环境限制：本脚本验证 Node HTTP loopback 层；真实浏览器/Electron 网络栈（凭据
 * 附带、预检缓存、渲染进程头可见性）未覆盖，留待 F 阶段指定环境（见
 * protocol/browser-smoke-report.md）。
 *
 * 用法：node scripts/smoke-history-protocol.mjs [--sizes 1000,10000] [--output dir]
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { serve } from "@hono/node-server";
import { Hono } from "hono";
import { applyCorsResponseHeaders } from "../server/http/cors-policy.ts";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const sizes = (process.argv.find((a, i) => process.argv[i - 1] === "--sizes")?.split(",").map(Number)) ?? [1000, 10000];
const outDir = process.argv[process.argv.indexOf("--output") + 1] ?? path.join(ROOT, "artifacts", "history-read-directory", "protocol");

const fx = await import(new URL("./lib/history-read-fixture.mjs", import.meta.url).href.replace("file://", "file:///").replace("///", "///"));
const mods = await fx.loadProductionModules();
const { createHistoryReadCounters } = await import(new URL("./lib/history-read-counters.mjs", import.meta.url).href);
const counters = createHistoryReadCounters({ label: "smoke-history-protocol" });

const results = [];
let failures = 0;

async function smokeSize(n) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `smoke-${n}-`));
  const env = fx.createBenchEnvironment({ mods, counters, n, rootDir: tmp, fixtureBytes: fx.buildLongRunFixtureBytes(n) });
  await fx.attachBenchApp(env, { mods, counters, engineOverrides: {} });
  // 真实生产 CORS 中间件（与 server/index.ts 同一函数）：挂载在 bench app 之外，
  // 使 smoke 覆盖真实链路的头契约（Allow-Headers/Expose-Headers/Allow-Origin）。
  const outer = new Hono();
  outer.use("*", async (c, next) => {
    applyCorsResponseHeaders(c, { origin: c.req.header("origin") ?? "" });
    await next();
  });
  outer.route("/", env.app);
  const server = serve({ fetch: (req) => outer.fetch(req), port: 0, hostname: "127.0.0.1" });
  await new Promise((r) => (server.address() ? r() : server.once("listening", r)));
  const port = server.address().port;
  const base = `http://127.0.0.1:${port}`;
  const url = fx.messagesUrl(env.sessionPath, { before: null, limit: 100 });

  const assert = (name, cond, detail = "") => {
    results.push({ size: n, name, pass: !!cond, detail });
    if (!cond) failures += 1;
    console.log(`  ${cond ? "PASS" : "FAIL"} ${name}${detail ? " — " + detail : ""}`);
  };

  // 1. 首次 200（带 Origin 模拟渲染进程跨源场景）
  const r1 = await fetch(base + url, { headers: { origin: "http://localhost:5173" } });
  const errBody = r1.status !== 200 ? "" : "";
  const etag = r1.headers.get("etag");
  if (r1.status !== 200) {
    const errText = await r1.clone().text().catch(() => "");
    console.error(`[smoke-dbg] messages ${r1.status}:`, errText.slice(0, 300));
  }
  assert("messages 200", r1.status === 200);
  assert("协议头 Lingxi-History-Protocol: 1", r1.headers.get("lingxi-history-protocol") === "1");
  assert("推荐页大小声明 = 100", r1.headers.get("lingxi-history-page-limit") === "100");
  assert("Cache-Control: private, no-store", (r1.headers.get("cache-control") || "").includes("private") && (r1.headers.get("cache-control") || "").includes("no-store"));
  assert("ETag 为 hrp1 弱标签", /^W\/"hrp1-[0-9a-f]{64}"$/.test(etag ?? ""));
  assert("CORS Allow-Origin 回显", r1.headers.get("access-control-allow-origin") === "http://localhost:5173");
  const expose = r1.headers.get("access-control-expose-headers") ?? "";
  assert("Expose-Headers 含 ETag/协议头/页大小", ["ETag", "Lingxi-History-Protocol", "Lingxi-History-Page-Limit"].every((h) => expose.includes(h)));
  const body1 = await r1.text();

  // 2. 条件请求 → 304
  const r2 = await fetch(base + url, { headers: { "if-none-match": etag, "cache-control": "no-store", origin: "http://localhost:5173" } });
  assert("条件命中 304", r2.status === 304);
  assert("304 无正文", (await r2.text()) === "");
  assert("304 无 Content-Length", r2.headers.get("content-length") === null);
  assert("304 ETag/私有策略在", r2.headers.get("etag") === etag && (r2.headers.get("cache-control") || "").includes("no-store"));

  // 3. 概览
  const ovRes = await fetch(`${base}/api/sessions/history-overview?path=${encodeURIComponent(env.sessionPath)}`, { headers: { origin: "http://localhost:5173" } });
  const ovBody = await ovRes.json();
  assert("概览 200 available", ovRes.status === 200 && ovBody.available === true);
  assert("概览 recommendedLimit 与消息头同源 (=100)", ovBody.pagination.recommendedLimit === Number(r1.headers.get("lingxi-history-page-limit")));
  assert("概览 Cache-Control 私有", (ovRes.headers.get("cache-control") || "").includes("no-store"));

  // 4. 预检
  const pre = await fetch(base + url, { method: "OPTIONS", headers: { origin: "http://localhost:5173", "access-control-request-method": "GET", "access-control-request-headers": "if-none-match" } });
  const allowHeaders = pre.headers.get("access-control-allow-headers") ?? "";
  assert("预检 Allow-Headers 含 If-None-Match", allowHeaders.includes("If-None-Match") || allowHeaders.includes("if-none-match"));
  assert("Allow-Credentials=true 且来源为白名单回显（非通配）", pre.headers.get("access-control-allow-credentials") === "true" && pre.headers.get("access-control-allow-origin") !== "*");

  server.close();
  fx.teardownBenchEnvironment(env);
  fs.rmSync(tmp, { recursive: true, force: true });
}

for (const n of sizes) {
  console.log(`smoke size=${n}`);
  await smokeSize(n);
}

const report = {
  generatedAt: new Date().toISOString(),
  environment: "Node HTTP loopback（真实 Hono 路由）；真实浏览器/Electron 环境未验证，留待 F 阶段指定环境",
  pass: failures === 0,
  failures,
  results,
};
fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "smoke-results.json"), JSON.stringify(report, null, 2), "utf8");
console.log(`smoke 完成：${failures === 0 ? "全部通过" : failures + " 项失败"}；报告 ${path.relative(process.cwd(), outDir)}`);
if (failures > 0) process.exitCode = 1;
