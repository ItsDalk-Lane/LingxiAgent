/** 真实工具回执在实时和历史中共用一张协作确认卡。 */
import type { SuggestionCardBlock } from '../stores/chat-types';

export function conversationSuggestionFromDetails(value: unknown): SuggestionCardBlock | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const detail = value as Record<string, unknown>;
  if (detail.kind !== 'session_send_draft' && detail.kind !== 'session_create_draft') return null;
  if (typeof detail.suggestionId !== 'string' || !detail.suggestionId) return null;
  if (!detail.draft || typeof detail.draft !== 'object' || Array.isArray(detail.draft)) return null;
  const frozen = detail.frozenApply && typeof detail.frozenApply === 'object' && !Array.isArray(detail.frozenApply)
    ? detail.frozenApply as Record<string, unknown> : undefined;
  const target = detail.target && typeof detail.target === 'object' && !Array.isArray(detail.target)
    ? detail.target as NonNullable<SuggestionCardBlock['target']> : undefined;
  return {
    type: 'suggestion_card', kind: detail.kind, suggestionId: detail.suggestionId,
    status: typeof detail.status === 'string' ? detail.status : 'pending', title: 'Session',
    ...(target ? { target } : {}), detail: { ...detail, draft: frozen || detail.draft },
    ...(frozen ? { frozenApply: frozen } : {}),
    ...(detail.result && typeof detail.result === 'object' && !Array.isArray(detail.result) ? { result: detail.result as Record<string, unknown> } : {}),
    ...(typeof detail.resultSessionId === 'string' ? { resultSessionId: detail.resultSessionId } : {}),
    ...(typeof detail.decisionPersisted === 'boolean' ? { decisionPersisted: detail.decisionPersisted } : {}),
  };
}
