/**
 * P06-T03｜提示词预算回归与缓存/开关失效契约（任务书 §5 P06-T03）。
 *
 * 口径：P00 PROMPT_BASELINE（docs/refactor-2026/P00/PROMPT_BASELINE.json）固定
 * fixture + estimateTextTokens（CJK=1.1 token/字，其余 4 chars/token——估算值，
 * 非精确 tokenizer）。本文件把 P00 的 not_measured_yet 两项补齐：
 *
 *   D 常驻工具 schema 字节——用生产入口 engine.buildTools（Object.create 桩只
 *     替换环境依赖，工具面构成由生产代码计算，同 tests/ptc-engine-assembly
 *     .test.ts 模式）测得常驻面 name+description+parameters 的估值。
 *   E 动态用户资料——按 P06-T01 捕获样本索引单列（CONTEXT_ASSEMBLY_MAP §5），
 *     不入常驻预算。
 *
 * 预算规则（P00 budget_rule）：常驻平台开销（A+B+C+D）不得超过基线。A/B/C 由
 * golden fixture 与生产常量直接测量并与 PROMPT_BASELINE 数值对照；D 为本轮新增
 * 计量（基线未含），登记进预算账本供后续阶段回归，不以 D 抵扣 A/B/C。
 *
 * 失效契约（任务书 T03 第 3/4 项）：
 *   - 记忆 master 关闭立即重建 master prompt（无 memory_context 段残留）；
 *   - per-session 记忆关闭只影响该 session 自己的快照（新快照无记忆段，
 *     用户档案/人格正文原样保留——不删用户内容凑预算）；
 *   - skill 集变化重建 master prompt；
 *   - 权限/工具 generation 撤销由 tests/tool-lifecycle-revocation.test.ts +
 *     tests/on-demand-first-party.test.ts 保护（映射登记，不在本文件重复）。
 *   - 压缩后必要上下文保护由 tests/session-compactor-skill-recall.test.ts 等
 *     既有套件保护（映射登记）。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Agent } from "../core/agent.ts";
import { LingxiEngine } from "../core/engine.ts";
import { estimateTextTokens } from "../lib/llm/estimate-text-tokens.ts";
import { createPtcBindingHolder, createPtcTool, RUN_TOOLS_TOOL_NAME } from "../lib/tools/ptc-tool.ts";

const P00_BASELINE = JSON.parse(fs.readFileSync(
  path.join(process.cwd(), "docs", "refactor-2026", "P00", "PROMPT_BASELINE.json"),
  "utf-8",
));

function component(componentName: string) {
  const hit = (P00_BASELINE.components as any[]).find((c) => c.component === componentName);
  expect(hit, `P00 baseline component missing: ${componentName}`).toBeTruthy();
  return hit;
}

/* ── 预算回归：A/B/C 与 P00 基线数值对照 ───────────────────────────────── */

describe("P06-T03 resident prompt budget regression (P00 baseline)", () => {
  it("A1/A2 system prompt golden fixtures stay byte-identical to the P00 baseline", () => {
    const zh = fs.readFileSync(path.join(process.cwd(), "tests", "fixtures", "system-prompt-golden-zh.txt"), "utf-8");
    const en = fs.readFileSync(path.join(process.cwd(), "tests", "fixtures", "system-prompt-golden-en.txt"), "utf-8");
    const a1 = component("A1 系统提示词正文（golden zh，当前真实输出）");
    const a2 = component("A2 系统提示词正文（golden en，当前真实输出）");
    // bytes = UTF-8 字节数；估算 token 用同一 estimateTextTokens。
    expect(Buffer.byteLength(zh, "utf-8")).toBe(a1.bytes);
    expect(estimateTextTokens(zh)).toBe(a1.estimated_tokens);
    expect(Buffer.byteLength(en, "utf-8")).toBe(a2.bytes);
    expect(estimateTextTokens(en)).toBe(a2.estimated_tokens);
  });

  it("B1/B2 on-demand catalog guidance lines stay within the P00 baseline", () => {
    // 生产常量（core/engine.ts toolCatalogIntroLine）——文本与 P00 计量同源。
    const zh = "以下目录里的工具未随会话预载：用 mcp_search_tools 按关键词查找，mcp_describe_tool 查看参数，mcp_call 调用（内置工具可省略 server）。";
    const en = "The tools listed below are not preloaded: find them with mcp_search_tools, read parameters with mcp_describe_tool, and call them with mcp_call (server may be omitted for built-in tools).";
    const b1 = component("B1 按需目录引导（zh）");
    const b2 = component("B2 按需目录引导（en）");
    expect(Buffer.byteLength(zh, "utf-8")).toBe(b1.bytes);
    expect(estimateTextTokens(zh)).toBe(b1.estimated_tokens);
    expect(Buffer.byteLength(en, "utf-8")).toBe(b2.bytes);
    expect(estimateTextTokens(en)).toBe(b2.estimated_tokens);
  });

  it("C PTC entry tool run_tools description stays within the P00 baseline", () => {
    const c = component("C PTC 入口工具 run_tools 描述");
    const tool = createPtcTool({ binding: createPtcBindingHolder() });
    expect(tool.name).toBe(RUN_TOOLS_TOOL_NAME);
    // 现测 1682 bytes / 1658 chars：P00 记录 1684/1660（研究期计量口径差 2 字节，
    // 该文件自研究 SHA 8037fae7 起零改动，git log 只有一个先于基线的提交）。
    // 预算规则是「不得超过基线」——按 ≤ 断言，差异登记进 PROMPT_BUDGET_REPORT。
    expect(Buffer.byteLength(tool.description, "utf-8")).toBeLessThanOrEqual(c.bytes);
    expect(estimateTextTokens(tool.description)).toBeLessThanOrEqual(c.estimated_tokens);
  });
});

/* ── D 常驻工具 schema 估值（生产入口 buildTools） ─────────────────────── */

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
    execute: vi.fn(async () => ({ content: [{ type: "text", text: "ran" }] })),
  };
}

function makeEngineForTools(tools: any[]) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p06-budget-"));
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
  const build = () => engine.buildTools(workspace, null, {
    agentDir,
    workspace,
    getSessionPath: () => sessionPath,
    getPermissionMode: () => "operate",
    allowHumanApproval: false,
  });
  return { build, tmpDir };
}

describe("P06-T03 resident tool schema budget (component D)", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  it("measures the resident surface at the request boundary via the production buildTools entry", () => {
    const made = makeEngineForTools([
      firstPartyTool("knowledge_search"),
      firstPartyTool("todo_write", { kind: "review" }),
    ]);
    dirs.push(made.tmpDir);
    const result = made.build();

    const surface = [...result.tools, ...result.customTools].filter(Boolean);
    const names = surface.map((t: any) => t.name).sort();
    // 常驻面 = 4 个 Pi 基础工具 + 3 个目录桥工具；按需目录承载其余。
    expect(names).toEqual([
      "edit", "exec_command", "mcp_call", "mcp_describe_tool", "mcp_search_tools", "read", "write",
    ]);
    expect(result.toolCatalogManifest).toBeTruthy();

    const perTool = surface.map((t: any) => ({
      name: t.name,
      tokens: estimateTextTokens(`${t.name}\n${t.description}\n${JSON.stringify(t.parameters ?? {})}`),
    }));
    const total = perTool.reduce((sum, t) => sum + t.tokens, 0);
    expect(total).toBeGreaterThan(0);
    // 脱敏样本：只输出来源与估值，供 PROMPT_BUDGET_REPORT 登记（命令日志归档）。
    // eslint-disable-next-line no-console
    console.log(`P06_RESIDENT_TOOL_BUDGET ${JSON.stringify({ tools: names, per_tool: perTool, total_estimated_tokens: total })}`);
  });
});

/* ── 开关失效契约（记忆/skill） ───────────────────────────────────────── */

function makeSwitchAgent() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p06-switch-"));
  const agentsDir = path.join(root, "agents");
  const productDir = path.join(root, "product");
  const userDir = path.join(root, "user");
  const agentDir = path.join(agentsDir, "hana");
  fs.mkdirSync(path.join(agentDir, "memory"), { recursive: true });
  fs.mkdirSync(path.join(productDir, "yuan"), { recursive: true });
  fs.mkdirSync(userDir, { recursive: true });
  fs.writeFileSync(path.join(productDir, "yuan", "lingxi.md"), "SW-PERSONA {{userName}}", "utf-8");
  fs.writeFileSync(path.join(userDir, "user.md"), "SW-PROFILE", "utf-8");
  fs.writeFileSync(path.join(agentDir, "memory", "memory.md"), "SW-MEMORY", "utf-8");
  // setMemoryMasterEnabled/updateConfig 会 loadConfig（config.yaml）——fixture 提供最小合法配置。
  fs.writeFileSync(path.join(agentDir, "config.yaml"), "agent:\n  yuan: lingxi\nmemory:\n  enabled: true\n", "utf-8");
  const agent = new Agent({ id: "hana", agentsDir, productDir, userDir } as any);
  agent._config = {
    locale: "zh-CN",
    agent: { yuan: "lingxi" },
    memory: { enabled: true },
    experience: { enabled: false },
    user: { name: "黎" },
  };
  agent.userName = "黎";
  agent.agentName = "Hanako";
  agent._canInjectAppearancePrompt = () => false;
  agent._isComputerUseAvailableForThisAgent = () => false;
  agent._listAgents = () => [];
  agent._cb = { getTimezone: () => "Asia/Shanghai" };
  agent._memoryMasterEnabled = true;
  agent._memorySessionEnabled = true;
  agent._systemPrompt = agent.buildSystemPrompt({ forceMemoryEnabled: true });
  return { agent, root };
}

describe("P06-T03 memory/skill switch invalidation", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  it("memory master off rebuilds the master prompt immediately with no residual memory section", () => {
    const { agent, root } = makeSwitchAgent();
    dirs.push(root);
    expect(agent.systemPrompt).toContain("SW-MEMORY");

    agent.setMemoryMasterEnabled(false);
    expect(agent.memoryMasterEnabled).toBe(false);
    // 立即失效：master 缓存重建后不再含记忆段（规则/置顶/长期记忆全部退出）。
    expect(agent.systemPrompt.includes("SW-MEMORY")).toBe(false);
    expect(agent.systemPrompt.includes("## 记忆使用规则")).toBe(false);
    // 用户内容不被删减：人格与用户档案原样保留（不以删用户内容凑预算）。
    expect(agent.systemPrompt).toContain("SW-PERSONA");
    expect(agent.systemPrompt).toContain("SW-PROFILE");
  });

  it("per-session memory off yields a session snapshot without memory sections and keeps user content", () => {
    const { agent, root } = makeSwitchAgent();
    dirs.push(root);
    // per-session 开关不改 master 缓存（agent.ts:1295 注释契约）。
    const masterBefore = agent.systemPrompt;
    agent.setMemoryEnabled(false);
    expect(agent.systemPrompt).toBe(masterBefore);

    // session 快照按自己的开关单独构建：无记忆段、用户内容保留。
    const sessionPrompt = agent.buildSystemPrompt({ forceMemoryEnabled: false });
    expect(sessionPrompt.includes("SW-MEMORY")).toBe(false);
    expect(sessionPrompt.includes("## 记忆使用规则")).toBe(false);
    expect(sessionPrompt).toContain("SW-PERSONA");
    expect(sessionPrompt).toContain("SW-PROFILE");

    // 开关差异只落在 memory_context——其余段字节一致（动态内容差异可解释）。
    const withMemory = agent.buildSystemPrompt({ forceMemoryEnabled: true });
    const a = withMemory.split("\n");
    const b = sessionPrompt.split("\n");
    expect(a.length).toBeGreaterThan(b.length);
    // 前缀直到记忆段之前完全一致（cache 分界线之前的静态前缀不受开关影响）。
    const memoryAt = withMemory.indexOf("## 记忆使用规则");
    expect(memoryAt).toBeGreaterThan(0);
    expect(sessionPrompt.startsWith(withMemory.slice(0, withMemory.indexOf("# 用户档案")))).toBe(true);
  });

  it("skill set change rebuilds the master prompt through the same assembly", () => {
    const { agent, root } = makeSwitchAgent();
    dirs.push(root);
    const before = agent.systemPrompt;
    agent.setEnabledSkills([{ name: "s1" }]);
    // 重建仍走同一装配：人格/档案不变，输出仍是合法 prompt。
    expect(typeof agent.systemPrompt).toBe("string");
    expect(agent.systemPrompt).toContain("SW-PERSONA");
    expect(agent.systemPrompt.length).toBeGreaterThan(0);
    // skills 列表本身不进入 Lingxi 基座（SDK <available_skills> 注入），
    // master prompt 重建前后基座应一致（时间戳段为分钟粒度，同一分钟内稳定）。
    expect(agent.systemPrompt).toBe(before);
    expect(agent.enabledSkills).toEqual([{ name: "s1" }]);
  });
});
