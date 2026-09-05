import type { KnowledgeReferenceMode } from "./knowledge-refs.ts";

export type KnowledgeExecutionPath = "conversation" | "fast_local" | "detailed_research";

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

/** 普通知识问答统一在当前聊天查阅；不再用问题关键词强制整库扫描或独立调查。 */
export function resolveKnowledgeExecutionPolicy(input: {
  mode: KnowledgeReferenceMode;
  question: string;
  selectedNotebookCount: number;
  selectedSourceCount: number;
}): KnowledgeExecutionPolicy {
  return {
    mode: "auto",
    path: "conversation",
    completenessPolicy: "best_effort",
    responseDetail: input.mode === "detailed" ? "detailed" : "normal",
    retrievalDeadlineMs: null,
  };
}
