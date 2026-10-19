import { resolveServerConnection } from './server-connection';

/** 地址、令牌和当前身份均属于一次请求，切换连接后旧结果不能写回页面。 */
export function bridgeConnectionKey(state: Parameters<typeof resolveServerConnection>[0]): string {
  return JSON.stringify(resolveServerConnection(state));
}

/** 跨窗口只传公开连接坐标；令牌留在各窗口自己的请求身份里。 */
export function bridgePublicConnectionKey(state: Parameters<typeof resolveServerConnection>[0]): string {
  const connection = resolveServerConnection(state);
  return JSON.stringify(connection ? [connection.connectionId, connection.kind, connection.baseUrl, connection.serverId, connection.serverNodeId, connection.userId, connection.studioId] : null);
}

export async function readBridgeResponse(response: Response): Promise<Record<string, any>> {
  if (response.ok === false) throw new Error(`消息平台请求失败 (${response.status})`);
  const value: unknown = await response.json();
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('消息平台返回内容无效');
  const data = value as Record<string, any>;
  if (data.ok === false || data.error) throw new Error(typeof data.error === 'string' ? data.error : '消息平台请求未完成');
  return data;
}

export function bridgeSessions(data: Record<string, any>): any[] {
  if (!Array.isArray(data.sessions) || data.sessions.some(row => !row || typeof row.sessionKey !== 'string' || typeof row.chatId !== 'string')) throw new Error('消息平台会话列表无效');
  return data.sessions;
}
