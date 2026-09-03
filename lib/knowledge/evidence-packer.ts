import crypto from "node:crypto";
import { estimateTextTokens } from "../llm/estimate-text-tokens.ts";
import { buildWarningLine, markUntrusted, scan } from "../security/injection-scan.ts";
import type { KnowledgeEvidenceSpan } from "../../shared/knowledge-evidence.ts";
import { KnowledgeError } from "./errors.ts";
import { trimEvidenceSpan } from "./evidence-span-extractor.ts";
import {
  KNOWLEDGE_FAST_MAX_EVIDENCE_SPANS,
  KNOWLEDGE_FAST_PER_SPAN_MAX_TOKENS,
  KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS,
  type FastKnowledgeEvidenceStages,
  type FastKnowledgePackedEvidence,
} from "./fast-knowledge-pipeline.ts";
import { knowledgeChunkIndexVariantId } from "./knowledge-index-store.ts";
import type { KnowledgeEvidenceIdentityEntry } from "./knowledge-context-injector.ts";

type PackingInput = Parameters<FastKnowledgeEvidenceStages["packEvidence"]>[0];

function renderEvidence(span: KnowledgeEvidenceSpan, ordinal: number): string {
  const location = [
    span.headingPath?.join(" > "),
    span.pageNumber != null ? `page ${span.pageNumber}` : null,
    `block ${span.blockId} offsets ${span.startOffset}-${span.endOffset}`,
  ].filter(Boolean).join("; ");
  const evidence = `[K${ordinal}]\nSource: ${span.sourceName}\nLocation: ${location}\nEvidence:\n${span.text}`;
  const warning = buildWarningLine(scan(evidence).decision);
  return markUntrusted(warning ? `${warning}\n${evidence}` : evidence);
}

/** 同一组已验证范围决定注入正文、连续引用编号和持久化身份清单。 */
export class EvidencePacker {
  pack(input: PackingInput): FastKnowledgePackedEvidence {
    const scope = input.compiledScope;
    const header = ["[KnowledgeContext]", "Mode: fast", "Execution path: local FTS", `Scope: ${scope.scopeId}`,
      `Retrieval deadline: ${input.deadlineMs}ms`, `Deadline exceeded: ${input.deadlineExceeded ? "yes" : "no"}`, ""].join("\n");
    const footer = ["", "Instructions:", "- Answer only from the evidence above.",
      "- Cite evidence ids using {{cite:N}} for [KN].",
      "- If the evidence is insufficient, say so explicitly.", "[/KnowledgeContext]"].join("\n");
    const render = (spans: KnowledgeEvidenceSpan[]) => header + spans.map((span, index) => renderEvidence(span, index + 1)).join("\n\n") + footer;
    const sources = new Map(scope.sources.map(source => [source.sourceId, source]));
    const hits = new Map(input.hits.map(hit => [hit.id, hit]));
    const readySourceCount = scope.sources.filter(source => source.status === "ready").length;
    const selected: KnowledgeEvidenceSpan[] = [];
    const selectedIds = new Set<string>();
    const sourceCounts = new Map<string, number>();

    const add = (original: KnowledgeEvidenceSpan) => {
      if (selected.length >= KNOWLEDGE_FAST_MAX_EVIDENCE_SPANS || selectedIds.has(original.id)) return;
      if (readySourceCount !== 1 && (sourceCounts.get(original.sourceId) ?? 0) >= 3) return;
      const source = sources.get(original.sourceId);
      const hit = original.chunkId ? hits.get(original.chunkId) : null;
      if (!source || source.contentSnapshotId !== original.contentSnapshotId
        || source.parseArtifactId !== original.parseArtifactId || !hit
        || hit.parseArtifactId !== original.parseArtifactId || hit.chunkIndexVariantId !== original.chunkIndexVariantId
        || !scope.readyChunkVariantIds.includes(hit.chunkIndexVariantId)) {
        throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Evidence identity does not match the frozen scope");
      }
      if (original.text.length !== original.endOffset - original.startOffset
        || crypto.createHash("sha256").update(original.text).digest("hex") !== original.textSha256) {
        throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Canonical evidence text hash or offsets mismatch");
      }
      let span = original;
      if (selected.length === 0 && estimateTextTokens(render([span])) > KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS) {
        span = trimEvidenceSpan(span, KNOWLEDGE_FAST_PER_SPAN_MAX_TOKENS);
      }
      if (estimateTextTokens(render([...selected, span])) > KNOWLEDGE_FAST_RENDER_BUDGET_TOKENS) return;
      selected.push(span);
      selectedIds.add(original.id);
      sourceCounts.set(span.sourceId, (sourceCounts.get(span.sourceId) ?? 0) + 1);
    };
    const visitedSources = new Set<string>();
    for (const span of input.spans) {
      if (!visitedSources.has(span.sourceId)) {
        visitedSources.add(span.sourceId);
        add(span);
      }
    }
    for (const span of input.spans) add(span);

    const entries = new Map<string, KnowledgeEvidenceIdentityEntry>();
    for (const [index, span] of selected.entries()) {
      const hit = hits.get(span.chunkId!)!;
      const location = hit.spans.find(item => item.blockId === span.blockId
        && item.blockStartOffset <= span.startOffset && item.blockEndOffset >= span.endOffset);
      if (!location) throw new KnowledgeError("KNOWLEDGE_INDEX_INVALID", "Evidence offsets are outside their chunk span");
      const notebook = scope.notebooks.find(item => item.sourceIds.includes(span.sourceId) && item.chunkProfileHash
        && knowledgeChunkIndexVariantId(span.parseArtifactId, item.chunkProfileHash) === hit.chunkIndexVariantId);
      if (!notebook) throw new KnowledgeError("KNOWLEDGE_SCOPE_VIOLATION", "Evidence profile is outside the frozen scope");
      let entry = entries.get(hit.id);
      if (!entry) {
        entry = {
          chunkId: hit.id, ordinal: hit.ordinal, parseArtifactId: hit.parseArtifactId,
          chunkIndexVariantId: hit.chunkIndexVariantId, chunkProfileHash: notebook.chunkProfileHash,
          sourceId: span.sourceId, notebookId: notebook.notebookId, contextOnly: false,
          citationLabels: [], blockSpans: [],
        };
        entries.set(hit.id, entry);
      }
      entry.citationLabels.push(`K${index + 1}`);
      entry.blockSpans.push({
        blockId: span.blockId,
        blockStartOffset: span.startOffset,
        blockEndOffset: span.endOffset,
        chunkStartOffset: location.chunkStartOffset + span.startOffset - location.blockStartOffset,
        chunkEndOffset: location.chunkStartOffset + span.endOffset - location.blockStartOffset,
      });
    }
    const block = render(selected);
    return { block, spans: selected, usedTokens: estimateTextTokens(block),
      evidence: { entries: [...entries.values()], searchedVectorVariants: [] } };
  }
}
