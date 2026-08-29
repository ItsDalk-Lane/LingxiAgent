import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import { isKnowledgeError, KnowledgeError } from "./errors.ts";
import {
  DEFAULT_KNOWLEDGE_IMPORT_MAX_BYTES,
  readSecureKnowledgeImportFile,
} from "./file-import-security.ts";
import { KnowledgeStore, resolveNotebookConfig } from "./knowledge-store.ts";
import { computeAutoChunkTargetChars } from "./chunker.ts";
import { KnowledgeIndexStore } from "./knowledge-index-store.ts";
import {
  KnowledgeIngestionService,
  type KnowledgeIngestionEmbedRequest,
} from "./ingestion-service.ts";
import {
  KnowledgeQueryService,
  buildKnowledgeBlockLocatorIndex,
  type KnowledgeEmbeddingResult,
  type KnowledgeReranker,
} from "./knowledge-query-service.ts";
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
import type {
  ContentSnapshot,
  ImportedKnowledgeSource,
  KnowledgeModelRef,
  KnowledgeParseArtifact,
} from "./types.ts";

export const KNOWLEDGE_PARSER_ID = "lingxi-citation";
export const KNOWLEDGE_PARSER_VERSION = "1";
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
]);

function createId(prefix: string): string {
  return `${prefix}_${crypto.randomUUID()}`;
}

function resolveSupportedMimeType(fileName: string, bytes: Buffer): string {
  const extension = path.extname(fileName).toLowerCase();
  const mimeType = SUPPORTED_FILE_TYPES.get(extension);
  if (!mimeType) {
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
  rerank?: KnowledgeReranker | null;
  /**
   * 摄入管线嵌入回调（engine 用 ModelOperationResolver/EmbeddingClient 按显式
   * 模型引用接线）；缺省时摄入在 embed 相位落显式 pending_embedding。
   */
  embedTextsForModel?: ((request: KnowledgeIngestionEmbedRequest) => Promise<KnowledgeEmbeddingResult | null>) | null;
  canEmbedWithModel?: ((modelRef: KnowledgeModelRef) => boolean) | null;
  /** 查嵌入模型上下文窗口（token 数）：自动分块与生效值展示共用。 */
  getEmbeddingModelContextWindow?: ((modelRef: KnowledgeModelRef) => number | null) | null;
  ingestionLog?: (message: string) => void;
  /** 测试注入：file 源 watcher 的计时器/IO 参数（防抖/退避/轮询时长、watch/stat 工厂）。 */
  fileWatcher?: KnowledgeSourceFileWatcherTuning;
  fetchWebSnapshot?: (
    url: unknown,
    options?: WebSnapshotFetchOptions,
  ) => Promise<WebSnapshotFetchResult>;
}

/**
 * Knowledge 领域入口。数据库、托管原文、解析产物和索引保持物理分离，
 * Engine 只持有这个入口，不承载领域内的事务细节。
 */
export class KnowledgeManager {
  readonly knowledgeRoot: string;
  readonly sourcesRoot: string;
  readonly artifactsRoot: string;
  readonly indexesRoot: string;
  readonly store: KnowledgeStore;
  readonly indexStore: KnowledgeIndexStore;
  readonly vectorIndex: PortableVectorIndexAdapter;
  readonly queryService: KnowledgeQueryService;
  readonly ingestion: KnowledgeIngestionService;
  readonly watcher: KnowledgeSourceFileWatcher;
  private readonly lingxiHome: string;
  private readonly maxImportBytes: number;
  private readonly options: KnowledgeManagerOptions;
  private readonly idGenerator: (prefix: string) => string;
  private readonly fetchWebSnapshot: NonNullable<KnowledgeManagerOptions["fetchWebSnapshot"]>;

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
    this.knowledgeRoot = path.join(this.lingxiHome, "knowledge");
    this.sourcesRoot = path.join(this.knowledgeRoot, "sources");
    this.artifactsRoot = path.join(this.knowledgeRoot, "artifacts");
    this.indexesRoot = path.join(this.knowledgeRoot, "indexes");

    fs.mkdirSync(this.sourcesRoot, { recursive: true, mode: 0o700 });
    fs.mkdirSync(this.artifactsRoot, { recursive: true, mode: 0o700 });
    fs.mkdirSync(this.indexesRoot, { recursive: true, mode: 0o700 });
    this.store = new KnowledgeStore({
      dbPath: path.join(this.knowledgeRoot, "knowledge.db"),
      Database: options.Database,
      now: options.now,
      idGenerator: this.idGenerator,
    });
    this.indexStore = new KnowledgeIndexStore({
      dbPath: path.join(this.indexesRoot, "knowledge-fts.db"),
      Database: options.Database,
      now: options.now,
    });
    this.vectorIndex = new PortableVectorIndexAdapter({
      dbPath: path.join(this.indexesRoot, "knowledge-vector.db"),
      Database: options.Database,
      now: options.now,
    });
    this.queryService = new KnowledgeQueryService({
      store: this.store,
      indexStore: this.indexStore,
      vectorIndex: this.vectorIndex,
      embedTextsForModel: options.embedTextsForModel ?? null,
      rerank: options.rerank,
    });
    this.ingestion = new KnowledgeIngestionService({
      store: this.store,
      queryService: this.queryService,
      parseSource: (input) => this.parseSource(input),
      embedTextsForModel: options.embedTextsForModel ?? null,
      canEmbedWithModel: options.canEmbedWithModel ?? null,
      getEmbeddingModelContextWindow: options.getEmbeddingModelContextWindow ?? null,
      now: options.now,
      log: options.ingestionLog,
    });
    this.watcher = new KnowledgeSourceFileWatcher({
      refresh: (input) => this.refreshFileSource(input),
      enqueueForNotebook: (input) => this.ingestion.enqueueSourceIngestion(input),
      log: options.ingestionLog,
      now: options.now,
      ...options.fileWatcher,
    });
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
    const notebook = this.store.deleteNotebook(input);
    // 笔记本删除后摘掉其全部 watch membership（最后一个 membership 消失即摘 watcher）。
    this.watcher.untrackNotebook(notebook.id);
    return notebook;
  }

  listNotebookSources(input: Parameters<KnowledgeStore["listNotebookSources"]>[0]) {
    return this.store.listNotebookSources(input);
  }

  addSourceToNotebook(input: Parameters<KnowledgeStore["addSourceToNotebook"]>[0]) {
    const membership = this.store.addSourceToNotebook(input);
    // 既有 file 源被加进新笔记本：并入该源的 watch 项（多 membership 共用一个 watcher）。
    const source = this.store.getSource({ studioId: input?.studioId, sourceId: input?.sourceId });
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
    this.watcher.untrackSourceMembership({
      sourceId: membership.sourceId,
      notebookId: membership.notebookId,
    });
    return membership;
  }

  getSource(input: Parameters<KnowledgeStore["getSource"]>[0]) {
    return this.store.getSource(input);
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
    const mimeType = resolveSupportedMimeType(imported.fileName, imported.bytes);
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
        await handle.writeFile(imported.bytes);
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
        displayName: normalizeDisplayName(input.displayName, imported.fileName),
        originMetadata: {
          kind: "local_file",
          fileName: imported.fileName,
          originalPath: imported.realPath,
        },
        snapshot: {
          sha256: crypto.createHash("sha256").update(imported.bytes).digest("hex"),
          mimeType,
          byteSize: imported.bytes.length,
          storagePath: relativeStoragePath,
        },
      });
      // file 源导入成功即挂 watch（外部原文件后续变化 → 自动 refresh + 摄入）。
      this.watcher.trackSource({
        studioId: created.source.studioId,
        notebookId: created.membership.notebookId,
        sourceId: created.source.id,
        filePath: imported.realPath,
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
    const snapshot = this.store.getLatestContentSnapshotForSource(input);
    const parserConfigHash = crypto.createHash("sha256")
      .update(JSON.stringify(KNOWLEDGE_PARSER_CONFIG), "utf8")
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
      const parsed = await parseCitationGradeSnapshot({ mimeType: snapshot.mimeType, bytes });
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
    }
  }

  listArtifactBlocks(input: Parameters<KnowledgeStore["listArtifactBlocks"]>[0]) {
    return this.store.listArtifactBlocks(input);
  }

  /**
   * 分块卡片（GET /api/knowledge/parse-artifacts/:id/chunks 的数据面）：
   * 校验 artifact 归属（studio 隔离经 getParseArtifact 的 JOIN 链）后，
   * 走 indexArtifactForIngestion 幂等兜底（纯 chunk+FTS，不依赖嵌入模型），
   * 再组装每 chunk 的 headingPath / pageNumber 定位信息。
   * ordinal 为 1-based 展示序号（与注入块的 [KN] 编号语义一致）。
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
    const { chunkerConfigId } = this.queryService.indexArtifactForIngestion(
      String(input?.studioId),
      artifact.id,
    );
    const chunks = this.indexStore.listArtifactChunks(artifact.id);
    const locatorIndex = buildKnowledgeBlockLocatorIndex(
      this.store.listArtifactBlocks({ studioId: input?.studioId, parseArtifactId: artifact.id }),
    );
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

  /** 笔记本配置变更后的全量重建门面。 */
  enqueueNotebookRebuild(input: { studioId: unknown; notebookId: unknown }) {
    return this.ingestion.enqueueNotebookRebuild(input);
  }

  /**
   * 模型配置可能变更的信号（模型 init/refresh、provider 变更、嵌入偏好变更后由 engine 调用）：
   * 嵌入可解析时把 pending_embedding 批量置回 queued 补跑。
   */
  onModelConfigMayHaveChanged() {
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
   * → 全量重建该笔记本全部源；rerank/topK 只影响查询时行为，不触发重建。
   */
  updateNotebookSettings(input: {
    studioId: unknown;
    notebookId: unknown;
    embeddingModelRef?: unknown;
    rerankModelRef?: unknown;
    chunkTargetChars?: unknown;
    retrievalTopK?: unknown;
  }) {
    const before = this.store.getNotebookConfig({
      studioId: input?.studioId,
      notebookId: input?.notebookId,
    });
    const after = this.store.updateNotebookConfig(input);
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
    // 先停 watcher（不再产生 refresh/enqueue），再停摄入队列（abort 进行中的嵌入、
    // 被中断 job 留给启动恢复），最后关库。
    this.watcher.stop();
    this.ingestion.stop();
    this.vectorIndex.close();
    this.indexStore.close();
    this.store.close();
  }
}
