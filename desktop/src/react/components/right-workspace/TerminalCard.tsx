import { useEffect, useRef, useState } from 'react';
import { AnsiUp } from 'ansi_up';
import type { TerminalPublicEntry, TerminalTranscriptChunk } from '../../../../../shared/terminal-ui-contract.ts';
import { useContinuousBottomScroll } from '../../hooks/use-continuous-bottom-scroll';
import { requestTerminalSnapshot, requestTerminalTail } from '../../services/terminal-client';
import { terminalOutputStream } from '../../services/terminal-output-stream';
import { stopTerminalProcess } from '../../services/background-process-control';
import { navigateToChatCard } from '../../services/chat-card-navigation';
import { useStore } from '../../stores';
import { selectTerminals } from '../../stores/terminal-slice';
import { formatElapsed } from '../../utils/format-duration';
import styles from './TerminalCard.module.css';

function displayName(terminal: TerminalPublicEntry): string {
  return terminal.label.trim() || terminal.command.trim() || terminal.terminalId;
}

function createAnsiParser(): AnsiUp {
  const parser = new AnsiUp();
  parser.escape_html = true;
  // V1 不让终端内容创建可点击链接；URL 仍作为普通 transcript 文本处理。
  parser.url_allowlist = {};
  return parser;
}

// 预览只保留最近 N 块：与服务端 tail 默认上限（TERMINAL_TAIL_DEFAULT_MAX_CHUNKS = 500）
// 对齐，长输出不会让 React state 无界增长。丢弃的是已渲染的 HTML 字符串，
// ANSI 解析状态在 parser 实例内，不受影响。
const MAX_PREVIEW_CHUNKS = 500;

// stopping 兜底复位：3 倍于 background-process-control 的 REQUEST_TIMEOUT_MS（10s）。
const STOPPING_FALLBACK_RESET_MS = 30_000;

export function TerminalPreview({ terminal }: { terminal: TerminalPublicEntry }) {
  const [htmlChunks, setHtmlChunks] = useState<Array<{ seq: number; html: string }>>([]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const { armInstantLanding } = useContinuousBottomScroll({
    scrollRef,
    contentRef,
    active: true,
    stickyThreshold: 40,
    largeJumpPx: 400,
  });
  const t = window.t ?? ((key: string) => key);
  const terminalId = terminal.terminalId;
  const sessionId = terminal.sessionId;
  const sessionPath = terminal.sessionPath;
  const wsState = useStore((state) => state.wsState);

  useEffect(() => {
    let parser = createAnsiParser();
    setHtmlChunks([]);
    armInstantLanding();
    const ref = { terminalId, sessionId, sessionPath };
    const unsubscribe = terminalOutputStream.subscribe(ref, {
      onChunks: ({ chunks, reset }) => {
        if (reset) parser = createAnsiParser();
        const converted = chunks
          .map((chunk: TerminalTranscriptChunk) => ({
            seq: chunk.seq,
            html: parser.ansi_to_html(chunk.data),
          }))
          .filter((chunk) => chunk.html.length > 0);
        setHtmlChunks((current) => {
          const merged = reset ? converted : [...current, ...converted];
          return merged.length > MAX_PREVIEW_CHUNKS
            ? merged.slice(merged.length - MAX_PREVIEW_CHUNKS)
            : merged;
        });
      },
      onGap: ({ lastSeq }) => {
        requestTerminalTail({ terminalId, sessionId, sessionPath, sinceSeq: lastSeq });
      },
    });
    return unsubscribe;
  }, [armInstantLanding, sessionId, sessionPath, terminalId]);

  // 首个 tail 请求可能在 socket 尚未 OPEN 时落空（requestTerminalTail 返回 false 且无人
  // 重试），此后 live 块全进 pending、预览永久空白。等连接（重）建立后再发；tail 响应
  // 幂等、按 seq 去重，重连后重复请求无副作用。
  useEffect(() => {
    if (wsState !== 'connected') return;
    requestTerminalTail({ terminalId, sessionId, sessionPath });
  }, [wsState, terminalId, sessionId, sessionPath]);

  return (
    <div
      ref={scrollRef}
      className={styles.preview}
      data-testid={`terminal-preview-${terminalId}`}
      role="region"
      aria-label={t('rightWorkspace.terminal.preview')}
      tabIndex={0}
    >
      <div
        ref={contentRef}
        className={styles.output}
        data-testid={`terminal-output-${terminalId}`}
        role="log"
        aria-live="off"
      >
        {htmlChunks.map((chunk) => (
          <span
            key={chunk.seq}
            data-terminal-seq={chunk.seq}
            dangerouslySetInnerHTML={{ __html: chunk.html }}
          />
        ))}
      </div>
    </div>
  );
}

function TerminalRow({ terminal, now }: {
  terminal: TerminalPublicEntry;
  now: number;
}) {
  const t = window.t ?? ((key: string) => key);
  const [stopping, setStopping] = useState(false);
  const sessionId = terminal.sessionId;
  const stoppingFallbackTimerRef = useRef<number | null>(null);

  useEffect(() => () => {
    if (stoppingFallbackTimerRef.current !== null) window.clearTimeout(stoppingFallbackTimerRef.current);
  }, []);

  const handleStop = async () => {
    if (stopping) return;
    setStopping(true);
    try {
      await stopTerminalProcess({
        sessionId,
        sessionPath: terminal.sessionPath,
        terminalId: terminal.terminalId,
      });
      // 正常路径由权威 terminal_state 事件把这行移除；事件丢失时用超时兜底复位，
      // 避免按钮永久 disabled。几倍于 background-process-control 的 REQUEST_TIMEOUT_MS。
      stoppingFallbackTimerRef.current = window.setTimeout(
        () => setStopping(false),
        STOPPING_FALLBACK_RESET_MS,
      );
    } catch {
      useStore.getState().addToast(t('rightWorkspace.process.stopFailed'), 'error');
      setStopping(false);
    }
  };

  return (
    <div className={styles.item} data-terminal-id={terminal.terminalId}>
      <button
        type="button"
        className={styles.titleButton}
        data-terminal-row=""
        data-terminal-id={terminal.terminalId}
        data-testid={`terminal-name-${terminal.terminalId}`}
        title={displayName(terminal)}
        onClick={() => navigateToChatCard({
          kind: 'terminal',
          ids: [terminal.toolCallId, terminal.terminalId].filter((id): id is string => !!id),
          sessionPath: terminal.sessionPath,
        })}
      >
        <span className={styles.terminalIcon} aria-hidden="true">›_</span>
        <span className={styles.name}>{displayName(terminal)}</span>
      </button>
      <div className={styles.footer}>
        <span>{t('rightWorkspace.terminal.runningFor', { text: formatElapsed(now - terminal.createdAt) })}</span>
        <button type="button" className={styles.stopButton} disabled={stopping} onClick={() => void handleStop()}>
          <span aria-hidden="true">■</span>
          {stopping ? t('rightWorkspace.process.stopping') : t('rightWorkspace.terminal.stop')}
        </button>
      </div>
    </div>
  );
}

function visibleTerminals(terminals: TerminalPublicEntry[]): TerminalPublicEntry[] {
  return terminals
    .filter((terminal) => terminal.status === 'running')
    .sort((a, b) => b.createdAt - a.createdAt);
}export function TerminalCard() {
  const [now, setNow] = useState(() => Date.now());
  const sessionId = useStore((state) => state.currentSessionId);
  const sessionPath = useStore((state) => state.currentSessionPath);
  const terminals = useStore(selectTerminals(sessionPath));
  const t = window.t ?? ((key: string) => key);

  useEffect(() => {
    if (!sessionPath) return;
    requestTerminalSnapshot({ sessionId, sessionPath });
  }, [sessionId, sessionPath]);

  const visible = visibleTerminals(terminals);
  useEffect(() => {
    if (!visible.length) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [visible.length]);

  if (!visible.length) return null;

  return (
    <section className={`universal-card ${styles.card}`} aria-label={t('rightWorkspace.terminal.title')}>
      <div className={styles.header}>
        <span className={styles.title}>{t('rightWorkspace.terminal.title')}</span>
        <span className={styles.count}>{t('rightWorkspace.terminal.count', { n: visible.length })}</span>
      </div>
      <div className={styles.list}>
        {visible.map((terminal) => (
          <TerminalRow key={terminal.terminalId} terminal={terminal} now={now} />
        ))}
      </div>
    </section>
  );
}
