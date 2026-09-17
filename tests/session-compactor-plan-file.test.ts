// 压缩摘要的 <plan-file> 计划文件保护：会话旁 plan.md 的正文在压缩时从磁盘
// 现读、贴进摘要尾部，跨压缩存活；文件不存在/为空则不加段；超长截头并指路。
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { convertAgentMessagesToLlmMock } = vi.hoisted(() => ({
  convertAgentMessagesToLlmMock: vi.fn(async (messages) => messages),
}));

vi.mock("../lib/pi-sdk/index.js", async (importOriginal) => ({
  ...await importOriginal<any>(),
  convertAgentMessagesToLlm: convertAgentMessagesToLlmMock,
}));

import { createCachePreservingCompactionResult } from "../core/session-compactor.ts";

const VALID_COMPACTION_SUMMARY = `## Goal
Keep the session useful.

## Constraints & Preferences
- Preserve the retained suffix.

## Progress
### Done
- [x] Summarized the old region.

### In Progress
- [ ] Continue from the retained suffix.

### Blocked
- (none)

## Key Decisions
- Keep the proven boundary stable.

## Next Steps
1. Continue the session.

## Critical Context
- The recent tail remains verbatim.`;

function agentStreamOf(text = VALID_COMPACTION_SUMMARY) {
  const message = {
    role: "assistant",
    content: [{ type: "text", text }],
    api: "openai-completions",
    provider: "test-provider",
    model: "test-model",
    usage: {
      input: 10, output: 5, cacheRead: 0, cacheWrite: 0, totalTokens: 15,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
    },
    stopReason: "stop",
    timestamp: Date.now(),
  };
  return {
    async *[Symbol.asyncIterator]() {
      yield { type: "done", reason: "stop", message };
    },
    async result() {
      return message;
    },
  };
}

function piAssistant(text: string, timestamp: number) {
  return {
    role: "assistant",
    content: [{ type: "text", text }],
    api: "openai-completions",
    provider: "test-provider",
    model: "test-model",
    usage: {
      input: 10, output: 5, cacheRead: 0, cacheWrite: 0, totalTokens: 15,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
    },
    stopReason: "stop",
    timestamp,
  };
}

const tempDirs: string[] = [];

function makeTempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-plan-file-"));
  tempDirs.push(dir);
  return dir;
}

async function compactWith(preparation: any) {
  return await createCachePreservingCompactionResult({
    preparation,
    model: { id: "model", reasoning: true },
    systemPrompt: "agent system prompt",
    messages: [piAssistant("KEPT_TAIL_REMAINS_VERBATIM", 99)],
    retainedMessageCount: 1,
    tools: [],
    customInstructions: undefined,
    signal: new AbortController().signal,
    thinkingLevel: "high",
    outputPolicy: "bounded",
    streamFn: vi.fn(async () => agentStreamOf()),
    convertToLlm: async (input: any[]) => input,
  } as any);
}

function basePreparation(overrides: Record<string, any> = {}) {
  return {
    firstKeptEntryId: "entry-keep",
    tokensBefore: 1234,
    previousSummary: undefined,
    messagesToSummarize: [],
    turnPrefixMessages: [],
    isSplitTurn: false,
    settings: { reserveTokens: 1000 },
    fileOps: { read: new Set<string>(), written: new Set<string>(), edited: new Set<string>() },
    ...overrides,
  };
}

beforeEach(() => {
  convertAgentMessagesToLlmMock.mockClear();
});

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("compaction plan-file 保护", () => {
  it("preparation 无 planFilePath：摘要保持原样", async () => {
    const result = await compactWith(basePreparation());
    expect(result.summary).not.toContain("<plan-file>");
  });

  it("计划文件存在：正文与路径贴进摘要尾部", async () => {
    const dir = makeTempDir();
    const planPath = path.join(dir, "sess-1.plan.md");
    fs.writeFileSync(planPath, "# 重构计划\n\n第一步：拆分模块。\n", "utf-8");
    const result = await compactWith(basePreparation({ planFilePath: planPath }));
    expect(result.summary).toContain("<plan-file>");
    expect(result.summary).toContain(planPath);
    expect(result.summary).toContain("第一步：拆分模块。");
    expect(result.summary.indexOf("</plan-file>")).toBeGreaterThan(result.summary.indexOf("<plan-file>"));
  });

  it("计划文件不存在：不加段（没有要保护的东西）", async () => {
    const dir = makeTempDir();
    const result = await compactWith(basePreparation({
      planFilePath: path.join(dir, "missing.plan.md"),
    }));
    expect(result.summary).not.toContain("<plan-file>");
  });

  it("计划文件为空：不加段", async () => {
    const dir = makeTempDir();
    const planPath = path.join(dir, "empty.plan.md");
    fs.writeFileSync(planPath, "   \n  ", "utf-8");
    const result = await compactWith(basePreparation({ planFilePath: planPath }));
    expect(result.summary).not.toContain("<plan-file>");
  });

  it("超长计划截头并指路回文件", async () => {
    const dir = makeTempDir();
    const planPath = path.join(dir, "big.plan.md");
    fs.writeFileSync(planPath, "x".repeat(9000), "utf-8");
    const result = await compactWith(basePreparation({ planFilePath: planPath }));
    expect(result.summary).toContain("<plan-file>");
    expect(result.summary).toContain("plan truncated for compaction");
    expect(result.summary.length).toBeLessThan(VALID_COMPACTION_SUMMARY.length + 9000);
  });
});
