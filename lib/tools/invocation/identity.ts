import type { ToolTargetId, ToolTargetIdentity } from "./types.ts";

interface CommonIdentityInput {
  publicName: string;
  capabilityBase: string;
}

export type FirstPartyToolIdentityInput = CommonIdentityInput;

export interface PluginToolIdentityInput extends CommonIdentityInput {
  pluginId: string;
}

export interface McpToolIdentityInput extends CommonIdentityInput {
  serverId: string;
  remoteToolName: string;
}

function requireIdentityPart(value: unknown, field: string): string {
  if (typeof value !== "string" || value.trim() === "") {
    throw new TypeError(`${field} must be a non-empty string`);
  }
  return value.trim();
}

function asTargetId(value: string): ToolTargetId {
  return value as ToolTargetId;
}

function freezeIdentity(identity: ToolTargetIdentity): ToolTargetIdentity {
  return Object.freeze(identity);
}

export function createFirstPartyToolIdentity(
  input: FirstPartyToolIdentityInput,
): ToolTargetIdentity {
  const publicName = requireIdentityPart(input?.publicName, "publicName");
  const capabilityBase = requireIdentityPart(input?.capabilityBase, "capabilityBase");
  return freezeIdentity({
    targetId: asTargetId(`tool:first-party:${encodeURIComponent(publicName)}`),
    origin: "first-party",
    sourceId: "first-party",
    localName: publicName,
    publicName,
    capabilityBase,
  });
}

export function createPluginToolIdentity(
  input: PluginToolIdentityInput,
): ToolTargetIdentity {
  const pluginId = requireIdentityPart(input?.pluginId, "pluginId");
  const publicName = requireIdentityPart(input?.publicName, "publicName");
  const capabilityBase = requireIdentityPart(input?.capabilityBase, "capabilityBase");
  const prefix = `${pluginId}_`;
  const localName = requireIdentityPart(
    publicName.startsWith(prefix) ? publicName.slice(prefix.length) : publicName,
    "localName",
  );
  return freezeIdentity({
    targetId: asTargetId(
      `tool:plugin:${encodeURIComponent(pluginId)}:${encodeURIComponent(localName)}`,
    ),
    origin: "plugin",
    sourceId: pluginId,
    localName,
    publicName,
    capabilityBase,
  });
}

export function createMcpToolIdentity(input: McpToolIdentityInput): ToolTargetIdentity {
  const serverId = requireIdentityPart(input?.serverId, "serverId");
  const remoteToolName = requireIdentityPart(input?.remoteToolName, "remoteToolName");
  const publicName = requireIdentityPart(input?.publicName, "publicName");
  const capabilityBase = requireIdentityPart(input?.capabilityBase, "capabilityBase");
  return freezeIdentity({
    targetId: asTargetId(
      `tool:mcp:${encodeURIComponent(serverId)}:${encodeURIComponent(remoteToolName)}`,
    ),
    origin: "mcp",
    sourceId: serverId,
    localName: remoteToolName,
    publicName,
    capabilityBase,
  });
}
