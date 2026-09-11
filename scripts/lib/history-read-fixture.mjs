/**
 * A03 修正后长 Run 合法夹具 + 真实服务端路由 harness（复用
 * tests/history-pagination-run-continuity.test.ts 的 writeLongRunSession / buildApp 构造，
 * 字节级一致，装载时与 fixture-audit.json 的 sha256 交叉校验）。
 *
 * 注意：本模块不静态 import 任何生产模块——SDK 对 node:fs 具名导入在模块装载时快照，
 * 基准入口必须先安装 fs/JSON 包装与模块重定向钩子，再通过 loadProductionModules()
 * 动态加载生产代码（A05 仪表「覆盖真实读取入口」的前提）。
 */

import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";

export const SESSION_FILE_HEADER_ID = "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31";
export const BUSINESS_AGENT_ID = "hana";

/** A03 writeLongRunSession 的逐字节移植（无尾换行、无随机数、全索引派生）。 */
export function buildLongRunFixtureLines(n) {
  const jsonlLine = (id, parentId, message) =>
    JSON.stringify({
      type: "message",
      id,
      parentId,
      timestamp: "2026-09-10T10:00:00Z",
      message,
    });
  const lines = [
    JSON.stringify({
      type: "session",
      version: 3,
      id: SESSION_FILE_HEADER_ID,
      cwd: "/tmp",
      timestamp: "2026-09-10T09:00:00Z",
    }),
    jsonlLine("u1", null, { role: "user", content: "请完成长任务并给出最终报告" }),
  ];
  let parent = "u1";
  for (let i = 1; i <= n; i += 1) {
    const isFinal = i === n;
    const content = isFinal
      ? [
          { type: "thinking", thinking: `最终整理 ${i}` },
          { type: "text", text: `这是最终报告：任务已完成（第 ${i} 次调用后）。` },
        ]
      : [
          { type: "thinking", thinking: `思考片段 ${i}` },
          { type: "tool_use", id: `tu-${i}`, name: "read_file", input: { path: `f${i}` } },
        ];
    lines.push(jsonlLine(`a${i}`, parent, { role: "assistant", content }));
    if (!isFinal) {
      lines.push(
        jsonlLine(`r${i}`, `a${i}`, {
          role: "toolResult",
          toolCallId: `tu-${i}`,
          toolName: "read_file",
          content: `文件内容 ${i}`,
        }),
      );
      parent = `r${i}`;
    }
  }
  return lines;
}

export function buildLongRunFixtureBytes(n) {
  return Buffer.from(buildLongRunFixtureLines(n).join("\n"), "utf8");
}

export function sha256(buf) {
  return createHash("sha256").update(buf).digest("hex");
}

/** 与 A03 fixture-audit.json 的记录交叉校验；auditPath 不存在时跳过（显式记录，不静默）。 */
export function verifyFixtureAgainstAudit(bytesByN, auditPath) {
  const result = { auditPath, checked: [], auditAvailable: false };
  if (!fs.existsSync(auditPath)) {
    result.note = "fixture-audit.json 不存在，跳过交叉校验（构建参数仍按 A03 定义断言）";
    return result;
  }
  const audit = JSON.parse(fs.readFileSync(auditPath, "utf8"));
  result.auditAvailable = true;
  for (const [n, bytes] of Object.entries(bytesByN)) {
    const key = `N=${n}`;
    const record = audit.fixtures?.[key];
    if (!record) {
      result.checked.push({ n: Number(n), found: false });
      continue;
    }
    const actual = { bytes: bytes.length, sha256: sha256(bytes) };
    const ok = actual.bytes === record.bytes && actual.sha256 === record.sha256;
    result.checked.push({ n: Number(n), found: true, ok, expected: { bytes: record.bytes, sha256: record.sha256 }, actual });
    if (!ok) {
      throw new Error(
        `夹具字节与 A03 审计记录不一致（N=${n}）：期望 ${record.bytes}B/${record.sha256}，实际 ${actual.bytes}B/${actual.sha256}`,
      );
    }
  }
  return result;
}

/** 动态加载生产模块（必须在安装 fs/JSON 包装与模块重定向之后调用）。 */
export async function loadProductionModules() {
  const { Hono } = await import("hono");
  const { SessionManager } = await import("../../lib/pi-sdk/index.ts");
  const { SessionManifestStore } = await import("../../core/session-manifest/store.ts");
  const { createSessionsRoute } = await import("../../server/routes/sessions.ts");
  return { Hono, SessionManager, SessionManifestStore, createSessionsRoute };
}

/**
 * A03 buildApp 的移植：真实 createSessionsRoute + 真实 SessionManager.open/getBranch，
 * 仅加计数/异常跟踪包装（经 counters 归因到当前请求），不改变行为。
 */
export function createBranchTracker() {
  return {
    openCalls: 0,
    openSessionPathCalls: 0,
    openThrows: 0,
    getBranchCalls: 0,
    getBranchThrows: 0,
  };
}

export async function buildBenchApp({ mods, counters, agentsDir, sessionPath, sessionManifest, engineOverrides = {} }) {
  const { Hono, SessionManager, createSessionsRoute } = mods;
  const tracker = createBranchTracker();
  const app = new Hono();
  const engine = {
    agentsDir,
    currentSessionPath: null,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => BUSINESS_AGENT_ID,
    getAgent: () => ({ agentName: "Hana" }),
    getSessionWorkspaceMount: () => null,
    getSessionManifest: (id) => {
      const countersActive = counters;
      const t0 = process.hrtime.bigint();
      try {
        return id === sessionManifest.sessionId ? sessionManifest : null;
      } finally {
        if (countersActive) {
          countersActive.recordScan("identity.getSessionManifest", null, Number(process.hrtime.bigint() - t0) / 1e6);
        }
      }
    },
    getSessionIdForPath: (p) => {
      const countersActive = counters;
      const t0 = process.hrtime.bigint();
      try {
        return p === sessionPath ? sessionManifest.sessionId : null;
      } finally {
        if (countersActive) {
          countersActive.recordScan("identity.getSessionIdForPath", null, Number(process.hrtime.bigint() - t0) / 1e6);
        }
      }
    },
    openSessionManagerAtCurrentBranch: (p, dir) => {
      tracker.openCalls += 1;
      if (p === sessionPath) tracker.openSessionPathCalls += 1;
      let manager;
      try {
        manager = SessionManager.open(p, dir);
      } catch (error) {
        tracker.openThrows += 1;
        counters?.recordBranchOpen(true);
        throw error;
      }
      counters?.recordBranchOpen(false);
      const rawGetBranch = manager.getBranch.bind(manager);
      manager.getBranch = (...args) => {
        tracker.getBranchCalls += 1;
        try {
          return rawGetBranch(...args);
        } catch (error) {
          tracker.getBranchThrows += 1;
          counters?.recordGetBranch(true);
          throw error;
        }
      };
      counters?.recordGetBranch(false);
      return manager;
    },
    ...engineOverrides,
  };
  app.route("/api", createSessionsRoute(engine));
  return { app, tracker };
}

/**
 * 一个独立的基准环境：全新临时目录、从同一原始夹具字节复制出的独立会话文件、
 * 全新真实 SessionManifestStore（外部 store 固定为空）、全新 Hono app。
 */
export function createBenchEnvironment({ mods, counters, n, rootDir, fixtureBytes }) {
  const agentsDir = path.join(rootDir, "agents");
  const sessionPath = path.join(agentsDir, BUSINESS_AGENT_ID, "sessions", "page-target.jsonl");
  fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
  fs.writeFileSync(sessionPath, fixtureBytes);
  const preSha256 = sha256(fs.readFileSync(sessionPath));
  if (preSha256 !== sha256(fixtureBytes)) {
    throw new Error(`会话文件复制后 sha256 与原始夹具不一致：${preSha256}`);
  }
  const manifestStore = new mods.SessionManifestStore({ dbPath: path.join(rootDir, "session-manifest.db") });
  const sessionManifest = manifestStore.createForPath({
    sessionPath,
    ownerAgentId: BUSINESS_AGENT_ID,
    domain: "desktop",
    kind: "chat",
  });
  return { agentsDir, sessionPath, manifestStore, sessionManifest, preSha256, fixtureBytes, n };
}

export async function attachBenchApp(env, { mods, counters, engineOverrides = {} }) {
  const { app, tracker } = await buildBenchApp({
    mods,
    counters,
    agentsDir: env.agentsDir,
    sessionPath: env.sessionPath,
    sessionManifest: env.sessionManifest,
    engineOverrides,
  });
  env.app = app;
  env.tracker = tracker;
  env.engineOverrides = engineOverrides;
  return env;
}

export function teardownBenchEnvironment(env) {
  try {
    env.manifestStore?.close();
  } finally {
    try {
      fs.rmSync(path.dirname(env.agentsDir), { recursive: true, force: true });
    } catch {
      // 清理失败不影响结果，由上层记录临时目录
    }
  }
}

export function messagesUrl(sessionPath, { before = null, limit = null, all = false } = {}) {
  let url = `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}`;
  if (limit != null) url += `&limit=${limit}`;
  if (before != null) url += `&before=${before}`;
  if (all) url += `&all=1`;
  return url;
}
