/**
 * 删除助手时连带删除「孤儿技能」——cleanup-preview 预览与 DELETE /agents/:id
 * 的 deleteSkills 联动。技能本体在全局池（skillsDir），agent 只在 config.yaml 的
 * skills.enabled 里引用；「孤儿」= 仅目标助手启用、其他存活助手未启用的可删技能。
 */
import fs from "fs";
import os from "os";
import path from "path";
import { Hono } from "hono";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const SEEDED_SKILL = "skill-creator"; // skills2set/ 内置种子（仓库根真实存在）

describe("agent delete + orphan skills", () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hana-agent-delete-skills-"));

  beforeEach(() => {
    fs.rmSync(tempRoot, { recursive: true, force: true });
    fs.mkdirSync(path.join(tempRoot, "agents"), { recursive: true });
    fs.mkdirSync(path.join(tempRoot, "skills"), { recursive: true });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  function agentDir(agentId) {
    return path.join(tempRoot, "agents", agentId);
  }

  function skillDir(name) {
    return path.join(tempRoot, "skills", name);
  }

  function makeAgent(agentId, enabledSkills = []) {
    const dir = agentDir(agentId);
    fs.mkdirSync(dir, { recursive: true });
    const list = enabledSkills.map(n => `    - ${n}`).join("\n");
    fs.writeFileSync(
      path.join(dir, "config.yaml"),
      `agent:\n  name: ${agentId}\nskills:\n  enabled:\n${list}\n`,
      "utf-8",
    );
    return dir;
  }

  function makeSkill(name) {
    const dir = skillDir(name);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "SKILL.md"), `---\nname: ${name}\n---\n# ${name}\n`, "utf-8");
    return dir;
  }

  function makeEngine(viewSkills, opts: { deleteAgentResult?: any } = {}) {
    const { deleteAgentResult } = opts;
    return {
      agentsDir: path.join(tempRoot, "agents"),
      skillsDir: path.join(tempRoot, "skills"),
      lingxiHome: tempRoot,
      getAllSkills: vi.fn(() => viewSkills),
      getAgent: vi.fn(() => ({ agentName: "replacement" })),
      deleteAgent: vi.fn().mockResolvedValue(
        deleteAgentResult || { ok: true, replacementAgentId: "deepseek" },
      ),
      reloadSkills: vi.fn().mockResolvedValue(undefined),
      emitEvent: vi.fn(),
    };
  }

  async function makeApp(engine) {
    const { createAgentsRoute } = await import("../server/routes/agents.ts");
    const app = new Hono();
    app.route("/api", createAgentsRoute(engine));
    return app;
  }

  it("cleanup-preview 只返回目标助手独占、可删的用户技能", async () => {
    makeAgent("hana", ["alpha", "shared", SEEDED_SKILL, "disabled-one"]);
    makeAgent("deepseek", ["shared"]);
    makeSkill("alpha");
    makeSkill("shared");
    makeSkill(SEEDED_SKILL);
    makeSkill("disabled-one");

    const engine = makeEngine([
      { name: "alpha", description: "Alpha skill", enabled: true },
      { name: "shared", description: "Shared", enabled: true },
      { name: SEEDED_SKILL, description: "seed", enabled: true },      // 内置种子 → 排除
      { name: "disabled-one", description: "", enabled: false },       // 未启用 → 排除
      { name: "external-one", description: "", enabled: true, readonly: true },   // 外部 → 排除
      { name: "ws-one", description: "", enabled: true, source: "workspace" },    // workspace → 排除
      { name: "plugin-one", description: "", enabled: true, managedBy: "plugin" },// 插件 → 排除
      { name: "ghost", description: "", enabled: true },               // 池里不存在 → 排除
    ]);
    const app = await makeApp(engine);

    const res = await app.request("/api/agents/hana/skills/cleanup-preview");
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      skills: [{ name: "alpha", description: "Alpha skill" }],
    });
  });

  it("cleanup-preview 忽略已墓碑（先前删除）助手的技能占用", async () => {
    makeAgent("hana", ["alpha"]);
    const deadDir = makeAgent("gone", ["alpha"]);
    fs.writeFileSync(path.join(deadDir, ".deleted-agent.json"), "{}", "utf-8");
    makeSkill("alpha");

    const engine = makeEngine([{ name: "alpha", description: "", enabled: true }]);
    const app = await makeApp(engine);

    const res = await app.request("/api/agents/hana/skills/cleanup-preview");
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ skills: [{ name: "alpha", description: "" }] });
  });

  it("cleanup-preview 对不存在助手返回 404", async () => {
    makeAgent("hana", []);
    const engine = makeEngine([]);
    const app = await makeApp(engine);

    const res = await app.request("/api/agents/ghost/skills/cleanup-preview");
    expect(res.status).toBe(404);
  });

  it("DELETE 携带 deleteSkills：删独占技能、保留共享技能、清理引用并广播", async () => {
    makeAgent("hana", ["alpha", "shared"]);
    makeAgent("deepseek", ["shared"]);
    makeSkill("alpha");
    makeSkill("shared");

    const engine = makeEngine([
      { name: "alpha", description: "", enabled: true },
      { name: "shared", description: "", enabled: true },
    ]);
    const app = await makeApp(engine);

    const res = await app.request("/api/agents/hana", {
      method: "DELETE",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ deleteSkills: ["alpha", "shared"] }),
    });

    expect(res.status).toBe(200);
    const data = await res.json();
    expect(data.ok).toBe(true);
    expect(data.skillsDeleted).toEqual(["alpha"]);                       // shared 被其他助手占用 → 不删
    expect(data.skillsSkipped).toEqual([{ name: "shared", reason: "not_orphan" }]);

    expect(fs.existsSync(skillDir("alpha"))).toBe(false);
    expect(fs.existsSync(skillDir("shared"))).toBe(true);
    expect(engine.deleteAgent).toHaveBeenCalledWith("hana");
    expect(engine.reloadSkills).toHaveBeenCalledTimes(1);

    const { loadConfig } = await import("../lib/memory/config-loader.ts");
    const deepseekConfig = loadConfig(path.join(agentDir("deepseek"), "config.yaml"));
    expect(deepseekConfig.skills.enabled).toEqual(["shared"]);

    const eventTypes = engine.emitEvent.mock.calls.map(call => call[0]?.event?.type);
    expect(eventTypes).toContain("agent-deleted");
    expect(eventTypes).toContain("skills-changed");
  });

  it("DELETE 不带 body：行为与旧版一致，不动任何技能", async () => {
    makeAgent("hana", ["alpha"]);
    makeAgent("deepseek", []);
    makeSkill("alpha");

    const engine = makeEngine([{ name: "alpha", description: "", enabled: true }]);
    const app = await makeApp(engine);

    const res = await app.request("/api/agents/hana", { method: "DELETE" });

    expect(res.status).toBe(200);
    const data = await res.json();
    expect(data).toMatchObject({ ok: true, replacementAgentId: "deepseek", skillsDeleted: [], skillsSkipped: [] });
    expect(fs.existsSync(skillDir("alpha"))).toBe(true);
    expect(engine.reloadSkills).not.toHaveBeenCalled();
  });

  it("技能清理失败时请求仍成功，失败项显式标注（助手删除不可逆，不回滚）", async () => {
    makeAgent("hana", ["alpha"]);
    makeAgent("deepseek", []);
    makeSkill("alpha");

    const engine = makeEngine([{ name: "alpha", description: "", enabled: true }]);
    engine.reloadSkills = vi.fn().mockRejectedValue(new Error("reload boom"));
    const app = await makeApp(engine);

    const res = await app.request("/api/agents/hana", {
      method: "DELETE",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ deleteSkills: ["alpha"] }),
    });

    expect(res.status).toBe(200);
    const data = await res.json();
    expect(data.ok).toBe(true);
    expect(data.skillsDeleted).toEqual([]);
    expect(data.skillsSkipped).toEqual([{ name: "alpha", reason: "removal_failed" }]);
  });

  it("DELETE 的 deleteAgent 失败时不触碰技能", async () => {
    makeAgent("hana", ["alpha"]);
    makeAgent("deepseek", []);
    makeSkill("alpha");

    const engine = makeEngine([{ name: "alpha", description: "", enabled: true }]);
    engine.deleteAgent = vi.fn().mockRejectedValue(new Error("cannot delete the last agent"));
    const app = await makeApp(engine);

    const res = await app.request("/api/agents/hana", {
      method: "DELETE",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ deleteSkills: ["alpha"] }),
    });

    expect(res.status).toBe(400);
    expect(fs.existsSync(skillDir("alpha"))).toBe(true);
    expect(engine.reloadSkills).not.toHaveBeenCalled();
  });
});
