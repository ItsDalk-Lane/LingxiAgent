import type { UpdateDigestHistoryResult } from '../types';
import { usePlatform } from '../hooks/use-platform';

const EMPTY_HISTORY: UpdateDigestHistoryResult = {
  entries: [],
  source: 'none',
  complete: false,
};

export async function loadUpdateDigestHistory(): Promise<UpdateDigestHistoryResult> {
  const result = await usePlatform()?.getUpdateDigestHistory?.();
  if (!result || !Array.isArray(result.entries)) return EMPTY_HISTORY;
  return {
    entries: result.entries.slice(0, 5),
    source: result.source,
    complete: result.complete && result.entries.length >= 5,
  };
}
