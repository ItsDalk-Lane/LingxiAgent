/**
 * 会话地图纯逻辑测试：
 *  - buildMapWorkspaces：projectId / cwd 兜底 / 未分组归组、messageCount>0 过滤、排序；
 *  - applyTurnsToThreads：fork 边界过滤（命中 → 隐藏前缀；未命中 → 全部可见）；
 *  - layoutConversationGraph：车道分配、锚点定位、chain/fork 连线、碰撞下压。
 */
import { describe, expect, it, vi } from 'vitest';

vi.mock('../../../stores', () => ({
  useStore: {
    getState: () => ({
      mapTurnsBySessionId: {},
      setMapTurnsEntry: () => {},
    }),
  },
}));
vi.mock('../../../hooks/use-hana-fetch', () => ({
  lingxiFetch: vi.fn(),
  lingxiUrl: (p: string) => p,
}));

import {
  MAP_PENDING_TURN_MAX_AGE_MS,
  applyTurnsToThreads,
  buildMapWorkspaces,
  shouldClearMapPendingTurn,
  type MapThread,
  type MapWorkspace,
} from '../../../components/conversation-map/conversation-map-projection';
import {
  CARD_GAP_Y,
  CARD_HEIGHT,
  LANE_STEP_X,
  MAP_ORIGIN,
  applyCollapse,
  collectDescendantCardIds,
  firstAvailablePosition,
  layoutConversationGraph,
} from '../../../components/conversation-map/conversation-map-layout';
import type { MapTurn } from '../../../stores/conversation-map-slice';
import type { Session } from '../../../types';

function session(overrides: Partial<Session>): Session {
  return {
    path: `/sessions/${overrides.sessionId ?? Math.random()}.jsonl`,
    title: null,
    firstMessage: '',
    modified: '2026-09-01T00:00:00.000Z',
    messageCount: 1,
    agentId: null,
    agentName: null,
    cwd: null,
    ...overrides,
  };
}

function turn(overrides: Partial<MapTurn>): MapTurn {
  return {
    turnIndex: 0,
    questionEntryId: null,
    question: 'q',
    questionAt: null,
    answerEntryId: null,
    answer: 'a',
    answerAt: null,
    processCount: 0,
    entryIds: [],
    truncated: false,
    ...overrides,
  };
}

function thread(overrides: Partial<MapThread>): MapThread {
  return {
    sessionId: 's',
    sessionPath: '/sessions/s.jsonl',
    title: 'thread',
    agentId: null,
    agentName: null,
    cwd: null,
    modified: '2026-09-01T00:00:00.000Z',
    forkedFrom: null,
    color: '#0f766e',
    turns: [],
    hiddenTurnCount: 0,
    ...overrides,
  };
}

describe('buildMapWorkspaces', () => {
  it('按 projectId 分组并使用目录项目名，threads 按 modified 升序', () => {
    const workspaces = buildMapWorkspaces([
      session({ sessionId: 'a', projectId: 'p1', modified: '2026-09-02T00:00:00Z' }),
      session({ sessionId: 'b', projectId: 'p1', modified: '2026-09-01T00:00:00Z' }),
    ], [{ id: 'p1', name: 'Alpha' }]);
    expect(workspaces).toHaveLength(1);
    expect(workspaces[0].key).toBe('p1');
    expect(workspaces[0].title).toBe('Alpha');
    expect(workspaces[0].threads.map((t) => t.sessionId)).toEqual(['b', 'a']);
  });

  it('无 projectId 时按 cwd 自动分组，标题取 cwd 末段；无 cwd 进未分组', () => {
    const workspaces = buildMapWorkspaces([
      session({ sessionId: 'a', cwd: '/Users/x/projects/demo' }),
      session({ sessionId: 'b' }),
    ], []);
    const byKey = new Map(workspaces.map((ws) => [ws.key, ws]));
    const cwdWs = workspaces.find((ws) => ws.key.startsWith('cwd:'));
    expect(cwdWs?.title).toBe('demo');
    expect(byKey.get('map-ungrouped')?.title).toBe('Ungrouped');
    expect(byKey.get('map-ungrouped')?.threads).toHaveLength(1);
  });

  it('未分组标题可走 i18n', () => {
    const workspaces = buildMapWorkspaces(
      [session({ sessionId: 'a' })],
      [],
      (key) => (key === 'map.ungrouped' ? '未分组' : key),
    );
    expect(workspaces[0].title).toBe('未分组');
  });

  it('过滤 messageCount<=0 的会话，工作区按最近 modified 降序', () => {
    const workspaces = buildMapWorkspaces([
      session({ sessionId: 'empty', projectId: 'p1', messageCount: 0 }),
      session({ sessionId: 'old', projectId: 'p2', modified: '2026-09-01T00:00:00Z' }),
      session({ sessionId: 'new', projectId: 'p1', modified: '2026-09-03T00:00:00Z' }),
    ], [{ id: 'p1', name: 'One' }, { id: 'p2', name: 'Two' }]);
    expect(workspaces.map((ws) => ws.key)).toEqual(['p1', 'p2']);
    expect(workspaces[0].threads.map((t) => t.sessionId)).toEqual(['new']);
    expect(workspaces[1].threads.map((t) => t.sessionId)).toEqual(['old']);
  });
});

describe('applyTurnsToThreads', () => {
  const turns = [
    turn({ turnIndex: 0, entryIds: ['e0'] }),
    turn({ turnIndex: 1, entryIds: ['e1', 'e1b'] }),
    turn({ turnIndex: 2, entryIds: ['e2'] }),
  ];
  const baseWorkspace: MapWorkspace = {
    key: 'k',
    title: 'k',
    threads: [thread({ sessionId: 's' })],
  };

  it('非 fork 会话显示全部轮次', () => {
    const [ws] = applyTurnsToThreads([baseWorkspace], {
      s: { revision: 'r', turns, loading: false, error: null },
    });
    expect(ws.threads[0].turns).toHaveLength(3);
    expect(ws.threads[0].hiddenTurnCount).toBe(0);
  });

  it('fork 会话隐藏分叉边界（含）之前的轮次', () => {
    const forked: MapWorkspace = {
      key: 'k',
      title: 'k',
      threads: [thread({ sessionId: 's', forkedFrom: { sessionId: 'p', entryId: 'e1b' } })],
    };
    const [ws] = applyTurnsToThreads([forked], {
      s: { revision: 'r', turns, loading: false, error: null },
    });
    expect(ws.threads[0].turns.map((t) => t.turnIndex)).toEqual([2]);
    expect(ws.threads[0].hiddenTurnCount).toBe(2);
  });

  it('边界 entryId 未命中时显示全部轮次', () => {
    const forked: MapWorkspace = {
      key: 'k',
      title: 'k',
      threads: [thread({ sessionId: 's', forkedFrom: { sessionId: 'p', entryId: 'missing' } })],
    };
    const [ws] = applyTurnsToThreads([forked], {
      s: { revision: 'r', turns, loading: false, error: null },
    });
    expect(ws.threads[0].turns).toHaveLength(3);
    expect(ws.threads[0].hiddenTurnCount).toBe(0);
  });

  it('未加载完成的会话保持空 turns', () => {
    const [ws] = applyTurnsToThreads([baseWorkspace], {
      s: { revision: null, turns, loading: true, error: null },
    });
    expect(ws.threads[0].turns).toEqual([]);
  });
});

describe('layoutConversationGraph', () => {
  const rootTurns = [
    turn({ turnIndex: 0, questionEntryId: 'q0', entryIds: ['e0', 'q0'] }),
    turn({ turnIndex: 1, questionEntryId: 'q1', entryIds: ['e1', 'q1'] }),
  ];
  const childTurns = [
    turn({ turnIndex: 0, questionEntryId: 'cq0', entryIds: ['cq0'] }),
  ];

  it('root 与 fork 子会话分占 lane 0/1，子首卡锚定父命中卡片右侧', () => {
    const { cards, edges } = layoutConversationGraph([
      thread({ sessionId: 'r', turns: rootTurns }),
      thread({ sessionId: 'c', turns: childTurns, forkedFrom: { sessionId: 'r', entryId: 'q0' } }),
    ]);

    const r0 = cards.find((c) => c.id === 'r:q0')!;
    const r1 = cards.find((c) => c.id === 'r:q1')!;
    const c0 = cards.find((c) => c.id === 'c:cq0')!;
    expect(r0.x).toBe(MAP_ORIGIN.x);
    expect(r0.y).toBe(MAP_ORIGIN.y);
    expect(r1.x).toBe(MAP_ORIGIN.x + LANE_STEP_X);
    expect(r1.y).toBe(MAP_ORIGIN.y);
    expect(c0.x).toBe(r0.x + LANE_STEP_X);
    expect(c0.y).toBe(MAP_ORIGIN.y + (CARD_HEIGHT + CARD_GAP_Y));
    expect(c0.isThreadStart).toBe(true);

    const chainEdges = edges.filter((e) => e.kind === 'chain');
    const forkEdges = edges.filter((e) => e.kind === 'fork');
    expect(chainEdges).toHaveLength(1);
    expect(chainEdges[0]).toMatchObject({ from: 'r:q0', to: 'r:q1' });
    expect(forkEdges).toHaveLength(1);
    expect(forkEdges[0]).toMatchObject({ from: 'r:q0', to: 'c:cq0' });
  });

  it('锚点 entryId 未命中父卡片时退回父最后一张卡片', () => {
    const { cards, edges } = layoutConversationGraph([
      thread({ sessionId: 'r', turns: rootTurns }),
      thread({ sessionId: 'c', turns: childTurns, forkedFrom: { sessionId: 'r', entryId: 'missing' } }),
    ]);
    const r1 = cards.find((c) => c.id === 'r:q1')!;
    const c0 = cards.find((c) => c.id === 'c:cq0')!;
    expect(c0.x).toBe(r1.x + LANE_STEP_X);
    expect(edges.find((e) => e.kind === 'fork')).toMatchObject({ from: 'r:q1', to: 'c:cq0' });
  });

  it('零可见轮次的 thread 生成占位卡片；父会话不在工作区时按 root 处理', () => {
    const { cards, edges } = layoutConversationGraph([
      thread({ sessionId: 'a', turns: [] }),
      thread({ sessionId: 'b', turns: childTurns, forkedFrom: { sessionId: 'elsewhere', entryId: 'x' } }),
    ]);
    const placeholder = cards.find((c) => c.id === 'a:empty')!;
    expect(placeholder.turnIndex).toBe(-1);
    expect(placeholder.isThreadStart).toBe(true);
    expect(placeholder.question).toBe('thread');
    const b0 = cards.find((c) => c.id === 'b:cq0')!;
    expect(b0.x).toBe(MAP_ORIGIN.x);
    expect(edges.filter((e) => e.kind === 'fork')).toHaveLength(0);
  });

  it('重叠的候选卡片被下压到碰撞卡片之下', () => {
    // 直接验证布局产出的不变量：任意两张卡片不重叠；
    // 碰撞分支通过 lane 间距（恰好等于重叠阈值）与锚点列偏移共同保证。
    const manyTurns = Array.from({ length: 6 }, (_, i) =>
      turn({ turnIndex: i, questionEntryId: `q${i}`, entryIds: [`e${i}`] }));
    const { cards } = layoutConversationGraph([
      thread({ sessionId: 'r1', turns: manyTurns }),
      thread({ sessionId: 'r2', turns: manyTurns }),
      thread({ sessionId: 'c', turns: childTurns, forkedFrom: { sessionId: 'r1', entryId: 'e5' } }),
    ]);
    for (let i = 0; i < cards.length; i++) {
      for (let j = i + 1; j < cards.length; j++) {
        const overlap = Math.abs(cards[i].x - cards[j].x) < 320
          && Math.abs(cards[i].y - cards[j].y) < CARD_HEIGHT + CARD_GAP_Y;
        expect(overlap).toBe(false);
      }
    }
    // 锚定父会话最后一张卡片（e5 → q5）右侧。
    const q5 = cards.find((c) => c.id === 'r1:q5')!;
    const c0 = cards.find((c) => c.id === 'c:cq0')!;
    expect(c0.x).toBe(q5.x + LANE_STEP_X);
    expect(c0.y).toBeGreaterThanOrEqual(MAP_ORIGIN.y);
  });

  it('锁定坐标（拖拽持久化）的卡片使用存储坐标且仍作为碰撞障碍物', () => {
    const threads = [
      thread({ sessionId: 'r', turns: rootTurns }),
      thread({ sessionId: 'c', turns: childTurns, forkedFrom: { sessionId: 'r', entryId: 'q0' } }),
    ];
    const lockedPos = { x: 4000, y: 3000 };
    const { cards } = layoutConversationGraph(threads, { 'r:q1': lockedPos });
    const r1 = cards.find((c) => c.id === 'r:q1')!;
    expect(r1.positionLocked).toBe(true);
    expect(r1.x).toBe(lockedPos.x);
    expect(r1.y).toBe(lockedPos.y);
    // 未锁定的卡片保持自然定位
    const r0 = cards.find((c) => c.id === 'r:q0')!;
    expect(r0.positionLocked).toBe(false);
    expect(r0.x).toBe(MAP_ORIGIN.x);
    expect(r0.y).toBe(MAP_ORIGIN.y);
    // 锁定卡参与碰撞：r:q1 被拖到子卡的自然落位（锚点右侧一列、lane 1），
    // 子卡 c0 应被压到它下面。
    const lane1Y = MAP_ORIGIN.y + (CARD_HEIGHT + CARD_GAP_Y);
    const { cards: collided } = layoutConversationGraph(threads, {
      'r:q1': { x: MAP_ORIGIN.x + LANE_STEP_X, y: lane1Y },
    });
    const c0 = collided.find((c) => c.id === 'c:cq0')!;
    expect(c0.x).toBe(MAP_ORIGIN.x + LANE_STEP_X);
    expect(c0.y).toBe(lane1Y + CARD_HEIGHT + CARD_GAP_Y);
  });
});

describe('collectDescendantCardIds', () => {
  const forkedThreads = [
    thread({
      sessionId: 'r',
      turns: [
        turn({ turnIndex: 0, questionEntryId: 'q0', entryIds: ['q0'] }),
        turn({ turnIndex: 1, questionEntryId: 'q1', entryIds: ['q1'] }),
        turn({ turnIndex: 2, questionEntryId: 'q2', entryIds: ['q2'] }),
      ],
    }),
    thread({
      sessionId: 'c',
      turns: [turn({ turnIndex: 0, questionEntryId: 'cq0', entryIds: ['cq0'] })],
      forkedFrom: { sessionId: 'r', entryId: 'q1' },
    }),
    thread({
      sessionId: 'g',
      turns: [turn({ turnIndex: 0, questionEntryId: 'gq0', entryIds: ['gq0'] })],
      forkedFrom: { sessionId: 'c', entryId: 'cq0' },
    }),
  ];

  it('链式后代 + 锚定在后代上的 fork 子树都被收集', () => {
    const { cards, edges } = layoutConversationGraph(forkedThreads);
    const descendants = collectDescendantCardIds('r:q1', cards, edges);
    expect([...descendants].sort()).toEqual(['c:cq0', 'g:gq0', 'r:q2']);
    // 锚点本身与之前的卡片不在后代集合中
    expect(descendants.has('r:q1')).toBe(false);
    expect(descendants.has('r:q0')).toBe(false);
  });

  it('末位卡片没有后代', () => {
    const { cards, edges } = layoutConversationGraph(forkedThreads);
    expect(collectDescendantCardIds('g:gq0', cards, edges).size).toBe(0);
  });

  it('环安全：手工构造的环不会死循环', () => {
    const { cards } = layoutConversationGraph([
      thread({
        sessionId: 'r',
        turns: [
          turn({ turnIndex: 0, questionEntryId: 'q0', entryIds: ['q0'] }),
          turn({ turnIndex: 1, questionEntryId: 'q1', entryIds: ['q1'] }),
        ],
      }),
    ]);
    const cyclicEdges = [
      { id: 'a', from: 'r:q0', to: 'r:q1', kind: 'chain' as const },
      { id: 'b', from: 'r:q1', to: 'r:q0', kind: 'chain' as const },
    ];
    const descendants = collectDescendantCardIds('r:q0', cards, cyclicEdges);
    expect(descendants.has('r:q1')).toBe(true);
    expect(descendants.has('r:q0')).toBe(false);
  });
});

describe('applyCollapse', () => {
  const forkedThreads = [
    thread({
      sessionId: 'r',
      turns: [
        turn({ turnIndex: 0, questionEntryId: 'q0', entryIds: ['q0'] }),
        turn({ turnIndex: 1, questionEntryId: 'q1', entryIds: ['q1'] }),
        turn({ turnIndex: 2, questionEntryId: 'q2', entryIds: ['q2'] }),
      ],
    }),
    thread({
      sessionId: 'c',
      turns: [turn({ turnIndex: 0, questionEntryId: 'cq0', entryIds: ['cq0'] })],
      forkedFrom: { sessionId: 'r', entryId: 'q1' },
    }),
  ];

  it('折叠卡片的后代被隐藏，携带隐藏计数，相关连线被丢弃', () => {
    const { cards, edges } = layoutConversationGraph(forkedThreads);
    const { visibleCards, visibleEdges, hiddenCountByCardId } =
      applyCollapse(cards, edges, ['r:q1']);
    const visibleIds = visibleCards.map((c) => c.id).sort();
    expect(visibleIds).toEqual(['r:q0', 'r:q1']);
    expect(hiddenCountByCardId).toEqual({ 'r:q1': 2 });
    // r:q1→r:q2（chain）与 r:q1→c:cq0（fork）都被丢弃
    expect(visibleEdges).toHaveLength(1);
    expect(visibleEdges[0]).toMatchObject({ from: 'r:q0', to: 'r:q1', kind: 'chain' });
  });

  it('空折叠集与原样返回；未知 id 被忽略', () => {
    const { cards, edges } = layoutConversationGraph(forkedThreads);
    const unchanged = applyCollapse(cards, edges, []);
    expect(unchanged.visibleCards).toBe(cards);
    expect(unchanged.visibleEdges).toBe(edges);
    const unknown = applyCollapse(cards, edges, ['nope']);
    expect(unknown.visibleCards).toHaveLength(cards.length);
    expect(unknown.hiddenCountByCardId).toEqual({});
  });

  it('嵌套折叠：外层隐藏内层时只保留可见折叠卡的计数', () => {
    const { cards, edges } = layoutConversationGraph(forkedThreads);
    const { visibleCards, hiddenCountByCardId } = applyCollapse(cards, edges, ['r:q1', 'r:q2']);
    expect(visibleCards.map((c) => c.id).sort()).toEqual(['r:q0', 'r:q1']);
    expect(Object.keys(hiddenCountByCardId)).toEqual(['r:q1']);
    expect(hiddenCountByCardId['r:q1']).toBe(2);
  });
});

describe('shouldClearMapPendingTurn', () => {
  const pending = { question: '  继续展开讲讲  ', startedAt: Date.now() };

  it('问题尚未落盘时保留待回复卡片', () => {
    expect(shouldClearMapPendingTurn([turn({ question: '别的问题' })], pending, true)).toBe(false);
    expect(shouldClearMapPendingTurn([], pending, false)).toBe(false);
  });

  it('已落盘但答复为空且仍在流式输出时保留（等待流式文本贴入）', () => {
    const turns = [turn({ question: '继续展开讲讲', answer: '' })];
    expect(shouldClearMapPendingTurn(turns, pending, true)).toBe(false);
  });

  it('已落盘且答复非空时清除，交接给正式卡片', () => {
    const turns = [turn({ question: '继续展开讲讲', answer: '答复内容' })];
    expect(shouldClearMapPendingTurn(turns, pending, true)).toBe(true);
    expect(shouldClearMapPendingTurn(turns, pending, false)).toBe(true);
  });

  it('已落盘、答复为空且会话已停止流式时清除（失败/空答复兜底）', () => {
    const turns = [turn({ question: '继续展开讲讲', answer: '' })];
    expect(shouldClearMapPendingTurn(turns, pending, false)).toBe(true);
  });

  it('超过安全网寿命的条目一律清除', () => {
    const stale = { question: 'q', startedAt: Date.now() - MAP_PENDING_TURN_MAX_AGE_MS - 1 };
    expect(shouldClearMapPendingTurn([], stale, true)).toBe(true);
  });
});

describe('firstAvailablePosition', () => {
  it('与已有卡片碰撞时持续下压到空位', () => {
    const { cards } = layoutConversationGraph([
      thread({
        sessionId: 'r',
        turns: [turn({ turnIndex: 0, questionEntryId: 'q0', entryIds: ['q0'] })],
      }),
    ]);
    const anchor = cards[0];
    const candidate = { x: anchor.x, y: anchor.y };
    const first = firstAvailablePosition(cards, candidate);
    expect(first.x).toBe(candidate.x);
    expect(first.y).toBe(anchor.y + CARD_HEIGHT + CARD_GAP_Y);
    // 占据第一个空位后，再压到再下一格。
    const occupied = [...cards, { ...anchor, id: 'draft-1', y: first.y }];
    const second = firstAvailablePosition(occupied, candidate);
    expect(second.y).toBe(first.y + CARD_HEIGHT + CARD_GAP_Y);
    // 横向错开一格后不再碰撞。
    const aside = firstAvailablePosition(cards, { x: anchor.x + 320, y: anchor.y });
    expect(aside.y).toBe(anchor.y);
  });
});
