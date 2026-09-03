import type { KnowledgeReferenceMode } from "./knowledge-refs.ts";

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

/** 执行策略统一在此解析；问题驱动的完整性升级在 P3 接入。 */
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
    completenessPolicy: "source_diverse",
    responseDetail: "detailed",
    retrievalDeadlineMs: null,
  };
}
