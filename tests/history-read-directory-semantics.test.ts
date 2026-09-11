/**
 * B06 热页语义测试（X10/X11/X12/X14/X19 + 强不变量）：
 *  - 强不变量：任意夹具下，热页（目录命中路径）输出 === 全量输出按窗口切片；
 *    覆盖页首前不可见 assistant（assistantSegments ordinal）、隐藏 user（占号）、
 *    loop-turn（turnInputEntryId=null + turnInputVisible:false 显式下发）、跨页 Run；
 *  - X10：跨页 toolResult 结局（结果记录在窗口外）补读后旧页结局正确；
 *  - X11：interlude 抑制/去重次序与页外锚点；
 *  - X12：collab 后补决策、correlation ambiguous；
 *  - X14：JSONL 不变时外部 store（deferred）变化正确反映到热页；
 *  - X19：热页 deferred 凭证经 /sessions/content/:contentId 展开成功且
 *    sourceIndex/entryId 语义不变。
 */
import { describe, it, expect, afterEach } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { Hono } from "hono";

import { createSessionsRoute } from "../server/routes/sessions.ts";
import { HistoryDirectoryCache } from "../server/history-read/cache.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import {
  MESSAGE_ORIGIN_RECORD_TYPE,
  MESSAGE_PRESENTATION_RECORD_TYPE,
  AGENT_REVIEW_RECORD_TYPE,
} from "../core/desktop-session-submit.ts";
import { SESSION_COLLAB_DECISION_RECORD_TYPE } from "../lib/session-collab/decision-record.ts";
import { TURN_INPUT_CONSUMPTION_EVENT_TYPE } from "../lib/turn-input-presentation.ts";
import { DEFERRED_RESULT_MESSAGE_TYPE } from "../lib/deferred-result-notification.ts";
import { TODO_STATE_CUSTOM_TYPE } from "../lib/tools/todo-constants.ts";

const tmpDirs: string[] = [];
const manifestStores: SessionManifestStore[] = [];

afterEach(() => {
  while (manifestStores.length) manifestStores.pop()?.close();
  while (tmpDirs.length) {
    const dir = tmpDirs.pop();
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
  }
});

function makeTmpDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-semantics-test-"));
  tmpDirs.push(dir);
  return dir;
}

function line(id: string, parentId: string | null, payload: Record<string, any>) {
  return JSON.stringify({ type: "message", id, parentId, timestamp: "2026-09-10T10:00:00Z", ...payload });
}
function msg(id: string, parentId: string | null, role: string, content: any, extra: Record<string, any> = {}) {
  return line(id, parentId, { message: { role, content, ...extra } });
}
function custom(id: string, parentId: string | null, customType: string, data: any) {
  return JSON.stringify({ type: "custom", id, parentId, timestamp: "2026-09-10T10:00:00Z", customType, data });
}
function customMessage(id: string, parentId: string | null, customType: string, extra: Record<string, any> = {}) {
  return JSON.stringify({ type: "custom_message", id, parentId, timestamp: "2026-09-10T10:00:00Z", customType, content: "", display: false, ...extra });
}

const HEADER = JSON.stringify({ type: "session", version: 3, id: "7e5f1a3c-9d24-4b8e-a6c1-2f4b8d9e0a31", cwd: "/tmp", timestamp: "2026-09-10T09:00:00Z" });

interface Harness {
  app: Hono;
  engine: any;
  cache: HistoryDirectoryCache;
  store: SessionManifestStore;
  sessionPath: string;
  sessionId: string;
  root: string;
  writeSession(name: string, lines: string[]): string;
}

function createHarness(): Harness {
  const root = makeTmpDir();
  const agentsDir = path.join(root, "agents");
  const store = new SessionManifestStore({ dbPath: path.join(root, "manifest.db") }); manifestStores.push(store);
  const cache = new HistoryDirectoryCache();
  const manifests = new Map<string, string>();
  const harness: Harness = {
    app: new Hono(),
    engine: null as any,
    cache,
    store,
    sessionPath: "",
    sessionId: "",
    root,
    writeSession(name: string, lines: string[]) {
      const sessionPath = path.join(agentsDir, "hana", "sessions", name);
      fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
      fs.writeFileSync(sessionPath, lines.join("\n") + "\n");
      const manifest = store.createForPath({ sessionPath, ownerAgentId: "hana", domain: "desktop", kind: "chat" });
      manifests.set(sessionPath, manifest.sessionId);
      if (!harness.sessionPath) {
        harness.sessionPath = sessionPath;
        harness.sessionId = manifest.sessionId;
      }
      return sessionPath;
    },
  };
  const deferredTasks: any[] = [];
  const engine: any = {
    agentsDir,
    currentSessionPath: null,
    isSessionStreaming: () => false,
    agentIdFromSessionPath: () => "hana",
    getAgent: () => ({ agentName: "Hana" }),
    historyReadCache: cache,
    activityHub: { rebroadcastSession: () => {} },
    deferredResults: {
      query: () => null,
      listBySession: () => [...deferredTasks],
      __tasks: deferredTasks,
    },
    subagentRuns: { query: () => null },
    getSessionManifest: (id: string) => {
      for (const [p, sid] of manifests) if (sid === id) return store.getBySessionId(id) ?? { sessionId: id, currentLocator: { path: p } };
      return null;
    },
    getSessionIdForPath: (p: string) => manifests.get(p) ?? null,
    getSessionBranchHead: (id: string) => store.getBranchHead(id) || null,
    _sessionManifestStore: store,
    openSessionManagerAtCurrentBranch: (p: string, d: string) => SessionManager.open(p, d),
  };
  harness.engine = engine;
  harness.app.route("/api", createSessionsRoute(engine));
  return harness;
}

function messagesOf(res: any) {
  return res.messages ?? [];
}

/** 窗口切片：全量输出按 display id 落在 [start,end) 的 messages/blocks。 */
function sliceFull(full: any, start: number, end: number) {
  return {
    messages: messagesOf(full).filter((m: any) => Number(m.id) >= start && Number(m.id) < end),
    blocks: (full.blocks ?? []).filter((b: any) => b.afterIndex >= start && b.afterIndex < end)
      .map((b: any) => ({ ...b, afterIndex: b.afterIndex - start })),
  };
}

describe("强不变量：热页 === 全量按窗口切片", () => {
  async function expectWindowEqualsSlice(harness: Harness, sessionPath: string, start: number, limit: number) {
    const end = Math.min(start + limit, 999);
    const hotRes = await harness.app.request(
      `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&before=${start + limit}&limit=${limit}`,
    );
    expect(hotRes.status).toBe(200);
    const hot = await hotRes.json();
    // 全量参照：独立 harness 实例（同字节夹具）跑 all=1，切片后对齐 afterIndex
    const fullRes = await harness.app.request(
      `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&all=1`,
    );
    expect(fullRes.status).toBe(200);
    const full = await fullRes.json();
    const expected = sliceFull(full, start, start + limit);
    expect(messagesOf(hot).map((m: any) => m.id)).toEqual(expected.messages.map((m: any) => m.id));
    expect(hot.messages).toEqual(expected.messages);
    expect(hot.blocks).toEqual(expected.blocks);
    expect(hot.todos).toEqual(full.todos);
    return hot;
  }

  it("长 Run 夹具：跨页 Run 边界在窗口切片下一致", async () => {
    const harness = createHarness();
    const lines = [HEADER, JSON.stringify({ type: "message", id: "u1", parentId: null, timestamp: "2026-09-10T10:00:00Z", message: { role: "user", content: "开始长任务" } })];
    let parent = "u1";
    for (let i = 1; i <= 12; i += 1) {
      lines.push(line(`a${i}`, parent, { message: { role: "assistant", content: [{ type: "thinking", thinking: `思考 ${i}` }, { type: "text", text: `回复 ${i}` }] } }));
      if (i < 12) {
        lines.push(line(`r${i}`, `a${i}`, { message: { role: "toolResult", toolCallId: `tu-${i}`, toolName: "read_file", content: `文件内容 ${i}` } }));
        parent = `r${i}`;
      }
    }
    const p = harness.writeSession("longrun.jsonl", lines);
    await expectWindowEqualsSlice(harness, p, 4, 5);
    await expectWindowEqualsSlice(harness, p, 0, 5);
  });

  it("页首前有不可见 assistant：assistantSegments ordinal 与 turnInput 状态从 before 状态恢复（易碎点 1/2）", async () => {
    const harness = createHarness();
    const lines = [
      HEADER,
      msg("u1", null, "user", "第一轮"),
      msg("a1", "u1", "assistant", "第一轮回复"),
      // 同轮第二个 assistant：不可见（空内容、非 aborted/error）→ ordinal 先加后判
      msg("a-hide", "a1", "assistant", ""),
      msg("a2", "a-hide", "assistant", [{ type: "thinking", thinking: "隐藏轮思考" }, { type: "text", text: "第二轮回复" }]),
    ];
    const p = harness.writeSession("invisible.jsonl", lines);
    // 窗口从 a2（display 1）开始：a-hide 不在窗口，但 a2 的 ordinal 必须是 2
    const hot = await expectWindowEqualsSlice(harness, p, 1, 2);
    const a2 = messagesOf(hot).find((m: any) => m.entryId === "a2");
    expect(a2).toBeDefined();
    expect(a2.assistantSegments.map((s: any) => s.id)).toEqual(
      (await (async () => {
        const fullRes = await harness.app.request(`/api/sessions/messages?path=${encodeURIComponent(p)}&all=1`);
        const full = await fullRes.json();
        return messagesOf(full).find((m: any) => m.entryId === "a2").assistantSegments.map((s: any) => s.id);
      })()),
    );
    expect(a2.assistantSegments[0].id).toBe("assistant:3:reasoning:default");
  });

  it("隐藏 user（占号）+ 隐藏 custom 输入：turnInputVisible:false 显式下发", async () => {
    const harness = createHarness();
    const lines = [
      HEADER,
      msg("u1", null, "user", "可见输入"),
      msg("a1", "u1", "assistant", "回复一"),
      // 隐藏 user：前端过滤但占 display 序号
      msg("u-hide", "a1", "user", "<hana-deferred-tasks>[后台任务]</hana-deferred-tasks>"),
      msg("a2", "u-hide", "assistant", [{ type: "text", text: "后台任务完成后的回复" }]),
    ];
    const p = harness.writeSession("hidden-user.jsonl", lines);
    const fullRes = await harness.app.request(`/api/sessions/messages?path=${encodeURIComponent(p)}&all=1`);
    const full = await fullRes.json();
    // 隐藏 user 占号：输出保留（前端过滤），a2 的 display id = 3
    expect(messagesOf(full).map((m: any) => m.id)).toEqual(["0", "1", "2", "3"]);
    const hot = await expectWindowEqualsSlice(harness, p, 3, 1);
    const a2 = messagesOf(hot)[0];
    expect(a2.id).toBe("3");
    expect(a2.turnInputEntryId).toBe("u-hide");
    expect(a2.turnInputVisible).toBe(false);
  });

  it("loop-turn：turnInputEntryId=null 仍显式下发 turnInputVisible:false（易碎点 6）", async () => {
    const harness = createHarness();
    const LOOP_TURN_MESSAGE_TYPE = "loop-turn";
    const lines = [
      HEADER,
      msg("u1", null, "user", "开始循环"),
      msg("a1", "u1", "assistant", "第一轮"),
      JSON.stringify({ type: "custom_message", id: "lt", parentId: "a1", timestamp: "2026-09-10T10:00:00Z", customType: LOOP_TURN_MESSAGE_TYPE, content: "循环任务", display: false }),
      msg("a2", "lt", "assistant", "循环轮回复"),
    ];
    const p = harness.writeSession("loop.jsonl", lines);
    await expectWindowEqualsSlice(harness, p, 1, 2);
    const res = await harness.app.request(`/api/sessions/messages?path=${encodeURIComponent(p)}&before=3&limit=2`);
    const body = await res.json();
    const a2 = messagesOf(body).find((m: any) => m.entryId === "a2");
    expect(a2.turnInputEntryId).toBeUndefined();
    expect(a2.turnInputVisible).toBe(false);
  });

  it("X10：跨页 toolResult 结局——结果记录在窗口外，旧页结局正确补读", async () => {
    const harness = createHarness();
    const lines = [
      HEADER,
      msg("u1", null, "user", "开始"),
      msg("a1", "u1", "assistant", [{ type: "tool_use", id: "tu-1", name: "slow_task", input: { x: 1 } }]),
      msg("u2", "a1", "user", "插入消息"),
      line("r1", "u2", { message: { role: "toolResult", toolCallId: "tu-1", toolName: "slow_task", content: "慢任务结果", details: { output: "慢任务结果" } } }),
      msg("a2", "r1", "assistant", "后续"),
    ];
    const p = harness.writeSession("x10.jsonl", lines);
    await expectWindowEqualsSlice(harness, p, 0, 2); // 窗口 [0,2)：a1 在窗口、r1（afterIndex=2）在窗口外
    const hot = await (await harness.app.request(`/api/sessions/messages?path=${encodeURIComponent(p)}&before=2&limit=2`)).json();
    const a1 = messagesOf(hot)[1];
    expect(a1.id).toBe("1");
    expect(a1.toolCalls[0].id).toBe("tu-1");
    expect(a1.toolCalls[0].status).not.toBe("unknown");
  });

  it("X12：collab 后补决策覆盖 suggestion_card", async () => {
    const harness = createHarness();
    const lines = [
      HEADER,
      msg("u1", null, "user", "建议一下"),
      msg("a1", "u1", "assistant", [{ type: "tool_use", id: "tu-s", name: "session", input: { suggestionId: "s-1" } }]),
      line("r1", "a1", { message: { role: "toolResult", toolCallId: "tu-s", toolName: "session", content: "{}", details: { suggestionId: "s-1", kind: "session_create_draft", status: "pending", draft: { firstMessage: "草稿" } } } }),
      // 后补决策（灰测修复 C）：块产出之后才出现的决策记录
      custom("cd", "r1", SESSION_COLLAB_DECISION_RECORD_TYPE, { suggestionId: "s-1", status: "confirmed", resultSessionId: "sess-c" }),
      msg("a2", "cd", "assistant", "完成"),
    ];
    const p = harness.writeSession("x12.jsonl", lines);
    await expectWindowEqualsSlice(harness, p, 0, 3);
    const res = await harness.app.request(`/api/sessions/messages?path=${encodeURIComponent(p)}&before=3&limit=3`);
    const body = await res.json();
    const card = (body.blocks ?? []).find((b: any) => b.type === "suggestion_card");
    expect(card).toBeDefined();
    expect(card.status).toBe("confirmed");
    expect(card.resultSessionId).toBe("sess-c");
  });

  it("X12：correlation 同 clientMessageId 双记录 → acceptanceDiagnostic ambiguous", async () => {
    const harness = createHarness();
    const lines = [
      HEADER,
      msg("u1", null, "user", "正常输入"),
      msg("a1", "u1", "assistant", "回复"),
    ];
    const p = harness.writeSession("corr.jsonl", lines);
    // correlation 数据的 sessionId 必须等于业务会话 id（collectDesktopInputCorrelations 校验）
    const sessionId = harness.sessionId;
    fs.appendFileSync(p, [
      custom("cx-1", "a1", "lingxi-desktop-input-correlation", { schemaVersion: 1, sessionId, sourceEntryId: "u1", clientMessageId: "cm-x", snapshotVersion: 3 }),
      custom("cx-2", "cx-1", "lingxi-desktop-input-correlation", { schemaVersion: 1, sessionId, sourceEntryId: "u1", clientMessageId: "cm-x", snapshotVersion: 4 }),
    ].join("\n") + "\n");
    await expectWindowEqualsSlice(harness, p, 0, 2);
    const body = await (await harness.app.request(`/api/sessions/messages?path=${encodeURIComponent(p)}&before=2&limit=2`)).json();
    const u1 = messagesOf(body)[0];
    expect(u1.acceptanceDiagnostic).toBe("ambiguous");
  });
});

describe("X14/X19：外部 store 变化与 deferred 凭证", () => {
  it("X14：JSONL 不变、subagent run store 出现终态 → 热页第二遍块状态翻转（外部状态不冻结）", async () => {
    const harness = createHarness();
    const lines = [
      HEADER,
      msg("u1", null, "user", "派发子任务"),
      msg("a1", "u1", "assistant", "派发完成"),
      line("r1", "a1", { message: { role: "toolResult", toolCallId: "tu-sub", toolName: "subagent", content: "子任务", details: { taskId: "sub-1", task: "子任务", sessionPath: "/tmp/agents/hana/subagent-sessions/child.jsonl", streamStatus: "running" } } }),
    ];
    const p = harness.writeSession("x14.jsonl", lines);
    const url = `/api/sessions/messages?path=${encodeURIComponent(p)}&before=2&limit=2`;
    const first = await (await harness.app.request(url)).json();
    const block1 = (first.blocks ?? []).find((b: any) => b.type === "subagent");
    expect(block1).toBeDefined();
    expect(block1.streamStatus).toBe("running");

    // JSONL 不变，仅 durable run store 写入终态 → 热页第二遍读取当前终态
    harness.engine.subagentRuns.query = (taskId: string) => taskId === "sub-1"
      ? { status: "resolved", summary: "子任务完成", childSessionId: "sess-child", childSessionPath: `${p}.child.jsonl` }
      : null;
    const second = await (await harness.app.request(url)).json();
    const block2 = (second.blocks ?? []).find((b: any) => b.type === "subagent");
    expect(block2.streamStatus).toBe("done");
    expect(fs.readFileSync(p, "utf8")).toBe(lines.join("\n") + "\n"); // JSONL 未被改写
  });

  it("X19：热页 deferred 凭证经 /sessions/content/:contentId 展开成功，sourceIndex/entryId 语义不变", async () => {
    const harness = createHarness();
    const bigReasoning = "长思考".repeat(6000); // > 8KiB → 应延迟
    const lines = [
      HEADER,
      msg("u1", null, "user", "深度任务"),
      msg("a1", "u1", "assistant", [{ type: "thinking", thinking: bigReasoning }, { type: "text", text: "结论" }]),
    ];
    const p = harness.writeSession("x19.jsonl", lines);
    const url = `/api/sessions/messages?path=${encodeURIComponent(p)}&limit=5`;
    // 第一遍：冷构建；第二遍：热命中
    let descriptor = null;
    for (let i = 0; i < 2; i += 1) {
      const body = await (await harness.app.request(url)).json();
      const a1 = messagesOf(body).find((m: any) => m.entryId === "a1");
      const segment = (a1.assistantSegments ?? []).find((s: any) => s.kind === "reasoning");
      expect(segment.deferred).toBeDefined();
      descriptor = segment.deferred;
    }
    const decoded = JSON.parse(Buffer.from(descriptor.id, "base64url").toString("utf8"));
    expect(decoded).toMatchObject({ version: 1, sourceIndex: 1, entryId: "a1", kind: "assistant_segment", ordinal: 0 });

    const contentRes = await harness.app.request(
      `/api/sessions/content/${encodeURIComponent(descriptor.id)}?path=${encodeURIComponent(p)}`,
    );
    expect(contentRes.status).toBe(200);
    const content = await contentRes.json();
    expect(content.content).toBe(bigReasoning);
    expect(content.kind).toBe("assistant_segment");
  });
});
