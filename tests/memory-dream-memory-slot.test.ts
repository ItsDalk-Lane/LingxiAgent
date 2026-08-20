/**
 * Lingxi 专属契约：Memory Dream 的模型解析必须走与 MemoryTicker 完全相同的
 * memory slot 通道（engine.resolveAuxiliaryExecution("memory", { agentId })），
 * 不得引入 / 回退到 approval / title / summarize / chat 等其他 slot，
 * 也不得复活上游同步前的 utility 体系。
 *
 * 三层断言：
 *   1. ticker → runner：createMemoryDreamRunner 收到的 getResolvedMemoryModel
 *      与 MemoryTicker 记忆维护用的是同一个函数引用。
 *   2. agent 接线：agent.ts 提供给 ticker 的 getResolvedMemoryModel 闭包只解析
 *      "memory" slot（带 agentId），全程不触碰其他 slot。
 *   3. 映射形状：该闭包把 execution 映射为模型调用配置（含凭证与 usage 归属）。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { fileURLToPath } from "url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const dreamRunnerOptions: any[] = [];

vi.mock("../lib/memory/dream/runner.ts", () => ({
  createMemoryDreamRunner: vi.fn((opts: any) => {
    dreamRunnerOptions.push(opts);
    return {
      start: vi.fn(),
      startAutomaticIfEligible: vi.fn(),
      getStatus: vi.fn(() => ({ status: "idle", runId: null, startedAt: null, lastRun: null })),
      restoreRevision: vi.fn(),
      stop: vi.fn(),
      isRunning: vi.fn(() => false),
    };
  }),
}));

vi.mock("../lib/memory/compile.ts", () => ({
  compileToday: vi.fn().mockResolvedValue("compiled"),
  compileDaily: vi.fn().mockResolvedValue("compiled"),
  assembleWeekFromDaily: vi.fn(),
  rollDailyWindow: vi.fn().mockResolvedValue({ folded: [], failed: [] }),
  compileEditableFacts: vi.fn().mockResolvedValue("compiled"),
  assemble: vi.fn(),
  ensureEditableFactsBaseline: vi.fn(),
  migrateLegacyEditableFacts: vi.fn(),
  migrateLegacyWeekToLongterm: vi.fn().mockResolvedValue({ migrated: false }),
}));

vi.mock("../lib/memory/deep-memory.ts", () => ({
  processDirtySessions: vi.fn().mockResolvedValue({ processed: 0, factsAdded: 0 }),
}));

vi.mock("../lib/debug-log.ts", () => ({
  debugLog: () => null,
  createModuleLogger: () => ({ log: vi.fn(), warn: vi.fn(), error: vi.fn() }),
}));

import { Agent } from "../core/agent.ts";

async function makeInitializedAgent(tmpDir: string, engine: any) {
  const agentsDir = path.join(tmpDir, "agents");
  const agentDir = path.join(agentsDir, "test-agent");
  fs.mkdirSync(path.join(agentDir, "memory", "summaries"), { recursive: true });
  fs.mkdirSync(path.join(agentDir, "desk"), { recursive: true });
  fs.mkdirSync(path.join(agentDir, "sessions"), { recursive: true });
  fs.mkdirSync(path.join(tmpDir, "user"), { recursive: true });
  fs.writeFileSync(
    path.join(agentDir, "config.yaml"),
    ["agent:", "  name: TestAgent", "  yuan: lingxi", "user:", "  name: Tester", "locale: en", "memory:", "  enabled: true"].join("\n"),
    "utf-8",
  );
  fs.writeFileSync(path.join(agentDir, "identity.md"), "identity\n", "utf-8");
  fs.writeFileSync(path.join(agentDir, "AGENTS.md"), "persona\n", "utf-8");
  fs.writeFileSync(path.join(tmpDir, "user", "user.md"), "user\n", "utf-8");

  const agent = new Agent({
    id: "test-agent",
    agentsDir,
    userDir: path.join(tmpDir, "user"),
    productDir: path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "lib"),
  } as any);
  agent.setCallbacks({
    getEngine: () => engine,
    getLearnSkills: () => ({}),
    isChannelsEnabled: () => false,
  });
  await agent.init(() => {});
  return agent;
}

describe("Dream memory-slot contract (Lingxi)", () => {
  let tmpDir: string;

  beforeEach(() => {
    vi.clearAllMocks();
    dreamRunnerOptions.length = 0;
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-dream-slot-"));
  });

  afterEach(() => fs.rmSync(tmpDir, { recursive: true, force: true }));

  it("agent init wires the Dream runner to the agent-provided getResolvedMemoryModel", async () => {
    const engine = {
      resolveAuxiliaryExecution: vi.fn(async () => null),
      getPrimaryAgentId: () => "test-agent",
    };
    const agent = await makeInitializedAgent(tmpDir, engine);

    // runner 由真 ticker 工厂创建（runner mock 截获）：agent.ts 给 ticker 的
    // getResolvedMemoryModel 原样传给 Dream runner，中间不接第二条解析路径。
    expect(dreamRunnerOptions).toHaveLength(1);
    expect(typeof dreamRunnerOptions[0].getResolvedMemoryModel).toBe("function");
    const probe = await dreamRunnerOptions[0].getResolvedMemoryModel().catch((err: any) => err);
    expect(engine.resolveAuxiliaryExecution).toHaveBeenCalledWith("memory", { agentId: "test-agent" });
    void probe;
    await agent.dispose();
  });

  it("agent-provided getResolvedMemoryModel resolves only the \"memory\" slot", async () => {
    const slotCalls: Array<{ slot: string; options: any }> = [];
    const engine = {
      resolveAuxiliaryExecution: vi.fn(async (slot: string, options: any) => {
        slotCalls.push({ slot, options });
        return {
          model: { id: "memory-sentinel" },
          provider: "ollama",
          api: "openai",
          apiKey: "k",
          baseUrl: "http://localhost:11434/v1",
          headers: { "X-Test": "1" },
          credentialSource: "keychain",
          accountId: "acc-1",
        };
      }),
      getPrimaryAgentId: () => "test-agent",
    };
    const agent = await makeInitializedAgent(tmpDir, engine);
    const getResolvedMemoryModel = dreamRunnerOptions[0].getResolvedMemoryModel;

    slotCalls.length = 0;
    const resolved = await getResolvedMemoryModel();

    // 只解析 memory slot，带当前 agentId；approval/title/summarize/chat 一概不碰。
    expect(slotCalls).toEqual([{ slot: "memory", options: { agentId: "test-agent" } }]);
    // execution 映射为模型调用配置，含凭证与 usage 归属。
    expect(resolved).toMatchObject({
      model: { id: "memory-sentinel" },
      provider: "ollama",
      api: "openai",
      api_key: "k",
      base_url: "http://localhost:11434/v1",
      headers: { "X-Test": "1" },
      credential_source: "keychain",
      accountId: "acc-1",
      usageAgentId: "test-agent",
    });
    await agent.dispose();
  });

  it("startup probe and ticker resolution both stay on the memory slot", async () => {
    const slotCalls: string[] = [];
    const engine = {
      resolveAuxiliaryExecution: vi.fn(async (slot: string) => {
        slotCalls.push(slot);
        return null;
      }),
      getPrimaryAgentId: () => "test-agent",
    };
    const agent = await makeInitializedAgent(tmpDir, engine);

    // init 的启动告警探测也必须解析 memory slot（agent.ts 启动探测路径）。
    expect(slotCalls.length).toBeGreaterThan(0);
    expect(slotCalls.every((slot) => slot === "memory")).toBe(true);
    await agent.dispose();
  });
});
