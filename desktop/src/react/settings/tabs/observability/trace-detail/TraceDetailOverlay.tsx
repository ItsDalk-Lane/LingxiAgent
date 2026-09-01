/**
 * TraceDetailOverlay.tsx — 轨迹详情全屏层（dsh ui-trajectory 布局 + 灵犀独有头部）。
 *
 * 视图编排移植自 dsh TrajectoryView（状态：折叠集合/时间线选区/搜索索引/
 * Duration 切换），按静态历史数据简化（无流式 partial、无向前补页）。
 * 灵犀独有合并：
 *   - 头部摘要行：origin / Trace ID / 时间范围 / 调用数 / 成功·失败·中止 /
 *     usage 汇总 / 载荷完整度 / 会话 join 状态；
 *   - 检查器「观测载荷」tab（TrajectoryTable 内实现）。
 *
 * 正文双通道：会话消息按 calls 的 sessionId 多数票反查
 * GET /api/sessions/messages（失败 → calls-only 降级，界面显式标注）。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Overlay } from '../../../../ui';
import { t } from '../../../helpers';
import {
  isObservabilityAbortError,
  loadObservabilityTraceDetail,
  observabilityRequest,
} from '../model-observability-actions';
import { formatCompactNumber } from '../model-observability-format';
import { sourceIdentityKindLabel, sourceIdentityTitle } from '../model-observability-labels';
import type {
  ModelObservabilityTraceDetail,
  ModelObservabilityUsageSummary,
} from '../../../../../../../shared/model-observability-api-contract.ts';
import {
  buildTraceConversationModel,
  type SessionPromptSnapshotInput,
} from './trace-conversation-model';
import {
  TrajectoryTable,
  type TrajectoryRequestNumber,
} from './TrajectoryTable';
import { TrajectoryToolbar } from './TrajectoryToolbar';
import { TrajectoryTimeline } from './TrajectoryTimeline';
import { trajectoryRecordId } from './trajectory-record';
import { TrajectorySearchIndex } from './trajectory-search-index';
import {
  trajectoryTimelineFocusIndexes,
  type TrajectoryTimelineMode,
  type TrajectoryTimeRange,
} from './timeline';
import css from './TraceDetailOverlay.module.css';
import viewCss from './TrajectoryView.module.css';

const tr = (key: string): string => t(`settings.observability.traceDetail.${key}`);

const EMPTY_TURN_IDS: ReadonlySet<number> = new Set();
const EMPTY_RECORD_IDS: ReadonlySet<string> = new Set();

/* ── 会话消息拉取（观测连接面复用；失败 = calls-only 降级，不炸视图）──── */

async function fetchSessionMessages(
  sessionId: string,
  signal?: AbortSignal | null,
): Promise<unknown> {
  const res = await observabilityRequest(
    `/api/sessions/messages?sessionId=${encodeURIComponent(sessionId)}&all=1`,
    { signal, timeout: 30_000 },
  );
  const data = await res.json() as { messages?: unknown };
  return data?.messages ?? null;
}

/**
 * 会话冻结提示词快照（session-meta.json sidecar）——SYSTEM 首记录在未开启
 * 载荷捕获时的系统提示词来源。失败 → null（SYSTEM 记录退回载荷懒加载/未捕获态）。
 */
async function fetchSessionPromptContext(
  sessionId: string,
  signal?: AbortSignal | null,
): Promise<{ promptSnapshot: SessionPromptSnapshotInput | null; toolNames: string[] | null } | null> {
  try {
    const res = await observabilityRequest(
      `/api/sessions/prompt-snapshot?sessionId=${encodeURIComponent(sessionId)}`,
      { signal, timeout: 15_000 },
    );
    const data = await res.json() as {
      promptSnapshot?: SessionPromptSnapshotInput | null;
      toolNames?: string[] | null;
    };
    return {
      promptSnapshot: data?.promptSnapshot ?? null,
      toolNames: Array.isArray(data?.toolNames) ? data!.toolNames! : null,
    };
  } catch {
    return null;
  }
}

function formatUsageSummary(summary: ModelObservabilityUsageSummary | null): string {
  if (!summary) return '—';
  const parts: string[] = [];
  if (typeof summary.totalTokens === 'number') {
    parts.push(`${formatCompactNumber(summary.totalTokens)} tok`);
  } else if (
    typeof summary.inputTokens === 'number'
    || typeof summary.outputTokens === 'number'
  ) {
    const input = summary.inputTokens ?? 0;
    const output = summary.outputTokens ?? 0;
    parts.push(`${formatCompactNumber(input + output)} tok`);
  }
  if (typeof summary.costTotal === 'number' && summary.costTotal > 0) {
    parts.push(`$${summary.costTotal.toFixed(4)}`);
  }
  return parts.length > 0 ? parts.join(' · ') : '—';
}

export interface TraceDetailOverlayProps {
  traceId: string;
  onClose: () => void;
  isLocalOwner?: boolean;
  /** 一次性 inspect：打开后自动选中并滚到该 call 的记录行。 */
  inspectCallId?: string | null;
  onInspectApplied?: (() => void) | undefined;
}

export function TraceDetailOverlay({
  traceId,
  onClose,
  isLocalOwner = false,
  inspectCallId = null,
  onInspectApplied,
}: TraceDetailOverlayProps) {
  const [detail, setDetail] = useState<ModelObservabilityTraceDetail | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [sessionRaw, setSessionRaw] = useState<unknown>(null);
  const [promptContext, setPromptContext] = useState<{
    promptSnapshot: SessionPromptSnapshotInput | null;
    toolNames: string[] | null;
  } | null>(null);
  const [collapsedTurns, setCollapsedTurns] = useState<ReadonlySet<number>>(EMPTY_TURN_IDS);
  const [collapsedAssistants, setCollapsedAssistants] =
    useState<ReadonlySet<string>>(EMPTY_RECORD_IDS);
  const [timelineSelection, setTimelineSelection] = useState<TrajectoryTimeRange | null>(null);
  const [actualDuration, setActualDuration] = useState(true);
  const [actualTime, setActualTime] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedTimelineIndex, setSelectedTimelineIndex] = useState<number | null>(null);
  const [timelineRecordSelection, setTimelineRecordSelection] = useState<{
    readonly index: number;
  } | null>(null);
  const [timelineRecordFocus, setTimelineRecordFocus] = useState<{
    readonly index: number;
  } | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    setDetail(null);
    setLoadError(null);
    setSessionRaw(null);
    setPromptContext(null);
    loadObservabilityTraceDetail(traceId, { signal: controller.signal })
      .then(next => {
        setDetail(next);
        const sessionId = next.calls.length > 0
          ? majoritySessionId(next)
          : null;
        if (sessionId === null) return;
        // 会话消息失败不阻塞轨迹视图——calls-only 降级。
        fetchSessionMessages(sessionId, controller.signal)
          .then(messages => { setSessionRaw(messages) })
          .catch(() => { setSessionRaw(null) });
        // 提示词快照（SYSTEM 首记录正文来源，未捕获载荷时的兜底）。
        fetchSessionPromptContext(sessionId, controller.signal)
          .then(context => { setPromptContext(context) });
      })
      .catch((error: unknown) => {
        if (isObservabilityAbortError(error)) return;
        setLoadError(error instanceof Error ? error.message : String(error));
      });
    return () => { controller.abort() };
  }, [traceId]);

  const model = useMemo(
    () => (detail === null
      ? null
      : buildTraceConversationModel(detail, sessionRaw, promptContext?.promptSnapshot ?? null, promptContext?.toolNames ?? null)),
    [detail, sessionRaw, promptContext],
  );
  const turns = model?.turns ?? [];
  const requestNumbers = useMemo(
    () => model?.requestNumbers ?? [] as readonly TrajectoryRequestNumber[],
    [model],
  );

  const [searchIndex] = useState(() => new TrajectorySearchIndex());
  const searchLayouts = useMemo(() => [turns] as const, [turns]);
  const searchMatchRecordIds = useMemo(() => {
    searchIndex.update(searchLayouts);
    return searchIndex.search(searchQuery);
  }, [searchIndex, searchLayouts, searchQuery]);
  const searchMatchIndexes = useMemo(() => {
    if (searchMatchRecordIds === null) return null;
    const indexes = new Set<number>();
    for (const turn of turns) {
      for (const group of turn.groups) {
        for (const cell of group.cells) {
          if (searchMatchRecordIds.has(trajectoryRecordId(cell))) indexes.add(cell.index);
        }
      }
    }
    return indexes;
  }, [turns, searchMatchRecordIds]);

  const timelineMode: TrajectoryTimelineMode = actualDuration
    ? actualTime ? 'actual' : 'duration'
    : actualTime ? 'time' : 'sequence';
  const timelineRange = timelineSelection;
  const timelineFocusIndexes = useMemo(
    () => timelineRange === null
      ? null
      : trajectoryTimelineFocusIndexes(turns, timelineRange, timelineMode),
    [timelineMode, timelineRange, turns],
  );

  const handleTimelineRecordSelect = useCallback((index: number) => {
    setTimelineSelection(null);
    setTimelineRecordSelection({ index });
    setSelectedTimelineIndex(index);
  }, []);

  const handleRecordSelect = useCallback((index: number) => {
    if (timelineFocusIndexes !== null && !timelineFocusIndexes.has(index)) {
      setTimelineSelection(null);
    }
  }, [timelineFocusIndexes]);

  const collapsibleTurnIds = useMemo(
    () => turns
      .filter(turn =>
        turn.turn !== null
        && turn.groups.reduce(
          (count, group) =>
            count + group.cells.filter(cell =>
              cell.requestOnly !== true && cell.kind !== 'system').length,
          0,
        ) > 1)
      .flatMap(turn => turn.turn === null ? [] : [turn.turn]),
    [turns],
  );
  const allTurnsCollapsed = collapsibleTurnIds.length > 0
    && collapsibleTurnIds.every(turn => collapsedTurns.has(turn));
  const collapsibleAssistantIds = useMemo(() => {
    const ids: string[] = [];
    for (const turn of turns) {
      const cells = turn.groups.flatMap(group => group.cells);
      for (let i = 0; i < cells.length; i++) {
        const cell = cells[i];
        if (cell?.kind !== 'message') continue;
        const next = cells[i + 1];
        if (next?.kind === 'tool' || next?.kind === 'subtool') {
          ids.push(trajectoryRecordId(cell));
        }
      }
    }
    return ids;
  }, [turns]);
  const allAssistantsCollapsed = collapsibleAssistantIds.length > 0
    && collapsibleAssistantIds.every(id => collapsedAssistants.has(id));

  const toggleTurn = (turn: number) => {
    setCollapsedTurns((current) => {
      const collapsed = new Set(current);
      if (collapsed.has(turn)) collapsed.delete(turn);
      else collapsed.add(turn);
      return collapsed;
    });
  };
  const toggleAllTurns = () => {
    setCollapsedTurns((current) => {
      const collapsed = new Set(current);
      if (allTurnsCollapsed) {
        for (const turn of collapsibleTurnIds) collapsed.delete(turn);
      } else {
        for (const turn of collapsibleTurnIds) collapsed.add(turn);
      }
      return collapsed;
    });
  };
  const toggleAssistant = (id: string) => {
    setCollapsedAssistants((current) => {
      const collapsed = new Set(current);
      if (collapsed.has(id)) collapsed.delete(id);
      else collapsed.add(id);
      return collapsed;
    });
  };
  const toggleAllAssistants = () => {
    setCollapsedAssistants((current) => {
      const collapsed = new Set(current);
      if (allAssistantsCollapsed) {
        for (const id of collapsibleAssistantIds) collapsed.delete(id);
      } else {
        for (const id of collapsibleAssistantIds) collapsed.add(id);
      }
      return collapsed;
    });
  };

  const historyLoading = detail === null && loadError === null;

  const usage = detail?.usageAggregate.summary ?? null;
  const payload = detail?.payloadCompleteness ?? null;
  const payloadNote = payload === null
    ? ''
    : payload.present > 0
      ? tr('header.payloadPresent').replace('{n}', String(payload.present))
      : payload.notCaptured > 0
        ? tr('header.payloadNotCaptured')
        : '';

  return (
    <Overlay
      open
      scope="inline"
      backdrop="none"
      closeOnEsc
      trapFocus
      className={css.layer}
      onClose={onClose}
    >
      <div className={css.header}>
          <span className={css.headerTitle}>
            {detail ? sourceIdentityTitle(detail.trace.sourceIdentity) : '…'}
            {detail ? ` · ${sourceIdentityKindLabel(detail.trace.sourceIdentity?.kind ?? 'unknown')}` : ''}
            <code className={css.headerId} title={detail?.trace.traceId ?? ''}>
              {detail?.trace.traceId ?? tr('header.loading')}
            </code>
          </span>
          {detail && (
            <>
              <span className={css.headerStat}>
                {tr('header.calls')}
                <span className={css.headerStatValue}>{formatCompactNumber(detail.trace.callCount)}</span>
              </span>
              <span className={css.headerStat}>
                {tr('header.ok')}
                <span className={css.headerStatValue}>{formatCompactNumber(detail.trace.terminalOk)}</span>
              </span>
              {detail.trace.terminalError > 0 && (
                <span className={css.headerStat}>
                  {tr('header.error')}
                  <span className={css.headerStatValue} data-status="error">
                    {formatCompactNumber(detail.trace.terminalError)}
                  </span>
                </span>
              )}
              {detail.trace.terminalAborted > 0 && (
                <span className={css.headerStat}>
                  {tr('header.aborted')}
                  <span className={css.headerStatValue} data-status="aborted">
                    {formatCompactNumber(detail.trace.terminalAborted)}
                  </span>
                </span>
              )}
              <span className={css.headerStat}>
                {tr('header.usage')}
                <span className={css.headerStatValue}>{formatUsageSummary(usage)}</span>
              </span>
              {payloadNote !== '' && (
                <span className={css.headerStat}>
                  <span className={css.headerStatValue}>{payloadNote}</span>
                </span>
              )}
              <span className={css.headerStat} title={model?.sessionId ?? undefined}>
                {tr('header.session')}
                <span className={css.headerStatValue}>
                  {model === null ? '…' : model.sessionJoined ? tr('header.sessionJoined') : tr('header.sessionNotJoined')}
                </span>
              </span>
            </>
          )}
          <button
            type="button"
            className={css.headerClose}
            aria-label={tr('header.close')}
            onClick={onClose}
          >
            ×
          </button>
        </div>
        <div className={css.headerBody}>
          {loadError !== null ? (
            <div className="observability-error-detail" role="alert">{loadError}</div>
          ) : (
            <div className={viewCss.root}>
              <TrajectoryToolbar
                actualDuration={actualDuration}
                onActualDurationChange={(next) => {
                  setActualDuration(next);
                  setTimelineSelection(null);
                }}
                actualTime={actualTime}
                onActualTimeChange={(next) => {
                  setActualTime(next);
                  setTimelineSelection(null);
                }}
                allTurnsCollapsed={allTurnsCollapsed}
                onToggleAllTurns={toggleAllTurns}
                allAssistantsCollapsed={allAssistantsCollapsed}
                onToggleAllAssistants={toggleAllAssistants}
                searchQuery={searchQuery}
                onSearchQueryChange={setSearchQuery}
              />
              <TrajectoryTimeline
                turns={turns}
                mode={timelineMode}
                range={timelineRange}
                hasEarlierRecords={false}
                selectedIndex={selectedTimelineIndex}
                searchMatchIndexes={searchMatchIndexes}
                onRangeChange={setTimelineSelection}
                onRecordSelect={handleTimelineRecordSelect}
                onRecordFocus={(index) => { setTimelineRecordFocus({ index }) }}
              />
              <div className={viewCss.ledger}>
                <TrajectoryTable
                  requestNumbers={requestNumbers}
                  turns={turns}
                  timelineFocusIndexes={timelineFocusIndexes}
                  searchMatchIndexes={searchMatchIndexes}
                  onSelectedIndexChange={setSelectedTimelineIndex}
                  onRecordSelect={handleRecordSelect}
                  recordSelection={timelineRecordSelection}
                  recordFocus={timelineRecordFocus}
                  historyLoading={historyLoading}
                  collapsedTurns={collapsedTurns}
                  onToggleTurn={toggleTurn}
                  collapsedAssistants={collapsedAssistants}
                  onToggleAssistant={toggleAssistant}
                  inspectCallId={inspectCallId}
                  onInspectApplied={onInspectApplied}
                  isLocalOwner={isLocalOwner}
                />
              </div>
            </div>
          )}
        </div>
    </Overlay>
  );
}

function majoritySessionId(detail: ModelObservabilityTraceDetail): string | null {
  const votes = new Map<string, number>();
  for (const call of detail.calls) {
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
