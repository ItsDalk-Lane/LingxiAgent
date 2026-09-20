/**
 * conversation-map-layout — 纯函数布局算法（移植自 dsh-synapse）
 *
 * 输入一个工作区内的 threads（turns 已按 fork 边界过滤），
 * 输出绝对定位的卡片与连线。车道（lane）分配用迭代 DFS 先序遍历：
 * root0→lane0，其第一个子→lane1，子的子→lane2，回溯后第二个子→lane3……
 */

import type { MapThread } from './conversation-map-projection';

export const CARD_WIDTH = 320;
export const CARD_HEIGHT = 276;
export const CARD_GAP_Y = 42;
export const LANE_STEP_X = 365;
export const MAP_ORIGIN = { x: 86, y: 82 };

export interface MapCard {
  id: string;
  threadId: string;
  sessionId: string;
  sessionPath: string;
  agentId: string | null;
  turnIndex: number;
  question: string;
  answer: string;
  questionEntryId: string | null;
  answerEntryId: string | null;
  error: string | null;
  processCount: number;
  truncated: boolean;
  color: string;
  isThreadStart: boolean;
  /** 用户拖拽过的卡片：采用持久化坐标，不参与自然定位。 */
  positionLocked: boolean;
  x: number;
  y: number;
}

export interface MapEdge {
  id: string;
  from: string;
  to: string;
  kind: 'chain' | 'fork';
}

interface PlacedRect {
  x: number;
  y: number;
}

function overlaps(a: PlacedRect, b: PlacedRect): boolean {
  return Math.abs(a.x - b.x) < CARD_WIDTH
    && Math.abs(a.y - b.y) < CARD_HEIGHT + CARD_GAP_Y;
}

export function layoutConversationGraph(
  threads: MapThread[],
  positions?: Record<string, { x: number; y: number }>,
): { cards: MapCard[]; edges: MapEdge[] } {
  const cards: MapCard[] = [];
  const edges: MapEdge[] = [];
  if (threads.length === 0) return { cards, edges };

  const threadById = new Map(threads.map((t) => [t.sessionId, t]));

  // ── roots / children ──
  const childrenByParent = new Map<string, MapThread[]>();
  const roots: MapThread[] = [];
  for (const thread of threads) {
    const parentId = thread.forkedFrom?.sessionId;
    if (parentId && threadById.has(parentId)) {
      const list = childrenByParent.get(parentId);
      if (list) list.push(thread);
      else childrenByParent.set(parentId, [thread]);
    } else {
      roots.push(thread);
    }
  }

  // ── 迭代 DFS 先序：车道分配 + 卡片生成顺序 ──
  interface Frame { thread: MapThread; lane: number }
  const order: Frame[] = [];
  let nextLane = 0;
  for (const root of roots) {
    const stack: MapThread[] = [root];
    while (stack.length > 0) {
      const thread = stack.pop()!;
      order.push({ thread, lane: nextLane++ });
      const children = childrenByParent.get(thread.sessionId) ?? [];
      // 保持工作区内 thread 顺序：逆序入栈，弹出时仍是正序。
      for (let i = children.length - 1; i >= 0; i--) stack.push(children[i]);
    }
  }
  // 防御：出现环或孤儿时把未访问到的 thread 也排进来。
  const visited = new Set(order.map((f) => f.thread.sessionId));
  for (const thread of threads) {
    if (!visited.has(thread.sessionId)) order.push({ thread, lane: nextLane++ });
  }

  const placed: PlacedRect[] = [];
  const cardsByThread = new Map<string, MapCard[]>();
  const firstCardByThread = new Map<string, MapCard>();

  const resolveY = (candidate: PlacedRect): number => {
    let y = Math.max(candidate.y, MAP_ORIGIN.y);
    // 与任何已放置卡片重叠时，压到最靠下那张的下面。
    for (;;) {
      let lowest: PlacedRect | null = null;
      for (const rect of placed) {
        if (overlaps({ x: candidate.x, y }, rect)) {
          if (!lowest || rect.y > lowest.y) lowest = rect;
        }
      }
      if (!lowest) return y;
      y = lowest.y + CARD_HEIGHT + CARD_GAP_Y;
    }
  };

  for (const { thread, lane } of order) {
    // fork 锚点：父 thread 中包含 forkedFrom.entryId 的那张卡片（找不到时退父最后一张）。
    let parentAnchor: MapCard | null = null;
    if (thread.forkedFrom) {
      const parentCards = cardsByThread.get(thread.forkedFrom.sessionId);
      if (parentCards && parentCards.length > 0) {
        const parent = threadById.get(thread.forkedFrom.sessionId);
        // entryId 可缺（引用型子对话）：缺时找不到锚点轮，退回挂到主对话末轮。
        const anchorIndex = parent && thread.forkedFrom.entryId
          ? parent.turns.findIndex((turn) =>
            turn.entryIds.includes(thread.forkedFrom!.entryId!))
          : -1;
        parentAnchor = (anchorIndex >= 0 && parentCards[anchorIndex])
          ? parentCards[anchorIndex]
          : parentCards[parentCards.length - 1];
      }
    }

    const threadCards: MapCard[] = [];
    const baseY = MAP_ORIGIN.y + lane * (CARD_HEIGHT + CARD_GAP_Y);

    if (thread.turns.length === 0) {
      const card: MapCard = {
        id: `${thread.sessionId}:empty`,
        threadId: thread.sessionId,
        sessionId: thread.sessionId,
        sessionPath: thread.sessionPath,
        agentId: thread.agentId,
        turnIndex: -1,
        question: thread.title,
        answer: '',
        questionEntryId: null,
        answerEntryId: null,
        error: null,
        processCount: 0,
        truncated: false,
        color: thread.color,
        isThreadStart: true,
        positionLocked: false,
        x: parentAnchor ? parentAnchor.x + LANE_STEP_X : MAP_ORIGIN.x,
        y: 0,
      };
      const locked = positions?.[card.id];
      if (locked) {
        card.x = locked.x;
        card.y = locked.y;
        card.positionLocked = true;
      } else {
        card.y = resolveY({ x: card.x, y: baseY });
      }
      placed.push({ x: card.x, y: card.y });
      threadCards.push(card);
    } else {
      thread.turns.forEach((turn, index) => {
        const prev = threadCards[threadCards.length - 1];
        const card: MapCard = {
          id: `${thread.sessionId}:${turn.questionEntryId ?? `idx-${turn.turnIndex}`}`,
          threadId: thread.sessionId,
          sessionId: thread.sessionId,
          sessionPath: thread.sessionPath,
          agentId: thread.agentId,
          turnIndex: turn.turnIndex,
          question: turn.question,
          answer: turn.answer,
          questionEntryId: turn.questionEntryId,
          answerEntryId: turn.answerEntryId,
          error: turn.error ?? null,
          processCount: turn.processCount,
          truncated: turn.truncated,
          color: thread.color,
          isThreadStart: index === 0,
          positionLocked: false,
          x: prev
            ? prev.x + LANE_STEP_X
            : (parentAnchor ? parentAnchor.x + LANE_STEP_X : MAP_ORIGIN.x),
          y: 0,
        };
        const locked = positions?.[card.id];
        if (locked) {
          card.x = locked.x;
          card.y = locked.y;
          card.positionLocked = true;
        } else {
          card.y = prev ? prev.y : resolveY({ x: card.x, y: baseY });
        }
        placed.push({ x: card.x, y: card.y });
        if (prev) {
          edges.push({ id: `chain:${prev.id}->${card.id}`, from: prev.id, to: card.id, kind: 'chain' });
        }
        threadCards.push(card);
      });
    }

    cards.push(...threadCards);
    cardsByThread.set(thread.sessionId, threadCards);
    firstCardByThread.set(thread.sessionId, threadCards[0]);

    if (parentAnchor) {
      edges.push({
        id: `fork:${parentAnchor.id}->${threadCards[0].id}`,
        from: parentAnchor.id,
        to: threadCards[0].id,
        kind: 'fork',
      });
    }
  }

  return { cards, edges };
}

/**
 * 收集 cardId 的全部后代卡片 id：沿 chain 边覆盖同 thread 的后续卡片，
 * 再沿任何后代上锚定的 fork 边递归到子 thread。visited 集合保证环安全。
 */
export function collectDescendantCardIds(
  cardId: string,
  cards: MapCard[],
  edges: MapEdge[],
): Set<string> {
  const outgoing = new Map<string, string[]>();
  for (const edge of edges) {
    const list = outgoing.get(edge.from);
    if (list) list.push(edge.to);
    else outgoing.set(edge.from, [edge.to]);
  }
  const known = new Set(cards.map((card) => card.id));
  const result = new Set<string>();
  const queue = [cardId];
  const visited = new Set<string>([cardId]);
  while (queue.length > 0) {
    const current = queue.pop()!;
    for (const next of outgoing.get(current) ?? []) {
      if (visited.has(next) || !known.has(next)) continue;
      visited.add(next);
      result.add(next);
      queue.push(next);
    }
  }
  return result;
}

export interface CollapseResult {
  visibleCards: MapCard[];
  visibleEdges: MapEdge[];
  /** 每个折叠卡片 → 被它隐藏的可见后代数（仅仍可见的折叠卡片有条目）。 */
  hiddenCountByCardId: Record<string, number>;
}

/**
 * 应用折叠态：任何折叠卡片的后代都被隐藏；有一端隐藏的连线被丢弃；
 * 仍可见的折叠卡片携带其隐藏后代计数。
 */
export function applyCollapse(
  cards: MapCard[],
  edges: MapEdge[],
  collapsedIds: string[],
): CollapseResult {
  const cardById = new Map(cards.map((card) => [card.id, card]));
  const activeCollapsed = collapsedIds.filter((id) => cardById.has(id));
  if (activeCollapsed.length === 0) {
    return { visibleCards: cards, visibleEdges: edges, hiddenCountByCardId: {} };
  }
  const hidden = new Set<string>();
  const countById: Record<string, number> = {};
  for (const id of activeCollapsed) {
    const descendants = collectDescendantCardIds(id, cards, edges);
    countById[id] = descendants.size;
    for (const descendant of descendants) hidden.add(descendant);
  }
  const visibleCards = cards.filter((card) => !hidden.has(card.id));
  const visibleIds = new Set(visibleCards.map((card) => card.id));
  const visibleEdges = edges.filter((edge) => visibleIds.has(edge.from) && visibleIds.has(edge.to));
  const hiddenCountByCardId: Record<string, number> = {};
  for (const id of activeCollapsed) {
    if (visibleIds.has(id)) hiddenCountByCardId[id] = countById[id];
  }
  return { visibleCards, visibleEdges, hiddenCountByCardId };
}

/**
 * 为草稿等临时卡片找候选位置：与任一可见卡片重叠时持续下压，
 * 语义与布局内的 resolveY 一致。
 */
export function firstAvailablePosition(
  visibleCards: MapCard[],
  candidate: { x: number; y: number },
): { x: number; y: number } {
  let y = Math.max(candidate.y, MAP_ORIGIN.y);
  for (;;) {
    let lowest: MapCard | null = null;
    for (const card of visibleCards) {
      if (overlaps({ x: candidate.x, y }, card)) {
        if (!lowest || card.y > lowest.y) lowest = card;
      }
    }
    if (!lowest) return { x: candidate.x, y };
    y = lowest.y + CARD_HEIGHT + CARD_GAP_Y;
  }
}
