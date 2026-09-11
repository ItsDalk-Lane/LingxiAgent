// @vitest-environment jsdom

/**
 * 历史 Run 结局与跨页身份边界（T06/T07/T10/T12/T14）回归。
 *
 * 与 history-pagination-run-continuity.test.ts 同一真实链路（真实路由 + 真实投影 +
 * 真实 chat-slice 合并），聚焦结局裁决与 Run 归属的边界形状：
 *  - T07 真完成但无用户可见交付 → 恰一个 missing_final_answer；
 *  - T10 中间失败恢复成功不被放大；真终态失败与部分正文共存；
 *  - T12 相邻不同 Run 不误合并；
 *  - T06 活跃流最新 Run 不派生终态（openTailRun）；
 *  - T14 文件 blocks 随 Run 片段缝合正确归属、不重复。
 */

import { Hono } from "hono";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

const harness = vi.hoisted(() => ({ app: null as any }));
vi.mock("../desktop/src/react/hooks/use-hana-fetch.ts", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return {
    ...original,
    lingxiFetch: async (url: string, opts?: any) => harness.app.request(url, opts),
  };
});

const { useStore } = await import("../desktop/src/react/stores/index.ts");
const { sessionScopedValue } = await import("../desktop/src/react/stores/session-slice.ts");
const { buildItemsFromHistory } = await import("../desktop/src/react/utils/history-builder.ts");

interface FixtureEntry {
  role: string;
  content?: unknown;
  stopReason?: string;
  toolCallId?: string;
  toolName?: string;
  details?: Record<string, unknown>;
}

function writeSession(sessionPath: string, entries: FixtureEntry[]): void {
  const lines: string[] = [
    JSON.stringify({
      type: "session",
      version: 3,
      id: "sess_edges",
      cwd: "/tmp",
      timestamp: "2026-09-10T09:00:00Z",
    }),
  ];
  let parent: string | null = null;
  let n = 0;
  for (const entry of entries) {
    n += 1;
    const id = `e${n}`;
    const message: Record<string, unknown> = { role: entry.role };
    if (entry.content !== undefined) message.content = entry.content;
    if (entry.stopReason !== undefined) message.stopReason = entry.stopReason;
    if (entry.toolCallId !== undefined) message.toolCallId = entry.toolCallId;
    if (entry.toolName !== undefined) message.toolName = entry.toolName;
    if (entry.details !== undefined) message.details = entry.details;
    lines.push(
      JSON.stringify({
        type: "message",
        id,
        parentId: parent,
        timestamp: "2026-09-10T10:00:00Z",
        message,
      }),
    );
    parent = id;
  }
  fs.writeFileSync(sessionPath, lines.join("\n"), "utf8");
}

const THINK = (i: number) => [{ type: "thinking", thinking: `思考 ${i}` }];
const TOOL = (i: number, name = "probe_tool") => [
  { type: "thinking", thinking: `思考 ${i}` },
  { type: "tool_use", id: `tu-${i}`, name, input: {} },
];
const TEXT = (text: string, phase: "commentary" | "final_answer" = "final_answer") => [
  { type: "text", text, ...(phase === "commentary" ? { textSignature: '{"phase":"commentary"}' } : {}) },
];

async function fetchAllJsonl(sessionPath: string) {
  const res = await harness.app.request(
    `/api/sessions/messages?path=${encodeURIComponent(sessionPath)}&all=1`,
  );
  expect(res.status).toBe(200);
  return res.json();
}

function project(data: any, options?: { openTailRun?: boolean }) {
  return buildItemsFromHistory(data, options);
}

function assistantItems(items: any[]) {
  return items.filter((item) => item.type === "message" && item.data.role === "assistant");
}

function turnStatusBlocks(message: any) {
  return (message.data.blocks || []).filter((block: any) => block.type === "turn_status");
}

describe("历史 Run 结局与跨页身份边界", () => {
  let agentsDir: string;
  let sessionPath: string;

  beforeEach(async () => {
    agentsDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-history-edges-"));
    sessionPath = path.join(agentsDir, "hana", "sessions", "edges.jsonl");
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
    const { createSessionsRoute } = await import("../server/routes/sessions.ts");
    const app = new Hono();
    app.route(
      "/api",
      createSessionsRoute({
        agentsDir,
        currentSessionPath: null,
        isSessionStreaming: () => false,
        agentIdFromSessionPath: () => "hana",
        getAgent: () => ({ agentName: "Hana" }),
        getSessionWorkspaceMount: () => null,
        getSessionManifest: (id: string) =>
          id === "sess_edges" ? { currentLocator: { path: sessionPath } } : null,
      }),
    );
    harness.app = app;
    useStore.setState({ chatSessions: {} } as never);
  });

  afterEach(() => {
    fs.rmSync(agentsDir, { recursive: true, force: true });
  });

  it("T07：真完成但只有过程（思考+工具）→ 恰一个 missing_final_answer，不因有工具豁免", async () => {
    writeSession(sessionPath, [
      { role: "user", content: "执行任务" },
      { role: "assistant", content: THINK(1) },
      { role: "toolResult", toolCallId: "tu-1", toolName: "probe_tool", details: { ok: true } },
      { role: "assistant", content: THINK(2) },
    ]);
    const items = project(await fetchAllJsonl(sessionPath));
    const assistants = assistantItems(items);
    expect(assistants.length).toBe(1);
    const status = turnStatusBlocks(assistants[0]);
    expect(status.length).toBe(1);
    expect(status[0]).toMatchObject({ status: "missing_final_answer" });
    expect(assistants[0].data.turnProjection.outcome).toBe("completed_without_user_output");
    expect(assistants[0].data.turnProjection.missingFinalAnswerReason).toBe("only_process_blocks");
  });

  it("T10a：中间失败后恢复成功 → 整轮完成，不被中间错误放大为失败", async () => {
    writeSession(sessionPath, [
      { role: "user", content: "长任务" },
      { role: "assistant", content: TOOL(1) },
      { role: "assistant", content: TEXT("第一次尝试失败", "commentary"), stopReason: "error" },
      { role: "assistant", content: TOOL(2) },
      { role: "assistant", content: TEXT("最终成功结论") },
    ]);
    const items = project(await fetchAllJsonl(sessionPath));
    const assistants = assistantItems(items);
    expect(assistants.length).toBe(1);
    expect(turnStatusBlocks(assistants[0])).toEqual([]);
    expect(assistants[0].data.turnProjection.outcome).toBe("completed_with_answer");
  });

  it("T10b：最终失败带部分正文 → failed 终态与部分正文共存", async () => {
    writeSession(sessionPath, [
      { role: "user", content: "长任务" },
      { role: "assistant", content: TOOL(1) },
      { role: "assistant", content: TEXT("部分结论，但中断了"), stopReason: "error" },
    ]);
    const items = project(await fetchAllJsonl(sessionPath));
    const assistants = assistantItems(items);
    expect(assistants.length).toBe(1);
    const status = turnStatusBlocks(assistants[0]);
    expect(status.length).toBe(1);
    expect(status[0]).toMatchObject({ status: "failed" });
    expect(assistants[0].data.turnProjection.outcome).toBe("failed");
    // 部分正文仍在答案区可见，不被终态块吞掉。
    const answer = (assistants[0].data.blocks || []).filter(
      (b: any) => b.type === "text" && b.surfaceRole === "answer",
    );
    expect(answer.length).toBe(1);
    expect(answer[0].source).toContain("部分结论");
  });

  it("T12：相邻不同 Run（两次用户输入）不误合并，各自持有自己的结局", async () => {
    writeSession(sessionPath, [
      { role: "user", content: "第一问" },
      { role: "assistant", content: THINK(1) }, // run A：过程 only → 一个无回复提示
      { role: "user", content: "第二问" },
      { role: "assistant", content: TEXT("第二问的回答") }, // run B：有答案
    ]);
    const items = project(await fetchAllJsonl(sessionPath));
    const assistants = assistantItems(items);
    expect(assistants.length).toBe(2);
    expect(turnStatusBlocks(assistants[0]).map((b: any) => b.status)).toEqual(["missing_final_answer"]);
    expect(turnStatusBlocks(assistants[1])).toEqual([]);
    expect(assistants[1].data.turnProjection.outcome).toBe("completed_with_answer");
  });

  it("T06：流仍活跃时（openTailRun）最新 Run 组不派生终态；流结束后同一事实恢复终态", async () => {
    writeSession(sessionPath, [
      { role: "user", content: "正在跑的长任务" },
      { role: "assistant", content: THINK(1) },
      { role: "assistant", content: THINK(2) },
    ]);
    const data = await fetchAllJsonl(sessionPath);
    const open = assistantItems(project(data, { openTailRun: true }));
    expect(open.length).toBe(1);
    expect(turnStatusBlocks(open[0])).toEqual([]);
    expect(open[0].data.turnProjection.outcome).toBeUndefined();

    const settled = assistantItems(project(data));
    expect(turnStatusBlocks(settled[0]).map((b: any) => b.status)).toEqual(["missing_final_answer"]);
  });

  it("T14：文件 blocks 随 Run 片段缝合归属正确、跨页不重复", async () => {
    // 一个 Run：[思考+工具+stage_files 文件产出, ...(若干记录)..., 最终答案]，
    // 限页 4 强制跨页；文件块锚定第 2 条助手记录。
    const entries: FixtureEntry[] = [{ role: "user", content: "生成文件并总结" }];
    for (let i = 1; i <= 6; i += 1) {
      if (i === 2) {
        entries.push({
          role: "assistant",
          content: [
            { type: "thinking", thinking: "准备产出文件" },
            { type: "tool_use", id: "tu-2", name: "stage_files", input: {} },
          ],
        });
        entries.push({
          role: "toolResult",
          toolCallId: "tu-2",
          toolName: "stage_files",
          details: { files: [{ filePath: "/tmp/report.md", label: "report.md", ext: "md" }] },
        });
      } else if (i === 6) {
        entries.push({ role: "assistant", content: TEXT("总结：文件已生成。") });
      } else {
        entries.push({ role: "assistant", content: THINK(i) });
      }
    }
    writeSession(sessionPath, entries);

    // 手动镜像 loadMessages/loadMoreMessages 的分页驱动（limit=4 强制切页）。
    const fetchPage = async (before?: string) => {
      const q = new URLSearchParams({ path: sessionPath, limit: "4" });
      if (before) q.set("before", before);
      const res = await harness.app.request(`/api/sessions/messages?${q}`);
      expect(res.status).toBe(200);
      return res.json();
    };
    const first = await fetchPage();
    useStore.getState().initSession(sessionPath, buildItemsFromHistory(first), first.hasMore ?? false, null, first.nextBefore ?? undefined);
    let guard = 0;
    for (;;) {
      guard += 1;
      if (guard > 20) throw new Error("分页失控");
      const state: any = useStore.getState();
      const session: any = sessionScopedValue(state, state.chatSessions, sessionPath);
      if (!session?.hasMore) break;
      const page = await fetchPage(session.nextBefore ?? session.oldestId ?? undefined);
      useStore.getState().prependItems(sessionPath, buildItemsFromHistory(page), page.hasMore ?? false, page.nextBefore ?? undefined);
    }

    const state: any = useStore.getState();
    const session: any = sessionScopedValue(state, state.chatSessions, sessionPath);
    const assistants = assistantItems(session.items);
    expect(assistants.length).toBe(1);
    const fileBlocks = (assistants[0].data.blocks || []).filter((b: any) => b.type === "file");
    expect(fileBlocks.length).toBe(1);
    expect(fileBlocks[0].filePath).toBe("/tmp/report.md");

    // 与全量恢复逐块等价。
    const full = assistantItems(project(await fetchAllJsonl(sessionPath)));
    expect(full.length).toBe(1);
    expect(assistants[0].data.blocks.map((b: any) => [b.id, b.type])).toEqual(
      full[0].data.blocks.map((b: any) => [b.id, b.type]),
    );
  });
});
