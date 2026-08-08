/**
 * 模型选择无 fallback 测试
 *
 * 验证所有模型选择路径在找不到指定模型时抛错，而非静默 fallback。
 */

import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, it, expect, vi, beforeEach } from "vitest";

// ── Mock Pi SDK ──

const { createAgentSessionMock, emitSessionShutdownMock, sessionManagerCreateMock } = vi.hoisted(() => ({
  createAgentSessionMock: vi.fn(),
  emitSessionShutdownMock: vi.fn(async () => true),
  sessionManagerCreateMock: vi.fn(),
}));

vi.mock("../lib/pi-sdk/index.js", () => ({
  createAgentSession: createAgentSessionMock,
  emitSessionShutdown: emitSessionShutdownMock,
  getPiModels: vi.fn(() => []),
  SessionManager: {
    create: sessionManagerCreateMock,
    open: vi.fn(),
  },
  SettingsManager: {
    inMemory: vi.fn(() => ({})),
  },
}));

vi.mock("../lib/debug-log.js", () => ({
  createModuleLogger: () => ({
    log: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  }),
}));

import { SessionCoordinator } from "../core/session-coordinator.ts";

// ── Helpers ──

function makeModels(list = []) {
  return {
    authStorage: {},
    modelRegistry: {},
    defaultModel: list[0] || null,
    availableModels: list,
    resolveExecutionModel: (m) => m,
    resolveThinkingLevel: () => "medium",
    inferModelProvider: () => null,
  };
}

function makeCoordinator(tempDir, { agentConfig = {}, models = makeModels() } = {}) {
  const sessionPath = path.join(tempDir, "s.jsonl");
  let manifest = null;
  const branchHeads = new Map();
  const sessionManifestStore = {
    resolveByLocatorPath: vi.fn((candidate) => manifest?.currentLocator?.path === candidate ? manifest : null),
    getBySessionId: vi.fn((sessionId) => manifest?.sessionId === sessionId ? manifest : null),
    createForPath: vi.fn((input) => {
      manifest = {
        ...input,
        sessionId: "sess_model_test",
        lifecycle: "active",
        currentLocator: { path: input.sessionPath },
      };
      return manifest;
    }),
    updateLocatorLifecycle: vi.fn((sessionId, nextPath, lifecycle) => {
      manifest = { ...manifest, sessionId, lifecycle, currentLocator: { path: nextPath } };
      return manifest;
    }),
    getBranchHead: vi.fn((sessionId) => branchHeads.get(sessionId) || null),
    setBranchHead: vi.fn((sessionId, head) => {
      const stored = { ...head, sessionId };
      branchHeads.set(sessionId, stored);
      return stored;
    }),
    setMemoryPolicy: vi.fn(),
    setPermissionModeSnapshot: vi.fn(),
    setThinkingLevel: vi.fn(),
    setWorkspaceScope: vi.fn(),
    setPlugin: vi.fn(),
  };
  sessionManagerCreateMock.mockReturnValue({
    getCwd: () => tempDir,
    getSessionFile: () => sessionPath,
  });
  createAgentSessionMock.mockResolvedValue({
    session: {
      sessionManager: { getSessionFile: () => sessionPath },
      subscribe: vi.fn(() => vi.fn()),
      abort: vi.fn(),
    },
  });

  return new SessionCoordinator({
    agentsDir: tempDir,
    getAgent: () => ({
      agentDir: tempDir,
      sessionDir: tempDir,
      agentName: "test-agent",
      config: agentConfig,
      tools: [],
      buildSystemPrompt: () => "prompt",
    }),
    getActiveAgentId: () => "test",
    getModels: () => models,
    getResourceLoader: () => ({ getSystemPrompt: () => "prompt" }),
    getSkills: () => ({ getSkillsForAgent: () => [] }),
    buildTools: () => ({ tools: [], customTools: [] }),
    emitEvent: () => {},
    getHomeCwd: () => tempDir,
    agentIdFromSessionPath: () => null,
    switchAgentOnly: async () => {},
    getConfig: () => ({}),
    getPrefs: () => ({ getThinkingLevel: () => "medium" }),
    getAgents: () => new Map(),
    getActivityStore: () => null,
    getAgentById: (id) => ({
      agentDir: tempDir,
      sessionDir: tempDir,
      agentName: id,
      config: agentConfig,
      tools: [],
      buildSystemPrompt: () => "prompt",
    }),
    listAgents: () => [],
    sessionManifestStore,
  });
}

// ── Tests ──

describe("模型选择无 fallback", () => {
  let tempDir;

  beforeEach(() => {
    vi.clearAllMocks();
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-model-nofallback-"));
  });

  afterEach(() => {
    fs.rmSync(tempDir, { recursive: true, force: true });
  });

  // ────── resolveModel (createSessionContext) ──────

  describe("resolveModel", () => {
    it("找到指定模型时正常返回", () => {
      const models = makeModels([
        { id: "qwen3.5-plus", provider: "dashscope" },
        { id: "gpt-5", provider: "openai" },
      ]);
      const coord = makeCoordinator(tempDir, {
        agentConfig: { models: { chat: { id: "qwen3.5-plus", provider: "dashscope" } } },
        models,
      });
      const ctx = coord.createSessionContext();
      const result = ctx.resolveModel({ models: { chat: { id: "qwen3.5-plus", provider: "dashscope" } } });
      expect(result).toEqual({ id: "qwen3.5-plus", provider: "dashscope" });
    });

    it("models.chat 未配置、有 defaultModel 时回退到默认模型", () => {
      const coord = makeCoordinator(tempDir, {
        agentConfig: {},
        models: makeModels([{ id: "some-model", provider: "x" }]),
      });
      const ctx = coord.createSessionContext();
      expect(ctx.resolveModel({})).toEqual({ id: "some-model", provider: "x" });
      expect(ctx.resolveModel({ models: {} })).toEqual({ id: "some-model", provider: "x" });
    });

    it("models.chat 未配置、无 defaultModel 时抛错", () => {
      const coord = makeCoordinator(tempDir, {
        agentConfig: {},
        models: makeModels([]),
      });
      const ctx = coord.createSessionContext();
      expect(() => ctx.resolveModel({})).toThrow(/resolveModelNoChatModel|models\.chat|未指定/);
    });

    it("指定的模型不在 availableModels 中、有 defaultModel 时回退", () => {
      const models = makeModels([
        { id: "gpt-5", provider: "openai" },
        { id: "MiniMax-M2", provider: "minimax" },
      ]);
      const coord = makeCoordinator(tempDir, { models });
      const ctx = coord.createSessionContext();
      // 有 defaultModel，回退而非抛错
      expect(ctx.resolveModel({ models: { chat: { id: "qwen3.5-plus", provider: "dashscope" } } }))
        .toEqual({ id: "gpt-5", provider: "openai" });
    });

    it("指定的模型不在 availableModels 中、无 defaultModel 时抛错", () => {
      const models = { ...makeModels([]), defaultModel: null };
      const coord = makeCoordinator(tempDir, { models });
      const ctx = coord.createSessionContext();
      expect(() => ctx.resolveModel({ models: { chat: { id: "qwen3.5-plus", provider: "dashscope" } } }))
        .toThrow(/resolveModelNotAvailable|不在可用列表|not available/);
    });

    it("availableModels 为空时抛错", () => {
      const coord = makeCoordinator(tempDir, { models: makeModels([]) });
      const ctx = coord.createSessionContext();
      expect(() => ctx.resolveModel({ models: { chat: { id: "qwen3.5-plus", provider: "dashscope" } } }))
        .toThrow(/resolveModelNotAvailable|不在可用列表|not available/);
    });

    it("restores a disabled historical model as unavailable before the SDK can fallback", async () => {
      const allowedModel = { id: "allowed-model", provider: "openai" };
      const coord = makeCoordinator(tempDir, { models: makeModels([allowedModel]) });
      const sessionMgr = {
        getCwd: () => tempDir,
        getSessionFile: () => path.join(tempDir, "disabled-restore.jsonl"),
        getEntries: () => [],
        resetLeaf: vi.fn(),
        buildSessionContext: () => ({
          model: { provider: "openai-codex", modelId: "disabled-model" },
        }),
      };
      createAgentSessionMock.mockImplementationOnce(async (options) => ({
        session: {
          sessionManager: sessionMgr,
          model: options.model,
          messages: [],
          agent: {
            state: {
              model: options.model,
              messages: [],
              systemPrompt: "prompt",
              tools: [],
            },
            streamFn: vi.fn(),
          },
          isStreaming: false,
          isCompacting: false,
          subscribe: vi.fn(() => vi.fn()),
          setActiveToolsByName: vi.fn(),
          setThinkingLevel: vi.fn(),
          getContextUsage: vi.fn(() => null),
        },
      }));

      await expect(coord.createSession(
        sessionMgr,
        tempDir,
        true,
        null,
        { restore: true },
      )).resolves.toBeDefined();
      expect(createAgentSessionMock).toHaveBeenCalledOnce();
      expect(createAgentSessionMock.mock.calls[0][0].model).toMatchObject({
        id: "disabled-model",
        provider: "openai-codex",
        api: "hana-unavailable-model",
      });
      expect(coord.getSessionModelAvailability(sessionMgr.getSessionFile())).toMatchObject({
        available: false,
        modelRef: "openai-codex/disabled-model",
      });
    });

    it("tears down a restored session when the SDK reports a model fallback", async () => {
      const allowedModel = { id: "allowed-model", provider: "openai" };
      const coord = makeCoordinator(tempDir, { models: makeModels([allowedModel]) });
      const dispose = vi.fn();
      createAgentSessionMock.mockResolvedValue({
        session: { model: allowedModel, dispose },
        modelFallbackMessage: "disabled-model -> allowed-model",
      });
      const sessionMgr = {
        getCwd: () => tempDir,
        getSessionFile: () => path.join(tempDir, "fallback-restore.jsonl"),
        getEntries: () => [],
        resetLeaf: vi.fn(),
        buildSessionContext: () => ({
          model: { provider: "openai", modelId: "allowed-model" },
        }),
      };

      await expect(coord.createSession(
        sessionMgr,
        tempDir,
        true,
        null,
        { restore: true },
      )).rejects.toThrow(/fallback rejected/);
      expect(emitSessionShutdownMock).toHaveBeenCalled();
      expect(dispose).toHaveBeenCalled();
    });
  });

  // ────── executeIsolated ──────

  describe("executeIsolated", () => {
    it("agent 未配置 models.chat、无 defaultModel 时抛错", async () => {
      const coord = makeCoordinator(tempDir, {
        agentConfig: {},
        models: makeModels([]),
      });
      const result = await coord.executeIsolated("hello");
      expect(result.error).toMatch(/executeIsolatedNoModel|无可用模型|no available model/);
    });

    it("配置的模型不在可用列表中、无 defaultModel 时抛错", async () => {
      const coord = makeCoordinator(tempDir, {
        agentConfig: { models: { chat: { id: "nonexistent-model", provider: "dashscope" } } },
        models: { ...makeModels([]), defaultModel: null },
      });
      const result = await coord.executeIsolated("hello");
      expect(result.error).toMatch(/executeIsolatedNoModel|无可用模型|no available model/);
    });

    it("模型匹配成功时正常执行", async () => {
      const coord = makeCoordinator(tempDir, {
        agentConfig: { models: { chat: { id: "qwen3.5-plus", provider: "dashscope" } } },
        models: makeModels([{ id: "qwen3.5-plus", provider: "dashscope" }]),
      });

      createAgentSessionMock.mockResolvedValue({
        session: {
          sessionManager: { getSessionFile: () => path.join(tempDir, "s.jsonl") },
          subscribe: vi.fn(() => vi.fn()),
          prompt: vi.fn(),
        },
      });

      const result = await coord.executeIsolated("hello");
      expect(result.error).toBeFalsy();
      expect(createAgentSessionMock).toHaveBeenCalledOnce();
      expect(createAgentSessionMock.mock.calls[0][0].model).toEqual({
        id: "qwen3.5-plus",
        provider: "dashscope",
      });
    });

    it("通过 opts.model 显式传入模型时跳过 config 查找", async () => {
      const explicitModel = { id: "explicit", provider: "test" };
      const coord = makeCoordinator(tempDir, {
        agentConfig: {},  // 没有 models.chat
        models: makeModels([explicitModel]),
      });

      createAgentSessionMock.mockResolvedValue({
        session: {
          sessionManager: { getSessionFile: () => path.join(tempDir, "s.jsonl") },
          subscribe: vi.fn(() => vi.fn()),
          prompt: vi.fn(),
        },
      });

      const result = await coord.executeIsolated("hello", { model: explicitModel });
      expect(result.error).toBeFalsy();
    });
  });

  // ────── resolveModelWithCredentials ──────

  describe("resolveModelWithCredentials", () => {
    let ModelManager;

    beforeEach(async () => {
      const mod = await import("../core/model-manager.ts");
      ModelManager = mod.ModelManager;
    });

    it("对象模型引用会先解析成 availableModels 里的完整模型对象", () => {
      const mm = new ModelManager({ lingxiHome: tempDir });
      const fullModel = {
        id: "kimi-k2.6",
        provider: "kimi-coding",
        input: ["text", "image"],
        contextWindow: 262144,
      };
      mm._availableModels = [fullModel];
      mm.providerRegistry = {
        getCredentials: vi.fn((provider) => (
          provider === "kimi-coding"
            ? {
                api: "anthropic-messages",
                apiKey: "sk-test",
                baseUrl: "https://api.kimi.com/coding/",
              }
            : null
        )),
      };

      const result = mm.resolveModelWithCredentials({
        id: "kimi-k2.6",
        provider: "kimi-coding",
      });

      expect(result.model).toBe(fullModel);
      expect(result.model.input).toEqual(["text", "image"]);
    });

    it("provider 声明无须 key 时，远程 baseUrl 也能解析执行凭证", () => {
      const mm = new ModelManager({ lingxiHome: tempDir });
      const fullModel = {
        id: "llama3",
        provider: "ollama",
        input: ["text"],
      };
      const allowsMissingApiKey = vi.fn(() => true);
      mm._availableModels = [fullModel];
      mm.providerRegistry = {
        getCredentials: vi.fn((provider) => (
          provider === "ollama"
            ? {
                api: "openai-completions",
                apiKey: "",
                baseUrl: "http://192.168.1.20:11434/v1",
              }
            : null
        )),
        allowsMissingApiKey,
      };

      const result = mm.resolveModelWithCredentials({
        id: "llama3",
        provider: "ollama",
      });

      expect(result.model).toBe(fullModel);
      expect(result.api_key).toBe("");
      expect(result.base_url).toBe("http://192.168.1.20:11434/v1");
      expect(allowsMissingApiKey).toHaveBeenCalledWith(
        "ollama",
        "http://192.168.1.20:11434/v1",
      );
    });

    it("uses the model API even when the provider-wide API is empty", () => {
      const mm = new ModelManager({ lingxiHome: tempDir });
      const fullModel = {
        id: "gpt-5.6-sol",
        provider: "openai",
        api: "openai-responses",
      };
      mm._availableModels = [fullModel];
      mm.providerRegistry = {
        getCredentials: vi.fn(() => ({
          api: "",
          apiKey: "sk-test",
          baseUrl: "https://api.openai.com/v1",
        })),
      };

      expect(mm.resolveModelWithCredentials(fullModel).api).toBe("openai-responses");
    });
  });

  describe("resolveAuxiliaryModel (slot-based)", () => {
    // 直接测试 AuxiliaryModelResolver：每个 Slot 独立解析，无 dual-utility 概念。
    let AuxiliaryModelResolver;

    beforeEach(async () => {
      const mod = await import("../core/auxiliary-model-resolver.ts");
      AuxiliaryModelResolver = mod.AuxiliaryModelResolver;
    });

    /**
     * 构造 resolver。
     * @param {object[]} models  availableModels 列表（支持 _cred / _allowMissingApiKey 测试标记）
     * @param {object} slotRefs  { summarize: {id, provider}, memory: {...} }
     * @param {object|null} chatModel  chat fallback 模型（未配置 Slot 时用）
     */
    function makeResolver(models, slotRefs, chatModel = null) {
      const resolveModel = (ref) => {
        if (!ref || typeof ref !== "object") return null;
        return models.find((m) => m.id === ref.id && m.provider === ref.provider) || null;
      };
      const getChatModel = () => chatModel;
      const getSlotModelRef = (slot) => slotRefs?.[slot] || null;
      const getProviderCredentials = (provider) => {
        const model = models.find((m) => m.provider === provider);
        return model?._cred || null;
      };
      const resolveProviderCredentialsFresh = async (provider) => getProviderCredentials(provider);
      const allowsMissingApiKey = (provider, baseUrl) => {
        const model = models.find((m) => m.provider === provider);
        return model?._allowMissingApiKey === true || false;
      };
      return new AuxiliaryModelResolver({
        resolveModel,
        getChatModel,
        getSlotModelRef,
        resolveProviderCredentialsFresh,
        getProviderCredentials,
        allowsMissingApiKey,
      });
    }

    it("Slot 已配置时返回该 Slot 的模型（不再有 utility/utility_large 区分）", () => {
      const models = [
        { id: "util-model", provider: "test-provider", _cred: { api: "openai-completions", apiKey: "sk-test", baseUrl: "https://test.example.com/v1" } },
        { id: "large-model", provider: "test-provider", _cred: { api: "openai-completions", apiKey: "sk-test", baseUrl: "https://test.example.com/v1" } },
      ];
      const resolver = makeResolver(models, {
        summarize: { id: "util-model", provider: "test-provider" },
        memory: { id: "large-model", provider: "test-provider" },
      });

      const summarize = resolver.resolveAuxiliaryModel("summarize");
      const memory = resolver.resolveAuxiliaryModel("memory");

      expect(summarize.model).toMatchObject({ id: "util-model", provider: "test-provider" });
      expect(memory.model).toMatchObject({ id: "large-model", provider: "test-provider" });
      expect(summarize.apiKey).toBe("sk-test");
      expect(summarize.api).toBe("openai-completions");
    });

    it("同一 provider 下两个 Slot 保留各自的 model API", () => {
      const credential = { api: "", apiKey: "sk-test", baseUrl: "https://test.example.com/v1" };
      const models = [
        { id: "util-model", provider: "test-provider", api: "openai-responses", _cred: credential },
        { id: "large-model", provider: "test-provider", api: "openai-completions", _cred: credential },
      ];
      const resolver = makeResolver(models, {
        summarize: { id: "util-model", provider: "test-provider" },
        memory: { id: "large-model", provider: "test-provider" },
      });

      const summarize = resolver.resolveAuxiliaryModel("summarize");
      const memory = resolver.resolveAuxiliaryModel("memory");

      expect(summarize.api).toBe("openai-responses");
      expect(memory.api).toBe("openai-completions");
    });

    it("Slot 模型携带 OAuth accountId，供请求归因使用", () => {
      const models = [
        {
          id: "gpt-5.4-codex",
          provider: "openai-codex-oauth",
          _cred: {
            api: "openai-codex-responses",
            apiKey: "oauth-token",
            baseUrl: "https://chatgpt.com/backend-api",
            accountId: "acct_123",
          },
        },
      ];
      const resolver = makeResolver(models, {
        summarize: { id: "gpt-5.4-codex", provider: "openai-codex-oauth" },
        memory: { id: "gpt-5.4-codex", provider: "openai-codex-oauth" },
      });

      const summarize = resolver.resolveAuxiliaryModel("summarize");
      const memory = resolver.resolveAuxiliaryModel("memory");

      expect(summarize.model).toMatchObject({ accountId: "acct_123" });
      expect(memory.model).toMatchObject({ accountId: "acct_123" });
    });

    it("provider 声明无须 key 时，远程 baseUrl 可不填 apiKey", () => {
      const models = [
        {
          id: "util-model",
          provider: "ollama",
          _allowMissingApiKey: true,
          _cred: {
            api: "openai-completions",
            apiKey: "",
            baseUrl: "http://192.168.1.20:11434/v1",
          },
        },
        {
          id: "large-model",
          provider: "ollama",
          _allowMissingApiKey: true,
          _cred: {
            api: "openai-completions",
            apiKey: "",
            baseUrl: "http://192.168.1.20:11434/v1",
          },
        },
      ];
      const resolver = makeResolver(models, {
        summarize: { id: "util-model", provider: "ollama" },
        memory: { id: "large-model", provider: "ollama" },
      });

      const summarize = resolver.resolveAuxiliaryModel("summarize");
      const memory = resolver.resolveAuxiliaryModel("memory");

      expect(summarize.model).toMatchObject({ id: "util-model", provider: "ollama" });
      expect(memory.model).toMatchObject({ id: "large-model", provider: "ollama" });
      expect(summarize.apiKey).toBe("");
      expect(summarize.baseUrl).toBe("http://192.168.1.20:11434/v1");
    });

    it("Slot 已配置但模型不在 availableModels 中时抛错（不 fallback）", () => {
      const models = [
        { id: "other-model", provider: "test-provider", _cred: { api: "openai-completions", apiKey: "sk-test", baseUrl: "https://test.example.com/v1" } },
      ];
      const resolver = makeResolver(models, {
        summarize: { id: "missing-model", provider: "test-provider" },
      });

      expect(() => resolver.resolveAuxiliaryModel("summarize"))
        .toThrow(/auxiliarySlotModelNotFound|modelNotFound|模型/);
    });

    it("Slot 未配置且有 chat fallback 时回退到 chat 模型", () => {
      const chatModel = { id: "chat-model", provider: "openai", api: "openai-responses" };
      const models = [
        {
          id: "chat-model",
          provider: "openai",
          api: "openai-responses",
          _cred: { api: "openai-responses", apiKey: "sk-chat", baseUrl: "https://chat.example/v1" },
        },
      ];
      // summarize 未配置 → fallback chat
      const resolver = makeResolver(models, {}, chatModel);

      const summarize = resolver.resolveAuxiliaryModel("summarize");

      expect(summarize.model).toMatchObject({ id: "chat-model", provider: "openai" });
    });

    it("approval/guard Slot 未配置时返回 null（不 fallback chat）", () => {
      const chatModel = { id: "chat-model", provider: "openai", api: "openai-responses" };
      const resolver = makeResolver([], {}, chatModel);

      expect(resolver.resolveAuxiliaryModel("approval")).toBeNull();
      expect(resolver.resolveAuxiliaryModel("guard")).toBeNull();
    });

    it("不再接受 hardcoded fallback 模型名", () => {
      const resolver = makeResolver([], {}, null);

      // approval Slot 未配置、无 chat fallback → null（绝不静默 fallback 到硬编码模型）
      expect(resolver.resolveAuxiliaryModel("approval")).toBeNull();
    });
  });
});
