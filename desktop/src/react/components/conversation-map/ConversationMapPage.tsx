/**
 * ConversationMapPage — 会话地图页（阶段 C：交互层）
 *
 * 从 sessions + 项目目录投影出工作区/泳道，按需加载各会话的 turns，
 * 渲染连线卡片画布。布局（卡片坐标/折叠态）经
 * GET/PUT /api/conversation-map/layout 持久化；卡片选中与 currentSessionId
 * 双向同步：地图发起的切换不移动相机，外部切换自动展开并居中。
 */

import { useEffect, useMemo, useRef } from 'react';
import { useStore } from '../../stores';
import { useI18n } from '../../hooks/use-i18n';
import { switchSession } from '../../stores/session-actions';
import { sessionScopedListIncludes } from '../../stores/session-slice';
import {
  applyTurnsToThreads,
  buildMapWorkspaces,
  fetchMapLayout,
  loadTurnsForWorkspace,
  persistMapLayout,
  shouldClearMapPendingTurn,
} from './conversation-map-projection';
import {
  applyCollapse,
  collectDescendantCardIds,
  layoutConversationGraph,
} from './conversation-map-layout';
import {
  ConversationMapCanvas,
  type ConversationMapCanvasHandle,
} from './ConversationMapCanvas';
import { ConversationMapInspector } from './ConversationMapInspector';
import styles from './ConversationMap.module.css';

/** 地图发起的会话切换在 2s 内不触发相机居中和自动展开。 */
const MAP_INITIATED_SWITCH_WINDOW_MS = 2000;

let mapInitiatedSwitch: { sessionId: string; at: number } | null = null;

export function noteMapInitiatedSwitch(sessionId: string): void {
  mapInitiatedSwitch = { sessionId, at: Date.now() };
}

function consumeMapInitiatedSwitch(sessionId: string): boolean {
  const pending = mapInitiatedSwitch;
  if (!pending || pending.sessionId !== sessionId) return false;
  if (Date.now() - pending.at >= MAP_INITIATED_SWITCH_WINDOW_MS) return false;
  mapInitiatedSwitch = null;
  return true;
}

export function ConversationMapPage() {
  const { t } = useI18n();
  const sessions = useStore((s) => s.sessions);
  const projects = useStore((s) => s.sessionProjectCatalog.projects);
  const currentSessionId = useStore((s) => s.currentSessionId);
  const mapActiveWorkspaceKey = useStore((s) => s.mapActiveWorkspaceKey);
  const setMapActiveWorkspace = useStore((s) => s.setMapActiveWorkspace);
  const mapTurnsBySessionId = useStore((s) => s.mapTurnsBySessionId);
  const mapPendingTurns = useStore((s) => s.mapPendingTurns);
  const streamingSessions = useStore((s) => s.streamingSessions);
  const mapCardPositions = useStore((s) => s.mapCardPositions);
  const mapCollapsedCardIds = useStore((s) => s.mapCollapsedCardIds);
  const mapSelectedCardId = useStore((s) => s.mapSelectedCardId);
  const setMapSelectedCard = useStore((s) => s.setMapSelectedCard);
  const mapLayoutLoaded = useStore((s) => s.mapLayoutLoaded);
  const setMapCollapsedCardIds = useStore((s) => s.setMapCollapsedCardIds);
  const setMapCardPositions = useStore((s) => s.setMapCardPositions);
  const clearMapCamera = useStore((s) => s.clearMapCamera);
  const setMapDraft = useStore((s) => s.setMapDraft);
  const canvasRef = useRef<ConversationMapCanvasHandle>(null);

  const workspaces = useMemo(
    () => applyTurnsToThreads(
      buildMapWorkspaces(sessions, projects, t),
      mapTurnsBySessionId,
    ),
    [sessions, projects, mapTurnsBySessionId, t],
  );

  const activeWorkspace = useMemo(() => {
    if (mapActiveWorkspaceKey) {
      const found = workspaces.find((ws) => ws.key === mapActiveWorkspaceKey);
      if (found) return found;
    }
    if (currentSessionId) {
      const current = workspaces.find((ws) =>
        ws.threads.some((thread) => thread.sessionId === currentSessionId));
      if (current) return current;
    }
    return workspaces[0] ?? null;
  }, [workspaces, mapActiveWorkspaceKey, currentSessionId]);

  // 布局只在首次挂载时拉取；mapLayoutLoaded 防止覆盖用户随后的本地改动。
  useEffect(() => {
    if (mapLayoutLoaded) return;
    let cancelled = false;
    fetchMapLayout()
      .then(({ positions, collapsed }) => {
        if (cancelled) return;
        const state = useStore.getState();
        if (state.mapLayoutLoaded) return;
        state.setMapCardPositions(positions);
        state.setMapCollapsedCardIds(collapsed);
        state.setMapLayoutLoaded(true);
      })
      .catch((err) => {
        console.warn('[conversation-map] load layout failed', err);
      });
    return () => { cancelled = true; };
  }, [mapLayoutLoaded]);

  // sessions 驱动刷新：thread.modified 作为 revision，WS 会话列表更新自动重取过期 turns。
  useEffect(() => {
    if (!activeWorkspace) return;
    void loadTurnsForWorkspace(activeWorkspace);
  }, [activeWorkspace]);

  // 待回复卡片交接：问题落盘且（答复非空或已停止流式）→ 清除 pending 条目；
  // 超时条目由 shouldClearMapPendingTurn 兜底清除。
  useEffect(() => {
    const entries = Object.entries(mapPendingTurns);
    if (entries.length === 0) return;
    const state = useStore.getState();
    for (const [sessionPath, pending] of entries) {
      const thread = activeWorkspace?.threads.find((t) => t.sessionPath === sessionPath);
      // thread 尚未投影（branch 新车道未出现）：turns 按空处理，等 sessions 刷新后再裁决。
      const turns = thread ? (mapTurnsBySessionId[thread.sessionId]?.turns ?? []) : [];
      const isStreaming = sessionScopedListIncludes(state, state.streamingSessions, sessionPath);
      if (shouldClearMapPendingTurn(turns, pending, isStreaming)) {
        state.clearMapPendingTurn(sessionPath);
      }
    }
  }, [mapPendingTurns, mapTurnsBySessionId, streamingSessions, activeWorkspace]);

  const { cards, edges } = useMemo(
    () => (activeWorkspace
      ? layoutConversationGraph(activeWorkspace.threads, mapCardPositions)
      : { cards: [], edges: [] }),
    [activeWorkspace, mapCardPositions],
  );

  const { visibleCards, visibleEdges, hiddenCountByCardId } = useMemo(
    () => applyCollapse(cards, edges, mapCollapsedCardIds),
    [cards, edges, mapCollapsedCardIds],
  );

  const cardById = useMemo(() => {
    const map = new Map(cards.map((card) => [card.id, card]));
    return map;
  }, [cards]);

  // ── currentSessionId → 地图同步（外部切换：自动展开 + 居中） ──
  const prevSessionRef = useRef<string | null>(null);
  const pendingCenterRef = useRef(false);
  const revealAttemptedRef = useRef(false);

  useEffect(() => {
    if (!currentSessionId) {
      prevSessionRef.current = null;
      pendingCenterRef.current = false;
      return;
    }
    if (prevSessionRef.current !== currentSessionId) {
      prevSessionRef.current = currentSessionId;
      pendingCenterRef.current = !consumeMapInitiatedSwitch(currentSessionId);
      revealAttemptedRef.current = false;
    }
    if (!pendingCenterRef.current) return;
    // 自动展开：当前会话被折叠子树隐藏时，解除挡住它的折叠。
    if (!revealAttemptedRef.current) {
      revealAttemptedRef.current = true;
      if (mapCollapsedCardIds.length > 0) {
        const blocking = mapCollapsedCardIds.filter((id) => {
          if (!cardById.has(id)) return false;
          for (const descendant of collectDescendantCardIds(id, cards, edges)) {
            if (cardById.get(descendant)?.sessionId === currentSessionId) return true;
          }
          return false;
        });
        if (blocking.length > 0) {
          const next = mapCollapsedCardIds.filter((id) => !blocking.includes(id));
          setMapCollapsedCardIds(next);
          persistMapLayout({ collapsed: next });
          return; // 折叠态变化后重新进入本 effect 再居中
        }
      }
    }
    const sessionCards = visibleCards.filter((card) => card.sessionId === currentSessionId);
    const target = sessionCards[sessionCards.length - 1];
    // turns 尚未加载完成时保留居中意图，等下一轮 cards 变化。
    if (!target) return;
    pendingCenterRef.current = false;
    canvasRef.current?.centerOnCard(target.id);
  }, [currentSessionId, cards, edges, visibleCards, cardById, mapCollapsedCardIds, setMapCollapsedCardIds]);

  // ── Esc：先关草稿，再关详情面板 ──
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      const state = useStore.getState();
      if (state.mapDraft) {
        state.setMapDraft(null);
        return;
      }
      if (state.mapSelectedCardId) state.setMapSelectedCard(null);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  const isLoading = useMemo(() => activeWorkspace
    ? activeWorkspace.threads.some((thread) => mapTurnsBySessionId[thread.sessionId]?.loading)
    : false, [activeWorkspace, mapTurnsBySessionId]);

  const loadFailed = useMemo(() => activeWorkspace
    ? activeWorkspace.threads.some((thread) => mapTurnsBySessionId[thread.sessionId]?.error)
    : false, [activeWorkspace, mapTurnsBySessionId]);

  const stats = useMemo(() => {
    if (!activeWorkspace) return null;
    const threads = activeWorkspace.threads.length;
    const cardCount = activeWorkspace.threads.reduce(
      (sum, thread) => sum + Math.max(thread.turns.length, 1),
      0,
    );
    return t('map.stats', { threads, cards: cardCount });
  }, [activeWorkspace, t]);

  // 工作区切换 → 激活该工作区最近修改的会话（地图 → 原生同步）。
  const onWorkspaceChange = (key: string) => {
    setMapActiveWorkspace(key || null);
    const workspace = workspaces.find((ws) => ws.key === key);
    if (!workspace) return;
    const latest = workspace.threads.reduce<(typeof workspace.threads)[number] | null>(
      (best, thread) => (!best || (thread.modified ?? '') > (best.modified ?? '') ? thread : best),
      null,
    );
    if (latest) {
      noteMapInitiatedSwitch(latest.sessionId);
      void switchSession(latest.sessionPath);
    }
  };

  const onResetLayout = () => {
    if (!activeWorkspace) return;
    if (!window.confirm(t('map.resetLayoutConfirm'))) return;
    setMapCardPositions({});
    clearMapCamera(activeWorkspace.key);
    persistMapLayout({ positions: {}, replacePositions: true });
  };

  const selectedCard = mapSelectedCardId
    ? visibleCards.find((card) => card.id === mapSelectedCardId) ?? null
    : null;

  const selectedThreadTitle = selectedCard
    ? activeWorkspace?.threads.find((thread) => thread.sessionId === selectedCard.threadId)?.title ?? ''
    : '';

  const selectedIsLastInThread = useMemo(() => {
    if (!selectedCard) return false;
    const threadCards = visibleCards.filter((card) => card.threadId === selectedCard.threadId);
    return threadCards[threadCards.length - 1]?.id === selectedCard.id;
  }, [selectedCard, visibleCards]);

  return (
    <div className={styles.page}>
      <div className={styles.toolbar}>
        <span className={styles.workspaceLabel}>{t('map.workspace')}</span>
        <select
          className={styles.workspaceSelect}
          value={activeWorkspace?.key ?? ''}
          onChange={(event) => onWorkspaceChange(event.target.value)}
        >
          {workspaces.map((ws) => (
            <option key={ws.key} value={ws.key}>{ws.title}</option>
          ))}
        </select>
        <button
          type="button"
          className={styles.toolbarButton}
          title={t('map.zoomOut')}
          onClick={() => canvasRef.current?.zoomBy(-0.1)}
        >
          −
        </button>
        <button
          type="button"
          className={styles.toolbarButton}
          title={t('map.zoomIn')}
          onClick={() => canvasRef.current?.zoomBy(0.1)}
        >
          +
        </button>
        <button
          type="button"
          className={styles.toolbarButton}
          onClick={() => canvasRef.current?.locateCurrent()}
        >
          {t('map.locate')}
        </button>
        <button
          type="button"
          className={styles.toolbarButton}
          onClick={onResetLayout}
        >
          {t('map.resetLayout')}
        </button>
        {stats && <span className={styles.stats}>{stats}</span>}
      </div>
      {activeWorkspace && activeWorkspace.threads.length > 0 ? (
        <div className={styles.mapArea}>
          <ConversationMapCanvas
            ref={canvasRef}
            workspace={activeWorkspace}
            cards={visibleCards}
            edges={visibleEdges}
            hiddenCountByCardId={hiddenCountByCardId}
          />
          {(isLoading || loadFailed) && (
            <div className={styles.loadingHint}>
              {loadFailed && !isLoading ? t('map.turnsLoadFailed') : t('map.loading')}
            </div>
          )}
          {selectedCard && (
            <ConversationMapInspector
              card={selectedCard}
              threadTitle={selectedThreadTitle}
              isLastInThread={selectedIsLastInThread}
              onClose={() => setMapSelectedCard(null)}
            />
          )}
        </div>
      ) : (
        <div className={styles.emptyState}>{t('map.empty')}</div>
      )}
    </div>
  );
}
