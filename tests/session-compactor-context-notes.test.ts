// <context-notes> 上下文笔记保护：压缩时从磁盘现读笔记全文注入摘要尾部，
// 跨压缩存活；无笔记/空笔记不加段；超长在 16KiB 兜底截断。harness 镜像
// tests/session-compactor-plan-file.test.ts。
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
import {
  createContextNotesTool,
  contextNotesPath,
  CONTEXT_NOTES_MAX_BYTES,
} from "../lib/tools/context-notes-tool.ts";

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
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-ctx-notes-"));
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

beforeEach(() => { convertAgentMessagesToLlmMock.mockClear(); });
afterEach(() => { for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true }); });

describe("compaction context-notes 保护", () => {
  it("无 contextNotesPath：摘要保持原样", async () => {
    const result = await compactWith(basePreparation());
    expect(result.summary).not.toContain("<context-notes>");
  });

  it("笔记存在：全文注入摘要尾部", async () => {
    const dir = makeTempDir();
    const sessionPath = path.join(dir, "sess-1.jsonl");
    fs.writeFileSync(contextNotesPath(sessionPath), "- 用 pnpm 不用 npm\n- 配置在 src/config.ts\n", "utf-8");
    const result = await compactWith(basePreparation({ contextNotesPath: sessionPath }));
    expect(result.summary).toContain("<context-notes>");
    expect(result.summary).toContain("用 pnpm 不用 npm");
    expect(result.summary).toContain("配置在 src/config.ts");
    expect(result.summary.indexOf("</context-notes>")).toBeGreaterThan(result.summary.indexOf("<context-notes>"));
  });

  it("笔记为空/文件缺失：不加段", async () => {
    const dir = makeTempDir();
    const empty = path.join(dir, "empty.jsonl");
    fs.writeFileSync(contextNotesPath(empty), "   \n", "utf-8");
    expect((await compactWith(basePreparation({ contextNotesPath: empty }))).summary).not.toContain("<context-notes>");
    const missing = path.join(dir, "missing.jsonl");
    expect((await compactWith(basePreparation({ contextNotesPath: missing }))).summary).not.toContain("<context-notes>");
  });

  it("与 plan-file 段共存", async () => {
    const dir = makeTempDir();
    const sessionPath = path.join(dir, "sess-1.jsonl");
    fs.writeFileSync(contextNotesPath(sessionPath), "NOTE-ALIVE", "utf-8");
    fs.writeFileSync(`${sessionPath.replace(/\.jsonl$/, "")}.plan.md`, "PLAN-ALIVE", "utf-8");
    const result = await compactWith(basePreparation({
      contextNotesPath: sessionPath,
      planFilePath: `${sessionPath.replace(/\.jsonl$/, "")}.plan.md`,
    }));
    expect(result.summary).toContain("<plan-file>");
    expect(result.summary).toContain("PLAN-ALIVE");
    expect(result.summary).toContain("<context-notes>");
    expect(result.summary).toContain("NOTE-ALIVE");
  });
});

describe("context_notes 工具", () => {
  beforeEach(() => { vi.clearAllMocks(); });
  afterEach(() => { for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true }); });

  function makeTool(sp: string) {
    return createContextNotesTool({ getSessionPath: () => sp });
  }
  function freshSession() {
    const dir = makeTempDir();
    return path.join(dir, "s.jsonl");
  }

  it("write → read 回读；append 追加到尾部", async () => {
    const sp = freshSession();
    const tool = makeTool(sp);
    await tool.execute("c1", { action: "write", text: "第一条" });
    const appended: any = await tool.execute("c2", { action: "append", text: "第二条" });
    expect(appended.content[0].text).toContain("appended");
    const read: any = await tool.execute("c3", { action: "read" });
    expect(read.content[0].text).toContain("第一条");
    expect(read.content[0].text).toContain("第二条");
    const idx = read.content[0].text.indexOf("第一条");
    expect(read.content[0].text.indexOf("第二条")).toBeGreaterThan(idx);
  });

  it("超 16KiB：write/append 都拒绝并给 CONTEXT_NOTES_OVERSIZE", async () => {
    const sp = freshSession();
    const tool = makeTool(sp);
    const big = "x".repeat(CONTEXT_NOTES_MAX_BYTES + 1);
    const r1: any = await tool.execute("c1", { action: "write", text: big });
    expect(r1.isError).toBe(true);
    expect(r1.details.errorCode).toBe("CONTEXT_NOTES_OVERSIZE");
    await tool.execute("c2", { action: "write", text: "y".repeat(CONTEXT_NOTES_MAX_BYTES - 10) });
    const r3: any = await tool.execute("c3", { action: "append", text: "overflow-block" });
    expect(r3.isError).toBe(true);
    expect(r3.details.errorCode).toBe("CONTEXT_NOTES_OVERSIZE");
  });

  it("空文本 write/append 被拒", async () => {
    const sp = freshSession();
    const tool = makeTool(sp);
    const r1: any = await tool.execute("c1", { action: "write", text: "  " });
    expect(r1.isError).toBeUndefined();
    expect(r1.content[0].text).toContain("refusing");
    const r2: any = await tool.execute("c2", { action: "append", text: "" });
    expect(r2.content[0].text).toContain("required");
  });

  it("read 权限恒 read；write/append=routine", () => {
    const tool = makeTool("/tmp/s.jsonl");
    expect(tool.sessionPermission.resolveInvocation({ action: "read" }).kind).toBe("read");
    expect(tool.sessionPermission.resolveInvocation(undefined).kind).toBe("read");
    expect(tool.sessionPermission.resolveInvocation({ action: "write" }).kind).toBe("routine");
    expect(tool.sessionPermission.resolveInvocation({ action: "append" }).kind).toBe("routine");
  });
});
