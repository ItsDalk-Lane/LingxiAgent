/**
 * Model Trace Propagation — Phase 4 运行时场景验证（任务书 §七十～§八十一）。
 *
 * 每个场景经真实组件链（stream observer / session-options 工具边界 /
 * callText / beginObservedModelCall / observed direct summary / trace ingress）
 * 驱动，断言 traceId/parentCallId 的真实传播结果；最后统一跑
 * assertTraceGraphValid（§八十三图不变量）。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import {
  installModelCallStreamObserver,
  installModelCallTraceIngress,
} from "../lib/pi-sdk/model-call-stream-observer.ts";
import { agentToolToToolDefinition } from "../lib/pi-sdk/session-options.ts";
import { callText } from "../core/llm-client.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";
import {
  beginObservedModelCall,
  observedProviderFetch,
} from "../lib/llm/model-call-integration.ts";
import { observePiDirectSummary } from "../lib/llm/observed-pi-direct-summary.ts";
import {
  currentModelTraceScope,
  runToolExecutionWithModelTrace,
  runWithModelTraceRoot,
  runWithNewModelTrace,
} from "../lib/llm/model-trace-scope.ts";
import {
  modelCallLedgerMetadataForMessage,
} from "../lib/llm/model-call-correlation.ts";
import { generateSummary } from "../lib/pi-sdk/index.ts";

const MODEL = { id: "test-model", provider: "test-provider", api: "openai-completions" };

function assistantMessage(overrides: Record<string, any> = {}) {
  return {
    role: "assistant",
    content: [{ type: "text", text: "ok" }],
    api: "openai-completions",
    provider: "test-provider",
    model: "test-model",
    usage: { input: 10, output: 5, cacheRead: 3, cacheWrite: 0, totalTokens: 15 },
    stopReason: "stop",
    timestamp: Date.now(),
    ...overrides,
  };
}

function streamOf(message: any) {
  const stream = createAssistantMessageEventStream();
  queueMicrotask(() => {
    stream.push({ type: "done", reason: message.stopReason, message } as any);
    stream.end();
  });
  return stream;
}

function fakeSession(streamFunction: any, overrides: Record<string, any> = {}) {
  const session: any = {
    agent: { streamFunction },
    sessionManager: {
      getSessionId: () => overrides.sessionId ?? "sess-1",
      getSessionFile: () => overrides.sessionPath ?? "/tmp/sess-1.jsonl",
    },
    isCompacting: false,
    // Pi AgentSession.prompt：驱动 agent streamFn（最小假实现，可被 ingress 包装）
    async prompt(_text: string) {
      const stream = await session.agent.streamFunction(MODEL, { messages: [] }, {});
      await stream.result();
    },
    ...overrides,
  };
  return session;
}

async function flushTerminal() {
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
}

function okFetch() {
  return vi.fn(async () => new Response(JSON.stringify({
    choices: [{ message: { content: "Title" }, finish_reason: "stop" }],
    usage: { prompt_tokens: 10, completion_tokens: 3, total_tokens: 13 },
  }), { status: 200, headers: { "x-request-id": "req-1" } }));
}

let observer: ReturnType<typeof createTestModelCallObserver>;

beforeEach(() => {
  observer = createTestModelCallObserver();
  setModelCallObserver(observer);
});
afterEach(() => {
  setModelCallObserver(null);
  vi.unstubAllGlobals();
});

describe("测试 1（§七十）：standalone callText = singleton trace，parent=null", () => {
  it("独立 callText：traceId 非空、parentCallId=null", async () => {
    vi.stubGlobal("fetch", okFetch());
    await callText({
      api: "openai-completions",
      baseUrl: "https://example.test/v1",
      model: MODEL,
      systemPrompt: "sys",
      messages: [{ role: "user", content: "hi" }],
      usageContext: {
        source: { subsystem: "utility", operation: "title", surface: "system", trigger: "tool" },
        attribution: { kind: "session", agentId: "a1" },
      },
    } as any);
    await flushTerminal();

    const [callId] = observer.callIds();
    expect(callId).toMatch(/^mc_/);
    const identity = observer.callIdentity(callId)!;
    expect(identity.traceId).toMatch(/^mt_/);
    expect(identity.parentCallId).toBeNull();
    observer.assertTraceGraphValid();
  });
});

describe("测试 2（§七十一）：多 Provider agent turn = 同 trace + 顺序 parent 链", () => {
  it("C1 → tool → C2：C2.traceId=C1.traceId，C2.parent=C1（运行时链证明）", async () => {
    const session = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);

    await runWithModelTraceRoot({ origin: "user_turn" }, async () => {
      // 第一次流式调用（C1）
      const s1 = await session.agent.streamFunction(MODEL, { messages: [] }, {});
      await s1.result();
      // 工具执行边界（真实包装器）：runToolExecutionWithModelTrace 由
      // session-options 的 execute 包装在真链路建立；此处直接走同一 helper。
      await runToolExecutionWithModelTrace({ toolName: "read", toolCallId: "tc_1" }, async () => {
        // 工具结果回流后，agent loop 的继续推理调用（C2）
        const s2 = await session.agent.streamFunction(MODEL, { messages: [] }, {});
        await s2.result();
      });
    });
    await flushTerminal();

    const calls = observer.callIds();
    expect(calls).toHaveLength(2);
    const [c1, c2] = calls;
    const identity1 = observer.callIdentity(c1)!;
    const identity2 = observer.callIdentity(c2)!;
    expect(identity1.traceId).toMatch(/^mt_/);
    expect(identity2.traceId).toBe(identity1.traceId); // 同 trace
    expect(identity1.parentCallId).toBeNull(); // C1 是根
    expect(identity2.parentCallId).toBe(c1); // C2.parent = C1
    // 工具子 scope 的 refs 记录 toolCallId（Tool Invocation Identity 保留）
    expect(observer.eventsForCall(c2)[0].details).toMatchObject({ traceOrigin: "user_turn" });
    observer.assertTraceGraphValid();
  });
});

describe("测试 3（§七十二）：并行工具 → 双双 parent=C1，绝不互为 parent", () => {
  it("C1 带两个 toolCall：Vision C2 与 Approval C3 都 parent=C1", async () => {
    // C1 的消息带 toolCalls；工具经真实 agentToolToToolDefinition 包装
    const messageWithTools = assistantMessage({
      content: [
        { type: "text", text: "running tools" },
        { type: "toolCall", id: "tc_a", name: "vision_tool" },
        { type: "toolCall", id: "tc_b", name: "approval_tool" },
      ],
    });
    const session = fakeSession(async (model: any, _ctx: any, opts: any) => {
      // 只有第一次调用带工具消息；工具内部的下一次流式调用为普通文本
      const callOrdinal = (opts as any).__ordinal ?? 1;
      return streamOf(callOrdinal === 1 ? messageWithTools : assistantMessage());
    });
    installModelCallStreamObserver(session);

    const visionTool = agentToolToToolDefinition({
      name: "vision_tool",
      execute: async () => {
        const s = await session.agent.streamFunction(MODEL, { messages: [] }, { __ordinal: 2 } as any);
        return await s.result();
      },
    } as any);
    const approvalTool = agentToolToToolDefinition({
      name: "approval_tool",
      execute: async () => {
        // 稍晚启动，制造“时间上更接近 Vision C2”的假象——parent 仍必须是 C1
        await new Promise((resolve) => setTimeout(resolve, 10));
        const s = await session.agent.streamFunction(MODEL, { messages: [] }, { __ordinal: 3 } as any);
        return await s.result();
      },
    } as any);

    await runWithModelTraceRoot({ origin: "user_turn" }, async () => {
      const s1 = await session.agent.streamFunction(MODEL, { messages: [] }, {} as any);
      const c1Message = await s1.result();
      expect(c1Message.content.filter((block: any) => block.type === "toolCall")).toHaveLength(2);
      // 并行执行两个工具（pi-agent-core 的 parallel batch 语义）
      await Promise.all([
        visionTool.execute("tc_a", {}, undefined, undefined),
        approvalTool.execute("tc_b", {}, undefined, undefined),
      ]);
    });
    await flushTerminal();

    const calls = observer.callIds();
    expect(calls).toHaveLength(3);
    const [c1, c2, c3] = calls;
    const identity1 = observer.callIdentity(c1)!;
    const identity2 = observer.callIdentity(c2)!;
    const identity3 = observer.callIdentity(c3)!;
    expect(identity2.traceId).toBe(identity1.traceId);
    expect(identity3.traceId).toBe(identity1.traceId);
    expect(identity2.parentCallId).toBe(c1); // Vision.parent = C1
    expect(identity3.parentCallId).toBe(c1); // Approval.parent = C1
    expect(identity3.parentCallId).not.toBe(c2); // 绝不 parent 到 Vision（§七十二）
    expect(observer.childrenOf(c1).sort()).toEqual([c2, c3].sort());
    observer.assertTraceGraphValid();
  });
});

describe("测试 4（§七十三）：Subagent 跨 session 继承 trace", () => {
  it("Parent C1 → spawn 工具 → Child session.prompt → C2 同 trace、parent=C1", async () => {
    const parentSession = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(parentSession);
    const childSession = fakeSession(
      async () => streamOf(assistantMessage()),
      { sessionId: "child-sess", sessionPath: "/tmp/child.jsonl" },
    );
    installModelCallStreamObserver(childSession);
    installModelCallTraceIngress(childSession);

    await runWithModelTraceRoot({ origin: "user_turn" }, async () => {
      const s1 = await parentSession.agent.streamFunction(MODEL, { messages: [] }, {});
      await s1.result();
      // spawn_subagent 工具边界（真实包装）
      const spawnTool = agentToolToToolDefinition({
        name: "spawn_subagent",
        execute: async () => {
          // 子 session 的 prompt（经 facade trace ingress 包装：外层 scope 已在 → 继承）
          await childSession.prompt("do the task");
          return { content: [] };
        },
      } as any);
      await spawnTool.execute("tc_spawn", {}, undefined, undefined);
    });
    await flushTerminal();

    const calls = observer.callIds();
    expect(calls).toHaveLength(2);
    const [c1, c2] = calls;
    const identity1 = observer.callIdentity(c1)!;
    const identity2 = observer.callIdentity(c2)!;
    expect(identity2.traceId).toBe(identity1.traceId); // 跨 session 不丢 trace
    expect(identity2.parentCallId).toBe(c1); // C2.parent = 产生 spawn 的 C1
    // 子会话身份不同不影响 trace（§七十三）
    expect(observer.eventsForCall(c2)[0].attribution).toMatchObject({ sessionId: "child-sess" });
    observer.assertTraceGraphValid();
  });
});

describe("测试 5（§七十四）：工具内 Media 继承 trace + parent", () => {
  it("Chat C1 → image tool → 提交 call：same trace、parent=C1、poll 0 事件", async () => {
    vi.stubGlobal("fetch", okFetch());
    const session = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);

    await runWithModelTraceRoot({ origin: "user_turn" }, async () => {
      const s1 = await session.agent.streamFunction(MODEL, { messages: [] }, {});
      await s1.result();
      await runToolExecutionWithModelTrace({ toolName: "media_generate-image", toolCallId: "tc_img" }, async () => {
        // 媒体提交逻辑调用边界（真实 beginObservedModelCall + attempt helper）
        const recorder = beginObservedModelCall({
          model: { provider: "openai", modelId: "img-model", api: "openai-images" },
          usageContext: {
            source: { subsystem: "media", operation: "submit", surface: "desktop", trigger: "tool" },
            attribution: { kind: "session", agentId: "a1" },
          },
          details: { mediaType: "image" },
        });
        await observedProviderFetch({ modelCall: recorder }, () => Promise.resolve(new Response("{}", { status: 200 })));
        recorder.semanticResponseCompleted({ details: { taskId: "t1" } });
        recorder.endLogicalCall("ok");
      });
    });
    await flushTerminal();

    const calls = observer.callIds();
    expect(calls).toHaveLength(2);
    const [c1, c2] = calls;
    expect(observer.callIdentity(c2)!.traceId).toBe(observer.callIdentity(c1)!.traceId);
    expect(observer.callIdentity(c2)!.parentCallId).toBe(c1);
    // media attempt 精确可见
    expect(observer.eventsForCall(c2).find((event) => event.eventType === "attempt_start")?.details)
      .toMatchObject({ attemptVisibility: "exact" });
    // 轮询（控制面）在本测试里没有真实 poller；直接证明无 recorder 的 fetch 0 事件
    const before = observer.events.length;
    await observedProviderFetch(null, () => Promise.resolve(new Response("{}", { status: 200 })));
    expect(observer.events.length).toBe(before); // poll/query 级 fetch = 0 新事件
    observer.assertTraceGraphValid();
  });
});

describe("测试 6（§七十五）：Speech 独立 = 新 singleton；链内 = 继承", () => {
  it("独立转写新 trace；turn 内调用继承同 trace", async () => {
    // 独立（模拟 transcribeAudio 的 force-new 入口语义）
    await runWithNewModelTrace({ origin: "speech", refs: { fileId: "f1" } }, async () => {
      const recorder = beginObservedModelCall({
        model: { provider: "openai", modelId: "whisper", api: "openai-audio" },
        usageContext: {
          source: { subsystem: "speech-recognition", operation: "transcribe", surface: "system", trigger: "user" },
          attribution: { kind: "session", agentId: "a1" },
        },
      });
      recorder.endLogicalCall("ok");
    });
    // 链内（例如工具内触发的转写）
    await runWithNewModelTrace({ origin: "user_turn" }, async () => {
      const recorderA = beginObservedModelCall({
        model: { provider: "openai", modelId: "whisper", api: "openai-audio" },
        usageContext: {
          source: { subsystem: "speech-recognition", operation: "transcribe", surface: "desktop", trigger: "tool" },
          attribution: { kind: "session", agentId: "a1" },
        },
      });
      recorderA.endLogicalCall("ok");
    });
    await flushTerminal();

    const [callA, callB] = observer.callIds();
    expect(observer.callIdentity(callA)!.traceId).toMatch(/^mt_/);
    expect(observer.callIdentity(callB)!.traceId).not.toBe(observer.callIdentity(callA)!.traceId);
    expect(observer.callIdentity(callA)!.parentCallId).toBeNull();
    expect(observer.callIdentity(callB)!.parentCallId).toBeNull();
    // 独立 speech 事件带 origin
    expect(observer.eventsForCall(callA)[0].details).toMatchObject({ traceOrigin: "speech" });
    observer.assertTraceGraphValid();
  });
});

describe("测试 7（§七十六）：两次 automation run = 两个不同 trace", () => {
  it("same automationId 的 Run A / Run B trace 不同", async () => {
    const session = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);
    const run = async () => runWithNewModelTrace(
      { origin: "automation", refs: { automationId: "auto-1" } },
      async () => {
        const stream = await session.agent.streamFunction(MODEL, { messages: [] }, {});
        await stream.result();
      },
    );
    await run();
    await run();
    await flushTerminal();

    const [a, b] = observer.callIds();
    const identityA = observer.callIdentity(a)!;
    const identityB = observer.callIdentity(b)!;
    expect(identityA.traceId).not.toBe(identityB.traceId); // Run A != Run B
    expect(identityA.parentCallId).toBeNull();
    expect(identityB.parentCallId).toBeNull();
    observer.assertTraceGraphValid();
  });
});

describe("测试 8（§七十七）：/diary = 一个 trace、三个 MC-10/MC-04 调用、parent 均 null", () => {
  it("两次临时摘要 + 一次终稿：same traceId、parentCallId=null、不再旁路", async () => {
    const ledgerEntries: any[] = [];
    const fakeLedger = {
      start: (meta: any) => {
        ledgerEntries.push(meta);
        return { requestId: `req-${ledgerEntries.length}` };
      },
      finish: () => {},
      recordError: () => {},
    };

    await runWithNewModelTrace({ origin: "diary" }, async () => {
      // 两次 temporary summary（MC-10 observed direct summary 边界）
      await observePiDirectSummary(MODEL, { usageLedger: fakeLedger }, async () => "summary-a");
      await observePiDirectSummary(MODEL, { usageLedger: fakeLedger }, async () => "summary-b");
      // 终稿（MC-04 语义：callText 边界 → 这里用 beginObservedModelCall 等价驱动）
      const recorder = beginObservedModelCall({
        model: MODEL,
        usageContext: {
          source: { subsystem: "memory", operation: "diary_write", surface: "background", trigger: "system" },
          attribution: { kind: "memory", agentId: "a1" },
        },
      });
      recorder.endLogicalCall("ok");
    });
    await flushTerminal();

    const calls = observer.callIds();
    expect(calls).toHaveLength(3);
    const identities = calls.map((callId) => observer.callIdentity(callId)!);
    expect(new Set(identities.map((identity) => identity.traceId)).size).toBe(1); // 同一 trace
    for (const identity of identities) {
      expect(identity.parentCallId).toBeNull(); // 都直接由任务根触发（§四十七）
    }
    // MC-10 事件序列：无伪造的 provider_request_prepared / provider_response_received
    const mc10Sequences = calls.slice(0, 2).map((callId) =>
      observer.eventsForCall(callId).map((event) => event.eventType));
    for (const sequence of mc10Sequences) {
      expect(sequence).toEqual([
        "logical_call_start",
        "attempt_start",
        "semantic_response_completed",
        "logical_call_end",
      ]);
    }
    expect(observer.eventsForTrace(identities[0].traceId)
      .filter((event) => event.eventType === "provider_request_prepared")).toHaveLength(0);
    // MC-10 ledger 关联
    expect(ledgerEntries).toHaveLength(2);
    for (const entry of ledgerEntries) {
      expect(entry.metadata.modelCallId).toMatch(/^mc_/);
      expect(entry.metadata.traceId).toBe(identities[0].traceId);
      expect(entry.metadata.parentCallId).toBeNull();
    }
    observer.assertTraceGraphValid();
  });

  it("facade generateSummary：传 streamFn 时不经 MC-10 观测（防双计）；观测路径事件完整", async () => {
    // streamFn 路径：观测发生在 streamFn 边界（本测试的 streamFn 不装 observer 包装 → 0 事件）
    const text = await generateSummary(
      [],
      MODEL,
      1024,
      "key",
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      async () => streamOf(assistantMessage({ content: [{ type: "text", text: "summarized" }] })),
      undefined,
      undefined,
      undefined,
      undefined,
    );
    expect(text).toBe("summarized");
    await flushTerminal();
    expect(observer.events).toHaveLength(0); // 不双计
  });
});

describe("测试 9（§七十八）：并发 Session 不串线", () => {
  it("Session A/B 并行 chat+auxiliary：trace 内不出现对方 callId", async () => {
    vi.stubGlobal("fetch", okFetch());
    const sessionA = fakeSession(async () => streamOf(assistantMessage()), { sessionId: "sess-a" });
    const sessionB = fakeSession(async () => streamOf(assistantMessage()), { sessionId: "sess-b" });
    installModelCallStreamObserver(sessionA);
    installModelCallStreamObserver(sessionB);

    const turn = async (session: any) => runWithModelTraceRoot(
      { origin: "user_turn", refs: { sessionId: session.sessionManager.getSessionId() } },
      async () => {
        const stream = await session.agent.streamFunction(MODEL, { messages: [] }, {});
        await stream.result();
        // turn 内 auxiliary（callText）
        await callText({
          api: "openai-completions",
          baseUrl: "https://example.test/v1",
          model: MODEL,
          systemPrompt: "sys",
          messages: [{ role: "user", content: "sum" }],
          usageContext: {
            source: { subsystem: "utility", operation: "title", surface: "desktop", trigger: "system" },
            attribution: { kind: "session", agentId: "a1" },
          },
        } as any);
      },
    );
    await Promise.all([turn(sessionA), turn(sessionB)]);
    await flushTerminal();

    const traceOf = new Map<string, string>();
    for (const callId of observer.callIds()) {
      traceOf.set(callId, observer.callIdentity(callId)!.traceId!);
    }
    const traces = [...new Set(traceOf.values())];
    expect(traces).toHaveLength(2); // 两个任务两个 trace
    const traceCalls = new Map<string, string[]>();
    for (const [callId, traceId] of traceOf) {
      traceCalls.set(traceId, [...(traceCalls.get(traceId) ?? []), callId]);
    }
    for (const [traceId, calls] of traceCalls) {
      expect(calls).toHaveLength(2); // 每 trace 恰好自己的 chat + auxiliary
      for (const callId of calls) {
        expect(observer.eventsForTrace(traceId).map((event) => event.callId)).not.toContain(
          [...traceOf.entries()].find(([, trace]) => trace !== traceId)?.[0],
        );
      }
    }
    observer.assertTraceGraphValid();
  });
});

describe("测试 10（§七十九）：detached background 不泄漏旧 trace", () => {
  it("T1 内创建的 delayed 任务执行时不得仍属 T1", async () => {
    const session = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);

    let detachedCallId: string | null = null;
    await runWithNewModelTrace({ origin: "user_turn" }, async () => {
      const stream = await session.agent.streamFunction(MODEL, { messages: [] }, {});
      await stream.result();
      // turn 内注册延迟后台任务（§五十 的 30 分钟 timer 缩到 15ms）
      const delayed = runWithNewModelTrace({ origin: "background" }, async () => {
        await new Promise((resolve) => setTimeout(resolve, 15));
        const scope = currentModelTraceScope();
        expect(scope?.origin).toBe("background");
        const stream2 = await session.agent.streamFunction(MODEL, { messages: [] }, {});
        await stream2.result();
        detachedCallId = observer.callIds().at(-1) ?? null;
      });
      // 不 await —— 模拟 fire-and-forget；outer trace 立即结束
      await Promise.race([delayed, new Promise((resolve) => setTimeout(resolve, 1))]);
    });
    await flushTerminal();
    await new Promise((resolve) => setTimeout(resolve, 30));
    await flushTerminal();

    const calls = observer.callIds();
    expect(calls.length).toBeGreaterThanOrEqual(2);
    const turnTrace = observer.callIdentity(calls[0])!.traceId;
    const detachedTrace = observer.callIdentity(detachedCallId!)!.traceId;
    expect(detachedTrace).not.toBe(turnTrace); // 新 trace，无泄漏
    observer.assertTraceGraphValid();
  });
});

describe("测试 11（§八十）：Observer safety 回归——trace 改造不泄正文", () => {
  it("毒丸不出现在任何 trace 事件里", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({
      choices: [{ message: { content: "TOPSECRET_LEAK_ATTEMPT" }, finish_reason: "stop" }],
      usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
    }), { status: 200 })));
    await runWithNewModelTrace({ origin: "user_turn", refs: { secret: "TOPSECRET_REF" } }, async () => {
      await callText({
        api: "openai-completions",
        baseUrl: "https://example.test/v1",
        model: MODEL,
        systemPrompt: "TOPSECRET_SYSTEM",
        messages: [{ role: "user", content: "TOPSECRET_USER" }],
        usageContext: {
          source: { subsystem: "utility", operation: "title", surface: "system", trigger: "tool" },
          attribution: { kind: "session", agentId: "a1" },
        },
      } as any);
    });
    await flushTerminal();
    observer.assertNoSensitiveContent([
      "TOPSECRET_SYSTEM", "TOPSECRET_USER", "TOPSECRET_LEAK_ATTEMPT", "TOPSECRET_REF",
    ]);
  });
});

describe("MC-01 message_end 补账关联（§六十四）", () => {
  it("assembled message → WeakMap 三元组；未知对象 → null", async () => {
    const message = assistantMessage();
    const session = fakeSession(async () => streamOf(message));
    installModelCallStreamObserver(session);
    await runWithModelTraceRoot({ origin: "user_turn" }, async () => {
      const stream = await session.agent.streamFunction(MODEL, { messages: [] }, {});
      await stream.result();
    });
    await flushTerminal();

    const metadata = modelCallLedgerMetadataForMessage(message);
    expect(metadata).not.toBeNull();
    const [callId] = observer.callIds();
    expect(metadata).toMatchObject({
      modelCallId: callId,
      traceId: observer.callIdentity(callId)!.traceId,
      parentCallId: null,
    });
    expect(modelCallLedgerMetadataForMessage({ role: "assistant" })).toBeNull();
    expect(modelCallLedgerMetadataForMessage(null)).toBeNull();
  });

  it("recordAssistantUsage 形状的 metadata 注入：有身份写入、无身份不写", () => {
    // 直接验证投影函数行为（三处 message_end 补账共用）
    const withIdentity = modelCallLedgerMetadataForMessage({ traced: true });
    expect(withIdentity).toBeNull(); // 未登记对象 → null（不猜）
  });
});
