/**
 * Phase 5 Semantic Input Provenance — 集成验证（Step 8/9/13）。
 *
 * 覆盖：
 *   - MC-01：stream observer 用冻结快照做 runtime 前缀验证（exact sections +
 *     SDK 尾段 structural + skills/agents identity-only）；无快照/验证失败 →
 *     诚实 structural；messages 分类（turn 证明 current_user_input、toolResult、
 *     tools）；observer 事件只带 summary + symbol 引用（毒丸不泄漏）。
 *   - MC-03：isCompacting → structural，不伪造 exact。
 *   - MC-02：显式 scope provenance + 尾段扩展（recovery/repair 的追加消息）。
 *   - Session Snapshot：create→serialize→restore roundtrip provenance 不变；
 *     Persona V1→V2 后旧快照 provenance 仍描述 V1（§四十七/§九十二）。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";

import {
  installModelCallStreamObserver,
  installModelCallTraceIngress,
  registerSessionModelCallContext,
} from "../lib/pi-sdk/model-call-stream-observer.ts";
import {
  createSemanticInputProvenance,
  renderProvenancedText,
  type SessionPromptProvenancePayload,
} from "../lib/llm/semantic-input-provenance.ts";
import { buildSessionPromptProvenancePayload } from "../lib/llm/semantic-input-provenance-payload.ts";
import {
  runWithModelCallScope,
} from "../lib/llm/model-call-scope.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";
import {
  buildSessionPromptSnapshot,
  normalizeSessionPromptSnapshot,
} from "../core/session-prompt-snapshot.ts";
import { Agent } from "../core/agent.ts";

function streamOf(message: any) {
  const stream = createAssistantMessageEventStream();
  queueMicrotask(() => {
    stream.push({ type: "done", reason: message.stopReason, message } as any);
    stream.end();
  });
  return stream;
}

function assistantMessage(overrides: Record<string, any> = {}) {
  return {
    role: "assistant",
    content: [{ type: "text", text: "ok" }],
    api: "openai-completions",
    provider: "test-provider",
    model: "test-model",
    stopReason: "stop",
    ...overrides,
  };
}

function fakeSession(streamFunction: any, overrides: Record<string, any> = {}) {
  return {
    agent: { streamFunction },
    sessionManager: {
      getSessionId: () => "sess-1",
      getSessionFile: () => "/tmp/sess-1.jsonl",
    },
    isCompacting: false,
    ...overrides,
  };
}

const MODEL = { id: "test-model", provider: "test-provider", api: "openai-completions" };

async function flushTerminal() {
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
}

/** 构造冻结快照 provenance payload（模拟 coordinator 注册的形状）。 */
function snapshotPayload(customPrompt: string): SessionPromptProvenancePayload {
  const rendered = renderProvenancedText([
    { text: "PLATFORM NOTE", category: "platform_instruction", source: { type: "runtime", id: "platform.intro" } },
    { text: "TOP_SECRET_PERSONA 你是谁", category: "persona", source: { type: "runtime", id: "persona" } },
    { text: "TOP_SECRET_MEMORY 记忆块", category: "memory_context", source: { type: "memory", id: "memory.longterm" } },
  ], "\n", { root: "systemPrompt" });
  // rendered.text 与 customPrompt 由调用方保证一致（coordinator 冻结同一装配）
  return buildSessionPromptProvenancePayload({
    systemPrompt: customPrompt,
    provenanceSections: rendered.sections,
    appendSystemPrompt: ["extra-append"],
    skillsResult: { skills: [{ name: "pdf-tools" }, { name: "web-search" }] },
    agentsFilesResult: { agentsFiles: [{ path: "/Users/alice/project/AGENTS.md" }] },
  })!;
}

afterEach(() => {
  setModelCallObserver(null);
  vi.useRealTimers();
});

describe("MC-01 stream observer provenance", () => {
  it("prefix verification passes → snapshot sections exact + SDK tail structural + identity sections", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const customPrompt = "PLATFORM NOTE\nTOP_SECRET_PERSONA 你是谁\nTOP_SECRET_MEMORY 记忆块";
    const payload = snapshotPayload(customPrompt);
    const finalSystemPrompt = customPrompt + "\n\n# Skills\n\n- pdf-tools\n\nCurrent working directory: /x";

    const session = fakeSession(async () => streamOf(assistantMessage()));
    registerSessionModelCallContext(session, () => ({
      source: { subsystem: "session", operation: "reply", surface: "desktop", trigger: "user" },
      attribution: { kind: "session" },
      promptProvenance: payload,
    }));
    installModelCallStreamObserver(session);

    await session.agent.streamFunction(MODEL, {
      systemPrompt: finalSystemPrompt,
      messages: [{ role: "user", content: "hi" }],
      tools: [{ name: "read" }, { name: "grep" }],
    }, {});
    await flushTerminal();

    const callId = observer.callIds()[0];
    const provenance = observer.provenanceForCall(callId)!;
    expect(provenance).not.toBeNull();
    const categories = provenance.sections.map((s) => s.category);
    expect(categories.slice(0, 3)).toEqual(["platform_instruction", "persona", "memory_context"]);
    expect(categories).toContain("sdk_internal");
    expect(categories).toContain("skill_instruction");
    expect(categories).toContain("agents_file");
    expect(categories).toContain("session_instruction"); // append identity-only
    expect(categories).toContain("tool_definition");
    // exact 段的 span 定位：persona 段 slice 出冻结内容。
    const persona = provenance.sections.find((s) => s.category === "persona")!;
    expect(finalSystemPrompt.slice(persona.locator.span!.start, persona.locator.span!.end))
      .toBe("TOP_SECRET_PERSONA 你是谁");
    // SDK 尾段 structural，不含 skills 内容反推。
    const tail = provenance.sections.find((s) => s.category === "sdk_internal")!;
    expect(tail.precision).toBe("structural");
    // agentsFiles 只保留 basename，绝对路径禁入。
    const agentsFile = provenance.sections.find((s) => s.category === "agents_file")!;
    expect(agentsFile.source?.id).toBe("AGENTS.md");
    // summary 进 logical_call_start details；毒丸不进事件。
    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.details).toMatchObject({
      inputShape: "chat_context",
      provenancePrecision: "partial", // SDK 尾段 structural → partial（诚实）
    });
    observer.assertNoSensitiveContent(["TOP_SECRET_PERSONA", "TOP_SECRET_MEMORY"]);
  });

  it("prefix verification fails → honest structural single section (legacy snapshot)", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const payload = snapshotPayload("完全不同的旧快照");
    const session = fakeSession(async () => streamOf(assistantMessage()));
    registerSessionModelCallContext(session, () => ({
      promptProvenance: payload,
      source: null as any,
      attribution: null as any,
    }));
    installModelCallStreamObserver(session);
    await session.agent.streamFunction(MODEL, {
      systemPrompt: "另一个 prompt",
      messages: [],
      tools: [],
    }, {});
    await flushTerminal();
    const provenance = observer.provenanceForCall(observer.callIds()[0])!;
    expect(provenance.sections).toHaveLength(1);
    expect(provenance.sections[0]).toMatchObject({
      category: "session_instruction",
      precision: "structural",
    });
  });

  it("classifies messages inside a prompt turn: last user = current input; toolResult = tool_result", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    let capturedContext: any = null;
    const session = fakeSession(async (_model: any, context: any) => {
      capturedContext = context;
      return streamOf(assistantMessage());
    });
    // 模拟 SDK prompt():把用户输入 append 进 messages 后调 streamFn。
    (session as any).prompt = async (input: string) => {
      await session.agent.streamFunction(MODEL, {
        systemPrompt: "S",
        messages: [
          { role: "user", content: "早前 TOP_SECRET_USER 输入" },
          { role: "assistant", content: "回复" },
          { role: "toolResult", toolCallId: "t1", toolName: "read", content: [] },
          { role: "user", content: `本轮当前输入 ${input}` },
        ],
        tools: [{ name: "read" }],
      }, {});
    };
    installModelCallStreamObserver(session);
    installModelCallTraceIngress(session);

    await (session as any).prompt("TOP_SECRET_USER");
    await flushTerminal();

    const provenance = observer.provenanceForCall(observer.callIds()[0])!;
    const messageSections = provenance.sections.filter((s) => s.locator.root === "messages");
    expect(messageSections.map((s) => s.category)).toEqual([
      "conversation_history",
      "conversation_history",
      "tool_result",
      "current_user_input",
    ]);
    expect(messageSections[3].role).toBe("user");
    expect(messageSections[2].source).toEqual({ type: "tool", id: "read" });
    const toolSection = provenance.sections.find((s) => s.locator.root === "tools")!;
    expect(toolSection.category).toBe("tool_definition");
    expect(capturedContext.messages).toHaveLength(4);
    observer.assertNoSensitiveContent(["TOP_SECRET_USER"]);
  });

  it("without a turn marker the last user message stays conversation_history (no guessing)", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const session = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);
    // 直接调 streamFn（无 prompt ingress）—— agent.continue / compaction 场景。
    await session.agent.streamFunction(MODEL, {
      systemPrompt: "S",
      messages: [{ role: "user", content: "续跑输入" }],
      tools: [],
    }, {});
    await flushTerminal();
    const provenance = observer.provenanceForCall(observer.callIds()[0])!;
    expect(provenance.sections.filter((s) => s.locator.root === "messages").map((s) => s.category))
      .toEqual(["conversation_history"]);
  });
});

describe("MC-03 native summarization (isCompacting)", () => {
  it("marks system + serialized message structural — no fake exact", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const session = fakeSession(async () => streamOf(assistantMessage()), { isCompacting: true });
    installModelCallStreamObserver(session);
    await session.agent.streamFunction(MODEL, {
      systemPrompt: "You are a context summarization assistant…",
      messages: [{ role: "user", content: [{ type: "text", text: "<conversation>…TOP_SECRET_MEMORY…" }] }],
      tools: [],
    }, {});
    await flushTerminal();
    const provenance = observer.provenanceForCall(observer.callIds()[0])!;
    expect(provenance.sections.map((s) => `${s.category}/${s.precision}`)).toEqual([
      "task_instruction/structural",
      "task_input/structural",
    ]);
    observer.assertNoSensitiveContent(["TOP_SECRET_MEMORY"]);
  });
});

describe("MC-02 explicit scope provenance + tail extension", () => {
  it("attaches runner provenance and classifies uncovered trailing messages", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const session = fakeSession(async (_model: any, context: any) => {
      // recovery 调用：live(2) + instruction + assistant + toolResult
      expect(context.messages).toHaveLength(5);
      return streamOf(assistantMessage());
    });
    installModelCallStreamObserver(session);

    const scopeProvenance = createSemanticInputProvenance("chat_context", [
      { category: "session_instruction", role: "system", precision: "structural",
        locator: { root: "systemPrompt", span: { start: 0, end: 10 } }, source: { type: "snapshot", id: "session.systemPrompt" } },
      { category: "conversation_history", role: "user", precision: "exact",
        locator: { root: "messages", path: [0] }, source: null },
      { category: "conversation_history", role: "assistant", precision: "exact",
        locator: { root: "messages", path: [1] }, source: null },
      { category: "task_instruction", role: "user", precision: "exact",
        locator: { root: "messages", path: [2] }, source: { type: "runtime", id: "compaction.instruction" } },
    ]);
    await runWithModelCallScope({
      callId: "mc_scope_test",
      semanticInputProvenance: scopeProvenance,
    }, () => session.agent.streamFunction(MODEL, {
      systemPrompt: "0123456789",
      messages: [
        { role: "user", content: "live1" },
        { role: "assistant", content: "live2" },
        { role: "user", content: "compaction instruction" },
        { role: "assistant", content: "draft", content2: undefined },
        { role: "toolResult", toolCallId: "t1", toolName: "placeholder", content: [] },
      ],
      tools: [],
    }, {}));
    await flushTerminal();

    const provenance = observer.provenanceForCall("mc_scope_test")!;
    const messageSections = provenance.sections.filter((s) => s.locator.root === "messages");
    expect(messageSections.map((s) => s.category)).toEqual([
      "conversation_history",
      "conversation_history",
      "task_instruction",
      "conversation_history", // 尾段扩展：assistant
      "tool_result",          // 尾段扩展：placeholder toolResult
    ]);
    const toolResult = messageSections[4];
    expect(toolResult.source).toEqual({ type: "tool", id: "placeholder" });
  });
});

describe("session prompt snapshot provenance (Step 7)", () => {
  const tempDirs: string[] = [];
  function makeTempDir() {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-snapshot-prov-"));
    tempDirs.push(dir);
    return dir;
  }
  afterEach(() => {
    for (const dir of tempDirs.splice(0)) {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  function makeAgent(locale = "zh-CN") {
    const root = makeTempDir();
    const agentsDir = path.join(root, "agents");
    const productDir = path.join(root, "product");
    const userDir = path.join(root, "user");
    const agentDir = path.join(agentsDir, "hana");
    fs.mkdirSync(path.join(agentDir, "memory"), { recursive: true });
    fs.mkdirSync(path.join(productDir, "yuan"), { recursive: true });
    fs.mkdirSync(userDir, { recursive: true });
    const yuanPath = path.join(productDir, "yuan", "lingxi.md");
    fs.writeFileSync(yuanPath, "PERSONA-V1-身份", "utf-8");
    fs.writeFileSync(path.join(userDir, "user.md"), "PROFILE", "utf-8");
    fs.writeFileSync(path.join(agentDir, "memory", "memory.md"), "MEMORY-V1", "utf-8");
    const agent = new Agent({ id: "hana", agentsDir, productDir, userDir } as any);
    agent._config = {
      locale,
      agent: { yuan: "lingxi" },
      memory: { enabled: true },
      experience: { enabled: false },
    };
    agent.userName = "黎";
    agent.agentName = "Hanako";
    agent._cb = { getTimezone: () => "Asia/Shanghai" };
    return { agent, yuanPath };
  }

  it("create → serialize → restore roundtrip keeps prompt and provenance unchanged", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-04T07:53:00.000Z"));
    const { agent } = makeAgent();
    const artifact = agent.buildSystemPromptArtifact({ forceMemoryEnabled: true });
    const snapshot = buildSessionPromptSnapshot({
      systemPrompt: artifact.text,
      appendSystemPrompt: [],
      skillsResult: null,
      agentsFilesResult: null,
      systemPromptProvenance: artifact.provenance,
    });
    // 持久化 roundtrip（JSONL 存的是 JSON 对象）
    const restored = normalizeSessionPromptSnapshot(JSON.parse(JSON.stringify(snapshot)));
    expect(restored?.systemPrompt).toBe(artifact.text);
    expect(restored?.systemPromptProvenance).toEqual(artifact.provenance);
    // span 仍指向原文：persona 段 slice 出 V1 内容。
    const persona = restored!.systemPromptProvenance!.find((s) => s.source?.id === "persona")!;
    expect(restored!.systemPrompt.slice(persona.locator.span!.start, persona.locator.span!.end))
      .toContain("PERSONA-V1-身份");
    // provenance JSON 不含内容（毒丸）。
    const serialized = JSON.stringify(restored!.systemPromptProvenance);
    expect(serialized.includes("PERSONA-V1-身份")).toBe(false);
    expect(serialized.includes("MEMORY-V1")).toBe(false);
  });

  it("persona V1 → V2: old snapshot provenance still describes V1, never rebuilt from V2", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-04T07:53:00.000Z"));
    const { agent, yuanPath } = makeAgent();
    const v1Artifact = agent.buildSystemPromptArtifact({ forceMemoryEnabled: true });

    // Persona 演进到 V2（换人格文件）。
    fs.writeFileSync(yuanPath, "PERSONA-V2-全新身份", "utf-8");
    const v2Artifact = agent.buildSystemPromptArtifact({ forceMemoryEnabled: true });
    expect(v2Artifact.text).not.toBe(v1Artifact.text);

    // 旧 session 的冻结快照不受影响：provenance 描述 V1。
    const snapshot = buildSessionPromptSnapshot({
      systemPrompt: v1Artifact.text,
      systemPromptProvenance: v1Artifact.provenance,
    });
    const restored = normalizeSessionPromptSnapshot(JSON.parse(JSON.stringify(snapshot)));
    const persona = restored!.systemPromptProvenance!.find((s) => s.source?.id === "persona")!;
    expect(restored!.systemPrompt.slice(persona.locator.span!.start, persona.locator.span!.end))
      .toContain("PERSONA-V1-身份");
    expect(restored!.systemPrompt).not.toContain("PERSONA-V2");
  });

  it("legacy snapshot without provenance normalizes without the field (honest structural)", () => {
    const restored = normalizeSessionPromptSnapshot({
      version: 1,
      systemPrompt: "旧 prompt",
      appendSystemPrompt: [],
      skillsResult: { skills: [], diagnostics: [] },
      agentsFilesResult: { agentsFiles: [] },
    });
    expect(restored?.systemPrompt).toBe("旧 prompt");
    expect((restored as any).systemPromptProvenance).toBeUndefined();
    // 非法 provenance 字段被 fail-closed 清洗。
    const dirty = normalizeSessionPromptSnapshot({
      version: 1,
      systemPrompt: "旧 prompt",
      systemPromptProvenance: [{ category: "not-a-category" }],
    });
    expect((dirty as any).systemPromptProvenance).toBeUndefined();
  });
});

describe("media/speech provenance descriptors (MC-06/08/09)", () => {
  it("image: prompt + references produce category/locator only, values never recorded", () => {
    const { buildImageTaskProvenanceForTest } = require("../core/media/image-task-runner.ts");
    const provenance = buildImageTaskProvenanceForTest({
      prompt: "TOP_SECRET_IMAGE_PROMPT 画一只猫",
      image: ["https://secret.example/ref-1.png", "https://secret.example/ref-2.png"],
      adapterId: "openai-images",
    })!;
    expect(provenance.inputShape).toBe("media_image");
    expect(provenance.sections.map((s: any) => s.category)).toEqual([
      "media_prompt",
      "media_reference",
      "media_reference",
    ]);
    expect(provenance.sections[0].locator).toEqual({ root: "parameters", path: ["prompt"] });
    expect(provenance.sections[1].locator).toEqual({ root: "parameters", path: ["image", 0] });
    const serialized = JSON.stringify(provenance);
    expect(serialized.includes("TOP_SECRET_IMAGE_PROMPT")).toBe(false);
    expect(serialized.includes("secret.example")).toBe(false);
  });

  it("speech: audio + language hint distinguishable, no audio/transcript content", () => {
    const { buildSpeechProvenanceForTest } = require("../core/speech-recognition-service.ts");
    const provenance = buildSpeechProvenanceForTest({ language: "zh" })!;
    expect(provenance.inputShape).toBe("speech_transcribe");
    expect(provenance.sections.map((s: any) => `${s.category}:${s.role}`)).toEqual([
      "audio_input:input",
      "language_hint:parameter",
    ]);
    expect(provenance.sections[0].locator).toEqual({ root: "input", path: ["audio"] });
    expect(provenance.sections[1].locator).toEqual({ root: "parameters", path: ["language"] });
    // language 值本身不进 section（source 只有固定 id）。
    expect(JSON.stringify(provenance).includes('"zh"')).toBe(false);
  });
});
