/**
 * The three tools a session carries in place of the tool schemas it deferred.
 *
 * Search and describe are pure lookups over the catalog. `mcp_call` is the
 * execution path: it resolves the real target and forwards the complete call
 * envelope to the canonical invocation gateway.
 *
 * Permission is the subtle part. The bridge must not become a way to launder
 * one approval into access to every connector, so `mcp_call` never asks to be
 * allowed as itself: it resolves the target first and presents that tool's real
 * capability. A session grant therefore means exactly the same thing whether
 * the model reached the tool directly or through here. The host authorizes that
 * by registering the tool object with the permission layer; see
 * registerBridgeCapabilityDelegates below.
 */

import { Type } from "../lib/pi-sdk/index.ts";
import { registerToolCapabilityDelegate } from "../lib/permission/tool-invocation-permission.ts";
import {
  createFirstPartyToolIdentity,
  isToolInvocationError,
  ToolInvocationError,
  type ToolTargetId,
} from "../lib/tools/invocation/index.ts";
import type { ToolCatalog, ToolCatalogEntry } from "./tool-catalog.ts";
import type { ToolInvocationGateway } from "./tool-invocation-gateway.ts";

export const BRIDGE_TOOL_NAMES = ["mcp_search_tools", "mcp_describe_tool", "mcp_call"] as const;

const SEARCH_TOOL_NAME = "mcp_search_tools";
const DESCRIBE_TOOL_NAME = "mcp_describe_tool";
const CALL_TOOL_NAME = "mcp_call";

export interface BridgeToolDeps {
  catalog: ToolCatalog;
  gateway: Pick<ToolInvocationGateway, "resolvePermission" | "invoke" | "canDelegateCapability">;
  log?: { warn?: (message: string) => void; log?: (message: string) => void };
}

const bridgeDelegatedTargets = new WeakMap<object, Map<string, Set<ToolTargetId>>>();
const bridgeDelegatedTargetsByResolver = new WeakMap<object, Map<string, Set<ToolTargetId>>>();

function delegatedCapabilityKey(capability: string, action: string): string {
  return JSON.stringify([capability, action]);
}

function text(value: string) {
  return { content: [{ type: "text", text: value }] };
}

/**
 * Resolve `(server, tool)` to a catalog entry.
 *
 * The model sees catalog names in the manifest and in search results, but the
 * server's own name for a tool is what appears in its documentation, so both
 * are accepted. The server must own the resolved entry either way: a tool named
 * under the wrong server never resolves.
 */
function resolveTarget(catalog: ToolCatalog, server: unknown, tool: unknown): ToolCatalogEntry | null {
  const serverId = typeof server === "string" ? server.trim() : "";
  const toolName = typeof tool === "string" ? tool.trim() : "";
  if (!serverId || !toolName) return null;

  try {
    const targetId = catalog.resolveTarget({ serverId, toolName });
    return catalog.getByTargetId(targetId);
  } catch (error) {
    if (isToolInvocationError(error) && error.code === "TARGET_NOT_FOUND") return null;
    throw error;
  }
}

function missingTargetError(server: unknown, tool: unknown): ToolInvocationError {
  const sourceId = typeof server === "string" && server.trim() ? server.trim() : null;
  return new ToolInvocationError({
    code: "TARGET_NOT_FOUND",
    message: "Deferred tool target is not registered.",
    route: "deferred",
    sourceId,
    details: {
      serverId: sourceId,
      toolName: typeof tool === "string" && tool.trim() ? tool.trim() : null,
    },
  });
}

function schemaProperties(schema: unknown): Record<string, any> {
  const properties = (schema as any)?.properties;
  return properties && typeof properties === "object" ? properties : {};
}

function requiredNames(schema: unknown): string[] {
  const required = (schema as any)?.required;
  return Array.isArray(required) ? required.filter((name): name is string => typeof name === "string") : [];
}

function renderHit(entry: ToolCatalogEntry, schemaRequired?: string[]): string {
  const required = schemaRequired?.length
    ? `必填：${schemaRequired.join(", ")}`
    : (entry.paramsSummary ? `参数：${entry.paramsSummary}` : "无参数");
  return [
    `${entry.name}（${entry.serverLabel}）`,
    entry.description || "无描述",
    required,
  ].join("\n  ");
}

function nearNames(catalog: ToolCatalog, name: string, limit = 3): string[] {
  const needle = name.trim().toLowerCase();
  if (!needle) return [];
  return catalog.all()
    .filter((entry) => entry.name.toLowerCase().includes(needle) || entry.toolName.toLowerCase().includes(needle))
    .map((entry) => entry.name)
    .sort((left, right) => left.localeCompare(right))
    .slice(0, limit);
}

function renderSchema(schema: unknown): string {
  const properties = schemaProperties(schema);
  const required = new Set(requiredNames(schema));
  const names = Object.keys(properties);
  if (names.length === 0) return "无参数";
  return names.map((name) => {
    const spec = properties[name] || {};
    const flag = required.has(name) ? "必填" : "可选";
    const type = typeof spec.type === "string" ? spec.type : "any";
    const description = typeof spec.description === "string" && spec.description ? ` — ${spec.description}` : "";
    return `- ${name}（${type}，${flag}）${description}`;
  }).join("\n");
}

function callExample(entry: ToolCatalogEntry, schema: unknown): string {
  const properties = schemaProperties(schema);
  const example: Record<string, unknown> = {};
  for (const name of requiredNames(schema)) {
    const type = properties[name]?.type;
    example[name] = type === "number" || type === "integer"
      ? 0
      : type === "boolean"
        ? true
        : type === "array"
          ? []
          : type === "object"
            ? {}
            : `<${name}>`;
  }
  return JSON.stringify({ server: entry.serverId, tool: entry.name, arguments: example }, null, 2);
}

export function createBridgeTools({ catalog, gateway }: BridgeToolDeps) {
  const delegatedTargets = new Map<string, Set<ToolTargetId>>();
  const searchTool = {
    name: SEARCH_TOOL_NAME,
    label: "Search MCP Tools",
    description:
      "Search deferred tools supplied by MCP connectors and bundled plugins. Use it when you need a capability that is not already loaded.",
    parameters: Type.Object({
      query: Type.String({ description: "Keywords describing the capability you need." }),
      limit: Type.Optional(Type.Number({ description: "Maximum number of results, default 5." })),
    }),
    sessionPermission: {
      resolveInvocation: () => ({
        action: "read",
        kind: "read",
        capability: `${SEARCH_TOOL_NAME}.read`,
      }),
    },
    execute: async (_id: string, params: any) => {
      const hits = catalog.search(String(params?.query ?? ""), {
        limit: Number.isFinite(params?.limit) ? Number(params.limit) : undefined,
      });
      if (hits.length === 0) {
        return text(
          `No matching connector or plugin tool. Try different keywords, or use ${DESCRIBE_TOOL_NAME} if you already know a tool name.`,
        );
      }
      const body = hits.map((hit) => renderHit(hit)).join("\n\n");
      return text(`${hits.length} 个匹配的 connector/plugin 工具：\n\n${body}\n\n用 ${DESCRIBE_TOOL_NAME} 查看完整参数，再用 ${CALL_TOOL_NAME} 调用。`);
    },
  };

  const describeTool = {
    name: DESCRIBE_TOOL_NAME,
    label: "Describe MCP Tool",
    description:
      "Show the full parameter schema for one deferred MCP connector or bundled plugin tool before calling it.",
    parameters: Type.Object({
      name: Type.String({ description: "Exact tool name, as returned by mcp_search_tools." }),
      server: Type.Optional(Type.String({ description: "Server or source id used to disambiguate a shared name." })),
    }),
    sessionPermission: {
      resolveInvocation: () => ({
        action: "read",
        kind: "read",
        capability: `${DESCRIBE_TOOL_NAME}.read`,
      }),
    },
    execute: async (_id: string, params: any) => {
      const requested = String(params?.name ?? "");
      const serverId = typeof params?.server === "string" && params.server.trim()
        ? params.server.trim()
        : "";
      let described;
      try {
        described = catalog.describe(requested, serverId ? { serverId } : {});
      } catch (error) {
        if (isToolInvocationError(error) && error.code === "TARGET_AMBIGUOUS") {
          return text(`Tool name ${requested} matches multiple sources. Provide server to choose one.`);
        }
        throw error;
      }
      if (!described) {
        const suggestions = nearNames(catalog, requested);
        return text(suggestions.length > 0
          ? `No tool named ${requested}. Closest matches: ${suggestions.join(", ")}.`
          : `No tool named ${requested}. Use ${SEARCH_TOOL_NAME} to find one.`);
      }
      return text([
        `${described.name}（${described.serverLabel}）`,
        described.description || "无描述",
        "",
        "参数：",
        renderSchema(described.schema),
        "",
        `调用示例（${CALL_TOOL_NAME}）：`,
        callExample(catalog.getByTargetId(described.targetId)!, described.schema),
      ].join("\n"));
    },
  };

  const callTool = {
    name: CALL_TOOL_NAME,
    label: "Call MCP Tool",
    description:
      "Call one deferred MCP connector or bundled plugin tool by server/source and tool name. Look it up first so the arguments match its schema.",
    parameters: Type.Object({
      server: Type.String({ description: "Server id that owns the tool." }),
      tool: Type.String({ description: "Tool name to invoke." }),
      arguments: Type.Optional(Type.Object({}, {
        description: "Arguments object matching the tool's schema.",
        additionalProperties: true,
      })),
    }),
    sessionPermission: {
      /**
       * Resolve the real target and speak in its name. Returning null for an
       * unresolvable target makes the permission layer fail closed, so a call
       * to something outside the catalog never reaches the network.
       */
      resolveInvocation: (params: any) => {
        const entry = resolveTarget(catalog, params?.server, params?.tool);
        if (!entry) throw missingTargetError(params?.server, params?.tool);
        const prepared = gateway.resolvePermission({
          targetId: entry.targetId,
          route: "deferred",
          arguments: (params?.arguments ?? {}) as Record<string, unknown>,
          lifecycleGeneration: entry.lifecycleGeneration,
          toolCallId: `${CALL_TOOL_NAME}:permission`,
        });
        const resolvedEntry = catalog.getByTargetId(prepared.targetId);
        if (!resolvedEntry) return null;
        const key = delegatedCapabilityKey(prepared.permission.capability, prepared.permission.action);
        const targetIds = delegatedTargets.get(key) ?? new Set<ToolTargetId>();
        targetIds.add(prepared.targetId);
        delegatedTargets.set(key, targetIds);
        return {
          ...prepared.permission,
          effectiveInvocation: {
            targetId: prepared.targetId,
            toolName: resolvedEntry.publicName,
            arguments: prepared.arguments,
            generation: prepared.lifecycleGeneration,
          },
        };
      },
    },
    _toolTargetIdentity: createFirstPartyToolIdentity({
      publicName: CALL_TOOL_NAME,
      capabilityBase: CALL_TOOL_NAME,
    }),
    _toolInvocationRoute: "deferred",
    execute: async (
      toolCallId: string,
      params: any,
      signal: AbortSignal | undefined,
      onUpdate: unknown,
      ctx: unknown,
    ) => {
      const entry = resolveTarget(catalog, params?.server, params?.tool);
      if (!entry) throw missingTargetError(params?.server, params?.tool);
      const runtime = ctx && typeof ctx === "object" && !Array.isArray(ctx)
        ? ctx as Record<string, unknown>
        : {};
      const stableRuntimeText = (key: string) => (
        typeof runtime[key] === "string" && runtime[key].trim()
          ? runtime[key].trim() as string
          : null
      );
      return gateway.invoke({
        targetId: entry!.targetId,
        route: "deferred",
        arguments: (params?.arguments ?? {}) as Record<string, unknown>,
        sessionId: stableRuntimeText("sessionId"),
        sessionPath: stableRuntimeText("sessionPath"),
        agentId: stableRuntimeText("agentId"),
        lifecycleGeneration: entry!.lifecycleGeneration,
        toolCallId,
        signal,
        onUpdate,
        ctx,
        runtimeContext: ctx,
      });
    },
  };

  bridgeDelegatedTargets.set(callTool, delegatedTargets);
  bridgeDelegatedTargetsByResolver.set(callTool.sessionPermission.resolveInvocation, delegatedTargets);

  return [searchTool, describeTool, callTool];
}

/**
 * Authorize the bridge to speak for the tools it fronts.
 *
 * The gateway checks the capability against the authoritative target registry;
 * the catalog's display strings are never treated as proof of ownership.
 *
 * Registration is keyed on the tool object itself, so this must be called with
 * the same objects createBridgeTools returned, before they are wrapped.
 */
export function registerBridgeCapabilityDelegates(
  tools: readonly any[],
  { gateway }: { gateway: Pick<ToolInvocationGateway, "canDelegateCapability"> },
): void {
  const callTool = tools.find((tool) => tool?.name === CALL_TOOL_NAME);
  if (!callTool) return;
  const resolver = callTool?.sessionPermission?.resolveInvocation;
  const delegatedTargets = bridgeDelegatedTargets.get(callTool)
    ?? (resolver && typeof resolver === "function"
      ? bridgeDelegatedTargetsByResolver.get(resolver)
      : undefined);
  registerToolCapabilityDelegate(callTool, (capability, action) => {
    const targetIds = delegatedTargets?.get(delegatedCapabilityKey(capability, action));
    if (!targetIds || targetIds.size === 0) return false;
    return [...targetIds].some((targetId) => (
      gateway.canDelegateCapability(targetId, capability, action)
    ));
  });
}
