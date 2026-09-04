import crypto from "node:crypto";

import type { KnowledgeBlock } from "./types.ts";
import { CJK_TOKENS_PER_CHAR, NON_CJK_CHARS_PER_TOKEN, estimateTextTokens } from "../llm/estimate-text-tokens.ts";

export const KNOWLEDGE_CHUNKER_VERSION = "4";
/** 章节划分未变；切片配置升级不能改变已有章节及原文读取的身份。 */
const KNOWLEDGE_SECTION_VERSION = "3";
/** @deprecated 仅描述 v3 历史片段；当前片段按生效字符配置切分。 */
export const KNOWLEDGE_SPAN_TARGET_TOKENS = 512;
/** @deprecated 仅描述 v3 历史重叠；当前重叠为目标字符数的八分之一。 */
export const KNOWLEDGE_SPAN_OVERLAP_TOKENS = 64;
export const KNOWLEDGE_SECTION_SOFT_MAX_TOKENS = 8192;
export const KNOWLEDGE_CHUNK_TARGET_CHARS = 2048;
const LEGACY_CHUNK_TARGET_CHARS = 1200;

export type KnowledgeChunkerStrategy = "fixed" | "markdown" | "text" | "pdf" | "html";

export interface KnowledgeChunkerOptions {
  /** 正文目标字符数；在目标范围内优先沿句段边界切分，并参与索引变体身份。 */
  targetChars?: number;
  /** 历史算法测试和明确的旧版重建入口；新生产调用缺省使用当前版本。 */
  legacyVersion?: "2";
}

export interface KnowledgeChunkerConfig {
  strategy: KnowledgeChunkerStrategy;
  targetChars: number;
  /**
   * 分块配置指纹：sha256(`${KNOWLEDGE_CHUNKER_VERSION}${strategy}${targetChars}`) 的前 16 个 hex 字符。
   * 即 ChunkProfile 的 profileHash（knowledge.db chunk_profiles.profile_hash），也是
   * ChunkIndexVariant 的跨库身份键（knowledge-fts.db chunk_index_variants.chunk_profile_hash），
   * 并编入 chunk id 与 ingestion_jobs.chunker_config_id。
   *
   * 身份语义（v9 起）：同一源被多个笔记本以不同分块配置引用时，每个配置产生一个
   * 独立的 ChunkIndexVariant 并存，互不覆盖；不再有"以触发方笔记本配置为准"的
   * 单一份 chunk 集。
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
  /** 自 v3 起始终写入；旧版草稿和迁移前索引没有章节归属。 */
  sectionId?: string | null;
  spans: KnowledgeChunkSpanDraft[];
}

export interface KnowledgeSectionSpanDraft {
  blockId: string;
  blockStartOffset: number;
  blockEndOffset: number;
  sectionStartOffset: number;
  sectionEndOffset: number;
}

export interface KnowledgeSectionDraft {
  id: string;
  parseArtifactId: string;
  sectionOrdinal: number;
  headingPath: string[];
  startBlockOrdinal: number;
  endBlockOrdinal: number;
  text: string;
  tokenCount: number;
  /** 仅构建阶段使用：每个原始块只在首次片段所在节拥有主归属，后续子节只引用其余区间。 */
  primaryBlockIds?: string[];
  /** 仅构建阶段使用：跨节切开大块时保持原始偏移，落库仍只写锁定的章节字段。 */
  blockSpans?: KnowledgeSectionSpanDraft[];
}

// targetChars sanity 边界：下限保证 0.35/0.6 边界系数有意义，上限防止病态配置撑爆内存。
// 导出供 knowledge-store 的笔记本配置校验与 DDL 边界保持一致。
export const MIN_KNOWLEDGE_CHUNK_TARGET_CHARS = 100;
export const MAX_KNOWLEDGE_CHUNK_TARGET_CHARS = 100_000;

/** @deprecated 仅旧版显式兼容使用；当前版本不以嵌入窗口改变分块粒度。 */
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

function normalizeTargetChars(targetChars: number | undefined, legacy = false): number {
  const value = targetChars ?? (legacy ? LEGACY_CHUNK_TARGET_CHARS : KNOWLEDGE_CHUNK_TARGET_CHARS);
  if (!Number.isSafeInteger(value) || value < MIN_KNOWLEDGE_CHUNK_TARGET_CHARS || value > MAX_KNOWLEDGE_CHUNK_TARGET_CHARS) {
    throw new Error(`Knowledge chunk targetChars is invalid: ${String(targetChars)}`);
  }
  return value;
}

export function knowledgeChunkerConfigId(
  strategy: KnowledgeChunkerStrategy,
  targetChars: number,
  options?: Pick<KnowledgeChunkerOptions, "legacyVersion">,
): string {
  return crypto.createHash("sha256")
    .update(`${options?.legacyVersion ?? KNOWLEDGE_CHUNKER_VERSION}${strategy}${targetChars}`, "utf8")
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
  const targetChars = normalizeTargetChars(options?.targetChars, options?.legacyVersion === "2");
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
      strategy = (options?.legacyVersion === "2" ? hasTextChapterStructure(blocks) : blocks.some(block => isTextChapterHeading(block.text))) ? "text" : "fixed";
      break;
    default:
      strategy = "fixed";
      break;
  }
  return { strategy, targetChars, configId: knowledgeChunkerConfigId(strategy, targetChars, options) };
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
export function buildLegacyKnowledgeChunks(
  parseArtifactId: string,
  blocks: readonly KnowledgeBlock[],
  options?: KnowledgeChunkerOptions,
): KnowledgeChunkDraft[] {
  const config = resolveKnowledgeChunkerConfig(blocks, { ...options, legacyVersion: "2" });
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

/** 按共享估算器的字符成本推进，边界始终落在完整UTF-16码点之间。 */
function tokenBoundary(text: string, start: number, limit: number, backwards = false, floor = 0): number {
  let cursor = start, cjk = 0, other = 0;
  while (backwards ? cursor > floor : cursor < text.length) {
    let next: number;
    if (backwards) {
      next = cursor - 1;
      const low = text.charCodeAt(next), high = text.charCodeAt(next - 1);
      if (next > floor && low >= 0xdc00 && low <= 0xdfff && high >= 0xd800 && high <= 0xdbff) next -= 1;
    } else next = cursor + (text.codePointAt(cursor)! > 0xffff ? 2 : 1);
    const character = backwards ? text.slice(next, cursor) : text.slice(cursor, next);
    const isCjk = estimateTextTokens(character) > 1;
    const nextCjk = cjk + Number(isCjk), nextOther = other + Number(!isCjk);
    if (Math.ceil(nextCjk * CJK_TOKENS_PER_CHAR + nextOther / NON_CJK_CHARS_PER_TOKEN) > limit) break;
    cjk = nextCjk; other = nextOther; cursor = next;
  }
  return cursor;
}

/** 字符数按完整码点计数，返回值仍为原文的 UTF-16 偏移。 */
function characterBoundary(text: string, start: number, limit: number, backwards = false, floor = 0): number {
  let cursor = start;
  for (let count = 0; count < limit && (backwards ? cursor > floor : cursor < text.length); count += 1) {
    if (backwards) {
      cursor -= 1;
      const low = text.charCodeAt(cursor), high = text.charCodeAt(cursor - 1);
      if (cursor > floor && low >= 0xdc00 && low <= 0xdfff && high >= 0xd800 && high <= 0xdbff) cursor -= 1;
    } else cursor += text.codePointAt(cursor)! > 0xffff ? 2 : 1;
  }
  return cursor;
}

/** 在目标片段后四成内依次找段落、换行、句末和词间边界，避免产生过小片段。 */
function preferredChunkEnd(text: string, start: number, end: number): number {
  if (end === text.length) return end;
  const floor = start + Math.floor((end - start) * 0.6);
  const tail = text.slice(floor, end);
  const boundaries = [
    /\n[\t ]*\n/gu,
    /[\r\n]/gu,
    /[。！？；][”’"'」』】）》]*|[.!?;][”’"'」』】）》]*(?=\s|$)/gu,
    /\s+/gu,
  ];
  for (const pattern of boundaries) {
    let lastEnd = 0;
    for (const match of tail.matchAll(pattern)) lastEnd = match.index! + match[0].length;
    if (lastEnd > 0) return floor + lastEnd;
  }
  return end;
}

interface PrimarySectionGroup { headingPath: string[]; blocks: KnowledgeBlock[] }

function primarySectionGroups(blocks: readonly KnowledgeBlock[]): PrimarySectionGroup[] {
  const groups: PrimarySectionGroup[] = [];
  let previousKey: string | null = null, activeHeading: string[] = [];
  for (const block of blocks) {
    let headingPath: string[] = [], key = "plain";
    if (block.locatorType === "markdown" || block.locatorType === "html") {
      headingPath = headingPathOf(block); key = JSON.stringify(["heading", headingPath]);
    } else if (block.locatorType === "text") {
      const path = headingPathOf(block);
      // 解析阶段已有的章节定位优先；只有缺少定位时才从正文识别章标题。
      if (path.length > 0) activeHeading = path;
      else if (isTextChapterHeading(block.text)) activeHeading = [block.text.trim()];
      headingPath = activeHeading; key = JSON.stringify(["chapter", headingPath]);
    } else if (block.locatorType === "pdf") {
      const path = headingPathOf(block);
      if (path.length > 0) activeHeading = path;
      headingPath = activeHeading;
      const page = block.locator.pageNumber ?? block.locator.page ?? null;
      key = headingPath.length > 0 ? JSON.stringify(["heading", headingPath]) : JSON.stringify(["page", page]);
    }
    if (key !== previousKey || groups.length === 0) groups.push({ headingPath: [...headingPath], blocks: [] });
    groups[groups.length - 1].blocks.push(block); previousKey = key;
  }
  return groups;
}

/**
 * 章节是原文的确定性连续分区。大块允许拆成同标题的子节：所有片段不重不漏，
 * 但原块仅在首个片段所在节拥有一次主归属。重复块序号不表示重复主归属。
 */
export function buildKnowledgeSections(parseArtifactId: string, blocks: readonly KnowledgeBlock[]): KnowledgeSectionDraft[] {
  const ordered = [...blocks].sort((left, right) => left.ordinal - right.ordinal);
  const ids = new Set<string>(), ordinals = new Set<number>();
  for (const block of ordered) {
    if (block.parseArtifactId !== parseArtifactId || ids.has(block.id) || ordinals.has(block.ordinal)) {
      throw new Error("Knowledge section blocks must have unique identities within the parse artifact");
    }
    ids.add(block.id); ordinals.add(block.ordinal);
  }
  const sections: KnowledgeSectionDraft[] = [], ownedBlocks = new Set<string>();
  for (const group of primarySectionGroups(ordered)) {
    let text = "";
    const positions = group.blocks.map((block, index) => {
      if (index > 0) text += "\n\n";
      const start = text.length;
      text += block.text;
      return { block, start, end: text.length };
    });
    let start = 0;
    do {
      const end = tokenBoundary(text, start, KNOWLEDGE_SECTION_SOFT_MAX_TOKENS);
      const blockSpans: KnowledgeSectionSpanDraft[] = [], primaryBlockIds: string[] = [], coveredOrdinals: number[] = [];
      for (const position of positions) {
        const from = Math.max(start, position.start), to = Math.min(end, position.end);
        const emptyOwnedHere = position.start === position.end && position.start >= start
          && (position.start < end || end === text.length);
        if (to <= from && !emptyOwnedHere) continue;
        coveredOrdinals.push(position.block.ordinal);
        if (!ownedBlocks.has(position.block.id)) { primaryBlockIds.push(position.block.id); ownedBlocks.add(position.block.id); }
        if (to > from) blockSpans.push({ blockId: position.block.id, blockStartOffset: from - position.start,
          blockEndOffset: to - position.start, sectionStartOffset: from - start, sectionEndOffset: to - start });
      }
      const sectionOrdinal = sections.length, sectionText = text.slice(start, end);
      // 分隔符本身不是原文块；极端空行片只按相邻原块保留位置，不能伪造引用跨度。
      const fallbackOrdinal = positions.find(position => position.end >= start)?.block.ordinal ?? group.blocks[group.blocks.length - 1].ordinal;
      const id = "section_" + crypto.createHash("sha256").update(JSON.stringify([KNOWLEDGE_SECTION_VERSION, parseArtifactId, sectionOrdinal])).digest("hex").slice(0, 32);
      sections.push({ id, parseArtifactId, sectionOrdinal, headingPath: [...group.headingPath],
        startBlockOrdinal: coveredOrdinals[0] ?? fallbackOrdinal, endBlockOrdinal: coveredOrdinals.at(-1) ?? fallbackOrdinal,
        text: sectionText, tokenCount: estimateTextTokens(sectionText), primaryBlockIds, blockSpans });
      start = end;
    } while (start < text.length);
  }
  return sections;
}

/** 片段只在节内滑动，正文和重叠均随字符配置变化；引用跨度逐层回到原始块。 */
export function buildKnowledgeChunks(parseArtifactId: string, blocks: readonly KnowledgeBlock[], options?: KnowledgeChunkerOptions): KnowledgeChunkDraft[] {
  if (options?.legacyVersion === "2") return buildLegacyKnowledgeChunks(parseArtifactId, blocks, options);
  const config = resolveKnowledgeChunkerConfig(blocks, options), chunks: KnowledgeChunkDraft[] = [];
  const overlapChars = Math.floor(config.targetChars / 8);
  for (const section of buildKnowledgeSections(parseArtifactId, blocks)) {
    let start = 0;
    while (start < section.text.length) {
      const end = preferredChunkEnd(section.text, start, characterBoundary(section.text, start, config.targetChars));
      const spans: KnowledgeChunkSpanDraft[] = [];
      for (const span of section.blockSpans ?? []) {
        const from = Math.max(start, span.sectionStartOffset), to = Math.min(end, span.sectionEndOffset);
        if (to > from) spans.push({ blockId: span.blockId,
          blockStartOffset: span.blockStartOffset + from - span.sectionStartOffset,
          blockEndOffset: span.blockStartOffset + to - span.sectionStartOffset,
          chunkStartOffset: from - start, chunkEndOffset: to - start });
      }
      if (spans.length > 0) {
        const text = section.text.slice(start, end), ordinal = chunks.length;
        chunks.push({ id: deterministicChunkId(config.configId, parseArtifactId, ordinal), parseArtifactId, ordinal,
          sectionId: section.id, text, tokenCount: estimateTextTokens(text), spans });
      }
      if (end === section.text.length) break;
      const next = characterBoundary(section.text, end, overlapChars, true, start);
      start = next > start ? next : end;
    }
  }
  return chunks;
}

/** 旧测试与显式历史生成使用这些门面，不影响新生产入口的当前默认。 */
export function resolveLegacyKnowledgeChunkerConfig(blocks: readonly KnowledgeBlock[], options?: KnowledgeChunkerOptions): KnowledgeChunkerConfig {
  return resolveKnowledgeChunkerConfig(blocks, { ...options, legacyVersion: "2" });
}
export function legacyKnowledgeChunkerConfigId(strategy: KnowledgeChunkerStrategy, targetChars: number): string {
  return knowledgeChunkerConfigId(strategy, targetChars, { legacyVersion: "2" });
}
export function legacyKnowledgeBlockFingerprint(blocks: readonly KnowledgeBlock[]): string {
  return knowledgeBlockFingerprint(blocks, { legacyVersion: "2" });
}

export function knowledgeBlockFingerprint(blocks: readonly KnowledgeBlock[], options?: Pick<KnowledgeChunkerOptions, "legacyVersion">): string {
  const hash = crypto.createHash("sha256");
  hash.update(options?.legacyVersion ?? KNOWLEDGE_CHUNKER_VERSION, "utf8");
  for (const block of blocks) {
    hash.update("\0", "utf8");
    hash.update(block.id, "utf8");
    hash.update("\0", "utf8");
    hash.update(block.textSha256, "utf8");
  }
  return hash.digest("hex");
}
