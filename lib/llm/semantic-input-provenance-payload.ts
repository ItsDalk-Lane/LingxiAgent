/**
 * semantic-input-provenance-payload.ts — Session 冻结快照 → stream observer
 * provenance payload 的装配 helper。
 *
 * 独立小模块：契约（semantic-input-provenance.ts）保持纯 schema/构造器，
 * 这里只做「快照各部分 → SessionPromptProvenancePayload」的字段搬运与
 * 安全清洗（skill/agents-file 名取逻辑名，basename 兜底，绝不放绝对路径——§四十九）。
 */
import path from "node:path";
import type { SessionPromptProvenancePayload } from "./semantic-input-provenance.ts";

const MAX_IDENTITY_NAMES = 8;

function logicalNameList(values: unknown, nameOf: (entry: any) => string | null): string[] {
  if (!Array.isArray(values)) return [];
  const out: string[] = [];
  for (const entry of values) {
    const name = nameOf(entry);
    if (name && name.length <= 64 && !out.includes(name)) out.push(name);
    if (out.length >= MAX_IDENTITY_NAMES) break;
  }
  return out;
}

export function buildSessionPromptProvenancePayload({
  systemPrompt,
  provenanceSections,
  appendSystemPrompt = null,
  skillsResult = null,
  agentsFilesResult = null,
}: {
  systemPrompt: string;
  provenanceSections: unknown;
  appendSystemPrompt?: string[] | null;
  skillsResult?: { skills?: unknown[] } | null;
  agentsFilesResult?: { agentsFiles?: unknown[] } | null;
}): SessionPromptProvenancePayload | null {
  if (!Array.isArray(provenanceSections) || provenanceSections.length === 0) return null;
  if (typeof systemPrompt !== "string" || systemPrompt.length === 0) return null;
  const skillNames = logicalNameList(skillsResult?.skills, (entry) =>
    typeof entry?.name === "string" && entry.name.trim() ? entry.name.trim() : null);
  const agentsFileNames = logicalNameList(agentsFilesResult?.agentsFiles, (entry) => {
    if (typeof entry?.name === "string" && entry.name.trim()) return entry.name.trim();
    // basename 兜底：只保留逻辑文件名，绝不携带目录（绝对路径禁入 provenance）。
    if (typeof entry?.path === "string" && entry.path.trim()) {
      return path.basename(entry.path.trim());
    }
    return null;
  });
  return {
    customPrompt: systemPrompt,
    sections: provenanceSections as SessionPromptProvenancePayload["sections"],
    appendSystemPrompt: Array.isArray(appendSystemPrompt) ? appendSystemPrompt : null,
    skillNames,
    agentsFileNames,
  };
}
