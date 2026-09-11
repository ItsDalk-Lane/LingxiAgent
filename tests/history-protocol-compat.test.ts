/**
 * F01 四组合兼容矩阵（服务端侧；真实 HTTP 链路）：
 *  - 旧+旧（控制组）：A01 clone 的旧 sessions 路由以子进程真实运行，原请求构造 → 200 旧 JSON、无协议头；
 *  - 新+旧（新服务端 × 旧客户端构造）：省略 limit → 服务端默认 50、正常 200、原 JSON 七字段可消费、
 *    新增头不影响旧解析（旧客户端只读 body）；
 *  - 旧+新（新客户端 × 旧服务端）：旧服务端事实——概览 404、无 ETag/协议头（新客户端据此无感回退；
 *    客户端侧行为在 desktop/src/react/__tests__/history-protocol-compat.test.ts 以真实 HTTP 验证）；
 *  - 新+新（真实 HTTP）：条件请求 200→304、ETag 随 limit 变化。
 *
 * 层级标注：旧服务端=clone 子进程真实运行（Node type-stripping 跑 A01 旧代码）；
 * 旧客户端=clone 源码的请求构造方式 + 真实 HTTP（未运行完整旧 renderer，如实记录）。
 *
 * runner 自包含：模板 tracked 于 tests/compat-old-server-runner.template.mjs，beforeAll 幂等写入
 * clone tests/compat-old-server-runner.mjs 再 spawn（clone 内副本可随时重建，不作状态载体）。
 * clone 缺席时整套件 skip（describe.skipIf），并在模块加载时 console 明示
 * 「四组合中旧服务端侧指定环境未验证」（不静默 skip）。
 */
import { describe, it, expect, beforeAll, afterAll } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { Hono } from "hono";
import { serve } from "@hono/node-server";

import { createSessionsRoute } from "../server/routes/sessions.ts";
import { HistoryDirectoryCache } from "../server/history-read/cache.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { SessionManager } from "../lib/pi-sdk/index.ts";

const CLONE_ROOT = "/tmp/lingxi-baseline-1d42b740";
const CLONE_HEAD = "1d42b7405c76292f617291e3a01cd2f3ef5efd04";
const RUNNER_TEMPLATE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "compat-old-server-runner.template.mjs");
const RUNNER_PATH = path.join(CLONE_ROOT, "tests", "compat-old-server-runner.mjs");

function cloneReady(): boolean {
  return (
    fs.existsSync(path.join(CLONE_ROOT, ".git")) &&
    fs.existsSync(path.join(CLONE_ROOT, "server", "routes", "sessions.ts")) &&
    fs.existsSync(RUNNER_TEMPLATE)
  );
}

let child: ReturnType<typeof spawn> | null = null;
let oldPort = "";
let oldSessionPath = "";
let oldSessionId = "";

async function startOldServer(): Promise<void> {
  const runner = path.join(CLONE_ROOT, "tests", "compat-old-server-runner.mjs");
  child = spawn("node", [runner], { cwd: CLONE_ROOT, stdio: ["ignore", "pipe", "pipe"] });
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("old server 启动超时")), 30000);
    const onData = (buf: Buffer) => {
      const text = buf.toString();
      const portMatch = text.match(/PORT=(\d+)/);
      const pathMatch = text.match(/SESSION_PATH=(.*)/);
      const idMatch = text.match(/SESSION_ID=(.*)/);
      if (portMatch) oldPort = portMatch[1];
      if (pathMatch) oldSessionPath = pathMatch[1].trim();
      if (idMatch) oldSessionId = idMatch[1].trim();
      if (text.includes("OLD_SERVER_READY") && oldPort) {
        clearTimeout(timer);
        resolve();
      }
    };
    child!.stdout!.on("data", onData);
    child!.stderr!.on("data", (b: Buffer) => process.stderr.write(b));
    child!.on("exit", (code) => reject(new Error(`old server 提前退出 code=${code}`)));
  });
}

/** 新服务端：主仓库现行路由经真实 HTTP 暴露（非 app.request 内存路径）。 */
async function startNewServer() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "compat-new-server-"));
  const agentsDir = path.join(root, "agents");
  const sessionPath = path.join(agentsDir, "hana", "sessions", "longrun.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  const lines = [JSON.stringify({ type: "session", version: 3, id: "compat-new", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" })];
  lines.push(JSON.stringify({ type: "message", id: "u1", parentId: null, timestamp: "2026-09-10T10:00:00Z", message: { role: "user", content: "开始" } }));
  let parent = "u1";
  for (let i = 1; i <= 10; i += 1) {
    lines.push(JSON.stringify({ type: "message", id: `a${i}`, parentId: parent, timestamp: "2026-09-10T10:00:00Z", message: { role: "assistant", content: `回答 ${i}` } }));
    parent = `a${i}`;
    if (i < 10) {
      lines.push(JSON.stringify({ type: "message", id: `r${i}`, parentId: parent, timestamp: "2026-09-10T10:00:00Z", message: { role: "toolResult", content: [{ type: "text", text: `工具结果 ${i}` }] } }));
      parent = `r${i}`;
    }
  }
  fs.writeFileSync(sessionPath, lines.join("\n") + "\n");
  const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") });
  const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
  const cache = new HistoryDirectoryCache();
  const engine: any = {
    agentsDir,
    currentSessionPath: sessionPath,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => "hana",
    getAgent: () => ({ agentName: "Hana" }),
    historyReadCache: cache,
    activityHub: null,
    deferredResults: null,
    subagentRuns: null,
    getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
    getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
    getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
    _sessionManifestStore: manifestStore,
    openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
  };
  const app = new Hono();
  app.route("/api", createSessionsRoute(engine));
  const server = serve({ fetch: (req) => app.fetch(req), port: 0, hostname: "127.0.0.1" });
  await new Promise<void>((r) => (server.address() ? r(undefined) : server.once("listening", () => r(undefined))));
  const port = (server.address() as any).port;
  return { root, sessionPath, port, server, manifest };
}

if (!cloneReady()) {
  console.warn(`[history-protocol-compat] A01 clone 缺席（${CLONE_ROOT}）：四组合中旧服务端侧指定环境未验证，本套件 skip（见 protocol/compatibility-matrix.json 环境依赖注记）`);
}

describe.skipIf(!cloneReady())("F01 四组合兼容矩阵（真实 HTTP）", () => {
  beforeAll(async () => {
    // runner 自包含：tracked 模板幂等覆盖写入 clone（每次运行重建，防止旧副本/缺失副本漂移）
    fs.copyFileSync(RUNNER_TEMPLATE, RUNNER_PATH);
    await startOldServer();
  }, 40000);
  afterAll(() => {
    child?.kill("SIGTERM");
  });

  it("旧+旧（控制组）：旧服务端 + 旧客户端请求构造 → 200 旧 JSON、无协议头", async () => {
    const oldClientUrl = `/api/sessions/messages?path=${encodeURIComponent(oldSessionPath)}&sessionId=${oldSessionId}`;
    const res = await fetch(`http://127.0.0.1:${oldPort}${oldClientUrl}`);
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(Array.isArray(body.messages)).toBe(true);
    expect(typeof body.hasMore).toBe("boolean");
    expect(res.headers.get("lingxi-history-protocol")).toBeNull(); // 旧服务端无协议头
    expect(res.headers.get("etag")).toBeNull();
  });

  it("新+旧：新服务端对旧客户端构造 → 省略 limit 默认 50、正常 200、七字段 JSON 可消费、新增头不破坏旧解析", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "compat-new-srv-"));
    const agentsDir = path.join(root, "agents");
    const sessionPath = path.join(agentsDir, "hana", "sessions", "longrun.jsonl");
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
    const lines = [JSON.stringify({ type: "session", version: 3, id: "compat-new-old", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" })];
    lines.push(JSON.stringify({ type: "message", id: "u1", parentId: null, timestamp: "2026-09-10T10:00:00Z", message: { role: "user", content: "开始" } }));
    let parent = "u1";
    for (let i = 1; i <= 60; i += 1) {
      lines.push(JSON.stringify({ type: "message", id: `a${i}`, parentId: parent, timestamp: "2026-09-10T10:00:00Z", message: { role: "assistant", content: `回答 ${i}` } }));
      parent = `a${i}`;
    }
    fs.writeFileSync(sessionPath, lines.join("\n") + "\n");
    const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") });
    const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
    const cache = new HistoryDirectoryCache();
    const engine: any = {
      agentsDir, currentSessionPath: sessionPath, isSessionStreaming: () => false,
      agentIdFromSessionPath: () => "hana", getAgent: () => ({ agentName: "Hana" }),
      historyReadCache: new HistoryDirectoryCache(), activityHub: null, deferredResults: null, subagentRuns: null,
      getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
      getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
      getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
      _sessionManifestStore: manifestStore,
      openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
    };
    const app = new Hono();
    app.route("/api", createSessionsRoute(engine));
    const server = serve({ fetch: (req) => app.fetch(req), port: 0, hostname: "127.0.0.1" });
    await new Promise<void>((r) => (server.address() ? r(undefined) : server.once("listening", () => r(undefined))));
    const port = (server.address() as any).port;
    try {
      // 旧客户端构造：path+sessionId，省略 limit，不携带条件头
      const oldStyleUrl = `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&sessionId=${manifest.sessionId}`;
      const res = await fetch(`http://127.0.0.1:${port}${oldStyleUrl}`);
      expect(res.status).toBe(200);
      const body = await res.json();
      expect(body.messages.length).toBeLessThanOrEqual(50); // 服务端默认 50
      for (const field of ["messages", "blocks", "todos", "sessionFiles", "hasMore", "nextBefore", "revision"]) {
        expect(field in body).toBe(true); // 原 JSON 字段齐全 → 旧客户端可消费
      }
    } finally {
      server.close();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  it("旧+新：旧服务端对概览返回 404、消息无 ETag/协议头（新客户端据此无感回退）", async () => {
    const ov = await fetch(`http://127.0.0.1:${oldPort}/api/sessions/history-overview?path=${encodeURIComponent(oldSessionPath)}`);
    expect(ov.status).toBe(404); // 旧服务端无概览端点
    const res = await fetch(`http://127.0.0.1:${oldPort}/api/sessions/messages?path=${encodeURIComponent(oldSessionPath)}&limit=50`);
    expect(res.status).toBe(200);
    expect(res.headers.get("etag")).toBeNull();
    expect(res.headers.get("lingxi-history-protocol")).toBeNull();
    await res.text();
  });

  it("新+新：真实 HTTP 条件请求 200→304；ETag 随 limit 变化", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "compat-new-new-"));
    const agentsDir = path.join(root, "agents");
    const sessionPath = path.join(agentsDir, "hana", "sessions", "longrun.jsonl");
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
    const lines = [JSON.stringify({ type: "session", version: 3, id: "compat-new-new", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" })];
    lines.push(JSON.stringify({ type: "message", id: "u1", parentId: null, timestamp: "2026-09-10T10:00:00Z", message: { role: "user", content: "开始" } }));
    fs.writeFileSync(sessionPath, lines.join("\n") + "\n");
    const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") });
    const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
    const cache = new HistoryDirectoryCache();
    const engine: any = {
      agentsDir, currentSessionPath: sessionPath, isSessionStreaming: () => false,
      agentIdFromSessionPath: () => "hana", getAgent: () => ({ agentName: "Hana" }),
      historyReadCache: new HistoryDirectoryCache(), activityHub: null, deferredResults: null, subagentRuns: null,
      getSessionManifest: (id: string) => (id === manifest.sessionId ? manifest : null),
      getSessionIdForPath: (p: string) => (p === sessionPath ? manifest.sessionId : null),
      getSessionBranchHead: (id: string) => manifestStore.getBranchHead(id) || null,
      _sessionManifestStore: manifestStore,
      openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
    };
    const app = new Hono();
    app.route("/api", createSessionsRoute(engine));
    const server = serve({ fetch: (req) => app.fetch(req), port: 0, hostname: "127.0.0.1" });
    await new Promise<void>((r) => (server.address() ? r(undefined) : server.once("listening", () => r(undefined))));
    const port = (server.address() as any).port;
    try {
      const url50 = `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&limit=50`;
      const first = await fetch(`http://127.0.0.1:${port}${url50}`);
      expect(first.status).toBe(200);
      const etag = first.headers.get("etag")!;
      expect(etag).toMatch(/^W\/"hrp1-/);
      const notMod = await fetch(`http://127.0.0.1:${port}${url50}`, { headers: { "if-none-match": etag } });
      expect(notMod.status).toBe(304);
      expect(await notMod.text()).toBe("");
      // limit 变化 → 不同标签
      const other = await fetch(`http://127.0.0.1:${port}/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&limit=100`);
      expect(other.headers.get("etag")).not.toBe(etag);
      await other.text();
    } finally {
      server.close();
      fs.rmSync(root, { recursive: true, force: true });
    }
  });
});
