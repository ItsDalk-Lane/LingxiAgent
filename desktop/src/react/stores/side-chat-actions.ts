import { useStore } from './index';
import type { QuotedSelection } from './input-slice';
import { lingxiFetch } from '../hooks/use-hana-fetch';
import { noteSessionMemoryEnabled } from '../components/input/composer-memory-mode';

/**
 * side-chat-actions.ts — 侧边对话的创建与引用投递。
 *
 * 用户动作只有两个：
 *   1. 选中聊天内容 → 浮层「在侧边聊天中对话」→ openSelectionInSideChat()
 *   2. 侧栏内关闭 → closeSideChat()
 *
 * 会话创建走服务端 /api/sessions/new-detached：它在服务端是「建会话但不切换
 * 焦点」的语义（createDetachedSession 保存并恢复 _session/_currentSessionPath），
 * 因此主对话的运行时焦点不会被侧边会话抢走。
 */

/** 从当前会话投影里取侧边会话应当继承的工作台身份。 */
function inheritWorkspaceBody(): Record<string, unknown> {
  const state = useStore.getState();
  const current = state.currentSessionPath
    ? state.sessions.find(session => session.path === state.currentSessionPath)
    : null;
  const mountId = typeof current?.workspaceMountId === 'string' && current.workspaceMountId.trim()
    ? current.workspaceMountId.trim()
    : null;
  const body: Record<string, unknown> = {};
  if (mountId) {
    // mount 工作台：服务端要求 cwd 与 workspaceMountId 互斥，只传 mount。
    body.workspaceMountId = mountId;
    if (typeof current?.workspaceLabel === 'string' && current.workspaceLabel.trim()) {
      body.workspaceLabel = current.workspaceLabel.trim();
    }
    return body;
  }
  const cwd = typeof current?.cwd === 'string' && current.cwd.trim() ? current.cwd.trim() : null;
  if (cwd) body.cwd = cwd;
  return body;
}

/**
 * 为侧边会话准备首条引用：把「引用到对话」的候选投递到侧边输入区。
 * 引用随输入区一起等待用户补充正文，不自动发送。
 */
function seedSideChatComposer(sessionPath: string, quote: QuotedSelection | null): void {
  const state = useStore.getState();
  if (!quote) return;
  state.addQuotedSelectionForSession(sessionPath, quote);
  // 草稿区留空但把光标请求给到侧边输入区，用户可以直接继续打字。
  state.requestInputFocus('gesture');
}

/** 侧边会话创建：失败时保留面板并给出可读错误，不静默回落主会话。 */
export async function ensureSideChatSession(openedAt: number): Promise<void> {
  const initial = useStore.getState();
  if (initial.sideChat.sessionPath) return;
  if (!initial.sideChat.open) return;

  try {
    const body: Record<string, unknown> = {
      ...inheritWorkspaceBody(),
      memoryEnabled: initial.sideChat.memoryEnabled === true,
      recordWorkspaceHistory: false,
    };
    const agentId = initial.currentAgentId || null;
    if (agentId) body.agentId = agentId;
    if (Array.isArray(initial.workspaceFolders) && initial.workspaceFolders.length) {
      body.workspaceFolders = initial.workspaceFolders;
    }

    const res = await lingxiFetch('/api/sessions/new-detached', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
      throwOnHttpError: false,
    });
    const data = await res.json();
    if (!res.ok || data?.error || !data?.path) {
      throw new Error(data?.error || res.statusText || 'side chat session creation failed');
    }

    const sessionPath = String(data.path);
    const sessionId = typeof data.sessionId === 'string' && data.sessionId.trim() ? data.sessionId.trim() : null;
    const agentIdResolved = typeof data.agentId === 'string' && data.agentId.trim()
      ? data.agentId.trim()
      : (agentId || null);

    const latest = useStore.getState();
    // 面板在等待期间被关掉（或又被打开成新的一次）：这次创建结果不再属于当前视图。
    if (!latest.sideChat.open || latest.sideChat.createdAt !== openedAt) return;

    const seedQuote = latest.sideChat.seedQuote;
    latest.setSideChatSession({ sessionPath, sessionId, agentId: agentIdResolved });
    // 建会话请求里的 memoryEnabled 就是该会话的初始值，直接写缓存省一次读请求。
    noteSessionMemoryEnabled(sessionPath, latest.sideChat.memoryEnabled === true);
    // 会话投影先落地，侧栏渲染与后续 WS 事件才有可用的身份（与 ensureSession 的
    // 处理一致：不预种空 items 缓存，历史交给 loadMessages）。
    useStore.setState((state: any) => ({
      sessions: [
        {
          path: sessionPath,
          sessionId,
          title: null,
          firstMessage: '',
          modified: new Date().toISOString(),
          messageCount: 0,
          agentId: agentIdResolved,
          agentName: data.agentName || null,
          cwd: typeof data.cwd === 'string' ? data.cwd : null,
          workspaceMountId: typeof data.workspaceMountId === 'string' ? data.workspaceMountId : null,
          workspaceLabel: typeof data.workspaceLabel === 'string' ? data.workspaceLabel : null,
          _optimistic: true,
        },
        ...(state.sessions || []).filter((item: any) => item?.path !== sessionPath),
      ],
      ...(sessionId
        ? { sessionLocatorsById: { ...(state.sessionLocatorsById || {}), [sessionId]: { path: sessionPath } } }
        : {}),
      sideChatSeedQuote: null,
    }));
    seedSideChatComposer(sessionPath, seedQuote);
  } catch (err) {
    const latest = useStore.getState();
    if (!latest.sideChat.open || latest.sideChat.createdAt !== openedAt) return;
    latest.failSideChat(err instanceof Error ? err.message : String(err));
  }
}

/**
 * 选中内容 → 侧边对话。返回创建的 Promise，调用方可以不等待
 * （浮层按钮点击后立刻收起，面板自己显示创建中/失败态）。
 */
export function openSelectionInSideChat(quote?: QuotedSelection | null): Promise<void> {
  const state = useStore.getState();
  // 一次只开一个侧边会话：旧侧边会话只是从侧栏移除，其会话本体与记录保留。
  if (state.sideChat.open) state.closeSideChat();
  const selected = quote ?? state.quoteCandidate;
  state.openSideChat(selected ? { ...selected } : null);
  if (selected) state.clearQuoteCandidate();
  const openedAt = useStore.getState().sideChat.createdAt;
  return ensureSideChatSession(openedAt);
}
