/** 快捷窗复用正式聊天连接/发送器，只适配已有服务入口。 */
import type { ServerConnection } from '../services/service-client';
import { useStore } from '../stores';
import { defaultComposerSendFlowDeps, sendWithLease, tryAcquireSendLease } from '../services/composer-send-coordinator';
import type { AttachedFile } from '../stores/input-slice';
export function quickChatRoute(connection: ServerConnection, path: string): string {
  if (connection.serverNodeKind !== 'lingxi-service') return path;
  if (path === '/api/health') return '/lingxi/v1/settings/avatar-health';
  if (path === '/api/config') return '/lingxi/v1/settings/config';
  if (path === '/api/sessions/new-detached') return '/lingxi/v1/sessions/new-detached-context';
  if (/^\/api\/(?:agents|models|preferences\/(?:quick-chat|models|session-permission-default))(?:[/?]|$)/.test(path)) return `/lingxi/v1${path.slice(4)}`;
  throw Error('当前服务没有接通该快捷聊天操作');
}
export async function sendQuickChatRustMessage(path: string, text: string, files: AttachedFile[]): Promise<void> {
  const state=useStore.getState();const sid=path.startsWith('native:')?path.slice(7):null;
  if (!sid || state.currentSessionId!==sid || state.currentSessionPath!==path) throw Error('快捷聊天会话已改变');
  const sessionRef={sessionId:sid,sessionPath:path,agentId:state.currentAgentId};
  const bundle={type:'prompt' as const,sessionRef,text,skills:[],fileRefs:[],sessionRefs:[],agentMentions:[],inputFiles:files,knowledgeRefs:null,docContextAttached:false,doc:null,quotes:[],uiContext:null};
  const lease=tryAcquireSendLease({identity:{kind:'session',...sessionRef},bundle,queueItemId:null,snapshotVersion:1,composerRevisionAtClick:null,targetRun:null});
  if (!lease.ok) throw Error('该会话已有正在提交的内容，请稍后重试');
  const result=await sendWithLease(lease.leaseId,defaultComposerSendFlowDeps());
  if (result.kind!=='transport_submitted') throw Error('快捷聊天内容尚未确认发送，请检查连接后重试');
}
