/**
 * TrajectoryTimeline.tsx — 轨迹台账上方的固定全览时间线（三泳道）。
 *
 * 移植自 dsh-desktop packages/client/ui-trajectory/src/client/TrajectoryTimeline.tsx
 * （MIT）。适配点：dsh ui-primitives Tooltip → 灵犀 ui/Tooltip（label/side →
 * content/placement）；文案走灵犀 i18n。
 */

import {
  memo, useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent,
  type PointerEvent,
} from 'react';
import { Tooltip } from '../../../../ui';
import { t } from '../../../helpers';
import type { TrajectoryTurnModel } from './trace-layout.ts';
import type { AssistantMetricDetail, TrajectoryCellKind, TrajectoryCellProps } from './trajectory-record.ts';
import {
  deriveTrajectoryTimeline,
  formatTimelineOffset,
  type TrajectoryTimelineMode,
  type TrajectoryTimeRange,
} from './timeline.ts';
import css from './TrajectoryTimeline.module.css';

const MINIMUM_DRAG_PX = 3;
const MINIMUM_ZOOM_OPERATIONS = 4;
const EDGE_PAN_ZONE_FRACTION = 0.08;
const EDGE_PAN_STEP_FRACTION = 0.025;
const MAXIMUM_EDGE_PAN_PX = 32;
const TIMELINE_TOOLTIP_DELAY_MS = 500;

interface TimelineRecordDetail {
  decodingMs?: number;
  durationMs?: number;
  startedAt?: number;
  ttftMs?: number;
}

interface FractionRange {
  start: number;
  end: number;
}

interface HoverPoint {
  fraction: number;
  recordIndex: number | null;
}

interface PanGesture {
  anchorClientX: number;
  anchorStart: number;
  moved: boolean;
  pannable: boolean;
  pointerId: number;
}

function assistantTimingDetail(
  metrics: AssistantMetricDetail | undefined,
): Pick<TimelineRecordDetail, 'ttftMs' | 'decodingMs'> {
  const start = metrics?.stepStartTime;
  // 响应到达事实（产品口径 2026-09-05）：时间线的请求段/解码段以
  // 「provider 响应到达」为分界——不虚构首 token。
  const first = metrics?.responseArrivalTime;
  const completed = metrics?.completedTime;
  if (
    metrics?.timingRecorded !== true
    || typeof start !== 'number'
    || typeof first !== 'number'
    || typeof completed !== 'number'
    || !Number.isFinite(start)
    || !Number.isFinite(first)
    || !Number.isFinite(completed)
    || first < start
    || completed < first
  ) return {};
  return { ttftMs: first - start, decodingMs: completed - first };
}

function timelineRecordDetail(cell: TrajectoryCellProps): TimelineRecordDetail {
  const durationMs = cell.timeSeconds === null || !Number.isFinite(cell.timeSeconds)
    ? undefined
    : Math.max(0, cell.timeSeconds * 1_000);
  const startedAt = cell.startedAt === null || cell.startedAt === undefined || !Number.isFinite(cell.startedAt)
    ? undefined
    : cell.startedAt;
  return {
    ...(durationMs === undefined ? {} : { durationMs }),
    ...(startedAt === undefined ? {} : { startedAt }),
    ...assistantTimingDetail(cell.assistantMetrics),
  };
}

function timelineKindLabel(kind: TrajectoryCellKind): string {
  return t(`settings.observability.traceDetail.kind.${kind}`);
}

function formatRecordedTime(timestamp: number): string {
  return new Date(timestamp).toLocaleTimeString(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    fractionalSecondDigits: 3,
  });
}

function timelineTooltipLabel(
  kind: TrajectoryCellKind,
  detail: TimelineRecordDetail | undefined,
): string {
  const heading = timelineKindLabel(kind);
  if (detail === undefined) return heading;
  const duration = detail.durationMs === undefined
    ? null
    : `${t('settings.observability.traceDetail.timeline.timing.total')} ${formatTimelineOffset(detail.durationMs)}`;
  const range = detail.startedAt === undefined
    ? null
    : detail.durationMs === undefined
      ? `${t('settings.observability.traceDetail.timeline.timing.started')} ${formatRecordedTime(detail.startedAt)}`
      : `${formatRecordedTime(detail.startedAt)} → ${formatRecordedTime(
        detail.startedAt + detail.durationMs,
      )}`;
  const segments = detail.ttftMs === undefined || detail.decodingMs === undefined
    ? null
    : `TTFT ${formatTimelineOffset(detail.ttftMs)} · ${t('settings.observability.traceDetail.timeline.timing.decoding')} ${formatTimelineOffset(
      detail.decodingMs,
    )}`;
  const timing = [duration, segments].filter(value => value !== null).join(' · ');
  return [heading, range, timing].filter(value => value !== null && value !== '').join('\n');
}

/** 固定全览时间线的 props。 */
export interface TrajectoryTimelineProps {
  turns: readonly TrajectoryTurnModel[];
  mode: TrajectoryTimelineMode;
  range: TrajectoryTimeRange | null;
  /** 已加载时间线是否省略了更早历史前缀。 */
  hasEarlierRecords?: boolean;
  /** 从截断控件加载一页更早历史。 */
  onLoadEarlier?: () => Promise<boolean>;
  selectedIndex?: number | null;
  /** 与当前台账搜索匹配的记录序号集合；无查询时为 null。 */
  searchMatchIndexes?: ReadonlySet<number> | null;
  onRangeChange: (range: TrajectoryTimeRange | null) => void;
  /** 直接点击时间块时选中该记录。 */
  onRecordSelect?: (index: number) => void;
  /** 点击时间线空白后把最近记录滚入视野。 */
  onRecordFocus?: (index: number) => void;
}

function orderedRange(left: number, right: number): FractionRange {
  return left <= right ? { start: left, end: right } : { start: right, end: left };
}

function clampFraction(value: number): number {
  return Math.min(1, Math.max(0, value));
}

function centeredRange(
  center: number,
  width: number,
  minimum: number,
  maximum: number,
): FractionRange {
  const clampedWidth = Math.min(maximum - minimum, Math.max(0, width));
  const start = Math.min(
    Math.max(center - clampedWidth / 2, minimum),
    maximum - clampedWidth,
  );
  return { start, end: start + clampedWidth };
}

function rangeFraction(
  range: TrajectoryTimeRange,
  start: number,
  duration: number,
  minimum: number,
  maximum: number,
): FractionRange {
  const bounded = orderedRange(
    Math.min(maximum, Math.max(minimum, range.start)),
    Math.min(maximum, Math.max(minimum, range.end)),
  );
  return {
    start: (bounded.start - start) / duration,
    end: (bounded.end - start) / duration,
  };
}

function LaneLabels() {
  return (
    <div className={css.labels} aria-hidden="true">
      <span>{t('settings.observability.traceDetail.lane.input')}</span>
      <span>{t('settings.observability.traceDetail.lane.model')}</span>
      <span>{t('settings.observability.traceDetail.lane.tools')}</span>
    </div>
  );
}

function EarlierHistoryBoundary({
  loading,
  onHover,
  onLoad,
}: {
  loading: boolean;
  onHover: () => void;
  onLoad: (() => void) | undefined;
}) {
  return (
    <Tooltip
      content={loading
        ? t('settings.observability.traceDetail.table.loadingEarlier')
        : t('settings.observability.traceDetail.timeline.loadEarlierHint')}
      placement="right"
      delayMs={TIMELINE_TOOLTIP_DELAY_MS}
    >
      <button
        type="button"
        className={css.earlierHistory}
        data-earlier-history
        data-loading={loading || undefined}
        aria-label={loading
          ? t('settings.observability.traceDetail.table.loadingEarlier')
          : t('settings.observability.traceDetail.table.loadEarlier')}
        aria-disabled={loading || onLoad === undefined}
        onClick={onLoad}
        onPointerEnter={(event) => {
          event.stopPropagation();
          onHover();
        }}
        onPointerMove={(event) => { event.stopPropagation() }}
        onPointerDown={(event) => { event.stopPropagation() }}
      >
        …
      </button>
    </Tooltip>
  );
}

/** 带拖选、点击聚焦与 Escape 复位的全览渲染器。 */
export const TrajectoryTimeline = memo(function TrajectoryTimeline({
  turns,
  mode,
  range,
  hasEarlierRecords = false,
  onLoadEarlier,
  selectedIndex = null,
  searchMatchIndexes = null,
  onRangeChange,
  onRecordSelect,
  onRecordFocus,
}: TrajectoryTimelineProps) {
  const model = useMemo(() => deriveTrajectoryTimeline(turns, mode), [mode, turns]);
  const detailByIndex = useMemo(
    () => new Map(turns.flatMap(turn =>
      turn.groups.flatMap(group =>
        group.cells.map(cell => [cell.index, timelineRecordDetail(cell)] as const),
      ),
    )),
    [turns],
  );
  const dragRef = useRef<{
    pointerId: number;
    anchorTime: number;
    anchorClientX: number;
    recordIndex: number | null;
  } | null>(null);
  const panRef = useRef<PanGesture | null>(null);
  const rootRef = useRef<HTMLElement | null>(null);
  const trackRef = useRef<HTMLDivElement | null>(null);
  const [draft, setDraft] = useState<TrajectoryTimeRange | null>(null);
  const [hover, setHover] = useState<HoverPoint | null>(null);
  const [loadingEarlier, setLoadingEarlier] = useState(false);
  const [panning, setPanning] = useState(false);
  const [viewport, setViewport] = useState<TrajectoryTimeRange | null>(null);
  const [animateViewport, setAnimateViewport] = useState(false);
  useEffect(() => {
    if (
      model !== null
      && range !== null
      && (range.end < model.start || range.start > model.end)
    ) {
      onRangeChange(null);
    }
  }, [model, onRangeChange, range]);
  useEffect(() => {
    if (model === null) return;
    setAnimateViewport(false);
    setViewport(current =>
      current !== null && (current.end < model.start || current.start > model.end)
        ? null
        : current);
  }, [model]);
  useEffect(() => {
    if (model === null || selectedIndex === null) return;
    const selectedSpan = model.spans.find(span => span.index === selectedIndex);
    if (selectedSpan === undefined) return;
    setAnimateViewport(true);
    setViewport((current) => {
      if (current === null) return current;
      if (
        selectedSpan.end > current.start
        && selectedSpan.start < current.end
      ) return current;
      const duration = Math.max(1, current.end - current.start);
      const desiredStart = selectedSpan.end <= current.start
        ? selectedSpan.start
        : selectedSpan.end - duration;
      const nextStart = Math.min(
        Math.max(desiredStart, model.start),
        Math.max(model.start, model.end - duration),
      );
      if (nextStart === current.start) return current;
      return { start: nextStart, end: nextStart + duration };
    });
  }, [model, selectedIndex]);
  const fullDuration = Math.max(1, (model?.end ?? 0) - (model?.start ?? 0));
  const viewportDuration = Math.min(
    fullDuration,
    Math.max(1, (viewport?.end ?? 0) - (viewport?.start ?? 0)),
  );
  const viewportStart = model === null || viewport === null
    ? model?.start ?? 0
    : Math.min(
      Math.max(viewport.start, model.start),
      model.end - viewportDuration,
    );
  const domainDuration = viewport === null ? fullDuration : viewportDuration;
  const domainStart = viewport === null ? model?.start ?? 0 : viewportStart;
  const showsEarlierBoundary = hasEarlierRecords
    && model !== null
    && domainStart === model.start;
  const loadEarlier = onLoadEarlier === undefined || loadingEarlier
    ? undefined
    : () => {
      setLoadingEarlier(true);
      void onLoadEarlier().finally(() => { setLoadingEarlier(false) });
    };
  const projectedDomainStyle = model === null
    ? undefined
    : {
      '--trajectory-domain-left':
        `${-(domainStart - model.start) / domainDuration * 100}%`,
      '--trajectory-domain-width': `${fullDuration / domainDuration * 100}%`,
    } as CSSProperties;
  const committed = model === null || range === null
    ? null
    : rangeFraction(range, domainStart, domainDuration, model.start, model.end);
  const draftFraction = model === null || draft === null
    ? null
    : rangeFraction(draft, domainStart, domainDuration, model.start, model.end);
  const visibleRange = draftFraction ?? committed;
  const activeRange = draft ?? range;
  useEffect(() => {
    const root = rootRef.current;
    if (root === null) return;
    const onWheel = (event: globalThis.WheelEvent): void => {
      event.preventDefault();
      const track = trackRef.current;
      if (track === null || model === null) return;
      setAnimateViewport(false);
      const rect = track.getBoundingClientRect();
      const anchorFraction =
        clampFraction((event.clientX - rect.left) / Math.max(1, rect.width));
      const nextDuration = Math.min(
        fullDuration,
        Math.max(
          Math.min(mode === 'sequence' ? MINIMUM_ZOOM_OPERATIONS : 20, fullDuration),
          domainDuration * Math.exp(event.deltaY * 0.0015),
        ),
      );
      if (nextDuration >= fullDuration * 0.999) {
        setViewport(null);
        return;
      }
      const anchorTime = domainStart + anchorFraction * domainDuration;
      const nextStart = Math.min(
        Math.max(anchorTime - anchorFraction * nextDuration, model.start),
        model.end - nextDuration,
      );
      setViewport({ start: nextStart, end: nextStart + nextDuration });
    };
    root.addEventListener('wheel', onWheel, { passive: false });
    return () => { root.removeEventListener('wheel', onWheel) };
  }, [domainDuration, domainStart, fullDuration, mode, model]);

  if (model === null) {
    return (
      <section ref={rootRef} className={css.root} aria-label={t('settings.observability.traceDetail.timeline.aria')}>
        <div className={css.plot}>
          <LaneLabels />
          <div className={css.track}>
            <span className={css.empty}>{t('settings.observability.traceDetail.timeline.noTimingData')}</span>
            {hasEarlierRecords && (
              <EarlierHistoryBoundary
                loading={loadingEarlier}
                onHover={() => { setHover(null) }}
                onLoad={loadEarlier}
              />
            )}
          </div>
        </div>
      </section>
    );
  }

  const minimumSelectionDuration = Math.min(
    domainDuration,
    fullDuration / model.spans.length,
  );

  const fractionAt = (event: PointerEvent<HTMLDivElement>): number => {
    const rect = event.currentTarget.getBoundingClientRect();
    return clampFraction((event.clientX - rect.left) / Math.max(1, rect.width));
  };

  const recordIndexAt = (event: PointerEvent<HTMLDivElement>): number | null => {
    const target = event.target instanceof HTMLElement ? event.target : null;
    const value = target?.closest<HTMLElement>('[data-timeline-record-index]')
      ?.dataset.timelineRecordIndex;
    if (value === undefined) return null;
    const index = Number(value);
    return Number.isFinite(index) ? index : null;
  };

  const commit = (nextRange: TrajectoryTimeRange) => {
    onRangeChange(nextRange);
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button === 2) {
      panRef.current = {
        anchorClientX: event.clientX,
        anchorStart: domainStart,
        moved: false,
        pannable: viewport !== null,
        pointerId: event.pointerId,
      };
      if (viewport !== null) setAnimateViewport(false);
      setPanning(true);
      if (typeof event.currentTarget.setPointerCapture === 'function') {
        event.currentTarget.setPointerCapture(event.pointerId);
      }
      return;
    }
    if (event.button !== 0) return;
    const anchor = fractionAt(event);
    const anchorTime = domainStart + anchor * domainDuration;
    const recordIndex = recordIndexAt(event);
    setHover({ fraction: anchor, recordIndex });
    dragRef.current = {
      pointerId: event.pointerId,
      anchorTime,
      anchorClientX: event.clientX,
      recordIndex,
    };
    if (typeof event.currentTarget.setPointerCapture === 'function') {
      event.currentTarget.setPointerCapture(event.pointerId);
    }
    setDraft({ start: anchorTime, end: anchorTime });
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const fraction = fractionAt(event);
    setHover({ fraction, recordIndex: recordIndexAt(event) });
    const pan = panRef.current;
    if (pan !== null && pan.pointerId === event.pointerId) {
      if (Math.abs(event.clientX - pan.anchorClientX) >= MINIMUM_DRAG_PX) {
        pan.moved = true;
      }
      if (!pan.pannable) return;
      const delta = (event.clientX - pan.anchorClientX) / Math.max(1, rect.width);
      const nextStart = Math.min(
        Math.max(pan.anchorStart - delta * domainDuration, model.start),
        model.end - domainDuration,
      );
      setViewport({ start: nextStart, end: nextStart + domainDuration });
      return;
    }
    const drag = dragRef.current;
    if (drag === null || drag.pointerId !== event.pointerId) return;
    let nextDomainStart = domainStart;
    if (viewport !== null) {
      const localX = event.clientX - rect.left;
      const edgeWidth = Math.min(
        MAXIMUM_EDGE_PAN_PX,
        Math.max(1, rect.width * EDGE_PAN_ZONE_FRACTION),
      );
      const direction = localX < edgeWidth
        ? -1
        : localX > rect.width - edgeWidth ? 1 : 0;
      if (direction !== 0) {
        const edgeDistance = direction < 0
          ? edgeWidth - localX
          : localX - (rect.width - edgeWidth);
        const strength = clampFraction(edgeDistance / edgeWidth);
        const desiredStart = domainStart
          + direction * domainDuration * EDGE_PAN_STEP_FRACTION
          * Math.max(0.2, strength);
        nextDomainStart = Math.min(
          Math.max(desiredStart, model.start),
          model.end - domainDuration,
        );
        if (nextDomainStart !== domainStart) {
          setAnimateViewport(false);
          setViewport({
            start: nextDomainStart,
            end: nextDomainStart + domainDuration,
          });
        }
      }
    }
    const pointTime = nextDomainStart + fraction * domainDuration;
    setDraft(orderedRange(drag.anchorTime, pointTime));
  };

  const onPointerEnd = (event: PointerEvent<HTMLDivElement>) => {
    const pan = panRef.current;
    if (pan !== null && pan.pointerId === event.pointerId) {
      const moved = pan.moved
        || Math.abs(event.clientX - pan.anchorClientX) >= MINIMUM_DRAG_PX;
      panRef.current = null;
      setPanning(false);
      if (!moved) onRangeChange(null);
      return;
    }
    const drag = dragRef.current;
    if (drag === null || drag.pointerId !== event.pointerId) return;
    const pointFraction = fractionAt(event);
    const pointTime = domainStart + pointFraction * domainDuration;
    const selected = orderedRange(drag.anchorTime, pointTime);
    setHover({ fraction: pointFraction, recordIndex: recordIndexAt(event) });
    dragRef.current = null;
    setDraft(null);
    const click = Math.abs(event.clientX - drag.anchorClientX) < MINIMUM_DRAG_PX;
    const clickedSpan = click && drag.recordIndex !== null
      ? model.spans.find(span => span.index === drag.recordIndex)
      : undefined;
    if (clickedSpan !== undefined) {
      onRangeChange(null);
      onRecordSelect?.(clickedSpan.index);
      return;
    }
    const committedRange = selected.end - selected.start < minimumSelectionDuration
      ? centeredRange(
        click ? selected.start : (selected.start + selected.end) / 2,
        minimumSelectionDuration,
        model.start,
        model.end,
      )
      : selected;
    commit(committedRange);
    if (click) {
      const timelinePoint = selected.start;
      const nearest = model.spans.reduce((candidate, span) => {
        const candidateDistance = timelinePoint < candidate.start
          ? candidate.start - timelinePoint
          : timelinePoint > candidate.end ? timelinePoint - candidate.end : 0;
        const spanDistance = timelinePoint < span.start
          ? span.start - timelinePoint
          : timelinePoint > span.end ? timelinePoint - span.end : 0;
        return spanDistance < candidateDistance ? span : candidate;
      });
      onRecordFocus?.(nearest.index);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'Escape' || range === null) return;
    event.preventDefault();
    onRangeChange(null);
  };

  const onPointerCancel = () => {
    dragRef.current = null;
    panRef.current = null;
    setDraft(null);
    setHover(null);
    setPanning(false);
  };

  return (
    <section ref={rootRef} className={css.root} aria-label={t('settings.observability.traceDetail.timeline.aria')}>
      <div className={css.plot}>
        <LaneLabels />
        <div
          ref={trackRef}
          className={css.track}
          data-panning={panning || undefined}
          aria-label={t('settings.observability.traceDetail.timeline.trackAria')}
          tabIndex={0}
          onKeyDown={onKeyDown}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerEnd}
          onPointerCancel={onPointerCancel}
          onPointerLeave={() => {
            if (dragRef.current === null && panRef.current === null) setHover(null);
          }}
          onDoubleClick={(event) => {
            event.preventDefault();
            onRangeChange(null);
          }}
          onContextMenu={(event) => {
            event.preventDefault();
          }}
        >
          {showsEarlierBoundary && (
            <EarlierHistoryBoundary
              loading={loadingEarlier}
              onHover={() => { setHover(null) }}
              onLoad={loadEarlier}
            />
          )}
          {hover !== null && hover.recordIndex === null && draft === null && (
            <div
              className={css.hoverLine}
              data-timeline-hover-line
              aria-hidden="true"
              style={{
                '--trajectory-hover-left': `${hover.fraction * 100}%`,
              } as CSSProperties}
            />
          )}
          {visibleRange !== null && (
            <>
              <div
                className={css.selection}
                data-dragging={draft === null ? undefined : 'true'}
                aria-hidden="true"
                style={{
                  '--trajectory-selection-left': `${visibleRange.start * 100}%`,
                  '--trajectory-selection-width': `${(visibleRange.end - visibleRange.start) * 100}%`,
                } as CSSProperties}
              />
              <div
                className={css.selectionEdges}
                data-dragging={draft === null ? undefined : 'true'}
                aria-hidden="true"
                style={{
                  '--trajectory-selection-left': `${visibleRange.start * 100}%`,
                  '--trajectory-selection-width': `${(visibleRange.end - visibleRange.start) * 100}%`,
                } as CSSProperties}
              />
            </>
          )}
          <div
            className={css.turnBoundaries}
            data-animate-viewport={animateViewport || undefined}
            aria-hidden="true"
            style={projectedDomainStyle}
          >
            {model.turnBoundaries
              .filter(boundary =>
                boundary.time > model.start
                && boundary.time >= domainStart
                && boundary.time <= domainStart + domainDuration)
              .map(boundary => (
                <span
                  className={css.turnBoundary}
                  data-turn={boundary.turn}
                  key={boundary.turn}
                  style={{
                    '--trajectory-turn-left':
                      `${(boundary.time - model.start) / fullDuration * 100}%`,
                  } as CSSProperties}
                />
              ))}
          </div>
          <div
            className={css.lanes}
            data-animate-viewport={animateViewport || undefined}
            data-timeline-domain
            style={projectedDomainStyle}
          >
            {model.spans
              .filter(span =>
                span.index === selectedIndex
                || (span.end >= domainStart && span.start <= domainStart + domainDuration))
              .map((span) => {
                const left = (span.start - model.start) / fullDuration;
                const width = (span.end - span.start) / fullDuration;
                const widthPercent = width * 100;
                const detail = detailByIndex.get(span.index);
                const ttftMs = detail?.ttftMs;
                const decodingMs = detail?.decodingMs;
                const ttftFraction = ttftMs === undefined
                  || decodingMs === undefined
                  || ttftMs + decodingMs <= 0
                  ? null
                  : ttftMs / (ttftMs + decodingMs);
                const tooltipText = timelineTooltipLabel(span.kind, detail);
                return (
                  <Tooltip
                    key={span.index}
                    content={tooltipText}
                    placement="bottom"
                    delayMs={TIMELINE_TOOLTIP_DELAY_MS}
                  >
                    <span
                      aria-hidden="true"
                      className={css.span}
                      data-timeline-span={span.kind}
                      data-timeline-record-index={span.index}
                      data-assistant-timing={ttftFraction === null ? undefined : 'true'}
                      data-error={span.isError || undefined}
                      data-equal-duration={mode === 'time' || undefined}
                      data-current={span.index === selectedIndex || undefined}
                      data-hovered={hover?.recordIndex === span.index || undefined}
                      data-search-match={searchMatchIndexes === null
                        ? undefined
                        : searchMatchIndexes.has(span.index) ? 'true' : 'false'}
                      data-selected={activeRange === null
                        ? undefined
                        : span.start <= activeRange.end && span.end >= activeRange.start
                          ? 'true'
                          : 'false'}
                      style={{
                        '--trajectory-span-left': `${left * 100}%`,
                        '--trajectory-span-width': `${widthPercent}%`,
                        '--trajectory-span-gap': `min(${widthPercent * 0.08}%, 1px)`,
                        '--trajectory-span-lane': span.lane,
                        ...(ttftFraction === null
                          ? {}
                          : { '--trajectory-assistant-ttft': `${ttftFraction * 100}%` }),
                      } as CSSProperties}
                    />
                  </Tooltip>
                );
              })}
          </div>
        </div>
      </div>
    </section>
  );
});
