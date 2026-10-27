/** 快捷窗必须携带来源指纹；主窗实际核对会话后才确认交接。 */
import { useStore } from '../stores';
import { loadSessions, switchSession } from '../stores/session-actions';
import { captureSessionConnection, ownsSessionConnection, sessionOwnerFetch, sessionSelectionRevision } from './session-connection-owner';
import type { ServerConnection } from './server-connection';
import type { QuickChatOpenSessionRequest, QuickChatOpenSessionResult } from '../types';
let requestVersion = 0;
function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).sort(([a],[b])=>a.localeCompare(b)).map(([k,v])=>[k,canonical(v)]));
  return value;
}
/** 只传不可逆的摘要；地址、凭证与身份共同参与，界面名称不作为授权事实。 */
export async function quickChatOwnerKey(connection: ServerConnection): Promise<string> {
  const identity = [connection.connectionId, connection.kind, connection.serverId, connection.serverNodeId,
    connection.serverNodeKind, connection.serverNodeTransport, connection.userId, connection.studioId,
    connection.baseUrl, connection.wsUrl, connection.token, connection.authState, connection.trustState,
    connection.credentialKind, connection.platformAccountId, connection.officialServiceKind, canonical(connection.executionBoundary)];
  const hash=await crypto.subtle.digest('SHA-256',new TextEncoder().encode(JSON.stringify(identity)));
  return Array.from(new Uint8Array(hash),b=>b.toString(16).padStart(2,'0')).join('');
}
export async function openQuickChatSession(request: QuickChatOpenSessionRequest): Promise<QuickChatOpenSessionResult> {
  const version=++requestVersion;const owner=captureSessionConnection();const selection=sessionSelectionRevision();
  const valid=()=>version===requestVersion&&ownsSessionConnection(owner)&&Date.now()<request.expiresAt;
  if (!owner || !request.requestId || !request.sessionPath || !valid()
    || await quickChatOwnerKey(owner.connection)!==request.ownerKey || !valid()) throw Error('快捷聊天的来源连接已改变');
  const native=owner.connection.serverNodeKind==='lingxi-service';
  if (native && (!request.sessionId || request.sessionPath!==`native:${request.sessionId}`)) throw Error('快捷聊天会话标识不一致');
  // 先到真实服务核实归属和焦点。同路径的界面快捷返回不能充当存在性证明。
  const response=await sessionOwnerFetch(owner,native?'/lingxi/v1/sessions/switch':'/api/sessions/switch',{
    method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(native?{sessionId:request.sessionId}:{path:request.sessionPath,...(request.sessionId?{sessionId:request.sessionId}:{})}),
  });
  const data=await response.json();const sid=typeof data.sessionId==='string'?data.sessionId:null;
  if (!response.ok || data.error || !sid || data.path!==request.sessionPath || (request.sessionId&&sid!==request.sessionId)) throw Error('目标会话不存在或无权访问');
  if (!valid() || sessionSelectionRevision()!==selection) throw Error('会话或来源已改变，请重新打开');
  await switchSession(request.sessionPath);
  const current=useStore.getState();
  if (!valid() || current.currentSessionId!==sid || current.currentSessionPath!==request.sessionPath || current.pendingSessionSwitchPath) throw Error('主窗口尚未切换到目标会话');
  void loadSessions();
  return {ok:true,requestId:request.requestId,sessionId:sid,sessionPath:request.sessionPath,ownerKey:request.ownerKey};
}
