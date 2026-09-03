import crypto from "node:crypto";
import { KnowledgeError } from "./errors.ts";
import type { KnowledgeStore } from "./knowledge-store.ts";
import type { KnowledgeIndexStore } from "./knowledge-index-store.ts";
import type { KnowledgeTurnScope } from "./types.ts";

export interface CompiledKnowledgeSource {
  sourceId: string;
  sourceName: string;
  notebookIds: string[];
  contentSnapshotId: string;
  parseArtifactId: string | null;
  chunkProfileHash: string | null;
  chunkIndexVariantId: string | null;
  chunkCount: number;
  firstHeadingPath: string[] | null;
  sectionKeys: string[];
  status: "ready" | "parse_pending" | "needs_ocr" | "index_missing" | "index_building" | "index_failed";
}

export interface CompiledKnowledgeNotebook {
  notebookId: string;
  notebookName: string;
  embeddingModelRef: { provider: string; id: string } | null;
  rerankModelRef: { provider: string; id: string } | null;
  chunkProfileHash: string | null;
  sourceIds: string[];
}

export interface CompiledKnowledgeScope {
  scopeId: string;
  turnId: string;
  sessionPath: string;
  studioId: string;
  notebookIds: string[];
  snapshotHash: string;
  notebooks: CompiledKnowledgeNotebook[];
  sources: CompiledKnowledgeSource[];
  readyChunkVariantIds: string[];
  warnings: string[];
}

interface CompilerDependencies {
  store: KnowledgeStore;
  indexStore: KnowledgeIndexStore;
  requestVariantBuild: (input: {
    studioId: string;
    notebookId: string;
    sourceId: string;
    parseArtifactId: string;
  }) => void;
}

/** 冻结事实只读编译；同一轮的并发读共享一份结果，生命周期变化显式失效。 */
export class ScopeSnapshotCompiler {
  private readonly cache = new Map<string, {
    scope: KnowledgeTurnScope;
    promise: Promise<CompiledKnowledgeScope>;
  }>();
  private disposed = false;

  private readonly deps: CompilerDependencies;

  constructor(deps: CompilerDependencies) { this.deps = deps; }

  compile(scope: KnowledgeTurnScope): Promise<CompiledKnowledgeScope> {
    if (this.disposed || scope.status !== "active") {
      return Promise.reject(new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Knowledge scope is closed"));
    }
    const cached = this.cache.get(scope.id);
    if (cached) {
      if (cached.scope.studioId !== scope.studioId || cached.scope.sessionPath !== scope.sessionPath) {
        return Promise.reject(new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Knowledge scope owner mismatch"));
      }
      return cached.promise;
    }
    // 在开始读取前登记 promise，避免同一轮并发重复读取。
    const promise = Promise.resolve().then(() => this.compileSnapshot(scope));
    const entry = { scope, promise };
    this.cache.set(scope.id, entry);
    void promise.catch(() => {
      if (this.cache.get(scope.id) === entry) this.cache.delete(scope.id);
    });
    return promise;
  }

  invalidateScope(scopeId: string): void {
    this.cache.delete(scopeId);
  }

  invalidateSession(sessionPath: string): void {
    for (const [id, entry] of this.cache) {
      if (entry.scope.sessionPath === sessionPath) this.cache.delete(id);
    }
  }

  invalidateSource(sourceId: string): void {
    for (const [id, entry] of this.cache) {
      if (entry.scope.sources.some(source => source.sourceId === sourceId)) this.cache.delete(id);
    }
  }

  invalidateNotebook(notebookId: string): void {
    for (const [id, entry] of this.cache) {
      if (entry.scope.notebookIds.includes(notebookId)) this.cache.delete(id);
    }
  }

  dispose(): void {
    this.disposed = true;
    this.cache.clear();
  }

  private compileSnapshot(input: KnowledgeTurnScope): CompiledKnowledgeScope {
    if (this.disposed) throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Knowledge scope is closed");
    // 从宿主落库的冻结事实读取，传入对象无法扩大范围或替换产物。
    const scope = this.deps.store.getTurnScope({ scopeId: input.id });
    if (this.disposed || !scope || scope.status !== "active"
      || scope.studioId !== input.studioId || scope.sessionPath !== input.sessionPath) {
      throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Knowledge scope is unavailable");
    }
    const entry = this.cache.get(scope.id);
    if (entry) entry.scope = scope;
    const notebookIds = [...scope.notebookIds].sort();
    const notebooks = notebookIds.map(notebookId => ({
      ...this.deps.store.getNotebookRetrievalProfileSnapshot({ studioId: scope.studioId, notebookId }),
      sourceIds: scope.sources.filter(source => source.notebookIds.includes(notebookId))
        .map(source => source.sourceId).sort(),
    }));
    const notebookById = new Map(notebooks.map(notebook => [notebook.notebookId, notebook]));
    const warnings: string[] = [];
    const readyIds = new Set<string>();
    const identities: Array<{
      sourceId: string;
      contentSnapshotId: string;
      parseArtifactId: string | null;
      notebookIds: string[];
      chunkProfileHash: string | null;
      chunkIndexVariantId: string | null;
    }> = [];
    const sources = [...scope.sources].sort((a, b) => a.sourceId.localeCompare(b.sourceId)).map(frozen => {
      const source = this.deps.store.getSource({ studioId: scope.studioId, sourceId: frozen.sourceId });
      const artifact = frozen.parseArtifactId
        ? this.deps.store.getParseArtifact({ studioId: scope.studioId, parseArtifactId: frozen.parseArtifactId })
        : null;
      if (artifact && artifact.contentSnapshotId !== frozen.contentSnapshotId) {
        throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Frozen parse artifact identity mismatch");
      }
      const row: CompiledKnowledgeSource = {
        sourceId: frozen.sourceId,
        sourceName: source.displayName,
        notebookIds: [...frozen.notebookIds].sort(),
        contentSnapshotId: frozen.contentSnapshotId,
        parseArtifactId: frozen.parseArtifactId,
        chunkProfileHash: null,
        chunkIndexVariantId: null,
        chunkCount: 0,
        firstHeadingPath: null,
        sectionKeys: [],
        status: !artifact || artifact.status === "parsing" ? "parse_pending"
          : artifact.status === "needs_ocr" ? "needs_ocr"
            : artifact.status === "failed" ? "index_failed" : "index_missing",
      };
      // 同源不同配置保留全部变体；单份来源摘要选稳定排序下首个就绪变体。
      const profiles = new Map<string | null, string[]>();
      for (const notebookId of row.notebookIds) {
        const hash = notebookById.get(notebookId)!.chunkProfileHash;
        profiles.set(hash, [...(profiles.get(hash) ?? []), notebookId]);
      }
      for (const [hash, members] of [...profiles].sort(([a], [b]) => (a ?? "").localeCompare(b ?? ""))) {
        const metadata = artifact?.status === "ready" && hash
          ? this.deps.indexStore.getReadyVariantMetadata({ parseArtifactId: artifact.id, chunkProfileHash: hash })
          : null;
        identities.push({
          sourceId: row.sourceId,
          contentSnapshotId: row.contentSnapshotId,
          parseArtifactId: row.parseArtifactId,
          notebookIds: members,
          chunkProfileHash: hash,
          chunkIndexVariantId: metadata?.id ?? null,
        });
        if (metadata) {
          readyIds.add(metadata.id);
          if (row.status !== "ready") {
            row.status = "ready";
            row.chunkProfileHash = hash;
            row.chunkIndexVariantId = metadata.id;
            row.chunkCount = metadata.chunkCount;
            row.firstHeadingPath = metadata.firstHeadingPath;
            row.sectionKeys = metadata.sectionKeys;
          }
          if (metadata.metadataMissing) warnings.push(`${row.sourceId}:${metadata.id}:section_metadata_missing`);
        } else if (artifact?.status === "ready") {
          const variant = hash ? this.deps.indexStore.resolveChunkIndexVariant(artifact.id, hash) : null;
          const status = variant?.status === "building" ? "index_building"
            : variant?.status === "failed" ? "index_failed" : "index_missing";
          if (row.status !== "ready") {
            row.status = status;
            row.chunkProfileHash = hash;
          }
          warnings.push(`${row.sourceId}:${members.join(",")}:${status}`);
          for (const notebookId of members) {
            this.deps.requestVariantBuild({
              studioId: scope.studioId, notebookId, sourceId: row.sourceId, parseArtifactId: artifact.id,
            });
          }
        }
      }
      if (artifact?.status !== "ready") warnings.push(`${row.sourceId}:${row.status}`);
      return row;
    });
    const snapshotHash = crypto.createHash("sha256").update(JSON.stringify({
      scopeId: scope.id, notebookIds, sources: identities,
    })).digest("hex");
    return {
      scopeId: scope.id,
      turnId: scope.turnId,
      sessionPath: scope.sessionPath,
      studioId: scope.studioId,
      notebookIds,
      snapshotHash,
      notebooks,
      sources,
      readyChunkVariantIds: [...readyIds].sort(),
      warnings: [...new Set(warnings)].sort(),
    };
  }
}
