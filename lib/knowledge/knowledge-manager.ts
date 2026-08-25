import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import { isKnowledgeError, KnowledgeError } from "./errors.ts";
import {
  DEFAULT_KNOWLEDGE_IMPORT_MAX_BYTES,
  readSecureKnowledgeImportFile,
} from "./file-import-security.ts";
import { KnowledgeStore } from "./knowledge-store.ts";
import { KnowledgeIndexStore } from "./knowledge-index-store.ts";
import {
  KnowledgeQueryService,
  type KnowledgeEmbedder,
  type KnowledgeReranker,
  type KnowledgeTextGenerator,
} from "./knowledge-query-service.ts";
import { PortableVectorIndexAdapter } from "./vector-index-adapter.ts";
import { KnowledgeResearchStore } from "./research-store.ts";
import { KnowledgeResearchService } from "./research-service.ts";
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
  generateText?: KnowledgeTextGenerator | null;
  embedTexts?: KnowledgeEmbedder | null;
  rerank?: KnowledgeReranker | null;
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
  readonly researchStore: KnowledgeResearchStore;
  readonly researchService: KnowledgeResearchService;
  private readonly lingxiHome: string;
  private readonly maxImportBytes: number;
  private readonly idGenerator: (prefix: string) => string;
  private readonly fetchWebSnapshot: NonNullable<KnowledgeManagerOptions["fetchWebSnapshot"]>;

  constructor(options: KnowledgeManagerOptions) {
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
    const researchNow = options.now || (() => new Date().toISOString());
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
      embedTexts: options.embedTexts,
      rerank: options.rerank,
      generateText: options.generateText,
    });
    this.researchStore = new KnowledgeResearchStore({
      db: this.store.db,
      now: researchNow,
      idGenerator: this.idGenerator,
    });
    this.researchService = new KnowledgeResearchService({
      store: this.store,
      researchStore: this.researchStore,
      generateText: options.generateText,
      prioritizeScope: (input) => this.queryService.prioritizeResearchScope(input),
      idGenerator: this.idGenerator,
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
    return this.store.deleteNotebook(input);
  }

  listNotebookSources(input: Parameters<KnowledgeStore["listNotebookSources"]>[0]) {
    return this.store.listNotebookSources(input);
  }

  addSourceToNotebook(input: Parameters<KnowledgeStore["addSourceToNotebook"]>[0]) {
    return this.store.addSourceToNotebook(input);
  }

  removeSourceFromNotebook(input: Parameters<KnowledgeStore["removeSourceFromNotebook"]>[0]) {
    return this.store.removeSourceFromNotebook(input);
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

      return this.store.createSourceWithSnapshot({
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
      const parseArtifact = await this.parseSource({
        studioId: input.studioId,
        sourceId: input.sourceId,
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

  async runQuickAnswer(input: Parameters<KnowledgeQueryService["runQuickAnswer"]>[0]) {
    const result = await this.queryService.runQuickAnswer(input);
    return {
      ...result,
      citations: result.run.citations.map(ref => this.resolveCitation({
        studioId: result.scope.studioId,
        citationId: ref.citationId,
      })),
    };
  }

  attachTaskRegistry(taskRegistry: any) {
    this.researchService.attachTaskRegistry(taskRegistry);
  }

  resumeResearchRuns() {
    return this.researchService.resumeRecoveringRuns();
  }

  startResearch(input: Parameters<KnowledgeResearchService["startResearch"]>[0]) {
    return this.researchService.startResearch(input);
  }

  getResearchRun(input: Parameters<KnowledgeResearchService["getResearchRun"]>[0]) {
    return this.researchService.getResearchRun(input);
  }

  getResearchReport(input: Parameters<KnowledgeResearchService["getReport"]>[0]) {
    return this.researchService.getReport(input);
  }

  listActiveResearchRuns(input: Parameters<KnowledgeResearchService["listActiveResearchRuns"]>[0]) {
    return this.researchService.listActiveResearchRuns(input);
  }

  cancelResearch(input: Parameters<KnowledgeResearchService["cancel"]>[0]) {
    return this.researchService.cancel(input);
  }

  waitForResearch(runId: string) {
    return this.researchService.waitForRun(runId);
  }

  getKnowledgeRun(input: Parameters<KnowledgeStore["getKnowledgeRun"]>[0]) {
    return this.store.getKnowledgeRun(input);
  }

  getScopeSnapshot(input: Parameters<KnowledgeStore["getScopeSnapshot"]>[0]) {
    return this.store.getScopeSnapshot(input);
  }

  close() {
    this.researchService.suspendForShutdown();
    this.vectorIndex.close();
    this.indexStore.close();
    this.store.close();
  }
}
