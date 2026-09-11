// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { openSelectionInSideChat } from '../../stores/side-chat-actions';
import type { QuotedSelection } from '../../stores/input-slice';

function quote(overrides: Partial<QuotedSelection> = {}): QuotedSelection {
  return {
    text: '被选中的一段回答',
    sourceTitle: 'Assistant message',
    sourceKind: 'chat',
    sourceSessionPath: '/session/main.jsonl',
    sourceMessageId: 'assistant-1',
    sourceRole: 'assistant',
    charCount: 8,
    ...overrides,
  };
}

function jsonResponse(body: unknown, ok = true) {
  return {
    ok,
    status: ok ? 200 : 500,
    statusText: ok ? 'OK' : 'Internal Server Error',
    json: async () => body,
  } as unknown as Response;
}

describe('side chat open', () => {
  beforeEach(() => {
    useStore.getState().closeSideChat();
    useStore.setState({
      serverPort: 3210,
      serverToken: null,
      activeServerConnection: null,
      activeServerConnectionId: null,
      currentSessionPath: '/session/main.jsonl',
      currentSessionId: 'sess_main',
      currentAgentId: 'hana',
      agentName: 'Hana',
      sessions: [{
        path: '/session/main.jsonl',
        sessionId: 'sess_main',
        agentId: 'hana',
        agentName: 'Hana',
        cwd: '/workspace/demo',
      }],
      sessionLocatorsById: { sess_main: { path: '/session/main.jsonl' } },
      quotedSelections: [],
      quotedSelectionsBySession: {},
      quoteCandidate: null,
      workspaceFolders: [],
    } as never);
  });

  it('creates a detached session without touching the main session focus', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      ok: true,
      path: '/session/side.jsonl',
      sessionId: 'sess_side',
      sessionPath: '/session/side.jsonl',
      agentId: 'hana',
      agentName: 'Hana',
      cwd: '/workspace/demo',
      permissionMode: 'ask',
    }));
    vi.stubGlobal('fetch', fetchMock);

    await openSelectionInSideChat(quote());

    const state = useStore.getState();
    expect(fetchMock).toHaveBeenCalledOnce();
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toContain('/api/sessions/new-detached');
    expect(url).toBe('http://127.0.0.1:3210/api/sessions/new-detached');
    const body = JSON.parse(String(init.body));
    expect(body).toMatchObject({
      cwd: '/workspace/demo',
      agentId: 'hana',
      memoryEnabled: false,
      recordWorkspaceHistory: false,
    });

    expect(state.sideChat).toMatchObject({
      open: true,
      status: 'ready',
      sessionPath: '/session/side.jsonl',
      sessionId: 'sess_side',
      error: null,
    });
    // 主会话完全没有被切换。
    expect(state.currentSessionPath).toBe('/session/main.jsonl');
    expect(state.currentSessionId).toBe('sess_main');
    // 引用投递到侧边会话的桶，主会话的引用列表保持干净。
    expect(state.quotedSelections).toEqual([]);
    expect(state.quotedSelectionsBySession.sess_side).toHaveLength(1);
    expect(state.quotedSelectionsBySession.sess_side?.[0]).toMatchObject({ text: '被选中的一段回答' });
    expect(state.quoteCandidate).toBeNull();
    // 会话投影先落地，侧栏渲染与 WS 事件才有身份可用。
    expect(state.sessions.some(session => session.path === '/session/side.jsonl')).toBe(true);
    vi.unstubAllGlobals();
  });

  it('keeps the panel open with an error state when creation fails', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: 'detached session creation unavailable' }, false)));

    await openSelectionInSideChat(quote());

    const state = useStore.getState();
    expect(state.sideChat.open).toBe(true);
    expect(state.sideChat.status).toBe('error');
    expect(state.sideChat.error).toContain('detached session creation unavailable');
    expect(state.sideChat.sessionPath).toBeNull();
    expect(state.currentSessionPath).toBe('/session/main.jsonl');
    vi.unstubAllGlobals();
  });

  it('inherits the mount workspace as workspaceMountId when the main session uses one', async () => {
    useStore.setState({
      sessions: [{
        path: '/session/main.jsonl',
        sessionId: 'sess_main',
        agentId: 'hana',
        workspaceMountId: 'studio:notes',
        workspaceLabel: 'Notes',
        cwd: '/workspace/demo',
      }],
    } as never);
    const fetchMock = vi.fn(async () => jsonResponse({
      ok: true,
      path: '/session/side.jsonl',
      sessionId: 'sess_side',
      agentId: 'hana',
    }));
    vi.stubGlobal('fetch', fetchMock);

    await openSelectionInSideChat(quote());

    const body = JSON.parse(String((fetchMock.mock.calls[0] as unknown as [string, RequestInit])[1].body));
    expect(body.workspaceMountId).toBe('studio:notes');
    expect(body.workspaceLabel).toBe('Notes');
    expect(body.cwd).toBeUndefined();
    vi.unstubAllGlobals();
  });

  it('keeps the side composer scoped: chat-selection quotes follow their session', async () => {
    const { quoteCandidateOwnedBySession } = await import('../../stores/selection-actions');
    const sideQuote = quote({ sourceSessionPath: '/session/side.jsonl', sourceMessageId: 'assistant-side' });
    const mainQuote = quote({ sourceSessionPath: '/session/main.jsonl', sourceMessageId: 'assistant-main' });

    // 侧边浮层只认侧边会话的选区，主聊天浮层只认主会话（preview 选区归主聊天页）。
    expect(quoteCandidateOwnedBySession(sideQuote, '/session/side.jsonl', '/session/main.jsonl')).toBe(true);
    expect(quoteCandidateOwnedBySession(sideQuote, null, '/session/main.jsonl')).toBe(false);
    expect(quoteCandidateOwnedBySession(mainQuote, null, '/session/main.jsonl')).toBe(true);
    expect(quoteCandidateOwnedBySession(mainQuote, '/session/side.jsonl', '/session/main.jsonl')).toBe(false);
    expect(quoteCandidateOwnedBySession({ sourceKind: 'preview' }, null, '/session/main.jsonl')).toBe(true);
    expect(quoteCandidateOwnedBySession({ sourceKind: 'preview' }, '/session/side.jsonl', '/session/main.jsonl')).toBe(false);
  });

  it('closes the side chat and drops its identity', () => {
    useStore.getState().openSideChat(quote());
    useStore.getState().setSideChatSession({ sessionPath: '/session/side.jsonl', sessionId: 'sess_side', agentId: 'hana' });
    // 会话本体在会话列表里（创建流程会写入投影；关闭侧栏不撤掉它）。
    useStore.setState({
      sessions: [
        { path: '/session/side.jsonl', sessionId: 'sess_side', agentId: 'hana' },
        ...useStore.getState().sessions,
      ],
    } as never);
    expect(useStore.getState().sideChat.open).toBe(true);

    useStore.getState().closeSideChat();

    const state = useStore.getState();
    expect(state.sideChat.open).toBe(false);
    expect(state.sideChat.sessionPath).toBeNull();
    expect(state.sideChat.status).toBe('idle');
    // 会话本体与记录保留在会话列表中（不做归档/删除）。
    expect(state.sessions.some(session => session.path === '/session/side.jsonl')).toBe(true);
  });
});
