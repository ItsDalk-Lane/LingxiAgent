import { describe, expect, it, vi } from "vitest";
import { createPluginDevTools } from "../core/plugin-dev-tools.ts";
import { ToolInvocationGateway } from "../core/tool-invocation-gateway.ts";
import { ToolTargetRegistry, type RegisteredToolTarget } from "../core/tool-target-registry.ts";
import {
  createPluginToolIdentity,
  createToolSchemaValidator,
  normalizeToolPermissionContract,
  runWithPreparedInvocation,
} from "../lib/tools/invocation/index.ts";

function serviceStub() {
  return {
    installFromSource: vi.fn(),
    reloadPlugin: vi.fn(),
    enablePlugin: vi.fn(),
    disablePlugin: vi.fn(),
    resetPlugin: vi.fn(),
    uninstallPlugin: vi.fn(),
    invokeTool: vi.fn(),
    getDiagnostics: vi.fn(),
    listSurfaces: vi.fn(),
    describeSurfaceDebug: vi.fn(),
    runScenario: vi.fn(),
  };
}

function registerPluginTarget(
  registry: ToolTargetRegistry,
  {
    pluginId,
    localName,
    permission,
    executeCanonical = vi.fn(async (
      _id: string,
      args: Record<string, unknown>,
      _signal: AbortSignal | undefined,
      _onUpdate: unknown,
      _ctx: unknown,
    ) => ({ content: [{ type: "text", text: JSON.stringify(args) }] })),
    available = true,
  }: {
    pluginId: string;
    localName: string;
    permission: { readOnly: true } | {
      kind: "external_side_effect";
      describeSideEffect: (args: Record<string, unknown>) => Record<string, unknown>;
    };
    executeCanonical?: RegisteredToolTarget["executeCanonical"];
    available?: boolean;
  },
) {
  const publicName = `${pluginId}_${localName}`;
  const identity = createPluginToolIdentity({ pluginId, publicName, capabilityBase: localName });
  const parameters = {
    type: "object",
    additionalProperties: false,
    required: ["query"],
    properties: { query: { type: "string" } },
  };
  const permissionContract = normalizeToolPermissionContract({
    name: publicName,
    _pluginId: pluginId,
    sessionPermission: permission,
  }, identity);
  const validator = createToolSchemaValidator(parameters, identity);
  return registry.register({
    identity,
    label: publicName,
    description: publicName,
    parameters,
    deferrable: true,
    pinned: false,
    permission: permissionContract,
    validator,
    availability: available ? { eligible: true } : { eligible: false, reason: "disabled" },
    getCurrentGeneration: () => 1,
    isCurrentlyAvailable: () => available,
    executeCanonical,
    normalizeResult: (result) => result,
  });
}

function fixture() {
  const registry = new ToolTargetRegistry();
  const sideEffect = vi.fn((args: Record<string, unknown>) => ({
    kind: "external_api",
    summary: `Send ${String(args.query)}`,
    ruleId: "dev-send",
  }));
  const read = registerPluginTarget(registry, {
    pluginId: "dev-read",
    localName: "lookup",
    permission: { readOnly: true },
  });
  const writeExecute = vi.fn(async (
    _id: string,
    args: Record<string, unknown>,
    _signal: AbortSignal | undefined,
    _onUpdate: unknown,
    _ctx: unknown,
  ) => ({ content: [{ type: "text", text: JSON.stringify(args) }] }));
  const write = registerPluginTarget(registry, {
    pluginId: "dev-write",
    localName: "send",
    permission: { kind: "external_side_effect", describeSideEffect: sideEffect },
    executeCanonical: writeExecute,
  });
  const disabled = registerPluginTarget(registry, {
    pluginId: "dev-disabled",
    localName: "lookup",
    permission: { readOnly: true },
    available: false,
  });
  const gateway = new ToolInvocationGateway({ registry, authorize: async () => undefined });
  const allowedTargets = new Set([
    read.identity.targetId,
    write.identity.targetId,
    disabled.identity.targetId,
  ]);
  const resolveChatToolTarget = (pluginId: string, toolName: string): RegisteredToolTarget => {
    const target = registry.resolveCatalogTarget({ serverId: pluginId, toolName });
    if (!allowedTargets.has(target.identity.targetId)) throw new Error("not a dev target");
    return target;
  };
  const service = serviceStub();
  const tools = createPluginDevTools({
    pluginDevService: service,
    getAgentId: () => "host-agent",
    invocationGateway: gateway,
    resolveChatToolTarget,
  });
  const invoke = tools.find((tool) => tool.name === "plugin_dev_invoke_tool");
  return { gateway, invoke, read, write, writeExecute, disabled, sideEffect, service };
}

function permissionFor(invoke: ReturnType<typeof fixture>["invoke"], pluginId: string, toolName: string) {
  return invoke.sessionPermission.resolveInvocation({
    pluginId,
    toolName,
    arguments: { query: "hello" },
  });
}

describe("plugin dev chat 规范调用", () => {
  it("模型 schema 不暴露任何会话或 Agent 身份覆盖字段", () => {
    const { invoke } = fixture();
    expect(invoke.parameters).toMatchObject({
      additionalProperties: false,
      required: ["pluginId", "toolName"],
      properties: {
        pluginId: { type: "string" },
        toolName: { type: "string" },
        arguments: { type: "object" },
      },
    });
    for (const field of ["sessionId", "sessionRef", "sessionPath", "legacySessionPath", "agentId", "input"]) {
      expect(invoke.parameters.properties).not.toHaveProperty(field);
    }
  });

  it("权限解析保留真实 read/auto 与副作用目标自身的 review/capability/说明", () => {
    const { invoke, read, write, sideEffect } = fixture();

    expect(permissionFor(invoke, "dev-read", "lookup")).toMatchObject({
      action: "read",
      kind: "read",
      capability: "lookup.read",
      effectiveInvocation: { targetId: read.identity.targetId, toolName: read.identity.publicName },
    });
    expect(permissionFor(invoke, "dev-write", "send")).toMatchObject({
      action: "execute",
      kind: "review",
      capability: "send.execute",
      sideEffect: { kind: "external_api", ruleId: "dev-send" },
      effectiveInvocation: { targetId: write.identity.targetId, toolName: write.identity.publicName },
    });
    expect(sideEffect).toHaveBeenCalledWith({ query: "hello" });
  });

  it("执行只使用宿主身份并通过 Gateway 的 plugin-dev-chat 路由", async () => {
    const { gateway, invoke, write, writeExecute, service } = fixture();
    const runtimeCtx = {
      sessionId: "host-session",
      sessionPath: "/host/session.jsonl",
      agentId: "host-agent",
    };
    const request = {
      targetId: write.identity.targetId,
      route: "plugin-dev-chat" as const,
      arguments: { query: "hello" },
      sessionId: "host-session",
      sessionPath: "/host/session.jsonl",
      agentId: "host-agent",
      lifecycleGeneration: write.lifecycleGeneration,
      toolCallId: "outer-call",
      runtimeContext: runtimeCtx,
    };
    const prepared = gateway.resolvePermission(request);
    const result = await runWithPreparedInvocation(prepared, () => invoke.execute(
      "outer-call",
      {
        pluginId: "dev-write",
        toolName: "send",
        arguments: { query: "hello" },
        sessionId: "forged-session",
        sessionPath: "/forged.jsonl",
        agentId: "forged-agent",
      },
      undefined,
      undefined,
      runtimeCtx,
    )) as { content: Array<{ text: string }> };

    expect(result.content[0].text).toContain("hello");
    expect(writeExecute).toHaveBeenCalledWith(
      "outer-call",
      { query: "hello" },
      undefined,
      undefined,
      expect.objectContaining({
        sessionId: "host-session",
        sessionPath: "/host/session.jsonl",
        agentId: "host-agent",
        invocationRoute: "plugin-dev-chat",
      }),
    );
    expect(service.invokeTool).not.toHaveBeenCalled();
  });

  it("无法解析的、禁用的或非 dev 目标在调用前关闭", () => {
    const { invoke, service } = fixture();
    expect(permissionFor(invoke, "missing", "tool")).toBeNull();
    expect(() => permissionFor(invoke, "dev-disabled", "lookup")).toThrowError(
      expect.objectContaining({ code: "TARGET_NOT_VISIBLE" }),
    );
    expect(service.invokeTool).not.toHaveBeenCalled();
  });
});
