import { useEffect, useState } from 'react';
import type { DeferredHistoryContent } from '../stores/chat-types';
import { lingxiFetch } from './use-hana-fetch';

export interface DeferredHistoryContentResult {
  id: string;
  kind: DeferredHistoryContent['kind'];
  content: string;
  mimeType?: string;
}

type LoadState = {
  data: DeferredHistoryContentResult | null;
  loading: boolean;
  error: Error | null;
};

const resolvedContent = new Map<string, DeferredHistoryContentResult>();
const pendingContent = new Map<string, Promise<DeferredHistoryContentResult>>();

export function asDeferredHistoryContent(value: unknown): DeferredHistoryContent | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const candidate = value as Partial<DeferredHistoryContent>;
  return typeof candidate.id === 'string'
    && typeof candidate.kind === 'string'
    && typeof candidate.size === 'number'
    && candidate.available === true
    ? candidate as DeferredHistoryContent
    : null;
}

function cacheKey(sessionPath: string, descriptor: DeferredHistoryContent): string {
  return `${sessionPath}\u0000${descriptor.id}`;
}

async function loadDeferredHistoryContent(
  sessionPath: string,
  descriptor: DeferredHistoryContent,
): Promise<DeferredHistoryContentResult> {
  const key = cacheKey(sessionPath, descriptor);
  const cached = resolvedContent.get(key);
  if (cached) return cached;
  const pending = pendingContent.get(key);
  if (pending) return pending;

  const request = lingxiFetch(
    `/api/sessions/content/${encodeURIComponent(descriptor.id)}?path=${encodeURIComponent(sessionPath)}`,
  ).then(async (response) => {
    const data = await response.json() as DeferredHistoryContentResult;
    if (typeof data?.content !== 'string') throw new Error('Historical content response is invalid');
    resolvedContent.set(key, data);
    return data;
  }).finally(() => {
    pendingContent.delete(key);
  });
  pendingContent.set(key, request);
  return request;
}

export function useDeferredHistoryContent(
  sessionPath: string,
  descriptor: DeferredHistoryContent | null | undefined,
  enabled: boolean,
): LoadState {
  const key = descriptor ? cacheKey(sessionPath, descriptor) : null;
  const [state, setState] = useState<LoadState>(() => ({
    data: key ? resolvedContent.get(key) || null : null,
    loading: false,
    error: null,
  }));

  useEffect(() => {
    if (!descriptor || !enabled) return;
    const cached = resolvedContent.get(cacheKey(sessionPath, descriptor));
    if (cached) {
      setState({ data: cached, loading: false, error: null });
      return;
    }
    let active = true;
    setState((current) => ({ ...current, loading: true, error: null }));
    void loadDeferredHistoryContent(sessionPath, descriptor).then(
      (data) => {
        if (active) setState({ data, loading: false, error: null });
      },
      (error) => {
        if (active) setState({ data: null, loading: false, error: error as Error });
      },
    );
    return () => { active = false; };
  }, [descriptor, enabled, sessionPath]);

  return state;
}

export function clearDeferredHistoryContentCacheForTests(): void {
  resolvedContent.clear();
  pendingContent.clear();
}
