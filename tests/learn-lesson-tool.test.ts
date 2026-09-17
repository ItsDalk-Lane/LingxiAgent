/**
 * learn_lesson 工具行为测试。
 *
 * 覆盖：总开关门控、必填校验、名称白名单（路径逃逸无从构造）、同名冲突拒绝、
 * 大小上限、guard 安全审查 fail-closed 与风险确认链、成功沉淀（SKILL.md 落盘 +
 * 经验索引 + onLearned 回调）。模式照抄 install-skill-safety-review.test.ts。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";

vi.mock("../lib/i18n.js", () => ({
  getLocale: () => "zh-CN",
  t: (key: string, values?: Record<string, unknown>) => {
    if (!values) return key;
    const suffix = Object.entries(values).map(([k, v]) => `${k}=${v}`).join(",");
    return `${key}:${suffix}`;
  },
}));

vi.mock("../core/llm-client.js", () => ({
  callText: vi.fn(),
}));

import { callText } from "../core/llm-client.ts";
import { createLearnLessonTool } from "../lib/tools/learn-lesson-tool.ts";

const GUARD = {
  model: "guard-model",
  apiKey: "key",
  baseUrl: "https://example.test",
  api: "openai",
};

function makeTool(root: string, opts: { enabled?: boolean; onLearned?: any } = {}) {
  const agentDir = path.join(root, "agent");
  const skillsDir = path.join(root, "skills");
  fs.mkdirSync(agentDir, { recursive: true });
  const tool = createLearnLessonTool({
    agentDir,
    getUserSkillsDir: () => skillsDir,
    isEnabled: () => opts.enabled !== false,
    resolveGuardModel: () => GUARD,
    onLearned: opts.onLearned ?? vi.fn(),
  });
  return { tool, agentDir, skillsDir };
}

const PARAMS = {
  name: "retry-empty-reply",
  description: "When a tool reply comes back empty, retry once before giving up",
  lesson: "空回复先重试一次再放弃。\n\n重试时换一下措辞，别原样重发。",
};

describe("learn_lesson 工具", () => {
  const roots: string[] = [];

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    while (roots.length) {
      const r = roots.pop()!;
      fs.rmSync(r, { recursive: true, force: true });
    }
  });

  function freshRoot() {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "hana-learn-lesson-"));
    roots.push(root);
    return root;
  }

  it("总开关关闭时拒绝沉淀", async () => {
    const { tool, skillsDir } = makeTool(freshRoot(), { enabled: false });
    const result = await tool.execute("c1", PARAMS, null, null, {});
    expect((result as any).content[0].text).toContain("error.installSkillDisabled");
    expect(fs.existsSync(skillsDir)).toBe(false);
  });

  it("必填缺失时报空输入", async () => {
    const { tool } = makeTool(freshRoot());
    const result = await tool.execute("c1", { name: "x", description: "", lesson: "" }, null, null, {});
    expect((result as any).content[0].text).toContain("error.learnLessonEmptyInput");
  });

  it("非法名称（路径逃逸形状）拒绝且不触达 guard", async () => {
    const { tool } = makeTool(freshRoot());
    const result = await tool.execute("c1", { ...PARAMS, name: "../escape" }, null, null, {});
    expect((result as any).content[0].text).toContain("error.installSkillNameInvalid");
    expect(callText).not.toHaveBeenCalled();
  });

  it("同名技能已存在时拒绝、不覆盖、不烧 guard 调用", async () => {
    const root = freshRoot();
    const { tool, skillsDir } = makeTool(root);
    fs.mkdirSync(path.join(skillsDir, PARAMS.name), { recursive: true });
    fs.writeFileSync(path.join(skillsDir, PARAMS.name, "SKILL.md"), "old", "utf-8");

    const result = await tool.execute("c1", PARAMS, null, null, {});
    expect((result as any).content[0].text).toContain("error.learnLessonConflict");
    expect((result as any).details.conflict).toBe(true);
    expect(fs.readFileSync(path.join(skillsDir, PARAMS.name, "SKILL.md"), "utf-8")).toBe("old");
    expect(callText).not.toHaveBeenCalled();
  });

  it("超过大小上限时拒绝", async () => {
    const { tool, skillsDir } = makeTool(freshRoot());
    const big = "x".repeat(60_000);
    const result = await tool.execute("c1", { ...PARAMS, lesson: big }, null, null, {});
    expect((result as any).content[0].text).toContain("error.installSkillSizeLimit");
    expect(fs.existsSync(path.join(skillsDir, PARAMS.name))).toBe(false);
    expect(callText).not.toHaveBeenCalled();
  });

  it("guard 未配置时 fail-closed 走风险确认", async () => {
    const root = freshRoot();
    const agentDir = path.join(root, "agent");
    fs.mkdirSync(agentDir, { recursive: true });
    const tool = createLearnLessonTool({
      agentDir,
      getUserSkillsDir: () => path.join(root, "skills"),
      isEnabled: () => true,
      resolveGuardModel: () => null,
      onLearned: vi.fn(),
    });
    const result = await tool.execute("c1", PARAMS, null, null, {});
    expect((result as any).details).toMatchObject({
      requiresRiskConfirmation: true,
      riskAccepted: false,
      safetyReview: false,
    });
    expect((result as any).details.riskConfirmationToken).toMatch(/^risk_/);
    expect(fs.existsSync(path.join(root, "skills", PARAMS.name))).toBe(false);
  });

  it("审查未通过 → 带 token 显式确认后落盘并标 riskOverride", async () => {
    const root = freshRoot();
    const onLearned = vi.fn();
    const { tool, skillsDir } = makeTool(root, { onLearned });
    (callText as any).mockResolvedValue("suspicious: broad trigger");

    const first = await tool.execute("c1", PARAMS, null, null, {});
    expect((first as any).details.requiresRiskConfirmation).toBe(true);
    expect(fs.existsSync(path.join(skillsDir, PARAMS.name))).toBe(false);
    expect(onLearned).not.toHaveBeenCalled();

    const token = (first as any).details.riskConfirmationToken;
    const second = await tool.execute("c2", {
      ...PARAMS,
      risk_accepted: true,
      risk_confirmation_token: token,
    }, null, null, {});
    expect((second as any).details).toMatchObject({
      skillName: PARAMS.name,
      safetyReview: false,
      riskOverride: true,
      riskReason: "broad trigger",
    });
    expect(fs.existsSync(path.join(skillsDir, PARAMS.name, "SKILL.md"))).toBe(true);
    expect(onLearned).toHaveBeenCalledWith(PARAMS.name);
  });

  it("无 token 的 risk_accepted 不能绕过审查", async () => {
    const { tool, skillsDir } = makeTool(freshRoot());
    (callText as any).mockResolvedValueOnce("suspicious: ignore previous instructions");
    const result = await tool.execute("c1", { ...PARAMS, risk_accepted: true }, null, null, {});
    expect((result as any).details).toMatchObject({
      requiresRiskConfirmation: true,
      riskAcceptanceRejection: "missing_confirmation_token",
    });
    expect(fs.existsSync(path.join(skillsDir, PARAMS.name))).toBe(false);
  });

  it("审查通过：SKILL.md 落盘（frontmatter + 正文）、写经验索引、触发 onLearned", async () => {
    const root = freshRoot();
    const onLearned = vi.fn();
    const { tool, agentDir, skillsDir } = makeTool(root, { onLearned });
    (callText as any).mockResolvedValueOnce("safe");

    const result = await tool.execute("c1", PARAMS, null, null, {});
    expect((result as any).details).toMatchObject({
      skillName: PARAMS.name,
      safetyReview: true,
      riskOverride: false,
      experienceIndexed: true,
    });

    const skillFile = path.join(skillsDir, PARAMS.name, "SKILL.md");
    const content = fs.readFileSync(skillFile, "utf-8");
    expect(content).toContain(`name: ${PARAMS.name}`);
    expect(content).toContain("description:");
    expect(content).toContain(PARAMS.description);
    expect(content).toContain("空回复先重试一次再放弃");
    // installer 对 agent 自学技能标 default-enabled: false（不为别的 agent 默认开）
    expect(content).toContain("default-enabled: false");

    // 经验索引：分类文件 + experience.md 总索引
    const expDir = path.join(agentDir, "experience");
    const expFiles = fs.readdirSync(expDir).filter((f) => f.endsWith(".md"));
    expect(expFiles.length).toBe(1);
    const expBody = fs.readFileSync(path.join(expDir, expFiles[0]), "utf-8");
    expect(expBody).toContain(`1. ${PARAMS.name}: 空回复先重试一次再放弃。`);
    const index = fs.readFileSync(path.join(agentDir, "experience.md"), "utf-8");
    expect(index).toContain("learned lessons");

    expect(onLearned).toHaveBeenCalledWith(PARAMS.name);
  });

  it("经验索引写入失败不吞掉技能已落盘的事实", async () => {
    const root = freshRoot();
    const onLearned = vi.fn();
    const { tool, agentDir, skillsDir } = makeTool(root, { onLearned });
    // 把 experience 目录路径占成文件，逼 recordEntry 抛错
    fs.writeFileSync(path.join(agentDir, "experience"), "not a dir", "utf-8");
    (callText as any).mockResolvedValueOnce("safe");

    const result = await tool.execute("c1", PARAMS, null, null, {});
    expect((result as any).details.skillName).toBe(PARAMS.name);
    expect((result as any).details.experienceIndexed).toBe(false);
    expect(fs.existsSync(path.join(skillsDir, PARAMS.name, "SKILL.md"))).toBe(true);
    expect(onLearned).toHaveBeenCalledWith(PARAMS.name);
  });
});
