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

/**
 * 模型引用，与 shared/model-ref.ts 的持久化形状一致：完整 {id, provider}，
 * 任一缺失即非法，不做按 id 降级。
 */
export interface KnowledgeModelRef {
  id: string;
  provider: string;
}

/**
 * 笔记本级摄入/检索配置（schema v6 notebooks 新列的读取形状）。
 * 任一项为 null 表示未设置：模型引用沿解析链 笔记本列 → 全局偏好 → null；
 * 数值项无全局偏好，回退内置默认。见 knowledge-store.ts 的 resolveNotebookConfig。
 */
export interface NotebookConfig {
  embeddingModelRef: KnowledgeModelRef | null;
  rerankModelRef: KnowledgeModelRef | null;
  chunkTargetChars: number | null;
  retrievalTopK: number | null;
  /** null = 永久保留（默认）；正整数 = 旧版本向量 N 天未被查询命中即回收（schema v9）。 */
  vectorRetentionDays: number | null;
}

/** 摄入管线的 phase 链：parse → chunk → fts_index → embed → done。 */
export type IngestionPhase = "parse" | "chunk" | "fts_index" | "embed" | "done";

/**
 * pending_embedding 是显式终态（非失败）：FTS 已可查、嵌入模型未配置，
 * 等模型就绪信号置回 queued 补跑（禁静默降级红线）。
 */
export type IngestionJobStatus = "queued" | "running" | "pending_embedding" | "failed" | "done";

export interface IngestionJob {
  id: string;
  notebookId: string;
  sourceId: string;
  /** parse 完成前未知，随 phase 推进绑定。 */
  artifactId: string | null;
  /** 下一个待执行的 phase；done 表示全部完成。 */
  phase: IngestionPhase;
  status: IngestionJobStatus;
  attempt: number;
  /** 下次可重试时间（ISO）；NULL = 立即可取。 */
  retryAfter: string | null;
  error: string | null;
  /**
   * 触发本次摄入的笔记本分块配置指纹（knowledgeChunkerConfigId）。
   * 权衡（显式记录）：同一源被多个笔记本以不同配置引用时，
   * 分块以触发摄入的笔记本配置为准。
   */
  chunkerConfigId: string;
  /** 嵌入进度：已完成块数（每批嵌入后递增；失败/重试重置为 0）。 */
  progressDone: number;
  /** 嵌入总块数；NULL = 尚未进入 embed 相位（无进度语义）。 */
  progressTotal: number | null;
  createdAt: string;
  updatedAt: string;
}
