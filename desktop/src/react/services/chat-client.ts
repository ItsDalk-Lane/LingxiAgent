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
  const scope = JSON.stringify([state.currentSessionId,state.currentSessionPath,state.pendingNewSession,state.currentAgentId,state.selectedAgentId]);
  const ownsSelection = () => { const current=useStore.getState(); return sessionSelectionRevision()===selectionRevision && JSON.stringify([current.currentSessionId,current.currentSessionPath,current.pendingNewSession,current.currentAgentId,current.selectedAgentId])===scope; };
  const url = new URL(path, 'http://chat.invalid');
  let nextPath: string;
  let nextOptions = options;
  let projectJson: ((value: unknown) => unknown) | null = null;
  const sessionId = (locator: unknown): string => {
    if (typeof locator !== 'string' || !locator) throw new Error('会话身份尚未就绪');
    const id = sessionIdForPathFromLocatorState(state, locator);
    if (!id) throw new Error('无法确定本次操作所属的会话');
    return id;
  };
  switch (url.pathname) {
    case '/api/upload-blob': {
      const body = JSON.parse(String(options.body || '{}')) as Record<string, unknown>;
      const id = sessionId(body.sessionPath);
      if (body.sessionId && body.sessionId !== id) throw new Error('附件会话身份不一致');
      nextPath = `/lingxi/v1/sessions/${encodeURIComponent(id)}/attachments/blob`;
      nextOptions = {...options, body: JSON.stringify({name: body.name, mime: body.mimeType, dataBase64: body.base64Data})};
      projectJson = value => {
        if (!response.ok) return value;
        const file = (value as {file?: {fileId?: unknown; sessionId?: unknown; realPath?: unknown; filename?: unknown; label?: unknown}})?.file;
        if (!file || file.sessionId !== id || typeof file.fileId !== 'string' || typeof file.realPath !== 'string' || typeof file.filename !== 'string') throw new Error('附件身份响应无效');
        return {uploads: [{fileId: file.fileId, dest: file.realPath, name: typeof body.name === 'string' && body.name ? body.name : typeof file.label === 'string' ? file.label : file.filename}]};
      };
      break;
    }
    case '/api/models': {
      const query = new URLSearchParams();
      const agentId = state.pendingNewSession ? state.selectedAgentId || state.currentAgentId : state.currentAgentId;
      if (agentId) query.set('agentId', agentId);
      if (!state.pendingNewSession && state.currentSessionId) query.set('sessionId', state.currentSessionId);
      nextPath = `/lingxi/v1/chat/models${query.size ? `?${query}` : ''}`;
      break;
    }
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
    return projectJson ? projectJson(value) : value;
  };
  return response;
}

/** 异步读剪贴板或文件之前绑定目标，防止读完后误投递到新连接。 */
export function captureChatUploadScope(sessionPath: string | null | undefined): () => void {
  if (resolveServerConnection(useStore.getState())?.serverNodeKind !== 'lingxi-service') return () => {};
  const owner = captureSessionConnection();
  if (owner?.connection.serverNodeKind !== 'lingxi-service') return () => {};
  const revision = sessionSelectionRevision();
  const state = useStore.getState();
  const side = sessionPath && state.sideChat.sessionPath === sessionPath ? state.sideChat.createdAt : null;
  return () => {
    const current = useStore.getState();
    if (!ownsSessionConnection(owner) || sessionSelectionRevision() !== revision || (side !== null
      ? !current.sideChat.open || current.sideChat.sessionPath !== sessionPath || current.sideChat.createdAt !== side
      : current.currentSessionPath !== sessionPath)) throw new Error('聊天目标已切换，旧附件已丢弃');
  };
}
