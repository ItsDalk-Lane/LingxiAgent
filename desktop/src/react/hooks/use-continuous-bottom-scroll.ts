import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  type MutableRefObject,
  type RefObject,
} from 'react';

type ScrollMode = 'instant' | 'follow' | 'smooth';

interface ScrollToBottomOptions {
  mode?: ScrollMode;
  forceSticky?: boolean;
}

interface UseContinuousBottomScrollOptions {
  scrollRef: RefObject<HTMLElement | null>;
  contentRef?: RefObject<HTMLElement | null>;
  active?: boolean;
  stickyThreshold?: number;
  largeJumpPx?: number;
}

export interface ContinuousBottomScrollController {
  isStickyRef: MutableRefObject<boolean>;
  checkSticky: () => boolean;
  markSticky: () => void;
  cancelFollow: () => void;
  followBottom: () => void;
  scrollToBottom: (options?: ScrollToBottomOptions) => void;
  /**
   * Arm a one-shot "instant landing": the very next bottom-follow (whether driven by the
   * ResizeObserver content callback or an explicit `followBottom()` call) snaps straight to the
   * bottom instead of animating, then disarms. Use this when switching/hydrating a session so the
   * first fill lands without a visible scroll animation, while subsequent streaming growth keeps
   * the smooth follow. Honors sticky: if the user has scrolled up, nothing moves.
   */
  armInstantLanding: () => void;
}

const DEFAULT_STICKY_THRESHOLD = 48;
const DEFAULT_LARGE_JUMP_PX = 720;
const FOLLOW_TIME_CONSTANT_MS = 85;
const SCROLL_EPSILON_PX = 0.5;

function finiteNumber(value: unknown, fallback = 0): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function maxScrollTop(el: HTMLElement): number {
  return Math.max(0, finiteNumber(el.scrollHeight) - finiteNumber(el.clientHeight));
}

function distanceFromBottom(el: HTMLElement): number {
  return maxScrollTop(el) - finiteNumber(el.scrollTop);
}

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false;
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

export function useContinuousBottomScroll({
  scrollRef,
  contentRef,
  active = true,
  stickyThreshold = DEFAULT_STICKY_THRESHOLD,
  largeJumpPx = DEFAULT_LARGE_JUMP_PX,
}: UseContinuousBottomScrollOptions): ContinuousBottomScrollController {
  const isStickyRef = useRef(true);
  const activeRef = useRef(active);
  const thresholdRef = useRef(stickyThreshold);
  const largeJumpRef = useRef(largeJumpPx);
  const rafRef = useRef<number | null>(null);
  const lastFrameTimeRef = useRef<number | null>(null);
  const followingRef = useRef(false);
  const instantLandingArmedRef = useRef(false);
  const programmaticScrollTopRef = useRef<number | null>(null);
  const lastObservedScrollHeightRef = useRef<number | null>(null);
  // 指针按压窗口标记：按下到抬起之间冻结跟随动画（见 onPointerDown）。
  const pointerHoldRef = useRef(false);

  activeRef.current = active;
  thresholdRef.current = stickyThreshold;
  largeJumpRef.current = largeJumpPx;

  const stopFollow = useCallback(() => {
    if (rafRef.current !== null) {
      window.cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    followingRef.current = false;
    lastFrameTimeRef.current = null;
  }, []);

  const setProgrammaticScrollTop = useCallback((el: HTMLElement, value: number) => {
    const safeValue = finiteNumber(value);
    programmaticScrollTopRef.current = safeValue;
    el.scrollTop = safeValue;
  }, []);

  const checkSticky = useCallback(() => {
    if (followingRef.current) return isStickyRef.current;
    const el = scrollRef.current;
    if (!el) return isStickyRef.current;
    const sticky = distanceFromBottom(el) <= finiteNumber(thresholdRef.current, DEFAULT_STICKY_THRESHOLD);
    isStickyRef.current = sticky;
    return sticky;
  }, [scrollRef]);

  const markSticky = useCallback(() => {
    isStickyRef.current = true;
  }, []);

  const cancelFollow = useCallback(() => {
    stopFollow();
    isStickyRef.current = false;
    instantLandingArmedRef.current = false;
  }, [stopFollow]);

  const armInstantLanding = useCallback(() => {
    instantLandingArmedRef.current = true;
  }, []);

  const runFrame = useCallback((time: number) => {
    const el = scrollRef.current;
    if (!el || !activeRef.current || !isStickyRef.current) {
      stopFollow();
      return;
    }

    const target = maxScrollTop(el);
    const current = finiteNumber(el.scrollTop);
    const delta = target - current;

    if (Math.abs(delta) <= SCROLL_EPSILON_PX) {
      setProgrammaticScrollTop(el, target);
      stopFollow();
      return;
    }

    if (delta < 0) {
      stopFollow();
      checkSticky();
      return;
    }

    // 按压冻结：按下期间只推进时间基准，不动 scrollTop；松手后按新时刻继续
    // 平滑追赶（时间基准持续刷新，避免恢复瞬间 dt 猛增造成跳变）。
    if (pointerHoldRef.current) {
      lastFrameTimeRef.current = finiteNumber(time, lastFrameTimeRef.current ?? time);
      rafRef.current = window.requestAnimationFrame(runFrame);
      return;
    }

    const largeJump = finiteNumber(largeJumpRef.current, DEFAULT_LARGE_JUMP_PX);
    if (delta > largeJump || prefersReducedMotion()) {
      setProgrammaticScrollTop(el, target);
      stopFollow();
      return;
    }

    followingRef.current = true;
    const safeTime = finiteNumber(time, finiteNumber(lastFrameTimeRef.current, 0) + 16);
    const previous = finiteNumber(lastFrameTimeRef.current, safeTime - 16);
    const dt = Math.max(1, safeTime - previous);
    lastFrameTimeRef.current = safeTime;
    const rawAlpha = 1 - Math.exp(-dt / FOLLOW_TIME_CONSTANT_MS);
    const alpha = Number.isFinite(rawAlpha) ? rawAlpha : 1;
    setProgrammaticScrollTop(el, current + delta * alpha);
    rafRef.current = window.requestAnimationFrame(runFrame);
  }, [checkSticky, scrollRef, setProgrammaticScrollTop, stopFollow]);

  const followBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el || !activeRef.current || !isStickyRef.current) return;

    const target = maxScrollTop(el);
    const delta = target - finiteNumber(el.scrollTop);
    if (Math.abs(delta) <= SCROLL_EPSILON_PX) {
      // Already at bottom: a no-op follow must NOT consume an armed instant landing — the arm is
      // reserved for the first *meaningful* growth (the async hydrate after a switch).
      setProgrammaticScrollTop(el, target);
      return;
    }

    if (delta < 0) {
      stopFollow();
      checkSticky();
      return;
    }

    // First fill after a session switch/hydrate: snap to bottom synchronously instead of
    // animating from a mid position, then disarm so subsequent streaming growth animates normally.
    if (instantLandingArmedRef.current) {
      instantLandingArmedRef.current = false;
      setProgrammaticScrollTop(el, target);
      stopFollow();
      return;
    }

    const largeJump = finiteNumber(largeJumpRef.current, DEFAULT_LARGE_JUMP_PX);
    if (delta > largeJump || prefersReducedMotion()) {
      setProgrammaticScrollTop(el, target);
      stopFollow();
      return;
    }

    if (rafRef.current !== null) return;
    lastFrameTimeRef.current = null;
    followingRef.current = true;
    rafRef.current = window.requestAnimationFrame(runFrame);
  }, [checkSticky, runFrame, scrollRef, stopFollow]);

  const scrollToBottom = useCallback((options: ScrollToBottomOptions = {}) => {
    const el = scrollRef.current;
    if (!el) return;
    if (options.forceSticky) markSticky();
    stopFollow();

    const mode = options.mode ?? 'instant';
    if (mode === 'instant') {
      setProgrammaticScrollTop(el, maxScrollTop(el));
      return;
    }
    followBottom();
  }, [followBottom, markSticky, scrollRef, setProgrammaticScrollTop, stopFollow]);

  useEffect(() => {
    if (!active) stopFollow();
  }, [active, stopFollow]);

  useEffect(() => stopFollow, [stopFollow]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !active) return undefined;

    const onScroll = () => {
      if (followingRef.current) {
        const expected = programmaticScrollTopRef.current;
        const current = finiteNumber(el.scrollTop);
        if (expected !== null && Math.abs(current - expected) <= 1) return;
        // A scroll event whose scrollHeight already differs from what we last observed cannot be
        // user intent (a wheel/touch/keyboard scroll is cancelled explicitly elsewhere, and a
        // scrollbar drag never changes content height). It's the browser clamping/anchoring the
        // scroll position in reaction to a same-frame content-size change (e.g. process-fold
        // collapse or typing-indicator removal at turn end) — ignore it and let the
        // ResizeObserver branch below handle the resulting follow.
        const lastObserved = lastObservedScrollHeightRef.current;
        if (lastObserved !== null
          && Math.abs(finiteNumber(el.scrollHeight) - lastObserved) > SCROLL_EPSILON_PX) {
          return;
        }
        if (distanceFromBottom(el) > finiteNumber(thresholdRef.current, DEFAULT_STICKY_THRESHOLD)) {
          cancelFollow();
          return;
        }
      }
      checkSticky();
    };
    const onWheel = (event: WheelEvent) => {
      if (event.deltaY < 0) cancelFollow();
    };
    const onTouchStart = () => cancelFollow();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'ArrowUp' || event.key === 'PageUp' || event.key === 'Home') {
        cancelFollow();
      }
    };

    // 按压暂停：浏览器只在按下与抬起命中同一元素时才派发 click；流式期间逐帧
    // 追底会让卡片从光标下滑走，点击批量落空。主键按下窗口内冻结跟随（只暂停
    // 不取消，贴底状态保持），松手/取消后从当前时刻恢复追赶。滚轮、触摸、键盘
    // 的既有接管路径不受影响；滚动条拖拽不经由内容区的 pointerdown，天然无关。
    const releasePointerHold = () => {
      if (!pointerHoldRef.current) return;
      pointerHoldRef.current = false;
      window.removeEventListener('pointerup', releasePointerHold);
      window.removeEventListener('pointercancel', releasePointerHold);
    };
    const onPointerDown = (event: PointerEvent) => {
      if (event.button !== 0) return;
      if (pointerHoldRef.current) return;
      pointerHoldRef.current = true;
      window.addEventListener('pointerup', releasePointerHold);
      window.addEventListener('pointercancel', releasePointerHold);
    };

    el.addEventListener('scroll', onScroll, { passive: true });
    el.addEventListener('wheel', onWheel, { passive: true });
    el.addEventListener('touchstart', onTouchStart, { passive: true });
    el.addEventListener('keydown', onKeyDown);
    el.addEventListener('pointerdown', onPointerDown);
    onScroll();

    return () => {
      el.removeEventListener('scroll', onScroll);
      el.removeEventListener('wheel', onWheel);
      el.removeEventListener('touchstart', onTouchStart);
      el.removeEventListener('keydown', onKeyDown);
      el.removeEventListener('pointerdown', onPointerDown);
      releasePointerHold();
    };
  }, [active, cancelFollow, checkSticky, scrollRef]);

  useLayoutEffect(() => {
    const ResizeObserverImpl = window.ResizeObserver;
    const el = scrollRef.current;
    const target = contentRef?.current ?? scrollRef.current;
    if (!active || !el || !target || !ResizeObserverImpl) return undefined;

    lastObservedScrollHeightRef.current = finiteNumber(el.scrollHeight);

    const observer = new ResizeObserverImpl(() => {
      const previousScrollHeight = lastObservedScrollHeightRef.current;
      const nextScrollHeight = finiteNumber(el.scrollHeight);
      lastObservedScrollHeightRef.current = nextScrollHeight;

      if (
        previousScrollHeight !== null
        && nextScrollHeight < previousScrollHeight - SCROLL_EPSILON_PX
      ) {
        // Content shrank (e.g. process-fold collapse / typing-indicator removal at turn end).
        // If we were actively sticky, keep following to the new bottom instead of dropping out —
        // the user hadn't scrolled away, the content just got shorter underneath them.
        if (isStickyRef.current) {
          // If a follow animation is already mid-flight, let it be: its own runFrame will settle
          // against the new (smaller) target on its next tick, and followBottom()'s reentrancy
          // guard (rafRef.current !== null) would no-op here anyway. This preserves "don't force
          // an upward snap mid-animation" — a shrink that lands behind an in-flight animation's
          // current position shouldn't yank the view backwards mid-motion.
          if (followingRef.current) {
            followBottom();
            return;
          }
          // Otherwise we were at rest (already caught up to the old bottom). The shrink can leave
          // scrollTop resting past the new maxScrollTop — a real browser clamps that overscroll
          // natively and instantly, so mirror that here instead of waiting for unrelated future
          // growth to paper over the drift.
          const stickyEl = scrollRef.current;
          if (stickyEl) setProgrammaticScrollTop(stickyEl, maxScrollTop(stickyEl));
          return;
        }
        stopFollow();
        checkSticky();
        return;
      }

      followBottom();
    });
    observer.observe(target);
    return () => observer.disconnect();
  }, [active, checkSticky, contentRef, followBottom, scrollRef, setProgrammaticScrollTop, stopFollow]);

  return useMemo(() => ({
    isStickyRef,
    checkSticky,
    markSticky,
    cancelFollow,
    followBottom,
    scrollToBottom,
    armInstantLanding,
  }), [armInstantLanding, cancelFollow, checkSticky, followBottom, markSticky, scrollToBottom]);
}
