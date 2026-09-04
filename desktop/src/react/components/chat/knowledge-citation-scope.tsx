import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import type { ChatListItem } from '../../stores/chat-types';
import { useStore } from '../../stores';
import { resolveServerConnection } from '../../services/server-connection';
import { resolveKnowledgeCitation, type KnowledgeResolvedCitationDto } from '../knowledge/knowledge-api';

export function knowledgeCitationId(href: string): string | null {
  return /^#knowledge-citation-(cite_[a-zA-Z0-9_-]{1,128})$/.exec(href)?.[1] ?? null;
}

function connectionScope(state: Parameters<typeof resolveServerConnection>[0]): string {
  const connection = resolveServerConnection(state);
  return JSON.stringify(connection ? [connection.connectionId, connection.serverId, connection.studioId,
    connection.userId, connection.authState, connection.baseUrl] : []);
}

/** 缓存只属于当前对话和连接身份；短时复用同一引用，失败后允许再次读取。 */
function createCitationCache() {
  const entries = new Map<string, { promise: Promise<KnowledgeResolvedCitationDto>; expiresAt: number }>();
  return {
    load(id: string, retry = false): Promise<KnowledgeResolvedCitationDto> {
      const cached = entries.get(id);
      if (!retry && cached && cached.expiresAt > Date.now()) return cached.promise;
      if (!/^cite_[a-zA-Z0-9_-]{1,128}$/.test(id)) return Promise.reject(new Error('Invalid citation'));
      if (entries.size >= 64 && !entries.has(id)) entries.delete(entries.keys().next().value!);
      const entry = { promise: Promise.resolve(null as unknown as KnowledgeResolvedCitationDto), expiresAt: Infinity };
      entry.promise = resolveKnowledgeCitation(id).then(value => {
        entry.expiresAt = Date.now() + 30_000;
        return value;
      }, error => {
        entry.expiresAt = Date.now() + 1_000;
        throw error;
      });
      entries.set(id, entry);
      return entry.promise;
    },
  };
}

type CitationCache = ReturnType<typeof createCitationCache>;
interface CitationScope {
  cache: CitationCache;
  numberFor: (messageId: string | undefined, citationId: string) => number;
}
const CitationContext = createContext<CitationScope | null>(null);

/** 同一次提问的正文共享编号；不预扫工具结果、思考或尚未显示的引用。 */
export function KnowledgeCitationProvider({ items, sessionPath, children }: {
  items: ChatListItem[]; sessionPath: string; children: ReactNode;
}) {
  const connectionKey = useStore(connectionScope);
  const cache = useMemo(createCitationCache, [sessionPath, connectionKey]);
  const numbers = useMemo(() => new Map<string, Map<string, number>>(), [sessionPath, connectionKey]);
  const answers = useMemo(() => {
    const result = new Map<string, string>();
    let precedingUser: string | null = null;
    let legacyAnswer: string | null = null;
    for (const item of items) {
      if (item.type !== 'message') continue;
      if (item.data.role === 'user') {
        precedingUser = item.data.sourceEntryId || item.data.id;
        legacyAnswer = null;
      } else if (item.data.role === 'assistant') {
        legacyAnswer ??= item.data.turnProjection?.id || item.data.id;
        result.set(item.data.id, item.data.turnInputEntryId?.trim() || precedingUser || legacyAnswer);
      }
    }
    return result;
  }, [items]);
  const numberFor = useCallback((messageId: string | undefined, citationId: string): number => {
    const answer = messageId ? answers.get(messageId) ?? messageId : 'standalone';
    let citations = numbers.get(answer);
    if (!citations) { citations = new Map(); numbers.set(answer, citations); }
    const existing = citations.get(citationId);
    if (existing !== undefined) return existing;
    const ordinal = citations.size + 1;
    citations.set(citationId, ordinal);
    return ordinal;
  }, [answers, numbers]);
  const value = useMemo(() => ({ cache, numberFor }), [cache, numberFor]);
  return <CitationContext.Provider value={value}>{children}</CitationContext.Provider>;
}

export function useKnowledgeCitationNumbers(messageId?: string) {
  const shared = useContext(CitationContext);
  const local = useRef(new Map<string, number>());
  return useCallback((citationId: string): number => {
    if (shared && messageId) return shared.numberFor(messageId, citationId);
    const existing = local.current.get(citationId);
    if (existing !== undefined) return existing;
    const number = local.current.size + 1;
    local.current.set(citationId, number);
    return number;
  }, [shared, messageId]);
}

export function useKnowledgeCitationResource(citationId: string) {
  const shared = useContext(CitationContext);
  const connectionKey = useStore(connectionScope);
  const localCache = useMemo(createCitationCache, [connectionKey]);
  const cache = shared?.cache ?? localCache;
  const [resolved, setResolved] = useState<KnowledgeResolvedCitationDto | null>(null);
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    setResolved(null);
    setFailed(false);
    void cache.load(citationId, attempt > 0).then(value => {
      if (active) setResolved(value);
    }).catch(() => { if (active) setFailed(true); });
    return () => { active = false; };
  }, [cache, citationId, attempt]);
  const retry = useCallback(() => setAttempt(value => value + 1), []);
  return { resolved, failed, retry };
}
