/** 聊天页已迁移请求的唯一协议适配；未迁移路由继续明确拒绝。 */
import { lingxiFetch } from '../hooks/use-hana-fetch';
import { resolveServerConnection } from './server-connection';
import { sessionIdForPathFromLocatorState } from '../stores/session-slice';
import { useStore } from '../stores';
import { captureSessionConnection, ownsSessionConnection, sessionSelectionRevision } from './session-connection-owner';

type Options = Parameters<typeof lingxiFetch>[1];
export async function chatFetch(path: string, options: Options = {}): Promise<Response> {
  const state = useStore.getState();
  const connection = resolveServerConnection(state);
  if (connection?.serverNodeKind !== 'lingxi-service') return lingxiFetch(path, options);
  const authority = JSON.stringify(connection);
  const owner = captureSessionConnection();
  const selectionRevision = sessionSelectionRevision();
  const scope = JSON.stringify([state.currentSessionId,state.currentSessionPath,state.pendingNewSession]);
  const ownsSelection = () => { const current=useStore.getState(); return sessionSelectionRevision()===selectionRevision && JSON.stringify([current.currentSessionId,current.currentSessionPath,current.pendingNewSession])===scope; };
  const url = new URL(path, 'http://chat.invalid');
  let nextPath: string;
  let nextOptions = options;
  const sessionId = (locator: unknown): string => {
    if (typeof locator !== 'string' || !locator) throw new Error('会话身份尚未就绪');
    const id = sessionIdForPathFromLocatorState(state, locator);
    if (!id) throw new Error('无法确定本次操作所属的会话');
    return id;
  };
  switch (url.pathname) {
    case '/api/models': nextPath = '/lingxi/v1/models'; break;
    case '/api/models/set': nextPath = '/lingxi/v1/models/set'; break;
    case '/api/models/switch': {
      const body = JSON.parse(String(options.body || '{}')) as Record<string, unknown>;
      const { sessionPath, ...choice } = body;
      nextPath = '/lingxi/v1/models/switch';
      nextOptions = { ...options, body: JSON.stringify({ ...choice, sessionId: sessionId(sessionPath) }) };
      break;
    }
    case '/api/session-thinking-level': {
      nextPath = '/lingxi/v1/session-thinking-level';
      if (options.body) {
        const { sessionPath, ...body } = JSON.parse(String(options.body)) as Record<string, unknown>;
        nextOptions = { ...options, body: JSON.stringify({ ...body, ...(sessionPath ? { sessionPath: sessionId(sessionPath) } : {}) }) };
      } else if (url.searchParams.has('sessionPath')) {
        nextPath += `?sessionPath=${encodeURIComponent(sessionId(url.searchParams.get('sessionPath')))}`;
      }
      break;
    }
    default: throw new Error(`聊天服务尚未迁移此请求：${url.pathname}`);
  }
  const response = await lingxiFetch(nextPath, nextOptions);
  if (!ownsSelection() || !ownsSessionConnection(owner) || JSON.stringify(resolveServerConnection(useStore.getState())) !== authority) {
    throw new Error('连接已切换，本次旧连接结果已丢弃');
  }
  const readJson = response.json.bind(response);
  response.json = async () => {
    const value: unknown = await readJson();
    if (!ownsSelection() || !ownsSessionConnection(owner)) throw new Error('聊天目标已切换，旧回复已丢弃');
    return value;
  };
  return response;
}
