/** 列出冻结范围的已编译目录；普通目录查询不计算完整性覆盖单元。 */
import { Type } from "../pi-sdk/index.ts";
import { fidelityFromLocatorTypes, type CoverageSourceFidelity } from "../knowledge/knowledge-coverage-manifest.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import type { KnowledgeResearchToolContext } from "../knowledge/evidence-receipt-service.ts";
import type { KnowledgeManager } from "../knowledge/knowledge-manager.ts";
import { ResearchStore } from "../knowledge/research/research-store.ts";
import { resolveReadyKnowledgeQueryVariant, type CompiledKnowledgeSource } from "../knowledge/scope-snapshot-compiler.ts";
import {
  knowledgeScopeViolation,
  resolveKnowledgeTurnScope,
  type KnowledgeToolSessionContext,
} from "./knowledge-scope.ts";
import { toolError, toolOk } from "./tool-result.ts";

/** 防护上限：单源首层 heading 摘要条数（超出截断并标注）。 */
const MAX_HEADINGS_PER_SOURCE = 40;
/** 单条 heading 截断长度。 */
const MAX_HEADING_CHARS = 120;
/** 防护上限：单 notebook 列出的冻结源条数。 */
const MAX_SOURCES_PER_NOTEBOOK = 100;
/** 防护上限：列出的 notebook 条数。 */
const MAX_NOTEBOOKS = 50;

export interface KnowledgeOutlineToolDeps {
  /** engine 级 KnowledgeManager（跨会话）；null = Knowledge 不可用。 */
  getKnowledge: () => KnowledgeManager | null;
  /** 当前 runtime studioId；null = 运行时上下文不可用。 */
  getStudioId: () => string | null;
  /** 工具执行会话的 scope 归属上下文（与 knowledge_read 同一接线契约）。 */
  resolveSessionContext?: (ctx: unknown) => KnowledgeToolSessionContext;
  resolveResearchContext?: (ctx: unknown) => KnowledgeResearchToolContext | null;
}

/** 保留冻结原文的可信度；目录大小与标题来自索引元数据，不重新拆覆盖单元。 */
function summarizeFrozenArtifact(knowledge: KnowledgeManager, studioId: string, source: CompiledKnowledgeSource,
  chunkProfileHash: string | null, readyChunkVariantIds: string[], sectionOffset: number, limit: number) {
  const artifact = source.parseArtifactId ? knowledge.store.getParseArtifact({ studioId, parseArtifactId: source.parseArtifactId }) : null;
  const blockMetadata = artifact?.status === "ready"
    ? knowledge.store.getArtifactBlockMetadata({ studioId, parseArtifactId: artifact.id }) : { blockCount: 0, locatorTypes: [] };
  const fidelity: CoverageSourceFidelity = !artifact || artifact.status === "failed" || artifact.status === "parsing" ? "unavailable"
    : artifact.status === "needs_ocr" ? "needs_ocr" : artifact.processingArtifactId ? artifact.fidelity : fidelityFromLocatorTypes(blockMetadata.locatorTypes);
  const metadata = artifact?.status === "ready" && chunkProfileHash
    ? resolveReadyKnowledgeQueryVariant({ store: knowledge.store, indexStore: knowledge.indexStore,
      parseArtifactId: artifact.id, chunkProfileHash, readyChunkVariantIds }) : null;
  const sectionKeys = metadata?.sectionKeys ?? [];
  const sections = metadata && artifact ? knowledge.indexStore.listArtifactSectionMetadata(artifact.id) : [];
  const page = sections.slice(sectionOffset, sectionOffset + limit);
  const totalSections = sections.length || sectionKeys.length;
  const pageKeys = sections.length ? page.map(section => section.headingPath.join(" > "))
    : sectionKeys.slice(sectionOffset, sectionOffset + limit);
  const headings = [...new Set(pageKeys)].map(heading => heading.length > MAX_HEADING_CHARS
    ? `${heading.slice(0, MAX_HEADING_CHARS)}…` : heading);
  return { fidelity, status: metadata ? "ready" : source.status === "ready" ? "index_missing" : source.status,
    blockCount: blockMetadata.blockCount, chunkCount: metadata?.chunkCount ?? 0,
    chunkIndexVariantId: metadata?.id ?? null, firstHeadingPath: metadata?.firstHeadingPath ?? null,
    sections: page.map(section => ({ sectionId: section.id, ordinal: section.sectionOrdinal + 1,
      headingPath: section.headingPath, startBlockOrdinal: section.startBlockOrdinal, endBlockOrdinal: section.endBlockOrdinal })),
    sectionKeys: pageKeys, sectionOffset, totalSections,
    sectionsTruncated: sectionOffset + limit < totalSections,
    nextSectionOffset: sectionOffset + limit < totalSections ? sectionOffset + limit : null,
    headings, totalHeadings: totalSections, headingsTruncated: sectionOffset + limit < totalSections,
    metadataMissing: metadata?.metadataMissing ?? true };
}

export function createKnowledgeOutlineTool(deps: KnowledgeOutlineToolDeps) {
  return {
    name: "knowledge_outline",
    label: "Knowledge Outline",
    description: "List the frozen knowledge scope's notebooks and sources, indexed chunk counts, heading summaries, section keys and fidelity/status. "
      + "Read-only metadata lookup. This outline does not prove complete coverage. Use knowledge_search, knowledge_read or knowledge_grep to inspect the original evidence."
      + "目录支持翻页：指定 sourceId 和返回的 nextSectionOffset 继续查看后续章节；每页 sections 中的 sectionId 可以直接阅读。资料列表用 notebookId 和 nextSourceOffset 续查。",
    parameters: Type.Object({
      scopeId: Type.String({
        description: "Knowledge turn scope id from the [KnowledgeContext] block header (the Scope line). Required.",
      }),
      notebookId: Type.Optional(Type.String()),
      sourceId: Type.Optional(Type.String()),
      notebookOffset: Type.Optional(Type.Integer({ minimum: 0 })),
      sourceOffset: Type.Optional(Type.Integer({ minimum: 0 })),
      sectionOffset: Type.Optional(Type.Integer({ minimum: 0 })),
      limit: Type.Optional(Type.Integer({ minimum: 1, maximum: 100 })),
    }),
    sessionPermission: {
      // 只读、无副作用：枚举 scope 冻结集合的结构元数据，不做任何写入或外部请求。
      resolveInvocation: () => ({
        action: "read",
        kind: "read",
        capability: "knowledge_outline.read",
      }),
    },
    execute: async (_toolCallId: any, params: Record<string, any> = {}, _signal?: any, _onUpdate?: any, ctx?: any) => {
      const knowledge = deps.getKnowledge();
      const studioId = deps.getStudioId();
      if (!knowledge || !studioId) {
        return toolError("knowledge_outline unavailable: Knowledge is not accessible in this runtime.", {
          errorCode: "KNOWLEDGE_MODEL_UNAVAILABLE",
        });
      }
      try {
        // scopeId 缺失 = 契约违例：显式拒绝，不得回落到旧的全 studio 行为（§二十二）。
        const scopeId = typeof params.scopeId === "string" && params.scopeId.trim()
          ? params.scopeId.trim()
          : null;
        if (!scopeId) {
          throw knowledgeScopeViolation(
            "scopeId is required: pass the scope id from the [KnowledgeContext] block header (Scope line)",
          );
        }
        const sessionContext = deps.resolveSessionContext?.(ctx) ?? {
          sessionPath: null,
          scopeOwnerSessionPath: null,
        };
        const scope = resolveKnowledgeTurnScope({ knowledge, studioId, scopeId, sessionContext });
        const notebookOffset = params.notebookOffset ?? 0, sourceOffset = params.sourceOffset ?? 0;
        const sectionOffset = params.sectionOffset ?? 0, limit = params.limit ?? MAX_HEADINGS_PER_SOURCE;
        if (![notebookOffset, sourceOffset, sectionOffset].every(value => Number.isSafeInteger(value) && value >= 0)
          || !Number.isSafeInteger(limit) || limit < 1 || limit > 100) {
          throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "目录位置必须是非负整数，每页数量为 1 到 100。");
        }
        if ((params.notebookId != null && !scope.notebookIds.includes(params.notebookId))
          || (params.sourceId != null && !scope.sources.some(source => source.sourceId === params.sourceId))) {
          throw knowledgeScopeViolation("目录筛选超出本轮资料范围。");
        }
        const researchContext = deps.resolveResearchContext?.(ctx) ?? null;
        if (researchContext) {
          const run = new ResearchStore(knowledge.store).requireRun(researchContext.runId);
          if (run.turnScopeId !== scopeId || !["planning", "running", "synthesizing"].includes(run.status)
            || (researchContext.allowedSourceIds !== undefined
              && (!Array.isArray(researchContext.allowedSourceIds)
                || researchContext.allowedSourceIds.some(id => !scope.sources.some(source => source.sourceId === id))))) {
            throw knowledgeScopeViolation("Knowledge outline is outside the research scope");
          }
        }
        const compiled = await knowledge.compileTurnScope(scope);
        const allowedSources = researchContext?.allowedSourceIds === undefined
          ? null : new Set(researchContext.allowedSourceIds);
        const visibleSources = allowedSources ? compiled.sources.filter(source => allowedSources.has(source.sourceId)) : compiled.sources;
        if (params.sourceId != null && !visibleSources.some(source => source.sourceId === params.sourceId)) {
          throw knowledgeScopeViolation("目录筛选超出当前分配的资料范围。");
        }
        // 编译警告以来源编号开头，不能借警告暴露未分配的来源。
        const visibleWarnings = allowedSources ? compiled.warnings.filter(warning => allowedSources.has(warning.split(":", 1)[0])) : compiled.warnings;

        // 枚举只使用已编译的冻结目录，不扫描范围外来源。
        // 编译完成后来源被并发删除时，逐条标注错误，不静默省略。
        const notebooks: Array<Record<string, unknown>> = [];
        let returnedBytes = 0;
        const selectedNotebooks = compiled.notebooks.filter(notebook => !params.notebookId || notebook.notebookId === params.notebookId);
        for (const notebook of selectedNotebooks.slice(notebookOffset, notebookOffset + MAX_NOTEBOOKS)) {
          const notebookId = notebook.notebookId;
          if (notebooks.length >= MAX_NOTEBOOKS || returnedBytes >= 20_000) break;
          const frozenForNotebook = visibleSources.filter(source => source.notebookIds.includes(notebookId)
            && (!params.sourceId || source.sourceId === params.sourceId));
          const sources: Array<Record<string, unknown>> = [];
          for (const frozen of frozenForNotebook.slice(sourceOffset, sourceOffset + MAX_SOURCES_PER_NOTEBOOK)) {
            if (sources.length >= MAX_SOURCES_PER_NOTEBOOK || returnedBytes >= 20_000) break;
            let entry: Record<string, unknown> = {
              sourceId: frozen.sourceId,
              contentSnapshotId: frozen.contentSnapshotId,
              parseArtifactId: frozen.parseArtifactId,
            };
            try {
              const source = knowledge.getSource({ studioId, sourceId: frozen.sourceId });
              entry = {
                ...entry,
                sourceName: source.displayName,
                sourceType: source.sourceType,
                ...summarizeFrozenArtifact(knowledge, studioId, frozen, notebook.chunkProfileHash,
                  compiled.readyChunkVariantIds, sectionOffset, params.sourceId ? limit : Math.min(limit, 3)),
              };
            } catch (error) {
              entry = {
                ...entry,
                error: error instanceof KnowledgeError
                  ? `${error.code}: ${error.message}`
                  : error instanceof Error ? error.message : String(error),
              };
            }
            const entryBytes = Buffer.byteLength(JSON.stringify(entry), "utf8");
            if (returnedBytes > 0 && returnedBytes + entryBytes > 24_000) break;
            sources.push(entry);
            returnedBytes += entryBytes;
          }
          notebooks.push({
            notebookId,
            notebookName: notebook.notebookName,
            sources,
            totalSources: frozenForNotebook.length, sourceOffset,
            sourcesTruncated: sourceOffset + sources.length < frozenForNotebook.length,
            nextSourceOffset: sourceOffset + sources.length < frozenForNotebook.length ? sourceOffset + sources.length : null,
          });
        }
        return toolOk(JSON.stringify({
          scopeId,
          turnId: scope.turnId,
          notebooks,
          notebooksTruncated: notebookOffset + notebooks.length < selectedNotebooks.length,
          nextNotebookOffset: notebookOffset + notebooks.length < selectedNotebooks.length ? notebookOffset + notebooks.length : null,
          totalSources: visibleSources.length,
          warnings: visibleWarnings,
        }), { scopeId });
      } catch (error) {
        if (isKnowledgeError(error)) {
          return toolError(`knowledge_outline failed: ${error.code}: ${error.message}`, {
            errorCode: error.code,
          });
        }
        const message = error instanceof Error ? error.message : String(error);
        return toolError(`knowledge_outline failed: ${message}`, {
          errorCode: "KNOWLEDGE_INTERNAL_ERROR",
        });
      }
    },
  };
}
