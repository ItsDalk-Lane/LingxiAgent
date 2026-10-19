import type { McpSessionPermissionRequest, McpSessionPermissionResponse } from '../../../../contracts/generated/ts/lingxi-protocol';
import { ownsSessionConnection, sessionOwnerFetch, type SessionConnectionOwner } from './session-connection-owner';

/** 授权和确认分开；只接受当前连接真实目录给出的目标和能力。 */
export async function grantRustMcpSessionCapability(owner: SessionConnectionOwner, request: McpSessionPermissionRequest): Promise<void> {
  if (owner.connection.serverNodeKind !== 'lingxi-service' || !ownsSessionConnection(owner)) throw new Error('会话服务连接已改变');
  const response = await sessionOwnerFetch(owner, '/lingxi/v1/mcp/session-permissions', {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(request),
  });
  const data: McpSessionPermissionResponse = await response.json();
  if (!ownsSessionConnection(owner)) throw new Error('连接已改变，旧会话授权结果已忽略');
  if (!data || data.ok !== true || data.persisted !== false || data.sessionId !== request.sessionId
    || data.targetId !== request.targetId || data.capability !== request.capability) throw new Error('会话能力授权回执不匹配');
}
