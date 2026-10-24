// 合成窗口只提供真实服务连接，批准页面和连接守卫均使用生产模块。
import { createRoot } from 'react-dom/client';
import RustSessionReviewPanel from '../../components/session/RustSessionReviewPanel';
import { useStore } from '../../stores';
import { createLocalServerConnection } from '../../services/server-connection';
import { getDesktopHost } from '../../host/desktop-host';
const info = await getDesktopHost().api.getServerConnectionInfo!();
const connection = createLocalServerConnection({ serverPort: info.port, serverToken: info.token, serverNodeKind: 'lingxi-service' })!;
useStore.setState({ activeServerConnection: connection, activeServerConnectionId: connection.connectionId,
  serverConnections: { [connection.connectionId]: connection }, currentSessionId: 'tool-a', currentSessionPath: 'native:tool-a', currentAgentId: 'lingxi' });
(window as typeof window & { sessionReviewFixture: { store: typeof useStore } }).sessionReviewFixture = { store: useStore };
createRoot(document.getElementById('react-root')!).render(<RustSessionReviewPanel />);
