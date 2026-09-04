import type { KnowledgeReferenceMode } from "./knowledge-refs.ts";
import { deriveKnowledgeCompletenessPolicy } from "../lib/knowledge/research/completeness-policy.ts";

export type KnowledgeExecutionPath = "fast_local" | "detailed_research";

export type KnowledgeCompletenessPolicy =
  | "best_effort"
  | "source_diverse"
  | "relevant_sections_complete"
  | "scope_complete";

export type KnowledgeResponseDetail = "normal" | "detailed";

export interface KnowledgeExecutionPolicy {
  mode: KnowledgeReferenceMode;
  path: KnowledgeExecutionPath;
  completenessPolicy: KnowledgeCompletenessPolicy;
  responseDetail: KnowledgeResponseDetail;
  retrievalDeadlineMs: number | null;
}

/** 执行路径和用户要求的最低完整性统一解析，快速模式始终只做尽力检索。 */
export function resolveKnowledgeExecutionPolicy(input: {
  mode: KnowledgeReferenceMode;
  question: string;
  selectedNotebookCount: number;
  selectedSourceCount: number;
}): KnowledgeExecutionPolicy {
  if (input.mode === "fast") {
    return {
      mode: input.mode,
      path: "fast_local",
      completenessPolicy: "best_effort",
      responseDetail: "normal",
      retrievalDeadlineMs: 1200,
    };
  }
  return {
    mode: input.mode,
    path: "detailed_research",
    completenessPolicy: deriveKnowledgeCompletenessPolicy(input),
    responseDetail: "detailed",
    retrievalDeadlineMs: null,
  };
}
