import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ModelRuntime } from "@earendil-works/pi-coding-agent";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import * as sdk from "../lib/pi-sdk/index.ts";
import { SessionCoordinator } from "../core/session-coordinator.ts";
import { SessionManifestStore } from "../core/session-manifest/store.ts";
import { getKnowledgeResearchToolNames } from "../shared/tool-categories.ts";

const surfaces = ["knowledge_research_root", "knowledge_research_worker", "knowledge_completeness_worker"] as const;
const fixtures: Array<{ close: () => void }> = [];
afterEach(() => { vi.restoreAllMocks(); for (const fixture of fixtures.splice(0)) fixture.close(); });

async function setup() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-runtime-assembly-"));
  const agentDir = path.join(directory, "agents", "owner"), sessionDir = path.join(agentDir, "sessions");
  fs.mkdirSync(sessionDir, { recursive: true });
  const manifests = new SessionManifestStore({ dbPath: path.join(directory, "manifests.db") });
  fixtures.push({ close: () => { manifests.close(); fs.rmSync(directory, { recursive: true, force: true }); } });
  const model = { id: "research-test-model", provider: "research-test-provider", name: "研究装配测试模型",
    api: "openai-completions" as const, baseUrl: "http://127.0.0.1:1/v1", reasoning: false, input: ["text" as const],
    contextWindow: 32768, maxTokens: 1024, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 } };
  const runtime = await ModelRuntime.create({ authPath: path.join(directory, "auth.json"), modelsPath: null, refreshOnCreate: false });
  runtime.registerProvider(model.provider, { name: "本地测试供应商", baseUrl: model.baseUrl, api: model.api,
    apiKey: "local-test-placeholder", authHeader: true });
  // 只替换模型返回的字节流；会话创建、扩展绑定、提示执行和释放均走真实 SDK。
  const stream = vi.spyOn(runtime, "streamSimple").mockImplementation(() => {
    const result = createAssistantMessageEventStream();
    const message = { role: "assistant" as const, api: model.api, provider: model.provider, model: model.id,
      content: [{ type: "text" as const, text: "真实研究会话已执行" }], stopReason: "stop" as const, timestamp: Date.now(),
      usage: { input: 1, output: 1, cacheRead: 0, cacheWrite: 0, totalTokens: 2,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } } };
    queueMicrotask(() => { result.push({ type: "done", reason: "stop", message }); result.end(); });
    return result;
  });
  const externalExecute = vi.fn(async () => ({ content: [{ type: "text" as const, text: "外部扩展" }], details: {} }));
  const loader = new sdk.DefaultResourceLoader({ cwd: directory, agentDir,
    noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true, noContextFiles: true,
    extensionFactories: [pi => pi.registerTool({ name: "external_write", label: "外部写入", description: "不得进入研究会话",
      parameters: sdk.Type.Object({}), execute: externalExecute })] });
  await loader.reload();
  const skillPath = path.join(directory, "SKILL.md");
  fs.writeFileSync(skillPath, "---\nname: external-skill\ndescription: 外部技能\n---\n外部工作说明");
  vi.spyOn(loader, "getSkills").mockReturnValue({ skills: [{ name: "external-skill", description: "外部技能",
    filePath: skillPath, baseDir: directory,
    sourceInfo: { path: skillPath, source: "test", scope: "user", origin: "top-level" }, disableModelInvocation: false }], diagnostics: [] });
  const inheritedExtensions = loader.getExtensions(), inheritedSendMessage = inheritedExtensions.runtime.sendMessage;
  const names = [...new Set(surfaces.flatMap(surface => getKnowledgeResearchToolNames(surface)))];
  const tools = [...names, "write", "knowledge_manage"].map(name => ({ name, label: name, description: name,
    parameters: { type: "object", properties: {} }, execute: vi.fn(async () => ({ content: [{ type: "text", text: name }] })) }));
  const owner = { id: "owner", agentName: "owner", agentDir, sessionDir, memoryMasterEnabled: true, experienceEnabled: true,
    config: { models: { chat: { provider: model.provider, id: model.id } }, desk: { patrol_tools: "*" } },
    getToolsSnapshot: vi.fn(() => tools), buildSystemPrompt: () => "只核查冻结的知识原文。", systemPrompt: "普通会话说明" };
  const mainPath = path.join(sessionDir, "main.jsonl");
  fs.writeFileSync(mainPath, `${JSON.stringify({ type: "session", version: 3, id: "main", cwd: directory })}\n`);
  const main = manifests.createForPath({ sessionPath: mainPath, ownerAgentId: owner.id, domain: "desktop", kind: "chat",
    lifecycle: "active", provenance: { studioId: "studio-a" } });
  const models = { defaultModel: model, currentModel: model, availableModels: [model], modelRuntime: runtime,
    resolveExecutionModel: value => value, resolveThinkingLevel: value => value };
  const buildTools = vi.fn((_cwd, snapshot, options) => ({ tools: [], customTools: [...snapshot, ...options.extraCustomTools] }));
  const coordinator = new SessionCoordinator({ agentsDir: path.join(directory, "agents"), getAgent: () => owner,
    getActiveAgentId: () => owner.id, getAgentById: id => id === owner.id ? owner : undefined,
    getAgents: () => new Map([[owner.id, owner]]), listAgents: () => [owner], ensureAgentRuntime: async () => owner,
    getModels: () => models, getResourceLoader: () => loader, getSkills: () => ({ getSkillsForAgent: () => loader.getSkills() }),
    buildTools, getHomeCwd: () => directory, getConfig: () => ({}), getPrefs: () => ({ getThinkingLevel: () => "off" }),
    emitEvent: vi.fn(), emitDevLog: vi.fn(), agentIdFromSessionPath: () => owner.id,
    switchAgentOnly: async () => {}, getActivityStore: () => null, sessionManifestStore: manifests });
  coordinator._sessions.set(main.sessionId, { session: { model: model } });
  // 透传侦测仅保留真实装配结果，不能用替身会话绕过第三方组装。
  const assembled = vi.spyOn(sdk, "createAgentSession");
  function optionsFor(surface: typeof surfaces[number]) {
    const research = { runId: "run-a", scopeId: "scope-a", studioId: "studio-a" };
    if (surface === "knowledge_research_root") return { surface, research, parentSessionPath: mainPath, parentSessionId: main.sessionId };
    const parentPath = path.join(agentDir, ".ephemeral", "root.jsonl");
    fs.mkdirSync(path.dirname(parentPath), { recursive: true }); fs.writeFileSync(parentPath, "研究父会话");
    const parent = manifests.createForPath({ sessionPath: parentPath, ownerAgentId: owner.id, domain: "subagent",
      kind: "knowledge_research_root", lifecycle: "active", provenance: { parentSessionId: main.sessionId,
        studioId: research.studioId, researchContext: { runId: research.runId, scopeId: research.scopeId, role: "root" } } });
    coordinator._researchModelsBySession.set(parent.sessionId, model);
    return { surface, parentSessionPath: parentPath, parentSessionId: parent.sessionId,
      research: { ...research, allowedNeedIds: ["need-a"], allowedSourceIds: ["source-a"],
        ...(surface === "knowledge_completeness_worker"
          ? { completenessCheckId: "check-a", completenessShardId: "shard-a", completeness: {} } : {}) } };
  }
  return { coordinator, manifests, stream, loader, assembled, buildTools, externalExecute, inheritedExtensions, inheritedSendMessage, optionsFor };
}

describe("研究隔离会话经过已安装 SDK 的真实装配", () => {
  it.each(surfaces)("%s 能完成一轮，空扩展仍具备独立且稳定的运行载体", async surface => {
    const f = await setup();
    const result = await f.coordinator.executeIsolated("核查资料", { ...f.optionsFor(surface),
      permissionMode: "operate", toolFilter: "*", builtinFilter: ["write"], extraCustomTools: [{ name: "external_write" }] });
    expect(result).toMatchObject({ error: null, replyText: "真实研究会话已执行", sessionPath: null, stopReason: "stop" });
    expect(f.stream).toHaveBeenCalledOnce();
    const created = await f.assembled.mock.results[0].value, session = created.session;
    const first = session.resourceLoader.getExtensions(), second = session.resourceLoader.getExtensions();
    expect(first.extensions).toEqual([]);
    expect(first.errors).toEqual([]);
    expect(first.runtime).toBe(second.runtime);
    expect(first.runtime).toBe(created.extensionsResult.runtime);
    expect(first.runtime).toBe(Reflect.get(session.extensionRunner, "runtime"));
    expect(first.runtime).not.toBe(f.inheritedExtensions.runtime);
    expect(first.runtime.sendMessage).toBeTypeOf("function");
    expect(f.inheritedExtensions.extensions).toHaveLength(1);
    expect(f.inheritedExtensions.runtime.sendMessage).toBe(f.inheritedSendMessage);
    expect(session.resourceLoader.getSkills()).toEqual({ skills: [], diagnostics: [] });
    expect(session.resourceLoader.getAgentsFiles()).toEqual({ agentsFiles: [] });
    expect(session.getActiveToolNames().sort()).toEqual([...getKnowledgeResearchToolNames(surface)].sort());
    expect(JSON.stringify(f.stream.mock.calls[0][1])).not.toContain("external-skill");
    expect(f.externalExecute).not.toHaveBeenCalled();
    const permission = f.buildTools.mock.calls[0][2];
    expect(permission.getPermissionMode()).toBe("read_only");
    expect(permission).toMatchObject({ approvalPolicy: "deny_on_prompt", allowHumanApproval: false, extraCustomTools: [] });
    const sessionPath = session.sessionManager.getSessionFile();
    expect(fs.existsSync(sessionPath)).toBe(false);
    expect(f.manifests.resolveByLocatorPath(sessionPath)).toMatchObject({ lifecycle: "deleted", kind: surface });
  });

  it("两次研究各自持有运行载体，不会覆盖另一会话的绑定", async () => {
    const f = await setup(), options = f.optionsFor("knowledge_research_root");
    expect((await f.coordinator.executeIsolated("第一轮", options)).error).toBeNull();
    const first = await f.assembled.mock.results[0].value;
    const firstSendMessage = first.extensionsResult.runtime.sendMessage;
    expect((await f.coordinator.executeIsolated("第二轮", options)).error).toBeNull();
    const second = await f.assembled.mock.results[1].value;
    expect(second.extensionsResult.runtime).not.toBe(first.extensionsResult.runtime);
    expect(first.extensionsResult.runtime.sendMessage).toBe(firstSendMessage);
    expect(f.stream).toHaveBeenCalledTimes(2);
  });

  it("普通隔离会话仍继承原资源并完成真实模型轮次", async () => {
    const f = await setup();
    const result = await f.coordinator.executeIsolated("普通临时任务", { subagentContext: true,
      workspaceFolders: [], authorizedFolders: [], fileReadSessionPaths: [] });
    expect(result).toMatchObject({ error: null, replyText: "真实研究会话已执行", sessionPath: null });
    expect(f.stream).toHaveBeenCalledOnce();
    const created = await f.assembled.mock.results[0].value;
    expect(created.session.resourceLoader.getExtensions()).toBe(f.inheritedExtensions);
    expect(created.extensionsResult.runtime).toBe(Reflect.get(created.session.extensionRunner, "runtime"));
    expect(created.session.resourceLoader.getSkills().skills.map(skill => skill.name)).toEqual(["external-skill"]);
    expect(fs.existsSync(created.session.sessionManager.getSessionFile())).toBe(false);
  });
});
