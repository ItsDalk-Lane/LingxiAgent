/**
 * ObservabilityTraceExplorer.tsx — Trace Explorer（Phase 9 §八十七～九十五）。
 *
 *   - 垂直树/嵌套列表（§八十七：不要 D3 force graph）。
 *   - 图由后端 roots/edges/orphanEdges 构建（§八十九：不用数组顺序猜父子）。
 *   - orphan parent 标「Missing parent」（§九十）；graphIntegrity=degraded →
 *     警告但不崩（§九十一）；前端 visited-set 环保护（§九十二：不管后端
 *     是否已防御，UI 递归必须有 visited）。
 *   - trace 节点 → Call Inspector；Trace→Call→Trace 导航保上下文（§九十三）。
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type {
  ModelObservabilityCallListItem,
  ModelObservabilityTraceDetail,
  ModelObservabilityTraceListItem,
  ModelObservabilityTracePage,
} from '../../../../../../shared/model-observability-api-contract.ts';
import { MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT } from '../../../../../../shared/model-observability-api-contract.ts';
import { t } from '../../helpers';
import { Button, Overlay } from '../../../ui';
import styles from '../../Settings.module.css';
import {
  isObservabilityAbortError,
  loadObservabilityTraceDetail,
  ModelObservabilityRequestError,
  queryObservabilityTraces,
} from './model-observability-actions';
import { buildCallFilterInput, type ObservabilityFilterState } from './model-observability-filter';
import {
  formatCompactNumber,
  formatCost,
  formatDurationMs,
  formatLocalDateTime,
  formatNumber,
  isoTooltip,
  shortId,
} from './model-observability-format';
import { originLabel, terminalStatusLabel } from './model-observability-labels';

/* ── 树构建（纯函数，§八十九/§九十二；测试直接锁定）────────────────────── */

const TRACE_CHART_HEIGHT = 180;
const TRACE_CHART_PAD_X = 10;
const TRACE_CHART_PAD_Y = 14;

export type TraceTreeNode = {
  callId: string;
  call: ModelObservabilityCallListItem | null;
  /** 合成节点：缺失的 parent（§九十）。 */
  missingParent: boolean;
  children: TraceTreeNode[];
  /** 递归中被 visited 截断（环，§九十二）。 */
  cycle: boolean;
};

export function buildTraceForest(detail: ModelObservabilityTraceDetail): TraceTreeNode[] {
  const callById = new Map(detail.calls.map((call) => [call.callId, call]));
  const childIds = new Map<string, string[]>();
  for (const edge of detail.edges) {
    const list = childIds.get(edge.parentCallId) ?? [];
    list.push(edge.childCallId);
    childIds.set(edge.parentCallId, list);
  }

  const makeNode = (callId: string, missingParent: boolean, visited: Set<string>): TraceTreeNode => {
    if (visited.has(callId)) {
      return { callId, call: callById.get(callId) ?? null, missingParent, children: [], cycle: true };
    }
    const nextVisited = new Set(visited);
    nextVisited.add(callId);
    const children = (childIds.get(callId) ?? []).map((childId) => makeNode(childId, false, nextVisited));
    return { callId, call: callById.get(callId) ?? null, missingParent, children, cycle: false };
  };

  const forest: TraceTreeNode[] = [];
  const rootVisited = new Set<string>();
  for (const root of detail.roots) {
    forest.push(makeNode(root.callId, root.orphanParent, rootVisited));
    rootVisited.add(root.callId);
  }
  // orphanEdges：parent 不在本 trace 详情里——child 挂到「Missing parent」合成节点下。
  for (const orphan of detail.orphanEdges) {
    const alreadyRooted = detail.roots.some((root) => root.callId === orphan.childCallId);
    if (alreadyRooted) continue;
    forest.push({
      callId: orphan.missingParentCallId,
      call: null,
      missingParent: true,
      cycle: false,
      children: [makeNode(orphan.childCallId, false, new Set())],
    });
  }
  // 防御：既不在 roots 也无 edge 指向的 call（后端不应产生），作为独立根列出，
  // 绝不静默丢行。
  const covered = new Set<string>();
  const mark = (node: TraceTreeNode) => {
    covered.add(node.callId);
    node.children.forEach(mark);
  };
  forest.forEach(mark);
  for (const call of detail.calls) {
    if (!covered.has(call.callId)) {
      forest.push({ callId: call.callId, call, missingParent: false, children: [], cycle: false });
    }
  }
  return forest;
}

/* ── 树渲染（纯函数）────────────────────────────────────────────────────── */

function TraceChart({ detail, onSelectCall }: {
  detail: ModelObservabilityTraceDetail;
  onSelectCall: (callId: string) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(640);

  useEffect(() => {
    const el = containerRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver((entries) => {
      const next = entries[0]?.contentRect.width;
      if (next && next > 0) setWidth(Math.round(next));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const parentOf = useMemo(() => {
    const map = new Map<string, string>();
    for (const edge of detail.edges) map.set(edge.childCallId, edge.parentCallId);
    return map;
  }, [detail.edges]);

  // 按开始时间升序排点；时间未知（不完整记录）排在末尾。
  const points = useMemo(() => detail.calls
    .map((call) => ({ call, time: call.startedAt ? new Date(call.startedAt).getTime() : null }))
    .sort((a, b) => {
      if (a.time === null && b.time === null) return a.call.callId.localeCompare(b.call.callId);
      if (a.time === null) return 1;
      if (b.time === null) return -1;
      return a.time - b.time || a.call.callId.localeCompare(b.call.callId);
    }), [detail.calls]);

  const maxDuration = Math.max(0, ...points.map((point) => point.call.durationMs ?? 0));
  const timeValues = points.map((point) => point.time).filter((value): value is number => value !== null);
  const minTime = timeValues.length > 0 ? Math.min(...timeValues) : 0;
  const maxTime = timeValues.length > 0 ? Math.max(...timeValues) : 0;
  const span = maxTime - minTime;
  const plotWidth = width - TRACE_CHART_PAD_X * 2;

  const xOf = (time: number | null, index: number): number => {
    if (time === null || span <= 0) {
      return points.length <= 1 ? width / 2 : TRACE_CHART_PAD_X + (index / (points.length - 1)) * plotWidth;
    }
    return TRACE_CHART_PAD_X + ((time - minTime) / span) * plotWidth;
  };
  // 耗时未知（null）的调用点标在底线，绝不猜一个数（诚实原则）。
  const yOf = (duration: number | null): number => (
    duration === null || maxDuration <= 0
      ? TRACE_CHART_HEIGHT - TRACE_CHART_PAD_Y
      : TRACE_CHART_PAD_Y + (1 - duration / maxDuration) * (TRACE_CHART_HEIGHT - TRACE_CHART_PAD_Y * 2)
  );

  const linePoints = points.map((point, index) => `${xOf(point.time, index)},${yOf(point.call.durationMs)}`).join(' ');

  return (
    <div className={styles['observability-trace-chart']} ref={containerRef}>
      <svg
        viewBox={`0 0 ${width} ${TRACE_CHART_HEIGHT}`}
        role="img"
        aria-label={t('settings.observability.trace.chart.aria')}
      >
        <line
          className={styles['observability-trace-baseline']}
          x1={TRACE_CHART_PAD_X}
          y1={TRACE_CHART_HEIGHT - TRACE_CHART_PAD_Y}
          x2={width - TRACE_CHART_PAD_X}
          y2={TRACE_CHART_HEIGHT - TRACE_CHART_PAD_Y}
        />
        {points.length > 1 && (
          <polyline className={styles['observability-trace-line']} points={linePoints} />
        )}
        {points.map((point, index) => {
          const parentCallId = parentOf.get(point.call.callId);
          const titleLines = [
            shortId(point.call.callId),
            point.call.model.modelId ?? '—',
            `${terminalStatusLabel(point.call.terminalStatus)} · ${formatDurationMs(point.call.durationMs)}`,
            point.call.usage.summary ? `${formatCompactNumber(point.call.usage.summary.totalTokens)} tok` : '',
            point.time !== null ? formatLocalDateTime(point.call.startedAt) : t('settings.observability.trace.chart.noTime'),
            parentCallId ? `${t('settings.observability.trace.chart.parent')}: ${shortId(parentCallId)}` : '',
          ].filter(Boolean);
          const label = titleLines.join(' · ');
          const open = () => onSelectCall(point.call.callId);
          return (
            <circle
              key={point.call.callId}
              cx={xOf(point.time, index)}
              cy={yOf(point.call.durationMs)}
              r={4}
              className={styles['observability-trace-dot']}
              data-status={point.call.terminalStatus ?? 'unknown'}
              data-call-id={point.call.callId}
              role="button"
              tabIndex={0}
              aria-label={label}
              onClick={open}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault();
                  open();
                }
              }}
            >
              <title>{titleLines.join('\n')}</title>
            </circle>
          );
        })}
      </svg>
      {/* §一百六十二：图表必须配文字摘要与图例（不只靠图形/颜色） */}
      <div className={styles['observability-groups-summary']}>
        {t('settings.observability.trace.chart.summary', {
          total: formatNumber(detail.trace.callCount),
          ok: formatNumber(detail.trace.terminalOk),
          error: formatNumber(detail.trace.terminalError + detail.trace.terminalAborted),
          peak: formatDurationMs(maxDuration > 0 ? maxDuration : null),
        })}
      </div>
      <div className={styles['observability-groups-summary']}>
        {t('settings.observability.trace.chart.hint')}
      </div>
    </div>
  );
}

/* ── 详情统计四态（标签 + 数字，data-status 着色；0 值弱化）────────────────── */

function TraceTerm({ status, count }: { status: 'ok' | 'error' | 'aborted' | 'incomplete'; count: number }) {
  return (
    <span className={styles['observability-trace-term']} data-status={status} data-zero={count === 0 || undefined}>
      {terminalStatusLabel(status)} {formatCompactNumber(count)}
    </span>
  );
}

/* ── Trace 详情（分组统计卡 + 树）─────────────────────────────────────── */

function TraceDetailPanel({ detail, onSelectCall }: {
  detail: ModelObservabilityTraceDetail;
  onSelectCall: (callId: string) => void;
}) {
  const summary = detail.trace;
  return (
    <div className={styles['observability-trace-dialog']}>
      <h3 className={styles['observability-panel-title']}>
        {t('settings.observability.trace.title')}
        <span className={styles['observability-ledger-muted']}>
          {' · '}<code title={summary.traceId}>{shortId(summary.traceId)}</code>
        </span>
      </h3>
      {detail.graphIntegrity === 'degraded' && (
        <div className={styles['observability-completeness-note']} role="status">
          {t('settings.observability.trace.degradedWarning')}
        </div>
      )}
      {detail.dataCompleteness.status === 'unknown' && (
        <div className={styles['observability-completeness-note']} role="status" data-completeness="unknown">
          {t('settings.observability.ledger.completenessUnknown')}
        </div>
      )}
      <div className={styles['observability-trace-stat-grid']}>
        <div className={styles['observability-trace-stat-group']}>
          <div className={styles['observability-trace-stat-title']}>
            {t('settings.observability.trace.groups.trace')}
          </div>
          <div>
            <code title={summary.traceId}>{shortId(summary.traceId)}</code>
            <span className={styles['observability-ledger-muted']} title={summary.origin ?? undefined}>{originLabel(summary.origin)}</span>
          </div>
          <div
            className={styles['observability-ledger-muted']}
            title={`${isoTooltip(summary.firstSeenAt)} → ${isoTooltip(summary.lastSeenAt)}`}
          >
            {formatLocalDateTime(summary.firstSeenAt)} → {formatLocalDateTime(summary.lastSeenAt)}
          </div>
        </div>
        <div className={styles['observability-trace-stat-group']}>
          <div className={styles['observability-trace-stat-title']}>
            {t('settings.observability.trace.groups.calls')}
          </div>
          <div>{t('settings.observability.trace.stats.calls', { count: formatNumber(summary.callCount) })}</div>
          <div className={styles['observability-trace-terms']}>
            <TraceTerm status="ok" count={summary.terminalOk} />
            <TraceTerm status="error" count={summary.terminalError} />
            <TraceTerm status="aborted" count={summary.terminalAborted} />
            <TraceTerm status="incomplete" count={summary.incomplete} />
          </div>
        </div>
        <div className={styles['observability-trace-stat-group']}>
          <div className={styles['observability-trace-stat-title']}>
            {t('settings.observability.trace.groups.usage')}
          </div>
          <div data-usage-availability={detail.usageAggregate.availability}>
            {t(`settings.observability.trace.usageAvailability.${detail.usageAggregate.availability}`, {
              covered: formatNumber(detail.usageAggregate.coveredCalls),
              corrupt: formatNumber(detail.usageAggregate.corruptCalls),
              unknown: formatNumber(detail.usageAggregate.unknownCalls),
              total: formatNumber(detail.usageAggregate.totalCalls),
            })}
          </div>
          {detail.usageAggregate.summary && (
            <div>
              {formatCompactNumber(detail.usageAggregate.summary.totalTokens)} tok · {formatCost(detail.usageAggregate.summary.costTotal)}
            </div>
          )}
        </div>
        <div className={styles['observability-trace-stat-group']}>
          <div className={styles['observability-trace-stat-title']}>
            {t('settings.observability.trace.groups.payload')}
          </div>
          <div>
            {t('settings.observability.trace.stats.payload', {
              present: formatNumber(detail.payloadCompleteness.present),
              expired: formatNumber(detail.payloadCompleteness.expired),
              dropped: formatNumber(detail.payloadCompleteness.dropped),
              notCaptured: formatNumber(detail.payloadCompleteness.notCaptured),
              unknown: formatNumber(detail.payloadCompleteness.unknown),
            })}
          </div>
        </div>
      </div>
      <TraceChart detail={detail} onSelectCall={onSelectCall} />
    </div>
  );
}

/* ── 主组件（trace 列表 + 展开详情）───────────────────────────────────── */

export function ObservabilityTraceExplorer({ appliedFilter, selectedTraceId, onSelectTrace, onSelectCall, refreshToken }: {
  appliedFilter: ObservabilityFilterState;
  selectedTraceId: string | null;
  onSelectTrace: (traceId: string | null) => void;
  onSelectCall: (callId: string) => void;
  refreshToken: number;
}) {
  const [traces, setTraces] = useState<ModelObservabilityTraceListItem[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [listError, setListError] = useState<string | null>(null);
  const [listAbsent, setListAbsent] = useState(false);
  const [detail, setDetail] = useState<ModelObservabilityTraceDetail | null>(null);
  const [detailError, setDetailError] = useState<string | null>(null);
  const generationRef = useRef(0);
  const detailGenerationRef = useRef(0);

  useEffect(() => {
    const generation = ++generationRef.current;
    const controller = new AbortController();
    setLoading(true);
    setListError(null);
    setListAbsent(false);
    setTraces([]);
    setNextCursor(null);
    queryObservabilityTraces(
      { filter: buildCallFilterInput(appliedFilter), limit: MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT },
      { signal: controller.signal },
    ).then((page: ModelObservabilityTracePage) => {
      if (generationRef.current !== generation) return;
      setTraces(page.traces);
      setNextCursor(page.nextCursor);
      setLoading(false);
    }).catch((error: unknown) => {
      if (generationRef.current !== generation || isObservabilityAbortError(error)) return;
      if (error instanceof ModelObservabilityRequestError && error.kind === 'not_initialized') {
        setListAbsent(true);
      } else {
        setListError(error instanceof Error ? error.message : String(error));
      }
      setLoading(false);
    });
    return () => controller.abort();
  }, [appliedFilter, refreshToken]);

  useEffect(() => {
    if (!selectedTraceId) {
      setDetail(null);
      setDetailError(null);
      return;
    }
    const generation = ++detailGenerationRef.current;
    const controller = new AbortController();
    setDetail(null);
    setDetailError(null);
    loadObservabilityTraceDetail(selectedTraceId, { signal: controller.signal })
      .then((value) => {
        if (detailGenerationRef.current !== generation) return;
        setDetail(value);
      })
      .catch((error: unknown) => {
        if (detailGenerationRef.current !== generation || isObservabilityAbortError(error)) return;
        setDetailError(error instanceof Error ? error.message : String(error));
      });
    return () => controller.abort();
  }, [selectedTraceId]);

  const loadMore = useCallback(() => {
    if (!nextCursor || loadingMore) return;
    const generation = generationRef.current;
    setLoadingMore(true);
    queryObservabilityTraces(
      { filter: buildCallFilterInput(appliedFilter), limit: MODEL_OBSERVABILITY_PAGE_DEFAULT_LIMIT, cursor: nextCursor },
    )
      .then((page) => {
        if (generationRef.current !== generation) return;
        setTraces((prev) => {
          const seen = new Set(prev.map((trace) => trace.traceId));
          return [...prev, ...page.traces.filter((trace) => !seen.has(trace.traceId))];
        });
        setNextCursor(page.nextCursor);
        setLoadingMore(false);
      })
      .catch((error: unknown) => {
        if (generationRef.current !== generation || isObservabilityAbortError(error)) return;
        setListError(error instanceof Error ? error.message : String(error));
        setLoadingMore(false);
      });
  }, [appliedFilter, nextCursor, loadingMore]);

  if (listAbsent) {
    return (
      <div className={styles['observability-empty']} data-state="not-initialized">
        {t('settings.observability.empty.storeAbsent')}
      </div>
    );
  }
  if (listError) {
    return (
      <div className={styles['observability-error']} role="alert" data-kind="query_failed">
        <div className={styles['observability-error-detail']}>{listError}</div>
      </div>
    );
  }

  return (
    <div className={styles['observability-trace-explorer']}>
      {!loading && traces.length > 0 && (
        <div className={styles['observability-trace-header']} aria-hidden>
          <span>{t('settings.observability.trace.header.traceId')}</span>
          <span>{t('settings.observability.trace.header.origin')}</span>
          <span>{t('settings.observability.trace.header.lastSeen')}</span>
          <span>{t('settings.observability.trace.header.calls')}</span>
          <span data-status="ok">{t('settings.observability.trace.header.ok')}</span>
          <span data-status="error">{t('settings.observability.trace.header.error')}</span>
          <span data-status="aborted">{t('settings.observability.trace.header.aborted')}</span>
          <span data-status="incomplete">{t('settings.observability.trace.header.incomplete')}</span>
        </div>
      )}
      <div className={styles['observability-trace-list']} data-loading={loading || undefined}>
        {traces.map((trace) => (
          <button
            key={trace.traceId}
            type="button"
            className={styles['observability-trace-row']}
            data-selected={trace.traceId === selectedTraceId || undefined}
            onClick={() => onSelectTrace(trace.traceId === selectedTraceId ? null : trace.traceId)}
          >
            <code title={trace.traceId}>{shortId(trace.traceId)}</code>
            <span className={styles['observability-trace-origin']} title={trace.origin ?? undefined}>{originLabel(trace.origin)}</span>
            <span className={styles['observability-ledger-muted']} title={isoTooltip(trace.lastSeenAt)}>
              {formatLocalDateTime(trace.lastSeenAt)}
            </span>
            <span>{t('settings.observability.trace.stats.calls', { count: formatNumber(trace.callCount) })}</span>
            <span data-status="ok" data-zero={trace.terminalOk === 0 || undefined}>{formatCompactNumber(trace.terminalOk)}</span>
            <span data-status="error" data-zero={trace.terminalError === 0 || undefined}>{formatCompactNumber(trace.terminalError)}</span>
            <span data-status="aborted" data-zero={trace.terminalAborted === 0 || undefined}>{formatCompactNumber(trace.terminalAborted)}</span>
            <span data-status="incomplete" data-zero={trace.incomplete === 0 || undefined}>{formatCompactNumber(trace.incomplete)}</span>
          </button>
        ))}
        {!loading && traces.length === 0 && (
          <div className={styles['observability-empty']} data-state="no-results">
            {t('settings.observability.empty.noTraces')}
          </div>
        )}
        {loading && (
          <div className={styles['observability-loading']} aria-busy>
            {t('settings.observability.loading.traces')}
          </div>
        )}
      </div>
      {nextCursor && !loading && (
        <div className={styles['observability-ledger-more']}>
          <Button variant="secondary" size="sm" loading={loadingMore} onClick={loadMore}>
            {t('settings.observability.ledger.loadMore')}
          </Button>
        </div>
      )}
      {selectedTraceId && detailError && (
        <div className={styles['observability-error']} role="alert" data-kind="query_failed">
          <div className={styles['observability-error-detail']}>{detailError}</div>
        </div>
      )}
      {selectedTraceId && !detail && !detailError && (
        <div className={styles['observability-loading']} aria-busy>
          {t('settings.observability.loading.traceDetail')}
        </div>
      )}
      {detail && (
        <Overlay
          open
          scope="inline"
          onClose={() => onSelectTrace(null)}
          closeOnEsc
          closeOnBackdrop
          trapFocus
          contentProps={{ role: 'dialog', 'aria-label': t('settings.observability.trace.dialogAria') }}
        >
          <TraceDetailPanel
            detail={detail}
            onSelectCall={(callId) => {
              // 树节点 → Call Inspector 抽屉：先收起轨迹弹窗，避免两层浮层叠加。
              onSelectTrace(null);
              onSelectCall(callId);
            }}
          />
        </Overlay>
      )}
    </div>
  );
}
