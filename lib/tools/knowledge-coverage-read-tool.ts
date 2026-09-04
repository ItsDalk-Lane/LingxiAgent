import { Type } from "../pi-sdk/index.ts";
import { isKnowledgeError, KnowledgeError } from "../knowledge/errors.ts";
import type { KnowledgeCompletenessExecutor } from "../knowledge/research/knowledge-completeness-executor.ts";
import { requireResearchToolContext, type KnowledgeResearchToolDeps } from "../knowledge/research/research-tool-budget.ts";
import { toolError, toolOk } from "./tool-result.ts";

type CompletenessToolDeps = KnowledgeResearchToolDeps & {
  completeness: Pick<KnowledgeCompletenessExecutor, "readAssignedShard" | "markAssignedUnits">;
};

function invalid(): never { throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Invalid coverage read arguments"); }
function scopeViolation(): never { throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Coverage read requires its assigned completeness shard"); }

/** 完整性工作会话只能读取宿主分配的分片，原文凭据由宿主按冻结位置签发。 */
export function createKnowledgeCoverageReadTool(deps: CompletenessToolDeps) {
  return {
    name: "knowledge_coverage_read",
    label: "Knowledge Coverage Read",
    description: "读取宿主分配的完整性检查分片，返回每个覆盖单元的位置、原文和阅读凭据。",
    parameters: Type.Object({ runId: Type.String(), checkId: Type.String(), shardId: Type.String() }, { additionalProperties: false }),
    sessionPermission: {
      resolveInvocation: () => ({ action: "read", kind: "read", capability: "knowledge_coverage_read.read" }),
    },
    execute: async (_toolCallId: string, params: Record<string, unknown> = {}, signal?: AbortSignal, _onUpdate?: unknown, ctx?: unknown) => {
      try {
        signal?.throwIfAborted();
        const context = requireResearchToolContext(deps, ctx, params?.runId);
        if (context.role !== "worker" || !context.completenessCheckId || !context.completenessShardId
          || context.completenessCheckId !== params?.checkId || context.completenessShardId !== params?.shardId) scopeViolation();
        return await deps.budget.execute({ context, toolName: "knowledge_coverage_read", requestSummary: {}, signal }, activeSignal => {
          if (!params || typeof params !== "object" || Array.isArray(params)
            || ![Object.prototype, null].includes(Object.getPrototypeOf(params))
            || Object.keys(params).some(key => !["runId", "checkId", "shardId"].includes(key))) invalid();
          for (const key of ["runId", "checkId", "shardId"]) {
            if (typeof params[key] !== "string" || !params[key].trim() || params[key].length > 128) invalid();
          }
          activeSignal.throwIfAborted();
          const result = deps.completeness.readAssignedShard(context, {
            runId: params.runId as string, checkId: params.checkId as string, shardId: params.shardId as string,
          });
          activeSignal.throwIfAborted();
          // 正文只进入工具的可读内容，动作台账只保留凭据编号和数量。
          const units = result.units.map(unit => ({ unitId: unit.unitId, sourceId: unit.sourceId, contentSnapshotId: unit.contentSnapshotId,
            parseArtifactId: unit.parseArtifactId, blockId: unit.blockId, startOffset: unit.startOffset, endOffset: unit.endOffset,
            sectionKey: unit.sectionKey, status: unit.status,
            ...(unit.receiptId === undefined ? {} : { receiptId: unit.receiptId }) }));
          return { value: toolOk(result.text, { runId: result.runId, checkId: result.checkId, shardId: result.shardId, units }),
            summary: { receiptIds: units.flatMap(unit => unit.receiptId === undefined ? [] : [unit.receiptId]),
              count: units.length, status: "completed" } };
        });
      } catch (error) {
        signal?.throwIfAborted();
        if (error instanceof Error && error.name === "AbortError") throw error;
        if (isKnowledgeError(error)) return toolError(`knowledge_coverage_read failed: ${error.code}`, { errorCode: error.code });
        return toolError("knowledge_coverage_read failed: retrieval unavailable", { errorCode: "KNOWLEDGE_RETRIEVAL_UNAVAILABLE" });
      }
    },
  };
}
