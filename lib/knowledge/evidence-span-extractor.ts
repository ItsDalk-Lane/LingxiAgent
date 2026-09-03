import crypto from "node:crypto";
import { estimateTextTokens } from "../llm/estimate-text-tokens.ts";
import { tokenizeSearchText } from "../search/search-text.ts";
import type { KnowledgeEvidenceSpan } from "../../shared/knowledge-evidence.ts";
import { KnowledgeError } from "./errors.ts";
import { KNOWLEDGE_FAST_PER_SPAN_MAX_TOKENS, type FastKnowledgeEvidenceStages } from "./fast-knowledge-pipeline.ts";
import type { KnowledgeStore } from "./knowledge-store.ts";

/** 返回合法 UTF-16 边界；不能把一个补充平面字符从代理对中间截开。 */
function safeBoundary(text: string, offset: number): number {
  const index = Math.max(0, Math.min(text.length, offset));
  if (index > 0 && index < text.length && /[\uDC00-\uDFFF]/u.test(text[index])
    && /[\uD800-\uDBFF]/u.test(text[index - 1])) return index - 1;
  return index;
}

/** 与统一估算器使用同一预算口径，通过二分选出完整码点前缀。 */
export function evidencePrefixWithinBudget(text: string, budgetTokens: number): string {
  let low = 0;
  let high = text.length;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    const boundary = safeBoundary(text, middle);
    if (estimateTextTokens(text.slice(0, boundary)) <= budgetTokens) low = middle;
    else high = middle - 1;
  }
  return text.slice(0, safeBoundary(text, low));
}

function evidenceIdentity(artifactId: string, blockId: string, start: number, end: number): string {
  return `kes_${crypto.createHash("sha256").update(JSON.stringify([artifactId, blockId, start, end])).digest("hex").slice(0, 24)}`;
}

/** 打包阶段裁剪超大证据时，同步更新偏移、原文摘要和身份。 */
export function trimEvidenceSpan(span: KnowledgeEvidenceSpan, budgetTokens: number): KnowledgeEvidenceSpan {
  const text = evidencePrefixWithinBudget(span.text, budgetTokens);
  const endOffset = span.startOffset + text.length;
  return {
    ...span, text, endOffset,
    id: evidenceIdentity(span.parseArtifactId, span.blockId, span.startOffset, endOffset),
    textSha256: crypto.createHash("sha256").update(text).digest("hex"),
  };
}

function selectWindow(text: string, start: number, end: number, terms: string[]): [number, number] {
  const budget = KNOWLEDGE_FAST_PER_SPAN_MAX_TOKENS;
  const region = text.slice(start, end);
  if (estimateTextTokens(region) <= budget) return [start, end];
  // 用局部平均字符成本确定密度窗口，再以统一 token 估算器严格裁剪。
  const windowChars = Math.max(1, Math.floor(region.length * budget / estimateTextTokens(region)));
  const escaped = [...terms].sort((a, b) => b.length - a.length)
    .map(term => term.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&"));
  const matches = escaped.length > 0
    ? [...region.matchAll(new RegExp(escaped.join("|"), "giu"))].map(match => ({
      start: start + match.index, term: match[0].toLowerCase(),
    })) : [];
  let anchor = start;
  let bestDistinct = 0;
  let bestCount = 0;
  let left = 0;
  const counts = new Map<string, number>();
  for (let right = 0; right < matches.length; right++) {
    counts.set(matches[right].term, (counts.get(matches[right].term) ?? 0) + 1);
    while (matches[right].start - matches[left].start >= windowChars) {
      const term = matches[left++].term;
      const count = counts.get(term)! - 1;
      if (count === 0) counts.delete(term);
      else counts.set(term, count);
    }
    const count = right - left + 1;
    if (counts.size > bestDistinct || (counts.size === bestDistinct && count > bestCount)) {
      bestDistinct = counts.size;
      bestCount = count;
      anchor = matches[Math.floor((left + right) / 2)].start;
    }
  }
  // 完整段落优先；过长段落才退到句子边界。
  const paragraphStart = Math.max(start, text.lastIndexOf("\n\n", anchor - 1) + 2);
  const nextParagraph = text.indexOf("\n\n", anchor);
  const paragraphEnd = nextParagraph < 0 ? end : Math.min(end, nextParagraph);
  const pStart = text.lastIndexOf("\n\n", anchor - 1) < start ? start : paragraphStart;
  if (estimateTextTokens(text.slice(pStart, paragraphEnd)) <= budget) return [pStart, paragraphEnd];
  const boundaries = [pStart];
  for (const match of text.slice(pStart, paragraphEnd).matchAll(/[。！？!?；;\n]+|\.(?=\s|$)/gu)) {
    boundaries.push(pStart + match.index + match[0].length);
  }
  if (boundaries.at(-1) !== paragraphEnd) boundaries.push(paragraphEnd);
  let section = 0;
  while (section + 1 < boundaries.length - 1 && boundaries[section + 1] <= anchor) section++;
  let from = boundaries[section];
  let to = boundaries[section + 1];
  if (estimateTextTokens(text.slice(from, to)) <= budget) {
    let low = section;
    let high = section + 1;
    while (true) {
      if (high + 1 < boundaries.length && estimateTextTokens(text.slice(from, boundaries[high + 1])) <= budget) {
        to = boundaries[++high];
      } else if (low > 0 && estimateTextTokens(text.slice(boundaries[low - 1], to)) <= budget) {
        from = boundaries[--low];
      } else break;
    }
    return [from, to];
  }
  from = safeBoundary(text, Math.max(start, anchor - Math.floor(windowChars / 2)));
  to = from + evidencePrefixWithinBudget(text.slice(from, end), budget).length;
  if (to <= anchor) {
    from = anchor;
    to = from + evidencePrefixWithinBudget(text.slice(from, end), budget).length;
  }
  return [from, to];
}

/** 搜索正文只作线索；引用必须回到同一冻结解析块，再按原文切片。 */
export class EvidenceSpanExtractor {
  constructor(private readonly store: Pick<KnowledgeStore, "getArtifactBlocksByIds">) {}

  extract(input: Parameters<FastKnowledgeEvidenceStages["extractSpans"]>[0]): KnowledgeEvidenceSpan[] {
    input.signal?.throwIfAborted();
    const sourceByArtifact = new Map(input.compiledScope.sources.filter(source => source.parseArtifactId)
      .map(source => [source.parseArtifactId!, source]));
    const allowedVariants = new Set(input.compiledScope.readyChunkVariantIds);
    const requestedBlocks = new Map<string, Set<string>>();
    for (const hit of input.hits) {
      if (!sourceByArtifact.has(hit.parseArtifactId) || !allowedVariants.has(hit.chunkIndexVariantId)) {
        throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Evidence candidate is outside the frozen scope");
      }
      const ids = requestedBlocks.get(hit.parseArtifactId) ?? new Set<string>();
      for (const span of hit.spans) ids.add(span.blockId);
      requestedBlocks.set(hit.parseArtifactId, ids);
    }
    const blocks = new Map<string, ReturnType<KnowledgeStore["getArtifactBlocksByIds"]>[number]>();
    for (const [parseArtifactId, ids] of requestedBlocks) {
      input.signal?.throwIfAborted();
      for (const block of this.store.getArtifactBlocksByIds({
        studioId: input.compiledScope.studioId, parseArtifactId, blockIds: [...ids],
      })) blocks.set(block.id, block);
    }
    const terms = tokenizeSearchText(input.query);
    const result: KnowledgeEvidenceSpan[] = [];
    for (const hit of input.hits) {
      input.signal?.throwIfAborted();
      const source = sourceByArtifact.get(hit.parseArtifactId)!;
      for (const location of hit.spans) {
        const block = blocks.get(location.blockId);
        if (!block || block.parseArtifactId !== hit.parseArtifactId
          || location.blockStartOffset < 0 || location.blockEndOffset > block.text.length
          || location.blockStartOffset >= location.blockEndOffset) {
          throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Evidence block span does not match canonical text");
        }
        const [startOffset, endOffset] = selectWindow(block.text,
          safeBoundary(block.text, location.blockStartOffset), safeBoundary(block.text, location.blockEndOffset), terms);
        const text = block.text.slice(startOffset, endOffset);
        const heading = block.locator.headingPath;
        const page = block.locator.pageNumber;
        result.push({
          id: evidenceIdentity(hit.parseArtifactId, block.id, startOffset, endOffset),
          sourceId: source.sourceId, sourceName: source.sourceName, notebookIds: source.notebookIds,
          contentSnapshotId: source.contentSnapshotId, parseArtifactId: hit.parseArtifactId,
          chunkIndexVariantId: hit.chunkIndexVariantId, chunkId: hit.id,
          blockId: block.id, startOffset, endOffset, text,
          textSha256: crypto.createHash("sha256").update(text).digest("hex"),
          headingPath: Array.isArray(heading) && heading.every(item => typeof item === "string") ? heading : null,
          pageNumber: typeof page === "number" && Number.isInteger(page) && page > 0 ? page : null,
          retrievalChannels: ["fts"],
          // SQLite BM25 越小越相关，证据层统一成越大越相关。
          score: -hit.score,
        });
      }
    }
    const selected: KnowledgeEvidenceSpan[] = [];
    for (const span of result.sort((a, b) => (b.score ?? 0) - (a.score ?? 0))) {
      if (!selected.some(existing => existing.blockId === span.blockId
        && Math.max(0, Math.min(existing.endOffset, span.endOffset) - Math.max(existing.startOffset, span.startOffset))
          / Math.min(existing.endOffset - existing.startOffset, span.endOffset - span.startOffset) > 0.6)) {
        selected.push(span);
      }
    }
    return selected;
  }
}
