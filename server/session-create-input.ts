import { ensureJsonObjectBody, httpJsonError, rejectWrongFieldTypes } from './hono-helpers.ts';

/** 两种创建入口共用的实际输入边界；null 沿用未提供语义，未知扩展字段保持忽略。 */
export interface SessionCreateInput {
  cwd: string | null;
  workspaceMountId: string | null;
  workspaceFolders: string[];
  memoryEnabled: boolean | null;
  agentId: string | null;
  currentAgentId: string | null;
  thinkingLevel: string | null;
  projectId: string | null;
  permissionMode: string | null;
  forkedFromSessionId: string | null;
  recordWorkspaceHistory: boolean | null;
}

export interface SessionCreateOptions {
  workspaceFolders: string[];
  visibleInSessionList: boolean;
  thinkingLevel?: string;
  workspaceMountId?: string;
  workspaceLabel?: string | null;
}
export interface DetachedSessionCreateOptions extends SessionCreateOptions {
  cwd: string | undefined;
  memoryEnabled: boolean;
  agentId: string | null;
  permissionMode: string | null;
}

export function parseSessionCreateInput(value: unknown, kind: 'focused' | 'detached'): SessionCreateInput {
  const body = ensureJsonObjectBody(value);
  rejectWrongFieldTypes(body, {
    cwd: 'string', workspaceMountId: 'string', memoryEnabled: 'boolean',
    agentId: 'string', thinkingLevel: 'string', projectId: 'string',
    ...(kind === 'focused'
      ? { currentAgentId: 'string' as const }
      : { permissionMode: 'string' as const, forkedFromSessionId: 'string' as const, recordWorkspaceHistory: 'boolean' as const }),
  });
  const folders = body.workspaceFolders;
  if (folders != null && (!Array.isArray(folders) || !folders.every((folder: unknown) => typeof folder === 'string'))) {
    throw httpJsonError(400, 'invalid_field_type', 'Field "workspaceFolders" must be an array of strings');
  }
  return {
    cwd: stringOrNull(body.cwd),
    workspaceMountId: stringOrNull(body.workspaceMountId),
    // 仅保留此前就支持的空白目录忽略规则；非字符串元素已在上面拒绝。
    workspaceFolders: Array.isArray(folders) ? folders.filter((folder: unknown): folder is string => typeof folder === 'string' && !!folder.trim()) : [],
    memoryEnabled: booleanOrNull(body.memoryEnabled),
    agentId: stringOrNull(body.agentId),
    // @ui-focus-ok: 仅校验并回传客户端当前视图标识；不从服务端焦点推断资源所有者。
    currentAgentId: kind === 'focused' ? stringOrNull(body.currentAgentId) : null,
    thinkingLevel: stringOrNull(body.thinkingLevel),
    projectId: stringOrNull(body.projectId),
    permissionMode: kind === 'detached' ? stringOrNull(body.permissionMode) : null,
    forkedFromSessionId: kind === 'detached' ? stringOrNull(body.forkedFromSessionId) : null,
    recordWorkspaceHistory: kind === 'detached' ? booleanOrNull(body.recordWorkspaceHistory) : null,
  };
}

function stringOrNull(value: unknown): string | null {
  return typeof value === 'string' ? value : null;
}
function booleanOrNull(value: unknown): boolean | null {
  return typeof value === 'boolean' ? value : null;
}
