import { lingxiFetch, lingxiUrl } from '../../hooks/use-hana-fetch';

export interface KnowledgeModelRefDto {
  id: string;
  provider: string;
}

/**
 * 笔记本级配置（v8 语义）：模型引用 null = 未配置（检索降级纯全文）；
 * chunkTargetChars null = 自动分块（遗留显式值仍生效）；retrievalTopK
 * null = 无上限召回。
 */
export interface KnowledgeNotebookConfigDto {
  embeddingModelRef: KnowledgeModelRefDto | null;
  rerankModelRef: KnowledgeModelRefDto | null;
  chunkTargetChars: number | null;
  retrievalTopK: number | null;
  /** null = 永久保留（默认）；正整数 = 旧版本向量 N 天未被查询命中即回收。 */
  vectorRetentionDays: number | null;
}

/** 按每个源的最新摄入 job 归类的就绪汇总。 */
export interface KnowledgeNotebookIngestionDto {
  done: number;
  pendingEmbedding: number;
  processing: number;
  failed: number;
  untracked: number;
}

export interface KnowledgeNotebookDto {
  id: string;
  studioId: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
  config: KnowledgeNotebookConfigDto;
  /** 生效分块尺寸（遗留显式列 > 嵌入模型上下文 ×80% 自动值），设置弹窗只读展示。 */
  chunkTargetCharsEffective?: number | null;
  sourceCount: number;
  ingestion: KnowledgeNotebookIngestionDto;
}

export type KnowledgeIngestionStatusDto = 'queued' | 'running' | 'pending_embedding' | 'failed' | 'done';
export type KnowledgeIngestionPhaseDto = 'parse' | 'chunk' | 'fts_index' | 'embed' | 'done';

export interface KnowledgeIngestionJobDto {
  id: string;
  notebookId: string;
  sourceId: string;
  artifactId: string | null;
  phase: KnowledgeIngestionPhaseDto;
  status: KnowledgeIngestionStatusDto;
  attempt: number;
  retryAfter: string | null;
  error: string | null;
  chunkerConfigId: string;
  createdAt: string;
  updatedAt: string;
  /** embed 阶段进度：progressDone 从 0 递增；progressTotal null = 未进入 embed 阶段。 */
  progressDone?: number;
  progressTotal?: number | null;
}

export interface KnowledgeIngestionStateDto {
  jobs: KnowledgeIngestionJobDto[];
  counts: Record<KnowledgeIngestionStatusDto, number>;
  /** 文件 watch 检出"源文件不可达"的源（仅不可达项；老服务端可能不返回该字段） */
  unreachableSources?: Array<{
    sourceId: string;
    studioId: string;
    notebooks: string[];
    watching: boolean;
    unreachable: boolean;
    unreachableReason: string | null;
    unreachableSince: string | null;
  }>;
}

export type KnowledgeParseStatusDto = 'parsing' | 'ready' | 'needs_ocr' | 'failed';

export interface KnowledgeParseArtifactDto {
  id: string;
  contentSnapshotId: string;
  parserId: string;
  parserVersion: string;
  parserConfigHash: string;
  status: KnowledgeParseStatusDto;
  warnings: string[];
  createdAt: string;
  completedAt: string | null;
}

export interface KnowledgeSourceEntryDto {
  source: {
    id: string;
    studioId: string;
    sourceType: 'file' | 'pasted_text' | 'web_snapshot';
    displayName: string;
    originMetadata: { kind?: string; fileName?: string; url?: string; fetchedAt?: string };
    createdAt: string;
    deletedAt: string | null;
  };
  snapshot: {
    id: string;
    sourceId: string;
    sha256: string;
    mimeType: string;
    byteSize: number;
    capturedAt: string;
  };
  membership: {
    notebookId: string;
    sourceId: string;
    addedAt: string;
    removedAt: string | null;
  };
  parseArtifact: KnowledgeParseArtifactDto | null;
}

export interface KnowledgeBlockDto {
  id: string;
  parseArtifactId: string;
  ordinal: number;
  text: string;
  textSha256: string;
  locatorType: 'text' | 'markdown' | 'pdf' | 'html';
  locator: Record<string, unknown>;
}

export interface KnowledgeCitationDto {
  id: string;
  parseArtifactId: string;
  blockId: string;
  startOffset: number;
  endOffset: number;
  canonicalText: string;
  canonicalTextSha256: string;
  createdAt: string;
}

export interface KnowledgeResolvedCitationDto {
  citation: KnowledgeCitationDto;
  block: KnowledgeBlockDto;
  parseArtifact: KnowledgeParseArtifactDto;
  snapshot: KnowledgeSourceEntryDto['snapshot'];
  source: KnowledgeSourceEntryDto['source'];
  viewer: {
    contentUrl: string;
    locator: Record<string, unknown>;
  };
}

interface KnowledgeApiErrorBody {
  error?: string;
  message?: string;
}

async function knowledgeRequest<T>(path: string, options?: RequestInit): Promise<T> {
  const response = await lingxiFetch(path, { ...options, throwOnHttpError: false });
  let body: T | KnowledgeApiErrorBody;
  try {
    body = await response.json() as T | KnowledgeApiErrorBody;
  } catch {
    throw new Error(`Knowledge request failed (${response.status})`);
  }
  if (!response.ok) {
    const errorBody = body as KnowledgeApiErrorBody;
    throw new Error(errorBody.message || errorBody.error || `Knowledge request failed (${response.status})`);
  }
  return body as T;
}

export async function listKnowledgeNotebooks(): Promise<KnowledgeNotebookDto[]> {
  const data = await knowledgeRequest<{ notebooks: KnowledgeNotebookDto[] }>('/api/knowledge/notebooks');
  return data.notebooks;
}

export async function createKnowledgeNotebook(name: string): Promise<KnowledgeNotebookDto> {
  const data = await knowledgeRequest<{ notebook: KnowledgeNotebookDto }>('/api/knowledge/notebooks', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  return data.notebook;
}

export async function renameKnowledgeNotebook(id: string, name: string): Promise<KnowledgeNotebookDto> {
  const data = await knowledgeRequest<{ notebook: KnowledgeNotebookDto }>(
    `/api/knowledge/notebooks/${encodeURIComponent(id)}`,
    {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name }),
    },
  );
  return data.notebook;
}

export async function deleteKnowledgeNotebook(id: string): Promise<void> {
  await knowledgeRequest(`/api/knowledge/notebooks/${encodeURIComponent(id)}`, { method: 'DELETE' });
}

/** 笔记本设置部分更新：字段 omitted=不变、null=清除（未配置/无上限召回）。 */
export async function updateKnowledgeNotebookSettings(
  id: string,
  settings: {
    embeddingModelRef?: KnowledgeModelRefDto | null;
    rerankModelRef?: KnowledgeModelRefDto | null;
    retrievalTopK?: number | null;
    vectorRetentionDays?: number | null;
  },
): Promise<KnowledgeNotebookConfigDto> {
  const data = await knowledgeRequest<{ config: KnowledgeNotebookConfigDto }>(
    `/api/knowledge/notebooks/${encodeURIComponent(id)}/settings`,
    {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(settings),
    },
  );
  return data.config;
}

export async function listKnowledgeIngestion(input: {
  notebookId?: string;
  sourceId?: string;
}): Promise<KnowledgeIngestionStateDto> {
  const params = new URLSearchParams();
  if (input.notebookId) params.set('notebookId', input.notebookId);
  if (input.sourceId) params.set('sourceId', input.sourceId);
  const query = params.toString();
  return knowledgeRequest<KnowledgeIngestionStateDto>(
    `/api/knowledge/ingestion${query ? `?${query}` : ''}`,
  );
}

/** failed 摄入 job 手动重试；无 job 时服务端兜底入队。 */
export async function reingestKnowledgeSource(
  notebookId: string,
  sourceId: string,
): Promise<{ job: KnowledgeIngestionJobDto; retried: boolean }> {
  return knowledgeRequest<{ job: KnowledgeIngestionJobDto; retried: boolean }>(
    `/api/knowledge/notebooks/${encodeURIComponent(notebookId)}/sources/${encodeURIComponent(sourceId)}/reingest`,
    { method: 'POST' },
  );
}

export async function listKnowledgeSources(notebookId: string): Promise<KnowledgeSourceEntryDto[]> {
  const data = await knowledgeRequest<{ sources: KnowledgeSourceEntryDto[] }>(
    `/api/knowledge/notebooks/${encodeURIComponent(notebookId)}/sources`,
  );
  return data.sources;
}

export async function importKnowledgeFileSource(
  notebookId: string,
  filePath: string,
): Promise<KnowledgeSourceEntryDto> {
  return knowledgeRequest<KnowledgeSourceEntryDto>(
    `/api/knowledge/notebooks/${encodeURIComponent(notebookId)}/sources`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ kind: 'file', filePath }),
    },
  );
}

export async function importKnowledgePastedText(
  notebookId: string,
  input: { text: string; displayName?: string },
): Promise<KnowledgeSourceEntryDto> {
  return knowledgeRequest<KnowledgeSourceEntryDto>(
    `/api/knowledge/notebooks/${encodeURIComponent(notebookId)}/sources`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ kind: 'pasted_text', ...input }),
    },
  );
}

export async function importKnowledgeWebSnapshot(
  notebookId: string,
  input: { url: string; displayName?: string },
): Promise<KnowledgeSourceEntryDto> {
  return knowledgeRequest<KnowledgeSourceEntryDto>(
    `/api/knowledge/notebooks/${encodeURIComponent(notebookId)}/sources`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ kind: 'web_snapshot', ...input }),
    },
  );
}

export interface KnowledgeDirectoryImportResultDto {
  imported: Array<{
    sourceId: string;
    path: string;
    reused: boolean;
    ingestion: 'enqueued' | 'parse_failed_enqueued_for_retry';
  }>;
  skipped: Array<{ path: string; reason: string }>;
  failed: Array<{ path: string; reason: string }>;
}

export async function importKnowledgeDirectory(
  notebookId: string,
  dirPath: string,
): Promise<KnowledgeDirectoryImportResultDto> {
  return knowledgeRequest<KnowledgeDirectoryImportResultDto>(
    `/api/knowledge/notebooks/${encodeURIComponent(notebookId)}/import-directory`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ dirPath }),
    },
  );
}

export async function removeKnowledgeSource(notebookId: string, sourceId: string): Promise<void> {
  await knowledgeRequest(
    `/api/knowledge/notebooks/${encodeURIComponent(notebookId)}/sources/${encodeURIComponent(sourceId)}`,
    { method: 'DELETE' },
  );
}

export async function refreshKnowledgeSource(
  notebookId: string,
  sourceId: string,
): Promise<KnowledgeSourceEntryDto & { changed: boolean }> {
  return knowledgeRequest<KnowledgeSourceEntryDto & { changed: boolean }>(
    `/api/knowledge/notebooks/${encodeURIComponent(notebookId)}/sources/${encodeURIComponent(sourceId)}/refresh`,
    { method: 'POST' },
  );
}

export async function listKnowledgeBlocks(parseArtifactId: string): Promise<KnowledgeBlockDto[]> {
  const data = await knowledgeRequest<{ blocks: KnowledgeBlockDto[] }>(
    `/api/knowledge/parse-artifacts/${encodeURIComponent(parseArtifactId)}/blocks`,
  );
  return data.blocks;
}

/** 摄入分块卡片（GET .../chunks；ordinal 为 1-based，定位信息来自 block locator 组装）。 */
export interface KnowledgeChunkDto {
  id: string;
  ordinal: number;
  text: string;
  tokenCount: number;
  charCount: number;
  headingPath?: string[];
  pageNumber?: number;
}

export interface KnowledgeChunksDto {
  chunkerConfigId: string;
  chunks: KnowledgeChunkDto[];
}

/** 分块内容视图数据；artifact 未 ready 时服务端返回 422。 */
export async function listKnowledgeChunks(parseArtifactId: string): Promise<KnowledgeChunksDto> {
  return knowledgeRequest<KnowledgeChunksDto>(
    `/api/knowledge/parse-artifacts/${encodeURIComponent(parseArtifactId)}/chunks`,
  );
}

export async function resolveKnowledgeCitation(citationId: string): Promise<KnowledgeResolvedCitationDto> {
  return knowledgeRequest<KnowledgeResolvedCitationDto>(
    `/api/knowledge/citations/${encodeURIComponent(citationId)}`,
  );
}

export function knowledgeSnapshotContentUrl(snapshotId: string): string {
  return lingxiUrl(`/api/knowledge/snapshots/${encodeURIComponent(snapshotId)}/content`);
}
