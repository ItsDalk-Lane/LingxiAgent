import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import { McpManager } from "../core/mcp/manager.ts";
import { PluginManager } from "../core/plugin-manager.ts";
import { ToolInvocationGateway } from "../core/tool-invocation-gateway.ts";
import {
  ToolTargetRegistry,
  type RegisteredToolTarget,
  type ToolTargetAvailabilityDecision,
} from "../core/tool-target-registry.ts";
import {
  createToolSchemaValidator,
  runWithPreparedInvocation,
  type ToolInvocationRoute,
} from "../lib/tools/invocation/index.ts";

interface ExecutableTool {
  name: string;
  description: string;
  parameters: unknown;
  _pluginId?: string;
  _toolLifecycleGeneration: number;
  _toolTargetIdentity: RegisteredToolTarget["identity"];
  _normalizedPermissionContract: RegisteredToolTarget["permission"];
}

interface InvocationFixture {
  gateway: ToolInvocationGateway;
  request: {
    targetId: RegisteredToolTarget["identity"]["targetId"];
    route: ToolInvocationRoute;
    arguments: Record<string, unknown>;
    lifecycleGeneration: string | number;
    toolCallId: string;
    runtimeContext: Record<string, unknown>;
  };
  target: RegisteredToolTarget;
  executeCanonical: ReturnType<typeof vi.fn>;
}

const tempDirs: string[] = [];

afterEach(() => {
  for (const dir of tempDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function registerInvocationTarget({
  tool,
  getCurrentGeneration,
  isCurrentlyAvailable,
  runtimeContext = {},
}: {
  tool: ExecutableTool;
  getCurrentGeneration: () => number;
  isCurrentlyAvailable: (
    runtimeContext: unknown,
  ) => boolean | ToolTargetAvailabilityDecision | Promise<boolean | ToolTargetAvailabilityDecision>;
  runtimeContext?: Record<string, unknown>;
}): InvocationFixture {
  const registry = new ToolTargetRegistry();
  const validator = createToolSchemaValidator(tool.parameters, tool._toolTargetIdentity);
  const executeCanonical = vi.fn(async () => ({ content: [{ type: "text", text: "side effect" }] }));
  const target = registry.register({
    identity: tool._toolTargetIdentity,
    label: tool.name,
    description: tool.description,
    parameters: validator.schema,
    deferrable: true,
    pinned: false,
    permission: tool._normalizedPermissionContract,
    validator,
    availability: { eligible: true },
    getCurrentGeneration,
    isCurrentlyAvailable,
    executeCanonical,
    normalizeResult: (result) => result,
  });
  const gateway = new ToolInvocationGateway({ registry, authorize: async () => undefined });
  return {
    gateway,
    target,
    executeCanonical,
    request: {
      targetId: target.identity.targetId,
      route: "direct",
      arguments: { query: "same" },
      lifecycleGeneration: target.lifecycleGeneration,
      toolCallId: "approved-call",
      runtimeContext,
    },
  };
}

async function invokeAfterApproval(
  fixture: InvocationFixture,
  mutate: () => void | Promise<void>,
) {
  const prepared = fixture.gateway.resolvePermission(fixture.request);
  await mutate();
  return runWithPreparedInvocation(prepared, () => fixture.gateway.invoke(fixture.request));
}

async function pluginFixture(pluginId: string) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-tool-lifecycle-plugin-"));
  tempDirs.push(root);
  const builtinDir = path.join(root, "builtin");
  const communityDir = path.join(root, "community");
  const pluginDir = path.join(communityDir, pluginId);
  fs.mkdirSync(builtinDir, { recursive: true });
  fs.mkdirSync(pluginDir, { recursive: true });
  fs.writeFileSync(path.join(pluginDir, "manifest.json"), JSON.stringify({
    id: pluginId,
    name: pluginId,
    version: "1.0.0",
  }));
  const preferencesManager = {
    disabled: [] as string[],
    getDisabledPlugins() { return [...this.disabled]; },
    setDisabledPlugins(value: string[]) { this.disabled = [...value]; },
    getAllowFullAccessPlugins: () => false,
  };
  const manager = new PluginManager({
    pluginsDirs: [builtinDir, communityDir],
    dataDir: path.join(root, "data"),
    bus: { emit: vi.fn() },
    preferencesManager,
  } as never);
  manager.scan();
  await manager.loadAll();
  const entry = manager.getPlugin(pluginId);
  manager.addTool(pluginId, {
    name: "write",
    description: "Write through plugin",
    parameters: {
      type: "object",
      additionalProperties: false,
      required: ["query"],
      properties: { query: { type: "string" } },
    },
    sessionPermission: { kind: "external_side_effect" },
    execute: async () => "should not run",
  }, { pluginKey: entry.pluginKey, source: entry.source });
  const tool = manager.getPluginTool(pluginId, "write") as ExecutableTool;
  const invocation = registerInvocationTarget({
    tool,
    getCurrentGeneration: () => tool._toolLifecycleGeneration,
    isCurrentlyAvailable: () => manager.isPluginToolCurrentlyAvailable(pluginId, tool.name),
  });
  return { manager, tool, invocation };
}

describe("旧插件会话撤销", () => {
  it.each([
    ["disable", async (manager: PluginManager, pluginId: string) => manager.disablePlugin(pluginId)],
    ["uninstall", async (manager: PluginManager, pluginId: string) => manager.removePlugin(pluginId)],
    ["reload", async (manager: PluginManager, pluginId: string) => manager.enablePlugin(pluginId)],
  ])("审批后 %s 会以 TARGET_REVOKED 拒绝且不执行旧对象", async (_operation, mutate) => {
    const pluginId = `lifecycle-${_operation}`;
    const { manager, invocation } = await pluginFixture(pluginId);

    await expect(invokeAfterApproval(invocation, () => mutate(manager, pluginId)))
      .rejects.toMatchObject({ code: "TARGET_REVOKED" });
    expect(invocation.executeCanonical).not.toHaveBeenCalled();
  });
});

function mcpFixture() {
  let config = {
    enabled: true,
    connectors: [{
      id: "alpha",
      name: "Alpha",
      url: "https://alpha.example.test",
      tools: [{
        name: "search",
        description: "Old description",
        inputSchema: {
          type: "object",
          additionalProperties: false,
          required: ["query"],
          properties: { query: { type: "string" } },
        },
      }],
    }],
  };
  const callTool = vi.fn(async () => ({ content: [{ type: "text", text: "wire" }] }));
  const manager = new McpManager({
    dataDir: path.join(os.tmpdir(), `lingxi-tool-lifecycle-mcp-${Date.now()}-${Math.random()}`),
    log: { info() {}, warn() {}, error() {}, debug() {} },
  }, {
    configStore: {
      get: vi.fn(() => config),
      set: vi.fn((_key: string, value: unknown) => { config = value as typeof config; }),
    },
  });
  manager.registerCachedTools();
  manager.clients.set("alpha", { running: true, callTool });
  const descriptor = manager.getToolTargetDescriptors()[0];
  const tool = {
    ...descriptor.publishedTool,
    _toolLifecycleGeneration: descriptor.catalogMetadata.lifecycleGeneration,
    _toolTargetIdentity: descriptor.identity,
    _normalizedPermissionContract: descriptor.permission,
  } as ExecutableTool;
  const runtimeContext = {
    agentConfig: {
      mcp: { connectors: { alpha: { enabled: true, tools: { search: true } } } },
    },
  };
  const invocation = registerInvocationTarget({
    tool,
    getCurrentGeneration: descriptor.getCurrentGeneration,
    isCurrentlyAvailable: descriptor.isCurrentlyAvailable,
    runtimeContext,
  });
  return { manager, descriptor, invocation, callTool, getConfig: () => config };
}

describe("旧 MCP 会话撤销", () => {
  it("工具列表变化后保留旧描述快照但拒绝执行旧目标", async () => {
    const { manager, invocation, getConfig, callTool } = mcpFixture();
    const oldDescription = invocation.target.description;

    await expect(invokeAfterApproval(invocation, () => {
      const next = structuredClone(getConfig());
      next.connectors[0].tools[0].description = "New description";
      manager.saveConfig(next);
      manager.registerCachedTools();
    })).rejects.toMatchObject({ code: "TARGET_REVOKED" });

    expect(invocation.target.description).toBe(oldDescription);
    expect(callTool).not.toHaveBeenCalled();
    expect(invocation.executeCanonical).not.toHaveBeenCalled();
  });

  it("临时断线保持 TRANSPORT_FAILURE，不推进代次也不执行旧对象", async () => {
    const { manager, invocation, callTool } = mcpFixture();
    const generation = invocation.target.lifecycleGeneration;

    await expect(invokeAfterApproval(invocation, () => {
      manager.clients.delete("alpha");
    })).rejects.toMatchObject({
      code: "TRANSPORT_FAILURE",
      details: { reason: "mcp_connector_stopped" },
    });

    expect(invocation.target.getCurrentGeneration()).toBe(generation);
    expect(callTool).not.toHaveBeenCalled();
    expect(invocation.executeCanonical).not.toHaveBeenCalled();
  });
});

describe("Catalog 漂移清单", () => {
  it("同时包含当前 plugin 与 MCP 目标", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-tool-lifecycle-catalog-"));
    tempDirs.push(root);
    const pluginManager = new PluginManager({
      pluginsDir: path.join(root, "plugins"),
      dataDir: path.join(root, "data"),
      bus: { emit: vi.fn() },
    } as never);
    pluginManager.addTool("catalog", {
      name: "search",
      description: "Plugin search",
      sessionPermission: { readOnly: true },
      execute: async () => "ok",
    });
    const engine = Object.create(LingxiEngine.prototype);
    engine._pluginManager = pluginManager;
    engine._prefs = { getBuiltinToolDeferEnabled: () => true };
    engine._mcp = {
      getAllTools: () => [{ name: "mcp_alpha_search" }],
      getToolTargetDescriptors: () => [{
        publishedTool: { name: "mcp_alpha_search" },
        catalogMetadata: { publicName: "alpha_search" },
      }],
    };

    expect(engine.getLiveToolCatalogNames()).toEqual(["alpha_search", "catalog_search"]);
  });
});
