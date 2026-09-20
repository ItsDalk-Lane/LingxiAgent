import { describe, expect, it, vi } from "vitest";

import { SettingsManager } from "../lib/pi-sdk/index.ts";
import {
  MIDRUN_COMPACTION_NOTICE,
  computeCompactionReserveTokens,
  installDynamicCompactionReserve,
  installMidRunCompaction,
} from "../core/session-compaction-runtime.ts";

const FLOOR_RESERVE = 16_384;

function createSettingsManager() {
  return SettingsManager.inMemory({
    steeringMode: "all",
    compaction: {
      enabled: true,
      reserveTokens: FLOOR_RESERVE,
      keepRecentTokens: 20_000,
    },
  });
}

function createFakeSession({
  contextWindow = 200_000,
  branch = [],
  messages = [{ role: "user", content: "old" }],
  rebuiltMessages = [{ role: "custom", content: "rebuilt" }],
}: {
  contextWindow?: number;
  branch?: any[];
  messages?: any[];
  rebuiltMessages?: any[];
} = {}) {
  const settingsManager = createSettingsManager();
  const session: any = {
    model: { contextWindow },
    settingsManager,
    isCompacting: false,
    _emit: vi.fn(),
    agent: {
      state: { messages: [...messages] },
    },
    sessionManager: {
      getBranch: vi.fn(() => branch),
      getSessionFile: vi.fn(() => "/tmp/fake-session.jsonl"),
      appendCustomMessageEntry: vi.fn(() => "entry-1"),
      buildSessionContext: vi.fn(() => ({ messages: rebuiltMessages })),
    },
  };
  installDynamicCompactionReserve(session);
  return { session, settingsManager, rebuiltMessages };
}

function createAssistantTurn(totalTokens: number, overrides: Record<string, any> = {}) {
  return {
    message: {
      role: "assistant",
      stopReason: "stop",
      timestamp: Date.now(),
      usage: { totalTokens },
      ...overrides,
    },
    toolResults: [],
    context: { systemPrompt: "sys", tools: [], messages: [{ role: "user", content: "old" }] },
    newMessages: [],
  };
}

describe("computeCompactionReserveTokens", () => {
  it("scales the reserve with the context window but never below the floor", () => {
    // reserve = max(16384, 20% of window)，触发点即 80% 强制线
    expect(computeCompactionReserveTokens(200_000)).toBe(40_000);
    expect(computeCompactionReserveTokens(100_000)).toBe(20_000);
    expect(computeCompactionReserveTokens(1_048_576)).toBe(209_716);
  });

  it("falls back to the floor for missing or nonsensical windows", () => {
    expect(computeCompactionReserveTokens(0)).toBe(16_384);
    expect(computeCompactionReserveTokens(undefined)).toBe(16_384);
    expect(computeCompactionReserveTokens(Number.NaN)).toBe(16_384);
    expect(computeCompactionReserveTokens(-5)).toBe(16_384);
    expect(computeCompactionReserveTokens(Number.POSITIVE_INFINITY)).toBe(16_384);
  });

  it("places the trigger point at the smaller of 80% of the window and window minus the floor", () => {
    for (const window of [200_000, 1_048_576]) {
      const triggerPoint = window - computeCompactionReserveTokens(window);
      expect(triggerPoint).toBe(Math.min(Math.floor(0.8 * window), window - FLOOR_RESERVE));
    }
  });
});

describe("installDynamicCompactionReserve", () => {
  it("derives reserveTokens from the live model window through the real settings manager", () => {
    const settingsManager = createSettingsManager();
    const session: any = { model: { contextWindow: 1_000_000 }, settingsManager };

    installDynamicCompactionReserve(session);

    expect(settingsManager.getCompactionSettings().reserveTokens).toBe(200_000);

    session.model = { contextWindow: 120_000 };
    expect(settingsManager.getCompactionSettings().reserveTokens).toBe(24_000);
  });

  it("is idempotent", () => {
    const settingsManager = createSettingsManager();
    const session: any = { model: { contextWindow: 1_000_000 }, settingsManager };

    installDynamicCompactionReserve(session);
    const first = settingsManager.getCompactionReserveTokens;
    installDynamicCompactionReserve(session);

    expect(settingsManager.getCompactionReserveTokens).toBe(first);
    expect(settingsManager.getCompactionSettings().reserveTokens).toBe(200_000);
  });

  it("throws when the SDK no longer exposes the reserve accessor", () => {
    const settingsManager: any = { getCompactionSettings: () => ({}) };
    expect(() => installDynamicCompactionReserve({ settingsManager })).toThrow(/getCompactionReserveTokens/);
  });
});

describe("installMidRunCompaction", () => {
  it("compacts between turns and appends a task-continuation notice", async () => {
    const { session, rebuiltMessages } = createFakeSession({ contextWindow: 200_000 });
    const runCompaction = vi.fn(async (_session: any, _options: any) => ({}));

    installMidRunCompaction(session, { runCompaction });

    const signal = new AbortController().signal;
    const snapshot = await session.agent.prepareNextTurnWithContext(createAssistantTurn(190_000), signal);

    expect(runCompaction).toHaveBeenCalledTimes(1);
    expect(runCompaction.mock.calls[0][1]).toMatchObject({ lifecycleReason: "threshold", emitLifecycle: true });

    expect(session.sessionManager.appendCustomMessageEntry).toHaveBeenCalledTimes(1);
    const [customType, content, display] = session.sessionManager.appendCustomMessageEntry.mock.calls[0];
    expect(customType).toBe("midrun-compaction-notice");
    expect(content).toBe(MIDRUN_COMPACTION_NOTICE);
    expect(content).toContain("not a user message");
    expect(display).toBe(false);

    expect(session.agent.state.messages).toEqual(rebuiltMessages);
    expect(snapshot.context.messages).toEqual(rebuiltMessages);
    expect(snapshot.context.messages).not.toBe(session.agent.state.messages);
  });

  it("leaves the previous snapshot untouched below the threshold", async () => {
    const { session } = createFakeSession({ contextWindow: 200_000 });
    const previousSnapshot = { context: { systemPrompt: "sys", tools: [], messages: [] } };
    session.agent.prepareNextTurnWithContext = async () => previousSnapshot;
    const runCompaction = vi.fn(async (_session: any, _options: any) => ({}));

    installMidRunCompaction(session, { runCompaction });

    const snapshot = await session.agent.prepareNextTurnWithContext(createAssistantTurn(100_000), undefined);

    expect(runCompaction).not.toHaveBeenCalled();
    expect(snapshot).toBe(previousSnapshot);
    expect(session.sessionManager.appendCustomMessageEntry).not.toHaveBeenCalled();
  });

  it("never breaks the run when compaction fails", async () => {
    const { session } = createFakeSession({ contextWindow: 200_000 });
    const previousSnapshot = { context: { systemPrompt: "sys", tools: [], messages: [] } };
    session.agent.prepareNextTurnWithContext = async () => previousSnapshot;
    const runCompaction = vi.fn(async (_session: any, _options: any) => { throw new Error("compaction exploded"); });

    installMidRunCompaction(session, { runCompaction });

    const snapshot = await session.agent.prepareNextTurnWithContext(createAssistantTurn(190_000), undefined);

    expect(runCompaction).toHaveBeenCalledTimes(1);
    expect(snapshot).toBe(previousSnapshot);
    expect(session.sessionManager.appendCustomMessageEntry).not.toHaveBeenCalled();
  });

  it("stays out of the way while another compaction is running", async () => {
    const { session } = createFakeSession({ contextWindow: 200_000 });
    session.isCompacting = true;
    const runCompaction = vi.fn(async (_session: any, _options: any) => ({}));

    installMidRunCompaction(session, { runCompaction });
    await session.agent.prepareNextTurnWithContext(createAssistantTurn(190_000), undefined);

    expect(runCompaction).not.toHaveBeenCalled();
  });

  it("skips assistant usage that predates the latest compaction", async () => {
    const compactionAt = new Date();
    const { session } = createFakeSession({
      contextWindow: 200_000,
      branch: [{ type: "compaction", timestamp: compactionAt.toISOString() }],
    });
    const runCompaction = vi.fn(async (_session: any, _options: any) => ({}));

    installMidRunCompaction(session, { runCompaction });
    await session.agent.prepareNextTurnWithContext(
      createAssistantTurn(190_000, { timestamp: compactionAt.getTime() - 1 }),
      undefined,
    );

    expect(runCompaction).not.toHaveBeenCalled();
  });

  it("preserves the model and thinking level carried by the wrapped hook", async () => {
    const { session, rebuiltMessages } = createFakeSession({ contextWindow: 200_000 });
    const model = { id: "m-1", contextWindow: 200_000 };
    session.agent.prepareNextTurnWithContext = async () => ({
      model,
      thinkingLevel: "low",
      context: { systemPrompt: "sys", tools: ["t"], messages: [] },
    });
    const runCompaction = vi.fn(async (_session: any, _options: any) => ({}));

    installMidRunCompaction(session, { runCompaction });

    const snapshot = await session.agent.prepareNextTurnWithContext(createAssistantTurn(190_000), undefined);

    expect(runCompaction).toHaveBeenCalledTimes(1);
    expect(snapshot.model).toBe(model);
    expect(snapshot.thinkingLevel).toBe("low");
    expect(snapshot.context.systemPrompt).toBe("sys");
    expect(snapshot.context.tools).toEqual(["t"]);
    expect(snapshot.context.messages).toEqual(rebuiltMessages);
  });

  it("is idempotent", async () => {
    const { session } = createFakeSession({ contextWindow: 200_000 });
    const runCompaction = vi.fn(async (_session: any, _options: any) => ({}));

    installMidRunCompaction(session, { runCompaction });
    const first = session.agent.prepareNextTurnWithContext;
    installMidRunCompaction(session, { runCompaction });

    expect(session.agent.prepareNextTurnWithContext).toBe(first);

    await session.agent.prepareNextTurnWithContext(createAssistantTurn(190_000), undefined);
    expect(runCompaction).toHaveBeenCalledTimes(1);
  });

  it("emits compaction_suggested in the ask band without compacting", async () => {
    const { session } = createFakeSession({ contextWindow: 200_000 });
    const runCompaction = vi.fn(async (_session: any, _options: any) => ({}));

    installMidRunCompaction(session, { runCompaction });
    await session.agent.prepareNextTurnWithContext(createAssistantTurn(120_000), undefined);

    expect(runCompaction).not.toHaveBeenCalled();
    const suggestions = session._emit.mock.calls
      .map(([event]) => event)
      .filter((event: any) => event.type === "compaction_suggested");
    expect(suggestions).toHaveLength(1);
    expect(suggestions[0]).toMatchObject({
      sessionPath: "/tmp/fake-session.jsonl",
      percent: 60,
      askPercent: 50,
      forcePercent: 80,
    });
  });

  it("forces compaction at the force line and emits compaction_auto", async () => {
    const { session } = createFakeSession({ contextWindow: 200_000 });
    const runCompaction = vi.fn(async (_session: any, _options: any) => ({}));

    installMidRunCompaction(session, { runCompaction });
    await session.agent.prepareNextTurnWithContext(createAssistantTurn(170_000), undefined);

    expect(runCompaction).toHaveBeenCalledTimes(1);
    const autos = session._emit.mock.calls
      .map(([event]) => event)
      .filter((event: any) => event.type === "compaction_auto");
    expect(autos).toHaveLength(1);
    expect(autos[0]).toMatchObject({ percentBefore: 85, forcePercent: 80 });
    expect(session._emit.mock.calls.some(([event]: any[]) => event.type === "compaction_suggested")).toBe(false);
  });

  it("throttles repeated suggestions until usage grows by another 5% of the window", async () => {
    const { session } = createFakeSession({ contextWindow: 200_000 });
    const runCompaction = vi.fn(async (_session: any, _options: any) => ({}));

    installMidRunCompaction(session, { runCompaction });

    // 60%：第一次询问
    await session.agent.prepareNextTurnWithContext(createAssistantTurn(120_000), undefined);
    // 62.5%（+2.5% 窗口 < 5%）：静默
    await session.agent.prepareNextTurnWithContext(createAssistantTurn(125_000), undefined);
    // 72.5%（+12.5% 窗口 >= 5%）：再次询问
    await session.agent.prepareNextTurnWithContext(createAssistantTurn(145_000), undefined);

    expect(runCompaction).not.toHaveBeenCalled();
    const suggestions = session._emit.mock.calls
      .map(([event]) => event)
      .filter((event: any) => event.type === "compaction_suggested");
    expect(suggestions).toHaveLength(2);
    expect(suggestions.map((event: any) => event.percent)).toEqual([60, 73]);
  });

  it("resets the ask state after a compaction-sized usage drop", async () => {
    const { session } = createFakeSession({ contextWindow: 200_000 });
    const runCompaction = vi.fn(async (_session: any, _options: any) => ({}));

    installMidRunCompaction(session, { runCompaction });

    // 60%：第一次询问
    await session.agent.prepareNextTurnWithContext(createAssistantTurn(120_000), undefined);
    // 用户手动压缩：用量骤降 70%（>= 20% 窗口）→ 状态重置
    await session.agent.prepareNextTurnWithContext(createAssistantTurn(60_000), undefined);
    // 回涨到 61%：重新询问
    await session.agent.prepareNextTurnWithContext(createAssistantTurn(122_000), undefined);

    const suggestions = session._emit.mock.calls
      .map(([event]) => event)
      .filter((event: any) => event.type === "compaction_suggested");
    expect(suggestions).toHaveLength(2);
    expect(suggestions.map((event: any) => event.percent)).toEqual([60, 61]);
  });
});
