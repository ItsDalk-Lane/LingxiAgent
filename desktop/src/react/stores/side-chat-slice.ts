import type { QuotedSelection } from './input-slice';
import { resetSessionMemoryCache } from '../components/input/composer-memory-mode';

/**
 * side-chat-slice.ts — 侧边对话（right rail side chat）状态。
 *
 * 产品语义：在聊天界面选中一段内容 → 右侧栏打开一个独立会话，带着这段引用
 * 继续追问，主对话既不被切换也不被打断。侧边会话是真实会话（服务端
 * /api/sessions/new-detached 创建），因此历史、流式、模型/思考/权限等配置
 * 全部复用会话级基础设施，侧栏关闭后仍可在左侧会话列表找回。
 *
 * 状态边界：
 * - 这里只保存「侧栏 UI + 侧边会话身份」；消息与流式状态仍按 sessionPath
 *   存在 chatSessions / streamingSessions 等会话级容器里（与主会话同源）。
 * - 一次只开一个侧边会话；再次点击「在侧边聊天中对话」会先关闭旧的（旧的
 *   只是从侧栏移除，会话本体与其记录保留在会话列表中）。
 */

export type SideChatStatus = 'idle' | 'creating' | 'ready' | 'error';

export interface SideChatState {
  open: boolean;
  /** 侧边会话 path（创建成功后写入）；null = 尚未创建完成 */
  sessionPath: string | null;
  sessionId: string | null;
  agentId: string | null;
  status: SideChatStatus;
  error: string | null;
  /** 打开侧栏时携带的引用（预置进侧边输入区，随首条消息发出） */
  seedQuote: QuotedSelection | null;
  /**
   * 侧边会话的记忆开关。默认关闭：侧边追问是主会话的旁支讨论，不写入
   * 长期记忆可避免与主会话并发写记忆互相串味；用户可在侧栏内自行打开。
   */
  memoryEnabled: boolean;
  createdAt: number;
}

export interface SideChatSlice {
  sideChat: SideChatState;
  openSideChat: (seedQuote?: QuotedSelection | null) => void;
  setSideChatSession: (session: { sessionPath: string; sessionId: string | null; agentId: string | null }) => void;
  failSideChat: (message: string) => void;
  setSideChatMemoryEnabled: (enabled: boolean) => void;
  updateSideChatSeedQuote: (updater: (quote: QuotedSelection) => QuotedSelection) => void;
  closeSideChat: () => void;
}

export const SIDE_CHAT_INITIAL_STATE: SideChatState = {
  open: false,
  sessionPath: null,
  sessionId: null,
  agentId: null,
  status: 'idle',
  error: null,
  seedQuote: null,
  memoryEnabled: false,
  createdAt: 0,
};

export const createSideChatSlice = (
  set: (partial: Partial<SideChatSlice> | ((s: SideChatSlice) => Partial<SideChatSlice>)) => void
): SideChatSlice => ({
  sideChat: { ...SIDE_CHAT_INITIAL_STATE },
  openSideChat: (seedQuote = null) =>
    set((s) => ({
      sideChat: {
        ...s.sideChat,
        open: true,
        status: 'creating',
        error: null,
        seedQuote: seedQuote ? { ...seedQuote } : null,
        createdAt: Date.now(),
      },
    })),
  setSideChatSession: ({ sessionPath, sessionId, agentId }) =>
    set((s) => ({ sideChat: { ...s.sideChat, sessionPath, sessionId, agentId, status: 'ready', error: null } })),
  failSideChat: (message) =>
    set((s) => ({ sideChat: { ...s.sideChat, status: 'error', error: message } })),
  setSideChatMemoryEnabled: (enabled) =>
    set((s) => ({ sideChat: { ...s.sideChat, memoryEnabled: enabled } })),
  updateSideChatSeedQuote: (updater) =>
    set((s) => (s.sideChat.seedQuote ? { sideChat: { ...s.sideChat, seedQuote: updater(s.sideChat.seedQuote) } } : {})),
  closeSideChat: () => {
    // 关闭侧栏即作废该会话的记忆开关缓存：下次打开重新向服务端确认。
    resetSessionMemoryCache();
    set({ sideChat: { ...SIDE_CHAT_INITIAL_STATE, createdAt: Date.now() } });
  },
});
