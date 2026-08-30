/**
 * knowledge_outline 工具 —— 列当前 KnowledgeTurnScope 冻结集合的结构
 * （Phase 11，任务书 §二十三 Agent Knowledge 工具体系）。
 *
 * 消费方是主模型：拿到 [KnowledgeContext] 注入块（或分片清单）后，先看本轮
 * 知识里有哪些 notebook / source、各自规模（coverage 单元数）与首层 heading
 * 摘要，再决定读哪片（knowledge_read）或检索什么（knowledge_grep）。
 *
 * 权限边界（与 knowledge_read 同链，任务书 §二十~§二十二，校验链在
 * lib/tools/knowledge-scope.ts）：
 * - 只读；studio 隔离（所有 store 查询都带 studioId）；
 * - scopeId 必填且服务端逐次复核：scope 存在、active、属于当前会话（subagent
 *   子会话经 manifest provenance 继承父会话 scope）；
 * - 绝不列出 scope 冻结集合之外的 notebook/source（枚举以 scope 为唯一来源，
 *   不做全 studio 扫描）；
 * - 结构数据锚定 scope 冻结的 parseArtifact（§四十三），从 blocks/headingPath
 *   元数据派生；未解析/未就绪的源按 fidelity 摘要单列（对齐 coverage manifest
 *   语义），不整单失败也不静默省略；
 * - 输出量级有界截断，截断处显式标注（truncated 字段）。
 */
import { Type } from "../pi-sdk/index.ts";
import { fidelityFromLocatorTypes, type CoverageSourceFidelity } from "../knowledge/knowledge-coverage-manifest.ts";
import { buildCoverageUnits } from "../knowledge/knowledge-coverage-unit.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import type { KnowledgeManager } from "../knowledge/knowledge-manager.ts";
import type { KnowledgeBlock } from "../knowledge/types.ts";
import {
  knowledgeBlockHeadingPath,
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
}

/** 冻结 artifact 的结构摘要：fidelity + coverage 单元数 + 首层 heading 列表。 */
function summarizeFrozenArtifact(input: {
  knowledge: KnowledgeManager;
  studioId: string;
  sourceId: string;
  parseArtifactId: string | null;
}): {
  fidelity: CoverageSourceFidelity;
  blockCount: number;
  coverageUnits: number;
  headings: string[];
  totalHeadings: number;
  headingsTruncated: boolean;
} {
  if (input.parseArtifactId == null) {
    return { fidelity: "unavailable", blockCount: 0, coverageUnits: 0, headings: [], totalHeadings: 0, headingsTruncated: false };
  }
  const artifact = input.knowledge.store.getParseArtifact({
    studioId: input.studioId,
    parseArtifactId: input.parseArtifactId,
  });
  if (artifact.status === "needs_ocr") {
    return { fidelity: "needs_ocr", blockCount: 0, coverageUnits: 0, headings: [], totalHeadings: 0, headingsTruncated: false };
  }
  if (artifact.status !== "ready") {
    // 冻结时刻解析未完 / 失败：无可处理文本，fidelity 单列。
    return { fidelity: "unavailable", blockCount: 0, coverageUnits: 0, headings: [], totalHeadings: 0, headingsTruncated: false };
  }
  const blocks: KnowledgeBlock[] = input.knowledge.listArtifactBlocks({
    studioId: input.studioId,
    parseArtifactId: input.parseArtifactId,
  });
  // §五十九：经 ProcessingArtifact 管线的 artifact 以持久化的 fidelity 为准。
  const fidelity = artifact.processingArtifactId
    ? artifact.fidelity
    : fidelityFromLocatorTypes(blocks.map(block => block.locatorType));
  // coverage 单元数是 exhaustive 分母口径（与 coverage manifest 同一切分纯函数），
  // 不依赖检索索引是否已建。
  const coverageUnits = buildCoverageUnits({
    sourceId: input.sourceId,
    parseArtifactId: input.parseArtifactId,
    blocks,
  }).length;
  // 首层 heading 摘要：headingPath[0] 按块序去重。
  const seen = new Set<string>();
  const headings: string[] = [];
  let totalHeadings = 0;
  for (const block of blocks) {
    const first = knowledgeBlockHeadingPath(block)[0];
    if (first == null || seen.has(first)) continue;
    seen.add(first);
    totalHeadings += 1;
    if (headings.length < MAX_HEADINGS_PER_SOURCE) {
      headings.push(first.length > MAX_HEADING_CHARS ? `${first.slice(0, MAX_HEADING_CHARS)}…` : first);
    }
  }
  return {
    fidelity,
    blockCount: blocks.length,
    coverageUnits,
    headings,
    totalHeadings,
    headingsTruncated: totalHeadings > headings.length,
  };
}

export function createKnowledgeOutlineTool(deps: KnowledgeOutlineToolDeps) {
  return {
    name: "knowledge_outline",
    label: "Knowledge Outline",
    description: "List the structure of the current turn's knowledge scope: selected notebooks, their frozen sources "
      + "(name/type/fidelity/coverage unit count) and each source's first-level heading summary. "
      + "Use it after a [KnowledgeContext] block to see what knowledge is available this turn before reading shards "
      + "(knowledge_read) or searching text (knowledge_grep). Only sources inside the scope are ever listed. Read-only.",
    parameters: Type.Object({
      scopeId: Type.String({
        description: "Knowledge turn scope id from the [KnowledgeContext] block header (the Scope line). Required.",
      }),
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
          parentSessionPath: null,
        };
        const scope = resolveKnowledgeTurnScope({ knowledge, studioId, scopeId, sessionContext });

        // 枚举以 scope 为唯一来源：选中 notebooks（按选择顺序）→ 每本引用的冻结源。
        // notebook/source 行在 scope 创建后被并发删除的窗口：逐条标注 error，
        // 不静默省略也不让整单失败（显式降级并标注）。
        const notebooks: Array<Record<string, unknown>> = [];
        for (const notebookId of scope.notebookIds) {
          if (notebooks.length >= MAX_NOTEBOOKS) break;
          let notebookName: string | null = null;
          let notebookError: string | null = null;
          try {
            notebookName = knowledge.getNotebook({ studioId, notebookId }).name;
          } catch (error) {
            notebookError = error instanceof Error ? error.message : String(error);
          }
          const frozenForNotebook = scope.sources.filter(
            frozen => frozen.notebookIds.includes(notebookId),
          );
          const sources: Array<Record<string, unknown>> = [];
          for (const frozen of frozenForNotebook) {
            if (sources.length >= MAX_SOURCES_PER_NOTEBOOK) break;
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
                ...summarizeFrozenArtifact({
                  knowledge,
                  studioId,
                  sourceId: frozen.sourceId,
                  parseArtifactId: frozen.parseArtifactId,
                }),
              };
            } catch (error) {
              entry = {
                ...entry,
                error: error instanceof KnowledgeError
                  ? `${error.code}: ${error.message}`
                  : error instanceof Error ? error.message : String(error),
              };
            }
            sources.push(entry);
          }
          notebooks.push({
            notebookId,
            ...(notebookName != null ? { notebookName } : {}),
            ...(notebookError != null ? { error: notebookError } : {}),
            sources,
            sourcesTruncated: frozenForNotebook.length > sources.length,
          });
        }
        return toolOk(JSON.stringify({
          scopeId,
          turnId: scope.turnId,
          notebooks,
          notebooksTruncated: scope.notebookIds.length > notebooks.length,
          totalSources: scope.sources.length,
        }, null, 2), { scopeId });
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
