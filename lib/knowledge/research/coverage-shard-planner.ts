import crypto from "node:crypto";
import { estimateTextTokens } from "../../llm/estimate-text-tokens.ts";
import { buildWarningLine, markUntrusted, scan } from "../../security/injection-scan.ts";
import { KnowledgeError } from "../errors.ts";
import type { CoverageUnit } from "../knowledge-coverage-unit.ts";

export const KNOWLEDGE_COMPLETENESS_SHARD_MAX_TOKENS = 12000;

export interface CoverageShard {
  id: string;
  checkId: string;
  ordinal: number;
  units: CoverageUnit[];
  tokenEstimate: number;
}

// 实际凭据最长128字符；规划时按中文字的较高成本预留，不提前签发未交给模型的凭据。
const RECEIPT_ID_RESERVATION = "凭".repeat(128);

/** 阅读工具直接返回此纯文本；正文只加警告和边界，不经 JSON 转义、清洗或截断。 */
export function renderCoverageShard(input: {
  runId: string;
  checkId: string;
  shardId: string;
  units: readonly CoverageUnit[];
  receiptIds?: Record<string, string>;
}): string {
  const units = input.units.map(unit => {
    const receiptId = input.receiptIds === undefined ? RECEIPT_ID_RESERVATION : input.receiptIds[unit.id];
    if (typeof receiptId !== "string" || receiptId.length === 0 || receiptId.length > 128
      || (input.receiptIds !== undefined && !Object.hasOwn(input.receiptIds, unit.id))) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Coverage unit requires a valid receipt identity", { unitId: unit.id });
    }
    const warning = buildWarningLine(scan(unit.text).decision);
    return [
      `unitId: ${unit.id}; sourceId: ${unit.sourceId}; parseArtifactId: ${unit.parseArtifactId}; blockId: ${unit.blockId}`,
      `offsets: ${unit.startOffset}-${unit.endOffset}; receiptId: ${receiptId}`,
      markUntrusted(warning ? `${warning}\n${unit.text}` : unit.text),
    ].join("\n");
  });
  return ["[KnowledgeCoverageShard]", `runId: ${input.runId}; checkId: ${input.checkId}; shardId: ${input.shardId}`,
    ...units, "[/KnowledgeCoverageShard]"].join("\n\n");
}

/** 原覆盖单元按输入顺序完整装填；计入实际阅读格式的全部身份、警告、边界与凭据开销。 */
export function planCoverageShards(input: {
  runId: string;
  checkId: string;
  units: readonly CoverageUnit[];
}): CoverageShard[] {
  const shards: CoverageShard[] = [], seen = new Set<string>();
  const makeShard = (units: CoverageUnit[], ordinal: number): CoverageShard => {
    const id = "kcs_" + crypto.createHash("sha256").update(JSON.stringify([input.checkId, units.map(unit => unit.id)])).digest("hex");
    const tokenEstimate = estimateTextTokens(renderCoverageShard({ runId: input.runId, checkId: input.checkId, shardId: id, units }));
    return { id, checkId: input.checkId, ordinal, units, tokenEstimate };
  };
  let current: CoverageShard | null = null;
  for (const unit of input.units) {
    if (seen.has(unit.id)) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Coverage unit cannot occur in more than one shard", { unitId: unit.id });
    }
    seen.add(unit.id);
    const candidate = makeShard([...(current?.units ?? []), unit], shards.length);
    if (candidate.tokenEstimate <= KNOWLEDGE_COMPLETENESS_SHARD_MAX_TOKENS) { current = candidate; continue; }
    if (current) shards.push(current);
    current = makeShard([unit], shards.length);
    if (current.tokenEstimate > KNOWLEDGE_COMPLETENESS_SHARD_MAX_TOKENS) {
      // 不二次切分、跳过或照送超限单元；执行器保留这个分母并登记无法检查的原因。
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Coverage unit exceeds the completeness shard token budget",
        { unitId: unit.id, estimatedTokens: current.tokenEstimate, maxTokens: KNOWLEDGE_COMPLETENESS_SHARD_MAX_TOKENS });
    }
  }
  if (current) shards.push(current);
  return shards;
}
