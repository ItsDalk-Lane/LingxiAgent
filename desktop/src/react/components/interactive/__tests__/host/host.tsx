import React from 'react';
import { createRoot } from 'react-dom/client';
import { InteractiveToolsPanel } from '../../InteractiveToolsPanel';
import { useStore } from '../../../../stores';
import { createLocalServerConnection } from '../../../../services/server-connection';
const query = new URLSearchParams(location.search);
const base = query.get('service')!;
const connection = createLocalServerConnection({ serverPort: new URL(base).port, serverToken: query.get('token')!, serverNodeKind: 'lingxi-service', serverNodeTransport: 'http' })!;
useStore.setState({ serverConnections: { local: connection }, activeServerConnectionId: 'local', activeServerConnection: connection, currentSessionId: 'confirm', currentSessionPath: 'confirm' });
(window as any).__interactionTest = { select: (id: string) => useStore.setState({ currentSessionId: id, currentSessionPath: id }), switchConnection: () => useStore.setState({ activeServerConnection: { ...connection, token: 'replaced-token' }, serverConnections: { local: { ...connection, token: 'replaced-token' } } }) };
(window as any).hostSentinel = 'unchanged';
window.addEventListener('message', event => {
  if (event.data?.type === 'interaction-proof' && Array.from(document.querySelectorAll('iframe')).some(frame => frame.contentWindow === event.source)) (window as any).__cardProof = event.data;
});
createRoot(document.getElementById('root')!).render(<main style={{ maxWidth: 900, margin: '24px auto', fontFamily: 'system-ui', color: '#292622' }}><h1>会话工作区</h1><p id="host-sentinel">页面保持完整</p><InteractiveToolsPanel /><label>消息输入框<input aria-label="消息输入框" /></label></main>);
