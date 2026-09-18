/**
 * PTC 引擎装配集成测试（沿 on-demand-first-party 的 Object.create 桩）：
 * - run_tools 由 buildTools 自造、进目录、不占直挂面；
 * - 模型经 mcp_call 触达 run_tools（deferred 路径）；
 * - 程序内的子调用重走包装后工具面（直挂工具命中包装后的 execute、
 *   合成子调用 ID <外层>:ptc:<n>、结果按 PTC 语义只回 console+return）。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LingxiEngine } from "../core/engine.ts";

function firstPartyTool(name: string, options: { kind?: "read" | "review" } = {}) {
  const kind = options.kind ?? "read";
  const action = kind === "review" ? "execute" : "read";
  return {
    name,
    label: name,
    description: `First-party tool ${name}.`,
    parameters: {
      type: "object",
      properties: { query: { type: "string", description: "lookup key" } },
      required: ["query"],
    },
    sessionPermission: {
      resolveInvocation: () => ({ action, kind, capability: `${name}.${action}` }),
    },
    execute: vi.fn(async (_id: string, args: Record<string, unknown>) => ({
      content: [{ type: "text", text: `ran ${name}:${String(args?.query ?? "")}` }],
    })),
  };
}

function makeEngine({ tools = [] }: { tools?: any[] } = {}) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-ptc-assembly-"));
  const agentDir = path.join(tmpDir, "agents", "focus");
  const workspace = path.join(tmpDir, "workspace");
  fs.mkdirSync(workspace, { recursive: true });
  const agent = { id: "focus", agentDir, config: {}, tools };

  const engine = Object.create(LingxiEngine.prototype);
  engine.lingxiHome = tmpDir;
  engine.getAgent = vi.fn(() => agent);
  engine._pluginManager = null;
  engine._mcp = {
    getAllTools: () => [],
    getToolTargetDescriptors: () => [],
    getConfig: () => ({ enabled: true, deferEnabled: true, deferThreshold: 10, connectors: [] }),
    resolveToolPermissionKind: () => "read",
  };
  engine._prefs = {
    getFileBackup: () => ({ enabled: false }),
    getBuiltinToolDeferEnabled: () => true,
  };
  engine._readPreferences = () => ({ sandbox: true });
  engine._confirmStore = null;
  engine._emitEvent = vi.fn();
  engine.getSessionPermissionMode = vi.fn(() => "operate");
  engine._agentMgr = { agent };

  const sessionPath = path.join(agentDir, "sessions", "main.jsonl");
  const build = (extra: Record<string, unknown> = {}) => engine.buildTools(workspace, null, {
    agentDir,
    workspace,
    getSessionPath: () => sessionPath,
    getPermissionMode: () => "operate",
    allowHumanApproval: false,
    ...extra,
  });
  return { engine, build, tmpDir, sessionPath, workspace };
}

describe("PTC 引擎装配", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  it("run_tools 进目录、不占直挂面", () => {
    const made = makeEngine({ tools: [firstPartyTool("knowledge_search")] });
    dirs.push(made.tmpDir);
    const result = made.build();
    const directNames = [...result.tools, ...result.customTools].map((tool) => tool?.name);
    expect(directNames).not.toContain("run_tools");
    expect(directNames).toContain("mcp_call");
    expect(result.toolCatalogManifest).toBeTruthy();
    const manifestText = typeof result.toolCatalogManifest === "string"
      ? result.toolCatalogManifest
      : JSON.stringify(result.toolCatalogManifest);
    expect(manifestText).toContain("run_tools");
    expect(manifestText).toContain("knowledge_search");
  });

  it("端到端：mcp_call → run_tools → 程序调直挂工具（合成子调用 ID）", async () => {
    const echo = {
      name: "ptc_echo_probe",
      label: "ptc_echo_probe",
      description: "PTC probe tool.",
      parameters: { type: "object", properties: { v: { type: "number" } } },
      sessionPermission: {
        resolveInvocation: () => ({ action: "read", kind: "read", capability: "ptc_echo_probe.read" }),
      },
      execute: vi.fn(async (_id: string, args: Record<string, unknown>) => ({
        content: [{ type: "text", text: `echo:${String(args?.v ?? "")}` }],
      })),
    };
    const made = makeEngine({ tools: [firstPartyTool("knowledge_search")] });
    dirs.push(made.tmpDir);
    // extraCustomTools 不在第一方延迟注册循环里，天然留在直挂面。
    const result = made.build({ extraCustomTools: [echo] });
    const directNames = [...result.tools, ...result.customTools].map((tool) => tool?.name);
    expect(directNames).toContain("ptc_echo_probe");

    const bridge = result.customTools.find((tool) => tool?.name === "mcp_call");
    expect(bridge).toBeTruthy();
    const outer = await bridge.execute("tc-outer", {
      tool: "run_tools",
      arguments: {
        code: "const a = await tools.ptc_echo_probe({ v: 7 });\nconst b = await tools.ptc_echo_probe({ v: 8 });\nreturn a + ' & ' + b;",
        description: "probe ptc loop",
      },
    }, undefined, undefined, { sessionPath: made.sessionPath });

    expect(outer?.isError).toBeUndefined();
    const text = outer?.content?.[0]?.text ?? "";
    expect(text).toContain("echo:7 & echo:8");
    expect(echo.execute).toHaveBeenCalledTimes(2);
    expect(echo.execute.mock.calls.map((call: any[]) => call[0])).toEqual(["tc-outer:ptc:1", "tc-outer:ptc:2"]);
    // 子调用明细随 details 出栈（UI 展开清单的数据源），但不进正文
    expect(outer.details.subcalls).toHaveLength(2);
    expect(text).not.toContain('"subcalls"');
  }, 20_000);

  it("端到端：程序调目录工具（deferred 目标经桥执行）", async () => {
    const knowledge = firstPartyTool("knowledge_search");
    const made = makeEngine({ tools: [knowledge] });
    dirs.push(made.tmpDir);
    const result = made.build();
    const bridge = result.customTools.find((tool) => tool?.name === "mcp_call");
    const outer = await bridge.execute("tc-outer", {
      tool: "run_tools",
      arguments: {
        code: "return await tools.knowledge_search({ query: 'inside-ptc' });",
        description: "reach catalog tool",
      },
    }, undefined, undefined, { sessionPath: made.sessionPath });

    expect(outer?.isError).toBeUndefined();
    expect(outer?.content?.[0]?.text ?? "").toContain("ran knowledge_search:inside-ptc");
    expect(knowledge.execute).toHaveBeenCalledTimes(1);
  }, 20_000);

  it("端到端：只读模式拒绝 run_tools 进入执行（外层契约生效）", async () => {
    const made = makeEngine({ tools: [firstPartyTool("knowledge_search")] });
    dirs.push(made.tmpDir);
    const result = made.build({ getPermissionMode: () => "read_only" });
    const bridge = result.customTools.find((tool) => tool?.name === "mcp_call");
    const outer = await bridge.execute("tc-outer", {
      tool: "run_tools",
      arguments: { code: "return 1;", description: "read-only probe" },
    }, undefined, undefined, { sessionPath: made.sessionPath });
    expect(outer?.isError).toBe(true);
  });
});
