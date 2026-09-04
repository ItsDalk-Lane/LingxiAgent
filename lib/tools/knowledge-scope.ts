/**
 * knowledge 工具共享的 KnowledgeTurnScope 校验链（Phase 11 从 knowledge-read-tool
 * 的 Phase 4 实现原位抽取，供 knowledge_read / knowledge_outline / knowledge_grep
 * 复用；语义不变，任务书 §二十~§二十二）：
 *
 * - scopeId 必填且服务端逐次复核：scope 存在、active、属于当前 studio 与当前会话
 *   （subagent 子会话经 manifest provenance 继承父会话 scope——scope 只能缩小）；
 * - sourceId / notebookId 必须在 scope 冻结集合内，不信任模型传入的任何 id；
 * - 读取锚定 scope 冻结的 snapshot/artifact（§四十三）。任何一项失败 →
 *   KNOWLEDGE_SCOPE_VIOLATION / 显式错误，不回落到旧的全 studio 扫描行为。
 */
import path from "node:path";

import { KnowledgeError } from "../knowledge/errors.ts";
import type { KnowledgeManager } from "../knowledge/knowledge-manager.ts";
import type { KnowledgeBlock, KnowledgeTurnScope, KnowledgeTurnScopeSource } from "../knowledge/types.ts";

/** 工具执行会话的 scope 归属上下文（Pi SDK execute 第 5 参 ctx 的解析结果）。 */
export interface KnowledgeToolSessionContext {
  sessionPath: string | null;
  scopeOwnerSessionPath: string | null;
}

export function knowledgeScopeViolation(message: string): KnowledgeError {
  return new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", message);
}

export function sameKnowledgeSessionPath(left: string, right: string): boolean {
  return path.resolve(left) === path.resolve(right);
}

/**
 * 无会话上下文的 surface（如独立 CLI 调用）：显式不可用，不静默放行。
 * 返回可用的会话路径。
 */
export function requireKnowledgeSessionContext(
  sessionContext: KnowledgeToolSessionContext,
): string {
  if (!sessionContext.sessionPath) {
    throw new KnowledgeError(
      "KNOWLEDGE_MODEL_UNAVAILABLE",
      "this knowledge tool requires a session-bound KnowledgeTurnScope context",
    );
  }
  if (!sessionContext.scopeOwnerSessionPath) {
    throw knowledgeScopeViolation("Knowledge scope owner must be resolved from session manifests");
  }
  return sessionContext.sessionPath;
}

/**
 * scope 归属校验（服务端复核，不信任模型传入的 scopeId）：
 * 存在、active、属于当前 studio、属于当前会话或经真实祖先链核验的范围拥有者。
 * 通过返回完整 scope（含冻结源集合）。
 */
export function resolveKnowledgeTurnScope(input: {
  knowledge: KnowledgeManager;
  studioId: string;
  scopeId: string;
  sessionContext: KnowledgeToolSessionContext;
}): KnowledgeTurnScope {
  requireKnowledgeSessionContext(input.sessionContext);
  const scope = input.knowledge.getTurnScope({ scopeId: input.scopeId });
  if (!scope) throw knowledgeScopeViolation("Unknown knowledge turn scope");
  if (scope.studioId !== input.studioId) {
    throw knowledgeScopeViolation("Knowledge turn scope belongs to a different studio");
  }
  if (scope.status !== "active") {
    throw knowledgeScopeViolation("Knowledge turn scope is closed (superseded by a newer turn)");
  }
  const ownsScope = sameKnowledgeSessionPath(scope.sessionPath, input.sessionContext.sessionPath!)
    || sameKnowledgeSessionPath(scope.sessionPath, input.sessionContext.scopeOwnerSessionPath!);
  if (!ownsScope) {
    throw knowledgeScopeViolation("Knowledge turn scope does not belong to this session");
  }
  return scope;
}

/** sourceId 必须在 scope 冻结集合内；返回冻结条目（含 snapshot/artifact 身份）。 */
export function requireKnowledgeScopeSource(
  scope: KnowledgeTurnScope,
  sourceId: string,
): KnowledgeTurnScopeSource {
  const frozen = scope.sources.find(source => source.sourceId === sourceId);
  if (!frozen) {
    throw knowledgeScopeViolation("Knowledge source is outside this turn's scope");
  }
  return frozen;
}

/**
 * notebookId 归属解析：给出时必须同时属于 scope 选中集合与该源的冻结引用集合；
 * 缺失时取冻结集合内第一个引用笔记本（限选中集合，不扫全 studio）。
 */
export function resolveKnowledgeOwningNotebookId(
  scope: KnowledgeTurnScope,
  frozen: KnowledgeTurnScopeSource,
  notebookId: string | null,
): string {
  if (notebookId) {
    if (!scope.notebookIds.includes(notebookId) || !frozen.notebookIds.includes(notebookId)) {
      throw knowledgeScopeViolation("Notebook is outside this turn's scope for this source");
    }
    return notebookId;
  }
  return frozen.notebookIds[0];
}

/**
 * block 的有效 headingPath（与 chunker.headingPathOf 同口径）：locator.headingPath
 * 可能是稀疏数组经 JSON 序列化后的含 null 形态，只保留有效标题。
 */
export function knowledgeBlockHeadingPath(block: KnowledgeBlock): string[] {
  const raw = (block?.locator as Record<string, unknown> | undefined)?.headingPath;
  if (!Array.isArray(raw)) return [];
  return raw.filter((entry): entry is string => typeof entry === "string" && entry.trim().length > 0);
}

export interface KnowledgeRawReadRange {
  blockId: string;
  startOffset?: number;
  endOffset?: number;
}

/** 引用由程序直接绑定冻结原文，模型只需使用返回的链接。 */
export function createKnowledgeToolCitation(input: {
  knowledge: KnowledgeManager;
  studioId: string;
  scope: KnowledgeTurnScope;
  sourceId: string;
  block: KnowledgeBlock;
  startOffset: number;
  endOffset: number;
}) {
  // 检索过程中可能开始了下一轮；签发前重新查库，不能沿用等待前的 active 状态。
  const current = input.knowledge.getTurnScope({ scopeId: input.scope.id });
  if (!current || current.status !== "active" || current.studioId !== input.studioId
    || !sameKnowledgeSessionPath(current.sessionPath, input.scope.sessionPath)) {
    throw knowledgeScopeViolation("Knowledge turn scope closed before citation issuance");
  }
  const frozen = requireKnowledgeScopeSource(current, input.sourceId);
  const original = requireKnowledgeScopeSource(input.scope, input.sourceId);
  if (frozen.parseArtifactId !== input.block.parseArtifactId
    || frozen.parseArtifactId !== original.parseArtifactId
    || frozen.contentSnapshotId !== original.contentSnapshotId) {
    throw knowledgeScopeViolation("Citation block is outside the frozen source");
  }
  const citation = input.knowledge.createCitation({ studioId: input.studioId,
    parseArtifactId: input.block.parseArtifactId, blockId: input.block.id,
    startOffset: input.startOffset, endOffset: input.endOffset });
  return {
    citationId: citation.id,
    citationMarkdown: `[来源 · 原文](#knowledge-citation-${citation.id})`,
  };
}

/** 原文只在 spans 中出现一次；位置以去重后的连续原块范围计数，不包含人工分隔符。 */
export function readKnowledgeCitationPage(input: {
  knowledge: KnowledgeManager;
  studioId: string;
  scope: KnowledgeTurnScope;
  sourceId: string;
  parseArtifactId: string;
  ranges: KnowledgeRawReadRange[];
  offset?: number;
  maxChars?: number;
  maxBytes?: number;
  signal?: AbortSignal;
}) {
  const frozen = requireKnowledgeScopeSource(input.scope, input.sourceId);
  if (frozen.parseArtifactId !== input.parseArtifactId) {
    throw knowledgeScopeViolation("Read positions are outside the frozen source");
  }
  const offset = input.offset ?? 0;
  const maxChars = input.maxChars ?? 6000;
  if (!Number.isSafeInteger(offset) || offset < 0 || !Number.isSafeInteger(maxChars)
    || maxChars < 256 || maxChars > 8000) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "offset must be non-negative; maxChars must be an integer from 256 to 8000");
  }
  input.signal?.throwIfAborted();
  const blocks = new Map(input.knowledge.store.getArtifactBlocksByIds({
    studioId: input.studioId, parseArtifactId: input.parseArtifactId,
    blockIds: input.ranges.map(range => range.blockId),
  }).map(block => [block.id, block] as const));
  const ordered = input.ranges.map(range => {
    const block = blocks.get(range.blockId);
    const start = range.startOffset ?? 0;
    const end = range.endOffset ?? block?.text.length ?? 0;
    if (!block || !Number.isSafeInteger(start) || !Number.isSafeInteger(end)
      || start < 0 || end < start || end > block.text.length) {
      throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Read range is outside its frozen raw block");
    }
    return { block, start, end };
  }).sort((left, right) => left.block.ordinal - right.block.ordinal || left.start - right.start);
  const ranges: typeof ordered = [];
  for (const range of ordered) {
    if (range.end === range.start) continue;
    const previous = ranges.at(-1);
    if (previous?.block.id === range.block.id && range.start <= previous.end) {
      previous.end = Math.max(previous.end, range.end);
    } else ranges.push({ ...range });
  }
  const totalChars = ranges.reduce((sum, range) => sum + range.end - range.start, 0);
  if (offset > totalChars) throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "offset exceeds the selected original text");
  const spans: Array<{
    blockId: string; blockOrdinal: number; startOffset: number; endOffset: number;
    headingPath: string[]; pageNumber: number | null; text: string;
    citationId: string; citationMarkdown: string;
  }> = [];
  let skip = offset;
  let returnedChars = 0;
  let remainingBytes = input.maxBytes ?? 24_000;
  for (const range of ranges) {
    input.signal?.throwIfAborted();
    const length = range.end - range.start;
    if (skip >= length) { skip -= length; continue; }
    const start = range.start + skip;
    skip = 0;
    if (start > 0 && /[\uDC00-\uDFFF]/u.test(range.block.text[start])
      && /[\uD800-\uDBFF]/u.test(range.block.text[start - 1])) {
      throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "offset must not split a Unicode character");
    }
    const metadata = { blockId: range.block.id, blockOrdinal: range.block.ordinal,
      startOffset: start, headingPath: knowledgeBlockHeadingPath(range.block),
      pageNumber: typeof range.block.locator.pageNumber === "number" ? range.block.locator.pageNumber : null };
    let low = 0, high = Math.min(range.end - start, maxChars - returnedChars);
    // 为真实引用编号及链接留空间，保证长原文不会触发外层工具的整段截断。
    while (low < high) {
      const count = Math.ceil((low + high) / 2);
      const bytes = Buffer.byteLength(JSON.stringify({ ...metadata, endOffset: start + count,
        text: range.block.text.slice(start, start + count) }), "utf8") + 512;
      if (bytes <= remainingBytes) low = count;
      else high = count - 1;
    }
    let end = start + low;
    if (end < range.block.text.length && /[\uDC00-\uDFFF]/u.test(range.block.text[end])
      && /[\uD800-\uDBFF]/u.test(range.block.text[end - 1])) end--;
    if (end <= start) break;
    const span = { ...metadata, endOffset: end, text: range.block.text.slice(start, end),
      ...createKnowledgeToolCitation({ ...input, block: range.block, startOffset: start, endOffset: end }) };
    spans.push(span);
    returnedChars += end - start;
    remainingBytes -= Buffer.byteLength(JSON.stringify(span), "utf8") + 1;
    if (end < range.end || returnedChars >= maxChars) break;
  }
  if (offset < totalChars && returnedChars === 0) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "The selected source metadata exceeds this read page budget; read the block directly");
  }
  const nextOffset = offset + returnedChars < totalChars ? offset + returnedChars : null;
  return { spans, offset, returnedChars, totalChars, truncated: nextOffset !== null, nextOffset };
}
