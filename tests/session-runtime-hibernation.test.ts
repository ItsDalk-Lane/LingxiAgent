import path from "path";
import fs from "node:fs";
import { describe, it, expect, vi, beforeEach } from "vitest";

const { createAgentSessionMock, sessionManagerCreateMock, sessionManagerOpenMock, emitSessionShutdownMock, refreshSessionModelFromRegistryMock } = vi.hoisted(() => ({
  createAgentSessionMock: vi.fn(),
  sessionManagerCreateMock: vi.fn(),
  sessionManagerOpenMock: vi.fn(),
  emitSessionShutdownMock: vi.fn(),
  refreshSessionModelFromRegistryMock: vi.fn(),
}));

vi.mock("../lib/pi-sdk/index.js", () => ({
  createAgentSession: createAgentSessionMock,
  emitSessionShutdown: emitSessionShutdownMock,
  SessionManager: {
    create: sessionManagerCreateMock,
    open: sessionManagerOpenMock,
  },
  SettingsManager: {
    inMemory: vi.fn(() => ({})),
  },
  estimateTokens: vi.fn(() => 0),
  findCutPoint: vi.fn(() => 0),
  generateSummary: vi.fn(),
  refreshSessionModelFromRegistry: refreshSessionModelFromRegistryMock,
}));

vi.mock("../lib/debug-log.js", () => ({
  createModuleLogger: () => ({
    log: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  }),
}));

import { SessionCoordinator } from "../core/session-coordinator.ts";

const MODEL = {
  id: "test-model",
  name: "test-model",
  provider: "test",
  input: ["text", "image"],
};

function makeSession(sessionPath, overrides: any = {}) {
  const sessionManager = overrides.sessionManager || {
    getSessionFile: () => sessionPath,
    getCwd: () => path.dirname(sessionPath),
  };
  return {
    sessionManager,
    model: MODEL,
    isStreaming: false,
    isCompacting: false,
    messages: [],
    prompt: vi.fn(async () => {}),
    subscribe: vi.fn(() => vi.fn()),
    setActiveToolsByName: vi.fn(),
    dispose: vi.fn(),
    ...overrides,
  };
}

function makeAgent(root = "/tmp/hana-runtime-hibernation") {
  return {
    id: "hana",
    agentDir: path.join(root, "agents", "hana"),
    sessionDir: path.join(root, "agents", "hana", "sessions"),
    memoryMasterEnabled: true,
    sessionMemoryEnabled: true,
    setMemoryEnabled: vi.fn(),
    buildSystemPrompt: () => "BASE",
    getToolsSnapshot: vi.fn(() => []),
    config: {},
  };
}

function makeCoordinator( overrides: any = {}) {
  const root = overrides.root || "/tmp/hana-runtime-hibernation";
  const agent = overrides.agent || makeAgent(root);
  const models = overrides.models || {
    currentModel: MODEL,
    availableModels: [MODEL],
    authStorage: {},
    modelRegistry: {},
    resolveThinkingLevel: () => "medium",
  };
  return new SessionCoordinator({
    agentsDir: path.join(root, "agents"),
    getAgent: () => agent,
    getActiveAgentId: () => agent.id,
    getModels: () => models,
    getResourceLoader: () => ({
      getSystemPrompt: () => "BASE",
      getAppendSystemPrompt: () => [],
      getExtensions: () => ({ extensions: [], errors: [] }),
      getSkills: () => ({ skills: [], diagnostics: [] }),
      getAgentsFiles: () => ({ agentsFiles: [] }),
    }),
    getSkills: () => null,
    buildTools: vi.fn(() => ({ tools: [], customTools: [] })),
    emitEvent: vi.fn(),
    getHomeCwd: () => root,
    agentIdFromSessionPath: () => agent.id,
    switchAgentOnly: vi.fn(async () => {}),
    getConfig: () => ({}),
    getPrefs: () => ({ getThinkingLevel: () => "medium" }),
    getAgents: () => new Map([[agent.id, agent]]),
    getActivityStore: () => null,
    getAgentById: () => agent,
    listAgents: () => [agent],
    getDeferredResultStore: () => null,
    memoryPressure: overrides.memoryPressure,
    getEngine: overrides.getEngine,
  });
}

describe("SessionCoordinator runtime hibernation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    refreshSessionModelFromRegistryMock.mockImplementation((session, allowedModel) => {
      if (allowedModel !== undefined) {
        if (session?.agent?.state) session.agent.state.model = allowedModel;
        if (session && Object.prototype.hasOwnProperty.call(session, "model")) session.model = allowedModel;
      } else {
        session?._refreshCurrentModelFromRegistry?.();
      }
      return true;
    });
    emitSessionShutdownMock.mockResolvedValue(false);
    sessionManagerCreateMock.mockReturnValue({ getCwd: () => "/tmp/workspace", getSessionFile: () => "/tmp/session.jsonl" });
  });

  it("releases a focused runtime while preserving the current session path", async () => {
    const sessionPath = "/tmp/hana-runtime-hibernation/agents/hana/sessions/current.jsonl";
    const session = makeSession(sessionPath, {
      getContextUsage: vi.fn(() => ({ tokens: 123, contextWindow: 1000, percent: 12.3 })),
    });
    const unsub = vi.fn();
    const coordinator = makeCoordinator();
    coordinator._session = session;
    coordinator._sessionStarted = true;
    coordinator._sessions.set(sessionPath, {
      session,
      unsub,
      agentId: "hana",
      modelId: "test-model",
      modelProvider: "test",
      workspaceFolders: ["/tmp/workspace", "/tmp/other"],
      permissionMode: "operate",
      accessMode: "operate",
      planMode: false,
      thinkingLevel: "high",
      lastTouchedAt: Date.now() - 60_000,
    });

    await expect(
      coordinator.hibernateSessionRuntime(sessionPath, "test"),
    ).resolves.toBe(true);

    expect(coordinator.getSessionByPath(sessionPath)).toBeNull();
    expect(coordinator.session).toBeNull();
    expect(coordinator.currentSessionPath).toBe(sessionPath);
    expect(coordinator.sessionStarted).toBe(true);
    expect(coordinator.getCurrentSessionModelRef()).toEqual({ id: "test-model", provider: "test" });
    expect(coordinator.getSessionWorkspaceFolders(sessionPath)).toEqual(["/tmp/workspace", "/tmp/other"]);
    expect(coordinator.getPermissionMode(sessionPath)).toBe("operate");
    expect(coordinator.getSessionThinkingLevel(sessionPath)).toBe("high");
    expect(coordinator.getSessionContextUsage(sessionPath)).toEqual({ tokens: 123, contextWindow: 1000, percent: 12.3 });
    expect(unsub).toHaveBeenCalledOnce();
    expect(session.dispose).toHaveBeenCalledOnce();
  });

  it("freezes the context breakdown with the usage snapshot and reconciles it on read", async () => {
    const sessionPath = "/tmp/hana-runtime-hibernation/agents/hana/sessions/current.jsonl";
    const session = makeSession(sessionPath, {
      getContextUsage: vi.fn(() => ({ tokens: 100, contextWindow: 1000, percent: 10 })),
    });
    const coordinator = makeCoordinator();
    coordinator._session = session;
    coordinator._sessionStarted = true;
    const estimate = {
      system: 40, skills: 0, files: 0, tools: 10, mcp: 0,
      conversation: 30, user: 5, toolResults: 0, computedAt: 1,
    };
    coordinator._sessions.set(sessionPath, {
      session,
      unsub: vi.fn(),
      agentId: "hana",
      modelId: "test-model",
      modelProvider: "test",
      workspaceFolders: [],
      permissionMode: "operate",
      accessMode: "operate",
      planMode: false,
      thinkingLevel: "high",
      lastTouchedAt: Date.now() - 60_000,
      contextBreakdown: estimate,
    });

    // live 读取:breakdown 与 usage.tokens 对账,other 吸收差值,合计闭合。
    const live = coordinator.getSessionContextUsage(sessionPath);
    expect(live.breakdown).toMatchObject({
      system: 40, tools: 10, conversation: 30, user: 5, other: 15, total: 100,
    });

    await expect(coordinator.hibernateSessionRuntime(sessionPath, "test")).resolves.toBe(true);

    // 休眠后读取同一份冻结快照,明细不丢、口径不变。
    const hibernatedMeta = coordinator._getRuntimeValueForPath(coordinator._hibernatedSessionMeta, sessionPath);
    expect(hibernatedMeta?.contextBreakdown).toEqual(estimate);
    const frozen = coordinator.getSessionContextUsage(sessionPath);
    expect(frozen.breakdown).toMatchObject({
      system: 40, tools: 10, conversation: 30, user: 5, other: 15, total: 100,
    });
  });

  it("omits the breakdown when the total is unknown after compaction", async () => {
    const sessionPath = "/tmp/hana-runtime-hibernation/agents/hana/sessions/current.jsonl";
    const session = makeSession(sessionPath, {
      getContextUsage: vi.fn(() => ({ tokens: null, contextWindow: 1000, percent: null })),
    });
    const coordinator = makeCoordinator();
    coordinator._session = session;
    coordinator._sessionStarted = true;
    coordinator._sessions.set(sessionPath, {
      session,
      unsub: vi.fn(),
      agentId: "hana",
      contextBreakdown: {
        system: 40, skills: 0, files: 0, tools: 10, mcp: 0,
        conversation: 30, user: 5, toolResults: 0, computedAt: 1,
      },
    });

    // compaction 后 tokens 未知:breakdown 显式缺席(WS 层回落 null),不伪造明细。
    expect(coordinator.getSessionContextUsage(sessionPath)).toEqual({
      tokens: null,
      contextWindow: 1000,
      percent: null,
    });
  });

  it("restores a hibernated focused runtime before prompting", async () => {
    const sessionPath = "/tmp/hana-runtime-hibernation/agents/hana/sessions/current.jsonl";
    const manager = { getCwd: () => "/tmp/workspace", getSessionFile: () => sessionPath };
    const restored = makeSession(sessionPath, { sessionManager: manager });
    sessionManagerOpenMock.mockReturnValue(manager);
    createAgentSessionMock.mockResolvedValue({ session: restored });

    const coordinator = makeCoordinator();
    coordinator._currentSessionPath = sessionPath;
    coordinator._sessionStarted = true;

    await (coordinator as any).promptSession(sessionPath, "hello");

    expect(sessionManagerOpenMock).toHaveBeenCalledWith(sessionPath, expect.stringContaining("sessions"));
    expect(restored.prompt).toHaveBeenCalledWith("hello", undefined);
    expect(coordinator.session).toBe(restored);
    expect(coordinator.currentSessionPath).toBe(sessionPath);
  });

  it("restores the persisted context breakdown estimate from session-meta when the entry is rebuilt", async () => {
    // 用户路径:聊天后切换会话再切回,entry 已被淘汰/重建,内存 contextBreakdown 丢失。
    // 落盘的 contextUsageEstimate 在 restore 时交还新 entry,读取侧与 tokens 对账,
    // Context Ring 详情不再只剩"暂无数据"。
    const sessionPath = "/tmp/hana-runtime-hibernation/agents/hana/sessions/meta-restore.jsonl";
    const sessionsDir = path.dirname(sessionPath);
    fs.mkdirSync(sessionsDir, { recursive: true });
    const metaPath = path.join(sessionsDir, "session-meta.json");
    const estimate = {
      system: 40, skills: 0, files: 10, tools: 10, mcp: 0,
      conversation: 30, user: 5, toolResults: 0, computedAt: 42,
    };
    fs.writeFileSync(metaPath, JSON.stringify({ "meta-restore.jsonl": { contextUsageEstimate: estimate } }, null, 2));

    const manager = { getCwd: () => "/tmp/workspace", getSessionFile: () => sessionPath };
    const restored = makeSession(sessionPath, {
      sessionManager: manager,
      getContextUsage: vi.fn(() => ({ tokens: 100, contextWindow: 1000, percent: 10 })),
    });
    sessionManagerOpenMock.mockReturnValue(manager);
    createAgentSessionMock.mockResolvedValue({ session: restored });

    const coordinator = makeCoordinator();
    coordinator._currentSessionPath = sessionPath;
    coordinator._sessionStarted = true;

    await (coordinator as any).promptSession(sessionPath, "hello");

    const usage = coordinator.getSessionContextUsage(sessionPath);
    // known = 40+10+10+30+5 = 95,total = 100 → other 吸收 5。
    expect(usage.breakdown).toMatchObject({
      system: 40, skills: 0, files: 10, tools: 10, mcp: 0,
      conversation: 30, user: 5, toolResults: 0, other: 5, total: 100,
    });
  });

  it("ignores a malformed persisted context estimate instead of restoring a broken split", async () => {
    const sessionPath = "/tmp/hana-runtime-hibernation/agents/hana/sessions/meta-corrupt.jsonl";
    const sessionsDir = path.dirname(sessionPath);
    fs.mkdirSync(sessionsDir, { recursive: true });
    const metaPath = path.join(sessionsDir, "session-meta.json");
    let meta: Record<string, any> = {};
    if (fs.existsSync(metaPath)) {
      try { meta = JSON.parse(fs.readFileSync(metaPath, "utf-8")); } catch { meta = {}; }
    }
    meta["meta-corrupt.jsonl"] = { contextUsageEstimate: { system: "40", conversation: 30 } };
    fs.writeFileSync(metaPath, JSON.stringify(meta, null, 2));

    const manager = { getCwd: () => "/tmp/workspace", getSessionFile: () => sessionPath };
    const restored = makeSession(sessionPath, {
      sessionManager: manager,
      getContextUsage: vi.fn(() => ({ tokens: 100, contextWindow: 1000, percent: 10 })),
    });
    sessionManagerOpenMock.mockReturnValue(manager);
    createAgentSessionMock.mockResolvedValue({ session: restored });

    const coordinator = makeCoordinator();
    coordinator._currentSessionPath = sessionPath;
    coordinator._sessionStarted = true;

    await (coordinator as any).promptSession(sessionPath, "hello");

    const usage = coordinator.getSessionContextUsage(sessionPath);
    expect(usage.breakdown).toBeUndefined();
  });

  it("rejects the next live-session prompt before vision or Pi when its model was disabled", async () => {
    const sessionPath = "/tmp/hana-runtime-hibernation/agents/hana/sessions/gpt56.jsonl";
    const staleModel = {
      id: "gpt-5.6-sol",
      provider: "openai-codex",
      input: ["text"],
      contextWindow: 353400,
    };
    const models = {
      currentModel: staleModel,
      availableModels: [staleModel],
      authStorage: {},
      modelRegistry: {},
      resolveThinkingLevel: () => "low",
    };
    const visionPrepare = vi.fn(async () => ({ text: "vision output", images: [] }));
    const session = makeSession(sessionPath, { model: staleModel });
    const coordinator = makeCoordinator({
      models,
      getEngine: () => ({
        isVisionAuxiliaryEnabled: () => true,
        getVisionBridge: () => ({ prepare: visionPrepare }),
      }),
    });
    coordinator._sessions.set(sessionPath, {
      session,
      agentId: "hana",
      modelId: staleModel.id,
      modelProvider: staleModel.provider,
      lastTouchedAt: 0,
    });

    models.availableModels = [];
    coordinator.refreshAllSessionsModels();

    const result = coordinator.promptSession(sessionPath, "describe image", {
      images: [{ type: "image", data: "aGVsbG8=", mimeType: "image/png" }],
    });
    await expect(result).rejects.toMatchObject({
      code: "MODEL_NOT_AVAILABLE",
      modelRef: "openai-codex/gpt-5.6-sol",
    });
    expect(visionPrepare).not.toHaveBeenCalled();
    expect(session.prompt).not.toHaveBeenCalled();
    expect(refreshSessionModelFromRegistryMock).not.toHaveBeenCalled();
  });

  it("rebinds a live session to current Hana metadata and continues prompting", async () => {
    const sessionPath = "/tmp/hana-runtime-hibernation/agents/hana/sessions/gpt56-metadata.jsonl";
    const staleModel = {
      id: "gpt-5.6-sol",
      provider: "openai-codex",
      api: "openai-codex-responses",
      baseUrl: "https://stale.example",
      contextWindow: 272000,
      thinkingLevels: ["low", "medium", "high"],
    };
    const freshModel = {
      ...staleModel,
      baseUrl: "https://chatgpt.com/backend-api",
      contextWindow: 353400,
      maxTokens: 128000,
      thinkingLevels: ["low", "medium", "high", "max"],
      thinkingLevelMap: { xhigh: "max" },
    };
    const models = {
      currentModel: freshModel,
      availableModels: [freshModel],
      authStorage: {},
      modelRegistry: {},
      resolveThinkingLevel: (level) => level,
    };
    const session = makeSession(sessionPath, {
      model: staleModel,
      agent: { state: { model: staleModel, systemPrompt: "BASE", tools: [] } },
    });
    const coordinator = makeCoordinator({ models });
    coordinator._sessions.set(sessionPath, {
      session,
      agentId: "hana",
      modelId: staleModel.id,
      modelProvider: staleModel.provider,
      lastTouchedAt: 0,
    });

    coordinator.refreshAllSessionsModels();

    expect(refreshSessionModelFromRegistryMock).toHaveBeenCalledWith(session, freshModel);
    expect(session.model).toBe(freshModel);
    expect(session.model).toMatchObject({
      baseUrl: "https://chatgpt.com/backend-api",
      contextWindow: 353400,
      maxTokens: 128000,
      thinkingLevels: ["low", "medium", "high", "max"],
      thinkingLevelMap: { xhigh: "max" },
    });
    await expect(coordinator.promptSession(sessionPath, "hello", undefined)).resolves.toBeUndefined();
    expect(session.prompt).toHaveBeenCalledWith("hello", undefined);
  });

  it("hibernates only heavy idle runtimes under memory pressure", async () => {
    const sessionPath = "/tmp/hana-runtime-hibernation/agents/hana/sessions/heavy.jsonl";
    const session = makeSession(sessionPath, {
      messages: [
        {
          role: "user",
          content: [
            { type: "text", text: "analyze this" },
            { type: "image", data: "a".repeat(4096), mimeType: "image/png" },
          ],
        },
      ],
    });
    const coordinator = makeCoordinator({
      memoryPressure: {
        getMemoryUsage: () => ({
          rss: 2 * 1024 * 1024 * 1024,
          heapUsed: 128 * 1024 * 1024,
          external: 64 * 1024 * 1024,
          arrayBuffers: 64 * 1024 * 1024,
        }),
        thresholds: {
          minRetainedBytes: 1024,
          highRssBytes: 1024,
          highPayloadBytes: 4096,
        },
      },
    });
    coordinator._currentSessionPath = sessionPath;
    coordinator._session = session;
    coordinator._sessions.set(sessionPath, {
      session,
      unsub: vi.fn(),
      agentId: "hana",
      lastTouchedAt: Date.now() - 60_000,
    });

    await expect(
      coordinator.checkRuntimeMemoryPressure(sessionPath, "test"),
    ).resolves.toMatchObject({ hibernated: true });

    expect(coordinator.getSessionByPath(sessionPath)).toBeNull();
    expect(coordinator.currentSessionPath).toBe(sessionPath);
  });
});
