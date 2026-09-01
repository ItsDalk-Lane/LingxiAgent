/**
 * trace-conversation-model.ts — 观测 trace → dsh 形状轨迹布局的数据装配。
 *
 * 输入两路数据、输出一份合并视图：
 *   1. 观测库 TraceDetail（全部 model calls + usage）——永远可用，是骨架；
 *   2. 会话消息（/api/sessions/messages，按 trace calls 的 sessionId 多数票
 *      反查）——可用时提供对话叙事（用户消息/助手正文/工具调用与结果）。
 *
 * 轮次语义（对齐 dsh ui-trajectory）：用户消息开 Turn N；助手消息与其工具
 * 调用、观测子调用归入轮内 `Step k` 分组；轮内未配对的侧线调用（知识滚动
 * 等，发生在助手回复之前）各自成 Step 分组；发生在轮内最后一条助手消息
 * 之后的侧线调用（标题生成/压缩等）进 `Between turns` 独立区段（turn=null）。
 * 无会话数据（后台/日记类 trace，或会话拉取失败）时降级为 calls-only：
 * Turn 1 单区段，主链调用各占一个 Step 分组，子调用按 parentCallId 嵌套。
 *
 * 时间 join 诚实性：消息与调用都允许缺时间戳——缺时间戳的消息按相邻消息
 * 归属轮次并继续参与（不伪造时间），整体无法归属时进 calls-only。
 */

import type {
  ModelObservabilityCallListItem,
  ModelObservabilityTraceDetail,
  ModelObservabilityUsageSummary,
} from '../../../../../../../shared/model-observability-api-contract.ts';
import {
  describeTrajectoryGroup,
  durationSecondsBetween,
  type LaidCellLike,
  type TrajectoryRequestNumber,
  type TrajectoryTurnModel,
  type TrajectoryUsage,
} from './trace-layout.ts';
import type { TrajectoryCellProps, TrajectorySourceBlock } from './trajectory-record.ts';

/* ── 会话消息最小形状（/api/sessions/messages 响应的子集）────────────── */

export interface SessionToolCallProjection {
  id?: string;
  name: string;
  args?: string;
  status?: string;
  success?: boolean;
  error?: unknown;
  /** 会话条目时间戳（ISO）：assistant 条目时间（执行前落盘）。 */
  startedAt?: unknown;
  /** 会话条目时间戳（ISO）：toolResult 条目时间（执行后落盘）。 */
  endedAt?: unknown;
  details?: { output?: unknown } & Record<string, unknown>;
}

export interface ParsedSessionMessage {
  role: 'user' | 'assistant';
  entryId: string | null;
  content: string;
  thinking?: string;
  displayText?: string;
  images?: { data: string; mimeType: string }[];
  toolCalls?: SessionToolCallProjection[];
  turnStatus?: string;
  /** epoch ms；缺失为 null（老 entry 可能没有 timestamp，按相邻归属）。 */
  timestampMs: number | null;
}

/** 会话冻结提示词快照（/api/sessions/prompt-snapshot 的 promptSnapshot 字段子集）。 */
export interface SessionPromptSnapshotInput {
  systemPrompt?: string | null;
  appendSystemPrompt?: string[];
  finalSystemPrompt?: string;
}

/** 装配结果。 */
export interface TraceConversationModel {
  turns: readonly TrajectoryTurnModel[];
  requestNumbers: readonly TrajectoryRequestNumber[];
  /** 多数票选出的会话 id（calls 无 sessionId 时为 null）。 */
  sessionId: string | null;
  /** 会话消息成功 join（false = calls-only 降级）。 */
  sessionJoined: boolean;
}

function epochMs(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string' && value !== '') {
    const parsed = Date.parse(value);
    return Number.isNaN(parsed) ? null : parsed;
  }
  return null;
}

function callStartMs(call: ModelObservabilityCallListItem): number | null {
  return epochMs(call.startedAt);
}

function callEndMs(call: ModelObservabilityCallListItem): number | null {
  return epochMs(call.endedAt);
}

function callLabel(call: ModelObservabilityCallListItem): string {
  return call.callPurpose || call.source?.operation || call.model?.modelId || call.callId;
}

/**
 * 会话主链步骤判据：subsystem session + operation/purpose reply。
 * agent 多步轮里第 2..N 步回复由上一步工具结果引发、观测记了
 * parentCallId（实测 253 次 session/reply 中 151 次带 parent），语义仍是
 * 主回复链——参与助手配对；auxiliary/title、subagent/run 等侧线保持子调用。
 */
function isMainChainReply(call: ModelObservabilityCallListItem): boolean {
  return call.source?.subsystem === 'session'
    && (call.source?.operation === 'reply' || call.callPurpose === 'reply');
}

function callStatus(call: ModelObservabilityCallListItem): 'complete' | 'running' | 'error' {
  if (call.terminalStatus === 'ok') return 'complete';
  if (call.terminalStatus === 'error' || call.terminalStatus === 'aborted') return 'error';
  return 'running';
}

function usageFromSummary(
  summary: ModelObservabilityUsageSummary | null | undefined,
): TrajectoryUsage | undefined {
  if (!summary) return undefined;
  const usage: TrajectoryUsage = {};
  if (typeof summary.inputTokens === 'number') usage.input = summary.inputTokens;
  if (typeof summary.cacheReadTokens === 'number') usage.cacheRead = summary.cacheReadTokens;
  if (typeof summary.cacheWriteTokens === 'number') usage.cacheWrite = summary.cacheWriteTokens;
  if (typeof summary.outputTokens === 'number') usage.output = summary.outputTokens;
  if (typeof summary.reasoningTokens === 'number') usage.reasoning = summary.reasoningTokens;
  return Object.keys(usage).length === 0 ? undefined : usage;
}

/* ── 会话消息解析（结构宽容：坏行跳过，不炸整体）─────────────────────── */

export function parseSessionMessages(raw: unknown): ParsedSessionMessage[] | null {
  if (!Array.isArray(raw)) return null;
  const out: ParsedSessionMessage[] = [];
  for (const item of raw) {
    if (typeof item !== 'object' || item === null) continue;
    const message = item as Record<string, unknown>;
    const role = message.role;
    if (role !== 'user' && role !== 'assistant') continue;
    const parsed: ParsedSessionMessage = {
      role,
      entryId: typeof message.entryId === 'string' && message.entryId !== '' ? message.entryId : null,
      content: typeof message.content === 'string' ? message.content : '',
      timestampMs: epochMs(message.timestamp),
    };
    if (role === 'user') {
      if (typeof message.displayText === 'string' && message.displayText !== '') {
        parsed.displayText = message.displayText;
      }
      if (Array.isArray(message.images)) {
        const images = message.images
          .map((image): { data: string; mimeType: string } | null => {
            if (typeof image !== 'object' || image === null) return null;
            const candidate = image as Record<string, unknown>;
            if (typeof candidate.data !== 'string' || candidate.data === '') return null;
            return {
              data: candidate.data,
              mimeType: typeof candidate.mimeType === 'string' ? candidate.mimeType : 'image/png',
            };
          })
          .filter((image): image is { data: string; mimeType: string } => image !== null);
        if (images.length > 0) parsed.images = images;
      }
    } else {
      if (typeof message.thinking === 'string' && message.thinking !== '') {
        parsed.thinking = message.thinking;
      }
      if (typeof message.turnStatus === 'string') parsed.turnStatus = message.turnStatus;
      if (Array.isArray(message.toolCalls)) {
        const toolCalls = message.toolCalls
          .map((call): SessionToolCallProjection | null => {
            if (typeof call !== 'object' || call === null) return null;
            const candidate = call as Record<string, unknown>;
            if (typeof candidate.name !== 'string' || candidate.name === '') return null;
            const projected: SessionToolCallProjection = { name: candidate.name };
            if (typeof candidate.id === 'string' && candidate.id !== '') projected.id = candidate.id;
            if (typeof candidate.args === 'string' && candidate.args !== '') projected.args = candidate.args;
            if (typeof candidate.status === 'string') projected.status = candidate.status;
            if (typeof candidate.success === 'boolean') projected.success = candidate.success;
            if (candidate.error !== undefined) projected.error = candidate.error;
            if (candidate.startedAt !== undefined) projected.startedAt = candidate.startedAt;
            if (candidate.endedAt !== undefined) projected.endedAt = candidate.endedAt;
            if (typeof candidate.details === 'object' && candidate.details !== null) {
              projected.details = candidate.details as { output?: unknown } & Record<string, unknown>;
            }
            return projected;
          })
          .filter((call): call is SessionToolCallProjection => call !== null);
        if (toolCalls.length > 0) parsed.toolCalls = toolCalls;
      }
    }
    out.push(parsed);
  }
  return out;
}

/* ── 布局构造内部类型 ─────────────────────────────────────────────────── */

interface GroupDraft {
  title: string;
  laid: LaidCellLike[];
}

interface TurnDraft {
  turn: number;
  startMs: number | null;
  endMs: number | null;
  groups: GroupDraft[];
}

let recordCounter = 0;

function nextIndex(): number {
  recordCounter += 1;
  return recordCounter;
}

function pushCell(turn: TurnDraft, groupTitle: string, laid: LaidCellLike): void {
  const existing = turn.groups.find(group => group.title === groupTitle);
  if (existing !== undefined) {
    existing.laid.push(laid);
    return;
  }
  turn.groups.push({ title: groupTitle, laid: [laid] });
}

function finalizeGroup(group: GroupDraft) {
  if (group.laid.length === 0) return null;
  const description = describeTrajectoryGroup(group.laid);
  return {
    title: group.title,
    ...(description !== undefined ? { description } : {}),
    cells: group.laid.map(laid => laid.cell),
  };
}

function finalizeTurns(turnDrafts: TurnDraft[], standaloneGroups: GroupDraft[]): TrajectoryTurnModel[] {
  const turns = turnDrafts
    .map(draft => ({ turn: draft.turn, groups: draft.groups.map(finalizeGroup) }))
    .map(draft => ({ turn: draft.turn, groups: draft.groups.filter(g => g !== null) }))
    .filter(draft => draft.groups.length > 0);
  const standalone = standaloneGroups
    .map(finalizeGroup)
    .filter(g => g !== null);
  if (standalone.length === 0) return turns;
  // Between turns 区段时间上晚于轮内最后一条助手消息，按 dsh 独立 compaction
  // 语义排在末尾。
  return [...turns, { turn: null, groups: standalone }];
}

/* ── 主装配 ───────────────────────────────────────────────────────────── */

/** 会话归属投票：取 calls attribution.sessionId 的众数。 */
export function resolveTraceSessionId(
  calls: readonly ModelObservabilityCallListItem[],
): string | null {
  const votes = new Map<string, number>();
  for (const call of calls) {
    const sessionId = call.attribution?.sessionId;
    if (typeof sessionId !== 'string' || sessionId === '') continue;
    votes.set(sessionId, (votes.get(sessionId) ?? 0) + 1);
  }
  let best: string | null = null;
  let bestCount = 0;
  for (const [sessionId, count] of votes) {
    if (count > bestCount) {
      best = sessionId;
      bestCount = count;
    }
  }
  return best;
}

export function buildTraceConversationModel(
  detail: ModelObservabilityTraceDetail,
  sessionMessagesRaw: unknown,
  promptSnapshot: SessionPromptSnapshotInput | null = null,
  toolNames: readonly string[] | null = null,
): TraceConversationModel {
  recordCounter = 0;
  const calls = detail.calls;
  const sessionId = resolveTraceSessionId(calls);
  const sessionMessages = sessionMessagesRaw === null || sessionMessagesRaw === undefined
    ? null
    : parseSessionMessages(sessionMessagesRaw);

  const callById = new Map(calls.map(call => [call.callId, call] as const));
  const childrenByParent = new Map<string, ModelObservabilityCallListItem[]>();
  const mainCalls: ModelObservabilityCallListItem[] = [];
  for (const call of calls) {
    const parent = call.parentCallId;
    if (
      typeof parent === 'string' && parent !== '' && callById.has(parent)
      && !isMainChainReply(call)
    ) {
      const siblings = childrenByParent.get(parent) ?? [];
      siblings.push(call);
      childrenByParent.set(parent, siblings);
    } else {
      mainCalls.push(call);
    }
  }
  const byStart = (
    left: ModelObservabilityCallListItem,
    right: ModelObservabilityCallListItem,
  ): number => {
    const leftStart = callStartMs(left) ?? Number.POSITIVE_INFINITY;
    const rightStart = callStartMs(right) ?? Number.POSITIVE_INFINITY;
    return leftStart - rightStart || left.callId.localeCompare(right.callId);
  };
  mainCalls.sort(byStart);
  for (const siblings of childrenByParent.values()) siblings.sort(byStart);

  if (sessionMessages !== null && sessionMessages.length > 0 && canJoinSession(sessionMessages, calls)) {
    return buildSessionJoinedModel(sessionId, sessionMessages, mainCalls, childrenByParent, promptSnapshot, toolNames);
  }
  return buildCallsOnlyModel(mainCalls, childrenByParent, promptSnapshot, toolNames);
}

/** join 前提：会话里至少一条消息有时间戳，且 trace 至少一条 call 有开始时间。 */
function canJoinSession(
  messages: readonly ParsedSessionMessage[],
  calls: readonly ModelObservabilityCallListItem[],
): boolean {
  const anyMessageTime = messages.some(message => message.timestampMs !== null);
  const anyCallTime = calls.some(call => callStartMs(call) !== null);
  return anyMessageTime && anyCallTime;
}

/* ── 会话 join 模式 ───────────────────────────────────────────────────── */

interface TurnSlice {
  turn: number;
  startMs: number | null;
  endMs: number | null;
  messages: ParsedSessionMessage[];
  lastAssistantTs: number | null;
}

function buildSessionJoinedModel(
  sessionId: string | null,
  messages: readonly ParsedSessionMessage[],
  mainCalls: readonly ModelObservabilityCallListItem[],
  childrenByParent: ReadonlyMap<string, ModelObservabilityCallListItem[]>,
  promptSnapshot: SessionPromptSnapshotInput | null,
  toolNames: readonly string[] | null,
): TraceConversationModel {
  // 时间窗：从 trace 首个 call 往前找到最近的用户消息作为锚点，到窗后首个
  // 用户消息（不含）为止——保证容纳首尾 call 的完整轮次。
  const callTimes = mainCalls
    .map(call => callStartMs(call))
    .filter((time): time is number => time !== null);
  const windowStart = callTimes.length > 0 ? Math.min(...callTimes) : null;
  const windowEnd = callTimes.length > 0
    ? Math.max(...mainCalls.map(call => callEndMs(call) ?? callStartMs(call) ?? 0))
    : null;

  const timedUserIndexes = messages
    .map((message, index) => ({ message, index }))
    .filter(entry => entry.message.role === 'user' && entry.message.timestampMs !== null);
  let anchorIndex = 0;
  if (windowStart !== null && timedUserIndexes.length > 0) {
    const before = timedUserIndexes.filter(entry => (entry.message.timestampMs ?? 0) <= windowStart);
    anchorIndex = before.length > 0 ? before[before.length - 1]!.index : 0;
  }
  const windowMessages: ParsedSessionMessage[] = [];
  for (let index = anchorIndex; index < messages.length; index++) {
    const message = messages[index];
    if (message === undefined) break;
    const ts = message.timestampMs;
    if (
      windowEnd !== null
      && message.role === 'user'
      && ts !== null
      && ts > windowEnd
      && windowMessages.length > 0
    ) break;
    windowMessages.push(message);
  }

  // 轮次切分 + 每轮窗口时间。
  const slices: TurnSlice[] = [];
  for (const message of windowMessages) {
    const current = slices[slices.length - 1];
    if (message.role === 'user' || current === undefined) {
      slices.push({
        turn: slices.length + 1,
        startMs: message.timestampMs,
        endMs: null,
        messages: [message],
        lastAssistantTs: null,
      });
      continue;
    }
    current.messages.push(message);
    if (message.role === 'assistant' && message.timestampMs !== null) {
      current.lastAssistantTs = message.timestampMs;
    }
  }
  for (let index = 0; index < slices.length; index++) {
    const slice = slices[index];
    if (slice === undefined) continue;
    slice.endMs = slices[index + 1]?.startMs ?? null;
    if (slice.startMs === null) {
      slice.startMs = slice.messages.find(message => message.timestampMs !== null)?.timestampMs ?? null;
    }
  }

  // 主链调用按时间中点归轮；归不进任何轮的（无时间或在首轮之前）挂最后一轮。
  const callsBySlice = new Map<TurnSlice, ModelObservabilityCallListItem[]>();
  const unplacedCalls: ModelObservabilityCallListItem[] = [];
  for (const call of mainCalls) {
    const start = callStartMs(call);
    const end = callEndMs(call) ?? start;
    const mid = start !== null && end !== null ? (start + end) / 2 : null;
    const slice = mid === null
      ? undefined
      : slices.find(candidate => {
        const sliceStart = candidate.startMs;
        if (sliceStart === null) return false;
        const sliceEnd = candidate.endMs ?? Number.POSITIVE_INFINITY;
        return mid >= sliceStart && mid < sliceEnd;
      });
    if (slice === undefined) {
      unplacedCalls.push(call);
      continue;
    }
    const bucket = callsBySlice.get(slice) ?? [];
    bucket.push(call);
    callsBySlice.set(slice, bucket);
  }
  const lastSlice = slices[slices.length - 1];
  if (lastSlice !== undefined && unplacedCalls.length > 0) {
    const bucket = callsBySlice.get(lastSlice) ?? [];
    bucket.push(...unplacedCalls);
    callsBySlice.set(lastSlice, bucket);
  }

  const standaloneGroups: GroupDraft[] = [];
  let sideOrdinal = 0;
  const turnDrafts: TurnDraft[] = slices.map(slice => ({
    turn: slice.turn,
    startMs: slice.startMs,
    endMs: slice.endMs,
    groups: [],
  }));

  for (let sliceIndex = 0; sliceIndex < slices.length; sliceIndex++) {
    const slice = slices[sliceIndex];
    const draft = turnDrafts[sliceIndex];
    if (slice === undefined || draft === undefined) continue;
    const sliceCalls = callsBySlice.get(slice) ?? [];
    const sliceAssistantMessages = slice.messages.filter(message => message.role === 'assistant');
    const pairings = pairAssistantCalls(sliceAssistantMessages, sliceCalls);
    const consumedCallIds = new Set<string>();
    let step = 0;
    for (const message of slice.messages) {
      if (message.role === 'user') {
        pushCell(draft, 'Message', {
          absTime: message.timestampMs,
          cell: userCell(message),
        });
        continue;
      }
      step += 1;
      const groupTitle = `Step ${step}`;
      const linkedCall = pairings.get(message);
      if (linkedCall !== undefined) consumedCallIds.add(linkedCall.callId);
      pushCell(draft, groupTitle, {
        absTime: message.timestampMs,
        cell: assistantMessageCell(message, linkedCall),
      });
      for (const toolCall of message.toolCalls ?? []) {
        pushCell(draft, groupTitle, {
          absTime: null,
          toolName: toolCall.name,
          cell: sessionToolCell(message, toolCall),
        });
      }
      if (linkedCall !== undefined) {
        appendChildCallCells(draft, groupTitle, linkedCall.callId, childrenByParent, consumedCallIds);
      }
    }
    // 未配对主链调用二分：发生在轮内最后一条助手消息之后的（标题/压缩）
    // → Between turns 独立区段；之前的（知识滚动等轮内侧线）→ 轮内新 Step。
    for (const call of sliceCalls) {
      if (consumedCallIds.has(call.callId)) continue;
      const start = callStartMs(call);
      const afterTurn = slice.lastAssistantTs !== null
        && start !== null
        && start > slice.lastAssistantTs;
      consumedCallIds.add(call.callId);
      if (afterTurn) {
        sideOrdinal += 1;
        const sideGroup: GroupDraft = { title: `Side ${sideOrdinal}`, laid: [] };
        sideGroup.laid.push({
          absTime: start,
          cell: modelCallCell(call),
        });
        appendChildCallCellsToGroup(sideGroup, call.callId, childrenByParent, consumedCallIds);
        standaloneGroups.push(sideGroup);
        continue;
      }
      step += 1;
      const groupTitle = `Step ${step}`;
      pushCell(draft, groupTitle, {
        absTime: start,
        cell: modelCallCell(call),
      });
      appendChildCallCells(draft, groupTitle, call.callId, childrenByParent, consumedCallIds);
    }
  }

  prependSystemCell(turnDrafts, mainCalls[0], promptSnapshot, toolNames);
  const turns = finalizeTurns(turnDrafts, standaloneGroups);
  return {
    turns,
    requestNumbers: buildRequestNumbers(turns),
    sessionId,
    sessionJoined: true,
  };
}

/**
 * 助手消息 ↔ 主链调用配对：助手消息 entry 写入时刻紧随其调用的结束——
 * 给「结束于消息时刻之前且最近」的未用调用打最低分；调用结束晚于消息
 * 时刻的加罚项（更不可能是该消息的来源）。±90s 容差。
 */
function pairAssistantCalls(
  assistants: readonly ParsedSessionMessage[],
  calls: readonly ModelObservabilityCallListItem[],
): Map<ParsedSessionMessage, ModelObservabilityCallListItem> {
  const pairings = new Map<ParsedSessionMessage, ModelObservabilityCallListItem>();
  const used = new Set<string>();
  const assistantsWithTime = assistants.filter(message => message.timestampMs !== null);
  for (const message of assistantsWithTime) {
    const ts = message.timestampMs;
    if (ts === null) continue;
    let best: ModelObservabilityCallListItem | null = null;
    let bestScore = Number.POSITIVE_INFINITY;
    for (const call of calls) {
      if (used.has(call.callId)) continue;
      const start = callStartMs(call);
      const end = callEndMs(call) ?? start;
      if (start === null || end === null) continue;
      const score = end <= ts ? ts - end : end - ts + 60_000;
      if (score < bestScore) {
        best = call;
        bestScore = score;
      }
    }
    if (best !== null && bestScore <= 90_000) {
      pairings.set(message, best);
      used.add(best.callId);
    }
  }
  // 无时间戳的助手消息按剩余顺序配对（数量兜底）。
  const remaining = calls.filter(call => !used.has(call.callId));
  let remainingIndex = 0;
  for (const message of assistants) {
    if (message.timestampMs !== null || pairings.has(message)) continue;
    const call = remaining[remainingIndex];
    remainingIndex += 1;
    if (call === undefined) break;
    pairings.set(message, call);
    used.add(call.callId);
  }
  return pairings;
}

function appendChildCallCells(
  draft: TurnDraft,
  groupTitle: string,
  parentCallId: string,
  childrenByParent: ReadonlyMap<string, ModelObservabilityCallListItem[]>,
  consumedCallIds: Set<string>,
): void {
  const group = draft.groups.find(candidate => candidate.title === groupTitle);
  if (group === undefined) return;
  appendChildCallCellsToGroup(group, parentCallId, childrenByParent, consumedCallIds);
}

function appendChildCallCellsToGroup(
  group: GroupDraft,
  parentCallId: string,
  childrenByParent: ReadonlyMap<string, ModelObservabilityCallListItem[]>,
  consumedCallIds: Set<string>,
): void {
  const stack = [...(childrenByParent.get(parentCallId) ?? [])];
  while (stack.length > 0) {
    const call = stack.shift();
    if (call === undefined || consumedCallIds.has(call.callId)) continue;
    consumedCallIds.add(call.callId);
    group.laid.push({
      absTime: callStartMs(call),
      cell: modelCallCell(call, 'subtool'),
    });
    stack.push(...(childrenByParent.get(call.callId) ?? []));
  }
}

/* ── calls-only 模式（无会话数据）────────────────────────────────────── */

function buildCallsOnlyModel(
  mainCalls: readonly ModelObservabilityCallListItem[],
  childrenByParent: ReadonlyMap<string, ModelObservabilityCallListItem[]>,
  promptSnapshot: SessionPromptSnapshotInput | null,
  toolNames: readonly string[] | null,
): TraceConversationModel {
  const draft: TurnDraft = { turn: 1, startMs: null, endMs: null, groups: [] };
  const consumed = new Set<string>();
  mainCalls.forEach((call, position) => {
    const groupTitle = `Step ${position + 1}`;
    const group: GroupDraft = { title: groupTitle, laid: [] };
    group.laid.push({
      absTime: callStartMs(call),
      cell: modelCallCell(call),
    });
    consumed.add(call.callId);
    appendChildCallCellsToGroup(group, call.callId, childrenByParent, consumed);
    draft.groups.push(group);
  });
  prependSystemCell([draft], mainCalls[0], promptSnapshot, toolNames);
  const turns = finalizeTurns([draft], []);
  return {
    turns,
    requestNumbers: buildRequestNumbers(turns),
    sessionId: null,
    sessionJoined: false,
  };
}


/**
 * SYSTEM 首记录（dsh 首屏对齐）：text 为 i18n 键尾段，渲染层翻译。
 * 会话提示词快照可用时直接装配 promptDetail（System Prompt/Tools tab 即刻
 * 有真实内容，无需开载荷捕获）；不可用时留 observabilityCallId 供检查器
 * 懒加载 semantic_request 载荷兜底。
 */
function systemCell(
  firstMainCall: ModelObservabilityCallListItem | undefined,
  promptSnapshot: SessionPromptSnapshotInput | null,
  toolNames: readonly string[] | null,
): TrajectoryCellProps {
  const system = promptSnapshot === null
    ? null
    : promptSnapshot.finalSystemPrompt
      ?? [promptSnapshot.systemPrompt ?? '', ...(promptSnapshot.appendSystemPrompt ?? [])]
        .filter(part => typeof part === 'string' && part !== '')
        .join('\n\n');
  const hasSystem = typeof system === 'string' && system !== '';
  const tools = toolNames === null
    ? null
    : toolNames.map(name => ({ name, description: '', parameters: {} }));
  const promptDetail = hasSystem === false && tools === null
    ? undefined
    : {
      system: hasSystem ? system! : '',
      tools: tools ?? [],
    };
  return {
    index: nextIndex(),
    recordId: 'system\u0000initial',
    kind: 'system',
    text: 'systemPromptInitial',
    ...(promptDetail !== undefined ? { promptDetail } : {}),
    ...(firstMainCall !== undefined ? { observabilityCallId: firstMainCall.callId } : {}),
    timeSeconds: 0,
    startedAt: firstMainCall !== undefined ? callStartMs(firstMainCall) : null,
  };
}

function prependSystemCell(
  drafts: TurnDraft[],
  firstMainCall: ModelObservabilityCallListItem | undefined,
  promptSnapshot: SessionPromptSnapshotInput | null,
  toolNames: readonly string[] | null,
): void {
  const first = drafts[0];
  if (first === undefined) return;
  const cell = systemCell(firstMainCall, promptSnapshot, toolNames);
  const group = first.groups[0];
  if (group === undefined) {
    first.groups.push({ title: 'Message', laid: [{ absTime: null, cell }] });
    return;
  }
  group.laid.unshift({ absTime: null, cell });
}

/* ── cell 构造 ────────────────────────────────────────────────────────── */

function userCell(message: ParsedSessionMessage): TrajectoryCellProps {
  const text = message.displayText ?? message.content;
  const sourceBlocks: TrajectorySourceBlock[] = [];
  if (text !== '') sourceBlocks.push({ type: 'text', content: text });
  for (const image of message.images ?? []) {
    sourceBlocks.push({
      type: 'image',
      content: '',
      imageSrc: `data:${image.mimeType};base64,${image.data}`,
    });
  }
  return {
    index: nextIndex(),
    recordId: `user\u0000${message.entryId ?? `idx\u0000${message.timestampMs ?? 'x'}`}`,
    kind: 'user',
    text: '',
    ...(text !== '' ? { previewMarkdown: text } : {}),
    inputDetail: text,
    ...(sourceBlocks.length > 0 ? { sourceBlocks } : {}),
    messageSource: { kind: 'user' },
    opensTurn: true,
    timeSeconds: 0,
    startedAt: message.timestampMs,
  };
}

function assistantMessageCell(
  message: ParsedSessionMessage,
  linkedCall: ModelObservabilityCallListItem | undefined,
): TrajectoryCellProps {
  const callStart = linkedCall !== undefined ? callStartMs(linkedCall) : null;
  const callEnd = linkedCall !== undefined ? callEndMs(linkedCall) : null;
  const duration = linkedCall !== undefined ? durationSecondsBetween(callEnd, callStart) : null;
  const usage = linkedCall !== undefined ? usageFromSummary(linkedCall.usage?.summary) : undefined;
  const cell: TrajectoryCellProps = {
    index: nextIndex(),
    recordId: `assistant\u0000${message.entryId ?? `idx\u0000${message.timestampMs ?? 'x'}`}`,
    kind: 'message',
    text: message.content !== '' || message.thinking ? '' : 'No content',
    ...(message.content !== ''
      ? { previewMarkdown: message.content, outputDetail: message.content }
      : {}),
    ...(message.thinking ? { thinkingDetail: message.thinking } : {}),
    sourceBlocks: assistantSourceBlocks(message),
    timeSeconds: duration,
    startedAt: callStart ?? message.timestampMs,
    ...(linkedCall !== undefined
      ? { observabilityCallId: linkedCall.callId, callId: linkedCall.callId, timingSource: 'observability' as const }
      : { timingSource: 'session' as const }),
  };
  if (usage !== undefined) {
    if (usage.input !== undefined) cell.input = usage.input;
    if (usage.cacheRead !== undefined) cell.cacheRead = usage.cacheRead;
    if (usage.cacheWrite !== undefined) cell.cacheWrite = usage.cacheWrite;
    if (usage.output !== undefined) cell.output = usage.output;
    if (usage.reasoning !== undefined) cell.think = usage.reasoning;
  }
  cell.assistantMetrics = {
    timingRecorded: linkedCall !== undefined,
    stepStartTime: callStart,
    // 无首 token 事实——不虚构 TTFT，检查器与时间线如实显示不可用。
    firstTokenTime: null,
    completedTime: callEnd ?? message.timestampMs,
    usageProvided: usage !== undefined,
    outputTokens: usage?.output ?? null,
  };
  if (
    message.turnStatus === 'failed'
    || message.turnStatus === 'aborted'
    || (linkedCall !== undefined && callStatus(linkedCall) === 'error')
  ) {
    cell.isError = true;
  }
  return cell;
}

function assistantSourceBlocks(message: ParsedSessionMessage): TrajectorySourceBlock[] {
  const blocks: TrajectorySourceBlock[] = [];
  if (message.thinking) blocks.push({ type: 'thinking', content: message.thinking });
  if (message.content !== '') blocks.push({ type: 'text', content: message.content });
  for (const toolCall of message.toolCalls ?? []) {
    blocks.push({
      type: 'tool-call',
      content: toolCall.args ?? '',
      callId: toolCall.id,
      toolName: toolCall.name,
    });
  }
  return blocks;
}

function sessionToolCell(
  message: ParsedSessionMessage,
  toolCall: SessionToolCallProjection,
): TrajectoryCellProps {
  const output = typeof toolCall.details?.output === 'string' ? toolCall.details.output : undefined;
  const failed = toolCall.success === false
    || toolCall.status === 'error'
    || toolCall.status === 'failed';
  const errorText = toolCall.error !== undefined && toolCall.error !== null
    ? typeof toolCall.error === 'string'
      ? toolCall.error
      : JSON.stringify(toolCall.error)
    : undefined;
  // 计时：会话条目时间戳（assistant 条目时间 → toolResult 条目时间）。
  const startedAt = epochMs((toolCall as { startedAt?: unknown }).startedAt);
  const endedAt = epochMs((toolCall as { endedAt?: unknown }).endedAt);
  const duration = durationSecondsBetween(endedAt, startedAt);
  return {
    index: nextIndex(),
    recordId: `tool\u0000${toolCall.id ?? `${message.entryId ?? 'x'}\u0000${toolCall.name}`}`,
    kind: 'tool',
    text: toolCall.name,
    ...(toolCall.args !== undefined ? { previewMarkdown: toolCall.args, inputDetail: toolCall.args } : {}),
    ...(output !== undefined
      ? {
        outputDetail: output,
        resultPreviewMarkdown: output,
        outputBlocks: [{ type: 'text', content: output }],
      }
      // 无输出的成功工具是终态（历史记录，非在途）——outputDetail 置空串
      // 让 stateOf 判 complete，避免误标「等待中」。
      : failed
        ? { result: errorText ?? 'error' }
        : { result: 'No output', outputDetail: '' }),
    callId: toolCall.id,
    isError: failed || undefined,
    timeSeconds: duration,
    ...(startedAt !== null ? { startedAt } : {}),
    timingSource: 'session',
  };
}

function modelCallCell(
  call: ModelObservabilityCallListItem,
  kind: 'message' | 'subtool' = 'message',
): TrajectoryCellProps {
  const start = callStartMs(call);
  const end = callEndMs(call);
  const usage = usageFromSummary(call.usage?.summary);
  const cell: TrajectoryCellProps = {
    index: nextIndex(),
    recordId: `call\u0000${call.callId}`,
    kind,
    text: callLabel(call),
    callId: call.callId,
    observabilityCallId: call.callId,
    sourceBlocks: [{
      type: 'text',
      content: `${call.model?.provider ?? ''}/${call.model?.modelId ?? ''} · ${callLabel(call)}`,
    }],
    timeSeconds: durationSecondsBetween(end, start),
    startedAt: start,
    isError: callStatus(call) === 'error' || undefined,
    timingSource: 'observability',
  };
  if (usage !== undefined) {
    if (usage.input !== undefined) cell.input = usage.input;
    if (usage.cacheRead !== undefined) cell.cacheRead = usage.cacheRead;
    if (usage.cacheWrite !== undefined) cell.cacheWrite = usage.cacheWrite;
    if (usage.output !== undefined) cell.output = usage.output;
    if (usage.reasoning !== undefined) cell.think = usage.reasoning;
  }
  if (kind === 'message') {
    cell.assistantMetrics = {
      timingRecorded: true,
      stepStartTime: start,
      firstTokenTime: null,
      completedTime: end,
      usageProvided: usage !== undefined,
      outputTokens: usage?.output ?? null,
    };
  }
  return cell;
}

/* ── 请求编号（台账请求边界圆点 + 检查器请求面板数据）────────────────── */

function buildRequestNumbers(
  turns: readonly TrajectoryTurnModel[],
): readonly TrajectoryRequestNumber[] {
  const numbered: TrajectoryRequestNumber[] = [];
  let cumulative: TrajectoryUsage | undefined;
  let seq = 0;
  for (const turn of turns) {
    for (const group of turn.groups) {
      const callCell = group.cells.find(
        cell => typeof cell.observabilityCallId === 'string'
          && cell.kind !== 'subtool'
          && cell.kind !== 'system',
      );
      if (callCell === undefined || callCell.observabilityCallId === undefined) continue;
      seq += 1;
      const usage = callUsageOf(callCell);
      cumulative = addUsage(cumulative, usage);
      const step = stepOfGroupTitle(group.title);
      const betweenTurns = turn.turn === null;
      numbered.push({
        seq,
        group: group.title,
        number: seq,
        ...(callCell.isError === true ? { status: 'error' as const } : {}),
        ...(callCell.startedAt !== null && callCell.startedAt !== undefined
          ? { startedAt: callCell.startedAt }
          : {}),
        ...(callCell.assistantMetrics?.completedTime != null
          ? { completedAt: callCell.assistantMetrics.completedTime }
          : { completedAt: null }),
        ...(usage !== undefined ? { usage } : {}),
        ...(cumulative !== undefined ? { cumulativeUsage: cumulative } : {}),
        ...(betweenTurns
          ? { purpose: 'side' as const, turn: null, step: 0 }
          : { turn: turn.turn ?? 1, step: step ?? 1 }),
      });
    }
  }
  return numbered;
}

function stepOfGroupTitle(title: string): number | undefined {
  if (!title.startsWith('Step ')) return undefined;
  const value = Number(title.slice('Step '.length));
  return Number.isInteger(value) && value > 0 ? value : undefined;
}

function callUsageOf(cell: TrajectoryCellProps): TrajectoryUsage | undefined {
  const usage: TrajectoryUsage = {};
  if (cell.input !== undefined) usage.input = cell.input;
  if (cell.cacheRead !== undefined) usage.cacheRead = cell.cacheRead;
  if (cell.cacheWrite !== undefined) usage.cacheWrite = cell.cacheWrite;
  if (cell.output !== undefined) usage.output = cell.output;
  if (cell.think !== undefined) usage.reasoning = cell.think;
  return Object.keys(usage).length === 0 ? undefined : usage;
}

function addUsage(
  total: TrajectoryUsage | undefined,
  usage: TrajectoryUsage | undefined,
): TrajectoryUsage | undefined {
  if (usage === undefined) return total;
  return {
    ...(total?.input === undefined && usage.input === undefined
      ? {}
      : { input: (total?.input ?? 0) + (usage.input ?? 0) }),
    ...(total?.cacheRead === undefined && usage.cacheRead === undefined
      ? {}
      : { cacheRead: (total?.cacheRead ?? 0) + (usage.cacheRead ?? 0) }),
    ...(total?.cacheWrite === undefined && usage.cacheWrite === undefined
      ? {}
      : { cacheWrite: (total?.cacheWrite ?? 0) + (usage.cacheWrite ?? 0) }),
    ...(total?.output === undefined && usage.output === undefined
      ? {}
      : { output: (total?.output ?? 0) + (usage.output ?? 0) }),
    ...(total?.reasoning === undefined && usage.reasoning === undefined
      ? {}
      : { reasoning: (total?.reasoning ?? 0) + (usage.reasoning ?? 0) }),
  };
}
