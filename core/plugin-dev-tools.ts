import { registerToolCapabilityDelegate } from "../lib/permission/tool-invocation-permission.ts";
import {
  isToolInvocationError,
  ToolInvocationError,
  type ToolTargetId,
} from "../lib/tools/invocation/index.ts";
import type { ToolInvocationGateway } from "./tool-invocation-gateway.ts";
import type { RegisteredToolTarget } from "./tool-target-registry.ts";

function toolOk(message, details = {}) {
  return {
    content: [{ type: "text", text: message }],
    details,
  };
}

function toolError(message, details = {}) {
  return {
    content: [{ type: "text", text: message }],
    details: { ok: false, ...details },
  };
}

function safeJson(value) {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function findRuntimeCtx(args) {
  for (let i = args.length - 1; i >= 2; i -= 1) {
    const value = args[i];
    if (value && typeof value === "object" && (value.sessionManager || value.sessionId || value.sessionRef || value.sessionPath || value.agentId || value.model)) {
      return value;
    }
  }
  return null;
}

function textOrNull(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

type PluginDevHostRuntimeContext = Record<string, unknown> & {
  sessionId?: unknown;
  sessionPath?: unknown;
  legacySessionPath?: unknown;
  sessionRef?: Record<string, unknown>;
  sessionManager?: { getSessionFile?: () => unknown };
  agentId?: unknown;
};

function normalizeHostSessionTarget(runtimeCtx: PluginDevHostRuntimeContext): {
  sessionId?: string;
  sessionPath?: string;
  sessionRef?: {
    sessionId: string;
    sessionPath?: string;
    legacySessionPath?: string;
  };
} {
  const source = runtimeCtx;
  const rawRef = source?.sessionRef && typeof source.sessionRef === "object"
    ? source.sessionRef
    : null;
  const explicitSessionId = textOrNull(source?.sessionId);
  const refSessionId = textOrNull(rawRef?.sessionId);
  const sessionId = explicitSessionId || refSessionId;
  const sessionPath = textOrNull(rawRef?.sessionPath)
    || textOrNull(rawRef?.path)
    || textOrNull(source?.sessionPath)
    || textOrNull(runtimeCtx?.sessionManager?.getSessionFile?.());
  const legacySessionPath = textOrNull(rawRef?.legacySessionPath)
    || textOrNull(source?.legacySessionPath);
  if (!sessionId) {
    return sessionPath ? { sessionPath } : {};
  }
  const sessionRef = {
    sessionId,
    ...(sessionPath ? { sessionPath } : {}),
    ...(legacySessionPath ? { legacySessionPath } : {}),
  };
  return {
    sessionId,
    ...(sessionPath ? { sessionPath } : {}),
    sessionRef,
  };
}

function delegatedCapabilityKey(capability: string, action: string): string {
  return JSON.stringify([capability, action]);
}

const pluginDevDelegatedTargets = new WeakMap<object, Map<string, Set<ToolTargetId>>>();

function createPluginDevInvokeTool({
  invocationGateway,
  resolveChatToolTarget,
  getAgentId,
}: {
  invocationGateway: ToolInvocationGateway;
  resolveChatToolTarget: (pluginId: string, toolName: string) => RegisteredToolTarget | null;
  getAgentId?: () => string | null | undefined;
}) {
  const delegatedTargets = new Map<string, Set<ToolTargetId>>();
  const resolveTarget = (params) => {
    const pluginId = typeof params?.pluginId === "string" ? params.pluginId.trim() : "";
    const toolName = typeof params?.toolName === "string" ? params.toolName.trim() : "";
    try {
      const target = resolveChatToolTarget(pluginId, toolName);
      if (target) return target;
    } catch (cause) {
      if (isToolInvocationError(cause) && cause.code === "TARGET_NOT_FOUND") {
        throw new ToolInvocationError({
          code: "TARGET_NOT_FOUND",
          message: "Development plugin tool target is not registered.",
          route: "plugin-dev-chat",
          sourceId: pluginId || null,
          details: { toolName: toolName || null },
          cause,
        });
      }
      throw new ToolInvocationError({
        code: "TARGET_NOT_VISIBLE",
        message: "Development plugin tool target is not visible on this surface.",
        route: "plugin-dev-chat",
        sourceId: pluginId || null,
        details: { toolName: toolName || null },
        cause,
      });
    }
    throw new ToolInvocationError({
      code: "TARGET_NOT_FOUND",
      message: "Development plugin tool target is not registered.",
      route: "plugin-dev-chat",
      sourceId: pluginId || null,
      details: { toolName: toolName || null },
    });
  };
  const tool = {
    name: "plugin_dev_invoke_tool",
    description: "Invoke one currently loaded development plugin tool through the host invocation gateway.",
    parameters: createSchema({
      pluginId: { type: "string" },
      toolName: { type: "string" },
      arguments: { type: "object", additionalProperties: true },
    }, ["pluginId", "toolName"]),
    metadata: { pluginDevTool: true },
    _toolInvocationRoute: "plugin-dev-chat",
    sessionPermission: {
      resolveInvocation: (params) => {
        const target = resolveTarget(params);
        const prepared = invocationGateway.resolvePermission({
          targetId: target.identity.targetId,
          route: "plugin-dev-chat",
          arguments: params?.arguments || {},
          lifecycleGeneration: target.lifecycleGeneration,
          toolCallId: "plugin_dev_invoke_tool:permission",
        });
        const key = delegatedCapabilityKey(prepared.permission.capability, prepared.permission.action);
        const targetIds = delegatedTargets.get(key) || new Set<ToolTargetId>();
        targetIds.add(prepared.targetId);
        delegatedTargets.set(key, targetIds);
        return {
          ...prepared.permission,
          effectiveInvocation: {
            targetId: prepared.targetId,
            toolName: target.identity.publicName,
            arguments: prepared.arguments,
            generation: prepared.lifecycleGeneration,
          },
        };
      },
    },
    execute: async (toolCallId, params, signal, onUpdate, runtimeCtx) => {
      const target = resolveTarget(params);
      const hostRuntime = runtimeCtx && typeof runtimeCtx === "object"
        ? runtimeCtx as PluginDevHostRuntimeContext
        : {};
      const sessionTarget = normalizeHostSessionTarget(hostRuntime);
      const agentId = textOrNull(hostRuntime.agentId) || textOrNull(getAgentId?.());
      return invocationGateway.invoke({
        targetId: target.identity.targetId,
        route: "plugin-dev-chat",
        arguments: params?.arguments || {},
        sessionId: sessionTarget.sessionId || null,
        sessionPath: sessionTarget.sessionPath || null,
        agentId,
        lifecycleGeneration: target.lifecycleGeneration,
        toolCallId,
        signal,
        onUpdate,
        ctx: hostRuntime,
        runtimeContext: hostRuntime,
      });
    },
  };
  pluginDevDelegatedTargets.set(tool, delegatedTargets);
  return tool;
}

export function registerPluginDevCapabilityDelegates(
  tools: readonly object[],
  { gateway }: { gateway: Pick<ToolInvocationGateway, "canDelegateCapability"> },
) {
  const invokeTool = tools.find((tool) => (tool as { name?: string }).name === "plugin_dev_invoke_tool");
  if (!invokeTool) return;
  const delegatedTargets = pluginDevDelegatedTargets.get(invokeTool);
  registerToolCapabilityDelegate(invokeTool, (capability, action) => {
    const targetIds = delegatedTargets?.get(delegatedCapabilityKey(capability, action));
    if (!targetIds) return false;
    for (const targetId of targetIds) {
      if (gateway.canDelegateCapability(targetId, capability, action)) return true;
    }
    return false;
  });
}

function createSchema(properties, required = []) {
  return {
    type: "object",
    properties,
    required,
    additionalProperties: false,
  };
}

function createPluginDevTool({
  name,
  description,
  parameters,
  service,
  handler,
  permissionAction,
  permissionKind = "review",
}) {
  return {
    name,
    description,
    parameters,
    metadata: { pluginDevTool: true },
    sessionPermission: {
      resolveInvocation: () => ({
        action: permissionAction,
        kind: permissionKind,
        capability: `${name}.${permissionAction}`,
      }),
    },
    execute: async (...args) => {
      const params = args[1] || {};
      const runtimeCtx = findRuntimeCtx(args);
      try {
        const result = await handler({ params, runtimeCtx, service });
        return toolOk(safeJson(result), result);
      } catch (err) {
        return toolError(err?.message || String(err), {
          errorCode: err?.code || "PLUGIN_DEV_TOOL_ERROR",
          status: err?.status || 500,
        });
      }
    },
  };
}

export function createPluginDevTools({
  pluginDevService,
  getAgentId,
  invocationGateway,
  resolveChatToolTarget,
}: {
  pluginDevService?: any;
  getAgentId?: any;
  invocationGateway?: ToolInvocationGateway;
  resolveChatToolTarget?: (pluginId: string, toolName: string) => RegisteredToolTarget | null;
} = {}) {
  if (!pluginDevService) return [];
  return [
    createPluginDevTool({
      name: "plugin_dev_install",
      description: "Install a Hana plugin source directory into the isolated development plugin slot. Requires the Agent plugin dev tools setting to be enabled by the user.",
      service: pluginDevService,
      parameters: createSchema({
        sourcePath: { type: "string", description: "Absolute path to the plugin source directory." },
        pluginId: { type: "string", description: "Optional expected plugin id from manifest.json." },
        allowFullAccess: { type: "boolean", description: "Temporarily allow full-access while this dev slot is loaded." },
      }, ["sourcePath"]),
      permissionAction: "install",
      handler: ({ params, service }) => service.installFromSource({
        sourcePath: params.sourcePath,
        pluginId: params.pluginId,
        allowFullAccess: params.allowFullAccess === true,
      }),
    }),
    createPluginDevTool({
      name: "plugin_dev_reload",
      description: "Reload a previously installed development plugin from its remembered source slot.",
      service: pluginDevService,
      parameters: createSchema({
        pluginId: { type: "string" },
        devRunId: { type: "string", description: "Optional active dev run id guard." },
        allowFullAccess: { type: "boolean" },
      }, ["pluginId"]),
      permissionAction: "reload",
      handler: ({ params, service }) => service.reloadPlugin(params.pluginId, {
        devRunId: params.devRunId,
        allowFullAccess: params.allowFullAccess,
      }),
    }),
    createPluginDevTool({
      name: "plugin_dev_enable",
      description: "Enable a development plugin without changing normal plugin preferences.",
      service: pluginDevService,
      parameters: createSchema({
        pluginId: { type: "string" },
        devRunId: { type: "string", description: "Optional active dev run id guard." },
        allowFullAccess: { type: "boolean" },
      }, ["pluginId"]),
      permissionAction: "enable",
      handler: ({ params, service }) => service.enablePlugin(params.pluginId, {
        devRunId: params.devRunId,
        allowFullAccess: params.allowFullAccess,
      }),
    }),
    createPluginDevTool({
      name: "plugin_dev_disable",
      description: "Disable a development plugin without changing normal plugin preferences.",
      service: pluginDevService,
      parameters: createSchema({
        pluginId: { type: "string" },
        devRunId: { type: "string", description: "Optional active dev run id guard." },
      }, ["pluginId"]),
      permissionAction: "disable",
      handler: ({ params, service }) => service.disablePlugin(params.pluginId, {
        devRunId: params.devRunId,
      }),
    }),
    createPluginDevTool({
      name: "plugin_dev_reset",
      description: "Reset a development plugin by reloading it from its remembered source slot and creating a fresh dev run.",
      service: pluginDevService,
      parameters: createSchema({
        pluginId: { type: "string" },
        devRunId: { type: "string", description: "Optional active dev run id guard." },
        allowFullAccess: { type: "boolean" },
      }, ["pluginId"]),
      permissionAction: "reset",
      handler: ({ params, service }) => service.resetPlugin(params.pluginId, {
        devRunId: params.devRunId,
        allowFullAccess: params.allowFullAccess,
      }),
    }),
    createPluginDevTool({
      name: "plugin_dev_uninstall",
      description: "Uninstall a development plugin from the isolated dev plugin directory and forget its dev slot.",
      service: pluginDevService,
      parameters: createSchema({
        pluginId: { type: "string" },
        devRunId: { type: "string", description: "Optional active dev run id guard." },
      }, ["pluginId"]),
      permissionAction: "uninstall",
      handler: ({ params, service }) => service.uninstallPlugin(params.pluginId, {
        devRunId: params.devRunId,
      }),
    }),
    ...(invocationGateway && resolveChatToolTarget
      ? [createPluginDevInvokeTool({ invocationGateway, resolveChatToolTarget, getAgentId })]
      : []),
    createPluginDevTool({
      name: "plugin_dev_diagnostics",
      description: "Read development plugin slots, load status, logs, UI surfaces, and scenarios.",
      service: pluginDevService,
      parameters: createSchema({
        pluginId: { type: "string" },
      }),
      permissionAction: "diagnose",
      permissionKind: "read",
      handler: ({ params, service }) => service.getDiagnostics(params.pluginId),
    }),
    createPluginDevTool({
      name: "plugin_dev_list_surfaces",
      description: "List page and widget surfaces exposed by development plugins.",
      service: pluginDevService,
      parameters: createSchema({
        pluginId: { type: "string" },
      }),
      permissionAction: "list",
      permissionKind: "read",
      handler: ({ params, service }) => service.listSurfaces(params.pluginId),
    }),
    createPluginDevTool({
      name: "plugin_dev_describe_surface",
      description: "Describe a plugin UI surface with an element-first debugging strategy.",
      service: pluginDevService,
      parameters: createSchema({
        pluginId: { type: "string" },
        kind: { type: "string" },
        route: { type: "string" },
      }, ["pluginId"]),
      permissionAction: "describe",
      permissionKind: "read",
      handler: ({ params, service }) => service.describeSurfaceDebug(params),
    }),
    createPluginDevTool({
      name: "plugin_dev_run_scenario",
      description: "Run one manifest.dev.scenarios smoke test for a development plugin.",
      service: pluginDevService,
      parameters: createSchema({
        pluginId: { type: "string" },
        scenarioId: { type: "string" },
        allowDestructive: { type: "boolean" },
      }, ["pluginId", "scenarioId"]),
      permissionAction: "run",
      handler: ({ params, service }) => service.runScenario(params),
    }),
  ];
}
