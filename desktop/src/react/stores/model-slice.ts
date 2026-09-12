import type { Model } from '../types';

export type ThinkingLevel = 'off' | 'auto' | 'low' | 'medium' | 'high' | 'xhigh' | 'max';
export const DEFAULT_THINKING_LEVELS: ThinkingLevel[] = ['off', 'medium', 'high'];

export function normalizeThinkingLevel(level: ThinkingLevel): ThinkingLevel {
  if (level === 'auto') return 'medium';
  if (level === 'xhigh') return 'max';
  return level;
}

export function normalizeThinkingLevels(levels: readonly ThinkingLevel[] | null | undefined): ThinkingLevel[] | null {
  if (!Array.isArray(levels)) return null;
  const normalized: ThinkingLevel[] = [];
  for (const rawLevel of levels) {
    const level = normalizeThinkingLevel(rawLevel);
    if (!normalized.includes(level)) normalized.push(level);
  }
  return normalized.length > 0 ? normalized : null;
}

export function getModelThinkingLevels(model: { thinkingLevels?: readonly ThinkingLevel[]; xhigh?: boolean } | null | undefined): ThinkingLevel[] {
  const explicit = normalizeThinkingLevels(model?.thinkingLevels);
  if (explicit) return explicit;
  return model?.xhigh ? [...DEFAULT_THINKING_LEVELS, 'max'] : [...DEFAULT_THINKING_LEVELS];
}

export interface ModelSlice {
  models: Model[];
  currentModel: { id: string; provider: string } | null;
  thinkingLevel: ThinkingLevel;
  /**
   * 按 session path 存储的思考级别（权威源）。
   * 主聊天页与侧边对话面板可同时挂载，各自从服务端读到的是本会话的级别，
   * 因此不能只写全局字段——否则一边的读取会把另一边的显示覆盖掉。
   * 全局 thinkingLevel 保留为「主聊天页兼容镜像」。
   */
  thinkingLevelBySession: Record<string, ThinkingLevel>;
  setModels: (models: Model[]) => void;
  setCurrentModel: (model: { id: string; provider: string } | null) => void;
  setThinkingLevel: (level: ThinkingLevel) => void;
  /** 指定会话写思考级别；sessionPath 为空时等价于全局写入。 */
  setThinkingLevelForSession: (sessionPath: string | null | undefined, level: ThinkingLevel) => void;
}

export const createModelSlice = (
  set: (partial: Partial<ModelSlice>) => void,
  /** zustand 的 get；用于读取「当前主会话」判定兼容镜像归属。 */
  get: () => ModelSlice & { currentSessionPath?: string | null } = () => ({ currentSessionPath: null } as ModelSlice & { currentSessionPath?: string | null })
): ModelSlice => ({
  models: [],
  currentModel: null,
  thinkingLevel: 'medium',
  thinkingLevelBySession: {},
  setModels: (models) => set({ models }),
  setCurrentModel: (model) => set({ currentModel: model }),
  setThinkingLevel: (level) => set({ thinkingLevel: normalizeThinkingLevel(level) }),
  setThinkingLevelForSession: (sessionPath, level) => {
    const normalized = normalizeThinkingLevel(level);
    if (!sessionPath) {
      set({ thinkingLevel: normalized });
      return;
    }
    const state = get();
    set({
      thinkingLevelBySession: { ...state.thinkingLevelBySession, [sessionPath]: normalized },
      // 主聊天页兼容镜像：只有目标就是当前主会话时才回写全局字段。
      ...(sessionPath === state.currentSessionPath ? { thinkingLevel: normalized } : {}),
    });
  },
});
