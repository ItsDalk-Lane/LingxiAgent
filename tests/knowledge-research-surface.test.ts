import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

const { createAgentSessionMock } = vi.hoisted(() => ({ createAgentSessionMock: vi.fn() }));
// 本文件替换 SDK 会话，验证装配参数与生命周期；真实组装见 knowledge-research-runtime-assembly.test.ts。
vi.mock("../lib/pi-sdk/index.ts", async importOriginal => ({
  ...await importOriginal<Record<string, unknown>>(), createAgentSession: createAgentSessionMock,
}));
import { SessionCoordinator } from "../core/session-coordinator.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { SessionManager } from "../lib/pi-sdk/index.ts";
import { getKnowledgeResearchToolNames } from "../shared/tool-categories.ts";

const fixtures: Array<{ close: () => void }> = [];
afterEach(() => { vi.restoreAllMocks(); createAgentSessionMock.mockReset(); for (const fixture of fixtures.splice(0)) fixture.close(); });

function setup() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-research-surface-"));
  const manifests = new SessionManifestStore({ dbPath: path.join(directory, "manifests.db") });
  const tools = [...getKnowledgeResearchToolNames("knowledge_research_root"), ...getKnowledgeResearchToolNames("knowledge_completeness_worker"), "knowledge_manage", "write", "edit",
    "exec_command", "browser", "web_search", "subagent", "search_memory", "workflow", "session"].map(name => ({
    name, execute: vi.fn(async () => ({ content: [{ type: "text", text: name }] })),
  }));
  function agent(id: string) {
    const agentDir = path.join(directory, "agents", id), sessionDir = path.join(agentDir, "sessions");
    fs.mkdirSync(sessionDir, { recursive: true });
    return { id, agentName: id, agentDir, sessionDir, memoryMasterEnabled: true, experienceEnabled: true,
      config: { models: { chat: { provider: "test", id: `${id}-chat` } }, desk: { patrol_tools: "*" } },
      getToolsSnapshot: vi.fn((_options: Record<string, unknown>) => tools), buildSystemPrompt: vi.fn((_options: Record<string, unknown>) => "研究系统提示"),
      systemPrompt: "不应该继承的长期记忆", _memoryTicker: { notifyPromoted: vi.fn() } };
  }
  const owner = agent("owner"), other = agent("other"), agents = new Map([[owner.id, owner], [other.id, other]]);
  const mainPath = path.join(owner.sessionDir, "main.jsonl");
  fs.writeFileSync(mainPath, `${JSON.stringify({ type: "session", version: 3, id: "main", cwd: directory })}\n`);
  const main = manifests.createForPath({ sessionPath: mainPath, ownerAgentId: owner.id, domain: "desktop", kind: "chat",
    lifecycle: "active", provenance: { studioId: "studio-a" } });
  const models = { defaultModel: { id: "global-default", provider: "test" }, currentModel: { id: "focused-chat", provider: "test" },
    availableModels: [{ id: "owner-chat", provider: "test" }, { id: "other-chat", provider: "test" }],
    resolveExecutionModel: vi.fn(model => model), resolveThinkingLevel: (level: string) => level,
    modelRuntime: {}, authStorage: {}, modelRegistry: {} };
  const buildTools = vi.fn((_cwd, snapshot, options) => ({
    tools: [{ name: "read" }, { name: "write" }, { name: "exec_command" }], customTools: [...snapshot, ...options.extraCustomTools],
  }));
  const resourceLoader = { getSystemPrompt: () => "原始系统提示", getAppendSystemPrompt: () => ["额外工作区说明"],
    getExtensions: () => ({ extensions: [{ tools: [{ name: "write" }] }], errors: [] }),
    getSkills: () => ({ skills: [{ name: "write-skill" }], diagnostics: [] }), getAgentsFiles: () => ({ agentsFiles: ["AGENTS.md"] }) };
  const coordinator = new SessionCoordinator({ agentsDir: path.join(directory, "agents"), getAgent: () => other,
    getActiveAgentId: () => other.id, getAgentById: id => agents.get(id), getAgents: () => agents, listAgents: () => [...agents.values()],
    ensureAgentRuntime: async id => agents.get(id), getModels: () => models, getResourceLoader: () => resourceLoader,
    getSkills: () => ({ getSkillsForAgent: () => resourceLoader.getSkills() }), buildTools,
    getHomeCwd: () => directory, getConfig: () => ({}), getPrefs: () => ({ getThinkingLevel: () => "medium" }),
    emitEvent: vi.fn(), emitDevLog: vi.fn(), agentIdFromSessionPath: () => owner.id,
    switchAgentOnly: async () => {}, getActivityStore: () => null, sessionManifestStore: manifests });
  const lifecycle: Array<{ phase: string; exists: boolean; sessionPath: string }> = [];
  const sessions: any[] = [];
  const behavior: { prompt?: (options: any, session: any) => Promise<void>; shutdown?: () => Promise<void>; failAssembly?: boolean; failBeforeReady?: boolean } = {};
  createAgentSessionMock.mockImplementation(async options => {
    const manager = options.sessionManager;
    // 使用真实 SDK 会话管理器写出临时文件，隐藏思考只在此临时会话中出现。
    manager.appendMessage({ role: "assistant", content: [{ type: "thinking", thinking: "临时隐藏思考" }],
      stopReason: "toolUse", timestamp: Date.now(), provider: "test", model: options.model.id, api: "test" });
    const listeners = new Set<(event: any) => void>();
    let aborted = false;
    const record = (phase: string) => lifecycle.push({ phase, exists: fs.existsSync(manager.getSessionFile()), sessionPath: manager.getSessionFile() });
    const session: any = { sessionManager: manager, model: options.model,
      settingsManager: behavior.failBeforeReady ? {} : undefined,
      subscribe: (listener: (event: any) => void) => {
        if (behavior.failAssembly) throw new Error("订阅初始化失败");
        listeners.add(listener); return () => { record("unsubscribe"); listeners.delete(listener); };
      },
      extensionRunner: { hasHandlers: (name: string) => name === "session_shutdown", emit: async () => {
        record("shutdown_start"); await behavior.shutdown?.(); record("shutdown_end");
      } },
      dispose: vi.fn(() => record("dispose")), abort: vi.fn(async () => { aborted = true; }),
      prompt: async () => {
        await behavior.prompt?.(options, session);
        for (const listener of listeners) listener({ type: "message_end", message: { role: "assistant", stopReason: aborted ? "aborted" : "stop",
          content: [{ type: "text", text: "研究结束" }] } });
      },
    };
    sessions.push(session); return { session };
  });
  const research = { runId: "run-a", scopeId: "scope-a", studioId: "studio-a" };
  const rootOptions = { surface: "knowledge_research_root", research, parentSessionPath: mainPath, parentSessionId: main.sessionId };
  function rootParent(overrides: Record<string, unknown> = {}) {
    const sessionPath = path.join(owner.agentDir, ".ephemeral", "active-root.jsonl");
    fs.mkdirSync(path.dirname(sessionPath), { recursive: true });
    fs.writeFileSync(sessionPath, "临时根会话");
    const manifest = manifests.createForPath({ sessionPath, ownerAgentId: owner.id, domain: "subagent", kind: "knowledge_research_root",
      lifecycle: "active", provenance: { parentSessionId: main.sessionId, studioId: "studio-a",
        researchContext: { runId: "run-a", scopeId: "scope-a", role: "root" }, ...overrides } });
    return { sessionPath, manifest };
  }
  const fixture = { directory, manifests, main, mainPath, owner, other, models, tools, coordinator, buildTools,
    research, rootOptions, rootParent, lifecycle, sessions, behavior,
    close: () => { manifests.close(); fs.rmSync(directory, { recursive: true, force: true }); } };
  fixtures.push(fixture); return fixture;
}

describe("研究隔离会话装配参数与生命周期", () => {
  it("完整性Worker仅装配两个专用工具，真实Root父身份和分片写入登记库，权限被固定为只读", async () => {
    const f = setup(), parent = f.rootParent();
    const completeness = { internalText: "禁止写入登记库的宿主正文" };
    f.behavior.prompt = async options => {
      expect(options.tools).toEqual([]);
      expect(options.customTools.map(tool => tool.name)).toEqual(["knowledge_coverage_read", "knowledge_completeness_mark"]);
      await options.customTools[0].execute();
      expect(options.resourceLoader.getExtensions()).toEqual({ extensions: [], errors: [],
        runtime: expect.objectContaining({ sendMessage: expect.any(Function) }) });
      expect(options.resourceLoader.getSkills()).toEqual({ skills: [], diagnostics: [] });
      expect(options.resourceLoader.getAgentsFiles()).toEqual({ agentsFiles: [] });
    };
    const result = await f.coordinator.executeIsolated("检查全部原文", { ...f.rootOptions, surface: "knowledge_completeness_worker",
      parentSessionPath: parent.sessionPath, parentSessionId: parent.manifest.sessionId,
      research: { ...f.research, completenessCheckId: "check-a", completenessShardId: "shard-a", completeness,
        allowedNeedIds: ["need-a"], allowedSourceIds: ["source-a"] },
      permissionMode: "operate", approvalPolicy: "interactive", allowHumanApproval: true,
      toolFilter: "*", builtinFilter: ["write"], extraCustomTools: [{ name: "knowledge_delegate" }],
      workspaceFolders: ["/write"], authorizedFolders: ["/outside"], fileReadSessionPaths: [f.mainPath] });
    expect(result).toMatchObject({ sessionPath: null, error: null });
    const assembled = createAgentSessionMock.mock.calls[0][0], snapshot = f.owner.getToolsSnapshot.mock.calls[0][0] as any;
    const manifest = f.manifests.resolveByLocatorPath(assembled.sessionManager.getSessionFile());
    expect(snapshot.research.completeness).toBe(completeness);
    expect(snapshot.research.actorContext).toMatchObject({ role: "worker", completenessCheckId: "check-a", completenessShardId: "shard-a",
      actorSessionId: manifest.sessionId, actorAgentId: "owner" });
    expect(manifest).toMatchObject({ kind: "knowledge_completeness_worker", lifecycle: "deleted",
      permissionModeSnapshot: { mode: "read_only" }, memoryPolicy: { mode: "disabled" },
      provenance: { parentSessionId: parent.manifest.sessionId, researchContext: { runId: "run-a", scopeId: "scope-a", role: "worker",
        completenessCheckId: "check-a", completenessShardId: "shard-a", allowedNeedIds: ["need-a"], allowedSourceIds: ["source-a"] } } });
    expect(JSON.stringify(manifest)).not.toContain(completeness.internalText);
    const built = f.buildTools.mock.calls[0][2];
    expect(built).toMatchObject({ approvalPolicy: "deny_on_prompt", allowHumanApproval: false, workspaceFolders: [], authorizedFolders: [],
      fileReadSessionPaths: [], extraCustomTools: [], permissionContext: { knowledgeResearchSurface: "knowledge_completeness_worker" } });
    expect(built.getPermissionMode()).toBe("read_only");
    expect(f.lifecycle.map(item => item.phase)).toEqual(["shutdown_start", "shutdown_end", "unsubscribe", "dispose"]);
    expect(f.lifecycle.every(item => item.exists)).toBe(true);
    expect(fs.existsSync(assembled.sessionManager.getSessionFile())).toBe(false);
  });

  it("完整性分配缺字段、直挂main、跨研究和普通会话夹带分片均在模型装配前拒绝", async () => {
    const f = setup(), parent = f.rootParent();
    const research = { ...f.research, completenessCheckId: "check-a", completenessShardId: "shard-a", completeness: {},
      allowedNeedIds: ["need-a"], allowedSourceIds: ["source-a"] };
    const worker = { ...f.rootOptions, surface: "knowledge_completeness_worker", research,
      parentSessionPath: parent.sessionPath, parentSessionId: parent.manifest.sessionId };
    for (const options of [
      ...["completenessCheckId", "completenessShardId", "completeness"].map(key => ({ ...worker, research: { ...research, [key]: undefined } })),
      { ...worker, parentSessionPath: f.mainPath, parentSessionId: f.main.sessionId },
      { ...worker, research: { ...research, runId: "another-run" } },
      { ...worker, research: { ...research, scopeId: "another-scope" } },
      { ...worker, research: { ...research, studioId: "another-studio" } },
      { ...worker, surface: "knowledge_research_worker" },
      { ...f.rootOptions, research },
    ]) await expect(f.coordinator.executeIsolated("检查原文", options)).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(createAgentSessionMock).not.toHaveBeenCalled();
    expect(f.owner.getToolsSnapshot).not.toHaveBeenCalled();
  });

  it.each(["knowledge_research_worker", "knowledge_completeness_worker"])("%s不能作为新完整性Worker的父会话", async kind => {
    const f = setup(), parent = f.rootParent();
    f.manifests.db.prepare("UPDATE session_manifests SET kind=?,provenance_json=? WHERE session_id=?").run(kind,
      JSON.stringify({ ...parent.manifest.provenance, researchContext: { runId: "run-a", scopeId: "scope-a", role: "worker",
        allowedNeedIds: ["need-a"], ...(kind === "knowledge_completeness_worker" ? { completenessCheckId: "check-a", completenessShardId: "shard-a" } : {}) } }),
      parent.manifest.sessionId);
    await expect(f.coordinator.executeIsolated("不得再次委派", { ...f.rootOptions, surface: "knowledge_completeness_worker",
      parentSessionPath: parent.sessionPath, parentSessionId: parent.manifest.sessionId,
      research: { ...f.research, allowedNeedIds: ["need-a"], allowedSourceIds: ["source-a"],
        completenessCheckId: "check-a", completenessShardId: "shard-b", completeness: {} } }))
      .rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    expect(createAgentSessionMock).not.toHaveBeenCalled();
  });

  it("Root固定七工具、只读权限与无记忆，使用父Agent聊天模型并绑定真实会话身份", async () => {
    const f = setup(), onFinishAccepted = vi.fn(), isCompletenessSatisfied = vi.fn();
    f.behavior.prompt = async options => {
      expect(options.tools).toEqual([]);
      expect(options.customTools.map(tool => tool.name)).toEqual(getKnowledgeResearchToolNames("knowledge_research_root"));
      await options.customTools.find(tool => tool.name === "knowledge_search").execute();
      expect(options.resourceLoader.getSystemPrompt()).toBe("研究系统提示");
      expect(options.resourceLoader.getAppendSystemPrompt()).toEqual([]);
      expect(options.resourceLoader.getExtensions()).toEqual({ extensions: [], errors: [],
        runtime: expect.objectContaining({ sendMessage: expect.any(Function) }) });
      expect(options.resourceLoader.getSkills()).toEqual({ skills: [], diagnostics: [] });
      expect(options.resourceLoader.getAgentsFiles()).toEqual({ agentsFiles: [] });
    };
    const result = await f.coordinator.executeIsolated("研究问题", { ...f.rootOptions,
      research: { ...f.research, onFinishAccepted, isCompletenessSatisfied, prompt: "禁止写入登记库的提示" },
      permissionMode: "operate", approvalPolicy: "interactive", allowHumanApproval: true, subagentContext: false,
      toolFilter: "*", builtinFilter: ["write"], extraCustomTools: [{ name: "injected" }],
      workspaceFolders: ["/write"], authorizedFolders: ["/outside"], fileReadSessionPaths: [f.mainPath] });
    expect(result).toMatchObject({ sessionPath: null, error: null, replyText: "研究结束" });
    const assembled = createAgentSessionMock.mock.calls[0][0];
    expect(assembled.model.id).toBe("owner-chat");
    expect(f.other.getToolsSnapshot).not.toHaveBeenCalled();
    expect(f.owner.buildSystemPrompt).toHaveBeenCalledWith({ forSubagent: true });
    const snapshot = f.owner.getToolsSnapshot.mock.calls[0][0] as any;
    expect(snapshot).toMatchObject({ surface: "knowledge_research_root", forceMemoryEnabled: false, forceExperienceEnabled: false });
    expect(snapshot.research.onFinishAccepted).toBe(onFinishAccepted);
    expect(snapshot.research.isCompletenessSatisfied).toBe(isCompletenessSatisfied);
    const manifest = f.manifests.resolveByLocatorPath(assembled.sessionManager.getSessionFile());
    expect(snapshot.research.actorContext).toMatchObject({ runId: "run-a", scopeId: "scope-a", role: "root",
      actorSessionId: manifest.sessionId, actorAgentId: "owner" });
    expect(snapshot.research.sessionPath).toBe(assembled.sessionManager.getSessionFile());
    expect(manifest).toMatchObject({ lifecycle: "deleted", kind: "knowledge_research_root", memoryPolicy: { mode: "disabled" },
      permissionModeSnapshot: { mode: "read_only" }, workspaceScope: { primaryCwd: null, workspaceFolders: [], authorizedFolders: [] },
      provenance: { parentSessionId: f.main.sessionId, studioId: "studio-a", researchContext: { runId: "run-a", scopeId: "scope-a", role: "root" } } });
    expect(Object.keys(manifest.provenance)).toEqual(["createdBy", "studioId", "researchContext", "parentSessionId"]);
    expect(JSON.stringify(manifest)).not.toMatch(/隐藏思考|禁止写入登记库的提示|研究问题/);
    expect(f.buildTools.mock.calls[0][2]).toMatchObject({ workspace: null, workspaceFolders: [], authorizedFolders: [],
      fileReadSessionPaths: [], extraCustomTools: [], allowHumanApproval: false, approvalPolicy: "deny_on_prompt",
      permissionContext: { isSubagent: true, knowledgeResearchSurface: "knowledge_research_root" } });
    expect(f.buildTools.mock.calls[0][2].getPermissionMode()).toBe("read_only");
    expect(f.tools.find(tool => tool.name === "knowledge_search")!.execute).toHaveBeenCalledOnce();
    expect(f.lifecycle.map(item => item.phase)).toEqual(["shutdown_start", "shutdown_end", "unsubscribe", "dispose"]);
    expect(f.lifecycle.every(item => item.exists)).toBe(true);
    expect(fs.existsSync(assembled.sessionManager.getSessionFile())).toBe(false);
    expect(fs.existsSync(path.join(f.owner.sessionDir, "session-meta.json"))).toBe(false);
  });

  it.each([undefined, "other"])("Worker经Root挂接main，只有五个工具且使用对应Agent聊天模型：%s", async agentId => {
    const f = setup(), parent = f.rootParent();
    f.behavior.prompt = async options => {
      expect(options.tools).toEqual([]);
      expect(options.customTools.map(tool => tool.name)).toEqual(getKnowledgeResearchToolNames("knowledge_research_worker"));
      await options.customTools.find(tool => tool.name === "knowledge_read").execute();
    };
    const result = await f.coordinator.executeIsolated("工作分配", { ...f.rootOptions, surface: "knowledge_research_worker", agentId,
      toolFilter: "*", builtinFilter: ["write"], extraCustomTools: [{ name: "write" }, { name: "knowledge_delegate" }], permissionMode: "operate",
      parentSessionPath: parent.sessionPath, parentSessionId: parent.manifest.sessionId,
      research: { ...f.research, allowedNeedIds: ["need-a"], allowedSourceIds: ["source-a"] } });
    expect(result.error).toBeNull();
    const options = createAgentSessionMock.mock.calls[0][0];
    expect(options.model.id).toBe(`${agentId || "owner"}-chat`);
    const manifest = f.manifests.resolveByLocatorPath(options.sessionManager.getSessionFile());
    expect(manifest.provenance).toMatchObject({ parentSessionId: parent.manifest.sessionId,
      researchContext: { role: "worker", allowedNeedIds: ["need-a"], allowedSourceIds: ["source-a"] } });
    expect(parent.manifest.provenance.parentSessionId).toBe(f.main.sessionId);
    expect(f.tools.find(tool => tool.name === "knowledge_read")!.execute).toHaveBeenCalledOnce();
    expect(f.tools.find(tool => tool.name === "knowledge_delegate")!.execute).not.toHaveBeenCalled();
  });

  it("不能持久化、恢复旧会话、替换模型或通过错误surface逃离固定装配", async () => {
    const f = setup(), open = vi.spyOn(SessionManager, "open");
    for (const changed of [{ persist: f.owner.sessionDir }, { resumeSessionPath: f.mainPath }, { model: f.models.defaultModel },
      { surface: "knowledge_research_root_extra" }]) {
      await expect(f.coordinator.executeIsolated("问题", { ...f.rootOptions, ...changed })).rejects.toMatchObject({ code: "KNOWLEDGE_INVALID_ARGUMENT" });
    }
    expect(open).not.toHaveBeenCalled();
    expect(createAgentSessionMock).not.toHaveBeenCalled();
  });

  it("父身份缺失、跨studio、Root换Agent、Worker直连main和跨run分配全部拒绝", async () => {
    const f = setup(), parent = f.rootParent();
    const worker = { ...f.rootOptions, surface: "knowledge_research_worker", research: { ...f.research, allowedNeedIds: ["need-a"] },
      parentSessionPath: parent.sessionPath, parentSessionId: parent.manifest.sessionId };
    for (const options of [{ ...f.rootOptions, parentSessionPath: path.join(f.directory, "missing.jsonl") },
      { ...f.rootOptions, parentSessionId: "fake-id" }, { ...f.rootOptions, research: { ...f.research, studioId: "studio-b" } },
      { ...f.rootOptions, agentId: "other" }, { ...worker, parentSessionPath: f.mainPath, parentSessionId: f.main.sessionId },
      { ...worker, research: { ...worker.research, runId: "other-run" } }, { ...worker, research: f.research },
      { ...f.rootOptions, parentSessionPath: parent.sessionPath, parentSessionId: parent.manifest.sessionId }]) {
      await expect(f.coordinator.executeIsolated("问题", options)).rejects.toMatchObject({ code: "KNOWLEDGE_SCOPE_VIOLATION" });
    }
    expect(createAgentSessionMock).not.toHaveBeenCalled();
  });

  it("Agent模型缺失时明确失败，不使用全局默认或辅助模型", async () => {
    const f = setup(); f.models.availableModels = [];
    const result = await f.coordinator.executeIsolated("问题", f.rootOptions);
    expect(result.error).toMatch(/Agent chat model is unavailable/);
    expect(createAgentSessionMock).not.toHaveBeenCalled();
    expect(f.models.resolveExecutionModel).not.toHaveBeenCalled();
  });

  it("正常结束必须等待真实shutdown完成才能dispose和删除临时文件", async () => {
    const f = setup(); let release!: () => void, entered!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; }), started = new Promise<void>(resolve => { entered = resolve; });
    f.behavior.shutdown = async () => { entered(); await gate; };
    let settled = false;
    const pending = f.coordinator.executeIsolated("问题", f.rootOptions).then(result => { settled = true; return result; });
    await started;
    const session = f.sessions[0], temporaryPath = session.sessionManager.getSessionFile();
    expect(fs.existsSync(temporaryPath)).toBe(true);
    expect(settled).toBe(false);
    expect(session.dispose).not.toHaveBeenCalled();
    release(); await pending;
    expect(session.dispose).toHaveBeenCalledOnce();
    expect(fs.existsSync(temporaryPath)).toBe(false);
  });

  it.each(["prompt", "assembly"])("%s抛错时仍先完整释放运行资源再删除临时文件", async failure => {
    const f = setup();
    if (failure === "prompt") f.behavior.prompt = async () => { throw new Error("研究执行失败"); };
    else f.behavior.failAssembly = true;
    const result = await f.coordinator.executeIsolated("问题", f.rootOptions);
    expect(result.error).toMatch(/失败/);
    expect(f.sessions[0].dispose).toHaveBeenCalledOnce();
    expect(f.lifecycle.every(item => item.exists)).toBe(true);
    expect(fs.existsSync(f.sessions[0].sessionManager.getSessionFile())).toBe(false);
    expect(f.manifests.resolveByLocatorPath(f.sessions[0].sessionManager.getSessionFile()).lifecycle).toBe("deleted");
  });

  it.each(["ready", "prompt"])("%s时取消研究，同样先释放资源再删除临时文件", async phase => {
    const f = setup(), controller = new AbortController();
    if (phase === "prompt") f.behavior.prompt = async () => { controller.abort(); };
    const result = await f.coordinator.executeIsolated("问题", { ...f.rootOptions, signal: controller.signal,
      onSessionReady: () => { if (phase === "ready") controller.abort(); } });
    expect(result.error).toBe("aborted");
    expect(f.sessions[0].dispose).toHaveBeenCalledOnce();
    expect(f.lifecycle.map(item => item.phase)).toEqual(["shutdown_start", "shutdown_end", "unsubscribe", "dispose"]);
    expect(f.lifecycle.every(item => item.exists)).toBe(true);
    expect(fs.existsSync(f.sessions[0].sessionManager.getSessionFile())).toBe(false);
    expect(f.manifests.resolveByLocatorPath(f.sessions[0].sessionManager.getSessionFile()).lifecycle).toBe("deleted");
  });

  it("初始化回滚删除真实文件遇到一次权限错误时明确抛出，不能静默遗留临时思考", async () => {
    const f = setup(); f.behavior.failBeforeReady = true;
    const unlink = fs.unlinkSync.bind(fs), failure = Object.assign(new Error("研究临时文件删除被拒绝"), { code: "EACCES" });
    let failures = 0;
    vi.spyOn(fs, "unlinkSync").mockImplementation(file => {
      if (typeof file === "string" && file.startsWith(path.join(f.owner.agentDir, ".ephemeral")) && failures === 0) {
        failures++; throw failure;
      }
      unlink(file);
    });
    await expect(f.coordinator.executeIsolated("问题", f.rootOptions)).rejects.toBe(failure);
    const temporaryPath = f.sessions[0].sessionManager.getSessionFile();
    expect(failures).toBe(1);
    expect(f.sessions[0].dispose).toHaveBeenCalledOnce();
    expect(f.lifecycle.every(item => item.exists)).toBe(true);
    expect(fs.readFileSync(temporaryPath, "utf8")).toContain("临时隐藏思考");
    expect(f.manifests.resolveByLocatorPath(temporaryPath).lifecycle).toBe("deleted");
  });

  it("初始化回滚无法登记删除时明确抛出并保留原登记状态", async () => {
    const f = setup(); f.behavior.failBeforeReady = true;
    vi.spyOn(f.manifests, "updateLocatorLifecycle").mockImplementationOnce(() => { throw new Error("登记库暂不可写"); });
    await expect(f.coordinator.executeIsolated("问题", f.rootOptions)).rejects.toThrow("Research temporary session manifest cleanup failed");
    const temporaryPath = f.sessions[0].sessionManager.getSessionFile();
    expect(f.sessions[0].dispose).toHaveBeenCalledOnce();
    expect(fs.existsSync(temporaryPath)).toBe(true);
    expect(f.manifests.resolveByLocatorPath(temporaryPath).lifecycle).toBe("active");
  });

  it("执行中的研究会话不能被普通活动提升入口搬入正式会话", async () => {
    const f = setup();
    f.behavior.prompt = async options => {
      const file = options.sessionManager.getSessionFile();
      expect(await f.coordinator.promoteActivitySession(path.join("..", ".ephemeral", path.basename(file)), "owner")).toBeNull();
      expect(fs.existsSync(file)).toBe(true);
      expect(f.owner._memoryTicker.notifyPromoted).not.toHaveBeenCalled();
    };
    expect((await f.coordinator.executeIsolated("问题", f.rootOptions)).error).toBeNull();
  });
});
