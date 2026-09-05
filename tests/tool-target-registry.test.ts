import { describe, expect, it, vi } from "vitest";
import { ToolTargetRegistry } from "../core/tool-target-registry.ts";
import { createToolCatalog } from "../core/tool-catalog.ts";
import {
  createMcpToolIdentity,
  createPluginToolIdentity,
  createToolSchemaValidator,
  normalizeToolPermissionContract,
  ToolInvocationError,
  type ToolTargetIdentity,
} from "../lib/tools/invocation/index.ts";

function target(
  identity: ToolTargetIdentity,
  options: {
    eligible?: boolean;
    deferrable?: boolean;
    pinned?: boolean;
  } = {},
) {
  const permissionTool = {
    name: identity.publicName,
    ...(identity.origin === "plugin" ? { _pluginId: identity.sourceId } : {}),
    sessionPermission: { readOnly: true },
  };
  return {
    identity,
    label: identity.publicName,
    description: `Description for ${identity.publicName}`,
    parameters: { type: "object", additionalProperties: false },
    deferrable: options.deferrable ?? true,
    pinned: options.pinned ?? false,
    permission: normalizeToolPermissionContract(permissionTool, identity),
    validator: createToolSchemaValidator({ type: "object", additionalProperties: false }, identity),
    availability: {
      eligible: options.eligible ?? true,
      reason: options.eligible === false ? "disabled" : null,
    },
    getCurrentGeneration: () => 1,
    isCurrentlyAvailable: () => options.eligible ?? true,
    executeCanonical: vi.fn(async () => ({ ok: true })),
    normalizeResult: (result: unknown) => result,
  };
}

function mcpIdentity(serverId: string, remoteToolName: string, publicName = remoteToolName) {
  return createMcpToolIdentity({
    serverId,
    remoteToolName,
    publicName,
    capabilityBase: `${serverId}_${remoteToolName}`,
  });
}

function captureError(call: () => unknown, code: ToolInvocationError["code"]) {
  try {
    call();
  } catch (error) {
    expect(error).toBeInstanceOf(ToolInvocationError);
    expect(error).toMatchObject({ code });
    return error;
  }
  throw new Error(`expected ${code}`);
}

describe("会话级工具目标注册表", () => {
  it("以 TargetId 为主键并立即拒绝重复注册", () => {
    const registry = new ToolTargetRegistry();
    const registered = target(mcpIdentity("alpha", "search"));

    registry.register(registered);

    expect(registry.getByTargetId(registered.identity.targetId)).toBe(registered);
    expect(() => registry.register(registered)).toThrow();
  });

  it("跨来源同名时未限定来源会报歧义，限定来源后精确命中", () => {
    const registry = new ToolTargetRegistry();
    const alpha = target(mcpIdentity("alpha", "search", "search"));
    const beta = target(mcpIdentity("beta", "search", "search"));
    registry.register(alpha);
    registry.register(beta);

    captureError(
      () => registry.resolveCatalogTarget({ toolName: "search" }),
      "TARGET_AMBIGUOUS",
    );
    expect(registry.resolveCatalogTarget({ serverId: "alpha", toolName: "search" })).toBe(alpha);
    expect(registry.resolveCatalogTarget({ serverId: "beta", toolName: "search" })).toBe(beta);
  });

  it("同一来源的显示名重名仍报歧义，远端本名可分别解析", () => {
    const registry = new ToolTargetRegistry();
    const first = target(mcpIdentity("gamma", "remote_one", "shared"));
    const second = target(mcpIdentity("gamma", "remote_two", "shared"));
    registry.register(first);
    registry.register(second);

    captureError(
      () => registry.resolveCatalogTarget({ serverId: "gamma", toolName: "shared" }),
      "TARGET_AMBIGUOUS",
    );
    expect(registry.resolveCatalogTarget({ serverId: "gamma", toolName: "remote_one" })).toBe(first);
    expect(registry.resolveCatalogTarget({ serverId: "gamma", toolName: "remote_two" })).toBe(second);
  });

  it("按 capability 和 action 查找，不把同名 capability 静默选成第一个", () => {
    const registry = new ToolTargetRegistry();
    const unique = target(createPluginToolIdentity({
      pluginId: "office",
      publicName: "office_status",
      capabilityBase: "status",
    }));
    const alpha = target(mcpIdentity("alpha", "search", "shared"));
    const beta = target(mcpIdentity("beta", "search", "shared"));
    registry.register(unique);
    registry.register(alpha);
    registry.register(beta);

    expect(registry.findByCapability("status.read", "read")).toBe(unique);
    expect(registry.findByCapability("missing.read", "read")).toBeNull();
    captureError(
      () => registry.findByCapability("alpha_search.read", "write"),
      "CAPABILITY_MISMATCH",
    );
  });

  it("eligible 与 deferred 列表保留 pinned/deferrable，并排除装配时不可用目标", () => {
    const registry = new ToolTargetRegistry();
    const deferred = target(mcpIdentity("alpha", "deferred"));
    const pinned = target(mcpIdentity("alpha", "pinned"), { pinned: true });
    const nonDeferrable = target(mcpIdentity("alpha", "fixed"), { deferrable: false });
    const unavailable = target(mcpIdentity("alpha", "off"), { eligible: false });
    for (const item of [deferred, pinned, nonDeferrable, unavailable]) registry.register(item);

    expect(registry.listEligible()).toEqual([deferred, nonDeferrable, pinned]);
    expect(registry.listDeferredCandidates()).toEqual([deferred]);
  });

  it("找不到目标时返回稳定错误而不是 null", () => {
    const registry = new ToolTargetRegistry();

    captureError(
      () => registry.resolveCatalogTarget({ serverId: "missing", toolName: "search" }),
      "TARGET_NOT_FOUND",
    );
  });

  it("目录只返回 TargetId，再由注册表取得唯一的可执行目标", () => {
    const registry = new ToolTargetRegistry();
    const registered = target(mcpIdentity("alpha", "search", "shared_search"));
    registry.register(registered);
    const catalog = createToolCatalog();
    catalog.registerSource("mcp:alpha", [{
      targetId: registered.identity.targetId,
      origin: registered.identity.origin,
      sourceId: registered.identity.sourceId,
      serverId: "alpha",
      serverLabel: "Alpha",
      publicName: registered.identity.publicName,
      toolName: registered.identity.localName,
      capabilityBase: registered.identity.capabilityBase,
      description: registered.description,
      paramsSummary: "query (string, required)",
      schemaRef: () => registered.parameters,
      lifecycleGeneration: registered.getCurrentGeneration(),
      deferrable: registered.deferrable,
      pinned: registered.pinned,
    }]);

    const targetId = catalog.resolveTarget({ serverId: "alpha", toolName: "search" });
    expect(targetId).toBe(registered.identity.targetId);
    expect(registry.getByTargetId(targetId)).toBe(registered);
  });
});
