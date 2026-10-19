/**
 * InteractiveCard — show_card tool 的渲染组件
 *
 * 加载方式（关键架构）：
 *   卡片 HTML 由当前服务经 http 提供（Rust cards / 兼容Node cards），
 *   iframe 用 src 加载，**不用 srcdoc/blob**。原因：srcdoc/blob 会继承渲染进程
 *   的 `script-src 'self'` CSP，导致 iframe 内所有内联脚本（高度上报 + agent 的
 *   onclick/addEventListener）被静默阻断。独立HTTP文档使用自己的CSP，允许
 *   内联脚本与样式，同时禁止网络连接、子框架和表单；iframe保持独立沙箱。
 *
 * 数据流：
 *   挂载 → 捕获当前连接与主题 → PUT cards/:cardId登记 → 同一连接GET加载。
 *   Rust使用/lingxi/v1，Node使用/api；iframe凭证只属于捕获连接。
 *   直播与历史走同一条路（两者 block 都带 cardId+code），server 缓存为瞬态 LRU，
 *   重启后下次渲染自动回填。
 *
 * 高度机制：iframe 内 ResizeObserver 上报内容高度 → 设置显式 height；
 *   宿主在 load 后主动 ping 兜底首报竞态；上限 900px，超出后 iframe 滚动。
 */

import { memo, useCallback, useEffect, useRef, useState } from 'react';
import s from './InteractiveCard.module.css';
import type { InteractiveCardPut, InteractiveCardPutResponse } from '../../../../../contracts/generated/ts/lingxi-protocol';
import { useStore } from '../../stores';
import { buildConnectionUrl } from '../../services/server-connection';
import { captureSessionConnection, ownsSessionConnection, sessionOwnerFetch } from '../../services/session-connection-owner';

// ── 宿主注入 iframe 的主题 CSS 变量 ──
// 从 document root 的 computed style 中提取，随注册 PUT 给 server 注入到 iframe :root。
const THEME_VARS = [
  '--bg', '--bg-card', '--sidebar-bg',
  '--text', '--text-light', '--text-muted',
  '--border',
  '--accent', '--accent-hover', '--accent-light', '--accent-rgb',
  '--green', '--danger',
  '--radius-chat-card', '--radius-chat-card-inner',
  '--space-xs', '--space-sm', '--space-md', '--space-lg',
] as const;

function collectThemeVars(): string {
  const root = getComputedStyle(document.documentElement);
  return THEME_VARS
    .map(name => {
      const val = root.getPropertyValue(name).trim();
      return val ? `  ${name}: ${val};` : '';
    })
    .filter(Boolean)
    .join('\n');
}

// ── 高度 cap ──
const HEIGHT_CAP = 900;

interface InteractiveCardProps {
  block: {
    type: 'interactive_card';
    cardId: string;
    title: string;
    code: string;
  };
}

export const InteractiveCard = memo(function InteractiveCard({ block }: InteractiveCardProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [explicitHeight, setExplicitHeight] = useState<number | null>(null);
  const [loaded, setLoaded] = useState<{ owner: unknown; cardId: string; code: string; src: string } | null>(null);
  const [failedOwner, setFailedOwner] = useState<unknown>(null);
  // 连接就绪信号：作为 effect 依赖，连接建立后自动触发注册。
  const connection = useStore(st => st.activeServerConnection);
  // 单调递增版本号：code/vars 变化重注册时改变 src，强制 iframe reload。
  const verRef = useRef(0);

  // 注册卡片代码 → 拿到可加载的 http URL
  useEffect(() => {
    let cancelled = false;
    const abort = new AbortController();
    const cardId = block.cardId;
    setLoaded(null);
    setFailedOwner(null);
    const owner = captureSessionConnection();
    if (!cardId || !connection || !owner || useStore.getState().activeServerConnection !== connection) return;
    const route = `${connection.serverNodeKind === 'lingxi-service' ? '/lingxi/v1' : '/api'}/cards/${encodeURIComponent(cardId)}`;
    const varsCss = collectThemeVars();
    (async () => {
      try {
        const body: InteractiveCardPut = { code: block.code, title: block.title, varsCss };
        const response = await sessionOwnerFetch(owner, route, {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
          signal: abort.signal,
        });
        if (!response.ok) throw new Error(`card registration: ${response.status}`);
        const result: InteractiveCardPutResponse = await response.json();
        if (result.ok !== true) throw new Error('card registration unconfirmed');
        if (cancelled || !ownsSessionConnection(owner)) return;
        const base = buildConnectionUrl(owner.connection, route, { includeTokenQuery: true });
        verRef.current += 1;
        const sep = base.includes('?') ? '&' : '?';
        setLoaded({ owner: connection, cardId, code: block.code, src: `${base}${sep}v=${verRef.current}` });
      } catch (err) {
        if (!cancelled) {
          console.error('[InteractiveCard] register failed:', err);
          if (ownsSessionConnection(owner)) setFailedOwner(connection);
        }
      }
    })();
    return () => { cancelled = true; abort.abort(); };
  }, [block.cardId, block.code, block.title, connection]);

  // 连接或卡片变化的当帧即隐藏旧iframe，不等新登记完成才回收旧显示。
  const src = loaded && loaded.owner === connection && loaded.cardId === block.cardId && loaded.code === block.code ? loaded.src : null;

  // 监听 iframe 内容高度上报
  const handleMessage = useCallback((e: MessageEvent) => {
    if (e.source !== iframeRef.current?.contentWindow) return;
    if (e.data?.type !== 'hana.card-resize') return;

    const contentH = e.data.height;
    if (contentH > 0) {
      setExplicitHeight(Math.min(contentH, HEIGHT_CAP));
    }
  }, []);

  useEffect(() => {
    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, [handleMessage]);

  // iframe load 后主动 ping，确保高度上报不因竞态丢失
  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe) return;
    const onLoad = () => {
      iframe.contentWindow?.postMessage('hana.card-ping', '*');
    };
    iframe.addEventListener('load', onLoad);
    return () => iframe.removeEventListener('load', onLoad);
  }, [src]);

  const isScrollable = explicitHeight !== null && explicitHeight >= HEIGHT_CAP;

  const frameStyle: React.CSSProperties = explicitHeight != null
    ? { height: explicitHeight }
    : {};

  const frameClassName = [
    s.interactiveCardFrame,
    isScrollable ? s.interactiveCardFrameScrollable : '',
  ].filter(Boolean).join(' ');

  return (
    <div className={s.interactiveCard}>
      {failedOwner !== null && failedOwner === connection && <div role="alert">{window.t?.('devtools.loadFailed') || '加载失败'}</div>}
      <div
        className={frameClassName}
        style={frameStyle}
      >
        {src && (
          <iframe
            ref={iframeRef}
            src={src}
            sandbox="allow-scripts"
            referrerPolicy="no-referrer"
            title={block.title || 'Interactive card'}
          />
        )}
      </div>
    </div>
  );
});
