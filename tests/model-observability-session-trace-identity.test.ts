/**
 * 会话级轨迹复用的「双身份」生产接线回归（2026-09-10）。
 *
 * 背景：Lingxi 业务会话 ID（session manifest，`sess_...`）与 Pi SDK 会话文件头
 * UUID（uuidv7，`SessionManager.getSessionId()`）是两套独立身份（c9494ae9 加固）。
 * 2026-09-05 的会话级轨迹复用（df2d91a8）把复用查找键错接到 SDK UUID 上，而
 * attribution/内存索引/SQL 全部按业务会话 ID 写入——真实桌面上两个值从不相等，
 * 复用查找结构性零命中，同一桌面会话每轮重新铸根。旧的复用测试用同一个字符串
 * 同时扮演两种身份，制造了假阳性。
 *
 * 本文件用真实生产组合锁死不变量：
 *
 *   desktop prompt → SessionCoordinator（真实）→ session manifest（真实业务 ID）
 *   → installModelCallStreamObserver（真实接点）→ persistence（真实安装）
 *   → query service（真实读取）
 *
 * 只 mock pi-sdk 包入口（createAgentSession/SessionManager），fake session 的
 * SDK UUID 与 manifest 业务 ID 故意不同。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";

const {
  createAgentSessionMock,
  sessionManagerCreateMock,
  sessionManagerOpenMock,
} = vi.hoisted(() => ({
  createAgentSessionMock: vi.fn(),
  sessionManagerCreateMock: vi.fn(),
  sessionManagerOpenMock: vi.fn(),
}));

vi.mock("../lib/pi-sdk/index.js", () => ({
  createAgentSession: createAgentSessionMock,
  createExtensionRuntime: vi.fn(),
  SessionManager: {
    create: sessionManagerCreateMock,
    open: sessionManagerOpenMock,
    list: vi.fn(async () => []),
  },
  SettingsManager: { inMemory: vi.fn(() => ({})) },
  estimateTokens: vi.fn(() => 0),
  emitSessionShutdown: vi.fn(async () => false),
  refreshSessionModelFromRegistry: vi.fn((session: any, model: any) => {
    session.model = model;
    return true;
  }),
  resizeModelImageInput: vi.fn(async (image: unknown) => image),
  formatModelImageDimensionNote: vi.fn(() => undefined),
}));

vi.mock("../lib/debug-log.js", () => ({
  createModuleLogger: () => ({
    log: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  }),
}));

import { SessionCoordinator } from "../core/session-coordinator.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import {
  installModelCallStreamObserver,
  installModelCallTraceIngress,
} from "../lib/pi-sdk/model-call-stream-observer.ts";
import { createModelObservabilityTestHarness } from "../lib/llm/model-observability-testing.ts";
import {
  installModelObservabilityPersistence,
  type ModelObservabilityPersistenceHandle,
} from "../lib/llm/model-observability-persistence.ts";
import { createModelObservabilityQueryService } from "../lib/llm/model-observability-query.ts";
import { normalizeModelObservabilityTraceQuery } from "../lib/llm/model-observability-query-types.ts";

const MODEL = { id: "test-model", provider: "test", name: "Test Model" };

/** uuidv7 形态的 SDK 文件头身份——与 sess_ 业务身份绝不相同。 */
const SDK_UUID_FOCUS = "0192b7c4-3f2a-7e1d-9c4b-2a8f6d5e4c3b";
const SDK_UUID_RESTORED = "0192b7c4-ffff-7e1d-9c4b-2a8f6d5e4c3b";

function assistantStream(fail = false) {
  const stream = createAssistantMessageEventStream();
  queueMicrotask(() => {
    const message = {
      role: "assistant",
      content: [{ type: "text", text: "ok" }],
      api: "openai-completions",
      provider: "test",
      model: "test-model",
      usage: { input: 10, output: 5, cacheRead: 0, cacheWrite: 0, totalTokens: 15 },
      stopReason: fail ? "error" : "stop",
      errorMessage: fail ? "provider exploded" : undefined,
      timestamp: Date.now(),
    };
    if (fail) stream.push({ type: "error", reason: "error", error: message } as any);
    else stream.push({ type: "done", reason: "stop", message } as any);
    stream.end();
  });
  return stream;
}

/** fake SDK SessionManager：SDK 文件头 UUID 与业务会话 ID 故意分离。 */
function fakeSdkSessionManager({ sessionPath, sdkSessionId, cwd }: {
  sessionPath: string;
  sdkSessionId: string;
  cwd: string;
}) {
  return {
    getSessionFile: () => sessionPath,
    getSessionId: () => sdkSessionId,
    getCwd: () => cwd,
    getEntries: () => [],
    getBranch: () => [],
    getLeafId: () => null,
    getEntry: () => null,
    resetLeaf: vi.fn(),
    branch: vi.fn(),
    fileEntries: [],
    buildSessionContext: () => ({ model: null }),
    // 测试钩子：置 true 时下一轮 provider 流以 error 终态返回。
    __failNext: false,
  };
}

/**
 * fake AgentSession：prompt() 内部经真实 installModelCallStreamObserver 包装的
 * streamFunction 走一轮模型调用——与生产 lib/pi-sdk/index.ts createAgentSession
 * 安装的接点完全一致（observer + trace ingress 都是真实模块）。
 */
function buildFakeAgentSession({ manager, model }: { manager: any; model: any }) {
  const session: any = {
    sessionManager: manager,
    model,
    isStreaming: false,
    isCompacting: false,
    subscribe: vi.fn(() => vi.fn()),
    setActiveToolsByName: vi.fn(),
    agent: {
      // 测试可按 manager.__failNext = true 让下一轮 provider 失败。
      streamFunction: async () => {
        const fail = manager.__failNext === true;
        manager.__failNext = false;
        return assistantStream(fail);
      },
      state: { tools: [] },
    },
  };
  session.prompt = async function prompt() {
    const stream = await session.agent.streamFunction(
      session.model,
      { systemPrompt: "BASE", messages: [] },
      {},
    );
    await stream.result();
  };
  installModelCallStreamObserver(session);
  installModelCallTraceIngress(session);
  return session;
}

/** 等 observer 的 result().then 终态观察与队列入队跑完。 */
async function settleObserver() {
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
}

describe("模型观测会话轨迹：业务会话 ID ≠ SDK UUID 的真实生产接线", () => {
  let tempDir: string;
  let agentsDir: string;
  let sessionDir: string;
  let lingxiHome: string;
  let store: SessionManifestStore | null;
  let harness: ReturnType<typeof createModelObservabilityTestHarness>;
  let currentHandle: ModelObservabilityPersistenceHandle;
  let service: ReturnType<typeof createModelObservabilityQueryService>;
  let reuseLookupKeys: Array<string | null>;
  let manifestIdCounter: number;
  let sdkFileCounter: number;

  const agent = {
    id: "hana",
    agentName: "Hana",
    name: "Hana",
    get agentDir() { return path.join(agentsDir, "hana"); },
    get sessionDir() { return sessionDir; },
    memoryMasterEnabled: true,
    sessionMemoryEnabled: true,
    config: {},
    tools: [],
    buildSystemPrompt: vi.fn(() => "system"),
  };

  function buildCoordinator() {
    return new SessionCoordinator({
      agentsDir,
      getAgent: () => agent,
      getActiveAgentId: () => "hana",
      getModels: () => ({
        currentModel: MODEL,
        availableModels: [MODEL],
        authStorage: {},
        modelRegistry: {},
        resolveThinkingLevel: (level: unknown) => level || "medium",
      }),
      getResourceLoader: () => ({
        getSystemPrompt: () => "BASE",
        getAppendSystemPrompt: () => [],
        getExtensions: () => ({ extensions: [], errors: [] }),
        getSkills: () => ({ skills: [], diagnostics: [] }),
        getAgentsFiles: () => ({ agentsFiles: [] }),
      }),
      getSkills: () => null,
      buildTools: () => ({ tools: [], customTools: [] }),
      emitEvent: vi.fn(),
      emitDevLog: vi.fn(),
      getHomeCwd: () => tempDir,
      agentIdFromSessionPath: () => "hana",
      switchAgentOnly: async () => {},
      getConfig: () => ({}),
      getPrefs: () => ({
        getThinkingLevel: () => "medium",
        getChannelsEnabled: () => true,
      }),
      getAgents: () => new Map(),
      getActivityStore: () => null,
      getAgentById: () => agent,
      listAgents: () => [agent],
      sessionManifestStore: store,
      // 与 core/engine.ts 生产 wiring 一致的复用查找委托。
      resolveSessionReusableTraceId: (sessionId: string | null) => {
        reuseLookupKeys.push(sessionId);
        return currentHandle.findReusableSessionTraceId(sessionId);
      },
    });
  }

  function queryTraces() {
    const normalized = normalizeModelObservabilityTraceQuery({ filter: {}, minCallCount: 1 });
    if (normalized.ok !== true) throw new Error(`normalize failed: ${JSON.stringify(normalized)}`);
    const page = service.queryTraces(normalized.value);
    if (page.ok !== true) throw new Error(`queryTraces failed: ${JSON.stringify(page)}`);
    return page.value;
  }

  function businessSessionIdOf(coordinator: any, sessionPath: string) {
    return coordinator._getSessionEntryByPath(sessionPath)?.sessionId || null;
  }

  beforeEach(() => {
    vi.clearAllMocks();
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-obs-trace-identity-"));
    agentsDir = path.join(tempDir, "agents");
    sessionDir = path.join(agentsDir, "hana", "sessions");
    fs.mkdirSync(sessionDir, { recursive: true });
    lingxiHome = path.join(tempDir, "lingxi-home");
    fs.mkdirSync(lingxiHome, { recursive: true });
    manifestIdCounter = 0;
    sdkFileCounter = 0;
    reuseLookupKeys = [];
    store = new SessionManifestStore({
      dbPath: path.join(tempDir, "session-manifest.db"),
      idGenerator: () => `sess_business_${String(++manifestIdCounter).padStart(4, "0")}`,
    });
    harness = createModelObservabilityTestHarness({ lingxiHome });
    currentHandle = harness.handle;
    service = createModelObservabilityQueryService({ lingxiHome });

    sessionManagerCreateMock.mockImplementation((cwd: string, dir: string) => {
      sdkFileCounter += 1;
      return fakeSdkSessionManager({
        sessionPath: path.join(dir, `2026-09-10T10-00-0${sdkFileCounter}_sdk-file.jsonl`),
        sdkSessionId: SDK_UUID_FOCUS,
        cwd: cwd || tempDir,
      });
    });
    createAgentSessionMock.mockImplementation(async (opts: any) => ({
      session: buildFakeAgentSession({ manager: opts.sessionManager, model: opts.model || MODEL }),
    }));
  });

  afterEach(async () => {
    service?.close?.();
    await harness.close();
    harness.cleanup();
    try { store?.close(); } catch { /* best-effort */ }
    fs.rmSync(tempDir, { recursive: true, force: true });
  });

  it("T1/T2/T3：同一桌面会话连续三轮 prompt（未等 flush）→ 一个轨迹、三个根调用", async () => {
    const coordinator = buildCoordinator();
    const created = await coordinator.createSession(null, tempDir, true);
    const sessionPath = created.session.sessionManager.getSessionFile();
    const businessSessionId = businessSessionIdOf(coordinator, sessionPath);
    expect(businessSessionId).toBe("sess_business_0001");
    // 双身份事实前提：SDK 文件头身份与业务会话身份必须不同。
    expect(created.session.sessionManager.getSessionId()).toBe(SDK_UUID_FOCUS);
    expect(created.session.sessionManager.getSessionId()).not.toBe(businessSessionId);

    // 三轮紧跟，全程不做 durable flush——热路径复用必须走内存索引。
    await coordinator.prompt("第一轮", undefined);
    await settleObserver();
    await coordinator.prompt("第二轮", undefined);
    await settleObserver();
    await coordinator.prompt("第三轮", undefined);
    await settleObserver();
    harness.flush();

    // 复用查找必须按业务会话 ID 发起（且每轮都查过）。
    expect(reuseLookupKeys).toHaveLength(3);
    for (const key of reuseLookupKeys) expect(key).toBe(businessSessionId);

    const page = queryTraces();
    expect(page.traces).toHaveLength(1);
    const trace = page.traces[0]!;
    expect(trace.origin).toBe("user_turn");
    expect(trace.callCount).toBe(3);
    expect(trace.terminalOk).toBe(3);

    // 三轮各自是独立根调用（不跨轮伪造 parentCallId），归属同一业务会话；
    // Trace Detail 一次返回三轮调用（T12 数据层契约）。
    const detail = service.queryTraceDetail(trace.traceId);
    if (detail.ok !== true) throw new Error(`queryTraceDetail failed: ${JSON.stringify(detail)}`);
    expect(detail.value.calls).toHaveLength(3);
    for (const call of detail.value.calls) {
      expect(call.parentCallId).toBeNull();
      expect(call.attribution?.sessionId).toBe(businessSessionId);
    }
  });

  it("T5：运行时重建（restore）后 SDK 身份变化，同一业务会话仍复用原轨迹", async () => {
    const coordinator = buildCoordinator();
    const created = await coordinator.createSession(null, tempDir, true);
    const sessionPath = created.session.sessionManager.getSessionFile();
    const businessSessionId = businessSessionIdOf(coordinator, sessionPath);

    await coordinator.prompt("第一轮", undefined);
    await settleObserver();
    harness.flush();

    // ── 模拟应用重启：观测 handle 重开（内存索引清空）+ manifest store 重开
    // （同一 dbPath）+ 新 coordinator；恢复出的 SDK 会话身份故意换一个 UUID。 ──
    await harness.handle.close();
    const reopenedHandle = installModelObservabilityPersistence({
      lingxiHome,
      policy: { enabled: true, persistPayloads: true, persistBlobs: true },
    });
    currentHandle = reopenedHandle;
    try { store?.close(); } catch { /* best-effort */ }
    store = new SessionManifestStore({
      dbPath: path.join(tempDir, "session-manifest.db"),
      idGenerator: () => `sess_business_${String(++manifestIdCounter).padStart(4, "0")}`,
    });
    sessionManagerOpenMock.mockImplementation((p: string) => fakeSdkSessionManager({
      sessionPath: p,
      sdkSessionId: SDK_UUID_RESTORED,
      cwd: tempDir,
    }));
    const restarted = buildCoordinator();
    await restarted.ensureSessionLoaded(sessionPath);
    const restoredEntry = restarted._getSessionEntryByPath(sessionPath);
    expect(restoredEntry?.sessionId).toBe(businessSessionId);
    expect(restoredEntry?.session?.sessionManager?.getSessionId()).toBe(SDK_UUID_RESTORED);

    await restarted.promptSession(sessionPath, "重启后的第二轮", undefined);
    await settleObserver();
    reopenedHandle.flushSync();

    const page = queryTraces();
    expect(page.traces).toHaveLength(1);
    expect(page.traces[0]).toMatchObject({ origin: "user_turn", callCount: 2, terminalOk: 2 });
    await reopenedHandle.close();
  });

  it("T9：失败轮次计入同一轨迹 terminalError，后续轮继续复用", async () => {
    const coordinator = buildCoordinator();
    const created = await coordinator.createSession(null, tempDir, true);
    const sessionPath = created.session.sessionManager.getSessionFile();
    const businessSessionId = businessSessionIdOf(coordinator, sessionPath);

    await coordinator.prompt("第一轮", undefined);
    await settleObserver();
    // 第二轮 provider 失败。
    (created.session.sessionManager as any).__failNext = true;
    await coordinator.prompt("第二轮", undefined);
    await settleObserver();
    await coordinator.prompt("第三轮", undefined);
    await settleObserver();
    harness.flush();

    const page = queryTraces();
    expect(page.traces).toHaveLength(1);
    expect(page.traces[0]).toMatchObject({
      origin: "user_turn",
      callCount: 3,
      terminalOk: 2,
      terminalError: 1,
    });
    // 失败轮之后复用查找仍命中同一轨迹（以业务会话 ID）。
    expect(currentHandle.findReusableSessionTraceId(businessSessionId)).toBe(page.traces[0]!.traceId);
  });

  it("T4/T6：不同业务会话即使 SDK 身份相同也绝不共享轨迹", async () => {
    const coordinator = buildCoordinator();
    const first = await coordinator.createSession(null, tempDir, true);
    const firstPath = first.session.sessionManager.getSessionFile();
    const firstBusinessId = businessSessionIdOf(coordinator, firstPath);

    // 第二个业务会话：SDK 侧复用同一 UUID（runtime 对象碰巧复用的极端情形），
    // 业务 manifest 身份必须仍然区分两条轨迹。
    await coordinator.prompt("会话 A 第一轮", undefined);
    await settleObserver();

    const second = await coordinator.createSession(null, tempDir, true);
    const secondPath = second.session.sessionManager.getSessionFile();
    const secondBusinessId = businessSessionIdOf(coordinator, secondPath);
    expect(secondBusinessId).not.toBe(firstBusinessId);
    expect(second.session.sessionManager.getSessionId()).toBe(SDK_UUID_FOCUS);

    await coordinator.prompt("会话 B 第一轮", undefined);
    await settleObserver();
    harness.flush();

    const page = queryTraces();
    expect(page.traces).toHaveLength(2);
    const byTraceId = new Map(page.traces.map((trace) => [trace.traceId, trace]));
    expect(byTraceId.size).toBe(2);
    const detailCalls = [...byTraceId.keys()].flatMap((traceId) => {
      const detail = service.queryTraceDetail(traceId);
      if (detail.ok !== true) throw new Error("queryTraceDetail failed");
      return detail.value.calls;
    });
    const sessionIds = new Set(detailCalls.map((call) => call.attribution?.sessionId));
    expect(sessionIds).toEqual(new Set([firstBusinessId, secondBusinessId]));
  });
});
