import type { ContentBlock } from './chat-types';

export type LiveAssistantSegmentPhase = 'reasoning' | 'commentary' | 'final_answer' | 'unresolved';

export interface LiveAssistantSegment {
  id: string;
  kind: 'text' | 'reasoning';
  semanticPhase: LiveAssistantSegmentPhase;
  source: string;
  lifecycle: 'streaming' | 'sealed';
}

export interface LiveAssistantMessageSnapshot {
  messageId: string;
  blocks: readonly ContentBlock[];
  segmentsById: Readonly<Record<string, LiveAssistantSegment>>;
  segmentOrder: readonly string[];
  status: 'streaming' | 'sealed';
  revision: number;
}

type Listener = () => void;

const snapshots = new Map<string, LiveAssistantMessageSnapshot>();
const listeners = new Map<string, Set<Listener>>();
let resolveSessionKey: ((sessionPath: string) => string | null | undefined) | null = null;

export function configureLiveTurnSessionKeyResolver(
  resolver: ((sessionPath: string) => string | null | undefined) | null,
): void {
  resolveSessionKey = typeof resolver === 'function' ? resolver : null;
}

function sessionKey(sessionPath: string): string {
  return resolveSessionKey?.(sessionPath) || sessionPath;
}

function snapshotKey(sessionPath: string, messageId: string): string {
  return `${sessionKey(sessionPath)}\u0000${messageId}`;
}

function legacySnapshotKey(sessionPath: string, messageId: string): string {
  return `${sessionPath}\u0000${messageId}`;
}

function notify(key: string): void {
  for (const listener of listeners.get(key) || []) listener();
}

export function publishLiveAssistantMessage(
  sessionPath: string,
  messageId: string,
  blocks: readonly ContentBlock[],
  semanticState: {
    segmentsById?: Readonly<Record<string, LiveAssistantSegment>>;
    segmentOrder?: readonly string[];
    status?: 'streaming' | 'sealed';
  } = {},
): LiveAssistantMessageSnapshot {
  const key = snapshotKey(sessionPath, messageId);
  const legacyKey = legacySnapshotKey(sessionPath, messageId);
  const previous = snapshots.get(key) || snapshots.get(legacyKey) || null;
  const snapshot: LiveAssistantMessageSnapshot = {
    messageId,
    blocks,
    segmentsById: semanticState.segmentsById || previous?.segmentsById || {},
    segmentOrder: semanticState.segmentOrder || previous?.segmentOrder || [],
    status: semanticState.status || previous?.status || 'streaming',
    revision: (previous?.revision || 0) + 1,
  };
  if (legacyKey !== key) snapshots.delete(legacyKey);
  snapshots.set(key, snapshot);
  notify(key);
  if (legacyKey !== key) notify(legacyKey);
  return snapshot;
}

export function readLiveAssistantMessage(
  sessionPath: string,
  messageId: string,
): LiveAssistantMessageSnapshot | null {
  const key = snapshotKey(sessionPath, messageId);
  return snapshots.get(key) || snapshots.get(legacySnapshotKey(sessionPath, messageId)) || null;
}

export function subscribeLiveAssistantMessage(
  sessionPath: string,
  messageId: string,
  listener: Listener,
): () => void {
  const key = snapshotKey(sessionPath, messageId);
  const set = listeners.get(key) || new Set<Listener>();
  set.add(listener);
  listeners.set(key, set);
  return () => {
    set.delete(listener);
    if (set.size === 0) listeners.delete(key);
  };
}

export function clearLiveAssistantMessage(sessionPath: string, messageId: string): void {
  const key = snapshotKey(sessionPath, messageId);
  const legacyKey = legacySnapshotKey(sessionPath, messageId);
  const removedResolved = snapshots.delete(key);
  const removedLegacy = legacyKey !== key && snapshots.delete(legacyKey);
  const changed = removedResolved || removedLegacy;
  if (!changed) return;
  notify(key);
  if (legacyKey !== key) notify(legacyKey);
}

export function clearLiveTurnStore(sessionPath?: string): void {
  if (sessionPath === undefined) {
    const keys = [...snapshots.keys()];
    snapshots.clear();
    for (const key of keys) notify(key);
    return;
  }
  const resolvedPrefix = `${sessionKey(sessionPath)}\u0000`;
  const legacyPrefix = `${sessionPath}\u0000`;
  for (const key of [...snapshots.keys()]) {
    if (!key.startsWith(resolvedPrefix) && !key.startsWith(legacyPrefix)) continue;
    snapshots.delete(key);
    notify(key);
  }
}
