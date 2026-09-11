/**
 * 轮中节流提醒（机制 3）：buildTodoContextReminder + installTodoContextReminder。
 *
 * - 距上次 todo_write ≥10 个工具调用且快照有未完成项 → 生成 system-reminder 文本。
 * - 节流：距上次注入 <10 个工具调用 → 不重复注入。
 * - 清单为空 / 已结束 / 已收纳 / 无快照 → 不注入。
 * - transformContext 链：命中时在末尾追加 ephemeral user 消息；失败不阻断。
 */
import { describe, it, expect } from "vitest";
import {
  buildTodoContextReminder,
  installTodoContextReminder,
} from "../lib/pi-sdk/todo-context-reminder.ts";
import { TODO_FORMAT_VERSION } from "../lib/tools/todo-constants.ts";

const UNFINISHED = [
  { content: "读 spec", activeForm: "正在读 spec", status: "completed" },
  { content: "改代码", activeForm: "正在改代码", status: "in_progress" },
];

function todoWriteResult(id: string, todos: any[]) {
  return {
    role: "toolResult",
    toolCallId: id,
    toolName: "todo_write",
    content: [{ type: "text", text: "ok" }],
    isError: false,
    timestamp: 1,
    details: { todos, todoVersion: TODO_FORMAT_VERSION },
  };
}

function otherToolResult(id: string) {
  return {
    role: "toolResult",
    toolCallId: id,
    toolName: "bash",
    content: [{ type: "text", text: "ok" }],
    isError: false,
    timestamp: 1,
  };
}

/** todo_write 后接 n 个其他工具调用的上下文。 */
function contextAfterTodoWrite(n: number, todos: any[] = UNFINISHED) {
  const messages: any[] = [
    { role: "user", content: [{ type: "text", text: "go" }], timestamp: 1 },
    todoWriteResult("tw-1", todos),
  ];
  for (let i = 0; i < n; i += 1) messages.push(otherToolResult(`b-${i}`));
  return messages;
}

const freshState = () => ({ toolCallCountAtLastReminder: -10 });

describe("buildTodoContextReminder", () => {
  it("距上次 todo_write ≥10 且有未完成项：生成提醒文本", () => {
    const text = buildTodoContextReminder(contextAfterTodoWrite(10), freshState());
    expect(text).toBeTruthy();
    expect(text).toContain("<system-reminder>");
    expect(text).toContain("1. [completed] 读 spec");
    expect(text).toContain("2. [in_progress] 改代码");
    expect(text).toMatch(/hasn't been updated recently/i);
    expect(text).toMatch(/Never mention this reminder/i);
  });

  it("距上次 todo_write <10：不注入", () => {
    expect(buildTodoContextReminder(contextAfterTodoWrite(9), freshState())).toBeNull();
    expect(buildTodoContextReminder(contextAfterTodoWrite(0), freshState())).toBeNull();
  });

  it("节流：注入后 <10 个工具调用内不重复注入", () => {
    const state = freshState();
    const first = buildTodoContextReminder(contextAfterTodoWrite(10), state);
    expect(first).toBeTruthy();
    // 距上次注入只有 1 个工具调用（即便距 write 已 11 个）→ 不注入
    expect(buildTodoContextReminder(contextAfterTodoWrite(11), state)).toBeNull();
    // 距上次注入满 10 → 再次注入
    expect(buildTodoContextReminder(contextAfterTodoWrite(20), state)).toBeTruthy();
  });

  it("清单全部完成：不注入", () => {
    const done = UNFINISHED.map((td) => ({ ...td, status: "completed" }));
    expect(buildTodoContextReminder(contextAfterTodoWrite(12, done), freshState())).toBeNull();
  });

  it("无清单历史 / 空上下文：不注入", () => {
    const messages = [otherToolResult(`b-1`), ...Array.from({ length: 11 }, (_, i) => otherToolResult(`x-${i}`))];
    expect(buildTodoContextReminder(messages, freshState())).toBeNull();
    expect(buildTodoContextReminder([], freshState())).toBeNull();
    expect(buildTodoContextReminder(null as any, freshState())).toBeNull();
  });

  it("todo_write 后又写了新清单：以最新快照为准，重新计距", () => {
    const messages = [
      ...contextAfterTodoWrite(8),
      todoWriteResult("tw-2", UNFINISHED),
      otherToolResult("b-new"),
    ];
    // 距最新 write 只有 1 个工具调用 → 不注入
    expect(buildTodoContextReminder(messages, freshState())).toBeNull();
  });
});

describe("installTodoContextReminder", () => {
  function fakeSession(transformed?: any[]) {
    const calls: any[] = [];
    return {
      calls,
      agent: {
        transformContext: async (messages: any[]) => {
          calls.push(messages);
          return transformed ?? messages;
        },
      },
    };
  }

  it("命中条件：在链式结果末尾追加 ephemeral user 消息", async () => {
    const session = fakeSession();
    installTodoContextReminder(session);
    const out = await session.agent.transformContext(contextAfterTodoWrite(10));
    const last = out[out.length - 1];
    expect(last.role).toBe("user");
    expect(last.content[0].text).toContain("<system-reminder>");
  });

  it("未命中：透传上游结果，不追加", async () => {
    const session = fakeSession();
    installTodoContextReminder(session);
    const input = contextAfterTodoWrite(3);
    const out = await session.agent.transformContext(input);
    expect(out).toHaveLength(input.length);
  });

  it("重复安装不会叠加多条链", async () => {
    const session = fakeSession();
    installTodoContextReminder(session);
    installTodoContextReminder(session);
    const out = await session.agent.transformContext(contextAfterTodoWrite(10));
    const reminders = out.filter(
      (m: any) => m.role === "user" && String(m.content?.[0]?.text || "").includes("<system-reminder>"),
    );
    expect(reminders).toHaveLength(1);
  });

  it("上游 transformContext 抛错：不阻断，提醒链整体由调用方处理", async () => {
    const session = {
      agent: {
        transformContext: async () => {
          throw new Error("upstream boom");
        },
      },
    };
    installTodoContextReminder(session);
    await expect((session.agent.transformContext as any)([])).rejects.toThrow("upstream boom");
  });

  it("无 agent 的会话：静默跳过", () => {
    expect(() => installTodoContextReminder(null)).not.toThrow();
    expect(() => installTodoContextReminder({})).not.toThrow();
  });
});
