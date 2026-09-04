import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { createRequire } from "node:module";

import { KnowledgeError } from "./errors.ts";
import {
  KNOWLEDGE_CHUNK_TARGET_CHARS,
  KNOWLEDGE_CHUNKER_VERSION,
  MAX_KNOWLEDGE_CHUNK_TARGET_CHARS,
  MIN_KNOWLEDGE_CHUNK_TARGET_CHARS,
  computeAutoChunkTargetChars,
  knowledgeChunkerConfigId,
} from "./chunker.ts";
import type { KnowledgeChunkerStrategy } from "./chunker.ts";
import type {
  ContentSnapshot,
  ImportedKnowledgeSource,
  IngestionJob,
  IngestionJobStatus,
  IngestionPhase,
  KnowledgeBlock,
  KnowledgeChunkProfile,
  KnowledgeChunkTargetCharsSource,
  KnowledgeChunkProfileType,
  KnowledgeCitation,
  KnowledgeEvidenceManifest,
  KnowledgeEvidenceManifestEntry,
  KnowledgeIngestionEmbeddingStats,
  KnowledgeModelRef,
  KnowledgeNotebook,
  KnowledgeParseArtifact,
  KnowledgeParseStatus,
  KnowledgeProcessingArtifact,
  KnowledgeRetrievalProfile,
  KnowledgeSource,
  KnowledgeSourceType,
  KnowledgeTurnScope,
  KnowledgeTurnScopeSource,
  NotebookConfig,
  NotebookSourceMembership,
  ResolvedKnowledgeCitation,
} from "./types.ts";
import type { KnowledgeBlockDraft } from "./source-adapters.ts";
import { KNOWLEDGE_EMBEDDING_INTERRUPTED } from "../../shared/knowledge-reason-codes.ts";
import {
  isKnowledgeCoverageClassifierUsed,
  isKnowledgeCoverageIntent,
  isKnowledgeCoverageMode,
  isKnowledgeCoverageScopeLevel,
  type KnowledgeCoveragePlan,
  type KnowledgeCoveragePlanRecord,
} from "./knowledge-coverage-planner.ts";

export const KNOWLEDGE_SCHEMA_VERSION = 19;

const SOURCE_TYPES = new Set<KnowledgeSourceType>(["file", "pasted_text", "web_snapshot"]);
const PARSE_STATUSES = new Set<KnowledgeParseStatus>(["parsing", "ready", "needs_ocr", "failed"]);
const INGESTION_PHASES = new Set<IngestionPhase>(["parse", "chunk", "fts_index", "embed", "done"]);
const INGESTION_STATUSES = new Set<IngestionJobStatus>(["queued", "running", "pending_embedding", "failed", "done"]);
const PROCESSING_STATUSES = new Set<KnowledgeProcessingArtifact["status"]>([
  "processing", "ready", "failed",
]);
const PROCESSING_FIDELITIES = new Set(["citation_grade", "structural", "semantic_only"]);
/** 与 chunker.ts 的 KnowledgeChunkerStrategy 保持一致（策略由首块 locatorType 派发）。 */
const CHUNK_PROFILE_STRATEGIES: readonly KnowledgeChunkerStrategy[] = ["fixed", "markdown", "text", "pdf", "html"];
const CHUNK_PROFILE_TYPES = new Set<KnowledgeChunkProfileType>(["standard", "legacy"]);
const CHUNK_TARGET_CHARS_SOURCES = new Set<KnowledgeChunkTargetCharsSource>(["explicit", "auto"]);

/**
 * retrieval_top_k 的 sanity 边界。v8 起笔记本列 NULL = 无上限（返回全部
 * 匹配块）；"无上限"在检索核心的物理边界就是 MAX（防病态全表膨胀）。
 * KNOWLEDGE_DEFAULT_RETRIEVAL_TOP_K 已随 v8 语义反转退役，保留导出仅供
 * 旧测试/文档理解 v7 及以前的行为（NULL 当时回退 12）。
 */
export const KNOWLEDGE_DEFAULT_RETRIEVAL_TOP_K = 12;
const MIN_RETRIEVAL_TOP_K = 1;
export const MAX_RETRIEVAL_TOP_K = 1000;
/** 向量保留天数边界：1 天 ~ 10 年。 */
export const MIN_VECTOR_RETENTION_DAYS = 1;
export const MAX_VECTOR_RETENTION_DAYS = 3650;
const require = createRequire(import.meta.url);
let BetterSqliteDatabase: any = null;

function loadDatabase() {
  if (!BetterSqliteDatabase) {
    const mod = require("better-sqlite3");
    BetterSqliteDatabase = mod?.default || mod;
  }
  return BetterSqliteDatabase;
}

function requiredString(value: unknown, field: string, maxLength: number): string {
  if (typeof value !== "string") {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be a string`);
  }
  const normalized = value.trim();
  if (!normalized) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must not be empty`);
  }
  if (normalized.length > maxLength) {
    throw new KnowledgeError(
      "KNOWLEDGE_INVALID_ARGUMENT",
      `${field} exceeds the ${maxLength} character limit`,
      { field, maxLength },
    );
  }
  return normalized;
}

function parseObjectJson(value: unknown, field: string): Record<string, unknown> {
  try {
    const parsed = JSON.parse(typeof value === "string" ? value : "");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) throw new Error("not an object");
    return parsed;
  } catch {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
}

function serializeObjectJson(value: unknown, field: string): string {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be an object`);
  }
  const serialized = JSON.stringify(value);
  if (Buffer.byteLength(serialized, "utf8") > 64 * 1024) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} is too large`);
  }
  return serialized;
}

function parseStringArrayJson(value: unknown, field: string): string[] {
  try {
    const parsed = JSON.parse(typeof value === "string" ? value : "");
    if (!Array.isArray(parsed) || parsed.some((entry) => typeof entry !== "string")) {
      throw new Error("not a string array");
    }
    return parsed;
  } catch {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
}

function serializeStringArray(value: unknown, field: string): string {
  if (!Array.isArray(value) || value.some((entry) => typeof entry !== "string" || entry.length > 256)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be a string array`);
  }
  return JSON.stringify(value);
}

/** 覆盖档位列的可空校验（evidence_manifests 头；null = 未知/未接入，不伪造）。 */
function optionalCoverageMode(value: unknown, field: string): "high_recall" | "broad" | "exhaustive" | null {
  if (value == null) return null;
  if (value !== "high_recall" && value !== "broad" && value !== "exhaustive") {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} is invalid`);
  }
  return value;
}

/** chunk 配置指纹（knowledgeChunkerConfigId 输出：sha256 前 16 hex）。 */
function chunkProfileHashValue(value: unknown, field: string): string {
  if (typeof value !== "string" || !/^[0-9a-f]{16}$/u.test(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be 16 lowercase hex characters`);
  }
  return value;
}

/**
 * block spans 序列化（evidence_manifest_entries）：形状校验 + chunkId 归属复核
 * （span 的 chunkId 必须在 chunkIds ∪ neighborChunkIds 内——身份链不收录
 * 未申报的 chunk）+ 总量上限（防失控膨胀，显式报错不静默截断）。
 */
function serializeBlockSpans(
  value: unknown,
  field: string,
  chunkIdsJson: string,
  neighborChunkIdsJson: string,
): string {
  if (!Array.isArray(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be an array`);
  }
  const declared = new Set([
    ...(JSON.parse(chunkIdsJson) as string[]),
    ...(JSON.parse(neighborChunkIdsJson) as string[]),
  ]);
  const serialized = JSON.stringify(value.map((span: any) => {
    if (
      !span || typeof span !== "object"
      || typeof span.chunkId !== "string"
      || !Array.isArray(span.spans)
      || span.spans.some((entry: any) => (
        !entry || typeof entry !== "object"
        || typeof entry.blockId !== "string"
        || !Number.isSafeInteger(entry.blockStartOffset)
        || !Number.isSafeInteger(entry.blockEndOffset)
        || !Number.isSafeInteger(entry.chunkStartOffset)
        || !Number.isSafeInteger(entry.chunkEndOffset)
      ))
    ) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} entries must be block span groups`);
    }
    if (!declared.has(span.chunkId)) {
      throw new KnowledgeError(
        "KNOWLEDGE_INVALID_ARGUMENT",
        `${field} references chunk ${span.chunkId} that is not declared in chunk/neighbor ids`,
      );
    }
    return { chunkId: span.chunkId, spans: span.spans };
  }));
  if (Buffer.byteLength(serialized, "utf8") > 2 * 1024 * 1024) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} exceeds the 2MB limit`);
  }
  return serialized;
}

function storagePath(value: unknown): string {
  const normalized = requiredString(value, "storagePath", 1024);
  if (
    path.isAbsolute(normalized)
    || normalized.includes("\\")
    || normalized.split("/").some((segment) => segment === ".." || segment === "")
  ) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "storagePath must be a safe relative path");
  }
  return normalized;
}

function sourceType(value: unknown): KnowledgeSourceType {
  if (typeof value !== "string" || !SOURCE_TYPES.has(value as KnowledgeSourceType)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "sourceType is unsupported");
  }
  return value as KnowledgeSourceType;
}

function sha256(value: unknown): string {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/u.test(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "sha256 must be 64 lowercase hex characters");
  }
  return value;
}

function byteSize(value: unknown): number {
  if (!Number.isSafeInteger(value) || Number(value) < 0) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "byteSize must be a non-negative integer");
  }
  return Number(value);
}

function toNotebook(row: any): KnowledgeNotebook | null {
  if (!row) return null;
  return {
    id: row.id,
    studioId: row.studio_id,
    name: row.name,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
    deletedAt: row.deleted_at || null,
  };
}

function toSource(row: any): KnowledgeSource | null {
  if (!row) return null;
  return {
    id: row.id,
    studioId: row.studio_id,
    sourceType: row.source_type,
    displayName: row.display_name,
    originMetadata: parseObjectJson(row.origin_metadata_json, "origin metadata"),
    createdAt: row.created_at,
    deletedAt: row.deleted_at || null,
    orphanedAt: row.orphaned_at || null,
  };
}

function toMembership(row: any): NotebookSourceMembership | null {
  if (!row) return null;
  return {
    notebookId: row.notebook_id,
    sourceId: row.source_id,
    addedAt: row.added_at,
    removedAt: row.removed_at || null,
    relativePath: row.relative_path ?? null,
    folderNode: row.folder_node ?? null,
    displayOrder: row.display_order == null ? null : Number(row.display_order),
  };
}

function toSnapshot(row: any): ContentSnapshot | null {
  if (!row) return null;
  return {
    id: row.id,
    sourceId: row.source_id,
    sha256: row.sha256,
    mimeType: row.mime_type,
    byteSize: Number(row.byte_size),
    storagePath: row.storage_path,
    capturedAt: row.captured_at,
  };
}

function toParseArtifact(row: any): KnowledgeParseArtifact | null {
  if (!row) return null;
  return {
    id: row.id,
    contentSnapshotId: row.content_snapshot_id,
    parserId: row.parser_id,
    parserVersion: row.parser_version,
    parserConfigHash: row.parser_config_hash,
    status: row.status,
    warnings: parseStringArrayJson(row.warnings_json, "parse warnings"),
    semanticArtifactPath: row.semantic_artifact_path || null,
    fidelity: row.fidelity ?? "citation_grade",
    processingArtifactId: row.processing_artifact_id ?? null,
    createdAt: row.created_at,
    completedAt: row.completed_at || null,
  };
}

function toProcessingArtifact(row: any): KnowledgeProcessingArtifact | null {
  if (!row) return null;
  return {
    id: row.id,
    contentSnapshotId: row.content_snapshot_id,
    processorId: row.processor_id,
    processorVersion: row.processor_version,
    processorConfigHash: row.processor_config_hash,
    status: row.status,
    fidelity: row.fidelity ?? null,
    outputMime: row.output_mime ?? null,
    outputPath: row.output_path ?? null,
    locatorMap: parseObjectJson(row.locator_map_json, "processing locator map"),
    warnings: parseStringArrayJson(row.warnings_json, "processing warnings"),
    createdAt: row.created_at,
    completedAt: row.completed_at || null,
  };
}

function toBlock(row: any): KnowledgeBlock | null {
  if (!row) return null;
  return {
    id: row.id,
    parseArtifactId: row.parse_artifact_id,
    ordinal: Number(row.ordinal),
    text: row.text,
    textSha256: row.text_sha256,
    locatorType: row.locator_type,
    locator: parseObjectJson(row.locator_payload_json, "block locator"),
  };
}

function toCitation(row: any): KnowledgeCitation | null {
  if (!row) return null;
  return {
    id: row.id,
    parseArtifactId: row.parse_artifact_id,
    blockId: row.block_id,
    startOffset: Number(row.start_offset),
    endOffset: Number(row.end_offset),
    canonicalText: row.canonical_text,
    canonicalTextSha256: row.canonical_text_sha256,
    createdAt: row.created_at,
  };
}

function toTurnScopeSource(row: any): KnowledgeTurnScopeSource {
  return {
    scopeId: row.scope_id,
    sourceId: row.source_id,
    contentSnapshotId: row.content_snapshot_id,
    parseArtifactId: row.parse_artifact_id || null,
    notebookIds: parseStringArrayJson(row.notebook_ids_json, "turn scope source notebooks"),
  };
}

function toTurnScope(row: any, sources: KnowledgeTurnScopeSource[]): KnowledgeTurnScope {
  return {
    id: row.id,
    turnId: row.turn_id,
    sessionPath: row.session_path,
    studioId: row.studio_id,
    notebookIds: parseStringArrayJson(row.notebook_ids_json, "turn scope notebooks"),
    status: row.status,
    createdAt: row.created_at,
    sources,
  };
}

function toCoveragePlanRecord(row: any): KnowledgeCoveragePlanRecord | null {
  if (!row) return null;
  return {
    id: row.id,
    turnScopeId: row.turn_scope_id || null,
    question: row.question,
    intent: row.intent,
    coverageMode: row.coverage_mode,
    requiresCompleteness: Number(row.requires_completeness) === 1,
    scopeLevel: row.scope_level,
    subQueries: parseStringArrayJson(row.sub_queries_json, "coverage plan sub-queries"),
    confidence: Number(row.confidence),
    matchedRuleIds: parseStringArrayJson(row.matched_rule_ids_json, "coverage plan matched rules"),
    classifierUsed: row.classifier_used,
    degradeReason: row.degrade_reason || null,
    createdAt: row.created_at,
  };
}

/** block spans 的持久化形状：[{chunkId, spans:[{blockId, 偏移四元组}]}]（身份定位元数据）。 */
function parseBlockSpansJson(value: unknown, field: string): KnowledgeEvidenceManifestEntry["blockSpans"] {
  let parsed: any;
  try {
    parsed = JSON.parse(typeof value === "string" ? value : "");
  } catch {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
  if (!Array.isArray(parsed)) {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
  return parsed.map((span: any) => {
    if (
      !span || typeof span !== "object"
      || typeof span.chunkId !== "string"
      || !Array.isArray(span.spans)
      || span.spans.some((entry: any) => (
        !entry || typeof entry !== "object"
        || typeof entry.blockId !== "string"
        || !Number.isSafeInteger(entry.blockStartOffset)
        || !Number.isSafeInteger(entry.blockEndOffset)
        || !Number.isSafeInteger(entry.chunkStartOffset)
        || !Number.isSafeInteger(entry.chunkEndOffset)
      ))
    ) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
    }
    return { chunkId: span.chunkId, spans: span.spans };
  });
}

function toEvidenceManifestEntry(row: any): KnowledgeEvidenceManifestEntry {
  return {
    ordinal: Number(row.ordinal),
    sourceId: row.source_id,
    contentSnapshotId: row.content_snapshot_id,
    parseArtifactId: row.parse_artifact_id || null,
    chunkProfileHash: row.chunk_profile_hash || null,
    chunkIndexVariantId: row.chunk_index_variant_id || null,
    vectorIndexVariantIds: parseStringArrayJson(row.vector_index_variant_ids_json, "evidence manifest vector variants"),
    chunkIds: parseStringArrayJson(row.chunk_ids_json, "evidence manifest chunk ids"),
    neighborChunkIds: parseStringArrayJson(row.neighbor_chunk_ids_json, "evidence manifest neighbor chunk ids"),
    blockSpans: parseBlockSpansJson(row.block_spans_json, "evidence manifest block spans"),
    citationLabels: parseStringArrayJson(row.citation_labels_json, "evidence manifest citation labels"),
  };
}

function toEvidenceManifest(row: any, entries: KnowledgeEvidenceManifestEntry[]): KnowledgeEvidenceManifest | null {
  if (!row) return null;
  return {
    id: row.id,
    turnScopeId: row.turn_scope_id,
    sessionPath: row.session_path,
    turnId: row.turn_id,
    notebookIds: parseStringArrayJson(row.notebook_ids_json, "evidence manifest notebooks"),
    coverageMode: row.coverage_mode || null,
    executedCoverageMode: row.executed_coverage_mode || null,
    coverageRunId: row.coverage_run_id || null,
    coverageManifestHash: row.coverage_manifest_hash || null,
    createdAt: row.created_at,
    entries,
  };
}

/** 与 shared/model-ref.ts 的持久化纪律一致：完整 {id, provider}，不做按 id 降级。 */
function serializeModelRef(value: unknown, field: string): string {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be an object`);
  }
  const ref = value as Record<string, unknown>;
  const id = requiredString(ref.id, `${field}.id`, 256);
  const provider = requiredString(ref.provider, `${field}.provider`, 256);
  return JSON.stringify({ id, provider });
}

function parseModelRefJson(value: unknown, field: string): KnowledgeModelRef | null {
  if (value == null) return null;
  const parsed = parseObjectJson(value, field);
  if (
    typeof parsed.id !== "string" || !parsed.id
    || typeof parsed.provider !== "string" || !parsed.provider
  ) {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", `${field} is corrupt`);
  }
  return { id: parsed.id, provider: parsed.provider };
}

function optionalIntegerInRange(
  value: unknown,
  field: string,
  min: number,
  max: number,
): number | null {
  if (value == null) return null;
  if (!Number.isSafeInteger(value) || Number(value) < min || Number(value) > max) {
    throw new KnowledgeError(
      "KNOWLEDGE_INVALID_ARGUMENT",
      `${field} must be an integer between ${min} and ${max}`,
      { field, min, max },
    );
  }
  return Number(value);
}

function chunkerConfigId(value: unknown): string {
  if (typeof value !== "string" || !/^[0-9a-f]{16}$/u.test(value)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "chunkerConfigId must be 16 lowercase hex characters");
  }
  return value;
}

function chunkProfileStrategy(value: unknown): KnowledgeChunkerStrategy {
  if (typeof value !== "string" || !CHUNK_PROFILE_STRATEGIES.includes(value as KnowledgeChunkerStrategy)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "chunk profile strategy is unsupported");
  }
  return value as KnowledgeChunkerStrategy;
}

function chunkTargetCharsSource(value: unknown): KnowledgeChunkTargetCharsSource {
  if (typeof value !== "string" || !CHUNK_TARGET_CHARS_SOURCES.has(value as KnowledgeChunkTargetCharsSource)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "targetCharsSource must be 'explicit' or 'auto'");
  }
  return value as KnowledgeChunkTargetCharsSource;
}

/**
 * RetrievalProfile 身份键：chunkProfileHash + 两个模型引用 + topK 的规范化 JSON
 * （对象字面量键序固定即规范化）的 sha256 前 16 hex。同配置跨笔记本同 key，
 * 从而共享同一份派生索引（任务书 §九）。
 */
export function knowledgeRetrievalProfileKey(input: {
  chunkProfileHash: string;
  embeddingModelRef: KnowledgeModelRef | null;
  rerankModelRef: KnowledgeModelRef | null;
  retrievalTopK: number | null;
}): string {
  const canonical = JSON.stringify({
    chunkProfileHash: input.chunkProfileHash,
    embeddingModelRef: input.embeddingModelRef
      ? { id: input.embeddingModelRef.id, provider: input.embeddingModelRef.provider }
      : null,
    rerankModelRef: input.rerankModelRef
      ? { id: input.rerankModelRef.id, provider: input.rerankModelRef.provider }
      : null,
    retrievalTopK: input.retrievalTopK ?? null,
  });
  return crypto.createHash("sha256").update(canonical, "utf8").digest("hex").slice(0, 16);
}

function isoTimestampOrNull(value: unknown, field: string): string | null {
  if (value == null) return null;
  if (typeof value !== "string" || Number.isNaN(Date.parse(value))) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", `${field} must be an ISO timestamp`);
  }
  return value;
}

function toNotebookConfig(row: any): NotebookConfig {
  return {
    embeddingModelRef: parseModelRefJson(row?.embedding_model_ref, "embedding model ref"),
    rerankModelRef: parseModelRefJson(row?.rerank_model_ref, "rerank model ref"),
    chunkTargetChars: row?.chunk_target_chars == null ? null : Number(row.chunk_target_chars),
    retrievalTopK: row?.retrieval_top_k == null ? null : Number(row.retrieval_top_k),
    vectorRetentionDays: row?.vector_retention_days == null ? null : Number(row.vector_retention_days),
  };
}

function toIngestionJob(row: any): IngestionJob | null {
  if (!row) return null;
  if (!INGESTION_PHASES.has(row.phase) || !INGESTION_STATUSES.has(row.status)) {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Ingestion job state is invalid");
  }
  return {
    id: row.id,
    notebookId: row.notebook_id,
    sourceId: row.source_id,
    artifactId: row.artifact_id || null,
    phase: row.phase,
    status: row.status,
    attempt: Number(row.attempt),
    retryAfter: row.retry_after || null,
    error: row.error || null,
    chunkerConfigId: row.chunker_config_id,
    cancelledAt: row.cancelled_at || null,
    progressDone: Number(row.progress_done ?? 0),
    progressTotal: row.progress_total == null ? null : Number(row.progress_total),
    embeddingStats: parseEmbeddingStats(row.embedding_stats),
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

/**
 * ingestion_jobs.embedding_stats（v10 JSON 列）解析：NULL → null；坏 JSON 或形状
 * 不符按事实库损坏显式抛错（禁静默丢诊断）。
 */
function parseEmbeddingStats(raw: unknown): KnowledgeIngestionEmbeddingStats | null {
  if (raw == null) return null;
  if (typeof raw !== "string") {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Ingestion job embedding stats are invalid");
  }
  let parsed: any;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Ingestion job embedding stats are invalid");
  }
  const counts = [parsed?.chunksNewlyEmbedded, parsed?.chunksResumedFromCheckpoint,
    parsed?.chunksReusedFromReadyVariant, parsed?.requestCount];
  if (
    !parsed || typeof parsed !== "object"
    || counts.some(value => !Number.isSafeInteger(value) || value < 0)
    || typeof parsed.resetStaleVectors !== "boolean"
    || (parsed.abandonedStaleVariantId !== null && typeof parsed.abandonedStaleVariantId !== "string")
    || (parsed.model !== null && (
      typeof parsed.model?.key !== "string"
      || typeof parsed.model?.provider !== "string"
      || typeof parsed.model?.modelId !== "string"
      || typeof parsed.model?.protocol !== "string"
      || !Number.isSafeInteger(parsed.model?.dimensions)
    ))
  ) {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Ingestion job embedding stats are invalid");
  }
  return parsed as KnowledgeIngestionEmbeddingStats;
}

function toChunkProfile(row: any): KnowledgeChunkProfile | null {
  if (!row) return null;
  if (!CHUNK_PROFILE_TYPES.has(row.profile_type)) {
    throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Chunk profile state is invalid");
  }
  return {
    id: row.id,
    profileHash: row.profile_hash,
    strategy: row.strategy == null ? null : chunkProfileStrategy(row.strategy),
    targetChars: row.target_chars == null ? null : Number(row.target_chars),
    targetCharsSource: row.target_chars_source == null ? null : chunkTargetCharsSource(row.target_chars_source),
    chunkerVersion: row.chunker_version ?? null,
    structuralOptions: row.structural_options_json == null
      ? null
      : parseObjectJson(row.structural_options_json, "chunk profile structural options"),
    profileType: row.profile_type,
    createdAt: row.created_at,
  };
}

function toRetrievalProfile(row: any): KnowledgeRetrievalProfile | null {
  if (!row) return null;
  return {
    id: row.id,
    profileKey: row.profile_key,
    chunkProfileId: row.chunk_profile_id,
    embeddingModelRef: parseModelRefJson(row.embedding_model_ref, "retrieval profile embedding model ref"),
    rerankModelRef: parseModelRefJson(row.rerank_model_ref, "retrieval profile rerank model ref"),
    retrievalTopK: row.retrieval_top_k == null ? null : Number(row.retrieval_top_k),
    createdAt: row.created_at,
  };
}

export interface ResolvedNotebookConfig {
  embeddingModelRef: KnowledgeModelRef | null;
  rerankModelRef: KnowledgeModelRef | null;
  /** null = 自动（按嵌入模型上下文 ×80% 派生，摄入时计算）；遗留显式值仍生效。 */
  chunkTargetChars: number | null;
  /** null = 无上限（返回全部匹配块）；正整数 = 最大召回数。 */
  retrievalTopK: number | null;
  /** null = 永久保留（默认）；正整数 = 旧版本向量 N 天未被查询命中即回收。 */
  vectorRetentionDays: number | null;
}

/**
 * 笔记本配置解析（v8 起）：仅笔记本列，无全局偏好级。模型引用未配置返回
 * null：摄入落 pending_embedding，查询走纯 FTS 并显式标注 retrievalMode="fts"
 * （禁静默降级，不偷换其他模型）。数值项 NULL = 自动/无上限语义。
 */
export function resolveNotebookConfig(config: NotebookConfig): ResolvedNotebookConfig {
  return {
    embeddingModelRef: config.embeddingModelRef ?? null,
    rerankModelRef: config.rerankModelRef ?? null,
    chunkTargetChars: config.chunkTargetChars ?? null,
    retrievalTopK: config.retrievalTopK ?? null,
    vectorRetentionDays: config.vectorRetentionDays ?? null,
  };
}

/**
 * 生效分块尺寸：显式列 > 嵌入模型上下文 ×80% 自动值（computeAutoChunkTargetChars）。
 * 摄入侧与查询侧懒构建必须经同一函数解析——两侧各自解析曾是查询侧按默认 1200
 * 重建索引、与摄入侧指纹互相打架的根因（configId 进 chunk id，尺寸不同即全量重建+重嵌）。
 */
export function resolveEffectiveChunkTargetChars(
  resolved: Pick<ResolvedNotebookConfig, "chunkTargetChars" | "embeddingModelRef">,
  getEmbeddingModelContextWindow?: ((modelRef: KnowledgeModelRef) => number | null) | null,
): number {
  return resolved.chunkTargetChars
    ?? computeAutoChunkTargetChars(
      resolved.embeddingModelRef
        ? getEmbeddingModelContextWindow?.(resolved.embeddingModelRef) ?? null
        : null,
    );
}

export interface KnowledgeStoreOptions {
  dbPath: string;
  Database?: any;
  now?: () => string;
  idGenerator?: (prefix: string) => string;
  /**
   * 查嵌入模型上下文窗口（token 数）；自动分块尺寸的解析依赖它
   * （resolveEffectiveChunkTargetChars）。未接线时按内置兜底窗口解析——
   * 与摄入/查询侧同一解析函数、同一兜底语义，不是静默降级。
   */
  getEmbeddingModelContextWindow?: ((modelRef: KnowledgeModelRef) => number | null) | null;
}

/** Knowledge 领域事实库；索引和大文件字节不写入这里。 */
export class KnowledgeStore {
  declare db: any;
  private readonly now: () => string;
  private readonly idGenerator: (prefix: string) => string;
  private readonly getEmbeddingModelContextWindow: ((modelRef: KnowledgeModelRef) => number | null) | null;

  constructor(options: KnowledgeStoreOptions) {
    if (!options?.dbPath || !path.isAbsolute(options.dbPath)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "KnowledgeStore requires an absolute dbPath");
    }
    fs.mkdirSync(path.dirname(options.dbPath), { recursive: true, mode: 0o700 });
    const Database = options.Database || loadDatabase();
    this.db = new Database(options.dbPath);
    this.now = options.now || (() => new Date().toISOString());
    this.idGenerator = options.idGenerator || ((prefix) => `${prefix}_${crypto.randomUUID()}`);
    this.getEmbeddingModelContextWindow = options.getEmbeddingModelContextWindow ?? null;

    try {
      this.db.pragma("journal_mode = WAL");
      this.db.pragma("synchronous = NORMAL");
      this.db.pragma("foreign_keys = ON");
      this.db.pragma("busy_timeout = 5000");
      this.migrate();
    } catch (error) {
      try {
        this.db.close();
      } catch {
        // 保留原始建库错误。
      }
      this.db = null;
      throw error;
    }
  }

  private migrate() {
    const current = Number(this.db.pragma("user_version", { simple: true }));
    if (!Number.isSafeInteger(current) || current < 0) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Knowledge database schema version is invalid");
    }
    if (current > KNOWLEDGE_SCHEMA_VERSION) {
      throw new KnowledgeError(
        "KNOWLEDGE_SCHEMA_NEWER",
        "Knowledge database uses a newer schema",
        { supportedVersion: KNOWLEDGE_SCHEMA_VERSION },
      );
    }
    if (current === KNOWLEDGE_SCHEMA_VERSION) return;

    this.db.transaction(() => {
      let version = current;
      while (version < KNOWLEDGE_SCHEMA_VERSION) {
        if (version === 0) this.createSchemaV1();
        if (version === 1) this.createSchemaV2();
        if (version === 2) this.createSchemaV3();
        if (version === 3) this.createSchemaV4();
        if (version === 4) this.createSchemaV5();
        if (version === 5) this.createSchemaV6();
        if (version === 6) this.createSchemaV7();
        if (version === 7) this.createSchemaV8();
        if (version === 8) this.createSchemaV9();
        if (version === 9) this.createSchemaV10();
        if (version === 10) this.createSchemaV11();
        if (version === 11) this.createSchemaV12();
        if (version === 12) this.createSchemaV13();
        if (version === 13) this.createSchemaV14();
        if (version === 14) this.createSchemaV15();
        if (version === 15) this.createSchemaV16();
        if (version === 16) this.createSchemaV17();
        if (version === 17) this.createSchemaV18();
        if (version === 18) this.createSchemaV19();
        // main 线（PR #30）曾独立把版本推进到自己的 v9（只加 vector_retention_days，
        // 无 chunk_profiles）：这类库进合并链会跳过 version===8 的 v9 步。v9 体幂等
        // （IF NOT EXISTS + 列存在检查），缺表时补跑一次即可对齐。
        if (
          version >= 9
          && (this.db.prepare("PRAGMA table_info(chunk_profiles)").all() as unknown[]).length === 0
        ) {
          this.createSchemaV9();
        }
        version += 1;
      }
      this.db.pragma(`user_version = ${KNOWLEDGE_SCHEMA_VERSION}`);
    })();
  }

  private createSchemaV1() {
    this.db.exec(`
      CREATE TABLE notebooks (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        name TEXT NOT NULL CHECK(length(trim(name)) > 0),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        deleted_at TEXT
      );

      CREATE INDEX idx_notebooks_studio_active
        ON notebooks(studio_id, deleted_at, updated_at DESC);

      CREATE TABLE sources (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        source_type TEXT NOT NULL CHECK(source_type IN ('file', 'pasted_text', 'web_snapshot')),
        display_name TEXT NOT NULL CHECK(length(trim(display_name)) > 0),
        origin_metadata_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        deleted_at TEXT
      );

      CREATE INDEX idx_sources_studio_active
        ON sources(studio_id, deleted_at, created_at DESC);

      CREATE TABLE notebook_sources (
        notebook_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        added_at TEXT NOT NULL,
        removed_at TEXT,
        PRIMARY KEY(notebook_id, source_id),
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_notebook_sources_active
        ON notebook_sources(notebook_id, removed_at, added_at);
      CREATE INDEX idx_source_notebooks_active
        ON notebook_sources(source_id, removed_at, added_at);

      CREATE TABLE content_snapshots (
        id TEXT PRIMARY KEY,
        source_id TEXT NOT NULL,
        sha256 TEXT NOT NULL CHECK(length(sha256) = 64),
        mime_type TEXT NOT NULL,
        byte_size INTEGER NOT NULL CHECK(byte_size >= 0),
        storage_path TEXT NOT NULL UNIQUE,
        captured_at TEXT NOT NULL,
        UNIQUE(source_id, sha256),
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_content_snapshots_source
        ON content_snapshots(source_id, captured_at DESC);
    `);
  }

  private createSchemaV2() {
    this.db.exec(`
      CREATE TABLE parse_artifacts (
        id TEXT PRIMARY KEY,
        content_snapshot_id TEXT NOT NULL,
        parser_id TEXT NOT NULL,
        parser_version TEXT NOT NULL,
        parser_config_hash TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('parsing', 'ready', 'needs_ocr', 'failed')),
        warnings_json TEXT NOT NULL,
        semantic_artifact_path TEXT,
        created_at TEXT NOT NULL,
        completed_at TEXT,
        UNIQUE(content_snapshot_id, parser_id, parser_version, parser_config_hash),
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_parse_artifacts_snapshot
        ON parse_artifacts(content_snapshot_id, created_at DESC);
      CREATE INDEX idx_parse_artifacts_status
        ON parse_artifacts(status, created_at);

      CREATE TABLE knowledge_blocks (
        id TEXT PRIMARY KEY,
        parse_artifact_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        text TEXT NOT NULL,
        text_sha256 TEXT NOT NULL CHECK(length(text_sha256) = 64),
        locator_type TEXT NOT NULL CHECK(locator_type IN ('text', 'markdown', 'pdf', 'html')),
        locator_payload_json TEXT NOT NULL,
        UNIQUE(parse_artifact_id, ordinal),
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_knowledge_blocks_artifact
        ON knowledge_blocks(parse_artifact_id, ordinal);

      CREATE TABLE knowledge_citations (
        id TEXT PRIMARY KEY,
        parse_artifact_id TEXT NOT NULL,
        block_id TEXT NOT NULL,
        start_offset INTEGER NOT NULL CHECK(start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(end_offset > start_offset),
        canonical_text TEXT NOT NULL,
        canonical_text_sha256 TEXT NOT NULL CHECK(length(canonical_text_sha256) = 64),
        created_at TEXT NOT NULL,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_knowledge_citations_artifact
        ON knowledge_citations(parse_artifact_id, created_at);
      CREATE INDEX idx_knowledge_citations_block
        ON knowledge_citations(block_id, start_offset, end_offset);
    `);
  }

  private createSchemaV3() {
    this.db.exec(`
      CREATE TABLE scope_snapshots (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        mode TEXT NOT NULL CHECK(mode IN ('quick', 'research')),
        created_at TEXT NOT NULL
      );

      CREATE INDEX idx_scope_snapshots_studio
        ON scope_snapshots(studio_id, created_at DESC);

      CREATE TABLE scope_notebooks (
        scope_snapshot_id TEXT NOT NULL,
        notebook_id TEXT NOT NULL,
        notebook_name TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        PRIMARY KEY(scope_snapshot_id, notebook_id),
        UNIQUE(scope_snapshot_id, ordinal),
        FOREIGN KEY(scope_snapshot_id) REFERENCES scope_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT
      );

      CREATE TABLE scope_sources (
        scope_snapshot_id TEXT NOT NULL,
        notebook_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        source_display_name TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        PRIMARY KEY(scope_snapshot_id, notebook_id, source_id),
        UNIQUE(scope_snapshot_id, ordinal),
        FOREIGN KEY(scope_snapshot_id) REFERENCES scope_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_scope_sources_artifact
        ON scope_sources(scope_snapshot_id, parse_artifact_id);
      CREATE INDEX idx_scope_sources_snapshot
        ON scope_sources(content_snapshot_id);

      CREATE TABLE knowledge_runs (
        id TEXT PRIMARY KEY,
        studio_id TEXT NOT NULL,
        mode TEXT NOT NULL CHECK(mode IN ('quick', 'research')),
        question TEXT NOT NULL,
        scope_snapshot_id TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'cancelled')),
        retrieval_mode TEXT NOT NULL CHECK(retrieval_mode IN ('fts', 'hybrid')),
        answer_text TEXT,
        error_code TEXT,
        created_at TEXT NOT NULL,
        completed_at TEXT,
        FOREIGN KEY(scope_snapshot_id) REFERENCES scope_snapshots(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_knowledge_runs_studio
        ON knowledge_runs(studio_id, created_at DESC);
      CREATE INDEX idx_knowledge_runs_status
        ON knowledge_runs(status, created_at);

      CREATE TABLE knowledge_run_retrievals (
        run_id TEXT NOT NULL,
        rank INTEGER NOT NULL CHECK(rank > 0),
        chunk_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        score REAL NOT NULL,
        PRIMARY KEY(run_id, rank),
        FOREIGN KEY(run_id) REFERENCES knowledge_runs(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE TABLE knowledge_run_citations (
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        marker INTEGER NOT NULL CHECK(marker > 0),
        citation_id TEXT NOT NULL,
        candidate_ref TEXT NOT NULL,
        PRIMARY KEY(run_id, ordinal),
        UNIQUE(run_id, marker),
        FOREIGN KEY(run_id) REFERENCES knowledge_runs(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_knowledge_run_citations_citation
        ON knowledge_run_citations(citation_id);
    `);
  }

  private createSchemaV4() {
    this.db.exec(`
      CREATE TABLE research_runs (
        run_id TEXT PRIMARY KEY,
        host_task_id TEXT NOT NULL UNIQUE,
        state TEXT NOT NULL CHECK(state IN (
          'queued', 'preparing_scope', 'building_manifest', 'scanning',
          'building_claims', 'checking_contradictions', 'synthesizing',
          'completed', 'recovering', 'partial', 'failed', 'canceled'
        )),
        spec_json TEXT NOT NULL,
        error_code TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        completed_at TEXT,
        FOREIGN KEY(run_id) REFERENCES knowledge_runs(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_runs_state
        ON research_runs(state, updated_at);

      CREATE TABLE analysis_manifests (
        run_id TEXT PRIMARY KEY,
        source_count INTEGER NOT NULL CHECK(source_count >= 0),
        parse_artifact_count INTEGER NOT NULL CHECK(parse_artifact_count >= 0),
        block_count INTEGER NOT NULL CHECK(block_count >= 0),
        unit_count INTEGER NOT NULL CHECK(unit_count > 0),
        primary_char_count INTEGER NOT NULL CHECK(primary_char_count > 0),
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE analysis_units (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        priority INTEGER NOT NULL CHECK(priority >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        primary_char_count INTEGER NOT NULL CHECK(primary_char_count > 0),
        context_char_count INTEGER NOT NULL CHECK(context_char_count >= 0),
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, parse_artifact_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_analysis_units_run_status
        ON analysis_units(run_id, status, priority, ordinal);

      CREATE TABLE analysis_unit_spans (
        unit_id TEXT NOT NULL,
        kind TEXT NOT NULL CHECK(kind IN ('primary', 'context')),
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        block_id TEXT NOT NULL,
        block_ordinal INTEGER NOT NULL CHECK(block_ordinal >= 0),
        start_offset INTEGER NOT NULL CHECK(start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(end_offset > start_offset),
        PRIMARY KEY(unit_id, kind, ordinal),
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_analysis_unit_spans_block
        ON analysis_unit_spans(block_id, kind, start_offset, end_offset);

      CREATE TABLE execution_batches (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        estimated_chars INTEGER NOT NULL CHECK(estimated_chars > 0),
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_execution_batches_run_status
        ON execution_batches(run_id, status, ordinal);

      CREATE TABLE execution_batch_units (
        batch_id TEXT NOT NULL,
        unit_id TEXT NOT NULL UNIQUE,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        PRIMARY KEY(batch_id, ordinal),
        FOREIGN KEY(batch_id) REFERENCES execution_batches(id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT
      );

      CREATE TABLE research_jobs (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        phase TEXT NOT NULL CHECK(phase IN ('claim_build', 'final_synthesis')),
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        input_refs_json TEXT NOT NULL,
        output_json TEXT,
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, phase, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE task_attempts (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        work_type TEXT NOT NULL CHECK(work_type IN ('scan_batch', 'claim_job', 'contradiction_check', 'synthesis_job')),
        work_id TEXT NOT NULL,
        attempt_number INTEGER NOT NULL CHECK(attempt_number > 0),
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'canceled')),
        error_code TEXT,
        output_json TEXT,
        started_at TEXT NOT NULL,
        completed_at TEXT,
        UNIQUE(work_type, work_id, attempt_number),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_task_attempts_run
        ON task_attempts(run_id, work_type, started_at);

      CREATE TABLE analysis_unit_results (
        unit_id TEXT PRIMARY KEY,
        attempt_id TEXT NOT NULL,
        result_json TEXT NOT NULL,
        completed_at TEXT NOT NULL,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(attempt_id) REFERENCES task_attempts(id) ON DELETE RESTRICT
      );

      CREATE TABLE evidence_validations (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        origin_type TEXT NOT NULL CHECK(origin_type IN ('analysis', 'contradiction')),
        origin_id TEXT NOT NULL,
        candidate_ordinal INTEGER NOT NULL CHECK(candidate_ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('validated', 'invalid')),
        reason_code TEXT,
        citation_id TEXT,
        canonical_quote TEXT,
        quote_checksum TEXT,
        epistemic_basis TEXT NOT NULL CHECK(epistemic_basis IN ('explicit', 'inferred', 'mixed')),
        created_at TEXT NOT NULL,
        UNIQUE(origin_type, origin_id, candidate_ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_evidence_validations_run
        ON evidence_validations(run_id, status, created_at);

      CREATE TABLE research_evidence (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        validation_id TEXT NOT NULL UNIQUE,
        citation_id TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        block_id TEXT NOT NULL,
        start_offset INTEGER NOT NULL CHECK(start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(end_offset > start_offset),
        canonical_quote TEXT NOT NULL,
        quote_checksum TEXT NOT NULL CHECK(length(quote_checksum) = 64),
        epistemic_basis TEXT NOT NULL CHECK(epistemic_basis IN ('explicit', 'inferred', 'mixed')),
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(validation_id) REFERENCES evidence_validations(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_evidence_run
        ON research_evidence(run_id, created_at);

      CREATE TABLE research_claims (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        origin_job_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        text TEXT NOT NULL,
        support_status TEXT NOT NULL CHECK(support_status IN ('supported', 'partial', 'disputed', 'insufficient')),
        epistemic_basis TEXT NOT NULL CHECK(epistemic_basis IN ('explicit', 'inferred', 'mixed')),
        created_at TEXT NOT NULL,
        UNIQUE(origin_job_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(origin_job_id) REFERENCES research_jobs(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_claims_run
        ON research_claims(run_id, ordinal);

      CREATE TABLE claim_evidence (
        claim_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        relation TEXT NOT NULL CHECK(relation IN ('supports', 'contradicts', 'context')),
        PRIMARY KEY(claim_id, evidence_id, relation),
        FOREIGN KEY(claim_id) REFERENCES research_claims(id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT
      );

      CREATE TABLE claim_packs (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        claim_ids_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE contradiction_manifests (
        run_id TEXT PRIMARY KEY,
        unit_count INTEGER NOT NULL CHECK(unit_count >= 0),
        claim_pack_count INTEGER NOT NULL CHECK(claim_pack_count >= 0),
        total_check_count INTEGER NOT NULL CHECK(total_check_count >= 0),
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT
      );

      CREATE TABLE contradiction_checks (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        claim_pack_id TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        attempt_id TEXT,
        result_json TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, unit_id, claim_pack_id),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT,
        FOREIGN KEY(claim_pack_id) REFERENCES claim_packs(id) ON DELETE RESTRICT,
        FOREIGN KEY(attempt_id) REFERENCES task_attempts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_contradiction_checks_run_status
        ON contradiction_checks(run_id, status, unit_id);

      CREATE TABLE research_contradictions (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        check_id TEXT NOT NULL,
        claim_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        relation TEXT NOT NULL CHECK(relation IN ('contradicts', 'context')),
        explanation TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(check_id, claim_id, evidence_id),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(check_id) REFERENCES contradiction_checks(id) ON DELETE RESTRICT,
        FOREIGN KEY(claim_id) REFERENCES research_claims(id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT
      );

      CREATE TABLE research_reports (
        run_id TEXT PRIMARY KEY,
        synthesis_job_id TEXT NOT NULL UNIQUE,
        report_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(synthesis_job_id) REFERENCES research_jobs(id) ON DELETE RESTRICT
      );

      CREATE TABLE research_report_citations (
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        marker INTEGER NOT NULL CHECK(marker > 0),
        evidence_id TEXT NOT NULL,
        citation_id TEXT NOT NULL,
        PRIMARY KEY(run_id, ordinal),
        UNIQUE(run_id, marker),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT,
        FOREIGN KEY(citation_id) REFERENCES knowledge_citations(id) ON DELETE RESTRICT
      );
    `);
  }

  private createSchemaV5() {
    this.db.exec(`
      CREATE TABLE research_verification_steps (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        trigger_synthesis_job_id TEXT NOT NULL UNIQUE,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        requests_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(trigger_synthesis_job_id) REFERENCES research_jobs(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_steps_run
        ON research_verification_steps(run_id, status, ordinal);

      CREATE TABLE research_verification_cells (
        id TEXT PRIMARY KEY,
        step_id TEXT NOT NULL,
        unit_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'canceled')),
        result_json TEXT,
        created_at TEXT NOT NULL,
        started_at TEXT,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(step_id, unit_id),
        UNIQUE(step_id, ordinal),
        FOREIGN KEY(step_id) REFERENCES research_verification_steps(id) ON DELETE RESTRICT,
        FOREIGN KEY(unit_id) REFERENCES analysis_units(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_cells_step
        ON research_verification_cells(step_id, status, ordinal);

      CREATE TABLE research_verification_attempts (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        step_id TEXT NOT NULL,
        cell_id TEXT NOT NULL,
        attempt_number INTEGER NOT NULL CHECK(attempt_number > 0),
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'canceled')),
        error_code TEXT,
        output_json TEXT,
        started_at TEXT NOT NULL,
        completed_at TEXT,
        UNIQUE(cell_id, attempt_number),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(step_id) REFERENCES research_verification_steps(id) ON DELETE RESTRICT,
        FOREIGN KEY(cell_id) REFERENCES research_verification_cells(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_attempts_run
        ON research_verification_attempts(run_id, started_at);

      CREATE TABLE research_verification_relations (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        step_id TEXT NOT NULL,
        cell_id TEXT NOT NULL,
        claim_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        relation TEXT NOT NULL CHECK(relation IN ('supports', 'contradicts', 'context')),
        explanation TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(cell_id, claim_id, evidence_id, relation),
        FOREIGN KEY(run_id) REFERENCES research_runs(run_id) ON DELETE RESTRICT,
        FOREIGN KEY(step_id) REFERENCES research_verification_steps(id) ON DELETE RESTRICT,
        FOREIGN KEY(cell_id) REFERENCES research_verification_cells(id) ON DELETE RESTRICT,
        FOREIGN KEY(claim_id) REFERENCES research_claims(id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES research_evidence(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_research_verification_relations_run
        ON research_verification_relations(run_id, relation, created_at);
    `);
  }

  private createSchemaV6() {
    // 笔记本级配置：模型引用存 {id, provider} JSON，NULL = 继承全局偏好；
    // 数值列带默认值（旧行回填 1200/12），显式置 NULL = 回退内置默认。
    this.db.exec(`
      ALTER TABLE notebooks ADD COLUMN embedding_model_ref TEXT;
      ALTER TABLE notebooks ADD COLUMN rerank_model_ref TEXT;
      ALTER TABLE notebooks ADD COLUMN chunk_target_chars INTEGER DEFAULT 1200
        CHECK(chunk_target_chars IS NULL OR (chunk_target_chars >= 100 AND chunk_target_chars <= 100000));
      ALTER TABLE notebooks ADD COLUMN retrieval_top_k INTEGER DEFAULT 12
        CHECK(retrieval_top_k IS NULL OR (retrieval_top_k >= 1 AND retrieval_top_k <= 1000));

      CREATE TABLE ingestion_jobs (
        id TEXT PRIMARY KEY,
        notebook_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        artifact_id TEXT,
        phase TEXT NOT NULL CHECK(phase IN ('parse', 'chunk', 'fts_index', 'embed', 'done')),
        status TEXT NOT NULL CHECK(status IN ('queued', 'running', 'pending_embedding', 'failed', 'done')),
        attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt >= 0),
        retry_after TEXT,
        error TEXT,
        chunker_config_id TEXT NOT NULL CHECK(length(chunker_config_id) = 16),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY(notebook_id) REFERENCES notebooks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX idx_ingestion_jobs_status
        ON ingestion_jobs(status, retry_after, created_at);
      CREATE INDEX idx_ingestion_jobs_source
        ON ingestion_jobs(source_id, created_at DESC);
      CREATE INDEX idx_ingestion_jobs_notebook
        ON ingestion_jobs(notebook_id, created_at DESC);
    `);

    // 提问功能已删（Phase 1），V3-V5 研究表零读路径，全部是从 V1-V2 核心表
    // 派生的产物，与 v6 新列同事务 DROP。按 子表 → 父表 顺序，避免 foreign_keys
    // 开启时 DROP 父表的隐式 DELETE 触发子行 FK 检查；IF EXISTS 容忍残缺的旧库。
    this.db.exec(`
      DROP TABLE IF EXISTS research_verification_relations;
      DROP TABLE IF EXISTS research_verification_attempts;
      DROP TABLE IF EXISTS research_verification_cells;
      DROP TABLE IF EXISTS research_verification_steps;
      DROP TABLE IF EXISTS research_report_citations;
      DROP TABLE IF EXISTS research_reports;
      DROP TABLE IF EXISTS research_contradictions;
      DROP TABLE IF EXISTS contradiction_checks;
      DROP TABLE IF EXISTS contradiction_manifests;
      DROP TABLE IF EXISTS claim_packs;
      DROP TABLE IF EXISTS claim_evidence;
      DROP TABLE IF EXISTS research_claims;
      DROP TABLE IF EXISTS research_evidence;
      DROP TABLE IF EXISTS evidence_validations;
      DROP TABLE IF EXISTS analysis_unit_results;
      DROP TABLE IF EXISTS task_attempts;
      DROP TABLE IF EXISTS research_jobs;
      DROP TABLE IF EXISTS execution_batch_units;
      DROP TABLE IF EXISTS execution_batches;
      DROP TABLE IF EXISTS analysis_unit_spans;
      DROP TABLE IF EXISTS analysis_units;
      DROP TABLE IF EXISTS analysis_manifests;
      DROP TABLE IF EXISTS research_runs;
      DROP TABLE IF EXISTS knowledge_run_citations;
      DROP TABLE IF EXISTS knowledge_run_retrievals;
      DROP TABLE IF EXISTS knowledge_runs;
      DROP TABLE IF EXISTS scope_sources;
      DROP TABLE IF EXISTS scope_notebooks;
      DROP TABLE IF EXISTS scope_snapshots;
    `);
  }

  private createSchemaV7() {
    // 嵌入进度列：done 从 0 递增（每批嵌入后由摄入 worker 落库）；
    // total NULL = 尚未进入 embed 相位（parse/chunk/fts_index 阶段无进度语义）。
    this.db.exec(`
      ALTER TABLE ingestion_jobs ADD COLUMN progress_done INTEGER NOT NULL DEFAULT 0;
      ALTER TABLE ingestion_jobs ADD COLUMN progress_total INTEGER;
    `);
  }

  private createSchemaV8() {
    // retrieval_top_k 语义反转：NULL = 无上限（新默认，返回全部匹配块）。
    // v6 的 ADD COLUMN … DEFAULT 12 把存量行回填成显式 12，与用户手填的 12
    // 不可区分——统一清 NULL，让所有笔记本从新的"无上限"默认起步；此前显式
    // 设置过召回数的笔记本同样回到无上限（符合"默认无上限"的需求方向，用户
    // 可在设置里重新指定最大召回数）。纯数据迁移，无 DDL 结构变更。
    this.db.exec(`
      UPDATE notebooks SET retrieval_top_k = NULL WHERE deleted_at IS NULL;
    `);
  }

  /**
   * v9（P0 索引身份，任务书 §六/§九/§七十六）：新增 chunk_profiles / retrieval_profiles
   * 两张身份注册表 + notebooks.retrieval_profile_id 绑定列。纯 additive：
   * 不 DROP、不改既有列、不触碰任何索引/向量数据，更不触发重新 embedding。
   * DDL 幂等（IF NOT EXISTS + 列存在性检查），容忍压版本重放的迁移测试与残缺库。
   * notebooks.retrieval_profile_id 按 SQLite ADD COLUMN 限制不加表级 FK，由应用层保证。
   */
  private createSchemaV9() {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS chunk_profiles (
        id TEXT PRIMARY KEY,
        profile_hash TEXT NOT NULL UNIQUE CHECK(length(profile_hash) = 16),
        strategy TEXT CHECK(strategy IS NULL OR strategy IN ('fixed', 'markdown', 'text', 'pdf', 'html')),
        target_chars INTEGER CHECK(target_chars IS NULL OR (target_chars >= 100 AND target_chars <= 100000)),
        target_chars_source TEXT CHECK(target_chars_source IS NULL OR target_chars_source IN ('explicit', 'auto')),
        chunker_version TEXT,
        structural_options_json TEXT,
        profile_type TEXT NOT NULL CHECK(profile_type IN ('standard', 'legacy')),
        created_at TEXT NOT NULL
      );

      CREATE TABLE IF NOT EXISTS retrieval_profiles (
        id TEXT PRIMARY KEY,
        profile_key TEXT NOT NULL UNIQUE CHECK(length(profile_key) = 16),
        chunk_profile_id TEXT NOT NULL,
        embedding_model_ref TEXT,
        rerank_model_ref TEXT,
        retrieval_top_k INTEGER CHECK(retrieval_top_k IS NULL OR (retrieval_top_k >= 1 AND retrieval_top_k <= 1000)),
        created_at TEXT NOT NULL,
        FOREIGN KEY(chunk_profile_id) REFERENCES chunk_profiles(id) ON DELETE RESTRICT
      );
    `);
    const notebookColumns = new Set<string>(
      this.db.pragma("table_info(notebooks)").map((column: any) => column.name),
    );
    if (!notebookColumns.has("retrieval_profile_id")) {
      this.db.exec(`ALTER TABLE notebooks ADD COLUMN retrieval_profile_id TEXT`);
    }
    this.backfillChunkProfilesFromIngestionHistory();
  }

  /**
   * v10（Phase 3 批级 checkpoint，任务书 §十四/§七十四）：ingestion_jobs 新增
   * embedding_stats（JSON TEXT，NULL = 尚未执行过 embed 相位）——embed 相位每次
   * 执行结束由 worker 写入 chunksNewlyEmbedded / chunksResumedFromCheckpoint /
   * chunksReusedFromReadyVariant / requestCount / 模型身份等成本观测，后端可查询。
   * 纯 additive：单列 ALTER，存量行自然为 NULL。
   */
  private createSchemaV10() {
    const columns = new Set<string>(
      this.db.pragma("table_info(ingestion_jobs)").map((column: any) => column.name),
    );
    if (!columns.has("embedding_stats")) {
      this.db.exec(`ALTER TABLE ingestion_jobs ADD COLUMN embedding_stats TEXT`);
    }
  }

  /**
   * v11（Phase 4 KnowledgeTurnScope，任务书 §二十/§四十三）：本轮知识权限天花板。
   * knowledge_turn_scopes 记一轮的选中笔记本集合；knowledge_turn_scope_sources
   * 冻结每源当时的最新 snapshot/artifact——本轮读取锚定冻结版本，watcher 产生的
   * 新版本下一轮才生效。closed 行保留（供 EvidenceManifest 追溯），统一 GC 留给
   * 生命周期 Phase 5（§十八），此处不做定期清理。纯 additive：两张新表。
   */
  private createSchemaV11() {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS knowledge_turn_scopes (
        id TEXT PRIMARY KEY,
        turn_id TEXT NOT NULL CHECK(length(trim(turn_id)) > 0),
        session_path TEXT NOT NULL CHECK(length(trim(session_path)) > 0),
        studio_id TEXT NOT NULL,
        notebook_ids_json TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('active', 'closed')),
        created_at TEXT NOT NULL
      );

      CREATE INDEX IF NOT EXISTS idx_knowledge_turn_scopes_session
        ON knowledge_turn_scopes(session_path, status, created_at DESC);

      CREATE TABLE IF NOT EXISTS knowledge_turn_scope_sources (
        scope_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT,
        notebook_ids_json TEXT NOT NULL,
        PRIMARY KEY(scope_id, source_id),
        FOREIGN KEY(scope_id) REFERENCES knowledge_turn_scopes(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );
    `);
  }

  /**
   * v12（Phase 5 生命周期治理，任务书 §十六–§十九）：纯 additive 两列 + 一个部分唯一索引。
   * - sources.orphaned_at：零活跃 membership 的 orphan 标记（保留期后 GC 物理清理；
   *   重新加入笔记本即清除）。deleted_at 死列自本版起承载显式删除语义（deleteSource 置位，
   *   activeSource 过滤使其立即对一切 ensure/enqueue 生效——delete wins）。
   * - ingestion_jobs.cancelled_at：显式取消留痕。status 列的 CHECK 约束不可 ALTER，
   *   取消退回 failed 终态 + 本列显式标注（requeueIngestionJob 拒绝 cancelled 行）。
   * - 部分唯一索引 (notebook_id, source_id) WHERE 活跃态：活跃 job 去重的 DB 级兜底
   *   （此前仅 SELECT-then-INSERT）。建索引前先收敛存量重复活跃行——每组保留最新一条
   *   （created_at, rowid 序），其余显式 failed 留痕，避免 UNIQUE 建立失败中断迁移。
   */
  private createSchemaV12() {
    const sourceColumns = new Set<string>(
      this.db.pragma("table_info(sources)").map((column: any) => column.name),
    );
    const jobColumns = new Set<string>(
      this.db.pragma("table_info(ingestion_jobs)").map((column: any) => column.name),
    );
    if (!sourceColumns.has("orphaned_at")) {
      this.db.exec(`ALTER TABLE sources ADD COLUMN orphaned_at TEXT`);
    }
    if (!jobColumns.has("cancelled_at")) {
      this.db.exec(`ALTER TABLE ingestion_jobs ADD COLUMN cancelled_at TEXT`);
    }
    this.db.exec(`
      CREATE INDEX IF NOT EXISTS idx_sources_orphaned
        ON sources(orphaned_at) WHERE orphaned_at IS NOT NULL;
    `);
    this.db.prepare(`
      UPDATE ingestion_jobs
      SET status = 'failed',
        error = 'KNOWLEDGE_CONFLICT: duplicate active job collapsed by schema v12 active unique index',
        updated_at = ?
      WHERE status IN ('queued', 'running', 'pending_embedding')
        AND EXISTS (
          SELECT 1 FROM ingestion_jobs newer
          WHERE newer.notebook_id = ingestion_jobs.notebook_id
            AND newer.source_id = ingestion_jobs.source_id
            AND newer.status IN ('queued', 'running', 'pending_embedding')
            AND (newer.created_at > ingestion_jobs.created_at
              OR (newer.created_at = ingestion_jobs.created_at AND newer.rowid > ingestion_jobs.rowid))
        )
    `).run(this.now());
    this.db.exec(`
      CREATE UNIQUE INDEX IF NOT EXISTS idx_ingestion_jobs_active
        ON ingestion_jobs(notebook_id, source_id)
        WHERE status IN ('queued', 'running', 'pending_embedding');
    `);
  }

  /**
   * v13（Phase 7 KnowledgeCoveragePlanner，任务书 §二十九）：覆盖计划持久化。
   * 只存结构化分类结果（intent/coverageMode/requiresCompleteness/scopeLevel/
   * subQueries/confidence/matchedRuleIds/classifierUsed/degrade_reason），不存任何
   * CoT 或原始模型输出。turn_scope_id 可空关联 knowledge_turn_scopes（非会话
   * 路径无 scope）。纯 additive：一张新表 + 两个查询索引。
   */
  private createSchemaV13() {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS knowledge_coverage_plans (
        id TEXT PRIMARY KEY,
        turn_scope_id TEXT,
        question TEXT NOT NULL CHECK(length(trim(question)) > 0),
        intent TEXT NOT NULL CHECK(intent IN ('fact_lookup', 'cross_source_synthesis', 'whole_scope_analysis', 'global_negative', 'open_summary')),
        coverage_mode TEXT NOT NULL CHECK(coverage_mode IN ('high_recall', 'broad', 'exhaustive')),
        requires_completeness INTEGER NOT NULL CHECK(requires_completeness IN (0, 1)),
        scope_level TEXT NOT NULL CHECK(scope_level IN ('local', 'source', 'multi_source', 'notebook', 'multi_notebook', 'whole_scope')),
        sub_queries_json TEXT NOT NULL,
        confidence REAL NOT NULL CHECK(confidence >= 0 AND confidence <= 1),
        matched_rule_ids_json TEXT NOT NULL,
        classifier_used TEXT NOT NULL CHECK(classifier_used IN ('rules', 'llm', 'rules+llm')),
        degrade_reason TEXT,
        created_at TEXT NOT NULL,
        FOREIGN KEY(turn_scope_id) REFERENCES knowledge_turn_scopes(id) ON DELETE RESTRICT
      );

      CREATE INDEX IF NOT EXISTS idx_knowledge_coverage_plans_scope
        ON knowledge_coverage_plans(turn_scope_id, created_at DESC);
      CREATE INDEX IF NOT EXISTS idx_knowledge_coverage_plans_created
        ON knowledge_coverage_plans(created_at DESC);
    `);
  }

  /**
   * v14（Phase 9 EXHAUSTIVE 覆盖执行，任务书 §六十五）：coverage_runs /
   * coverage_shards 两张执行事实表。run 行冻结 manifest 身份（manifest_hash 64 hex
   * + manifest_json 完整冻结结构，含 unit 文本——恢复时 worker 输入的唯一来源）；
   * shard 行持久化确定性分片（id 即 'cshard_'+hash(manifestHash+ordinal)，UNIQUE
   * (run_id, ordinal)）与 attempt_count / result_json。恢复语义在
   * loadResumableCoverageRun（completed 不重跑、pending 续跑、running 置回 pending）。
   * 纯 additive：两张新表 + 查询索引，不触碰任何既有表。
   */
  private createSchemaV14() {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS coverage_runs (
        id TEXT PRIMARY KEY,
        turn_scope_id TEXT NOT NULL,
        manifest_hash TEXT NOT NULL CHECK(length(manifest_hash) = 64),
        manifest_json TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'complete', 'partial', 'cancelled', 'failed')),
        expected_units INTEGER NOT NULL CHECK(expected_units >= 0),
        processed_units INTEGER NOT NULL DEFAULT 0 CHECK(processed_units >= 0),
        failed_units INTEGER NOT NULL DEFAULT 0 CHECK(failed_units >= 0),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY(turn_scope_id) REFERENCES knowledge_turn_scopes(id) ON DELETE RESTRICT
      );

      CREATE INDEX IF NOT EXISTS idx_coverage_runs_manifest
        ON coverage_runs(manifest_hash, created_at DESC);
      CREATE INDEX IF NOT EXISTS idx_coverage_runs_status
        ON coverage_runs(status, updated_at);

      CREATE TABLE IF NOT EXISTS coverage_shards (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        unit_ids_json TEXT NOT NULL,
        context_before_ids_json TEXT NOT NULL,
        context_after_ids_json TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed', 'cancelled')),
        attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count >= 0),
        result_json TEXT,
        updated_at TEXT NOT NULL,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES coverage_runs(id) ON DELETE RESTRICT
      );

      CREATE INDEX IF NOT EXISTS idx_coverage_shards_run_status
        ON coverage_shards(run_id, status, ordinal);
    `);
  }

  /**
   * v15（任务书 §六十七 EvidenceManifest 轻量持久化）：evidence_manifests /
   * evidence_manifest_entries 两张身份链表。manifest 头冻结轮级关联
   * （turn scope / session / turn / coverage 档位与 run 关联），entries 按
   * (source, chunkIndexVariant) 分组记录该轮实际读取的 snapshot/artifact/
   * profile/变体/chunk id/邻接块/block spans/引用标签——Source 后续更新后仍
   * 知旧回答基于哪个版本。只存身份与定位元数据，绝不存 chunk 正文、CoT 或
   * 任何模型输出。纯 additive：两张新表 + 查询/GC 索引，不触碰任何既有表。
   */
  private createSchemaV15() {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS evidence_manifests (
        id TEXT PRIMARY KEY,
        turn_scope_id TEXT NOT NULL,
        session_path TEXT NOT NULL CHECK(length(trim(session_path)) > 0),
        turn_id TEXT NOT NULL CHECK(length(trim(turn_id)) > 0),
        coverage_mode TEXT CHECK(coverage_mode IS NULL OR coverage_mode IN ('high_recall', 'broad', 'exhaustive')),
        executed_coverage_mode TEXT CHECK(executed_coverage_mode IS NULL OR executed_coverage_mode IN ('high_recall', 'broad', 'exhaustive')),
        notebook_ids_json TEXT NOT NULL,
        coverage_run_id TEXT,
        coverage_manifest_hash TEXT CHECK(coverage_manifest_hash IS NULL OR length(coverage_manifest_hash) = 64),
        created_at TEXT NOT NULL,
        FOREIGN KEY(turn_scope_id) REFERENCES knowledge_turn_scopes(id) ON DELETE RESTRICT,
        FOREIGN KEY(coverage_run_id) REFERENCES coverage_runs(id) ON DELETE RESTRICT
      );

      CREATE INDEX IF NOT EXISTS idx_evidence_manifests_scope
        ON evidence_manifests(turn_scope_id, created_at DESC);
      CREATE INDEX IF NOT EXISTS idx_evidence_manifests_turn
        ON evidence_manifests(turn_id, created_at DESC);
      CREATE INDEX IF NOT EXISTS idx_evidence_manifests_run
        ON evidence_manifests(coverage_run_id) WHERE coverage_run_id IS NOT NULL;

      CREATE TABLE IF NOT EXISTS evidence_manifest_entries (
        manifest_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(ordinal >= 0),
        source_id TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT,
        chunk_profile_hash TEXT,
        chunk_index_variant_id TEXT,
        vector_index_variant_ids_json TEXT NOT NULL,
        chunk_ids_json TEXT NOT NULL,
        neighbor_chunk_ids_json TEXT NOT NULL,
        block_spans_json TEXT NOT NULL,
        citation_labels_json TEXT NOT NULL,
        PRIMARY KEY(manifest_id, ordinal, source_id),
        FOREIGN KEY(manifest_id) REFERENCES evidence_manifests(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT
      );

      CREATE INDEX IF NOT EXISTS idx_evidence_manifest_entries_source
        ON evidence_manifest_entries(source_id);
    `);
  }

  /**
   * v16（任务书 §五十八/§五十九/§六十九 ProcessingArtifact 与目录组织路径）：
   * processing_artifacts 记录二进制格式（DOCX/XLSX/CSV）→ 结构化文本的持久化
   * 转换产物（processor 身份四元组唯一，locatorMap 记录输出行 → 原始定位的
   * 反向映射）；parse_artifacts 增加 fidelity（证据可信度等级，legacy 行由
   * DEFAULT 'citation_grade' 兜底）与 processing_artifact_id（来源转换产物）；
   * notebook_sources 增加 relative_path / folder_node / display_order（目录路径
   * 属于 Membership，同一 Source 在不同 Notebook 可有不同位置）。纯 additive。
   */
  private createSchemaV16() {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS processing_artifacts (
        id TEXT PRIMARY KEY,
        content_snapshot_id TEXT NOT NULL,
        processor_id TEXT NOT NULL,
        processor_version TEXT NOT NULL,
        processor_config_hash TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('processing', 'ready', 'failed')),
        fidelity TEXT CHECK(fidelity IN ('citation_grade', 'structural', 'semantic_only')),
        output_mime TEXT,
        output_path TEXT,
        locator_map_json TEXT NOT NULL DEFAULT '{}',
        warnings_json TEXT NOT NULL DEFAULT '[]',
        created_at TEXT NOT NULL,
        completed_at TEXT,
        UNIQUE(content_snapshot_id, processor_id, processor_version, processor_config_hash),
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT
      );

      CREATE INDEX IF NOT EXISTS idx_processing_artifacts_snapshot
        ON processing_artifacts(content_snapshot_id, created_at DESC);
    `);
    const parseArtifactColumns = new Set<string>(
      this.db.pragma("table_info(parse_artifacts)").map((column: any) => column.name),
    );
    if (!parseArtifactColumns.has("fidelity")) {
      this.db.exec(`ALTER TABLE parse_artifacts ADD COLUMN fidelity TEXT NOT NULL DEFAULT 'citation_grade'`);
    }
    if (!parseArtifactColumns.has("processing_artifact_id")) {
      this.db.exec(`ALTER TABLE parse_artifacts ADD COLUMN processing_artifact_id TEXT`);
    }
    const membershipColumns = new Set<string>(
      this.db.pragma("table_info(notebook_sources)").map((column: any) => column.name),
    );
    if (!membershipColumns.has("relative_path")) {
      this.db.exec(`ALTER TABLE notebook_sources ADD COLUMN relative_path TEXT`);
    }
    if (!membershipColumns.has("folder_node")) {
      this.db.exec(`ALTER TABLE notebook_sources ADD COLUMN folder_node TEXT`);
    }
    if (!membershipColumns.has("display_order")) {
      this.db.exec(`ALTER TABLE notebook_sources ADD COLUMN display_order INTEGER`);
    }
  }

  /**
   * §76 旧数据映射：以 ingestion_jobs.chunker_config_id 为证据反推 chunk_profiles 行。
   * 候选 = 各笔记本（含软删，其历史同样是事实）当前生效配置 × 全部策略，
   * 经 knowledgeChunkerConfigId 正算后与历史指纹比对——命中即证明该指纹由这组
   * 真实值产生（standard 行，写真实值）；无法匹配的指纹只保留身份键本身
   * （profile_type='legacy'，strategy/target/chunkerVersion 留 NULL，不伪造配置）。
   * 当前生效配置尚无历史证据的行不预建，由首次 resolve 幂等懒建。幂等可重放。
   */
  private backfillChunkProfilesFromIngestionHistory() {
    interface Candidate {
      strategy: KnowledgeChunkerStrategy;
      targetChars: number;
      source: KnowledgeChunkTargetCharsSource;
    }
    const candidates = new Map<string, Candidate>();
    const notebooks = this.db.prepare(`
      SELECT embedding_model_ref, chunk_target_chars FROM notebooks
    `).all();
    for (const row of notebooks) {
      const resolved = resolveNotebookConfig(toNotebookConfig(row));
      const targetChars = resolveEffectiveChunkTargetChars(resolved, this.getEmbeddingModelContextWindow);
      const source: KnowledgeChunkTargetCharsSource = resolved.chunkTargetChars != null ? "explicit" : "auto";
      for (const strategy of CHUNK_PROFILE_STRATEGIES) {
        const hash = knowledgeChunkerConfigId(strategy, targetChars);
        const existing = candidates.get(hash);
        // 同一指纹被多本笔记本推导出来时，explicit 来源比 auto 更具体，优先保留。
        if (!existing || (existing.source === "auto" && source === "explicit")) {
          candidates.set(hash, { strategy, targetChars, source });
        }
      }
    }
    const insert = this.db.prepare(`
      INSERT INTO chunk_profiles (
        id, profile_hash, strategy, target_chars, target_chars_source,
        chunker_version, structural_options_json, profile_type, created_at
      ) VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?)
      ON CONFLICT(profile_hash) DO NOTHING
    `);
    const now = this.now();
    const usedHashes = this.db.prepare(`
      SELECT DISTINCT chunker_config_id FROM ingestion_jobs
    `).all();
    for (const row of usedHashes) {
      const hash = chunkerConfigId(row.chunker_config_id);
      const candidate = candidates.get(hash);
      if (candidate) {
        insert.run(
          `cp_${hash}`, hash, candidate.strategy, candidate.targetChars, candidate.source,
          KNOWLEDGE_CHUNKER_VERSION, "standard", now,
        );
      } else {
        insert.run(`cp_${hash}`, hash, null, null, null, null, "legacy", now);
      }
    }
  }

  /**
   * v17（向量保留天数，原 PR #30 的独立 v9 重编号）：NULL = 永久保留（默认）；
   * 正整数 = 旧版本向量超过 N 天未被查询命中即由 sweep 回收（换模型/重嵌产生
   * 的作废向量不再无限叠加）。列存在检查：测试压版本重开时 ALTER 会对已存在
   * 列报 duplicate，需幂等。
   */
  private createSchemaV17() {
    const columns = this.db.prepare(`PRAGMA table_info(notebooks)`).all() as any[];
    if (columns.some((col) => col.name === "vector_retention_days")) return;
    this.db.exec(`
      ALTER TABLE notebooks ADD COLUMN vector_retention_days INTEGER;
    `);
  }

  /** v18 只新增研究台账；建表和版本号由同一个迁移事务提交，旧资料不改写。 */
  private createSchemaV18() {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS knowledge_research_runs (
        id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(id)) > 0),
        turn_scope_id TEXT NOT NULL,
        turn_id TEXT NOT NULL CHECK(length(trim(turn_id)) > 0),
        parent_session_path TEXT NOT NULL CHECK(length(trim(parent_session_path)) > 0),
        question TEXT NOT NULL CHECK(length(trim(question)) > 0),
        status TEXT NOT NULL CHECK(status IN ('planning', 'running', 'synthesizing', 'completed', 'partial', 'failed', 'cancelled')),
        completeness_policy TEXT NOT NULL CHECK(completeness_policy IN ('best_effort', 'source_diverse', 'relevant_sections_complete', 'scope_complete')),
        budget_json TEXT NOT NULL CHECK(CASE WHEN json_valid(budget_json) THEN
          json_type(budget_json) = 'object' AND COALESCE(
            json_type(budget_json, '$.maxRounds') = 'integer' AND json_extract(budget_json, '$.maxRounds') > 0 AND
            json_type(budget_json, '$.maxParallelAgents') = 'integer' AND json_extract(budget_json, '$.maxParallelAgents') > 0 AND
            json_type(budget_json, '$.maxToolCalls') = 'integer' AND json_extract(budget_json, '$.maxToolCalls') > 0 AND
            json_type(budget_json, '$.maxWallClockMs') = 'integer' AND json_extract(budget_json, '$.maxWallClockMs') > 0 AND
            json_type(budget_json, '$.maxSearchesPerRound') = 'integer' AND json_extract(budget_json, '$.maxSearchesPerRound') > 0 AND
            json_type(budget_json, '$.maxReadsPerRound') = 'integer' AND json_extract(budget_json, '$.maxReadsPerRound') > 0 AND
            json_type(budget_json, '$.maxFinalEvidenceSpans') = 'integer' AND json_extract(budget_json, '$.maxFinalEvidenceSpans') > 0 AND
            json_type(budget_json, '$.finalEvidenceBudgetTokens') = 'integer' AND json_extract(budget_json, '$.finalEvidenceBudgetTokens') > 0,
            0
          ) ELSE 0 END),
        rounds_completed INTEGER NOT NULL DEFAULT 0 CHECK(typeof(rounds_completed) = 'integer' AND rounds_completed >= 0),
        tool_calls_used INTEGER NOT NULL DEFAULT 0 CHECK(typeof(tool_calls_used) = 'integer' AND tool_calls_used >= 0),
        search_calls INTEGER NOT NULL DEFAULT 0 CHECK(typeof(search_calls) = 'integer' AND search_calls >= 0),
        read_calls INTEGER NOT NULL DEFAULT 0 CHECK(typeof(read_calls) = 'integer' AND read_calls >= 0),
        grep_calls INTEGER NOT NULL DEFAULT 0 CHECK(typeof(grep_calls) = 'integer' AND grep_calls >= 0),
        delegated_agents INTEGER NOT NULL DEFAULT 0 CHECK(typeof(delegated_agents) = 'integer' AND delegated_agents >= 0),
        stop_reason TEXT,
        degraded_reason TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        completed_at TEXT,
        FOREIGN KEY(turn_scope_id) REFERENCES knowledge_turn_scopes(id) ON DELETE RESTRICT
      );

      CREATE TABLE IF NOT EXISTS knowledge_evidence_needs (
        id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(id)) > 0),
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(typeof(ordinal) = 'integer' AND ordinal >= 0),
        claim TEXT NOT NULL CHECK(length(trim(claim)) > 0),
        kind TEXT NOT NULL CHECK(kind IN ('fact', 'comparison', 'cause', 'timeline', 'counterexample', 'completeness')),
        required INTEGER NOT NULL CHECK(required IN (0, 1)),
        min_independent_sources INTEGER NOT NULL CHECK(typeof(min_independent_sources) = 'integer' AND min_independent_sources > 0),
        require_counter_evidence INTEGER NOT NULL CHECK(require_counter_evidence IN (0, 1)),
        require_all_relevant_units INTEGER NOT NULL CHECK(require_all_relevant_units IN (0, 1)),
        status TEXT NOT NULL CHECK(status IN ('uncovered', 'partial', 'supported', 'conflicted', 'not_applicable')),
        unresolved_gaps_json TEXT NOT NULL CHECK(CASE WHEN json_valid(unresolved_gaps_json) THEN json_type(unresolved_gaps_json) = 'array' ELSE 0 END),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES knowledge_research_runs(id) ON DELETE RESTRICT
      );

      CREATE TABLE IF NOT EXISTS knowledge_research_rounds (
        id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(id)) > 0),
        run_id TEXT NOT NULL,
        ordinal INTEGER NOT NULL CHECK(typeof(ordinal) = 'integer' AND ordinal >= 0),
        focus_json TEXT NOT NULL CHECK(CASE WHEN json_valid(focus_json) THEN json_type(focus_json) = 'array' ELSE 0 END),
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'cancelled')),
        new_evidence_count INTEGER NOT NULL DEFAULT 0 CHECK(typeof(new_evidence_count) = 'integer' AND new_evidence_count >= 0),
        started_at TEXT NOT NULL,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES knowledge_research_runs(id) ON DELETE RESTRICT
      );

      CREATE TABLE IF NOT EXISTS knowledge_research_read_receipts (
        id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(id)) > 0),
        run_id TEXT NOT NULL,
        actor_session_id TEXT,
        source_id TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        chunk_index_variant_id TEXT,
        chunk_id TEXT,
        block_id TEXT NOT NULL,
        start_offset INTEGER NOT NULL CHECK(typeof(start_offset) = 'integer' AND start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(typeof(end_offset) = 'integer' AND end_offset > start_offset),
        canonical_text_sha256 TEXT NOT NULL CHECK(length(canonical_text_sha256) = 64 AND length(CAST(canonical_text_sha256 AS BLOB)) = 64 AND canonical_text_sha256 NOT GLOB '*[^0-9a-f]*'),
        channel TEXT NOT NULL CHECK(channel IN ('knowledge_read', 'knowledge_grep')),
        created_at TEXT NOT NULL,
        consumed_at TEXT,
        FOREIGN KEY(run_id) REFERENCES knowledge_research_runs(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE TABLE IF NOT EXISTS knowledge_evidence_items (
        id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(id)) > 0),
        run_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        content_snapshot_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        chunk_index_variant_id TEXT,
        chunk_id TEXT,
        block_id TEXT NOT NULL,
        start_offset INTEGER NOT NULL CHECK(typeof(start_offset) = 'integer' AND start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(typeof(end_offset) = 'integer' AND end_offset > start_offset),
        canonical_text TEXT NOT NULL CHECK(length(canonical_text) > 0),
        canonical_text_sha256 TEXT NOT NULL CHECK(length(canonical_text_sha256) = 64 AND length(CAST(canonical_text_sha256 AS BLOB)) = 64 AND canonical_text_sha256 NOT GLOB '*[^0-9a-f]*'),
        heading_path_json TEXT CHECK(heading_path_json IS NULL OR CASE WHEN json_valid(heading_path_json) THEN json_type(heading_path_json) = 'array' ELSE 0 END),
        page_number INTEGER CHECK(page_number IS NULL OR (typeof(page_number) = 'integer' AND page_number > 0)),
        created_at TEXT NOT NULL,
        UNIQUE(run_id, parse_artifact_id, block_id, start_offset, end_offset),
        FOREIGN KEY(run_id) REFERENCES knowledge_research_runs(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(content_snapshot_id) REFERENCES content_snapshots(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE TABLE IF NOT EXISTS knowledge_need_evidence (
        need_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        relation TEXT NOT NULL CHECK(relation IN ('supports', 'contradicts', 'context')),
        rationale TEXT NOT NULL CHECK(length(trim(rationale)) > 0),
        source_independence_key TEXT NOT NULL,
        created_at TEXT NOT NULL,
        PRIMARY KEY(need_id, evidence_id, relation),
        FOREIGN KEY(need_id) REFERENCES knowledge_evidence_needs(id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES knowledge_evidence_items(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_independence_key) REFERENCES sources(id) ON DELETE RESTRICT
      );

      CREATE TABLE IF NOT EXISTS knowledge_research_actions (
        id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(id)) > 0),
        run_id TEXT NOT NULL,
        round_id TEXT,
        ordinal INTEGER NOT NULL CHECK(typeof(ordinal) = 'integer' AND ordinal >= 0),
        actor_session_id TEXT,
        actor_agent_id TEXT,
        action_type TEXT NOT NULL CHECK(length(trim(action_type)) > 0),
        request_summary_json TEXT NOT NULL CHECK(CASE WHEN json_valid(request_summary_json) THEN json_type(request_summary_json) = 'object' ELSE 0 END),
        response_summary_json TEXT CHECK(response_summary_json IS NULL OR CASE WHEN json_valid(response_summary_json) THEN json_type(response_summary_json) = 'object' ELSE 0 END),
        status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'cancelled')),
        started_at TEXT NOT NULL,
        completed_at TEXT,
        error_code TEXT,
        UNIQUE(run_id, ordinal),
        FOREIGN KEY(run_id) REFERENCES knowledge_research_runs(id) ON DELETE RESTRICT,
        FOREIGN KEY(round_id) REFERENCES knowledge_research_rounds(id) ON DELETE RESTRICT
      );
    `);
  }

  /** v19 只新增完整性核查记录；与版本号一起提交，旧资料和研究证据均保持原样。 */
  private createSchemaV19() {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS knowledge_completeness_checks (
        id TEXT PRIMARY KEY NOT NULL CHECK(length(trim(id)) > 0),
        research_run_id TEXT NOT NULL UNIQUE,
        policy TEXT NOT NULL CHECK(policy IN ('best_effort', 'source_diverse', 'relevant_sections_complete', 'scope_complete')),
        status TEXT NOT NULL CHECK(length(trim(status)) > 0),
        total_units INTEGER NOT NULL DEFAULT 0 CHECK(typeof(total_units) = 'integer' AND total_units >= 0),
        checked_units INTEGER NOT NULL DEFAULT 0 CHECK(typeof(checked_units) = 'integer' AND checked_units >= 0),
        relevant_units INTEGER NOT NULL DEFAULT 0 CHECK(typeof(relevant_units) = 'integer' AND relevant_units >= 0),
        unavailable_units INTEGER NOT NULL DEFAULT 0 CHECK(typeof(unavailable_units) = 'integer' AND unavailable_units >= 0),
        coverage_ratio REAL NOT NULL DEFAULT 0 CHECK(coverage_ratio >= 0 AND coverage_ratio <= 1),
        exact INTEGER NOT NULL DEFAULT 0 CHECK(exact IN (0, 1)),
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        completed_at TEXT,
        CHECK(checked_units + unavailable_units <= total_units),
        CHECK(relevant_units <= checked_units),
        CHECK(exact = 0 OR (checked_units = total_units AND unavailable_units = 0 AND coverage_ratio = 1)),
        FOREIGN KEY(research_run_id) REFERENCES knowledge_research_runs(id) ON DELETE RESTRICT
      );

      CREATE TABLE IF NOT EXISTS knowledge_completeness_units (
        check_id TEXT NOT NULL,
        coverage_unit_id TEXT NOT NULL CHECK(length(trim(coverage_unit_id)) > 0),
        source_id TEXT NOT NULL,
        parse_artifact_id TEXT NOT NULL,
        block_id TEXT NOT NULL,
        start_offset INTEGER NOT NULL CHECK(typeof(start_offset) = 'integer' AND start_offset >= 0),
        end_offset INTEGER NOT NULL CHECK(typeof(end_offset) = 'integer' AND end_offset > start_offset),
        section_key TEXT,
        status TEXT NOT NULL CHECK(status IN ('pending', 'checked_relevant', 'checked_irrelevant', 'unavailable', 'failed')),
        worker_session_id TEXT,
        updated_at TEXT NOT NULL,
        PRIMARY KEY(check_id, coverage_unit_id),
        FOREIGN KEY(check_id) REFERENCES knowledge_completeness_checks(id) ON DELETE RESTRICT,
        FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE RESTRICT,
        FOREIGN KEY(parse_artifact_id) REFERENCES parse_artifacts(id) ON DELETE RESTRICT,
        FOREIGN KEY(block_id) REFERENCES knowledge_blocks(id) ON DELETE RESTRICT
      );

      CREATE TABLE IF NOT EXISTS knowledge_completeness_unit_evidence (
        check_id TEXT NOT NULL,
        coverage_unit_id TEXT NOT NULL,
        evidence_id TEXT NOT NULL,
        PRIMARY KEY(check_id, coverage_unit_id, evidence_id),
        FOREIGN KEY(check_id, coverage_unit_id) REFERENCES knowledge_completeness_units(check_id, coverage_unit_id) ON DELETE RESTRICT,
        FOREIGN KEY(evidence_id) REFERENCES knowledge_evidence_items(id) ON DELETE RESTRICT
      );

      CREATE TABLE IF NOT EXISTS knowledge_completeness_coverage_runs (
        check_id TEXT NOT NULL,
        coverage_run_id TEXT NOT NULL,
        PRIMARY KEY(check_id, coverage_run_id),
        FOREIGN KEY(check_id) REFERENCES knowledge_completeness_checks(id) ON DELETE RESTRICT,
        FOREIGN KEY(coverage_run_id) REFERENCES coverage_runs(id) ON DELETE RESTRICT
      );
    `);
  }

  private newId(prefix: string): string {
    return requiredString(this.idGenerator(prefix), `${prefix} id`, 128);
  }

  private activeNotebook(studioId: string, notebookId: string): KnowledgeNotebook {
    const row = this.db.prepare(`
      SELECT * FROM notebooks
      WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
    `).get(notebookId, studioId);
    const notebook = toNotebook(row);
    if (!notebook) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook not found");
    return notebook;
  }

  private activeSource(studioId: string, sourceId: string): KnowledgeSource {
    const row = this.db.prepare(`
      SELECT * FROM sources
      WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
    `).get(sourceId, studioId);
    const source = toSource(row);
    if (!source) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge source not found");
    return source;
  }

  createNotebook(input: { studioId: unknown; name: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const name = requiredString(input?.name, "name", 120);
    const id = this.newId("nb");
    const now = this.now();
    // v8 起显式写 NULL：retrieval_top_k/chunk_target_chars 列的 DDL DEFAULT
    // （12/1200）是 v6 遗留，新笔记本必须以"无上限/自动"起步而非撞上旧默认。
    this.db.prepare(`
      INSERT INTO notebooks (id, studio_id, name, created_at, updated_at, deleted_at, retrieval_top_k, chunk_target_chars)
      VALUES (?, ?, ?, ?, ?, NULL, NULL, NULL)
    `).run(id, studioId, name, now, now);
    return this.getNotebook({ studioId, notebookId: id });
  }

  listNotebooks(input: { studioId: unknown }): KnowledgeNotebook[] {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    return this.db.prepare(`
      SELECT * FROM notebooks
      WHERE studio_id = ? AND deleted_at IS NULL
      ORDER BY updated_at DESC, id ASC
    `).all(studioId).map(toNotebook);
  }

  getNotebook(input: { studioId: unknown; notebookId: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    return this.activeNotebook(studioId, notebookId);
  }

  /** 查询只读当前绑定，一次关联读取，不扫描正文或重新计算分块配置。 */
  getNotebookRetrievalProfileSnapshot(input: {
    studioId: unknown;
    notebookId: unknown;
  }): {
    notebookId: string;
    notebookName: string;
    chunkProfileHash: string | null;
    embeddingModelRef: KnowledgeModelRef | null;
    rerankModelRef: KnowledgeModelRef | null;
  } {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const row = this.db.prepare(`
      SELECT n.id, n.name, cp.profile_hash, rp.embedding_model_ref, rp.rerank_model_ref
      FROM notebooks n
      LEFT JOIN retrieval_profiles rp ON rp.id = n.retrieval_profile_id
      LEFT JOIN chunk_profiles cp ON cp.id = rp.chunk_profile_id
      WHERE n.id = ? AND n.studio_id = ? AND n.deleted_at IS NULL
    `).get(notebookId, studioId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook not found");
    return {
      notebookId: row.id,
      notebookName: row.name,
      chunkProfileHash: row.profile_hash ?? null,
      embeddingModelRef: parseModelRefJson(row.embedding_model_ref, "retrieval profile embedding model ref"),
      rerankModelRef: parseModelRefJson(row.rerank_model_ref, "retrieval profile rerank model ref"),
    };
  }

  renameNotebook(input: { studioId: unknown; notebookId: unknown; name: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const name = requiredString(input?.name, "name", 120);
    this.activeNotebook(studioId, notebookId);
    this.db.prepare(`
      UPDATE notebooks SET name = ?, updated_at = ?
      WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
    `).run(name, this.now(), notebookId, studioId);
    return this.activeNotebook(studioId, notebookId);
  }

  deleteNotebook(input: { studioId: unknown; notebookId: unknown }): KnowledgeNotebook {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const notebook = this.activeNotebook(studioId, notebookId);
    const deletedAt = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        UPDATE notebooks SET deleted_at = ?, updated_at = ?
        WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
      `).run(deletedAt, deletedAt, notebookId, studioId);
      this.db.prepare(`
        UPDATE notebook_sources SET removed_at = ?
        WHERE notebook_id = ? AND removed_at IS NULL
      `).run(deletedAt, notebookId);
    })();
    return { ...notebook, updatedAt: deletedAt, deletedAt };
  }

  getNotebookConfig(input: { studioId: unknown; notebookId: unknown }): NotebookConfig {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    this.activeNotebook(studioId, notebookId);
    return toNotebookConfig(this.db.prepare(`
      SELECT embedding_model_ref, rerank_model_ref, chunk_target_chars, retrieval_top_k, vector_retention_days
      FROM notebooks
      WHERE id = ? AND studio_id = ?
    `).get(notebookId, studioId));
  }

  /**
   * 源的全部解析产物 id（含历史版本）：派生索引清理用。
   * 软删除源同样返回——索引清理正是删除语义的一部分。
   */
  listSourceArtifactIds(input: { sourceId: unknown }): string[] {
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return this.db.prepare(`
      SELECT pa.id FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      WHERE cs.source_id = ?
      ORDER BY pa.created_at
    `).all(sourceId).map((row: any) => row.id);
  }

  /**
   * 仍活跃挂靠该源的笔记本 id（membership 未移除且笔记本未删除）。
   * 空数组 = 孤儿源：没有任何笔记本可达，派生索引可清理。
   */
  listActiveNotebookIdsForSource(input: { sourceId: unknown }): string[] {
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return this.db.prepare(`
      SELECT ns.notebook_id FROM notebook_sources ns
      JOIN notebooks n ON n.id = ns.notebook_id
      WHERE ns.source_id = ? AND ns.removed_at IS NULL AND n.deleted_at IS NULL
    `).all(sourceId).map((row: any) => row.notebook_id);
  }

  /**
   * 孤儿源的全部解析产物 id：源本身未删（快照/产物仍活跃）但已无任何
   * 活跃挂靠笔记本（UI 不可达）。历史删除动作发生在删除清理逻辑上线前的
   * 残留由 sweep 兜底回收；新删除已由 manager 即时清理，通常为空。
   */
  listOrphanArtifactIds(): string[] {
    return this.db.prepare(`
      SELECT pa.id FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id AND s.deleted_at IS NULL
      WHERE NOT EXISTS (
        SELECT 1 FROM notebook_sources ns
        JOIN notebooks n ON n.id = ns.notebook_id AND n.deleted_at IS NULL
        WHERE ns.source_id = s.id AND ns.removed_at IS NULL
      )
    `).all().map((row: any) => row.id);
  }

  /**
   * 向量 sweep 的归属视图：全部活跃源的解析产物 × 是否该源最新产物 ×
   * 活跃挂靠笔记本的保留策略（取最宽松的最大值；任一笔记本未配置 = 该源
   * 永久保留，不误删仍被引用的向量）。历史产物（isLatestForSource=false）
   * 与同产物非当前模型身份的向量是"旧版本"回收候选。
   */
  listArtifactVectorSweepRows(): Array<{
    artifactId: string;
    sourceId: string;
    isLatestForSource: boolean;
    retentionDays: number | null;
  }> {
    const retentionBySource = new Map<string, number | null>();
    for (const row of this.db.prepare(`
      SELECT ns.source_id,
             MAX(n.vector_retention_days) AS max_days,
             COUNT(n.vector_retention_days) AS configured_count,
             COUNT(*) AS total_count
      FROM notebook_sources ns
      JOIN notebooks n ON n.id = ns.notebook_id
      WHERE ns.removed_at IS NULL AND n.deleted_at IS NULL
      GROUP BY ns.source_id
    `).all() as any[]) {
      // 任一挂靠笔记本未配置保留策略 = 该源永久保留。
      retentionBySource.set(
        row.source_id,
        Number(row.configured_count) < Number(row.total_count)
          ? null
          : row.max_days == null ? null : Number(row.max_days),
      );
    }
    const latestBySource = new Map<string, string>();
    const artifacts: Array<{ sourceId: string; artifactId: string }> = this.db.prepare(`
      SELECT cs.source_id, pa.id AS artifact_id
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id AND s.deleted_at IS NULL
      ORDER BY pa.created_at
    `).all().map((row: any) => ({ sourceId: row.source_id, artifactId: row.artifact_id }));
    for (const { sourceId, artifactId } of artifacts) {
      latestBySource.set(sourceId, artifactId);
    }
    return artifacts.map(({ sourceId, artifactId }) => ({
      artifactId,
      sourceId,
      isLatestForSource: latestBySource.get(sourceId) === artifactId,
      retentionDays: retentionBySource.get(sourceId) ?? null,
    }));
  }

  /**
   * 笔记本配置部分更新：字段 omitted → 不变；null → 清除为 NULL
   * （模型引用回 NULL = 未配置，数值回 NULL = 自动分块/无上限召回）；
   * 否则校验后写入。至少给一个字段，避免空调用被静默接受。
   * 配置变更后同事务刷新 RetrievalProfile 绑定（v9）：沿用当前绑定 chunk profile
   * 的策略重算生效配置并 find-or-create 新 profile 后切换绑定（建立新版本 → 切换，
   * 旧 profile 行保留）；从未绑定的笔记本保持 NULL，留给首次 resolve 惰性建绑。
   */
  updateNotebookConfig(input: {
    studioId: unknown;
    notebookId: unknown;
    embeddingModelRef?: unknown;
    rerankModelRef?: unknown;
    chunkTargetChars?: unknown;
    retrievalTopK?: unknown;
    getEmbeddingModelContextWindow?: ((modelRef: KnowledgeModelRef) => number | null) | null;
    vectorRetentionDays?: unknown;
  }): NotebookConfig {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    this.activeNotebook(studioId, notebookId);
    const assignments: string[] = [];
    const params: unknown[] = [];
    const hasField = (field: string) => Object.prototype.hasOwnProperty.call(input ?? {}, field);
    if (hasField("embeddingModelRef")) {
      assignments.push("embedding_model_ref = ?");
      params.push(input.embeddingModelRef == null
        ? null
        : serializeModelRef(input.embeddingModelRef, "embeddingModelRef"));
    }
    if (hasField("rerankModelRef")) {
      assignments.push("rerank_model_ref = ?");
      params.push(input.rerankModelRef == null
        ? null
        : serializeModelRef(input.rerankModelRef, "rerankModelRef"));
    }
    if (hasField("chunkTargetChars")) {
      assignments.push("chunk_target_chars = ?");
      params.push(optionalIntegerInRange(
        input.chunkTargetChars,
        "chunkTargetChars",
        MIN_KNOWLEDGE_CHUNK_TARGET_CHARS,
        MAX_KNOWLEDGE_CHUNK_TARGET_CHARS,
      ));
    }
    if (hasField("retrievalTopK")) {
      assignments.push("retrieval_top_k = ?");
      params.push(optionalIntegerInRange(
        input.retrievalTopK,
        "retrievalTopK",
        MIN_RETRIEVAL_TOP_K,
        MAX_RETRIEVAL_TOP_K,
      ));
    }
    if (hasField("vectorRetentionDays")) {
      assignments.push("vector_retention_days = ?");
      params.push(optionalIntegerInRange(
        input.vectorRetentionDays,
        "vectorRetentionDays",
        MIN_VECTOR_RETENTION_DAYS,
        MAX_VECTOR_RETENTION_DAYS,
      ));
    }
    if (assignments.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Notebook config update requires at least one field");
    }
    this.db.transaction(() => {
      this.db.prepare(`
        UPDATE notebooks SET ${assignments.join(", ")}, updated_at = ?
        WHERE id = ? AND studio_id = ? AND deleted_at IS NULL
      `).run(...params, this.now(), notebookId, studioId);
      this.refreshNotebookRetrievalProfileBinding({
        studioId,
        notebookId,
        getEmbeddingModelContextWindow: input.getEmbeddingModelContextWindow,
      });
    })();
    return this.getNotebookConfig({ studioId, notebookId });
  }

  /**
   * 一次性迁移（v8 配套）：把已退役的全局嵌入/重排引用写入所有未单独配置
   * 的活跃笔记本（WHERE … IS NULL 保证不覆盖显式配置）。迁移值与旧解析链
   * 会解析出的同一引用，语义零变化，不触发重建。幂等：列已写/无全局值时 0 行。
   */
  migrateLegacyGlobalModelRefs(input: {
    embeddingModelRef: KnowledgeModelRef | null;
    rerankModelRef: KnowledgeModelRef | null;
  }): { notebooksUpdated: number } {
    let notebooksUpdated = 0;
    this.db.transaction(() => {
      if (input.embeddingModelRef) {
        notebooksUpdated += this.db.prepare(`
          UPDATE notebooks SET embedding_model_ref = ?, updated_at = ?
          WHERE embedding_model_ref IS NULL AND deleted_at IS NULL
        `).run(JSON.stringify(input.embeddingModelRef), this.now()).changes;
      }
      if (input.rerankModelRef) {
        notebooksUpdated += this.db.prepare(`
          UPDATE notebooks SET rerank_model_ref = ?, updated_at = ?
          WHERE rerank_model_ref IS NULL AND deleted_at IS NULL
        `).run(JSON.stringify(input.rerankModelRef), this.now()).changes;
      }
    })();
    return { notebooksUpdated };
  }

  /**
   * ChunkProfile 幂等注册：profileHash = knowledgeChunkerConfigId(strategy, targetChars)
   * （跨库身份键，与 chunk id / ingestion_jobs.chunker_config_id 同源）；同 hash 返回既有行。
   * 只创建 standard 行——legacy 行仅由 v9 迁移对不可推导历史指纹生成。
   */
  findOrCreateChunkProfile(input: {
    strategy: unknown;
    targetChars: unknown;
    targetCharsSource: unknown;
    structuralOptions?: unknown;
  }): KnowledgeChunkProfile {
    const strategy = chunkProfileStrategy(input?.strategy);
    const targetChars = optionalIntegerInRange(
      input?.targetChars,
      "targetChars",
      MIN_KNOWLEDGE_CHUNK_TARGET_CHARS,
      MAX_KNOWLEDGE_CHUNK_TARGET_CHARS,
    );
    if (targetChars == null) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "targetChars is required");
    }
    const source = chunkTargetCharsSource(input?.targetCharsSource);
    const structuralOptionsJson = input?.structuralOptions == null
      ? null
      : serializeObjectJson(input.structuralOptions, "structuralOptions");
    const profileHash = knowledgeChunkerConfigId(strategy, targetChars);
    const existing = toChunkProfile(this.db.prepare(`
      SELECT * FROM chunk_profiles WHERE profile_hash = ?
    `).get(profileHash));
    if (existing) return existing;
    this.db.prepare(`
      INSERT INTO chunk_profiles (
        id, profile_hash, strategy, target_chars, target_chars_source,
        chunker_version, structural_options_json, profile_type, created_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, 'standard', ?)
    `).run(
      `cp_${profileHash}`, profileHash, strategy, targetChars, source,
      KNOWLEDGE_CHUNKER_VERSION, structuralOptionsJson, this.now(),
    );
    return this.getChunkProfile({ profileHash });
  }

  /**
   * RetrievalProfile 幂等注册：profileKey 由 chunkProfileHash + 模型引用 + topK
   * 规范化生成（knowledgeRetrievalProfileKey）；同 key 返回既有行。
   */
  findOrCreateRetrievalProfile(input: {
    chunkProfileId: unknown;
    embeddingModelRef?: unknown;
    rerankModelRef?: unknown;
    retrievalTopK?: unknown;
  }): KnowledgeRetrievalProfile {
    const chunkProfileId = requiredString(input?.chunkProfileId, "chunkProfileId", 128);
    const chunkProfile = this.getChunkProfile({ profileId: chunkProfileId });
    const embeddingModelRef = input?.embeddingModelRef == null
      ? null
      : parseModelRefJson(serializeModelRef(input.embeddingModelRef, "embeddingModelRef"), "embeddingModelRef");
    const rerankModelRef = input?.rerankModelRef == null
      ? null
      : parseModelRefJson(serializeModelRef(input.rerankModelRef, "rerankModelRef"), "rerankModelRef");
    const retrievalTopK = optionalIntegerInRange(
      input?.retrievalTopK,
      "retrievalTopK",
      MIN_RETRIEVAL_TOP_K,
      MAX_RETRIEVAL_TOP_K,
    );
    const profileKey = knowledgeRetrievalProfileKey({
      chunkProfileHash: chunkProfile.profileHash,
      embeddingModelRef,
      rerankModelRef,
      retrievalTopK,
    });
    const existing = toRetrievalProfile(this.db.prepare(`
      SELECT * FROM retrieval_profiles WHERE profile_key = ?
    `).get(profileKey));
    if (existing) return existing;
    this.db.prepare(`
      INSERT INTO retrieval_profiles (
        id, profile_key, chunk_profile_id, embedding_model_ref, rerank_model_ref, retrieval_top_k, created_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?)
    `).run(
      `rp_${profileKey}`, profileKey, chunkProfileId,
      embeddingModelRef ? JSON.stringify(embeddingModelRef) : null,
      rerankModelRef ? JSON.stringify(rerankModelRef) : null,
      retrievalTopK, this.now(),
    );
    return this.getRetrievalProfile({ profileKey });
  }

  getChunkProfile(input: { profileId?: unknown; profileHash?: unknown }): KnowledgeChunkProfile {
    let row: any;
    if (input?.profileId != null) {
      row = this.db.prepare(`SELECT * FROM chunk_profiles WHERE id = ?`)
        .get(requiredString(input.profileId, "profileId", 128));
    } else if (input?.profileHash != null) {
      row = this.db.prepare(`SELECT * FROM chunk_profiles WHERE profile_hash = ?`)
        .get(chunkerConfigId(input.profileHash));
    } else {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "getChunkProfile requires profileId or profileHash");
    }
    const profile = toChunkProfile(row);
    if (!profile) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Chunk profile not found");
    return profile;
  }

  getRetrievalProfile(input: { profileId?: unknown; profileKey?: unknown }): KnowledgeRetrievalProfile {
    let row: any;
    if (input?.profileId != null) {
      row = this.db.prepare(`SELECT * FROM retrieval_profiles WHERE id = ?`)
        .get(requiredString(input.profileId, "profileId", 128));
    } else if (input?.profileKey != null) {
      row = this.db.prepare(`SELECT * FROM retrieval_profiles WHERE profile_key = ?`)
        .get(chunkerConfigId(input.profileKey));
    } else {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "getRetrievalProfile requires profileId or profileKey");
    }
    const profile = toRetrievalProfile(row);
    if (!profile) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Retrieval profile not found");
    return profile;
  }

  /**
   * 解析笔记本生效检索配置并绑定 RetrievalProfile（任务书 §九）：
   * notebooks 现有列（含 resolveEffectiveChunkTargetChars 自动解析链）→ find-or-create
   * chunk/retrieval profile → 绑定不同则同事务更新 notebooks.retrieval_profile_id。
   * strategy 必传：分块策略随各 artifact 内容派发（resolveKnowledgeChunkerConfig），
   * 调用方（摄入/查询侧）在持有具体 artifact 时已解析；绑定本身不加 updated_at，
   * 避免不同策略的源轮流解析时扰动笔记本列表排序。旧 profile 行保留（additive），
   * 零引用清理属于后续 GC 波次，不在此处删除。
   */
  resolveNotebookRetrievalProfile(input: {
    studioId: unknown;
    notebookId: unknown;
    strategy: unknown;
    getEmbeddingModelContextWindow?: ((modelRef: KnowledgeModelRef) => number | null) | null;
  }): {
    chunkProfile: KnowledgeChunkProfile;
    retrievalProfile: KnowledgeRetrievalProfile;
    bindingUpdated: boolean;
  } {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const strategy = chunkProfileStrategy(input?.strategy);
    this.activeNotebook(studioId, notebookId);
    const result = this.db.transaction(() => this.refreshNotebookRetrievalProfileBinding({
      studioId,
      notebookId,
      strategy,
      getEmbeddingModelContextWindow: input?.getEmbeddingModelContextWindow,
    }))();
    if (!result) {
      // 显式 strategy 下 refresh 必有结果；此分支仅为类型收敛兜底。
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Notebook retrieval profile resolution failed");
    }
    return result;
  }

  /**
   * 绑定刷新（resolve / updateNotebookConfig 共用）。strategy 缺省时继承当前绑定
   * 的 chunk profile 策略（配置变更不改变策略——策略由 artifact 内容派发）；
   * 从未绑定或绑定到 legacy profile 时返回 null，保持 NULL 绑定、留给首次
   * resolve 惰性建绑（不伪造策略）。
   */
  private refreshNotebookRetrievalProfileBinding(input: {
    studioId: string;
    notebookId: string;
    strategy?: KnowledgeChunkerStrategy;
    getEmbeddingModelContextWindow?: ((modelRef: KnowledgeModelRef) => number | null) | null;
  }): {
    chunkProfile: KnowledgeChunkProfile;
    retrievalProfile: KnowledgeRetrievalProfile;
    bindingUpdated: boolean;
  } | null {
    const row = this.db.prepare(`
      SELECT embedding_model_ref, rerank_model_ref, chunk_target_chars, retrieval_top_k, retrieval_profile_id
      FROM notebooks
      WHERE id = ? AND studio_id = ?
    `).get(input.notebookId, input.studioId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook not found");
    let strategy = input.strategy ?? null;
    if (!strategy) {
      if (row.retrieval_profile_id == null) return null;
      const bound = this.db.prepare(`
        SELECT cp.strategy AS strategy, cp.profile_type AS profile_type
        FROM retrieval_profiles rp
        JOIN chunk_profiles cp ON cp.id = rp.chunk_profile_id
        WHERE rp.id = ?
      `).get(row.retrieval_profile_id);
      if (!bound || bound.profile_type !== "standard" || bound.strategy == null) return null;
      strategy = chunkProfileStrategy(bound.strategy);
    }
    const resolved = resolveNotebookConfig(toNotebookConfig(row));
    const targetChars = resolveEffectiveChunkTargetChars(
      resolved,
      input.getEmbeddingModelContextWindow ?? this.getEmbeddingModelContextWindow,
    );
    const chunkProfile = this.findOrCreateChunkProfile({
      strategy,
      targetChars,
      targetCharsSource: resolved.chunkTargetChars != null ? "explicit" : "auto",
    });
    const retrievalProfile = this.findOrCreateRetrievalProfile({
      chunkProfileId: chunkProfile.id,
      embeddingModelRef: resolved.embeddingModelRef,
      rerankModelRef: resolved.rerankModelRef,
      retrievalTopK: resolved.retrievalTopK,
    });
    const bindingUpdated = row.retrieval_profile_id !== retrievalProfile.id;
    if (bindingUpdated) {
      this.db.prepare(`
        UPDATE notebooks SET retrieval_profile_id = ? WHERE id = ? AND studio_id = ?
      `).run(retrievalProfile.id, input.notebookId, input.studioId);
    }
    return { chunkProfile, retrievalProfile, bindingUpdated };
  }

  createSourceWithSnapshot(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId?: unknown;
    snapshotId?: unknown;
    sourceType: unknown;
    displayName: unknown;
    originMetadata: unknown;
    snapshot: {
      sha256: unknown;
      mimeType: unknown;
      byteSize: unknown;
      storagePath: unknown;
    };
  }): ImportedKnowledgeSource {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    this.activeNotebook(studioId, notebookId);
    const id = input.sourceId == null
      ? this.newId("src")
      : requiredString(input.sourceId, "sourceId", 128);
    const snapshotId = input.snapshotId == null
      ? this.newId("snap")
      : requiredString(input.snapshotId, "snapshotId", 128);
    const normalizedType = sourceType(input.sourceType);
    const displayName = requiredString(input.displayName, "displayName", 255);
    const originMetadataJson = serializeObjectJson(input.originMetadata, "originMetadata");
    const snapshotSha = sha256(input.snapshot?.sha256);
    const mimeType = requiredString(input.snapshot?.mimeType, "mimeType", 255).toLowerCase();
    const snapshotBytes = byteSize(input.snapshot?.byteSize);
    const snapshotStoragePath = storagePath(input.snapshot?.storagePath);
    const now = this.now();

    this.db.transaction(() => {
      this.db.prepare(`
        INSERT INTO sources (
          id, studio_id, source_type, display_name, origin_metadata_json, created_at, deleted_at
        ) VALUES (?, ?, ?, ?, ?, ?, NULL)
      `).run(id, studioId, normalizedType, displayName, originMetadataJson, now);
      this.db.prepare(`
        INSERT INTO content_snapshots (
          id, source_id, sha256, mime_type, byte_size, storage_path, captured_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      `).run(snapshotId, id, snapshotSha, mimeType, snapshotBytes, snapshotStoragePath, now);
      this.db.prepare(`
        INSERT INTO notebook_sources (notebook_id, source_id, added_at, removed_at)
        VALUES (?, ?, ?, NULL)
      `).run(notebookId, id, now);
      this.db.prepare(`UPDATE notebooks SET updated_at = ? WHERE id = ?`).run(now, notebookId);
    })();

    return {
      source: this.activeSource(studioId, id),
      snapshot: this.getContentSnapshot({ studioId, snapshotId }),
      membership: this.getMembership(notebookId, id),
    };
  }

  private getMembership(notebookId: string, sourceId: string): NotebookSourceMembership {
    const membership = toMembership(this.db.prepare(`
      SELECT * FROM notebook_sources WHERE notebook_id = ? AND source_id = ?
    `).get(notebookId, sourceId));
    if (!membership) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook source membership not found");
    return membership;
  }

  addSourceToNotebook(input: { studioId: unknown; notebookId: unknown; sourceId: unknown }) {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeNotebook(studioId, notebookId);
    this.activeSource(studioId, sourceId);
    const existing = toMembership(this.db.prepare(`
      SELECT * FROM notebook_sources WHERE notebook_id = ? AND source_id = ?
    `).get(notebookId, sourceId));
    if (existing && existing.removedAt === null) return existing;

    const now = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        INSERT INTO notebook_sources (notebook_id, source_id, added_at, removed_at)
        VALUES (?, ?, ?, NULL)
        ON CONFLICT(notebook_id, source_id) DO UPDATE SET
          added_at = excluded.added_at,
          removed_at = NULL
      `).run(notebookId, sourceId, now);
      this.db.prepare(`UPDATE notebooks SET updated_at = ? WHERE id = ?`).run(now, notebookId);
    })();
    return this.getMembership(notebookId, sourceId);
  }

  removeSourceFromNotebook(input: { studioId: unknown; notebookId: unknown; sourceId: unknown }) {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeNotebook(studioId, notebookId);
    this.activeSource(studioId, sourceId);
    const membership = this.getMembership(notebookId, sourceId);
    if (membership.removedAt !== null) {
      throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook source membership not found");
    }
    const now = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        UPDATE notebook_sources SET removed_at = ?
        WHERE notebook_id = ? AND source_id = ? AND removed_at IS NULL
      `).run(now, notebookId, sourceId);
      this.db.prepare(`UPDATE notebooks SET updated_at = ? WHERE id = ?`).run(now, notebookId);
    })();
    return { ...membership, removedAt: now };
  }

  listNotebookSources(input: { studioId: unknown; notebookId: unknown }): Array<{
    source: KnowledgeSource;
    snapshot: ContentSnapshot;
    membership: NotebookSourceMembership;
    parseArtifact: KnowledgeParseArtifact | null;
  }> {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    this.activeNotebook(studioId, notebookId);
    const rows = this.db.prepare(`
      SELECT
        s.*,
        ns.notebook_id AS membership_notebook_id,
        ns.source_id AS membership_source_id,
        ns.added_at AS membership_added_at,
        ns.removed_at AS membership_removed_at,
        ns.relative_path AS membership_relative_path,
        ns.folder_node AS membership_folder_node,
        ns.display_order AS membership_display_order,
        cs.id AS snapshot_id,
        cs.sha256 AS snapshot_sha256,
        cs.mime_type AS snapshot_mime_type,
        cs.byte_size AS snapshot_byte_size,
        cs.storage_path AS snapshot_storage_path,
        cs.captured_at AS snapshot_captured_at,
        pa.id AS parse_id,
        pa.content_snapshot_id AS parse_content_snapshot_id,
        pa.parser_id AS parse_parser_id,
        pa.parser_version AS parse_parser_version,
        pa.parser_config_hash AS parse_parser_config_hash,
        pa.status AS parse_status,
        pa.warnings_json AS parse_warnings_json,
        pa.semantic_artifact_path AS parse_semantic_artifact_path,
        pa.fidelity AS parse_fidelity,
        pa.processing_artifact_id AS parse_processing_artifact_id,
        pa.created_at AS parse_created_at,
        pa.completed_at AS parse_completed_at
      FROM notebook_sources ns
      JOIN sources s ON s.id = ns.source_id
      JOIN content_snapshots cs ON cs.id = (
        SELECT inner_cs.id FROM content_snapshots inner_cs
        WHERE inner_cs.source_id = s.id
        ORDER BY inner_cs.captured_at DESC, inner_cs.id DESC
        LIMIT 1
      )
      LEFT JOIN parse_artifacts pa ON pa.id = (
        SELECT inner_pa.id FROM parse_artifacts inner_pa
        WHERE inner_pa.content_snapshot_id = cs.id
        ORDER BY inner_pa.created_at DESC, inner_pa.id DESC
        LIMIT 1
      )
      WHERE ns.notebook_id = ?
        AND ns.removed_at IS NULL
        AND s.studio_id = ?
        AND s.deleted_at IS NULL
      ORDER BY ns.added_at ASC, s.id ASC
    `).all(notebookId, studioId);

    return rows.map((row: any) => ({
      source: toSource(row)!,
      membership: {
        notebookId: row.membership_notebook_id,
        sourceId: row.membership_source_id,
        addedAt: row.membership_added_at,
        removedAt: row.membership_removed_at || null,
        relativePath: row.membership_relative_path ?? null,
        folderNode: row.membership_folder_node ?? null,
        displayOrder: row.membership_display_order == null ? null : Number(row.membership_display_order),
      },
      snapshot: {
        id: row.snapshot_id,
        sourceId: row.id,
        sha256: row.snapshot_sha256,
        mimeType: row.snapshot_mime_type,
        byteSize: Number(row.snapshot_byte_size),
        storagePath: row.snapshot_storage_path,
        capturedAt: row.snapshot_captured_at,
      },
      parseArtifact: row.parse_id ? {
        id: row.parse_id,
        contentSnapshotId: row.parse_content_snapshot_id,
        parserId: row.parse_parser_id,
        parserVersion: row.parse_parser_version,
        parserConfigHash: row.parse_parser_config_hash,
        status: row.parse_status,
        warnings: parseStringArrayJson(row.parse_warnings_json, "parse warnings"),
        semanticArtifactPath: row.parse_semantic_artifact_path || null,
        fidelity: row.parse_fidelity ?? "citation_grade",
        processingArtifactId: row.parse_processing_artifact_id ?? null,
        createdAt: row.parse_created_at,
        completedAt: row.parse_completed_at || null,
      } : null,
    }));
  }

  /**
   * source-file-watcher 的启动扫描：全部活跃 file 源 × 活跃 membership
   * （一行一条 membership，watcher 按 sourceId 聚合成多笔记本 watch 项）。
   * originMetadata.originalPath 缺失/非绝对路径的行跳过——这类源无法 refresh
   * （refreshFileSource 会抛 KNOWLEDGE_STORAGE_INVALID），watch 无意义。
   */
  listWatchableFileSources(): Array<{
    studioId: string;
    notebookId: string;
    sourceId: string;
    originalPath: string;
  }> {
    const rows = this.db.prepare(`
      SELECT s.id AS source_id, s.studio_id, s.origin_metadata_json, ns.notebook_id
      FROM sources s
      JOIN notebook_sources ns ON ns.source_id = s.id AND ns.removed_at IS NULL
      JOIN notebooks n ON n.id = ns.notebook_id AND n.deleted_at IS NULL
      WHERE s.source_type = 'file' AND s.deleted_at IS NULL
      ORDER BY s.id ASC, ns.notebook_id ASC
    `).all();
    const result: Array<{
      studioId: string;
      notebookId: string;
      sourceId: string;
      originalPath: string;
    }> = [];
    for (const row of rows as any[]) {
      const metadata = parseObjectJson(row.origin_metadata_json, "origin metadata");
      const originalPath = metadata.originalPath;
      if (typeof originalPath !== "string" || !path.isAbsolute(originalPath)) continue;
      result.push({
        studioId: row.studio_id,
        notebookId: row.notebook_id,
        sourceId: row.source_id,
        originalPath,
      });
    }
    return result;
  }

  getContentSnapshot(input: { studioId: unknown; snapshotId: unknown }): ContentSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const snapshotId = requiredString(input?.snapshotId, "snapshotId", 128);
    const snapshot = toSnapshot(this.db.prepare(`
      SELECT cs.*
      FROM content_snapshots cs
      JOIN sources s ON s.id = cs.source_id
      WHERE cs.id = ? AND s.studio_id = ?
    `).get(snapshotId, studioId));
    if (!snapshot) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Content snapshot not found");
    return snapshot;
  }

  createContentSnapshot(input: {
    studioId: unknown;
    sourceId: unknown;
    snapshotId?: unknown;
    sha256: unknown;
    mimeType: unknown;
    byteSize: unknown;
    storagePath: unknown;
  }): ContentSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeSource(studioId, sourceId);
    const snapshotSha = sha256(input.sha256);
    const existing = toSnapshot(this.db.prepare(`
      SELECT * FROM content_snapshots WHERE source_id = ? AND sha256 = ?
    `).get(sourceId, snapshotSha));
    if (existing) return existing;
    const snapshotId = input.snapshotId == null
      ? this.newId("snap")
      : requiredString(input.snapshotId, "snapshotId", 128);
    const mimeType = requiredString(input.mimeType, "mimeType", 255).toLowerCase();
    const snapshotBytes = byteSize(input.byteSize);
    const snapshotStoragePath = storagePath(input.storagePath);
    this.db.prepare(`
      INSERT INTO content_snapshots (
        id, source_id, sha256, mime_type, byte_size, storage_path, captured_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?)
    `).run(
      snapshotId,
      sourceId,
      snapshotSha,
      mimeType,
      snapshotBytes,
      snapshotStoragePath,
      this.now(),
    );
    return this.getContentSnapshot({ studioId, snapshotId });
  }

  getSource(input: { studioId: unknown; sourceId: unknown }): KnowledgeSource {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return this.activeSource(studioId, sourceId);
  }

  getLatestContentSnapshotForSource(input: { studioId: unknown; sourceId: unknown }): ContentSnapshot {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeSource(studioId, sourceId);
    const snapshot = toSnapshot(this.db.prepare(`
      SELECT * FROM content_snapshots
      WHERE source_id = ?
      ORDER BY captured_at DESC, id DESC
      LIMIT 1
    `).get(sourceId));
    if (!snapshot) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Content snapshot not found");
    return snapshot;
  }

  /**
   * 源的活跃 membership 数（§十八）：notebook_sources.removed_at IS NULL 且所属
   * 笔记本未删除（deleteNotebook 会置 removed_at，JOIN 是防御性双保险）。
   * 不做 studio 归属校验——orphan 判定按源全局数（共享源跨 studio 不存在，
   * membership 本就要求 notebook 同 studio，此处仅计数）。
   */
  countActiveSourceMemberships(input: { sourceId: unknown }): number {
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count
      FROM notebook_sources ns
      JOIN notebooks n ON n.id = ns.notebook_id AND n.deleted_at IS NULL
      WHERE ns.source_id = ? AND ns.removed_at IS NULL
    `).get(sourceId).count);
  }

  /** orphan 标记（§十八）：零活跃 membership 时置位；已置位则幂等返回。 */
  markSourceOrphaned(input: { studioId: unknown; sourceId: unknown }): KnowledgeSource {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    const source = this.activeSource(studioId, sourceId);
    if (source.orphanedAt == null) {
      this.db.prepare(`
        UPDATE sources SET orphaned_at = ? WHERE id = ? AND orphaned_at IS NULL
      `).run(this.now(), sourceId);
    }
    return this.getSource({ studioId, sourceId });
  }

  /** orphan 清除（复活）：重新获得活跃 membership 时调用；未置位则幂等无操作。 */
  clearSourceOrphan(input: { studioId: unknown; sourceId: unknown }): KnowledgeSource {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    const source = this.activeSource(studioId, sourceId);
    if (source.orphanedAt != null) {
      this.db.prepare(`
        UPDATE sources SET orphaned_at = NULL WHERE id = ? AND orphaned_at IS NOT NULL
      `).run(sourceId);
    }
    return this.getSource({ studioId, sourceId });
  }

  /**
   * 显式删除标记（§十九 delete wins）：置 deleted_at 后 activeSource 即拒绝
   * 该源的一切读取/入队（KNOWLEDGE_NOT_FOUND），并发 ensure 必须显式失败。
   * 仅活跃源可标记（重复删除 404）；行与派生物理清理由调用方随后执行。
   */
  markSourceDeleted(input: { studioId: unknown; sourceId: unknown }): KnowledgeSource {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    const source = this.activeSource(studioId, sourceId);
    const deletedAt = this.now();
    this.db.prepare(`
      UPDATE sources SET deleted_at = ?, orphaned_at = NULL WHERE id = ? AND deleted_at IS NULL
    `).run(deletedAt, sourceId);
    return { ...source, deletedAt, orphanedAt: null };
  }

  /**
   * orphan GC 候选（§十八）：未显式删除、orphaned_at 已过保留期的源。
   * 返回 studio 归属供日志/留痕；实际清理前的安全检查由调用方逐源复核。
   */
  listSourcesPastOrphanRetention(input: { cutoffIso: unknown }): Array<{
    studioId: string;
    sourceId: string;
    orphanedAt: string;
  }> {
    const cutoffIso = isoTimestampOrNull(input?.cutoffIso, "cutoffIso")!;
    return this.db.prepare(`
      SELECT id, studio_id, orphaned_at FROM sources
      WHERE deleted_at IS NULL AND orphaned_at IS NOT NULL AND orphaned_at <= ?
      ORDER BY orphaned_at ASC, id ASC
    `).all(cutoffIso).map((row: any) => ({
      studioId: row.studio_id,
      sourceId: row.id,
      orphanedAt: row.orphaned_at,
    }));
  }

  /** 活跃 KnowledgeTurnScope 对该源的冻结引用数（§十八 GC 前检查清单）。 */
  countActiveTurnScopesForSource(input: { sourceId: unknown }): number {
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count
      FROM knowledge_turn_scope_sources tss
      JOIN knowledge_turn_scopes ts ON ts.id = tss.scope_id
      WHERE tss.source_id = ? AND ts.status = 'active'
    `).get(sourceId).count);
  }

  /** 该源是否仍有活跃（queued/running/pending_embedding）摄入 job（§十八 GC 前检查）。 */
  hasActiveIngestionJobsForSource(input: { sourceId: unknown }): boolean {
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return !!this.db.prepare(`
      SELECT 1 FROM ingestion_jobs
      WHERE source_id = ? AND status IN ('queued', 'running', 'pending_embedding')
      LIMIT 1
    `).get(sourceId);
  }

  /**
   * 取消该源全部活跃 job（§十九 delete wins）：status → failed、置 cancelled_at、
   * error 记录取消原因（SQLite CHECK 不可加 'cancelled' 枚举，failed+cancelled_at
   * 是显式留痕的最低成本方案；requeueIngestionJob 拒绝 cancelled 行）。返回取消的
   * job id 列表；running job 的进程内 abort/等待收尾由摄入服务负责。
   */
  cancelSourceIngestionJobs(input: { sourceId: unknown; reason: unknown }): string[] {
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    const reason = requiredString(input.reason, "reason", 400);
    const now = this.now();
    return this.db.transaction(() => {
      const ids = (this.db.prepare(`
        SELECT id FROM ingestion_jobs
        WHERE source_id = ? AND status IN ('queued', 'running', 'pending_embedding')
        ORDER BY created_at ASC, id ASC
      `).all(sourceId) as any[]).map(row => row.id);
      if (ids.length === 0) return [];
      const cancel = this.db.prepare(`
        UPDATE ingestion_jobs
        SET status = 'failed', error = ?, cancelled_at = ?, updated_at = ?
        WHERE id = ? AND status IN ('queued', 'running', 'pending_embedding')
      `);
      for (const id of ids) cancel.run(reason, now, now, id);
      return ids;
    })();
  }

  /**
   * 源的物理清理（§十八/§十九，事实库部分）：单事务删除该源全部事实行及其派生引用。
   * 仅接受已显式删除（deleted_at）或由调用方保证过保留期 orphan 的源；活跃 turn scope
   * 的冻结行不删（调用方须先确认无活跃引用，否则 FK RESTRICT 会使本事务整体失败——
   * 宁可失败不可半删）。closed scope 的冻结行随源删除（历史轮已关闭，读取侧本就拒绝）。
   * 返回各表删除计数与文件清理所需的 artifact/snapshot id 清单。
   */
  purgeSourceRows(input: { studioId: unknown; sourceId: unknown }): {
    studioId: string;
    sourceId: string;
    jobs: number;
    turnScopeSources: number;
    citations: number;
    blocks: number;
    parseArtifacts: number;
    processingArtifacts: number;
    snapshots: number;
    memberships: number;
    parseArtifactIds: string[];
    contentSnapshotIds: string[];
  } {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    const row = this.db.prepare(`
      SELECT id FROM sources WHERE id = ? AND studio_id = ?
    `).get(sourceId, studioId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge source not found");
    return this.db.transaction(() => {
      const parseArtifactIds = (this.db.prepare(`
        SELECT pa.id AS id
        FROM parse_artifacts pa
        JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
        WHERE cs.source_id = ?
        ORDER BY pa.created_at ASC
      `).all(sourceId) as any[]).map(r => r.id);
      const contentSnapshotIds = (this.db.prepare(`
        SELECT id FROM content_snapshots WHERE source_id = ? ORDER BY captured_at ASC
      `).all(sourceId) as any[]).map(r => r.id);
      const count = (sql: string) => Number(this.db.prepare(sql).run(sourceId).changes);
      const jobs = count(`DELETE FROM ingestion_jobs WHERE source_id = ?`);
      const turnScopeSources = Number(this.db.prepare(`
        DELETE FROM knowledge_turn_scope_sources
        WHERE source_id = ?
          AND scope_id NOT IN (SELECT id FROM knowledge_turn_scopes WHERE status = 'active')
      `).run(sourceId).changes);
      const citations = Number(this.db.prepare(`
        DELETE FROM knowledge_citations WHERE parse_artifact_id IN (
          SELECT pa.id FROM parse_artifacts pa
          JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
          WHERE cs.source_id = ?
        )
      `).run(sourceId).changes);
      const blocks = Number(this.db.prepare(`
        DELETE FROM knowledge_blocks WHERE parse_artifact_id IN (
          SELECT pa.id FROM parse_artifacts pa
          JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
          WHERE cs.source_id = ?
        )
      `).run(sourceId).changes);
      const parseArtifacts = Number(this.db.prepare(`
        DELETE FROM parse_artifacts WHERE content_snapshot_id IN (
          SELECT id FROM content_snapshots WHERE source_id = ?
        )
      `).run(sourceId).changes);
      const processingArtifacts = Number(this.db.prepare(`
        DELETE FROM processing_artifacts WHERE content_snapshot_id IN (
          SELECT id FROM content_snapshots WHERE source_id = ?
        )
      `).run(sourceId).changes);
      const snapshots = count(`DELETE FROM content_snapshots WHERE source_id = ?`);
      const memberships = count(`DELETE FROM notebook_sources WHERE source_id = ?`);
      const sources = count(`DELETE FROM sources WHERE id = ?`);
      if (sources !== 1) {
        // FK RESTRICT 兜底：仍有未清理引用时事务整体回滚（本行仅显式留痕）。
        throw new KnowledgeError(
          "KNOWLEDGE_CONFLICT",
          "Knowledge source still has live references; purge aborted",
        );
      }
      return {
        studioId,
        sourceId,
        jobs,
        turnScopeSources,
        citations,
        blocks,
        parseArtifacts,
        processingArtifacts,
        snapshots,
        memberships,
        parseArtifactIds,
        contentSnapshotIds,
      };
    })();
  }

  /**
   * KnowledgeTurnScope 创建（schema v11，任务书 §二十/§四十三）：选中 notebooks
   * → 活跃 memberships → 每源当前最新 snapshot/artifact，同事务冻结落库。
   * 同事务把同会话其它 active scope 置为 closed（轮级 supersede：冻结集合只对
   * 创建它的轮负责；旧轮行保留供追溯，读取侧拒绝已关闭 scope）。
   * 笔记本不存在/已删除 → activeNotebook 抛 KNOWLEDGE_NOT_FOUND（显式拒绝）。
   */
  createTurnScope(input: {
    studioId: unknown;
    sessionPath: unknown;
    turnId?: unknown;
    notebookIds: unknown;
  }): KnowledgeTurnScope {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sessionPath = requiredString(input?.sessionPath, "sessionPath", 1024);
    const turnId = input?.turnId == null
      ? this.newId("turn")
      : requiredString(input.turnId, "turnId", 128);
    if (!Array.isArray(input?.notebookIds) || input.notebookIds.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "notebookIds must be a non-empty array");
    }
    const notebookIds = [...new Set(
      input.notebookIds.map(id => requiredString(id, "notebookId", 128)),
    )];
    const id = this.newId("kts");
    const now = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        UPDATE knowledge_turn_scopes SET status = 'closed'
        WHERE session_path = ? AND status = 'active'
      `).run(sessionPath);
      this.db.prepare(`
        INSERT INTO knowledge_turn_scopes (
          id, turn_id, session_path, studio_id, notebook_ids_json, status, created_at
        ) VALUES (?, ?, ?, ?, ?, 'active', ?)
      `).run(id, turnId, sessionPath, studioId, JSON.stringify(notebookIds), now);
      // 冻结集合：逐选中笔记本取活跃 membership（listNotebookSources 内部
      // activeNotebook 校验归属），每源记录最新 snapshot/artifact 与引用它的
      // 选中笔记本（按选择顺序）。同一源被多个选中笔记本引用时合并为一行。
      const frozen = new Map<string, {
        sourceId: string;
        contentSnapshotId: string;
        parseArtifactId: string | null;
        notebookIds: string[];
      }>();
      for (const notebookId of notebookIds) {
        for (const entry of this.listNotebookSources({ studioId, notebookId })) {
          const existing = frozen.get(entry.source.id);
          if (existing) {
            existing.notebookIds.push(notebookId);
            continue;
          }
          frozen.set(entry.source.id, {
            sourceId: entry.source.id,
            contentSnapshotId: entry.snapshot.id,
            parseArtifactId: entry.parseArtifact?.id ?? null,
            notebookIds: [notebookId],
          });
        }
      }
      const insert = this.db.prepare(`
        INSERT INTO knowledge_turn_scope_sources (
          scope_id, source_id, content_snapshot_id, parse_artifact_id, notebook_ids_json
        ) VALUES (?, ?, ?, ?, ?)
      `);
      for (const row of frozen.values()) {
        insert.run(id, row.sourceId, row.contentSnapshotId, row.parseArtifactId, JSON.stringify(row.notebookIds));
      }
    })();
    return this.getTurnScope({ scopeId: id })!;
  }

  /**
   * 读取 TurnScope（含冻结源集合）；不存在返回 null——读取方按自己的错误语义
   * 决定报错码（knowledge_read 映射为 KNOWLEDGE_SCOPE_VIOLATION）。
   */
  getTurnScope(input: { scopeId: unknown }): KnowledgeTurnScope | null {
    const scopeId = requiredString(input?.scopeId, "scopeId", 128);
    const row = this.db.prepare(`
      SELECT * FROM knowledge_turn_scopes WHERE id = ?
    `).get(scopeId);
    if (!row) return null;
    const sources = this.db.prepare(`
      SELECT * FROM knowledge_turn_scope_sources WHERE scope_id = ? ORDER BY source_id ASC
    `).all(scopeId).map(toTurnScopeSource);
    return toTurnScope(row, sources);
  }

  /** 关闭 TurnScope（幂等）；返回关闭后的 scope，不存在返回 null。 */
  closeTurnScope(input: { scopeId: unknown }): KnowledgeTurnScope | null {
    const scopeId = requiredString(input?.scopeId, "scopeId", 128);
    this.db.prepare(`
      UPDATE knowledge_turn_scopes SET status = 'closed'
      WHERE id = ? AND status = 'active'
    `).run(scopeId);
    return this.getTurnScope({ scopeId });
  }

  /**
   * 覆盖计划落库（schema v13，任务书 §二十九）：只持久化结构化分类结果，
   * 禁 CoT/原始模型输出。turnScopeId 非空时必须是存在的 TurnScope（外键
   * RESTRICT 之外再显式校验，给出稳定错误码而非裸 SQLite 约束错误）。
   */
  insertCoveragePlan(input: {
    turnScopeId?: unknown;
    question: unknown;
    plan: {
      intent: unknown;
      coverageMode: unknown;
      scopeLevel: unknown;
      subQueries?: unknown;
      confidence: unknown;
      matchedRuleIds: unknown;
      classifierUsed: unknown;
      degradeReason?: unknown;
    };
  }): KnowledgeCoveragePlanRecord {
    const plan = input?.plan;
    if (!plan || typeof plan !== "object") {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "plan must be an object");
    }
    const question = requiredString(input?.question, "question", 10_000);
    if (!isKnowledgeCoverageIntent(plan.intent)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "plan.intent is invalid");
    }
    if (!isKnowledgeCoverageMode(plan.coverageMode)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "plan.coverageMode is invalid");
    }
    if (!isKnowledgeCoverageScopeLevel(plan.scopeLevel)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "plan.scopeLevel is invalid");
    }
    if (!isKnowledgeCoverageClassifierUsed(plan.classifierUsed)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "plan.classifierUsed is invalid");
    }
    const confidence = Number(plan.confidence);
    if (!Number.isFinite(confidence) || confidence < 0 || confidence > 1) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "plan.confidence must be within 0 and 1");
    }
    const subQueries: string[] = Array.isArray(plan.subQueries) ? [...plan.subQueries] : [];
    if (subQueries.some(entry => typeof entry !== "string")) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "plan.subQueries must be an array of strings");
    }
    if (subQueries.length > 8 || subQueries.some(entry => entry.length > 500)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "plan.subQueries exceeds the entry limits");
    }
    const matchedRuleIds: string[] = Array.isArray(plan.matchedRuleIds) ? [...plan.matchedRuleIds] : [];
    if (matchedRuleIds.some(entry => typeof entry !== "string")) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "plan.matchedRuleIds must be an array of strings");
    }
    if (matchedRuleIds.length > 16 || matchedRuleIds.some(entry => !entry || entry.length > 128)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "plan.matchedRuleIds exceeds the entry limits");
    }
    let degradeReason: string | null = null;
    if (plan.degradeReason != null) {
      degradeReason = requiredString(plan.degradeReason, "plan.degradeReason", 500);
    }
    let turnScopeId: string | null = null;
    if (input?.turnScopeId != null) {
      turnScopeId = requiredString(input.turnScopeId, "turnScopeId", 128);
      if (!this.db.prepare(`SELECT 1 FROM knowledge_turn_scopes WHERE id = ?`).get(turnScopeId)) {
        throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "turnScopeId does not reference an existing turn scope");
      }
    }
    const id = this.newId("kcp");
    this.db.prepare(`
      INSERT INTO knowledge_coverage_plans (
        id, turn_scope_id, question, intent, coverage_mode, requires_completeness,
        scope_level, sub_queries_json, confidence, matched_rule_ids_json,
        classifier_used, degrade_reason, created_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    `).run(
      id,
      turnScopeId,
      question,
      plan.intent,
      plan.coverageMode,
      0, // requires_completeness：exhaustive 档移除后新行恒 0（遗留列，存量行保留原值）
      plan.scopeLevel,
      JSON.stringify(subQueries),
      confidence,
      JSON.stringify(matchedRuleIds),
      plan.classifierUsed,
      degradeReason,
      this.now(),
    );
    return toCoveragePlanRecord(this.db.prepare(`
      SELECT * FROM knowledge_coverage_plans WHERE id = ?
    `).get(id));
  }

  /**
   * 最近一条覆盖计划；给 turnScopeId 时限定该 scope（按 created_at DESC，
   * 并列按 rowid 定序），否则取全局最近一条。无行返回 null。
   */
  getLatestCoveragePlan(input?: { turnScopeId?: unknown }): KnowledgeCoveragePlanRecord | null {
    if (input?.turnScopeId != null) {
      const turnScopeId = requiredString(input.turnScopeId, "turnScopeId", 128);
      return toCoveragePlanRecord(this.db.prepare(`
        SELECT * FROM knowledge_coverage_plans WHERE turn_scope_id = ?
        ORDER BY created_at DESC, rowid DESC LIMIT 1
      `).get(turnScopeId));
    }
    return toCoveragePlanRecord(this.db.prepare(`
      SELECT * FROM knowledge_coverage_plans
      ORDER BY created_at DESC, rowid DESC LIMIT 1
    `).get());
  }

  /**
   * EvidenceManifest 写入（schema v15，任务书 §六十七）：manifest 头 + entries
   * 同事务落库。头字段（session/turn/notebookIds）从 TurnScope 行服务端复读——
   * 调用方只给 scopeId，不信任任何外部传入的轮身份。逐 entry 复核冻结集合：
   * sourceId 必须在 scope 冻结集合内，contentSnapshotId/parseArtifactId 必须与
   * 冻结行一致（不一致显式拒绝，绝不伪造身份）；coverageRunId 给定时必须指向
   * 存在的 coverage run。ordinal 必须连续 0-based。只存身份链，无正文。
   */
  insertEvidenceManifest(input: {
    turnScopeId: unknown;
    coverageMode?: unknown;
    executedCoverageMode?: unknown;
    coverageRunId?: unknown;
    coverageManifestHash?: unknown;
    entries: unknown;
  }): KnowledgeEvidenceManifest {
    const turnScopeId = requiredString(input?.turnScopeId, "turnScopeId", 128);
    const scopeRow = this.db.prepare(`
      SELECT * FROM knowledge_turn_scopes WHERE id = ?
    `).get(turnScopeId);
    if (!scopeRow) {
      throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "turnScopeId does not reference an existing turn scope");
    }
    const coverageMode = optionalCoverageMode(input?.coverageMode, "coverageMode");
    const executedCoverageMode = optionalCoverageMode(input?.executedCoverageMode, "executedCoverageMode");
    let coverageRunId: string | null = null;
    if (input?.coverageRunId != null) {
      coverageRunId = requiredString(input.coverageRunId, "coverageRunId", 128);
      if (!this.db.prepare(`SELECT 1 FROM coverage_runs WHERE id = ?`).get(coverageRunId)) {
        throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "coverageRunId does not reference an existing coverage run");
      }
    }
    let coverageManifestHash: string | null = null;
    if (input?.coverageManifestHash != null) {
      coverageManifestHash = sha256(input.coverageManifestHash);
    }
    if (!Array.isArray(input?.entries)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "entries must be an array");
    }
    // 冻结集合复核基准：scope 内每源的 (snapshot, artifact)。
    const frozenBySource = new Map<string, { contentSnapshotId: string; parseArtifactId: string | null }>();
    for (const row of this.db.prepare(`
      SELECT source_id, content_snapshot_id, parse_artifact_id
      FROM knowledge_turn_scope_sources WHERE scope_id = ?
    `).all(turnScopeId) as any[]) {
      frozenBySource.set(row.source_id, {
        contentSnapshotId: row.content_snapshot_id,
        parseArtifactId: row.parse_artifact_id || null,
      });
    }
    const normalizedEntries = input.entries.map((entry: any, index: number) => {
      if (!entry || typeof entry !== "object") {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "evidence manifest entries must be objects");
      }
      const ordinal = Number(entry.ordinal);
      if (!Number.isSafeInteger(ordinal) || ordinal !== index || ordinal < 0) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "evidence manifest entry ordinals must be contiguous");
      }
      const sourceId = requiredString(entry.sourceId, "entry sourceId", 128);
      const frozen = frozenBySource.get(sourceId);
      if (!frozen) {
        throw new KnowledgeError(
          "KNOWLEDGE_SCOPE_VIOLATION",
          `evidence manifest entry references source outside the frozen turn scope: ${sourceId}`,
        );
      }
      const contentSnapshotId = requiredString(entry.contentSnapshotId, "entry contentSnapshotId", 128);
      if (contentSnapshotId !== frozen.contentSnapshotId) {
        throw new KnowledgeError(
          "KNOWLEDGE_CONFLICT",
          `evidence manifest entry snapshot does not match the frozen scope snapshot of source ${sourceId}`,
        );
      }
      let parseArtifactId: string | null = null;
      if (entry.parseArtifactId != null) {
        parseArtifactId = requiredString(entry.parseArtifactId, "entry parseArtifactId", 128);
      }
      if (parseArtifactId !== frozen.parseArtifactId) {
        throw new KnowledgeError(
          "KNOWLEDGE_CONFLICT",
          `evidence manifest entry artifact does not match the frozen scope artifact of source ${sourceId}`,
        );
      }
      let chunkProfileHash: string | null = null;
      if (entry.chunkProfileHash != null) {
        chunkProfileHash = chunkProfileHashValue(entry.chunkProfileHash, "entry chunkProfileHash");
      }
      let chunkIndexVariantId: string | null = null;
      if (entry.chunkIndexVariantId != null) {
        chunkIndexVariantId = requiredString(entry.chunkIndexVariantId, "entry chunkIndexVariantId", 128);
      }
      const chunkIds = serializeStringArray(entry.chunkIds, "entry chunkIds");
      const neighborChunkIds = serializeStringArray(entry.neighborChunkIds, "entry neighborChunkIds");
      const vectorIndexVariantIds = serializeStringArray(entry.vectorIndexVariantIds, "entry vectorIndexVariantIds");
      const citationLabels = serializeStringArray(entry.citationLabels, "entry citationLabels");
      const blockSpans = serializeBlockSpans(entry.blockSpans, "entry blockSpans", chunkIds, neighborChunkIds);
      return {
        ordinal,
        sourceId,
        contentSnapshotId,
        parseArtifactId,
        chunkProfileHash,
        chunkIndexVariantId,
        vectorIndexVariantIds,
        chunkIds,
        neighborChunkIds,
        blockSpans,
        citationLabels,
      };
    });
    const id = this.newId("evman");
    const now = this.now();
    this.db.transaction(() => {
      this.db.prepare(`
        INSERT INTO evidence_manifests (
          id, turn_scope_id, session_path, turn_id, coverage_mode, executed_coverage_mode,
          notebook_ids_json, coverage_run_id, coverage_manifest_hash, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `).run(
        id,
        turnScopeId,
        scopeRow.session_path,
        scopeRow.turn_id,
        coverageMode,
        executedCoverageMode,
        scopeRow.notebook_ids_json,
        coverageRunId,
        coverageManifestHash,
        now,
      );
      const insertEntry = this.db.prepare(`
        INSERT INTO evidence_manifest_entries (
          manifest_id, ordinal, source_id, content_snapshot_id, parse_artifact_id,
          chunk_profile_hash, chunk_index_variant_id, vector_index_variant_ids_json,
          chunk_ids_json, neighbor_chunk_ids_json, block_spans_json, citation_labels_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `);
      for (const entry of normalizedEntries) {
        insertEntry.run(
          id,
          entry.ordinal,
          entry.sourceId,
          entry.contentSnapshotId,
          entry.parseArtifactId,
          entry.chunkProfileHash,
          entry.chunkIndexVariantId,
          entry.vectorIndexVariantIds,
          entry.chunkIds,
          entry.neighborChunkIds,
          entry.blockSpans,
          entry.citationLabels,
        );
      }
    })();
    return this.evidenceManifestById(id);
  }

  /** 该 TurnScope 最新一条 EvidenceManifest；无行返回 null。 */
  getEvidenceManifestByScope(input: { scopeId: unknown }): KnowledgeEvidenceManifest | null {
    const scopeId = requiredString(input?.scopeId, "scopeId", 128);
    const row = this.db.prepare(`
      SELECT id FROM evidence_manifests WHERE turn_scope_id = ?
      ORDER BY created_at DESC, rowid DESC LIMIT 1
    `).get(scopeId);
    if (!row) return null;
    return this.evidenceManifestById(row.id);
  }

  /** 该 turnId 最新一条 EvidenceManifest；无行返回 null。 */
  getEvidenceManifestByTurn(input: { turnId: unknown }): KnowledgeEvidenceManifest | null {
    const turnId = requiredString(input?.turnId, "turnId", 128);
    const row = this.db.prepare(`
      SELECT id FROM evidence_manifests WHERE turn_id = ?
      ORDER BY created_at DESC, rowid DESC LIMIT 1
    `).get(turnId);
    if (!row) return null;
    return this.evidenceManifestById(row.id);
  }

  /**
   * 引用该源的 EvidenceManifest 数（§六十七 GC 前检查）：
   * - 条目级：该源 chunk 进入某 manifest 的身份链（普通轮实际注入的证据）；
   * - run 关联：exhaustive 轮 manifest（coverage_run_id 非空）的 TurnScope 冻结过
   *   该源——全量扫描读过每个冻结单元，条目为空也是真引用。
   * 零证据普通轮（无条目、无 run）不保护未贡献证据的源。manifest 无 TTL 前
   * 全部保留：历史回答的证据版本追溯优先于物理清理。
   */
  countEvidenceManifestsForSource(input: { sourceId: unknown }): number {
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count FROM evidence_manifests em
      WHERE EXISTS (
          SELECT 1 FROM evidence_manifest_entries e
          WHERE e.manifest_id = em.id AND e.source_id = ?
        )
        OR (
          em.coverage_run_id IS NOT NULL
          AND EXISTS (
            SELECT 1 FROM knowledge_turn_scope_sources tss
            WHERE tss.scope_id = em.turn_scope_id AND tss.source_id = ?
          )
        )
    `).get(sourceId, sourceId).count);
  }

  /** 研究原文凭据、证据及完整性分母持续保护来源；检查关联的冻结范围也保留没有原文块的不可用源。 */
  hasResearchReferencesForSource(input: { sourceId: unknown }): boolean {
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    return !!this.db.prepare(`
      SELECT 1 WHERE
        EXISTS (SELECT 1 FROM knowledge_research_read_receipts WHERE source_id = ?)
        OR EXISTS (SELECT 1 FROM knowledge_evidence_items WHERE source_id = ?)
        OR EXISTS (SELECT 1 FROM knowledge_completeness_units WHERE source_id = ?)
        OR EXISTS (
          SELECT 1 FROM knowledge_completeness_checks checks
          JOIN knowledge_research_runs runs ON runs.id = checks.research_run_id
          JOIN knowledge_turn_scope_sources sources ON sources.scope_id = runs.turn_scope_id
          WHERE sources.source_id = ?
        )
    `).get(sourceId, sourceId, sourceId, sourceId);
  }

  private evidenceManifestById(id: string): KnowledgeEvidenceManifest {
    const row = this.db.prepare(`
      SELECT * FROM evidence_manifests WHERE id = ?
    `).get(id);
    const entries = this.db.prepare(`
      SELECT * FROM evidence_manifest_entries WHERE manifest_id = ? ORDER BY ordinal ASC
    `).all(id).map(toEvidenceManifestEntry);
    const manifest = toEvidenceManifest(row, entries);
    if (!manifest) throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Evidence manifest is corrupt");
    return manifest;
  }

  findParseArtifactByIdentity(input: {
    studioId: unknown;
    contentSnapshotId: unknown;
    parserId: unknown;
    parserVersion: unknown;
    parserConfigHash: unknown;
  }): KnowledgeParseArtifact | null {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const contentSnapshotId = requiredString(input?.contentSnapshotId, "contentSnapshotId", 128);
    const parserId = requiredString(input?.parserId, "parserId", 128);
    const parserVersion = requiredString(input?.parserVersion, "parserVersion", 64);
    const parserConfigHash = sha256(input?.parserConfigHash);
    return toParseArtifact(this.db.prepare(`
      SELECT pa.*
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE pa.content_snapshot_id = ?
        AND pa.parser_id = ?
        AND pa.parser_version = ?
        AND pa.parser_config_hash = ?
        AND s.studio_id = ?
    `).get(contentSnapshotId, parserId, parserVersion, parserConfigHash, studioId));
  }

  beginParseArtifact(input: {
    studioId: unknown;
    contentSnapshotId: unknown;
    parseArtifactId?: unknown;
    parserId: unknown;
    parserVersion: unknown;
    parserConfigHash: unknown;
  }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const contentSnapshotId = requiredString(input?.contentSnapshotId, "contentSnapshotId", 128);
    // 先经过 studio 归属检查，不能靠外键存在性代替授权。
    this.getContentSnapshot({ studioId, snapshotId: contentSnapshotId });
    const parserId = requiredString(input?.parserId, "parserId", 128);
    const parserVersion = requiredString(input?.parserVersion, "parserVersion", 64);
    const parserConfigHash = sha256(input?.parserConfigHash);
    const existing = this.findParseArtifactByIdentity({
      studioId,
      contentSnapshotId,
      parserId,
      parserVersion,
      parserConfigHash,
    });
    if (existing) {
      this.db.transaction(() => {
        this.db.prepare(`DELETE FROM knowledge_citations WHERE parse_artifact_id = ?`).run(existing.id);
        this.db.prepare(`DELETE FROM knowledge_blocks WHERE parse_artifact_id = ?`).run(existing.id);
        this.db.prepare(`
          UPDATE parse_artifacts
          SET status = 'parsing', warnings_json = '[]', semantic_artifact_path = NULL,
              fidelity = 'citation_grade', processing_artifact_id = NULL, completed_at = NULL
          WHERE id = ?
        `).run(existing.id);
      })();
      return this.getParseArtifact({ studioId, parseArtifactId: existing.id });
    }

    const id = input.parseArtifactId == null
      ? this.newId("parse")
      : requiredString(input.parseArtifactId, "parseArtifactId", 128);
    const now = this.now();
    this.db.prepare(`
      INSERT INTO parse_artifacts (
        id, content_snapshot_id, parser_id, parser_version, parser_config_hash,
        status, warnings_json, semantic_artifact_path, created_at, completed_at
      ) VALUES (?, ?, ?, ?, ?, 'parsing', '[]', NULL, ?, NULL)
    `).run(id, contentSnapshotId, parserId, parserVersion, parserConfigHash, now);
    return this.getParseArtifact({ studioId, parseArtifactId: id });
  }

  completeParseArtifact(input: {
    studioId: unknown;
    parseArtifactId: unknown;
    status: "ready" | "needs_ocr";
    warnings: unknown;
    semanticArtifactPath: unknown;
    blocks: KnowledgeBlockDraft[];
    fidelity?: "citation_grade" | "structural" | "semantic_only";
    processingArtifactId?: unknown;
  }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    this.getParseArtifact({ studioId, parseArtifactId });
    if (input.status !== "ready" && input.status !== "needs_ocr") {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Parse completion status is invalid");
    }
    const fidelity = input.fidelity ?? "citation_grade";
    if (!new Set(["citation_grade", "structural", "semantic_only"]).has(fidelity)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Parse artifact fidelity is invalid");
    }
    const processingArtifactId = input.processingArtifactId == null
      ? null
      : requiredString(input.processingArtifactId, "processingArtifactId", 128);
    if (processingArtifactId) {
      // 引用的 ProcessingArtifact 必须真实存在且属同一 snapshot，不伪造来源。
      this.getProcessingArtifact({ studioId, processingArtifactId });
    }
    const warningsJson = serializeStringArray(input.warnings, "warnings");
    const semanticArtifactPath = storagePath(input.semanticArtifactPath);
    if (!Array.isArray(input.blocks)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "blocks must be an array");
    }
    if (input.status === "needs_ocr" && input.blocks.length > 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "needs_ocr artifacts cannot contain blocks");
    }
    if (input.status === "ready" && input.blocks.length === 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "ready artifacts require at least one block");
    }

    const normalizedBlocks = input.blocks.map((block, index) => {
      if (block.ordinal !== index) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Block ordinals must be contiguous");
      }
      const text = requiredString(block.text, "block text", 2_000_000);
      const locatorType = block.locatorType;
      if (!new Set(["text", "markdown", "pdf", "html"]).has(locatorType)) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Block locator type is invalid");
      }
      return {
        id: this.newId("block"),
        ordinal: index,
        text,
        textSha256: crypto.createHash("sha256").update(text, "utf8").digest("hex"),
        locatorType,
        locatorJson: serializeObjectJson(block.locator, "block locator"),
      };
    });
    const completedAt = this.now();
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM knowledge_citations WHERE parse_artifact_id = ?`).run(parseArtifactId);
      this.db.prepare(`DELETE FROM knowledge_blocks WHERE parse_artifact_id = ?`).run(parseArtifactId);
      const insert = this.db.prepare(`
        INSERT INTO knowledge_blocks (
          id, parse_artifact_id, ordinal, text, text_sha256, locator_type, locator_payload_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      `);
      for (const block of normalizedBlocks) {
        insert.run(
          block.id,
          parseArtifactId,
          block.ordinal,
          block.text,
          block.textSha256,
          block.locatorType,
          block.locatorJson,
        );
      }
      this.db.prepare(`
        UPDATE parse_artifacts
        SET status = ?, warnings_json = ?, semantic_artifact_path = ?, fidelity = ?, processing_artifact_id = ?, completed_at = ?
        WHERE id = ?
      `).run(input.status, warningsJson, semanticArtifactPath, fidelity, processingArtifactId, completedAt, parseArtifactId);
    })();
    return this.getParseArtifact({ studioId, parseArtifactId });
  }

  failParseArtifact(input: {
    studioId: unknown;
    parseArtifactId: unknown;
    warnings?: unknown;
  }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    this.getParseArtifact({ studioId, parseArtifactId });
    const warningsJson = serializeStringArray(input.warnings ?? ["parse_failed"], "warnings");
    const completedAt = this.now();
    this.db.transaction(() => {
      this.db.prepare(`DELETE FROM knowledge_citations WHERE parse_artifact_id = ?`).run(parseArtifactId);
      this.db.prepare(`DELETE FROM knowledge_blocks WHERE parse_artifact_id = ?`).run(parseArtifactId);
      this.db.prepare(`
        UPDATE parse_artifacts
        SET status = 'failed', warnings_json = ?, semantic_artifact_path = NULL, completed_at = ?
        WHERE id = ?
      `).run(warningsJson, completedAt, parseArtifactId);
    })();
    return this.getParseArtifact({ studioId, parseArtifactId });
  }

  getProcessingArtifact(input: { studioId: unknown; processingArtifactId: unknown }): KnowledgeProcessingArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const processingArtifactId = requiredString(input?.processingArtifactId, "processingArtifactId", 128);
    const artifact = toProcessingArtifact(this.db.prepare(`
      SELECT pa.*
      FROM processing_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE pa.id = ? AND s.studio_id = ?
    `).get(processingArtifactId, studioId));
    if (!artifact) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Processing artifact not found");
    return artifact;
  }

  findProcessingArtifactByIdentity(input: {
    studioId: unknown;
    contentSnapshotId: unknown;
    processorId: unknown;
    processorVersion: unknown;
    processorConfigHash: unknown;
  }): KnowledgeProcessingArtifact | null {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const contentSnapshotId = requiredString(input?.contentSnapshotId, "contentSnapshotId", 128);
    const processorId = requiredString(input?.processorId, "processorId", 128);
    const processorVersion = requiredString(input?.processorVersion, "processorVersion", 64);
    const processorConfigHash = sha256(input?.processorConfigHash);
    return toProcessingArtifact(this.db.prepare(`
      SELECT pa.*
      FROM processing_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE pa.content_snapshot_id = ?
        AND pa.processor_id = ?
        AND pa.processor_version = ?
        AND pa.processor_config_hash = ?
        AND s.studio_id = ?
    `).get(contentSnapshotId, processorId, processorVersion, processorConfigHash, studioId));
  }

  beginProcessingArtifact(input: {
    studioId: unknown;
    contentSnapshotId: unknown;
    processingArtifactId?: unknown;
    processorId: unknown;
    processorVersion: unknown;
    processorConfigHash: unknown;
  }): KnowledgeProcessingArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const contentSnapshotId = requiredString(input?.contentSnapshotId, "contentSnapshotId", 128);
    // 先经过 studio 归属检查，不能靠外键存在性代替授权。
    this.getContentSnapshot({ studioId, snapshotId: contentSnapshotId });
    const processorId = requiredString(input?.processorId, "processorId", 128);
    const processorVersion = requiredString(input?.processorVersion, "processorVersion", 64);
    const processorConfigHash = sha256(input?.processorConfigHash);
    const existing = this.findProcessingArtifactByIdentity({
      studioId,
      contentSnapshotId,
      processorId,
      processorVersion,
      processorConfigHash,
    });
    if (existing) {
      this.db.prepare(`
        UPDATE processing_artifacts
        SET status = 'processing', fidelity = NULL, output_mime = NULL, output_path = NULL,
            locator_map_json = '{}', warnings_json = '[]', completed_at = NULL
        WHERE id = ?
      `).run(existing.id);
      return this.getProcessingArtifact({ studioId, processingArtifactId: existing.id });
    }

    const id = input.processingArtifactId == null
      ? this.newId("proc")
      : requiredString(input.processingArtifactId, "processingArtifactId", 128);
    const now = this.now();
    this.db.prepare(`
      INSERT INTO processing_artifacts (
        id, content_snapshot_id, processor_id, processor_version, processor_config_hash,
        status, fidelity, output_mime, output_path, locator_map_json, warnings_json,
        created_at, completed_at
      ) VALUES (?, ?, ?, ?, ?, 'processing', NULL, NULL, NULL, '{}', '[]', ?, NULL)
    `).run(id, contentSnapshotId, processorId, processorVersion, processorConfigHash, now);
    return this.getProcessingArtifact({ studioId, processingArtifactId: id });
  }

  completeProcessingArtifact(input: {
    studioId: unknown;
    processingArtifactId: unknown;
    fidelity: unknown;
    outputMime: unknown;
    outputPath: unknown;
    locatorMap: unknown;
    warnings: unknown;
  }): KnowledgeProcessingArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const processingArtifactId = requiredString(input?.processingArtifactId, "processingArtifactId", 128);
    this.getProcessingArtifact({ studioId, processingArtifactId });
    const fidelity = requiredString(input?.fidelity, "fidelity", 32);
    if (!new Set(["citation_grade", "structural", "semantic_only"]).has(fidelity)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Processing artifact fidelity is invalid");
    }
    const outputMime = requiredString(input?.outputMime, "outputMime", 128);
    const outputPath = storagePath(input?.outputPath);
    const locatorMapJson = serializeObjectJson(input?.locatorMap ?? {}, "locator map");
    const warningsJson = serializeStringArray(input.warnings ?? [], "warnings");
    const completedAt = this.now();
    this.db.prepare(`
      UPDATE processing_artifacts
      SET status = 'ready', fidelity = ?, output_mime = ?, output_path = ?,
          locator_map_json = ?, warnings_json = ?, completed_at = ?
      WHERE id = ?
    `).run(fidelity, outputMime, outputPath, locatorMapJson, warningsJson, completedAt, processingArtifactId);
    return this.getProcessingArtifact({ studioId, processingArtifactId });
  }

  failProcessingArtifact(input: {
    studioId: unknown;
    processingArtifactId: unknown;
    warnings?: unknown;
  }): KnowledgeProcessingArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const processingArtifactId = requiredString(input?.processingArtifactId, "processingArtifactId", 128);
    this.getProcessingArtifact({ studioId, processingArtifactId });
    const warningsJson = serializeStringArray(input.warnings ?? ["processing_failed"], "warnings");
    const completedAt = this.now();
    this.db.prepare(`
      UPDATE processing_artifacts
      SET status = 'failed', fidelity = NULL, output_mime = NULL, output_path = NULL,
          locator_map_json = '{}', warnings_json = ?, completed_at = ?
      WHERE id = ?
    `).run(warningsJson, completedAt, processingArtifactId);
    return this.getProcessingArtifact({ studioId, processingArtifactId });
  }

  /**
   * §六十九：目录组织路径属于 Membership。仅更新活跃 membership；
   * 三字段均可置 null（无目录语境），displayOrder 需为非负整数。
   */
  updateMembershipPath(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId: unknown;
    relativePath?: unknown;
    folderNode?: unknown;
    displayOrder?: unknown;
  }): NotebookSourceMembership {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeNotebook(studioId, notebookId);
    this.activeSource(studioId, sourceId);
    const membership = this.getMembership(notebookId, sourceId);
    if (membership.removedAt !== null) {
      throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Notebook source membership not found");
    }
    const relativePath = input.relativePath == null
      ? null
      : requiredString(input.relativePath, "relativePath", 1024);
    const folderNode = input.folderNode == null
      ? null
      : requiredString(input.folderNode, "folderNode", 1024);
    let displayOrder: number | null = null;
    if (input.displayOrder != null) {
      if (!Number.isSafeInteger(input.displayOrder) || Number(input.displayOrder) < 0) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "displayOrder must be a non-negative integer");
      }
      displayOrder = Number(input.displayOrder);
    }
    this.db.prepare(`
      UPDATE notebook_sources
      SET relative_path = ?, folder_node = ?, display_order = ?
      WHERE notebook_id = ? AND source_id = ? AND removed_at IS NULL
    `).run(relativePath, folderNode, displayOrder, notebookId, sourceId);
    return this.getMembership(notebookId, sourceId);
  }

  /** 目录导入去重（§六十九）：按内容 sha 查同 studio 未删除 Source，命中即复用。 */
  findSourceIdByContentSha(input: { studioId: unknown; sha256: unknown }): string | null {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const contentSha = sha256(input?.sha256);
    const row = this.db.prepare(`
      SELECT s.id AS id
      FROM sources s
      JOIN content_snapshots cs ON cs.source_id = s.id
      WHERE cs.sha256 = ? AND s.studio_id = ? AND s.deleted_at IS NULL
      ORDER BY cs.captured_at DESC
      LIMIT 1
    `).get(contentSha, studioId) as any;
    return row ? String(row.id) : null;
  }

  getParseArtifact(input: { studioId: unknown; parseArtifactId: unknown }): KnowledgeParseArtifact {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    const artifact = toParseArtifact(this.db.prepare(`
      SELECT pa.*
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE pa.id = ? AND s.studio_id = ?
    `).get(parseArtifactId, studioId));
    if (!artifact) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Parse artifact not found");
    if (!PARSE_STATUSES.has(artifact.status)) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Parse artifact status is invalid");
    }
    return artifact;
  }

  /** 目录工具只需数量与定位类型，不读取正文或重建覆盖单元。 */
  getArtifactBlockMetadata(input: { studioId: unknown; parseArtifactId: unknown }): { blockCount: number; locatorTypes: string[] } {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    this.getParseArtifact({ studioId, parseArtifactId });
    const rows = this.db.prepare(`SELECT locator_type, COUNT(*) AS count FROM knowledge_blocks
      WHERE parse_artifact_id = ? GROUP BY locator_type`).all(parseArtifactId);
    return { blockCount: rows.reduce((sum: number, row: any) => sum + Number(row.count), 0),
      locatorTypes: rows.map((row: any) => row.locator_type) };
  }

  listArtifactBlocks(input: { studioId: unknown; parseArtifactId: unknown }): KnowledgeBlock[] {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    this.getParseArtifact({ studioId, parseArtifactId });
    return this.db.prepare(`
      SELECT * FROM knowledge_blocks
      WHERE parse_artifact_id = ?
      ORDER BY ordinal ASC
    `).all(parseArtifactId).map(toBlock);
  }

  /** 按产物批量读取命中块；归属过滤在同一条 SQL 中完成，避免逐命中查询。 */
  getArtifactBlocksByIds(input: {
    studioId: unknown;
    parseArtifactId: unknown;
    blockIds: unknown[];
  }): KnowledgeBlock[] {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    if (!Array.isArray(input?.blockIds)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "blockIds must be an array");
    }
    const blockIds = [...new Set(input.blockIds.map(id => requiredString(id, "blockId", 128)))];
    if (blockIds.length === 0) return [];
    return this.db.prepare(`
      SELECT b.* FROM knowledge_blocks b
      JOIN parse_artifacts pa ON pa.id = b.parse_artifact_id
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE s.studio_id = ? AND b.parse_artifact_id = ?
        AND b.id IN (SELECT value FROM json_each(?))
      ORDER BY b.ordinal ASC, b.id ASC
    `).all(studioId, parseArtifactId, JSON.stringify(blockIds)).map(toBlock);
  }

  createCitation(input: {
    studioId: unknown;
    parseArtifactId: unknown;
    blockId: unknown;
    startOffset: unknown;
    endOffset: unknown;
  }): KnowledgeCitation {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const parseArtifactId = requiredString(input?.parseArtifactId, "parseArtifactId", 128);
    const artifact = this.getParseArtifact({ studioId, parseArtifactId });
    if (artifact.status !== "ready") {
      throw new KnowledgeError("KNOWLEDGE_PARSE_NOT_READY", "Parse artifact is not ready for citation");
    }
    const blockId = requiredString(input?.blockId, "blockId", 128);
    const block = toBlock(this.db.prepare(`
      SELECT * FROM knowledge_blocks WHERE id = ? AND parse_artifact_id = ?
    `).get(blockId, parseArtifactId));
    if (!block) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge block not found");
    if (!Number.isSafeInteger(input.startOffset) || !Number.isSafeInteger(input.endOffset)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Citation offsets must be integers");
    }
    const startOffset = Number(input.startOffset);
    const endOffset = Number(input.endOffset);
    if (startOffset < 0 || endOffset <= startOffset || endOffset > block.text.length) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Citation offsets are outside the block");
    }
    const canonicalText = block.text.slice(startOffset, endOffset);
    const id = this.newId("cite");
    const createdAt = this.now();
    const canonicalTextSha256 = crypto.createHash("sha256").update(canonicalText, "utf8").digest("hex");
    this.db.prepare(`
      INSERT INTO knowledge_citations (
        id, parse_artifact_id, block_id, start_offset, end_offset,
        canonical_text, canonical_text_sha256, created_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    `).run(
      id,
      parseArtifactId,
      blockId,
      startOffset,
      endOffset,
      canonicalText,
      canonicalTextSha256,
      createdAt,
    );
    return toCitation(this.db.prepare(`SELECT * FROM knowledge_citations WHERE id = ?`).get(id))!;
  }

  resolveCitation(input: { studioId: unknown; citationId: unknown }): ResolvedKnowledgeCitation {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const citationId = requiredString(input?.citationId, "citationId", 128);
    const row = this.db.prepare(`
      SELECT
        kc.id AS citation_id,
        kc.parse_artifact_id AS citation_parse_artifact_id,
        kc.block_id AS citation_block_id,
        kc.start_offset AS citation_start_offset,
        kc.end_offset AS citation_end_offset,
        kc.canonical_text AS citation_canonical_text,
        kc.canonical_text_sha256 AS citation_canonical_text_sha256,
        kc.created_at AS citation_created_at,
        kb.id AS block_id,
        kb.parse_artifact_id AS block_parse_artifact_id,
        kb.ordinal AS block_ordinal,
        kb.text AS block_text,
        kb.text_sha256 AS block_text_sha256,
        kb.locator_type AS block_locator_type,
        kb.locator_payload_json AS block_locator_payload_json,
        pa.id AS artifact_id,
        pa.content_snapshot_id AS artifact_content_snapshot_id,
        pa.parser_id AS artifact_parser_id,
        pa.parser_version AS artifact_parser_version,
        pa.parser_config_hash AS artifact_parser_config_hash,
        pa.status AS artifact_status,
        pa.warnings_json AS artifact_warnings_json,
        pa.semantic_artifact_path AS artifact_semantic_artifact_path,
        pa.fidelity AS artifact_fidelity,
        pa.processing_artifact_id AS artifact_processing_artifact_id,
        pa.created_at AS artifact_created_at,
        pa.completed_at AS artifact_completed_at,
        cs.id AS snapshot_id,
        cs.source_id AS snapshot_source_id,
        cs.sha256 AS snapshot_sha256,
        cs.mime_type AS snapshot_mime_type,
        cs.byte_size AS snapshot_byte_size,
        cs.storage_path AS snapshot_storage_path,
        cs.captured_at AS snapshot_captured_at,
        s.id AS source_id,
        s.studio_id AS source_studio_id,
        s.source_type AS source_type,
        s.display_name AS source_display_name,
        s.origin_metadata_json AS source_origin_metadata_json,
        s.created_at AS source_created_at,
        s.deleted_at AS source_deleted_at,
        s.orphaned_at AS source_orphaned_at
      FROM knowledge_citations kc
      JOIN knowledge_blocks kb ON kb.id = kc.block_id AND kb.parse_artifact_id = kc.parse_artifact_id
      JOIN parse_artifacts pa ON pa.id = kc.parse_artifact_id
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      JOIN sources s ON s.id = cs.source_id
      WHERE kc.id = ? AND s.studio_id = ?
    `).get(citationId, studioId);
    if (!row) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge citation not found");

    return {
      citation: {
        id: row.citation_id,
        parseArtifactId: row.citation_parse_artifact_id,
        blockId: row.citation_block_id,
        startOffset: Number(row.citation_start_offset),
        endOffset: Number(row.citation_end_offset),
        canonicalText: row.citation_canonical_text,
        canonicalTextSha256: row.citation_canonical_text_sha256,
        createdAt: row.citation_created_at,
      },
      block: {
        id: row.block_id,
        parseArtifactId: row.block_parse_artifact_id,
        ordinal: Number(row.block_ordinal),
        text: row.block_text,
        textSha256: row.block_text_sha256,
        locatorType: row.block_locator_type,
        locator: parseObjectJson(row.block_locator_payload_json, "block locator"),
      },
      artifact: {
        id: row.artifact_id,
        contentSnapshotId: row.artifact_content_snapshot_id,
        parserId: row.artifact_parser_id,
        parserVersion: row.artifact_parser_version,
        parserConfigHash: row.artifact_parser_config_hash,
        status: row.artifact_status,
        warnings: parseStringArrayJson(row.artifact_warnings_json, "parse warnings"),
        semanticArtifactPath: row.artifact_semantic_artifact_path || null,
        fidelity: row.artifact_fidelity ?? "citation_grade",
        processingArtifactId: row.artifact_processing_artifact_id ?? null,
        createdAt: row.artifact_created_at,
        completedAt: row.artifact_completed_at || null,
      },
      snapshot: {
        id: row.snapshot_id,
        sourceId: row.snapshot_source_id,
        sha256: row.snapshot_sha256,
        mimeType: row.snapshot_mime_type,
        byteSize: Number(row.snapshot_byte_size),
        storagePath: row.snapshot_storage_path,
        capturedAt: row.snapshot_captured_at,
      },
      source: {
        id: row.source_id,
        studioId: row.source_studio_id,
        sourceType: row.source_type,
        displayName: row.source_display_name,
        originMetadata: parseObjectJson(row.source_origin_metadata_json, "origin metadata"),
        createdAt: row.source_created_at,
        deletedAt: row.source_deleted_at || null,
        orphanedAt: row.source_orphaned_at || null,
      },
    };
  }

  countContentSnapshots(input: { studioId: unknown; sourceId: unknown }): number {
    const source = this.getSource(input);
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count FROM content_snapshots WHERE source_id = ?
    `).get(source.id).count);
  }

  countParseArtifacts(input: { studioId: unknown; sourceId: unknown }): number {
    const source = this.getSource(input);
    return Number(this.db.prepare(`
      SELECT COUNT(*) AS count
      FROM parse_artifacts pa
      JOIN content_snapshots cs ON cs.id = pa.content_snapshot_id
      WHERE cs.source_id = ?
    `).get(source.id).count);
  }

  /**
   * 入队一个摄入 job（phase 链 parse → chunk → fts_index → embed → done）。
   * 同一 notebook+source 已有活跃 job（queued/running/pending_embedding）时直接去重返回，
   * 不重复排队；done/failed 的历史 job 不挡新的摄入（配置变更重建语义）。
   * chunkerConfigId 记录触发摄入的笔记本分块配置——一源多笔记本配置冲突时以触发方为准。
   */
  enqueueIngestionJob(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId: unknown;
    chunkerConfigId: unknown;
    artifactId?: unknown;
  }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const notebookId = requiredString(input?.notebookId, "notebookId", 128);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeNotebook(studioId, notebookId);
    this.activeSource(studioId, sourceId);
    const membership = toMembership(this.db.prepare(`
      SELECT * FROM notebook_sources
      WHERE notebook_id = ? AND source_id = ? AND removed_at IS NULL
    `).get(notebookId, sourceId));
    if (!membership) {
      throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge source is not in this Notebook");
    }
    const configId = chunkerConfigId(input.chunkerConfigId);
    const artifactId = input.artifactId == null
      ? null
      : this.getParseArtifact({ studioId, parseArtifactId: input.artifactId }).id;

    const existing = toIngestionJob(this.db.prepare(`
      SELECT * FROM ingestion_jobs
      WHERE notebook_id = ? AND source_id = ?
        AND status IN ('queued', 'running', 'pending_embedding')
      ORDER BY created_at DESC, id DESC
      LIMIT 1
    `).get(notebookId, sourceId));
    if (existing) return existing;

    const id = this.newId("ingjob");
    const now = this.now();
    try {
      this.db.prepare(`
        INSERT INTO ingestion_jobs (
          id, notebook_id, source_id, artifact_id, phase, status,
          attempt, retry_after, error, chunker_config_id, created_at, updated_at
        ) VALUES (?, ?, ?, ?, 'parse', 'queued', 0, NULL, NULL, ?, ?, ?)
      `).run(id, notebookId, sourceId, artifactId, configId, now, now);
    } catch (error: any) {
      // v12 部分唯一索引的 DB 级兜底：并发路径下同 (notebook, source) 活跃行已被
      // 另一路径插入时收敛为「返回既有活跃 job」（ensure 语义），其余错误照抛。
      const active = toIngestionJob(this.db.prepare(`
        SELECT * FROM ingestion_jobs
        WHERE notebook_id = ? AND source_id = ?
          AND status IN ('queued', 'running', 'pending_embedding')
        ORDER BY created_at DESC, id DESC
        LIMIT 1
      `).get(notebookId, sourceId));
      if (!active || !String(error?.code || "").startsWith("SQLITE_CONSTRAINT")) throw error;
      return active;
    }
    return this.getIngestionJob({ studioId, jobId: id });
  }

  getIngestionJob(input: { studioId: unknown; jobId: unknown }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const jobId = requiredString(input?.jobId, "jobId", 128);
    const job = toIngestionJob(this.db.prepare(`
      SELECT j.*
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE j.id = ? AND nb.studio_id = ?
    `).get(jobId, studioId));
    if (!job) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Ingestion job not found");
    return job;
  }

  /**
   * 原子认领下一个到期 queued job（retry_after 未到的跳过），置 running。
   * 不做 key 冲突挑选的「无条件认领」版：worker 池的兼容性认领走
   * listClaimableIngestionJobs + claimIngestionJobById（见 ingestion-service），
   * 本方法保留给测试与单点直取。同步驱动下单事务即原子。
   */
  claimNextIngestionJob(): IngestionJob | null {
    const now = this.now();
    return this.db.transaction(() => {
      const row = this.db.prepare(`
        SELECT * FROM ingestion_jobs
        WHERE status = 'queued' AND (retry_after IS NULL OR retry_after <= ?)
        ORDER BY created_at ASC, id ASC
        LIMIT 1
      `).get(now);
      if (!row) return null;
      this.db.prepare(`
        UPDATE ingestion_jobs SET status = 'running', updated_at = ?
        WHERE id = ?
      `).run(now, row.id);
      return toIngestionJob(this.db.prepare(`SELECT * FROM ingestion_jobs WHERE id = ?`).get(row.id));
    })();
  }

  /**
   * 到期 queued job 候选清单（Phase 5 §十六 keyed locking）：worker 池按 key 冲突
   * 挑选可并行的 job，选定后经 claimIngestionJobById 原子认领（两个 worker 同抢
   * 同一条时后到者得到 null，重扫即可）。附 studio 归属（key 计算需要解析笔记本配置）。
   */
  listClaimableIngestionJobs(input: { limit?: unknown }): Array<IngestionJob & { studioId: string }> {
    const limit = optionalIntegerInRange(input?.limit, "limit", 1, 200) ?? 32;
    return this.db.prepare(`
      SELECT j.*, nb.studio_id AS studio_id
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE j.status = 'queued' AND (j.retry_after IS NULL OR j.retry_after <= ?)
      ORDER BY j.created_at ASC, j.id ASC
      LIMIT ?
    `).all(this.now(), limit).map((row: any) => ({
      ...(toIngestionJob(row) as IngestionJob),
      studioId: row.studio_id,
    }));
  }

  /** 按 id 原子认领（worker 池用）：仅当仍为到期 queued 时置 running，否则返回 null。 */
  claimIngestionJobById(input: { jobId: unknown }): IngestionJob | null {
    const jobId = requiredString(input?.jobId, "jobId", 128);
    const now = this.now();
    return this.db.transaction(() => {
      const row = this.db.prepare(`
        SELECT * FROM ingestion_jobs WHERE id = ?
      `).get(jobId);
      if (!row || row.status !== "queued" || (row.retry_after != null && row.retry_after > now)) {
        return null;
      }
      const result = this.db.prepare(`
        UPDATE ingestion_jobs SET status = 'running', updated_at = ?
        WHERE id = ? AND status = 'queued'
      `).run(now, jobId);
      if (Number(result.changes) !== 1) return null;
      return toIngestionJob(this.db.prepare(`SELECT * FROM ingestion_jobs WHERE id = ?`).get(jobId));
    })();
  }

  private runningIngestionJob(studioId: unknown, jobId: unknown): IngestionJob {
    const job = this.getIngestionJob({ studioId, jobId });
    if (job.status !== "running") {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Ingestion job is not running");
    }
    return job;
  }

  /** 推进到下一个待执行 phase；parse 完成时顺带绑定产生的 parse artifact。 */
  updateIngestionJobPhase(input: {
    studioId: unknown;
    jobId: unknown;
    phase: unknown;
    artifactId?: unknown;
  }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    const phase = input.phase;
    if (typeof phase !== "string" || !INGESTION_PHASES.has(phase as IngestionPhase) || phase === "done") {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Ingestion phase is invalid");
    }
    const artifactId = input.artifactId == null
      ? job.artifactId
      : this.getParseArtifact({ studioId, parseArtifactId: input.artifactId }).id;
    this.db.prepare(`
      UPDATE ingestion_jobs SET phase = ?, artifact_id = ?, updated_at = ?
      WHERE id = ?
    `).run(phase, artifactId, this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /** 摄入 worker 的嵌入进度落库：仅 running 可写；total 首次给出时初始化（NULL → 已知值）。 */
  updateIngestionJobProgress(input: {
    studioId: unknown;
    jobId: unknown;
    done: unknown;
    total?: unknown;
  }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    const done = input?.done;
    if (!Number.isSafeInteger(done) || Number(done) < 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Ingestion progress done must be a non-negative integer");
    }
    if (input?.total != null && (!Number.isSafeInteger(input.total) || Number(input.total) < 0)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Ingestion progress total must be a non-negative integer");
    }
    const total = input?.total == null ? job.progressTotal : Number(input.total);
    if (total != null && Number(done) > total) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Ingestion progress done must not exceed total");
    }
    this.db.prepare(`
      UPDATE ingestion_jobs
      SET progress_done = ?, progress_total = ?, updated_at = ?
      WHERE id = ?
    `).run(Number(done), total, this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  completeIngestionJob(input: { studioId: unknown; jobId: unknown }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    this.db.prepare(`
      UPDATE ingestion_jobs
      SET phase = 'done', status = 'done', error = NULL, retry_after = NULL,
        progress_done = COALESCE(progress_total, progress_done), updated_at = ?
      WHERE id = ?
    `).run(this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /** 显式终态（非失败）：FTS 已可查、嵌入模型未配置，等模型就绪信号补跑。 */
  markIngestionJobPendingEmbedding(input: { studioId: unknown; jobId: unknown }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    this.db.prepare(`
      UPDATE ingestion_jobs SET phase = 'embed', status = 'pending_embedding', updated_at = ?
      WHERE id = ?
    `).run(this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /**
   * 记录一次失败：attempt + 1。进度保留（Phase 3 起 embed 相位有批级 checkpoint，
   * progress 反映真实已落库向量数，保留供诊断；UI 不回退因为续嵌只增不减——
   * 指纹漂移重建是唯一回退场景，由 embedding_stats.resetStaleVectors 显式留痕）。
   * 带 retryAfter → 回到 queued 等退避到期；不带 → 标 failed
   * （attempt 上限判定在服务层，store 只做状态机）。
   */
  failIngestionJob(input: {
    studioId: unknown;
    jobId: unknown;
    error: unknown;
    retryAfter?: unknown;
  }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    const error = requiredString(input.error, "error", 512);
    const retryAfter = isoTimestampOrNull(input.retryAfter, "retryAfter");
    this.db.prepare(`
      UPDATE ingestion_jobs
      SET status = ?, attempt = attempt + 1, error = ?, retry_after = ?, updated_at = ?
      WHERE id = ?
    `).run(retryAfter ? "queued" : "failed", error, retryAfter, this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /** UI 手动重试：failed → queued，attempt 归零、进度重置；phase 保留，从失败的 phase 续跑（各步幂等）。
   * cancelled 行（deleteSource 显式取消，cancelled_at 非空）拒绝重试——delete wins，
   * 被删源的 job 不得复活（源行已物理清理，重试只会撞 NOT_FOUND）。 */
  requeueIngestionJob(input: { studioId: unknown; jobId: unknown }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.getIngestionJob({ studioId, jobId: input?.jobId });
    if (job.status !== "failed") {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Only failed ingestion jobs can be retried");
    }
    if (job.cancelledAt != null) {
      throw new KnowledgeError(
        "KNOWLEDGE_CONFLICT",
        "Cancelled ingestion jobs cannot be retried (source deleted)",
      );
    }
    this.db.prepare(`
      UPDATE ingestion_jobs
      SET status = 'queued', attempt = 0, error = NULL, retry_after = NULL,
        progress_done = 0, progress_total = NULL, updated_at = ?
      WHERE id = ?
    `).run(this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /** 模型就绪信号：全部 pending_embedding 一次性置回 queued 补跑嵌入。返回置回数量。 */
  requeuePendingEmbeddingIngestionJobs(): number {
    const result = this.db.prepare(`
      UPDATE ingestion_jobs SET status = 'queued', updated_at = ?
      WHERE status = 'pending_embedding'
    `).run(this.now());
    return Number(result.changes);
  }

  /**
   * 单 job 版置回（查询侧后台补齐用）：查询刚用该笔记本的嵌入模型成功嵌入，
   * 说明模型已可解析——把去重命中的 pending_embedding job 置回 queued 立即补跑，
   * 不再干等下一次模型就绪信号。幂等：非 pending_embedding 不改动，返回是否置回。
   */
  requeuePendingEmbeddingIngestionJob(input: { studioId: unknown; jobId: unknown }): boolean {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const jobId = requiredString(input?.jobId, "jobId", 128);
    const result = this.db.prepare(`
      UPDATE ingestion_jobs SET status = 'queued', updated_at = ?
      WHERE id = ? AND status = 'pending_embedding'
        AND notebook_id IN (SELECT id FROM notebooks WHERE studio_id = ?)
    `).run(this.now(), jobId, studioId);
    return Number(result.changes) === 1;
  }

  /**
   * 启动恢复：running 残留（进程崩溃/强杀中断）重置回 queued 续跑。
   * 各 phase 幂等（fingerprint/hasArtifact 判断），从 phase 断点续跑无副作用。返回重置数量。
   * embed 相位的中断显式留痕（§一百零四 KNOWLEDGE_EMBEDDING_INTERRUPTED）：
   * 已落库的批级 checkpoint 向量保留，续跑只补缺失 chunk，不静默重新消费 API。
   */
  requeueRunningIngestionJobs(): number {
    const result = this.db.prepare(`
      UPDATE ingestion_jobs
      SET status = 'queued',
        error = CASE WHEN phase = 'embed' THEN ? ELSE error END,
        updated_at = ?
      WHERE status = 'running'
    `).run(
      `${KNOWLEDGE_EMBEDDING_INTERRUPTED}: embedding interrupted; checkpointed vectors are reused on resume`,
      this.now(),
    );
    return Number(result.changes);
  }

  /**
   * 单 job 版启动恢复（worker 池 stop 路径用）：仅当该 job 仍为 running 时置回
   * queued——不再全局翻 running，避免把池内其他仍在收尾的 job 连带打断。
   * embed 相位中断留痕语义与全局版一致。返回是否置回。
   */
  requeueRunningIngestionJobById(input: { jobId: unknown }): boolean {
    const jobId = requiredString(input?.jobId, "jobId", 128);
    const result = this.db.prepare(`
      UPDATE ingestion_jobs
      SET status = 'queued',
        error = CASE WHEN phase = 'embed' THEN ? ELSE error END,
        updated_at = ?
      WHERE id = ? AND status = 'running'
    `).run(
      `${KNOWLEDGE_EMBEDDING_INTERRUPTED}: embedding interrupted; checkpointed vectors are reused on resume`,
      this.now(),
      jobId,
    );
    return Number(result.changes) === 1;
  }

  /**
   * embed 相位成本观测落库（任务书 §七十四）：每次 embed 执行结束（含
   * unavailable/skipped）由摄入 worker 写入；仅 running 可写（先于 complete/
   * fail/pending 状态翻转调用）。stats 形状由 toIngestionJob 侧解析校验兜底。
   */
  recordIngestionJobEmbeddingStats(input: {
    studioId: unknown;
    jobId: unknown;
    stats: KnowledgeIngestionEmbeddingStats;
  }): IngestionJob {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const job = this.runningIngestionJob(studioId, input?.jobId);
    const stats = input?.stats;
    const counts = [stats?.chunksNewlyEmbedded, stats?.chunksResumedFromCheckpoint,
      stats?.chunksReusedFromReadyVariant, stats?.requestCount];
    if (
      !stats || typeof stats !== "object"
      || counts.some(value => !Number.isSafeInteger(value) || Number(value) < 0)
      || typeof stats.resetStaleVectors !== "boolean"
    ) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Ingestion embedding stats are invalid");
    }
    this.db.prepare(`
      UPDATE ingestion_jobs SET embedding_stats = ?, updated_at = ? WHERE id = ?
    `).run(JSON.stringify(stats), this.now(), job.id);
    return this.getIngestionJob({ studioId, jobId: job.id });
  }

  /** 摄入 worker 跨 studio 认领 job 后回查归属（job 行不冗余存 studio_id，经 notebook join 推导）。 */
  getIngestionJobOwner(input: { jobId: unknown }): {
    studioId: string;
    notebookId: string;
    sourceId: string;
  } | null {
    const jobId = requiredString(input?.jobId, "jobId", 128);
    const row = this.db.prepare(`
      SELECT nb.studio_id AS studio_id, j.notebook_id AS notebook_id, j.source_id AS source_id
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE j.id = ?
    `).get(jobId);
    if (!row) return null;
    return {
      studioId: row.studio_id,
      notebookId: row.notebook_id,
      sourceId: row.source_id,
    };
  }

  /** 跨 studio 列出 pending_embedding job（模型就绪补跑判定用），每行附归属 studioId。 */
  listPendingEmbeddingIngestionJobs(): Array<IngestionJob & { studioId: string }> {
    return this.db.prepare(`
      SELECT j.*, nb.studio_id AS studio_id
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE j.status = 'pending_embedding'
      ORDER BY j.created_at ASC, j.id ASC
    `).all().map((row: any) => ({ ...(toIngestionJob(row) as IngestionJob), studioId: row.studio_id }));
  }

  /**
   * 源的最新摄入 job。可选 notebookId 过滤：一源多笔记本时各笔记本的
   * 摄入状态彼此独立（job 按 notebook+source 去重入队），不传则跨笔记本
   * 取最新一条（源级视图用）。
   */
  getLatestIngestionJobForSource(input: {
    studioId: unknown;
    sourceId: unknown;
    notebookId?: unknown;
  }): IngestionJob | null {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const sourceId = requiredString(input?.sourceId, "sourceId", 128);
    this.activeSource(studioId, sourceId);
    const notebookId = input?.notebookId == null
      ? null
      : requiredString(input.notebookId, "notebookId", 128);
    return toIngestionJob(notebookId == null
      ? this.db.prepare(`
          SELECT * FROM ingestion_jobs
          WHERE source_id = ?
          ORDER BY created_at DESC, id DESC
          LIMIT 1
        `).get(sourceId)
      : this.db.prepare(`
          SELECT * FROM ingestion_jobs
          WHERE source_id = ? AND notebook_id = ?
          ORDER BY created_at DESC, id DESC
          LIMIT 1
        `).get(sourceId, notebookId));
  }

  listIngestionJobs(input: {
    studioId: unknown;
    notebookId?: unknown;
    sourceId?: unknown;
    statuses?: unknown;
    limit?: unknown;
  }): IngestionJob[] {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const clauses = ["nb.studio_id = ?"];
    const params: unknown[] = [studioId];
    if (input?.notebookId != null) {
      clauses.push("j.notebook_id = ?");
      params.push(requiredString(input.notebookId, "notebookId", 128));
    }
    if (input?.sourceId != null) {
      clauses.push("j.source_id = ?");
      params.push(requiredString(input.sourceId, "sourceId", 128));
    }
    if (input?.statuses != null) {
      if (
        !Array.isArray(input.statuses) || input.statuses.length === 0
        || input.statuses.some((status) => !INGESTION_STATUSES.has(status))
      ) {
        throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "statuses must be a non-empty ingestion status array");
      }
      clauses.push(`j.status IN (${input.statuses.map(() => "?").join(", ")})`);
      params.push(...input.statuses);
    }
    const limit = optionalIntegerInRange(input?.limit, "limit", 1, 500) ?? 100;
    params.push(limit);
    return this.db.prepare(`
      SELECT j.*
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE ${clauses.join(" AND ")}
      ORDER BY j.created_at DESC, j.id DESC
      LIMIT ?
    `).all(...params).map(toIngestionJob);
  }

  countIngestionJobsByStatus(input: {
    studioId: unknown;
    notebookId?: unknown;
  }): Record<IngestionJobStatus, number> {
    const studioId = requiredString(input?.studioId, "studioId", 256);
    const clauses = ["nb.studio_id = ?"];
    const params: unknown[] = [studioId];
    if (input?.notebookId != null) {
      clauses.push("j.notebook_id = ?");
      params.push(requiredString(input.notebookId, "notebookId", 128));
    }
    const counts: Record<IngestionJobStatus, number> = {
      queued: 0,
      running: 0,
      pending_embedding: 0,
      failed: 0,
      done: 0,
    };
    const rows = this.db.prepare(`
      SELECT j.status AS status, COUNT(*) AS count
      FROM ingestion_jobs j
      JOIN notebooks nb ON nb.id = j.notebook_id
      WHERE ${clauses.join(" AND ")}
      GROUP BY j.status
    `).all(...params);
    for (const row of rows) {
      if (INGESTION_STATUSES.has(row.status)) {
        counts[row.status as IngestionJobStatus] = Number(row.count);
      }
    }
    return counts;
  }

  /**
   * 活跃笔记本当前绑定的 RetrievalProfile 的全部 chunkProfileHash（§十八 DerivedIndexVariant
   * GC 候选判定用）：任何笔记本仍指向该 profile 的变体都不是零引用。
   */
  listActiveRetrievalProfileChunkHashes(): string[] {
    return (this.db.prepare(`
      SELECT DISTINCT cp.profile_hash AS profile_hash
      FROM notebooks n
      JOIN retrieval_profiles rp ON rp.id = n.retrieval_profile_id
      JOIN chunk_profiles cp ON cp.id = rp.chunk_profile_id
      WHERE n.deleted_at IS NULL
    `).all() as any[]).map(row => row.profile_hash);
  }

  /** 活跃（queued/running/pending_embedding）job 锚定的 artifact id 集合（variant GC 排除用）。 */
  listActiveIngestionArtifactIds(): string[] {
    return (this.db.prepare(`
      SELECT DISTINCT artifact_id FROM ingestion_jobs
      WHERE status IN ('queued', 'running', 'pending_embedding') AND artifact_id IS NOT NULL
    `).all() as any[]).map(row => row.artifact_id);
  }

  close() {
    if (!this.db) return;
    this.db.close();
    this.db = null;
  }
}
