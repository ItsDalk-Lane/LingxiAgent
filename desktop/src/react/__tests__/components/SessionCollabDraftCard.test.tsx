// @vitest-environment jsdom

import '@testing-library/jest-dom/vitest';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SessionCollabDraftCard } from '../../components/chat/SessionCollabDraftCard';
import { AssistantMessage } from '../../components/chat/AssistantMessage';
import { lingxiFetch } from '../../hooks/use-hana-fetch';
import { useStore } from '../../stores';

vi.mock('../../hooks/use-hana-fetch', () => ({
  lingxiFetch: vi.fn(async () => new Response(JSON.stringify({ ok: true, result: null }), { status: 200 })),
  lingxiUrl: (path: string) => `http://127.0.0.1:3210${path}`,
}));

vi.mock('../../utils/screenshot', () => ({
  takeScreenshot: vi.fn(),
}));

const SOURCE_SESSION_PATH = '/sessions/main.jsonl';
const SOURCE_SESSION_ID = 'sid-source-1';

function sendBlock(overrides: Record<string, unknown> = {}) {
  return {
    type: 'suggestion_card',
    kind: 'session_send_draft',
    suggestionId: 'suggestion_send_1',
    status: 'pending',
    title: 'sid-target-1',
    description: 'original message',
    target: { type: 'session', sessionId: 'sid-target-1', sessionTitle: null, agentId: 'lingxi', agentName: 'Hanako' },
    detail: {
      kind: 'session_send_draft',
      draft: { targetSessionId: 'sid-target-1', message: 'original message' },
    },
    actions: [{ id: 'view', kind: 'open' }],
    ...overrides,
  };
}

function createBlock(overrides: Record<string, unknown> = {}) {
  return {
    type: 'suggestion_card',
    kind: 'session_create_draft',
    suggestionId: 'suggestion_create_1',
    status: 'pending',
    title: 'Hanako',
    description: 'first message body',
    target: { type: 'agent', agentId: 'lingxi', agentName: 'Hanako' },
    detail: {
      kind: 'session_create_draft',
      draft: { agentId: 'lingxi', model: 'claude', title: '', firstMessage: 'first message body' },
    },
    actions: [{ id: 'view', kind: 'open' }],
    ...overrides,
  };
}

function renderCard(block: Record<string, unknown>, sessionPath = SOURCE_SESSION_PATH) {
  return render(<SessionCollabDraftCard block={block as any} sessionPath={sessionPath} />);
}

describe('SessionCollabDraftCard', () => {
  beforeEach(() => {
    window.t = ((key: string, params?: Record<string, string>) => {
      if (params) {
        return `${key}:${Object.entries(params).map(([k, v]) => `${k}=${v}`).join(',')}`;
      }
      return key;
    }) as typeof window.t;
    useStore.setState({
      agents: [
        { id: 'lingxi', name: 'Hanako', yuan: 'lingxi', homeFolder: '/home/hanako' },
        { id: 'maomao', name: '毛毛', yuan: 'maomao', homeFolder: '/home/maomao' },
      ],
      agentName: 'Hanako',
      agentYuan: 'lingxi',
      currentAgentId: 'lingxi',
      streamingSessions: [],
      selectedMessageIdsBySession: {},
      // 源 session 的 path→sessionId 映射：走既有 currentSessionPath/currentSessionId
      // 而不是把 sessionPath 当 sessionId 传给 reject。
      currentSessionPath: SOURCE_SESSION_PATH,
      currentSessionId: SOURCE_SESSION_ID,
      sessions: [],
    } as never);
    vi.mocked(lingxiFetch).mockReset();
    vi.mocked(lingxiFetch).mockResolvedValue(new Response(JSON.stringify({ ok: true, result: null }), { status: 200 }));
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it('renders a send draft card with the target agent avatar and the store-resolved session title', () => {
    useStore.setState({
      sessions: [
        { path: '/sessions/target.jsonl', sessionId: 'sid-target-1', title: 'Project kickoff', firstMessage: '', modified: '', messageCount: 0, agentId: 'lingxi', agentName: 'Hanako', cwd: null },
      ],
    } as never);
    renderCard(sendBlock());

    expect(screen.getByAltText('Hanako')).toBeInTheDocument();
    expect(screen.getByText('Project kickoff')).toBeInTheDocument();
    expect(screen.queryByText('sid-target-1')).not.toBeInTheDocument();
    expect(screen.getByDisplayValue('original message')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'sessionCollab.confirmSend' })).toBeInTheDocument();
  });

  it('falls back to agent name + short id tail (never the raw sessionId) when no matching session is in the store', () => {
    renderCard(sendBlock());

    // 完整裸 sessionId 不允许出现在标题区；退化标题是 agentName + 短尾巴（'sid-target-1' 后 4 位 'et-1'）。
    expect(screen.queryByText('sid-target-1')).not.toBeInTheDocument();
    const fallbackTitle = screen.getByText('Hanako …et-1');
    expect(fallbackTitle.textContent).not.toContain('sid-target-1');
  });

  it('submits the edited message to /api/session-collab/apply and shows the sent state on success', async () => {
    renderCard(sendBlock());

    const textarea = screen.getByDisplayValue('original message');
    fireEvent.change(textarea, { target: { value: 'edited' } });
    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.confirmSend' }));

    await waitFor(() => {
      expect(lingxiFetch).toHaveBeenCalledWith('/api/session-collab/apply', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({
          suggestionId: 'suggestion_send_1',
          draft: { targetSessionId: 'sid-target-1', message: 'edited' },
        }),
      }));
    });

    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'sessionCollab.confirmSend' })).not.toBeInTheDocument();
    });
  });

  it('shows the expired message and disables the confirm button on 404 draft_expired', async () => {
    vi.mocked(lingxiFetch).mockResolvedValue(
      new Response(JSON.stringify({ error: 'draft not found', code: 'draft_expired' }), { status: 404 }),
    );
    renderCard(sendBlock());

    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.confirmSend' }));

    await waitFor(() => {
      expect(screen.getByText('sessionCollab.expired')).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: 'sessionCollab.confirmSend' })).toBeDisabled();
  });

  it('shows a retryable error on 500 apply_failed and allows re-submitting', async () => {
    vi.mocked(lingxiFetch).mockResolvedValue(
      new Response(JSON.stringify({ error: 'session_busy', code: 'apply_failed' }), { status: 500 }),
    );
    renderCard(sendBlock());

    const confirmBtn = screen.getByRole('button', { name: 'sessionCollab.confirmSend' });
    fireEvent.click(confirmBtn);

    await waitFor(() => {
      expect(screen.getByText(/sessionCollab\.sendFailed/)).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: 'sessionCollab.confirmSend' })).not.toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.confirmSend' }));
    await waitFor(() => {
      expect(vi.mocked(lingxiFetch).mock.calls.length).toBe(2);
    });
  });

  it('renders a create draft card headerless: agent selector first, no duplicated header row', () => {
    renderCard(createBlock());

    expect(screen.getByDisplayValue('first message body')).toBeInTheDocument();
    // 无头形态：agent 名只出现一次（选择器触发器）；改动前头部行+触发器共两次
    expect(screen.getAllByText('Hanako')).toHaveLength(1);
    expect(screen.getByRole('button', { name: 'sessionCollab.confirmCreate' })).toBeInTheDocument();
  });

  it('shows the half-created message with sessionId on 500 first_message_failed and stays retryable', async () => {
    vi.mocked(lingxiFetch).mockResolvedValue(
      new Response(JSON.stringify({ error: 'first_message_failed: boom', code: 'first_message_failed', sessionId: 'sid-new' }), { status: 500 }),
    );
    renderCard(createBlock());

    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.confirmCreate' }));

    await waitFor(() => {
      expect(screen.getByText(/sessionCollab\.halfCreated/)).toBeInTheDocument();
      expect(screen.getByText(/sid-new/)).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: 'sessionCollab.retryMessage' })).not.toBeDisabled();
  });

  it('posts /api/session-collab/reject on ignore and converges to the rejected state on success', async () => {
    renderCard(sendBlock());

    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.ignore' }));

    await waitFor(() => {
      expect(lingxiFetch).toHaveBeenCalledWith('/api/session-collab/reject', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ suggestionId: 'suggestion_send_1', sourceSessionId: SOURCE_SESSION_ID }),
      }));
    });

    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'sessionCollab.confirmSend' })).not.toBeInTheDocument();
      expect(screen.getByText('common.rejected')).toBeInTheDocument();
    });
  });

  it('treats a 404 reject response (already-expired draft) as converged to rejected', async () => {
    vi.mocked(lingxiFetch).mockResolvedValue(
      new Response(JSON.stringify({ error: 'not found', code: 'draft_expired' }), { status: 404 }),
    );
    renderCard(sendBlock());

    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.ignore' }));

    await waitFor(() => {
      expect(screen.getByText('common.rejected')).toBeInTheDocument();
    });
  });

  it('shows an inFlight message on 409 reject and keeps the card pending (retryable)', async () => {
    vi.mocked(lingxiFetch).mockResolvedValue(
      new Response(JSON.stringify({ error: 'draft is being applied', code: 'draft_in_flight' }), { status: 409 }),
    );
    renderCard(sendBlock());

    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.ignore' }));

    await waitFor(() => {
      expect(screen.getByText('sessionCollab.inFlight')).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: 'sessionCollab.ignore' })).toBeInTheDocument();
  });

  it('shows a retryable error and keeps status pending on 500 reject failure', async () => {
    vi.mocked(lingxiFetch).mockResolvedValue(
      new Response(JSON.stringify({ error: 'store_unavailable' }), { status: 500 }),
    );
    renderCard(sendBlock());

    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.ignore' }));

    await waitFor(() => {
      expect(screen.getByText(/sessionCollab\.rejectFailed/)).toBeInTheDocument();
    });
    // 状态没有翻转：确认按钮还在，忽略按钮可以重试。
    expect(screen.getByRole('button', { name: 'sessionCollab.confirmSend' })).toBeInTheDocument();
    const ignoreBtn = screen.getByRole('button', { name: 'sessionCollab.ignore' });
    expect(ignoreBtn).not.toBeDisabled();

    vi.mocked(lingxiFetch).mockResolvedValue(new Response(JSON.stringify({ ok: true }), { status: 200 }));
    fireEvent.click(ignoreBtn);
    await waitFor(() => {
      expect(vi.mocked(lingxiFetch).mock.calls.length).toBe(2);
    });
  });

  it('renders the converged approved state directly (no confirm button) when block.status starts as approved', () => {
    renderCard(sendBlock({ status: 'approved' }));

    expect(screen.queryByRole('button', { name: 'sessionCollab.confirmSend' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'sessionCollab.ignore' })).not.toBeInTheDocument();
    expect(screen.getByText('common.approved')).toBeInTheDocument();
  });

  it('shows the created session id in the converged approved state when block.resultSessionId is set (history rebuild)', () => {
    renderCard(createBlock({ status: 'approved', resultSessionId: 'sid-rebuilt' }));

    expect(screen.getByText('sessionCollab.createdSession:id=sid-rebuilt')).toBeInTheDocument();
  });

  it('AssistantMessage dispatches session_send_draft suggestion_card blocks to this card, not the automation fallback', () => {
    render(
      <AssistantMessage
        agentDisplay={{ id: 'hana', displayName: 'Hana', avatarUrl: null, fallbackAvatar: null, yuan: 'hana', isUser: false }}
        isStreaming={false}
        isSelected={false}
        showAvatar={false}
        sessionPath="/sessions/main.jsonl"
        message={{
          id: 'assistant-session-collab-1',
          role: 'assistant',
          timestamp: Date.now(),
          blocks: [sendBlock()],
        } as any}
      />,
    );

    // 走到 SessionCollabDraftCard：send 卡的编辑区（textarea + 确认按钮）应该出现。
    expect(screen.getByDisplayValue('original message')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'sessionCollab.confirmSend' })).toBeInTheDocument();
    // 不应该走 CronConfirmBlock 的 automation 通用兜底（那条路径没有 view/open 相关 aria-label）。
    expect(screen.queryByRole('button', { name: 'automation.openDraft' })).not.toBeInTheDocument();
  });
  it('部分创建成功只用同一草稿与同一编辑内容重试首条消息', async () => {
    vi.mocked(lingxiFetch).mockResolvedValueOnce(new Response(JSON.stringify({ ok: false, partialSuccess: true,
      code: 'first_message_failed', sessionId: 'sid-once', decisionPersisted: true,
      result: { sessionId: 'sid-once', sessionCreated: true, firstMessageAccepted: false, deliveryStatus: 'failed', retryable: true },
    }), { status: 500 })).mockResolvedValueOnce(new Response(JSON.stringify({ ok: true, decisionPersisted: true,
      result: { sessionId: 'sid-once', firstMessageAccepted: true, deliveryStatus: 'accepted' },
    })));
    renderCard(createBlock());
    fireEvent.change(screen.getByDisplayValue('first message body'), { target: { value: 'edited first message' } });
    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.confirmCreate' }));
    const retry = await screen.findByRole('button', { name: 'sessionCollab.retryMessage' });
    expect(screen.getByDisplayValue('edited first message')).toBeDisabled();
    fireEvent.click(retry);
    await screen.findByText('sessionCollab.createdSession:id=sid-once');
    const calls = vi.mocked(lingxiFetch).mock.calls;
    expect(calls).toHaveLength(2);
    expect(calls[0][0]).toBe('/api/session-collab/apply');
    expect(calls[1][0]).toBe('/api/session-collab/apply');
    expect(calls[1][1]?.body).toBe(calls[0][1]?.body);
    expect(JSON.parse(String(calls[1][1]?.body))).toMatchObject({ suggestionId: 'suggestion_create_1' });
  });

  it('排队确认不显示为已送达，也不再提供第二次创建按钮', async () => {
    vi.mocked(lingxiFetch).mockResolvedValueOnce(new Response(JSON.stringify({ ok: true, decisionPersisted: true,
      result: { sessionId: 'sid-queued', targetSessionId: 'sid-queued', sessionCreated: true,
        deliveryStatus: 'queued', firstMessageAccepted: false, acceptedRunId: null },
    })));
    renderCard(createBlock());
    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.confirmCreate' }));
    await screen.findByText('sessionCollab.queued');
    expect(screen.getByText('sessionCollab.queuedHint')).toBeInTheDocument();
    expect(screen.queryByText('common.approved')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'sessionCollab.confirmCreate' })).not.toBeInTheDocument();
    expect(lingxiFetch).toHaveBeenCalledTimes(1);
  });

  it('历史排队状态随后可由真实accepted结果更新', () => {
    const block = sendBlock({ status: 'approved', result: { deliveryStatus: 'queued' } });
    const { rerender } = renderCard(block);
    expect(screen.getByText('sessionCollab.queued')).toBeInTheDocument();
    rerender(<SessionCollabDraftCard block={{ ...block, result: { deliveryStatus: 'accepted', firstMessageAccepted: true, acceptedRunId: 'run-real' } }} sessionPath={SOURCE_SESSION_PATH} />);
    expect(screen.getByText('common.approved')).toBeInTheDocument();
    expect(screen.queryByText('sessionCollab.queued')).not.toBeInTheDocument();
  });

  it.each(['apply', 'reject'])('实际%s成功但决定未保存时保留结果并显示警示', async operation => {
    vi.mocked(lingxiFetch).mockResolvedValueOnce(new Response(JSON.stringify({ ok: true, decisionPersisted: false,
      result: { sessionId: 'sid-real' },
    })));
    renderCard(sendBlock());
    fireEvent.click(screen.getByRole('button', { name: operation === 'apply' ? 'sessionCollab.confirmSend' : 'sessionCollab.ignore' }));
    await screen.findByText(operation === 'apply' ? 'common.approved' : 'common.rejected');
    expect(screen.getByRole('alert')).toHaveTextContent('sessionCollab.decisionNotPersisted');
    expect(screen.queryByRole('button', { name: 'sessionCollab.confirmSend' })).not.toBeInTheDocument();
  });

  it('未知投递结果明确显示，禁止再次点击或自动重发', async () => {
    vi.mocked(lingxiFetch).mockResolvedValueOnce(new Response(JSON.stringify({ ok: false, code: 'delivery_unknown',
      result: { sessionId: 'sid-unknown', deliveryStatus: 'unknown', retryable: false },
    }), { status: 409 }));
    renderCard(createBlock());
    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.confirmCreate' }));
    await screen.findByText('sessionCollab.unknown');
    expect(screen.getByText('sessionCollab.deliveryUnknown')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'sessionCollab.retryMessage' })).not.toBeInTheDocument();
    expect(lingxiFetch).toHaveBeenCalledTimes(1);
  });

  it('切换草稿后旧确认响应不能覆盖新草稿', async () => {
    let resolve!: (response: Response) => void;
    vi.mocked(lingxiFetch).mockReturnValueOnce(new Promise(done => { resolve = done; }));
    const { rerender } = renderCard(sendBlock());
    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.confirmSend' }));
    rerender(<SessionCollabDraftCard block={createBlock({ suggestionId: 'new-card' })} sessionPath={SOURCE_SESSION_PATH} />);
    await act(async () => { resolve(new Response(JSON.stringify({ ok: true, decisionPersisted: false }))); });
    expect(screen.getByRole('button', { name: 'sessionCollab.confirmCreate' })).not.toBeDisabled();
    expect(screen.getByDisplayValue('first message body')).toBeInTheDocument();
    expect(screen.queryByText('common.approved')).not.toBeInTheDocument();
    expect(screen.queryByText('sessionCollab.decisionNotPersisted')).not.toBeInTheDocument();
  });

  it('助手切换后旧请求不把当前卡变成已确认', async () => {
    let resolve!: (response: Response) => void;
    vi.mocked(lingxiFetch).mockReturnValueOnce(new Promise(done => { resolve = done; }));
    renderCard(sendBlock());
    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.confirmSend' }));
    act(() => { useStore.setState({ currentAgentId: 'maomao' }); });
    await act(async () => { resolve(new Response(JSON.stringify({ ok: true }))); });
    expect(screen.getByRole('button', { name: 'sessionCollab.confirmSend' })).not.toBeDisabled();
    expect(screen.queryByText('common.approved')).not.toBeInTheDocument();
  });

  it('重开部分成功的历史草稿后仍只重试同一冻结payload', async () => {
    const block = createBlock({ status: 'approved', resultSessionId: 'sid-existing',
      result: { sessionId: 'sid-existing', deliveryStatus: 'failed', retryable: true },
      frozenApply: { agentId: 'maomao', title: 'frozen title', firstMessage: 'frozen first message' },
    });
    renderCard(block);
    expect(screen.getByDisplayValue('frozen title')).toBeDisabled();
    expect(screen.getByDisplayValue('frozen first message')).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'sessionCollab.retryMessage' }));
    await waitFor(() => { expect(lingxiFetch).toHaveBeenCalledTimes(1); });
    expect(JSON.parse(String(vi.mocked(lingxiFetch).mock.calls[0][1]?.body))).toEqual({
      suggestionId: 'suggestion_create_1', draft: { agentId: 'maomao', title: 'frozen title', firstMessage: 'frozen first message' },
    });
  });

});
