/**
 * TraceConversationModel 测试 — 观测 trace → dsh 形状轨迹布局的数据装配。
 *
 * 覆盖：calls-only 降级 / 会话 join（轮次重建、助手↔主链调用配对、轮内
 * 侧线 vs Between turns 划分）/ 时间窗锚定 / 缺时间戳降级 / sessionId
 * 多数票 / 请求编号与 usage 累计 / 移植纯函数核心断言（timeline 泳道与
 * 区间聚焦、虚拟行分组）。
 */
import { describe, expect, it } from 'vitest';
import type {
  ModelObservabilityCallListItem,
  ModelObservabilityTraceDetail,
} from '../../../../../../shared/model-observability-api-contract.ts';
import {
  buildTraceConversationModel,
  parseSessionMessages,
  resolveTraceSessionId,
} from '../../../settings/tabs/observability/trace-detail/trace-conversation-model';
import { deriveTrajectoryTimeline, trajectoryTimelineFocusIndexes } from '../../../settings/tabs/observability/trace-detail/timeline';
import { groupTrajectoryVirtualRows } from '../../../settings/tabs/observability/trace-detail/trajectory-virtual-rows';
import type { TrajectoryCellProps } from '../../../settings/tabs/observability/trace-detail/trajectory-record';

const T0 = Date.UTC(2026, 8, 1, 10, 0, 0);
const TOOL_END_NOOP = () => T0 + 1_200;

const BASE_ATTRIBUTION = {
  kind: 'session', sessionId: 's1', sessionPath: null, conversationId: 'c1',
  conversationType: 'dm', agentId: 'a1', childAgentId: null, childSessionId: null, taskId: null,
};

function makeCall(overrides: Partial<ModelObservabilityCallListItem> & { callId: string }): ModelObservabilityCallListItem {
  const { attribution, ...rest } = overrides;
  return {
    traceId: 'mt_x',
    parentCallId: null,
    startedAt: new Date(T0).toISOString(),
    endedAt: new Date(T0 + 2_000).toISOString(),
    durationMs: 2_000,
    terminalStatus: 'ok',
    persistenceCompleteness: 'known',
    interruptedByRestart: false,
    model: { provider: 'openai', modelId: 'gpt-test', api: 'responses' },
    source: { subsystem: 'llm', operation: 'chat', surface: 'server', trigger: 'user_turn' },
    callPurpose: null,
    inputShape: 'messages',
    provenancePrecision: 'exact',
    provenance: { sectionCount: 0, opaqueCount: 0, categories: [], categoriesState: 'absent' },
    payloadAvailability: 'not_captured',
    payloadRecordCount: 0,
    usage: {
      availability: 'present',
      status: 'ok',
      summary: {
        inputTokens: 100, outputTokens: 40, reasoningTokens: 10,
        cacheReadTokens: 0, cacheWriteTokens: 0, totalTokens: 140, costTotal: 0.01,
      },
    },
    attemptCount: 1,
    providerRequestCount: 1,
    ...rest,
    attribution: { ...BASE_ATTRIBUTION, ...(attribution ?? {}) },
  };
}

function makeDetail(calls: ModelObservabilityCallListItem[]): ModelObservabilityTraceDetail {
  return {
    trace: {
      traceId: 'mt_x', origin: 'user_turn', firstSeenAt: '', lastSeenAt: '',
      callCount: calls.length, terminalOk: 0, terminalError: 0, terminalAborted: 0, incomplete: 0,
    },
    calls,
    roots: [],
    edges: [],
    orphanEdges: [],
    graphIntegrity: 'ok',
    usageAggregate: {
      availability: 'complete', coveredCalls: 0, corruptCalls: 0,
      notCorrelatedCalls: 0, unknownCalls: 0, totalCalls: calls.length, summary: null,
    },
    payloadCompleteness: { present: 0, expired: 0, dropped: 0, notCaptured: calls.length, unknown: 0 },
    dataCompleteness: {
      status: 'known', droppedTraceEvents: 0, droppedPayloadRecords: 0,
      droppedBlobs: 0, interruptedByRestartCalls: 0,
    },
  };
}

function cellsOf(turns: ReturnType<typeof buildTraceConversationModel>['turns']): TrajectoryCellProps[] {
  return turns.flatMap(turn => turn.groups.flatMap(group => group.cells));
}

describe('resolveTraceSessionId', () => {
  it('多数票选出 sessionId，忽略空值', () => {
    const calls = [
      makeCall({ callId: 'c1' }),
      makeCall({ callId: 'c2', attribution: { ...BASE_ATTRIBUTION, sessionId: 's2' } }),
      makeCall({ callId: 'c3', attribution: { ...BASE_ATTRIBUTION, sessionId: null } }),
    ];
    expect(resolveTraceSessionId(calls)).toBe('s1');
  });
  it('全部为空 → null', () => {
    expect(resolveTraceSessionId([makeCall({ callId: 'c1', attribution: { ...BASE_ATTRIBUTION, sessionId: null } })])).toBeNull();
  });
});

describe('buildTraceConversationModel — calls-only 降级', () => {
  it('无会话数据：主链调用各占 Step，子调用按 parentCallId 嵌套为 subtool', () => {
    const main = makeCall({ callId: 'main' });
    const child = makeCall({
      callId: 'child',
      parentCallId: 'main',
      startedAt: new Date(T0 + 100).toISOString(),
      callPurpose: 'knowledge_rollup',
    });
    const main2 = makeCall({ callId: 'main2', startedAt: new Date(T0 + 5_000).toISOString() });
    const model = buildTraceConversationModel(makeDetail([main, child, main2]), null);
    expect(model.sessionJoined).toBe(false);
    expect(model.sessionId).toBeNull();
    expect(model.turns.length).toBe(1);
    const turn = model.turns[0]!;
    expect(turn.turn).toBe(1);
    expect(turn.groups.map(group => group.title)).toEqual(['Step 1', 'Step 2']);
    const step1Kinds = turn.groups[0]!.cells.map(cell => cell.kind);
    // SYSTEM 首记录（dsh 首屏对齐）位于第一条调用记录之前。
    expect(step1Kinds).toEqual(['system', 'message', 'subtool']);
    expect(turn.groups[0]!.cells[0]!.kind).toBe('system');
    expect(turn.groups[0]!.cells[1]!.observabilityCallId).toBe('main');
    expect(turn.groups[0]!.cells[2]!.observabilityCallId).toBe('child');
    // 请求编号：两个 Step 各一条。
    expect(model.requestNumbers.length).toBe(2);
    expect(model.requestNumbers[0]!.number).toBe(1);
    expect(model.requestNumbers[1]!.number).toBe(2);
    // usage 累计：100+100 input。
    expect(model.requestNumbers[1]!.cumulativeUsage?.input).toBe(200);
  });

  it('error/aborted 调用标记 isError；请求编号 status=error', () => {
    const failed = makeCall({ callId: 'bad', terminalStatus: 'error' });
    const model = buildTraceConversationModel(makeDetail([failed]), []);
    const cells = cellsOf(model.turns);
    expect(cells.find(cell => cell.kind === 'message')!.isError).toBe(true);
    expect(model.requestNumbers[0]!.status).toBe('error');
  });
});

describe('buildTraceConversationModel — 会话 join', () => {
  const userMsg = {
    role: 'user',
    entryId: 'eu1',
    content: '帮我查一下',
    timestamp: new Date(T0 - 5_000).toISOString(),
  };
  const assistantMsg = (offset: number, overrides: Record<string, unknown> = {}) => ({
    role: 'assistant',
    entryId: 'ea1',
    content: '答案正文',
    timestamp: new Date(T0 + offset).toISOString(),
    ...overrides,
  });

  it('用户消息开轮、助手消息配对主链调用并挂 usage/timing', () => {
    const main = makeCall({ callId: 'main' }); // [T0, T0+2s]
    const messages = [userMsg, assistantMsg(3_000)];
    const model = buildTraceConversationModel(makeDetail([main]), messages);
    expect(model.sessionJoined).toBe(true);
    expect(model.turns.length).toBe(1);
    const cells = cellsOf(model.turns);
    const userCell = cells.find(cell => cell.kind === 'user');
    expect(userCell?.opensTurn).toBe(true);
    expect(userCell?.previewMarkdown).toBe('帮我查一下');
    const messageCell = cells.find(cell => cell.kind === 'message');
    expect(messageCell?.observabilityCallId).toBe('main');
    expect(messageCell?.outputDetail).toBe('答案正文');
    expect(messageCell?.input).toBe(100);
    expect(messageCell?.output).toBe(40);
    expect(messageCell?.think).toBe(10);
    expect(messageCell?.assistantMetrics?.stepStartTime).toBe(T0);
    // 无首 token 事实：TTFT 不虚构。
    expect(messageCell?.assistantMetrics?.firstTokenTime).toBeNull();
    expect(messageCell?.timeSeconds).toBe(2);
  });

  it('助手 toolCalls 投影 → tool 记录（成功带输出、失败带错误）', () => {
    const main = makeCall({ callId: 'main' });
    const messages = [
      userMsg,
      assistantMsg(3_000, {
        toolCalls: [
          { id: 'tu1', name: 'search', args: '{"q":"x"}', status: 'ok', success: true, details: { output: '3 hits' } },
          { id: 'tu2', name: 'write', status: 'error', success: false, error: 'denied' },
        ],
      }),
    ];
    const model = buildTraceConversationModel(makeDetail([main]), messages);
    const cells = cellsOf(model.turns);
    const tools = cells.filter(cell => cell.kind === 'tool');
    expect(tools.length).toBe(2);
    expect(tools[0]!.text).toBe('search');
    expect(tools[0]!.resultPreviewMarkdown).toBe('3 hits');
    expect(tools[0]!.isError).toBeUndefined();
    expect(tools[1]!.text).toBe('write');
    expect(tools[1]!.result).toBe('denied');
    expect(tools[1]!.isError).toBe(true);
  });

  it('工具计时来自会话条目时间戳；无输出的成功工具是终态而非等待中', () => {
    const main = makeCall({ callId: 'main' });
    const messages = [
      userMsg,
      assistantMsg(3_000, {
        timestamp: new Date(T0 + 1_000).toISOString(),
        toolCalls: [
          {
            id: 'tu1', name: 'read_file', status: 'ok', success: true,
            startedAt: new Date(T0 + 1_000).toISOString(),
            endedAt: new Date(T0 + 2_500).toISOString(),
            details: { output: '内容' },
          },
          {
            id: 'tu2', name: 'noop', status: 'ok', success: true,
            startedAt: new Date(T0 + 1_000).toISOString(),
            endedAt: new Date(TOOL_END_NOOP()).toISOString(),
            // 无 details.output：终态「无输出」，不得是等待中。
          },
        ],
      }),
    ];
    const model = buildTraceConversationModel(makeDetail([main]), messages);
    const tools = cellsOf(model.turns).filter(cell => cell.kind === 'tool');
    // 计时：startedAt/时长/来源=会话时间戳。
    expect(tools[0]!.startedAt).toBe(T0 + 1_000);
    expect(tools[0]!.timeSeconds).toBe(1.5);
    expect(tools[0]!.timingSource).toBe('session');
    // 无输出终态：outputDetail 空串（stateOf→complete）+ result=No output。
    expect(tools[1]!.outputDetail).toBe('');
    expect(tools[1]!.result).toBe('No output');
  });

  it('轮内未配对侧线 → 额外 Step；轮后侧线 → Between turns 独立区段', () => {
    const main = makeCall({ callId: 'main' }); // [T0, T0+2s]，assistant ts=T0+3s
    const rollup = makeCall({
      callId: 'rollup',
      callPurpose: 'knowledge_rollup',
      startedAt: new Date(T0 + 500).toISOString(),
      endedAt: new Date(T0 + 1_500).toISOString(),
    });
    const title = makeCall({
      callId: 'title',
      callPurpose: 'title',
      startedAt: new Date(T0 + 6_000).toISOString(),
      endedAt: new Date(T0 + 6_500).toISOString(),
    });
    const messages = [userMsg, assistantMsg(3_000)];
    const model = buildTraceConversationModel(makeDetail([main, rollup, title]), messages);
    expect(model.turns.length).toBe(2);
    expect(model.turns[0]!.turn).toBe(1);
    expect(model.turns[1]!.turn).toBeNull();
    // 轮 1：Message(user) + Step1(assistant+main) + Step2(rollup)
    expect(model.turns[0]!.groups.map(group => group.title)).toEqual(['Message', 'Step 1', 'Step 2']);
    expect(model.turns[0]!.groups[2]!.cells[0]!.observabilityCallId).toBe('rollup');
    // Between turns：title 侧线调用。
    expect(model.turns[1]!.groups.length).toBe(1);
    expect(model.turns[1]!.groups[0]!.cells[0]!.observabilityCallId).toBe('title');
    // 请求编号：轮内两条 assistant 语义，侧线 purpose=side 且 turn=null。
    const side = model.requestNumbers.find(request => request.purpose === 'side');
    expect(side?.turn).toBeNull();
    expect(side?.step).toBe(0);
  });

  it('配对防抢：知识滚动调用不会抢走助手消息（最近结束原则）', () => {
    const rollup = makeCall({
      callId: 'rollup',
      startedAt: new Date(T0 - 4_000).toISOString(),
      endedAt: new Date(T0 - 3_000).toISOString(),
    });
    const main = makeCall({ callId: 'main' }); // 结束 T0+2s，距 assistant(T0+3s) 更近
    const messages = [userMsg, assistantMsg(3_000)];
    const model = buildTraceConversationModel(makeDetail([rollup, main]), messages);
    const cells = cellsOf(model.turns);
    const messageCell = cells.find(cell => cell.kind === 'message');
    expect(messageCell?.observabilityCallId).toBe('main');
  });

  it('时间窗锚定：窗口前的最近用户消息被拉入，窗口后的用户消息截断', () => {
    const earlierUser = {
      role: 'user', entryId: 'eu0', content: '旧问题',
      timestamp: new Date(T0 - 60_000).toISOString(),
    };
    const laterUser = {
      role: 'user', entryId: 'eu2', content: '下一问',
      timestamp: new Date(T0 + 30_000).toISOString(),
    };
    const main = makeCall({ callId: 'main' });
    const messages = [
      earlierUser,
      { role: 'assistant', entryId: 'ea0', content: '旧回答', timestamp: new Date(T0 - 55_000).toISOString() },
      userMsg,
      assistantMsg(3_000),
      laterUser,
    ];
    const model = buildTraceConversationModel(makeDetail([main]), messages);
    // 旧轮不在窗口（其 assistant 结束远早于窗口起点 T0）……锚点用户消息是
    // eu1（<= T0 的最近用户消息），故旧轮被排除、eu2 之后截断。
    const texts = cellsOf(model.turns)
      .filter(cell => cell.kind === 'user')
      .map(cell => cell.previewMarkdown);
    expect(texts).toEqual(['帮我查一下']);
    expect(model.turns.length).toBe(1);
  });

  it('缺时间戳无法 join → calls-only 降级', () => {
    const main = makeCall({ callId: 'main' });
    const messages = [{ role: 'user', entryId: 'eu1', content: '无时间戳' }];
    const model = buildTraceConversationModel(makeDetail([main]), messages);
    expect(model.sessionJoined).toBe(false);
    expect(cellsOf(model.turns).some(cell => cell.kind === 'user')).toBe(false);
  });
});

describe('parseSessionMessages', () => {
  it('结构宽容：非数组 → null；坏行跳过、字段缺省安全', () => {
    expect(parseSessionMessages(null)).toBeNull();
    expect(parseSessionMessages('x')).toBeNull();
    const parsed = parseSessionMessages([
      { role: 'system', content: 'ignored' },
      { role: 'user', content: 'hi', timestamp: new Date(T0).toISOString() },
      'garbage',
      { role: 'assistant', content: '', toolCalls: [{ name: 't1', id: 'x1' }, { name: '' }] },
    ] as unknown[]);
    expect(parsed?.length).toBe(2);
    expect(parsed?.[0]?.timestampMs).toBe(T0);
    expect(parsed?.[1]?.toolCalls?.length).toBe(1);
  });
});

describe('移植纯函数核心断言', () => {
  const cell = (index: number, kind: TrajectoryCellProps['kind'], startedAt: number | null, seconds: number | null): TrajectoryCellProps => ({
    index, kind, text: `r${index}`, timeSeconds: seconds, startedAt,
  });
  const turns = [
    { turn: 1, groups: [{ title: 'Step 1', cells: [cell(1, 'user', 0, 0), cell(2, 'message', 100, 1)] }] },
    { turn: null, groups: [{ title: 'Side 1', cells: [cell(3, 'message', 2_000, 1)] }] },
  ];

  it('sequence 模式三泳道投影：user→0、message→1、tool→2', () => {
    const model = deriveTrajectoryTimeline(turns, 'sequence');
    expect(model).not.toBeNull();
    const lanes = model!.spans.map(span => span.lane);
    expect(lanes).toEqual([0, 1, 1]);
    // 轮次边界只在非空 turn。
    expect(model!.turnBoundaries.map(boundary => boundary.turn)).toEqual([1]);
  });

  it('duration 模式按记录耗时投影并压缩空隙', () => {
    const model = deriveTrajectoryTimeline(turns, 'duration');
    expect(model).not.toBeNull();
    expect(model!.spans[0]!.start).toBe(0);
    expect(model!.spans[0]!.end).toBe(0); // duration 模式 span 终点=起点（等宽 0）
  });

  it('区间聚焦：闭区间重叠的记录入选', () => {
    const focused = trajectoryTimelineFocusIndexes(turns, { start: 1_500, end: 2_500 }, 'duration');
    expect([...focused].sort((a, b) => a - b)).toEqual([3]);
  });

  it('request-only 记录并入下一条内容行；终端分隔独立成行', () => {
    const records = [
      { cell: { ...cell(1, 'message', 0, null), requestOnly: true } },
      { cell: cell(2, 'message', 10, 1) },
      { cell: { ...cell(3, 'message', 20, null), requestOnly: true } },
    ];
    const rows = groupTrajectoryVirtualRows(records);
    expect(rows.length).toBe(2);
    expect(rows[0]!.entries.length).toBe(2);
    expect(rows[0]!.height).toBe(30);
    expect(rows[1]!.height).toBe(9);
    expect(rows[1]!.entries[0]!.record.cell.requestOnly).toBe(true);
  });
});

describe('SYSTEM 首记录 — 会话提示词快照装配', () => {
  it('快照可用时 promptDetail 即刻携带系统提示词与工具目录', () => {
    const main = makeCall({ callId: 'main' });
    const model = buildTraceConversationModel(
      makeDetail([main]),
      null,
      { systemPrompt: 'base', appendSystemPrompt: ['extra'], finalSystemPrompt: 'final-assembled' },
      ['read_file', 'write_file'],
    );
    const cells = cellsOf(model.turns);
    const systemCellFound = cells.find(cell => cell.kind === 'system');
    expect(systemCellFound?.promptDetail?.system).toBe('final-assembled');
    expect(systemCellFound?.promptDetail?.tools.map(tool => tool.name)).toEqual(['read_file', 'write_file']);
  });

  it('缺 finalSystemPrompt 时回退 systemPrompt + appendSystemPrompt 拼接', () => {
    const main = makeCall({ callId: 'main' });
    const model = buildTraceConversationModel(
      makeDetail([main]),
      null,
      { systemPrompt: 'base', appendSystemPrompt: ['extra-1', 'extra-2'] },
      null,
    );
    const systemCellFound = cellsOf(model.turns).find(cell => cell.kind === 'system');
    expect(systemCellFound?.promptDetail?.system).toBe('base\n\nextra-1\n\nextra-2');
    expect(systemCellFound?.promptDetail?.tools).toEqual([]);
  });

  it('快照缺失 → promptDetail 留空（检查器走载荷懒加载/未捕获态）', () => {
    const main = makeCall({ callId: 'main' });
    const model = buildTraceConversationModel(makeDetail([main]), null);
    const systemCellFound = cellsOf(model.turns).find(cell => cell.kind === 'system');
    expect(systemCellFound?.promptDetail).toBeUndefined();
    expect(systemCellFound?.observabilityCallId).toBe('main');
  });
});

describe('主链分类 — agent 多步轮（实盘回归 sess_0mti8f3we）', () => {
  // 实测形状：reply(call1, root) → title(parent=1) → reply2(parent=1) → reply3(parent=2)。
  // reply2/reply3 带 parentCallId 但语义是主回复链，必须各自配对助手消息。
  const userMsgChain = {
    role: 'user', entryId: 'eu-chain', content: '多步问题',
    timestamp: new Date(T0 - 5_000).toISOString(),
  };
  const assistantChain = (offset: number, entryId: string) => ({
    role: 'assistant', entryId, content: `第${entryId}段回答`,
    timestamp: new Date(T0 + offset).toISOString(),
  });

  it('带 parent 的 session/reply 参与配对：每个助手都有观测载荷关联', () => {
    const replySource = { subsystem: 'session', operation: 'reply', surface: 'server', trigger: 'user_turn' };
    const reply1 = makeCall({ callId: 'reply1', source: replySource }); // [T0, T0+7.3s]
    const title = makeCall({
      callId: 'title',
      parentCallId: 'reply1',
      startedAt: new Date(T0 + 6_100).toISOString(), endedAt: new Date(T0 + 6_500).toISOString(),
      source: { subsystem: 'auxiliary', operation: 'title', surface: 'server', trigger: 'user_turn' },
      callPurpose: 'title',
    });
    const reply2 = makeCall({
      callId: 'reply2', parentCallId: 'reply1', source: replySource,
      startedAt: new Date(T0 + 7_300).toISOString(), endedAt: new Date(T0 + 10_900).toISOString(),
    });
    const reply3 = makeCall({
      callId: 'reply3', parentCallId: 'reply2', source: replySource,
      startedAt: new Date(T0 + 11_000).toISOString(), endedAt: new Date(T0 + 25_500).toISOString(),
    });
    const messages = [
      userMsgChain,
      assistantChain(7_400, 'ea1'),
      assistantChain(11_000, 'ea2'),
      assistantChain(25_600, 'ea3'),
    ];
    const model = buildTraceConversationModel(
      makeDetail([reply1, title, reply2, reply3]),
      messages,
    );
    expect(model.sessionJoined).toBe(true);
    const messageCells = cellsOf(model.turns).filter(cell => cell.kind === 'message');
    // 三个助手消息各自配到 reply1/reply2/reply3（title 保持子调用）。
    expect(messageCells.length).toBe(3);
    expect(messageCells[0]!.observabilityCallId).toBe('reply1');
    expect(messageCells[1]!.observabilityCallId).toBe('reply2');
    expect(messageCells[2]!.observabilityCallId).toBe('reply3');
    // title 仍是 subtool（嵌在首个 Step 内）。
    const subtools = cellsOf(model.turns).filter(cell => cell.kind === 'subtool');
    expect(subtools.map(cell => cell.observabilityCallId)).toEqual(['title']);
    // 三个主链调用各自的 Step 分组 + 请求编号。
    expect(model.requestNumbers.length).toBe(3);
  });

  it('calls-only 模式：带 parent 的 reply 也各占 Step（不再折叠成 subtool）', () => {
    const replySource = { subsystem: 'session', operation: 'reply', surface: 'server', trigger: 'user_turn' };
    const reply1 = makeCall({ callId: 'reply1', source: replySource });
    const reply2 = makeCall({
      callId: 'reply2', parentCallId: 'reply1', source: replySource,
      startedAt: new Date(T0 + 8_000).toISOString(), endedAt: new Date(T0 + 9_000).toISOString(),
    });
    const model = buildTraceConversationModel(makeDetail([reply1, reply2]), null);
    const turn = model.turns[0]!;
    expect(turn.groups.map(group => group.title)).toEqual(['Step 1', 'Step 2']);
    expect(turn.groups[1]!.cells[0]!.observabilityCallId).toBe('reply2');
  });
});
