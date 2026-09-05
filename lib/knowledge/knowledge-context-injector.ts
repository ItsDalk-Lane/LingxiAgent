/** 快速检索和详细研究共用的精确证据身份与清单门面；不包含查询或研究编排。 */
import { KnowledgeError } from "./errors.ts";
import type { KnowledgeChunkSpanDraft } from "./chunker.ts";
import type { KnowledgeEvidenceManifestEntry, KnowledgeTurnScope } from "./types.ts";
export { EvidencePacker } from "./evidence-packer.ts";

/** 真实引文在冻结分块中的定位；只承载身份和偏移，不承载正文。 */
export interface KnowledgeEvidenceIdentityEntry {
  chunkId: string;
  /** chunk 在变体内的 0-based ordinal（与 knowledge_read / stats.chunkOrdinal-1 同源）。 */
  ordinal: number;
  parseArtifactId: string;
  chunkIndexVariantId: string;
  chunkProfileHash: string | null;
  sourceId: string;
  notebookId: string;
  contextOnly: boolean;
  citationLabels: string[];
  blockSpans: KnowledgeChunkSpanDraft[];
}

/**
 * 一轮注入的身份链载荷（任务书 §六十七）：entries 为实际进入注入链路的块
 * 级身份；searchedVectorVariants 汇总本轮各检索结果实际参与向量搜索的变体
 * 身份（fts-only 轮为空数组）。由快速证据装填或研究原文映射随 block/stats
 * 一起产出，engine 侧组装 EvidenceManifest 落库；不进 KnowledgeRetrievalStats
 * （展示 stats 与持久化 manifest 分离）。
 */
export interface KnowledgeInjectionEvidence {
  entries: KnowledgeEvidenceIdentityEntry[];
  searchedVectorVariants: Array<{
    parseArtifactId: string;
    chunkProfileHash: string;
    chunkIndexVariantId: string;
    vectorIndexVariantId: string;
  }>;
}

/**
 * EvidenceManifest 条目组装（任务书 §六十七）：把注入产出的块级身份链按
 * (source, chunkIndexVariant) 分组成 manifest 条目。服务端复核（不信任任何
 * 外部传入 id）：每条 entry 的 sourceId 必须在 TurnScope 冻结集合内，且其
 * parseArtifactId 必须与冻结行一致——不一致即抛错（宁可拒写不可伪造身份）。
 * 同源多分块配置（v9 起变体并存）天然得到多条目。纯函数，无 IO。
 */
export function assembleKnowledgeEvidenceManifestEntries(input: {
  turnScope: KnowledgeTurnScope;
  evidence: KnowledgeInjectionEvidence;
}): KnowledgeEvidenceManifestEntry[] {
  const frozenBySource = new Map(input.turnScope.sources.map(source => [source.sourceId, source]));
  interface EntryGroup {
    sourceId: string;
    contentSnapshotId: string;
    parseArtifactId: string | null;
    chunkIndexVariantId: string | null;
    chunkProfileHash: string | null;
    chunkIds: string[];
    neighborChunkIds: string[];
    blockSpans: Array<{ chunkId: string; spans: KnowledgeChunkSpanDraft[] }>;
    citationLabels: string[];
  }
  const groups = new Map<string, EntryGroup>();
  for (const entry of input.evidence.entries) {
    const frozen = frozenBySource.get(entry.sourceId);
    if (!frozen) {
      throw new KnowledgeError(
        "KNOWLEDGE_SCOPE_VIOLATION",
        `evidence entry references source outside the frozen turn scope: ${entry.sourceId}`,
      );
    }
    if (frozen.parseArtifactId !== entry.parseArtifactId) {
      throw new KnowledgeError(
        "KNOWLEDGE_CONFLICT",
        `evidence entry artifact ${entry.parseArtifactId} does not match the frozen scope artifact of source ${entry.sourceId}`,
      );
    }
    const key = `${entry.sourceId}\0${entry.chunkIndexVariantId}`;
    let group = groups.get(key);
    if (!group) {
      group = {
        sourceId: entry.sourceId,
        contentSnapshotId: frozen.contentSnapshotId,
        parseArtifactId: entry.parseArtifactId,
        chunkIndexVariantId: entry.chunkIndexVariantId,
        chunkProfileHash: entry.chunkProfileHash,
        chunkIds: [],
        neighborChunkIds: [],
        blockSpans: [],
        citationLabels: [],
      };
      groups.set(key, group);
    }
    if (entry.contextOnly) {
      if (!group.neighborChunkIds.includes(entry.chunkId)) group.neighborChunkIds.push(entry.chunkId);
    } else if (!group.chunkIds.includes(entry.chunkId)) {
      group.chunkIds.push(entry.chunkId);
    }
    if (!group.blockSpans.some(span => span.chunkId === entry.chunkId)) {
      group.blockSpans.push({ chunkId: entry.chunkId, spans: entry.blockSpans });
    }
    for (const label of entry.citationLabels) {
      if (!group.citationLabels.includes(label)) group.citationLabels.push(label);
    }
  }
  return [...groups.values()].map((group, ordinal) => ({
    ordinal,
    ...group,
    // 向量变体身份按 chunkIndexVariant 关联（多嵌入模型引用可并列；fts-only 空）。
    vectorIndexVariantIds: [...new Set(
      input.evidence.searchedVectorVariants
        .filter(variant => variant.chunkIndexVariantId === group.chunkIndexVariantId)
        .map(variant => variant.vectorIndexVariantId),
    )],
  }));
}
