export type KnowledgeSourceType = "file" | "pasted_text" | "web_snapshot";

export interface KnowledgeNotebook {
  id: string;
  studioId: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
}

export interface KnowledgeSource {
  id: string;
  studioId: string;
  sourceType: KnowledgeSourceType;
  displayName: string;
  originMetadata: Record<string, unknown>;
  createdAt: string;
  deletedAt: string | null;
}

export interface NotebookSourceMembership {
  notebookId: string;
  sourceId: string;
  addedAt: string;
  removedAt: string | null;
}

export interface ContentSnapshot {
  id: string;
  sourceId: string;
  sha256: string;
  mimeType: string;
  byteSize: number;
  /** 相对 knowledge 根目录保存，不能持久化为机器专属绝对路径。 */
  storagePath: string;
  capturedAt: string;
}

export interface ImportedKnowledgeSource {
  source: KnowledgeSource;
  snapshot: ContentSnapshot;
  membership: NotebookSourceMembership;
}

export type KnowledgeParseStatus = "parsing" | "ready" | "needs_ocr" | "failed";

export interface KnowledgeParseArtifact {
  id: string;
  contentSnapshotId: string;
  parserId: string;
  parserVersion: string;
  parserConfigHash: string;
  status: KnowledgeParseStatus;
  warnings: string[];
  semanticArtifactPath: string | null;
  createdAt: string;
  completedAt: string | null;
}

export interface KnowledgeBlock {
  id: string;
  parseArtifactId: string;
  ordinal: number;
  text: string;
  textSha256: string;
  locatorType: "text" | "markdown" | "pdf" | "html";
  locator: Record<string, unknown>;
}

export interface KnowledgeCitation {
  id: string;
  parseArtifactId: string;
  blockId: string;
  startOffset: number;
  endOffset: number;
  canonicalText: string;
  canonicalTextSha256: string;
  createdAt: string;
}

export interface ResolvedKnowledgeCitation {
  citation: KnowledgeCitation;
  block: KnowledgeBlock;
  artifact: KnowledgeParseArtifact;
  snapshot: ContentSnapshot;
  source: KnowledgeSource;
}

export type KnowledgeQueryMode = "quick" | "research";

export interface KnowledgeScopeNotebook {
  scopeSnapshotId: string;
  notebookId: string;
  notebookName: string;
  ordinal: number;
}

export interface KnowledgeScopeSource {
  scopeSnapshotId: string;
  notebookId: string;
  sourceId: string;
  sourceDisplayName: string;
  contentSnapshotId: string;
  parseArtifactId: string;
  ordinal: number;
}

export interface KnowledgeScopeSnapshot {
  id: string;
  studioId: string;
  mode: KnowledgeQueryMode;
  createdAt: string;
  notebooks: KnowledgeScopeNotebook[];
  sources: KnowledgeScopeSource[];
}

export type KnowledgeRunStatus = "running" | "completed" | "failed" | "cancelled";

export interface KnowledgeRunCitationRef {
  runId: string;
  ordinal: number;
  marker: number;
  citationId: string;
  candidateRef: string;
}

export interface KnowledgeRunRetrieval {
  runId: string;
  rank: number;
  chunkId: string;
  parseArtifactId: string;
  score: number;
}

export interface KnowledgeRun {
  id: string;
  studioId: string;
  mode: KnowledgeQueryMode;
  question: string;
  scopeSnapshotId: string;
  status: KnowledgeRunStatus;
  retrievalMode: "fts" | "hybrid";
  answerText: string | null;
  errorCode: string | null;
  createdAt: string;
  completedAt: string | null;
  citations: KnowledgeRunCitationRef[];
  retrievals: KnowledgeRunRetrieval[];
}

export type KnowledgeResearchState =
  | "queued"
  | "preparing_scope"
  | "building_manifest"
  | "scanning"
  | "building_claims"
  | "checking_contradictions"
  | "synthesizing"
  | "completed"
  | "recovering"
  | "partial"
  | "failed"
  | "canceled";

export type KnowledgeResearchWorkStatus = "pending" | "running" | "completed" | "failed" | "canceled";
export type KnowledgeSupportStatus = "supported" | "partial" | "disputed" | "insufficient";
export type KnowledgeEpistemicBasis = "explicit" | "inferred" | "mixed";
export type KnowledgeClaimEvidenceRelation = "supports" | "contradicts" | "context";

export interface KnowledgeResearchSpec {
  originalQuestion: string;
  scopeSnapshotId: string;
  notebookIds: string[];
  goal: string;
  dimensions: string[];
  outputRequirements: string[];
  definitions: string[];
  assumptions: string[];
}

export interface KnowledgeAnalysisUnitSpan {
  kind: "primary" | "context";
  ordinal: number;
  blockId: string;
  blockOrdinal: number;
  startOffset: number;
  endOffset: number;
}

export interface KnowledgeAnalysisUnit {
  id: string;
  runId: string;
  parseArtifactId: string;
  ordinal: number;
  priority: number;
  status: KnowledgeResearchWorkStatus;
  primaryCharCount: number;
  contextCharCount: number;
  completedAt: string | null;
  errorCode: string | null;
  spans: KnowledgeAnalysisUnitSpan[];
}

export interface KnowledgeAnalysisManifest {
  runId: string;
  sourceCount: number;
  parseArtifactCount: number;
  blockCount: number;
  unitCount: number;
  primaryCharCount: number;
  createdAt: string;
}

export interface KnowledgeResearchCoverageMetric {
  completed: number;
  total: number;
}

export interface KnowledgeResearchCoverage {
  sourceReadiness: KnowledgeResearchCoverageMetric;
  extraction: KnowledgeResearchCoverageMetric;
  primaryScan: KnowledgeResearchCoverageMetric;
  contradiction: KnowledgeResearchCoverageMetric;
  citationValidation: KnowledgeResearchCoverageMetric & { valid: number; invalid: number };
}

export interface KnowledgeResearchRun {
  runId: string;
  hostTaskId: string;
  state: KnowledgeResearchState;
  spec: KnowledgeResearchSpec;
  manifest: KnowledgeAnalysisManifest | null;
  coverage: KnowledgeResearchCoverage;
  reportAvailable: boolean;
  errorCode: string | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface KnowledgeResearchReportCitation {
  marker: number;
  evidenceId: string;
  citationId: string;
}

export interface KnowledgeResearchReportItem {
  text: string;
  claimIds: string[];
  citationMarkers: number[];
}

export interface KnowledgeResearchReport {
  runId: string;
  title: string;
  summary: string;
  conclusions: KnowledgeResearchReportItem[];
  majorFindings: KnowledgeResearchReportItem[];
  conflicts: KnowledgeResearchReportItem[];
  uncertainties: string[];
  limitations: string[];
  coverage: KnowledgeResearchCoverage;
  citations: KnowledgeResearchReportCitation[];
  createdAt: string;
}
