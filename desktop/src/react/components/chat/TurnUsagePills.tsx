/**
 * TurnUsagePills.tsx — assistant 轮次「用量」「用时」胶囊（各带点击弹出的明细窗）。
 *
 * 移植自 design-review/harness-usage-pills-reference/TurnUsagePanel.tsx
 * （DeepSeek Harness，MIT 许可）：结构与交互 1:1——图标+紧凑总量胶囊、
 * 触发钮锚定上方弹窗（12px 视口边距钳位）、外点/Esc 关闭、弹窗行按数据有无条件渲染。
 * 适配点：@deepseek-ai primitives（useAnchoredPosition/useDismissOnOutsidePointer/
 * Outline 图标）换成本文件内联实现；文案用组件内中文常量（照抄参考包
 * locale-keys.zh.txt，不经 desktop/src/locales/*.json）；TTFT 行不移植
 * （本项目无「首个 token」事实）。
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type MutableRefObject } from 'react';
import { createPortal } from 'react-dom';
import {
  formatExactTokens,
  formatLatencySeconds,
  formatRunDuration,
  formatTokensCompact,
  formatTokensPerSecond,
  type TurnUsageStats,
} from './turn-usage';
import css from './TurnUsagePills.module.css';

/* ── 弹窗中文文案（组件内常量，键值照抄 locale-keys.zh.txt）────────────── */
const USAGE_TITLE = '本轮用量';
const TIME_TITLE = '本轮用时和速度';
const LABEL_MODEL = '提供方 / 模型';
const LABEL_CACHE_HIT = '缓存命中';
const LABEL_INPUT = '未缓存输入';
const LABEL_CACHE_READ = '缓存读取';
const LABEL_CACHE_WRITE = '缓存写入';
const LABEL_OUTPUT = '输出';
const LABEL_DURATION = '本轮总用时';
const LABEL_SPEED = '输出速度（TPS）';
const LABEL_TTFT = '首 token 用时（TTFT）';
const formatConsumed = (total: string): string => `用量 ${total}`;
const formatReasoningNote = (tokens: string): string => `（其中推理 ${tokens}）`;
const formatTokCount = (count: string): string => `${count} tok`;
const formatTps = (tps: string): string => `${tps} tok/s`;
const formatRanFor = (duration: string): string => `用时 ${duration}`;

/** Viewport margin the placement clamp keeps（参考包 PANEL_MARGIN）。 */
const PANEL_MARGIN = 12;
/** Distance between the trigger's top edge and the panel's bottom（PANEL_GAP）。 */
const PANEL_GAP = 8;

/**
 * Unplaced portal panel: hidden but laid out so the clamp measures real
 * dimensions（参考包 MEASURE_STYLE）。
 */
const MEASURE_STYLE: CSSProperties = { visibility: 'hidden', left: 0, top: 0 };

interface StatDialogSeat {
  open: boolean;
  setOpen: (open: boolean) => void;
  rootRef: MutableRefObject<HTMLSpanElement | null>;
  panelRef: MutableRefObject<HTMLDivElement | null>;
  pos: CSSProperties | null;
}

/** One trigger-anchored dialog seat: open state, viewport-clamped placement, outside-close. */
function useStatDialog(): StatDialogSeat {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLSpanElement | null>(null);
  const panelRef = useRef<HTMLDivElement | null>(null);
  const [pos, setPos] = useState<CSSProperties | null>(null);

  const setOpenSafe = useCallback((next: boolean) => setOpen(next), []);

  // Portal placement: fixed above the trigger, clamped inside the viewport
  // (12px margin), so a trigger near the window edge cannot push it off-screen.
  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    const root = rootRef.current;
    const panel = panelRef.current;
    if (!root || !panel) return;
    const rect = root.getBoundingClientRect();
    const panelWidth = panel.offsetWidth;
    const panelHeight = panel.offsetHeight;
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;
    const left = Math.max(
      PANEL_MARGIN,
      Math.min(rect.left, viewportWidth - PANEL_MARGIN - panelWidth),
    );
    const top = Math.max(
      PANEL_MARGIN,
      Math.min(rect.top - PANEL_GAP - panelHeight, viewportHeight - PANEL_MARGIN - panelHeight),
    );
    setPos({ left, top });
  }, [open]);

  // Outside pointerdown closes; the portaled panel counts as inside.
  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent): void => {
      const target = event.target as Node | null;
      if (!target) return;
      if (rootRef.current?.contains(target)) return;
      if (panelRef.current?.contains(target)) return;
      setOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    return () => { document.removeEventListener('pointerdown', onPointerDown); };
  }, [open]);

  // Escape close stays local, one listener while open.
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('keydown', onKeyDown);
    return () => { document.removeEventListener('keydown', onKeyDown); };
  }, [open]);

  return { open, setOpen: setOpenSafe, rootRef, panelRef, pos };
}

/* ── 内联图标（项目惯例：同文件 function 声明，stroke 用 currentColor）──── */

function DatabaseOutlineIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <ellipse cx="12" cy="5.5" rx="8" ry="3" />
      <path d="M4 5.5v6.5c0 1.66 3.58 3 8 3s8-1.34 8-3V5.5" />
      <path d="M4 12v6.5c0 1.66 3.58 3 8 3s8-1.34 8-3V12" />
    </svg>
  );
}

function ClockOutlineIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 2" />
    </svg>
  );
}

interface TurnUsagePillsProps {
  stats: TurnUsageStats;
}

/**
 * 「用量 + 用时」双胶囊。无 usage 事实（totalTokens 无数据）整体不渲染，
 * 不渲染 0。
 */
export function TurnUsagePills({ stats }: TurnUsagePillsProps) {
  if (stats.totalTokens === null || stats.totalTokens === undefined) return null;
  return (
    <>
      <TurnUsagePanel stats={stats} />
      {stats.runMs !== null && <TurnTimePanel stats={stats} />}
    </>
  );
}

/** 用量胶囊 + 明细弹窗（行按数据有无条件渲染）。 */
function TurnUsagePanel({ stats }: { stats: TurnUsageStats }) {
  const { open, setOpen, rootRef, panelRef, pos } = useStatDialog();
  const total = stats.totalTokens ?? 0;

  return (
    <span ref={rootRef} className={css.root}>
      <button
        type="button"
        className={css.trigger}
        aria-haspopup="dialog"
        aria-expanded={open}
        data-testid="turn-usage-pill"
        onClick={() => { setOpen(!open); }}
      >
        <DatabaseOutlineIcon />
        <span className={css.label}>{formatConsumed(formatTokCount(formatTokensCompact(total)))}</span>
      </button>
      {open && createPortal(
        <div
          ref={panelRef}
          className={css.panel}
          role="dialog"
          aria-label={USAGE_TITLE}
          style={pos ?? MEASURE_STYLE}
          data-testid="turn-usage-dialog"
        >
          <div className={css.title}>
            <span className={css.titleLabel}>
              <DatabaseOutlineIcon />
              {USAGE_TITLE}
            </span>
            <span className={css.titleValue}>{formatTokCount(formatExactTokens(total))}</span>
          </div>
          <div className={css.titleRule} aria-hidden />
          <dl className={css.details} data-turn-usage-details>
            {stats.modelLabels.length > 0 && (
              <>
                <dt>{LABEL_MODEL}</dt>
                <dd className={css.route}>{stats.modelLabels.join(', ')}</dd>
              </>
            )}
            {stats.cacheHitPercent !== null && (
              <>
                <dt>{LABEL_CACHE_HIT}</dt>
                <dd>{`${stats.cacheHitPercent}%`}</dd>
              </>
            )}
            {stats.uncachedInputTokens !== null && (
              <>
                <dt>{LABEL_INPUT}</dt>
                <dd>{formatTokCount(formatExactTokens(stats.uncachedInputTokens))}</dd>
              </>
            )}
            {stats.cacheReadTokens !== null && (
              <>
                <dt>{LABEL_CACHE_READ}</dt>
                <dd>{formatTokCount(formatExactTokens(stats.cacheReadTokens))}</dd>
              </>
            )}
            {stats.cacheWriteTokens !== null && stats.cacheWriteTokens > 0 && (
              <>
                <dt>{LABEL_CACHE_WRITE}</dt>
                <dd>{formatTokCount(formatExactTokens(stats.cacheWriteTokens))}</dd>
              </>
            )}
            {stats.outputTokens !== null && (
              <>
                <dt>{LABEL_OUTPUT}</dt>
                <dd>
                  {formatTokCount(formatExactTokens(stats.outputTokens))}
                  {stats.reasoningTokens !== null && (
                    <span className={css.reasoning}>
                      {formatReasoningNote(formatTokCount(formatExactTokens(stats.reasoningTokens)))}
                    </span>
                  )}
                </dd>
              </>
            )}
          </dl>
        </div>,
        document.body,
      )}
    </span>
  );
}

/** 用时胶囊 + 明细弹窗（TTFT 无事实来源，不渲染该行）。 */
function TurnTimePanel({ stats }: { stats: TurnUsageStats }) {
  const { open, setOpen, rootRef, panelRef, pos } = useStatDialog();
  if (stats.runMs === null) return null;

  return (
    <span ref={rootRef} className={css.root}>
      <button
        type="button"
        className={css.trigger}
        aria-haspopup="dialog"
        aria-expanded={open}
        data-testid="turn-time-pill"
        onClick={() => { setOpen(!open); }}
      >
        <ClockOutlineIcon />
        <span className={css.label}>{formatRanFor(formatRunDuration(stats.runMs))}</span>
      </button>
      {open && createPortal(
        <div
          ref={panelRef}
          className={css.panel}
          role="dialog"
          aria-label={TIME_TITLE}
          style={pos ?? MEASURE_STYLE}
          data-testid="turn-time-dialog"
        >
          <div className={css.title}>
            <span className={css.titleLabel}>
              <ClockOutlineIcon />
              {TIME_TITLE}
            </span>
          </div>
          <div className={css.titleRule} aria-hidden />
          <dl className={css.details} data-turn-time-details>
            <dt>{LABEL_DURATION}</dt>
            <dd>{formatRunDuration(stats.runMs)}</dd>
            {stats.tokensPerSecond !== null && (
              <>
                <dt>{LABEL_SPEED}</dt>
                <dd>{formatTps(formatTokensPerSecond(stats.tokensPerSecond))}</dd>
              </>
            )}
            {stats.ttftMs !== null && (
              <>
                <dt>{LABEL_TTFT}</dt>
                <dd>{`${formatLatencySeconds(stats.ttftMs)}秒`}</dd>
              </>
            )}
          </dl>
        </div>,
        document.body,
      )}
    </span>
  );
}
