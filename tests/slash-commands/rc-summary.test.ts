import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

// Mock callText before importing module under test
vi.mock("../../core/llm-client.js", () => ({
  callText: vi.fn(),
}));

import { callText } from "../../core/llm-client.ts";
import { summarizeSessionForRc } from "../../core/slash-commands/rc-summary.ts";
import { AuxiliaryConfigurationError } from "../../core/auxiliary-slots.ts";

let tmpFile;

function writeSessionFile(lines) {
  tmpFile = path.join(os.tmpdir(), `rc-summary-${Date.now()}-${Math.random().toString(36).slice(2)}.jsonl`);
  fs.writeFileSync(tmpFile, lines.map(l => JSON.stringify(l)).join("\n"));
  return tmpFile;
}

function makeUserMsg(text) {
  return { type: "message", message: { role: "user", content: [{ type: "text", text }] } };
}
function makeAssistantMsg(text, tools = []) {
  const blocks = [{ type: "text", text }];
  for (const name of tools) blocks.push({ type: "tool_use", name, input: {} } as any);
  return { type: "message", message: { role: "assistant", content: blocks } };
}

/**
 * Engine mock 现在忠实模拟统一 resolver 的契约：
 *   resolveAuxiliaryModelFresh("summarize", ctx)
 *     - summarizeResolved = {model,apiKey,baseUrl,api,headers}  → 调用方调用该模型
 *     - summarizeResolved = null                                  → 无可用模型
 *     - throw AuxiliaryConfigurationError                         → 显式配置错误
 *
 * 关键：调用方只应调用 resolveAuxiliaryModelFresh 一次。
 * resolveModelWithCredentialsFresh（chat 解析）是新架构下禁止的二次 fallback 入口，
 * 断言它在配置错误场景下绝不被调用。
 */
function makeEngine({ summarizeResolved, summarizeThrows }: any = {}) {
  return {
    resolveAuxiliaryModelFresh: summarizeThrows
      ? vi.fn(async () => { throw summarizeThrows; })
      : vi.fn(async () => summarizeResolved ?? null),
    resolveModelWithCredentialsFresh: vi.fn(async () => {
      throw new Error("chat resolve must not be reached — caller-side fallback forbidden");
    }),
    getSessionIdForPath: vi.fn(() => "sess_rc_summary"),
    usageLedger: { record: vi.fn() },
  };
}

function makeAgent(chatId = "gpt-5", provider = "openai") {
  return { config: { models: { chat: { id: chatId, provider } } } };
}

beforeEach(() => {
  (callText as any).mockReset();
});
afterEach(() => {
  if (tmpFile && fs.existsSync(tmpFile)) fs.unlinkSync(tmpFile);
  tmpFile = null;
});

describe("summarizeSessionForRc — 统一 resolver 收口（无 caller-side fallback）", () => {
  it("returns null when session path is missing", async () => {
    const r = await summarizeSessionForRc(makeEngine(), makeAgent(), "/does/not/exist.jsonl");
    expect(r).toBeNull();
    expect(callText).not.toHaveBeenCalled();
  });

  it("returns null when session is empty (no messages)", async () => {
    const p = writeSessionFile([]);
    const r = await summarizeSessionForRc(makeEngine(), makeAgent(), p);
    expect(r).toBeNull();
  });

  // ── RC-1：summarize 未配置 + chat 有效 → resolver 单次调用返回 chat 配置 ──
  it("RC-1: summarize 未配置时 resolver fallback 到 chat，调用方只 resolve 一次", async () => {
    const p = writeSessionFile([makeUserMsg("hi"), makeAssistantMsg("hello")]);
    (callText as any).mockResolvedValueOnce("chat-via-resolver summary");
    const engine = makeEngine({
      // resolver 对未配置 summarize 已 fallback 到 chat，返回完整 resolved。
      summarizeResolved: {
        model: "gpt-5", apiKey: "k", baseUrl: "https://x", api: "openai",
      },
    });
    const r = await summarizeSessionForRc(engine, makeAgent("gpt-5"), p);
    expect(r).toBe("chat-via-resolver summary");
    expect(callText).toHaveBeenCalledTimes(1);
    // 调用方只 resolve 一次——禁止二次手动解析 chat。
    expect(engine.resolveAuxiliaryModelFresh).toHaveBeenCalledTimes(1);
    expect(engine.resolveModelWithCredentialsFresh).not.toHaveBeenCalled();
  });

  // ── RC-2：summarize 显式有效 → 只调用 summarize 模型 ──
  it("RC-2: summarize 显式有效 → 只调用 summarize 模型，不碰 chat resolve", async () => {
    const p = writeSessionFile([makeUserMsg("hi"), makeAssistantMsg("hello")]);
    (callText as any).mockResolvedValueOnce("summarize summary");
    const engine = makeEngine({
      summarizeResolved: {
        model: "gpt-4o-mini",
        apiKey: "k", baseUrl: "https://x", api: "openai",
        headers: { "X-Provider-Protocol": "summarize" },
      },
    });
    const r = await summarizeSessionForRc(engine, makeAgent(), p);
    expect(r).toBe("summarize summary");
    expect(callText).toHaveBeenCalledTimes(1);
    expect((callText as any).mock.calls[0][0].headers).toEqual({ "X-Provider-Protocol": "summarize" });
    expect((callText as any).mock.calls[0][0]).not.toHaveProperty("maxTokens");
    expect(engine.resolveModelWithCredentialsFresh).not.toHaveBeenCalled();
  });

  it("records rc summary usage against sessionId while keeping the path locator", async () => {
    const p = writeSessionFile([makeUserMsg("hi"), makeAssistantMsg("hello")]);
    (callText as any).mockResolvedValueOnce("summarize summary");
    const engine = makeEngine({
      summarizeResolved: {
        model: "gpt-4o-mini",
        apiKey: "k",
        baseUrl: "https://x",
        api: "openai",
      },
    });

    await summarizeSessionForRc(engine, makeAgent(), p);

    expect(engine.getSessionIdForPath).toHaveBeenCalledWith(p);
    expect((callText as any).mock.calls[0][0].usageContext.attribution).toMatchObject({
      kind: "session",
      agentId: null,
      sessionId: "sess_rc_summary",
      sessionPath: p,
    });
  });

  // ── RC-3（最重要）：summarize 显式配置错误 + chat 有效 → 不调用 chat ──
  it("RC-3: summarize 显式配置错误 + chat 有效 → resolver throw → 不调用 chat，返回 null", async () => {
    const p = writeSessionFile([makeUserMsg("hi"), makeAssistantMsg("hello")]);
    const engine = makeEngine({
      summarizeThrows: new AuxiliaryConfigurationError(
        "已配置 summarize 模型 \"ollama/non-existent\"，但无法解析。",
        "model_not_found",
        "summarize",
      ),
    });
    const r = await summarizeSessionForRc(engine, makeAgent("gpt-5"), p);
    expect(r).toBeNull();
    // 显式配置错误时 resolver 抛错 → 调用方不得 fallback chat。
    expect(callText).not.toHaveBeenCalled();
    expect(engine.resolveModelWithCredentialsFresh).not.toHaveBeenCalled();
  });

  // ── RC-4：summarize 最终模型调用 timeout → return null，禁止第二次改用 chat ──
  it("RC-4: summarize 模型运行时 timeout → return null，LLM 调用恰好 1 次，不 fallback chat", async () => {
    const p = writeSessionFile([makeUserMsg("hi"), makeAssistantMsg("hello")]);
    (callText as any).mockRejectedValueOnce(new Error("timeout"));
    const engine = makeEngine({
      summarizeResolved: {
        model: "gpt-4o-mini",
        apiKey: "k", baseUrl: "https://x", api: "openai",
      },
    });
    const r = await summarizeSessionForRc(engine, makeAgent("gpt-5"), p);
    expect(r).toBeNull();
    // 运行时失败允许 best-effort 返回 null，但禁止第二次改用 chat。
    expect(callText).toHaveBeenCalledTimes(1);
    expect(engine.resolveModelWithCredentialsFresh).not.toHaveBeenCalled();
  });

  it("utility config without api_key still runs when the resolver approved it", async () => {
    const p = writeSessionFile([makeUserMsg("hi"), makeAssistantMsg("hello")]);
    (callText as any).mockResolvedValueOnce("from header-only utility");
    const engine = makeEngine({
      summarizeResolved: {
        model: "u",
        apiKey: "",
        baseUrl: "https://x", api: "openai",
        headers: { "X-Gateway-Auth": "resolved-header" },
      },
    });
    const r = await summarizeSessionForRc(engine, makeAgent(), p);
    expect(r).toBe("from header-only utility");
    expect(callText).toHaveBeenCalledTimes(1);
    expect((callText as any).mock.calls[0][0].headers).toEqual({
      "X-Gateway-Auth": "resolved-header",
    });
  });

  it("trims whitespace on success", async () => {
    const p = writeSessionFile([makeUserMsg("hi"), makeAssistantMsg("hello")]);
    (callText as any).mockResolvedValueOnce("  padded summary  \n");
    const engine = makeEngine({
      summarizeResolved: {
        model: "u",
        apiKey: "k", baseUrl: "https://x", api: "openai",
      },
    });
    const r = await summarizeSessionForRc(engine, makeAgent(), p);
    expect(r).toBe("padded summary");
  });

  it("asks for a concise but useful Chinese summary around 100 characters", async () => {
    const p = writeSessionFile([
      makeUserMsg("帮我检查远程控制的摘要为什么太短"),
      makeAssistantMsg("我正在查看 /rc 接管后的摘要生成逻辑，准备调整提示词。", ["read"]),
    ]);
    (callText as any).mockResolvedValueOnce("正在调整 /rc 摘要提示词，重点补足当前进展和下一步线索。");
    const engine = makeEngine({
      summarizeResolved: {
        model: "u",
        apiKey: "k", baseUrl: "https://x", api: "openai",
      },
    });

    await summarizeSessionForRc(engine, makeAgent(), p);

    const system = (callText as any).mock.calls[0][0].messages[0].content;
    expect(system).toContain("目标约 100 字");
    expect(system).toContain("60-200 字");
    expect(system).toContain("当前进展");
    expect(system).toContain("下一步线索");
    expect(system).not.toContain("40 字以内");
  });

  it("repairs an overlong result without falling through to another model", async () => {
    const p = writeSessionFile([
      makeUserMsg("帮我检查远程控制的摘要为什么太短"),
      makeAssistantMsg("我正在查看 /rc 接管后的摘要生成逻辑，准备调整提示词。", ["read"]),
    ]);
    const overlong = `${"非常".repeat(140)}长的摘要。`;
    (callText as any)
      .mockResolvedValueOnce(overlong)
      .mockResolvedValueOnce("正在调整 /rc 摘要提示词，补足当前进展和下一步线索。");
    const engine = makeEngine({
      summarizeResolved: {
        model: "u",
        apiKey: "k", baseUrl: "https://x", api: "openai",
      },
    });

    const r = await summarizeSessionForRc(engine, makeAgent(), p);

    expect(r).toBe("正在调整 /rc 摘要提示词，补足当前进展和下一步线索。");
    expect(callText).toHaveBeenCalledTimes(2);
    expect((callText as any).mock.calls[1][0]).toMatchObject({
      api: "openai",
      model: "u",
      apiKey: "k",
      baseUrl: "https://x",
    });
    expect((callText as any).mock.calls[1][0]).not.toHaveProperty("maxTokens");
    expect(engine.resolveModelWithCredentialsFresh).not.toHaveBeenCalled();
  });

  // ── 反向守卫：resolver 返回 null（未配置且 chat 缺失）→ 不调用任何模型 ──
  it("resolver 返回 null（无可用模型）→ 返回 null，不调用 LLM，不 fallback chat", async () => {
    const p = writeSessionFile([makeUserMsg("hi"), makeAssistantMsg("hello")]);
    const engine = makeEngine({ summarizeResolved: null });
    const r = await summarizeSessionForRc(engine, makeAgent(), p);
    expect(r).toBeNull();
    expect(callText).not.toHaveBeenCalled();
    expect(engine.resolveModelWithCredentialsFresh).not.toHaveBeenCalled();
  });
});
