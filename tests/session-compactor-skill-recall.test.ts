// 压缩摘要的 <skill-recall> 技能回顾：被摘要区里用过的技能正文要在压缩后
// 重新贴回上下文（读磁盘现读+预算截头），上一份摘要里的回顾要跨次续传，
// 文件失踪/只有点名没有读入都要显式标注而不是静默丢弃。
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

import { createCachePreservingCompactionResult, parseExistingSkillRecall } from "../core/session-compactor.ts";

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
      input: 10,
      output: 5,
      cacheRead: 0,
      cacheWrite: 0,
      totalTokens: 15,
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

function piUser(text: string, timestamp: number) {
  return { role: "user", content: [{ type: "text", text }], timestamp };
}

function piAssistant(text: string, timestamp: number) {
  return {
    role: "assistant",
    content: [{ type: "text", text }],
    api: "openai-completions",
    provider: "test-provider",
    model: "test-model",
    usage: {
      input: 10,
      output: 5,
      cacheRead: 0,
      cacheWrite: 0,
      totalTokens: 15,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
    },
    stopReason: "stop",
    timestamp,
  };
}

function piReadCall(path: string, timestamp: number) {
  return {
    role: "assistant",
    content: [{ type: "toolCall", id: `call-${timestamp}`, name: "read", arguments: { path } }],
    api: "openai-completions",
    provider: "test-provider",
    model: "test-model",
    usage: {
      input: 10,
      output: 5,
      cacheRead: 0,
      cacheWrite: 0,
      totalTokens: 15,
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
    },
    stopReason: "toolUse",
    timestamp,
  };
}

const tempDirs: string[] = [];

function makeTempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-skill-recall-"));
  tempDirs.push(dir);
  return dir;
}

function writeSkillFile(dir: string, name: string, body: string) {
  const skillDir = path.join(dir, name);
  fs.mkdirSync(skillDir, { recursive: true });
  const skillPath = path.join(skillDir, "SKILL.md");
  fs.writeFileSync(skillPath, body, "utf-8");
  return skillPath;
}

async function compactWith({
  preparation,
  messages,
  retainedMessageCount,
}: {
  preparation: any;
  messages: any[];
  retainedMessageCount: number;
}) {
  return await createCachePreservingCompactionResult({
    preparation,
    model: { id: "model", reasoning: true },
    systemPrompt: "agent system prompt",
    messages,
    retainedMessageCount,
    tools: [],
    customInstructions: undefined,
    signal: new AbortController().signal,
    thinkingLevel: "high",
    outputPolicy: "bounded",
    streamFn: vi.fn(async (_model: any, context: any) => {
      void context;
      return agentStreamOf();
    }),
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

const retainedTail = piAssistant("KEPT_TAIL_REMAINS_VERBATIM", 99);

beforeEach(() => {
  convertAgentMessagesToLlmMock.mockClear();
});

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

describe("compaction skill recall", () => {
  it("leaves the summary untouched when no skill was invoked", async () => {
    const oldMessage = piAssistant("ordinary old content", 10);
    const result = await compactWith({
      preparation: basePreparation({ messagesToSummarize: [oldMessage] }),
      messages: [oldMessage, retainedTail],
      retainedMessageCount: 1,
    });

    expect(result.summary).toBe(VALID_COMPACTION_SUMMARY);
  });

  it("re-attaches the invoked skill body after compaction", async () => {
    const dir = makeTempDir();
    const skillPath = writeSkillFile(dir, "my-skill", "SKILL_BODY_MARKER step one step two");
    const invocation = piUser("[Use skill: my-skill]\nplease do the thing", 10);
    const skillRead = piReadCall(skillPath, 11);

    const result = await compactWith({
      preparation: basePreparation({ messagesToSummarize: [invocation, skillRead] }),
      messages: [invocation, skillRead, retainedTail],
      retainedMessageCount: 1,
    });

    expect(result.summary).toContain("<skill-recall>");
    expect(result.summary).toContain("### Skill: my-skill");
    expect(result.summary).toContain(`Path: ${skillPath}`);
    expect(result.summary).toContain("SKILL_BODY_MARKER step one step two");
    expect(result.summary).toContain("keep following them");
    expect(result.summary.trimEnd().endsWith("</skill-recall>")).toBe(true);
  });

  it("reads string-content user messages for [Use skill:] prefixes too", async () => {
    const dir = makeTempDir();
    const skillPath = writeSkillFile(dir, "flat-skill", "FLAT_BODY");
    const invocation = { role: "user", content: "[Use skill: flat-skill]\ndo it", timestamp: 10 };
    const skillRead = piReadCall(skillPath, 11);

    const result = await compactWith({
      preparation: basePreparation({ messagesToSummarize: [invocation, skillRead] }),
      messages: [invocation, skillRead, retainedTail],
      retainedMessageCount: 1,
    });

    expect(result.summary).toContain("### Skill: flat-skill");
    expect(result.summary).toContain("FLAT_BODY");
  });

  it("truncates long skill bodies and points back at the path", async () => {
    const dir = makeTempDir();
    const skillPath = writeSkillFile(dir, "long-skill", "A".repeat(20_000) + "TAIL_NEVER_KEPT");
    const skillRead = piReadCall(skillPath, 11);

    const result = await compactWith({
      preparation: basePreparation({ messagesToSummarize: [skillRead] }),
      messages: [skillRead, retainedTail],
      retainedMessageCount: 1,
    });

    expect(result.summary).toContain(`Path: ${skillPath}`);
    expect(result.summary).toContain("[... skill instructions truncated for compaction");
    expect(result.summary).not.toContain("TAIL_NEVER_KEPT");
    const recallBody = result.summary.slice(result.summary.indexOf("<skill-recall>"));
    const keptBodyChars = (recallBody.match(/A/g) || []).length;
    expect(keptBodyChars).toBeGreaterThan(0);
    expect(keptBodyChars).toBeLessThanOrEqual(6_000);
  });

  it("carries a previous recall forward without re-reading the file from disk", async () => {
    const previousSummary = `## Goal
older work.

<skill-recall>
The following skills were invoked earlier in this session.
### Skill: old-skill
Path: /nonexistent/old-skill/SKILL.md
OLD_BODY_MARK
</skill-recall>`;
    const oldMessage = piAssistant("more history", 10);

    const result = await compactWith({
      preparation: basePreparation({
        previousSummary,
        messagesToSummarize: [oldMessage],
      }),
      messages: [oldMessage, retainedTail],
      retainedMessageCount: 1,
    });

    expect(result.summary).toContain("### Skill: old-skill");
    expect(result.summary).toContain("OLD_BODY_MARK");
    expect(result.summary).toContain("Path: /nonexistent/old-skill/SKILL.md");
  });

  it("marks a name-only invocation explicitly instead of dropping it", async () => {
    const invocation = piUser("[Use skill: ghost-skill]\nplease", 10);
    const oldMessage = piAssistant("no skill file was ever read", 11);

    const result = await compactWith({
      preparation: basePreparation({ messagesToSummarize: [invocation, oldMessage] }),
      messages: [invocation, oldMessage, retainedTail],
      retainedMessageCount: 1,
    });

    expect(result.summary).toContain("### Skill: ghost-skill");
    expect(result.summary).toContain("(invoked by name only, never read in the compacted history");
  });

  it("marks a vanished skill file as unavailable instead of dropping it", async () => {
    const missingPath = path.join(makeTempDir(), "gone-skill", "SKILL.md");
    const skillRead = piReadCall(missingPath, 11);

    const result = await compactWith({
      preparation: basePreparation({ messagesToSummarize: [skillRead] }),
      messages: [skillRead, retainedTail],
      retainedMessageCount: 1,
    });

    expect(result.summary).toContain("### Skill: gone-skill");
    expect(result.summary).toContain(`Path: ${missingPath}`);
    expect(result.summary).toContain("(skill file could not be read at compaction time");
  });

  it("drops bodies beyond the total budget to an explicit omission note", async () => {
    const dir = makeTempDir();
    const messages: any[] = [];
    const paths: string[] = [];
    for (const name of ["skill-a", "skill-b", "skill-c", "skill-d", "skill-e"]) {
      const skillPath = writeSkillFile(dir, name, `BODY_${name}_` + "x".repeat(10_000));
      paths.push(skillPath);
      messages.push(piReadCall(skillPath, 10 + paths.length));
    }

    const result = await compactWith({
      preparation: basePreparation({ messagesToSummarize: messages }),
      messages: [...messages, retainedTail],
      retainedMessageCount: 1,
    });

    expect(result.summary).toContain("### Skill: skill-a");
    expect(result.summary).toContain("BODY_skill-a_");
    expect(result.summary).toContain("### Skill: skill-e");
    expect(result.summary).toContain("(content omitted to stay within the recall budget; read the Path above)");
    const recallBody = result.summary.slice(result.summary.indexOf("<skill-recall>"));
    expect(recallBody.length).toBeLessThan(30_000);
  });

  it("fills in the path of a previously name-only entry once the read shows up", () => {
    const merged = parseExistingSkillRecall(
      "prefix\n<skill-recall>\n### Skill: ghost\nPath: /a/SKILL.md\nBODY\n### Skill: bare\n(no note)\n</skill-recall>\n",
    );

    expect(merged).toEqual([
      { name: "ghost", path: "/a/SKILL.md", content: "BODY" },
      { name: "bare", path: null, content: "(no note)" },
    ]);
  });
});
