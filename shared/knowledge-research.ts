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

/** 实时过程只传宿主计数和任务身份，不包含模型思考、工具正文或数据库语句。 */
export type KnowledgeResearchProgressUpdate =
  | { type: "knowledge_research_started" | "knowledge_research_plan_updated" }
  | { type: "knowledge_research_round_started"; roundId: string; round: number }
  | { type: "knowledge_research_worker_started"; taskId: string; label: string }
  | { type: "knowledge_research_worker_completed"; taskId: string; label: string; status: "completed" | "failed" | "cancelled" }
  | { type: "knowledge_research_ledger_updated"; phase: "investigating" | "reviewing" }
  | { type: "knowledge_research_completed"; status: "completed" | "partial" | "failed" | "cancelled"; stopReason: string | null };

export type KnowledgeResearchProgress = KnowledgeResearchProgressUpdate & {
  runId: string;
  scopeId: string;
  rounds: number;
  maxRounds: number;
  searchCalls: number;
  readCalls: number;
  delegatedAgents: number;
  needsTotal: number;
  needsSupported: number;
  needsPartial: number;
  needsConflicted: number;
  unresolvedNeedIds: string[];
};
