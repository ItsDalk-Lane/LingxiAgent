/**
 * Phase 5 MC-04 callText × Semantic Input Provenance — Fake Provider e2e。
 *
 * 覆盖任务书 §五十九～§六十一 + §一百零四（Provider Payload 不变）：
 *   - 显式 provenance（传入形状）随 system merge 同步 remap，summary 进
 *     logical_call_start details；
 *   - 无 provenance caller → structural fallback（不得 exact）；
 *   - codex 空系统注入 DEFAULT_CODEX_UTILITY_INSTRUCTIONS → adapter_injected 段；
 *   - 传/不传 provenance 的 wire body 逐字节一致（provenance 是 sidecar，
 *     绝不进 payload，§一百零五）。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { callText } from "../core/llm-client.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";
import {
  createSemanticInputProvenance,
  provenanceSection,
} from "../lib/llm/semantic-input-provenance.ts";

// llm-client 的模块私有常量（镜像断言用；改文案时同步改这里）。
const DEFAULT_CODEX_UTILITY_INSTRUCTIONS = [
  "You are Hana's utility model.",
  "Follow the user request exactly and return only the requested content.",
].join("\n");

const MODEL = { id: "gpt-5-mini", provider: "openai" };
const BASE_URL = "https://example.test/v1";
const USAGE_CONTEXT = {
  source: { subsystem: "utility", operation: "title", surface: "system", trigger: "tool" },
  attribution: { kind: "session", agentId: "agent-1" },
};

function okFetch() {
  return vi.fn(async (_url: unknown, _init: unknown) => new Response(JSON.stringify({
    choices: [{ message: { content: "Title" }, finish_reason: "stop" }],
    usage: { prompt_tokens: 10, completion_tokens: 3, total_tokens: 13 },
  }), { status: 200 }));
}

function okCodexFetch() {
  // codex responses 走 SSE 流（readCodexResponsesStream 解析 data: 行）。
  const sse = [
    `data: ${JSON.stringify({ type: "response.completed", response: { output_text: "ok", usage: { input_tokens: 4, output_tokens: 2, total_tokens: 6 } } })}`,
    "",
    "data: [DONE]",
    "",
  ].join("\n");
  return vi.fn(async () => new Response(sse, { status: 200 }));
}

function baseOptions(extra: Record<string, unknown> = {}) {
  return {
    api: "openai-completions",
    baseUrl: BASE_URL,
    model: MODEL,
    systemPrompt: "",
    messages: [
      { role: "system", content: "SYS_ONE TOP_SECRET_SYSTEM" },
      { role: "user", content: "hello TOP_SECRET_USER" },
    ],
    usageContext: USAGE_CONTEXT,
    ...extra,
  } as any;
}

describe("MC-04 callText semantic input provenance", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;
  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
  });
  afterEach(() => {
    setModelCallObserver(null);
    vi.unstubAllGlobals();
  });

  it("remaps explicit provenance through system merge and emits summary at start", async () => {
    const fetchMock = okFetch();
    vi.stubGlobal("fetch", fetchMock);
    const provenance = createSemanticInputProvenance("calltext", [
      provenanceSection(
        { root: "messages", path: [0], span: { start: 0, end: 8 } },
        "task_instruction",
        { role: "system", source: { type: "template", id: "title.system" } },
      ),
      provenanceSection(
        { root: "messages", path: [1] },
        "task_input",
        { role: "user", source: { type: "runtime", id: "title.user" } },
      ),
    ])!;

    await callText(baseOptions({ semanticInputProvenance: provenance }));

    const callId = observer.callIds()[0];
    const attached = observer.provenanceForCall(callId)!;
    // system 消息 remap 到 merged systemPrompt：span [0,8)（merged 无 prefix）。
    const systemSection = attached.sections.find((s) => s.locator.root === "systemPrompt")!;
    expect(systemSection.category).toBe("task_instruction");
    expect(systemSection.locator.span).toEqual({ start: 0, end: 8 });
    // user 消息重排到 index 0。
    const userSection = attached.sections.find((s) => s.locator.root === "messages")!;
    expect(userSection.locator.path).toEqual([0]);
    // summary 安全 metadata（§七十六）。
    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.details).toMatchObject({
      inputShape: "calltext",
      provenancePrecision: "exact",
      inputSectionCount: 2,
      inputCategories: ["task_instruction", "task_input"],
      opaqueSectionCount: 0,
    });
    observer.assertNoSensitiveContent(["TOP_SECRET_SYSTEM", "TOP_SECRET_USER"]);
  });

  it("falls back to structural provenance when caller provides none (never exact)", async () => {
    vi.stubGlobal("fetch", okFetch());
    await callText(baseOptions());
    const callId = observer.callIds()[0];
    const attached = observer.provenanceForCall(callId)!;
    expect(attached.sections.map((s) => `${s.category}/${s.precision}`)).toEqual([
      "task_instruction/structural",
      "task_input/structural",
    ]);
    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.details?.provenancePrecision).toBe("partial");
  });

  it("wire body is byte-identical with and without provenance (sidecar, §一百零五)", async () => {
    const withMock = okFetch();
    vi.stubGlobal("fetch", withMock);
    await callText(baseOptions());
    const bodyWithout = (withMock.mock.calls[0]![1] as any).body as string;

    const withMock2 = okFetch();
    vi.stubGlobal("fetch", withMock2);
    const provenance = createSemanticInputProvenance("calltext", [
      provenanceSection({ root: "messages", path: [1] }, "task_input", { role: "user" }),
    ])!;
    await callText(baseOptions({ semanticInputProvenance: provenance }));
    const bodyWith = (withMock2.mock.calls[0]![1] as any).body as string;
    expect(bodyWith).toBe(bodyWithout);
    // system merge 结果确实在 payload 里（provenance 描述的就是它）。
    const parsed = JSON.parse(bodyWithout);
    const systemMessage = parsed.messages.find((m: any) => m.role === "system");
    expect(systemMessage.content).toBe("SYS_ONE TOP_SECRET_SYSTEM");
  });

  it("codex empty system → adapter_injected section for the injected default instructions", async () => {
    vi.stubGlobal("fetch", okCodexFetch());
    await callText({
      api: "openai-codex-responses",
      baseUrl: "https://chatgpt.com/backend-api",
      apiKey: "header.payload.sig",
      model: { id: "gpt-5-codex", provider: "openai", accountId: "acct-1" },
      systemPrompt: "",
      messages: [{ role: "user", content: "u" }],
      usageContext: USAGE_CONTEXT,
    } as any);

    const callId = observer.callIds()[0];
    const attached = observer.provenanceForCall(callId)!;
    const injected = attached.sections.find((s) => s.category === "adapter_injected")!;
    expect(injected).toBeDefined();
    expect(injected.precision).toBe("exact");
    expect(injected.source).toEqual({ type: "adapter", id: "codex-utility-default-instructions" });
    expect(injected.locator.span).toEqual({
      start: 0,
      end: DEFAULT_CODEX_UTILITY_INSTRUCTIONS.length,
    });
  });
});
