import crypto from "node:crypto";

import type { KnowledgeBlock } from "./types.ts";

export const KNOWLEDGE_CHUNKER_VERSION = "1";
export const KNOWLEDGE_CHUNK_TARGET_CHARS = 1200;

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

function safeSplitEnd(text: string, start: number, proposedEnd: number): number {
  let end = Math.min(text.length, proposedEnd);
  if (end >= text.length) return text.length;
  // 不在 UTF-16 代理对中间截断。
  const previous = text.charCodeAt(end - 1);
  const next = text.charCodeAt(end);
  if (previous >= 0xd800 && previous <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) end -= 1;

  const minimumBoundary = start + Math.floor(KNOWLEDGE_CHUNK_TARGET_CHARS * 0.6);
  for (let cursor = end; cursor > minimumBoundary; cursor -= 1) {
    if (/[\s。！？；.!?;]/u.test(text[cursor - 1])) return cursor;
  }
  return end;
}

function deterministicChunkId(parseArtifactId: string, ordinal: number): string {
  const digest = crypto.createHash("sha256")
    .update(`${KNOWLEDGE_CHUNKER_VERSION}\0${parseArtifactId}\0${ordinal}`, "utf8")
    .digest("hex")
    .slice(0, 32);
  return `chunk_${digest}`;
}

/**
 * Chunk 是可重建检索派生物；每个字符区间仍精确指回原始 Block。
 */
export function buildKnowledgeChunks(
  parseArtifactId: string,
  blocks: readonly KnowledgeBlock[],
): KnowledgeChunkDraft[] {
  const chunks: KnowledgeChunkDraft[] = [];
  let currentText = "";
  let currentSpans: KnowledgeChunkSpanDraft[] = [];

  const flush = () => {
    if (!currentText) return;
    const ordinal = chunks.length;
    chunks.push({
      id: deterministicChunkId(parseArtifactId, ordinal),
      parseArtifactId,
      ordinal,
      text: currentText,
      tokenCount: Math.max(1, Math.ceil(Array.from(currentText).length / 4)),
      spans: currentSpans,
    });
    currentText = "";
    currentSpans = [];
  };

  const appendPiece = (block: KnowledgeBlock, start: number, end: number) => {
    const piece = block.text.slice(start, end);
    if (!piece) return;
    const separator = currentText ? "\n\n" : "";
    if (currentText && currentText.length + separator.length + piece.length > KNOWLEDGE_CHUNK_TARGET_CHARS) {
      flush();
    }
    const chunkStartOffset = currentText.length + (currentText ? 2 : 0);
    if (currentText) currentText += "\n\n";
    currentText += piece;
    currentSpans.push({
      blockId: block.id,
      blockStartOffset: start,
      blockEndOffset: end,
      chunkStartOffset,
      chunkEndOffset: chunkStartOffset + piece.length,
    });
  };

  for (const block of blocks) {
    let start = 0;
    while (start < block.text.length) {
      const remainingCapacity = currentText
        ? KNOWLEDGE_CHUNK_TARGET_CHARS - currentText.length - 2
        : KNOWLEDGE_CHUNK_TARGET_CHARS;
      if (remainingCapacity < Math.floor(KNOWLEDGE_CHUNK_TARGET_CHARS * 0.35)) {
        flush();
        continue;
      }
      const end = safeSplitEnd(block.text, start, start + remainingCapacity);
      appendPiece(block, start, end);
      start = end;
      if (start < block.text.length) flush();
    }
  }
  flush();
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
