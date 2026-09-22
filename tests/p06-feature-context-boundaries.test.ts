/**
 * P06-T04｜人格、记忆、Skill 与知识功能的上下文边界（任务书 §5 P06-T04 / 场景 A09）。
 *
 * 本阶段只验证「接入方式不改变业务设计」：
 *   1. 人格/用户资料只读使用——装配多次不落盘、不改写用户文件；
 *   2. 多变体装配（记忆 on/off × subagent）下人格段字节一致（人格不被改写），
 *      记忆内容只在开关开启时出现；
 *   3. 基座 prompt 不携带知识/Skill 材料：knowledge_* 走工具面、skills 走 SDK
 *      <available_skills> 段，两者都不进入 Lingxi canonical 基座（不悄悄混入
 *      未选择来源）。
 *
 * 既有保护的映射（本文件不重复实现）：
 *   - MOOD/思考区块名与触发锚：tests/yuan-metadata.test.ts + yuan-trigger-anchor
 *     .test.ts（真实模板锁定）；输出侧 <mood> 保留协议：P05 normalizer 套件。
 *   - 知识 scope 冻结/跨 session 拒绝/子代理拦截：tests/knowledge-agent-tools.test.ts。
 *   - Skill 指针源删除的显式 unavailable：tests/session-skill-snapshot.test.ts
 *     "omits pointer skills whose source file was removed"。
 *   - 记忆开关立即失效：tests/p06-prompt-budget-and-invalidation.test.ts（T03）。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it } from "vitest";

import { Agent } from "../core/agent.ts";

const tempDirs: string[] = [];

function makeTempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hana-p06-bounds-"));
  tempDirs.push(dir);
  return dir;
}

function makeAgent() {
  const root = makeTempDir();
  const agentsDir = path.join(root, "agents");
  const productDir = path.join(root, "product");
  const userDir = path.join(root, "user");
  const agentDir = path.join(agentsDir, "hana");
  fs.mkdirSync(path.join(agentDir, "memory"), { recursive: true });
  fs.mkdirSync(path.join(productDir, "yuan"), { recursive: true });
  fs.mkdirSync(userDir, { recursive: true });
  const personaFile = path.join(productDir, "yuan", "lingxi.md");
  const profileFile = path.join(userDir, "user.md");
  const memoryFile = path.join(agentDir, "memory", "memory.md");
  const personaText = "BOUNDS-PERSONA 你是{{userName}}的伙伴，风格保持。";
  const profileText = "BOUNDS-PROFILE 用户档案正文";
  const memoryText = "BOUNDS-MEMORY 长期记忆正文";
  fs.writeFileSync(personaFile, personaText, "utf-8");
  fs.writeFileSync(profileFile, profileText, "utf-8");
  fs.writeFileSync(memoryFile, memoryText, "utf-8");

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
  return { agent, personaFile, profileFile, memoryFile, personaText, profileText, memoryText, root };
}

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function personaSlice(prompt: string) {
  const at = prompt.indexOf("BOUNDS-PERSONA");
  expect(at).toBeGreaterThanOrEqual(0);
  return prompt.slice(at, prompt.indexOf("\n", at) >= 0 ? prompt.indexOf("\n", at) : undefined);
}

describe("P06-T04 persona / memory / skill / knowledge context boundaries", () => {
  it("assembly is read-only: repeated builds never modify persona, profile, or memory files", () => {
    const { agent, personaFile, profileFile, memoryFile, personaText, profileText, memoryText } = makeAgent();
    const before = {
      persona: fs.readFileSync(personaFile, "utf-8"),
      profile: fs.readFileSync(profileFile, "utf-8"),
      memory: fs.readFileSync(memoryFile, "utf-8"),
    };
    const snapshot = fs.statSync(memoryFile);
    for (let i = 0; i < 3; i += 1) {
      agent.buildSystemPromptArtifact({ forceMemoryEnabled: true });
      agent.buildSystemPromptArtifact({ forceMemoryEnabled: false });
      agent.buildSystemPromptArtifact({ forSubagent: true, forceMemoryEnabled: true });
    }
    expect(fs.readFileSync(personaFile, "utf-8")).toBe(before.persona);
    expect(fs.readFileSync(profileFile, "utf-8")).toBe(before.profile);
    expect(fs.readFileSync(memoryFile, "utf-8")).toBe(before.memory);
    expect(personaText).toContain("{{userName}}"); // 模板原文未被写回替换
    expect(profileText).toContain("BOUNDS-PROFILE");
    expect(memoryText).toContain("BOUNDS-MEMORY");
    expect(fs.statSync(memoryFile).mtimeMs).toBe(snapshot.mtimeMs);
  });

  it("variant matrix keeps the persona byte-identical while memory rides its switches", () => {
    const { agent } = makeAgent();
    const variants = {
      memoryOn: agent.buildSystemPromptArtifact({ forceMemoryEnabled: true }),
      memoryOff: agent.buildSystemPromptArtifact({ forceMemoryEnabled: false }),
      subagent: agent.buildSystemPromptArtifact({ forSubagent: true, forceMemoryEnabled: true }),
      subagentOff: agent.buildSystemPromptArtifact({ forSubagent: true, forceMemoryEnabled: false }),
    };

    // 人格段（模板替换后）在所有变体下字节一致——开关与隔离不改写人格。
    const slices = Object.values(variants).map((v) => personaSlice(v.text));
    expect(new Set(slices).size).toBe(1);
    expect(slices[0]).toContain("你是黎的伙伴"); // {{userName}} 已替换

    // 记忆内容只在开启且非 subagent 时出现。
    expect(variants.memoryOn.text).toContain("BOUNDS-MEMORY");
    expect(variants.memoryOff.text.includes("BOUNDS-MEMORY")).toBe(false);
    expect(variants.subagent.text.includes("BOUNDS-MEMORY")).toBe(false);
    expect(variants.subagentOff.text.includes("BOUNDS-MEMORY")).toBe(false);

    // 用户档案保留在所有变体（现行设计，T01 已登记），且只出现一次。
    for (const v of Object.values(variants)) {
      expect(v.text.split("BOUNDS-PROFILE").length - 1).toBe(1);
    }

    // provenance 类别：persona 段在所有变体都有；memory_context 只在 memoryOn。
    for (const v of Object.values(variants)) {
      const ids = v.provenance.map((s: any) => s.source?.id);
      expect(ids).toContain("persona");
      expect(ids).toContain("user.profile");
    }
    expect(variants.memoryOn.provenance.map((s: any) => s.category)).toContain("memory_context");
    for (const key of ["memoryOff", "subagent", "subagentOff"] as const) {
      expect(variants[key].provenance.map((s: any) => s.category)).not.toContain("memory_context");
    }
  });

  it("canonical base never carries knowledge materials or a skills listing", () => {
    const { agent } = makeAgent();
    for (const options of [
      { forceMemoryEnabled: true },
      { forceMemoryEnabled: false },
      { forSubagent: true, forceMemoryEnabled: true },
    ]) {
      const artifact = agent.buildSystemPromptArtifact(options as any);
      // 知识检索/读取是工具面能力（knowledge_* 目录项），不属于 prompt 基座。
      expect(artifact.text.includes("knowledge_search")).toBe(false);
      expect(artifact.text.includes("knowledge_read")).toBe(false);
      expect(artifact.text.includes("knowledge_outline")).toBe(false);
      // Skill 一览由 SDK <available_skills> 段注入（#399：Lingxi 不自行拼接）。
      // 注：skill-usage 常驻规则文本合法提到「读 SKILL.md」这一动作，不等于清单注入。
      expect(artifact.text.includes("<available_skills>")).toBe(false);
      expect(artifact.text.includes("<skill>")).toBe(false);
      // provenance 里没有知识来源段；技能相关的 source id 只有常驻规则段
      // （platform.skill-usage；本 fixture 未开启 learn_skills，条件段不注入），
      // 没有材料清单段。
      const ids = artifact.provenance.map((s: any) => s.source?.id);
      expect(ids.filter((id: string) => id.includes("knowledge"))).toEqual([]);
      expect(ids.filter((id: string) => id.includes("skill"))).toEqual(["platform.skill-usage"]);
    }
  });
});
