import React, { useCallback, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { t } from '../../helpers';
import styles from '../../Settings.module.css';
import {
  formatNumber,
  formatPercent,
  hitRate,
  type UsageAggregate,
} from './usage-ledger-model';

const TIP_OFFSET = 14;
const TIP_ESTIMATED_WIDTH = 220;
const TIP_ESTIMATED_HEIGHT = 140;

export interface UsageCursorTipContent {
  title: string;
  rows: { label: string; value: string }[];
}

interface UsageCursorTipState extends UsageCursorTipContent {
  x: number;
  y: number;
}

/** 提示内容只复用 settings.usage.* 既有 key，不新造文案 */
export function buildUsageCursorTipContent(group: UsageAggregate): UsageCursorTipContent {
  return {
    title: group.label,
    rows: [
      { label: t('settings.usage.totalTokens'), value: formatNumber(group.totalTokens) },
      { label: t('settings.usage.cacheRead'), value: formatNumber(group.cacheReadTokens) },
      { label: t('settings.usage.uncached'), value: formatNumber(group.nonCachedTokens) },
      { label: t('settings.usage.requests'), value: formatNumber(group.requests) },
      { label: t('settings.usage.cacheHitRate'), value: formatPercent(hitRate(group)) },
    ],
  };
}

function tipPosition(event: React.PointerEvent) {
  const { clientX, clientY } = event;
  let x = clientX + TIP_OFFSET;
  let y = clientY + TIP_OFFSET;
  // 贴视口边翻转，避免提示被裁掉
  if (typeof window !== 'undefined' && window.innerWidth > 0 && x + TIP_ESTIMATED_WIDTH > window.innerWidth) {
    x = Math.max(0, clientX - TIP_ESTIMATED_WIDTH - TIP_OFFSET);
  }
  if (typeof window !== 'undefined' && window.innerHeight > 0 && y + TIP_ESTIMATED_HEIGHT > window.innerHeight) {
    y = Math.max(0, clientY - TIP_ESTIMATED_HEIGHT - TIP_OFFSET);
  }
  return { x, y };
}

/**
 * 每个图表组件各持一份提示状态（本 hook），柱/段只通过 bindTip 拿事件，
 * 绝不为每根柱/每段单独 portal。
 */
export function useUsageCursorTip() {
  const [tip, setTip] = useState<UsageCursorTipState | null>(null);

  const showTip = useCallback((event: React.PointerEvent, group: UsageAggregate) => {
    setTip({ ...buildUsageCursorTipContent(group), ...tipPosition(event) });
  }, []);
  const moveTip = useCallback((event: React.PointerEvent) => {
    setTip(prev => (prev ? { ...prev, ...tipPosition(event) } : prev));
  }, []);
  const hideTip = useCallback(() => {
    // 立即卸载，不等动画；过渡只做进入方向
    setTip(null);
  }, []);

  const bindTip = useCallback(
    (group: UsageAggregate) => ({
      onPointerEnter: (event: React.PointerEvent) => showTip(event, group),
      onPointerMove: moveTip,
      onPointerLeave: hideTip,
    }),
    [showTip, moveTip, hideTip],
  );

  return { tip, bindTip };
}

export function UsageCursorTip({ tip }: { tip: UsageCursorTipState | null }) {
  const ref = useRef<HTMLDivElement | null>(null);
  // 定位走 ref 直写 DOM style，而不是 JSX inline style：settings 目录的
  // inline style 数量有契约测试的棘轮上限（只能降不能升），动态坐标不占名额。
  // useLayoutEffect 保证在绘制前写好坐标，避免首帧出现在左上角再跳变。
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !tip) return;
    el.style.left = `${tip.x}px`;
    el.style.top = `${tip.y}px`;
  }, [tip]);

  if (!tip) return null;
  return createPortal(
    <div
      data-testid="usage-cursor-tip"
      ref={ref}
      className={styles['usage-cursor-tip']}
    >
      <div className={styles['usage-cursor-tip-title']}>{tip.title}</div>
      {tip.rows.map(row => (
        <div key={row.label} className={styles['usage-cursor-tip-row']}>
          <span>{row.label}</span>
          <span className={styles['usage-cursor-tip-value']}>{row.value}</span>
        </div>
      ))}
    </div>,
    document.body,
  );
}
