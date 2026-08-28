import { lingxiFetch, lingxiUrl } from '../../hooks/use-hana-fetch';

export interface KnowledgeNotebookDto {
  id: string;
  studioId: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
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

export interface KnowledgeScopeSnapshotDto {
  id: string;
  studioId: string;
  mode: 'quick' | 'research';
  createdAt: string;
  notebooks: Array<{
    scopeSnapshotId: string;
    notebookId: string;
    notebookName: string;
    ordinal: number;
  }>;
  sources: Array<{
    scopeSnapshotId: string;
    notebookId: string;
    sourceId: string;
    sourceDisplayName: string;
    contentSnapshotId: string;
    parseArtifactId: string;
    ordinal: number;
  }>;
}

export interface KnowledgeRunDto {
  id: string;
  studioId: string;
  mode: 'quick' | 'research';
  question: string;
  scopeSnapshotId: string;
  status: 'running' | 'completed' | 'failed' | 'cancelled';
  retrievalMode: 'fts' | 'hybrid';
  answerText: string | null;
  errorCode: string | null;
  createdAt: string;
  completedAt: string | null;
  citations: Array<{
    runId: string;
    ordinal: number;
    marker: number;
    citationId: string;
    candidateRef: string;
  }>;
  retrievals: Array<{
    runId: string;
    rank: number;
    chunkId: string;
    parseArtifactId: string;
    score: number;
  }>;
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

export interface KnowledgeQueryResultDto {
  run: KnowledgeRunDto;
  scope: KnowledgeScopeSnapshotDto;
  retrievalBasis: 'related_content';
  citations: Array<Omit<KnowledgeResolvedCitationDto, 'block'> & {
    marker: number;
    locator: Record<string, unknown>;
  }>;
}

export interface KnowledgeCoverageMetricDto {
  completed: number;
  total: number;
}

export interface KnowledgeResearchCoverageDto {
  sourceReadiness: KnowledgeCoverageMetricDto;
  extraction: KnowledgeCoverageMetricDto;
  primaryScan: KnowledgeCoverageMetricDto;
  contradiction: KnowledgeCoverageMetricDto;
  citationValidation: KnowledgeCoverageMetricDto & { valid: number; invalid: number };
}

export interface KnowledgeResearchRunDto {
  runId: string;
  hostTaskId: string;
  state:
    | 'queued'
    | 'preparing_scope'
    | 'building_manifest'
    | 'scanning'
    | 'building_claims'
    | 'checking_contradictions'
    | 'synthesizing'
    | 'completed'
    | 'recovering'
    | 'partial'
    | 'failed'
    | 'canceled';
  spec: {
    originalQuestion: string;
    scopeSnapshotId: string;
    notebookIds: string[];
    goal: string;
    dimensions: string[];
    outputRequirements: string[];
    definitions: string[];
    assumptions: string[];
  };
  manifest: {
    runId: string;
    sourceCount: number;
    parseArtifactCount: number;
    blockCount: number;
    unitCount: number;
    primaryCharCount: number;
    createdAt: string;
  } | null;
  coverage: KnowledgeResearchCoverageDto;
  reportAvailable: boolean;
  errorCode: string | null;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface KnowledgeResearchRunResultDto {
  run: KnowledgeRunDto;
  scope: KnowledgeScopeSnapshotDto;
  research: KnowledgeResearchRunDto;
  citations: [];
}

export interface KnowledgeResearchReportItemDto {
  text: string;
  claimIds: string[];
  citationMarkers: number[];
}

export interface KnowledgeResearchReportDto {
  runId: string;
  title: string;
  summary: string;
  conclusions: KnowledgeResearchReportItemDto[];
  majorFindings: KnowledgeResearchReportItemDto[];
  conflicts: KnowledgeResearchReportItemDto[];
  uncertainties: string[];
  limitations: string[];
  coverage: KnowledgeResearchCoverageDto;
  citations: Array<{ marker: number; evidenceId: string; citationId: string }>;
  createdAt: string;
}

export interface KnowledgeResearchReportResultDto {
  report: KnowledgeResearchReportDto;
  citations: Array<Omit<KnowledgeResolvedCitationDto, 'block'> & {
    marker: number;
    evidenceId: string;
    locator: Record<string, unknown>;
  }>;
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

export async function runKnowledgeQuickAnswer(input: {
  question: string;
  notebookIds: string[];
}): Promise<KnowledgeQueryResultDto> {
  return knowledgeRequest<KnowledgeQueryResultDto>('/api/knowledge/query', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      mode: 'quick',
      question: input.question,
      notebookIds: input.notebookIds,
    }),
  });
}

export async function runKnowledgeResearch(input: {
  question: string;
  notebookIds: string[];
}): Promise<KnowledgeResearchRunResultDto> {
  return knowledgeRequest<KnowledgeResearchRunResultDto>('/api/knowledge/query', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      mode: 'research',
      question: input.question,
      notebookIds: input.notebookIds,
    }),
  });
}

export async function listActiveKnowledgeResearchRuns(): Promise<KnowledgeResearchRunResultDto[]> {
  const data = await knowledgeRequest<{ runs: KnowledgeResearchRunResultDto[] }>('/api/knowledge/runs');
  return data.runs;
}

export async function getKnowledgeResearchRun(runId: string): Promise<KnowledgeResearchRunResultDto> {
  return knowledgeRequest<KnowledgeResearchRunResultDto>(
    `/api/knowledge/runs/${encodeURIComponent(runId)}`,
  );
}

export async function cancelKnowledgeResearch(runId: string): Promise<{
  run: KnowledgeRunDto;
  research: KnowledgeResearchRunDto;
}> {
  return knowledgeRequest(
    `/api/knowledge/runs/${encodeURIComponent(runId)}/cancel`,
    { method: 'POST' },
  );
}

export async function getKnowledgeResearchReport(runId: string): Promise<KnowledgeResearchReportResultDto> {
  return knowledgeRequest<KnowledgeResearchReportResultDto>(
    `/api/knowledge/runs/${encodeURIComponent(runId)}/report`,
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
