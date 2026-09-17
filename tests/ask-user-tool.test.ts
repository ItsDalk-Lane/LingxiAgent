/**
 * ask_user 工具行为测试。
 *
 * 覆盖：questions 载荷校验（数量/类型/label 唯一性防注入/推荐项指向）、
 * 确认通道缺失、确认块形状、confirmed 答案映射（labels/多选/未知值丢弃/text 截断）、
 * 超时自动选推荐并如实标注、dismiss 与 abort 是合法结果而非错误。
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/i18n.js", () => ({
  getLocale: () => "zh-CN",
  t: (key: string, values?: Record<string, unknown>) => {
    if (!values) return key;
    const suffix = Object.entries(values).map(([k, v]) => `${k}=${v}`).join(",");
    return `${key}:${suffix}`;
  },
}));

import { createAskUserTool } from "../lib/tools/ask-user-tool.ts";

const CTX = { sessionManager: { getSessionFile: () => "/tmp/sess-1" } };

/** 手动决议的确认存储：execute 阻塞在 promise 上，测试择机 resolve */
function deferredStore() {
  let resolveFn: ((v: any) => void) | null = null;
  const create = vi.fn((_kind: string, _payload: any, _ref: any, _timeoutMs?: number) => {
    const promise = new Promise((resolve) => { resolveFn = resolve; });
    return { confirmId: "confirm-1", promise };
  });
  return {
    create,
    resolve: (action: string, value?: any) => resolveFn?.({ action, value }),
  };
}

/** 立即以固定 action 决议的存储（timeout/rejected/aborted 路径） */
function instantStore(action: string, value?: any) {
  return {
    create: vi.fn(() => ({ confirmId: "confirm-1", promise: Promise.resolve({ action, value }) })),
  };
}

function makeTool(store: any, emitEvent?: any) {
  return createAskUserTool({
    getConfirmStore: () => store,
    getSessionPath: () => null, // 焦点兜底故意为空：必须走 ctx
    emitEvent: emitEvent ?? vi.fn(),
  });
}

const TWO_OPTIONS = [
  { value: "a", label: "方案 A", description: "稳妥但慢" },
  { value: "b", label: "方案 B" },
];

const SINGLE_QUESTION = [{
  key: "approach",
  question: "用哪种方案重构？",
  type: "single",
  options: TWO_OPTIONS,
  recommended: "a",
}];

describe("ask_user 工具", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("questions 为空数组直接拒绝，不创建确认", async () => {
    const store = deferredStore();
    const tool = makeTool(store);
    const res: any = await tool.execute("c1", { questions: [] }, null, null, CTX);
    expect(res.isError).toBe(true);
    expect(res.content[0].text).toContain("error.askUserInvalidQuestions");
    expect(store.create).not.toHaveBeenCalled();
  });

  it("超过 8 题拒绝", async () => {
    const store = deferredStore();
    const tool = makeTool(store);
    const questions = Array.from({ length: 9 }, (_, i) => ({ question: `问题 ${i + 1}` }));
    const res: any = await tool.execute("c1", { questions }, null, null, CTX);
    expect(res.isError).toBe(true);
    expect(store.create).not.toHaveBeenCalled();
  });

  it("同题选项 label 重复拒绝（防注入歧义）", async () => {
    const store = deferredStore();
    const tool = makeTool(store);
    const res: any = await tool.execute("c1", {
      questions: [{
        question: "选一个",
        options: [
          { value: "a", label: "一样的名字" },
          { value: "b", label: "一样的名字" },
        ],
      }],
    }, null, null, CTX);
    expect(res.isError).toBe(true);
    expect(res.details.reason).toContain("repeats option label");
  });

  it("推荐项指向不存在的选项拒绝", async () => {
    const store = deferredStore();
    const tool = makeTool(store);
    const res: any = await tool.execute("c1", {
      questions: [{
        question: "选一个",
        options: TWO_OPTIONS,
        recommended: "ghost",
      }],
    }, null, null, CTX);
    expect(res.isError).toBe(true);
    expect(res.details.reason).toContain("unknown option value");
  });

  it("text 题不允许携带 options", async () => {
    const store = deferredStore();
    const tool = makeTool(store);
    const res: any = await tool.execute("c1", {
      questions: [{ question: "说点什么", type: "text", options: TWO_OPTIONS }],
    }, null, null, CTX);
    expect(res.isError).toBe(true);
    expect(res.details.reason).toContain("cannot carry options");
  });

  it("确认通道缺失报 unavailable", async () => {
    const tool = createAskUserTool({ getConfirmStore: () => null, getSessionPath: () => null });
    const res: any = await tool.execute("c1", { questions: SINGLE_QUESTION }, null, null, CTX);
    expect(res.isError).toBe(true);
    expect(res.content[0].text).toBe("error.askUserUnavailable");
  });

  it("confirmed：确认块形状正确，答案映射回 label 并丢弃未知值", async () => {
    const store = deferredStore();
    const emitEvent = vi.fn();
    const tool = makeTool(store, emitEvent);
    const pending = tool.execute("c1", {
      questions: [
        ...SINGLE_QUESTION,
        {
          key: "extras",
          question: "还要哪些？",
          type: "multi",
          options: [
            { value: "x", label: "额外 X" },
            { value: "y", label: "额外 Y" },
          ],
          recommended: ["x"],
          required: false,
        },
        { key: "note", question: "补充说明", type: "text", required: false },
      ],
      timeout_seconds: 10, // 低于下限，应被钳到 30s
    }, null, null, CTX);

    // execute 阻塞中：确认已创建、卡片已广播
    expect(store.create).toHaveBeenCalledWith("ask_user", expect.any(Object), "/tmp/sess-1", 30_000);
    expect(emitEvent).toHaveBeenCalledTimes(1);
    const [event, sessionPath] = emitEvent.mock.calls[0];
    expect(sessionPath).toBe("/tmp/sess-1");
    const block = event.request;
    expect(block.kind).toBe("ask_user");
    expect(block.surface).toBe("input");
    expect(block.status).toBe("pending");
    expect(block.payload.questions).toHaveLength(3);
    // 规范化：recommended 统一成数组
    expect(block.payload.questions[0].recommended).toEqual(["a"]);

    store.resolve("confirmed", { approach: "b", extras: ["y", "ghost"], note: "  随便写写  " });
    const res: any = await pending;
    expect(res.isError).toBeUndefined();
    expect(res.details.timedOut).toBe(false);
    const [approach, extras, note] = res.details.answers;
    expect(approach.labels).toEqual(["方案 B"]);
    expect(extras.values).toEqual(["y"]); // ghost 被丢弃
    expect(extras.labels).toEqual(["额外 Y"]);
    expect(note.values).toEqual(["随便写写"]);
    expect(res.content[0].text).toContain("approval.askUser.answered");
    expect(res.content[0].text).toContain("方案 B");
  });

  it("timeout：有推荐的自动选并标注，无推荐的如实 unanswered", async () => {
    const store = instantStore("timeout");
    const tool = makeTool(store);
    const res: any = await tool.execute("c1", {
      questions: [
        ...SINGLE_QUESTION,
        { key: "free", question: "自由发挥", type: "text", required: false },
      ],
    }, null, null, CTX);
    expect(res.isError).toBeUndefined();
    expect(res.details.answered).toBe(true);
    expect(res.details.timedOut).toBe(true);
    const [approach, free] = res.details.answers;
    expect(approach.values).toEqual(["a"]);
    expect(approach.labels).toEqual(["方案 A"]);
    expect(approach.autoSelected).toBe(true);
    expect(free.unanswered).toBe(true);
    expect(free.autoSelected).toBe(false);
    expect(res.content[0].text).toContain("approval.askUser.timedOut");
    expect(res.content[0].text).toContain("approval.askUser.autoMarker");
  });

  it("rejected（暂不回答）是合法结果，不是错误", async () => {
    const store = instantStore("rejected");
    const tool = makeTool(store);
    const res: any = await tool.execute("c1", { questions: SINGLE_QUESTION }, null, null, CTX);
    expect(res.isError).toBeUndefined();
    expect(res.details.answered).toBe(false);
    expect(res.details.dismissed).toBe(true);
    expect(res.content[0].text).toBe("approval.askUser.dismissed");
  });

  it("aborted 如实回报", async () => {
    const store = instantStore("aborted");
    const tool = makeTool(store);
    const res: any = await tool.execute("c1", { questions: SINGLE_QUESTION }, null, null, CTX);
    expect(res.isError).toBeUndefined();
    expect(res.details.aborted).toBe(true);
    expect(res.content[0].text).toBe("approval.askUser.aborted");
  });

  it("read 级权限契约：任何模式放行（计划模式收工依赖它）", () => {
    const tool = makeTool(deferredStore());
    const resolved = tool.sessionPermission.resolveInvocation({});
    expect(resolved).toMatchObject({ kind: "read", capability: "ask_user.ask" });
  });
});
