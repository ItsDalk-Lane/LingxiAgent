/**
 * compat-old-server-runner.template.mjs — F01 四组合矩阵：旧服务端真实 HTTP 运行器（tracked 模板）。
 * 由兼容测试（tests/history-protocol-compat.test.ts 与
 * desktop/src/react/__tests__/history-protocol-compat.test.ts）在 beforeAll 幂等写入
 * A01 快照 clone（/tmp/lingxi-baseline-1d42b740，HEAD=1d42b740）的 tests/compat-old-server-runner.mjs，
 * 再以子进程 spawn；以文件自身位置定位 clone 根（写入点=clone/tests/，CLONE_ROOT=clone/）。
 * 以 Node 原生 TS type-stripping 直接运行 clone 的旧 sessions 路由（无 E 协议）；
 * stdout 打印 PORT=、SESSION_PATH=、SESSION_ID=、OLD_SERVER_READY；SIGTERM 退出。
 * 防污染设计：模板随主仓库 tracked 分发，clone 内副本可随时重建（不作状态载体），
 * clone 缺席时测试 skip 并在输出明示「四组合中旧服务端侧指定环境未验证」。
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { serve } from "@hono/node-server";
// 裸导入：ESM 从本文件位置（clone/tests/）向上解析 → 使用 clone 自身的依赖副本
import { Hono } from "hono";

const CLONE_ROOT = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/private/, "")), "..");
const { SessionManifestStore } = await import(`${CLONE_ROOT}/core/session-manifest/store.ts`);
const { SessionManager } = await import(`${CLONE_ROOT}/lib/pi-sdk/index.ts`);

const root = fs.mkdtempSync(path.join(os.tmpdir(), "compat-old-server-"));
const agentsDir = path.join(root, "agents");
const sessionPath = path.join(agentsDir, "hana", "sessions", "longrun.jsonl");
fs.mkdirSync(path.dirname(sessionPath), { recursive: true });

// 与 A03 同构固定夹具：1 头 + 1 user + 10 assistant + 9 toolResult（display=11）
const lines = [JSON.stringify({ type: "session", version: 3, id: "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" })];
const message = (id, parentId, m) => lines.push(JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T10:00:00Z", message: m }));
message("u1", null, { role: "user", content: "开始" });
let parent = "u1";
for (let i = 1; i <= 10; i += 1) {
  message(`a${i}`, parent, { role: "assistant", content: `回答 ${i}` });
  parent = `a${i}`;
  if (i < 10) {
    message(`r${i}`, parent, { role: "toolResult", content: [{ type: "text", text: `工具结果 ${i}` }] });
    parent = `r${i}`;
  }
}
fs.writeFileSync(sessionPath, lines.join("\n") + "\n");

const manifestStore = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") });
const manifest = manifestStore.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });

const engine = {
  agentsDir,
  currentSessionPath: sessionPath,
  isSessionStreaming: () => false,
  agentIdFromSessionPath: () => "hana",
  getAgent: () => ({ agentName: "Hana" }),
  deferredResults: null,
  subagentRuns: null,
  getSessionManifest: (id) => (id === manifest.sessionId ? manifest : null),
  getSessionIdForPath: (p) => (p === sessionPath ? manifest.sessionId : null),
  getSessionBranchHead: (id) => manifestStore.getBranchHead(id) || null,
  _sessionManifestStore: manifestStore,
  openSessionManagerAtCurrentBranch: (p, d) => SessionManager.open(p, d),
};

const { createSessionsRoute } = await import(`${CLONE_ROOT}/server/routes/sessions.ts`);
const app = new Hono();
app.route("/api", createSessionsRoute(engine));

const server = serve({ fetch: (req) => app.fetch(req), port: 0, hostname: "127.0.0.1" });
await new Promise((r) => (server.address() ? r() : server.once("listening", r)));
const port = server.address().port;
console.log(`PORT=${port}`);
console.log(`SESSION_PATH=${sessionPath}`);
console.log(`SESSION_ID=${manifest.sessionId}`);
console.log("OLD_SERVER_READY");
process.stdin.resume();
