// Phase 5 Step 6：Agent system prompt 单一 canonical 装配的等价锁定。
// golden fixture 于改造前（HEAD 6b93929e 的旧 parts.join 实现）生成——
// 改造后 buildSystemPromptArtifact().text 必须与 golden 字节级一致（§三十四/三十六）。
// platform-prompt mock 掉 env 相关内容（$SHELL/os 版本），保证跨机器确定性。
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../core/platform-prompt.ts", () => ({
  getPlatformPromptNote: () => "FIXED-PLATFORM-NOTE-LINE",
}));

import { Agent } from "../core/agent.ts";
import {
  readAgentAvatarResource,
  writeAgentAppearanceProfileResource,
} from "../lib/agent-appearance-summary.ts";

const tempDirs: string[] = [];

function makeTempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-prompt-equiv-"));
  tempDirs.push(dir);
  return dir;
}

function makeAgent(locale: string) {
  const root = makeTempDir();
  const agentsDir = path.join(root, "agents");
  const productDir = path.join(root, "product");
  const userDir = path.join(root, "user");
  const agentDir = path.join(agentsDir, "hana");
  fs.mkdirSync(path.join(agentDir, "memory"), { recursive: true });
  fs.mkdirSync(path.join(agentDir, "avatars"), { recursive: true });
  fs.mkdirSync(path.join(productDir, "yuan"), { recursive: true });
  fs.mkdirSync(userDir, { recursive: true });
  fs.writeFileSync(
    path.join(productDir, "yuan", "lingxi.md"),
    "AGENTSMD-TEMPLATE-TOP_SECRET_PERSONA 你是{{userName}}的伙伴🎉",
    "utf-8",
  );
  fs.writeFileSync(path.join(userDir, "user.md"), "PROFILE-TOP_SECRET 简介𝐀\n", "utf-8");
  fs.writeFileSync(path.join(agentDir, "pinned.md"), "PINNED-TOP_SECRET 置顶\n", "utf-8");
  fs.writeFileSync(path.join(agentDir, "memory", "memory.md"), "MEMORY-TOP_SECRET 记忆🇨🇳\n", "utf-8");

  fs.writeFileSync(path.join(agentDir, "avatars", "agent.png"), Buffer.from("fake-avatar-bytes"));
  const avatar = readAgentAvatarResource(agentDir);
  writeAgentAppearanceProfileResource(agentDir, {
    avatarHash: avatar!.hash,
    summary: "APPEARANCE-TOP_SECRET 样貌",
    model: null,
  });

  const agent = new Agent({ id: "hana", agentsDir, productDir, userDir } as any);
  agent._config = {
    locale,
    agent: { yuan: "lingxi" },
    memory: { enabled: true },
    experience: { enabled: false },
    user: { name: locale.startsWith("zh") ? "黎" : "Li" },
    capabilities: { learn_skills: { enabled: true, allow_github_fetch: true } },
  };
  agent.userName = locale.startsWith("zh") ? "黎" : "Li";
  agent.agentName = "Hanako";
  agent._canInjectAppearancePrompt = () => true;
  agent._isComputerUseAvailableForThisAgent = () => true;
  agent._listAgents = () => [
    { id: "hana", name: "Hanako", model: "gpt-test", summary: "主 agent TOP_SECRET" },
    { id: "beta", name: "Beta", model: "claude-test", summary: "副 agent" },
  ];
  agent._cb = {
    getTimezone: () => "Asia/Shanghai",
    getPreferences: () => ({}),
    getLearnSkills: () => ({ enabled: true, allow_github_fetch: true }),
  };
  return agent;
}

afterEach(() => {
  vi.useRealTimers();
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

const CASES = [
  { locale: "zh-CN", golden: "system-prompt-golden-zh.txt" },
  { locale: "en", golden: "system-prompt-golden-en.txt" },
] as const;

describe("agent system prompt provenance equivalence", () => {
  for (const { locale, golden } of CASES) {
    it(`keeps legacy byte output and produces provenance (${locale})`, () => {
      vi.useFakeTimers();
      vi.setSystemTime(new Date("2026-06-04T07:53:00.000Z"));

      const agent = makeAgent(locale);
      const goldenText = fs.readFileSync(path.join(process.cwd(), "tests", "fixtures", golden), "utf-8");
      const text = agent.buildSystemPrompt({ forceMemoryEnabled: true });
      expect(text).toBe(goldenText);

      const artifact = agent.buildSystemPromptArtifact({ forceMemoryEnabled: true });
      expect(artifact.text).toBe(text); // 单一装配：text API 只取 artifact.text（§四十六）

      const categories = artifact.provenance.map((s) => s.category);
      for (const expected of [
        "platform_instruction",
        "user_profile",
        "persona",
        "memory_context",
        "agent_roster",
        "session_instruction",
      ]) {
        expect(categories).toContain(expected);
      }
      // span 全部落在 text 内且按顺序首尾相接（separator "\n"）。
      let cursor = 0;
      for (const section of artifact.provenance) {
        const span = section.locator.span!;
        expect(span.start).toBe(cursor);
        expect(span.end).toBeGreaterThan(span.start);
        expect(span.end).toBeLessThanOrEqual(text.length);
        cursor = span.end + 1;
      }
      // 内容定位：persona 段 slice 出模板正文；记忆段含 memory.md 内容。
      const personaSection = artifact.provenance.find((s) => s.source?.id === "persona")!;
      expect(text.slice(personaSection.locator.span!.start, personaSection.locator.span!.end))
        .toContain("AGENTSMD-TEMPLATE-TOP_SECRET_PERSONA");
      const memorySection = artifact.provenance.find((s) => s.source?.id === "memory.longterm")!;
      expect(text.slice(memorySection.locator.span!.start, memorySection.locator.span!.end))
        .toContain("MEMORY-TOP_SECRET 记忆🇨🇳");
      // provenance 无正文（§三十七）。
      const serialized = JSON.stringify(artifact.provenance);
      for (const marker of ["TOP_SECRET_PERSONA", "TOP_SECRET_MEMORY", "PROFILE-TOP_SECRET", "APPEARANCE-TOP_SECRET"]) {
        expect(serialized.includes(marker)).toBe(false);
      }
    });
  }

  it("subagent variant still renders through the same assembly", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-04T07:53:00.000Z"));
    const agent = makeAgent("zh-CN");
    const artifact = agent.buildSystemPromptArtifact({ forSubagent: true, forceMemoryEnabled: true });
    const categories = artifact.provenance.map((s) => s.category);
    expect(categories).not.toContain("memory_context");
    expect(categories).not.toContain("agent_roster");
    expect(artifact.text.startsWith("你运行在灵犀（Lingxi）平台上。")).toBe(true);
  });
});
