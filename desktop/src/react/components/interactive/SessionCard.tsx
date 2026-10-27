import { useEffect, useState } from 'react';
import { ownsSessionConnection, sessionOwnerFetch, type SessionConnectionOwner } from '../../services/session-connection-owner';
export interface RuntimeCard { cardId: string; title: string; code: string }
/** 内容在服务提供的独立文档中执行；opaque-origin 沙箱不能接触聊天页面。 */
export function SessionCard({ card, owner }: { card: RuntimeCard; owner: SessionConnectionOwner }) {
  const [open, setOpen] = useState(true);
  const [src, setSrc] = useState('');
  const [error, setError] = useState('');
  useEffect(() => {
    if (!open) return;
    let alive = true; const abort = new AbortController();
    setSrc(''); setError('');
    void (async () => {
      try {
        const response = await sessionOwnerFetch(owner, '/lingxi/v1/preview/html', { method: 'POST', headers: { 'Content-Type': 'application/json' }, signal: abort.signal,
          body: JSON.stringify({ content: `<!doctype html><meta charset="utf-8"><style>body{font:15px system-ui;color:#2a2622;background:transparent;padding:12px}button{cursor:pointer}</style>${card.code}` }) });
        const value = await response.json();
        const url = new URL(value.previewUrl);
        if (url.origin !== new URL(owner.connection.baseUrl).origin || !url.pathname.startsWith('/lingxi/v1/preview/html/')) throw new Error('卡片地址无效');
        if (alive && ownsSessionConnection(owner)) setSrc(url.href);
      } catch (failure) { if (alive && ownsSessionConnection(owner)) setError(String(failure)); }
    })();
    return () => { alive = false; abort.abort(); };
  }, [card.cardId, card.code, open, owner]);
  return <section data-card-id={card.cardId} aria-label={card.title || '内容卡'}><h3>{card.title || '内容卡'}</h3>
    <button onClick={() => setOpen(value => !value)}>{open ? '收起卡片' : '重开卡片'}</button>
    {open && error && <p role="alert">{error}</p>}
    {open && src && <iframe title={card.title || '内容卡'} src={src} sandbox="allow-scripts" referrerPolicy="no-referrer" style={{ width: '100%', minHeight: 240, border: 0 }} />}
  </section>;
}
