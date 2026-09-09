/**
 * MCP 连接器生命周期四档（eager/lazy/keep-alive/lazy-keep-alive）+ 空闲停泊。
 *
 * 这些测试用 fake timer + 注入的 fake client 验证 runtime 调度，不触碰真实
 * spawn/fetch。核心契约：
 *   - 归一化：缺省/非法 lifecycle → keep-alive（旧行为等价）；非法 idleTimeoutMinutes → null
 *   - load() 只自动启动 eager/keep-alive；lazy 档保持休眠（idle 状态，非 stopped）
 *   - lazy 档首次 callTool 按需启动；空闲超时后停泊（intent 仍 running、预期关闭、不触发重连）
 *   - 停泊后 eligibility 为 eligible，下一次 callTool 重新拉起
 *   - keep-alive 档永不空闲停泊；eager 档缺省不停泊，显式配置分钟数后停泊
 *   - 调用在途时停泊定时器不得触发；完成后重新武装
 *   - 用户停机（enabled=false）后 eligibility 仍拒绝（startableOnDemand=false 路径）
 *   - updateConnector 改档位即时生效（lazy→keep-alive 休眠中会被后台拉起）
 */
import os from "node:os";
import path from "node:path";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { McpManager, evaluateMcpToolEligibility } from "../core/mcp/manager.ts";

function makeFakeClientFactory() {
  const instances: any[] = [];
  const factory = (connector: any, opts: any) => {
    const client = {
      connector,
      opts,
      running: false,
      startCalls: 0,
      stopCalls: 0,
      start: vi.fn(async () => {
        client.startCalls += 1;
        client.running = true;
      }),
      stop: vi.fn(async () => {
        client.stopCalls += 1;
        client.running = false;
      }),
      listTools: vi.fn(async () => [{ name: "lookup", title: "Lookup" }]),
      callTool: vi.fn(async () => ({ content: [{ type: "text", text: "ok" }] })),
    };
    instances.push(client);
    return client;
  };
  factory.instances = instances;
  return factory as any;
}

function makeRuntime(stored: any, factory: any) {
  let current = stored;
  const runtime = new McpManager({
    dataDir: path.join(os.tmpdir(), `hana-mcp-lifecycle-${Math.random().toString(36).slice(2)}`),
    log: { info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() },
  }, {
    clientFactory: factory,
    configStore: {
      get: vi.fn(() => current),
      set: vi.fn((_key: string, value: any) => {
        current = { ...current, ...value };
      }),
    },
  });
  return runtime;
}

function connectorWith(overrides: Record<string, unknown>) {
  return {
    id: "local",
    name: "Local",
    command: "npx",
    args: ["-y", "mcp-server"],
    tools: [{ name: "lookup", title: "Lookup" }],
    ...overrides,
  };
}

// The per-agent exposure gate requires the agent config to explicitly enable
// the connector (null fails closed), so eligibility tests pass this shape.
const permissiveAgentConfig = {
  mcp: { connectors: { local: { enabled: true, tools: { lookup: true } } } },
};

describe("MCP connector lifecycle normalization", () => {
  it("defaults a legacy connector (no lifecycle field) to keep-alive", () => {
    const runtime = makeRuntime({ enabled: true, connectors: [connectorWith({})] }, makeFakeClientFactory());
    const view = runtime.getState().connectors[0];
    expect(view.lifecycle).toBe("keep-alive");
    expect(view.idleTimeoutMinutes).toBeNull();
  });

  it("drops an unrecognized lifecycle back to keep-alive and garbage timeouts to null", () => {
    const runtime = makeRuntime({
      enabled: true,
      connectors: [connectorWith({ lifecycle: "turbo", idleTimeoutMinutes: "whenever" })],
    }, makeFakeClientFactory());
    const view = runtime.getState().connectors[0];
    expect(view.lifecycle).toBe("keep-alive");
    expect(view.idleTimeoutMinutes).toBeNull();
  });

  it("keeps an explicit 0-minute timeout (never park) instead of collapsing it to the default", () => {
    const runtime = makeRuntime({
      enabled: true,
      connectors: [connectorWith({ lifecycle: "lazy", idleTimeoutMinutes: 0 })],
    }, makeFakeClientFactory());
    expect(runtime.getState().connectors[0].idleTimeoutMinutes).toBe(0);
  });
});

describe("MCP lifecycle auto-start at load", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("starts eager and keep-alive connectors but leaves lazy ones dormant", async () => {
    const factory = makeFakeClientFactory();
    const runtime = makeRuntime({
      enabled: true,
      connectors: [
        connectorWith({ id: "eager", lifecycle: "eager" }),
        connectorWith({ id: "keep", lifecycle: "keep-alive" }),
        connectorWith({ id: "lazy", lifecycle: "lazy", idleTimeoutMinutes: 5 }),
        connectorWith({ id: "lazykeep", lifecycle: "lazy-keep-alive" }),
      ],
    }, factory);
    await runtime.load();
    await vi.advanceTimersByTimeAsync(0);

    expect(factory.instances.filter((c) => c.connector.id === "eager")[0]?.running).toBe(true);
    expect(factory.instances.filter((c) => c.connector.id === "keep")[0]?.running).toBe(true);
    expect(factory.instances.filter((c) => c.connector.id === "lazy")).toHaveLength(0);
    expect(factory.instances.filter((c) => c.connector.id === "lazykeep")).toHaveLength(0);
    // A never-started lazy connector is dormant by design, not stopped.
    const statuses = Object.fromEntries(runtime.getState().connectors.map((c: any) => [c.id, c.status]));
    expect(statuses.lazy).toBe("idle");
    expect(statuses.lazykeep).toBe("idle");
    expect(statuses.eager).toBe("running");
  });

  it("skips auto-start after add for a lazy connector", async () => {
    const factory = makeFakeClientFactory();
    const runtime = makeRuntime({ enabled: true, connectors: [] }, factory);
    runtime.addConnector({ id: "added", command: "npx", lifecycle: "lazy", idleTimeoutMinutes: 5 });
    await runtime.autoStartAfterAdd("added");
    await vi.advanceTimersByTimeAsync(0);
    expect(factory.instances).toHaveLength(0);
  });
});

describe("MCP lazy lifecycle: on-demand start and idle parking", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("starts on the first call, parks after the idle timeout, and restarts on the next call", async () => {
    const factory = makeFakeClientFactory();
    const runtime = makeRuntime({
      enabled: true,
      connectors: [connectorWith({ lifecycle: "lazy", idleTimeoutMinutes: 10 })],
    }, factory);
    await runtime.load();

    await runtime.callTool("local", "lookup", {});
    expect(factory.instances).toHaveLength(1);
    expect(factory.instances[0].running).toBe(true);
    expect(runtime.connectorStatusFor("local")).toBe("running");

    // Ten idle minutes later the connector is parked by design: the client was
    // stopped, intent stays running, and the status reads idle.
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(factory.instances[0].stopCalls).toBeGreaterThanOrEqual(1);
    expect(runtime.connectorStatusFor("local")).toBe("idle");
    expect(runtime.desiredStates.get("local")).toBe("running");
    // The expected close must not reach the reconnect machinery.
    expect(runtime.reconnectState.has("local")).toBe(false);

    // Eligibility treats dormancy as callable; the next call restarts it.
    const eligibility = runtime.evaluateToolEligibility("local", "lookup", permissiveAgentConfig);
    expect(eligibility.eligible).toBe(true);
    await runtime.callTool("local", "lookup", {});
    expect(factory.instances.filter((c) => c.running)).toHaveLength(1);
    expect(runtime.connectorStatusFor("local")).toBe("running");
  });

  it("parks an eager connector only when an explicit idle timeout is set", async () => {
    const factory = makeFakeClientFactory();
    const runtime = makeRuntime({
      enabled: true,
      connectors: [connectorWith({ lifecycle: "eager", idleTimeoutMinutes: 1 })],
    }, factory);
    await runtime.load();
    await vi.advanceTimersByTimeAsync(0);
    expect(factory.instances[0].running).toBe(true);

    await vi.advanceTimersByTimeAsync(60_000);
    expect(runtime.connectorStatusFor("local")).toBe("idle");

    // Default eager (no timeout) never parks.
    const factory2 = makeFakeClientFactory();
    const runtime2 = makeRuntime({ enabled: true, connectors: [connectorWith({ lifecycle: "eager" })] }, factory2);
    await runtime2.load();
    await vi.advanceTimersByTimeAsync(60 * 60_000);
    expect(runtime2.connectorStatusFor("local")).toBe("running");
  });

  it("never parks keep-alive or lazy-keep-alive connectors", async () => {
    for (const lifecycle of ["keep-alive", "lazy-keep-alive"]) {
      const factory = makeFakeClientFactory();
      const runtime = makeRuntime({
        enabled: true,
        connectors: [connectorWith({ lifecycle })],
      }, factory);
      await runtime.load();
      await vi.advanceTimersByTimeAsync(0);
      await runtime.callTool("local", "lookup", {});
      await vi.advanceTimersByTimeAsync(24 * 60 * 60_000);
      expect(runtime.connectorStatusFor("local")).toBe("running");
    }
  });

  it("does not park while a call is in flight, and parks only after it completes", async () => {
    const factory = makeFakeClientFactory();
    const runtime = makeRuntime({
      enabled: true,
      connectors: [connectorWith({ lifecycle: "lazy", idleTimeoutMinutes: 5 })],
    }, factory);
    await runtime.load();

    // First call brings the connector up and completes normally.
    await runtime.callTool("local", "lookup", {});
    const client = factory.instances[0];

    // Second call hangs server-side: the park timer must not fire under it.
    let releaseCall: (value?: unknown) => void = () => {};
    client.callTool = vi.fn(() => new Promise((resolve) => { releaseCall = resolve; }));
    const pending = runtime.callTool("local", "lookup", {});
    await vi.advanceTimersByTimeAsync(60 * 60_000);
    expect(client.stopCalls).toBe(0);
    expect(runtime.connectorStatusFor("local")).toBe("running");

    releaseCall({ content: [{ type: "text", text: "ok" }] });
    await pending;
    // Completion refreshed the idle clock: the timer re-armed for a fresh
    // window instead of parking immediately.
    expect(runtime.connectorStatusFor("local")).toBe("running");
    await vi.advanceTimersByTimeAsync(5 * 60_000);
    expect(runtime.connectorStatusFor("local")).toBe("idle");
  });

  it("clears the park timer when the user stops the connector", async () => {
    const factory = makeFakeClientFactory();
    const runtime = makeRuntime({
      enabled: true,
      connectors: [connectorWith({ lifecycle: "lazy", idleTimeoutMinutes: 5 })],
    }, factory);
    await runtime.load();
    await runtime.callTool("local", "lookup", {});
    await runtime.stopConnector("local");
    await vi.advanceTimersByTimeAsync(60 * 60_000);
    // No park machinery firing after a user stop; the connector reads stopped.
    expect(runtime.connectorStatusFor("local")).toBe("stopped");
    expect(factory.instances[0].stopCalls).toBe(1);
  });
});

describe("MCP lifecycle eligibility boundaries", () => {
  it("keeps a user-stopped connector ineligible while a dormant one is eligible", () => {
    const base = {
      globalEnabled: true,
      connectorId: "local",
      toolName: "lookup",
      connectorPresent: true,
      connectorEnabled: true,
      toolPresent: true,
    };
    const agent = permissiveAgentConfig;
    // Dormant (idle override, startable) is callable.
    expect(evaluateMcpToolEligibility(agent, {
      ...base,
      status: "idle",
      transportAvailable: false,
      startableOnDemand: true,
    })).toMatchObject({ eligible: true });
    // A never-started lazy connector (plain stopped + on-demand intent) too.
    expect(evaluateMcpToolEligibility(agent, {
      ...base,
      status: "stopped",
      transportAvailable: false,
      startableOnDemand: true,
    })).toMatchObject({ eligible: true });
    // The pure function defaults to the old fail-closed behaviour: callers that
    // cannot see the intent map must not silently start connectors.
    expect(evaluateMcpToolEligibility(agent, {
      ...base,
      status: "stopped",
      transportAvailable: false,
    })).toMatchObject({ eligible: false, reason: "mcp_connector_stopped" });
    // Idle without an on-demand path is just as dead.
    expect(evaluateMcpToolEligibility(agent, {
      ...base,
      status: "idle",
      transportAvailable: false,
    })).toMatchObject({ eligible: false, reason: "mcp_connector_stopped" });
  });

  it("runtime eligibility refuses a connector the user switched off", async () => {
    const factory = makeFakeClientFactory();
    const runtime = makeRuntime({
      enabled: true,
      connectors: [connectorWith({ lifecycle: "lazy", enabled: false })],
    }, factory);
    await runtime.load();
    const result = runtime.evaluateToolEligibility("local", "lookup", permissiveAgentConfig);
    expect(result).toMatchObject({ eligible: false, reason: "mcp_connector_disabled" });
  });
});

describe("MCP lifecycle edits apply without a restart", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("brings a parked connector up when its lifecycle switches to keep-alive", async () => {
    const factory = makeFakeClientFactory();
    const runtime = makeRuntime({
      enabled: true,
      connectors: [connectorWith({ lifecycle: "lazy", idleTimeoutMinutes: 5 })],
    }, factory);
    await runtime.load();
    await runtime.callTool("local", "lookup", {});
    await vi.advanceTimersByTimeAsync(5 * 60_000);
    expect(runtime.connectorStatusFor("local")).toBe("idle");

    await runtime.updateConnector("local", { lifecycle: "keep-alive" });
    await vi.advanceTimersByTimeAsync(0);
    expect(runtime.connectorStatusFor("local")).toBe("running");
    expect(factory.instances.filter((c) => c.running)).toHaveLength(1);
  });

  it("changing connection fields (not lifecycle) does not rearm a park on a dormant connector", async () => {
    const factory = makeFakeClientFactory();
    const runtime = makeRuntime({
      enabled: true,
      connectors: [connectorWith({ lifecycle: "lazy", idleTimeoutMinutes: 5 })],
    }, factory);
    await runtime.load();
    await runtime.callTool("local", "lookup", {});
    await vi.advanceTimersByTimeAsync(5 * 60_000);
    expect(runtime.connectorStatusFor("local")).toBe("idle");

    // A name edit leaves the parked state exactly as it was.
    await runtime.updateConnector("local", { name: "Renamed" });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(runtime.connectorStatusFor("local")).toBe("idle");
  });
});
