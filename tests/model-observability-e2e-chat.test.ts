/**
 * Phase 10 E2E Truth — MC-01 真实 Pi AgentSession chat（S1/S2，§一百三十六：
 * 走真实 session.prompt/chat 边界）。
 *
 * 真实链：Lingxi createAgentSession facade（安装 stream observer + trace ingress）
 * → Pi AgentSession.prompt → pi-ai openai-completions adapter → 真实 HTTP →
 * Fake Provider Witness（SSE）。无 mock fetch、无 fake session。
 *
 * 断言（§三十七/三十八）：
 *   S1 simple：1 trace / 1 call / origin / parent=null / 四层 payload /
 *     provider_request(runtime_exact hook body) ≡ witness body / witness 见毒丸。
 *   S2 tool loop：C1(toolCall) → tool 执行 → C2(text)，C2.parentCallId=C1.callId
 *     （绝不是 toolCallId）；同 trace。
 */
import fs from "node:fs";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { SessionManager } from "@earendil-works/pi-coding-agent";
import { createAgentSession } from "../lib/pi-sdk/index.ts";
import {
  createScenarioHarness,
  flushAsync,
  openaiCompletionsSseBody,
  type ScenarioHarness,
} from "./helpers/model-observability-scenario-harness.ts";

const POISON_KEY = "sk-E2E-CHAT-WITNESS-POISON-77cbe0f1";
const USER_INPUT = "E2E_CHAT_USER_输入查一下天气";

let harness: ScenarioHarness;
let modelRuntime: Awaited<ReturnType<typeof import("@earendil-works/pi-coding-agent").ModelRuntime.create>> | null = null;
let session: any = null;

beforeEach(async () => {
  harness = await createScenarioHarness();
});

afterEach(async () => {
  try { await session?.dispose?.(); } catch { /* best-effort */ }
  session = null;
  await harness.close();
  harness.cleanup();
});

/** 真实 Pi ModelRuntime：离线创建 + extension provider 注册（凭证/baseUrl 生产语义）。 */
async function createWitnessRuntime() {
  const { ModelRuntime } = await import("@earendil-works/pi-coding-agent");
  const runtime = await ModelRuntime.create({ modelsPath: null, refreshOnCreate: false });
  runtime.registerProvider("witness-provider", {
    name: "Witness Provider",
    baseUrl: `${harness.witness.baseUrl}/v1`,
    api: "openai-completions",
    apiKey: POISON_KEY,
    authHeader: true,
  } as any);
  modelRuntime = runtime;
  return runtime;
}

function witnessModel() {
  return {
    id: "witness-model",
    provider: "witness-provider",
    api: "openai-completions",
    baseUrl: `${harness.witness.baseUrl}/v1`,
    maxTokens: 1024,
    input: ["text"],
    cost: { input: 1, output: 2, cacheRead: 0.1, cacheWrite: 1.25, total: 0 },
  };
}

/** pi openai-completions SSE：普通文本回复。 */
function chatSse(content: string) {
  return { kind: "sse", body: openaiCompletionsSseBody({ content, usage: { prompt_tokens: 9, completion_tokens: 4, total_tokens: 13 } }) } as const;
}

/**
 * 生产等价的 resourceLoader：engine 用 DefaultResourceLoader({extensionFactories})
 * 注册 server 级扩展（engine.ts:2786 + server/index.ts:551 的 observer ext）。
 * 这里以同机制注入 observer ext；noExtensions 隔离 agentDir 文件扩展。
 */
async function observerResourceLoader() {
  const { DefaultResourceLoader } = await import("@earendil-works/pi-coding-agent");
  const { createModelCallObserverExtension } = await import("../lib/extensions/model-call-observer-ext.ts");
  const loader = new DefaultResourceLoader({
    cwd: harness.lingxiHome,
    agentDir: harness.lingxiHome,
    noExtensions: true,
    noSkills: true,
    noPromptTemplates: true,
    noThemes: true,
    noContextFiles: true,
    extensionFactories: [createModelCallObserverExtension()],
  } as any);
  await loader.reload();
  return loader;
}

describe("E2E truth — MC-01 真实 Pi chat", () => {
  it("S1 simple chat：1 trace/1 call；hook body ≡ witness body；四层 payload；毒丸不入库", async () => {
    const runtime = await createWitnessRuntime();
    harness.witness.scriptNext(chatSse("E2E_CHAT_ASSISTANT_REPLY 今天晴"));

    const created = await createAgentSession({
      model: witnessModel(),
      modelRuntime: runtime,
      sessionManager: SessionManager.inMemory(),
      noTools: "all",
      resourceLoader: await observerResourceLoader(),
      cwd: harness.lingxiHome,
    } as any);
    session = created.session;

    await session.prompt(USER_INPUT);
    await flushAsync(5);
    harness.flush();
    await flushAsync(3);

    /* witness：1 次 POST /chat/completions，凭证可见（redaction 只改 capture） */
    const posts = harness.witness.requestsTo("/chat/completions");
    expect(posts).toHaveLength(1);
    expect(posts[0].headers["authorization"]).toContain(POISON_KEY);
    const witnessBody = posts[0].bodyJson as any;
    expect(witnessBody.stream).toBe(true);
    expect(JSON.stringify(witnessBody.messages)).toContain(USER_INPUT);

    /* observer：1 call、attempt logical 边界内可见 */
    const callIds = harness.observer!.callIds();
    expect(callIds).toHaveLength(1);
    const callId = callIds[0];
    const identity = harness.observer!.callIdentity(callId)!;
    expect(identity.traceId).toBeTruthy();
    expect(identity.parentCallId).toBeNull();

    /* durable：四层 payload；provider_request = hook body（runtime_exact）≡ witness */
    const query = harness.query();
    const detail = query.queryCallDetail(callId);
    expect(detail.ok).toBe(true);
    if (!detail.ok) throw new Error("detail failed");
    const kinds = detail.value.payloadRecords.map((r: any) => r.kind).sort();
    expect(kinds).toEqual(["provider_request", "provider_response", "semantic_request", "semantic_response"]);

    const providerRequestMeta = detail.value.payloadRecords.find((r: any) => r.kind === "provider_request")!;
    const providerRequest = query.getPayloadRecord(providerRequestMeta.id);
    expect(providerRequest.ok).toBe(true);
    if (!providerRequest.ok) throw new Error("provider_request read failed");
    expect(providerRequest.value.fidelity).toBe("runtime_exact");
    const capturedBody = (providerRequest.value.payload as any).transport.body;
    expect(capturedBody.model).toBe(witnessBody.model);
    expect(capturedBody.stream).toBe(true);
    // §二十二：witness ≡ capture「除 redaction/externalization/truncation 明确
    // 改变的字段外一致」。system 消息内含本地绝对路径（pi docs 路径）→ 被
    // Redactor 内联替换（§三十 文档化行为）；用户消息不含路径 → 必须逐字一致。
    expect(capturedBody.messages).toHaveLength(witnessBody.messages.length);
    const witnessUser = witnessBody.messages.filter((m: any) => m.role === "user").pop();
    const capturedUser = capturedBody.messages.filter((m: any) => m.role === "user").pop();
    expect(capturedUser).toEqual(witnessUser);
    expect(JSON.stringify(capturedBody)).toContain(USER_INPUT);
    expect(JSON.stringify(capturedBody)).not.toContain("/Users/study_superior/");
    // redaction 事实在 DTO 以 sanitizationStatus 表达（详情 actions 在存储层）
    expect(String(providerRequest.value.sanitizationStatus ?? "")).toContain("redact");
    // Pi hook 不暴露 headers/endpoint（诚实 null/缺失）
    expect((providerRequest.value.payload as any).transport.headers ?? null).toBeNull();

    /* provider_response = metadata_only（hook 只有 status/headers，§一六二） */
    const providerResponseMeta = detail.value.payloadRecords.find((r: any) => r.kind === "provider_response")!;
    const providerResponse = query.getPayloadRecord(providerResponseMeta.id);
    expect(providerResponse.ok).toBe(true);
    if (providerResponse.ok) {
      expect(providerResponse.value.visibility).toBe("metadata_only");
    }

    /* semantic_response = assembled text */
    const semanticResponseMeta = detail.value.payloadRecords.find((r: any) => r.kind === "semantic_response")!;
    const semanticResponse = query.getPayloadRecord(semanticResponseMeta.id);
    expect(semanticResponse.ok).toBe(true);
    if (semanticResponse.ok) {
      expect((semanticResponse.value.payload as any).text).toBe("E2E_CHAT_ASSISTANT_REPLY 今天晴");
    }

    /* 毒丸：Witness 可见、DB 不可见 */
    harness.observer!.assertNoSensitiveContent([POISON_KEY]);
    const { scanStoreFilesForPoison } = await import("../lib/llm/model-observability-testing.ts");
    harness.flush();
    const files = [harness.dbPath, `${harness.dbPath}-wal`, `${harness.dbPath}-shm`]
      .filter((p) => fs.existsSync(p))
      .map((p) => ({ name: p, bytes: fs.readFileSync(p) }));
    const poisoned = scanStoreFilesForPoison(files, [POISON_KEY]);
    expect(poisoned.hit).toBe(false);
  }, 30_000);

  it("S2 tool loop：C1 toolCall → 工具执行 → C2 text；C2.parent=C1.callId（非 toolCallId）", async () => {
    const runtime = await createWitnessRuntime();

    // C1 回 toolCall；C2 回纯文本
    const toolCallChunk = {
      id: "chatcmpl-w1", object: "chat.completion.chunk", created: 0, model: "witness-model",
      choices: [{ index: 0, delta: { role: "assistant", tool_calls: [{ index: 0, id: "tc_e2e_1", type: "function", function: { name: "e2e_probe", arguments: "{\"city\":\"北京\"}" } }] }, finish_reason: null }],
    };
    const toolCallDone = {
      id: "chatcmpl-w1", object: "chat.completion.chunk", created: 0, model: "witness-model",
      choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }],
      usage: { prompt_tokens: 9, completion_tokens: 3, total_tokens: 12 },
    };
    const toolCallSse = [
      `data: ${JSON.stringify(toolCallChunk)}`,
      `data: ${JSON.stringify(toolCallDone)}`,
      "data: [DONE]",
      "",
    ].join("\n\n");
    harness.witness.scriptNext({ kind: "sse", body: toolCallSse });
    harness.witness.scriptNext(chatSse("E2E_CHAT_TOOLLOOP_REPLY 已查到"));

    const created = await createAgentSession({
      model: witnessModel(),
      modelRuntime: runtime,
      sessionManager: SessionManager.inMemory(),
      noTools: "all",
      resourceLoader: await observerResourceLoader(),
      customTools: [{
        name: "e2e_probe",
        description: "e2e probe tool",
        parameters: { type: "object", properties: { city: { type: "string" } }, required: ["city"] },
        // Pi 工具定义契约：execute(toolCallId, args) → { content: ContentBlock[] }。
        async execute(_toolCallId: string, args: any) {
          return { content: [{ type: "text", text: `E2E_TOOL_RESULT_${args.city}_25度` }] };
        },
      }],
      cwd: harness.lingxiHome,
    } as any);
    session = created.session;

    await session.prompt(USER_INPUT);
    await flushAsync(6);
    harness.flush();
    await flushAsync(3);

    /* witness：两轮 POST */
    expect(harness.witness.requestsTo("/chat/completions")).toHaveLength(2);
    // 第二轮 body 应包含 tool result（assistant toolCall + toolResult 消息）
    const secondBody = JSON.stringify(harness.witness.requestsTo("/chat/completions")[1].bodyJson);
    expect(secondBody).toContain("E2E_TOOL_RESULT_北京_25度");

    /* observer：2 calls，C2.parent=C1（因果：loop 内工具回流后继续推理） */
    const callIds = harness.observer!.callIds();
    expect(callIds).toHaveLength(2);
    const [c1, c2] = callIds;
    const identity1 = harness.observer!.callIdentity(c1)!;
    const identity2 = harness.observer!.callIdentity(c2)!;
    expect(identity1.traceId).toBe(identity2.traceId);
    expect(identity2.parentCallId).toBe(c1); // 直接 causative upstream call id
    expect(identity2.parentCallId).not.toContain("tc_"); // 绝不是 toolCallId
    harness.observer!.assertTraceGraphValid();

    /* durable：trace detail 体现 C1→C2 边 */
    const query = harness.query();
    const traceDetail = query.queryTraceDetail(identity1.traceId);
    expect(traceDetail.ok).toBe(true);
    if (traceDetail.ok) {
      const edge = traceDetail.value.edges.find((e: any) => e.childCallId === c2);
      expect(edge?.parentCallId).toBe(c1);
    }

    /* C2 的 semantic_response 含最终文本 */
    const detail2 = query.queryCallDetail(c2);
    expect(detail2.ok).toBe(true);
    if (detail2.ok) {
      const meta = detail2.value.payloadRecords.find((r: any) => r.kind === "semantic_response");
      const record = meta ? query.getPayloadRecord(meta.id) : null;
      if (record?.ok) {
        expect((record.value.payload as any).text).toBe("E2E_CHAT_TOOLLOOP_REPLY 已查到");
      }
    }
  }, 30_000);
});
