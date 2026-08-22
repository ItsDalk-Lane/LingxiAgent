/**
 * Control Plane 回归锁（任务书 §四十六～§四十八/§六十四）。
 *
 * 以下动作执行后 ModelCallObserver 必须 0 events、不增加模型用量 ledger：
 *   - media poll/query（adapter.query 只查已提交任务的状态）
 *   - external CLI credential authorization（provider:authorize-external-credential-use）
 *
 * （GET /models 探测的 0-event 锁在 model-call-probe-observer.test.ts。）
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Poller } from "../core/media/poller.ts";
import { Hub } from "../hub/index.ts";
import { setModelCallObserver } from "../lib/llm/model-call-observer.ts";
import { createTestModelCallObserver } from "../lib/llm/model-call-observer-testing.ts";

describe("control plane × ModelCallObserver（0-event 锁）", () => {
  let observer: ReturnType<typeof createTestModelCallObserver>;

  beforeEach(() => {
    observer = createTestModelCallObserver();
    setModelCallObserver(observer);
    vi.useFakeTimers();
  });
  afterEach(() => {
    setModelCallObserver(null);
    vi.useRealTimers();
  });

  it("media poll（adapter.query）：0 ModelCall 事件 + 0 ledger 请求", async () => {
    const usageLedger = {
      start: vi.fn(() => ({ requestId: "should-never-start" })),
      finish: vi.fn(),
      recordError: vi.fn(),
    };
    const mockAdapter = {
      id: "test-adapter",
      query: vi.fn(async () => ({ status: "pending" })),
    };
    const mockStore = {
      listPending: vi.fn(() => []),
      get: vi.fn(() => ({
        taskId: "task1",
        adapterId: "test-adapter",
        status: "pending",
        files: [],
        createdAt: new Date().toISOString(),
      })),
      update: vi.fn(),
    };
    const poller = new Poller({
      store: mockStore,
      registry: { get: vi.fn(() => mockAdapter) },
      bus: { request: vi.fn(async () => {}) },
      dataDir: "/tmp/media-data",
      generatedDir: "/tmp/media-generated",
      log: { info: vi.fn(), warn: vi.fn(), error: vi.fn() },
      registerSessionFile: vi.fn(),
      usageLedger,
    });

    poller.start();
    poller.add("task1");
    await vi.advanceTimersByTimeAsync(5_000);
    poller.stop();

    // poll 发生了（业务事实成立）
    expect(mockAdapter.query).toHaveBeenCalled();
    // 但它不是 Model Call：0 事件、0 ledger
    expect(observer.events).toHaveLength(0);
    expect(usageLedger.start).not.toHaveBeenCalled();
    expect(usageLedger.finish).not.toHaveBeenCalled();
    expect(usageLedger.recordError).not.toHaveBeenCalled();
  });

  it("external CLI credential authorization：0 ModelCall 事件 + 0 ledger 请求", async () => {
    const usageLedger = {
      start: vi.fn(),
      finish: vi.fn(),
      recordError: vi.fn(),
    };
    const providerRegistry: any = {
      get: vi.fn(() => ({ source: { kind: "plugin", pluginId: "jimeng-cli" } })),
      authorizeExternalCredentialUse: vi.fn(() => ({
        providerId: "jimeng-cli",
        boundaryId: "dreamina-cli-login",
        operation: "submit",
        credentialSource: "external",
      })),
      registerRuntimeMediaCapabilitySource: vi.fn(),
      unregisterRuntimeMediaCapabilitySource: vi.fn(),
    };
    const engine: any = {
      agentsDir: "/agents",
      channelsDir: null,
      lingxiHome: "/tmp/hana",
      providerRegistry,
      usageLedger,
      setHubCallbacks: vi.fn(),
      setEventBus: vi.fn(),
      getAgent: vi.fn(() => null),
      updateConfig: vi.fn(async () => {}),
      listAgents: vi.fn(() => []),
      listSessions: vi.fn(async () => []),
      isSessionStreaming: vi.fn(() => false),
      promptSession: vi.fn(async () => {}),
      abortSession: vi.fn(async () => true),
      dispose: vi.fn(async () => {}),
      prompt: vi.fn(async () => {}),
      executeExternalMessage: vi.fn(async () => {}),
      executeIsolated: vi.fn(async () => {}),
    };
    const hub = new Hub({ engine });

    const result = await hub.eventBus.request(
      "provider:authorize-external-credential-use",
      {
        providerId: "jimeng-cli",
        boundaryId: "dreamina-cli-login",
        operation: "submit",
      },
      { caller: { kind: "plugin", pluginId: "jimeng-cli" } },
    );

    // 许可照常签发（安全控制面行为不变）
    expect(result).toMatchObject({
      ok: true,
      permit: { providerId: "jimeng-cli", credentialSource: "external" },
    });
    // 但它不是 Model Call
    expect(observer.events).toHaveLength(0);
    expect(usageLedger.start).not.toHaveBeenCalled();
  });
});
