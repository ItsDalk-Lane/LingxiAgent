/**
 * Phase 9 API client 测试 — error contract（§十一）要求 status/kind/code/
 * field/matchedCalls/maxCalls 全量保留，绝不压成一句话。
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../../settings/store', () => ({
  useSettingsStore: {
    getState: () => ({ connection: { port: 3000, token: 'tok' } }),
  },
}));

vi.mock('../../../services/server-connection', () => ({
  appendConnectionAuth: () => ({}),
  buildConnectionUrl: (_conn: unknown, path: string) => path,
  requireServerConnection: () => ({ port: 3000 }),
}));

import {
  isObservabilityAbortError,
  isObservabilityErrorKind,
  loadObservabilityHealth,
  ModelObservabilityRequestError,
  probeObservabilityBlob,
  queryObservabilityCalls,
  updateObservabilitySettings,
} from '../../../settings/tabs/observability/model-observability-actions';

function jsonResponse(status: number, body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), { status, headers });
}

const fetchMock = vi.fn<typeof fetch>();

describe('observability client (Phase 9 §十一 error contract)', () => {
  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal('fetch', fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('health/settings/query hit the documented routes with the right verbs', async () => {
    fetchMock
      .mockResolvedValueOnce(jsonResponse(200, { recordingStatus: 'active', query: { queryStatus: 'ready' } }))
      .mockResolvedValueOnce(jsonResponse(200, { desired: {}, effective: {} }))
      .mockResolvedValueOnce(jsonResponse(200, { calls: [], nextCursor: null }));

    await loadObservabilityHealth();
    await updateObservabilitySettings({ enabled: true });
    await queryObservabilityCalls({ filter: { terminalStatus: ['error'] }, limit: 10 });

    expect(fetchMock).toHaveBeenNthCalledWith(1, '/api/model-observability/health',
      expect.objectContaining({ method: 'GET' }));
    expect(fetchMock).toHaveBeenNthCalledWith(2, '/api/model-observability/settings',
      expect.objectContaining({ method: 'PUT' }));
    const third = fetchMock.mock.calls[2];
    expect(third[0]).toBe('/api/model-observability/query/calls');
    expect((third[1] as RequestInit).method).toBe('POST');
    expect(JSON.parse((third[1] as RequestInit).body as string)).toEqual({
      filter: { terminalStatus: ['error'] }, limit: 10,
    });
  });

  it('413 export_limit preserves matchedCalls / maxCalls (§一百一十八)', async () => {
    fetchMock.mockResolvedValue(jsonResponse(413, {
      error: 'export_limit', message: 'too many calls', matchedCalls: 12345, maxCalls: 10000,
    }));
    const error = await queryObservabilityCalls({}).catch((e: unknown) => e) as ModelObservabilityRequestError;
    expect(error).toBeInstanceOf(ModelObservabilityRequestError);
    expect(error.status).toBe(413);
    expect(error.kind).toBe('export_limit');
    expect(error.matchedCalls).toBe(12345);
    expect(error.maxCalls).toBe(10000);
    expect(isObservabilityErrorKind(error, 'export_limit')).toBe(true);
  });

  it('400 invalid_query keeps the offending field', async () => {
    fetchMock.mockResolvedValue(jsonResponse(400, {
      error: 'invalid_query', message: 'bad limit', field: 'limit',
    }));
    const error = await queryObservabilityCalls({}).catch((e: unknown) => e) as ModelObservabilityRequestError;
    expect(error.status).toBe(400);
    expect(error.field).toBe('limit');
  });

  it('403 denial without a code falls back to the kind as code (§十一)', async () => {
    fetchMock.mockResolvedValue(jsonResponse(403, {
      error: 'forbidden', reason: 'remote_session_not_owner',
    }));
    const error = await loadObservabilityHealth().catch((e: unknown) => e) as ModelObservabilityRequestError;
    expect(error.status).toBe(403);
    expect(error.kind).toBe('forbidden');
    expect(error.code).toBe('forbidden');
    expect(error.reason).toBe('remote_session_not_owner');
  });

  it('HTTP 200 with embedded error body still throws', async () => {
    fetchMock.mockResolvedValue(jsonResponse(200, { error: 'query_failed' }));
    const error = await loadObservabilityHealth().catch((e: unknown) => e) as ModelObservabilityRequestError;
    expect(error).toBeInstanceOf(ModelObservabilityRequestError);
  });

  it('abort errors are detectable and re-thrown untouched (§十二: race belongs to caller)', async () => {
    const abortError = Object.assign(new Error('aborted'), { name: 'AbortError' });
    fetchMock.mockRejectedValue(abortError);
    const error = await loadObservabilityHealth().catch((e: unknown) => e);
    expect(error).toBe(abortError);
    expect(isObservabilityAbortError(error)).toBe(true);
    expect(isObservabilityAbortError(new Error('plain'))).toBe(false);
  });

  it('blob probe is HEAD-only and parses content-length defensively', async () => {
    fetchMock.mockResolvedValue(new Response(null, {
      status: 200,
      headers: { 'content-type': 'image/png', 'content-length': '2048' },
    }));
    const probe = await probeObservabilityBlob('mb_test123');
    const call = fetchMock.mock.calls[0];
    expect(call[0]).toBe('/api/model-observability/blobs/mb_test123');
    expect((call[1] as RequestInit).method).toBe('HEAD');
    expect(probe).toEqual({ contentType: 'image/png', contentLength: 2048 });
  });

  it('blob probe with missing/garbage headers yields nulls, not NaN', async () => {
    fetchMock.mockResolvedValue(new Response(null, { status: 200, headers: { 'content-length': 'abc' } }));
    const probe = await probeObservabilityBlob('mb_test123');
    expect(probe).toEqual({ contentType: null, contentLength: null });
  });
});
