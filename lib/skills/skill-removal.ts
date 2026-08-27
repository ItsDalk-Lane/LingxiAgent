/**
 * 用户技能删除与「随助手删除」孤儿判定
 *
 * 技能本体是全局池（lingxiHome/skills/<name>/），agent 只在 config.yaml 的
 * skills.enabled 里按名字引用。这里提供两条共享能力：
 *  1. computeAgentOrphanSkills — 判定「仅某个助手启用、其他存活助手都未启用」
 *     的可删除用户技能，供删除助手前的勾选预览与删除时的复核共用；
 *  2. removeUserSkills — 物理删除技能目录并清掉所有 agent config 与 bundle
 *     里的引用（从 skills 路由的单个删除逻辑抽出，行为等价）。
 */
import fs from "fs";
import path from "path";
import { fromRoot } from "../../shared/hana-root.ts";
import { loadConfig, saveConfig } from "../memory/config-loader.ts";
import { sanitizeSkillName } from "./skill-package-installer.ts";
import { removeSkillsFromBundles } from "../skill-bundles/store.ts";
import { createModuleLogger } from "../debug-log.ts";

const log = createModuleLogger("skill-removal");

/** 内置种子技能名（skills2set/，每次启动都会同步回用户池，不该随助手删除） */
export function listSeededSkillNames() {
  const seededDir = fromRoot("skills2set");
  if (!fs.existsSync(seededDir)) return [];
  return fs.readdirSync(seededDir, { withFileTypes: true })
    .filter(entry => entry.isDirectory() && !entry.name.startsWith("."))
    .map(entry => entry.name);
}

function isDeletableUserSkill(skill, skillsDir, seeded) {
  if (!skill || typeof skill.name !== "string") return false;
  // 与技能面板删除按钮同规则：外部(readonly)/workspace/插件管理的技能不可删
  if (skill.readonly) return false;
  if (skill.source === "workspace") return false;
  if (skill.managedBy === "workspace" || skill.managedBy === "plugin") return false;
  if (seeded.has(skill.name)) return false;
  // 用户池物理存在才算（外部技能不在 skillsDir 下，天然被排除）
  return fs.existsSync(path.join(skillsDir, skill.name));
}

/** 其他存活助手启用的技能名集合（读磁盘 config，跳过墓碑与目标助手自身） */
function collectEnabledSkillsOfOtherAgents(agentsDir, excludeAgentId) {
  const used = new Set();
  if (!agentsDir || !fs.existsSync(agentsDir)) return used;
  for (const entry of fs.readdirSync(agentsDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    if (entry.name === excludeAgentId) continue;
    const agentDir = path.join(agentsDir, entry.name);
    if (fs.existsSync(path.join(agentDir, ".deleted-agent.json"))) continue;
    const configPath = path.join(agentDir, "config.yaml");
    if (!fs.existsSync(configPath)) continue;
    try {
      const cfg = loadConfig(configPath);
      const enabled = cfg?.skills?.enabled;
      if (Array.isArray(enabled)) {
        for (const name of enabled) {
          if (typeof name === "string") used.add(name);
        }
      }
    } catch (err) {
      log.warn(`读取 agent ${entry.name} 的 skills.enabled 失败: ${err.message}`);
    }
  }
  return used;
}

/**
 * 计算可随指定助手一并删除的「孤儿」技能：
 * 该助手已启用 + 用户池可删 + 非内置种子 + 没有任何其他存活助手启用。
 */
export function computeAgentOrphanSkills(engine, agentId) {
  const seeded = new Set(listSeededSkillNames());
  const all = engine.getAllSkills(agentId) || [];
  const candidates = all
    .filter(skill => skill?.enabled && isDeletableUserSkill(skill, engine.skillsDir, seeded))
    .map(skill => ({ name: skill.name, description: skill.description || "" }));
  const usedElsewhere = collectEnabledSkillsOfOtherAgents(engine.agentsDir, agentId);
  return { skills: candidates.filter(skill => !usedElsewhere.has(skill.name)) };
}

/** 从所有 agent 的 config.yaml 里移除对指定技能的启用引用 */
function scrubSkillReferencesFromAgentConfigs(agentsDir, names) {
  const nameSet = new Set(names);
  if (!agentsDir || !fs.existsSync(agentsDir)) return;
  for (const agentName of fs.readdirSync(agentsDir)) {
    const configPath = path.join(agentsDir, agentName, "config.yaml");
    if (!fs.existsSync(configPath)) continue;
    try {
      const cfg = loadConfig(configPath);
      const enabled = cfg?.skills?.enabled;
      if (Array.isArray(enabled) && enabled.some(name => nameSet.has(name))) {
        saveConfig(configPath, { skills: { enabled: enabled.filter(name => !nameSet.has(name)) } });
      }
    } catch (err) {
      log.error(`清理 agent ${agentName} 的 skill 引用失败: ${err.message}`);
    }
  }
}

/**
 * 物理删除一批用户技能并清理全部引用（agent config + bundle），最后整体 reload 一次。
 * 单个失败不影响其余（记入 skipped），与单个删除端点的既有语义一致。
 */
export async function removeUserSkills(engine, names) {
  const removed = [];
  const skipped = [];
  for (const raw of names) {
    const name = typeof raw === "string" ? raw.trim() : "";
    if (!name || !sanitizeSkillName(name)) {
      skipped.push({ name: String(raw), reason: "invalid_name" });
      continue;
    }
    const dir = path.join(engine.skillsDir, name);
    if (!fs.existsSync(dir)) {
      skipped.push({ name, reason: "not_found" });
      continue;
    }
    try {
      fs.rmSync(dir, { recursive: true, force: true });
      removed.push(name);
    } catch (err) {
      skipped.push({ name, reason: "remove_failed" });
      log.error(`删除技能目录失败 (${name}): ${err.message}`);
    }
  }
  if (removed.length > 0) {
    scrubSkillReferencesFromAgentConfigs(engine.agentsDir, removed);
    if (engine.lingxiHome) {
      try {
        removeSkillsFromBundles(engine, removed);
      } catch (err) {
        log.error(`从 skill bundle 清理技能引用失败: ${err.message}`);
      }
    }
    await engine.reloadSkills();
  }
  return { removed, skipped };
}
