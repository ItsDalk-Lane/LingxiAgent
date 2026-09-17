import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import {
  assertOnDemandCoreToolNamesSound,
  BUILT_IN_PERMISSION_GATEWAY_TOOL_NAMES,
  CORE_TOOL_NAMES,
  FIRST_PARTY_DEFERRED_PERMISSION_CONTRACTS,
  firstPartyDeferredInvocation,
  ONDEMAND_CORE_TOOL_NAMES,
  OPTIONAL_TOOL_NAMES,
  RESIDENT_CORE_TOOL_NAMES,
  STANDARD_TOOL_NAMES,
} from "../shared/tool-categories.ts";

const BRIDGE_NAMES = ["mcp_search_tools", "mcp_describe_tool", "mcp_call"];
const RESIDENT_FOUR = ["edit", "exec_command", "read", "write"];

/**
 * A first-party tool in the shape the agent snapshot hands to buildTools:
 * name from ONDEMAND_CORE_TOOL_NAMES, its own invocation resolver, and the
 * same fields the direct surface reads.
 */
function firstPartyTool(
  name: string,
  options: { resolver?: unknown; description?: string; kind?: "read" | "review" } = {},
) {
  const kind = options.kind ?? "read";
  const action = kind === "review" ? "execute" : "read";
  return {
    name,
    label: name,
    description: options.description ?? `First-party tool ${name}.`,
    parameters: {
      type: "object",
      properties: { query: { type: "string", description: "lookup key" } },
      required: ["query"],
    },
    sessionPermission: options.resolver === undefined
      ? {
        // review 类调用必须持真实目标的授权才放行，read 类在只读会话自动允许；
        // 授权串用测试必须用 review 类，否则测的是 read 自动放行而不是授权边界。
        resolveInvocation: () => ({
          action,
          kind,
          capability: `${name}.${action}`,
        }),
      }
      : options.resolver,
    execute: vi.fn(async (_id: string, args: Record<string, unknown>) => ({
      content: [{ type: "text", text: `ran ${name}:${String(args?.query ?? "")}` }],
    })),
  };
}

/**
 * Engine stub mirroring tests/engine-tool-defer.test.ts, with first-party tools
 * arriving through the agent snapshot and the sandbox layer building the real
 * Pi primitives (whose deferral is exactly what this suite pins).
 */
function makeEngine({
  tools = [],
  builtinDefer = true,
  mcpTools = 0,
}: {
  tools?: any[];
  builtinDefer?: boolean;
  mcpTools?: number;
} = {}) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-on-demand-"));
  const agentDir = path.join(tmpDir, "agents", "focus");
  const workspace = path.join(tmpDir, "workspace");
  fs.mkdirSync(workspace, { recursive: true });
  const agent = { id: "focus", agentDir, config: {}, tools };

  // Raw MCP tools below the default threshold keep the MCP source from
  // deferring, so the catalog exercises the first-party source alone.
  const published = Array.from({ length: mcpTools }, (_, index) => ({
    name: `mcp_srv_t_${index}`,
    description: `MCP tool ${index}.`,
    parameters: { type: "object", properties: {} },
    _pluginId: "mcp",
    metadata: { kind: "mcp", connectorId: "srv", toolName: `t_${index}` },
    sessionPermission: {
      resolveInvocation: () => ({ action: "read", kind: "read", capability: `srv_t_${index}.read` }),
    },
    execute: vi.fn(async () => ({ content: [{ type: "text", text: "ok" }] })),
  }));

  const engine = Object.create(LingxiEngine.prototype);
  engine.lingxiHome = tmpDir;
  engine.getAgent = vi.fn(() => agent);
  engine._pluginManager = null;
  engine._mcp = {
    getAllTools: () => published,
    getToolTargetDescriptors: () => [],
    getConfig: () => ({ enabled: true, deferEnabled: true, deferThreshold: 10, connectors: [] }),
    resolveToolPermissionKind: () => "read",
  };
  engine._prefs = {
    getFileBackup: () => ({ enabled: false }),
    getBuiltinToolDeferEnabled: () => builtinDefer,
  };
  engine._readPreferences = () => ({ sandbox: true });
  engine._confirmStore = null;
  engine._emitEvent = vi.fn();
  engine.getSessionPermissionMode = vi.fn(() => "operate");
  engine._agentMgr = { agent };

  const sessionPath = path.join(agentDir, "sessions", "main.jsonl");
  // customTools 传 null（而非 []）：空数组是真值，会短路 agentDir 反查，
  // 第一方工具必须经 agent.tools 快照流入 buildTools 才贴近真实链路。
  const build = (extra: Record<string, unknown> = {}) => engine.buildTools(workspace, null, {
    agentDir,
    workspace,
    getSessionPath: () => sessionPath,
    getPermissionMode: () => "read_only",
    allowHumanApproval: false,
    ...extra,
  });
  return { engine, build, tmpDir, sessionPath, workspace, published };
}

describe("on-demand partition invariants", () => {
  it("keeps the on-demand split sound", () => {
    expect(() => assertOnDemandCoreToolNamesSound()).not.toThrow();
  });

  it("keeps exactly the four original Pi tools resident", () => {
    expect([...RESIDENT_CORE_TOOL_NAMES].sort()).toEqual(RESIDENT_FOUR);
    for (const name of RESIDENT_CORE_TOOL_NAMES) {
      expect(CORE_TOOL_NAMES).toContain(name);
    }
  });

  it("defers every other categorized built-in by policy", () => {
    const onDemand = new Set(ONDEMAND_CORE_TOOL_NAMES);
    for (const name of ["grep", "find", "ls", "write_stdin", "materialize", "web_search", "web_fetch",
      "todo_write", "search_memory", "pin_memory", "unpin_memory", "computer", "session", "workflow"]) {
      expect(onDemand.has(name), name).toBe(true);
    }
    // Resident four never leak into the on-demand set.
    for (const name of RESIDENT_CORE_TOOL_NAMES) {
      expect(onDemand.has(name)).toBe(false);
    }
  });

  it("gives every non-resident gateway tool a synthetic permission contract", () => {
    // 网关表工具没有自有解析器；延迟调用靠合成契约授权，缺一个就等于把
    // 该工具静默留驻。read/write/edit 常驻所以不需要。
    for (const name of BUILT_IN_PERMISSION_GATEWAY_TOOL_NAMES) {
      if (RESIDENT_CORE_TOOL_NAMES.includes(name)) continue;
      expect(FIRST_PARTY_DEFERRED_PERMISSION_CONTRACTS[name as keyof typeof FIRST_PARTY_DEFERRED_PERMISSION_CONTRACTS],
        name).toBeTruthy();
    }
  });

  it("maps synthetic contracts to faithful levels", () => {
    // INFORMATION_TOOLS 成员全模式放行 → read；其余 execute（等于或严于直载）。
    expect(firstPartyDeferredInvocation("grep")).toMatchObject({ kind: "read", action: "read" });
    expect(firstPartyDeferredInvocation("web_search")).toMatchObject({ kind: "read" });
    expect(firstPartyDeferredInvocation("write_stdin")).toMatchObject({ kind: "review", action: "execute" });
    expect(firstPartyDeferredInvocation("computer")).toMatchObject({ kind: "review" });
    // file 参数感知：stat 只读，其余动作要审查。
    expect(firstPartyDeferredInvocation("file", { action: "stat" })).toMatchObject({ kind: "read" });
    expect(firstPartyDeferredInvocation("file", { action: "copy" })).toMatchObject({ kind: "review" });
    expect(firstPartyDeferredInvocation("file")).toBeNull();
    // 未知工具无契约（fail-closed 由引擎回退常驻）。
    expect(firstPartyDeferredInvocation("totally_unknown")).toBeNull();
  });
});

describe("first-party on-demand assembly", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  function make(options: Parameters<typeof makeEngine>[0]) {
    const made = makeEngine(options);
    dirs.push(made.tmpDir);
    return made;
  }

  it("keeps only the four Pi primitives on the direct surface", () => {
    const knowledge = firstPartyTool("knowledge_search");
    const todo = firstPartyTool("todo_write");
    const { build } = make({ tools: [knowledge, todo] });
    const { tools, customTools } = build();

    expect(tools.map((tool: any) => tool.name).sort()).toEqual(RESIDENT_FOUR);
    for (const bridge of BRIDGE_NAMES) {
      expect(customTools.map((tool: any) => tool.name)).toContain(bridge);
    }
    expect(customTools.map((tool: any) => tool.name)).not.toContain("knowledge_search");
    expect(customTools.map((tool: any) => tool.name)).not.toContain("todo_write");
  });

  it("catalogs deferred built-ins under the 内置 group with a usage intro", () => {
    const knowledge = firstPartyTool("knowledge_search", { description: "检索知识库段落。" });
    const { build } = make({ tools: [knowledge] });
    const { toolCatalogManifest } = build();

    expect(toolCatalogManifest).toBeTruthy();
    expect(toolCatalogManifest.text).toContain("内置");
    expect(toolCatalogManifest.text).toContain("knowledge_search");
    expect(toolCatalogManifest.text).toContain("grep");
    expect(toolCatalogManifest.text).toContain("mcp_search_tools");
    // The fingerprint tracks MCP+plugin names only; first-party rows are static
    // per build and must not turn an app upgrade into a catalog-changed broadcast.
    expect(toolCatalogManifest.names).not.toContain("knowledge_search");
  });

  it("keeps every tool direct when the builtin defer preference opts out", () => {
    const knowledge = firstPartyTool("knowledge_search");
    const todo = firstPartyTool("todo_write");
    const { build } = make({ tools: [knowledge, todo], builtinDefer: false });
    const { tools, customTools, toolCatalogManifest } = build();
    const names = customTools.map((tool: any) => tool.name);

    expect(names).toContain("knowledge_search");
    expect(names).toContain("todo_write");
    expect(tools.map((tool: any) => tool.name)).toContain("grep");
    for (const bridge of BRIDGE_NAMES) expect(names).not.toContain(bridge);
    expect(toolCatalogManifest).toBeNull();
  });

  it("defers a garbage resolver and fails closed at call time, never at build", async () => {
    // 契约构造是惰性的：解析器返回垃圾不会在装配期暴露，工具照常进目录；
    // 真正的边界在调用期——解析器给不出合法描述符就必须拒绝执行。
    const broken = firstPartyTool("notify", {
      resolver: { resolveInvocation: () => ({ nonsense: true }) },
    });
    const made = make({ tools: [broken] });
    const { customTools } = made.build();
    const names = customTools.map((tool: any) => tool.name);

    expect(names).not.toContain("notify");
    for (const bridge of BRIDGE_NAMES) expect(names).toContain(bridge);

    made.engine.getSessionAllowedInvocationCapabilities = () => ["notify.execute"];
    const callTool = customTools.find((entry: any) => entry.name === "mcp_call");
    await callTool.execute("call-broken", { tool: "notify", arguments: { query: "x" } }, {
      sessionPath: made.sessionPath,
      sessionManager: { getSessionFile: () => made.sessionPath },
    });
    expect(broken.execute).not.toHaveBeenCalled();
  });

  it("keeps MCP tools below their threshold direct while builtins defer", () => {
    // MCP 源与内置源独立把关：3 个连接器工具低于阈值保持直载，而内置按需
    // 已让目录存在——桥接工具就该在，目录只含内置行。
    const mcpMixed = make({ tools: [], mcpTools: 3 });
    const { customTools, toolCatalogManifest } = mcpMixed.build();
    const names = customTools.map((tool: any) => tool.name);

    expect(names).toContain("mcp_srv_t_0");
    for (const bridge of BRIDGE_NAMES) expect(names).toContain(bridge);
    expect(toolCatalogManifest).toBeTruthy();
    expect(toolCatalogManifest.text).toContain("内置");
    expect(toolCatalogManifest.text).not.toContain("mcp_srv_t_0");
  });
});

describe("deferred Pi primitive execution", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  it("runs the real sandbox grep through the bridge without a grant", async () => {
    // grep 的合成契约是 read（与宿主分类器 INFORMATION_TOOLS 一致），只读
    // 会话自动放行——延迟路径不能比直载路径更挑剔。
    const made = makeEngine({ tools: [] });
    dirs.push(made.tmpDir);
    fs.writeFileSync(path.join(made.workspace, "haystack.txt"), "needle in prose\n");
    const { customTools, sessionPath } = (() => {
      const built = made.build();
      return { customTools: built.customTools, sessionPath: made.sessionPath };
    })();

    const callTool = customTools.find((entry: any) => entry.name === "mcp_call");
    const result = await callTool.execute("call-grep", {
      tool: "grep",
      arguments: { pattern: "needle" },
    }, {
      sessionPath,
      sessionManager: { getSessionFile: () => sessionPath },
    });
    const text = (result as any).content.map((block: any) => block.text ?? "").join("\n");
    expect(text).toContain("haystack.txt");
    expect(text).toContain("needle");
  });

  it("describes a deferred Pi primitive from the catalog", async () => {
    const made = makeEngine({ tools: [] });
    dirs.push(made.tmpDir);
    const { customTools, sessionPath } = (() => {
      const built = made.build();
      return { customTools: built.customTools, sessionPath: made.sessionPath };
    })();

    const describeTool = customTools.find((entry: any) => entry.name === "mcp_describe_tool");
    const result = await describeTool.execute("d-grep", { name: "grep" }, {
      sessionPath,
      sessionManager: { getSessionFile: () => sessionPath },
    });
    const text = (result as any).content[0].text as string;
    expect(text).toContain("grep");
    expect(text).toContain("pattern");
  });
});

describe("first-party bridge invocation", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  function readOnlySessionWithGrants(grants: string[], tools: any[]) {
    const made = makeEngine({ tools });
    dirs.push(made.tmpDir);
    made.engine.getSessionAllowedInvocationCapabilities = () => grants;
    const { customTools } = made.build();
    return { made, customTools, sessionPath: made.sessionPath };
  }

  async function callBridge(customTools: any[], tool: string, sessionPath: string) {
    const callTool = customTools.find((entry: any) => entry.name === "mcp_call");
    expect(callTool).toBeTruthy();
    return callTool.execute("call-1", { tool, arguments: { query: "灵犀" } }, {
      sessionPath,
      sessionManager: { getSessionFile: () => sessionPath },
    });
  }

  it("executes a deferred built-in without a server qualifier on a real-target grant", async () => {
    const knowledge = firstPartyTool("knowledge_search", { kind: "review" });
    const { customTools, sessionPath } = readOnlySessionWithGrants(
      ["knowledge_search.execute"],
      [knowledge],
    );
    await callBridge(customTools, "knowledge_search", sessionPath);
    expect(knowledge.execute).toHaveBeenCalledTimes(1);
    expect((knowledge.execute as any).mock.calls[0][1]).toEqual({ query: "灵犀" });
  });

  it("resolves by name alone and rejects a grant issued for another tool", async () => {
    const knowledge = firstPartyTool("knowledge_search", { kind: "review" });
    const notify = firstPartyTool("notify", { kind: "review" });
    const { customTools, sessionPath } = readOnlySessionWithGrants(
      ["knowledge_search.execute"],
      [knowledge, notify],
    );
    // 权限拒绝以工具错误结果返回而不是抛异常，与 MCP 桥接既有语义一致：
    // 判据是真实目标从未被执行。
    await callBridge(customTools, "notify", sessionPath);
    expect(notify.execute).not.toHaveBeenCalled();
    expect(knowledge.execute).not.toHaveBeenCalled();
  });

  it("fails closed for a name that is not in the catalog", async () => {
    const knowledge = firstPartyTool("knowledge_search", { kind: "review" });
    const { customTools, sessionPath } = readOnlySessionWithGrants(
      ["knowledge_search.execute"],
      [knowledge],
    );
    await callBridge(customTools, "definitely_not_a_tool", sessionPath);
    expect(knowledge.execute).not.toHaveBeenCalled();
  });

  it("exposes the schema through describe without loading anything", async () => {
    const knowledge = firstPartyTool("knowledge_search", { description: "检索知识库段落。" });
    const { customTools, sessionPath } = readOnlySessionWithGrants([], [knowledge]);
    const describeTool = customTools.find((entry: any) => entry.name === "mcp_describe_tool");
    const result = await describeTool.execute("d-1", { name: "knowledge_search" }, {
      sessionPath,
      sessionManager: { getSessionFile: () => sessionPath },
    });
    const text = (result as any).content[0].text as string;
    expect(text).toContain("knowledge_search");
    expect(text).toContain("query");
    expect(knowledge.execute).not.toHaveBeenCalled();
  });

  it("finds deferred built-ins by search keywords", async () => {
    const knowledge = firstPartyTool("knowledge_search", { description: "检索知识库段落，支持过滤。" });
    const { customTools, sessionPath } = readOnlySessionWithGrants([], [knowledge]);
    const searchTool = customTools.find((entry: any) => entry.name === "mcp_search_tools");
    const result = await searchTool.execute("s-1", { query: "知识库" }, {
      sessionPath,
      sessionManager: { getSessionFile: () => sessionPath },
    });
    const text = (result as any).content[0].text as string;
    expect(text).toContain("knowledge_search");
  });
});
