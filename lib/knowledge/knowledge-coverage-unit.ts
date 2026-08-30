/**
 * knowledge-coverage-unit —— Canonical Coverage Unit（任务书 §四十四/§四十五，Phase 9）。
 *
 * EXHAUSTIVE 全文覆盖的事实分母建立在 ParseArtifact / Blocks 之上，不是 Retrieval
 * Variant Chunk：同一 artifact 的多个 chunk profile 变体会重复扫描同一原文。本模块
 * 把冻结 ParseArtifact 的 blocks 切成确定性的 CoverageUnit——某个 artifact 中一个
 * 确定的、不重叠的原文区间，携带 blockId/startOffset/endOffset 可回溯 citation。
 *
 * 覆盖不变量（§四十五）：全部可处理原文恰好被 primary unit 覆盖一次——无遗漏、
 * 无重叠。切分算法按 block 顺序、区间左端点递增推进（每个 unit 的 startOffset =
 * 前一个的 endOffset），构造上保证分区；buildCoverageUnits 末尾用
 * verifyCoverageUnits 全量自检，违例抛错（宁失败不可静默漏覆盖）。
 *
 * needs_ocr / failed artifact 不生成 unit（由 manifest 层进 fidelity 摘要）。
 * 纯函数化可测：无 IO、无状态。
 */
import crypto from "node:crypto";

import { KnowledgeError } from "./errors.ts";
import { CJK_TOKENS_PER_CHAR, NON_CJK_CHARS_PER_TOKEN, estimateTextTokens } from "../llm/estimate-text-tokens.ts";
import type { KnowledgeBlock } from "./types.ts";

/** 单个 CoverageUnit 的 token 预算（常量，§四十五：超预算 block 再切分）。 */
export const COVERAGE_UNIT_TOKEN_BUDGET = 2048;

/** unit id 前缀；后接 64 hex sha256（与 store 的 sha256 列校验同源）。 */
const COVERAGE_UNIT_ID_PREFIX = "cu_";

export interface CoverageUnit {
  /** 确定性 id：'cu_' + sha256(parseArtifactId + blockOrdinal + offsetRange)。 */
  id: string;
  sourceId: string;
  parseArtifactId: string;
  blockId: string;
  blockOrdinal: number;
  /** 区间 [startOffset, endOffset)，相对 block.text（与 citation 偏移同源）。 */
  startOffset: number;
  endOffset: number;
  /** block.text.slice(startOffset, endOffset) 的原文（worker 输入用）。 */
  text: string;
  /** estimateTextTokens(text)：sharding 的贪心装填成本。 */
  tokenEstimate: number;
}

/** 确定性 unit id：同 artifact 同区间必得同 id（重启/重建后 shard 边界一致的前提）。 */
export function coverageUnitId(input: {
  parseArtifactId: string;
  blockOrdinal: number;
  startOffset: number;
  endOffset: number;
}): string {
  const canonical = JSON.stringify([
    input.parseArtifactId,
    input.blockOrdinal,
    input.startOffset,
    input.endOffset,
  ]);
  return COVERAGE_UNIT_ID_PREFIX + crypto.createHash("sha256").update(canonical, "utf8").digest("hex");
}

/** CJK 判定（与 estimate-text-tokens.ts 的区间表同源镜像；该模块未导出判定函数）。 */
function isCjkCodePoint(codePoint: number): boolean {
  return (codePoint >= 0x1100 && codePoint <= 0x11ff)
    || (codePoint >= 0x2e80 && codePoint <= 0x9fff)
    || (codePoint >= 0xac00 && codePoint <= 0xd7af)
    || (codePoint >= 0xf900 && codePoint <= 0xfaff)
    || (codePoint >= 0xfe30 && codePoint <= 0xfe4f)
    || (codePoint >= 0xff00 && codePoint <= 0xffef)
    || codePoint >= 0x20000;
}

/**
 * 从 pos 起找预算内的最右字符边界（独占 end）：逐字符按 CJK/非 CJK 口径累计
 * （与 estimateTextTokens 同源），返回使 [pos, end) 估算不超过 budget 的最大 end。
 * 至少前进 1 个字符（巨型单字符也照送不丢，与 distiller 单块独占一批同纪律）。
 */
function hardCutWithinBudget(text: string, pos: number, budgetTokens: number): number {
  let used = 0;
  let end = pos;
  for (let index = pos; index < text.length; index += 1) {
    const codePoint = text.codePointAt(index)!;
    if (codePoint > 0xffff) index += 1; // 代理对按一个码点计一次
    used += isCjkCodePoint(codePoint) ? CJK_TOKENS_PER_CHAR : 1 / NON_CJK_CHARS_PER_TOKEN;
    if (end > pos && used > budgetTokens) break;
    end = index + 1;
  }
  return Math.min(end, text.length);
}

/**
 * 在 [pos, hardEnd) 内找最靠右的软边界（段落 \n\n → 行 \n → 空白），
 * 使切点尽量落在自然语义边界上；无软边界则用硬边界。返回值为独占 end。
 */
function preferSoftBoundary(text: string, pos: number, hardEnd: number): number {
  const window = text.slice(pos, hardEnd);
  for (const separator of ["\n\n", "\n", " "]) {
    const last = window.lastIndexOf(separator);
    if (last >= 0) return pos + last + separator.length;
  }
  return hardEnd;
}

/**
 * 超预算 block 的切分：从左到右按预算推进，软边界优先。构造上保证：
 * units[0].startOffset = 0；units[i+1].startOffset = units[i].endOffset；
 * 最后一个 endOffset = text.length——即 [0, text.length) 的有序不重叠全覆盖。
 */
function splitBlockIntoUnits(input: {
  sourceId: string;
  parseArtifactId: string;
  block: { id: string; ordinal: number; text: string };
  budgetTokens: number;
}): CoverageUnit[] {
  const { text } = input.block;
  const units: CoverageUnit[] = [];
  let pos = 0;
  while (pos < text.length) {
    const hardEnd = hardCutWithinBudget(text, pos, input.budgetTokens);
    const end = hardEnd >= text.length ? text.length : preferSoftBoundary(text, pos, hardEnd);
    const unitText = text.slice(pos, end);
    units.push({
      id: coverageUnitId({
        parseArtifactId: input.parseArtifactId,
        blockOrdinal: input.block.ordinal,
        startOffset: pos,
        endOffset: end,
      }),
      sourceId: input.sourceId,
      parseArtifactId: input.parseArtifactId,
      blockId: input.block.id,
      blockOrdinal: input.block.ordinal,
      startOffset: pos,
      endOffset: end,
      text: unitText,
      tokenEstimate: estimateTextTokens(unitText),
    });
    pos = end;
  }
  return units;
}

/** 校验输入并切分单个 artifact 的全部 blocks（按 ordinal 升序）。 */
export function buildCoverageUnits(input: {
  sourceId: string;
  parseArtifactId: string;
  blocks: KnowledgeBlock[];
  /** 覆盖缺省 COVERAGE_UNIT_TOKEN_BUDGET。 */
  unitTokenBudget?: number;
}): CoverageUnit[] {
  const budgetTokens = input?.unitTokenBudget ?? COVERAGE_UNIT_TOKEN_BUDGET;
  if (!Number.isSafeInteger(budgetTokens) || budgetTokens <= 0) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "unitTokenBudget must be a positive integer");
  }
  if (!Array.isArray(input?.blocks)) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "blocks must be an array");
  }
  const sorted = [...input.blocks].sort((left, right) => left.ordinal - right.ordinal);
  const units: CoverageUnit[] = [];
  for (const block of sorted) {
    // 空 text 的 block（store 侧不允许，防御性容忍）贡献零区间，不产生 unit。
    if (!block.text || block.text.length === 0) continue;
    if (block.parseArtifactId !== input.parseArtifactId) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "Block does not belong to the parse artifact");
    }
    if (estimateTextTokens(block.text) <= budgetTokens) {
      units.push({
        id: coverageUnitId({
          parseArtifactId: input.parseArtifactId,
          blockOrdinal: block.ordinal,
          startOffset: 0,
          endOffset: block.text.length,
        }),
        sourceId: input.sourceId,
        parseArtifactId: input.parseArtifactId,
        blockId: block.id,
        blockOrdinal: block.ordinal,
        startOffset: 0,
        endOffset: block.text.length,
        text: block.text,
        tokenEstimate: estimateTextTokens(block.text),
      });
      continue;
    }
    units.push(...splitBlockIntoUnits({
      sourceId: input.sourceId,
      parseArtifactId: input.parseArtifactId,
      block: { id: block.id, ordinal: block.ordinal, text: block.text },
      budgetTokens,
    }));
  }
  const verification = verifyCoverageUnits(units, sorted);
  if (!verification.exact) {
    throw new KnowledgeError(
      "KNOWLEDGE_STORAGE_INVALID",
      "Coverage unit construction violated the exact-coverage invariant",
      { gaps: verification.gaps, overlaps: verification.overlaps },
    );
  }
  return units;
}

export interface UnitCoverageVerification {
  /** 全部 block 区间被恰好覆盖一次（无遗漏、无重叠）。 */
  exact: boolean;
  gaps: Array<{ blockOrdinal: number; startOffset: number; endOffset: number }>;
  overlaps: Array<{ blockOrdinal: number; first: CoverageUnit; second: CoverageUnit }>;
}

/**
 * 覆盖不变量的机器可查证明（§四十五）：按 (blockOrdinal, startOffset) 排序后游走，
 * 相邻 unit 必须首尾相接且不交，并从 0 起覆盖到每个 block 的全长。测试与
 * buildCoverageUnits 自检共用。
 */
export function verifyCoverageUnits(
  units: CoverageUnit[],
  blocks: Array<{ ordinal: number; text: string }>,
): UnitCoverageVerification {
  const gaps: UnitCoverageVerification["gaps"] = [];
  const overlaps: UnitCoverageVerification["overlaps"] = [];
  const blockLengths = new Map(blocks.map(block => [block.ordinal, block.text.length]));
  const byBlock = new Map<number, CoverageUnit[]>();
  for (const unit of units) {
    const list = byBlock.get(unit.blockOrdinal) ?? [];
    list.push(unit);
    byBlock.set(unit.blockOrdinal, list);
  }
  for (const [blockOrdinal, blockUnits] of byBlock) {
    const ordered = [...blockUnits].sort((left, right) =>
      left.startOffset - right.startOffset || left.endOffset - right.endOffset);
    let cursor = 0;
    for (const unit of ordered) {
      if (unit.startOffset > cursor) {
        gaps.push({ blockOrdinal, startOffset: cursor, endOffset: unit.startOffset });
      } else if (unit.startOffset < cursor) {
        const previous = ordered.find(candidate =>
          candidate !== unit && candidate.endOffset === cursor && candidate.startOffset <= unit.startOffset);
        overlaps.push({
          blockOrdinal,
          first: previous ?? ordered[0],
          second: unit,
        });
      }
      cursor = Math.max(cursor, unit.endOffset);
    }
    const length = blockLengths.get(blockOrdinal);
    if (length != null && cursor < length) {
      gaps.push({ blockOrdinal, startOffset: cursor, endOffset: length });
    }
  }
  // blocks 中存在但零 unit 覆盖的非空区间（空 text block 除外）也算 gap。
  for (const [blockOrdinal, length] of blockLengths) {
    if (length > 0 && !byBlock.has(blockOrdinal)) {
      gaps.push({ blockOrdinal, startOffset: 0, endOffset: length });
    }
  }
  return { exact: gaps.length === 0 && overlaps.length === 0, gaps, overlaps };
}
