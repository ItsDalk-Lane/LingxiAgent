/**
 * A4: getToolsSnapshot 条件注入回归。
 *
 * 目标：channel 工具只在 `_cb.isChannelsEnabled()` 返回 true 时出现在快照里；
 * install_skill 工具只在 `_cb.getLearnSkills()` (或 config.capabilities.learn_skills)
 * 的 enabled 严格等于 true 时出现在快照里。此前两者都无条件注入，execute 层的
 * isEnabled 校验只在真正调用时才拦截，导致模型在工具关闭时仍能看到并尝试调用它们。
 *
 * bootstrapAgent 沿用 tests/session-tool-gating.test.ts / tests/builtin-tool-
 * permission-coverage.test.ts 的真实 Agent 构造范本；channel 工具需要
 * channelsDir + agentsDir 才会被组装出来（core/agent.ts 第 569 行判断），
 * 因此这里必须提供 channelsDir，否则 isChannelsEnabled 的开关测不出区别。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../lib/memory/memory-ticker.js", () => ({
  createMemoryTicker: () => ({
    start: vi.fn(),
    stop: vi.fn().mockResolvedValue(undefined),
    tick: vi.fn().mockResolvedValue(undefined),
    triggerNow: vi.fn(),
    notifyTurn: vi.fn(),
    notifySessionEnd: vi.fn().mockResolvedValue(undefined),
    notifyPromoted: vi.fn().mockResolvedValue(undefined),
    flushSession: vi.fn().mockResolvedValue(undefined),
    getHealthStatus: vi.fn().mockReturnValue({}),
  }),
}));

import { Agent } from "../core/agent.ts";

function bootstrapAgent(rootDir: string) {
  const agentsDir = path.join(rootDir, "agents");
  const agentDir = path.join(agentsDir, "hana");
  const userDir = path.join(rootDir, "user");
  const channelsDir = path.join(rootDir, "channels");
  fs.mkdirSync(path.join(agentDir, "memory", "summaries"), { recursive: true });
  fs.mkdirSync(path.join(agentDir, "desk"), { recursive: true });
  fs.mkdirSync(path.join(agentDir, "sessions"), { recursive: true });
  fs.mkdirSync(userDir, { recursive: true });
  fs.mkdirSync(channelsDir, { recursive: true });

  fs.writeFileSync(
    path.join(agentDir, "config.yaml"),
    [
      "agent:",
      "  name: Hana",
      "  yuan: hanako",
      "user:",
      "  name: Tester",
      "locale: en",
      "memory:",
      "  enabled: false",
      "models:",
      "  chat:",
      "    id: gpt-4",
      "    provider: openai",
    ].join("\n"),
    "utf-8",
  );
  fs.writeFileSync(path.join(agentDir, "identity.md"), "identity\n", "utf-8");
  fs.writeFileSync(path.join(agentDir, "ishiki.md"), "ishiki\n", "utf-8");
  fs.writeFileSync(path.join(userDir, "user.md"), "user profile\n", "utf-8");

  const productDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "lib");
  return { agentsDir, productDir, userDir, channelsDir };
}

describe("Agent.getToolsSnapshot conditional injection (A4)", () => {
  const roots: string[] = [];

  afterEach(async () => {
    while (roots.length) {
      fs.rmSync(roots.pop()!, { recursive: true, force: true });
    }
  });

  async function buildAgent(rootDir: string, cb?: Record<string, unknown>) {
    const { agentsDir, productDir, userDir, channelsDir } = bootstrapAgent(rootDir);
    const agent = new Agent({ id: "hana", agentsDir, productDir, userDir, channelsDir } as any);
    if (cb) agent.setCallbacks(cb);
    await agent.init(() => {});
    return agent;
  }

  // ── channel 工具：由 _cb.isChannelsEnabled() 门控 ──

  it("includes channel tool when _cb.isChannelsEnabled() returns true", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tools-cond-"));
    roots.push(root);
    const agent = await buildAgent(root, {
      isChannelsEnabled: () => true,
      getLearnSkills: () => ({}),
    });
    const toolNames = agent.getToolsSnapshot({ forceMemoryEnabled: false }).map((t) => t.name);
    expect(toolNames).toContain("channel");
    await agent.dispose();
  });

  it("excludes channel tool when _cb.isChannelsEnabled() returns false", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tools-cond-"));
    roots.push(root);
    const agent = await buildAgent(root, {
      isChannelsEnabled: () => false,
      getLearnSkills: () => ({}),
    });
    const toolNames = agent.getToolsSnapshot({ forceMemoryEnabled: false }).map((t) => t.name);
    expect(toolNames).not.toContain("channel");
    await agent.dispose();
  });

  it("excludes channel tool when the isChannelsEnabled callback is missing", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tools-cond-"));
    roots.push(root);
    // 完全不调用 setCallbacks：_cb 保持构造期默认值 null。
    const agent = await buildAgent(root);
    const toolNames = agent.getToolsSnapshot({ forceMemoryEnabled: false }).map((t) => t.name);
    expect(toolNames).not.toContain("channel");
    await agent.dispose();
  });

  // ── install_skill 工具：由 _cb.getLearnSkills().enabled 门控 ──

  it("includes install_skill tool when _cb.getLearnSkills() returns { enabled: true }", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tools-cond-"));
    roots.push(root);
    const agent = await buildAgent(root, {
      isChannelsEnabled: () => false,
      getLearnSkills: () => ({ enabled: true }),
    });
    const toolNames = agent.getToolsSnapshot({ forceMemoryEnabled: false }).map((t) => t.name);
    expect(toolNames).toContain("install_skill");
    await agent.dispose();
  });

  it("excludes install_skill tool when _cb.getLearnSkills() returns { enabled: false }", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tools-cond-"));
    roots.push(root);
    const agent = await buildAgent(root, {
      isChannelsEnabled: () => false,
      getLearnSkills: () => ({ enabled: false }),
    });
    const toolNames = agent.getToolsSnapshot({ forceMemoryEnabled: false }).map((t) => t.name);
    expect(toolNames).not.toContain("install_skill");
    await agent.dispose();
  });

  it("excludes install_skill tool when _cb.getLearnSkills() returns {} (no enabled key)", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tools-cond-"));
    roots.push(root);
    const agent = await buildAgent(root, {
      isChannelsEnabled: () => false,
      getLearnSkills: () => ({}),
    });
    const toolNames = agent.getToolsSnapshot({ forceMemoryEnabled: false }).map((t) => t.name);
    expect(toolNames).not.toContain("install_skill");
    await agent.dispose();
  });

  it("excludes install_skill tool when the getLearnSkills callback is missing entirely", async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-tools-cond-"));
    roots.push(root);
    const agent = await buildAgent(root);
    const toolNames = agent.getToolsSnapshot({ forceMemoryEnabled: false }).map((t) => t.name);
    expect(toolNames).not.toContain("install_skill");
    await agent.dispose();
  });
});
