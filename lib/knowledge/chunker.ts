import crypto from "node:crypto";

import type { KnowledgeBlock } from "./types.ts";

export const KNOWLEDGE_CHUNKER_VERSION = "2";
export const KNOWLEDGE_CHUNK_TARGET_CHARS = 1200;

export type KnowledgeChunkerStrategy = "fixed" | "markdown" | "text" | "pdf" | "html";

export interface KnowledgeChunkerOptions {
  /**
   * 目标 chunk 字符数。摄入侧由 ingestion-service 按笔记本配置（schema v6 列，
   * 解析链见 knowledge-store.resolveNotebookConfig）传入；查询侧懒构建兜底仍用默认值。
   */
  targetChars?: number;
}

export interface KnowledgeChunkerConfig {
  strategy: KnowledgeChunkerStrategy;
  targetChars: number;
  /**
   * 分块配置指纹：sha256(`${KNOWLEDGE_CHUNKER_VERSION}${strategy}${targetChars}`) 的前 16 个 hex 字符。
   * 编入 chunk id，并作为 artifact_indexes.chunker_version 列的新语义值
   * （该列从常量版本号改存 configId；索引库是可重建缓存，不匹配即整体重建，无需迁移）。
   *
   * 权衡（显式记录）：同一源被多个笔记本以不同分块配置引用时，分块以
   * "触发本次摄入的笔记本"的配置为准；configId 由 ingestion-service 记入 ingestion job 行。
   */
  configId: string;
}

export interface KnowledgeChunkSpanDraft {
  blockId: string;
  blockStartOffset: number;
  blockEndOffset: number;
  chunkStartOffset: number;
  chunkEndOffset: number;
}

export interface KnowledgeChunkDraft {
  id: string;
  parseArtifactId: string;
  ordinal: number;
  text: string;
  tokenCount: number;
  spans: KnowledgeChunkSpanDraft[];
}

// targetChars sanity 边界：下限保证 0.35/0.6 边界系数有意义，上限防止病态配置撑爆内存。
// 导出供 knowledge-store 的笔记本配置校验与 DDL 边界保持一致。
export const MIN_KNOWLEDGE_CHUNK_TARGET_CHARS = 100;
export const MAX_KNOWLEDGE_CHUNK_TARGET_CHARS = 100_000;

/** 自动分块：笔记本嵌入模型上下文窗口的可占用比例。 */
export const KNOWLEDGE_CHUNK_CONTEXT_FRACTION = 0.8;
/** 嵌入模型上下文查不到时的兜底窗口（token 数）。 */
export const KNOWLEDGE_CHUNK_FALLBACK_CONTEXT_TOKENS = 8192;

/**
 * 自动分块尺寸 = 嵌入模型上下文窗口 × 80%，按最保守口径（1 token = 1 字符）
 * 换算成字符数——中文块恰好占满 80% 窗口，英文块实际约 20% 用量，任何语言
 * 都不会超过嵌入模型的输入上限。窗口未知/非法回退 8192。结果夹在
 * MIN/MAX 边界内（与 normalizeTargetChars 同一边界集）。
 */
export function computeAutoChunkTargetChars(contextWindowTokens: number | null | undefined): number {
  const window = typeof contextWindowTokens === "number"
    && Number.isFinite(contextWindowTokens) && contextWindowTokens > 0
    ? contextWindowTokens
    : KNOWLEDGE_CHUNK_FALLBACK_CONTEXT_TOKENS;
  const target = Math.floor(window * KNOWLEDGE_CHUNK_CONTEXT_FRACTION);
  return Math.min(
    MAX_KNOWLEDGE_CHUNK_TARGET_CHARS,
    Math.max(MIN_KNOWLEDGE_CHUNK_TARGET_CHARS, target),
  );
}

function normalizeTargetChars(targetChars: number | undefined): number {
  const value = targetChars ?? KNOWLEDGE_CHUNK_TARGET_CHARS;
  if (!Number.isSafeInteger(value) || value < MIN_KNOWLEDGE_CHUNK_TARGET_CHARS || value > MAX_KNOWLEDGE_CHUNK_TARGET_CHARS) {
    throw new Error(`Knowledge chunk targetChars is invalid: ${String(targetChars)}`);
  }
  return value;
}

export function knowledgeChunkerConfigId(
  strategy: KnowledgeChunkerStrategy,
  targetChars: number,
): string {
  return crypto.createHash("sha256")
    .update(`${KNOWLEDGE_CHUNKER_VERSION}${strategy}${targetChars}`, "utf8")
    .digest("hex")
    .slice(0, 16);
}

// 小说等纯文本的章节标题行首启发式。
const TEXT_CHAPTER_HEADING_PATTERNS = [
  /^\s*第[零〇一二三四五六七八九十百千万\d]{1,7}[章节回卷部篇]/u,
  /^Chapter\s+\d+/u,
  /^(?:序章|楔子|尾声|终章|番外|后记)/u,
] as const;

// 前 200 个 Block（text 解析器按非空行出 Block）0 命中章节标题时回退固定大小策略。
const TEXT_CHAPTER_PROBE_BLOCKS = 200;

function isTextChapterHeading(text: string): boolean {
  return TEXT_CHAPTER_HEADING_PATTERNS.some(pattern => pattern.test(text));
}

function hasTextChapterStructure(blocks: readonly KnowledgeBlock[]): boolean {
  return blocks.slice(0, TEXT_CHAPTER_PROBE_BLOCKS).some(block => isTextChapterHeading(block.text));
}

function headingPathOf(block: KnowledgeBlock): string[] {
  const raw = block?.locator?.headingPath;
  if (!Array.isArray(raw)) return [];
  // headingPath 可能是稀疏数组经 JSON 序列化后的含 null 形态，只保留有效标题。
  return raw.filter((entry): entry is string => typeof entry === "string" && entry.trim().length > 0);
}

/**
 * 按首块 locatorType 分发策略（同一 parseArtifact 的 Block 由同一解析器产出，
 * locatorType 一致）；无匹配 locatorType、或无章节结构的纯文本回退 "fixed"。
 */
export function resolveKnowledgeChunkerConfig(
  blocks: readonly KnowledgeBlock[],
  options?: KnowledgeChunkerOptions,
): KnowledgeChunkerConfig {
  const targetChars = normalizeTargetChars(options?.targetChars);
  let strategy: KnowledgeChunkerStrategy;
  switch (blocks[0]?.locatorType) {
    case "markdown":
      strategy = "markdown";
      break;
    case "html":
      strategy = "html";
      break;
    case "pdf":
      strategy = "pdf";
      break;
    case "text":
      strategy = hasTextChapterStructure(blocks) ? "text" : "fixed";
      break;
    default:
      strategy = "fixed";
      break;
  }
  return { strategy, targetChars, configId: knowledgeChunkerConfigId(strategy, targetChars) };
}

function safeSplitEnd(text: string, start: number, proposedEnd: number, budget: number): number {
  let end = Math.min(text.length, proposedEnd);
  if (end >= text.length) return text.length;
  // 不在 UTF-16 代理对中间截断。
  const previous = text.charCodeAt(end - 1);
  const next = text.charCodeAt(end);
  if (previous >= 0xd800 && previous <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) end -= 1;

  const minimumBoundary = start + Math.floor(budget * 0.6);
  for (let cursor = end; cursor > minimumBoundary; cursor -= 1) {
    if (/[\s。！？；.!?;]/u.test(text[cursor - 1])) return cursor;
  }
  return end;
}

function deterministicChunkId(configId: string, parseArtifactId: string, ordinal: number): string {
  const digest = crypto.createHash("sha256")
    .update(`${configId}\0${parseArtifactId}\0${ordinal}`, "utf8")
    .digest("hex")
    .slice(0, 32);
  return `chunk_${digest}`;
}

interface PieceSegment {
  block: KnowledgeBlock;
  start: number;
  end: number;
}

type ChunkHeaderFor = (block: KnowledgeBlock) => string;

const NO_HEADER: ChunkHeaderFor = () => "";

/**
 * 落出一个 chunk。header（面包屑）是注入的合成文本、不属于任何 Block，
 * span 只覆盖正文区间；正文为空的段不落 chunk。
 */
function emitChunk(
  chunks: KnowledgeChunkDraft[],
  config: KnowledgeChunkerConfig,
  parseArtifactId: string,
  header: string,
  segments: readonly PieceSegment[],
) {
  let body = "";
  const spans: KnowledgeChunkSpanDraft[] = [];
  for (const segment of segments) {
    const piece = segment.block.text.slice(segment.start, segment.end);
    if (!piece) continue;
    const chunkStartOffset = header.length + body.length + (body ? 2 : 0);
    if (body) body += "\n\n";
    body += piece;
    spans.push({
      blockId: segment.block.id,
      blockStartOffset: segment.start,
      blockEndOffset: segment.end,
      chunkStartOffset,
      chunkEndOffset: chunkStartOffset + piece.length,
    });
  }
  if (!body) return;
  const text = header + body;
  const ordinal = chunks.length;
  chunks.push({
    id: deterministicChunkId(config.configId, parseArtifactId, ordinal),
    parseArtifactId,
    ordinal,
    text,
    tokenCount: Math.max(1, Math.ceil(Array.from(text).length / 4)),
    spans,
  });
}

/**
 * 固定大小边界切分（v1 行为的参数化版本）：无匹配 locatorType、无章节结构的
 * 纯文本回退到本策略；超长单段落也在段内复用本切分。
 */
function emitFixedRanges(
  chunks: KnowledgeChunkDraft[],
  config: KnowledgeChunkerConfig,
  parseArtifactId: string,
  headerFor: ChunkHeaderFor,
  blocks: readonly KnowledgeBlock[],
) {
  let segments: PieceSegment[] = [];
  let bodyLength = 0;
  let header = "";

  const flush = () => {
    if (segments.length === 0) return;
    emitChunk(chunks, config, parseArtifactId, header, segments);
    segments = [];
    bodyLength = 0;
  };

  for (const block of blocks) {
    let start = 0;
    while (start < block.text.length) {
      if (segments.length === 0) header = headerFor(block);
      const budget = Math.max(1, config.targetChars - header.length);
      const remainingCapacity = bodyLength ? budget - bodyLength - 2 : budget;
      if (remainingCapacity < Math.floor(budget * 0.35)) {
        flush();
        continue;
      }
      const end = safeSplitEnd(block.text, start, start + remainingCapacity, budget);
      if (end <= start) break; // 防御病态 budget，保证循环可终止。
      segments.push({ block, start, end });
      bodyLength += (bodyLength ? 2 : 0) + (end - start);
      start = end;
      if (start < block.text.length) flush();
    }
  }
  flush();
}

type ParagraphMode = "line-gap" | "block";

function groupParagraphs(blocks: readonly KnowledgeBlock[], mode: ParagraphMode): KnowledgeBlock[][] {
  if (mode === "block") return blocks.map(block => [block]);
  // markdown/text 解析器按非空行出 Block 且跳过空行：行号断层即空行段落边界。
  const paragraphs: KnowledgeBlock[][] = [];
  let previousLineEnd: number | null = null;
  for (const block of blocks) {
    const lineStart = Number(block.locator?.lineStart);
    const lineEnd = Number(block.locator?.lineEnd);
    const contiguous = paragraphs.length > 0
      && previousLineEnd !== null
      && Number.isSafeInteger(lineStart)
      && lineStart <= previousLineEnd + 1;
    if (contiguous) paragraphs[paragraphs.length - 1].push(block);
    else paragraphs.push([block]);
    if (Number.isSafeInteger(lineEnd)) previousLineEnd = lineEnd;
  }
  return paragraphs;
}

function blocksTextLength(blocks: readonly KnowledgeBlock[]): number {
  return blocks.reduce((sum, block) => sum + block.text.length, 0) + Math.max(0, blocks.length - 1) * 2;
}

/**
 * 段落组总量超过 softCap 时在段落边界二分（取离总量一半最近的边界），
 * 直到每组 ≤ softCap；单段落超大无法二分时原样返回，交由固定切分处理。
 */
function splitParagraphGroups(paragraphs: KnowledgeBlock[][], softCap: number): KnowledgeBlock[][] {
  const total = paragraphs.reduce((sum, paragraph) => sum + blocksTextLength(paragraph), 0)
    + Math.max(0, paragraphs.length - 1) * 2;
  if (paragraphs.length <= 1 || total <= softCap) return [paragraphs.flat()];
  let bestIndex = 1;
  let bestDelta = Number.POSITIVE_INFINITY;
  let accumulated = 0;
  for (let index = 0; index < paragraphs.length - 1; index += 1) {
    accumulated += blocksTextLength(paragraphs[index]) + 2;
    const delta = Math.abs(accumulated - total / 2);
    if (delta < bestDelta) {
      bestDelta = delta;
      bestIndex = index + 1;
    }
  }
  return [
    ...splitParagraphGroups(paragraphs.slice(0, bestIndex), softCap),
    ...splitParagraphGroups(paragraphs.slice(bestIndex), softCap),
  ];
}

/**
 * chunk 头部注入 headingPath 面包屑文本（标题词进入 FTS/向量文本，提升检索质量）。
 * 超过 targetChars 60% 时依次丢弃顶层标题，仍超长则硬截断，避免面包屑挤占正文预算。
 */
function breadcrumbHeader(block: KnowledgeBlock, targetChars: number): string {
  let path = headingPathOf(block);
  if (path.length === 0) return "";
  const limit = Math.floor(targetChars * 0.6);
  let joined = path.join(" > ");
  while (joined.length > limit && path.length > 1) {
    path = path.slice(1);
    joined = path.join(" > ");
  }
  if (joined.length > limit) joined = joined.slice(0, limit);
  return `${joined}\n\n`;
}

interface SectionDraft {
  blocks: KnowledgeBlock[];
}

/** 按 headingPath 首元素变更切节；首个标题前的内容归为序言节（markdown/html 共用）。 */
function splitHeadingSections(blocks: readonly KnowledgeBlock[]): SectionDraft[] {
  const sections: SectionDraft[] = [];
  let current: KnowledgeBlock[] = [];
  let currentKey: string | null = null;
  for (const block of blocks) {
    const key = headingPathOf(block)[0] ?? null;
    if (current.length > 0 && key !== currentKey) {
      sections.push({ blocks: current });
      current = [];
    }
    currentKey = key;
    current.push(block);
  }
  if (current.length > 0) sections.push({ blocks: current });
  return sections;
}

/** 按章节标题启发式切节；首个章节标题前的内容归为序言节。 */
function splitTextChapterSections(blocks: readonly KnowledgeBlock[]): SectionDraft[] {
  const sections: SectionDraft[] = [];
  let current: KnowledgeBlock[] = [];
  for (const block of blocks) {
    if (isTextChapterHeading(block.text) && current.length > 0) {
      sections.push({ blocks: current });
      current = [];
    }
    current.push(block);
  }
  if (current.length > 0) sections.push({ blocks: current });
  return sections;
}

/** 按页码分组（页内超长在后续段落二分/固定切分中处理）。 */
function splitPdfPageSections(blocks: readonly KnowledgeBlock[]): SectionDraft[] {
  const sections: SectionDraft[] = [];
  let current: KnowledgeBlock[] = [];
  let currentPage: number | null = null;
  for (const block of blocks) {
    const page = Number(block.locator?.page);
    const key = Number.isFinite(page) ? page : null;
    if (current.length > 0 && key !== currentPage) {
      sections.push({ blocks: current });
      current = [];
    }
    currentPage = key;
    current.push(block);
  }
  if (current.length > 0) sections.push({ blocks: current });
  return sections;
}

/**
 * 结构化策略公共落地：节内按空行段落分组，节超过 targetChars*1.5 时
 * 在段落边界二分；单段落仍超软上限时段内回退固定大小边界切分。
 */
function emitStructuredSections(
  chunks: KnowledgeChunkDraft[],
  config: KnowledgeChunkerConfig,
  parseArtifactId: string,
  sections: readonly SectionDraft[],
  paragraphMode: ParagraphMode,
  headerFor: ChunkHeaderFor,
) {
  const softCap = Math.floor(config.targetChars * 1.5);
  for (const section of sections) {
    const paragraphs = groupParagraphs(section.blocks, paragraphMode);
    for (const group of splitParagraphGroups(paragraphs, softCap)) {
      if (blocksTextLength(group) > softCap) {
        emitFixedRanges(chunks, config, parseArtifactId, headerFor, group);
      } else {
        emitChunk(
          chunks,
          config,
          parseArtifactId,
          group.length > 0 ? headerFor(group[0]) : "",
          group.map(block => ({ block, start: 0, end: block.text.length })),
        );
      }
    }
  }
}

/**
 * Chunk 是可重建检索派生物；每个字符区间仍精确指回原始 Block。
 * 结构化策略（markdown/html 标题节、text 章节、pdf 页）按 locator 聚合成节，
 * 节内超长二分；无匹配结构时回退固定大小边界策略。
 */
export function buildKnowledgeChunks(
  parseArtifactId: string,
  blocks: readonly KnowledgeBlock[],
  options?: KnowledgeChunkerOptions,
): KnowledgeChunkDraft[] {
  const config = resolveKnowledgeChunkerConfig(blocks, options);
  const chunks: KnowledgeChunkDraft[] = [];
  const breadcrumb: ChunkHeaderFor = block => breadcrumbHeader(block, config.targetChars);
  switch (config.strategy) {
    case "markdown":
      emitStructuredSections(chunks, config, parseArtifactId, splitHeadingSections(blocks), "line-gap", breadcrumb);
      break;
    case "html":
      emitStructuredSections(chunks, config, parseArtifactId, splitHeadingSections(blocks), "block", breadcrumb);
      break;
    case "text":
      emitStructuredSections(chunks, config, parseArtifactId, splitTextChapterSections(blocks), "line-gap", NO_HEADER);
      break;
    case "pdf":
      emitStructuredSections(chunks, config, parseArtifactId, splitPdfPageSections(blocks), "block", NO_HEADER);
      break;
    default:
      emitFixedRanges(chunks, config, parseArtifactId, NO_HEADER, blocks);
  }
  return chunks;
}

export function knowledgeBlockFingerprint(blocks: readonly KnowledgeBlock[]): string {
  const hash = crypto.createHash("sha256");
  hash.update(KNOWLEDGE_CHUNKER_VERSION, "utf8");
  for (const block of blocks) {
    hash.update("\0", "utf8");
    hash.update(block.id, "utf8");
    hash.update("\0", "utf8");
    hash.update(block.textSha256, "utf8");
  }
  return hash.digest("hex");
}
