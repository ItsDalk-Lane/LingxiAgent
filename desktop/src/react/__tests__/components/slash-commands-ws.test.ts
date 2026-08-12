// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../../stores';
import { executeSlashViaWs } from '../../components/input/slash-commands';

const { sendMock, getWebSocketMock, ensureSessionMock, loadSessionsMock } = vi.hoisted(() => ({
  sendMock: vi.fn(),
  getWebSocketMock: vi.fn(),
  ensureSessionMock: vi.fn(),
  loadSessionsMock: vi.fn(),
}));

vi.mock('../../services/websocket', () => ({
  getWebSocket: getWebSocketMock,
}));

vi.mock('../../stores/session-actions', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  ensureSession: ensureSessionMock,
  loadSessions: loadSessionsMock,
}));

const tMock = (key: string) => key;

describe('executeSlashViaWs', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    getWebSocketMock.mockReturnValue({ readyState: WebSocket.OPEN, send: sendMock });
    useStore.setState({
      currentSessionPath: '/session/a.jsonl',
      pendingNewSession: false,
      pendingDraftId: null,
    } as never);
  });

  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it('carries the agent that owns this input box so the server never has to guess', async () => {
    await executeSlashViaWs(tMock, 'stop', 'agent-a', vi.fn(), vi.fn(), vi.fn())();

    expect(JSON.parse(sendMock.mock.calls[0][0])).toEqual({
      type: 'slash',
      text: '/stop',
      sessionPath: '/session/a.jsonl',
      agentId: 'agent-a',
    });
  });

  it('keeps the typed command line intact and still carries the identity', async () => {
    await executeSlashViaWs(tMock, 'reset', 'agent-b', vi.fn(), vi.fn(), vi.fn())('  /reset hard  ');

    expect(JSON.parse(sendMock.mock.calls[0][0])).toMatchObject({
      text: '/reset hard',
      agentId: 'agent-b',
    });
  });

  it('states an unknown identity explicitly instead of dropping the field', async () => {
    // Silently omitting the field is what made the server fall back to its own
    // lookup and then fail with a raw assertion; an explicit null keeps the
    // contract visible on the wire.
    await executeSlashViaWs(tMock, 'stop', null, vi.fn(), vi.fn(), vi.fn())();

    expect(JSON.parse(sendMock.mock.calls[0][0])).toHaveProperty('agentId', null);
  });

  it('materializes a pending draft session before sending, like the prompt channel does', async () => {
    // 草稿会话没有服务端身份，直接发送会被服务端按 internal_contract 拒掉
    // （用户看到"应用内部出了点问题"）。先 ensureSession 把会话建出来再发。
    useStore.setState({
      currentSessionPath: null,
      pendingNewSession: true,
      pendingDraftId: 'draft-1',
    } as never);
    ensureSessionMock.mockResolvedValue({
      sessionId: 'sess_new',
      sessionPath: '/session/new.jsonl',
      agentId: 'agent-new',
    });

    await executeSlashViaWs(tMock, 'loop', 'agent-draft', vi.fn(), vi.fn(), vi.fn())('/loop 每轮检查一次构建');

    expect(ensureSessionMock).toHaveBeenCalledWith('draft-1');
    expect(loadSessionsMock).toHaveBeenCalledTimes(1);
    expect(JSON.parse(sendMock.mock.calls[0][0])).toEqual({
      type: 'slash',
      text: '/loop 每轮检查一次构建',
      sessionPath: '/session/new.jsonl',
      agentId: 'agent-draft',
    });
  });

  it('falls back to the ensured session owner when the caller has no agent identity', async () => {
    useStore.setState({
      currentSessionPath: null,
      pendingNewSession: true,
      pendingDraftId: 'draft-1',
    } as never);
    ensureSessionMock.mockResolvedValue({
      sessionId: 'sess_new',
      sessionPath: '/session/new.jsonl',
      agentId: 'agent-new',
    });

    await executeSlashViaWs(tMock, 'loop', null, vi.fn(), vi.fn(), vi.fn())();

    expect(JSON.parse(sendMock.mock.calls[0][0])).toHaveProperty('agentId', 'agent-new');
  });

  it('does not send an identity-less slash when session creation fails', async () => {
    // ensureSession 自己已经弹出创建失败的原因并返回 null，这里只负责不发。
    useStore.setState({
      currentSessionPath: null,
      pendingNewSession: true,
      pendingDraftId: 'draft-1',
    } as never);
    ensureSessionMock.mockResolvedValue(null);

    await executeSlashViaWs(tMock, 'loop', 'agent-draft', vi.fn(), vi.fn(), vi.fn())('/loop x');

    expect(sendMock).not.toHaveBeenCalled();
  });

  it('refuses explicitly when there is no session at all instead of tripping the server contract', async () => {
    const toastMock = vi.fn();
    useStore.setState({
      currentSessionPath: null,
      pendingNewSession: false,
      pendingDraftId: null,
      addToast: toastMock,
    } as never);

    await executeSlashViaWs(tMock, 'loop', 'agent-a', vi.fn(), vi.fn(), vi.fn())('/loop x');

    expect(sendMock).not.toHaveBeenCalled();
    expect(toastMock).toHaveBeenCalledWith('error.noActiveSession', 'error', 6000);
  });
});
