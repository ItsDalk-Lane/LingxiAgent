/**
 * conversation-map-send — 地图草稿（继续追问/创建分支）的发送通道
 *
 * 复用 composer-send-coordinator 的会话级租约与队列兜底：
 * 地图只是另一个输入入口，传输与回执仍走既有 ws 链路。
 */

import { useStore } from '../../stores';
import type { ComposerSendBundle } from '../../stores/chat-types';
import type { MapDraft } from '../../stores/conversation-map-slice';
import { forkSessionTurn } from '../../stores/message-turn-actions';
import { switchSession } from '../../stores/session-actions';
import { sessionScopedListIncludes } from '../../stores/session-slice';
import {
  defaultComposerSendFlowDeps,
  sendWithLease,
  tryAcquireSendLease,
} from '../../services/composer-send-coordinator';

type TranslateFn = (key: string, params?: Record<string, string | number>) => string;

interface SessionIdentity {
  sessionId: string;
  sessionPath: string;
  agentId: string | null;
}

function buildPromptBundle(identity: SessionIdentity, text: string): ComposerSendBundle {
  return {
    type: 'prompt',
    sessionRef: { sessionId: identity.sessionId, sessionPath: identity.sessionPath, agentId: identity.agentId },
    text,
    skills: [],
    fileRefs: [],
    sessionRefs: [],
    agentMentions: [],
    inputFiles: [],
    knowledgeRefs: null,
    docContextAttached: false,
    doc: null,
    quotes: [],
    uiContext: null,
  };
}

function enqueueFallback(identity: SessionIdentity, text: string, bundle: ComposerSendBundle): void {
  useStore.getState().enqueueQueuedTurnInput(identity.sessionPath, {
    id: `map-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    sessionPath: identity.sessionPath,
    text,
    createdAt: Date.now(),
    snapshotVersion: 1,
    status: 'ready',
    bundle,
  });
}

/** 向指定会话发一条 prompt；成功/已排队返回 true，明确失败返回 false。 */
async function sendPromptToSession(
  identity: SessionIdentity,
  text: string,
  t: TranslateFn,
): Promise<boolean> {
  const bundle = buildPromptBundle(identity, text);
  const acq = tryAcquireSendLease({
    identity: { kind: 'session', sessionId: identity.sessionId, sessionPath: identity.sessionPath, agentId: identity.agentId },
    bundle,
    queueItemId: null,
    snapshotVersion: 1,
    composerRevisionAtClick: null,
    targetRun: null,
  });
  if (acq.ok) {
    const result = await sendWithLease(acq.leaseId, defaultComposerSendFlowDeps());
    if (result.kind === 'transport_submitted') {
      // 传输已提交：立即挂出待回复卡片，等 turns 落盘后无缝交接。
      // 入队兜底不算已发送，不挂 pending。
      useStore.getState().setMapPendingTurn(identity.sessionPath, {
        question: text,
        startedAt: Date.now(),
      });
      return true;
    }
    // delivery_unknown 无 retryable 字段：投递结果不可证，禁止入队重发。
    if (result.kind !== 'delivery_unknown' && result.retryable) {
      enqueueFallback(identity, text, bundle);
      useStore.getState().addToast(t('map.queued'), 'info');
      return true;
    }
    useStore.getState().addToast(t('map.sendFailed'), 'error');
    return false;
  }
  // 传输互斥/未决输入占用：入队由对话页自动续发。
  enqueueFallback(identity, text, bundle);
  useStore.getState().addToast(t('map.queued'), 'info');
  return true;
}

export interface MapDraftAnchor {
  answerEntryId: string | null;
  questionEntryId: string | null;
}

/**
 * 发送地图草稿。continue 直接发往原会话；branch 先 fork 再发往新会话。
 * 返回 true 表示草稿可关闭（已提交或已入队）。
 */
export async function sendMapDraft(
  draft: MapDraft,
  anchor: MapDraftAnchor,
  t: TranslateFn,
): Promise<boolean> {
  const text = draft.text.trim();
  if (!text) return false;

  if (draft.kind === 'continue') {
    return sendPromptToSession(
      { sessionId: draft.sessionId, sessionPath: draft.sessionPath, agentId: draft.agentId },
      text,
      t,
    );
  }

  // branch：先 fork 出新会话，再向新会话发送。
  const target = anchor.answerEntryId
    ? { role: 'assistant' as const, entryId: anchor.answerEntryId }
    : anchor.questionEntryId
      ? { role: 'user' as const, entryId: anchor.questionEntryId }
      : null;
  if (!target) {
    useStore.getState().addToast(t('map.sendFailed'), 'error');
    return false;
  }
  const state = useStore.getState();
  const streaming = sessionScopedListIncludes(state as never, state.streamingSessions, draft.sessionPath);
  const forked = await forkSessionTurn(draft.sessionPath, target);
  if (!forked) {
    useStore.getState().addToast(t(streaming ? 'map.forkBusy' : 'map.sendFailed'), 'error');
    return false;
  }
  const sent = await sendPromptToSession(
    { sessionId: forked.sessionId, sessionPath: forked.sessionPath, agentId: forked.agentId },
    text,
    t,
  );
  if (sent) void switchSession(forked.sessionPath);
  return sent;
}
