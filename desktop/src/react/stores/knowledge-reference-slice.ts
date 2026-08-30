import { sessionScopedKey } from './session-slice';
import type { KnowledgeReferenceMode } from '../../../../shared/knowledge-refs.ts';

export type { KnowledgeReferenceMode } from '../../../../shared/knowledge-refs.ts';

export interface KnowledgeSessionReference {
  notebookIds: string[];
  /**
   * 展示用名称缓存：toggle 时从笔记本菜单 DTO 写入。笔记本重命名后可能滞后，
   * 仅用于消息投影/chip 显示；引用功能只认 notebookIds。
   */
  notebookNames: Record<string, string>;
  mode: KnowledgeReferenceMode;
}

/**
 * 会话级知识库引用状态（仿 attachedFilesBySession 按 session 隔离，键为
 * sessionScopedKey 解析结果；pending 新会话用 HOME_DRAFT_KEY 占位）。
 *
 * 脏 id 策略：slice 不感知笔记本列表，读取时不做存在性过滤——被删除/无权限的
 * 笔记本 id 保留在状态里，由引用条 UI 兜底显示原始 id 并允许手动移除（发送时
 * 服务端按非法引用显式拒绝，不静默丢弃）。
 */
export interface KnowledgeReferenceSlice {
  knowledgeRefsBySession: Record<string, KnowledgeSessionReference>;
  toggleKnowledgeNotebook: (sessionKey: string, notebookId: string, notebookName?: string) => void;
  removeKnowledgeNotebook: (sessionKey: string, notebookId: string) => void;
  setKnowledgeReferenceMode: (sessionKey: string, mode: KnowledgeReferenceMode) => void;
  clearKnowledgeReferences: (sessionKey: string) => void;
}

const DEFAULT_MODE: KnowledgeReferenceMode = 'qa';

function resolveSessionKey(state: unknown, sessionKey: string): string {
  return sessionScopedKey(state as Parameters<typeof sessionScopedKey>[0], sessionKey) || sessionKey;
}

export function selectKnowledgeRefsForSession(
  state: KnowledgeReferenceSlice & Parameters<typeof sessionScopedKey>[0],
  sessionKey: string | null | undefined,
): KnowledgeSessionReference | null {
  if (!sessionKey) return null;
  const map = state.knowledgeRefsBySession;
  if (!map) return null;
  const key = sessionScopedKey(state, sessionKey);
  if (key && Object.prototype.hasOwnProperty.call(map, key)) return map[key];
  return Object.prototype.hasOwnProperty.call(map, sessionKey) ? map[sessionKey] : null;
}

export const createKnowledgeReferenceSlice = (
  set: (partial: Partial<KnowledgeReferenceSlice> | ((s: KnowledgeReferenceSlice) => Partial<KnowledgeReferenceSlice>)) => void,
): KnowledgeReferenceSlice => ({
  knowledgeRefsBySession: {},
  toggleKnowledgeNotebook: (sessionKey, notebookId, notebookName) =>
    set((s) => {
      const key = resolveSessionKey(s, sessionKey);
      const prev = s.knowledgeRefsBySession[key];
      const ids = prev?.notebookIds ?? [];
      const removing = ids.includes(notebookId);
      const nextIds = removing
        ? ids.filter((id) => id !== notebookId)
        : [...ids, notebookId];
      const knowledgeRefsBySession = { ...s.knowledgeRefsBySession };
      if (nextIds.length === 0) {
        delete knowledgeRefsBySession[key];
      } else {
        const notebookNames = { ...(prev?.notebookNames ?? {}) };
        if (removing) delete notebookNames[notebookId];
        else if (notebookName) notebookNames[notebookId] = notebookName;
        knowledgeRefsBySession[key] = { notebookIds: nextIds, notebookNames, mode: prev?.mode ?? DEFAULT_MODE };
      }
      if (key !== sessionKey) delete knowledgeRefsBySession[sessionKey];
      return { knowledgeRefsBySession };
    }),
  removeKnowledgeNotebook: (sessionKey, notebookId) =>
    set((s) => {
      const key = resolveSessionKey(s, sessionKey);
      const prev = s.knowledgeRefsBySession[key];
      if (!prev) return {};
      const nextIds = prev.notebookIds.filter((id) => id !== notebookId);
      const knowledgeRefsBySession = { ...s.knowledgeRefsBySession };
      if (nextIds.length === 0) delete knowledgeRefsBySession[key];
      else {
        const notebookNames = { ...prev.notebookNames };
        delete notebookNames[notebookId];
        knowledgeRefsBySession[key] = { ...prev, notebookIds: nextIds, notebookNames };
      }
      if (key !== sessionKey) delete knowledgeRefsBySession[sessionKey];
      return { knowledgeRefsBySession };
    }),
  setKnowledgeReferenceMode: (sessionKey, mode) =>
    set((s) => {
      const key = resolveSessionKey(s, sessionKey);
      const prev = s.knowledgeRefsBySession[key];
      if (!prev || prev.mode === mode) return {};
      return {
        knowledgeRefsBySession: { ...s.knowledgeRefsBySession, [key]: { ...prev, mode } },
      };
    }),
  clearKnowledgeReferences: (sessionKey) =>
    set((s) => {
      const key = resolveSessionKey(s, sessionKey);
      if (!s.knowledgeRefsBySession[key] && !s.knowledgeRefsBySession[sessionKey]) return {};
      const knowledgeRefsBySession = { ...s.knowledgeRefsBySession };
      delete knowledgeRefsBySession[key];
      if (key !== sessionKey) delete knowledgeRefsBySession[sessionKey];
      return { knowledgeRefsBySession };
    }),
});
