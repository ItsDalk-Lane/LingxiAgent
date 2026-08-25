import { describe, expect, it } from "vitest";

import {
  computeContextUsageEstimate,
  reconcileContextUsageBreakdown,
  sanitizeContextUsageEstimate,
} from "../lib/llm/context-usage-breakdown.ts";
import { estimateTextTokens } from "../lib/llm/estimate-text-tokens.ts";
import { estimateTokens } from "../lib/pi-sdk/index.ts";

function userMessage(text): any {
  return { role: "user", content: text, timestamp: 0 };
}

function assistantMessage(text): any {
  return {
    role: "assistant",
    content: [{ type: "text", text }],
    api: "openai-completions",
    provider: "test",
    model: "test-model",
    usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { total: 0 } },
    stopReason: "stop",
    timestamp: 0,
  };
}

function toolResultMessage(text): any {
  return {
    role: "toolResult",
    toolCallId: "call_1",
    toolName: "read",
    content: [{ type: "text", text }],
    isError: false,
    timestamp: 0,
  };
}

function tool(name, description = "desc") {
  return {
    name,
    description,
    parameters: { type: "object", properties: { query: { type: "string" } } },
  };
}

function toolSchemaTokens(t) {
  return estimateTextTokens(`${t.name}\n${t.description}\n${JSON.stringify(t.parameters)}`);
}

const PROJECT_CONTEXT_SEGMENT = [
  "<project_context>",
  "Project-specific instructions and guidelines:",
  '<project_instructions path="AGENTS.md">Be concise.</project_instructions>',
  "</project_context>",
].join("\n");

const SKILLS_SEGMENT = [
  "The following skills provide specialized instructions for specific tasks.",
  "Use the read tool to load a skill's file when the task matches its description.",
  "",
  "<available_skills>",
  "  <skill><name>quiet-musing</name></skill>",
  "</available_skills>",
].join("\n");

function knownCategorySum(estimate) {
  return estimate.system + estimate.skills + estimate.files + estimate.tools
    + estimate.mcp + estimate.conversation + estimate.user + estimate.toolResults;
}

describe("computeContextUsageEstimate", () => {
  it("classifies system prompt segments, messages, and tools by real source", () => {
    const systemBody = "You are Lingxi, a personal AI assistant.";
    const systemPrompt = `${systemBody}\n\n${PROJECT_CONTEXT_SEGMENT}\n${SKILLS_SEGMENT}\nCurrent working directory: /tmp`;

    const historyUser = userMessage("u".repeat(8));
    const assistant = assistantMessage("a".repeat(16));
    const toolResult = toolResultMessage("r".repeat(12));
    const currentUser = userMessage("c".repeat(20));

    const builtinTool = tool("read");
    const pluginTool = tool("office_export");
    const mcpTool = tool("mcp_web_search");
    const mcpBridge = tool("mcp_call");

    const estimate = computeContextUsageEstimate({
      systemPrompt,
      messages: [historyUser, assistant, toolResult, currentUser],
      tools: [builtinTool, pluginTool, mcpTool, mcpBridge],
    });

    // systemPrompt 分段:project_context → files,skills listing → skills,其余主体 → system。
    const expectedRest = `${systemBody}\n\n` + `\n` + `\nCurrent working directory: /tmp`;
    expect(estimate.system).toBe(estimateTextTokens(expectedRest));
    expect(estimate.files).toBe(estimateTextTokens(PROJECT_CONTEXT_SEGMENT));
    expect(estimate.skills).toBe(estimateTextTokens(SKILLS_SEGMENT));

    // 消息:历史 user/assistant → conversation,toolResult → toolResults,最后一条 user → user。
    expect(estimate.conversation).toBe(estimateTokens(historyUser) + estimateTokens(assistant));
    expect(estimate.user).toBe(estimateTokens(currentUser));
    expect(estimate.user).toBe(5); // "c".repeat(20) → ceil(20/4),口径与 pi estimateTokens 一致
    expect(estimate.toolResults).toBe(estimateTokens(toolResult));

    // 工具:mcp_ 前缀(含 mcp bridge)→ mcp,其余 → tools。
    expect(estimate.tools).toBe(toolSchemaTokens(builtinTool) + toolSchemaTokens(pluginTool));
    expect(estimate.mcp).toBe(toolSchemaTokens(mcpTool) + toolSchemaTokens(mcpBridge));

    expect(estimate.computedAt).toBeGreaterThan(0);
  });

  it("does not count deferred tools whose schema never enters the request", () => {
    // deferred 装配(engine._planDeferredToolAssembly)超过阈值时,deferred 工具的
    // schema 不进 Context.tools,只剩 bridge 三件套。统计只能看到、也只统计请求体里
    // 真实存在的工具。
    const estimate = computeContextUsageEstimate({
      systemPrompt: "sys",
      messages: [userMessage("hi")],
      tools: [tool("read"), tool("mcp_search_tools"), tool("mcp_describe_tool"), tool("mcp_call")],
    });

    expect(estimate.tools).toBe(toolSchemaTokens(tool("read")));
    expect(estimate.mcp).toBe(
      toolSchemaTokens(tool("mcp_search_tools"))
        + toolSchemaTokens(tool("mcp_describe_tool"))
        + toolSchemaTokens(tool("mcp_call")),
    );
  });

  it("keeps unmarked prompt text in system when markers are missing or unclosed", () => {
    const prompt = "plain system prompt with <project_context> but no closing tag";
    const estimate = computeContextUsageEstimate({ systemPrompt: prompt, messages: [], tools: [] });
    expect(estimate.system).toBe(estimateTextTokens(prompt));
    expect(estimate.files).toBe(0);
    expect(estimate.skills).toBe(0);
  });

  it("treats a missing system prompt and missing tools as zero, not an error", () => {
    const estimate = computeContextUsageEstimate({ messages: [userMessage("hello")] });
    expect(estimate.system).toBe(0);
    expect(estimate.tools).toBe(0);
    expect(estimate.mcp).toBe(0);
    expect(estimate.user).toBe(estimateTokens(userMessage("hello")));
  });

  it("classifies the last user message as user even when a toolResult follows it", () => {
    const currentUser = userMessage("current question");
    const estimate = computeContextUsageEstimate({
      systemPrompt: "sys",
      messages: [currentUser, toolResultMessage("tool output")],
      tools: [],
    });
    expect(estimate.user).toBe(estimateTokens(currentUser));
    expect(estimate.toolResults).toBe(estimateTokens(toolResultMessage("tool output")));
    expect(estimate.conversation).toBe(0);
  });
});

describe("reconcileContextUsageBreakdown", () => {
  const estimate = {
    system: 100,
    skills: 40,
    files: 30,
    tools: 60,
    mcp: 20,
    conversation: 200,
    user: 10,
    toolResults: 50,
    computedAt: 123,
  };

  it("closes sum(categories) + other to the real total", () => {
    const breakdown = reconcileContextUsageBreakdown(estimate, 600);
    expect(breakdown).not.toBeNull();
    expect(breakdown.other).toBe(600 - knownCategorySum(estimate));
    expect(knownCategorySum(breakdown) + breakdown.other).toBe(breakdown.total);
    expect(breakdown.total).toBe(600);
    expect(breakdown.computedAt).toBe(123);
  });

  it("clamps other at zero when the estimate overshoots the total", () => {
    const breakdown = reconcileContextUsageBreakdown(estimate, knownCategorySum(estimate) - 10);
    expect(breakdown.other).toBe(0);
    expect(breakdown.total).toBe(knownCategorySum(estimate) - 10);
  });

  it("returns null when the total is unknown (post-compaction) instead of inventing detail", () => {
    expect(reconcileContextUsageBreakdown(estimate, null)).toBeNull();
    expect(reconcileContextUsageBreakdown(estimate, undefined)).toBeNull();
  });

  it("returns null when no estimate was cached", () => {
    expect(reconcileContextUsageBreakdown(null, 600)).toBeNull();
    expect(reconcileContextUsageBreakdown(undefined, 600)).toBeNull();
  });
});

describe("sanitizeContextUsageEstimate", () => {
  const validEstimate = {
    system: 100,
    skills: 40,
    files: 30,
    tools: 60,
    mcp: 20,
    conversation: 200,
    user: 10,
    toolResults: 50,
    computedAt: 123,
  };

  it("round-trips a well-formed estimate", () => {
    expect(sanitizeContextUsageEstimate(validEstimate)).toEqual(validEstimate);
  });

  it("keeps an estimate whose computedAt is missing or malformed, defaulting it to 0", () => {
    const { computedAt: _drop, ...withoutComputedAt } = validEstimate;
    expect(sanitizeContextUsageEstimate(withoutComputedAt)).toEqual({ ...validEstimate, computedAt: 0 });
    expect(sanitizeContextUsageEstimate({ ...validEstimate, computedAt: "recent" as unknown as number }))
      .toEqual({ ...validEstimate, computedAt: 0 });
  });

  it("rejects nullish/non-object values and arrays", () => {
    expect(sanitizeContextUsageEstimate(null)).toBeNull();
    expect(sanitizeContextUsageEstimate(undefined)).toBeNull();
    expect(sanitizeContextUsageEstimate("100")).toBeNull();
    expect(sanitizeContextUsageEstimate([validEstimate])).toBeNull();
  });

  it("rejects a missing or malformed category instead of restoring a partial split", () => {
    const { conversation: _missing, ...withoutConversation } = validEstimate;
    expect(sanitizeContextUsageEstimate(withoutConversation)).toBeNull();
    expect(sanitizeContextUsageEstimate({ ...validEstimate, mcp: -5 })).toBeNull();
    expect(sanitizeContextUsageEstimate({ ...validEstimate, tools: Number.NaN })).toBeNull();
    expect(sanitizeContextUsageEstimate({ ...validEstimate, system: "100" as unknown as number })).toBeNull();
  });

  it("rejects an all-zero estimate as meaningless", () => {
    const zeroEstimate = {
      system: 0, skills: 0, files: 0, tools: 0,
      mcp: 0, conversation: 0, user: 0, toolResults: 0,
      computedAt: 1,
    };
    expect(sanitizeContextUsageEstimate(zeroEstimate)).toBeNull();
  });
});
