// @vitest-environment jsdom

/**
 * R08 UI 叶 F-D01-UI-UI-PAGE-CHAT-81B217 客户端份额缺口补证。
 *
 * 真实渲染生产 `ChatPage`（含真实 store、WelcomeScreen、ChatArea、InputArea；
 * jsdom 缺失的 TipTap 编辑器用最小替身，等价于既有 InputArea 套件的 jsdom 环境
 * 适配，其余业务模块均为生产实现）；网络层只 mock `lingxiFetch` 传输函数。
 * 既有同类断言（prompt/slash、中止/插队、侧边对话/附件/胶囊、模型切换、思考档位）
 * 另绑定各现有专属测试文件，本文件只补其未覆盖的空态/错误态与页面组合投影。
 */

import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';

const transfer = vi.hoisted(() => ({ lingxiFetch: vi.fn(async (_path: string, _opts?: RequestInit) => new Response(JSON.stringify({ ok: true, thinkingLevel: 'medium' }), { status: 200 })) }));

const editorMocks = vi.hoisted(() => ({
  dispatch: vi.fn(),
  setMeta: vi.fn(() => ({})),
}));

vi.mock('@tiptap/react', () => ({
  useEditor: () => ({
    commands: { focus: vi.fn(), clearContent: vi.fn(), scrollIntoView: vi.fn(), setContent: vi.fn(), insertContent: vi.fn() },
    chain: () => ({ clearContent: () => ({ insertContent: () => ({ insertContent: () => ({ focus: () => ({ run: vi.fn() }) }) }) }), focus: () => ({ run: vi.fn() }) }),
    getText: () => '',
    getJSON: () => ({ type: 'doc', content: [] }),
    state: { tr: { setMeta: editorMocks.setMeta } },
    view: { dispatch: editorMocks.dispatch },
    on: vi.fn(),
    off: vi.fn(),
    isDestroyed: false,
  }),
  EditorContent: () => React.createElement('div', { 'data-testid': 'editor' }),
}));
vi.mock('@tiptap/starter-kit', () => ({ default: { configure: () => ({}) } }));
vi.mock('@tiptap/extension-placeholder', () => ({ default: { configure: () => ({}) } }));
vi.mock('../../components/input/extensions/skill-badge', () => ({ SkillBadge: {} }));
vi.mock('../../stores/channel-actions', () => ({ loadChannels: vi.fn(), openChannel: vi.fn() }));
vi.mock('../../stores/preview-actions', () => ({ handleLegacyArtifactBlock: vi.fn() }));
vi.mock('../../services/app-event-actions', () => ({ handleAppEvent: vi.fn() }));
vi.mock('../../services/stream-resume', () => ({
  injectHandlers: vi.fn(),
  injectWebSocketGetter: vi.fn(),
  invalidateSessionStreamMeta: vi.fn(),
  getSessionStreamMeta: () => null,
  requestStreamResume: vi.fn(),
  canApplyIncrementalResume: () => false,
  replayStreamResume: vi.fn(),
  isStreamResumeRebuilding: () => null,
  isStreamScopedMessage: () => false,
  updateSessionStreamMeta: vi.fn(() => true),
}));
vi.mock('../../services/stream-key-dispatcher', () => ({ dispatchStreamKey: vi.fn() }));
vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: (path: string, opts?: RequestInit) => transfer.lingxiFetch(path, opts),
  lingxiUrl: (path: string) => path,
}));

import { ChatPage } from '../../components/app/ChatPage';
import { handleServerMessage } from '../../services/ws-message-handler';
import { RegionalErrorBoundary } from '../../components/RegionalErrorBoundary';
import { ModelSelector } from '../../components/input/ModelSelector';
import { ThinkingLevelButton } from '../../components/input/ThinkingLevelButton';
import { useStore } from '../../stores';
import { sessionScopedListIncludes, sessionScopedValue } from '../../stores/session-slice';
import { createTestTranslator } from '../helpers/i18n-test-strings';
import { createLocalServerConnection } from '../../services/server-connection';

const initialStore = useStore.getState();

function sendButton(): HTMLButtonElement | null {
  return document.querySelector('button[class*="send-btn"]') as HTMLButtonElement | null;
}

class MockResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

function seedConnection(port = 44710) {
  const connection = createLocalServerConnection({
    serverPort: port,
    serverToken: `owned-${port}`,
    serverNodeKind: 'lingxi-service',
    serverNodeTransport: 'http',
  })!;
  useStore.setState({
    activeServerConnectionId: 'local',
    serverConnections: { local: connection },
    activeServerConnection: connection,
  });
}

function seedSessions() {
  useStore.setState({
    sessions: [{
      path: '/sessions/current.jsonl',
      sessionId: 'current',
      agentId: 'hana',
      agentName: 'Hana',
      title: '当前会话',
      firstMessage: '',
      modified: '',
      messageCount: 1,
    }],
    sessionLocatorsById: { current: { path: '/sessions/current.jsonl' } },
    chatSessions: {
      '/sessions/current.jsonl': { items: [], hasMore: false, loadingMore: false, revision: null, nextCursor: null },
    },
    streamingSessions: [],
    turnPendingSessions: [],
    knowledgeRetrievingSessions: [],
    inlineErrors: {},
    toasts: [],
    attachedFiles: [],
    attachedFilesBySession: {},
    models: [],
    sessionModelsByPath: {},
    previewItems: [],
    previewOpen: false,
    currentAgentId: 'hana',
    connected: true,
    modelSwitching: false,
  } as never);
}

beforeEach(() => {
  transfer.lingxiFetch.mockReset();
  transfer.lingxiFetch.mockResolvedValue(new Response(JSON.stringify({ ok: true, thinkingLevel: 'medium' }), { status: 200 }));
  window.t = createTestTranslator() as typeof window.t;
  window.i18n = { locale: 'zh-CN', load: vi.fn(async () => {}), t: window.t } as unknown as typeof window.i18n;
  window.platform = { selectFolder: vi.fn(async () => null) } as unknown as typeof window.platform;
  Object.defineProperty(window, 'ResizeObserver', { configurable: true, value: MockResizeObserver });
  seedConnection();
  seedSessions();
  useStore.setState({
    welcomeVisible: true,
    currentSessionPath: null,
    currentSessionId: null,
    pendingNewSession: false,
  } as never);
});

afterEach(() => {
  cleanup();
  useStore.setState(initialStore, true);
});

describe('R08 聊天页：欢迎态/消息流/输入区（断言 1）', () => {
  it('无会话显示欢迎页与输入框；选中当前会话后欢迎页隐藏、消息区与输入区并存', async () => {
    render(<ChatPage />);
    await waitFor(() => expect(sendButton()).not.toBeNull());
    const welcome = document.querySelector('#welcome');
    expect(welcome).not.toBeNull();
    expect((welcome as HTMLElement).classList.contains('hidden')).toBe(false);
    expect(document.querySelector('.chat-area')).not.toBeNull();

    act(() => {
      useStore.setState({
        welcomeVisible: false,
        currentSessionId: 'current',
        currentSessionPath: '/sessions/current.jsonl',
      } as never);
    });

    await waitFor(() => expect((document.querySelector('#welcome') as HTMLElement).classList.contains('hidden')).toBe(true));
    expect(document.querySelector('.chat-area')?.classList.contains('has-panels')).toBe(true);
    // 当前会话投影与欢迎态互斥；输入区仍在同一页面内。
    expect(sendButton()).not.toBeNull();
  }, 20000);

  it('mobile 或服务未连接时不请求该会话思考档位 GET', async () => {
    const thinkingGets = () => transfer.lingxiFetch.mock.calls.filter(([path]) => String(path).startsWith('/api/session-thinking-level')).length;
    transfer.lingxiFetch.mockClear();
    render(<ChatPage inputSurface="mobile" />);
    await waitFor(() => expect(sendButton()).not.toBeNull());
    await act(async () => { await Promise.resolve(); });
    expect(thinkingGets()).toBe(0);

    cleanup();
    useStore.setState({ connected: false } as never);
    transfer.lingxiFetch.mockClear();
    render(<ChatPage inputSurface="desktop" />);
    await waitFor(() => expect(sendButton()).not.toBeNull());
    await act(async () => { await Promise.resolve(); });
    expect(thinkingGets()).toBe(0);
  }, 20000);

  it('区域渲染失败由 RegionalErrorBoundary 捕获并显示重试，不冒充消息成功', async () => {
    function Bomb(): React.ReactElement {
      throw new Error('region projection failed');
    }
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      render(
        <RegionalErrorBoundary region="r08-chat">
          <Bomb />
        </RegionalErrorBoundary>,
      );
      const retry = await screen.findByRole('button', { name: '重试' });
      expect(retry).toBeTruthy();
      expect(screen.queryByText('message-success')).toBeNull();
    } finally {
      quiet.mockRestore();
    }
  });
});

describe('R08 聊天页：运行状态控件（断言 3）', () => {
  it('运行信息控件随真实运行态切换：无运行任务只有发送，会话在跑显示中止', async () => {
    act(() => {
      useStore.setState({
        welcomeVisible: false,
        currentSessionId: 'current',
        currentSessionPath: '/sessions/current.jsonl',
        streamingSessions: [],
      } as never);
    });
    render(<ChatPage />);
    await waitFor(() => expect(sendButton()).not.toBeNull());
    expect(sendButton()!.textContent).toContain('chat.send');

    act(() => {
      useStore.setState({ streamingSessions: ['/sessions/current.jsonl'] } as never);
    });
    await waitFor(() => expect(sendButton()!.textContent).toContain('chat.stop'));

    act(() => {
      useStore.setState({ streamingSessions: [] } as never);
    });
    await waitFor(() => expect(sendButton()!.textContent).toContain('chat.send'));
  }, 20000);
});

describe('R08 聊天页：模型切换空态（断言 5）', () => {
  it('已建会话选择相同且可用模型直接返回、不发 switch 请求；无可选模型不产生有效切换', async () => {
    const model = { id: 'deepseek-v4-flash', name: 'DeepSeek V4 Flash', provider: 'deepseek' };
    useStore.setState({
      currentSessionPath: '/sessions/current.jsonl',
      currentSessionId: 'current',
      chatSessions: { '/sessions/current.jsonl': { items: [{ type: 'message' }], hasMore: false, loadingMore: false } },
      sessionModelsByPath: { '/sessions/current.jsonl': model },
      models: [model],
      pendingNewSession: false,
    } as never);
    transfer.lingxiFetch.mockClear();

    const { container } = render(<ModelSelector models={[model]} sessionModel={model} />);
    fireEvent.click(screen.getByRole('button', { name: /DeepSeek V4 Flash/ }));
    const option = await screen.findByRole('option', { name: /DeepSeek V4 Flash/ });
    fireEvent.click(option);
    await act(async () => { await Promise.resolve(); });
    expect(transfer.lingxiFetch.mock.calls.filter(([path]) => String(path) === '/api/models/switch')).toHaveLength(0);
    expect(container.textContent).toContain('DeepSeek V4 Flash');

    cleanup();
    transfer.lingxiFetch.mockClear();
    render(<ModelSelector models={[]} />);
    await act(async () => { await Promise.resolve(); });
    expect(transfer.lingxiFetch.mock.calls.filter(([path]) => String(path) === '/api/models/switch' || String(path) === '/api/models/set')).toHaveLength(0);
  }, 20000);
});

describe('R08 聊天页：思考档位空态/错误态（断言 6）', () => {
  it('未建立会话时只改默认档位（请求不带 sessionPath）；保存失败仅 console.error，不宣称已持久化', async () => {
    const quiet = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      useStore.setState({ currentSessionPath: null, pendingNewSession: true } as never);
      transfer.lingxiFetch.mockResolvedValueOnce(new Response(JSON.stringify({ ok: true, thinkingLevel: 'high' }), { status: 200 }));
      const onChange = vi.fn();
      const first = render(<ThinkingLevelButton level="medium" onChange={onChange} availableLevels={['off', 'medium', 'high']} />);
      fireEvent.click(screen.getByRole('button', { name: /中等/ }));
      fireEvent.click(await screen.findByRole('option', { name: /深度/ }));
      await waitFor(() => expect(onChange).toHaveBeenCalledWith('high'));
      const body = JSON.parse(String(transfer.lingxiFetch.mock.calls.at(-1)![1]?.body));
      expect(body).toEqual({ level: 'high' });
      first.unmount();

      // 已建会话：请求带 sessionPath；保存失败只记录错误，不触发 onChange 宣称成功。
      cleanup();
      useStore.setState({
        currentSessionPath: '/sessions/current.jsonl',
        currentSessionId: 'current',
        pendingNewSession: false,
      } as never);
      transfer.lingxiFetch.mockClear();
      transfer.lingxiFetch.mockRejectedValueOnce(new Error('思考档位写入失败'));
      const failedOnChange = vi.fn();
      render(<ThinkingLevelButton level="medium" onChange={failedOnChange} availableLevels={['off', 'medium', 'high']} />);
      fireEvent.click(screen.getByRole('button', { name: /中等/ }));
      fireEvent.click(await screen.findByRole('option', { name: /深度/ }));
      await waitFor(() => expect(quiet).toHaveBeenCalledWith('[thinking-level] save failed:', expect.any(Error)));
      const failBody = JSON.parse(String(transfer.lingxiFetch.mock.calls.at(-1)![1]?.body));
      // 聊天协议把界面 sessionPath 翻译成真实会话 id（chat-client 的既有映射）。
      expect(failBody).toEqual({ sessionPath: 'current', level: 'high' });
      expect(failedOnChange).not.toHaveBeenCalled();
    } finally {
      quiet.mockRestore();
    }
  }, 20000);
});


describe('R08 聊天页：WS 拒绝/断线与恢复流状态（补充断言 2/3 未覆盖子句）', () => {
  it('WS 拒绝/断线事件在页面内显式呈现错误，不冒充消息成功', async () => {
    act(() => {
      useStore.setState({
        welcomeVisible: false,
        currentSessionId: 'current',
        currentSessionPath: '/sessions/current.jsonl',
        streamingSessions: [],
        inlineErrors: {},
      } as never);
    });
    render(<ChatPage />);
    await waitFor(() => expect(sendButton()).not.toBeNull());

    act(() => {
      handleServerMessage({
        type: 'error',
        sessionPath: '/sessions/current.jsonl',
        message: '连接已断开，消息未发送',
        code: 'ws_disconnected',
      });
    });

    await waitFor(() => {
      const state = useStore.getState();
      const inline = sessionScopedValue(state, state.inlineErrors, '/sessions/current.jsonl');
      expect(inline).toBeTruthy();
      const inlineText = (inline as any)?.text ?? '';
      expect(inlineText).toContain('连接已断开');
    });
    // 页面内（输入区上方错误条）可见同一错误，不把失败装成成功/空数据。
    await waitFor(() => expect(document.body.textContent).toContain('连接已断开，消息未发送'));
    expect(sendButton()).not.toBeNull();
  }, 20000);

  it('恢复流的运行状态经真实 ws 分发后与输入区控件态一致', async () => {
    act(() => {
      useStore.setState({
        welcomeVisible: false,
        currentSessionId: 'current',
        currentSessionPath: '/sessions/current.jsonl',
        streamingSessions: [],
        activeSessionStreams: {},
      } as never);
    });
    render(<ChatPage />);
    await waitFor(() => expect(sendButton()!.textContent).toContain('chat.send'));

    act(() => {
      handleServerMessage({
        type: 'status',
        sessionPath: '/sessions/current.jsonl',
        isStreaming: true,
        streamId: 'stream-r08-resume',
        turnId: 'turn-r08-resume',
      });
    });
    await waitFor(() => expect(sessionScopedListIncludes(useStore.getState(), useStore.getState().streamingSessions, '/sessions/current.jsonl')).toBe(true));
    await waitFor(() => expect(sendButton()!.textContent).toContain('chat.stop'));

    act(() => {
      handleServerMessage({
        type: 'status',
        sessionPath: '/sessions/current.jsonl',
        isStreaming: false,
        streamId: 'stream-r08-resume',
        turnId: 'turn-r08-resume',
      });
    });
    await waitFor(() => expect(sessionScopedListIncludes(useStore.getState(), useStore.getState().streamingSessions, '/sessions/current.jsonl')).toBe(false));
    await waitFor(() => expect(sendButton()!.textContent).toContain('chat.send'));
  }, 20000);
});
