import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { ContentBlock } from '../../stores/chat-types';
import { useStore } from '../../stores';
import { sessionIdForPathFromLocatorState } from '../../stores/session-slice';
import { AssistantContentPreview } from './AssistantContentPreview';
import { SubagentSessionPreview } from './SubagentSessionPreview';
import styles from './Chat.module.css';

type InterludeContentBlock = Extract<ContentBlock, { type: 'interlude' }>;

interface FloatingPosition {
  left: number;
  top: number;
  width: number;
  maxHeight: number;
}

function isPreviewEnabledViewport(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return true;
  return !window.matchMedia('(max-width: 720px), (pointer: coarse)').matches;
}

function useInterludePreviewEnabled(): boolean {
  const [enabled, setEnabled] = useState(isPreviewEnabledViewport);

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return;
    const query = window.matchMedia('(max-width: 720px), (pointer: coarse)');
    const update = () => setEnabled(!query.matches);
    update();
    query.addEventListener?.('change', update);
    return () => query.removeEventListener?.('change', update);
  }, []);

  return enabled;
}

function clamp(value: number, min: number, max: number): number {
  if (max < min) return min;
  return Math.min(Math.max(value, min), max);
}

function measurePopover(anchor: HTMLElement): FloatingPosition {
  const rect = anchor.getBoundingClientRect();
  const viewportW = window.innerWidth || document.documentElement.clientWidth || 1024;
  const viewportH = window.innerHeight || document.documentElement.clientHeight || 768;
  const width = Math.min(Math.max(320, viewportW * 0.6), 640, Math.max(320, viewportW - 32));
  const maxHeight = Math.min(560, Math.max(220, viewportH * 0.72));
  const left = clamp(rect.left + rect.width / 2 - width / 2, 16, viewportW - width - 16);
  const belowTop = rect.bottom + 8;
  const top = belowTop + maxHeight <= viewportH - 16
    ? belowTop
    : Math.max(16, rect.top - maxHeight - 8);
  return { left, top, width, maxHeight };
}

function streamStatusFromInterlude(status: string | undefined): 'done' | 'failed' | 'aborted' {
  if (status === 'failed') return 'failed';
  if (status === 'aborted') return 'aborted';
  return 'done';
}

const InterludeDetailPreview = memo(function InterludeDetailPreview({ detailMarkdown }: { detailMarkdown: string }) {
  return (
    <AssistantContentPreview
      content={detailMarkdown}
      className={styles.interludePopoverMarkdown}
    />
  );
});

/**
 * 循环 interlude 气泡上的控制按钮（暂停/恢复 + 停止）。
 *
 * 按钮态由该 interlude **所属会话**的循环状态驱动：running → [⏸暂停][⏹停止]，paused →
 * [▶恢复][⏹停止]，其它（completed/stopped/无）→ 不渲染。会话身份必须由渲染上下文透传
 * （ChatTranscript/AssistantMessage 的 sessionPath+agentId）——InterludeBlock 会被
 * SubagentSessionPreview/ChannelsPanel/BridgePanel/QuickChatApp 等非当前会话上下文复用，
 * 读全局 current* 会把别的会话的气泡配上主会话的按钮态，点击停止会误停主会话的循环。
 * 点击复用现有 /loop slash 通道（ws type:'slash'）。放在 interludeTrigger 内部，
 * stopPropagation 防止冒泡触发浮层切换。仅在 loop interlude 上挂载，避免给
 * deferred_result 等气泡加无用订阅。
 */
const LoopControls = memo(function LoopControls({ sessionPath, agentId }: {
  sessionPath?: string | null;
  agentId?: string | null;
}) {
  // 只按所属会话解析身份：sessionId 从 locator 反解（loopStatusBySession 以 sessionId 为键），
  // 解析不到就不渲染——宁可少显示按钮，也不能把按钮接到错的会话上。
  const sessionId = useStore((s) => (sessionPath ? sessionIdForPathFromLocatorState(s, sessionPath) : null));
  const status = useStore((s) => (sessionId ? s.loopStatusBySession[sessionId] : undefined));
  if (!status || (status.status !== 'running' && status.status !== 'paused')) return null;
  // 动态 import websocket，避免组件顶层静态依赖触发 websocket.ts 的模块加载副作用
  // （injectHandlers → stream-resume），那样会让所有 import 本组件的测试都得 mock 那条链。
  const send = async (cmd: string) => {
    const { getWebSocket } = await import('../../services/websocket');
    const ws = getWebSocket();
    if (!ws || ws.readyState !== WebSocket.OPEN || !sessionPath) return;
    ws.send(JSON.stringify({ type: 'slash', text: cmd, sessionPath, agentId: agentId || undefined }));
  };
  const resumeOrPause = status.status === 'paused'
    ? { label: '恢复循环', glyph: '▶', cmd: '/loop resume' }
    : { label: '暂停循环', glyph: '⏸', cmd: '/loop pause' };
  return (
    <span
      className={styles.loopControls}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => e.stopPropagation()}
    >
      <span className={styles.loopControlBtn} title={resumeOrPause.label} onClick={() => send(resumeOrPause.cmd)}>
        {resumeOrPause.glyph}
      </span>
      <span className={styles.loopControlBtn} title="停止循环" onClick={() => send('/loop stop')}>
        ⏹
      </span>
    </span>
  );
});

export const InterludeBlock = memo(function InterludeBlock({ block, sessionPath, agentId }: {
  block: InterludeContentBlock;
  /** 渲染该气泡的会话身份（由所属 transcript 透传）。LoopControls 据此查状态、发命令，
   *  缺失时退化为不渲染控制按钮——绝不能回退到全局 current*（见 LoopControls 注释）。 */
  sessionPath?: string | null;
  agentId?: string | null;
}) {
  const anchorRef = useRef<HTMLButtonElement | HTMLDivElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const scrollContainerRef = useRef<HTMLDivElement | null>(null);
  const previewEnabled = useInterludePreviewEnabled();
  const detailMarkdown = (block.detailMarkdown || '').trim();
  const previewSessionPath = typeof block.previewSessionPath === 'string' && block.previewSessionPath.trim()
    ? block.previewSessionPath
    : null;
  const previewSessionId = typeof block.previewSessionId === 'string' && block.previewSessionId.trim()
    ? block.previewSessionId
    : null;
  const canPreview = previewEnabled && (detailMarkdown.length > 0 || !!previewSessionPath || !!previewSessionId);
  const [position, setPosition] = useState<FloatingPosition | null>(null);

  const setAnchor = useCallback((node: HTMLButtonElement | HTMLDivElement | null) => {
    anchorRef.current = node;
  }, []);

  const setPopover = useCallback((node: HTMLDivElement | null) => {
    popoverRef.current = node;
    scrollContainerRef.current = node;
  }, []);

  const close = useCallback(() => setPosition(null), []);

  const toggle = useCallback(() => {
    if (!canPreview || !anchorRef.current) return;
    setPosition(current => current ? null : measurePopover(anchorRef.current as HTMLElement));
  }, [canPreview]);

  useEffect(() => {
    if (!position) return;
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (anchorRef.current?.contains(target) || popoverRef.current?.contains(target)) return;
      close();
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') close();
    };
    const handleScroll = (event: Event) => {
      const target = event.target;
      if (target instanceof Node && popoverRef.current?.contains(target)) return;
      close();
    };
    window.addEventListener('pointerdown', handlePointerDown, true);
    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('resize', close);
    window.addEventListener('scroll', handleScroll, true);
    return () => {
      window.removeEventListener('pointerdown', handlePointerDown, true);
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('resize', close);
      window.removeEventListener('scroll', handleScroll, true);
    };
  }, [close, position]);

  useEffect(() => {
    if (!canPreview) setPosition(null);
  }, [canPreview]);

  // loop interlude 追加"第 X / M 轮"（显示值 = min(turnCount+1, maxTurns)，封顶防超）。
  const loopTurnSuffix = block.variant === 'loop' && Number.isFinite(block.maxTurns as number)
    ? ` · 第 ${Math.min((block.turnCount ?? 0) + 1, block.maxTurns as number)}/${block.maxTurns} 轮`
    : null;
  const content = (
    <>
      <span className={styles.interludeLine} aria-hidden="true" />
      <span className={styles.interludeText}>{block.text}</span>
      {loopTurnSuffix ? <span className={styles.interludeTurn}>{loopTurnSuffix}</span> : null}
      <span className={styles.interludeLine} aria-hidden="true" />
      {block.variant === 'loop' ? <LoopControls sessionPath={sessionPath} agentId={agentId} /> : null}
    </>
  );

  return (
    <div className={styles.interludeRow} data-interlude-status={block.status || 'success'}>
      {canPreview ? (
        <button
          ref={setAnchor}
          type="button"
          className={`${styles.interludeTrigger} ${styles.interludeTriggerInteractive}`}
          onClick={toggle}
          aria-expanded={!!position}
        >
          {content}
        </button>
      ) : (
        <div ref={setAnchor} className={styles.interludeTrigger}>
          {content}
        </div>
      )}
      {position && createPortal(
        <div
          ref={setPopover}
          className={styles.interludePopover}
          style={{
            left: position.left,
            top: position.top,
            width: position.width,
            maxHeight: position.maxHeight,
          }}
          role="dialog"
        >
          {(previewSessionPath || previewSessionId) && block.taskId ? (
            <SubagentSessionPreview
              taskId={block.taskId}
              sessionId={previewSessionId}
              sessionPath={previewSessionPath}
              agentId={block.previewAgentId || null}
              streamStatus={streamStatusFromInterlude(block.status)}
              summary={block.sourceLabel || null}
              scrollContainerRef={scrollContainerRef}
            />
          ) : (
            <InterludeDetailPreview detailMarkdown={detailMarkdown} />
          )}
        </div>,
        document.body,
      )}
    </div>
  );
});
