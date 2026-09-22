import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";
import {
  assertOnDemandCoreToolNamesSound,
  BUILT_IN_PERMISSION_GATEWAY_TOOL_NAMES,
  CORE_TOOL_NAMES,
  FILE_TOOL_READ_ACTIONS,
  FIRST_PARTY_DEFERRED_PERMISSION_CONTRACTS,
  firstPartyDeferredInvocation,
  ONDEMAND_CORE_TOOL_NAMES,
  OPTIONAL_TOOL_NAMES,
  RESIDENT_CORE_TOOL_NAMES,
  SESSION_FOLDERS_TOOL_READ_ACTIONS,
  STANDARD_TOOL_NAMES,
} from "../shared/tool-categories.ts";
import {
  classifySessionPermission,
  FILE_READ_ACTIONS,
  SESSION_FOLDERS_READ_ACTIONS,
  SESSION_PERMISSION_MODES,
} from "../core/session-permission-mode.ts";

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
  agentConfig = {},
}: {
  tools?: any[];
  builtinDefer?: boolean;
  mcpTools?: number;
  agentConfig?: Record<string, unknown>;
} = {}) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-on-demand-"));
  const agentDir = path.join(tmpDir, "agents", "focus");
  const workspace = path.join(tmpDir, "workspace");
  fs.mkdirSync(workspace, { recursive: true });
  const agent = { id: "focus", agentDir, config: agentConfig, tools };

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

// ── P03-T01：合成契约镜像与宿主分类器一致性 ──────────────────────────────
// shared 层不依赖 core，动作集合靠字面镜像；这里同时锁定“字面集合相等”和
// “行为语义永不宽于直载路径”两层，任何一侧改动而另一侧未跟都会变红。
describe("P03-T01 deferred permission contract mirrors the host classifier", () => {
  const MODES = [
    SESSION_PERMISSION_MODES.READ_ONLY,
    SESSION_PERMISSION_MODES.AUTO,
    SESSION_PERMISSION_MODES.ASK,
    SESSION_PERMISSION_MODES.OPERATE,
  ] as const;
  // 宽松度排序：deny < prompt < review < allow（延迟路径只能等于或更严）。
  const PERMISSIVENESS: Record<string, number> = {
    deny: 0,
    prompt: 1,
    review: 2,
    allow: 3,
  };
  const PROBE_ACTIONS = ["stat", "list", "read", "?", "write", "copy", "move", "send", ""];

  function deferredDecision(name: string, params: unknown) {
    const descriptor = firstPartyDeferredInvocation(name, params);
    expect(descriptor).toBeTruthy();
    return MODES.map((mode) => classifySessionPermission({
      mode,
      toolName: name,
      params: {},
      context: { toolInvocation: descriptor },
    }).action);
  }

  function directDecisions(name: string, params: unknown) {
    return MODES.map((mode) => classifySessionPermission({
      mode,
      toolName: name,
      params,
      context: {},
    }).action);
  }

  function allAllow(actions: string[]) {
    return actions.every((action) => action === "allow");
  }

  it("keeps the literal read-action mirrors identical to the classifier sets", () => {
    expect([...FILE_TOOL_READ_ACTIONS].sort()).toEqual([...FILE_READ_ACTIONS].sort());
    expect([...SESSION_FOLDERS_TOOL_READ_ACTIONS].sort())
      .toEqual([...SESSION_FOLDERS_READ_ACTIONS].sort());
  });

  it("never classifies a synthetic contract more permissive than the direct path", () => {
    for (const [name, kind] of Object.entries(FIRST_PARTY_DEFERRED_PERMISSION_CONTRACTS)) {
      const paramSets = kind === "file" || kind === "session-folders"
        ? PROBE_ACTIONS.map((action) => ({ action }))
        : [{}];
      for (const params of paramSets) {
        const deferred = deferredDecision(name, params);
        const direct = directDecisions(name, params);
        deferred.forEach((action, index) => {
          expect(
            PERMISSIVENESS[action] ?? -1,
            `${name} ${JSON.stringify(params)} mode=${MODES[index]}: deferred ${action} vs direct ${direct[index]}`,
          ).toBeLessThanOrEqual(PERMISSIVENESS[direct[index]] ?? -1);
        });
        // read 类合成契约承诺“直载路径全模式放行”，因此两侧必须同为 allow。
        if (kind === "read") {
          expect(allAllow(deferred), name).toBe(true);
          expect(allAllow(direct), name).toBe(true);
        }
      }
    }
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

  // ── P03-T04-3：schema 无法安全消费时显式回退常驻（warn，不静默、不盲延迟）──
  it("keeps a tool resident with a warning when its schema cannot be consumed safely", () => {
    // properties 值不是 schema 节点：注册期元 schema 检查即抛错，JSON 往返后
    // 仍抛——该工具显式回退常驻（带 warn 日志），绝不带病延迟；它自带
    // resolver，启动断言（权限覆盖）仍满足。
    const unvalidatable = firstPartyTool("notify", { kind: "review" });
    // 故意构造非法 schema（properties 值非节点）；类型系统如实拒绝，这里显式越型注入。
    (unvalidatable as { parameters: unknown }).parameters = {
      type: "object",
      properties: { query: "not-a-schema-node" },
      required: ["query"],
    };
    const deferred = firstPartyTool("knowledge_search", { kind: "review" });
    const made = make({ tools: [unvalidatable, deferred] });
    const { customTools, toolCatalogManifest } = made.build();
    const names = customTools.map((tool: any) => tool.name);

    // notify 保持直载可见（不进目录、不盲延迟），knowledge_search 正常延迟。
    expect(names).toContain("notify");
    expect(names).not.toContain("knowledge_search");
    for (const bridge of BRIDGE_NAMES) expect(names).toContain(bridge);
    expect(toolCatalogManifest.text).toContain("knowledge_search");
    expect(toolCatalogManifest.text).not.toContain("notify");
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

  // ── P03-A07：目录按会话隔离，两个 agent 的禁用列表互不泄漏 ──
  it("isolates per-agent catalogs: a disabled tool never leaks across agents (P03-A07)", async () => {
    const officeFor = () => firstPartyTool("office", { kind: "review" });
    const agentA = makeEngine({ tools: [officeFor()], agentConfig: {} });
    const agentB = makeEngine({
      tools: [officeFor()],
      agentConfig: { tools: { disabled: ["office"] } },
    });
    dirs.push(agentA.tmpDir, agentB.tmpDir);
    const builtA = agentA.build();
    const builtB = agentB.build();

    // A 可见 office；B 的目录与清单都不含它（engine 对不可用目标直接不注册）。
    expect(builtA.toolCatalogManifest.text).toContain("office");
    expect(builtB.toolCatalogManifest.text).not.toContain("office");
    expect(builtA.toolTargetRegistry.resolveCatalogTarget({ toolName: "office" })).toBeTruthy();
    expect(() => builtB.toolTargetRegistry.resolveCatalogTarget({ toolName: "office" }))
      .toThrow(expect.objectContaining({ code: "TARGET_NOT_FOUND" }));

    const search = (built: ReturnType<typeof agentA.build>, made: { sessionPath: string }) => {
      const searchTool = built.customTools.find((entry: any) => entry.name === "mcp_search_tools");
      return searchTool.execute("s", { query: "office" }, {
        sessionPath: made.sessionPath,
        sessionManager: { getSessionFile: () => made.sessionPath },
      });
    };
    const aFirst = await search(builtA, agentA);
    // 先 A 查询、再 B 查询、再回 A：A 的结果不受 B 的禁用影响，B 永远查不到。
    await search(builtB, agentB);
    const aAgain = await search(builtA, agentA);
    expect(((aFirst as any).content[0].text as string)).toContain("office");
    expect(((aAgain as any).content[0].text as string)).toBe(((aFirst as any).content[0].text as string));

    const officeB = officeFor();
    const grantedB = makeEngine({
      tools: [officeB],
      agentConfig: { tools: { disabled: ["office"] } },
    });
    dirs.push(grantedB.tmpDir);
    grantedB.engine.getSessionAllowedInvocationCapabilities = () => ["office.execute"];
    const builtGranted = grantedB.build();
    const callTool = builtGranted.customTools.find((entry: any) => entry.name === "mcp_call");
    await callTool.execute("call-b", { tool: "office", arguments: { query: "x" } }, {
      sessionPath: grantedB.sessionPath,
      sessionManager: { getSessionFile: () => grantedB.sessionPath },
    });
    // B 即使握有同名 capability 授权，也执行不到未注册目标（0 执行）。
    expect(officeB.execute).not.toHaveBeenCalled();
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
