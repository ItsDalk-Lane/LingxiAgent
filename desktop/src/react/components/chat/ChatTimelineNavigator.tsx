import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { RefObject } from 'react';
import { TimelineRailNavigator, type TimelineRailItem } from '../shared/TimelineRailNavigator';
import type { TimelineAnchor } from './timeline-anchors';

interface MarkerLayout {
  targetTop: number;
}

interface Props {
  anchors: TimelineAnchor[];
  scrollRef: RefObject<HTMLDivElement | null>;
  contentRef: RefObject<HTMLDivElement | null>;
  messageElementsRef: RefObject<Map<string, HTMLDivElement>>;
  active: boolean;
  railVisible: boolean;
  /** 跳转前退出贴底跟随：否则流式/内容变化会把视口拽回底部，跳转看起来没反应。 */
  exitFollow?: () => void;
  /** 目标消息不在渲染窗口内（无 DOM 可测位置）时回调，交给定位管线扩窗+滚动。 */
  onLocate?: (anchor: TimelineAnchor) => void;
}

function clamp(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(Math.max(value, min), max);
}

function finiteNumber(value: unknown, fallback = 0): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function layoutsEqual(
  a: Record<string, MarkerLayout>,
  b: Record<string, MarkerLayout>,
): boolean {
  const keysA = Object.keys(a);
  const keysB = Object.keys(b);
  if (keysA.length !== keysB.length) return false;
  for (const key of keysA) {
    const layoutA = a[key];
    const layoutB = b[key];
    if (!layoutB) return false;
    if (layoutA.targetTop !== layoutB.targetTop) return false;
  }
  return true;
}

export const ChatTimelineNavigator = memo(function ChatTimelineNavigator({
  anchors,
  scrollRef,
  contentRef,
  messageElementsRef,
  active,
  railVisible,
  exitFollow,
  onLocate,
}: Props) {
  const [layouts, setLayouts] = useState<Record<string, MarkerLayout>>({});
  const [activeId, setActiveId] = useState<string | null>(null);
  const rafRef = useRef<number | null>(null);
  const measureRafRef = useRef<number | null>(null);
  const shouldMeasure = active && anchors.length > 0;

  const measure = useCallback(() => {
    const panel = scrollRef.current;
    if (!panel || anchors.length === 0) {
      setLayouts(prev => (Object.keys(prev).length === 0 ? prev : {}));
      setActiveId(null);
      return;
    }

    const maxScroll = Math.max(0, finiteNumber(panel.scrollHeight) - finiteNumber(panel.clientHeight));
    const panelRect = panel.getBoundingClientRect();
    const panelTop = finiteNumber(panelRect.top);
    const panelScrollTop = finiteNumber(panel.scrollTop);
    const next: Record<string, MarkerLayout> = {};

    for (const anchor of anchors) {
      const element = messageElementsRef.current?.get(anchor.messageId);
      if (!element) continue;
      const rect = element.getBoundingClientRect();
      const targetTop = clamp(panelScrollTop + finiteNumber(rect.top) - panelTop - 16, 0, maxScroll);
      next[anchor.messageId] = {
        targetTop,
      };
    }

    setLayouts(prev => (layoutsEqual(prev, next) ? prev : next));
  }, [anchors, messageElementsRef, scrollRef]);

  const updateActive = useCallback(() => {
    const panel = scrollRef.current;
    if (!panel || anchors.length === 0) {
      setActiveId(null);
      return;
    }

    const threshold = finiteNumber(panel.scrollTop) + 96;
    let nextId = anchors[0]?.messageId ?? null;
    for (const anchor of anchors) {
      const layout = layouts[anchor.messageId];
      if (!layout) continue;
      if (layout.targetTop <= threshold) {
        nextId = anchor.messageId;
      } else {
        break;
      }
    }
    setActiveId(nextId);
  }, [anchors, layouts, scrollRef]);

  useLayoutEffect(() => {
    if (!shouldMeasure) {
      setLayouts(prev => (Object.keys(prev).length === 0 ? prev : {}));
      setActiveId(null);
      return;
    }
    measure();
  }, [measure, shouldMeasure]);

  useEffect(() => {
    const panel = scrollRef.current;
    if (!panel || !shouldMeasure) return;
    const content = contentRef.current;
    const observer = new ResizeObserver(() => {
      if (measureRafRef.current != null) return;
      measureRafRef.current = window.requestAnimationFrame(() => {
        measureRafRef.current = null;
        measure();
      });
    });
    observer.observe(panel);
    if (content) observer.observe(content);
    return () => {
      observer.disconnect();
      if (measureRafRef.current != null) {
        window.cancelAnimationFrame(measureRafRef.current);
        measureRafRef.current = null;
      }
    };
  }, [contentRef, measure, scrollRef, shouldMeasure]);

  useEffect(() => {
    const panel = scrollRef.current;
    if (!panel || !shouldMeasure) return;

    const schedule = () => {
      if (rafRef.current != null) return;
      rafRef.current = window.requestAnimationFrame(() => {
        rafRef.current = null;
        updateActive();
      });
    };

    updateActive();
    panel.addEventListener('scroll', schedule, { passive: true });
    return () => {
      panel.removeEventListener('scroll', schedule);
      if (rafRef.current != null) {
        window.cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
    };
  }, [scrollRef, shouldMeasure, updateActive]);

  const jumpTo = useCallback((anchor: TimelineAnchor) => {
    // 与查找定位（pendingLocate 的 finishScroll）同规则：先退出贴底跟随，
    // 防止流式 ResizeObserver 立刻把视口拽回底部，跳转被夺走。
    exitFollow?.();
    const panel = scrollRef.current;
    const layout = layouts[anchor.messageId];
    if (!panel || !layout) {
      // 目标消息未挂载（渲染窗口外）：交给定位管线（扩窗 → 等元素 → 滚动）。
      onLocate?.(anchor);
      return;
    }
    panel.scrollTo({ top: layout.targetTop, behavior: 'smooth' });
  }, [exitFollow, layouts, onLocate, scrollRef]);

  // 锚点基于全部已加载消息生成；未挂载的锚点没有可测位置但不隐藏，
  // 点击时经 onLocate 走定位管线。位置测量（layouts）仍只覆盖已挂载消息。
  const railItems: Array<TimelineRailItem<TimelineAnchor>> = useMemo(
    () => anchors.map(anchor => ({
      id: anchor.messageId,
      label: anchor.label,
      markerWidthEm: anchor.markerWidthEm,
      payload: anchor,
    })),
    [anchors],
  );

  if (!active || anchors.length === 0) return null;

  return (
    <TimelineRailNavigator
      items={railItems}
      active={active}
      activeId={activeId}
      railVisible={railVisible}
      ariaLabel={window.t?.('chat.timeline.navAriaLabel') || 'Turn navigation'}
      jumpLabel={item => (window.t?.('chat.timeline.jumpTo') || 'Jump to {label}').replace('{label}', item.label)}
      onJump={item => jumpTo(item.payload)}
    />
  );
});
