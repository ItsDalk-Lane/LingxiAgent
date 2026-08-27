/**
 * Pi streamFunction 统一观测接点的运行时验证（MC-01/02/03，Scenario E/F/G）。
 *
 * 覆盖：
 *   - MC-01：普通 chat 的 callId 在 original streamFn 被调用（=Provider 请求）
 *     之前已存在；注册归属正确映射 source/attribution。
 *   - MC-03：session.isCompacting → native summarization 分类（以前是盲区）。
 *   - MC-02：cache-preserving AgentRun 经显式 ALS scope 接管 callId，且业务级
 *     tool recovery 产生第二个 logical call；ledger metadata.modelCallId 关联。
 *   - 终态：stream error / abort / pre-stream throw。
 *   - 旁路：observer 爆炸不影响业务结果。
 *   - extension hooks：before/after provider hook 经 ALS scope 关联，且绝不
 *     修改 payload、绝不记录正文。
 */
import { describe, it, expect, afterEach } from "vitest";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import {
  installModelCallStreamObserver,
  registerSessionModelCallContext,
} from "../lib/pi-sdk/model-call-stream-observer.ts";
import { createModelCallObserverExtension } from "../lib/extensions/model-call-observer-ext.ts";
import {
  currentModelCallScope,
  runWithModelCallScope,
} from "../lib/llm/model-call-scope.ts";
import {
  setModelCallObserver,
  type ModelCallObserver,
} from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";
import { runCachePreservingCompactionAgentRun } from "../lib/llm/cache-preserving-compaction-agent-run.ts";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";

const usage = {
  input: 10,
  output: 5,
  cacheRead: 3,
  cacheWrite: 0,
  totalTokens: 15,
};

function assistantMessage(overrides: Record<string, any> = {}) {
  return {
    role: "assistant",
    content: [{ type: "text", text: "ok" }],
    api: "openai-completions",
    provider: "test-provider",
    model: "test-model",
    usage,
    stopReason: "stop",
    timestamp: Date.now(),
    ...overrides,
  };
}

function streamOf(message: any) {
  const stream = createAssistantMessageEventStream();
  queueMicrotask(() => {
    if (message.stopReason === "error" || message.stopReason === "aborted") {
      stream.push({ type: "error", reason: message.stopReason, error: message } as any);
    } else {
      stream.push({ type: "done", reason: message.stopReason, message } as any);
    }
    stream.end();
  });
  return stream;
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

/** 等 observer 的 result().then 终态观察跑完。 */
async function flushTerminal() {
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
}

afterEach(() => {
  setModelCallObserver(null);
});

describe("installModelCallStreamObserver — MC-01 普通 chat", () => {
  it("callId 在 original streamFn（Provider 请求）之前已存在，生命周期完整", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);

    let scopeSeenInsideProvider: string | null = null;
    const session = fakeSession(async () => {
      // 关键断言：进入 Provider 调用时 ALS scope 已带 callId（pre-request identity）
      scopeSeenInsideProvider = currentModelCallScope()?.callId ?? null;
      return streamOf(assistantMessage());
    });
    installModelCallStreamObserver(session);
    registerSessionModelCallContext(session, () => ({
      source: { subsystem: "session", operation: "reply", surface: "desktop", trigger: "user" },
      attribution: { kind: "session", agentId: "agent-1", sessionId: "sess-1", sessionPath: "/tmp/sess-1.jsonl" },
    }));

    const stream = await session.agent.streamFunction(MODEL, { messages: [] }, {});
    await stream.result();
    await flushTerminal();

    const starts = observer.eventsOfType("logical_call_start");
    expect(starts).toHaveLength(1);
    expect(starts[0].callId).toMatch(/^mc_/);
    // pre-request identity：Provider 调用点看到的 callId 与事件一致
    expect(scopeSeenInsideProvider).toBe(starts[0].callId);
    expect(starts[0]).toMatchObject({
      model: { provider: "test-provider", modelId: "test-model", api: "openai-completions" },
      source: { subsystem: "session", operation: "reply", surface: "desktop", trigger: "user" },
      attribution: { kind: "session", agentId: "agent-1", sessionId: "sess-1" },
    });

    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "semantic_response_completed",
      "logical_call_end",
    ]);
    const attempt = observer.eventsOfType("attempt_start")[0];
    expect(attempt.attemptId).toMatch(/^ma_/);
    expect(attempt.attemptId).not.toBe(starts[0].callId);
    expect(attempt.details).toMatchObject({ attemptVisibility: "logical_boundary" });
    expect(observer.eventsOfType("semantic_response_completed")[0].details).toMatchObject({
      hasText: true,
      stopReason: "stop",
      usagePresent: true,
      usage: { input: 10, output: 5 },
    });
    expect(observer.eventsOfType("logical_call_end")[0].status).toBe("ok");
    // 全事件同一 callId
    expect(observer.callIds()).toEqual([starts[0].callId]);
  });

  it("未注册归属时诚实落 unknown，并带 sessionId/sessionPath", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const session = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(MODEL, {}, {});
    await stream.result();
    await flushTerminal();

    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.source).toMatchObject({ subsystem: "session", surface: "unknown" });
    expect(start.attribution).toMatchObject({
      kind: "unknown",
      sessionId: "sess-1",
      sessionPath: "/tmp/sess-1.jsonl",
    });
  });

  it("stream error 终态：attempt_error + logical_call_error + end(error)", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const session = fakeSession(async () => streamOf(assistantMessage({
      stopReason: "error",
      errorMessage: "provider exploded",
      content: [],
    })));
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(MODEL, {}, {});
    await stream.result();
    await flushTerminal();

    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
    // Phase 2.5 错误安全契约：provider 流错误正文（errorMessage）不得进入
    // Observer——message=null，只留 name 结构事实。
    const logicalError = observer.eventsOfType("logical_call_error")[0];
    expect(logicalError.error).toEqual({ name: "Error", message: null, code: null });
    expect(JSON.stringify(observer.events)).not.toContain("provider exploded");
    expect(observer.eventsOfType("logical_call_end")[0].status).toBe("error");
  });

  it("abort 终态：logical_call_aborted + end(aborted)", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const session = fakeSession(async () => streamOf(assistantMessage({
      stopReason: "aborted",
      content: [],
    })));
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(MODEL, {}, {});
    await stream.result();
    await flushTerminal();

    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "logical_call_aborted",
      "logical_call_end",
    ]);
    expect(observer.eventsOfType("logical_call_end")[0].status).toBe("aborted");
  });

  it("pre-stream throw：终态记录后错误照常抛出（行为不变）", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const session = fakeSession(async () => {
      throw new Error("credential boundary failure");
    });
    installModelCallStreamObserver(session);

    await expect(session.agent.streamFunction(MODEL, {}, {})).rejects.toThrow("credential boundary failure");
    expect(observer.sequence()).toEqual([
      "logical_call_start",
      "attempt_start",
      "attempt_error",
      "logical_call_error",
      "logical_call_end",
    ]);
  });

  it("observer 爆炸不影响业务：stream 正常返回", async () => {
    const exploding: ModelCallObserver = {
      handleModelCallEvent() {
        throw new Error("observer exploded");
      },
    };
    setModelCallObserver(exploding);
    const session = fakeSession(async () => streamOf(assistantMessage()));
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(MODEL, {}, {});
    const message = await stream.result();
    expect(message.stopReason).toBe("stop");
    await flushTerminal();
  });
});

describe("installModelCallStreamObserver — MC-03 native compaction", () => {
  it("isCompacting 会话的调用被分类为 compaction（不再是盲区）", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const session = fakeSession(async () => streamOf(assistantMessage()), { isCompacting: true });
    installModelCallStreamObserver(session);

    const stream = await session.agent.streamFunction(MODEL, {}, {});
    await stream.result();
    await flushTerminal();

    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.source).toMatchObject({ subsystem: "compaction", operation: "compact" });
    expect(start.details).toMatchObject({ path: "pi_stream", nativeSummarization: true });
    expect(start.usageCorrelation).toBe("not_correlated");
    expect(observer.events.every((event) => event.usageCorrelation === "not_correlated")).toBe(true);
    expect(observer.eventsOfType("logical_call_end")[0].status).toBe("ok");
  });

  it("真实 native stream wrapper 把 not_correlated 作为运行时事实持久化", async () => {
    const harness = createModelObservabilityTestHarness();
    setModelCallObserver(harness.handle.observer);
    try {
      const session = fakeSession(async () => streamOf(assistantMessage()), { isCompacting: true });
      installModelCallStreamObserver(session);

      const stream = await session.agent.streamFunction(MODEL, {}, {});
      await stream.result();
      await flushTerminal();
      harness.flush();

      const reader = harness.openReader();
      try {
        const rows = reader.db.prepare(
          `SELECT subsystem, operation, usage_correlation_state FROM model_calls`,
        ).all();
        expect(rows).toEqual([{
          subsystem: "compaction",
          operation: "compact",
          usage_correlation_state: "not_correlated",
        }]);
      } finally {
        reader.close();
      }
    } finally {
      setModelCallObserver(null);
      await harness.close();
      harness.cleanup();
    }
  });
});

describe("installModelCallStreamObserver — MC-02 显式 scope", () => {
  it("显式 ALS scope 接管 callId 并 merge details", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const session = fakeSession(async () => streamOf(assistantMessage()), { isCompacting: true });
    installModelCallStreamObserver(session);

    const stream = await runWithModelCallScope({
      callId: "mc_runner_owned",
      source: { subsystem: "compaction", operation: "fresh_compact", surface: "desktop", trigger: "threshold" },
      attribution: { kind: "session", sessionPath: "/tmp/sess-1.jsonl" },
      details: { compactionPhase: "strict" },
    }, () => session.agent.streamFunction(MODEL, {}, {}));
    await stream.result();
    await flushTerminal();

    // 显式 scope 优先于 isCompacting 推断
    const start = observer.eventsOfType("logical_call_start")[0];
    expect(start.callId).toBe("mc_runner_owned");
    expect(start.source).toMatchObject({ operation: "fresh_compact" });
    expect(start.details).toMatchObject({ compactionPhase: "strict" });
    expect(start.details).not.toHaveProperty("nativeSummarization");
    expect(start).not.toHaveProperty("usageCorrelation");
    expect(observer.callIds()).toEqual(["mc_runner_owned"]);
  });
});

describe("model-call-observer-ext — provider hooks 经 ALS scope 关联", () => {
  function piMock() {
    const handlers: Record<string, any> = {};
    return {
      handlers,
      on(event: string, handler: any) {
        handlers[event] = handler;
      },
    };
  }

  it("before_provider_request：结构 metadata、带 scope 身份、绝不改 payload", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const pi = piMock();
    createModelCallObserverExtension()(pi);

    const payload = {
      stream: true,
      system: "TOPSECRET_SYSTEM",
      messages: [
        { role: "user", content: [{ type: "text", text: "TOPSECRET_USER" }] },
      ],
      tools: [{ name: "read" }],
    };
    const result = runWithModelCallScope({
      callId: "mc_hook",
      attemptId: "ma_hook",
      model: { provider: "anthropic", modelId: "claude", api: "anthropic-messages" },
    }, () => pi.handlers.before_provider_request({ type: "before_provider_request", payload }, {}));    expect(result).toBeUndefined();
    expect(payload.system).toBe("TOPSECRET_SYSTEM"); // payload 未被改动

    const prepared = observer.eventsOfType("provider_request_prepared")[0];
    expect(prepared).toMatchObject({ callId: "mc_hook", attemptId: "ma_hook" });
    expect(prepared.details).toMatchObject({
      messageCount: 1,
      toolCount: 1,
      hasSystemPrompt: true,
      hasMedia: false,
      streaming: true,
      // Output Budget Fact：payload 无 cap 字段 → absent（included 家族信息仍保留）
      outputBudget: {
        field: null,
        value: null,
        ownership: "absent",
        composition: "included",
      },
    });
    const serialized = JSON.stringify(prepared);
    expect(serialized).not.toContain("TOPSECRET_SYSTEM");
    expect(serialized).not.toContain("TOPSECRET_USER");
  });

  it("before_provider_request：cap 字段物化为 hana-chat-default（included 家族）", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const pi = piMock();
    createModelCallObserverExtension()(pi);

    const payload = {
      stream: true,
      system: "TOPSECRET_SYSTEM",
      messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
      max_tokens: 81920,
    };
    runWithModelCallScope({
      callId: "mc_hook_budget",
      attemptId: "ma_hook_budget",
      model: { provider: "anthropic", modelId: "claude", api: "anthropic-messages" },
      modelBudgetMeta: { maxTokens: 128000 },
    }, () => pi.handlers.before_provider_request({ type: "before_provider_request", payload }, {}));

    const prepared = observer.eventsOfType("provider_request_prepared")[0];
    expect(prepared.callId).toBe("mc_hook_budget");
    expect(prepared.details.outputBudget).toMatchObject({
      field: "max_tokens",
      value: 81920,
      composition: "included",
      ownership: "hana-chat-default",
      chatDefault: 81920,
      declaredMaxOutput: 128000,
    });
  });

  it("after_provider_response：httpStatus + providerRequestId（allowlist 头）", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const pi = piMock();
    createModelCallObserverExtension()(pi);

    runWithModelCallScope({
      callId: "mc_hook",
      attemptId: "ma_hook",
    }, () => pi.handlers.after_provider_response({
      type: "after_provider_response",
      status: 200,
      headers: {
        "x-request-id": "req-abc",
        authorization: "Bearer TOPSECRET", // 绝不记录
        cookie: "session=TOPSECRET",
      },
    }, {}));

    const received = observer.eventsOfType("provider_response_received")[0];
    expect(received).toMatchObject({
      callId: "mc_hook",
      attemptId: "ma_hook",
      providerRequestId: "req-abc",
      details: { httpStatus: 200 },
    });
    const serialized = JSON.stringify(received);
    expect(serialized).not.toContain("TOPSECRET");
    expect(serialized).not.toContain("authorization");
  });

  it("scope 缺失时 hook 直接跳过（不伪造关联）", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const pi = piMock();
    createModelCallObserverExtension()(pi);

    pi.handlers.before_provider_request({ payload: { messages: [] } }, {});
    pi.handlers.after_provider_response({ status: 200, headers: {} }, {});
    expect(observer.events).toHaveLength(0);
  });
});

describe("MC-02 cache-preserving AgentRun 集成", () => {
  const VALID_SUMMARY = [
    "## Goal",
    "- g",
    "## Constraints & Preferences",
    "- c",
    "## Progress",
    "### Done",
    "- d",
    "### In Progress",
    "- i",
    "### Blocked",
    "- b",
    "## Key Decisions",
    "- k",
    "## Next Steps",
    "- n",
    "## Critical Context",
    "- x",
  ].join("\n");

  function runnerFixture(streamFunction: any) {
    return {
      liveMessages: [
        { role: "user", content: [{ type: "text", text: "old" }], timestamp: 1 },
      ],
      instruction: {
        role: "user",
        content: [{ type: "text", text: "Summarize." }],
        timestamp: 2,
      },
      tools: [],
      model: {
        id: "test-model",
        provider: "test-provider",
        api: "openai-completions",
        contextWindow: 128_000,
        maxTokens: 8_192,
      },
      systemPrompt: "sys",
      convertToLlm: async (messages: any) => messages,
      streamFn: streamFunction,
      cacheMetadata: { cacheStrategy: "session_snapshot", strict: true },
      usageContext: {
        source: { subsystem: "compaction", operation: "compact", surface: "desktop", trigger: "threshold" },
        attribution: { kind: "session", sessionPath: "/tmp/sess-1.jsonl" },
      },
    } as any;
  }

  function createLedger() {
    const starts: any[] = [];
    return {
      starts,
      start(meta: any) {
        starts.push(meta);
        return { requestId: `request-${starts.length}` };
      },
      finish() {},
      recordError() {},
    };
  }

  it("单 turn 成功：compaction 分类 + callId↔ledger metadata 关联", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const ledger = createLedger();
    const session = fakeSession(async () => streamOf(assistantMessage({ content: [{ type: "text", text: VALID_SUMMARY }] })));
    installModelCallStreamObserver(session);

    const result = await runCachePreservingCompactionAgentRun({
      ...runnerFixture(session.agent.streamFunction),
      usageLedger: ledger,
    });
    await flushTerminal();

    expect(result.summary).toBe(VALID_SUMMARY);
    const starts = observer.eventsOfType("logical_call_start");
    expect(starts).toHaveLength(1);
    expect(starts[0].source).toMatchObject({ subsystem: "compaction", operation: "compact" });
    expect(starts[0].details).toMatchObject({ compactionPhase: "strict", providerRequestOrdinal: 1 });
    // callId↔ledger 关联：metadata.modelCallId 与 observer 事件同一身份
    expect(ledger.starts).toHaveLength(1);
    expect(ledger.starts[0].metadata.modelCallId).toBe(starts[0].callId);
    expect(ledger.starts[0].metadata.cacheStrategy).toBe("session_snapshot");
    expect(observer.eventsOfType("logical_call_end")[0].status).toBe("ok");
  });

  it("tool recovery 产生第二个 logical call（新 callId，同一 runner 不压缩身份）", async () => {
    const observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    const ledger = createLedger();
    const responses = [
      assistantMessage({
        stopReason: "toolUse",
        content: [{ type: "toolCall", id: "c1", name: "read", arguments: {} }],
      }),
      assistantMessage({ content: [{ type: "text", text: VALID_SUMMARY }] }),
    ];
    let call = 0;
    const session = fakeSession(async () => streamOf(responses[call++]));
    installModelCallStreamObserver(session);

    await runCachePreservingCompactionAgentRun({
      ...runnerFixture(session.agent.streamFunction),
      usageLedger: ledger,
    });
    await flushTerminal();

    // 两次真实模型调用 → 两个 logical call，两个 callId
    const starts = observer.eventsOfType("logical_call_start");
    expect(starts).toHaveLength(2);
    expect(starts[0].callId).not.toBe(starts[1].callId);
    expect(starts[1].details).toMatchObject({ compactionPhase: "tool_recovery", providerRequestOrdinal: 2 });
    expect(ledger.starts).toHaveLength(2);
    expect(ledger.starts[0].metadata.modelCallId).toBe(starts[0].callId);
    expect(ledger.starts[1].metadata.modelCallId).toBe(starts[1].callId);
    // 每个 call 各一个 attempt，互不混用
    expect(observer.attemptIds()).toHaveLength(2);
    expect(observer.eventsOfType("logical_call_end")).toHaveLength(2);
  });
});
