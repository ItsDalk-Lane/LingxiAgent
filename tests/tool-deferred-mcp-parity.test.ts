import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import {
  McpManager,
  evaluateMcpToolEligibility,
} from "../core/mcp/manager.ts";

interface TestMcpToolConfig {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  _meta?: { ui: { visibility?: string[]; resourceUri?: string } };
}

interface TestMcpConfig {
  enabled: boolean;
  deferEnabled: boolean;
  deferThreshold: number;
  connectors: Array<{
    id: string;
    name: string;
    url: string;
    tools: TestMcpToolConfig[];
  }>;
}

interface TestToolResult {
  content: Array<{ type: string; text: string }>;
  structuredContent?: Record<string, unknown>;
  isError?: boolean;
  details?: Record<string, unknown> & { errorCode?: string };
}

interface ExecutableTestTool {
  name: string;
  execute: (...args: unknown[]) => Promise<TestToolResult>;
}

function agentConfig(toolNames: string[]) {
  return {
    mcp: {
      connectors: {
        alpha: {
          enabled: true,
          tools: Object.fromEntries(toolNames.map((name) => [name, true])),
        },
      },
    },
  };
}

function managerWithTools({ deferEnabled = true }: { deferEnabled?: boolean } = {}) {
  let configValue: TestMcpConfig = {
    enabled: true,
    deferEnabled,
    deferThreshold: 1,
    connectors: [{
      id: "alpha",
      name: "Alpha",
      url: "https://alpha.example.test",
      tools: [
        {
          name: "search",
          description: "Search Alpha",
          inputSchema: {
            type: "object",
            additionalProperties: false,
            required: ["query"],
            properties: { query: { type: "string" } },
          },
          _meta: { ui: { resourceUri: "ui://alpha/search" } },
        },
        {
          name: "app_only",
          description: "Only for app surfaces",
          inputSchema: { type: "object", properties: {} },
          _meta: { ui: { visibility: ["app"] } },
        },
        {
          name: "lookup",
          description: "Lookup Alpha",
          inputSchema: { type: "object", properties: { query: { type: "string" } } },
        },
      ],
    }],
  };
  const manager = new McpManager({
    dataDir: path.join(os.tmpdir(), `lingxi-mcp-target-${Date.now()}-${Math.random()}`),
    log: { info() {}, warn() {}, error() {}, debug() {} },
  }, {
    configStore: {
      get: vi.fn(() => configValue),
      set: vi.fn((_key: string, value: unknown) => { configValue = value as TestMcpConfig; }),
    },
  });
  manager.registerCachedTools();
  const callTool = vi.fn(async () => ({
    content: [{ type: "text", text: "wire-ok" }],
    structuredContent: { preserved: true },
    details: {
      provenance: { connectorId: "alpha", resultId: "result-1" },
      rounds: [{ round: 1, state: "complete" }],
    },
  }));
  manager.clients.set("alpha", {
    running: true,
    callTool,
  });
  return {
    manager,
    getConfig: () => configValue,
    setConfig: (value: TestMcpConfig) => { configValue = value; },
    callTool,
  };
}

function buildEngineWithManager(
  deferEnabled: boolean,
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-mcp-path-parity-")),
) {
  const fixture = managerWithTools({ deferEnabled });
  const agentDir = path.join(tmpDir, "agents", "focus");
  const workspace = path.join(tmpDir, "workspace");
  const sessionPath = path.join(agentDir, "sessions", "main.jsonl");
  let config = agentConfig(["search", "lookup"]);
  fixture.manager._bus = { request: vi.fn(async () => ({ config })) };
  const agent = { id: "focus", agentDir, config, tools: [] };
  const engine = Object.create(LingxiEngine.prototype);
  engine.lingxiHome = tmpDir;
  engine.getAgent = vi.fn(() => agent);
  engine._agentMgr = { agent };
  engine._pluginManager = null;
  engine._mcp = fixture.manager;
  engine._prefs = {
    getFileBackup: () => ({ enabled: false }),
    getBuiltinToolDeferEnabled: () => false,
  };
  engine._readPreferences = () => ({ sandbox: true });
  engine._confirmStore = null;
  engine._approvalGateway = null;
  engine._emitEvent = vi.fn();
  engine.getSessionPermissionMode = vi.fn(() => "operate");
  engine.getSessionAllowedInvocationCapabilities = vi.fn(() => []);
  engine.getSessionIdForPath = vi.fn(() => "session-1");
  const result = engine.buildTools(workspace, [], {
    agentDir,
    workspace,
    getSessionPath: () => sessionPath,
    getPermissionMode: () => "operate",
  });
  return {
    ...fixture,
    engine,
    result,
    tmpDir,
    sessionPath,
    setAgentTools: (toolNames: string[]) => { config = agentConfig(toolNames); },
  };
}

describe("MCP 唯一 eligibility 判定", () => {
  const base = {
    globalEnabled: true,
    connectorId: "alpha",
    connectorPresent: true,
    connectorEnabled: true,
    toolName: "search",
    toolPresent: true,
    visibility: ["model", "app"],
    surface: "model" as const,
    status: "running",
    transportAvailable: true,
  };

  it.each([
    [{ globalEnabled: false }, "mcp_global_disabled"],
    [{ connectorPresent: false }, "mcp_connector_removed"],
    [{ connectorEnabled: false }, "mcp_connector_disabled"],
    [{ toolPresent: false }, "mcp_tool_removed"],
    [{ visibility: ["app"] }, "mcp_not_visible"],
    [{ status: "stopped", transportAvailable: false }, "mcp_connector_stopped"],
    [{ status: "running", transportAvailable: false }, "mcp_transport_unavailable"],
  ])("拒绝不可用状态 %j", (patch, reason) => {
    expect(evaluateMcpToolEligibility(agentConfig(["search"]), { ...base, ...patch })).toMatchObject({
      eligible: false,
      reason,
    });
  });

  it("拒绝 Agent 未启用并接受所有条件同时满足的模型工具", () => {
    expect(evaluateMcpToolEligibility(agentConfig([]), base)).toMatchObject({
      eligible: false,
      reason: "mcp_agent_disabled",
    });
    expect(evaluateMcpToolEligibility(agentConfig(["search"]), base)).toEqual({ eligible: true });
  });
});

describe("MCP target descriptor", () => {
  it("同时提供发布工具、规范目录元数据和读取实时状态的 eligibility callback", () => {
    const { manager, getConfig, setConfig } = managerWithTools();
    const descriptors = manager.getToolTargetDescriptors();
    const search = descriptors.find((entry) => entry.catalogMetadata.toolName === "search");
    const appOnly = descriptors.find((entry) => entry.catalogMetadata.toolName === "app_only");

    expect(descriptors.some((entry) => entry.publishedTool.name === "mcp_connectors_status")).toBe(false);
    expect(search).toBeDefined();
    expect(appOnly).toBeDefined();

    expect(search).toMatchObject({
      publishedTool: { name: "mcp_alpha_search" },
      catalogMetadata: {
        targetId: "tool:mcp:alpha:search",
        origin: "mcp",
        sourceId: "alpha",
        serverId: "alpha",
        publicName: "alpha_search",
        toolName: "search",
        capabilityBase: "alpha_search",
      },
    });
    expect(search.evaluateEligibility(agentConfig(["search"]), { surface: "model" })).toEqual({ eligible: true });
    expect(appOnly.evaluateEligibility(agentConfig(["app_only"]), { surface: "model" })).toMatchObject({
      eligible: false,
      reason: "mcp_not_visible",
    });

    const withoutSearch = structuredClone(getConfig());
    withoutSearch.connectors[0].tools = withoutSearch.connectors[0].tools.filter(
      (tool) => tool.name !== "search",
    );
    setConfig(withoutSearch);
    expect(search.evaluateEligibility(agentConfig(["search"]), { surface: "model" })).toMatchObject({
      eligible: false,
      reason: "mcp_tool_removed",
    });
  });
});

describe("MCP direct/deferred 路径等价", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  async function invoke(tool: ExecutableTestTool, params: Record<string, unknown>, sessionPath: string) {
    const controller = new AbortController();
    const onUpdate = vi.fn();
    const result = await tool.execute(
      "same-call",
      params,
      controller.signal,
      onUpdate,
      { sessionId: "session-1", sessionPath, agentId: "focus" },
    );
    return { result, controller, onUpdate };
  }

  it("直接与延迟调用复用同一发布适配器并保留结构化结果", async () => {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-mcp-path-parity-"));
    const direct = buildEngineWithManager(false, tmpDir);
    const deferred = buildEngineWithManager(true, tmpDir);
    dirs.push(tmpDir);
    const directTool = direct.result.customTools.find((tool: ExecutableTestTool) => tool.name === "mcp_alpha_search");
    const bridge = deferred.result.customTools.find((tool: ExecutableTestTool) => tool.name === "mcp_call");

    expect(directTool).toBeDefined();
    expect(bridge).toBeDefined();

    const directOutput = await invoke(directTool, { query: "same" }, direct.sessionPath);
    const deferredOutput = await invoke(bridge, {
      server: "alpha",
      tool: "search",
      arguments: { query: "same" },
    }, deferred.sessionPath);

    expect(deferredOutput.result).toEqual(directOutput.result);
    expect(deferredOutput.result).toMatchObject({
      content: [{ type: "text", text: "wire-ok" }],
      structuredContent: { preserved: true },
      details: {
        provenance: { connectorId: "alpha", resultId: "result-1" },
        rounds: [{ round: 1, state: "complete" }],
        mcpAppCard: {
          type: "mcp_app",
          connectorId: "alpha",
          resourceUri: "ui://alpha/search",
          toolCallId: "same-call",
        },
      },
    });
    expect(direct.callTool).toHaveBeenCalledWith("search", { query: "same" }, undefined);
    expect(deferred.callTool).toHaveBeenCalledWith("search", { query: "same" }, undefined);
  });

  it("app-only 工具不进入模型目录、描述或调用", async () => {
    const deferred = buildEngineWithManager(true);
    dirs.push(deferred.tmpDir);
    const search = deferred.result.customTools.find((tool: ExecutableTestTool) => tool.name === "mcp_search_tools");
    const describeTool = deferred.result.customTools.find((tool: ExecutableTestTool) => tool.name === "mcp_describe_tool");
    const call = deferred.result.customTools.find((tool: ExecutableTestTool) => tool.name === "mcp_call");

    expect(search).toBeDefined();
    expect(describeTool).toBeDefined();
    expect(call).toBeDefined();

    expect((await search.execute("search", { query: "app_only" })).content[0].text)
      .toContain("No matching connector or plugin tool");
    expect((await describeTool.execute("describe", { name: "mcp_alpha_app_only" })).content[0].text)
      .toContain("No tool named");
    const result = await call.execute("call", {
      server: "alpha",
      tool: "app_only",
      arguments: {},
    }, { sessionPath: deferred.sessionPath });
    expect(result).toMatchObject({
      isError: true,
      details: { errorCode: "TOOL_INVOCATION_RESOLVER_FAILED" },
    });
  });

  it("Agent 禁用后直接与延迟调用返回同一稳定错误", async () => {
    const direct = buildEngineWithManager(false);
    const deferred = buildEngineWithManager(true);
    dirs.push(direct.tmpDir, deferred.tmpDir);
    direct.setAgentTools([]);
    deferred.setAgentTools([]);
    const directTool = direct.result.customTools.find((tool: ExecutableTestTool) => tool.name === "mcp_alpha_search");
    const bridge = deferred.result.customTools.find((tool: ExecutableTestTool) => tool.name === "mcp_call");

    expect(directTool).toBeDefined();
    expect(bridge).toBeDefined();

    const directError = await invoke(directTool, { query: "same" }, direct.sessionPath).catch((error) => error);
    const deferredError = await invoke(bridge, {
      server: "alpha",
      tool: "search",
      arguments: { query: "same" },
    }, deferred.sessionPath).catch((error) => error);

    expect(directError).toMatchObject({
      code: "TARGET_DISABLED_FOR_AGENT",
      details: { reason: "mcp_agent_disabled" },
    });
    expect(deferredError).toMatchObject({
      code: directError.code,
      details: directError.details,
    });
    expect(direct.callTool).not.toHaveBeenCalled();
    expect(deferred.callTool).not.toHaveBeenCalled();
  });
});
