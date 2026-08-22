import { resolveSessionSkillsForRuntime } from "../lib/skills/session-skill-snapshot.ts";
import { sanitizeSemanticInputSection } from "../lib/llm/semantic-input-provenance.ts";

export const SESSION_PROMPT_SNAPSHOT_VERSION = 1;

function jsonClone(value, fallback) {
  try {
    return JSON.parse(JSON.stringify(value));
  } catch {
    return fallback;
  }
}

export function normalizeStringArray(value) {
  return Array.isArray(value) ? value.filter((item) => typeof item === "string") : [];
}

export function freezeSkillsResult(value) {
  const next = {
    skills: Array.isArray(value?.skills) ? value.skills : [],
    diagnostics: Array.isArray(value?.diagnostics) ? value.diagnostics : [],
  };
  return jsonClone(next, { skills: [], diagnostics: [] });
}

export function freezeAgentsFilesResult(value) {
  const next = {
    agentsFiles: Array.isArray(value?.agentsFiles) ? value.agentsFiles : [],
  };
  return jsonClone(next, { agentsFiles: [] });
}

/**
 * Phase 5（§十三/§十四）：冻结快照附带的安全 provenance metadata——
 * 只有 category/locator/source/precision，不含任何 Prompt 内容副本。
 * 逐段 sanitize（fail closed）；非法段丢弃，全部非法/缺失 → null。
 * 旧 snapshot 无此字段 → null（恢复后诚实 structural，§八十五）。
 */
export function freezeSystemPromptProvenance(value) {
  if (!Array.isArray(value)) return null;
  const sections = [];
  for (const section of value) {
    const safe = sanitizeSemanticInputSection(section);
    if (safe) sections.push(safe);
  }
  return sections.length > 0 ? sections : null;
}

export function buildSessionPromptSnapshot({
  systemPrompt = "",
  appendSystemPrompt = [],
  skillsResult = null,
  agentsFilesResult = null,
  systemPromptProvenance = null,
} = {}) {
  const provenance = freezeSystemPromptProvenance(systemPromptProvenance);
  return {
    version: SESSION_PROMPT_SNAPSHOT_VERSION,
    systemPrompt: String(systemPrompt || ""),
    appendSystemPrompt: normalizeStringArray(appendSystemPrompt),
    skillsResult: freezeSkillsResult(skillsResult),
    agentsFilesResult: freezeAgentsFilesResult(agentsFilesResult),
    ...(provenance ? { systemPromptProvenance: provenance } : {}),
  };
}

export function normalizeSessionPromptSnapshot(value) {
  if (!value || typeof value !== "object") return null;
  if (value.version !== SESSION_PROMPT_SNAPSHOT_VERSION) return null;
  if (typeof value.systemPrompt !== "string") return null;
  const provenance = freezeSystemPromptProvenance(value.systemPromptProvenance);
  return {
    version: SESSION_PROMPT_SNAPSHOT_VERSION,
    systemPrompt: value.systemPrompt,
    appendSystemPrompt: normalizeStringArray(value.appendSystemPrompt),
    skillsResult: freezeSkillsResult(value.skillsResult),
    agentsFilesResult: freezeAgentsFilesResult(value.agentsFilesResult),
    ...(typeof value.finalSystemPrompt === "string"
      ? { finalSystemPrompt: value.finalSystemPrompt }
      : {}),
    ...(provenance ? { systemPromptProvenance: provenance } : {}),
  };
}

export function createPromptSnapshotResourceLoader(baseResourceLoader, snapshot, extraProps = {}) {
  const normalized = normalizeSessionPromptSnapshot(snapshot)
    || buildSessionPromptSnapshot({ systemPrompt: "" });
  return Object.create(baseResourceLoader || {}, {
    getSystemPrompt: {
      value: () => normalized.systemPrompt,
    },
    getAppendSystemPrompt: {
      value: () => [...normalized.appendSystemPrompt],
    },
    getSkills: {
      value: () => resolveSessionSkillsForRuntime(normalized.skillsResult),
    },
    getAgentsFiles: {
      value: () => freezeAgentsFilesResult(normalized.agentsFilesResult),
    },
    ...extraProps,
  });
}
