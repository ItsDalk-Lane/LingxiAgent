// 只替换窗口组合入口；输入框、Tiptap、命令发送和结果显示均使用生产模块。
import { createRoot } from 'react-dom/client';
import { InputArea } from '../../components/InputArea';
import { useStore } from '../../stores';
import { connectWebSocket } from '../../services/websocket';
import { createLocalServerConnection } from '../../services/server-connection';
import { executeSlashViaWs } from '../../components/input/slash-commands';
const info = await window.hana.getServerConnectionInfo!();
const connection = createLocalServerConnection({ serverPort: info.port, serverToken: info.token, serverNodeKind: 'lingxi-service' })!;
useStore.setState({ activeServerConnection: connection, activeServerConnectionId: connection.connectionId,
  serverConnections: { [connection.connectionId]: connection }, currentSessionId: 'desktop', currentSessionPath: 'native:desktop', currentAgentId: 'lingxi',
  sessions: [{ sessionId: 'desktop', path: 'native:desktop', agentId: 'lingxi', title: '实际命令会话' }] as never,
});
const surface = window as typeof window & { slashFixture: { store: typeof useStore; execute: (text: string) => Promise<void>; connect: typeof connectWebSocket } };
surface.slashFixture = { store: useStore, connect: connectWebSocket, execute: text => executeSlashViaWs(window.t, text.slice(1), 'lingxi', () => {}, () => {}, () => {})(text) };
createRoot(document.getElementById('react-root')!).render(<InputArea />);
