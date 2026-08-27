import crypto from "node:crypto";

import { KnowledgeError } from "./errors.ts";
import type { ResearchBatchDraft, ResearchUnitDraft } from "./research-store.ts";
import type { KnowledgeBlock, KnowledgeScopeSnapshot } from "./types.ts";

export const DEFAULT_ANALYSIS_UNIT_PRIMARY_CHARS = 6_000;
export const DEFAULT_EXECUTION_BATCH_CHARS = 18_000;
const CONTEXT_CHARS = 240;
const MAX_UNITS_PER_BATCH = 2;

function stableId(prefix: string, ...parts: Array<string | number>): string {
  const digest = crypto.createHash("sha256").update(parts.join("\u0000"), "utf8").digest("hex");
  return `${prefix}_${digest.slice(0, 32)}`;
}

function addContextSpans(unit: ResearchUnitDraft, blocks: KnowledgeBlock[]) {
  const primary = unit.spans.filter(span => span.kind === "primary");
  const first = primary[0];
  const last = primary.at(-1)!;
  const contexts: ResearchUnitDraft["spans"] = [];
  const firstBlock = blocks[first.blockOrdinal];
  const lastBlock = blocks[last.blockOrdinal];

  if (first.startOffset > 0) {
    contexts.push({
      kind: "context",
      ordinal: contexts.length,
      blockId: first.blockId,
      blockOrdinal: first.blockOrdinal,
      startOffset: Math.max(0, first.startOffset - CONTEXT_CHARS),
      endOffset: first.startOffset,
    });
  } else if (first.blockOrdinal > 0) {
    const previous = blocks[first.blockOrdinal - 1];
    contexts.push({
      kind: "context",
      ordinal: contexts.length,
      blockId: previous.id,
      blockOrdinal: previous.ordinal,
      startOffset: Math.max(0, previous.text.length - CONTEXT_CHARS),
      endOffset: previous.text.length,
    });
  }

  if (last.endOffset < lastBlock.text.length) {
    contexts.push({
      kind: "context",
      ordinal: contexts.length,
      blockId: last.blockId,
      blockOrdinal: last.blockOrdinal,
      startOffset: last.endOffset,
      endOffset: Math.min(lastBlock.text.length, last.endOffset + CONTEXT_CHARS),
    });
  } else if (last.blockOrdinal + 1 < blocks.length) {
    const next = blocks[last.blockOrdinal + 1];
    contexts.push({
      kind: "context",
      ordinal: contexts.length,
      blockId: next.id,
      blockOrdinal: next.ordinal,
      startOffset: 0,
      endOffset: Math.min(next.text.length, CONTEXT_CHARS),
    });
  }

  unit.spans.push(...contexts.filter(span => span.endOffset > span.startOffset));
  unit.contextCharCount = contexts.reduce(
    (total, span) => total + Math.max(0, span.endOffset - span.startOffset),
    0,
  );
}

function buildArtifactUnits(input: {
  runId: string;
  artifactId: string;
  blocks: KnowledgeBlock[];
  targetChars: number;
  prioritizedBlockIds: Set<string>;
}): ResearchUnitDraft[] {
  const units: ResearchUnitDraft[] = [];
  let unit: ResearchUnitDraft | null = null;

  const ensureUnit = () => {
    if (!unit) {
      const ordinal = units.length;
      unit = {
        id: stableId("aunit", input.runId, input.artifactId, ordinal),
        parseArtifactId: input.artifactId,
        ordinal,
        priority: 1,
        primaryCharCount: 0,
        contextCharCount: 0,
        spans: [],
      };
    }
    return unit;
  };

  const publishUnit = () => {
    if (!unit || unit.primaryCharCount === 0) return;
    if (unit.spans.some(span => input.prioritizedBlockIds.has(span.blockId))) unit.priority = 0;
    addContextSpans(unit, input.blocks);
    units.push(unit);
    unit = null;
  };

  for (const block of input.blocks) {
    if (!block.text.length) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Citation-grade blocks must not be empty");
    }
    let startOffset = 0;
    while (startOffset < block.text.length) {
      const current = ensureUnit();
      const available = input.targetChars - current.primaryCharCount;
      const endOffset = Math.min(block.text.length, startOffset + available);
      current.spans.push({
        kind: "primary",
        ordinal: current.spans.filter(span => span.kind === "primary").length,
        blockId: block.id,
        blockOrdinal: block.ordinal,
        startOffset,
        endOffset,
      });
      current.primaryCharCount += endOffset - startOffset;
      startOffset = endOffset;
      if (current.primaryCharCount >= input.targetChars) publishUnit();
    }
  }
  publishUnit();
  return units;
}

export function assertCompletePrimaryCoverage(input: {
  units: ResearchUnitDraft[];
  blocksByArtifact: Map<string, KnowledgeBlock[]>;
}) {
  for (const [artifactId, blocks] of input.blocksByArtifact) {
    const artifactUnits = input.units.filter(unit => unit.parseArtifactId === artifactId);
    for (const block of blocks) {
      const spans = artifactUnits
        .flatMap(unit => unit.spans)
        .filter(span => span.kind === "primary" && span.blockId === block.id)
        .sort((a, b) => a.startOffset - b.startOffset);
      let cursor = 0;
      for (const span of spans) {
        if (span.startOffset !== cursor || span.endOffset > block.text.length) {
          throw new KnowledgeError(
            "KNOWLEDGE_STORAGE_INVALID",
            "AnalysisUnit primary ranges contain a gap or overlap",
            { artifactId, blockId: block.id, expectedOffset: cursor, actualOffset: span.startOffset },
          );
        }
        cursor = span.endOffset;
      }
      if (cursor !== block.text.length) {
        throw new KnowledgeError(
          "KNOWLEDGE_STORAGE_INVALID",
          "AnalysisUnit primary ranges do not fully cover a block",
          { artifactId, blockId: block.id, expectedOffset: block.text.length, actualOffset: cursor },
        );
      }
    }
  }
}

export function buildAnalysisManifest(input: {
  runId: string;
  scope: KnowledgeScopeSnapshot;
  blocksByArtifact: Map<string, KnowledgeBlock[]>;
  prioritizedBlockIds?: Set<string>;
  targetUnitChars?: number;
  targetBatchChars?: number;
}): {
  sourceCount: number;
  parseArtifactCount: number;
  blockCount: number;
  primaryCharCount: number;
  units: ResearchUnitDraft[];
  batches: ResearchBatchDraft[];
} {
  const targetUnitChars = input.targetUnitChars ?? DEFAULT_ANALYSIS_UNIT_PRIMARY_CHARS;
  const targetBatchChars = input.targetBatchChars ?? DEFAULT_EXECUTION_BATCH_CHARS;
  if (!Number.isSafeInteger(targetUnitChars) || targetUnitChars <= 0) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "AnalysisUnit size is invalid");
  }
  if (!Number.isSafeInteger(targetBatchChars) || targetBatchChars < targetUnitChars) {
    throw new KnowledgeError("KNOWLEDGE_INVALID_ARGUMENT", "ExecutionBatch size is invalid");
  }
  const artifactIds = [...new Set(input.scope.sources.map(source => source.parseArtifactId))];
  const units: ResearchUnitDraft[] = [];
  for (const artifactId of artifactIds) {
    const blocks = input.blocksByArtifact.get(artifactId);
    if (!blocks?.length || blocks.some((block, index) => block.ordinal !== index)) {
      throw new KnowledgeError("KNOWLEDGE_STORAGE_INVALID", "Frozen ParseArtifact blocks are incomplete");
    }
    units.push(...buildArtifactUnits({
      runId: input.runId,
      artifactId,
      blocks,
      targetChars: targetUnitChars,
      prioritizedBlockIds: input.prioritizedBlockIds || new Set(),
    }));
  }
  assertCompletePrimaryCoverage({ units, blocksByArtifact: input.blocksByArtifact });

  const orderedUnits = [...units].sort((left, right) => (
    left.priority - right.priority
    || artifactIds.indexOf(left.parseArtifactId) - artifactIds.indexOf(right.parseArtifactId)
    || left.ordinal - right.ordinal
  ));
  const batches: ResearchBatchDraft[] = [];
  let current: ResearchUnitDraft[] = [];
  let currentChars = 0;
  const publishBatch = () => {
    if (current.length === 0) return;
    const ordinal = batches.length;
    batches.push({
      id: stableId("ebatch", input.runId, ordinal),
      ordinal,
      estimatedChars: currentChars,
      unitIds: current.map(unit => unit.id),
    });
    current = [];
    currentChars = 0;
  };
  for (const unit of orderedUnits) {
    const unitChars = unit.primaryCharCount + unit.contextCharCount;
    if (current.length > 0 && (
      currentChars + unitChars > targetBatchChars
      || current.length >= MAX_UNITS_PER_BATCH
    )) publishBatch();
    current.push(unit);
    currentChars += unitChars;
  }
  publishBatch();

  return {
    sourceCount: input.scope.sources.length,
    parseArtifactCount: artifactIds.length,
    blockCount: [...input.blocksByArtifact.values()].reduce((total, blocks) => total + blocks.length, 0),
    primaryCharCount: units.reduce((total, unit) => total + unit.primaryCharCount, 0),
    units,
    batches,
  };
}
