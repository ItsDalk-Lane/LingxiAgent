import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import {
  createPluginToolIdentity,
  getPreparedInvocation,
  normalizeToolPermissionContract,
  runWithPreparedInvocation,
  ToolInvocationError,
} from "../lib/tools/invocation/index.ts";

const BRIDGE_NAMES = ["mcp_search_tools", "mcp_describe_tool", "mcp_call"];

type ToolOptions = {
  readOnly?: boolean;
  deferrable?: boolean;
  pinned?: boolean;
  enabled?: boolean;
  newDialect?: boolean;
};

type PluginToolFixture = ReturnType<typeof pluginTool>;
type ModelTool = {
  name: string;
  execute: (...args: unknown[]) => Promise<unknown>;
  sessionPermission?: { resolveInvocation?: (input: unknown) => unknown };
};
type TextToolResult = { content: Array<{ type: string; text: string }> };

function pluginTool(pluginId: string, localName: string, options: ToolOptions = {}) {
  const publicName = `${pluginId}_${localName}`;
  const identity = createPluginToolIdentity({
    pluginId,
    publicName,
    capabilityBase: localName,
  });
  const permission = normalizeToolPermissionContract({
    name: publicName,
    sessionPermission: options.newDialect
      ? {
        resolveInvocation: () => ({
          action: options.readOnly === false ? "execute" : "read",
          kind: options.readOnly === false ? "review" : "read",
          capability: `${localName}.${options.readOnly === false ? "execute" : "read"}`,
          ...(options.readOnly === false
            ? { sideEffect: { kind: "plugin_output", summary: `Run ${publicName}` } }
            : {}),
        }),
      }
      : options.readOnly === false
        ? {
          kind: "external_side_effect",
          describeSideEffect: () => ({ kind: "plugin_output", summary: `Run ${publicName}` }),
        }
        : { readOnly: true },
  }, identity);
  const execute = vi.fn(async (
    toolCallId: string,
    args: Record<string, unknown>,
    signal: AbortSignal | undefined,
    onUpdate: unknown,
    ctx: unknown,
  ) => ({
    content: [{ type: "text", text: `${publicName}:${String(args.value ?? "")}` }],
    details: { toolCallId, signal, onUpdate, ctx, prepared: getPreparedInvocation() },
  }));
  return {
    name: publicName,
    label: publicName,
    description: `Fixture ${publicName}`,
    parameters: {
      type: "object",
      properties: { value: { type: "string" } },
      additionalProperties: false,
    },
    deferrable: options.deferrable !== false,
    pinned: options.pinned === true,
    _pluginId: pluginId,
    _toolTargetIdentity: identity,
    _normalizedPermissionContract: permission,
    sessionPermission: { resolveInvocation: permission.resolveInvocation },
    isEnabledForAgentConfig: () => options.enabled !== false,
    execute,
  };
}

function makeEngine(pluginTools: PluginToolFixture[], {
  deferEnabled = true,
  deferThreshold = 1,
  builtinDefer = true,
  permissionMode = "operate",
}: {
  deferEnabled?: boolean;
  deferThreshold?: number;
  builtinDefer?: boolean;
  permissionMode?: string;
} = {}) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-builtin-parity-"));
  const agentDir = path.join(tmpDir, "agents", "focus");
  const workspace = path.join(tmpDir, "workspace");
  const sessionPath = path.join(agentDir, "sessions", "main.jsonl");
  const agent = { id: "focus", agentDir, config: { tools: { disabled: [] } }, tools: [] };
  const engine = Object.create(LingxiEngine.prototype);
  engine.lingxiHome = tmpDir;
  engine.getAgent = vi.fn(() => agent);
  engine._agentMgr = { agent };
  engine._pluginManager = { getAllTools: () => pluginTools };
  engine._mcp = {
    getAllTools: () => [],
    getConfig: () => ({ enabled: true, deferEnabled, deferThreshold, connectors: [] }),
    getCatalogEntries: () => [],
    resolveToolPermissionKind: () => "review",
    callTool: vi.fn(async () => {
      throw new Error("raw MCP execution is outside this fixture");
    }),
  };
  engine._prefs = {
    getFileBackup: () => ({ enabled: false }),
    getBuiltinToolDeferEnabled: () => builtinDefer,
  };
  engine._readPreferences = () => ({ sandbox: true });
  engine._confirmStore = null;
  engine._approvalGateway = null;
  engine._emitEvent = vi.fn();
  engine.getSessionPermissionMode = vi.fn(() => permissionMode);
  engine.getSessionAllowedInvocationCapabilities = vi.fn(() => []);
  engine.getSessionIdForPath = vi.fn(() => "session-1");

  const result = engine.buildTools(workspace, [], {
    agentDir,
    workspace,
    getSessionPath: () => sessionPath,
    getPermissionMode: () => permissionMode,
  });
  return { engine, result, tmpDir, sessionPath };
}

async function invoke(
  tools: ModelTool[],
  name: string,
  args: Record<string, unknown>,
  sessionPath: string,
  handles: { signal?: AbortSignal; onUpdate?: unknown } = {},
): Promise<TextToolResult> {
  const tool = tools.find((candidate) => candidate.name === name);
  expect(tool, `missing tool ${name}`).toBeTruthy();
  return await tool.execute(
    "call-original",
    args,
    handles.signal,
    handles.onUpdate,
    { sessionPath, sessionId: "session-1", agentId: "focus" },
  ) as TextToolResult;
}

describe("deferred bundled plugin parity", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  function build(tools: PluginToolFixture[], options: Parameters<typeof makeEngine>[1] = {}) {
    const made = makeEngine(tools, options);
    dirs.push(made.tmpDir);
    return made;
  }

  it("filters unavailable plugin targets before counting the defer threshold", () => {
    const tools = Array.from({ length: 12 }, (_, index) => pluginTool(
      "office",
      `tool-${index}`,
      { enabled: index === 0 },
    ));
    const { result } = build(tools, { deferThreshold: 10 });
    const names = result.customTools.map((tool: { name?: string }) => tool.name);

    expect(names).toContain("office_tool-0");
    for (const bridgeName of BRIDGE_NAMES) expect(names).not.toContain(bridgeName);
    expect(result.toolCatalogManifest).toBeNull();
  });

  it("keeps agent-disabled office and beautify targets out of search, describe, and call", async () => {
    const enabled = Array.from({ length: 11 }, (_, index) => pluginTool("demo", `tool-${index}`));
    const office = pluginTool("office", "read-document", { enabled: false });
    const beautify = pluginTool("beautify", "create-cover", { enabled: false, readOnly: false });
    const { result, sessionPath } = build([...enabled, office, beautify], { deferThreshold: 10 });

    const search = await invoke(
      result.customTools,
      "mcp_search_tools",
      { query: "office beautify" },
      sessionPath,
    );
    const describe = await invoke(
      result.customTools,
      "mcp_describe_tool",
      { name: office.name },
      sessionPath,
    );
    const call = await invoke(result.customTools, "mcp_call", {
      server: "office",
      tool: office.name,
      arguments: { value: "blocked" },
    }, sessionPath);

    expect(search.content[0].text).toContain("No matching external tool");
    expect(describe.content[0].text).toContain(`No tool named ${office.name}`);
    expect(call).toMatchObject({
      isError: true,
      details: { errorCode: "TOOL_INVOCATION_RESOLVER_FAILED" },
    });
    expect(office.execute).not.toHaveBeenCalled();
    expect(beautify.execute).not.toHaveBeenCalled();
  });

  it("keeps pinned and non-deferrable plugin tools on the direct surface", async () => {
    const pinned = pluginTool("demo", "pinned", { pinned: true });
    const fixed = pluginTool("demo", "fixed", { deferrable: false });
    const deferred = Array.from({ length: 3 }, (_, index) => pluginTool("demo", `later-${index}`));
    const { result, sessionPath } = build([pinned, fixed, ...deferred], { deferThreshold: 1 });
    const names = result.customTools.map((tool: { name?: string }) => tool.name);

    expect(names).toContain(pinned.name);
    expect(names).toContain(fixed.name);
    expect(names).not.toContain(deferred[0].name);
    for (const bridgeName of BRIDGE_NAMES) expect(names).toContain(bridgeName);

    await invoke(result.customTools, "mcp_call", {
      server: "demo",
      tool: pinned.name,
      arguments: { value: "catalog-too" },
    }, sessionPath);
    expect(pinned.execute).toHaveBeenCalledOnce();
  });

  it("makes direct and deferred read-only calls preserve result and invocation handles", async () => {
    const directTarget = pluginTool("demo", "read", { readOnly: true });
    const direct = build([directTarget], { deferEnabled: false, permissionMode: "read_only" });
    const directController = new AbortController();
    const directUpdate = vi.fn();
    const directResult = await invoke(
      direct.result.customTools,
      directTarget.name,
      { value: "same" },
      direct.sessionPath,
      { signal: directController.signal, onUpdate: directUpdate },
    );

    const deferredTarget = pluginTool("demo", "read", { readOnly: true });
    const filler = pluginTool("demo", "other", { readOnly: true });
    const deferred = build([deferredTarget, filler], { deferThreshold: 1, permissionMode: "read_only" });
    const deferredController = new AbortController();
    const deferredUpdate = vi.fn();
    const deferredResult = await invoke(
      deferred.result.customTools,
      "mcp_call",
      { server: "demo", tool: deferredTarget.name, arguments: { value: "same" } },
      deferred.sessionPath,
      { signal: deferredController.signal, onUpdate: deferredUpdate },
    );

    expect(deferredResult.content).toEqual(directResult.content);
    expect(deferredTarget.execute).toHaveBeenCalledWith(
      "call-original",
      { value: "same" },
      deferredController.signal,
      deferredUpdate,
      expect.objectContaining({
        invocationRoute: "deferred",
        effectiveTargetId: deferredTarget._toolTargetIdentity.targetId,
      }),
    );
  });

  it("binds deferred legacy side effects and new-dialect capabilities to the true target", () => {
    const legacy = pluginTool("beautify", "create-cover", { readOnly: false });
    const modern = pluginTool("media", "generate-image", { readOnly: false, newDialect: true });
    const { result } = build([legacy, modern], { deferThreshold: 1 });
    const bridge = result.customTools.find((tool: ModelTool) => tool.name === "mcp_call");

    for (const target of [legacy, modern]) {
      const descriptor = bridge.sessionPermission.resolveInvocation({
        server: target._pluginId,
        tool: target.name,
        arguments: { value: "x" },
      });
      expect(descriptor).toMatchObject({
        capability: `${target._toolTargetIdentity.capabilityBase}.execute`,
        kind: "review",
        effectiveInvocation: {
          targetId: target._toolTargetIdentity.targetId,
          toolName: target.name,
          arguments: { value: "x" },
        },
      });
      expect(descriptor).toMatchObject({
        ...target._normalizedPermissionContract.resolveInvocation({ value: "x" }),
      });
      expect(descriptor.capability).not.toContain(`${target._pluginId}_${target._pluginId}_`);
    }
  });

  it("routes the seven read-only and five side-effect bundled contracts through deferred targets", async () => {
    const readonlyNames = [
      ["media", "describe-options"],
      ["media", "get-guide"],
      ["beautify", "get-cover-style-guide"],
      ["beautify", "get-html-style-guide"],
      ["beautify", "list-capabilities"],
      ["office", "list-capabilities"],
      ["office", "read-document"],
    ] as const;
    const sideEffectNames = [
      ["media", "generate-image"],
      ["media", "generate-video"],
      ["beautify", "apply-cover-candidate"],
      ["beautify", "create-cover"],
      ["office", "html-to-pdf"],
    ] as const;
    const targets = [
      ...readonlyNames.map(([pluginId, name]) => pluginTool(pluginId, name, { readOnly: true })),
      ...sideEffectNames.map(([pluginId, name]) => pluginTool(pluginId, name, { readOnly: false })),
    ];
    const { result, sessionPath } = build(targets, { deferThreshold: 10, permissionMode: "operate" });

    for (const target of targets) {
      const output = await invoke(result.customTools, "mcp_call", {
        server: target._pluginId,
        tool: target.name,
        arguments: { value: "ok" },
      }, sessionPath);
      expect(output).toMatchObject({
        content: [{ type: "text", text: `${target.name}:ok` }],
      });
      expect(target.execute).toHaveBeenCalledOnce();
    }
  });

  it("does not retain the raw builtinToolsByName execution map", () => {
    const source = fs.readFileSync(path.resolve("core/engine.ts"), "utf8");
    expect(source).not.toContain("builtinToolsByName");
  });

  it("rejects argument replacement and approval reuse across assembled plugin targets", async () => {
    const first = pluginTool("demo", "first", { readOnly: false });
    const second = pluginTool("demo", "second", { readOnly: false });
    const { result } = build([first, second], { deferEnabled: false });
    const registry = result.toolTargetRegistry;
    const gateway = result.toolInvocationGateway;
    const firstTarget = registry.getByTargetId(first._toolTargetIdentity.targetId);
    const request = {
      targetId: first._toolTargetIdentity.targetId,
      route: "direct" as const,
      arguments: { value: "approved" },
      sessionId: "session-1",
      sessionPath: "/sessions/one.jsonl",
      agentId: "focus",
      lifecycleGeneration: firstTarget.getCurrentGeneration(),
      toolCallId: "approved-call",
    };
    const prepared = gateway.resolvePermission(request);

    await expect(runWithPreparedInvocation(prepared, () => gateway.invoke({
      ...request,
      arguments: { value: "replaced" },
    }))).rejects.toMatchObject({
      code: "PREPARED_INVOCATION_MISMATCH",
    } satisfies Partial<ToolInvocationError>);
    await expect(runWithPreparedInvocation(prepared, () => gateway.invoke({
      ...request,
      targetId: second._toolTargetIdentity.targetId,
      lifecycleGeneration: registry.getByTargetId(second._toolTargetIdentity.targetId).getCurrentGeneration(),
    }))).rejects.toMatchObject({
      code: "PREPARED_INVOCATION_MISMATCH",
    } satisfies Partial<ToolInvocationError>);
    expect(first.execute).not.toHaveBeenCalled();
    expect(second.execute).not.toHaveBeenCalled();
  });
});
