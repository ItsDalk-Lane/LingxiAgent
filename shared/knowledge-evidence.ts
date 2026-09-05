/** 原始解析块中的精确证据范围；偏移按原文字符串坐标计算。 */
export interface KnowledgeEvidenceSpan {
  id: string;
  sourceId: string;
  sourceName: string;
  notebookIds: string[];
  contentSnapshotId: string;
  parseArtifactId: string;
  chunkIndexVariantId: string | null;
  chunkId: string | null;
  blockId: string;
  startOffset: number;
  endOffset: number;
  text: string;
  textSha256: string;
  headingPath: string[] | null;
  pageNumber: number | null;
  retrievalChannels: Array<"fts" | "vector" | "grep" | "ordinal_read">;
  score: number | null;
}
