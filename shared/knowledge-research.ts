export type KnowledgeResearchRunStatus =
  | "planning"
  | "running"
  | "synthesizing"
  | "completed"
  | "partial"
  | "failed"
  | "cancelled";

export type KnowledgeEvidenceNeedKind =
  | "fact"
  | "comparison"
  | "cause"
  | "timeline"
  | "counterexample"
  | "completeness";

export type KnowledgeEvidenceNeedStatus =
  | "uncovered"
  | "partial"
  | "supported"
  | "conflicted"
  | "not_applicable";

export type KnowledgeEvidenceRelation =
  | "supports"
  | "contradicts"
  | "context";

export interface KnowledgeEvidenceNeed {
  id: string;
  ordinal: number;
  claim: string;
  kind: KnowledgeEvidenceNeedKind;
  required: boolean;
  minIndependentSources: number;
  requireCounterEvidence: boolean;
  requireAllRelevantUnits: boolean;
  status: KnowledgeEvidenceNeedStatus;
  evidenceIds: string[];
  counterEvidenceIds: string[];
  unresolvedGaps: string[];
}

export interface KnowledgeResearchBudget {
  maxRounds: number;
  maxParallelAgents: number;
  maxToolCalls: number;
  maxWallClockMs: number;
  maxSearchesPerRound: number;
  maxReadsPerRound: number;
  maxFinalEvidenceSpans: number;
  finalEvidenceBudgetTokens: number;
}

/** 研究预算由宿主统一执行，各轮和各个工作会话不能各自重置。 */
export const DEFAULT_KNOWLEDGE_RESEARCH_BUDGET = {
  maxRounds: 4,
  maxParallelAgents: 4,
  maxToolCalls: 32,
  maxWallClockMs: 180_000,
  maxSearchesPerRound: 8,
  maxReadsPerRound: 12,
  maxFinalEvidenceSpans: 32,
  finalEvidenceBudgetTokens: 16_000,
} as const;
