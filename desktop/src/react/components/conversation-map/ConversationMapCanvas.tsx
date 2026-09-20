/**
 * ConversationMapCanvas — 可平移/缩放的会话地图画布（阶段 C：交互层）
 *
 * 相机模型：screen = world * zoom + (x, y)，单一 .canvas-content 容器
 * 用 translate + scale 变换。滚轮缩放以光标为锚点；指针拖拽空白处平移。
 * 卡片头拖拽移动卡片（坐标持久化），选区唤起追问按钮，草稿卡片随锚点渲染。
 */

import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from 'react';
import type {
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
  RefObject,
} from 'react';
import { useStore } from '../../stores';
import { switchSession } from '../../stores/session-actions';
import { useI18n } from '../../hooks/use-i18n';
import { persistMapLayout, type MapWorkspace } from './conversation-map-projection';
import {
  CARD_HEIGHT,
  CARD_WIDTH,
  LANE_STEP_X,
  MAP_ORIGIN,
  collectDescendantCardIds,
  firstAvailablePosition,
  type MapCard,
  type MapEdge,
} from './conversation-map-layout';
import { ConversationCard } from './ConversationCard';
import { ConversationMapDraftCard } from './ConversationMapDraftCard';
import { ConversationMapPendingCard } from './ConversationMapPendingCard';
import { noteMapInitiatedSwitch } from './ConversationMapPage';
import styles from './ConversationMap.module.css';

const ZOOM_MIN = 0.6;
const ZOOM_MAX = 4;
/** 拖拽与点击的位移阈值（屏幕像素）：超过则视为拖拽，抑制随后的 click。 */
const DRAG_CLICK_THRESHOLD_PX = 4;
const FOLLOWUP_MIN_LEN = 1;
const FOLLOWUP_MAX_LEN = 4000;

function clampZoom(zoom: number): number {
  return Math.round(Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, zoom)) * 100) / 100;
}

export interface ConversationMapCanvasHandle {
  /** 以视口中心为锚点做增量缩放（delta 为加法步长，如 ±0.1）。 */
  zoomBy: (delta: number) => void;
  /** 把相机对准当前会话的最后一张可见卡片（没有则对准第一张卡片）。 */
  locateCurrent: () => void;
  /** 把相机对准指定卡片。 */
  centerOnCard: (cardId: string) => void;
}

interface Props {
  workspace: MapWorkspace;
  /** 已套用折叠态的可见卡片/连线（由页面层计算，供详情面板复用）。 */
  cards: MapCard[];
  edges: MapEdge[];
  hiddenCountByCardId: Record<string, number>;
  ref: RefObject<ConversationMapCanvasHandle | null>;
}

interface DragState {
  cardId: string;
  dx: number;
  dy: number;
}

interface FollowUpState {
  cardId: string;
  text: string;
  x: number;
  y: number;
}

export function ConversationMapCanvas({ workspace, cards, edges, hiddenCountByCardId, ref }: Props) {
  const { t } = useI18n();
  const currentSessionId = useStore((s) => s.currentSessionId);
  const mapSelectedCardId = useStore((s) => s.mapSelectedCardId);
  const setMapSelectedCard = useStore((s) => s.setMapSelectedCard);
  const mapCollapsedCardIds = useStore((s) => s.mapCollapsedCardIds);
  const setMapCollapsedCardIds = useStore((s) => s.setMapCollapsedCardIds);
  const mergeMapCardPosition = useStore((s) => s.mergeMapCardPosition);
  const mapDraft = useStore((s) => s.mapDraft);
  const setMapDraft = useStore((s) => s.setMapDraft);
  const mapPendingTurns = useStore((s) => s.mapPendingTurns);
  const addToast = useStore((s) => s.addToast);
  const camera = useStore((s) =>
    s.mapCameraByWorkspace[workspace.key] ?? DEFAULT_CAMERA);
  const setMapCamera = useStore((s) => s.setMapCamera);

  const viewportRef = useRef<HTMLDivElement>(null);
  const [panning, setPanning] = useState(false);
  const panState = useRef<{ pointerId: number; startX: number; startY: number; baseX: number; baseY: number } | null>(null);
  const panRaf = useRef(0);
  const pendingPan = useRef<{ x: number; y: number } | null>(null);

  // ── 卡片拖拽 ──
  const [drag, setDrag] = useState<DragState | null>(null);
  const dragSession = useRef<{
    cardId: string;
    pointerId: number;
    startX: number;
    startY: number;
    baseX: number;
    baseY: number;
    moved: boolean;
  } | null>(null);
  const dragRaf = useRef(0);
  const pendingDrag = useRef<DragState | null>(null);
  const suppressClick = useRef(false);

  const [followUp, setFollowUp] = useState<FollowUpState | null>(null);

  const cardById = useMemo(() => {
    const map = new Map<string, MapCard>();
    for (const card of cards) map.set(card.id, card);
    return map;
  }, [cards]);

  // 拖拽中的卡片用临时位移渲染（卡片与连线同步跟随）。
  const displayCards = useMemo(() => {
    if (!drag) return cards;
    return cards.map((card) => (card.id === drag.cardId
      ? { ...card, x: card.x + drag.dx, y: card.y + drag.dy }
      : card));
  }, [cards, drag]);

  const displayCardById = useMemo(() => {
    if (!drag) return cardById;
    const map = new Map(cardById);
    const dragged = cardById.get(drag.cardId);
    if (dragged) map.set(drag.cardId, { ...dragged, x: dragged.x + drag.dx, y: dragged.y + drag.dy });
    return map;
  }, [cardById, drag]);

  const threadTitleById = useMemo(() => {
    const map = new Map<string, string>();
    for (const thread of workspace.threads) map.set(thread.sessionId, thread.title);
    return map;
  }, [workspace.threads]);

  const hasDescendantsById = useMemo(() => {
    const set = new Set<string>();
    for (const edge of edges) set.add(edge.from);
    return set;
  }, [edges]);

  const applyCamera = useCallback((next: { x: number; y: number; zoom: number }) => {
    setMapCamera(workspace.key, next);
  }, [setMapCamera, workspace.key]);

  const zoomAtPoint = useCallback((delta: number, point: { x: number; y: number }) => {
    const cam = useStore.getState().mapCameraByWorkspace[workspace.key] ?? DEFAULT_CAMERA;
    const zoom = clampZoom(cam.zoom + delta);
    if (zoom === cam.zoom) return;
    // 保持光标下的世界坐标不动：world = (point - offset) / zoom
    const worldX = (point.x - cam.x) / cam.zoom;
    const worldY = (point.y - cam.y) / cam.zoom;
    applyCamera({
      x: point.x - worldX * zoom,
      y: point.y - worldY * zoom,
      zoom,
    });
  }, [applyCamera, workspace.key]);

  const centerOnCard = useCallback((cardId: string) => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const target = cardById.get(cardId);
    if (!target) return;
    const cam = useStore.getState().mapCameraByWorkspace[workspace.key] ?? DEFAULT_CAMERA;
    const rect = viewport.getBoundingClientRect();
    applyCamera({
      x: rect.width / 2 - (target.x + CARD_WIDTH / 2) * cam.zoom,
      y: rect.height / 2 - (target.y + CARD_HEIGHT / 2) * cam.zoom,
      zoom: cam.zoom,
    });
  }, [applyCamera, cardById, workspace.key]);

  useImperativeHandle(ref, () => ({
    zoomBy: (delta: number) => {
      const viewport = viewportRef.current;
      if (!viewport) return;
      const rect = viewport.getBoundingClientRect();
      zoomAtPoint(delta, { x: rect.width / 2, y: rect.height / 2 });
    },
    locateCurrent: () => {
      const sessionCards = currentSessionId
        ? cards.filter((c) => c.sessionId === currentSessionId)
        : [];
      const target = sessionCards.length > 0
        ? sessionCards[sessionCards.length - 1]
        : cards[0];
      if (target) centerOnCard(target.id);
    },
    centerOnCard,
  }), [cards, centerOnCard, currentSessionId, ref, zoomAtPoint]);

  // ── 平移 ──

  const flushPan = useCallback(() => {
    panRaf.current = 0;
    const pending = pendingPan.current;
    if (!pending) return;
    pendingPan.current = null;
    const cam = useStore.getState().mapCameraByWorkspace[workspace.key] ?? DEFAULT_CAMERA;
    applyCamera({ x: pending.x, y: pending.y, zoom: cam.zoom });
  }, [applyCamera, workspace.key]);

  const onPointerDown = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest('[data-map-card], [data-map-followup], button, select, a')) return;
    const cam = useStore.getState().mapCameraByWorkspace[workspace.key] ?? DEFAULT_CAMERA;
    panState.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      baseX: cam.x,
      baseY: cam.y,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    setPanning(true);
  }, [workspace.key]);

  const onPointerMove = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const pan = panState.current;
    if (!pan || pan.pointerId !== event.pointerId) return;
    pendingPan.current = {
      x: pan.baseX + (event.clientX - pan.startX),
      y: pan.baseY + (event.clientY - pan.startY),
    };
    if (!panRaf.current) panRaf.current = requestAnimationFrame(flushPan);
  }, [flushPan]);

  const endPan = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    if (panState.current?.pointerId !== event.pointerId) return;
    panState.current = null;
    setPanning(false);
  }, []);

  useEffect(() => () => {
    if (panRaf.current) cancelAnimationFrame(panRaf.current);
    if (dragRaf.current) cancelAnimationFrame(dragRaf.current);
  }, []);

  // ── 滚轮缩放（原生监听：React 根委托的 wheel 是 passive，preventDefault 无效） ──
  // 与 dsh-synapse 一致：滚轮落在卡片上时只做卡片内部原生滚动（overscroll 就地消化），
  // 只有空白处滚轮才以光标为锚点缩放。
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const onWheel = (event: WheelEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest?.('[data-map-card], [data-map-followup], button, select, a, textarea, input')) return;
      event.preventDefault();
      const rect = viewport.getBoundingClientRect();
      zoomAtPoint(event.deltaY < 0 ? 0.05 : -0.05, {
        x: event.clientX - rect.left,
        y: event.clientY - rect.top,
      });
    };
    viewport.addEventListener('wheel', onWheel, { passive: false });
    return () => viewport.removeEventListener('wheel', onWheel);
  }, [zoomAtPoint]);

  // ── 卡片拖拽（指针捕获 + rAF 合并；位移换算到世界坐标） ──

  const flushDrag = useCallback(() => {
    dragRaf.current = 0;
    const pending = pendingDrag.current;
    if (!pending) return;
    pendingDrag.current = null;
    setDrag(pending);
  }, []);

  useEffect(() => () => {
    dragSession.current = null;
  }, []);

  const onHeaderPointerDown = useCallback((card: MapCard, event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    event.stopPropagation();
    event.preventDefault();
    dragSession.current = {
      cardId: card.id,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      baseX: card.x,
      baseY: card.y,
      moved: false,
    };
    const onMove = (moveEvent: PointerEvent) => {
      const session = dragSession.current;
      if (!session || session.pointerId !== moveEvent.pointerId) return;
      const screenDx = moveEvent.clientX - session.startX;
      const screenDy = moveEvent.clientY - session.startY;
      if (!session.moved
        && Math.hypot(screenDx, screenDy) <= DRAG_CLICK_THRESHOLD_PX) return;
      session.moved = true;
      const zoom = (useStore.getState().mapCameraByWorkspace[workspace.key] ?? DEFAULT_CAMERA).zoom;
      pendingDrag.current = {
        cardId: session.cardId,
        dx: screenDx / zoom,
        dy: screenDy / zoom,
      };
      if (!dragRaf.current) dragRaf.current = requestAnimationFrame(flushDrag);
    };
    const onUp = (upEvent: PointerEvent) => {
      if (dragSession.current?.pointerId !== upEvent.pointerId) return;
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onUp);
      // 让最后一帧位移落进 state 后再结算。
      flushDrag();
      // flushDrag 后 drag state 尚未提交到本闭包，直接按 pending 结算。
      const session = dragSession.current;
      if (session?.moved) {
        const zoom = (useStore.getState().mapCameraByWorkspace[workspace.key] ?? DEFAULT_CAMERA).zoom;
        const dx = (upEvent.clientX - session.startX) / zoom;
        const dy = (upEvent.clientY - session.startY) / zoom;
        suppressClick.current = true;
        setTimeout(() => { suppressClick.current = false; }, 0);
        const finalX = session.baseX + dx;
        const finalY = session.baseY + dy;
        mergeMapCardPosition(session.cardId, { x: finalX, y: finalY });
        persistMapLayout({ positions: { [session.cardId]: { x: finalX, y: finalY } } });
      }
      dragSession.current = null;
      pendingDrag.current = null;
      setDrag(null);
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onUp);
  }, [flushDrag, mergeMapCardPosition, workspace.key]);

  // ── 折叠 / 展开 ──

  const onToggleCollapse = useCallback((card: MapCard) => {
    const collapsed = useStore.getState().mapCollapsedCardIds;
    if (collapsed.includes(card.id)) {
      const next = collapsed.filter((id) => id !== card.id);
      setMapCollapsedCardIds(next);
      persistMapLayout({ collapsed: next });
      return;
    }
    const descendants = collectDescendantCardIds(card.id, cards, edges);
    if (currentSessionId) {
      for (const id of descendants) {
        if (cardById.get(id)?.sessionId === currentSessionId) {
          addToast(t('map.collapseBlockedCurrent'), 'error');
          return;
        }
      }
    }
    const next = [...collapsed, card.id];
    setMapCollapsedCardIds(next);
    persistMapLayout({ collapsed: next });
  }, [addToast, cardById, cards, currentSessionId, edges, setMapCollapsedCardIds, t]);

  // ── 选区追问 ──

  const isLastVisibleInThread = useCallback((card: MapCard): boolean => {
    const threadCards = cards.filter((c) => c.threadId === card.threadId);
    const last = threadCards[threadCards.length - 1];
    return last?.id === card.id;
  }, [cards]);

  const openDraftForCard = useCallback((card: MapCard, prefill: string) => {
    setMapDraft({
      kind: isLastVisibleInThread(card) ? 'continue' : 'branch',
      anchorCardId: card.id,
      sessionId: card.sessionId,
      sessionPath: card.sessionPath,
      agentId: card.agentId,
      text: prefill,
      sending: false,
    });
  }, [isLastVisibleInThread, setMapDraft]);

  const onAnswerMouseUp = useCallback((card: MapCard, _event: ReactMouseEvent<HTMLDivElement>) => {
    // 等浏览器完成选区更新后再读取。
    setTimeout(() => {
      const selection = window.getSelection();
      const text = selection?.toString().trim() ?? '';
      if (!selection || selection.rangeCount === 0
        || text.length < FOLLOWUP_MIN_LEN || text.length > FOLLOWUP_MAX_LEN) {
        return;
      }
      const range = selection.getRangeAt(0);
      const container = range.commonAncestorContainer;
      const answerEl = (container instanceof Element ? container : container.parentElement)
        ?.closest('[data-map-answer]');
      if (!answerEl) return;
      const viewport = viewportRef.current;
      if (!viewport) return;
      const rect = range.getBoundingClientRect();
      const viewportRect = viewport.getBoundingClientRect();
      setFollowUp({
        cardId: card.id,
        text,
        x: Math.max(8, rect.left - viewportRect.left),
        y: Math.max(8, rect.top - viewportRect.top - 34),
      });
    }, 0);
  }, []);

  // 选区坍缩或点击他处时隐藏追问按钮。
  useEffect(() => {
    if (!followUp) return;
    const onSelectionChange = () => {
      const selection = window.getSelection();
      if (!selection || selection.isCollapsed) setFollowUp(null);
    };
    const onGlobalPointerDown = (event: PointerEvent) => {
      if ((event.target as HTMLElement).closest('[data-map-followup]')) return;
      setFollowUp(null);
    };
    document.addEventListener('selectionchange', onSelectionChange);
    window.addEventListener('pointerdown', onGlobalPointerDown, true);
    return () => {
      document.removeEventListener('selectionchange', onSelectionChange);
      window.removeEventListener('pointerdown', onGlobalPointerDown, true);
    };
  }, [followUp]);

  // ── 点选卡片 ──

  const onSelectCard = useCallback((card: MapCard) => {
    if (suppressClick.current) return;
    setMapSelectedCard(card.id);
    noteMapInitiatedSwitch(card.sessionId);
    void switchSession(card.sessionPath);
  }, [setMapSelectedCard]);

  // 卡片右缘「+」：末位卡片继续追问，其余创建分支（与选区追问共用草稿入口）。
  const onOpenCardDraft = useCallback((card: MapCard) => {
    openDraftForCard(card, '');
  }, [openDraftForCard]);

  // ── 连线 ──

  const edgePaths = useMemo(() => edges.map((edge) => {
    const from = displayCardById.get(edge.from);
    const to = displayCardById.get(edge.to);
    if (!from || !to) return null;
    const sx = from.x + CARD_WIDTH;
    const sy = from.y + CARD_HEIGHT / 2;
    const tx = to.x;
    const ty = to.y + CARD_HEIGHT / 2;
    const bend = Math.min(110, Math.max(36, Math.abs(tx - sx) * 0.2));
    return {
      id: edge.id,
      kind: edge.kind,
      color: edge.kind === 'fork' ? to.color : undefined,
      d: `M ${sx} ${sy} C ${sx + bend} ${sy}, ${tx - bend} ${ty}, ${tx} ${ty}`,
    };
  }).filter((p): p is NonNullable<typeof p> => p !== null), [edges, displayCardById]);

  // ── 草稿卡片 ──

  const draftAnchor = mapDraft ? cardById.get(mapDraft.anchorCardId) ?? null : null;
  const draftRender = useMemo(() => {
    if (!mapDraft || !draftAnchor) return null;
    const position = firstAvailablePosition(cards, {
      x: draftAnchor.x + LANE_STEP_X,
      y: draftAnchor.y,
    });
    const sx = draftAnchor.x + CARD_WIDTH;
    const sy = draftAnchor.y + CARD_HEIGHT / 2;
    const tx = position.x;
    const ty = position.y + CARD_HEIGHT / 2;
    const bend = Math.min(110, Math.max(36, Math.abs(tx - sx) * 0.2));
    return {
      position,
      connector: `M ${sx} ${sy} C ${sx + bend} ${sy}, ${tx - bend} ${ty}, ${tx} ${ty}`,
    };
  }, [mapDraft, draftAnchor, cards]);

  // 锚点被折叠隐藏时丢弃草稿，避免悬空输入。
  useEffect(() => {
    if (mapDraft && !draftAnchor) setMapDraft(null);
  }, [mapDraft, draftAnchor, setMapDraft]);

  // ── 待回复卡片（已发送未落盘的轮次） ──
  // 只投影当前工作区里已存在 thread 的 pending 条目；branch 新车道在
  // sessions 列表刷新后出现，届时自然补渲染。
  const pendingRenders = useMemo(() => {
    const list: {
      sessionPath: string;
      question: string;
      color: string;
      position: { x: number; y: number };
      connector: string | null;
    }[] = [];
    const occupied: { x: number; y: number }[] = [...displayCards];
    for (const thread of workspace.threads) {
      const pending = mapPendingTurns[thread.sessionPath];
      if (!pending) continue;
      const threadCards = displayCards.filter((c) => c.threadId === thread.sessionId);
      const last = threadCards[threadCards.length - 1];
      const position = firstAvailablePosition(
        occupied as MapCard[],
        last ? { x: last.x + LANE_STEP_X, y: last.y } : MAP_ORIGIN,
      );
      occupied.push(position);
      let connector: string | null = null;
      if (last) {
        const sx = last.x + CARD_WIDTH;
        const sy = last.y + CARD_HEIGHT / 2;
        const tx = position.x;
        const ty = position.y + CARD_HEIGHT / 2;
        const bend = Math.min(110, Math.max(36, Math.abs(tx - sx) * 0.2));
        connector = `M ${sx} ${sy} C ${sx + bend} ${sy}, ${tx - bend} ${ty}, ${tx} ${ty}`;
      }
      list.push({
        sessionPath: thread.sessionPath,
        question: pending.question,
        color: thread.color,
        position,
        connector,
      });
    }
    return list;
  }, [workspace.threads, mapPendingTurns, displayCards]);

  const followUpCard = followUp ? cardById.get(followUp.cardId) ?? null : null;

  return (
    <div
      ref={viewportRef}
      className={panning ? `${styles.viewport} ${styles.viewportPanning}` : styles.viewport}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endPan}
      onPointerCancel={endPan}
    >
      <div
        className={styles.canvasContent}
        style={{ transform: `translate(${camera.x}px, ${camera.y}px) scale(${camera.zoom})` }}
      >
        <svg className={styles.edgeLayer} width={1} height={1}>
          {edgePaths.map((path) => (
            <path
              key={path.id}
              d={path.d}
              className={path.kind === 'fork' ? styles.edgeFork : styles.edgeChain}
              stroke={path.color}
            />
          ))}
          {draftRender && (
            <path
              d={draftRender.connector}
              className={styles.edgeDraft}
              stroke={draftAnchor?.color}
            />
          )}
          {pendingRenders.map((pending) => pending.connector && (
            <path
              key={`pending:${pending.sessionPath}`}
              d={pending.connector}
              className={styles.edgeDraft}
              stroke={pending.color}
            />
          ))}
        </svg>
        {displayCards.map((card) => (
          <ConversationCard
            key={card.id}
            card={card}
            threadTitle={threadTitleById.get(card.threadId) ?? ''}
            isCurrent={currentSessionId !== null && card.sessionId === currentSessionId}
            isSelected={card.id === mapSelectedCardId}
            isCollapsed={mapCollapsedCardIds.includes(card.id)}
            isLastInThread={isLastVisibleInThread(card)}
            hiddenCount={hiddenCountByCardId[card.id] ?? 0}
            hasDescendants={hasDescendantsById.has(card.id)}
            dragging={drag?.cardId === card.id}
            onSelect={onSelectCard}
            onToggleCollapse={onToggleCollapse}
            onOpenDraft={onOpenCardDraft}
            onHeaderPointerDown={onHeaderPointerDown}
            onAnswerMouseUp={onAnswerMouseUp}
          />
        ))}
        {pendingRenders.map((pending) => (
          <ConversationMapPendingCard
            key={`pending:${pending.sessionPath}`}
            sessionPath={pending.sessionPath}
            question={pending.question}
            color={pending.color}
            position={pending.position}
          />
        ))}
        {mapDraft && draftAnchor && draftRender && (
          <ConversationMapDraftCard
            draft={mapDraft}
            anchor={draftAnchor}
            position={draftRender.position}
          />
        )}
      </div>
      {followUp && followUpCard && (
        <button
          type="button"
          className={styles.followUpButton}
          data-map-followup=""
          style={{ left: followUp.x, top: followUp.y }}
          onClick={() => {
            openDraftForCard(followUpCard, `${followUp.text}\n\n`);
            setFollowUp(null);
            window.getSelection()?.removeAllRanges();
          }}
        >
          {t('map.followUp')}
        </button>
      )}
    </div>
  );
}

const DEFAULT_CAMERA = { x: 0, y: 0, zoom: 1 };
