import type { KnowledgeChunkerStrategy, KnowledgeChunkSpanDraft } from "./chunker.ts";
import type { KnowledgeCoverageMode } from "../../shared/knowledge-refs.ts";

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
  /**
   * 显式删除标记（Phase 5 §十九 delete wins）：deleteSource 置位，此后一切
   * ensure/enqueue/refresh 经 activeSource 显式失败（KNOWLEDGE_NOT_FOUND），
   * 行本身随后由物理清理删除。
   */
  deletedAt: string | null;
  /**
   * orphan 标记（Phase 5 §十八）：零活跃 membership 时置位；保留期
   * （KNOWLEDGE_ORPHAN_SOURCE_RETENTION_MS）过后由 GC 物理清理。重新加入
   * 任一笔记本即清除（复活语义）。
   */
  orphanedAt: string | null;
}

export interface NotebookSourceMembership {
  notebookId: string;
  sourceId: string;
  addedAt: string;
  removedAt: string | null;
  /**
   * 目录组织路径（任务书 §六十九：目录路径属于 Membership，同一 Source 在不同
   * Notebook 可有不同位置）。目录导入时写入（如 "技术文档/API/x.docx"）。
   */
  relativePath: string | null;
  /** 目录层级节点（"/" 分层，如 "技术文档/API"）；无目录语境为 null。 */
  folderNode: string | null;
  displayOrder: number | null;
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
  /**
   * 证据可信度等级（任务书 §五十九）：citation_grade（精确行/页坐标）/
   * structural（结构坐标，如 DOCX 段落、XLSX 单元格）/semantic_only（无反向定位）。
   * v16 起持久化；legacy 行默认 citation_grade。
   */
  fidelity: "citation_grade" | "structural" | "semantic_only";
  /** 经 ProcessingArtifact 管线（§五十八）转换的 artifact 记录其来源 id。 */
  processingArtifactId: string | null;
  createdAt: string;
  completedAt: string | null;
}

/** §五十八 ProcessingArtifact：二进制格式 → 结构化文本的持久化转换产物。 */
export interface KnowledgeProcessingArtifact {
  id: string;
  contentSnapshotId: string;
  processorId: string;
  processorVersion: string;
  processorConfigHash: string;
  status: "processing" | "ready" | "failed";
  fidelity: "citation_grade" | "structural" | "semantic_only" | null;
  outputMime: string | null;
  outputPath: string | null;
  locatorMap: Record<string, unknown>;
  warnings: string[];
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
}

/**
 * ChunkProfile 类型（任务书 §76）：
 * - standard：配置值可推导（真实 strategy/targetChars，禁伪造）；
 * - legacy：历史遗留指纹（chunker_config_id）无法反推出配置，只保留身份键。
 */
export type KnowledgeChunkProfileType = "standard" | "legacy";

/** targetChars 来源诊断：explicit = 笔记本显式列；auto = 按嵌入模型上下文窗口自动派生。 */
export type KnowledgeChunkTargetCharsSource = "explicit" | "auto";

/**
 * ChunkProfile（schema v9 chunk_profiles）：分块配置的注册身份。
 * profileHash 复用 knowledgeChunkerConfigId 算法输出（16 hex），是跨库身份键——
 * 与 ingestion_jobs.chunker_config_id、索引库 artifact_indexes.chunker_version 同源。
 * legacy 行的 strategy/targetChars/chunkerVersion 为 NULL（不可推导，不伪造）。
 */
export interface KnowledgeChunkProfile {
  /** 确定性 id：'cp_' + profileHash。 */
  id: string;
  profileHash: string;
  strategy: KnowledgeChunkerStrategy | null;
  /** 解析后的具体值（NULL=自动时存 resolveEffectiveChunkTargetChars 的结果）。 */
  targetChars: number | null;
  targetCharsSource: KnowledgeChunkTargetCharsSource | null;
  chunkerVersion: string | null;
  structuralOptions: Record<string, unknown> | null;
  profileType: KnowledgeChunkProfileType;
  createdAt: string;
}

/**
 * RetrievalProfile（schema v9 retrieval_profiles）：笔记本绑定的检索配置身份。
 * profileKey = chunkProfileHash + embeddingModelRef + rerankModelRef + retrievalTopK
 * 规范化 JSON 的 sha256 前 16 hex；同配置跨笔记本共享同一 profile（共享派生索引）。
 */
export interface KnowledgeRetrievalProfile {
  /** 确定性 id：'rp_' + profileKey。 */
  id: string;
  profileKey: string;
  chunkProfileId: string;
  embeddingModelRef: KnowledgeModelRef | null;
  rerankModelRef: KnowledgeModelRef | null;
  /** null = 无上限（与 notebooks.retrieval_top_k 的 v8 语义一致）。 */
  retrievalTopK: number | null;
  createdAt: string;
}

/**
 * KnowledgeTurnScope 状态（schema v11 knowledge_turn_scopes.status）：
 * active = 该会话当前轮的知识权限天花板；closed = 被同会话新一轮 scope
 * supersede（行保留供 EvidenceManifest 追溯，读取侧拒绝已关闭 scope）。
 */
export type KnowledgeTurnScopeStatus = "active" | "closed";

/**
 * 轮级 scope 冻结的单源条目（schema v11 knowledge_turn_scope_sources）：
 * 创建时冻结该源当前最新 ContentSnapshot / ParseArtifact——本轮即使 watcher
 * 产生新版本，读取仍锚定这里的冻结版本（任务书 §四十三）。
 */
export interface KnowledgeTurnScopeSource {
  scopeId: string;
  sourceId: string;
  contentSnapshotId: string;
  /** NULL = 冻结时刻该源尚无 parse artifact（读取显式报 KNOWLEDGE_PARSE_NOT_READY）。 */
  parseArtifactId: string | null;
  /** 选中集合内引用该源的笔记本 id（按选择顺序）。 */
  notebookIds: string[];
}

/**
 * KnowledgeTurnScope（任务书 §二十，Phase 4）：用户本轮选择的 Notebook 集合
 * 提升为服务端强制的知识权限天花板。selectedNotebookIds + 冻结的
 * Source/ContentSnapshot/ParseArtifact 集合在创建时一次性落库（同事务），
 * 之后 knowledge_read / 注入链路只认这份冻结集合。
 */
export interface KnowledgeTurnScope {
  id: string;
  /** 会话轮标识：提交方给的 clientMessageId，缺省时 store 生成 turn_<uuid>。 */
  turnId: string;
  sessionPath: string;
  studioId: string;
  notebookIds: string[];
  status: KnowledgeTurnScopeStatus;
  createdAt: string;
  sources: KnowledgeTurnScopeSource[];
}

/**
 * EvidenceManifest 的单源身份条目（schema v15 evidence_manifest_entries，
 * 任务书 §六十七）：一轮 Knowledge Answer 实际读取的证据身份链，按
 * (source, chunkIndexVariant) 分组——同源被多个不同分块配置的笔记本引用时
 * 一轮可能经多个变体读取，ordinal 区分。只存 id/序号/偏移等身份与定位
 * 元数据，绝不存 chunk 正文或任何模型输出。
 */
export interface KnowledgeEvidenceManifestEntry {
  /** manifest 内的条目序号（0-based，写入序）。 */
  ordinal: number;
  sourceId: string;
  /** 该源本轮读取锚定的冻结快照（TurnScope 冻结行复核）。 */
  contentSnapshotId: string;
  /** 冻结的解析产物；null = 冻结时刻尚无 artifact（无 chunk 身份可记）。 */
  parseArtifactId: string | null;
  /** 分块配置指纹（ChunkProfile 身份键）；null = 无法解析（不伪造）。 */
  chunkProfileHash: string | null;
  chunkIndexVariantId: string | null;
  /** 本轮实际参与向量检索的 VectorIndexVariant id（fts-only / 未就绪为空）。 */
  vectorIndexVariantIds: string[];
  /** 检索命中并注入的 chunk id（含蒸馏路径的蒸馏输入锚点）。 */
  chunkIds: string[];
  /** 邻接扩展注入的 context-only chunk id（§三十六，不计检索命中）。 */
  neighborChunkIds: string[];
  /** chunk → knowledge_blocks 偏移定位（block id + chunk/block 双层偏移）。 */
  blockSpans: Array<{ chunkId: string; spans: KnowledgeChunkSpanDraft[] }>;
  /** 该源证据块在注入块中的引用标签（[KN] 编号；蒸馏节锚点带节编号）。 */
  citationLabels: string[];
}

/**
 * 每轮 Knowledge Answer 的轻量证据清单（schema v15 evidence_manifests，
 * 任务书 §六十七）：记录该轮回答基于哪个 snapshot/variant/chunks，供
 * Source 后续更新时追溯旧回答的证据版本。coverage_mode 是计划档位、
 * executed_coverage_mode 是实际执行档位（Phase 8/9 语义）；exhaustive 轮
 * 额外关联 coverage run（runId + manifestHash，条目级身份在 coverage_runs
 * 的冻结 manifest 内）。不存 CoT、模型输出原文或 chunk 正文。
 */
export interface KnowledgeEvidenceManifest {
  id: string;
  turnScopeId: string;
  sessionPath: string;
  turnId: string;
  notebookIds: string[];
  coverageMode: KnowledgeCoverageMode | null;
  executedCoverageMode: "high_recall" | "broad" | "exhaustive" | null;
  coverageRunId: string | null;
  coverageManifestHash: string | null;
  createdAt: string;
  entries: KnowledgeEvidenceManifestEntry[];
}

/** 摄入管线的 phase 链：parse → chunk → fts_index → embed → done。 */
export type IngestionPhase = "parse" | "chunk" | "fts_index" | "embed" | "done";

/**
 * pending_embedding 是显式终态（非失败）：FTS 已可查、嵌入模型未配置，
 * 等模型就绪信号置回 queued 补跑（禁静默降级红线）。
 */
export type IngestionJobStatus = "queued" | "running" | "pending_embedding" | "failed" | "done";

/**
 * embed 相位的成本观测快照（任务书 §七十四，Phase 3 批级 checkpoint）：
 * embed 相位每次执行结束（embedded/skipped/unavailable）时由摄入 worker 落
 * ingestion_jobs.embedding_stats（JSON），后端可查询；请求级 token/次数台账
 * 仍由 usageContext 走模型调用可观测性，这里记 chunk 级账目。
 */
export interface KnowledgeIngestionEmbeddingStats {
  /** 本轮真实调用嵌入并写入 checkpoint 的块数（skipped 的探测批不计入）。 */
  chunksNewlyEmbedded: number;
  /** 本轮从既有 checkpoint（building 变体已落库向量）续用的块数。 */
  chunksResumedFromCheckpoint: number;
  /** 已 ready 变体整体命中而免于重嵌的块数（duplicate embedding avoided）。 */
  chunksReusedFromReadyVariant: number;
  /** 本轮发出的嵌入请求批次数（每批一次 provider 调用；skipped 的探测批计入）。 */
  requestCount: number;
  /** 真实响应确认的模型身份；unavailable 且无任何响应时为 null。 */
  model: {
    key: string;
    provider: string;
    modelId: string;
    protocol: string;
    dimensions: number;
  } | null;
  /** begin 时检测到指纹/维度漂移，旧向量已显式清除重建（留痕字段）。 */
  resetStaleVectors: boolean;
  /** 断点恢复时检测到模型已更换而被显式落 failed 的旧 building 变体 id。 */
  abandonedStaleVariantId: string | null;
}

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
  /**
   * 显式取消留痕（Phase 5 §十九 delete wins）：deleteSource 取消本 job 时置位。
   * status CHECK 约束无法 ALTER 增加 'cancelled' 枚举，取消复用 failed 终态 +
   * 本列显式标注；requeueIngestionJob 拒绝 cancelled 行，被删源永不复活。
   */
  cancelledAt: string | null;
  /** 嵌入进度：已完成块数（每批嵌入并持久化后递增；失败保留供诊断，手动重试重置为 0）。 */
  progressDone: number;
  /** 嵌入总块数；NULL = 尚未进入 embed 相位（无进度语义）。 */
  progressTotal: number | null;
  /** 最近一次 embed 相位执行的成本观测（§七十四）；NULL = 尚未执行过 embed。 */
  embeddingStats: KnowledgeIngestionEmbeddingStats | null;
  createdAt: string;
  updatedAt: string;
}
