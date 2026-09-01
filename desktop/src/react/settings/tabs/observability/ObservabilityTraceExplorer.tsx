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
  ModelObservabilityRequestError,
  queryObservabilityTraces,
} from './model-observability-actions';
import { buildCallFilterInput, type ObservabilityFilterState } from './model-observability-filter';
import { TraceDetailOverlay } from './trace-detail/TraceDetailOverlay';
import {
  formatCompactNumber,
  formatLocalDateTime,
  formatNumber,
  isoTooltip,
  shortId,
} from './model-observability-format';
import { sourceIdentityKindLabel, sourceIdentityTitle } from './model-observability-labels';

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

/* ── 树渲染（纯函数）────────────────────────────────────────────────────── */

/* ── 主组件（trace 列表 + 展开详情）───────────────────────────────────── */

export function ObservabilityTraceExplorer({ appliedFilter, selectedTraceId, onSelectTrace, onSelectCall, refreshToken, isLocalOwner = false }: {
  appliedFilter: ObservabilityFilterState;
  selectedTraceId: string | null;
  onSelectTrace: (traceId: string | null) => void;
  onSelectCall: (callId: string) => void;
  refreshToken: number;
  /** 轨迹详情层内观测载荷 tab 的本机 owner 判定。 */
  isLocalOwner?: boolean;
}) {
  const [traces, setTraces] = useState<ModelObservabilityTraceListItem[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [listError, setListError] = useState<string | null>(null);
  const [listAbsent, setListAbsent] = useState(false);
  const generationRef = useRef(0);

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
            <span className={styles['observability-trace-origin']} title={trace.sourceIdentity?.entityId ?? trace.origin ?? undefined}>
              {sourceIdentityTitle(trace.sourceIdentity)}
              <small> · {sourceIdentityKindLabel(trace.sourceIdentity?.kind ?? 'unknown')}</small>
            </span>
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
      {selectedTraceId && (
        <TraceDetailOverlay
          traceId={selectedTraceId}
          onClose={() => onSelectTrace(null)}
          isLocalOwner={isLocalOwner}
        />
      )}
    </div>
  );
}
