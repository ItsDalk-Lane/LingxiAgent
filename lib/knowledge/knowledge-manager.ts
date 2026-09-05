import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import { isKnowledgeError, KnowledgeError } from "./errors.ts";
import {
  DEFAULT_KNOWLEDGE_IMPORT_MAX_BYTES,
  readSecureKnowledgeImportFile,
} from "./file-import-security.ts";
import { KnowledgeStore, resolveNotebookConfig } from "./knowledge-store.ts";
import { computeAutoChunkTargetChars, resolveKnowledgeChunkerConfig } from "./chunker.ts";
import { KnowledgeIndexStore } from "./knowledge-index-store.ts";
import {
  KnowledgeIngestionService,
  type KnowledgeEmbeddingGateLimits,
  type KnowledgeIngestionEmbedRequest,
} from "./ingestion-service.ts";
import {
  KnowledgeQueryService,
  buildKnowledgeBlockLocatorIndex,
  type KnowledgeEmbeddingResult,
  type KnowledgeReranker,
} from "./knowledge-query-service.ts";
import { KNOWLEDGE_RERANK_DISABLED_POLICY } from "./rerank-policy.ts";
import { createKnowledgeVectorSearchBackend } from "./vector-search-backend-factory.ts";
import type { KnowledgeVectorSearchBackend } from "./vector-search-backend.ts";
import { PortableVectorIndexAdapter } from "./vector-index-adapter.ts";
import {
  KnowledgeSourceFileWatcher,
  type KnowledgeSourceFileWatcherTuning,
} from "./source-file-watcher.ts";
import {
  DEFAULT_WEB_SNAPSHOT_MAX_BYTES,
  fetchCitationGradeWebSnapshot,
  type WebSnapshotFetchOptions,
  type WebSnapshotFetchResult,
} from "./web-snapshot-security.ts";
import { parseCitationGradeSnapshot } from "./source-adapters.ts";
import {
  PROCESSOR_MIME_CSV,
  PROCESSOR_MIME_DOCX,
  PROCESSOR_MIME_XLSX,
  processKnowledgeSnapshot,
  rebuildBlocksFromProcessorOutput,
  resolveKnowledgeProcessor,
  type KnowledgeProcessorPlan,
} from "./source-processors.ts";
import type { KnowledgeBlockDraft } from "./source-adapters.ts";
import { ScopeSnapshotCompiler } from "./scope-snapshot-compiler.ts";
import { FastKnowledgePipeline, type FastKnowledgeEvidenceStages } from "./fast-knowledge-pipeline.ts";
import { EvidencePacker } from "./evidence-packer.ts";
import { KnowledgeSearchService } from "./knowledge-search-service.ts";
import type {
  ContentSnapshot,
  ImportedKnowledgeSource,
  KnowledgeModelRef,
  KnowledgeParseArtifact,
  KnowledgeProcessingArtifact,
} from "./types.ts";

export const KNOWLEDGE_PARSER_ID = "lingxi-citation";
export const KNOWLEDGE_PARSER_VERSION = "1";

/**
 * orphan Source 保留期（§十八）：最后一个 membership 被移除后进入 orphan 态，
 * 过保留期且通过全部安全检查才物理 GC（宁可漏删不可误删）。默认 7 天。
 */
export const KNOWLEDGE_ORPHAN_SOURCE_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;
/** 生命周期维护（orphan GC + 零引用变体诊断）的定时周期；启动时也会先跑一轮。 */
export const KNOWLEDGE_LIFECYCLE_GC_INTERVAL_MS = 60 * 60 * 1000;
/** 目录导入防护上限（§六十九）：递归深度与文件数，超限显式 skipped 留痕。 */
export const KNOWLEDGE_DIRECTORY_IMPORT_MAX_DEPTH = 8;
export const KNOWLEDGE_DIRECTORY_IMPORT_MAX_FILES = 500;
const KNOWLEDGE_PARSER_CONFIG = Object.freeze({
  schemaVersion: 1,
  text: { unit: "non_empty_line" },
  markdown: { headingPath: true, unit: "non_empty_line" },
  html: { structuralPath: true, scripts: "removed" },
  pdf: { unit: "visual_line", coordinates: true },
});

const SUPPORTED_FILE_TYPES = new Map([
  [".txt", "text/plain"],
  [".md", "text/markdown"],
  [".markdown", "text/markdown"],
  [".pdf", "application/pdf"],
  [".html", "text/html"],
  [".htm", "text/html"],
  [".docx", PROCESSOR_MIME_DOCX],
  [".xlsx", PROCESSOR_MIME_XLSX],
  [".csv", PROCESSOR_MIME_CSV],
]);

/**
 * 显式不支持的格式（§五十八：无可用 processor 的格式显式拒绝，绝不静默降级为
 * 无定位的纯文本）。错误消息不含路径，details 只带扩展名。
 */
const UNSUPPORTED_PROCESSOR_EXTENSIONS = new Set([
  ".ppt", ".pptx", ".pptm", ".epub", ".doc", ".xls",
]);

function createId(prefix: string): string {
  return `${prefix}_${crypto.randomUUID()}`;
}

function resolveSupportedMimeType(fileName: string, bytes: Buffer): string {
  const extension = path.extname(fileName).toLowerCase();
  const mimeType = SUPPORTED_FILE_TYPES.get(extension);
  if (!mimeType) {
    if (UNSUPPORTED_PROCESSOR_EXTENSIONS.has(extension)) {
      throw new KnowledgeError(
        "KNOWLEDGE_IMPORT_PROCESSOR_UNAVAILABLE",
        "This file format has no registered knowledge processor",
        { extension },
      );
    }
    throw new KnowledgeError(
      "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED",
      "This file type does not yet support citation-grade Knowledge import",
    );
  }
  if (mimeType === "application/pdf" && !bytes.subarray(0, 5).equals(Buffer.from("%PDF-"))) {
    throw new KnowledgeError(
      "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED",
      "The file content does not match its PDF extension",
    );
  }
  if (mimeType.startsWith("text/") && bytes.includes(0)) {
    throw new KnowledgeError(
      "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED",
      "Binary content cannot be imported as text",
    );
  }
  return mimeType;
}

function normalizeDisplayName(value: unknown, fallback: string): string {
  if (value == null) return fallback;
  if (typeof value !== "string" || !value.trim() || value.trim().length > 255) {
    throw new KnowledgeError(
      "KNOWLEDGE_INVALID_ARGUMENT",
      "displayName must be a non-empty string no longer than 255 characters",
    );
  }
  return value.trim();
}

export interface KnowledgeManagerOptions {
  lingxiHome: string;
  maxImportBytes?: number;
  now?: () => string;
  idGenerator?: (prefix: string) => string;
  Database?: any;
  log?: (message: string) => void;
  rerank?: KnowledgeReranker | null;
  /**
   * 摄入管线嵌入回调（engine 用 ModelOperationResolver/EmbeddingClient 按显式
   * 模型引用接线）；缺省时摄入在 embed 相位落显式 pending_embedding。
   */
  embedTextsForModel?: ((request: KnowledgeIngestionEmbedRequest) => Promise<KnowledgeEmbeddingResult | null>) | null;
  canEmbedWithModel?: ((modelRef: KnowledgeModelRef) => boolean) | null;
  getModelConfigurationRevision?: (ref: KnowledgeModelRef) => string;
  /**
   * 查询侧 rerank 执行回调（v8：按笔记本显式引用路由）。配置类不可解析由
   * engine 侧记日志并返回 null，检索显式降级 RRF 名次；请求级错误照常抛出。
   */
  rerankForModel?: ((request: {
    runId: string;
    query: string;
    documents: string[];
    topN: number;
    signal?: AbortSignal;
    modelRef: KnowledgeModelRef;
  }) => Promise<{ results: Array<{ index: number; score: number }> } | null>) | null;
  /** 查嵌入模型上下文窗口（token 数）：自动分块与生效值展示共用。 */
  getEmbeddingModelContextWindow?: ((modelRef: KnowledgeModelRef) => number | null) | null;
  ingestionLog?: (message: string) => void;
  /** 测试注入：file 源 watcher 的计时器/IO 参数（防抖/退避/轮询时长、watch/stat 工厂）。 */
  fileWatcher?: KnowledgeSourceFileWatcherTuning;
  /** 摄入 worker 池并发上限（§十六；默认 KNOWLEDGE_INGESTION_DEFAULT_CONCURRENCY=3）。 */
  ingestionConcurrency?: number;
  /** embedding provider 限流（§十六 Provider Semaphore）；缺省保守默认（并发 2 / 间隔 250ms）。 */
  embeddingGate?: KnowledgeEmbeddingGateLimits | null;
  /** orphan Source 保留期（§十八；默认 7 天）。 */
  orphanRetentionMs?: number;
  /** 生命周期维护定时周期（默认 1 小时）。 */
  lifecycleGcIntervalMs?: number;
  fetchWebSnapshot?: (
    url: unknown,
    options?: WebSnapshotFetchOptions,
  ) => Promise<WebSnapshotFetchResult>;
}

export interface KnowledgeSourcePurgeResult {
  studioId: string;
  sourceId: string;
  jobs: number;
  turnScopeSources: number;
  citations: number;
  blocks: number;
  parseArtifacts: number;
  snapshots: number;
  memberships: number;
  parseArtifactIds: string[];
  contentSnapshotIds: string[];
  removedChunkVariants: number;
  removedVectorArtifacts: number;
}

export interface KnowledgeOrphanGcReport {
  scanned: number;
  purged: string[];
  skipped: Array<{ sourceId: string; reason: string }>;
}

/** 目录导入结果（§六十九）：每个文件的去向显式归入三组之一，绝不静默。 */
export interface KnowledgeDirectoryImportResult {
  imported: Array<{ sourceId: string; path: string; reused: boolean }>;
  skipped: Array<{ path: string; reason: string }>;
  failed: Array<{ path: string; reason: string }>;
}

/** 零引用 ChunkIndexVariant 的诊断候选（§十八 DerivedIndexVariant GC，本阶段只检测不清理）。 */
export interface KnowledgeVariantGcCandidate {
  parseArtifactId: string;
  chunkProfileHash: string;
  status: "building" | "ready" | "failed" | "retiring";
}

/**
 * Knowledge 领域入口。数据库、托管原文、解析产物和索引保持物理分离，
 * Engine 只持有这个入口，不承载领域内的事务细节。
 */
export class KnowledgeManager {
  readonly knowledgeRoot: string;
  readonly sourcesRoot: string;
  readonly artifactsRoot: string;
  readonly processedRoot: string;
  readonly indexesRoot: string;
  readonly store: KnowledgeStore;
  readonly indexStore: KnowledgeIndexStore;
  readonly vectorIndex: PortableVectorIndexAdapter;
  readonly vectorSearchBackend: KnowledgeVectorSearchBackend;
  readonly queryService: KnowledgeQueryService;
  readonly ingestion: KnowledgeIngestionService;
  readonly watcher: KnowledgeSourceFileWatcher;
  readonly scopeCompiler: ScopeSnapshotCompiler;
  readonly searchService: KnowledgeSearchService;
  private readonly scopeBuildRequests = new Map<string, ReturnType<typeof setImmediate>>();
  private readonly lingxiHome: string;
  private readonly maxImportBytes: number;
  private readonly options: KnowledgeManagerOptions;
  private readonly idGenerator: (prefix: string) => string;
  private readonly fetchWebSnapshot: NonNullable<KnowledgeManagerOptions["fetchWebSnapshot"]>;
  private readonly now: () => string;
  private readonly orphanRetentionMs: number;
  private readonly lifecycleGcIntervalMs: number;
  private readonly lifecycleLog: (message: string) => void;
  private lifecycleGcTimer: ReturnType<typeof setInterval> | null = null;
  private metadataBackfill: ReturnType<typeof setImmediate> | null = null;

  constructor(options: KnowledgeManagerOptions) {
    this.options = options;
    if (!options?.lingxiHome || !path.isAbsolute(options.lingxiHome)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "KnowledgeManager requires an absolute lingxiHome");
    }
    this.lingxiHome = options.lingxiHome;
    this.maxImportBytes = options.maxImportBytes ?? DEFAULT_KNOWLEDGE_IMPORT_MAX_BYTES;
    if (!Number.isSafeInteger(this.maxImportBytes) || this.maxImportBytes <= 0) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "maxImportBytes must be a positive integer");
    }
    this.idGenerator = options.idGenerator || createId;
    this.fetchWebSnapshot = options.fetchWebSnapshot || fetchCitationGradeWebSnapshot;
    this.now = options.now || (() => new Date().toISOString());
    this.orphanRetentionMs = options.orphanRetentionMs ?? KNOWLEDGE_ORPHAN_SOURCE_RETENTION_MS;
    this.lifecycleGcIntervalMs = options.lifecycleGcIntervalMs ?? KNOWLEDGE_LIFECYCLE_GC_INTERVAL_MS;
    this.lifecycleLog = options.ingestionLog || (() => {});
    this.knowledgeRoot = path.join(this.lingxiHome, "knowledge");
    this.sourcesRoot = path.join(this.knowledgeRoot, "sources");
    this.artifactsRoot = path.join(this.knowledgeRoot, "artifacts");
    this.processedRoot = path.join(this.knowledgeRoot, "processed");
    this.indexesRoot = path.join(this.knowledgeRoot, "indexes");

    fs.mkdirSync(this.sourcesRoot, { recursive: true, mode: 0o700 });
    fs.mkdirSync(this.artifactsRoot, { recursive: true, mode: 0o700 });
    fs.mkdirSync(this.processedRoot, { recursive: true, mode: 0o700 });
    fs.mkdirSync(this.indexesRoot, { recursive: true, mode: 0o700 });
    this.store = new KnowledgeStore({
      dbPath: path.join(this.knowledgeRoot, "knowledge.db"),
      Database: options.Database,
      now: options.now,
      idGenerator: this.idGenerator,
      // 自动分块尺寸/Profile 解析与 engine 的模型目录同一口径（core/engine.ts 接线）。
      getEmbeddingModelContextWindow: options.getEmbeddingModelContextWindow ?? null,
    });
    this.indexStore = new KnowledgeIndexStore({
      dbPath: path.join(this.indexesRoot, "knowledge-fts.db"),
      Database: options.Database,
      now: options.now,
    });
    this.vectorIndex = new PortableVectorIndexAdapter({
      dbPath: path.join(this.indexesRoot, "knowledge-vector.db"),
      onReadyVariant: id => this.vectorSearchBackend?.scheduleBuild(id),
      onInvalidateVariant: id => this.vectorSearchBackend?.invalidate(id),
      Database: options.Database,
      now: options.now,
      // 向量库 v1→v2 迁移回填：从 FTS 库读该 artifact 当前 variant 的
      // chunk_profile_hash（无行 → null → legacy_unknown 建档）。必须是全函数：
      // 抛错会中止迁移并按「缓存损坏」语义删库重建（见 vector-index-adapter 文件头）。
      profileHashResolver: (parseArtifactId) => {
        try {
          const row = this.indexStore.db.prepare(`
            SELECT chunk_profile_hash FROM chunk_index_variants
            WHERE parse_artifact_id = ?
            ORDER BY CASE status WHEN 'ready' THEN 0 ELSE 1 END, created_at ASC, id ASC
            LIMIT 1
          `).get(parseArtifactId);
          return typeof row?.chunk_profile_hash === "string" ? row.chunk_profile_hash : null;
        } catch {
          return null;
        }
      },
    });
    this.vectorSearchBackend = createKnowledgeVectorSearchBackend({
      indexesRoot: this.indexesRoot, portable: this.vectorIndex, Database: options.Database,
      now: options.now, log: options.ingestionLog,
    });
    this.queryService = new KnowledgeQueryService({
      store: this.store,
      indexStore: this.indexStore,
      vectorIndex: this.vectorIndex,
      vectorSearchBackend: this.vectorSearchBackend,
      getModelConfigurationRevision: options.getModelConfigurationRevision,
      embedTextsForModel: options.embedTextsForModel ?? null,
      rerank: options.rerank,
      rerankForModel: options.rerankForModel ?? null,
      getEmbeddingModelContextWindow: options.getEmbeddingModelContextWindow ?? null,
      // 查询侧后台补齐（§十二）：变体缺失/未就绪 → 幂等入队摄入（活跃 job 去重），
      // 查询线程不等待构建。入队失败（如笔记本被并发删除）不阻断本轮查询——
      // 降级已显式留痕，下一次查询会再次幂等尝试入队。
      requestVariantBuild: (input) => {
        try {
          // 字段映射：查询侧锚名 parseArtifactId → 摄入侧 artifactId（漏映射会走
          // 占位 configId 路径，丢失入队时的真实分块配置记录与 profile 建绑）。
          this.ingestion.requestVariantBuild({
            studioId: input.studioId,
            notebookId: input.notebookId,
            sourceId: input.sourceId,
            artifactId: input.parseArtifactId,
          });
        } catch (error) {
          options.ingestionLog?.(
            `knowledge query: background variant build enqueue failed: ${
              error instanceof Error ? error.message : String(error)
            }`,
          );
        }
      },
    });
    this.searchService = new KnowledgeSearchService({ store: this.store, indexStore: this.indexStore, queryService: this.queryService });
    this.ingestion = new KnowledgeIngestionService({
      store: this.store,
      queryService: this.queryService,
      parseSource: (input) => this.parseSource(input),
      embedTextsForModel: options.embedTextsForModel ?? null,
      canEmbedWithModel: options.canEmbedWithModel ?? null,
      getEmbeddingModelContextWindow: options.getEmbeddingModelContextWindow ?? null,
      concurrency: options.ingestionConcurrency,
      embeddingGate: options.embeddingGate ?? null,
      now: options.now,
      log: options.ingestionLog,
    });
    this.scopeCompiler = new ScopeSnapshotCompiler({
      store: this.store,
      indexStore: this.indexStore,
      requestVariantBuild: (input) => {
        const key = `${input.notebookId}:${input.sourceId}:${input.parseArtifactId}`;
        if (this.scopeBuildRequests.has(key)) return;
        // 既有入队接口会读正文解析配置；推到后台，编译关键路径只读元信息。
        const pending = setImmediate(() => {
          this.scopeBuildRequests.delete(key);
          try {
            this.ingestion.requestVariantBuild({
              studioId: input.studioId,
              notebookId: input.notebookId,
              sourceId: input.sourceId,
              artifactId: input.parseArtifactId,
            });
            this.scopeCompiler.invalidateNotebook(input.notebookId);
          } catch (error) {
            this.lifecycleLog(`knowledge scope: background variant enqueue failed: ${
              error instanceof Error ? error.message : String(error)
            }`);
          }
        });
        pending.unref();
        this.scopeBuildRequests.set(key, pending);
      },
    });
    this.watcher = new KnowledgeSourceFileWatcher({
      refresh: (input) => this.refreshFileSource(input),
      enqueueForNotebook: (input) => this.ingestion.enqueueSourceIngestion(input),
      log: options.ingestionLog,
      now: options.now,
      ...options.fileWatcher,
    });
    this.scheduleMetadataBackfill();
  }

  /** 启动完成后再逐批补齐目录；失败留痕并继续其他变体，关闭时取消未执行批次。 */
  private scheduleMetadataBackfill(afterId = ""): void {
    this.metadataBackfill = setImmediate(() => {
      this.metadataBackfill = null;
      try {
        const variants = this.indexStore.listReadyVariantsMissingMetadata(afterId);
        for (const variant of variants) {
          try {
            const sourceId = this.queryService.backfillVariantMetadata(variant);
            this.scopeCompiler.invalidateSource(sourceId);
            this.searchService.clearResults();
          } catch (error) {
            this.lifecycleLog(`knowledge metadata: background backfill failed for ${variant.id}: ${
              error instanceof Error ? error.message : String(error)
            }`);
          }
        }
        if (variants.length === 20) this.scheduleMetadataBackfill(variants.at(-1)!.id);
      } catch (error) {
        this.lifecycleLog(`knowledge metadata: background scan failed: ${error instanceof Error ? error.message : String(error)}`);
      }
    });
    this.metadataBackfill.unref();
  }

  createNotebook(input: Parameters<KnowledgeStore["createNotebook"]>[0]) {
    return this.store.createNotebook(input);
  }

  listNotebooks(input: Parameters<KnowledgeStore["listNotebooks"]>[0]) {
    return this.store.listNotebooks(input);
  }

  getNotebook(input: Parameters<KnowledgeStore["getNotebook"]>[0]) {
    return this.store.getNotebook(input);
  }

  renameNotebook(input: Parameters<KnowledgeStore["renameNotebook"]>[0]) {
    return this.store.renameNotebook(input);
  }

  deleteNotebook(input: Parameters<KnowledgeStore["deleteNotebook"]>[0]) {
    this.scopeCompiler.invalidateNotebook(String(input.notebookId));
    this.searchService.clearResults();
    // 删除前记录该笔记本的全部源（删除后 orphan 判定要用；listNotebookSources 要求活跃笔记本）。
    const affectedSourceIds = this.store.listNotebookSources({
      studioId: input?.studioId,
      notebookId: input?.notebookId,
    }).map(entry => entry.source.id);
    const notebook = this.store.deleteNotebook(input);
    // 笔记本删除后摘掉其全部 watch membership（最后一个 membership 消失即摘 watcher）。
    this.watcher.untrackNotebook(notebook.id);
    for (const sourceId of affectedSourceIds) {
      // 各源重算 orphan 状态（§十八：最后 membership 消失 → orphan 标记，保留期后 GC）。
      this.recomputeSourceOrphanState(String(input?.studioId), sourceId);
      // 孤儿源派生索引即时回收（PR #30 语义）：有活跃 TurnScope/EvidenceManifest
      // 引用的源不即时清（见 pruneOrphanSourceIndexes 内保护闸），留到保留期 GC。
      this.pruneOrphanSourceIndexes(sourceId);
    }
    return notebook;
  }

  listNotebookSources(input: Parameters<KnowledgeStore["listNotebookSources"]>[0]) {
    return this.store.listNotebookSources(input);
  }

  addSourceToNotebook(input: Parameters<KnowledgeStore["addSourceToNotebook"]>[0]) {
    const membership = this.store.addSourceToNotebook(input);
    this.scopeCompiler.invalidateSource(membership.sourceId);
    // 复活语义（§十八）：orphan 源重新获得 membership 即清 orphan 标记（保留期内
    // 物理行都还在，恢复零成本；过保留期但尚未被 GC 扫到的同样复活）。
    const source = this.store.getSource({ studioId: input?.studioId, sourceId: input?.sourceId });
    if (source.orphanedAt != null) {
      this.store.clearSourceOrphan({ studioId: input?.studioId, sourceId: input?.sourceId });
      this.lifecycleLog(`knowledge lifecycle: source ${source.id} re-referenced; orphan marker cleared`);
    }
    // 既有 file 源被加进新笔记本：并入该源的 watch 项（多 membership 共用一个 watcher）。
    if (source.sourceType === "file") {
      const originalPath = source.originMetadata.originalPath;
      if (typeof originalPath === "string" && path.isAbsolute(originalPath)) {
        this.watcher.trackSource({
          studioId: source.studioId,
          notebookId: membership.notebookId,
          sourceId: source.id,
          filePath: originalPath,
        });
      }
    }
    return membership;
  }

  removeSourceFromNotebook(input: Parameters<KnowledgeStore["removeSourceFromNotebook"]>[0]) {
    const membership = this.store.removeSourceFromNotebook(input);
    this.scopeCompiler.invalidateSource(membership.sourceId);
    this.watcher.untrackSourceMembership({
      sourceId: membership.sourceId,
      notebookId: membership.notebookId,
    });
    // §十八三层生命周期：移除 membership ≠ 删 Source——重算该源活跃引用，
    // 仍有引用一切保留；零引用才标 orphan（保留期后 GC，物理清理绝不即时发生）。
    this.recomputeSourceOrphanState(String(input?.studioId), membership.sourceId);
    // 孤儿源派生索引即时回收（PR #30 语义）：受保护源（TurnScope/manifest 引用）
    // 不即时清，见 pruneOrphanSourceIndexes 内保护闸。
    this.pruneOrphanSourceIndexes(membership.sourceId);
    return membership;
  }

  /**
   * 源 orphan 状态重算（§十八）：零活跃 membership → 标 orphan（幂等）；
   * 仍有引用 → 防御性清 orphan（addSourceToNotebook 复活路径的正常兜底）。
   * GC 本身不在这里——删除性操作只发生在显式 deleteSource / 过保留期的 GC 扫描。
   */
  private recomputeSourceOrphanState(studioId: string, sourceId: string) {
    try {
      if (this.store.countActiveSourceMemberships({ sourceId }) === 0) {
        this.store.markSourceOrphaned({ studioId, sourceId });
        this.lifecycleLog(`knowledge lifecycle: source ${sourceId} orphaned (no active membership); GC eligible after retention`);
      } else {
        this.store.clearSourceOrphan({ studioId, sourceId });
      }
    } catch (error) {
      // 源在并发窗口被显式删除等：orphan 状态无意义，显式留痕即可。
      this.lifecycleLog(
        `knowledge lifecycle: orphan recompute skipped for ${sourceId}: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }

  /**
   * 显式删除源（Phase 5 §十九 delete wins 的载体）：
   * 1. 活跃 turn scope 冻结引用检查——冻结集合锚定 snapshot/artifact 行（FK RESTRICT），
   *    物理删除会破坏本轮读取；显式拒绝（409），新一轮 scope 接管后再删。
   * 2. 标记 deleted_at（此后一切 ensure/enqueue/refresh 经 activeSource 显式失败，
   *    并发 reingest 不得复活）。
   * 3. 取消该源全部活跃 job：queued/pending → failed+cancelled_at；running → abort
   *    嵌入并等待收尾（相位边界检查兜底）。cancelled job 永久不可 requeue。
   * 4. 物理清理（与 orphan GC 同一实现）：事实行 + 派生索引变体 + 托管字节/产物文件。
   */
  async deleteSource(input: { studioId: unknown; sourceId: unknown }): Promise<{
    source: import("./types.ts").KnowledgeSource;
    cancelledJobs: string[];
    purge: KnowledgeSourcePurgeResult;
  }> {
    const source = this.store.getSource(input);
    if (this.store.countActiveTurnScopesForSource({ sourceId: source.id }) > 0) {
      throw new KnowledgeError(
        "KNOWLEDGE_CONFLICT",
        "Knowledge source is frozen by an active knowledge turn scope; start another turn or close the scope first",
      );
    }
    // EvidenceManifest 引用检查（§六十七）：manifest 无 TTL 前全部保留——历史回答
    // 的证据版本追溯优先于物理清理。被任何 manifest 引用的源显式拒绝删除。
    if (this.store.countEvidenceManifestsForSource({ sourceId: source.id }) > 0) {
      throw new KnowledgeError(
        "KNOWLEDGE_CONFLICT",
        "Knowledge source is referenced by persisted evidence manifests; historical answers still trace to this source version",
      );
    }
    const deleted = this.store.markSourceDeleted({ studioId: input?.studioId, sourceId: source.id });
    this.watcher.untrackSource(source.id);
    const { cancelledJobIds } = await this.ingestion.cancelSourceJobs({
      studioId: input?.studioId,
      sourceId: source.id,
    });
    const purge = this.purgeSource({ studioId: source.studioId, sourceId: source.id });
    this.lifecycleLog(
      `knowledge lifecycle: source ${source.id} deleted `
      + `(jobs cancelled=${cancelledJobIds.length}, snapshots=${purge.snapshots}, artifacts=${purge.parseArtifacts})`,
    );
    return { source: deleted, cancelledJobs: cancelledJobIds, purge };
  }

  /**
   * 源的完整物理清理（deleteSource 与 orphan GC 共用）：
   * - 事实行先行（单事务，FK RESTRICT 保证半删即整体回滚）；
   * - 派生索引（FTS chunk 变体 / 向量变体）随后按 artifact 清除——索引库是可重建
   *   缓存，清除失败不回滚事实（显式留痕，残留行可被诊断面发现并另行清理）；
   * - 托管字节（sources/<src>/）与解析产物（artifacts/<snap>/）最后删除（best-effort，
   *   失败留痕；DB 行已删，残留文件不可达）。
   */
  private purgeSource(input: { studioId: string; sourceId: string }): KnowledgeSourcePurgeResult {
    this.scopeCompiler.invalidateSource(input.sourceId);
    const purge = this.store.purgeSourceRows({ studioId: input.studioId, sourceId: input.sourceId });
    let removedChunkVariants = 0;
    let removedVectorArtifacts = 0;
    for (const artifactId of purge.parseArtifactIds) {
      try {
        removedChunkVariants += this.indexStore.removeChunkIndexVariantsByArtifact(artifactId);
      } catch (error) {
        this.lifecycleLog(
          `knowledge lifecycle: chunk variant cleanup failed for artifact ${artifactId} of source ${input.sourceId}: ${
            error instanceof Error ? error.message : String(error)
          }`,
        );
      }
      try {
        this.vectorIndex.removeArtifact(artifactId);
        removedVectorArtifacts += 1;
      } catch (error) {
        this.lifecycleLog(
          `knowledge lifecycle: vector cleanup failed for artifact ${artifactId} of source ${input.sourceId}: ${
            error instanceof Error ? error.message : String(error)
          }`,
        );
      }
    }
    for (const snapshotId of purge.contentSnapshotIds) {
      try {
        fs.rmSync(path.join(this.artifactsRoot, snapshotId), { recursive: true, force: true });
      } catch (error) {
        this.lifecycleLog(
          `knowledge lifecycle: artifact dir cleanup failed for snapshot ${snapshotId}: ${
            error instanceof Error ? error.message : String(error)
          }`,
        );
      }
    }
    try {
      fs.rmSync(path.join(this.sourcesRoot, input.sourceId), { recursive: true, force: true });
    } catch (error) {
      this.lifecycleLog(
        `knowledge lifecycle: snapshot bytes cleanup failed for source ${input.sourceId}: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
    return { ...purge, removedChunkVariants, removedVectorArtifacts };
  }

  /**
   * orphan Source GC（§十八）：仅作用于「未显式删除、orphaned_at 已过保留期」的行，
   * 且逐源复核安全检查（宁可漏删不可误删）：
   * - 仍有活跃 membership（不应发生，防御）→ 清 orphan 标记并跳过；
   * - 仍被活跃 KnowledgeTurnScope 冻结引用 → 跳过（快照/artifact/派生索引整体保留）；
   * - 仍被 EvidenceManifest 引用（§六十七，manifest 无 TTL 前全部保留）→ 跳过；
   * - 仍有活跃摄入 job（membership 移除前入队的在跑/排队 job）→ 跳过，等 job 终态。
   * 通过全部检查才物理清理（purgeSource 同一实现）。返回扫描/清理/跳过明细。
   */
  runOrphanSourceGc(): KnowledgeOrphanGcReport {
    const cutoffIso = new Date(Date.parse(this.now()) - this.orphanRetentionMs).toISOString();
    const candidates = this.store.listSourcesPastOrphanRetention({ cutoffIso });
    const report: KnowledgeOrphanGcReport = { scanned: candidates.length, purged: [], skipped: [] };
    for (const candidate of candidates) {
      if (this.store.countActiveSourceMemberships({ sourceId: candidate.sourceId }) > 0) {
        // 防御：orphan 标记与活跃 membership 并存说明复活路径漏清（或并发窗口），
        // 修正标记、跳过本轮——绝不删除仍有引用的源。
        this.store.clearSourceOrphan({ studioId: candidate.studioId, sourceId: candidate.sourceId });
        report.skipped.push({ sourceId: candidate.sourceId, reason: "active-membership-reappeared" });
        continue;
      }
      if (this.store.countActiveTurnScopesForSource({ sourceId: candidate.sourceId }) > 0) {
        report.skipped.push({ sourceId: candidate.sourceId, reason: "active-turn-scope-reference" });
        continue;
      }
      // §六十七 EvidenceManifest 引用：manifest 无 TTL 前保守保留全部——被引用
      // 的源跳过物理清理（跳过原因显式留痕），历史回答的证据版本可继续追溯。
      if (this.store.countEvidenceManifestsForSource({ sourceId: candidate.sourceId }) > 0) {
        report.skipped.push({ sourceId: candidate.sourceId, reason: "evidence-manifest-referenced" });
        continue;
      }
      if (this.store.hasActiveIngestionJobsForSource({ sourceId: candidate.sourceId })) {
        report.skipped.push({ sourceId: candidate.sourceId, reason: "active-ingestion-job" });
        continue;
      }
      try {
        this.purgeSource({ studioId: candidate.studioId, sourceId: candidate.sourceId });
        report.purged.push(candidate.sourceId);
      } catch (error) {
        report.skipped.push({
          sourceId: candidate.sourceId,
          reason: `purge-failed: ${error instanceof Error ? error.message : String(error)}`,
        });
      }
    }
    if (report.purged.length > 0 || report.skipped.length > 0) {
      this.lifecycleLog(
        `knowledge lifecycle: orphan GC scanned=${report.scanned} purged=${report.purged.length} `
        + `skipped=${report.skipped.length}${report.skipped.length > 0 ? ` (${report.skipped.map(s => `${s.sourceId}:${s.reason}`).join(", ")})` : ""}`,
      );
    }
    return report;
  }

  /**
   * 零引用 ChunkIndexVariant 诊断候选（§十八 DerivedIndexVariant，本阶段保守实现：
   * 只检测 + 留痕，物理清理留待后续波次）：chunk_profile_hash 不被任何活跃笔记本的
   * RetrievalProfile 指向，且 artifact 不被任何活跃 job 锚定的变体。
   */
  collectDerivedVariantGcCandidates(): KnowledgeVariantGcCandidate[] {
    const activeHashes = new Set(this.store.listActiveRetrievalProfileChunkHashes());
    const activeArtifacts = new Set(this.store.listActiveIngestionArtifactIds());
    const candidates = this.indexStore.listChunkIndexVariants()
      .filter(variant => !activeHashes.has(variant.chunkProfileHash)
        && !activeArtifacts.has(variant.parseArtifactId))
      .map(variant => ({
        parseArtifactId: variant.parseArtifactId,
        chunkProfileHash: variant.chunkProfileHash,
        status: variant.status,
      }));
    if (candidates.length > 0) {
      this.lifecycleLog(
        `knowledge lifecycle: ${candidates.length} zero-reference chunk variant(s) detected (diagnostic only; physical cleanup deferred)`,
      );
    }
    return candidates;
  }

  /**
   * 启动生命周期维护（engine init 调用一次）：立即跑一轮 + 定时周期（默认 1 小时）。
   * orphan GC 与零引用变体诊断都是幂等扫描；单轮失败显式留痕不中断维护。
   */
  startLifecycleMaintenance() {
    if (this.lifecycleGcTimer) return;
    const runMaintenance = () => {
      try {
        this.runOrphanSourceGc();
      } catch (error) {
        this.lifecycleLog(
          `knowledge lifecycle: orphan GC failed: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
      try {
        this.collectDerivedVariantGcCandidates();
      } catch (error) {
        this.lifecycleLog(
          `knowledge lifecycle: variant GC diagnostics failed: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    };
    runMaintenance();
    this.lifecycleGcTimer = setInterval(runMaintenance, this.lifecycleGcIntervalMs);
    this.lifecycleGcTimer.unref?.();
  }

  /**
   * 孤儿源派生索引清理：源不再挂靠任何活跃笔记本时，删除其全部解析产物的
   * 向量与 FTS 行。事实数据（快照/解析产物记录）保留软删除语义可追溯；
   * 派生索引可由重摄入完全重建，清掉不损失信息。清理失败只记日志不阻断删除
   * （残留索引由 sweep 兜底回收）。
   * 保护闸（§十八/§六十七）：仍有活跃 TurnScope 或 EvidenceManifest 引用的源
   * 不即时清派生索引——这份数据是受保护证据链的一部分，统一留给过保留期的
   * orphan GC（同一套安全检查）物理处理。
   */
  private pruneOrphanSourceIndexes(sourceId: string) {
    try {
      if (this.store.listActiveNotebookIdsForSource({ sourceId }).length > 0) return;
      if (this.store.countActiveTurnScopesForSource({ sourceId }) > 0) return;
      if (this.store.countEvidenceManifestsForSource({ sourceId }) > 0) return;
      for (const artifactId of this.store.listSourceArtifactIds({ sourceId })) {
        this.vectorIndex.removeArtifact(artifactId);
        this.indexStore.removeArtifact(artifactId);
      }
    } catch (error) {
      this.options?.log?.(`knowledge: orphan index prune failed for ${sourceId}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  getSource(input: Parameters<KnowledgeStore["getSource"]>[0]) {
    return this.store.getSource(input);
  }

  /**
   * KnowledgeTurnScope 创建门面（任务书 §二十，Phase 4）：desktop 提交链路在
   * 携带 knowledgeRefs 的消息注入前调用，把本轮选中 notebooks 的活跃
   * membership × 各源最新 snapshot/artifact 冻结落库（store 同事务 supersede
   * 同会话旧 scope）。
   */
  createTurnScope(input: Parameters<KnowledgeStore["createTurnScope"]>[0]) {
    const scope = this.store.createTurnScope(input);
    this.scopeCompiler.invalidateSession(scope.sessionPath);
    return scope;
  }

  compileTurnScope(scope: Parameters<ScopeSnapshotCompiler["compile"]>[0]) {
    return this.scopeCompiler.compile(scope);
  }

  createFastKnowledgePipeline(stages: FastKnowledgeEvidenceStages) {
    let searchStats = { embeddingGroups: 0, rerankGroups: 0, queryEmbeddingCacheHit: false, retrievalResultCacheHit: false };
    return new FastKnowledgePipeline({
      ...stages,
      compile: scope => this.compileTurnScope(scope),
      search: async input => {
        const result = await this.searchService.searchWithEvidence({
          ...input,
          channel: "fts",
          rerankPolicy: KNOWLEDGE_RERANK_DISABLED_POLICY,
        });
        const { embeddingGroups, rerankGroups, queryEmbeddingCacheHit, retrievalResultCacheHit } = result.response;
        searchStats = { embeddingGroups, rerankGroups, queryEmbeddingCacheHit, retrievalResultCacheHit };
        return result.evidence.candidates;
      },
      searchStats: () => searchStats,
    });
  }

  runFastKnowledgePipeline(input: Parameters<FastKnowledgePipeline["run"]>[0]) {
    const packer = new EvidencePacker();
    return this.createFastKnowledgePipeline({
      extractSpans: request => this.queryService.extractEvidenceSpans(request),
      packEvidence: request => packer.pack(request),
    }).run(input);
  }

  getTurnScope(input: Parameters<KnowledgeStore["getTurnScope"]>[0]) {
    return this.store.getTurnScope(input);
  }

  closeTurnScope(input: Parameters<KnowledgeStore["closeTurnScope"]>[0]) {
    this.scopeCompiler.invalidateScope(String(input.scopeId));
    return this.store.closeTurnScope(input);
  }

  /**
   * 覆盖计划落库门面（schema v13，任务书 §二十九，Phase 7）：engine 在
   * planner 判定后持久化结构化结果（不存 CoT）；失败由调用方留痕日志，
   * 不阻断注入链路。
   */
  insertCoveragePlan(input: Parameters<KnowledgeStore["insertCoveragePlan"]>[0]) {
    return this.store.insertCoveragePlan(input);
  }

  getLatestCoveragePlan(input?: Parameters<KnowledgeStore["getLatestCoveragePlan"]>[0]) {
    return this.store.getLatestCoveragePlan(input);
  }

  /**
   * EvidenceManifest 门面（schema v15，任务书 §六十七）：engine 在注入完成后
   * 组装并持久化该轮的身份链（只存 id/序号/偏移，禁正文/CoT）；写入失败由
   * 调用方（desktop-session-submit）留痕 warn，不阻断会话提交。
   */
  insertEvidenceManifest(input: Parameters<KnowledgeStore["insertEvidenceManifest"]>[0]) {
    return this.store.insertEvidenceManifest(input);
  }

  getEvidenceManifestByScope(input: Parameters<KnowledgeStore["getEvidenceManifestByScope"]>[0]) {
    return this.store.getEvidenceManifestByScope(input);
  }

  getEvidenceManifestByTurn(input: Parameters<KnowledgeStore["getEvidenceManifestByTurn"]>[0]) {
    return this.store.getEvidenceManifestByTurn(input);
  }

  /** scope 成员校验：sourceId 是否在该 scope 的冻结集合内（knowledge_read 用）。 */
  isSourceInTurnScope(input: { scopeId: unknown; sourceId: unknown }): boolean {
    const scope = this.store.getTurnScope({ scopeId: input?.scopeId });
    if (!scope) return false;
    return scope.sources.some(source => source.sourceId === input?.sourceId);
  }

  /**
   * 取 scope 内某源的冻结条目（contentSnapshotId / parseArtifactId / 选中笔记本
   * 子集）；不在冻结集合返回 null（调用方映射 KNOWLEDGE_SCOPE_VIOLATION）。
   */
  getTurnScopeFrozenSource(input: { scopeId: unknown; sourceId: unknown }) {
    const scope = this.store.getTurnScope({ scopeId: input?.scopeId });
    if (!scope) return null;
    return scope.sources.find(source => source.sourceId === input?.sourceId) ?? null;
  }

  async importFile(input: {
    studioId: unknown;
    notebookId: unknown;
    filePath: unknown;
    displayName?: unknown;
  }): Promise<ImportedKnowledgeSource> {
    // 先验证 Notebook 所属关系，避免非法请求触发任何外部文件读取。
    this.store.getNotebook({ studioId: input?.studioId, notebookId: input?.notebookId });
    const imported = await readSecureKnowledgeImportFile({
      filePath: input?.filePath,
      lingxiHome: this.lingxiHome,
      maxBytes: this.maxImportBytes,
    });
    return this.importBytesAsSource({
      studioId: input.studioId,
      notebookId: input.notebookId,
      fileName: imported.fileName,
      realPath: imported.realPath,
      bytes: imported.bytes,
      displayName: input.displayName,
    });
  }

  /**
   * 字节 → Source+Snapshot 的公共内核（importFile / importDirectory 共用）：
   * 解析 MIME（不支持的格式在此显式拒绝）→ 原子落盘快照 → 落库 → 可选写入
   * 目录组织路径（Membership 维度）→ 挂文件 watch。
   */
  private async importBytesAsSource(input: {
    studioId: unknown;
    notebookId: unknown;
    fileName: string;
    realPath: string;
    bytes: Buffer;
    displayName?: unknown;
    membershipPath?: { relativePath: string; folderNode: string | null; displayOrder: number };
  }): Promise<ImportedKnowledgeSource> {
    const mimeType = resolveSupportedMimeType(input.fileName, input.bytes);
    const sourceId = this.idGenerator("src");
    const snapshotId = this.idGenerator("snap");
    const relativeStoragePath = path.posix.join("sources", sourceId, `${snapshotId}.bin`);
    const sourceDirectory = path.join(this.sourcesRoot, sourceId);
    const snapshotPath = path.join(sourceDirectory, `${snapshotId}.bin`);
    const temporaryPath = path.join(sourceDirectory, `.${snapshotId}.${crypto.randomBytes(6).toString("hex")}.tmp`);

    fs.mkdirSync(sourceDirectory, { recursive: false, mode: 0o700 });
    let published = false;
    try {
      const handle = await fs.promises.open(temporaryPath, "wx", 0o600);
      try {
        await handle.writeFile(input.bytes);
        await handle.sync();
      } finally {
        await handle.close();
      }
      await fs.promises.rename(temporaryPath, snapshotPath);
      published = true;

      const created = this.store.createSourceWithSnapshot({
        studioId: input.studioId,
        notebookId: input.notebookId,
        sourceId,
        snapshotId,
        sourceType: "file",
        displayName: normalizeDisplayName(input.displayName, input.fileName),
        originMetadata: {
          kind: "local_file",
          fileName: input.fileName,
          originalPath: input.realPath,
        },
        snapshot: {
          sha256: crypto.createHash("sha256").update(input.bytes).digest("hex"),
          mimeType,
          byteSize: input.bytes.length,
          storagePath: relativeStoragePath,
        },
      });
      if (input.membershipPath) {
        this.store.updateMembershipPath({
          studioId: input.studioId,
          notebookId: input.notebookId,
          sourceId: created.source.id,
          ...input.membershipPath,
        });
      }
      // file 源导入成功即挂 watch（外部原文件后续变化 → 自动 refresh + 摄入）。
      this.watcher.trackSource({
        studioId: created.source.studioId,
        notebookId: created.membership.notebookId,
        sourceId: created.source.id,
        filePath: input.realPath,
      });
      return created;
    } catch (error) {
      // 数据库事务失败时移除尚未被领域事实引用的字节；不触碰任何已有快照。
      await fs.promises.rm(temporaryPath, { force: true }).catch(() => {});
      if (published) await fs.promises.rm(snapshotPath, { force: true }).catch(() => {});
      await fs.promises.rmdir(sourceDirectory).catch(() => {});
      throw error;
    }
  }

  /**
   * 目录导入（§六十九）：递归展开目录（深度 ≤8、文件 ≤500），逐文件走
   * readSecureKnowledgeImportFile 安全读取；同 studio 内容 sha 去重（命中即复用
   * 既有 Source，仅补 membership + 目录路径）；目录组织路径写入 Membership。
   * 返回 imported/skipped/failed 三组明细——每个文件的去向都必须显式，绝不静默。
   */
  async importDirectory(input: {
    studioId: unknown;
    notebookId: unknown;
    dirPath: unknown;
  }): Promise<KnowledgeDirectoryImportResult> {
    this.store.getNotebook({ studioId: input?.studioId, notebookId: input?.notebookId });
    if (typeof input?.dirPath !== "string" || !path.isAbsolute(input.dirPath)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "dirPath must be an absolute path");
    }
    const stats = await fs.promises.lstat(input.dirPath).catch(() => null);
    if (!stats || stats.isSymbolicLink() || !stats.isDirectory()) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "dirPath must be a real directory");
    }
    const dirRealPath = await fs.promises.realpath(input.dirPath);

    const result: KnowledgeDirectoryImportResult = { imported: [], skipped: [], failed: [] };
    const entries: Array<{ realPath: string; relativePath: string }> = [];
    const walk = async (current: string, depth: number): Promise<void> => {
      if (depth > KNOWLEDGE_DIRECTORY_IMPORT_MAX_DEPTH) {
        result.skipped.push({
          path: path.relative(dirRealPath, current).split(path.sep).join("/") || ".",
          reason: "depth_limit",
        });
        return;
      }
      const children = await fs.promises.readdir(current, { withFileTypes: true });
      children.sort((a, b) => a.name.localeCompare(b.name));
      for (const child of children) {
        if (entries.length >= KNOWLEDGE_DIRECTORY_IMPORT_MAX_FILES) return;
        const childPath = path.join(current, child.name);
        const childRelative = path.relative(dirRealPath, childPath).split(path.sep).join("/");
        if (child.isSymbolicLink()) {
          result.skipped.push({ path: childRelative, reason: "symlink_rejected" });
          continue;
        }
        if (child.isDirectory()) {
          await walk(childPath, depth + 1);
          continue;
        }
        if (!child.isFile()) {
          result.skipped.push({ path: childRelative, reason: "not_regular_file" });
          continue;
        }
        entries.push({ realPath: childPath, relativePath: childRelative });
      }
    };
    await walk(dirRealPath, 1);
    const truncated = entries.length >= KNOWLEDGE_DIRECTORY_IMPORT_MAX_FILES
      ? "file_count_limit"
      : null;

    let displayOrder = 0;
    for (const entry of entries) {
      const relativePath = entry.relativePath;
      const folder = path.posix.dirname(relativePath);
      const membershipPath = {
        relativePath,
        folderNode: folder === "." ? null : folder,
        displayOrder,
      };
      displayOrder += 1;
      try {
        const imported = await readSecureKnowledgeImportFile({
          filePath: entry.realPath,
          lingxiHome: this.lingxiHome,
          maxBytes: this.maxImportBytes,
        });
        resolveSupportedMimeType(imported.fileName, imported.bytes);
        const contentSha = crypto.createHash("sha256").update(imported.bytes).digest("hex");
        const existingSourceId = this.store.findSourceIdByContentSha({
          studioId: input.studioId,
          sha256: contentSha,
        });
        if (existingSourceId) {
          this.store.addSourceToNotebook({
            studioId: input.studioId,
            notebookId: input.notebookId,
            sourceId: existingSourceId,
          });
          this.store.updateMembershipPath({
            studioId: input.studioId,
            notebookId: input.notebookId,
            sourceId: existingSourceId,
            ...membershipPath,
          });
          result.imported.push({ sourceId: existingSourceId, path: relativePath, reused: true });
          continue;
        }
        const created = await this.importBytesAsSource({
          studioId: input.studioId,
          notebookId: input.notebookId,
          fileName: imported.fileName,
          realPath: imported.realPath,
          bytes: imported.bytes,
          membershipPath,
        });
        result.imported.push({ sourceId: created.source.id, path: relativePath, reused: false });
      } catch (error) {
        if (isKnowledgeError(error)) {
          const code = String(error.code);
          if (code === "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED" || code === "KNOWLEDGE_IMPORT_PROCESSOR_UNAVAILABLE") {
            result.skipped.push({ path: relativePath, reason: code });
          } else {
            result.failed.push({ path: relativePath, reason: code });
          }
        } else {
          result.failed.push({ path: relativePath, reason: "import_failed" });
        }
      }
    }
    if (truncated) {
      result.skipped.push({ path: ".", reason: truncated });
    }
    return result;
  }

  private async importManagedBytes(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceType: "pasted_text" | "web_snapshot";
    displayName: string;
    originMetadata: Record<string, unknown>;
    mimeType: string;
    bytes: Buffer;
  }): Promise<ImportedKnowledgeSource> {
    this.store.getNotebook({ studioId: input.studioId, notebookId: input.notebookId });
    if (input.bytes.length === 0 || input.bytes.length > this.maxImportBytes) {
      throw new KnowledgeError("KNOWLEDGE_IMPORT_TOO_LARGE", "Knowledge source size is invalid");
    }
    const sourceId = this.idGenerator("src");
    const snapshotId = this.idGenerator("snap");
    const relativeStoragePath = path.posix.join("sources", sourceId, `${snapshotId}.bin`);
    const sourceDirectory = path.join(this.sourcesRoot, sourceId);
    const snapshotPath = path.join(sourceDirectory, `${snapshotId}.bin`);
    const temporaryPath = path.join(sourceDirectory, `.${snapshotId}.${crypto.randomBytes(6).toString("hex")}.tmp`);
    fs.mkdirSync(sourceDirectory, { recursive: false, mode: 0o700 });
    let published = false;
    try {
      const handle = await fs.promises.open(temporaryPath, "wx", 0o600);
      try {
        await handle.writeFile(input.bytes);
        await handle.sync();
      } finally {
        await handle.close();
      }
      await fs.promises.rename(temporaryPath, snapshotPath);
      published = true;
      return this.store.createSourceWithSnapshot({
        studioId: input.studioId,
        notebookId: input.notebookId,
        sourceId,
        snapshotId,
        sourceType: input.sourceType,
        displayName: input.displayName,
        originMetadata: input.originMetadata,
        snapshot: {
          sha256: crypto.createHash("sha256").update(input.bytes).digest("hex"),
          mimeType: input.mimeType,
          byteSize: input.bytes.length,
          storagePath: relativeStoragePath,
        },
      });
    } catch (error) {
      await fs.promises.rm(temporaryPath, { force: true }).catch(() => {});
      if (published) await fs.promises.rm(snapshotPath, { force: true }).catch(() => {});
      await fs.promises.rmdir(sourceDirectory).catch(() => {});
      throw error;
    }
  }

  importPastedText(input: {
    studioId: unknown;
    notebookId: unknown;
    text: unknown;
    displayName?: unknown;
  }) {
    if (typeof input.text !== "string" || !input.text.trim() || input.text.includes("\u0000")) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Pasted text must contain valid text");
    }
    const bytes = Buffer.from(input.text, "utf8");
    return this.importManagedBytes({
      studioId: input.studioId,
      notebookId: input.notebookId,
      sourceType: "pasted_text",
      displayName: normalizeDisplayName(input.displayName, "Pasted text"),
      originMetadata: { kind: "pasted_text" },
      mimeType: "text/plain",
      bytes,
    });
  }

  async importWebSnapshot(input: {
    studioId: unknown;
    notebookId: unknown;
    url: unknown;
    displayName?: unknown;
  }) {
    this.store.getNotebook({ studioId: input.studioId, notebookId: input.notebookId });
    const fetched = await this.fetchWebSnapshot(input.url, {
      maxBytes: Math.min(this.maxImportBytes, DEFAULT_WEB_SNAPSHOT_MAX_BYTES),
    });
    if (fetched.bytes.includes(0)) {
      throw new KnowledgeError("KNOWLEDGE_WEB_TYPE_UNSUPPORTED", "Web source contains invalid HTML bytes");
    }
    return this.importManagedBytes({
      studioId: input.studioId,
      notebookId: input.notebookId,
      sourceType: "web_snapshot",
      displayName: normalizeDisplayName(input.displayName, new URL(fetched.finalUrl).hostname),
      originMetadata: {
        kind: "web_snapshot",
        originalUrl: fetched.originalUrl,
        finalUrl: fetched.finalUrl,
        fetchedAt: fetched.fetchedAt,
      },
      mimeType: fetched.mimeType,
      bytes: fetched.bytes,
    });
  }

  async refreshFileSource(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId: unknown;
  }) {
    const entries = this.store.listNotebookSources({
      studioId: input.studioId,
      notebookId: input.notebookId,
    });
    const current = entries.find(entry => entry.source.id === input.sourceId);
    if (!current) throw new KnowledgeError("KNOWLEDGE_NOT_FOUND", "Knowledge source is not in this Notebook");
    if (current.source.sourceType !== "file") {
      throw new KnowledgeError("KNOWLEDGE_CONFLICT", "Only local file sources can be refreshed");
    }
    const originalPath = current.source.originMetadata.originalPath;
    if (typeof originalPath !== "string" || !originalPath) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "File source origin is unavailable");
    }
    const imported = await readSecureKnowledgeImportFile({
      filePath: originalPath,
      lingxiHome: this.lingxiHome,
      maxBytes: this.maxImportBytes,
    });
    const mimeType = resolveSupportedMimeType(imported.fileName, imported.bytes);
    const contentSha = crypto.createHash("sha256").update(imported.bytes).digest("hex");
    if (contentSha === current.snapshot.sha256) {
      return { ...current, changed: false };
    }

    const snapshotId = this.idGenerator("snap");
    const relativeStoragePath = path.posix.join("sources", current.source.id, `${snapshotId}.bin`);
    const sourceDirectory = path.join(this.sourcesRoot, current.source.id);
    const snapshotPath = path.join(sourceDirectory, `${snapshotId}.bin`);
    const temporaryPath = path.join(
      sourceDirectory,
      `.${snapshotId}.${crypto.randomBytes(6).toString("hex")}.tmp`,
    );
    let published = false;
    let committed = false;
    try {
      const handle = await fs.promises.open(temporaryPath, "wx", 0o600);
      try {
        await handle.writeFile(imported.bytes);
        await handle.sync();
      } finally {
        await handle.close();
      }
      await fs.promises.rename(temporaryPath, snapshotPath);
      published = true;
      const snapshot = this.store.createContentSnapshot({
        studioId: input.studioId,
        sourceId: input.sourceId,
        snapshotId,
        sha256: contentSha,
        mimeType,
        byteSize: imported.bytes.length,
        storagePath: relativeStoragePath,
      });
      committed = true;
      let parseArtifact: KnowledgeParseArtifact;
      try {
        parseArtifact = await this.parseSource({
          studioId: input.studioId,
          sourceId: input.sourceId,
        });
      } catch (error) {
        // 新快照解析失败也保证有摄入 job：worker 从 parse 相位重试，
        // 超限后标 failed（显式终态，UI 可手动重试），不允许静默无状态。
        this.ingestion.enqueueSourceIngestion({
          studioId: input.studioId,
          notebookId: input.notebookId,
          sourceId: input.sourceId,
        });
        throw error;
      }
      // 刷新完成即入队摄入（HTTP 调用方立即返回，不等摄入完成）。
      this.ingestion.enqueueSourceIngestion({
        studioId: input.studioId,
        notebookId: input.notebookId,
        sourceId: input.sourceId,
        artifactId: parseArtifact.id,
      });
      return {
        source: current.source,
        snapshot,
        membership: current.membership,
        parseArtifact,
        changed: true,
      };
    } catch (error) {
      await fs.promises.rm(temporaryPath, { force: true }).catch(() => {});
      if (published && !committed) await fs.promises.rm(snapshotPath, { force: true }).catch(() => {});
      throw error;
    }
  }

  private resolveSnapshotPath(snapshot: ContentSnapshot): string {
    const candidate = path.resolve(this.knowledgeRoot, ...snapshot.storagePath.split("/"));
    const rootPrefix = `${path.resolve(this.sourcesRoot)}${path.sep}`;
    if (!candidate.startsWith(rootPrefix)) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Content snapshot storage locator is invalid");
    }
    return candidate;
  }

  readContentSnapshot(input: { studioId: unknown; snapshotId: unknown }): Buffer {
    const snapshot = this.store.getContentSnapshot(input);
    const filePath = this.resolveSnapshotPath(snapshot);
    let bytes: Buffer;
    try {
      bytes = fs.readFileSync(filePath);
    } catch {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Content snapshot bytes are unavailable");
    }
    const actualSha = crypto.createHash("sha256").update(bytes).digest("hex");
    if (bytes.length !== snapshot.byteSize || actualSha !== snapshot.sha256) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Content snapshot integrity check failed");
    }
    return bytes;
  }

  async parseSource(input: { studioId: unknown; sourceId: unknown }): Promise<KnowledgeParseArtifact> {
    const source = this.store.getSource(input);
    this.scopeCompiler.invalidateSource(source.id);
    const snapshot = this.store.getLatestContentSnapshotForSource(input);
    // §五十八：processor 管理的格式（DOCX/XLSX/CSV）把 processor 身份并入解析
    // 配置指纹——processor 版本/配置变化会得到新身份，自然触发重解析。
    const processorPlan = resolveKnowledgeProcessor(snapshot.mimeType);
    const parserConfig = processorPlan
      ? {
        ...KNOWLEDGE_PARSER_CONFIG,
        processor: {
          id: processorPlan.processorId,
          version: processorPlan.processorVersion,
          configHash: processorPlan.processorConfigHash,
        },
      }
      : KNOWLEDGE_PARSER_CONFIG;
    const parserConfigHash = crypto.createHash("sha256")
      .update(JSON.stringify(parserConfig), "utf8")
      .digest("hex");
    const existing = this.store.findParseArtifactByIdentity({
      studioId: input.studioId,
      contentSnapshotId: snapshot.id,
      parserId: KNOWLEDGE_PARSER_ID,
      parserVersion: KNOWLEDGE_PARSER_VERSION,
      parserConfigHash,
    });
    if (existing?.status === "ready" || existing?.status === "needs_ocr") return existing;

    const parseArtifactId = existing?.id || this.idGenerator("parse");
    const artifact = this.store.beginParseArtifact({
      studioId: input.studioId,
      contentSnapshotId: snapshot.id,
      parseArtifactId,
      parserId: KNOWLEDGE_PARSER_ID,
      parserVersion: KNOWLEDGE_PARSER_VERSION,
      parserConfigHash,
    });
    const relativeArtifactPath = path.posix.join(
      "artifacts",
      snapshot.id,
      `${artifact.id}.json`,
    );
    const artifactDirectory = path.join(this.artifactsRoot, snapshot.id);
    const artifactPath = path.join(artifactDirectory, `${artifact.id}.json`);
    const artifactTemporaryPath = path.join(
      artifactDirectory,
      `.${artifact.id}.${crypto.randomBytes(6).toString("hex")}.tmp`,
    );
    let published = false;

    try {
      const bytes = this.readContentSnapshot({ studioId: input.studioId, snapshotId: snapshot.id });
      let parsed: {
        status: "ready" | "needs_ocr";
        warnings: string[];
        semanticText: string;
        blocks: KnowledgeBlockDraft[];
      };
      let processingArtifactId: string | null = null;
      let fidelity: "citation_grade" | "structural" | "semantic_only" = "citation_grade";
      if (processorPlan) {
        // processor 管线：先确保 ProcessingArtifact（二进制 → 结构化文本 +
        // locatorMap），再以其输出重建 blocks；fidelity 只能是 structural。
        const processed = await this.ensureProcessingArtifact({
          studioId: input.studioId,
          snapshot,
          plan: processorPlan,
          bytes,
        });
        processingArtifactId = processed.artifact.id;
        fidelity = processed.fidelity;
        parsed = {
          status: "ready",
          warnings: processed.artifact.warnings,
          semanticText: processed.output.toString("utf8"),
          blocks: processed.blocks,
        };
      } else {
        parsed = await parseCitationGradeSnapshot({ mimeType: snapshot.mimeType, bytes });
      }
      if (parsed.status === "ready" && parsed.blocks.length === 0) {
        throw new Error("empty_document");
      }
      const serialized = Buffer.from(JSON.stringify({
        schemaVersion: 1,
        sourceId: source.id,
        contentSnapshotId: snapshot.id,
        contentSha256: snapshot.sha256,
        parserId: KNOWLEDGE_PARSER_ID,
        parserVersion: KNOWLEDGE_PARSER_VERSION,
        parserConfigHash,
        fidelity,
        processingArtifactId,
        status: parsed.status,
        warnings: parsed.warnings,
        semanticText: parsed.semanticText,
        blocks: parsed.blocks,
      }), "utf8");

      fs.mkdirSync(artifactDirectory, { recursive: true, mode: 0o700 });
      const artifactHandle = await fs.promises.open(artifactTemporaryPath, "wx", 0o600);
      try {
        await artifactHandle.writeFile(serialized);
        await artifactHandle.sync();
      } finally {
        await artifactHandle.close();
      }
      // 仅替换处于 parsing/failed 的同一解析身份留下的未提交文件。
      await fs.promises.rm(artifactPath, { force: true });
      await fs.promises.rename(artifactTemporaryPath, artifactPath);
      published = true;

      return this.store.completeParseArtifact({
        studioId: input.studioId,
        parseArtifactId: artifact.id,
        status: parsed.status,
        warnings: parsed.warnings,
        semanticArtifactPath: relativeArtifactPath,
        blocks: parsed.blocks,
        fidelity,
        processingArtifactId,
      });
    } catch (error) {
      await fs.promises.rm(artifactTemporaryPath, { force: true }).catch(() => {});
      if (published) await fs.promises.rm(artifactPath, { force: true }).catch(() => {});
      const failureReason = error instanceof Error && [
        "invalid_utf8",
        "unsupported_citation_format",
        "empty_document",
      ].includes(error.message)
        ? error.message
        : "parse_failed";
      try {
        this.store.failParseArtifact({
          studioId: input.studioId,
          parseArtifactId: artifact.id,
          warnings: [failureReason],
        });
      } catch {
        // 保留原始失败；下一次启动会从 parsing/failed 身份重试。
      }
      if (isKnowledgeError(error)) throw error;
      throw new KnowledgeError(
        "KNOWLEDGE_PARSE_FAILED",
        "Knowledge source parsing failed",
        { reason: failureReason },
      );
    } finally {
      this.scopeCompiler.invalidateSource(source.id);
    }
  }

  /**
   * §五十八 ProcessingArtifact 确保（幂等）：同一 (snapshot, processor 身份四元组)
   * 已 ready 时直接复用持久化输出 + locatorMap 重建 blocks，绝不重复跑 processor；
   * 否则执行转换并把输出原子落盘到 knowledge/processed/<snapshotId>/<id>.txt。
   * 失败路径 failProcessingArtifact 留痕后由 parseSource 统一归类。
   */
  private async ensureProcessingArtifact(input: {
    studioId: unknown;
    snapshot: ContentSnapshot;
    plan: KnowledgeProcessorPlan;
    bytes: Buffer;
  }): Promise<{
    artifact: KnowledgeProcessingArtifact;
    fidelity: "structural";
    output: Buffer;
    blocks: KnowledgeBlockDraft[];
  }> {
    const { snapshot, plan, bytes } = input;
    const identity = {
      studioId: input.studioId,
      contentSnapshotId: snapshot.id,
      processorId: plan.processorId,
      processorVersion: plan.processorVersion,
      processorConfigHash: plan.processorConfigHash,
    };
    const existing = this.store.findProcessingArtifactByIdentity(identity);
    if (existing?.status === "ready") {
      if (!existing.outputPath || !existing.outputPath.startsWith("processed/")) {
        throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Processing artifact storage locator is invalid");
      }
      const processedOutputFile = path.join(this.knowledgeRoot, existing.outputPath);
      let output: Buffer;
      try {
        output = fs.readFileSync(processedOutputFile);
      } catch {
        throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Processing artifact bytes are unavailable");
      }
      const blocks = rebuildBlocksFromProcessorOutput({ output, locatorMap: existing.locatorMap });
      return { artifact: existing, fidelity: "structural", output, blocks };
    }

    const artifact = this.store.beginProcessingArtifact({
      ...identity,
      processingArtifactId: existing?.id || this.idGenerator("proc"),
    });
    const relativeOutputPath = path.posix.join("processed", snapshot.id, `${artifact.id}.txt`);
    const processedDirectory = path.join(this.processedRoot, snapshot.id);
    const processedOutputPath = path.join(processedDirectory, `${artifact.id}.txt`);
    const processedTemporaryPath = path.join(
      processedDirectory,
      `.${artifact.id}.${crypto.randomBytes(6).toString("hex")}.tmp`,
    );
    let published = false;
    try {
      const processed = await processKnowledgeSnapshot({ mimeType: snapshot.mimeType, bytes });
      if (processed.blocks.length === 0) {
        throw new Error("empty_document");
      }
      fs.mkdirSync(processedDirectory, { recursive: true, mode: 0o700 });
      const handle = await fs.promises.open(processedTemporaryPath, "wx", 0o600);
      try {
        await handle.writeFile(processed.output);
        await handle.sync();
      } finally {
        await handle.close();
      }
      // 仅替换处于 processing/failed 的同一处理身份留下的未提交文件。
      await fs.promises.rm(processedOutputPath, { force: true });
      await fs.promises.rename(processedTemporaryPath, processedOutputPath);
      published = true;

      const completed = this.store.completeProcessingArtifact({
        studioId: input.studioId,
        processingArtifactId: artifact.id,
        fidelity: processed.fidelity,
        outputMime: processed.outputMime,
        outputPath: relativeOutputPath,
        locatorMap: processed.locatorMap,
        warnings: processed.warnings,
      });
      return {
        artifact: completed,
        fidelity: "structural",
        output: processed.output,
        blocks: processed.blocks,
      };
    } catch (error) {
      await fs.promises.rm(processedTemporaryPath, { force: true }).catch(() => {});
      if (published) await fs.promises.rm(processedOutputPath, { force: true }).catch(() => {});
      try {
        this.store.failProcessingArtifact({
          studioId: input.studioId,
          processingArtifactId: artifact.id,
          warnings: [error instanceof Error && error.message === "empty_document" ? "empty_document" : "processing_failed"],
        });
      } catch {
        // 保留原始失败；下一次启动会从 processing/failed 身份重试。
      }
      throw error;
    }
  }

  listArtifactBlocks(input: Parameters<KnowledgeStore["listArtifactBlocks"]>[0]) {
    return this.store.listArtifactBlocks(input);
  }
  /**
   * 分块卡片（GET /api/knowledge/parse-artifacts/:id/chunks 的数据面）：
   * 校验 artifact 归属（studio 隔离经 getParseArtifact 的 JOIN 链）后，
   * 按 owning notebook 的 RetrievalProfile 解析 chunkProfileHash（v9 起索引身份
   * 锚点是 ChunkIndexVariant），走 indexArtifactForIngestion 幂等兜底（纯 chunk+FTS，
   * 不依赖嵌入模型）建出该变体，再按变体列 chunks 并组装 headingPath / pageNumber
   * 定位信息。ordinal 为 1-based 展示序号（与注入块的 [KN] 编号语义一致）。
   */
  listArtifactChunkCards(input: { studioId: unknown; parseArtifactId: unknown }): {
    chunkerConfigId: string;
    chunks: Array<{
      id: string;
      ordinal: number;
      text: string;
      tokenCount: number;
      charCount: number;
      headingPath?: string[];
      pageNumber?: number;
    }>;
  } {
    const artifact = this.store.getParseArtifact({
      studioId: input?.studioId,
      parseArtifactId: input?.parseArtifactId,
    });
    if (artifact.status !== "ready") {
      throw new KnowledgeError("KNOWLEDGE_PARSE_NOT_READY", "Parse artifact is not ready for chunk cards");
    }
    // owning notebook 的生效分块配置（与摄入侧同源）：卡片视图的幂等兜底也必须
    // 按同一 chunkProfileHash 锚定变体，否则打开卡片即以默认 1200 建新变体、
    // 与摄入/查询侧身份打架。策略随 artifact 内容派发，经 resolveNotebookRetrievalProfile
    // 惰性完成 Notebook → RetrievalProfile 绑定。
    const blocks = this.store.listArtifactBlocks({
      studioId: input?.studioId,
      parseArtifactId: artifact.id,
    });
    let chunkProfileHash: string | null = null;
    let chunkTargetChars: number | null = null;
    for (const notebook of this.store.listNotebooks({ studioId: input?.studioId })) {
      const inNotebook = this.store.listNotebookSources({ studioId: input?.studioId, notebookId: notebook.id })
        .some(entry => entry.parseArtifact?.id === artifact.id);
      if (!inNotebook) continue;
      chunkTargetChars = this.getNotebookEffectiveChunkTargetChars({
        studioId: input?.studioId,
        notebookId: notebook.id,
      });
      const strategy = resolveKnowledgeChunkerConfig(blocks, { targetChars: chunkTargetChars }).strategy;
      chunkProfileHash = this.store.resolveNotebookRetrievalProfile({
        studioId: input?.studioId,
        notebookId: notebook.id,
        strategy,
        getEmbeddingModelContextWindow: this.options.getEmbeddingModelContextWindow,
      }).chunkProfile.profileHash;
      break;
    }
    const { chunkerConfigId } = this.queryService.indexArtifactForIngestion(
      String(input?.studioId),
      artifact.id,
      chunkTargetChars != null ? { targetChars: chunkTargetChars } : undefined,
    );
    // 兜底构建与 profile 解析同一 configId；无 owning notebook 时以兜底身份为锚。
    const variant = this.indexStore.resolveChunkIndexVariant(artifact.id, chunkProfileHash ?? chunkerConfigId);
    if (!variant || variant.status !== "ready") {
      throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Knowledge chunk index variant is not ready");
    }
    const chunks = this.indexStore.listVariantChunks(variant.id);
    const locatorIndex = buildKnowledgeBlockLocatorIndex(blocks);
    return {
      chunkerConfigId,
      chunks: chunks.map(chunk => {
        const locator = locatorIndex.get(chunk.spans?.[0]?.blockId ?? "");
        return {
          id: chunk.id,
          ordinal: chunk.ordinal + 1,
          text: chunk.text,
          tokenCount: chunk.tokenCount,
          charCount: chunk.text.length,
          ...(locator?.headingPath ? { headingPath: locator.headingPath } : {}),
          ...(locator?.pageNumber != null ? { pageNumber: locator.pageNumber } : {}),
        };
      }),
    };
  }

  createCitation(input: Parameters<KnowledgeStore["createCitation"]>[0]) {
    return this.store.createCitation(input);
  }

  resolveCitation(input: Parameters<KnowledgeStore["resolveCitation"]>[0]) {
    const resolved = this.store.resolveCitation(input);
    const blockSha = crypto.createHash("sha256").update(resolved.block.text, "utf8").digest("hex");
    const canonicalText = resolved.block.text.slice(
      resolved.citation.startOffset,
      resolved.citation.endOffset,
    );
    const citationSha = crypto.createHash("sha256").update(canonicalText, "utf8").digest("hex");
    if (
      resolved.artifact.status !== "ready"
      || blockSha !== resolved.block.textSha256
      || canonicalText !== resolved.citation.canonicalText
      || citationSha !== resolved.citation.canonicalTextSha256
    ) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Knowledge citation integrity check failed");
    }
    return resolved;
  }

  /** 摄入入队门面：路由/watcher 在源导入或刷新完成后调用；HTTP 立即返回。 */
  enqueueSourceIngestion(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId: unknown;
    artifactId?: unknown;
  }) {
    return this.ingestion.enqueueSourceIngestion(input);
  }

  /**
   * 查询侧后台补齐门面（§十二）：knowledge_read 等读路径发现索引变体未就绪时
   * 调用，幂等入队构建（活跃 job 去重；pending_embedding 且嵌入可解析时置回
   * queued 立即补跑）。查询侧 retrieveForNotebooks 经 queryService 依赖注入
   * 走同一入口。
   */
  requestVariantBuild(input: {
    studioId: unknown;
    notebookId: unknown;
    sourceId: unknown;
    artifactId?: unknown;
  }) {
    return this.ingestion.requestVariantBuild(input);
  }

  /**
   * 启动 file 源 watcher（engine init 调用一次）：从 store 扫描全部活跃 file 源的
   * 活跃 membership 建立 watch 项，然后启动目录 watch + 兜底轮询。幂等。
   * 运行期新增/删除源由 importFile/addSourceToNotebook/removeSourceFromNotebook/
   * deleteNotebook 内的挂钩动态增删，无需再走这里。
   */
  startSourceFileWatcher() {
    for (const row of this.store.listWatchableFileSources()) {
      this.watcher.trackSource({
        studioId: row.studioId,
        notebookId: row.notebookId,
        sourceId: row.sourceId,
        filePath: row.originalPath,
      });
    }
    this.watcher.start();
  }

  /** watch 状态门面（含 unreachable="源文件不可达"标记），供状态端点/测试使用。 */
  listSourceFileWatchStates() {
    return this.watcher.getWatchStates();
  }

  /** 笔记本配置变更后的重建门面：为全部活跃源 ensure 新 profile 的索引变体（旧变体共存不被覆盖）。 */
  enqueueNotebookRebuild(input: { studioId: unknown; notebookId: unknown }) {
    return this.ingestion.enqueueNotebookRebuild(input);
  }

  /**
   * 模型配置可能变更的信号（模型 init/refresh、provider 变更、嵌入偏好变更后由 engine 调用）：
   * 嵌入可解析时把 pending_embedding 批量置回 queued 补跑。
   */
  onModelConfigMayHaveChanged() {
    this.queryService.onModelConfigMayHaveChanged();
    this.searchService.refreshModelConfigurations();
    return this.ingestion.onModelConfigMayHaveChanged();
  }

    /**
   * 生效分块尺寸（只读展示用）：笔记本遗留显式列 > 嵌入模型上下文 ×80%
   * 自动值（窗口查不到回退 8192）。与摄入侧 resolveConfig 同一派生口径。
   */
  getNotebookEffectiveChunkTargetChars(input: { studioId: unknown; notebookId: unknown }): number {
    const resolved = resolveNotebookConfig(
      this.store.getNotebookConfig({ studioId: input?.studioId, notebookId: input?.notebookId }),
    );
    if (resolved.chunkTargetChars != null) return resolved.chunkTargetChars;
    return computeAutoChunkTargetChars(
      resolved.embeddingModelRef
        ? this.options.getEmbeddingModelContextWindow?.(resolved.embeddingModelRef) ?? null
        : null,
    );
  }

  /**
   * 一次性迁移门面（v8 配套）：把已退役的全局嵌入/重排引用写入所有未单独
   * 配置的活跃笔记本。幂等；不触发重建（迁移值与旧解析链结果一致）。
   */
  migrateLegacyGlobalModelRefs(input: {
    embeddingModelRef: import("./types.ts").KnowledgeModelRef | null;
    rerankModelRef: import("./types.ts").KnowledgeModelRef | null;
  }): { notebooksUpdated: number } {
    return this.store.migrateLegacyGlobalModelRefs(input);
  }

/**
   * 笔记本设置更新门面：更新配置后，分块尺寸或嵌入模型引用变化（影响派生产物）
   * → 入队后台重建；rerank/topK 只影响查询时行为，不触发重建。
   * 重建语义（v9 起）：worker 按新配置 ensure 新 profile 的 ChunkIndexVariant /
   * VectorIndexVariant 并后台 build——变体以 (parseArtifactId, chunkProfileHash[,
   * modelKey]) 为身份天然共存，旧 profile 的变体保留、不被覆盖（任务书 §十）；
   * Notebook → RetrievalProfile 绑定切换由 resolveNotebookRetrievalProfile
   * 惰性完成（updateNotebookConfig 内按继承策略先绑一次，逐 artifact 策略在
   * 摄入/查询侧首次 resolve 时再绑）。
   */
  updateNotebookSettings(input: {
    studioId: unknown;
    notebookId: unknown;
    embeddingModelRef?: unknown;
    rerankModelRef?: unknown;
    chunkTargetChars?: unknown;
    retrievalTopK?: unknown;
    vectorRetentionDays?: unknown;
  }) {
    const before = this.store.getNotebookConfig({
      studioId: input?.studioId,
      notebookId: input?.notebookId,
    });
    const after = this.store.updateNotebookConfig(input);
    this.scopeCompiler.invalidateNotebook(String(input.notebookId));
    this.searchService.clearResults();
    const embeddingChanged = JSON.stringify(before.embeddingModelRef ?? null)
      !== JSON.stringify(after.embeddingModelRef ?? null);
    const chunkChanged = (before.chunkTargetChars ?? null) !== (after.chunkTargetChars ?? null);
    if (embeddingChanged || chunkChanged) {
      this.ingestion.enqueueNotebookRebuild({
        studioId: input?.studioId,
        notebookId: input?.notebookId,
      });
    }
    return after;
  }

  getNotebookConfig(input: Parameters<KnowledgeStore["getNotebookConfig"]>[0]) {
    return this.store.getNotebookConfig(input);
  }

  listIngestionJobs(input: Parameters<KnowledgeStore["listIngestionJobs"]>[0]) {
    return this.store.listIngestionJobs(input);
  }

  countIngestionJobsByStatus(input: Parameters<KnowledgeStore["countIngestionJobsByStatus"]>[0]) {
    return this.store.countIngestionJobsByStatus(input);
  }

  getLatestIngestionJobForSource(input: Parameters<KnowledgeStore["getLatestIngestionJobForSource"]>[0]) {
    return this.store.getLatestIngestionJobForSource(input);
  }

  /**
   * 摄入手动重试门面（reingest 端点）：该 notebook+source 最新 job 为 failed 时
   * requeue（attempt 归零、从失败 phase 续跑）并唤醒队列；从未有过 job 时兜底入队。
   * 最新 job 非 failed 时由 store 抛 KNOWLEDGE_CONFLICT（409）。
   */
  requeueSourceIngestion(input: { studioId: unknown; notebookId: unknown; sourceId: unknown }) {
    const latest = this.store.listIngestionJobs({
      studioId: input?.studioId,
      notebookId: input?.notebookId,
      sourceId: input?.sourceId,
      limit: 1,
    })[0] ?? null;
    if (!latest) {
      return { job: this.enqueueSourceIngestion(input), retried: false };
    }
    const job = this.store.requeueIngestionJob({ studioId: input?.studioId, jobId: latest.id });
    this.ingestion.wake();
    return { job, retried: true };
  }

  close() {
    this.searchService.close();
    if (this.metadataBackfill) clearImmediate(this.metadataBackfill);
    this.metadataBackfill = null;
    this.scopeCompiler.dispose();
    for (const pending of this.scopeBuildRequests.values()) clearImmediate(pending);
    this.scopeBuildRequests.clear();
    // 先停生命周期维护（不再触发 GC 扫描），再停 watcher（不再产生 refresh/enqueue），
    // 再停摄入池（abort 进行中的嵌入、被中断 job 留给启动恢复），最后关库。
    if (this.lifecycleGcTimer) {
      clearInterval(this.lifecycleGcTimer);
      this.lifecycleGcTimer = null;
    }
    this.watcher.stop();
    this.ingestion.stop();
    // 先同步停下后台调度，再关事实库；异步退出阶段不再访问这些库。
    const backendClosed = this.vectorSearchBackend.close();
    this.vectorIndex.close();
    this.indexStore.close();
    this.store.close();
    return backendClosed;
  }
}
