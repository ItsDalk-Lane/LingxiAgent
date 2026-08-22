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
import { Button } from '../../../ui';
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
import { terminalStatusLabel } from './model-observability-labels';

/* ── 树构建（纯函数，§八十九/§九十二；测试直接锁定）────────────────────── */

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

/* ── 树渲染 ───────────────────────────────────────────────────────────── */

function TraceTreeNodeView({ node, depth, onSelectCall }: {
  node: TraceTreeNode;
  depth: number;
  onSelectCall: (callId: string) => void;
}) {
  return (
    <div className={styles['observability-trace-node']} data-depth={Math.min(depth, 8)}>
      {node.missingParent ? (
        <div className={styles['observability-trace-missing-parent']}>
          {t('settings.observability.trace.missingParent')}
          <code>{shortId(node.callId)}</code>
        </div>
      ) : (
        <button
          type="button"
          className={styles['observability-trace-call']}
          data-status={node.call?.terminalStatus ?? 'unknown'}
          onClick={() => onSelectCall(node.callId)}
        >
          <span className={styles['observability-trace-call-id']} title={node.callId}>
            {shortId(node.callId)}
          </span>
          <span>{node.call?.model.modelId ?? '—'}</span>
          <span className={styles['observability-ledger-muted']}>
            {terminalStatusLabel(node.call?.terminalStatus)}
            {' · '}{formatDurationMs(node.call?.durationMs ?? null)}
            {node.call?.usage.summary ? ` · ${formatCompactNumber(node.call.usage.summary.totalTokens)} tok` : ''}
          </span>
          {node.cycle && (
            <span className={styles['observability-trace-cycle']}>
              {t('settings.observability.trace.cycle')}
            </span>
          )}
        </button>
      )}
      {node.children.length > 0 && (
        <div className={styles['observability-trace-children']}>
          {node.children.map((child) => (
            <TraceTreeNodeView key={child.callId} node={child} depth={depth + 1} onSelectCall={onSelectCall} />
          ))}
        </div>
      )}
    </div>
  );
}

/* ── Trace 详情（header 统计 + 树）────────────────────────────────────── */

function TraceDetailPanel({ detail, onSelectCall }: {
  detail: ModelObservabilityTraceDetail;
  onSelectCall: (callId: string) => void;
}) {
  const forest = useMemo(() => buildTraceForest(detail), [detail]);
  const summary = detail.trace;
  return (
    <div className={styles['observability-trace-detail']}>
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
      <div className={styles['observability-trace-stats']}>
        <span title={summary.traceId}><code>{shortId(summary.traceId)}</code></span>
        <span>{summary.origin ?? '—'}</span>
        <span title={isoTooltip(summary.firstSeenAt)}>{formatLocalDateTime(summary.firstSeenAt)}</span>
        <span>→</span>
        <span title={isoTooltip(summary.lastSeenAt)}>{formatLocalDateTime(summary.lastSeenAt)}</span>
        <span>{t('settings.observability.trace.stats.calls', { count: formatNumber(summary.callCount) })}</span>
        <span data-status="ok">{formatCompactNumber(summary.terminalOk)}</span>
        <span data-status="error">{formatCompactNumber(summary.terminalError)}</span>
        <span data-status="aborted">{formatCompactNumber(summary.terminalAborted)}</span>
        <span data-status="incomplete">{formatCompactNumber(summary.incomplete)}</span>
        <span data-usage-availability={detail.usageAggregate.availability}>
          {t(`settings.observability.trace.usageAvailability.${detail.usageAggregate.availability}`, {
            covered: formatNumber(detail.usageAggregate.coveredCalls),
            corrupt: formatNumber(detail.usageAggregate.corruptCalls),
            unknown: formatNumber(detail.usageAggregate.unknownCalls),
            total: formatNumber(detail.usageAggregate.totalCalls),
          })}
        </span>
        {detail.usageAggregate.summary && (
          <>
            <span>{formatCompactNumber(detail.usageAggregate.summary.totalTokens)} tok</span>
            <span>{formatCost(detail.usageAggregate.summary.costTotal)}</span>
          </>
        )}
        <span>
          {t('settings.observability.trace.stats.payload', {
            present: formatNumber(detail.payloadCompleteness.present),
            expired: formatNumber(detail.payloadCompleteness.expired),
            dropped: formatNumber(detail.payloadCompleteness.dropped),
            notCaptured: formatNumber(detail.payloadCompleteness.notCaptured),
            unknown: formatNumber(detail.payloadCompleteness.unknown),
          })}
        </span>
      </div>
      <div className={styles['observability-trace-tree']}>
        {forest.map((node) => (
          <TraceTreeNodeView key={`${node.callId}:${node.missingParent}`} node={node} depth={0} onSelectCall={onSelectCall} />
        ))}
      </div>
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
            <span>{trace.origin ?? '—'}</span>
            <span className={styles['observability-ledger-muted']} title={isoTooltip(trace.lastSeenAt)}>
              {formatLocalDateTime(trace.lastSeenAt)}
            </span>
            <span>{t('settings.observability.trace.stats.calls', { count: formatNumber(trace.callCount) })}</span>
            <span data-status="ok">{formatCompactNumber(trace.terminalOk)}</span>
            <span data-status="error">{formatCompactNumber(trace.terminalError)}</span>
            <span data-status="aborted">{formatCompactNumber(trace.terminalAborted)}</span>
            <span data-status="incomplete">{formatCompactNumber(trace.incomplete)}</span>
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
      {detail && <TraceDetailPanel detail={detail} onSelectCall={onSelectCall} />}
    </div>
  );
}
