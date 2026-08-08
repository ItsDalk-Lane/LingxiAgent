import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, it, expect, vi } from "vitest";
import { ConfigCoordinator } from "../core/config-coordinator.ts";

/** Match runtime normalizeWorkspacePath: backslash → forward slash for cross-platform persistence */
const n = (p: string) => p.replace(/\\/g, "/");

describe("updateConfig with agentId", () => {
  const tempRoots = [];

  afterEach(() => {
    for (const dir of tempRoots.splice(0)) {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  function makeTempDir(name) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-config-coord-"));
    tempRoots.push(root);
    const dir = path.join(root, name);
    fs.mkdirSync(dir, { recursive: true });
    return dir;
  }

  function makeDeps( overrides: any = {}) {
    const focusAgent: any = {
      id: "focus",
      config: { models: { chat: { id: "focus-chat", provider: "openai" } } },
      updateConfig: vi.fn(),
    };
    const targetAgent: any = {
      id: "target",
      config: { models: { chat: { id: "target-chat", provider: "deepseek" } } },
      updateConfig: vi.fn(),
    };
    return {
      focusAgent,
      targetAgent,
      deps: {
        lingxiHome: "/tmp/test",
        agentsDir: "/tmp/test/agents",
        getAgent: () => focusAgent,
        getAgentById: (id) => (id === "target" ? targetAgent : null),
        getActiveAgentId: () => "focus",
        getAgents: () => new Map([["focus", focusAgent], ["target", targetAgent]]),
        getModels: () => ({ availableModels: [], defaultModel: null }),
        getPrefs: () => ({ getPreferences: () => ({}), savePreferences: vi.fn() }),
        getSkills: () => ({ syncAgentSkills: vi.fn() }),
        getSession: () => null,
        getSessionCoordinator: () => null,
        getHub: () => null,
        emitEvent: vi.fn(),
        emitDevLog: vi.fn(),
        getCurrentModel: () => null,
        ...overrides,
      },
    };
  }

  it("returns only the requested agent explicit home folder", () => {
    const focusHome = makeTempDir("focus-home");
    const { focusAgent, targetAgent, deps } = makeDeps({
      getPrefs: () => ({
        getPrimaryAgent: () => "focus",
        getPreferences: () => ({ primaryAgent: "focus" }),
        savePreferences: vi.fn(),
      }),
    });
    focusAgent.config.desk = { home_folder: focusHome };
    targetAgent.config.desk = {};
    const coord = new ConfigCoordinator(deps);

    expect(coord.getExplicitHomeFolder("target")).toBeNull();
    expect(coord.getHomeFolder("target")).not.toBe(focusHome);
  });

  it("clears a requested agent explicit home folder only when the path is missing", () => {
    const missingHome = path.join(os.tmpdir(), `hana-missing-home-${Date.now()}`);
    const { targetAgent, deps } = makeDeps();
    targetAgent.config.desk = { home_folder: missingHome };
    const coord = new ConfigCoordinator(deps);

    expect(coord.getExplicitHomeFolder("target")).toBeNull();
    expect(targetAgent.updateConfig).toHaveBeenCalledWith({ desk: { home_folder: null } });
  });

  it("keeps a requested agent explicit home folder when stat reports temporary access failure", () => {
    const blockedHome = path.join(os.tmpdir(), `hana-blocked-home-${Date.now()}`);
    const originalStatSync = fs.statSync;
    const statSpy = vi.spyOn(fs, "statSync").mockImplementation((target, ...args) => {
      if (typeof target === "string" && n(path.normalize(target)) === n(path.normalize(blockedHome))) {
        throw Object.assign(new Error("permission denied"), { code: "EACCES" });
      }
      return originalStatSync.call(fs, target, ...args);
    });
    const { targetAgent, deps } = makeDeps();
    targetAgent.config.desk = { home_folder: blockedHome };
    const coord = new ConfigCoordinator(deps);

    expect(coord.getExplicitHomeFolder("target")).toBe(n(blockedHome));
    expect(statSpy).toHaveBeenCalledWith(n(blockedHome));
    expect(targetAgent.updateConfig).not.toHaveBeenCalled();
  });

  it("传入 agentId 时刷新目标 agent 而非焦点 agent", async () => {
    const { focusAgent, targetAgent, deps } = makeDeps();
    const coord = new ConfigCoordinator(deps);

    await coord.updateConfig({}, { agentId: "target" });

    expect(targetAgent.updateConfig).toHaveBeenCalledWith({});
    expect(focusAgent.updateConfig).not.toHaveBeenCalled();
  });

  it("显式刷新 description 时只把刷新意图传给目标 agent", async () => {
    const { focusAgent, targetAgent, deps } = makeDeps();
    const coord = new ConfigCoordinator(deps);

    await coord.updateConfig({}, { agentId: "target", refreshDescription: true });

    expect(targetAgent.updateConfig).toHaveBeenCalledWith({}, { refreshDescription: true });
    expect(focusAgent.updateConfig).not.toHaveBeenCalled();
  });

  it("不传 agentId 时刷新焦点 agent", async () => {
    const { focusAgent, targetAgent, deps } = makeDeps();
    const coord = new ConfigCoordinator(deps);

    await coord.updateConfig({});

    expect(focusAgent.updateConfig).toHaveBeenCalledWith({});
    expect(targetAgent.updateConfig).not.toHaveBeenCalled();
  });

  it("getSharedModels reads auxiliary slot preferences", () => {
    let prefs = {
      title_model: { id: "title-m", provider: "openai" },
      summarize_model: { id: "sum-m", provider: "openai" },
      memory_model: { id: "mem-m", provider: "openai" },
    };
    const { deps } = makeDeps({
      getPrefs: () => ({
        getPreferences: () => prefs,
        savePreferences: (next) => { prefs = { ...next }; },
      }),
    });
    const coord = new ConfigCoordinator(deps);

    const shared = coord.getSharedModels();
    expect(shared.title).toEqual({ id: "title-m", provider: "openai" });
    expect(shared.summarize).toEqual({ id: "sum-m", provider: "openai" });
    expect(shared.memory).toEqual({ id: "mem-m", provider: "openai" });
  });

  it("setSharedModels persists auxiliary slot model refs", () => {
    let prefs: any = {};
    const { deps } = makeDeps({
      getPrefs: () => ({
        getPreferences: () => prefs,
        savePreferences: (next) => { prefs = { ...next }; },
      }),
    });
    const coord = new ConfigCoordinator(deps);

    coord.setSharedModels({
      title: { id: "t1", provider: "p1" },
      approval: { id: "a1", provider: "p2" },
      guard: { id: "g1", provider: "p3" },
    });

    expect(prefs.title_model).toEqual({ id: "t1", provider: "p1" });
    expect(prefs.approval_model).toEqual({ id: "a1", provider: "p2" });
    expect(prefs.guard_model).toEqual({ id: "g1", provider: "p3" });
  });

  it("setSharedModels stores vision without mutating utility or memory runtime state", () => {
    let prefs = {
      vision_model: { id: "qwen-vl", provider: "dashscope" },
    };
    const { deps } = makeDeps({
      getPrefs: () => ({
        getPreferences: () => prefs,
        savePreferences: (next) => { prefs = { ...next }; },
      }),
    });
    const coord = new ConfigCoordinator(deps);

    coord.setSharedModels({
      vision: { id: "gpt-4o", provider: "openai" },
    });

    expect(prefs.vision_model).toEqual({ id: "gpt-4o", provider: "openai" });
  });

  it("getSharedModels exposes auxiliary vision as disabled by default", () => {
    const { deps } = makeDeps({
      getPrefs: () => ({
        getPreferences: () => ({}),
        savePreferences: vi.fn(),
      }),
    });
    const coord = new ConfigCoordinator(deps);

    expect(coord.getSharedModels()).toEqual(expect.objectContaining({
      vision_enabled: false,
    }));
  });

  it("setHeartbeatMaster only restarts agents that explicitly opted in", () => {
    let prefs: any = {};
    const focusHb = { start: vi.fn(), stop: vi.fn() };
    const targetHb = { start: vi.fn(), stop: vi.fn() };
    const { focusAgent, targetAgent, deps } = makeDeps({
      getPrefs: () => ({
        getPreferences: () => prefs,
        savePreferences: (next) => { prefs = { ...next }; },
      }),
      getHub: () => ({
        scheduler: {
          getHeartbeat: (agentId) => (agentId === "focus" ? focusHb : targetHb),
        },
      }),
    });
    focusAgent.config.desk = {};
    targetAgent.config.desk = { heartbeat_enabled: true };
    const coord = new ConfigCoordinator(deps);

    coord.setHeartbeatMaster(true);

    expect(focusHb.start).not.toHaveBeenCalled();
    expect(targetHb.start).toHaveBeenCalledOnce();
  });

  it("setSharedModels stores and clears auxiliary vision without mutating utility or memory runtime state", () => {
    let prefs: any = {};
    const { deps } = makeDeps({
      getPrefs: () => ({
        getPreferences: () => prefs,
        savePreferences: (next) => { prefs = { ...next }; },
      }),
    });
    const coord = new ConfigCoordinator(deps);

    coord.setSharedModels({ vision_enabled: true });
    expect(prefs.vision_auxiliary_enabled).toBe(true);
    expect(coord.getSharedModels()).toEqual(expect.objectContaining({
      vision_enabled: true,
    }));

    coord.setSharedModels({ vision_enabled: false });
    expect(prefs).not.toHaveProperty("vision_auxiliary_enabled");
    expect(coord.getSharedModels()).toEqual(expect.objectContaining({
      vision_enabled: false,
    }));
  });

  it("agentId 等于焦点 agent 时，模型切换逻辑正常执行", async () => {
    const models = {
      availableModels: [{ id: "gpt-4", provider: "openai", name: "GPT-4" }],
      defaultModel: null,
    };
    const { focusAgent, deps } = makeDeps({
      getModels: () => models,
      getActiveAgentId: () => "focus",
    });
    const coord = new ConfigCoordinator(deps);

    await coord.updateConfig(
      { models: { chat: { id: "gpt-4", provider: "openai" } } },
      { agentId: "focus" },
    );

    expect(focusAgent.updateConfig).toHaveBeenCalled();
    // defaultModel 应被设置（findModel 会找到 gpt-4）
    expect(models.defaultModel).toEqual({ id: "gpt-4", provider: "openai", name: "GPT-4" });
  });

  it("agentId 为非焦点 agent 时，不执行模型切换", async () => {
    const models = {
      availableModels: [{ id: "gpt-4", provider: "openai", name: "GPT-4" }],
      defaultModel: null,
    };
    const { targetAgent, deps } = makeDeps({
      getModels: () => models,
    });
    const coord = new ConfigCoordinator(deps);

    await coord.updateConfig(
      { models: { chat: { id: "gpt-4", provider: "openai" } } },
      { agentId: "target" },
    );

    expect(targetAgent.updateConfig).toHaveBeenCalled();
    // defaultModel 不应被设置（非焦点 agent 不做模型切换）
    expect(models.defaultModel).toBeNull();
  });

  it("setDefaultModel 传入非焦点 agentId 时，只更新目标 agent 配置", async () => {
    const models = {
      availableModels: [{ id: "gpt-4", provider: "openai", name: "GPT-4" }],
      defaultModel: null,
    };
    const { focusAgent, targetAgent, deps } = makeDeps({
      getModels: () => models,
    });
    const coord = new ConfigCoordinator(deps);

    const result = await coord.setDefaultModel("gpt-4", "openai", { agentId: "target" });

    expect(result).toEqual({ id: "gpt-4", provider: "openai", name: "GPT-4" });
    expect(targetAgent.updateConfig).toHaveBeenCalledWith({
      models: { chat: { id: "gpt-4", provider: "openai" } },
    });
    expect(focusAgent.updateConfig).not.toHaveBeenCalled();
    expect(models.defaultModel).toBeNull();
  });

  it("setDefaultModel 不传 agentId 时，保持焦点 agent 语义", async () => {
    const models = {
      availableModels: [{ id: "gpt-4", provider: "openai", name: "GPT-4" }],
      defaultModel: null,
    };
    const { focusAgent, targetAgent, deps } = makeDeps({
      getModels: () => models,
      getActiveAgentId: () => "focus",
    });
    const coord = new ConfigCoordinator(deps);

    const result = await coord.setDefaultModel("gpt-4", "openai");

    expect(result).toEqual({ id: "gpt-4", provider: "openai", name: "GPT-4" });
    expect(focusAgent.updateConfig).toHaveBeenCalledWith({
      models: { chat: { id: "gpt-4", provider: "openai" } },
    });
    expect(targetAgent.updateConfig).not.toHaveBeenCalled();
    expect(models.defaultModel).toEqual({ id: "gpt-4", provider: "openai", name: "GPT-4" });
  });

  /**
   * 焦点会话固定为 focused.jsonl，与被显式要求持久化的会话区分开。
   * persistSessionMeta 曾经读焦点指针，所以这个夹具能把"写错会话"照出来。
   */
  function makePersistMetaCoord() {
    const focusAgent = {
      id: "focus",
      memoryEnabled: true,
      sessionMemoryEnabled: true,
    };
    const writeSessionMeta = vi.fn();
    const getSessionMemoryEnabled = vi.fn(() => false);
    const coord = new ConfigCoordinator({
      lingxiHome: "/tmp/test",
      agentsDir: "/tmp/test/agents",
      getAgent: () => focusAgent,
      getAgentById: () => null,
      getActiveAgentId: () => "focus",
      getAgents: () => new Map([["focus", focusAgent]]),
      getModels: () => ({ availableModels: [], defaultModel: null }),
      getPrefs: () => ({ getPreferences: () => ({}), savePreferences: vi.fn() }),
      getSkills: () => ({ syncAgentSkills: vi.fn() }),
      getSession: () => ({
        sessionManager: {
          getSessionFile: () => "/tmp/test/agents/focus/sessions/focused.jsonl",
        },
      }),
      getSessionCoordinator: () => ({ getSessionMemoryEnabled, writeSessionMeta }),
      getHub: () => null,
      emitEvent: vi.fn(),
      emitDevLog: vi.fn(),
      getCurrentModel: () => null,
    });
    return { coord, writeSessionMeta, getSessionMemoryEnabled };
  }

  it("persistSessionMeta writes the path-scoped session memory flag", async () => {
    const { coord, writeSessionMeta, getSessionMemoryEnabled } = makePersistMetaCoord();

    await coord.persistSessionMeta("/tmp/test/agents/focus/sessions/frozen.jsonl");

    expect(getSessionMemoryEnabled).toHaveBeenCalledWith("/tmp/test/agents/focus/sessions/frozen.jsonl");
    expect(writeSessionMeta).toHaveBeenCalledWith(
      "/tmp/test/agents/focus/sessions/frozen.jsonl",
      { memoryEnabled: false },
    );
  });

  /**
   * 回归钉子：detached 创建会在 finally 里把焦点还给上一个会话，
   * 所以"读焦点"的旧实现会把新会话的记忆开关写到上一个会话头上。
   */
  it("persistSessionMeta writes the requested session, not the focused one", async () => {
    const { coord, writeSessionMeta, getSessionMemoryEnabled } = makePersistMetaCoord();

    await coord.persistSessionMeta("/tmp/test/agents/focus/sessions/detached-new.jsonl");

    expect(getSessionMemoryEnabled).toHaveBeenCalledWith("/tmp/test/agents/focus/sessions/detached-new.jsonl");
    expect(writeSessionMeta).toHaveBeenCalledTimes(1);
    expect(writeSessionMeta.mock.calls[0][0]).toBe("/tmp/test/agents/focus/sessions/detached-new.jsonl");
    expect(writeSessionMeta).not.toHaveBeenCalledWith(
      "/tmp/test/agents/focus/sessions/focused.jsonl",
      expect.anything(),
    );
  });

  it("persistSessionMeta throws when no session path is supplied", async () => {
    const { coord, writeSessionMeta } = makePersistMetaCoord();

    // 显式绕过类型检查：模拟未走类型约束的运行时调用方漏传 path，
    // 这时必须直接抛，不许退回读焦点。
    expect(() => (coord as any).persistSessionMeta()).toThrow(/sessionPath is required/);
    expect(() => coord.persistSessionMeta(undefined)).toThrow(/sessionPath is required/);
    expect(writeSessionMeta).not.toHaveBeenCalled();
  });

  // 这里曾经有一条 ConfigCoordinator.setMemoryEnabled 的用例，用来防止它写错
  // session。那个方法本身从"当前焦点会话"反推写入目标，违反状态归属只能显式
  // 传递的底线，而且生产路径零调用方，已随实现一起删除，所以这条用例的保护
  // 对象也不存在了。别从 git 历史里把它原样抄回来：会话进行中切记忆开关的正确
  // 形状是调用方显式传 sessionPath 调 sessionCoord.setSessionMemoryEnabled，
  // 那种实现要配的是一条针对显式入参的用例。
});
